//! Block payload codec: crop fertilizer tier and deterministic quality seed,
//! stored via `Context` block metadata.
//!
//! Crop quality is decided at harvest time from this stored data (see the
//! plan's Frontier rules); the quality seed keeps per-crop rolls
//! deterministic across restarts.

// Consumed by the agriculture handlers (Cultivation track).
#![allow(dead_code)]

use pumpkin::{plugin::Context, world::World};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::position::BlockPos;

/// Key for the versioned Cabbage crop payload.
pub const CROP_DATA_KEY: &str = "crop_v1";

/// Cabbage crop metadata, version 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CropDataV1 {
    /// Fertilizer tier applied to this crop; 0 means unfertilized.
    pub fertilizer_tier: u8,
    /// Deterministic seed for quality rolls at harvest time.
    pub quality_seed: u64,
}

impl CropDataV1 {
    pub const VERSION: i32 = 1;

    pub fn encode(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.put_int("version", Self::VERSION);
        compound.put_byte("fertilizer_tier", self.fertilizer_tier as i8);
        compound.put_long("quality_seed", self.quality_seed as i64);
        compound
    }

    /// Decode a payload, rejecting malformed or version-incompatible data.
    pub fn decode(compound: &NbtCompound) -> Option<Self> {
        if compound.get_int("version")? != Self::VERSION {
            return None;
        }
        let fertilizer_tier = u8::try_from(compound.get_byte("fertilizer_tier")?).ok()?;
        let quality_seed = u64::try_from(compound.get_long("quality_seed")?).ok()?;
        Some(Self {
            fertilizer_tier,
            quality_seed,
        })
    }

    /// Read the Cabbage crop payload at a block position, if present and valid.
    pub fn read(context: &Context, world: &World, position: &BlockPos) -> Option<Self> {
        context
            .get_block_metadata(world, position, CROP_DATA_KEY)
            .ok()
            .flatten()
            .and_then(|compound| Self::decode(&compound))
    }

    /// Write the Cabbage crop payload at a block position.
    pub fn write(
        &self,
        context: &Context,
        world: &World,
        position: &BlockPos,
    ) -> Result<(), String> {
        context
            .set_block_metadata(world, position, CROP_DATA_KEY, Some(self.encode()))
            .map_err(|error| format!("failed to write crop data: {error}"))
    }

    /// Remove the Cabbage crop payload at a block position.
    #[allow(dead_code)] // used when crops are harvested or broken (Phase 1)
    pub fn clear(context: &Context, world: &World, position: &BlockPos) -> Result<(), String> {
        context
            .set_block_metadata(world, position, CROP_DATA_KEY, None)
            .map_err(|error| format!("failed to clear crop data: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_all_fields() {
        let data = CropDataV1 {
            fertilizer_tier: 2,
            quality_seed: 0xDEAD_BEEF_CAFE,
        };
        let decoded = CropDataV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn default_payload_round_trips() {
        let data = CropDataV1::default();
        let decoded = CropDataV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn wrong_version_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", 99);
        compound.put_byte("fertilizer_tier", 1);
        compound.put_long("quality_seed", 1);
        assert!(CropDataV1::decode(&compound).is_none());
    }

    #[test]
    fn missing_fields_are_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", CropDataV1::VERSION);
        assert!(CropDataV1::decode(&compound).is_none());
    }

    #[test]
    fn negative_seed_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", CropDataV1::VERSION);
        compound.put_byte("fertilizer_tier", 0);
        compound.put_long("quality_seed", -5);
        assert!(CropDataV1::decode(&compound).is_none());
    }
}
