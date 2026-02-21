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
/// Components are stored as serialised byte blobs (one `Vec<u8>` per entity).
/// This supports variable-length encodings such as MessagePack.
#[derive(Debug, Clone)]
pub struct Column {
    /// The component type stored in this column.
    pub type_id: ComponentTypeId,
    /// Per-entity byte blobs, parallel with `ArchetypeTable::entities`.
    pub data: Vec<Vec<u8>>,
}

impl Column {
    /// Create a new empty column for the given component type.
    #[must_use]
    pub fn new(type_id: ComponentTypeId) -> Self {
        Self {
            type_id,
            data: Vec::new(),
        }
    }

    /// Returns the number of component instances stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if this column contains no components.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Push a component's serialised bytes into the column.
    pub fn push_raw(&mut self, bytes: &[u8]) {
        self.data.push(bytes.to_vec());
    }

    /// Get a reference to the serialised bytes of the component at `index`.
    #[must_use]
    pub fn get_raw(&self, index: usize) -> Option<&[u8]> {
        self.data.get(index).map(|v| v.as_slice())
    }

    /// Get a mutable reference to the serialised bytes of the component at
    /// `index`.
    #[must_use]
    pub fn get_raw_mut(&mut self, index: usize) -> Option<&mut Vec<u8>> {
        self.data.get_mut(index)
    }

    /// Remove the element at `index` by swap-removing (O(1)).
    pub fn swap_remove(&mut self, index: usize) {
        self.data.swap_remove(index);
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
    pub fn new(component_types: BTreeSet<ComponentTypeId>) -> Self {
        let id = ArchetypeId::from_component_types(&component_types);
        let columns = component_types
            .iter()
            .map(|&type_id| Column::new(type_id))
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
        let mut col = Column::new(ComponentTypeId(1));
        let val: f32 = 3.14;
        let bytes = val.to_le_bytes();
        col.push_raw(&bytes);
        assert_eq!(col.len(), 1);
        let got = col.get_raw(0).unwrap();
        let restored = f32::from_le_bytes(got.try_into().unwrap());
        assert!((restored - 3.14).abs() < f32::EPSILON);
    }

    #[test]
    fn test_archetype_table_creation() {
        let types = make_types();
        let table = ArchetypeTable::new(types.clone());
        assert_eq!(table.component_types, types);
        assert!(table.is_empty());
        assert_eq!(table.columns.len(), 2);
    }
}
