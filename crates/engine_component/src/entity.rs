//! Entity type and allocation utilities.
//!
//! An [`Entity`] is a lightweight `u64` identifier with no inherent data.
//! All entity IDs are allocated by the coordinator to ensure global uniqueness.

use serde::{Deserialize, Serialize};

/// A unique entity identifier.
///
/// Entities are pure identifiers — they carry no data of their own. Components
/// are attached to entities to give them meaning.
///
/// Entity IDs are allocated by the coordinator and are guaranteed to be unique
/// across the entire distributed system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Entity(pub u64);

impl Entity {
    /// The null / invalid entity sentinel.
    pub const INVALID: Entity = Entity(0);

    /// Create an entity from a raw `u64` identifier.
    #[must_use]
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw `u64` identifier.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.0
    }

    /// Returns `true` if this is a valid (non-zero) entity.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Entity({})", self.0)
    }
}

/// Allocates monotonically increasing entity IDs.
///
/// This allocator lives in the coordinator and is the single source of truth
/// for entity identity. A free-list for recycling destroyed entity IDs can be
/// added later.
#[derive(Debug)]
pub struct EntityAllocator {
    next_id: u64,
}

impl EntityAllocator {
    /// Creates a new allocator. IDs start at 1 (0 is reserved for [`Entity::INVALID`]).
    #[must_use]
    pub fn new() -> Self {
        Self { next_id: 1 }
    }

    /// Allocates a fresh entity ID.
    pub fn allocate(&mut self) -> Entity {
        let id = self.next_id;
        self.next_id += 1;
        Entity(id)
    }

    /// Returns the number of entities allocated so far.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.next_id - 1
    }
}

impl Default for EntityAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_creation() {
        let e = Entity::from_raw(42);
        assert_eq!(e.id(), 42);
        assert!(e.is_valid());
    }

    #[test]
    fn test_entity_invalid() {
        assert!(!Entity::INVALID.is_valid());
        assert_eq!(Entity::INVALID.id(), 0);
    }

    #[test]
    fn test_allocator_produces_unique_ids() {
        let mut alloc = EntityAllocator::new();
        let e1 = alloc.allocate();
        let e2 = alloc.allocate();
        let e3 = alloc.allocate();
        assert_eq!(e1.id(), 1);
        assert_eq!(e2.id(), 2);
        assert_eq!(e3.id(), 3);
        assert_eq!(alloc.count(), 3);
    }

    #[test]
    fn test_entity_serialization_roundtrip() {
        let entity = Entity::from_raw(999);
        let bytes = rmp_serde::to_vec(&entity).unwrap();
        let restored: Entity = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(entity, restored);
    }
}
