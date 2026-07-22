use std::sync::{Arc, atomic::Ordering};

use pumpkin::{
    command::{
        CommandExecutor, CommandResult, CommandSender,
        args::ConsumedArgs,
        tree::{CommandTree, builder::literal},
    },
    plugin::Context,
    server::Server,
};
use pumpkin_util::{
    permission::{Permission, PermissionDefault, PermissionLvl},
    text::{TextComponent, color::NamedColor},
};

use crate::{
    drops::{ClearDropsState, clear_drops_debug},
    event_log::EventLogState,
    metrics::{MetricsReporterState, metrics_command_allowed, metrics_log_message},
};

const CABBAGE_PERMISSION: &str = "Cabbage:command.cabbage";
pub(crate) const CABBAGE_NAMES: [&str; 1] = ["cabbage"];
const CLEAR_DROPS_PERMISSION: &str = "Cabbage:command.clear_drops";
pub(crate) const CLEAR_DROPS_NAMES: [&str; 1] = ["cleardrops"];
const METRICS_PERMISSION: &str = "Cabbage:command.metrics";
pub(crate) const METRICS_NAMES: [&str; 1] = ["metrics"];
const EVENTS_PERMISSION: &str = "Cabbage:command.events";
pub(crate) const EVENTS_NAMES: [&str; 1] = ["events"];

struct CabbageInfoExecutor;

impl CommandExecutor for CabbageInfoExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            sender
                .send_message(TextComponent::text(
                    "Cabbage commands:\n/cleardrops - Queue dropped item cleanup.\n/metrics - Print current metrics once.\n/metrics log - Toggle periodic console metric logging.\n/events - Toggle Phase 2/3/4 event logging to chat and output.log.",
                ))
                .await;

            Ok(1)
        })
    }
}

struct EventsToggleExecutor {
    state: Arc<EventLogState>,
}

impl CommandExecutor for EventsToggleExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let enabled = self.state.toggle();
            let (state_text, color) = if enabled {
                ("on", NamedColor::Green)
            } else {
                ("off", NamedColor::Red)
            };

            sender
                .send_message(
                    TextComponent::text("Phase 2/3/4 event logging is ")
                        .add_child(TextComponent::text(state_text).color_named(color))
                        .add_text(". Events are written to output.log in the Cabbage data folder."),
                )
                .await;

            Ok(1)
        })
    }
}

struct ClearDropsExecutor {
    state: Arc<ClearDropsState>,
}

impl CommandExecutor for ClearDropsExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            let queued = !self.state.pending.swap(true, Ordering::SeqCst);
            if queued && let Ok(mut pending_sender) = self.state.sender.lock() {
                *pending_sender = Some(sender.clone());
            }

            clear_drops_debug(format!("command received from {sender}; queued={queued}"));

            sender
                .send_message(TextComponent::text(if queued {
                    "Queued dropped item cleanup for the next server tick."
                } else {
                    "Dropped item cleanup is already queued."
                }))
                .await;

            Ok(1)
        })
    }
}

struct MetricsPrintExecutor {
    state: Arc<MetricsReporterState>,
}

impl CommandExecutor for MetricsPrintExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            if !metrics_command_allowed(sender) {
                sender
                    .send_message(
                        TextComponent::text("You do not have permission to run /metrics.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            }

            let snapshot = self.state.collect_metrics(server);
            sender
                .send_message(TextComponent::text(snapshot.format()))
                .await;

            Ok(1)
        })
    }
}

struct MetricsLogToggleExecutor {
    state: Arc<MetricsReporterState>,
}

impl CommandExecutor for MetricsLogToggleExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            if !metrics_command_allowed(sender) {
                sender
                    .send_message(
                        TextComponent::text("You do not have permission to run /metrics.")
                            .color_named(NamedColor::Red),
                    )
                    .await;
                return Ok(0);
            }

            let enabled = self.state.toggle_metrics_log();
            sender.send_message(metrics_log_message(enabled)).await;

            Ok(1)
        })
    }
}

fn cabbage_command_tree() -> CommandTree {
    CommandTree::new(CABBAGE_NAMES, "List Cabbage commands").execute(CabbageInfoExecutor)
}

fn clear_drops_command_tree(state: Arc<ClearDropsState>) -> CommandTree {
    CommandTree::new(
        CLEAR_DROPS_NAMES,
        "Remove loaded and saved dropped item entities",
    )
    .execute(ClearDropsExecutor { state })
}

fn metrics_command_tree(state: Arc<MetricsReporterState>) -> CommandTree {
    CommandTree::new(METRICS_NAMES, "Show metrics and toggle metrics logging")
        .execute(MetricsPrintExecutor {
            state: state.clone(),
        })
        .then(literal("log").execute(MetricsLogToggleExecutor { state }))
}

fn events_command_tree(state: Arc<EventLogState>) -> CommandTree {
    CommandTree::new(
        EVENTS_NAMES,
        "Toggle Phase 2/3/4 event logging to chat and output.log",
    )
    .execute(EventsToggleExecutor { state })
}

pub(crate) async fn register_commands(
    context: &Arc<Context>,
    clear_drops_state: &Arc<ClearDropsState>,
    metrics_reporter_state: &Arc<MetricsReporterState>,
    event_log_state: &Arc<EventLogState>,
) -> Result<(), String> {
    let cabbage_permission = Permission::new(
        CABBAGE_PERMISSION,
        "Allows viewing Cabbage command information.",
        PermissionDefault::Op(PermissionLvl::Two),
    );

    match context.register_permission(cabbage_permission).await {
        Ok(()) => {}
        Err(error) if error.contains("already registered") => {}
        Err(error) => return Err(error),
    }

    let clear_drops_permission = Permission::new(
        CLEAR_DROPS_PERMISSION,
        "Allows clearing all loaded dropped item entities.",
        PermissionDefault::Allow,
    );

    match context.register_permission(clear_drops_permission).await {
        Ok(()) => {}
        Err(error) if error.contains("already registered") => {}
        Err(error) => return Err(error),
    }

    let metrics_permission = Permission::new(
        METRICS_PERMISSION,
        "Allows viewing metrics and toggling metrics console logging.",
        PermissionDefault::Op(PermissionLvl::Two),
    );

    match context.register_permission(metrics_permission).await {
        Ok(()) => {}
        Err(error) if error.contains("already registered") => {}
        Err(error) => return Err(error),
    }

    let events_permission = Permission::new(
        EVENTS_PERMISSION,
        "Allows toggling Phase 2/3/4 event logging to chat and output.log.",
        PermissionDefault::Op(PermissionLvl::Two),
    );

    match context.register_permission(events_permission).await {
        Ok(()) => {}
        Err(error) if error.contains("already registered") => {}
        Err(error) => return Err(error),
    }

    context
        .register_command(cabbage_command_tree(), CABBAGE_PERMISSION)
        .await;
    context
        .register_command(
            clear_drops_command_tree(clear_drops_state.clone()),
            CLEAR_DROPS_PERMISSION,
        )
        .await;
    context
        .register_command(
            metrics_command_tree(metrics_reporter_state.clone()),
            METRICS_PERMISSION,
        )
        .await;
    context
        .register_command(
            events_command_tree(event_log_state.clone()),
            EVENTS_PERMISSION,
        )
        .await;

    Ok(())
}
