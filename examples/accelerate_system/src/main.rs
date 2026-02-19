//! Accelerate system — increases velocity each tick.
//!
//! This system reads and writes `Velocity` components, adding a small
//! acceleration on each tick to demonstrate a read-write system.

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use components::Velocity;
use engine_component::{Component, QueryDescriptor};
use engine_math::Vec3;
use engine_system::{SystemConfig, SystemRunner};

/// Acceleration applied per tick (world units per second² is not relevant
/// here since we have no dt — this is a fixed increment per tick).
const ACCELERATION: Vec3 = Vec3::new(0.0, 0.0, 1.0);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("accelerate_system=info".parse()?),
        )
        .init();

    let config = SystemConfig::new(
        "accelerate",
        QueryDescriptor::new().write(Velocity::component_type_id()),
    );

    let runner = SystemRunner::new(config);
    info!("accelerate system starting");

    runner
        .run(|ctx| {
            let mut velocities = ctx.read_components::<Velocity>();

            for (_entity, vel) in &mut velocities {
                vel.linear += ACCELERATION;
            }

            let changed: Vec<_> = velocities.iter().map(|(e, v)| (*e, *v)).collect();
            ctx.write_changed(&changed);

            info!(
                tick = ctx.tick_id,
                count = changed.len(),
                "accelerated entities"
            );
        })
        .await?;

    Ok(())
}
