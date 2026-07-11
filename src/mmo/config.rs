use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ore_reveal::config::OreRevealConfig;
use super::skills::SkillId;

fn default_true() -> bool {
    true
}

/// Top-level Cabbage plugin configuration, now stored as RON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginConfig {
    pub metrics_log: bool,
    #[serde(default = "default_true")]
    pub mob_ai: bool,
    pub mmo: Option<MmoConfig>,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            metrics_log: false,
            mob_ai: true,
            mmo: Some(MmoConfig::default()),
        }
    }
}

/// Legacy JSON config used for one-time migration to RON.
#[derive(Deserialize)]
pub struct LegacyPluginConfig {
    pub metrics_log: bool,
    #[serde(default = "default_true")]
    pub mob_ai: bool,
}

impl From<LegacyPluginConfig> for PluginConfig {
    fn from(legacy: LegacyPluginConfig) -> Self {
        Self {
            metrics_log: legacy.metrics_log,
            mob_ai: legacy.mob_ai,
            mmo: Some(MmoConfig::default()),
        }
    }
}

/// Per-skill levelling parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillConfig {
    /// Highest achievable level for this skill.
    pub max_level: u32,
    /// Base XP required to reach level 2.
    pub base_xp: u64,
    /// Multiplier applied to the XP requirement for each subsequent level.
    pub xp_multiplier: f64,
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            max_level: 99,
            base_xp: 50,
            xp_multiplier: 1.15,
        }
    }
}

/// Configuration for the MMO levelling subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MmoConfig {
    /// Whether the MMO module is active.
    pub enabled: bool,
    /// Levelling curve for each skill.
    pub skills: HashMap<SkillId, SkillConfig>,
    /// Send a chat message when a player levels up.
    pub message_on_level_up: bool,
    /// How often (in server ticks) to flush cached progress to the database.
    pub save_interval_ticks: u32,
    /// Placed-feature registry names that should not generate.
    #[serde(default = "default_disabled_world_features")]
    pub disabled_world_features: Vec<String>,
    /// Rules for revealing ore veins after natural stone is mined.
    #[serde(default)]
    pub ore_reveal: OreRevealConfig,
    /// Schema version for one-time migration of reward values from SQLite.
    #[serde(default)]
    pub reward_config_version: u32,
    /// Static XP rewards, kept in RON so all balance settings reload together.
    #[serde(default)]
    pub xp_rewards: XpRewardsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct XpRewardsConfig {
    #[serde(default = "default_mob_xp_rewards")]
    pub mobs: HashMap<String, u64>,
    #[serde(default = "default_block_xp_rewards")]
    pub blocks: HashMap<String, u64>,
}

impl Default for XpRewardsConfig {
    fn default() -> Self {
        Self {
            mobs: default_mob_xp_rewards(),
            blocks: default_block_xp_rewards(),
        }
    }
}

fn default_mob_xp_rewards() -> HashMap<String, u64> {
    [
        ("zombie", 12),
        ("skeleton", 14),
        ("creeper", 18),
        ("spider", 12),
        ("enderman", 28),
        ("witch", 24),
        ("drowned", 14),
        ("husk", 14),
        ("stray", 14),
        ("phantom", 20),
        ("slime", 8),
        ("cave_spider", 14),
        ("piglin", 16),
        ("piglin_brute", 32),
        ("zombified_piglin", 16),
        ("blaze", 22),
        ("ghast", 28),
        ("wither_skeleton", 30),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

fn default_block_xp_rewards() -> HashMap<String, u64> {
    [
        ("coal_ore", 8),
        ("deepslate_coal_ore", 10),
        ("iron_ore", 15),
        ("deepslate_iron_ore", 18),
        ("copper_ore", 12),
        ("deepslate_copper_ore", 14),
        ("gold_ore", 25),
        ("deepslate_gold_ore", 28),
        ("redstone_ore", 12),
        ("deepslate_redstone_ore", 14),
        ("lapis_ore", 20),
        ("deepslate_lapis_ore", 22),
        ("diamond_ore", 60),
        ("deepslate_diamond_ore", 70),
        ("emerald_ore", 50),
        ("deepslate_emerald_ore", 55),
        ("nether_quartz_ore", 16),
        ("nether_gold_ore", 22),
        ("ancient_debris", 150),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

fn default_disabled_world_features() -> Vec<String> {
    vec![
        "ore_coal_upper".to_string(),
        "ore_coal_lower".to_string(),
        "ore_iron_upper".to_string(),
        "ore_iron_middle".to_string(),
        "ore_iron_small".to_string(),
        "ore_gold".to_string(),
        "ore_gold_lower".to_string(),
        "ore_redstone".to_string(),
        "ore_redstone_lower".to_string(),
        "ore_diamond".to_string(),
        "ore_diamond_large".to_string(),
        "ore_diamond_buried".to_string(),
        "ore_diamond_medium".to_string(),
        "ore_lapis".to_string(),
        "ore_lapis_buried".to_string(),
        "ore_copper".to_string(),
        "ore_copper_large".to_string(),
        "ore_emerald".to_string(),
    ]
}

impl Default for MmoConfig {
    fn default() -> Self {
        let mut skills = HashMap::new();
        skills.insert(
            SkillId::Mining,
            SkillConfig {
                max_level: 99,
                base_xp: 50,
                xp_multiplier: 1.15,
            },
        );
        skills.insert(
            SkillId::Combat,
            SkillConfig {
                max_level: 99,
                base_xp: 60,
                xp_multiplier: 1.14,
            },
        );
        Self {
            enabled: true,
            skills,
            message_on_level_up: true,
            save_interval_ticks: 6000,
            disabled_world_features: default_disabled_world_features(),
            ore_reveal: OreRevealConfig::default(),
            reward_config_version: 0,
            xp_rewards: XpRewardsConfig::default(),
        }
    }
}

/// Computes XP requirements and level from cumulative XP.
#[derive(Debug, Clone)]
pub struct LevelCurve {
    /// XP threshold for each level. Index 0 is level 1 (always 0 XP).
    thresholds: Vec<u64>,
    max_level: u32,
}

impl LevelCurve {
    pub fn new(config: &SkillConfig) -> Self {
        let max_level = config.max_level.max(1);
        let mut thresholds = Vec::with_capacity(max_level as usize);
        thresholds.push(0);

        let mut total = 0u64;
        for level in 1..max_level {
            let requirement = (config.base_xp as f64 * config.xp_multiplier.powi(level as i32 - 1))
                .floor()
                .max(1.0) as u64;
            total = total.saturating_add(requirement);
            thresholds.push(total);
        }

        Self {
            thresholds,
            max_level,
        }
    }

    /// Total cumulative XP required to reach `level`.
    ///
    /// Level 1 always requires 0 XP. Levels above `max_level` clamp to the max threshold.
    #[allow(dead_code)]
    pub fn xp_for_level(&self, level: u32) -> u64 {
        if level <= 1 {
            0
        } else {
            let index = ((level - 1).min(self.max_level) as usize).min(self.thresholds.len() - 1);
            self.thresholds[index]
        }
    }

    /// Derive the current level, XP into the level, and XP needed for the next level.
    pub fn level_for_xp(&self, xp: u64) -> (u32, u64, u64) {
        let mut level = 1u32;
        while level < self.max_level && xp >= self.thresholds[level as usize] {
            level += 1;
        }

        let current_threshold = self.thresholds[(level - 1) as usize];
        let next_threshold = if level >= self.max_level {
            current_threshold
        } else {
            self.thresholds[level as usize]
        };

        let into_level = xp.saturating_sub(current_threshold);
        let needed = next_threshold.saturating_sub(current_threshold);
        (level, into_level, needed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_curve() -> LevelCurve {
        LevelCurve::new(&SkillConfig {
            max_level: 5,
            base_xp: 100,
            xp_multiplier: 2.0,
        })
    }

    #[test]
    fn level_one_is_free() {
        let curve = test_curve();
        assert_eq!(curve.xp_for_level(1), 0);
    }

    #[test]
    fn thresholds_follow_multiplier() {
        let curve = test_curve();
        assert_eq!(curve.xp_for_level(2), 100);
        assert_eq!(curve.xp_for_level(3), 300);
        assert_eq!(curve.xp_for_level(4), 700);
        assert_eq!(curve.xp_for_level(5), 1500);
    }

    #[test]
    fn level_for_xp_round_trips() {
        let curve = test_curve();
        for level in 1..=5 {
            let xp = curve.xp_for_level(level);
            let (computed, _, _) = curve.level_for_xp(xp);
            assert_eq!(computed, level);
        }
    }

    #[test]
    fn mid_level_progress() {
        let curve = test_curve();
        let (level, into, needed) = curve.level_for_xp(350);
        assert_eq!(level, 3);
        assert_eq!(into, 50);
        assert_eq!(needed, 400);
    }

    #[test]
    fn older_mmo_config_gets_default_ore_reveal_rules() {
        let config: MmoConfig = ron::from_str(
            "(enabled:true,skills:{},message_on_level_up:true,save_interval_ticks:6000,disabled_world_features:[])",
        )
        .unwrap();
        assert_eq!(config.ore_reveal, OreRevealConfig::default());
        assert_eq!(config.reward_config_version, 0);
        assert_eq!(config.xp_rewards, XpRewardsConfig::default());
    }

    #[test]
    fn default_worldgen_blacklist_includes_emerald() {
        assert!(default_disabled_world_features().contains(&"ore_emerald".to_string()));
    }

    #[test]
    fn default_xp_rewards_match_expected_values() {
        let rewards = XpRewardsConfig::default();
        assert_eq!(rewards.mobs.get("zombie"), Some(&12));
        assert_eq!(rewards.blocks.get("diamond_ore"), Some(&60));
        assert_eq!(rewards.blocks.get("deepslate_emerald_ore"), Some(&55));
    }
}
