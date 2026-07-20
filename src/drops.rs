use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use pumpkin::{
    command::CommandSender,
    entity::{EntityBase, RemovalReason},
    plugin::{
        BoxFuture, Context, EventHandler, EventPriority,
        server::server_tick_start::ServerTickStartEvent,
    },
    server::Server,
};
use pumpkin_util::{math::vector2::Vector2, text::TextComponent};
use ruzstd::{
    decoding::StreamingDecoder,
    encoding::{CompressionLevel, compress_to_vec},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DROPPED_ITEM_ENTITY_ID: &str = "minecraft:item";

#[derive(Default)]
pub(crate) struct DroppedItemCleanupState {
    last_out_of_range_items: Mutex<HashSet<Uuid>>,
}

#[derive(Default)]
pub(crate) struct ClearDropsState {
    pub(crate) pending: AtomicBool,
    pub(crate) sender: Mutex<Option<CommandSender>>,
}

#[derive(Default)]
struct SavedDropCleanup {
    folders_scanned: usize,
    files_scanned: usize,
    files_changed: usize,
    chunks_scanned: usize,
    chunks_changed: usize,
    saved_removed: usize,
    errors: usize,
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct SavedPumpData {
    x: i32,
    z: i32,
    pub(crate) chunks: BTreeMap<String, Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SavedEntityChunkNbt {
    data_version: i32,
    position: [i32; 2],
    entities: Vec<pumpkin_nbt::NbtCompound>,
}

pub(crate) fn clear_drops_debug(message: impl AsRef<str>) {
    println!("[Cabbage] /cleardrops: {}", message.as_ref());
}

pub(crate) async fn register(
    context: &Arc<Context>,
    clear_drops_state: &Arc<ClearDropsState>,
    dropped_item_cleanup_state: &Arc<DroppedItemCleanupState>,
) {
    context
        .register_event::<ServerTickStartEvent, _>(
            clear_drops_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
    context
        .register_event::<ServerTickStartEvent, _>(
            dropped_item_cleanup_state.clone(),
            EventPriority::Normal,
            false,
        )
        .await;
}

impl EventHandler<ServerTickStartEvent> for DroppedItemCleanupState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.tick % 100 != 0 {
                return;
            }

            let last_out_of_range = {
                let last_items = self.last_out_of_range_items.lock().unwrap();
                last_items.clone()
            };
            let mut current_out_of_range = HashSet::new();

            for world in server.worlds.load().iter() {
                let mut watched_chunks = HashSet::<Vector2<i32>>::new();

                for player in world.players.load().iter() {
                    let center = player.get_entity().chunk_pos.load();

                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            watched_chunks.insert(Vector2::new(center.x + dx, center.y + dz));
                        }
                    }
                }

                let entities = world.entities.load();
                for entity_base in entities.iter() {
                    let entity = entity_base.get_entity();

                    if entity.entity_type.resource_name != "item"
                        && entity.entity_type.resource_name != "minecraft:item"
                    {
                        continue;
                    }

                    let chunk_pos = entity.chunk_pos.load();
                    if watched_chunks.contains(&chunk_pos) {
                        continue;
                    }

                    let uuid = entity.entity_uuid;
                    let was_out_of_range = last_out_of_range.contains(&uuid);

                    if was_out_of_range {
                        entity.removed.store(true, Ordering::Relaxed);
                        entity.removal_reason.store(Some(RemovalReason::Discarded));
                        entity.remove().await;
                    } else {
                        current_out_of_range.insert(uuid);
                    }
                }
            }

            {
                let mut last_items = self.last_out_of_range_items.lock().unwrap();
                *last_items = current_out_of_range;
            }
        })
    }
}

impl EventHandler<ServerTickStartEvent> for ClearDropsState {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        self.run_clear_on_tick(server, event.tick, "handle")
    }

    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        self.run_clear_on_tick(server, event.tick, "handle_blocking")
    }
}

impl ClearDropsState {
    fn run_clear_on_tick<'a>(
        &'a self,
        server: &'a Arc<Server>,
        tick: i32,
        handler: &'static str,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.pending.swap(false, Ordering::SeqCst) {
                return;
            }

            clear_drops_debug(format!("tick {tick}: running cleanup via {handler}"));

            let sender = self.sender.lock().ok().and_then(|mut sender| sender.take());
            let loaded_removed = clear_loaded_drops(server).await;
            let saved_summary = clear_saved_drops(server);

            clear_drops_debug(format!(
                "tick {tick}: cleanup finished; loaded_removed={loaded_removed}, saved_removed={}, saved_files_changed={}, saved_errors={}",
                saved_summary.saved_removed, saved_summary.files_changed, saved_summary.errors
            ));

            if let Some(sender) = sender {
                sender
                    .send_message(TextComponent::text(format!(
                        "Cleared {loaded_removed} loaded dropped item{} and {} saved dropped item{} from .pump files{}.",
                        if loaded_removed == 1 { "" } else { "s" },
                        saved_summary.saved_removed,
                        if saved_summary.saved_removed == 1 {
                            ""
                        } else {
                            "s"
                        },
                        if saved_summary.errors == 0 {
                            String::new()
                        } else {
                            format!(" ({} saved file scan error{})", saved_summary.errors, if saved_summary.errors == 1 { "" } else { "s" })
                        }
                    )))
                    .await;
            }
        })
    }
}

async fn clear_loaded_drops(server: &Server) -> usize {
    let mut drops = Vec::new();

    let worlds = server.worlds.load();
    clear_drops_debug(format!("scanning {} loaded world(s)", worlds.len()));

    for (world_index, world) in worlds.iter().enumerate() {
        let entities = world.entities.load();
        let before = drops.len();

        drops.extend(entities.iter().filter_map(|entity| {
            (entity.get_entity().entity_type.resource_name == "item").then(|| entity.clone())
        }));

        let world_drops = drops.len() - before;
        clear_drops_debug(format!(
            "world #{world_index}: entities={}, dropped_items={world_drops}",
            entities.len()
        ));
    }

    let removed = drops.len();
    clear_drops_debug(format!("removing {removed} dropped item entity/entities"));

    for entity in drops {
        let base_entity = entity.get_entity();
        base_entity.removed.store(true, Ordering::Relaxed);
        base_entity
            .removal_reason
            .store(Some(RemovalReason::Discarded));
        base_entity.remove().await;
    }

    removed
}

fn clear_saved_drops(server: &Server) -> SavedDropCleanup {
    let mut summary = SavedDropCleanup::default();
    let folders = saved_entity_folders(server);

    clear_drops_debug(format!(
        "scanning saved .pump entity files in {} folder(s)",
        folders.len()
    ));

    for folder in folders {
        summary.folders_scanned += 1;
        clear_drops_debug(format!("saved scan folder: {}", folder.display()));

        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                record_saved_scan_error(&mut summary, &folder, error);
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    record_saved_scan_error(&mut summary, &folder, error);
                    continue;
                }
            };

            let path = entry.path();
            if !is_pump_file(&path) {
                continue;
            }

            if let Err(error) = clear_saved_pump_file(&path, &mut summary) {
                record_saved_scan_error(&mut summary, &path, error);
            }
        }
    }

    clear_drops_debug(format!(
        "saved scan finished: folders={}, files={}, files_changed={}, chunks={}, chunks_changed={}, saved_removed={}, errors={}",
        summary.folders_scanned,
        summary.files_scanned,
        summary.files_changed,
        summary.chunks_scanned,
        summary.chunks_changed,
        summary.saved_removed,
        summary.errors
    ));

    summary
}

fn saved_entity_folders(server: &Server) -> Vec<PathBuf> {
    let mut folders = Vec::new();

    for world in server.worlds.load().iter() {
        let folder = world.level.level_folder.entities_folder.clone();
        if !folders.iter().any(|existing| existing == &folder) {
            folders.push(folder);
        }
    }

    folders
}

fn clear_saved_pump_file(path: &Path, summary: &mut SavedDropCleanup) -> Result<(), String> {
    summary.files_scanned += 1;

    let file_bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut pump_data: SavedPumpData = pumpkin_nbt::from_bytes_unnamed(Cursor::new(file_bytes))
        .map_err(|error| {
            format!(
                "failed to parse pump region NBT {}: {error}",
                path.display()
            )
        })?;

    let mut file_changed = false;
    let region_x = pump_data.x;
    let region_z = pump_data.z;

    for (chunk_key, compressed_chunk) in pump_data.chunks.iter_mut() {
        let chunk_index = match chunk_key.parse::<i32>() {
            Ok(index) if (0..1024).contains(&index) => index,
            _ => {
                summary.errors += 1;
                clear_drops_debug(format!(
                    "saved scan error in {}: invalid chunk key {chunk_key}",
                    path.display()
                ));
                continue;
            }
        };

        summary.chunks_scanned += 1;

        let mut decoder = match StreamingDecoder::new(&compressed_chunk[..]) {
            Ok(decoder) => decoder,
            Err(error) => {
                summary.errors += 1;
                clear_drops_debug(format!(
                    "saved scan error in {} chunk {chunk_key}: zstd decoder error: {error}",
                    path.display()
                ));
                continue;
            }
        };

        let mut decompressed = Vec::new();
        if let Err(error) = decoder.read_to_end(&mut decompressed) {
            summary.errors += 1;
            clear_drops_debug(format!(
                "saved scan error in {} chunk {chunk_key}: zstd read error: {error}",
                path.display()
            ));
            continue;
        }

        let mut chunk_nbt: SavedEntityChunkNbt = match pumpkin_nbt::from_bytes_unnamed(Cursor::new(
            decompressed,
        )) {
            Ok(chunk_nbt) => chunk_nbt,
            Err(error) => {
                summary.errors += 1;
                clear_drops_debug(format!(
                    "saved scan error in {} chunk {chunk_key}: entity chunk NBT parse error: {error}",
                    path.display()
                ));
                continue;
            }
        };

        let rel_x = chunk_index % 32;
        let rel_z = chunk_index / 32;
        let expected_position = [region_x * 32 + rel_x, region_z * 32 + rel_z];
        if chunk_nbt.position != expected_position {
            clear_drops_debug(format!(
                "saved scan warning in {} chunk {chunk_key}: expected chunk {},{} but NBT says {},{}",
                path.display(),
                expected_position[0],
                expected_position[1],
                chunk_nbt.position[0],
                chunk_nbt.position[1]
            ));
        }

        let before = chunk_nbt.entities.len();
        chunk_nbt
            .entities
            .retain(|entity| entity.get_string("id") != Some(DROPPED_ITEM_ENTITY_ID));
        let removed = before - chunk_nbt.entities.len();

        if removed == 0 {
            continue;
        }

        let mut serialized_chunk = Vec::new();
        pumpkin_nbt::to_bytes_unnamed(&chunk_nbt, &mut serialized_chunk).map_err(|error| {
            format!(
                "failed to serialize entity chunk {chunk_key} in {}: {error}",
                path.display()
            )
        })?;

        *compressed_chunk = compress_to_vec(&serialized_chunk[..], CompressionLevel::Fastest);
        file_changed = true;
        summary.chunks_changed += 1;
        summary.saved_removed += removed;
    }

    if file_changed {
        let mut serialized_file = Vec::new();
        pumpkin_nbt::to_bytes_unnamed(&pump_data, &mut serialized_file).map_err(|error| {
            format!("failed to serialize pump file {}: {error}", path.display())
        })?;
        fs::write(path, serialized_file)
            .map_err(|error| format!("failed to write pump file {}: {error}", path.display()))?;
        summary.files_changed += 1;
    }

    Ok(())
}

fn is_pump_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pump"))
}

fn record_saved_scan_error(
    summary: &mut SavedDropCleanup,
    path: &Path,
    error: impl std::fmt::Display,
) {
    summary.errors += 1;
    clear_drops_debug(format!("saved scan error in {}: {error}", path.display()));
}
