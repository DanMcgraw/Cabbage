# Cabbage MMO Module

This module implements a small MMORPG-style levelling system for Pumpkin.
It currently supports **Mining** and **Combat** skills, persists player
progress in SQLite, and shows a transient bossbar (similar to mcMMO) when a
player earns XP. It also replaces disabled world-generated ores with a
configurable discovery mechanic: mining natural stone can expose a biome- and
height-dependent ore vein behind the mined face.

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
|-- ore_reveal/        # ore config, probability, shape, and provenance tracking
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
| `ore_reveal/` | Validated ore rules, deterministic vein growth, world replacement, and player-placed host tracking. |
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

CREATE TABLE non_natural_blocks (
    world_name    TEXT NOT NULL,
    dimension_name TEXT NOT NULL,
    x INTEGER NOT NULL,
    y INTEGER NOT NULL,
    z INTEGER NOT NULL,
    PRIMARY KEY (world_name, dimension_name, x, y, z)
);
```

- `player_skills` stores cumulative XP per (player, skill). The level is
  derived on read via `LevelCurve::level_for_xp`.
- `non_natural_blocks` is a sparse denylist of player-placed stone/deepslate,
  loaded into memory at startup and persisted in per-tick batches.

Mob and block XP rewards are static balance configuration and live in RON.
When upgrading, Cabbage migrates any customized `mob_xp` and `ore_xp` rows
into RON once, then drops those obsolete SQLite tables.

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
        reward_config_version: 1,
        xp_rewards: (
            mobs: {
                "zombie": 12,
                "skeleton": 14,
                "creeper": 18,
            },
            blocks: {
                "coal_ore": 8,
                "deepslate_coal_ore": 10,
                "diamond_ore": 60,
                "deepslate_diamond_ore": 70,
                "emerald_ore": 50,
                "deepslate_emerald_ore": 55,
            },
        ),
        ore_reveal: (
            enabled: true,
            host_blocks: ["stone", "deepslate"],
            max_total_frequency: 0.25,
            shape: (
                max_radius: 5,
                branch_chance: 0.22,
                forward_bias: 1.6,
                require_hidden_targets: true,
                max_vein_size: 32,
            ),
            ores: [
                (
                    id: "diamond",
                    stone_block: "diamond_ore",
                    deepslate_block: "deepslate_diamond_ore",
                    base_frequency: 0.001,
                    size: (min: 2, max: 4),
                    height_bands: [
                        (min_y: -64, max_y: 16, frequency: 1.0, size: 1.0),
                    ],
                ),
            ],
            biome_multipliers: {
                "badlands": (
                    frequency: 1.0,
                    size: 1.0,
                    ore_frequency: {"diamond": 0.75},
                    ore_size: {},
                ),
            },
        ),
    )),
)
```

`LevelCurve` precomputes cumulative XP thresholds from `base_xp` and
`xp_multiplier` so level lookups are O(1). The shipped ore defaults include
coal, iron, copper, gold, redstone, lapis, diamond, and emerald. Frequencies
are per eligible natural block break. Matching biome and height frequency
multipliers are multiplied together; size multipliers are applied to the
random inclusive `min..max` size. An empty `height_bands` list makes an ore
eligible at every height. Biome keys use Pumpkin registry IDs such as
`badlands`, `stony_peaks`, and `dripstone_caves`.

## Threading Model

- All Pumpkin event handlers run on the async Tokio runtime.
- SQLite access is **never** performed directly on the game thread.
- Mob and block reward lookups are in-memory reads from the reloaded RON config.
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
5. Add its reward source to `xp_rewards` in `config.rs`.

## Notes

- `Cargo.toml` uses `rusqlite = { version = "0.40.1", features = ["bundled"] }`,
  which is the latest stable release as of this writing.
- Bossbars last **100 ticks** (5 seconds at 20 TPS) and refresh on every XP
  gain while visible.
