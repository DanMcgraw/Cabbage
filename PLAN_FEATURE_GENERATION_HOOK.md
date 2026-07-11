# Plan: Plugin-Controllable World Feature Generation Hook

## Goal

Add a hook in Pumpkin that fires when the world generator is about to place a feature (ores, trees, structures, etc.) during chunk population, allowing the Cabbage plugin to cancel specific features — primarily to disable ore generation.

## Approach Overview

1. Add a cancellable `FeatureGenerateEvent` in Pumpkin's plugin API.
2. Extend `WorldPortalExt` with a `should_generate_feature` method so `pumpkin-world` can ask the controlling crate (`pumpkin`) whether to proceed.
3. Implement that method in `pumpkin::WorldPortal` by firing the event through the plugin manager.
4. Gate feature generation in `pumpkin-world`'s chunk population loop on the result.
5. In Cabbage, register for the event and cancel features whose names are in a config-driven blacklist.

This keeps `pumpkin-world` decoupled from the plugin system while giving Cabbage full control.

---

## Pumpkin Changes

### 1. New event type

**File:** `pumpkin/src/plugin/api/events/world/feature_generate.rs`

```rust
use crate::world::World;
use pumpkin_macros::{Event, cancellable};
use pumpkin_util::math::{position::BlockPos, vector2::Vector2};
use std::sync::Arc;

#[cancellable]
#[derive(Event, Clone)]
pub struct FeatureGenerateEvent {
    pub world: Arc<World>,
    pub chunk_pos: Vector2<i32>,
    pub feature: pumpkin_data::placed_feature::PlacedFeature,
    pub origin: BlockPos,
}
```

### 2. Export the event

**Files:**
- `pumpkin/src/plugin/api/events/world/mod.rs` — add `pub mod feature_generate;`
- `pumpkin/src/plugin/api/events/mod.rs` — re-export as needed for plugin use

### 3. Add hook method to `WorldPortalExt`

**File:** `pumpkin-world/src/world.rs`

```rust
fn should_generate_feature(
    &self,
    chunk_x: i32,
    chunk_z: i32,
    feature: pumpkin_data::placed_feature::PlacedFeature,
    origin: &BlockPos,
) -> bool;
```

This is the clean boundary: `pumpkin-world` asks, `pumpkin` decides.

### 4. Implement the hook in `WorldPortal`

**File:** `pumpkin/src/world/mod.rs`

Implement `should_generate_feature` for `WorldPortal`. Use the existing `runtime.block_on` pattern already used for `ChunkUnloadEvent` (see around line 347).

```rust
fn should_generate_feature(
    &self,
    chunk_x: i32,
    chunk_z: i32,
    feature: pumpkin_data::placed_feature::PlacedFeature,
    origin: &BlockPos,
) -> bool {
    let Some(server) = self.0.server.upgrade() else {
        return true;
    };
    let Some(runtime) = self.0.runtime.clone() else {
        return true;
    };

    let event = crate::plugin::api::events::world::feature_generate::FeatureGenerateEvent::new(
        self.0.clone(),
        Vector2::new(chunk_x, chunk_z),
        feature,
        *origin,
    );

    runtime.block_on(async move { !server.plugin_manager.fire(event).await.cancelled })
}
```

### 5. Gate feature generation

**File:** `pumpkin-world/src/generation/proto_chunk.rs`

In `generate_features_and_structure`, inside the `for (p, feature_enum)` loop around line 1069, add before `feature.generate(...)`:

```rust
if !block_registry.should_generate_feature(center_x, center_z, feature_enum, &origin_pos) {
    continue;
}
```

### 6. Update test `BlockRegistry` impls

**Files:**
- `pumpkin-world/src/chunk_system/generation.rs`
- `pumpkin-world/src/chunk/format/anvil.rs`

Add `should_generate_feature` returning `true` so tests and offline chunk loading still compile.

---

## Cabbage Changes

### 1. Config option

**File:** `src/mmo/config.rs`

Add a list of placed-feature names to disable:

```rust
pub struct MmoConfig {
    // ... existing fields ...
    pub disabled_world_features: Vec<String>,
}
```

### 2. Register the event handler

**File:** `src/mmo/mod.rs`

In `on_load`, register `FeatureGenerateEvent` when MMO is enabled, similar to existing event registrations.

### 3. Handle the event

**File:** `src/mmo/events.rs` (or new `src/mmo/worldgen.rs`)

```rust
use pumpkin::plugin::api::events::world::feature_generate::FeatureGenerateEvent;

pub async fn handle_feature_generate(state: &MmoState, event: &mut FeatureGenerateEvent) {
    let feature_name = event.feature.to_string();
    if state.config().disabled_world_features.contains(&feature_name) {
        event.cancelled = true;
    }
}
```

`PlacedFeature` exposes a name via `pumpkin_data::placed_feature::PlacedFeature::from_name`; the `to_string`/`name` accessor should be verified during implementation.

### 4. Default ore blacklist

Ship a sensible default in `config.ron`. Example names to verify against the enum:

```ron
disabled_world_features: [
    "ore_coal_upper",
    "ore_coal_lower",
    "ore_iron_upper",
    "ore_iron_middle",
    "ore_iron_small",
    "ore_gold",
    "ore_redstone",
    "ore_diamond_large",
    "ore_diamond_buried",
    "ore_lapis",
    "ore_lapis_buried",
    "ore_copper",
],
```

These names must match the generated `pumpkin_data::placed_feature::PlacedFeature` variants exactly.

---

## Files to Modify

### Pumpkin
- `pumpkin/src/plugin/api/events/world/feature_generate.rs` (new)
- `pumpkin/src/plugin/api/events/world/mod.rs`
- `pumpkin/src/plugin/api/events/mod.rs`
- `pumpkin/src/world/mod.rs`
- `pumpkin-world/src/world.rs`
- `pumpkin-world/src/generation/proto_chunk.rs`
- `pumpkin-world/src/chunk_system/generation.rs`
- `pumpkin-world/src/chunk/format/anvil.rs`

### Cabbage
- `src/mmo/config.rs`
- `src/mmo/mod.rs`
- `src/mmo/events.rs` (or new `src/mmo/worldgen.rs`)

---

## Trade-offs and Risks

| Concern | Mitigation |
|---|---|
| **Sync/async bridge** | Uses `runtime.block_on`, matching the existing `ChunkUnloadEvent` pattern. Acceptable because chunk generation is off the main tick loop, but adds per-feature latency. |
| **Per-feature overhead** | Every placed feature now fires an event. If profiling shows a bottleneck, cache the disabled set or switch to a direct registry lookup. |
| **Determinism** | Cancelling features is deterministic as long as the plugin handler is deterministic. Avoid RNG in the handler. |
| **Cross-crate dependency** | `pumpkin-world` stays decoupled from the plugin system; only `WorldPortalExt` is extended. |

---

## Open Questions

1. Should the event include the configured feature target block(s) so plugins can filter by ore type without string matching?
2. Should cancellation be logged for debugging?
3. Should there be a separate event for structure generation vs. placed features, or is one event sufficient?
4. Should the default config disable ores globally or per-skill/biome?
