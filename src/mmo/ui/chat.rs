//! Bounded chat fallback for clients that cannot use the native skill grid.
//!
//! Two page shapes, each capped at [`MAX_CHAT_LINES`] explicit lines so they
//! fit the vanilla 10-line unfocused chat view before wrapping:
//!
//! - a level-only three-column summary using `minecraft:uniform` cells; and
//! - one branch per page with full XP values.
//!
//! Nothing here assumes the server knows a client's chat dimensions:
//! padding is only ever applied inside uniform-font cells, and every page
//! stays understandable if alignment or wrapping differs.

use pumpkin_util::text::{TextComponent, click::ClickEvent, color::NamedColor};

use super::{
    super::{
        config::LevelCurve,
        progression::{PlayerSnapshot, branch_mastery},
        skills::{BranchId, SkillId},
    },
    progress_percent,
};

/// Explicit-line budget per fallback page (vanilla unfocused chat view).
pub(crate) const MAX_CHAT_LINES: usize = 10;

const UNIFORM_FONT: &str = "minecraft:uniform";

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

fn branch_tag(branch: BranchId) -> &'static str {
    match branch {
        BranchId::Frontier => "F",
        BranchId::Warfare => "W",
        BranchId::Enterprise => "E",
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

/// Fixed-width summary cell: abbreviation plus right-aligned level, `off`
/// for a disabled skill, or `lvl*` for a maxed one.
fn summary_cell_text(
    skill: SkillId,
    snapshot: &PlayerSnapshot,
    curve: &LevelCurve,
    enabled: bool,
) -> String {
    let state = if !enabled {
        "off".to_string()
    } else {
        let level = snapshot.level_of(skill, curve);
        if level >= curve.max_level() {
            format!("{level}*")
        } else {
            level.to_string()
        }
    };
    format!("{} {:>4}", skill_abbr(skill), state)
}

fn uniform(text: String) -> TextComponent {
    TextComponent::text(text).font(UNIFORM_FONT.to_string())
}

/// Level-only three-column summary: one prose header plus nine rows (three
/// per branch), for 10 explicit lines. Only the table cells carry
/// `minecraft:uniform`; the header stays in the default font.
pub(crate) fn summary_lines(
    snapshot: &PlayerSnapshot,
    skill_info: &SkillInfo,
) -> Vec<TextComponent> {
    let mut lines = vec![
        TextComponent::text(
            "Your skills (* = max, off = disabled). Detail: /mmo stats chat <branch>",
        )
        .color_named(NamedColor::Gold),
    ];
    for branch in BranchId::ALL {
        for chunk in branch.skills().chunks(3) {
            let mut line = TextComponent::empty().add_child(
                uniform(format!("{}: ", branch_tag(*branch)))
                    .color_named(branch_color(*branch))
                    .bold(),
            );
            for (index, skill) in chunk.iter().enumerate() {
                if index > 0 {
                    line = line
                        .add_child(uniform(" | ".to_string()).color_named(NamedColor::DarkGray));
                }
                let (curve, enabled) = skill_info(*skill);
                line = line.add_child(
                    uniform(summary_cell_text(*skill, snapshot, &curve, enabled))
                        .color_named(branch_color(*branch)),
                );
            }
            lines.push(line);
        }
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
    use crate::mmo::{config::SkillConfig, progression::PlayerSkillSnapshot};

    /// Test curve thresholds: 0, 100, 300, 700, 1500 (max level 5).
    fn test_curve() -> LevelCurve {
        LevelCurve::new(&SkillConfig {
            max_level: 5,
            base_xp: 100,
            xp_multiplier: 2.0,
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
    fn summary_stays_within_line_budget_and_covers_every_skill() {
        let snapshot = PlayerSnapshot::default();
        let lines = summary_lines(&snapshot, &enabled_info());
        assert_eq!(lines.len(), 10); // header + 3 rows per branch
        assert!(lines.len() <= MAX_CHAT_LINES);
        let text = join_text(&lines);
        for skill in SkillId::ALL {
            assert!(
                text.contains(skill_abbr(*skill)),
                "summary is missing {skill}"
            );
        }
    }

    #[test]
    fn summary_marks_maxed_and_disabled_skills() {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Mining, PlayerSkillSnapshot::new(u64::MAX));
        let skill_info = |skill: SkillId| (test_curve(), skill != SkillId::Trading);
        let text = join_text(&summary_lines(&snapshot, &skill_info));
        assert!(text.contains("Mine   5*"), "max marker missing: {text}");
        assert!(
            text.contains("Trad  off"),
            "disabled marker missing: {text}"
        );
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
