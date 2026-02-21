//! Query descriptors for system data access declarations.
//!
//! A [`QueryDescriptor`] declares which component types a system reads and
//! writes. The coordinator uses these descriptors to match archetypes, detect
//! conflicts between systems, and schedule execution stages.

use serde::{Deserialize, Serialize};

use crate::component::ComponentTypeId;

/// Describes the data access requirements of a system.
///
/// Systems declare their queries at registration time. The coordinator uses
/// this information to:
///
/// 1. Select matching archetypes (those that contain the required components).
/// 2. Detect read/write conflicts between systems for stage scheduling.
/// 3. Determine which component columns to ship to system instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDescriptor {
    /// Component types the system reads immutably.
    pub reads: Vec<ComponentTypeId>,
    /// Component types the system writes (mutable access).
    pub writes: Vec<ComponentTypeId>,
    /// Component types that are optional — the system can handle entities
    /// that do or do not have these components.
    pub optionals: Vec<ComponentTypeId>,
    /// Filters applied to the query (e.g. `With<T>`, `Without<T>`, `Changed<T>`).
    pub filters: Vec<QueryFilter>,
}

impl QueryDescriptor {
    /// Create a new empty query descriptor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reads: Vec::new(),
            writes: Vec::new(),
            optionals: Vec::new(),
            filters: Vec::new(),
        }
    }

    /// Add a read-only component requirement.
    #[must_use]
    pub fn read(mut self, type_id: ComponentTypeId) -> Self {
        self.reads.push(type_id);
        self
    }

    /// Add a mutable component requirement.
    #[must_use]
    pub fn write(mut self, type_id: ComponentTypeId) -> Self {
        self.writes.push(type_id);
        self
    }

    /// Add an optional component.
    #[must_use]
    pub fn optional(mut self, type_id: ComponentTypeId) -> Self {
        self.optionals.push(type_id);
        self
    }

    /// Add a query filter.
    #[must_use]
    pub fn filter(mut self, f: QueryFilter) -> Self {
        self.filters.push(f);
        self
    }

    /// Returns all component types that this query accesses (reads + writes + optionals).
    #[must_use]
    pub fn all_accessed_types(&self) -> Vec<ComponentTypeId> {
        let mut types = Vec::new();
        types.extend_from_slice(&self.reads);
        types.extend_from_slice(&self.writes);
        types.extend_from_slice(&self.optionals);
        types
    }

    /// Returns the set of required component types (reads + writes, excluding optionals).
    #[must_use]
    pub fn required_types(&self) -> Vec<ComponentTypeId> {
        let mut types = Vec::new();
        types.extend_from_slice(&self.reads);
        types.extend_from_slice(&self.writes);
        types
    }

    /// Checks whether this query conflicts with another.
    ///
    /// Two queries conflict when one writes a component type that the other
    /// reads or writes:
    ///
    /// ```text
    /// A.writes ∩ (B.reads ∪ B.writes) ≠ ∅  OR
    /// B.writes ∩ (A.reads ∪ A.writes) ≠ ∅
    /// ```
    #[must_use]
    pub fn conflicts_with(&self, other: &QueryDescriptor) -> bool {
        // Check if any of our writes overlap with their reads or writes.
        for w in &self.writes {
            if other.reads.contains(w) || other.writes.contains(w) {
                return true;
            }
        }
        // Check if any of their writes overlap with our reads or writes.
        for w in &other.writes {
            if self.reads.contains(w) || self.writes.contains(w) {
                return true;
            }
        }
        false
    }
}

impl Default for QueryDescriptor {
    fn default() -> Self {
        Self::new()
    }
}

/// A filter that narrows the set of entities matched by a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryFilter {
    /// Only match entities that have this component.
    With(ComponentTypeId),
    /// Only match entities that do NOT have this component.
    Without(ComponentTypeId),
    /// Only match entities where this component has changed since the last tick.
    Changed(ComponentTypeId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_conflict_both_read() {
        let transform = ComponentTypeId(1);

        let q1 = QueryDescriptor::new().read(transform);
        let q2 = QueryDescriptor::new().read(transform);

        assert!(!q1.conflicts_with(&q2));
    }

    #[test]
    fn test_conflict_read_vs_write() {
        let transform = ComponentTypeId(1);

        let q1 = QueryDescriptor::new().read(transform);
        let q2 = QueryDescriptor::new().write(transform);

        assert!(q1.conflicts_with(&q2));
    }

    #[test]
    fn test_conflict_write_vs_write() {
        let velocity = ComponentTypeId(2);

        let q1 = QueryDescriptor::new().write(velocity);
        let q2 = QueryDescriptor::new().write(velocity);

        assert!(q1.conflicts_with(&q2));
    }

    #[test]
    fn test_no_conflict_different_types() {
        let velocity = ComponentTypeId(2);
        let ai_state = ComponentTypeId(3);
        let transform = ComponentTypeId(1);

        // Physics: reads Transform, writes Velocity
        let physics = QueryDescriptor::new().read(transform).write(velocity);
        // AI: reads Transform, writes AiState
        let ai = QueryDescriptor::new().read(transform).write(ai_state);

        assert!(!physics.conflicts_with(&ai));
    }

    #[test]
    fn test_conflict_movement_vs_physics() {
        let transform = ComponentTypeId(1);
        let velocity = ComponentTypeId(2);

        // Physics: reads Transform, writes Velocity
        let physics = QueryDescriptor::new().read(transform).write(velocity);
        // Movement: reads Velocity, writes Transform
        let movement = QueryDescriptor::new().read(velocity).write(transform);

        assert!(physics.conflicts_with(&movement));
    }

    #[test]
    fn test_required_types() {
        let a = ComponentTypeId(1);
        let b = ComponentTypeId(2);
        let c = ComponentTypeId(3);

        let q = QueryDescriptor::new().read(a).write(b).optional(c);

        let required = q.required_types();
        assert!(required.contains(&a));
        assert!(required.contains(&b));
        assert!(!required.contains(&c));
    }
}
