# Cabbage Agent Guide

## Hard Boundary

- Only edit files inside this `Cabbage/` directory.
- Files outside this directory may be read for API discovery, examples, and compatibility checks, but must not be edited.
- Prefer accomplishing changes by extending these plugins rather than changing Pumpkin core crates.

## Project Purpose

Cabbage is a suite of native Pumpkin plugins: `Cabbage.Core` (server utilities, diagnostics, drop cleanup, event logging), `Cabbage.Mmo` (MMO-style skilling), and `Cabbage.MobAi` (custom mob AI/pathfinding), plus the shared `cabbage-api` rlib that holds their cross-plugin contracts. The README is the high-level product guide; this file is the working guide for future agents.

When the task involves Mob AI, pathfinding, movement, velocity planning, worker threads, or managed entity behavior, read `mobai/src/MOB_AI_DATA_FLOW.md` before editing files in `mobai/`.

When the task involves the MMO module (skills, XP, bossbars, ore reveal, SQLite), read `mmo/src/README.md` before editing files in `mmo/`.

## Current Layout

`Cargo.toml` at the root is a virtual workspace with four members:

```text
Cabbage/
|-- AGENTS.md
|-- README.md
|-- Cargo.toml             # virtual workspace: api, core, mmo, mobai
|-- compile.bat            # gitignored local build/deploy helper (see Build And Verification)
|-- api/                   # cabbage-api (rlib) — shared cross-plugin contracts ONLY
|   `-- src/
|       `-- lib.rs         # MobAiApi trait, MobAiMetricsSnapshot, MobAiService/CoreServices
|                          # payloads (hand-written Payload impls), MOB_AI_SERVICE/CORE_SERVICE names
|-- core/                  # cabbage-core (cdylib) -> plugin "Cabbage.Core" (no plugin dependencies)
|   `-- src/
|       |-- lib.rs         # DLL exports, metadata, lifecycle, legacy data migration
|       |-- commands.rs    # /cabbage, /cleardrops, /metrics, /events trees and executors
|       |-- drops.rs       # loaded dropped-item cleanup + .pump region file item scan
|       |-- metrics.rs     # MetricsReporterState, CoreConfig, disk scan; consumes MobAiService lazily
|       `-- event_log.rs   # /events toggle; event logging to chat and output.log
|-- mmo/                   # cabbage-mmo (rlib + cdylib) -> plugin "Cabbage.Mmo" (depends on Cabbage.Core)
|   `-- src/
|       |-- lib.rs         # MmoState, EventHandler impls, config load/save, legacy data migration
|       |-- plugin.rs      # DLL exports, metadata, event/command registration
|       |-- README.md      # MMO module guide (read before editing mmo/)
|       |-- BALANCE.md     # default balance profile and migration guide
|       |-- plan.md        # phased MMO implementation plan (historical)
|       |-- skills.rs      # SkillId (18 skills, 6/6/6 branches; retired names are aliases), BranchId, metadata
|       |-- progression.rs # central award_xp path and branch mastery
|       |-- audit.rs       # append-only audit log (mmo-audit.log)
|       |-- perks/         # perk scaffolding (cooldowns, batch breaks, level gates)
|       |-- persistence/   # typed Pumpkin player/item/entity/block codecs
|       |-- frontier/      # Frontier skill handlers and configuration
|       |-- warfare/       # Warfare skill handlers and configuration
|       |-- enterprise/    # Enterprise skill handlers and configuration
|       |-- ui/            # bossbars, protected skill menu, default chat summary grid,
|       |                  # /mmo skill detail pages + progression catalog
|       |-- commands.rs    # /mmo command tree and admin subcommands
|       |-- config.rs      # RON config and per-skill level curves
|       |-- db.rs          # SQLite worker thread, migrations, async DB API
|       `-- ore_reveal/    # ore config, probability, shape, provenance
`-- mobai/                 # cabbage-mobai (rlib + cdylib) -> plugin "Cabbage.MobAi" (depends on Cabbage.Core)
    `-- src/
        |-- lib.rs         # MobAiState, EventHandler impls, game-thread orchestration
        |-- plugin.rs      # DLL exports, metadata, event registration, MobAiService publication
        |-- MOB_AI_DATA_FLOW.md  # Mob AI threading model (read before editing mobai/)
        |-- pathfinding.rs # grid, bounds, bidirectional A*
        |-- movement.rs    # weighted lookahead helpers
        |-- workers.rs     # Rayon worker-pool job submission and ActiveJobGuard
        |-- clustering.rs  # mob-location table and anti-clump push
        `-- types.rs       # shared snapshots and small data structs
```

The `mmo` and `mobai` crates are `rlib`s so `cargo test` can exercise their
logic independently. Their `plugin.rs` modules retain feature-specific
lifecycle and registration code; `core` is the sole `cdylib` and native
plugin entry point.

## Conventions

- Keep game-thread access to Pumpkin entities/worlds in the event handler or narrowly named game-thread helpers.
- Worker jobs must receive owned snapshots only: block grids, positions, UUIDs, speed values, and cloned lookup tables.
- Do not move `Arc<World>`, `Entity`, `EntityBase`, or live Pumpkin entity handles into worker-pool jobs.
- Preserve focused unit tests. Pathfinding, grid indexing, movement math, and clustering should remain testable without a running server.
- Keep command utilities separate from Mob AI. Avoid mixing admin cleanup, metrics, and entity-control code in the same module.
- For features that need persistence, follow the MMO pattern: a dedicated worker thread/connection with an async request/response API. Do not run blocking SQLite or file I/O on the event-handler path.
- Shared service types live ONLY in `cabbage-api`. Feature crates must not depend on each other's concrete state types (see Services below).

## Plugin Suite Topology

- **Cabbage.Core** is the single native plugin (`cabbage.dll`). It owns the exported metadata and plugin lifecycle, provides `CoreServices` (`cabbage_core`), and invokes the MMO and Mob AI module lifecycles internally.
- The `mmo` and `mobai` feature crates have no DLL exports or plugin metadata. They remain independently testable and retain their own event/command/service registration code.
- Core receives `plugins/Cabbage.Core` from `Context::get_data_folder()`. The MMO module explicitly preserves its existing `plugins/Cabbage.Mmo` folder for configuration and SQLite data.
- Permission namespaces remain stable:
  - `Cabbage.Core:command.cabbage`, `Cabbage.Core:command.clear_drops`, `Cabbage.Core:command.metrics`, `Cabbage.Core:command.events`
  - `Cabbage.Mmo:command.mmo` (default allow), `Cabbage.Mmo:command.mmo.admin` (op level 3)
  - Mob AI registers no commands or permissions; the engine is toggled via the `mob_ai` key in Core's `config.ron` through the `MobAiService`.
- The old monolithic `Cabbage:*` permission nodes no longer exist — this is a breaking change for admins upgrading from the pre-split plugin.

### Legacy data migration (`plugins/Cabbage/`)

Files from the pre-split monolithic plugin are adopted on first load; legacy files are never modified or deleted:

- **Cabbage.Core** copies `output.log` into `plugins/Cabbage.Core/` when it does not exist there yet, and reads the legacy `config.ron` in place to adopt its `metrics_log`/`mob_ai` values (the legacy file's `mmo` section is ignored by Core).
- **Cabbage.Mmo** copies `config.ron`, `config.json`, `mmo.db`, and `mmo-audit.log` into `plugins/Cabbage.Mmo/` when they do not exist there yet. The legacy `config.ron` is a full `PluginConfig`, exactly the format Cabbage.Mmo reads.

## Pumpkin Native Plugin API

Cabbage is one native (`cdylib`) plugin loaded by Pumpkin's `NativePluginLoader`. It compiles against the `pumpkin` crate directly, not the WebAssembly `pumpkin-plugin-api`. The native API lives in `Pumpkin/pumpkin/src/plugin/` and is re-exported through `pumpkin::plugin`.

Critical compatibility requirement: the compiled DLL must expose the same `PUMPKIN_API_VERSION` as the server (currently `14`, defined in `Pumpkin/pumpkin/src/plugin/mod.rs`). If it changes, recompile Cabbage against the same Pumpkin source or the loader will reject the DLL with `ApiVersionMismatch`.

### Required DLL symbols

The native loader expects three symbols from the Core crate:

```rust
use std::mem::MaybeUninit;
use pumpkin::plugin::{Plugin, PluginMetadata, PLUGIN_API_VERSION};

#[unsafe(no_mangle)]
pub static PUMPKIN_API_VERSION: u32 = PLUGIN_API_VERSION;

#[unsafe(no_mangle)]
pub static mut METADATA: MaybeUninit<PluginMetadata> = MaybeUninit::uninit();

#[unsafe(no_mangle)]
pub fn plugin() -> Box<dyn Plugin> {
    Box::new(CabbageCorePlugin::new())
}
```

`METADATA` is written before the loader reads it (each plugin uses `ctor::ctor` in `core/src/lib.rs`, `mmo/src/plugin.rs`, and `mobai/src/plugin.rs`). `PluginMetadata.dependencies` lists required plugins by name (`Cabbage.Mmo` and `Cabbage.MobAi` both depend on `Cabbage.Core`). `plugin()` returns the actual plugin instance that implements the `Plugin` trait.

### Plugin lifecycle

```rust
use pumpkin::plugin::{Plugin, Context, PluginFuture};

impl Plugin for CabbageCorePlugin {
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

Unload rules every Cabbage plugin follows:

- Pumpkin never removes registered event handlers or services, and Windows keeps unloaded DLLs mapped. Every handler therefore gates on an `active: AtomicBool` that `on_unload` clears; handlers early-return once it flips.
- Each plugin unregisters its own commands in `on_unload` (`context.unregister_command(...)`).

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

Import existing events exactly as the Cabbage crates already do, e.g.:

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

// in on_load (inside Cabbage.Core):
context
    .register_command(my_command_tree(), "Cabbage.Core:command.mycommand")
    .await;
```

- `CommandResult` is `Result<i32, CommandError>`. Return `Ok(1)` for success or `Ok(0)` for silent failure.
- Node builders include `literal`, `argument`, `argument_default_name`, and `require`.
- The permission node passed to `register_command` should be namespaced with the registering plugin's name (`Cabbage.Core:...`, `Cabbage.Mmo:...`). If you omit the colon, `Context` prefixes the plugin name automatically.
- Unregister with `context.unregister_command("mycommand").await` in `on_unload`.

### Registering permissions

Permissions must be registered in the plugin's own namespace:

```rust
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionLvl};

let perm = Permission::new(
    "Cabbage.Core:command.mycommand",
    "Allows running /mycommand",
    PermissionDefault::Op(PermissionLvl::Two),
);
context.register_permission(perm).await?;
```

`PermissionLvl` values are `Zero` (normal player), `One`, `Two`, `Three`, `Four` (console/owner).

### Plugin data folder

```rust
let data_folder = context.get_data_folder(); // ./plugins/<plugin name>, e.g. ./plugins/Cabbage.Core
```

Use this for config files, logs, and databases. Core stores `config.ron` (`CoreConfig`: `metrics_log`, `mob_ai`) and `output.log` here; Mmo stores `config.ron` (`PluginConfig`), `mmo.db`, and `mmo-audit.log` here.

### Services (cross-plugin shared state)

Cross-plugin contracts live in the `cabbage-api` rlib and nowhere else:

- `cabbage-api` defines plain data snapshots (`MobAiMetricsSnapshot`), capability traits (`MobAiApi`), service wrappers (`MobAiService`, `CoreServices`), and the registry name constants (`MOB_AI_SERVICE = "cabbage_mob_ai"`, `CORE_SERVICE = "cabbage_core"`). Plugin crates share types ONLY through `cabbage-api`; they never depend on each other's concrete state types.
- `Payload` is implemented by hand (Pumpkin's `#[derive(Event)]` only resolves inside the pumpkin crate) with a `cabbage.`-prefixed name string (e.g. `"cabbage.MobAiService"`) so it can never collide with a Pumpkin event name during the registry's name-based downcast. Both `get_name_static` and `get_name` must return the same string.
- Publishers register in `on_load`; consumers must tolerate the service being absent. Cabbage.MobAi publishes `MobAiService` in `on_load`; Cabbage.Core looks it up once in `on_load` and then retries lazily on each metrics tick until it appears (runtime `/plugin load` order is not sorted by dependencies). Always use the `cabbage-api` name constants at both ends, never string literals.

```rust
// publisher (mobai/src/plugin.rs)
context
    .register_service(
        cabbage_api::MOB_AI_SERVICE,
        Arc::new(cabbage_api::MobAiService(Arc::new(MobAiApiAdapter(state.clone())))),
    )
    .await;

// consumer (core/src/lib.rs, retried from the metrics tick when None)
if let Some(service) = context
    .get_service::<cabbage_api::MobAiService>(cabbage_api::MOB_AI_SERVICE)
    .await
{
    metrics_reporter_state.set_mob_ai(service.0.clone());
}
```

Services must implement `Payload + 'static`.

## Build And Verification

- Use `cargo fmt` after Rust edits.
- Use `cargo check --workspace` before handoff. Because the workspace uses path dependencies into `../Pumpkin`, this also validates compatibility with the current Pumpkin source.
- Use `cargo test --workspace` for the full suite; `cargo test -p cabbage-mobai` for Mob AI changes specifically.
- Use `.\compile.bat` from this directory to build the workspace and deploy `cabbage.dll` from `target/debug/` to `../PumpkinRunner/plugins/`. It removes the obsolete `cabbage_core.dll`, `cabbage_mmo.dll`, and `cabbage_mobai.dll` files. The file is gitignored (local only).
- For a release build use `cargo build --release`. Load the resulting DLL from the Pumpkin console with `/plugin load plugins/cabbage.dll`.

## Git Commits

- After each prompt execution where files were changed, create a git commit containing those changes.
- Write a concise commit message that describes what changed and why, following the repo's existing commit style.
- Do not commit unrelated pre-existing changes; stage only the files this task touched.
- Commits stay local — do not push unless the user explicitly asks.

## Compatibility Note

These native plugins compile against the unstable Rust ABI. You must use the **same stable Rust toolchain version** to compile the Pumpkin server and every Cabbage plugin DLL to avoid memory layout mismatches and potential crashes.
