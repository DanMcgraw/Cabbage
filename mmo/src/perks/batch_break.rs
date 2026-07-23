//! Shared batch-break helper for Timber, Vein Miner, and Earthmover.
//!
//! Candidate discovery happens during the origin `BlockBreakEvent`, but the
//! extra blocks are not changed there. The request is committed by the
//! origin's `BlockBrokenEvent` and executed on a later server tick while the
//! player is not actively mining. This keeps Java's digging sequence,
//! origin-block acknowledgement, and predicted held-tool update out of the
//! server-side batch operation. Only the origin uses Pumpkin's player-caused
//! break pipeline; connected blocks are direct world mutations whose drops,
//! vanilla experience, skill experience, and durability are aggregated.

use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex, Weak, atomic::Ordering},
};

use pumpkin::{
    entity::{EntityBase, player::Player},
    world::World,
};
use pumpkin_data::{Block, BlockStateId, data_component_impl::ToolImpl, item_stack::ItemStack};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_world::inventory::Inventory;
use uuid::Uuid;

use crate::{
    MmoState,
    progression::{self, XpSource},
    skills::SkillId,
};

/// Do not retain an uncommitted or continually-busy action indefinitely.
const MAX_PENDING_AGE_TICKS: i32 = 40;
/// Bound work shared by all players in one server tick.
const MAX_BATCHES_PER_TICK: usize = 4;
/// Bound quiet slot synchronizations in one server tick.
const MAX_SLOT_SYNCS_PER_TICK: usize = 16;

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

struct PendingBatchBreak {
    world: Arc<World>,
    player: Weak<Player>,
    player_uuid: Uuid,
    origin: BlockPos,
    target: &'static Block,
    candidates: Vec<(BlockPos, BlockStateId)>,
    selected_slot: u8,
    tool_item_id: u16,
    damage_per_block: i32,
    cooldown_key: &'static str,
    queued_at_tick: i32,
    ready_at_tick: Option<i32>,
    origin_drops: Option<Vec<ItemStack>>,
}

struct PendingSlotSync {
    player: Weak<Player>,
    player_uuid: Uuid,
    slot: u8,
    queued_at_tick: i32,
    ready_at_tick: i32,
}

/// Short-lived queues that move server-authored batch work out of the
/// originating Java digging packet's call stack.
pub(crate) struct BatchBreakState {
    pending: Mutex<VecDeque<PendingBatchBreak>>,
    slot_syncs: Mutex<VecDeque<PendingSlotSync>>,
}

impl BatchBreakState {
    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            slot_syncs: Mutex::new(VecDeque::new()),
        }
    }

    fn schedule(&self, action: PendingBatchBreak) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push_back(action);
        }
    }

    /// Snapshot the origin's finalized drop list after Cabbage's drop perks
    /// have run. The backend portion repeats this exact result instead of
    /// executing loot tables and drop events for every connected block.
    pub(crate) fn capture_origin_drops(
        &self,
        world: &Arc<World>,
        player_uuid: Uuid,
        origin: BlockPos,
        items: &[ItemStack],
    ) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        if let Some(action) = pending.iter_mut().find(|action| {
            action.player_uuid == player_uuid
                && action.origin == origin
                && Arc::ptr_eq(&action.world, world)
        }) {
            action.origin_drops = Some(items.to_vec());
        }
    }

    /// Commit the request only after Pumpkin confirms that the player broke
    /// the origin block. Execution begins no earlier than the following tick,
    /// after the origin event path has had a chance to return to Java packet
    /// processing.
    pub(crate) fn mark_origin_broken(
        &self,
        world: &Arc<World>,
        player_uuid: Uuid,
        origin: BlockPos,
        current_tick: i32,
    ) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        if let Some(action) = pending.iter_mut().find(|action| {
            action.player_uuid == player_uuid
                && action.origin == origin
                && Arc::ptr_eq(&action.world, world)
        }) {
            action.ready_at_tick = Some(current_tick.saturating_add(1));
        }
    }

    fn take_ready(&self, current_tick: i32) -> Vec<PendingBatchBreak> {
        let Ok(mut pending) = self.pending.lock() else {
            return Vec::new();
        };
        let mut ready = Vec::new();
        let queued = pending.len();
        for _ in 0..queued {
            let Some(action) = pending.pop_front() else {
                break;
            };
            if is_expired(action.queued_at_tick, current_tick) {
                continue;
            }
            if ready.len() < MAX_BATCHES_PER_TICK
                && action
                    .ready_at_tick
                    .is_some_and(|ready_at| current_tick >= ready_at)
            {
                ready.push(action);
            } else {
                pending.push_back(action);
            }
        }
        ready
    }

    fn defer(&self, action: PendingBatchBreak) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push_back(action);
        }
    }

    fn schedule_slot_sync(&self, player: &Arc<Player>, slot: u8, current_tick: i32) {
        let player_uuid = player.gameprofile.id;
        let Ok(mut syncs) = self.slot_syncs.lock() else {
            return;
        };
        // Only the newest authoritative snapshot for a slot matters.
        syncs.retain(|sync| sync.player_uuid != player_uuid || sync.slot != slot);
        syncs.push_back(PendingSlotSync {
            player: Arc::downgrade(player),
            player_uuid,
            slot,
            queued_at_tick: current_tick,
            ready_at_tick: current_tick.saturating_add(1),
        });
    }

    fn take_ready_slot_syncs(&self, current_tick: i32) -> Vec<PendingSlotSync> {
        let Ok(mut syncs) = self.slot_syncs.lock() else {
            return Vec::new();
        };
        let mut ready = Vec::new();
        let queued = syncs.len();
        for _ in 0..queued {
            let Some(sync) = syncs.pop_front() else {
                break;
            };
            if is_expired(sync.queued_at_tick, current_tick) {
                continue;
            }
            if ready.len() < MAX_SLOT_SYNCS_PER_TICK && current_tick >= sync.ready_at_tick {
                ready.push(sync);
            } else {
                syncs.push_back(sync);
            }
        }
        ready
    }

    fn defer_slot_sync(&self, sync: PendingSlotSync) {
        if let Ok(mut syncs) = self.slot_syncs.lock() {
            syncs.push_back(sync);
        }
    }

    pub(crate) fn clear(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        if let Ok(mut syncs) = self.slot_syncs.lock() {
            syncs.clear();
        }
    }
}

fn is_expired(queued_at_tick: i32, current_tick: i32) -> bool {
    current_tick.saturating_sub(queued_at_tick) > MAX_PENDING_AGE_TICKS
}

/// Queue a batch break from `origin`.
///
/// Returns the number of extra blocks scheduled, or `None` when the perk did
/// not fire (global perk switch off, on cooldown, no candidates, or no usable
/// held tool). `max_blocks` is additionally clamped by the global batch cap.
///
/// The cooldown is charged before the request is queued and prevents another
/// origin action until this backend batch has settled.
pub(crate) async fn queue_batch_break(
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
    let target = world.get_block(&origin);
    if state
        .perk_cooldowns()
        .remaining_ticks(player_uuid, cooldown_key, current_tick)
        > 0
    {
        return None;
    }

    // Capture the originating slot and item. The deferred action is discarded
    // if the player changes either before it starts.
    let (selected_slot, tool_item_id, damage_per_block, max_extra_blocks) = {
        let selected_slot = player.inventory().get_selected_slot();
        let held = player.inventory().held_item();
        let stack = held.lock().await;
        if stack.is_empty() || stack.item_count == 0 {
            return None;
        }
        let damage_per_block = stack.get_data_component::<ToolImpl>().map_or(0, |tool| {
            i32::try_from(tool.damage_per_block).unwrap_or(i32::MAX)
        });
        let remaining_blocks =
            if !stack.is_damageable() || stack.is_unbreakable() || damage_per_block <= 0 {
                usize::MAX
            } else {
                let max_damage = stack.get_max_damage().unwrap_or(0);
                let remaining_damage = max_damage.saturating_sub(stack.get_damage());
                usize::try_from(remaining_damage / damage_per_block).unwrap_or(0)
            };
        (
            selected_slot,
            stack.item.id,
            damage_per_block,
            remaining_blocks.saturating_sub(1),
        )
    };
    if max_extra_blocks == 0 {
        return None;
    }

    let mut candidates = collect_batch_candidates(origin, max_blocks, is_candidate)
        .into_iter()
        .filter_map(|position| {
            world
                .get_block_state_id_if_loaded(&position)
                .map(|state_id| (position, state_id))
        })
        .collect::<Vec<_>>();
    candidates.truncate(max_extra_blocks);
    if candidates.is_empty() {
        return None;
    }

    if !state.perk_cooldowns().try_activate(
        player_uuid,
        cooldown_key,
        current_tick,
        config.perks.batch_break_cooldown_ticks,
    ) {
        return None;
    }

    let scheduled = candidates.len();
    state.batch_breaks().schedule(PendingBatchBreak {
        world: world.clone(),
        player: Arc::downgrade(player),
        player_uuid,
        origin,
        target,
        candidates,
        selected_slot,
        tool_item_id,
        damage_per_block,
        cooldown_key,
        queued_at_tick: current_tick,
        ready_at_tick: None,
        origin_drops: None,
    });
    Some(scheduled)
}

/// Execute committed actions and quiet inventory synchronizations for this
/// server tick.
pub(crate) async fn process_pending(state: &MmoState, current_tick: i32) {
    for sync in state.batch_breaks().take_ready_slot_syncs(current_tick) {
        let Some(player) = sync.player.upgrade() else {
            continue;
        };
        if !player.has_client_loaded() {
            continue;
        }
        if player.mining.load(Ordering::Relaxed) {
            state.batch_breaks().defer_slot_sync(sync);
            continue;
        }
        let stack = player.inventory().get_stack(sync.slot as usize).await;
        let stack = stack.lock().await.clone();
        player.sync_hand_slot(sync.slot as usize, stack).await;
    }

    for action in state.batch_breaks().take_ready(current_tick) {
        let Some(player) = action.player.upgrade() else {
            continue;
        };
        if !player.has_client_loaded() {
            continue;
        }
        if player.mining.load(Ordering::Relaxed) {
            state.batch_breaks().defer(action);
            continue;
        }
        let player_world = player.get_entity().world.load_full();
        if !Arc::ptr_eq(&action.world, &player_world)
            || player.inventory().get_selected_slot() != action.selected_slot
        {
            continue;
        }
        let held = player.inventory().held_item();
        if held.lock().await.item.id != action.tool_item_id {
            continue;
        }
        // A matching origin here means the committed break was superseded or
        // rolled back. Never remove the connected blocks in that case.
        if action.world.get_block(&action.origin) == action.target {
            continue;
        }
        execute_batch(state, action, &player, current_tick).await;
    }
}

async fn execute_batch(
    state: &MmoState,
    mut action: PendingBatchBreak,
    player: &Arc<Player>,
    current_tick: i32,
) {
    // Re-cap against durability after Pumpkin has charged the origin block.
    if action.damage_per_block > 0 {
        let held = player.inventory().held_item();
        let stack = held.lock().await;
        if stack.is_damageable() && !stack.is_unbreakable() {
            let remaining_damage = stack
                .get_max_damage()
                .unwrap_or(0)
                .saturating_sub(stack.get_damage());
            let remaining_blocks =
                usize::try_from(remaining_damage / action.damage_per_block).unwrap_or(0);
            action.candidates.truncate(remaining_blocks);
        }
    }
    if action.candidates.is_empty() {
        return;
    }

    let is_creative = player.gamemode.load() == pumpkin_util::GameMode::Creative;
    let mut broken_count = 0usize;

    for (position, expected_state_id) in &action.candidates {
        if player.mining.load(Ordering::Relaxed)
            || player.inventory().get_selected_slot() != action.selected_slot
        {
            break;
        }
        let held = player.inventory().held_item();
        let stack = held.lock().await;
        let tool_matches =
            !stack.is_empty() && stack.item_count > 0 && stack.item.id == action.tool_item_id;
        drop(stack);
        if !tool_matches {
            break;
        }

        let (broken_block, broken_block_state) = action.world.get_block_and_state_id(position);
        if broken_block != action.target || broken_block_state != *expected_state_id {
            continue;
        }

        let replaced_state_id = action
            .world
            .set_block_state(
                position,
                BlockStateId::AIR,
                pumpkin_world::world::BlockFlags::NOTIFY_ALL
                    | pumpkin_world::world::BlockFlags::SKIP_DROPS,
            )
            .await;
        if replaced_state_id != *expected_state_id {
            continue;
        }
        broken_count += 1;
    }

    if broken_count == 0 {
        return;
    }

    if !is_creative {
        // Pumpkin already spawned the origin's drops and vanilla XP. Repeat
        // the finalized origin drop snapshot and one representative vanilla
        // XP roll for every backend-removed block.
        if let Some(origin_drops) = action.origin_drops.as_deref() {
            for stack in multiply_drops(origin_drops, broken_count) {
                action.world.drop_stack(&action.origin, stack).await;
            }
            let extra_experience = roll_block_experience(action.target)
                .saturating_mul(u32::try_from(broken_count).unwrap_or(u32::MAX));
            if extra_experience > 0 {
                pumpkin::entity::experience_orb::ExperienceOrbEntity::spawn(
                    &action.world,
                    action.origin.to_f64(),
                    extra_experience,
                )
                .await;
            }
        }

        award_extra_skill_xp(state, player, &action, broken_count).await;
    }

    if !is_creative && action.damage_per_block > 0 {
        let still_holding_tool = player.inventory().get_selected_slot() == action.selected_slot
            && player.inventory().held_item().lock().await.item.id == action.tool_item_id;
        if still_holding_tool {
            let damage = i32::try_from(broken_count)
                .unwrap_or(i32::MAX)
                .saturating_mul(action.damage_per_block);
            let before_count = player.inventory().held_item().lock().await.item_count;
            if player.damage_held_item(damage).await {
                let final_stack = player.inventory().held_item().lock().await.clone();
                // A broken stack is already synchronized by Pumpkin. Normal
                // durability changes are sent once on a later quiet tick.
                if !final_stack.is_empty() && final_stack.item_count == before_count {
                    state.batch_breaks().schedule_slot_sync(
                        player,
                        action.selected_slot,
                        current_tick,
                    );
                }
            }
        } else {
            log::warn!(
                "[Cabbage MMO] skipped deferred durability for {} because the held tool changed",
                action.player_uuid
            );
        }
    }

    state.audit(&format!(
        "backend batch break: {} removed {broken_count} extra block(s) for {}",
        action.cooldown_key, action.player_uuid
    ));
}

fn multiply_drops(drops: &[ItemStack], multiplier: usize) -> Vec<ItemStack> {
    let mut repeated = Vec::with_capacity(drops.len().saturating_mul(multiplier));
    for _ in 0..multiplier {
        repeated.extend(drops.iter().cloned());
    }
    merge_drops(repeated)
}

fn roll_block_experience(block: &Block) -> u32 {
    let Some(experience) = &block.experience else {
        return 0;
    };
    let mut random = pumpkin_util::random::RandomGenerator::Xoroshiro(
        pumpkin_util::random::xoroshiro128::Xoroshiro::from_seed(pumpkin_util::random::get_seed()),
    );
    u32::try_from(experience.experience.get(&mut random)).unwrap_or(0)
}

async fn award_extra_skill_xp(
    state: &MmoState,
    player: &Arc<Player>,
    action: &PendingBatchBreak,
    broken_count: usize,
) {
    if !progression::earns_xp(player) {
        return;
    }
    let reward = {
        let config = state.config();
        match action.cooldown_key {
            "mining.vein_miner" => state
                .block_xp_reward(action.target.name)
                .map(|xp| (SkillId::Mining, xp)),
            "woodcutting.timber" => config
                .frontier
                .woodcutting
                .log_xp
                .get(action.target.name)
                .copied()
                .map(|xp| (SkillId::Woodcutting, xp)),
            "excavation.earthmover" => config
                .frontier
                .excavation
                .diggable_xp
                .get(action.target.name)
                .copied()
                .map(|xp| (SkillId::Excavation, xp)),
            _ => None,
        }
    };
    let Some((skill, xp_per_block)) = reward else {
        return;
    };
    let total_xp = xp_per_block.saturating_mul(u64::try_from(broken_count).unwrap_or(u64::MAX));
    progression::award_xp(state, player, skill, total_xp, XpSource::BlockBreak).await;
}

fn merge_drops(drops: Vec<ItemStack>) -> Vec<ItemStack> {
    let mut merged = Vec::<ItemStack>::new();
    for mut stack in drops {
        if stack.is_empty() {
            continue;
        }
        while stack.item_count > 0 {
            if let Some(existing) = merged.iter_mut().find(|existing| {
                existing.are_items_and_components_equal(&stack)
                    && existing.item_count < existing.get_max_stack_size()
            }) {
                let moved = stack
                    .item_count
                    .min(existing.get_max_stack_size() - existing.item_count);
                existing.increment(moved);
                stack.decrement(moved);
                continue;
            }

            let moved = stack.item_count.min(stack.get_max_stack_size());
            let mut split = stack.clone();
            split.item_count = moved;
            merged.push(split);
            stack.decrement(moved);
        }
    }
    merged
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

    #[test]
    fn pending_actions_expire_after_bounded_wait() {
        assert!(!is_expired(100, 140));
        assert!(is_expired(100, 141));
    }

    #[test]
    fn identical_drops_are_consolidated() {
        let drops = vec![
            ItemStack::new(2, &pumpkin_data::item::Item::COBBLESTONE),
            ItemStack::new(3, &pumpkin_data::item::Item::COBBLESTONE),
        ];
        let merged = merge_drops(drops);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].item_count, 5);
    }

    #[test]
    fn consolidated_drops_respect_the_item_stack_limit() {
        let drops = vec![
            ItemStack::new(40, &pumpkin_data::item::Item::COBBLESTONE),
            ItemStack::new(40, &pumpkin_data::item::Item::COBBLESTONE),
        ];
        let merged = merge_drops(drops);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].item_count, 64);
        assert_eq!(merged[1].item_count, 16);
    }

    #[test]
    fn origin_drops_are_multiplied_for_backend_blocks() {
        let drops = vec![ItemStack::new(3, &pumpkin_data::item::Item::COBBLESTONE)];
        let multiplied = multiply_drops(&drops, 4);
        assert_eq!(multiplied.len(), 1);
        assert_eq!(multiplied[0].item_count, 12);
    }

    #[test]
    fn multiplied_drops_respect_the_item_stack_limit() {
        let drops = vec![ItemStack::new(40, &pumpkin_data::item::Item::COBBLESTONE)];
        let multiplied = multiply_drops(&drops, 2);
        assert_eq!(multiplied.len(), 2);
        assert_eq!(multiplied[0].item_count, 64);
        assert_eq!(multiplied[1].item_count, 16);
    }

    #[test]
    fn zero_backend_blocks_multiply_to_no_drops() {
        let drops = vec![ItemStack::new(3, &pumpkin_data::item::Item::COBBLESTONE)];
        assert!(multiply_drops(&drops, 0).is_empty());
    }
}
