//! Mining skill: ore XP, Prospector bonus yields, and the Vein Miner perk.
//!
//! XP rule: one primary skill (Mining) per ore block break, awarded only for
//! natural blocks with a configured reward — player-placed ores (silk touch)
//! are excluded through the shared provenance tracker. Ore-reveal eligibility
//! lives in `ore_reveal/`; this handler owns XP and perks.

use pumpkin::{
    entity::EntityBase,
    plugin::api::events::block::{
        block_break::BlockBreakEvent, block_broken::BlockBrokenEvent,
        block_drop_item::BlockDropItemEvent,
    },
};
use rand::RngExt;

use super::super::{
    MmoState,
    ore_reveal::provenance::ProvenanceKey,
    perks::batch_break,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

const VEIN_MINER_COOLDOWN_KEY: &str = "mining.vein_miner";

fn prospector_chance(perk: &super::config::MiningPerkConfig, level: u32, global_cap: f64) -> f64 {
    (perk.prospector_base_chance + perk.prospector_chance_per_level * f64::from(level))
        .min(perk.prospector_max_chance)
        .min(global_cap)
}

/// Award the configured Mining XP once for a broken natural ore.
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
    let Some(base_xp) = state.block_xp_reward(event.block.name) else {
        return;
    };

    progression::award_xp(
        state,
        player,
        SkillId::Mining,
        base_xp,
        XpSource::BlockBreak,
    )
    .await;
}

/// Prospector: add exactly one item copied from the ore's normal committed
/// drops. This improves resource yield without minting additional Mining XP.
pub async fn handle_block_drop_item(state: &MmoState, event: &mut BlockDropItemEvent) {
    if event.cancelled || !earns_xp(&event.player) || event.items.is_empty() {
        return;
    }

    let config = state.config();
    let perk = &config.frontier.mining;
    if !perk.prospector_enabled
        || !config.perks.enabled
        || state.block_xp_reward(event.block.name).is_none()
    {
        return;
    }

    let world = event.player.world();
    if state
        .provenance()
        .contains(&ProvenanceKey::new(&world, event.block_position))
    {
        return;
    }

    let progress = match state
        .db()
        .get_skill(event.player.gameprofile.id, SkillId::Mining)
        .await
    {
        Ok(progress) => progress,
        Err(error) => {
            log::warn!("[Cabbage MMO] failed to read Mining level for Prospector: {error}");
            return;
        }
    };
    let level = state.curve(SkillId::Mining).level_for_xp(progress.xp).0;
    let chance = prospector_chance(perk, level, config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }

    let Some(mut bonus) = event
        .items
        .iter()
        .find(|stack| stack.item_count > 0)
        .cloned()
    else {
        return;
    };
    bonus.item_count = 1;
    let item_name = bonus.item.registry_key;
    event.items.push(bonus);
    state.audit(&format!(
        "prospector yield: one {item_name} for {} from {}",
        event.player.gameprofile.id, event.block.name
    ));
}

/// Vein Miner: sneaking while breaking a configured ore breaks the connected
/// vein in one bounded, cooldown-gated transaction.
pub async fn handle_block_break(state: &MmoState, event: &BlockBreakEvent) {
    let Some(player) = event.player.as_ref() else {
        return;
    };
    if event.cancelled || !player.get_entity().is_sneaking() || !earns_xp(player) {
        return;
    }
    let config = state.config();
    let perk = &config.frontier.mining;
    if !perk.vein_miner_enabled || !config.perks.enabled {
        return;
    }
    if state.block_xp_reward(event.block.name).is_none() {
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
        perk.vein_miner_max_blocks,
        VEIN_MINER_COOLDOWN_KEY,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prospector_chance_scales_and_respects_caps() {
        let perk = super::super::config::MiningPerkConfig::default();

        assert!((prospector_chance(&perk, 1, 1.0) - 0.052).abs() < f64::EPSILON);
        assert!((prospector_chance(&perk, 100, 1.0) - 0.25).abs() < f64::EPSILON);
        assert!((prospector_chance(&perk, 1000, 0.30) - 0.30).abs() < f64::EPSILON);
    }
}
