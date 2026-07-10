pub(super) mod config;
pub(crate) mod provenance;
mod shape;

use std::sync::RwLock;

use pumpkin::{
    plugin::api::events::block::{block_broken::BlockBrokenEvent, block_place::BlockPlaceEvent},
    world::World,
};
use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::{GameMode, math::position::BlockPos};
use pumpkin_world::world::BlockFlags;
use rand::Rng;

use self::{
    config::{CompiledOreRevealConfig, OreRevealConfig},
    provenance::{ProvenanceKey, ProvenanceTracker},
    shape::grow_vein,
};
use super::db::MmoDatabase;

pub(super) struct OreRevealState {
    config: RwLock<CompiledOreRevealConfig>,
    provenance: ProvenanceTracker,
}

impl OreRevealState {
    pub fn new(config: &OreRevealConfig, non_natural: Vec<ProvenanceKey>) -> Result<Self, String> {
        Ok(Self {
            config: RwLock::new(config.compile()?),
            provenance: ProvenanceTracker::new(non_natural),
        })
    }

    pub fn reload(&self, config: &OreRevealConfig) -> Result<(), String> {
        let compiled = config.compile()?;
        self.config
            .write()
            .map(|mut current| *current = compiled)
            .map_err(|_| "ore reveal configuration lock is poisoned".to_string())
    }

    pub fn handle_block_place(&self, event: &BlockPlaceEvent) {
        if event.cancelled || !event.can_build {
            return;
        }
        let is_host = self
            .config
            .read()
            .map(|config| config.is_host(event.block_placed))
            .unwrap_or(false);
        if is_host {
            self.provenance.mark(ProvenanceKey::new(
                &event.player.world(),
                event.block_position,
            ));
        }
    }

    pub async fn handle_block_broken(&self, event: &BlockBrokenEvent, allow_reveal: bool) {
        let key = ProvenanceKey::new(&event.world, event.block_position);
        let was_non_natural = self.provenance.take(&key);

        let Some(player) = event.player.as_ref() else {
            return;
        };
        if !allow_reveal
            || was_non_natural
            || !matches!(
                player.gamemode.load(),
                GameMode::Survival | GameMode::Adventure
            )
        {
            return;
        }
        let Some(face) = event.face else {
            return;
        };

        let config = match self.config.read() {
            Ok(config) if config.enabled && config.is_host(event.block) => config.clone(),
            _ => return,
        };
        let biome = event.world.get_biome(&event.block_position).registry_id;
        let forward = face.opposite();
        let start = event.block_position.offset(forward.to_offset());
        let world = &event.world;
        let Some((positions, stone_block, deepslate_block)) = (|| {
            let mut rng = rand::rng();
            let effective =
                config.select_ore(biome, event.block_position.0.y, rng.random(), rng.random())?;
            let ore = &config.ores[effective.index];
            let base_size = rng.random_range(ore.size.min..=ore.size.max) as f64;
            let target_size = (base_size * effective.size_multiplier)
                .round()
                .clamp(1.0, f64::from(config.shape.max_vein_size))
                as usize;
            let positions = grow_vein(
                &mut rng,
                start,
                forward,
                target_size,
                &config.shape,
                |position, is_seed| {
                    is_eligible_target(world, &config, &self.provenance, *position, is_seed)
                },
            );
            Some((positions, ore.stone_block, ore.deepslate_block))
        })() else {
            return;
        };

        for position in positions {
            let host = world.get_block(&position);
            if !config.is_host(host)
                || self
                    .provenance
                    .contains(&ProvenanceKey::new(world, position))
            {
                continue;
            }
            let replacement = if host == &Block::DEEPSLATE {
                deepslate_block
            } else {
                stone_block
            };
            world
                .set_block_state(
                    &position,
                    replacement.default_state.id,
                    BlockFlags::NOTIFY_LISTENERS | BlockFlags::SKIP_DROPS,
                )
                .await;
        }
    }

    pub fn flush_provenance(&self, db: &MmoDatabase) {
        let changes = self.provenance.drain_pending();
        if changes.is_empty() {
            return;
        }
        if let Err(error) = db.apply_provenance_changes(changes.clone()) {
            self.provenance.requeue(changes);
            log::warn!("[Cabbage MMO] failed to queue block provenance changes: {error}");
        }
    }
}

fn is_eligible_target(
    world: &World,
    config: &CompiledOreRevealConfig,
    provenance: &ProvenanceTracker,
    position: BlockPos,
    is_seed: bool,
) -> bool {
    let Some(state_id) = world.get_block_state_id_if_loaded(&position) else {
        return false;
    };
    if !config.is_host(Block::from_state_id(state_id))
        || provenance.contains(&ProvenanceKey::new(world, position))
    {
        return false;
    }
    if is_seed || !config.shape.require_hidden_targets {
        return true;
    }

    BlockDirection::all().into_iter().all(|direction| {
        world
            .get_block_state_if_loaded(&position.offset(direction.to_offset()))
            .is_some_and(|state| !state.is_air())
    })
}
