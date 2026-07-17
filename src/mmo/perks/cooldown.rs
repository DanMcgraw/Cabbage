//! Tick-based cooldown tracking for perk activations.
//!
//! Cooldowns are keyed by player UUID plus a static perk key, and compare
//! against the server tick counter the MMO module already tracks. They are
//! in-memory only: a restart clears them, which is acceptable for bounded
//! perk effects and avoids stale-lockout states after crashes.

// Consumed by perk handlers landing in Phases 1-3.
#![allow(dead_code)]

use std::{collections::HashMap, sync::Mutex};

use uuid::Uuid;

/// Tracks per-player, per-perk cooldowns in server ticks.
#[derive(Debug, Default)]
pub struct CooldownTracker {
    ready_at_tick: Mutex<HashMap<(Uuid, &'static str), i32>>,
}

impl CooldownTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remaining ticks before the perk can activate again (0 if ready).
    pub fn remaining_ticks(&self, player: Uuid, perk: &'static str, current_tick: i32) -> i32 {
        self.ready_at_tick
            .lock()
            .ok()
            .and_then(|ready| ready.get(&(player, perk)).copied())
            .map(|ready_at| ready_at.saturating_sub(current_tick).max(0))
            .unwrap_or(0)
    }

    /// Try to activate a perk. Returns `true` and starts the cooldown when
    /// the perk was ready; returns `false` while still on cooldown.
    pub fn try_activate(
        &self,
        player: Uuid,
        perk: &'static str,
        current_tick: i32,
        cooldown_ticks: u32,
    ) -> bool {
        if self.remaining_ticks(player, perk, current_tick) > 0 {
            return false;
        }
        if let Ok(mut ready) = self.ready_at_tick.lock() {
            ready.insert(
                (player, perk),
                current_tick.saturating_add(cooldown_ticks as i32),
            );
        }
        true
    }

    /// Drop all cooldowns for a player (e.g. on disconnect).
    #[allow(dead_code)] // wired to PlayerLeaveEvent in a later phase
    pub fn clear_player(&self, player: Uuid) {
        if let Ok(mut ready) = self.ready_at_tick.lock() {
            ready.retain(|(uuid, _), _| *uuid != player);
        }
    }

    /// Sweep expired entries so the map cannot grow without bound on a
    /// long-running server. Cheap enough to run from a periodic tick task.
    #[allow(dead_code)] // wired to a periodic sweep when perks land
    pub fn sweep_expired(&self, current_tick: i32) {
        if let Ok(mut ready) = self.ready_at_tick.lock() {
            ready.retain(|_, ready_at| *ready_at > current_tick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_starts_cooldown() {
        let tracker = CooldownTracker::new();
        let player = Uuid::new_v4();
        assert!(tracker.try_activate(player, "test.perk", 100, 50));
        assert!(!tracker.try_activate(player, "test.perk", 120, 50));
        assert_eq!(tracker.remaining_ticks(player, "test.perk", 120), 30);
    }

    #[test]
    fn cooldown_expires() {
        let tracker = CooldownTracker::new();
        let player = Uuid::new_v4();
        assert!(tracker.try_activate(player, "test.perk", 100, 50));
        assert_eq!(tracker.remaining_ticks(player, "test.perk", 150), 0);
        assert!(tracker.try_activate(player, "test.perk", 150, 50));
    }

    #[test]
    fn cooldowns_are_per_player_and_per_perk() {
        let tracker = CooldownTracker::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert!(tracker.try_activate(a, "test.one", 0, 100));
        assert!(tracker.try_activate(b, "test.one", 0, 100));
        assert!(tracker.try_activate(a, "test.two", 0, 100));
        assert!(!tracker.try_activate(a, "test.one", 50, 100));
    }

    #[test]
    fn clear_player_drops_only_that_player() {
        let tracker = CooldownTracker::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        tracker.try_activate(a, "test.perk", 0, 100);
        tracker.try_activate(b, "test.perk", 0, 100);
        tracker.clear_player(a);
        assert_eq!(tracker.remaining_ticks(a, "test.perk", 0), 0);
        assert_eq!(tracker.remaining_ticks(b, "test.perk", 0), 100);
    }

    #[test]
    fn sweep_expired_removes_only_finished_cooldowns() {
        let tracker = CooldownTracker::new();
        let player = Uuid::new_v4();
        tracker.try_activate(player, "test.short", 0, 10);
        tracker.try_activate(player, "test.long", 0, 1000);
        tracker.sweep_expired(50);
        assert_eq!(tracker.remaining_ticks(player, "test.short", 50), 0);
        assert_eq!(tracker.remaining_ticks(player, "test.long", 50), 950);
    }
}
