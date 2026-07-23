//! Fishing skill: catch XP, the reel perk, and configurable treasure
//! replacement.
//!
//! XP rule: one Fishing award per successful catch, valued by the caught
//! item's registry key (falling back to a default). Treasure replacement is
//! opt-in via config and never touches the catch otherwise.

use pumpkin::plugin::api::events::player::fish::{PlayerFishEvent, PlayerFishState};
use pumpkin_data::{item::Item, item_stack::ItemStack};

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};

/// Reel perk experience bonus: the base bonus plus the per-tier step.
fn reel_exp_bonus(fishing: &super::config::FishingConfig, level: u32) -> i32 {
    fishing.reel_exp_bonus.saturating_add(
        fishing
            .reel_exp_bonus_per_tier
            .saturating_mul(perk_tier(level) as i32),
    )
}

/// Award Fishing XP on a successful catch and apply reel/treasure perks.
pub async fn handle_player_fish(state: &MmoState, event: &mut PlayerFishEvent) {
    if event.state != PlayerFishState::CaughtFish || event.cancelled {
        return;
    }
    let player = &event.player;
    if !earns_xp(player) {
        return;
    }
    let config = state.config();
    let fishing = &config.frontier.fishing;

    let mut xp = fishing.default_catch_xp;
    if let Some(caught) = event.caught_item.as_ref() {
        let key = caught.item.registry_key;
        xp = fishing.catch_xp.get(key).copied().unwrap_or(xp);

        // Treasure replacement: swap a configured caught item for its mapped
        // replacement, preserving the stack count.
        if config.perks.enabled
            && let Some(replacement) = fishing.treasure_replacements.get(key)
            && let Some(item) = Item::from_registry_key(replacement)
        {
            event.caught_item = Some(ItemStack::new(caught.item_count, item));
        }
    }

    // Reel perk: extra vanilla experience on a successful catch.
    if config.perks.enabled {
        let level = perk_level(state, player.gameprofile.id, SkillId::Fishing).await;
        let bonus = reel_exp_bonus(fishing, level);
        if bonus > 0 {
            event.exp_to_drop = event.exp_to_drop.saturating_add(bonus);
        }
    }

    progression::award_xp(state, player, SkillId::Fishing, xp, XpSource::Fishing).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reel_exp_bonus_scales_with_tier() {
        let fishing = super::super::config::FishingConfig::default();

        // Tier 0 adds nothing: the pre-tier base bonus (2).
        assert_eq!(reel_exp_bonus(&fishing, 1), 2);
        assert_eq!(reel_exp_bonus(&fishing, 9), 2);
        // Tier 4 at level 100: 2 + 4 * 1 = 6.
        assert_eq!(reel_exp_bonus(&fishing, 100), 6);
        assert_eq!(reel_exp_bonus(&fishing, 50), 5);
    }

    #[test]
    fn reel_exp_bonus_saturates_instead_of_overflowing() {
        let fishing = super::super::config::FishingConfig {
            reel_exp_bonus: i32::MAX,
            reel_exp_bonus_per_tier: i32::MAX,
            ..super::super::config::FishingConfig::default()
        };

        assert_eq!(reel_exp_bonus(&fishing, 100), i32::MAX);
    }
}
