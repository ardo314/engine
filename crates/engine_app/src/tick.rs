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

use tracing::{debug, info, warn};

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
#[derive(Debug)]
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
        let systems: Vec<RegisteredSystem> = self
            .registry
            .iter()
            .map(|info| RegisteredSystem {
                name: info.name.clone(),
                query: info.query.clone(),
            })
            .collect();

        self.stages = scheduler::compute_stages(&systems);
        self.stages_dirty = false;

        info!(
            tick_id = self.tick_id,
            stage_count = self.stages.len(),
            system_count = systems.len(),
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
    pub fn tick(&mut self, dt: f64) {
        self.tick_id += 1;

        if self.stages_dirty {
            self.recompute_stages();
        }

        debug!(
            tick_id = self.tick_id,
            dt,
            stages = self.stages.len(),
            "tick start"
        );

        // In the full NATS implementation, each stage would:
        //   1. Publish component.set.<system> messages.
        //   2. Publish system.schedule.<system> messages.
        //   3. Wait for tick.done acks from all instances.
        //   4. Merge changed components.
        //
        // For now, we just advance the tick counter.
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
    /// implementation uses an async loop with NATS.
    pub fn run(&mut self) {
        let tick_duration = Duration::from_secs_f64(1.0 / self.config.tick_rate);
        let mut tick_count = 0u64;

        info!(
            tick_rate = self.config.tick_rate,
            max_ticks = self.config.max_ticks,
            "starting tick loop"
        );

        loop {
            let start = Instant::now();

            let dt = tick_duration.as_secs_f64();
            self.tick(dt);

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
        tick_loop.tick(1.0 / 60.0);
        assert_eq!(tick_loop.tick_id(), 1);
        tick_loop.tick(1.0 / 60.0);
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
}
