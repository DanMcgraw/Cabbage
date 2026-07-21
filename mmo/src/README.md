# Cabbage MMO Module

This crate implements a three-branch MMORPG-style levelling system for
Pumpkin, following the phased plan in `plan.md`. It supports **23
skills** across the Frontier, Warfare, and Enterprise branches, persists
player progress in SQLite, and shows a transient bossbar (similar to mcMMO)
when a player earns XP. It also replaces disabled world-generated ores with a
configurable discovery mechanic: mining natural stone can expose a biome- and
height-dependent ore vein behind the mined face.

## Module Layout

```text
mmo/src/
|-- README.md          # this file
|-- BALANCE.md         # default balance profile and migration guide
|-- plan.md            # phased implementation plan
|-- lib.rs             # MmoState, EventHandler impls, config load/save, feature blacklist
|-- plugin.rs          # DLL exports, metadata, event/command registration
|-- skills.rs          # SkillId (23 skills), BranchId, branch/skill metadata
|-- progression.rs     # central award_xp path, XpSource, branch mastery, snapshots
|-- audit.rs           # queued append-only audit worker (mmo-audit.log)
|-- perks/             # perk scaffolding: cooldowns, batch breaks, level gates
|-- persistence/       # typed versioned codecs for Pumpkin player/item/entity/block data
|-- frontier/          # Frontier skill handlers + branch config
|-- warfare/           # Warfare skill handlers + branch config
|-- enterprise/        # Enterprise skill handlers + branch config
|-- ui/                # bossbars, protected native skill menu, default chat summary grid
|-- commands.rs        # /mmo command tree and admin subcommands
|-- config.rs          # RON config, per-skill level curves, global perk caps
|-- db.rs              # SQLite worker thread, schema migrations, async DB API
`-- ore_reveal/        # ore config, probability, shape, and provenance tracking
```

## Skill Model

| Branch | Skills |
|---|---|
| Frontier | Agriculture, Herbalism, Woodcutting, Mining, Excavation, Fishing, Husbandry, Taming |
| Warfare | Blades, Axes, Archery, Unarmed, Defense, Acrobatics, Sorcery |
| Enterprise | Smithing, Repair, Salvage, Alchemy, Enchanting, Tinkering, Trading, Charisma |

Each skill levels independently (default max level 100); branch mastery is the
average of its member skill levels. The legacy `Combat` skill is retired: its
stored XP is preserved in a `legacy_combat_xp` record (SQLite schema v1) until
an administrator migrates it via `combat_migration.target` in config.ron or
`/mmo migrate combat <skill>`. `Mining` XP remains Mining XP.

## Perks

Perk effects are gated by the global `perks` config (`enabled` kill switch,
batch caps, proc-chance caps) and per-skill knobs in the `frontier` config
section. Cooldowns are tick-based and in-memory. Multi-break perks always go
through `Context::break_blocks` (max 128 blocks, deduplicated, protection-
and durability-aware); the cooldown is charged before the transaction so the
events fired per broken block cannot re-trigger the perk recursively.

| Perk | Skill | Activation | Effect |
|---|---|---|---|
| Prospector | Mining | passive | Chance (level-scaled, capped) to add one item copied from an eligible ore's normal drop list. Never grants bonus Mining XP. |
| Vein Miner | Mining | sneak + break ore | Breaks the connected ore vein in one bounded transaction. |
| Heartwood | Woodcutting | passive | Chance (capped) of one bonus log + bonus XP on natural log breaks. |
| Timber | Woodcutting | sneak + break natural log | Fells connected logs of the same type, bounded. |
| Harvest bonus | Agriculture | passive | Chance (capped) of one bonus crop item on mature harvests; fertilized crops (bone meal) get a deterministic roll and bonus XP. |
| Reel | Fishing | passive | Extra vanilla experience on a successful catch. |
| Treasure replacement | Fishing | passive | Configured caught items are swapped for their mapped replacement (off by default). |
| Quality yield | Herbalism | passive | Chance (capped) of one bonus item on natural plant breaks. |
| Consumable healing | Herbalism | passive | Configured plant foods restore bonus health (bounded). |
| Earthmover | Excavation | sneak + break diggable block | Excavates connected blocks of the same type, bounded. |
| Archaeology loot | Excavation | passive | Chance (capped) of a configured bonus item on diggable breaks. |
| Skill damage | Blades / Axes / Unarmed | passive | Level-scaled attack damage bonus, capped per skill and by the global damage cap. |
| Riposte | Blades | passive (cooldown) | Bonus damage when striking shortly after taking damage. |
| Knockback | Unarmed | passive | Level-scaled knockback bonus, capped. |
| Resilience | Defense | passive | Level-scaled incoming-damage reduction, capped. |
| Roll | Acrobatics | passive | Level-scaled fall-damage reduction, capped. |
| Healing bolt | Sorcery | right-click staff (mana + cooldown) | Restores bounded health; costs mana. |
| Repair discount | Repair | anvil (prepare preview) | Level-cost reduction while off cooldown; cooldown charged on take. |
| Salvage bonus | Salvage | grindstone (prepare preview) | Bonus disenchant experience; cooldown charged on take. |
| Material recovery | Salvage | grindstone take | Chance (capped) of one tool-tier material item. |
| Offer discount | Enchanting | enchanting table (offer preview) | Level-requirement reduction, capped per level. |
| Anvil marking | Smithing | anvil output | Adds creator/provenance item data; vanilla result preserved. |

Warfare XP attribution: melee weapons are classified from the attack event's
weapon snapshot (`_sword` → Blades, `_axe` → Axes, empty hand → Unarmed, bow
and crossbow → the projectile path). Kill XP is awarded exactly once from
Pumpkin's authoritative `PlayerKillEntityEvent`; its damage attribution and
weapon snapshot select the skill without consulting the player's later
inventory. Archery additionally earns small per-hit XP through projectile
owner provenance. Defense XP comes from damage taken; Acrobatics XP from fall
damage; Sorcery XP from casting.

Enterprise notes: Smithing earns craft and furnace-extraction XP; Alchemy
earns XP only for explicitly configured potion item keys after a committed
consumption (brewing itself is not attributable — `BrewEvent` carries no
player — and potency mutation has no safe hook, both documented as blocked).
Repair, Salvage, and Enchanting correlate previews with Pumpkin transaction
IDs and award XP only from their committed completion events. `CraftItemEvent`
is observational in this Pumpkin build, so creator/provenance markers are
written onto honored anvil outputs instead.
Trading and Charisma stay **disabled**: there is no villager-trade commit
transaction or general economy hook yet. Their config sections and the
`rep_v1` reputation ledger exist so server owners can migrate in later
without a schema change.

XP-only Frontier sources: Husbandry uses committed breeding and animal-product
events, with bounded configurable newborn trait rolls. Taming uses completed
tames and owner-validated, consumed pet feeds. Pumpkin does not yet emit the
feed completion for heal/trust interactions with already-tamed pets, so bond
feeding remains dormant rather than awarding pre-action XP. Likewise, the
bone-meal completion event is currently wired for bamboo but not ordinary
crops; Agriculture listens to the committed event and will begin tracking
fertilizer provenance when Pumpkin emits it for those crops.

Player-placed ores, logs, plants, and diggable blocks never earn XP or feed
perks: placements of tracked block types are recorded in the shared
provenance tracker (the same `non_natural_blocks` denylist ore reveal uses)
and excluded at break time.

## Data Flow

### Earning XP

All XP flows through one central path: `progression::award_xp(state, player,
skill, amount, source)`. It enforces module/skill enabled checks, max-level
behavior, the configured per-award clamp, level-up presentation, bossbar
refresh, and audit logging.

```text
Pumpkin event (BlockBreak, ...)
        │
        ▼
MmoState event handler (lib.rs)
        │
        ▼
frontier::mining::handle_block_break (skill handler)
        │
        ▼
progression::award_xp
        │
        ├──► MmoDatabase::add_xp ──► dedicated SQLite worker thread
        │       │
        │       ▼
        │   INSERT/UPDATE player_skills
        │   returns XpResult { awarded_xp, new_level, new_xp, leveled_up }
        │
        ├──► send level-up chat message + celebration (if configured)
        │
        └──► MmoState::show_xp_bossbar
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

## Persistence

- **Skill XP** lives in SQLite (`player_skills`), the durable authority.
- **Item quality/provenance** rides on the `ItemStack` under the `cabbage`
  namespace (`persistence::item::ItemDataV1`).
- **Player capability state** uses `Context` player data
  (`persistence::player::PlayerProfileV1`).
- **Pet profiles** use `Context` entity data (`persistence::entity::PetDataV1`).
- **Crop state** uses `Context` block metadata
  (`persistence::block::CropDataV1`).

All payloads are versioned `NbtCompound`s decoded through typed codecs;
malformed or version-incompatible data decodes to `None` instead of crashing
an event handler.

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

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE legacy_combat_xp (
    player_uuid TEXT PRIMARY KEY,
    xp INTEGER NOT NULL DEFAULT 0
);
```

- `player_skills` stores cumulative XP per (player, skill). The level is
  derived on read via `LevelCurve::level_for_xp`.
- `non_natural_blocks` is a sparse denylist of player-placed stone/deepslate,
  loaded into memory at startup and persisted in per-tick batches.
- `meta` records the schema version (`schema_version`) and migration markers
  (`combat_migrated_to`). Migrations are idempotent and run on open.
- `legacy_combat_xp` preserves retired Combat XP until an administrator
  migrates it.

Mob and block XP rewards are static balance configuration and live in RON.
When upgrading, Cabbage migrates any customized `mob_xp` and `ore_xp` rows
into RON once, then drops those obsolete SQLite tables.

## Configuration

The plugin config is stored as RON in the Cabbage.Mmo data folder
(`plugins/Cabbage.Mmo/config.ron`). `config_version`
tracks the MMO config schema; older files gain safe defaults for missing
sections and are saved back on upgrade. Unknown skill names in the `skills`
map (e.g. the retired `Combat`) are skipped with a warning.

```ron
PluginConfig(
    metrics_log: false,
    mob_ai: true,
    mmo: Some(MmoConfig(
        enabled: true,
        config_version: 1,
        message_on_level_up: true,
        save_interval_ticks: 6000,
        skills: {
            Mining: (max_level: 100, base_xp: 50, xp_multiplier: 1.15, enabled: true),
            // ... every skill in the three branches
        },
        progression: (max_xp_per_award: 10000),
        perks: (
            enabled: true,
            batch_break_max_blocks: 16,      // hard-capped at 128
            batch_break_cooldown_ticks: 100,
            max_damage_multiplier: 2.0,
            max_proc_chance: 0.35,
            max_effect_area_radius: 4,
        ),
        combat_migration: (target: None),    // or Some(Blades) etc.
        frontier: (
            mining: (
                prospector_enabled: true,
                prospector_base_chance: 0.05,
                prospector_chance_per_level: 0.002,
                prospector_max_chance: 0.35,
                vein_miner_enabled: true,
                vein_miner_max_blocks: 16,
            ),
            woodcutting: (
                log_xp: { "oak_log": 6, /* ... */ },
                heartwood_chance: 0.02,
                heartwood_xp_bonus: 25,
                timber_enabled: true,
                timber_max_blocks: 32,
            ),
            agriculture: (
                crops: { "wheat": (xp: 10, max_age: 7, bonus_item: "wheat"), /* ... */ },
                harvest_bonus_chance: 0.10,
                fertilizer_bonus_xp: 10,
                fertilizer_guarantees_bonus: true,
            ),
            fishing: (
                catch_xp: { "cod": 20, /* ... */ },
                default_catch_xp: 10,
                reel_exp_bonus: 2,
                treasure_replacements: {},
            ),
        ),
        reward_config_version: 1,
        xp_rewards: (
            mobs: { "zombie": 12, /* ... */ },
            blocks: { "coal_ore": 8, /* ... */ },
        ),
        ore_reveal: ( /* see ore_reveal/ defaults */ ),
    )),
)
```

`LevelCurve` precomputes cumulative XP thresholds from `base_xp` and
`xp_multiplier` so level lookups are O(1). Out-of-range values (batch caps,
proc chances, curve parameters) are clamped on load via `MmoConfig::sanitized`.

## Commands

- `/mmo` — show your skill summary as a 10-line chat grid (players);
  command help (console/RCON).
- `/mmo menu [player]` — open the protected 9x3 skill grid GUI for you or
  another online player.
- `/mmo stats [player]` — the same chat grid for you or another online
  player (players); full text table (console/RCON).
- `/mmo stats chat [branch]` — alias for the chat summary grid, or one
  branch per page with full XP values.
- `/mmo help [page]` — compact, paginated command list.
- `/mmo top <skill>` — top players for any skill.
- `/mmo reload` — reload config.ron (admin).
- `/mmo setxp <player> <skill> <xp>` — set a player's skill XP (admin).
- `/mmo migrate status` — legacy Combat XP preservation status (admin).
- `/mmo migrate combat <skill>` — move preserved Combat XP to a skill (admin).

The default chat summary is a three-column table — one column per branch,
one row per skill index — of fixed 10-character `minecraft:uniform` cells: a
four-letter skill code plus a right-aligned level. Levels above 999 show a
compact `1k+` marker; the exact level stays in hover text. Hover text on
headers and cells carries full branch/skill names, mastery, enabled counts,
progress toward the next level, and total XP. Disabled skills keep their
level in dark gray strikethrough (winning over the max-level bold style,
with hover reporting both states). The summary never assumes a client's
chat dimensions: padding is applied only inside uniform-font cells and each
page stays within 10 explicit lines.

`/mmo menu` opens the protected `Generic9x3` skill grid: one branch per row,
a branch summary in each row's first slot, all 23 skills visible at once, and
a Help slot that closes the menu and prints the command list. It is
read-only — items cannot be taken out or placed into it. Icon names stay
concise while structured lore shows progress toward the next level, total XP,
and explicit Disabled or Max level states.

## Threading Model

- MMO handlers that inspect live Pumpkin state or transactions are registered
  as blocking handlers so each receives the authoritative mutable event in
  Pumpkin's ordered dispatch path.
- SQLite access is **never** performed directly on the game thread.
- XP awards use one atomic worker request that clamps at max level and returns
  the actual amount awarded; bossbar presentation does not reread the row.
- Reward lookups are in-memory reads from the reloaded RON config.
- `MmoDatabase` spawns a single dedicated worker thread that owns the
  `rusqlite::Connection`. Async methods send a request over an `mpsc`
  channel and await the response via a `futures::channel::oneshot`.
- `BossbarState` and `CooldownTracker` use short-lived `std::sync::Mutex`es
  only to touch their internal maps; network calls are awaited without
  holding locks.
- `AuditLog` owns a dedicated writer thread; event handlers enqueue lines and
  never perform append/flush I/O on the event path.

## Notes

- `Cargo.toml` uses `rusqlite = { version = "0.40.1", features = ["bundled"] }`,
  which is the latest stable release as of this writing.
- Bossbars last **100 ticks** (5 seconds at 20 TPS) and refresh on every XP
  gain while visible.
