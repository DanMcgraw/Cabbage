use std::sync::Arc;

use pumpkin::{
    command::{
        CommandExecutor, CommandResult, CommandSender,
        args::{ConsumedArgs, FindArg, simple::SimpleArgConsumer},
        tree::{CommandTree, builder::literal},
    },
    entity::player::Player,
    server::Server,
};
use pumpkin_util::{
    permission::PermissionLvl,
    text::{TextComponent, color::NamedColor},
};

use super::{MmoState, player::PlayerSnapshot, skills::SkillId};

pub const MMO_NAMES: [&str; 1] = ["mmo"];
pub const MMO_PERMISSION: &str = "Cabbage:command.mmo";
pub(crate) const MMO_ADMIN_PERMISSION: &str = "Cabbage:command.mmo.admin";

fn skill_from_arg(arg: &str) -> Option<SkillId> {
    match arg.to_ascii_lowercase().as_str() {
        "mining" => Some(SkillId::Mining),
        "combat" => Some(SkillId::Combat),
        _ => None,
    }
}

fn format_snapshot(snapshot: &PlayerSnapshot, state: &MmoState) -> String {
    let mining = snapshot
        .mining
        .format_progress(&state.curve(SkillId::Mining));
    let combat = snapshot
        .combat
        .format_progress(&state.curve(SkillId::Combat));
    format!("Mining: {mining}\nCombat: {combat}",)
}

async fn fetch_snapshot(state: &MmoState, player: &Player) -> Result<PlayerSnapshot, String> {
    let mut snapshot = PlayerSnapshot::default();
    for skill in SkillId::ALL {
        let progress = state.db().get_skill(player.gameprofile.id, *skill).await?;
        snapshot.set(*skill, super::player::PlayerSkillSnapshot::new(progress.xp));
    }
    Ok(snapshot)
}

struct MmoRootExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoRootExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let mut lines = vec![
                "Cabbage MMO commands:".to_string(),
                "/mmo - Show your skill summary.".to_string(),
                "/mmo stats [player] - Show Mining and Combat levels.".to_string(),
                "/mmo top <skill> - Show top players for a skill.".to_string(),
            ];

            if sender.has_permission_lvl(PermissionLvl::Three) {
                lines.push("/mmo reload - Reload MMO config.".to_string());
                lines.push("/mmo setxp <player> <skill> <xp> - Set player skill XP.".to_string());
            }

            if let Some(player) = sender.as_player() {
                match fetch_snapshot(&self.state, &player).await {
                    Ok(snapshot) => {
                        lines.push(format!(
                            "\nYour skills:\n{}",
                            format_snapshot(&snapshot, &self.state)
                        ));
                    }
                    Err(error) => {
                        lines.push(format!("\n[Error fetching your skills: {error}]"));
                    }
                }
            }

            sender
                .send_message(TextComponent::text(lines.join("\n")))
                .await;
            Ok(1)
        })
    }
}

struct MmoStatsExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoStatsExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let target = if let Ok(name) = SimpleArgConsumer::find_arg(args, "target") {
                server.get_player_by_name(name)
            } else {
                sender.as_player()
            };

            let Some(player) = target else {
                sender
                    .send_message(
                        TextComponent::text("Player not found.").color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            match fetch_snapshot(&self.state, &player).await {
                Ok(snapshot) => {
                    let header = format!("{}'s MMO skills:", player.gameprofile.name);
                    let body = format_snapshot(&snapshot, &self.state);
                    sender
                        .send_message(TextComponent::text(format!("{header}\n{body}")))
                        .await;
                }
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!("Failed to fetch skills: {error}"))
                                .color_named(NamedColor::Red),
                        )
                        .await;
                }
            }

            Ok(1)
        })
    }
}

fn resolve_player_name(server: &Server, uuid_str: &str) -> String {
    let Ok(uuid) = uuid::Uuid::parse_str(uuid_str) else {
        return uuid_str.to_string();
    };

    if let Some(player) = server.get_player_by_uuid(uuid) {
        return player.gameprofile.name.clone();
    }

    if let Ok(mut cache) = server.data.user_cache.try_write() {
        if let Some(entry) = cache.get_by_uuid(uuid) {
            return entry.name;
        }
    }

    uuid_str.to_string()
}

struct MmoTopExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoTopExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let skill_arg: &str = match SimpleArgConsumer::find_arg(args, "skill") {
                Ok(s) => s,
                Err(_) => {
                    sender
                        .send_message(
                            TextComponent::text("Usage: /mmo top <mining|combat>")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let Some(skill) = skill_from_arg(skill_arg) else {
                sender
                    .send_message(
                        TextComponent::text("Skill must be mining or combat.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            match self
                .state
                .db()
                .get_top(skill, 10, self.state.curve(skill))
                .await
            {
                Ok(rows) => {
                    let mut lines = vec![format!("Top {} players", skill)];
                    for (index, (uuid_str, level, xp)) in rows.iter().enumerate() {
                        let name = resolve_player_name(server, uuid_str);
                        let (_, into, needed) = self.state.curve(skill).level_for_xp(*xp);
                        lines.push(format!(
                            "{}. {} - Level {} ({}/{} XP)",
                            index + 1,
                            name,
                            level,
                            into,
                            needed
                        ));
                    }
                    if lines.len() == 1 {
                        lines.push("No players found.".to_string());
                    }
                    sender
                        .send_message(TextComponent::text(lines.join("\n")))
                        .await;
                }
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!("Failed to fetch top players: {error}"))
                                .color_named(NamedColor::Red),
                        )
                        .await;
                }
            }

            Ok(1)
        })
    }
}

struct MmoReloadExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoReloadExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            match self.state.reload_config().await {
                Ok(()) => {
                    sender
                        .send_message(
                            TextComponent::text("MMO configuration reloaded.")
                                .color_named(NamedColor::Green),
                        )
                        .await;
                }
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!("Failed to reload MMO config: {error}"))
                                .color_named(NamedColor::Red),
                        )
                        .await;
                }
            }
            Ok(1)
        })
    }
}

struct MmoSetXpExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoSetXpExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let player_name: &str = match SimpleArgConsumer::find_arg(args, "player") {
                Ok(name) => name,
                Err(_) => {
                    sender
                        .send_message(
                            TextComponent::text("Usage: /mmo setxp <player> <skill> <xp>")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let skill_arg: &str = match SimpleArgConsumer::find_arg(args, "skill") {
                Ok(s) => s,
                Err(_) => {
                    sender
                        .send_message(
                            TextComponent::text("Usage: /mmo setxp <player> <skill> <xp>")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let xp_arg: &str = match SimpleArgConsumer::find_arg(args, "xp") {
                Ok(x) => x,
                Err(_) => {
                    sender
                        .send_message(
                            TextComponent::text("Usage: /mmo setxp <player> <skill> <xp>")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let Some(player) = server.get_player_by_name(player_name) else {
                sender
                    .send_message(
                        TextComponent::text("Player not found.").color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            let Some(skill) = skill_from_arg(skill_arg) else {
                sender
                    .send_message(
                        TextComponent::text("Skill must be mining or combat.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            let xp = match xp_arg.parse::<u64>() {
                Ok(x) => x,
                Err(_) => {
                    sender
                        .send_message(
                            TextComponent::text("XP must be a non-negative number.")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let curve = self.state.curve(skill);
            match self
                .state
                .db()
                .set_xp(player.gameprofile.id, skill, xp, curve)
                .await
            {
                Ok(new_level) => {
                    sender
                        .send_message(
                            TextComponent::text(format!(
                                "Set {}'s {} XP to {} (Level {}).",
                                player.gameprofile.name, skill, xp, new_level
                            ))
                            .color_named(NamedColor::Green),
                        )
                        .await;
                }
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!("Failed to set XP: {error}"))
                                .color_named(NamedColor::Red),
                        )
                        .await;
                }
            }

            Ok(1)
        })
    }
}

pub fn mmo_command_tree(state: Arc<MmoState>) -> CommandTree {
    CommandTree::new(MMO_NAMES, "Cabbage MMO levelling commands")
        .execute(MmoRootExecutor {
            state: state.clone(),
        })
        .then(
            literal("stats")
                .execute(MmoStatsExecutor {
                    state: state.clone(),
                })
                .then(
                    pumpkin::command::tree::builder::argument("target", SimpleArgConsumer).execute(
                        MmoStatsExecutor {
                            state: state.clone(),
                        },
                    ),
                ),
        )
        .then(literal("top").then(
            pumpkin::command::tree::builder::argument("skill", SimpleArgConsumer).execute(
                MmoTopExecutor {
                    state: state.clone(),
                },
            ),
        ))
        .then(literal("reload").execute(MmoReloadExecutor {
            state: state.clone(),
        }))
        .then(
            literal("setxp").then(
                pumpkin::command::tree::builder::argument("player", SimpleArgConsumer).then(
                    pumpkin::command::tree::builder::argument("skill", SimpleArgConsumer).then(
                        pumpkin::command::tree::builder::argument("xp", SimpleArgConsumer)
                            .execute(MmoSetXpExecutor { state }),
                    ),
                ),
            ),
        )
}
