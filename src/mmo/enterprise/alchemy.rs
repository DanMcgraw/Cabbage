//! Alchemy: XP for consuming potions.
//!
//! Brewing XP is not attributable (`BrewEvent` carries no player) and
//! potency/duration mutation of already-applied effects has no safe hook in
//! this Pumpkin build, so both stay documented as blocked in `plan.md`.

use pumpkin::plugin::api::events::player::player_item_use_finish::PlayerItemUseFinishEvent;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Award Alchemy XP for consuming a configured potion.
pub async fn handle_item_use_finish(state: &MmoState, event: &PlayerItemUseFinishEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }
    let config = state.config();
    let alchemy = &config.enterprise.alchemy;
    let xp = alchemy
        .potion_xp
        .get(event.item.item.registry_key)
        .copied()
        .unwrap_or(alchemy.default_potion_xp);
    progression::award_xp(
        state,
        &event.player,
        SkillId::Alchemy,
        xp,
        XpSource::Consume,
    )
    .await;
}
