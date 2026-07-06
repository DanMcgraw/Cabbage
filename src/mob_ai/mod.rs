use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use pumpkin::entity::mob::{
    Mob, creeper::CreeperEntity, skeleton::skeleton::SkeletonEntity, zombie::zombie::ZombieEntity,
};
use pumpkin::{
    entity::EntityBase,
    plugin::{
        BoxFuture, EventHandler,
        api::events::entity::{
            ChunkEntityLoadEvent, ChunkEntityUnloadEvent, EntityRemoveEvent, EntitySpawnEvent,
        },
        server::server_tick_start::ServerTickStartEvent,
    },
    server::Server,
    world::World,
};
use pumpkin_data::attributes::Attributes;
use pumpkin_util::math::{position::BlockPos, vector2::Vector2, vector3::Vector3};
use rayon::{ThreadPool, ThreadPoolBuilder};
use uuid::Uuid;

pub(crate) mod clustering;
pub(crate) mod movement;
pub(crate) mod pathfinding;
pub(crate) mod types;
pub(crate) mod workers;

use movement::weighted_lookahead_target;
use pathfinding::{
    BlockGrid, PathBounds, PlayerSearchTree, block_distance_squared, generate_player_tree,
    horizontal_block_distance, path_height_difference_exceeded, path_interval_ticks,
};
use types::{
    ActiveMobSnapshot, MobLocationEntry, MobLocationTable, PathVelocityTarget, VelocityPlan,
};
use workers::cabbage_worker_thread_count;

const PATH_BOX_OUTSET_BLOCKS: i32 = 3;

const MOB_JUMP_TYPES: [&str; 3] = ["zombie", "skeleton", "creeper"];
/// Number of ticks to skip a newly-loaded entity before touching its AI
/// structures. Entities spawned from chunk loading may not have their
/// goals/navigator/look-control fully initialised for the first few ticks.
const ENTITY_LOAD_GRACE_TICKS: i32 = 40;

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
    /// Managed mob UUIDs tracked via entity lifecycle events and tick discovery.
    pub(crate) managed_mobs: Mutex<HashSet<Uuid>>,
    pub(crate) disabled_mobs: Mutex<HashSet<Uuid>>,
    pub(crate) frozen_out_of_bounds_mobs: Mutex<HashSet<Uuid>>,
    /// Tick at which each entity was first observed. Entities inside their
    /// grace window are completely skipped by the AI loop.
    pub(crate) grace_period_mobs: Mutex<HashMap<Uuid, i32>>,
    /// Cached (start_block, goal_block) that produced the current path_steps
    /// entry.  Used to skip redundant A* jobs when neither endpoint moved.
    pub(crate) path_endpoints: Mutex<HashMap<Uuid, (BlockPos, BlockPos)>>,
    /// Cached Dijkstra search trees per player. Shared by nearby mobs.
    pub(crate) player_trees: Arc<Mutex<HashMap<Uuid, Arc<PlayerSearchTree>>>>,
    /// Last computed block position of players to detect movement.
    pub(crate) last_player_positions: Mutex<HashMap<Uuid, BlockPos>>,
    /// Last applied rotation yaw based on planned velocity.
    pub(crate) last_applied_yaws: Mutex<HashMap<Uuid, f32>>,
}

pub struct MobAiMetrics {
    pub active_path_jobs: usize,
    pub active_velocity_jobs: usize,
    pub total_worker_threads: usize,
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
            managed_mobs: Mutex::new(HashSet::new()),
            disabled_mobs: Mutex::new(HashSet::new()),
            frozen_out_of_bounds_mobs: Mutex::new(HashSet::new()),
            grace_period_mobs: Mutex::new(HashMap::new()),
            path_endpoints: Mutex::new(HashMap::new()),
            player_trees: Arc::new(Mutex::new(HashMap::new())),
            last_player_positions: Mutex::new(HashMap::new()),
            last_applied_yaws: Mutex::new(HashMap::new()),
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

impl EventHandler<EntitySpawnEvent> for MobAiState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntitySpawnEvent,
    ) -> BoxFuture<'a, ()> {
        self.register_managed_mob(event.world.uuid, event.entity.as_ref());
        Box::pin(async {})
    }

    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a mut EntitySpawnEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

impl EventHandler<EntityRemoveEvent> for MobAiState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityRemoveEvent,
    ) -> BoxFuture<'a, ()> {
        self.unregister_managed_mob(event.entity.get_entity().entity_uuid);
        Box::pin(async {})
    }

    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a mut EntityRemoveEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

impl EventHandler<ChunkEntityLoadEvent> for MobAiState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ChunkEntityLoadEvent,
    ) -> BoxFuture<'a, ()> {
        self.register_managed_mob(event.world.uuid, event.entity.as_ref());
        Box::pin(async {})
    }

    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a mut ChunkEntityLoadEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

impl EventHandler<ChunkEntityUnloadEvent> for MobAiState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ChunkEntityUnloadEvent,
    ) -> BoxFuture<'a, ()> {
        self.unregister_managed_mob(event.entity.get_entity().entity_uuid);
        Box::pin(async {})
    }

    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a mut ChunkEntityUnloadEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
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

            let mut active_mobs = Vec::new();

            // Lock all needed state maps once here to avoid locking/unlocking thousands of times in the loops!
            let mut last_player_positions = self.last_player_positions.lock().unwrap();
            let mut last_path_ticks = self.last_path_ticks.lock().unwrap();
            let mut active_path_jobs = self.active_path_jobs.lock().unwrap();
            let mut path_steps = self.path_steps.lock().unwrap();
            let mut planned_velocities = self.planned_velocities.lock().unwrap();
            let mut managed_mobs = self.managed_mobs.lock().unwrap();
            let mut disabled_mobs = self.disabled_mobs.lock().unwrap();
            let mut frozen_out_of_bounds = self.frozen_out_of_bounds_mobs.lock().unwrap();
            let mut grace_period = self.grace_period_mobs.lock().unwrap();
            let mut last_applied_yaws = self.last_applied_yaws.lock().unwrap();
            let mut path_endpoints = self.path_endpoints.lock().unwrap();
            let mut player_trees = self.player_trees.lock().unwrap();

            // Calculate actual tick period dynamically based on the number of currently managed mobs
            let managed_count = managed_mobs.len();
            let actual_period = if managed_count <= 600 {
                4
            } else {
                let excess = (managed_count as f64 - 600.0) / 300.0;
                let multiplier = 1.0 + 0.3 * excess;
                (4.0 * multiplier).round() as i32
            };
            let should_update_velocity = tick % actual_period == 0;

            struct PathJobToSpawn {
                uuid: Uuid,
                grid: BlockGrid,
                mob_pos: BlockPos,
                target_pos: BlockPos,
                player_tree: Option<Arc<PlayerSearchTree>>,
                distance_to_player: f64,
            }
            let mut path_jobs_to_spawn = Vec::new();

            // 1. Prioritized Player Tree Pre-Computation (Asynchronous & Non-blocking)
            let mut players_to_update = Vec::new();
            let mut active_player_uuids = HashSet::new();
            for world in server.worlds.load().iter() {
                for player in world.players.load().iter() {
                    let player_uuid = player.get_entity().entity_uuid;
                    active_player_uuids.insert(player_uuid);
                    let player_pos = player.get_entity().pos.load();
                    let player_block = BlockPos::floored_v(player_pos);

                    let needs_update = if last_player_positions.get(&player_uuid) != Some(&player_block) {
                        last_player_positions.insert(player_uuid, player_block);
                        true
                    } else {
                        false
                    };

                    if needs_update {
                        players_to_update.push((player_uuid, player_block, world.clone()));
                    }
                }
            }

            if !players_to_update.is_empty() {
                for (player_uuid, player_block, world) in players_to_update {
                    let player_trees_clone = Arc::clone(&self.player_trees);
                    self.worker_pool.spawn(move || {
                        // bounds region: +-6 horizontally, +-4 vertically
                        let bounds = PathBounds {
                            min: player_block.add(-6, -4, -6),
                            max: player_block.add(6, 4, 6),
                        };
                        if let Some(grid) = BlockGrid::sample(bounds, |pos| {
                            world.get_block_state(&pos).is_solid_block()
                        }) {
                            let tree = generate_player_tree(&grid, player_block);
                            player_trees_clone
                                .lock()
                                .unwrap()
                                .insert(player_uuid, Arc::new(tree));
                        }
                    });
                }
            }

            for world in server.worlds.load().iter() {
                if world.players.load().is_empty() {
                    continue;
                }

                // 2. Build the list of watched chunks (3x3 around each player)
                let mut watched_chunks = HashSet::new();
                for player in world.players.load().iter() {
                    let center = player.get_entity().chunk_pos.load();
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            watched_chunks.insert(Vector2::new(center.x + dx, center.y + dz));
                        }
                    }
                }

                // 3. Directly sample entities ONLY in the watched chunks
                let mut active_in_tick_mobs = HashSet::new();
                let mut entities_to_process = Vec::new();

                for chunk_pos in &watched_chunks {
                    if let Some(chunk_entities) = world.entities_by_chunk.get(chunk_pos) {
                        for entity_base in chunk_entities.iter() {
                            let entity = entity_base.get_entity();
                            if MOB_JUMP_TYPES.contains(&entity.entity_type.resource_name.as_ref()) {
                                entities_to_process.push(entity_base.clone());
                                active_in_tick_mobs.insert(entity.entity_uuid);
                            }
                        }
                    }
                }

                // 4. Process the active nearby mobs
                for entity_base in entities_to_process {
                    let entity = entity_base.get_entity();
                    let uuid = entity.entity_uuid;

                    // Ensure this mob is tracked.
                    if managed_mobs.insert(uuid) {
                        clear_pumpkin_mob_ai(entity_base.as_ref());
                    }

                    // Unfreeze mob if it was previously frozen
                    frozen_out_of_bounds.remove(&uuid);

                    // Grace period: skip entities that just appeared until their
                    // AI sub-systems have had time to fully initialise.
                    {
                        let first_seen = grace_period.entry(uuid).or_insert(tick);
                        if tick.wrapping_sub(*first_seen) < ENTITY_LOAD_GRACE_TICKS {
                            continue;
                        }
                    }

                    // Fix 5: Apply completed velocity plan BEFORE snapshotting
                    if should_update_velocity {
                        let plan = planned_velocities.remove(&uuid);
                        if let Some(plan) = plan {
                            if let Some(body_yaw) = plan.target_yaw {
                                entity.yaw.store(body_yaw);
                                entity.body_yaw.store(body_yaw);
                                entity.head_yaw.store(body_yaw);
                                entity.send_rotation();
                                last_applied_yaws.insert(uuid, body_yaw);
                            }
                            entity.velocity.store(plan.velocity);
                        }
                    }

                    let current_pos = entity.pos.load();
                    let mob_pos = BlockPos::floored_v(current_pos);
                    let current_velocity = entity.velocity.load();
                    let Some((player_uuid, target_pos, player_eye_pos, horizontal_distance)) =
                        nearest_player_pos(world, mob_pos)
                    else {
                        continue;
                    };

                    update_pumpkin_look_target(entity_base.as_ref(), player_eye_pos);

                    if path_height_difference_exceeded(mob_pos, target_pos) {
                        path_steps.remove(&uuid);
                        path_endpoints.remove(&uuid);
                        continue;
                    }

                    let Some(living_entity) = entity_base.get_living_entity() else {
                        continue;
                    };

                    let path_target = if should_update_velocity {
                        let target = if let Some(steps) = path_steps.get_mut(&uuid) {
                            while steps.front().is_some_and(|step| *step == mob_pos) {
                                steps.pop_front();
                            }
                            if steps.is_empty() {
                                path_steps.remove(&uuid);
                                None
                            } else if let Some(next_step) = steps.front().copied() {
                                weighted_lookahead_target(steps).map(|target| (target, next_step))
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        target.and_then(|(target_pos, next_step)| {
                            if world.get_block_state(&next_step).is_solid_block() {
                                path_steps.remove(&uuid);
                                path_endpoints.remove(&uuid);
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

                    let dist_3d = current_pos.squared_distance_to_vec(&player_eye_pos);
                    active_mobs.push(ActiveMobSnapshot {
                        uuid,
                        world_uuid: world.uuid,
                        current_pos,
                        current_block: mob_pos,
                        current_velocity,
                        movement_speed: living_entity
                            .get_attribute_value(&Attributes::MOVEMENT_SPEED),
                        path_target,
                        player_distance: dist_3d,
                    });

                    // Ensure the entity faces the direction of travel based on the last velocity it was given
                    if let Some(&last_yaw) = last_applied_yaws.get(&uuid) {
                        entity.yaw.store(last_yaw);
                        entity.body_yaw.store(last_yaw);
                        entity.head_yaw.store(last_yaw);
                        entity.send_rotation();
                    }

                    let interval = path_interval_ticks(horizontal_distance, active_mobs.len());

                    let mut should_start = false;
                    if active_path_jobs.insert(uuid) {
                        let should_attempt = last_path_ticks
                            .get(&uuid)
                            .is_none_or(|last_tick| i64::from(tick) - i64::from(*last_tick) >= interval);
                        if should_attempt {
                            last_path_ticks.insert(uuid, tick);
                            should_start = true;
                        } else {
                            active_path_jobs.remove(&uuid);
                        }
                    }
                    if !should_start {
                        continue;
                    }

                    // Reuse the existing path when neither endpoint has changed.
                    let mut can_reuse = false;
                    if let Some(&(cached_start, cached_goal)) = path_endpoints.get(&uuid) {
                        if cached_start == mob_pos && cached_goal == target_pos {
                            can_reuse = path_steps.contains_key(&uuid);
                        }
                    }
                    if can_reuse {
                        active_path_jobs.remove(&uuid);
                        continue;
                    }

                    let bounds = PathBounds::between(mob_pos, target_pos, PATH_BOX_OUTSET_BLOCKS);
                    let Some(grid) = BlockGrid::sample(bounds, |pos| {
                        world.get_block_state(&pos).is_solid_block()
                    }) else {
                        active_path_jobs.remove(&uuid);
                        continue;
                    };

                    let player_tree = player_trees.get(&player_uuid).cloned();

                    path_jobs_to_spawn.push(PathJobToSpawn {
                        uuid,
                        grid,
                        mob_pos,
                        target_pos,
                        player_tree,
                        distance_to_player: dist_3d,
                    });
                }

                // 5. Freeze newly out-of-bounds managed mobs
                let managed = managed_mobs.clone();
                for uuid in managed {
                    if !active_in_tick_mobs.contains(&uuid) {
                        let is_newly_frozen = frozen_out_of_bounds.insert(uuid);
                        if is_newly_frozen {
                            if let Some(entity_base) = world.get_entity_by_uuid(uuid) {
                                if disabled_mobs.insert(uuid) {
                                    clear_pumpkin_mob_ai(entity_base.as_ref());
                                }
                                let entity = entity_base.get_entity();
                                if entity.velocity.load() != Vector3::default() {
                                    entity.set_velocity(Vector3::default());
                                }
                            }
                        }
                    }
                }
            }

            // Inline retain_player_state using active guards BEFORE dropping them
            player_trees.retain(|uuid, _| active_player_uuids.contains(uuid));
            last_player_positions.retain(|uuid, _| active_player_uuids.contains(uuid));

            // Drop all locks explicitly to prevent deadlocks when calling helper methods that lock
            drop(last_player_positions);
            drop(last_path_ticks);
            drop(active_path_jobs);
            drop(path_steps);
            drop(planned_velocities);
            drop(managed_mobs);
            drop(disabled_mobs);
            drop(frozen_out_of_bounds);
            drop(grace_period);
            drop(last_applied_yaws);
            drop(path_endpoints);
            drop(player_trees);

            // Prioritize and spawn pathfinding jobs closest to the player first
            path_jobs_to_spawn.sort_by(|a, b| {
                a.distance_to_player
                    .partial_cmp(&b.distance_to_player)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for job in path_jobs_to_spawn {
                self.spawn_path_job(
                    job.uuid,
                    job.grid,
                    job.mob_pos,
                    job.target_pos,
                    job.player_tree,
                );
            }

            if should_update_velocity {
                self.update_mob_locations(&active_mobs);

                // Prioritize velocity planning updates closest to the player first
                let mut sorted_active_mobs = active_mobs.clone();
                sorted_active_mobs.sort_by(|a, b| {
                    a.player_distance
                        .partial_cmp(&b.player_distance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                self.spawn_velocity_jobs(&sorted_active_mobs);
            }
        })
    }
}

impl MobAiState {
    fn update_mob_locations(&self, active_mobs: &[ActiveMobSnapshot]) {
        let entries = active_mobs.iter().map(|mob| MobLocationEntry {
            uuid: mob.uuid,
            world_uuid: mob.world_uuid,
            pos: mob.current_pos,
        });

        *self.mob_locations.lock().unwrap() = MobLocationTable::from_entries(entries);
    }





    fn ensure_pumpkin_mob_ai_disabled(&self, entity_base: &dyn EntityBase, uuid: Uuid) -> bool {
        let is_newly_disabled = self.disabled_mobs.lock().unwrap().insert(uuid);
        if is_newly_disabled {
            clear_pumpkin_mob_ai(entity_base);
        }
        is_newly_disabled
    }

    fn register_managed_mob(&self, _world_uuid: Uuid, entity_base: &dyn EntityBase) {
        let entity = entity_base.get_entity();
        if !MOB_JUMP_TYPES.contains(&entity.entity_type.resource_name) {
            return;
        }

        let is_new = self.managed_mobs.lock().unwrap().insert(entity.entity_uuid);
        if is_new {
            self.ensure_pumpkin_mob_ai_disabled(entity_base, entity.entity_uuid);
        }
    }

    fn unregister_managed_mob(&self, uuid: Uuid) {
        self.managed_mobs.lock().unwrap().remove(&uuid);
        self.last_path_ticks.lock().unwrap().remove(&uuid);
        self.path_steps.lock().unwrap().remove(&uuid);
        self.path_endpoints.lock().unwrap().remove(&uuid);
        self.planned_velocities.lock().unwrap().remove(&uuid);
        self.disabled_mobs.lock().unwrap().remove(&uuid);
        self.frozen_out_of_bounds_mobs.lock().unwrap().remove(&uuid);
        self.grace_period_mobs.lock().unwrap().remove(&uuid);
        self.last_applied_yaws.lock().unwrap().remove(&uuid);
    }



    pub fn get_metrics(&self) -> MobAiMetrics {
        MobAiMetrics {
            active_path_jobs: self.active_path_jobs.lock().unwrap().len(),
            active_velocity_jobs: self.active_velocity_jobs.lock().unwrap().len(),
            total_worker_threads: self.worker_pool.current_num_threads(),
            managed_mobs_count: self.managed_mobs.lock().unwrap().len(),
            total_paths_completed: self
                .paths_completed
                .load(std::sync::atomic::Ordering::Relaxed),
            total_velocities_completed: self
                .velocities_completed
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

fn nearest_player_pos(
    world: &World,
    mob_pos: BlockPos,
) -> Option<(Uuid, BlockPos, Vector3<f64>, i64)> {
    world
        .players
        .load()
        .iter()
        .map(|player| {
            let player_entity = player.get_entity();
            let player_uuid = player_entity.entity_uuid;
            let player_block_pos = BlockPos::floored_v(player_entity.pos.load());
            let distance_squared = block_distance_squared(mob_pos, player_block_pos);
            let horizontal_distance = horizontal_block_distance(mob_pos, player_block_pos);
            let player_eye_pos = player_entity.get_eye_pos();
            (
                player_uuid,
                player_block_pos,
                player_eye_pos,
                distance_squared,
                horizontal_distance,
            )
        })
        .min_by_key(|(_, _, _, distance_squared, _)| *distance_squared)
        .map(
            |(player_uuid, player_block_pos, player_eye_pos, _, horizontal_distance)| {
                (
                    player_uuid,
                    player_block_pos,
                    player_eye_pos,
                    horizontal_distance,
                )
            },
        )
}

fn get_mob_helper(entity_base: &dyn EntityBase) -> Option<&dyn Mob> {
    if let Some(creeper) = entity_base.cast_any().downcast_ref::<CreeperEntity>() {
        return Some(creeper as &dyn Mob);
    }
    if let Some(skeleton) = entity_base.cast_any().downcast_ref::<SkeletonEntity>() {
        return Some(skeleton as &dyn Mob);
    }
    if let Some(zombie) = entity_base.cast_any().downcast_ref::<ZombieEntity>() {
        return Some(zombie as &dyn Mob);
    }
    None
}

fn update_pumpkin_look_target(entity_base: &dyn EntityBase, target_pos: Vector3<f64>) {
    if let Some(mob) = get_mob_helper(entity_base) {
        let mob_entity = mob.get_mob_entity();
        let mut look_control = mob_entity.look_control.lock().unwrap();
        look_control.look_at_position(mob, target_pos);
    }
}

fn clear_pumpkin_mob_ai(entity_base: &dyn EntityBase) {
    if let Some(mob) = get_mob_helper(entity_base) {
        let mob_entity = mob.get_mob_entity();
        mob_entity.set_no_ai(false);
        *mob_entity.goals_selector.lock().unwrap() =
            pumpkin::entity::ai::goal::goal_selector::GoalSelector::default();
        *mob_entity.target_selector.lock().unwrap() =
            pumpkin::entity::ai::goal::goal_selector::GoalSelector::default();
        if let Ok(mut target) = mob_entity.target.try_lock() {
            *target = None;
        }
        mob_entity.navigator.lock().unwrap().stop();
        *mob_entity.look_control.lock().unwrap() =
            pumpkin::entity::ai::control::look_control::LookControl::default();
        *mob_entity.move_control.lock().unwrap() =
            Box::new(pumpkin::entity::ai::control::move_control::MoveControl::default());

        let living_entity = &mob_entity.living_entity;
        living_entity.movement_input.store(Vector3::default());
        living_entity
            .jumping
            .store(false, std::sync::atomic::Ordering::SeqCst);
        living_entity
            .jumping_cooldown
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }
}
