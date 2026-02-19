//! Per-tick execution context provided to system functions.

use engine_component::{Component, Entity};
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
    /// Component shards received from the coordinator.
    pub input_shards: Vec<ComponentShard>,
    /// Component shards to publish back (modified data).
    pub output_shards: Vec<ComponentShard>,
}

impl SystemContext {
    /// Create a new context for a tick.
    #[must_use]
    pub fn new(tick_id: u64) -> Self {
        Self {
            tick_id,
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

    /// Read all instances of a component type `T` from the input shards.
    ///
    /// Returns a list of `(Entity, T)` pairs. Entities that fail to
    /// deserialise are silently skipped.
    pub fn read_components<T: Component>(&self) -> Vec<(Entity, T)> {
        let target = T::component_type_id();
        let mut result = Vec::new();
        for shard in &self.input_shards {
            if shard.component_type != target {
                continue;
            }
            for (entity, data) in shard.entities.iter().zip(shard.data.iter()) {
                if let Ok(value) = rmp_serde::from_slice::<T>(data) {
                    result.push((*entity, value));
                }
            }
        }
        result
    }

    /// Publish changed component data for type `T`.
    ///
    /// Takes a list of `(Entity, T)` pairs, serialises them, and adds the
    /// resulting shard to the output.
    pub fn write_changed<T: Component>(&mut self, components: &[(Entity, T)]) {
        if components.is_empty() {
            return;
        }
        let mut entities = Vec::with_capacity(components.len());
        let mut data = Vec::with_capacity(components.len());
        for (entity, value) in components {
            if let Ok(bytes) = rmp_serde::to_vec(value) {
                entities.push(*entity);
                data.push(bytes);
            }
        }
        if !entities.is_empty() {
            self.output_shards.push(ComponentShard {
                component_type: T::component_type_id(),
                entities,
                data,
            });
        }
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
        let ctx = SystemContext::new(1);
        assert_eq!(ctx.tick_id, 1);
        assert!(ctx.input_shards.is_empty());
        assert!(ctx.output_shards.is_empty());
    }

    #[test]
    fn test_publish_changed() {
        let mut ctx = SystemContext::new(1);
        ctx.publish_changed(ComponentShard {
            component_type: ComponentTypeId(1),
            entities: vec![Entity::from_raw(1)],
            data: vec![vec![0u8; 4]],
        });
        assert_eq!(ctx.output_shards.len(), 1);
    }

    #[test]
    fn test_read_write_typed_roundtrip() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct Vel {
            x: f32,
            y: f32,
        }
        impl Component for Vel {
            fn type_name() -> &'static str {
                "Vel"
            }
        }

        let entity = Entity::from_raw(42);
        let vel = Vel { x: 1.0, y: 2.0 };

        // Build an input shard.
        let shard = ComponentShard {
            component_type: Vel::component_type_id(),
            entities: vec![entity],
            data: vec![rmp_serde::to_vec(&vel).unwrap()],
        };

        let mut ctx = SystemContext::new(1);
        ctx.input_shards.push(shard);

        // Read back.
        let components = ctx.read_components::<Vel>();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].0, entity);
        assert_eq!(components[0].1, vel);

        // Modify and write.
        let modified: Vec<(Entity, Vel)> = components
            .into_iter()
            .map(|(e, mut v)| {
                v.x += 10.0;
                (e, v)
            })
            .collect();
        ctx.write_changed(&modified);

        assert_eq!(ctx.output_shards.len(), 1);
        assert_eq!(ctx.output_shards[0].entities, vec![entity]);
    }
}
