# Cabbage MMO GUI Plan

> **Superseded (historical record).** The 23-skill model this plan was
> written for was replaced by the 18-skill, six-per-branch consolidation in
> `skill_consolidation_plan.md`, which defines the final `/mmo` chat grid
> (eight lines, 12-character cells) and the `/mmo menu` 9x3 layout (3
> headers, 18 skills, Help, 5 fillers). Keep this document as the design
> record of the original GUI work; do not treat its skill counts or layout
> as current.

## Goal

Replace the oversized `/mmo` chat response with a player-facing grid that
shows all 23 skills at once, while preserving usable command output for the
console and for clients that cannot open the native plugin GUI.

This is an implementation plan only. No command or GUI behavior is changed by
this document.

## Chat-size research

Minecraft Java chat does **not** have a server-visible row and column count.
The client lays text out in pixels using local accessibility/chat options and
the active font. In the 1.21.11 client, `ChatHud` calculates:

```text
chat width  = floor(40 + 280 * width_option) pixels
chat height = floor(20 + 160 * height_option) pixels
line height = floor(9 * (1 + line_spacing_option)) pixels
visible rows = chat height / line height
```

The option values are client-local and range from 0 to 1. At vanilla defaults:

| Measurement | Default | Possible vanilla range |
|---|---:|---:|
| Chat width | 320 px | 40-320 px |
| Focused/open chat height | 180 px | 20-180 px |
| Unfocused/closed chat height | 90 px | 20-180 px |
| Line height | 9 px | 9-18 px |
| Visible focused rows | 20 | 1-20, depending on height and spacing |
| Visible unfocused rows | 10 | 1-20, depending on height and spacing |

Chat scale also changes the effective wrapping width. The normal Minecraft
font is proportional, so pixels cannot be converted to a reliable character
count. A client resource pack can change glyph widths as well.

The serverbound client-information packet only reports locale, view distance,
chat visibility mode, chat-color preference, skin parts, main hand, text
filtering, and server-listing preference. It does not report chat width,
height, scale, line spacing, GUI scale, screen resolution, or font. Pumpkin
therefore cannot select a safe per-player column width.

### Current Cabbage output

For a normal player, `/mmo` currently emits 33 explicit lines:

- 5 help lines;
- 1 blank separator and 1 `Your skills` heading;
- 3 branch headings; and
- 23 skill rows.

An operator receives four additional admin-help lines, for 37 total. Long
skill-progress and help lines can wrap into still more visual rows. `/mmo
stats` is also 27 explicit lines before wrapping. Neither response can fit in
the vanilla default 10-line unfocused view or 20-line focused view.

### Conclusion

A three-column chat table can be made attractive at one chosen width, but it
cannot be guaranteed to align or remain on one screen for every client. It
should only be a fallback. The native inventory GUI has a server-selected,
stable 9-column by 3-row container and is the correct primary grid.

## Recommended in-game design

### Primary skill grid

Use the existing protected `Generic9x3` plugin GUI. Assign one branch to each
row and reserve the first slot in the row for a branch summary:

```text
row 1: [Frontier]   [Agriculture] [Herbalism] [Woodcutting] [Mining] [Excavation] [Fishing] [Husbandry] [Taming]
row 2: [Warfare]    [Blades]      [Axes]       [Archery]     [Unarmed] [Defense]    [Acrobatics] [Sorcery] [Help]
row 3: [Enterprise] [Smithing]    [Repair]     [Salvage]     [Alchemy] [Enchanting] [Tinkering]  [Trading] [Charisma]
```

This uses all 27 slots, keeps branch boundaries visually stable, and displays
all 23 skills without scrolling.

Each branch slot should show:

- branch name and representative icon;
- branch mastery; and
- the number of enabled skills in that branch.

Each skill slot should show:

- a skill-specific icon rather than repeating the branch icon;
- skill name and level in the custom name;
- current/required XP and percentage toward the next level in lore;
- total XP in lore; and
- an explicit `Disabled` or `Max level` state where applicable.

The menu remains protected: players cannot take its icons or place inventory
items into it. The initial implementation should remain read-only except for
the Help slot. Skill-detail submenus can be added later without being required
to solve the current chat-overflow problem.

### Command behavior

1. `/mmo` from a player opens the skill grid.
2. `/mmo menu` remains an explicit alias and backward-compatible entry point.
3. `/mmo stats` from a player opens the same grid for that player.
4. `/mmo stats <player>` opens a read-only grid of the selected online player,
   preserving the command's current visibility rules.
5. `/mmo help` sends a compact, paginated command list rather than combining
   help and all skill data.
6. Console and RCON senders keep text output because they cannot open an
   inventory screen. Their output may be multiline because it is not bounded
   by the game chat HUD.
7. If GUI opening fails, send one concise error plus the chat-fallback command,
   rather than falling back automatically to 33 or more lines.

The root command must not fetch all skill rows merely to print help. This also
removes 23 unnecessary database requests when a player only wants command
usage.

## Chat fallback

Provide `/mmo stats chat [branch]` or an equivalent explicit fallback. Do not
try to put all 23 full progress records into one chat message.

- Without a branch, show a compact three-column level-only summary targeted at
  the 320 px default width. Keep it to at most 10 explicit lines.
- Use structured `TextComponent` children and `minecraft:uniform` where it is
  available, but treat alignment as best-effort because client resource packs
  and narrow chat settings remain outside server control.
- With a branch, show one branch per page with full XP values. The largest
  branch has 8 skills, so a heading plus 8 rows fits the default 10-line
  unfocused view before wrapping.
- Include a plain command hint for switching branch/page. Optional
  `SuggestCommand` click events may assist Java clients, but navigation must
  remain usable by typing because client behavior and accessibility settings
  vary.
- Never pad a table with a guessed number of spaces while using the default
  proportional font.

## Implementation phases

### Phase 1: Share snapshot loading

- Move the current snapshot fetcher out of command-only presentation code so
  the menu, stats command, and fallback formatter consume one immutable
  `PlayerSnapshot`.
- Generalize `open_skill_menu` to accept separate viewer and target players.
- Keep SQLite work on the existing database worker. Consider one `get_skills`
  request for all 23 rows if profiling shows the current 23 request/response
  round trips are visible when opening the menu.
- Preserve existing level-curve calculation and branch-mastery logic.

### Phase 2: Build and test the 9x3 layout

- Add explicit slot constants/mappings for branch headers, skills, and Help.
- Add a deterministic skill-to-icon mapping with a safe stone fallback.
- Build custom names and lore from the snapshot and current config.
- Keep `allow_grab_items` and `allow_put_items` false.
- Add pure unit tests proving every skill appears exactly once, every slot is
  in `0..27`, no slots collide, and each skill occupies its branch row.

### Phase 3: Route player commands to the grid

- Change the player root executor to open the menu.
- Add `/mmo help` and move the current command list there.
- Reuse the menu for player `/mmo stats [player]` calls.
- Preserve readable text behavior for console/RCON and existing permission
  checks for admin commands.
- Return short red errors for database or GUI-open failures.

### Phase 4: Add the bounded fallback

- Add a small chat presentation module rather than mixing width/layout logic
  into command execution.
- Implement branch paging and the optional three-column level summary.
- Apply `minecraft:uniform` only to the table cells, not error/help prose.
- Unit-test explicit row counts, abbreviations, max-level display, and very
  large configured XP values.

### Phase 5: Optional interaction

- Give the Help slot a `PluginGuiHandler::on_click` callback that closes the
  GUI and sends the compact help page.
- Later, skill clicks may open a separate detail/perk screen. Do not overload
  the summary grid with perk activation or state mutation in this change.
- Any new callback must validate the session, container slot, and target skill
  from server-owned slot mappings; never trust a client item stack as the
  action identity.

## Likely files

- `src/mmo/ui/menu.rs` - layout, item presentation, and owned click handler.
- `src/mmo/ui/chat.rs` - optional bounded fallback formatter.
- `src/mmo/ui/mod.rs` - UI module exports.
- `src/mmo/commands.rs` - root/help/stats routing and console behavior.
- `src/mmo/progression.rs` - only if snapshot helpers need a better shared
  home; do not move persistence or live player access here unnecessarily.
- `src/mmo/db.rs` - only if a batched all-skills read is added.
- `src/mmo/README.md` and `src/mmo/BALANCE.md` - final command/UI behavior.

## Verification

- `cargo fmt`
- `cargo check`
- focused unit tests for menu slot mapping and chat row bounds
- live Java-client checks at minimum/default/maximum chat width and height
- live checks with default and uniform fonts, plus a resource pack that
  replaces fonts
- normal-player and operator `/mmo` checks
- console/RCON `/mmo` and `/mmo stats` checks
- attempts to click, shift-click, number-key swap, drag, and insert items into
  the protected GUI
- rapid reopen and target disconnect while `/mmo stats <player>` is loading

## Acceptance criteria

- A player can see all 23 skill levels in one fixed 9x3 screen.
- `/mmo` no longer sends the 33/37-line combined help and stats response to a
  player.
- Branches occupy stable rows and every skill appears exactly once.
- The summary GUI remains read-only and cannot transfer or duplicate items.
- Console/RCON retains complete text access.
- The chat fallback uses no more than 10 explicit rows per page and remains
  understandable when alignment or wrapping differs.
- No implementation assumes that the server knows a client's chat dimensions.

## Research references

- Mojang 1.21.11 version metadata and official client artifact:
  <https://piston-meta.mojang.com/v1/packages/c94ff7d0895bf2272467e4b2639c6c77efcb2193/1.21.11.json>
- Yarn's mapped `ChatHud` API, used to identify the client methods verified in
  Mojang's bytecode:
  <https://maven.fabricmc.net/docs/yarn-1.21.11%2Bbuild.3/net/minecraft/client/gui/hud/ChatHud.html>
- Yarn's `GameOptions` mappings for chat width, focused/unfocused height, scale,
  and line spacing:
  <https://maven.fabricmc.net/docs/yarn-1.21.11%2Bbuild.3/net/minecraft/client/option/GameOptions.html>
- Yarn's `TextHandler` API showing pixel-width measurement and pixel-bounded
  line wrapping:
  <https://maven.fabricmc.net/docs/yarn-1.21.11%2Bbuild.1/net/minecraft/client/font/TextHandler.html>
- Pumpkin's local client-information packet definition:
  `../Pumpkin/pumpkin-protocol/src/java/server/play/client_information.rs`
- Pumpkin's native protected GUI API:
  `../Pumpkin/pumpkin/src/plugin/api/gui.rs`
