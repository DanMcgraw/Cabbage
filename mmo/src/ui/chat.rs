//! Default chat skill summary plus detailed per-branch pages.
//!
//! The summary is one three-column table — a column per branch, a row per
//! skill index — capped at [`MAX_CHAT_LINES`] explicit lines (title, branch
//! headers, and one row per skill index) so it fits the vanilla 10-line
//! unfocused chat view before wrapping. Branch pages list one skill per line
//! with full XP values for clients that want the detail.
//!
//! Nothing here assumes the server knows a client's chat dimensions:
//! padding is only ever applied inside `minecraft:uniform` cells, and every
//! page stays understandable if alignment or wrapping differs.

use pumpkin_util::text::{TextComponent, click::ClickEvent, color::NamedColor, hover::HoverEvent};

use super::{
    super::{
        config::LevelCurve,
        progression::{PlayerSnapshot, branch_mastery},
        skills::{BranchId, SkillId},
    },
    progress_percent,
};

/// Explicit-line budget per chat page (vanilla unfocused chat view).
pub(crate) const MAX_CHAT_LINES: usize = 10;

const UNIFORM_FONT: &str = "minecraft:uniform";

/// Fixed character width of every table column (header and skill cells).
const CELL_WIDTH: usize = 10;

/// Character width of the right-aligned visible level field inside a cell.
const LEVEL_FIELD_WIDTH: usize = 3;

/// Column separator between table cells.
const SEPARATOR: &str = " | ";

/// Visible budget for the title line: the full table width. The title is
/// plain prose (no padding), so this is only a conservative character count.
const TABLE_WIDTH: usize = 3 * CELL_WIDTH + 2 * SEPARATOR.len();

/// Visible level cap before the compact marker takes over the field.
const MAX_VISIBLE_LEVEL: u32 = 999;

/// Per-skill presentation inputs (level curve, enabled) resolved from live
/// config once per page build.
pub(crate) type SkillInfo<'a> = dyn Fn(SkillId) -> (LevelCurve, bool) + 'a;

fn branch_color(branch: BranchId) -> NamedColor {
    match branch {
        BranchId::Frontier => NamedColor::Green,
        BranchId::Warfare => NamedColor::Red,
        BranchId::Enterprise => NamedColor::Gold,
    }
}

/// Four-letter skill abbreviations used in the compact summary.
fn skill_abbr(skill: SkillId) -> &'static str {
    match skill {
        SkillId::Agriculture => "Agri",
        SkillId::Herbalism => "Herb",
        SkillId::Woodcutting => "Wood",
        SkillId::Mining => "Mine",
        SkillId::Excavation => "Exca",
        SkillId::Fishing => "Fish",
        SkillId::Husbandry => "Husb",
        SkillId::Taming => "Tame",
        SkillId::Blades => "Blad",
        SkillId::Axes => "Axes",
        SkillId::Archery => "Arch",
        SkillId::Unarmed => "Unar",
        SkillId::Defense => "Defe",
        SkillId::Acrobatics => "Acro",
        SkillId::Sorcery => "Sorc",
        SkillId::Smithing => "Smit",
        SkillId::Repair => "Repa",
        SkillId::Salvage => "Salv",
        SkillId::Alchemy => "Alch",
        SkillId::Enchanting => "Ench",
        SkillId::Tinkering => "Tink",
        SkillId::Trading => "Trad",
        SkillId::Charisma => "Char",
    }
}

/// Visible level field: right-aligned to [`LEVEL_FIELD_WIDTH`] characters.
/// Levels above three digits clamp to a compact marker so the grid never
/// widens; the exact level stays in the hover text. Presentation-only.
fn level_field(level: u32) -> String {
    if level > MAX_VISIBLE_LEVEL {
        "1k+".to_string()
    } else {
        format!("{level:>LEVEL_FIELD_WIDTH$}")
    }
}

/// Fixed-width skill cell text: four-character code, `L`, the right-aligned
/// level field, and one trailing space — exactly [`CELL_WIDTH`] characters.
fn skill_cell_text(skill: SkillId, level: u32) -> String {
    format!("{} L{} ", skill_abbr(skill), level_field(level))
}

/// Fixed-width branch header text: uppercased branch name, left-aligned.
fn branch_header_text(branch: BranchId) -> String {
    format!(
        "{:<CELL_WIDTH$}",
        branch.display_name().to_ascii_uppercase()
    )
}

fn uniform(text: String) -> TextComponent {
    TextComponent::text(text).font(UNIFORM_FONT.to_string())
}

fn separator() -> TextComponent {
    uniform(SEPARATOR.to_string()).color_named(NamedColor::DarkGray)
}

/// Title line: `CABBAGE MMO` (gold/bold) plus an aqua/underlined `/mmo menu`
/// hint with a suggest-command click event. For another player's summary the
/// base becomes `<name>'s MMO Skills`; the hint is only kept while the
/// combined title stays inside the conservative table-width budget.
fn summary_title(target_name: Option<&str>) -> TextComponent {
    const MENU_HINT: &str = "/mmo menu";
    const MIDDLE: &str = " · GUI ";

    let (base, base_len) = match target_name {
        None => (
            TextComponent::text("CABBAGE MMO")
                .color_named(NamedColor::Gold)
                .bold(),
            "CABBAGE MMO".len(),
        ),
        Some(name) => {
            let text = format!("{name}'s MMO Skills");
            (
                TextComponent::text(text.clone())
                    .color_named(NamedColor::Gold)
                    .bold(),
                text.len(),
            )
        }
    };

    if base_len + MIDDLE.len() + MENU_HINT.len() > TABLE_WIDTH {
        return base;
    }
    base.add_text(MIDDLE).add_child(
        TextComponent::text(MENU_HINT)
            .color_named(NamedColor::Aqua)
            .underlined()
            .click_event(ClickEvent::SuggestCommand {
                command: MENU_HINT.into(),
            }),
    )
}

/// Branch header hover: full name, mastery, and enabled skill count.
fn branch_hover(branch: BranchId, snapshot: &PlayerSnapshot, skill_info: &SkillInfo) -> String {
    let mastery = branch_mastery(branch, |skill| {
        let (curve, _) = skill_info(skill);
        snapshot.level_of(skill, &curve)
    });
    let skills = branch.skills();
    let enabled = skills.iter().filter(|skill| skill_info(**skill).1).count();
    format!(
        "{}\nMastery: {mastery:.1}\nEnabled: {enabled}/{} skills",
        branch.display_name(),
        skills.len()
    )
}

/// Skill cell hover: full name, exact level, progress toward the next level,
/// total XP, and disabled/max-level state.
fn skill_hover(
    skill: SkillId,
    snapshot: &PlayerSnapshot,
    curve: &LevelCurve,
    enabled: bool,
) -> String {
    let progress = snapshot.get(skill);
    let level = progress.level(curve);
    let maxed = level >= curve.max_level();
    let mut lines = vec![skill.display_name().to_string(), format!("Level {level}")];
    if !maxed {
        let (_, into, needed) = curve.level_for_xp(progress.xp);
        lines.push(format!(
            "XP: {into}/{needed} ({}%)",
            progress_percent(into, needed)
        ));
    }
    lines.push(format!("Total XP: {}", progress.xp));
    if !enabled {
        lines.push("Disabled".to_string());
    }
    if maxed {
        lines.push("Max level".to_string());
    }
    lines.join("\n")
}

fn branch_header_cell(
    branch: BranchId,
    snapshot: &PlayerSnapshot,
    skill_info: &SkillInfo,
) -> TextComponent {
    uniform(branch_header_text(branch))
        .color_named(branch_color(branch))
        .hover_event(HoverEvent::show_text(TextComponent::text(branch_hover(
            branch, snapshot, skill_info,
        ))))
}

/// One skill cell: branch-colored, bold at max level, dark gray and
/// struck through when disabled (disabled styling wins over max styling,
/// while the hover reports both states).
fn skill_cell(
    skill: SkillId,
    snapshot: &PlayerSnapshot,
    curve: &LevelCurve,
    enabled: bool,
) -> TextComponent {
    let level = snapshot.level_of(skill, curve);
    let maxed = level >= curve.max_level();
    let cell = uniform(skill_cell_text(skill, level)).hover_event(HoverEvent::show_text(
        TextComponent::text(skill_hover(skill, snapshot, curve, enabled)),
    ));
    if !enabled {
        cell.color_named(NamedColor::DarkGray).strikethrough()
    } else if maxed {
        cell.color_named(branch_color(skill.branch())).bold()
    } else {
        cell.color_named(branch_color(skill.branch()))
    }
}

/// A blank fixed-width cell for branch columns without a skill at this row.
fn empty_cell() -> TextComponent {
    uniform(" ".repeat(CELL_WIDTH))
}

/// The number of data rows in the summary table: the largest branch's skill
/// count. Derived from the branch definitions, never hard-coded.
fn summary_row_count() -> usize {
    BranchId::ALL
        .iter()
        .map(|branch| branch.skills().len())
        .max()
        .unwrap_or(0)
}

/// Default chat summary: title, one branch-header row, and one parallel
/// three-column row per skill index. `target_name` is `None` when viewers
/// look at their own skills, or `Some(name)` for another online player.
pub(crate) fn summary_lines(
    snapshot: &PlayerSnapshot,
    skill_info: &SkillInfo,
    target_name: Option<&str>,
) -> Vec<TextComponent> {
    let mut lines = Vec::with_capacity(2 + summary_row_count());
    lines.push(summary_title(target_name));

    let mut header = TextComponent::empty();
    for (index, branch) in BranchId::ALL.iter().enumerate() {
        if index > 0 {
            header = header.add_child(separator());
        }
        header = header.add_child(branch_header_cell(*branch, snapshot, skill_info));
    }
    lines.push(header);

    for row in 0..summary_row_count() {
        let mut line = TextComponent::empty();
        for (index, branch) in BranchId::ALL.iter().enumerate() {
            if index > 0 {
                line = line.add_child(separator());
            }
            let cell = match branch.skills().get(row) {
                Some(skill) => {
                    let (curve, enabled) = skill_info(*skill);
                    skill_cell(*skill, snapshot, &curve, enabled)
                }
                None => empty_cell(),
            };
            line = line.add_child(cell);
        }
        lines.push(line);
    }

    debug_assert!(lines.len() <= MAX_CHAT_LINES);
    lines
}

/// One branch per page with full XP values: heading, one row per skill
/// (8 at most), and a navigation hint — 10 lines for the largest branch.
pub(crate) fn branch_lines(
    snapshot: &PlayerSnapshot,
    skill_info: &SkillInfo,
    branch: BranchId,
) -> Vec<TextComponent> {
    let mastery = branch_mastery(branch, |skill| {
        let (curve, _) = skill_info(skill);
        snapshot.level_of(skill, &curve)
    });
    let mut lines = vec![
        TextComponent::text(format!(
            "== {} (mastery {mastery:.1}) ==",
            branch.display_name()
        ))
        .color_named(branch_color(branch))
        .bold(),
    ];
    for skill in branch.skills() {
        let (curve, enabled) = skill_info(*skill);
        let row = if !enabled {
            format!("{}: Disabled", skill.display_name())
        } else {
            let progress = snapshot.get(*skill);
            let (level, into, needed) = curve.level_for_xp(progress.xp);
            if level >= curve.max_level() {
                format!(
                    "{}: Level {} (Max level, {} total XP)",
                    skill.display_name(),
                    level,
                    progress.xp
                )
            } else {
                format!(
                    "{}: Level {} ({}/{} XP, {}%)",
                    skill.display_name(),
                    level,
                    into,
                    needed,
                    progress_percent(into, needed)
                )
            }
        };
        lines.push(TextComponent::text(row));
    }
    lines.push(branch_nav_line(branch));
    debug_assert!(lines.len() <= MAX_CHAT_LINES);
    lines
}

/// Navigation hint to the other branches. Click events suggest the command
/// on Java clients, but the plain command text keeps navigation usable by
/// typing regardless of client behavior.
fn branch_nav_line(current: BranchId) -> TextComponent {
    let mut line = TextComponent::text("Other branches: ").color_named(NamedColor::Gray);
    let mut first = true;
    for branch in BranchId::ALL {
        if *branch == current {
            continue;
        }
        if !first {
            line = line.add_text(", ");
        }
        first = false;
        let name = branch.display_name().to_ascii_lowercase();
        line = line.add_child(
            TextComponent::text(name.clone())
                .color_named(branch_color(*branch))
                .underlined()
                .click_event(ClickEvent::SuggestCommand {
                    command: format!("/mmo stats chat {name}").into(),
                }),
        );
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::SkillConfig, progression::PlayerSkillSnapshot};
    use pumpkin_util::text::{
        TextComponentBase, TextContent, click::ClickEvent, color::Color, hover::HoverEvent,
    };

    /// Test curve thresholds: 0, 100, 300, 700, 1500 (max level 5).
    fn test_curve() -> LevelCurve {
        LevelCurve::new(&SkillConfig {
            max_level: 5,
            base_xp: 100,
            xp_multiplier: 2.0,
            enabled: true,
        })
    }

    /// One-XP-per-level curve so tests can reach three- and four-digit
    /// levels without huge XP values.
    fn tall_curve(max_level: u32) -> LevelCurve {
        LevelCurve::new(&SkillConfig {
            max_level,
            base_xp: 1,
            xp_multiplier: 1.0,
            enabled: true,
        })
    }

    fn enabled_info() -> impl Fn(SkillId) -> (LevelCurve, bool) {
        |_: SkillId| (test_curve(), true)
    }

    fn join_text(lines: &[TextComponent]) -> String {
        lines
            .iter()
            .map(|line| line.clone().get_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn children(line: &TextComponent) -> &[TextComponentBase] {
        &line.0.extra
    }

    fn plain_text(base: &TextComponentBase) -> &str {
        match &*base.content {
            TextContent::Text { text } => text.as_ref(),
            other => panic!("expected plain text, got {other:?}"),
        }
    }

    fn hover_text(base: &TextComponentBase) -> String {
        match &base.style.hover_event {
            Some(HoverEvent::ShowText { value }) => value
                .iter()
                .map(|component| TextComponent(component.clone()).get_text())
                .collect::<Vec<_>>()
                .join("\n"),
            other => panic!("expected a ShowText hover, got {other:?}"),
        }
    }

    /// Cells of a table row (header or data): children with the two
    /// separators removed.
    fn row_cells(line: &TextComponent) -> Vec<&TextComponentBase> {
        children(line)
            .iter()
            .filter(|child| plain_text(child) != SEPARATOR)
            .collect()
    }

    #[test]
    fn abbreviations_are_unique_fixed_width_four_letter_codes() {
        let mut abbrs: Vec<&str> = SkillId::ALL
            .iter()
            .map(|skill| skill_abbr(*skill))
            .collect();
        assert!(abbrs.iter().all(|abbr| abbr.len() == 4));
        abbrs.sort_unstable();
        abbrs.dedup();
        assert_eq!(abbrs.len(), SkillId::ALL.len());
    }

    #[test]
    fn summary_is_ten_lines_title_headers_and_eight_rows() {
        let snapshot = PlayerSnapshot::default();
        let lines = summary_lines(&snapshot, &enabled_info(), None);
        assert_eq!(lines.len(), MAX_CHAT_LINES);
        // Title, branch header row, then the data rows.
        assert_eq!(lines.len() - 2, 8);
        assert_eq!(summary_row_count(), 8);
    }

    #[test]
    fn summary_shows_every_skill_exactly_once() {
        let snapshot = PlayerSnapshot::default();
        let text = join_text(&summary_lines(&snapshot, &enabled_info(), None));
        for skill in SkillId::ALL {
            assert_eq!(
                text.matches(skill_abbr(*skill)).count(),
                1,
                "summary must show {skill} exactly once: {text}"
            );
        }
    }

    #[test]
    fn skills_occupy_their_branch_column_and_canonical_row() {
        let snapshot = PlayerSnapshot::default();
        let lines = summary_lines(&snapshot, &enabled_info(), None);
        for (column, branch) in BranchId::ALL.iter().enumerate() {
            for (row, skill) in branch.skills().iter().enumerate() {
                let cells = row_cells(&lines[2 + row]);
                assert!(
                    plain_text(cells[column]).starts_with(skill_abbr(*skill)),
                    "{skill} must sit in row {row}, column {column}"
                );
            }
        }
    }

    #[test]
    fn missing_warfare_eighth_skill_is_a_blank_fixed_width_cell() {
        let snapshot = PlayerSnapshot::default();
        let lines = summary_lines(&snapshot, &enabled_info(), None);
        let last_row = row_cells(lines.last().expect("eighth data row"));
        let warfare_cell = plain_text(last_row[1]);
        assert_eq!(warfare_cell, " ".repeat(CELL_WIDTH));
        assert_eq!(warfare_cell.len(), CELL_WIDTH);
    }

    #[test]
    fn every_cell_header_and_separator_is_uniform_font_fixed_width() {
        let snapshot = PlayerSnapshot::default();
        let lines = summary_lines(&snapshot, &enabled_info(), None);
        for line in &lines[1..] {
            for child in children(line) {
                assert_eq!(
                    child.style.font.as_deref(),
                    Some(UNIFORM_FONT),
                    "table components must use the uniform font"
                );
                let expected = if plain_text(child) == SEPARATOR {
                    SEPARATOR.len()
                } else {
                    CELL_WIDTH
                };
                assert_eq!(plain_text(child).len(), expected);
            }
        }
    }

    #[test]
    fn one_two_and_three_digit_levels_align_identically() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Agriculture, PlayerSkillSnapshot::new(0)); // L1
        snapshot.set(SkillId::Herbalism, PlayerSkillSnapshot::new(100)); // L2
        let skill_info = |skill: SkillId| {
            if skill == SkillId::Woodcutting {
                (tall_curve(200), true)
            } else {
                (test_curve(), true)
            }
        };
        snapshot.set(SkillId::Woodcutting, PlayerSkillSnapshot::new(99)); // L100
        let lines = summary_lines(&snapshot, &skill_info, None);
        assert_eq!(plain_text(row_cells(&lines[2])[0]), "Agri L  1 ");
        assert_eq!(plain_text(row_cells(&lines[3])[0]), "Herb L  2 ");
        assert_eq!(plain_text(row_cells(&lines[4])[0]), "Wood L100 ");
    }

    #[test]
    fn levels_above_999_clamp_visibly_but_hover_keeps_the_exact_level() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Mining, PlayerSkillSnapshot::new(1000)); // L1001
        let skill_info = |_: SkillId| (tall_curve(2000), true);
        let lines = summary_lines(&snapshot, &skill_info, None);
        let cells = row_cells(&lines[5]); // Mining is Frontier row index 3
        let mining = cells[0];
        assert_eq!(plain_text(mining), "Mine L1k+ ");
        assert_eq!(plain_text(mining).len(), CELL_WIDTH);
        assert!(hover_text(mining).contains("Level 1001"));
    }

    #[test]
    fn disabled_cells_keep_their_level_with_disabled_styling() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Trading, PlayerSkillSnapshot::new(100)); // L2
        let skill_info = |skill: SkillId| (test_curve(), skill != SkillId::Trading);
        let lines = summary_lines(&snapshot, &skill_info, None);
        // Trading is Enterprise row index 6, column 2.
        let cells = row_cells(&lines[2 + 6]);
        let trading = cells[2];
        assert_eq!(plain_text(trading), "Trad L  2 ");
        assert_eq!(
            trading.style.color,
            Some(Color::Named(NamedColor::DarkGray))
        );
        assert_eq!(trading.style.strikethrough, Some(true));
        let hover = hover_text(trading);
        assert!(hover.contains("Trading"));
        assert!(hover.contains("Level 2"));
        assert!(hover.contains("Disabled"));
    }

    #[test]
    fn maxed_cells_are_bold_and_disabled_wins_over_maxed() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Mining, PlayerSkillSnapshot::new(u64::MAX)); // maxed
        snapshot.set(SkillId::Trading, PlayerSkillSnapshot::new(u64::MAX)); // maxed + disabled
        let skill_info = |skill: SkillId| (test_curve(), skill != SkillId::Trading);
        let lines = summary_lines(&snapshot, &skill_info, None);

        // Mining: Frontier row index 3, column 0.
        let mining = row_cells(&lines[2 + 3])[0];
        assert_eq!(plain_text(mining), "Mine L  5 ");
        assert_eq!(mining.style.bold, Some(true));
        assert_eq!(mining.style.strikethrough, None);
        assert_eq!(mining.style.color, Some(Color::Named(NamedColor::Green)));
        let mining_hover = hover_text(mining);
        assert!(mining_hover.contains("Max level"));
        assert!(!mining_hover.contains("Disabled"));

        // Trading: Enterprise row index 6, column 2.
        let trading = row_cells(&lines[2 + 6])[2];
        assert_eq!(plain_text(trading), "Trad L  5 ");
        assert_eq!(trading.style.strikethrough, Some(true));
        assert_eq!(trading.style.bold, None);
        assert_eq!(
            trading.style.color,
            Some(Color::Named(NamedColor::DarkGray))
        );
        let trading_hover = hover_text(trading);
        assert!(trading_hover.contains("Disabled"));
        assert!(trading_hover.contains("Max level"));
    }

    #[test]
    fn hover_texts_carry_full_names_and_detailed_values() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Mining, PlayerSkillSnapshot::new(150)); // L2, 50/200
        let lines = summary_lines(&snapshot, &enabled_info(), None);

        let header_cells = row_cells(&lines[1]);
        let frontier_hover = hover_text(header_cells[0]);
        assert!(frontier_hover.contains("Frontier"));
        assert!(frontier_hover.contains("Mastery:"));
        assert!(frontier_hover.contains("Enabled: 8/8 skills"));

        let mining = row_cells(&lines[2 + 3])[0];
        let hover = hover_text(mining);
        assert!(hover.contains("Mining"));
        assert!(hover.contains("Level 2"));
        assert!(hover.contains("XP: 50/200 (25%)"));
        assert!(hover.contains("Total XP: 150"));
    }

    #[test]
    fn own_title_contains_the_literal_mmo_menu_hint() {
        let snapshot = PlayerSnapshot::default();
        let lines = summary_lines(&snapshot, &enabled_info(), None);
        let title = &lines[0];
        assert!(title.clone().get_text().contains("/mmo menu"));
        let hint = children(title)
            .iter()
            .find(|child| plain_text(child) == "/mmo menu")
            .expect("title must contain the /mmo menu hint");
        assert_eq!(
            hint.style.click_event,
            Some(ClickEvent::SuggestCommand {
                command: "/mmo menu".into()
            })
        );
    }

    #[test]
    fn other_player_title_drops_the_hint_only_when_it_would_exceed_budget() {
        let snapshot = PlayerSnapshot::default();

        let short = summary_lines(&snapshot, &enabled_info(), Some("Al"));
        let short_title = short[0].clone().get_text();
        assert!(short_title.contains("Al's MMO Skills"));
        assert!(short_title.contains("/mmo menu"));

        let long = summary_lines(&snapshot, &enabled_info(), Some("AVeryLongPlayerName"));
        let long_title = long[0].clone().get_text();
        assert_eq!(long_title, "AVeryLongPlayerName's MMO Skills");
        assert!(!long_title.contains("/mmo menu"));
    }

    #[test]
    fn branch_pages_fit_the_line_budget() {
        let snapshot = PlayerSnapshot::default();
        for branch in BranchId::ALL {
            let lines = branch_lines(&snapshot, &enabled_info(), *branch);
            assert_eq!(lines.len(), 2 + branch.skills().len());
            assert!(lines.len() <= MAX_CHAT_LINES);
        }
    }

    #[test]
    fn branch_page_shows_full_progress_max_and_disabled_states() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Mining, PlayerSkillSnapshot::new(150)); // L2, 50/200
        snapshot.set(SkillId::Fishing, PlayerSkillSnapshot::new(u64::MAX)); // maxed
        let skill_info = |skill: SkillId| (test_curve(), skill != SkillId::Taming);
        let text = join_text(&branch_lines(&snapshot, &skill_info, BranchId::Frontier));
        assert!(text.contains("Mining: Level 2 (50/200 XP, 25%)"), "{text}");
        assert!(
            text.contains("Fishing: Level 5 (Max level, 18446744073709551615 total XP)"),
            "{text}"
        );
        assert!(text.contains("Taming: Disabled"), "{text}");
    }

    #[test]
    fn branch_nav_line_links_only_the_other_branches() {
        let snapshot = PlayerSnapshot::default();
        let lines = branch_lines(&snapshot, &enabled_info(), BranchId::Warfare);
        let nav = lines.last().expect("nav line").clone().get_text();
        assert!(nav.contains("frontier"));
        assert!(nav.contains("enterprise"));
        assert!(!nav.contains("warfare"));
    }
}
