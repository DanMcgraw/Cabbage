//! Sorcery: mana state and the first staff activation path.
//!
//! Right-clicking with the configured staff casts a bounded healing bolt:
//! mana is checked and consumed, a cooldown is charged, and the effect runs
//! entirely on ordinary Pumpkin primitives (`heal`, particles, sounds).
//! Everything else (projectiles, damage spells, attribute effects) stays
//! disabled pending the platform capabilities listed in `plan.md`.

use pumpkin::{
    entity::{EntityBase, player::Player},
    plugin::api::events::player::player_interact_event::{InteractAction, PlayerInteractEvent},
};
use pumpkin_data::{
    particle::Particle,
    sound::{Sound, SoundCategory},
};
use pumpkin_util::text::{TextComponent, color::NamedColor};
use std::sync::Arc;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};
use super::config::SorceryConfig;

const SORCERY_COOLDOWN_KEY: &str = "sorcery.cast";

/// Maximum mana: the base pool plus a step per perk tier.
fn mana_max(sorcery: &SorceryConfig, level: u32) -> f64 {
    sorcery.mana_max + sorcery.mana_max_per_tier * f64::from(perk_tier(level))
}

/// Healing-bolt health restored: the base heal plus a step per perk tier.
fn spell_heal(sorcery: &SorceryConfig, level: u32) -> f32 {
    sorcery.spell_heal + sorcery.spell_heal_per_tier * perk_tier(level) as f32
}

/// Cast cooldown in ticks: reduced by a step per perk tier, saturating at
/// zero.
fn spell_cooldown_ticks(sorcery: &SorceryConfig, level: u32) -> u32 {
    sorcery.spell_cooldown_ticks.saturating_sub(
        sorcery
            .spell_cooldown_reduction_per_tier
            .saturating_mul(perk_tier(level)),
    )
}

/// Cast the healing bolt when a player right-clicks with the staff item.
pub async fn handle_player_interact(state: &MmoState, event: &PlayerInteractEvent) {
    if event.cancelled {
        return;
    }
    if !matches!(
        event.action,
        InteractAction::RightClickAir | InteractAction::RightClickBlock
    ) {
        return;
    }
    let player = &event.player;
    if !earns_xp(player) {
        return;
    }

    let config = state.config();
    if !config.perks.enabled {
        return;
    }
    let sorcery = &config.warfare.sorcery;

    let held = player.inventory().held_item().lock().await.clone();
    if held.item_count == 0 || held.item.registry_key != sorcery.staff_item {
        return;
    }

    let player_uuid = player.gameprofile.id;
    let current_tick = state.current_tick();
    let level = perk_level(state, player_uuid, SkillId::Sorcery).await;
    let mana_max = mana_max(sorcery, level);
    let mana = state.warfare().current_mana(
        player_uuid,
        current_tick,
        mana_max,
        sorcery.mana_regen_per_tick,
    );
    if mana < sorcery.spell_mana_cost {
        player
            .show_title(
                &TextComponent::text(format!("Not enough mana ({mana:.0}/{mana_max})"))
                    .color_named(NamedColor::Red),
                &pumpkin::entity::player::TitleMode::ActionBar,
            )
            .await;
        return;
    }

    if !state.perk_cooldowns().try_activate(
        player_uuid,
        SORCERY_COOLDOWN_KEY,
        current_tick,
        spell_cooldown_ticks(sorcery, level),
    ) {
        return;
    }

    state
        .warfare()
        .set_mana(player_uuid, mana - sorcery.spell_mana_cost, current_tick);

    cast_healing_bolt(player, spell_heal(sorcery, level)).await;
    progression::award_xp(
        state,
        player,
        SkillId::Sorcery,
        sorcery.spell_xp,
        XpSource::Cast,
    )
    .await;
}

async fn cast_healing_bolt(player: &Arc<Player>, heal_amount: f32) {
    if heal_amount > 0.0 {
        player.living_entity.heal(heal_amount);
    }
    let world = player.get_entity().world.load_full();
    let pos = player.get_entity().pos.load();
    world.play_sound(Sound::EntityPlayerLevelup, SoundCategory::Players, &pos);
    world.spawn_particle(
        pos + pumpkin_util::math::vector3::Vector3::new(0.0, 1.0, 0.0),
        pumpkin_util::math::vector3::Vector3::new(0.4, 0.6, 0.4),
        0.05,
        20,
        Particle::Heart,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mana_max_adds_a_step_per_tier() {
        let sorcery = SorceryConfig::default();
        // Tier 0 matches the pre-tier pool.
        assert!((mana_max(&sorcery, 1) - 100.0).abs() < f64::EPSILON);
        // Tier 4 (level 100): 100 + 10*4 = 140.
        assert!((mana_max(&sorcery, 100) - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn spell_heal_adds_a_step_per_tier() {
        let sorcery = SorceryConfig::default();
        // Tier 0 matches the pre-tier heal.
        assert!((spell_heal(&sorcery, 1) - 4.0).abs() < f32::EPSILON);
        // Tier 4 (level 100): 4 + 1*4 = 8.
        assert!((spell_heal(&sorcery, 100) - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spell_cooldown_drops_a_step_per_tier_and_saturates() {
        let sorcery = SorceryConfig::default();
        // Tier 0 matches the pre-tier cooldown.
        assert_eq!(spell_cooldown_ticks(&sorcery, 1), 100);
        // Tier 4 (level 100): 100 - 10*4 = 60.
        assert_eq!(spell_cooldown_ticks(&sorcery, 100), 60);
        // The reduction saturates at zero instead of wrapping.
        let mut short = sorcery.clone();
        short.spell_cooldown_ticks = 30;
        assert_eq!(spell_cooldown_ticks(&short, 100), 0);
    }
}
