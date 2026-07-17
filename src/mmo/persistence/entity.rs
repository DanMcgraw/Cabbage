//! Entity payload codec: pet traits, bond state, and recovery state, stored
//! via `Context` entity data.
//!
//! Entity data is never proof of ownership on its own; ownership checks must
//! always consult Pumpkin's actual tameable owner state (plan Frontier
//! rules). This payload only records Cabbage's pet profile.

// Consumed by Taming/Husbandry handlers landing in Phase 1.
#![allow(dead_code)]

use pumpkin::{entity::EntityBase, plugin::Context};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};

/// Key for the versioned Cabbage pet payload.
pub const PET_DATA_KEY: &str = "pet_v1";

/// Recovery state of a pet that went down in combat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PetRecoveryState {
    #[default]
    Active,
    Recovering,
    Downed,
}

impl PetRecoveryState {
    fn as_str(self) -> &'static str {
        match self {
            PetRecoveryState::Active => "active",
            PetRecoveryState::Recovering => "recovering",
            PetRecoveryState::Downed => "downed",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "active" => Some(PetRecoveryState::Active),
            "recovering" => Some(PetRecoveryState::Recovering),
            "downed" => Some(PetRecoveryState::Downed),
            _ => None,
        }
    }
}

/// Cabbage pet profile, version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PetDataV1 {
    pub traits: Vec<String>,
    /// Bond level with the owner; 0 means unbonded.
    pub bond: u32,
    pub recovery: PetRecoveryState,
}

impl PetDataV1 {
    pub const VERSION: i32 = 1;

    pub fn encode(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.put_int("version", Self::VERSION);
        compound.put_list(
            "traits",
            self.traits
                .iter()
                .map(|pet_trait| NbtTag::String(pet_trait.clone().into_boxed_str()))
                .collect(),
        );
        compound.put_int("bond", self.bond as i32);
        compound.put_string("recovery", self.recovery.as_str().to_string());
        NbtTag::Compound(compound)
    }

    /// Decode a payload, rejecting malformed or version-incompatible data.
    pub fn decode(tag: &NbtTag) -> Option<Self> {
        let compound = tag.extract_compound()?;
        if compound.get_int("version")? != Self::VERSION {
            return None;
        }
        let traits = compound
            .get_list("traits")?
            .iter()
            .map(|tag| match tag {
                NbtTag::String(value) => Some(value.to_string()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let bond = u32::try_from(compound.get_int("bond")?).ok()?;
        let recovery = PetRecoveryState::from_str(compound.get_string("recovery")?)?;
        Some(Self {
            traits,
            bond,
            recovery,
        })
    }

    /// Read the Cabbage pet profile from an entity, if present and valid.
    pub fn read(context: &Context, entity: &dyn EntityBase) -> Option<Self> {
        context
            .get_entity_data(entity, PET_DATA_KEY)
            .ok()
            .flatten()
            .and_then(|tag| Self::decode(&tag))
    }

    /// Write the Cabbage pet profile onto an entity.
    pub fn write(&self, context: &Context, entity: &dyn EntityBase) -> Result<(), String> {
        context
            .set_entity_data(entity, PET_DATA_KEY, self.encode())
            .map_err(|error| format!("failed to write pet data: {error}"))
    }

    /// Remove the Cabbage pet profile from an entity.
    #[allow(dead_code)] // used when a pet is released or removed (Phase 1)
    pub fn clear(context: &Context, entity: &dyn EntityBase) -> Result<(), String> {
        context
            .remove_entity_data(entity, PET_DATA_KEY)
            .map_err(|error| format!("failed to clear pet data: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_all_fields() {
        let data = PetDataV1 {
            traits: vec!["loyal".to_string(), "keen_nose".to_string()],
            bond: 7,
            recovery: PetRecoveryState::Recovering,
        };
        let decoded = PetDataV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn default_payload_round_trips() {
        let data = PetDataV1::default();
        let decoded = PetDataV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn wrong_version_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", 99);
        compound.put_list("traits", vec![]);
        compound.put_int("bond", 1);
        compound.put_string("recovery", "active".to_string());
        assert!(PetDataV1::decode(&NbtTag::Compound(compound)).is_none());
    }

    #[test]
    fn unknown_recovery_state_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", PetDataV1::VERSION);
        compound.put_list("traits", vec![]);
        compound.put_int("bond", 1);
        compound.put_string("recovery", "exploded".to_string());
        assert!(PetDataV1::decode(&NbtTag::Compound(compound)).is_none());
    }

    #[test]
    fn non_compound_tag_is_rejected() {
        assert!(PetDataV1::decode(&NbtTag::Int(3)).is_none());
    }
}
