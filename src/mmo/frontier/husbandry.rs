//! Husbandry skill: breeding XP and animal-product XP.
//!
//! XP rules: one Husbandry award per bred animal (attributed to the breeder
//! from the event) and per collected animal product (right tool on the right
//! animal). Trait rolls are blocked: `EntityBreedEvent` does not expose the
//! baby entity, so traits cannot be attached — see the platform gaps in
//! `src/mmo/plan.md`.

use pumpkin::plugin::api::events::{
    entity::entity_breed::EntityBreedEvent,
    player::player_interact_entity_event::PlayerInteractEntityEvent,
};
use pumpkin_protocol::java::server::play::ActionType;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Award Husbandry XP to the breeder when two animals produce a baby.
pub async fn handle_entity_breed(state: &MmoState, event: &EntityBreedEvent) {
    let Some(breeder) = event.breeder.as_ref() else {
        return;
    };
    if event.cancelled || !earns_xp(breeder) {
        return;
    }
    let config = state.config();
    let husbandry = &config.frontier.husbandry;
    let xp = husbandry
        .breed_xp
        .get(event.baby_type.resource_name)
        .copied()
        .unwrap_or(husbandry.default_breed_xp);

    progression::award_xp(state, breeder, SkillId::Husbandry, xp, XpSource::Breed).await;
}

/// Award Husbandry XP for collecting animal products (bucket on a cow,
/// shears on a sheep, bowl on a mooshroom).
pub async fn handle_player_interact_entity(state: &MmoState, event: &PlayerInteractEntityEvent) {
    if event.cancelled || matches!(event.action, ActionType::Attack) {
        return;
    }
    let player = &event.player;
    if !earns_xp(player) {
        return;
    }
    let config = state.config();
    let husbandry = &config.frontier.husbandry;
    let entity_name = event.target.get_entity().entity_type.resource_name;
    let Some(product) = husbandry.product_xp.get(entity_name) else {
        return;
    };

    let held = player.inventory().held_item().lock().await.clone();
    if held.item_count == 0 || held.item.registry_key != product.held_item {
        return;
    }

    progression::award_xp(
        state,
        player,
        SkillId::Husbandry,
        product.xp,
        XpSource::Breed,
    )
    .await;
}
