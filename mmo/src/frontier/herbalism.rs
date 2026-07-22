//! Herbalism skill: plant/forage XP, quality yield, and consumable-healing
//! bonuses.
//!
//! XP rule: one Herbalism award per broken natural plant or per eaten
//! configured consumable. Tracked plants are provenance-marked on placement,
//! so player-placed plants earn nothing.

use pumpkin::plugin::api::events::{
    block::block_broken::BlockBrokenEvent,
    player::player_item_use_complete::PlayerItemUseCompleteEvent,
};
use pumpkin_data::item_stack::ItemStack;
use rand::RngExt;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Award Herbalism XP for a broken natural plant, then roll quality yield.
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

    progression::award_xp(state, player, SkillId::Herbalism, xp, XpSource::Forage).await;

    // Quality yield: chance for one bonus item of the broken plant.
    if !config.perks.enabled {
        return;
    }
    let chance = herbalism
        .quality_yield_chance
        .min(config.perks.max_proc_chance);
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

/// Award Herbalism XP and a bounded healing bonus for eating configured
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

    progression::award_xp(state, player, SkillId::Herbalism, xp, XpSource::Consume).await;

    // Consumable-healing bonus, bounded by config and the global perk switch.
    if !config.perks.enabled || herbalism.consumable_heal_bonus <= 0.0 {
        return;
    }
    let living = &player.living_entity;
    let max_health = living.get_max_health();
    if living.health.load() < max_health {
        living.heal(herbalism.consumable_heal_bonus);
    }
}
