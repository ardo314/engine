//! System registry — tracks registered systems and their instances.
//!
//! The coordinator maintains a registry of all systems that have connected
//! and declared their queries. Multiple instances of the same system share
//! a name but have distinct instance IDs.

#![allow(dead_code)]

use std::collections::HashMap;

use engine_component::QueryDescriptor;
use engine_net::messages::SystemDescriptor;

/// Information about a registered system (one logical system, potentially
/// with multiple instances).
#[derive(Debug, Clone)]
pub struct SystemInfo {
    /// The system's human-readable name.
    pub name: String,
    /// The system's data access requirements.
    pub query: QueryDescriptor,
    /// Instance IDs of all running instances of this system.
    pub instances: Vec<String>,
}

/// Registry of all systems known to the coordinator.
#[derive(Debug, Default)]
pub struct SystemRegistry {
    /// Systems keyed by name.
    systems: HashMap<String, SystemInfo>,
}

impl SystemRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            systems: HashMap::new(),
        }
    }

    /// Register a system instance. If the system name already exists, the
    /// instance ID is added to its instance list. If it is new, a new entry
    /// is created.
    pub fn register(&mut self, descriptor: SystemDescriptor) {
        let entry = self
            .systems
            .entry(descriptor.name.clone())
            .or_insert_with(|| SystemInfo {
                name: descriptor.name.clone(),
                query: descriptor.query.clone(),
                instances: Vec::new(),
            });
        if !entry.instances.contains(&descriptor.instance_id) {
            entry.instances.push(descriptor.instance_id);
        }
    }

    /// Remove a specific instance from the registry.
    ///
    /// Returns `true` if the instance was found and removed.
    pub fn unregister_instance(&mut self, system_name: &str, instance_id: &str) -> bool {
        if let Some(info) = self.systems.get_mut(system_name)
            && let Some(pos) = info.instances.iter().position(|id| id == instance_id)
        {
            info.instances.remove(pos);
            // If no instances remain, remove the system entirely.
            if info.instances.is_empty() {
                self.systems.remove(system_name);
            }
            return true;
        }
        false
    }

    /// Returns information about a system by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SystemInfo> {
        self.systems.get(name)
    }

    /// Returns an iterator over all registered systems.
    pub fn iter(&self) -> impl Iterator<Item = &SystemInfo> {
        self.systems.values()
    }

    /// Returns the number of distinct system types registered.
    #[must_use]
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Returns the total number of instances across all systems.
    #[must_use]
    pub fn total_instances(&self) -> usize {
        self.systems.values().map(|s| s.instances.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use engine_component::{ComponentTypeId, QueryDescriptor};
    use engine_net::messages::SystemDescriptor;

    use super::*;

    fn make_descriptor(name: &str, instance_id: &str) -> SystemDescriptor {
        SystemDescriptor {
            name: name.to_string(),
            query: QueryDescriptor::new()
                .read(ComponentTypeId(1))
                .write(ComponentTypeId(2)),
            instance_id: instance_id.to_string(),
        }
    }

    #[test]
    fn test_register_new_system() {
        let mut registry = SystemRegistry::new();
        registry.register(make_descriptor("physics", "inst-1"));
        assert_eq!(registry.system_count(), 1);
        assert_eq!(registry.total_instances(), 1);
    }

    #[test]
    fn test_register_multiple_instances() {
        let mut registry = SystemRegistry::new();
        registry.register(make_descriptor("physics", "inst-1"));
        registry.register(make_descriptor("physics", "inst-2"));
        assert_eq!(registry.system_count(), 1);
        assert_eq!(registry.total_instances(), 2);
    }

    #[test]
    fn test_register_different_systems() {
        let mut registry = SystemRegistry::new();
        registry.register(make_descriptor("physics", "inst-1"));
        registry.register(make_descriptor("ai", "inst-2"));
        assert_eq!(registry.system_count(), 2);
        assert_eq!(registry.total_instances(), 2);
    }

    #[test]
    fn test_unregister_instance() {
        let mut registry = SystemRegistry::new();
        registry.register(make_descriptor("physics", "inst-1"));
        registry.register(make_descriptor("physics", "inst-2"));
        assert!(registry.unregister_instance("physics", "inst-1"));
        assert_eq!(registry.total_instances(), 1);
    }

    #[test]
    fn test_unregister_last_instance_removes_system() {
        let mut registry = SystemRegistry::new();
        registry.register(make_descriptor("physics", "inst-1"));
        assert!(registry.unregister_instance("physics", "inst-1"));
        assert_eq!(registry.system_count(), 0);
    }

    #[test]
    fn test_duplicate_instance_id_not_added() {
        let mut registry = SystemRegistry::new();
        registry.register(make_descriptor("physics", "inst-1"));
        registry.register(make_descriptor("physics", "inst-1"));
        assert_eq!(registry.total_instances(), 1);
    }
}
