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
    perks::{batch_break, eligibility::perk_tier},
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};

const TIMBER_COOLDOWN_KEY: &str = "woodcutting.timber";

/// Heartwood roll chance: the base chance plus the per-tier step, clamped by
/// the global proc-chance cap.
fn heartwood_chance(
    woodcutting: &super::config::WoodcuttingConfig,
    level: u32,
    global_cap: f64,
) -> f64 {
    (woodcutting.heartwood_chance
        + woodcutting.heartwood_chance_per_tier * f64::from(perk_tier(level)))
    .min(global_cap)
}

/// Bonus XP awarded alongside a successful Heartwood roll.
fn heartwood_xp_bonus(woodcutting: &super::config::WoodcuttingConfig, level: u32) -> u64 {
    woodcutting.heartwood_xp_bonus.saturating_add(
        woodcutting
            .heartwood_xp_bonus_per_tier
            .saturating_mul(u64::from(perk_tier(level))),
    )
}

/// Maximum extra logs Timber may fell in one action: the base allowance plus
/// the per-tier step for the player's perk tier.
fn timber_max_blocks(woodcutting: &super::config::WoodcuttingConfig, level: u32) -> u32 {
    woodcutting.timber_max_blocks.saturating_add(
        woodcutting
            .timber_max_blocks_per_tier
            .saturating_mul(perk_tier(level)),
    )
}

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
    let level = perk_level(state, player.gameprofile.id, SkillId::Woodcutting).await;
    let chance = heartwood_chance(woodcutting, level, config.perks.max_proc_chance);
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
            heartwood_xp_bonus(woodcutting, level),
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

    let level = perk_level(state, player.gameprofile.id, SkillId::Woodcutting).await;
    let target: &'static Block = event.block;
    let provenance = state.provenance();
    let closure_world = world.clone();
    batch_break::try_batch_break(
        state,
        &world,
        player,
        event.block_position,
        timber_max_blocks(woodcutting, level),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartwood_chance_scales_with_tier() {
        let woodcutting = super::super::config::WoodcuttingConfig::default();

        // Tier 0 adds nothing: the pre-tier base chance (2%).
        assert!((heartwood_chance(&woodcutting, 1, 1.0) - 0.02).abs() < f64::EPSILON);
        assert!((heartwood_chance(&woodcutting, 9, 1.0) - 0.02).abs() < f64::EPSILON);
        // Tier 4 at level 100: 2% + 4 * 1% = 6%.
        assert!((heartwood_chance(&woodcutting, 100, 1.0) - 0.06).abs() < f64::EPSILON);
    }

    #[test]
    fn heartwood_chance_respects_global_cap() {
        let woodcutting = super::super::config::WoodcuttingConfig::default();

        assert!((heartwood_chance(&woodcutting, 100, 0.05) - 0.05).abs() < f64::EPSILON);
        assert!((heartwood_chance(&woodcutting, 1, 0.01) - 0.01).abs() < f64::EPSILON);
    }

    #[test]
    fn heartwood_xp_bonus_scales_with_tier() {
        let woodcutting = super::super::config::WoodcuttingConfig::default();

        assert_eq!(heartwood_xp_bonus(&woodcutting, 1), 25);
        assert_eq!(heartwood_xp_bonus(&woodcutting, 9), 25);
        assert_eq!(heartwood_xp_bonus(&woodcutting, 10), 30);
        assert_eq!(heartwood_xp_bonus(&woodcutting, 100), 45);
    }

    #[test]
    fn timber_max_blocks_scales_with_tier() {
        let woodcutting = super::super::config::WoodcuttingConfig::default();

        assert_eq!(timber_max_blocks(&woodcutting, 1), 32);
        assert_eq!(timber_max_blocks(&woodcutting, 9), 32);
        assert_eq!(timber_max_blocks(&woodcutting, 10), 40);
        assert_eq!(timber_max_blocks(&woodcutting, 50), 56);
        assert_eq!(timber_max_blocks(&woodcutting, 100), 64);
    }
}
