# Pumpkin Plugin API Improvement Plan

## Purpose

Extend Pumpkin's native plugin API so Cabbage and other native plugins can
implement protected menus, exact progression attribution, and safe
transactional result modifiers without inferring success from pre-action
events or mutating live inventory/world state outside Pumpkin's validated
transaction paths.

This document is a Pumpkin-facing implementation plan stored in Cabbage for
coordination. It does **not** authorize edits to the sibling `Pumpkin/`
checkout from this repository. Pumpkin changes should be implemented and
reviewed in Pumpkin, then Cabbage should be recompiled against the matching
source and native API version.

## Executive summary

The highest-value work is:

1. Add first-class native plugin GUI sessions with opaque ownership, public
   open/update/close handles, and session-aware click/drag/close callbacks.
2. Establish a consistent prepare/commit transaction model and opaque
   transaction IDs for inventory and world actions.
3. Add authoritative committed events for feeding, animal-product
   collection, and bone-meal application.
4. Carry complete damage attribution into death events, including attacker,
   direct source, projectile owner, weapon snapshot, and attack kind.
5. Complete prepare/commit coverage for crafting, anvil, grindstone,
   smithing, enchanting, brewing, and villager trading.
6. Expose the produced baby in breeding events and add the remaining bounded
   primitives needed by plugins: stable attribute modifiers, projectile
   deflection/owner transfer, reach validation, and targeted crop growth.

The first four items unblock the largest correctness problems currently seen
in Cabbage. The remaining items complete the platform surface needed for the
MMO design without unsafe workarounds.

## Current API findings

The current checkout already contains useful native building blocks:

- `pumpkin::plugin::api::gui::{PluginGui, PluginInventory,
  PluginScreenHandler}` are native Rust types.
- `Player::open_handled_screen_direct` is public.
- `InventoryOpenEvent`, `InventoryClickEvent`, `InventoryDragEvent`, and
  `InventoryCloseEvent` are native events.
- `PluginScreenHandler` has `allow_grab_items` and `allow_put_items` guards,
  and Pumpkin enforces those guards across ordinary clicks, quick moves,
  swaps, throws, drags, and pickup-all actions.
- Anvil, grindstone, enchanting, crafting, brewing, breeding, block,
  projectile, damage, and death events already provide partial transaction
  coverage.

The central problem is therefore not that GUI or transaction APIs are wholly
absent. It is that native plugins do not receive stable ownership and
correlation identities, and several events describe an attempted action
rather than a successfully committed one.

### Concrete gaps

| Area | Current gap | Result for plugins |
|---|---|---|
| Native GUI | Opening requires low-level screen-handler construction; lifecycle events have player/window data but no plugin GUI/session identity. | Plugins must keep fragile per-player guesses and cannot prove an event belongs to their menu. |
| Entity interaction | `PlayerInteractEntityEvent` fires before vanilla interaction and does not report its outcome. | Clicking a full-health pet, an already-sheared sheep, or another ineligible target can be mistaken for a successful action. |
| Bone meal | Generic interaction exposes the attempt, not whether bone meal was consumed or growth occurred. | Fertilizer provenance can be recorded for failed applications. |
| Kill attribution | `EntityDeathEvent` exposes a single `killer` entity and damage type, but not the direct source, projectile owner, weapon snapshot, or authoritative attack classification. | Recent-hit caches can misclassify kills. |
| Prepare/commit pairs | Anvil and grindstone prepare/take events have no shared transaction ID. | A plugin cannot prove that a committed output is the preview it modified. |
| Crafting | `CraftItemEvent.result` is observational in this checkout. | Plugins cannot safely add namespaced provenance to the actual committed output. |
| Brewing | `BrewEvent` has no player and no post-brew output/commit identity. | Player XP and safe output metadata are not attributable. |
| Breeding | `EntityBreedEvent` provides `baby_type` and position but not the spawned baby handle. | Plugins cannot attach durable traits to the newborn. |
| Trading | No cancellable mutable offer preparation plus authoritative trade commit event. | Reputation and bounded price perks cannot safely affect exchanges. |
| Smithing | No complete mutable prepare/commit pair. | Smithing-table perks cannot validate and enrich the committed output. |

## Design principles

1. **Pumpkin remains authoritative.** A plugin may propose bounded changes in
   a prepare event, but Pumpkin validates and commits the transaction.
2. **Attempt, prepare, commit, and completion are distinct concepts.** Event
   names and documentation must say exactly which point is represented.
3. **Committed progression uses commit events only.** A commit event fires at
   most once for a successful transaction and never for a rejected or
   cancelled attempt.
4. **Every multi-stage transaction has an opaque identity.** Plugins compare
   IDs; they do not hash item stacks or infer identity from player and window
   type.
5. **Snapshots are owned.** Events may contain cloned item/block/damage
   snapshots. Plugins must not move live world/entity handles into worker
   threads.
6. **Mutation stays bounded.** Mutable fields should be narrow and validated:
   result stack, cost, experience, offer uses, price adjustment, or a bounded
   effect—not arbitrary inventory access.
7. **Cancellation has explicit semantics.** Documentation must state what is
   rolled back, what the client is resynchronized with, and whether resources
   have already been consumed.
8. **Native and WASM surfaces describe the same lifecycle.** They can use
   different representation details, but they should not disagree about
   event timing or security guarantees.
9. **Java and Bedrock behavior is tested.** GUI and transaction APIs are not
   complete if they only work correctly for one protocol edition.
10. **ABI changes are deliberate.** Changing native event structs requires a
    `PUMPKIN_API_VERSION` bump and recompilation of native plugins.

## Phase 0 — Transaction identity and event semantics

Build the shared foundation before adding more workstation-specific events.

### 0.1 Opaque identifiers

Introduce identifiers owned and generated by Pumpkin:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PluginTransactionId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PluginGuiSessionId(u64);
```

Requirements:

- IDs are unique for the relevant server process lifetime.
- Transaction IDs are allocated when a validated action or preview cycle
  begins, not by plugins.
- GUI session IDs are unique per open instance, including reopening the same
  logical menu for the same player.
- Raw values need not be stable across restarts and should not be used as
  durable database keys.
- WASM bindings use an opaque resource or integer wrapper without allowing a
  guest to forge ownership.

### 0.2 Standard transaction phases

Use consistent terminology across APIs:

- `Attempt`: input was received, but vanilla validation has not completed.
- `Prepare`: Pumpkin computed a candidate result. The event is cancellable
  and exposes explicitly mutable preview fields.
- `Commit`: Pumpkin has revalidated the transaction and is about to atomically
  consume inputs/apply costs and deliver the result. A cancellable commit event
  must run before irreversible mutation.
- `Complete`: the transaction succeeded and all vanilla state changes were
  applied. This is immutable and suitable for XP, audit, and telemetry.

Not every action needs all four public events. At minimum, any action used for
progression should expose an immutable successful completion signal. Any
plugin-modifiable result should have a prepare event and a correlated commit.

### 0.3 Shared transaction context

Prefer a compact shared context embedded in related events:

```rust
pub struct TransactionContext {
    pub id: PluginTransactionId,
    pub initiated_tick: i32,
}
```

Player-owned transactions already carry `Arc<Player>` separately. Avoid
putting arbitrary plugin data into this context; plugins can use the ID as a
key in their own bounded, expiring maps.

### 0.4 Dispatch contract

Document and enforce these rules in the native plugin API:

- Blocking handlers are the only handlers allowed to cancel or mutate an
  event.
- Mutable prepare/commit events are always fired through blocking dispatch.
- Non-blocking handlers receive the finalized immutable event snapshot and
  are for observation or offloading owned data.
- Live world/entity access is only guaranteed during the blocking/game-thread
  portion documented for that event.
- Pumpkin must not hold inventory, screen-handler, entity, or world locks
  while awaiting plugin callbacks unless the lock order is explicitly safe
  and tested.
- Completion events should be constructed after commit from owned snapshots
  so observational handlers cannot race the transaction.

### 0.5 Phase 0 acceptance criteria

- A unit test proves transaction IDs are unique and preserved across each
  prepare/commit/complete sequence.
- Cancellation at prepare and commit produces no completion event.
- Exactly one completion event fires for one successful transaction.
- Documentation identifies the phase for every existing transaction event.
- Native API version is bumped when the new fields/types become public.

## Phase 1 — First-class native plugin GUI sessions

### 1.1 Correct the API positioning

Do not describe `PluginGui` as WASM-only. The native types and low-level open
method already exist. The goal is to make them safe and first-class for native
plugins without requiring direct manipulation of player screen sync fields or
global event inference.

### 1.2 Public native open API

Add a high-level method on `Context` or a plugin-oriented player extension:

```rust
pub async fn open_plugin_gui(
    &self,
    player: Arc<Player>,
    spec: PluginGuiSpec,
    handler: Arc<dyn PluginGuiHandler>,
) -> Result<PluginGuiHandle, PluginGuiError>;
```

Suggested supporting types:

```rust
pub struct PluginGuiSpec {
    pub window_type: WindowType,
    pub title: TextComponent,
    pub slots: Vec<ItemStack>,
    pub allow_grab_items: bool,
    pub allow_put_items: bool,
}

pub struct PluginGuiHandle {
    pub session_id: PluginGuiSessionId,
    pub player_uuid: Uuid,
}
```

`PluginGuiHandle` should provide bounded operations:

- `set_slot(slot, stack)`
- `set_title(...)` only if the protocol supports safe in-place updates;
  otherwise document that reopening is required
- `refresh()` or automatic dirty-slot synchronization
- `close(reason)`
- `is_open()`

Pumpkin, not the plugin, should:

- increment and assign the screen-handler sync ID;
- construct and install `PluginScreenHandler`;
- fire/open lifecycle events in the right order;
- close or replace any previous handled screen safely;
- register the owning plugin and GUI session internally;
- clean up on close, replacement, disconnect, death, world change, plugin
  unload, and server shutdown.

### 1.3 Ownership and event attribution

Add GUI/session identity to inventory lifecycle events where applicable:

```rust
pub struct PluginGuiEventContext {
    pub session_id: PluginGuiSessionId,
    pub owner_plugin: PluginIdentity,
    pub sync_id: u8,
}
```

For ordinary vanilla screens this context is `None`. For plugin screens it is
`Some(...)` on:

- inventory open;
- click;
- drag;
- close;
- container button click;
- server-initiated replacement.

If exposing `PluginIdentity` publicly is undesirable, keep ownership routing
internal and expose only an unforgeable session ID to the owning plugin.
Global event observers may receive the session ID but must not gain mutation
rights over another plugin's GUI.

### 1.4 Direct lifecycle callbacks

Prefer routing owned GUI input directly to the registered handler instead of
requiring every plugin to register global inventory listeners:

```rust
pub trait PluginGuiHandler: Send + Sync {
    fn on_click<'a>(
        &'a self,
        context: PluginGuiClickContext,
    ) -> BoxFuture<'a, PluginGuiClickResult>;

    fn on_drag<'a>(
        &'a self,
        context: PluginGuiDragContext,
    ) -> BoxFuture<'a, PluginGuiDragResult>;

    fn on_close<'a>(
        &'a self,
        context: PluginGuiCloseContext,
    ) -> BoxFuture<'a, ()>;
}
```

The click context should include:

- session ID and player;
- logical container slot and raw view slot;
- click type, hotbar button, cursor snapshot, clicked stack snapshot;
- whether the slot belongs to the plugin container or player inventory;
- revision/sync metadata needed for diagnostics, without allowing the plugin
  to forge client state.

Return an enum such as `Cancel`, `AllowVanilla`, or a narrowly scoped plugin
action. A read-only menu should default to cancellation. Do not make plugins
manually restore cursor or slot state after cancelling.

### 1.5 Close reasons

Expose a close reason:

```rust
pub enum PluginGuiCloseReason {
    PlayerEscape,
    PlayerInventoryKey,
    Replaced,
    PluginRequested,
    Disconnect,
    Death,
    WorldChange,
    PluginUnload,
    ServerShutdown,
    ProtocolError,
}
```

The close callback fires exactly once. Replacing one plugin GUI with another
must close the first session before opening the next.

### 1.6 Security invariants

Protected menus must be correct for:

- left/right/middle click;
- shift-click in both directions;
- number-key and offhand swaps;
- drop/control-drop;
- double-click/pickup-all;
- drag start/add/end sequences;
- creative clone behavior;
- stale revision or stale sync ID packets;
- client close and server close;
- disconnect during a callback;
- two rapid opens for the same player;
- Java and Bedrock inventory mappings.

When a click is denied, Pumpkin must resynchronize the cursor and affected
slots. Items must never be duplicated, deleted, or moved into a protected
container.

### 1.7 Native/WASM convergence

Refactor the existing WASM `open_gui` host implementation to call the same
internal service used by native `open_plugin_gui`. There should be one set of
screen installation, ownership, cleanup, and security logic.

### 1.8 GUI acceptance tests

Add a test plugin or integration harness that opens a 9x3 protected menu with
button items and verifies:

- the owning plugin receives clicks for the correct session only;
- a second plugin cannot claim the session;
- all transfer modes listed above are denied without item loss/duplication;
- close fires once for every close reason;
- replacement invalidates the previous session immediately;
- stale packets from the previous sync ID cannot affect the new menu;
- unload closes all sessions owned by the unloading plugin;
- behavior is equivalent for Java and Bedrock clients where automated
  protocol tests exist.

## Phase 2 — Authoritative committed interaction events

Generic pre-interaction events should remain available for cancellation and
observation, but progression must use specific successful completion events.

### 2.1 Entity feeding

Add a feed transaction around vanilla tameable/animal feeding:

```rust
pub struct EntityFeedPrepareEvent {
    pub transaction: TransactionContext,
    pub player: Arc<Player>,
    pub target: Arc<dyn EntityBase>,
    pub hand: Hand,
    pub item: ItemStack,
    pub purpose: FeedPurpose,
    pub consume_count: u8,
    pub cancelled: bool,
}

pub struct EntityFeedCompleteEvent {
    pub transaction: TransactionContext,
    pub player: Arc<Player>,
    pub target: Arc<dyn EntityBase>,
    pub item_before: ItemStack,
    pub consumed_count: u8,
    pub outcome: FeedOutcome,
}
```

Suggested purposes/outcomes include heal, enter-love-mode, tame attempt,
age-up, trust/bond interaction, and no-effect. The completion event should
only fire when vanilla accepted the action; `consumed_count` and outcome must
reflect committed state. Ownership remains explicit through the target's
authoritative tameable state.

### 2.2 Animal-product collection

Add a transaction for actions such as milking, shearing, and mooshroom bowl
collection:

```rust
pub enum AnimalProductKind {
    Milk,
    GoatMilk,
    Shear,
    MushroomStew,
    SuspiciousStew,
    Other,
}

pub struct AnimalProductCollectCompleteEvent {
    pub transaction: TransactionContext,
    pub player: Arc<Player>,
    pub target: Arc<dyn EntityBase>,
    pub kind: AnimalProductKind,
    pub tool_before: ItemStack,
    pub tool_after: ItemStack,
    pub outputs: Vec<ItemStack>,
}
```

Fire it only after eligibility checks pass and outputs/tool mutation commit.
If plugins need to change outputs, add a cancellable prepare event whose
modified outputs are revalidated by Pumpkin.

### 2.3 Bone-meal application

Add prepare and completion events around the actual bonemeal transaction:

```rust
pub struct BoneMealApplyPrepareEvent {
    pub transaction: TransactionContext,
    pub player: Arc<Player>,
    pub world: Arc<World>,
    pub position: BlockPos,
    pub hand: Hand,
    pub block_before: &'static Block,
    pub state_before: BlockStateId,
    pub cancelled: bool,
}

pub struct BoneMealApplyCompleteEvent {
    pub transaction: TransactionContext,
    pub player: Arc<Player>,
    pub world: Arc<World>,
    pub position: BlockPos,
    pub state_before: BlockStateId,
    pub state_after: BlockStateId,
    pub consumed_count: u8,
    pub growth_occurred: bool,
}
```

The completion event must distinguish successful consumption with no visible
state change, if vanilla permits that case, from a rejected application.

### 2.4 Item-use completion semantics

Clarify `PlayerItemUseFinishEvent`: it currently fires after the use animation
but before effects and inventory changes. Keep it as a cancellable prepare
hook, and add an immutable `PlayerItemUseCompleteEvent` after consumption,
remainder placement, and effects succeed. Include item-before, item-after or
consumed count, hand, nutrition/effect summary, and transaction ID.

### 2.5 Interaction acceptance tests

- Feeding a full-health animal with an item vanilla does not consume produces
  no successful feed completion.
- Feeding an eligible animal produces exactly one completion with the correct
  consumption and outcome.
- Shearing an already-sheared sheep produces no collection completion.
- Successful shearing reports the committed drops and tool damage.
- Invalid and mature-crop bone-meal attempts report correct success semantics.
- Cancelling prepare prevents inventory consumption and completion.

## Phase 3 — Exact combat and kill attribution

### 3.1 Shared damage attribution snapshot

Introduce an owned attribution structure constructed when damage is accepted:

```rust
pub enum AttackKind {
    Melee,
    Projectile,
    Magic,
    Thorns,
    Explosion,
    Pet,
    Environment,
    Other,
}

pub struct DamageAttribution {
    pub attack_id: PluginTransactionId,
    pub kind: AttackKind,
    pub attacker: Option<Arc<dyn EntityBase>>,
    pub attacking_player: Option<Arc<Player>>,
    pub direct_source: Option<Arc<dyn EntityBase>>,
    pub projectile: Option<Arc<dyn EntityBase>>,
    pub projectile_owner: Option<Arc<dyn EntityBase>>,
    pub weapon: Option<ItemStack>,
    pub damage_type: DamageType,
}
```

If retaining live handles in the persisted last-damage record is undesirable,
store UUID/entity IDs and an owned weapon snapshot internally, then resolve
online entities only when constructing the immediate event.

### 3.2 Damage events

Expose the same attribution snapshot, or a stable subset plus attack ID, on
validation and final-damage events. The weapon snapshot must be captured from
the authoritative attack transaction, not read later from inventory.

For projectile damage, `attacking_player` resolves the owner when known,
`direct_source` is the projectile, and `weapon` is the launch weapon snapshot.
Owner transfer/deflection must update attribution through an explicit
validated API rather than ad hoc field mutation.

### 3.3 Death and player-kill events

Extend `EntityDeathEvent` with the final lethal `DamageAttribution`, or add a
dedicated immutable event:

```rust
pub struct PlayerKillEntityEvent {
    pub player: Arc<Player>,
    pub victim: Arc<dyn EntityBase>,
    pub attribution: DamageAttribution,
    pub drops: Vec<ItemStack>,
    pub dropped_exp: i32,
}
```

Preferred behavior:

- `EntityDeathEvent` remains the mutable drop/XP event if needed.
- A successful immutable kill-completion event fires exactly once after death
  attribution is fixed.
- Projectile kills identify both player owner and projectile.
- Pet kills can identify the owning player without pretending the player was
  the direct attacker.
- Environmental deaths retain a recent attacker only if Pumpkin's vanilla
  kill-credit rules actually grant that credit.
- The lethal weapon snapshot survives inventory changes between hit and
  death.

### 3.4 Combat acceptance tests

- Sword, axe, empty hand, bow, crossbow, trident, magic, thorns, pet, TNT,
  and environmental deaths report distinct correct attribution.
- An arrow kill after the shooter performs an unrelated melee attack remains
  attributed to the arrow/launch weapon.
- Two players damaging one victim credits the player selected by Pumpkin's
  vanilla kill rules.
- One death produces one kill completion even if multiple damage callbacks or
  player/entity death events are involved.
- Weapon swapping after attack does not change attribution.

## Phase 4 — Workstation transaction coverage

### 4.1 Anvil and grindstone correlation

Add the same transaction ID to each prepare and take/commit pair. Include
enough state to validate that the commit corresponds to the preview:

- player;
- screen/session identity where applicable;
- input snapshots;
- result snapshot;
- vanilla and plugin-adjusted costs;
- transaction ID.

When inputs change, allocate a new transaction ID and invalidate the previous
preview. A stale client take must not commit a prior plugin-modified result.

Fire an immutable complete event after output delivery and cost consumption
so XP/audit does not rely on a pre-commit notification.

### 4.2 Crafting

Replace or supplement observational `CraftItemEvent` with:

- `CraftItemPrepareEvent`: mutable result after recipe resolution;
- `CraftItemCommitEvent`: cancellable before consuming ingredients;
- `CraftItemCompleteEvent`: immutable after result delivery.

Include:

- recipe identifier;
- crafting window type;
- input grid snapshots;
- result stack;
- number of recipe executions and total output for shift-crafting;
- transaction ID;
- cursor/inventory destination outcome if relevant.

Pumpkin must honor namespaced custom data added to the prepared result while
preserving vanilla components and validating stack size/item identity rules.

### 4.3 Smithing table

Add `SmithingPrepareEvent`, `SmithingCommitEvent`, and
`SmithingCompleteEvent` with template/base/addition inputs, recipe ID, mutable
result, material costs, and transaction ID. This should cover upgrade and
trim recipes without plugins directly editing slots.

### 4.4 Enchanting

Correlate generated offers and the selected enchant commit with:

- enchanting session ID;
- offer generation/version ID;
- selected offer index;
- item snapshot;
- lapis and level costs before/after plugin adjustments;
- generated enchantments;
- committed enchanted output.

Regenerating offers invalidates earlier IDs. Completion fires only after the
cost and enchanted item commit.

### 4.5 Brewing

Brewing is asynchronous and may be hopper-driven, so do not invent a player
owner where none exists. Add:

- `BrewPrepareEvent`: mutable/cancellable candidate outputs, ingredient,
  fuel, position, and brew transaction ID;
- `BrewCompleteEvent`: committed input/output snapshots and transaction ID;
- `BrewingStandExtractEvent`: player, extracted potion stack/count, position,
  and originating brew transaction/provenance when known.

Player progression should normally use extraction, not a guessed "last player
who touched the stand." Automated brewing can remain unattributed.

### 4.6 Villager trading

Add a validated trade lifecycle:

```rust
pub struct VillagerTradePrepareEvent {
    pub transaction: TransactionContext,
    pub player: Arc<Player>,
    pub merchant: Arc<dyn EntityBase>,
    pub offer_index: usize,
    pub input_first: ItemStack,
    pub input_second: ItemStack,
    pub result: ItemStack,
    pub uses: u32,
    pub max_uses: u32,
    pub price_multiplier: f32,
    pub demand: i32,
    pub special_price: i32,
    pub cancelled: bool,
}
```

Plugins should adjust a narrowly bounded effective price/result preview, not
mutate the villager's durable offer list accidentally. Commit revalidates
inventory, stock, reputation, and offer version. Completion reports actual
inputs consumed, result delivered, and updated uses.

### 4.7 Workstation acceptance tests

- Prepare and complete IDs match for successful transactions.
- Changing any input invalidates the prior preview ID.
- Cancelled/stale commits consume nothing and produce no completion event.
- Plugin item custom data survives anvil, crafting, smithing, grindstone, and
  enchanting serialization where the operation is defined to preserve it.
- Shift-crafting reports exact execution/output counts and cannot multiply XP
  incorrectly.
- Hopper-driven brewing has no fabricated player; player extraction is exact.
- Trade modification cannot create negative prices, overstacked results, or
  bypass offer stock/uses.

## Phase 5 — Entity, world, and combat capability backlog

These are lower priority than the correctness work above but are required for
the full Cabbage MMO design.

### 5.1 Breeding completion with baby entity

Keep a cancellable prepare event before spawning. Add an immutable
`EntityBreedCompleteEvent` after successful spawn containing:

- transaction ID;
- both parents;
- breeder player, if any;
- the actual baby `Arc<dyn EntityBase>`;
- consumed breeding items if attributable;
- experience spawned.

Plugins can then write entity data to the baby without searching nearby
entities by type and position.

### 5.2 Targeted crop growth

Add a bounded API for requesting or modifying one growth transition:

```rust
pub async fn request_block_growth(
    &self,
    world: Arc<World>,
    position: BlockPos,
    cause: GrowthCause,
) -> Result<GrowthOutcome, GrowthError>;
```

It must reuse vanilla validation, fire cancellable growth events, avoid
simulating arbitrary random ticks, and report before/after state.

### 5.3 Stable attribute modifiers

Provide namespaced, removable modifiers with explicit scope and persistence:

- opaque modifier key namespaced to the plugin;
- operation and bounded amount;
- player/entity target;
- transient, session, timed, or persistent lifetime;
- idempotent add/update/remove;
- automatic cleanup on plugin unload for non-persistent modifiers;
- correct client synchronization.

### 5.4 Projectile deflection and ownership transfer

Add a validated transaction that updates direction, velocity, direct owner,
damage attribution, and client synchronization together. Fire prepare and
complete events. Do not expose partially updated projectile ownership.

### 5.5 Reach-aware attack validation

Expose a bounded plugin contribution during target validation rather than
letting plugins bypass validation after the fact. Pumpkin retains absolute
Java/Bedrock limits and line-of-sight rules. The event/result should explain
the vanilla reach, allowed adjustment, final reach, and rejection reason.

## API compatibility and rollout

### Native plugins

- Bump `PUMPKIN_API_VERSION` whenever public native event/layout contracts
  change.
- Require Cabbage and Pumpkin server to compile with the same stable Rust
  toolchain and Pumpkin source revision.
- Prefer compile failure over silently omitting fields or downgrading commit
  guarantees.
- Do not add a compatibility shim inside Cabbage for older transaction
  semantics.

### WASM plugins

- Add new WIT resources/events additively under the current versioning policy.
- Route native and WASM GUI opens through the same internal service.
- Ensure cancellation and mutable fields round-trip correctly; do not expose
  fields as mutable in WIT if Pumpkin ignores the returned mutation.

### Event transition strategy

- Keep old observational events temporarily when ecosystem compatibility
  requires it, but document them as attempts/previews.
- Introduce new `CompleteEvent` names rather than silently changing an old
  event from pre-commit to post-commit timing.
- Do not fire both an old and new event into the same default registration
  path in a way that encourages double XP. Migration notes must name the
  authoritative replacement.
- Add deprecation annotations and a removal target after at least one
  documented migration window, unless the native API version policy already
  permits immediate replacement.

## Proposed Pumpkin implementation areas

Exact paths may change during implementation, but expected ownership is:

```text
pumpkin/src/plugin/api/
|-- gui.rs                         # native specs, handles, sessions, callbacks
|-- transaction.rs                 # IDs, shared phase/context types
`-- events/
    |-- block/
    |   |-- bone_meal.rs
    |   `-- brew.rs                # prepare + complete semantics
    |-- entity/
    |   |-- entity_feed.rs
    |   |-- entity_product.rs
    |   |-- entity_breed.rs
    |   `-- entity_death.rs        # full damage attribution
    `-- player/
        |-- inventory_*.rs         # GUI session attribution
        |-- item_use_complete.rs
        |-- craft_item.rs
        |-- anvil_*.rs
        |-- grindstone.rs
        |-- smithing.rs
        |-- enchant_*.rs
        |-- brewing_extract.rs
        `-- villager_trade.rs

pumpkin/src/plugin/loader/wasm/
`-- ...                            # WIT adapters backed by the same services

pumpkin/src/entity/player.rs       # GUI installation and inventory lifecycle
pumpkin/src/entity/living.rs       # lethal damage attribution
pumpkin-inventory/                 # transaction-aware screen handlers
pumpkin-world/                     # validated growth and workstation commits
```

Keep the public event definitions thin. Put transaction state machines and
validation in the owning gameplay/inventory modules, not in plugin adapters.

## Verification strategy

### Unit tests

- ID uniqueness and invalidation.
- Event phase ordering and exactly-once completion.
- GUI ownership lookup and close-once behavior.
- Damage-attribution construction for every source category.
- Price/cost/result bounds.
- Item custom-data preservation.

### Integration tests

Create a small Pumpkin test plugin that records transaction IDs and attempts
mutations/cancellations. Cover:

- native GUI click modes and lifecycle;
- feed/product/bone-meal success versus failure;
- anvil/grindstone/craft/smithing/enchant/brew/trade prepare-to-complete flow;
- projectile and melee kill attribution;
- plugin unload with open GUI and active previews;
- stale client packets and changed workstation inputs.

### In-server smoke matrix

Run on the matching Pumpkin build with at least one Java and one Bedrock
client where supported:

| Area | Required smoke checks |
|---|---|
| GUI | open, click buttons, shift-click, drag, hotbar swap, double click, close, replace, disconnect, unload |
| Entity interactions | successful/failed feed, full-health pet, sheared sheep, milk/bowl actions |
| Crops | valid and invalid bone meal, mature crop, cancelled growth |
| Combat | melee, projectile, delayed projectile kill after weapon swap, pet/environment kill |
| Workstations | input changes between preview and take, cancellation, shift-craft, metadata persistence |
| Trading | price preview, insufficient input, exhausted offer, successful commit |

No phase is complete solely because event structs compile. The relevant
vanilla transaction must demonstrably honor mutation/cancellation and report
one accurate completion.

## Recommended pull-request sequence

Keep Pumpkin changes reviewable and independently testable:

1. **Transaction foundation and documentation** — IDs, phase terminology,
   dispatch contract, native API version bump scaffolding.
2. **Unified native/WASM GUI service** — high-level native open API, session
   identity, cleanup, protected-menu integration tests.
3. **Committed interaction events** — feed, product collection, bone meal,
   item-use completion.
4. **Damage attribution** — shared snapshot and exact death/kill completion.
5. **Anvil/grindstone/enchant correlation** — IDs and completion events.
6. **Crafting and smithing transactions** — honored mutable results and
   exact shift-craft counts.
7. **Brewing and extraction attribution**.
8. **Villager trade prepare/commit/complete**.
9. **Breeding baby completion event**.
10. **Secondary primitives** — growth request, attributes, projectile
    deflection, reach validation.

Each PR should include native tests, WASM parity where applicable, API docs,
and a short migration note for plugin authors.

## Cabbage adoption gates

Cabbage should enable features only after the corresponding Pumpkin contract
lands and passes a matching-server smoke test:

| Cabbage feature | Pumpkin gate |
|---|---|
| Protected skill/pet/repair/salvage menus | Native GUI session ownership and full click/drag/close lifecycle |
| Taming feed XP/bond | `EntityFeedCompleteEvent` |
| Husbandry product XP | `AnimalProductCollectCompleteEvent` |
| Fertilizer provenance | `BoneMealApplyCompleteEvent` |
| Exact Warfare kill XP | lethal `DamageAttribution` or `PlayerKillEntityEvent` |
| Repair/Salvage preview correlation | anvil/grindstone transaction IDs + completion |
| Crafted provenance/Masterwork | honored crafting prepare result + completion |
| Tinkering receivers/smithing perks | smithing prepare/commit/complete |
| Brewing XP and potion provenance | brew completion + player extraction event |
| Trading perks | villager trade prepare/commit/complete |
| Husbandry traits | breed completion with baby entity |
| Permanent stat perks | stable namespaced attribute modifiers |
| Deflection perks | projectile deflection/owner-transfer transaction |
| Reach perks | bounded reach-aware validation hook |
| Growth acceleration | validated targeted growth request |

Until a gate is met, Cabbage should leave the feature disabled and document
the platform gap rather than award XP from an attempted interaction or edit
inventory/world state directly.

## Definition of done

This Pumpkin plan is complete when:

- native plugins can open a protected GUI through a documented public API and
  receive unambiguous session-owned lifecycle callbacks;
- GUI protection is verified for every inventory transfer mode, replacement,
  disconnect, unload, Java, and Bedrock behavior;
- feeding, animal products, bone meal, and item use have authoritative
  successful completion events;
- death attribution includes the actual lethal attack, player owner where
  appropriate, direct source/projectile, weapon snapshot, and damage kind;
- workstation prepare and completion stages share Pumpkin-issued transaction
  IDs and stale previews cannot commit;
- crafting and smithing honor validated plugin-enriched result metadata;
- brewing and trading expose honest actor attribution and exact completion;
- breeding completion exposes the actual baby entity;
- all new mutable events have explicit bounds and cancellation semantics;
- native and WASM surfaces use the same underlying transaction services;
- `PUMPKIN_API_VERSION`, documentation, migration notes, unit tests,
  integration tests, and in-server Java/Bedrock smoke tests are updated;
- Cabbage recompiles against the matching Pumpkin revision without fallback
  shims and can delete its inference-based workarounds.
