//! Repair activity: anvil cost discount and repair XP.
//!
//! Per the plan's Enterprise rules: the discount is previewed in the prepare
//! event (only while the cooldown is ready), and XP is awarded plus the
//! cooldown charged only when the output is actually taken. Since the
//! six-skill consolidation both the discount gate and the award use the
//! shared Maintenance track. Tool Care (a chance to refund 1 durability on
//! the held tool after a block break) rolls here as well.

use pumpkin::plugin::api::events::{
    block::block_break::BlockBreakEvent,
    player::{anvil_prepare::AnvilPrepareEvent, anvil_repair::AnvilCompleteEvent},
};
use rand::RngExt;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
    ui::skill_enabled,
};
use super::config::RepairConfig;

/// The shared skill track this activity awards and gates on: repair and
/// salvage both feed Maintenance since the six-skill consolidation.
pub(crate) const SKILL: SkillId = SkillId::Maintenance;

const REPAIR_COOLDOWN_KEY: &str = "repair.discount";

/// Repair level-cost discount: the per-level reduction clamped by the cap,
/// which gains a step per perk tier.
fn repair_discount(repair: &RepairConfig, level: u32) -> f64 {
    (repair.discount_per_level * f64::from(level))
        .min(repair.discount_cap + repair.discount_cap_per_tier * f64::from(perk_tier(level)))
}

/// Tool Care proc chance: the base chance plus a step per perk tier,
/// clamped by the global proc cap.
fn tool_care_chance(repair: &RepairConfig, level: u32, global_cap: f64) -> f64 {
    (repair.tool_care_chance + repair.tool_care_chance_per_tier * f64::from(perk_tier(level)))
        .min(global_cap)
}

/// Preview the Maintenance (repair activity) level-cost discount in the
/// anvil prepare event.
pub async fn handle_anvil_prepare(state: &MmoState, event: &mut AnvilPrepareEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    if !config.perks.enabled {
        return;
    }
    let repair = &config.enterprise.repair;
    let player_uuid = event.player.gameprofile.id;
    if state.perk_cooldowns().remaining_ticks(
        player_uuid,
        REPAIR_COOLDOWN_KEY,
        state.current_tick(),
    ) > 0
    {
        return;
    }

    let level = perk_level(state, player_uuid, SKILL).await;
    let discount = repair_discount(repair, level);
    if discount <= 0.0 {
        return;
    }
    event.level_cost = (event.level_cost as f64 - discount).round().max(0.0) as i16;
    state.mark_perk_preview(event.transaction.id, REPAIR_COOLDOWN_KEY);
}

/// Award Maintenance XP and charge the cooldown when the output is taken.
pub async fn handle_anvil_complete(state: &MmoState, event: &AnvilCompleteEvent) {
    if !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let repair = &config.enterprise.repair;

    let player_uuid = event.player.gameprofile.id;
    let current_tick = state.current_tick();
    if config.perks.enabled && state.take_perk_preview(event.transaction.id, REPAIR_COOLDOWN_KEY) {
        state.perk_cooldowns().try_activate(
            player_uuid,
            REPAIR_COOLDOWN_KEY,
            current_tick,
            repair.cooldown_ticks,
        );
    }

    progression::award_xp(state, &event.player, SKILL, repair.xp, XpSource::Repair).await;
    state.audit(&format!(
        "anvil commit: {} took output for {} level(s)",
        event.player.gameprofile.id, event.level_cost
    ));
}

/// Tool Care: after a block break, a bounded chance to refund 1 durability
/// on the held tool. Awards no XP.
///
/// Pumpkin has no item-durability event, so the roll happens per block
/// break; the refund goes through `ItemStack::repair_item`, which
/// intentionally bypasses Unbreaking (perk semantics, not an enchantment
/// interaction). Not audited: a 1-durability refund is too frequent and too
/// minor for the audit log (unlike the transactional anvil commit above).
pub async fn handle_tool_care(state: &MmoState, event: &BlockBreakEvent) {
    if event.cancelled {
        return;
    }
    let Some(player) = event.player.as_ref() else {
        return;
    };
    if !earns_xp(player) {
        return;
    }
    let config = state.config();
    let repair = &config.enterprise.repair;
    if !config.perks.enabled || !repair.tool_care_enabled || !skill_enabled(&config, SKILL) {
        return;
    }

    let level = perk_level(state, player.gameprofile.id, SKILL).await;
    let chance = tool_care_chance(repair, level, config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }

    let inventory = player.inventory();
    let held_slot = inventory.held_item();
    let mut held = held_slot.lock().await;
    if held.is_damageable() && held.get_damage() > 0 {
        held.repair_item(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_discount_tier_zero_matches_pre_tier_value() {
        let repair = RepairConfig::default();
        // Below the cap the per-level discount applies unchanged.
        assert!((repair_discount(&repair, 5) - 0.25).abs() < f64::EPSILON);
        // The old 10.0 cap still clamps at tier 0 (steep per-level value so
        // the cap is reached below level 10).
        let mut steep = repair.clone();
        steep.discount_per_level = 5.0;
        assert!((repair_discount(&steep, 9) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn repair_discount_tier_four_raises_the_cap() {
        let repair = RepairConfig::default();
        // 0.05*400 = 20.0 → 10.0 + 1.0*4 = 14.0.
        assert!((repair_discount(&repair, 400) - 14.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tool_care_chance_adds_a_step_per_tier() {
        let repair = RepairConfig::default();
        // Tier 0 matches the pre-tier base chance.
        assert!((tool_care_chance(&repair, 1, 1.0) - 0.05).abs() < f64::EPSILON);
        // Tier 4 (level 100): 0.05 + 0.025*4 = 0.15.
        assert!((tool_care_chance(&repair, 100, 1.0) - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn tool_care_chance_global_cap_still_wins() {
        let repair = RepairConfig::default();
        assert!((tool_care_chance(&repair, 100, 0.10) - 0.10).abs() < f64::EPSILON);
        assert!((tool_care_chance(&repair, 1, 0.01) - 0.01).abs() < f64::EPSILON);
    }
}
