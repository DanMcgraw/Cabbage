use crate::mob_ai::MobAiState;
use crate::mob_ai::pathfinding::{
    BlockGrid, PathBounds, PlayerSearchTree, bidirectional_a_star, connect_to_player_tree,
    movement_path_steps,
};
use pumpkin_util::math::position::BlockPos;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct ActiveJobGuard {
    pub uuid: Uuid,
    pub set: Arc<Mutex<HashSet<Uuid>>>,
}

impl Drop for ActiveJobGuard {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.uuid);
    }
}

pub fn cabbage_worker_thread_count() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);

    cabbage_worker_thread_count_for(Some(cores))
}

fn cabbage_worker_thread_count_for(logical_cores: Option<usize>) -> usize {
    let cores = logical_cores.unwrap_or(1);
    if cores <= 2 { 1 } else { cores - 1 }
}

impl MobAiState {
    pub fn spawn_path_job(
        &self,
        uuid: Uuid,
        mob_pos: BlockPos,
        target_pos: BlockPos,
        player_tree: Option<Arc<PlayerSearchTree>>,
    ) {
        // Record pathendpoints immediately on main thread to prevent spawning duplicate jobs
        self.path_endpoints
            .lock()
            .unwrap()
            .insert(uuid, (mob_pos, target_pos));

        let bounds = PathBounds::between(mob_pos, target_pos, super::PATH_BOX_OUTSET_BLOCKS);

        let active_path_jobs = Arc::clone(&self.active_path_jobs);
        let path_steps = Arc::clone(&self.path_steps);
        let paths_completed = Arc::clone(&self.paths_completed);
        let chunk_registry = self.chunk_registry_read.clone();

        self.worker_pool.spawn(move || {
            let _guard = ActiveJobGuard {
                uuid,
                set: active_path_jobs,
            };

            log::trace!(
                "path job uuid={} thread={:?}",
                uuid,
                std::thread::current().name()
            );

            // Sample using only chunk_registry, completely lock-free and Arc<World>-free!
            let grid_opt = BlockGrid::sample_registry(bounds, &chunk_registry);
            println!(
                "[Cabbage Debug] workers::spawn_path_job: sampling bounds={:?} grid_ok={}",
                bounds,
                grid_opt.is_some()
            );
            let Some(grid) = grid_opt else {
                return;
            };

            let path = if let Some(player_tree) = player_tree {
                connect_to_player_tree(&grid, mob_pos, target_pos, &player_tree)
            } else {
                bidirectional_a_star(&grid, mob_pos, target_pos).map(|vec| VecDeque::from(vec))
            };

            println!(
                "[Cabbage Debug] workers::spawn_path_job: path found={}",
                path.is_some()
            );

            let mut path_steps = path_steps.lock().unwrap();
            if let Some(path) = path {
                let vec_path: Vec<BlockPos> = path.into();
                let steps = movement_path_steps(&vec_path, mob_pos);
                path_steps.insert(uuid, steps);
            } else {
                path_steps.remove(&uuid);
            }

            paths_completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_thread_count_uses_available_logical_cores() {
        let available = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1);

        assert_eq!(cabbage_worker_thread_count_for(None), 1);
        assert_eq!(cabbage_worker_thread_count_for(Some(0)), 1);
        assert_eq!(cabbage_worker_thread_count_for(Some(4)), 3);
        assert!(cabbage_worker_thread_count() <= available);
    }
}
