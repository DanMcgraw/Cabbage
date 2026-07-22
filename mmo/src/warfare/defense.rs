//! Defense and fall (acrobatics) handling: damage-taken XP and bounded
//! damage reductions.
//!
//! Since the six-skill consolidation, fall XP and the safe-landing roll feed
//! and read the shared Athletics track. XP is computed from the incoming
//! damage *before* perk reductions, so a stronger defense never slows its own
//! progression. Reductions are multiplicative, capped per skill, and can
//! never amplify damage.

use pumpkin::{plugin::api::events::entity::entity_damage::EntityDamageEvent, server::Server};
use pumpkin_data::damage::DamageType;
use std::sync::Arc;

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp, find_player_by_uuid},
    skills::SkillId,
};

/// The shared skill track for fall damage: acrobatics and empty-hand combat
/// both feed Athletics since the six-skill consolidation.
pub(crate) const FALL_SKILL: SkillId = SkillId::Athletics;

/// Award damage-taken XP and apply bounded reductions when a player is hurt.
pub async fn handle_entity_damage(
    state: &MmoState,
    server: &Arc<Server>,
    event: &mut EntityDamageEvent,
) {
    if event.cancelled {
        return;
    }
    let entity = event.entity.get_entity();
    if entity.entity_type != &pumpkin_data::entity::EntityType::PLAYER {
        return;
    }
    let Some(player) = find_player_by_uuid(server, entity.entity_uuid) else {
        return;
    };
    if !earns_xp(&player) {
        return;
    }

    let player_uuid = player.gameprofile.id;
    let current_tick = state.current_tick();
    state
        .warfare()
        .record_damage_taken(player_uuid, current_tick);

    let config = state.config();
    let warfare = &config.warfare;
    let incoming = event.final_damage.max(0.0);
    if incoming <= 0.0 {
        return;
    }

    // Defense XP from any damage taken (before reductions).
    let defense_xp = (incoming as f64 * warfare.defense.xp_per_damage).round() as u64;
    let defense_xp = defense_xp.min(warfare.defense.xp_cap_per_hit);
    if defense_xp > 0 {
        progression::award_xp(
            state,
            &player,
            SkillId::Defense,
            defense_xp,
            XpSource::DamageTaken,
        )
        .await;
    }

    let is_fall = event.damage_type == DamageType::FALL;
    if is_fall {
        // Athletics XP from fall damage (before reductions).
        let fall_xp = (incoming as f64 * warfare.acrobatics.xp_per_fall_damage).round() as u64;
        let fall_xp = fall_xp.min(warfare.acrobatics.xp_cap_per_fall);
        if fall_xp > 0 {
            progression::award_xp(state, &player, FALL_SKILL, fall_xp, XpSource::Fall).await;
        }
    }

    if !config.perks.enabled {
        return;
    }

    // Bounded reductions: Athletics roll first, then Defense resilience.
    if is_fall {
        let level = player_level(state, &player, FALL_SKILL).await;
        let reduction = (warfare.acrobatics.roll_reduction_per_level * level as f64)
            .min(warfare.acrobatics.roll_reduction_cap);
        event.final_damage *= 1.0 - reduction as f32;
    }
    let level = player_level(state, &player, SkillId::Defense).await;
    let reduction =
        (warfare.defense.reduction_per_level * level as f64).min(warfare.defense.reduction_cap);
    if reduction > 0.0 {
        event.final_damage *= 1.0 - reduction as f32;
    }
}

async fn player_level(
    state: &MmoState,
    player: &Arc<pumpkin::entity::player::Player>,
    skill: SkillId,
) -> u32 {
    let curve = state.curve(skill);
    state
        .db()
        .get_skill(player.gameprofile.id, skill)
        .await
        .map(|data| curve.level_for_xp(data.xp).0)
        .unwrap_or(1)
}
