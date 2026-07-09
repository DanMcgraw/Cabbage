use super::{config::LevelCurve, skills::SkillId};

/// A point-in-time view of a player's skill progress.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerSkillSnapshot {
    pub level: u32,
    pub xp: u64,
}

impl PlayerSkillSnapshot {
    pub fn new(level: u32, xp: u64) -> Self {
        Self { level, xp }
    }

    /// Format progress as "Level 5 (1,234 / 1,500 XP)".
    pub fn format_progress(&self, curve: &LevelCurve) -> String {
        let (_, into, needed) = curve.level_for_xp(self.xp);
        format!("Level {} ({} / {} XP)", self.level, into, needed)
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
