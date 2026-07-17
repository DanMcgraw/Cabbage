//! Agriculture skill: mature-crop harvest XP, fertilizer provenance, and a
//! conservative harvest bonus.
//!
//! XP rule: one primary skill (Agriculture) per mature crop harvest.
//! Maturity comes from the broken block's `age` property, so immature crops
//! earn nothing. Fertilizer state travels with the block via `Context` block
//! metadata (`crop_v1`) and drives a deterministic quality roll at harvest;
//! player-placed crops cannot exist at maturity in survival, so no
//! provenance tracking is needed here.

use pumpkin::{
    entity::EntityBase,
    plugin::api::events::{
        block::block_broken::BlockBrokenEvent,
        player::player_interact_event::{InteractAction, PlayerInteractEvent},
    },
};
use pumpkin_data::{Block, BlockStateId, item::Item, item_stack::ItemStack};
use rand::Rng;

use super::super::{
    MmoState,
    persistence::CropDataV1,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Award harvest XP and roll the harvest bonus for a broken mature crop.
pub async fn handle_block_broken(state: &MmoState, event: &BlockBrokenEvent) {
    let Some(player) = event.player.as_ref() else {
        return;
    };
    if !earns_xp(player) {
        return;
    }
    let config = state.config();
    let agriculture = &config.frontier.agriculture;
    let Some(crop) = agriculture.crops.get(event.block.name) else {
        return;
    };
    let Some(age) = crop_age(event.block_state_id) else {
        return;
    };
    if age < crop.max_age {
        return;
    }

    let context = state.context().clone();
    let fertilizer = CropDataV1::read(&context, &event.world, &event.block_position).map(|data| {
        // Fertilizer data is single-use: the crop is gone now.
        let _ = CropDataV1::clear(&context, &event.world, &event.block_position);
        data
    });

    progression::award_xp(
        state,
        player,
        SkillId::Agriculture,
        crop.xp,
        XpSource::Harvest,
    )
    .await;

    let fertilized = fertilizer.is_some();
    if fertilized && agriculture.fertilizer_bonus_xp > 0 {
        state.audit(&format!(
            "quality roll: fertilized {} harvested by {}",
            event.block.name, player.gameprofile.id
        ));
        progression::award_xp(
            state,
            player,
            SkillId::Agriculture,
            agriculture.fertilizer_bonus_xp,
            XpSource::Harvest,
        )
        .await;
    }

    // Conservative harvest bonus: one bonus item on a successful roll.
    if !config.perks.enabled {
        return;
    }
    let guaranteed = fertilized && agriculture.fertilizer_guarantees_bonus;
    let roll = match &fertilizer {
        // Deterministic quality roll from the stored seed.
        Some(data) => (data.quality_seed % 1000) as f64 / 1000.0,
        None => rand::rng().random::<f64>(),
    };
    let chance = agriculture
        .harvest_bonus_chance
        .min(config.perks.max_proc_chance);
    if !guaranteed && roll >= chance {
        return;
    }
    if let Some(item) = Item::from_registry_key(&crop.bonus_item) {
        event
            .world
            .drop_stack(&event.block_position, ItemStack::new(1, item))
            .await;
    }
}

/// Mark a crop as fertilized when a player applies bone meal to it.
pub async fn handle_player_interact(state: &MmoState, event: &PlayerInteractEvent) {
    if event.action != InteractAction::RightClickBlock || event.cancelled {
        return;
    }
    let Some(position) = event.clicked_pos else {
        return;
    };
    let config = state.config();
    if !config.frontier.agriculture.is_crop(event.block.name) {
        return;
    }
    let player = &event.player;
    if !earns_xp(player) {
        return;
    }

    let held = player.inventory().held_item().lock().await.clone();
    if held.item_count == 0 || held.item.registry_key != "bone_meal" {
        return;
    }

    let data = CropDataV1 {
        fertilizer_tier: 1,
        quality_seed: rand::rng().random::<u64>(),
    };
    let world = player.get_entity().world.load_full();
    if let Err(error) = data.write(state.context(), &world, &position) {
        log::warn!("[Cabbage MMO] failed to record fertilizer data: {error}");
    }
}

/// Read the `age` block property of the broken crop state, if it has one.
fn crop_age(state_id: BlockStateId) -> Option<u32> {
    let block = Block::from_state_id(state_id);
    block.properties(state_id).and_then(|props| {
        props
            .to_props()
            .iter()
            .find(|(key, _)| *key == "age")
            .and_then(|(_, value)| value.parse::<u32>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheat_maturity_is_read_from_state() {
        let mature = Block::WHEAT
            .properties(Block::WHEAT.default_state.id)
            .map(|props| props.to_props());
        // Default state is age 0; the parser must at least find the property.
        let props = mature.expect("wheat has properties");
        assert!(props.iter().any(|(key, _)| *key == "age"));
        assert_eq!(crop_age(Block::WHEAT.default_state.id), Some(0));
    }
}
