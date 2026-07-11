use std::sync::Arc;

use pumpkin::{
    entity::{
        Entity, EntityBase, player::Player, projectile::firework_rocket::FireworkRocketEntity,
    },
    plugin::api::events::{
        block::block_break::BlockBreakEvent, entity::entity_death::EntityDeathEvent,
        world::feature_generate::FeatureGenerateEvent,
    },
    server::Server,
};
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_util::text::{TextComponent, color::NamedColor};

use super::{MmoState, skills::SkillId};

/// Award mining XP when a broken block has a configured reward.
pub async fn handle_block_break(state: &MmoState, event: &BlockBreakEvent) {
    let Some(player) = event.player.as_ref() else {
        return;
    };

    let block_name = event.block.name;
    let current_tick = state.current_tick();

    let Some(xp) = state.block_xp_reward(block_name) else {
        return;
    };
    let db = state.db();

    let uuid = player.gameprofile.id;
    let skill = SkillId::Mining;
    let curve = state.curve(skill);

    match db.add_xp(uuid, skill, xp, curve).await {
        Ok(result) => {
            if result.leveled_up {
                if state.config().message_on_level_up {
                    let message = TextComponent::text("Your Mining skill is now ")
                        .add_child(
                            TextComponent::text(format!("Level {}", result.new_level))
                                .color_named(NamedColor::Green),
                        )
                        .add_text("!");
                    player.send_system_message(&message).await;
                }
                celebrate_level_up(player, result.new_level).await;
            }
            state.show_xp_bossbar(player, skill, current_tick).await;
        }
        Err(error) => {
            log::warn!("[Cabbage MMO] failed to award Mining XP to {uuid}: {error}");
        }
    }
}

/// Award combat XP when a player kills a mob.
pub async fn handle_entity_death(state: &MmoState, server: Arc<Server>, event: &EntityDeathEvent) {
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

    let current_tick = state.current_tick();

    let mob_name = event.entity.get_entity().entity_type.resource_name;
    let Some(xp) = state.mob_xp_reward(mob_name) else {
        return;
    };
    let db = state.db();

    let uuid = player.gameprofile.id;
    let skill = SkillId::Combat;
    let curve = state.curve(skill);

    match db.add_xp(uuid, skill, xp, curve).await {
        Ok(result) => {
            if result.leveled_up {
                if state.config().message_on_level_up {
                    let message = TextComponent::text("Your Combat skill is now ")
                        .add_child(
                            TextComponent::text(format!("Level {}", result.new_level))
                                .color_named(NamedColor::Green),
                        )
                        .add_text("!");
                    player.send_system_message(&message).await;
                }
                celebrate_level_up(&player, result.new_level).await;
            }
            state.show_xp_bossbar(&player, skill, current_tick).await;
        }
        Err(error) => {
            log::warn!("[Cabbage MMO] failed to award Combat XP to {uuid}: {error}");
        }
    }
}

/// Play an anvil sound at the player, and spawn fireworks when they hit a
/// multiple-of-10 level milestone.
async fn celebrate_level_up(player: &Arc<Player>, new_level: u32) {
    let world = player.get_entity().world.load_full();
    let pos = player.get_entity().pos.load();

    world.play_sound(Sound::BlockAnvilUse, SoundCategory::Players, &pos);

    if new_level % 10 == 0 {
        let rocket_entity = Entity::new(
            world.clone(),
            pos,
            &pumpkin_data::entity::EntityType::FIREWORK_ROCKET,
        );
        let rocket = FireworkRocketEntity::new(rocket_entity);
        let rocket_arc: Arc<dyn EntityBase> = Arc::new(rocket);
        world.spawn_entity(rocket_arc).await;
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

/// Cancel feature placement if the feature is in the MMO blacklist.
pub async fn handle_feature_generate(state: &MmoState, event: &mut FeatureGenerateEvent) {
    let feature_name = placed_feature_name(event.feature);
    if should_disable_world_feature(&state.config(), &feature_name) {
        event.cancelled = true;
    }
}

fn should_disable_world_feature(config: &super::config::MmoConfig, feature_name: &str) -> bool {
    // Existing config files already contain a serialized blacklist, so adding
    // emerald to the default alone would not migrate them. Keep emerald tied
    // to the reveal system even for those existing installations.
    (config.ore_reveal.enabled && feature_name == "ore_emerald")
        || config
            .disabled_world_features
            .iter()
            .any(|name| name == feature_name)
}

/// Convert a `PlacedFeature` enum variant to its snake_case registry name.
fn placed_feature_name(feature: pumpkin_data::placed_feature::PlacedFeature) -> String {
    let pascal = format!("{feature:?}");
    let mut snake = String::with_capacity(pascal.len());
    for (i, ch) in pascal.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(ch.to_ascii_lowercase());
    }
    snake
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emerald_worldgen_is_disabled_for_existing_configs() {
        let mut config = super::super::config::MmoConfig::default();
        config.disabled_world_features.clear();
        assert!(should_disable_world_feature(&config, "ore_emerald"));
        assert!(!should_disable_world_feature(&config, "ore_diamond"));
    }

    #[test]
    fn emerald_worldgen_can_follow_reveal_disable_switch() {
        let mut config = super::super::config::MmoConfig::default();
        config.disabled_world_features.clear();
        config.ore_reveal.enabled = false;
        assert!(!should_disable_world_feature(&config, "ore_emerald"));
    }
}
