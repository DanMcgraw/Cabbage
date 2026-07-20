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
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

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
    if config.perks.enabled && fishing.reel_exp_bonus > 0 {
        event.exp_to_drop = event.exp_to_drop.saturating_add(fishing.reel_exp_bonus);
    }

    progression::award_xp(state, player, SkillId::Fishing, xp, XpSource::Fishing).await;
}
