//! Warfare branch configuration: per-skill XP values and bounded perk knobs.
//!
//! Damage modifiers are multipliers on the event's final damage and are
//! clamped both by their per-skill caps and the global
//! `config::PerkConfig::max_damage_multiplier`. Reduction effects have their
//! own caps so incoming damage can never be amplified by a perk.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Warfare branch configuration (Blades, Axes, Archery, Athletics, Defense,
/// Sorcery). The `unarmed` and `acrobatics` sections configure the retired
/// activities' knobs; both now feed and read the shared Athletics track.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WarfareConfig {
    #[serde(default)]
    pub blades: BladesConfig,
    #[serde(default)]
    pub axes: AxesConfig,
    #[serde(default)]
    pub archery: ArcheryConfig,
    #[serde(default)]
    pub unarmed: UnarmedConfig,
    #[serde(default)]
    pub defense: DefenseConfig,
    #[serde(default)]
    pub acrobatics: AcrobaticsConfig,
    #[serde(default)]
    pub sorcery: SorceryConfig,
}

/// Blades: sword damage bonus and the cooldown-gated Riposte perk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BladesConfig {
    /// Damage multiplier gained per Blades level.
    pub damage_bonus_per_level: f64,
    /// Hard cap for the total Blades damage multiplier.
    pub damage_bonus_cap: f64,
    /// Bonus damage when striking within a short window after taking damage.
    #[serde(default = "default_true")]
    pub riposte_enabled: bool,
    /// Window after taking damage in which Riposte can trigger, in ticks.
    pub riposte_window_ticks: u32,
    /// Cooldown between Riposte triggers, in ticks.
    pub riposte_cooldown_ticks: u32,
    /// Extra damage multiplier applied by Riposte.
    pub riposte_bonus_multiplier: f64,
    /// Additional Riposte damage multiplier per perk tier.
    #[serde(default = "default_riposte_multiplier_per_tier")]
    pub riposte_multiplier_per_tier: f64,
    /// Riposte cooldown reduction per perk tier, in ticks.
    #[serde(default = "default_riposte_cooldown_reduction_per_tier")]
    pub riposte_cooldown_reduction_per_tier: u32,
}

impl Default for BladesConfig {
    fn default() -> Self {
        Self {
            damage_bonus_per_level: 0.004,
            damage_bonus_cap: 0.5,
            riposte_enabled: true,
            riposte_window_ticks: 60,
            riposte_cooldown_ticks: 200,
            riposte_bonus_multiplier: 0.25,
            riposte_multiplier_per_tier: default_riposte_multiplier_per_tier(),
            riposte_cooldown_reduction_per_tier: default_riposte_cooldown_reduction_per_tier(),
        }
    }
}

fn default_riposte_multiplier_per_tier() -> f64 {
    0.05
}

fn default_riposte_cooldown_reduction_per_tier() -> u32 {
    20
}

/// Axes: axe damage bonus. Armor/shield-oriented effects stay disabled until
/// Pumpkin exposes that state (see plan Warfare rules).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AxesConfig {
    /// Damage multiplier gained per Axes level.
    pub damage_bonus_per_level: f64,
    /// Hard cap for the total Axes damage multiplier.
    pub damage_bonus_cap: f64,
    /// Additional damage cap per perk tier.
    #[serde(default = "default_axes_damage_cap_per_tier")]
    pub damage_cap_per_tier: f64,
}

impl Default for AxesConfig {
    fn default() -> Self {
        Self {
            damage_bonus_per_level: 0.005,
            damage_bonus_cap: 0.6,
            damage_cap_per_tier: default_axes_damage_cap_per_tier(),
        }
    }
}

fn default_axes_damage_cap_per_tier() -> f64 {
    0.05
}

/// Archery: hit-based XP and a level-scaled arrow damage bonus. Kill XP is
/// attributed once through `PlayerKillEntityEvent`, like every weapon skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArcheryConfig {
    /// XP per projectile hit on a living target.
    pub hit_xp: u64,
    /// Master switch for the arrow damage bonus.
    #[serde(default = "default_true")]
    pub damage_enabled: bool,
    /// Damage multiplier gained per Archery level.
    #[serde(default = "default_archery_damage_bonus_per_level")]
    pub damage_bonus_per_level: f64,
    /// Hard cap for the Archery damage multiplier.
    #[serde(default = "default_archery_damage_bonus_cap")]
    pub damage_bonus_cap: f64,
    /// Additional damage cap per perk tier.
    #[serde(default = "default_archery_damage_cap_per_tier")]
    pub damage_cap_per_tier: f64,
}

impl Default for ArcheryConfig {
    fn default() -> Self {
        Self {
            hit_xp: 4,
            damage_enabled: true,
            damage_bonus_per_level: default_archery_damage_bonus_per_level(),
            damage_bonus_cap: default_archery_damage_bonus_cap(),
            damage_cap_per_tier: default_archery_damage_cap_per_tier(),
        }
    }
}

fn default_archery_damage_bonus_per_level() -> f64 {
    0.004
}

fn default_archery_damage_bonus_cap() -> f64 {
    0.5
}

fn default_archery_damage_cap_per_tier() -> f64 {
    0.05
}

/// Unarmed: empty-hand damage and knockback bonuses. These knobs gate on the
/// merged Athletics level since the six-skill consolidation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnarmedConfig {
    /// Damage multiplier gained per Athletics level.
    pub damage_bonus_per_level: f64,
    /// Hard cap for the total empty-hand damage multiplier.
    pub damage_bonus_cap: f64,
    /// Knockback multiplier gained per Athletics level.
    pub knockback_bonus_per_level: f64,
    /// Hard cap for the knockback multiplier bonus.
    pub knockback_cap: f64,
    /// Additional knockback cap per perk tier.
    #[serde(default = "default_knockback_cap_per_tier")]
    pub knockback_cap_per_tier: f64,
}

impl Default for UnarmedConfig {
    fn default() -> Self {
        Self {
            damage_bonus_per_level: 0.003,
            damage_bonus_cap: 0.4,
            knockback_bonus_per_level: 0.004,
            knockback_cap: 0.5,
            knockback_cap_per_tier: default_knockback_cap_per_tier(),
        }
    }
}

fn default_knockback_cap_per_tier() -> f64 {
    0.05
}

/// Defense: damage-taken XP and a bounded incoming-damage reduction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DefenseConfig {
    /// XP per point of incoming damage (before reductions).
    pub xp_per_damage: f64,
    /// Maximum Defense XP from a single hit.
    pub xp_cap_per_hit: u64,
    /// Incoming-damage reduction gained per Defense level.
    pub reduction_per_level: f64,
    /// Hard cap for the damage reduction (0.15 = 15%).
    pub reduction_cap: f64,
    /// Additional reduction cap per perk tier.
    #[serde(default = "default_defense_reduction_cap_per_tier")]
    pub reduction_cap_per_tier: f64,
}

impl Default for DefenseConfig {
    fn default() -> Self {
        Self {
            xp_per_damage: 2.0,
            xp_cap_per_hit: 40,
            reduction_per_level: 0.0015,
            reduction_cap: 0.15,
            reduction_cap_per_tier: default_defense_reduction_cap_per_tier(),
        }
    }
}

fn default_defense_reduction_cap_per_tier() -> f64 {
    0.025
}

/// Acrobatics: fall XP and the bounded safe-landing roll. The awards and the
/// roll use the merged Athletics track since the six-skill consolidation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcrobaticsConfig {
    /// XP per point of fall damage (before reductions).
    pub xp_per_fall_damage: f64,
    /// Maximum Athletics XP from a single fall.
    pub xp_cap_per_fall: u64,
    /// Fall-damage reduction gained per Athletics level.
    pub roll_reduction_per_level: f64,
    /// Hard cap for the roll reduction (0.25 = 25%).
    pub roll_reduction_cap: f64,
    /// Additional roll reduction cap per perk tier.
    #[serde(default = "default_roll_reduction_cap_per_tier")]
    pub roll_reduction_cap_per_tier: f64,
}

impl Default for AcrobaticsConfig {
    fn default() -> Self {
        Self {
            xp_per_fall_damage: 3.0,
            xp_cap_per_fall: 60,
            roll_reduction_per_level: 0.002,
            roll_reduction_cap: 0.25,
            roll_reduction_cap_per_tier: default_roll_reduction_cap_per_tier(),
        }
    }
}

fn default_roll_reduction_cap_per_tier() -> f64 {
    0.05
}

/// Sorcery: mana state and the first staff activation path.
///
/// Mana is in-memory (volatile by design); a restart refills it. All effect
/// magnitudes are bounded here and by the global perk caps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SorceryConfig {
    /// Registry key of the item that acts as a staff.
    pub staff_item: String,
    /// Maximum mana.
    pub mana_max: f64,
    /// Additional maximum mana per perk tier.
    #[serde(default = "default_mana_max_per_tier")]
    pub mana_max_per_tier: f64,
    /// Mana regenerated per server tick.
    pub mana_regen_per_tick: f64,
    /// Mana cost of the healing bolt.
    pub spell_mana_cost: f64,
    /// Health restored by the healing bolt (2.0 = one heart).
    pub spell_heal: f32,
    /// Additional healing-bolt health per perk tier (2.0 = one heart).
    #[serde(default = "default_spell_heal_per_tier")]
    pub spell_heal_per_tier: f32,
    /// Cooldown between casts, in ticks.
    pub spell_cooldown_ticks: u32,
    /// Cast cooldown reduction per perk tier, in ticks.
    #[serde(default = "default_spell_cooldown_reduction_per_tier")]
    pub spell_cooldown_reduction_per_tier: u32,
    /// Sorcery XP per cast.
    pub spell_xp: u64,
}

impl Default for SorceryConfig {
    fn default() -> Self {
        Self {
            staff_item: "blaze_rod".to_string(),
            mana_max: 100.0,
            mana_max_per_tier: default_mana_max_per_tier(),
            mana_regen_per_tick: 0.05,
            spell_mana_cost: 25.0,
            spell_heal: 4.0,
            spell_heal_per_tier: default_spell_heal_per_tier(),
            spell_cooldown_ticks: 100,
            spell_cooldown_reduction_per_tier: default_spell_cooldown_reduction_per_tier(),
            spell_xp: 15,
        }
    }
}

fn default_mana_max_per_tier() -> f64 {
    10.0
}

fn default_spell_heal_per_tier() -> f32 {
    1.0
}

fn default_spell_cooldown_reduction_per_tier() -> u32 {
    10
}

impl WarfareConfig {
    /// Clamp out-of-range values into safe bounds.
    pub fn sanitized(mut self) -> Self {
        let clamp_nonnegative = |value: &mut f64| {
            if !value.is_finite() || *value < 0.0 {
                *value = 0.0;
            }
        };
        clamp_nonnegative(&mut self.blades.damage_bonus_per_level);
        clamp_nonnegative(&mut self.blades.damage_bonus_cap);
        clamp_nonnegative(&mut self.blades.riposte_bonus_multiplier);
        clamp_nonnegative(&mut self.blades.riposte_multiplier_per_tier);
        clamp_nonnegative(&mut self.axes.damage_bonus_per_level);
        clamp_nonnegative(&mut self.axes.damage_bonus_cap);
        clamp_nonnegative(&mut self.axes.damage_cap_per_tier);
        clamp_nonnegative(&mut self.archery.damage_bonus_per_level);
        clamp_nonnegative(&mut self.archery.damage_bonus_cap);
        clamp_nonnegative(&mut self.archery.damage_cap_per_tier);
        clamp_nonnegative(&mut self.unarmed.damage_bonus_per_level);
        clamp_nonnegative(&mut self.unarmed.damage_bonus_cap);
        clamp_nonnegative(&mut self.unarmed.knockback_bonus_per_level);
        clamp_nonnegative(&mut self.unarmed.knockback_cap);
        clamp_nonnegative(&mut self.unarmed.knockback_cap_per_tier);
        clamp_nonnegative(&mut self.defense.xp_per_damage);
        clamp_nonnegative(&mut self.defense.reduction_per_level);
        self.defense.reduction_cap = self.defense.reduction_cap.clamp(0.0, 0.9);
        self.defense.reduction_cap_per_tier = self.defense.reduction_cap_per_tier.clamp(0.0, 0.9);
        clamp_nonnegative(&mut self.acrobatics.xp_per_fall_damage);
        clamp_nonnegative(&mut self.acrobatics.roll_reduction_per_level);
        self.acrobatics.roll_reduction_cap = self.acrobatics.roll_reduction_cap.clamp(0.0, 0.9);
        self.acrobatics.roll_reduction_cap_per_tier =
            self.acrobatics.roll_reduction_cap_per_tier.clamp(0.0, 0.9);
        clamp_nonnegative(&mut self.sorcery.mana_max);
        clamp_nonnegative(&mut self.sorcery.mana_max_per_tier);
        clamp_nonnegative(&mut self.sorcery.mana_regen_per_tick);
        clamp_nonnegative(&mut self.sorcery.spell_mana_cost);
        if !self.sorcery.spell_heal.is_finite() || self.sorcery.spell_heal < 0.0 {
            self.sorcery.spell_heal = 0.0;
        }
        if !self.sorcery.spell_heal_per_tier.is_finite() || self.sorcery.spell_heal_per_tier < 0.0 {
            self.sorcery.spell_heal_per_tier = 0.0;
        }
        if self.sorcery.staff_item.is_empty() {
            self.sorcery.staff_item = "blaze_rod".to_string();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_clamps_reduction_caps() {
        let mut config = WarfareConfig::default();
        config.defense.reduction_cap = 5.0;
        config.acrobatics.roll_reduction_cap = -1.0;
        let config = config.sanitized();
        assert_eq!(config.defense.reduction_cap, 0.9);
        assert_eq!(config.acrobatics.roll_reduction_cap, 0.0);
    }

    #[test]
    fn sanitized_clamps_tier_knobs() {
        let mut config = WarfareConfig::default();
        config.blades.riposte_multiplier_per_tier = f64::NAN;
        config.axes.damage_cap_per_tier = -1.0;
        config.archery.damage_bonus_per_level = f64::INFINITY;
        config.archery.damage_bonus_cap = -0.5;
        config.archery.damage_cap_per_tier = -1.0;
        config.unarmed.knockback_cap_per_tier = -1.0;
        config.defense.reduction_cap_per_tier = 5.0;
        config.acrobatics.roll_reduction_cap_per_tier = -1.0;
        config.sorcery.mana_max_per_tier = -10.0;
        config.sorcery.spell_heal_per_tier = f32::NAN;
        let config = config.sanitized();
        assert_eq!(config.blades.riposte_multiplier_per_tier, 0.0);
        assert_eq!(config.axes.damage_cap_per_tier, 0.0);
        assert_eq!(config.archery.damage_bonus_per_level, 0.0);
        assert_eq!(config.archery.damage_bonus_cap, 0.0);
        assert_eq!(config.archery.damage_cap_per_tier, 0.0);
        assert_eq!(config.unarmed.knockback_cap_per_tier, 0.0);
        assert_eq!(config.defense.reduction_cap_per_tier, 0.9);
        assert_eq!(config.acrobatics.roll_reduction_cap_per_tier, 0.0);
        assert_eq!(config.sorcery.mana_max_per_tier, 0.0);
        assert_eq!(config.sorcery.spell_heal_per_tier, 0.0);
    }

    #[test]
    fn v2_warfare_sections_parse_with_tier_knob_defaults() {
        // Shape written by config v2: every section present, none of the
        // per-tier knobs or the Archery damage knobs added in v3.
        let config: WarfareConfig = ron::from_str(
            "(blades:(damage_bonus_per_level:0.004,damage_bonus_cap:0.5,riposte_enabled:true,riposte_window_ticks:60,riposte_cooldown_ticks:200,riposte_bonus_multiplier:0.25),\
             axes:(damage_bonus_per_level:0.005,damage_bonus_cap:0.6),\
             archery:(hit_xp:6),\
             unarmed:(damage_bonus_per_level:0.003,damage_bonus_cap:0.4,knockback_bonus_per_level:0.004,knockback_cap:0.5),\
             defense:(xp_per_damage:2.0,xp_cap_per_hit:40,reduction_per_level:0.0015,reduction_cap:0.15),\
             acrobatics:(xp_per_fall_damage:3.0,xp_cap_per_fall:60,roll_reduction_per_level:0.002,roll_reduction_cap:0.25),\
             sorcery:(staff_item:\"blaze_rod\",mana_max:100.0,mana_regen_per_tick:0.05,spell_mana_cost:25.0,spell_heal:4.0,spell_cooldown_ticks:100,spell_xp:15))",
        )
        .unwrap();
        // Adopted v2 values survive.
        assert_eq!(config.archery.hit_xp, 6);
        assert_eq!(config.blades.riposte_cooldown_ticks, 200);
        // The new knobs land on their defaults.
        assert_eq!(config.blades.riposte_multiplier_per_tier, 0.05);
        assert_eq!(config.blades.riposte_cooldown_reduction_per_tier, 20);
        assert_eq!(config.axes.damage_cap_per_tier, 0.05);
        assert!(config.archery.damage_enabled);
        assert_eq!(config.archery.damage_bonus_per_level, 0.004);
        assert_eq!(config.archery.damage_bonus_cap, 0.5);
        assert_eq!(config.archery.damage_cap_per_tier, 0.05);
        assert_eq!(config.unarmed.knockback_cap_per_tier, 0.05);
        assert_eq!(config.acrobatics.roll_reduction_cap_per_tier, 0.05);
        assert_eq!(config.defense.reduction_cap_per_tier, 0.025);
        assert_eq!(config.sorcery.spell_heal_per_tier, 1.0);
        assert_eq!(config.sorcery.mana_max_per_tier, 10.0);
        assert_eq!(config.sorcery.spell_cooldown_reduction_per_tier, 10);
    }
}
