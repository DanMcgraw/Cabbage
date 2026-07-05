use crate::mob_ai::MobAiState;
use crate::mob_ai::movement::compute_velocity_plan;
use crate::mob_ai::pathfinding::{
    BlockGrid, PlayerSearchTree, bidirectional_a_star, connect_to_player_tree, movement_path_steps,
};
use crate::mob_ai::types::{ActiveMobSnapshot, VelocityJobSnapshot};
use pumpkin_util::math::position::BlockPos;
use std::collections::HashSet;
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
    cabbage_worker_thread_count_for(
        std::thread::available_parallelism()
            .map(|count| count.get())
            .ok(),
    )
}

pub const fn cabbage_worker_thread_count_for(available_parallelism: Option<usize>) -> usize {
    match available_parallelism {
        Some(count) if count > 0 => count,
        _ => 1,
    }
}

impl MobAiState {
    pub fn spawn_path_job(
        &self,
        uuid: Uuid,
        grid: BlockGrid,
        mob_pos: BlockPos,
        target_pos: BlockPos,
        player_tree: Option<Arc<PlayerSearchTree>>,
    ) {
        // Record the endpoints on the game thread so the reuse check on the
        // next cycle can compare them without waiting for the worker to finish.
        self.path_endpoints
            .lock()
            .unwrap()
            .insert(uuid, (mob_pos, target_pos));

        let active_path_jobs = Arc::clone(&self.active_path_jobs);
        let path_steps = Arc::clone(&self.path_steps);
        let paths_completed = Arc::clone(&self.paths_completed);

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

            let steps = if let Some(ref tree) = player_tree {
                connect_to_player_tree(&grid, mob_pos, target_pos, tree)
            } else {
                bidirectional_a_star(&grid, mob_pos, target_pos)
                    .map(|path| movement_path_steps(&path, mob_pos))
                    .filter(|steps| !steps.is_empty())
            };

            let mut path_steps = path_steps.lock().unwrap();
            if let Some(steps) = steps {
                path_steps.insert(uuid, steps);
            } else {
                path_steps.remove(&uuid);
            }

            paths_completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    }

    pub fn spawn_velocity_jobs(&self, active_mobs: &[ActiveMobSnapshot]) {
        let location_table = Arc::new(self.mob_locations.lock().unwrap().clone());
        let velocities_completed = Arc::clone(&self.velocities_completed);

        for mob in active_mobs {
            {
                let mut active_velocity_jobs = self.active_velocity_jobs.lock().unwrap();
                if !active_velocity_jobs.insert(mob.uuid) {
                    continue;
                }
            }

            let job = VelocityJobSnapshot {
                uuid: mob.uuid,
                world_uuid: mob.world_uuid,
                current_pos: mob.current_pos,
                current_block: mob.current_block,
                current_velocity: mob.current_velocity,
                movement_speed: mob.movement_speed,
                path_target: mob.path_target,
                location_table: Arc::clone(&location_table),
            };
            let active_velocity_jobs = Arc::clone(&self.active_velocity_jobs);
            let planned_velocities = Arc::clone(&self.planned_velocities);
            let velocities_completed = Arc::clone(&velocities_completed);

            self.worker_pool.spawn(move || {
                let _guard = ActiveJobGuard {
                    uuid: job.uuid,
                    set: active_velocity_jobs,
                };

                log::trace!(
                    "velocity job uuid={} thread={:?}",
                    job.uuid,
                    std::thread::current().name()
                );

                if let Some(plan) = compute_velocity_plan(&job) {
                    planned_velocities.lock().unwrap().insert(job.uuid, plan);
                }

                velocities_completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
        }
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
        assert_eq!(cabbage_worker_thread_count_for(Some(4)), 4);
        assert!(cabbage_worker_thread_count() <= available);
    }
}
