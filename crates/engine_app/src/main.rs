//! # engine_app — Coordinator
//!
//! The coordinator is the single source of truth for world state in the
//! distributed ECS. It owns entity allocation, archetype storage, system
//! registration, and the tick loop.
//!
//! ## Startup Sequence
//!
//! 1. Connect to NATS (configurable URL, default `nats://localhost:4222`).
//! 2. Subscribe to `engine.system.register`.
//! 3. Enter the fixed-timestep tick loop.

mod registry;
mod scheduler;
mod tick;
mod world;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use tick::{TickConfig, TickLoop};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("engine_app=info".parse()?))
        .init();

    info!("engine coordinator starting");

    // TODO: Connect to NATS and subscribe to system.register.
    // For now, run a local tick loop for demonstration.
    let config = TickConfig {
        tick_rate: 60.0,
        max_ticks: 1, // Run a single tick for startup verification.
    };

    let mut tick_loop = TickLoop::new(config);
    tick_loop.run();

    info!("engine coordinator shut down");
    Ok(())
}
