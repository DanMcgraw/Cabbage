//! Enterprise branch configuration: per-skill XP values and bounded perk
//! knobs.
//!
//! Preview modifiers (anvil cost discount, grindstone XP bonus, enchanting
//! offer discount) are applied in prepare events only while the skill's
//! cooldown is ready; the cooldown is charged and XP awarded only in the
//! matching take/commit event, per the plan's Enterprise rules.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Enterprise branch configuration (Smithing, Repair, Salvage, Alchemy,
/// Enchanting, Tinkering, Trading, Charisma).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EnterpriseConfig {
    #[serde(default)]
    pub smithing: SmithingConfig,
    #[serde(default)]
    pub repair: RepairConfig,
    #[serde(default)]
    pub salvage: SalvageConfig,
    #[serde(default)]
    pub alchemy: AlchemyConfig,
    #[serde(default)]
    pub enchanting: EnchantingConfig,
    #[serde(default)]
    pub tinkering: TinkeringConfig,
    #[serde(default)]
    pub trading: TradingConfig,
    #[serde(default)]
    pub charisma: CharismaConfig,
}

/// Smithing: craft/smelt XP and durable creator/provenance markers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmithingConfig {
    /// XP per crafted item, keyed by item registry key.
    #[serde(default = "default_smithing_craft_xp")]
    pub craft_xp: HashMap<String, u64>,
    /// XP per smelted item extracted from a furnace, keyed by registry key.
    #[serde(default = "default_smelt_xp")]
    pub smelt_xp: HashMap<String, u64>,
    /// XP for furnace extraction without a configured value.
    pub default_smelt_xp: u64,
    /// Tag anvil outputs with creator/provenance item data.
    #[serde(default = "default_true")]
    pub mark_anvil_outputs: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SmithingConfig {
    fn default() -> Self {
        Self {
            craft_xp: default_smithing_craft_xp(),
            smelt_xp: default_smelt_xp(),
            default_smelt_xp: 2,
            mark_anvil_outputs: true,
        }
    }
}

fn default_smithing_craft_xp() -> HashMap<String, u64> {
    [
        ("iron_pickaxe", 15),
        ("iron_axe", 15),
        ("iron_shovel", 12),
        ("iron_hoe", 12),
        ("iron_sword", 15),
        ("iron_helmet", 18),
        ("iron_chestplate", 25),
        ("iron_leggings", 22),
        ("iron_boots", 15),
        ("golden_pickaxe", 20),
        ("golden_axe", 20),
        ("golden_sword", 20),
        ("diamond_pickaxe", 40),
        ("diamond_axe", 40),
        ("diamond_shovel", 35),
        ("diamond_hoe", 35),
        ("diamond_sword", 40),
        ("diamond_helmet", 45),
        ("diamond_chestplate", 60),
        ("diamond_leggings", 55),
        ("diamond_boots", 45),
        ("netherite_pickaxe", 80),
        ("netherite_axe", 80),
        ("netherite_sword", 80),
        ("netherite_helmet", 90),
        ("netherite_chestplate", 110),
        ("netherite_leggings", 100),
        ("netherite_boots", 90),
        ("shears", 8),
        ("flint_and_steel", 8),
        ("shield", 12),
        ("bucket", 8),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

fn default_smelt_xp() -> HashMap<String, u64> {
    [
        ("iron_ingot", 4),
        ("gold_ingot", 6),
        ("copper_ingot", 3),
        ("netherite_scrap", 40),
        ("glass", 1),
        ("brick", 2),
        ("nether_brick", 2),
        ("smooth_stone", 1),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

/// Repair: anvil cost discount in the prepare preview, XP on take.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairConfig {
    /// Level-cost reduction per Repair level.
    pub discount_per_level: f64,
    /// Hard cap for the level-cost reduction.
    pub discount_cap: f64,
    /// Cooldown between discounted repairs, in ticks. Charged only when the
    /// discounted output is taken.
    pub cooldown_ticks: u32,
    /// XP per completed repair.
    pub xp: u64,
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            discount_per_level: 0.05,
            discount_cap: 10.0,
            cooldown_ticks: 100,
            xp: 20,
        }
    }
}

/// Salvage: grindstone experience bonus and material-recovery rolls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SalvageConfig {
    /// Fractional experience bonus per Salvage level.
    pub xp_bonus_per_level: f64,
    /// Hard cap for the experience bonus fraction.
    pub xp_bonus_cap: f64,
    /// Cooldown between bonused grindstone takes, in ticks.
    pub cooldown_ticks: u32,
    /// XP per completed grindstone take.
    pub xp: u64,
    /// Chance to recover a material item from the input's tool tier.
    pub recovery_chance: f64,
    /// Tool-tier prefix → recovered material registry key.
    #[serde(default = "default_recovery_materials")]
    pub recovery_materials: HashMap<String, String>,
}

impl Default for SalvageConfig {
    fn default() -> Self {
        Self {
            xp_bonus_per_level: 0.002,
            xp_bonus_cap: 0.25,
            cooldown_ticks: 60,
            xp: 15,
            recovery_chance: 0.10,
            recovery_materials: default_recovery_materials(),
        }
    }
}

fn default_recovery_materials() -> HashMap<String, String> {
    [
        ("wooden", "oak_planks"),
        ("stone", "cobblestone"),
        ("iron", "iron_ingot"),
        ("golden", "gold_ingot"),
        ("diamond", "diamond"),
        ("netherite", "netherite_scrap"),
    ]
    .into_iter()
    .map(|(tier, item)| (tier.to_string(), item.to_string()))
    .collect()
}

/// Alchemy: XP for consuming potions.
///
/// Brewing itself is not attributable (`BrewEvent` carries no player) and
/// potency/duration mutation of applied effects has no safe hook, so both
/// stay documented as blocked in `src/mmo/plan.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlchemyConfig {
    /// XP per consumed potion, keyed by item registry key.
    #[serde(default = "default_potion_xp")]
    pub potion_xp: HashMap<String, u64>,
    /// Legacy compatibility value retained in serialized configs. Runtime
    /// rewards require an explicit `potion_xp` key so ordinary foods cannot
    /// accidentally earn Alchemy XP.
    pub default_potion_xp: u64,
}

impl Default for AlchemyConfig {
    fn default() -> Self {
        Self {
            potion_xp: default_potion_xp(),
            default_potion_xp: 8,
        }
    }
}

fn default_potion_xp() -> HashMap<String, u64> {
    [
        ("potion", 10),
        ("splash_potion", 12),
        ("lingering_potion", 14),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

/// Enchanting: offer discount in the generate preview, XP on commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnchantingConfig {
    /// Offer level-requirement reduction per Enchanting level.
    pub offer_discount_per_level: f64,
    /// Hard cap for the offer level-requirement reduction.
    pub offer_discount_cap: f64,
    /// XP granted per level of the commit's level cost.
    pub xp_per_level_cost: f64,
    /// Maximum XP from a single enchant.
    pub xp_cap: u64,
}

impl Default for EnchantingConfig {
    fn default() -> Self {
        Self {
            offer_discount_per_level: 0.02,
            offer_discount_cap: 5.0,
            xp_per_level_cost: 5.0,
            xp_cap: 100,
        }
    }
}

/// Tinkering: mechanism crafting XP and custom-item provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TinkeringConfig {
    /// XP per crafted mechanism, keyed by item registry key.
    #[serde(default = "default_tinkering_craft_xp")]
    pub craft_xp: HashMap<String, u64>,
}

impl Default for TinkeringConfig {
    fn default() -> Self {
        Self {
            craft_xp: default_tinkering_craft_xp(),
        }
    }
}

fn default_tinkering_craft_xp() -> HashMap<String, u64> {
    [
        ("piston", 15),
        ("sticky_piston", 18),
        ("dispenser", 15),
        ("dropper", 12),
        ("hopper", 15),
        ("observer", 15),
        ("repeater", 10),
        ("comparator", 12),
        ("redstone_torch", 6),
        ("daylight_detector", 15),
        ("lever", 4),
        ("tripwire_hook", 6),
        ("note_block", 8),
        ("rail", 4),
        ("powered_rail", 10),
        ("detector_rail", 10),
        ("activator_rail", 10),
    ]
    .into_iter()
    .map(|(name, xp)| (name.to_string(), xp))
    .collect()
}

/// Trading: configuration and reputation ledger only.
///
/// **Blocked** until Pumpkin exposes a villager-trade commit transaction;
/// no prices are ever modified. The reputation ledger (`rep_v1` player data)
/// records faction standing for future trade and Charisma effects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradingConfig {
    /// Master switch; stays false while the trade transaction is missing.
    pub enabled: bool,
    /// Reputation gained per completed trade, keyed by faction. Unused while
    /// `enabled` is false.
    #[serde(default)]
    pub reputation_gains: HashMap<String, i32>,
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            reputation_gains: HashMap::new(),
        }
    }
}

/// Charisma: configuration only.
///
/// **Blocked** for effects: there is no general economy/NPC transaction in
/// Pumpkin to hook. Reputation effects arrive with the Trading transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CharismaConfig {
    /// Master switch; stays false while effects have no transaction path.
    pub enabled: bool,
}

impl Default for CharismaConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

impl EnterpriseConfig {
    /// Clamp out-of-range values into safe bounds.
    pub fn sanitized(mut self) -> Self {
        let clamp_nonnegative = |value: &mut f64| {
            if !value.is_finite() || *value < 0.0 {
                *value = 0.0;
            }
        };
        clamp_nonnegative(&mut self.repair.discount_per_level);
        clamp_nonnegative(&mut self.repair.discount_cap);
        clamp_nonnegative(&mut self.salvage.xp_bonus_per_level);
        clamp_nonnegative(&mut self.salvage.xp_bonus_cap);
        clamp_nonnegative(&mut self.salvage.recovery_chance);
        self.salvage.recovery_chance = self.salvage.recovery_chance.min(1.0);
        clamp_nonnegative(&mut self.enchanting.offer_discount_per_level);
        clamp_nonnegative(&mut self.enchanting.offer_discount_cap);
        clamp_nonnegative(&mut self.enchanting.xp_per_level_cost);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_common_smithing_items() {
        let config = SmithingConfig::default();
        assert!(config.craft_xp.contains_key("iron_pickaxe"));
        assert!(config.craft_xp.contains_key("netherite_chestplate"));
        assert!(config.smelt_xp.contains_key("iron_ingot"));
    }

    #[test]
    fn recovery_materials_cover_vanilla_tiers() {
        let config = SalvageConfig::default();
        assert_eq!(
            config.recovery_materials.get("iron").map(String::as_str),
            Some("iron_ingot")
        );
        assert_eq!(
            config
                .recovery_materials
                .get("netherite")
                .map(String::as_str),
            Some("netherite_scrap")
        );
    }

    #[test]
    fn trading_and_charisma_are_disabled_by_default() {
        let config = EnterpriseConfig::default();
        assert!(!config.trading.enabled);
        assert!(!config.charisma.enabled);
    }
}
