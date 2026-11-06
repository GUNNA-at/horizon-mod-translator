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
use crate::host::{ContainerFacts, ContainerKind};
use crate::{Rt, V, api, api_common, hand, jutil, menus};

const INV: &str = "net/neoforged/neoforge/items/wrapper/InvWrapper";
const SIDED: &str = "net/neoforged/neoforge/items/wrapper/SidedInvWrapper";
const HOPPER: &str = "net/neoforged/neoforge/items/VanillaHopperItemHandler";
const ITEM_HANDLER: &str = "net/neoforged/neoforge/items/IItemHandler";
const MODIFIABLE: &str = "net/neoforged/neoforge/items/IItemHandlerModifiable";
/// A vanilla block's container: the game's own, by position (`$x`, `$y`, `$z`).
pub(crate) const VANILLA: &str = "compat:VanillaContainer";
const CONTAINER: &str = "net/minecraft/world/Container";
const WORLDLY: &str = "net/minecraft/world/WorldlyContainer";
const FURNACE: &str = "net/minecraft/world/level/block/entity/AbstractFurnaceBlockEntity";
const BREWING: &str = "net/minecraft/world/level/block/entity/BrewingStandBlockEntity";

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
        ITEM_HANDLER | MODIFIABLE => matches!(c, INV | SIDED | HOPPER),
        INV => c == HOPPER,
        CONTAINER | WORLDLY | "net/minecraft/world/Clearable" => c == VANILLA,
        _ => false,
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

/// Where container `inv` is, when it is a vanilla block's.
fn vanilla_at(rt: &Rt, inv: V) -> Option<[i32; 3]> {
    (inv.r() != 0 && rt.class_of(inv) == VANILLA).then(|| [rt.get_field(inv, "$x").i(), rt.get_field(inv, "$y").i(), rt.get_field(inv, "$z").i()])
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

// --- Dispatch -----------------------------------------------------------------------------------

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, INV | SIDED | HOPPER | ITEM_HANDLER | MODIFIABLE | VANILLA | CONTAINER | WORLDLY)
}

/// [`call`] by the call's key (`owner.name.desc`).
pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on the wrappers and on a vanilla block's container; None for anything else (another
/// handler, a mod's container).
pub fn call(rt: &mut Rt, owner: &str, name: &str, _desc: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    match owner {
        // `SidedInvWrapper.getSlot(inv, slot, side)`, static.
        SIDED if name == "getSlot" => Some(V::I(sided_slot(rt, this, args.get(1).copied().unwrap_or_default().i(), args.get(2).copied().unwrap_or_default()))),
        // By the receiver's class, as the JVM dispatches: a hopper's handler is an `InvWrapper`,
        // and the handlers' interfaces reach them all.
        INV | SIDED | HOPPER | ITEM_HANDLER | MODIFIABLE => {
            let sided = rt.instance_of(this, SIDED);
            let hopper = !sided && rt.instance_of(this, HOPPER);
            if !sided && !hopper && !rt.instance_of(this, INV) {
                return None;
            }
            wrapper_call(rt, sided, hopper, name, args)
        }
        VANILLA | CONTAINER | WORLDLY if this.r() != 0 && rt.class_of(this) == VANILLA => vanilla_call(rt, name, args),
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
}
