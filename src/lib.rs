use std::{mem::MaybeUninit, sync::Arc};

use pumpkin::plugin::{
    Context, EventPriority, PLUGIN_API_VERSION, Plugin, PluginFuture, PluginMetadata,
    api::events::entity::{
        ChunkEntityLoadEvent, ChunkEntityUnloadEvent, EntityRemoveEvent, EntitySpawnEvent,
        entity_damage::EntityDamageEvent, entity_shoot_bow::EntityShootBowEvent,
        entity_tame::EntityTameEvent, projectile_hit::ProjectileHitEvent,
    },
    api::events::player::{craft_item::CraftItemEvent, furnace_extract::FurnaceExtractEvent},
    server::server_tick_start::ServerTickStartEvent,
};

use cabbage_mmo as mmo;
use cabbage_mobai::MobAiState;

mod commands;
mod drops;
mod event_log;
mod metrics;

use commands::{CABBAGE_NAMES, CLEAR_DROPS_NAMES, EVENTS_NAMES, METRICS_NAMES};
use drops::{ClearDropsState, DroppedItemCleanupState};
use event_log::EventLogState;
use metrics::MetricsReporterState;

const PLUGIN_NAME: &str = "Cabbage";
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

impl Plugin for CabbagePlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            commands::register_commands(
                &context,
                &self.clear_drops_state,
                &self.metrics_reporter_state,
                &self.event_log_state,
            )
            .await?;

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

            metrics::spawn_disk_scan(
                context.server.worlds.load().iter().cloned().collect(),
                self.metrics_reporter_state.cached_map_chunks.clone(),
            );

            drops::register(
                &context,
                &self.clear_drops_state,
                &self.dropped_item_cleanup_state,
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
            metrics::register(&context, &self.metrics_reporter_state).await;

            if let Some(mmo_state) = self.mmo_state.as_ref() {
                if let Err(error) = mmo::register_permissions(&context).await {
                    println!("[Cabbage] Failed to register MMO permissions: {error}");
                }

                context
                    .register_event::<pumpkin::plugin::api::events::block::block_break::BlockBreakEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::block::block_place::BlockPlaceEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::block::block_drop_item::BlockDropItemEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::block::block_broken::BlockBrokenEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<ServerTickStartEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
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
                        true,
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
                    .register_event::<pumpkin::plugin::api::events::entity::entity_breed::EntityBreedCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<EntityTameEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::entity::entity_feed::EntityFeedCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::entity::entity_product::AnimalProductCollectCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::block::bone_meal::BoneMealApplyCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::player::player_item_use_complete::PlayerItemUseCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
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
                        true,
                    )
                    .await;
                context
                    .register_event::<ProjectileHitEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<pumpkin::plugin::api::events::entity::player_kill::PlayerKillEntityEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
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
                    .register_event::<pumpkin::plugin::api::events::player::anvil_repair::AnvilCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
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
                    .register_event::<pumpkin::plugin::api::events::player::grindstone::GrindstoneCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
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
                    .register_event::<pumpkin::plugin::api::events::player::enchant_item::EnchantItemCompleteEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<CraftItemEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_event::<FurnaceExtractEvent, _>(
                        mmo_state.clone(),
                        EventPriority::Normal,
                        true,
                    )
                    .await;
                context
                    .register_command(
                        mmo::mmo_command_tree(mmo_state.clone()),
                        mmo::MMO_PERMISSION,
                    )
                    .await;
            }

            event_log::register(&context, &self.event_log_state).await;

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

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(CabbagePlugin::new())
}
