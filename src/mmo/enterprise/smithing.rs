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
    persistence::ItemDataV1,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

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
