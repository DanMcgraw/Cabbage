use std::collections::VecDeque;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3, wrap_degrees};
use crate::mob_ai::types::{VelocityJobSnapshot, VelocityPlan};
use crate::mob_ai::pathfinding::block_center_feet_pos;
use crate::mob_ai::clustering::cluster_push_velocity;

pub const MOB_PATH_VELOCITY_BLOCKS_PER_TICK: f64 = 0.25;
pub const UPWARD_PATH_VELOCITY_MULTIPLIER: f64 = 2.0;
pub const LOOKAHEAD_WEIGHTS: [f64; 3] = [3.0, 2.0, 1.0];
pub const LOOKAHEAD_WEIGHT_TOTAL: f64 = 6.0;

pub fn compute_velocity_plan(job: &VelocityJobSnapshot) -> Option<VelocityPlan> {
    let mut steering_delta = Vector3::new(0.0, 0.0, 0.0);
    debug_assert_eq!(
        job.location_table
            .entries
            .get(&job.uuid)
            .map(|entry| entry.world_uuid),
        Some(job.world_uuid)
    );

    if let Some(path_target) = job.path_target {
        let path_velocity = velocity_toward(
            job.current_pos,
            path_target.target_pos,
            job.current_block,
            path_target.next_step,
            job.movement_speed,
        );
        steering_delta.x += path_velocity.x;
        steering_delta.y += path_velocity.y;
        steering_delta.z += path_velocity.z;
    }

    let cluster_velocity =
        cluster_push_velocity(job.uuid, job.movement_speed, job.location_table.as_ref());
    steering_delta.x += cluster_velocity.x;
    steering_delta.z += cluster_velocity.z;

    if steering_delta.length_squared() == 0.0 {
        return None;
    }

    Some(VelocityPlan {
        velocity: velocity_with_pathfinding_delta(job.current_velocity, steering_delta),
        steering_delta,
    })
}

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

pub fn velocity_toward(
    current_pos: Vector3<f64>,
    target_pos: Vector3<f64>,
    current_block: BlockPos,
    next_step: BlockPos,
    movement_speed: f64,
) -> Vector3<f64> {
    let delta = target_pos - current_pos;
    let mut velocity = if delta.length_squared() <= MOB_PATH_VELOCITY_BLOCKS_PER_TICK.powi(2) {
        delta
    } else {
        delta.normalize().multiply(
            MOB_PATH_VELOCITY_BLOCKS_PER_TICK,
            MOB_PATH_VELOCITY_BLOCKS_PER_TICK,
            MOB_PATH_VELOCITY_BLOCKS_PER_TICK,
        )
    };

    velocity.x *= 2.0 * movement_speed;
    velocity.z *= 2.0 * movement_speed;

    if velocity.y < 0.0 {
        velocity.y = 0.0;
    } else if velocity.y > 0.0 {
        if current_pos.y.trunc() != current_pos.y {
            velocity.y = 0.0;
        } else if next_step.0.y > current_block.0.y {
            velocity.y *= UPWARD_PATH_VELOCITY_MULTIPLIER;
        }
    }

    velocity
}

pub fn velocity_with_pathfinding_delta(
    mut current_velocity: Vector3<f64>,
    path_velocity: Vector3<f64>,
) -> Vector3<f64> {
    current_velocity = current_velocity * 0.3;
    current_velocity.x += path_velocity.x;
    current_velocity.y = path_velocity.y;
    current_velocity.z += path_velocity.z;
    current_velocity
}

pub fn point_body_along_velocity(entity: &pumpkin::entity::Entity, _velocity: Vector3<f64>) {
    entity.yaw.store(0.0);
    //entity.head_yaw.store(0.0);
    //entity.body_yaw.store(0.0);
}

#[allow(dead_code)]
pub fn yaw_from_xz_delta(dx: f64, dz: f64) -> Option<f32> {
    if dx.abs() <= 1.0E-5 && dz.abs() <= 1.0E-5 {
        None
    } else {
        Some(wrap_degrees((dz.atan2(dx) as f32).to_degrees() - 90.0))
    }
}

#[allow(dead_code)]
pub fn pitch_from_delta(dy: f64, horizontal_distance: f64) -> f32 {
    wrap_degrees(-(dy.atan2(horizontal_distance) as f32).to_degrees()).clamp(-90.0, 90.0)
}

#[allow(dead_code)]
pub fn rotation_to_packet_byte(rotation: f32) -> u8 {
    (rotation * 256.0 / 360.0).rem_euclid(256.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_from_xz_delta_matches_pumpkin_convention() {
        assert_eq!(yaw_from_xz_delta(1.0, 0.0), Some(-90.0));
        assert_eq!(yaw_from_xz_delta(0.0, 1.0), Some(0.0));
        assert_eq!(yaw_from_xz_delta(-1.0, 0.0), Some(90.0));
        assert_eq!(yaw_from_xz_delta(0.0, 0.0), None);
    }

    #[test]
    fn pitch_from_delta_faces_up_and_down() {
        assert_eq!(pitch_from_delta(1.0, 0.0), -90.0);
        assert_eq!(pitch_from_delta(-1.0, 0.0), 90.0);
        assert_eq!(pitch_from_delta(0.0, 1.0), -0.0);
    }

    #[test]
    fn weighted_lookahead_target_weights_first_three_steps() {
        let steps = VecDeque::from([BlockPos::new(1, 0, 0), BlockPos::new(3, 0, 0), BlockPos::new(5, 0, 0)]);
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

    #[test]
    fn velocity_toward_caps_to_path_speed() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 0, 0),
            1.0,
        );

        assert_eq!(
            velocity,
            Vector3::new(MOB_PATH_VELOCITY_BLOCKS_PER_TICK * 2.0, 0.0, 0.0)
        );
    }

    #[test]
    fn velocity_toward_uses_remaining_delta_when_close() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.1, 0.0, 0.0),
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 0, 0),
            1.0,
        );

        assert_eq!(velocity, Vector3::new(0.2, 0.0, 0.0));
    }

    #[test]
    fn velocity_toward_boosts_upward_velocity_for_upward_next_step() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            BlockPos::new(0, 0, 0),
            BlockPos::new(0, 1, 0),
            0.0,
        );

        assert_eq!(
            velocity,
            Vector3::new(
                0.0,
                MOB_PATH_VELOCITY_BLOCKS_PER_TICK * UPWARD_PATH_VELOCITY_MULTIPLIER,
                0.0
            )
        );
    }

    #[test]
    fn velocity_toward_blocks_upward_velocity_when_not_on_exact_block_y() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.5, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            BlockPos::new(0, 0, 0),
            BlockPos::new(0, 1, 0),
            0.0,
        );

        assert_eq!(velocity, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn velocity_toward_never_sets_negative_y_velocity() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            BlockPos::new(0, 1, 0),
            BlockPos::new(0, 0, 0),
            0.0,
        );

        assert_eq!(velocity, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn velocity_toward_scales_horizontal_velocity_by_movement_speed() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 0, 0),
            2.0,
        );

        assert_eq!(
            velocity,
            Vector3::new(MOB_PATH_VELOCITY_BLOCKS_PER_TICK * 4.0, 0.0, 0.0)
        );
    }

    #[test]
    fn velocity_with_pathfinding_delta_adds_horizontal_velocity_to_existing_velocity() {
        let velocity = velocity_with_pathfinding_delta(
            Vector3::new(0.25, -0.125, 0.5),
            Vector3::new(0.5, 0.75, -0.25),
        );

        assert_eq!(velocity, Vector3::new(0.575, 0.75, -0.1));
    }
}
