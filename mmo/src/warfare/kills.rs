//! Exact kill attribution from Pumpkin's committed player-kill event.
//!
//! Pumpkin carries the lethal attack kind and weapon snapshot through death,
//! so Cabbage never consults a recent-hit cache or the player's current hand.

use pumpkin::plugin::api::events::entity::{AttackKind, PlayerKillEntityEvent};

use super::super::{
    MmoState,
    progression::{self, XpSource, earns_xp},
    skills::SkillId,
};
use super::{WeaponClass, classify_weapon};

pub async fn handle_player_kill(state: &MmoState, event: &PlayerKillEntityEvent) {
    if !earns_xp(&event.player) {
        return;
    }

    let victim_name = event.victim.get_entity().entity_type.resource_name;
    let Some(xp) = state.mob_xp_reward(victim_name) else {
        return;
    };

    let skill = match event.attribution.kind {
        AttackKind::Projectile => Some(SkillId::Archery),
        AttackKind::Melee => {
            event
                .attribution
                .weapon
                .as_ref()
                .and_then(|weapon| match classify_weapon(weapon) {
                    WeaponClass::Melee(skill) => Some(skill),
                    WeaponClass::Ranged | WeaponClass::Other => None,
                })
        }
        _ => None,
    };
    let Some(skill) = skill else {
        return;
    };

    progression::award_xp(state, &event.player, skill, xp, XpSource::EntityKill).await;
}
