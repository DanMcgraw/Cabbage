//! Shared batch-break helper for Timber, Vein Miner, and Earthmover.
//!
//! All multi-break perks must go through `Context::break_blocks` (bounded,
//! deduplicated, protection- and durability-aware). The flow is: gate on the
//! global perk switch and the perk's cooldown, collect connected candidates,
//! break them in one transaction, and only then charge the cooldown — the
//! documented cost is paid once per completed action, never per attempt.

use std::{collections::HashSet, sync::Arc};

use pumpkin::{entity::player::Player, world::World};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use crate::mmo::MmoState;

/// Collect connected candidate positions for a batch break via BFS over the
/// 26-neighborhood, bounded to `max_blocks` unique positions. `origin` is
/// excluded (vanilla already broke it). Pure and unit-testable.
pub(crate) fn collect_batch_candidates(
    origin: BlockPos,
    max_blocks: usize,
    mut is_candidate: impl FnMut(BlockPos) -> bool,
) -> Vec<BlockPos> {
    let mut visited: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut result = Vec::new();
    if max_blocks == 0 {
        return result;
    }
    let mut queue = std::collections::VecDeque::new();
    visited.insert((origin.0.x, origin.0.y, origin.0.z));
    queue.push_back(origin);

    'outer: while let Some(current) = queue.pop_front() {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let next = BlockPos(Vector3::new(
                        current.0.x + dx,
                        current.0.y + dy,
                        current.0.z + dz,
                    ));
                    let key = (next.0.x, next.0.y, next.0.z);
                    if !visited.insert(key) {
                        continue;
                    }
                    if is_candidate(next) {
                        result.push(next);
                        queue.push_back(next);
                        if result.len() >= max_blocks {
                            break 'outer;
                        }
                    }
                }
            }
        }
    }
    result
}

/// Try to perform a batch break from `origin`.
///
/// Returns the number of extra blocks broken, or `None` when the perk did
/// not fire (global perk switch off, on cooldown, no candidates, or the
/// transaction failed). `max_blocks` is additionally clamped by the global
/// batch cap.
///
/// The cooldown is charged after candidates are found but *before* the
/// transaction runs: `Context::break_blocks` fires a fresh `BlockBreakEvent`
/// per broken block, and the active cooldown is what stops those events from
/// re-triggering the perk recursively. In the rare case the transaction then
/// breaks nothing, the cooldown is still consumed — acceptable, since the
/// documented cost is charged once per action, and it guarantees the
/// recursion guard can never be bypassed.
pub(crate) async fn try_batch_break(
    state: &MmoState,
    world: &Arc<World>,
    player: &Arc<Player>,
    origin: BlockPos,
    max_blocks: u32,
    cooldown_key: &'static str,
    is_candidate: impl FnMut(BlockPos) -> bool,
) -> Option<usize> {
    let config = state.config();
    if !config.perks.enabled {
        return None;
    }
    let max_blocks = max_blocks.min(config.perks.batch_break_max_blocks) as usize;
    if max_blocks == 0 {
        return None;
    }

    let player_uuid = player.gameprofile.id;
    let current_tick = state.current_tick();
    if state
        .perk_cooldowns()
        .remaining_ticks(player_uuid, cooldown_key, current_tick)
        > 0
    {
        return None;
    }

    let candidates = collect_batch_candidates(origin, max_blocks, is_candidate);
    if candidates.is_empty() {
        return None;
    }

    // Charge the cooldown before breaking: this is the recursion guard (see
    // the doc comment above).
    state.perk_cooldowns().try_activate(
        player_uuid,
        cooldown_key,
        current_tick,
        config.perks.batch_break_cooldown_ticks,
    );

    let broken = state
        .context()
        .break_blocks(world.clone(), player.clone(), candidates)
        .await
        .ok()?;
    if broken.is_empty() {
        return None;
    }
    Some(broken.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos(Vector3::new(x, y, z))
    }

    #[test]
    fn candidates_exclude_origin() {
        let result = collect_batch_candidates(pos(0, 0, 0), 10, |_| true);
        assert_eq!(result.len(), 10);
        assert!(!result.contains(&pos(0, 0, 0)));
    }

    #[test]
    fn candidates_respect_predicate() {
        // Only the two direct x-axis neighbors are candidates; expansion stops
        // there because no further position matches.
        let result = collect_batch_candidates(pos(0, 0, 0), 5, |p| {
            p.0.x.abs() == 1 && p.0.y == 0 && p.0.z == 0
        });
        assert_eq!(result.len(), 2);
        assert!(result.contains(&pos(1, 0, 0)));
        assert!(result.contains(&pos(-1, 0, 0)));
    }

    #[test]
    fn candidates_walk_diagonal_connections() {
        // A diagonal chain: (0,0,0) origin, (1,1,0), (2,2,0) are candidates.
        let result = collect_batch_candidates(pos(0, 0, 0), 10, |p| {
            (p.0.x == 1 && p.0.y == 1 && p.0.z == 0) || (p.0.x == 2 && p.0.y == 2 && p.0.z == 0)
        });
        assert_eq!(result, vec![pos(1, 1, 0), pos(2, 2, 0)]);
    }

    #[test]
    fn zero_cap_collects_nothing() {
        let result = collect_batch_candidates(pos(0, 0, 0), 0, |_| true);
        assert!(result.is_empty());
    }
}
