//! Core [`Component`] trait and associated metadata.
//!
//! Every piece of data stored in the ECS must implement [`Component`]. The trait
//! requires `Send + Sync + 'static` so components can be safely shared across
//! async boundaries and transported over the network.
//!
//! ## Polyglot Type Identity
//!
//! [`ComponentTypeId`] is derived from the component's **string name** using
//! the FNV-1a 64-bit hash algorithm. This is deterministic and
//! language-neutral — any language can compute the same ID for a given name.
//! See `PROTOCOL.md` for the algorithm specification.

use serde::{Deserialize, Serialize};

use crate::entity::Entity;

/// A unique identifier for a component type, derived from its string name
/// using the FNV-1a 64-bit hash algorithm.
///
/// The ID is deterministic and language-neutral: any implementation in any
/// language that applies FNV-1a to the same UTF-8 name bytes will produce
/// the same `ComponentTypeId`. See `PROTOCOL.md` for the algorithm
/// specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct ComponentTypeId(pub u64);

impl ComponentTypeId {
    /// FNV-1a 64-bit offset basis.
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

    /// FNV-1a 64-bit prime.
    const FNV_PRIME: u64 = 0x0100_0000_01b3;

    /// Compute the [`ComponentTypeId`] from a component's string name using
    /// the FNV-1a 64-bit hash algorithm.
    ///
    /// This is the **canonical** way to derive a `ComponentTypeId`. The
    /// algorithm is language-neutral — any implementation that applies
    /// FNV-1a to the same UTF-8 bytes will produce the same result.
    ///
    /// # Algorithm (FNV-1a 64-bit)
    ///
    /// ```text
    /// hash = 0xcbf29ce484222325          (offset basis)
    /// for each byte in name.as_bytes():
    ///     hash = hash XOR byte
    ///     hash = hash * 0x00000100000001b3  (prime)
    /// return hash
    /// ```
    #[must_use]
    pub const fn from_name(name: &str) -> Self {
        let bytes = name.as_bytes();
        let mut hash = Self::FNV_OFFSET_BASIS;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(Self::FNV_PRIME);
            i += 1;
        }
        Self(hash)
    }

    /// Compute the [`ComponentTypeId`] for a Rust component type `T`.
    ///
    /// This calls `T::type_name()` and hashes it with FNV-1a, producing
    /// the same result as [`ComponentTypeId::from_name`] with the same
    /// string.
    #[must_use]
    pub fn of<T: Component>() -> Self {
        Self::from_name(T::type_name())
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
    ///
    /// The default implementation hashes [`Component::type_name()`] with
    /// FNV-1a 64-bit, producing a deterministic, language-neutral ID.
    fn component_type_id() -> ComponentTypeId {
        ComponentTypeId::from_name(Self::type_name())
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
                rmp_serde::to_vec_named(value)
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
    fn test_component_type_id_matches_from_name() {
        // The trait method and the standalone function must produce the same ID.
        let from_trait = Health::component_type_id();
        let from_name = ComponentTypeId::from_name("Health");
        assert_eq!(from_trait, from_name);
    }

    #[test]
    fn test_component_type_id_from_name_is_deterministic() {
        // FNV-1a of "Health" — a known constant that any language can verify.
        let id = ComponentTypeId::from_name("Health");
        // Re-computing must yield the same value.
        assert_eq!(id, ComponentTypeId::from_name("Health"));
        // Different names must differ.
        assert_ne!(id, ComponentTypeId::from_name("Velocity"));
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
    fn test_fnv1a_known_vector() {
        // FNV-1a 64-bit of empty string is the offset basis itself.
        assert_eq!(
            ComponentTypeId::from_name(""),
            ComponentTypeId(0xcbf2_9ce4_8422_2325)
        );
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
        let bytes = rmp_serde::to_vec_named(&health).unwrap();
        let restored: Health = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(health, restored);
    }
}
