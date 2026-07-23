# Cabbage MMO Module

This crate implements a three-branch MMORPG-style levelling system for
Pumpkin, following the phased plan in `plan.md`. It supports **18
skills** across the Frontier, Warfare, and Enterprise branches — six per
branch since the six-skill consolidation (`../../skill_consolidation_plan.md`)
merged five skill pairs into shared progression tracks. It persists
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
|-- plugin.rs          # module lifecycle plus event/command registration
|-- skills.rs          # SkillId (18 skills), BranchId, branch/skill metadata, legacy aliases
|-- progression.rs     # central award_xp path, XpSource, branch mastery, snapshots
|-- audit.rs           # queued append-only audit worker (mmo-audit.log)
|-- perks/             # perk scaffolding: cooldowns, batch breaks, level gates
|-- persistence/       # typed versioned codecs for Pumpkin player/item/entity/block data
|-- frontier/          # Frontier skill handlers + branch config
|-- warfare/           # Warfare skill handlers + branch config
|-- enterprise/        # Enterprise skill handlers + branch config
|-- ui/                # bossbars, protected native skill menu, default chat summary grid,
|                      # per-skill detail pages
|-- commands.rs        # /mmo command tree and admin subcommands
|-- config.rs          # RON config, per-skill level curves, global perk caps
|-- db.rs              # SQLite worker thread, schema migrations, async DB API
`-- ore_reveal/        # ore config, probability, shape, and provenance tracking
```

## Skill Model

| Branch | Skills |
|---|---|
| Frontier | Cultivation, Woodcutting, Mining, Excavation, Fishing, AnimalHandling |
| Warfare | Blades, Axes, Archery, Athletics, Defense, Sorcery |
| Enterprise | Smithing, Maintenance, Alchemy, Enchanting, Tinkering, Commerce |

Each skill levels independently (default max level 100); branch mastery is the
average of its six member skill levels. Five skills are merged tracks that
share one XP pool and one level with a retired partner skill:

- `Cultivation` merges Agriculture + Herbalism (crop and herbal activities).
- `AnimalHandling` merges Husbandry + Taming (breeding and taming activities).
- `Athletics` merges Unarmed + Acrobatics (empty-hand combat and fall actions).
- `Maintenance` merges Repair + Salvage (anvil and grindstone activities).
- `Commerce` merges Trading + Charisma (both still disabled; see below).

The merged skill is the only `SkillId`: both activities award into the shared
pool and every perk gate reads the shared level, while the activity-specific
config sections (XP values, block/action rules, enable switches) stay
separate. Audit `XpSource` values remain activity-specific (e.g. `Repair` and
`Salvage` stay distinct). Existing databases and configs are upgraded by the
transactional, idempotent SQLite schema v2 and config v2 migrations described
below (sum-of-XP for player rows; deterministic anchor rule for curves).

The legacy `Combat` skill is retired: its
stored XP is preserved in a `legacy_combat_xp` record (SQLite schema v1) until
an administrator migrates it via `combat_migration.target` in `mmo.ron` or
`/mmo migrate combat <skill>`. `Mining` XP remains Mining XP.

For one compatibility period, `SkillId::from_name` and single-value config
fields (including `combat_migration.target`) still accept the ten retired
skill names and route them to their merged destination, but they are never
written back out or shown in help/completion output.

## Perks

Perk effects are gated by the global `perks` config (`enabled` kill switch,
batch caps, proc-chance caps) and per-activity knobs in the branch config
sections. Every gate reads the merged skill's shared level (e.g. both the
crop harvest bonus and the herbal quality yield gate on Cultivation).
Cooldowns are tick-based and in-memory. Multi-break perks always go
through `Context::break_blocks` (max 128 blocks, deduplicated, protection-
and durability-aware); the cooldown is charged before the transaction so the
events fired per broken block cannot re-trigger the perk recursively.

### Milestone Tiers (Levels 10, 25, 50, 100)

All 18 skills feature **perk tiers** at milestone levels 10, 25, 50, and 100 (`perk_tier(level) = 0..=4`).
Milestone tier step-ups scale batch limits (e.g. Timber 32→64, Vein Miner 16→32), raise caps (e.g. Axes damage cap 0.60→0.80, Defense resilience 0.15→0.25), boost proc chances and XP bonuses, and unlock capstone effects (such as Prospector 2-item drops at L100 and AnimalHandling 2nd distinct trait roll at L50+). Crossing a tier emits a chat announcement linked to `/mmo skill <skill>`.

| Perk | Skill | Activation | Effect |
|---|---|---|---|
| Prospector | Mining | passive | Chance (level-scaled, capped) for one bonus ore drop; Tier 4 (L100) drops 2 items. Never grants bonus Mining XP. |
| Vein Miner | Mining | sneak + break ore | Breaks connected ore vein (16 base → 32 at L100 blocks). |
| Heartwood | Woodcutting | passive | Chance (capped) of one bonus log + bonus XP on natural log breaks (tier scaled). |
| Timber | Woodcutting | sneak + break natural log | Fells connected logs of the same type (32 base → 64 at L100 blocks). |
| Harvest bonus | Cultivation | passive | Chance (capped) of one bonus crop item on mature harvests (tier scaled). |
| Reel | Fishing | passive | Extra vanilla experience on a successful catch (+1 XP/tier). |
| Treasure replacement | Fishing | passive | Configured caught items are swapped for their mapped replacement (off by default). |
| Quality yield | Cultivation | passive | Chance (capped) of one bonus item on natural plant breaks (+2%/tier). |
| Consumable healing | Cultivation | passive | Configured plant foods restore bonus health (+0.5 HP/tier). |
| Earthmover | Excavation | sneak + break diggable block | Excavates connected blocks of the same type (16 base → 32 at L100 blocks). |
| Archaeology loot | Excavation | passive | Chance (capped) of a configured bonus item on diggable breaks (+1%/tier). |
| Arrow damage | Archery | passive | Level-scaled projectile damage bonus, capped per tier (x1.50 base → x1.70 at L100). |
| Skill damage | Blades / Axes / Athletics | passive | Level-scaled attack damage bonus, capped per skill and by the global damage cap (tier raised caps). |
| Riposte | Blades | passive (cooldown) | Bonus damage when striking shortly after taking damage (+5%/tier, cooldown −20t/tier). |
| Knockback | Athletics | passive | Level-scaled knockback bonus, capped (+5%/tier cap). |
| Resilience | Defense | passive | Level-scaled incoming-damage reduction, capped (+2.5%/tier cap). |
| Roll | Athletics | passive | Level-scaled fall-damage reduction, capped (+5%/tier cap). |
| Healing bolt | Sorcery | right-click staff (mana + cooldown) | Restores health (+1 HP/tier), max mana (+10/tier), cooldown (−10t/tier); costs mana. |
| Tool Care | Maintenance | passive | Chance (5% base + 2.5%/tier) to refund 1 durability on held tool on block break. |
| Repair discount | Maintenance | anvil (prepare preview) | Level-cost reduction while off cooldown; cap +1 level/tier. |
| Salvage bonus | Maintenance | grindstone (prepare preview) | Bonus disenchant experience; cap +5%/tier. |
| Material recovery | Maintenance | grindstone take | Chance (capped) of one tool-tier material item. |
| Potion Mastery | Alchemy | passive | Bonus health restored on consuming any potion (0.5 base + 0.5 HP/tier). |
| Offer discount | Enchanting | enchanting table (offer preview) | Level-requirement reduction (cap +1 level/tier). |
| Anvil marking | Smithing | anvil output | Adds creator/provenance item data; vanilla result preserved. |

Warfare XP attribution: melee weapons are classified from the attack event's
weapon snapshot (`_sword` → Blades, `_axe` → Axes, empty hand → Athletics, bow
and crossbow → the projectile path). Kill XP is awarded exactly once from
Pumpkin's authoritative `PlayerKillEntityEvent`; its damage attribution and
weapon snapshot select the skill without consulting the player's later
inventory. Archery additionally earns small per-hit XP through projectile
owner provenance. Defense XP comes from damage taken; Athletics also earns XP
from fall damage; Sorcery XP from casting.

Enterprise notes: Smithing earns craft and furnace-extraction XP; Alchemy
earns XP only for explicitly configured potion item keys after a committed
consumption (brewing itself is not attributable — `BrewEvent` carries no
player — and potency mutation has no safe hook, both documented as blocked).
Maintenance (repair and salvage activities) and Enchanting correlate previews
with Pumpkin transaction
IDs and award XP only from their committed completion events. `CraftItemEvent`
is observational in this Pumpkin build, so creator/provenance markers are
written onto honored anvil outputs instead.
Commerce has no live XP source yet: the trading and charisma activity configs
stay **disabled** because there is no villager-trade commit transaction or
general economy hook yet. Their config sections and the
`rep_v1` reputation ledger exist so server owners can migrate in later
without a schema change.

XP-only Frontier sources: AnimalHandling earns from both the husbandry and
taming activity configs — committed breeding and animal-product events, with
bounded configurable newborn trait rolls, plus completed tames and
owner-validated, consumed pet feeds. Pumpkin does not yet emit the
feed completion for heal/trust interactions with already-tamed pets, so bond
feeding remains dormant rather than awarding pre-action XP. Likewise, the
bone-meal completion event is currently wired for bamboo but not ordinary
crops; the agriculture activity handler awards Cultivation from the committed
event and will begin tracking fertilizer provenance when Pumpkin emits it for
those crops.

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
- Schema v2 (six-skill consolidation) consolidates each retired skill pair's
  `player_skills` rows into the canonical destination in a single
  transaction: the destination row's XP becomes the sum of both retired rows
  plus any pre-existing destination row, then the retired rows are deleted.
  Sums saturate at SQLite's signed 64-bit limit rather than overflowing, and
  unrelated skill rows are left untouched. The migration is guarded by the
  schema version, rolls back (and fails the load) if the transaction cannot
  complete, and is a no-op when an already migrated database is reopened.
  Back up `mmo.db` before deploying the first build with this migration; a
  concise success log reports how many player rows were consolidated.
- `legacy_combat_xp` preserves retired Combat XP until an administrator
  migrates it.

Mob and block XP rewards are static balance configuration and live in
`mmo.rewards.ron`. When upgrading, Cabbage migrates any customized `mob_xp`
and `ore_xp` rows into RON once, then drops those obsolete SQLite tables.

## Configuration

MMO configuration is split across three RON files in the unified Cabbage
data folder (`plugins/Cabbage/`); Core separately owns `config.ron` for the
`metrics_log`/`mob_ai` core switches:

- `mmo.ron` — the full `MmoConfig`: module switch, per-skill curves,
  progression and perk bounds, the branch sections (`frontier`, `warfare`,
  `enterprise`), audit, and combat migration. `config_version` tracks the
  MMO config schema (currently 2); older files gain safe defaults for
  missing sections and are saved back on upgrade. Unknown skill names in
  the `skills` map (e.g. the retired `Combat`) are skipped with a warning.
- `mmo.rewards.ron` — the `XpRewardsConfig`: the static mob and block XP
  tables (`mobs`, `blocks`). This is the home of all kill/mining XP values.
- `mmo.ores.ron` — the `OreRevealConfig`: ore-vein reveal rules.

Migration from a unified `config.ron`: on first load the `mmo:` section is
copied out into the three files above (inline `xp_rewards` and `ore_reveal`
included), applying any pending schema upgrade. The old `config.ron` is
left byte-for-byte untouched, and once the split files exist the stale
`mmo:` section is ignored forever. A legacy `config.json` is adopted the
same way (core switches → `config.ron` when missing, MMO defaults → the
split files) and is never modified or deleted.

Config v2 (six-skill consolidation) merges retired pair entries in the
`skills` map deterministically during deserialization: an explicit canonical
entry always wins; otherwise the pair's anchor (Agriculture for Cultivation,
Husbandry for AnimalHandling, Unarmed for Athletics, Repair for Maintenance,
Trading for Commerce) supplies `max_level`/`base_xp`/`xp_multiplier`;
`enabled` is the logical OR of the two retired entries so a partly enabled
pair stays usable; conflicting curves are logged with the anchor's win.
Retired keys are removed and the canonical config is saved once. The nested
activity configs (`frontier.agriculture`, `frontier.herbalism`,
`warfare.unarmed`, `enterprise.repair`, and so on) are untouched — they
configure event sources, not levels.

```ron
// mmo.ron — every MMO setting except XP rewards and ore reveal
(
    enabled: true,
    config_version: 2,
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
)

// mmo.rewards.ron — static mob and block XP tables
(
    mobs: { "zombie": 12, /* ... */ },
    blocks: { "coal_ore": 8, /* ... */ },
)

// mmo.ores.ron — ore-vein reveal rules (see ore_reveal/ defaults)
(
    enabled: true,
    // host_blocks, shape, ores, biome_multipliers
)
```

`LevelCurve` precomputes cumulative XP thresholds from `base_xp` and
`xp_multiplier` so level lookups are O(1). Out-of-range values (batch caps,
proc chances, curve parameters) are clamped on load via `MmoConfig::sanitized`.

## Commands

- `/mmo` — show your skill summary as an eight-line compact chat grid
  (players); command help (console/RCON).
- `/mmo menu [player]` — open the protected 9x3 skill grid GUI for you or
  another online player.
- `/mmo stats [player]` — the same compact chat grid for you or another
  online player (players); full text table (console/RCON).
- `/mmo stats chat [branch]` — compatibility alias for the chat summary
  grid, or one branch in detail with full XP values.
- `/mmo skill <skill> [page]` — your detail page for one skill: level, XP,
  XP sources, live perk effects, and planned milestone unlocks (players).
- `/mmo help [page]` — compact, paginated command list.
- `/mmo top <skill>` — top players for any of the 18 canonical skills.
- `/mmo reload` — reload the MMO config files (`mmo.ron`, `mmo.rewards.ron`,
  `mmo.ores.ron`) and rebuild curves (admin).
- `/mmo setxp <player> <skill> <xp>` — set a player's skill XP (admin).
- `/mmo migrate status` — legacy Combat XP preservation status (admin).
- `/mmo migrate combat <skill>` — move preserved Combat XP to a skill (admin).

Skill arguments suggest and display only the 18 canonical skill names, but
still parse the ten retired names as aliases for one compatibility period
(see Skill Model).

The default chat summary is exactly eight lines — a title with a `/mmo menu`
hint, one branch-header row, and six skill rows — laid out as a three-column
table (one column per branch, one row per skill index) of fixed 12-character
`minecraft:uniform` cells: an up-to-eight-character skill label, one space,
and a three-character right-aligned level with no `L` marker. Levels above
999 show a compact `1k+` marker; the exact level stays in hover text. Hover
text on headers and cells carries full branch/skill names, mastery, enabled
counts, progress toward the next level, and total XP. Disabled skills keep
their level in dark gray strikethrough (winning over the max-level bold
style, with hover reporting both states). Clicking a skill cell suggests the
canonical `/mmo skill <skill>` command (retired names are never suggested).
The summary never assumes a
client's chat dimensions: padding is applied only inside uniform-font cells
and each page stays within the 10-line vanilla chat budget.

`/mmo menu` opens the protected `Generic9x3` skill grid: one branch per row,
a branch summary in each row's first slot (0/9/18), the 18 canonical skills
in slots 1-6, 10-15, and 19-24, a Help slot at 17 that closes the menu and
prints the command list, and inert filler slots at 7-8, 16, and 25-26. It is
read-only — items cannot be taken out or placed into it. Icon names stay
concise while structured lore shows progress toward the next level, total XP,
and explicit Disabled or Max level states. Clicking a skill icon closes the
menu and sends that skill's `/mmo skill` detail page for the menu's target;
branch headers and fillers stay inert.

`/mmo skill <skill> [page]` sends one skill's detail page in chat: a
branch-coloured `<Skill> — <Branch>` heading, the current level with total
XP and progress toward the next level (or `Max level`), an explicit
explanation when the MMO module or the skill itself is disabled (stored
level and XP stay visible), the XP activities that feed the skill (merged
skills name both activity families), and the progression section. Live perk
effects are shown as `Active from level 1` with their live configured value
at the viewer's level, resolved from the current config so `/mmo reload`
takes effect immediately; a disabled global perk switch or individual
feature switch labels the row `Disabled` instead of hiding it. The shared
milestone slots at levels 25/50/75/100 (from `perks::eligibility`) stay
visibly `Planned` until their concrete perk is implemented. A clickable
footer navigates to the previous/next skill in canonical branch order, the
branch page, and `/mmo`. Page 1 always fits the 10-line vanilla chat
budget; skills whose rows overflow (by default Cultivation, Athletics, and
Maintenance) continue on `/mmo skill <skill> 2` without omitting or
duplicating rows. The page is informational only: gameplay handlers remain
the authority on perk activation.

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
