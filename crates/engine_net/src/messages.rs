//! Message types exchanged between coordinator and systems.
//!
//! All message types derive `Serialize` and `Deserialize` for MessagePack
//! transport. Routing metadata (tick-id, instance-id, msg-type) is carried
//! in NATS headers — not in the payload.

use engine_component::{ComponentTypeId, Entity, QueryDescriptor};
use serde::{Deserialize, Serialize};

// ── Tick messages ───────────────────────────────────────────────────────────

/// Signals the start of a new simulation tick.
/// Published by the coordinator on [`subjects::COORD_TICK`](crate::subjects::COORD_TICK).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickStart {
    /// Monotonically increasing tick counter.
    pub tick_id: u64,
    /// Delta time since the last tick, in seconds.
    pub dt: f64,
}

/// A system instance acknowledges that it has finished processing a tick.
/// Published on [`subjects::COORD_TICK_DONE`](crate::subjects::COORD_TICK_DONE).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickAck {
    /// The tick that was completed.
    pub tick_id: u64,
    /// The unique instance identifier of the reporting system.
    pub instance_id: String,
}

// ── Entity lifecycle ────────────────────────────────────────────────────────

/// Broadcast when a new entity is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCreated {
    /// The newly allocated entity.
    pub entity: Entity,
    /// The archetype the entity was placed in (by its component type IDs).
    pub archetype: Vec<ComponentTypeId>,
}

/// Broadcast when an entity is destroyed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDestroyed {
    /// The entity that was removed.
    pub entity: Entity,
}

// ── Component data ──────────────────────────────────────────────────────────

/// A batch of component data for a set of entities.
///
/// This is the primary payload for shipping data between the coordinator and
/// system instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentShard {
    /// The component type being transported.
    pub component_type: ComponentTypeId,
    /// Entity IDs in this shard (parallel with `data`).
    pub entities: Vec<Entity>,
    /// MessagePack-encoded component data, one entry per entity.
    pub data: Vec<Vec<u8>>,
}

// ── System management ───────────────────────────────────────────────────────

/// A system registers itself with the coordinator on startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDescriptor {
    /// Human-readable system name (e.g. `"physics"`).
    pub name: String,
    /// The data access requirements of this system.
    pub query: QueryDescriptor,
    /// Unique instance identifier (UUID). Multiple instances of the same
    /// system share the `name` but have distinct `instance_id`s.
    pub instance_id: String,
}

/// The coordinator tells system instance(s) to execute on a given shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSchedule {
    /// The tick this schedule belongs to.
    pub tick_id: u64,
    /// Optional shard range hint (start index, count). Systems may receive
    /// the full archetype if sharding is not yet implemented.
    pub shard_range: Option<(usize, usize)>,
}

/// Periodic health and load report from a system instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    /// The reporting instance.
    pub instance_id: String,
    /// The system name.
    pub system: String,
    /// A load metric (0.0 = idle, 1.0 = fully saturated).
    pub load: f64,
}

// ── Ad-hoc queries ──────────────────────────────────────────────────────────

/// An ad-hoc query request (e.g. from the editor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// The query to execute against the coordinator's world state.
    pub query: QueryDescriptor,
}

/// Response to an ad-hoc query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    /// Matching entities.
    pub entities: Vec<Entity>,
    /// Component shards for each requested type.
    pub shards: Vec<ComponentShard>,
}

// ── NATS header keys ────────────────────────────────────────────────────────

/// Standard NATS header keys used for routing metadata.
pub mod headers {
    /// The message type (e.g. `"tick_start"`, `"system_register"`).
    pub const MSG_TYPE: &str = "msg-type";
    /// The tick ID this message belongs to.
    pub const TICK_ID: &str = "tick-id";
    /// The instance ID of the sender.
    pub const INSTANCE_ID: &str = "instance-id";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_start_roundtrip() {
        let msg = TickStart {
            tick_id: 42,
            dt: 0.016,
        };
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        let restored: TickStart = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(restored.tick_id, 42);
        assert!((restored.dt - 0.016).abs() < f64::EPSILON);
    }

    #[test]
    fn test_system_descriptor_roundtrip() {
        let desc = SystemDescriptor {
            name: "physics".to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: "inst-001".to_string(),
        };
        let bytes = rmp_serde::to_vec(&desc).unwrap();
        let restored: SystemDescriptor = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(restored.name, "physics");
        assert_eq!(restored.query.reads.len(), 1);
        assert_eq!(restored.query.writes.len(), 1);
    }
}
