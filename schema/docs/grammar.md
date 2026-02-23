# ECS IDL Grammar Specification

A language- and implementation-agnostic interface definition language for distributed
Entity Component System architectures. The IDL defines the **shape of data** (records)
and the **data dependencies of logic** (systems). It intentionally does not encode
networking, replication, authority, or any runtime concerns.

## Design Principles

- **Records are the universal entity-attachable type.** A record with fields is a
  component. An empty record is a tag. Events are records attached to event entities.
  There is no semantic distinction at the schema level.
- **Systems declare data dependencies, not implementations.** A system says _what_ it
  reads and writes, not _how_. Implementations live in Rust, C++, or whatever language
  targets the codegen output.
- **Value types are not attachable to entities.** `enum`, `variant`, `flags`, and `type`
  aliases can be used as field types within records, but they cannot appear in system
  queries or be attached to entities directly.
- **No user-defined generics.** Only the built-in parameterized types (`list`, `option`,
  `map`, `set`, `tuple`) accept type parameters. This keeps the language simple and
  codegen tractable.
- **`snake_case` naming convention** for all identifiers.

---

## Keywords

```
package  use  type  enum  variant  flags  record  system  phase  world  query
read  write  optional  exclude  changed  with  order_after  order_before  hz
```

---

## Package Declaration

Every `.ecs` file begins with a package declaration. Packages use a `namespace:name@version`
format. The version is optional.

```ecs
package engine:physics@0.1.0
```

```ecs
package my_game:combat
```

---

## Imports

The `use` statement imports types from other packages. Imported types can be used as
field types in records or referenced in system queries.

```ecs
use engine:std.{vec3, quat, entity_id}
use engine:physics.{transform, velocity}
```

You can rename imports with `as`:

```ecs
use engine:std.{vec3 as position_type}
```

---

## Primitive Types

The following primitive types are built in and always available:

| Type     | Description             |
| -------- | ----------------------- |
| `bool`   | Boolean                 |
| `u8`     | Unsigned 8-bit integer  |
| `u16`    | Unsigned 16-bit integer |
| `u32`    | Unsigned 32-bit integer |
| `u64`    | Unsigned 64-bit integer |
| `i8`     | Signed 8-bit integer    |
| `i16`    | Signed 16-bit integer   |
| `i32`    | Signed 32-bit integer   |
| `i64`    | Signed 64-bit integer   |
| `f32`    | 32-bit floating point   |
| `f64`    | 64-bit floating point   |
| `string` | UTF-8 string            |
| `bytes`  | Raw byte sequence       |

---

## Built-in Parameterized Types

These are the only types that accept type parameters. Users cannot define their own
generic types.

| Syntax             | Description                                      |
| ------------------ | ------------------------------------------------ |
| `list<T>`          | Ordered sequence of `T`                          |
| `option<T>`        | `T` or nothing                                   |
| `map<K, V>`        | Key-value mapping                                |
| `set<T>`           | Unordered unique collection of `T`               |
| `tuple<T1, T2, …>` | Anonymous product type of heterogeneous elements |

Examples:

```ecs
type scores = map<string, f64>
type maybe_target = option<entity_id>
type coordinate = tuple<f32, f32, f32>
```

---

## Type Aliases

Type aliases create a new name for an existing type. They do not create a new type for
the purposes of ECS storage — an alias is transparent.

```ecs
type entity_id = u64
type name = string
type inventory = list<entity_id>
```

---

## Enums

Enums are simple named alternatives with no associated data. They are value types only —
usable as field types in records but not attachable to entities.

```ecs
enum direction {
    north,
    south,
    east,
    west,
}
```

```ecs
enum body_type {
    static_body,
    dynamic,
    kinematic,
}
```

---

## Variants

Variants are discriminated unions. Each case can optionally carry a payload. They are
value types only.

```ecs
variant shape {
    circle(f32),
    rect(f32, f32),
    polygon(list<vec2>),
}
```

```ecs
variant filter {
    all,
    none,
    by_layer(set<layer>),
}
```

Cases without a payload are bare identifiers. Cases with a payload use `case_name(type)`.
Multi-field payloads use tuples or records.

---

## Flags

Flags are a set of named boolean flags represented as a bitfield. A value of a flags
type can contain any combination of the flags. They are value types only.

```ecs
flags collision_layer {
    terrain,
    players,
    projectiles,
    triggers,
}
```

```ecs
flags render_layer {
    opaque,
    transparent,
    ui,
    debug,
}
```

---

## Records

Records are the sole entity-attachable type. Every record can be attached to an entity,
stored in ECS archetype tables, and queried by systems.

### Component (record with fields)

A record with one or more fields is a standard ECS component:

```ecs
record transform {
    position: vec3,
    rotation: quat,
    scale: vec3,
}
```

```ecs
record velocity {
    linear: vec3,
    angular: vec3,
}
```

```ecs
record health {
    current: f32,
    max: f32,
}
```

### Tag (empty record)

An empty record is a zero-sized tag component. Tags are used as markers for filtering
in system queries without carrying any data:

```ecs
record player {}
record frozen {}
record dead {}
record grounded {}
```

### Events as Records

Events are records attached to event entities. There is no special `event` keyword.
Systems query for them like any other record, and the runtime is responsible for entity
lifecycle (spawning event entities, cleaning them up after consumption):

```ecs
record collision {
    other: entity_id,
    point: vec3,
    normal: vec3,
    impulse: f32,
}
```

```ecs
record damage_taken {
    amount: f32,
    source: option<entity_id>,
}
```

### Nested Types

Records can use any value type as a field type, including other records, enums, variants,
flags, and built-in parameterized types:

```ecs
record rigid_body {
    body_type: body_type,
    mass: f32,
    restitution: f32,
    friction: f32,
    layer: collision_layer,
    gravity_scale: f32,
}
```

---

## Systems

Systems declare their data dependencies through a query DSL. A system definition says
which records it reads, writes, optionally accesses, excludes, or filters by change
detection. It also declares ordering constraints and the phase it runs in.

### Basic System

```ecs
system apply_velocity {
    query {
        read: [velocity],
        write: [transform],
    }
}
```

### Full Query DSL

The query block supports five clauses:

| Clause     | Meaning                                                  |
| ---------- | -------------------------------------------------------- |
| `read`     | Records the system reads (immutable access)              |
| `write`    | Records the system reads and writes (mutable access)     |
| `optional` | Records the system accesses if present, but not required |
| `exclude`  | Entities with these records are skipped                  |
| `changed`  | Only match entities where these records changed recently |

All clauses take a list of record names in square brackets.

```ecs
system physics_step {
    query {
        read: [velocity, mass],
        write: [transform],
        optional: [drag],
        exclude: [frozen],
        changed: [velocity],
    }
    phase: fixed_update,
}
```

### Multiple Queries

Systems can declare multiple named queries when they need to access different entity
sets:

```ecs
system collision_response {
    query colliders {
        read: [transform, collider],
    }
    query dynamic_bodies {
        read: [rigid_body, mass],
        write: [velocity],
        exclude: [frozen],
    }
    phase: fixed_update,
    order_after: [collision_detect],
}
```

### Ordering and Phase

Systems can declare ordering constraints relative to other systems and the phase
(tick group) they belong to:

```ecs
system render_sprites {
    query {
        read: [transform, sprite],
        optional: [visibility],
    }
    phase: render,
    order_after: [physics_step, animation_update],
    order_before: [post_process],
}
```

- `phase` — the tick group this system runs in (references a `phase` definition).
- `order_after` — this system runs after all listed systems within the same phase.
- `order_before` — this system runs before all listed systems within the same phase.

---

## Phases

Phases define tick groups that control system scheduling. Systems are assigned to phases,
and phases execute in declaration order within a world.

### Fixed-Rate Phase

```ecs
phase fixed_update {
    hz: 60,
}
```

### Variable-Rate Phase

```ecs
phase update {}

phase render {}
```

### Startup Phase

A phase that runs exactly once:

```ecs
phase startup {}
```

---

## World

A world is the top-level composition unit. It groups records, systems, and phases into
a named definition. Worlds can include other worlds.

```ecs
package my_game:core@0.1.0

use engine:std.{vec3, quat, entity_id}

world game {
    // Phases (execution order is declaration order)
    phase startup {}
    phase fixed_update { hz: 60 }
    phase update {}
    phase render {}

    // Records (components, tags, events)
    record transform {
        position: vec3,
        rotation: quat,
        scale: vec3,
    }

    record velocity {
        linear: vec3,
        angular: vec3,
    }

    record player {}
    record frozen {}

    // Systems
    system apply_velocity {
        query {
            read: [velocity],
            write: [transform],
            exclude: [frozen],
        }
        phase: fixed_update,
    }
}
```

### World Composition

Worlds can include other worlds to compose functionality:

```ecs
world physics_world {
    include engine:physics.default
}

world game {
    include physics_world

    record player {}

    system player_move {
        query {
            read: [player, input],
            write: [velocity],
        }
        phase: fixed_update,
    }
}
```

---

## Comments

Line comments use `//`. Block comments use `/* ... */`.

```ecs
// This is a line comment.

/*
 * This is a block comment.
 */
record example {
    value: f32, // Inline comment
}
```

---

## File Organization

- One package per `.ecs` file or directory.
- Standard library types live in the `engine:std` package.
- `use` imports types across packages.
- Worlds can `include` other worlds across packages.
- File extension: `.ecs`

---

## Complete Example

```ecs
package my_game:rpg@0.1.0

use engine:std.{vec3, quat, entity_id}

world rpg {
    phase fixed_update { hz: 30 }
    phase update {}

    // Spatial
    record transform {
        position: vec3,
        rotation: quat,
        scale: vec3,
    }

    record velocity {
        linear: vec3,
        angular: vec3,
    }

    // Gameplay
    record health {
        current: f32,
        max: f32,
    }

    record damage_taken {
        amount: f32,
        source: option<entity_id>,
    }

    record player {}
    record dead {}
    record invulnerable {}

    // Systems
    system movement {
        query {
            read: [velocity],
            write: [transform],
            exclude: [dead],
        }
        phase: fixed_update,
    }

    system apply_damage {
        query {
            read: [damage_taken],
            write: [health],
            exclude: [invulnerable, dead],
        }
        phase: update,
        order_before: [check_death],
    }

    system check_death {
        query {
            read: [health],
            exclude: [dead],
            changed: [health],
        }
        phase: update,
    }
}
```
