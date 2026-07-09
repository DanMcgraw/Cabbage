use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use pumpkin::{
    plugin::{
        BoxFuture, Context, EventHandler,
        api::events::{
            block::block_break::BlockBreakEvent, entity::entity_death::EntityDeathEvent,
        },
        server::server_tick_start::ServerTickStartEvent,
    },
    server::Server,
};
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionLvl};

pub(crate) mod commands;
pub(crate) mod config;
pub(crate) mod db;
pub(crate) mod events;
pub(crate) mod player;
pub(crate) mod skills;

pub use commands::{MMO_NAMES, MMO_PERMISSION, mmo_command_tree};
pub use config::{LevelCurve, MmoConfig, PluginConfig, SkillConfig};
pub use skills::SkillId;

use commands::MMO_ADMIN_PERMISSION;
use config::LegacyPluginConfig;
use db::MmoDatabase;

const CONFIG_FILE: &str = "config.ron";
const LEGACY_CONFIG_FILE: &str = "config.json";

/// Shared state for the MMO levelling module.
pub struct MmoState {
    config: Mutex<MmoConfig>,
    db: Arc<MmoDatabase>,
    data_folder: PathBuf,
    curves: Mutex<HashMap<SkillId, LevelCurve>>,
}

impl MmoState {
    /// Initialize the MMO module: load or create config, open the database, and compute curves.
    pub async fn new(data_folder: PathBuf) -> Result<Arc<Self>, String> {
        let plugin_config = load_plugin_config(&data_folder)?;
        let mmo_config = plugin_config.mmo.clone().unwrap_or_default();

        let db = Arc::new(MmoDatabase::open(data_folder.clone())?);
        let curves = build_curves(&mmo_config);

        Ok(Arc::new(Self {
            config: Mutex::new(mmo_config),
            db,
            data_folder,
            curves: Mutex::new(curves),
        }))
    }

    pub fn is_enabled(&self) -> bool {
        self.config.lock().map(|c| c.enabled).unwrap_or(false)
    }

    pub fn config(&self) -> MmoConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn curve(&self, skill: SkillId) -> LevelCurve {
        self.curves
            .lock()
            .map(|c| {
                c.get(&skill)
                    .cloned()
                    .unwrap_or_else(|| LevelCurve::new(&SkillConfig::default()))
            })
            .unwrap_or_else(|_| LevelCurve::new(&SkillConfig::default()))
    }

    pub fn db(&self) -> Arc<MmoDatabase> {
        self.db.clone()
    }

    /// Reload the RON config and rebuild levelling curves.
    pub async fn reload_config(&self) -> Result<(), String> {
        let plugin_config = load_plugin_config(&self.data_folder)?;
        let mmo_config = plugin_config.mmo.clone().unwrap_or_default();

        if let Ok(mut guard) = self.config.lock() {
            *guard = mmo_config.clone();
        }
        if let Ok(mut guard) = self.curves.lock() {
            *guard = build_curves(&mmo_config);
        }

        Ok(())
    }
}

fn build_curves(config: &MmoConfig) -> HashMap<SkillId, LevelCurve> {
    let mut curves = HashMap::new();
    for skill in SkillId::ALL {
        let skill_config = config.skills.get(skill).cloned().unwrap_or_default();
        curves.insert(*skill, LevelCurve::new(&skill_config));
    }
    curves
}

fn load_plugin_config(data_folder: &PathBuf) -> Result<PluginConfig, String> {
    let ron_path = data_folder.join(CONFIG_FILE);
    let json_path = data_folder.join(LEGACY_CONFIG_FILE);

    if let Ok(contents) = fs::read_to_string(&ron_path) {
        ron::from_str::<PluginConfig>(&contents)
            .map_err(|e| format!("failed to parse {CONFIG_FILE}: {e}"))
    } else if let Ok(contents) = fs::read_to_string(&json_path) {
        let json_config = serde_json::from_str::<LegacyPluginConfig>(&contents)
            .map_err(|e| format!("failed to parse legacy {LEGACY_CONFIG_FILE}: {e}"))?;
        let plugin_config = PluginConfig::from(json_config);
        save_plugin_config(data_folder, &plugin_config)?;
        Ok(plugin_config)
    } else {
        let default = PluginConfig::default();
        save_plugin_config(data_folder, &default)?;
        Ok(default)
    }
}

fn save_plugin_config(data_folder: &PathBuf, config: &PluginConfig) -> Result<(), String> {
    fs::create_dir_all(data_folder).map_err(|e| format!("failed to create data folder: {e}"))?;
    let ron_path = data_folder.join(CONFIG_FILE);
    let contents = ron::ser::to_string_pretty(config, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("failed to serialize {CONFIG_FILE}: {e}"))?;
    fs::write(&ron_path, contents).map_err(|e| format!("failed to write {CONFIG_FILE}: {e}"))
}

/// Register MMO permissions with Pumpkin.
pub async fn register_permissions(context: &Arc<Context>) -> Result<(), String> {
    let perms = [
        Permission::new(
            MMO_PERMISSION,
            "Use Cabbage MMO commands.",
            PermissionDefault::Allow,
        ),
        Permission::new(
            MMO_ADMIN_PERMISSION,
            "Administer Cabbage MMO settings and player XP.",
            PermissionDefault::Op(PermissionLvl::Three),
        ),
    ];

    for permission in perms {
        match context.register_permission(permission).await {
            Ok(()) => {}
            Err(error) if error.contains("already registered") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

impl EventHandler<BlockBreakEvent> for MmoState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockBreakEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }
            events::handle_block_break(
                self.db(),
                self.config().message_on_level_up,
                self.curve(SkillId::Mining),
                event,
            )
            .await;
        })
    }
}

impl EventHandler<EntityDeathEvent> for MmoState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a EntityDeathEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }
            events::handle_entity_death(
                self.db(),
                self.config().message_on_level_up,
                self.curve(SkillId::Combat),
                server.clone(),
                event,
            )
            .await;
        })
    }
}

impl EventHandler<ServerTickStartEvent> for MmoState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        _event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // Currently writes happen immediately on events. A periodic flush or
            // online-player cache expiry could be added here in the future.
        })
    }
}
