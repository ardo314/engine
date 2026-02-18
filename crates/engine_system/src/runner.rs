//! System runner — the main harness for running a system as a process.
//!
//! The runner handles NATS connection, registration, and the per-tick
//! receive/execute/publish loop.

use anyhow::Result;
use futures::StreamExt;
use tracing::info;
use uuid::Uuid;

use engine_net::NatsConnection;
use engine_net::messages::SystemDescriptor;

use crate::config::SystemConfig;
use crate::context::SystemContext;

/// The system runner turns a system function into a NATS-connected process.
///
/// Call [`SystemRunner::run`] with a closure to start the system lifecycle.
#[derive(Debug)]
pub struct SystemRunner {
    /// System configuration.
    config: SystemConfig,
    /// Unique instance identifier for this process.
    instance_id: String,
}

impl SystemRunner {
    /// Create a new system runner.
    #[must_use]
    pub fn new(config: SystemConfig) -> Self {
        let instance_id = Uuid::new_v4().to_string();
        Self {
            config,
            instance_id,
        }
    }

    /// Returns the unique instance ID for this runner.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Returns the system name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Build the [`SystemDescriptor`] for registration.
    #[must_use]
    pub fn descriptor(&self) -> SystemDescriptor {
        SystemDescriptor {
            name: self.config.name.clone(),
            query: self.config.query.clone(),
            instance_id: self.instance_id.clone(),
        }
    }

    /// Run the system lifecycle.
    ///
    /// 1. Connect to NATS.
    /// 2. Publish a `system.register` message.
    /// 3. Subscribe to the schedule subject via queue group.
    /// 4. Loop: receive schedule → receive data → execute → publish changes → ack.
    ///
    /// The `system_fn` is called once per tick with a [`SystemContext`]
    /// containing the received component data.
    ///
    /// # Errors
    ///
    /// Returns an error if NATS connection or message handling fails.
    pub async fn run<F>(self, system_fn: F) -> Result<()>
    where
        F: Fn(&mut SystemContext) + Send + 'static,
    {
        let url = self
            .config
            .nats_url
            .as_deref()
            .unwrap_or(engine_net::connection::DEFAULT_NATS_URL);

        info!(
            system = self.config.name,
            instance_id = self.instance_id,
            url,
            "system starting"
        );

        // Connect to NATS.
        let conn = NatsConnection::connect_to(url).await?;

        // Register with the coordinator.
        let descriptor = self.descriptor();
        conn.publish(engine_net::subjects::SYSTEM_REGISTER, &descriptor)
            .await?;
        info!(
            system = self.config.name,
            instance_id = self.instance_id,
            "registered with coordinator"
        );

        // Subscribe to schedule messages via queue group.
        let schedule_subject = engine_net::subjects::system_schedule(&self.config.name);
        let mut schedule_sub = conn.subscribe(&schedule_subject).await?;
        info!(subject = schedule_subject, "subscribed to schedule");

        // Subscribe to component data.
        let data_subject = engine_net::subjects::component_set(&self.config.name);
        let mut _data_sub = conn.subscribe(&data_subject).await?;
        info!(subject = data_subject, "subscribed to component data");

        // Main loop: wait for schedule messages.
        while let Some(schedule_msg) = schedule_sub.next().await {
            // Decode the schedule message.
            let schedule: engine_net::messages::SystemSchedule =
                engine_net::decode(schedule_msg.payload.as_ref())?;

            // Create context for this tick.
            let mut ctx = SystemContext::new(schedule.tick_id, 0.0);

            // TODO: Receive component data shards for this tick.
            // In a full implementation, the coordinator sends component data
            // before the schedule message. We would collect those here.

            // Execute the system function.
            system_fn(&mut ctx);

            // Publish changed component data.
            let changed_subject = engine_net::subjects::component_changed(&self.config.name);
            for shard in &ctx.output_shards {
                conn.publish(&changed_subject, shard).await?;
            }

            // Ack tick completion.
            let ack = engine_net::messages::TickAck {
                tick_id: schedule.tick_id,
                instance_id: self.instance_id.clone(),
            };
            conn.publish(engine_net::subjects::COORD_TICK_DONE, &ack)
                .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use engine_component::{ComponentTypeId, QueryDescriptor};

    use super::*;

    #[test]
    fn test_runner_creation() {
        let config = SystemConfig::new(
            "physics",
            QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
        );
        let runner = SystemRunner::new(config);
        assert_eq!(runner.name(), "physics");
        assert!(!runner.instance_id().is_empty());
    }

    #[test]
    fn test_descriptor() {
        let config = SystemConfig::new(
            "ai",
            QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(3)),
        );
        let runner = SystemRunner::new(config);
        let desc = runner.descriptor();
        assert_eq!(desc.name, "ai");
        assert_eq!(desc.query.reads.len(), 1);
        assert_eq!(desc.query.writes.len(), 1);
        assert_eq!(desc.instance_id, runner.instance_id());
    }
}
