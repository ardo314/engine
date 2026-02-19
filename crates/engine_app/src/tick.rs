//! Coordinator tick loop.
//!
//! Implements the fixed-timestep tick lifecycle described in `ARCHITECTURE.md`:
//!
//! 1. Process entity creation/destruction commands.
//! 2. Build the dependency graph from registered system queries.
//! 3. Compute execution stages.
//! 4. For each stage: send data, schedule systems, wait for acks, merge results.
//! 5. Broadcast deferred events.
//! 6. Advance the tick counter.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use tracing::{debug, info, warn};

use engine_net::NatsConnection;
use engine_net::messages::{ComponentShard, SystemSchedule, TickAck};

use crate::registry::SystemRegistry;
use crate::scheduler::{self, RegisteredSystem, Stage};
use crate::world::World;

/// Configuration for the coordinator tick loop.
#[derive(Debug, Clone)]
pub struct TickConfig {
    /// Target ticks per second.
    pub tick_rate: f64,
    /// Maximum number of ticks to run (0 = unlimited).
    pub max_ticks: u64,
}

impl Default for TickConfig {
    fn default() -> Self {
        Self {
            tick_rate: 60.0,
            max_ticks: 0,
        }
    }
}

/// The coordinator's tick loop state.
pub struct TickLoop {
    /// Current tick counter.
    tick_id: u64,
    /// Tick configuration.
    config: TickConfig,
    /// The canonical world state.
    world: World,
    /// Registry of connected systems.
    registry: SystemRegistry,
    /// Pre-computed stages (recomputed when system set changes).
    stages: Vec<Stage>,
    /// Cached list of registered systems used alongside `stages`.
    systems: Vec<RegisteredSystem>,
    /// Whether the stage cache is dirty and needs recomputation.
    stages_dirty: bool,
}

impl TickLoop {
    /// Create a new tick loop with the given configuration.
    #[must_use]
    pub fn new(config: TickConfig) -> Self {
        Self {
            tick_id: 0,
            config,
            world: World::new(),
            registry: SystemRegistry::new(),
            stages: Vec::new(),
            systems: Vec::new(),
            stages_dirty: true,
        }
    }

    /// Returns the current tick counter.
    #[must_use]
    pub fn tick_id(&self) -> u64 {
        self.tick_id
    }

    /// Returns a reference to the world.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Returns a mutable reference to the world.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Returns a reference to the system registry.
    #[must_use]
    pub fn registry(&self) -> &SystemRegistry {
        &self.registry
    }

    /// Returns a mutable reference to the system registry.
    pub fn registry_mut(&mut self) -> &mut SystemRegistry {
        self.stages_dirty = true;
        &mut self.registry
    }

    /// Recompute execution stages from the current system registry.
    fn recompute_stages(&mut self) {
        self.systems = self
            .registry
            .iter()
            .map(|info| RegisteredSystem {
                name: info.name.clone(),
                query: info.query.clone(),
            })
            .collect();

        self.stages = scheduler::compute_stages(&self.systems);
        self.stages_dirty = false;

        info!(
            tick_id = self.tick_id,
            stage_count = self.stages.len(),
            system_count = self.systems.len(),
            "recomputed execution stages"
        );
    }

    /// Returns the current execution stages, recomputing if necessary.
    #[must_use]
    pub fn stages(&mut self) -> &[Stage] {
        if self.stages_dirty {
            self.recompute_stages();
        }
        &self.stages
    }

    /// Run one tick of the simulation (local-only, no NATS).
    ///
    /// In a full implementation this would publish/subscribe over NATS. This
    /// version advances state locally and is useful for testing.
    pub fn tick(&mut self) {
        self.tick_id += 1;

        if self.stages_dirty {
            self.recompute_stages();
        }

        debug!(
            tick_id = self.tick_id,
            stages = self.stages.len(),
            "tick start (local)"
        );

        for (stage_idx, stage) in self.stages.iter().enumerate() {
            debug!(
                tick_id = self.tick_id,
                stage = stage_idx,
                systems = stage.system_indices.len(),
                "executing stage"
            );
        }
    }

    /// Run the tick loop for the configured number of ticks, or indefinitely.
    ///
    /// This is a blocking loop intended for local testing. The full
    /// implementation uses [`run_async`](Self::run_async).
    pub fn run(&mut self) {
        let tick_duration = Duration::from_secs_f64(1.0 / self.config.tick_rate);
        let mut tick_count = 0u64;

        info!(
            tick_rate = self.config.tick_rate,
            max_ticks = self.config.max_ticks,
            "starting tick loop (local)"
        );

        loop {
            let start = Instant::now();
            self.tick();

            tick_count += 1;
            if self.config.max_ticks > 0 && tick_count >= self.config.max_ticks {
                info!(ticks = tick_count, "tick loop complete");
                break;
            }

            let elapsed = start.elapsed();
            if elapsed < tick_duration {
                std::thread::sleep(tick_duration - elapsed);
            } else {
                warn!(
                    tick_id = self.tick_id,
                    elapsed_ms = elapsed.as_millis() as u64,
                    budget_ms = tick_duration.as_millis() as u64,
                    "tick exceeded time budget"
                );
            }
        }
    }

    /// Run one NATS-connected tick.
    ///
    /// For each stage:
    ///   1. Publish `component.set.<system>` shards to every system in the stage.
    ///   2. Publish `system.schedule.<system>` to trigger execution.
    ///   3. Wait for `coord.tick.done` acks from all instances.
    ///   4. Collect `component.changed.<system>` shards and merge into the world.
    async fn tick_async(
        &mut self,
        conn: &NatsConnection,
        ack_sub: &mut async_nats::Subscriber,
    ) -> Result<()> {
        self.tick_id += 1;

        if self.stages_dirty {
            self.recompute_stages();
        }

        debug!(
            tick_id = self.tick_id,
            stages = self.stages.len(),
            "tick start"
        );

        if self.stages.is_empty() {
            return Ok(());
        }

        // Iterate stages sequentially.
        let stage_count = self.stages.len();
        for stage_idx in 0..stage_count {
            let stage = &self.stages[stage_idx];

            // Collect systems and their instance counts for this stage.
            let mut stage_systems: Vec<(String, usize)> = Vec::new();
            for &sys_idx in &stage.system_indices {
                let system = &self.systems[sys_idx];
                let instance_count = self
                    .registry
                    .get(&system.name)
                    .map(|info| info.instances.len())
                    .unwrap_or(0);
                stage_systems.push((system.name.clone(), instance_count));
            }

            debug!(
                tick_id = self.tick_id,
                stage = stage_idx,
                systems = ?stage_systems.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
                "stage start"
            );

            // Count total acks expected for this stage.
            let total_acks: usize = stage_systems.iter().map(|(_, count)| *count).sum();

            // 1. Publish component data shards to each system.
            // 2. Publish schedule messages.
            for (system_name, _) in &stage_systems {
                // Publish component.set.<system> — for now send all matching
                // archetype data as ComponentShards. Systems receive the full
                // data set for the component types they declared.
                let sys_info = self.registry.get(system_name);
                if let Some(info) = sys_info {
                    let required = info.query.required_types();
                    let matching = self.world.matching_archetypes(&required);

                    for &arch_id in &matching {
                        if let Some(table) = self.world.archetype(arch_id) {
                            // Send one ComponentShard per component type.
                            for col in &table.columns {
                                let shard = ComponentShard {
                                    component_type: col.type_id,
                                    entities: table.entities.clone(),
                                    data: (0..table.entities.len())
                                        .map(|i| {
                                            col.get_raw(i).map(|b| b.to_vec()).unwrap_or_default()
                                        })
                                        .collect(),
                                };
                                let subject = engine_net::subjects::component_set(system_name);
                                conn.publish(&subject, &shard).await?;
                            }
                        }
                    }
                }

                // Publish the schedule message to trigger execution.
                let schedule = SystemSchedule {
                    tick_id: self.tick_id,
                    shard_range: None,
                };
                let subject = engine_net::subjects::system_schedule(system_name);
                conn.publish(&subject, &schedule).await?;
            }

            // 3. Wait for acks from all system instances in this stage.
            if total_acks > 0 {
                let mut acks_received = 0usize;
                let deadline = Instant::now() + Duration::from_secs(5);

                while acks_received < total_acks {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        warn!(
                            tick_id = self.tick_id,
                            stage = stage_idx,
                            expected = total_acks,
                            received = acks_received,
                            "stage ack timeout — proceeding"
                        );
                        break;
                    }

                    match tokio::time::timeout(remaining, ack_sub.next()).await {
                        Ok(Some(msg)) => {
                            if let Ok(ack) = engine_net::decode::<TickAck>(msg.payload.as_ref())
                                && ack.tick_id == self.tick_id
                            {
                                acks_received += 1;
                                debug!(
                                    tick_id = self.tick_id,
                                    instance = ack.instance_id,
                                    acks = acks_received,
                                    total = total_acks,
                                    "ack received"
                                );
                            }
                        }
                        Ok(None) => break, // subscriber closed
                        Err(_) => {
                            warn!(
                                tick_id = self.tick_id,
                                stage = stage_idx,
                                expected = total_acks,
                                received = acks_received,
                                "stage ack timeout — proceeding"
                            );
                            break;
                        }
                    }
                }
            }

            // 4. Collect changed component data from systems.
            //    Subscribe transiently and drain any pending messages.
            for (system_name, _) in &stage_systems {
                let changed_subject = engine_net::subjects::component_changed(system_name);
                let mut changed_sub = conn.subscribe(&changed_subject).await?;

                // Give a very brief window to drain buffered messages.
                while let Ok(Some(msg)) =
                    tokio::time::timeout(Duration::from_millis(50), changed_sub.next()).await
                {
                    if let Ok(shard) = engine_net::decode::<ComponentShard>(msg.payload.as_ref()) {
                        self.merge_shard(&shard);
                    }
                }
                // Unsubscribe by dropping.
                drop(changed_sub);
            }

            debug!(tick_id = self.tick_id, stage = stage_idx, "stage complete");
        }

        debug!(tick_id = self.tick_id, "tick complete");
        Ok(())
    }

    /// Merge a changed component shard back into the canonical world state.
    ///
    /// For each entity in the shard, find it in the world and overwrite the
    /// corresponding column data.
    fn merge_shard(&mut self, shard: &ComponentShard) {
        for (i, &entity) in shard.entities.iter().enumerate() {
            if let Some(arch_id) = self.world.entity_archetype(entity)
                && let Some(table) = self.world.archetype_mut(arch_id)
                && let Some(col_idx) = table.column_index(shard.component_type)
                && let Some(row) = table.entity_row(entity)
                && let Some(bytes) = shard.data.get(i)
                && let Some(dst) = table.columns[col_idx].get_raw_mut(row)
            {
                let copy_len = dst.len().min(bytes.len());
                dst[..copy_len].copy_from_slice(&bytes[..copy_len]);
            }
        }
    }

    /// Run the async NATS-connected tick loop.
    ///
    /// This is the primary entry point for the coordinator. It connects to
    /// NATS, listens for system registrations, and runs the fixed-timestep
    /// tick loop publishing/receiving data over NATS.
    ///
    /// # Errors
    ///
    /// Returns an error if NATS communication fails.
    pub async fn run_async(&mut self, conn: &NatsConnection) -> Result<()> {
        let tick_duration = Duration::from_secs_f64(1.0 / self.config.tick_rate);

        // Subscribe to tick acks.
        let mut ack_sub = conn
            .subscribe(engine_net::subjects::COORD_TICK_DONE)
            .await?;

        // Subscribe to system registrations.
        let mut register_sub = conn
            .subscribe(engine_net::subjects::SYSTEM_REGISTER)
            .await?;

        info!(
            tick_rate = self.config.tick_rate,
            max_ticks = self.config.max_ticks,
            "starting tick loop (NATS)"
        );

        // Wait briefly for initial system registrations before starting ticks.
        info!("waiting for system registrations (2s)...");
        let registration_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let remaining = registration_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, register_sub.next()).await {
                Ok(Some(msg)) => {
                    if let Ok(desc) = engine_net::decode::<engine_net::messages::SystemDescriptor>(
                        msg.payload.as_ref(),
                    ) {
                        info!(
                            system = desc.name,
                            instance = desc.instance_id,
                            "system registered"
                        );
                        self.registry_mut().register(desc);
                    }
                }
                _ => break,
            }
        }

        info!(
            systems = self.registry.system_count(),
            instances = self.registry.total_instances(),
            "registration phase complete — starting ticks"
        );

        let mut tick_count = 0u64;

        loop {
            let start = Instant::now();

            // Drain any new registrations that arrived between ticks.
            while let Ok(Some(msg)) =
                tokio::time::timeout(Duration::ZERO, register_sub.next()).await
            {
                if let Ok(desc) = engine_net::decode::<engine_net::messages::SystemDescriptor>(
                    msg.payload.as_ref(),
                ) {
                    info!(
                        system = desc.name,
                        instance = desc.instance_id,
                        "system registered (mid-loop)"
                    );
                    self.registry_mut().register(desc);
                }
            }

            // Run the tick.
            self.tick_async(conn, &mut ack_sub).await?;

            tick_count += 1;
            if self.config.max_ticks > 0 && tick_count >= self.config.max_ticks {
                info!(ticks = tick_count, "tick loop complete");
                break;
            }

            let elapsed = start.elapsed();
            if elapsed < tick_duration {
                tokio::time::sleep(tick_duration - elapsed).await;
            } else {
                warn!(
                    tick_id = self.tick_id,
                    elapsed_ms = elapsed.as_millis() as u64,
                    budget_ms = tick_duration.as_millis() as u64,
                    "tick exceeded time budget"
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use engine_component::{ComponentTypeId, QueryDescriptor};
    use engine_net::messages::SystemDescriptor;

    use super::*;

    #[test]
    fn test_tick_advances_counter() {
        let mut tick_loop = TickLoop::new(TickConfig::default());
        assert_eq!(tick_loop.tick_id(), 0);
        tick_loop.tick();
        assert_eq!(tick_loop.tick_id(), 1);
        tick_loop.tick();
        assert_eq!(tick_loop.tick_id(), 2);
    }

    #[test]
    fn test_stages_recomputed_on_registry_change() {
        let mut tick_loop = TickLoop::new(TickConfig::default());

        // Initially no stages.
        let stages = tick_loop.stages();
        assert!(stages.is_empty());

        // Register a system.
        tick_loop.registry_mut().register(SystemDescriptor {
            name: "physics".to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: "inst-1".to_string(),
        });

        // Stages should recompute.
        let stages = tick_loop.stages();
        assert_eq!(stages.len(), 1);
    }

    #[test]
    fn test_run_limited_ticks() {
        let config = TickConfig {
            tick_rate: 1000.0, // fast for testing
            max_ticks: 5,
        };
        let mut tick_loop = TickLoop::new(config);
        tick_loop.run();
        assert_eq!(tick_loop.tick_id(), 5);
    }

    #[test]
    fn test_merge_shard() {
        use std::collections::BTreeSet;

        let mut tick_loop = TickLoop::new(TickConfig::default());

        // Create an entity with one component.
        let comp = ComponentTypeId(42);
        let mut types = BTreeSet::new();
        types.insert(comp);
        let entity = tick_loop.world_mut().spawn(types, &[4]);

        // Write initial component data (4 bytes of zeros).
        let arch_id = tick_loop.world().entity_archetype(entity).unwrap();
        tick_loop
            .world_mut()
            .archetype_mut(arch_id)
            .unwrap()
            .columns[0]
            .push_raw(&[0u8; 4]);

        // Merge a shard that overwrites with [1, 2, 3, 4].
        let shard = ComponentShard {
            component_type: comp,
            entities: vec![entity],
            data: vec![vec![1, 2, 3, 4]],
        };
        tick_loop.merge_shard(&shard);

        // Verify the data was merged.
        let table = tick_loop.world().archetype(arch_id).unwrap();
        let row = table.entity_row(entity).unwrap();
        let data = table.columns[0].get_raw(row).unwrap();
        assert_eq!(data, &[1, 2, 3, 4]);
    }
}
