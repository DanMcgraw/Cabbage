//! Alchemy: XP for consuming potions, plus the Potion Mastery heal.
//!
//! Brewing XP is not attributable (`BrewEvent` carries no player) and
//! potency/duration mutation of already-applied effects has no safe hook in
//! this Pumpkin build, so both stay documented as blocked in `plan.md`.

use pumpkin::plugin::api::events::player::player_item_use_complete::PlayerItemUseCompleteEvent;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
    ui::skill_enabled,
};
use super::config::AlchemyConfig;

/// Potion Mastery heal: the base heal plus a step per perk tier.
fn potion_heal(alchemy: &AlchemyConfig, level: u32) -> f32 {
    alchemy.heal_base + alchemy.heal_per_tier * perk_tier(level) as f32
}

/// Award Alchemy XP for consuming a configured potion, then apply the
/// Potion Mastery heal.
pub async fn handle_item_use_complete(state: &MmoState, event: &PlayerItemUseCompleteEvent) {
    if !earns_xp(&event.player) || event.consumed_count == 0 {
        return;
    }
    let config = state.config();
    let alchemy = &config.enterprise.alchemy;
    let Some(xp) = alchemy
        .potion_xp
        .get(event.item_before.item.registry_key)
        .copied()
    else {
        return;
    };
    progression::award_xp(
        state,
        &event.player,
        SkillId::Alchemy,
        xp,
        XpSource::Consume,
    )
    .await;

    // Potion Mastery: heal when consuming a configured potion. Pure perk
    // effect; no XP beyond the award above.
    if !config.perks.enabled || !alchemy.heal_enabled || !skill_enabled(&config, SkillId::Alchemy) {
        return;
    }
    let level = perk_level(state, event.player.gameprofile.id, SkillId::Alchemy).await;
    let heal = potion_heal(alchemy, level);
    if heal > 0.0 {
        event.player.living_entity.heal(heal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_alchemy_rewards_only_explicit_potion_items() {
        let config = AlchemyConfig::default();

        assert_eq!(config.potion_xp.get("potion"), Some(&10));
        assert_eq!(config.potion_xp.get("apple"), None);
    }

    #[test]
    fn potion_heal_adds_a_step_per_tier() {
        let alchemy = AlchemyConfig::default();
        // Tier 0 matches the pre-tier base heal.
        assert!((potion_heal(&alchemy, 1) - 0.5).abs() < f32::EPSILON);
        // Tier 4 (level 100): 0.5 + 0.5*4 = 2.5.
        assert!((potion_heal(&alchemy, 100) - 2.5).abs() < f32::EPSILON);
    }
}
