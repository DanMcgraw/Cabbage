//! Protected native 9x3 skill-summary menu.
//!
//! Layout: one branch per row, the branch summary in the row's first slot,
//! member skills in canonical order, a Help icon in the last Warfare-row
//! slot, and inert filler panes in the remaining row-end slots. The menu is
//! read-only (`allow_grab_items` and `allow_put_items` stay false) and every
//! click is cancelled; the only interactions are the Help slot (closes the
//! GUI and prints the command list) and the skill slots (close the GUI and
//! send that skill's `/mmo skill` detail page for the menu's target).
//!
use std::sync::Arc;

use pumpkin::{
    entity::player::Player,
    plugin::{
        BoxFuture,
        api::gui::{
            PluginGuiClickContext, PluginGuiCloseReason, PluginGuiHandler, PluginGuiInputResult,
            PluginGuiSpec,
        },
    },
};
use pumpkin_data::{item::Item, item_stack::ItemStack, screen::WindowType};
use pumpkin_util::{
    permission::PermissionLvl,
    text::{TextComponent, color::NamedColor},
};

use super::{
    super::{
        MmoState,
        config::LevelCurve,
        progression::{PlayerSkillSnapshot, PlayerSnapshot, branch_mastery, fetch_snapshot},
        skills::{BranchId, SkillId},
    },
    progress_percent, skill_detail, skill_enabled,
};

/// Number of container slots in a `Generic9x3` window.
pub(crate) const MENU_SLOTS: usize = 27;
/// First slot of each branch row, holding the branch summary icon.
pub(crate) const BRANCH_HEADER_SLOTS: [usize; 3] = [0, 9, 18];
/// Slot holding the Help icon (end of the Warfare row).
pub(crate) const HELP_SLOT: usize = 17;
/// Slots holding inert filler panes (row ends without a skill or Help).
pub(crate) const FILLER_SLOTS: [usize; 5] = [7, 8, 16, 25, 26];

/// What one menu slot displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuSlot {
    BranchHeader(BranchId),
    Skill(SkillId),
    /// Visually inert padding; the read-only window flags and the click
    /// handler's global cancel keep it from ever being taken or used.
    Filler,
    Help,
}

/// The fixed 27-slot assignment: one branch per row in canonical order,
/// branch header first, Help at [`HELP_SLOT`], and every remaining
/// non-skill slot an inert filler.
pub(crate) fn menu_layout() -> [MenuSlot; MENU_SLOTS] {
    let mut layout = [MenuSlot::Filler; MENU_SLOTS];
    for (branch, row_start) in BranchId::ALL.iter().zip(BRANCH_HEADER_SLOTS) {
        layout[row_start] = MenuSlot::BranchHeader(*branch);
        for (column, skill) in branch.skills().iter().enumerate() {
            layout[row_start + 1 + column] = MenuSlot::Skill(*skill);
        }
    }
    layout[HELP_SLOT] = MenuSlot::Help;
    debug_assert!(
        FILLER_SLOTS
            .iter()
            .all(|slot| layout[*slot] == MenuSlot::Filler)
    );
    layout
}

/// What one menu slot displays at `slot`, or `None` outside the window.
/// Click handling resolves actions through this server-owned mapping, never
/// through the clicked item stack.
fn menu_slot_at(slot: usize) -> Option<MenuSlot> {
    menu_layout().get(slot).copied()
}

fn branch_icon(branch: BranchId) -> &'static Item {
    let key = match branch {
        BranchId::Frontier => "grass_block",
        BranchId::Warfare => "iron_sword",
        BranchId::Enterprise => "emerald",
    };
    Item::from_registry_key(key).unwrap_or(&Item::STONE)
}

/// Deterministic skill-to-icon mapping; unknown keys fall back to stone.
fn skill_icon(skill: SkillId) -> &'static Item {
    let key = match skill {
        SkillId::Cultivation => "wheat",
        SkillId::Woodcutting => "oak_log",
        SkillId::Mining => "iron_pickaxe",
        SkillId::Excavation => "iron_shovel",
        SkillId::Fishing => "fishing_rod",
        SkillId::AnimalHandling => "bone",
        SkillId::Blades => "diamond_sword",
        SkillId::Axes => "diamond_axe",
        SkillId::Archery => "bow",
        SkillId::Athletics => "feather",
        SkillId::Defense => "shield",
        SkillId::Sorcery => "blaze_rod",
        SkillId::Smithing => "anvil",
        SkillId::Maintenance => "iron_ingot",
        SkillId::Alchemy => "brewing_stand",
        SkillId::Enchanting => "enchanting_table",
        SkillId::Tinkering => "piston",
        SkillId::Commerce => "emerald",
    };
    Item::from_registry_key(key).unwrap_or(&Item::STONE)
}

/// Inert padding icon for slots that are neither headers, skills, nor Help.
fn filler_icon() -> &'static Item {
    Item::from_registry_key("gray_stained_glass_pane").unwrap_or(&Item::STONE)
}

fn help_icon() -> &'static Item {
    Item::from_registry_key("book").unwrap_or(&Item::STONE)
}

/// Concise custom name for a skill icon. Detailed progress belongs in lore so
/// the inventory grid stays readable without flattening everything into one line.
fn skill_display_name(skill: SkillId, progress: PlayerSkillSnapshot, curve: &LevelCurve) -> String {
    let (level, _, _) = curve.level_for_xp(progress.xp);
    format!("{} - Level {}", skill.display_name(), level)
}

/// One-line explanation of the disciplines a merged skill covers, shown at
/// the top of its lore. Unmerged skills return `None`.
fn merged_discipline_line(skill: SkillId) -> Option<&'static str> {
    let line = match skill {
        SkillId::Cultivation => "Covers crops and herbalism",
        SkillId::AnimalHandling => "Covers husbandry and taming",
        SkillId::Athletics => "Covers unarmed combat and acrobatics",
        SkillId::Maintenance => "Covers repair and salvage",
        SkillId::Commerce => "Covers trading and charisma",
        _ => return None,
    };
    Some(line)
}

/// Structured lore for a skill icon: merged-discipline explanation, state,
/// progress toward the next level, and total accumulated XP.
fn skill_lore(
    skill: SkillId,
    progress: PlayerSkillSnapshot,
    curve: &LevelCurve,
    enabled: bool,
) -> Vec<TextComponent> {
    let (level, into, needed) = curve.level_for_xp(progress.xp);
    let mut lore = Vec::with_capacity(4);
    if let Some(line) = merged_discipline_line(skill) {
        lore.push(TextComponent::text(line).color_named(NamedColor::DarkGray));
    }
    if !enabled {
        lore.push(TextComponent::text("Disabled").color_named(NamedColor::Red));
    }
    if level >= curve.max_level() {
        if enabled {
            lore.push(TextComponent::text("Max level").color_named(NamedColor::Gold));
        }
    } else {
        lore.push(
            TextComponent::text(format!(
                "Progress: {into}/{needed} XP ({}%)",
                progress_percent(into, needed)
            ))
            .color_named(NamedColor::Yellow),
        );
    }
    lore.push(
        TextComponent::text(format!("Total XP: {}", progress.xp)).color_named(NamedColor::Gray),
    );
    lore
}

/// Structured branch summary shown beneath the branch name.
fn branch_lore(
    branch: BranchId,
    snapshot: &PlayerSnapshot,
    skill_info: &dyn Fn(SkillId) -> (LevelCurve, bool),
) -> Vec<TextComponent> {
    let mastery = branch_mastery(branch, |skill| {
        let (curve, _) = skill_info(skill);
        snapshot.level_of(skill, &curve)
    });
    let enabled = branch
        .skills()
        .iter()
        .filter(|skill| skill_info(**skill).1)
        .count();
    vec![
        TextComponent::text(format!("Mastery: {mastery:.1}")).color_named(NamedColor::Gold),
        TextComponent::text(format!(
            "Enabled skills: {enabled}/{}",
            branch.skills().len()
        ))
        .color_named(NamedColor::Gray),
    ]
}

/// Owned click handler for the skill menu. Every click is cancelled so the
/// menu stays read-only; the Help slot closes the GUI and sends the compact
/// help page, and a skill slot closes the GUI and sends that skill's detail
/// page for the menu's target. The action identity comes from the
/// server-owned slot mapping and the live session, never from the clicked
/// item stack.
struct SkillMenuHandler {
    state: Arc<MmoState>,
    /// The player whose skills the menu shows (usually the viewer).
    target: uuid::Uuid,
}

impl PluginGuiHandler for SkillMenuHandler {
    fn on_click(&self, context: PluginGuiClickContext) -> BoxFuture<'_, PluginGuiInputResult> {
        Box::pin(async move {
            if !context.is_container_slot
                || context.player.plugin_gui_session().await != Some(context.session_id)
            {
                return PluginGuiInputResult::Cancel;
            }
            match usize::try_from(context.slot).ok().and_then(menu_slot_at) {
                Some(MenuSlot::Help) => {
                    context
                        .player
                        .close_plugin_gui(PluginGuiCloseReason::PluginRequested)
                        .await;
                    let admin = matches!(
                        context.player.permission_lvl.load(),
                        PermissionLvl::Three | PermissionLvl::Four
                    );
                    let message = super::super::commands::help_message(admin, 1);
                    context.player.send_system_message(&message).await;
                }
                Some(MenuSlot::Skill(skill)) => {
                    context
                        .player
                        .close_plugin_gui(PluginGuiCloseReason::PluginRequested)
                        .await;
                    match fetch_snapshot(&self.state, self.target).await {
                        Ok(snapshot) => {
                            let config = self.state.config();
                            let curve = self.state.curve(skill);
                            for line in skill_detail::skill_detail_lines(
                                skill, &snapshot, &curve, &config, 1,
                            ) {
                                context.player.send_system_message(&line).await;
                            }
                        }
                        Err(error) => {
                            context
                                .player
                                .send_system_message(
                                    &TextComponent::text(format!(
                                        "Failed to fetch skills: {error}"
                                    ))
                                    .color_named(NamedColor::Red),
                                )
                                .await;
                        }
                    }
                }
                // Branch headers and fillers stay inert.
                _ => {}
            }
            PluginGuiInputResult::Cancel
        })
    }
}

/// Build the 27 icon stacks for the fixed layout against one snapshot.
fn menu_slots(
    snapshot: &PlayerSnapshot,
    skill_info: &dyn Fn(SkillId) -> (LevelCurve, bool),
) -> Vec<ItemStack> {
    menu_layout()
        .iter()
        .map(|menu_slot| {
            let (icon, name, lore) = match menu_slot {
                MenuSlot::BranchHeader(branch) => (
                    branch_icon(*branch),
                    branch.display_name().to_string(),
                    branch_lore(*branch, snapshot, skill_info),
                ),
                MenuSlot::Skill(skill) => {
                    let (curve, enabled) = skill_info(*skill);
                    let progress = snapshot.get(*skill);
                    (
                        skill_icon(*skill),
                        skill_display_name(*skill, progress, &curve),
                        skill_lore(*skill, progress, &curve, enabled),
                    )
                }
                MenuSlot::Filler => (filler_icon(), " ".to_string(), Vec::new()),
                MenuSlot::Help => (
                    help_icon(),
                    "Help".to_string(),
                    vec![
                        TextComponent::text("Click for the /mmo command list")
                            .color_named(NamedColor::Yellow),
                    ],
                ),
            };
            let mut stack = ItemStack::new(1, icon);
            stack.set_custom_name(name);
            stack.set_lore(lore);
            stack
        })
        .collect()
}

/// Menu window title: first person when viewers open their own skills,
/// possessive when viewing another player.
fn menu_title(viewer_is_target: bool, target_name: &str) -> String {
    if viewer_is_target {
        "Your MMO Skills".to_string()
    } else {
        format!("{target_name}'s MMO Skills")
    }
}

/// Assemble the read-only window specification. Both transfer flags stay
/// false so no click, drag, or shift-click path can move items; the click
/// handler additionally cancels every input after resolving the Help and
/// skill actions.
fn menu_spec(title: String, slots: Vec<ItemStack>) -> PluginGuiSpec {
    PluginGuiSpec {
        window_type: WindowType::Generic9x3,
        title: TextComponent::text(title),
        slots,
        allow_grab_items: false,
        allow_put_items: false,
    }
}

/// Open the read-only skill grid for `viewer`, showing `target`'s skills.
pub(crate) async fn open_skill_menu(
    state: &Arc<MmoState>,
    viewer: Arc<Player>,
    target: &Player,
) -> Result<(), String> {
    let snapshot = fetch_snapshot(state, target.gameprofile.id).await?;
    let config = state.config();
    let skill_info = |skill: SkillId| (state.curve(skill), skill_enabled(&config, skill));

    let slots = menu_slots(&snapshot, &skill_info);
    let title = menu_title(
        viewer.gameprofile.id == target.gameprofile.id,
        &target.gameprofile.name,
    );

    state
        .context()
        .open_plugin_gui(
            viewer,
            menu_spec(title, slots),
            Arc::new(SkillMenuHandler {
                state: state.clone(),
                target: target.gameprofile.id,
            }),
        )
        .await
        .map(|_| ())
        .map_err(|error| format!("failed to open MMO skill menu: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SkillConfig;

    fn test_curve() -> LevelCurve {
        LevelCurve::new(&SkillConfig {
            max_level: 5,
            base_xp: 100,
            xp_multiplier: 2.0,
            enabled: true,
        })
    }

    #[test]
    fn every_skill_appears_exactly_once() {
        let layout = menu_layout();
        for skill in SkillId::ALL {
            let count = layout
                .iter()
                .filter(|slot| **slot == MenuSlot::Skill(*skill))
                .count();
            assert_eq!(count, 1, "{skill} should appear exactly once");
        }
    }

    #[test]
    fn layout_covers_all_slots_without_collisions() {
        let layout = menu_layout();
        let headers = layout
            .iter()
            .filter(|slot| matches!(slot, MenuSlot::BranchHeader(_)))
            .count();
        let skills = layout
            .iter()
            .filter(|slot| matches!(slot, MenuSlot::Skill(_)))
            .count();
        let help = layout
            .iter()
            .filter(|slot| matches!(slot, MenuSlot::Help))
            .count();
        let fillers = layout
            .iter()
            .filter(|slot| matches!(slot, MenuSlot::Filler))
            .count();
        // 3 headers + 18 skills + 1 interactive Help slot + 5 inert fillers.
        assert_eq!((headers, skills, help, fillers), (3, 18, 1, 5));
        assert_eq!(headers + skills + help + fillers, MENU_SLOTS);
    }

    #[test]
    fn branch_headers_help_and_fillers_occupy_reserved_slots() {
        let layout = menu_layout();
        for (index, branch) in BranchId::ALL.iter().enumerate() {
            assert_eq!(BRANCH_HEADER_SLOTS[index], index * 9);
            assert_eq!(
                layout[BRANCH_HEADER_SLOTS[index]],
                MenuSlot::BranchHeader(*branch)
            );
        }
        assert_eq!(HELP_SLOT, 17);
        assert_eq!(layout[HELP_SLOT], MenuSlot::Help);
        assert_eq!(FILLER_SLOTS, [7, 8, 16, 25, 26]);
        for slot in FILLER_SLOTS {
            assert_eq!(layout[slot], MenuSlot::Filler, "slot {slot} must be filler");
        }
    }

    #[test]
    fn skills_sit_in_canonical_order_after_each_branch_header() {
        let layout = menu_layout();
        for (branch, row_start) in BranchId::ALL.iter().zip(BRANCH_HEADER_SLOTS) {
            for (column, skill) in branch.skills().iter().enumerate() {
                assert_eq!(
                    layout[row_start + 1 + column],
                    MenuSlot::Skill(*skill),
                    "{skill} must sit at column {column} of the {branch} row"
                );
            }
        }
    }

    #[test]
    fn click_slots_resolve_through_the_layout() {
        for (branch, row_start) in BranchId::ALL.iter().zip(BRANCH_HEADER_SLOTS) {
            assert_eq!(
                menu_slot_at(row_start),
                Some(MenuSlot::BranchHeader(*branch))
            );
            for (column, skill) in branch.skills().iter().enumerate() {
                assert_eq!(
                    menu_slot_at(row_start + 1 + column),
                    Some(MenuSlot::Skill(*skill)),
                    "slot {} must resolve to {skill}",
                    row_start + 1 + column
                );
            }
        }
        assert_eq!(menu_slot_at(HELP_SLOT), Some(MenuSlot::Help));
        for slot in FILLER_SLOTS {
            assert_eq!(menu_slot_at(slot), Some(MenuSlot::Filler));
        }
        assert_eq!(menu_slot_at(MENU_SLOTS), None);
    }

    fn lore_text(lines: &[TextComponent]) -> Vec<String> {
        lines.iter().cloned().map(TextComponent::get_text).collect()
    }

    #[test]
    fn merged_skills_explain_their_disciplines_in_lore() {
        let expected = [
            (SkillId::Cultivation, "Covers crops and herbalism"),
            (SkillId::AnimalHandling, "Covers husbandry and taming"),
            (SkillId::Athletics, "Covers unarmed combat and acrobatics"),
            (SkillId::Maintenance, "Covers repair and salvage"),
            (SkillId::Commerce, "Covers trading and charisma"),
        ];
        let progress = PlayerSkillSnapshot::new(0);
        for (skill, line) in expected {
            let lore = skill_lore(skill, progress, &test_curve(), true);
            assert_eq!(lore_text(&lore)[0], line, "{skill} lore");
            assert_eq!(
                lore[0].0.style.color,
                Some(pumpkin_util::text::color::Color::Named(
                    NamedColor::DarkGray
                ))
            );
        }
        // Unmerged skills keep the plain state/progress lore.
        let lore = skill_lore(SkillId::Mining, progress, &test_curve(), true);
        assert!(
            !lore_text(&lore)
                .iter()
                .any(|line| line.starts_with("Covers"))
        );
    }

    #[test]
    fn merged_skills_have_the_planned_distinct_icons() {
        let expected = [
            (SkillId::Cultivation, "wheat"),
            (SkillId::AnimalHandling, "bone"),
            (SkillId::Athletics, "feather"),
            (SkillId::Maintenance, "iron_ingot"),
            (SkillId::Commerce, "emerald"),
        ];
        for (skill, key) in expected {
            assert_eq!(skill_icon(skill).registry_key, key, "{skill} icon");
        }
        // No two skill slots share an icon.
        let mut keys: Vec<&str> = SkillId::ALL
            .iter()
            .map(|skill| skill_icon(*skill).registry_key)
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SkillId::ALL.len());
    }

    #[test]
    fn disabled_skill_has_concise_name_and_explicit_lore() {
        let progress = PlayerSkillSnapshot::new(0);
        let name = skill_display_name(SkillId::Commerce, progress, &test_curve());
        let lore = skill_lore(SkillId::Commerce, progress, &test_curve(), false);
        assert_eq!(name, "Commerce - Level 1");
        assert_eq!(
            lore_text(&lore),
            [
                "Covers trading and charisma",
                "Disabled",
                "Progress: 0/100 XP (0%)",
                "Total XP: 0"
            ]
        );
        assert_eq!(
            lore[1].0.style.color,
            Some(pumpkin_util::text::color::Color::Named(NamedColor::Red))
        );
    }

    #[test]
    fn max_level_state_moves_from_name_into_lore() {
        let progress = PlayerSkillSnapshot::new(u64::MAX);
        let name = skill_display_name(SkillId::Mining, progress, &test_curve());
        let lore = skill_lore(SkillId::Mining, progress, &test_curve(), true);
        assert_eq!(name, "Mining - Level 5");
        assert_eq!(
            lore_text(&lore),
            ["Max level", "Total XP: 18446744073709551615"]
        );
    }

    #[test]
    fn skill_lore_shows_progress_percent_and_total_xp() {
        // Test curve thresholds: 0, 100, 300, 700, 1500 (max level 5).
        let progress = PlayerSkillSnapshot::new(150);
        let name = skill_display_name(SkillId::Mining, progress, &test_curve());
        let lore = skill_lore(SkillId::Mining, progress, &test_curve(), true);
        assert_eq!(name, "Mining - Level 2");
        assert_eq!(
            lore_text(&lore),
            ["Progress: 50/200 XP (25%)", "Total XP: 150"]
        );
    }

    #[test]
    fn branch_lore_shows_mastery_and_enabled_count() {
        let curve = test_curve();
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(SkillId::Blades, PlayerSkillSnapshot::new(150)); // level 2
        let skill_info = |_: SkillId| (curve.clone(), true);
        // Warfare: levels 2,1,1,1,1,1 -> mastery 7/6 ~= 1.2.
        let lore = branch_lore(BranchId::Warfare, &snapshot, &skill_info);
        assert_eq!(lore_text(&lore), ["Mastery: 1.2", "Enabled skills: 6/6"]);
    }

    #[test]
    fn menu_slots_fill_every_slot_and_render_fillers_inertly() {
        let snapshot = PlayerSnapshot::default();
        let skill_info = |_: SkillId| (test_curve(), true);
        let slots = menu_slots(&snapshot, &skill_info);
        assert_eq!(slots.len(), MENU_SLOTS);
        for (index, stack) in slots.iter().enumerate() {
            assert_ne!(
                stack.item.registry_key, "air",
                "slot {index} must be filled"
            );
        }
        for slot in FILLER_SLOTS {
            let filler = &slots[slot];
            assert_eq!(filler.item.registry_key, "gray_stained_glass_pane");
            assert!(
                filler.get_lore().is_none(),
                "filler slot {slot} must not carry lore"
            );
        }
        // Spot-check the other slot kinds: branch header, skill, Help.
        assert_eq!(slots[0].item.registry_key, "grass_block");
        let mining = &slots[3]; // Frontier header at 0, Mining third skill.
        assert_eq!(mining.item.registry_key, "iron_pickaxe");
        assert!(mining.get_lore().is_some_and(|lore| !lore.is_empty()));
        assert_eq!(slots[HELP_SLOT].item.registry_key, "book");
    }

    #[test]
    fn menu_spec_stays_a_read_only_9x3_window() {
        let spec = menu_spec(
            "Title".to_string(),
            vec![ItemStack::EMPTY.clone(); MENU_SLOTS],
        );
        assert!(matches!(spec.window_type, WindowType::Generic9x3));
        assert!(!spec.allow_grab_items);
        assert!(!spec.allow_put_items);
        assert_eq!(spec.slots.len(), MENU_SLOTS);
    }

    #[test]
    fn menu_title_distinguishes_viewer_and_target() {
        assert_eq!(menu_title(true, "Alex"), "Your MMO Skills");
        assert_eq!(menu_title(false, "Alex"), "Alex's MMO Skills");
    }
}
