# Cabbage: High-Performance AI Orchestrator & Utility Suite

A native dynamic plugin (`cdylib`) for the [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) server. Cabbage serves as a central orchestrator for mob AI, pathfinding, and administrative server optimization.

Because it compiles directly to native machine code (`.dll` / `.so`), Cabbage executes at bare-metal speeds without Wasm sandboxing overhead, allowing it to manage complex AI calculations and large-scale entity operations efficiently.

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

### 3. Administrative Utility Commands
* `/cabbage`
  * Displays help, current configuration options, and plugin status.
* `/cleardrops`
  * Queues a clean job to remove all loaded dropped item entities from active worlds.
  * Scans and deletes saved dropped items (`minecraft:item`) directly from `.pump` region files inside world save folders.
* `/metrics`
  * Prints a live report of server tick rates, thread states, and memory allocations.
  * `/metrics log`: Toggles real-time console metrics logging.

### 4. Configuration & Tuning Options
Exposes parameters to balance detail vs. performance:
* **Tick Intervals**: Adjust the AI update rate (`MOB_MOVE_PERIOD_TICKS`) and pathfinder recalculation cooldowns.
* **Pathfinding Search Bounds**: Configure constraints like max jump height (`MAX_PATH_HEIGHT_DIFFERENCE`) and search space volume limit (`MAX_PATH_GRID_VOLUME`).
* **Entity Radius & Range**: Limit how far mobs can search for targets and when they clear out-of-range path caches.

---

## Development Build

To compile the plugin locally:

```powershell
.\compile.bat
```

This compiles Cabbage in debug mode and automatically copies the dynamic library into the server's plugins directory:
```text
../../plugins/cabbage.dll
```

## Production Build

For final performance testing, build a fully optimized release bundle:

```powershell
cargo build --release
```

Load the compiled plugin dynamically from the Pumpkin console:
```text
/plugin load plugins/cabbage.dll
```

---

## Compatibility Note
This native plugin compiles against the unstable Rust ABI. You must use the **same stable Rust toolchain version** to compile both the Pumpkin server and the Cabbage plugin to avoid memory layout mismatches and potential crashes.
