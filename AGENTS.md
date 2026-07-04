# Cabbage Agent Guide

## Hard Boundary

- Modify files only inside this plugin directory: `plugin-examples/Cabbage`.
- Files outside this directory may be read for API discovery, examples, and compatibility checks, but must not be edited.
- Prefer accomplishing changes by extending this plugin rather than changing Pumpkin core crates.

## Project Purpose

Cabbage is a native Pumpkin plugin focused on server utilities, diagnostics, and custom mob AI/pathfinding. The README is the high-level product guide; this file is the working guide for future agents.

When the task involves Mob AI, pathfinding, movement, velocity planning, worker threads, or managed entity behavior, read `src/mob_ai/MOB_AI_DATA_FLOW.md` before editing files in `src/mob_ai/`.

## Current Layout

```text
Cabbage/
|-- AGENTS.md
|-- README.md
|-- Cargo.toml
|-- compile.bat
`-- src/
    |-- lib.rs
    `-- mob_ai/
        |-- mod.rs               # event handler and state ownership
        |-- MOB_AI_DATA_FLOW.md  # mob AI architecture and data flow rules
        |-- pathfinding.rs       # grid, bounds, bidirectional A*
        |-- movement.rs          # velocity plans, lookahead, rotation
        |-- workers.rs           # Rayon worker-pool job submission and ActiveJobGuard
        |-- clustering.rs        # mob-location table and anti-clump push
        `-- types.rs             # shared snapshots and small data structs
```

## Refactor Organization Chart

Use this target structure as files become larger. (The **mob_ai** refactor has been completed).

```text
src/
|-- lib.rs
|   |-- plugin metadata and load/unload wiring
|   |-- command/event registration only
|   `-- module exports
|
|-- commands/
|   |-- mod.rs
|   |-- cabbage.rs
|   |-- clear_drops.rs
|   `-- metrics.rs
|
|-- drops/
|   |-- mod.rs
|   |-- loaded_cleanup.rs
|   `-- saved_region_cleanup.rs
|
`-- metrics/
    |-- mod.rs
    |-- config.rs
    |-- reporter.rs
    `-- disk_scan.rs
```

## Refactor Rules

- Keep game-thread access to Pumpkin entities/worlds in the event handler or narrowly named game-thread helpers.
- Worker jobs must receive owned snapshots only: block grids, positions, UUIDs, speed values, and cloned lookup tables.
- Do not move `Arc<World>`, `Entity`, `EntityBase`, or live Pumpkin entity handles into worker-pool jobs.
- Preserve focused unit tests when moving functions. Pathfinding, grid indexing, movement math, and clustering should remain testable without a running server.
- Keep command utilities separate from Mob AI. Avoid mixing admin cleanup, metrics, and entity-control code in the same module.

## Build And Verification

- Use `cargo fmt` after Rust edits.
- Use `cargo test mob_ai` for Mob AI changes.
- Use `cargo check` before handoff.
- Use `.\compile.bat` from this directory to build/copy the plugin DLL. The root-level `..\..\compile.bat` also runs the server after building/copying.
