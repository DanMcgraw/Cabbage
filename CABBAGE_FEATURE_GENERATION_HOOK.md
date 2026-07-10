# Plan: Cabbage Feature Generation Hook

## Goal

Wire Cabbage up to Pumpkin's new `FeatureGenerateEvent` so the MMO module can cancel placed features (primarily overworld ores) based on a config-driven blacklist.

## Background

Pumpkin now fires a cancellable `FeatureGenerateEvent` every time the chunk population loop is about to place a feature. Cabbage can register a handler for this event and set `event.cancelled = true` to skip specific features.

Event type path:

```rust
pumpkin::plugin::api::events::world::feature_generate::FeatureGenerateEvent
```

The event carries `feature: pumpkin_data::placed_feature::PlacedFeature`. The enum exposes `fn name(&self) -> &'static str`, which returns the snake_case registry name (e.g. `"ore_coal_upper"`).

---

## Changes

### 1. Config option

**File:** `src/mmo/config.rs`

Add a `disabled_world_features` list to `MmoConfig`. Use `#[serde(default)]` so existing `config.ron` files without the field keep working.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MmoConfig {
    pub enabled: bool,
    pub skills: HashMap<SkillId, SkillConfig>,
    pub message_on_level_up: bool,
    pub save_interval_ticks: u32,
    /// Placed-feature registry names that should not generate.
    #[serde(default)]
    pub disabled_world_features: Vec<String>,
}
```

Initialize it to an empty vector in `MmoConfig::default()`:

```rust
Self {
    enabled: true,
    skills,
    message_on_level_up: true,
    save_interval_ticks: 6000,
    disabled_world_features: Vec::new(),
}
```

### 2. Event handler

**File:** `src/mmo/events.rs`

Add a handler that cancels features whose registry name is in the configured blacklist.

```rust
use pumpkin::plugin::api::events::world::feature_generate::FeatureGenerateEvent;

/// Cancel feature placement if the feature is in the MMO blacklist.
pub async fn handle_feature_generate(state: &MmoState, event: &mut FeatureGenerateEvent) {
    let feature_name = event.feature.name();
    if state.config().disabled_world_features.iter().any(|n| n == feature_name) {
        event.cancelled = true;
    }
}
```

### 3. Register the event on MmoState

**File:** `src/mmo/mod.rs`

Import the event type and implement `EventHandler<FeatureGenerateEvent>` for `MmoState`, mirroring the existing `BlockBreakEvent` and `EntityDeathEvent` handlers.

```rust
use pumpkin::plugin::api::events::{
    block::block_break::BlockBreakEvent,
    entity::entity_death::EntityDeathEvent,
    world::feature_generate::FeatureGenerateEvent,
};
```

```rust
impl EventHandler<FeatureGenerateEvent> for MmoState {
    fn handle<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut FeatureGenerateEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.is_enabled() {
                return;
            }
            events::handle_feature_generate(self, event).await;
        })
    }
}
```

### 4. Register the handler in on_load

**File:** `src/lib.rs`

Inside the `if let Some(mmo_state) = self.mmo_state.as_ref()` block in `on_load`, register the new event:

```rust
context
    .register_event::<pumpkin::plugin::api::events::world::feature_generate::FeatureGenerateEvent, _>(
        mmo_state.clone(),
        EventPriority::Normal,
        false,
    )
    .await;
```

### 5. Default ore blacklist

**File:** `config.ron`

Ship a sensible default that disables overworld ore generation. The names below match `pumpkin_data::placed_feature::PlacedFeature::name()` exactly.

```ron
disabled_world_features: [
    "ore_coal_upper",
    "ore_coal_lower",
    "ore_iron_upper",
    "ore_iron_middle",
    "ore_iron_small",
    "ore_gold",
    "ore_gold_lower",
    "ore_redstone",
    "ore_redstone_lower",
    "ore_diamond",
    "ore_diamond_large",
    "ore_diamond_buried",
    "ore_diamond_medium",
    "ore_lapis",
    "ore_lapis_buried",
    "ore_copper",
    "ore_copper_large",
],
```

> Note: `ore_emerald`, `ore_ancient_debris_large`, `ore_debris_small`, and Nether-specific ores (`ore_gold_nether`, `ore_quartz_nether`, etc.) are intentionally excluded from this default. Add or remove names as desired.

---

## Files to Modify

- `src/mmo/config.rs`
- `src/mmo/events.rs`
- `src/mmo/mod.rs`
- `src/lib.rs`
- `config.ron`

---

## Verification

1. `cargo check` passes.
2. `cargo fmt` has been run.
3. A new world generates without the blacklisted ores.
4. Removing a feature name from `disabled_world_features` and reloading config restores that feature.

---

## Open Questions

1. Should the blacklist be per-dimension or per-skill instead of global?
2. Should cancelled features be logged for debugging?
3. Should structure generation be handled separately from placed features?
