use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read},
    mem::MaybeUninit,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use pumpkin::{
    command::{
        CommandExecutor, CommandResult, CommandSender,
        args::ConsumedArgs,
        tree::{CommandTree, builder::literal},
    },
    entity::{EntityBase, RemovalReason},
    plugin::{
        BoxFuture, Context, EventHandler, EventPriority, PLUGIN_API_VERSION, Plugin, PluginFuture,
        PluginMetadata,
        api::events::block::{
            block_damage::BlockDamageEvent, block_drop_item::BlockDropItemEvent,
            block_piston_extend::BlockPistonExtendEvent,
            block_piston_retract::BlockPistonRetractEvent, brew::BrewEvent,
            furnace_burn::FurnaceBurnEvent, furnace_smelt::FurnaceSmeltEvent,
        },
        api::events::entity::{
            ChunkEntityLoadEvent, ChunkEntityUnloadEvent, EntityRemoveEvent, EntitySpawnEvent,
            entity_breed::EntityBreedEvent, entity_combust_by_entity::EntityCombustByEntityEvent,
            entity_damage::EntityDamageEvent, entity_damage_by_entity::EntityDamageByEntityEvent,
            entity_death::EntityDeathEvent, entity_explode::EntityExplodeEvent,
            entity_pickup_item::EntityPickupItemEvent, entity_shoot_bow::EntityShootBowEvent,
            entity_tame::EntityTameEvent, entity_target::EntityTargetEvent,
            entity_target_living_entity::EntityTargetLivingEntityEvent,
            entity_transform::EntityTransformEvent, explosion_prime::ExplosionPrimeEvent,
            potion_splash::PotionSplashEvent, projectile_hit::ProjectileHitEvent,
            projectile_launch::ProjectileLaunchEvent,
        },
        api::events::inventory::InventoryMoveItemEvent,
        api::events::player::{
            craft_item::CraftItemEvent, food_level_change::FoodLevelChangeEvent,
            furnace_extract::FurnaceExtractEvent, inventory_drag::InventoryDragEvent,
            inventory_interact::InventoryClickEvent, inventory_open::InventoryOpenEvent,
            player_death::PlayerDeathEvent, player_drop_item::PlayerDropItemEvent,
        },
        server::server_tick_start::ServerTickStartEvent,
    },
    server::Server,
};
use pumpkin_util::{
    math::vector2::Vector2,
    permission::{Permission, PermissionDefault, PermissionLvl},
    text::{TextComponent, color::NamedColor},
};
use ruzstd::{
    decoding::StreamingDecoder,
    encoding::{CompressionLevel, compress_to_vec},
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize};
use sysinfo::{Pid, System, get_current_pid};
use uuid::Uuid;

mod mmo;
mod mob_ai;

use mmo::config::PluginConfig;
use mob_ai::MobAiState;

const PLUGIN_NAME: &str = "Cabbage";
const CABBAGE_PERMISSION: &str = "Cabbage:command.cabbage";
const CABBAGE_NAMES: [&str; 1] = ["cabbage"];
const CLEAR_DROPS_PERMISSION: &str = "Cabbage:command.clear_drops";
const CLEAR_DROPS_NAMES: [&str; 1] = ["cleardrops"];
const METRICS_PERMISSION: &str = "Cabbage:command.metrics";
const METRICS_NAMES: [&str; 1] = ["metrics"];
const EVENTS_PERMISSION: &str = "Cabbage:command.events";
const EVENTS_NAMES: [&str; 1] = ["events"];
const DROPPED_ITEM_ENTITY_ID: &str = "minecraft:item";
const EVENT_LOG_FILE: &str = "output.log";

#[unsafe(no_mangle)]
pub static PUMPKIN_API_VERSION: u32 = PLUGIN_API_VERSION;

#[unsafe(no_mangle)]
pub static mut METADATA: MaybeUninit<PluginMetadata> = MaybeUninit::uninit();

#[ctor::ctor]
fn init_metadata() {
    let metadata = PluginMetadata {
        name: PLUGIN_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        authors: vec!["Pumpkin Server Admin".to_string()],
        description: "Cabbage server utility plugin.".to_string(),
        dependencies: Vec::new(),
        permissions: Vec::new(),
    };

    unsafe {
        core::ptr::addr_of_mut!(METADATA)
            .cast::<PluginMetadata>()
            .write(metadata);
    }
}

struct CabbagePlugin {
    clear_drops_state: Arc<ClearDropsState>,
    mob_ai_state: Arc<MobAiState>,
    dropped_item_cleanup_state: Arc<DroppedItemCleanupState>,
    metrics_reporter_state: Arc<MetricsReporterState>,
    event_log_state: Arc<EventLogState>,
    mmo_state: Option<Arc<mmo::MmoState>>,
}

impl CabbagePlugin {
    fn new() -> Self {
        let mob_ai_state = Arc::new(MobAiState::default());
        Self {
            clear_drops_state: Arc::new(ClearDropsState::default()),
            mob_ai_state: mob_ai_state.clone(),
            dropped_item_cleanup_state: Arc::new(DroppedItemCleanupState::default()),
            metrics_reporter_state: Arc::new(MetricsReporterState::new(mob_ai_state)),
            event_log_state: Arc::new(EventLogState::default()),
            mmo_state: None,
        }
    }
}

struct MetricsReporterState {
    sys: Arc<Mutex<System>>,
    pid: Pid,
    cached_map_chunks: Arc<AtomicUsize>,
    metrics_log: AtomicBool,
    config_path: Mutex<Option<PathBuf>>,
    mob_ai_state: Arc<MobAiState>,
    last_paths_completed: std::sync::atomic::AtomicUsize,
    last_velocities_completed: std::sync::atomic::AtomicUsize,
    last_metrics_time: Mutex<std::time::Instant>,

    last_app_ram_bytes: Arc<AtomicU64>,
    last_chunk_ram_bytes: Arc<AtomicU64>,
    ram_scanning: Arc<AtomicBool>,
}

impl MetricsReporterState {
    fn new(mob_ai_state: Arc<MobAiState>) -> Self {
        let sys = System::new();
        let pid = get_current_pid().expect("Failed to get current process ID");
        Self {
            sys: Arc::new(Mutex::new(sys)),
            pid,
            cached_map_chunks: Arc::new(AtomicUsize::new(0)),
            metrics_log: AtomicBool::new(false),
            config_path: Mutex::new(None),
            mob_ai_state,
            last_paths_completed: std::sync::atomic::AtomicUsize::new(0),
            last_velocities_completed: std::sync::atomic::AtomicUsize::new(0),
            last_metrics_time: Mutex::new(std::time::Instant::now()),

            last_app_ram_bytes: Arc::new(AtomicU64::new(0)),
            last_chunk_ram_bytes: Arc::new(AtomicU64::new(0)),
            ram_scanning: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Default)]
struct DroppedItemCleanupState {
    last_out_of_range_items: Mutex<HashSet<Uuid>>,
}

#[derive(Default)]
struct ClearDropsState {
    pending: AtomicBool,
    sender: Mutex<Option<CommandSender>>,
}

#[derive(Default)]
struct EventLogState {
    enabled: AtomicBool,
    log_path: Mutex<Option<PathBuf>>,
}

impl EventLogState {
    fn set_log_path(&self, path: PathBuf) {
        if let Ok(mut lock) = self.log_path.lock() {
            *lock = Some(path);
        }
    }

    fn toggle(&self) -> bool {
        !self.enabled.fetch_xor(true, Ordering::SeqCst)
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    fn log(&self, message: &str) {
        let Some(path) = self.log_path.lock().ok().and_then(|path| path.clone()) else {
            return;
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("[{}] ", d.as_secs()))
            .unwrap_or_default();

        let line = format!("{timestamp}{message}\n");

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = file.write_all(line.as_bytes());
        }
    }
}

#[derive(Default)]
struct SavedDropCleanup {
    folders_scanned: usize,
    files_scanned: usize,
    files_changed: usize,
    chunks_scanned: usize,
    chunks_changed: usize,
    saved_removed: usize,
    errors: usize,
}

#[derive(Serialize, Deserialize, Default)]
struct SavedPumpData {
    x: i32,
    z: i32,
    chunks: BTreeMap<String, Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SavedEntityChunkNbt {
    data_version: i32,
    position: [i32; 2],
    entities: Vec<pumpkin_nbt::NbtCompound>,
}

fn clear_drops_debug(message: impl AsRef<str>) {
    println!("[Cabbage] /cleardrops: {}", message.as_ref());
}

impl Plugin for CabbagePlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let cabbage_permission = Permission::new(
                CABBAGE_PERMISSION,
                "Allows viewing Cabbage command information.",
                PermissionDefault::Op(PermissionLvl::Two),
            );

            match context.register_permission(cabbage_permission).await {
                Ok(()) => {}
                Err(error) if error.contains("already registered") => {}
                Err(error) => return Err(error),
            }

            let clear_drops_permission = Permission::new(
                CLEAR_DROPS_PERMISSION,
                "Allows clearing all loaded dropped item entities.",
                PermissionDefault::Allow,
            );

            match context.register_permission(clear_drops_permission).await {
                Ok(()) => {}
                Err(error) if error.contains("already registered") => {}
                Err(error) => return Err(error),
            }

            let metrics_permission = Permission::new(
                METRICS_PERMISSION,
                "Allows viewing metrics and toggling metrics console logging.",
                PermissionDefault::Op(PermissionLvl::Two),
            );

            match context.register_permission(metrics_permission).await {
                Ok(()) => {}
                Err(error) if error.contains("already registered") => {}
                Err(error) => return Err(error),
            }

            let events_permission = Permission::new(
                EVENTS_PERMISSION,
                "Allows toggling Phase 2/3/4 event logging to chat and output.log.",
                PermissionDefault::Op(PermissionLvl::Two),
            );

            match context.register_permission(events_permission).await {
                Ok(()) => {}
                Err(error) if error.contains("already registered") => {}
                Err(error) => return Err(error),
            }

            self.metrics_reporter_state
                .load_config(context.get_data_folder());

            self.event_log_state
                .set_log_path(context.get_data_folder().join(EVENT_LOG_FILE));

            match mmo::MmoState::new(context.clone()).await {
                Ok(state) => {
                    self.mmo_state = Some(state);
                }
                Err(error) => {
                    println!("[Cabbage] Failed to initialize MMO module: {error}");
                    self.mmo_state = None;
                }
            }

            spawn_disk_scan(
                context.server.worlds.load().iter().cloned().collect(),
                self.metrics_reporter_state.cached_map_chunks.clone(),
            );

            context
                .register_command(cabbage_command_tree(), CABBAGE_PERMISSION)
                .await;
            context
                .register_event::<ServerTickStartEvent, _>(
                    self.clear_drops_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ServerTickStartEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    true,
                )
                .await;
            context
                .register_event::<EntitySpawnEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityRemoveEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ChunkEntityLoadEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ChunkEntityUnloadEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<pumpkin::plugin::api::events::world::chunk_send::ChunkSend, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<pumpkin::plugin::api::events::block::block_place::BlockPlaceEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<pumpkin::plugin::api::events::block::block_break::BlockBreakEvent, _>(
                    self.mob_ai_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ServerTickStartEvent, _>(
                    self.dropped_item_cleanup_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ServerTickStartEvent, _>(
                    self.metrics_reporter_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_command(
                    clear_drops_command_tree(self.clear_drops_state.clone()),
                    CLEAR_DROPS_PERMISSION,
                )
                .await;
            context
                .register_command(
                    metrics_command_tree(self.metrics_reporter_state.clone()),
                    METRICS_PERMISSION,
                )
                .await;
            context
                .register_command(
                    events_command_tree(self.event_log_state.clone()),
                    EVENTS_PERMISSION,
                )
                .await;

            if let Some(mmo_state) = self.mmo_state.as_ref() {
                if let Err(error) = mmo::register_permissions(&context).await {
                    println!("[Cabbage] Failed to register MMO permissions: {error}");
                }

                context
                    .register_event::<pumpkin::plugin::api::events::block::block_break::BlockBreakEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::block::block_place::BlockPlaceEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::block::block_broken::BlockBrokenEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<ServerTickStartEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::world::feature_generate::FeatureGenerateEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::player_interact_event::PlayerInteractEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::fish::PlayerFishEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<EntityBreedEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<EntityTameEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::player_interact_entity_event::PlayerInteractEntityEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::player_item_use_finish::PlayerItemUseFinishEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::player_attack::PlayerAttackDamageEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<EntityShootBowEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<ProjectileHitEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<EntityDeathEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<EntityDamageEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::anvil_prepare::AnvilPrepareEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::anvil_repair::AnvilRepairEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::grindstone::GrindstoneEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::grindstone::GrindstoneTakeEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::enchant_item_generate::EnchantItemGenerateEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::enchant_item::EnchantItemEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<CraftItemEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_event::<FurnaceExtractEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        false,
                    )
                    .await;
                context
                    .register_command(
                        mmo::mmo_command_tree(mmo_state.clone()),
                        mmo::MMO_PERMISSION,
                    )
                    .await;
            }

            context
                .register_event::<BlockDamageEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<BlockDropItemEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<BlockPistonExtendEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<BlockPistonRetractEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityDamageEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityDamageByEntityEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityDeathEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<PlayerDeathEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<FoodLevelChangeEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ProjectileLaunchEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ProjectileHitEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<PlayerDropItemEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<InventoryOpenEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<InventoryClickEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<InventoryDragEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<InventoryMoveItemEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<CraftItemEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<FurnaceSmeltEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<FurnaceBurnEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<FurnaceExtractEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<BrewEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityBreedEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityTameEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityTargetEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityTargetLivingEntityEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityPickupItemEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityShootBowEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityCombustByEntityEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityExplodeEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<ExplosionPrimeEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<EntityTransformEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<PotionSplashEvent, _>(
                    self.event_log_state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;

            Ok(())
        })
    }

    fn on_unload(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            context.unregister_command(CABBAGE_NAMES[0]).await;
            context.unregister_command(CLEAR_DROPS_NAMES[0]).await;
            context.unregister_command(METRICS_NAMES[0]).await;
            context.unregister_command(EVENTS_NAMES[0]).await;
            context.unregister_command(mmo::MMO_NAMES[0]).await;
            Ok(())
        })
    }
}

impl EventHandler<ServerTickStartEvent> for DroppedItemCleanupState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.tick % 100 != 0 {
                return;
            }

            let last_out_of_range = {
                let last_items = self.last_out_of_range_items.lock().unwrap();
                last_items.clone()
            };
            let mut current_out_of_range = HashSet::new();

            for world in server.worlds.load().iter() {
                let mut watched_chunks = HashSet::<Vector2<i32>>::new();

                for player in world.players.load().iter() {
                    let center = player.get_entity().chunk_pos.load();

                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            watched_chunks.insert(Vector2::new(center.x + dx, center.y + dz));
                        }
                    }
                }

                let entities = world.entities.load();
                for entity_base in entities.iter() {
                    let entity = entity_base.get_entity();

                    if entity.entity_type.resource_name != "item"
                        && entity.entity_type.resource_name != "minecraft:item"
                    {
                        continue;
                    }

                    let chunk_pos = entity.chunk_pos.load();
                    if watched_chunks.contains(&chunk_pos) {
                        continue;
                    }

                    let uuid = entity.entity_uuid;
                    let was_out_of_range = last_out_of_range.contains(&uuid);

                    if was_out_of_range {
                        entity.removed.store(true, Ordering::Relaxed);
                        entity.removal_reason.store(Some(RemovalReason::Discarded));
                        entity.remove().await;
                    } else {
                        current_out_of_range.insert(uuid);
                    }
                }
            }

            {
                let mut last_items = self.last_out_of_range_items.lock().unwrap();
                *last_items = current_out_of_range;
            }
        })
    }
}

struct CabbageInfoExecutor;

impl CommandExecutor for CabbageInfoExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            sender
                .send_message(TextComponent::text(
                    "Cabbage commands:\n/cleardrops - Queue dropped item cleanup.\n/metrics - Print current metrics once.\n/metrics log - Toggle periodic console metric logging.\n/events - Toggle Phase 2/3/4 event logging to chat and output.log.",
                ))
                .await;

            Ok(1)
        })
    }
}

struct EventsToggleExecutor {
    state: Arc<EventLogState>,
}

impl CommandExecutor for EventsToggleExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let enabled = self.state.toggle();
            let (state_text, color) = if enabled {
                ("on", NamedColor::Green)
            } else {
                ("off", NamedColor::Red)
            };

            sender
                .send_message(
                    TextComponent::text("Phase 2/3/4 event logging is ")
                        .add_child(TextComponent::text(state_text).color_named(color))
                        .add_text(". Events are written to output.log in the Cabbage data folder."),
                )
                .await;

            Ok(1)
        })
    }
}

struct ClearDropsExecutor {
    state: Arc<ClearDropsState>,
}

impl CommandExecutor for ClearDropsExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let queued = !self.state.pending.swap(true, Ordering::SeqCst);
            if queued && let Ok(mut pending_sender) = self.state.sender.lock() {
                *pending_sender = Some(sender.clone());
            }

            clear_drops_debug(format!("command received from {sender}; queued={queued}"));

            sender
                .send_message(TextComponent::text(if queued {
                    "Queued dropped item cleanup for the next server tick."
                } else {
                    "Dropped item cleanup is already queued."
                }))
                .await;

            Ok(1)
        })
    }
}

struct MetricsPrintExecutor {
    state: Arc<MetricsReporterState>,
}

impl CommandExecutor for MetricsPrintExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            if !metrics_command_allowed(sender) {
                sender
                    .send_message(
                        TextComponent::text("You do not have permission to run /metrics.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            }

            let snapshot = self.state.collect_metrics(server);
            sender
                .send_message(TextComponent::text(snapshot.format()))
                .await;

            Ok(1)
        })
    }
}

struct MetricsLogToggleExecutor {
    state: Arc<MetricsReporterState>,
}

impl CommandExecutor for MetricsLogToggleExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            if !metrics_command_allowed(sender) {
                sender
                    .send_message(
                        TextComponent::text("You do not have permission to run /metrics.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            }

            let enabled = self.state.toggle_metrics_log();
            sender.send_message(metrics_log_message(enabled)).await;

            Ok(1)
        })
    }
}

fn metrics_command_allowed(sender: &CommandSender) -> bool {
    matches!(sender, CommandSender::Console | CommandSender::Rcon(_))
        || (sender.is_player() && sender.has_permission_lvl(PermissionLvl::Two))
}

fn metrics_log_message(enabled: bool) -> TextComponent {
    let (state, color) = if enabled {
        ("on", NamedColor::Green)
    } else {
        ("off", NamedColor::Red)
    };

    TextComponent::text("Turning metric logging ")
        .add_child(TextComponent::text(state).color_named(color))
        .add_text(".")
}

impl EventHandler<ServerTickStartEvent> for ClearDropsState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        self.run_clear_on_tick(server, event.tick, "handle")
    }

    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        self.run_clear_on_tick(server, event.tick, "handle_blocking")
    }
}

impl ClearDropsState {
    fn run_clear_on_tick<'a>(
        &'a self,
        server: &'a Arc<Server>,
        tick: i32,
        handler: &'static str,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.pending.swap(false, Ordering::SeqCst) {
                return;
            }

            clear_drops_debug(format!("tick {tick}: running cleanup via {handler}"));

            let sender = self.sender.lock().ok().and_then(|mut sender| sender.take());
            let loaded_removed = clear_loaded_drops(server).await;
            let saved_summary = clear_saved_drops(server);

            clear_drops_debug(format!(
                "tick {tick}: cleanup finished; loaded_removed={loaded_removed}, saved_removed={}, saved_files_changed={}, saved_errors={}",
                saved_summary.saved_removed, saved_summary.files_changed, saved_summary.errors
            ));

            if let Some(sender) = sender {
                sender
                    .send_message(TextComponent::text(format!(
                        "Cleared {loaded_removed} loaded dropped item{} and {} saved dropped item{} from .pump files{}.",
                        if loaded_removed == 1 { "" } else { "s" },
                        saved_summary.saved_removed,
                        if saved_summary.saved_removed == 1 {
                            ""
                        } else {
                            "s"
                        },
                        if saved_summary.errors == 0 {
                            String::new()
                        } else {
                            format!(" ({} saved file scan error{})", saved_summary.errors, if saved_summary.errors == 1 { "" } else { "s" })
                        }
                    )))
                    .await;
            }
        })
    }
}

async fn clear_loaded_drops(server: &Server) -> usize {
    let mut drops = Vec::new();

    let worlds = server.worlds.load();
    clear_drops_debug(format!("scanning {} loaded world(s)", worlds.len()));

    for (world_index, world) in worlds.iter().enumerate() {
        let entities = world.entities.load();
        let before = drops.len();

        drops.extend(entities.iter().filter_map(|entity| {
            (entity.get_entity().entity_type.resource_name == "item").then(|| entity.clone())
        }));

        let world_drops = drops.len() - before;
        clear_drops_debug(format!(
            "world #{world_index}: entities={}, dropped_items={world_drops}",
            entities.len()
        ));
    }

    let removed = drops.len();
    clear_drops_debug(format!("removing {removed} dropped item entity/entities"));

    for entity in drops {
        let base_entity = entity.get_entity();
        base_entity.removed.store(true, Ordering::Relaxed);
        base_entity
            .removal_reason
            .store(Some(RemovalReason::Discarded));
        base_entity.remove().await;
    }

    removed
}

fn clear_saved_drops(server: &Server) -> SavedDropCleanup {
    let mut summary = SavedDropCleanup::default();
    let folders = saved_entity_folders(server);

    clear_drops_debug(format!(
        "scanning saved .pump entity files in {} folder(s)",
        folders.len()
    ));

    for folder in folders {
        summary.folders_scanned += 1;
        clear_drops_debug(format!("saved scan folder: {}", folder.display()));

        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                record_saved_scan_error(&mut summary, &folder, error);
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    record_saved_scan_error(&mut summary, &folder, error);
                    continue;
                }
            };

            let path = entry.path();
            if !is_pump_file(&path) {
                continue;
            }

            if let Err(error) = clear_saved_pump_file(&path, &mut summary) {
                record_saved_scan_error(&mut summary, &path, error);
            }
        }
    }

    clear_drops_debug(format!(
        "saved scan finished: folders={}, files={}, files_changed={}, chunks={}, chunks_changed={}, saved_removed={}, errors={}",
        summary.folders_scanned,
        summary.files_scanned,
        summary.files_changed,
        summary.chunks_scanned,
        summary.chunks_changed,
        summary.saved_removed,
        summary.errors
    ));

    summary
}

fn saved_entity_folders(server: &Server) -> Vec<PathBuf> {
    let mut folders = Vec::new();

    for world in server.worlds.load().iter() {
        let folder = world.level.level_folder.entities_folder.clone();
        if !folders.iter().any(|existing| existing == &folder) {
            folders.push(folder);
        }
    }

    folders
}

fn clear_saved_pump_file(path: &Path, summary: &mut SavedDropCleanup) -> Result<(), String> {
    summary.files_scanned += 1;

    let file_bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut pump_data: SavedPumpData = pumpkin_nbt::from_bytes_unnamed(Cursor::new(file_bytes))
        .map_err(|error| {
            format!(
                "failed to parse pump region NBT {}: {error}",
                path.display()
            )
        })?;

    let mut file_changed = false;
    let region_x = pump_data.x;
    let region_z = pump_data.z;

    for (chunk_key, compressed_chunk) in pump_data.chunks.iter_mut() {
        let chunk_index = match chunk_key.parse::<i32>() {
            Ok(index) if (0..1024).contains(&index) => index,
            _ => {
                summary.errors += 1;
                clear_drops_debug(format!(
                    "saved scan error in {}: invalid chunk key {chunk_key}",
                    path.display()
                ));
                continue;
            }
        };

        summary.chunks_scanned += 1;

        let mut decoder = match StreamingDecoder::new(&compressed_chunk[..]) {
            Ok(decoder) => decoder,
            Err(error) => {
                summary.errors += 1;
                clear_drops_debug(format!(
                    "saved scan error in {} chunk {chunk_key}: zstd decoder error: {error}",
                    path.display()
                ));
                continue;
            }
        };

        let mut decompressed = Vec::new();
        if let Err(error) = decoder.read_to_end(&mut decompressed) {
            summary.errors += 1;
            clear_drops_debug(format!(
                "saved scan error in {} chunk {chunk_key}: zstd read error: {error}",
                path.display()
            ));
            continue;
        }

        let mut chunk_nbt: SavedEntityChunkNbt = match pumpkin_nbt::from_bytes_unnamed(Cursor::new(
            decompressed,
        )) {
            Ok(chunk_nbt) => chunk_nbt,
            Err(error) => {
                summary.errors += 1;
                clear_drops_debug(format!(
                    "saved scan error in {} chunk {chunk_key}: entity chunk NBT parse error: {error}",
                    path.display()
                ));
                continue;
            }
        };

        let rel_x = chunk_index % 32;
        let rel_z = chunk_index / 32;
        let expected_position = [region_x * 32 + rel_x, region_z * 32 + rel_z];
        if chunk_nbt.position != expected_position {
            clear_drops_debug(format!(
                "saved scan warning in {} chunk {chunk_key}: expected chunk {},{} but NBT says {},{}",
                path.display(),
                expected_position[0],
                expected_position[1],
                chunk_nbt.position[0],
                chunk_nbt.position[1]
            ));
        }

        let before = chunk_nbt.entities.len();
        chunk_nbt
            .entities
            .retain(|entity| entity.get_string("id") != Some(DROPPED_ITEM_ENTITY_ID));
        let removed = before - chunk_nbt.entities.len();

        if removed == 0 {
            continue;
        }

        let mut serialized_chunk = Vec::new();
        pumpkin_nbt::to_bytes_unnamed(&chunk_nbt, &mut serialized_chunk).map_err(|error| {
            format!(
                "failed to serialize entity chunk {chunk_key} in {}: {error}",
                path.display()
            )
        })?;

        *compressed_chunk = compress_to_vec(&serialized_chunk[..], CompressionLevel::Fastest);
        file_changed = true;
        summary.chunks_changed += 1;
        summary.saved_removed += removed;
    }

    if file_changed {
        let mut serialized_file = Vec::new();
        pumpkin_nbt::to_bytes_unnamed(&pump_data, &mut serialized_file).map_err(|error| {
            format!("failed to serialize pump file {}: {error}", path.display())
        })?;
        fs::write(path, serialized_file)
            .map_err(|error| format!("failed to write pump file {}: {error}", path.display()))?;
        summary.files_changed += 1;
    }

    Ok(())
}

fn is_pump_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pump"))
}

fn record_saved_scan_error(
    summary: &mut SavedDropCleanup,
    path: &Path,
    error: impl std::fmt::Display,
) {
    summary.errors += 1;
    clear_drops_debug(format!("saved scan error in {}: {error}", path.display()));
}

fn cabbage_command_tree() -> CommandTree {
    CommandTree::new(CABBAGE_NAMES, "List Cabbage commands").execute(CabbageInfoExecutor)
}

fn clear_drops_command_tree(state: Arc<ClearDropsState>) -> CommandTree {
    CommandTree::new(
        CLEAR_DROPS_NAMES,
        "Remove loaded and saved dropped item entities",
    )
    .execute(ClearDropsExecutor { state })
}

fn metrics_command_tree(state: Arc<MetricsReporterState>) -> CommandTree {
    CommandTree::new(METRICS_NAMES, "Show metrics and toggle metrics logging")
        .execute(MetricsPrintExecutor {
            state: state.clone(),
        })
        .then(literal("log").execute(MetricsLogToggleExecutor { state }))
}

fn events_command_tree(state: Arc<EventLogState>) -> CommandTree {
    CommandTree::new(
        EVENTS_NAMES,
        "Toggle Phase 2/3/4 event logging to chat and output.log",
    )
    .execute(EventsToggleExecutor { state })
}

fn spawn_disk_scan(worlds: Vec<Arc<pumpkin::world::World>>, cached_map_chunks: Arc<AtomicUsize>) {
    std::thread::spawn(move || {
        let mut total_chunks = 0;

        for world in worlds {
            let region_folder = &world.level.level_folder.region_folder;
            if let Ok(entries) = std::fs::read_dir(region_folder) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("pump") {
                        if let Ok(file_bytes) = std::fs::read(&path) {
                            if let Ok(pump_data) = pumpkin_nbt::from_bytes_unnamed::<SavedPumpData>(
                                std::io::Cursor::new(file_bytes),
                            ) {
                                total_chunks += pump_data.chunks.len();
                            }
                        }
                    }
                }
            }
        }

        cached_map_chunks.store(total_chunks, Ordering::SeqCst);
    });
}

fn estimate_chunk_ram(chunk: &pumpkin_world::chunk::ChunkData) -> usize {
    let mut size = std::mem::size_of::<pumpkin_world::chunk::ChunkData>();

    if let Ok(block_sections) = chunk.section.block_sections.read() {
        for palette in block_sections.iter() {
            if let pumpkin_world::chunk::palette::PalettedContainer::Heterogeneous(_) = palette {
                size += 6144;
            }
        }
    }
    if let Ok(biome_sections) = chunk.section.biome_sections.read() {
        for palette in biome_sections.iter() {
            if let pumpkin_world::chunk::palette::PalettedContainer::Heterogeneous(_) = palette {
                size += 64;
            }
        }
    }
    if let Ok(light) = chunk.light_engine.lock() {
        size += light.sky_light.len() * 2048;
        size += light.block_light.len() * 2048;
    }
    size
}

struct MetricsSnapshot {
    loaded_chunks_total: usize,
    loaded_chunks_ram_mib: f64,
    loaded_entity_chunks: usize,
    total_map_chunks: usize,
    tps: f64,
    mspt: f64,
    app_ram_mib: f64,
    mob_ai_managed_mobs: usize,
    mob_ai_active_path_jobs: usize,
    mob_ai_active_velocity_jobs: usize,
    mob_ai_worker_threads: usize,
    mob_ai_paths_per_sec: f64,
    mob_ai_velocities_per_sec: f64,
}

impl MetricsSnapshot {
    fn format(&self) -> String {
        format!(
            "[Cabbage] Chunks loaded in world: {} ({:.2} MB) | Chunks loaded in chunk.data (entities): {} | Total map chunks: {} | TPS: {:.1} (MSPT: {:.2}ms) | App RAM: {:.2} MB\n\
             [Cabbage Mob AI] Managed mobs: {} | Active jobs: path={}, velocity={} (pool size: {})\n\
             [Cabbage Mob AI] Processing rates: paths/s: {:.1}, velocity_plans/s: {:.1}",
            self.loaded_chunks_total,
            self.loaded_chunks_ram_mib,
            self.loaded_entity_chunks,
            self.total_map_chunks,
            self.tps,
            self.mspt,
            self.app_ram_mib,
            self.mob_ai_managed_mobs,
            self.mob_ai_active_path_jobs,
            self.mob_ai_active_velocity_jobs,
            self.mob_ai_worker_threads,
            self.mob_ai_paths_per_sec,
            self.mob_ai_velocities_per_sec
        )
    }
}

impl MetricsReporterState {
    fn load_config(&self, data_folder: PathBuf) {
        let path = data_folder.join("config.ron");
        let config = fs::read_to_string(&path)
            .ok()
            .and_then(|contents| ron::from_str::<PluginConfig>(&contents).ok())
            .unwrap_or_default();

        self.metrics_log.store(config.metrics_log, Ordering::SeqCst);
        self.mob_ai_state
            .mob_ai_enabled
            .store(config.mob_ai, Ordering::SeqCst);

        if let Ok(mut config_path) = self.config_path.lock() {
            *config_path = Some(path);
        }

        self.save_config(config.metrics_log, config.mob_ai);
    }

    fn toggle_metrics_log(&self) -> bool {
        let enabled = !self.metrics_log.fetch_xor(true, Ordering::SeqCst);
        let mob_ai = self.mob_ai_state.mob_ai_enabled.load(Ordering::SeqCst);
        self.save_config(enabled, mob_ai);
        enabled
    }

    fn save_config(&self, metrics_log: bool, mob_ai: bool) {
        let path = self.config_path.lock().ok().and_then(|path| path.clone());
        let Some(path) = path else {
            return;
        };

        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            println!(
                "[Cabbage] Failed to create metrics config folder {}: {error}",
                parent.display()
            );
            return;
        }

        let mut config = fs::read_to_string(&path)
            .ok()
            .and_then(|contents| ron::from_str::<PluginConfig>(&contents).ok())
            .unwrap_or_default();
        config.metrics_log = metrics_log;
        config.mob_ai = mob_ai;

        let Ok(contents) = ron::ser::to_string_pretty(&config, ron::ser::PrettyConfig::default())
        else {
            return;
        };

        if let Err(error) = fs::write(&path, contents) {
            println!(
                "[Cabbage] Failed to write metrics config {}: {error}",
                path.display()
            );
        }
    }

    fn collect_metrics(&self, server: &Server) -> MetricsSnapshot {
        let tps = server.get_tps().min(server.basic_config.tps as f64);
        let mspt = server.get_mspt();

        let app_ram_mib = self.last_app_ram_bytes.load(Ordering::SeqCst) as f64 / (1024.0 * 1024.0);
        let loaded_chunks_ram = self.last_chunk_ram_bytes.load(Ordering::SeqCst);

        let mut loaded_chunks_total = 0;
        let mut loaded_entity_chunks = 0;

        for world in server.worlds.load().iter() {
            loaded_chunks_total += world.level.loaded_chunks.len();
            loaded_entity_chunks += world.level.loaded_entity_chunks_count();
        }

        let mob_ai_metrics = self.mob_ai_state.get_metrics();

        let (paths_rate, velocities_rate) = {
            let mut last_time_lock = self.last_metrics_time.lock().unwrap();
            let elapsed = last_time_lock.elapsed().as_secs_f64();
            *last_time_lock = std::time::Instant::now();

            let last_paths = self
                .last_paths_completed
                .swap(mob_ai_metrics.total_paths_completed, Ordering::SeqCst);
            let last_velocities = self
                .last_velocities_completed
                .swap(mob_ai_metrics.total_velocities_completed, Ordering::SeqCst);

            if elapsed > 0.0 {
                let p_rate = (mob_ai_metrics
                    .total_paths_completed
                    .saturating_sub(last_paths)) as f64
                    / elapsed;
                let v_rate = (mob_ai_metrics
                    .total_velocities_completed
                    .saturating_sub(last_velocities)) as f64
                    / elapsed;
                (p_rate, v_rate)
            } else {
                (0.0, 0.0)
            }
        };

        MetricsSnapshot {
            loaded_chunks_total,
            loaded_chunks_ram_mib: loaded_chunks_ram as f64 / (1024.0 * 1024.0),
            loaded_entity_chunks,
            total_map_chunks: self.cached_map_chunks.load(Ordering::SeqCst),
            tps,
            mspt,
            app_ram_mib,
            mob_ai_managed_mobs: mob_ai_metrics.managed_mobs_count,
            mob_ai_active_path_jobs: mob_ai_metrics.active_path_jobs,
            mob_ai_active_velocity_jobs: mob_ai_metrics.active_velocity_jobs,
            mob_ai_worker_threads: mob_ai_metrics.total_worker_threads,
            mob_ai_paths_per_sec: paths_rate,
            mob_ai_velocities_per_sec: velocities_rate,
        }
    }
}

impl EventHandler<ServerTickStartEvent> for MetricsReporterState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.metrics_log.load(Ordering::SeqCst) || event.tick % 40 != 0 {
                return;
            }

            if !self.ram_scanning.swap(true, Ordering::SeqCst) {
                // Collect chunks to scan on background thread
                let mut chunk_copies = Vec::new();
                for world in server.worlds.load().iter() {
                    for entry in world.level.loaded_chunks.iter() {
                        chunk_copies.push(Arc::clone(entry.value()));
                    }
                }

                let sys = Arc::clone(&self.sys);
                let pid = self.pid;
                let last_app_ram_bytes = Arc::clone(&self.last_app_ram_bytes);
                let last_chunk_ram_bytes = Arc::clone(&self.last_chunk_ram_bytes);
                let ram_scanning = Arc::clone(&self.ram_scanning);

                std::thread::spawn(move || {
                    let app_ram = {
                        let mut sys_lock = sys.lock().unwrap();
                        sys_lock.refresh_process(pid);
                        if let Some(process) = sys_lock.process(pid) {
                            process.memory()
                        } else {
                            0
                        }
                    };
                    last_app_ram_bytes.store(app_ram, Ordering::SeqCst);

                    let mut chunk_ram = 0;
                    for chunk in chunk_copies {
                        chunk_ram += estimate_chunk_ram(&chunk);
                    }
                    last_chunk_ram_bytes.store(chunk_ram as u64, Ordering::SeqCst);

                    ram_scanning.store(false, Ordering::SeqCst);
                });
            }

            println!("{}", self.collect_metrics(server).format());
        })
    }
}

impl EventHandler<BlockDamageEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockDamageEvent: player={}, block={}, pos={:?}, insta_break={}",
                event.player.gameprofile.name,
                event.block.name,
                event.block_position,
                event.insta_break
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<BlockDropItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockDropItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockDropItemEvent: player={}, block={}, pos={:?}, items={}",
                event.player.gameprofile.name,
                event.block.name,
                event.block_position,
                event.items.len()
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<BlockPistonExtendEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockPistonExtendEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockPistonExtendEvent: block={}, pos={:?}, direction={:?}, moved_blocks={}, broken_blocks={}",
                event.piston_block.name,
                event.piston_pos,
                event.direction,
                event.moved_blocks.len(),
                event.broken_blocks.len()
            );

            self.log(&message);
        })
    }
}

impl EventHandler<BlockPistonRetractEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockPistonRetractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockPistonRetractEvent: block={}, pos={:?}, direction={:?}, moved_blocks={}",
                event.piston_block.name,
                event.piston_pos,
                event.direction,
                event.moved_blocks.len()
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityDamageEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityDamageEvent: entity={}, damage_type={}, damage={}, final_damage={}",
                event.entity.get_entity().entity_type.resource_name,
                event.damage_type.message_id,
                event.damage,
                event.final_damage
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityDamageByEntityEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityDamageByEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let attacker_name = event
                .attacker
                .as_ref()
                .map(|a| a.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityDamageByEntityEvent: entity={}, damager={}, attacker={}, damage_type={}, damage={}, final_damage={}",
                event.entity.get_entity().entity_type.resource_name,
                event.damager.get_entity().entity_type.resource_name,
                attacker_name,
                event.damage_type.message_id,
                event.damage,
                event.final_damage
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityDeathEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityDeathEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let killer_name = event
                .killer
                .as_ref()
                .map(|k| k.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityDeathEvent: entity={}, killer={}, damage_type={}, drops={}, dropped_exp={}",
                event.entity.get_entity().entity_type.resource_name,
                killer_name,
                event.damage_type.message_id,
                event.drops.len(),
                event.dropped_exp
            );

            self.log(&message);
        })
    }
}

impl EventHandler<PlayerDeathEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a PlayerDeathEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let killer_name = event
                .killer
                .as_ref()
                .map(|k| k.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] PlayerDeathEvent: player={}, killer={}, damage_type={}, drops={}, dropped_exp={}, keep_inventory={}, keep_level={}",
                event.player.gameprofile.name,
                killer_name,
                event.damage_type.message_id,
                event.drops.len(),
                event.dropped_exp,
                event.keep_inventory,
                event.keep_level
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<FoodLevelChangeEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FoodLevelChangeEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FoodLevelChangeEvent: player={}, food_level={}",
                event.player.gameprofile.name, event.food_level
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<ProjectileLaunchEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ProjectileLaunchEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let shooter_name = event
                .shooter
                .as_ref()
                .map(|s| s.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] ProjectileLaunchEvent: projectile={}, shooter={}",
                event.projectile.get_entity().entity_type.resource_name,
                shooter_name
            );

            self.log(&message);
        })
    }
}

impl EventHandler<ProjectileHitEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ProjectileHitEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let hit_entity_name = event
                .hit_entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let hit_block_name = event.hit_block.map(|b| b.name).unwrap_or("none");
            let message = format!(
                "[Cabbage Events] ProjectileHitEvent: projectile={}, hit_entity={}, hit_block={}",
                event.projectile.get_entity().entity_type.resource_name,
                hit_entity_name,
                hit_block_name
            );

            self.log(&message);
        })
    }
}

impl EventHandler<PlayerDropItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a PlayerDropItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] PlayerDropItemEvent: player={}, item={}, count={}",
                event.player.gameprofile.name, event.item.item.registry_key, event.item.item_count
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryOpenEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryOpenEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] InventoryOpenEvent: player={}, window_type={:?}, block_pos={:?}",
                event.player.gameprofile.name, event.window_type, event.block_pos
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryClickEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryClickEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let clicked_name = event
                .clicked_item
                .as_ref()
                .map(|i| i.item.registry_key.as_ref())
                .unwrap_or("empty");
            let cursor_name = event
                .cursor
                .as_ref()
                .map(|i| i.item.registry_key.as_ref())
                .unwrap_or("empty");
            let message = format!(
                "[Cabbage Events] InventoryClickEvent: player={}, window_type={:?}, slot={}, click_type={:?}, clicked={}, cursor={}",
                event.player.gameprofile.name,
                event.window_type,
                event.slot,
                event.click_type,
                clicked_name,
                cursor_name
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryDragEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryDragEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] InventoryDragEvent: player={}, window_type={:?}, slots={:?}, click_type={:?}",
                event.player.gameprofile.name, event.window_type, event.slots, event.click_type
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryMoveItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryMoveItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] InventoryMoveItemEvent: item={}, count={}, source_pos={:?}, destination_pos={:?}",
                event.item.item.registry_key,
                event.item.item_count,
                event.source_pos,
                event.destination_pos
            );

            self.log(&message);
        })
    }
}

impl EventHandler<CraftItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a CraftItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] CraftItemEvent: player={}, result={}, count={}, window_type={:?}",
                event.player.gameprofile.name,
                event.result.item.registry_key,
                event.result.item_count,
                event.window_type
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<FurnaceSmeltEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FurnaceSmeltEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FurnaceSmeltEvent: block={}, pos={:?}, input={}, fuel={}, output={}",
                event.block.name,
                event.block_position,
                event.input.item.registry_key,
                event.fuel.item.registry_key,
                event.output.item.registry_key
            );

            self.log(&message);
        })
    }
}

impl EventHandler<FurnaceBurnEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FurnaceBurnEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FurnaceBurnEvent: block={}, pos={:?}, fuel={}, burn_time={}",
                event.block.name,
                event.block_position,
                event.fuel.item.registry_key,
                event.burn_time
            );

            self.log(&message);
        })
    }
}

impl EventHandler<FurnaceExtractEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FurnaceExtractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FurnaceExtractEvent: player={}, pos={:?}, item={}, count={}, experience={}",
                event.player.gameprofile.name,
                event.block_position,
                event.item.item.registry_key,
                event.item.item_count,
                event.experience
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<BrewEvent> for EventLogState {
    fn handle<'a>(&'a self, _server: &'a Arc<Server>, event: &'a BrewEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BrewEvent: block={}, pos={:?}, ingredient={}, potions={}, fuel={}",
                event.block.name,
                event.block_position,
                event.ingredient.item.registry_key,
                event.potions.len(),
                event.fuel
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityBreedEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityBreedEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let breeder_name = event
                .breeder
                .as_ref()
                .map(|p| p.gameprofile.name.as_str())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityBreedEvent: mother={}, father={}, breeder={}, entity_type={}, experience={}",
                event.mother.get_entity().entity_type.resource_name,
                event.father.get_entity().entity_type.resource_name,
                breeder_name,
                event.baby_type.resource_name,
                event.experience
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityTameEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTameEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityTameEvent: entity={}, owner={}",
                event.entity.get_entity().entity_type.resource_name,
                event.owner.gameprofile.name
            );

            event
                .owner
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<EntityTargetEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTargetEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let target_name = event
                .target
                .as_ref()
                .map(|t| t.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let reason = event.reason.unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityTargetEvent: entity={}, target={}, reason={}",
                event.entity.get_entity().entity_type.resource_name,
                target_name,
                reason
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityTargetLivingEntityEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTargetLivingEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityTargetLivingEntityEvent: entity={}, target={}",
                event.entity.get_entity().entity_type.resource_name,
                event.target.get_entity().entity_type.resource_name
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityPickupItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityPickupItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityPickupItemEvent: entity={}, item={}, amount={}",
                event.entity.get_entity().entity_type.resource_name,
                event.item.item.registry_key,
                event.amount
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityShootBowEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityShootBowEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let consumable_name = event
                .consumable
                .as_ref()
                .map(|i| i.item.registry_key.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityShootBowEvent: player={}, projectile={}, bow={}, consumable={}, force={}",
                event.player.gameprofile.name,
                event.projectile.get_entity().entity_type.resource_name,
                event.bow.item.registry_key,
                consumable_name,
                event.force
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<EntityCombustByEntityEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityCombustByEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityCombustByEntityEvent: entity={}, combuster={}, duration={}",
                event.entity.get_entity().entity_type.resource_name,
                event.combuster.get_entity().entity_type.resource_name,
                event.duration
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityExplodeEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityExplodeEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let entity_name = event
                .entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityExplodeEvent: entity={}, blocks={}, yield={}",
                entity_name,
                event.affected_blocks.len(),
                event.yield_
            );

            self.log(&message);
        })
    }
}

impl EventHandler<ExplosionPrimeEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ExplosionPrimeEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let entity_name = event
                .entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] ExplosionPrimeEvent: entity={}, radius={}, fire={}",
                entity_name, event.radius, event.fire
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityTransformEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTransformEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let reason = event.reason.unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityTransformEvent: entity={}, transform_to={}, reason={}",
                event.entity.get_entity().entity_type.resource_name,
                event.transform_to.resource_name,
                reason
            );

            self.log(&message);
        })
    }
}

impl EventHandler<PotionSplashEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a PotionSplashEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let hit_entity_name = event
                .hit_entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] PotionSplashEvent: entity={}, hit_entity={}, affected={}, potion={}",
                event.entity.get_entity().entity_type.resource_name,
                hit_entity_name,
                event.affected_entities.len(),
                event.potion.item.registry_key
            );

            self.log(&message);
        })
    }
}

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(CabbagePlugin::new())
}
