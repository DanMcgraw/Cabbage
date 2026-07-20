//! Repair: anvil cost discount and repair XP.
//!
//! Per the plan's Enterprise rules: the discount is previewed in the prepare
//! event (only while the cooldown is ready), and XP is awarded plus the
//! cooldown charged only when the output is actually taken.

use pumpkin::plugin::api::events::player::{
    anvil_prepare::AnvilPrepareEvent, anvil_repair::AnvilCompleteEvent,
};

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

const REPAIR_COOLDOWN_KEY: &str = "repair.discount";

/// Preview the Repair level-cost discount in the anvil prepare event.
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

    let level = player_level(state, &event.player).await;
    let discount = (repair.discount_per_level * level as f64).min(repair.discount_cap);
    if discount <= 0.0 {
        return;
    }
    event.level_cost = (event.level_cost as f64 - discount).round().max(0.0) as i16;
    state.mark_perk_preview(event.transaction.id, REPAIR_COOLDOWN_KEY);
}

/// Award Repair XP and charge the cooldown when the output is taken.
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

    progression::award_xp(
        state,
        &event.player,
        SkillId::Repair,
        repair.xp,
        XpSource::Repair,
    )
    .await;
    state.audit(&format!(
        "anvil commit: {} took output for {} level(s)",
        event.player.gameprofile.id, event.level_cost
    ));
}

async fn player_level(state: &MmoState, player: &pumpkin::entity::player::Player) -> u32 {
    let curve = state.curve(SkillId::Repair);
    state
        .db()
        .get_skill(player.gameprofile.id, SkillId::Repair)
        .await
        .map(|data| curve.level_for_xp(data.xp).0)
        .unwrap_or(1)
}
