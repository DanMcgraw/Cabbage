//! Smithing: craft/smelt XP and durable creator/provenance markers.
//!
//! XP rules: one Smithing award per configured craft and per configured
//! furnace extraction. `CraftItemEvent` is observational in this Pumpkin
//! build (its result is not read back), so creator/provenance markers are
//! written onto *anvil* outputs instead — the prepare event's output is
//! honored, and the marker only ever adds Cabbage-namespaced data while the
//! vanilla-computed item stays the default.

use pumpkin::plugin::api::events::{
    player::anvil_prepare::AnvilPrepareEvent, player::craft_item::CraftItemEvent,
    player::furnace_extract::FurnaceExtractEvent,
};

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    persistence::ItemDataV1,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};
use super::config::SmithingConfig;

/// Tier-scaled XP award: the configured amount multiplied by
/// `1.0 + xp_multiplier_per_tier * T`, rounded. The result still flows
/// through the central `award_xp` clamp.
fn tier_scaled_xp(smithing: &SmithingConfig, level: u32, base: u64) -> u64 {
    (base as f64 * (1.0 + smithing.xp_multiplier_per_tier * f64::from(perk_tier(level)))).round()
        as u64
}

/// Award Smithing XP for crafting configured tools, weapons, and armor.
pub async fn handle_craft_item(state: &MmoState, event: &CraftItemEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let Some(xp) = config
        .enterprise
        .smithing
        .craft_xp
        .get(event.result.item.registry_key)
        .copied()
    else {
        return;
    };
    let level = perk_level(state, event.player.gameprofile.id, SkillId::Smithing).await;
    let xp = tier_scaled_xp(&config.enterprise.smithing, level, xp);
    progression::award_xp(state, &event.player, SkillId::Smithing, xp, XpSource::Craft).await;
}

/// Award Smithing XP for extracting configured smelted items.
pub async fn handle_furnace_extract(state: &MmoState, event: &FurnaceExtractEvent) {
    if !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let smithing = &config.enterprise.smithing;
    let xp = smithing
        .smelt_xp
        .get(event.item.item.registry_key)
        .copied()
        .unwrap_or(smithing.default_smelt_xp);
    let level = perk_level(state, event.player.gameprofile.id, SkillId::Smithing).await;
    let xp = tier_scaled_xp(smithing, level, xp);
    progression::award_xp(state, &event.player, SkillId::Smithing, xp, XpSource::Smelt).await;
}

/// Tag anvil outputs with creator/provenance item data (durable marker).
pub async fn handle_anvil_prepare(state: &MmoState, event: &mut AnvilPrepareEvent) {
    let config = state.config();
    if !config.perks.enabled || !config.enterprise.smithing.mark_anvil_outputs {
        return;
    }
    if event.output.item_count == 0 {
        return;
    }
    // Preserve the vanilla-computed item; only add the Cabbage marker.
    let mut marker = ItemDataV1::read(&event.output).unwrap_or_default();
    marker.creator = Some(event.player.gameprofile.id);
    marker.provenance = Some("smithing".to_string());
    marker.write(&mut event.output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_scaled_xp_tier_zero_matches_pre_tier_value() {
        let smithing = SmithingConfig::default();
        for level in [1, 5, 9] {
            assert_eq!(tier_scaled_xp(&smithing, level, 15), 15, "level {level}");
        }
    }

    #[test]
    fn tier_scaled_xp_tier_four_adds_twenty_percent() {
        let smithing = SmithingConfig::default();
        // Tier 4 (level 100): 1.0 + 0.05*4 = 1.2x.
        assert_eq!(tier_scaled_xp(&smithing, 100, 15), 18);
        assert_eq!(tier_scaled_xp(&smithing, 100, 40), 48);
    }
}
