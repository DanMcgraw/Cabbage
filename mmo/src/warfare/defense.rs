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
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, find_player_by_uuid, perk_level},
    skills::SkillId,
};
use super::config::{AcrobaticsConfig, DefenseConfig};

/// The shared skill track for fall damage: acrobatics and empty-hand combat
/// both feed Athletics since the six-skill consolidation.
pub(crate) const FALL_SKILL: SkillId = SkillId::Athletics;

/// Athletics roll reduction: the per-level multiplier clamped by the cap,
/// which gains a step per perk tier.
fn roll_reduction(acrobatics: &AcrobaticsConfig, level: u32) -> f64 {
    (acrobatics.roll_reduction_per_level * f64::from(level)).min(
        acrobatics.roll_reduction_cap
            + acrobatics.roll_reduction_cap_per_tier * f64::from(perk_tier(level)),
    )
}

/// Defense resilience reduction: the per-level multiplier clamped by the
/// cap, which gains a step per perk tier.
fn resilience_reduction(defense: &DefenseConfig, level: u32) -> f64 {
    (defense.reduction_per_level * f64::from(level))
        .min(defense.reduction_cap + defense.reduction_cap_per_tier * f64::from(perk_tier(level)))
}

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
        let level = perk_level(state, player_uuid, FALL_SKILL).await;
        let reduction = roll_reduction(&warfare.acrobatics, level);
        event.final_damage *= 1.0 - reduction as f32;
    }
    let level = perk_level(state, player_uuid, SkillId::Defense).await;
    let reduction = resilience_reduction(&warfare.defense, level);
    if reduction > 0.0 {
        event.final_damage *= 1.0 - reduction as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_reduction_tier_zero_matches_pre_tier_value() {
        let acrobatics = AcrobaticsConfig::default();
        // Below the cap the per-level reduction applies unchanged.
        assert!((roll_reduction(&acrobatics, 5) - 0.01).abs() < f64::EPSILON);
        // The old 0.25 cap still clamps at tier 0 (steep per-level value so
        // the cap is reached below level 10).
        let mut steep = acrobatics.clone();
        steep.roll_reduction_per_level = 0.1;
        assert!((roll_reduction(&steep, 9) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn roll_reduction_tier_four_raises_the_cap() {
        let acrobatics = AcrobaticsConfig::default();
        // 0.002*300 = 0.6 → 0.25 + 0.05*4 = 0.45.
        assert!((roll_reduction(&acrobatics, 300) - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn resilience_reduction_tier_zero_matches_pre_tier_value() {
        let defense = DefenseConfig::default();
        assert!((resilience_reduction(&defense, 5) - 0.0075).abs() < f64::EPSILON);
        let mut steep = defense.clone();
        steep.reduction_per_level = 0.1;
        assert!((resilience_reduction(&steep, 9) - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn resilience_reduction_tier_four_raises_the_cap() {
        let defense = DefenseConfig::default();
        // 0.0015*200 = 0.3 → 0.15 + 0.025*4 = 0.25.
        assert!((resilience_reduction(&defense, 200) - 0.25).abs() < f64::EPSILON);
    }
}
