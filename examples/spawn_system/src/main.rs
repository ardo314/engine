//! Spawn system — creates an entity with a Velocity component.
//!
//! This system demonstrates how to request entity creation from the
//! coordinator. On its first tick it publishes an [`EntitySpawnRequest`]
//! containing a [`Velocity`] component. The coordinator allocates the
//! entity and places it into the appropriate archetype so other systems
//! (e.g. `accelerate_system`, `print_velocity_system`) can operate on it.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use components::Velocity;
use engine_component::{Component, QueryDescriptor};
use engine_net::messages::EntitySpawnRequest;
use engine_system::{SystemConfig, SystemRunner};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("spawn_system=info".parse()?))
        .init();

    // This system writes Velocity (it creates entities with that component).
    let config = SystemConfig::new(
        "spawn",
        QueryDescriptor::new().write(Velocity::component_type_id()),
    );

    let runner = SystemRunner::new(config);
    info!("spawn system starting");

    let spawned = AtomicBool::new(false);

    runner
        .run(move |ctx| {
            if !spawned.load(Ordering::Relaxed) {
                // Build the Velocity component to attach to the new entity.
                let velocity = Velocity::new(1.0, 0.0, 0.0);
                let vel_bytes =
                    engine_net::encode(&velocity).expect("failed to serialise Velocity");

                let request = EntitySpawnRequest {
                    component_types: vec![Velocity::component_type_id()],
                    component_data: vec![vel_bytes],
                };

                // Queue the spawn request — the system context gives us
                // access to publish it via output.
                ctx.spawn_requests.push(request);

                info!(
                    tick = ctx.tick_id,
                    "requested entity spawn with Velocity(1, 0, 0)"
                );
                spawned.store(true, Ordering::Relaxed);
            }
        })
        .await?;

    Ok(())
}
