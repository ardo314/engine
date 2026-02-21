//! # engine_net
//!
//! NATS transport layer for the distributed ECS engine.
//!
//! This crate provides:
//!
//! - [`subjects`] — NATS subject hierarchy constants and builders.
//! - [`messages`] — Message types exchanged between coordinator and systems.
//! - [`codec`] — MessagePack serialisation/deserialisation helpers.
//! - [`connection`] — NATS connection management.
//! - [`error`] — Network-layer error types.

pub mod codec;
pub mod connection;
pub mod error;
pub mod messages;
pub mod subjects;

pub use codec::{decode, encode};
pub use connection::NatsConnection;
pub use error::NetError;
