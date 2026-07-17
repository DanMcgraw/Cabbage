# Pumpkin Core Platform Plan for Cabbage MMO Skills & Perks

This document lists the Pumpkin core changes required for Cabbage's three-branch MMO skill tree: **Frontier**, **Warfare**, and **Enterprise**. Cabbage owns MMO rules, balance, XP curves, cooldowns, quests, economy, and content; Pumpkin owns the vanilla simulation and exposes stable, safe plugin primitives.

The goal is **event-driven, data-driven integration**: Cabbage registers handlers for game transactions and reads/writes namespaced persistent data through stable plugin APIs. Pumpkin must validate mutations and remain authoritative for inventory, damage, block, and entity state.

---

## 0. Skill Tree Vision, Scope, and Delivery Rules

Each discipline has 100 levels. Every level grants a small passive modifier; levels 25, 50, and 75 unlock a major perk; level 100 unlocks a capstone. Branch mastery uses the average of every discipline in its branch, with milestones at averages 25, 50, 75, and 100. Cabbage must publish the XP curve, passive caps (normally 10–25%), cooldown/resource budgets, and the single-skill XP attribution rule before implementation begins.

| Branch | Disciplines | Pumpkin platform dependencies |
|---|---|---|
| **Frontier** | Agriculture, Herbalism, Woodcutting, Mining, Excavation, Fishing, Husbandry, Taming | block/drop transactions, crop growth, item and block data, fishing loot, entity interaction/breeding, tameable ownership, batch block actions |
| **Warfare** | Blades, Axes, Archery, Unarmed, Defense, Acrobatics, Sorcery | damage and kill attribution, target validation, projectile ownership, attribute modifiers, item-use/action hooks |
| **Enterprise** | Smithing, Repair, Salvage, Alchemy, Enchanting, Tinkering, Trading, Charisma | crafting/smithing/anvil/grindstone, brewing and item consumption, item data, inventories, villager trades; NPC/reputation remains Cabbage-owned |

The old Vivarium, Crucible, Vanguard, Aetheric, and Tempest taxonomy is retired. Features formerly associated with it are retained only when they serve a discipline above: for example, crop quality belongs to Agriculture, pet abilities to Taming, combat effects to Warfare, and redstone gadgets to Enterprise Tinkering. Pumpkin must not implement a general redstone-overclock system, economy, quest system, or magic system.

### 0.1 Platform boundary

- **Pumpkin provides:** vanilla-complete mechanics, validated transactions, stable events, per-player/item/block/entity persistent data, and capability APIs.
- **Cabbage provides:** XP, levels, perks, quality rolls, custom items, mana/stamina, cooldowns, ability behaviour, NPCs, reputation, economics, and content configuration.
- **Every mutable event** must state when it fires, which fields plugins may change, cancellation/refund semantics, server-side validation, and whether it is a preview or a committed action. IDs exposed to plugins use resource locations or typed keys, not numeric IDs or unbounded strings.
- **Every ability requiring client input** must have an approved activation path before implementation (command, inventory UI, or an existing vanilla interaction). A custom keybind requires a client mod and is not assumed by this plan.

### 0.2 New foundations required by the recommended tree

1. **Namespaced persistent data:** move per-player storage from nice-to-have to phase zero. Include schema versioning, migration, quotas, async-safe access, and plugin ownership.
2. **Persistent item data:** provide namespaced ItemStack data that round-trips through inventories, containers, drops, crafting, NBT, and restarts. It stores quality, creator/provenance, Heartwood, Masterwork, runes, infusions, and anvil prior-work cost. `repair_cost` must not be a one-off field.
3. **Persistent entity data:** add namespaced entity data for tameable ownership, animal traits, and Cabbage-managed recovery state. Pumpkin implements vanilla ownership/sitting/following; Cabbage implements MMO pet commands and bonuses.
4. **Batch action primitive:** provide a bounded, permission-aware block-operation transaction with a per-block event, durability/food accounting, drops, and cancellation. This supports Timber, Vein Miner, and Earthmover without bypassing protections.
5. **Authoritative transaction hooks:** add block break/drop, crafting, smithing-table, brewing completion, furnace extraction, villager trade, entity interaction/breeding, damage, and kill-attribution events before specialised perks rely on them.

### 0.3 Feature priority

The recommended tree—not the ease of adding isolated events—sets delivery priority:

1. **Foundation:** API contract, player/item/entity data, XP attribution policy, and ability activation rules.
2. **Frontier vertical slice:** harvesting and drops, crop growth, Fishing, Husbandry/Taming interaction, then safe batch gathering.
3. **Warfare vertical slice:** damage/death attribution, projectiles, attributes, and movement/blocking hooks.
4. **Enterprise vertical slice:** functional vanilla anvil/grindstone plus crafting, brewing, enchanting, and trade hooks.
5. **Capstones and UI:** custom inventory lifecycle, quality/masterwork/runes, pet command UI, and high-complexity capstones.

This is a dependency map for the whole MMO layer; an individual section is not implementation approval until its API contract and validation tests are accepted.

### 0.4 Discipline-to-transaction map

This is the implementation source of truth for the 25/50/75/100 ability bands. It prevents an isolated core feature from being prioritised merely because it is easy to add.

| Discipline | Representative abilities | Required Pumpkin primitive |
|---|---|---|
| Agriculture | Green Thumb, Cultivated Soil, Seasonal Bounty, Bountiful Harvest | right-click harvest, crop/drop transaction, growth transition, block/item data |
| Herbalism | Forager's Eye, Careful Harvest, Natural Remedy, Master Botanist | block/drop transaction, item-use finish, optional per-player visual overlay |
| Woodcutting | Arborist, Timber, Heartwood, Forest Warden | natural-tree policy in Cabbage, batch block action, drops, item data |
| Mining | Prospector, Deep Delver, Vein Miner, Motherlode | block/drop transaction, batch action, optional per-player ore hinting |
| Excavation | Sifter, Earthmover, Archaeologist, Master Excavator | block/drop and archaeology loot transactions, batch action |
| Fishing | Angler's Sense, Careful Reel, Treasure Hunter, Master Angler | fishing loot preview/commit, mutable catch and XP |
| Husbandry | Gentle Hand, Selective Breeding, Herdkeeper, Prize Stock | entity interaction, breeding/product transaction, entity data and attributes |
| Taming | Recall Companion, Command, Bonded Spirit, Beastmaster | vanilla tameable ownership/state, entity data, controlled pet commands |
| Blades | Riposte, Lunge, Blade Dance, Cleave | target validation, melee damage/sweep transaction, activation path |
| Axes | Sundering Blow, Overhead Strike, Executioner, Colossus Breaker | melee damage, shield/armor state, activation path |
| Archery | Steady Aim, Pinning Shot, Quickload, Deadeye | projectile ownership, projectile/damage transaction, item durability |
| Unarmed | Counterstrike, Palm Strike, Grappler, Iron Fist | melee damage, knockback and equipment-durability transaction |
| Defense | Brace, Shield Bash, Second Wind, Unyielding | blocking/timed-block and damage transaction, attributes |
| Acrobatics | Safe Landing, Combat Roll, Vault, Untouchable | fall-damage and movement action hooks; activation policy |
| Sorcery | Arcane Bolt, Spellweave, Overcharge, Archmage | Cabbage-owned mana/cooldowns/spells using item-use, projectile, damage, and attribute primitives |
| Smithing | Temper, Reinforce, Artisan's Mark, Masterwork | crafting/smithing transaction, persistent item data |
| Repair | Efficient Repair, Field Repair, Careful Hands, Restorer | anvil prepare/commit, inventory transaction, persistent item data |
| Salvage | Salvager, Component Recovery, Arcane Residue, Nothing Wasted | grindstone prepare/commit, persistent item data |
| Alchemy | Batch Brewing, Concentrate, Infusion, Brewmaster | brewing completion, item-use finish, persistent item data |
| Enchanting | Arcane Appraisal, Reroll, Runic Infusion, Grand Enchanter | enchant offer/commit, anvil, persistent item data |
| Tinkering | Signal Probe, Remote Trigger, Modular Tool, Master Inventor | custom items/inventories, block metadata for plugin receivers; no global redstone timing mutation |
| Trading | Appraise, Haggler, Merchant's Eye, Trade Network | villager trade prepare/commit, Cabbage economy/contracts |
| Charisma | Persuade, Rally, Renowned, Silver Tongue | Cabbage-owned reputation/NPC systems; attributes and party effects only where Pumpkin already supports them |

---

## 1. Anvil: full repair/combine logic + events

### Why this is needed

Enterprise **Repair** and **Enchanting** both depend on the anvil. Right now Pumpkin's anvil only supports renaming; it does not implement material repair, item combining, enchantment merging, or prior-work cost tracking. Without these, Repair XP cannot be awarded, costs cannot be discounted, and high-level enchantment-combination perks cannot be implemented.

### Current state / files

- `pumpkin-inventory/src/anvil/anvil_screen_handler.rs` (231 lines)
  - `AnvilScreenHandler::new()` creates 3 slots: inputs 0/1 and output 2.
  - `update_result_slot()` at line 55 currently only handles renaming (lines 71-84).
  - `on_slot_click()` at line 181 handles taking the output at slot-index 2 (lines 189-220); it consumes XP and decrements input slot 0, but ignores slot 1.
- `WindowType::Anvil` and `WindowProperty::Anvil::RepairCost` already exist in the protocol layer.
- Experience level helpers exist on `Player` (`experience_level()`, `add_experience_levels()`).

### Implementation steps

1. **Implement the vanilla anvil algorithm in `update_result_slot()`** (around line 78, before the existing `if cost > 0` block):
   - Material repair: if input 0 is a damageable item and input 1 is the matching repair material (e.g., diamond gear + diamond), restore durability and compute cost.
   - Same-item combine: if input 0 and input 1 are the same item type, merge durability with a 12% bonus cap and combine enchantments using the standard anvil rules.
   - Enchantment merge: for each enchantment on both inputs, apply the higher level; if levels are equal, promote by one (capped at max). Add the enchantment's anvil cost from the enchantment value table.
   - Prior-work penalty: read `repair_cost` stored on each input `ItemStack` (vanilla uses a hidden component) and compute `penalty = 2^penalty_level - 1`. Sum penalties from both inputs. Increment the output's penalty by one.
   - Rename cost: keep existing `+1` rename cost; if only renaming, still show the output.
   - If no operation is possible, leave output empty and cost 0.

2. **Add hidden anvil-cost data to `ItemStack`** (or reuse an existing component if Pumpkin already has one):
   - Vanilla stores `repair_cost` as an NBT integer. Pumpkin's `ItemStack` currently stores custom name and count but may not persist this value.
   - The cheapest fix is to add a `repair_cost: i16` field to `ItemStack` and round-trip it through NBT.

3. **Fire `AnvilPrepareEvent`** immediately after the vanilla result is computed but before writing slot 2 and calling `set_repair_cost()`:
   ```rust
   pub struct AnvilPrepareEvent {
       pub player: Arc<Player>,
       pub input_first: ItemStack,
       pub input_second: ItemStack,
       pub output: ItemStack,
       pub level_cost: i32,
       pub cancelled: bool,
   }
   ```
   - Cabbage can mutate `output` and `level_cost` based on Repair skill.
   - If `cancelled` is true, clear slot 2 and set cost to 0.
   - This event is the hook for Enterprise Repair discounts and high-level Enchanting combination perks.

4. **Fire `AnvilRepairEvent`** in `on_slot_click()` slot-index-2 branch (around line 195, after confirming the player can afford the cost but before consuming XP/inputs):
   ```rust
   pub struct AnvilRepairEvent {
       pub player: Arc<Player>,
       pub input_first: ItemStack,
       pub input_second: ItemStack,
       pub output: ItemStack,
       pub level_cost: i32,
       pub cancelled: bool,
   }
   ```
   - Cabbage uses this to award Repair XP and optionally refund a portion of materials.
   - Honor cancellation: if cancelled, abort the click and call `send_content_updates()`.

5. **Re-export the events** by adding `pub mod anvil_prepare;` and `pub mod anvil_repair;` to `pumpkin/src/plugin/api/events/player/mod.rs`.

### Where to hook events

- `AnvilPrepareEvent`: `pumpkin-inventory/src/anvil/anvil_screen_handler.rs`, `update_result_slot()`, between the result computation and `self.inventory.set_stack(2, result_item).await`.
- `AnvilRepairEvent`: `pumpkin-inventory/src/anvil/anvil_screen_handler.rs`, `on_slot_click()`, inside the `slot_index == 2` branch, after the level check and before `player.add_experience_levels()` and input consumption.

### Plugin API contract

```rust
// pumpkin/src/plugin/api/events/player/anvil_prepare.rs
#[cancellable]
#[derive(Event, Clone)]
pub struct AnvilPrepareEvent {
    pub player: Arc<Player>,
    pub input_first: ItemStack,
    pub input_second: ItemStack,
    pub output: ItemStack,
    pub level_cost: i32,
}

// pumpkin/src/plugin/api/events/player/anvil_repair.rs
#[cancellable]
#[derive(Event, Clone)]
pub struct AnvilRepairEvent {
    pub player: Arc<Player>,
    pub input_first: ItemStack,
    pub input_second: ItemStack,
    pub output: ItemStack,
    pub level_cost: i32,
}
```

Both events implement `PlayerEvent`.

### Cabbage usage example

```rust
// Enterprise Repair: 5 % cost reduction per Repair level, capped at 50 %.
if let Some(repair_level) = cabbage_skill_level(event.player.uuid, "repair") {
    let discount = (event.level_cost as f32 * 0.05 * repair_level as f32).min(0.5);
    event.level_cost = (event.level_cost as f32 * (1.0 - discount)) as i32;
    cabbage_award_xp(event.player.uuid, "repair", event.level_cost as f64 * 2.0);
}
```

### Fallback until core lands

Cabbage can cancel `PlayerInteractEvent` on anvil right-clicks and reimplement a custom anvil screen with the custom inventory API (Section 11). That is heavy, so the anvil events should be the highest-priority core change.

### Additional research / pointers

- The vanilla anvil cost formula is documented on the [Minecraft Wiki](https://minecraft.wiki/w/Anvil_mechanics). The key constants are: combining two items adds the prior-work penalties, then adds the enchantment combine costs, then adds the rename cost. The output's prior-work penalty becomes `max(penalty_a, penalty_b) + 1`.
- Enchantment cost values (in level cost) are defined in vanilla data; Pumpkin may already load enchantment definitions in `pumpkin_data`. If not, hard-code a small lookup table for the common enchantments and expand later.
- The `ItemStack::repair_cost` field is the only new data model needed beyond the screen handler logic.

---

## 2. Grindstone: screen handler + salvage event

### Why this is needed

Enterprise **Salvage** turns the grindstone from an enchantment stripper into a material-recovery device. Vanilla grindstones remove enchantments and can repair items by combining two damaged items; Salvage lets high-level players recover a percentage of the base materials (e.g., iron from iron swords). Pumpkin currently has no grindstone screen handler at all, so the block is inert.

### Current state / files

- `pumpkin/src/block/blocks/grindstone.rs` (65 lines)
  - Implements `BlockBehaviour` for placement and neighbor updates only.
  - Does **not** implement `normal_use`, so right-clicking the block does nothing.
- `pumpkin-inventory/src/lib.rs` currently has no grindstone module.
- `WindowType` enum in `pumpkin_data` likely already includes `Grindstone`; if not, it must be added.

### Implementation steps

1. **Create `pumpkin-inventory/src/grindstone/grindstone_screen_handler.rs`**:
   - Define `GrindstoneScreenHandler` with 2 input slots and 1 output slot.
   - Use `WindowType::Grindstone`.
   - Implement `ScreenHandler`:
     - `update_result_slot()` computes the output:
       - If both inputs are the same item type, combine durability (vanilla: sum + 5 % bonus, capped at max).
       - Otherwise, take input 0, remove all enchantments, and set `repair_cost` to 0.
       - Compute experience reward from the sum of enchantment levels removed.
     - `on_slot_click()` for slot 2 consumes inputs and optionally gives XP.
     - `on_closed()` returns inputs to the player or drops them.

2. **Add `normal_use` to `GrindstoneBlock`**:
   - Implement `BlockBehaviour::normal_use` (or the equivalent method in Pumpkin's block API) to open the grindstone screen.
   - The method signature pattern matches other usable blocks such as crafting tables and chests.
   - Pass `player.open_handled_screen(...)` a factory that creates `GrindstoneScreenHandler`.

3. **Add `GrindstoneEvent`** fired when the output is computed (inside `update_result_slot()`):
   ```rust
   pub struct GrindstoneEvent {
       pub player: Arc<Player>,
       pub input_top: ItemStack,
       pub input_bottom: ItemStack,
       pub output: ItemStack,
       pub experience: i32,
       pub cancelled: bool,
   }
   ```
   - Cabbage can mutate `output` and `experience` based on Salvage level.
   - If cancelled, clear the output slot and set XP to 0.

4. **Optionally add `GrindstoneTakeEvent`** for the moment the player pulls the output, analogous to `AnvilRepairEvent`.

5. **Re-export the event** in `pumpkin/src/plugin/api/events/player/mod.rs`.

### Where to hook events

- `GrindstoneEvent`: `pumpkin-inventory/src/grindstone/grindstone_screen_handler.rs`, inside `update_result_slot()`, after computing the vanilla output but before writing slot 2.
- `GrindstoneTakeEvent`: inside `on_slot_click()` for `slot_index == 2`, after verifying the output exists but before removing inputs.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct GrindstoneEvent {
    pub player: Arc<Player>,
    pub input_top: ItemStack,
    pub input_bottom: ItemStack,
    pub output: ItemStack,
    pub experience: i32,
}
```

### Cabbage usage example

```rust
// Salvage: high-level Enterprise players recover base materials.
if let Some(salvage_level) = cabbage_skill_level(event.player.uuid, "salvage") {
    let chance = (salvage_level as f32 * 0.02).min(0.5);
    if let Some(material) = base_material_for(event.input_top.item) {
        if rand::random::<f32>() < chance {
            event.output = ItemStack::new(1, material);
            event.experience += salvage_level * 2;
        }
    }
    cabbage_award_xp(event.player.uuid, "salvage", event.experience as f64);
}
```

### Fallback until core lands

Cabbage can use `PlayerInteractEvent` on grindstone blocks to intercept the click, cancel it, and drop recovered materials manually. This is clunky because the vanilla grindstone UI never opens.

### Additional research / pointers

- Vanilla grindstone behavior: two inputs, one output. If both inputs are the same item, it combines durability and removes enchantments. If only one input is present, it removes enchantments and resets repair cost. The XP reward equals the sum of the removed enchantment levels, capped by the player's current XP.
- Pumpkin's `Player::open_handled_screen_direct` and `ScreenHandlerFactory` are the patterns to follow; see existing handlers such as `GenericContainerScreenHandler` for the boilerplate.
- The grindstone is a wall-mounted block; ensure `normal_use` respects facing and attach-face state so the screen opens only when the player clicks the usable face (similar to how vanilla prevents opening through the back of a furnace).

---

## 3. Enchanting table events

### Why this is needed

Enterprise **Enchanting** (including its Infusion, Runecraft, and Disenchantment specializations) needs to interact with enchantment generation. Vanilla Minecraft chooses enchantments based on bookshelves, player level, and a random seed; plugins need hooks to modify the offered list, the cost, and the final applied enchantments. Without these events, perks such as an additional compatible enchantment or lower level requirement cannot exist.

### Current state / files

- `pumpkin-inventory/src/enchanting/enchanting_screen_handler.rs`
  - `update_enchantments()` chooses three enchantment options for the three buttons.
  - `on_button_click()` applies the selected enchantment and consumes levels/lapis.
  - No events are fired at either point.

### Implementation steps

1. **Add `EnchantItemGenerateEvent`** inside `update_enchantments()` after the enchantment list is chosen but before it is sent to the client:
   ```rust
   pub struct EnchantItemGenerateEvent {
       pub player: Arc<Player>,
       pub item: ItemStack,
       pub slot: usize,             // 0, 1, or 2
       pub bookshelf_count: i32,
       pub level_requirement: i32,  // the button-level shown
       pub enchantment_id: i32,     // chosen enchantment key
       pub enchantment_level: i32,
       pub cancelled: bool,
   }
   ```
   - Cabbage can mutate `enchantment_id`, `enchantment_level`, and `level_requirement`.
   - If cancelled, hide that enchantment option (set it to empty/-1).

2. **Add `EnchantItemEvent`** inside `on_button_click()` after the enchantment list is computed but before applying it to the item:
   ```rust
   pub struct EnchantItemEvent {
       pub player: Arc<Player>,
       pub item: ItemStack,
       pub slot: usize,
       pub level_cost: i32,
       pub applied: Vec<(i32, i32)>, // (enchantment_id, level)
       pub cancelled: bool,
   }
   ```
   - Cabbage can add/remove/change enchantments and mutate the level cost.
   - If cancelled, abort the click and refund lapis/levels.

3. **Re-export both events** in `pumpkin/src/plugin/api/events/player/mod.rs`.

### Where to hook events

- `EnchantItemGenerateEvent`: `pumpkin-inventory/src/enchanting/enchanting_screen_handler.rs`, `update_enchantments()`, after the per-button enchantment is selected.
- `EnchantItemEvent`: `pumpkin-inventory/src/enchanting/enchanting_screen_handler.rs`, `on_button_click()`, after the server recomputes the enchantment for the clicked slot but before writing it onto the item and deducting lapis/levels.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct EnchantItemGenerateEvent {
    pub player: Arc<Player>,
    pub item: ItemStack,
    pub slot: usize,
    pub bookshelf_count: i32,
    pub level_requirement: i32,
    pub enchantment_id: i32,
    pub enchantment_level: i32,
}

#[cancellable]
#[derive(Event, Clone)]
pub struct EnchantItemEvent {
    pub player: Arc<Player>,
    pub item: ItemStack,
    pub slot: usize,
    pub level_cost: i32,
    pub applied: Vec<(i32, i32)>,
}
```

### Cabbage usage example

```rust
// Enchanting "Blessed Channel": level 50+ Enchanting adds a secondary enchantment.
if let Some(tinkering) = cabbage_skill_level(event.player.uuid, "tinkering") {
    if tinkering >= 50 && event.applied.len() == 1 {
        if let Some(extra) = choose_secondary_enchantment(&event.item, event.slot) {
            event.applied.push(extra);
        }
    }
    event.level_cost = (event.level_cost as f32 * (1.0 - tinkering as f32 * 0.005)).max(1.0) as i32;
}
```

### Fallback until core lands

Cabbage cannot intercept vanilla enchantment generation without the events. A workaround is to cancel `PlayerInteractEvent` on enchantment tables and replace the screen with a custom inventory that simulates the table, but reproducing the bookshelf detection and seed logic is expensive.

### Additional research / pointers

- The vanilla enchantment algorithm uses a pseudo-random seed derived from the player's enchantment seed and the item. Modifying the output predictably while staying compatible with the client is easiest if Pumpkin computes the list server-side and then fires the event before sending `CSetContainerProperty` / `CSetContainerSlot`.
- Enchantment IDs are stable resource-location strings in modern Minecraft but Pumpkin may represent them as small integers internally. Use whatever representation `Enchantment` uses in `pumpkin_data`; the event fields can be adjusted to match.

---

## 4. Taming: ownable entity component

### Why this is needed

Frontier **Taming**—including Recall Companion, Command, Bonded Spirit, and Beastmaster—depends on tamed pets. Pumpkin has `WolfEntity`, `CatEntity`, and `ParrotEntity`, but they currently store no owner UUID. `EntityTameEvent` may exist in the plugin API, but nothing fires it because there is no taming logic. Without vanilla tameable state, pets cannot follow, sit, teleport to owners, or benefit from Cabbage-managed skill bonuses.

### Current state / files

- `pumpkin/src/entity/passive/wolf.rs`, `cat.rs`, `parrot.rs`
  - These files define passive mob entities.
  - None contain owner/sitting/tamed state.
- `pumpkin/src/entity/mod.rs`
  - Defines `EntityBase` and related traits.
  - No helpers for owner UUID or tamed status.
- `EntityTameEvent` may be declared but is not wired.

### Implementation steps

1. **Define a `Tameable` component** in a new file, e.g. `pumpkin/src/entity/passive/tameable.rs`:
   ```rust
   pub struct Tameable {
       pub owner: Option<Uuid>,
       pub sitting: AtomicBool,
       pub tamed: AtomicBool,
   }
   ```
   - The component should be clone-friendly because entity snapshots may need it.

2. **Attach `Tameable` to tameable passive entities**:
   - Add a field to `WolfEntity`, `CatEntity`, and `ParrotEntity`.
   - Initialize it as untamed on spawn.

3. **Wire taming interactions**:
   - Wolf: feed bones until hearts appear, then set `tamed = true` and `owner = player.uuid`.
   - Cat/Parrot: feed the appropriate item (fish/seeds) and repeat the same pattern.
   - Fire cancellable `EntityTameEvent` after the random taming roll succeeds but before changing ownership. Commit owner/tamed state only if it is not cancelled; optionally emit a read-only `EntityTamedEvent` after commit:
     ```rust
     pub struct EntityTameEvent {
         pub animal: Arc<dyn EntityBase>,
         pub tamer: Arc<Player>,
         pub cancelled: bool,
     }
     ```

4. **Persist owner and sitting state in NBT**:
   - In `write_nbt`, serialize `Owner` and `Sitting`.
   - In `read_nbt`, reconstruct the `Tameable` component.

5. **Expose helpers on `EntityBase`**:
   ```rust
   fn owner_uuid(&self) -> Option<Uuid>;
   fn is_tamed(&self) -> bool;
   fn is_sitting(&self) -> bool;
   fn set_sitting(&self, sitting: bool);
   ```
   - Provide default implementations that return `None`/`false` for non-tameable entities.

6. **Implement basic vanilla pet AI** (required before exposing the feature):
   - Tamed mobs should follow/teleport to their owner unless sitting.
   - Sitting mobs should stay in place.
   - This can be a follow-up PR; the first milestone is just state + event.

### Where to hook events

- `EntityTameEvent`: `pumpkin/src/entity/passive/wolf.rs`, `cat.rs`, `parrot.rs`, inside the feeding/interaction handler, after the random taming roll succeeds and before setting `tamed = true` or `owner`.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct EntityTameEvent {
    pub animal: Arc<dyn EntityBase>,
    pub tamer: Arc<Player>,
}

// EntityBase extension
trait EntityBase: Send + Sync {
    fn owner_uuid(&self) -> Option<Uuid> { None }
    fn is_tamed(&self) -> bool { false }
    fn is_sitting(&self) -> bool { false }
    fn set_sitting(&self, _sitting: bool) {}
}
```

### Cabbage usage example

```rust
// Beastmaster: Cabbage applies pet bonuses from the owner's Taming level.
if let Some(taming) = cabbage_skill_level(event.tamer.uuid, "taming") {
    let bonus = (taming / 25) as f32 * 0.10;
    apply_pet_attribute_modifier(&event.animal, Attributes::MAX_HEALTH, bonus);
    apply_pet_attribute_modifier(&event.animal, Attributes::ATTACK_DAMAGE, bonus);
}
```

### Fallback until core lands

Cabbage can track a `HashMap<Uuid, Uuid>` of "pet owner" relationships in its own SQLite database and update it on `PlayerInteractEntityEvent` when the player feeds a tameable mob. This is fragile because it duplicates state that Pumpkin should own.

### Additional research / pointers

- Vanilla NBT keys are `Owner` (UUID string) and `Sitting` (bool). Keep the same keys for compatibility if Pumpkin ever reads/writes Anvil entity data.
- The taming probability is 1/3 for wolves and cats, guaranteed for parrots on first successful feed.
- For a polished Beastmaster tree, Pumpkin will eventually need a `TameCommandEvent` or similar so Cabbage can intercept `/sit`, `/follow`, and `/attack` commands bound to pets.

---

## 5. Attribute modifier API

### Why this is needed

Every parent tree grants **combined level bonuses** that should persist across logins. Status effects are temporary; skill levels are permanent. Pumpkin already has an internal attribute system (`Modifier`, `ModifierOperation`, `AttributeInstance`), but there is no public API for plugins to add/remove modifiers. Without it, Cabbage would have to reapply potion effects on every login, which is hacky and visually noisy.

### Current state / files

- `pumpkin/src/entity/attributes.rs` (323 lines)
  - Defines `ModifierOperation` (Add, MultiplyBase, MultiplyTotal).
  - Defines `Modifier { id, amount, operation }`.
  - Defines `AttributeInstance` with `base_value`, `modifiers: Vec<Modifier>`, cached value.
  - Already has `add_or_replace_modifier()` and `remove_modifier()` on `AttributeInstance`.
  - `send_attribute_updates_for_living()` already sends modifiers to clients in Java edition.

### Implementation steps

1. **Expose public helpers on `LivingEntity`** (in `pumpkin/src/entity/living.rs`):
   ```rust
   pub async fn add_attribute_modifier(&self, attribute: Attributes, modifier: Modifier);
   pub async fn remove_attribute_modifier(&self, attribute: Attributes, id: &str);
   pub async fn clear_attribute_modifiers(&self, attribute: Attributes);
   pub fn get_attribute_modifiers(&self, attribute: Attributes) -> Vec<Modifier>;
   ```
   - Internally these should lock the `attributes` map, get or create the `AttributeInstance`, call the existing `add_or_replace_modifier`/`remove_modifier`, and then call `send_attribute_updates_for_living` with the changed attribute.

2. **Add a `PersistentAttributeModifiers` component** or persist modifiers in NBT:
   - When a player logs out, serialize all modifiers that were added by plugins.
   - When a player logs in, reapply them.
   - Prefix plugin modifier IDs with the plugin name and current branch/discipline to avoid collisions, e.g. `cabbage:warfare_health_1`.

3. **Re-export `Modifier` and `ModifierOperation`** from `pumpkin/src/entity/mod.rs` or `pumpkin/src/plugin/api/mod.rs` so plugins can construct them without importing from the internal `attributes` module.

### Where to hook events

- No new event is needed in Pumpkin for modifiers; Cabbage calls the helper directly from its own level-up handlers.
- Optionally fire `AttributeModifierAddEvent` / `AttributeModifierRemoveEvent` for cross-plugin compatibility, but this is lower priority.

### Plugin API contract

```rust
// Re-exported from pumpkin::entity::attributes
pub struct Modifier {
    pub id: String,
    pub amount: f64,
    pub operation: ModifierOperation,
}

pub enum ModifierOperation {
    Add = 0,
    MultiplyBase = 1,
    MultiplyTotal = 2,
}

// LivingEntity helpers
impl LivingEntity {
    pub async fn add_attribute_modifier(&self, attribute: Attributes, modifier: Modifier);
    pub async fn remove_attribute_modifier(&self, attribute: Attributes, id: &str);
    pub async fn clear_attribute_modifiers(&self, attribute: Attributes);
}
```

### Cabbage usage example

```rust
// Warfare branch bonus: +2 max health per 10 average Warfare levels.
let warfare_average = cabbage_branch_average(player.uuid, "warfare");
let bonus = (warfare_average / 10) as f64 * 2.0;
player.living_entity.add_attribute_modifier(
    Attributes::MAX_HEALTH,
    Modifier {
        id: "cabbage:warfare_health".into(),
        amount: bonus,
        operation: ModifierOperation::Add,
    },
).await;
```

### Fallback until core lands

Cabbage can store per-player skill levels in SQLite and reapply long-duration status effects (`Health Boost`, `Strength`, etc.) on login and level-up. This works but conflicts with vanilla potion particles and durations.

### Additional research / pointers

- The attribute computation order in `AttributeInstance::value()` is already correct: sum `Add` modifiers, multiply by `1 + sum(MultiplyBase)`, then multiply by product of `1 + MultiplyTotal`. Use `MultiplyBase` for percentage bonuses that should stack additively with vanilla tool/armor bonuses; use `MultiplyTotal` for final multipliers that should compound.
- `send_attribute_updates_for_living` currently handles Java and Bedrock packets. Bedrock does not serialize modifiers in the packet (comment in the file), so Bedrock players will not see client-side attribute changes unless the server manually clamps health/damage calculations server-side. Document this limitation.

---

## 6. Item-use completion transaction

### Why this is needed

Frontier **Herbalism** and Enterprise **Alchemy** need to know exactly what item was consumed. `FoodLevelChangeEvent` only tells Cabbage the resulting food level, not the food item, potion, or golden apple. This prevents Natural Remedy, custom ingredient quality, and potion-consumption perks from being implemented safely.

### Current state / files

- `pumpkin/src/entity/living.rs`
  - `item_in_use: Mutex<Option<ItemStack>>` is set by `set_active_hand()` (line 88, 263).
  - The consumption logic is around line 2592-2597: when `item_use_time` reaches 0, the code reads `item_in_use`, checks `FoodImpl`, and applies hunger/saturation.
- `pumpkin/src/plugin/api/events/player/food_level_change.rs`
  - Only exposes `player` and `food_level`.

### Implementation steps

1. **Add `PlayerItemUseFinishEvent`** in `pumpkin/src/plugin/api/events/player/player_item_use_finish.rs`. It covers food, potions, milk buckets, and any future consumable item—not food alone:
   ```rust
   pub struct PlayerItemUseFinishEvent {
       pub player: Arc<Player>,
       pub item: ItemStack,
       pub hand: Hand,
       pub result_item: ItemStack,
       pub food_effect: Option<FoodEffect>,
   }
   ```

2. **Fire it in `LivingEntity::tick()`** around line 2596-2597, immediately after cloning `item_in_use` and before consuming the stack or applying any vanilla result:
   - Read the item from `item_in_use.lock().await.clone()`.
   - Determine the vanilla replacement item and food/potion effect.
   - Fire the event.
   - If not cancelled, validate and apply the mutated replacement item and effect.
   - Cabbage can inspect namespaced item data and potion components, while Pumpkin remains responsible for validating the final stack/effect.

3. **Re-export the event** in `pumpkin/src/plugin/api/events/player/mod.rs`. Keep `FoodLevelChangeEvent` as the post-commit hunger notification.

### Where to hook events

- `PlayerItemUseFinishEvent`: `pumpkin/src/entity/living.rs`, inside the per-tick completion block, after the vanilla result is determined and before the stack/effect is committed.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerItemUseFinishEvent {
    pub player: Arc<Player>,
    pub item: ItemStack,
    pub hand: Hand,
    pub result_item: ItemStack,
    pub food_effect: Option<FoodEffect>,
}
```

### Cabbage usage example

```rust
// Herbalism: tagged natural foods grant modest extra saturation, within the configured cap.
if event.item.has_data("cabbage:herbal_ingredient") {
    if let Some(herbalism) = cabbage_skill_level(event.player.uuid, "herbalism") {
        if let Some(food) = &mut event.food_effect {
            food.saturation += (herbalism as f32 * 0.02).min(1.0);
        }
        cabbage_award_xp(event.player.uuid, "herbalism", 5.0);
    }
}
```

### Fallback until core lands

Cabbage can listen to `FoodLevelChangeEvent`, but it cannot differentiate between eating bread versus steak. It would have to track the player's held item during `PlayerInteractEvent` and guess what was consumed, which breaks for off-hand eating, hotbar swaps, and automated feeding.

### Additional research / pointers

- The consumption tick happens server-side in `LivingEntity::tick()`. The client predicts the animation but the server is authoritative. Firing the event server-side is sufficient.
- Potions use `PotionContentsImpl` or similar data component; make sure `ItemStack` exposes enough component getters that Cabbage can identify the potion type from `event.item`.

---

## 7. Fishing event improvements

### Why this is needed

Frontier **Fishing** needs to see, and possibly replace, the caught item for Angler's Sense, Treasure Hunter, and Master Angler. The current `PlayerFishEvent` exposes the caught entity but not the fish/item. In `fishing_bobber.rs`, the caught item is created at line 117 and then discarded (`let _item_stack = ItemStack::new(1, &Item::COD);`).

### Current state / files

- `pumpkin/src/entity/projectile/fishing_bobber.rs` (343 lines)
  - `reel_in()` fires `PlayerFishEvent` for `CaughtFish` at line 93-102 but hard-codes a raw cod and discards it.
  - `owner_id` is stored as an entity ID (not UUID), which is fine internally but should also expose UUID in the event.

### Implementation steps

1. **Extend `PlayerFishEvent`** to include a mutable caught item:
   ```rust
   pub struct PlayerFishEvent {
       pub player: Arc<Player>,
       pub caught_uuid: Option<Uuid>,
       pub caught_type: String,
       pub caught_item: Option<ItemStack>,
       pub hook_uuid: Uuid,
       pub state: PlayerFishState,
       pub hand: Hand,
       pub exp_to_drop: i32,
       pub cancelled: bool,
   }
   ```

2. **Resolve loot tables in `reel_in()`** when `state == CaughtFish`:
   - Replace the hard-coded cod with a call to the fishing loot table.
   - If Pumpkin does not have loot-table support yet, add a simple deterministic fallback based on biome/water type.
   - Place the resulting `ItemStack` into `caught_item` before firing the event.

3. **Honor event mutations**:
   - If `cancelled`, do not give the item or XP.
   - If `caught_item` is changed, give the plugin's item instead.
   - If `exp_to_drop` is changed, award that much experience.

4. **Give the item to the player**:
   - Use `player.inventory().add_item(...).await` if public; otherwise drop it at the player's feet.

### Where to hook events

- `PlayerFishEvent`: `pumpkin/src/entity/projectile/fishing_bobber.rs`, `reel_in()`, inside the `CaughtFish` branch around line 92-126. Populate `caught_item` before the event and use the returned event's `caught_item` after.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerFishEvent {
    pub player: Arc<Player>,
    pub caught_uuid: Option<Uuid>,
    pub caught_type: String,
    pub caught_item: Option<ItemStack>,
    pub hook_uuid: Uuid,
    pub state: PlayerFishState,
    pub hand: Hand,
    pub exp_to_drop: i32,
}
```

### Cabbage usage example

```rust
// Treasure Hunter: Fishing perks may upgrade an eligible catch using Cabbage's configured table.
if event.state == PlayerFishState::CaughtFish {
    if let Some(fishing) = cabbage_skill_level(event.player.uuid, "fishing") {
        let chance = (fishing as f32 * 0.005).min(0.25);
        if rand::random::<f32>() < chance {
            event.caught_item = Some(choose_treasure_loot());
            cabbage_award_xp(event.player.uuid, "fishing", 50.0);
        }
    }
}
```

### Fallback until core lands

Cabbage can cancel the `CaughtFish` event and spawn its own item entity, but the player will still see the default fish because the event currently fires after the item is already decided. With the improved event, no workaround is needed.

### Additional research / pointers

- Vanilla fishing loot tables have three pools: fish, junk, and treasure. The treasure pool is only available when the bobber is in open water (5x4x5 area of water). Implementing a full loot-table parser is not required for the first iteration; a hard-coded weighted list per biome is enough.
- The XP dropped by vanilla fishing is 1-6 for fish/junk and 1-6 for treasure. Make `exp_to_drop` mutable so Cabbage can scale it.

---

## 8. Projectile ownership and deflection helpers

### Why this is needed

Warfare **Archery** needs to attribute kills to the shooter for XP, while Defense and Sorcery may redirect projectiles. Right now `ArrowEntity` stores `owner_id: Option<i32>` (entity ID), which is not stable across chunk reloads and not exposed to plugins. Thrown items (snowballs, eggs, potions) likely have the same gap.

### Current state / files

- `pumpkin/src/entity/projectile/arrow.rs`
  - `ArrowEntity` has `owner_id: Option<i32>` (line 56).
  - `new_shot()` sets `owner_id` from `shooter.entity_id` (line 105).
  - No UUID is stored or exposed.

### Implementation steps

1. **Add `owner_uuid: Option<Uuid>` to `ArrowEntity`** and other projectile entities:
   - Populate it from `shooter.entity_uuid` in `new_shot()` and similar constructors.
   - Keep `owner_id` for internal lookups if needed, but use UUID as the canonical plugin-facing identifier.

2. **Add `ProjectileEntity` trait** in `pumpkin/src/entity/projectile/mod.rs`:
   ```rust
   pub trait ProjectileEntity: EntityBase {
       fn owner_uuid(&self) -> Option<Uuid>;
       fn set_owner_uuid(&self, uuid: Option<Uuid>);
       fn base_damage(&self) -> f64;
       fn set_base_damage(&self, damage: f64);
   }
   ```

3. **Expose helpers on `EntityBase`**:
   ```rust
   fn owner_uuid(&self) -> Option<Uuid>;
   fn is_projectile(&self) -> bool;
   ```
   - Default to `None`/`false` for non-projectile entities.

4. **Fire `ProjectileDeflectEvent`** (optional but useful):
   ```rust
   pub struct ProjectileDeflectEvent {
       pub projectile: Arc<dyn EntityBase>,
       pub deflector: Arc<Player>,
       pub new_velocity: Vector3<f64>,
       pub cancelled: bool,
   }
   ```
   - Fire it when the server detects a shield/block interaction that would deflect a projectile.
   - Cabbage can change `new_velocity` to make deflected arrows return to the shooter.

### Where to hook events

- Ownership population: constructors in `pumpkin/src/entity/projectile/arrow.rs` and `thrown_item.rs`.
- `ProjectileDeflectEvent`: wherever shield/projectile collision is handled, likely in `pumpkin/src/entity/living.rs` or `pumpkin/src/entity/projectile/arrow.rs` tick logic.

### Plugin API contract

```rust
pub trait ProjectileEntity: EntityBase {
    fn owner_uuid(&self) -> Option<Uuid>;
    fn set_owner_uuid(&self, uuid: Option<Uuid>);
    fn base_damage(&self) -> f64;
    fn set_base_damage(&self, damage: f64);
}

#[cancellable]
#[derive(Event, Clone)]
pub struct ProjectileDeflectEvent {
    pub projectile: Arc<dyn EntityBase>,
    pub deflector: Arc<Player>,
    pub new_velocity: Vector3<f64>,
}
```

### Cabbage usage example

```rust
// Defense: a validated, timed block deflects arrows back at 150 % speed.
if let Some(defense) = cabbage_skill_level(event.deflector.uuid, "defense") {
    if defense >= 25 {
        event.new_velocity *= 1.5;
        cabbage_award_xp(event.deflector.uuid, "defense", 10.0);
    }
}
```

### Fallback until core lands

Cabbage can try to map entity IDs to UUIDs when projectiles are spawned, but entity IDs are reused and not exposed in a spawn event. The fallback is weak; ownership UUIDs should be added early.

### Additional research / pointers

- Vanilla projectiles store owner UUID in NBT under the key `Owner`. Align with this for save/load compatibility.
- `pumpkin/src/entity/projectile/mod.rs` already has `is_projectile()` helper; extend it or add the trait there.

---

## 9. Combat reach / weapon-class hooks

### Why this is needed

Warfare **Blades**, **Axes**, **Unarmed**, and **Defense** need to influence melee combat. `Player::attack()` is the unified path for Java melee, but a single late attack event cannot safely change both target reach and final damage; this section therefore defines separate validation and damage hooks.

### Current state / files

- `pumpkin/src/entity/player.rs`
  - `attack(&self, victim: Arc<dyn EntityBase>)` at line 1181 is the Java melee path.
  - `attack_from_bedrock(&self, victim: Arc<dyn EntityBase>)` at line 1452 handles Bedrock clients.
  - Both compute reach, weapon damage, sweeping edge, and knockback internally.

### Implementation steps

1. **Add `PlayerAttackValidateEvent`** before target acceptance, and a separate `PlayerAttackDamageEvent` before damage is applied:
   ```rust
   pub struct PlayerAttackValidateEvent {
       pub player: Arc<Player>,
       pub target: Arc<dyn EntityBase>,
       pub weapon: ItemStack,
       pub hand: Hand,
       pub maximum_reach: f64,
   }

   pub struct PlayerAttackDamageEvent {
       pub player: Arc<Player>,
       pub target: Arc<dyn EntityBase>,
       pub weapon: ItemStack,
       pub hand: Hand,
       pub base_damage: f32,
       pub final_damage: f32,
       pub sweeping: bool,
       pub knockback_multiplier: f32,
   }
   ```

2. **Fire validation before range is checked and damage after vanilla attack state is computed:**
   - Start with the vanilla maximum reach and let the validation event apply only a bounded, server-validated modifier.
   - Abort the attack if the validation event is cancelled or the target remains outside the resulting range.
   - Compute base damage and sweeping state, then fire the damage event before applying damage.
   - Clamp plugin-modified damage, sweep, and knockback to documented limits before applying them.

3. **Mirror both events in `attack_from_bedrock()`** or refactor both paths to share helpers. Java and Bedrock must have equivalent cancellation and accounting semantics.

4. **Re-export both events** in `pumpkin/src/plugin/api/events/player/mod.rs`.

### Where to hook events

- `PlayerAttackValidateEvent`: `pumpkin/src/entity/player.rs`, before the target distance check in both attack paths.
- `PlayerAttackDamageEvent`: after the vanilla attack state is calculated and before the damage application loop.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerAttackValidateEvent {
    pub player: Arc<Player>,
    pub target: Arc<dyn EntityBase>,
    pub weapon: ItemStack,
    pub hand: Hand,
    pub maximum_reach: f64,
}

#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerAttackDamageEvent {
    pub player: Arc<Player>,
    pub target: Arc<dyn EntityBase>,
    pub weapon: ItemStack,
    pub hand: Hand,
    pub base_damage: f32,
    pub final_damage: f32,
    pub sweeping: bool,
    pub knockback_multiplier: f32,
}
```

### Cabbage usage example

```rust
// Blades: Cabbage applies bounded damage and sweep modifiers from the player's level.
if let Some(blades) = cabbage_skill_level(event.player.uuid, "blades") {
    let tier = blades / 10;
    event.final_damage *= 1.0 + 0.02 * tier as f32;
    if tier >= 8 {
        event.sweeping = true; // Always sweep at high level
    }
    cabbage_award_xp(event.player.uuid, "blades", event.final_damage as f64 * 1.5);
}
```

### Fallback until core lands

Cabbage can listen to `EntityDamageByEntityEvent` and inflate damage after the fact, but it cannot change reach, sweeping, or knockback. It also cannot award skill XP based on the weapon class because the weapon is not in that event.

### Additional research / pointers

- Reach calculation in modern Minecraft uses attribute `minecraft:player.block_interaction_range` for block reach and a hard-coded 3.0 for entity reach (or 6.0 in creative). The validation event must run before this check; mutating a late damage event cannot extend reach.
- Sweeping edge logic checks attack cooldown, weapon type (sword), and whether the target is on the ground. Expose these as read-only fields if needed.

---

## 10. Generic block metadata API

### Why this is needed

Frontier **Agriculture** needs crop provenance and fertilizer state; Enterprise **Tinkering** may attach state to plugin-created receivers. Without chunk-scoped metadata, plugins must maintain external SQLite tables keyed by world + position, which is fragile when blocks are broken, replaced, or unloaded. Quality and provenance that leave the world must instead use the persistent ItemStack data API in Section 0.2.

### Current state / files

- `pumpkin-world/src/chunk/mod.rs`
  - `ChunkData` has `pending_block_entities: Mutex<FxHashMap<BlockPos, NbtCompound>>` (line 78).
  - The chunk is already serialized to NBT via `AnvilChunkFile` / `LinearV2File` / `PumpFile`.
- `pumpkin/src/world/mod.rs`
  - `World` exposes block getters/setters but no custom metadata API.

### Implementation steps

1. **Add bounded, namespaced `custom_block_data` to `ChunkData`**:
   - Key 1: `BlockPos`.
   - Key 2: namespace-prefixed metadata key (e.g., `cabbage:crop_quality`).
   - Value: a size-limited, validated NBT compound owned by that namespace. Reject unnamespaced keys, nested data beyond the configured limit, and writes that exceed the per-chunk/per-plugin quota.

2. **Add `World` helpers** in `pumpkin/src/world/mod.rs`:
   ```rust
   impl World {
       pub async fn get_block_metadata(&self, pos: BlockPos, key: &str) -> Option<NbtCompound>;
       pub async fn set_block_metadata(&self, pos: BlockPos, key: &str, value: Option<NbtCompound>);
   }
   ```
   - Internally find the chunk for `pos`, lock the map, and read/write only the calling plugin's namespace.
   - Mark the chunk dirty so it saves.

3. **Clear metadata on block break/replace**:
   - Hook into every successful block-state replacement path (`set_block`, `break_block`, explosion, piston move, etc.) and remove entries for the replaced position.
   - Vanilla block entities must not share this store. Define explicit copy/move semantics for operations such as pistons before exposing the API.

4. **Persist the metadata**:
   - Extend `ChunkNbt` (or whatever the chunk serialization struct is called) to include a `custom_data` compound tag.
   - On load, populate `custom_block_data` from the NBT.
   - On save, write it back.

5. **Expose a plugin-friendly helper on `Context`** (optional):
   ```rust
   impl Context {
       pub async fn get_block_metadata(&self, world: &World, pos: BlockPos, key: &str) -> Option<NbtCompound>;
       pub async fn set_block_metadata(&self, world: &World, pos: BlockPos, key: &str, value: Option<NbtCompound>);
   }
   ```

### Where to hook events

- No new event is required for the metadata API itself.
- Use `BlockBreakEvent` and `BlockPlaceEvent` in Cabbage to update metadata (e.g., record fertilizer when bonemeal is used).

### Plugin API contract

```rust
impl World {
    pub async fn get_block_metadata(&self, pos: BlockPos, key: &str) -> Option<NbtCompound>;
    pub async fn set_block_metadata(&self, pos: BlockPos, key: &str, value: Option<NbtCompound>);
}
```

### Cabbage usage example

```rust
// Agriculture crop quality: store fertilizer tier and a quality seed at plant time.
let mut nbt = NbtCompound::new();
nbt.insert("fertilizer_tier", NbtTag::Int(2));
nbt.insert("quality_seed", NbtTag::Long(rand::random::<i64>()));
world.set_block_metadata(pos, "cabbage:crop_quality", Some(nbt)).await;

// On harvest, read it back and roll quality.
if let Some(meta) = world.get_block_metadata(pos, "cabbage:crop_quality").await {
    let tier = meta.get_int("fertilizer_tier").unwrap_or(0);
    let seed = meta.get_long("quality_seed").unwrap_or(0);
    let quality = roll_crop_quality(tier, seed, farming_level);
}
```

### Fallback until core lands

Cabbage can maintain a `HashMap<(WorldUuid, BlockPos), NbtCompound>` in memory plus a SQLite backing table. It must manually clear entries on `BlockBreakEvent` and unload, and it must be careful about chunk unload/load ordering. The core API removes all of that bookkeeping.

### Additional research / pointers

- Stardew Valley crop quality is determined at harvest time from a formula that includes farming level, fertilizer tier, and a random roll. [Quality Fertilizer](https://stardewvalley.fandom.com/wiki/Quality_Fertilizer) significantly boosts the chance of higher-quality crops. Cabbage can replicate this by storing fertilizer tier at plant time and reusing the same seed for deterministic quality.
- Using `FxHashMap` keeps hashing fast and matches the existing `pending_block_entities` style.
- The NBT key should always be namespaced to avoid collisions between plugins. Pumpkin can enforce this by rejecting keys that do not contain a `:`.

---

## 11. Custom plugin inventory API

### Why this is needed

Several Cabbage features benefit from custom GUIs: a **skill tree menu**, a **repair preview**, a **salvage output preview**, and a **pet command wheel**. Pumpkin already has screen handlers internally, but native plugins cannot open a custom screen and receive slot clicks.

### Current state / files

- `pumpkin/src/entity/player.rs`
  - `open_handled_screen_direct` exists and opens a `ScreenHandler` for a player.
- `pumpkin-inventory/src/screen_handler/mod.rs`
  - Defines `ScreenHandler`, `ScreenHandlerBehaviour`, slots, and click routing.
- `pumpkin/src/plugin/api/context.rs`
  - `Context` exposes commands, events, services, and permissions, but no inventory helpers.
- `pumpkin/src/plugin/api/events/player/inventory_interact.rs`
  - `InventoryClickEvent` already fires for every screen click.

### Implementation steps

1. **Add a managed plugin-inventory builder** in `pumpkin/src/plugin/api/context.rs`:
   ```rust
   pub async fn open_plugin_inventory(
       &self,
       player: Arc<Player>,
       definition: PluginInventoryDefinition,
   ) -> Result<PluginInventoryHandle, PluginInventoryError>;
   ```
   - Validate that `size` is a multiple of 9 and within allowed container sizes (9-54).
   - Create a `SimpleInventory` of the requested size.
   - Create a `GenericContainerScreenHandler` wrapping the inventory.
   - Open the screen for the player via `player.open_handled_screen_direct(...)`.
   - Associate the resulting handle with the plugin and screen `sync_id`; clean it up on close, disconnect, plugin unload, or replacement.

2. **Route lifecycle events back to the plugin**:
   - Emit `PluginInventoryClickEvent` with the handle, raw slot, click action, cursor stack, clicked stack, and cancellation result.
   - Emit `PluginInventoryCloseEvent` and ensure shift-click, drag, hotbar-swap, double-click, and close semantics cannot move protected menu items.

3. **Add `PluginInventoryHandle::close`** so plugins can forcibly close only screens they own.

4. **Expose inventory mutation helpers**:
   ```rust
   pub async fn set_inventory_item(&self, player: Arc<Player>, slot: usize, item: ItemStack);
   pub async fn get_inventory_item(&self, player: Arc<Player>, slot: usize) -> ItemStack;
   ```

### Where to hook events

- Plugin inventory clicks and closes: route from the screen handler only after validating the action against the inventory definition.

### Plugin API contract

```rust
impl Context {
    pub async fn open_plugin_inventory(
        &self,
        player: Arc<Player>,
        definition: PluginInventoryDefinition,
    ) -> Result<PluginInventoryHandle, PluginInventoryError>;

    pub async fn set_inventory_item(&self, player: Arc<Player>, slot: usize, item: ItemStack);
    pub async fn get_inventory_item(&self, player: Arc<Player>, slot: usize) -> ItemStack;
}

pub struct PluginInventoryClickEvent { /* handle, action, slot, cursor, clicked item */ }
pub struct PluginInventoryCloseEvent { /* handle, player */ }
```

### Cabbage usage example

```rust
// Skill tree menu: open a protected inventory and handle its lifecycle events.
let skills_menu = context.open_plugin_inventory(player.clone(), skills_menu_definition()).await?;
register_skill_menu_handlers(skills_menu).await;
```

### Fallback until core lands

Cabbage can use chat-based menus, commands, and action-bar prompts. These are less user-friendly but do not require core changes.

### Additional research / pointers

- `WindowType::Generic9x3` is the Java protocol window type for a 27-slot chest. Use the appropriate `WindowType` based on `size`.
- Never expose a raw callback tied only to a screen ID. The handle and close event prevent callbacks and menu state from leaking after a disconnect, a different screen opening, or plugin unload.

---

## 12. Crop growth transition API (optional after Frontier MVP)

### Why this is needed

Frontier **Agriculture** and **Herbalism** want growth-boost perks. Pumpkin may already fire `BlockGrowEvent` for crops, but a general random-tick event would fire far too frequently to be a default plugin API. Prefer a targeted growth-transition event or a bounded scheduled growth helper.

### Current state / files

- `pumpkin/src/world/mod.rs`
  - The world random-tick loop iterates over chunk sections and selects blocks to tick.
- `pumpkin/src/plugin/api/events/block/block_grow.rs`
  - `BlockGrowEvent` may already exist for specific growth transitions.

### Implementation steps

1. **Extend `BlockGrowEvent` or add `BlockGrowthTransitionEvent`** in `pumpkin/src/plugin/api/events/block/block_growth_transition.rs`:
   ```rust
   pub struct BlockGrowthTransitionEvent {
       pub world: Arc<World>,
       pub old_state: BlockState,
       pub new_state: BlockState,
       pub block_position: BlockPos,
   }
   ```

2. **Fire it only when a growth transition is about to be committed.**
   - If cancelled, skip that transition.
   - Do not fire an event for every selected random tick that does not grow a block.

3. **Add a bounded `World::request_growth_transition` helper** for plugin perks. It must validate the requested block state, obey configured rate limits, and emit the same transition event.

4. **Re-export the event and helper types** in `pumpkin/src/plugin/api/events/block/mod.rs`.

### Where to hook events

- `BlockGrowthTransitionEvent`: the block-growth commit path, after the candidate next state is calculated and before it is written.

### Plugin API contract

```rust
#[cancellable]
#[derive(Event, Clone)]
pub struct BlockGrowthTransitionEvent {
    pub world: Arc<World>,
    pub old_state: BlockState,
    pub new_state: BlockState,
    pub block_position: BlockPos,
}
```

### Cabbage usage example

```rust
// Cultivated Soil: request one bounded extra growth transition for a valid crop.
if cabbage_skill_level(player.uuid, "agriculture").unwrap_or_default() >= 50 {
    world.request_growth_transition(crop_position).await?;
}
```

### Fallback until core lands

Cabbage can listen to `BlockGrowEvent` for individual growth transitions. This misses plants that do not have a distinct "grow" event (e.g., cactus height increments) but covers the common cases.

### Additional research / pointers

- Vanilla random ticks happen 3 times per chunk section per game tick by default (randomTickSpeed = 3). Do not expose an event for every tick: a growth-transition event has materially lower frequency and a clear mutation/validation point.
- Combine this with the block metadata API (Section 10) to implement fertilizer that boosts growth only on specific tilled tiles.

---

## 13. Required supporting transactions

These are not nice-to-haves: they are the generic, authoritative transactions required by the recommended Frontier, Warfare, and Enterprise disciplines. Implement them before the corresponding Cabbage perk relies on them.

### 13.1 Player harvest crop event

A dedicated `PlayerHarvestCropEvent` that fires when a player harvests a fully grown crop and includes the block position, crop state, tool, and mutable drops would let Frontier Agriculture award XP without guessing from `BlockBreakEvent` + block state.

```rust
pub struct PlayerHarvestCropEvent {
    pub player: Arc<Player>,
    pub block: &'static Block,
    pub position: BlockPos,
    pub drops: Vec<ItemStack>,
    pub cancelled: bool,
}
```

### 13.2 Player smelt / furnace extract improvements

`FurnaceExtractEvent` already exists. Make sure it exposes the smelted item, furnace position, and XP yielded for Enterprise Smithing/Alchemy materials and any future refinement content. Add analogous crafting, smithing-table, and brewing-completion transactions.

### 13.3 Entity death / kill attribution event

A `PlayerKillEntityEvent` with the killer, victim, weapon, damage source, and projectile owner would simplify XP awarding for Warfare skills.

### 13.4 Player interact entity event for feeding/breeding

`PlayerInteractEntityEvent` likely exists; verify it exposes the item in hand so Cabbage can detect bonemeal, breeding food, and taming items without separate event types.

### 13.5 Persistent per-player data API

This is a phase-zero requirement defined in Section 0.2. The API must be namespaced, schema-versioned, migration-capable, quota-limited, and safe across plugin unload/reload—not merely a JSON or SQLite helper.

### 13.6 Villager trade transaction

Add a cancellable, mutable prepare/commit pair around villager trades. It must expose the villager, player, input stacks, offered result, uses, price modifiers, and final inventory transfer. Enterprise Trading uses it for Haggler and Merchant's Eye; Enterprise Charisma uses it only for Cabbage-managed reputation or NPC rules.

### 13.7 Crafting, smithing, brewing, and batch-block transactions

Add prepare/commit pairs for crafting results, smithing-table output, brewing completion, and the bounded batch-block operation from Section 0.2. They must validate all item/block changes and expose the resulting drops and XP. These transactions are required for Smithing, Alchemy, Tinkering, Woodcutting, Mining, and Excavation.

---

## Recommended implementation order

1. **Foundation and contracts:** namespaced player/item/entity data, event mutation rules, XP attribution, ability activation, and transaction tests. Do this first; otherwise crop quality, Masterwork gear, runes, pet state, and branch progress will have no safe persistence model.
2. **Frontier vertical slice:** block break/drop and crop-harvest transactions, block metadata, Fishing, entity interaction/breeding/taming, then the bounded batch-action primitive. This implements Agriculture, Herbalism, Woodcutting, Mining, Excavation, Fishing, Husbandry, and Taming in the order their common primitives become available.
3. **Warfare vertical slice:** shared Java/Bedrock target validation, damage/death attribution, projectile ownership/deflection, attributes, and movement/blocking hooks. This supports Blades, Axes, Archery, Unarmed, Defense, Acrobatics, and Cabbage-owned Sorcery.
4. **Enterprise vertical slice:** complete vanilla anvil and grindstone flows, then crafting/smithing, brewing/item consumption, enchanting, and villager trade transactions. This supports Smithing, Repair, Salvage, Alchemy, Enchanting, Tinkering, Trading, and Cabbage-owned Charisma.
5. **Capstones and presentation:** managed custom inventories, quality/masterwork/rune content, advanced pet commands, and targeted growth transitions. Add only after the supporting transaction is proven in a vertical slice.

---

## Notes for Cabbage

Until these core changes land, Cabbage can work around most gaps:

- Use commands and action-bar prompts for early ability activation and skill menus.
- Use `BlockGrowEvent` for limited growth boosts.
- Keep temporary data in Cabbage only during prototype work; do not ship quality, traits, or progression that cannot persist safely.
- Use documented, bounded status effects only for temporary combat effects; do not use them as a replacement for permanent modifiers.

The workarounds are deliberately short-lived. The highest-value core work is the persistence/transaction foundation followed by the Frontier, Warfare, and Enterprise vertical slices; isolated screen handlers or events should not leapfrog those dependencies.

---

## Appendix: file reference cheat sheet

| Feature | Primary file(s) | Event file | Re-export file |
|---|---|---|---|
| Anvil prepare/repair | `pumpkin-inventory/src/anvil/anvil_screen_handler.rs` | `pumpkin/src/plugin/api/events/player/anvil_prepare.rs` | `pumpkin/src/plugin/api/events/player/mod.rs` |
| Grindstone | `pumpkin/src/block/blocks/grindstone.rs`, `pumpkin-inventory/src/grindstone/grindstone_screen_handler.rs` | `pumpkin/src/plugin/api/events/player/grindstone.rs` | `pumpkin/src/plugin/api/events/player/mod.rs` |
| Enchanting | `pumpkin-inventory/src/enchanting/enchanting_screen_handler.rs` | `pumpkin/src/plugin/api/events/player/enchant_item_generate.rs`, `enchant_item.rs` | `pumpkin/src/plugin/api/events/player/mod.rs` |
| Taming | `pumpkin/src/entity/passive/{wolf,cat,parrot}.rs` | `pumpkin/src/plugin/api/events/entity/entity_tame.rs` | `pumpkin/src/plugin/api/events/entity/mod.rs` |
| Attribute modifiers | `pumpkin/src/entity/living.rs`, `pumpkin/src/entity/attributes.rs` | (none) | `pumpkin/src/entity/mod.rs` |
| Player/item/entity data | player persistence, `ItemStack` component serialization, entity NBT | (none) | plugin `Context` / data API |
| Core transactions | block break/drop, crafting, smithing, brewing, villager trade | prepare/commit event pairs | relevant event modules |
| Batch block action | world block-operation service | per-block transaction event | plugin `Context` / world API |
| Item-use finish | `pumpkin/src/entity/living.rs` | `pumpkin/src/plugin/api/events/player/player_item_use_finish.rs` | `pumpkin/src/plugin/api/events/player/mod.rs` |
| Fishing | `pumpkin/src/entity/projectile/fishing_bobber.rs` | `pumpkin/src/plugin/api/events/player/fish.rs` | (already exists) |
| Projectiles | `pumpkin/src/entity/projectile/arrow.rs` | `pumpkin/src/plugin/api/events/entity/projectile_deflect.rs` | `pumpkin/src/plugin/api/events/entity/mod.rs` |
| Combat | `pumpkin/src/entity/player.rs` | `player_attack_validate.rs`, `player_attack_damage.rs` | `pumpkin/src/plugin/api/events/player/mod.rs` |
| Block metadata | `pumpkin-world/src/chunk/mod.rs`, `pumpkin/src/world/mod.rs` | (none) | (none) |
| Custom inventory | `pumpkin/src/plugin/api/context.rs` | `pumpkin/src/plugin/api/events/player/inventory_interact.rs` | (already exists) |
| Growth transition | block growth commit path | `pumpkin/src/plugin/api/events/block/block_growth_transition.rs` | `pumpkin/src/plugin/api/events/block/mod.rs` |
