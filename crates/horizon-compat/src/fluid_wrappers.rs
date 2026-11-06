//! NeoForge's fluid handlers for vanilla's own containers, natively, as NeoForge writes them:
//! - `FluidBucketWrapper`, a bucket stack's `Capabilities.FluidHandler.ITEM`: NeoForge's fallback
//!   for every item whose class is exactly `BucketItem` (vanilla's empty, water and lava buckets and
//!   the buckets mods make of that class), after the mods' own providers. Mods make it and extend it
//!   themselves too (Mekanism, SecurityCraft, Ad Astra, Architectury...): its state is NeoForge's
//!   protected field `container`, and the steps a subclass may override (`getFluid`, `setFluid`,
//!   `canFillFluidType`) are called virtually.
//! - `CauldronWrapper`, a cauldron's `FluidHandler.BLOCK` (NeoForge's own, ahead of every mod's
//!   provider), by `CauldronFluidContent`: vanilla's empty, water and lava cauldrons and those mods
//!   register with `RegisterCauldronFluidContentEvent` (CreateDragonsPlus's dragon's breath). A
//!   cauldron takes and gives whole increments only: a water cauldron's three levels hold 1000 mB,
//!   so it fills from empty to full and drains from full to empty.
//! - `FluidUtil`'s lookups and transfers: `getFilledBucket`, `getFluidHandler`,
//!   `getFluidContained`, `tryFluidTransfer`.
//!
//! Create's spout fills an empty bucket and its item drain empties a full one through the bucket's
//! wrapper (`GenericItemFilling` checks the wrapper's exact class).
use crate::{Data, Rt, V, api, fluids, jutil, tanks};

const BUCKET_WRAPPER: &str = "net/neoforged/neoforge/fluids/capability/wrappers/FluidBucketWrapper";
const CAULDRON_WRAPPER: &str = "net/neoforged/neoforge/fluids/capability/wrappers/CauldronWrapper";
const CONTENT: &str = "net/neoforged/neoforge/fluids/CauldronFluidContent";
const REGISTER: &str = "net/neoforged/neoforge/fluids/RegisterCauldronFluidContentEvent";
const FLUID_UTIL: &str = "net/neoforged/neoforge/fluids/FluidUtil";
const HANDLER: &str = "net/neoforged/neoforge/fluids/capability/IFluidHandler";
const ITEM_HANDLER: &str = "net/neoforged/neoforge/fluids/capability/IFluidHandlerItem";
const BUCKET_ITEM: &str = "net/minecraft/world/item/BucketItem";
const MILK_BUCKET: &str = "net/minecraft/world/item/MilkBucketItem";
/// Where [`registry`] keeps the cauldrons' contents, vanilla's first.
const CONTENTS: &str = "compat:cauldron-contents";

// The bucket wrapper's overridable steps, called virtually: (the API's key, the method's).
const GET_FLUID: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/wrappers/FluidBucketWrapper.getFluid.()Lnet/neoforged/neoforge/fluids/FluidStack;",
    "getFluid.()Lnet/neoforged/neoforge/fluids/FluidStack;",
);
const SET_FLUID: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/wrappers/FluidBucketWrapper.setFluid.(Lnet/neoforged/neoforge/fluids/FluidStack;)V",
    "setFluid.(Lnet/neoforged/neoforge/fluids/FluidStack;)V",
);
const CAN_FILL: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/wrappers/FluidBucketWrapper.canFillFluidType.(Lnet/neoforged/neoforge/fluids/FluidStack;)Z",
    "canFillFluidType.(Lnet/neoforged/neoforge/fluids/FluidStack;)Z",
);
/// `FluidType.getBucket(stack)`: a mod's type may name its own bucket.
const TYPE_BUCKET: (&str, &str) = (
    "net/neoforged/neoforge/fluids/FluidType.getBucket.(Lnet/neoforged/neoforge/fluids/FluidStack;)Lnet/minecraft/world/item/ItemStack;",
    "getBucket.(Lnet/neoforged/neoforge/fluids/FluidStack;)Lnet/minecraft/world/item/ItemStack;",
);
// What `FluidUtil` asks of the handlers it is given.
const FILL: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/IFluidHandler.fill.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)I",
    "fill.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)I",
);
const DRAIN: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/IFluidHandler.drain.(ILnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;",
    "drain.(ILnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;",
);
const DRAIN_STACK: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/IFluidHandler.drain.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;",
    "drain.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;",
);
const SAME: &str = "net/neoforged/neoforge/fluids/FluidStack.isSameFluidSameComponents.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/FluidStack;)Z";
const BUCKET_FLUID: &str = "net/minecraft/world/item/BucketItem.getFluid.()Lnet/minecraft/world/level/material/Fluid;";
const GET_STATE: &str = "net/minecraft/world/level/Level.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const SET_STATE: &str = "net/minecraft/world/level/Level.setBlockAndUpdate.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z";
const DEFAULT_STATE: &str = "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
const SET_VALUE: &str = "net/minecraft/world/level/block/state/StateHolder.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;";
const ITEM_CAPABILITY: &str = "net/minecraft/world/item/ItemStack.getCapability.(Lnet/neoforged/neoforge/capabilities/ItemCapability;)Ljava/lang/Object;";
const BLOCK_CAPABILITY: &str = "net/minecraft/world/level/Level.getCapability.(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Ljava/lang/Object;";
const COPY_WITH_COUNT: &str = "net/minecraft/world/item/ItemStack.copyWithCount.(I)Lnet/minecraft/world/item/ItemStack;";

fn throw(rt: &mut Rt, class: &'static str, message: &str) {
    let e = rt.new_object(class);
    let m = rt.string(message);
    rt.put_field(e, "message", m);
    rt.thrown = Some(e);
}

/// Whether `c` is `class` among these handlers' types (`instanceof IFluidHandlerItem`); NeoForge's
/// tank is one too.
pub(crate) fn is_a(c: &str, class: &str) -> bool {
    match class {
        HANDLER => matches!(c, BUCKET_WRAPPER | CAULDRON_WRAPPER | ITEM_HANDLER | "net/neoforged/neoforge/fluids/capability/templates/FluidTank"),
        ITEM_HANDLER => c == BUCKET_WRAPPER,
        _ => false,
    }
}

// --- FluidBucketWrapper -------------------------------------------------------------------------

/// A `FluidBucketWrapper` of `stack`, as NeoForge's fallback provider makes it.
pub(crate) fn bucket(rt: &mut Rt, stack: V) -> V {
    let w = rt.new_object(BUCKET_WRAPPER);
    rt.put_field(w, "container", stack);
    w
}

/// Whether NeoForge's fallback gives `stack` a bucket wrapper: its item's class is exactly
/// `BucketItem` (`item.getClass() == BucketItem.class`), so not a fish bucket nor a mod's own
/// subclass. Milk's would need NeoForge's milk fluid, which isn't registered here.
pub(crate) fn plain_bucket(rt: &Rt, stack: V) -> bool {
    let item = rt.get_field(stack, "$item");
    item.r() != 0 && rt.type_of(item) == BUCKET_ITEM
}

/// `ItemStack.getCount()`: 0 for an empty stack.
fn count(rt: &Rt, stack: V) -> i32 {
    if crate::api_common::stack_empty(rt, stack) { 0 } else { rt.get_field(stack, "$count").i() }
}

/// `getFluid()`: a bucket item's fluid, a bucketful of it (of `minecraft:empty` for the empty
/// bucket: an empty stack); nothing for any other item.
fn bucket_fluid(rt: &mut Rt, w: V) -> V {
    let item = rt.get_field(rt.get_field(w, "container"), "$item");
    let f = if rt.instance_of(item, BUCKET_ITEM) { api::call(rt, BUCKET_FLUID, &[item]) } else { V::NULL };
    if f.r() == 0 { fluids::fluid_stack_empty(rt) } else { fluids::fluid_stack(rt, f, 1000) }
}

/// `FluidUtil.getFilledBucket(stack)`: vanilla's water or lava bucket, else the bucket the fluid's
/// type gives (`FluidType.getBucket(stack)`, a mod's override included).
pub(crate) fn filled_bucket(rt: &mut Rt, stack: V) -> V {
    let f = rt.get_field(stack, "$fluid");
    match fluids::name_of(rt, f).as_str() {
        "minecraft:water" => crate::api_common::stack(rt, "minecraft:water_bucket", 1),
        "minecraft:lava" => crate::api_common::stack(rt, "minecraft:lava_bucket", 1),
        _ => {
            let ty = fluids::type_of(rt, f);
            tanks::virt(rt, TYPE_BUCKET, &[ty, stack])
        }
    }
}

/// `canFillFluidType(stack)`: water, lava, or a fluid whose type has a bucket.
fn can_fill_type(rt: &mut Rt, stack: V) -> bool {
    let f = rt.get_field(stack, "$fluid");
    if matches!(fluids::name_of(rt, f).as_str(), "minecraft:water" | "minecraft:lava") {
        return true;
    }
    if f.r() == 0 {
        return false;
    }
    let ty = fluids::type_of(rt, f);
    let b = tanks::virt(rt, TYPE_BUCKET, &[ty, stack]);
    !crate::api_common::stack_empty(rt, b)
}

fn bucket_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(w, "container", a(1));
            V::Void
        }
        "getContainer" => rt.get_field(w, "container"),
        "getFluid" => bucket_fluid(rt, w),
        // The container becomes an empty bucket, or the filled bucket of what was put in.
        "setFluid" => {
            let b = if fluids::stack_empty(rt, a(1)) { crate::api_common::stack(rt, "minecraft:bucket", 1) } else { filled_bucket(rt, a(1)) };
            rt.put_field(w, "container", b);
            V::Void
        }
        "canFillFluidType" => V::I(can_fill_type(rt, a(1)) as i32),
        "getTanks" => V::I(1),
        "getFluidInTank" => tanks::virt(rt, GET_FLUID, &[w]),
        "getTankCapacity" => V::I(1000),
        "isFluidValid" => V::I(1),
        // A whole bucketful into one empty bucket (not milk's), of a fluid it can hold.
        "fill" => {
            let (resource, simulate) = (a(1), tanks::simulating(rt, a(2)));
            let c = rt.get_field(w, "container");
            let item = rt.get_field(c, "$item");
            if count(rt, c) != 1 || fluids::stack_amount(rt, resource) < 1000 || rt.instance_of(item, MILK_BUCKET) {
                return Some(V::I(0));
            }
            let have = tanks::virt(rt, GET_FLUID, &[w]);
            if !fluids::stack_empty(rt, have) || tanks::virt(rt, CAN_FILL, &[w, resource]).i() == 0 {
                return Some(V::I(0));
            }
            if !simulate {
                tanks::virt(rt, SET_FLUID, &[w, resource]);
            }
            V::I(1000)
        }
        // The whole bucketful out of one bucket, (FluidStack, action) only of the same fluid.
        "drain" => {
            let by_stack = desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidStack;");
            let max = if by_stack { fluids::stack_amount(rt, a(1)) } else { a(1).i() };
            let simulate = tanks::simulating(rt, a(2));
            let c = rt.get_field(w, "container");
            if count(rt, c) != 1 || max < 1000 {
                return Some(fluids::fluid_stack_empty(rt));
            }
            let have = tanks::virt(rt, GET_FLUID, &[w]);
            if fluids::stack_empty(rt, have) || (by_stack && api::call(rt, SAME, &[have, a(1)]).i() == 0) {
                return Some(fluids::fluid_stack_empty(rt));
            }
            if !simulate {
                let e = fluids::fluid_stack_empty(rt);
                tanks::virt(rt, SET_FLUID, &[w, e]);
            }
            have
        }
        _ => return None,
    })
}

// --- CauldronFluidContent and CauldronWrapper --------------------------------------------------

/// The cauldrons' contents (`CauldronFluidContent`), registered the first time they are asked for
/// as NeoForge's `CauldronFluidContent.init()` does: vanilla's three, then the mods' through
/// `RegisterCauldronFluidContentEvent`.
fn registry(rt: &mut Rt) -> V {
    if let Some(&l) = rt.interned.get(CONTENTS) {
        return l;
    }
    let l = rt.alloc("compat:list", Data::List(Vec::new()));
    rt.interned.insert(CONTENTS.into(), l);
    let level = api::get_static(rt, "net/minecraft/world/level/block/LayeredCauldronBlock.LEVEL");
    for (block, fluid, levels) in [("minecraft:cauldron", "minecraft:empty", V::NULL), ("minecraft:water_cauldron", "minecraft:water", level), ("minecraft:lava_cauldron", "minecraft:lava", V::NULL)] {
        let (b, f) = (api::block(rt, block), fluids::fluid(rt, fluid));
        register(rt, b, f, 1000, levels);
    }
    let ev = rt.new_object(REGISTER);
    crate::neo::post(rt, REGISTER, ev);
    l
}

/// Registers the cauldrons' contents before anything asks for a capability, as NeoForge's
/// `RegistrationEvents.init` does.
pub(crate) fn init(rt: &mut Rt) {
    registry(rt);
}

/// `RegisterCauldronFluidContentEvent.register(block, fluid, totalAmount, levelProperty)`: refused
/// with NeoForge's exceptions for a block or fluid already registered, an amount that isn't
/// positive, or levels that don't start at 1.
fn register(rt: &mut Rt, block: V, fluid: V, total: i32, levels: V) {
    let list = registry(rt);
    let (b, f) = (api::block_name(rt, block), fluids::name_of(rt, fluid));
    for c in jutil::elements(rt, list) {
        if api::block_name(rt, rt.get_field(c, "block")) == b {
            return throw(rt, "java/lang/IllegalArgumentException", &format!("Duplicate cauldron registration for block Block{{{b}}}."));
        }
        if fluids::name_of(rt, rt.get_field(c, "fluid")) == f {
            return throw(rt, "java/lang/IllegalArgumentException", &format!("Duplicate cauldron registration for fluid {f}."));
        }
    }
    if total <= 0 {
        return throw(rt, "java/lang/IllegalArgumentException", &format!("Cauldron total amount {total} should be positive."));
    }
    let max = if levels.r() == 0 {
        1
    } else {
        let values: Vec<i32> = crate::blocks::property_values(rt, levels).iter().filter_map(|v| v.parse().ok()).collect();
        match (values.iter().min(), values.iter().max()) {
            (None, _) | (_, None) => return throw(rt, "java/lang/IllegalArgumentException", "Cauldron should have at least one possible level."),
            (Some(&lowest), Some(&highest)) if lowest == 1 => highest,
            _ => return throw(rt, "java/lang/IllegalStateException", "Minimum level should be 1, and maximum level should be >= 1."),
        }
    };
    let c = rt.new_object(CONTENT);
    for (k, v) in [("block", block), ("fluid", fluid), ("totalAmount", V::I(total)), ("maxLevel", V::I(max)), ("levelProperty", levels)] {
        rt.put_field(c, k, v);
    }
    jutil::list_add(rt, list, c);
}

/// The contents registered for the block named `block`.
fn for_block(rt: &mut Rt, block: &str) -> Option<V> {
    let list = registry(rt);
    jutil::elements(rt, list).into_iter().find(|&c| api::block_name(rt, rt.get_field(c, "block")) == block)
}

/// The contents registered for `fluid` (a source: flowing water has none).
fn for_fluid(rt: &mut Rt, fluid: V) -> Option<V> {
    if fluid.r() == 0 {
        return None;
    }
    let name = fluids::name_of(rt, fluid);
    let list = registry(rt);
    jutil::elements(rt, list).into_iter().find(|&c| fluids::name_of(rt, rt.get_field(c, "fluid")) == name)
}

/// Whether the block named `block` is one of the cauldrons NeoForge gives a fluid handler.
pub(crate) fn is_cauldron(rt: &mut Rt, block: &str) -> bool {
    for_block(rt, block).is_some()
}

fn int_field(rt: &Rt, c: V, f: &'static str) -> i32 {
    rt.get_field(c, f).i()
}

/// `CauldronFluidContent.currentLevel(state)`: none for the empty cauldron, 1 for a cauldron
/// without levels, else the level `state` (its text) is at.
fn current_level(rt: &Rt, c: V, state: &str) -> i32 {
    if fluids::name_of(rt, rt.get_field(c, "fluid")) == "minecraft:empty" {
        return 0;
    }
    let levels = rt.get_field(c, "levelProperty");
    if levels.r() == 0 {
        return 1;
    }
    let name = crate::api_common::prop_parts(rt, levels).0;
    let (_, props) = crate::host::split_state(state);
    props.iter().find(|(k, _)| *k == name).and_then(|(_, v)| v.parse().ok()).unwrap_or(0)
}

/// NeoForge's `FluidHandler.BLOCK` of the cauldron at `pos` (`CauldronFluidContent.registerCapabilities`,
/// ahead of every mod's provider): a `CauldronWrapper`; None for another block.
pub(crate) fn cauldron(rt: &mut Rt, level: V, pos: V, block: &str) -> Option<V> {
    for_block(rt, block)?;
    let w = rt.new_object(CAULDRON_WRAPPER);
    rt.put_field(w, "level", level);
    rt.put_field(w, "pos", pos);
    Some(w)
}

/// The wrapper's block state now (its text) and its contents; None, an `IllegalStateException`
/// thrown as NeoForge's `getContent` throws it, when the block is no cauldron any more.
fn cauldron_here(rt: &mut Rt, w: V) -> Option<(String, V)> {
    let (level, pos) = (rt.get_field(w, "level"), rt.get_field(w, "pos"));
    let s = api::call(rt, GET_STATE, &[level, pos]);
    let text = rt.str_of(s).unwrap_or("minecraft:air").to_owned();
    let block = crate::host::split_state(&text).0.to_owned();
    if let Some(c) = for_block(rt, &block) {
        return Some((text, c));
    }
    let p = api::pos_of(rt, pos);
    throw(rt, "java/lang/IllegalStateException", &format!("Unexpected error: no cauldron at location BlockPos{{x={}, y={}, z={}}}", p[0], p[1], p[2]));
    None
}

/// Puts the cauldron of contents `c` at `level` in the wrapper's place (`updateLevel`); level 0 is
/// the empty cauldron.
fn set_level(rt: &mut Rt, w: V, c: V, level: i32) {
    let block = if level == 0 { api::block(rt, "minecraft:cauldron") } else { rt.get_field(c, "block") };
    let mut state = api::call(rt, DEFAULT_STATE, &[block]);
    let levels = rt.get_field(c, "levelProperty");
    if level > 0 && levels.r() != 0 {
        state = api::call(rt, SET_VALUE, &[state, levels, V::I(level)]);
    }
    let (at, pos) = (rt.get_field(w, "level"), rt.get_field(w, "pos"));
    api::call(rt, SET_STATE, &[at, pos, state]);
}

fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// What one whole increment of contents `c` is: (mB, levels), by `IntMath.gcd(maxLevel, totalAmount)`.
fn increments(rt: &Rt, c: V) -> (i32, i32) {
    let (total, max) = (int_field(rt, c, "totalAmount"), int_field(rt, c, "maxLevel"));
    let d = gcd(max, total).max(1);
    (total / d, max / d)
}

/// `fill(resource, action)`: whole increments of a fluid that has a cauldron, into an empty one or
/// one of the same fluid, up to the top level; the mB that go in.
fn cauldron_fill(rt: &mut Rt, w: V, resource: V, simulate: bool) -> i32 {
    if fluids::stack_empty(rt, resource) {
        return 0;
    }
    let f = rt.get_field(resource, "$fluid");
    let Some(insert) = for_fluid(rt, f) else { return 0 };
    let Some((state, current)) = cauldron_here(rt, w) else { return 0 };
    let held = fluids::name_of(rt, rt.get_field(current, "fluid"));
    if held != "minecraft:empty" && held != fluids::name_of(rt, f) {
        return 0;
    }
    let (amount_step, level_step) = increments(rt, insert);
    let level = current_level(rt, current, &state);
    let room = (int_field(rt, insert, "maxLevel") - level) / level_step;
    let n = (fluids::stack_amount(rt, resource) / amount_step).min(room).max(0);
    if n > 0 && !simulate {
        set_level(rt, w, insert, level + n * level_step);
    }
    n * amount_step
}

/// `drain(state, maxDrain, action)`: whole increments off the top of contents `c`; what comes out.
fn cauldron_drain(rt: &mut Rt, w: V, state: &str, c: V, max: i32, simulate: bool) -> V {
    let (amount_step, level_step) = increments(rt, c);
    let level = current_level(rt, c, state);
    let n = (max / amount_step).min(level / level_step);
    if n <= 0 {
        return fluids::fluid_stack_empty(rt);
    }
    if !simulate {
        set_level(rt, w, c, level - n * level_step);
    }
    let f = rt.get_field(c, "fluid");
    fluids::fluid_stack(rt, f, n * amount_step)
}

fn cauldron_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(w, "level", a(1));
            rt.put_field(w, "pos", a(2));
            V::Void
        }
        "getTanks" => V::I(1),
        "getFluidInTank" => {
            let Some((state, c)) = cauldron_here(rt, w) else { return Some(V::NULL) };
            let (f, total, max) = (rt.get_field(c, "fluid"), int_field(rt, c, "totalAmount"), int_field(rt, c, "maxLevel"));
            let amount = total * current_level(rt, c, &state) / max.max(1);
            fluids::fluid_stack(rt, f, amount)
        }
        "getTankCapacity" => match cauldron_here(rt, w) {
            Some((_, c)) => V::I(int_field(rt, c, "totalAmount")),
            None => V::I(0),
        },
        "isFluidValid" => {
            let f = rt.get_field(a(2), "$fluid");
            V::I(for_fluid(rt, f).is_some() as i32)
        }
        "fill" => {
            let simulate = tanks::simulating(rt, a(2));
            V::I(cauldron_fill(rt, w, a(1), simulate))
        }
        // (FluidStack, action): only the cauldron's own fluid comes out.
        "drain" if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidStack;") => {
            if fluids::stack_empty(rt, a(1)) {
                return Some(fluids::fluid_stack_empty(rt));
            }
            let simulate = tanks::simulating(rt, a(2));
            let Some((state, c)) = cauldron_here(rt, w) else { return Some(V::NULL) };
            if fluids::name_of(rt, rt.get_field(c, "fluid")) != fluids::name_of(rt, rt.get_field(a(1), "$fluid")) {
                return Some(fluids::fluid_stack_empty(rt));
            }
            let max = fluids::stack_amount(rt, a(1));
            cauldron_drain(rt, w, &state, c, max, simulate)
        }
        "drain" => {
            let (max, simulate) = (a(1).i(), tanks::simulating(rt, a(2)));
            if max <= 0 {
                return Some(fluids::fluid_stack_empty(rt));
            }
            let Some((state, c)) = cauldron_here(rt, w) else { return Some(V::NULL) };
            cauldron_drain(rt, w, &state, c, max, simulate)
        }
        _ => return None,
    })
}

// --- FluidUtil ----------------------------------------------------------------------------------

/// `stack.getCapability(Capabilities.FluidHandler.ITEM)`.
fn item_handler(rt: &mut Rt, stack: V) -> V {
    let cap = api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.ITEM");
    api::call(rt, ITEM_CAPABILITY, &[stack, cap])
}

/// `tryFluidTransfer_Internal`: what `dest` takes of `drainable`, drained from `source` and put in
/// when `transfer`; the fluid moved (or that would move).
fn transfer(rt: &mut Rt, dest: V, source: V, drainable: V, transfer: bool) -> V {
    let simulate = tanks::action(rt, true);
    let fillable = tanks::virt(rt, FILL, &[dest, drainable, simulate]).i();
    if fillable <= 0 {
        return fluids::fluid_stack_empty(rt);
    }
    rt.put_field(drainable, "$amount", V::I(fillable));
    if !transfer {
        return drainable;
    }
    let execute = tanks::action(rt, false);
    let drained = tanks::virt(rt, DRAIN_STACK, &[source, drainable, execute]);
    if fluids::stack_empty(rt, drained) {
        return fluids::fluid_stack_empty(rt);
    }
    let filled = tanks::virt(rt, FILL, &[dest, drained, execute]).i();
    rt.put_field(drained, "$amount", V::I(filled));
    drained
}

fn util_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "getFilledBucket" => filled_bucket(rt, a(0)),
        "getFluidHandler" if desc.starts_with("(Lnet/minecraft/world/item/ItemStack;)") => {
            let h = item_handler(rt, a(0));
            crate::api_common::optional(rt, h)
        }
        // (level, pos, side)
        "getFluidHandler" => {
            let cap = api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.BLOCK");
            let h = api::call(rt, BLOCK_CAPABILITY, &[a(0), cap, a(1), a(2)]);
            crate::api_common::optional(rt, h)
        }
        // What one of the stack holds, as its handler would give it all up; nothing for an empty
        // stack, one without a handler, or an empty container.
        "getFluidContained" => {
            let mut held = V::NULL;
            if !crate::api_common::stack_empty(rt, a(0)) {
                let one = api::call(rt, COPY_WITH_COUNT, &[a(0), V::I(1)]);
                let h = item_handler(rt, one);
                if h.r() != 0 {
                    let simulate = tanks::action(rt, true);
                    let s = tanks::virt(rt, DRAIN, &[h, V::I(i32::MAX), simulate]);
                    if !fluids::stack_empty(rt, s) {
                        held = s;
                    }
                }
            }
            crate::api_common::optional(rt, held)
        }
        // (destination, source, maxAmount | resource, doTransfer)
        "tryFluidTransfer" => {
            let (dest, source, move_it) = (a(0), a(1), a(3).i() != 0);
            let simulate = tanks::action(rt, true);
            let drainable = if desc.contains(";IZ)") {
                tanks::virt(rt, DRAIN, &[source, a(2), simulate])
            } else {
                let d = tanks::virt(rt, DRAIN_STACK, &[source, a(2), simulate]);
                if !fluids::stack_empty(rt, d) && api::call(rt, SAME, &[a(2), d]).i() == 0 {
                    return Some(fluids::fluid_stack_empty(rt));
                }
                d
            };
            if fluids::stack_empty(rt, drainable) {
                return Some(fluids::fluid_stack_empty(rt));
            }
            transfer(rt, dest, source, drainable, move_it)
        }
        _ => return None,
    })
}

// --- Dispatch -----------------------------------------------------------------------------------

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, BUCKET_WRAPPER | CAULDRON_WRAPPER | CONTENT | REGISTER | FLUID_UTIL | HANDLER | ITEM_HANDLER)
}

/// [`call`] by the call's key (`owner.name.desc`).
pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on the wrappers, cauldron contents and `FluidUtil`; None for anything else.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        BUCKET_WRAPPER => bucket_call(rt, name, desc, args),
        CAULDRON_WRAPPER => cauldron_call(rt, name, desc, args),
        CONTENT => Some(match name {
            "getForBlock" => {
                let b = api::block_name(rt, a(0));
                for_block(rt, &b).unwrap_or(V::NULL)
            }
            "getForFluid" => for_fluid(rt, a(0)).unwrap_or(V::NULL),
            "currentLevel" => {
                let state = rt.str_of(a(1)).unwrap_or("").to_owned();
                V::I(current_level(rt, a(0), &state))
            }
            _ => return None,
        }),
        REGISTER if name == "register" => {
            register(rt, a(1), a(2), a(3).i(), a(4));
            Some(V::Void)
        }
        FLUID_UTIL => util_call(rt, name, desc, args),
        // A wrapper's methods called through the handlers' interfaces.
        HANDLER | ITEM_HANDLER if !args.is_empty() && rt.instance_of(a(0), BUCKET_WRAPPER) => bucket_call(rt, name, desc, args),
        HANDLER | ITEM_HANDLER if !args.is_empty() && rt.instance_of(a(0), CAULDRON_WRAPPER) => cauldron_call(rt, name, desc, args),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn rt() -> Rt {
        crate::install();
        Rt::new()
    }

    const CONTAINER: &str = "net/neoforged/neoforge/fluids/capability/IFluidHandlerItem.getContainer.()Lnet/minecraft/world/item/ItemStack;";
    const TANK_INIT: &str = "net/neoforged/neoforge/fluids/capability/templates/FluidTank.<init>.(I)V";
    const TANK_AMOUNT: &str = "net/neoforged/neoforge/fluids/capability/templates/FluidTank.getFluidAmount.()I";

    fn water(rt: &mut Rt, amount: i32) -> V {
        let w = fluids::fluid(rt, "minecraft:water");
        fluids::fluid_stack(rt, w, amount)
    }

    fn item_of(rt: &Rt, stack: V) -> String {
        crate::api_common::item_like_name(rt, rt.get_field(stack, "$item"))
    }

    fn fluid_of(rt: &Rt, s: V) -> (String, i32) {
        (fluids::name_of(rt, rt.get_field(s, "$fluid")), fluids::stack_amount(rt, s))
    }

    /// A bucket's handler, as Create's spout and item drain use it: an empty bucket takes a whole
    /// bucketful and becomes a water bucket, a water bucket gives all of it back and becomes an
    /// empty one. A fish or milk bucket gets no handler; two buckets in one stack take nothing.
    #[test]
    fn a_bucket_takes_a_bucketful_and_gives_it_back() {
        let mut rt = rt();
        let (execute, simulate) = (tanks::action(&mut rt, false), tanks::action(&mut rt, true));
        let cap = api::get_static(&mut rt, "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.ITEM");
        let empty = crate::api_common::stack(&mut rt, "minecraft:bucket", 1);
        let h = api::call(&mut rt, ITEM_CAPABILITY, &[empty, cap]);
        assert_eq!(rt.class_of(h), BUCKET_WRAPPER, "GenericItemFilling checks the exact class");
        assert!(rt.instance_of(h, ITEM_HANDLER));

        let w = water(&mut rt, 999);
        assert_eq!(api::call(&mut rt, FILL.0, &[h, w, execute]).i(), 0, "less than a bucketful");
        let w = water(&mut rt, 1500);
        assert_eq!(api::call(&mut rt, FILL.0, &[h, w, simulate]).i(), 1000);
        let c = api::call(&mut rt, CONTAINER, &[h]);
        assert_eq!(item_of(&rt, c), "minecraft:bucket", "simulating changes nothing");
        assert_eq!(api::call(&mut rt, FILL.0, &[h, w, execute]).i(), 1000);
        let c = api::call(&mut rt, CONTAINER, &[h]);
        assert_eq!(item_of(&rt, c), "minecraft:water_bucket");
        let w = water(&mut rt, 1000);
        assert_eq!(api::call(&mut rt, FILL.0, &[h, w, execute]).i(), 0, "a full bucket takes no more");

        let full = crate::api_common::stack(&mut rt, "minecraft:water_bucket", 1);
        let h = api::call(&mut rt, ITEM_CAPABILITY, &[full, cap]);
        let out = api::call(&mut rt, DRAIN.0, &[h, V::I(999), execute]);
        assert!(fluids::stack_empty(&rt, out), "all of it or nothing");
        let out = api::call(&mut rt, DRAIN.0, &[h, V::I(1000), execute]);
        assert_eq!(fluid_of(&rt, out), ("minecraft:water".to_owned(), 1000));
        let c = api::call(&mut rt, CONTAINER, &[h]);
        assert_eq!(item_of(&rt, c), "minecraft:bucket");

        for item in ["minecraft:cod_bucket", "minecraft:milk_bucket", "minecraft:stone"] {
            let s = crate::api_common::stack(&mut rt, item, 1);
            assert_eq!(api::call(&mut rt, ITEM_CAPABILITY, &[s, cap]), V::NULL, "{item}");
        }
        let two = crate::api_common::stack(&mut rt, "minecraft:bucket", 2);
        let h = api::call(&mut rt, ITEM_CAPABILITY, &[two, cap]);
        let w = water(&mut rt, 1000);
        assert_eq!(api::call(&mut rt, FILL.0, &[h, w, execute]).i(), 0);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A cauldron's handler, as NeoForge's `CauldronWrapper`: a water cauldron's three levels hold
    /// a bucketful that isn't a multiple of three, so water goes in from empty to full and out from
    /// full to empty, 1000 mB at a time; a cauldron two thirds full shows 666 mB and gives nothing.
    /// Lava doesn't go into water, and a block that is no cauldron any more throws.
    #[test]
    fn a_cauldron_fills_and_drains_a_bucketful_at_a_time() {
        let mut rt = rt();
        let (execute, simulate) = (tanks::action(&mut rt, false), tanks::action(&mut rt, true));
        let cap = api::get_static(&mut rt, "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.BLOCK");
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let pos = api::block_pos(&mut rt, 0, 64, 0);
        let in_tank = "net/neoforged/neoforge/fluids/capability/IFluidHandler.getFluidInTank.(I)Lnet/neoforged/neoforge/fluids/FluidStack;";
        let mut host = MemoryHost::default();
        host.blocks.insert([0, 64, 0], "minecraft:cauldron".into());
        rt.with_host(&mut host, |rt| {
            let here = |rt: &mut Rt| {
                let s = api::call(rt, GET_STATE, &[level, pos]);
                rt.str_of(s).unwrap_or("").to_owned()
            };
            let h = api::call(rt, BLOCK_CAPABILITY, &[level, cap, pos, V::NULL]);
            assert_eq!(rt.class_of(h), CAULDRON_WRAPPER);
            let w = water(rt, 999);
            assert_eq!(api::call(rt, FILL.0, &[h, w, execute]).i(), 0);
            let w = water(rt, 2500);
            assert_eq!(api::call(rt, FILL.0, &[h, w, simulate]).i(), 1000);
            assert_eq!(here(rt), "minecraft:cauldron");
            assert_eq!(api::call(rt, FILL.0, &[h, w, execute]).i(), 1000);
            assert_eq!(here(rt), "minecraft:water_cauldron[level=3]");
            let lava = fluids::fluid(rt, "minecraft:lava");
            let l = fluids::fluid_stack(rt, lava, 1000);
            assert_eq!(api::call(rt, FILL.0, &[h, l, execute]).i(), 0, "lava doesn't go into water");
            let out = api::call(rt, DRAIN.0, &[h, V::I(1000), execute]);
            assert_eq!(fluid_of(rt, out), ("minecraft:water".to_owned(), 1000));
            assert_eq!(here(rt), "minecraft:cauldron");

            let s = api::state(rt, "minecraft:water_cauldron[level=2]");
            api::call(rt, SET_STATE, &[level, pos, s]);
            let held = api::call(rt, in_tank, &[h, V::I(0)]);
            assert_eq!(fluid_of(rt, held), ("minecraft:water".to_owned(), 666));
            let out = api::call(rt, DRAIN.0, &[h, V::I(1000), execute]);
            assert!(fluids::stack_empty(rt, out));
            assert_eq!(here(rt), "minecraft:water_cauldron[level=2]");

            let s = api::state(rt, "minecraft:stone");
            api::call(rt, SET_STATE, &[level, pos, s]);
            api::call(rt, in_tank, &[h, V::I(0)]);
            let thrown = rt.thrown.take().expect("a cauldron that is gone throws");
            assert_eq!(rt.class_of(thrown), "java/lang/IllegalStateException");
            let stone = api::call(rt, BLOCK_CAPABILITY, &[level, cap, pos, V::NULL]);
            assert_eq!(stone, V::NULL);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `CauldronFluidContent`: vanilla's three cauldrons by block and by fluid, and a second
    /// registration of a fluid refused as NeoForge refuses it.
    #[test]
    fn cauldron_contents_are_found_by_block_and_by_fluid() {
        let mut rt = rt();
        let for_fluid = "net/neoforged/neoforge/fluids/CauldronFluidContent.getForFluid.(Lnet/minecraft/world/level/material/Fluid;)Lnet/neoforged/neoforge/fluids/CauldronFluidContent;";
        let for_block = "net/neoforged/neoforge/fluids/CauldronFluidContent.getForBlock.(Lnet/minecraft/world/level/block/Block;)Lnet/neoforged/neoforge/fluids/CauldronFluidContent;";
        let w = fluids::fluid(&mut rt, "minecraft:water");
        let c = api::call(&mut rt, for_fluid, &[w]);
        assert_eq!(api::block_name(&rt, rt.get_field(c, "block")), "minecraft:water_cauldron");
        assert_eq!((rt.get_field(c, "totalAmount"), rt.get_field(c, "maxLevel")), (V::I(1000), V::I(3)));
        let lava = api::block(&mut rt, "minecraft:lava_cauldron");
        let c = api::call(&mut rt, for_block, &[lava]);
        assert_eq!(fluids::name_of(&rt, rt.get_field(c, "fluid")), "minecraft:lava");
        assert_eq!(rt.get_field(c, "maxLevel"), V::I(1));
        let flowing = fluids::fluid(&mut rt, "minecraft:flowing_water");
        assert_eq!(api::call(&mut rt, for_fluid, &[flowing]), V::NULL);

        let ev = rt.new_object(REGISTER);
        let again = api::block(&mut rt, "minecraft:cauldron");
        let register = "net/neoforged/neoforge/fluids/RegisterCauldronFluidContentEvent.register.(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/world/level/material/Fluid;ILnet/minecraft/world/level/block/state/properties/IntegerProperty;)V";
        api::call(&mut rt, register, &[ev, again, w, V::I(1000), V::NULL]);
        let thrown = rt.thrown.take().expect("a duplicate throws");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `FluidUtil`: what a water bucket holds, and a bucketful moved from it into a mod's tank, the
    /// bucket left empty.
    #[test]
    fn fluid_util_finds_and_moves_a_buckets_fluid() {
        let mut rt = rt();
        let full = crate::api_common::stack(&mut rt, "minecraft:water_bucket", 1);
        let contained = api::call(&mut rt, "net/neoforged/neoforge/fluids/FluidUtil.getFluidContained.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/Optional;", &[full]);
        let held = rt.get_field(contained, "value");
        assert_eq!(fluid_of(&rt, held), ("minecraft:water".to_owned(), 1000));
        let empty = crate::api_common::stack(&mut rt, "minecraft:bucket", 1);
        let none = api::call(&mut rt, "net/neoforged/neoforge/fluids/FluidUtil.getFluidContained.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/Optional;", &[empty]);
        assert_eq!(rt.get_field(none, "value"), V::NULL);

        let handler = api::call(&mut rt, "net/neoforged/neoforge/fluids/FluidUtil.getFluidHandler.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/Optional;", &[full]);
        let bucket = rt.get_field(handler, "value");
        let tank = rt.new_object("net/neoforged/neoforge/fluids/capability/templates/FluidTank");
        api::call(&mut rt, TANK_INIT, &[tank, V::I(4000)]);
        let moved = api::call(&mut rt, "net/neoforged/neoforge/fluids/FluidUtil.tryFluidTransfer.(Lnet/neoforged/neoforge/fluids/capability/IFluidHandler;Lnet/neoforged/neoforge/fluids/capability/IFluidHandler;IZ)Lnet/neoforged/neoforge/fluids/FluidStack;", &[tank, bucket, V::I(i32::MAX), V::I(1)]);
        assert_eq!(fluid_of(&rt, moved), ("minecraft:water".to_owned(), 1000));
        assert_eq!(api::call(&mut rt, TANK_AMOUNT, &[tank]).i(), 1000);
        let c = api::call(&mut rt, CONTAINER, &[bucket]);
        assert_eq!(item_of(&rt, c), "minecraft:bucket");
        let filled = api::call(&mut rt, "net/neoforged/neoforge/fluids/FluidUtil.getFilledBucket.(Lnet/neoforged/neoforge/fluids/FluidStack;)Lnet/minecraft/world/item/ItemStack;", &[moved]);
        assert_eq!(item_of(&rt, filled), "minecraft:water_bucket");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
