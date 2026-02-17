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
- **ECS** (`engine_ecs`) — core primitives: Entity, Component, Archetype,
  Query, System.

---

## Repository Layout

```
engine/
├── crates/
│   ├── engine_app/       # Coordinator binary
│   ├── engine_ecs/       # Core ECS types and traits
│   ├── engine_math/      # Math (glam re-exports + engine-specific types)
│   ├── engine_net/       # NATS transport layer (planned)
│   └── engine_system/   # System runtime (one system per process)
├── engine_editor/        # Tauri + React desktop editor
│   ├── src/              # React/TypeScript frontend
│   └── src-tauri/        # Tauri Rust backend
├── examples/
│   └── components/       # Example component definitions
├── ARCHITECTURE.md       # Distributed ECS design document
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
- Use `rmp-serde` (MessagePack) for wire format.
- Use `serde_json` only for human-readable config files.

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
- See `ARCHITECTURE.md` for the full subject hierarchy.
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
