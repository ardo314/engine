//! World state storage for the coordinator.
//!
//! The [`World`] holds the canonical entity and archetype data. It is the
//! single source of truth in the distributed ECS.

// Many public APIs are not yet called from main() but are exercised by tests
// and will be used once the full NATS tick loop is wired up.
#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};

use engine_component::{ArchetypeId, ArchetypeTable, ComponentTypeId, Entity, EntityAllocator};

/// The canonical world state managed by the coordinator.
///
/// Contains entity allocation, archetype storage, and entity-to-archetype
/// mapping.
#[derive(Debug)]
pub struct World {
    /// Entity ID allocator.
    allocator: EntityAllocator,
    /// All archetype tables, keyed by archetype ID.
    archetypes: HashMap<ArchetypeId, ArchetypeTable>,
    /// Maps each entity to the archetype it belongs to.
    entity_archetype: HashMap<Entity, ArchetypeId>,
    /// Maps component type sets to archetype IDs, for fast lookup.
    type_set_to_archetype: HashMap<BTreeSet<ComponentTypeId>, ArchetypeId>,
}

impl World {
    /// Create a new empty world.
    #[must_use]
    pub fn new() -> Self {
        Self {
            allocator: EntityAllocator::new(),
            archetypes: HashMap::new(),
            entity_archetype: HashMap::new(),
            type_set_to_archetype: HashMap::new(),
        }
    }

    /// Allocate a new entity without any components.
    ///
    /// The entity exists but has no archetype until components are added.
    pub fn spawn_empty(&mut self) -> Entity {
        self.allocator.allocate()
    }

    /// Allocate a new entity and place it into an archetype defined by the
    /// given component types.
    ///
    /// The caller must separately write the component data into the returned
    /// archetype table.
    pub fn spawn(
        &mut self,
        component_types: BTreeSet<ComponentTypeId>,
        item_sizes: &[usize],
    ) -> Entity {
        let entity = self.allocator.allocate();

        let archetype_id = self.get_or_create_archetype(component_types.clone(), item_sizes);

        // Add the entity to the archetype table's entity list.
        if let Some(table) = self.archetypes.get_mut(&archetype_id) {
            table.entities.push(entity);
        }

        self.entity_archetype.insert(entity, archetype_id);
        entity
    }

    /// Allocate a new entity and write serialised component data into its
    /// archetype.
    ///
    /// `component_types`, `component_data`, and `component_sizes` must be
    /// parallel slices — one entry per component. The component data is raw
    /// MessagePack bytes that get written directly into the archetype columns.
    ///
    /// Returns the new entity, or `None` if the slices are mismatched.
    pub fn spawn_with_data(
        &mut self,
        component_types: &[ComponentTypeId],
        component_data: &[Vec<u8>],
        component_sizes: &[usize],
    ) -> Option<Entity> {
        if component_types.len() != component_data.len()
            || component_types.len() != component_sizes.len()
        {
            return None;
        }

        let type_set: BTreeSet<ComponentTypeId> = component_types.iter().copied().collect();
        let entity = self.allocator.allocate();
        let archetype_id = self.get_or_create_archetype(type_set, component_sizes);

        if let Some(table) = self.archetypes.get_mut(&archetype_id) {
            table.entities.push(entity);

            // Write each component's data into the matching column.
            for (ty, data) in component_types.iter().zip(component_data.iter()) {
                if let Some(col_idx) = table.column_index(*ty) {
                    table.columns[col_idx].push_raw(data);
                }
            }
        }

        self.entity_archetype.insert(entity, archetype_id);
        Some(entity)
    }

    /// Destroy an entity, removing it from its archetype.
    ///
    /// Returns `true` if the entity existed and was removed.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if let Some(archetype_id) = self.entity_archetype.remove(&entity) {
            if let Some(table) = self.archetypes.get_mut(&archetype_id)
                && let Some(pos) = table.entities.iter().position(|&e| e == entity)
            {
                table.entities.swap_remove(pos);
                // Also swap-remove from each column.
                for col in &mut table.columns {
                    if col.len() > pos {
                        let last = col.len() - 1;
                        if pos != last {
                            let item_size = col.item_size;
                            let last_start = last * item_size;
                            let pos_start = pos * item_size;
                            // Copy last element into the removed position.
                            for i in 0..item_size {
                                col.data[pos_start + i] = col.data[last_start + i];
                            }
                        }
                        col.data.truncate(last * col.item_size);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Get or create an archetype for the given set of component types.
    fn get_or_create_archetype(
        &mut self,
        component_types: BTreeSet<ComponentTypeId>,
        item_sizes: &[usize],
    ) -> ArchetypeId {
        if let Some(&id) = self.type_set_to_archetype.get(&component_types) {
            return id;
        }

        let table = ArchetypeTable::new(component_types.clone(), item_sizes);
        let id = table.id;
        self.archetypes.insert(id, table);
        self.type_set_to_archetype.insert(component_types, id);
        id
    }

    /// Returns a reference to an archetype table by ID.
    #[must_use]
    pub fn archetype(&self, id: ArchetypeId) -> Option<&ArchetypeTable> {
        self.archetypes.get(&id)
    }

    /// Returns a mutable reference to an archetype table by ID.
    #[must_use]
    pub fn archetype_mut(&mut self, id: ArchetypeId) -> Option<&mut ArchetypeTable> {
        self.archetypes.get_mut(&id)
    }

    /// Returns the archetype ID for a given entity.
    #[must_use]
    pub fn entity_archetype(&self, entity: Entity) -> Option<ArchetypeId> {
        self.entity_archetype.get(&entity).copied()
    }

    /// Returns an iterator over all archetype tables.
    pub fn archetypes(&self) -> impl Iterator<Item = &ArchetypeTable> {
        self.archetypes.values()
    }

    /// Returns the total number of entities in the world.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entity_archetype.len()
    }

    /// Returns the number of archetypes in the world.
    #[must_use]
    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    /// Find all archetypes that contain ALL of the given required component types.
    #[must_use]
    pub fn matching_archetypes(&self, required: &[ComponentTypeId]) -> Vec<ArchetypeId> {
        self.archetypes
            .values()
            .filter(|table| required.iter().all(|ty| table.has_component(*ty)))
            .map(|table| table.id)
            .collect()
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_empty() {
        let mut world = World::new();
        let e = world.spawn_empty();
        assert!(e.is_valid());
        assert_eq!(world.entity_count(), 0); // spawn_empty doesn't add to archetype
    }

    #[test]
    fn test_spawn_with_components() {
        let mut world = World::new();
        let mut types = BTreeSet::new();
        types.insert(ComponentTypeId(1));
        types.insert(ComponentTypeId(2));

        let e = world.spawn(types, &[4, 8]);
        assert!(e.is_valid());
        assert_eq!(world.entity_count(), 1);
        assert_eq!(world.archetype_count(), 1);
    }

    #[test]
    fn test_despawn() {
        let mut world = World::new();
        let mut types = BTreeSet::new();
        types.insert(ComponentTypeId(1));

        let e = world.spawn(types, &[4]);
        assert!(world.despawn(e));
        assert_eq!(world.entity_count(), 0);
    }

    #[test]
    fn test_matching_archetypes() {
        let mut world = World::new();
        let mut types_ab = BTreeSet::new();
        types_ab.insert(ComponentTypeId(1));
        types_ab.insert(ComponentTypeId(2));

        let mut types_a = BTreeSet::new();
        types_a.insert(ComponentTypeId(1));

        let _ = world.spawn(types_ab, &[4, 4]);
        let _ = world.spawn(types_a, &[4]);

        // Query for type 1 — both archetypes match.
        let matches = world.matching_archetypes(&[ComponentTypeId(1)]);
        assert_eq!(matches.len(), 2);

        // Query for types 1 and 2 — only the first archetype matches.
        let matches = world.matching_archetypes(&[ComponentTypeId(1), ComponentTypeId(2)]);
        assert_eq!(matches.len(), 1);
    }
}
