use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pumpkin::{
    entity::player::Player,
    server::Server,
    world::bossbar::{Bossbar, BossbarColor, BossbarDivisions, BossbarFlags},
};
use pumpkin_util::text::{TextComponent, color::NamedColor};
use uuid::Uuid;

use super::super::skills::{BranchId, SkillId};

const BOSSBAR_DURATION_TICKS: i32 = 100; // 5 seconds at 20 TPS

#[derive(Debug, Clone)]
struct BossbarEntry {
    uuid: Uuid,
    expiry_tick: i32,
}

/// Tracks transient skill-progress bossbars per player.
///
/// Internally synchronized so it can be held directly by [`MmoState`] without an
/// additional async lock.
#[derive(Debug, Default)]
pub struct BossbarState {
    active: Mutex<HashMap<(Uuid, SkillId), BossbarEntry>>,
}

impl BossbarState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show or update a bossbar displaying the player's current skill progress.
    ///
    /// Arguments:
    /// - `player`: the player to show the bar to.
    /// - `skill`: the skill whose progress is being displayed.
    /// - `level`: the player's current level in that skill.
    /// - `xp_into_level`: XP earned towards the next level.
    /// - `xp_for_next`: total XP needed to reach the next level.
    /// - `current_tick`: current server tick, used to schedule automatic hide.
    pub async fn show_skill_progress(
        &self,
        player: &Arc<Player>,
        skill: SkillId,
        level: u32,
        xp_into_level: u64,
        xp_for_next: u64,
        current_tick: i32,
    ) {
        let key = (player.gameprofile.id, skill);
        let uuid = {
            let mut active = self.active.lock().unwrap();
            let entry = active.entry(key).or_insert_with(|| BossbarEntry {
                uuid: Uuid::new_v4(),
                expiry_tick: current_tick + BOSSBAR_DURATION_TICKS,
            });
            entry.expiry_tick = current_tick + BOSSBAR_DURATION_TICKS;
            entry.uuid
        };

        let health = if xp_for_next == 0 {
            1.0
        } else {
            (xp_into_level as f64 / xp_for_next as f64).clamp(0.0, 1.0) as f32
        };

        let title = skill_title(skill, level, xp_into_level, xp_for_next);
        let color = skill_color(skill);

        let bossbar = Bossbar {
            uuid,
            title,
            health,
            color,
            division: BossbarDivisions::NoDivision,
            flags: BossbarFlags::empty(),
        };

        player.send_bossbar(&bossbar).await;
    }

    /// Remove any bossbars whose display duration has expired.
    pub async fn cleanup_expired(&self, server: &Server, current_tick: i32) {
        let expired = {
            let mut active = self.active.lock().unwrap();
            let mut expired = Vec::new();
            active.retain(|(player_uuid, skill), entry| {
                if current_tick >= entry.expiry_tick {
                    expired.push((*player_uuid, *skill, entry.uuid));
                    false
                } else {
                    true
                }
            });
            expired
        };

        for (player_uuid, _skill, bar_uuid) in expired {
            if let Some(player) = server.get_player_by_uuid(player_uuid) {
                player.remove_bossbar(bar_uuid).await;
            }
        }
    }
}

fn skill_title(skill: SkillId, level: u32, into: u64, needed: u64) -> TextComponent {
    let name = skill.display_name();
    let color = match skill.branch() {
        BranchId::Frontier => NamedColor::Green,
        BranchId::Warfare => NamedColor::Red,
        BranchId::Enterprise => NamedColor::Gold,
    };

    TextComponent::text(format!("{name} Level {level}"))
        .color_named(color)
        .add_text(format!(" ({into}/{needed} XP)"))
}

fn skill_color(skill: SkillId) -> BossbarColor {
    match skill.branch() {
        BranchId::Frontier => BossbarColor::Green,
        BranchId::Warfare => BossbarColor::Red,
        BranchId::Enterprise => BossbarColor::Yellow,
    }
}
