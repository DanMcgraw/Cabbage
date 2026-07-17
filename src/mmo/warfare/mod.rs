//! Warfare branch skill handlers, state, and configuration.
//!
//! Warfare skills: Blades, Axes, Archery, Unarmed, Defense, Acrobatics,
//! Sorcery. Melee weapons are classified from the attack event's weapon
//! snapshot, never inferred later from a possibly changed inventory. Kill XP
//! is awarded exactly once through Pumpkin's authoritative
//! `PlayerKillEntityEvent`; projectile provenance retained here is used for
//! per-hit Archery XP, not death inference.

pub(crate) mod archery;
pub(crate) mod config;
pub(crate) mod defense;
pub(crate) mod kills;
pub(crate) mod melee;
pub(crate) mod sorcery;

use std::{collections::HashMap, sync::Mutex};

use pumpkin_data::item_stack::ItemStack;
use uuid::Uuid;

use super::skills::SkillId;

/// How long recent damage state remains valid for Riposte, in ticks.
pub(crate) const RECENT_ATTACK_WINDOW_TICKS: i32 = 100;

/// How long a projectile→shooter record is kept, in ticks.
const PROJECTILE_RECORD_TTL_TICKS: i32 = 600;

/// Weapon classification derived from the attack event's weapon snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WeaponClass {
    /// A melee weapon mapped to a Warfare skill (or an empty hand → Unarmed).
    Melee(SkillId),
    /// Bow or crossbow; XP flows through the projectile path.
    Ranged,
    /// Anything else (tools, blocks, ...). No Warfare XP.
    Other,
}

/// Classify the held weapon from an event's item snapshot.
pub(crate) fn classify_weapon(stack: &ItemStack) -> WeaponClass {
    if stack.item_count == 0 {
        return WeaponClass::Melee(SkillId::Unarmed);
    }
    let key = stack.item.registry_key;
    if key.ends_with("_sword") {
        WeaponClass::Melee(SkillId::Blades)
    } else if key.ends_with("_axe") {
        WeaponClass::Melee(SkillId::Axes)
    } else if key == "bow" || key == "crossbow" {
        WeaponClass::Ranged
    } else {
        WeaponClass::Other
    }
}

/// In-memory Warfare state: attack records for kill attribution and Sorcery
/// mana. Volatile by design; a restart clears everything.
#[derive(Debug, Default)]
pub(crate) struct WarfareState {
    /// projectile entity → shooter player.
    projectile_owners: Mutex<HashMap<Uuid, (Uuid, i32)>>,
    /// player → tick of the most recent damage taken (for Riposte).
    last_damage_taken: Mutex<HashMap<Uuid, i32>>,
    /// player → (mana, tick of last update). Regen is computed lazily.
    sorcery_mana: Mutex<HashMap<Uuid, (f64, i32)>>,
}

impl WarfareState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_projectile_owner(&self, projectile: Uuid, shooter: Uuid, tick: i32) {
        if let Ok(mut owners) = self.projectile_owners.lock() {
            owners.insert(projectile, (shooter, tick));
        }
    }

    pub fn projectile_owner(&self, projectile: Uuid, tick: i32) -> Option<Uuid> {
        self.projectile_owners
            .lock()
            .ok()
            .and_then(|owners| owners.get(&projectile).copied())
            .filter(|(_, at_tick)| tick - at_tick <= PROJECTILE_RECORD_TTL_TICKS)
            .map(|(shooter, _)| shooter)
    }

    pub fn record_damage_taken(&self, player: Uuid, tick: i32) {
        if let Ok(mut taken) = self.last_damage_taken.lock() {
            taken.insert(player, tick);
        }
    }

    pub fn last_damage_taken_at(&self, player: Uuid) -> Option<i32> {
        self.last_damage_taken
            .lock()
            .ok()
            .and_then(|taken| taken.get(&player).copied())
    }

    /// Current mana for a player, applying lazy regen since the last update.
    pub fn current_mana(&self, player: Uuid, tick: i32, max: f64, regen_per_tick: f64) -> f64 {
        let mut entry = self.sorcery_mana.lock().ok();
        let Some(mana) = entry.as_mut() else {
            return max;
        };
        let (stored, at_tick) = mana.get(&player).copied().unwrap_or((max, tick));
        let elapsed = tick.saturating_sub(at_tick).max(0) as f64;
        let current = (stored + elapsed * regen_per_tick).min(max);
        mana.insert(player, (current, tick));
        current
    }

    pub fn set_mana(&self, player: Uuid, value: f64, tick: i32) {
        if let Ok(mut mana) = self.sorcery_mana.lock() {
            mana.insert(player, (value, tick));
        }
    }

    /// Drop stale records so the maps cannot grow without bound.
    pub fn sweep(&self, tick: i32) {
        if let Ok(mut owners) = self.projectile_owners.lock() {
            owners.retain(|_, (_, at_tick)| tick - *at_tick <= PROJECTILE_RECORD_TTL_TICKS);
        }
        if let Ok(mut taken) = self.last_damage_taken.lock() {
            taken.retain(|_, at_tick| tick - *at_tick <= RECENT_ATTACK_WINDOW_TICKS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projectile_records_resolve_owner_and_hits() {
        let state = WarfareState::new();
        let shooter = Uuid::new_v4();
        let projectile = Uuid::new_v4();
        state.record_projectile_owner(projectile, shooter, 0);
        assert_eq!(state.projectile_owner(projectile, 20), Some(shooter));
    }

    #[test]
    fn mana_regenerates_lazily_and_caps_at_max() {
        let state = WarfareState::new();
        let player = Uuid::new_v4();
        state.set_mana(player, 50.0, 0);
        assert_eq!(state.current_mana(player, 100, 100.0, 0.1), 60.0);
        assert_eq!(state.current_mana(player, 10_000, 100.0, 0.1), 100.0);
    }

    #[test]
    fn sweep_drops_stale_records() {
        let state = WarfareState::new();
        let player = Uuid::new_v4();
        state.record_projectile_owner(Uuid::new_v4(), player, 0);
        state.sweep(10_000);
        assert!(state.projectile_owners.lock().unwrap().is_empty());
    }
}
