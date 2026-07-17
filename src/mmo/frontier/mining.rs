//! Mining skill: XP attribution for ore breaks.
//!
//! XP rule: one primary skill (Mining) per ore block break, awarded only for
//! blocks with a configured reward. Ore-reveal eligibility and player-placed
//! host exclusion live in `ore_reveal/`; this handler never fires for blocks
//! without a configured reward, so placed blocks cannot farm progression.

use pumpkin::plugin::api::events::block::block_break::BlockBreakEvent;

use super::super::{
    MmoState,
    progression::{self, XpSource},
    skills::SkillId,
};

/// Award Mining XP when a broken block has a configured reward.
pub async fn handle_block_break(state: &MmoState, event: &BlockBreakEvent) {
    let Some(player) = event.player.as_ref() else {
        return;
    };
    let Some(xp) = state.block_xp_reward(event.block.name) else {
        return;
    };
    progression::award_xp(state, player, SkillId::Mining, xp, XpSource::BlockBreak).await;
}
