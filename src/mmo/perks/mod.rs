//! Perk eligibility, cooldowns, effects, and activation guards.
//!
//! Every perk is bounded by the global caps in `config::PerkConfig` and by
//! its own per-skill configuration. Shared scaffolding lives here; per-skill
//! perk logic lives in the branch modules and lands with each phase.

mod cooldown;

pub(crate) use cooldown::CooldownTracker;
