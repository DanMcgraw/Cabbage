//! Tinkering: mechanism crafting XP.
//!
//! Custom-item provenance already rides on every Cabbage-marked item via the
//! shared `item_v1` codec. "Small, contained receiver behavior" beyond
//! provenance waits for protected UI primitives; mechanism crafting XP is
//! the shippable slice.

use pumpkin::plugin::api::events::player::craft_item::CraftItemEvent;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

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
    progression::award_xp(
        state,
        &event.player,
        SkillId::Tinkering,
        xp,
        XpSource::Craft,
    )
    .await;
}
