//! Protected native skill-summary menu.

use std::sync::Arc;

use pumpkin::{
    entity::player::Player,
    plugin::api::gui::{PluginGuiHandler, PluginGuiSpec},
};
use pumpkin_data::{item::Item, item_stack::ItemStack, screen::WindowType};
use pumpkin_util::text::TextComponent;

use super::super::{
    MmoState,
    skills::{BranchId, SkillId},
};

struct SkillMenuHandler;

impl PluginGuiHandler for SkillMenuHandler {}

fn branch_icon(branch: BranchId) -> &'static Item {
    let key = match branch {
        BranchId::Frontier => "grass_block",
        BranchId::Warfare => "iron_sword",
        BranchId::Enterprise => "emerald",
    };
    Item::from_registry_key(key).unwrap_or(&Item::STONE)
}

pub async fn open_skill_menu(state: &MmoState, player: Arc<Player>) -> Result<(), String> {
    let mut slots = vec![ItemStack::EMPTY.clone(); 27];
    for (slot, skill) in SkillId::ALL.iter().enumerate() {
        let progress = state.db().get_skill(player.gameprofile.id, *skill).await?;
        let curve = state.curve(*skill);
        let (level, into, needed) = curve.level_for_xp(progress.xp);
        let mut icon = ItemStack::new(1, branch_icon(skill.branch()));
        icon.set_custom_name(format!(
            "{} — Level {} ({}/{})",
            skill.display_name(),
            level,
            into,
            needed
        ));
        slots[slot] = icon;
    }

    state
        .context()
        .open_plugin_gui(
            player,
            PluginGuiSpec {
                window_type: WindowType::Generic9x3,
                title: TextComponent::text("Cabbage MMO Skills"),
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
