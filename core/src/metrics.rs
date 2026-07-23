use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use cabbage_api::{MobAiApi, MobAiMetricsSnapshot};
use pumpkin::{
    command::CommandSender,
    plugin::{
        BoxFuture, Context, EventHandler, EventPriority,
        server::server_tick_start::ServerTickStartEvent,
    },
    server::Server,
};
use pumpkin_util::{
    permission::PermissionLvl,
    text::{TextComponent, color::NamedColor},
};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};

use crate::drops::SavedPumpData;

fn default_true() -> bool {
    true
}

/// Core-only switches written to `config.ron` on a fresh install. The file
/// is otherwise read and round-tripped as `cabbage_mmo::PluginConfig` so a
/// legacy unified `mmo:` section survives `/metrics log` toggles; the MMO
/// module's own settings live in `mmo.ron`, `mmo.rewards.ron`, and
/// `mmo.ores.ron`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct CoreConfig {
    #[serde(default)]
    metrics_log: bool,
    #[serde(default = "default_true")]
    mob_ai: bool,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            metrics_log: false,
            mob_ai: true,
        }
    }
}

/// Write a Core-only `config.ron` (fresh install). Failures are logged and
/// otherwise ignored: the in-memory defaults still apply for this run.
fn write_core_config(path: &Path, config: &CoreConfig) {
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        println!(
            "[Cabbage] Failed to create config folder {}: {error}",
            parent.display()
        );
        return;
    }

    let Ok(contents) = ron::ser::to_string_pretty(config, ron::ser::PrettyConfig::default()) else {
        return;
    };

    if let Err(error) = fs::write(path, contents) {
        println!(
            "[Cabbage] Failed to write config {}: {error}",
            path.display()
        );
    }
}

pub(crate) struct MetricsReporterState {
    sys: Arc<Mutex<System>>,
    pid: Pid,
    pub(crate) cached_map_chunks: Arc<AtomicUsize>,
    metrics_log: AtomicBool,
    config_path: Mutex<Option<PathBuf>>,
    mob_ai: Mutex<Option<Arc<dyn MobAiApi>>>,
    /// Last configured Mob AI flag; applied whenever the MobAi service is
    /// (re)discovered so late-loading plugins still honor the config.
    configured_mob_ai: AtomicBool,
    /// Plugin context used to retry the MobAi service lookup after load.
    context: Mutex<Option<Arc<Context>>>,
    /// False once the plugin is unloaded; handlers must early-return then.
    active: AtomicBool,
    last_paths_completed: std::sync::atomic::AtomicUsize,
    last_velocities_completed: std::sync::atomic::AtomicUsize,
    last_metrics_time: Mutex<std::time::Instant>,

    last_app_ram_bytes: Arc<AtomicU64>,
    last_chunk_ram_bytes: Arc<AtomicU64>,
    ram_scanning: Arc<AtomicBool>,
}

impl MetricsReporterState {
    pub(crate) fn new() -> Self {
        let sys = System::new();
        let pid = get_current_pid().expect("Failed to get current process ID");
        Self {
            sys: Arc::new(Mutex::new(sys)),
            pid,
            cached_map_chunks: Arc::new(AtomicUsize::new(0)),
            metrics_log: AtomicBool::new(false),
            config_path: Mutex::new(None),
            mob_ai: Mutex::new(None),
            configured_mob_ai: AtomicBool::new(true),
            context: Mutex::new(None),
            active: AtomicBool::new(true),
            last_paths_completed: std::sync::atomic::AtomicUsize::new(0),
            last_velocities_completed: std::sync::atomic::AtomicUsize::new(0),
            last_metrics_time: Mutex::new(std::time::Instant::now()),

            last_app_ram_bytes: Arc::new(AtomicU64::new(0)),
            last_chunk_ram_bytes: Arc::new(AtomicU64::new(0)),
            ram_scanning: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Connects the Mob AI engine. Until this is set, metrics report zeros
    /// for the Mob AI fields.
    pub(crate) fn set_mob_ai(&self, api: Arc<dyn MobAiApi>) {
        if let Ok(mut mob_ai) = self.mob_ai.lock() {
            *mob_ai = Some(api);
        }
    }

    /// Stores the plugin context so the MobAi service lookup can be retried
    /// when the MobAi plugin loads after Core.
    pub(crate) fn set_context(&self, context: Arc<Context>) {
        if let Ok(mut guard) = self.context.lock() {
            *guard = Some(context);
        }
    }

    pub(crate) fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::SeqCst);
    }

    fn mob_ai_api(&self) -> Option<Arc<dyn MobAiApi>> {
        self.mob_ai.lock().ok().and_then(|mob_ai| mob_ai.clone())
    }

    /// Retries the MobAi service lookup while the service is absent. Runtime
    /// `/plugin load` skips dependency resolution, so MobAi can appear after
    /// Core has already loaded.
    async fn ensure_mob_ai_service(&self) {
        if self.mob_ai_api().is_some() {
            return;
        }
        let Some(context) = self.context.lock().ok().and_then(|c| c.clone()) else {
            return;
        };
        if let Some(service) = context
            .get_service::<cabbage_api::MobAiService>(cabbage_api::MOB_AI_SERVICE)
            .await
        {
            service
                .0
                .set_enabled(self.configured_mob_ai.load(Ordering::SeqCst));
            self.set_mob_ai(service.0.clone());
        }
    }
}

pub(crate) async fn register(context: &Arc<Context>, metrics_state: &Arc<MetricsReporterState>) {
    context
        .register_event::<ServerTickStartEvent, _>(
            metrics_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
}

pub(crate) fn metrics_command_allowed(sender: &CommandSender) -> bool {
    matches!(sender, CommandSender::Console | CommandSender::Rcon(_))
        || (sender.is_player() && sender.has_permission_lvl(PermissionLvl::Two))
}

pub(crate) fn metrics_log_message(enabled: bool) -> TextComponent {
    let (state, color) = if enabled {
        ("on", NamedColor::Green)
    } else {
        ("off", NamedColor::Red)
    };

    TextComponent::text("Turning metric logging ")
        .add_child(TextComponent::text(state).color_named(color))
        .add_text(".")
}

pub(crate) fn spawn_disk_scan(
    worlds: Vec<Arc<pumpkin::world::World>>,
    cached_map_chunks: Arc<AtomicUsize>,
) {
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

fn zero_mob_ai_metrics() -> MobAiMetricsSnapshot {
    MobAiMetricsSnapshot {
        active_path_jobs: 0,
        active_velocity_jobs: 0,
        total_worker_threads: 0,
        managed_mobs_count: 0,
        total_paths_completed: 0,
        total_velocities_completed: 0,
    }
}

pub(crate) struct MetricsSnapshot {
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
    pub(crate) fn format(&self) -> String {
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
    pub(crate) fn load_config(&self, data_folder: PathBuf) {
        let path = data_folder.join("config.ron");
        let config = match fs::read_to_string(&path) {
            Ok(contents) => {
                ron::from_str::<cabbage_mmo::PluginConfig>(&contents).unwrap_or_default()
            }
            Err(_) => {
                // Fresh install: Core owns `config.ron` and seeds it with
                // just the core switches. When a legacy `config.json` is
                // still waiting to be adopted, leave the file creation to
                // the MMO module so the json switches survive.
                if !data_folder.join("config.json").exists() {
                    write_core_config(&path, &CoreConfig::default());
                }
                cabbage_mmo::PluginConfig::default()
            }
        };

        self.metrics_log.store(config.metrics_log, Ordering::SeqCst);
        self.configured_mob_ai
            .store(config.mob_ai, Ordering::SeqCst);
        if let Some(mob_ai) = self.mob_ai_api() {
            mob_ai.set_enabled(config.mob_ai);
        }

        if let Ok(mut config_path) = self.config_path.lock() {
            *config_path = Some(path);
        }
    }

    pub(crate) fn toggle_metrics_log(&self) -> bool {
        let enabled = !self.metrics_log.fetch_xor(true, Ordering::SeqCst);
        let mob_ai = self.mob_ai_api().map(|mob_ai| mob_ai.is_enabled());
        self.save_config(enabled, mob_ai);
        enabled
    }

    fn save_config(&self, metrics_log: bool, mob_ai: Option<bool>) {
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
            .and_then(|contents| ron::from_str::<cabbage_mmo::PluginConfig>(&contents).ok())
            .unwrap_or_default();
        config.metrics_log = metrics_log;
        if let Some(mob_ai) = mob_ai {
            config.mob_ai = mob_ai;
        }

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

    pub(crate) fn collect_metrics(&self, server: &Server) -> MetricsSnapshot {
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

        let mob_ai_metrics = self
            .mob_ai_api()
            .map(|mob_ai| mob_ai.metrics())
            .unwrap_or_else(zero_mob_ai_metrics);

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
            if !self.active.load(Ordering::SeqCst) {
                return;
            }

            self.ensure_mob_ai_service().await;

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
                        sys_lock.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn test_config_folder() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("cabbage_core_config_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&path);
        path
    }

    fn cleanup(folder: &PathBuf) {
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn unified_plugin_config_preserves_mmo_settings() {
        let mut original = cabbage_mmo::PluginConfig::default();
        original.metrics_log = true;
        original.mob_ai = false;
        original.mmo = Some(cabbage_mmo::MmoConfig::default());
        original.mmo.as_mut().unwrap().enabled = false;
        let serialized = ron::to_string(&original).unwrap();
        let config: cabbage_mmo::PluginConfig = ron::from_str(&serialized).unwrap();
        assert!(config.metrics_log);
        assert!(!config.mob_ai);
        assert!(!config.mmo.unwrap().enabled);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let config: cabbage_mmo::PluginConfig = ron::from_str("()").unwrap();
        assert_eq!(config, cabbage_mmo::PluginConfig::default());
    }

    #[test]
    fn plugin_config_without_mmo_section_serializes_core_only() {
        let config = cabbage_mmo::PluginConfig::default();
        assert_eq!(config.mmo, None);
        let serialized = ron::to_string(&config).unwrap();
        assert!(!serialized.contains("mmo:"));
    }

    #[test]
    fn fresh_install_creates_core_only_config() {
        let folder = test_config_folder();
        let state = MetricsReporterState::new();
        state.load_config(folder.clone());

        let text = std::fs::read_to_string(folder.join("config.ron")).unwrap();
        assert!(!text.contains("mmo:"));
        let config: cabbage_mmo::PluginConfig = ron::from_str(&text).unwrap();
        assert!(!config.metrics_log);
        assert!(config.mob_ai);
        assert_eq!(config.mmo, None);
        // The in-memory switches follow the written defaults.
        assert!(!state.metrics_log.load(Ordering::SeqCst));

        cleanup(&folder);
    }

    #[test]
    fn legacy_config_json_defers_file_creation_to_the_mmo_module() {
        let folder = test_config_folder();
        std::fs::write(
            folder.join("config.json"),
            "{\"metrics_log\":true,\"mob_ai\":false}",
        )
        .unwrap();
        let state = MetricsReporterState::new();
        state.load_config(folder.clone());

        // Core must not overwrite the pending json adoption with defaults.
        assert!(!folder.join("config.ron").exists());

        cleanup(&folder);
    }

    #[test]
    fn toggle_on_core_only_file_stays_core_only() {
        let folder = test_config_folder();
        let state = MetricsReporterState::new();
        state.load_config(folder.clone());

        assert!(state.toggle_metrics_log());

        let text = std::fs::read_to_string(folder.join("config.ron")).unwrap();
        assert!(!text.contains("mmo:"));
        let config: cabbage_mmo::PluginConfig = ron::from_str(&text).unwrap();
        assert!(config.metrics_log);
        assert!(config.mob_ai);
        assert_eq!(config.mmo, None);

        cleanup(&folder);
    }

    #[test]
    fn toggle_preserves_legacy_mmo_section() {
        let folder = test_config_folder();
        std::fs::write(
            folder.join("config.ron"),
            "(metrics_log:false,mob_ai:true,mmo:Some((\
             enabled:false,config_version:2,message_on_level_up:true,save_interval_ticks:6000,\
             skills:{},disabled_world_features:[],reward_config_version:1)))",
        )
        .unwrap();
        let state = MetricsReporterState::new();
        state.load_config(folder.clone());

        assert!(state.toggle_metrics_log());

        let text = std::fs::read_to_string(folder.join("config.ron")).unwrap();
        let config: cabbage_mmo::PluginConfig = ron::from_str(&text).unwrap();
        assert!(config.metrics_log);
        assert!(config.mob_ai);
        let mmo = config.mmo.expect("mmo section must survive the toggle");
        assert!(!mmo.enabled);
        assert_eq!(mmo.save_interval_ticks, 6000);

        cleanup(&folder);
    }
}
