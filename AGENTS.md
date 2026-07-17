# Cabbage Agent Guide

## Hard Boundary

- Only edit files inside this `Cabbage/` directory.
- Files outside this directory may be read for API discovery, examples, and compatibility checks, but must not be edited.
- Prefer accomplishing changes by extending this plugin rather than changing Pumpkin core crates.

## Project Purpose

Cabbage is a native Pumpkin plugin focused on server utilities, diagnostics, custom mob AI/pathfinding, and an MMO-style skilling module. The README is the high-level product guide; this file is the working guide for future agents.

When the task involves Mob AI, pathfinding, movement, velocity planning, worker threads, or managed entity behavior, read `src/mob_ai/MOB_AI_DATA_FLOW.md` before editing files in `src/mob_ai/`.

When the task involves the MMO module (skills, XP, bossbars, ore reveal, SQLite), read `src/mmo/README.md` before editing files in `src/mmo/`.

## Current Layout

```text
Cabbage/
|-- AGENTS.md
|-- README.md
|-- Cargo.toml
|-- compile.bat
`-- src/
    |-- lib.rs                 # plugin metadata, lifecycle, command/event registration
    |-- mmo/                   # MMO skilling module (see src/mmo/README.md)
    |   |-- README.md
    |   |-- mod.rs             # MmoState and event handler wiring
    |   |-- bossbar.rs         # transient skill-progress bossbars
    |   |-- commands.rs        # /mmo command tree and admin subcommands
    |   |-- config.rs          # RON config and per-skill level curves
    |   |-- db.rs              # SQLite worker thread and async DB API
    |   |-- events.rs          # BlockBreakEvent / EntityDeathEvent XP handlers
    |   |-- player.rs          # player-related utilities
    |   |-- skills.rs          # SkillId enum and skill metadata
    |   `-- ore_reveal/        # ore config, probability, shape, provenance
    `-- mob_ai/                # custom multithreaded mob AI engine
        |-- mod.rs             # event handler and state ownership
        |-- MOB_AI_DATA_FLOW.md
        |-- pathfinding.rs     # grid, bounds, bidirectional A*
        |-- movement.rs        # velocity plans, lookahead, rotation
        |-- workers.rs         # Rayon worker-pool job submission and ActiveJobGuard
        |-- clustering.rs      # mob-location table and anti-clump push
        `-- types.rs           # shared snapshots and small data structs
```

## Refactor Organization Chart

Use this target structure as files become larger. (The **mob_ai** refactor has been completed).

```text
src/
|-- lib.rs
|   |-- plugin metadata and load/unload wiring
|   |-- command/event registration only
|   `-- module exports
|
|-- commands/
|   |-- mod.rs
|   |-- cabbage.rs
|   |-- clear_drops.rs
|   `-- metrics.rs
|
|-- drops/
|   |-- mod.rs
|   |-- loaded_cleanup.rs
|   `-- saved_region_cleanup.rs
|
`-- metrics/
    |-- mod.rs
    |-- config.rs
    |-- reporter.rs
    `-- disk_scan.rs
```

## Refactor Rules

- Keep game-thread access to Pumpkin entities/worlds in the event handler or narrowly named game-thread helpers.
- Worker jobs must receive owned snapshots only: block grids, positions, UUIDs, speed values, and cloned lookup tables.
- Do not move `Arc<World>`, `Entity`, `EntityBase`, or live Pumpkin entity handles into worker-pool jobs.
- Preserve focused unit tests when moving functions. Pathfinding, grid indexing, movement math, and clustering should remain testable without a running server.
- Keep command utilities separate from Mob AI. Avoid mixing admin cleanup, metrics, and entity-control code in the same module.
- For features that need persistence, follow the MMO pattern: a dedicated worker thread/connection with an async request/response API. Do not run blocking SQLite or file I/O on the event-handler path.

## Pumpkin Native Plugin API

Cabbage is a native (`cdylib`) plugin loaded by Pumpkin's `NativePluginLoader`. It compiles against the `pumpkin` crate directly, not the WebAssembly `pumpkin-plugin-api`. The native API lives in `Pumpkin/pumpkin/src/plugin/` and is re-exported through `pumpkin::plugin`.

Critical compatibility requirement: the compiled DLL must expose the same `PUMPKIN_API_VERSION` as the server. The current value is defined in `Pumpkin/pumpkin/src/plugin/mod.rs`. If it changes, recompile Cabbage against the same Pumpkin source or the loader will reject the DLL with `ApiVersionMismatch`.

### Required DLL symbols

The native loader expects three symbols:

```rust
use std::mem::MaybeUninit;
use pumpkin::plugin::{Plugin, PluginMetadata, PLUGIN_API_VERSION};

#[unsafe(no_mangle)]
pub static PUMPKIN_API_VERSION: u32 = PLUGIN_API_VERSION;

#[unsafe(no_mangle)]
pub static mut METADATA: MaybeUninit<PluginMetadata> = MaybeUninit::uninit();

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(CabbagePlugin::new())
}
```

`METADATA` is written before the loader reads it (Cabbage uses `ctor::ctor` in `src/lib.rs`). `plugin()` returns the actual plugin instance that implements the `Plugin` trait.

### Plugin lifecycle

```rust
use pumpkin::plugin::{Plugin, Context, PluginFuture};

impl Plugin for CabbagePlugin {
    fn on_load(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            // register permissions, commands, and event handlers
            Ok(())
        })
    }

    fn on_unload(&mut self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            // cleanup, unregister commands
            Ok(())
        })
    }
}
```

### Registering event handlers

Event handlers implement `EventHandler<E>` for a concrete event type `E`.

```rust
use std::sync::Arc;
use pumpkin::plugin::{EventHandler, EventPriority, BoxFuture};
use pumpkin::plugin::api::events::server::server_tick_start::ServerTickStartEvent;
use pumpkin::server::Server;

struct TickListener;

impl EventHandler<ServerTickStartEvent> for TickListener {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // read-only observation
        })
    }

    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut ServerTickStartEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // can mutate or cancel the event
        })
    }
}

// in on_load:
context
    .register_event::<ServerTickStartEvent, _>(
        Arc::new(TickListener),
        EventPriority::Normal,
        true, // true = blocking handler, receives &mut E
    )
    .await;
```

Handler rules:

- `blocking = true`: the handler is awaited sequentially before non-blocking handlers and receives `&mut E`. Use this when you need to mutate/cancel the event or read live world/entity state that is only safe on the game thread.
- `blocking = false`: the handler is run concurrently with other non-blocking handlers via `join_all` and receives `&E`. Use only for observation or for offloading work to worker threads.
- All handlers execute in the same async call path as `PluginManager::fire`, which is normally the server tick. Avoid blocking I/O directly in a handler.
- `EventPriority` is stored on each handler but does not currently affect dispatch order; handlers run in registration order.

### Cancelling events

Events that implement `Cancellable` can be cancelled from a blocking handler:

```rust
use pumpkin::plugin::api::events::Cancellable;

fn handle_blocking<'a>(
    &'a self,
    _server: &'a Arc<Server>,
    event: &'a mut SomeCancellableEvent,
) -> BoxFuture<'a, ()> {
    Box::pin(async move {
        if should_cancel(event) {
            event.set_cancelled(true);
        }
    })
}
```

### Event categories

Events are organized under `pumpkin::plugin::api::events`:

- `block` — `BlockBreakEvent`, `BlockPlaceEvent`, `BlockDamageEvent`, `BlockDropItemEvent`, `BlockPistonExtendEvent`, `BlockPistonRetractEvent`, `BrewEvent`, `FurnaceBurnEvent`, `FurnaceSmeltEvent`, etc.
- `entity` — `EntitySpawnEvent`, `EntityRemoveEvent`, `EntityDamageEvent`, `EntityDamageByEntityEvent`, `EntityDeathEvent`, `EntityTargetEvent`, `EntityPickupItemEvent`, `ExplosionPrimeEvent`, etc.
- `inventory` — `InventoryClickEvent`, `InventoryMoveItemEvent`, etc.
- `player` — `PlayerJoinEvent`, `PlayerLeaveEvent`, `PlayerMoveEvent`, `PlayerDeathEvent`, `PlayerDropItemEvent`, `CraftItemEvent`, etc.
- `server` — `ServerTickStartEvent`, `ServerTickEndEvent`, `ServerBroadcastEvent`, `ServerCommandEvent`, `SpawnChangeEvent`.
- `world` — `ChunkLoadEvent`, `ChunkSaveEvent`, `ChunkSendEvent`, `ChunkUnloadEvent`, `FeatureGenerateEvent`, `WorldLoadEvent`, `WorldUnloadEvent`, `SpawnChangeEvent`.

Import existing events exactly as Cabbage already does, e.g.:

```rust
use pumpkin::plugin::api::events::block::block_break::BlockBreakEvent;
use pumpkin::plugin::api::events::entity::EntityDeathEvent;
use pumpkin::plugin::api::events::world::feature_generate::FeatureGenerateEvent;
```

### Registering commands

Commands are built with `CommandTree` and registered via `Context::register_command`:

```rust
use pumpkin::command::{
    CommandExecutor, CommandResult, CommandSender, args::ConsumedArgs,
    tree::{CommandTree, builder::literal},
};
use pumpkin_util::text::TextComponent;

struct MyCommandExecutor;

impl CommandExecutor for MyCommandExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> CommandResult<'a> {
        Box::pin(async move {
            sender.send_message(TextComponent::text("Hello from Cabbage")).await;
            Ok(1)
        })
    }
}

fn my_command_tree() -> CommandTree {
    CommandTree::new(["mycommand"], "My command description")
        .execute(MyCommandExecutor)
        .then(literal("sub").execute(MyCommandExecutor))
}

// in on_load:
context
    .register_command(my_command_tree(), "Cabbage:command.mycommand")
    .await;
```

- `CommandResult` is `Result<i32, CommandError>`. Return `Ok(1)` for success or `Ok(0)` for silent failure.
- Node builders include `literal`, `argument`, `argument_default_name`, and `require`.
- The permission node passed to `register_command` should be namespaced with the plugin name (`Cabbage:...`). If you omit the colon, `Context` prefixes the plugin name automatically.
- Unregister with `context.unregister_command("mycommand").await` in `on_unload`.

### Registering permissions

Permissions must be registered in the plugin namespace:

```rust
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionLvl};

let perm = Permission::new(
    "Cabbage:command.mycommand",
    "Allows running /mycommand",
    PermissionDefault::Op(PermissionLvl::Two),
);
context.register_permission(perm).await?;
```

`PermissionLvl` values are `Zero` (normal player), `One`, `Two`, `Three`, `Four` (console/owner).

### Plugin data folder

```rust
let data_folder = context.get_data_folder(); // ./plugins/Cabbage
```

Use this for config files, logs, and the MMO database. Cabbage already stores `output.log` and `mmo.db` here.

### Services (cross-plugin shared state)

For large features you can register a typed service:

```rust
context.register_service("cabbage_mob_ai", state.clone()).await;
let state = context.get_service::<MobAiState>("cabbage_mob_ai").await;
```

Services must implement `Payload + 'static`.

## Build And Verification

- Use `cargo fmt` after Rust edits.
- Use `cargo check` before handoff. Because `Cargo.toml` uses path dependencies into `../Pumpkin`, this also validates compatibility with the current Pumpkin source.
- Use `cargo test mob_ai` for Mob AI changes.
- Use `.\compile.bat` from this directory to build/copy the plugin DLL. It copies the debug artifact to `../PumpkinRunner/plugins/cabbage.dll`.
- For a release build use `cargo build --release`. Load the resulting DLL from the Pumpkin console with `/plugin load plugins/cabbage.dll`.

## Git Commits

- After each prompt execution where files were changed, create a git commit containing those changes.
- Write a concise commit message that describes what changed and why, following the repo's existing commit style.
- Do not commit unrelated pre-existing changes; stage only the files this task touched.
- Commits stay local — do not push unless the user explicitly asks.

## Compatibility Note

This native plugin compiles against the unstable Rust ABI. You must use the **same stable Rust toolchain version** to compile both the Pumpkin server and the Cabbage plugin to avoid memory layout mismatches and potential crashes.
