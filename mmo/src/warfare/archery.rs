//! Archery: projectile provenance and hit-based XP.
//!
//! Arrows are tagged with their shooter at launch (the event's player is
//! authoritative), and hits award bounded XP through that provenance. Kill XP
//! for projectile kills flows once through `kills::handle_entity_death`.

use pumpkin::{
    plugin::api::events::entity::{
        entity_shoot_bow::EntityShootBowEvent, projectile_hit::ProjectileHitEvent,
    },
    server::Server,
};
use std::sync::Arc;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp, find_player_by_uuid},
    skills::SkillId,
};

/// Record the shooter of a fired projectile for later hit/kill attribution.
pub async fn handle_shoot_bow(state: &MmoState, event: &EntityShootBowEvent) {
    if event.cancelled {
        return;
    }
    let projectile_uuid = event.projectile.get_entity().entity_uuid;
    state.warfare().record_projectile_owner(
        projectile_uuid,
        event.player.gameprofile.id,
        state.current_tick(),
    );
}

/// Award hit-based Archery XP when a tracked projectile hits a target.
pub async fn handle_projectile_hit(
    state: &MmoState,
    server: &Arc<Server>,
    event: &ProjectileHitEvent,
) {
    if event.hit_entity.is_none() {
        return;
    }
    let projectile_uuid = event.projectile.get_entity().entity_uuid;
    let current_tick = state.current_tick();
    let Some(shooter_uuid) = state
        .warfare()
        .projectile_owner(projectile_uuid, current_tick)
    else {
        return;
    };

    let Some(shooter) = find_player_by_uuid(server, shooter_uuid) else {
        return;
    };
    if !earns_xp(&shooter) {
        return;
    }
    let xp = state.config().warfare.archery.hit_xp;
    progression::award_xp(state, &shooter, SkillId::Archery, xp, XpSource::Projectile).await;
}
