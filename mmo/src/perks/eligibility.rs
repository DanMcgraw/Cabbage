//! Perk tier eligibility by skill level.
//!
//! Every skill has four perk milestones — tiers — at levels 10/25/50/100.
//! Reaching a tier steps up the skill's existing perk effects (bigger batch
//! breaks, higher proc chances, raised caps) and, at the top tier, unlocks
//! capstone-style extras. This module owns the tier thresholds so every
//! per-skill perk formula and the level-up announcements use the same
//! levels.

/// Levels at which perk tiers unlock. `perk_tier` counts how many of these
/// a level has reached; the skill detail catalog builds its milestone rows
/// from this list.
pub(crate) const PERK_TIER_LEVELS: [u32; 4] = [10, 25, 50, 100];

/// Number of perk milestones reached at `level` (0..=4).
pub(crate) fn perk_tier(level: u32) -> u32 {
    PERK_TIER_LEVELS
        .iter()
        .filter(|&&tier| level >= tier)
        .count() as u32
}

/// Tier levels crossed going from `old` to `new` (for unlock announcements),
/// in ascending order. Handles multi-level jumps and yields each crossed
/// tier level once; empty when no tier is crossed.
pub(crate) fn tiers_crossed(old: u32, new: u32) -> impl Iterator<Item = u32> {
    PERK_TIER_LEVELS
        .into_iter()
        .filter(move |&tier| old < tier && tier <= new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perk_tier_counts_reached_milestones() {
        // Tier 0 for any level below 10 (9 is only an edge input; no tier 9).
        for level in [0, 1, 5, 9] {
            assert_eq!(perk_tier(level), 0, "level {level}");
        }
        // Tier 1 from 10 up to 24.
        for level in [10, 11, 24] {
            assert_eq!(perk_tier(level), 1, "level {level}");
        }
        // Tier 2 from 25 up to 49.
        for level in [25, 26, 49] {
            assert_eq!(perk_tier(level), 2, "level {level}");
        }
        // Tier 3 from 50 up to 99 (99 is only an edge input; no tier 99).
        for level in [50, 51, 99] {
            assert_eq!(perk_tier(level), 3, "level {level}");
        }
        // Tier 4 at 100 and beyond.
        for level in [100, 101, 1000] {
            assert_eq!(perk_tier(level), 4, "level {level}");
        }
    }

    #[test]
    fn tiers_crossed_is_empty_when_no_tier_is_crossed() {
        assert_eq!(tiers_crossed(1, 9).collect::<Vec<_>>(), Vec::<u32>::new());
        assert_eq!(tiers_crossed(10, 24).collect::<Vec<_>>(), Vec::<u32>::new());
        assert_eq!(tiers_crossed(50, 50).collect::<Vec<_>>(), Vec::<u32>::new());
        // Level loss never announces a tier.
        assert_eq!(tiers_crossed(60, 10).collect::<Vec<_>>(), Vec::<u32>::new());
    }

    #[test]
    fn tiers_crossed_yields_a_single_crossed_tier() {
        assert_eq!(tiers_crossed(9, 10).collect::<Vec<_>>(), vec![10]);
        assert_eq!(tiers_crossed(24, 25).collect::<Vec<_>>(), vec![25]);
        assert_eq!(tiers_crossed(99, 100).collect::<Vec<_>>(), vec![100]);
    }

    #[test]
    fn tiers_crossed_handles_multi_level_jumps() {
        assert_eq!(
            tiers_crossed(1, 100).collect::<Vec<_>>(),
            vec![10, 25, 50, 100]
        );
        assert_eq!(tiers_crossed(9, 26).collect::<Vec<_>>(), vec![10, 25]);
        // Each crossed tier level is yielded exactly once.
        assert_eq!(tiers_crossed(10, 50).collect::<Vec<_>>(), vec![25, 50]);
    }
}
