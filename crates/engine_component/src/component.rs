//! Core [`Component`] trait and associated metadata.
//!
//! Every piece of data stored in the ECS must implement [`Component`]. The trait
//! requires `Send + Sync + 'static` so components can be safely shared across
//! async boundaries and transported over the network.

use std::any::TypeId;

use serde::{Deserialize, Serialize};

use crate::entity::Entity;

/// A unique identifier for a component type, derived from [`TypeId`].
///
/// Two components of the same Rust type will always produce the same
/// `ComponentTypeId`. The inner value is an opaque `u64` hash — do not rely
/// on its numeric value being stable across compiler versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct ComponentTypeId(pub u64);

impl ComponentTypeId {
    /// Compute the [`ComponentTypeId`] for a concrete type `T`.
    #[must_use]
    pub fn of<T: 'static>() -> Self {
        // TypeId doesn't expose its inner bits publicly, so we hash it.
        let type_id = TypeId::of::<T>();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            type_id.hash(&mut hasher);
            hasher.finish()
        };
        Self(hash)
    }
}

/// Metadata about a component type, used for type-erased storage.
#[derive(Debug, Clone)]
pub struct ComponentMeta {
    /// The unique type identifier.
    pub type_id: ComponentTypeId,
    /// The human-readable name of the component (e.g. `"Transform3D"`).
    pub name: &'static str,
    /// Size of one component instance in bytes.
    pub layout: std::alloc::Layout,
    /// Function pointer to drop a component in-place.
    pub drop_fn: Option<unsafe fn(*mut u8)>,
    /// Serialise a single component instance to MessagePack bytes.
    pub serialize_fn: fn(&[u8]) -> Result<Vec<u8>, rmp_serde::encode::Error>,
    /// Deserialise a single component instance from MessagePack bytes.
    pub deserialize_fn: fn(&[u8]) -> Result<Vec<u8>, rmp_serde::decode::Error>,
}

/// The core component trait.
///
/// All data stored in the ECS must implement this trait. Components must be
/// serialisable for network transport and `Send + Sync` for safe concurrent
/// access.
///
/// # Examples
///
/// ```rust
/// use serde::{Serialize, Deserialize};
/// use engine_component::Component;
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct Health {
///     current: f32,
///     max: f32,
/// }
///
/// impl Component for Health {
///     fn type_name() -> &'static str { "Health" }
/// }
/// ```
pub trait Component: Send + Sync + 'static + Serialize + for<'de> Deserialize<'de> {
    /// A human-readable name for this component type.
    fn type_name() -> &'static str;

    /// Returns the [`ComponentTypeId`] for this component.
    fn component_type_id() -> ComponentTypeId {
        ComponentTypeId::of::<Self>()
    }

    /// Returns the [`ComponentMeta`] descriptor for this component type.
    fn meta() -> ComponentMeta {
        ComponentMeta {
            type_id: Self::component_type_id(),
            name: Self::type_name(),
            layout: std::alloc::Layout::new::<Self>(),
            drop_fn: if std::mem::needs_drop::<Self>() {
                Some(|ptr: *mut u8| unsafe {
                    std::ptr::drop_in_place(ptr as *mut Self);
                })
            } else {
                None
            },
            serialize_fn: |bytes: &[u8]| {
                assert!(bytes.len() >= std::mem::size_of::<Self>());
                // SAFETY: Caller guarantees `bytes` points to a valid `Self`.
                let value = unsafe { &*(bytes.as_ptr() as *const Self) };
                rmp_serde::to_vec(value)
            },
            deserialize_fn: |bytes: &[u8]| {
                let value: Self = rmp_serde::from_slice(bytes)
                    .map_err(|e| rmp_serde::decode::Error::Syntax(e.to_string()))?;
                let mut result = vec![0u8; std::mem::size_of::<Self>()];
                // SAFETY: We write a valid `Self` into the correctly-sized buffer.
                unsafe {
                    std::ptr::write(result.as_mut_ptr() as *mut Self, value);
                }
                Ok(result)
            },
        }
    }
}

/// A record pairing an [`Entity`] with serialised component data.
///
/// Used when shipping component shards over the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRecord {
    /// The entity this component belongs to.
    pub entity: Entity,
    /// MessagePack-encoded component bytes.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    struct Health {
        current: f32,
        max: f32,
    }

    impl Component for Health {
        fn type_name() -> &'static str {
            "Health"
        }
    }

    #[test]
    fn test_component_type_id_is_stable() {
        let id1 = Health::component_type_id();
        let id2 = Health::component_type_id();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_component_type_id_differs_between_types() {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        struct Velocity {
            x: f32,
            y: f32,
        }
        impl Component for Velocity {
            fn type_name() -> &'static str {
                "Velocity"
            }
        }

        assert_ne!(Health::component_type_id(), Velocity::component_type_id());
    }

    #[test]
    fn test_component_meta_name() {
        let meta = Health::meta();
        assert_eq!(meta.name, "Health");
    }

    #[test]
    fn test_component_meta_layout() {
        let meta = Health::meta();
        assert_eq!(meta.layout, std::alloc::Layout::new::<Health>());
    }

    #[test]
    fn test_component_roundtrip_serialization() {
        let health = Health {
            current: 80.0,
            max: 100.0,
        };
        let bytes = rmp_serde::to_vec(&health).unwrap();
        let restored: Health = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(health, restored);
    }
}
