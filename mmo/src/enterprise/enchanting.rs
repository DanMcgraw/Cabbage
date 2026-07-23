//! Enchanting: offer adjustment and commit-time XP.
//!
//! The offer's level requirement is discounted in the generate preview; XP
//! is awarded only when the enchant is committed. Rune rules are follow-on
//! content (see the plan's Phase 3 intro).

use pumpkin::plugin::api::events::player::{
    enchant_item::EnchantItemCompleteEvent, enchant_item_generate::EnchantItemGenerateEvent,
};

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};
use super::config::EnchantingConfig;

/// Offer discount: the per-level reduction clamped by the cap, which gains
/// a step per perk tier.
fn offer_discount(enchanting: &EnchantingConfig, level: u32) -> f64 {
    (enchanting.offer_discount_per_level * f64::from(level)).min(
        enchanting.offer_discount_cap
            + enchanting.offer_discount_cap_per_tier * f64::from(perk_tier(level)),
    )
}

/// Enchant XP cap: the base cap plus a step per perk tier.
fn enchant_xp_cap(enchanting: &EnchantingConfig, level: u32) -> u64 {
    enchanting
        .xp_cap
        .saturating_add(enchanting.xp_cap_per_tier.saturating_mul(u64::from(perk_tier(level))))
}

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

    let level = perk_level(state, event.player.gameprofile.id, SkillId::Enchanting).await;
    let discount = offer_discount(enchanting, level);
    if discount <= 0.0 {
        return;
    }
    event.level_requirement = (event.level_requirement as f64 - discount).round().max(0.0) as i32;
}

/// Award Enchanting XP scaled by the commit's level cost.
pub async fn handle_enchant_complete(state: &MmoState, event: &EnchantItemCompleteEvent) {
    if !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let enchanting = &config.enterprise.enchanting;
    let xp = (event.level_cost.max(0) as f64 * enchanting.xp_per_level_cost).round() as u64;
    let level = perk_level(state, event.player.gameprofile.id, SkillId::Enchanting).await;
    let xp = xp.min(enchant_xp_cap(enchanting, level));
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
    state.audit(&format!(
        "enchant commit: {} enchanted for {} level(s)",
        event.player.gameprofile.id, event.level_cost
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_discount_tier_zero_matches_pre_tier_value() {
        let enchanting = EnchantingConfig::default();
        // Below the cap the per-level discount applies unchanged.
        assert!((offer_discount(&enchanting, 5) - 0.1).abs() < f64::EPSILON);
        // The old 5.0 cap still clamps at tier 0 (steep per-level value so
        // the cap is reached below level 10).
        let mut steep = enchanting.clone();
        steep.offer_discount_per_level = 1.0;
        assert!((offer_discount(&steep, 9) - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn offer_discount_tier_four_raises_the_cap() {
        let enchanting = EnchantingConfig::default();
        // 0.02*500 = 10.0 → 5.0 + 1.0*4 = 9.0.
        assert!((offer_discount(&enchanting, 500) - 9.0).abs() < f64::EPSILON);
    }

    #[test]
    fn enchant_xp_cap_adds_a_step_per_tier() {
        let enchanting = EnchantingConfig::default();
        // Tier 0 matches the pre-tier cap.
        assert_eq!(enchant_xp_cap(&enchanting, 1), 100);
        // Tier 4 (level 100): 100 + 25*4 = 200.
        assert_eq!(enchant_xp_cap(&enchanting, 100), 200);
    }
}
