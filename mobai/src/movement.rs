#![allow(dead_code)]
use crate::pathfinding::block_center_feet_pos;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use std::collections::VecDeque;

pub const LOOKAHEAD_WEIGHTS: [f64; 3] = [3.0, 2.0, 1.0];
pub const LOOKAHEAD_WEIGHT_TOTAL: f64 = 6.0;

pub fn weighted_lookahead_target(steps: &VecDeque<BlockPos>) -> Option<Vector3<f64>> {
    let first = steps.front().copied()?;
    let mut weighted = Vector3::new(0.0, 0.0, 0.0);

    for (index, weight) in LOOKAHEAD_WEIGHTS.iter().copied().enumerate() {
        let step = steps.get(index).copied().unwrap_or(first);
        let center = block_center_feet_pos(step);
        weighted.x += center.x * weight;
        weighted.y += center.y * weight;
        weighted.z += center.z * weight;
    }

    Some(Vector3::new(
        weighted.x / LOOKAHEAD_WEIGHT_TOTAL,
        weighted.y / LOOKAHEAD_WEIGHT_TOTAL,
        weighted.z / LOOKAHEAD_WEIGHT_TOTAL,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_lookahead_target_weights_first_three_steps() {
        let steps = VecDeque::from([
            BlockPos::new(1, 0, 0),
            BlockPos::new(3, 0, 0),
            BlockPos::new(5, 0, 0),
        ]);
        let target = weighted_lookahead_target(&steps).unwrap();

        assert!((target.x - (17.0 / 6.0)).abs() < f64::EPSILON);
        assert_eq!(target.y, 0.0);
        assert_eq!(target.z, 0.5);
    }

    #[test]
    fn weighted_lookahead_target_uses_first_step_for_missing_steps() {
        let steps = VecDeque::from([BlockPos::new(1, 2, 3)]);
        let target = weighted_lookahead_target(&steps).unwrap();

        assert_eq!(target, block_center_feet_pos(BlockPos::new(1, 2, 3)));
    }
}
