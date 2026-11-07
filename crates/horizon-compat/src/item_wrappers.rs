//! NeoForge's item handlers of a `Container`, natively, as NeoForge writes them:
//! - `InvWrapper`: every slot of the container, each as it is;
//! - `SidedInvWrapper`: the slots of a `WorldlyContainer` one face reaches (every slot without a
//!   face), by that face's rules; one bottle in each of a brewing stand's bottle slots, one bucket
//!   at a time into a furnace's fuel slot;
//! - `VanillaHopperItemHandler`: a hopper's `InvWrapper`, whose fill of the empty hopper sets the
//!   hopper's cooldown, as a hopper's own transfer does.
//!
//! NeoForge gives vanilla's containers these handlers, ahead of every mod's provider
//! (`CapabilityHooks.registerVanillaProviders`): a chest (both halves of a double one), a barrel, a
//! dispenser, a dropper, a crafter, a chiseled bookshelf or a jukebox an `InvWrapper`; a furnace, a
//! blast furnace, a smoker, a brewing stand or a shulker box a `SidedInvWrapper` of the face asked
//! through; a hopper its own. Create's funnels, chutes and mechanical arms fill and empty them
//! through these. Such a container is the game's own here ([`VANILLA`]: by position, through the
//! host). A container a mod wraps itself (`new InvWrapper(this)`, as 25 mods of a Create pack do) is
//! the mod's object, whose methods are called virtually, so its own answers count.
//!
//! And NeoForge's handlers of entities' items:
//! - `RangedWrapper`: a run of another handler's slots;
//! - `PlayerMainInvWrapper`, `PlayerArmorInvWrapper` and `PlayerOffhandInvWrapper`: a player's
//!   inventory's 36 slots, its 4 armour slots (which take only what is worn there) and its off hand,
//!   each a `RangedWrapper` of the inventory's `InvWrapper`; `PlayerInvWrapper` the three as one
//!   `CombinedInvWrapper` (NeoForge's handler of a player, `Capabilities.ItemHandler.ENTITY`);
//! - `EntityHandsInvWrapper` and `EntityArmorInvWrapper`: a living entity's hands and armour through
//!   `getItemBySlot` and `setItemSlot` (together NeoForge's handler of any other living entity).
//!
//! A mod's subclass (Create's `ItemReturnInvWrapper`, a `PlayerMainInvWrapper`) reaches these through
//! its `super` calls.
//!
//! `Level.getBlockEntity` at such a container finds its block entity, of vanilla's class (a
//! `ChestBlockEntity`, a `HopperBlockEntity`): a `Container` of the same slots, by position through
//! the host (Create's threshold switch measures a chest only where a block entity is).
use crate::host::{ContainerFacts, ContainerKind};
use crate::{Data, Rt, V, api, api_common, hand, jutil, menus};

const INV: &str = "net/neoforged/neoforge/items/wrapper/InvWrapper";
const SIDED: &str = "net/neoforged/neoforge/items/wrapper/SidedInvWrapper";
const RANGED: &str = "net/neoforged/neoforge/items/wrapper/RangedWrapper";
const PLAYER_INV: &str = "net/neoforged/neoforge/items/wrapper/PlayerInvWrapper";
const PLAYER_MAIN: &str = "net/neoforged/neoforge/items/wrapper/PlayerMainInvWrapper";
const PLAYER_ARMOR: &str = "net/neoforged/neoforge/items/wrapper/PlayerArmorInvWrapper";
const PLAYER_OFFHAND: &str = "net/neoforged/neoforge/items/wrapper/PlayerOffhandInvWrapper";
const EQUIPMENT: &str = "net/neoforged/neoforge/items/wrapper/EntityEquipmentInvWrapper";
const HANDS: &str = "net/neoforged/neoforge/items/wrapper/EntityHandsInvWrapper";
const ARMOR: &str = "net/neoforged/neoforge/items/wrapper/EntityArmorInvWrapper";
const COMBINED: &str = "net/neoforged/neoforge/items/wrapper/CombinedInvWrapper";
const HOPPER: &str = "net/neoforged/neoforge/items/VanillaHopperItemHandler";
const ITEM_HANDLER: &str = "net/neoforged/neoforge/items/IItemHandler";
const MODIFIABLE: &str = "net/neoforged/neoforge/items/IItemHandlerModifiable";
/// A vanilla block's container: the game's own, by position (`$x`, `$y`, `$z`).
pub(crate) const VANILLA: &str = "compat:VanillaContainer";
const CONTAINER: &str = "net/minecraft/world/Container";
const WORLDLY: &str = "net/minecraft/world/WorldlyContainer";
const FURNACE: &str = "net/minecraft/world/level/block/entity/AbstractFurnaceBlockEntity";
const BREWING: &str = "net/minecraft/world/level/block/entity/BrewingStandBlockEntity";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";

// What the wrappers ask of a mod's container.
const C_SIZE: &str = "net/minecraft/world/Container.getContainerSize.()I";
const C_GET: &str = "net/minecraft/world/Container.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
const C_SET: &str = "net/minecraft/world/Container.setItem.(ILnet/minecraft/world/item/ItemStack;)V";
const C_REMOVE: &str = "net/minecraft/world/Container.removeItem.(II)Lnet/minecraft/world/item/ItemStack;";
const C_CHANGED: &str = "net/minecraft/world/Container.setChanged.()V";
const C_MAX: &str = "net/minecraft/world/Container.getMaxStackSize.()I";
const C_PLACE: &str = "net/minecraft/world/Container.canPlaceItem.(ILnet/minecraft/world/item/ItemStack;)Z";
const C_EMPTY: &str = "net/minecraft/world/Container.isEmpty.()Z";
const W_FACE: &str = "net/minecraft/world/WorldlyContainer.getSlotsForFace.(Lnet/minecraft/core/Direction;)[I";
const W_PLACE: &str = "net/minecraft/world/WorldlyContainer.canPlaceItemThroughFace.(ILnet/minecraft/world/item/ItemStack;Lnet/minecraft/core/Direction;)Z";
const W_TAKE: &str = "net/minecraft/world/WorldlyContainer.canTakeItemThroughFace.(ILnet/minecraft/world/item/ItemStack;Lnet/minecraft/core/Direction;)Z";
const H_CUSTOM: &str = "net/minecraft/world/level/block/entity/HopperBlockEntity.isOnCustomCooldown.()Z";
const H_COOLDOWN: &str = "net/minecraft/world/level/block/entity/HopperBlockEntity.setCooldown.(I)V";
const HASH: &str = "java/lang/Object.hashCode.()I";
// The wrappers' steps a mod's subclass may override, called virtually: (the API's key, the method's).
const SLOT_LIMIT: (&str, &str) = ("net/neoforged/neoforge/items/wrapper/InvWrapper.getSlotLimit.(I)I", "getSlotLimit.(I)I");
const SIDED_SLOT_LIMIT: (&str, &str) = ("net/neoforged/neoforge/items/wrapper/SidedInvWrapper.getSlotLimit.(I)I", "getSlotLimit.(I)I");
const GET_INV: (&str, &str) = ("net/neoforged/neoforge/items/wrapper/InvWrapper.getInv.()Lnet/minecraft/world/Container;", "getInv.()Lnet/minecraft/world/Container;");

/// Whether `c` is `class` among these types: the wrappers are item handlers (the hopper's an
/// `InvWrapper`), a vanilla block's container a `WorldlyContainer`.
pub(crate) fn is_a(c: &str, class: &str) -> bool {
    match class {
        ITEM_HANDLER | MODIFIABLE => {
            matches!(c, INV | SIDED | HOPPER | RANGED | PLAYER_INV | PLAYER_MAIN | PLAYER_ARMOR | PLAYER_OFFHAND | EQUIPMENT | HANDS | ARMOR)
        }
        INV => c == HOPPER,
        CONTAINER | "net/minecraft/world/Clearable" => c == VANILLA || CONTAINER_BLOCK_ENTITIES.contains(&c),
        WORLDLY => c == VANILLA || WORLDLY_BLOCK_ENTITIES.contains(&c),
        _ => false,
    }
}

/// 1.21.1's block entity classes that are a `Container` (a `BaseContainerBlockEntity`'s, a
/// chiseled bookshelf's, a jukebox's, a decorated pot's).
const CONTAINER_BLOCK_ENTITIES: [&str; 18] = [
    "net/minecraft/world/level/block/entity/BaseContainerBlockEntity",
    "net/minecraft/world/level/block/entity/RandomizableContainerBlockEntity",
    "net/minecraft/world/level/block/entity/ChestBlockEntity",
    "net/minecraft/world/level/block/entity/TrappedChestBlockEntity",
    "net/minecraft/world/level/block/entity/BarrelBlockEntity",
    "net/minecraft/world/level/block/entity/DispenserBlockEntity",
    "net/minecraft/world/level/block/entity/DropperBlockEntity",
    "net/minecraft/world/level/block/entity/HopperBlockEntity",
    "net/minecraft/world/level/block/entity/ShulkerBoxBlockEntity",
    "net/minecraft/world/level/block/entity/CrafterBlockEntity",
    "net/minecraft/world/level/block/entity/AbstractFurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/FurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/SmokerBlockEntity",
    "net/minecraft/world/level/block/entity/BlastFurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/BrewingStandBlockEntity",
    "net/minecraft/world/level/block/entity/ChiseledBookShelfBlockEntity",
    "net/minecraft/world/level/block/entity/JukeboxBlockEntity",
    "net/minecraft/world/level/block/entity/DecoratedPotBlockEntity",
];

/// Those of them that are a `WorldlyContainer`, whose faces reach their own slots.
const WORLDLY_BLOCK_ENTITIES: [&str; 6] = [
    "net/minecraft/world/level/block/entity/ShulkerBoxBlockEntity",
    "net/minecraft/world/level/block/entity/AbstractFurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/FurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/SmokerBlockEntity",
    "net/minecraft/world/level/block/entity/BlastFurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/BrewingStandBlockEntity",
];

/// The class of the block entity of vanilla container block `block` and its type's field in
/// `BlockEntityType`; None for a block without one.
fn block_entity_class(block: &str) -> Option<(&'static str, &'static str)> {
    let class = |name: &'static str, field: &'static str| Some((name, field));
    match block.strip_prefix("minecraft:")? {
        "trapped_chest" => class("net/minecraft/world/level/block/entity/TrappedChestBlockEntity", "TRAPPED_CHEST"),
        "barrel" => class("net/minecraft/world/level/block/entity/BarrelBlockEntity", "BARREL"),
        "dispenser" => class("net/minecraft/world/level/block/entity/DispenserBlockEntity", "DISPENSER"),
        "dropper" => class("net/minecraft/world/level/block/entity/DropperBlockEntity", "DROPPER"),
        "hopper" => class("net/minecraft/world/level/block/entity/HopperBlockEntity", "HOPPER"),
        "crafter" => class("net/minecraft/world/level/block/entity/CrafterBlockEntity", "CRAFTER"),
        "furnace" => class("net/minecraft/world/level/block/entity/FurnaceBlockEntity", "FURNACE"),
        "smoker" => class("net/minecraft/world/level/block/entity/SmokerBlockEntity", "SMOKER"),
        "blast_furnace" => class("net/minecraft/world/level/block/entity/BlastFurnaceBlockEntity", "BLAST_FURNACE"),
        "brewing_stand" => class("net/minecraft/world/level/block/entity/BrewingStandBlockEntity", "BREWING_STAND"),
        "chiseled_bookshelf" => class("net/minecraft/world/level/block/entity/ChiseledBookShelfBlockEntity", "CHISELED_BOOKSHELF"),
        "jukebox" => class("net/minecraft/world/level/block/entity/JukeboxBlockEntity", "JUKEBOX"),
        "ender_chest" => None,
        b if b.ends_with("shulker_box") => class("net/minecraft/world/level/block/entity/ShulkerBoxBlockEntity", "SHULKER_BOX"),
        // A chest, and the copper ones, which are chests.
        b if b.ends_with("chest") => class("net/minecraft/world/level/block/entity/ChestBlockEntity", "CHEST"),
        _ => None,
    }
}

/// `Level.getBlockEntity(pos)` at the vanilla container in block state `state` at `pos`: its
/// block entity, of vanilla's class, in the level with the block's state, one object for each
/// position and block. None where the game has no container ([`ContainerFacts`]).
pub(crate) fn vanilla_block_entity(rt: &mut Rt, pos: [i32; 3], state: &str) -> Option<V> {
    facts(rt, pos)?;
    let block = crate::host::split_state(state).0;
    let (class, field) = block_entity_class(block)?;
    let be = rt.intern(format!("compat:vanilla-block-entity/{},{},{}/{block}", pos[0], pos[1], pos[2]), |rt| {
        let be = rt.new_object(class);
        rt.put_field(be, "$x", V::I(pos[0]));
        rt.put_field(be, "$y", V::I(pos[1]));
        rt.put_field(be, "$z", V::I(pos[2]));
        rt.put_field(be, VANILLA_BLOCK_ENTITY, V::I(1));
        let ty = api_common::vanilla_block_entity_type(rt, field);
        let at = api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
        for (k, v) in [("$type", ty), ("type", ty), ("$pos", at), ("worldPosition", at)] {
            rt.put_field(be, k, v);
        }
        be
    });
    // The block's state and the level as they are now.
    let (st, level) = (api::state(rt, state), crate::glue::level(rt));
    for (k, v) in [("$state", st), ("blockState", st), ("level", level)] {
        rt.put_field(be, k, v);
    }
    Some(be)
}

/// The field that marks a vanilla container's block entity ([`vanilla_block_entity`]).
const VANILLA_BLOCK_ENTITY: &str = "$vanillaBlockEntity";

/// The wrappers' superclasses, which answer what a wrapper doesn't itself: a player's whole
/// handler is a `CombinedInvWrapper`, its parts `RangedWrapper`s, the hands' and the armour's
/// `EntityEquipmentInvWrapper`s.
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    match class {
        PLAYER_MAIN | PLAYER_ARMOR | PLAYER_OFFHAND => Some(RANGED),
        PLAYER_INV => Some(COMBINED),
        HANDS | ARMOR => Some(EQUIPMENT),
        _ => None,
    }
}

// --- The container ------------------------------------------------------------------------------

/// The vanilla container at `pos`: one object for each position, for as long as the runtime runs,
/// so a handler made of it twice is equal to the first.
pub(crate) fn vanilla(rt: &mut Rt, pos: [i32; 3]) -> V {
    rt.intern(format!("compat:vanilla-container/{},{},{}", pos[0], pos[1], pos[2]), |rt| {
        let c = rt.new_object(VANILLA);
        rt.put_field(c, "$x", V::I(pos[0]));
        rt.put_field(c, "$y", V::I(pos[1]));
        rt.put_field(c, "$z", V::I(pos[2]));
        c
    })
}

/// Where container `inv` is, when it is a vanilla block's (or its block entity).
fn vanilla_at(rt: &Rt, inv: V) -> Option<[i32; 3]> {
    let vanilla = inv.r() != 0 && !matches!(inv, V::Void) && (rt.class_of(inv) == VANILLA || rt.get_field(inv, VANILLA_BLOCK_ENTITY).i() != 0);
    vanilla.then(|| [rt.get_field(inv, "$x").i(), rt.get_field(inv, "$y").i(), rt.get_field(inv, "$z").i()])
}

fn facts(rt: &Rt, pos: [i32; 3]) -> Option<ContainerFacts> {
    rt.host()?.container(pos)
}

fn kind(rt: &Rt, inv: V) -> Option<ContainerKind> {
    vanilla_at(rt, inv).and_then(|p| facts(rt, p)).map(|f| f.kind)
}

/// `getContainerSize()`.
fn size(rt: &mut Rt, inv: V) -> i32 {
    match vanilla_at(rt, inv) {
        Some(p) => facts(rt, p).map_or(0, |f| f.size as i32),
        None => menus::virt(rt, C_SIZE, inv, &[]).i(),
    }
}

/// `getItem(slot)`.
fn get(rt: &mut Rt, inv: V, slot: i32) -> V {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let held = u32::try_from(slot).ok().and_then(|s| rt.host()?.container_item(p, s));
            hand::load(rt, held)
        }
        None => menus::virt(rt, C_GET, inv, &[V::I(slot)]),
    }
}

/// `setItem(slot, stack)`.
fn set(rt: &mut Rt, inv: V, slot: i32, stack: V) {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let held = hand::hand_stack_of(rt, stack);
            if let (Ok(s), Some(h)) = (u32::try_from(slot), rt.host_mut()) {
                h.set_container_item(p, s, held.as_ref());
            }
        }
        None => {
            menus::virt(rt, C_SET, inv, &[V::I(slot), stack]);
        }
    }
}

/// `setChanged()`.
fn changed(rt: &mut Rt, inv: V) {
    match vanilla_at(rt, inv) {
        Some(p) => {
            if let Some(h) = rt.host_mut() {
                h.container_changed(p);
            }
        }
        None => {
            menus::virt(rt, C_CHANGED, inv, &[]);
        }
    }
}

/// `removeItem(slot, n)`: up to `n` split off the slot's stack (`ContainerHelper.removeItem`; the
/// wrappers say `setChanged` after it).
fn remove(rt: &mut Rt, inv: V, slot: i32, n: i32) -> V {
    if vanilla_at(rt, inv).is_none() {
        return menus::virt(rt, C_REMOVE, inv, &[V::I(slot), V::I(n)]);
    }
    let there = get(rt, inv, slot);
    if api_common::stack_empty(rt, there) || n <= 0 {
        return empty(rt);
    }
    let out = split(rt, there, n);
    set(rt, inv, slot, there);
    out
}

/// `getMaxStackSize()`.
fn max(rt: &mut Rt, inv: V) -> i32 {
    match vanilla_at(rt, inv) {
        Some(p) => facts(rt, p).map_or(0, |f| f.max_stack as i32),
        None => menus::virt(rt, C_MAX, inv, &[]).i(),
    }
}

/// `canPlaceItem(slot, stack)`.
fn can_place(rt: &mut Rt, inv: V, slot: i32, stack: V) -> bool {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let held = hand::hand_stack_of(rt, stack);
            matches!((u32::try_from(slot), held, rt.host()), (Ok(s), Some(held), Some(h)) if h.container_can_place(p, s, &held))
        }
        None => menus::virt(rt, C_PLACE, inv, &[V::I(slot), stack]).i() != 0,
    }
}

/// `isEmpty()`.
fn is_empty(rt: &mut Rt, inv: V) -> bool {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let n = facts(rt, p).map_or(0, |f| f.size);
            rt.host().is_none_or(|h| (0..n).all(|s| h.container_item(p, s).is_none()))
        }
        None => menus::virt(rt, C_EMPTY, inv, &[]).i() != 0,
    }
}

/// `getSlotsForFace(side)`.
fn face_slots(rt: &mut Rt, inv: V, side: V) -> Vec<i32> {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let face = api_common::direction_ordinal(rt, side);
            rt.host().map_or_else(Vec::new, |h| h.container_face_slots(p, face).into_iter().map(|s| s as i32).collect())
        }
        None => {
            let slots = menus::virt(rt, W_FACE, inv, &[side]);
            if slots.r() == 0 { Vec::new() } else { jutil::elements(rt, slots).into_iter().map(|s| s.i()).collect() }
        }
    }
}

/// `canPlaceItemThroughFace(slot, stack, side)`.
fn can_place_through(rt: &mut Rt, inv: V, slot: i32, stack: V, side: V) -> bool {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let held = hand::hand_stack_of(rt, stack);
            matches!((u32::try_from(slot), held, rt.host()), (Ok(s), Some(held), Some(h)) if h.container_can_place_through(p, s, &held))
        }
        None => menus::virt(rt, W_PLACE, inv, &[V::I(slot), stack, side]).i() != 0,
    }
}

/// `canTakeItemThroughFace(slot, stack, side)`.
fn can_take_through(rt: &mut Rt, inv: V, slot: i32, stack: V, side: V) -> bool {
    match vanilla_at(rt, inv) {
        Some(p) => {
            let held = hand::hand_stack_of(rt, stack);
            let face = api_common::direction_ordinal(rt, side);
            matches!((u32::try_from(slot), held, rt.host()), (Ok(s), Some(held), Some(h)) if h.container_can_take(p, s, &held, face))
        }
        None => menus::virt(rt, W_TAKE, inv, &[V::I(slot), stack, side]).i() != 0,
    }
}

/// The hopper took items while it was empty: `setCooldown(8)`, unless `isOnCustomCooldown`.
fn hopper_filled(rt: &mut Rt, hopper: V) {
    match vanilla_at(rt, hopper) {
        Some(p) => {
            if let Some(h) = rt.host_mut() {
                h.hopper_filled(p);
            }
        }
        None => {
            if menus::virt(rt, H_CUSTOM, hopper, &[]).i() == 0 {
                menus::virt(rt, H_COOLDOWN, hopper, &[V::I(8)]);
            }
        }
    }
}

// --- Stacks -------------------------------------------------------------------------------------

const COUNT: &str = "net/minecraft/world/item/ItemStack.getCount.()I";
const MAX_STACK: &str = "net/minecraft/world/item/ItemStack.getMaxStackSize.()I";
const COPY: &str = "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;";
const GROW: &str = "net/minecraft/world/item/ItemStack.grow.(I)V";
const SPLIT: &str = "net/minecraft/world/item/ItemStack.split.(I)Lnet/minecraft/world/item/ItemStack;";
const SET_COUNT: &str = "net/minecraft/world/item/ItemStack.setCount.(I)V";
const SAME: &str = "net/minecraft/world/item/ItemStack.isSameItemSameComponents.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";

fn empty(rt: &mut Rt) -> V {
    api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
}

fn count(rt: &mut Rt, stack: V) -> i32 {
    api::call(rt, COUNT, &[stack]).i()
}

fn max_stack(rt: &mut Rt, stack: V) -> i32 {
    api::call(rt, MAX_STACK, &[stack]).i()
}

fn copy(rt: &mut Rt, stack: V) -> V {
    api::call(rt, COPY, &[stack])
}

fn grow(rt: &mut Rt, stack: V, n: i32) {
    api::call(rt, GROW, &[stack, V::I(n)]);
}

/// `stack.split(n)`: up to `n` taken off `stack`.
fn split(rt: &mut Rt, stack: V, n: i32) -> V {
    api::call(rt, SPLIT, &[stack, V::I(n)])
}

fn same_item(rt: &mut Rt, a: V, b: V) -> bool {
    api::call(rt, SAME, &[a, b]).i() != 0
}

fn is_bucket(rt: &Rt, stack: V) -> bool {
    !api_common::stack_empty(rt, stack) && api_common::item_like_name(rt, rt.get_field(stack, "$item")) == "minecraft:bucket"
}

// --- The wrappers -------------------------------------------------------------------------------

/// The wrapper's container: `InvWrapper.getInv()` (a mod's subclass's own answer counts), a
/// `SidedInvWrapper`'s field `inv`.
fn inv(rt: &mut Rt, w: V, sided: bool) -> V {
    if !sided && rt.is_mod_class(rt.class_of(w)) {
        return crate::tanks::virt(rt, GET_INV, &[w]);
    }
    rt.get_field(w, "inv")
}

/// `SidedInvWrapper.getSlot(inv, slot, side)`: the container's slot that the handler's slot `slot`
/// is through `side` (itself without a side); -1 for none.
fn sided_slot(rt: &mut Rt, inv: V, slot: i32, side: V) -> i32 {
    if side.r() == 0 {
        return slot;
    }
    let slots = face_slots(rt, inv, side);
    usize::try_from(slot).ok().and_then(|s| slots.get(s).copied()).unwrap_or(-1)
}

/// `getSlotLimit(slot)`: a mod's subclass's own answer counts.
fn slot_limit(rt: &mut Rt, w: V, sided: bool, slot: i32) -> i32 {
    if rt.is_mod_class(rt.class_of(w)) {
        return crate::tanks::virt(rt, if sided { SIDED_SLOT_LIMIT } else { SLOT_LIMIT }, &[w, V::I(slot)]).i();
    }
    own_slot_limit(rt, w, sided, slot)
}

/// The wrappers' own `getSlotLimit(slot)`: the container's `getMaxStackSize()`, but 1 in a
/// brewing stand's three bottle slots.
fn own_slot_limit(rt: &mut Rt, w: V, sided: bool, slot: i32) -> i32 {
    let inv = inv(rt, w, sided);
    if sided && brewing(rt, inv) {
        let side = rt.get_field(w, "side");
        if sided_slot(rt, inv, slot, side) < 3 {
            return 1;
        }
    }
    max(rt, inv)
}

fn brewing(rt: &Rt, inv: V) -> bool {
    kind(rt, inv).map_or_else(|| rt.instance_of(inv, BREWING), |k| k == ContainerKind::Brewing)
}

fn furnace(rt: &Rt, inv: V) -> bool {
    kind(rt, inv).map_or_else(|| rt.instance_of(inv, FURNACE), |k| k == ContainerKind::Furnace)
}

/// Puts `stack` in the container's `slot`: `InvWrapper` sets it, then says `setChanged`;
/// `SidedInvWrapper.setInventorySlotContents` the other way round.
fn put(rt: &mut Rt, inv: V, sided: bool, slot: i32, stack: V) {
    if sided {
        changed(rt, inv);
        set(rt, inv, slot, stack);
    } else {
        set(rt, inv, slot, stack);
        changed(rt, inv);
    }
}

/// `insertItem(slot, stack, simulate)` of either wrapper: what doesn't go in comes back.
fn insert(rt: &mut Rt, w: V, sided: bool, slot: i32, stack: V, simulate: bool) -> V {
    if api_common::stack_empty(rt, stack) {
        return empty(rt);
    }
    let inv = inv(rt, w, sided);
    let side = if sided { rt.get_field(w, "side") } else { V::NULL };
    let at = if sided { sided_slot(rt, inv, slot, side) } else { slot };
    if at == -1 {
        return stack;
    }
    let there = get(rt, inv, at);
    let placeable = |rt: &mut Rt| (!sided || can_place_through(rt, inv, at, stack, side)) && can_place(rt, inv, at, stack);
    if !api_common::stack_empty(rt, there) {
        let limit = slot_limit(rt, w, sided, slot);
        let have = count(rt, there);
        if have >= max_stack(rt, there).min(limit) || !same_item(rt, stack, there) || !placeable(rt) {
            return stack;
        }
        let m = max_stack(rt, stack).min(slot_limit(rt, w, sided, slot)) - have;
        if count(rt, stack) <= m {
            if !simulate {
                let all = copy(rt, stack);
                grow(rt, all, have);
                put(rt, inv, sided, at, all);
            }
            return empty(rt);
        }
        let rest = copy(rt, stack);
        if simulate {
            grow(rt, rest, -m);
        } else {
            let part = split(rt, rest, m);
            grow(rt, part, have);
            put(rt, inv, sided, at, part);
        }
        return rest;
    }
    if !placeable(rt) {
        return stack;
    }
    // `newStackInsertLimit`: one bucket at a time into a furnace's fuel slot.
    let m = if sided && at == 1 && is_bucket(rt, stack) && furnace(rt, inv) {
        1
    } else {
        let limit = slot_limit(rt, w, sided, slot);
        max_stack(rt, stack).min(limit)
    };
    if m < count(rt, stack) {
        let rest = copy(rt, stack);
        if simulate {
            grow(rt, rest, -m);
        } else {
            let part = split(rt, rest, m);
            put(rt, inv, sided, at, part);
        }
        return rest;
    }
    if !simulate {
        put(rt, inv, sided, at, stack);
    }
    empty(rt)
}

/// `VanillaHopperItemHandler.insertItem`: as `InvWrapper`'s, and the hopper filled while it was
/// empty waits before it moves the items on.
fn hopper_insert(rt: &mut Rt, w: V, slot: i32, stack: V, simulate: bool) -> V {
    if simulate {
        return insert(rt, w, false, slot, stack, true);
    }
    let inv = inv(rt, w, false);
    let was_empty = is_empty(rt, inv);
    let before = count(rt, stack);
    let rest = insert(rt, w, false, slot, stack, false);
    if was_empty && before > count(rt, rest) {
        let hopper = rt.get_field(w, "hopper");
        hopper_filled(rt, hopper);
    }
    rest
}

/// `extractItem(slot, amount, simulate)` of either wrapper.
fn extract(rt: &mut Rt, w: V, sided: bool, slot: i32, amount: i32, simulate: bool) -> V {
    if amount == 0 {
        return empty(rt);
    }
    let inv = inv(rt, w, sided);
    let side = if sided { rt.get_field(w, "side") } else { V::NULL };
    let at = if sided { sided_slot(rt, inv, slot, side) } else { slot };
    if at == -1 {
        return empty(rt);
    }
    let there = get(rt, inv, at);
    if api_common::stack_empty(rt, there) || (side.r() != 0 && !can_take_through(rt, inv, at, there, side)) {
        return empty(rt);
    }
    if simulate {
        let out = copy(rt, there);
        if count(rt, there) >= amount {
            api::call(rt, SET_COUNT, &[out, V::I(amount)]);
        }
        return out;
    }
    let m = count(rt, there).min(amount);
    let out = remove(rt, inv, at, m);
    changed(rt, inv);
    out
}

/// The handler made of the vanilla container at `pos` through `side` (null: none), as NeoForge's
/// providers make it; None where no vanilla block holds one.
pub(crate) fn vanilla_handler(rt: &mut Rt, pos: [i32; 3], side: V) -> Option<V> {
    let kind = facts(rt, pos)?.kind;
    let inv = vanilla(rt, pos);
    let w = match kind {
        ContainerKind::Plain => rt.new_object(INV),
        ContainerKind::Hopper => {
            let w = rt.new_object(HOPPER);
            rt.put_field(w, "hopper", inv);
            w
        }
        ContainerKind::Furnace | ContainerKind::Brewing | ContainerKind::Shulker => {
            let w = rt.new_object(SIDED);
            rt.put_field(w, "side", side);
            w
        }
    };
    rt.put_field(w, "inv", inv);
    Some(w)
}

fn wrapper_call(rt: &mut Rt, sided: bool, hopper: bool, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(w, "inv", a(1));
            if sided {
                rt.put_field(w, "side", a(2));
            } else if hopper {
                rt.put_field(w, "hopper", a(1));
            }
            V::Void
        }
        "getInv" if !sided => rt.get_field(w, "inv"),
        "getSlots" => {
            let inv = inv(rt, w, sided);
            let side = if sided { rt.get_field(w, "side") } else { V::NULL };
            V::I(if side.r() == 0 { size(rt, inv) } else { face_slots(rt, inv, side).len() as i32 })
        }
        "getStackInSlot" => {
            let inv = inv(rt, w, sided);
            let at = if sided {
                let side = rt.get_field(w, "side");
                sided_slot(rt, inv, a(1).i(), side)
            } else {
                a(1).i()
            };
            if at == -1 { empty(rt) } else { get(rt, inv, at) }
        }
        "insertItem" if hopper => hopper_insert(rt, w, a(1).i(), a(2), a(3).i() != 0),
        "insertItem" => insert(rt, w, sided, a(1).i(), a(2), a(3).i() != 0),
        "extractItem" => extract(rt, w, sided, a(1).i(), a(2).i(), a(3).i() != 0),
        "setStackInSlot" if sided => {
            let (inv, side) = (rt.get_field(w, "inv"), rt.get_field(w, "side"));
            let at = sided_slot(rt, inv, a(1).i(), side);
            if at != -1 {
                put(rt, inv, true, at, a(2));
            }
            V::Void
        }
        "setStackInSlot" => {
            let inv = inv(rt, w, false);
            set(rt, inv, a(1).i(), a(2));
            V::Void
        }
        "getSlotLimit" => V::I(own_slot_limit(rt, w, sided, a(1).i())),
        "isItemValid" => {
            let inv = inv(rt, w, sided);
            let at = if sided {
                let side = rt.get_field(w, "side");
                sided_slot(rt, inv, a(1).i(), side)
            } else {
                a(1).i()
            };
            V::I((!(sided && at == -1) && can_place(rt, inv, at, a(2))) as i32)
        }
        // Equal to a handler of the same class around the same container (and face).
        "equals" => {
            let o = a(1);
            let same = o == w
                || (o.r() != 0
                    && rt.class_of(o) == rt.class_of(w)
                    && rt.get_field(o, "inv") == rt.get_field(w, "inv")
                    && (!sided || rt.get_field(o, "side") == rt.get_field(w, "side")));
            V::I(same as i32)
        }
        "hashCode" => {
            let inv = inv(rt, w, sided);
            let h = menus::virt(rt, HASH, inv, &[]).i();
            let side = if sided { rt.get_field(w, "side") } else { V::NULL };
            let s = if side.r() == 0 { 0 } else { menus::virt(rt, HASH, side, &[]).i() };
            V::I(if sided { h.wrapping_mul(31).wrapping_add(s) } else { h })
        }
        _ => return None,
    })
}

/// A vanilla block's container's own methods (`Container`, `WorldlyContainer`), for a mod that
/// reaches it (`InvWrapper.getInv()`).
fn vanilla_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let inv = a(0);
    Some(match name {
        "getContainerSize" => V::I(size(rt, inv)),
        "getItem" => get(rt, inv, a(1).i()),
        "setItem" => {
            set(rt, inv, a(1).i(), a(2));
            V::Void
        }
        "removeItem" => {
            let out = remove(rt, inv, a(1).i(), a(2).i());
            if !api_common::stack_empty(rt, out) {
                changed(rt, inv);
            }
            out
        }
        "setChanged" => {
            changed(rt, inv);
            V::Void
        }
        "getMaxStackSize" if args.len() == 1 => V::I(max(rt, inv)),
        // (stack): `Container`'s own, the smaller of the container's and the stack's.
        "getMaxStackSize" => {
            let most = max(rt, inv);
            V::I(most.min(max_stack(rt, a(1))))
        }
        "canPlaceItem" => V::I(can_place(rt, inv, a(1).i(), a(2)) as i32),
        "isEmpty" => V::I(is_empty(rt, inv) as i32),
        "getSlotsForFace" => {
            let slots: Vec<V> = face_slots(rt, inv, a(1)).into_iter().map(V::I).collect();
            rt.alloc("[I", crate::Data::Array(slots))
        }
        "canPlaceItemThroughFace" => V::I(can_place_through(rt, inv, a(1).i(), a(2), a(3)) as i32),
        "canTakeItemThroughFace" => V::I(can_take_through(rt, inv, a(1).i(), a(2), a(3)) as i32),
        "stillValid" => V::I(1),
        _ => return None,
    })
}

// --- Entities' items ---------------------------------------------------------------------------

const EQUIPMENT_SLOT: &str = "net/minecraft/world/entity/EquipmentSlot";
const EQUIPABLE: &str = "net/minecraft/world/item/Equipable";
const EQUIPABLE_SLOT: (&str, &str) = (
    "net/minecraft/world/item/Equipable.getEquipmentSlot.()Lnet/minecraft/world/entity/EquipmentSlot;",
    "getEquipmentSlot.()Lnet/minecraft/world/entity/EquipmentSlot;",
);
const ITEM_BY_SLOT: &str = "net/minecraft/world/entity/LivingEntity.getItemBySlot.(Lnet/minecraft/world/entity/EquipmentSlot;)Lnet/minecraft/world/item/ItemStack;";
const SET_ITEM_SLOT: &str = "net/minecraft/world/entity/LivingEntity.setItemSlot.(Lnet/minecraft/world/entity/EquipmentSlot;Lnet/minecraft/world/item/ItemStack;)V";
const MATCHES: &str = "net/minecraft/world/item/ItemStack.matches.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";
const H_SET: &str = "net/neoforged/neoforge/items/IItemHandlerModifiable.setStackInSlot.(ILnet/minecraft/world/item/ItemStack;)V";
const H_VALID: &str = "net/neoforged/neoforge/items/IItemHandler.isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z";
const COMBINED_INIT: &str = "net/neoforged/neoforge/items/wrapper/CombinedInvWrapper.<init>.([Lnet/neoforged/neoforge/items/IItemHandlerModifiable;)V";
/// `EquipmentSlot.Type.HAND`'s slots and `HUMANOID_ARMOR`'s, in `EquipmentSlot.values()` order (the
/// armour's by their index: feet 0 to head 3).
const HAND_SLOTS: [&str; 2] = ["MAINHAND", "OFFHAND"];
const ARMOR_SLOTS: [&str; 4] = ["FEET", "LEGS", "CHEST", "HEAD"];
/// A player's inventory: the 36 main slots, then the 4 armour slots, then the off hand.
const MAIN_SIZE: i32 = 36;
const ARMOR_END: i32 = 40;
const OFFHAND_END: i32 = 41;

/// The equipment slot `stack` is worn in, as `LivingEntity.getEquipmentSlotForItem` finds it (an
/// `EquipmentSlot`'s name): an `Equipable` item's own (a mod's armour by its type, its own answer
/// counting), the game's for a vanilla item; the main hand for anything worn nowhere.
pub(crate) fn equipment_slot_for(rt: &mut Rt, stack: V) -> String {
    if api_common::stack_empty(rt, stack) {
        return "MAINHAND".into();
    }
    let item = rt.get_field(stack, "$item");
    // A vanilla item is its id; the game knows where it is worn.
    if let Some(id) = rt.str_of(item).map(str::to_owned) {
        let slot = rt.host().and_then(|h| h.item_equip_slot(&id));
        return match slot.as_deref() {
            Some(s @ ("head" | "chest" | "legs" | "feet" | "body" | "offhand")) => s.to_ascii_uppercase(),
            _ => "MAINHAND".into(),
        };
    }
    if item.r() != 0 && rt.instance_of(item, EQUIPABLE) {
        let slot = crate::tanks::virt(rt, EQUIPABLE_SLOT, &[item]);
        if let Some(name) = (slot.r() != 0).then(|| rt.get_field(slot, "$name")).and_then(|n| rt.str_of(n).map(str::to_owned)) {
            return name;
        }
    }
    "MAINHAND".into()
}

/// `EquipmentSlot.<name>`.
fn slot_constant(rt: &mut Rt, name: &str) -> V {
    api_common::get_static(rt, EQUIPMENT_SLOT, name).unwrap_or(V::NULL)
}

/// A new `InvWrapper` of container `inv`.
fn inv_wrapper(rt: &mut Rt, inv: V) -> V {
    let w = rt.new_object(INV);
    rt.put_field(w, "inv", inv);
    w
}

/// `new RangedWrapper(compose, min, max)`'s fields, on `w`.
fn ranged_init(rt: &mut Rt, w: V, compose: V, min: i32, max: i32) {
    rt.put_field(w, "compose", compose);
    rt.put_field(w, "minSlot", V::I(min));
    rt.put_field(w, "maxSlot", V::I(max));
}

/// `new PlayerMainInvWrapper(inv)` (and the armour's and the off hand's) on `w`: a `RangedWrapper`
/// of the inventory's `InvWrapper`.
fn player_part_init(rt: &mut Rt, w: V, class: &str, inv: V) {
    let (min, max) = match class {
        PLAYER_MAIN => (0, MAIN_SIZE),
        PLAYER_ARMOR => (MAIN_SIZE, ARMOR_END),
        _ => (ARMOR_END, OFFHAND_END),
    };
    let compose = inv_wrapper(rt, inv);
    ranged_init(rt, w, compose, min, max);
    rt.put_field(w, "inventoryPlayer", inv);
}

/// `new PlayerInvWrapper(inv)` on `w`: the main inventory's, the armour's and the off hand's
/// wrappers as one `CombinedInvWrapper`.
fn player_inv_init(rt: &mut Rt, w: V, inv: V) {
    let mut parts = Vec::with_capacity(3);
    for class in [PLAYER_MAIN, PLAYER_ARMOR, PLAYER_OFFHAND] {
        let part = rt.new_object(class);
        player_part_init(rt, part, class, inv);
        parts.push(part);
    }
    let array = rt.alloc("[Lnet/neoforged/neoforge/items/IItemHandlerModifiable;", Data::Array(parts));
    api::call(rt, COMBINED_INIT, &[w, array]);
}

/// NeoForge's item handler of a player (`Capabilities.ItemHandler.ENTITY`, registered for
/// `EntityType.PLAYER`): a `PlayerInvWrapper` of their inventory.
pub(crate) fn player_handler(rt: &mut Rt, player: V) -> V {
    let inv = menus::inventory(rt, player);
    let w = rt.new_object(PLAYER_INV);
    player_inv_init(rt, w, inv);
    w
}

/// NeoForge's fallback item handler of a living entity that isn't a player or a horse: its hands'
/// and its armour's wrappers as one `CombinedInvWrapper`.
pub(crate) fn equipment_handler(rt: &mut Rt, entity: V) -> V {
    let mut parts = Vec::with_capacity(2);
    for (class, slots) in [(HANDS, &HAND_SLOTS[..]), (ARMOR, &ARMOR_SLOTS[..])] {
        let part = rt.new_object(class);
        equipment_init(rt, part, entity, slots);
        parts.push(part);
    }
    let array = rt.alloc("[Lnet/neoforged/neoforge/items/IItemHandlerModifiable;", Data::Array(parts));
    let w = rt.new_object(COMBINED);
    api::call(rt, COMBINED_INIT, &[w, array]);
    w
}

/// Whether the armour wrapper's slot `slot` (feet 0 to head 3) takes `stack`:
/// `stack.canEquip(slot, player)`, the slot the stack is worn in being that one.
fn worn_there(rt: &mut Rt, slot: i32, stack: V) -> bool {
    let Some(want) = usize::try_from(slot).ok().and_then(|s| ARMOR_SLOTS.get(s)) else {
        return false;
    };
    !api_common::stack_empty(rt, stack) && equipment_slot_for(rt, stack) == *want
}

/// `RangedWrapper`'s calls, and its subclasses' (the player's wrappers): slot `s` is the handler's
/// slot `s + minSlot`, while that is below `maxSlot`. The armour's wrapper takes only what is worn
/// in a slot; the main inventory's wrapper answers its inventory (`getInventoryPlayer`).
fn ranged_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    match (owner, name) {
        (RANGED, "<init>") => {
            ranged_init(rt, w, a(1), a(2).i(), a(3).i());
            return Some(V::Void);
        }
        (PLAYER_MAIN | PLAYER_ARMOR | PLAYER_OFFHAND, "<init>") => {
            player_part_init(rt, w, owner, a(1));
            return Some(V::Void);
        }
        (_, "getInventoryPlayer") => return Some(rt.get_field(w, "inventoryPlayer")),
        _ => {}
    }
    let compose = rt.get_field(w, "compose");
    let (min, max) = (rt.get_field(w, "minSlot").i(), rt.get_field(w, "maxSlot").i());
    if name == "getSlots" {
        return Some(V::I(max - min));
    }
    let slot = a(1).i();
    // `checkSlot`: below the end (NeoForge checks no more).
    let inside = slot + min < max;
    let at = V::I(slot + min);
    Some(match name {
        "getStackInSlot" if inside => menus::virt(rt, H_GET, compose, &[at]),
        "getStackInSlot" => empty(rt),
        "insertItem" => {
            // `PlayerArmorInvWrapper.insertItem`, past `RangedWrapper`'s own (`super`).
            let armour = owner != RANGED && rt.instance_of(w, PLAYER_ARMOR);
            if !inside || (armour && !worn_there(rt, slot, a(2))) {
                return Some(a(2));
            }
            menus::virt(rt, H_INSERT, compose, &[at, a(2), a(3)])
        }
        "extractItem" if inside => menus::virt(rt, H_EXTRACT, compose, &[at, a(2), a(3)]),
        "extractItem" => empty(rt),
        "setStackInSlot" => {
            if inside {
                menus::virt(rt, H_SET, compose, &[at, a(2)]);
            }
            V::Void
        }
        "getSlotLimit" if inside => menus::virt(rt, H_LIMIT, compose, &[at]),
        "getSlotLimit" => V::I(0),
        "isItemValid" if inside => menus::virt(rt, H_VALID, compose, &[at, a(2)]),
        "isItemValid" => V::I(0),
        _ => return None,
    })
}

/// `new EntityEquipmentInvWrapper(entity, type)`'s fields on `w`: the entity and the slots of the
/// type, as `EquipmentSlot` constants.
fn equipment_init(rt: &mut Rt, w: V, entity: V, slots: &[&str]) {
    rt.put_field(w, "entity", entity);
    let constants: Vec<V> = slots.iter().map(|s| slot_constant(rt, s)).collect();
    let list = jutil::new_list(rt, constants);
    rt.put_field(w, "slots", list);
}

/// What `entity` holds or wears in `slot` (an `EquipmentSlot`): its own answer, a mod's entity's too.
fn worn(rt: &mut Rt, entity: V, slot: V) -> V {
    menus::virt(rt, ITEM_BY_SLOT, entity, &[slot])
}

fn wear(rt: &mut Rt, entity: V, slot: V, stack: V) {
    menus::virt(rt, SET_ITEM_SLOT, entity, &[slot, stack]);
}

fn with_count(rt: &mut Rt, stack: V, n: i32) -> V {
    let out = copy(rt, stack);
    api::call(rt, SET_COUNT, &[out, V::I(n)]);
    out
}

/// `EntityEquipmentInvWrapper`'s calls (the hands' and the armour's wrappers): each slot an
/// equipment slot of the entity's, one item in an armour slot, 99 in a hand.
fn equipment_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    if name == "<init>" {
        let slots: &[&str] = match owner {
            HANDS => &HAND_SLOTS,
            ARMOR => &ARMOR_SLOTS,
            // (entity, type): the slots of an `EquipmentSlot.Type`.
            _ => match rt.get_field(a(2), "$name") {
                n if rt.str_of(n) == Some("HAND") => &HAND_SLOTS,
                n if rt.str_of(n) == Some("HUMANOID_ARMOR") => &ARMOR_SLOTS,
                _ => &["BODY"],
            },
        };
        equipment_init(rt, w, a(1), slots);
        return Some(V::Void);
    }
    let entity = rt.get_field(w, "entity");
    let slots = jutil::elements(rt, rt.get_field(w, "slots"));
    if name == "getSlots" {
        return Some(V::I(slots.len() as i32));
    }
    if name == "isItemValid" {
        return Some(V::I(1));
    }
    if (name == "insertItem" && api_common::stack_empty(rt, a(2))) || (name == "extractItem" && a(2).i() == 0) {
        return Some(empty(rt));
    }
    let slot = a(1).i();
    // `validateSlotIndex`.
    let Some(&eq) = usize::try_from(slot).ok().and_then(|s| slots.get(s)) else {
        return Some(crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &format!("Slot {slot} not in valid range - [0,{})", slots.len())));
    };
    let eq_name = rt.str_of(rt.get_field(eq, "$name")).unwrap_or("").to_owned();
    let slot_limit = if ARMOR_SLOTS.contains(&eq_name.as_str()) { 1 } else { 99 };
    Some(match name {
        "getStackInSlot" => worn(rt, entity, eq),
        "getSlotLimit" => V::I(slot_limit),
        "insertItem" => {
            let (stack, simulate) = (a(2), a(3).i() != 0);
            let existing = worn(rt, entity, eq);
            let mut limit = slot_limit.min(max_stack(rt, stack));
            if !api_common::stack_empty(rt, existing) {
                if !same_item(rt, stack, existing) {
                    return Some(stack);
                }
                limit -= count(rt, existing);
            }
            if limit <= 0 {
                return Some(stack);
            }
            let n = count(rt, stack);
            let reached = n > limit;
            if !simulate {
                if api_common::stack_empty(rt, existing) {
                    let put = if reached { with_count(rt, stack, limit) } else { stack };
                    wear(rt, entity, eq, put);
                } else {
                    grow(rt, existing, if reached { limit } else { n });
                }
            }
            if reached { with_count(rt, stack, n - limit) } else { empty(rt) }
        }
        "extractItem" => {
            let (amount, simulate) = (a(2).i(), a(3).i() != 0);
            let existing = worn(rt, entity, eq);
            if api_common::stack_empty(rt, existing) {
                return Some(empty(rt));
            }
            let take = amount.min(max_stack(rt, existing));
            let have = count(rt, existing);
            if have <= take {
                if !simulate {
                    let none = empty(rt);
                    wear(rt, entity, eq, none);
                }
                existing
            } else {
                if !simulate {
                    let rest = with_count(rt, existing, have - take);
                    wear(rt, entity, eq, rest);
                }
                with_count(rt, existing, take)
            }
        }
        "setStackInSlot" => {
            let existing = worn(rt, entity, eq);
            if api::call(rt, MATCHES, &[existing, a(2)]).i() == 0 {
                wear(rt, entity, eq, a(2));
            }
            V::Void
        }
        _ => return None,
    })
}

// --- VanillaInventoryCodeHooks ------------------------------------------------------------------
//
// NeoForge's patches to hoppers, droppers and crafters: each asks the block it pushes into or pulls
// from for its item handler (`Capabilities.ItemHandler.BLOCK`), so a mod's machine takes and gives
// items as a chest does. The game runs its own rules for its own containers and asks these for the
// rest (a mod's block), so a hopper's own cooldown for a hopper it fills stays the game's.

const H_SLOTS: &str = "net/neoforged/neoforge/items/IItemHandler.getSlots.()I";
const H_GET: &str = "net/neoforged/neoforge/items/IItemHandler.getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;";
const H_INSERT: &str = "net/neoforged/neoforge/items/IItemHandler.insertItem.(ILnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/item/ItemStack;";
const H_EXTRACT: &str = "net/neoforged/neoforge/items/IItemHandler.extractItem.(IIZ)Lnet/minecraft/world/item/ItemStack;";
const H_LIMIT: &str = "net/neoforged/neoforge/items/IItemHandler.getSlotLimit.(I)I";
/// `Direction.DOWN`'s 3D data value.
const DOWN: u8 = 0;

/// The block next to `pos` in direction `d` (a 3D data value: down, up, north, south, west, east).
fn step(pos: [i32; 3], d: u8) -> [i32; 3] {
    let [x, y, z] = pos;
    match d {
        0 => [x, y - 1, z],
        1 => [x, y + 1, z],
        2 => [x, y, z - 1],
        3 => [x, y, z + 1],
        4 => [x - 1, y, z],
        _ => [x + 1, y, z],
    }
}

/// `getItemHandlerAt`, the block's half: the item handler at `at` reached through `side` (its
/// providers, NeoForge's own for a vanilla block first); None for none.
fn handler_at(rt: &mut Rt, at: [i32; 3], side: u8) -> Option<V> {
    let h = crate::capabilities::block_item_handler(rt, at, side);
    rt.report_thrown("getCapability");
    (h.r() != 0).then_some(h)
}

fn h_slots(rt: &mut Rt, h: V) -> i32 {
    menus::virt(rt, H_SLOTS, h, &[]).i()
}

fn h_get(rt: &mut Rt, h: V, slot: i32) -> V {
    menus::virt(rt, H_GET, h, &[V::I(slot)])
}

fn h_insert(rt: &mut Rt, h: V, slot: i32, stack: V, simulate: bool) -> V {
    menus::virt(rt, H_INSERT, h, &[V::I(slot), stack, V::I(simulate as i32)])
}

fn h_extract(rt: &mut Rt, h: V, slot: i32, n: i32, simulate: bool) -> V {
    menus::virt(rt, H_EXTRACT, h, &[V::I(slot), V::I(n), V::I(simulate as i32)])
}

/// `isFull(handler)`: every slot holds as much as the handler lets it.
fn handler_full(rt: &mut Rt, h: V) -> bool {
    for slot in 0..h_slots(rt, h) {
        let s = h_get(rt, h, slot);
        if api_common::stack_empty(rt, s) || count(rt, s) < menus::virt(rt, H_LIMIT, h, &[V::I(slot)]).i() {
            return false;
        }
    }
    true
}

/// `putStackInInventoryAllSlots`: `stack` offered to the handler's slots in order, until none is
/// left; what is left.
fn put_in_all_slots(rt: &mut Rt, h: V, mut stack: V) -> V {
    let mut slot = 0;
    while slot < h_slots(rt, h) && !api_common::stack_empty(rt, stack) {
        stack = insert_stack(rt, h, stack, slot);
        slot += 1;
    }
    stack
}

/// `insertStack`: only a stack the slot takes whole goes in, into an empty slot or onto the same
/// item and components.
fn insert_stack(rt: &mut Rt, h: V, stack: V, slot: i32) -> V {
    let there = h_get(rt, h, slot);
    let tried = h_insert(rt, h, slot, stack, true);
    if !api_common::stack_empty(rt, tried) {
        return stack;
    }
    if api_common::stack_empty(rt, there) {
        h_insert(rt, h, slot, stack, false);
        return empty(rt);
    }
    if same_item(rt, there, stack) {
        return h_insert(rt, h, slot, stack, false);
    }
    stack
}

/// `VanillaInventoryCodeHooks.insertHook`: the hopper at `pos` pushes into the item handler in
/// front (`facing`, reached through the face opposite). Nothing when the handler is full; else one
/// item of the hopper's first slot whose item the handler takes, the slot put back as it was for
/// each that it doesn't. Whether one went in; None when there is no handler in front.
pub(crate) fn insert_hook(rt: &mut Rt, pos: [i32; 3], facing: u8) -> Option<bool> {
    let h = handler_at(rt, step(pos, facing), facing ^ 1)?;
    if handler_full(rt, h) {
        return Some(false);
    }
    let hopper = vanilla(rt, pos);
    for i in 0..size(rt, hopper) {
        let there = get(rt, hopper, i);
        if api_common::stack_empty(rt, there) {
            continue;
        }
        let original = copy(rt, there);
        let one = remove(rt, hopper, i, 1);
        changed(rt, hopper);
        let rest = put_in_all_slots(rt, h, one);
        if api_common::stack_empty(rt, rest) {
            return Some(true);
        }
        set(rt, hopper, i, original);
    }
    Some(false)
}

/// `VanillaInventoryCodeHooks.extractHook`: the hopper at `pos` pulls one item from the item
/// handler above it, through that block's bottom: the first slot that gives one, into the hopper's
/// first slot that takes it (empty, or the same item and components under both limits). Whether
/// one came; None when there is no handler above (the hopper sucks up items lying there instead).
pub(crate) fn extract_hook(rt: &mut Rt, pos: [i32; 3]) -> Option<bool> {
    let h = handler_at(rt, step(pos, 1), DOWN)?;
    let hopper = vanilla(rt, pos);
    for i in 0..h_slots(rt, h) {
        let one = h_extract(rt, h, i, 1, true);
        if api_common::stack_empty(rt, one) {
            continue;
        }
        for j in 0..size(rt, hopper) {
            let dest = get(rt, hopper, j);
            let fits = can_place(rt, hopper, j, one)
                && (api_common::stack_empty(rt, dest)
                    || (count(rt, dest) < max_stack(rt, dest) && count(rt, dest) < max(rt, hopper) && same_item(rt, one, dest)));
            if !fits {
                continue;
            }
            let taken = h_extract(rt, h, i, 1, false);
            if api_common::stack_empty(rt, dest) {
                set(rt, hopper, j, taken);
            } else {
                grow(rt, dest, 1);
                set(rt, hopper, j, dest);
            }
            changed(rt, hopper);
            return Some(true);
        }
    }
    Some(false)
}

/// `VanillaInventoryCodeHooks.dropperInsertHook`: the dropper at `pos` facing `facing` puts one of
/// slot `slot`'s items into the item handler in front; the slot loses it when it went in and keeps
/// it when it didn't (nothing drops). False when there is no handler in front: the dropper drops
/// the item as usual.
pub(crate) fn dropper_insert_hook(rt: &mut Rt, pos: [i32; 3], facing: u8, slot: i32) -> bool {
    let Some(h) = handler_at(rt, step(pos, facing), facing ^ 1) else {
        return false;
    };
    let dropper = vanilla(rt, pos);
    let stack = get(rt, dropper, slot);
    let whole = copy(rt, stack);
    let one = split(rt, whole, 1);
    let rest = put_in_all_slots(rt, h, one);
    let remainder = copy(rt, stack);
    if api_common::stack_empty(rt, rest) {
        grow(rt, remainder, -1);
    }
    set(rt, dropper, slot, remainder);
    changed(rt, dropper);
    true
}

/// `VanillaInventoryCodeHooks.insertCrafterOutput`: what of `stack` the item handler in front of the
/// crafter at `pos` (its front `facing`) doesn't take, offered slot by slot; None when there is no
/// handler there.
pub(crate) fn crafter_output(rt: &mut Rt, pos: [i32; 3], facing: u8, stack: V) -> Option<V> {
    let h = handler_at(rt, step(pos, facing), facing ^ 1)?;
    Some(put_in_all_slots(rt, h, stack))
}

// --- Dispatch -----------------------------------------------------------------------------------

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        INV | SIDED | HOPPER | ITEM_HANDLER | MODIFIABLE | VANILLA | CONTAINER | WORLDLY | RANGED | PLAYER_INV | PLAYER_MAIN | PLAYER_ARMOR | PLAYER_OFFHAND | EQUIPMENT | HANDS | ARMOR
    ) || (owner == ITEM_STACK && name == "canEquip")
        || name == "getEquipmentSlotForItem"
        || CONTAINER_BLOCK_ENTITIES.contains(&owner)
}

/// [`call`] by the call's key (`owner.name.desc`).
pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on the wrappers and on a vanilla block's container; None for anything else (another
/// handler, a mod's container).
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // `stack.canEquip(slot, entity)` (NeoForge's, by default where the stack is worn) and
    // `entity.getEquipmentSlotForItem(stack)`.
    if owner == ITEM_STACK && name == "canEquip" {
        let want = rt.str_of(rt.get_field(a(1), "$name")).unwrap_or("").to_owned();
        return Some(V::I((!want.is_empty() && equipment_slot_for(rt, this) == want) as i32));
    }
    if name == "getEquipmentSlotForItem" && desc == "(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/entity/EquipmentSlot;" {
        let slot = equipment_slot_for(rt, a(1));
        return Some(slot_constant(rt, &slot));
    }
    match owner {
        // `SidedInvWrapper.getSlot(inv, slot, side)`, static.
        SIDED if name == "getSlot" => Some(V::I(sided_slot(rt, this, a(1).i(), a(2)))),
        PLAYER_INV if name == "<init>" => {
            player_inv_init(rt, this, a(1));
            Some(V::Void)
        }
        // The rest of a player's handler is its `CombinedInvWrapper`'s.
        PLAYER_INV => None,
        RANGED | PLAYER_MAIN | PLAYER_ARMOR | PLAYER_OFFHAND => ranged_call(rt, owner, name, args),
        EQUIPMENT | HANDS | ARMOR => equipment_call(rt, owner, name, args),
        // By the receiver's class, as the JVM dispatches: a hopper's handler is an `InvWrapper`,
        // and the handlers' interfaces reach them all.
        INV | SIDED | HOPPER | ITEM_HANDLER | MODIFIABLE => {
            let sided = rt.instance_of(this, SIDED);
            let hopper = !sided && rt.instance_of(this, HOPPER);
            if sided || hopper || rt.instance_of(this, INV) {
                return wrapper_call(rt, sided, hopper, name, args);
            }
            if !matches!(owner, ITEM_HANDLER | MODIFIABLE) || this.r() == 0 {
                return None;
            }
            if rt.instance_of(this, RANGED) {
                return ranged_call(rt, owner, name, args);
            }
            if rt.instance_of(this, EQUIPMENT) {
                return equipment_call(rt, owner, name, args);
            }
            None
        }
        VANILLA | CONTAINER | WORLDLY if vanilla_at(rt, this).is_some() => vanilla_call(rt, name, args),
        // A vanilla container's block entity by its own class (`ChestBlockEntity.getItem`); what
        // isn't the container's is the block entity's (`getBlockPos`).
        o if CONTAINER_BLOCK_ENTITIES.contains(&o) && vanilla_at(rt, this).is_some() => vanilla_call(rt, name, args),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{HandStack, MemoryContainer, MemoryHost};

    const BLOCK_CAPABILITY: &str = "net/minecraft/world/level/Level.getCapability.(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Ljava/lang/Object;";
    const SLOTS: &str = "net/neoforged/neoforge/items/IItemHandler.getSlots.()I";
    const GET_STACK: &str = "net/neoforged/neoforge/items/IItemHandler.getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;";
    const INSERT: &str = "net/neoforged/neoforge/items/IItemHandler.insertItem.(ILnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/item/ItemStack;";
    const EXTRACT: &str = "net/neoforged/neoforge/items/IItemHandler.extractItem.(IIZ)Lnet/minecraft/world/item/ItemStack;";
    const LIMIT: &str = "net/neoforged/neoforge/items/IItemHandler.getSlotLimit.(I)I";
    const VALID: &str = "net/neoforged/neoforge/items/IItemHandler.isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z";
    const AT: [i32; 3] = [0, 64, 0];

    fn rt() -> Rt {
        crate::install();
        Rt::new()
    }

    fn container(kind: ContainerKind, size: u32, faces: Option<[Vec<u32>; 6]>) -> MemoryContainer {
        MemoryContainer { facts: ContainerFacts { size, max_stack: 99, kind }, items: vec![None; size as usize], faces, ..MemoryContainer::default() }
    }

    fn held(item: &str, count: u32) -> HandStack {
        HandStack { item: item.into(), count, ..HandStack::default() }
    }

    /// A slot of the host's container: its item and count.
    fn slot(host: &MemoryHost, at: [i32; 3], i: usize) -> Option<(String, u32)> {
        host.containers[&at].items[i].as_ref().map(|h| (h.item.clone(), h.count))
    }

    fn n(rt: &Rt, stack: V) -> i32 {
        if api_common::stack_empty(rt, stack) { 0 } else { rt.get_field(stack, "$count").i() }
    }

    fn item_of(rt: &Rt, stack: V) -> String {
        api_common::item_like_name(rt, rt.get_field(stack, "$item"))
    }

    /// `level.getCapability(Capabilities.ItemHandler.BLOCK, pos, side)` at `at`.
    fn handler(rt: &mut Rt, at: [i32; 3], side: Option<i64>) -> V {
        let cap = api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.BLOCK");
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let pos = api::block_pos(rt, at[0] as i64, at[1] as i64, at[2] as i64);
        let side = side.map_or(V::NULL, |s| api::direction(rt, s));
        api::call(rt, BLOCK_CAPABILITY, &[level, cap, pos, side])
    }

    /// A chest's handler, as NeoForge's `InvWrapper`: a stack goes onto the same item up to the
    /// item's own limit (64, under the chest's 99), and the rest comes back; another item is
    /// refused; simulating changes nothing; asking for more than a slot holds takes it all.
    #[test]
    fn a_chest_fills_up_to_a_stack_and_gives_back_the_rest() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, "minecraft:chest[facing=north,type=single,waterlogged=false]".into());
        let mut chest = container(ContainerKind::Plain, 27, None);
        chest.items[0] = Some(held("minecraft:iron_ingot", 60));
        host.containers.insert(AT, chest);
        rt.with_host(&mut host, |rt| {
            let h = handler(rt, AT, None);
            assert_eq!(rt.class_of(h), INV);
            assert_eq!(api::call(rt, SLOTS, &[h]).i(), 27);
            let iron = api_common::stack(rt, "minecraft:iron_ingot", 10);
            let rest = api::call(rt, INSERT, &[h, V::I(0), iron, V::I(1)]);
            assert_eq!(n(rt, rest), 6);
            let there = api::call(rt, GET_STACK, &[h, V::I(0)]);
            assert_eq!(n(rt, there), 60, "simulating changes nothing");
            let rest = api::call(rt, INSERT, &[h, V::I(0), iron, V::I(0)]);
            assert_eq!(n(rt, rest), 6);
            assert_eq!(n(rt, iron), 10, "the stack given stays as it was");
            let gold = api_common::stack(rt, "minecraft:gold_ingot", 5);
            assert_eq!(api::call(rt, INSERT, &[h, V::I(0), gold, V::I(0)]), gold, "not onto iron");
            let rest = api::call(rt, INSERT, &[h, V::I(1), gold, V::I(0)]);
            assert!(api_common::stack_empty(rt, rest));
            let took = api::call(rt, EXTRACT, &[h, V::I(0), V::I(70), V::I(0)]);
            assert_eq!((item_of(rt, took), n(rt, took)), ("minecraft:iron_ingot".to_owned(), 64));
            assert_eq!(api::call(rt, LIMIT, &[h, V::I(0)]).i(), 99);
        });
        assert_eq!(slot(&host, AT, 0), None);
        assert_eq!(slot(&host, AT, 1), Some(("minecraft:gold_ingot".to_owned(), 5)));
        assert_eq!(host.containers[&AT].changed, 3, "each change once");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Level.getBlockEntity` at a chest finds its block entity, of vanilla's class: a block entity
    /// at the chest's position and a container of the chest's slots, the same object each time, as
    /// Create's threshold switch takes it before it measures the chest. A furnace's is a
    /// `WorldlyContainer` of its 3 slots. Stone has none, nor a chest the game holds no container
    /// for.
    #[test]
    fn a_vanilla_containers_block_entity_is_its_container() {
        const GET_BE: &str = "net/minecraft/world/level/Level.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
        const BE_POS: &str = "net/minecraft/world/level/block/entity/BlockEntity.getBlockPos.()Lnet/minecraft/core/BlockPos;";
        const BE_STATE: &str = "net/minecraft/world/level/block/entity/BlockEntity.getBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
        const SIZE: &str = "net/minecraft/world/Container.getContainerSize.()I";
        const GET: &str = "net/minecraft/world/Container.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
        const OWN_SIZE: &str = "net/minecraft/world/level/block/entity/ChestBlockEntity.getContainerSize.()I";
        const CHEST: &str = "minecraft:chest[facing=north,type=single,waterlogged=false]";
        let (furnace_at, stone, lone) = ([1, 64, 0], [2, 64, 0], [3, 64, 0]);
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, CHEST.into());
        let mut chest = container(ContainerKind::Plain, 27, None);
        chest.items[3] = Some(held("minecraft:iron_ingot", 5));
        host.containers.insert(AT, chest);
        host.blocks.insert(furnace_at, "minecraft:furnace[facing=north,lit=false]".into());
        host.containers.insert(furnace_at, container(ContainerKind::Furnace, 3, None));
        host.blocks.insert(stone, "minecraft:stone".into());
        host.blocks.insert(lone, CHEST.into());
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let mut at = |p: [i32; 3]| {
                let pos = api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]));
                api::call(rt, GET_BE, &[level, pos])
            };
            let (be, again, furnace) = (at(AT), at(AT), at(furnace_at));
            assert_eq!((at(stone), at(lone)), (V::NULL, V::NULL));
            assert_eq!(rt.class_of(be), "net/minecraft/world/level/block/entity/ChestBlockEntity");
            assert_eq!(again, be, "the same object again");
            assert!(rt.instance_of(be, "net/minecraft/world/level/block/entity/BlockEntity"));
            assert!(rt.instance_of(be, CONTAINER) && !rt.instance_of(be, WORLDLY));
            assert!(rt.instance_of(furnace, CONTAINER) && rt.instance_of(furnace, WORLDLY));
            let pos = api::call(rt, BE_POS, &[be]);
            assert_eq!(api::pos_of(rt, pos), AT);
            let state = api::call(rt, BE_STATE, &[be]);
            assert_eq!(rt.str_of(state), Some(CHEST));
            assert_eq!(api::call(rt, SIZE, &[be]).i(), 27);
            assert_eq!(api::call(rt, OWN_SIZE, &[be]).i(), 27);
            let iron = api::call(rt, GET, &[be, V::I(3)]);
            assert_eq!((item_of(rt, iron), n(rt, iron)), ("minecraft:iron_ingot".to_owned(), 5));
            assert_eq!(api::call(rt, SIZE, &[furnace]).i(), 3);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A furnace's handler, as NeoForge's `SidedInvWrapper` of the face asked through: its top
    /// reaches the input, its sides the fuel, its bottom the output then the fuel. A bucket goes in
    /// as fuel one at a time; through the bottom an empty bucket comes back out of the fuel slot
    /// but coal doesn't. Without a face every slot is reached.
    #[test]
    fn a_furnace_takes_and_gives_through_its_faces() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, "minecraft:furnace[facing=north,lit=false]".into());
        host.stack_limits.insert("minecraft:bucket".into(), 16);
        let mut furnace = container(ContainerKind::Furnace, 3, Some([vec![2, 1], vec![0], vec![1], vec![1], vec![1], vec![1]]));
        furnace.items[2] = Some(held("minecraft:iron_ingot", 3));
        furnace.keeps = vec![(1, "minecraft:coal".into())];
        host.containers.insert(AT, furnace);
        rt.with_host(&mut host, |rt| {
            let top = handler(rt, AT, Some(1));
            assert_eq!(rt.class_of(top), SIDED);
            assert_eq!(api::call(rt, SLOTS, &[top]).i(), 1);
            let ore = api_common::stack(rt, "minecraft:raw_iron", 5);
            let rest = api::call(rt, INSERT, &[top, V::I(0), ore, V::I(0)]);
            assert!(api_common::stack_empty(rt, rest));
            let side = handler(rt, AT, Some(2));
            let buckets = api_common::stack(rt, "minecraft:bucket", 3);
            let rest = api::call(rt, INSERT, &[side, V::I(0), buckets, V::I(0)]);
            assert_eq!(n(rt, rest), 2, "one bucket at a time");
            let bottom = handler(rt, AT, Some(0));
            assert_eq!(api::call(rt, SLOTS, &[bottom]).i(), 2);
            let out = api::call(rt, EXTRACT, &[bottom, V::I(0), V::I(64), V::I(0)]);
            assert_eq!((item_of(rt, out), n(rt, out)), ("minecraft:iron_ingot".to_owned(), 3));
            let out = api::call(rt, EXTRACT, &[bottom, V::I(1), V::I(64), V::I(0)]);
            assert_eq!(item_of(rt, out), "minecraft:bucket");
            let coal = api_common::stack(rt, "minecraft:coal", 8);
            let rest = api::call(rt, INSERT, &[side, V::I(0), coal, V::I(0)]);
            assert!(api_common::stack_empty(rt, rest));
            let kept = api::call(rt, EXTRACT, &[bottom, V::I(1), V::I(64), V::I(0)]);
            assert!(api_common::stack_empty(rt, kept), "coal stays in through the bottom");
            let any = handler(rt, AT, None);
            assert_eq!(api::call(rt, SLOTS, &[any]).i(), 3);
            let out = api::call(rt, EXTRACT, &[any, V::I(1), V::I(64), V::I(1)]);
            assert_eq!(n(rt, out), 8, "without a face nothing is kept");
        });
        assert_eq!(slot(&host, AT, 0), Some(("minecraft:raw_iron".to_owned(), 5)));
        assert_eq!(slot(&host, AT, 1), Some(("minecraft:coal".to_owned(), 8)));
        assert_eq!(slot(&host, AT, 2), None);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A brewing stand's handler holds one bottle in each of its three bottle slots (its sides
    /// reach those and the fuel); a shulker box's takes no shulker box through a face, though the
    /// box itself reckons one valid (`canPlaceItem`).
    #[test]
    fn a_brewing_stand_holds_one_bottle_a_slot_and_a_shulker_box_no_shulker_box() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        let box_at = [1, 64, 0];
        host.blocks.insert(AT, "minecraft:brewing_stand".into());
        host.blocks.insert(box_at, "minecraft:shulker_box[facing=up]".into());
        let sides = vec![0, 1, 2, 4];
        let stand = container(ContainerKind::Brewing, 5, Some([vec![0, 1, 2, 3], vec![3], sides.clone(), sides.clone(), sides.clone(), sides]));
        host.containers.insert(AT, stand);
        let mut shulker = container(ContainerKind::Shulker, 27, None);
        shulker.refuses_through = vec!["minecraft:shulker_box".into()];
        host.containers.insert(box_at, shulker);
        rt.with_host(&mut host, |rt| {
            let h = handler(rt, AT, Some(4));
            assert_eq!(api::call(rt, SLOTS, &[h]).i(), 4);
            assert_eq!(api::call(rt, LIMIT, &[h, V::I(0)]).i(), 1);
            assert_eq!(api::call(rt, LIMIT, &[h, V::I(3)]).i(), 99, "the fuel slot");
            let potions = api_common::stack(rt, "minecraft:potion", 3);
            let rest = api::call(rt, INSERT, &[h, V::I(0), potions, V::I(0)]);
            assert_eq!(n(rt, rest), 2);
            let b = handler(rt, box_at, Some(1));
            let inner = api_common::stack(rt, "minecraft:shulker_box", 1);
            assert_eq!(api::call(rt, VALID, &[b, V::I(0), inner]).i(), 1);
            assert_eq!(api::call(rt, INSERT, &[b, V::I(0), inner, V::I(0)]), inner);
        });
        assert_eq!(slot(&host, AT, 0), Some(("minecraft:potion".to_owned(), 1)));
        assert_eq!(slot(&host, box_at, 0), None);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A hopper's handler, as NeoForge's `VanillaHopperItemHandler`: filling the empty hopper sets
    /// its cooldown; a simulated insert doesn't, nor one into a hopper that holds items already.
    #[test]
    fn a_hopper_filled_while_empty_waits() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, "minecraft:hopper[enabled=true,facing=down]".into());
        host.containers.insert(AT, container(ContainerKind::Hopper, 5, None));
        rt.with_host(&mut host, |rt| {
            let h = handler(rt, AT, Some(1));
            assert_eq!(rt.class_of(h), HOPPER);
            assert!(rt.instance_of(h, INV) && rt.instance_of(h, ITEM_HANDLER));
            let dirt = api_common::stack(rt, "minecraft:dirt", 4);
            api::call(rt, INSERT, &[h, V::I(0), dirt, V::I(1)]);
            api::call(rt, INSERT, &[h, V::I(0), dirt, V::I(0)]);
            api::call(rt, INSERT, &[h, V::I(1), dirt, V::I(0)]);
        });
        assert_eq!(slot(&host, AT, 1), Some(("minecraft:dirt".to_owned(), 4)));
        assert_eq!(host.containers[&AT].filled, 1);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The `InvWrapper` a mod makes of a container of its own (`new InvWrapper(container)`): the
    /// container answers for itself, a `SimpleContainer` with its 99 a slot.
    #[test]
    fn a_wrapper_of_a_mods_container_asks_the_container() {
        let mut rt = rt();
        let c = rt.new_object("net/minecraft/world/SimpleContainer");
        api::call(&mut rt, "net/minecraft/world/SimpleContainer.<init>.(I)V", &[c, V::I(2)]);
        let w = rt.new_object(INV);
        api::call(&mut rt, "net/neoforged/neoforge/items/wrapper/InvWrapper.<init>.(Lnet/minecraft/world/Container;)V", &[w, c]);
        assert_eq!(api::call(&mut rt, SLOTS, &[w]).i(), 2);
        assert_eq!(api::call(&mut rt, LIMIT, &[w, V::I(0)]).i(), 99);
        let iron = api_common::stack(&mut rt, "minecraft:iron_ingot", 70);
        let rest = api::call(&mut rt, INSERT, &[w, V::I(0), iron, V::I(0)]);
        assert_eq!(n(&rt, rest), 6);
        let there = api::call(&mut rt, "net/minecraft/world/Container.getItem.(I)Lnet/minecraft/world/item/ItemStack;", &[c, V::I(0)]);
        assert_eq!(n(&rt, there), 64);
        let took = api::call(&mut rt, EXTRACT, &[w, V::I(0), V::I(10), V::I(1)]);
        assert_eq!(n(&rt, took), 10);
        assert_eq!(n(&rt, there), 64, "simulating takes nothing");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Every test lambda is a capability provider answering with what it captured.
    fn providers(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        Some(rt.captured(args[0]).first().copied().unwrap_or(V::NULL))
    }

    const SET_STACK: &str = "net/neoforged/neoforge/items/IItemHandlerModifiable.setStackInSlot.(ILnet/minecraft/world/item/ItemStack;)V";

    /// A mod's block `test:depot` at `at` whose block entity hands out a two-slot `ItemStackHandler`
    /// (`Capabilities.ItemHandler.BLOCK`, registered for its block entity type): the handler.
    fn mod_depot(rt: &mut Rt, host: &mut MemoryHost, at: [i32; 3], slots: [Option<(&str, i32)>; 2]) -> V {
        rt.lambda_call = Some(providers);
        let cap = api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.BLOCK");
        let ty = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType");
        let h = rt.new_object("net/neoforged/neoforge/items/ItemStackHandler");
        api::call(rt, "net/neoforged/neoforge/items/ItemStackHandler.<init>.(I)V", &[h, V::I(2)]);
        for (i, s) in slots.iter().enumerate() {
            if let Some((item, count)) = s {
                let stack = api_common::stack(rt, item, *count);
                api::call(rt, SET_STACK, &[h, V::I(i as i32), stack]);
            }
        }
        let provider = rt.lambda_id("test/Provider", 1, vec![h]);
        let ev = rt.new_object("net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent");
        let register = "net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent.registerBlockEntity.(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/neoforged/neoforge/capabilities/ICapabilityProvider;)V";
        api::call(rt, register, &[ev, cap, ty, provider]);
        let be = rt.new_object("test/DepotBlockEntity");
        rt.put_field(be, "$type", ty);
        rt.block_entities.insert(at, be);
        host.blocks.insert(at, "test:depot".into());
        h
    }

    /// What slot `i` of handler `h` holds: its item and count.
    fn in_handler(rt: &mut Rt, h: V, i: i32) -> Option<(String, i32)> {
        let s = api::call(rt, GET_STACK, &[h, V::I(i)]);
        (!api_common::stack_empty(rt, s)).then(|| (item_of(rt, s), n(rt, s)))
    }

    fn owned(s: &str, n: i32) -> Option<(String, i32)> {
        Some((s.to_owned(), n))
    }

    /// NeoForge's `insertHook`: a hopper facing down onto a mod's block pushes one item a turn into
    /// its handler, from its first slot with an item the handler takes; a handler with no room for
    /// it leaves the hopper as it was; with no handler in front the game's own rules go on.
    #[test]
    fn a_hopper_pushes_one_item_a_turn_into_a_mods_handler() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, "minecraft:hopper[enabled=true,facing=down]".into());
        let mut hopper = container(ContainerKind::Hopper, 5, None);
        hopper.items[1] = Some(held("minecraft:cobblestone", 3));
        host.containers.insert(AT, hopper);
        let below = [AT[0], AT[1] - 1, AT[2]];
        let h = mod_depot(&mut rt, &mut host, below, [None, None]);
        let moved = rt.with_host(&mut host, |rt| [insert_hook(rt, AT, 0), insert_hook(rt, AT, 0)]);
        assert_eq!(moved, [Some(true), Some(true)]);
        assert_eq!(in_handler(&mut rt, h, 0), owned("minecraft:cobblestone", 2));
        assert_eq!(slot(&host, AT, 1), Some(("minecraft:cobblestone".to_owned(), 1)));

        // Room for another item only: the hopper's cobblestone stays.
        let east = [AT[0] + 1, AT[1], AT[2]];
        let full = mod_depot(&mut rt, &mut host, east, [Some(("minecraft:stone", 64)), Some(("minecraft:dirt", 64))]);
        let moved = rt.with_host(&mut host, |rt| [insert_hook(rt, AT, 5), insert_hook(rt, AT, 2)]);
        assert_eq!(moved, [Some(false), None], "nothing fits; no handler to the north");
        assert_eq!(slot(&host, AT, 1), Some(("minecraft:cobblestone".to_owned(), 1)));
        assert_eq!(in_handler(&mut rt, full, 0), owned("minecraft:stone", 64));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's `extractHook`: a hopper under a mod's block pulls one item a turn out of its
    /// handler's first slot that gives one, onto the same item in the hopper or into an empty slot;
    /// a full hopper takes nothing; under a block with no handler it is left to the game.
    #[test]
    fn a_hopper_pulls_one_item_a_turn_out_of_a_mods_handler() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, "minecraft:hopper[enabled=true,facing=down]".into());
        let mut hopper = container(ContainerKind::Hopper, 5, None);
        hopper.items[0] = Some(held("minecraft:dirt", 64));
        hopper.items[1] = Some(held("minecraft:iron_ingot", 1));
        host.containers.insert(AT, hopper);
        let above = [AT[0], AT[1] + 1, AT[2]];
        let h = mod_depot(&mut rt, &mut host, above, [None, Some(("minecraft:iron_ingot", 5))]);
        let took = rt.with_host(&mut host, |rt| [extract_hook(rt, AT), extract_hook(rt, AT)]);
        assert_eq!(took, [Some(true), Some(true)]);
        assert_eq!(slot(&host, AT, 1), Some(("minecraft:iron_ingot".to_owned(), 3)), "onto the iron, past the full dirt");
        assert_eq!(slot(&host, AT, 2), None);
        assert_eq!(in_handler(&mut rt, h, 1), owned("minecraft:iron_ingot", 3));

        // A hopper with no room.
        let below = [AT[0], AT[1] - 2, AT[2]];
        host.blocks.insert(below, "minecraft:hopper[enabled=true,facing=down]".into());
        let mut packed = container(ContainerKind::Hopper, 5, None);
        packed.items = vec![Some(held("minecraft:dirt", 64)); 5];
        host.containers.insert(below, packed);
        mod_depot(&mut rt, &mut host, [AT[0], AT[1] - 1, AT[2]], [Some(("minecraft:gold_ingot", 2)), None]);
        let lone = [AT[0] + 5, AT[1], AT[2]];
        host.blocks.insert(lone, "minecraft:hopper[enabled=true,facing=down]".into());
        host.containers.insert(lone, container(ContainerKind::Hopper, 5, None));
        let took = rt.with_host(&mut host, |rt| [extract_hook(rt, below), extract_hook(rt, lone)]);
        assert_eq!(took, [Some(false), None]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's `dropperInsertHook`: a dropper facing a mod's block puts one item of the slot it
    /// drops from into the handler; when nothing fits, the slot keeps it and nothing drops; facing
    /// a block with no handler, it drops as usual.
    #[test]
    fn a_dropper_puts_one_item_into_a_mods_handler_or_keeps_it() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        host.blocks.insert(AT, "minecraft:dropper[facing=east,triggered=false]".into());
        let mut dropper = container(ContainerKind::Plain, 9, None);
        dropper.items[4] = Some(held("minecraft:arrow", 3));
        host.containers.insert(AT, dropper);
        let east = [AT[0] + 1, AT[1], AT[2]];
        let h = mod_depot(&mut rt, &mut host, east, [None, None]);
        let west = [AT[0] - 1, AT[1], AT[2]];
        mod_depot(&mut rt, &mut host, west, [Some(("minecraft:stone", 64)), Some(("minecraft:dirt", 64))]);
        let handled = rt.with_host(&mut host, |rt| [dropper_insert_hook(rt, AT, 5, 4), dropper_insert_hook(rt, AT, 4, 4), dropper_insert_hook(rt, AT, 1, 4)]);
        assert_eq!(handled, [true, true, false], "east takes one, west none, nothing above");
        assert_eq!(in_handler(&mut rt, h, 0), owned("minecraft:arrow", 1));
        assert_eq!(slot(&host, AT, 4), Some(("minecraft:arrow".to_owned(), 2)));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's `insertCrafterOutput`: a crafter's result goes into a mod's handler slot by slot,
    /// only into a slot that takes all of what is left (6 bread skip a slot with room for 2), and
    /// what no slot takes comes back.
    #[test]
    fn a_crafters_output_goes_where_it_fits_whole() {
        let mut rt = rt();
        let mut host = MemoryHost::default();
        let south = [AT[0], AT[1], AT[2] + 1];
        let h = mod_depot(&mut rt, &mut host, south, [Some(("minecraft:bread", 62)), None]);
        let north = [AT[0], AT[1], AT[2] - 1];
        mod_depot(&mut rt, &mut host, north, [Some(("minecraft:bread", 62)), Some(("minecraft:stone", 1))]);
        let rest = rt.with_host(&mut host, |rt| {
            let into_south = api_common::stack(rt, "minecraft:bread", 6);
            let into_north = api_common::stack(rt, "minecraft:bread", 6);
            let up = api_common::stack(rt, "minecraft:bread", 6);
            let (south, north, up) = (crafter_output(rt, AT, 3, into_south), crafter_output(rt, AT, 2, into_north), crafter_output(rt, AT, 1, up));
            [south.map(|s| n(rt, s)), north.map(|s| n(rt, s)), up.map(|s| n(rt, s))]
        });
        assert_eq!(rest, [Some(0), Some(6), None]);
        assert_eq!((in_handler(&mut rt, h, 0), in_handler(&mut rt, h, 1)), (owned("minecraft:bread", 62), owned("minecraft:bread", 6)));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const PLAYER_CAPABILITY: &str = "net/minecraft/world/entity/player/Player.getCapability.(Lnet/neoforged/neoforge/capabilities/EntityCapability;Ljava/lang/Object;)Ljava/lang/Object;";
    const CAN_EQUIP: &str = "net/minecraft/world/item/ItemStack.canEquip.(Lnet/minecraft/world/entity/EquipmentSlot;Lnet/minecraft/world/entity/LivingEntity;)Z";
    const SLOT_FOR: &str = "net/minecraft/world/entity/LivingEntity.getEquipmentSlotForItem.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/entity/EquipmentSlot;";

    /// A player whose game wears a diamond helmet (one to a stack) on the head and an elytra on the
    /// chest, with dirt in inventory slot 5 and torches in the off hand.
    fn wearing_host() -> MemoryHost {
        let mut host = MemoryHost { inventory: true, player_id: Some(7), offhand: Some(held("minecraft:torch", 3)), ..MemoryHost::default() };
        host.equip_slots.insert("minecraft:diamond_helmet".into(), "head".into());
        host.equip_slots.insert("minecraft:elytra".into(), "chest".into());
        host.stack_limits.insert("minecraft:diamond_helmet".into(), 1);
        host.slots.insert(5, held("minecraft:dirt", 10));
        host
    }

    /// NeoForge's handler of a player (`Capabilities.ItemHandler.ENTITY`): the inventory's 36 slots,
    /// then the armour's 4 (feet first), then the off hand. A main slot fills as a chest's does; an
    /// armour slot takes only what is worn there, one at a time; the game's slots get what it did.
    #[test]
    fn a_players_handler_is_their_inventory_whose_armour_takes_only_what_is_worn_there() {
        let mut rt = rt();
        let mut host = wearing_host();
        rt.with_host(&mut host, |rt| {
            let player = api_common::acting_player(rt);
            let cap = api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.ENTITY");
            let h = api::call(rt, PLAYER_CAPABILITY, &[player, cap, V::NULL]);
            assert_eq!(rt.class_of(h), PLAYER_INV);
            assert_eq!(api::call(rt, SLOTS, &[h]).i(), 41);
            let dirt = api::call(rt, GET_STACK, &[h, V::I(5)]);
            assert_eq!((item_of(rt, dirt), n(rt, dirt)), ("minecraft:dirt".to_owned(), 10));
            let torches = api::call(rt, GET_STACK, &[h, V::I(40)]);
            assert_eq!((item_of(rt, torches), n(rt, torches)), ("minecraft:torch".to_owned(), 3));
            let more = api_common::stack(rt, "minecraft:dirt", 4);
            let rest = api::call(rt, INSERT, &[h, V::I(5), more, V::I(0)]);
            assert!(api_common::stack_empty(rt, rest));
            // Dirt is worn nowhere, the helmet on the head only (the armour's slot 3), one of two.
            let dirt = api_common::stack(rt, "minecraft:dirt", 1);
            assert_eq!(api::call(rt, INSERT, &[h, V::I(39), dirt, V::I(0)]), dirt);
            let helmets = api_common::stack(rt, "minecraft:diamond_helmet", 2);
            assert_eq!(api::call(rt, INSERT, &[h, V::I(36), helmets, V::I(0)]), helmets, "not on the feet");
            let rest = api::call(rt, INSERT, &[h, V::I(39), helmets, V::I(0)]);
            assert_eq!((item_of(rt, rest), n(rt, rest)), ("minecraft:diamond_helmet".to_owned(), 1));
            let took = api::call(rt, EXTRACT, &[h, V::I(40), V::I(2), V::I(0)]);
            assert_eq!((item_of(rt, took), n(rt, took)), ("minecraft:torch".to_owned(), 2));
            assert_eq!(api::call(rt, LIMIT, &[h, V::I(0)]).i(), 99);
        });
        assert_eq!(host.slots.get(&5).map(|s| s.count), Some(14));
        assert_eq!(host.worn[0].as_ref().map(|s| (s.item.as_str(), s.count)), Some(("minecraft:diamond_helmet", 1)));
        assert_eq!(host.offhand.as_ref().map(|s| s.count), Some(1));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Where a stack is worn, as `LivingEntity.getEquipmentSlotForItem` finds it and NeoForge's
    /// `ItemStack.canEquip` asks it: a helmet on the head and an elytra on the chest as the game wears
    /// them, a mod's boots on the feet by their armour type, dirt in the main hand.
    #[test]
    fn a_stack_is_equipped_where_its_item_is_worn() {
        fn forward(rt: &mut Rt, key: &'static str, _: &'static str, args: &[V]) -> V {
            api::call(rt, key, args)
        }
        let mut rt = rt();
        rt.vcall = Some(forward);
        rt.supers.insert("test/CardboardBoots", "net/minecraft/world/item/ArmorItem");
        let mut host = wearing_host();
        rt.with_host(&mut host, |rt| {
            let material = api::get_static(rt, "net/minecraft/world/item/ArmorMaterials.LEATHER");
            let props = rt.new_object("net/minecraft/world/item/Item$Properties");
            api::call(rt, "net/minecraft/world/item/Item$Properties.<init>.()V", &[props]);
            let boots = rt.new_object("test/CardboardBoots");
            let ty = api::get_static(rt, "net/minecraft/world/item/ArmorItem$Type.BOOTS");
            api::call(rt, "net/minecraft/world/item/ArmorItem.<init>.(Lnet/minecraft/core/Holder;Lnet/minecraft/world/item/ArmorItem$Type;Lnet/minecraft/world/item/Item$Properties;)V", &[boots, material, ty, props]);
            rt.registered.insert(("item".into(), "test:cardboard_boots".into()), boots);
            let boots_stack = rt.new_object(ITEM_STACK);
            api::call(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[boots_stack, boots]);
            let player = api_common::acting_player(rt);
            let slots = ["HEAD", "CHEST", "FEET", "MAINHAND"].map(|s| api_common::get_static(rt, EQUIPMENT_SLOT, s).unwrap());
            let stacks = [
                api_common::stack(rt, "minecraft:diamond_helmet", 1),
                api_common::stack(rt, "minecraft:elytra", 1),
                boots_stack,
                api_common::stack(rt, "minecraft:dirt", 1),
            ];
            for (i, s) in stacks.into_iter().enumerate() {
                assert_eq!(api::call(rt, SLOT_FOR, &[player, s]), slots[i], "stack {i}");
                for (j, there) in slots.into_iter().enumerate() {
                    assert_eq!(api::call(rt, CAN_EQUIP, &[s, there, player]).i() != 0, i == j, "stack {i} in slot {j}");
                }
            }
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
