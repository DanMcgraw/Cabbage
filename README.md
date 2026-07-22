# Cabbage: High-Performance AI Orchestrator & Utility Suite

A native dynamic plugin (`cdylib`) for the [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) server, built from a Cargo workspace with refactored feature crates:

* **Cabbage** (`Cabbage.dll`) — the single Pumpkin plugin entry point: administrative utility commands, drop cleanup, server metrics, event logging, MMO skilling, and Mob AI.
* **`mmo` crate** — the MMO-style skilling module (23 skills, bossbars, ore reveal, SQLite persistence), registered by Core during plugin load.
* **`mobai` crate** — the custom mob AI & pathfinding engine, also registered by Core and exposed through the internal service registry for metrics/control.

Because they compile directly to native machine code (`.dll` / `.so`), the Cabbage plugins execute at bare-metal speeds without Wasm sandboxing overhead, allowing them to manage complex AI calculations and large-scale entity operations efficiently.

---

## Installation

Drop the one DLL into the server's `plugins/` directory:

```text
plugins/Cabbage.dll
```

The feature crates are linked into `Cabbage.dll`, so Pumpkin loads only one native plugin and the common Rust/Pumpkin code is linked once.

### Data and permission compatibility

* **Permissions use one namespace.** Use `Cabbage:command.cabbage`, `Cabbage:command.clear_drops`, `Cabbage:command.metrics`, `Cabbage:command.events`, `Cabbage:command.mmo`, and `Cabbage:command.mmo.admin`. The base MMO permission defaults to allow for normal players.
* **Data uses one folder.** The unified `config.ron`, `mmo.db`, `mmo-audit.log`, and `output.log` live in `plugins/Cabbage/`. On first load, missing files are copied from the former `Cabbage.Core` and `Cabbage.Mmo` folders; those source folders are left untouched as backups.

---

## Key Pillars & Features

### 1. Native Mob AI & Pathfinding Engine
Cabbage implements a custom, multithreaded AI engine that replaces Pumpkin's built-in selectors to act as the single source of truth for entity behavior:
* **Generic A* Pathfinding**: Supports multi-stage A* path searches, height-climbing/climbing constraints, diagonal XZ movement, and solid block obstacle avoidance.
* **Separation of Rotation Concerns**:
  * **Body Yaw**: Aligns automatically with the entity's movement velocity vector (`entity.yaw` and `entity.body_yaw`) so the model walks forward naturally.
  * **Head Yaw & Pitch**: Keeps the head smoothly tracking the nearest player on every tick by integrating directly with Pumpkin's `LookControl` system.
* **Orchestrator Mode**: Dynamically locks and clears the server's default goal and target selectors for managed mobs (`creeper`, `zombie`, `skeleton`), preventing conflicts and twitching.

### 2. Performance Optimizations
Designed from the ground up to prevent TPS drops under entity load:
* **AI Goal Disabling**: Disables all unneeded default AI selectors to eliminate useless tick-based scans.
* **Background Path Calculations**: Offloads heavy pathfinding computations onto asynchronous worker tasks to keep the main server tick thread fast and responsive.
* **Staggered Searches**: Spaces out nearest-player distance checks and target scans instead of running expensive searches for every mob on every single tick.
* **Memory Buffer Caching**: Minimizes heap allocation cycles by reusing pre-allocated vectors during block grid sampling.

### 3. Administrative Utility Commands (Cabbage)
* `/cabbage`
  * Displays help, current configuration options, and plugin status.
* `/cleardrops`
  * Queues a clean job to remove all loaded dropped item entities from active worlds.
  * Scans and deletes saved dropped items (`minecraft:item`) directly from `.pump` region files inside world save folders.
* `/metrics`
  * Prints a live report of server tick rates, thread states, memory allocations, and Mob AI counters.
  * `/metrics log`: Toggles real-time console metrics logging.
* `/events`
  * Toggles event diagnostic logging to chat and `output.log` in the Cabbage data folder.

### 4. Configuration & Tuning Options
The plugin owns one `config.ron` in `plugins/Cabbage/`. Its `metrics_log` and `mob_ai` switches control the utility and AI modules, while its `mmo` section carries the full skilling and ore-reveal balance profile. Exposed parameters balance detail vs. performance:
* **Tick Intervals**: Adjust the AI update rate and pathfinder recalculation cooldowns.
* **Pathfinding Search Bounds**: Configure constraints like max jump height (`MAX_PATH_HEIGHT_DIFFERENCE`) and search space volume limit (`MAX_PATH_GRID_VOLUME`).
* **Entity Radius & Range**: Limit how far mobs can search for targets and when they clear out-of-range path caches.

---

## Development Build

To compile the plugins locally:

```powershell
.\compile.bat
```

This builds the whole workspace in debug mode and copies the combined DLL into the server's plugins directory. It removes the obsolete split DLL names:
```text
../PumpkinRunner/plugins/Cabbage.dll
```

## Production Build

For final performance testing, build a fully optimized release bundle:

```powershell
cargo build --release
```

Load the compiled plugin dynamically from the Pumpkin console:
```text
/plugin load plugins/Cabbage.dll
```

---

## Compatibility Note
This native plugin compiles against the unstable Rust ABI. You must use the **same stable Rust toolchain version** to compile the Pumpkin server and Cabbage DLL to avoid memory layout mismatches and potential crashes.
