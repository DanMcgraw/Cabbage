//! Native plugin entry point for the standalone `Cabbage.MobAi` plugin DLL.
//!
//! The rlib half of this crate stays plugin-agnostic for testability; this
//! module owns the DLL exports, metadata, event registration, and the
//! `MobAiService` publication consumed by Cabbage.Core metrics.

use std::{mem::MaybeUninit, sync::Arc};

use pumpkin::plugin::{
    Context, EventPriority, PLUGIN_API_VERSION, Plugin, PluginFuture, PluginMetadata,
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

const PLUGIN_NAME: &str = "Cabbage.MobAi";

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
        description: "Cabbage multithreaded mob AI plugin.".to_string(),
        dependencies: vec!["Cabbage.Core".to_string()],
        permissions: Vec::new(),
    };

    unsafe {
        core::ptr::addr_of_mut!(METADATA)
            .cast::<PluginMetadata>()
            .write(metadata);
    }
}

struct MobAiPlugin {
    state: Arc<MobAiState>,
}

impl Plugin for MobAiPlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            context
                .register_event::<ServerTickStartEvent, _>(
                    self.state.clone(),
                    EventPriority::Normal,
                    true,
                )
                .await;
            context
                .register_event::<EntitySpawnEvent, _>(
                    self.state.clone(),
                    EventPriority::Normal,
                    false,
                )
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
                .register_event::<BlockPlaceEvent, _>(
                    self.state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;
            context
                .register_event::<BlockBreakEvent, _>(
                    self.state.clone(),
                    EventPriority::Normal,
                    false,
                )
                .await;

            context
                .register_service(
                    cabbage_api::MOB_AI_SERVICE,
                    Arc::new(cabbage_api::MobAiService(Arc::new(MobAiApiAdapter(
                        self.state.clone(),
                    )))),
                )
                .await;

            Ok(())
        })
    }

    fn on_unload(&mut self, _context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            // Event handlers are never auto-removed and the DLL stays mapped,
            // so gate every handler on the active flag.
            self.state.set_active(false);
            Ok(())
        })
    }
}

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(MobAiPlugin {
        state: Arc::new(MobAiState::default()),
    })
}
