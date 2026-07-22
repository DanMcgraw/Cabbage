use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

/// One of the three MMO skill branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BranchId {
    Frontier,
    Warfare,
    Enterprise,
}

impl BranchId {
    /// All branches in canonical display order.
    pub const ALL: &[BranchId] = &[BranchId::Frontier, BranchId::Warfare, BranchId::Enterprise];

    /// User-facing name.
    pub fn display_name(self) -> &'static str {
        match self {
            BranchId::Frontier => "Frontier",
            BranchId::Warfare => "Warfare",
            BranchId::Enterprise => "Enterprise",
        }
    }

    /// Member skills in canonical display order.
    pub fn skills(self) -> &'static [SkillId] {
        match self {
            BranchId::Frontier => SkillId::FRONTIER,
            BranchId::Warfare => SkillId::WARFARE,
            BranchId::Enterprise => SkillId::ENTERPRISE,
        }
    }

    /// Parse a branch from a user-supplied name (case-insensitive).
    pub fn from_name(name: &str) -> Option<BranchId> {
        let needle = name.trim().to_ascii_lowercase();
        BranchId::ALL
            .iter()
            .copied()
            .find(|branch| branch.display_name().to_ascii_lowercase() == needle)
    }
}

impl Display for BranchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Supported MMO skills, grouped into three branches of six skills each.
///
/// The original two-skill model (`Mining`, `Combat`) was replaced by the
/// three-branch model; `Mining` kept its identity and storage key while
/// `Combat` was retired (see `db.rs` schema v1 for the preserved legacy
/// record). The six-skill branch consolidation (SQLite schema v2, config
/// schema v2) then merged five pairs of skills into shared progression
/// tracks, retiring ten variants:
///
/// - `Agriculture` + `Herbalism` → `Cultivation`
/// - `Husbandry` + `Taming` → `AnimalHandling`
/// - `Unarmed` + `Acrobatics` → `Athletics`
/// - `Repair` + `Salvage` → `Maintenance`
/// - `Trading` + `Charisma` → `Commerce`
///
/// Retired names are still accepted as deserialization aliases and by
/// [`SkillId::from_name`] for one compatibility period, but they always
/// resolve to their canonical destination and are never written back out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillId {
    // Frontier
    #[serde(alias = "Agriculture", alias = "Herbalism")]
    Cultivation,
    Woodcutting,
    Mining,
    Excavation,
    Fishing,
    #[serde(alias = "Husbandry", alias = "Taming")]
    AnimalHandling,
    // Warfare
    Blades,
    Axes,
    Archery,
    #[serde(alias = "Unarmed", alias = "Acrobatics")]
    Athletics,
    Defense,
    Sorcery,
    // Enterprise
    Smithing,
    #[serde(alias = "Repair", alias = "Salvage")]
    Maintenance,
    Alchemy,
    Enchanting,
    Tinkering,
    #[serde(alias = "Trading", alias = "Charisma")]
    Commerce,
}

impl SkillId {
    /// Frontier branch skills in canonical order.
    pub const FRONTIER: &[SkillId] = &[
        SkillId::Cultivation,
        SkillId::Woodcutting,
        SkillId::Mining,
        SkillId::Excavation,
        SkillId::Fishing,
        SkillId::AnimalHandling,
    ];

    /// Warfare branch skills in canonical order.
    pub const WARFARE: &[SkillId] = &[
        SkillId::Blades,
        SkillId::Axes,
        SkillId::Archery,
        SkillId::Athletics,
        SkillId::Defense,
        SkillId::Sorcery,
    ];

    /// Enterprise branch skills in canonical order.
    pub const ENTERPRISE: &[SkillId] = &[
        SkillId::Smithing,
        SkillId::Maintenance,
        SkillId::Alchemy,
        SkillId::Enchanting,
        SkillId::Tinkering,
        SkillId::Commerce,
    ];

    /// All skills in canonical order, branch by branch.
    pub const ALL: &[SkillId] = &[
        SkillId::Cultivation,
        SkillId::Woodcutting,
        SkillId::Mining,
        SkillId::Excavation,
        SkillId::Fishing,
        SkillId::AnimalHandling,
        SkillId::Blades,
        SkillId::Axes,
        SkillId::Archery,
        SkillId::Athletics,
        SkillId::Defense,
        SkillId::Sorcery,
        SkillId::Smithing,
        SkillId::Maintenance,
        SkillId::Alchemy,
        SkillId::Enchanting,
        SkillId::Tinkering,
        SkillId::Commerce,
    ];

    /// Retired skill names accepted by [`SkillId::from_name`] (lowercase),
    /// mapped to the canonical skill that replaced them.
    const LEGACY_ALIASES: &[(&str, SkillId)] = &[
        ("agriculture", SkillId::Cultivation),
        ("herbalism", SkillId::Cultivation),
        ("husbandry", SkillId::AnimalHandling),
        ("taming", SkillId::AnimalHandling),
        ("unarmed", SkillId::Athletics),
        ("acrobatics", SkillId::Athletics),
        ("repair", SkillId::Maintenance),
        ("salvage", SkillId::Maintenance),
        ("trading", SkillId::Commerce),
        ("charisma", SkillId::Commerce),
    ];

    /// Stable storage key used in SQLite and RON. Never reuse a key for a
    /// different skill: existing databases address rows by this string.
    pub fn as_str(self) -> &'static str {
        match self {
            SkillId::Cultivation => "Cultivation",
            SkillId::Woodcutting => "Woodcutting",
            SkillId::Mining => "Mining",
            SkillId::Excavation => "Excavation",
            SkillId::Fishing => "Fishing",
            SkillId::AnimalHandling => "AnimalHandling",
            SkillId::Blades => "Blades",
            SkillId::Axes => "Axes",
            SkillId::Archery => "Archery",
            SkillId::Athletics => "Athletics",
            SkillId::Defense => "Defense",
            SkillId::Sorcery => "Sorcery",
            SkillId::Smithing => "Smithing",
            SkillId::Maintenance => "Maintenance",
            SkillId::Alchemy => "Alchemy",
            SkillId::Enchanting => "Enchanting",
            SkillId::Tinkering => "Tinkering",
            SkillId::Commerce => "Commerce",
        }
    }

    /// User-facing name.
    pub fn display_name(self) -> &'static str {
        self.as_str()
    }

    /// The branch this skill belongs to.
    pub fn branch(self) -> BranchId {
        match self {
            SkillId::Cultivation
            | SkillId::Woodcutting
            | SkillId::Mining
            | SkillId::Excavation
            | SkillId::Fishing
            | SkillId::AnimalHandling => BranchId::Frontier,
            SkillId::Blades
            | SkillId::Axes
            | SkillId::Archery
            | SkillId::Athletics
            | SkillId::Defense
            | SkillId::Sorcery => BranchId::Warfare,
            SkillId::Smithing
            | SkillId::Maintenance
            | SkillId::Alchemy
            | SkillId::Enchanting
            | SkillId::Tinkering
            | SkillId::Commerce => BranchId::Enterprise,
        }
    }

    /// Parse a skill from a user-supplied name (case-insensitive). Retired
    /// skill names resolve to their canonical destination for one
    /// compatibility period; only canonical variants are ever returned.
    pub fn from_name(name: &str) -> Option<SkillId> {
        let needle = name.trim().to_ascii_lowercase();
        SkillId::ALL
            .iter()
            .copied()
            .find(|skill| skill.as_str().to_ascii_lowercase() == needle)
            .or_else(|| {
                SkillId::LEGACY_ALIASES
                    .iter()
                    .copied()
                    .find(|(alias, _)| *alias == needle)
                    .map(|(_, skill)| skill)
            })
    }
}

impl Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_skills_are_branch_members_exactly_once() {
        assert_eq!(SkillId::ALL.len(), 18);
        for skill in SkillId::ALL {
            let members = skill.branch().skills();
            assert!(members.contains(skill));
            assert_eq!(members.iter().filter(|s| *s == skill).count(), 1);
        }
    }

    #[test]
    fn branch_skill_lists_cover_all_skills() {
        let total = SkillId::FRONTIER.len() + SkillId::WARFARE.len() + SkillId::ENTERPRISE.len();
        assert_eq!(total, SkillId::ALL.len());
        assert_eq!(SkillId::FRONTIER.len(), 6);
        assert_eq!(SkillId::WARFARE.len(), 6);
        assert_eq!(SkillId::ENTERPRISE.len(), 6);
    }

    #[test]
    fn branch_arrays_follow_the_canonical_row_order() {
        assert_eq!(
            SkillId::FRONTIER,
            &[
                SkillId::Cultivation,
                SkillId::Woodcutting,
                SkillId::Mining,
                SkillId::Excavation,
                SkillId::Fishing,
                SkillId::AnimalHandling,
            ]
        );
        assert_eq!(
            SkillId::WARFARE,
            &[
                SkillId::Blades,
                SkillId::Axes,
                SkillId::Archery,
                SkillId::Athletics,
                SkillId::Defense,
                SkillId::Sorcery,
            ]
        );
        assert_eq!(
            SkillId::ENTERPRISE,
            &[
                SkillId::Smithing,
                SkillId::Maintenance,
                SkillId::Alchemy,
                SkillId::Enchanting,
                SkillId::Tinkering,
                SkillId::Commerce,
            ]
        );
    }

    #[test]
    fn storage_keys_are_unique() {
        let mut keys: Vec<&str> = SkillId::ALL.iter().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SkillId::ALL.len());
    }

    #[test]
    fn branch_from_name_is_case_insensitive() {
        assert_eq!(BranchId::from_name("frontier"), Some(BranchId::Frontier));
        assert_eq!(BranchId::from_name("WARFARE"), Some(BranchId::Warfare));
        assert_eq!(
            BranchId::from_name(" Enterprise "),
            Some(BranchId::Enterprise)
        );
        assert_eq!(BranchId::from_name("mining"), None);
        assert_eq!(BranchId::from_name(""), None);
    }

    #[test]
    fn from_name_is_case_insensitive() {
        assert_eq!(SkillId::from_name("mining"), Some(SkillId::Mining));
        assert_eq!(SkillId::from_name("MINING"), Some(SkillId::Mining));
        assert_eq!(
            SkillId::from_name("woodcutting"),
            Some(SkillId::Woodcutting)
        );
        assert_eq!(
            SkillId::from_name("animalhandling"),
            Some(SkillId::AnimalHandling)
        );
        assert_eq!(SkillId::from_name("combat"), None);
        assert_eq!(SkillId::from_name("notaskill"), None);
    }

    #[test]
    fn from_name_routes_retired_names_to_canonical_destinations() {
        let expected = [
            ("agriculture", SkillId::Cultivation),
            ("herbalism", SkillId::Cultivation),
            ("husbandry", SkillId::AnimalHandling),
            ("taming", SkillId::AnimalHandling),
            ("unarmed", SkillId::Athletics),
            ("acrobatics", SkillId::Athletics),
            ("repair", SkillId::Maintenance),
            ("salvage", SkillId::Maintenance),
            ("trading", SkillId::Commerce),
            ("charisma", SkillId::Commerce),
        ];
        for (alias, destination) in expected {
            assert_eq!(SkillId::from_name(alias), Some(destination), "{alias}");
        }
        // Aliases are case-insensitive too.
        assert_eq!(SkillId::from_name("Repair"), Some(SkillId::Maintenance));
        assert_eq!(SkillId::from_name("HERBALISM"), Some(SkillId::Cultivation));
    }

    #[test]
    fn retired_spellings_deserialize_to_canonical_skills() {
        assert_eq!(
            ron::from_str::<SkillId>("Agriculture").unwrap(),
            SkillId::Cultivation
        );
        assert_eq!(
            ron::from_str::<SkillId>("Salvage").unwrap(),
            SkillId::Maintenance
        );
        assert_eq!(
            ron::from_str::<SkillId>("Charisma").unwrap(),
            SkillId::Commerce
        );
    }

    #[test]
    fn canonical_names_round_trip_through_ron() {
        for skill in SkillId::ALL {
            let serialized = ron::ser::to_string(skill).unwrap();
            assert_eq!(serialized, skill.as_str());
            assert_eq!(ron::from_str::<SkillId>(&serialized).unwrap(), *skill);
        }
    }

    #[test]
    fn display_matches_storage_key() {
        for skill in SkillId::ALL {
            assert_eq!(skill.to_string(), skill.as_str());
            assert_eq!(skill.display_name(), skill.as_str());
        }
    }
}
