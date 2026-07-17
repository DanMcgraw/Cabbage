use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::frontier::config::FrontierConfig;
use super::ore_reveal::config::OreRevealConfig;
use super::skills::SkillId;
use super::warfare::config::WarfareConfig;

fn default_true() -> bool {
    true
}

/// Current schema version of `MmoConfig`. Older files are upgraded in place
/// on load (missing sections gain safe defaults) and saved back.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

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
    /// Whether this skill can earn XP and present progress.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            max_level: 100,
            base_xp: 50,
            xp_multiplier: 1.15,
            enabled: true,
        }
    }
}

/// Global progression bounds enforced by the central `award_xp` path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressionConfig {
    /// Largest XP amount any single award may grant. Larger requests are
    /// clamped to this value.
    pub max_xp_per_award: u64,
}

impl Default for ProgressionConfig {
    fn default() -> Self {
        Self {
            max_xp_per_award: 10_000,
        }
    }
}

/// Global perk bounds and kill switch.
///
/// Per-skill perk knobs live in branch-specific config sections; every perk
/// must stay within these global caps regardless of its own configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerkConfig {
    /// Master switch for every perk effect. XP flow is unaffected.
    pub enabled: bool,
    /// Maximum blocks one batch-break perk (Timber, Vein Miner, Earthmover)
    /// may break in a single action. Hard-capped at Pumpkin's 128-block limit.
    pub batch_break_max_blocks: u32,
    /// Cooldown between batch-break activations, in server ticks.
    pub batch_break_cooldown_ticks: u32,
    /// Upper bound for any perk damage multiplier.
    pub max_damage_multiplier: f64,
    /// Upper bound for any perk proc chance (0.0 - 1.0).
    pub max_proc_chance: f64,
    /// Upper bound for perk area effects, in blocks of radius.
    pub max_effect_area_radius: u32,
}

impl PerkConfig {
    /// Pumpkin's hard limit for `Context::break_blocks`.
    pub const PUMPKIN_BATCH_BREAK_LIMIT: u32 = 128;

    /// Clamp every value into its valid range.
    pub fn sanitized(mut self) -> Self {
        self.batch_break_max_blocks = self
            .batch_break_max_blocks
            .clamp(1, Self::PUMPKIN_BATCH_BREAK_LIMIT);
        if !self.max_damage_multiplier.is_finite() || self.max_damage_multiplier < 1.0 {
            self.max_damage_multiplier = 1.0;
        }
        self.max_proc_chance = if self.max_proc_chance.is_finite() {
            self.max_proc_chance.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.max_effect_area_radius = self.max_effect_area_radius.min(16);
        self
    }
}

impl Default for PerkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            batch_break_max_blocks: 16,
            batch_break_cooldown_ticks: 100,
            max_damage_multiplier: 2.0,
            max_proc_chance: 0.35,
            max_effect_area_radius: 4,
        }
    }
}

/// Migration settings for retired legacy Combat XP.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CombatMigrationConfig {
    /// Destination skill for preserved legacy Combat XP. `None` (the default)
    /// keeps the XP in its legacy record until an administrator chooses a
    /// migration (via this setting or `/mmo migrate combat <skill>`).
    #[serde(default)]
    pub target: Option<SkillId>,
}

/// Configuration for the MMO levelling subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MmoConfig {
    /// Whether the MMO module is active.
    pub enabled: bool,
    /// Schema version of this config; upgraded automatically on load.
    #[serde(default)]
    pub config_version: u32,
    /// Levelling curve for each skill. Unknown skill names (for example the
    /// retired `Combat`) are skipped with a warning instead of failing to load.
    #[serde(deserialize_with = "deserialize_skill_configs")]
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
    /// Progression bounds applied by the central XP award path.
    #[serde(default)]
    pub progression: ProgressionConfig,
    /// Global perk bounds and kill switch.
    #[serde(default)]
    pub perks: PerkConfig,
    /// Frontier branch skill and perk configuration.
    #[serde(default)]
    pub frontier: FrontierConfig,
    /// Warfare branch skill and perk configuration.
    #[serde(default)]
    pub warfare: WarfareConfig,
    /// Legacy Combat XP migration settings.
    #[serde(default)]
    pub combat_migration: CombatMigrationConfig,
}

impl MmoConfig {
    /// Clamp out-of-range values into safe bounds. Called after every load.
    pub fn sanitized(mut self) -> Self {
        self.perks = self.perks.sanitized();
        self.frontier = self.frontier.sanitized();
        self.warfare = self.warfare.sanitized();
        for skill_config in self.skills.values_mut() {
            skill_config.base_xp = skill_config.base_xp.max(1);
            skill_config.max_level = skill_config.max_level.clamp(1, 1000);
            if !skill_config.xp_multiplier.is_finite() || skill_config.xp_multiplier < 1.0 {
                skill_config.xp_multiplier = 1.0;
            }
        }
        if self.progression.max_xp_per_award == 0 {
            self.progression.max_xp_per_award = 1;
        }
        self
    }
}

/// Lenient map key used only when deserializing `MmoConfig::skills`.
///
/// Unknown skill names (such as the retired `Combat` in pre-three-branch
/// config files) deserialize as `Unknown` and are skipped with a warning
/// instead of failing the entire config load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum SkillConfigKey {
    Agriculture,
    Herbalism,
    Woodcutting,
    Mining,
    Excavation,
    Fishing,
    Husbandry,
    Taming,
    Blades,
    Axes,
    Archery,
    Unarmed,
    Defense,
    Acrobatics,
    Sorcery,
    Smithing,
    Repair,
    Salvage,
    Alchemy,
    Enchanting,
    Tinkering,
    Trading,
    Charisma,
    #[serde(other)]
    Unknown,
}

impl SkillConfigKey {
    fn skill(self) -> Option<SkillId> {
        match self {
            SkillConfigKey::Agriculture => Some(SkillId::Agriculture),
            SkillConfigKey::Herbalism => Some(SkillId::Herbalism),
            SkillConfigKey::Woodcutting => Some(SkillId::Woodcutting),
            SkillConfigKey::Mining => Some(SkillId::Mining),
            SkillConfigKey::Excavation => Some(SkillId::Excavation),
            SkillConfigKey::Fishing => Some(SkillId::Fishing),
            SkillConfigKey::Husbandry => Some(SkillId::Husbandry),
            SkillConfigKey::Taming => Some(SkillId::Taming),
            SkillConfigKey::Blades => Some(SkillId::Blades),
            SkillConfigKey::Axes => Some(SkillId::Axes),
            SkillConfigKey::Archery => Some(SkillId::Archery),
            SkillConfigKey::Unarmed => Some(SkillId::Unarmed),
            SkillConfigKey::Defense => Some(SkillId::Defense),
            SkillConfigKey::Acrobatics => Some(SkillId::Acrobatics),
            SkillConfigKey::Sorcery => Some(SkillId::Sorcery),
            SkillConfigKey::Smithing => Some(SkillId::Smithing),
            SkillConfigKey::Repair => Some(SkillId::Repair),
            SkillConfigKey::Salvage => Some(SkillId::Salvage),
            SkillConfigKey::Alchemy => Some(SkillId::Alchemy),
            SkillConfigKey::Enchanting => Some(SkillId::Enchanting),
            SkillConfigKey::Tinkering => Some(SkillId::Tinkering),
            SkillConfigKey::Trading => Some(SkillId::Trading),
            SkillConfigKey::Charisma => Some(SkillId::Charisma),
            SkillConfigKey::Unknown => None,
        }
    }
}

fn deserialize_skill_configs<'de, D>(
    deserializer: D,
) -> Result<HashMap<SkillId, SkillConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = HashMap::<SkillConfigKey, SkillConfig>::deserialize(deserializer)?;
    let mut skills = HashMap::with_capacity(raw.len());
    for (key, config) in raw {
        match key.skill() {
            Some(skill) => {
                skills.insert(skill, config);
            }
            None => {
                log::warn!("[Cabbage MMO] ignoring unknown skill entry in config.ron");
            }
        }
    }
    Ok(skills)
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
        let skills = SkillId::ALL
            .iter()
            .map(|skill| (*skill, SkillConfig::default()))
            .collect();
        Self {
            enabled: true,
            config_version: CURRENT_CONFIG_VERSION,
            skills,
            message_on_level_up: true,
            save_interval_ticks: 6000,
            disabled_world_features: default_disabled_world_features(),
            ore_reveal: OreRevealConfig::default(),
            reward_config_version: 0,
            xp_rewards: XpRewardsConfig::default(),
            progression: ProgressionConfig::default(),
            perks: PerkConfig::default(),
            frontier: FrontierConfig::default(),
            warfare: WarfareConfig::default(),
            combat_migration: CombatMigrationConfig::default(),
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

    /// Highest level on this curve.
    #[allow(dead_code)] // consumed by perk unlock checks (Phases 1-4)
    pub fn max_level(&self) -> u32 {
        self.max_level
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
            enabled: true,
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
        assert_eq!(config.config_version, 0);
        assert_eq!(config.progression, ProgressionConfig::default());
        assert_eq!(config.perks, PerkConfig::default());
        assert_eq!(config.frontier, FrontierConfig::default());
        assert_eq!(config.combat_migration, CombatMigrationConfig::default());
    }

    #[test]
    fn legacy_two_skill_config_loads_and_drops_combat() {
        // Shape of config.ron written before the three-branch model: only
        // Mining and Combat curves, no perk/progression/migration sections.
        let config: MmoConfig = ron::from_str(
            "(enabled:true,message_on_level_up:true,save_interval_ticks:6000,\
             skills:{\
                 Mining:(max_level:99,base_xp:50,xp_multiplier:1.15),\
                 Combat:(max_level:99,base_xp:60,xp_multiplier:1.14)\
             },\
             disabled_world_features:[],\
             reward_config_version:1,\
             xp_rewards:(mobs:{\"zombie\":99},blocks:{\"coal_ore\":42}))",
        )
        .unwrap();

        let mining = config.skills.get(&SkillId::Mining).unwrap();
        assert_eq!(mining.max_level, 99);
        assert!(mining.enabled);
        assert!(!config.skills.contains_key(&SkillId::Blades));
        assert_eq!(config.skills.len(), 1);
        assert_eq!(config.xp_rewards.mobs.get("zombie"), Some(&99));
        assert_eq!(config.xp_rewards.blocks.get("coal_ore"), Some(&42));
    }

    #[test]
    fn default_config_covers_every_skill() {
        let config = MmoConfig::default();
        for skill in SkillId::ALL {
            assert!(config.skills.contains_key(skill));
        }
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn sanitized_clamps_out_of_range_values() {
        let mut config = MmoConfig::default();
        config.perks.batch_break_max_blocks = 9999;
        config.perks.max_proc_chance = 4.0;
        config.perks.max_damage_multiplier = f64::NAN;
        config.progression.max_xp_per_award = 0;
        config
            .skills
            .get_mut(&SkillId::Mining)
            .unwrap()
            .xp_multiplier = 0.5;

        let config = config.sanitized();
        assert_eq!(config.perks.batch_break_max_blocks, 128);
        assert_eq!(config.perks.max_proc_chance, 1.0);
        assert_eq!(config.perks.max_damage_multiplier, 1.0);
        assert_eq!(config.progression.max_xp_per_award, 1);
        assert_eq!(config.skills[&SkillId::Mining].xp_multiplier, 1.0);
    }

    #[test]
    fn combat_migration_target_round_trips() {
        let config: MmoConfig = ron::from_str(
            "(enabled:true,skills:{},message_on_level_up:true,save_interval_ticks:6000,\
             combat_migration:(target:Some(Blades)))",
        )
        .unwrap();
        assert_eq!(config.combat_migration.target, Some(SkillId::Blades));
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
