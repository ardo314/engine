//! Print velocity system — logs velocity each tick.
//!
//! This system reads `Velocity` components and prints their values. It is a
//! read-only system and never publishes changed data back.

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use components::Velocity;
use engine_component::{Component, QueryDescriptor};
use engine_system::{SystemConfig, SystemRunner};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("print_velocity_system=info".parse()?),
        )
        .init();

    let config = SystemConfig::new(
        "print_velocity",
        QueryDescriptor::new().read(Velocity::component_type_id()),
    );

    let runner = SystemRunner::new(config);
    info!("print_velocity system starting");

    runner
        .run(|ctx| {
            let velocities = ctx.read_components::<Velocity>();

            for (entity, vel) in &velocities {
                info!(
                    tick = ctx.tick_id,
                    entity = entity.id(),
                    x = vel.linear.x,
                    y = vel.linear.y,
                    z = vel.linear.z,
                    "velocity"
                );
            }
        })
        .await?;

    Ok(())
}
