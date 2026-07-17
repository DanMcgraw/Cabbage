//! Enterprise branch skill handlers and configuration.
//!
//! Enterprise skills: Smithing, Repair, Salvage, Alchemy, Enchanting,
//! Tinkering, Trading, Charisma. Vanilla-result modifiers ship first;
//! quality, Masterwork, runes, and economy are follow-on content. Trading
//! and Charisma stay disabled until Pumpkin exposes a villager-trade commit
//! transaction (see the platform gaps in `src/mmo/plan.md`); their
//! configuration and the `rep_v1` reputation ledger exist already.

pub(crate) mod alchemy;
pub(crate) mod config;
pub(crate) mod enchanting;
pub(crate) mod repair;
pub(crate) mod salvage;
pub(crate) mod smithing;
pub(crate) mod tinkering;
