//! Shared API surface for the Cabbage plugin crates.
//!
//! This crate holds the contracts that decouple Cabbage's feature crates from
//! each other: plain data snapshots, capability traits, and the service
//! wrappers registered with Pumpkin's plugin service registry. It exists so
//! that feature crates (and, after the future DLL split, separate plugin
//! binaries) can talk to each other without depending on concrete state
//! types.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use pumpkin::plugin::api::events::Payload;

/// Registry name for the Mob AI service (`MobAiService`).
pub const MOB_AI_SERVICE: &str = "cabbage_mob_ai";
/// Registry name for the core plugin services (`CoreServices`).
pub const CORE_SERVICE: &str = "cabbage_core";

/// Point-in-time snapshot of Mob AI engine counters.
pub struct MobAiMetricsSnapshot {
    pub active_path_jobs: usize,
    pub active_velocity_jobs: usize,
    pub total_worker_threads: usize,
    pub managed_mobs_count: usize,
    pub total_paths_completed: usize,
    pub total_velocities_completed: usize,
}

/// Capability trait exposed by the Mob AI engine.
pub trait MobAiApi: Send + Sync {
    /// Collects a snapshot of the engine's current metrics.
    fn metrics(&self) -> MobAiMetricsSnapshot;
    /// Enables or disables the Mob AI engine.
    fn set_enabled(&self, enabled: bool);
    /// Returns whether the Mob AI engine is currently enabled.
    fn is_enabled(&self) -> bool;
}

/// Service wrapper exposing the Mob AI engine through Pumpkin's service
/// registry.
///
/// `Payload` is implemented by hand (rather than via Pumpkin's
/// `#[derive(Event)]`, which only resolves inside the pumpkin crate) and uses
/// a `cabbage.`-prefixed name so it can never collide with a Pumpkin event
/// name during the registry's name-based downcast.
pub struct MobAiService(pub Arc<dyn MobAiApi>);

impl Payload for MobAiService {
    fn get_name_static() -> &'static str {
        "cabbage.MobAiService"
    }

    fn get_name(&self) -> &'static str {
        Self::get_name_static()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Core shared services provided by the Cabbage plugin.
pub struct CoreServices {
    /// The plugin's data folder (`./plugins/Cabbage`).
    pub data_folder: PathBuf,
    /// Appends a line to the plugin's event log (`output.log`).
    pub log_event: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Payload for CoreServices {
    fn get_name_static() -> &'static str {
        "cabbage.CoreServices"
    }

    fn get_name(&self) -> &'static str {
        Self::get_name_static()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
