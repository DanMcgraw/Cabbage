//! Audit logging for progression-sensitive actions.
//!
//! Covers XP grants by admins/migrations, batch-break actions, item-quality
//! rolls, and committed anvil/grindstone/enchant operations. The audit log is
//! append-only (`mmo-audit.log` in the plugin data folder) and independent of
//! player-facing messages; console echo is configurable.

use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
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

enum AuditRequest {
    Line(String),
    Shutdown,
}

/// Append-only audit writer backed by a dedicated file-I/O thread.
#[derive(Debug)]
pub(crate) struct AuditLog {
    sender: Sender<AuditRequest>,
    worker: Option<JoinHandle<()>>,
}

impl AuditLog {
    pub fn new(data_folder: PathBuf) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("cabbage-mmo-audit".to_string())
            .spawn(move || {
                let path = data_folder.join(AUDIT_LOG_FILE);
                let mut writer: Option<BufWriter<std::fs::File>> = None;
                while let Ok(request) = receiver.recv() {
                    match request {
                        AuditRequest::Line(line) => {
                            if writer.is_none() {
                                let opened = std::fs::create_dir_all(&data_folder).and_then(|()| {
                                    OpenOptions::new().create(true).append(true).open(&path)
                                });
                                match opened {
                                    Ok(file) => writer = Some(BufWriter::new(file)),
                                    Err(error) => {
                                        log::warn!(
                                            "[Cabbage MMO] failed to open audit log: {error}"
                                        );
                                        continue;
                                    }
                                }
                            }
                            let write_error = writer
                                .as_mut()
                                .and_then(|output| output.write_all(line.as_bytes()).err());
                            if let Some(error) = write_error {
                                log::warn!("[Cabbage MMO] failed to write audit log: {error}");
                                writer = None;
                            }
                        }
                        AuditRequest::Shutdown => break,
                    }
                }
                if let Some(mut writer) = writer {
                    writer.flush().ok();
                }
            })
            .map_err(|error| format!("failed to spawn MMO audit worker: {error}"))?;
        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    /// Queue one audit line without performing file I/O on the event path.
    pub fn log(&self, config: &AuditConfig, message: &str) {
        if !config.enabled {
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = self
            .sender
            .send(AuditRequest::Line(format!("[{timestamp}] {message}\n")));

        if config.console {
            log::info!("[Cabbage MMO audit] {message}");
        }
    }
}

impl Drop for AuditLog {
    fn drop(&mut self) {
        let _ = self.sender.send(AuditRequest::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
