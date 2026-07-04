use std::collections::HashMap;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use uuid::Uuid;

#[derive(Clone)]
pub struct ActiveMobSnapshot {
    pub uuid: Uuid,
    pub world_uuid: Uuid,
    pub current_pos: Vector3<f64>,
    pub current_block: BlockPos,
    pub current_velocity: Vector3<f64>,
    pub movement_speed: f64,
    pub path_target: Option<PathVelocityTarget>,
}

#[derive(Clone)]
pub struct VelocityJobSnapshot {
    pub uuid: Uuid,
    pub world_uuid: Uuid,
    pub current_pos: Vector3<f64>,
    pub current_block: BlockPos,
    pub current_velocity: Vector3<f64>,
    pub movement_speed: f64,
    pub path_target: Option<PathVelocityTarget>,
    pub location_table: std::sync::Arc<MobLocationTable>,
}

#[derive(Clone, Copy)]
pub struct PathVelocityTarget {
    pub target_pos: Vector3<f64>,
    pub next_step: BlockPos,
}

#[derive(Clone, Copy)]
pub struct VelocityPlan {
    pub velocity: Vector3<f64>,
    pub steering_delta: Vector3<f64>,
}

#[derive(Clone, Copy)]
pub struct MobLocationEntry {
    pub uuid: Uuid,
    pub world_uuid: Uuid,
    pub pos: Vector3<f64>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClusterCell {
    pub world_uuid: Uuid,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Default)]
pub struct MobLocationTable {
    pub entries: HashMap<Uuid, MobLocationEntry>,
    pub cells: HashMap<ClusterCell, Vec<Uuid>>,
}
