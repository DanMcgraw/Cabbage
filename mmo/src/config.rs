use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::audit::AuditConfig;
use super::enterprise::config::EnterpriseConfig;
use super::frontier::config::FrontierConfig;
use super::ore_reveal::config::OreRevealConfig;
use super::skills::SkillId;
use super::warfare::config::WarfareConfig;

fn default_true() -> bool {
    true
}

/// Current schema version of `MmoConfig`. Older files are upgraded in place
/// on load (missing sections gain safe defaults) and saved back. Version 2
/// merges the retired skill-pair entries of the six-skill consolidation;
/// that merge happens deterministically while the `skills` map is
/// deserialized (see [`migrate_skill_configs`]).
pub const CURRENT_CONFIG_VERSION: u32 = 2;

/// Top-level Cabbage plugin configuration, now stored as RON.
///
/// Core owns this file (`config.ron`) and only ever writes the two core
/// switches. The `mmo` section exists only so legacy unified files still
/// parse; the MMO module's canonical home is `mmo.ron`, `mmo.rewards.ron`,
/// and `mmo.ores.ron`, so a missing section deserializes to `None`
/// and `None` is never written back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginConfig {
    #[serde(default)]
    pub metrics_log: bool,
    #[serde(default = "default_true")]
    pub mob_ai: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmo: Option<MmoConfig>,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            metrics_log: false,
            mob_ai: true,
            mmo: None,
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
            mmo: None,
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
    /// Levelling curve for each skill. Retired pair entries from pre-v2 files
    /// (e.g. `Agriculture`/`Herbalism`) are merged deterministically into
    /// their canonical destination during deserialization; unknown skill
    /// names (for example the retired `Combat`) are skipped with a warning
    /// instead of failing to load.
    #[serde(deserialize_with = "deserialize_skill_configs")]
    pub skills: HashMap<SkillId, SkillConfig>,
    /// Send a chat message when a player levels up.
    pub message_on_level_up: bool,
    /// How often (in server ticks) to flush cached progress to the database.
    pub save_interval_ticks: u32,
    /// Placed-feature registry names that should not generate.
    #[serde(default = "default_disabled_world_features")]
    pub disabled_world_features: Vec<String>,
    /// Rules for revealing ore veins after natural stone is mined. Still
    /// parsed from legacy unified files, but never serialized into
    /// `mmo.ron`; the canonical home is `mmo.ores.ron`.
    #[serde(default, skip_serializing)]
    pub ore_reveal: OreRevealConfig,
    /// Schema version for one-time migration of reward values from SQLite.
    #[serde(default)]
    pub reward_config_version: u32,
    /// Static XP rewards, kept in RON so all balance settings reload
    /// together. Still parsed from legacy unified files, but never
    /// serialized into `mmo.ron`; the canonical home is `mmo.rewards.ron`.
    #[serde(default, skip_serializing)]
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
    /// Enterprise branch skill and perk configuration.
    #[serde(default)]
    pub enterprise: EnterpriseConfig,
    /// Audit logging for progression-sensitive actions.
    #[serde(default)]
    pub audit: AuditConfig,
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
        self.enterprise = self.enterprise.sanitized();
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
/// Canonical v2 keys map straight to their `SkillId`. Retired pair keys from
/// pre-v2 files stay *distinct* here so the migration pass can merge them
/// deterministically — deserializing them as aliases of the destination key
/// would let hash-map iteration order pick a winner. Unknown skill names
/// (such as the retired `Combat` in pre-three-branch config files)
/// deserialize as `Unknown` and are skipped with a warning instead of
/// failing the entire config load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum SkillConfigKey {
    Cultivation,
    Woodcutting,
    Mining,
    Excavation,
    Fishing,
    AnimalHandling,
    Blades,
    Axes,
    Archery,
    Athletics,
    Defense,
    Sorcery,
    Smithing,
    Maintenance,
    Alchemy,
    Enchanting,
    Tinkering,
    Commerce,
    // Retired v1 keys, kept distinct for the deterministic migration pass.
    Agriculture,
    Herbalism,
    Husbandry,
    Taming,
    Unarmed,
    Acrobatics,
    Repair,
    Salvage,
    Trading,
    Charisma,
    #[serde(other)]
    Unknown,
}

impl SkillConfigKey {
    /// The canonical skill for a current config key, or `None` for retired
    /// and unknown keys.
    fn canonical_skill(self) -> Option<SkillId> {
        match self {
            SkillConfigKey::Cultivation => Some(SkillId::Cultivation),
            SkillConfigKey::Woodcutting => Some(SkillId::Woodcutting),
            SkillConfigKey::Mining => Some(SkillId::Mining),
            SkillConfigKey::Excavation => Some(SkillId::Excavation),
            SkillConfigKey::Fishing => Some(SkillId::Fishing),
            SkillConfigKey::AnimalHandling => Some(SkillId::AnimalHandling),
            SkillConfigKey::Blades => Some(SkillId::Blades),
            SkillConfigKey::Axes => Some(SkillId::Axes),
            SkillConfigKey::Archery => Some(SkillId::Archery),
            SkillConfigKey::Athletics => Some(SkillId::Athletics),
            SkillConfigKey::Defense => Some(SkillId::Defense),
            SkillConfigKey::Sorcery => Some(SkillId::Sorcery),
            SkillConfigKey::Smithing => Some(SkillId::Smithing),
            SkillConfigKey::Maintenance => Some(SkillId::Maintenance),
            SkillConfigKey::Alchemy => Some(SkillId::Alchemy),
            SkillConfigKey::Enchanting => Some(SkillId::Enchanting),
            SkillConfigKey::Tinkering => Some(SkillId::Tinkering),
            SkillConfigKey::Commerce => Some(SkillId::Commerce),
            _ => None,
        }
    }
}

/// Retired config keys merged by config schema v2, as
/// `(anchor, secondary, destination)`. When the destination has no explicit
/// entry, the anchor supplies the curve and `enabled` is the logical OR of
/// both retired entries.
const RETIRED_CONFIG_PAIRS: [(SkillConfigKey, SkillConfigKey, SkillId); 5] = [
    (
        SkillConfigKey::Agriculture,
        SkillConfigKey::Herbalism,
        SkillId::Cultivation,
    ),
    (
        SkillConfigKey::Husbandry,
        SkillConfigKey::Taming,
        SkillId::AnimalHandling,
    ),
    (
        SkillConfigKey::Unarmed,
        SkillConfigKey::Acrobatics,
        SkillId::Athletics,
    ),
    (
        SkillConfigKey::Repair,
        SkillConfigKey::Salvage,
        SkillId::Maintenance,
    ),
    (
        SkillConfigKey::Trading,
        SkillConfigKey::Charisma,
        SkillId::Commerce,
    ),
];

/// Deterministically merge retired pair entries into their canonical
/// destinations (config schema v2).
///
/// Rules, in order: an explicit canonical entry always wins; otherwise the
/// pair's anchor supplies `max_level`/`base_xp`/`xp_multiplier` (the
/// secondary's curve is used only when the anchor is absent); `enabled` is
/// the logical OR of the two retired entries so a partly enabled pair stays
/// usable. When both retired entries exist and their curves disagree, the
/// anchor still wins and the conflict is logged — never resolved by map
/// iteration order. Destinations with no old or new entry are left absent
/// and gain defaults during the version-bump save in `MmoState`.
fn migrate_skill_configs(
    raw: HashMap<SkillConfigKey, SkillConfig>,
) -> HashMap<SkillId, SkillConfig> {
    let mut skills = HashMap::with_capacity(raw.len());
    let mut retired: HashMap<SkillConfigKey, SkillConfig> = HashMap::new();
    for (key, config) in raw {
        match key.canonical_skill() {
            Some(skill) => {
                skills.insert(skill, config);
            }
            None if key != SkillConfigKey::Unknown => {
                retired.insert(key, config);
            }
            None => {
                log::warn!("[Cabbage MMO] ignoring unknown skill entry in config.ron");
            }
        }
    }

    for (anchor, secondary, destination) in RETIRED_CONFIG_PAIRS {
        if skills.contains_key(&destination) {
            if retired.contains_key(&anchor) || retired.contains_key(&secondary) {
                log::info!(
                    "[Cabbage MMO] config migration: explicit {destination} entry wins over \
                     the retired {anchor:?}/{secondary:?} entries"
                );
            }
            continue;
        }
        let anchor_config = retired.get(&anchor);
        let secondary_config = retired.get(&secondary);
        if anchor_config.is_none() && secondary_config.is_none() {
            continue;
        }
        let enabled = anchor_config.is_some_and(|config| config.enabled)
            || secondary_config.is_some_and(|config| config.enabled);
        let curve_source = anchor_config
            .or(secondary_config)
            .cloned()
            .unwrap_or_default();
        if let (Some(anchor_config), Some(secondary_config)) = (anchor_config, secondary_config) {
            let anchor_curve = (
                anchor_config.max_level,
                anchor_config.base_xp,
                anchor_config.xp_multiplier,
            );
            let secondary_curve = (
                secondary_config.max_level,
                secondary_config.base_xp,
                secondary_config.xp_multiplier,
            );
            if anchor_curve != secondary_curve {
                log::info!(
                    "[Cabbage MMO] config migration: {anchor:?} and {secondary:?} curves \
                     disagree; using the {anchor:?} anchor curve for {destination}"
                );
            }
        }
        skills.insert(
            destination,
            SkillConfig {
                max_level: curve_source.max_level,
                base_xp: curve_source.base_xp,
                xp_multiplier: curve_source.xp_multiplier,
                enabled,
            },
        );
    }
    skills
}

fn deserialize_skill_configs<'de, D>(
    deserializer: D,
) -> Result<HashMap<SkillId, SkillConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = HashMap::<SkillConfigKey, SkillConfig>::deserialize(deserializer)?;
    Ok(migrate_skill_configs(raw))
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
        "ore_gold_extra".to_string(),
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
            enterprise: EnterpriseConfig::default(),
            audit: AuditConfig::default(),
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

    fn parse_skills(skills_ron: &str) -> HashMap<SkillId, SkillConfig> {
        let config: MmoConfig = ron::from_str(&format!(
            "(enabled:true,message_on_level_up:true,save_interval_ticks:6000,\
             skills:{{{skills_ron}}},disabled_world_features:[])"
        ))
        .unwrap();
        config.skills
    }

    #[test]
    fn config_v2_merges_equal_curve_pair_and_ors_enabled() {
        let pair = "Agriculture:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:false),\
                    Herbalism:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true)";
        let skills = parse_skills(pair);
        let cultivation = skills.get(&SkillId::Cultivation).unwrap();
        assert_eq!(cultivation.max_level, 99);
        assert_eq!(cultivation.base_xp, 60);
        assert_eq!(cultivation.xp_multiplier, 1.14);
        // A partly enabled pair stays usable: enabled is the logical OR.
        assert!(cultivation.enabled);
        assert_eq!(skills.len(), 1);

        let pair = "Agriculture:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:false),\
                    Herbalism:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:false)";
        let skills = parse_skills(pair);
        assert!(!skills.get(&SkillId::Cultivation).unwrap().enabled);
    }

    #[test]
    fn config_v2_conflicting_curves_resolve_to_the_anchor() {
        let skills = parse_skills(
            "Unarmed:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true),\
             Acrobatics:(max_level:50,base_xp:40,xp_multiplier:1.10,enabled:false)",
        );
        // The deterministic anchor (Unarmed) supplies the curve; the
        // secondary still contributes to the enabled OR.
        let athletics = skills.get(&SkillId::Athletics).unwrap();
        assert_eq!(athletics.max_level, 99);
        assert_eq!(athletics.base_xp, 60);
        assert_eq!(athletics.xp_multiplier, 1.14);
        assert!(athletics.enabled);
        assert_eq!(skills.len(), 1);
    }

    #[test]
    fn config_v2_secondary_only_pair_uses_the_secondary_curve() {
        let skills =
            parse_skills("Taming:(max_level:80,base_xp:70,xp_multiplier:1.2,enabled:false)");
        let animal_handling = skills.get(&SkillId::AnimalHandling).unwrap();
        assert_eq!(animal_handling.max_level, 80);
        assert_eq!(animal_handling.base_xp, 70);
        assert!(!animal_handling.enabled);
    }

    #[test]
    fn config_v2_explicit_canonical_entry_wins_over_the_retired_pair() {
        let skills = parse_skills(
            "Maintenance:(max_level:42,base_xp:70,xp_multiplier:1.2,enabled:false),\
             Repair:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true),\
             Salvage:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true)",
        );
        let maintenance = skills.get(&SkillId::Maintenance).unwrap();
        assert_eq!(maintenance.max_level, 42);
        assert_eq!(maintenance.base_xp, 70);
        assert!(!maintenance.enabled);
        assert_eq!(skills.len(), 1);
    }

    #[test]
    fn config_v2_serialization_contains_only_canonical_names() {
        let skills = parse_skills(
            "Agriculture:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Herbalism:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Husbandry:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Taming:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Unarmed:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Acrobatics:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Repair:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Salvage:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Trading:(max_level:99,base_xp:60,xp_multiplier:1.14),\
             Charisma:(max_level:99,base_xp:60,xp_multiplier:1.14)",
        );
        assert_eq!(skills.len(), 5);
        let mut config = MmoConfig::default();
        config.skills = skills;
        let serialized = ron::ser::to_string(&config).unwrap();
        // PascalCase retired keys must never be written back; the lowercase
        // nested activity config fields (agriculture, repair, ...) stay.
        for retired in [
            "Agriculture",
            "Herbalism",
            "Husbandry",
            "Taming",
            "Unarmed",
            "Acrobatics",
            "Repair",
            "Salvage",
            "Trading",
            "Charisma",
        ] {
            assert!(
                !serialized.contains(retired),
                "serialized config still contains {retired}"
            );
        }
        for skill in [
            "Cultivation",
            "AnimalHandling",
            "Athletics",
            "Maintenance",
            "Commerce",
        ] {
            assert!(serialized.contains(skill), "missing {skill}");
        }
    }

    #[test]
    fn combat_migration_target_routes_retired_names() {
        let config: MmoConfig = ron::from_str(
            "(enabled:true,skills:{},message_on_level_up:true,save_interval_ticks:6000,\
             combat_migration:(target:Some(Repair)))",
        )
        .unwrap();
        assert_eq!(config.combat_migration.target, Some(SkillId::Maintenance));

        let config: MmoConfig = ron::from_str(
            "(enabled:true,skills:{},message_on_level_up:true,save_interval_ticks:6000,\
             combat_migration:(target:Some(Charisma)))",
        )
        .unwrap();
        assert_eq!(config.combat_migration.target, Some(SkillId::Commerce));
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
