# MMO Six-Skill Branch Consolidation and Compact UI Plan

## Objective

Reduce the MMO skill list from 23 skills to 18 skills, arranged as exactly six
skills in each of the three branches. The default `/mmo` display remains a
compact three-column chat grid, while the protected inventory GUI remains
available under `/mmo menu`.

This is not only a display change. Five pairs of skills will become five real,
shared progression tracks. Existing player XP and configuration must be
migrated without loss, every XP-producing handler must award the new skill,
and all menus, commands, bossbars, documentation, and tests must use the same
canonical 6/6/6 model.

## Final skill model

The canonical order below should be used everywhere: `SkillId::ALL`, branch
arrays, the chat grid, the inventory menu, command suggestions, and tests.

| Row | Frontier | Replaces | Warfare | Replaces | Enterprise | Replaces |
|---:|---|---|---|---|---|---|
| 1 | Cultivation | Agriculture + Herbalism | Blades | unchanged | Smithing | unchanged |
| 2 | Woodcutting | unchanged | Axes | unchanged | Maintenance | Repair + Salvage |
| 3 | Mining | unchanged | Archery | unchanged | Alchemy | unchanged |
| 4 | Excavation | unchanged | Athletics | Unarmed + Acrobatics | Enchanting | unchanged |
| 5 | Fishing | unchanged | Defense | unchanged | Tinkering | unchanged |
| 6 | Animal Handling | Husbandry + Taming | Sorcery | unchanged | Commerce | Trading + Charisma |

The new canonical `SkillId` variants should be:

- `Cultivation`
- `Woodcutting`
- `Mining`
- `Excavation`
- `Fishing`
- `AnimalHandling`
- `Blades`
- `Axes`
- `Archery`
- `Athletics`
- `Defense`
- `Sorcery`
- `Smithing`
- `Maintenance`
- `Alchemy`
- `Enchanting`
- `Tinkering`
- `Commerce`

The ten retired variants must no longer appear in new database rows or saved
configuration: `Agriculture`, `Herbalism`, `Husbandry`, `Taming`, `Unarmed`,
`Acrobatics`, `Repair`, `Salvage`, `Trading`, and `Charisma`.

## User-visible behavior

### Default `/mmo` chat summary

The default player command should produce eight explicit chat lines: one title,
one branch-header row, and six skill rows. It should not open the inventory GUI.

```text
CABBAGE MMO · GUI /mmo menu
FRONTIER     | WARFARE      | ENTERPRISE
Cultiv     1 | Blades     1 | Smithing   1
Woodcut   12 | Axes       8 | Maintain   3
Mining    41 | Archery   14 | Alchemy   19
Excavat   10 | Athletic   2 | Enchant   11
Fishing   18 | Defense   31 | Tinker     7
Animals    9 | Sorcery    5 | Commerce   1
```

The example values are illustrative. The implementation must generate the
table from a snapshot and the canonical branch arrays.

#### Cell contract

- Use a maximum of eight visible characters for each skill label.
- Do not display `L` or another level prefix.
- Format each cell as an eight-character, left-aligned label, one space, and a
  three-character, right-aligned level field.
- Each cell is therefore exactly 12 uniform-font characters.
- Separate cells with ` | `, yielding 42 uniform-font characters for a full
  row. At the normal six-pixel glyph width this is about 252 pixels, safely
  below the vanilla default 320-pixel chat width.
- Use `minecraft:uniform` for every padded component so spaces and alignment
  are deterministic with the standard client resources.
- Render levels over 999 with a bounded marker such as `1k+`; put the exact
  level in hover text. This is presentation only and must not alter the stored
  level.

Use these visible labels:

| Frontier | Warfare | Enterprise |
|---|---|---|
| `Cultiv` | `Blades` | `Smithing` |
| `Woodcut` | `Axes` | `Maintain` |
| `Mining` | `Archery` | `Alchemy` |
| `Excavat` | `Athletic` | `Enchant` |
| `Fishing` | `Defense` | `Tinker` |
| `Animals` | `Sorcery` | `Commerce` |

The full skill names, exact level, current and required XP, progress percentage,
total XP, and disabled/max-level state remain in the cell hover text. Branch
headers retain branch mastery and enabled-skill-count hover text. Keep the
current green, red, and gold branch colors, dark-gray separators, disabled
strikethrough styling, and bold max-level styling.

The table cannot be guaranteed not to wrap on clients that choose an unusually
narrow chat width or replace the uniform font. The target is a clean single-row
fit at default Java client settings, with every row still understandable if it
does wrap.

### Inventory GUI

Keep the completed protected inventory GUI under `/mmo menu`. It should remain
a 9x3 read-only menu with skill icons, branch headers, hover lore, target/viewer
handling, Help action, and transfer protection.

Reducing the skills from 23 to 18 means the slot model can no longer assume
that every non-header slot is a skill or Help slot. Add an explicit empty or
filler slot variant and use this layout:

| Inventory row | Header | Skill slots | Other slots |
|---|---:|---|---|
| Frontier | 0 | 1-6 | 7-8 filler |
| Warfare | 9 | 10-15 | 16 filler, 17 Help |
| Enterprise | 18 | 19-24 | 25-26 filler |

Suggested representative icons for the merged skills are:

- Cultivation: wheat or a golden hoe.
- Animal Handling: lead or bone.
- Athletics: feather or leather boots.
- Maintenance: grindstone, anvil, or iron ingot.
- Commerce: emerald.

The exact icon choice is cosmetic, but each merged skill must have distinct
material, name, branch color, current level, XP progress, and explanatory lore.

### Commands

- `/mmo`: show the sender's compact chat grid.
- `/mmo menu [player]`: open the retained inventory GUI.
- `/mmo stats`: show the sender's compact chat grid.
- `/mmo stats <player>`: show an online player's compact grid to a player
  sender; preserve readable full text for console/RCON if that is the current
  behavior.
- `/mmo stats chat`: remain a compatibility alias for the summary.
- `/mmo stats chat <branch>`: retain the detailed branch view.
- `/mmo top`, `/mmo setxp`, and other skill arguments: suggest and display only
  the 18 canonical names.

For one compatibility period, `SkillId::from_name` and configuration parsing
should accept retired names and route them to their merged destination. This
lets existing admin scripts such as `/mmo top repair` continue to work, but
help and completion output must teach only the new names.

## Progression semantics

Each merge creates one shared XP pool and one shared level. Activity-specific
settings and XP sources should remain distinct where they already exist; only
the skill receiving that XP changes.

| New skill | Activities sharing the level | Required handler changes |
|---|---|---|
| Cultivation | crop/agriculture and herbalism actions | `frontier/agriculture.rs` and `frontier/herbalism.rs` award `Cultivation` |
| Animal Handling | husbandry and taming actions | `frontier/husbandry.rs` and `frontier/taming.rs` award `AnimalHandling` |
| Athletics | empty-hand combat plus acrobatics/fall actions | empty-hand classification in `warfare/mod.rs`, special handling in `warfare/melee.rs`, and acrobatics logic in `warfare/defense.rs` use `Athletics` |
| Maintenance | repair and salvage actions | `enterprise/repair.rs` and `enterprise/salvage.rs` award/use `Maintenance` |
| Commerce | trading and charisma/reputation activities | future/current commerce hooks use `Commerce` |

Do not merge the nested activity configuration structures solely because the
level is shared. For example, `frontier.agriculture` and
`frontier.herbalism` may keep separate XP values, block/action rules, and
enable switches while both award Cultivation. The same applies to husbandry
versus taming, unarmed versus acrobatics, repair versus salvage, and trading
versus charisma.

Keep `XpSource::Repair` and `XpSource::Salvage` distinct. They describe why XP
was awarded for auditing and balancing; they do not need to match `SkillId`.
Apply the same principle to any other activity-specific audit source.

The combined skills will naturally receive XP from more activities. During
implementation, review the XP rates and perks in `mmo/src/BALANCE.md` and make
an explicit choice for every pair:

- accept faster progression because the skill covers a broader discipline; or
- reduce activity XP rates to preserve roughly the previous time-to-level.

Do not silently change perk strength. Cultivation should drive both crop and
herbal perks, Animal Handling both breeding and taming perks, Athletics both
unarmed and acrobatics perks, and Maintenance both repair and salvage perks.
Commerce may initially have no live XP source because trading/charisma master
switches are currently disabled; document that rather than inventing an XP
event.

## Durable data migration

### SQLite schema version 2

Raise `CURRENT_SCHEMA_VERSION` in `mmo/src/db.rs` to 2 and add a worker-thread
migration that consolidates `player_skills` rows in one transaction.

For each player and each pair:

1. Read XP for the two retired skill keys and any already-existing destination
   key.
2. Sum all three cumulative XP values.
3. Insert or update the canonical destination row with the sum.
4. Delete the two retired rows.
5. Leave every unrelated skill row unchanged.

The mapping is:

| Retired rows | Destination row |
|---|---|
| `Agriculture` + `Herbalism` | `Cultivation` |
| `Husbandry` + `Taming` | `AnimalHandling` |
| `Unarmed` + `Acrobatics` | `Athletics` |
| `Repair` + `Salvage` | `Maintenance` |
| `Trading` + `Charisma` | `Commerce` |

Sum raw cumulative XP rather than choosing the larger level or averaging the
values. This preserves all XP earned in both former activities. Do not cap XP
to a configured level curve during migration; normal level calculation will
interpret the resulting total using the destination skill's curve.

Use checked conversion/addition and a documented saturation or error policy at
SQLite's signed 64-bit limit. The migration must be guarded by schema version,
fully transactional, and idempotent when the database is reopened. Apply the
existing schema version 1 migration before version 2 so both fresh databases
and old installations follow a deterministic upgrade path.

The existing legacy Combat migration remains separate. If its configured
target uses a retired name, parse that target as the corresponding new skill.
Historical metadata strings may continue to report the old name because they
describe a completed past operation.

Before deploying the first build with this migration, back up `mmo.db`. Log a
concise success message with the number of player rows consolidated, and return
a clear load error if the transaction cannot complete.

### Configuration version 2

Raise the MMO configuration version to 2. Configuration migration must not
deserialize old pair names directly into the same `HashMap<SkillId, ...>` and
allow iteration order to choose a winner; hash-map order would make conflicting
settings nondeterministic.

Use an explicit migration pass with these rules:

1. If a canonical destination entry already exists, it wins.
2. Otherwise, use the first retired skill below as the deterministic curve
   anchor.
3. Preserve the anchor's `max_level`, `base_xp`, and `xp_multiplier`.
4. Set `enabled` to the logical OR of the two old skill entries so a partly
   enabled pair remains usable. If product intent changes this rule, change it
   deliberately and document it before implementation.
5. If old curve values disagree, log which anchor won.
6. Remove retired keys and save the canonical version 2 configuration once.
7. Insert defaults for canonical skills with no old or new entry.

| Destination | Deterministic anchor | Secondary entry |
|---|---|---|
| Cultivation | Agriculture | Herbalism |
| Animal Handling | Husbandry | Taming |
| Athletics | Unarmed | Acrobatics |
| Maintenance | Repair | Salvage |
| Commerce | Trading | Charisma |

Continue to accept retired spellings as serde/command aliases where a field
contains one skill value, including `combat_migration.target`. New saves must
always serialize the canonical destination name.

Keep the existing nested activity configs (`agriculture`, `herbalism`,
`husbandry`, `taming`, `unarmed`, `acrobatics`, `repair`, `salvage`, `trading`,
and `charisma`). They configure different event sources and should not be
discarded by the skill-curve migration.

## Code changes by subsystem

### `mmo/src/skills.rs`

- Replace the ten retired enum variants with the five destination variants.
- Change `SkillId::ALL` from 23 to 18 entries.
- Change the branch arrays from 8/7/8 to 6/6/6 in the final row order.
- Add canonical display names and branch mapping for the new variants.
- Extend `from_name` with legacy aliases, but return only canonical variants.
- Update unit tests for counts, ordering, uniqueness, branch membership,
  canonical round-tripping, and legacy aliases.

### Skill handlers and perks

- Change every award, level lookup, enabled check, bossbar update, and perk
  gate that references a retired skill.
- Preserve activity-specific config and audit source selection.
- Search the entire workspace for all ten retired variant names after the edit;
  remaining occurrences should be limited to migration code, compatibility
  aliases, historical documentation, and explicit migration tests.
- Update warfare weapon classification and kill attribution so empty-hand XP
  reaches Athletics through the normal central award path.
- Update progression/branch-mastery tests to average six Warfare skills rather
  than the former seven.

### `mmo/src/ui/chat.rs`

- Change the cell width from the current four-character-code plus `L` format
  to the 12-character contract described above.
- Replace the abbreviation table with the final labels, allowing up to eight
  characters.
- Build exactly six data rows from the three branch arrays; no column should
  require a blank seventh or eighth cell.
- Keep structured text components so colors, hover data, font, title command
  suggestion, and state styling are not lost.
- Update line-count, cell-width, content, overflow-level, hover, disabled, and
  max-level tests.

### `mmo/src/ui/menu.rs`

- Add an explicit filler/empty `MenuSlot` variant.
- Rebuild the 9x3 layout around three headers, 18 skills, one Help slot, and
  five fillers.
- Add icons and lore for each merged skill.
- Keep the existing read-only transfer policy and viewer/target behavior.
- Update layout tests to assert exactly 3 headers, 18 unique skill slots,
  1 Help slot, and 5 fillers.

### `mmo/src/commands.rs`

- Ensure root/stats commands use the compact chat summary and `menu` remains
  the inventory entry point.
- Use the canonical 18-skill list for parsing hints, help, top, set-XP, and
  migration commands.
- Accept retired aliases without showing them as preferred choices.
- Update help copy to describe `/mmo` as chat and `/mmo menu` as GUI.

### Progression, bossbars, persistence, and configuration

- Update `mmo/src/progression.rs` branch-mastery expectations and any retired
  skill matches.
- Bossbars are already keyed by `SkillId`; confirm that handlers now create
  bars for canonical IDs and display the merged names.
- Add SQLite schema v2 and config v2 migrations before normal state is exposed
  to event handlers.
- Update persistence comments that refer to retired skills where they describe
  current behavior. No new cross-plugin service type is expected.
- No changes should be needed in `api/`, `core/`, or `mobai/` unless a final
  workspace search finds an actual dependency on a retired `SkillId`.

### Documentation

Update these documents as part of the implementation:

- `mmo/src/README.md`: 18 skills, 6/6/6 branches, command routing, chat format,
  and merged progression behavior.
- `mmo/src/BALANCE.md`: combined XP sources, chosen rate policy, and shared
  perk-level behavior.
- Root `README.md` and `AGENTS.md` if they state the old 23-skill count.
- Existing GUI plans: add a short superseded note rather than rewriting their
  historical design record.

## Recommended implementation order

1. Add migration-focused tests for old database/config shapes before changing
   the enum.
2. Implement canonical `SkillId` variants, arrays, display names, and aliases.
3. Implement and test SQLite schema v2 and configuration v2 migration.
4. Remap every event handler, perk lookup, and progression calculation.
5. Update the compact chat grid to six rows and the 12-character cell format.
6. Reflow the protected inventory menu and add filler-slot handling.
7. Update commands, help, bossbar naming, and documentation.
8. Run formatting, focused tests, full workspace tests, and live-server checks.

Data migration should land in the same deployable commit as the enum change.
Do not release a build that knows only the new enum but cannot adopt old rows
and configuration.

## Test matrix

### Migration tests

- Both retired rows exist and their XP is summed.
- Only the anchor row exists.
- Only the secondary row exists.
- A destination row already exists and is included in the sum.
- Multiple players migrate independently.
- Zero-XP rows do not produce incorrect totals.
- Unrelated skill rows remain byte-for-byte equivalent in value.
- Reopening an already migrated database makes no further changes.
- Schema version 0 and version 1 databases both reach version 2.
- A failed migration rolls back rather than leaving mixed old/new rows.
- Old config with equal and conflicting pair curves migrates deterministically.
- An explicit new config entry wins over old pair entries.
- Old single-value fields and command arguments resolve through aliases.

### Progression tests

- Each old activity awards exactly its new destination skill.
- Both activities in a pair increase the same cumulative XP value.
- Branch mastery uses six skills per branch.
- Activity-specific enable switches still prevent their own XP awards.
- Shared perk gates read the merged level.
- Audit entries retain the correct activity source.

### Chat UI tests

- The summary contains exactly eight lines.
- Every canonical skill appears exactly once and in the correct row/column.
- Every data cell is exactly 12 uniform-font characters before separators.
- No visible cell contains the former `L` marker.
- Labels are no longer than eight characters.
- Levels 1, 9, 10, 99, 100, 999, and over 999 stay aligned.
- Exact level/XP values remain in hover text when visible values are compacted.
- Disabled and max-level style precedence remains correct.
- The title still contains a readable/clickable `/mmo menu` hint.

### Inventory UI tests

- The menu contains 18 unique canonical skills.
- Header, Help, and five filler slots are in the planned positions.
- Every merged skill has a valid icon and lore.
- All click/drag/transfer paths remain blocked except the intended Help action.
- Viewing another player preserves the viewer/target distinction.

## Verification and rollout

1. Back up a representative production `mmo.db` and `config.ron`.
2. Run `cargo fmt` after Rust edits.
3. Run focused MMO migration, progression, command, and UI tests.
4. Run `cargo check --workspace`.
5. Run `cargo test --workspace`.
6. Start a test server with a copy of the old 23-skill database/config and
   confirm the migration log, XP sums, curve choice, and saved canonical keys.
7. Run `/mmo` at default Java chat settings and confirm all eight lines fit,
   all three columns align, and no `L` markers remain.
8. Test narrower chat widths, Unicode font, and a font-replacing resource pack
   for graceful degradation.
9. Open `/mmo menu`, inspect every merged skill, and attempt all item-transfer
   click modes.
10. Exercise old command aliases, new canonical commands, console output,
    branch details, bossbars, and XP awards from both sides of every merge.

## Acceptance criteria

- There are exactly 18 canonical skills: six per branch.
- The five pairs share real XP/level tracks, not merely combined display cells.
- Existing XP is preserved by a transactional, idempotent database migration.
- Existing skill curves are migrated by a documented deterministic rule.
- New saves contain no retired skill keys.
- `/mmo` renders an eight-line, three-column grid with labels up to eight
  characters and no `L` marker.
- `/mmo menu` retains the protected GUI with all 18 skills and no usable filler
  slots.
- Commands display canonical names while accepting legacy aliases.
- Both activities behind every merged skill award and gate against the shared
  level correctly.
- The full workspace formats, checks, and tests successfully, and a copied
  pre-migration installation upgrades successfully in a live test server.
