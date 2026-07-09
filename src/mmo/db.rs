use std::{
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use futures::channel::oneshot;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::config::LevelCurve;
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
        respond: oneshot::Sender<Result<Vec<(String, u32, u64)>, String>>,
    },
    GetMobXp {
        mob_resource_name: String,
        respond: oneshot::Sender<Result<Option<u64>, String>>,
    },
    GetOreXp {
        block_name: String,
        respond: oneshot::Sender<Result<Option<u64>, String>>,
    },
    ReplaceMobXp {
        values: Vec<(String, u64)>,
        respond: oneshot::Sender<Result<(), String>>,
    },
    ReplaceOreXp {
        values: Vec<(String, u64)>,
        respond: oneshot::Sender<Result<(), String>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSkill {
    pub level: u32,
    pub xp: u64,
}

impl Default for PlayerSkill {
    fn default() -> Self {
        Self { level: 1, xp: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct XpResult {
    pub new_level: u32,
    #[allow(dead_code)]
    pub new_xp: u64,
    pub leveled_up: bool,
}

/// Handle to the dedicated database worker thread.
pub struct MmoDatabase {
    sender: Sender<DbRequest>,
    _worker: JoinHandle<()>,
}

impl MmoDatabase {
    /// Spawn a dedicated worker thread and open the SQLite database in it.
    pub fn open(data_folder: PathBuf) -> Result<Self, String> {
        let db_path = data_folder.join("mmo.db");
        let (sender, receiver) = mpsc::channel::<DbRequest>();

        let worker = thread::Builder::new()
            .name("cabbage-mmo-db".to_string())
            .spawn(move || {
                let mut conn = match Connection::open(&db_path) {
                    Ok(conn) => conn,
                    Err(e) => {
                        log::error!(
                            "[Cabbage MMO] failed to open database at {}: {e}",
                            db_path.display()
                        );
                        return;
                    }
                };

                if let Err(e) = Self::migrate(&conn) {
                    log::error!("[Cabbage MMO] failed to run migrations: {e}");
                    return;
                }
                if let Err(e) = Self::seed_defaults(&conn) {
                    log::error!("[Cabbage MMO] failed to seed xp tables: {e}");
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
                            respond,
                        } => {
                            let _ = respond.send(Self::do_get_top(&conn, skill, limit));
                        }
                        DbRequest::GetMobXp {
                            mob_resource_name,
                            respond,
                        } => {
                            let _ = respond.send(Self::do_get_mob_xp(&conn, &mob_resource_name));
                        }
                        DbRequest::GetOreXp {
                            block_name,
                            respond,
                        } => {
                            let _ = respond.send(Self::do_get_ore_xp(&conn, &block_name));
                        }
                        DbRequest::ReplaceMobXp { values, respond } => {
                            let _ = respond.send(Self::do_replace_mob_xp(&mut conn, &values));
                        }
                        DbRequest::ReplaceOreXp { values, respond } => {
                            let _ = respond.send(Self::do_replace_ore_xp(&mut conn, &values));
                        }
                    }
                }
            })
            .map_err(|e| format!("failed to spawn mmo database worker: {e}"))?;

        Ok(Self {
            sender,
            _worker: worker,
        })
    }

    fn migrate(conn: &Connection) -> Result<(), String> {
        let statements = [
            r"CREATE TABLE IF NOT EXISTS player_skills (
                player_uuid TEXT NOT NULL,
                skill TEXT NOT NULL,
                level INTEGER NOT NULL DEFAULT 1,
                xp INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (player_uuid, skill)
            );",
            r"CREATE TABLE IF NOT EXISTS mob_xp (
                mob_resource_name TEXT PRIMARY KEY,
                xp INTEGER NOT NULL
            );",
            r"CREATE TABLE IF NOT EXISTS ore_xp (
                block_name TEXT PRIMARY KEY,
                xp INTEGER NOT NULL
            );",
        ];

        for sql in statements {
            conn.execute(sql, [])
                .map_err(|e| format!("failed to run mmo migration: {e}"))?;
        }
        Ok(())
    }

    fn seed_defaults(conn: &Connection) -> Result<(), String> {
        let mob_sql = r"
            INSERT OR IGNORE INTO mob_xp (mob_resource_name, xp) VALUES
                ('zombie', 12), ('skeleton', 14), ('creeper', 18),
                ('spider', 12), ('enderman', 28), ('witch', 24),
                ('drowned', 14), ('husk', 14), ('stray', 14),
                ('phantom', 20), ('slime', 8), ('cave_spider', 14),
                ('piglin', 16), ('piglin_brute', 32), ('zombified_piglin', 16),
                ('blaze', 22), ('ghast', 28), ('wither_skeleton', 30);
        ";

        let ore_sql = r"
            INSERT OR IGNORE INTO ore_xp (block_name, xp) VALUES
                ('coal_ore', 8), ('deepslate_coal_ore', 10),
                ('iron_ore', 15), ('deepslate_iron_ore', 18),
                ('copper_ore', 12), ('deepslate_copper_ore', 14),
                ('gold_ore', 25), ('deepslate_gold_ore', 28),
                ('redstone_ore', 12), ('deepslate_redstone_ore', 14),
                ('lapis_ore', 20), ('deepslate_lapis_ore', 22),
                ('diamond_ore', 60), ('deepslate_diamond_ore', 70),
                ('emerald_ore', 50), ('deepslate_emerald_ore', 55),
                ('nether_quartz_ore', 16), ('nether_gold_ore', 22),
                ('ancient_debris', 150);
        ";

        conn.execute(mob_sql, [])
            .map_err(|e| format!("failed to seed mob xp table: {e}"))?;
        conn.execute(ore_sql, [])
            .map_err(|e| format!("failed to seed ore xp table: {e}"))?;
        Ok(())
    }

    fn do_get_skill(
        conn: &Connection,
        player_uuid: Uuid,
        skill: SkillId,
    ) -> Result<PlayerSkill, String> {
        let mut stmt = conn
            .prepare("SELECT level, xp FROM player_skills WHERE player_uuid = ?1 AND skill = ?2")
            .map_err(|e| format!("failed to prepare player skill query: {e}"))?;

        let result = stmt
            .query_row(params![player_uuid.to_string(), skill.to_string()], |row| {
                let level: i64 = row.get(0).unwrap_or(1);
                let xp: i64 = row.get(1).unwrap_or(0);
                Ok(PlayerSkill {
                    level: level.max(1) as u32,
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
        let new_xp = current.xp.saturating_add(xp);
        let new_level = curve.level_for_xp(new_xp).0;
        let leveled_up = new_level > current.level;

        conn.execute(
            "INSERT INTO player_skills (player_uuid, skill, level, xp)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(player_uuid, skill)
             DO UPDATE SET level = excluded.level, xp = excluded.xp",
            params![
                player_uuid.to_string(),
                skill.to_string(),
                new_level as i64,
                new_xp as i64,
            ],
        )
        .map_err(|e| format!("failed to update player xp: {e}"))?;

        Ok(XpResult {
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
            "INSERT INTO player_skills (player_uuid, skill, level, xp)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(player_uuid, skill)
             DO UPDATE SET level = excluded.level, xp = excluded.xp",
            params![
                player_uuid.to_string(),
                skill.to_string(),
                new_level as i64,
                xp as i64,
            ],
        )
        .map_err(|e| format!("failed to set player xp: {e}"))?;

        Ok(new_level)
    }

    fn do_get_top(
        conn: &Connection,
        skill: SkillId,
        limit: u32,
    ) -> Result<Vec<(String, u32, u64)>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT player_uuid, level, xp FROM player_skills
                 WHERE skill = ?1
                 ORDER BY level DESC, xp DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("failed to prepare top players query: {e}"))?;

        let rows = stmt
            .query_map(params![skill.to_string(), limit as i64], |row| {
                let uuid: String = row.get(0).unwrap_or_default();
                let level: i64 = row.get(1).unwrap_or(1);
                let xp: i64 = row.get(2).unwrap_or(0);
                Ok((uuid, level.max(1) as u32, xp.max(0) as u64))
            })
            .map_err(|e| format!("failed to query top players: {e}"))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("failed to read top player row: {e}"))?);
        }
        Ok(results)
    }

    fn do_get_mob_xp(conn: &Connection, mob_resource_name: &str) -> Result<Option<u64>, String> {
        let result = conn
            .query_row(
                "SELECT xp FROM mob_xp WHERE mob_resource_name = ?1",
                params![mob_resource_name],
                |row| {
                    let xp: i64 = row.get(0).unwrap_or(0);
                    Ok(xp.max(0) as u64)
                },
            )
            .optional()
            .map_err(|e| format!("failed to query mob xp: {e}"))?;
        Ok(result)
    }

    fn do_get_ore_xp(conn: &Connection, block_name: &str) -> Result<Option<u64>, String> {
        let result = conn
            .query_row(
                "SELECT xp FROM ore_xp WHERE block_name = ?1",
                params![block_name],
                |row| {
                    let xp: i64 = row.get(0).unwrap_or(0);
                    Ok(xp.max(0) as u64)
                },
            )
            .optional()
            .map_err(|e| format!("failed to query ore xp: {e}"))?;
        Ok(result)
    }

    fn do_replace_mob_xp(conn: &mut Connection, values: &[(String, u64)]) -> Result<(), String> {
        let tx = conn
            .transaction()
            .map_err(|e| format!("failed to start mob xp transaction: {e}"))?;
        tx.execute("DELETE FROM mob_xp", [])
            .map_err(|e| format!("failed to clear mob xp table: {e}"))?;
        for (name, xp) in values {
            tx.execute(
                "INSERT INTO mob_xp (mob_resource_name, xp) VALUES (?1, ?2)",
                params![name.as_str(), *xp as i64],
            )
            .map_err(|e| format!("failed to insert mob xp: {e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("failed to commit mob xp transaction: {e}"))
    }

    fn do_replace_ore_xp(conn: &mut Connection, values: &[(String, u64)]) -> Result<(), String> {
        let tx = conn
            .transaction()
            .map_err(|e| format!("failed to start ore xp transaction: {e}"))?;
        tx.execute("DELETE FROM ore_xp", [])
            .map_err(|e| format!("failed to clear ore xp table: {e}"))?;
        for (name, xp) in values {
            tx.execute(
                "INSERT INTO ore_xp (block_name, xp) VALUES (?1, ?2)",
                params![name.as_str(), *xp as i64],
            )
            .map_err(|e| format!("failed to insert ore xp: {e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("failed to commit ore xp transaction: {e}"))
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
    ) -> Result<Vec<(String, u32, u64)>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::GetTop {
                skill,
                limit,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn get_mob_xp(&self, mob_resource_name: String) -> Result<Option<u64>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::GetMobXp {
                mob_resource_name,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    pub async fn get_ore_xp(&self, block_name: String) -> Result<Option<u64>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::GetOreXp {
                block_name,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    #[allow(dead_code)]
    pub async fn replace_mob_xp(&self, values: Vec<(String, u64)>) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::ReplaceMobXp {
                values,
                respond: tx,
            })
            .map_err(|_| "mmo database worker has shut down".to_string())?;
        rx.await
            .map_err(|_| "mmo database worker dropped the response".to_string())?
    }

    #[allow(dead_code)]
    pub async fn replace_ore_xp(&self, values: Vec<(String, u64)>) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(DbRequest::ReplaceOreXp {
                values,
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
        });

        let result = db
            .add_xp(uuid, SkillId::Mining, 75, curve.clone())
            .await
            .unwrap();
        assert_eq!(result.new_xp, 75);
        assert!(result.leveled_up);

        let skill = db.get_skill(uuid, SkillId::Mining).await.unwrap();
        assert_eq!(skill.xp, 75);
        assert_eq!(skill.level, 2);

        cleanup(&folder);
    }

    #[tokio::test]
    async fn default_ore_xp_present() {
        let (db, folder) = open_test_db();
        assert!(
            db.get_ore_xp("diamond_ore".to_string())
                .await
                .unwrap()
                .is_some()
        );
        assert!(db.get_ore_xp("dirt".to_string()).await.unwrap().is_none());
        cleanup(&folder);
    }
}
