//! Frontier branch skill handlers and configuration.
//!
//! Frontier skills: Agriculture, Herbalism, Woodcutting, Mining, Excavation,
//! Fishing, Husbandry, Taming. Handlers land per the phased plan in
//! `src/mmo/plan.md`; only skills with a written XP attribution rule get one.

pub(crate) mod agriculture;
pub(crate) mod config;
pub(crate) mod fishing;
pub(crate) mod mining;
pub(crate) mod woodcutting;
