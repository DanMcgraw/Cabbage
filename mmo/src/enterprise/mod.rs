//! Enterprise branch skill handlers and configuration.
//!
//! Enterprise skills: Smithing, Maintenance, Alchemy, Enchanting, Tinkering,
//! Commerce. The repair and salvage activities share the Maintenance track
//! since the six-skill consolidation. Vanilla-result modifiers ship first;
//! quality, Masterwork, runes, and economy are follow-on content. The
//! trading and charisma activity configs stay disabled until Pumpkin exposes
//! a villager-trade commit transaction (see the platform gaps in
//! `src/mmo/plan.md`); their configuration and the `rep_v1` reputation
//! ledger exist already, so Commerce currently has no live XP source.

pub(crate) mod alchemy;
pub(crate) mod config;
pub(crate) mod enchanting;
pub(crate) mod repair;
pub(crate) mod salvage;
pub(crate) mod smithing;
pub(crate) mod tinkering;
