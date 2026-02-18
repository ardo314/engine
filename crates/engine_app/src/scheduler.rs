//! System scheduler — conflict detection and stage computation.
//!
//! The scheduler groups registered systems into **stages** based on their
//! read/write access sets. Systems within a stage have no conflicts and run
//! in parallel. Stages execute sequentially with a merge barrier between them.

#![allow(dead_code)]

use engine_component::QueryDescriptor;

/// A registered system with its name and query descriptor.
#[derive(Debug, Clone)]
pub struct RegisteredSystem {
    /// The system name (e.g. `"physics"`).
    pub name: String,
    /// The system's data access requirements.
    pub query: QueryDescriptor,
}

/// A stage is a group of systems that can run in parallel (no conflicts).
#[derive(Debug, Clone)]
pub struct Stage {
    /// Indices into the scheduler's system list.
    pub system_indices: Vec<usize>,
}

/// Computes execution stages from a set of registered systems.
///
/// The algorithm is a greedy graph colouring:
/// 1. For each system, check if it conflicts with any system already placed
///    in the current stage.
/// 2. If no conflict, add it to the current stage.
/// 3. If conflict, try the next stage, or create a new one.
///
/// This produces a valid (though not necessarily optimal) stage assignment
/// that guarantees no two conflicting systems run in the same stage.
#[must_use]
pub fn compute_stages(systems: &[RegisteredSystem]) -> Vec<Stage> {
    if systems.is_empty() {
        return Vec::new();
    }

    let mut stages: Vec<Stage> = Vec::new();

    for (sys_idx, system) in systems.iter().enumerate() {
        let mut placed = false;

        for stage in &mut stages {
            // Check if this system conflicts with any system in this stage.
            let conflicts = stage
                .system_indices
                .iter()
                .any(|&existing_idx| system.query.conflicts_with(&systems[existing_idx].query));

            if !conflicts {
                stage.system_indices.push(sys_idx);
                placed = true;
                break;
            }
        }

        if !placed {
            stages.push(Stage {
                system_indices: vec![sys_idx],
            });
        }
    }

    stages
}

#[cfg(test)]
mod tests {
    use engine_component::ComponentTypeId;

    use super::*;

    fn make_system(name: &str, reads: &[u64], writes: &[u64]) -> RegisteredSystem {
        let mut query = QueryDescriptor::new();
        for &r in reads {
            query = query.read(ComponentTypeId(r));
        }
        for &w in writes {
            query = query.write(ComponentTypeId(w));
        }
        RegisteredSystem {
            name: name.to_string(),
            query,
        }
    }

    #[test]
    fn test_no_systems_no_stages() {
        let stages = compute_stages(&[]);
        assert!(stages.is_empty());
    }

    #[test]
    fn test_single_system_one_stage() {
        let systems = vec![make_system("physics", &[1], &[2])];
        let stages = compute_stages(&systems);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].system_indices, vec![0]);
    }

    #[test]
    fn test_non_conflicting_systems_same_stage() {
        // Physics: reads Transform(1), writes Velocity(2)
        // AI: reads Transform(1), writes AiState(3)
        let systems = vec![
            make_system("physics", &[1], &[2]),
            make_system("ai", &[1], &[3]),
        ];
        let stages = compute_stages(&systems);
        assert_eq!(
            stages.len(),
            1,
            "non-conflicting systems should share a stage"
        );
        assert_eq!(stages[0].system_indices.len(), 2);
    }

    #[test]
    fn test_conflicting_systems_different_stages() {
        // Physics: reads Transform(1), writes Velocity(2)
        // Movement: reads Velocity(2), writes Transform(1)
        let systems = vec![
            make_system("physics", &[1], &[2]),
            make_system("movement", &[2], &[1]),
        ];
        let stages = compute_stages(&systems);
        assert_eq!(
            stages.len(),
            2,
            "conflicting systems must be in separate stages"
        );
    }

    #[test]
    fn test_architecture_example_stages() {
        // From ARCHITECTURE.md:
        //   Physics — reads Transform(1), writes Velocity(2)
        //   AI      — reads Transform(1), writes AiState(3)
        //   Movement — reads Velocity(2), writes Transform(1)
        //
        // Expected: Stage 1 = [Physics, AI], Stage 2 = [Movement]
        let systems = vec![
            make_system("physics", &[1], &[2]),
            make_system("ai", &[1], &[3]),
            make_system("movement", &[2], &[1]),
        ];
        let stages = compute_stages(&systems);
        assert_eq!(stages.len(), 2);
        // Stage 1: Physics and AI (no conflict).
        assert_eq!(stages[0].system_indices, vec![0, 1]);
        // Stage 2: Movement (conflicts with both).
        assert_eq!(stages[1].system_indices, vec![2]);
    }
}
