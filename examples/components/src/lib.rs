//! Example component definitions for the distributed ECS engine.
//!
//! These demonstrate how to define components that satisfy the [`Component`]
//! trait requirements: `Serialize`, `Deserialize`, `Send + Sync + 'static`.

use engine_component::Component;
use engine_math::Vec3;
use serde::{Deserialize, Serialize};

/// A 3D velocity component.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Velocity {
    /// Linear velocity in world units per second.
    pub linear: Vec3,
}

impl Velocity {
    /// Zero velocity.
    pub const ZERO: Self = Self { linear: Vec3::ZERO };

    /// Create a new velocity.
    #[must_use]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            linear: Vec3::new(x, y, z),
        }
    }
}

impl Default for Velocity {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Component for Velocity {
    fn type_name() -> &'static str {
        "Velocity"
    }
}

/// A health component with current and maximum hit points.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Health {
    /// Current hit points.
    pub current: f32,
    /// Maximum hit points.
    pub max: f32,
}

impl Health {
    /// Create a new health component at full HP.
    #[must_use]
    pub fn full(max: f32) -> Self {
        Self { current: max, max }
    }

    /// Returns `true` if the entity is alive (HP > 0).
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.current > 0.0
    }

    /// Apply damage, clamping to zero.
    pub fn damage(&mut self, amount: f32) {
        self.current = (self.current - amount).max(0.0);
    }

    /// Heal, clamping to max.
    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }
}

impl Component for Health {
    fn type_name() -> &'static str {
        "Health"
    }
}

/// A simple name tag component for debugging.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Name {
    /// The entity's display name.
    pub value: String,
}

impl Name {
    /// Create a new name component.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { value: name.into() }
    }
}

impl Component for Name {
    fn type_name() -> &'static str {
        "Name"
    }
}

/// A simple mesh reference component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeshRef {
    /// Path or identifier of the mesh asset.
    pub asset_path: String,
}

impl MeshRef {
    /// Create a new mesh reference.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            asset_path: path.into(),
        }
    }
}

impl Component for MeshRef {
    fn type_name() -> &'static str {
        "MeshRef"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_serialization() {
        let v = Velocity::new(1.0, 2.0, 3.0);
        let bytes = rmp_serde::to_vec(&v).unwrap();
        let restored: Velocity = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(v, restored);
    }

    #[test]
    fn test_health_damage_and_heal() {
        let mut h = Health::full(100.0);
        assert!(h.is_alive());
        h.damage(60.0);
        assert_eq!(h.current, 40.0);
        h.heal(30.0);
        assert_eq!(h.current, 70.0);
        h.damage(200.0);
        assert_eq!(h.current, 0.0);
        assert!(!h.is_alive());
    }

    #[test]
    fn test_name_component() {
        let name = Name::new("Player");
        let bytes = rmp_serde::to_vec(&name).unwrap();
        let restored: Name = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(name, restored);
    }
}
