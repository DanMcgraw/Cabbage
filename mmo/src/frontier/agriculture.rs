//! Agriculture activity: mature-crop harvest XP, fertilizer provenance, and a
//! conservative harvest bonus.
//!
//! XP rule: one primary skill per mature crop harvest. Since the six-skill
//! consolidation that skill is Cultivation, shared with the herbalism
//! activity. Maturity comes from the broken block's `age` property, so
//! immature crops earn nothing. Fertilizer state travels with the block via
//! `Context` block metadata (`crop_v1`) and drives a deterministic quality
//! roll at harvest; player-placed crops cannot exist at maturity in
//! survival, so no provenance tracking is needed here.

use pumpkin::plugin::api::events::{
    block::block_broken::BlockBrokenEvent, block::bone_meal::BoneMealApplyCompleteEvent,
};
use pumpkin_data::{Block, BlockStateId, item::Item, item_stack::ItemStack};
use rand::RngExt;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    persistence::CropDataV1,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};

/// The shared skill track this activity awards: agriculture and herbalism
/// both feed Cultivation since the six-skill consolidation.
pub(crate) const SKILL: SkillId = SkillId::Cultivation;

/// Harvest bonus chance: the base chance plus the per-tier step, clamped by
/// the global proc-chance cap.
fn harvest_bonus_chance(
    agriculture: &super::config::AgricultureConfig,
    level: u32,
    global_cap: f64,
) -> f64 {
    (agriculture.harvest_bonus_chance
        + agriculture.harvest_bonus_chance_per_tier * f64::from(perk_tier(level)))
    .min(global_cap)
}

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

    progression::award_xp(state, player, SKILL, crop.xp, XpSource::Harvest).await;

    let fertilized = fertilizer.is_some();
    if fertilized && agriculture.fertilizer_bonus_xp > 0 {
        state.audit(&format!(
            "quality roll: fertilized {} harvested by {}",
            event.block.name, player.gameprofile.id
        ));
        progression::award_xp(
            state,
            player,
            SKILL,
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
    if !guaranteed {
        let roll = match &fertilizer {
            // Deterministic quality roll from the stored seed.
            Some(data) => (data.quality_seed % 1000) as f64 / 1000.0,
            None => rand::rng().random::<f64>(),
        };
        let level = perk_level(state, player.gameprofile.id, SKILL).await;
        let chance = harvest_bonus_chance(agriculture, level, config.perks.max_proc_chance);
        if roll >= chance {
            return;
        }
    }
    if let Some(item) = Item::from_registry_key(&crop.bonus_item) {
        event
            .world
            .drop_stack(&event.block_position, ItemStack::new(1, item))
            .await;
    }
}

/// Mark a crop as fertilized when a player applies bone meal to it.
pub async fn handle_bone_meal_complete(state: &MmoState, event: &BoneMealApplyCompleteEvent) {
    if event.consumed_count == 0 || !event.growth_occurred {
        return;
    }
    let block = Block::from_state_id(event.state_before);
    let config = state.config();
    if !config.frontier.agriculture.is_crop(block.name) {
        return;
    }
    let player = &event.player;
    if !earns_xp(player) {
        return;
    }

    let data = CropDataV1 {
        fertilizer_tier: 1,
        quality_seed: rand::rng().random::<u64>(),
    };
    if let Err(error) = data.write(state.context(), &event.world, &event.position) {
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

    #[test]
    fn harvest_bonus_chance_scales_with_tier() {
        let agriculture = super::super::config::AgricultureConfig::default();

        // Tier 0 adds nothing: the pre-tier base chance (10%).
        assert!((harvest_bonus_chance(&agriculture, 1, 1.0) - 0.10).abs() < f64::EPSILON);
        assert!((harvest_bonus_chance(&agriculture, 9, 1.0) - 0.10).abs() < f64::EPSILON);
        // Tier 4 at level 100: 10% + 4 * 2% = 18%.
        assert!((harvest_bonus_chance(&agriculture, 100, 1.0) - 0.18).abs() < f64::EPSILON);
    }

    #[test]
    fn harvest_bonus_chance_respects_global_cap() {
        let agriculture = super::super::config::AgricultureConfig::default();

        assert!((harvest_bonus_chance(&agriculture, 100, 0.15) - 0.15).abs() < f64::EPSILON);
        assert!((harvest_bonus_chance(&agriculture, 1, 0.05) - 0.05).abs() < f64::EPSILON);
    }
}
