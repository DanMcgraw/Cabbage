//! Typed codecs for Cabbage data stored in Pumpkin-owned persistent stores.
//!
//! All Cabbage payloads are versioned `NbtCompound`s decoded through these
//! codecs; handlers must never scatter raw NBT key strings. Malformed or
//! version-incompatible payloads decode to `None` so a corrupt entry can
//! never crash an event handler.
//!
//! Storage ownership per the plan's architectural decisions:
//! - Item quality/provenance rides on the `ItemStack` itself (`item` module).
//! - Player capability state uses `Context` player data (`player` module).
//! - Pet state uses `Context` entity data (`entity` module).
//! - Crop state uses `Context` block metadata (`block` module).

pub(crate) mod block;
pub(crate) mod entity;
pub(crate) mod item;
pub(crate) mod player;

/// Namespace used for all Cabbage item custom data. `Context` player/entity/
/// block data is namespaced by Pumpkin under the plugin name automatically.
pub const CABBAGE_NAMESPACE: &str = "cabbage";

// Re-exported codec types form the persistence module's API surface; branch
// handlers import them from here as they land in Phases 1-3.
#[allow(unused_imports)]
pub(crate) use block::CropDataV1;
#[allow(unused_imports)]
pub(crate) use entity::PetDataV1;
#[allow(unused_imports)]
pub(crate) use item::ItemDataV1;
#[allow(unused_imports)]
pub(crate) use player::PlayerProfileV1;
