# Cabbage MMO Balance Profile and Migration Guide

This document is the published default balance profile for the three-branch
skill model, plus the migration guide for server owners upgrading from the
two-skill (`Mining`/`Combat`) era. Everything below is a *default*; every
value lives in RON (`mmo.ron`, with XP rewards in `mmo.rewards.ron` and ore
reveal in `mmo.ores.ron`, all under `plugins/Cabbage/`) and can be
tuned per server.

## Skill model

- 18 skills in 3 branches (six per branch). Each levels independently to
  100 by default.
- Branch mastery = average of the branch's six member skill levels.
- Five skills are merged progression tracks that share one XP pool and one
  level with a retired partner skill: Cultivation (Agriculture +
  Herbalism), AnimalHandling (Husbandry + Taming), Athletics (Unarmed +
  Acrobatics), Maintenance (Repair + Salvage), Commerce (Trading +
  Charisma). The activity-specific XP sources below are unchanged; both
  activities in a pair simply award the merged skill, and every shared perk
  gates on the merged level (Cultivation drives crop and herbal perks,
  AnimalHandling breeding and taming perks, Athletics unarmed and
  acrobatics perks, Maintenance repair and salvage perks). Perk strength
  values are unchanged from the pre-merge profiles.
- **Rate decision:** for all five pairs we accept faster progression
  because each merged skill covers a broader discipline — no XP rate
  numbers were changed.
- Commerce has no live XP source yet: the trading and charisma activity
  configs stay disabled (no villager-trade commit transaction or general
  economy hook), so the merged skill currently earns nothing.
- Default curve for every skill: `base_xp: 50`, `xp_multiplier: 1.15`
  (level 2 at 50 XP, requirement grows ×1.15 per level). Older configs keep
  their serialized per-skill values (e.g. `max_level: 99`); config v2
  migrates retired pair curves deterministically (see the migration guide).

## Milestone Tier Scaling (Levels 10, 25, 50, 100)

Perk strength scales dynamically with **perk tiers** `T = perk_tier(level) ∈ [0, 4]` (unlocked at levels 10, 25, 50, 100). All tier multipliers are configurable in RON.

## Frontier

| Skill | XP sources (defaults) | Perks & Tier Scaling (defaults) |
|---|---|---|
| Mining | ore breaks from `mmo.rewards.ron` (coal 8 → debris 150) | Prospector (+1% chance cap/tier; T=4 capstone: bonus drops 2 items), Vein Miner (+4 max blocks/tier: 16→32) |
| Woodcutting | natural logs 6–8 | Heartwood (+1% chance/tier, +5 XP/tier), Timber (+8 max blocks/tier: 32→64) |
| Cultivation | mature harvests 10–14; plants 2–6, plant foods 3–25 | Harvest bonus (+2% chance/tier), Quality yield (+2% chance/tier), Consumable healing (+0.5 HP/tier) |
| Excavation | diggable blocks 4–8 | Earthmover (+4 max blocks/tier: 16→32), Archaeology loot (+1% chance cap/tier) |
| Fishing | catches 5–60, default 10 | Reel (+1 bonus vanilla XP/tier), Treasure replacement (off) |
| AnimalHandling | breeding 15–40; products 8–10; tames 30–50; pet feed 4 | Newborn trait roll (+2% chance/tier; T≥3 unlocks 2nd distinct trait roll) |

Player-placed blocks never earn XP (shared provenance denylist). Batch perks
are capped by `perks.batch_break_max_blocks` and Pumpkin's hard 128, and
share `perks.batch_break_cooldown_ticks` (100).

## Warfare

| Skill | XP sources (defaults) | Perks & Tier Scaling (defaults) |
|---|---|---|
| Blades | kills from `mmo.rewards.ron` (sword snapshot) | Damage +0.4%/level, Riposte (+5% bonus/tier: 25%→45%, cooldown −20t/tier: 200t→120t) |
| Axes | kills | Damage +0.5%/level (+5% cap/tier: 0.60→0.80) |
| Archery | kills; +4 XP per projectile hit | Arrow damage +0.4%/level (+5% cap/tier: x1.50→x1.70) |
| Athletics | empty-hand kills; 3 XP per fall HP (cap 60/fall) | Unarmed damage +0.3%/level, Knockback +0.4%/level (+5% cap/tier: 0.50→0.70); Roll: −0.2%/level fall damage (+5% cap/tier: 0.25→0.45) |
| Defense | 2 XP per damage point taken (cap 40/hit) | Resilience: −0.15%/level incoming damage (+2.5% cap/tier: 0.15→0.25) |
| Sorcery | 15 XP per cast | Healing bolt: 25 mana, heals 4.0 (+1 HP/tier: 4→8), max mana 100 (+10/tier: 100→140), cooldown 100t (−10t/tier: 100t→60t) |

All damage perks are clamped by `perks.max_damage_multiplier` (2.0× base) and
proc chances by `perks.max_proc_chance` (35%).

## Enterprise

| Skill | XP sources (defaults) | Perks & Tier Scaling (defaults) |
|---|---|---|
| Smithing | crafts 8–110, furnace extraction 1–40 (default 2) | Craft & smelt XP multiplier +5%/tier; Anvil outputs carry creator/provenance item data |
| Maintenance | 20 XP per anvil take; 15 XP per grindstone take | Tool Care (5% base + 2.5%/tier durability refund); Repair discount (+1 level cap/tier); Salvage (+5% XP cap/tier, 10% recovery roll) |
| Alchemy | potions 10–14, default 8 | Potion Mastery (+0.5 HP/tier bonus heal on potion consume) |
| Enchanting | 5 XP per level of cost (cap 100) | Offer discount (+1 level cap/tier); Enchant XP cap (+25/tier: 100→200) |
| Tinkering | mechanism crafts 4–18 | Craft XP multiplier +5%/tier |
| Commerce | **disabled** (trading: no trade transaction; charisma: no economy/NPC hook) | reputation ledger only |

## Progression bounds

- `progression.max_xp_per_award`: 10,000 XP clamp per single award.
- Creative/Spectator players never earn XP or trigger perks.
- `perks.enabled: false` disables every perk effect globally without
  touching XP flow. Each skill also has an `enabled` toggle.

## Audit and telemetry

- `mmo-audit.log` in the plugin data folder records every successful XP
  grant, batch breaks, quality rolls, and anvil/grindstone/enchant commits.
- Audit writes are queued to a dedicated writer thread rather than performed
  in an event handler.
- `audit.enabled` (default true), `audit.console` (default false) control it.

## Migration guide (two-skill → three-branch)

1. **Backup** `plugins/Cabbage/mmo.db` and the RON config files
   (`config.ron`, `mmo.ron`, `mmo.rewards.ron`, `mmo.ores.ron`)
   before upgrading. Files from the former `plugins/Cabbage.Mmo/` split
   layout are copied into this unified folder on first load when missing.
2. On first load, Cabbage upgrades automatically and idempotently:
   - `player_skills` rows for `Combat` move to `legacy_combat_xp`
     (SQLite schema v1, recorded in the `meta` table).
   - `Mining` rows stay untouched.
   - The MMO config gains the new sections with safe defaults; your
     serialized skill curves are preserved; the unknown `Combat` entry is
     dropped with a log warning. (A legacy unified `config.ron` is first
     copied out to `mmo.ron`/`mmo.rewards.ron`/`mmo.ores.ron`; the
     old file is left untouched.)
3. **Choose a Combat destination** (or don't — the XP stays preserved
   indefinitely):
   - Set `combat_migration.target: Some(Blades)` (or another Warfare skill)
     in `mmo.ron` to migrate automatically on next load, or
   - run `/mmo migrate combat <skill>` in-game as an admin.
   - `/mmo migrate status` shows preserved rows and the chosen destination.
   The migration is one-time and idempotent; re-running it is a no-op.
4. Review the new `frontier`/`warfare`/`enterprise`/`perks` sections in
   `mmo.ron` and tune to taste; `/mmo reload` applies changes.

## Migration guide (23 skills → 18 skills)

1. **Backup** `plugins/Cabbage/mmo.db` and the RON config files
   (`config.ron`, `mmo.ron`, `mmo.rewards.ron`, `mmo.ores.ron`)
   before upgrading.
2. On first load, Cabbage upgrades automatically and idempotently:
   - SQLite schema v2 consolidates each retired pair's `player_skills` rows
     in one transaction: the canonical destination row receives the sum of
     both retired rows plus any pre-existing destination row (saturating at
     the signed 64-bit limit), then the retired rows are deleted. Unrelated
     rows are untouched, a failure rolls back and fails the load, and
     reopening a migrated database is a no-op.
   - Config v2 merges retired `skills` map entries deterministically: an
     explicit canonical entry wins; otherwise the anchor (Agriculture,
     Husbandry, Unarmed, Repair, Trading) supplies
     `max_level`/`base_xp`/`xp_multiplier`, `enabled` is the logical OR of
     the pair, and curve conflicts are logged with the anchor's win. New
     saves contain only canonical keys.
   - Nested activity configs (agriculture, herbalism, husbandry, taming,
     unarmed, acrobatics, repair, salvage, trading, charisma) and all perk
     knobs are untouched.
3. Old admin scripts keep working for one compatibility period: retired
   skill names still parse as aliases in `/mmo top`, `/mmo setxp`,
   `/mmo migrate combat`, and `combat_migration.target`, but help and
   completion output only teach the 18 canonical names.

## Major perks, capstones, and menus

Per the phased plan, major perks (25/50/75) and capstones (100) are enabled
one at a time only after a skill's basic XP flow has been live-tested; none
are on by default. `/mmo` is the default chat-area summary: an eight-line,
three-column level grid for players whose skill cells suggest the matching
`/mmo skill <skill>` command. `/mmo menu` uses Pumpkin's native
protected GUI lifecycle to show all 18 skill levels and progress in a
read-only 9×3 inventory; clicking a skill icon closes the menu and sends
that skill's detail page. `/mmo stats [player]` prints the same tabular chat
summary for players (full text for console), and `/mmo stats chat <branch>`
keeps the detailed per-branch text view. `/mmo skill <skill> [page]` shows
one skill's progress and unlock timeline in chat: live, level-scaled effects
are listed as `Active from level 1` with their live configured value at the
viewer's level, while the 25/50/75/100 milestone slots stay visibly
`Planned` until their concrete perk ships — the page never presents a
placeholder as an active unlock. Disabled module/skill/perk switches are
labelled `Disabled` rather than hidden. Pages beyond the first
(`/mmo skill <skill> 2`) only exist when a skill's rows overflow the
10-line chat budget (by default Cultivation, Athletics, and Maintenance).
More interactive perk/capstone menus remain follow-on work after their
gameplay is live-tested.
