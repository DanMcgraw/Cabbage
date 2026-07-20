use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use pumpkin::{
    plugin::{
        BoxFuture, Context, EventHandler, EventPriority,
        api::events::block::{
            block_damage::BlockDamageEvent, block_drop_item::BlockDropItemEvent,
            block_piston_extend::BlockPistonExtendEvent,
            block_piston_retract::BlockPistonRetractEvent, brew::BrewEvent,
            furnace_burn::FurnaceBurnEvent, furnace_smelt::FurnaceSmeltEvent,
        },
        api::events::entity::{
            entity_breed::EntityBreedEvent, entity_combust_by_entity::EntityCombustByEntityEvent,
            entity_damage::EntityDamageEvent, entity_damage_by_entity::EntityDamageByEntityEvent,
            entity_death::EntityDeathEvent, entity_explode::EntityExplodeEvent,
            entity_pickup_item::EntityPickupItemEvent, entity_shoot_bow::EntityShootBowEvent,
            entity_tame::EntityTameEvent, entity_target::EntityTargetEvent,
            entity_target_living_entity::EntityTargetLivingEntityEvent,
            entity_transform::EntityTransformEvent, explosion_prime::ExplosionPrimeEvent,
            potion_splash::PotionSplashEvent, projectile_hit::ProjectileHitEvent,
            projectile_launch::ProjectileLaunchEvent,
        },
        api::events::inventory::InventoryMoveItemEvent,
        api::events::player::{
            craft_item::CraftItemEvent, food_level_change::FoodLevelChangeEvent,
            furnace_extract::FurnaceExtractEvent, inventory_drag::InventoryDragEvent,
            inventory_interact::InventoryClickEvent, inventory_open::InventoryOpenEvent,
            player_death::PlayerDeathEvent, player_drop_item::PlayerDropItemEvent,
        },
    },
    server::Server,
};
use pumpkin_util::text::TextComponent;

pub(crate) struct EventLogState {
    enabled: AtomicBool,
    log_path: Mutex<Option<PathBuf>>,
    /// False once the plugin is unloaded; handlers must early-return then.
    active: AtomicBool,
}

impl Default for EventLogState {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            log_path: Mutex::new(None),
            active: AtomicBool::new(true),
        }
    }
}

impl EventLogState {
    pub(crate) fn set_log_path(&self, path: PathBuf) {
        if let Ok(mut lock) = self.log_path.lock() {
            *lock = Some(path);
        }
    }

    pub(crate) fn toggle(&self) -> bool {
        !self.enabled.fetch_xor(true, Ordering::SeqCst)
    }

    pub(crate) fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::SeqCst);
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst) && self.active.load(Ordering::SeqCst)
    }

    pub(crate) fn log(&self, message: &str) {
        let Some(path) = self.log_path.lock().ok().and_then(|path| path.clone()) else {
            return;
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("[{}] ", d.as_secs()))
            .unwrap_or_default();

        let line = format!("{timestamp}{message}\n");

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = file.write_all(line.as_bytes());
        }
    }
}

pub(crate) async fn register(context: &Arc<Context>, event_log_state: &Arc<EventLogState>) {
    context
        .register_event::<BlockDamageEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<BlockDropItemEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<BlockPistonExtendEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<BlockPistonRetractEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityDamageEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityDamageByEntityEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityDeathEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<PlayerDeathEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<FoodLevelChangeEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<ProjectileLaunchEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<ProjectileHitEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<PlayerDropItemEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<InventoryOpenEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<InventoryClickEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<InventoryDragEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<InventoryMoveItemEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<CraftItemEvent, _>(event_log_state.clone(), EventPriority::Normal, false)
        .await;
    context
        .register_event::<FurnaceSmeltEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<FurnaceBurnEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<FurnaceExtractEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<BrewEvent, _>(event_log_state.clone(), EventPriority::Normal, false)
        .await;
    context
        .register_event::<EntityBreedEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityTameEvent, _>(event_log_state.clone(), EventPriority::Normal, false)
        .await;
    context
        .register_event::<EntityTargetEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityTargetLivingEntityEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityPickupItemEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityShootBowEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityCombustByEntityEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityExplodeEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<ExplosionPrimeEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<EntityTransformEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<PotionSplashEvent, _>(
            event_log_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
}

impl EventHandler<BlockDamageEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockDamageEvent: player={}, block={}, pos={:?}, insta_break={}",
                event.player.gameprofile.name,
                event.block.name,
                event.block_position,
                event.insta_break
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<BlockDropItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockDropItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockDropItemEvent: player={}, block={}, pos={:?}, items={}",
                event.player.gameprofile.name,
                event.block.name,
                event.block_position,
                event.items.len()
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<BlockPistonExtendEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockPistonExtendEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockPistonExtendEvent: block={}, pos={:?}, direction={:?}, moved_blocks={}, broken_blocks={}",
                event.piston_block.name,
                event.piston_pos,
                event.direction,
                event.moved_blocks.len(),
                event.broken_blocks.len()
            );

            self.log(&message);
        })
    }
}

impl EventHandler<BlockPistonRetractEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a BlockPistonRetractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BlockPistonRetractEvent: block={}, pos={:?}, direction={:?}, moved_blocks={}",
                event.piston_block.name,
                event.piston_pos,
                event.direction,
                event.moved_blocks.len()
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityDamageEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityDamageEvent: entity={}, damage_type={}, damage={}, final_damage={}",
                event.entity.get_entity().entity_type.resource_name,
                event.damage_type.message_id,
                event.damage,
                event.final_damage
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityDamageByEntityEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityDamageByEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let attacker_name = event
                .attacker
                .as_ref()
                .map(|a| a.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityDamageByEntityEvent: entity={}, damager={}, attacker={}, damage_type={}, damage={}, final_damage={}",
                event.entity.get_entity().entity_type.resource_name,
                event.damager.get_entity().entity_type.resource_name,
                attacker_name,
                event.damage_type.message_id,
                event.damage,
                event.final_damage
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityDeathEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityDeathEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let killer_name = event
                .killer
                .as_ref()
                .map(|k| k.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityDeathEvent: entity={}, killer={}, damage_type={}, drops={}, dropped_exp={}",
                event.entity.get_entity().entity_type.resource_name,
                killer_name,
                event.damage_type.message_id,
                event.drops.len(),
                event.dropped_exp
            );

            self.log(&message);
        })
    }
}

impl EventHandler<PlayerDeathEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a PlayerDeathEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let killer_name = event
                .killer
                .as_ref()
                .map(|k| k.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] PlayerDeathEvent: player={}, killer={}, damage_type={}, drops={}, dropped_exp={}, keep_inventory={}, keep_level={}",
                event.player.gameprofile.name,
                killer_name,
                event.damage_type.message_id,
                event.drops.len(),
                event.dropped_exp,
                event.keep_inventory,
                event.keep_level
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<FoodLevelChangeEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FoodLevelChangeEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FoodLevelChangeEvent: player={}, food_level={}",
                event.player.gameprofile.name, event.food_level
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<ProjectileLaunchEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ProjectileLaunchEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let shooter_name = event
                .shooter
                .as_ref()
                .map(|s| s.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] ProjectileLaunchEvent: projectile={}, shooter={}",
                event.projectile.get_entity().entity_type.resource_name,
                shooter_name
            );

            self.log(&message);
        })
    }
}

impl EventHandler<ProjectileHitEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ProjectileHitEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let hit_entity_name = event
                .hit_entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let hit_block_name = event.hit_block.map(|b| b.name).unwrap_or("none");
            let message = format!(
                "[Cabbage Events] ProjectileHitEvent: projectile={}, hit_entity={}, hit_block={}",
                event.projectile.get_entity().entity_type.resource_name,
                hit_entity_name,
                hit_block_name
            );

            self.log(&message);
        })
    }
}

impl EventHandler<PlayerDropItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a PlayerDropItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] PlayerDropItemEvent: player={}, item={}, count={}",
                event.player.gameprofile.name, event.item.item.registry_key, event.item.item_count
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryOpenEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryOpenEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] InventoryOpenEvent: player={}, window_type={:?}, block_pos={:?}",
                event.player.gameprofile.name, event.window_type, event.block_pos
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryClickEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryClickEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let clicked_name = event
                .clicked_item
                .as_ref()
                .map(|i| i.item.registry_key.as_ref())
                .unwrap_or("empty");
            let cursor_name = event
                .cursor
                .as_ref()
                .map(|i| i.item.registry_key.as_ref())
                .unwrap_or("empty");
            let message = format!(
                "[Cabbage Events] InventoryClickEvent: player={}, window_type={:?}, slot={}, click_type={:?}, clicked={}, cursor={}",
                event.player.gameprofile.name,
                event.window_type,
                event.slot,
                event.click_type,
                clicked_name,
                cursor_name
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryDragEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryDragEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] InventoryDragEvent: player={}, window_type={:?}, slots={:?}, click_type={:?}",
                event.player.gameprofile.name, event.window_type, event.slots, event.click_type
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<InventoryMoveItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a InventoryMoveItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] InventoryMoveItemEvent: item={}, count={}, source_pos={:?}, destination_pos={:?}",
                event.item.item.registry_key,
                event.item.item_count,
                event.source_pos,
                event.destination_pos
            );

            self.log(&message);
        })
    }
}

impl EventHandler<CraftItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a CraftItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] CraftItemEvent: player={}, result={}, count={}, window_type={:?}",
                event.player.gameprofile.name,
                event.result.item.registry_key,
                event.result.item_count,
                event.window_type
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<FurnaceSmeltEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FurnaceSmeltEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FurnaceSmeltEvent: block={}, pos={:?}, input={}, fuel={}, output={}",
                event.block.name,
                event.block_position,
                event.input.item.registry_key,
                event.fuel.item.registry_key,
                event.output.item.registry_key
            );

            self.log(&message);
        })
    }
}

impl EventHandler<FurnaceBurnEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FurnaceBurnEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FurnaceBurnEvent: block={}, pos={:?}, fuel={}, burn_time={}",
                event.block.name,
                event.block_position,
                event.fuel.item.registry_key,
                event.burn_time
            );

            self.log(&message);
        })
    }
}

impl EventHandler<FurnaceExtractEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a FurnaceExtractEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] FurnaceExtractEvent: player={}, pos={:?}, item={}, count={}, experience={}",
                event.player.gameprofile.name,
                event.block_position,
                event.item.item.registry_key,
                event.item.item_count,
                event.experience
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<BrewEvent> for EventLogState {
    fn handle<'a>(&'a self, _server: &'a Arc<Server>, event: &'a BrewEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] BrewEvent: block={}, pos={:?}, ingredient={}, potions={}, fuel={}",
                event.block.name,
                event.block_position,
                event.ingredient.item.registry_key,
                event.potions.len(),
                event.fuel
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityBreedEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityBreedEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let breeder_name = event
                .breeder
                .as_ref()
                .map(|p| p.gameprofile.name.as_str())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityBreedEvent: mother={}, father={}, breeder={}, entity_type={}, experience={}",
                event.mother.get_entity().entity_type.resource_name,
                event.father.get_entity().entity_type.resource_name,
                breeder_name,
                event.baby_type.resource_name,
                event.experience
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityTameEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTameEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityTameEvent: entity={}, owner={}",
                event.entity.get_entity().entity_type.resource_name,
                event.owner.gameprofile.name
            );

            event
                .owner
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<EntityTargetEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTargetEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let target_name = event
                .target
                .as_ref()
                .map(|t| t.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let reason = event.reason.unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityTargetEvent: entity={}, target={}, reason={}",
                event.entity.get_entity().entity_type.resource_name,
                target_name,
                reason
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityTargetLivingEntityEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTargetLivingEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityTargetLivingEntityEvent: entity={}, target={}",
                event.entity.get_entity().entity_type.resource_name,
                event.target.get_entity().entity_type.resource_name
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityPickupItemEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityPickupItemEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityPickupItemEvent: entity={}, item={}, amount={}",
                event.entity.get_entity().entity_type.resource_name,
                event.item.item.registry_key,
                event.amount
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityShootBowEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityShootBowEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let consumable_name = event
                .consumable
                .as_ref()
                .map(|i| i.item.registry_key.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityShootBowEvent: player={}, projectile={}, bow={}, consumable={}, force={}",
                event.player.gameprofile.name,
                event.projectile.get_entity().entity_type.resource_name,
                event.bow.item.registry_key,
                consumable_name,
                event.force
            );

            event
                .player
                .send_system_message(&TextComponent::text(message.clone()))
                .await;
            self.log(&message);
        })
    }
}

impl EventHandler<EntityCombustByEntityEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityCombustByEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let message = format!(
                "[Cabbage Events] EntityCombustByEntityEvent: entity={}, combuster={}, duration={}",
                event.entity.get_entity().entity_type.resource_name,
                event.combuster.get_entity().entity_type.resource_name,
                event.duration
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityExplodeEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityExplodeEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let entity_name = event
                .entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityExplodeEvent: entity={}, blocks={}, yield={}",
                entity_name,
                event.affected_blocks.len(),
                event.yield_
            );

            self.log(&message);
        })
    }
}

impl EventHandler<ExplosionPrimeEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a ExplosionPrimeEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let entity_name = event
                .entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] ExplosionPrimeEvent: entity={}, radius={}, fire={}",
                entity_name, event.radius, event.fire
            );

            self.log(&message);
        })
    }
}

impl EventHandler<EntityTransformEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a EntityTransformEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let reason = event.reason.unwrap_or("none");
            let message = format!(
                "[Cabbage Events] EntityTransformEvent: entity={}, transform_to={}, reason={}",
                event.entity.get_entity().entity_type.resource_name,
                event.transform_to.resource_name,
                reason
            );

            self.log(&message);
        })
    }
}

impl EventHandler<PotionSplashEvent> for EventLogState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a PotionSplashEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }

            let hit_entity_name = event
                .hit_entity
                .as_ref()
                .map(|e| e.get_entity().entity_type.resource_name.as_ref())
                .unwrap_or("none");
            let message = format!(
                "[Cabbage Events] PotionSplashEvent: entity={}, hit_entity={}, affected={}, potion={}",
                event.entity.get_entity().entity_type.resource_name,
                hit_entity_name,
                event.affected_entities.len(),
                event.potion.item.registry_key
            );

            self.log(&message);
        })
    }
}
