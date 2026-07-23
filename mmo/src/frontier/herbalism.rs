//! Herbalism activity: plant/forage XP, quality yield, and consumable-healing
//! bonuses.
//!
//! XP rule: one award per broken natural plant or per eaten configured
//! consumable. Since the six-skill consolidation the awards feed Cultivation,
//! shared with the agriculture activity. Tracked plants are
//! provenance-marked on placement, so player-placed plants earn nothing.

use pumpkin::plugin::api::events::{
    block::block_broken::BlockBrokenEvent,
    player::player_item_use_complete::PlayerItemUseCompleteEvent,
};
use pumpkin_data::item_stack::ItemStack;
use rand::RngExt;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};

/// The shared skill track this activity awards: herbalism and agriculture
/// both feed Cultivation since the six-skill consolidation.
pub(crate) const SKILL: SkillId = SkillId::Cultivation;

/// Quality yield chance: the base chance plus the per-tier step, clamped by
/// the global proc-chance cap.
fn quality_yield_chance(
    herbalism: &super::config::HerbalismConfig,
    level: u32,
    global_cap: f64,
) -> f64 {
    (herbalism.quality_yield_chance
        + herbalism.quality_yield_chance_per_tier * f64::from(perk_tier(level)))
    .min(global_cap)
}

/// Bonus health restored by configured consumables (2.0 = one heart): the
/// base bonus plus the per-tier step.
fn consumable_heal_bonus(herbalism: &super::config::HerbalismConfig, level: u32) -> f32 {
    herbalism.consumable_heal_bonus
        + herbalism.consumable_heal_bonus_per_tier * perk_tier(level) as f32
}

/// Award Cultivation XP for a broken natural plant, then roll quality yield.
pub async fn handle_block_broken(
    state: &MmoState,
    event: &BlockBrokenEvent,
    was_non_natural: bool,
) {
    let Some(player) = event.player.as_ref() else {
        return;
    };
    if was_non_natural || !earns_xp(player) {
        return;
    }
    let config = state.config();
    let herbalism = &config.frontier.herbalism;
    let Some(xp) = herbalism.plant_xp.get(event.block.name).copied() else {
        return;
    };

    progression::award_xp(state, player, SKILL, xp, XpSource::Forage).await;

    // Quality yield: chance for one bonus item of the broken plant.
    if !config.perks.enabled {
        return;
    }
    let level = perk_level(state, player.gameprofile.id, SKILL).await;
    let chance = quality_yield_chance(herbalism, level, config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }
    if let Some(item) = pumpkin_data::item::Item::from_registry_key(event.block.name) {
        event
            .world
            .drop_stack(&event.block_position, ItemStack::new(1, item))
            .await;
    }
}

/// Award Cultivation XP and a bounded healing bonus for eating configured
/// plant-based consumables.
pub async fn handle_item_use_complete(state: &MmoState, event: &PlayerItemUseCompleteEvent) {
    let player = &event.player;
    if !earns_xp(player) || event.consumed_count == 0 {
        return;
    }
    let config = state.config();
    let herbalism = &config.frontier.herbalism;
    let key = event.item_before.item.registry_key;
    let Some(xp) = herbalism.consumable_xp.get(key).copied() else {
        return;
    };

    progression::award_xp(state, player, SKILL, xp, XpSource::Consume).await;

    // Consumable-healing bonus, bounded by config and the global perk switch.
    if !config.perks.enabled {
        return;
    }
    let level = perk_level(state, player.gameprofile.id, SKILL).await;
    let heal = consumable_heal_bonus(herbalism, level);
    if heal <= 0.0 {
        return;
    }
    let living = &player.living_entity;
    let max_health = living.get_max_health();
    if living.health.load() < max_health {
        living.heal(heal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_yield_chance_scales_with_tier() {
        let herbalism = super::super::config::HerbalismConfig::default();

        // Tier 0 adds nothing: the pre-tier base chance (8%).
        assert!((quality_yield_chance(&herbalism, 1, 1.0) - 0.08).abs() < f64::EPSILON);
        assert!((quality_yield_chance(&herbalism, 9, 1.0) - 0.08).abs() < f64::EPSILON);
        // Tier 4 at level 100: 8% + 4 * 2% = 16%.
        assert!((quality_yield_chance(&herbalism, 100, 1.0) - 0.16).abs() < f64::EPSILON);
    }

    #[test]
    fn quality_yield_chance_respects_global_cap() {
        let herbalism = super::super::config::HerbalismConfig::default();

        assert!((quality_yield_chance(&herbalism, 100, 0.12) - 0.12).abs() < f64::EPSILON);
        assert!((quality_yield_chance(&herbalism, 1, 0.05) - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn consumable_heal_bonus_scales_with_tier() {
        let herbalism = super::super::config::HerbalismConfig::default();

        // Tier 0 adds nothing: the pre-tier base heal (1 hp).
        assert_eq!(consumable_heal_bonus(&herbalism, 1), 1.0);
        assert_eq!(consumable_heal_bonus(&herbalism, 9), 1.0);
        // Tier 4 at level 100: 1 + 4 * 0.5 = 3 hp.
        assert_eq!(consumable_heal_bonus(&herbalism, 100), 3.0);
        assert_eq!(consumable_heal_bonus(&herbalism, 50), 2.5);
    }
}
