//! Excavation skill: diggable-block XP, archaeology-style bonus loot, and the
//! bounded Earthmover perk.
//!
//! XP rule: one Excavation award per broken natural diggable block. Tracked
//! blocks are provenance-marked on placement, so player-placed blocks earn
//! nothing and cannot feed Earthmover.

use pumpkin::{
    entity::EntityBase,
    plugin::api::events::block::{block_break::BlockBreakEvent, block_broken::BlockBrokenEvent},
};
use pumpkin_data::item_stack::ItemStack;
use rand::RngExt;

use super::super::{
    MmoState,
    ore_reveal::provenance::ProvenanceKey,
    perks::batch_break,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

const EARTHMOVER_COOLDOWN_KEY: &str = "excavation.earthmover";

/// Award Excavation XP for a broken natural diggable block, then roll bonus
/// loot.
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
    let excavation = &config.frontier.excavation;
    let Some(xp) = excavation.diggable_xp.get(event.block.name).copied() else {
        return;
    };

    progression::award_xp(state, player, SkillId::Excavation, xp, XpSource::BlockBreak).await;

    // Archaeology-style bonus loot roll.
    if !config.perks.enabled {
        return;
    }
    let Some(loot) = excavation.bonus_loot.get(event.block.name) else {
        return;
    };
    let chance = loot.chance.min(config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }
    if let Some(item) = pumpkin_data::item::Item::from_registry_key(&loot.item) {
        state.audit(&format!(
            "quality roll: excavation loot {} from {} for {}",
            loot.item, event.block.name, player.gameprofile.id
        ));
        event
            .world
            .drop_stack(&event.block_position, ItemStack::new(1, item))
            .await;
    }
}

/// Earthmover: sneaking while breaking a natural diggable block excavates
/// connected blocks of the same type in one bounded, cooldown-gated
/// transaction.
pub async fn handle_block_break(state: &MmoState, event: &BlockBreakEvent) {
    let Some(player) = event.player.as_ref() else {
        return;
    };
    if event.cancelled || !player.get_entity().is_sneaking() || !earns_xp(player) {
        return;
    }
    let config = state.config();
    let excavation = &config.frontier.excavation;
    if !excavation.earthmover_enabled || !config.perks.enabled {
        return;
    }
    if !excavation.is_tracked_diggable(event.block.name) {
        return;
    }

    let world = player.get_entity().world.load_full();
    if state
        .provenance()
        .contains(&ProvenanceKey::new(&world, event.block_position))
    {
        return;
    }

    let target = event.block;
    let provenance = state.provenance();
    let closure_world = world.clone();
    batch_break::try_batch_break(
        state,
        &world,
        player,
        event.block_position,
        excavation.earthmover_max_blocks,
        EARTHMOVER_COOLDOWN_KEY,
        move |position| {
            let Some(state_id) = closure_world.get_block_state_id_if_loaded(&position) else {
                return false;
            };
            if pumpkin_data::Block::from_state_id(state_id) != target {
                return false;
            }
            !provenance.contains(&ProvenanceKey::new(&closure_world, position))
        },
    )
    .await;
}
