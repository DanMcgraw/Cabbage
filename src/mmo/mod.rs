use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
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
                block_place::BlockPlaceEvent, bone_meal::BoneMealApplyCompleteEvent,
            },
            entity::{
                entity_breed::EntityBreedCompleteEvent, entity_damage::EntityDamageEvent,
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
pub(crate) mod config;
pub(crate) mod db;
pub(crate) mod enterprise;
pub(crate) mod frontier;
pub(crate) mod ore_reveal;
pub(crate) mod perks;
pub(crate) mod persistence;
pub(crate) mod progression;
pub(crate) mod skills;
pub(crate) mod ui;
pub(crate) mod warfare;

pub use commands::{MMO_NAMES, MMO_PERMISSION, mmo_command_tree};
pub use config::{LevelCurve, MmoConfig, PluginConfig, SkillConfig};
pub use skills::SkillId;

use commands::MMO_ADMIN_PERMISSION;
use config::{CURRENT_CONFIG_VERSION, LegacyPluginConfig};
use db::MmoDatabase;
use ore_reveal::provenance::{ProvenanceKey, ProvenanceTracker};
use perks::CooldownTracker;
use ui::BossbarState;

const CONFIG_FILE: &str = "config.ron";
const LEGACY_CONFIG_FILE: &str = "config.json";

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
}

impl MmoState {
    /// Initialize the MMO module: load or create config, open the database,
    /// run pending migrations, and compute curves.
    pub async fn new(context: Arc<Context>) -> Result<Arc<Self>, String> {
        let data_folder = context.get_data_folder();
        let mut plugin_config = load_plugin_config(&data_folder)?;
        let mut mmo_config = plugin_config.mmo.clone().unwrap_or_default().sanitized();
        let mut config_dirty = false;

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
                log::info!("[Cabbage MMO] migrated XP rewards from SQLite to config.ron");
            } else {
                log::info!("[Cabbage MMO] removed obsolete SQLite XP reward tables");
            }
        }

        // Config schema upgrade: fill any missing skill curves so the saved
        // file documents every skill, and record the current version.
        if mmo_config.config_version < CURRENT_CONFIG_VERSION {
            mmo_config.config_version = CURRENT_CONFIG_VERSION;
            for skill in SkillId::ALL {
                mmo_config.skills.entry(*skill).or_default();
            }
            config_dirty = true;
        }
        if config_dirty {
            plugin_config.mmo = Some(mmo_config.clone());
            save_plugin_config(&data_folder, &plugin_config)?;
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
        let plugin_config = load_plugin_config(&self.data_folder)?;
        let mmo_config = plugin_config.mmo.clone().unwrap_or_default().sanitized();
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

/// Cancel feature placement if the feature is in the MMO blacklist.
fn handle_feature_generate(state: &MmoState, event: &mut FeatureGenerateEvent) {
    let feature_name = placed_feature_name(event.feature);
    if should_disable_world_feature(&state.config(), &feature_name) {
        event.cancelled = true;
    }
}

fn should_disable_world_feature(config: &MmoConfig, feature_name: &str) -> bool {
    // Existing config files already contain a serialized blacklist, so adding
    // emerald to the default alone would not migrate them. Keep emerald tied
    // to the reveal system even for those existing installations.
    (config.ore_reveal.enabled && feature_name == "ore_emerald")
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
            if !self.is_enabled() {
                return;
            }
            frontier::mining::handle_block_break(self, event).await;
            frontier::woodcutting::handle_block_break(self, event).await;
            frontier::excavation::handle_block_break(self, event).await;
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
            if event.cancelled || !event.can_build {
                return;
            }
            let config = self.config();
            let tracked = self.ore_reveal_state.is_host_block(event.block_placed)
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

impl EventHandler<BlockBrokenEvent> for MmoState {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockBrokenEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
                return;
            }
            warfare::defense::handle_entity_damage(self, server, event).await;
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
            if !self.is_enabled() {
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
}
