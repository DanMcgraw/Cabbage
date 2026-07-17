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
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};

const SORCERY_COOLDOWN_KEY: &str = "sorcery.cast";

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
    let mana = state.warfare().current_mana(
        player_uuid,
        current_tick,
        sorcery.mana_max,
        sorcery.mana_regen_per_tick,
    );
    if mana < sorcery.spell_mana_cost {
        player
            .show_title(
                &TextComponent::text(format!("Not enough mana ({mana:.0}/{})", sorcery.mana_max))
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
        sorcery.spell_cooldown_ticks,
    ) {
        return;
    }

    state
        .warfare()
        .set_mana(player_uuid, mana - sorcery.spell_mana_cost, current_tick);

    cast_healing_bolt(player, sorcery.spell_heal).await;
    progression::award_xp(
        state,
        player,
        SkillId::Sorcery,
        sorcery.spell_xp,
        XpSource::Melee,
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
