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

/// The combined plugin keeps the former Core identity so existing Core config
/// and event-log files continue to live in `plugins/Cabbage.Core`.
const PLUGIN_NAME: &str = "Cabbage.Core";
const EVENT_LOG_FILE: &str = "output.log";
/// Data folder of the pre-split monolithic `Cabbage` plugin. Existing files
/// are migrated (copied, never deleted) on first load.
pub(crate) const LEGACY_DATA_FOLDER: &str = "Cabbage";

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

/// Copies the legacy monolithic plugin's event log into this plugin's data
/// folder when the new file does not exist yet. The legacy file is left in
/// place.
fn migrate_legacy_event_log(log_path: &Path) {
    if log_path.exists() {
        return;
    }
    let Some(legacy_path) = log_path
        .parent()
        .and_then(Path::parent)
        .map(|plugins_dir| plugins_dir.join(LEGACY_DATA_FOLDER).join(EVENT_LOG_FILE))
    else {
        return;
    };
    if !legacy_path.exists() {
        return;
    }

    match std::fs::copy(&legacy_path, log_path) {
        Ok(_) => println!(
            "[Cabbage.Core] Migrated legacy event log {} to {}",
            legacy_path.display(),
            log_path.display()
        ),
        Err(error) => println!(
            "[Cabbage.Core] Failed to migrate legacy event log {}: {error}",
            legacy_path.display()
        ),
    }
}

impl Plugin for CabbageCorePlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.metrics_reporter_state.set_context(context.clone());

            let event_log_state = self.event_log_state.clone();
            context
                .register_service(
                    cabbage_api::CORE_SERVICE,
                    Arc::new(cabbage_api::CoreServices {
                        data_folder: context.get_data_folder(),
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

            self.metrics_reporter_state
                .load_config(context.get_data_folder());

            let log_path = context.get_data_folder().join(EVENT_LOG_FILE);
            migrate_legacy_event_log(&log_path);
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
