# Cabbage MMO Module

This module implements a small MMORPG-style levelling system for Pumpkin.
It currently supports **Mining** and **Combat** skills, persists player
progress in SQLite, and shows a transient bossbar (similar to mcMMO) when a
player earns XP.

## Module Layout

```text
src/mmo/
|-- README.md          # this file
|-- mod.rs             # MmoState, event handler wiring, public helpers
|-- bossbar.rs         # transient skill-progress bossbars
|-- commands.rs        # /mmo command tree and admin subcommands
|-- config.rs          # RON config, per-skill level curves
|-- db.rs              # SQLite worker thread and async DB API
|-- events.rs          # BlockBreakEvent / EntityDeathEvent XP handlers
|-- player.rs          # player-related utilities
|-- skills.rs          # SkillId enum and skill metadata
```

## Responsibilities

| File | What it owns |
|------|--------------|
| `mod.rs` | `MmoState` — the single shared state used by all event handlers. Loads/saves RON config, owns the DB handle, level curves, and bossbar tracker. |
| `bossbar.rs` | `BossbarState` — per-player, per-skill bossbar entries with an expiry tick. Sends/updates bars and removes stale ones. |
| `commands.rs` | `/mmo` player info and `/mmo admin` XP management commands. |
| `config.rs` | `PluginConfig`, `MmoConfig`, `SkillConfig`, and `LevelCurve`. Computes XP thresholds from RON values. |
| `db.rs` | `MmoDatabase` — a dedicated SQLite worker thread with an async request/response API. |
| `events.rs` | Event-driven XP awards: ore blocks → Mining, mob kills → Combat. |
| `player.rs` | Small helpers for resolving players from server state. |
| `skills.rs` | `SkillId` enum, canonical skill list, and display names. |

## Data Flow

### Earning XP

```text
Pumpkin event (BlockBreak / EntityDeath)
        │
        ▼
MmoState event handler (mod.rs)
        │
        ▼
events::handle_block_break / handle_entity_death
        │
        ├──► MmoDatabase::add_xp ──► dedicated SQLite worker thread
        │       │
        │       ▼
        │   INSERT/UPDATE player_skills
        │   returns XpResult { new_level, new_xp, leveled_up }
        │
        ├──► send level-up chat message (if configured)
        │
        └──► MmoState::show_xp_bossbar
                │
                ├──► MmoDatabase::get_skill
                ├──► LevelCurve::level_for_xp
                └──► BossbarState::show_skill_progress
                        └──► player.send_bossbar
```

### Bossbar Expiry

```text
ServerTickStartEvent
        │
        ▼
MmoState tick handler
        │
        ├──► store event.tick as last_tick
        └──► BossbarState::cleanup_expired
                └──► player.remove_bossbar for expired entries
```

## Public API

### From outside the module

```rust
use std::sync::Arc;
use cabbage::mmo::{MmoState, SkillId, show_xp_bossbar};

// Trigger the skill-progress bossbar for a player manually.
show_xp_bossbar(state.clone(), player, SkillId::Mining).await;
```

### From event handlers that already hold `&MmoState`

```rust
state.show_xp_bossbar(&player, SkillId::Combat, current_tick).await;
```

## Database Schema

All tables live in `mmo.db` inside the plugin data folder.

```sql
CREATE TABLE player_skills (
    player_uuid TEXT NOT NULL,
    skill       TEXT NOT NULL,
    xp          INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (player_uuid, skill)
);

CREATE TABLE mob_xp (
    mob_resource_name TEXT PRIMARY KEY,
    xp                INTEGER NOT NULL
);

CREATE TABLE ore_xp (
    block_name TEXT PRIMARY KEY,
    xp         INTEGER NOT NULL
);
```

- `player_skills` stores cumulative XP per (player, skill). The level is
  derived on read via `LevelCurve::level_for_xp`.
- `mob_xp` maps mob resource names (e.g. `zombie`, `enderman`) to combat XP.
- `ore_xp` maps block names (e.g. `diamond_ore`) to mining XP.

Default values are seeded on first open; admin commands can overwrite them.

## Configuration

The plugin config is stored as RON in the Cabbage data folder:

```ron
PluginConfig(
    metrics_log: false,
    mob_ai: true,
    mmo: Some(MmoConfig(
        enabled: true,
        message_on_level_up: true,
        save_interval_ticks: 6000,
        skills: {
            Mining: (max_level: 99, base_xp: 50, xp_multiplier: 1.15),
            Combat: (max_level: 99, base_xp: 60, xp_multiplier: 1.14),
        },
    )),
)
```

`LevelCurve` precomputes cumulative XP thresholds from `base_xp` and
`xp_multiplier` so level lookups are O(1).

## Threading Model

- All Pumpkin event handlers run on the async Tokio runtime.
- SQLite access is **never** performed directly on the game thread.
- `MmoDatabase` spawns a single dedicated worker thread that owns the
  `rusqlite::Connection`. Async methods send a request over an `mpsc`
  channel and await the response via a `futures::channel::oneshot`.
- `BossbarState` uses a short-lived `std::sync::Mutex` only to touch its
  internal `HashMap`; network calls (`send_bossbar`, `remove_bossbar`) are
  awaited without holding the lock.

## Adding a New Skill

1. Add the variant to `SkillId` in `skills.rs`.
2. Add a default `SkillConfig` entry in `config.rs` (`MmoConfig::default`).
3. Wire an event handler in `events.rs` that awards XP and calls
   `state.show_xp_bossbar(...)`.
4. Register the event in `mod.rs` (if a new event type is needed).
5. Add a default XP source table (or reuse `mob_xp`/`ore_xp`) in `db.rs`.

## Notes

- `Cargo.toml` uses `rusqlite = { version = "0.40.1", features = ["bundled"] }`,
  which is the latest stable release as of this writing.
- Bossbars last **100 ticks** (5 seconds at 20 TPS) and refresh on every XP
  gain while visible.
