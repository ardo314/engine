//! # engine_component
//!
//! The "C" in ECS — defines what a component is, how it is stored, and how it
//! is serialised for network transport.
//!
//! This crate provides:
//!
//! - [`Component`] trait — the contract all ECS data must satisfy.
//! - [`Entity`] — lightweight `u64` entity identifiers.
//! - [`EntityAllocator`] — monotonically increasing ID allocator.
//! - [`ArchetypeTable`] — SoA storage grouped by component combination.
//! - [`QueryDescriptor`] — declarative data access requirements for systems.

pub mod archetype;
pub mod component;
pub mod entity;
pub mod query;

pub use archetype::{ArchetypeId, ArchetypeTable, Column};
pub use component::{Component, ComponentMeta, ComponentRecord, ComponentTypeId};
pub use entity::{Entity, EntityAllocator};
pub use query::{QueryDescriptor, QueryFilter};
