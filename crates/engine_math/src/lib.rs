//! # engine_math
//!
//! Math types for the distributed ECS engine. Re-exports [`glam`] for linear
//! algebra and defines engine-specific spatial types that implement
//! [`Component`](engine_component::Component).

pub mod transform;

// Re-export glam types for convenience.
pub use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

pub use transform::Transform3D;
