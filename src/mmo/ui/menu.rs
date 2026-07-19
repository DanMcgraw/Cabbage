//! Protected native 9x3 skill-summary menu.
//!
//! Layout: one branch per row, the branch summary in the row's first slot,
//! member skills in canonical order, and a Help icon in the last Warfare-row
//! slot. The menu is read-only (`allow_grab_items` and `allow_put_items`
//! stay false); the only interaction is the Help slot's click callback.
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
    progress_percent, skill_enabled,
};

/// Number of container slots in a `Generic9x3` window.
pub(crate) const MENU_SLOTS: usize = 27;
/// First slot of each branch row, holding the branch summary icon.
pub(crate) const BRANCH_HEADER_SLOTS: [usize; 3] = [0, 9, 18];
/// Slot holding the Help icon (end of the Warfare row).
pub(crate) const HELP_SLOT: usize = 17;

/// What one menu slot displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuSlot {
    BranchHeader(BranchId),
    Skill(SkillId),
    Help,
}

/// The fixed 27-slot assignment: one branch per row in canonical order,
/// branch header first, Help at [`HELP_SLOT`].
pub(crate) fn menu_layout() -> [MenuSlot; MENU_SLOTS] {
    let mut layout = [MenuSlot::Help; MENU_SLOTS];
    for (branch, row_start) in BranchId::ALL.iter().zip(BRANCH_HEADER_SLOTS) {
        layout[row_start] = MenuSlot::BranchHeader(*branch);
        for (column, skill) in branch.skills().iter().enumerate() {
            layout[row_start + 1 + column] = MenuSlot::Skill(*skill);
        }
    }
    layout[HELP_SLOT] = MenuSlot::Help;
    layout
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
        SkillId::Agriculture => "wheat",
        SkillId::Herbalism => "sweet_berries",
        SkillId::Woodcutting => "oak_log",
        SkillId::Mining => "iron_pickaxe",
        SkillId::Excavation => "iron_shovel",
        SkillId::Fishing => "fishing_rod",
        SkillId::Husbandry => "egg",
        SkillId::Taming => "bone",
        SkillId::Blades => "diamond_sword",
        SkillId::Axes => "diamond_axe",
        SkillId::Archery => "bow",
        SkillId::Unarmed => "stick",
        SkillId::Defense => "shield",
        SkillId::Acrobatics => "feather",
        SkillId::Sorcery => "blaze_rod",
        SkillId::Smithing => "anvil",
        SkillId::Repair => "iron_ingot",
        SkillId::Salvage => "grindstone",
        SkillId::Alchemy => "brewing_stand",
        SkillId::Enchanting => "enchanting_table",
        SkillId::Tinkering => "piston",
        SkillId::Trading => "gold_ingot",
        SkillId::Charisma => "name_tag",
    };
    Item::from_registry_key(key).unwrap_or(&Item::STONE)
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

/// Structured lore for a skill icon: state, progress toward the next level,
/// and total accumulated XP.
fn skill_lore(
    progress: PlayerSkillSnapshot,
    curve: &LevelCurve,
    enabled: bool,
) -> Vec<TextComponent> {
    let (level, into, needed) = curve.level_for_xp(progress.xp);
    let mut lore = Vec::with_capacity(3);
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
/// menu stays read-only; the Help slot additionally closes the GUI and sends
/// the compact help page. The action identity comes from the server-owned
/// slot mapping and the live session, never from the clicked item stack.
struct SkillMenuHandler;

impl PluginGuiHandler for SkillMenuHandler {
    fn on_click(&self, context: PluginGuiClickContext) -> BoxFuture<'_, PluginGuiInputResult> {
        Box::pin(async move {
            if context.is_container_slot
                && context.slot == HELP_SLOT as i16
                && context.player.plugin_gui_session().await == Some(context.session_id)
            {
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
            PluginGuiInputResult::Cancel
        })
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

    let layout = menu_layout();
    let mut slots = vec![ItemStack::EMPTY.clone(); MENU_SLOTS];
    for (index, menu_slot) in layout.iter().enumerate() {
        let (icon, name, lore) = match menu_slot {
            MenuSlot::BranchHeader(branch) => (
                branch_icon(*branch),
                branch.display_name().to_string(),
                branch_lore(*branch, &snapshot, &skill_info),
            ),
            MenuSlot::Skill(skill) => {
                let (curve, enabled) = skill_info(*skill);
                let progress = snapshot.get(*skill);
                (
                    skill_icon(*skill),
                    skill_display_name(*skill, progress, &curve),
                    skill_lore(progress, &curve, enabled),
                )
            }
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
        slots[index] = stack;
    }

    let title = if viewer.gameprofile.id == target.gameprofile.id {
        "Your MMO Skills".to_string()
    } else {
        format!("{}'s MMO Skills", target.gameprofile.name)
    };

    state
        .context()
        .open_plugin_gui(
            viewer,
            PluginGuiSpec {
                window_type: WindowType::Generic9x3,
                title: TextComponent::text(title),
                slots,
                allow_grab_items: false,
                allow_put_items: false,
            },
            Arc::new(SkillMenuHandler),
        )
        .await
        .map(|_| ())
        .map_err(|error| format!("failed to open MMO skill menu: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mmo::config::SkillConfig;

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
        assert_eq!((headers, skills, help), (3, 23, 1));
        assert_eq!(headers + skills + help, MENU_SLOTS);
    }

    #[test]
    fn branch_headers_and_help_occupy_reserved_slots() {
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
    }

    #[test]
    fn each_skill_sits_in_its_branch_row() {
        let layout = menu_layout();
        for (index, slot) in layout.iter().enumerate() {
            if let MenuSlot::Skill(skill) = slot {
                let row = BranchId::ALL
                    .iter()
                    .position(|branch| *branch == skill.branch())
                    .expect("every skill has a branch");
                assert_eq!(index / 9, row, "{skill} sits outside its branch row");
            }
        }
    }

    fn lore_text(lines: &[TextComponent]) -> Vec<String> {
        lines.iter().cloned().map(TextComponent::get_text).collect()
    }

    #[test]
    fn disabled_skill_has_concise_name_and_explicit_lore() {
        let progress = PlayerSkillSnapshot::new(0);
        let name = skill_display_name(SkillId::Trading, progress, &test_curve());
        let lore = skill_lore(progress, &test_curve(), false);
        assert_eq!(name, "Trading - Level 1");
        assert_eq!(
            lore_text(&lore),
            ["Disabled", "Progress: 0/100 XP (0%)", "Total XP: 0"]
        );
        assert_eq!(
            lore[0].0.style.color,
            Some(pumpkin_util::text::color::Color::Named(NamedColor::Red))
        );
    }

    #[test]
    fn max_level_state_moves_from_name_into_lore() {
        let progress = PlayerSkillSnapshot::new(u64::MAX);
        let name = skill_display_name(SkillId::Mining, progress, &test_curve());
        let lore = skill_lore(progress, &test_curve(), true);
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
        let lore = skill_lore(progress, &test_curve(), true);
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
        // Warfare: levels 2,1,1,1,1,1,1 -> mastery 8/7 ~= 1.1.
        let lore = branch_lore(BranchId::Warfare, &snapshot, &skill_info);
        assert_eq!(lore_text(&lore), ["Mastery: 1.1", "Enabled skills: 7/7"]);
    }
}
