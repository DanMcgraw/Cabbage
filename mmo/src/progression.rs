//! Central progression flow.
//!
//! Every XP award in the MMO module flows through [`award_xp`]. That single
//! path enforces module/skill enabled checks, max-level behavior, configured
//! award bounds, level-up presentation, bossbar refresh, and audit logging,
//! so individual event handlers never duplicate progression logic.

use std::{collections::HashMap, fmt, sync::Arc};

use pumpkin::{
    entity::{EntityBase, player::Player},
    server::Server,
};
use pumpkin_data::{
    particle::Particle,
    sound::{Sound, SoundCategory},
};
use pumpkin_util::{
    math::vector3::Vector3,
    text::{TextComponent, color::NamedColor},
};

use super::{
    MmoState,
    config::LevelCurve,
    skills::{BranchId, SkillId},
};

/// Where an XP award originated. Used for audit logging and future rate
/// limiting; add a variant per new attribution rule, never reuse one.
///
/// Most variants are constructed by branch handlers landing in Phases 1-3.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XpSource {
    BlockBreak,
    EntityKill,
    Fishing,
    Harvest,
    Forage,
    Craft,
    Smelt,
    Brew,
    Repair,
    Salvage,
    Enchant,
    Tame,
    PetFeed,
    Breed,
    AnimalProduct,
    Melee,
    Cast,
    Projectile,
    DamageTaken,
    Fall,
    Consume,
    Admin,
    Migration,
}

impl fmt::Display for XpSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            XpSource::BlockBreak => "block_break",
            XpSource::EntityKill => "entity_kill",
            XpSource::Fishing => "fishing",
            XpSource::Harvest => "harvest",
            XpSource::Forage => "forage",
            XpSource::Craft => "craft",
            XpSource::Smelt => "smelt",
            XpSource::Brew => "brew",
            XpSource::Repair => "repair",
            XpSource::Salvage => "salvage",
            XpSource::Enchant => "enchant",
            XpSource::Tame => "tame",
            XpSource::PetFeed => "pet_feed",
            XpSource::Breed => "breed",
            XpSource::AnimalProduct => "animal_product",
            XpSource::Melee => "melee",
            XpSource::Cast => "cast",
            XpSource::Projectile => "projectile",
            XpSource::DamageTaken => "damage_taken",
            XpSource::Fall => "fall",
            XpSource::Consume => "consume",
            XpSource::Admin => "admin",
            XpSource::Migration => "migration",
        };
        f.write_str(name)
    }
}

/// What a completed XP award changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwardOutcome {
    /// XP actually granted after clamping.
    pub xp: u64,
    pub new_level: u32,
    pub leveled_up: bool,
}

/// Award XP to a player through the single central path.
///
/// Returns `None` when the award is skipped (module or skill disabled,
/// zero amount, or the player already sits at the skill's max level).
/// Configuration errors are logged and also yield `None`; event handlers
/// must not treat a skipped award as a failure.
pub async fn award_xp(
    state: &MmoState,
    player: &Arc<Player>,
    skill: SkillId,
    amount: u64,
    source: XpSource,
) -> Option<AwardOutcome> {
    if amount == 0 || !state.is_enabled() {
        return None;
    }

    let config = state.config();
    let skill_config = config.skills.get(&skill).cloned().unwrap_or_default();
    if !skill_config.enabled {
        return None;
    }

    let curve = state.curve(skill);
    let uuid = player.gameprofile.id;
    let db = state.db();

    let amount = amount.min(config.progression.max_xp_per_award);
    let result = match db.add_xp(uuid, skill, amount, curve).await {
        Ok(result) => result,
        Err(error) => {
            log::warn!(
                "[Cabbage MMO] failed to award {amount} {skill} XP to {uuid} ({source}): {error}"
            );
            return None;
        }
    };
    if result.awarded_xp == 0 {
        return None;
    }
    let amount = result.awarded_xp;

    if matches!(source, XpSource::Admin | XpSource::Migration) {
        log::info!("[Cabbage MMO] awarded {amount} {skill} XP to {uuid} via {source}");
    } else {
        log::debug!("[Cabbage MMO] awarded {amount} {skill} XP to {uuid} via {source}");
    }
    state.audit(&format!(
        "xp grant: {amount} {skill} XP to {uuid} via {source}"
    ));

    if result.leveled_up {
        if config.message_on_level_up {
            let message = TextComponent::text(format!("Your {skill} skill is now "))
                .add_child(
                    TextComponent::text(format!("Level {}", result.new_level))
                        .color_named(NamedColor::Green),
                )
                .add_text("!");
            player.send_system_message(&message).await;
        }
        celebrate_level_up(player, result.new_level).await;
    }

    let current_tick = state.current_tick();
    state
        .show_xp_bossbar_at_xp(player, skill, result.new_xp, current_tick)
        .await;

    Some(AwardOutcome {
        xp: amount,
        new_level: result.new_level,
        leveled_up: result.leveled_up,
    })
}

/// Fetch one immutable snapshot of a player's 18 skill rows.
///
/// This is the single snapshot loader shared by the skill menu, the stats
/// commands, and the chat fallback, so every presentation consumes the same
/// data. SQLite work stays on the database worker; only the returned
/// snapshot crosses onto the caller.
pub(crate) async fn fetch_snapshot(
    state: &MmoState,
    player_uuid: uuid::Uuid,
) -> Result<PlayerSnapshot, String> {
    let mut snapshot = PlayerSnapshot::default();
    for skill in SkillId::ALL {
        let progress = state.db().get_skill(player_uuid, *skill).await?;
        snapshot.set(*skill, PlayerSkillSnapshot::new(progress.xp));
    }
    Ok(snapshot)
}

/// Branch mastery: the average level of the branch's member skills.
///
/// Computed on demand; add caching only if profiling justifies it.
pub fn branch_mastery(branch: BranchId, level_of: impl Fn(SkillId) -> u32) -> f64 {
    let skills = branch.skills();
    let total: u32 = skills.iter().map(|skill| level_of(*skill)).sum();
    total as f64 / skills.len() as f64
}

/// Whether this player may earn progression XP. Creative and Spectator mode
/// players cannot farm XP or trigger perk effects.
pub(crate) fn earns_xp(player: &Player) -> bool {
    matches!(
        player.gamemode.load(),
        pumpkin_util::GameMode::Survival | pumpkin_util::GameMode::Adventure
    )
}

/// Read a player's current level for perk gating, falling back to level 1
/// when the database read fails so perks keep their level-1 behavior
/// (perk tier 0) instead of being skipped on a transient error.
pub(crate) async fn perk_level(state: &MmoState, player_uuid: uuid::Uuid, skill: SkillId) -> u32 {
    match state.db().get_skill(player_uuid, skill).await {
        Ok(progress) => state.curve(skill).level_for_xp(progress.xp).0,
        Err(error) => {
            log::warn!("[Cabbage MMO] failed to read {skill} level for perk gating: {error}");
            1
        }
    }
}

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
    skills: HashMap<SkillId, PlayerSkillSnapshot>,
}

impl PlayerSnapshot {
    pub fn get(&self, skill: SkillId) -> PlayerSkillSnapshot {
        self.skills.get(&skill).copied().unwrap_or_default()
    }

    pub fn set(&mut self, skill: SkillId, snapshot: PlayerSkillSnapshot) {
        self.skills.insert(skill, snapshot);
    }

    /// The player's level in a skill, derived from the configured curve.
    pub fn level_of(&self, skill: SkillId, curve: &LevelCurve) -> u32 {
        self.get(skill).level(curve)
    }
}

/// Play an anvil sound at the player, and celebrate a multiple-of-10 level
/// milestone with firework particles and sounds above them.
async fn celebrate_level_up(player: &Arc<Player>, new_level: u32) {
    let world = player.get_entity().world.load_full();
    let pos = player.get_entity().pos.load();

    world.play_sound(Sound::BlockAnvilUse, SoundCategory::Players, &pos);

    if new_level % 10 == 0 {
        // Use particles and sounds instead of spawning a firework rocket entity.
        // Pumpkin's firework entity currently has incomplete visuals/sounds and
        // its collision/sync can freeze or glitch the player.
        let firework_pos = pos + Vector3::new(0.0, 2.0, 0.0);
        world.play_sound(
            Sound::EntityFireworkRocketLaunch,
            SoundCategory::Players,
            &firework_pos,
        );
        world.spawn_particle(
            firework_pos,
            Vector3::new(0.3, 0.3, 0.3),
            0.1,
            30,
            Particle::Firework,
        );
        world.play_sound(
            Sound::EntityFireworkRocketTwinkle,
            SoundCategory::Players,
            &firework_pos,
        );
    }
}

/// Find an online player by UUID across all loaded worlds.
#[allow(dead_code)] // used by Warfare kill attribution (Phase 2)
pub(crate) fn find_player_by_uuid(server: &Server, uuid: uuid::Uuid) -> Option<Arc<Player>> {
    for world in server.worlds.load().iter() {
        if let Some(player) = world.get_player_by_uuid(uuid) {
            return Some(player);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SkillConfig;

    #[test]
    fn branch_mastery_averages_member_levels() {
        // Warfare has six member skills since the six-skill consolidation.
        let mastery = branch_mastery(BranchId::Warfare, |skill| match skill {
            SkillId::Blades => 60,
            SkillId::Axes => 0,
            SkillId::Archery => 0,
            SkillId::Athletics => 0,
            SkillId::Defense => 0,
            SkillId::Sorcery => 0,
            _ => unreachable!(),
        });
        assert!((mastery - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn branch_mastery_uses_six_skills_per_branch() {
        for branch in BranchId::ALL {
            assert_eq!(branch.skills().len(), 6, "{branch} must have six skills");
            let mastery = branch_mastery(*branch, |_| 6);
            assert!((mastery - 6.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn merged_activities_award_the_same_destination_skill() {
        // Each activity handler names its award skill in a `SKILL` constant;
        // both activities behind a merged skill must feed one shared track.
        assert_eq!(crate::frontier::agriculture::SKILL, SkillId::Cultivation);
        assert_eq!(crate::frontier::herbalism::SKILL, SkillId::Cultivation);
        assert_eq!(crate::frontier::husbandry::SKILL, SkillId::AnimalHandling);
        assert_eq!(crate::frontier::taming::SKILL, SkillId::AnimalHandling);
        assert_eq!(crate::enterprise::repair::SKILL, SkillId::Maintenance);
        assert_eq!(crate::enterprise::salvage::SKILL, SkillId::Maintenance);
        assert_eq!(crate::warfare::defense::FALL_SKILL, SkillId::Athletics);
    }

    #[test]
    fn merged_audit_sources_stay_activity_specific() {
        // Both activities in a pair award the same skill but keep distinct
        // audit sources; they describe *why* XP was awarded.
        assert_eq!(XpSource::Repair.to_string(), "repair");
        assert_eq!(XpSource::Salvage.to_string(), "salvage");
        assert_ne!(XpSource::Repair, XpSource::Salvage);
        assert_eq!(XpSource::Harvest.to_string(), "harvest");
        assert_eq!(XpSource::Forage.to_string(), "forage");
        assert_eq!(XpSource::Breed.to_string(), "breed");
        assert_eq!(XpSource::Tame.to_string(), "tame");
        assert_eq!(XpSource::Fall.to_string(), "fall");
    }

    #[test]
    fn snapshot_defaults_to_level_one_progress() {
        let snapshot = PlayerSnapshot::default();
        let curve = LevelCurve::new(&SkillConfig::default());
        assert_eq!(snapshot.level_of(SkillId::Mining, &curve), 1);
        assert_eq!(snapshot.get(SkillId::Mining).xp, 0);
    }
}
