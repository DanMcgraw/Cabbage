# Mob AI Data Flow

This document describes how the `src/mob_ai/` module separates game-thread work from worker-pool work. Keep this model intact when changing Mob AI.

## Ownership Model

```text
Pumpkin game thread
|-- reads live worlds, players, mobs, blocks, and entity attributes
|-- mutates live entities: velocity, rotation, dirty flags, path cache cleanup
|-- creates owned snapshots for background jobs
|
Rayon worker pool
|-- computes paths from owned BlockGrid snapshots
|-- computes velocity plans from owned ActiveMobSnapshot/VelocityJobSnapshot data
|-- writes results into shared maps by mob UUID
|
Shared synchronized state
|-- last_path_ticks: cooldown for path recalculation
|-- active_path_jobs: one path job per mob
|-- path_steps: queued block steps from completed path jobs
|-- mob_locations: latest game-thread snapshot for clustering
|-- active_velocity_jobs: one velocity job per mob
|-- planned_velocities: worker-computed velocity plans awaiting game-thread apply
|-- disabled_mobs: loaded managed mobs whose Pumpkin AI/control state has already been cleared
`-- frozen_out_of_bounds_mobs: loaded managed mobs that have already been stopped after leaving the active AI window
```

## Per-Tick Flow

1. `MobAiState::run_tick` runs from `ServerTickStartEvent` (only on `handle_blocking` to ensure game-thread safety).
2. The game thread scans loaded worlds and managed mob types: creeper, zombie, skeleton.
3. For each mob, the game thread gathers live data:
   - current position and block position
   - current velocity
   - nearest player block position
   - movement speed attribute
   - sampled block grid for pathfinding, when the path cooldown allows it
4. If a path update is due, the game thread samples blocks into an owned `BlockGrid` and submits a path job.
5. Every `MOB_MOVE_PERIOD_TICKS`, the game thread:
   - applies any completed `VelocityPlan`
   - snapshots active mob locations
   - submits velocity jobs for mobs with current path targets
6. The game thread prunes stale UUIDs from caches after the scan.
7. Managed mobs that are loaded but outside the active player chunk window are frozen on the game thread only when they first leave that window: Pumpkin AI/control state is cleared once, pending Cabbage path/velocity state is removed by cache pruning, movement input and jumping are cleared, and live velocity is zeroed.

## Pathfinding Jobs

Path jobs run on the Rayon pool through `spawn_path_job` (implemented in `src/mob_ai/workers.rs`).

Inputs:
- mob UUID
- owned `BlockGrid`
- mob start `BlockPos`
- target player `BlockPos`

Worker responsibilities:
- run `bidirectional_a_star`
- convert the path into queued movement steps with `movement_path_steps`
- publish `VecDeque<BlockPos>` into `path_steps`
- remove the UUID from `active_path_jobs` via `ActiveJobGuard`

Workers must not read live worlds or mutate entities.

## Velocity Jobs

Velocity jobs run on the Rayon pool through `spawn_velocity_jobs` (implemented in `src/mob_ai/workers.rs`).

Inputs:
- `VelocityJobSnapshot`
- cloned `MobLocationTable`
- current path target and next step, when available

Worker responsibilities:
- compute path-following velocity with weighted lookahead
- compute cluster push velocity so mobs spread apart
- combine these into a `VelocityPlan`
- publish the plan into `planned_velocities`
- remove the UUID from `active_velocity_jobs` via `ActiveJobGuard`

Workers must only calculate. They do not call Pumpkin entity methods.

## Game-Thread Mutation

Only the game thread applies worker results:

- `apply_planned_velocity` removes a `VelocityPlan` from `planned_velocities` and applies it under minimal lock scope.
- It currently force-locks entity yaw, head yaw, and body yaw to `0.0` for rotation debugging. Pumpkin's normal mob tick remains the only rotation packet sender.
- It stores velocity and marks `velocity_dirty`.
- `freeze_out_of_bounds_mob` handles managed mobs outside the active AI window. It does not submit worker jobs and does not repeat heavy AI/control resets for mobs that are already frozen out of bounds.

Keep all direct entity mutation here or in helpers called only from this game-thread path.

## Module Layout

The module split separates code along these boundaries:

- `mod.rs`: `MobAiState`, event handling, game-thread orchestration.
- `types.rs`: snapshots and small data structs shared across submodules.
- `pathfinding.rs`: `PathBounds`, `BlockGrid`, bidirectional A*, movement costs, and heuristics.
- `movement.rs`: weighted lookahead, velocity planning, and yaw/pitch rotation helpers.
- `clustering.rs`: `MobLocationTable`, cluster cells, and cluster push velocity calculation.
- `workers.rs`: Rayon job submission, thread pool setup, and `ActiveJobGuard` cleanup.
