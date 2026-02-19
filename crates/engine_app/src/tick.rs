//! Coordinator tick loop.
//!
//! Implements the fixed-timestep tick lifecycle described in `ARCHITECTURE.md`:
//!
//! 1. Apply pending system register/unregister changes.
//! 2. Process entity creation/destruction commands.
//! 3. Build the dependency graph from registered system queries.
//! 4. Compute execution stages.
//! 5. For each stage: send data, schedule systems, wait for acks, merge results.
//! 6. Broadcast deferred events.
//! 7. Advance the tick counter.
//!
//! Systems may register or unregister at any time via NATS. Incoming requests
//! are queued and applied atomically before the next tick starts, ensuring the
//! system set never changes mid-tick.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use tracing::{debug, info, warn};

use engine_net::NatsConnection;
use engine_net::messages::{
    self, ComponentShard, DataDone, SystemSchedule, SystemUnregister, TickAck,
};

use crate::registry::SystemRegistry;
use crate::scheduler::{self, RegisteredSystem, Stage};
use crate::world::World;

/// A pending system change to apply before the next tick.
#[derive(Debug, Clone)]
pub(crate) enum PendingSystemChange {
    /// A system instance wants to register.
    Register(engine_net::messages::SystemDescriptor),
    /// A system instance wants to unregister.
    Unregister { name: String, instance_id: String },
}

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
    /// Queue of pending register/unregister changes applied before each tick.
    pending_changes: Vec<PendingSystemChange>,
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
            pending_changes: Vec::new(),
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

    /// Enqueue a pending system change to be applied before the next tick.
    pub fn enqueue_change(&mut self, change: PendingSystemChange) {
        self.pending_changes.push(change);
    }

    /// Apply all pending register/unregister changes to the registry.
    ///
    /// This is called once at the start of each tick, ensuring that systems
    /// are only added or removed between ticks — never mid-tick.
    fn apply_pending_changes(&mut self) {
        if self.pending_changes.is_empty() {
            return;
        }

        let changes: Vec<PendingSystemChange> = self.pending_changes.drain(..).collect();
        for change in changes {
            match change {
                PendingSystemChange::Register(descriptor) => {
                    info!(
                        system = descriptor.name,
                        instance = descriptor.instance_id,
                        "applying queued registration"
                    );
                    self.registry.register(descriptor);
                    self.stages_dirty = true;
                }
                PendingSystemChange::Unregister { name, instance_id } => {
                    info!(
                        system = name,
                        instance = instance_id,
                        "applying queued unregistration"
                    );
                    if self.registry.unregister_instance(&name, &instance_id) {
                        self.stages_dirty = true;
                    } else {
                        warn!(
                            system = name,
                            instance = instance_id,
                            "unregister requested but instance not found"
                        );
                    }
                }
            }
        }
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

        // Apply any queued register/unregister changes before running.
        self.apply_pending_changes();

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
    ///   1. Subscribe to `component.changed.<system>` for each system.
    ///   2. Publish `component.set.<system>` shards, a `DataDone` sentinel,
    ///      and `system.schedule.<system>`.
    ///   3. Drain `component.changed.<system>` until `ChangesDone` sentinels
    ///      arrive from every instance, merging shards into the world.
    ///   4. Wait for `coord.tick.done` acks from all instances.
    async fn tick_async(
        &mut self,
        conn: &NatsConnection,
        ack_sub: &mut async_nats::Subscriber,
    ) -> Result<()> {
        self.tick_id += 1;

        // Apply any queued register/unregister changes before running.
        self.apply_pending_changes();

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

            // 1. Subscribe to component.changed.<system> for each system
            //    BEFORE publishing schedules, so we don't miss any messages.
            let mut changed_subs: Vec<(String, usize, async_nats::Subscriber)> = Vec::new();
            for (system_name, instance_count) in &stage_systems {
                let changed_subject = engine_net::subjects::component_changed(system_name);
                let sub = conn.subscribe(&changed_subject).await?;
                changed_subs.push((system_name.clone(), *instance_count, sub));
            }

            // 2. Publish component data shards, data-done sentinel, and schedule.
            for (system_name, _) in &stage_systems {
                // Publish component.set.<system> — for now send all matching
                // archetype data as ComponentShards. Systems receive the full
                // data set for the component types they declared.
                let set_subject = engine_net::subjects::component_set(system_name);

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
                                conn.publish(&set_subject, &shard).await?;
                            }
                        }
                    }
                }

                // Publish a DataDone sentinel so the system knows all data
                // shards have been sent and can stop waiting immediately.
                let data_done = DataDone {
                    tick_id: self.tick_id,
                };
                let mut headers = async_nats::HeaderMap::new();
                headers.insert(messages::headers::MSG_TYPE, messages::DATA_DONE_MSG_TYPE);
                conn.publish_with_headers(&set_subject, headers, &data_done)
                    .await?;

                // Publish the schedule message to trigger execution.
                let schedule = SystemSchedule {
                    tick_id: self.tick_id,
                    shard_range: None,
                };
                let subject = engine_net::subjects::system_schedule(system_name);
                conn.publish(&subject, &schedule).await?;
            }

            // 3. Collect changed component data from systems.
            //
            //    Each system instance publishes its changed component shards
            //    on `component.changed.<system>`, followed by a ChangesDone
            //    sentinel (same subject, msg-type header = "changes_done").
            //    We drain until we receive a sentinel from every instance,
            //    or hit a timeout.
            let deadline = Instant::now() + Duration::from_secs(5);

            for (system_name, instance_count, mut changed_sub) in changed_subs {
                let mut sentinels_received = 0usize;

                while sentinels_received < instance_count {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        warn!(
                            tick_id = self.tick_id,
                            stage = stage_idx,
                            system = system_name,
                            expected = instance_count,
                            received = sentinels_received,
                            "changes-done timeout — proceeding with partial data"
                        );
                        break;
                    }

                    match tokio::time::timeout(remaining, changed_sub.next()).await {
                        Ok(Some(msg)) => {
                            // Check if this is a ChangesDone sentinel.
                            let is_sentinel = msg
                                .headers
                                .as_ref()
                                .and_then(|h| h.get(messages::headers::MSG_TYPE))
                                .is_some_and(|v| v.as_str() == messages::CHANGES_DONE_MSG_TYPE);

                            if is_sentinel {
                                sentinels_received += 1;
                                debug!(
                                    tick_id = self.tick_id,
                                    system = system_name,
                                    sentinels = sentinels_received,
                                    total = instance_count,
                                    "changes-done received"
                                );
                            } else if let Ok(shard) =
                                engine_net::decode::<ComponentShard>(msg.payload.as_ref())
                            {
                                self.merge_shard(&shard);
                            }
                        }
                        Ok(None) => break, // subscriber closed
                        Err(_) => {
                            warn!(
                                tick_id = self.tick_id,
                                stage = stage_idx,
                                system = system_name,
                                expected = instance_count,
                                received = sentinels_received,
                                "changes-done timeout — proceeding with partial data"
                            );
                            break;
                        }
                    }
                }
                // Unsubscribe by dropping.
                drop(changed_sub);
            }

            // 4. Wait for acks from all system instances in this stage.
            if total_acks > 0 {
                let mut acks_received = 0usize;

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
    /// NATS, listens for system registrations and unregistrations, and runs
    /// the fixed-timestep tick loop publishing/receiving data over NATS.
    ///
    /// Systems may register or unregister at any time. Incoming requests are
    /// queued and applied atomically before the next tick starts, ensuring
    /// the system set never changes mid-tick.
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

        // Subscribe to system unregistrations.
        let mut unregister_sub = conn
            .subscribe(engine_net::subjects::SYSTEM_UNREGISTER)
            .await?;

        info!(
            tick_rate = self.config.tick_rate,
            max_ticks = self.config.max_ticks,
            "starting tick loop (NATS)"
        );

        let mut tick_count = 0u64;

        loop {
            let start = Instant::now();

            // Drain any pending registrations that arrived since the last tick.
            while let Ok(Some(msg)) =
                tokio::time::timeout(Duration::ZERO, register_sub.next()).await
            {
                if let Ok(desc) = engine_net::decode::<engine_net::messages::SystemDescriptor>(
                    msg.payload.as_ref(),
                ) {
                    info!(
                        system = desc.name,
                        instance = desc.instance_id,
                        "queued system registration"
                    );
                    self.enqueue_change(PendingSystemChange::Register(desc));
                }
            }

            // Drain any pending unregistrations that arrived since the last tick.
            while let Ok(Some(msg)) =
                tokio::time::timeout(Duration::ZERO, unregister_sub.next()).await
            {
                if let Ok(unreg) = engine_net::decode::<SystemUnregister>(msg.payload.as_ref()) {
                    info!(
                        system = unreg.name,
                        instance = unreg.instance_id,
                        "queued system unregistration"
                    );
                    self.enqueue_change(PendingSystemChange::Unregister {
                        name: unreg.name,
                        instance_id: unreg.instance_id,
                    });
                }
            }

            // Run the tick (applies pending changes internally).
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

    #[test]
    fn test_pending_register_applied_on_tick() {
        let mut tick_loop = TickLoop::new(TickConfig::default());

        // Enqueue a registration.
        tick_loop.enqueue_change(PendingSystemChange::Register(SystemDescriptor {
            name: "physics".to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: "inst-1".to_string(),
        }));

        // Before the tick, registry should still be empty.
        assert_eq!(tick_loop.registry().system_count(), 0);

        // After the tick, the system should be registered.
        tick_loop.tick();
        assert_eq!(tick_loop.registry().system_count(), 1);
        assert_eq!(tick_loop.registry().total_instances(), 1);
    }

    #[test]
    fn test_pending_unregister_applied_on_tick() {
        let mut tick_loop = TickLoop::new(TickConfig::default());

        // Directly register a system first.
        tick_loop.registry_mut().register(SystemDescriptor {
            name: "physics".to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: "inst-1".to_string(),
        });
        assert_eq!(tick_loop.registry().system_count(), 1);

        // Enqueue an unregistration.
        tick_loop.enqueue_change(PendingSystemChange::Unregister {
            name: "physics".to_string(),
            instance_id: "inst-1".to_string(),
        });

        // Before the tick, system should still be registered.
        assert_eq!(tick_loop.registry().system_count(), 1);

        // After the tick, the system should be gone.
        tick_loop.tick();
        assert_eq!(tick_loop.registry().system_count(), 0);
    }

    #[test]
    fn test_pending_changes_applied_in_order() {
        let mut tick_loop = TickLoop::new(TickConfig::default());

        // Enqueue register then unregister for the same system.
        tick_loop.enqueue_change(PendingSystemChange::Register(SystemDescriptor {
            name: "physics".to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: "inst-1".to_string(),
        }));
        tick_loop.enqueue_change(PendingSystemChange::Unregister {
            name: "physics".to_string(),
            instance_id: "inst-1".to_string(),
        });

        // After tick, register then unregister leaves the registry empty.
        tick_loop.tick();
        assert_eq!(tick_loop.registry().system_count(), 0);
    }

    #[test]
    fn test_pending_changes_trigger_stage_recomputation() {
        let config = TickConfig {
            tick_rate: 1000.0,
            max_ticks: 0,
        };
        let mut tick_loop = TickLoop::new(config);

        // Run a tick with no systems.
        tick_loop.tick();
        assert!(tick_loop.stages.is_empty());

        // Enqueue a system registration.
        tick_loop.enqueue_change(PendingSystemChange::Register(SystemDescriptor {
            name: "physics".to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: "inst-1".to_string(),
        }));

        // After tick, stages should have been recomputed.
        tick_loop.tick();
        assert_eq!(tick_loop.stages.len(), 1);
    }
}
