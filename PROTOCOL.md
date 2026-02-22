# PROTOCOL.md — Wire Protocol Reference

This document is the definitive reference for the NATS-based wire protocol used
by the distributed ECS engine. It describes every subject, message type, header
convention, serialisation format, and the exact sequence of messages exchanged
during system registration, tick execution, entity lifecycle, and ad-hoc
queries.

Related documents:

- **`ARCHITECTURE.md`** — High-level design, crate responsibilities, scheduling
  algorithm, component storage, query system, and design rationale.
- **`AGENTS.md`** — Coding conventions, style rules, and instructions for AI
  agents working on this codebase.

---

## Table of Contents

- [Transport](#transport)
- [Serialisation](#serialisation)
- [NATS Headers](#nats-headers)
- [Subject Hierarchy](#subject-hierarchy)
- [Message Types](#message-types)
  - [TickStart](#tickstart)
  - [TickAck](#tickack)
  - [EntityCreated](#entitycreated)
  - [EntityDestroyed](#entitydestroyed)
  - [EntitySpawnRequest](#entityspawnrequest)
  - [ComponentShard](#componentshard)
  - [DataDone](#datadone)
  - [ChangesDone](#changesdone)
  - [SystemDescriptor](#systemdescriptor)
  - [SystemUnregister](#systemunregister)
  - [SystemSchedule](#systemschedule)
  - [Heartbeat](#heartbeat)
  - [QueryRequest](#queryrequest)
  - [QueryResponse](#queryresponse)
  - [ComponentSchema](#componentschema)
  - [SchemaRequest](#schemarequest)
  - [SchemaResponse](#schemaresponse)
- [Sequences](#sequences)
  - [System Registration](#system-registration)
  - [Per-Tick Execution](#per-tick-execution)
  - [Entity Spawning](#entity-spawning)
  - [System Shutdown](#system-shutdown)
  - [Ad-hoc Query](#ad-hoc-query)
- [Sentinel Protocol](#sentinel-protocol)
- [Queue Groups & Horizontal Scaling](#queue-groups--horizontal-scaling)
- [JetStream Persistence](#jetstream-persistence)
- [Error Handling](#error-handling)
- [Timeouts](#timeouts)
- [Component Type Identity (FNV-1a)](#component-type-identity-fnv-1a)
- [Polyglot Encoding Conventions](#polyglot-encoding-conventions)

---

## Transport

All communication uses [NATS](https://nats.io) as the message transport.

| Property          | Value                                            |
| ----------------- | ------------------------------------------------ |
| Protocol          | NATS (Core + JetStream)                          |
| Default URL       | `nats://localhost:4222`                          |
| URL override      | `NATS_URL` environment variable                  |
| Client library    | `async-nats` 0.38+                               |
| Subject namespace | All subjects prefixed with `engine.`             |
| Authentication    | None (development); configurable via NATS server |

---

## Serialisation

All message payloads are encoded with **MessagePack** (`rmp-serde`). Routing
metadata lives exclusively in NATS headers — never in the payload.

```
┌──────────────────┬─────────────────────────┐
│   NATS Headers   │   MessagePack Payload   │
├──────────────────┼─────────────────────────┤
│ msg-type: "..."  │ { field₁, field₂, … }   │
│ tick-id: "42"    │                         │
│ instance-id: "…" │                         │
└──────────────────┴─────────────────────────┘
```

### Encoding Rules

- **Payloads**: MessagePack via `rmp_serde::to_vec_named` /
  `rmp_serde::from_slice`. The `_named` variant produces **map-encoded**
  MessagePack where each struct field is keyed by its string name, making
  payloads self-describing and readable by any language.
- **Component data within shards**: Each individual component value is
  independently MessagePack-encoded (map format) into a `Vec<u8>`, then
  wrapped inside the outer `ComponentShard` MessagePack envelope.
- **No JSON on the wire**: JSON is reserved for human-readable configuration
  files and component schema definitions (see
  [ComponentSchema](#componentschema)). Component **data** always uses
  MessagePack.
- **Named (map) encoding is mandatory**: All producers must use map-encoded
  MessagePack (field names as string keys). Array-encoded payloads (positional
  fields) are not supported and may fail to decode in non-Rust consumers.

---

## NATS Headers

Standard header keys used across all messages:

| Header Key    | Type     | Description                                                                      |
| ------------- | -------- | -------------------------------------------------------------------------------- |
| `msg-type`    | `String` | Discriminant for message type on multiplexed subjects (e.g. sentinel detection). |
| `tick-id`     | `String` | The tick ID this message belongs to (numeric, stringified).                      |
| `instance-id` | `String` | UUID of the sending system instance.                                             |

Header values for sentinel messages:

| Constant       | Value            | Used on                             |
| -------------- | ---------------- | ----------------------------------- |
| `DATA_DONE`    | `"data_done"`    | `engine.component.set.<system>`     |
| `CHANGES_DONE` | `"changes_done"` | `engine.component.changed.<system>` |

Headers are optional on most messages. They are **required** on sentinel
messages (`DataDone`, `ChangesDone`) so receivers can identify them without
deserialising the payload.

---

## Subject Hierarchy

All subjects are prefixed with `engine.` to namespace within a shared NATS
cluster.

### Static Subjects

| Subject                       | Direction               | Payload              | Purpose                               |
| ----------------------------- | ----------------------- | -------------------- | ------------------------------------- |
| `engine.coord.tick`           | Coordinator → Systems   | `TickStart`          | Signals start of a new tick.          |
| `engine.coord.tick.done`      | Systems → Coordinator   | `TickAck`            | System instance acks tick completion. |
| `engine.entity.create`        | Coordinator → \*        | `EntityCreated`      | Broadcasts entity creation.           |
| `engine.entity.destroy`       | Coordinator → \*        | `EntityDestroyed`    | Broadcasts entity destruction.        |
| `engine.entity.spawn.request` | Systems → Coordinator   | `EntitySpawnRequest` | System requests new entity creation.  |
| `engine.system.register`      | System → Coordinator    | `SystemDescriptor`   | System registers on startup.          |
| `engine.system.unregister`    | System → Coordinator    | `SystemUnregister`   | System unregisters on shutdown.       |
| `engine.system.heartbeat`     | Systems → Coordinator   | `Heartbeat`          | Periodic health & load report.        |
| `engine.query.request`        | Any → Coordinator       | `QueryRequest`       | Ad-hoc query (e.g. from editor).      |
| `engine.query.response`       | Coordinator → Requester | `QueryResponse`      | Response to an ad-hoc query.          |
| `engine.schema.request`       | Any → Coordinator       | `SchemaRequest`      | Request component schemas.            |
| `engine.schema.response`      | Coordinator → Requester | `SchemaResponse`     | Response with component schemas.      |

### Dynamic Subjects

These subjects contain a `<system>` segment that is the system's registered
name (e.g. `physics`, `ai`).

| Subject Pattern                     | Direction            | Payload                           | Purpose                             |
| ----------------------------------- | -------------------- | --------------------------------- | ----------------------------------- |
| `engine.component.set.<system>`     | Coordinator → System | `ComponentShard` or `DataDone`    | Sends component data to a system.   |
| `engine.component.changed.<system>` | System → Coordinator | `ComponentShard` or `ChangesDone` | System publishes mutated data back. |
| `engine.system.schedule.<system>`   | Coordinator → System | `SystemSchedule`                  | Tells system instances to execute.  |

### Queue Groups

System instances subscribe to `engine.system.schedule.<system>` using a queue
group named `q.<system>` (e.g. `q.physics`). NATS delivers each message to
exactly one member of the queue group, enabling automatic load balancing.

---

## Message Types

All types are defined in `engine_net::messages` and derive `Serialize` +
`Deserialize`.

### TickStart

Signals the beginning of a new simulation tick.

```
Subject: engine.coord.tick
Direction: Coordinator → Systems
```

| Field     | Type  | Description                                 |
| --------- | ----- | ------------------------------------------- |
| `tick_id` | `u64` | Monotonically increasing tick counter.      |
| `dt`      | `f64` | Delta time since the last tick, in seconds. |

---

### TickAck

System instance acknowledges tick completion.

```
Subject: engine.coord.tick.done
Direction: Systems → Coordinator
```

| Field         | Type     | Description                            |
| ------------- | -------- | -------------------------------------- |
| `tick_id`     | `u64`    | The tick that was completed.           |
| `instance_id` | `String` | UUID of the reporting system instance. |

---

### EntityCreated

Broadcast when a new entity is created.

```
Subject: engine.entity.create
Direction: Coordinator → *
```

| Field       | Type                   | Description                                        |
| ----------- | ---------------------- | -------------------------------------------------- |
| `entity`    | `Entity` (`u64`)       | The newly allocated entity.                        |
| `archetype` | `Vec<ComponentTypeId>` | Component types placed in this entity's archetype. |

---

### EntityDestroyed

Broadcast when an entity is destroyed.

```
Subject: engine.entity.destroy
Direction: Coordinator → *
```

| Field    | Type             | Description               |
| -------- | ---------------- | ------------------------- |
| `entity` | `Entity` (`u64`) | The entity being removed. |

---

### EntitySpawnRequest

A system requests that the coordinator create a new entity.

```
Subject: engine.entity.spawn.request
Direction: Systems → Coordinator
```

| Field             | Type                   | Description                                                            |
| ----------------- | ---------------------- | ---------------------------------------------------------------------- |
| `component_types` | `Vec<ComponentTypeId>` | The component types the new entity should have.                        |
| `component_data`  | `Vec<Vec<u8>>`         | MessagePack-encoded component values, parallel with `component_types`. |

The coordinator processes spawn requests between ticks, allocates entity IDs,
writes component data into the appropriate archetype, and broadcasts
`EntityCreated` events.

---

### ComponentShard

A batch of component data for a set of entities. This is the primary data
transfer payload between coordinator and systems.

```
Subject: engine.component.set.<system>    (coordinator → system)
         engine.component.changed.<system> (system → coordinator)
```

| Field            | Type              | Description                                               |
| ---------------- | ----------------- | --------------------------------------------------------- |
| `component_type` | `ComponentTypeId` | The component type being transported.                     |
| `entities`       | `Vec<Entity>`     | Entity IDs in this shard (parallel with `data`).          |
| `data`           | `Vec<Vec<u8>>`    | MessagePack-encoded component data, one entry per entity. |

The `entities` and `data` vectors are parallel: `data[i]` is the serialised
component value for `entities[i]`.

---

### DataDone

Sentinel published by the coordinator after all component shards for a tick
have been sent to a system.

```
Subject: engine.component.set.<system>
Direction: Coordinator → System
Header: msg-type = "data_done"
```

| Field     | Type  | Description                        |
| --------- | ----- | ---------------------------------- |
| `tick_id` | `u64` | The tick this sentinel belongs to. |

Systems use this sentinel to stop draining input data immediately rather than
relying on a timeout.

---

### ChangesDone

Sentinel published by a system instance after all changed component shards for
a tick have been sent back to the coordinator.

```
Subject: engine.component.changed.<system>
Direction: System → Coordinator
Header: msg-type = "changes_done"
```

| Field         | Type     | Description                                 |
| ------------- | -------- | ------------------------------------------- |
| `tick_id`     | `u64`    | The tick that was completed.                |
| `instance_id` | `String` | The instance that finished sending changes. |

The coordinator uses this sentinel to stop draining changed data and proceed
with merging.

---

### SystemDescriptor

A system instance registers itself with the coordinator.

```
Subject: engine.system.register
Direction: System → Coordinator
```

| Field               | Type                   | Description                                                                           |
| ------------------- | ---------------------- | ------------------------------------------------------------------------------------- |
| `name`              | `String`               | Human-readable system name (e.g. `"physics"`).                                        |
| `query`             | `QueryDescriptor`      | Data access requirements (reads, writes, optionals, filters).                         |
| `instance_id`       | `String`               | UUID unique to this process instance.                                                 |
| `component_schemas` | `Vec<ComponentSchema>` | Schemas for component types this system reads or writes (optional, defaults to `[]`). |

Multiple instances of the same system share the `name` but have distinct
`instance_id` values. The `component_schemas` field allows the coordinator to
build a shared component registry for polyglot interop — see
[ComponentSchema](#componentschema).

#### QueryDescriptor

| Field       | Type                   | Description                                       |
| ----------- | ---------------------- | ------------------------------------------------- |
| `reads`     | `Vec<ComponentTypeId>` | Component types read immutably.                   |
| `writes`    | `Vec<ComponentTypeId>` | Component types written (mutable access).         |
| `optionals` | `Vec<ComponentTypeId>` | Optional component types.                         |
| `filters`   | `Vec<QueryFilter>`     | Archetype filters (`With`, `Without`, `Changed`). |

#### QueryFilter

| Variant       | Inner Type        | Description                                       |
| ------------- | ----------------- | ------------------------------------------------- |
| `With(id)`    | `ComponentTypeId` | Entity must have this component type.             |
| `Without(id)` | `ComponentTypeId` | Entity must not have this component type.         |
| `Changed(id)` | `ComponentTypeId` | Only match entities where this component changed. |

#### QueryFilter MessagePack Encoding

`QueryFilter` is encoded as a **serde externally-tagged enum**. In named
MessagePack, each variant becomes a single-entry map where the key is the
variant name and the value is the inner `ComponentTypeId`:

```json
{"With": 12345}       // With(ComponentTypeId(12345))
{"Without": 67890}    // Without(ComponentTypeId(67890))
{"Changed": 11111}    // Changed(ComponentTypeId(11111))
```

Non-Rust implementations must produce/consume this tagged-map format.

---

### SystemUnregister

A system instance unregisters from the coordinator on graceful shutdown.

```
Subject: engine.system.unregister
Direction: System → Coordinator
```

| Field         | Type     | Description                 |
| ------------- | -------- | --------------------------- |
| `name`        | `String` | The system name.            |
| `instance_id` | `String` | The instance being removed. |

---

### SystemSchedule

The coordinator instructs system instances to execute on a given tick.

```
Subject: engine.system.schedule.<system>
Direction: Coordinator → System(s)
Queue group: q.<system>
```

| Field         | Type                     | Description                                              |
| ------------- | ------------------------ | -------------------------------------------------------- |
| `tick_id`     | `u64`                    | The tick this schedule belongs to.                       |
| `shard_range` | `Option<(usize, usize)>` | Optional (start_index, count) hint for shard assignment. |

When `shard_range` is `None`, the system receives the full archetype data.

---

### Heartbeat

Periodic health and load report from a system instance.

```
Subject: engine.system.heartbeat
Direction: Systems → Coordinator
```

| Field         | Type     | Description                                     |
| ------------- | -------- | ----------------------------------------------- |
| `instance_id` | `String` | UUID of the reporting instance.                 |
| `system`      | `String` | The system name.                                |
| `load`        | `f64`    | Load metric: 0.0 = idle, 1.0 = fully saturated. |

---

### QueryRequest

An ad-hoc query against the coordinator's world state (typically from the
editor).

```
Subject: engine.query.request
Direction: Any → Coordinator
```

| Field   | Type              | Description           |
| ------- | ----------------- | --------------------- |
| `query` | `QueryDescriptor` | The query to execute. |

---

### QueryResponse

Response to an ad-hoc query.

```
Subject: engine.query.response
Direction: Coordinator → Requester
```

| Field      | Type                  | Description                             |
| ---------- | --------------------- | --------------------------------------- |
| `entities` | `Vec<Entity>`         | Matching entity IDs.                    |
| `shards`   | `Vec<ComponentShard>` | Component data for each requested type. |

---

### ComponentSchema

Describes the schema of a component type for polyglot interoperability. Systems
include component schemas when they register (via `SystemDescriptor`), and they
can also be queried on-demand via `SchemaRequest` / `SchemaResponse`.

```
Embedded in: SystemDescriptor, SchemaResponse
```

| Field     | Type              | Description                                                                                           |
| --------- | ----------------- | ----------------------------------------------------------------------------------------------------- |
| `name`    | `String`          | Human-readable component name (e.g. `"Velocity"`).                                                    |
| `type_id` | `ComponentTypeId` | Deterministic FNV-1a hash of `name` (see [Component Type Identity](#component-type-identity-fnv-1a)). |
| `schema`  | `JSON Value`      | A JSON Schema object describing the component's fields and types.                                     |

#### Schema Example

```json
{
  "name": "Velocity",
  "type_id": 8502879624764554621,
  "schema": {
    "type": "object",
    "properties": {
      "x": { "type": "number", "format": "float" },
      "y": { "type": "number", "format": "float" },
      "z": { "type": "number", "format": "float" }
    },
    "required": ["x", "y", "z"]
  }
}
```

The `schema` field follows [JSON Schema](https://json-schema.org/) conventions.
Non-Rust systems use it to correctly serialise/deserialise components to/from
MessagePack.

---

### SchemaRequest

Request component schemas from the coordinator's registry.

```
Subject: engine.schema.request
Direction: Any → Coordinator
```

| Field   | Type          | Description                                     |
| ------- | ------------- | ----------------------------------------------- |
| `names` | `Vec<String>` | Component names to look up. Empty = return all. |

---

### SchemaResponse

Response containing component schemas.

```
Subject: engine.schema.response
Direction: Coordinator → Requester
```

| Field     | Type                   | Description            |
| --------- | ---------------------- | ---------------------- |
| `schemas` | `Vec<ComponentSchema>` | The requested schemas. |

---

## Sequences

### System Registration

```
System                              Coordinator
  │                                     │
  │─── [1] NATS connect ───────────────►│
  │                                     │
  │─── [2] engine.system.register ─────►│
  │    { name, query, instance_id }     │
  │                                     │── [3] Queue registration
  │                                     │       (applied before next tick)
  │                                     │
  │◄── [4] engine.system.schedule.X ────│  (when first tick arrives)
  │                                     │
```

1. System connects to NATS at the configured URL.
2. System publishes a `SystemDescriptor` to `engine.system.register`.
3. Coordinator queues the registration; it is applied atomically before the
   next tick starts. The system set never changes mid-tick.
4. On the next tick, the coordinator includes the system in scheduling.

---

### Per-Tick Execution

This sequence occurs for each system in each stage within a tick.

```
Coordinator                             System Instance
  │                                          │
  │─── [1] engine.component.set.X ─────────►│  (N shard messages)
  │─── [2] engine.component.set.X ─────────►│  (DataDone sentinel,
  │        hdr: msg-type=data_done           │   hdr: msg-type=data_done)
  │                                          │
  │─── [3] engine.system.schedule.X ───────►│
  │        { tick_id, shard_range }          │
  │                                          │── [4] Execute system_fn
  │                                          │
  │◄── [5] engine.component.changed.X ──────│  (N shard messages)
  │◄── [6] engine.component.changed.X ──────│  (ChangesDone sentinel,
  │        hdr: msg-type=changes_done        │   hdr: msg-type=changes_done)
  │                                          │
  │◄── [7] engine.entity.spawn.request ─────│  (0..N spawn requests)
  │                                          │
  │◄── [8] engine.coord.tick.done ──────────│
  │        { tick_id, instance_id }          │
  │                                          │
  │── [9] Merge changes into                 │
  │       canonical store                    │
  │                                          │
```

**Step details:**

1. **Data send**: Coordinator publishes `ComponentShard` messages to
   `engine.component.set.<system>` — one message per component type per
   archetype shard.
2. **DataDone sentinel**: Coordinator publishes a `DataDone` message on the
   same subject with `msg-type: data_done` header. The system stops draining.
3. **Schedule**: Coordinator publishes `SystemSchedule` to
   `engine.system.schedule.<system>` (via queue group).
4. **Execute**: System deserialises shards into local archetype tables, runs
   the system function, and collects changed data.
5. **Changed data**: System publishes `ComponentShard` messages for modified
   components to `engine.component.changed.<system>`.
6. **ChangesDone sentinel**: System publishes a `ChangesDone` message on the
   same subject with `msg-type: changes_done` header. The coordinator stops
   draining.
7. **Spawn requests**: System publishes any `EntitySpawnRequest` messages.
   These are processed between ticks.
8. **Tick ack**: System publishes `TickAck` to `engine.coord.tick.done`.
9. **Merge**: Coordinator writes changed components into canonical storage.
   Within a stage, systems have non-conflicting writes, so merge is a simple
   overwrite.

---

### Entity Spawning

```
System                              Coordinator
  │                                     │
  │─── engine.entity.spawn.request ────►│
  │    { component_types, data }        │
  │                                     │── (queued)
  │                                     │
  ├─────── tick boundary ──────────────►│
  │                                     │── Allocate entity ID
  │                                     │── Write to archetype table
  │                                     │
  │◄── engine.entity.create ────────────│
  │    { entity, archetype }            │
  │                                     │
```

Spawn requests are **never** processed mid-tick. They are queued and applied
atomically at the start of the next tick, after which `EntityCreated` events
are broadcast.

---

### System Shutdown

```
System                              Coordinator
  │                                     │
  │─── engine.system.unregister ───────►│
  │    { name, instance_id }            │
  │                                     │── Queue unregistration
  │                                     │   (applied before next tick)
  │─── NATS disconnect                  │
  │                                     │
```

If the system crashes without sending `system.unregister`, the coordinator
detects the missing `tick.done` ack within a timeout and removes the instance.

---

### Ad-hoc Query

Used by the editor or other tools to inspect world state.

```
Requester                           Coordinator
  │                                     │
  │─── engine.query.request ───────────►│
  │    { query }                        │
  │                                     │── Match archetypes
  │                                     │── Gather data
  │◄── engine.query.response ───────────│
  │    { entities, shards }             │
  │                                     │
```

---

## Sentinel Protocol

Sentinels are "end-of-stream" markers used on multiplexed subjects where a
variable number of `ComponentShard` messages precede a termination signal.

### How sentinels work

1. The sender publishes N `ComponentShard` messages on a subject.
2. The sender then publishes a sentinel message **on the same subject** with
   the `msg-type` header set to the sentinel value.
3. The receiver drains messages from the subject, checking each message's
   `msg-type` header. When the sentinel value is found, the receiver stops
   draining.

### Sentinel types

| Sentinel      | Header value     | Subject pattern                     | Sender      | Receiver    |
| ------------- | ---------------- | ----------------------------------- | ----------- | ----------- |
| `DataDone`    | `"data_done"`    | `engine.component.set.<system>`     | Coordinator | System      |
| `ChangesDone` | `"changes_done"` | `engine.component.changed.<system>` | System      | Coordinator |

### Fallback timeout

If a sentinel does not arrive within a deadline (currently **5 seconds** in the
system runner), the receiver proceeds with whatever data has been collected and
logs a warning. This prevents indefinite hangs if the sender crashes mid-stream.

---

## Queue Groups & Horizontal Scaling

Multiple instances of the same system form a NATS **queue group** named
`q.<system>` (e.g. `q.physics`). The coordinator sends schedule and data
messages to the system's subjects; NATS delivers each message to exactly one
instance in the queue group.

For the architectural rationale behind this scaling model, see
**`ARCHITECTURE.md` → Design Decisions & Rationale**.

```
                    engine.system.schedule.physics
                        (queue: q.physics)
                              │
                ┌─────────────┼─────────────┐
                ▼             ▼             ▼
          ┌──────────┐ ┌──────────┐ ┌──────────┐
          │ inst-aaa │ │ inst-bbb │ │ inst-ccc │
          └──────────┘ └──────────┘ └──────────┘
```

**Scaling properties:**

- **Add instances** at any time — they register via `system.register` and join
  the queue group. The coordinator includes them in the next tick automatically.
- **Remove instances** at any time — they unregister via `system.unregister`
  and NATS stops routing to them.
- **No coordinator changes** are needed — horizontal scaling is entirely a
  deployment concern.

---

## JetStream Persistence

NATS JetStream is used for `engine.component.*` subjects so that:

- Late-joining system instances can replay the latest component state.
- The coordinator can recover world state after a crash by replaying from
  the stream.
- Snapshots can be written to a dedicated `engine.snapshot.<id>` stream by
  pausing the tick loop and publishing all archetype tables.

---

## Error Handling

Network errors are represented by `engine_net::NetError`.
For architectural resilience strategies (crash recovery, slow systems, etc.),
see **`ARCHITECTURE.md` → Error Handling & Resilience**.

| Variant         | Cause                                     |
| --------------- | ----------------------------------------- |
| `Encode`        | MessagePack serialisation failed.         |
| `Decode`        | MessagePack deserialisation failed.       |
| `Nats`          | General NATS error.                       |
| `Subscribe`     | NATS subscription failed.                 |
| `Publish`       | NATS publish failed.                      |
| `Connect`       | NATS connection could not be established. |
| `MissingHeader` | A required NATS header was absent.        |

### Failure scenarios

| Failure           | Behaviour                                                                                                                                                          |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| System crash      | Coordinator detects missing `tick.done` ack within timeout. If other instances exist, NATS already rerouted shards. Otherwise the system is skipped for that tick. |
| Coordinator crash | JetStream retains component state. New coordinator replays from stream and resumes.                                                                                |
| NATS disconnect   | `async-nats` reconnects automatically. Systems buffer locally and retry.                                                                                           |
| Slow system       | Coordinator enforces a tick deadline. If an instance misses it, its results are dropped and the tick proceeds. Scale horizontally to distribute load.              |

---

## Timeouts

| Timeout                 | Default      | Context                                                 |
| ----------------------- | ------------ | ------------------------------------------------------- |
| Data drain (`DataDone`) | 5 seconds    | System waits for all component shards from coordinator. |
| Tick ack deadline       | Configurable | Coordinator waits for `tick.done` from all instances.   |
| Heartbeat interval      | Periodic     | System instances report health to coordinator.          |

---

## Component Type Identity (FNV-1a)

Every component type is identified by a `ComponentTypeId` — a `u64` derived
from the component's human-readable **name** (e.g. `"Transform3D"`,
`"Velocity"`) using the **FNV-1a 64-bit** hash algorithm. This produces a
deterministic, language-neutral identifier that any implementation can compute.

### Algorithm

```
Input:  name   — a UTF-8 string (the component's type name)
Output: hash   — a u64 value

hash ← 0xcbf29ce484222325            (FNV offset basis)
for each byte b in name:
    hash ← hash XOR b
    hash ← hash × 0x00000100000001b3  (FNV prime, wrapping multiply)
return hash
```

### Constants

| Constant     | Value (hex)          | Value (decimal)      |
| ------------ | -------------------- | -------------------- |
| Offset basis | `0xcbf29ce484222325` | 14695981039346656037 |
| Prime        | `0x00000100000001b3` | 1099511628211        |

### Test Vectors

| Name         | Expected `ComponentTypeId` (hex)      |
| ------------ | ------------------------------------- |
| `""` (empty) | `0xcbf29ce484222325` (offset basis)   |
| `"Health"`   | Compute with reference implementation |
| `"Velocity"` | Compute with reference implementation |

The empty-string case is useful for verifying the offset basis is correct.
Implementations should confirm they produce the offset basis for an empty
input.

### Reference Implementation (Rust)

```rust
const fn fnv1a_64(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x00000100000001b3);
        i += 1;
    }
    hash
}
```

### Reference Implementation (Python)

```python
def fnv1a_64(name: str) -> int:
    hash = 0xcbf29ce484222325
    for byte in name.encode("utf-8"):
        hash ^= byte
        hash = (hash * 0x00000100000001b3) % (2**64)
    return hash
```

### Naming Conventions

- Component names should be **short, PascalCase identifiers** (e.g.
  `"Transform3D"`, `"Velocity"`, `"Health"`).
- The name is the **single source of truth** for identity. Two components with
  the same name are considered the same type, regardless of their field layout.
- All systems using a component must agree on the same name string. The
  coordinator validates consistency when systems register conflicting schemas
  for the same `ComponentTypeId`.

---

## Polyglot Encoding Conventions

The wire protocol is designed to be implementable in **any language** that has
a NATS client library and a MessagePack library. This section summarises the
requirements for non-Rust implementations.

### MessagePack Format

All payloads use **map-encoded** (named) MessagePack. Each struct is serialised
as a MessagePack map where keys are the struct field names as strings and
values are the field values. This is self-describing and avoids positional
ambiguity.

Example encoding of `TickStart { tick_id: 42, dt: 0.016 }`:

```
MessagePack map:
  "tick_id" → 42
  "dt"      → 0.016
```

Array-encoded (positional) MessagePack is **not supported** on the wire.

### Enum Encoding (Serde Externally Tagged)

Rust enums are encoded using serde's **externally tagged** representation: a
single-entry map where the key is the variant name.

| Rust value                 | MessagePack map encoding |
| -------------------------- | ------------------------ |
| `QueryFilter::With(id)`    | `{"With": <id>}`         |
| `QueryFilter::Without(id)` | `{"Without": <id>}`      |
| `QueryFilter::Changed(id)` | `{"Changed": <id>}`      |
| `Option::Some(v)`          | `v` (unwrapped)          |
| `Option::None`             | `nil`                    |

### Component Data

Component values inside `ComponentShard.data` entries are each independently
map-encoded MessagePack. For example, a `Health { current: 80.0, max: 100.0 }`
component is encoded as:

```
MessagePack map:
  "current" → 80.0
  "max"     → 100.0
```

### Schema Discovery

Non-Rust systems can discover component layouts at runtime by sending a
`SchemaRequest` to `engine.schema.request` and receiving a `SchemaResponse`
on `engine.schema.response`. The response contains `ComponentSchema` entries
with JSON Schema definitions for each component type. See
[ComponentSchema](#componentschema), [SchemaRequest](#schemarequest), and
[SchemaResponse](#schemaresponse).
