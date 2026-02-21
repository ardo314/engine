//! 3D transform component.
//!
//! [`Transform3D`] represents position, rotation, and scale in 3D space.
//! It is one of the most commonly used components in any game or simulation.

use engine_component::Component;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// A 3D transform representing position, rotation, and uniform scale.
///
/// This is the primary spatial component — nearly every visible entity will
/// have a `Transform3D`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Transform3D {
    /// World-space position.
    pub position: Vec3,
    /// Rotation as a unit quaternion.
    pub rotation: Quat,
    /// Uniform scale factor.
    pub scale: Vec3,
}

impl Transform3D {
    /// The identity transform: origin, no rotation, unit scale.
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Create a new transform with the given position and default rotation/scale.
    #[must_use]
    pub fn from_position(position: Vec3) -> Self {
        Self {
            position,
            ..Self::IDENTITY
        }
    }

    /// Create a new transform with position and rotation.
    #[must_use]
    pub fn from_position_rotation(position: Vec3, rotation: Quat) -> Self {
        Self {
            position,
            rotation,
            ..Self::IDENTITY
        }
    }

    /// Compute the 4×4 model matrix for this transform.
    #[must_use]
    pub fn to_matrix(&self) -> glam::Mat4 {
        glam::Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    /// Translate the transform by the given offset.
    #[must_use]
    pub fn translated(mut self, offset: Vec3) -> Self {
        self.position += offset;
        self
    }

    /// Rotate the transform by the given quaternion.
    #[must_use]
    pub fn rotated(mut self, rotation: Quat) -> Self {
        self.rotation = rotation * self.rotation;
        self
    }

    /// Apply a uniform scale factor.
    #[must_use]
    pub fn scaled(mut self, factor: f32) -> Self {
        self.scale *= factor;
        self
    }
}

impl Default for Transform3D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Component for Transform3D {
    fn type_name() -> &'static str {
        "Transform3D"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_transform() {
        let t = Transform3D::IDENTITY;
        assert_eq!(t.position, Vec3::ZERO);
        assert_eq!(t.rotation, Quat::IDENTITY);
        assert_eq!(t.scale, Vec3::ONE);
    }

    #[test]
    fn test_from_position() {
        let t = Transform3D::from_position(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.rotation, Quat::IDENTITY);
    }

    #[test]
    fn test_translated() {
        let t = Transform3D::IDENTITY.translated(Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(t.position, Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn test_matrix_identity() {
        let t = Transform3D::IDENTITY;
        let m = t.to_matrix();
        assert_eq!(m, glam::Mat4::IDENTITY);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let t = Transform3D::from_position(Vec3::new(1.0, 2.0, 3.0));
        let bytes = rmp_serde::to_vec(&t).unwrap();
        let restored: Transform3D = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(t, restored);
    }
}
