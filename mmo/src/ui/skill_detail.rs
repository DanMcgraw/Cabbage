//! Per-skill detail pages (`/mmo skill <skill>`) and their data-only
//! progression catalog.
//!
//! The catalog maps every canonical skill to an XP-source description, the
//! live perk effects to describe, and the shared planned milestone slots
//! from `perks::eligibility`. Configuration-dependent text and values are
//! resolved while rendering, so `/mmo reload` takes effect immediately. A
//! disabled global perk switch or individual feature switch produces an
//! explicit `Disabled` row rather than hiding it.
//!
//! The catalog is display-only: gameplay handlers remain the authority on
//! whether an effect can fire. Milestone slots stay visibly `Planned` until
//! their concrete perk is implemented — the page never promises that a
//! level-25/50/75/100 placeholder is already active. Threshold states
//! (`Unlocked`/`Next`/`Locked`) land together with the first implemented
//! milestone perk; today every row is `Active from level 1`, `Disabled`, or
//! `Planned`.

use pumpkin_util::text::{TextComponent, click::ClickEvent, color::NamedColor};

use super::{
    super::{
        config::{LevelCurve, MmoConfig},
        perks::eligibility::{CAPSTONE_LEVEL, MAJOR_PERK_LEVELS},
        progression::{PlayerSkillSnapshot, PlayerSnapshot},
        skills::SkillId,
    },
    chat::{MAX_CHAT_LINES, branch_color},
    progress_percent, skill_enabled,
};

/// One live perk effect described on a skill's detail page. Every variant is
/// active from level 1 (level-scaled effects show their live value at the
/// viewer's level); threshold-gated perks are the planned milestone slots,
/// not live effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LiveEffect {
    Prospector,
    VeinMiner,
    Heartwood,
    Timber,
    HarvestBonus,
    QualityYield,
    ConsumableHealing,
    Earthmover,
    ArchaeologyLoot,
    Reel,
    TreasureReplacement,
    TraitRolls,
    BladesDamage,
    Riposte,
    AxesDamage,
    UnarmedDamage,
    Knockback,
    Roll,
    Resilience,
    HealingBolt,
    AnvilMarking,
    RepairDiscount,
    SalvageBonus,
    MaterialRecovery,
    OfferDiscount,
}

impl LiveEffect {
    /// Every live effect, for catalog coverage tests.
    #[cfg(test)]
    pub(crate) const ALL: &[LiveEffect] = &[
        LiveEffect::Prospector,
        LiveEffect::VeinMiner,
        LiveEffect::Heartwood,
        LiveEffect::Timber,
        LiveEffect::HarvestBonus,
        LiveEffect::QualityYield,
        LiveEffect::ConsumableHealing,
        LiveEffect::Earthmover,
        LiveEffect::ArchaeologyLoot,
        LiveEffect::Reel,
        LiveEffect::TreasureReplacement,
        LiveEffect::TraitRolls,
        LiveEffect::BladesDamage,
        LiveEffect::Riposte,
        LiveEffect::AxesDamage,
        LiveEffect::UnarmedDamage,
        LiveEffect::Knockback,
        LiveEffect::Roll,
        LiveEffect::Resilience,
        LiveEffect::HealingBolt,
        LiveEffect::AnvilMarking,
        LiveEffect::RepairDiscount,
        LiveEffect::SalvageBonus,
        LiveEffect::MaterialRecovery,
        LiveEffect::OfferDiscount,
    ];

    /// Concise unlock name shown in the progression row.
    fn name(self) -> &'static str {
        match self {
            LiveEffect::Prospector => "Prospector",
            LiveEffect::VeinMiner => "Vein Miner",
            LiveEffect::Heartwood => "Heartwood",
            LiveEffect::Timber => "Timber",
            LiveEffect::HarvestBonus => "Harvest bonus",
            LiveEffect::QualityYield => "Quality yield",
            LiveEffect::ConsumableHealing => "Consumable healing",
            LiveEffect::Earthmover => "Earthmover",
            LiveEffect::ArchaeologyLoot => "Archaeology loot",
            LiveEffect::Reel => "Reel",
            LiveEffect::TreasureReplacement => "Treasure replacement",
            LiveEffect::TraitRolls => "Newborn traits",
            LiveEffect::BladesDamage => "Sword damage",
            LiveEffect::Riposte => "Riposte",
            LiveEffect::AxesDamage => "Axe damage",
            LiveEffect::UnarmedDamage => "Unarmed damage",
            LiveEffect::Knockback => "Knockback",
            LiveEffect::Roll => "Roll",
            LiveEffect::Resilience => "Resilience",
            LiveEffect::HealingBolt => "Healing bolt",
            LiveEffect::AnvilMarking => "Anvil marking",
            LiveEffect::RepairDiscount => "Repair discount",
            LiveEffect::SalvageBonus => "Salvage bonus",
            LiveEffect::MaterialRecovery => "Material recovery",
            LiveEffect::OfferDiscount => "Offer discount",
        }
    }

    /// Resolve this effect against the live config at the viewer's level.
    /// The formulas mirror the gameplay handlers so the page reports the
    /// same value the perk would apply; the handlers stay the authority on
    /// activation.
    fn render(self, config: &MmoConfig, level: u32) -> EffectRender {
        let global_proc_cap = config.perks.max_proc_chance;
        let batch_cap = config.perks.batch_break_max_blocks;
        match self {
            LiveEffect::Prospector => {
                let perk = &config.frontier.mining;
                let cap = perk.prospector_max_chance.min(global_proc_cap);
                let chance = (perk.prospector_base_chance
                    + perk.prospector_chance_per_level * f64::from(level))
                .min(cap);
                EffectRender {
                    switch_on: perk.prospector_enabled,
                    detail: format!(
                        "{} chance at your level for one bonus ore drop (cap {})",
                        percent(chance),
                        percent(cap)
                    ),
                    off_reason: "prospector_enabled is off in config",
                }
            }
            LiveEffect::VeinMiner => {
                let perk = &config.frontier.mining;
                let blocks = perk.vein_miner_max_blocks.min(batch_cap);
                EffectRender {
                    switch_on: perk.vein_miner_enabled,
                    detail: format!(
                        "sneak + break an ore to break the connected vein (up to {blocks} extra blocks)"
                    ),
                    off_reason: "vein_miner_enabled is off in config",
                }
            }
            LiveEffect::Heartwood => {
                let woodcutting = &config.frontier.woodcutting;
                let chance = woodcutting.heartwood_chance.min(global_proc_cap);
                EffectRender {
                    switch_on: chance > 0.0,
                    detail: format!(
                        "{} chance for one bonus log and +{} XP on natural log breaks",
                        percent(chance),
                        woodcutting.heartwood_xp_bonus
                    ),
                    off_reason: "heartwood_chance is 0 in config",
                }
            }
            LiveEffect::Timber => {
                let woodcutting = &config.frontier.woodcutting;
                let blocks = woodcutting.timber_max_blocks.min(batch_cap);
                EffectRender {
                    switch_on: woodcutting.timber_enabled,
                    detail: format!(
                        "sneak + break a natural log to fell connected logs of the same type (up to {blocks} extra blocks)"
                    ),
                    off_reason: "timber_enabled is off in config",
                }
            }
            LiveEffect::HarvestBonus => {
                let agriculture = &config.frontier.agriculture;
                let chance = agriculture.harvest_bonus_chance.min(global_proc_cap);
                let detail = if agriculture.fertilizer_guarantees_bonus {
                    format!(
                        "{} chance for one bonus crop item; fertilized crops guarantee it and give +{} XP",
                        percent(chance),
                        agriculture.fertilizer_bonus_xp
                    )
                } else {
                    format!(
                        "{} chance for one bonus crop item; fertilized crops give +{} XP",
                        percent(chance),
                        agriculture.fertilizer_bonus_xp
                    )
                };
                EffectRender {
                    switch_on: chance > 0.0 || agriculture.fertilizer_guarantees_bonus,
                    detail,
                    off_reason: "harvest bonus and fertilizer guarantee are both off in config",
                }
            }
            LiveEffect::QualityYield => {
                let chance = config
                    .frontier
                    .herbalism
                    .quality_yield_chance
                    .min(global_proc_cap);
                EffectRender {
                    switch_on: chance > 0.0,
                    detail: format!(
                        "{} chance for one bonus item on natural plant breaks",
                        percent(chance)
                    ),
                    off_reason: "quality_yield_chance is 0 in config",
                }
            }
            LiveEffect::ConsumableHealing => {
                let bonus = config.frontier.herbalism.consumable_heal_bonus;
                EffectRender {
                    switch_on: bonus > 0.0,
                    detail: format!(
                        "configured plant foods restore +{} bonus health",
                        decimal(f64::from(bonus))
                    ),
                    off_reason: "consumable_heal_bonus is 0 in config",
                }
            }
            LiveEffect::Earthmover => {
                let excavation = &config.frontier.excavation;
                let blocks = excavation.earthmover_max_blocks.min(batch_cap);
                EffectRender {
                    switch_on: excavation.earthmover_enabled,
                    detail: format!(
                        "sneak + break a natural diggable block to excavate connected blocks of the same type (up to {blocks} extra blocks)"
                    ),
                    off_reason: "earthmover_enabled is off in config",
                }
            }
            LiveEffect::ArchaeologyLoot => {
                let excavation = &config.frontier.excavation;
                let live: Vec<f64> = excavation
                    .bonus_loot
                    .values()
                    .map(|loot| loot.chance.min(global_proc_cap))
                    .filter(|chance| *chance > 0.0)
                    .collect();
                let best = live.iter().copied().fold(0.0, f64::max);
                EffectRender {
                    switch_on: !live.is_empty(),
                    detail: format!(
                        "{} configured bonus finds on diggable breaks (up to {} each)",
                        live.len(),
                        percent(best)
                    ),
                    off_reason: "no bonus loot with a chance above 0 is configured",
                }
            }
            LiveEffect::Reel => {
                let bonus = config.frontier.fishing.reel_exp_bonus;
                EffectRender {
                    switch_on: bonus > 0,
                    detail: format!("+{bonus} vanilla experience on every catch"),
                    off_reason: "reel_exp_bonus is 0 in config",
                }
            }
            LiveEffect::TreasureReplacement => {
                let replacements = &config.frontier.fishing.treasure_replacements;
                EffectRender {
                    switch_on: !replacements.is_empty(),
                    detail: format!(
                        "swaps {} configured caught items for their replacements",
                        replacements.len()
                    ),
                    off_reason: "no treasure replacements are configured",
                }
            }
            LiveEffect::TraitRolls => {
                let husbandry = &config.frontier.husbandry;
                let chance = husbandry.trait_roll_chance.min(global_proc_cap);
                EffectRender {
                    switch_on: chance > 0.0 && !husbandry.traits.is_empty(),
                    detail: format!(
                        "{} chance for a newborn animal to gain one of {} configured traits",
                        percent(chance),
                        husbandry.traits.len()
                    ),
                    off_reason: if husbandry.traits.is_empty() {
                        "no traits are configured"
                    } else {
                        "trait_roll_chance is 0 in config"
                    },
                }
            }
            LiveEffect::BladesDamage => {
                let blades = &config.warfare.blades;
                let bonus =
                    (blades.damage_bonus_per_level * f64::from(level)).min(blades.damage_bonus_cap);
                EffectRender {
                    switch_on: blades.damage_bonus_per_level > 0.0 && blades.damage_bonus_cap > 0.0,
                    detail: format!(
                        "+{} damage at your level (cap {})",
                        percent(bonus),
                        percent(blades.damage_bonus_cap)
                    ),
                    off_reason: "damage bonus is 0 in config",
                }
            }
            LiveEffect::Riposte => {
                let blades = &config.warfare.blades;
                EffectRender {
                    switch_on: blades.riposte_enabled,
                    detail: format!(
                        "+{} damage within {} ticks after taking damage ({}-tick cooldown)",
                        percent(blades.riposte_bonus_multiplier),
                        blades.riposte_window_ticks,
                        blades.riposte_cooldown_ticks
                    ),
                    off_reason: "riposte_enabled is off in config",
                }
            }
            LiveEffect::AxesDamage => {
                let axes = &config.warfare.axes;
                let bonus =
                    (axes.damage_bonus_per_level * f64::from(level)).min(axes.damage_bonus_cap);
                EffectRender {
                    switch_on: axes.damage_bonus_per_level > 0.0 && axes.damage_bonus_cap > 0.0,
                    detail: format!(
                        "+{} damage at your level (cap {})",
                        percent(bonus),
                        percent(axes.damage_bonus_cap)
                    ),
                    off_reason: "damage bonus is 0 in config",
                }
            }
            LiveEffect::UnarmedDamage => {
                let unarmed = &config.warfare.unarmed;
                let bonus = (unarmed.damage_bonus_per_level * f64::from(level))
                    .min(unarmed.damage_bonus_cap);
                EffectRender {
                    switch_on: unarmed.damage_bonus_per_level > 0.0
                        && unarmed.damage_bonus_cap > 0.0,
                    detail: format!(
                        "+{} empty-hand damage at your level (cap {})",
                        percent(bonus),
                        percent(unarmed.damage_bonus_cap)
                    ),
                    off_reason: "damage bonus is 0 in config",
                }
            }
            LiveEffect::Knockback => {
                let unarmed = &config.warfare.unarmed;
                let bonus = (unarmed.knockback_bonus_per_level * f64::from(level))
                    .min(unarmed.knockback_cap);
                EffectRender {
                    switch_on: unarmed.knockback_bonus_per_level > 0.0
                        && unarmed.knockback_cap > 0.0,
                    detail: format!(
                        "+{} knockback at your level (cap {})",
                        percent(bonus),
                        percent(unarmed.knockback_cap)
                    ),
                    off_reason: "knockback bonus is 0 in config",
                }
            }
            LiveEffect::Roll => {
                let acrobatics = &config.warfare.acrobatics;
                let reduction = (acrobatics.roll_reduction_per_level * f64::from(level))
                    .min(acrobatics.roll_reduction_cap);
                EffectRender {
                    switch_on: acrobatics.roll_reduction_per_level > 0.0
                        && acrobatics.roll_reduction_cap > 0.0,
                    detail: format!(
                        "reduces fall damage by {} at your level (cap {})",
                        percent(reduction),
                        percent(acrobatics.roll_reduction_cap)
                    ),
                    off_reason: "roll reduction is 0 in config",
                }
            }
            LiveEffect::Resilience => {
                let defense = &config.warfare.defense;
                let reduction =
                    (defense.reduction_per_level * f64::from(level)).min(defense.reduction_cap);
                EffectRender {
                    switch_on: defense.reduction_per_level > 0.0 && defense.reduction_cap > 0.0,
                    detail: format!(
                        "reduces incoming damage by {} at your level (cap {})",
                        percent(reduction),
                        percent(defense.reduction_cap)
                    ),
                    off_reason: "damage reduction is 0 in config",
                }
            }
            LiveEffect::HealingBolt => {
                let sorcery = &config.warfare.sorcery;
                EffectRender {
                    switch_on: sorcery.spell_heal > 0.0,
                    detail: format!(
                        "right-click a {} to restore {} health ({} mana, {}-tick cooldown)",
                        sorcery.staff_item,
                        decimal(f64::from(sorcery.spell_heal)),
                        decimal(sorcery.spell_mana_cost),
                        sorcery.spell_cooldown_ticks
                    ),
                    off_reason: "spell_heal is 0 in config",
                }
            }
            LiveEffect::AnvilMarking => {
                let smithing = &config.enterprise.smithing;
                EffectRender {
                    switch_on: smithing.mark_anvil_outputs,
                    detail: "anvil outputs carry durable creator/provenance markers".to_string(),
                    off_reason: "mark_anvil_outputs is off in config",
                }
            }
            LiveEffect::RepairDiscount => {
                let repair = &config.enterprise.repair;
                let discount =
                    (repair.discount_per_level * f64::from(level)).min(repair.discount_cap);
                EffectRender {
                    switch_on: repair.discount_per_level > 0.0 && repair.discount_cap > 0.0,
                    detail: format!(
                        "anvil level cost -{} at your level (cap {}, {}-tick cooldown)",
                        decimal(discount),
                        decimal(repair.discount_cap),
                        repair.cooldown_ticks
                    ),
                    off_reason: "repair discount is 0 in config",
                }
            }
            LiveEffect::SalvageBonus => {
                let salvage = &config.enterprise.salvage;
                let bonus =
                    (salvage.xp_bonus_per_level * f64::from(level)).min(salvage.xp_bonus_cap);
                EffectRender {
                    switch_on: salvage.xp_bonus_per_level > 0.0 && salvage.xp_bonus_cap > 0.0,
                    detail: format!(
                        "+{} grindstone experience at your level (cap {}, {}-tick cooldown)",
                        percent(bonus),
                        percent(salvage.xp_bonus_cap),
                        salvage.cooldown_ticks
                    ),
                    off_reason: "salvage experience bonus is 0 in config",
                }
            }
            LiveEffect::MaterialRecovery => {
                let salvage = &config.enterprise.salvage;
                let chance = salvage.recovery_chance.min(global_proc_cap);
                EffectRender {
                    switch_on: chance > 0.0 && !salvage.recovery_materials.is_empty(),
                    detail: format!(
                        "{} chance for one tool-tier material back on grindstone takes",
                        percent(chance)
                    ),
                    off_reason: if salvage.recovery_materials.is_empty() {
                        "no recovery materials are configured"
                    } else {
                        "recovery_chance is 0 in config"
                    },
                }
            }
            LiveEffect::OfferDiscount => {
                let enchanting = &config.enterprise.enchanting;
                let discount = (enchanting.offer_discount_per_level * f64::from(level))
                    .min(enchanting.offer_discount_cap);
                EffectRender {
                    switch_on: enchanting.offer_discount_per_level > 0.0
                        && enchanting.offer_discount_cap > 0.0,
                    detail: format!(
                        "enchanting offer level requirement -{} at your level (cap {})",
                        decimal(discount),
                        decimal(enchanting.offer_discount_cap)
                    ),
                    off_reason: "offer discount is 0 in config",
                }
            }
        }
    }
}

/// A live effect resolved against the live config: whether its own switches
/// and values allow it, the plain-language text with live values, and why it
/// is off when its own switch is the blocker.
pub(crate) struct EffectRender {
    pub switch_on: bool,
    pub detail: String,
    pub off_reason: &'static str,
}

/// Display-only catalog entry for one skill.
pub(crate) struct CatalogEntry {
    /// Plain-language XP-source description. Merged skills name both
    /// activity families.
    pub xp_sources: &'static str,
    /// Merged-track note for skills that share one XP pool across two
    /// retired activity families.
    pub merged_note: Option<&'static str>,
    /// Live effects to describe, in display order.
    pub effects: &'static [LiveEffect],
}

/// The single source of truth for what each skill's detail page explains.
/// Display-only: gameplay handlers stay the authority on perk activation.
pub(crate) fn catalog_entry(skill: SkillId) -> CatalogEntry {
    const MERGED_NOTE: &str = "Merged track: both activity families share one level.";
    match skill {
        SkillId::Cultivation => CatalogEntry {
            xp_sources: "agriculture (mature crop harvests) and herbalism (natural plant breaks, eating plant foods)",
            merged_note: Some(MERGED_NOTE),
            effects: &[
                LiveEffect::HarvestBonus,
                LiveEffect::QualityYield,
                LiveEffect::ConsumableHealing,
            ],
        },
        SkillId::Woodcutting => CatalogEntry {
            xp_sources: "breaking natural logs",
            merged_note: None,
            effects: &[LiveEffect::Heartwood, LiveEffect::Timber],
        },
        SkillId::Mining => CatalogEntry {
            xp_sources: "breaking natural ores",
            merged_note: None,
            effects: &[LiveEffect::Prospector, LiveEffect::VeinMiner],
        },
        SkillId::Excavation => CatalogEntry {
            xp_sources: "breaking natural diggable blocks (dirt, sand, gravel, clay)",
            merged_note: None,
            effects: &[LiveEffect::ArchaeologyLoot, LiveEffect::Earthmover],
        },
        SkillId::Fishing => CatalogEntry {
            xp_sources: "catching fish, junk, and treasure",
            merged_note: None,
            effects: &[LiveEffect::Reel, LiveEffect::TreasureReplacement],
        },
        SkillId::AnimalHandling => CatalogEntry {
            xp_sources: "husbandry (breeding, animal products) and taming (completed tames)",
            merged_note: Some(MERGED_NOTE),
            effects: &[LiveEffect::TraitRolls],
        },
        SkillId::Blades => CatalogEntry {
            xp_sources: "kills with swords (attributed by weapon snapshot)",
            merged_note: None,
            effects: &[LiveEffect::BladesDamage, LiveEffect::Riposte],
        },
        SkillId::Axes => CatalogEntry {
            xp_sources: "kills with axes",
            merged_note: None,
            effects: &[LiveEffect::AxesDamage],
        },
        SkillId::Archery => CatalogEntry {
            xp_sources: "bow and crossbow kills, plus small XP per projectile hit",
            merged_note: None,
            effects: &[],
        },
        SkillId::Athletics => CatalogEntry {
            xp_sources: "unarmed combat (empty-hand kills) and acrobatics (fall damage taken)",
            merged_note: Some(MERGED_NOTE),
            effects: &[
                LiveEffect::UnarmedDamage,
                LiveEffect::Knockback,
                LiveEffect::Roll,
            ],
        },
        SkillId::Defense => CatalogEntry {
            xp_sources: "taking damage (scaled by damage received)",
            merged_note: None,
            effects: &[LiveEffect::Resilience],
        },
        SkillId::Sorcery => CatalogEntry {
            xp_sources: "casting the healing bolt",
            merged_note: None,
            effects: &[LiveEffect::HealingBolt],
        },
        SkillId::Smithing => CatalogEntry {
            xp_sources: "crafting tools and armor, and extracting furnace results",
            merged_note: None,
            effects: &[LiveEffect::AnvilMarking],
        },
        SkillId::Maintenance => CatalogEntry {
            xp_sources: "repair (anvil completions) and salvage (grindstone completions)",
            merged_note: Some(MERGED_NOTE),
            effects: &[
                LiveEffect::RepairDiscount,
                LiveEffect::SalvageBonus,
                LiveEffect::MaterialRecovery,
            ],
        },
        SkillId::Alchemy => CatalogEntry {
            xp_sources: "consuming configured potions",
            merged_note: None,
            effects: &[],
        },
        SkillId::Enchanting => CatalogEntry {
            xp_sources: "completing enchants (scaled by the level cost)",
            merged_note: None,
            effects: &[LiveEffect::OfferDiscount],
        },
        SkillId::Tinkering => CatalogEntry {
            xp_sources: "crafting redstone mechanisms",
            merged_note: None,
            effects: &[],
        },
        SkillId::Commerce => CatalogEntry {
            xp_sources: "trading and charisma activities — no live XP source yet (both dormant)",
            merged_note: Some(MERGED_NOTE),
            effects: &[],
        },
    }
}

/// State of one progression row. Live effects are `Active from level 1`;
/// disabled module/skill/global/individual switches surface as `Disabled`;
/// milestone slots stay `Planned` until their perk is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    Active,
    Disabled,
    Planned,
}

impl RowState {
    fn label(self) -> &'static str {
        match self {
            RowState::Active => "Active from level 1",
            RowState::Disabled => "Disabled",
            RowState::Planned => "Planned",
        }
    }

    fn color(self) -> NamedColor {
        match self {
            RowState::Active => NamedColor::Green,
            RowState::Disabled => NamedColor::Red,
            RowState::Planned => NamedColor::Aqua,
        }
    }
}

/// One resolved progression row: unlock level, unlock name, state, and the
/// plain-language effect text.
struct ProgressionRow {
    level: u32,
    name: &'static str,
    state: RowState,
    detail: String,
}

/// The skill's progression rows in unlock-level order: live effects (level
/// 1) first, then the shared planned milestone slots built from the
/// `perks::eligibility` constants so the levels are never duplicated here.
fn progression_rows(skill: SkillId, config: &MmoConfig, level: u32) -> Vec<ProgressionRow> {
    let entry = catalog_entry(skill);
    let module_on = config.enabled;
    let skill_on = skill_enabled(config, skill);
    let perks_on = config.perks.enabled;

    let mut rows = Vec::with_capacity(entry.effects.len() + MAJOR_PERK_LEVELS.len() + 1);
    for effect in entry.effects {
        let render = effect.render(config, level);
        let (state, detail) = if !module_on {
            (RowState::Disabled, "the MMO module is disabled".to_string())
        } else if !skill_on {
            (RowState::Disabled, "this skill is disabled".to_string())
        } else if !perks_on {
            (
                RowState::Disabled,
                "the global perks switch is off".to_string(),
            )
        } else if !render.switch_on {
            (RowState::Disabled, render.off_reason.to_string())
        } else {
            (RowState::Active, render.detail)
        };
        rows.push(ProgressionRow {
            level: 1,
            name: effect.name(),
            state,
            detail,
        });
    }
    for milestone in MAJOR_PERK_LEVELS {
        rows.push(ProgressionRow {
            level: milestone,
            name: "Major perk slot",
            state: RowState::Planned,
            detail: "planned for a future update; nothing unlocks here yet".to_string(),
        });
    }
    rows.push(ProgressionRow {
        level: CAPSTONE_LEVEL,
        name: "Capstone slot",
        state: RowState::Planned,
        detail: "planned for a future update; nothing unlocks here yet".to_string(),
    });
    rows
}

/// Format a fraction as a percentage with enough precision to stay
/// meaningful for sub-1% values, trimming trailing zeros.
fn percent(fraction: f64) -> String {
    let value = fraction * 100.0;
    let decimals = if value >= 10.0 {
        0
    } else if value >= 1.0 {
        1
    } else {
        2
    };
    let formatted = format!("{value:.decimals$}");
    let trimmed = if formatted.contains('.') {
        formatted.trim_end_matches('0').trim_end_matches('.')
    } else {
        formatted.as_str()
    };
    format!("{trimmed}%")
}

/// Compact decimal (two fraction digits at most, trailing zeros trimmed) for
/// non-percentage values like level costs, health, and mana.
fn decimal(value: f64) -> String {
    let formatted = format!("{value:.2}");
    if formatted.contains('.') {
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        formatted
    }
}

/// Canonical suggested command for a skill's detail page. Shared by the
/// summary-grid cells, the navigation footer, and page links.
pub(crate) fn skill_command(skill: SkillId) -> String {
    format!("/mmo skill {}", skill.as_str().to_ascii_lowercase())
}

/// Suggested command for one page of a skill's detail.
fn skill_page_command(skill: SkillId, page: usize) -> String {
    format!("{} {page}", skill_command(skill))
}

/// Branch-coloured bold heading: `<Skill> — <Branch>`, plus a page marker
/// when the progression section spans multiple pages.
fn heading(skill: SkillId, page: usize, page_count: usize) -> TextComponent {
    let branch = skill.branch();
    let mut line = TextComponent::text(format!(
        "{} — {}",
        skill.display_name(),
        branch.display_name()
    ))
    .color_named(branch_color(branch))
    .bold();
    if page_count > 1 {
        line = line.add_child(
            TextComponent::text(format!(" (page {page}/{page_count})"))
                .color_named(NamedColor::DarkGray),
        );
    }
    line
}

/// Current state: level, total XP, and either progress toward the next
/// level or the max-level marker.
fn state_line(progress: PlayerSkillSnapshot, curve: &LevelCurve) -> TextComponent {
    let (level, into, needed) = curve.level_for_xp(progress.xp);
    let mut line = TextComponent::text(format!("Level {level}")).color_named(NamedColor::Yellow);
    line = line.add_child(
        TextComponent::text(format!(" · Total XP: {}", progress.xp)).color_named(NamedColor::Gray),
    );
    if level >= curve.max_level() {
        line = line.add_child(TextComponent::text(" · Max level").color_named(NamedColor::Gold));
    } else {
        line = line.add_child(
            TextComponent::text(format!(
                " · XP: {into}/{needed} ({}%)",
                progress_percent(into, needed)
            ))
            .color_named(NamedColor::Gray),
        );
    }
    line
}

/// XP-source description; merged skills name both activity families and
/// carry the merged-track note.
fn xp_source_line(entry: &CatalogEntry) -> TextComponent {
    let mut text = format!("XP from {}.", entry.xp_sources);
    if let Some(note) = entry.merged_note {
        text.push(' ');
        text.push_str(note);
    }
    TextComponent::text(text).color_named(NamedColor::Gray)
}

/// One progression row: `Lv <level> · <name> — <state>: <detail>`.
fn row_line(row: &ProgressionRow) -> TextComponent {
    TextComponent::text(format!("Lv {} · ", row.level))
        .color_named(NamedColor::DarkGray)
        .add_child(TextComponent::text(row.name))
        .add_child(TextComponent::text(" — ").color_named(NamedColor::DarkGray))
        .add_child(TextComponent::text(row.state.label()).color_named(row.state.color()))
        .add_child(TextComponent::text(format!(": {}", row.detail)).color_named(NamedColor::Gray))
}

/// Navigation footer: previous/next skill in canonical branch order
/// (wrapping inside the branch), the branch detail page, `/mmo`, and page
/// links when the progression section spans multiple pages.
fn nav_line(skill: SkillId, page: usize, page_count: usize) -> TextComponent {
    let branch = skill.branch();
    let skills = branch.skills();
    let index = skills
        .iter()
        .position(|member| *member == skill)
        .unwrap_or(0);
    let prev = skills[(index + skills.len() - 1) % skills.len()];
    let next = skills[(index + 1) % skills.len()];

    fn sep() -> TextComponent {
        TextComponent::text(" · ").color_named(NamedColor::DarkGray)
    }

    fn link(label: String, command: String, color: NamedColor) -> TextComponent {
        TextComponent::text(label)
            .color_named(color)
            .underlined()
            .click_event(ClickEvent::SuggestCommand {
                command: command.into(),
            })
    }

    let mut line = TextComponent::text("Skills: ").color_named(NamedColor::Gray);
    line = line.add_child(link(
        format!("« {}", prev.display_name()),
        skill_command(prev),
        NamedColor::Aqua,
    ));
    line = line.add_child(sep());
    line = line.add_child(link(
        format!("{} »", next.display_name()),
        skill_command(next),
        NamedColor::Aqua,
    ));
    line = line.add_child(sep());
    line = line.add_child(link(
        branch.display_name().to_string(),
        format!(
            "/mmo stats chat {}",
            branch.display_name().to_ascii_lowercase()
        ),
        branch_color(branch),
    ));
    line = line.add_child(sep());
    line = line.add_child(link(
        "/mmo".to_string(),
        "/mmo".to_string(),
        NamedColor::Aqua,
    ));
    if page > 1 {
        line = line.add_child(sep());
        line = line.add_child(link(
            format!("« page {}", page - 1),
            skill_page_command(skill, page - 1),
            NamedColor::Yellow,
        ));
    }
    if page < page_count {
        line = line.add_child(sep());
        line = line.add_child(link(
            format!("page {} »", page + 1),
            skill_page_command(skill, page + 1),
            NamedColor::Yellow,
        ));
    }
    line
}

/// Render the detail page for one skill against the caller's snapshot and
/// the live config. Pages are 1-based and clamp into range; page 1 always
/// fits the vanilla unfocused-chat budget of [`MAX_CHAT_LINES`] lines, and
/// the progression section paginates without omitting or duplicating rows.
pub(crate) fn skill_detail_lines(
    skill: SkillId,
    snapshot: &PlayerSnapshot,
    curve: &LevelCurve,
    config: &MmoConfig,
    page: usize,
) -> Vec<TextComponent> {
    let progress = snapshot.get(skill);
    let level = progress.level(curve);
    let rows = progression_rows(skill, config, level);

    // Page 1 fixed lines: heading, state, one disabled explanation per
    // disabled scope, XP sources, footer. Later pages carry only the
    // heading and footer, leaving more room for progression rows.
    let mut disabled_notes = Vec::new();
    if !config.enabled {
        disabled_notes
            .push("The MMO module is disabled: progress is kept, but XP and perks are off.");
    }
    if !skill_enabled(config, skill) {
        disabled_notes.push("This skill is disabled: progress is kept, but XP and perks are off.");
    }
    let first_capacity = MAX_CHAT_LINES - 4 - disabled_notes.len();
    let later_capacity = MAX_CHAT_LINES - 2;
    let page_count = if rows.len() <= first_capacity {
        1
    } else {
        1 + (rows.len() - first_capacity).div_ceil(later_capacity)
    };
    let page = page.clamp(1, page_count);
    let (start, end) = if page == 1 {
        (0, first_capacity.min(rows.len()))
    } else {
        let start = first_capacity + (page - 2) * later_capacity;
        (start, (start + later_capacity).min(rows.len()))
    };

    let mut lines = Vec::with_capacity(MAX_CHAT_LINES);
    lines.push(heading(skill, page, page_count));
    if page == 1 {
        lines.push(state_line(progress, curve));
        for note in disabled_notes {
            lines.push(TextComponent::text(note).color_named(NamedColor::Red));
        }
        lines.push(xp_source_line(&catalog_entry(skill)));
    }
    lines.extend(rows[start..end].iter().map(row_line));
    lines.push(nav_line(skill, page, page_count));
    debug_assert!(lines.len() <= MAX_CHAT_LINES);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::SkillConfig,
        progression::PlayerSkillSnapshot,
        skills::{BranchId, SkillId},
    };
    use pumpkin_util::text::{TextComponentBase, click::ClickEvent};
    use std::collections::HashMap;

    /// One-XP-per-level curve so tests can reach any level directly
    /// (xp = level - 1).
    fn tall_curve(max_level: u32) -> LevelCurve {
        LevelCurve::new(&SkillConfig {
            max_level,
            base_xp: 1,
            xp_multiplier: 1.0,
            enabled: true,
        })
    }

    fn snapshot_at_level(skill: SkillId, level: u32) -> PlayerSnapshot {
        let mut snapshot = PlayerSnapshot::default();
        snapshot.set(skill, PlayerSkillSnapshot::new(u64::from(level - 1)));
        snapshot
    }

    fn join_text(lines: &[TextComponent]) -> String {
        lines
            .iter()
            .map(|line| line.clone().get_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn children(line: &TextComponent) -> &[TextComponentBase] {
        &line.0.extra
    }

    /// Every `SuggestCommand` click event anywhere in the line's children.
    fn suggested_commands(line: &TextComponent) -> Vec<String> {
        children(line)
            .iter()
            .filter_map(|child| match &child.style.click_event {
                Some(ClickEvent::SuggestCommand { command }) => Some(command.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn catalog_covers_every_skill_and_every_effect_has_exactly_one_owner() {
        let mut owners = HashMap::new();
        for skill in SkillId::ALL {
            let entry = catalog_entry(*skill);
            assert!(!entry.xp_sources.is_empty(), "{skill} has no XP sources");
            for effect in entry.effects {
                assert!(
                    owners.insert(*effect, *skill).is_none(),
                    "{effect:?} is listed under more than one skill"
                );
            }
        }
        // No orphaned effects: every variant is owned by exactly one skill.
        assert_eq!(owners.len(), LiveEffect::ALL.len());
        for effect in LiveEffect::ALL {
            assert!(owners.contains_key(effect), "{effect:?} is orphaned");
        }
    }

    #[test]
    fn merged_notes_mark_exactly_the_five_merged_skills() {
        for skill in SkillId::ALL {
            let merged = matches!(
                skill,
                SkillId::Cultivation
                    | SkillId::AnimalHandling
                    | SkillId::Athletics
                    | SkillId::Maintenance
                    | SkillId::Commerce
            );
            assert_eq!(
                catalog_entry(*skill).merged_note.is_some(),
                merged,
                "{skill} merged note"
            );
        }
    }

    #[test]
    fn milestone_levels_come_from_the_eligibility_constants_in_order() {
        let config = MmoConfig::default();
        let rows = progression_rows(SkillId::Mining, &config, 1);
        let milestone_levels: Vec<u32> = rows
            .iter()
            .filter(|row| row.state == RowState::Planned)
            .map(|row| row.level)
            .collect();
        let mut expected = MAJOR_PERK_LEVELS.to_vec();
        expected.push(CAPSTONE_LEVEL);
        assert_eq!(milestone_levels, expected);

        // The whole progression section is ordered by unlock level.
        let levels: Vec<u32> = rows.iter().map(|row| row.level).collect();
        let mut sorted = levels.clone();
        sorted.sort_unstable();
        assert_eq!(levels, sorted);
    }

    #[test]
    fn milestones_stay_planned_below_at_and_above_their_level() {
        let config = MmoConfig::default();
        let milestones = MAJOR_PERK_LEVELS.iter().copied().chain([CAPSTONE_LEVEL]);
        for milestone in milestones {
            for level in [milestone - 1, milestone, milestone + 1] {
                let rows = progression_rows(SkillId::Mining, &config, level);
                let row = rows
                    .iter()
                    .find(|row| row.level == milestone)
                    .expect("milestone row must exist");
                assert_eq!(
                    row.state,
                    RowState::Planned,
                    "milestone {milestone} at player level {level} must stay visibly planned"
                );
            }
        }
    }

    #[test]
    fn disabled_module_is_explained_and_labels_every_live_effect() {
        let mut config = MmoConfig::default();
        config.enabled = false;
        let curve = tall_curve(1000);
        let snapshot = snapshot_at_level(SkillId::Mining, 10);

        let rows = progression_rows(SkillId::Mining, &config, 10);
        for row in rows.iter().filter(|row| row.level == 1) {
            assert_eq!(
                row.state,
                RowState::Disabled,
                "{} must be Disabled",
                row.name
            );
            assert!(row.detail.contains("module"), "{}", row.detail);
        }
        // Milestones stay Planned; live rows are labelled, never omitted.
        assert_eq!(
            rows.iter()
                .filter(|row| row.state == RowState::Planned)
                .count(),
            4
        );

        let text = join_text(&skill_detail_lines(
            SkillId::Mining,
            &snapshot,
            &curve,
            &config,
            1,
        ));
        assert!(text.contains("The MMO module is disabled"), "{text}");
        // Stored level and XP stay visible.
        assert!(text.contains("Level 10"), "{text}");
        assert!(text.contains("Total XP: 9"), "{text}");
    }

    #[test]
    fn disabled_skill_is_explained_and_labels_its_live_effects() {
        let mut config = MmoConfig::default();
        config.skills.get_mut(&SkillId::Mining).unwrap().enabled = false;
        let curve = tall_curve(1000);
        let snapshot = snapshot_at_level(SkillId::Mining, 10);

        let rows = progression_rows(SkillId::Mining, &config, 10);
        for row in rows.iter().filter(|row| row.level == 1) {
            assert_eq!(
                row.state,
                RowState::Disabled,
                "{} must be Disabled",
                row.name
            );
        }
        let text = join_text(&skill_detail_lines(
            SkillId::Mining,
            &snapshot,
            &curve,
            &config,
            1,
        ));
        assert!(text.contains("This skill is disabled"), "{text}");
        assert!(text.contains("Level 10"), "{text}");
    }

    #[test]
    fn disabled_global_perks_switch_labels_rows_without_hiding_them() {
        let mut config = MmoConfig::default();
        config.perks.enabled = false;
        let rows = progression_rows(SkillId::Mining, &config, 10);
        let names: Vec<&str> = rows.iter().map(|row| row.name).collect();
        assert!(names.contains(&"Prospector"));
        assert!(names.contains(&"Vein Miner"));
        for row in rows.iter().filter(|row| row.level == 1) {
            assert_eq!(
                row.state,
                RowState::Disabled,
                "{} must be Disabled",
                row.name
            );
            assert!(row.detail.contains("global perks switch"), "{}", row.detail);
        }
    }

    #[test]
    fn disabled_individual_switch_labels_only_that_row() {
        let mut config = MmoConfig::default();
        config.frontier.mining.prospector_enabled = false;
        let rows = progression_rows(SkillId::Mining, &config, 10);
        let prospector = rows
            .iter()
            .find(|row| row.name == "Prospector")
            .expect("Prospector row must be present");
        assert_eq!(prospector.state, RowState::Disabled);
        assert!(prospector.detail.contains("prospector_enabled"));
        let vein_miner = rows
            .iter()
            .find(|row| row.name == "Vein Miner")
            .expect("Vein Miner row must be present");
        assert_eq!(vein_miner.state, RowState::Active);
    }

    #[test]
    fn merged_skills_name_both_activity_families() {
        let expected = [
            (SkillId::Cultivation, ["agriculture", "herbalism"]),
            (SkillId::AnimalHandling, ["husbandry", "taming"]),
            (SkillId::Athletics, ["unarmed", "acrobatics"]),
            (SkillId::Maintenance, ["repair", "salvage"]),
            (SkillId::Commerce, ["trading", "charisma"]),
        ];
        for (skill, families) in expected {
            let entry = catalog_entry(skill);
            for family in families {
                assert!(
                    entry.xp_sources.contains(family),
                    "{skill} XP sources must name {family}: {}",
                    entry.xp_sources
                );
            }
            assert!(entry.merged_note.is_some(), "{skill} merged note");
        }
    }

    #[test]
    fn legacy_aliases_resolve_to_the_canonical_detail_page() {
        let config = MmoConfig::default();
        let curve = tall_curve(1000);
        let snapshot = PlayerSnapshot::default();
        let expected = [
            ("agriculture", SkillId::Cultivation),
            ("herbalism", SkillId::Cultivation),
            ("husbandry", SkillId::AnimalHandling),
            ("taming", SkillId::AnimalHandling),
            ("unarmed", SkillId::Athletics),
            ("acrobatics", SkillId::Athletics),
            ("repair", SkillId::Maintenance),
            ("salvage", SkillId::Maintenance),
            ("trading", SkillId::Commerce),
            ("charisma", SkillId::Commerce),
        ];
        for (alias, skill) in expected {
            let resolved = SkillId::from_name(alias).expect(alias);
            assert_eq!(resolved, skill, "{alias}");
            let heading = skill_detail_lines(resolved, &snapshot, &curve, &config, 1)[0]
                .clone()
                .get_text();
            assert!(
                heading.contains(skill.display_name()),
                "{alias} must open the {skill} page: {heading}"
            );
        }
    }

    #[test]
    fn commerce_has_no_live_effects_but_keeps_its_planned_milestones() {
        assert!(catalog_entry(SkillId::Commerce).effects.is_empty());
        let config = MmoConfig::default();
        let rows = progression_rows(SkillId::Commerce, &config, 1);
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.state == RowState::Planned));
    }

    #[test]
    fn live_values_resolve_from_the_config_at_render_time() {
        let curve = tall_curve(1000);
        let snapshot = snapshot_at_level(SkillId::Mining, 50);

        // Default: 5% + 0.2% * 50 = 15% at level 50.
        let config = MmoConfig::default();
        let rows = progression_rows(SkillId::Mining, &config, 50);
        assert!(rows[0].detail.contains("15%"), "{}", rows[0].detail);

        // A "reload" with a new base chance changes the page immediately:
        // 10% + 0.2% * 50 = 20%.
        let mut config = config;
        config.frontier.mining.prospector_base_chance = 0.10;
        let text = join_text(&skill_detail_lines(
            SkillId::Mining,
            &snapshot,
            &curve,
            &config,
            1,
        ));
        assert!(text.contains("20%"), "{text}");
    }

    #[test]
    fn batch_break_rows_show_the_effective_global_cap() {
        let mut config = MmoConfig::default();
        config.perks.batch_break_max_blocks = 8;
        let rows = progression_rows(SkillId::Woodcutting, &config, 1);
        let timber = rows
            .iter()
            .find(|row| row.name == "Timber")
            .expect("Timber row");
        // min(timber_max_blocks 32, global cap 8) wins.
        assert!(
            timber.detail.contains("8 extra blocks"),
            "{}",
            timber.detail
        );
    }

    #[test]
    fn pages_stay_within_budget_and_paginate_without_gaps_or_duplicates() {
        let curve = tall_curve(1000);
        let config_variants = [
            MmoConfig::default(),
            MmoConfig {
                enabled: false,
                ..MmoConfig::default()
            },
            MmoConfig {
                perks: crate::config::PerkConfig {
                    enabled: false,
                    ..crate::config::PerkConfig::default()
                },
                ..MmoConfig::default()
            },
        ];
        for skill in SkillId::ALL {
            for config in &config_variants {
                let snapshot = snapshot_at_level(*skill, 60);
                let expected: Vec<String> = progression_rows(*skill, config, 60)
                    .iter()
                    .map(|row| row_line(row).get_text())
                    .collect();
                let mut collected = Vec::new();
                for page in 1..=8 {
                    let lines = skill_detail_lines(*skill, &snapshot, &curve, config, page);
                    assert!(
                        lines.len() <= MAX_CHAT_LINES,
                        "{skill} page {page} has {} lines",
                        lines.len()
                    );
                    for line in &lines {
                        let text = line.clone().get_text();
                        if text.starts_with("Lv ") {
                            collected.push(text);
                        }
                    }
                    if collected.len() >= expected.len() {
                        break;
                    }
                }
                assert_eq!(collected, expected, "{skill} pagination drifted");
            }
        }
    }

    #[test]
    fn default_config_single_page_layout_and_exact_line_counts() {
        let config = MmoConfig::default();
        let curve = tall_curve(1000);

        // Mining: 2 effects + 4 milestones = 6 rows — page 1 is exactly the
        // 10-line budget: heading, state, sources, 6 rows, footer.
        let snapshot = snapshot_at_level(SkillId::Mining, 60);
        let lines = skill_detail_lines(SkillId::Mining, &snapshot, &curve, &config, 1);
        assert_eq!(lines.len(), MAX_CHAT_LINES);
        let heading = lines[0].clone().get_text();
        assert_eq!(heading, "Mining — Frontier");
        let state = lines[1].clone().get_text();
        assert!(state.contains("Level 60"), "{state}");
        assert!(state.contains("Total XP: 59"), "{state}");
        assert!(state.contains("XP: 0/1 (0%)"), "{state}");
        assert!(lines[2].clone().get_text().starts_with("XP from "));

        // Commerce: no live effects — 4 milestone rows, 8 lines, one page.
        let snapshot = snapshot_at_level(SkillId::Commerce, 1);
        let lines = skill_detail_lines(SkillId::Commerce, &snapshot, &curve, &config, 1);
        assert_eq!(lines.len(), 8);
        let text = join_text(&lines);
        assert!(text.contains("no live XP source yet"), "{text}");
        assert!(!text.contains("Active from level 1"), "{text}");
    }

    #[test]
    fn skills_with_seven_rows_need_a_second_page() {
        let config = MmoConfig::default();
        let curve = tall_curve(1000);
        for skill in [
            SkillId::Cultivation,
            SkillId::Athletics,
            SkillId::Maintenance,
        ] {
            let snapshot = snapshot_at_level(skill, 60);
            let first = skill_detail_lines(skill, &snapshot, &curve, &config, 1);
            assert_eq!(first.len(), MAX_CHAT_LINES, "{skill} page 1");
            assert_eq!(
                first[0].clone().get_text(),
                format!(
                    "{} — {} (page 1/2)",
                    skill.display_name(),
                    skill.branch().display_name()
                )
            );
            let second = skill_detail_lines(skill, &snapshot, &curve, &config, 2);
            assert_eq!(second.len(), 3, "{skill} page 2: heading + 1 row + footer");
            assert!(
                second[0].clone().get_text().contains("(page 2/2)"),
                "{skill} page 2 heading"
            );
            // Page links exist in both directions.
            let first_footer = suggested_commands(first.last().unwrap());
            assert!(
                first_footer.contains(&skill_page_command(skill, 2)),
                "{skill} page 1 footer: {first_footer:?}"
            );
            let second_footer = suggested_commands(second.last().unwrap());
            assert!(
                second_footer.contains(&skill_page_command(skill, 1)),
                "{skill} page 2 footer: {second_footer:?}"
            );
            // Out-of-range pages clamp instead of duplicating or dropping rows.
            assert_eq!(
                join_text(&skill_detail_lines(skill, &snapshot, &curve, &config, 0)),
                join_text(&first)
            );
            assert_eq!(
                join_text(&skill_detail_lines(skill, &snapshot, &curve, &config, 99)),
                join_text(&second)
            );
        }
    }

    #[test]
    fn footer_links_prev_next_branch_and_summary_in_canonical_order() {
        let config = MmoConfig::default();
        let curve = tall_curve(1000);

        // Mining sits between Woodcutting and Excavation in Frontier order.
        let snapshot = snapshot_at_level(SkillId::Mining, 60);
        let lines = skill_detail_lines(SkillId::Mining, &snapshot, &curve, &config, 1);
        let footer = lines.last().unwrap();
        let text = footer.clone().get_text();
        assert!(text.contains("« Woodcutting"), "{text}");
        assert!(text.contains("Excavation »"), "{text}");
        assert!(text.contains("Frontier"), "{text}");
        let commands = suggested_commands(footer);
        for expected in [
            "/mmo skill woodcutting",
            "/mmo skill excavation",
            "/mmo stats chat frontier",
            "/mmo",
        ] {
            assert!(
                commands.iter().any(|command| command == expected),
                "footer must suggest {expected}: {commands:?}"
            );
        }

        // Prev/next wrap inside the branch: Cultivation's previous skill is
        // AnimalHandling, and every footer uses canonical lowercase names.
        for skill in SkillId::ALL {
            let snapshot = snapshot_at_level(*skill, 1);
            let lines = skill_detail_lines(*skill, &snapshot, &curve, &config, 1);
            let commands = suggested_commands(lines.last().unwrap());
            let branch_skills = skill.branch().skills();
            let index = branch_skills
                .iter()
                .position(|member| member == skill)
                .unwrap();
            let prev = branch_skills[(index + branch_skills.len() - 1) % branch_skills.len()];
            let next = branch_skills[(index + 1) % branch_skills.len()];
            assert!(
                commands.contains(&skill_command(prev)),
                "{skill} footer must link previous skill {prev}"
            );
            assert!(
                commands.contains(&skill_command(next)),
                "{skill} footer must link next skill {next}"
            );
            let branch_name = skill.branch().display_name().to_ascii_lowercase();
            assert!(
                commands.contains(&format!("/mmo stats chat {branch_name}")),
                "{skill} footer must link the branch page"
            );
        }
        // Cultivation (first Frontier skill) wraps to AnimalHandling.
        let snapshot = snapshot_at_level(SkillId::Cultivation, 1);
        let lines = skill_detail_lines(SkillId::Cultivation, &snapshot, &curve, &config, 1);
        assert!(
            suggested_commands(lines.last().unwrap())
                .contains(&"/mmo skill animalhandling".to_string())
        );
        // Branch assertions keep BranchId import used even if layouts shift.
        assert_eq!(SkillId::Cultivation.branch(), BranchId::Frontier);
    }

    #[test]
    fn percent_and_decimal_formatting_is_compact() {
        assert_eq!(percent(0.052), "5.2%");
        assert_eq!(percent(0.15), "15%");
        assert_eq!(percent(0.35), "35%");
        assert_eq!(percent(0.5), "50%");
        assert_eq!(percent(0.004), "0.4%");
        assert_eq!(percent(0.0015), "0.15%");
        assert_eq!(decimal(10.0), "10");
        assert_eq!(decimal(0.05), "0.05");
        assert_eq!(decimal(0.15000000000000002), "0.15");
        assert_eq!(decimal(4.0), "4");
    }
}
