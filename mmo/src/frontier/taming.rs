//! Taming activity: Cabbage pet profiles and owner-validated interactions.
//!
//! XP rules: one award per successful tame (the event's owner is
//! authoritative) and a small award per owner-validated feeding. Since the
//! six-skill consolidation the awards feed AnimalHandling, shared with the
//! husbandry activity. Entity data is never treated as proof of ownership:
//! feeding checks Pumpkin's actual tameable owner state
//! (`EntityBase::owner_uuid`).

use pumpkin::plugin::api::events::entity::{
    entity_feed::{EntityFeedCompleteEvent, FeedOutcome},
    entity_tame::EntityTameEvent,
};

use super::super::{
    MmoState,
    persistence::PetDataV1,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

/// The shared skill track this activity awards: taming and husbandry both
/// feed AnimalHandling since the six-skill consolidation.
pub(crate) const SKILL: SkillId = SkillId::AnimalHandling;

/// Record a Cabbage pet profile and award AnimalHandling XP when a player
/// tames an animal.
pub async fn handle_entity_tame(state: &MmoState, event: &EntityTameEvent) {
    if event.cancelled || !earns_xp(&event.owner) {
        return;
    }
    let config = state.config();
    let taming = &config.frontier.taming;
    let xp = taming
        .tame_xp
        .get(event.entity.get_entity().entity_type.resource_name)
        .copied()
        .unwrap_or(taming.default_tame_xp);

    // Record the Cabbage pet profile on the entity. This is a profile only —
    // ownership checks always consult Pumpkin's tameable owner state.
    let profile = PetDataV1::default();
    if let Err(error) = profile.write(state.context(), event.entity.as_ref()) {
        log::warn!("[Cabbage MMO] failed to record pet profile: {error}");
    }

    progression::award_xp(state, &event.owner, SKILL, xp, XpSource::Tame).await;
}

/// Award bond and XP when an owner feeds their own tamed pet.
pub async fn handle_feed_complete(state: &MmoState, event: &EntityFeedCompleteEvent) {
    let player = &event.player;
    if !earns_xp(player)
        || event.consumed_count == 0
        || !matches!(
            event.outcome,
            FeedOutcome::Healed | FeedOutcome::TrustIncreased
        )
    {
        return;
    }

    // Owner validation against Pumpkin's actual tameable owner state.
    if event.target.owner_uuid() != Some(player.gameprofile.id) {
        return;
    }

    let config = state.config();
    let taming = &config.frontier.taming;

    if event.item_before.item_count == 0
        || !taming
            .bond_food_items
            .iter()
            .any(|item| item == event.item_before.item.registry_key)
    {
        return;
    }

    // Bond increments only for entities carrying a Cabbage pet profile.
    let context = state.context().clone();
    let Some(mut profile) = PetDataV1::read(&context, event.target.as_ref()) else {
        return;
    };
    if profile.bond >= taming.bond_cap {
        return;
    }
    profile.bond += 1;
    if let Err(error) = profile.write(&context, event.target.as_ref()) {
        log::warn!("[Cabbage MMO] failed to update pet bond: {error}");
        return;
    }

    progression::award_xp(state, player, SKILL, taming.bond_feed_xp, XpSource::PetFeed).await;
}
