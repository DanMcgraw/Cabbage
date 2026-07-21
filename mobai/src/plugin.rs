//! Mob AI module lifecycle and event registration.
//!
//! `cabbage-core` owns the native plugin DLL export and delegates the Mob AI
//! portion of that lifecycle to this module. Keeping the registration code
//! here preserves the Mob AI crate boundary without producing a second DLL.

use std::sync::Arc;

use pumpkin::plugin::{
    Context, EventPriority,
    api::events::{
        block::{block_break::BlockBreakEvent, block_place::BlockPlaceEvent},
        entity::{
            ChunkEntityLoadEvent, ChunkEntityUnloadEvent, EntityRemoveEvent, EntitySpawnEvent,
        },
        world::chunk_send::ChunkSend,
    },
    server::server_tick_start::ServerTickStartEvent,
};

use crate::{MobAiApiAdapter, MobAiState};

/// Stateful Mob AI portion of the combined Cabbage plugin.
pub struct MobAiModule {
    state: Arc<MobAiState>,
}

impl Default for MobAiModule {
    fn default() -> Self {
        Self {
            state: Arc::new(MobAiState::default()),
        }
    }
}

impl MobAiModule {
    /// Registers the Mob AI event handlers and service.
    pub async fn load(&mut self, context: &Arc<Context>) {
        context
            .register_event::<ServerTickStartEvent, _>(
                self.state.clone(),
                EventPriority::Normal,
                true,
            )
            .await;
        context
            .register_event::<EntitySpawnEvent, _>(self.state.clone(), EventPriority::Normal, false)
            .await;
        context
            .register_event::<EntityRemoveEvent, _>(
                self.state.clone(),
                EventPriority::Normal,
                false,
            )
            .await;
        context
            .register_event::<ChunkEntityLoadEvent, _>(
                self.state.clone(),
                EventPriority::Normal,
                false,
            )
            .await;
        context
            .register_event::<ChunkEntityUnloadEvent, _>(
                self.state.clone(),
                EventPriority::Normal,
                false,
            )
            .await;
        context
            .register_event::<ChunkSend, _>(self.state.clone(), EventPriority::Normal, false)
            .await;
        context
            .register_event::<BlockPlaceEvent, _>(self.state.clone(), EventPriority::Normal, false)
            .await;
        context
            .register_event::<BlockBreakEvent, _>(self.state.clone(), EventPriority::Normal, false)
            .await;

        context
            .register_service(
                cabbage_api::MOB_AI_SERVICE,
                Arc::new(cabbage_api::MobAiService(Arc::new(MobAiApiAdapter(
                    self.state.clone(),
                )))),
            )
            .await;
    }

    /// Gates retained event handlers when the combined plugin unloads.
    pub fn unload(&mut self) {
        // Event handlers are never auto-removed and the DLL stays mapped,
        // so gate every handler on the active flag.
        self.state.set_active(false);
    }
}
