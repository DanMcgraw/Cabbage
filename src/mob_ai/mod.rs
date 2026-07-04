use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use pumpkin::{
    entity::EntityBase,
    plugin::{BoxFuture, EventHandler, server::server_tick_start::ServerTickStartEvent},
    server::Server,
    world::World,
};
use pumpkin_data::attributes::Attributes;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3, vector2::Vector2};
use rayon::{ThreadPool, ThreadPoolBuilder};
use uuid::Uuid;

pub(crate) mod types;
pub(crate) mod pathfinding;
pub(crate) mod movement;
pub(crate) mod clustering;
pub(crate) mod workers;

use types::{ActiveMobSnapshot, PathVelocityTarget, VelocityPlan, MobLocationEntry, MobLocationTable};
use pathfinding::{
    BlockGrid, PathBounds, path_interval_ticks, path_height_difference_exceeded,
    block_distance_squared, horizontal_block_distance,
};
use movement::{point_body_along_velocity, weighted_lookahead_target};
use workers::cabbage_worker_thread_count;

const PATH_BOX_OUTSET_BLOCKS: i32 = 3;

const MOB_JUMP_TYPES: [&str; 3] = ["zombie", "skeleton", "creeper"];
const MOB_MOVE_PERIOD_TICKS: i32 = 4;

pub(crate) struct MobAiState {
    pub(crate) worker_pool: Arc<ThreadPool>,
    pub(crate) last_path_ticks: Mutex<HashMap<Uuid, i32>>,
    pub(crate) active_path_jobs: Arc<Mutex<HashSet<Uuid>>>,
    pub(crate) path_steps: Arc<Mutex<HashMap<Uuid, VecDeque<BlockPos>>>>,
    pub(crate) active_velocity_jobs: Arc<Mutex<HashSet<Uuid>>>,
    pub(crate) planned_velocities: Arc<Mutex<HashMap<Uuid, VelocityPlan>>>,
    pub(crate) mob_locations: Mutex<MobLocationTable>,
    pub(crate) paths_completed: Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) velocities_completed: Arc<std::sync::atomic::AtomicUsize>,
}

pub struct MobAiMetrics {
    pub active_path_jobs: usize,
    pub active_velocity_jobs: usize,
    pub total_worker_threads: usize,
    pub path_steps_cached: usize,
    pub planned_velocities_cached: usize,
    pub managed_mobs_count: usize,
    pub total_paths_completed: usize,
    pub total_velocities_completed: usize,
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
            paths_completed: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            velocities_completed: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}

impl EventHandler<ServerTickStartEvent> for MobAiState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        // Fix 1: Do not call run_tick from the non-blocking handler.
        // run_tick touches live Pumpkin world/entity state, which is only safe on
        // the game thread. The non-blocking handler may run on an async executor
        // thread, causing access violations (error 0x05).
        Box::pin(async {})
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
            log::trace!(
                "MobAiState::run_tick tick={} thread={:?}",
                tick,
                std::thread::current().name()
            );

            let mut seen_mobs = HashSet::new();
            let mut active_mobs = Vec::new();
            let should_update_velocity = tick % MOB_MOVE_PERIOD_TICKS == 0;

            for world in server.worlds.load().iter() {
                if world.players.load().is_empty() {
                    continue;
                }

                let mut watched_chunks = HashSet::new();
                for player in world.players.load().iter() {
                    let center = player.get_entity().chunk_pos.load();
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            watched_chunks.insert(Vector2::new(center.x + dx, center.y + dz));
                        }
                    }
                }

                for entity_base in world.entities.load().iter() {
                    let entity = entity_base.get_entity();

                    if !MOB_JUMP_TYPES.contains(&entity.entity_type.resource_name) {
                        continue;
                    }

                    // Skip mobs outside the 3x3 chunk area around players.
                    // Skipping them prevents seen_mobs insertion, which automatically
                    // flushes them from pathing, velocity plan, and search interval caches.
                    let chunk_pos = entity.chunk_pos.load();
                    if !watched_chunks.contains(&chunk_pos) {
                        continue;
                    }

                    seen_mobs.insert(entity.entity_uuid);

                    // Fix 5: Apply completed velocity plan BEFORE snapshotting
                    // so the snapshot reflects post-apply state for the next
                    // worker job, avoiding stale velocity data.
                    if should_update_velocity {
                        self.apply_planned_velocity(entity_base.as_ref(), entity.entity_uuid);
                    }

                    let current_pos = entity.pos.load();
                    let mob_pos = BlockPos::floored_v(current_pos);
                    let current_velocity = entity.velocity.load();
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

                    active_mobs.push(ActiveMobSnapshot {
                        uuid: entity.entity_uuid,
                        world_uuid: world.uuid,
                        current_pos,
                        current_block: mob_pos,
                        current_velocity,
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

    fn update_mob_locations(&self, active_mobs: &[ActiveMobSnapshot]) {
        let entries = active_mobs.iter().map(|mob| MobLocationEntry {
            uuid: mob.uuid,
            world_uuid: mob.world_uuid,
            pos: mob.current_pos,
        });

        *self.mob_locations.lock().unwrap() = MobLocationTable::from_entries(entries);
    }

    fn apply_planned_velocity(&self, entity_base: &dyn EntityBase, uuid: Uuid) {
        // Fix 6: Extract the plan under the lock and immediately release it.
        // No local mutex is held while mutating live Pumpkin entity state.
        let plan = {
            let mut planned_velocities = self.planned_velocities.lock().unwrap();
            planned_velocities.remove(&uuid)
        };

        let Some(plan) = plan else {
            return;
        };

        let entity = entity_base.get_entity();
        point_body_along_velocity(entity, plan.steering_delta);
        entity.velocity.store(plan.velocity);
        entity
            .velocity_dirty
            .store(true, std::sync::atomic::Ordering::SeqCst);
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
        // Fix 3: Only prune cache/result maps, NOT active job ownership sets.
        // Worker jobs own their active marker lifecycle via ActiveJobGuard.
        // If a mob disappears while a job is running, the job finishes and
        // the result gets ignored or pruned because the mob is no longer seen.
        // This preserves the one-active-job-per-UUID deduplication invariant.
        self.last_path_ticks
            .lock()
            .unwrap()
            .retain(|uuid, _| seen_mobs.contains(uuid));

        self.path_steps
            .lock()
            .unwrap()
            .retain(|uuid, _| seen_mobs.contains(uuid));

        self.planned_velocities
            .lock()
            .unwrap()
            .retain(|uuid, _| seen_mobs.contains(uuid));
    }

    pub fn get_metrics(&self) -> MobAiMetrics {
        MobAiMetrics {
            active_path_jobs: self.active_path_jobs.lock().unwrap().len(),
            active_velocity_jobs: self.active_velocity_jobs.lock().unwrap().len(),
            total_worker_threads: self.worker_pool.current_num_threads(),
            path_steps_cached: self.path_steps.lock().unwrap().len(),
            planned_velocities_cached: self.planned_velocities.lock().unwrap().len(),
            managed_mobs_count: self.last_path_ticks.lock().unwrap().len(),
            total_paths_completed: self.paths_completed.load(std::sync::atomic::Ordering::Relaxed),
            total_velocities_completed: self.velocities_completed.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
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
