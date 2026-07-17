# Cabbage MMO Implementation Plan

## Purpose

Implement the Cabbage-owned MMO layer described by
`pumpkin_needs_plan.md`, using the Pumpkin APIs now available in the sibling
Pumpkin checkout. This plan deliberately contains **no Pumpkin core work**.
Cabbage owns progression, balance, perks, cooldowns, custom-item rules,
quality, quests, economy, and player-facing presentation. Pumpkin remains the
authority for vanilla transactions and entity/world state.

This is a replacement planning document for Cabbage work. It does not change
the existing Mob AI scope.

## Current starting point

The MMO module currently has:

- `Mining` and `Combat` as the only `SkillId` variants.
- RON-configured level curves and static block/mob XP rewards.
- SQLite as the durable source for per-skill cumulative XP.
- Block-break and entity-death XP handlers, transient bossbars, `/mmo` stats,
  top, reload, and admin XP commands.
- Ore-reveal/provenance logic for Mining.

Pumpkin now provides the following Cabbage-consumable building blocks:

| Cabbage need | Pumpkin API to consume |
|---|---|
| Durable player, entity, and block state | `Context::{get,set,remove}_player_data`, `Context::{get,set,remove}_entity_data`, and `Context::{get,set}_block_metadata` |
| Durable item provenance and quality | `ItemStack::{get,set,remove}_custom_data` |
| Safe Timber, Vein Miner, and Earthmover actions | `Context::break_blocks` (maximum 128 unique positions) |
| Repair and Salvage | `AnvilPrepareEvent`, `AnvilRepairEvent`, `GrindstoneEvent`, and `GrindstoneTakeEvent` |
| Enchanting | `EnchantItemGenerateEvent` and `EnchantItemEvent` |
| Food/potion perks | `PlayerItemUseFinishEvent` |
| Fishing perks | `PlayerFishEvent` with mutable catch and XP |
| Taming persistence | `EntityTameEvent` plus entity data |
| Melee perks | `PlayerAttackValidateEvent` and `PlayerAttackDamageEvent` |
| Existing supporting transactions | `CraftItemEvent`, `BrewEvent`, furnace events, breeding, block break/place/drop, and projectile launch/hit events |

Before beginning, compile Cabbage against this same Pumpkin checkout and
confirm the native plugin API versions match. Do not retain a compatibility
layer for older Pumpkin versions: compile failure is preferable to a silent
loss of transactional guarantees.

## Architectural decisions

1. **SQLite remains authoritative for skill XP.** Keep the existing dedicated
   worker thread and `player_skills` table. Do not move hot XP writes into
   Pumpkin player data.
2. **Pumpkin persistent data holds state that must travel with game objects.**
   Store item quality, Masterwork/provenance, crop fertilizer/quality seeds,
   pet traits, and compact player capability state in the appropriate
   Pumpkin-owned player/item/entity/block store.
3. **All Cabbage keys use the `cabbage` namespace.** Keep keys versioned and
   compact, for example `profile_v1`, `item_v1`, `crop_v1`, and `pet_v1`.
4. **The game thread only reads live Pumpkin state and commits transactions.**
   It may schedule SQLite requests, but no blocking I/O or live entity/world
   handles may enter worker threads.
5. **Every perk is bounded and configurable.** Limits include maximum damage
   modifiers, cooldowns, proc chances, area sizes, batch blocks, and XP
   attribution. Never replace validation by directly editing inventory/world
   state outside the supplied transaction/event path.
6. **Commands and vanilla interactions are the initial activation paths.** A
   client-side keybind is out of scope. GUI work starts only after the native
   plugin GUI lifecycle is confirmed sufficient for protected menus.

## Target skill model

Replace the two generic skills with the three-branch model below. Each skill
is independently levelled to 100; branch mastery is the average of its member
skills. Level 25/50/75 unlocks a major perk and level 100 a capstone. The
exact XP curve and every passive cap must be configured rather than hard-coded.

| Branch | Skills |
|---|---|
| Frontier | Agriculture, Herbalism, Woodcutting, Mining, Excavation, Fishing, Husbandry, Taming |
| Warfare | Blades, Axes, Archery, Unarmed, Defense, Acrobatics, Sorcery |
| Enterprise | Smithing, Repair, Salvage, Alchemy, Enchanting, Tinkering, Trading, Charisma |

`Combat` is retired rather than extended. Provide a one-time migration from
legacy Combat XP to an explicitly configured destination (default: no
automatic split; preserve it in a legacy record until an administrator chooses
a migration). Mining XP remains Mining XP. The migration must be idempotent,
record its schema version in SQLite, and be covered by tests.

## Phase 0 — MMO foundation and migration

### 0.1 Restructure the module

Split the growing `src/mmo/` module by responsibility while preserving its
existing database worker and bossbar behavior:

```text
src/mmo/
|-- mod.rs              # state construction, lifecycle, event registration
|-- skills.rs           # SkillId, BranchId, metadata, legacy migration mapping
|-- progression.rs      # central award_xp flow, levels, branch mastery
|-- perks/              # eligibility, cooldowns, effects, activation guards
|-- persistence/        # typed Pumpkin player/item/entity/block-data codecs
|-- frontier/           # skill handlers and configuration
|-- warfare/            # skill handlers and configuration
|-- enterprise/         # skill handlers and configuration
|-- ui/                 # bossbars now; menus only when supported
|-- db.rs               # retain SQLite worker and migrations
`-- ore_reveal/         # retain as the Mining-specific feature
```

Move code incrementally; do not combine Mob AI with MMO modules.

### 0.2 Define the data contracts

- Expand `SkillId`, add `BranchId`, `SkillDefinition`, and deterministic
  display/order metadata.
- Evolve `MmoConfig` to include each skill curve, XP rewards, passive caps,
  cooldowns, ability costs, batch limits, feature toggles, and a config schema
  version. Existing RON files must deserialize with safe defaults.
- Create typed codecs around `NbtTag`; handlers must not scatter raw NBT key
  strings. Reject malformed/version-incompatible Cabbage payloads safely.
- Define item tags for quality tier, creator UUID, provenance, Masterwork,
  runes, infusions, and a data version. Preserve unrelated item custom data.
- Define block tags for crop fertilizer tier and deterministic quality seed.
- Define entity tags for pet traits, bond state, and recovery state.
- Decide and document a single XP attribution rule per action before its
  handler is written (for example, one primary skill per block break or kill,
  never an accidental multi-skill award).

### 0.3 Centralize progression

- Replace per-event duplicated DB/bossbar/level-up logic with one
  `award_xp(player, skill, amount, source)` path.
- Enforce disabled-skill checks, max-level behavior, anti-duplicate guards,
  configuration bounds, logging, bossbar refresh, and level-up presentation in
  that path.
- Add an XP-source enum suitable for audit logging and future rate limiting.
- Add branch-mastery calculation, cached only if profiling justifies it.
- Extend `/mmo stats`, `/mmo top`, `/mmo setxp`, and reload validation for all
  skills and branches; add an admin-only migration/status command.

### 0.4 Foundation acceptance criteria

- Fresh and existing databases migrate without data loss or duplicate runs.
- An existing config retains its semantics after upgrade.
- Item/player/entity/block codec round trips preserve Cabbage data and reject
  corrupt versions without crashing an event handler.
- Every new skill can earn XP, level, show a bossbar, and display correctly in
  commands before a perk is enabled.
- `cargo fmt`, `cargo check`, and focused unit tests pass.

## Phase 1 — Frontier vertical slice

Implement each skill only after its XP rule, state ownership, configuration,
and perk limits are defined. Begin with a small shippable slice (Mining,
Woodcutting, Fishing, and Agriculture) before turning on the whole branch.

| Skill | First Cabbage implementation | Pumpkin transaction/data path |
|---|---|---|
| Mining | Migrate current XP and ore reveal; add Prospector and a bounded Vein Miner. | Block break/drop; `Context::break_blocks`; item data for tool perks. |
| Woodcutting | XP for natural logs, Heartwood roll, and bounded Timber. Natural-tree detection belongs in Cabbage. | Block break/drop; `Context::break_blocks`; item data. |
| Agriculture | Harvest XP, fertilizer/quality provenance, and a conservative harvest bonus. | Block break/drop; `Context` block metadata. |
| Herbalism | Plant/forage XP, quality yield, and consumable-healing bonuses. | Block break/drop; `PlayerItemUseFinishEvent`. |
| Excavation | Diggable-block XP, archaeology-style loot rules, and bounded Earthmover. | Block break/drop; `Context::break_blocks`. |
| Fishing | Catch XP, configurable treasure replacement, and reel/treasure perks. | `PlayerFishEvent`. |
| Husbandry | Feeding/breeding/product XP and configured trait rolls. | Entity-interaction/breeding events; entity data. |
| Taming | Record Cabbage pet profile on tame; implement only owner-validated commands and bonuses. | `EntityTameEvent`; entity data. |

Frontier rules:

- Use `Context::break_blocks` for all multi-break perks; deduplicate candidates,
  use the configured cap (never exceed Pumpkin's 128), and charge the
  documented tool/durability/cooldown cost once per completed action.
- Cabbage must validate natural-log/ore/crop eligibility itself; do not make
  player-placed blocks a source of farmable progression.
- Start crop quality at harvest time from stored data. Add accelerated growth
  only if a stable growth-transition API is available; do not simulate random
  ticks in the plugin.
- Make pet commands commands or existing interactions until a protected menu is
  proven; never treat entity data as proof of ownership without checking
  Pumpkin's actual tameable owner state.

## Phase 2 — Warfare vertical slice

Start with melee classification and XP attribution, then add bounded passive
effects. Sorcery remains entirely Cabbage-owned above Pumpkin's ordinary
item/projectile/damage primitives.

| Skill | First Cabbage implementation | Pumpkin transaction/data path |
|---|---|---|
| Blades | Sword XP, bounded damage/sweep perks, and cooldown-gated Riposte. | `PlayerAttackDamageEvent`. |
| Axes | Axe XP and bounded armor/shield-oriented effects where state is exposed. | `PlayerAttackDamageEvent`. |
| Archery | Bow XP, projectile provenance, and hit-based rewards. | Projectile launch/hit; item data. |
| Unarmed | Empty-hand classification and bounded knockback/damage perks. | `PlayerAttackDamageEvent`. |
| Defense | Damage-taken XP and temporary, bounded defensive effects. | Damage and item-use/blocking paths available in Pumpkin. |
| Acrobatics | Fall/movement XP and safe-landing effects through existing damage hooks. | Damage/movement events. |
| Sorcery | Mana/cooldown state, staff/item activation, Cabbage projectile/effect rules. | Item use, projectiles, damage, attributes only when available. |

Warfare rules:

- Register mutable attack handlers as blocking handlers. The final damage must
  remain within configured caps and cancellation must be deliberate.
- Classify the held item from the event snapshot; do not infer weapon class
  later from a potentially changed inventory.
- Award kill XP once from an authoritative killer/projectile-owner path. Until
  Pumpkin provides a stronger kill-attribution transaction, retain and harden
  the current `EntityDeathEvent` lookup rather than double-awarding on hit and
  death.
- Do not implement extended reach, projectile deflection, or permanent
  attributes by workarounds. Keep those perks disabled pending the platform
  capabilities listed below.

## Phase 3 — Enterprise vertical slice

Ship Vanilla-result modifiers first. Quality, Masterwork, runes, economy, and
complex custom screens are follow-on content, not prerequisites for basic
progression.

| Skill | First Cabbage implementation | Pumpkin transaction/data path |
|---|---|---|
| Smithing | Craft/smelt XP and durable creator/provenance markers. | Craft and furnace events; item custom data. |
| Repair | Anvil prepare cost discount; XP only when output is taken. | `AnvilPrepareEvent`, `AnvilRepairEvent`. |
| Salvage | Grindstone output/XP modifier and material-recovery rolls. | `GrindstoneEvent`, `GrindstoneTakeEvent`. |
| Alchemy | Brewing XP and consumable potency/duration rules that remain bounded. | `BrewEvent`, `PlayerItemUseFinishEvent`. |
| Enchanting | Offer adjustment and commit-time cost/rune rules. | `EnchantItemGenerateEvent`, `EnchantItemEvent`; item data. |
| Tinkering | Custom-item provenance and small, contained receiver behavior. | Item/block data, crafting, and protected UI when available. |
| Trading | Configuration and reputation ledger only; no prices are modified until a trade commit transaction exists. | Pending villager-trade API. |
| Charisma | Cabbage reputation/NPC/party effects; no general economy system in Pumpkin. | Cabbage data plus only existing validated effects. |

Enterprise rules:

- Apply discount/output preview logic in prepare events, then award XP and
  consume a Cabbage cooldown only in the corresponding take/commit event.
- Preserve the vanilla-computed item as the default. A perk may replace or
  enrich it only after validating its Cabbage item metadata.
- Never grant items directly as a substitute for a cancelled transaction.
- Keep item tags concise and versioned so they survive inventories, drops,
  crafting, containers, and restart serialization.

## Phase 4 — presentation, capstones, and live operations

- **GUI evaluation outcome (this checkout):** `PluginGui`/
  `PluginScreenHandler` exist only as WASM-facing API. There is no native
  open path and no click/close attribution to a native plugin, so protected
  menus are **not** built. Presentation stays on commands, bossbars, and
  action-bar prompts until the native GUI lifecycle is sufficient.
- Add major perks at 25/50/75 only after the skill's basic XP flow has been
  live-tested. Add level-100 capstones last, one at a time, with telemetry
  and a configuration kill switch. Level gates live in
  `perks::eligibility`; no major perks or capstones are enabled yet.
- Audit logging for XP grants, batch actions, item-quality rolls, and
  committed anvil/grindstone/enchant operations lives in `audit.rs`
  (`mmo-audit.log`, `audit` config section); player-facing logging stays
  configurable.
- The default balance profile and migration guide for server owners is
  published in `src/mmo/BALANCE.md`.

## Platform gaps — do not work around in Cabbage

These features remain blocked until Pumpkin exposes a validated primitive.
Track them in this plan, but do not edit Pumpkin from this project or emulate
them through unsafe inventory/world changes.

| Blocked Cabbage feature | Required Pumpkin capability |
|---|---|
| Permanent/modifier-based attribute perks | Stable attribute-modifier API with safe removal and player/entity scope. |
| Projectile deflection and reliable owner transfer | Projectile deflect/ownership mutation transaction. |
| Reach-changing melee perks | Target-validation support that permits the intended bounded reach change and is documented for Java/Bedrock. |
| Crop-growth acceleration beyond existing grow events | Targeted cancellable growth-transition or bounded growth-request API. |
| Player crop-harvest attribution | Dedicated harvest transaction with crop state/tool/drops, if `BlockBreakEvent` cannot meet the XP rule. |
| Exact projectile kill attribution | Player-kill event carrying the killer, weapon, damage source, and projectile owner. |
| Trading perks that change exchanges | Cancellable prepare/commit villager trade transaction. |
| Smithing-table-specific perks | Mutable smithing prepare/commit transaction. |
| Full custom menus | Native plugin GUI handles and lifecycle/click events that protect menus from all inventory transfer actions. |

## Implementation sequence and checkpoints

1. **Foundation:** complete Phase 0 and verify the 22-skill migration on a copy
   of a live database/config.
2. **Frontier MVP:** Mining migration, Woodcutting, Fishing, and Agriculture;
   test a restart, block provenance, and batch-break limits.
3. **Frontier completion:** Herbalism, Excavation, Husbandry, and Taming;
   enable major perks one skill at a time.
4. **Warfare MVP:** Blades, Axes, Archery, and kill attribution hardening.
5. **Warfare completion:** Unarmed, Defense, Acrobatics, and first Sorcery
   activation path.
6. **Enterprise MVP:** Repair, Salvage, Enchanting, Smithing, and Alchemy.
7. **Enterprise completion:** Tinkering, then Trading/Charisma only to the
   extent their required transactions are present.
8. **Presentation/capstones:** Phase 4 after functional skills are stable.

At every checkpoint run `cargo fmt`, `cargo check`, focused unit tests for
pure progression/config/perk math, and an in-server smoke test using the
matching Pumpkin build. For changes under `src/mmo/`, follow
`src/mmo/README.md`; preserve the existing dedicated SQLite worker and keep
Pumpkin live handles off background threads.

## Definition of done

The MMO feature is complete only when the enabled skills have a documented
XP source, configuration, bounded perk behavior, persistence ownership,
commands/UI presentation, unit coverage for pure logic, and an in-server
transaction smoke test. A skill whose required Pumpkin primitive is missing
remains disabled and documented as blocked rather than approximated.
