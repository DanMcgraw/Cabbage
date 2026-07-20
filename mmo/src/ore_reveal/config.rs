use std::collections::HashMap;

use pumpkin_data::{Block, biome::Biome};
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

fn default_max_total_frequency() -> f64 {
    0.25
}

fn default_max_radius() -> u32 {
    5
}

fn default_branch_chance() -> f64 {
    0.22
}

fn default_forward_bias() -> f64 {
    1.6
}

fn default_max_vein_size() -> u32 {
    32
}

fn default_host_blocks() -> Vec<String> {
    vec!["stone".to_string(), "deepslate".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OreRevealConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_host_blocks")]
    pub host_blocks: Vec<String>,
    #[serde(default = "default_max_total_frequency")]
    pub max_total_frequency: f64,
    #[serde(default)]
    pub shape: VeinShapeConfig,
    #[serde(default = "default_ores")]
    pub ores: Vec<OreConfig>,
    #[serde(default = "default_biome_multipliers")]
    pub biome_multipliers: HashMap<String, BiomeMultiplierConfig>,
}

impl Default for OreRevealConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            host_blocks: default_host_blocks(),
            max_total_frequency: default_max_total_frequency(),
            shape: VeinShapeConfig::default(),
            ores: default_ores(),
            biome_multipliers: default_biome_multipliers(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VeinShapeConfig {
    #[serde(default = "default_max_radius")]
    pub max_radius: u32,
    #[serde(default = "default_branch_chance")]
    pub branch_chance: f64,
    #[serde(default = "default_forward_bias")]
    pub forward_bias: f64,
    #[serde(default = "default_true")]
    pub require_hidden_targets: bool,
    #[serde(default = "default_max_vein_size")]
    pub max_vein_size: u32,
}

impl Default for VeinShapeConfig {
    fn default() -> Self {
        Self {
            max_radius: default_max_radius(),
            branch_chance: default_branch_chance(),
            forward_bias: default_forward_bias(),
            require_hidden_targets: true,
            max_vein_size: default_max_vein_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OreConfig {
    pub id: String,
    pub stone_block: String,
    pub deepslate_block: String,
    pub base_frequency: f64,
    pub size: VeinSizeConfig,
    #[serde(default)]
    pub height_bands: Vec<HeightBandConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VeinSizeConfig {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct HeightBandConfig {
    pub min_y: i32,
    pub max_y: i32,
    #[serde(default = "one")]
    pub frequency: f64,
    #[serde(default = "one")]
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BiomeMultiplierConfig {
    #[serde(default = "one")]
    pub frequency: f64,
    #[serde(default = "one")]
    pub size: f64,
    #[serde(default)]
    pub ore_frequency: HashMap<String, f64>,
    #[serde(default)]
    pub ore_size: HashMap<String, f64>,
}

impl Default for BiomeMultiplierConfig {
    fn default() -> Self {
        Self {
            frequency: 1.0,
            size: 1.0,
            ore_frequency: HashMap::new(),
            ore_size: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompiledOreRevealConfig {
    pub enabled: bool,
    pub hosts: Vec<&'static Block>,
    pub max_total_frequency: f64,
    pub shape: VeinShapeConfig,
    pub ores: Vec<CompiledOre>,
    pub biome_multipliers: HashMap<String, BiomeMultiplierConfig>,
}

#[derive(Debug, Clone)]
pub(super) struct CompiledOre {
    pub id: String,
    pub stone_block: &'static Block,
    pub deepslate_block: &'static Block,
    pub base_frequency: f64,
    pub size: VeinSizeConfig,
    pub height_bands: Vec<HeightBandConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct EffectiveOre {
    pub index: usize,
    pub frequency: f64,
    pub size_multiplier: f64,
}

impl OreRevealConfig {
    pub(super) fn compile(&self) -> Result<CompiledOreRevealConfig, String> {
        validate_unit("max_total_frequency", self.max_total_frequency)?;
        if self.shape.max_radius == 0 || self.shape.max_radius > 16 {
            return Err("ore_reveal.shape.max_radius must be between 1 and 16".to_string());
        }
        validate_unit("shape.branch_chance", self.shape.branch_chance)?;
        validate_positive("shape.forward_bias", self.shape.forward_bias)?;
        if self.shape.max_vein_size == 0 || self.shape.max_vein_size > 128 {
            return Err("ore_reveal.shape.max_vein_size must be between 1 and 128".to_string());
        }

        let mut hosts = Vec::with_capacity(self.host_blocks.len());
        for name in &self.host_blocks {
            let block = Block::from_name(name)
                .ok_or_else(|| format!("ore_reveal host block '{name}' does not exist"))?;
            if !hosts.contains(&block) {
                hosts.push(block);
            }
        }
        if hosts.is_empty() {
            return Err("ore_reveal.host_blocks cannot be empty".to_string());
        }

        let mut seen_ids = std::collections::HashSet::new();
        let mut ores = Vec::with_capacity(self.ores.len());
        for ore in &self.ores {
            if ore.id.trim().is_empty() || !seen_ids.insert(ore.id.as_str()) {
                return Err(format!(
                    "ore_reveal ore id '{}' is empty or duplicated",
                    ore.id
                ));
            }
            validate_unit(
                &format!("ore '{}'.base_frequency", ore.id),
                ore.base_frequency,
            )?;
            if ore.size.min == 0 || ore.size.min > ore.size.max {
                return Err(format!("ore '{}' has an invalid size range", ore.id));
            }
            if ore.size.max > self.shape.max_vein_size {
                return Err(format!(
                    "ore '{}' size exceeds shape.max_vein_size ({})",
                    ore.id, self.shape.max_vein_size
                ));
            }
            validate_height_bands(&ore.id, &ore.height_bands)?;
            let stone_block = Block::from_name(&ore.stone_block).ok_or_else(|| {
                format!(
                    "ore '{}' block '{}' does not exist",
                    ore.id, ore.stone_block
                )
            })?;
            let deepslate_block = Block::from_name(&ore.deepslate_block).ok_or_else(|| {
                format!(
                    "ore '{}' block '{}' does not exist",
                    ore.id, ore.deepslate_block
                )
            })?;
            ores.push(CompiledOre {
                id: ore.id.clone(),
                stone_block,
                deepslate_block,
                base_frequency: ore.base_frequency,
                size: ore.size,
                height_bands: ore.height_bands.clone(),
            });
        }

        for (biome, multiplier) in &self.biome_multipliers {
            if Biome::from_name(biome).is_none() {
                return Err(format!("ore_reveal biome '{biome}' does not exist"));
            }
            validate_positive(&format!("biome '{biome}'.frequency"), multiplier.frequency)?;
            validate_positive(&format!("biome '{biome}'.size"), multiplier.size)?;
            for (ore, value) in &multiplier.ore_frequency {
                if !seen_ids.contains(ore.as_str()) {
                    return Err(format!("biome '{biome}' references unknown ore '{ore}'"));
                }
                validate_positive(&format!("biome '{biome}' ore '{ore}' frequency"), *value)?;
            }
            for (ore, value) in &multiplier.ore_size {
                if !seen_ids.contains(ore.as_str()) {
                    return Err(format!("biome '{biome}' references unknown ore '{ore}'"));
                }
                validate_positive(&format!("biome '{biome}' ore '{ore}' size"), *value)?;
            }
        }

        Ok(CompiledOreRevealConfig {
            enabled: self.enabled,
            hosts,
            max_total_frequency: self.max_total_frequency,
            shape: self.shape.clone(),
            ores,
            biome_multipliers: self.biome_multipliers.clone(),
        })
    }
}

impl CompiledOreRevealConfig {
    pub fn is_host(&self, block: &Block) -> bool {
        self.hosts.iter().any(|host| *host == block)
    }

    pub fn effective_ores(&self, biome: &str, y: i32) -> Vec<EffectiveOre> {
        let biome_rule = self.biome_multipliers.get(biome);
        self.ores
            .iter()
            .enumerate()
            .filter_map(|(index, ore)| {
                let (height_frequency, height_size) = if ore.height_bands.is_empty() {
                    (1.0, 1.0)
                } else {
                    let height = ore
                        .height_bands
                        .iter()
                        .find(|band| (band.min_y..=band.max_y).contains(&y))?;
                    (height.frequency, height.size)
                };
                let biome_frequency = biome_rule.map_or(1.0, |rule| {
                    rule.frequency * rule.ore_frequency.get(&ore.id).copied().unwrap_or(1.0)
                });
                let biome_size = biome_rule.map_or(1.0, |rule| {
                    rule.size * rule.ore_size.get(&ore.id).copied().unwrap_or(1.0)
                });
                let frequency = ore.base_frequency * height_frequency * biome_frequency;
                (frequency > 0.0).then_some(EffectiveOre {
                    index,
                    frequency,
                    size_multiplier: height_size * biome_size,
                })
            })
            .collect()
    }

    pub fn select_ore(
        &self,
        biome: &str,
        y: i32,
        trigger_roll: f64,
        ore_roll: f64,
    ) -> Option<EffectiveOre> {
        let effective = self.effective_ores(biome, y);
        let total = effective.iter().map(|ore| ore.frequency).sum::<f64>();
        let trigger_chance = total.min(self.max_total_frequency);
        if total <= 0.0 || trigger_roll >= trigger_chance {
            return None;
        }

        let mut cursor = ore_roll.clamp(0.0, 1.0 - f64::EPSILON) * total;
        for ore in effective {
            if cursor < ore.frequency {
                return Some(ore);
            }
            cursor -= ore.frequency;
        }
        None
    }
}

fn validate_height_bands(ore_id: &str, bands: &[HeightBandConfig]) -> Result<(), String> {
    for (index, band) in bands.iter().enumerate() {
        if band.min_y > band.max_y {
            return Err(format!(
                "ore '{ore_id}' has a height band with min_y > max_y"
            ));
        }
        validate_positive(&format!("ore '{ore_id}' height frequency"), band.frequency)?;
        validate_positive(&format!("ore '{ore_id}' height size"), band.size)?;
        if bands
            .iter()
            .skip(index + 1)
            .any(|other| band.min_y <= other.max_y && other.min_y <= band.max_y)
        {
            return Err(format!("ore '{ore_id}' has overlapping height bands"));
        }
    }
    Ok(())
}

fn validate_unit(name: &str, value: f64) -> Result<(), String> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(format!("ore_reveal.{name} must be between 0 and 1"))
    }
}

fn validate_positive(name: &str, value: f64) -> Result<(), String> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(format!("ore_reveal.{name} must be finite and nonnegative"))
    }
}

fn ore(
    id: &str,
    base_frequency: f64,
    min: u32,
    max: u32,
    height_bands: Vec<HeightBandConfig>,
) -> OreConfig {
    OreConfig {
        id: id.to_string(),
        stone_block: format!("{id}_ore"),
        deepslate_block: format!("deepslate_{id}_ore"),
        base_frequency,
        size: VeinSizeConfig { min, max },
        height_bands,
    }
}

fn band(min_y: i32, max_y: i32, frequency: f64, size: f64) -> HeightBandConfig {
    HeightBandConfig {
        min_y,
        max_y,
        frequency,
        size,
    }
}

fn default_ores() -> Vec<OreConfig> {
    vec![
        ore("coal", 0.012, 6, 12, vec![band(0, 192, 1.0, 1.0)]),
        ore("iron", 0.008, 4, 8, vec![band(-64, 128, 1.0, 1.0)]),
        ore("copper", 0.007, 6, 12, vec![band(-16, 112, 1.0, 1.0)]),
        ore("gold", 0.002, 3, 6, vec![band(-64, 48, 1.0, 1.0)]),
        ore("redstone", 0.0035, 4, 8, vec![band(-64, 16, 1.0, 1.0)]),
        ore("lapis", 0.0018, 3, 6, vec![band(-64, 64, 1.0, 1.0)]),
        ore("diamond", 0.001, 2, 4, vec![band(-64, 16, 1.0, 1.0)]),
        ore("emerald", 0.0004, 2, 3, vec![band(-16, 256, 1.0, 1.0)]),
    ]
}

fn default_biome_multipliers() -> HashMap<String, BiomeMultiplierConfig> {
    let mut result = HashMap::new();

    let mut badlands = BiomeMultiplierConfig::default();
    badlands.ore_frequency.insert("gold".to_string(), 2.5);
    badlands.ore_size.insert("gold".to_string(), 1.2);
    result.insert("badlands".to_string(), badlands);

    for biome in ["stony_peaks", "windswept_hills", "meadow"] {
        let mut mountain = BiomeMultiplierConfig::default();
        mountain.ore_frequency.insert("emerald".to_string(), 3.0);
        result.insert(biome.to_string(), mountain);
    }

    let mut dripstone = BiomeMultiplierConfig::default();
    dripstone.ore_frequency.insert("copper".to_string(), 2.0);
    dripstone.ore_size.insert("copper".to_string(), 1.25);
    result.insert("dripstone_caves".to_string(), dripstone);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_compile() {
        OreRevealConfig::default().compile().unwrap();
    }

    #[test]
    fn height_and_biome_multipliers_apply() {
        let compiled = OreRevealConfig::default().compile().unwrap();
        let normal = compiled
            .effective_ores("plains", 20)
            .into_iter()
            .find(|ore| compiled.ores[ore.index].id == "gold")
            .unwrap();
        let badlands = compiled
            .effective_ores("badlands", 20)
            .into_iter()
            .find(|ore| compiled.ores[ore.index].id == "gold")
            .unwrap();
        assert_eq!(badlands.frequency, normal.frequency * 2.5);
        assert_eq!(badlands.size_multiplier, normal.size_multiplier * 1.2);
    }

    #[test]
    fn selection_has_a_no_vein_outcome() {
        let compiled = OreRevealConfig::default().compile().unwrap();
        assert!(compiled.select_ore("plains", 12, 0.99, 0.0).is_none());
        assert!(compiled.select_ore("plains", 12, 0.0, 0.0).is_some());
    }

    #[test]
    fn overlapping_height_bands_are_rejected() {
        let mut config = OreRevealConfig::default();
        config.ores[0].height_bands = vec![band(0, 10, 1.0, 1.0), band(10, 20, 1.0, 1.0)];
        assert!(config.compile().is_err());
    }

    #[test]
    fn empty_height_bands_apply_at_every_height() {
        let mut config = OreRevealConfig::default();
        config.ores[0].height_bands.clear();
        let compiled = config.compile().unwrap();
        assert!(
            compiled
                .effective_ores("plains", -2048)
                .iter()
                .any(|ore| compiled.ores[ore.index].id == "coal")
        );
    }
}
