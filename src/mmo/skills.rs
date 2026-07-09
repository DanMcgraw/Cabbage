use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

/// Supported MMO skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillId {
    Mining,
    Combat,
}

impl SkillId {
    /// All skills in canonical order.
    pub const ALL: &[SkillId] = &[SkillId::Mining, SkillId::Combat];

    /// User-facing name.
    pub fn display_name(self) -> &'static str {
        match self {
            SkillId::Mining => "Mining",
            SkillId::Combat => "Combat",
        }
    }
}

impl Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}
