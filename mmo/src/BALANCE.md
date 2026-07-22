# Cabbage MMO Balance Profile and Migration Guide

This document is the published default balance profile for the three-branch
skill model, plus the migration guide for server owners upgrading from the
two-skill (`Mining`/`Combat`) era. Everything below is a *default*; every
value lives in `config.ron` and can be tuned per server.

## Skill model

- 23 skills in 3 branches. Each levels independently to 100 by default.
- Branch mastery = average of the branch's member skill levels.
- Default curve for every skill: `base_xp: 50`, `xp_multiplier: 1.15`
  (level 2 at 50 XP, requirement grows ×1.15 per level). Older configs keep
  their serialized per-skill values (e.g. `max_level: 99`).

## Frontier

| Skill | XP sources (defaults) | Perks (defaults) |
|---|---|---|
| Mining | ore breaks from the `xp_rewards.blocks` table (coal 8 → ancient debris 150) | Prospector (5% + 0.2%/level, cap 35%, +1 approved ore drop; no bonus skill XP), Vein Miner (sneak+break, ≤16 blocks) |
| Woodcutting | natural logs 6–8 | Heartwood (2%: bonus log + 25 XP), Timber (sneak+break, ≤32 blocks) |
| Agriculture | mature harvests 10–14 | Harvest bonus (10% +1 item), fertilizer (bone meal): deterministic roll, guaranteed bonus + 10 XP |
| Herbalism | plants 2–6, consumables 3–25 | Quality yield (8% +1 item), consumable healing (+1.0 health) |
| Excavation | diggable blocks 4–8 | Archaeology loot (3–8% per table), Earthmover (sneak+break, ≤16 blocks) |
| Fishing | catches 5–60, default 10 | Reel (+2 vanilla XP), treasure replacement (off) |
| Husbandry | breeding 15–40, default 15; products 8–10 | Newborn trait roll (15%, bounded by global proc cap; configured trait list) |
| Taming | tames 30–50, default 30; pet feeding 4 | — |

Player-placed blocks never earn XP (shared provenance denylist). Batch perks
are capped by `perks.batch_break_max_blocks` (16) and Pumpkin's hard 128, and
share `perks.batch_break_cooldown_ticks` (100).

## Warfare

| Skill | XP sources (defaults) | Perks (defaults) |
|---|---|---|
| Blades | kills from `xp_rewards.mobs` (attributed by weapon snapshot) | Damage +0.4%/level (cap 50%), Riposte (+25% within 60 ticks of taking damage, 200-tick cooldown) |
| Axes | kills | Damage +0.5%/level (cap 60%) |
| Archery | kills; +4 XP per projectile hit | — |
| Unarmed | kills | Damage +0.3%/level (cap 40%), knockback +0.4%/level (cap 50%) |
| Defense | 2 XP per damage point taken (cap 40/hit) | Resilience: −0.15%/level incoming damage (cap 15%) |
| Acrobatics | 3 XP per fall-damage point (cap 60/fall) | Roll: −0.2%/level fall damage (cap 25%) |
| Sorcery | 15 XP per cast | Healing bolt: 25 mana, 100-tick cooldown, heals 4.0; mana 100 max, 0.05/tick regen |

All damage perks are clamped by `perks.max_damage_multiplier` (2.0× base) and
proc chances by `perks.max_proc_chance` (35%).

## Enterprise

| Skill | XP sources (defaults) | Perks (defaults) |
|---|---|---|
| Smithing | crafts 8–110, furnace extraction 1–40 (default 2) | Anvil outputs carry creator/provenance item data |
| Repair | 20 XP per anvil take | −0.05 cost/level (cap 10), 100-tick cooldown |
| Salvage | 15 XP per grindstone take | +0.2% experience/level (cap 25%), recovery roll 10% |
| Alchemy | potions 10–14, default 8 | — |
| Enchanting | 5 XP per level of cost (cap 100) | −0.02 offer requirement/level (cap 5) |
| Tinkering | mechanism crafts 4–18 | — |
| Trading | **disabled** (no trade transaction) | reputation ledger only |
| Charisma | **disabled** (no economy/NPC hook) | reputation ledger only |

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

1. **Backup** `plugins/Cabbage/mmo.db` and `plugins/Cabbage/config.ron`
   before upgrading. Files from the former `plugins/Cabbage.Mmo/` split
   layout are copied into this unified folder on first load when missing.
2. On first load, Cabbage upgrades automatically and idempotently:
   - `player_skills` rows for `Combat` move to `legacy_combat_xp`
     (SQLite schema v1, recorded in the `meta` table).
   - `Mining` rows stay untouched.
   - `config.ron` gains the new sections with safe defaults; your serialized
     skill curves are preserved; the unknown `Combat` entry is dropped with a
     log warning.
3. **Choose a Combat destination** (or don't — the XP stays preserved
   indefinitely):
   - Set `combat_migration.target: Some(Blades)` (or another Warfare skill)
     in `config.ron` to migrate automatically on next load, or
   - run `/mmo migrate combat <skill>` in-game as an admin.
   - `/mmo migrate status` shows preserved rows and the chosen destination.
   The migration is one-time and idempotent; re-running it is a no-op.
4. Review the new `frontier`/`warfare`/`enterprise`/`perks` sections in
   `config.ron` and tune to taste; `/mmo reload` applies changes.

## Major perks, capstones, and menus

Per the phased plan, major perks (25/50/75) and capstones (100) are enabled
one at a time only after a skill's basic XP flow has been live-tested; none
are on by default. `/mmo` is the default chat-area summary: a 10-line,
three-column level grid for players. `/mmo menu` uses Pumpkin's native
protected GUI lifecycle to show all 23 skill levels and progress in a
read-only 9×3 inventory. `/mmo stats [player]` prints the same tabular chat
summary for players (full text for console), and `/mmo stats chat <branch>`
keeps the detailed per-branch text view. More interactive perk/capstone
menus remain follow-on work after their gameplay is live-tested.
