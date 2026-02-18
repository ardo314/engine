//! NATS subject hierarchy.
//!
//! All engine subjects are prefixed with `engine.` to namespace within a
//! shared NATS cluster. See `ARCHITECTURE.md` for the full hierarchy.

/// Root prefix for all engine NATS subjects.
pub const PREFIX: &str = "engine";

// ── Coordinator subjects ────────────────────────────────────────────────────

/// Signals the start of a new tick. Coordinator → Systems.
pub const COORD_TICK: &str = "engine.coord.tick";

/// System acknowledges tick completion. Systems → Coordinator.
pub const COORD_TICK_DONE: &str = "engine.coord.tick.done";

// ── Entity lifecycle ────────────────────────────────────────────────────────

/// Broadcasts entity creation. Coordinator → *.
pub const ENTITY_CREATE: &str = "engine.entity.create";

/// Broadcasts entity destruction. Coordinator → *.
pub const ENTITY_DESTROY: &str = "engine.entity.destroy";

// ── System management ───────────────────────────────────────────────────────

/// System registers itself. System → Coordinator.
pub const SYSTEM_REGISTER: &str = "engine.system.register";

/// Periodic heartbeat. Systems → Coordinator.
pub const SYSTEM_HEARTBEAT: &str = "engine.system.heartbeat";

// ── Query (ad-hoc) ──────────────────────────────────────────────────────────

/// Ad-hoc query request (e.g. from editor). Any → Coordinator.
pub const QUERY_REQUEST: &str = "engine.query.request";

/// Ad-hoc query response. Coordinator → Requester.
pub const QUERY_RESPONSE: &str = "engine.query.response";

// ── Dynamic subject builders ────────────────────────────────────────────────

/// Build the subject for sending component data to a specific system.
///
/// `engine.component.set.<system_name>`
#[must_use]
pub fn component_set(system_name: &str) -> String {
    format!("engine.component.set.{system_name}")
}

/// Build the subject for receiving changed component data from a system.
///
/// `engine.component.changed.<system_name>`
#[must_use]
pub fn component_changed(system_name: &str) -> String {
    format!("engine.component.changed.{system_name}")
}

/// Build the subject for scheduling a specific system.
///
/// `engine.system.schedule.<system_name>`
#[must_use]
pub fn system_schedule(system_name: &str) -> String {
    format!("engine.system.schedule.{system_name}")
}

/// Build the queue group name for a system's instances.
///
/// `q.<system_name>`
#[must_use]
pub fn queue_group(system_name: &str) -> String {
    format!("q.{system_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_set_subject() {
        assert_eq!(component_set("physics"), "engine.component.set.physics");
    }

    #[test]
    fn test_component_changed_subject() {
        assert_eq!(
            component_changed("physics"),
            "engine.component.changed.physics"
        );
    }

    #[test]
    fn test_system_schedule_subject() {
        assert_eq!(system_schedule("physics"), "engine.system.schedule.physics");
    }

    #[test]
    fn test_queue_group_name() {
        assert_eq!(queue_group("physics"), "q.physics");
    }
}
