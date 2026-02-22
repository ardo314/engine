# AGENTS.md — Guidelines for AI Coding Agents

This file contains instructions and conventions for AI agents (GitHub Copilot,
Cursor, Cline, etc.) working on this codebase.

---

## Project Overview

This is a **distributed Entity Component System (ECS) engine** written in Rust,
with a Tauri-based desktop editor. See `ARCHITECTURE.md` for the full design.

Key concepts:

- **Coordinator** (`engine_app`) — single authority for world state.
- **Systems** (`engine_system`) — stateless processes, each running exactly
  one system function.
- **NATS** — message transport between coordinator, systems, and editor.
- **Component** (`engine_component`) — the "C" in ECS. Component trait,
  derive utilities, and serialisation support.

---

## Repository Layout

```
engine/
├── crates/
│   ├── engine_app/       # Coordinator binary
│   ├── engine_component/ # Component trait and types (the "C" in ECS)
│   ├── engine_math/      # Math (glam re-exports + engine-specific types)
│   ├── engine_net/       # NATS transport layer (planned)
│   └── engine_system/   # System runtime (one system per process)
├── engine_editor/        # Tauri + React desktop editor
│   ├── src/              # React/TypeScript frontend
│   └── src-tauri/        # Tauri Rust backend
├── examples/
│   └── components/       # Example component definitions
├── ARCHITECTURE.md       # High-level design, crate map, scheduling, storage
├── PROTOCOL.md           # Wire protocol: subjects, messages, sequences
├── AGENTS.md             # This file
├── Cargo.toml            # Workspace manifest
└── README.md
```

---

## Rust Conventions

### Edition & Toolchain

- Rust **2024 edition** (`edition = "2024"` in Cargo.toml).
- Use stable Rust. No nightly-only features unless absolutely necessary.

### Style

- Follow standard `rustfmt` formatting. Run `cargo fmt` before committing.
- Run `cargo clippy -- -D warnings` and fix all warnings.
- Prefer `snake_case` for functions and variables, `PascalCase` for types.
- Use `//!` doc comments at the top of each module/crate explaining purpose.
- Use `///` doc comments on all public items.

### Error Handling

- Use `anyhow::Result` for application-level code (binaries, tests).
- Use `thiserror` for library crates that define their own error types.
- Never use `.unwrap()` in library code. Prefer `?` or explicit error handling.
- `.unwrap()` is acceptable only in tests and examples.

### Async

- Use `tokio` as the async runtime.
- Prefer `async fn` over manual `Future` implementations.
- Use `async-nats` for all NATS communication.

### Serialisation

- All types that cross the network boundary must derive `serde::Serialize`
  and `serde::Deserialize`.
- Use `rmp-serde` (MessagePack) for wire format. Always use **named (map)
  encoding** (`rmp_serde::to_vec_named`) — never `rmp_serde::to_vec`
  (array encoding). Named encoding makes payloads self-describing for
  polyglot consumers.
- Use `serde_json` only for human-readable config files and component schema
  definitions.

### Polyglot & Component Identity

- `ComponentTypeId` is derived from a component's **string name** using the
  FNV-1a 64-bit hash. See `PROTOCOL.md` for the algorithm specification.
- Component `type_name()` values must be **short PascalCase identifiers**
  (e.g. `"Transform3D"`, not `"engine::math::Transform3D"`). Two components
  with the same name are the same type.
- When adding a new component, ensure `type_name()` does not collide with
  existing names.
- Systems may include `ComponentSchema` entries (JSON Schema) in their
  `SystemDescriptor` to describe component layouts for non-Rust consumers.
- Never use Rust's `std::any::TypeId` for component identity — it is
  non-deterministic across compilations and language-specific.

### ECS-Specific Rules

- The `Component` trait must be implemented for any type stored in the ECS.
- Components must be `Send + Sync + 'static`.
- Components must derive `Serialize, Deserialize` for network transport.
- Entity IDs are `u64`. Do not use `usize` for entity identifiers.
- Systems must declare their queries explicitly — no implicit world access.
- Each system process runs **exactly one system function** — never multiplex
  multiple systems in a single process.
- Horizontal scaling of a system is done by launching more instances of the
  same system behind a NATS queue group, not by combining systems.

### Dependencies

- Keep dependency count minimal. Justify new dependencies.
- Pin major versions in `Cargo.toml` (e.g. `serde = "1"`).
- Workspace-level dependencies should be defined in the root `Cargo.toml`
  `[workspace.dependencies]` and referenced with `{ workspace = true }`.

### Testing

- Write unit tests in `#[cfg(test)] mod tests` within each module.
- Write integration tests in a `tests/` directory per crate.
- Use `cargo test --workspace` to run all tests.
- Name tests descriptively: `test_entity_creation_allocates_unique_ids`.

---

## NATS Conventions

- All subjects are prefixed with `engine.`.
- See `PROTOCOL.md` for the full subject hierarchy, message schemas, header
  conventions, sentinel protocol, and sequence diagrams.
- See `ARCHITECTURE.md` for the high-level design rationale behind NATS usage.
- Use NATS headers for routing metadata (`msg-type`, `tick-id`, `instance-id`).
- Never put routing information in the payload.
- Use JetStream for any data that must survive restarts.

---

## Editor (Tauri + React)

- Frontend is React + TypeScript + Vite.
- Use functional components with hooks. No class components.
- TypeScript strict mode is enabled (`tsconfig.json`).
- Tauri commands are defined in `engine_editor/src-tauri/src/lib.rs`.
- Keep Tauri commands thin — delegate to engine crates.

---

## Git Conventions

- Branch naming: `feat/<name>`, `fix/<name>`, `refactor/<name>`.
- Write clear, imperative commit messages: "Add entity allocation to coordinator".
- Keep commits atomic — one logical change per commit.
- Do not commit build artifacts or IDE-specific files.

---

## Documentation ↔ Code Consistency

Three documents describe the engine design. Each has a distinct scope:

| Document          | Scope                                                                                                                    |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `ARCHITECTURE.md` | High-level design: crate map, ECS concepts, scheduling, storage, query system, design rationale.                         |
| `PROTOCOL.md`     | Wire protocol: NATS subjects, message schemas, header keys, sentinel protocol, sequence diagrams, error types, timeouts. |
| `AGENTS.md`       | Coding conventions, style rules, and instructions for AI agents.                                                         |

All three documents must stay in sync **with each other** and **with the
implementation**. Treat the documents as the source of truth for their
respective scopes, and the code as the source of truth for implementation
detail.

### Rules

- **Code changes → update documentation.** When you add, remove, or modify a
  crate, NATS subject, message type, ECS concept, system lifecycle step, or
  wire format, update the corresponding section in the relevant document(s)
  to reflect the change. Keep updates conceptual in `ARCHITECTURE.md` and
  precise in `PROTOCOL.md`.
- **Documentation changes → update code.** When you change a design decision
  in `ARCHITECTURE.md`, a message schema in `PROTOCOL.md`, or a convention in
  `AGENTS.md`, propagate the change to the relevant code, types, and module
  docs.
- **Check alignment before implementing.** Before starting work, read the
  relevant sections of `ARCHITECTURE.md` and `PROTOCOL.md` and verify the
  planned change is consistent with the documented design. If it is **not**,
  stop and ask the user whether the change is intentional. Do not silently
  diverge from the documentation.
- **Flag unintentional drift.** If you discover existing code that contradicts
  any document (or vice versa), notify the user and ask how to resolve the
  inconsistency before proceeding.
- **No duplication across documents.** Each fact should live in exactly one
  document. Cross-reference the other documents instead of repeating content.
  If you notice duplicate content creeping in, refactor it so the detail lives
  in the document that owns that scope and the others link to it.

---

## What NOT to Do

- Do not add `unsafe` code without a `// SAFETY:` comment explaining why it
  is sound.
- Do not use `Box<dyn Any>` for component storage without a compelling reason
  — prefer type-erased `BlobVec` with known layouts.
- Do not introduce circular dependencies between crates.
- Do not bypass the coordinator for entity creation — all entity IDs must come
  from `engine_app`.
- Do not use blocking I/O in async contexts. Use `tokio::task::spawn_blocking`
  if needed.
- Do not hardcode NATS URLs — always read from configuration or environment
  variables (`NATS_URL`).

---

## Common Tasks

### Building

```bash
cargo build --workspace
```

### Running the Coordinator

```bash
cargo run -p engine_app
```

### Running Tests

```bash
cargo test --workspace
```

### Running Clippy

```bash
cargo clippy --workspace -- -D warnings
```

### Formatting

```bash
cargo fmt --all
```

### Starting NATS (development)

```bash
nats-server -js  # JetStream enabled
```
