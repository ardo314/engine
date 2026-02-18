//! Per-tick execution context provided to system functions.

use engine_component::Entity;
use engine_net::messages::ComponentShard;

/// Context provided to a system function on each tick.
///
/// Contains the component data the system has been assigned, along with
/// tick metadata. After execution, the system marks which shards have
/// been modified so only changed data is published back.
#[derive(Debug)]
pub struct SystemContext {
    /// The current tick ID.
    pub tick_id: u64,
    /// Delta time since the last tick, in seconds.
    pub dt: f64,
    /// Component shards received from the coordinator.
    pub input_shards: Vec<ComponentShard>,
    /// Component shards to publish back (modified data).
    pub output_shards: Vec<ComponentShard>,
}

impl SystemContext {
    /// Create a new context for a tick.
    #[must_use]
    pub fn new(tick_id: u64, dt: f64) -> Self {
        Self {
            tick_id,
            dt,
            input_shards: Vec::new(),
            output_shards: Vec::new(),
        }
    }

    /// Get all entities from the input shards.
    #[must_use]
    pub fn entities(&self) -> Vec<Entity> {
        let mut all = Vec::new();
        for shard in &self.input_shards {
            all.extend_from_slice(&shard.entities);
        }
        all.sort();
        all.dedup();
        all
    }

    /// Publish a modified component shard to be sent back to the coordinator.
    pub fn publish_changed(&mut self, shard: ComponentShard) {
        self.output_shards.push(shard);
    }
}

#[cfg(test)]
mod tests {
    use engine_component::ComponentTypeId;

    use super::*;

    #[test]
    fn test_context_creation() {
        let ctx = SystemContext::new(1, 0.016);
        assert_eq!(ctx.tick_id, 1);
        assert!((ctx.dt - 0.016).abs() < f64::EPSILON);
        assert!(ctx.input_shards.is_empty());
        assert!(ctx.output_shards.is_empty());
    }

    #[test]
    fn test_publish_changed() {
        let mut ctx = SystemContext::new(1, 0.016);
        ctx.publish_changed(ComponentShard {
            component_type: ComponentTypeId(1),
            entities: vec![Entity::from_raw(1)],
            data: vec![vec![0u8; 4]],
        });
        assert_eq!(ctx.output_shards.len(), 1);
    }
}
