//! Archetype definitions and storage.
//!
//! An archetype is a unique combination of component types. Entities sharing
//! the same set of components are grouped into the same archetype for
//! cache-friendly iteration and efficient shard distribution.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::component::ComponentTypeId;
use crate::entity::Entity;

/// A unique identifier for an archetype, computed from its sorted set of
/// [`ComponentTypeId`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArchetypeId(pub u64);

impl ArchetypeId {
    /// Compute the archetype ID from a set of component type IDs.
    ///
    /// The result is deterministic: the same set of types always produces the
    /// same archetype ID regardless of insertion order.
    #[must_use]
    pub fn from_component_types(types: &BTreeSet<ComponentTypeId>) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for ty in types {
            ty.hash(&mut hasher);
        }
        Self(hasher.finish())
    }
}

/// A column in an archetype table, storing components of a single type.
///
/// Components are stored as raw bytes for type-erased access. Each element is
/// `item_size` bytes, laid out contiguously.
#[derive(Debug, Clone)]
pub struct Column {
    /// The component type stored in this column.
    pub type_id: ComponentTypeId,
    /// Size of a single component instance in bytes.
    pub item_size: usize,
    /// Raw byte storage. Length is always `item_size * entity_count`.
    pub data: Vec<u8>,
}

impl Column {
    /// Create a new empty column for the given component type.
    #[must_use]
    pub fn new(type_id: ComponentTypeId, item_size: usize) -> Self {
        Self {
            type_id,
            item_size,
            data: Vec::new(),
        }
    }

    /// Returns the number of component instances stored.
    #[must_use]
    pub fn len(&self) -> usize {
        if self.item_size == 0 {
            return 0;
        }
        self.data.len() / self.item_size
    }

    /// Returns `true` if this column contains no components.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Push a component's raw bytes into the column.
    pub fn push_raw(&mut self, bytes: &[u8]) {
        assert_eq!(
            bytes.len(),
            self.item_size,
            "byte slice size mismatch: expected {}, got {}",
            self.item_size,
            bytes.len()
        );
        self.data.extend_from_slice(bytes);
    }

    /// Get a reference to the raw bytes of the component at `index`.
    #[must_use]
    pub fn get_raw(&self, index: usize) -> Option<&[u8]> {
        let start = index * self.item_size;
        let end = start + self.item_size;
        if end > self.data.len() {
            return None;
        }
        Some(&self.data[start..end])
    }

    /// Get a mutable reference to the raw bytes of the component at `index`.
    #[must_use]
    pub fn get_raw_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        let start = index * self.item_size;
        let end = start + self.item_size;
        if end > self.data.len() {
            return None;
        }
        Some(&mut self.data[start..end])
    }

    /// Push a typed component value into the column.
    ///
    /// # Safety
    ///
    /// The caller must ensure `T` matches the component type stored in this
    /// column (same size and alignment).
    pub unsafe fn push<T: Sized>(&mut self, value: T) {
        assert_eq!(std::mem::size_of::<T>(), self.item_size);
        let bytes =
            // SAFETY: We read `size_of::<T>()` bytes from a valid `T` value.
            unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, self.item_size) };
        self.data.extend_from_slice(bytes);
        std::mem::forget(value);
    }

    /// Get a typed reference to the component at `index`.
    ///
    /// # Safety
    ///
    /// The caller must ensure `T` matches the component type stored in this
    /// column.
    #[must_use]
    pub unsafe fn get<T: Sized>(&self, index: usize) -> Option<&T> {
        let bytes = self.get_raw(index)?;
        // SAFETY: Caller guarantees type match.
        Some(unsafe { &*(bytes.as_ptr() as *const T) })
    }

    /// Get a typed mutable reference to the component at `index`.
    ///
    /// # Safety
    ///
    /// The caller must ensure `T` matches the component type stored in this
    /// column.
    #[must_use]
    pub unsafe fn get_mut<T: Sized>(&mut self, index: usize) -> Option<&mut T> {
        let bytes = self.get_raw_mut(index)?;
        // SAFETY: Caller guarantees type match.
        Some(unsafe { &mut *(bytes.as_mut_ptr() as *mut T) })
    }
}

/// A table of entities sharing the same archetype (set of component types).
///
/// Data is stored in struct-of-arrays (SoA) layout: one [`Column`] per
/// component type, with entity IDs stored in a parallel vector.
#[derive(Debug, Clone)]
pub struct ArchetypeTable {
    /// The archetype identifier.
    pub id: ArchetypeId,
    /// Sorted set of component type IDs that define this archetype.
    pub component_types: BTreeSet<ComponentTypeId>,
    /// Entity IDs in insertion order. `entities[i]` corresponds to row `i`
    /// in every column.
    pub entities: Vec<Entity>,
    /// One column per component type, in the same order as `component_types`.
    pub columns: Vec<Column>,
}

impl ArchetypeTable {
    /// Create a new, empty archetype table.
    #[must_use]
    pub fn new(component_types: BTreeSet<ComponentTypeId>, item_sizes: &[usize]) -> Self {
        let id = ArchetypeId::from_component_types(&component_types);
        let columns = component_types
            .iter()
            .zip(item_sizes.iter())
            .map(|(&type_id, &size)| Column::new(type_id, size))
            .collect();

        Self {
            id,
            component_types,
            entities: Vec::new(),
            columns,
        }
    }

    /// Returns the number of entities in this archetype table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Returns `true` if this table has no entities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Returns `true` if this archetype contains the given component type.
    #[must_use]
    pub fn has_component(&self, type_id: ComponentTypeId) -> bool {
        self.component_types.contains(&type_id)
    }

    /// Returns the column index for the given component type, if present.
    #[must_use]
    pub fn column_index(&self, type_id: ComponentTypeId) -> Option<usize> {
        self.component_types.iter().position(|&tid| tid == type_id)
    }

    /// Find the row index for a given entity.
    #[must_use]
    pub fn entity_row(&self, entity: Entity) -> Option<usize> {
        self.entities.iter().position(|&e| e == entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_types() -> BTreeSet<ComponentTypeId> {
        let mut set = BTreeSet::new();
        set.insert(ComponentTypeId(1));
        set.insert(ComponentTypeId(2));
        set
    }

    #[test]
    fn test_archetype_id_deterministic() {
        let types = make_types();
        let id1 = ArchetypeId::from_component_types(&types);
        let id2 = ArchetypeId::from_component_types(&types);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_archetype_id_order_independent() {
        let mut set1 = BTreeSet::new();
        set1.insert(ComponentTypeId(1));
        set1.insert(ComponentTypeId(2));

        let mut set2 = BTreeSet::new();
        set2.insert(ComponentTypeId(2));
        set2.insert(ComponentTypeId(1));

        assert_eq!(
            ArchetypeId::from_component_types(&set1),
            ArchetypeId::from_component_types(&set2)
        );
    }

    #[test]
    fn test_column_push_and_get() {
        let mut col = Column::new(ComponentTypeId(1), std::mem::size_of::<f32>());
        let val: f32 = 3.14;
        // SAFETY: Column type matches f32.
        unsafe { col.push(val) };
        assert_eq!(col.len(), 1);
        let got = unsafe { col.get::<f32>(0) }.unwrap();
        assert!((got - 3.14).abs() < f32::EPSILON);
    }

    #[test]
    fn test_archetype_table_creation() {
        let types = make_types();
        let table = ArchetypeTable::new(types.clone(), &[4, 8]);
        assert_eq!(table.component_types, types);
        assert!(table.is_empty());
        assert_eq!(table.columns.len(), 2);
    }
}
