//! Alchemy: XP for consuming potions.
//!
//! Brewing XP is not attributable (`BrewEvent` carries no player) and
//! potency/duration mutation of already-applied effects has no safe hook in
//! this Pumpkin build, so both stay documented as blocked in `plan.md`.

use pumpkin::plugin::api::events::player::player_item_use_complete::PlayerItemUseCompleteEvent;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Award Alchemy XP for consuming a configured potion.
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
}

#[cfg(test)]
mod tests {
    use super::super::config::AlchemyConfig;

    #[test]
    fn default_alchemy_rewards_only_explicit_potion_items() {
        let config = AlchemyConfig::default();

        assert_eq!(config.potion_xp.get("potion"), Some(&10));
        assert_eq!(config.potion_xp.get("apple"), None);
    }
}
