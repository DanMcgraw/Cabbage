use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
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
};
use pumpkin_data::attributes::Attributes;
use pumpkin_util::math::{position::BlockPos, vector2::Vector2, vector3::Vector3, wrap_degrees};
use rayon::{ThreadPool, ThreadPoolBuilder};
use uuid::Uuid;

pub use cabbage_api::MobAiMetricsSnapshot as MobAiMetrics;

pub(crate) mod clustering;
pub(crate) mod movement;
pub(crate) mod pathfinding;
pub(crate) mod types;
pub(crate) mod workers;

use pathfinding::{
    BlockGrid, PathBounds, PlayerSearchTree, block_distance_squared, generate_player_tree,
    horizontal_block_distance, path_height_difference_exceeded, path_interval_ticks,
};
use types::{ActiveMobSnapshot, MobLocationEntry, MobLocationTable};
use workers::cabbage_worker_thread_count;

const PATH_BOX_OUTSET_BLOCKS: i32 = 3;

const MOB_JUMP_TYPES: [&str; 3] = ["zombie", "skeleton", "creeper"];
/// Number of ticks to skip a newly-loaded entity before touching its AI
/// structures. Entities spawned from chunk loading may not have their
/// goals/navigator/look-control fully initialised for the first few ticks.
const ENTITY_LOAD_GRACE_TICKS: i32 = 40;

pub struct MobAiState {
    pub(crate) worker_pool: Arc<ThreadPool>,
    pub(crate) last_path_ticks: Mutex<HashMap<Uuid, i32>>,
    pub(crate) active_path_jobs: Arc<Mutex<HashSet<Uuid>>>,
    pub(crate) path_steps: Arc<Mutex<HashMap<Uuid, VecDeque<BlockPos>>>>,
    pub(crate) mob_locations: Mutex<MobLocationTable>,
    pub(crate) paths_completed: Arc<std::sync::atomic::AtomicUsize>,
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
    pub(crate) active_mobs_count: std::sync::atomic::AtomicUsize,
    pub(crate) chunk_registry_read: types::ChunkRegistryRead,
    pub(crate) chunk_registry_write: Arc<Mutex<types::ChunkRegistryWrite>>,
    pub mob_ai_enabled: std::sync::atomic::AtomicBool,
}

impl Default for MobAiState {
    fn default() -> Self {
        let worker_threads = cabbage_worker_thread_count();
        let worker_pool = ThreadPoolBuilder::new()
            .num_threads(worker_threads)
            .thread_name(|index| format!("cabbage-mob-ai-{index}"))
            .build()
            .expect("failed to build Cabbage mob AI worker pool");

        let (chunk_registry_write, chunk_registry_read) = flashmap::new();

        Self {
            worker_pool: Arc::new(worker_pool),
            last_path_ticks: Mutex::new(HashMap::new()),
            active_path_jobs: Arc::new(Mutex::new(HashSet::new())),
            path_steps: Arc::new(Mutex::new(HashMap::new())),
            mob_locations: Mutex::new(MobLocationTable::default()),
            paths_completed: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            managed_mobs: Mutex::new(HashSet::new()),
            disabled_mobs: Mutex::new(HashSet::new()),
            frozen_out_of_bounds_mobs: Mutex::new(HashSet::new()),
            grace_period_mobs: Mutex::new(HashMap::new()),
            path_endpoints: Mutex::new(HashMap::new()),
            player_trees: Arc::new(Mutex::new(HashMap::new())),
            last_player_positions: Mutex::new(HashMap::new()),
            active_mobs_count: std::sync::atomic::AtomicUsize::new(0),
            chunk_registry_read,
            chunk_registry_write: Arc::new(Mutex::new(chunk_registry_write)),
            mob_ai_enabled: std::sync::atomic::AtomicBool::new(true),
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
            if !self
                .mob_ai_enabled
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                return;
            }
            log::trace!(
                "MobAiState::run_tick tick={} thread={:?}",
                tick,
                std::thread::current().name()
            );

            // Periodically sweep registry to remove chunks that are no longer loaded in any world
            if tick % 100 == 0 {
                let mut write = self.chunk_registry_write.lock().unwrap();
                let mut to_remove = Vec::new();
                {
                    let guard = self.chunk_registry_read.guard();
                    for (&(cx, cz), _) in guard.iter() {
                        let chunk_pos = Vector2::new(cx, cz);
                        let mut loaded = false;
                        for world in server.worlds.load().iter() {
                            if world.level.is_chunk_loaded(&chunk_pos) {
                                loaded = true;
                                break;
                            }
                        }
                        if !loaded {
                            to_remove.push((cx, cz));
                        }
                    }
                }
                if !to_remove.is_empty() {
                    let mut write_guard = write.guard();
                    for key in to_remove {
                        write_guard.remove(key);
                    }
                    write_guard.publish();
                }
            }

            let mut active_mobs = Vec::new();

            // Lock all needed state maps once here to avoid locking/unlocking thousands of times in the loops!
            let mut last_player_positions = self.last_player_positions.lock().unwrap();
            let mut last_path_ticks = self.last_path_ticks.lock().unwrap();
            let mut active_path_jobs = self.active_path_jobs.lock().unwrap();
            let mut path_steps = self.path_steps.lock().unwrap();
            let mut managed_mobs = self.managed_mobs.lock().unwrap();
            let mut disabled_mobs = self.disabled_mobs.lock().unwrap();
            let mut frozen_out_of_bounds = self.frozen_out_of_bounds_mobs.lock().unwrap();
            let mut grace_period = self.grace_period_mobs.lock().unwrap();
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

                    let needs_update =
                        if last_player_positions.get(&player_uuid) != Some(&player_block) {
                            last_player_positions.insert(player_uuid, player_block);
                            true
                        } else {
                            false
                        };

                    if needs_update {
                        players_to_update.push((player_uuid, player_block));
                    }
                }
            }

            if !players_to_update.is_empty() {
                for (player_uuid, player_block) in players_to_update {
                    let player_trees_clone = Arc::clone(&self.player_trees);
                    let bounds = PathBounds {
                        min: player_block.add(-6, -4, -6),
                        max: player_block.add(6, 4, 6),
                    };

                    let chunk_registry = self.chunk_registry_read.clone();
                    self.worker_pool.spawn(move || {
                        if let Some(grid) = BlockGrid::sample_registry(bounds, &chunk_registry) {
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
                let players: Vec<PlayerSnapshot> = world
                    .players
                    .load()
                    .iter()
                    .map(|player| {
                        let player_entity = player.get_entity();
                        PlayerSnapshot {
                            uuid: player_entity.entity_uuid,
                            block_pos: BlockPos::floored_v(player_entity.pos.load()),
                            eye_pos: player_entity.get_eye_pos(),
                            chunk_pos: player_entity.chunk_pos.load(),
                        }
                    })
                    .collect();

                if players.is_empty() {
                    continue;
                }

                // 2. Build the list of watched chunks (3x3 around each player)
                let mut watched_chunks = HashSet::new();
                for player in &players {
                    let center = player.chunk_pos;
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
                            let resource_name = entity.entity_type.resource_name.as_ref();

                            if MOB_JUMP_TYPES.contains(&resource_name) {
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

                    let current_pos = entity.pos.load();
                    let mob_pos = BlockPos::floored_v(current_pos);
                    let Some((player_uuid, target_pos, player_eye_pos, horizontal_distance)) =
                        nearest_player_pos(&players, mob_pos)
                    else {
                        if let Some(living_entity) = entity_base.get_living_entity() {
                            living_entity.movement_input.store(Vector3::default());
                            living_entity
                                .jumping
                                .store(false, std::sync::atomic::Ordering::SeqCst);
                        }
                        continue;
                    };

                    update_pumpkin_look_target(entity_base.as_ref(), player_eye_pos);

                    if path_height_difference_exceeded(mob_pos, target_pos) {
                        path_steps.remove(&uuid);
                        path_endpoints.remove(&uuid);
                        if let Some(living_entity) = entity_base.get_living_entity() {
                            living_entity.movement_input.store(Vector3::default());
                            living_entity
                                .jumping
                                .store(false, std::sync::atomic::Ordering::SeqCst);
                        }
                        continue;
                    }

                    let Some(living_entity) = entity_base.get_living_entity() else {
                        continue;
                    };

                    living_entity.fall_distance.store(0.0);

                    let speed = living_entity.get_attribute_value(&Attributes::MOVEMENT_SPEED);
                    let mut path_target = None;
                    if let Some(steps) = path_steps.get_mut(&uuid) {
                        while let Some(front_step) = steps.front() {
                            let target_pos = Vector3::new(
                                front_step.0.x as f64 + 0.5,
                                front_step.0.y as f64,
                                front_step.0.z as f64 + 0.5,
                            );
                            let dist = (target_pos - current_pos).length();
                            if dist < 0.15 {
                                steps.pop_front();
                            } else {
                                if world.get_block_state(front_step).is_solid_block() {
                                    path_steps.remove(&uuid);
                                    path_endpoints.remove(&uuid);
                                } else {
                                    path_target = Some(target_pos);
                                }
                                break;
                            }
                        }
                    }

                    if let Some(target) = path_target {
                        let displacement = target - current_pos;
                        let distance = displacement.length();

                        let mut new_pos = if distance <= speed {
                            if let Some(steps) = path_steps.get_mut(&uuid) {
                                steps.pop_front();
                            }
                            target
                        } else {
                            let direction = displacement * (1.0 / distance);
                            current_pos + direction * speed
                        };

                        // Prevent clipping by stepping up/down when horizontally close to the next node
                        let horizontal_dist =
                            Vector3::new(displacement.x, 0.0, displacement.z).length();
                        if horizontal_dist < 0.8 {
                            new_pos.y = target.y;
                        } else {
                            new_pos.y = current_pos.y;
                        }

                        entity.set_pos(new_pos);

                        // Update rotation (yaw/body_yaw) to face the target walking direction
                        let y_rot_d =
                            (displacement.z.atan2(displacement.x).to_degrees() as f32) - 90.0;
                        let current_yaw = entity.yaw.load();
                        let new_yaw = wrap_degrees(
                            current_yaw + wrap_degrees(y_rot_d - current_yaw).clamp(-40.0, 40.0),
                        );
                        entity.yaw.store(new_yaw);
                        entity.body_yaw.store(new_yaw);

                        entity.send_pos_rot();
                    }

                    active_mobs.push(ActiveMobSnapshot {
                        uuid,
                        world_uuid: world.uuid,
                        current_pos,
                        movement_speed: living_entity
                            .get_attribute_value(&Attributes::MOVEMENT_SPEED),
                    });

                    let interval = path_interval_ticks(horizontal_distance, active_mobs.len());

                    let mut should_start = false;
                    if active_path_jobs.insert(uuid) {
                        let should_attempt = last_path_ticks.get(&uuid).is_none_or(|last_tick| {
                            i64::from(tick) - i64::from(*last_tick) >= interval
                        });
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

                    let player_tree = player_trees.get(&player_uuid).cloned();

                    let dist_3d = current_pos.squared_distance_to_vec(&player_eye_pos);
                    path_jobs_to_spawn.push(PathJobToSpawn {
                        uuid,
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
                                if let Some(living_entity) = entity_base.get_living_entity() {
                                    living_entity.movement_input.store(Vector3::default());
                                    living_entity
                                        .jumping
                                        .store(false, std::sync::atomic::Ordering::SeqCst);
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
            drop(managed_mobs);
            drop(disabled_mobs);
            drop(frozen_out_of_bounds);
            drop(grace_period);
            drop(path_endpoints);
            drop(player_trees);

            // Prioritize and spawn pathfinding jobs closest to the player first
            path_jobs_to_spawn.sort_by(|a, b| {
                a.distance_to_player
                    .partial_cmp(&b.distance_to_player)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for job in path_jobs_to_spawn {
                self.spawn_path_job(job.uuid, job.mob_pos, job.target_pos, job.player_tree);
            }

            if should_update_velocity {
                self.update_mob_locations(&active_mobs);

                let location_table = self.mob_locations.lock().unwrap();
                for mob in &active_mobs {
                    let push_velocity = clustering::cluster_push_velocity(
                        mob.uuid,
                        mob.movement_speed,
                        &location_table,
                    );
                    if push_velocity.length_squared() > 0.0 {
                        if let Some(world) = server
                            .worlds
                            .load()
                            .iter()
                            .find(|w| w.uuid == mob.world_uuid)
                        {
                            if let Some(entity_base) = world.get_entity_by_uuid(mob.uuid) {
                                let entity = entity_base.get_entity();
                                let current_pos = entity.pos.load();
                                entity.set_pos(current_pos + push_velocity);
                                entity.send_pos_rot();
                            }
                        }
                    }
                }
            }

            self.active_mobs_count
                .store(active_mobs.len(), std::sync::atomic::Ordering::Relaxed);
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
    pub(crate) fn register_chunk(
        &self,
        chunk_pos: Vector2<i32>,
        chunk_data: Arc<pumpkin_world::chunk::ChunkData>,
    ) {
        if !self
            .mob_ai_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let mut write = self.chunk_registry_write.lock().unwrap();
        let mut write_guard = write.guard();
        write_guard.insert(
            (chunk_pos.x, chunk_pos.y),
            Arc::new(types::ChunkPassability::new(Some(chunk_data))),
        );
        write_guard.publish();
    }

    pub(crate) fn update_block(
        &self,
        block_pos: BlockPos,
        block_state: &'static pumpkin_data::Block,
    ) {
        if !self
            .mob_ai_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let chunk_pos = (block_pos.0.x >> 4, block_pos.0.z >> 4);
        let rx = (block_pos.0.x & 15) as usize;
        let rz = (block_pos.0.z & 15) as usize;

        let guard = self.chunk_registry_read.guard();
        if let Some(chunk) = guard.get(&chunk_pos) {
            chunk.set_solid(
                rx,
                block_pos.0.y,
                rz,
                block_state.default_state.is_solid_block(),
            );
        }
    }

    fn ensure_pumpkin_mob_ai_disabled(&self, entity_base: &dyn EntityBase, uuid: Uuid) -> bool {
        let is_newly_disabled = self.disabled_mobs.lock().unwrap().insert(uuid);
        if is_newly_disabled {
            clear_pumpkin_mob_ai(entity_base);
        }
        is_newly_disabled
    }

    fn register_managed_mob(&self, _world_uuid: Uuid, entity_base: &dyn EntityBase) {
        if !self
            .mob_ai_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let entity = entity_base.get_entity();
        if !MOB_JUMP_TYPES.contains(&entity.entity_type.resource_name.as_ref()) {
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
        self.disabled_mobs.lock().unwrap().remove(&uuid);
        self.frozen_out_of_bounds_mobs.lock().unwrap().remove(&uuid);
        self.grace_period_mobs.lock().unwrap().remove(&uuid);
    }

    pub fn get_metrics(&self) -> MobAiMetrics {
        MobAiMetrics {
            active_path_jobs: self.active_path_jobs.lock().unwrap().len(),
            active_velocity_jobs: 0,
            total_worker_threads: self.worker_pool.current_num_threads(),
            managed_mobs_count: self
                .active_mobs_count
                .load(std::sync::atomic::Ordering::Relaxed),
            total_paths_completed: self
                .paths_completed
                .load(std::sync::atomic::Ordering::Relaxed),
            total_velocities_completed: 0,
        }
    }
}

/// Adapter exposing a [`MobAiState`] through the shared
/// [`cabbage_api::MobAiApi`] trait so consumers (metrics, other plugins) do
/// not depend on the concrete state type.
pub struct MobAiApiAdapter(pub Arc<MobAiState>);

impl cabbage_api::MobAiApi for MobAiApiAdapter {
    fn metrics(&self) -> cabbage_api::MobAiMetricsSnapshot {
        self.0.get_metrics()
    }

    fn set_enabled(&self, enabled: bool) {
        self.0
            .mob_ai_enabled
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_enabled(&self) -> bool {
        self.0
            .mob_ai_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn nearest_player_pos(
    players: &[PlayerSnapshot],
    mob_pos: BlockPos,
) -> Option<(Uuid, BlockPos, Vector3<f64>, i64)> {
    players
        .iter()
        .map(|player| {
            let distance_squared = block_distance_squared(mob_pos, player.block_pos);
            let horizontal_distance = horizontal_block_distance(mob_pos, player.block_pos);
            (
                player.uuid,
                player.block_pos,
                player.eye_pos,
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

struct PlayerSnapshot {
    uuid: Uuid,
    block_pos: BlockPos,
    eye_pos: Vector3<f64>,
    chunk_pos: Vector2<i32>,
}

fn update_pumpkin_look_target(entity_base: &dyn EntityBase, target_pos: Vector3<f64>) {
    let entity = entity_base.get_entity();
    let eye_pos = entity_base.get_eye_pos();

    let xd = target_pos.x - eye_pos.x;
    let yd = target_pos.y - eye_pos.y;
    let zd = target_pos.z - eye_pos.z;
    let horizontal_distance = (xd * xd + zd * zd).sqrt();

    if horizontal_distance > 1E-7 {
        let yaw = (zd.atan2(xd).to_degrees() as f32) - 90.0;
        let pitch = -(yd.atan2(horizontal_distance).to_degrees() as f32);

        let current_head_yaw = entity.head_yaw.load();
        let target_head_yaw = wrap_degrees(
            current_head_yaw + wrap_degrees(yaw - current_head_yaw).clamp(-10.0, 10.0),
        );
        entity.head_yaw.store(target_head_yaw);

        let current_pitch = entity.pitch.load();
        let target_pitch =
            wrap_degrees(current_pitch + wrap_degrees(pitch - current_pitch).clamp(-10.0, 10.0));
        entity.pitch.store(target_pitch);
    }
}

fn clear_pumpkin_mob_ai(entity_base: &dyn EntityBase) {
    if let Some(living_entity) = entity_base.get_living_entity() {
        living_entity.movement_input.store(Vector3::default());
        living_entity
            .jumping
            .store(false, std::sync::atomic::Ordering::SeqCst);
        living_entity
            .jumping_cooldown
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

impl EventHandler<pumpkin::plugin::api::events::world::chunk_send::ChunkSend> for MobAiState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a pumpkin::plugin::api::events::world::chunk_send::ChunkSend,
    ) -> BoxFuture<'a, ()> {
        let chunk_pos = Vector2::new(event.chunk.x, event.chunk.z);
        let chunk_data = Arc::clone(&event.chunk);
        self.register_chunk(chunk_pos, chunk_data);
        Box::pin(async move {})
    }

    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a mut pumpkin::plugin::api::events::world::chunk_send::ChunkSend,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

impl EventHandler<pumpkin::plugin::api::events::block::block_place::BlockPlaceEvent>
    for MobAiState
{
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a pumpkin::plugin::api::events::block::block_place::BlockPlaceEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.update_block(event.block_position, event.block_placed);
        })
    }
}

impl EventHandler<pumpkin::plugin::api::events::block::block_break::BlockBreakEvent>
    for MobAiState
{
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a pumpkin::plugin::api::events::block::block_break::BlockBreakEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.update_block(event.block_position, &pumpkin_data::Block::AIR);
        })
    }
}
