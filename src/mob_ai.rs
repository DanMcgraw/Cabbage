use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use pumpkin::{
    entity::EntityBase,
    plugin::{BoxFuture, EventHandler, server::server_tick_start::ServerTickStartEvent},
    server::Server,
    world::World,
};
use pumpkin_data::attributes::Attributes;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3, wrap_degrees};
use rayon::{ThreadPool, ThreadPoolBuilder};
use uuid::Uuid;

const MOB_JUMP_TYPES: [&str; 3] = ["zombie", "skeleton", "creeper"];
const PATH_BOX_OUTSET_BLOCKS: i32 = 3;
const MOB_MOVE_PERIOD_TICKS: i32 = 4;
const PATH_INTERVAL_HORIZONTAL_DISTANCE_MULTIPLIER_TICKS: i64 = 2;
const MIN_PATH_INTERVAL_TICKS: i64 = 5;
const MAX_PATH_HEIGHT_DIFFERENCE: i32 = 8;
const MAX_PATH_GRID_VOLUME: usize = 65_536;
const MOB_PATH_VELOCITY_BLOCKS_PER_TICK: f64 = 0.25;
const UPWARD_PATH_VELOCITY_MULTIPLIER: f64 = 3.0;
const CLUSTER_DIAMETER_BLOCKS: f64 = 2.0;
const CLUSTER_CENTER_PUSH_RADIUS_BLOCKS: f64 = 1.0;
const CLUSTER_CELL_SIZE_BLOCKS: f64 = CLUSTER_DIAMETER_BLOCKS;
const LOOKAHEAD_WEIGHTS: [f64; 3] = [3.0, 2.0, 1.0];
const LOOKAHEAD_WEIGHT_TOTAL: f64 = 6.0;
const CARDINAL_PATH_STEP_COST: u32 = 10;
const DIAGONAL_XZ_PATH_STEP_COST: u32 = 14;
const NEIGHBOR_OFFSETS: [Vector3<i32>; 10] = [
    Vector3::new(1, 0, 0),
    Vector3::new(-1, 0, 0),
    Vector3::new(0, 1, 0),
    Vector3::new(0, -1, 0),
    Vector3::new(0, 0, 1),
    Vector3::new(0, 0, -1),
    Vector3::new(1, 0, 1),
    Vector3::new(1, 0, -1),
    Vector3::new(-1, 0, 1),
    Vector3::new(-1, 0, -1),
];

pub(crate) struct MobAiState {
    worker_pool: Arc<ThreadPool>,
    last_path_ticks: Mutex<HashMap<Uuid, i32>>,
    active_path_jobs: Arc<Mutex<HashSet<Uuid>>>,
    path_steps: Arc<Mutex<HashMap<Uuid, VecDeque<BlockPos>>>>,
    active_velocity_jobs: Arc<Mutex<HashSet<Uuid>>>,
    planned_velocities: Arc<Mutex<HashMap<Uuid, VelocityPlan>>>,
    mob_locations: Mutex<MobLocationTable>,
}

impl Default for MobAiState {
    fn default() -> Self {
        let worker_threads = cabbage_worker_thread_count();
        let worker_pool = ThreadPoolBuilder::new()
            .num_threads(worker_threads)
            .thread_name(|index| format!("cabbage-mob-ai-{index}"))
            .build()
            .expect("failed to build Cabbage mob AI worker pool");

        Self {
            worker_pool: Arc::new(worker_pool),
            last_path_ticks: Mutex::new(HashMap::new()),
            active_path_jobs: Arc::new(Mutex::new(HashSet::new())),
            path_steps: Arc::new(Mutex::new(HashMap::new())),
            active_velocity_jobs: Arc::new(Mutex::new(HashSet::new())),
            planned_velocities: Arc::new(Mutex::new(HashMap::new())),
            mob_locations: Mutex::new(MobLocationTable::default()),
        }
    }
}

impl EventHandler<ServerTickStartEvent> for MobAiState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        self.run_tick(server, event.tick)
    }

    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        self.run_tick(server, event.tick)
    }
}

impl MobAiState {
    fn run_tick<'a>(&'a self, server: &'a Arc<Server>, tick: i32) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut seen_mobs = HashSet::new();
            let mut active_mobs = Vec::new();
            let should_update_velocity = tick % MOB_MOVE_PERIOD_TICKS == 0;

            for world in server.worlds.load().iter() {
                if world.players.load().is_empty() {
                    continue;
                }

                for entity_base in world.entities.load().iter() {
                    let entity = entity_base.get_entity();

                    if !MOB_JUMP_TYPES.contains(&entity.entity_type.resource_name) {
                        continue;
                    }

                    seen_mobs.insert(entity.entity_uuid);

                    let current_pos = entity.pos.load();
                    let mob_pos = BlockPos::floored_v(current_pos);
                    let Some((target_pos, horizontal_distance)) =
                        nearest_player_pos(world, mob_pos)
                    else {
                        continue;
                    };

                    if path_height_difference_exceeded(mob_pos, target_pos) {
                        self.clear_path(entity.entity_uuid);
                        continue;
                    }

                    let Some(living_entity) = entity_base.get_living_entity() else {
                        continue;
                    };

                    let path_target = if should_update_velocity {
                        self.path_velocity_target(entity.entity_uuid, mob_pos)
                            .and_then(|(target_pos, next_step)| {
                                if world.get_block_state(&next_step).is_solid_block() {
                                    self.clear_path(entity.entity_uuid);
                                    None
                                } else {
                                    Some(PathVelocityTarget {
                                        target_pos,
                                        next_step,
                                    })
                                }
                            })
                    } else {
                        None
                    };

                    if should_update_velocity {
                        self.apply_planned_velocity(entity_base.as_ref(), entity.entity_uuid);
                    }

                    active_mobs.push(ActiveMobSnapshot {
                        uuid: entity.entity_uuid,
                        world_uuid: world.uuid,
                        current_pos,
                        current_block: mob_pos,
                        current_velocity: entity.velocity.load(),
                        movement_speed: living_entity
                            .get_attribute_value(&Attributes::MOVEMENT_SPEED),
                        path_target,
                    });

                    let interval = path_interval_ticks(horizontal_distance);
                    if !self.should_start_path_job(entity.entity_uuid, tick, interval) {
                        continue;
                    }

                    let bounds = PathBounds::between(mob_pos, target_pos, PATH_BOX_OUTSET_BLOCKS);
                    let Some(grid) = BlockGrid::sample(bounds, |pos| {
                        world.get_block_state(&pos).is_solid_block()
                    }) else {
                        self.clear_active_path_job(entity.entity_uuid);
                        continue;
                    };

                    self.spawn_path_job(entity.entity_uuid, grid, mob_pos, target_pos);
                }
            }

            if should_update_velocity {
                self.update_mob_locations(&active_mobs);
                self.spawn_velocity_jobs(&active_mobs);
            }

            self.retain_seen_mobs(&seen_mobs);
        })
    }
}

impl MobAiState {
    fn should_start_path_job(&self, uuid: Uuid, tick: i32, interval: i64) -> bool {
        {
            let mut active_path_jobs = self.active_path_jobs.lock().unwrap();
            if !active_path_jobs.insert(uuid) {
                return false;
            }
        }

        let mut last_path_ticks = self.last_path_ticks.lock().unwrap();
        let should_attempt = last_path_ticks
            .get(&uuid)
            .is_none_or(|last_tick| i64::from(tick) - i64::from(*last_tick) >= interval);

        if should_attempt {
            last_path_ticks.insert(uuid, tick);
        } else {
            self.clear_active_path_job(uuid);
        }

        should_attempt
    }

    fn spawn_path_job(&self, uuid: Uuid, grid: BlockGrid, mob_pos: BlockPos, target_pos: BlockPos) {
        let active_path_jobs = Arc::clone(&self.active_path_jobs);
        let path_steps = Arc::clone(&self.path_steps);

        self.worker_pool.spawn(move || {
            let steps = bidirectional_a_star(&grid, mob_pos, target_pos)
                .map(|path| movement_path_steps(&path, mob_pos))
                .filter(|steps| !steps.is_empty());

            {
                let mut path_steps = path_steps.lock().unwrap();
                if let Some(steps) = steps {
                    path_steps.insert(uuid, steps);
                } else {
                    path_steps.remove(&uuid);
                }
            }

            active_path_jobs.lock().unwrap().remove(&uuid);
        });
    }

    fn update_mob_locations(&self, active_mobs: &[ActiveMobSnapshot]) {
        let entries = active_mobs.iter().map(|mob| MobLocationEntry {
            uuid: mob.uuid,
            world_uuid: mob.world_uuid,
            pos: mob.current_pos,
        });

        *self.mob_locations.lock().unwrap() = MobLocationTable::from_entries(entries);
    }

    fn apply_planned_velocity(&self, entity_base: &dyn EntityBase, uuid: Uuid) {
        let mut planned_velocities = self.planned_velocities.lock().unwrap();
        let Some(plan) = planned_velocities.remove(&uuid) else {
            return;
        };

        drop(planned_velocities);

        let entity = entity_base.get_entity();
        point_body_along_velocity(entity, plan.steering_delta);
        entity.velocity.store(plan.velocity);
        entity
            .velocity_dirty
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn spawn_velocity_jobs(&self, active_mobs: &[ActiveMobSnapshot]) {
        let location_table = Arc::new(self.mob_locations.lock().unwrap().clone());

        for mob in active_mobs {
            {
                let mut active_velocity_jobs = self.active_velocity_jobs.lock().unwrap();
                if !active_velocity_jobs.insert(mob.uuid) {
                    continue;
                }
            }

            let job = VelocityJobSnapshot {
                uuid: mob.uuid,
                world_uuid: mob.world_uuid,
                current_pos: mob.current_pos,
                current_block: mob.current_block,
                current_velocity: mob.current_velocity,
                movement_speed: mob.movement_speed,
                path_target: mob.path_target,
                location_table: Arc::clone(&location_table),
            };
            let active_velocity_jobs = Arc::clone(&self.active_velocity_jobs);
            let planned_velocities = Arc::clone(&self.planned_velocities);

            self.worker_pool.spawn(move || {
                if let Some(plan) = compute_velocity_plan(&job) {
                    planned_velocities.lock().unwrap().insert(job.uuid, plan);
                }

                active_velocity_jobs.lock().unwrap().remove(&job.uuid);
            });
        }
    }

    fn path_velocity_target(
        &self,
        uuid: Uuid,
        current_pos: BlockPos,
    ) -> Option<(Vector3<f64>, BlockPos)> {
        let mut path_steps = self.path_steps.lock().unwrap();
        let steps = path_steps.get_mut(&uuid)?;

        while steps.front().is_some_and(|step| *step == current_pos) {
            steps.pop_front();
        }

        if steps.is_empty() {
            path_steps.remove(&uuid);
            return None;
        }

        let next_step = steps.front().copied()?;
        weighted_lookahead_target(steps).map(|target| (target, next_step))
    }

    fn clear_path(&self, uuid: Uuid) {
        self.path_steps.lock().unwrap().remove(&uuid);
    }

    fn clear_active_path_job(&self, uuid: Uuid) {
        self.active_path_jobs.lock().unwrap().remove(&uuid);
    }

    fn retain_seen_mobs(&self, seen_mobs: &HashSet<Uuid>) {
        let mut last_path_ticks = self.last_path_ticks.lock().unwrap();
        last_path_ticks.retain(|uuid, _| seen_mobs.contains(uuid));

        let mut path_steps = self.path_steps.lock().unwrap();
        path_steps.retain(|uuid, _| seen_mobs.contains(uuid));

        let mut active_velocity_jobs = self.active_velocity_jobs.lock().unwrap();
        active_velocity_jobs.retain(|uuid| seen_mobs.contains(uuid));

        let mut planned_velocities = self.planned_velocities.lock().unwrap();
        planned_velocities.retain(|uuid, _| seen_mobs.contains(uuid));
    }
}

#[derive(Clone)]
struct ActiveMobSnapshot {
    uuid: Uuid,
    world_uuid: Uuid,
    current_pos: Vector3<f64>,
    current_block: BlockPos,
    current_velocity: Vector3<f64>,
    movement_speed: f64,
    path_target: Option<PathVelocityTarget>,
}

#[derive(Clone)]
struct VelocityJobSnapshot {
    uuid: Uuid,
    world_uuid: Uuid,
    current_pos: Vector3<f64>,
    current_block: BlockPos,
    current_velocity: Vector3<f64>,
    movement_speed: f64,
    path_target: Option<PathVelocityTarget>,
    location_table: Arc<MobLocationTable>,
}

#[derive(Clone, Copy)]
struct PathVelocityTarget {
    target_pos: Vector3<f64>,
    next_step: BlockPos,
}

#[derive(Clone, Copy)]
struct VelocityPlan {
    velocity: Vector3<f64>,
    steering_delta: Vector3<f64>,
}

#[derive(Clone, Copy)]
struct MobLocationEntry {
    uuid: Uuid,
    world_uuid: Uuid,
    pos: Vector3<f64>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ClusterCell {
    world_uuid: Uuid,
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Clone, Default)]
struct MobLocationTable {
    entries: HashMap<Uuid, MobLocationEntry>,
    cells: HashMap<ClusterCell, Vec<Uuid>>,
}

impl MobLocationTable {
    fn from_entries(entries: impl IntoIterator<Item = MobLocationEntry>) -> Self {
        let mut table = Self::default();

        for entry in entries {
            let cell = cluster_cell(entry.world_uuid, entry.pos);
            table.cells.entry(cell).or_default().push(entry.uuid);
            table.entries.insert(entry.uuid, entry);
        }

        table
    }

    fn query_cluster(&self, uuid: Uuid) -> Option<MobCluster> {
        let entry = self.entries.get(&uuid).copied()?;
        let center_cell = cluster_cell(entry.world_uuid, entry.pos);
        let mut candidates = Vec::new();
        let max_distance_squared = CLUSTER_DIAMETER_BLOCKS * CLUSTER_DIAMETER_BLOCKS;

        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let cell = ClusterCell {
                        world_uuid: entry.world_uuid,
                        x: center_cell.x + dx,
                        y: center_cell.y + dy,
                        z: center_cell.z + dz,
                    };

                    let Some(uuids) = self.cells.get(&cell) else {
                        continue;
                    };

                    for candidate_uuid in uuids {
                        let Some(candidate) = self.entries.get(candidate_uuid).copied() else {
                            continue;
                        };

                        if vector_distance_squared(entry.pos, candidate.pos) <= max_distance_squared
                        {
                            candidates.push(candidate);
                        }
                    }
                }
            }
        }

        candidates.sort_by(|a, b| {
            vector_distance_squared(entry.pos, a.pos)
                .total_cmp(&vector_distance_squared(entry.pos, b.pos))
                .then_with(|| a.uuid.cmp(&b.uuid))
        });

        let candidates = diameter_limited_cluster_candidates(entry, candidates);
        if candidates.len() < 2 {
            return None;
        }

        let center = cluster_center(&candidates);
        let center_radius_squared =
            CLUSTER_CENTER_PUSH_RADIUS_BLOCKS * CLUSTER_CENTER_PUSH_RADIUS_BLOCKS;
        let mut members = candidates
            .into_iter()
            .filter_map(|candidate| {
                let offset_from_center = candidate.pos - center;
                (offset_from_center.length_squared() <= center_radius_squared).then_some(
                    MobClusterMember {
                        uuid: candidate.uuid,
                        offset_from_center,
                    },
                )
            })
            .collect::<Vec<_>>();

        if members.len() < 2 {
            return None;
        }

        members.sort_by_key(|member| member.uuid);
        Some(MobCluster { center, members })
    }
}

#[derive(Clone, Debug)]
struct MobCluster {
    center: Vector3<f64>,
    members: Vec<MobClusterMember>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MobClusterMember {
    uuid: Uuid,
    offset_from_center: Vector3<f64>,
}

fn cabbage_worker_thread_count() -> usize {
    cabbage_worker_thread_count_for(
        std::thread::available_parallelism()
            .map(|count| count.get())
            .ok(),
    )
}

const fn cabbage_worker_thread_count_for(available_parallelism: Option<usize>) -> usize {
    match available_parallelism {
        Some(count) if count > 0 => count,
        _ => 1,
    }
}

fn compute_velocity_plan(job: &VelocityJobSnapshot) -> Option<VelocityPlan> {
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
    steering_delta.y += cluster_velocity.y;
    steering_delta.z += cluster_velocity.z;

    if steering_delta.length_squared() == 0.0 {
        return None;
    }

    Some(VelocityPlan {
        velocity: velocity_with_pathfinding_delta(job.current_velocity, steering_delta),
        steering_delta,
    })
}

fn cluster_push_velocity(
    uuid: Uuid,
    movement_speed: f64,
    location_table: &MobLocationTable,
) -> Vector3<f64> {
    let Some(cluster) = location_table.query_cluster(uuid) else {
        return Vector3::new(0.0, 0.0, 0.0);
    };
    let _cluster_center = cluster.center;
    let Some(member) = cluster.members.iter().find(|member| member.uuid == uuid) else {
        return Vector3::new(0.0, 0.0, 0.0);
    };

    let mut direction = member.offset_from_center.normalize();
    if direction.length_squared() == 0.0 {
        direction = stable_horizontal_direction(uuid);
    }

    let mut velocity = direction.multiply(movement_speed, movement_speed, movement_speed);
    if velocity.y < 0.0 {
        velocity.y = 0.0;
    }
    velocity
}

fn stable_horizontal_direction(uuid: Uuid) -> Vector3<f64> {
    let radians = ((uuid.as_u128() % 360) as f64).to_radians();
    Vector3::new(radians.cos(), 0.0, radians.sin())
}

fn cluster_cell(world_uuid: Uuid, pos: Vector3<f64>) -> ClusterCell {
    ClusterCell {
        world_uuid,
        x: cluster_axis_cell(pos.x),
        y: cluster_axis_cell(pos.y),
        z: cluster_axis_cell(pos.z),
    }
}

fn cluster_axis_cell(value: f64) -> i32 {
    (value / CLUSTER_CELL_SIZE_BLOCKS).floor() as i32
}

fn cluster_center(entries: &[MobLocationEntry]) -> Vector3<f64> {
    let mut sum = Vector3::new(0.0, 0.0, 0.0);

    for entry in entries {
        sum.x += entry.pos.x;
        sum.y += entry.pos.y;
        sum.z += entry.pos.z;
    }

    let count = entries.len() as f64;
    Vector3::new(sum.x / count, sum.y / count, sum.z / count)
}

fn vector_distance_squared(a: Vector3<f64>, b: Vector3<f64>) -> f64 {
    (a - b).length_squared()
}

fn diameter_limited_cluster_candidates(
    entry: MobLocationEntry,
    candidates: Vec<MobLocationEntry>,
) -> Vec<MobLocationEntry> {
    let max_distance_squared = CLUSTER_DIAMETER_BLOCKS * CLUSTER_DIAMETER_BLOCKS;
    let mut members = vec![entry];

    for candidate in candidates {
        if candidate.uuid == entry.uuid {
            continue;
        }

        if members.iter().all(|member| {
            vector_distance_squared(member.pos, candidate.pos) <= max_distance_squared
        }) {
            members.push(candidate);
        }
    }

    members
}

fn weighted_lookahead_target(steps: &VecDeque<BlockPos>) -> Option<Vector3<f64>> {
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

fn velocity_toward(
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

    velocity.x *= movement_speed;
    velocity.z *= movement_speed;

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

fn velocity_with_pathfinding_delta(
    mut current_velocity: Vector3<f64>,
    path_velocity: Vector3<f64>,
) -> Vector3<f64> {
    current_velocity.x += path_velocity.x;
    current_velocity.y = path_velocity.y;
    current_velocity.z += path_velocity.z;
    current_velocity
}

fn nearest_player_pos(world: &World, mob_pos: BlockPos) -> Option<(BlockPos, i64)> {
    world
        .players
        .load()
        .iter()
        .map(|player| {
            let player_entity = player.get_entity();
            let player_pos = BlockPos::floored_v(player_entity.pos.load());
            let distance_squared = block_distance_squared(mob_pos, player_pos);
            let horizontal_distance = horizontal_block_distance(mob_pos, player_pos);
            (player_pos, distance_squared, horizontal_distance)
        })
        .min_by_key(|(_, distance_squared, _)| *distance_squared)
        .map(|(player_pos, _, horizontal_distance)| (player_pos, horizontal_distance))
}

fn path_interval_ticks(horizontal_distance: i64) -> i64 {
    PATH_INTERVAL_HORIZONTAL_DISTANCE_MULTIPLIER_TICKS
        .saturating_mul(horizontal_distance)
        .max(MIN_PATH_INTERVAL_TICKS)
}

fn path_height_difference_exceeded(mob_pos: BlockPos, target_pos: BlockPos) -> bool {
    mob_pos.0.y.abs_diff(target_pos.0.y) > MAX_PATH_HEIGHT_DIFFERENCE as u32
}

fn block_distance_squared(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i128::from(a.0.x) - i128::from(b.0.x);
    let dy = i128::from(a.0.y) - i128::from(b.0.y);
    let dz = i128::from(a.0.z) - i128::from(b.0.z);
    let distance_squared = dx * dx + dy * dy + dz * dz;
    distance_squared.min(i128::from(i64::MAX)) as i64
}

fn horizontal_block_distance(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i64::from(a.0.x).abs_diff(i64::from(b.0.x));
    let dz = i64::from(a.0.z).abs_diff(i64::from(b.0.z));
    i64::try_from(dx.saturating_add(dz)).unwrap_or(i64::MAX)
}

fn block_center_feet_pos(pos: BlockPos) -> Vector3<f64> {
    Vector3::new(
        f64::from(pos.0.x) + 0.5,
        f64::from(pos.0.y),
        f64::from(pos.0.z) + 0.5,
    )
}

fn point_body_along_velocity(entity: &pumpkin::entity::Entity, velocity: Vector3<f64>) {
    if let Some(body_yaw) = yaw_from_xz_delta(velocity.x, velocity.z) {
        entity.yaw.store(body_yaw);
        entity.head_yaw.store(body_yaw);
        entity.body_yaw.store(body_yaw);
    }
}

fn yaw_from_xz_delta(dx: f64, dz: f64) -> Option<f32> {
    if dx.abs() <= 1.0E-5 && dz.abs() <= 1.0E-5 {
        None
    } else {
        Some(wrap_degrees((dz.atan2(dx) as f32).to_degrees() - 90.0))
    }
}

#[allow(dead_code)]
fn pitch_from_delta(dy: f64, horizontal_distance: f64) -> f32 {
    wrap_degrees(-(dy.atan2(horizontal_distance) as f32).to_degrees()).clamp(-90.0, 90.0)
}

#[allow(dead_code)]
fn rotation_to_packet_byte(rotation: f32) -> u8 {
    (rotation * 256.0 / 360.0).rem_euclid(256.0) as u8
}

#[cfg(test)]
fn next_horizontal_path_step(path: &[BlockPos], start: BlockPos) -> Option<BlockPos> {
    path.iter()
        .skip(1)
        .copied()
        .find(|pos| pos.0.x != start.0.x || pos.0.z != start.0.z)
}

fn movement_path_steps(path: &[BlockPos], start: BlockPos) -> VecDeque<BlockPos> {
    let mut steps = VecDeque::new();
    let mut last_kept = start;

    for pos in path.iter().skip(1).copied() {
        let advances_xz = pos.0.x != last_kept.0.x || pos.0.z != last_kept.0.z;
        let climbs_up = pos.0.y > last_kept.0.y;
        if !advances_xz && !climbs_up {
            continue;
        }

        last_kept = pos;
        steps.push_back(pos);
    }

    steps
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PathBounds {
    min: BlockPos,
    max: BlockPos,
}

impl PathBounds {
    fn between(a: BlockPos, b: BlockPos, outset: i32) -> Self {
        Self {
            min: BlockPos::new(
                a.0.x.min(b.0.x).saturating_sub(outset),
                a.0.y.min(b.0.y).saturating_sub(outset),
                a.0.z.min(b.0.z).saturating_sub(outset),
            ),
            max: BlockPos::new(
                a.0.x.max(b.0.x).saturating_add(outset),
                a.0.y.max(b.0.y).saturating_add(outset),
                a.0.z.max(b.0.z).saturating_add(outset),
            ),
        }
    }

    fn contains(&self, pos: BlockPos) -> bool {
        pos.0.x >= self.min.0.x
            && pos.0.x <= self.max.0.x
            && pos.0.y >= self.min.0.y
            && pos.0.y <= self.max.0.y
            && pos.0.z >= self.min.0.z
            && pos.0.z <= self.max.0.z
    }
}

struct BlockGrid {
    bounds: PathBounds,
    size_x: usize,
    size_y: usize,
    size_z: usize,
    closed: Vec<bool>,
}

impl BlockGrid {
    fn sample(mut bounds: PathBounds, mut is_closed: impl FnMut(BlockPos) -> bool) -> Option<Self> {
        normalize_bounds(&mut bounds);

        let size_x = axis_len(bounds.min.0.x, bounds.max.0.x)?;
        let size_y = axis_len(bounds.min.0.y, bounds.max.0.y)?;
        let size_z = axis_len(bounds.min.0.z, bounds.max.0.z)?;
        let volume = size_x.checked_mul(size_y)?.checked_mul(size_z)?;
        if volume > MAX_PATH_GRID_VOLUME {
            return None;
        }

        let mut closed = Vec::with_capacity(volume);

        for z in bounds.min.0.z..=bounds.max.0.z {
            for y in bounds.min.0.y..=bounds.max.0.y {
                for x in bounds.min.0.x..=bounds.max.0.x {
                    closed.push(is_closed(BlockPos::new(x, y, z)));
                }
            }
        }

        Some(Self {
            bounds,
            size_x,
            size_y,
            size_z,
            closed,
        })
    }

    fn is_open(&self, pos: BlockPos) -> bool {
        self.index(pos)
            .and_then(|index| self.closed.get(index))
            .is_some_and(|closed| !closed)
    }

    fn neighbors(&self, pos: BlockPos, forward_search: bool) -> Vec<(BlockPos, u32)> {
        NEIGHBOR_OFFSETS
            .iter()
            .filter_map(|offset| {
                let next = pos.offset(*offset);

                if !self.movement_is_allowed(pos, next, *offset, forward_search)
                    || !self.is_open(next)
                {
                    return None;
                }

                Some((next, movement_cost(*offset)))
            })
            .collect()
    }

    fn movement_is_allowed(
        &self,
        current: BlockPos,
        next: BlockPos,
        offset: Vector3<i32>,
        forward_search: bool,
    ) -> bool {
        if !self.bounds.contains(next) {
            return false;
        }

        if is_xz_diagonal_offset(offset)
            && (!self.is_open(current.add(offset.x, 0, 0))
                || !self.is_open(current.add(0, 0, offset.z)))
        {
            return false;
        }

        let delta_y = next.0.y - current.0.y;
        if forward_search {
            delta_y <= 1
        } else {
            delta_y >= -1
        }
    }

    fn index(&self, pos: BlockPos) -> Option<usize> {
        if !self.bounds.contains(pos) {
            return None;
        }

        let x = usize::try_from(pos.0.x - self.bounds.min.0.x).ok()?;
        let y = usize::try_from(pos.0.y - self.bounds.min.0.y).ok()?;
        let z = usize::try_from(pos.0.z - self.bounds.min.0.z).ok()?;

        debug_assert!(x < self.size_x);
        debug_assert!(y < self.size_y);
        debug_assert!(z < self.size_z);

        Some((z * self.size_y + y) * self.size_x + x)
    }
}

fn normalize_bounds(bounds: &mut PathBounds) {
    let min = BlockPos::new(
        bounds.min.0.x.min(bounds.max.0.x),
        bounds.min.0.y.min(bounds.max.0.y),
        bounds.min.0.z.min(bounds.max.0.z),
    );
    let max = BlockPos::new(
        bounds.min.0.x.max(bounds.max.0.x),
        bounds.min.0.y.max(bounds.max.0.y),
        bounds.min.0.z.max(bounds.max.0.z),
    );

    bounds.min = min;
    bounds.max = max;
}

fn axis_len(min: i32, max: i32) -> Option<usize> {
    let len = i64::from(max) - i64::from(min) + 1;
    usize::try_from(len).ok()
}

fn bidirectional_a_star(
    grid: &BlockGrid,
    start: BlockPos,
    goal: BlockPos,
) -> Option<Vec<BlockPos>> {
    if !grid.is_open(start) || !grid.is_open(goal) {
        return None;
    }

    if start == goal {
        return Some(vec![start]);
    }

    let mut forward = SearchData::new(start, goal);
    let mut backward = SearchData::new(goal, start);

    while !forward.open.is_empty() && !backward.open.is_empty() {
        let expand_forward = forward.peek_priority() <= backward.peek_priority();
        let meet = if expand_forward {
            expand_search(&mut forward, &backward, grid, goal, true)
        } else {
            expand_search(&mut backward, &forward, grid, start, false)
        };

        if let Some(meet) = meet {
            return Some(reconstruct_path(meet, &forward, &backward));
        }
    }

    None
}

fn expand_search(
    search: &mut SearchData,
    other_search: &SearchData,
    grid: &BlockGrid,
    target: BlockPos,
    forward_search: bool,
) -> Option<BlockPos> {
    let current = search.pop_current()?;

    if other_search.g_score.contains_key(&current) {
        return Some(current);
    }

    let current_g = search.g_score[&current];
    for (neighbor, step_cost) in grid.neighbors(current, forward_search) {
        let tentative_g = current_g.saturating_add(step_cost);
        if tentative_g >= *search.g_score.get(&neighbor).unwrap_or(&u32::MAX) {
            continue;
        }

        search.came_from.insert(neighbor, current);
        search.g_score.insert(neighbor, tentative_g);
        search.push(neighbor, tentative_g, path_heuristic(neighbor, target));

        if other_search.g_score.contains_key(&neighbor) {
            return Some(neighbor);
        }
    }

    None
}

fn reconstruct_path(meet: BlockPos, forward: &SearchData, backward: &SearchData) -> Vec<BlockPos> {
    let mut path_to_start = vec![meet];
    let mut current = meet;
    while let Some(previous) = forward.came_from.get(&current).copied() {
        path_to_start.push(previous);
        current = previous;
    }
    path_to_start.reverse();

    current = meet;
    while let Some(next) = backward.came_from.get(&current).copied() {
        path_to_start.push(next);
        current = next;
    }

    path_to_start
}

fn movement_cost(offset: Vector3<i32>) -> u32 {
    if is_xz_diagonal_offset(offset) {
        DIAGONAL_XZ_PATH_STEP_COST
    } else {
        CARDINAL_PATH_STEP_COST
    }
}

fn is_xz_diagonal_offset(offset: Vector3<i32>) -> bool {
    offset.x != 0 && offset.z != 0
}

fn path_heuristic(a: BlockPos, b: BlockPos) -> u32 {
    let dx = a.0.x.abs_diff(b.0.x);
    let dy = a.0.y.abs_diff(b.0.y);
    let dz = a.0.z.abs_diff(b.0.z);
    let diagonal = dx.min(dz);
    let straight = dx.max(dz) - diagonal;

    DIAGONAL_XZ_PATH_STEP_COST
        .saturating_mul(diagonal)
        .saturating_add(CARDINAL_PATH_STEP_COST.saturating_mul(straight))
        .saturating_add(CARDINAL_PATH_STEP_COST.saturating_mul(dy))
}

struct SearchData {
    open: BinaryHeap<QueueEntry>,
    g_score: HashMap<BlockPos, u32>,
    came_from: HashMap<BlockPos, BlockPos>,
}

impl SearchData {
    fn new(start: BlockPos, target: BlockPos) -> Self {
        let mut search = Self {
            open: BinaryHeap::new(),
            g_score: HashMap::new(),
            came_from: HashMap::new(),
        };
        search.g_score.insert(start, 0);
        search.push(start, 0, path_heuristic(start, target));
        search
    }

    fn push(&mut self, pos: BlockPos, g: u32, h: u32) {
        self.open.push(QueueEntry {
            pos,
            g,
            h,
            f: g.saturating_add(h),
        });
    }

    fn peek_priority(&self) -> u32 {
        self.open.peek().map_or(u32::MAX, |entry| entry.f)
    }

    fn pop_current(&mut self) -> Option<BlockPos> {
        while let Some(entry) = self.open.pop() {
            if self
                .g_score
                .get(&entry.pos)
                .is_some_and(|current_g| *current_g == entry.g)
            {
                return Some(entry.pos);
            }
        }

        None
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct QueueEntry {
    pos: BlockPos,
    g: u32,
    h: u32,
    f: u32,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.cmp(&self.f).then_with(|| other.h.cmp(&self.h))
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(x, y, z)
    }

    fn open_grid(bounds: PathBounds) -> BlockGrid {
        BlockGrid::sample(bounds, |_| false).unwrap()
    }

    fn grid_with_closed(bounds: PathBounds, closed: &[BlockPos]) -> BlockGrid {
        let closed = closed.iter().copied().collect::<HashSet<_>>();
        BlockGrid::sample(bounds, |pos| closed.contains(&pos)).unwrap()
    }

    fn test_uuid(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn location_entry(uuid: Uuid, world_uuid: Uuid, x: f64, y: f64, z: f64) -> MobLocationEntry {
        MobLocationEntry {
            uuid,
            world_uuid,
            pos: Vector3::new(x, y, z),
        }
    }

    #[test]
    fn cuboid_expands_by_outset() {
        let bounds = PathBounds::between(pos(5, 3, -2), pos(8, 9, 4), 3);

        assert_eq!(bounds.min, pos(2, 0, -5));
        assert_eq!(bounds.max, pos(11, 12, 7));
    }

    #[test]
    fn outside_box_is_rejected() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0));

        assert!(grid.is_open(pos(1, 0, 0)));
        assert!(!grid.is_open(pos(1, 1, 0)));
        assert!(!grid.is_open(pos(-1, 0, 0)));
    }

    #[test]
    fn grid_sampling_tracks_closed_blocks() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0),
            &[pos(1, 0, 0)],
        );

        assert!(grid.is_open(pos(0, 0, 0)));
        assert!(!grid.is_open(pos(1, 0, 0)));
        assert!(grid.is_open(pos(2, 0, 0)));
    }

    #[test]
    fn grid_sampling_indexes_y_and_z_correctly() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(1, 1, 1), 0),
            &[pos(0, 1, 0), pos(1, 0, 1)],
        );

        assert!(!grid.is_open(pos(0, 1, 0)));
        assert!(!grid.is_open(pos(1, 0, 1)));
        assert!(grid.is_open(pos(1, 1, 0)));
        assert!(grid.is_open(pos(0, 0, 1)));
    }

    #[test]
    fn grid_sampling_rejects_oversized_boxes() {
        let bounds = PathBounds::between(pos(0, 0, 0), pos(MAX_PATH_GRID_VOLUME as i32, 0, 0), 0);

        assert!(BlockGrid::sample(bounds, |_| false).is_none());
    }

    #[test]
    fn height_difference_over_eight_skips_pathing() {
        assert!(!path_height_difference_exceeded(pos(0, 0, 0), pos(0, 8, 0)));
        assert!(path_height_difference_exceeded(pos(0, 0, 0), pos(0, 9, 0)));
        assert!(path_height_difference_exceeded(pos(0, 9, 0), pos(0, 0, 0)));
    }

    #[test]
    fn path_interval_uses_horizontal_distance_with_minimum() {
        assert_eq!(path_interval_ticks(0), 5);
        assert_eq!(path_interval_ticks(1), 5);
        assert_eq!(path_interval_ticks(2), 5);
        assert_eq!(path_interval_ticks(3), 6);
        assert_eq!(path_interval_ticks(10), 20);
    }

    #[test]
    fn horizontal_distance_ignores_y() {
        assert_eq!(horizontal_block_distance(pos(0, 0, 0), pos(3, 99, -4)), 7);
    }

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
    fn next_horizontal_step_skips_y_only_steps() {
        let path = vec![pos(0, 0, 0), pos(0, 1, 0), pos(1, 1, 0)];

        assert_eq!(
            next_horizontal_path_step(&path, pos(0, 0, 0)),
            Some(pos(1, 1, 0))
        );
    }

    #[test]
    fn next_horizontal_step_returns_none_for_y_only_path() {
        let path = vec![pos(0, 0, 0), pos(0, 1, 0), pos(0, 2, 0)];

        assert_eq!(next_horizontal_path_step(&path, pos(0, 0, 0)), None);
    }

    #[test]
    fn movement_path_steps_keep_xz_progress_and_upward_steps() {
        let path = vec![
            pos(0, 0, 0),
            pos(0, 1, 0),
            pos(1, 1, 0),
            pos(1, 2, 0),
            pos(1, 2, 1),
        ];

        assert_eq!(
            movement_path_steps(&path, pos(0, 0, 0)),
            VecDeque::from([pos(0, 1, 0), pos(1, 1, 0), pos(1, 2, 0), pos(1, 2, 1)])
        );
    }

    #[test]
    fn movement_path_steps_still_skip_downward_y_only_steps() {
        let path = vec![pos(0, 2, 0), pos(0, 1, 0), pos(1, 1, 0)];

        assert_eq!(
            movement_path_steps(&path, pos(0, 2, 0)),
            VecDeque::from([pos(1, 1, 0)])
        );
    }

    #[test]
    fn weighted_lookahead_target_weights_first_three_steps() {
        let steps = VecDeque::from([pos(1, 0, 0), pos(3, 0, 0), pos(5, 0, 0)]);
        let target = weighted_lookahead_target(&steps).unwrap();

        assert!((target.x - (17.0 / 6.0)).abs() < f64::EPSILON);
        assert_eq!(target.y, 0.0);
        assert_eq!(target.z, 0.5);
    }

    #[test]
    fn weighted_lookahead_target_reuses_first_step_for_missing_steps() {
        let steps = VecDeque::from([pos(1, 2, 3)]);
        let target = weighted_lookahead_target(&steps).unwrap();

        assert_eq!(target, block_center_feet_pos(pos(1, 2, 3)));
    }

    #[test]
    fn velocity_toward_caps_to_path_speed() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            pos(0, 0, 0),
            pos(1, 0, 0),
            1.0,
        );

        assert_eq!(
            velocity,
            Vector3::new(MOB_PATH_VELOCITY_BLOCKS_PER_TICK, 0.0, 0.0)
        );
    }

    #[test]
    fn velocity_toward_uses_remaining_delta_when_close() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.1, 0.0, 0.0),
            pos(0, 0, 0),
            pos(1, 0, 0),
            1.0,
        );

        assert_eq!(velocity, Vector3::new(0.1, 0.0, 0.0));
    }

    #[test]
    fn velocity_toward_boosts_upward_velocity_for_upward_next_step() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            pos(0, 0, 0),
            pos(0, 1, 0),
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
            pos(0, 0, 0),
            pos(0, 1, 0),
            0.0,
        );

        assert_eq!(velocity, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn velocity_toward_never_sets_negative_y_velocity() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            pos(0, 1, 0),
            pos(0, 0, 0),
            0.0,
        );

        assert_eq!(velocity, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn velocity_toward_scales_horizontal_velocity_by_movement_speed() {
        let velocity = velocity_toward(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            pos(0, 0, 0),
            pos(1, 0, 0),
            2.0,
        );

        assert_eq!(
            velocity,
            Vector3::new(MOB_PATH_VELOCITY_BLOCKS_PER_TICK * 2.0, 0.0, 0.0)
        );
    }

    #[test]
    fn velocity_with_pathfinding_delta_adds_horizontal_velocity_to_existing_velocity() {
        let velocity = velocity_with_pathfinding_delta(
            Vector3::new(0.25, -0.125, 0.5),
            Vector3::new(0.5, 0.75, -0.25),
        );

        assert_eq!(velocity, Vector3::new(0.75, 0.75, 0.25));
    }

    #[test]
    fn worker_thread_count_uses_available_logical_cores() {
        let available = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1);

        assert_eq!(cabbage_worker_thread_count_for(None), 1);
        assert_eq!(cabbage_worker_thread_count_for(Some(0)), 1);
        assert_eq!(cabbage_worker_thread_count_for(Some(4)), 4);
        assert!(cabbage_worker_thread_count() <= available);
    }

    #[test]
    fn cluster_table_finds_full_3d_cluster_with_relative_offsets() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 2.0, 0.0, 0.0),
        ]);

        let cluster = table.query_cluster(first_uuid).unwrap();

        assert_eq!(cluster.center, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(
            cluster.members,
            vec![
                MobClusterMember {
                    uuid: first_uuid,
                    offset_from_center: Vector3::new(-1.0, 0.0, 0.0),
                },
                MobClusterMember {
                    uuid: second_uuid,
                    offset_from_center: Vector3::new(1.0, 0.0, 0.0),
                },
            ]
        );
    }

    #[test]
    fn cluster_table_excludes_mobs_more_than_two_blocks_apart_vertically() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 0.0, 2.1, 0.0),
        ]);

        assert!(table.query_cluster(first_uuid).is_none());
    }

    #[test]
    fn cluster_table_excludes_mobs_more_than_two_blocks_apart_horizontally() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 2.1, 0.0, 0.0),
        ]);

        assert!(table.query_cluster(first_uuid).is_none());
    }

    #[test]
    fn cluster_table_does_not_bridge_chain_into_wide_cluster() {
        let world_uuid = test_uuid(100);
        let left_uuid = test_uuid(1);
        let middle_uuid = test_uuid(2);
        let right_uuid = test_uuid(3);
        let table = MobLocationTable::from_entries([
            location_entry(left_uuid, world_uuid, -1.5, 0.0, 0.0),
            location_entry(middle_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(right_uuid, world_uuid, 1.5, 0.0, 0.0),
        ]);

        let cluster = table.query_cluster(middle_uuid).unwrap();

        assert!(cluster.members.len() < 3);
        assert!(
            cluster
                .members
                .iter()
                .any(|member| member.uuid == middle_uuid)
        );
        for first in &cluster.members {
            for second in &cluster.members {
                assert!(
                    (first.offset_from_center - second.offset_from_center).length_squared()
                        <= CLUSTER_DIAMETER_BLOCKS * CLUSTER_DIAMETER_BLOCKS
                );
            }
        }
    }

    #[test]
    fn cluster_push_adds_outward_movement_speed_velocity() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, -1.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 1.0, 0.0, 0.0),
        ]);

        let velocity = cluster_push_velocity(first_uuid, 0.35, &table);

        assert_eq!(velocity, Vector3::new(-0.35, 0.0, 0.0));
    }

    #[test]
    fn cluster_push_clamps_negative_y_velocity() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, -0.6, -0.6, 0.0),
            location_entry(second_uuid, world_uuid, 0.6, 0.6, 0.0),
        ]);

        let velocity = cluster_push_velocity(first_uuid, 0.5, &table);

        assert!(velocity.x < 0.0);
        assert_eq!(velocity.y, 0.0);
        assert_eq!(velocity.z, 0.0);
    }

    #[test]
    fn cluster_push_uses_stable_nonzero_horizontal_fallback_for_zero_offset() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 1.0, 1.0, 1.0),
            location_entry(second_uuid, world_uuid, 1.0, 1.0, 1.0),
        ]);

        let velocity = cluster_push_velocity(first_uuid, 0.5, &table);

        assert_eq!(velocity.y, 0.0);
        assert!(velocity.horizontal_length_squared() > 0.0);
        assert!((velocity.horizontal_length() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn straight_open_path_succeeds() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(3, 0, 0), 0));
        let path = bidirectional_a_star(&grid, pos(0, 0, 0), pos(3, 0, 0)).unwrap();

        assert_eq!(path.first().copied(), Some(pos(0, 0, 0)));
        assert_eq!(path.last().copied(), Some(pos(3, 0, 0)));
        assert_eq!(path.get(1).copied(), Some(pos(1, 0, 0)));
    }

    #[test]
    fn open_square_uses_diagonal_xz_route() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(3, 0, 3), 0));
        let path = bidirectional_a_star(&grid, pos(0, 0, 0), pos(3, 0, 3)).unwrap();

        assert_eq!(
            path,
            vec![pos(0, 0, 0), pos(1, 0, 1), pos(2, 0, 2), pos(3, 0, 3)]
        );
    }

    #[test]
    fn diagonal_xz_route_does_not_cut_blocked_corners() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(1, 0, 1), 0),
            &[pos(1, 0, 0)],
        );

        assert!(
            !grid
                .neighbors(pos(0, 0, 0), true)
                .iter()
                .any(|(neighbor, _)| *neighbor == pos(1, 0, 1))
        );
    }

    #[test]
    fn solid_wall_blocks_path() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0),
            &[pos(1, 0, 0)],
        );

        assert!(bidirectional_a_star(&grid, pos(0, 0, 0), pos(2, 0, 0)).is_none());
    }

    #[test]
    fn path_cannot_leave_box() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0),
            &[pos(1, 0, 0)],
        );

        assert!(grid.is_open(pos(0, 0, 0)));
        assert!(!grid.is_open(pos(0, 1, 0)));
        assert!(bidirectional_a_star(&grid, pos(0, 0, 0), pos(2, 0, 0)).is_none());
    }

    #[test]
    fn upward_movement_cannot_skip_a_block() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(0, 2, 0), 0),
            &[pos(0, 1, 0)],
        );

        assert!(bidirectional_a_star(&grid, pos(0, 0, 0), pos(0, 2, 0)).is_none());
    }

    #[test]
    fn backward_downward_constraint_matches_forward_climb_rule() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(0, 2, 0), 0),
            &[pos(0, 1, 0)],
        );

        assert_eq!(
            grid.neighbors(pos(0, 2, 0), false),
            Vec::<(BlockPos, u32)>::new()
        );
    }

    #[test]
    fn returned_next_step_is_open_and_adjacent_to_start() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(0, 0, 2), 0));
        let path = bidirectional_a_star(&grid, pos(0, 0, 0), pos(0, 0, 2)).unwrap();
        let next = path[1];

        assert!(grid.is_open(next));
        assert_eq!(block_distance_squared(pos(0, 0, 0), next), 1);
    }
}
