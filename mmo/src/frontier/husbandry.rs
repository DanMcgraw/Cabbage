//! Husbandry activity: breeding XP and animal-product XP.
//!
//! XP rules: one award per bred animal (attributed to the breeder from the
//! event) and per collected animal product (right tool on the right animal).
//! Since the six-skill consolidation the awards feed AnimalHandling, shared
//! with the taming activity. A bounded, configurable trait roll writes
//! directly to the baby exposed by `EntityBreedCompleteEvent`.

use pumpkin::plugin::api::events::entity::{
    entity_breed::EntityBreedCompleteEvent, entity_product::AnimalProductCollectCompleteEvent,
};
use rand::RngExt;

use super::super::{
    MmoState,
    perks::eligibility::perk_tier,
    persistence::PetDataV1,
    progression::{self, XpSource, earns_xp, perk_level},
    skills::SkillId,
};

/// The shared skill track this activity awards: husbandry and taming both
/// feed AnimalHandling since the six-skill consolidation.
pub(crate) const SKILL: SkillId = SkillId::AnimalHandling;

/// Newborn trait roll chance: the base chance plus the per-tier step,
/// clamped by the global proc-chance cap.
fn trait_roll_chance(
    husbandry: &super::config::HusbandryConfig,
    level: u32,
    global_cap: f64,
) -> f64 {
    (husbandry.trait_roll_chance
        + husbandry.trait_roll_chance_per_tier * f64::from(perk_tier(level)))
    .min(global_cap)
}

/// How many traits a successful newborn roll grants: two at or above the
/// configured double-trait tier when at least two traits are configured
/// (the second pick is always distinct), one otherwise.
fn trait_roll_count(husbandry: &super::config::HusbandryConfig, level: u32) -> usize {
    if husbandry.traits.len() >= 2 && perk_tier(level) >= husbandry.double_trait_min_tier {
        2
    } else {
        1
    }
}

/// Map a draw from `0..len-1` to an index in `0..len` that differs from
/// `first`: the draw picks from the list with the first index removed, so
/// the second trait is always distinct without rerolling.
fn second_distinct_index(first: usize, draw: usize) -> usize {
    if draw >= first { draw + 1 } else { draw }
}

/// Award AnimalHandling XP to the breeder when two animals produce a baby.
pub async fn handle_entity_breed_complete(state: &MmoState, event: &EntityBreedCompleteEvent) {
    let Some(breeder) = event.breeder.as_ref() else {
        return;
    };
    if !earns_xp(breeder) {
        return;
    }
    let config = state.config();
    let husbandry = &config.frontier.husbandry;
    let xp = husbandry
        .breed_xp
        .get(event.baby.get_entity().entity_type.resource_name)
        .copied()
        .unwrap_or(husbandry.default_breed_xp);

    progression::award_xp(state, breeder, SKILL, xp, XpSource::Breed).await;

    if !config.perks.enabled || husbandry.traits.is_empty() {
        return;
    }
    let level = perk_level(state, breeder.gameprofile.id, SKILL).await;
    let chance = trait_roll_chance(husbandry, level, config.perks.max_proc_chance);
    if chance <= 0.0 || rand::rng().random::<f64>() >= chance {
        return;
    }
    let first_index = rand::rng().random_range(0..husbandry.traits.len());
    let mut selected = vec![husbandry.traits[first_index].clone()];
    if trait_roll_count(husbandry, level) == 2 {
        // Draw from the list with the first pick removed: the second trait
        // is always distinct.
        let draw = rand::rng().random_range(0..husbandry.traits.len() - 1);
        selected.push(husbandry.traits[second_distinct_index(first_index, draw)].clone());
    }
    let mut profile = PetDataV1::read(state.context(), event.baby.as_ref()).unwrap_or_default();
    let mut added: Vec<String> = Vec::new();
    for trait_name in selected {
        if !profile.traits.contains(&trait_name) {
            profile.traits.push(trait_name.clone());
            added.push(trait_name);
        }
    }
    if added.is_empty() {
        return;
    }
    if let Err(error) = profile.write(state.context(), event.baby.as_ref()) {
        log::warn!("[Cabbage MMO] failed to write newborn trait: {error}");
        return;
    }
    state.audit(&format!(
        "quality roll: newborn {} received trait {} for {}",
        event.baby.get_entity().entity_uuid,
        added.join(", "),
        breeder.gameprofile.id
    ));
}

/// Award AnimalHandling XP for collecting animal products (bucket on a cow,
/// shears on a sheep, bowl on a mooshroom).
pub async fn handle_product_complete(state: &MmoState, event: &AnimalProductCollectCompleteEvent) {
    let player = &event.player;
    if !earns_xp(player) {
        return;
    }
    let config = state.config();
    let husbandry = &config.frontier.husbandry;
    let entity_name = event.target.get_entity().entity_type.resource_name;
    let Some(product) = husbandry.product_xp.get(entity_name) else {
        return;
    };

    progression::award_xp(state, player, SKILL, product.xp, XpSource::AnimalProduct).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_roll_chance_scales_with_tier() {
        let husbandry = super::super::config::HusbandryConfig::default();

        // Tier 0 adds nothing: the pre-tier base chance (15%).
        assert!((trait_roll_chance(&husbandry, 1, 1.0) - 0.15).abs() < f64::EPSILON);
        assert!((trait_roll_chance(&husbandry, 9, 1.0) - 0.15).abs() < f64::EPSILON);
        // Tier 4 at level 100: 15% + 4 * 2% = 23%.
        assert!((trait_roll_chance(&husbandry, 100, 1.0) - 0.23).abs() < f64::EPSILON);
    }

    #[test]
    fn trait_roll_chance_respects_global_cap() {
        let husbandry = super::super::config::HusbandryConfig::default();

        assert!((trait_roll_chance(&husbandry, 100, 0.20) - 0.20).abs() < f64::EPSILON);
        assert!((trait_roll_chance(&husbandry, 1, 0.10) - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn trait_roll_count_unlocks_second_trait_at_min_tier() {
        let husbandry = super::super::config::HusbandryConfig::default();

        // Default config: three traits, double_trait_min_tier 3 (level 50).
        for level in [1, 9, 10, 25, 49] {
            assert_eq!(trait_roll_count(&husbandry, level), 1, "level {level}");
        }
        for level in [50, 99, 100] {
            assert_eq!(trait_roll_count(&husbandry, level), 2, "level {level}");
        }
    }

    #[test]
    fn trait_roll_count_needs_two_configured_traits() {
        let husbandry = super::super::config::HusbandryConfig {
            traits: vec!["hardy".to_string()],
            ..super::super::config::HusbandryConfig::default()
        };

        // A single configured trait can never yield a distinct second pick.
        assert_eq!(trait_roll_count(&husbandry, 100), 1);

        let husbandry = super::super::config::HusbandryConfig {
            double_trait_min_tier: 4,
            ..super::super::config::HusbandryConfig::default()
        };
        assert_eq!(trait_roll_count(&husbandry, 50), 1);
        assert_eq!(trait_roll_count(&husbandry, 100), 2);
    }

    #[test]
    fn second_distinct_index_is_always_distinct_and_in_range() {
        const LEN: usize = 3;
        for first in 0..LEN {
            for draw in 0..LEN - 1 {
                let index = second_distinct_index(first, draw);
                assert_ne!(index, first, "first {first} draw {draw}");
                assert!(index < LEN, "first {first} draw {draw}");
            }
        }
        // Every non-first index is reachable from some draw.
        for first in 0..LEN {
            let reached: std::collections::HashSet<usize> = (0..LEN - 1)
                .map(|draw| second_distinct_index(first, draw))
                .collect();
            assert_eq!(reached.len(), LEN - 1, "first {first}");
        }
    }
}
