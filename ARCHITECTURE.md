# Architecture — Distributed ECS

## Overview

This engine implements a **distributed Entity Component System (ECS)** where the
world state is spread across multiple processes that communicate over
[NATS](https://nats.io). The `engine_app` crate acts as the **central
coordinator** — it owns the canonical entity table, registers systems and
queries, orchestrates tick execution, and brokers component data between system
processes.

Each **system** is both the logic _and_ the process that runs it — there is no
separate "worker" concept. A system is a standalone process that connects to
NATS, declares its query, receives component shards, executes, and publishes
results. Horizontal scaling is achieved by launching multiple instances of the
same system behind a NATS queue group — the coordinator distributes archetype
shards across instances automatically.

```
┌──────────────────────────────────────────────────────────────┐
│                        NATS Cluster                          │
└──┬──────────┬──────────┬──────────┬──────────┬────────┬──┬──┘
   │          │          │          │          │        │  │
   ▼          ▼          ▼          ▼          ▼        ▼  ▼
┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐ ┌────────┐
│Physics│ │Physics│ │  AI   │ │Render │ │  …    │ │ Editor │
│ (#1)  │ │ (#2)  │ │       │ │Prep   │ │       │ │(Tauri) │
└───────┘ └───────┘ └───────┘ └───────┘ └───────┘ └────────┘
     ▲         ▲         ▲
     │         │         │
     └─────────┴─────────┘
               │
     ┌─────────┴─────────┐
     │    engine_app      │
     │   (Coordinator)    │
     └────────────────────┘
```

> Instances of the same system (e.g. Physics #1 and #2) form a **NATS queue
> group** so the coordinator can scatter shards across them.

---

## Core Concepts

### Entity

A unique `u64` identifier allocated by the coordinator. Entities have no data of
their own — they are pure identifiers that components are attached to.

### Component

A serialisable piece of data attached to an entity (e.g. `Transform3D`,
`Mesh`). Components must implement the `Component` trait **and**
`serde::Serialize + serde::Deserialize` so they can travel over the wire.

### Archetype

A unique combination of component types. Entities with the same set of
components are stored together for cache-friendly iteration. Each archetype is
identified by a deterministic hash of its sorted component type IDs.

### System

A function that operates on a **query** — a filtered view of entities and their
components. Each system runs as its own process. A system connects to NATS,
declares its query to the coordinator, receives matching component shards,
executes, and publishes changed data back. Multiple instances of the same
system can be launched to parallelise work across archetype shards via NATS
queue groups.

### Query

A declarative description of which component types a system needs, and whether
it needs them mutably or immutably. The coordinator uses queries to compute
data dependencies and schedule systems with maximum parallelism.

---

## Crate Map

```
engine/
├── crates/
│   ├── engine_app/         — Coordinator binary. Entity allocation, system
│   │                         registry, tick loop, NATS connection management.
│   ├── engine_component/   — Component trait and derive utilities. The "C"
│   │                         in ECS — defines what a component is and how
│   │                         it is stored and serialised.
│   ├── engine_math/        — Math types (re-exports glam). Transform, AABB, etc.
│   ├── engine_net/         — (new) NATS transport layer. Serialisation,
│   │                         subjects, request/reply helpers, JetStream
│   │                         persistence.
│   └── engine_system/      — (new) System runtime library. Provides the
│                              harness for running a system as a process:
│                              NATS connection, registration, shard
│                              receive/publish loop.
├── engine_editor/          — Tauri desktop editor. Connects to coordinator
│                              over NATS for live inspection / authoring.
└── examples/
    └── components/         — Example component definitions.
```

### New Crates

| Crate           | Purpose                                                                                                                                                                                                                                                                                                                                                               |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `engine_net`    | Thin abstraction over `async-nats`. Defines NATS subject hierarchy, message envelope (header + payload), codec (MessagePack via `rmp-serde`), and helpers for request/reply and JetStream streams.                                                                                                                                                                    |
| `engine_system` | System runtime library. Provides the harness that turns a system function into a standalone NATS-connected process: connects, registers the system's query with the coordinator, receives component shards, invokes the system function, and publishes changed components back. Multiple instances of the same system form a NATS queue group for horizontal scaling. |

---

## NATS Subject Hierarchy

All subjects are prefixed with `engine.` to namespace within a shared NATS
cluster. The subjects fall into four groups:

- **Tick coordination** — `engine.coord.tick`, `engine.coord.tick.done`
- **Entity lifecycle** — `engine.entity.create`, `engine.entity.destroy`,
  `engine.entity.spawn.request`
- **Component data** — `engine.component.set.<system>`,
  `engine.component.changed.<system>` (with sentinel messages for
  end-of-stream signalling)
- **System management** — `engine.system.register`,
  `engine.system.unregister`, `engine.system.schedule.<system>`,
  `engine.system.heartbeat`
- **Ad-hoc queries** — `engine.query.request`, `engine.query.response`

> JetStream is used for `engine.component.*` subjects so that late-joining
> system instances can replay the latest state.

For the complete subject table, message schemas, header conventions, sentinel
protocol, and sequence diagrams, see **`PROTOCOL.md`**.

---

## Tick Lifecycle

```
Coordinator                         Systems (one process each)
    │                                  │
    │── 0. Apply pending system        │
    │      register/unregister changes │
    │── 1. Allocate / destroy entities │
    │── 2. Build dependency graph      │
    │── 3. Compute execution stages    │
    │                                  │
    │   ┌─── Stage 1 (parallel) ───┐   │
    │   │  Systems with no conflicts│   │
    ├──►│  run concurrently         │   │
    │   └──────────────────────────┘   │
    │── 4a. Merge stage 1 results      │
    │                                  │
    │   ┌─── Stage 2 (parallel) ───┐   │
    │   │  Next conflict-free set   │   │
    ├──►│  runs concurrently        │   │
    │   └──────────────────────────┘   │
    │── 4b. Merge stage 2 results      │
    │                                  │
    │   ┌─── Stage N … ───────────┐   │
    │   │  ...                     │   │
    │   └──────────────────────────┘   │
    │── 4n. Merge stage N results      │
    │                                  │
    │── 5. Broadcast events            │
    │── 6. Advance tick                │
    ▼                                  ▼
```

### Scheduling Algorithm

The coordinator groups systems into **stages** using their `QueryDescriptor`
read/write sets:

1. Two systems **conflict** if one writes a component type that the other
   reads or writes. Formally, systems A and B conflict when:
   `A.writes ∩ (B.reads ∪ B.writes) ≠ ∅` or
   `B.writes ∩ (A.reads ∪ A.writes) ≠ ∅`.
2. Systems that do **not** conflict with each other are placed in the same
   stage and execute **in parallel** (as separate processes, across NATS).
3. Systems that **do** conflict are placed in different stages that run
   **sequentially**. The coordinator waits for all systems in a stage to
   complete and merges their results before starting the next stage.
4. Within a stage, systems are topologically sorted so that if ordering
   constraints exist (explicit dependencies), they are respected by placing
   them in earlier/later stages.

### Example

Given three systems:

- **Physics** — reads `Transform3D`, writes `Velocity`
- **AI** — reads `Transform3D`, writes `AiState`
- **Movement** — reads `Velocity`, writes `Transform3D`

The scheduler produces:

| Stage | Systems     | Reason                                                                                                                |
| ----- | ----------- | --------------------------------------------------------------------------------------------------------------------- |
| 1     | Physics, AI | No conflict — Physics writes `Velocity`, AI writes `AiState`. Both only read `Transform3D`.                           |
| 2     | Movement    | Conflicts with Physics (`Velocity` read vs write) and with both (`Transform3D` write vs read). Must wait for stage 1. |

Physics and AI run in parallel. Movement runs after both complete.

### Step-by-step

0. **Apply pending system changes** — The coordinator drains the pending
   register/unregister queue, updating the system registry. Systems may
   register or unregister at any time; changes are queued and applied
   atomically before the next tick starts, ensuring the system set never
   changes mid-tick.
1. **Entity management** — The coordinator drains queued
   `EntitySpawnRequest` messages from systems, allocates entity IDs, writes
   component data into archetype tables, and broadcasts `entity.create` /
   `entity.destroy` events.
2. **Dependency graph** — The coordinator builds a conflict graph from the
   read/write sets of all registered systems.
3. **Stage computation** — The conflict graph is partitioned into stages.
   Systems within a stage have no conflicts and run in parallel. Stages
   execute sequentially.
4. **Per-stage execution** — For each stage the coordinator sends component
   data to each system, the systems execute and publish their changes back,
   and the coordinator merges the results before proceeding to the next
   stage. (See the **Per-Tick Execution** sequence in `PROTOCOL.md` for the
   exact message exchange, sentinel protocol, and ack flow.)
5. **Broadcast events** — After all stages complete, the coordinator
   broadcasts any deferred events.
6. **Advance tick** — The coordinator increments the tick counter and loops.

---

## Coordinator (`engine_app`)

The coordinator is the **single source of truth** for world state. Its
responsibilities:

- **Entity allocation** — Monotonically increasing `u64` IDs. Recycling via a
  free-list is optional and can be added later.
- **Archetype storage** — Canonical SoA (struct-of-arrays) tables for every
  archetype. Stored in-process and replicated to JetStream for persistence.
- **System registry** — Maintains a list of all registered systems, their
  queries, and how many instances are available for each. Systems may register
  or unregister at any time via NATS; changes are queued and applied
  atomically before the next tick starts.
- **Scheduler** — Builds a conflict graph from system read/write sets each
  tick. Partitions systems into stages: systems within a stage run in
  parallel (no read/write conflicts), stages run sequentially with a merge
  barrier between them.
- **Merge & conflict resolution** — After each stage, applies changed
  component shards from all systems in that stage. Because systems within a
  stage have no conflicting writes, merging is a simple overwrite.
- **Event bus** — Provides a pub/sub event layer (over NATS) for cross-cutting
  concerns (entity lifecycle, editor notifications, etc.).

### Coordinator Startup Sequence

1. Connect to NATS (configurable URL, default `nats://localhost:4222`).
2. Create JetStream streams for component persistence.
3. Subscribe to `engine.system.register` and `engine.system.unregister`.
4. Enter tick loop (fixed timestep, configurable Hz). Register/unregister
   requests that arrive between ticks are queued and applied before the
   next tick begins.

---

## System Process (`engine_system`)

A system is both the logic and the process. The `engine_system` crate provides
a runtime harness that turns a system function into a standalone NATS-connected
process. Systems are stateless — they receive data, compute, and return results.

### Lifecycle

1. **Register** — Connect to NATS and publish a `system.register` message
   declaring `instance_id`, system `name`, and `QueryDescriptor`. The
   coordinator queues the registration and applies it before the next tick.
2. **Subscribe** — Subscribe to the system's schedule and data subjects
   using a NATS queue group for load balancing.
3. **Per-tick loop** — Receive component data shards, execute the system
   function, publish changed data back, and acknowledge tick completion.
4. **Shutdown** — Publish a `system.unregister` message so the coordinator
   removes the instance before the next tick.

For the exact message exchange (subjects, sentinels, ack flow), see the
**Per-Tick Execution** sequence in `PROTOCOL.md`.

### Horizontal Scaling

To scale a computationally expensive system (e.g. physics), launch additional
instances of the same system binary. They share a NATS queue group
(`q.<system>`), so the coordinator's shard messages are automatically
distributed — no coordinator changes needed.

See **Queue Groups & Horizontal Scaling** in `PROTOCOL.md` for details.

---

## Serialisation

All messages are serialised with **MessagePack** (`rmp-serde`) for compact
binary encoding. NATS headers carry routing metadata (`msg-type`, `tick-id`,
`instance-id`) so systems can filter without deserialising the payload.

For the full envelope format, encoding rules, and header key reference, see
**`PROTOCOL.md`**.

---

## Component Storage

### Canonical (Coordinator)

The coordinator stores components in **archetype tables** — a hashmap keyed by
archetype ID, where each value is a struct-of-arrays (SoA) table:

```rust
struct ArchetypeTable {
    /// Sorted list of ComponentTypeIds that define this archetype.
    component_types: Vec<ComponentTypeId>,
    /// Entity IDs in insertion order.
    entities: Vec<Entity>,
    /// One column per component type, stored as type-erased byte vectors.
    columns: Vec<BlobVec>,
}
```

### System-local (Transient)

Systems receive a **shard** — a subset of rows from one or more archetype
tables. They deserialise into the same `ArchetypeTable` layout for cache-
friendly iteration, execute, then serialise only the **changed** columns back.

---

## Query System

Queries describe what data a system needs:

```rust
Query<(&Transform3D, &mut Velocity, Option<&Mass>)>
```

This is compiled at system-registration time into a `QueryDescriptor`:

```rust
struct QueryDescriptor {
    reads:    Vec<ComponentTypeId>,
    writes:   Vec<ComponentTypeId>,
    optionals: Vec<ComponentTypeId>,
    filters:  Vec<Filter>,          // With<T>, Without<T>, Changed<T>
}
```

The coordinator uses `QueryDescriptor` to:

1. Match archetypes that satisfy the query.
2. Detect read/write conflicts between systems and partition them into
   sequential stages (see [Tick Lifecycle](#tick-lifecycle)).
3. Determine which component columns to ship to system instances.

Two systems conflict when their access sets overlap with at least one write:

- `&T` vs `&T` — **no conflict** (both read).
- `&T` vs `&mut T` — **conflict** (read vs write).
- `&mut T` vs `&mut T` — **conflict** (write vs write).

---

## Editor Integration

The Tauri-based editor (`engine_editor/`) connects to the coordinator via NATS
(using the same `engine_net` crate compiled to WASM + `nats.ws`). This allows
the editor to:

- **Inspect** entities and components in real-time via `engine.query.request`.
- **Modify** components by publishing `engine.component.changed.*` with editor
  authority.
- **Create / destroy** entities by sending commands to the coordinator.
- **Observe** the system schedule and per-system load.

---

## Persistence (Future)

NATS JetStream provides durable streams for component data. A snapshot of the
full world state can be taken by:

1. Pausing the tick loop.
2. Publishing all archetype tables to a dedicated `engine.snapshot.<id>`
   stream.
3. Resuming.

Systems can replay from the snapshot stream to reconstruct world state after a
crash.

---

## Error Handling & Resilience

The system is designed for graceful degradation:

- **System crash** — Coordinator detects missing tick ack. If other instances
  exist in the queue group, NATS already rerouted shards. Otherwise the system
  is skipped for that tick.
- **Coordinator crash** — JetStream retains component state; a new coordinator
  replays from the stream and resumes.
- **NATS disconnect** — `async-nats` reconnects automatically.
- **Slow system** — Scale horizontally or let the tick deadline drop results.

For protocol-level error types (`NetError` variants) and timeout values, see
**`PROTOCOL.md`**.

---

## Dependencies (Planned)

| Crate        | Version | Purpose                                              |
| ------------ | ------- | ---------------------------------------------------- |
| `async-nats` | 0.38+   | NATS client                                          |
| `rmp-serde`  | 1.x     | MessagePack serialisation                            |
| `serde`      | 1.x     | Serialisation framework                              |
| `tokio`      | 1.x     | Async runtime                                        |
| `glam`       | 0.29+   | Math (already used via `engine_math`)                |
| `tracing`    | 0.1     | Structured logging                                   |
| `dashmap`    | 6.x     | Concurrent hashmap for coordinator archetype storage |

---

## Design Decisions & Rationale

1. **NATS over gRPC / TCP sockets** — NATS provides built-in pub/sub,
   request/reply, load-balanced queue groups, and JetStream persistence with
   minimal boilerplate. This avoids hand-rolling a message broker.

2. **System = process** — There is no separate "worker" concept. Each system
   is both the logic and the process that runs it. This eliminates a layer of
   indirection, makes the architecture easier to reason about, and keeps each
   process trivially simple. Horizontal scaling is handled by launching more
   instances in the same NATS queue group — no scheduler changes needed. Failure
   domains are small: a crash in one system cannot affect another.

3. **Coordinator as single authority** — Simplifies entity allocation and
   conflict resolution. The coordinator is not a bottleneck because it only
   manages metadata and schedules — heavy computation happens in systems.

4. **Staged scheduling** — Systems are grouped into stages based on
   component-level read/write conflict detection. Systems with no conflicts
   run in parallel; conflicting systems are sequentialised across stages with
   a merge barrier in between. This maximises parallelism while guaranteeing
   data-race freedom without locks — the schedule itself _is_ the
   synchronisation mechanism.

5. **MessagePack over JSON / Protobuf** — Compact binary format with no schema
   compilation step. Faster to serialise/deserialise than JSON, simpler to
   integrate than Protobuf in a Rust-first codebase.

6. **Archetype-based storage** — Cache-friendly SoA layout is proven in ECS
   literature (see: Flecs, Bevy). Distributing entire archetype shards (rather
   than per-entity messages) amortises network overhead.

7. **Fixed tick loop** — Deterministic simulation. Variable-rate rendering can
   be layered on top by interpolating between ticks.

---

## Glossary

| Term            | Definition                                                                                                     |
| --------------- | -------------------------------------------------------------------------------------------------------------- |
| **Archetype**   | A unique combination of component types.                                                                       |
| **Coordinator** | The `engine_app` process that owns world state and schedules systems.                                          |
| **Entity**      | A `u64` identifier with no inherent data.                                                                      |
| **Instance**    | One OS process running a system. Multiple instances of the same system form a queue group.                     |
| **Query**       | A declarative description of component access requirements.                                                    |
| **Shard**       | A contiguous slice of rows from an archetype table, sent to a system instance.                                 |
| **Stage**       | A group of systems with no read/write conflicts, executing in parallel. Stages run sequentially within a tick. |
| **System**      | A function that operates on entities matching a query, running as its own process.                             |
| **Tick**        | One discrete simulation step.                                                                                  |
