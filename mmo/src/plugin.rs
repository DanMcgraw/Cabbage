//! Native plugin entry point for the standalone `Cabbage.Mmo` plugin DLL.
//!
//! The rlib half of this crate stays plugin-agnostic for testability; this
//! module owns the DLL exports, metadata, and event/command registration.

use std::{mem::MaybeUninit, sync::Arc};

use pumpkin::plugin::{
    Context, EventPriority, PLUGIN_API_VERSION, Plugin, PluginFuture, PluginMetadata,
    api::events::{
        block::{
            block_break::BlockBreakEvent, block_broken::BlockBrokenEvent,
            block_drop_item::BlockDropItemEvent, block_place::BlockPlaceEvent,
            bone_meal::BoneMealApplyCompleteEvent,
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
};

use crate::{MMO_NAMES, MMO_PERMISSION, MmoState, mmo_command_tree, register_permissions};

const PLUGIN_NAME: &str = "Cabbage.Mmo";

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
        description: "Cabbage MMO skilling plugin.".to_string(),
        dependencies: vec!["Cabbage.Core".to_string()],
        permissions: Vec::new(),
    };

    unsafe {
        core::ptr::addr_of_mut!(METADATA)
            .cast::<PluginMetadata>()
            .write(metadata);
    }
}

struct MmoPlugin {
    state: Option<Arc<MmoState>>,
}

impl Plugin for MmoPlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let state = match MmoState::new(context.clone()).await {
                Ok(state) => state,
                Err(error) => {
                    println!("[Cabbage.Mmo] Failed to initialize MMO module: {error}");
                    return Ok(());
                }
            };
            self.state = Some(state.clone());

            if let Err(error) = register_permissions(&context).await {
                println!("[Cabbage.Mmo] Failed to register MMO permissions: {error}");
            }

            register_events(&context, &state).await;

            context
                .register_command(mmo_command_tree(state), MMO_PERMISSION)
                .await;

            Ok(())
        })
    }

    fn on_unload(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            context.unregister_command(MMO_NAMES[0]).await;
            // Event handlers are never auto-removed and the DLL stays mapped,
            // so gate every handler on the active flag.
            if let Some(state) = self.state.as_ref() {
                state.set_active(false);
            }
            Ok(())
        })
    }
}

async fn register_events(context: &Arc<Context>, state: &Arc<MmoState>) {
    context
        .register_event::<BlockBreakEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<BlockPlaceEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<BlockDropItemEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<BlockBrokenEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<ServerTickStartEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<FeatureGenerateEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<PlayerInteractEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<PlayerFishEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EntityBreedCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EntityTameEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EntityFeedCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<AnimalProductCollectCompleteEvent, _>(
            state.clone(),
            EventPriority::Normal,
            true,
        )
        .await;
    context
        .register_event::<BoneMealApplyCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<PlayerItemUseCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<PlayerAttackDamageEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EntityShootBowEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<ProjectileHitEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<PlayerKillEntityEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EntityDamageEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<AnvilPrepareEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<AnvilCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<GrindstoneEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<GrindstoneCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EnchantItemGenerateEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<EnchantItemCompleteEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<CraftItemEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
    context
        .register_event::<FurnaceExtractEvent, _>(state.clone(), EventPriority::Normal, true)
        .await;
}

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(MmoPlugin { state: None })
}
