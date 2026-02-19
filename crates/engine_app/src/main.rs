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

use engine_net::NatsConnection;
use tick::{TickConfig, TickLoop};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("engine_app=info".parse()?))
        .init();

    info!("engine coordinator starting");

    // Connect to NATS.
    let conn = NatsConnection::connect().await?;
    info!("connected to NATS");

    let config = TickConfig {
        tick_rate: 60.0,
        max_ticks: 0, // Run indefinitely.
    };

    let mut tick_loop = TickLoop::new(config);
    tick_loop.run_async(&conn).await?;

    info!("engine coordinator shut down");
    Ok(())
}
