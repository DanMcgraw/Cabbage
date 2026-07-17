//! Salvage: grindstone experience bonus and material-recovery rolls.
//!
//! The experience bonus is previewed in the grindstone prepare event (while
//! the cooldown is ready); XP, the cooldown, and the material-recovery roll
//! happen only when the output is taken.

use pumpkin::{
    entity::EntityBase,
    plugin::api::events::player::grindstone::{GrindstoneEvent, GrindstoneTakeEvent},
};
use pumpkin_data::item_stack::ItemStack;
use rand::Rng;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

const SALVAGE_COOLDOWN_KEY: &str = "salvage.bonus";

/// Preview the Salvage experience bonus in the grindstone prepare event.
pub async fn handle_grindstone(state: &MmoState, event: &mut GrindstoneEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    if !config.perks.enabled {
        return;
    }
    let salvage = &config.enterprise.salvage;
    let player_uuid = event.player.gameprofile.id;
    if state.perk_cooldowns().remaining_ticks(
        player_uuid,
        SALVAGE_COOLDOWN_KEY,
        state.current_tick(),
    ) > 0
    {
        return;
    }

    let level = player_level(state, &event.player).await;
    let bonus_fraction = (salvage.xp_bonus_per_level * level as f64).min(salvage.xp_bonus_cap);
    if bonus_fraction <= 0.0 || event.experience <= 0 {
        return;
    }
    let bonus = (event.experience as f64 * bonus_fraction).round() as i32;
    event.experience = event.experience.saturating_add(bonus);
}

/// Award Salvage XP, charge the cooldown, and roll material recovery when
/// the grindstone output is taken.
pub async fn handle_grindstone_take(state: &MmoState, event: &GrindstoneTakeEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let salvage = &config.enterprise.salvage;
    let player = &event.player;
    let player_uuid = player.gameprofile.id;
    let current_tick = state.current_tick();

    if config.perks.enabled {
        state.perk_cooldowns().try_activate(
            player_uuid,
            SALVAGE_COOLDOWN_KEY,
            current_tick,
            salvage.cooldown_ticks,
        );
    }

    progression::award_xp(
        state,
        player,
        SkillId::Salvage,
        salvage.xp,
        XpSource::Salvage,
    )
    .await;
    state.audit(&format!(
        "grindstone commit: {} took output for {} experience",
        player.gameprofile.id, event.experience
    ));

    // Material-recovery roll: one material item of the input's tool tier,
    // dropped at the player. The grindstone transaction itself is untouched.
    if !config.perks.enabled {
        return;
    }
    let chance = salvage.recovery_chance.min(config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }
    let Some(material) = recovery_material(&event.input_top, &salvage.recovery_materials) else {
        return;
    };
    let Some(item) = pumpkin_data::item::Item::from_registry_key(material) else {
        return;
    };
    let world = player.get_entity().world.load_full();
    let pos = player.get_entity().block_pos.load();
    state.audit(&format!(
        "quality roll: salvage recovered {} for {}",
        material, player_uuid
    ));
    world.drop_stack(&pos, ItemStack::new(1, item)).await;
}

/// Find the recovery material for an input item by its tool-tier prefix.
fn recovery_material<'a>(
    input: &ItemStack,
    materials: &'a std::collections::HashMap<String, String>,
) -> Option<&'a str> {
    let key = input.item.registry_key;
    materials
        .iter()
        .find(|(tier, _)| key.starts_with(tier.as_str()))
        .map(|(_, item)| item.as_str())
}

async fn player_level(state: &MmoState, player: &pumpkin::entity::player::Player) -> u32 {
    let curve = state.curve(SkillId::Salvage);
    state
        .db()
        .get_skill(player.gameprofile.id, SkillId::Salvage)
        .await
        .map(|data| curve.level_for_xp(data.xp).0)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn recovery_material_matches_tier_prefix() {
        let mut materials = HashMap::new();
        materials.insert("iron".to_string(), "iron_ingot".to_string());
        materials.insert("diamond".to_string(), "diamond".to_string());

        let pickaxe = pumpkin_data::item::Item::from_registry_key("iron_pickaxe").unwrap();
        let stack = ItemStack::new(1, pickaxe);
        assert_eq!(recovery_material(&stack, &materials), Some("iron_ingot"));

        let stick = pumpkin_data::item::Item::from_registry_key("stick").unwrap();
        let stack = ItemStack::new(1, stick);
        assert_eq!(recovery_material(&stack, &materials), None);
    }
}
