//! Blades, Axes, and Unarmed: melee classification and bounded damage perks.
//!
//! Weapons are classified from the event's weapon snapshot and recorded for
//! kill attribution. Perk effects only ever *scale* the event's
//! `final_damage` within per-skill caps and the global damage cap; they never
//! cancel the attack or rewrite inventory state. Kill XP is awarded once via
//! `kills::handle_entity_death`, never here.

use pumpkin::{
    entity::player::Player, plugin::api::events::player::player_attack::PlayerAttackDamageEvent,
};

use super::{
    super::{MmoState, progression::earns_xp, skills::SkillId},
    WeaponClass, classify_weapon,
};

const RIPOSTE_COOLDOWN_KEY: &str = "blades.riposte";

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
    state
        .warfare()
        .record_melee_attack(player_uuid, skill, current_tick);

    let config = state.config();
    if !config.perks.enabled {
        return;
    }
    let warfare = &config.warfare;

    let level = player_level(state, player, skill).await;

    // Bounded skill damage bonus.
    let (per_level, cap) = match skill {
        SkillId::Blades => (
            warfare.blades.damage_bonus_per_level,
            warfare.blades.damage_bonus_cap,
        ),
        SkillId::Axes => (
            warfare.axes.damage_bonus_per_level,
            warfare.axes.damage_bonus_cap,
        ),
        SkillId::Unarmed => (
            warfare.unarmed.damage_bonus_per_level,
            warfare.unarmed.damage_bonus_cap,
        ),
        _ => (0.0, 0.0),
    };
    let bonus = (per_level * level as f64).min(cap);
    if bonus > 0.0 {
        event.final_damage *= 1.0 + bonus as f32;
    }

    // Unarmed knockback bonus, bounded by its own cap.
    if skill == SkillId::Unarmed {
        let knockback_bonus = (warfare.unarmed.knockback_bonus_per_level * level as f64)
            .min(warfare.unarmed.knockback_cap);
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
                warfare.blades.riposte_cooldown_ticks,
            )
        {
            event.final_damage *= 1.0 + warfare.blades.riposte_bonus_multiplier as f32;
        }
    }

    // Global cap: the final damage must stay within the configured bound.
    let max_final = event.base_damage * config.perks.max_damage_multiplier as f32;
    event.final_damage = event.final_damage.min(max_final);
}

async fn player_level(state: &MmoState, player: &Player, skill: SkillId) -> u32 {
    let curve = state.curve(skill);
    state
        .db()
        .get_skill(player.gameprofile.id, skill)
        .await
        .map(|data| curve.level_for_xp(data.xp).0)
        .unwrap_or(1)
}
