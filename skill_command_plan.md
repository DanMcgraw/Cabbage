# Skill Detail Command Plan

## Goal

Add a player-facing detailed skill page that can be opened three equivalent
ways:

- `/mmo skill <skill>`
- clicking a skill in the default `/mmo` chat summary
- clicking that skill's icon in `/mmo menu`

The page is sent in chat and explains the player's current progress and the
skill's progression unlocks. It is informational only; it does not change XP,
perks, configuration, or player data.

## Command contract

Register `skill` as a player command under the existing `/mmo` tree:

```text
/mmo skill <skill>
```

- `<skill>` uses the existing canonical skill argument/suggestions.
- The ten temporary retired spellings continue to parse through
  `SkillId::from_name`, then display the canonical destination. For example,
  `/mmo skill repair` opens **Maintenance**.
- Console and RCON receive a concise error explaining that the page needs an
  online player, consistent with `/mmo menu`.
- The command uses the caller's own progress. Do not add an optional target
  until there is a defined permissions and privacy policy for it.

## Page content

Render a compact, readable chat page for one skill. Keep the normal display
within the vanilla ten-line unfocused-chat budget; if a skill has more unlocks
than fit, use explicit pages (`/mmo skill <skill> [page]`) rather than silently
omitting entries.

The first page should contain:

1. A branch-coloured heading: `<Skill> — <Branch>`.
2. Current state: level, total XP, and either `XP: current / next` with a
   percentage or `Max level`.
3. A disabled-state explanation when the skill or global MMO module is
   disabled. Retain its stored level and XP in the display.
4. A short description of the XP activities that contribute to this skill.
   Merged skills explicitly name both activity families, such as Cultivation
   (agriculture and herbalism) and Maintenance (repair and salvage).
5. The progression section, ordered by unlock level. Each row shows the
   required level, a concise unlock name, a plain-language effect, and its
   state: `Unlocked`, `Next`, `Locked`, `Disabled`, or `Planned`.
6. A navigation footer with clickable suggested commands for the previous and
   next skill in canonical branch order, the branch detail page, and `/mmo`.

The initial unlock timeline includes the shared milestone slots at levels 25,
50, 75, and 100. These are labelled **Planned** until their concrete gameplay
perk is implemented. Active, level-scaled effects (for example Mining
Prospector, Defense resilience, or Enchanting offer discount) are also shown,
but as `Active from level 1` with their live configured value at the player's
current level. This avoids falsely presenting every current perk as a future
threshold unlock.

## Single source of truth

Add a data-only skill progression catalog, preferably in
`mmo/src/ui/skill_detail.rs` (or a small sibling module if it becomes shared).
It should map every `SkillId` to:

- XP-source description(s),
- the live effects to describe,
- milestone entries at 25/50/75/100, and
- merged-skill notes where relevant.

The catalog must use `SkillId`, the existing branch configuration types, and
the shared constants in `perks::eligibility`; do not duplicate magic milestone
levels. Resolve configuration-dependent text and values while rendering, so
`/mmo reload` immediately changes the page. A disabled global perk switch or
individual feature switch must produce an explicit `Disabled` state rather
than hiding the affected row.

Keep this catalog display-only in the first implementation. It must not become
the authority for perk activation; gameplay handlers remain the source of
truth for whether an effect can fire.

## Integration work

1. Add `SkillDetailExecutor` and the `literal("skill")` command branch in
   `mmo/src/commands.rs`; update help text and command tests.
2. Add `ui::skill_detail::skill_detail_lines(...)`, returning structured
   `TextComponent` lines. The command executor sends those lines after loading
   the caller's `PlayerSnapshot` with the same async database path used by
   `/mmo` and `/mmo stats`.
3. Update `ui/chat.rs` so each skill cell has a `SuggestCommand` click event
   for `/mmo skill <CanonicalSkill>`. Preserve hover text and fixed-width grid
   layout.
4. Update `ui/menu.rs` so `SkillMenuHandler` identifies `MenuSlot::Skill` at
   the clicked slot, closes the GUI, and sends the same detail page. Continue
   cancelling every click so the menu remains read-only; branch headers and
   fillers stay inert, and Help keeps its current behavior.
5. Update `mmo/src/README.md` and `mmo/src/BALANCE.md` to describe the command,
   clickable skill entries, live-effects terminology, and planned milestone
   slots accurately.

## Tests and acceptance criteria

- Unit-test that every canonical `SkillId` has a catalog entry and no catalog
  entry is orphaned.
- Test milestone order and that 25/50/75/100 come from the eligibility
  constants.
- Test a level below, at, and above each milestone renders the correct state.
- Test disabled MMO, disabled skill, disabled global perks, and disabled
  individual effect switches are labelled rather than omitted.
- Test merged skills list both contributing activities and legacy command
  aliases resolve to their canonical detail page.
- Test generated page length obeys the chat-line budget and pagination has no
  missing or duplicate rows.
- Test every summary skill cell suggests its canonical `/mmo skill <skill>`
  command.
- Test clicking a menu skill closes the GUI, opens the matching page, and
  cannot move inventory items; existing Help and inert-slot behavior remains
  unchanged.
- Run `cargo fmt`, `cargo check --workspace`, and `cargo test --workspace`.

## Non-goals

- No per-skill inventory GUI.
- No new persistence, database schema, configuration format, or permission.
- No changes to perk balance or unlock timing.
- No promise that the level-25/50/75/100 placeholders are already active;
  their presentation must remain visibly planned until implemented.
