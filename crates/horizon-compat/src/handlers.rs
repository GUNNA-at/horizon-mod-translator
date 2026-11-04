//! NeoForge's item handlers, natively: `ItemStackHandler` (a block entity's slots: Create's depot
//! outputs, basins, toolboxes...) as NeoForge writes it, a mod's subclass of it included (its own
//! `onContentsChanged`, `isItemValid` or `getSlotLimit` answer, through the mod's code), and
//! `ItemHandlerHelper`'s inserts into any `IItemHandler`, the mod's own ones too.
//!
//! The stacks are the handler's `stacks` field, a `NonNullList`, as in NeoForge (a subclass reads
//! `this.stacks` itself).
use crate::{Data, Rt, V, api, api_common, jutil, nbt, tags};

const HANDLER: &str = "net/neoforged/neoforge/items/ItemStackHandler";
const ITEM_HANDLER: &str = "net/neoforged/neoforge/items/IItemHandler";
const MODIFIABLE: &str = "net/neoforged/neoforge/items/IItemHandlerModifiable";
const SERIALIZABLE: &str = "net/neoforged/neoforge/common/util/INBTSerializable";
const HELPER: &str = "net/neoforged/neoforge/items/ItemHandlerHelper";
const COMBINED: &str = "net/neoforged/neoforge/items/wrapper/CombinedInvWrapper";
const NON_NULL_LIST: &str = "net/minecraft/core/NonNullList";
const CONTENTS: &str = "net/minecraft/world/item/component/ItemContainerContents";
/// `Item.ABSOLUTE_MAX_STACK_SIZE`.
const ABSOLUTE_MAX: i32 = 99;

// The handler's overridable steps, called virtually: (the API's key, the method's).
const ON_CHANGED: (&str, &str) = ("net/neoforged/neoforge/items/ItemStackHandler.onContentsChanged.(I)V", "onContentsChanged.(I)V");
const ON_LOAD: (&str, &str) = ("net/neoforged/neoforge/items/ItemStackHandler.onLoad.()V", "onLoad.()V");
const IS_VALID: (&str, &str) = (
    "net/neoforged/neoforge/items/ItemStackHandler.isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z",
    "isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z",
);
const STACK_LIMIT: (&str, &str) = (
    "net/neoforged/neoforge/items/ItemStackHandler.getStackLimit.(ILnet/minecraft/world/item/ItemStack;)I",
    "getStackLimit.(ILnet/minecraft/world/item/ItemStack;)I",
);
const SLOT_LIMIT: (&str, &str) = ("net/neoforged/neoforge/items/ItemStackHandler.getSlotLimit.(I)I", "getSlotLimit.(I)I");
// Any handler's methods (a mod's own `IItemHandler` too), for `ItemHandlerHelper`.
const SLOTS: (&str, &str) = ("net/neoforged/neoforge/items/IItemHandler.getSlots.()I", "getSlots.()I");
const GET: (&str, &str) = ("net/neoforged/neoforge/items/IItemHandler.getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;", "getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;");
const INSERT: (&str, &str) = (
    "net/neoforged/neoforge/items/IItemHandler.insertItem.(ILnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/item/ItemStack;",
    "insertItem.(ILnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/item/ItemStack;",
);

const EXTRACT: (&str, &str) = (
    "net/neoforged/neoforge/items/IItemHandler.extractItem.(IIZ)Lnet/minecraft/world/item/ItemStack;",
    "extractItem.(IIZ)Lnet/minecraft/world/item/ItemStack;",
);
const ANY_SLOT_LIMIT: (&str, &str) = ("net/neoforged/neoforge/items/IItemHandler.getSlotLimit.(I)I", "getSlotLimit.(I)I");
const ANY_IS_VALID: (&str, &str) = (
    "net/neoforged/neoforge/items/IItemHandler.isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z",
    "isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z",
);
const ANY_SET: (&str, &str) = (
    "net/neoforged/neoforge/items/IItemHandlerModifiable.setStackInSlot.(ILnet/minecraft/world/item/ItemStack;)V",
    "setStackInSlot.(ILnet/minecraft/world/item/ItemStack;)V",
);

/// A method that may be the mod's own: its override, else the API's.
fn virt(rt: &mut Rt, (owner_key, key): (&'static str, &'static str), args: &[V]) -> V {
    match rt.vcall {
        Some(vc) => vc(rt, owner_key, key, args),
        None => api::call(rt, owner_key, args),
    }
}

fn empty(rt: &mut Rt) -> V {
    api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
}

fn count(rt: &Rt, stack: V) -> i32 {
    if api_common::stack_empty(rt, stack) { 0 } else { rt.get_field(stack, "$count").i() }
}

fn copy_with_count(rt: &mut Rt, stack: V, n: i32) -> V {
    api::call(rt, "net/minecraft/world/item/ItemStack.copyWithCount.(I)Lnet/minecraft/world/item/ItemStack;", &[stack, V::I(n)])
}

fn same_item(rt: &mut Rt, a: V, b: V) -> bool {
    api::call(rt, "net/minecraft/world/item/ItemStack.isSameItemSameComponents.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z", &[a, b]).i() != 0
}

fn max_stack(rt: &mut Rt, stack: V) -> i32 {
    api::call(rt, "net/minecraft/world/item/ItemStack.getMaxStackSize.()I", &[stack]).i()
}

fn sized(rt: &mut Rt, handler: V, size: i32) {
    let e = empty(rt);
    let list = rt.alloc(NON_NULL_LIST, Data::List(vec![e; size.max(0) as usize]));
    rt.put_field(handler, "stacks", list);
}

fn stacks(rt: &Rt, handler: V) -> Vec<V> {
    jutil::elements(rt, rt.get_field(handler, "stacks"))
}

fn set(rt: &mut Rt, handler: V, slot: i32, stack: V) {
    let list = rt.get_field(handler, "stacks");
    if let Some(crate::Object { data: Data::List(l), .. }) = rt.heap.get_mut(list.r() as usize)
        && let Some(s) = l.get_mut(slot as usize)
    {
        *s = stack;
    }
}

/// `validateSlotIndex`: a slot outside the handler throws, as NeoForge's does.
fn valid_slot(rt: &mut Rt, handler: V, slot: i32) -> bool {
    let n = stacks(rt, handler).len() as i32;
    if (0..n).contains(&slot) {
        return true;
    }
    let e = rt.new_object("java/lang/RuntimeException");
    let m = rt.string(&format!("Slot {slot} not in valid range - [0,{n})"));
    rt.put_field(e, "message", m);
    rt.thrown = Some(e);
    false
}

fn changed(rt: &mut Rt, handler: V, slot: i32) {
    virt(rt, ON_CHANGED, &[handler, V::I(slot)]);
}

/// `ItemStackHandler.insertItem`: onto the slot's stack of the same item, up to the limit; the
/// rest comes back.
fn insert(rt: &mut Rt, handler: V, slot: i32, stack: V, simulate: bool) -> V {
    if api_common::stack_empty(rt, stack) {
        return empty(rt);
    }
    if virt(rt, IS_VALID, &[handler, V::I(slot), stack]).i() == 0 {
        return stack;
    }
    if !valid_slot(rt, handler, slot) {
        return V::NULL;
    }
    let existing = stacks(rt, handler)[slot as usize];
    let mut limit = virt(rt, STACK_LIMIT, &[handler, V::I(slot), stack]).i();
    if !api_common::stack_empty(rt, existing) {
        if !same_item(rt, stack, existing) {
            return stack;
        }
        limit -= count(rt, existing);
    }
    if limit <= 0 {
        return stack;
    }
    let n = count(rt, stack);
    let reached = n > limit;
    if !simulate {
        if api_common::stack_empty(rt, existing) {
            let put = if reached { copy_with_count(rt, stack, limit) } else { stack };
            set(rt, handler, slot, put);
        } else {
            let grown = count(rt, existing) + if reached { limit } else { n };
            rt.put_field(existing, "$count", V::I(grown));
        }
        changed(rt, handler, slot);
    }
    if reached { copy_with_count(rt, stack, n - limit) } else { empty(rt) }
}

/// `ItemStackHandler.extractItem`: up to `amount` (and a stack's worth) off the slot.
fn extract(rt: &mut Rt, handler: V, slot: i32, amount: i32, simulate: bool) -> V {
    if amount == 0 || !valid_slot(rt, handler, slot) {
        return if rt.thrown.is_some() { V::NULL } else { empty(rt) };
    }
    let existing = stacks(rt, handler)[slot as usize];
    if api_common::stack_empty(rt, existing) {
        return empty(rt);
    }
    let take = amount.min(max_stack(rt, existing));
    let have = count(rt, existing);
    if have <= take {
        if simulate {
            return api::call(rt, "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;", &[existing]);
        }
        let e = empty(rt);
        set(rt, handler, slot, e);
        changed(rt, handler, slot);
        return existing;
    }
    if !simulate {
        let left = copy_with_count(rt, existing, have - take);
        set(rt, handler, slot, left);
        changed(rt, handler, slot);
    }
    copy_with_count(rt, existing, take)
}

/// `serializeNBT`: `{Items: [{Slot, id, count}...], Size}`, the empty slots left out.
fn serialize(rt: &mut Rt, handler: V) -> V {
    let mut items = Vec::new();
    for (i, s) in stacks(rt, handler).into_iter().enumerate() {
        if api_common::stack_empty(rt, s) {
            continue;
        }
        let tag = tags::new_compound(rt);
        let k = rt.string("Slot");
        api::call(rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[tag, k, V::I(i as i32)]);
        api_common::save_stack(rt, s, tag);
        items.push(tag);
    }
    let n = stacks(rt, handler).len() as i32;
    let out = tags::new_compound(rt);
    let list = tags::new_list(rt, items);
    let (ik, sk) = (rt.string("Items"), rt.string("Size"));
    api::call(rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[out, ik, list]);
    api::call(rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[out, sk, V::I(n)]);
    out
}

/// `deserializeNBT`: the size, then each saved slot's stack; then `onLoad`.
fn deserialize(rt: &mut Rt, handler: V, tag: V) {
    let map = nbt::from_runtime(rt, tag);
    let size = match map.get("Size") {
        Some(nbt::Nbt::Int(n)) => *n,
        _ => stacks(rt, handler).len() as i32,
    };
    sized(rt, handler, size);
    if let Some(nbt::Nbt::List(items)) = map.get("Items") {
        for item in items {
            let nbt::Nbt::Compound(c) = item else { continue };
            let slot = match c.get("Slot") {
                Some(nbt::Nbt::Int(n)) => *n,
                Some(nbt::Nbt::Byte(n)) => i32::from(*n),
                _ => 0,
            };
            if (0..size).contains(&slot)
                && let Some(s) = api_common::parse_stack(rt, c)
            {
                set(rt, handler, slot, s);
            }
        }
    }
    virt(rt, ON_LOAD, &[handler]);
}

fn handler_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let h = a(0);
    Some(match name {
        "<init>" => {
            match desc {
                "()V" => sized(rt, h, 1),
                "(I)V" => sized(rt, h, a(1).i()),
                // (NonNullList<ItemStack>)
                _ => rt.put_field(h, "stacks", a(1)),
            }
            V::Void
        }
        "setSize" => {
            sized(rt, h, a(1).i());
            V::Void
        }
        "getSlots" => V::I(stacks(rt, h).len() as i32),
        "getStackInSlot" => {
            if !valid_slot(rt, h, a(1).i()) {
                return Some(V::NULL);
            }
            stacks(rt, h)[a(1).i() as usize]
        }
        "setStackInSlot" => {
            if valid_slot(rt, h, a(1).i()) {
                set(rt, h, a(1).i(), a(2));
                changed(rt, h, a(1).i());
            }
            V::Void
        }
        "insertItem" => insert(rt, h, a(1).i(), a(2), a(3).i() != 0),
        "extractItem" => extract(rt, h, a(1).i(), a(2).i(), a(3).i() != 0),
        "getSlotLimit" => V::I(ABSOLUTE_MAX),
        "getStackLimit" => {
            let slot_limit = virt(rt, SLOT_LIMIT, &[h, a(1)]).i();
            let max = max_stack(rt, a(2));
            V::I(slot_limit.min(max))
        }
        "isItemValid" => V::I(1),
        "onContentsChanged" | "onLoad" => V::Void,
        "serializeNBT" => serialize(rt, h),
        "deserializeNBT" => {
            // (Provider, CompoundTag)
            deserialize(rt, h, a(2));
            V::Void
        }
        _ => return None,
    })
}

/// `ItemHandlerHelper.insertItem`: into each slot in turn; the rest comes back.
fn insert_any(rt: &mut Rt, dest: V, mut stack: V, simulate: bool) -> V {
    if dest.r() == 0 || api_common::stack_empty(rt, stack) {
        return stack;
    }
    let n = virt(rt, SLOTS, &[dest]).i();
    for i in 0..n {
        stack = virt(rt, INSERT, &[dest, V::I(i), stack, V::I(simulate as i32)]);
        if api_common::stack_empty(rt, stack) {
            return empty(rt);
        }
    }
    stack
}

/// `ItemHandlerHelper.insertItemStacked`: onto stacks of the same item first, then into empty
/// slots; the rest comes back.
fn insert_stacked(rt: &mut Rt, dest: V, mut stack: V, simulate: bool) -> V {
    if dest.r() == 0 || api_common::stack_empty(rt, stack) {
        return stack;
    }
    if max_stack(rt, stack) <= 1 {
        return insert_any(rt, dest, stack, simulate);
    }
    let n = virt(rt, SLOTS, &[dest]).i();
    for pass in 0..2 {
        for i in 0..n {
            let there = virt(rt, GET, &[dest, V::I(i)]);
            let fits = if pass == 0 { same_item(rt, there, stack) } else { api_common::stack_empty(rt, there) };
            if fits {
                stack = virt(rt, INSERT, &[dest, V::I(i), stack, V::I(simulate as i32)]);
                if api_common::stack_empty(rt, stack) {
                    return empty(rt);
                }
            }
        }
    }
    stack
}

/// `CombinedInvWrapper`: several handlers seen as one, their slots one after another. Its fields
/// are NeoForge's (`itemHandler`, `baseIndex`: where each handler's slots end, `slotCount`), counted
/// once when it's made, as NeoForge counts them.
fn combined_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    if name == "<init>" {
        let handlers = jutil::elements(rt, a(1));
        let mut ends = Vec::with_capacity(handlers.len());
        let mut index = 0;
        for &h in &handlers {
            index += virt(rt, SLOTS, &[h]).i();
            ends.push(V::I(index));
        }
        let base = rt.alloc("[I", Data::Array(ends));
        rt.put_field(w, "itemHandler", a(1));
        rt.put_field(w, "baseIndex", base);
        rt.put_field(w, "slotCount", V::I(index));
        return Some(V::Void);
    }
    if name == "getSlots" {
        return Some(V::I(rt.get_field(w, "slotCount").i()));
    }
    // `getIndexForSlot`, `getHandlerFromIndex`, `getSlotFromIndex`: the handler and its own slot;
    // none for a slot outside them all (NeoForge's empty handler answers then).
    let slot = a(1).i();
    let ends: Vec<i32> = jutil::elements(rt, rt.get_field(w, "baseIndex")).iter().map(|e| e.i()).collect();
    let handlers = jutil::elements(rt, rt.get_field(w, "itemHandler"));
    let index = if slot < 0 { None } else { ends.iter().position(|&end| slot < end) };
    let target = index.and_then(|i| handlers.get(i).copied().map(|h| (h, if i == 0 { slot } else { slot - ends[i - 1] })));
    Some(match (name, target) {
        ("getStackInSlot", Some((h, s))) => virt(rt, GET, &[h, V::I(s)]),
        ("getStackInSlot", None) => empty(rt),
        ("setStackInSlot", Some((h, s))) => {
            virt(rt, ANY_SET, &[h, V::I(s), a(2)]);
            V::Void
        }
        ("setStackInSlot", None) => V::Void,
        ("insertItem", Some((h, s))) => virt(rt, INSERT, &[h, V::I(s), a(2), a(3)]),
        ("insertItem", None) => a(2),
        ("extractItem", Some((h, s))) => virt(rt, EXTRACT, &[h, V::I(s), a(2), a(3)]),
        ("extractItem", None) => empty(rt),
        ("getSlotLimit", Some((h, s))) => virt(rt, ANY_SLOT_LIMIT, &[h, V::I(s)]),
        ("getSlotLimit", None) => V::I(0),
        ("isItemValid", Some((h, s))) => virt(rt, ANY_IS_VALID, &[h, V::I(s), a(2)]),
        ("isItemValid", None) => V::I(0),
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, HELPER | HANDLER | COMBINED | ITEM_HANDLER | MODIFIABLE | SERIALIZABLE | CONTENTS)
}

// ---- `ItemContainerContents`: the stacks a container item holds (a shulker box's, a filter's) ----

/// `ItemContainerContents` holding `items` as given (the caller copies), in slot order.
pub(crate) fn contents(rt: &mut Rt, items: Vec<V>) -> V {
    let c = rt.new_object(CONTENTS);
    let list = rt.alloc(NON_NULL_LIST, Data::List(items));
    rt.put_field(c, "$items", list);
    c
}

/// The stacks `ItemContainerContents` holds, in slot order, empty ones too.
pub(crate) fn contents_items(rt: &Rt, c: V) -> Vec<V> {
    if c.r() == 0 { Vec::new() } else { jutil::elements(rt, rt.get_field(c, "$items")) }
}

/// `ItemContainerContents.EMPTY`.
fn no_contents(rt: &mut Rt) -> V {
    rt.intern("compat:contents-empty".into(), |rt| contents(rt, Vec::new()))
}

fn copy(rt: &mut Rt, stack: V) -> V {
    api::call(rt, "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;", &[stack])
}

/// `ItemContainerContents.fromItems(list)`: copies up to the last stack that isn't empty; EMPTY when
/// all are.
pub(crate) fn contents_from_items(rt: &mut Rt, items: &[V]) -> V {
    let Some(last) = items.iter().rposition(|&s| !api_common::stack_empty(rt, s)) else {
        return no_contents(rt);
    };
    let mut out = Vec::with_capacity(last + 1);
    for &s in &items[..=last] {
        out.push(copy(rt, s));
    }
    contents(rt, out)
}

/// `ItemContainerContents` from slots as its codec reads them (index, stack): as many slots as the
/// highest index needs, the rest empty; EMPTY for none.
pub(crate) fn contents_from_slots(rt: &mut Rt, slots: Vec<(usize, V)>) -> V {
    let Some(n) = slots.iter().map(|(i, _)| i + 1).max() else {
        return no_contents(rt);
    };
    let e = empty(rt);
    let mut items = vec![e; n];
    for (i, s) in slots {
        items[i] = s;
    }
    contents(rt, items)
}

fn contents_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "fromItems" => {
            let items = jutil::elements(rt, a(0));
            contents_from_items(rt, &items)
        }
        // NeoForge's: how many slots it holds, and a copy of one (past them, as NeoForge's
        // `validateSlotIndex`, an UnsupportedOperationException).
        "getSlots" => V::I(contents_items(rt, a(0)).len() as i32),
        "getStackInSlot" => {
            let items = contents_items(rt, a(0));
            match usize::try_from(a(1).i()).ok().and_then(|i| items.get(i)) {
                Some(&s) => copy(rt, s),
                None => {
                    rt.throw_new("java/lang/UnsupportedOperationException");
                    V::NULL
                }
            }
        }
        "copyInto" => {
            let items = contents_items(rt, a(0));
            let n = jutil::elements(rt, a(1)).len();
            for i in 0..n {
                let s = match items.get(i) {
                    Some(&s) => copy(rt, s),
                    None => empty(rt),
                };
                if let Some(crate::Object { data: Data::List(l), .. }) = rt.heap.get_mut(a(1).r() as usize) {
                    l[i] = s;
                }
            }
            V::Void
        }
        "copyOne" => match contents_items(rt, a(0)).first() {
            Some(&s) => copy(rt, s),
            None => empty(rt),
        },
        "stream" | "nonEmptyStream" => {
            let mut out = Vec::new();
            for s in contents_items(rt, a(0)) {
                if name == "stream" || !api_common::stack_empty(rt, s) {
                    out.push(copy(rt, s));
                }
            }
            rt.alloc("java/util/stream/Stream", Data::Stream(out))
        }
        // The stacks themselves (Guava's filtered view), or copies of them.
        "nonEmptyItems" | "nonEmptyItemsCopy" => {
            let mut out = Vec::new();
            for s in contents_items(rt, a(0)) {
                if !api_common::stack_empty(rt, s) {
                    out.push(if name == "nonEmptyItems" { s } else { copy(rt, s) });
                }
            }
            jutil::new_list(rt, out)
        }
        // `ItemStack.listMatches`: the same stacks in the same slots, components and all.
        "equals" => {
            let other = a(1);
            if other == a(0) {
                return Some(V::I(1));
            }
            if other.r() == 0 || !rt.instance_of(other, CONTENTS) {
                return Some(V::I(0));
            }
            let (x, y) = (contents_items(rt, a(0)), contents_items(rt, other));
            let mut same = x.len() == y.len();
            for (&p, &q) in x.iter().zip(&y) {
                if !same {
                    break;
                }
                same = count(rt, p) == count(rt, q) && same_item(rt, p, q);
            }
            V::I(same as i32)
        }
        "hashCode" => {
            let mut h: i32 = 0;
            for s in contents_items(rt, a(0)) {
                let one = if api_common::stack_empty(rt, s) {
                    0
                } else {
                    let id = api_common::item_like_name(rt, rt.get_field(s, "$item"));
                    let id_hash = id.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
                    id_hash.wrapping_mul(31).wrapping_add(count(rt, s))
                };
                h = h.wrapping_mul(31).wrapping_add(one);
            }
            V::I(h)
        }
        _ => return None,
    })
}

/// `ItemContainerContents.EMPTY`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (CONTENTS, "EMPTY") => Some(no_contents(rt)),
        _ => None,
    }
}

/// [`call`] by the call's key (`owner.name.desc`).
pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on item handlers; None for what this module doesn't model.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        HELPER => Some(match name {
            "insertItem" => insert_any(rt, a(0), a(1), a(2).i() != 0),
            "insertItemStacked" => insert_stacked(rt, a(0), a(1), a(2).i() != 0),
            // (Player, ItemStack[, preferredSlot]): what doesn't fit stays with the stack.
            "giveItemToPlayer" => {
                api::call(rt, "net/minecraft/world/entity/player/Inventory.placeItemBackInInventory.(Lnet/minecraft/world/item/ItemStack;)V", &[V::NULL, a(1)]);
                V::Void
            }
            "canItemStacksStack" => V::I(same_item(rt, a(0), a(1)) as i32),
            "copyStackWithSize" => copy_with_count(rt, a(0), a(1).i()),
            _ => return None,
        }),
        HANDLER => handler_call(rt, name, desc, args),
        CONTENTS => contents_call(rt, name, args),
        COMBINED => combined_call(rt, name, args),
        ITEM_HANDLER | MODIFIABLE if !args.is_empty() && rt.instance_of(a(0), COMBINED) => combined_call(rt, name, args),
        ITEM_HANDLER | MODIFIABLE | SERIALIZABLE if !args.is_empty() && rt.instance_of(a(0), HANDLER) => handler_call(rt, name, desc, args),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::Nbt;

    fn key(name: &str) -> &'static str {
        Box::leak(format!("{HANDLER}.{name}").into_boxed_str())
    }

    #[test]
    fn a_handler_stacks_what_fits_hands_back_the_rest_and_saves_its_slots() {
        let mut rt = Rt::new();
        crate::install();
        let h = rt.new_object(HANDLER);
        api::call(&mut rt, key("<init>.(I)V"), &[h, V::I(2)]);
        let iron = api_common::stack(&mut rt, "minecraft:iron_ingot", 40);
        let insert = key("insertItem.(ILnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/item/ItemStack;");
        let rest = api::call(&mut rt, insert, &[h, V::I(0), iron, V::I(0)]);
        assert!(api_common::stack_empty(&rt, rest));
        let more = api_common::stack(&mut rt, "minecraft:iron_ingot", 40);
        let rest = api::call(&mut rt, insert, &[h, V::I(0), more, V::I(0)]);
        // A slot holds a stack's worth (64): 16 of the second 40 come back.
        assert_eq!(rt.get_field(rest, "$count"), V::I(16));
        let gold = api_common::stack(&mut rt, "minecraft:gold_ingot", 1);
        let refused = api::call(&mut rt, insert, &[h, V::I(0), gold, V::I(0)]);
        assert_eq!(refused, gold);
        // Stacked: the gold goes to the empty slot, not onto the iron.
        let helper = "net/neoforged/neoforge/items/ItemHandlerHelper.insertItemStacked.(Lnet/neoforged/neoforge/items/IItemHandler;Lnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/item/ItemStack;";
        let rest = api::call(&mut rt, helper, &[h, gold, V::I(0)]);
        assert!(api_common::stack_empty(&rt, rest));
        let extract = key("extractItem.(IIZ)Lnet/minecraft/world/item/ItemStack;");
        let took = api::call(&mut rt, extract, &[h, V::I(0), V::I(10), V::I(0)]);
        assert_eq!(rt.get_field(took, "$count"), V::I(10));
        // Saved and read back into a new handler.
        let tag = api::call(&mut rt, key("serializeNBT.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;"), &[h, V::NULL]);
        let back = rt.new_object(HANDLER);
        api::call(&mut rt, key("<init>.()V"), &[back]);
        api::call(&mut rt, key("deserializeNBT.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)V"), &[back, V::NULL, tag]);
        let slots: Vec<(String, i32)> = stacks(&rt, back)
            .into_iter()
            .map(|s| (api_common::item_like_name(&rt, rt.get_field(s, "$item")), count(&rt, s)))
            .collect();
        assert_eq!(slots, [("minecraft:iron_ingot".to_owned(), 54), ("minecraft:gold_ingot".to_owned(), 1)]);
    }

    /// A filter's contents as Create saves them: `ItemContainerContents.fromItems` over a handler's
    /// 18 slots, written by `CODEC` the way vanilla's `ItemContainerContents.Slot` codec writes
    /// (`[{slot, item: {id, count}}]`, empty slots left out) and read back.
    #[test]
    fn container_contents_keep_their_slots_through_the_codec() {
        let mut rt = Rt::new();
        crate::install();
        let e = empty(&mut rt);
        let mut items = vec![e; 18];
        items[0] = api_common::stack(&mut rt, "minecraft:diamond", 1);
        items[3] = api_common::stack(&mut rt, "minecraft:iron_ingot", 5);
        let list = rt.alloc(NON_NULL_LIST, Data::List(items.clone()));
        let from = "net/minecraft/world/item/component/ItemContainerContents.fromItems.(Ljava/util/List;)Lnet/minecraft/world/item/component/ItemContainerContents;";
        let c = api::call(&mut rt, from, &[list]);
        // Up to the last stack that isn't empty, each a copy.
        let held = contents_items(&rt, c);
        assert_eq!(held.len(), 4);
        assert_ne!(held[0], items[0]);
        assert_eq!(count(&rt, held[3]), 5);
        let codec = api::get_static(&mut rt, "net/minecraft/world/item/component/ItemContainerContents.CODEC");
        let nbt = crate::codecs::encode(&mut rt, codec, c).unwrap();
        let item = |id: &str, n: i32| Nbt::Compound([("id".to_owned(), Nbt::String(id.into())), ("count".to_owned(), Nbt::Int(n))].into());
        let slot = |i: i32, it: Nbt| Nbt::Compound([("slot".to_owned(), Nbt::Int(i)), ("item".to_owned(), it)].into());
        assert_eq!(nbt, Nbt::List(vec![slot(0, item("minecraft:diamond", 1)), slot(3, item("minecraft:iron_ingot", 5))]));
        let json = serde_json::json!([{"slot": 3, "item": {"id": "minecraft:iron_ingot", "count": 5}}, {"slot": 0, "item": {"id": "minecraft:diamond"}}]);
        let back = crate::codecs::decode(&mut rt, codec, &json).unwrap();
        let equals = "net/minecraft/world/item/component/ItemContainerContents.equals.(Ljava/lang/Object;)Z";
        assert_eq!(api::call(&mut rt, equals, &[back, c]), V::I(1));
        // NeoForge's slot view, and copying into a list of the handler's size.
        let slots = "net/minecraft/world/item/component/ItemContainerContents.getSlots.()I";
        assert_eq!(api::call(&mut rt, slots, &[back]), V::I(4));
        let into = rt.alloc(NON_NULL_LIST, Data::List(vec![e; 18]));
        api::call(&mut rt, "net/minecraft/world/item/component/ItemContainerContents.copyInto.(Lnet/minecraft/core/NonNullList;)V", &[back, into]);
        let got: Vec<i32> = jutil::elements(&rt, into).into_iter().map(|s| count(&rt, s)).collect();
        assert_eq!(got, [1, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // A slot past 255 is refused, as `Codec.intRange(0, 255)` refuses it.
        let bad = serde_json::json!([{"slot": 256, "item": {"id": "minecraft:diamond"}}]);
        assert!(crate::codecs::decode(&mut rt, codec, &bad).is_err());
    }
}
