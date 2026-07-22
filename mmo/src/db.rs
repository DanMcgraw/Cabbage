use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use futures::channel::oneshot;
use rusqlite::{Connection, params};
use uuid::Uuid;

use super::config::LevelCurve;
use super::ore_reveal::provenance::{ProvenanceChange, ProvenanceKey};
use super::skills::SkillId;

/// Request sent to the dedicated database worker thread.
enum DbRequest {
    GetSkill {
        player_uuid: Uuid,
        skill: SkillId,
        respond: oneshot::Sender<Result<PlayerSkill, String>>,
    },
    AddXp {
        player_uuid: Uuid,
        skill: SkillId,
        xp: u64,
        curve: LevelCurve,
        respond: oneshot::Sender<Result<XpResult, String>>,
    },
    SetXp {
        player_uuid: Uuid,
        skill: SkillId,
        xp: u64,
        curve: LevelCurve,
        respond: oneshot::Sender<Result<u32, String>>,
    },
    GetTop {
        skill: SkillId,
        limit: u32,
        curve: LevelCurve,
        respond: oneshot::Sender<Result<Vec<(String, u32, u64)>, String>>,
    },
    LoadLegacyXpRewards {
        respond: oneshot::Sender<Result<Option<LegacyXpRewards>, String>>,
    },
    DropLegacyXpRewardTables {
        respond: oneshot::Sender<Result<(), String>>,
    },
    LoadNonNaturalBlocks {
        respond: oneshot::Sender<Result<Vec<ProvenanceKey>, String>>,
    },
    ApplyProvenanceChanges {
        changes: Vec<ProvenanceChange>,
    },
    CombatMigrationStatus {
        respond: oneshot::Sender<Result<CombatMigrationStatus, String>>,
    },
    MigrateCombatXp {
        target: SkillId,
        respond: oneshot::Sender<Result<CombatMigrationOutcome, String>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSkill {
    pub xp: u64,
}

impl Default for PlayerSkill {
    fn default() -> Self {
        Self { xp: 0 }
    }
}

impl PlayerSkill {
    /// Derive the current level from cumulative XP using the configured curve.
    pub fn level(&self, curve: &LevelCurve) -> u32 {
        curve.level_for_xp(self.xp).0
    }
}

#[derive(Debug, Clone)]
pub struct XpResult {
    pub awarded_xp: u64,
    pub new_level: u32,
    pub new_xp: u64,
    pub leveled_up: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyXpRewards {
    pub mobs: HashMap<String, u64>,
    pub blocks: HashMap<String, u64>,
}

/// Storage key the retired two-branch Combat skill used in `player_skills`.
pub const LEGACY_COMBAT_SKILL_KEY: &str = "Combat";

/// Current SQLite schema version. Bump when adding migrations; each version's
/// migration runs exactly once, in order.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Retired `player_skills` keys merged by schema v2, as
/// `(anchor, secondary, destination)` storage keys. Both retired rows plus
/// any pre-existing destination row sum into the destination; the retired
/// rows are then deleted.
const RETIRED_SKILL_PAIRS: [(&str, &str, &str); 5] = [
    ("Agriculture", "Herbalism", "Cultivation"),
    ("Husbandry", "Taming", "AnimalHandling"),
    ("Unarmed", "Acrobatics", "Athletics"),
    ("Repair", "Salvage", "Maintenance"),
    ("Trading", "Charisma", "Commerce"),
];

/// Snapshot of the legacy Combat XP preservation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatMigrationStatus {
    pub schema_version: u32,
    pub players_with_legacy_xp: u64,
    pub total_legacy_xp: u64,
    pub migrated_to: Option<String>,
}

/// Result of moving legacy Combat XP into a destination skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatMigrationOutcome {
    pub players_migrated: u64,
    pub xp_moved: u64,
}

/// Handle to the dedicated database worker thread.
pub struct MmoDatabase {
    sender: Sender<DbRequest>,
    _worker: JoinHandle<()>,
}

impl MmoDatabase {
    /// Spawn a dedicated worker thread and open the SQLite database in it.
    ///
    /// Blocks until the worker reports that the database opened and every
    /// schema migration committed, so a failed migration surfaces here as a
    /// clear load error instead of a silently dead worker.
    pub fn open(data_folder: PathBuf) -> Result<Self, String> {
        let db_path = data_folder.join("mmo.db");
        let (sender, receiver) = mpsc::channel::<DbRequest>();
        let (ready_sender, ready_receiver) = mpsc::channel::<Result<(), String>>();

        let worker = thread::Builder::new()
            .name("cabbage-mmo-db".to_string())
            .spawn(move || {
                let mut conn = match Connection::open(&db_path) {
                    Ok(conn) => conn,
                    Err(e) => {
                        let message =
                            format!("failed to open database at {}: {e}", db_path.display());
                        log::error!("[Cabbage MMO] {message}");
                        let _ = ready_sender.send(Err(message));
                        return;
                    }
                };

                if let Err(e) = Self::migrate(&mut conn) {
                    let message = format!("failed to run migrations: {e}");
                    log::error!("[Cabbage MMO] {message}");
                    let _ = ready_sender.send(Err(message));
                    return;
                }
                if ready_sender.send(Ok(())).is_err() {
                    return;
                }
                while let Ok(request) = receiver.recv() {
                    match request {
                        DbRequest::GetSkill {
                            player_uuid,
                            skill,
                            respond,
                        } => {
                            let _ = respond.send(Self::do_get_skill(&conn, player_uuid, skill));
                        }
                        DbRequest::AddXp {
                            player_uuid,
                            skill,
                            xp,
                            curve,
                            respond,
                        } => {
                            let _ = respond.send(Self::do_add_xp(
                                &conn,
                                player_uuid,
                                skill,
                                xp,
                                &curve,
                            ));
                        }
                        DbRequest::SetXp {
                            player_uuid,
                            skill,
                            xp,
                            curve,
                            respond,
                        } => {
                            let _ = respond.send(Self::do_set_xp(
                                &conn,
                                player_uuid,
                                skill,
                                xp,
                                &curve,
                            ));
                        }
                        DbRequest::GetTop {
                            skill,
                            limit,
                            curve,
                            respond,
                        } => {
                            let _ = respond.send(Self::do_get_top(&conn, skill, limit, &curve));
                        }
                        DbRequest::LoadLegacyXpRewards { respond } => {
                            let _ = respond.send(Self::do_load_legacy_xp_rewards(&conn));
                        }
                        DbRequest::DropLegacyXpRewardTables { respond } => {
                            let _ = respond.send(Self::do_drop_legacy_xp_reward_tables(&mut conn));
                        }
                        DbRequest::LoadNonNaturalBlocks { respond } => {
                            let _ = respond.send(Self::do_load_non_natural_blocks(&conn));
                        }
                        DbRequest::ApplyProvenanceChanges { changes } => {
                            if let Err(error) =
                                Self::do_apply_provenance_changes(&mut conn, &changes)
                            {
                                log::error!(
                                    "[Cabbage MMO] failed to persist block provenance: {error}"
                                );
                            }
                        }
                        DbRequest::CombatMigrationStatus { respond } => {
                            let _ = respond.send(Self::do_combat_migration_status(&conn));
                        }
                        DbRequest::MigrateCombatXp { target, respond } => {
                            let _ = respond.send(Self::do_migrate_combat_xp(&mut conn, target));
                        }
                    }
                }
            })
            .map_err(|e| format!("failed to spawn mmo database worker: {e}"))?;

        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                sender,
                _worker: worker,
            }),
            Ok(Err(message)) => Err(message),
            Err(_) => Err("mmo database worker died during startup".to_string()),
        }
    }

    fn migrate(conn: &mut Connection) -> Result<(), String> {
        let statements = [
            r"CREATE TABLE IF NOT EXISTS player_skills (
                player_uuid TEXT NOT NULL,
                skill TEXT NOT NULL,
                xp INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (player_uuid, skill)
            );",
            r"CREATE TABLE IF NOT EXISTS non_natural_blocks (
                world_name TEXT NOT NULL,
                dimension_name TEXT NOT NULL,
                x INTEGER NOT NULL,
                y INTEGER NOT NULL,
                z INTEGER NOT NULL,
                PRIMARY KEY (world_name, dimension_name, x, y, z)
            );",
            r"CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
            r"CREATE TABLE IF NOT EXISTS legacy_combat_xp (
                player_uuid TEXT PRIMARY KEY,
                xp INTEGER NOT NULL DEFAULT 0
            );",
        ];

        for sql in statements {
            conn.execute(sql, [])
                .map_err(|e| format!("failed to run mmo migration: {e}"))?;
        }

        // Best-effort removal of the legacy level column. Fresh databases will
        // not have it; existing databases may still carry it from older builds.
        let _ = conn.execute("ALTER TABLE player_skills DROP COLUMN level", []);

        // Schema v1: retire the Combat skill. Preserve any stored Combat XP in
        // the legacy_combat_xp record; administrators choose its destination
        // later via config or `/mmo migrate combat <skill>`. Idempotent: the
        // version row guards re-entry even if the server restarts mid-upgrade.
        let schema_version = Self::schema_version(conn)?;
        if schema_version < 1 {
            let transaction = conn
                .transaction()
                .map_err(|e| format!("failed to start combat retirement migration: {e}"))?;
            transaction
                .execute(
                    "INSERT INTO legacy_combat_xp (player_uuid, xp)
                     SELECT player_uuid, xp FROM player_skills WHERE skill = ?1
                     ON CONFLICT(player_uuid) DO UPDATE SET xp = excluded.xp",
                    params![LEGACY_COMBAT_SKILL_KEY],
                )
                .map_err(|e| format!("failed to preserve legacy combat xp: {e}"))?;
            transaction
                .execute(
                    "DELETE FROM player_skills WHERE skill = ?1",
                    params![LEGACY_COMBAT_SKILL_KEY],
                )
                .map_err(|e| format!("failed to remove legacy combat rows: {e}"))?;
            transaction
                .execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', '1')",
                    [],
                )
                .map_err(|e| format!("failed to record schema version: {e}"))?;
            transaction
                .commit()
                .map_err(|e| format!("failed to commit combat retirement migration: {e}"))?;
            log::info!("[Cabbage MMO] retired Combat skill; XP preserved in legacy record");
        }

        // Schema v2: merge the five retired skill pairs into their shared
        // destination tracks. Runs after the v1 migration so v0 and v1
        // databases follow the same deterministic upgrade path.
        if Self::schema_version(conn)? < CURRENT_SCHEMA_VERSION {
            Self::migrate_skill_consolidation(conn)?;
        }

        Ok(())
    }

    /// Schema v2: consolidate the retired skill pairs of the six-skill branch
    /// consolidation into their canonical destination rows.
    ///
    /// For every player and every pair, the cumulative XP of both retired
    /// keys plus any pre-existing destination row is summed and written to
    /// the destination row, then the retired rows are deleted. Raw XP is
    /// summed (never the larger level, never an average) and is not capped
    /// to a level curve; normal level calculation interprets the total on
    /// read. A destination row is only written when the sum is positive or a
    /// destination row already exists, so pairs of zero-XP retired rows do
    /// not fabricate new rows. Unrelated skill rows are never touched.
    ///
    /// Saturation policy: stored XP values are non-negative (negative
    /// anomalies clamp to 0, matching every read path). The three values are
    /// summed with checked i64 addition; on overflow the total saturates at
    /// `i64::MAX` and a warning is logged rather than failing the migration
    /// — an error would strand the whole database over an unreachable value.
    ///
    /// The whole consolidation runs in one transaction: any failure rolls
    /// every player back and is returned as a load error. Guarded by the
    /// schema version row, so reopening a migrated database is a no-op.
    fn migrate_skill_consolidation(conn: &mut Connection) -> Result<(), String> {
        let transaction = conn
            .transaction()
            .map_err(|e| format!("failed to start skill consolidation migration: {e}"))?;

        let rows: Vec<(String, String, i64)> = {
            let mut stmt = transaction
                .prepare(
                    "SELECT player_uuid, skill, xp FROM player_skills
                     WHERE skill IN (
                         'Agriculture', 'Herbalism', 'Husbandry', 'Taming',
                         'Unarmed', 'Acrobatics', 'Repair', 'Salvage',
                         'Trading', 'Charisma', 'Cultivation', 'AnimalHandling',
                         'Athletics', 'Maintenance', 'Commerce'
                     )",
                )
                .map_err(|e| format!("failed to prepare skill consolidation query: {e}"))?;
            let mapped = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(|e| format!("failed to read skill consolidation rows: {e}"))?;
            mapped
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read skill consolidation row: {e}"))?
        };

        let mut by_player: HashMap<String, HashMap<String, i64>> = HashMap::new();
        for (player_uuid, skill, xp) in rows {
            let xp = xp.max(0);
            by_player.entry(player_uuid).or_default().insert(skill, xp);
        }

        let mut players_consolidated = 0u64;
        let mut retired_rows_removed = 0u64;
        for (player_uuid, skills) in &by_player {
            let mut player_had_retired_rows = false;
            for (anchor, secondary, destination) in RETIRED_SKILL_PAIRS {
                let anchor_xp = skills.get(anchor).copied();
                let secondary_xp = skills.get(secondary).copied();
                if anchor_xp.is_none() && secondary_xp.is_none() {
                    continue;
                }
                player_had_retired_rows = true;
                retired_rows_removed +=
                    u64::from(anchor_xp.is_some()) + u64::from(secondary_xp.is_some());

                let destination_xp = skills.get(destination).copied();
                let mut sum = 0i64;
                let mut saturated = false;
                for value in [anchor_xp, secondary_xp, destination_xp]
                    .into_iter()
                    .flatten()
                {
                    match sum.checked_add(value) {
                        Some(total) => sum = total,
                        None => {
                            sum = i64::MAX;
                            saturated = true;
                        }
                    }
                }
                if saturated {
                    log::warn!(
                        "[Cabbage MMO] skill consolidation for player {player_uuid} exceeded \
                         the SQLite 64-bit XP range; saturated {destination} at i64::MAX"
                    );
                }

                if sum > 0 || destination_xp.is_some() {
                    transaction
                        .execute(
                            "INSERT INTO player_skills (player_uuid, skill, xp)
                             VALUES (?1, ?2, ?3)
                             ON CONFLICT(player_uuid, skill)
                             DO UPDATE SET xp = excluded.xp",
                            params![player_uuid, destination, sum],
                        )
                        .map_err(|e| {
                            format!("failed to write consolidated {destination} row: {e}")
                        })?;
                }
                transaction
                    .execute(
                        "DELETE FROM player_skills
                         WHERE player_uuid = ?1 AND skill IN (?2, ?3)",
                        params![player_uuid, anchor, secondary],
                    )
                    .map_err(|e| {
                        format!("failed to remove retired {anchor}/{secondary} rows: {e}")
                    })?;
            }
            if player_had_retired_rows {
                players_consolidated += 1;
            }
        }

        transaction
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', '2')",
                [],
            )
            .map_err(|e| format!("failed to record schema version: {e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("failed to commit skill consolidation migration: {e}"))?;
        log::info!(
            "[Cabbage MMO] schema v2: consolidated {retired_rows_removed} retired skill row(s) \
             across {players_consolidated} player(s) into the five merged skills"
        );
        Ok(())
    }

    fn schema_version(conn: &Connection) -> Result<u32, String> {
        let version: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(version.and_then(|v| v.parse::<u32>().ok()).unwrap_or(0))
    }

    fn do_combat_migration_status(conn: &Connection) -> Result<CombatMigrationStatus, String> {
        let (players, total_xp): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(xp), 0) FROM legacy_combat_xp",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("failed to read legacy combat record: {e}"))?;
        let migrated_to: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'combat_migrated_to'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(CombatMigrationStatus {
            schema_version: Self::schema_version(conn)?,
            players_with_legacy_xp: players.max(0) as u64,
            total_legacy_xp: total_xp.max(0) as u64,
            migrated_to,
        })
    }

    fn do_migrate_combat_xp(
        conn: &mut Connection,
        target: SkillId,
    ) -> Result<CombatMigrationOutcome, String> {
        let transaction = conn
            .transaction()
            .map_err(|e| format!("failed to start combat migration: {e}"))?;

        let rows: Vec<(String, i64)> = {
            let mut stmt = transaction
                .prepare("SELECT player_uuid, xp FROM legacy_combat_xp")
                .map_err(|e| format!("failed to prepare legacy combat query: {e}"))?;
            let mapped = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| format!("failed to read legacy combat rows: {e}"))?;
            mapped
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read legacy combat row: {e}"))?
        };

        let mut xp_moved = 0i64;
        for (player_uuid, xp) in &rows {
            transaction
                .execute(
                    "INSERT INTO player_skills (player_uuid, skill, xp)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(player_uuid, skill)
                     DO UPDATE SET xp = xp + excluded.xp",
                    params![player_uuid, target.as_str(), xp],
                )
                .map_err(|e| format!("failed to move combat xp: {e}"))?;
            xp_moved += xp;
        }

        transaction
            .execute("DELETE FROM legacy_combat_xp", [])
            .map_err(|e| format!("failed to clear legacy combat record: {e}"))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('combat_migrated_to', ?1)",
                params![target.as_str()],
            )
            .map_err(|e| format!("failed to record combat migration: {e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("failed to commit combat migration: {e}"))?;

        Ok(CombatMigrationOutcome {
            players_migrated: rows.len() as u64,
            xp_moved: xp_moved.max(0) as u64,
        })
    }

    fn do_get_skill(
        conn: &Connection,
        player_uuid: Uuid,
        skill: SkillId,
    ) -> Result<PlayerSkill, String> {
        let mut stmt = conn
            .prepare("SELECT xp FROM player_skills WHERE player_uuid = ?1 AND skill = ?2")
            .map_err(|e| format!("failed to prepare player skill query: {e}"))?;

        let result = stmt
            .query_row(params![player_uuid.to_string(), skill.to_string()], |row| {
                let xp: i64 = row.get(0).unwrap_or(0);
                Ok(PlayerSkill {
                    xp: xp.max(0) as u64,
                })
            })
            .unwrap_or_else(|_| PlayerSkill::default());

        Ok(result)
    }

    fn do_add_xp(
        conn: &Connection,
        player_uuid: Uuid,
        skill: SkillId,
        xp: u64,
        curve: &LevelCurve,
    ) -> Result<XpResult, String> {
        let current = Self::do_get_skill(conn, player_uuid, skill)?;
        let max_xp = curve.xp_for_level(curve.max_level());
        let awarded_xp = xp.min(max_xp.saturating_sub(current.xp));
        let new_xp = current.xp.saturating_add(awarded_xp);
        let current_level = current.level(curve);
        let new_level = curve.level_for_xp(new_xp).0;
        let leveled_up = new_level > current_level;

        if awarded_xp > 0 {
            conn.execute(
                "INSERT INTO player_skills (player_uuid, skill, xp)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(player_uuid, skill)
                 DO UPDATE SET xp = excluded.xp",
                params![player_uuid.to_string(), skill.to_string(), new_xp as i64,],
            )
            .map_err(|e| format!("failed to update player xp: {e}"))?;
        }

        Ok(XpResult {
            awarded_xp,
            new_level,
            new_xp,
            leveled_up,
        })
    }

    fn do_set_xp(
        conn: &Connection,
        player_uuid: Uuid,
        skill: SkillId,
        xp: u64,
        curve: &LevelCurve,
    ) -> Result<u32, String> {
        let new_level = curve.level_for_xp(xp).0;

        conn.execute(
            "INSERT INTO player_skills (player_uuid, skill, xp)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(player_uuid, skill)
             DO UPDATE SET xp = excluded.xp",
            params![player_uuid.to_string(), skill.to_string(), xp as i64,],
        )
        .map_err(|e| format!("failed to set player xp: {e}"))?;

        Ok(new_level)
    }

    fn do_get_top(
        conn: &Connection,
        skill: SkillId,
        limit: u32,
        curve: &LevelCurve,
    ) -> Result<Vec<(String, u32, u64)>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT player_uuid, xp FROM player_skills
                 WHERE skill = ?1
                 ORDER BY xp DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("failed to prepare top players query: {e}"))?;

        let rows = stmt
            .query_map(params![skill.to_string(), limit as i64], |row| {
                let uuid: String = row.get(0).unwrap_or_default();
                let xp: i64 = row.get(1).unwrap_or(0);
                let xp = xp.max(0) as u64;
                let level = curve.level_for_xp(xp).0;
                Ok((uuid, level, xp))
            })
            .map_err(|e| format!("failed to query top players: {e}"))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("failed to read top player row: {e}"))?);
        }
        Ok(results)
    }

    fn do_load_legacy_xp_rewards(conn: &Connection) -> Result<Option<LegacyXpRewards>, String> {
        let mob_exists = Self::table_exists(conn, "mob_xp")?;
        let block_exists = Self::table_exists(conn, "ore_xp")?;
        if !mob_exists && !block_exists {
            return Ok(None);
        }

        let mobs = if mob_exists {
            Self::read_legacy_rewards(conn, "SELECT mob_resource_name, xp FROM mob_xp")?
        } else {
            HashMap::new()
        };
        let blocks = if block_exists {
            Self::read_legacy_rewards(conn, "SELECT block_name, xp FROM ore_xp")?
        } else {
            HashMap::new()
        };
        Ok(Some(LegacyXpRewards { mobs, blocks }))
    }

    fn do_drop_legacy_xp_reward_tables(conn: &mut Connection) -> Result<(), String> {
        let transaction = conn
            .transaction()
            .map_err(|error| format!("failed to start XP reward cleanup: {error}"))?;
        transaction
            .execute_batch("DROP TABLE IF EXISTS mob_xp; DROP TABLE IF EXISTS ore_xp;")
            .map_err(|error| format!("failed to remove legacy XP reward tables: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit XP reward cleanup: {error}"))
    }

    fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                params![table],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to inspect legacy XP tables: {error}"))?;
        Ok(count > 0)
    }

    fn read_legacy_rewards(conn: &Connection, sql: &str) -> Result<HashMap<String, u64>, String> {
        let mut statement = conn
            .prepare(sql)
            .map_err(|error| format!("failed to prepare legacy XP query: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let xp: i64 = row.get(1)?;
                Ok((name, xp.max(0) as u64))
            })
            .map_err(|error| format!("failed to query legacy XP rewards: {error}"))?;
        rows.collect::<Result<HashMap<_, _>, _>>()
            .map_err(|error| format!("failed to read legacy XP reward: {error}"))
    }

    fn do_load_non_natural_blocks(conn: &Connection) -> Result<Vec<ProvenanceKey>, String> {
        let mut statement = conn
            .prepare("SELECT world_name, dimension_name, x, y, z FROM non_natural_blocks")
            .map_err(|error| format!("failed to prepare block provenance query: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(ProvenanceKey {
                    world: row.get(0)?,
                    dimension: row.get(1)?,
                    x: row.get(2)?,
                    y: row.get(3)?,
                    z: row.get(4)?,
                })
            })
            .map_err(|error| format!("failed to query block provenance: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read block provenance row: {error}"))
    }

    fn do_apply_provenance_changes(
        conn: &mut Connection,
        changes: &[ProvenanceChange],
    ) -> Result<(), String> {
        let transaction = conn
            .transaction()
            .map_err(|error| format!("failed to start block provenance transaction: {error}"))?;
        for change in changes {
            let key = &change.key;
            if change.non_natural {
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO non_natural_blocks
                         (world_name, dimension_name, x, y, z) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![key.world, key.dimension, key.x, key.y, key.z],
                    )
                    .map_err(|error| format!("failed to insert block provenance: {error}"))?;
            } else {
                transaction
                    .execute(
                        "DELETE FROM non_natural_blocks
                         WHERE world_name = ?1 AND dimension_name = ?2
                           AND x = ?3 AND y = ?4 AND z = ?5",
                        params![key.world, key.dimension, key.x, key.y, key.z],
                    )
                    .map_err(|error| format!("failed to delete block provenance: {error}"))?;
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit block provenance: {error}"))
    }

    /// Public async API: send a request and await the worker's response.
    pub async fn get_skill(
        &self,
        player_uuid: Uuid,
        skill: SkillId,
    ) -> Result<PlayerSkill, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::GetSkill {
                player_uuid,
                skill,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn add_xp(
        &self,
        player_uuid: Uuid,
        skill: SkillId,
        xp: u64,
        curve: LevelCurve,
    ) -> Result<XpResult, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::AddXp {
                player_uuid,
                skill,
                xp,
                curve,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn set_xp(
        &self,
        player_uuid: Uuid,
        skill: SkillId,
        xp: u64,
        curve: LevelCurve,
    ) -> Result<u32, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::SetXp {
                player_uuid,
                skill,
                xp,
                curve,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn get_top(
        &self,
        skill: SkillId,
        limit: u32,
        curve: LevelCurve,
    ) -> Result<Vec<(String, u32, u64)>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::GetTop {
                skill,
                limit,
                curve,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn load_legacy_xp_rewards(&self) -> Result<Option<LegacyXpRewards>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::LoadLegacyXpRewards { respond: tx })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn drop_legacy_xp_reward_tables(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::DropLegacyXpRewardTables { respond: tx })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn load_non_natural_blocks(&self) -> Result<Vec<ProvenanceKey>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::LoadNonNaturalBlocks { respond: tx })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub fn apply_provenance_changes(&self, changes: Vec<ProvenanceChange>) -> Result<(), String> {
        if changes.is_empty() {
            return Ok(());
        }
        self.sender
            .send(DbRequest::ApplyProvenanceChanges { changes })
            .map_err(|_| "mmo database worker has shut down".to_string())
    }

    /// Snapshot of the legacy Combat XP preservation record.
    pub async fn combat_migration_status(&self) -> Result<CombatMigrationStatus, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::CombatMigrationStatus { respond: tx })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    /// One-time, idempotent migration of preserved legacy Combat XP into the
    /// given destination skill.
    pub async fn migrate_combat_xp(
        &self,
        target: SkillId,
    ) -> Result<CombatMigrationOutcome, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::MigrateCombatXp {
                target,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db_folder() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("cabbage_mmo_test_{}", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&path);
        path
    }

    fn open_test_db() -> (MmoDatabase, PathBuf) {
        let folder = test_db_folder();
        let db = MmoDatabase::open(folder.clone()).unwrap();
        (db, folder)
    }

    fn cleanup(folder: &PathBuf) {
        let db_path = folder.join("mmo.db");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(folder);
    }

    #[tokio::test]
    async fn round_trip_player_skill() {
        let (db, folder) = open_test_db();
        let uuid = Uuid::new_v4();
        let curve = LevelCurve::new(&super::super::config::SkillConfig {
            max_level: 99,
            base_xp: 50,
            xp_multiplier: 1.15,
            enabled: true,
        });

        let result = db
            .add_xp(uuid, SkillId::Mining, 75, curve.clone())
            .await
            .unwrap();
        assert_eq!(result.new_xp, 75);
        assert_eq!(result.awarded_xp, 75);
        assert!(result.leveled_up);

        let skill = db.get_skill(uuid, SkillId::Mining).await.unwrap();
        assert_eq!(skill.xp, 75);
        assert_eq!(skill.level(&curve), 2);

        cleanup(&folder);
    }

    #[tokio::test]
    async fn xp_awards_clamp_atomically_at_max_level() {
        let (db, folder) = open_test_db();
        let uuid = Uuid::new_v4();
        let curve = LevelCurve::new(&super::super::config::SkillConfig {
            max_level: 5,
            base_xp: 100,
            xp_multiplier: 2.0,
            enabled: true,
        });
        let max_xp = curve.xp_for_level(curve.max_level());

        db.set_xp(uuid, SkillId::Mining, max_xp - 5, curve.clone())
            .await
            .unwrap();
        let first = db
            .add_xp(uuid, SkillId::Mining, 100, curve.clone())
            .await
            .unwrap();
        let second = db.add_xp(uuid, SkillId::Mining, 100, curve).await.unwrap();

        assert_eq!(first.awarded_xp, 5);
        assert_eq!(first.new_xp, max_xp);
        assert_eq!(second.awarded_xp, 0);
        assert_eq!(second.new_xp, max_xp);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn fresh_database_has_no_legacy_xp_tables() {
        let (db, folder) = open_test_db();
        assert!(db.load_legacy_xp_rewards().await.unwrap().is_none());
        cleanup(&folder);
    }

    #[tokio::test]
    async fn legacy_xp_tables_are_read_and_removed() {
        let folder = test_db_folder();
        let conn = Connection::open(folder.join("mmo.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE mob_xp (mob_resource_name TEXT PRIMARY KEY, xp INTEGER NOT NULL);
             CREATE TABLE ore_xp (block_name TEXT PRIMARY KEY, xp INTEGER NOT NULL);
             INSERT INTO mob_xp VALUES ('zombie', 99);
             INSERT INTO ore_xp VALUES ('diamond_ore', 123);",
        )
        .unwrap();
        drop(conn);

        let db = MmoDatabase::open(folder.clone()).unwrap();
        let rewards = db.load_legacy_xp_rewards().await.unwrap().unwrap();
        assert_eq!(rewards.mobs.get("zombie"), Some(&99));
        assert_eq!(rewards.blocks.get("diamond_ore"), Some(&123));
        assert!(db.load_legacy_xp_rewards().await.unwrap().is_some());
        db.drop_legacy_xp_reward_tables().await.unwrap();
        assert!(db.load_legacy_xp_rewards().await.unwrap().is_none());
        cleanup(&folder);
    }

    #[tokio::test]
    async fn block_provenance_round_trips() {
        let (db, folder) = open_test_db();
        let key = ProvenanceKey {
            world: "world".to_string(),
            dimension: "minecraft:overworld".to_string(),
            x: 4,
            y: 12,
            z: -9,
        };
        db.apply_provenance_changes(vec![ProvenanceChange {
            key: key.clone(),
            non_natural: true,
        }])
        .unwrap();
        assert_eq!(
            db.load_non_natural_blocks().await.unwrap(),
            vec![key.clone()]
        );

        db.apply_provenance_changes(vec![ProvenanceChange {
            key,
            non_natural: false,
        }])
        .unwrap();
        assert!(db.load_non_natural_blocks().await.unwrap().is_empty());
        cleanup(&folder);
    }

    fn seed_legacy_two_skill_db(folder: &PathBuf) -> Uuid {
        let conn = Connection::open(folder.join("mmo.db")).unwrap();
        let uuid = Uuid::new_v4();
        conn.execute_batch(
            "CREATE TABLE player_skills (
                player_uuid TEXT NOT NULL,
                skill TEXT NOT NULL,
                xp INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (player_uuid, skill)
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO player_skills (player_uuid, skill, xp) VALUES (?1, 'Mining', 500)",
            params![uuid.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO player_skills (player_uuid, skill, xp) VALUES (?1, 'Combat', 750)",
            params![uuid.to_string()],
        )
        .unwrap();
        drop(conn);
        uuid
    }

    #[tokio::test]
    async fn combat_xp_is_preserved_in_legacy_record() {
        let folder = test_db_folder();
        let uuid = seed_legacy_two_skill_db(&folder);

        let db = MmoDatabase::open(folder.clone()).unwrap();
        let status = db.combat_migration_status().await.unwrap();
        assert_eq!(status.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(status.players_with_legacy_xp, 1);
        assert_eq!(status.total_legacy_xp, 750);
        assert_eq!(status.migrated_to, None);

        // Mining rows are untouched; Combat rows left player_skills.
        let mining = db.get_skill(uuid, SkillId::Mining).await.unwrap();
        assert_eq!(mining.xp, 500);

        // Re-opening must not re-run or duplicate the migration.
        drop(db);
        let db = MmoDatabase::open(folder.clone()).unwrap();
        let status = db.combat_migration_status().await.unwrap();
        assert_eq!(status.players_with_legacy_xp, 1);
        assert_eq!(status.total_legacy_xp, 750);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn combat_migration_moves_xp_once() {
        let folder = test_db_folder();
        let uuid = seed_legacy_two_skill_db(&folder);

        let db = MmoDatabase::open(folder.clone()).unwrap();
        let outcome = db.migrate_combat_xp(SkillId::Blades).await.unwrap();
        assert_eq!(outcome.players_migrated, 1);
        assert_eq!(outcome.xp_moved, 750);

        let blades = db.get_skill(uuid, SkillId::Blades).await.unwrap();
        assert_eq!(blades.xp, 750);

        let status = db.combat_migration_status().await.unwrap();
        assert_eq!(status.players_with_legacy_xp, 0);
        assert_eq!(status.migrated_to.as_deref(), Some("Blades"));

        // A second run is a no-op.
        let outcome = db.migrate_combat_xp(SkillId::Blades).await.unwrap();
        assert_eq!(outcome.players_migrated, 0);
        assert_eq!(outcome.xp_moved, 0);
        let blades = db.get_skill(uuid, SkillId::Blades).await.unwrap();
        assert_eq!(blades.xp, 750);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn combat_migration_adds_to_existing_destination_xp() {
        let folder = test_db_folder();
        let uuid = seed_legacy_two_skill_db(&folder);

        let db = MmoDatabase::open(folder.clone()).unwrap();
        let curve = LevelCurve::new(&super::super::config::SkillConfig::default());
        db.add_xp(uuid, SkillId::Blades, 50, curve).await.unwrap();

        let outcome = db.migrate_combat_xp(SkillId::Blades).await.unwrap();
        assert_eq!(outcome.xp_moved, 750);
        let blades = db.get_skill(uuid, SkillId::Blades).await.unwrap();
        assert_eq!(blades.xp, 800);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn fresh_database_reports_current_schema_version() {
        let (db, folder) = open_test_db();
        let status = db.combat_migration_status().await.unwrap();
        assert_eq!(status.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(status.players_with_legacy_xp, 0);
        cleanup(&folder);
    }

    /// Write a schema-v1-shaped database (post-Combat-retirement) with the
    /// given `player_skills` rows, bypassing the worker so retired skill keys
    /// can exist on disk.
    fn seed_v1_db(folder: &PathBuf, rows: &[(&str, &str, i64)]) {
        let conn = Connection::open(folder.join("mmo.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE player_skills (
                player_uuid TEXT NOT NULL,
                skill TEXT NOT NULL,
                xp INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (player_uuid, skill)
            );
            CREATE TABLE non_natural_blocks (
                world_name TEXT NOT NULL,
                dimension_name TEXT NOT NULL,
                x INTEGER NOT NULL,
                y INTEGER NOT NULL,
                z INTEGER NOT NULL,
                PRIMARY KEY (world_name, dimension_name, x, y, z)
            );
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE legacy_combat_xp (
                player_uuid TEXT PRIMARY KEY,
                xp INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO meta (key, value) VALUES ('schema_version', '1');",
        )
        .unwrap();
        for (player_uuid, skill, xp) in rows {
            conn.execute(
                "INSERT INTO player_skills (player_uuid, skill, xp) VALUES (?1, ?2, ?3)",
                params![player_uuid, skill, xp],
            )
            .unwrap();
        }
    }

    /// Read one raw XP value straight from SQLite, bypassing the worker, so
    /// retired keys (no longer representable as `SkillId`) can be checked.
    fn raw_xp(folder: &PathBuf, player_uuid: &str, skill: &str) -> Option<i64> {
        let conn = Connection::open(folder.join("mmo.db")).unwrap();
        conn.query_row(
            "SELECT xp FROM player_skills WHERE player_uuid = ?1 AND skill = ?2",
            params![player_uuid, skill],
            |row| row.get(0),
        )
        .ok()
    }

    fn raw_schema_version(folder: &PathBuf) -> String {
        let conn = Connection::open(folder.join("mmo.db")).unwrap();
        conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn consolidation_sums_both_retired_rows() {
        let folder = test_db_folder();
        let uuid = Uuid::new_v4();
        let uuid_str = uuid.to_string();
        seed_v1_db(
            &folder,
            &[
                (&uuid_str, "Agriculture", 100),
                (&uuid_str, "Herbalism", 50),
                (&uuid_str, "Mining", 500),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        let cultivation = db.get_skill(uuid, SkillId::Cultivation).await.unwrap();
        assert_eq!(cultivation.xp, 150);
        drop(db);

        assert_eq!(raw_xp(&folder, &uuid_str, "Agriculture"), None);
        assert_eq!(raw_xp(&folder, &uuid_str, "Herbalism"), None);
        assert_eq!(raw_xp(&folder, &uuid_str, "Cultivation"), Some(150));
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_migrates_anchor_only_rows() {
        let folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(&folder, &[(&uuid_str, "Husbandry", 200)]);

        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_xp(&folder, &uuid_str, "AnimalHandling"), Some(200));
        assert_eq!(raw_xp(&folder, &uuid_str, "Husbandry"), None);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_migrates_secondary_only_rows() {
        let folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(&folder, &[(&uuid_str, "Taming", 75)]);

        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_xp(&folder, &uuid_str, "AnimalHandling"), Some(75));
        assert_eq!(raw_xp(&folder, &uuid_str, "Taming"), None);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_adds_pre_existing_destination_xp_to_the_sum() {
        let folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(
            &folder,
            &[
                (&uuid_str, "Repair", 100),
                (&uuid_str, "Salvage", 40),
                (&uuid_str, "Maintenance", 25),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_xp(&folder, &uuid_str, "Maintenance"), Some(165));
        assert_eq!(raw_xp(&folder, &uuid_str, "Repair"), None);
        assert_eq!(raw_xp(&folder, &uuid_str, "Salvage"), None);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_runs_independently_per_player() {
        let folder = test_db_folder();
        let alice = Uuid::new_v4().to_string();
        let bob = Uuid::new_v4().to_string();
        seed_v1_db(
            &folder,
            &[
                (&alice, "Unarmed", 10),
                (&alice, "Acrobatics", 5),
                (&bob, "Unarmed", 70),
                (&bob, "Defense", 9),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_xp(&folder, &alice, "Athletics"), Some(15));
        assert_eq!(raw_xp(&folder, &bob, "Athletics"), Some(70));
        assert_eq!(raw_xp(&folder, &alice, "Unarmed"), None);
        assert_eq!(raw_xp(&folder, &bob, "Acrobatics"), None);
        assert_eq!(raw_xp(&folder, &bob, "Defense"), Some(9));
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_zero_xp_rows_do_not_fabricate_totals() {
        let folder = test_db_folder();
        let zero_pair = Uuid::new_v4().to_string();
        let mixed = Uuid::new_v4().to_string();
        let zero_destination = Uuid::new_v4().to_string();
        seed_v1_db(
            &folder,
            &[
                // Both retired rows at 0 XP, no destination row: nothing to write.
                (&zero_pair, "Agriculture", 0),
                (&zero_pair, "Herbalism", 0),
                // A zero-XP anchor must not drag down the secondary's total.
                (&mixed, "Unarmed", 0),
                (&mixed, "Acrobatics", 30),
                // A pre-existing (zero) destination row is kept at its sum.
                (&zero_destination, "Trading", 0),
                (&zero_destination, "Charisma", 0),
                (&zero_destination, "Commerce", 0),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_xp(&folder, &zero_pair, "Cultivation"), None);
        assert_eq!(raw_xp(&folder, &zero_pair, "Agriculture"), None);
        assert_eq!(raw_xp(&folder, &zero_pair, "Herbalism"), None);
        assert_eq!(raw_xp(&folder, &mixed, "Athletics"), Some(30));
        assert_eq!(raw_xp(&folder, &zero_destination, "Commerce"), Some(0));
        assert_eq!(raw_xp(&folder, &zero_destination, "Trading"), None);
        assert_eq!(raw_xp(&folder, &zero_destination, "Charisma"), None);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_leaves_unrelated_rows_untouched() {
        let folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(
            &folder,
            &[
                (&uuid_str, "Agriculture", 10),
                (&uuid_str, "Mining", 500),
                (&uuid_str, "Blades", 60),
                (&uuid_str, "SomeFutureSkill", 7),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_xp(&folder, &uuid_str, "Mining"), Some(500));
        assert_eq!(raw_xp(&folder, &uuid_str, "Blades"), Some(60));
        assert_eq!(raw_xp(&folder, &uuid_str, "SomeFutureSkill"), Some(7));
        assert_eq!(raw_xp(&folder, &uuid_str, "Cultivation"), Some(10));
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_is_idempotent_on_reopen() {
        let folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(
            &folder,
            &[
                (&uuid_str, "Agriculture", 100),
                (&uuid_str, "Herbalism", 50),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        assert_eq!(raw_schema_version(&folder), "2");
        assert_eq!(raw_xp(&folder, &uuid_str, "Cultivation"), Some(150));
        drop(db);

        // Reopening must not re-run or re-sum the migration.
        let db = MmoDatabase::open(folder.clone()).unwrap();
        drop(db);
        assert_eq!(raw_schema_version(&folder), "2");
        assert_eq!(raw_xp(&folder, &uuid_str, "Cultivation"), Some(150));
        assert_eq!(raw_xp(&folder, &uuid_str, "Agriculture"), None);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn v0_and_v1_databases_both_reach_schema_v2() {
        // v0: no meta table at all; both migrations run in order.
        let v0_folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        let conn = Connection::open(v0_folder.join("mmo.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE player_skills (
                player_uuid TEXT NOT NULL,
                skill TEXT NOT NULL,
                xp INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (player_uuid, skill)
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO player_skills (player_uuid, skill, xp) VALUES (?1, 'Combat', 750)",
            params![uuid_str],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO player_skills (player_uuid, skill, xp) VALUES (?1, 'Agriculture', 40)",
            params![uuid_str],
        )
        .unwrap();
        drop(conn);

        let db = MmoDatabase::open(v0_folder.clone()).unwrap();
        let status = db.combat_migration_status().await.unwrap();
        assert_eq!(status.schema_version, 2);
        assert_eq!(status.total_legacy_xp, 750);
        drop(db);
        assert_eq!(raw_xp(&v0_folder, &uuid_str, "Cultivation"), Some(40));
        cleanup(&v0_folder);

        // v1: only the consolidation remains to run.
        let v1_folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(&v1_folder, &[(&uuid_str, "Agriculture", 40)]);
        let db = MmoDatabase::open(v1_folder.clone()).unwrap();
        let status = db.combat_migration_status().await.unwrap();
        assert_eq!(status.schema_version, 2);
        assert_eq!(status.total_legacy_xp, 0);
        drop(db);
        assert_eq!(raw_xp(&v1_folder, &uuid_str, "Cultivation"), Some(40));
        cleanup(&v1_folder);
    }

    #[tokio::test]
    async fn failed_consolidation_rolls_back_and_surfaces_a_load_error() {
        let folder = test_db_folder();
        let uuid_str = Uuid::new_v4().to_string();
        seed_v1_db(
            &folder,
            &[
                (&uuid_str, "Agriculture", 100),
                (&uuid_str, "Herbalism", 50),
            ],
        );
        // Force the consolidation's first upsert to fail inside the
        // transaction (a test-only trigger; no production hook needed).
        let conn = Connection::open(folder.join("mmo.db")).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_consolidation BEFORE INSERT ON player_skills
             WHEN NEW.skill = 'Cultivation'
             BEGIN SELECT RAISE(ABORT, 'forced consolidation failure'); END;",
        )
        .unwrap();
        drop(conn);

        let error = MmoDatabase::open(folder.clone())
            .err()
            .expect("consolidation should fail and surface a load error");
        assert!(
            error.contains("forced consolidation failure"),
            "unexpected error: {error}"
        );

        // Nothing was committed: retired rows and the old version survive.
        assert_eq!(raw_schema_version(&folder), "1");
        assert_eq!(raw_xp(&folder, &uuid_str, "Agriculture"), Some(100));
        assert_eq!(raw_xp(&folder, &uuid_str, "Herbalism"), Some(50));
        assert_eq!(raw_xp(&folder, &uuid_str, "Cultivation"), None);
        cleanup(&folder);
    }

    #[tokio::test]
    async fn consolidation_saturates_at_the_sqlite_64_bit_limit() {
        let folder = test_db_folder();
        let uuid = Uuid::new_v4();
        let uuid_str = uuid.to_string();
        seed_v1_db(
            &folder,
            &[
                (&uuid_str, "Agriculture", i64::MAX),
                (&uuid_str, "Herbalism", 1),
            ],
        );

        let db = MmoDatabase::open(folder.clone()).unwrap();
        let cultivation = db.get_skill(uuid, SkillId::Cultivation).await.unwrap();
        assert_eq!(cultivation.xp, i64::MAX as u64);
        drop(db);
        cleanup(&folder);
    }

    #[test]
    fn consolidation_destinations_are_canonical_storage_keys() {
        for (anchor, secondary, destination) in RETIRED_SKILL_PAIRS {
            let anchor_skill = SkillId::from_name(anchor).expect("anchor must alias");
            let secondary_skill = SkillId::from_name(secondary).expect("secondary must alias");
            assert_eq!(anchor_skill.as_str(), destination);
            assert_eq!(secondary_skill.as_str(), destination);
        }
    }

    #[tokio::test]
    async fn merged_pair_activities_share_one_cumulative_xp_track() {
        // Both activities behind a merged skill award through the same `SKILL`
        // constant, so their XP accumulates on one shared track.
        let (db, folder) = open_test_db();
        let uuid = Uuid::new_v4();
        let curve = LevelCurve::new(&super::super::config::SkillConfig::default());

        let agriculture = crate::frontier::agriculture::SKILL;
        let herbalism = crate::frontier::herbalism::SKILL;
        assert_eq!(agriculture, herbalism);
        db.add_xp(uuid, agriculture, 10, curve.clone())
            .await
            .unwrap();
        db.add_xp(uuid, herbalism, 14, curve.clone()).await.unwrap();
        let cultivation = db.get_skill(uuid, SkillId::Cultivation).await.unwrap();
        assert_eq!(cultivation.xp, 24);

        let repair = crate::enterprise::repair::SKILL;
        let salvage = crate::enterprise::salvage::SKILL;
        assert_eq!(repair, salvage);
        db.add_xp(uuid, repair, 20, curve.clone()).await.unwrap();
        db.add_xp(uuid, salvage, 15, curve).await.unwrap();
        let maintenance = db.get_skill(uuid, SkillId::Maintenance).await.unwrap();
        assert_eq!(maintenance.xp, 35);
        cleanup(&folder);
    }
}
