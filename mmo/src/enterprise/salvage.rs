//! Salvage activity: grindstone experience bonus and material-recovery rolls.
//!
//! The experience bonus is previewed in the grindstone prepare event (while
//! the cooldown is ready); XP, the cooldown, and the material-recovery roll
//! happen only when the output is taken. Since the six-skill consolidation
//! both the bonus gate and the award use the shared Maintenance track.

use pumpkin::{
    entity::EntityBase,
    plugin::api::events::player::grindstone::{GrindstoneCompleteEvent, GrindstoneEvent},
};
use pumpkin_data::item_stack::ItemStack;
use rand::RngExt;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};
use super::config::SalvageConfig;

/// The shared skill track this activity awards and gates on: salvage and
/// repair both feed Maintenance since the six-skill consolidation.
pub(crate) const SKILL: SkillId = SkillId::Maintenance;

const SALVAGE_COOLDOWN_KEY: &str = "salvage.bonus";

/// Salvage experience bonus fraction: the per-level fraction clamped by the
/// cap, which gains a step per perk tier.
fn xp_bonus_fraction(salvage: &SalvageConfig, level: u32) -> f64 {
    (salvage.xp_bonus_per_level * f64::from(level))
        .min(salvage.xp_bonus_cap + salvage.xp_bonus_cap_per_tier * f64::from(perk_tier(level)))
}

/// Preview the Maintenance (salvage activity) experience bonus in the
/// grindstone prepare event.
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

    let level = perk_level(state, player_uuid, SKILL).await;
    let bonus_fraction = xp_bonus_fraction(salvage, level);
    if bonus_fraction <= 0.0 || event.experience <= 0 {
        return;
    }
    let bonus = (event.experience as f64 * bonus_fraction).round() as i32;
    event.experience = event.experience.saturating_add(bonus);
    state.mark_perk_preview(event.transaction.id, SALVAGE_COOLDOWN_KEY);
}

/// Award Maintenance XP, charge the cooldown, and roll material recovery
/// when the grindstone output is taken.
pub async fn handle_grindstone_complete(state: &MmoState, event: &GrindstoneCompleteEvent) {
    if !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let salvage = &config.enterprise.salvage;
    let player = &event.player;
    let player_uuid = player.gameprofile.id;
    let current_tick = state.current_tick();

    if config.perks.enabled && state.take_perk_preview(event.transaction.id, SALVAGE_COOLDOWN_KEY) {
        state.perk_cooldowns().try_activate(
            player_uuid,
            SALVAGE_COOLDOWN_KEY,
            current_tick,
            salvage.cooldown_ticks,
        );
    }

    progression::award_xp(state, player, SKILL, salvage.xp, XpSource::Salvage).await;
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

    #[test]
    fn xp_bonus_fraction_tier_zero_matches_pre_tier_value() {
        let salvage = SalvageConfig::default();
        // Below the cap the per-level fraction applies unchanged.
        assert!((xp_bonus_fraction(&salvage, 5) - 0.01).abs() < f64::EPSILON);
        // The old 0.25 cap still clamps at tier 0 (steep per-level value so
        // the cap is reached below level 10).
        let mut steep = salvage.clone();
        steep.xp_bonus_per_level = 0.1;
        assert!((xp_bonus_fraction(&steep, 9) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn xp_bonus_fraction_tier_four_raises_the_cap() {
        let salvage = SalvageConfig::default();
        // 0.002*300 = 0.6 → 0.25 + 0.05*4 = 0.45.
        assert!((xp_bonus_fraction(&salvage, 300) - 0.45).abs() < f64::EPSILON);
    }
}
