pub(super) mod config;
pub(crate) mod provenance;
mod shape;

use std::sync::RwLock;

use pumpkin::{plugin::api::events::block::block_broken::BlockBrokenEvent, world::World};
use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::{GameMode, math::position::BlockPos};
use pumpkin_world::world::BlockFlags;
use rand::RngExt;

use self::{
    config::{CompiledOreRevealConfig, OreRevealConfig},
    provenance::{ProvenanceKey, ProvenanceTracker},
    shape::grow_vein,
};

pub(super) struct OreRevealState {
    config: RwLock<CompiledOreRevealConfig>,
}

impl OreRevealState {
    pub fn new(config: &OreRevealConfig) -> Result<Self, String> {
        Ok(Self {
            config: RwLock::new(config.compile()?),
        })
    }

    pub fn reload(&self, config: &OreRevealConfig) -> Result<(), String> {
        let compiled = config.compile()?;
        self.config
            .write()
            .map(|mut current| *current = compiled)
            .map_err(|_| "ore reveal configuration lock is poisoned".to_string())
    }

    /// Whether this block type hosts ore veins and must be provenance-tracked
    /// when player-placed.
    pub(crate) fn is_host_block(&self, block: &Block) -> bool {
        self.config
            .read()
            .map(|config| config.is_host(block))
            .unwrap_or(false)
    }

    /// Potentially reveal an ore vein behind a broken host block.
    ///
    /// `was_non_natural` comes from the shared provenance tracker (taken once
    /// by the central block-broken coordinator) and excludes player-placed
    /// hosts from both reveals and XP progression.
    pub async fn handle_block_broken(
        &self,
        event: &BlockBrokenEvent,
        allow_reveal: bool,
        provenance: &ProvenanceTracker,
        was_non_natural: bool,
    ) {
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
                    is_eligible_target(world, &config, provenance, *position, is_seed)
                },
            );
            Some((positions, ore.stone_block, ore.deepslate_block))
        })() else {
            return;
        };

        for position in positions {
            let host = world.get_block(&position);
            if !config.is_host(host) || provenance.contains(&ProvenanceKey::new(world, position)) {
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
