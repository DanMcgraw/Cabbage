//! Archery: projectile provenance and hit-based XP.
//!
//! Arrows are tagged with their shooter at launch (the event's player is
//! authoritative), and hits award bounded XP through that provenance. Kill XP
//! for projectile kills flows once through `kills::handle_entity_death`.

use pumpkin::{
    plugin::api::events::entity::{
        AttackKind, EntityDamageByEntityEvent, entity_shoot_bow::EntityShootBowEvent,
        projectile_hit::ProjectileHitEvent,
    },
    server::Server,
};
use std::sync::Arc;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, find_player_by_uuid, perk_level},
    skills::SkillId,
    ui::skill_enabled,
};
use super::{WeaponClass, classify_weapon, config::ArcheryConfig};

/// Arrow damage multiplier: `1.0 +` the per-level bonus, the bonus clamped
/// by the cap, which gains a step per perk tier.
fn archery_damage_multiplier(archery: &ArcheryConfig, level: u32) -> f64 {
    let bonus = (archery.damage_bonus_per_level * f64::from(level))
        .min(archery.damage_bonus_cap + archery.damage_cap_per_tier * f64::from(perk_tier(level)));
    1.0 + bonus
}

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

/// Scale arrow damage by the shooter player's Archery level.
///
/// Projectile-only by construction: the attribution kind must be
/// `Projectile` and the captured weapon a bow/crossbow, so melee hits (which
/// flow through `PlayerAttackDamageEvent`) are never scaled twice.
pub async fn handle_entity_damage_by_entity(
    state: &MmoState,
    event: &mut EntityDamageByEntityEvent,
) {
    if event.cancelled || event.attribution.kind != AttackKind::Projectile {
        return;
    }
    let Some(player) = event.attribution.attacking_player.as_ref() else {
        return;
    };
    if !earns_xp(player) {
        return;
    }
    let is_bow = event
        .attribution
        .weapon
        .as_ref()
        .is_some_and(|weapon| classify_weapon(weapon) == WeaponClass::Ranged);
    if !is_bow {
        return;
    }

    let config = state.config();
    let archery = &config.warfare.archery;
    if !config.perks.enabled || !archery.damage_enabled || !skill_enabled(&config, SkillId::Archery)
    {
        return;
    }

    let level = perk_level(state, player.gameprofile.id, SkillId::Archery).await;
    event.final_damage *= archery_damage_multiplier(archery, level) as f32;

    // Global cap, mirroring melee: the final damage stays within the
    // configured bound of the event's raw damage.
    let max_final = event.damage * config.perks.max_damage_multiplier as f32;
    event.final_damage = event.final_damage.min(max_final);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archery_damage_multiplier_tier_zero_matches_pre_tier_value() {
        let archery = ArcheryConfig::default();
        // Level 1: 1.0 + 0.004.
        assert!((archery_damage_multiplier(&archery, 1) - 1.004).abs() < f64::EPSILON);
        // The old 0.5 cap still clamps at tier 0 (steep per-level value so
        // the cap is reached below level 10).
        let mut steep = archery.clone();
        steep.damage_bonus_per_level = 0.1;
        assert!((archery_damage_multiplier(&steep, 9) - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn archery_damage_multiplier_tier_four_raises_the_cap() {
        let archery = ArcheryConfig::default();
        // Level 100 (tier 4): 0.004*100 = 0.4, under the raised 0.7 cap.
        assert!((archery_damage_multiplier(&archery, 100) - 1.4).abs() < f64::EPSILON);
        // The raised cap clamps: 0.004*200 = 0.8 → 1.0 + (0.5 + 0.05*4) = 1.7.
        assert!((archery_damage_multiplier(&archery, 200) - 1.7).abs() < f64::EPSILON);
    }

    #[test]
    fn archery_damage_multiplier_never_drops_below_one() {
        let mut archery = ArcheryConfig::default();
        archery.damage_bonus_per_level = 0.0;
        assert!((archery_damage_multiplier(&archery, 100) - 1.0).abs() < f64::EPSILON);
    }
}
