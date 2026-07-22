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

use super::{
    MmoState,
    progression::{PlayerSnapshot, branch_mastery, fetch_snapshot},
    skills::{BranchId, SkillId},
    ui::{chat, menu, skill_enabled},
};

pub const MMO_NAMES: [&str; 1] = ["mmo"];
pub const MMO_PERMISSION: &str = "Cabbage:command.mmo";
pub(crate) const MMO_ADMIN_PERMISSION: &str = "Cabbage:command.mmo.admin";

/// Commands shown per `/mmo help` page; keeps each page inside the vanilla
/// chat view.
const HELP_PAGE_SIZE: usize = 8;

fn skill_names_hint() -> String {
    SkillId::ALL
        .iter()
        .map(|skill| skill.as_str().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

fn help_lines(admin: bool) -> Vec<String> {
    let mut lines = vec![
        "/mmo - Show your skill summary in chat.".to_string(),
        "/mmo menu [player] - Open the protected skill grid GUI.".to_string(),
        "/mmo stats [player] - Chat skill summary (text table for console).".to_string(),
        "/mmo stats chat [branch] - Chat summary, or one branch in detail.".to_string(),
        "/mmo top <skill> - Show top players for a skill.".to_string(),
        "/mmo help [page] - Show this command list.".to_string(),
    ];
    if admin {
        lines.push("/mmo reload - Reload MMO config.".to_string());
        lines.push("/mmo setxp <player> <skill> <xp> - Set player skill XP.".to_string());
        lines.push("/mmo migrate status - Show legacy Combat XP migration status.".to_string());
        lines.push("/mmo migrate combat <skill> - Move legacy Combat XP to a skill.".to_string());
    }
    lines
}

/// Compact, paginated `/mmo` command list. Page numbers are 1-based and
/// clamp into range. Shared by `/mmo help`, the console root response, and
/// the skill menu's Help slot.
pub(crate) fn help_message(admin: bool, page: usize) -> TextComponent {
    let lines = help_lines(admin);
    let page_count = lines.len().div_ceil(HELP_PAGE_SIZE).max(1);
    let page = page.clamp(1, page_count);
    let start = (page - 1) * HELP_PAGE_SIZE;
    let end = (start + HELP_PAGE_SIZE).min(lines.len());
    let mut out = vec![format!("Cabbage MMO commands (page {page}/{page_count}):")];
    out.extend_from_slice(&lines[start..end]);
    if page < page_count {
        out.push(format!("Next page: /mmo help {}", page + 1));
    }
    TextComponent::text(out.join("\n"))
}

fn format_snapshot(snapshot: &PlayerSnapshot, state: &MmoState) -> String {
    let mut lines = Vec::new();
    for branch in BranchId::ALL {
        let mastery = branch_mastery(*branch, |skill| {
            snapshot.level_of(skill, &state.curve(skill))
        });
        lines.push(format!("== {branch} (mastery {mastery:.1}) =="));
        for skill in branch.skills() {
            let progress = snapshot.get(*skill).format_progress(&state.curve(*skill));
            lines.push(format!("{skill}: {progress}"));
        }
    }
    lines.join("\n")
}

/// Shared snapshot fetch + chat summary send behind `/mmo`, player
/// `/mmo stats [player]`, and `/mmo stats chat` so the three entry points
/// cannot drift. Each visual row is sent as its own system message to
/// preserve per-cell styling and hover text. Returns the command result.
async fn send_summary_grid(
    state: &MmoState,
    sender: &CommandSender,
    viewer: &Arc<Player>,
    target: &Player,
) -> i32 {
    let snapshot = match fetch_snapshot(state, target.gameprofile.id).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            sender
                .send_message(
                    TextComponent::text(format!("Failed to fetch skills: {error}"))
                        .color_named(NamedColor::Red),
                )
                .await;
            return 0;
        }
    };

    let config = state.config();
    let skill_info = |skill: SkillId| (state.curve(skill), skill_enabled(&config, skill));
    let target_name = (viewer.gameprofile.id != target.gameprofile.id)
        .then_some(target.gameprofile.name.as_str());

    for line in chat::summary_lines(&snapshot, &skill_info, target_name) {
        viewer.send_system_message(&line).await;
    }
    1
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
            if let Some(player) = sender.as_player() {
                // Players get the 10-line chat summary grid; the protected
                // inventory GUI lives at /mmo menu.
                let result = send_summary_grid(&self.state, sender, &player, &player).await;
                return Ok(result);
            }
            // Console/RCON cannot receive a chat HUD grid; text help only.
            let admin = sender.has_permission_lvl(PermissionLvl::Three);
            sender.send_message(help_message(admin, 1)).await;
            Ok(1)
        })
    }
}

struct MmoHelpExecutor;

impl CommandExecutor for MmoHelpExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let page = match SimpleArgConsumer::find_arg(args, "page") {
                Ok(raw) => match raw.parse::<usize>() {
                    Ok(page) => page,
                    Err(_) => {
                        sender
                            .send_message(
                                TextComponent::text("Usage: /mmo help [page]")
                                    .color_named(NamedColor::Red),
                            )
                            .await;
                        return Ok(0);
                    }
                },
                Err(_) => 1,
            };
            let admin = sender.has_permission_lvl(PermissionLvl::Three);
            sender.send_message(help_message(admin, page)).await;
            Ok(1)
        })
    }
}

struct MmoStatsExecutor {
    state: Arc<MmoState>,
}

struct MmoMenuExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoMenuExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let Some(viewer) = sender.as_player() else {
                sender
                    .send_message(
                        TextComponent::text("Only players can open the MMO menu.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };
            // GUI behavior stays under this explicit subcommand; an optional
            // online target reuses the same viewer/target menu split.
            let target = match SimpleArgConsumer::find_arg(args, "target") {
                Ok(name) => match server.get_player_by_name(name) {
                    Some(player) => player,
                    None => {
                        sender
                            .send_message(
                                TextComponent::text("Player not found.")
                                    .color_named(NamedColor::Red),
                            )
                            .await;
                        return Ok(0);
                    }
                },
                Err(_) => viewer.clone(),
            };
            if let Err(error) = menu::open_skill_menu(&self.state, viewer, &target).await {
                sender
                    .send_message(
                        TextComponent::text(format!("{error}. Try /mmo for a chat summary."))
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            }
            Ok(1)
        })
    }
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

            let Some(target) = target else {
                sender
                    .send_message(
                        TextComponent::text("Player not found.").color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            if let Some(viewer) = sender.as_player() {
                // Player senders get the target's chat summary grid.
                let result = send_summary_grid(&self.state, sender, &viewer, &target).await;
                return Ok(result);
            }

            // Console/RCON keeps complete text output; it is not bounded by
            // the game chat HUD.
            match fetch_snapshot(&self.state, target.gameprofile.id).await {
                Ok(snapshot) => {
                    let header = format!("{}'s MMO skills:", target.gameprofile.name);
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

struct MmoChatStatsExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoChatStatsExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let Some(player) = sender.as_player() else {
                sender
                    .send_message(
                        TextComponent::text(
                            "Only players can use the chat skill summary; use /mmo stats <player> instead."
                        )
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            let branch = match SimpleArgConsumer::find_arg(args, "branch") {
                // Backward-compatible alias for the summary grid.
                Err(_) => {
                    let result = send_summary_grid(&self.state, sender, &player, &player).await;
                    return Ok(result);
                }
                Ok(name) => match BranchId::from_name(name) {
                    Some(branch) => branch,
                    None => {
                        sender
                            .send_message(
                                TextComponent::text(
                                    "Unknown branch. Try: frontier, warfare, enterprise",
                                )
                                .color_named(NamedColor::Red),
                            )
                            .await;
                        return Ok(0);
                    }
                },
            };

            let snapshot = match fetch_snapshot(&self.state, player.gameprofile.id).await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!("Failed to fetch skills: {error}"))
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let config = self.state.config();
            let skill_info =
                |skill: SkillId| (self.state.curve(skill), skill_enabled(&config, skill));

            for line in chat::branch_lines(&snapshot, &skill_info, branch) {
                player.send_system_message(&line).await;
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
                            TextComponent::text("Usage: /mmo top <skill>")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let Some(skill) = SkillId::from_name(skill_arg) else {
                sender
                    .send_message(
                        TextComponent::text(format!("Unknown skill. Try: {}", skill_names_hint()))
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
                    let mut lines = vec![format!("Top {skill} players")];
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

            let Some(skill) = SkillId::from_name(skill_arg) else {
                sender
                    .send_message(
                        TextComponent::text(format!("Unknown skill. Try: {}", skill_names_hint()))
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
                                "Set {}'s {skill} XP to {} (Level {}).",
                                player.gameprofile.name, xp, new_level
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

struct MmoMigrateStatusExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoMigrateStatusExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            match self.state.db().combat_migration_status().await {
                Ok(status) => {
                    let destination = match status.migrated_to.as_deref() {
                        Some(target) => format!("migrated to {target}"),
                        None => "not migrated".to_string(),
                    };
                    sender
                        .send_message(TextComponent::text(format!(
                            "Legacy Combat XP status:\nSchema version: {}\nPreserved: {} player(s), {} XP\nDestination: {}",
                            status.schema_version,
                            status.players_with_legacy_xp,
                            status.total_legacy_xp,
                            destination,
                        )))
                        .await;
                }
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!(
                                "Failed to read migration status: {error}"
                            ))
                            .color_named(NamedColor::Red),
                        )
                        .await;
                }
            }
            Ok(1)
        })
    }
}

struct MmoMigrateCombatExecutor {
    state: Arc<MmoState>,
}

impl CommandExecutor for MmoMigrateCombatExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let skill_arg: &str = match SimpleArgConsumer::find_arg(args, "skill") {
                Ok(s) => s,
                Err(_) => {
                    sender
                        .send_message(
                            TextComponent::text("Usage: /mmo migrate combat <skill>")
                                .color_named(NamedColor::Red),
                        )
                        .await;
                    return Ok(0);
                }
            };

            let Some(skill) = SkillId::from_name(skill_arg) else {
                sender
                    .send_message(
                        TextComponent::text(format!("Unknown skill. Try: {}", skill_names_hint()))
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            };

            match self.state.db().migrate_combat_xp(skill).await {
                Ok(outcome) => {
                    sender
                        .send_message(
                            TextComponent::text(format!(
                                "Migrated {} XP across {} player(s) from legacy Combat to {skill}.",
                                outcome.xp_moved, outcome.players_migrated
                            ))
                            .color_named(NamedColor::Green),
                        )
                        .await;
                }
                Err(error) => {
                    sender
                        .send_message(
                            TextComponent::text(format!("Failed to migrate Combat XP: {error}"))
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
            literal("help").execute(MmoHelpExecutor).then(
                pumpkin::command::tree::builder::argument("page", SimpleArgConsumer)
                    .execute(MmoHelpExecutor),
            ),
        )
        .then(
            literal("stats")
                .execute(MmoStatsExecutor {
                    state: state.clone(),
                })
                // The "chat" literal is registered before the "target"
                // argument so `/mmo stats chat` always selects the fallback.
                .then(
                    literal("chat")
                        .execute(MmoChatStatsExecutor {
                            state: state.clone(),
                        })
                        .then(
                            pumpkin::command::tree::builder::argument("branch", SimpleArgConsumer)
                                .execute(MmoChatStatsExecutor {
                                    state: state.clone(),
                                }),
                        ),
                )
                .then(
                    pumpkin::command::tree::builder::argument("target", SimpleArgConsumer).execute(
                        MmoStatsExecutor {
                            state: state.clone(),
                        },
                    ),
                ),
        )
        .then(
            literal("menu")
                .execute(MmoMenuExecutor {
                    state: state.clone(),
                })
                .then(
                    pumpkin::command::tree::builder::argument("target", SimpleArgConsumer).execute(
                        MmoMenuExecutor {
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
        .then(literal("setxp").then(
            pumpkin::command::tree::builder::argument("player", SimpleArgConsumer).then(
                pumpkin::command::tree::builder::argument("skill", SimpleArgConsumer).then(
                    pumpkin::command::tree::builder::argument("xp", SimpleArgConsumer).execute(
                        MmoSetXpExecutor {
                            state: state.clone(),
                        },
                    ),
                ),
            ),
        ))
        .then(
            literal("migrate")
                .then(literal("status").execute(MmoMigrateStatusExecutor {
                    state: state.clone(),
                }))
                .then(
                    literal("combat").then(
                        pumpkin::command::tree::builder::argument("skill", SimpleArgConsumer)
                            .execute(MmoMigrateCombatExecutor { state }),
                    ),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_text_routes_root_to_chat_and_menu_to_the_gui() {
        let text = help_lines(false).join("\n");
        assert!(
            text.contains("/mmo - Show your skill summary in chat."),
            "{text}"
        );
        assert!(
            text.contains("/mmo menu [player] - Open the protected skill grid GUI."),
            "{text}"
        );
        assert!(
            text.contains("/mmo stats [player] - Chat skill summary (text table for console)."),
            "{text}"
        );
        assert!(
            text.contains("/mmo stats chat [branch] - Chat summary, or one branch in detail."),
            "{text}"
        );
    }

    #[test]
    fn help_pages_stay_within_the_chat_line_budget() {
        for admin in [false, true] {
            let page_count = help_lines(admin).len().div_ceil(HELP_PAGE_SIZE).max(1);
            for page in 1..=page_count {
                let line_count = help_message(admin, page).get_text().lines().count();
                assert!(
                    line_count <= chat::MAX_CHAT_LINES,
                    "help page {page} (admin={admin}) has {line_count} lines"
                );
            }
        }
    }

    #[test]
    fn help_page_numbers_clamp_into_range() {
        let first = help_message(false, 1).get_text();
        assert_eq!(help_message(false, 0).get_text(), first);
        assert_eq!(help_message(false, 999).get_text(), first);
        assert!(!first.contains("Next page"));
    }
}
