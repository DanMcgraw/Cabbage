//! Frontier branch skill handlers and configuration.
//!
//! Frontier skills: Cultivation, Woodcutting, Mining, Excavation, Fishing,
//! AnimalHandling. The agriculture/herbalism activities share the Cultivation
//! track and husbandry/taming share AnimalHandling since the six-skill
//! consolidation. Handlers land per the phased plan in `src/mmo/plan.md`;
//! only skills with a written XP attribution rule get one.

pub(crate) mod agriculture;
pub(crate) mod config;
pub(crate) mod excavation;
pub(crate) mod fishing;
pub(crate) mod herbalism;
pub(crate) mod husbandry;
pub(crate) mod mining;
pub(crate) mod taming;
pub(crate) mod woodcutting;
