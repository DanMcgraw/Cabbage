//! Husbandry skill: breeding XP and animal-product XP.
//!
//! XP rules: one Husbandry award per bred animal (attributed to the breeder
//! from the event) and per collected animal product (right tool on the right
//! animal). A bounded, configurable trait roll writes directly to the baby
//! exposed by `EntityBreedCompleteEvent`.

use pumpkin::plugin::api::events::entity::{
    entity_breed::EntityBreedCompleteEvent, entity_product::AnimalProductCollectCompleteEvent,
};
use rand::RngExt;

use super::super::{
    MmoState,
    persistence::PetDataV1,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// Award Husbandry XP to the breeder when two animals produce a baby.
pub async fn handle_entity_breed_complete(state: &MmoState, event: &EntityBreedCompleteEvent) {
    let Some(breeder) = event.breeder.as_ref() else {
        return;
    };
    if !earns_xp(breeder) {
        return;
    }
    let config = state.config();
    let husbandry = &config.frontier.husbandry;
    let xp = husbandry
        .breed_xp
        .get(event.baby.get_entity().entity_type.resource_name)
        .copied()
        .unwrap_or(husbandry.default_breed_xp);

    progression::award_xp(state, breeder, SkillId::Husbandry, xp, XpSource::Breed).await;

    if !config.perks.enabled || husbandry.traits.is_empty() {
        return;
    }
    let chance = husbandry
        .trait_roll_chance
        .min(config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }
    let index = rand::rng().random_range(0..husbandry.traits.len());
    let selected = husbandry.traits[index].clone();
    let mut profile = PetDataV1::read(state.context(), event.baby.as_ref()).unwrap_or_default();
    if profile.traits.contains(&selected) {
        return;
    }
    profile.traits.push(selected.clone());
    if let Err(error) = profile.write(state.context(), event.baby.as_ref()) {
        log::warn!("[Cabbage MMO] failed to write newborn trait: {error}");
        return;
    }
    state.audit(&format!(
        "quality roll: newborn {} received trait {} for {}",
        event.baby.get_entity().entity_uuid,
        selected,
        breeder.gameprofile.id
    ));
}

/// Award Husbandry XP for collecting animal products (bucket on a cow,
/// shears on a sheep, bowl on a mooshroom).
pub async fn handle_product_complete(state: &MmoState, event: &AnimalProductCollectCompleteEvent) {
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

    progression::award_xp(
        state,
        player,
        SkillId::Husbandry,
        product.xp,
        XpSource::AnimalProduct,
    )
    .await;
}
