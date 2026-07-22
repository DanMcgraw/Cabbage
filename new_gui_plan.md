# Default MMO Chat Grid Plan

> **Superseded (historical record).** The 23-skill, 10-line grid this plan
> produced was replaced by the 18-skill, six-per-branch consolidation in
> `skill_consolidation_plan.md`: the default `/mmo` summary is now exactly
> eight lines of 12-character uniform cells with up-to-eight-character
> labels and no `L` marker. Keep this document as the design record of the
> original chat grid; do not treat its line counts, cell format, or skill
> abbreviations as current.

## Objective

Make `/mmo` show a compact tabular skill summary in the chat area by default.
Keep the implemented protected inventory GUI available through `/mmo menu`
instead of opening it from the root command.

This plan builds on the implementation from `gui_plan.md`; it does not remove
or redesign the completed 9x3 inventory GUI.

## Current state

The previous plan has already been implemented:

- `/mmo` opens the protected 9x3 inventory GUI for players.
- `/mmo menu` opens the same GUI.
- `/mmo stats [player]` opens the GUI for player senders.
- `/mmo stats chat` provides a 10-line chat fallback.
- `src/mmo/ui/chat.rs` already owns four-character skill abbreviations,
  uniform-font cells, branch detail pages, disabled/max markers, and focused
  unit tests.

The existing fallback groups skills as three rows for Frontier, three rows for
Warfare, and three rows for Enterprise. It fits the line budget, but it reads
as three stacked lists rather than one summary table.

## Display constraints

Retain the measurements established in `gui_plan.md`:

- target the vanilla default 320-pixel chat width;
- use at most 10 explicit lines so the result fills the default unfocused chat
  height without pushing its own first rows out of view;
- use `minecraft:uniform` for every padded table component;
- do not assume Pumpkin can read a client's chat width, height, font, scale, or
  line-spacing settings; and
- remain understandable if a narrow client wraps a row or a resource pack
  replaces the uniform font.

The primary table should be substantially narrower than 320 pixels. Do not use
the full width merely because it is available at the vanilla default.

## Recommended table organization

Use branches as columns and canonical skill order as rows. The branches have
8, 7, and 8 skills, so all 23 skills fit in eight data rows:

| Row | Frontier | Warfare | Enterprise |
|---:|---|---|---|
| 1 | Agriculture | Blades | Smithing |
| 2 | Herbalism | Axes | Repair |
| 3 | Woodcutting | Archery | Salvage |
| 4 | Mining | Unarmed | Alchemy |
| 5 | Excavation | Defense | Enchanting |
| 6 | Fishing | Acrobatics | Tinkering |
| 7 | Husbandry | Sorcery | Trading |
| 8 | Taming | empty | Charisma |

This produces exactly 10 lines:

1. compact title and `/mmo menu` hint;
2. branch headers; and
3. eight parallel skill rows.

### Proposed visible form

Each table column is exactly 10 uniform-font characters. Columns are separated
by ` | `, for a maximum data/header width of 36 characters. A normal skill
cell uses a four-character abbreviation plus a right-aligned level:

```text
CABBAGE MMO · GUI /mmo menu
FRONTIER   | WARFARE    | ENTERPRISE
Agri L  1  | Blad L  1  | Smit L  1
Herb L 12  | Axes L  8  | Repa L  3
Wood L 27  | Arch L 14  | Salv L  6
Mine L 41  | Unar L  2  | Alch L 19
Exca L 10  | Defe L 31  | Ench L 11
Fish L 18  | Acro L 22  | Tink L  7
Husb L  9  | Sorc L  5  | Trad L  1
Tame L  4  |            | Char L  1
```

The example spacing is illustrative; the implementation must construct cells
from a fixed-width formatter rather than storing pre-padded rows.

### Cell format

Use a fixed 10-character cell:

```text
<4-char skill code> L<level right-aligned to 3 characters><1 trailing space>
```

Examples:

- `Agri L  1 `
- `Mine L 41 `
- `Wood L100 `
- ten spaces for Warfare's missing eighth skill

Levels above three digits should not expand the grid. Clamp only the visible
field to a compact three-character marker such as `1k+`; keep the authoritative
level in the hover text. This is presentation-only and must not change stored
or calculated levels.

Keep the existing unique abbreviations:

| Frontier | Warfare | Enterprise |
|---|---|---|
| Agri | Blad | Smit |
| Herb | Axes | Repa |
| Wood | Arch | Salv |
| Mine | Unar | Alch |
| Exca | Defe | Ench |
| Fish | Acro | Tink |
| Husb | Sorc | Trad |
| Tame | — | Char |

### Color and state

- Frontier header/cells: green.
- Warfare header/cells: red.
- Enterprise header/cells: gold.
- Separators: dark gray.
- Disabled skills: retain the visible level but render the cell dark gray and
  strikethrough; hover text says `Disabled`.
- Max-level skills: retain the visible level and render the cell bold. Hover
  text says `Max level`.
- If a skill is both disabled and maxed, disabled styling wins visibly while
  hover text reports both states.

Do not spend visible characters on `off` or `*`. Color/style and hover text
can carry those states without hiding the requested level or widening cells.

### Hover details

Attach `HoverEvent::ShowText` to components rather than to one flattened
string.

Branch headers should show:

- full branch name;
- branch mastery; and
- enabled skill count.

Skill cells should show:

- full skill name;
- exact level;
- current/required XP and percentage toward the next level;
- total XP; and
- disabled or max-level state.

The four-character code remains understandable without hover, so Bedrock
bridges, accessibility clients, and clients with hover disabled still receive
the essential skill-and-level summary.

### Title line

For a player's own root summary, use a short title such as:

```text
CABBAGE MMO · GUI /mmo menu
```

Style `CABBAGE MMO` gold/bold and `/mmo menu` aqua/underlined with a
`SuggestCommand` event. Keep the command readable as plain text; clicking is
optional convenience, not required navigation.

For another player's summary, use `<name>'s MMO Skills` and omit the GUI hint
if the combined title would exceed the same conservative width budget.

## Command behavior

### Player senders

- `/mmo` fetches the sender's snapshot and sends the new 10-line chat grid.
- `/mmo menu` keeps opening the existing protected 9x3 GUI for the sender.
- `/mmo stats` sends the same chat grid as `/mmo`.
- `/mmo stats <player>` sends the selected online player's chat grid.
- `/mmo stats chat` remains a backward-compatible alias for the summary grid.
- `/mmo stats chat <branch>` keeps the implemented full-XP branch detail page.
- `/mmo help [page]` remains unchanged except that its command descriptions
  must say `/mmo` shows chat and `/mmo menu` opens the GUI.

To keep all GUI behavior under the explicit menu subcommand, add an optional
target argument to `/mmo menu [player]` if viewing another player's inventory
grid is still desired. Reuse the existing viewer/target split in
`open_skill_menu`; do not duplicate GUI construction.

### Console and RCON

- `/mmo` continues to show paginated help because there is no player snapshot
  or chat HUD to target.
- `/mmo stats <player>` may retain the existing complete text output; it is not
  constrained by the in-game chat area.
- `/mmo menu` returns the existing player-only error.

### Errors

If snapshot loading fails, send one short red error. Do not open the GUI as an
automatic fallback and do not print the former 27-line text table.

## Implementation plan

### 1. Reorient the summary builder

In `src/mmo/ui/chat.rs`:

- replace the branch-by-branch `chunks(3)` summary loop with an eight-row
  parallel-column loop;
- keep `skill_abbr`, branch colors, `SkillInfo`, and branch detail pages;
- add a fixed cell-width constant and one formatter for normal, oversized,
  disabled, maxed, and empty cells;
- create branch-header, skill-cell, separator, and title components separately
  so their style and hover metadata remain intact; and
- rename `summary_lines` only if a clearer name materially improves call sites.

The row builder should use the maximum branch length and look up each branch's
skill at the current row index. It must not hard-code 8 in the iteration logic,
although tests should assert the current result is eight data rows.

### 2. Route the root command to chat

In `src/mmo/commands.rs`:

- change `MmoRootExecutor` for players from `menu::open_skill_menu` to shared
  snapshot loading plus the summary sender;
- factor the snapshot/config/line-send sequence out of
  `MmoChatStatsExecutor` so root, stats, and stats-chat do not drift;
- change player `MmoStatsExecutor` to the chat summary;
- leave `MmoMenuExecutor` as the only inventory-GUI entry point;
- optionally add the target argument to the menu command; and
- update help text and GUI failure/error hints.

Send each visual row as its own system message, as the current chat fallback
does. Do not flatten all rows into one newline-delimited `TextComponent`, since
per-cell hover and styling must be preserved.

### 3. Preserve the inventory GUI

Do not change the following behavior in `src/mmo/ui/menu.rs` except for any
optional target command wiring:

- fixed branch-row layout;
- skill icons and lore;
- branch mastery headers;
- Help slot callback;
- protected transfer policy; and
- viewer/target title handling.

### 4. Update tests

Replace the old stacked-summary assertions with tests that prove:

- the output is exactly 10 lines: title, headers, and eight skill rows;
- every one of the 23 skills occurs exactly once;
- each skill occurs in its branch's fixed column and canonical row;
- Warfare's eighth cell is blank and fixed width;
- every visible cell and header has the configured uniform-font width;
- one-, two-, and three-digit levels align identically;
- levels above 999 use the compact visible marker without changing hover data;
- disabled skills still display their level;
- maxed and disabled styling follows the stated precedence;
- branch and skill hover text contains the full names and detailed values;
- the title contains the literal `/mmo menu` fallback; and
- branch detail pages remain at or below 10 lines.

Add command-tree or executor-focused tests if the existing command framework
allows them without constructing a live server. Otherwise cover routing with
focused helper tests and the live verification matrix.

### 5. Update documentation

Update `src/mmo/README.md` and `src/mmo/BALANCE.md` so they state:

- `/mmo` is the default chat-area summary;
- `/mmo menu` is the protected inventory GUI;
- `/mmo stats [player]` is tabular chat for players; and
- `/mmo stats chat <branch>` remains the detailed branch view.

## Verification

- Run `cargo fmt`.
- Run the focused MMO UI/command tests.
- Run `cargo check`.
- In a Java client at default chat settings, confirm `/mmo` occupies exactly
  the 10-line unfocused chat area and no row wraps.
- Repeat at minimum and intermediate chat widths to verify graceful wrapping,
  accepting that the server cannot guarantee a grid at client-minimum width.
- Verify with default font, forced Unicode font, and a font-replacing resource
  pack.
- Check levels 1, 9, 10, 99, 100, and above 999.
- Check enabled, disabled, maxed, and disabled-plus-maxed skills.
- Hover every branch header and representative cells from all three branches.
- Confirm `/mmo menu` still opens the protected inventory grid and rejects all
  item-transfer click modes.
- Confirm `/mmo`, `/mmo stats`, `/mmo stats <player>`, `/mmo stats chat`, and
  `/mmo stats chat <branch>` follow the new routing.
- Confirm console/RCON help and stats behavior remains readable.

## Acceptance criteria

- `/mmo` shows a chat summary for players and no longer opens an inventory.
- The visible summary is a true three-column table with full branch names,
  skill abbreviations, and levels.
- The summary uses exactly 10 explicit lines and all 23 skills appear once.
- The normal table width is bounded to three 10-character cells plus two
  three-character separators.
- Full names and progress remain available through hover text.
- `/mmo menu` retains the completed protected 9x3 GUI behavior.
- The detailed branch chat pages and console text paths remain available.
- No code assumes the server knows the client's chat dimensions.
