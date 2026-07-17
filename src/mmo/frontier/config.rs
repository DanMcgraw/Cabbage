//! Frontier branch configuration: per-skill XP tables and bounded perk knobs.
//!
//! Every chance here is additionally clamped by the global
//! `config::PerkConfig` caps at the point of use; every batch size is clamped
//! by the global batch cap and Pumpkin's 128-block hard limit.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Frontier branch configuration (Mining, Woodcutting, Agriculture, Fishing
/// and, as they land, Herbalism, Excavation, Husbandry, Taming).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrontierConfig {
    #[serde(default)]
    pub mining: MiningPerkConfig,
    #[serde(default)]
    pub woodcutting: WoodcuttingConfig,
    #[serde(default)]
    pub agriculture: AgricultureConfig,
    #[serde(default)]
    pub fishing: FishingConfig,
}

impl Default for FrontierConfig {
    fn default() -> Self {
        Self {
            mining: MiningPerkConfig::default(),
            woodcutting: WoodcuttingConfig::default(),
            agriculture: AgricultureConfig::default(),
            fishing: FishingConfig::default(),
        }
    }
}

/// Mining perk knobs. Base ore XP lives in `config::XpRewardsConfig::blocks`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiningPerkConfig {
    /// Chance-based bonus XP on ore breaks (Prospector).
    #[serde(default = "default_true")]
    pub prospector_enabled: bool,
    /// Base proc chance at level 1.
    pub prospector_base_chance: f64,
    /// Additional proc chance per Mining level.
    pub prospector_chance_per_level: f64,
    /// Hard cap for the Prospector proc chance.
    pub prospector_max_chance: f64,
    /// Bonus XP = ore reward × this multiplier when Prospector procs.
    pub prospector_xp_multiplier: f64,
    /// Sneak + break an ore to break its connected vein (Vein Miner).
    #[serde(default = "default_true")]
    pub vein_miner_enabled: bool,
    /// Maximum extra blocks Vein Miner may break in one action.
    pub vein_miner_max_blocks: u32,
}

impl Default for MiningPerkConfig {
    fn default() -> Self {
        Self {
            prospector_enabled: true,
            prospector_base_chance: 0.05,
            prospector_chance_per_level: 0.002,
            prospector_max_chance: 0.35,
            prospector_xp_multiplier: 0.5,
            vein_miner_enabled: true,
            vein_miner_max_blocks: 16,
        }
    }
}

/// Woodcutting configuration: natural-log XP, Heartwood roll, Timber.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WoodcuttingConfig {
    /// XP per natural log break, keyed by block name.
    #[serde(default = "default_log_xp")]
    pub log_xp: HashMap<String, u64>,
    /// Chance for a natural log break to drop one extra log (Heartwood roll).
    pub heartwood_chance: f64,
    /// Bonus XP awarded alongside a successful Heartwood roll.
    pub heartwood_xp_bonus: u64,
    /// Sneak + break a natural log to fell connected logs (Timber).
    #[serde(default = "default_true")]
    pub timber_enabled: bool,
    /// Maximum extra blocks Timber may break in one action.
    pub timber_max_blocks: u32,
}

impl Default for WoodcuttingConfig {
    fn default() -> Self {
        Self {
            log_xp: default_log_xp(),
            heartwood_chance: 0.02,
            heartwood_xp_bonus: 25,
            timber_enabled: true,
            timber_max_blocks: 32,
        }
    }
}

impl WoodcuttingConfig {
    /// Whether this block name is XP-eligible and provenance-tracked.
    pub fn is_tracked_log(&self, block_name: &str) -> bool {
        self.log_xp.contains_key(block_name)
    }
}

fn default_log_xp() -> HashMap<String, u64> {
    [
        ("oak_log", 6),
        ("spruce_log", 6),
        ("birch_log", 6),
        ("jungle_log", 7),
        ("acacia_log", 6),
        ("dark_oak_log", 7),
        ("mangrove_log", 7),
        ("cherry_log", 6),
        ("pale_oak_log", 7),
        ("crimson_stem", 8),
        ("warped_stem", 8),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

/// Per-crop harvest reward.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CropReward {
    /// XP for harvesting this crop at full maturity.
    pub xp: u64,
    /// Value of the crop's `age` block property at full maturity.
    pub max_age: u32,
    /// Registry key of the item granted by the harvest bonus roll.
    pub bonus_item: String,
}

/// Agriculture configuration: mature-crop harvest XP, fertilizer provenance,
/// and a conservative harvest bonus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgricultureConfig {
    /// Harvest rewards keyed by block name.
    #[serde(default = "default_crops")]
    pub crops: HashMap<String, CropReward>,
    /// Chance for a mature harvest to add one bonus crop item.
    pub harvest_bonus_chance: f64,
    /// Bonus XP for harvesting a fertilized crop.
    pub fertilizer_bonus_xp: u64,
    /// Whether a fertilized crop always yields the harvest bonus item.
    #[serde(default = "default_true")]
    pub fertilizer_guarantees_bonus: bool,
}

impl Default for AgricultureConfig {
    fn default() -> Self {
        Self {
            crops: default_crops(),
            harvest_bonus_chance: 0.10,
            fertilizer_bonus_xp: 10,
            fertilizer_guarantees_bonus: true,
        }
    }
}

impl AgricultureConfig {
    /// Whether this block name is a configured crop.
    pub fn is_crop(&self, block_name: &str) -> bool {
        self.crops.contains_key(block_name)
    }
}

fn default_crops() -> HashMap<String, CropReward> {
    [
        ("wheat", 10, 7, "wheat"),
        ("carrots", 10, 7, "carrot"),
        ("potatoes", 10, 7, "potato"),
        ("beetroots", 12, 3, "beetroot"),
        ("nether_wart", 14, 3, "nether_wart"),
        ("cocoa", 12, 2, "cocoa_beans"),
    ]
    .into_iter()
    .map(|(name, xp, max_age, item)| {
        (
            name.to_string(),
            CropReward {
                xp,
                max_age,
                bonus_item: item.to_string(),
            },
        )
    })
    .collect()
}

/// Fishing configuration: catch XP, reel bonus, treasure replacement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FishingConfig {
    /// XP per caught item, keyed by item registry key.
    #[serde(default = "default_catch_xp")]
    pub catch_xp: HashMap<String, u64>,
    /// XP for catches without a configured value.
    pub default_catch_xp: u64,
    /// Extra vanilla experience dropped on a successful catch (reel perk).
    pub reel_exp_bonus: i32,
    /// Replace caught treasure items: caught registry key → replacement key.
    /// Empty by default (feature off until balanced).
    #[serde(default)]
    pub treasure_replacements: HashMap<String, String>,
}

impl Default for FishingConfig {
    fn default() -> Self {
        Self {
            catch_xp: default_catch_xp(),
            default_catch_xp: 10,
            reel_exp_bonus: 2,
            treasure_replacements: HashMap::new(),
        }
    }
}

fn default_catch_xp() -> HashMap<String, u64> {
    [
        ("cod", 20),
        ("salmon", 25),
        ("pufferfish", 35),
        ("tropical_fish", 40),
        ("bow", 50),
        ("enchanted_book", 60),
        ("fishing_rod", 40),
        ("name_tag", 60),
        ("saddle", 60),
        ("nautilus_shell", 55),
        ("leather", 5),
        ("stick", 5),
        ("string", 5),
        ("bowl", 5),
        ("bone", 5),
        ("ink_sac", 8),
        ("lily_pad", 8),
        ("tripwire_hook", 12),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

impl FrontierConfig {
    /// Clamp out-of-range values into safe bounds.
    pub fn sanitized(mut self) -> Self {
        let clamp_chance = |value: &mut f64| {
            *value = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            };
        };
        clamp_chance(&mut self.mining.prospector_base_chance);
        clamp_chance(&mut self.mining.prospector_chance_per_level);
        clamp_chance(&mut self.mining.prospector_max_chance);
        if !self.mining.prospector_xp_multiplier.is_finite()
            || self.mining.prospector_xp_multiplier < 0.0
        {
            self.mining.prospector_xp_multiplier = 0.0;
        }
        clamp_chance(&mut self.woodcutting.heartwood_chance);
        clamp_chance(&mut self.agriculture.harvest_bonus_chance);
        self.fishing.reel_exp_bonus = self.fishing.reel_exp_bonus.clamp(0, 100);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_vanilla_logs_and_crops() {
        let woodcutting = WoodcuttingConfig::default();
        assert!(woodcutting.is_tracked_log("oak_log"));
        assert!(woodcutting.is_tracked_log("warped_stem"));
        assert!(!woodcutting.is_tracked_log("oak_planks"));

        let agriculture = AgricultureConfig::default();
        assert!(agriculture.is_crop("wheat"));
        assert_eq!(agriculture.crops["wheat"].max_age, 7);
        assert_eq!(agriculture.crops["beetroots"].max_age, 3);
        assert_eq!(agriculture.crops["cocoa"].bonus_item, "cocoa_beans");
    }

    #[test]
    fn sanitized_clamps_chances() {
        let mut config = FrontierConfig::default();
        config.mining.prospector_max_chance = 7.0;
        config.woodcutting.heartwood_chance = f64::NAN;
        config.fishing.reel_exp_bonus = 10_000;
        let config = config.sanitized();
        assert_eq!(config.mining.prospector_max_chance, 1.0);
        assert_eq!(config.woodcutting.heartwood_chance, 0.0);
        assert_eq!(config.fishing.reel_exp_bonus, 100);
    }
}
