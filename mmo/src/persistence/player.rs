//! Player payload codec: compact per-player capability state, stored via
//! `Context` player data.
//!
//! Skill XP itself stays in SQLite (the plan's architectural decisions);
//! this payload holds only small capability flags that must travel with the
//! player and survive restarts.

// Consumed by perk activation paths landing in Phases 1-4.
#![allow(dead_code)]

use pumpkin::plugin::Context;
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use uuid::Uuid;

/// Key for the versioned Cabbage player profile payload.
pub const PROFILE_DATA_KEY: &str = "profile_v1";

/// Key for the versioned Cabbage reputation ledger payload.
pub const REPUTATION_DATA_KEY: &str = "rep_v1";

/// Cabbage player capability state, version 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerProfileV1 {
    /// Whether this player wants perk effects to fire for them.
    pub perks_enabled: bool,
    /// Perk identifiers the player has unlocked through play.
    pub unlocked_perks: Vec<String>,
}

impl Default for PlayerProfileV1 {
    fn default() -> Self {
        Self {
            perks_enabled: true,
            unlocked_perks: Vec::new(),
        }
    }
}

impl PlayerProfileV1 {
    pub const VERSION: i32 = 1;

    pub fn encode(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.put_int("version", Self::VERSION);
        compound.put_bool("perks_enabled", self.perks_enabled);
        compound.put_list(
            "unlocked_perks",
            self.unlocked_perks
                .iter()
                .map(|perk| NbtTag::String(perk.clone().into_boxed_str()))
                .collect(),
        );
        NbtTag::Compound(compound)
    }

    /// Decode a payload, rejecting malformed or version-incompatible data.
    pub fn decode(tag: &NbtTag) -> Option<Self> {
        let compound = tag.extract_compound()?;
        if compound.get_int("version")? != Self::VERSION {
            return None;
        }
        let perks_enabled = compound.get_bool("perks_enabled")?;
        let unlocked_perks = compound
            .get_list("unlocked_perks")?
            .iter()
            .map(|tag| match tag {
                NbtTag::String(value) => Some(value.to_string()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            perks_enabled,
            unlocked_perks,
        })
    }

    /// Read the Cabbage profile for a player, if present and valid.
    ///
    /// Works for online and offline players (Pumpkin loads from disk).
    pub async fn read(context: &Context, player_uuid: Uuid) -> Option<Self> {
        context
            .get_player_data(player_uuid, PROFILE_DATA_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|tag| Self::decode(&tag))
    }

    /// Read the profile, falling back to defaults when absent or corrupt.
    pub async fn read_or_default(context: &Context, player_uuid: Uuid) -> Self {
        Self::read(context, player_uuid).await.unwrap_or_default()
    }

    /// Persist the Cabbage profile for a player.
    pub async fn write(&self, context: &Context, player_uuid: Uuid) -> Result<(), String> {
        context
            .set_player_data(player_uuid, PROFILE_DATA_KEY, self.encode())
            .await
            .map_err(|error| format!("failed to write player profile: {error}"))
    }
}

/// Cabbage reputation ledger, version 1: faction standings for future
/// Commerce effects. Unused while the trading/charisma activity configs stay
/// disabled; the codec exists so reputation can be recorded without a schema
/// change later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReputationLedgerV1 {
    /// Faction name → standing.
    pub factions: std::collections::HashMap<String, i32>,
}

impl ReputationLedgerV1 {
    pub const VERSION: i32 = 1;

    pub fn encode(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.put_int("version", Self::VERSION);
        let mut factions = NbtCompound::new();
        for (faction, standing) in &self.factions {
            factions.put_int(faction, *standing);
        }
        compound.put_compound("factions", factions);
        NbtTag::Compound(compound)
    }

    /// Decode a payload, rejecting malformed or version-incompatible data.
    pub fn decode(tag: &NbtTag) -> Option<Self> {
        let compound = tag.extract_compound()?;
        if compound.get_int("version")? != Self::VERSION {
            return None;
        }
        let factions = compound
            .get_compound("factions")?
            .child_tags
            .iter()
            .map(|(name, tag)| match tag {
                NbtTag::Int(value) => Some((name.to_string(), *value)),
                _ => None,
            })
            .collect::<Option<std::collections::HashMap<_, _>>>()?;
        Some(Self { factions })
    }

    /// Read the ledger for a player, if present and valid.
    pub async fn read(context: &Context, player_uuid: Uuid) -> Option<Self> {
        context
            .get_player_data(player_uuid, REPUTATION_DATA_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|tag| Self::decode(&tag))
    }

    /// Persist the ledger for a player.
    pub async fn write(&self, context: &Context, player_uuid: Uuid) -> Result<(), String> {
        context
            .set_player_data(player_uuid, REPUTATION_DATA_KEY, self.encode())
            .await
            .map_err(|error| format!("failed to write reputation ledger: {error}"))
    }

    /// Add standing to a faction and persist the ledger.
    #[allow(dead_code)] // used when Commerce effects un-block
    pub async fn add_reputation(
        context: &Context,
        player_uuid: Uuid,
        faction: &str,
        delta: i32,
    ) -> Result<(), String> {
        let mut ledger = Self::read(context, player_uuid).await.unwrap_or_default();
        *ledger.factions.entry(faction.to_string()).or_insert(0) += delta;
        ledger.write(context, player_uuid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_all_fields() {
        let data = PlayerProfileV1 {
            perks_enabled: false,
            unlocked_perks: vec!["mining.vein_miner".to_string()],
        };
        let decoded = PlayerProfileV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn default_payload_round_trips() {
        let data = PlayerProfileV1::default();
        let decoded = PlayerProfileV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn wrong_version_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", 99);
        compound.put_bool("perks_enabled", true);
        compound.put_list("unlocked_perks", vec![]);
        assert!(PlayerProfileV1::decode(&NbtTag::Compound(compound)).is_none());
    }

    #[test]
    fn missing_fields_are_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", PlayerProfileV1::VERSION);
        assert!(PlayerProfileV1::decode(&NbtTag::Compound(compound)).is_none());
    }

    #[test]
    fn non_compound_tag_is_rejected() {
        assert!(PlayerProfileV1::decode(&NbtTag::Long(9)).is_none());
    }
}
