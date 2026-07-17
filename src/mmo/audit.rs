//! Audit logging for progression-sensitive actions.
//!
//! Covers XP grants by admins/migrations, batch-break actions, item-quality
//! rolls, and committed anvil/grindstone/enchant operations. The audit log is
//! append-only (`mmo-audit.log` in the plugin data folder) and independent of
//! player-facing messages; console echo is configurable.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// Audit logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditConfig {
    /// Whether audit logging is active.
    pub enabled: bool,
    /// Also echo audit lines to the server console/log.
    pub console: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            console: false,
        }
    }
}

const AUDIT_LOG_FILE: &str = "mmo-audit.log";

/// Append-only audit log writer, lazily opened on first use.
#[derive(Debug, Default)]
pub(crate) struct AuditLog {
    file: Mutex<Option<File>>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write one audit line. Failures are logged and swallowed: auditing
    /// must never break gameplay.
    pub fn log(&self, data_folder: &PathBuf, config: &AuditConfig, message: &str) {
        if !config.enabled {
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("[{timestamp}] {message}\n");

        if let Ok(mut guard) = self.file.lock() {
            if guard.is_none() {
                let path = data_folder.join(AUDIT_LOG_FILE);
                let opened = std::fs::create_dir_all(data_folder)
                    .and_then(|()| OpenOptions::new().create(true).append(true).open(&path));
                match opened {
                    Ok(file) => *guard = Some(file),
                    Err(error) => {
                        log::warn!("[Cabbage MMO] failed to open audit log: {error}");
                        return;
                    }
                }
            }
            if let Some(file) = guard.as_mut() {
                let _ = file.write_all(line.as_bytes());
            }
        }

        if config.console {
            log::info!("[Cabbage MMO audit] {message}");
        }
    }
}
