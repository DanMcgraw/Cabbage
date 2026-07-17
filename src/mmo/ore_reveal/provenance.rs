use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use pumpkin::world::World;
use pumpkin_util::math::position::BlockPos;

use crate::mmo::db::MmoDatabase;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProvenanceKey {
    pub world: String,
    pub dimension: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ProvenanceKey {
    pub fn new(world: &World, position: BlockPos) -> Self {
        Self {
            world: world.get_world_name().to_string(),
            dimension: world.dimension.minecraft_name.to_string(),
            x: position.0.x,
            y: position.0.y,
            z: position.0.z,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProvenanceChange {
    pub key: ProvenanceKey,
    pub non_natural: bool,
}

pub(crate) struct ProvenanceTracker {
    non_natural: Mutex<HashSet<ProvenanceKey>>,
    pending: Mutex<HashMap<ProvenanceKey, bool>>,
}

impl ProvenanceTracker {
    pub fn new(non_natural: Vec<ProvenanceKey>) -> Self {
        Self {
            non_natural: Mutex::new(non_natural.into_iter().collect()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn mark(&self, key: ProvenanceKey) {
        if let Ok(mut blocks) = self.non_natural.lock() {
            blocks.insert(key.clone());
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(key, true);
        }
    }

    pub fn take(&self, key: &ProvenanceKey) -> bool {
        let removed = self
            .non_natural
            .lock()
            .map(|mut blocks| blocks.remove(key))
            .unwrap_or(true);
        if removed && let Ok(mut pending) = self.pending.lock() {
            pending.insert(key.clone(), false);
        }
        removed
    }

    pub fn contains(&self, key: &ProvenanceKey) -> bool {
        self.non_natural
            .lock()
            .map(|blocks| blocks.contains(key))
            .unwrap_or(true)
    }

    pub fn drain_pending(&self) -> Vec<ProvenanceChange> {
        self.pending
            .lock()
            .map(|mut pending| {
                pending
                    .drain()
                    .map(|(key, non_natural)| ProvenanceChange { key, non_natural })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn requeue(&self, changes: Vec<ProvenanceChange>) {
        if let Ok(mut pending) = self.pending.lock() {
            for change in changes {
                pending.entry(change.key).or_insert(change.non_natural);
            }
        }
    }
}

/// Persist queued provenance changes, re-queueing them on failure so a
/// transient database error cannot silently lose placement data.
pub(crate) fn flush_provenance(tracker: &ProvenanceTracker, db: &MmoDatabase) {
    let changes = tracker.drain_pending();
    if changes.is_empty() {
        return;
    }
    if let Err(error) = db.apply_provenance_changes(changes.clone()) {
        tracker.requeue(changes);
        log::warn!("[Cabbage MMO] failed to queue block provenance changes: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: i32) -> ProvenanceKey {
        ProvenanceKey {
            world: "world".to_string(),
            dimension: "minecraft:overworld".to_string(),
            x,
            y: 12,
            z: 3,
        }
    }

    #[test]
    fn latest_change_wins_before_flush() {
        let tracker = ProvenanceTracker::new(Vec::new());
        tracker.mark(key(1));
        assert!(tracker.take(&key(1)));
        let changes = tracker.drain_pending();
        assert_eq!(changes.len(), 1);
        assert!(!changes[0].non_natural);
    }
}
