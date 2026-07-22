#![allow(non_snake_case)] // Preserve the requested `Cabbage.dll` artifact casing.

use std::{mem::MaybeUninit, path::Path, sync::Arc};

use pumpkin::plugin::{Context, PLUGIN_API_VERSION, Plugin, PluginFuture, PluginMetadata};

mod commands;
mod drops;
mod event_log;
mod metrics;

use commands::{CABBAGE_NAMES, CLEAR_DROPS_NAMES, EVENTS_NAMES, METRICS_NAMES};
use drops::{ClearDropsState, DroppedItemCleanupState};
use event_log::EventLogState;
use metrics::MetricsReporterState;

/// The combined DLL has one plugin identity, data folder, and permission
/// namespace.
const PLUGIN_NAME: &str = "Cabbage";
const EVENT_LOG_FILE: &str = "output.log";
const SPLIT_CORE_DATA_FOLDER: &str = "Cabbage.Core";
const SPLIT_MMO_DATA_FOLDER: &str = "Cabbage.Mmo";

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
        description: "Cabbage server utilities, MMO skilling, and Mob AI.".to_string(),
        dependencies: Vec::new(),
        permissions: Vec::new(),
    };

    unsafe {
        core::ptr::addr_of_mut!(METADATA)
            .cast::<PluginMetadata>()
            .write(metadata);
    }
}

struct CabbageCorePlugin {
    clear_drops_state: Arc<ClearDropsState>,
    dropped_item_cleanup_state: Arc<DroppedItemCleanupState>,
    metrics_reporter_state: Arc<MetricsReporterState>,
    event_log_state: Arc<EventLogState>,
    mmo_module: cabbage_mmo::MmoModule,
    mob_ai_module: cabbage_mobai::MobAiModule,
}

impl CabbageCorePlugin {
    fn new() -> Self {
        Self {
            clear_drops_state: Arc::new(ClearDropsState::default()),
            dropped_item_cleanup_state: Arc::new(DroppedItemCleanupState::default()),
            metrics_reporter_state: Arc::new(MetricsReporterState::new()),
            event_log_state: Arc::new(EventLogState::default()),
            mmo_module: cabbage_mmo::MmoModule::default(),
            mob_ai_module: cabbage_mobai::MobAiModule::default(),
        }
    }
}

fn copy_split_file_if_missing(source: &Path, destination: &Path) {
    if destination.exists() || !source.exists() {
        return;
    }
    match std::fs::copy(source, destination) {
        Ok(_) => println!(
            "[Cabbage] Migrated split-plugin data {} to {}",
            source.display(),
            destination.display()
        ),
        Err(error) => println!(
            "[Cabbage] Failed to migrate split-plugin data {}: {error}",
            source.display()
        ),
    }
}

/// Adopts files from the short-lived split-plugin layout. Sources are copied
/// and left untouched so an administrator can verify the unified folder
/// before removing old backups.
fn migrate_split_data_folder(data_folder: &Path) {
    let Some(plugins_folder) = data_folder.parent() else {
        return;
    };
    if let Err(error) = std::fs::create_dir_all(data_folder) {
        println!(
            "[Cabbage] Failed to create data folder {}: {error}",
            data_folder.display()
        );
        return;
    }

    let core_folder = plugins_folder.join(SPLIT_CORE_DATA_FOLDER);
    let mmo_folder = plugins_folder.join(SPLIT_MMO_DATA_FOLDER);
    let config_path = data_folder.join("config.ron");
    let config_was_missing = !config_path.exists();

    // The MMO config is the complete unified config shape, so prefer it when
    // both split folders contain a config.
    copy_split_file_if_missing(&mmo_folder.join("config.ron"), &config_path);
    copy_split_file_if_missing(&core_folder.join("config.ron"), &config_path);
    copy_split_file_if_missing(
        &mmo_folder.join("config.json"),
        &data_folder.join("config.json"),
    );
    copy_split_file_if_missing(&mmo_folder.join("mmo.db"), &data_folder.join("mmo.db"));
    copy_split_file_if_missing(
        &mmo_folder.join("mmo-audit.log"),
        &data_folder.join("mmo-audit.log"),
    );
    copy_split_file_if_missing(
        &core_folder.join(EVENT_LOG_FILE),
        &data_folder.join(EVENT_LOG_FILE),
    );

    // If the MMO config was just adopted, retain the newer Core switches too.
    if config_was_missing
        && let (Ok(unified), Ok(core)) = (
            std::fs::read_to_string(&config_path),
            std::fs::read_to_string(core_folder.join("config.ron")),
        )
        && let (Ok(mut unified), Ok(core)) = (
            ron::from_str::<cabbage_mmo::PluginConfig>(&unified),
            ron::from_str::<cabbage_mmo::PluginConfig>(&core),
        )
    {
        unified.metrics_log = core.metrics_log;
        unified.mob_ai = core.mob_ai;
        if let Ok(contents) =
            ron::ser::to_string_pretty(&unified, ron::ser::PrettyConfig::default())
            && let Err(error) = std::fs::write(&config_path, contents)
        {
            println!(
                "[Cabbage] Failed to merge split Core config into {}: {error}",
                config_path.display()
            );
        }
    }
}

impl Plugin for CabbageCorePlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let data_folder = context.get_data_folder();
            migrate_split_data_folder(&data_folder);
            self.metrics_reporter_state.set_context(context.clone());

            let event_log_state = self.event_log_state.clone();
            context
                .register_service(
                    cabbage_api::CORE_SERVICE,
                    Arc::new(cabbage_api::CoreServices {
                        data_folder: data_folder.clone(),
                        log_event: Arc::new(move |message: &str| {
                            event_log_state.log(message);
                        }),
                    }),
                )
                .await;

            commands::register_commands(
                &context,
                &self.clear_drops_state,
                &self.metrics_reporter_state,
                &self.event_log_state,
            )
            .await?;

            self.metrics_reporter_state.load_config(data_folder.clone());

            let log_path = data_folder.join(EVENT_LOG_FILE);
            self.event_log_state.set_log_path(log_path);

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
            metrics::register(&context, &self.metrics_reporter_state).await;
            event_log::register(&context, &self.event_log_state).await;

            // Feature crates retain independent state and registration code,
            // but are linked into this one native plugin DLL.
            self.mmo_module.load(context.clone()).await;
            self.mob_ai_module.load(&context).await;

            Ok(())
        })
    }

    fn on_unload(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.mmo_module.unload(&context).await;
            self.mob_ai_module.unload();

            // Event handlers are never auto-removed and the DLL stays mapped,
            // so gate every handler on the active flag.
            self.clear_drops_state.set_active(false);
            self.dropped_item_cleanup_state.set_active(false);
            self.metrics_reporter_state.set_active(false);
            self.event_log_state.set_active(false);

            context.unregister_command(CABBAGE_NAMES[0]).await;
            context.unregister_command(CLEAR_DROPS_NAMES[0]).await;
            context.unregister_command(METRICS_NAMES[0]).await;
            context.unregister_command(EVENTS_NAMES[0]).await;
            Ok(())
        })
    }
}

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(CabbageCorePlugin::new())
}
