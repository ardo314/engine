# PROTOCOL.md — Wire Protocol Reference

This document is the definitive reference for the NATS-based wire protocol used
by the distributed ECS engine. It describes every subject, message type, header
convention, serialisation format, and the exact sequence of messages exchanged
during system registration, tick execution, entity lifecycle, and ad-hoc
queries.

For high-level architecture and design rationale, see `ARCHITECTURE.md`.

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

- **Payloads**: MessagePack via `rmp_serde::to_vec` / `rmp_serde::from_slice`.
- **Component data within shards**: Each individual component value is
  independently MessagePack-encoded into a `Vec<u8>`, then wrapped inside the
  outer `ComponentShard` MessagePack envelope.
- **No JSON on the wire**: JSON is reserved for human-readable configuration
  files only.

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

| Field         | Type              | Description                                                   |
| ------------- | ----------------- | ------------------------------------------------------------- |
| `name`        | `String`          | Human-readable system name (e.g. `"physics"`).                |
| `query`       | `QueryDescriptor` | Data access requirements (reads, writes, optionals, filters). |
| `instance_id` | `String`          | UUID unique to this process instance.                         |

Multiple instances of the same system share the `name` but have distinct
`instance_id` values.

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

Network errors are represented by `engine_net::NetError`:

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
