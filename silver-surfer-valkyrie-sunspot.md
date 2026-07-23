# Perk Milestone Tiers (Levels 10/25/50/100)

## Goal

Introduce **perk tiers** at levels **10, 25, 50, 100** for all 18 MMO skills.
Existing perk effects gain discrete tier step-ups (bigger Timber trees, higher
double-drop chances, durability refund, stronger discounts...), a few new
effects are added where Pumpkin events make them implementable, the `/mmo
skill` page shows real milestone rows instead of "Planned" placeholders, and
level-ups that cross a tier announce the unlock in chat.

## Current state (verified)

- All perks are flat config values or `per_level * level` formulas clamped by
  per-skill caps and global `PerkConfig` caps; only Prospector uses a
  `base + per_level` chance. Level reads on the event path are async DB
  round-trips (`state.db().get_skill` + `curve.level_for_xp`).
- `perks/eligibility.rs` has `MAJOR_PERK_LEVELS [25,50,75]` / `CAPSTONE_LEVEL
  100` (consumed only by the skill-detail catalog) and two dead gate fns.
- `ui/skill_detail.rs` mirrors every handler formula in `LiveEffect::render`
  and emits "Planned" milestone rows from those constants; `RowState` has
  only `Active`/`Disabled`/`Planned` (`Unlocked`/`Next`/`Locked` documented
  as arriving with the first real milestone perk — this is that perk).
- `progression.rs award_xp` is the single level-up choke point (chat message
  + `celebrate_level_up`); `XpResult` carries `awarded_xp`/`new_xp`, so the
  old level is derivable.
- Pumpkin has **no item-durability event**, but `ItemStack::repair_item`/
  `set_damage` exist and the held stack is reachable
  (`player.inventory.held_item()`), so a durability-refund perk works by
  rolling after `BlockBreakEvent`. Arrow damage is hookable via
  `EntityDamageByEntityEvent` (projectile attribution). Potion consumption
  heals are hookable via `PlayerItemUseCompleteEvent` (already registered
  for Alchemy XP). No native scheduler; tick counting only.

## Design

### Tier infrastructure

Replace the 25/50/75/100 milestone scheme in `perks/eligibility.rs`:

```rust
pub(crate) const PERK_TIER_LEVELS: [u32; 4] = [10, 25, 50, 100];
pub(crate) fn perk_tier(level: u32) -> u32;        // 0..=4 milestones reached
pub(crate) fn tiers_crossed(old: u32, new: u32) -> impl Iterator<Item = u32>;
```

Remove the now-superseded `MAJOR_PERK_LEVELS`/`CAPSTONE_LEVEL` and the dead
gate fns. Tier scaling everywhere follows the existing pattern:
`tier_bonus = per_tier_knob * perk_tier(level)`, added to the current value
BEFORE the existing caps unless stated otherwise (caps that are raised by
tiers get their own `cap_per_tier` knob). All chance results stay clamped by
`config.perks.max_proc_chance`; all magnitudes stay clamped by the global
caps. Tiers never remove access — level 1 behavior is unchanged.

### Configuration (config v3)

Bump `CURRENT_CONFIG_VERSION` to 3. All new knobs use `#[serde(default =
...)]` so v2 files parse; `upgrade_mmo_config` re-saves `mmo.ron` once,
documenting the new fields (same mechanism as v2; the v2 skill-pair merge
still runs during deserialize). Sanitizers clamp the new chance/fraction
fields. Every per-tier value below is a config knob — the numbers are
defaults, not hardcoded.

### Tier effects per skill (defaults)

`T` = `perk_tier(level)` (0–4). Batch sizes remain bounded by
`perks.batch_break_max_blocks` and Pumpkin's 128 hard limit.

| Skill | Effect | Base → +/tier | At L100 |
|---|---|---|---|
| Cultivation | harvest bonus chance | +0.02 T | 10%→18% |
| | quality yield chance | +0.02 T | 8%→16% |
| | consumable heal (hp) | +0.5 T | 1→3 |
| Woodcutting | **Timber max blocks** | +8 T | 32→64 |
| | Heartwood chance | +0.01 T | 2%→6% |
| | Heartwood XP bonus | +5 T | 25→45 |
| Mining | Prospector chance | +0.01 T | ≤cap |
| | Vein Miner max blocks | +4 T | 16→32 |
| | Capstone (T=4): Prospector bonus drops 2 items | — | new |
| Excavation | Earthmover max blocks | +4 T | 16→32 |
| | archaeology loot chance | +0.01 T | +4% |
| Fishing | Reel exp bonus | +1 T | 2→6 |
| AnimalHandling | trait roll chance | +0.02 T | 15%→23% |
| | second distinct trait roll | unlocks at T≥3 (L50) | new |
| Blades | Riposte multiplier | +0.05 T | 0.25→0.45 |
| | Riposte cooldown | −20 T ticks | 200→120 |
| Axes | damage cap | +0.05 T | 0.6→0.8 |
| Archery | **NEW arrow damage bonus** (per_level 0.004, cap 0.5) + cap | +0.05 T cap | 0.5→0.7 |
| Athletics | knockback cap | +0.05 T | 0.5→0.7 |
| | roll (fall) reduction cap | +0.05 T | 0.25→0.45 |
| Defense | resilience cap | +0.025 T | 0.15→0.25 |
| Sorcery | spell heal (hp) | +1.0 T | 4→8 |
| | mana max | +10 T | 100→140 |
| | cast cooldown | −10 T ticks | 100→60 |
| Smithing | craft/smelt XP multiplier | +5% T | +20% |
| Maintenance | repair discount cap | +1.0 T | 10→14 |
| | salvage XP bonus cap | +0.05 T | 0.25→0.45 |
| | **NEW Tool Care**: refund 1 durability on the held tool after a block break | chance 0.05 + 0.025 T | 15% |
| Alchemy | **NEW Potion Mastery**: heal on potion consume (hp) | 0.5 + 0.5 T | 2.5 |
| Enchanting | offer discount cap | +1.0 T | 5→9 |
| | enchant XP cap | +25 T | 100→200 |
| Tinkering | craft XP multiplier | +5% T | +20% |
| Commerce | none (dormant; trading/charisma stay disabled) | — | milestones stay informational |

New-effect implementation notes:

- **Tool Care** (Maintenance): in the existing `BlockBreakEvent` handler
  path, after the frontier handlers, roll the chance; on success
  `held_item.lock().repair_item(1)` when the held stack `is_damageable()`
  and has damage. Skips while the tool has Unbreaking-untouched... no —
  `repair_item` is unconditional; fine (perk semantics). Award nothing else;
  no XP. Honor `perks.enabled`, skill enabled, and `max_proc_chance`.
- **Archery damage**: register `EntityDamageByEntityEvent`; when attribution
  shows the shooter is a player with a bow/crossbow projectile, scale
  `event.final_damage` by `(per_level * level).min(cap + cap_per_tier*T)`,
  still under `perks.max_damage_multiplier` — mirrors melee.rs structure.
- **Alchemy heal**: extend the existing `PlayerItemUseCompleteEvent`
  handler: after awarding potion XP, `living.heal(potion_heal(level))`.
- **Double trait** (AnimalHandling): in `husbandry.rs`, when T≥3 roll a
  second *distinct* trait from the configured list.

### Level-up announcements

In `progression.rs` after the existing level-up block: compute
`old_level` from `new_xp - awarded_xp` via the curve, then for every tier in
`tiers_crossed(old_level, new_level)` send a chat line such as
`⭐ {Skill} perk tier unlocked (level {T}) — see /mmo skill {skill}`
(branch-coloured, `SuggestCommand` click for the detail page). Single choke
point; no handler changes needed.

### Skill detail page sync (`ui/skill_detail.rs`)

- Milestone rows become real: per skill, one row per tier level naming the
  concrete upgrade (data table next to the catalog, e.g. "Timber: max felled
  logs 32→40"). States: `Unlocked` (level ≥ tier), `Next` (lowest tier not
  yet reached), `Locked` (the rest). Commerce keeps informational rows
  (documented dormant). Live-effect rows keep `Active from level 1` but
  `LiveEffect::render` must include tier contributions in the shown values
  so the page stays truthful (catalog coverage test enforces sync).
- `RowState` gains `Unlocked`/`Next`/`Locked` (as its doc comment already
  anticipates); pagination budget rules unchanged.

## Implementation order

1. **Infra**: `perks/eligibility.rs` tiers + tests; config v3 + all new knobs
   + sanitizers + defaults tests; `upgrade_mmo_config` re-save path.
2. **Frontier**: tier scaling in mining/woodcutting/excavation/fishing/
   agriculture/herbalism/husbandry handlers (+ Prospector capstone, double
   trait); extract pure formula helpers where the math is inline so it is
   unit-testable (pattern: `prospector_chance` already exists).
3. **Warfare + Enterprise**: tier scaling; new Archery damage handler +
   event registration; Maintenance Tool Care; Alchemy heal.
4. **Announcements** in `progression.rs` (+ crossed-tier tests).
5. **Catalog/page**: tier rows, new `RowState`s, `LiveEffect::render` tier
   values, new effects added to `LiveEffect`/catalog.
6. **Docs**: `mmo/src/README.md` perk table + `mmo/src/BALANCE.md` tier
   defaults; `AGENTS.md` only if layout facts change.
7. `cargo fmt`, `cargo check --workspace`, `cargo test --workspace`; commit.

## Tests

- `perk_tier` boundary checks: tier 0 for any level below 10, tier 1 from
  10 up to 24, tier 2 from 25 up to 49, tier 3 from 50 up to 99, tier 4 at
  100; `tiers_crossed` handles multi-level jumps. (No tiers exist at 9 or
  99 — those only appear as edge inputs proving the thresholds.)
- Every tier-scaled formula helper: tier 0 equals pre-change value; tier 4
  equals documented cap; caps and global clamps still win.
- Config v2 files load: new knobs get defaults, save bumps to v3; sanitizers
  clamp bad values.
- Tool Care: roll math, skips non-damageable/undamaged stacks (pure helper
  level; live stack mutation needs a server — note it).
- Archery damage formula mirrors melee pattern; attribution gate tests at
  the classification-helper level.
- Catalog: every skill has four tier rows (Commerce informational); states
  at level below/at/above each milestone; render includes tier values;
  no missing/duplicate rows across pages.
- Announcement: crossed tiers produce exactly one message each, none when no
  tier is crossed.

## Explicit non-goals

- No changes to XP curves/level pacing (only perk effects).
- No villager-trade/Commerce effects (still no Pumpkin hook — documented).
- No new persistence: everything derives from level + config.
- No shield/armor-state Warfare effects (Pumpkin limitation, unchanged).
