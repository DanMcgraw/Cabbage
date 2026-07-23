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

use crate::MmoState;

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

    // Durability safety check: ensure player is holding a valid tool and cap
    // candidates so the tool never breaks mid-batch (which causes drop loss,
    // empty-hand breaks, tool animation spam, and client interaction freeze).
    let max_extra_blocks = {
        let held = player.inventory().held_item();
        let stack = held.lock().await;
        if stack.is_empty() || stack.item_count == 0 {
            return None;
        }
        if !stack.is_damageable() || stack.is_unbreakable() {
            usize::MAX
        } else {
            let max_damage = stack.get_max_damage().unwrap_or(0);
            let current_damage = stack.get_damage();
            if current_damage >= max_damage {
                return None;
            }
            (max_damage - current_damage) as usize
        }
    };

    // The origin block break will consume 1 durability when finished, so
    // extra batch candidates are capped to remaining_durability - 1.
    let max_extra_blocks = max_extra_blocks.saturating_sub(1);
    if max_extra_blocks == 0 {
        return None;
    }

    let mut candidates = collect_batch_candidates(origin, max_blocks, is_candidate);
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() > max_extra_blocks {
        candidates.truncate(max_extra_blocks);
    }

    // Charge the cooldown before breaking: this is the recursion guard (see
    // the doc comment above).
    if !state.perk_cooldowns().try_activate(
        player_uuid,
        cooldown_key,
        current_tick,
        config.perks.batch_break_cooldown_ticks,
    ) {
        return None;
    }

    // Yield execution micro-task to allow Pumpkin's FinishedDigging handler to finish
    // sending sequence acknowledgement and block state sync to the player client BEFORE
    // extra batch blocks break. This prevents candidate block changes/item pickups from
    // breaking the client's digging prediction sequence machine (which causes fast tool
    // animation loops and blocks inventory opening).
    tokio::task::yield_now().await;

    let mut broken_count = 0usize;
    let mut collected_drops: Vec<pumpkin_data::item_stack::ItemStack> = Vec::new();

    let is_creative = player.gamemode.load() == pumpkin_util::GameMode::Creative;
    let luck = player
        .living_entity
        .get_attribute_value(&pumpkin_data::attributes::Attributes::LUCK) as f32;
    let is_raining = world.is_raining().await;
    let is_thundering = world.is_thundering().await;
    let day_time = world.level_info.load().day_time as u64;
    let server = world.server.upgrade()?;

    for position in candidates {
        let is_tool_valid = {
            let held = player.inventory().held_item();
            let stack = held.lock().await;
            !stack.is_empty()
                && stack.item_count > 0
                && (stack.get_damage() < stack.get_max_damage().unwrap_or(i32::MAX))
        };
        if !is_tool_valid {
            break;
        }

        let (broken_block, broken_block_state) = world.get_block_and_state_id(&position);

        // Break block in world with SKIP_DROPS to avoid spawning individual item entities
        // per candidate block. Spawning N item entities within a single tick causes per-entity
        // collision triggers, which send N full CSetContainerContent (46-slot inventory)
        // packets to the client in sub-millisecond bursts, flooding the Netty connection and
        // kicking the player for a Network Error.
        if world
            .break_block(
                &position,
                Some(player.clone()),
                pumpkin_world::world::BlockFlags::SKIP_DROPS
                    | pumpkin_world::world::BlockFlags::NOTIFY_NEIGHBORS,
            )
            .await
            .is_some()
        {
            broken_count += 1;

            if !is_creative {
                let tool = {
                    let hand_stack = player
                        .inventory
                        .get_stack_in_hand(pumpkin_util::Hand::Right)
                        .await;
                    let guard = hand_stack.lock().await;
                    (guard.item_count > 0).then(|| guard.clone())
                };

                let params = pumpkin::world::loot::LootContextParameters {
                    block_state: Some(pumpkin_data::block_state::BlockState::from_id(
                        broken_block_state,
                    )),
                    luck,
                    position: Some(pumpkin_util::math::vector3::Vector3::new(
                        position.0.x as f64,
                        position.0.y as f64,
                        position.0.z as f64,
                    )),
                    world_time: day_time,
                    tool,
                    is_raining: Some(is_raining),
                    is_thundering: Some(is_thundering),
                    ..Default::default()
                };

                let raw_items = pumpkin::block::get_loot_items(broken_block, params);

                let drop_event =
                    pumpkin::plugin::api::events::block::block_drop_item::BlockDropItemEvent::new(
                        player.clone(),
                        broken_block,
                        position,
                        raw_items,
                    );

                let drop_event = server.plugin_manager.fire(drop_event).await;

                if !drop_event.cancelled {
                    collected_drops.extend(drop_event.items);
                    pumpkin::block::drop_experience(world, broken_block, &position).await;
                }
            }
        }
    }

    if broken_count == 0 {
        return None;
    }

    // Merge identical item stacks in collected_drops so they drop as clean, consolidated stacks.
    let mut merged_drops: Vec<pumpkin_data::item_stack::ItemStack> = Vec::new();
    for stack in collected_drops {
        if stack.is_empty() {
            continue;
        }
        let mut added = false;
        for existing in &mut merged_drops {
            if existing.are_items_and_components_equal(&stack) {
                existing.increment(stack.item_count);
                added = true;
                break;
            }
        }
        if !added {
            merged_drops.push(stack);
        }
    }

    // Drop consolidated item stacks at the origin block position.
    for stack in merged_drops {
        world.drop_stack(&origin, stack).await;
    }

    // Apply tool durability damage ONCE for all extra blocks broken in the batch transaction.
    player.damage_held_item(broken_count as i32).await;

    state.audit(&format!(
        "batch break: {cooldown_key} broke {broken_count} block(s) for {player_uuid}",
    ));
    Some(broken_count)
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
