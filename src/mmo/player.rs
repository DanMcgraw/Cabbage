use super::{config::LevelCurve, skills::SkillId};

/// A point-in-time view of a player's skill progress.
///
/// Level is derived from total XP on demand, so this struct stores only the
/// authoritative cumulative XP value.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerSkillSnapshot {
    pub xp: u64,
}

impl PlayerSkillSnapshot {
    pub fn new(xp: u64) -> Self {
        Self { xp }
    }

    /// Derive the current level from total XP.
    pub fn level(&self, curve: &LevelCurve) -> u32 {
        curve.level_for_xp(self.xp).0
    }

    /// Format progress as "Level 5 (1,234 / 1,500 XP)".
    pub fn format_progress(&self, curve: &LevelCurve) -> String {
        let (_, into, needed) = curve.level_for_xp(self.xp);
        format!("Level {} ({} / {} XP)", self.level(curve), into, needed)
    }
}

/// All skill snapshots for a single player.
#[derive(Debug, Clone, Default)]
pub struct PlayerSnapshot {
    pub mining: PlayerSkillSnapshot,
    pub combat: PlayerSkillSnapshot,
}

impl PlayerSnapshot {
    #[allow(dead_code)]
    pub fn get(&self, skill: SkillId) -> &PlayerSkillSnapshot {
        match skill {
            SkillId::Mining => &self.mining,
            SkillId::Combat => &self.combat,
        }
    }

    pub fn set(&mut self, skill: SkillId, snapshot: PlayerSkillSnapshot) {
        match skill {
            SkillId::Mining => self.mining = snapshot,
            SkillId::Combat => self.combat = snapshot,
        }
    }
}
