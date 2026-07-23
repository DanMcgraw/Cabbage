//! MMO module lifecycle and event/command registration.
//!
//! `cabbage-core` owns the native plugin DLL export and delegates the MMO
//! portion of that lifecycle to this module. Keeping the registration code
//! here preserves the MMO crate boundary without producing a second DLL.

use std::sync::Arc;

use pumpkin::plugin::{
    Context, EventPriority,
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
};

use crate::{MMO_NAMES, MMO_PERMISSION, MmoState, mmo_command_tree, register_permissions};

/// Stateful MMO portion of the combined Cabbage plugin.
pub struct MmoModule {
    state: Option<Arc<MmoState>>,
}

impl Default for MmoModule {
    fn default() -> Self {
        Self { state: None }
    }
}

impl MmoModule {
    /// Registers the MMO module in the combined plugin's unified data folder.
    pub async fn load(&mut self, context: Arc<Context>) {
        let data_folder = context.get_data_folder();
        let state = match MmoState::new_in_data_folder(context.clone(), data_folder).await {
            Ok(state) => state,
            Err(error) => {
                println!("[Cabbage.Mmo] Failed to initialize MMO module: {error}");
                return;
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
    }

    /// Unregisters MMO commands and gates all retained event handlers.
    pub async fn unload(&mut self, context: &Arc<Context>) {
        context.unregister_command(MMO_NAMES[0]).await;
        // Event handlers are never auto-removed and the DLL stays mapped,
        // so gate every handler on the active flag.
        if let Some(state) = self.state.as_ref() {
            state.set_active(false);
        }
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
        .register_event::<EntityDamageByEntityEvent, _>(state.clone(), EventPriority::Normal, true)
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
