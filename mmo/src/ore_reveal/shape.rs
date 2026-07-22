use std::collections::HashSet;

use pumpkin_data::BlockDirection;
use pumpkin_util::math::position::BlockPos;
use rand::{Rng, RngExt};

use super::config::VeinShapeConfig;

pub(super) fn grow_vein<R, F>(
    rng: &mut R,
    start: BlockPos,
    forward: BlockDirection,
    target_size: usize,
    config: &VeinShapeConfig,
    mut is_eligible: F,
) -> Vec<BlockPos>
where
    R: Rng + ?Sized,
    F: FnMut(&BlockPos, bool) -> bool,
{
    if target_size == 0 || !is_eligible(&start, true) {
        return Vec::new();
    }

    let mut vein = vec![start];
    let mut seen = HashSet::from([start]);
    let max_attempts = target_size.saturating_mul(32).max(32);

    for _ in 0..max_attempts {
        if vein.len() >= target_size {
            break;
        }
        let anchor_index = if vein.len() > 1 && rng.random_bool(config.branch_chance) {
            rng.random_range(0..vein.len())
        } else {
            vein.len() - 1
        };
        let direction = weighted_direction(rng, forward, config.forward_bias);
        let candidate = vein[anchor_index].offset(direction.to_offset());
        if !inside_radius(start, candidate, config.max_radius)
            || !seen.insert(candidate)
            || !is_eligible(&candidate, false)
        {
            continue;
        }
        vein.push(candidate);
    }

    vein
}

fn weighted_direction<R: Rng + ?Sized>(
    rng: &mut R,
    forward: BlockDirection,
    forward_bias: f64,
) -> BlockDirection {
    let backward = forward.opposite();
    let weights = BlockDirection::all().map(|direction| {
        if direction == forward {
            forward_bias
        } else if direction == backward {
            0.35
        } else {
            1.0
        }
    });
    let mut roll = rng.random::<f64>() * weights.iter().sum::<f64>();
    for (direction, weight) in BlockDirection::all().into_iter().zip(weights) {
        if roll < weight {
            return direction;
        }
        roll -= weight;
    }
    forward
}

fn inside_radius(origin: BlockPos, candidate: BlockPos, radius: u32) -> bool {
    let delta = candidate.0 - origin.0;
    delta.x.unsigned_abs() <= radius
        && delta.y.unsigned_abs() <= radius
        && delta.z.unsigned_abs() <= radius
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    #[test]
    fn shape_is_deterministic_bounded_and_unique() {
        let config = VeinShapeConfig::default();
        let mut first_rng = StdRng::seed_from_u64(7);
        let mut second_rng = StdRng::seed_from_u64(7);
        let first = grow_vein(
            &mut first_rng,
            BlockPos::ZERO,
            BlockDirection::North,
            12,
            &config,
            |_, _| true,
        );
        let second = grow_vein(
            &mut second_rng,
            BlockPos::ZERO,
            BlockDirection::North,
            12,
            &config,
            |_, _| true,
        );
        assert_eq!(first, second);
        assert_eq!(first.len(), 12);
        assert_eq!(first.iter().copied().collect::<HashSet<_>>().len(), 12);
        assert!(
            first
                .iter()
                .all(|pos| inside_radius(BlockPos::ZERO, *pos, 5))
        );
    }

    #[test]
    fn ineligible_seed_prevents_a_vein() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!(
            grow_vein(
                &mut rng,
                BlockPos::ZERO,
                BlockDirection::Down,
                4,
                &VeinShapeConfig::default(),
                |_, seed| !seed,
            )
            .is_empty()
        );
    }
}
