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
}

impl Display for BranchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Supported MMO skills, grouped into three branches.
///
/// The original two-skill model (`Mining`, `Combat`) was replaced by this
/// three-branch model. `Mining` kept its identity and storage key; `Combat`
/// was retired rather than extended. See `db.rs` for the one-time, idempotent
/// migration that preserves legacy `Combat` XP in a legacy record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillId {
    // Frontier
    Agriculture,
    Herbalism,
    Woodcutting,
    Mining,
    Excavation,
    Fishing,
    Husbandry,
    Taming,
    // Warfare
    Blades,
    Axes,
    Archery,
    Unarmed,
    Defense,
    Acrobatics,
    Sorcery,
    // Enterprise
    Smithing,
    Repair,
    Salvage,
    Alchemy,
    Enchanting,
    Tinkering,
    Trading,
    Charisma,
}

impl SkillId {
    /// Frontier branch skills in canonical order.
    pub const FRONTIER: &[SkillId] = &[
        SkillId::Agriculture,
        SkillId::Herbalism,
        SkillId::Woodcutting,
        SkillId::Mining,
        SkillId::Excavation,
        SkillId::Fishing,
        SkillId::Husbandry,
        SkillId::Taming,
    ];

    /// Warfare branch skills in canonical order.
    pub const WARFARE: &[SkillId] = &[
        SkillId::Blades,
        SkillId::Axes,
        SkillId::Archery,
        SkillId::Unarmed,
        SkillId::Defense,
        SkillId::Acrobatics,
        SkillId::Sorcery,
    ];

    /// Enterprise branch skills in canonical order.
    pub const ENTERPRISE: &[SkillId] = &[
        SkillId::Smithing,
        SkillId::Repair,
        SkillId::Salvage,
        SkillId::Alchemy,
        SkillId::Enchanting,
        SkillId::Tinkering,
        SkillId::Trading,
        SkillId::Charisma,
    ];

    /// All skills in canonical order, branch by branch.
    pub const ALL: &[SkillId] = &[
        SkillId::Agriculture,
        SkillId::Herbalism,
        SkillId::Woodcutting,
        SkillId::Mining,
        SkillId::Excavation,
        SkillId::Fishing,
        SkillId::Husbandry,
        SkillId::Taming,
        SkillId::Blades,
        SkillId::Axes,
        SkillId::Archery,
        SkillId::Unarmed,
        SkillId::Defense,
        SkillId::Acrobatics,
        SkillId::Sorcery,
        SkillId::Smithing,
        SkillId::Repair,
        SkillId::Salvage,
        SkillId::Alchemy,
        SkillId::Enchanting,
        SkillId::Tinkering,
        SkillId::Trading,
        SkillId::Charisma,
    ];

    /// Stable storage key used in SQLite and RON. Never reuse a key for a
    /// different skill: existing databases address rows by this string.
    pub fn as_str(self) -> &'static str {
        match self {
            SkillId::Agriculture => "Agriculture",
            SkillId::Herbalism => "Herbalism",
            SkillId::Woodcutting => "Woodcutting",
            SkillId::Mining => "Mining",
            SkillId::Excavation => "Excavation",
            SkillId::Fishing => "Fishing",
            SkillId::Husbandry => "Husbandry",
            SkillId::Taming => "Taming",
            SkillId::Blades => "Blades",
            SkillId::Axes => "Axes",
            SkillId::Archery => "Archery",
            SkillId::Unarmed => "Unarmed",
            SkillId::Defense => "Defense",
            SkillId::Acrobatics => "Acrobatics",
            SkillId::Sorcery => "Sorcery",
            SkillId::Smithing => "Smithing",
            SkillId::Repair => "Repair",
            SkillId::Salvage => "Salvage",
            SkillId::Alchemy => "Alchemy",
            SkillId::Enchanting => "Enchanting",
            SkillId::Tinkering => "Tinkering",
            SkillId::Trading => "Trading",
            SkillId::Charisma => "Charisma",
        }
    }

    /// User-facing name.
    pub fn display_name(self) -> &'static str {
        self.as_str()
    }

    /// The branch this skill belongs to.
    pub fn branch(self) -> BranchId {
        match self {
            SkillId::Agriculture
            | SkillId::Herbalism
            | SkillId::Woodcutting
            | SkillId::Mining
            | SkillId::Excavation
            | SkillId::Fishing
            | SkillId::Husbandry
            | SkillId::Taming => BranchId::Frontier,
            SkillId::Blades
            | SkillId::Axes
            | SkillId::Archery
            | SkillId::Unarmed
            | SkillId::Defense
            | SkillId::Acrobatics
            | SkillId::Sorcery => BranchId::Warfare,
            SkillId::Smithing
            | SkillId::Repair
            | SkillId::Salvage
            | SkillId::Alchemy
            | SkillId::Enchanting
            | SkillId::Tinkering
            | SkillId::Trading
            | SkillId::Charisma => BranchId::Enterprise,
        }
    }

    /// Parse a skill from a user-supplied name (case-insensitive).
    pub fn from_name(name: &str) -> Option<SkillId> {
        let needle = name.trim().to_ascii_lowercase();
        SkillId::ALL
            .iter()
            .copied()
            .find(|skill| skill.as_str().to_ascii_lowercase() == needle)
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
        assert_eq!(SkillId::ALL.len(), 23);
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
    }

    #[test]
    fn storage_keys_are_unique() {
        let mut keys: Vec<&str> = SkillId::ALL.iter().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SkillId::ALL.len());
    }

    #[test]
    fn from_name_is_case_insensitive() {
        assert_eq!(SkillId::from_name("mining"), Some(SkillId::Mining));
        assert_eq!(SkillId::from_name("MINING"), Some(SkillId::Mining));
        assert_eq!(
            SkillId::from_name("woodcutting"),
            Some(SkillId::Woodcutting)
        );
        assert_eq!(SkillId::from_name("combat"), None);
        assert_eq!(SkillId::from_name("notaskill"), None);
    }

    #[test]
    fn display_matches_storage_key() {
        for skill in SkillId::ALL {
            assert_eq!(skill.to_string(), skill.as_str());
            assert_eq!(skill.display_name(), skill.as_str());
        }
    }
}
