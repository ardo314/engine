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
│   ├── engine_component/   — Core ECS primitives: Component trait, Entity,
│   │                         Archetype storage, Query types, System traits.
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
cluster.

| Subject                             | Direction               | Payload                                         | Purpose                                                                                                                   |
| ----------------------------------- | ----------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `engine.coord.tick`                 | Coordinator → Systems   | `TickStart { tick_id, dt }`                     | Signals start of a new tick.                                                                                              |
| `engine.coord.tick.done`            | Systems → Coordinator   | `TickAck { tick_id, instance_id }`              | System instance acknowledges tick completion.                                                                             |
| `engine.entity.create`              | Coordinator → \*        | `EntityCreated { entity, archetype }`           | Broadcasts entity creation.                                                                                               |
| `engine.entity.destroy`             | Coordinator → \*        | `EntityDestroyed { entity }`                    | Broadcasts entity destruction.                                                                                            |
| `engine.component.set.<system>`     | Coordinator → System(s) | `ComponentShard { entities[], data[] }`         | Sends a batch of component data to the system instance(s). Uses a queue group so shards are distributed across instances. |
| `engine.component.changed.<system>` | Systems → Coordinator   | `ComponentShard { entities[], data[] }`         | System publishes mutated component data back.                                                                             |
| `engine.system.register`            | System → Coordinator    | `SystemDescriptor { name, query, instance_id }` | System registers itself and its query on startup.                                                                         |
| `engine.system.schedule.<system>`   | Coordinator → System(s) | `SystemSchedule { tick_id, shard_range }`       | Tells system instance(s) to execute on the given shard. Delivered via queue group.                                        |
| `engine.system.heartbeat`           | Systems → Coordinator   | `Heartbeat { instance_id, system, load }`       | Periodic health & load report.                                                                                            |
| `engine.query.request`              | Any → Coordinator       | `QueryRequest { query }`                        | Ad-hoc query (e.g. from editor).                                                                                          |
| `engine.query.response`             | Coordinator → Requester | `QueryResponse { entities[], data[] }`          | Response to an ad-hoc query.                                                                                              |

> JetStream is used for `engine.component.*` subjects so that late-joining
> system instances can replay the latest state.

---

## Tick Lifecycle

```
Coordinator                         Systems (one process each)
    │                                  │
    │── 1. Allocate / destroy entities │
    │── 2. Compute dependency graph    │
    │── 3. Assign shards per system    │
    │                                  │
    ├──── engine.coord.tick ──────────►│
    ├──── engine.component.set.* ────►│
    ├──── engine.system.schedule.* ──►│
    │                                  │
    │   ┌────── execute system ────┐   │
    │   │  (one system per process)│   │
    │   └──────────────────────────┘   │
    │                                  │
    │◄── engine.component.changed.* ──┤
    │◄── engine.coord.tick.done ──────┤
    │                                  │
    │── 4. Merge changed components    │
    │── 5. Broadcast events            │
    │── 6. Advance tick                │
    ▼                                  ▼
```

1. **Entity management** — The coordinator processes queued entity
   creation/destruction commands, updates the archetype table, and broadcasts
   `entity.create` / `entity.destroy` events.
2. **Dependency graph** — Systems are topologically sorted based on their
   read/write component sets. Independent systems execute in parallel as
   separate processes.
3. **Shard assignment** — For each system the coordinator determines which
   archetype shards match the system's query. If multiple instances of a
   system exist (queue group), the coordinator publishes one shard per
   instance so work is distributed automatically by NATS.
4. **Component distribution** — The coordinator publishes the relevant
   `component.set.<system>` messages so each system instance has the data it
   needs.
5. **Execution** — Each system process runs on the received shard and publishes
   `component.changed.<system>` messages for any mutated data.
6. **Merge** — The coordinator merges the changed components back into the
   canonical archetype storage. The dependency graph guarantees no two systems
   write to the same component type on the same entity in the same tick, so
   merging is a simple overwrite.
7. **Tick complete** — The coordinator waits for all `tick.done` acks, then
   advances to the next tick.

---

## Coordinator (`engine_app`)

The coordinator is the **single source of truth** for world state. Its
responsibilities:

- **Entity allocation** — Monotonically increasing `u64` IDs. Recycling via a
  free-list is optional and can be added later.
- **Archetype storage** — Canonical SoA (struct-of-arrays) tables for every
  archetype. Stored in-process and replicated to JetStream for persistence.
- **System registry** — Maintains a list of all registered systems, their
  queries, and how many instances are available for each.
- **Scheduler** — Builds a dependency DAG each tick, partitions shards across
  system instances, publishes schedule messages.
- **Merge & conflict resolution** — Applies changed component shards received
  from systems. The dependency graph guarantees no two systems write to the
  same component type on the same entity in the same tick, so merging is a
  simple overwrite.
- **Event bus** — Provides a pub/sub event layer (over NATS) for cross-cutting
  concerns (entity lifecycle, editor notifications, etc.).

### Coordinator Startup Sequence

1. Connect to NATS (configurable URL, default `nats://localhost:4222`).
2. Create JetStream streams for component persistence.
3. Subscribe to `engine.system.register`.
4. Enter tick loop (fixed timestep, configurable Hz).

---

## System Process (`engine_system`)

A system is both the logic and the process. The `engine_system` crate provides
a runtime harness that turns a system function into a standalone NATS-connected
process. Systems are stateless — they receive data, compute, and return results.

### Lifecycle

1. Connect to NATS and publish a `system.register` message declaring its
   `instance_id`, system `name`, and `QueryDescriptor`.
2. Subscribe to `engine.system.schedule.<system>` using a **queue group** named
   after the system (e.g. `q.physics`). This means NATS automatically
   load-balances shard messages across all instances of this system.
3. On each tick:
   a. Receive a `SystemSchedule` message with the shard range.
   b. Receive component data via `engine.component.set.<system>`.
   c. Deserialise into local archetype tables.
   d. Execute the system function.
   e. Serialise and publish changed component data via
   `engine.component.changed.<system>`.
   f. Ack the tick via `engine.coord.tick.done`.

### Horizontal Scaling

To scale a computationally expensive system (e.g. physics), launch additional
instances of the same system binary. Because they share a NATS queue group, the
coordinator's shard messages are automatically distributed:

```
                ┌─────────────────┐
                │   Coordinator   │
                └────────┬────────┘
                         │
        engine.system.schedule.physics
                (queue: q.physics)
                         │
            ┌────────────┼────────────┐
            ▼            ▼            ▼
      ┌──────────┐ ┌──────────┐ ┌──────────┐
      │ Physics  │ │ Physics  │ │ Physics  │
      │   (#1)   │ │   (#2)   │ │   (#3)   │
      └──────────┘ └──────────┘ └──────────┘
```

No coordinator changes are needed — add or remove instances at any time.

---

## Serialisation

All messages are serialised with **MessagePack** (`rmp-serde`) for compact
binary encoding. The envelope format:

```
┌────────────┬───────────────────────────┐
│ NATS Hdrs  │ MessagePack payload       │
├────────────┼───────────────────────────┤
│ msg-type   │ { ... message fields }    │
│ tick-id    │                           │
│ instance-id│                           │
└────────────┴───────────────────────────┘
```

NATS headers carry routing metadata so systems can filter without deserialising
the payload.

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
2. Build the dependency graph (read/write sets).
3. Determine which component columns to ship to system instances.

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

| Failure           | Mitigation                                                                                                                                                                                                                                                |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| System crash      | Coordinator detects missing `tick.done` ack within timeout → if other instances of the same system exist in the queue group, NATS already routed their shards. If no instances remain, the coordinator skips that system for the tick and logs a warning. |
| Coordinator crash | JetStream retains the last-known component state. A new coordinator replays from the stream and resumes.                                                                                                                                                  |
| NATS disconnect   | `async-nats` reconnects automatically. Systems buffer locally and retry.                                                                                                                                                                                  |
| Slow system       | Scaling the system horizontally (more instances in the queue group) distributes load. Coordinator enforces a tick deadline; if an instance misses it, its results are dropped and the tick proceeds.                                                      |

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

4. **MessagePack over JSON / Protobuf** — Compact binary format with no schema
   compilation step. Faster to serialise/deserialise than JSON, simpler to
   integrate than Protobuf in a Rust-first codebase.

5. **Archetype-based storage** — Cache-friendly SoA layout is proven in ECS
   literature (see: Flecs, Bevy). Distributing entire archetype shards (rather
   than per-entity messages) amortises network overhead.

6. **Fixed tick loop** — Deterministic simulation. Variable-rate rendering can
   be layered on top by interpolating between ticks.

---

## Glossary

| Term            | Definition                                                                                 |
| --------------- | ------------------------------------------------------------------------------------------ |
| **Archetype**   | A unique combination of component types.                                                   |
| **Coordinator** | The `engine_app` process that owns world state and schedules systems.                      |
| **Entity**      | A `u64` identifier with no inherent data.                                                  |
| **Instance**    | One OS process running a system. Multiple instances of the same system form a queue group. |
| **Query**       | A declarative description of component access requirements.                                |
| **Shard**       | A contiguous slice of rows from an archetype table, sent to a system instance.             |
| **System**      | A function that operates on entities matching a query, running as its own process.         |
| **Tick**        | One discrete simulation step.                                                              |
