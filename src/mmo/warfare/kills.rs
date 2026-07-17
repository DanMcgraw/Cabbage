//! Kill attribution: award kill XP exactly once, from `EntityDeathEvent`.
//!
//! Attribution rules (see plan Warfare rules):
//! - Killer is a player: use their most recent *recorded* attack — melee
//!   snapshot classification for melee skills, or a recent projectile hit
//!   for Archery. Never inspect the killer's possibly changed inventory.
//! - Killer is a projectile entity: resolve the shooter recorded at launch.
//! - No fresh record means no award (conservative by design).

use std::sync::Arc;

use pumpkin::{plugin::api::events::entity::entity_death::EntityDeathEvent, server::Server};

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp, find_player_by_uuid},
    skills::SkillId,
};

/// Award kill XP for a mob death attributed to a weapon skill.
pub async fn handle_entity_death(state: &MmoState, server: &Arc<Server>, event: &EntityDeathEvent) {
    let Some(killer) = event.killer.as_ref() else {
        return;
    };

    let victim_name = event.entity.get_entity().entity_type.resource_name;
    let Some(xp) = state.mob_xp_reward(victim_name) else {
        return;
    };

    let killer_entity = killer.get_entity();
    let killer_uuid = killer_entity.entity_uuid;
    let victim_uuid = event.entity.get_entity().entity_uuid;
    let current_tick = state.current_tick();

    let (player_uuid, skill) = if killer_entity.entity_type
        == &pumpkin_data::entity::EntityType::PLAYER
    {
        // Melee record first, then a projectile hit on this victim.
        let skill = state
            .warfare()
            .recent_melee_skill(killer_uuid, current_tick)
            .or_else(|| {
                state
                    .warfare()
                    .recent_projectile_hit(victim_uuid, killer_uuid, current_tick)
                    .then_some(SkillId::Archery)
            });
        let Some(skill) = skill else {
            return;
        };
        (killer_uuid, skill)
    } else {
        // Projectile kill: resolve the recorded shooter.
        let Some(shooter_uuid) = state.warfare().projectile_owner(killer_uuid, current_tick) else {
            return;
        };
        (shooter_uuid, SkillId::Archery)
    };

    let Some(player) = find_player_by_uuid(server, player_uuid) else {
        return;
    };
    if !earns_xp(&player) {
        return;
    }
    progression::award_xp(state, &player, skill, xp, XpSource::EntityKill).await;
}
