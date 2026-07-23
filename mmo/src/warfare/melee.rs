//! Blades, Axes, and Athletics (empty hand): melee classification and
//! bounded damage perks.
//!
//! Weapons are classified from the event's weapon snapshot and recorded for
//! kill attribution. Perk effects only ever *scale* the event's
//! `final_damage` within per-skill caps and the global damage cap; they never
//! cancel the attack or rewrite inventory state. Kill XP is awarded once via
//! `kills::handle_entity_death`, never here.

use pumpkin::plugin::api::events::player::player_attack::PlayerAttackDamageEvent;

use super::{
    super::{
        MmoState,
        perks::eligibility::perk_tier,
        progression::{earns_xp, perk_level},
        skills::SkillId,
    },
    WeaponClass, classify_weapon,
    config::{AxesConfig, BladesConfig, UnarmedConfig},
};

const RIPOSTE_COOLDOWN_KEY: &str = "blades.riposte";

/// Total Axes damage bonus: the per-level multiplier clamped by the cap,
/// which gains a step per perk tier.
fn axes_damage_bonus(axes: &AxesConfig, level: u32) -> f64 {
    (axes.damage_bonus_per_level * f64::from(level))
        .min(axes.damage_bonus_cap + axes.damage_cap_per_tier * f64::from(perk_tier(level)))
}

/// Empty-hand knockback bonus: the per-level multiplier clamped by the cap,
/// which gains a step per perk tier.
fn unarmed_knockback_bonus(unarmed: &UnarmedConfig, level: u32) -> f64 {
    (unarmed.knockback_bonus_per_level * f64::from(level))
        .min(unarmed.knockback_cap + unarmed.knockback_cap_per_tier * f64::from(perk_tier(level)))
}

/// Riposte damage multiplier: the base bonus plus a step per perk tier.
fn riposte_multiplier(blades: &BladesConfig, level: u32) -> f64 {
    blades.riposte_bonus_multiplier
        + blades.riposte_multiplier_per_tier * f64::from(perk_tier(level))
}

/// Riposte cooldown in ticks: reduced by a step per perk tier, saturating
/// at zero.
fn riposte_cooldown_ticks(blades: &BladesConfig, level: u32) -> u32 {
    blades.riposte_cooldown_ticks.saturating_sub(
        blades
            .riposte_cooldown_reduction_per_tier
            .saturating_mul(perk_tier(level)),
    )
}

/// Apply melee classification and bounded damage perks to an attack.
pub async fn handle_attack_damage(state: &MmoState, event: &mut PlayerAttackDamageEvent) {
    if event.cancelled || !earns_xp(&event.player) {
        return;
    }

    let player = &event.player;
    let weapon = classify_weapon(&event.weapon);
    let WeaponClass::Melee(skill) = weapon else {
        return;
    };

    let current_tick = state.current_tick();
    let player_uuid = player.gameprofile.id;
    let config = state.config();
    if !config.perks.enabled {
        return;
    }
    let warfare = &config.warfare;

    let level = perk_level(state, player_uuid, skill).await;

    // Bounded skill damage bonus. The Axes cap gains a step per perk tier;
    // Blades and Athletics scale through their other perks instead.
    let bonus = match skill {
        SkillId::Blades => (warfare.blades.damage_bonus_per_level * level as f64)
            .min(warfare.blades.damage_bonus_cap),
        SkillId::Axes => axes_damage_bonus(&warfare.axes, level),
        SkillId::Athletics => (warfare.unarmed.damage_bonus_per_level * level as f64)
            .min(warfare.unarmed.damage_bonus_cap),
        _ => 0.0,
    };
    if bonus > 0.0 {
        event.final_damage *= 1.0 + bonus as f32;
    }

    // Empty-hand knockback bonus, gated on the merged Athletics level and
    // bounded by its own cap.
    if skill == SkillId::Athletics {
        let knockback_bonus = unarmed_knockback_bonus(&warfare.unarmed, level);
        if knockback_bonus > 0.0 {
            event.knockback_multiplier *= 1.0 + knockback_bonus as f32;
        }
    }

    // Riposte: bonus damage when striking shortly after taking damage.
    if skill == SkillId::Blades && warfare.blades.riposte_enabled {
        let recent_damage = state.warfare().last_damage_taken_at(player_uuid);
        let in_window = recent_damage.is_some_and(|at_tick| {
            current_tick - at_tick <= warfare.blades.riposte_window_ticks as i32
        });
        if in_window
            && state.perk_cooldowns().try_activate(
                player_uuid,
                RIPOSTE_COOLDOWN_KEY,
                current_tick,
                riposte_cooldown_ticks(&warfare.blades, level),
            )
        {
            event.final_damage *= 1.0 + riposte_multiplier(&warfare.blades, level) as f32;
        }
    }

    // Global cap: the final damage must stay within the configured bound.
    let max_final = event.base_damage * config.perks.max_damage_multiplier as f32;
    event.final_damage = event.final_damage.min(max_final);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_damage_bonus_tier_zero_matches_pre_tier_value() {
        let axes = AxesConfig::default();
        // Below the cap the per-level bonus applies unchanged.
        assert!((axes_damage_bonus(&axes, 5) - 0.025).abs() < f64::EPSILON);
        // The old cap still clamps at tier 0: 0.1*9 = 0.9 → 0.6.
        let mut steep = axes.clone();
        steep.damage_bonus_per_level = 0.1;
        assert!((axes_damage_bonus(&steep, 9) - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn axes_damage_bonus_tier_four_raises_the_cap() {
        let axes = AxesConfig::default();
        // Level 100 (tier 4): 0.005*100 = 0.5, under the raised 0.8 cap.
        assert!((axes_damage_bonus(&axes, 100) - 0.5).abs() < f64::EPSILON);
        // The raised cap clamps: 0.005*200 = 1.0 → 0.6 + 0.05*4 = 0.8.
        assert!((axes_damage_bonus(&axes, 200) - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn unarmed_knockback_bonus_scales_with_tiers() {
        let unarmed = UnarmedConfig::default();
        // Tier 0: the per-level bonus applies unchanged.
        assert!((unarmed_knockback_bonus(&unarmed, 5) - 0.02).abs() < f64::EPSILON);
        // Tier 4 (level 100+): 0.004*200 = 0.8 → 0.5 + 0.05*4 = 0.7.
        assert!((unarmed_knockback_bonus(&unarmed, 200) - 0.7).abs() < f64::EPSILON);
        // The tier-0 cap still clamps when the per-level bonus exceeds it.
        let mut steep = unarmed.clone();
        steep.knockback_bonus_per_level = 0.1;
        assert!((unarmed_knockback_bonus(&steep, 9) - 0.5).abs() < f64::EPSILON);
        assert!((unarmed_knockback_bonus(&steep, 100) - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn riposte_multiplier_adds_a_step_per_tier() {
        let blades = BladesConfig::default();
        // Tier 0 matches the pre-tier value.
        for level in [1, 5, 9] {
            assert!(
                (riposte_multiplier(&blades, level) - 0.25).abs() < f64::EPSILON,
                "level {level}"
            );
        }
        // Tier 4 (level 100): 0.25 + 0.05*4 = 0.45.
        assert!((riposte_multiplier(&blades, 100) - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn riposte_cooldown_drops_a_step_per_tier_and_saturates() {
        let blades = BladesConfig::default();
        // Tier 0 matches the pre-tier value.
        assert_eq!(riposte_cooldown_ticks(&blades, 1), 200);
        // Tier 4 (level 100): 200 - 20*4 = 120.
        assert_eq!(riposte_cooldown_ticks(&blades, 100), 120);
        // The reduction saturates at zero instead of wrapping.
        let mut short = blades.clone();
        short.riposte_cooldown_ticks = 30;
        assert_eq!(riposte_cooldown_ticks(&short, 100), 0);
    }
}
