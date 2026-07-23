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

## Frontier

| Skill | XP sources (defaults) | Perks (defaults) |
|---|---|---|
| Mining | ore breaks from the `xp_rewards.blocks` table in `mmo.rewards.ron` (coal 8 → ancient debris 150) | Prospector (5% + 0.2%/level, cap 35%, +1 approved ore drop; no bonus skill XP), Vein Miner (sneak+break, ≤16 blocks) |
| Woodcutting | natural logs 6–8 | Heartwood (2%: bonus log + 25 XP), Timber (sneak+break, ≤32 blocks) |
| Cultivation | mature harvests 10–14 (agriculture); plants 2–6, consumables 3–25 (herbalism) | Harvest bonus (10% +1 item), fertilizer (bone meal): deterministic roll, guaranteed bonus + 10 XP; Quality yield (8% +1 item), consumable healing (+1.0 health) |
| Excavation | diggable blocks 4–8 | Archaeology loot (3–8% per table), Earthmover (sneak+break, ≤16 blocks) |
| Fishing | catches 5–60, default 10 | Reel (+2 vanilla XP), treasure replacement (off) |
| AnimalHandling | breeding 15–40, default 15; products 8–10 (husbandry); tames 30–50, default 30; pet feeding 4 (taming) | Newborn trait roll (15%, bounded by global proc cap; configured trait list) |

Player-placed blocks never earn XP (shared provenance denylist). Batch perks
are capped by `perks.batch_break_max_blocks` (16) and Pumpkin's hard 128, and
share `perks.batch_break_cooldown_ticks` (100).

## Warfare

| Skill | XP sources (defaults) | Perks (defaults) |
|---|---|---|
| Blades | kills from `xp_rewards.mobs` in `mmo.rewards.ron` (attributed by weapon snapshot) | Damage +0.4%/level (cap 50%), Riposte (+25% within 60 ticks of taking damage, 200-tick cooldown) |
| Axes | kills | Damage +0.5%/level (cap 60%) |
| Archery | kills; +4 XP per projectile hit | — |
| Athletics | empty-hand kills (unarmed); 3 XP per fall-damage point, cap 60/fall (acrobatics) | Damage +0.3%/level (cap 40%), knockback +0.4%/level (cap 50%); Roll: −0.2%/level fall damage (cap 25%) |
| Defense | 2 XP per damage point taken (cap 40/hit) | Resilience: −0.15%/level incoming damage (cap 15%) |
| Sorcery | 15 XP per cast | Healing bolt: 25 mana, 100-tick cooldown, heals 4.0; mana 100 max, 0.05/tick regen |

All damage perks are clamped by `perks.max_damage_multiplier` (2.0× base) and
proc chances by `perks.max_proc_chance` (35%).

## Enterprise

| Skill | XP sources (defaults) | Perks (defaults) |
|---|---|---|
| Smithing | crafts 8–110, furnace extraction 1–40 (default 2) | Anvil outputs carry creator/provenance item data |
| Maintenance | 20 XP per anvil take (repair); 15 XP per grindstone take (salvage) | Repair discount −0.05 cost/level (cap 10), 100-tick cooldown; salvage +0.2% experience/level (cap 25%), recovery roll 10% |
| Alchemy | potions 10–14, default 8 | — |
| Enchanting | 5 XP per level of cost (cap 100) | −0.02 offer requirement/level (cap 5) |
| Tinkering | mechanism crafts 4–18 | — |
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
