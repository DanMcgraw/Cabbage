# Mob AI Data Flow

This document describes how `src/mob_ai.rs` separates Pumpkin game-thread work from Rayon worker-pool work. Keep this model intact when changing Mob AI.

The central rule is thread affinity:

- Live Pumpkin worlds, entities, players, blocks, attributes, velocity fields, yaw fields, and dirty flags are game-thread-only.
- Rayon workers may only operate on owned snapshots, cloned tables, copied values, and synchronized result mailboxes.
- `MobAiState::run_tick` must only be reached from the blocking event path. The non-blocking event handler must not call `run_tick`.

## Ownership Model

```text
Pumpkin game thread only
|-- enters Mob AI through handle_blocking(ServerTickStartEvent)
|-- reads live worlds, players, mobs, blocks, and entity attributes
|-- mutates live entities: velocity, rotation, dirty flags
|-- applies completed worker results
|-- creates owned snapshots for background jobs
|-- writes path job requests by spawning Rayon jobs
|-- writes velocity job requests by spawning Rayon jobs
|-- prunes stale result/cache data for mobs no longer seen
|
Rayon worker pool only
|-- computes paths from owned BlockGrid snapshots
|-- computes velocity plans from owned VelocityJobSnapshot data
|-- reads cloned MobLocationTable snapshots
|-- writes completed results into synchronized mailboxes by mob UUID
|-- owns cleanup of active job markers through ActiveJobGuard
|
Shared synchronized state
|-- last_path_ticks: game-thread-maintained cooldown for path recalculation
|-- active_path_jobs: worker-owned lifecycle marker, one path job per mob
|-- path_steps: queued block steps from completed path jobs
|-- mob_locations: latest game-thread-built snapshot for clustering
|-- active_velocity_jobs: worker-owned lifecycle marker, one velocity job per mob
`-- planned_velocities: worker-computed velocity plans awaiting game-thread apply
```

## Event Entry Invariant

`MobAiState::run_tick` must be called only from `handle_blocking`.

The regular non-blocking `handle` path must be a no-op or must otherwise avoid all live Pumpkin API access. It must not call `run_tick`, because that path may execute on an async executor thread rather than the Pumpkin game thread.

```text
ServerTickStartEvent
|-- handle(...)
|   `-- no-op for Mob AI
|
`-- handle_blocking(...)
    `-- MobAiState::run_tick(...)
        `-- live Pumpkin world/entity access is allowed here
```

This invariant exists because Rust `Mutex` protection around Mob AI maps does not protect Pumpkin's internal world/entity memory. The Mob AI locks only protect Mob AI's own shared state.

## Per-Tick Flow

1. `MobAiState::run_tick` runs from `handle_blocking(ServerTickStartEvent)` only.
2. The game thread scans loaded worlds and managed mob types: creeper, zombie, skeleton.
3. For each mob, the game thread gathers live data:
   - current position and block position
   - nearest player block position
   - movement speed attribute
   - current path target from `path_steps`, when velocity work is due
   - sampled block grid for pathfinding, when the path cooldown allows it
4. Every `MOB_MOVE_PERIOD_TICKS`, before building the velocity snapshot, the game thread applies any completed `VelocityPlan` for the mob.
5. After applying completed velocity, the game thread captures the current mob snapshot:
   - UUID
   - world UUID
   - current position
   - current block position
   - current velocity after any completed plan was applied
   - movement speed
   - current path target and next step, when available
6. If a path update is due, the game thread samples blocks into an owned `BlockGrid` and submits a path job.
7. After all worlds/entities are scanned, every `MOB_MOVE_PERIOD_TICKS`, the game thread:
   - rebuilds `mob_locations` from active mob snapshots
   - submits velocity jobs for mobs with available snapshot data
8. The game thread prunes stale non-active result/cache data after the scan.

## Pathfinding Jobs

Path jobs run on the Rayon pool through `spawn_path_job`.

Inputs:

- mob UUID
- owned `BlockGrid`
- mob start `BlockPos`
- target player `BlockPos`

Worker responsibilities:

- create an `ActiveJobGuard` for the UUID and `active_path_jobs`
- run `bidirectional_a_star`
- convert the path into queued movement steps with `movement_path_steps`
- publish `VecDeque<BlockPos>` into `path_steps`, or remove the mob's `path_steps` entry if no valid path exists
- allow `ActiveJobGuard` to remove the UUID from `active_path_jobs` on all exits, including panic unwind if unwinding is enabled

Workers must not read live worlds, read live entities, mutate entities, or call Pumpkin world/entity APIs.

## Velocity Jobs

Velocity jobs run on the Rayon pool through `spawn_velocity_jobs`.

Inputs:

- owned `VelocityJobSnapshot`
- cloned `MobLocationTable`
- current path target and next step, when available

Worker responsibilities:

- create an `ActiveJobGuard` for the UUID and `active_velocity_jobs`
- compute path-following velocity with weighted lookahead
- compute cluster push velocity so mobs spread apart
- combine these into a `VelocityPlan`
- publish the plan into `planned_velocities`, when movement is needed
- allow `ActiveJobGuard` to remove the UUID from `active_velocity_jobs` on all exits, including panic unwind if unwinding is enabled

Workers must only calculate. They do not call Pumpkin entity methods.

## Active Job Marker Ownership

`active_path_jobs` and `active_velocity_jobs` are lifecycle markers for jobs currently running on the worker pool.

Rules:

- The game thread may insert a UUID before spawning a job.
- If the game thread decides not to spawn the job after insertion, it must remove the UUID immediately.
- Once a worker job is spawned, the worker owns cleanup of that UUID through `ActiveJobGuard`.
- `retain_seen_mobs` must not prune `active_path_jobs` or `active_velocity_jobs`.

This prevents the game thread from deleting an active marker while a worker is still running, which could allow duplicate jobs for the same mob UUID.

## Shared State Semantics

Treat shared maps as mailboxes and caches, not as shared live ownership.

```text
last_path_ticks
- Written by game thread.
- Read by game thread.
- Pruned by game thread.
- Used only as a path recalculation cooldown.

active_path_jobs
- Inserted by game thread before path job spawn.
- Removed by game thread only if no job is spawned.
- Removed by worker through ActiveJobGuard after spawn.
- Not pruned by retain_seen_mobs.

path_steps
- Written by path workers after path completion.
- Read and consumed by game thread through path_velocity_target.
- Pruned by game thread for mobs no longer seen.

mob_locations
- Rebuilt by game thread from active mob snapshots.
- Cloned by game thread before velocity jobs are spawned.
- Read by workers only through the cloned MobLocationTable.

active_velocity_jobs
- Inserted by game thread before velocity job spawn.
- Removed by worker through ActiveJobGuard after spawn.
- Not pruned by retain_seen_mobs.

planned_velocities
- Written by velocity workers after velocity planning.
- Removed and applied by game thread through apply_planned_velocity.
- Pruned by game thread for mobs no longer seen.
```

## Game-Thread Mutation

Only the game thread applies worker results.

`apply_planned_velocity` must:

1. lock `planned_velocities`
2. remove the `VelocityPlan` for the mob UUID
3. drop the lock before touching the live entity
4. update entity body/head yaw from the steering delta
5. store velocity
6. mark `velocity_dirty`

Keep all direct entity mutation here or in helpers called only from this game-thread path.

No Mob AI `Mutex` should be held while mutating a live Pumpkin entity unless the lock is strictly local and known not to interact with Pumpkin internals. Prefer removing/copying the data from the Mob AI map, dropping the lock, then touching the entity.

## Locking Rules

Use `Mutex` only around Mob AI-owned maps and tables.

Do not assume these locks protect Pumpkin internals:

- `server.worlds`
- `world.players`
- `world.entities`
- live entity position, velocity, yaw, head yaw, body yaw, dirty flags
- live block state reads
- live entity attributes

The locks protect only the container they wrap. Thread-affinity still controls whether Pumpkin APIs may be called.

## Trace Logging

Trace logging should help prove thread affinity and worker lifecycle behavior.

Useful trace points:

- `handle` skipped Mob AI tick
- `handle_blocking` entered Mob AI tick
- `run_tick` start/end with tick number and thread name/id
- path job spawned/completed with mob UUID
- velocity job spawned/completed with mob UUID
- `ActiveJobGuard` removed active job marker
- `apply_planned_velocity` applied plan with mob UUID

Trace logging must not require holding Mob AI locks while calling Pumpkin APIs.

## Refactor Boundaries

When `src/mob_ai.rs` grows, split along these responsibilities:

- `pathfinding.rs`: `PathBounds`, `BlockGrid`, A*, movement costs, heuristic.
- `movement.rs`: weighted lookahead, velocity planning, yaw/pitch helpers.
- `workers.rs`: job submission, `ActiveJobGuard`, active-job bookkeeping, result publication.
- `clustering.rs`: `MobLocationTable`, cluster cells, cluster push velocity.
- `types.rs`: snapshots and small data structs shared across modules.
- `mod.rs`: `MobAiState`, event handling, game-thread orchestration.

The invariant after refactor: worker modules may depend on owned data types, snapshots, cloned tables, and synchronized Mob AI mailboxes, but must not depend on live Pumpkin entity/world mutation.

## Crash-Avoidance Checklist

Before changing Mob AI, verify these conditions:

- `handle` does not call `run_tick`.
- `handle_blocking` is the only event path that reaches live Pumpkin world/entity access.
- Workers receive no `World`, `Entity`, `EntityBase`, `LivingEntity`, player reference, or borrowed live Pumpkin object.
- Workers receive only owned/copy/clone data.
- Workers clean active job markers with `ActiveJobGuard`.
- `retain_seen_mobs` does not prune active job sets.
- `apply_planned_velocity` removes a plan from `planned_velocities` and drops the lock before mutating the entity.
- Velocity snapshots are captured after any completed velocity plan is applied.
- Path and velocity result maps are treated as mailboxes.
- Trace logs can confirm the game-thread entry path and worker job lifecycle.
```
