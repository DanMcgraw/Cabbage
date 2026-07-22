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

/// Frontier branch configuration (Cultivation, Woodcutting, Mining,
/// Excavation, Fishing, AnimalHandling). The `agriculture`/`herbalism` and
/// `husbandry`/`taming` sections keep separate activity knobs while each
/// pair feeds one shared skill track.
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
    #[serde(default)]
    pub herbalism: HerbalismConfig,
    #[serde(default)]
    pub excavation: ExcavationConfig,
    #[serde(default)]
    pub husbandry: HusbandryConfig,
    #[serde(default)]
    pub taming: TamingConfig,
}

impl Default for FrontierConfig {
    fn default() -> Self {
        Self {
            mining: MiningPerkConfig::default(),
            woodcutting: WoodcuttingConfig::default(),
            agriculture: AgricultureConfig::default(),
            fishing: FishingConfig::default(),
            herbalism: HerbalismConfig::default(),
            excavation: ExcavationConfig::default(),
            husbandry: HusbandryConfig::default(),
            taming: TamingConfig::default(),
        }
    }
}

/// Mining perk knobs. Base ore XP lives in `config::XpRewardsConfig::blocks`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiningPerkConfig {
    /// Chance to add one item from an ore's approved drop list (Prospector).
    #[serde(default = "default_true")]
    pub prospector_enabled: bool,
    /// Base proc chance at level 1.
    pub prospector_base_chance: f64,
    /// Additional proc chance per Mining level.
    pub prospector_chance_per_level: f64,
    /// Hard cap for the Prospector proc chance.
    pub prospector_max_chance: f64,
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

/// Herbalism configuration: plant/forage XP, quality yield, and
/// consumable-healing bonuses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HerbalismConfig {
    /// XP per broken plant, keyed by block name. Tracked plants are
    /// provenance-marked on placement so placed plants earn nothing.
    #[serde(default = "default_plant_xp")]
    pub plant_xp: HashMap<String, u64>,
    /// Chance for a plant break to yield one extra item (quality yield).
    pub quality_yield_chance: f64,
    /// XP per eaten plant-based consumable, keyed by item registry key.
    #[serde(default = "default_consumable_xp")]
    pub consumable_xp: HashMap<String, u64>,
    /// Extra health restored by configured consumables (2.0 = one heart).
    pub consumable_heal_bonus: f32,
}

impl Default for HerbalismConfig {
    fn default() -> Self {
        Self {
            plant_xp: default_plant_xp(),
            quality_yield_chance: 0.08,
            consumable_xp: default_consumable_xp(),
            consumable_heal_bonus: 1.0,
        }
    }
}

impl HerbalismConfig {
    /// Whether this block name is XP-eligible and provenance-tracked.
    pub fn is_tracked_plant(&self, block_name: &str) -> bool {
        self.plant_xp.contains_key(block_name)
    }
}

fn default_plant_xp() -> HashMap<String, u64> {
    [
        ("dandelion", 4),
        ("poppy", 4),
        ("blue_orchid", 5),
        ("allium", 5),
        ("azure_bluet", 5),
        ("red_tulip", 5),
        ("orange_tulip", 5),
        ("white_tulip", 5),
        ("pink_tulip", 5),
        ("oxeye_daisy", 5),
        ("cornflower", 5),
        ("lily_of_the_valley", 6),
        ("sunflower", 6),
        ("lilac", 6),
        ("rose_bush", 6),
        ("peony", 6),
        ("tall_grass", 2),
        ("large_fern", 3),
        ("fern", 2),
        ("brown_mushroom", 6),
        ("red_mushroom", 6),
        ("sugar_cane", 5),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

fn default_consumable_xp() -> HashMap<String, u64> {
    [
        ("apple", 4),
        ("sweet_berries", 4),
        ("glow_berries", 4),
        ("melon_slice", 3),
        ("carrot", 3),
        ("potato", 3),
        ("beetroot", 4),
        ("suspicious_stew", 12),
        ("golden_apple", 25),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

/// Bonus loot attached to a diggable block, archaeology-style.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExcavationLoot {
    /// Registry key of the bonus item.
    pub item: String,
    /// Drop chance per eligible block break.
    pub chance: f64,
}

/// Excavation configuration: diggable-block XP, archaeology-style loot, and
/// the bounded Earthmover perk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExcavationConfig {
    /// XP per broken diggable block, keyed by block name. Tracked blocks are
    /// provenance-marked on placement so placed blocks earn nothing.
    #[serde(default = "default_diggable_xp")]
    pub diggable_xp: HashMap<String, u64>,
    /// Bonus loot rolls per diggable block, keyed by block name.
    #[serde(default = "default_excavation_loot")]
    pub bonus_loot: HashMap<String, ExcavationLoot>,
    /// Sneak + break a diggable block to excavate connected blocks of the
    /// same type (Earthmover).
    #[serde(default = "default_true")]
    pub earthmover_enabled: bool,
    /// Maximum extra blocks Earthmover may break in one action.
    pub earthmover_max_blocks: u32,
}

impl Default for ExcavationConfig {
    fn default() -> Self {
        Self {
            diggable_xp: default_diggable_xp(),
            bonus_loot: default_excavation_loot(),
            earthmover_enabled: true,
            earthmover_max_blocks: 16,
        }
    }
}

impl ExcavationConfig {
    /// Whether this block name is XP-eligible and provenance-tracked.
    pub fn is_tracked_diggable(&self, block_name: &str) -> bool {
        self.diggable_xp.contains_key(block_name)
    }
}

fn default_diggable_xp() -> HashMap<String, u64> {
    [
        ("dirt", 4),
        ("grass_block", 4),
        ("coarse_dirt", 4),
        ("rooted_dirt", 4),
        ("podzol", 5),
        ("mycelium", 5),
        ("sand", 4),
        ("red_sand", 5),
        ("gravel", 6),
        ("clay", 8),
        ("mud", 5),
        ("soul_sand", 8),
        ("soul_soil", 8),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

fn default_excavation_loot() -> HashMap<String, ExcavationLoot> {
    [
        ("gravel", ("flint", 0.08)),
        ("dirt", ("wheat_seeds", 0.04)),
        ("grass_block", ("wheat_seeds", 0.04)),
        ("sand", ("dead_bush", 0.03)),
        ("clay", ("clay_ball", 0.06)),
        ("soul_soil", ("bone", 0.04)),
    ]
    .into_iter()
    .map(|(block, (item, chance))| {
        (
            block.to_string(),
            ExcavationLoot {
                item: item.to_string(),
                chance,
            },
        )
    })
    .collect()
}

/// Animal-product reward for right-clicking an animal with the right tool
/// (bucket on a cow, shears on a sheep, bowl on a mooshroom).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProductReward {
    /// Registry key of the required held item.
    pub held_item: String,
    /// XP for collecting the product.
    pub xp: u64,
}

/// Husbandry configuration: breeding XP, animal products, and newborn traits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HusbandryConfig {
    /// XP per bred animal, keyed by entity resource name.
    #[serde(default = "default_breed_xp")]
    pub breed_xp: HashMap<String, u64>,
    /// XP for animals without a configured value.
    pub default_breed_xp: u64,
    /// XP for collecting animal products, keyed by entity resource name.
    #[serde(default = "default_product_xp")]
    pub product_xp: HashMap<String, ProductReward>,
    /// Chance that a successfully spawned baby receives one Cabbage trait.
    #[serde(default = "default_trait_roll_chance")]
    pub trait_roll_chance: f64,
    /// Trait identifiers eligible for the newborn roll.
    #[serde(default = "default_husbandry_traits")]
    pub traits: Vec<String>,
}

impl Default for HusbandryConfig {
    fn default() -> Self {
        Self {
            breed_xp: default_breed_xp(),
            default_breed_xp: 15,
            product_xp: default_product_xp(),
            trait_roll_chance: default_trait_roll_chance(),
            traits: default_husbandry_traits(),
        }
    }
}

fn default_trait_roll_chance() -> f64 {
    0.15
}

fn default_husbandry_traits() -> Vec<String> {
    ["hardy", "swift", "fertile"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_breed_xp() -> HashMap<String, u64> {
    [
        ("cow", 20),
        ("pig", 20),
        ("sheep", 20),
        ("chicken", 15),
        ("horse", 40),
        ("donkey", 40),
        ("mule", 40),
        ("llama", 35),
        ("goat", 30),
        ("rabbit", 20),
        ("wolf", 35),
        ("cat", 35),
        ("bee", 25),
        ("mooshroom", 30),
        ("strider", 30),
        ("hoglin", 40),
        ("axolotl", 35),
        ("frog", 25),
        ("turtle", 30),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

fn default_product_xp() -> HashMap<String, ProductReward> {
    [
        ("cow", ("bucket", 8)),
        ("mooshroom", ("bowl", 10)),
        ("sheep", ("shears", 10)),
        ("goat", ("bucket", 8)),
    ]
    .into_iter()
    .map(|(entity, (item, xp))| {
        (
            entity.to_string(),
            ProductReward {
                held_item: item.to_string(),
                xp,
            },
        )
    })
    .collect()
}

/// Taming configuration: tame XP and owner-validated pet interactions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TamingConfig {
    /// XP per tamed animal, keyed by entity resource name.
    #[serde(default = "default_tame_xp")]
    pub tame_xp: HashMap<String, u64>,
    /// XP for tames without a configured value.
    pub default_tame_xp: u64,
    /// XP per owner-validated feeding of a tamed pet.
    pub bond_feed_xp: u64,
    /// Maximum bond level a pet can reach through feeding.
    pub bond_cap: u32,
    /// Item registry keys that count as pet food for bonding.
    #[serde(default = "default_bond_food_items")]
    pub bond_food_items: Vec<String>,
}

impl Default for TamingConfig {
    fn default() -> Self {
        Self {
            tame_xp: default_tame_xp(),
            default_tame_xp: 30,
            bond_feed_xp: 4,
            bond_cap: 100,
            bond_food_items: default_bond_food_items(),
        }
    }
}

fn default_bond_food_items() -> Vec<String> {
    [
        "bone",
        "beef",
        "chicken",
        "porkchop",
        "mutton",
        "rabbit",
        "rotten_flesh",
        "cod",
        "salmon",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_tame_xp() -> HashMap<String, u64> {
    [
        ("wolf", 50),
        ("cat", 50),
        ("parrot", 40),
        ("horse", 40),
        ("donkey", 40),
        ("mule", 40),
        ("llama", 40),
        ("trader_llama", 40),
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
        clamp_chance(&mut self.woodcutting.heartwood_chance);
        clamp_chance(&mut self.agriculture.harvest_bonus_chance);
        clamp_chance(&mut self.herbalism.quality_yield_chance);
        if !self.herbalism.consumable_heal_bonus.is_finite()
            || self.herbalism.consumable_heal_bonus < 0.0
        {
            self.herbalism.consumable_heal_bonus = 0.0;
        }
        for loot in self.excavation.bonus_loot.values_mut() {
            clamp_chance(&mut loot.chance);
        }
        clamp_chance(&mut self.husbandry.trait_roll_chance);
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

    #[test]
    fn old_prospector_xp_field_is_ignored() {
        let config: MiningPerkConfig = ron::from_str(
            "(prospector_enabled:true,prospector_base_chance:0.05,prospector_chance_per_level:0.002,prospector_max_chance:0.35,prospector_xp_multiplier:0.5,vein_miner_enabled:true,vein_miner_max_blocks:16)",
        )
        .unwrap();

        assert_eq!(config, MiningPerkConfig::default());
    }
}
