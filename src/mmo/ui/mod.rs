//! Player-facing presentation: bossbars, the protected native skill menu
//! (`/mmo menu`), and the default chat summary grid (`/mmo`).

pub(crate) mod bossbar;
pub(crate) mod chat;
pub(crate) mod menu;

pub(crate) use bossbar::BossbarState;

use super::{config::MmoConfig, skills::SkillId};

/// Whole percent of the current level completed. Clamped to 100 and safe
/// for very large configured XP values.
pub(crate) fn progress_percent(into: u64, needed: u64) -> u64 {
    if needed == 0 {
        return 100;
    }
    ((u128::from(into) * 100) / u128::from(needed)).min(100) as u64
}

/// Whether a skill is enabled in this config snapshot. Skills missing from
/// the config map default to enabled, matching `SkillConfig::default`.
pub(crate) fn skill_enabled(config: &MmoConfig, skill: SkillId) -> bool {
    config
        .skills
        .get(&skill)
        .is_none_or(|config| config.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_percent_is_exact_for_small_values() {
        assert_eq!(progress_percent(0, 200), 0);
        assert_eq!(progress_percent(50, 200), 25);
        assert_eq!(progress_percent(200, 200), 100);
    }

    #[test]
    fn progress_percent_clamps_and_needs_zero_is_full() {
        assert_eq!(progress_percent(250, 200), 100);
        assert_eq!(progress_percent(0, 0), 100);
    }

    #[test]
    fn progress_percent_survives_huge_xp_values() {
        let percent = progress_percent(u64::MAX - 1, u64::MAX);
        assert!(percent <= 100);
        assert_eq!(progress_percent(u64::MAX, u64::MAX), 100);
    }
}
