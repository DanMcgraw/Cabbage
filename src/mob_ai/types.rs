use pumpkin_util::math::vector3::Vector3;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

pub struct ChunkPassability {
    // 4 sections covering y = 32 to 96.
    // Each section is lazily initialized with a Box of 64 AtomicU64 (4096 bits)
    pub sections: [OnceLock<Box<[AtomicU64; 64]>>; 4],
    pub chunk_data: Option<Arc<pumpkin_world::chunk::ChunkData>>,
}

impl Default for ChunkPassability {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ChunkPassability {
    pub fn new(chunk_data: Option<Arc<pumpkin_world::chunk::ChunkData>>) -> Self {
        const INIT_ONCE_LOCK: OnceLock<Box<[AtomicU64; 64]>> = OnceLock::new();
        Self {
            sections: [INIT_ONCE_LOCK; 4],
            chunk_data,
        }
    }

    pub fn ensure_section(&self, section_idx: usize) {
        if section_idx >= 4 {
            return;
        }
        if self.sections[section_idx].get().is_none() {
            let mut bits = Box::new([const { std::sync::atomic::AtomicU64::new(0) }; 64]);
            if let Some(ref chunk_data) = self.chunk_data {
                let block_sections = chunk_data.section.block_sections.read().unwrap();
                // y = 32 maps to chunk section index 6 (since y = -64 is chunk section 0)
                let chunk_section_y = section_idx + 6;
                if let Some(palette) = block_sections.get(chunk_section_y) {
                    if !palette.has_only_air() {
                        for dy in 0..16 {
                            for dz in 0..16 {
                                for dx in 0..16 {
                                    let block_state_id = palette.get(dx, dy, dz);
                                    if pumpkin_data::BlockState::from_id(block_state_id)
                                        .is_solid_block()
                                    {
                                        let index = (dy << 8) | (dz << 4) | dx;
                                        let word_idx = index >> 6;
                                        let bit_idx = index & 63;
                                        *bits[word_idx].get_mut() |= 1u64 << bit_idx;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = self.sections[section_idx].set(bits);
        }
    }

    /// Returns `true` if the block at relative coordinate `(rx, y, rz)` is solid (not passable).
    /// If the section is not initialized, it returns `false` (passable/air by default).
    pub fn is_solid(&self, rx: usize, y: i32, rz: usize) -> bool {
        if y < 32 || y >= 96 {
            return false;
        }
        let section_idx = ((y - 32) >> 4) as usize;
        self.ensure_section(section_idx);
        let local_y = ((y - 32) & 15) as usize;
        if let Some(section) = self.sections[section_idx].get() {
            let index = (local_y << 8) | (rz << 4) | rx;
            let word_idx = index >> 6;
            let bit_idx = index & 63;
            let mask = 1u64 << bit_idx;
            (section[word_idx].load(Ordering::Relaxed) & mask) != 0
        } else {
            false
        }
    }

    /// Sets the block at relative coordinate `(rx, y, rz)` as solid or passable.
    pub fn set_solid(&self, rx: usize, y: i32, rz: usize, solid: bool) {
        if y < 32 || y >= 96 {
            return;
        }
        let section_idx = ((y - 32) >> 4) as usize;
        self.ensure_section(section_idx);
        let local_y = ((y - 32) & 15) as usize;
        if let Some(section) = self.sections[section_idx].get() {
            let index = (local_y << 8) | (rz << 4) | rx;
            let word_idx = index >> 6;
            let bit_idx = index & 63;
            let mask = 1u64 << bit_idx;
            if solid {
                section[word_idx].fetch_or(mask, Ordering::Relaxed);
            } else {
                section[word_idx].fetch_and(!mask, Ordering::Relaxed);
            }
        }
    }
}

pub type ChunkRegistryRead = flashmap::ReadHandle<(i32, i32), Arc<ChunkPassability>>;
pub type ChunkRegistryWrite = flashmap::WriteHandle<(i32, i32), Arc<ChunkPassability>>;

#[derive(Clone)]
pub struct ActiveMobSnapshot {
    pub uuid: Uuid,
    pub world_uuid: Uuid,
    pub current_pos: Vector3<f64>,
    pub movement_speed: f64,
}

#[derive(Clone, Copy)]
pub struct MobLocationEntry {
    pub uuid: Uuid,
    pub world_uuid: Uuid,
    pub pos: Vector3<f64>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClusterCell {
    pub world_uuid: Uuid,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Default)]
pub struct MobLocationTable {
    pub entries: HashMap<Uuid, MobLocationEntry>,
    pub cells: HashMap<ClusterCell, Vec<Uuid>>,
}
