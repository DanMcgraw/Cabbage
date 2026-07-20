//! Item payload codec: quality tier, creator, provenance, Masterwork, runes,
//! and infusions, stored on the `ItemStack` under the `cabbage` namespace.
//!
//! Reading and writing only ever touches the `item_v1` key in our namespace,
//! so unrelated item custom data (other plugins, other Cabbage keys) is
//! preserved untouched.

// Consumed by Frontier/Enterprise handlers landing in Phases 1-3.
#![allow(dead_code)]

use pumpkin_data::item_stack::ItemStack;
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use uuid::Uuid;

use super::CABBAGE_NAMESPACE;

/// Key for the versioned Cabbage item payload.
pub const ITEM_DATA_KEY: &str = "item_v1";

/// Cabbage item metadata, version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDataV1 {
    /// Quality tier; 0 means ungraded.
    pub quality_tier: u8,
    /// UUID of the player who created the item, when known.
    pub creator: Option<Uuid>,
    /// Short provenance marker (e.g. `smithing`, `ore_reveal`).
    pub provenance: Option<String>,
    pub masterwork: bool,
    pub runes: Vec<String>,
    pub infusions: Vec<String>,
}

impl ItemDataV1 {
    pub const VERSION: i32 = 1;

    pub fn encode(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.put_int("version", Self::VERSION);
        compound.put_byte("quality_tier", self.quality_tier as i8);
        if let Some(creator) = self.creator {
            compound.put_string("creator", creator.to_string());
        }
        if let Some(provenance) = &self.provenance {
            compound.put_string("provenance", provenance.clone());
        }
        compound.put_bool("masterwork", self.masterwork);
        if !self.runes.is_empty() {
            compound.put_list(
                "runes",
                self.runes
                    .iter()
                    .map(|rune| NbtTag::String(rune.clone().into_boxed_str()))
                    .collect(),
            );
        }
        if !self.infusions.is_empty() {
            compound.put_list(
                "infusions",
                self.infusions
                    .iter()
                    .map(|infusion| NbtTag::String(infusion.clone().into_boxed_str()))
                    .collect(),
            );
        }
        NbtTag::Compound(compound)
    }

    /// Decode a payload, rejecting malformed or version-incompatible data.
    pub fn decode(tag: &NbtTag) -> Option<Self> {
        let compound = tag.extract_compound()?;
        if compound.get_int("version")? != Self::VERSION {
            return None;
        }
        let quality_tier = u8::try_from(compound.get_byte("quality_tier")?).ok()?;
        let creator = compound
            .get_string("creator")
            .and_then(|raw| Uuid::parse_str(raw).ok());
        let provenance = compound.get_string("provenance").map(str::to_string);
        let masterwork = compound.get_bool("masterwork").unwrap_or(false);
        let runes = decode_string_list(compound.get_list("runes"))?;
        let infusions = decode_string_list(compound.get_list("infusions"))?;
        Some(Self {
            quality_tier,
            creator,
            provenance,
            masterwork,
            runes,
            infusions,
        })
    }

    /// Read the Cabbage payload from an item stack, if present and valid.
    pub fn read(stack: &ItemStack) -> Option<Self> {
        stack
            .get_custom_data(CABBAGE_NAMESPACE, ITEM_DATA_KEY)
            .as_ref()
            .and_then(Self::decode)
    }

    /// Write the Cabbage payload onto an item stack.
    pub fn write(&self, stack: &mut ItemStack) {
        stack.set_custom_data(CABBAGE_NAMESPACE, ITEM_DATA_KEY, self.encode());
    }

    /// Remove the Cabbage payload from an item stack.
    #[allow(dead_code)] // used by Enterprise salvage/repair flows
    pub fn clear(stack: &mut ItemStack) {
        stack.remove_custom_data(CABBAGE_NAMESPACE, ITEM_DATA_KEY);
    }
}

fn decode_string_list(list: Option<&[NbtTag]>) -> Option<Vec<String>> {
    // Absent means empty: `encode` omits empty lists.
    let list = list.unwrap_or(&[]);
    let mut strings = Vec::with_capacity(list.len());
    for tag in list {
        match tag {
            NbtTag::String(value) => strings.push(value.to_string()),
            _ => return None,
        }
    }
    Some(strings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ItemDataV1 {
        ItemDataV1 {
            quality_tier: 3,
            creator: Some(Uuid::new_v4()),
            provenance: Some("smithing".to_string()),
            masterwork: true,
            runes: vec!["rune_of_embers".to_string()],
            infusions: vec![],
        }
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let data = sample();
        let decoded = ItemDataV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn default_payload_round_trips() {
        let data = ItemDataV1::default();
        let decoded = ItemDataV1::decode(&data.encode()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn wrong_version_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", 99);
        compound.put_byte("quality_tier", 1);
        assert!(ItemDataV1::decode(&NbtTag::Compound(compound)).is_none());
    }

    #[test]
    fn non_compound_tag_is_rejected() {
        assert!(ItemDataV1::decode(&NbtTag::Int(1)).is_none());
    }

    #[test]
    fn missing_required_fields_are_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", ItemDataV1::VERSION);
        // no quality_tier
        assert!(ItemDataV1::decode(&NbtTag::Compound(compound)).is_none());
    }

    #[test]
    fn corrupt_creator_uuid_is_treated_as_absent() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", ItemDataV1::VERSION);
        compound.put_byte("quality_tier", 2);
        compound.put_string("creator", "not-a-uuid".to_string());
        let decoded = ItemDataV1::decode(&NbtTag::Compound(compound)).unwrap();
        assert_eq!(decoded.creator, None);
        assert_eq!(decoded.quality_tier, 2);
    }

    #[test]
    fn corrupt_list_element_is_rejected() {
        let mut compound = NbtCompound::new();
        compound.put_int("version", ItemDataV1::VERSION);
        compound.put_byte("quality_tier", 1);
        compound.put_list("runes", vec![NbtTag::Int(7)]);
        assert!(ItemDataV1::decode(&NbtTag::Compound(compound)).is_none());
    }
}
