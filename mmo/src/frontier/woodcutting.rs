//! Woodcutting skill: natural-log XP, the Heartwood roll, and the Timber perk.
//!
//! XP rule: one primary skill (Woodcutting) per natural log break. Natural
//! trees are validated in Cabbage through the shared provenance tracker:
//! player-placed logs earn nothing and cannot feed Timber.

use pumpkin::{
    entity::EntityBase,
    plugin::api::events::block::{block_break::BlockBreakEvent, block_broken::BlockBrokenEvent},
};
use pumpkin_data::{Block, item_stack::ItemStack};
use rand::RngExt;

use super::super::{
    MmoState,
    ore_reveal::provenance::ProvenanceKey,
    perks::batch_break,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

const TIMBER_COOLDOWN_KEY: &str = "woodcutting.timber";

/// Award Woodcutting XP for a broken natural log.
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
    let woodcutting = &config.frontier.woodcutting;
    let Some(xp) = woodcutting.log_xp.get(event.block.name).copied() else {
        return;
    };

    progression::award_xp(
        state,
        player,
        SkillId::Woodcutting,
        xp,
        XpSource::BlockBreak,
    )
    .await;

    // Heartwood roll: chance for one bonus log plus bonus XP.
    if !config.perks.enabled {
        return;
    }
    let chance = woodcutting
        .heartwood_chance
        .min(config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }
    if let Some(item) = pumpkin_data::item::Item::from_registry_key(event.block.name) {
        state.audit(&format!(
            "quality roll: heartwood proc on {} for {}",
            event.block.name, player.gameprofile.id
        ));
        event
            .world
            .drop_stack(&event.block_position, ItemStack::new(1, item))
            .await;
        progression::award_xp(
            state,
            player,
            SkillId::Woodcutting,
            woodcutting.heartwood_xp_bonus,
            XpSource::BlockBreak,
        )
        .await;
    }
}

/// Timber: sneaking while breaking a natural log fells connected logs of the
/// same type in one bounded, cooldown-gated transaction.
pub async fn handle_block_break(state: &MmoState, event: &BlockBreakEvent) {
    let Some(player) = event.player.as_ref() else {
        return;
    };
    if event.cancelled || !player.get_entity().is_sneaking() || !earns_xp(player) {
        return;
    }
    let config = state.config();
    let woodcutting = &config.frontier.woodcutting;
    if !woodcutting.timber_enabled || !config.perks.enabled {
        return;
    }
    if !woodcutting.is_tracked_log(event.block.name) {
        return;
    }

    let world = player.get_entity().world.load_full();
    if state
        .provenance()
        .contains(&ProvenanceKey::new(&world, event.block_position))
    {
        return;
    }

    let target: &'static Block = event.block;
    let provenance = state.provenance();
    let closure_world = world.clone();
    batch_break::try_batch_break(
        state,
        &world,
        player,
        event.block_position,
        woodcutting.timber_max_blocks,
        TIMBER_COOLDOWN_KEY,
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
