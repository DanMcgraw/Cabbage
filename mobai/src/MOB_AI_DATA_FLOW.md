# Mob AI Data Flow

This document describes how the `mobai/` crate (the Mob AI module inside the `Cabbage.Core` plugin) separates game-thread work from worker-pool work. Keep this model intact when changing Mob AI.

## Ownership Model

```text
Pumpkin game thread
|-- reads live worlds, players, mobs, blocks, and entity attributes
|-- mutates live entities: position steps, rotation, movement input, path cache cleanup
|-- creates owned snapshots for background jobs
|
Rayon worker pool
|-- computes paths from owned BlockGrid snapshots sampled from the chunk registry
|-- writes results into shared maps by mob UUID
|
Shared synchronized state
|-- managed_mobs: UUIDs of managed mob types tracked via entity lifecycle events
|-- last_path_ticks: cooldown for path recalculation
|-- active_path_jobs: one path job per mob
|-- path_steps: queued block steps from completed path jobs
|-- path_endpoints: cached (start, goal) that produced the current path_steps entry
|-- mob_locations: latest game-thread snapshot for clustering
|-- player_trees / last_player_positions: cached Dijkstra search trees per player
|-- grace_period_mobs: first-seen tick per entity (AI sub-system init grace window)
|-- disabled_mobs: managed mobs whose Pumpkin AI/control state has already been cleared
|-- frozen_out_of_bounds_mobs: managed mobs already stopped after leaving the active AI window
`-- chunk_registry: flashmap of per-chunk passability (ChunkSend feed + block updates)
```

## Per-Tick Flow

1. `MobAiState::run_tick` runs from `ServerTickStartEvent` (only on `handle_blocking` to ensure game-thread safety; the non-blocking `handle` is deliberately empty — touching live world state off the game thread caused access violations).
2. Entity lifecycle events (`EntitySpawnEvent`, `EntityRemoveEvent`, `ChunkEntityLoadEvent`, `ChunkEntityUnloadEvent`) maintain `managed_mobs` and prune stale state reactively.
3. Every 100 ticks the chunk registry is swept to drop chunks no longer loaded in any world.
4. Player search-tree pre-computation: for each player whose block position changed, a worker job samples a `BlockGrid` from the chunk registry and rebuilds that player's `PlayerSearchTree` (shared by nearby mobs).
5. The game thread builds the watched-chunk set (3x3 chunks around each player) per world and samples entities only in those chunks, filtering to managed mob types: creeper, zombie, skeleton.
6. For each active mob, the game thread:
   - registers it and clears Pumpkin's AI/control state on first sight
   - skips it during its 40-tick grace window (`ENTITY_LOAD_GRACE_TICKS`)
   - finds the nearest player and aims the head (yaw/pitch clamped per tick)
   - drops the path and stops the mob when the height difference to the target is exceeded
   - follows queued `path_steps` by direct position steps at the mob's movement-speed attribute, popping reached steps, discarding obstructed paths, and aligning `yaw`/`body_yaw` to the walk direction (clamped per tick)
7. Path recalculation is throttled per mob: one active job at a time, an interval scaled by horizontal distance, and no new job while the cached `(start, goal)` endpoints are unchanged. Due jobs are sorted nearest-player-first and submitted to the worker pool.
8. The update period itself scales with managed mob count (4 ticks at ≤600 mobs, growing beyond).
9. On each update-period tick, the game thread rebuilds `mob_locations` from the active-mob snapshot, computes the anti-clump push per mob, and applies it directly.
10. Managed mobs that leave the watched-chunk window are frozen once on the game thread: Pumpkin AI/control state cleared, movement input and jumping cleared, live velocity zeroed. State for removed/unloaded entities is pruned by the lifecycle events.
11. Stale player state (`player_trees`, `last_player_positions`) is pruned to online players after the scan.

## Pathfinding Jobs

Path jobs run on the Rayon pool through `spawn_path_job` (implemented in `mobai/src/workers.rs`).

Inputs:
- mob UUID
- mob start `BlockPos`
- target player `BlockPos`
- optional shared `PlayerSearchTree` for that player

Worker responsibilities:
- sample an owned `BlockGrid` from the lock-free chunk registry (no `Arc<World>`, no live world reads)
- run `connect_to_player_tree` when a player tree is available, otherwise `bidirectional_a_star`
- convert the path into queued movement steps with `movement_path_steps`
- publish `VecDeque<BlockPos>` into `path_steps` (or remove the entry when no path exists)
- remove the UUID from `active_path_jobs` via `ActiveJobGuard`

The game thread records `path_endpoints` before submission so duplicate jobs are never spawned for unchanged endpoints. Workers must not read live worlds or mutate entities.

## Game-Thread Mutation

Only the game thread applies worker results and touches live entities:

- Path following steps the entity directly along `path_steps` (`set_pos` + `send_pos_rot`) at the mob's movement-speed attribute, stepping up/down when horizontally close to the next node.
- Rotation is split: `yaw`/`body_yaw` align to the walk direction (clamped 40°/tick), while head yaw/pitch track the nearest player's eye position (clamped 10°/tick).
- The anti-clump push from `clustering::cluster_push_velocity` is applied as a direct position nudge.
- Freezing a mob that left the active window clears movement input/jumping and zeroes velocity exactly once per mob.
- The chunk registry is fed on the game thread by `ChunkSend` events and updated in place by block place/break events.

Keep all direct entity mutation here or in helpers called only from this game-thread path.

## Module Layout

The crate split separates code along these boundaries:

- `lib.rs`: `MobAiState`, `EventHandler` impls, game-thread orchestration, and the `MobAiApiAdapter` that exposes the engine through `cabbage-api`.
- `plugin.rs`: module lifecycle, event registration, and `MobAiService` publication.
- `types.rs`: snapshots, `ChunkPassability`, and small data structs shared across submodules.
- `pathfinding.rs`: `PathBounds`, `BlockGrid`, bidirectional A*, player search trees, movement costs, and heuristics.
- `movement.rs`: weighted lookahead helper (retained with its tests; not currently wired into the tick loop).
- `clustering.rs`: `MobLocationTable`, cluster cells, and cluster push velocity calculation.
- `workers.rs`: Rayon thread-pool setup, `spawn_path_job`, and `ActiveJobGuard` cleanup.
