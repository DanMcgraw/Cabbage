//! Enchanting: offer adjustment and commit-time XP.
//!
//! The offer's level requirement is discounted in the generate preview; XP
//! is awarded only when the enchant is committed. Rune rules are follow-on
//! content (see the plan's Phase 3 intro).

use pumpkin::plugin::api::events::player::{
    enchant_item::EnchantItemEvent, enchant_item_generate::EnchantItemGenerateEvent,
};

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Discount the offer's level requirement in the generate preview.
pub async fn handle_enchant_generate(state: &MmoState, event: &mut EnchantItemGenerateEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    if !config.perks.enabled {
        return;
    }
    let enchanting = &config.enterprise.enchanting;

    let level = player_level(state, &event.player).await;
    let discount =
        (enchanting.offer_discount_per_level * level as f64).min(enchanting.offer_discount_cap);
    if discount <= 0.0 {
        return;
    }
    event.level_requirement = (event.level_requirement as f64 - discount).round().max(0.0) as i32;
}

/// Award Enchanting XP scaled by the commit's level cost.
pub async fn handle_enchant_item(state: &MmoState, event: &EnchantItemEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let enchanting = &config.enterprise.enchanting;
    let xp = (event.level_cost.max(0) as f64 * enchanting.xp_per_level_cost).round() as u64;
    let xp = xp.min(enchanting.xp_cap);
    if xp == 0 {
        return;
    }
    progression::award_xp(
        state,
        &event.player,
        SkillId::Enchanting,
        xp,
        XpSource::Enchant,
    )
    .await;
}

async fn player_level(state: &MmoState, player: &pumpkin::entity::player::Player) -> u32 {
    let curve = state.curve(SkillId::Enchanting);
    state
        .db()
        .get_skill(player.gameprofile.id, SkillId::Enchanting)
        .await
        .map(|data| curve.level_for_xp(data.xp).0)
        .unwrap_or(1)
}
