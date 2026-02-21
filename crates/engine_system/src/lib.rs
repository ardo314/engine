//! # engine_system
//!
//! System runtime library for the distributed ECS engine.
//!
//! This crate provides the harness that turns a system function into a
//! standalone NATS-connected process. Each system:
//!
//! 1. Connects to NATS.
//! 2. Registers its query with the coordinator.
//! 3. Subscribes to its schedule subject (via a queue group for load balancing).
//! 4. On each tick: receives component shards, executes, publishes changes.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use engine_system::{SystemConfig, SystemRunner};
//! use engine_component::{ComponentTypeId, QueryDescriptor};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = SystemConfig::new(
//!         "physics",
//!         QueryDescriptor::new()
//!             .read(ComponentTypeId(1))
//!             .write(ComponentTypeId(2)),
//!     );
//!
//!     let runner = SystemRunner::new(config);
//!     // runner.run(|ctx| { /* system logic */ }).await.unwrap();
//! }
//! ```

pub mod config;
pub mod context;
pub mod runner;

pub use config::SystemConfig;
pub use context::SystemContext;
pub use runner::SystemRunner;
