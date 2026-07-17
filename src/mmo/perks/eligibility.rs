//! Perk unlock eligibility by skill level.
//!
//! Major perks unlock at levels 25/50/75 and capstones at 100. No major
//! perks or capstones are enabled yet: per the plan they arrive one at a
//! time only after the owning skill's basic XP flow has been live-tested,
//! behind configuration kill switches. This module owns the level gates so
//! every future perk uses the same thresholds.

/// Levels at which major perks unlock.
#[allow(dead_code)] // consumed by the first major perks (Phase 4, after live tests)
pub(crate) const MAJOR_PERK_LEVELS: [u32; 3] = [25, 50, 75];

/// Level at which a skill's capstone unlocks.
pub(crate) const CAPSTONE_LEVEL: u32 = 100;

/// Whether a major perk of the given unlock level is available at `level`.
#[allow(dead_code)] // consumed by the first major perks (Phase 4, after live tests)
pub(crate) fn is_major_perk_unlocked(level: u32, perk_level: u32) -> bool {
    MAJOR_PERK_LEVELS.contains(&perk_level) && level >= perk_level
}

/// Whether the capstone is available at `level`.
#[allow(dead_code)] // consumed by the first capstone (Phase 4, after live tests)
pub(crate) fn is_capstone_unlocked(level: u32) -> bool {
    level >= CAPSTONE_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_perks_unlock_at_thresholds() {
        assert!(!is_major_perk_unlocked(24, 25));
        assert!(is_major_perk_unlocked(25, 25));
        assert!(is_major_perk_unlocked(80, 25));
        assert!(!is_major_perk_unlocked(49, 50));
        assert!(is_major_perk_unlocked(50, 50));
        assert!(!is_major_perk_unlocked(74, 75));
        assert!(is_major_perk_unlocked(75, 75));
    }

    #[test]
    fn non_threshold_levels_are_rejected() {
        assert!(!is_major_perk_unlocked(100, 30));
        assert!(!is_major_perk_unlocked(100, 100));
    }

    #[test]
    fn capstone_unlocks_at_100() {
        assert!(!is_capstone_unlocked(99));
        assert!(is_capstone_unlocked(100));
    }
}
