use std::sync::Arc;

use pumpkin::{
    entity::player::Player,
    plugin::api::events::{
        block::block_break::BlockBreakEvent, entity::entity_death::EntityDeathEvent,
    },
    server::Server,
};
use pumpkin_util::text::{TextComponent, color::NamedColor};

use super::{config::LevelCurve, db::MmoDatabase, skills::SkillId};

/// Award mining XP when a player breaks an ore block.
pub async fn handle_block_break(
    db: Arc<MmoDatabase>,
    message_on_level_up: bool,
    curve: LevelCurve,
    event: &BlockBreakEvent,
) {
    let Some(player) = event.player.as_ref() else {
        return;
    };

    let block_name = event.block.name;
    if !block_name.ends_with("_ore") {
        return;
    }

    let Some(xp) = db.get_ore_xp(block_name.to_string()).await.ok().flatten() else {
        return;
    };

    let uuid = player.gameprofile.id;
    let skill = SkillId::Mining;

    match db.add_xp(uuid, skill, xp, curve).await {
        Ok(result) => {
            if result.leveled_up && message_on_level_up {
                let message = TextComponent::text("Your Mining skill is now ")
                    .add_child(
                        TextComponent::text(format!("Level {}", result.new_level))
                            .color_named(NamedColor::Green),
                    )
                    .add_text("!");
                player.send_system_message(&message).await;
            }
        }
        Err(error) => {
            log::warn!("[Cabbage MMO] failed to award Mining XP to {uuid}: {error}");
        }
    }
}

/// Award combat XP when a player kills a mob.
pub async fn handle_entity_death(
    db: Arc<MmoDatabase>,
    message_on_level_up: bool,
    curve: LevelCurve,
    server: Arc<Server>,
    event: &EntityDeathEvent,
) {
    let Some(killer) = event.killer.as_ref() else {
        return;
    };

    let killer_entity = killer.get_entity();
    if killer_entity.entity_type != &pumpkin_data::entity::EntityType::PLAYER {
        return;
    }

    let killer_uuid = killer_entity.entity_uuid;
    let Some(player) = find_player_by_uuid(&server, killer_uuid) else {
        return;
    };

    let mob_name = event.entity.get_entity().entity_type.resource_name;
    let Some(xp) = db.get_mob_xp(mob_name.to_string()).await.ok().flatten() else {
        return;
    };

    let uuid = player.gameprofile.id;
    let skill = SkillId::Combat;

    match db.add_xp(uuid, skill, xp, curve).await {
        Ok(result) => {
            if result.leveled_up && message_on_level_up {
                let message = TextComponent::text("Your Combat skill is now ")
                    .add_child(
                        TextComponent::text(format!("Level {}", result.new_level))
                            .color_named(NamedColor::Green),
                    )
                    .add_text("!");
                player.send_system_message(&message).await;
            }
        }
        Err(error) => {
            log::warn!("[Cabbage MMO] failed to award Combat XP to {uuid}: {error}");
        }
    }
}

fn find_player_by_uuid(server: &Server, uuid: uuid::Uuid) -> Option<Arc<Player>> {
    for world in server.worlds.load().iter() {
        if let Some(player) = world.get_player_by_uuid(uuid) {
            return Some(player);
        }
    }
    None
}
