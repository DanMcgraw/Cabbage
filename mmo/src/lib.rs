use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};

use pumpkin::{
    entity::player::Player,
    plugin::{
        BoxFuture, Context, EventHandler,
        api::PluginTransactionId,
        api::events::{
            block::{
                block_break::BlockBreakEvent, block_broken::BlockBrokenEvent,
                block_drop_item::BlockDropItemEvent, block_place::BlockPlaceEvent,
                bone_meal::BoneMealApplyCompleteEvent,
            },
            entity::{
                entity_breed::EntityBreedCompleteEvent, entity_damage::EntityDamageEvent,
                entity_damage_by_entity::EntityDamageByEntityEvent,
                entity_feed::EntityFeedCompleteEvent,
                entity_product::AnimalProductCollectCompleteEvent,
                entity_shoot_bow::EntityShootBowEvent, entity_tame::EntityTameEvent,
                player_kill::PlayerKillEntityEvent, projectile_hit::ProjectileHitEvent,
            },
            player::{
                anvil_prepare::AnvilPrepareEvent,
                anvil_repair::AnvilCompleteEvent,
                craft_item::CraftItemEvent,
                enchant_item::EnchantItemCompleteEvent,
                enchant_item_generate::EnchantItemGenerateEvent,
                fish::PlayerFishEvent,
                furnace_extract::FurnaceExtractEvent,
                grindstone::{GrindstoneCompleteEvent, GrindstoneEvent},
                player_attack::PlayerAttackDamageEvent,
                player_interact_event::PlayerInteractEvent,
                player_item_use_complete::PlayerItemUseCompleteEvent,
            },
            world::feature_generate::FeatureGenerateEvent,
        },
        server::server_tick_start::ServerTickStartEvent,
    },
    server::Server,
};
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionLvl};

pub(crate) mod audit;
pub(crate) mod commands;
pub mod config;
pub(crate) mod db;
pub(crate) mod enterprise;
pub(crate) mod frontier;
pub(crate) mod ore_reveal;
pub(crate) mod perks;
pub(crate) mod persistence;
mod plugin;
pub(crate) mod progression;
pub(crate) mod skills;
pub(crate) mod ui;
pub(crate) mod warfare;

pub use commands::{MMO_NAMES, MMO_PERMISSION, mmo_command_tree};
pub use config::{LevelCurve, MmoConfig, PluginConfig, SkillConfig};
pub use plugin::MmoModule;
pub use skills::SkillId;

use commands::MMO_ADMIN_PERMISSION;
use config::{CURRENT_CONFIG_VERSION, LegacyPluginConfig, XpRewardsConfig};
use db::MmoDatabase;
use ore_reveal::config::OreRevealConfig;
use ore_reveal::provenance::{ProvenanceKey, ProvenanceTracker};
use perks::CooldownTracker;
use ui::BossbarState;

const CONFIG_FILE: &str = "config.ron";
const LEGACY_CONFIG_FILE: &str = "config.json";
const MMO_CONFIG_FILE: &str = "mmo.ron";
const REWARDS_CONFIG_FILE: &str = "mmo.rewards.ron";
const ORE_REVEAL_CONFIG_FILE: &str = "mmo.ores.ron";
/// Short-lived `mmo/` subfolder layout, superseded by the flat `mmo.*.ron`
/// files above. Subfolder files are still read (adopted) when the flat file
/// is missing and are never modified or deleted.
const LEGACY_MMO_SUBFOLDER: &str = "mmo";
const LEGACY_REWARDS_CONFIG_FILE: &str = "rewards.ron";
const LEGACY_ORE_REVEAL_CONFIG_FILE: &str = "ore_reveal.ron";

/// Shared state for the MMO levelling module.
pub struct MmoState {
    context: Arc<Context>,
    config: Mutex<MmoConfig>,
    db: Arc<MmoDatabase>,
    data_folder: PathBuf,
    curves: Mutex<HashMap<SkillId, LevelCurve>>,
    bossbar_state: BossbarState,
    ore_reveal_state: ore_reveal::OreRevealState,
    provenance: ProvenanceTracker,
    perk_cooldowns: CooldownTracker,
    perk_previews: Mutex<HashMap<(PluginTransactionId, &'static str), i32>>,
    warfare_state: warfare::WarfareState,
    audit_log: audit::AuditLog,
    last_tick: AtomicI32,
    /// False once the plugin is unloaded; event handlers must early-return
    /// then (handlers are never auto-removed and the DLL stays mapped).
    active: AtomicBool,
}

impl MmoState {
    /// Initialize the MMO module: load or create config, open the database,
    /// run pending migrations, and compute curves.
    pub async fn new(context: Arc<Context>) -> Result<Arc<Self>, String> {
        Self::new_in_data_folder(context.clone(), context.get_data_folder()).await
    }

    /// Initializes the MMO state with an explicit data folder. The combined
    /// plugin uses this to retain the former standalone module's data folder.
    pub async fn new_in_data_folder(
        context: Arc<Context>,
        data_folder: PathBuf,
    ) -> Result<Arc<Self>, String> {
        let loaded = load_mmo_config(&data_folder)?;
        let mut mmo_config = loaded.config.sanitized();
        let mut config_dirty = loaded.split_files_missing;

        let db = Arc::new(MmoDatabase::open(data_folder.clone())?);
        let legacy_rewards = db.load_legacy_xp_rewards().await?;
        let migrate_legacy_rewards = mmo_config.reward_config_version == 0;
        if migrate_legacy_rewards {
            if let Some(legacy) = legacy_rewards.as_ref() {
                if !legacy.mobs.is_empty() {
                    mmo_config.xp_rewards.mobs = legacy.mobs.clone();
                }
                if !legacy.blocks.is_empty() {
                    mmo_config.xp_rewards.blocks = legacy.blocks.clone();
                }
            }
            mmo_config.reward_config_version = 1;
            config_dirty = true;
        }
        if legacy_rewards.is_some() {
            db.drop_legacy_xp_reward_tables().await?;
            if migrate_legacy_rewards {
                log::info!("[Cabbage MMO] migrated XP rewards from SQLite to mmo.rewards.ron");
            } else {
                log::info!("[Cabbage MMO] removed obsolete SQLite XP reward tables");
            }
        }

        // Config schema upgrade: retired pair entries were already merged
        // into their canonical destinations while deserializing (config v2);
        // here we fill any missing skill curves so the saved file documents
        // every skill, and record the current version.
        if upgrade_mmo_config(&mut mmo_config) {
            config_dirty = true;
        }
        if config_dirty {
            save_mmo_config_files(&data_folder, &mmo_config)?;
        }

        // Legacy Combat XP: if the admin configured a destination skill, run
        // the one-time migration now; otherwise it stays preserved until an
        // administrator chooses one (see `/mmo migrate`).
        if let Some(target) = mmo_config.combat_migration.target {
            let status = db.combat_migration_status().await?;
            if status.players_with_legacy_xp > 0 {
                let outcome = db.migrate_combat_xp(target).await?;
                log::info!(
                    "[Cabbage MMO] migrated legacy Combat XP to {target}: \
                     {} player(s), {} XP",
                    outcome.players_migrated,
                    outcome.xp_moved
                );
            }
        }

        let non_natural = db.load_non_natural_blocks().await?;
        let ore_reveal_state = ore_reveal::OreRevealState::new(&mmo_config.ore_reveal)?;
        let curves = build_curves(&mmo_config);
        let audit_log = audit::AuditLog::new(data_folder.clone())?;

        Ok(Arc::new(Self {
            context,
            config: Mutex::new(mmo_config),
            db,
            data_folder,
            curves: Mutex::new(curves),
            bossbar_state: BossbarState::new(),
            ore_reveal_state,
            provenance: ProvenanceTracker::new(non_natural),
            perk_cooldowns: CooldownTracker::new(),
            perk_previews: Mutex::new(HashMap::new()),
            warfare_state: warfare::WarfareState::new(),
            audit_log,
            last_tick: AtomicI32::new(0),
            active: AtomicBool::new(true),
        }))
    }

    /// Plugin context, for Pumpkin persistent-data stores (player/entity/
    /// block data) used by the persistence codecs.
    #[allow(dead_code)] // used by per-skill handlers landing in Phases 1-3
    pub(crate) fn context(&self) -> &Arc<Context> {
        &self.context
    }

    pub fn is_enabled(&self) -> bool {
        self.config.lock().map(|c| c.enabled).unwrap_or(false)
    }

    /// True while the plugin is loaded. Event handlers early-return once this
    /// flips to false on unload.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
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

    pub(crate) fn db(&self) -> Arc<MmoDatabase> {
        self.db.clone()
    }

    pub fn mob_xp_reward(&self, mob_name: &str) -> Option<u64> {
        self.config
            .lock()
            .ok()
            .and_then(|config| config.xp_rewards.mobs.get(mob_name).copied())
    }

    pub fn block_xp_reward(&self, block_name: &str) -> Option<u64> {
        self.config
            .lock()
            .ok()
            .and_then(|config| config.xp_rewards.blocks.get(block_name).copied())
    }

    /// Shared perk cooldown tracker (tick-based).
    #[allow(dead_code)] // consumed by perk handlers landing in Phases 1-3
    pub(crate) fn perk_cooldowns(&self) -> &CooldownTracker {
        &self.perk_cooldowns
    }

    pub(crate) fn mark_perk_preview(&self, transaction: PluginTransactionId, perk: &'static str) {
        if let Ok(mut previews) = self.perk_previews.lock() {
            previews.insert((transaction, perk), self.current_tick());
        }
    }

    pub(crate) fn take_perk_preview(
        &self,
        transaction: PluginTransactionId,
        perk: &'static str,
    ) -> bool {
        self.perk_previews
            .lock()
            .ok()
            .and_then(|mut previews| previews.remove(&(transaction, perk)))
            .is_some()
    }

    /// Shared block provenance tracker (player-placed block denylist).
    ///
    /// Ore-reveal hosts and XP-eligible logs are marked on placement; the
    /// block-broken coordinator takes each key once and passes the result to
    /// every consumer so placed blocks never feed progression.
    pub(crate) fn provenance(&self) -> &ProvenanceTracker {
        &self.provenance
    }

    /// Warfare in-memory state (attack records, projectile provenance, mana).
    pub(crate) fn warfare(&self) -> &warfare::WarfareState {
        &self.warfare_state
    }

    /// Write one line to the MMO audit log (honors the audit config).
    pub(crate) fn audit(&self, message: &str) {
        let config = self.config();
        self.audit_log.log(&config.audit, message);
    }

    /// Last server tick observed by this module.
    pub fn current_tick(&self) -> i32 {
        self.last_tick.load(Ordering::Relaxed)
    }

    /// Show or refresh the skill-progress bossbar for a player.
    ///
    /// Reads the player's current skill data from the database, computes level
    /// progress, and updates the transient bossbar.
    pub async fn show_xp_bossbar(&self, player: &Arc<Player>, skill: SkillId, current_tick: i32) {
        let uuid = player.gameprofile.id;
        let skill_data = match self.db.get_skill(uuid, skill).await {
            Ok(data) => data,
            Err(error) => {
                log::warn!("[Cabbage MMO] failed to read {skill} skill for bossbar: {error}");
                return;
            }
        };

        self.show_xp_bossbar_at_xp(player, skill, skill_data.xp, current_tick)
            .await;
    }

    pub(crate) async fn show_xp_bossbar_at_xp(
        &self,
        player: &Arc<Player>,
        skill: SkillId,
        xp: u64,
        current_tick: i32,
    ) {
        let curve = self.curve(skill);
        let (level, xp_into_level, xp_for_next) = curve.level_for_xp(xp);
        self.bossbar_state
            .show_skill_progress(
                player,
                skill,
                level,
                xp_into_level,
                xp_for_next,
                current_tick,
            )
            .await;
    }

    /// Reload the RON config and rebuild levelling curves.
    pub async fn reload_config(&self) -> Result<(), String> {
        let mmo_config = load_mmo_config(&self.data_folder)?.config.sanitized();
        self.ore_reveal_state.reload(&mmo_config.ore_reveal)?;

        if let Ok(mut guard) = self.config.lock() {
            *guard = mmo_config.clone();
        }
        if let Ok(mut guard) = self.curves.lock() {
            *guard = build_curves(&mmo_config);
        }

        Ok(())
    }
}

/// Callable module-level helper to show a player's skill-progress bossbar.
///
/// This is the preferred public API for other modules or plugins that want to
/// trigger the bossbar without directly calling a method on [`MmoState`].
#[allow(dead_code)] // public API surface for external callers / future commands
pub async fn show_xp_bossbar(state: Arc<MmoState>, player: Arc<Player>, skill: SkillId) {
    let current_tick = state.current_tick();
    state.show_xp_bossbar(&player, skill, current_tick).await;
}

fn build_curves(config: &MmoConfig) -> HashMap<SkillId, LevelCurve> {
    let mut curves = HashMap::new();
    for skill in SkillId::ALL {
        let skill_config = config.skills.get(skill).cloned().unwrap_or_default();
        curves.insert(*skill, LevelCurve::new(&skill_config));
    }
    curves
}

/// Bring an older MMO config schema up to the current version, filling any
/// missing canonical skill curves with defaults. Returns true when the
/// config changed and should be saved back. Retired pair entries are merged
/// during deserialization (see `config::deserialize_skill_configs`), before
/// this runs; this pass is what inserts defaults for canonical skills that
/// had no old or new entry, and it saves exactly once via the caller.
fn upgrade_mmo_config(config: &mut MmoConfig) -> bool {
    if config.config_version >= CURRENT_CONFIG_VERSION {
        return false;
    }
    config.config_version = CURRENT_CONFIG_VERSION;
    for skill in SkillId::ALL {
        config.skills.entry(*skill).or_default();
    }
    true
}

/// Outcome of [`load_mmo_config`]: the assembled config plus whether any
/// split file was missing (and must therefore be written on first load).
struct LoadedMmoConfig {
    config: MmoConfig,
    split_files_missing: bool,
}

/// Load the MMO configuration from the split files with the legacy fallback
/// chain. Shared by initial load and `/mmo reload`; callers decide whether
/// to save missing split files back (initial load does, reload does not).
///
/// Fallback chain:
/// - `MmoConfig`: `mmo.ron`; otherwise the `mmo` section of a legacy unified
///   `config.ron` (or `config.json`, which carries no MMO data and therefore
///   yields defaults); otherwise defaults.
/// - `xp_rewards`: `mmo.rewards.ron`; otherwise the superseded
///   `mmo/rewards.ron` subfolder file; otherwise whatever the `MmoConfig`
///   source carried inline (old unified files embed it); otherwise defaults.
/// - `ore_reveal`: `mmo.ores.ron`; otherwise the superseded
///   `mmo/ore_reveal.ron` subfolder file; otherwise the source's inline
///   value; otherwise defaults.
///
/// Once the split files exist, a stale `mmo:` section in `config.ron` is
/// ignored forever; the legacy unified file and the superseded subfolder
/// files are never modified.
fn load_mmo_config(data_folder: &PathBuf) -> Result<LoadedMmoConfig, String> {
    let mmo_path = data_folder.join(MMO_CONFIG_FILE);
    let rewards_path = data_folder.join(REWARDS_CONFIG_FILE);
    let ore_reveal_path = data_folder.join(ORE_REVEAL_CONFIG_FILE);
    let legacy_rewards_path = data_folder
        .join(LEGACY_MMO_SUBFOLDER)
        .join(LEGACY_REWARDS_CONFIG_FILE);
    let legacy_ore_reveal_path = data_folder
        .join(LEGACY_MMO_SUBFOLDER)
        .join(LEGACY_ORE_REVEAL_CONFIG_FILE);

    let mut config = if let Ok(contents) = fs::read_to_string(&mmo_path) {
        ron::from_str::<MmoConfig>(&contents)
            .map_err(|e| format!("failed to parse {MMO_CONFIG_FILE}: {e}"))?
    } else {
        load_plugin_config(data_folder)?.mmo.unwrap_or_default()
    };

    if let Ok(contents) =
        fs::read_to_string(&rewards_path).or_else(|_| fs::read_to_string(&legacy_rewards_path))
    {
        config.xp_rewards = ron::from_str::<XpRewardsConfig>(&contents)
            .map_err(|e| format!("failed to parse rewards config ({REWARDS_CONFIG_FILE}): {e}"))?;
    }
    if let Ok(contents) = fs::read_to_string(&ore_reveal_path)
        .or_else(|_| fs::read_to_string(&legacy_ore_reveal_path))
    {
        config.ore_reveal = ron::from_str::<OreRevealConfig>(&contents).map_err(|e| {
            format!("failed to parse ore reveal config ({ORE_REVEAL_CONFIG_FILE}): {e}")
        })?;
    }

    let split_files_missing =
        !mmo_path.exists() || !rewards_path.exists() || !ore_reveal_path.exists();
    Ok(LoadedMmoConfig {
        config,
        split_files_missing,
    })
}

/// Read the legacy unified `config.ron` for its core switches and possibly
/// embedded `mmo` section. When only a legacy `config.json` exists, adopt it
/// once: the core switches are written to `config.ron` (the json file itself
/// is never modified or deleted) and the MMO side falls back to defaults.
/// `config.ron` is never written in any other case — Core owns it.
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
        Ok(PluginConfig::default())
    }
}

fn save_plugin_config(data_folder: &PathBuf, config: &PluginConfig) -> Result<(), String> {
    fs::create_dir_all(data_folder).map_err(|e| format!("failed to create data folder: {e}"))?;
    let ron_path = data_folder.join(CONFIG_FILE);
    write_ron_pretty(&ron_path, config)
}

/// Write the split MMO config files (`mmo.ron`, `mmo.rewards.ron`,
/// `mmo.ores.ron`), all flat inside the data folder. Called on first load
/// whenever any of them was missing or the config was upgraded/migrated.
fn save_mmo_config_files(data_folder: &Path, config: &MmoConfig) -> Result<(), String> {
    fs::create_dir_all(data_folder).map_err(|e| format!("failed to create data folder: {e}"))?;
    write_ron_pretty(&data_folder.join(MMO_CONFIG_FILE), config)?;
    write_ron_pretty(&data_folder.join(REWARDS_CONFIG_FILE), &config.xp_rewards)?;
    write_ron_pretty(
        &data_folder.join(ORE_REVEAL_CONFIG_FILE),
        &config.ore_reveal,
    )
}

fn write_ron_pretty<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let contents = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("failed to serialize {}: {e}", path.display()))?;
    fs::write(path, contents).map_err(|e| format!("failed to write {}: {e}", path.display()))
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

/// Cancel feature placement if the feature is in the MMO blacklist.
fn handle_feature_generate(state: &MmoState, event: &mut FeatureGenerateEvent) {
    let feature_name = placed_feature_name(event.feature);
    if should_disable_world_feature(&state.config(), &feature_name) {
        event.cancelled = true;
    }
}

fn should_disable_world_feature(config: &MmoConfig, feature_name: &str) -> bool {
    // Existing config files already contain a serialized blacklist, so adding
    // entries to the default alone would not migrate them. Keep these features
    // tied to the reveal system even for those existing installations:
    // `ore_emerald` (added after the blacklist first shipped) and
    // `ore_gold_extra` (badlands gold, never part of the default blacklist,
    // while the reveal system has always modelled badlands gold itself).
    (config.ore_reveal.enabled
        && (feature_name == "ore_emerald" || feature_name == "ore_gold_extra"))
        || config
            .disabled_world_features
            .iter()
            .any(|name| name == feature_name)
}

/// Convert a `PlacedFeature` enum variant to its snake_case registry name.
fn placed_feature_name(feature: pumpkin_data::placed_feature::PlacedFeature) -> String {
    let pascal = format!("{feature:?}");
    let mut snake = String::with_capacity(pascal.len());
    for (i, ch) in pascal.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(ch.to_ascii_lowercase());
    }
    snake
}

impl EventHandler<BlockBreakEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockBreakEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::mining::handle_block_break(self, event).await;
            frontier::woodcutting::handle_block_break(self, event).await;
            frontier::excavation::handle_block_break(self, event).await;
            enterprise::repair::handle_tool_care(self, event).await;
        })
    }
}

impl EventHandler<BlockPlaceEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockPlaceEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() {
                return;
            }
            if event.cancelled || !event.can_build {
                return;
            }
            let config = self.config();
            let tracked = self.ore_reveal_state.is_host_block(event.block_placed)
                || self.block_xp_reward(event.block_placed.name).is_some()
                || config
                    .frontier
                    .woodcutting
                    .is_tracked_log(event.block_placed.name)
                || config
                    .frontier
                    .herbalism
                    .is_tracked_plant(event.block_placed.name)
                || config
                    .frontier
                    .excavation
                    .is_tracked_diggable(event.block_placed.name);
            if tracked {
                self.provenance.mark(ProvenanceKey::new(
                    &event.player.world(),
                    event.block_position,
                ));
            }
        })
    }
}

impl EventHandler<BlockDropItemEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockDropItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::mining::handle_block_drop_item(self, event).await;
        })
    }
}

impl EventHandler<BlockBrokenEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockBrokenEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() {
                return;
            }
            // Take provenance once: every consumer below sees the same answer
            // for "was this block player-placed?".
            let key = ProvenanceKey::new(&event.world, event.block_position);
            let was_non_natural = self.provenance.take(&key);

            let enabled = self.is_enabled();
            self.ore_reveal_state
                .handle_block_broken(event, enabled, &self.provenance, was_non_natural)
                .await;
            if !enabled {
                return;
            }
            frontier::mining::handle_block_broken(self, event, was_non_natural).await;
            frontier::woodcutting::handle_block_broken(self, event, was_non_natural).await;
            frontier::agriculture::handle_block_broken(self, event).await;
            frontier::herbalism::handle_block_broken(self, event, was_non_natural).await;
            frontier::excavation::handle_block_broken(self, event, was_non_natural).await;
        })
    }
}

impl EventHandler<PlayerInteractEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerInteractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::sorcery::handle_player_interact(self, event).await;
        })
    }
}

impl EventHandler<PlayerFishEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerFishEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::fishing::handle_player_fish(self, event).await;
        })
    }
}

impl EventHandler<EntityBreedCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EntityBreedCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::husbandry::handle_entity_breed_complete(self, event).await;
        })
    }
}

impl EventHandler<EntityTameEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EntityTameEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::taming::handle_entity_tame(self, event).await;
        })
    }
}

impl EventHandler<EntityFeedCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EntityFeedCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::taming::handle_feed_complete(self, event).await;
        })
    }
}

impl EventHandler<AnimalProductCollectCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut AnimalProductCollectCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::husbandry::handle_product_complete(self, event).await;
        })
    }
}

impl EventHandler<BoneMealApplyCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BoneMealApplyCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::agriculture::handle_bone_meal_complete(self, event).await;
        })
    }
}

impl EventHandler<PlayerItemUseCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerItemUseCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            frontier::herbalism::handle_item_use_complete(self, event).await;
            enterprise::alchemy::handle_item_use_complete(self, event).await;
        })
    }
}

impl EventHandler<PlayerAttackDamageEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerAttackDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::melee::handle_attack_damage(self, event).await;
        })
    }
}

impl EventHandler<EntityShootBowEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EntityShootBowEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::archery::handle_shoot_bow(self, event).await;
        })
    }
}

impl EventHandler<ProjectileHitEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ProjectileHitEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::archery::handle_projectile_hit(self, server, event).await;
        })
    }
}

impl EventHandler<PlayerKillEntityEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerKillEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::kills::handle_player_kill(self, event).await;
        })
    }
}

impl EventHandler<EntityDamageEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::defense::handle_entity_damage(self, server, event).await;
        })
    }
}

impl EventHandler<EntityDamageByEntityEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EntityDamageByEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            warfare::archery::handle_entity_damage_by_entity(self, event).await;
        })
    }
}

impl EventHandler<AnvilPrepareEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut AnvilPrepareEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::repair::handle_anvil_prepare(self, event).await;
            enterprise::smithing::handle_anvil_prepare(self, event).await;
        })
    }
}

impl EventHandler<AnvilCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut AnvilCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::repair::handle_anvil_complete(self, event).await;
        })
    }
}

impl EventHandler<GrindstoneEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut GrindstoneEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::salvage::handle_grindstone(self, event).await;
        })
    }
}

impl EventHandler<GrindstoneCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut GrindstoneCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::salvage::handle_grindstone_complete(self, event).await;
        })
    }
}

impl EventHandler<EnchantItemGenerateEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EnchantItemGenerateEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::enchanting::handle_enchant_generate(self, event).await;
        })
    }
}

impl EventHandler<EnchantItemCompleteEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut EnchantItemCompleteEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::enchanting::handle_enchant_complete(self, event).await;
        })
    }
}

impl EventHandler<CraftItemEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut CraftItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::smithing::handle_craft_item(self, event).await;
            enterprise::tinkering::handle_craft_item(self, event).await;
        })
    }
}

impl EventHandler<FurnaceExtractEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut FurnaceExtractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            enterprise::smithing::handle_furnace_extract(self, event).await;
        })
    }
}

impl EventHandler<ServerTickStartEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() {
                return;
            }
            self.last_tick.store(event.tick, Ordering::Relaxed);
            self.bossbar_state.cleanup_expired(server, event.tick).await;
            if event.tick.rem_euclid(20) == 0 {
                ore_reveal::provenance::flush_provenance(&self.provenance, &self.db);
            }
            if event.tick.rem_euclid(600) == 0 {
                self.warfare_state.sweep(event.tick);
                if let Ok(mut previews) = self.perk_previews.lock() {
                    previews.retain(|_, created| event.tick.saturating_sub(*created) <= 600);
                }
            }
        })
    }
}

impl EventHandler<FeatureGenerateEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut FeatureGenerateEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_active() || !self.is_enabled() {
                return;
            }
            handle_feature_generate(self, event);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emerald_worldgen_is_disabled_for_existing_configs() {
        let mut config = MmoConfig::default();
        config.disabled_world_features.clear();
        assert!(should_disable_world_feature(&config, "ore_emerald"));
        assert!(!should_disable_world_feature(&config, "ore_diamond"));
    }

    #[test]
    fn emerald_worldgen_can_follow_reveal_disable_switch() {
        let mut config = MmoConfig::default();
        config.disabled_world_features.clear();
        config.ore_reveal.enabled = false;
        assert!(!should_disable_world_feature(&config, "ore_emerald"));
    }

    #[test]
    fn badlands_gold_worldgen_is_disabled_for_existing_configs() {
        // Configs written before `ore_gold_extra` joined the default
        // blacklist still serialize a list without it.
        let mut config = MmoConfig::default();
        config
            .disabled_world_features
            .retain(|name| name != "ore_gold_extra");
        assert!(should_disable_world_feature(&config, "ore_gold_extra"));
        assert!(!should_disable_world_feature(&config, "ore_gold_nether"));
    }

    #[test]
    fn badlands_gold_worldgen_can_follow_reveal_disable_switch() {
        let mut config = MmoConfig::default();
        config
            .disabled_world_features
            .retain(|name| name != "ore_gold_extra");
        config.ore_reveal.enabled = false;
        assert!(!should_disable_world_feature(&config, "ore_gold_extra"));
    }

    #[test]
    fn config_upgrade_fills_missing_canonical_skills_once() {
        let mut config = MmoConfig::default();
        config.config_version = 1;
        config.skills.retain(|skill, _| *skill == SkillId::Mining);

        assert!(upgrade_mmo_config(&mut config));
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.skills.len(), SkillId::ALL.len());
        for skill in SkillId::ALL {
            assert!(config.skills.contains_key(skill));
        }
        // A current-version config is left alone (no repeated saves).
        assert!(!upgrade_mmo_config(&mut config));
    }

    #[test]
    fn config_v1_file_with_retired_pairs_upgrades_to_a_full_canonical_map() {
        // Parse path first (deterministic pair merge), then the version bump
        // fills the canonical skills no old or new entry ever covered.
        let mut config: MmoConfig = ron::from_str(
            "(enabled:true,message_on_level_up:true,save_interval_ticks:6000,\
             config_version:1,\
             skills:{\
                 Mining:(max_level:99,base_xp:50,xp_multiplier:1.15),\
                 Repair:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true),\
                 Salvage:(max_level:50,base_xp:40,xp_multiplier:1.10,enabled:false)\
             },\
             disabled_world_features:[])",
        )
        .unwrap();
        assert_eq!(config.skills.len(), 2);
        assert_eq!(config.skills[&SkillId::Maintenance].max_level, 99);
        assert!(config.skills[&SkillId::Maintenance].enabled);

        assert!(upgrade_mmo_config(&mut config));
        assert_eq!(config.skills.len(), SkillId::ALL.len());
        // The merged destination keeps its migrated curve, not the default.
        assert_eq!(config.skills[&SkillId::Maintenance].max_level, 99);
        assert_eq!(
            config.skills[&SkillId::Cultivation],
            crate::config::SkillConfig::default()
        );
    }

    #[test]
    fn config_v2_file_upgrades_to_v3_and_saves_once_with_tier_knobs() {
        // A v2 file carries no per-tier knobs; the v2 pair merge and the
        // adopted values must keep working through the v3 bump.
        let mut config: MmoConfig = ron::from_str(
            "(enabled:true,message_on_level_up:true,save_interval_ticks:6000,\
             config_version:2,\
             skills:{\
                 Cultivation:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true),\
                 Agriculture:(max_level:80,base_xp:70,xp_multiplier:1.2,enabled:false),\
                 Herbalism:(max_level:80,base_xp:70,xp_multiplier:1.2,enabled:false)\
             },\
             disabled_world_features:[],\
             frontier:(mining:(prospector_enabled:true,prospector_base_chance:0.07,prospector_chance_per_level:0.002,prospector_max_chance:0.35,vein_miner_enabled:true,vein_miner_max_blocks:16)),\
             warfare:(archery:(hit_xp:6)),\
             enterprise:(repair:(discount_per_level:0.05,discount_cap:10.0,cooldown_ticks:100,xp:20)))",
        )
        .unwrap();
        // The v2 pair merge still runs during deserialize (explicit wins).
        assert_eq!(config.skills[&SkillId::Cultivation].max_level, 99);
        // v2 files parse: the new knobs land on their defaults.
        assert_eq!(config.frontier.mining.prospector_base_chance, 0.07);
        assert_eq!(config.frontier.mining.prospector_chance_per_tier, 0.01);
        assert!(config.frontier.mining.prospector_capstone_double);
        assert!(config.warfare.archery.damage_enabled);
        assert_eq!(config.warfare.archery.damage_cap_per_tier, 0.05);
        assert!(config.enterprise.repair.tool_care_enabled);
        assert_eq!(config.enterprise.repair.tool_care_chance_per_tier, 0.025);

        // The upgrade bumps to v3 exactly once and the re-saved file
        // documents the new fields.
        assert!(upgrade_mmo_config(&mut config));
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(!upgrade_mmo_config(&mut config));
        let serialized = ron::ser::to_string(&config).unwrap();
        assert!(serialized.contains("prospector_chance_per_tier"));
        assert!(serialized.contains("prospector_capstone_double"));
        assert!(serialized.contains("damage_cap_per_tier"));
        assert!(serialized.contains("tool_care_chance_per_tier"));
        assert!(serialized.contains("heal_per_tier"));
        assert!(serialized.contains("xp_cap_per_tier"));
    }

    fn test_config_folder() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("cabbage_mmo_config_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&path);
        path
    }

    fn cleanup_config_folder(folder: &PathBuf) {
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn unified_config_splits_into_dedicated_files() {
        let folder = test_config_folder();
        let legacy = "(metrics_log:true,mob_ai:false,mmo:Some((\
             enabled:true,config_version:1,message_on_level_up:true,save_interval_ticks:6000,\
             skills:{\
                 Mining:(max_level:99,base_xp:50,xp_multiplier:1.15),\
                 Repair:(max_level:99,base_xp:60,xp_multiplier:1.14,enabled:true),\
                 Salvage:(max_level:50,base_xp:40,xp_multiplier:1.10,enabled:false)\
             },\
             disabled_world_features:[],reward_config_version:1,\
             xp_rewards:(mobs:{\"zombie\":99},blocks:{\"coal_ore\":42}),\
             ore_reveal:(enabled:false))))";
        fs::write(folder.join(CONFIG_FILE), legacy).unwrap();

        let loaded = load_mmo_config(&folder).unwrap();
        assert!(loaded.split_files_missing);
        let mut config = loaded.config.sanitized();
        // The inline sections of the unified file are adopted.
        assert_eq!(config.xp_rewards.mobs.get("zombie"), Some(&99));
        assert_eq!(config.xp_rewards.blocks.get("coal_ore"), Some(&42));
        assert!(!config.ore_reveal.enabled);
        assert!(upgrade_mmo_config(&mut config));
        save_mmo_config_files(&folder, &config).unwrap();

        // The legacy unified file is left byte-for-byte untouched.
        assert_eq!(
            fs::read_to_string(folder.join(CONFIG_FILE)).unwrap(),
            legacy
        );

        // mmo.rewards.ron carries the adopted XP tables.
        let rewards: XpRewardsConfig =
            ron::from_str(&fs::read_to_string(folder.join(REWARDS_CONFIG_FILE)).unwrap()).unwrap();
        assert_eq!(rewards.mobs.get("zombie"), Some(&99));
        assert_eq!(rewards.blocks.get("coal_ore"), Some(&42));

        // mmo.ores.ron carries the adopted ore reveal section.
        let ore_reveal: OreRevealConfig =
            ron::from_str(&fs::read_to_string(folder.join(ORE_REVEAL_CONFIG_FILE)).unwrap())
                .unwrap();
        assert!(!ore_reveal.enabled);

        // mmo.ron carries the v2-upgraded config without the split sections.
        let mmo_text = fs::read_to_string(folder.join(MMO_CONFIG_FILE)).unwrap();
        assert!(!mmo_text.contains("xp_rewards"));
        assert!(!mmo_text.contains("ore_reveal"));
        let mmo: MmoConfig = ron::from_str(&mmo_text).unwrap();
        assert_eq!(mmo.config_version, CURRENT_CONFIG_VERSION);
        // The retired pair merged into the Maintenance anchor curve.
        assert_eq!(mmo.skills[&SkillId::Maintenance].max_level, 99);
        // The version bump filled every canonical skill.
        assert_eq!(mmo.skills.len(), SkillId::ALL.len());

        cleanup_config_folder(&folder);
    }

    #[test]
    fn split_files_win_over_stale_unified_section() {
        let folder = test_config_folder();
        // Stale unified section whose values must all be ignored.
        fs::write(
            folder.join(CONFIG_FILE),
            "(metrics_log:false,mob_ai:true,mmo:Some((\
             enabled:false,config_version:2,message_on_level_up:false,save_interval_ticks:1,\
             skills:{},disabled_world_features:[],reward_config_version:1,\
             xp_rewards:(mobs:{\"zombie\":99},blocks:{\"coal_ore\":99}),\
             ore_reveal:(enabled:true))))",
        )
        .unwrap();
        fs::write(
            folder.join(MMO_CONFIG_FILE),
            "(enabled:true,config_version:2,message_on_level_up:true,save_interval_ticks:6000,\
             skills:{},disabled_world_features:[],reward_config_version:1)",
        )
        .unwrap();
        fs::write(
            folder.join(REWARDS_CONFIG_FILE),
            "(mobs:{\"zombie\":7},blocks:{\"coal_ore\":3})",
        )
        .unwrap();
        fs::write(folder.join(ORE_REVEAL_CONFIG_FILE), "(enabled:false)").unwrap();

        let loaded = load_mmo_config(&folder).unwrap();
        assert!(!loaded.split_files_missing);
        let config = loaded.config;
        // Every value comes from the split files, not the stale section.
        assert!(config.enabled);
        assert!(config.message_on_level_up);
        assert_eq!(config.save_interval_ticks, 6000);
        assert_eq!(config.xp_rewards.mobs.get("zombie"), Some(&7));
        assert_eq!(config.xp_rewards.blocks.get("coal_ore"), Some(&3));
        assert!(!config.ore_reveal.enabled);

        cleanup_config_folder(&folder);
    }

    #[test]
    fn legacy_subfolder_files_are_adopted() {
        let folder = test_config_folder();
        save_mmo_config_files(&folder, &MmoConfig::default()).unwrap();
        // The superseded mmo/ subfolder layout still supplies values when the
        // flat files are missing; the subfolder files stay untouched.
        let legacy_folder = folder.join(LEGACY_MMO_SUBFOLDER);
        fs::create_dir_all(&legacy_folder).unwrap();
        let legacy_rewards = "(mobs:{\"zombie\":11},blocks:{\"coal_ore\":13})";
        fs::write(
            legacy_folder.join(LEGACY_REWARDS_CONFIG_FILE),
            legacy_rewards,
        )
        .unwrap();
        fs::write(
            legacy_folder.join(LEGACY_ORE_REVEAL_CONFIG_FILE),
            "(enabled:false)",
        )
        .unwrap();

        // Remove the flat rewards/ore files so only the subfolder copies
        // remain.
        fs::remove_file(folder.join(REWARDS_CONFIG_FILE)).unwrap();
        fs::remove_file(folder.join(ORE_REVEAL_CONFIG_FILE)).unwrap();

        let loaded = load_mmo_config(&folder).unwrap();
        // The flat files are missing, so the caller saves them back.
        assert!(loaded.split_files_missing);
        assert_eq!(loaded.config.xp_rewards.mobs.get("zombie"), Some(&11));
        assert_eq!(loaded.config.xp_rewards.blocks.get("coal_ore"), Some(&13));
        assert!(!loaded.config.ore_reveal.enabled);
        // The legacy subfolder files are left untouched.
        assert_eq!(
            fs::read_to_string(legacy_folder.join(LEGACY_REWARDS_CONFIG_FILE)).unwrap(),
            legacy_rewards
        );

        // Flat files win over the legacy subfolder once they exist.
        fs::write(
            folder.join(REWARDS_CONFIG_FILE),
            "(mobs:{\"zombie\":22},blocks:{\"coal_ore\":24})",
        )
        .unwrap();
        let loaded = load_mmo_config(&folder).unwrap();
        assert_eq!(loaded.config.xp_rewards.mobs.get("zombie"), Some(&22));

        cleanup_config_folder(&folder);
    }

    #[test]
    fn fresh_folder_loads_defaults_and_saves_split_files() {
        let folder = test_config_folder();

        let loaded = load_mmo_config(&folder).unwrap();
        assert!(loaded.split_files_missing);
        let config = loaded.config.sanitized();
        assert_eq!(config, MmoConfig::default());
        save_mmo_config_files(&folder, &config).unwrap();

        assert!(folder.join(MMO_CONFIG_FILE).exists());
        assert!(folder.join(REWARDS_CONFIG_FILE).exists());
        assert!(folder.join(ORE_REVEAL_CONFIG_FILE).exists());
        // The MMO module never creates config.ron; Core owns it.
        assert!(!folder.join(CONFIG_FILE).exists());

        // The split files round-trip back to the defaults.
        let mmo: MmoConfig =
            ron::from_str(&fs::read_to_string(folder.join(MMO_CONFIG_FILE)).unwrap()).unwrap();
        assert_eq!(mmo.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(mmo.skills.len(), SkillId::ALL.len());
        let rewards: XpRewardsConfig =
            ron::from_str(&fs::read_to_string(folder.join(REWARDS_CONFIG_FILE)).unwrap()).unwrap();
        assert_eq!(rewards, XpRewardsConfig::default());
        let ore_reveal: OreRevealConfig =
            ron::from_str(&fs::read_to_string(folder.join(ORE_REVEAL_CONFIG_FILE)).unwrap())
                .unwrap();
        assert_eq!(ore_reveal, OreRevealConfig::default());

        cleanup_config_folder(&folder);
    }

    #[test]
    fn mmo_ron_omits_the_split_out_sections() {
        let folder = test_config_folder();
        save_mmo_config_files(&folder, &MmoConfig::default()).unwrap();

        let mmo_text = fs::read_to_string(folder.join(MMO_CONFIG_FILE)).unwrap();
        assert!(!mmo_text.contains("xp_rewards"));
        assert!(!mmo_text.contains("ore_reveal"));
        // reward_config_version stays in mmo.ron.
        assert!(mmo_text.contains("reward_config_version"));

        let rewards_text = fs::read_to_string(folder.join(REWARDS_CONFIG_FILE)).unwrap();
        assert!(rewards_text.contains("mobs"));
        assert!(rewards_text.contains("blocks"));
        let ore_reveal_text = fs::read_to_string(folder.join(ORE_REVEAL_CONFIG_FILE)).unwrap();
        assert!(ore_reveal_text.contains("enabled"));
        assert!(ore_reveal_text.contains("ores"));

        cleanup_config_folder(&folder);
    }

    #[test]
    fn reload_load_path_picks_up_rewards_edits() {
        let folder = test_config_folder();
        save_mmo_config_files(&folder, &MmoConfig::default()).unwrap();

        // An admin edits mmo.rewards.ron; the reload path (load_mmo_config)
        // picks the edit up.
        fs::write(folder.join(REWARDS_CONFIG_FILE), "(mobs:{\"zombie\":55})").unwrap();
        let loaded = load_mmo_config(&folder).unwrap();
        assert!(!loaded.split_files_missing);
        assert_eq!(loaded.config.xp_rewards.mobs.get("zombie"), Some(&55));
        // Keys absent from the edited file fall back to defaults.
        assert_eq!(loaded.config.xp_rewards.blocks.get("coal_ore"), Some(&8));

        cleanup_config_folder(&folder);
    }

    #[test]
    fn legacy_config_json_converts_and_stays_untouched() {
        let folder = test_config_folder();
        let json = "{\"metrics_log\":true,\"mob_ai\":false}";
        fs::write(folder.join(LEGACY_CONFIG_FILE), json).unwrap();

        let loaded = load_mmo_config(&folder).unwrap();
        assert!(loaded.split_files_missing);
        // The json carries no MMO data, so the MMO side is defaults.
        assert_eq!(loaded.config, MmoConfig::default());

        // The adoption wrote a Core-only config.ron with the json switches.
        let config_text = fs::read_to_string(folder.join(CONFIG_FILE)).unwrap();
        assert!(!config_text.contains("mmo:"));
        let plugin_config: PluginConfig = ron::from_str(&config_text).unwrap();
        assert!(plugin_config.metrics_log);
        assert!(!plugin_config.mob_ai);
        assert_eq!(plugin_config.mmo, None);

        // The json file itself is never modified or deleted.
        assert_eq!(
            fs::read_to_string(folder.join(LEGACY_CONFIG_FILE)).unwrap(),
            json
        );

        cleanup_config_folder(&folder);
    }
}
