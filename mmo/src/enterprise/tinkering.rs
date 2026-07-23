//! Tinkering: mechanism crafting XP.
//!
//! Custom-item provenance already rides on every Cabbage-marked item via the
//! shared `item_v1` codec. "Small, contained receiver behavior" beyond
//! provenance waits for protected UI primitives; mechanism crafting XP is
//! the shippable slice.

use pumpkin::plugin::api::events::player::craft_item::CraftItemEvent;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};
use super::config::TinkeringConfig;

/// Tier-scaled XP award: the configured amount multiplied by
/// `1.0 + xp_multiplier_per_tier * T`, rounded. The result still flows
/// through the central `award_xp` clamp.
fn tier_scaled_xp(tinkering: &TinkeringConfig, level: u32, base: u64) -> u64 {
    (base as f64 * (1.0 + tinkering.xp_multiplier_per_tier * f64::from(perk_tier(level)))).round()
        as u64
}

/// Award Tinkering XP for crafting configured mechanisms.
pub async fn handle_craft_item(state: &MmoState, event: &CraftItemEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let Some(xp) = config
        .enterprise
        .tinkering
        .craft_xp
        .get(event.result.item.registry_key)
        .copied()
    else {
        return;
    };
    let level = perk_level(state, event.player.gameprofile.id, SkillId::Tinkering).await;
    let xp = tier_scaled_xp(&config.enterprise.tinkering, level, xp);
    progression::award_xp(
        state,
        &event.player,
        SkillId::Tinkering,
        xp,
        XpSource::Craft,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_scaled_xp_tier_zero_matches_pre_tier_value() {
        let tinkering = TinkeringConfig::default();
        for level in [1, 5, 9] {
            assert_eq!(tier_scaled_xp(&tinkering, level, 15), 15, "level {level}");
        }
    }

    #[test]
    fn tier_scaled_xp_tier_four_adds_twenty_percent() {
        let tinkering = TinkeringConfig::default();
        // Tier 4 (level 100): 1.0 + 0.05*4 = 1.2x.
        assert_eq!(tier_scaled_xp(&tinkering, 100, 15), 18);
        assert_eq!(tier_scaled_xp(&tinkering, 100, 10), 12);
    }
}
