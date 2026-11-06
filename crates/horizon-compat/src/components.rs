//! Item data components, natively. This covers the component types a mod registers
//! (`DataComponentType.builder().persistent(codec).build()`, NeoForge's
//! `DeferredRegister.DataComponents` too), vanilla's `DataComponents`, and a stack's own components:
//! `has/get/getOrDefault/set/remove/update`, including NeoForge's forms that take the type's holder.
//!
//! A stack keeps what was set on it (vanilla's component patch) in `$patch`, as (type, value)
//! pairs, and its copies keep them too. The game's stacks carry the mod's persistent types (those
//! built with a codec) as native components: [`persistent_types`] names them for the game to
//! register, [`load`] reads a game stack's into a stack here and [`saved`] writes them back, each
//! value through its type's codec (`encodeStart(NbtOps.INSTANCE, value)` and `parse`). A saved
//! stack (`ItemStack.save`, `ItemStack.CODEC`: what a block entity keeps) carries them as vanilla's
//! `components` map. Of vanilla's own types a game stack carries its enchantments (and a book's
//! stored ones), its potion contents ([`crate::food`]) and its custom data (`CustomData`, modelled
//! here) here and back; of an item's default components (vanilla's prototype) only `enchantments`
//! and a potion's empty `potion_contents` are modelled (`ItemEnchantments.EMPTY`:
//! `get(MAX_STACK_SIZE)` is null).
//!
//! Component maps: a stack's as a live view (`ItemStack.getComponents()`), maps built by
//! `DataComponentMap.builder()` and patches by `DataComponentPatch.builder()`, which
//! `ItemStack.applyComponents` puts on a stack.
//!
//! NeoForge's `FluidStack` holds components too (`MutableDataComponentHolder`: Create's potion
//! fluid carries the potion's contents): kept as an item stack's are, read and set through the
//! same calls, with no item defaults.
use crate::nbt::Nbt;
use crate::{Data, Rt, V};
use std::collections::BTreeMap;

pub(crate) const TYPE: &str = "net/minecraft/core/component/DataComponentType";
const BUILDER: &str = "net/minecraft/core/component/DataComponentType$Builder";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const FLUID_STACK: &str = "net/neoforged/neoforge/fluids/FluidStack";
const PATCH: &str = "compat:components";
/// Interfaces a stack's component methods are called through (`DataComponentHolder.has`,
/// NeoForge's `MutableDataComponentHolder.set`...): answered when the receiver is a stack.
const HOLDERS: [&str; 4] = [
    "net/minecraft/core/component/DataComponentHolder",
    "net/neoforged/neoforge/common/MutableDataComponentHolder",
    "net/neoforged/neoforge/common/extensions/IDataComponentHolderExtension",
    "net/neoforged/neoforge/common/extensions/IItemStackExtension",
];
/// Vanilla's types a game stack carries to the runtime and back: its enchantments.
const CARRIED_VANILLA: [&str; 2] = ["minecraft:enchantments", "minecraft:stored_enchantments"];
/// `DataComponents` statics that aren't component types.
const NOT_TYPES: [&str; 2] = ["COMMON_ITEM_COMPONENTS", "ITEM_STACK_CODEC"];

/// `DataComponentMap`: a stack's components (`$stack`: `ItemStack.getComponents()`, a live view)
/// or a map built here (`builder()`, `EMPTY`, `filter`, `composite`), whose (type, value) pairs
/// are kept as a stack keeps its own (`$patch`).
const MAP: &str = "net/minecraft/core/component/DataComponentMap";
const MAP_BUILDER: &str = "net/minecraft/core/component/DataComponentMap$Builder";
const TYPED: &str = "net/minecraft/core/component/TypedDataComponent";
/// A patch: from a saved stack or a recipe (`$json`, [`crate::recipes::component_patch`]) or built
/// by its builder (`$patch` pairs set, `$removed` types removed).
const COMPONENT_PATCH: &str = "net/minecraft/core/component/DataComponentPatch";
const PATCH_BUILDER: &str = "net/minecraft/core/component/DataComponentPatch$Builder";
/// The stack's own calls a map's reads and an applied map go through: the damage and the
/// defaults answer as the stack's do.
const GET_DESC: &str = "(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
const SET_DESC: &str = "(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
const PREDICATE_TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";

/// What a block entity reads of the stack it is placed from: `$map` (the stack's components), each
/// type it reads kept in `$used`.
const INPUT: &str = "net/minecraft/world/level/block/entity/BlockEntity$DataComponentInput";
const APPLY_IMPLICIT: &str = "applyImplicitComponents.(Lnet/minecraft/world/level/block/entity/BlockEntity$DataComponentInput;)V";
const APPLY_IMPLICIT_KEY: &str =
    "net/minecraft/world/level/block/entity/BlockEntity.applyImplicitComponents.(Lnet/minecraft/world/level/block/entity/BlockEntity$DataComponentInput;)V";
const COLLECT_IMPLICIT: &str = "collectImplicitComponents.(Lnet/minecraft/core/component/DataComponentMap$Builder;)V";
const COLLECT_IMPLICIT_KEY: &str = "net/minecraft/world/level/block/entity/BlockEntity.collectImplicitComponents.(Lnet/minecraft/core/component/DataComponentMap$Builder;)V";
const MAP_DESC: &str = "()Lnet/minecraft/core/component/DataComponentMap;";

/// The registry a mod's component types are registered in (`Registries.DATA_COMPONENT_TYPE`).
const REGISTRY: &str = "data_component_type";

/// A component type's codec, when it persists (`DataComponentType.Builder.persistent`).
fn codec_of(rt: &Rt, ty: V) -> Option<V> {
    match rt.get_field(ty, "$codec") {
        V::R(r) if r != 0 => Some(V::R(r)),
        _ => None,
    }
}

/// The mod's component types that persist (built with a codec), by name, sorted: the game registers
/// them as native components, so its stacks can carry their values. Only namespaced names outside
/// `minecraft` that a resource location allows (the game refuses to start on any other).
pub fn persistent_types(rt: &mut Rt) -> Vec<String> {
    crate::neo::resolve(rt);
    let mut names: Vec<String> = rt
        .registered
        .iter()
        .filter(|((r, n), v)| {
            r == REGISTRY
                && n.contains(':')
                && !n.starts_with("minecraft:")
                && crate::recipes::valid_location(n)
                && rt.class_of(**v) == TYPE
                && codec_of(rt, **v).is_some()
        })
        .map(|((_, n), _)| n.clone())
        .collect();
    names.sort();
    names
}

/// Puts components a game stack carries on `stack`: (type name, value as NBT), each read with its
/// type's codec. One the mod didn't register, or whose value its codec refuses, is left out (and told
/// once).
pub(crate) fn load(rt: &mut Rt, stack: V, carried: &[(String, Nbt)]) {
    for (name, value) in carried {
        match value {
            // Custom data as NBT: its tags' kinds stay.
            Nbt::Compound(tag) if name == CUSTOM_DATA_ID => load_custom_data(rt, stack, tag),
            _ => load_one(rt, stack, name, &crate::recipes::nbt_json(value)),
        }
    }
}

/// [`load`] from a saved stack's `components` (vanilla's patch, as `ItemStack.CODEC` writes it):
/// the mod's own types, the damage and the custom data; a removal (`!name`) and vanilla's other
/// types are left out.
pub(crate) fn load_saved(rt: &mut Rt, stack: V, patch: &serde_json::Map<String, serde_json::Value>) {
    for (name, value) in patch {
        if name == "minecraft:damage" {
            set_damage(rt, stack, value.as_i64().unwrap_or(0) as i32);
        } else if !name.starts_with('!') {
            load_one(rt, stack, name, value);
        }
    }
}

/// [`load_saved`] from NBT (`ItemStack.parse`): the custom data keeps its tags' kinds.
pub(crate) fn load_saved_nbt(rt: &mut Rt, stack: V, patch: &BTreeMap<String, Nbt>) {
    let mut rest = serde_json::Map::new();
    for (name, value) in patch {
        match value {
            Nbt::Compound(tag) if name == CUSTOM_DATA_ID => load_custom_data(rt, stack, tag),
            _ => {
                rest.insert(name.clone(), crate::recipes::nbt_json(value));
            }
        }
    }
    load_saved(rt, stack, &rest);
}

/// `stack`'s patch as `ItemStack.CODEC` writes it: [`saved`] and its damage, when it has some.
pub(crate) fn patch(rt: &mut Rt, stack: V) -> BTreeMap<String, Nbt> {
    let mut out: BTreeMap<String, Nbt> = saved(rt, stack).into_iter().collect();
    let n = damage(rt, stack);
    if n > 0 {
        out.insert("minecraft:damage".into(), Nbt::Int(n));
    }
    out
}

fn load_one(rt: &mut Rt, stack: V, name: &str, value: &serde_json::Value) {
    if name == crate::food::POTION_CONTENTS_ID {
        let contents = crate::food::decode_potion_contents(rt, value);
        set_vanilla(rt, stack, name, contents);
        return;
    }
    // Custom data as JSON (a saved stack's), through `CustomData.CODEC`.
    if name == CUSTOM_DATA_ID {
        let codec = crate::codecs::get_static(rt, CUSTOM_DATA, "CODEC").unwrap_or(V::NULL);
        match crate::codecs::decode(rt, codec, value) {
            Ok(v) => {
                set_vanilla(rt, stack, name, v);
            }
            Err(e) => crate::glue::trace_once(rt, &format!("component-read:{name}"), |_| format!("component {name} not read: {e}")),
        }
        return;
    }
    if CARRIED_VANILLA.contains(&name) {
        match crate::enchantments::decode(rt, value) {
            Ok(v) => {
                set_vanilla(rt, stack, name, v);
            }
            Err(e) => crate::glue::trace_once(rt, &format!("component-read:{name}"), |_| format!("component {name} not read: {e}")),
        }
        return;
    }
    let Some(&ty) = rt.registered.get(&(REGISTRY.to_owned(), name.to_owned())) else { return };
    let Some(codec) = codec_of(rt, ty) else { return };
    match crate::codecs::decode(rt, codec, value) {
        Ok(v) => {
            put(rt, stack, ty, v);
        }
        Err(e) => crate::glue::trace_once(rt, &format!("component-read:{name}"), |_| format!("component {name} not read: {e}")),
    }
}

/// `stack`'s components a game stack can carry, by type name (sorted), each written with its type's
/// codec: the mod's, vanilla's enchantments ([`CARRIED_VANILLA`], none when there are no levels)
/// and its custom data (its tag, none when empty). Vanilla's other types, types without a codec
/// and values the codec can't write are left out (the last told once).
pub(crate) fn saved(rt: &mut Rt, stack: V) -> Vec<(String, Nbt)> {
    let mut out = Vec::new();
    for p in pairs(rt, stack).chunks(2) {
        let (ty, value) = (p[0], p[1]);
        let id = rt.str_of(rt.get_field(ty, "$id")).unwrap_or("").to_owned();
        if id == CUSTOM_DATA_ID {
            let tag = custom_nbt(rt, value);
            if !tag.is_empty() {
                out.push((id, Nbt::Compound(tag)));
            }
            continue;
        }
        if id == crate::food::POTION_CONTENTS_ID {
            out.extend(crate::food::encode_potion_contents(rt, value).map(|n| (id, n)));
            continue;
        }
        if CARRIED_VANILLA.contains(&id.as_str()) {
            if !crate::enchantments::levels(rt, value).is_empty() {
                match crate::enchantments::encode(rt, value) {
                    Ok(n) => out.push((id, n)),
                    Err(e) => crate::glue::trace_once(rt, &format!("component-write:{id}"), |_| format!("component {id} not written: {e}")),
                }
            }
            continue;
        }
        let Some(name) = rt.registered_name(ty, REGISTRY).map(str::to_owned) else { continue };
        let Some(codec) = codec_of(rt, ty) else { continue };
        match crate::codecs::encode(rt, codec, value) {
            Ok(n) => out.push((name, n)),
            Err(e) => crate::glue::trace_once(rt, &format!("component-write:{name}"), |_| format!("component {name} not written: {e}")),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A stack's damage (`minecraft:damage`), 0 for none.
pub(crate) fn damage(rt: &Rt, stack: V) -> i32 {
    if stack.r() == 0 {
        return 0;
    }
    rt.get_field(stack, "$damage").i()
}

/// The damage at which `stack` breaks (`minecraft:max_damage`): the game's word for a stack from a
/// hand, else its mod item's `durability`, else the game's for a vanilla item (an iron sword's
/// 250); 0 for none.
pub(crate) fn max_damage(rt: &Rt, stack: V) -> i32 {
    if stack.r() == 0 {
        return 0;
    }
    if let V::I(n) = rt.get_field(stack, "$maxDamage") {
        return n;
    }
    let item = rt.get_field(stack, "$item");
    let props = if item.r() != 0 { rt.get_field(item, "$properties") } else { V::NULL };
    if props.r() != 0 {
        return rt.get_field(props, "$durability").i();
    }
    let name = crate::api_common::item_like_name(rt, item);
    rt.host().and_then(|h| h.item_max_damage(&name)).map_or(0, |n| i32::try_from(n).unwrap_or(i32::MAX))
}

/// The small bar under an item: `Item.isBarVisible`, `getBarWidth` and `getBarColor`, each taking
/// the stack (their keys on `Item`, and as a mod's class overrides them). 75 classes in 25 jars of
/// the Create pack override `isBarVisible` (Create's sequenced assembly shows its progress there,
/// its backtank its air).
pub(crate) const BAR: [(&str, &str, &str); 3] = [
    ("isBarVisible", "net/minecraft/world/item/Item.isBarVisible.(Lnet/minecraft/world/item/ItemStack;)Z", "isBarVisible.(Lnet/minecraft/world/item/ItemStack;)Z"),
    ("getBarWidth", "net/minecraft/world/item/Item.getBarWidth.(Lnet/minecraft/world/item/ItemStack;)I", "getBarWidth.(Lnet/minecraft/world/item/ItemStack;)I"),
    ("getBarColor", "net/minecraft/world/item/Item.getBarColor.(Lnet/minecraft/world/item/ItemStack;)I", "getBarColor.(Lnet/minecraft/world/item/ItemStack;)I"),
];

/// `item.isBarVisible(stack)` (`getBarWidth`, `getBarColor`: `name`), as `ItemStack`'s asks it,
/// virtually: a mod item's override answers, else NeoForge's `Item`'s ([`default_bar`], here
/// again).
fn bar_virtually(rt: &mut Rt, name: &str, item: V, stack: V) -> V {
    let Some(&(_, owner_key, key)) = BAR.iter().find(|(n, ..)| *n == name) else {
        return V::I(0);
    };
    match rt.vcall {
        Some(vcall) if item.r() != 0 => vcall(rt, owner_key, key, &[item, stack]),
        _ => default_bar(rt, name, stack),
    }
}

/// NeoForge's `Item.isBarVisible` (the stack is damaged), `getBarWidth` (13 pixels whole:
/// `Math.round(13 - damage * 13 / maxDamage)`) and `getBarColor` (`Mth.hsvToRgb` of a third of
/// the durability left: green whole, red spent), by `name`.
pub(crate) fn default_bar(rt: &mut Rt, name: &str, stack: V) -> V {
    let max = max_damage(rt, stack);
    let damage = damage(rt, stack).clamp(0, max.max(0));
    match name {
        "isBarVisible" => V::I((max > 0 && damage > 0) as i32),
        "getBarWidth" => V::I(crate::jlang::round_f32(13.0 - damage as f32 * 13.0 / max as f32)),
        _ => {
            let max = max as f32;
            let left = ((max - damage as f32) / max).max(0.0);
            V::I(crate::api_common::hsv_to_argb(left / 3.0, 1.0, 1.0, 0).unwrap_or(0))
        }
    }
}

/// Whether registered item `name` (a mod's) has a bar of its own: its class overrides
/// `isBarVisible`, `getBarWidth` or `getBarColor`.
pub(crate) fn own_bar(rt: &Rt, name: &str) -> bool {
    let Some(overrides) = rt.overrides else {
        return false;
    };
    let Some(&item) = rt.registered.get(&("item".to_owned(), name.to_owned())) else {
        return false;
    };
    rt.str_of(item).is_none() && BAR.iter().any(|(_, _, key)| overrides(rt.class_of(item), key))
}

/// The bar the game draws under a mod's item ([`crate::glue::item_bar`]), as
/// `GuiGraphics.renderItemDecorations` asks it: none unless `isBarVisible`, else `getBarWidth`
/// pixels (of 13) in `getBarColor`. The item's own methods answer where its class overrides them,
/// the game's bar (`given`: the durability, NeoForge's default) the rest.
pub(crate) fn held_bar(rt: &mut Rt, held: crate::host::HandStack, given: Option<(i32, u32)>) -> Option<(i32, u32)> {
    let item = rt.registered.get(&("item".to_owned(), held.item.clone())).copied()?;
    let (overrides, vcall) = (rt.overrides?, rt.vcall?);
    let stack = crate::hand::load(rt, Some(held));
    let ask = |rt: &mut Rt, i: usize| -> Option<i32> {
        let (name, owner_key, key) = BAR[i];
        if !overrides(rt.class_of(item), key) {
            return None;
        }
        let r = vcall(rt, owner_key, key, &[item, stack]).i();
        rt.report_thrown(name);
        Some(r)
    };
    if !ask(rt, 0).map_or(given.is_some(), |shown| shown != 0) {
        return None;
    }
    let width = match ask(rt, 1) {
        Some(w) => w,
        None => given.map_or_else(|| default_bar(rt, "getBarWidth", stack).i(), |(w, _)| w),
    };
    let colour = match ask(rt, 2) {
        Some(c) => c as u32,
        None => given.map_or_else(|| default_bar(rt, "getBarColor", stack).i() as u32, |(_, c)| c),
    };
    Some((width.clamp(0, 13), colour & 0xFF_FFFF))
}

fn set_damage(rt: &mut Rt, stack: V, n: i32) {
    let empty = crate::api_common::get_static(rt, ITEM_STACK, "EMPTY").unwrap_or(V::NULL);
    if stack.r() != 0 && stack != empty {
        rt.put_field(stack, "$damage", V::I(n.max(0)));
    }
}

/// Vanilla's damage components, read from the stack's damage: `minecraft:damage` (present on what
/// can take damage, or took some) and `minecraft:max_damage`.
fn damage_component(rt: &mut Rt, name: &str, stack: V, ty: V, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let id = rt.str_of(rt.get_field(ty, "$id"))?.to_owned();
    let boxed = crate::api_common::integer;
    Some(match id.as_str() {
        "minecraft:damage" => {
            let present = max_damage(rt, stack) > 0 || damage(rt, stack) > 0;
            let now = damage(rt, stack);
            match name {
                "has" => V::I(present as i32),
                "get" | "getOrDefault" if present => boxed(rt, now),
                "get" => V::NULL,
                "getOrDefault" => a(2),
                "set" | "remove" => {
                    let next = if name == "set" { rt.get_field(a(2), "value").i() } else { 0 };
                    set_damage(rt, stack, next);
                    if present { boxed(rt, now) } else { V::NULL }
                }
                _ => return None,
            }
        }
        "minecraft:max_damage" => {
            let max = max_damage(rt, stack);
            match name {
                "has" => V::I((max > 0) as i32),
                "get" | "getOrDefault" if max > 0 => boxed(rt, max),
                "get" => V::NULL,
                "getOrDefault" => a(2),
                "set" | "remove" => {
                    let next = if name == "set" { rt.get_field(a(2), "value").i() } else { 0 };
                    rt.put_field(stack, "$maxDamage", V::I(next.max(0)));
                    if max > 0 { boxed(rt, max) } else { V::NULL }
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Vanilla's component type `id` (`minecraft:damage`), one object per name, as vanilla's are.
pub fn vanilla_type(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:component/{id}"), |rt| {
        let t = rt.new_object(TYPE);
        let s = rt.string(id);
        rt.put_field(t, "$id", s);
        t
    })
}

/// What `stack` holds of vanilla's type `id` (`minecraft:enchantments`): what was set on it, else
/// the item's default ([`current`]).
pub(crate) fn vanilla(rt: &mut Rt, stack: V, id: &str) -> Option<V> {
    let ty = vanilla_type(rt, id);
    current(rt, stack, ty)
}

/// Sets vanilla's type `id` on `stack` (null removes it); the value it had set before, or null.
pub(crate) fn set_vanilla(rt: &mut Rt, stack: V, id: &str, value: V) -> V {
    let ty = vanilla_type(rt, id);
    put(rt, stack, ty, value)
}

/// What `stack` holds of type `ty`: what was set on it, else the item's default where this runtime
/// knows one. Every 1.21.1 item carries `enchantments` (`ItemEnchantments.EMPTY`, one of
/// `DataComponents.COMMON_ITEM_COMPONENTS`), an enchanted book `stored_enchantments` too. A
/// removed one reads as its default here, not as null.
fn current(rt: &mut Rt, stack: V, ty: V) -> Option<V> {
    if let Some(v) = get(rt, stack, ty) {
        return Some(v);
    }
    // A fluid stack has no defaults: only what was set on it.
    if rt.class_of(stack) != ITEM_STACK {
        return None;
    }
    let id = rt.str_of(rt.get_field(ty, "$id")).unwrap_or("").to_owned();
    let item = crate::api_common::item_like_name(rt, rt.get_field(stack, "$item"));
    let defaulted = match id.as_str() {
        "minecraft:enchantments" => true,
        "minecraft:stored_enchantments" => item == "minecraft:enchanted_book",
        // Vanilla's potions and tipped arrows hold `PotionContents.EMPTY`.
        crate::food::POTION_CONTENTS_ID => matches!(item.as_str(), "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion" | "minecraft:tipped_arrow"),
        _ => false,
    };
    if !defaulted || stack.r() == 0 || crate::api_common::stack_empty(rt, stack) {
        return None;
    }
    Some(if id == crate::food::POTION_CONTENTS_ID { crate::api::get_static(rt, "net/minecraft/world/item/alchemy/PotionContents.EMPTY") } else { crate::enchantments::empty(rt) })
}

/// A component type built from a builder the mod configured: its codec is kept.
pub fn build(rt: &mut Rt, builder: V) -> V {
    let t = rt.new_object(TYPE);
    let codec = match rt.get_field(builder, "$codec") {
        V::Void => V::NULL,
        c => c,
    };
    rt.put_field(t, "$codec", codec);
    t
}

/// NeoForge's `registerComponentType(name, builder -> ...)`: a builder given to the mod's
/// operator, then built.
pub fn build_with(rt: &mut Rt, operator: V) -> V {
    let b = rt.new_object(BUILDER);
    let configured = rt.call_functional("java/util/function/UnaryOperator.apply.(Ljava/lang/Object;)Ljava/lang/Object;", operator, &[b]);
    build(rt, if configured.r() == 0 { b } else { configured })
}

/// The component type `v` names: the type itself, or a holder of it.
fn type_of(rt: &mut Rt, v: V) -> V {
    if v.r() == 0 || rt.class_of(v) == TYPE {
        return v;
    }
    rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", v, &[])
}

/// Whether `v` holds components of its own: an item stack or a fluid stack.
fn holder(rt: &Rt, v: V) -> bool {
    v.r() != 0 && matches!(rt.class_of(v), ITEM_STACK | FLUID_STACK)
}

/// The stack's components, as (type, value) pairs in the order they were set.
fn pairs(rt: &Rt, stack: V) -> Vec<V> {
    let p = rt.get_field(stack, "$patch");
    match rt.heap.get(p.r() as usize).map(|o| &o.data) {
        Some(Data::List(l)) if p.r() != 0 => l.clone(),
        _ => Vec::new(),
    }
}

fn store(rt: &mut Rt, stack: V, l: Vec<V>) {
    let p = rt.get_field(stack, "$patch");
    if p.r() != 0
        && let Some(o) = rt.heap.get_mut(p.r() as usize)
    {
        o.data = Data::List(l);
        return;
    }
    let p = rt.alloc(PATCH, Data::List(l));
    rt.put_field(stack, "$patch", p);
}

fn get(rt: &Rt, stack: V, ty: V) -> Option<V> {
    let l = pairs(rt, stack);
    l.chunks(2).find(|p| p[0] == ty).map(|p| p[1])
}

/// Sets (or, for null, removes) the stack's component; the value it had before, or null.
fn put(rt: &mut Rt, stack: V, ty: V, value: V) -> V {
    // `ItemStack.EMPTY` and `FluidStack.EMPTY` are shared objects: nothing is written onto them.
    let empty = crate::api_common::get_static(rt, ITEM_STACK, "EMPTY").unwrap_or(V::NULL);
    let no_fluid = rt.interned.get("compat:fluid-stack/empty").copied().unwrap_or(V::NULL);
    if stack.r() == 0 || stack == empty || (no_fluid.r() != 0 && stack == no_fluid) || ty.r() == 0 {
        return V::NULL;
    }
    let mut l = pairs(rt, stack);
    let at = l.chunks(2).position(|p| p[0] == ty).map(|i| i * 2);
    let removing = value.r() == 0;
    let before = match at {
        Some(i) if removing => l.drain(i..i + 2).nth(1).unwrap_or(V::NULL),
        Some(i) => std::mem::replace(&mut l[i + 1], value),
        None if removing => V::NULL,
        None => {
            l.extend([ty, value]);
            V::NULL
        }
    };
    store(rt, stack, l);
    before
}

/// A view of `stack`'s components (`ItemStack.getComponents()`): what it reads is the stack's as
/// it is then.
fn stack_view(rt: &mut Rt, stack: V) -> V {
    let m = rt.new_object(MAP);
    rt.put_field(m, "$stack", stack);
    m
}

/// A component map built here holding `pairs` (type, value, type, value...).
fn plain_map(rt: &mut Rt, pairs: Vec<V>) -> V {
    let m = rt.new_object(MAP);
    if !pairs.is_empty() {
        store(rt, m, pairs);
    }
    m
}

/// The stack a map is a view of, if it is one.
fn viewed(rt: &Rt, map: V) -> Option<V> {
    match rt.get_field(map, "$stack") {
        V::R(r) if r != 0 => Some(V::R(r)),
        _ => None,
    }
}

/// The (type, value) pairs a component map holds: a stack's are what was set on it, then its
/// damage and the defaults this runtime knows (`enchantments`, a book's `stored_enchantments`).
fn map_pairs(rt: &mut Rt, map: V) -> Vec<V> {
    let Some(stack) = viewed(rt, map) else { return pairs(rt, map) };
    let mut l = pairs(rt, stack);
    for id in ["minecraft:enchantments", "minecraft:stored_enchantments", "minecraft:damage", "minecraft:max_damage"] {
        let ty = vanilla_type(rt, id);
        if l.chunks(2).any(|p| p[0] == ty) {
            continue;
        }
        let v = call(rt, ITEM_STACK, "get", GET_DESC, &[stack, ty]).unwrap_or(V::NULL);
        if v.r() != 0 {
            l.extend([ty, v]);
        }
    }
    l
}

/// What `applyComponents(map)` sets of a map: of a stack's, what was set on the stack and its
/// damage (vanilla keeps a value equal to the item's default out of a stack's patch); of a built
/// one, its pairs.
fn set_pairs(rt: &mut Rt, map: V) -> Vec<V> {
    let Some(stack) = viewed(rt, map) else { return pairs(rt, map) };
    let mut l = pairs(rt, stack);
    let n = damage(rt, stack);
    if n > 0 {
        let ty = vanilla_type(rt, "minecraft:damage");
        let v = crate::api_common::integer(rt, n);
        l.extend([ty, v]);
    }
    l
}

/// The types a built patch removes.
fn removed(rt: &Rt, patch: V) -> Vec<V> {
    match rt.get_field(patch, "$removed") {
        V::R(r) if r != 0 => crate::jutil::elements(rt, V::R(r)),
        _ => Vec::new(),
    }
}

fn set_removed(rt: &mut Rt, patch: V, types: Vec<V>) {
    let l = crate::jutil::new_list(rt, types);
    rt.put_field(patch, "$removed", l);
}

/// Whether a patch built here sets and removes nothing ([`crate::recipes`] asks it of any patch).
pub(crate) fn built_patch_empty(rt: &Rt, patch: V) -> bool {
    pairs(rt, patch).is_empty() && removed(rt, patch).is_empty()
}

/// The patch's written form (`$json`: a saved stack's or a recipe's components), if it has one.
fn patch_json(rt: &Rt, patch: V) -> Option<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str::<serde_json::Value>(rt.str_of(rt.get_field(patch, "$json"))?).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

/// The component type named `name`: the mod's registered one, else vanilla's.
fn type_named(rt: &mut Rt, name: &str) -> V {
    match rt.registered.get(&(REGISTRY.to_owned(), name.to_owned())).copied() {
        Some(t) => t,
        None => vanilla_type(rt, name),
    }
}

/// What a patch holds, type by type: the value it sets, or None where it removes the type. A
/// built patch's and a stack's (`getComponentsPatch`, which keeps the stack's values as they are),
/// else its written form's read with the types' codecs (the mod's types, the damage).
fn patch_entries(rt: &mut Rt, patch: V) -> Vec<(V, Option<V>)> {
    let mut out: Vec<(V, Option<V>)> = pairs(rt, patch).chunks(2).map(|p| (p[0], Some(p[1]))).collect();
    let kept = !out.is_empty();
    for ty in removed(rt, patch) {
        out.push((ty, None));
    }
    for (name, value) in patch_json(rt, patch).unwrap_or_default() {
        if let Some(gone) = name.strip_prefix('!') {
            let ty = type_named(rt, gone);
            out.push((ty, None));
        } else if name == "minecraft:damage" {
            let ty = vanilla_type(rt, &name);
            let n = crate::api_common::integer(rt, value.as_i64().unwrap_or(0) as i32);
            out.push((ty, Some(n)));
        } else if !kept {
            // Read onto a scratch stack, as a stack would take it.
            let scratch = rt.new_object(ITEM_STACK);
            load_one(rt, scratch, &name, &value);
            out.extend(pairs(rt, scratch).chunks(2).map(|p| (p[0], Some(p[1]))));
        }
    }
    out
}

/// `patch.forget(predicate)`: the patch without the types the predicate takes, whether it sets or
/// removes them (`EMPTY` when nothing is left), as Create's backtank leaves out of its drops what its
/// block entity keeps of its own.
fn forget(rt: &mut Rt, patch: V, predicate: V) -> V {
    let (mut kept, mut gone) = (Vec::new(), Vec::new());
    for (ty, value) in patch_entries(rt, patch) {
        if rt.call_functional("java/util/function/Predicate.test.(Ljava/lang/Object;)Z", predicate, &[ty]).i() != 0 {
            continue;
        }
        match value {
            Some(v) => kept.extend([ty, v]),
            None => gone.push(ty),
        }
    }
    if kept.is_empty() && gone.is_empty() {
        return crate::recipes::get_static(rt, COMPONENT_PATCH, "EMPTY").unwrap_or(V::NULL);
    }
    let p = rt.new_object(COMPONENT_PATCH);
    if !kept.is_empty() {
        store(rt, p, kept);
    }
    set_removed(rt, p, gone);
    p
}

/// `stack.applyComponents(patch)`: what the patch sets is set on the stack, what it removes taken
/// off (an item stack or a fluid stack).
pub(crate) fn apply_patch(rt: &mut Rt, stack: V, patch: V) {
    for (ty, value) in patch_entries(rt, patch) {
        match value {
            Some(v) => call(rt, ITEM_STACK, "set", SET_DESC, &[stack, ty, v]),
            None => call(rt, ITEM_STACK, "remove", GET_DESC, &[stack, ty]),
        };
    }
}

/// Calls on a patch: its entries (`entrySet`: type to `Optional` value, empty where it removes),
/// what it holds of a type (`get`: null for nothing, else an `Optional`), its size.
fn patch_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let entries = patch_entries(rt, a(0));
    Some(match name {
        "size" => V::I(entries.len() as i32),
        "get" => {
            let ty = type_of(rt, a(1));
            match entries.iter().find(|e| e.0 == ty) {
                Some(&(_, v)) => crate::api_common::optional(rt, v.unwrap_or(V::NULL)),
                None => V::NULL,
            }
        }
        "entrySet" => {
            let mut list = Vec::new();
            for (ty, v) in entries {
                let o = crate::api_common::optional(rt, v.unwrap_or(V::NULL));
                list.push(crate::jutil::entry(rt, ty, o));
            }
            crate::jutil::identity_set(rt, &list)
        }
        _ => return None,
    })
}

/// Calls on a component map: its reads (a view's go through its stack), its types and size, its
/// components one by one (`TypedDataComponent`s), a filtered copy.
fn map_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let map = a(0);
    Some(match name {
        "get" | "getOrDefault" | "has" => {
            if let Some(stack) = viewed(rt, map) {
                let mut all = vec![stack];
                all.extend_from_slice(args.get(1..).unwrap_or_default());
                return call(rt, ITEM_STACK, name, desc, &all);
            }
            let ty = type_of(rt, a(1));
            let v = get(rt, map, ty);
            match name {
                "has" => V::I(v.is_some() as i32),
                "get" => v.unwrap_or(V::NULL),
                _ => v.unwrap_or(a(2)),
            }
        }
        "isEmpty" => V::I(map_pairs(rt, map).is_empty() as i32),
        "size" => V::I((map_pairs(rt, map).len() / 2) as i32),
        "keySet" => {
            let types: Vec<V> = map_pairs(rt, map).chunks(2).map(|p| p[0]).collect();
            crate::jutil::new_set_of(rt, &types)
        }
        "iterator" | "stream" => {
            let l = map_pairs(rt, map);
            let typed: Vec<V> = l.chunks(2).map(|p| crate::records::make(rt, TYPED, &[p[0], p[1]])).collect();
            if name == "iterator" { crate::jutil::new_iter(rt, typed) } else { rt.alloc("java/util/stream/Stream", Data::Stream(typed)) }
        }
        "getTyped" => {
            let ty = type_of(rt, a(1));
            let v = map_call(rt, "get", GET_DESC, &[map, ty])?;
            if v.r() == 0 { V::NULL } else { crate::records::make(rt, TYPED, &[ty, v]) }
        }
        // `filter(predicate)`: a map of the components whose type passes.
        "filter" => {
            let l = map_pairs(rt, map);
            let mut kept = Vec::new();
            for p in l.chunks(2) {
                if rt.call_functional(PREDICATE_TEST, a(1), &[p[0]]).i() != 0 {
                    kept.extend([p[0], p[1]]);
                }
            }
            plain_map(rt, kept)
        }
        _ => return None,
    })
}

/// `BlockEntity.applyComponentsFromItemStack(stack)`, as a block placed from `stack` does: the block
/// entity reads the stack's components (its own `applyImplicitComponents`), and keeps those set on
/// the stack that it didn't read, `block_entity_data` aside (`components()`).
pub(crate) fn apply_from_stack(rt: &mut Rt, be: V, stack: V) {
    if be.r() == 0 || stack.r() == 0 {
        return;
    }
    let input = rt.new_object(INPUT);
    let view = stack_view(rt, stack);
    rt.put_field(input, "$map", view);
    let used = crate::jutil::new_list(rt, Vec::new());
    rt.put_field(input, "$used", used);
    let class = rt.class_of(be);
    if rt.overrides.is_some_and(|o| o(class, APPLY_IMPLICIT))
        && let Some(vc) = rt.vcall
    {
        vc(rt, APPLY_IMPLICIT_KEY, APPLY_IMPLICIT, &[be, input]);
        rt.report_thrown("applyImplicitComponents");
    }
    let read = crate::jutil::elements(rt, used);
    let entity_data = vanilla_type(rt, "minecraft:block_entity_data");
    let kept: Vec<V> = set_pairs(rt, view).chunks(2).filter(|p| p[0] != entity_data && !read.contains(&p[0])).flat_map(|p| [p[0], p[1]]).collect();
    let m = plain_map(rt, kept);
    rt.put_field(be, "$components", m);
}

/// What block entity `be` kept of the stack it was placed from (`components()`), as vanilla saves
/// them under `components` (`BlockEntity.saveWithoutMetadata`): those a game stack can carry, each
/// written with its type's codec ([`saved`]).
pub(crate) fn kept_saved(rt: &mut Rt, be: V) -> Vec<(String, Nbt)> {
    let kept = rt.get_field(be, "$components");
    if kept.r() == 0 {
        return Vec::new();
    }
    saved(rt, kept)
}

/// Block entity `be`'s kept components back from what it saved under `components`
/// (`BlockEntity.loadWithComponents`), each read with its type's codec ([`load`]).
pub(crate) fn load_kept(rt: &mut Rt, be: V, carried: &[(String, Nbt)]) {
    let m = plain_map(rt, Vec::new());
    load(rt, m, carried);
    rt.put_field(be, "$components", m);
}

/// `BlockEntity.collectComponents()`: what it kept of its stack, then what it adds itself (its own
/// `collectImplicitComponents`), as the item it drops or is picked as carries them.
pub(crate) fn collect_components(rt: &mut Rt, be: V) -> V {
    let b = rt.new_object(MAP_BUILDER);
    let kept = rt.get_field(be, "$components");
    if kept.r() != 0 {
        let l = map_pairs(rt, kept);
        store(rt, b, l);
    }
    let class = rt.class_of(be);
    if rt.overrides.is_some_and(|o| o(class, COLLECT_IMPLICIT))
        && let Some(vc) = rt.vcall
    {
        vc(rt, COLLECT_IMPLICIT_KEY, COLLECT_IMPLICIT, &[be, b]);
        rt.report_thrown("collectImplicitComponents");
    }
    let l = pairs(rt, b);
    plain_map(rt, l)
}

/// `to` gets `from`'s components and damage (a copy of the stack keeps them).
pub(crate) fn copy_patch(rt: &mut Rt, from: V, to: V) {
    let l = pairs(rt, from);
    if !l.is_empty() {
        store(rt, to, l);
    }
    for field in ["$damage", "$maxDamage"] {
        if let n @ V::I(_) = rt.get_field(from, field) {
            rt.put_field(to, field, n);
        }
    }
}

/// Component values as `Objects.equals` compares them: strings and boxed primitives by value,
/// the mod's records by their own `equals`.
fn same_value(rt: &mut Rt, x: V, y: V) -> bool {
    if x == y {
        return true;
    }
    if x.r() == 0 || y.r() == 0 {
        return false;
    }
    if let (Some(a), Some(b)) = (rt.str_of(x), rt.str_of(y)) {
        return a == b;
    }
    let class = rt.class_of(x);
    if class.starts_with("java/lang/") && class == rt.class_of(y) && !matches!(rt.get_field(x, "value"), V::Void) {
        return rt.get_field(x, "value") == rt.get_field(y, "value");
    }
    crate::jutil::equals(rt, x, y)
}

/// Whether two stacks carry the same components (`isSameItemSameComponents`).
pub(crate) fn same_patch(rt: &mut Rt, a: V, b: V) -> bool {
    let (pa, pb) = (pairs(rt, a), pairs(rt, b));
    if pa.len() != pb.len() || damage(rt, a) != damage(rt, b) {
        return false;
    }
    for p in pa.chunks(2) {
        let Some(other) = pb.chunks(2).find(|q| q[0] == p[0]).map(|q| q[1]) else { return false };
        if !same_value(rt, p[1], other) {
            return false;
        }
    }
    true
}

// ---- Vanilla's custom data (`minecraft:custom_data`) ----

/// `CustomData`: a stack's free-form NBT (`minecraft:custom_data`; a block entity's and an
/// entity's too), as vanilla keeps it: its own `CompoundTag` in `tag`, which `getUnsafe` hands out
/// and everything else copies. A game stack carries it here and back ([`load`], [`saved`]).
const CUSTOM_DATA: &str = "net/minecraft/world/item/component/CustomData";
const CUSTOM_DATA_ID: &str = "minecraft:custom_data";
const TAG_COPY: &str = "net/minecraft/nbt/CompoundTag.copy.()Lnet/minecraft/nbt/CompoundTag;";
const TAG_SIZE: &str = "net/minecraft/nbt/CompoundTag.size.()I";
const TAG_CONTAINS: &str = "net/minecraft/nbt/CompoundTag.contains.(Ljava/lang/String;)Z";
const TAG_TO_STRING: &str = "net/minecraft/nbt/CompoundTag.toString.()Ljava/lang/String;";
const CONSUMER_ACCEPT: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const GET_OR_DEFAULT_DESC: &str = "(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
const REMOVE_DESC: &str = "(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";

/// A `CustomData` holding `tag` itself (vanilla's private constructor).
fn custom_data(rt: &mut Rt, tag: V) -> V {
    let d = rt.new_object(CUSTOM_DATA);
    rt.put_field(d, "tag", tag);
    d
}

/// `CustomData.EMPTY`: one object, of an empty tag.
fn empty_custom_data(rt: &mut Rt) -> V {
    rt.intern("compat:custom-data/empty".into(), |rt| {
        let tag = crate::nbt::to_runtime(rt, &BTreeMap::new());
        custom_data(rt, tag)
    })
}

/// The tag `data` (a `CustomData`) holds, as NBT.
fn custom_nbt(rt: &Rt, data: V) -> BTreeMap<String, Nbt> {
    if data.r() == 0 {
        return BTreeMap::new();
    }
    crate::nbt::from_runtime(rt, rt.get_field(data, "tag"))
}

/// A stack's custom data from the NBT a game stack (or a saved one) carries.
fn load_custom_data(rt: &mut Rt, stack: V, tag: &BTreeMap<String, Nbt>) {
    let t = crate::nbt::to_runtime(rt, tag);
    let d = custom_data(rt, t);
    set_vanilla(rt, stack, CUSTOM_DATA_ID, d);
}

/// Whether `x` and `y` are custom data holding equal tags (vanilla's `CustomData.equals`); None
/// when either isn't custom data.
pub(crate) fn same_custom_data(rt: &Rt, x: V, y: V) -> Option<bool> {
    (x.r() != 0 && y.r() != 0 && rt.class_of(x) == CUSTOM_DATA && rt.class_of(y) == CUSTOM_DATA).then(|| custom_nbt(rt, x) == custom_nbt(rt, y))
}

/// Custom data's hash, from its tag's contents (equal tags, equal hashes); None for anything else.
pub(crate) fn custom_data_hash(rt: &Rt, v: V) -> Option<i32> {
    (v.r() != 0 && rt.class_of(v) == CUSTOM_DATA).then(|| crate::nbt::hash(&custom_nbt(rt, v)) as i32)
}

/// `CustomData`'s own calls, as vanilla's.
fn custom_data_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let tag = |rt: &Rt, d: V| if d.r() == 0 { V::NULL } else { rt.get_field(d, "tag") };
    Some(match name {
        "<init>" => {
            rt.put_field(a(0), "tag", a(1));
            V::Void
        }
        // `of(tag)`: a copy of the tag.
        "of" => {
            let t = crate::api::call(rt, TAG_COPY, &[a(0)]);
            custom_data(rt, t)
        }
        "getUnsafe" => tag(rt, a(0)),
        "copyTag" => {
            let t = tag(rt, a(0));
            crate::api::call(rt, TAG_COPY, &[t])
        }
        "isEmpty" => V::I(i32::from(custom_nbt(rt, a(0)).is_empty())),
        "size" => {
            let t = tag(rt, a(0));
            crate::api::call(rt, TAG_SIZE, &[t])
        }
        "contains" => {
            let t = tag(rt, a(0));
            crate::api::call(rt, TAG_CONTAINS, &[t, a(1)])
        }
        // `matchedBy(tag)`: `NbtUtils.compareNbt(tag, this.tag, true)`.
        "matchedBy" => {
            let expected = if a(1).r() == 0 { None } else { crate::nbt::element_from_runtime(rt, a(1)) };
            let actual = Nbt::Compound(custom_nbt(rt, a(0)));
            V::I(i32::from(expected.is_none_or(|e| crate::nbt::matches(&e, &actual, true))))
        }
        // `update(updater)`: a copy of the tag, as the updater leaves it.
        "update" if desc == "(Ljava/util/function/Consumer;)Lnet/minecraft/world/item/component/CustomData;" => updated(rt, a(0), a(1)),
        // `update(type, stack, updater)`: the stack's (or an empty one), updated; an empty result
        // is taken off the stack.
        "update" if desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;Lnet/minecraft/world/item/ItemStack;Ljava/util/function/Consumer;)") => {
            let empty = empty_custom_data(rt);
            let held = call(rt, ITEM_STACK, "getOrDefault", GET_OR_DEFAULT_DESC, &[a(1), a(0), empty]).unwrap_or(V::NULL);
            let data = updated(rt, if held.r() == 0 { empty } else { held }, a(2));
            put_custom_data(rt, a(0), a(1), data);
            V::Void
        }
        // `set(type, stack, tag)`: a copy of the tag on the stack; an empty tag takes it off.
        "set" if desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/nbt/CompoundTag;)") => {
            let t = crate::api::call(rt, TAG_COPY, &[a(2)]);
            let data = custom_data(rt, t);
            put_custom_data(rt, a(0), a(1), data);
            V::Void
        }
        "equals" => V::I(i32::from(a(0) == a(1) || same_custom_data(rt, a(0), a(1)) == Some(true))),
        "hashCode" => V::I(custom_data_hash(rt, a(0)).unwrap_or(0)),
        "toString" => {
            let t = tag(rt, a(0));
            crate::api::call(rt, TAG_TO_STRING, &[t])
        }
        _ => return None,
    })
}

/// `data.update(updater)`: a copy of its tag given to the updater, then held by new custom data.
fn updated(rt: &mut Rt, data: V, updater: V) -> V {
    let t = rt.get_field(data, "tag");
    let t = crate::api::call(rt, TAG_COPY, &[t]);
    rt.call_functional(CONSUMER_ACCEPT, updater, &[t]);
    custom_data(rt, t)
}

/// Custom data on `stack` as type `ty`, or none when its tag is empty (vanilla keeps no empty
/// custom data on a stack).
fn put_custom_data(rt: &mut Rt, ty: V, stack: V, data: V) {
    if custom_nbt(rt, data).is_empty() {
        call(rt, ITEM_STACK, "remove", REMOVE_DESC, &[stack, ty]);
    } else {
        call(rt, ITEM_STACK, "set", SET_DESC, &[stack, ty, data]);
    }
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        "net/minecraft/core/component/DataComponents" if !NOT_TYPES.contains(&field) && !field.ends_with("CODEC") => {
            Some(vanilla_type(rt, &format!("minecraft:{}", field.to_ascii_lowercase())))
        }
        MAP if field == "EMPTY" => Some(rt.intern("compat:component-map/empty".into(), |rt| plain_map(rt, Vec::new()))),
        CUSTOM_DATA if field == "EMPTY" => Some(empty_custom_data(rt)),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    let stack_method = matches!(
        name,
        "isComponentsPatchEmpty" | "getComponentsPatch" | "getDamageValue" | "setDamageValue" | "getMaxDamage" | "isDamageableItem" | "isDamaged" | "hurtAndBreak" | "hurtWithoutBreaking"
    ) || BAR.iter().any(|(n, ..)| *n == name);
    let typed = desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;") || desc.starts_with("(Ljava/util/function/Supplier;");
    let maps = matches!(owner, MAP | MAP_BUILDER | PATCH_BUILDER)
        || owner.ends_with("/PatchedDataComponentMap")
        || (owner == COMPONENT_PATCH && matches!(name, "builder" | "size" | "get" | "entrySet" | "forget"))
        || (name == "components" && desc == MAP_DESC)
        || owner == INPUT
        || matches!(name, "applyComponentsFromItemStack" | "setComponents" | "collectComponents" | "applyImplicitComponents" | "collectImplicitComponents");
    let stack_maps = matches!(name, "getComponents" | "applyComponents" | "applyComponentsAndValidate");
    let fluid_stack = owner == FLUID_STACK && (typed || matches!(name, "getComponents" | "getComponentsPatch" | "isComponentsPatchEmpty" | "applyComponents"));
    // An item's own bar methods a mod's override hands to its super (`super.getBarColor(stack)`).
    let item_bar = crate::api_common::is_item_class(owner) && BAR.iter().any(|(n, ..)| *n == name);
    matches!(owner, TYPE | BUILDER | CUSTOM_DATA) || maps || fluid_stack || item_bar || ((owner == ITEM_STACK || HOLDERS.contains(&owner)) && (stack_method || stack_maps || typed))
}

pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on component types, their builders, and a stack's components; None for anything else.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
        (CUSTOM_DATA, _) => custom_data_call(rt, name, desc, args),
        (TYPE, "builder") => Some(rt.new_object(BUILDER)),
        // `new DataComponentType.Builder<>()` (SophisticatedCore's): what `builder()` makes.
        (BUILDER, "<init>") => Some(V::Void),
        (BUILDER, "persistent") => {
            rt.put_field(a(0), "$codec", a(1));
            Some(a(0))
        }
        (BUILDER, "networkSynchronized" | "cacheEncoding") => Some(a(0)),
        (BUILDER, "build") => Some(build(rt, a(0))),
        (TYPE, "codec") => Some(match rt.get_field(a(0), "$codec") {
            V::Void => V::NULL,
            c => c,
        }),
        (TYPE, "isTransient") => Some(V::I((rt.get_field(a(0), "$codec").r() == 0) as i32)),
        // One object per type: compared by identity.
        (TYPE, "equals") => Some(V::I((a(0) == a(1)) as i32)),
        (TYPE, "hashCode") => Some(V::I(a(0).r() as i32)),
        // `getComponents()`: the stack's components as a map, live.
        (o, "getComponents") if (o == ITEM_STACK || o == FLUID_STACK || HOLDERS.contains(&o)) && holder(rt, a(0)) => Some(stack_view(rt, a(0))),
        // `applyComponents(map)`: each of the map's components set on the stack; `(patch)`: the
        // patch's sets and removals made.
        (ITEM_STACK | FLUID_STACK, "applyComponents") if desc == "(Lnet/minecraft/core/component/DataComponentMap;)V" => {
            let l = set_pairs(rt, a(1));
            for p in l.chunks(2) {
                call(rt, ITEM_STACK, "set", SET_DESC, &[a(0), p[0], p[1]]);
            }
            Some(V::Void)
        }
        // `applyComponentsAndValidate(patch)`: the patch applied (the stack's own checks aside).
        (ITEM_STACK | FLUID_STACK, "applyComponents" | "applyComponentsAndValidate") => {
            apply_patch(rt, a(0), a(1));
            Some(V::Void)
        }
        // `Item.components()`: the item's default components, as a new stack of it reads them.
        (_, "components") if desc == MAP_DESC && rt.instance_of(a(0), "net/minecraft/world/item/Item") => {
            let id = crate::api_common::item_like_name(rt, a(0));
            let stack = crate::api_common::stack(rt, &id, 1);
            Some(stack_view(rt, stack))
        }
        (MAP, "builder") => Some(rt.new_object(MAP_BUILDER)),
        (MAP_BUILDER, "<init>") | (PATCH_BUILDER, "<init>") => Some(V::Void),
        // `set(type, value)` (a null value takes it out), `addAll(map)`, then `build()`.
        (MAP_BUILDER, "set") => {
            let ty = type_of(rt, a(1));
            put(rt, a(0), ty, a(2));
            Some(a(0))
        }
        (MAP_BUILDER, "addAll") => {
            let l = map_pairs(rt, a(1));
            for p in l.chunks(2) {
                put(rt, a(0), p[0], p[1]);
            }
            Some(a(0))
        }
        (MAP_BUILDER, "build") => {
            let l = pairs(rt, a(0));
            Some(plain_map(rt, l))
        }
        // `composite(prototype, overrides)`: the overrides' components over the prototype's.
        (MAP, "composite") => {
            let (base, over) = (map_pairs(rt, a(0)), map_pairs(rt, a(1)));
            let m = plain_map(rt, base);
            for p in over.chunks(2) {
                put(rt, m, p[0], p[1]);
            }
            Some(m)
        }
        (o, n) if (o == MAP || o.ends_with("/PatchedDataComponentMap")) && rt.class_of(a(0)) == MAP => map_call(rt, n, desc, args),
        (COMPONENT_PATCH, "builder") => Some(rt.new_object(PATCH_BUILDER)),
        // What a block entity reads of the stack it is placed from; each type it reads is kept.
        (INPUT, n @ ("get" | "getOrDefault")) => {
            let map = rt.get_field(a(0), "$map");
            let ty = type_of(rt, a(1));
            let used = rt.get_field(a(0), "$used");
            if used.r() != 0 {
                crate::jutil::list_add(rt, used, ty);
            }
            let mut all = vec![map, ty];
            all.extend_from_slice(args.get(2..).unwrap_or_default());
            let desc = if n == "get" { GET_DESC } else { "(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;" };
            map_call(rt, n, desc, &all)
        }
        // A block entity's components: what it kept of the stack it was placed from
        // (`components()`, `setComponents`), and all it gives its item (`collectComponents()`).
        (_, "applyComponentsFromItemStack") if desc == "(Lnet/minecraft/world/item/ItemStack;)V" => {
            apply_from_stack(rt, a(0), a(1));
            Some(V::Void)
        }
        (_, "components") if desc == MAP_DESC && rt.instance_of(a(0), "net/minecraft/world/level/block/entity/BlockEntity") => {
            let kept = rt.get_field(a(0), "$components");
            Some(if kept.r() != 0 { kept } else { rt.intern("compat:component-map/empty".into(), |rt| plain_map(rt, Vec::new())) })
        }
        (_, "setComponents") if desc == "(Lnet/minecraft/core/component/DataComponentMap;)V" => {
            let l = map_pairs(rt, a(1));
            let m = plain_map(rt, l);
            rt.put_field(a(0), "$components", m);
            Some(V::Void)
        }
        (_, "collectComponents") if desc == MAP_DESC => Some(collect_components(rt, a(0))),
        // Vanilla's own do nothing: a mod's override calls them through `super`.
        (_, "applyImplicitComponents") if desc == "(Lnet/minecraft/world/level/block/entity/BlockEntity$DataComponentInput;)V" => Some(V::Void),
        (_, "collectImplicitComponents") if desc == "(Lnet/minecraft/core/component/DataComponentMap$Builder;)V" => Some(V::Void),
        (COMPONENT_PATCH, n @ ("size" | "get" | "entrySet")) => patch_call(rt, n, args),
        (COMPONENT_PATCH, "forget") => Some(forget(rt, a(0), a(1))),
        // A patch's builder: what it sets and what it removes, each undoing the other.
        (PATCH_BUILDER, "set" | "remove") => {
            let ty = type_of(rt, a(1));
            let setting = name == "set";
            put(rt, a(0), ty, if setting { a(2) } else { V::NULL });
            let mut gone: Vec<V> = removed(rt, a(0)).into_iter().filter(|&t| t != ty).collect();
            if !setting {
                gone.push(ty);
            }
            set_removed(rt, a(0), gone);
            Some(a(0))
        }
        (PATCH_BUILDER, "build") => {
            let p = rt.new_object(COMPONENT_PATCH);
            let (l, gone) = (pairs(rt, a(0)), removed(rt, a(0)));
            if !l.is_empty() {
                store(rt, p, l);
            }
            set_removed(rt, p, gone);
            Some(p)
        }
        (TYPE, "codecOrThrow") => {
            let c = rt.get_field(a(0), "$codec");
            if c.r() == 0 {
                let e = rt.new_object("java/lang/IllegalStateException");
                let m = rt.string("Component type is not persistent");
                rt.put_field(e, "message", m);
                rt.thrown = Some(e);
                return Some(V::NULL);
            }
            Some(c)
        }
        (ITEM_STACK, "isComponentsPatchEmpty") => Some(V::I(pairs(rt, a(0)).is_empty() as i32)),
        // An empty fluid stack's patch is empty, whatever was set on it.
        (FLUID_STACK, "isComponentsPatchEmpty") => Some(V::I((crate::fluids::stack_empty(rt, a(0)) || pairs(rt, a(0)).is_empty()) as i32)),
        (FLUID_STACK, "getComponentsPatch") if crate::fluids::stack_empty(rt, a(0)) => crate::recipes::get_static(rt, COMPONENT_PATCH, "EMPTY"),
        // What was set on the stack, as `ItemStack.CODEC` writes its `components` (empty: the one
        // `EMPTY`).
        // The values themselves stay on it too (a transient type's among them), for a stack it is
        // applied to (`applyComponents(other.getComponentsPatch())`).
        (ITEM_STACK | FLUID_STACK, "getComponentsPatch") => {
            let patch = patch(rt, a(0));
            let kept = pairs(rt, a(0));
            let json = serde_json::Value::Object(patch.iter().map(|(k, v)| (k.clone(), crate::recipes::nbt_json(v))).collect());
            let p = crate::recipes::component_patch(rt, (!patch.is_empty() || !kept.is_empty()).then_some(&json));
            if !kept.is_empty() {
                store(rt, p, kept);
            }
            Some(p)
        }
        // Durability: `ItemStack`'s own damage, worn by the game's rules on a stack in hand.
        (ITEM_STACK, "getDamageValue") => Some(V::I(damage(rt, a(0)).clamp(0, max_damage(rt, a(0)).max(0)))),
        (ITEM_STACK, "setDamageValue") => {
            set_damage(rt, a(0), a(1).i());
            Some(V::Void)
        }
        (ITEM_STACK, "getMaxDamage") => Some(V::I(max_damage(rt, a(0)))),
        (ITEM_STACK, "isDamageableItem") => Some(V::I((max_damage(rt, a(0)) > 0) as i32)),
        (ITEM_STACK, "isDamaged") => Some(V::I((max_damage(rt, a(0)) > 0 && damage(rt, a(0)) > 0) as i32)),
        // The bar under an item, each part asking the stack's item.
        (ITEM_STACK, "isBarVisible" | "getBarWidth" | "getBarColor") => {
            let item = rt.get_field(a(0), "$item");
            Some(bar_virtually(rt, name, item, a(0)))
        }
        (o, "isBarVisible" | "getBarWidth" | "getBarColor") if crate::api_common::is_item_class(o) => Some(default_bar(rt, name, a(1))),
        // `hurtAndBreak(amount, entity, slot)` and `(amount, level, entity, onBreak)`: worn by the
        // entity holding it.
        (ITEM_STACK, "hurtAndBreak") => {
            let wearer = if desc.starts_with("(ILnet/minecraft/server/level/ServerLevel;") { a(3) } else { a(2) };
            crate::hand::hurt(rt, a(0), a(1).i(), wearer);
            Some(V::Void)
        }
        (ITEM_STACK, "hurtWithoutBreaking") => {
            let max = max_damage(rt, a(0));
            if max > 0 && !rt.host().is_some_and(|h| h.player_creative()) {
                let worn = (damage(rt, a(0)) + a(1).i()).min(max - 1);
                set_damage(rt, a(0), worn);
            }
            Some(V::Void)
        }
        (o, _) if (o == ITEM_STACK || o == FLUID_STACK || HOLDERS.contains(&o)) && holder(rt, a(0)) => {
            let typed = desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;") || desc.starts_with("(Ljava/util/function/Supplier;");
            if !typed {
                return None;
            }
            let (stack, ty) = (a(0), type_of(rt, a(1)));
            // An item's damage is the stack's own; a fluid stack keeps any type as it was set.
            if rt.class_of(stack) == ITEM_STACK
                && let Some(v) = damage_component(rt, name, stack, ty, args)
            {
                return Some(v);
            }
            Some(match name {
                "has" => V::I(current(rt, stack, ty).is_some() as i32),
                "get" => current(rt, stack, ty).unwrap_or(V::NULL),
                "getOrDefault" => current(rt, stack, ty).unwrap_or(a(2)),
                "set" => put(rt, stack, ty, a(2)),
                "remove" => put(rt, stack, ty, V::NULL),
                // (type, default, UnaryOperator) or (type, default, argument, BiFunction): the
                // operator's result is set, and the value before it returned, as vanilla does.
                "update" => {
                    let held = current(rt, stack, ty).unwrap_or(a(2));
                    let next = if desc.contains("Ljava/util/function/UnaryOperator;") {
                        rt.call_functional("java/util/function/UnaryOperator.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(3), &[held])
                    } else {
                        rt.call_functional("java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", a(4), &[held, a(3)])
                    };
                    put(rt, stack, ty, next)
                }
                _ => return None,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api, api_common};

    fn boxed(rt: &mut Rt, n: i32) -> V {
        api::call(rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(n)])
    }

    /// The test's consumer (`test/Charger`): `tag -> tag.putByte("charge", 3)`; every other call
    /// is the API's, as a translated mod's dispatch leaves it.
    fn charger(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        if key == "accept.(Ljava/lang/Object;)V" && rt.class_of(args[0]) == "test/Charger" {
            let k = rt.string("charge");
            return api::call(rt, "net/minecraft/nbt/CompoundTag.putByte.(Ljava/lang/String;B)V", &[args[1], k, V::I(3)]);
        }
        api::call(rt, owner_key, args)
    }

    /// The test's item with a bar of its own (`test/Progress`, as Create's sequenced assembly):
    /// always shown, 7 pixels, teal; every other call is the API's.
    fn progress(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) == "test/Progress" {
            match key {
                "isBarVisible.(Lnet/minecraft/world/item/ItemStack;)Z" => return V::I(1),
                "getBarWidth.(Lnet/minecraft/world/item/ItemStack;)I" => return V::I(7),
                "getBarColor.(Lnet/minecraft/world/item/ItemStack;)I" => return V::I(0x46_FFE0),
                _ => {}
            }
        }
        api::call(rt, owner_key, args)
    }

    /// A stack's bar is its item's: a mod item's own where its class has one; else NeoForge's,
    /// shown while damaged, 7 of 13 pixels and yellow at half durability, green when whole.
    #[test]
    fn a_stacks_bar_is_its_items_own_else_its_durability() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(progress);
        rt.supers.insert("test/Progress", "net/minecraft/world/item/Item");
        let it = rt.new_object("test/Progress");
        rt.registered.insert(("item".into(), "test:progress".into()), it);
        let bar = |rt: &mut Rt, stack: V| {
            let visible = api::call(rt, "net/minecraft/world/item/ItemStack.isBarVisible.()Z", &[stack]).i() != 0;
            let width = api::call(rt, "net/minecraft/world/item/ItemStack.getBarWidth.()I", &[stack]).i();
            let colour = api::call(rt, "net/minecraft/world/item/ItemStack.getBarColor.()I", &[stack]).i();
            (visible, width, colour)
        };
        let pick = api_common::stack(&mut rt, "minecraft:iron_pickaxe", 1);
        rt.put_field(pick, "$maxDamage", V::I(250));
        assert_eq!(bar(&mut rt, pick), (false, 13, 0x00_FF00));
        rt.put_field(pick, "$damage", V::I(125));
        assert_eq!(bar(&mut rt, pick), (true, 7, 0xFF_FF00));
        let made = api_common::stack(&mut rt, "test:progress", 1);
        assert_eq!(bar(&mut rt, made), (true, 7, 0x46_FFE0));
        // A mod's override asking its super gets NeoForge's.
        let colour = api::call(&mut rt, "net/minecraft/world/item/Item.getBarColor.(Lnet/minecraft/world/item/ItemStack;)I", &[it, pick]);
        assert_eq!(colour, V::I(0xFF_FF00));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The bar the game draws under a mod's item: the item's own parts where its class has them,
    /// the game's bar for the rest; an item with none of its own isn't asked.
    #[test]
    fn the_game_draws_a_mod_items_own_bar() {
        fn overrides(class: &str, key: &str) -> bool {
            match class {
                "test/Progress" => key.starts_with("isBarVisible.") || key.starts_with("getBarWidth.") || key.starts_with("getBarColor."),
                "test/Tinted" => key.starts_with("getBarColor."),
                _ => false,
            }
        }
        fn tinted(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
            if rt.class_of(args[0]) == "test/Tinted" && key.starts_with("getBarColor.") {
                return V::I(0x12_3456);
            }
            progress(rt, owner_key, key, args)
        }
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(tinted);
        rt.overrides = Some(overrides);
        for (class, name) in [("test/Progress", "test:progress"), ("test/Tinted", "test:tinted"), ("test/Plain", "test:plain")] {
            rt.supers.insert(class, "net/minecraft/world/item/Item");
            let it = rt.new_object(class);
            rt.registered.insert(("item".into(), name.into()), it);
        }
        let held = |item: &str| crate::host::HandStack { item: item.into(), count: 1, damage: 0, max_damage: 0, components: Vec::new() };
        assert!(own_bar(&rt, "test:progress") && own_bar(&rt, "test:tinted"));
        assert!(!own_bar(&rt, "test:plain") && !own_bar(&rt, "minecraft:iron_pickaxe"));
        assert_eq!(held_bar(&mut rt, held("test:progress"), None), Some((7, 0x46_FFE0)));
        // Its own colour on the game's bar; no bar where the game shows none.
        assert_eq!(held_bar(&mut rt, held("test:tinted"), Some((5, 0xFF_0000))), Some((5, 0x12_3456)));
        assert_eq!(held_bar(&mut rt, held("test:tinted"), None), None);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's custom data on a stack (`minecraft:custom_data`), as vanilla's: updated through a
    /// copy, read as copies, matched in part, compared by its tag. A game stack gets it as NBT
    /// (its tags' kinds kept) and a stack read from that has it again; an empty tag takes it off.
    #[test]
    fn a_stacks_custom_data_is_vanillas_and_crosses_to_the_game() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(charger);
        let ty = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.CUSTOM_DATA");
        let data_class = "net/minecraft/world/item/component/CustomData";
        let update = "net/minecraft/world/item/component/CustomData.update.(Lnet/minecraft/core/component/DataComponentType;Lnet/minecraft/world/item/ItemStack;Ljava/util/function/Consumer;)V";
        let set = "net/minecraft/world/item/component/CustomData.set.(Lnet/minecraft/core/component/DataComponentType;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/nbt/CompoundTag;)V";
        let copy_tag = "net/minecraft/world/item/component/CustomData.copyTag.()Lnet/minecraft/nbt/CompoundTag;";
        let contains = "net/minecraft/world/item/component/CustomData.contains.(Ljava/lang/String;)Z";
        let matched = "net/minecraft/world/item/component/CustomData.matchedBy.(Lnet/minecraft/nbt/CompoundTag;)Z";
        let get = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let or = "net/minecraft/world/item/ItemStack.getOrDefault.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let same = "net/minecraft/world/item/ItemStack.isSameItemSameComponents.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";
        let get_byte = "net/minecraft/nbt/CompoundTag.getByte.(Ljava/lang/String;)B";
        let put_byte = "net/minecraft/nbt/CompoundTag.putByte.(Ljava/lang/String;B)V";

        let s = api_common::stack(&mut rt, "minecraft:stick", 1);
        let empty = api::get_static(&mut rt, "net/minecraft/world/item/component/CustomData.EMPTY");
        assert_eq!(api::call(&mut rt, or, &[s, ty, empty]), empty, "a stick has none of its own");
        let charger = rt.new_object("test/Charger");
        api::call(&mut rt, update, &[ty, s, charger]);
        let data = api::call(&mut rt, get, &[s, ty]);
        assert_eq!(rt.class_of(data), data_class);
        let charge = rt.string("charge");
        assert_eq!(api::call(&mut rt, contains, &[data, charge]), V::I(1));
        let copy = api::call(&mut rt, copy_tag, &[data]);
        assert_eq!(api::call(&mut rt, get_byte, &[copy, charge]), V::I(3));
        // The copy is the mod's: changing it leaves the stack's as it was.
        api::call(&mut rt, put_byte, &[copy, charge, V::I(9)]);
        let fresh = api::call(&mut rt, copy_tag, &[data]);
        assert_eq!(api::call(&mut rt, get_byte, &[fresh, charge]), V::I(3));
        assert_eq!(api::call(&mut rt, matched, &[data, fresh]), V::I(1));
        assert_eq!(api::call(&mut rt, matched, &[data, copy]), V::I(0), "9 isn't 3");
        let none = crate::nbt::to_runtime(&mut rt, &BTreeMap::new());
        assert_eq!(api::call(&mut rt, matched, &[data, none]), V::I(1), "an empty tag asks nothing");

        // To the game: a byte stays a byte. Back: a new stack holds equal data, so it stacks.
        let carried = saved(&mut rt, s);
        let tag = BTreeMap::from([("charge".to_owned(), Nbt::Byte(3))]);
        assert_eq!(carried, vec![(CUSTOM_DATA_ID.to_owned(), Nbt::Compound(tag.clone()))]);
        let back = api_common::stack(&mut rt, "minecraft:stick", 1);
        load(&mut rt, back, &carried);
        assert_ne!(api::call(&mut rt, get, &[back, ty]), data, "its own object");
        assert_eq!(api::call(&mut rt, same, &[s, back]), V::I(1), "equal by its tag");
        assert_eq!(saved(&mut rt, back), carried);
        let other = api_common::stack(&mut rt, "minecraft:stick", 1);
        let nine = crate::nbt::to_runtime(&mut rt, &BTreeMap::from([("charge".to_owned(), Nbt::Byte(9))]));
        api::call(&mut rt, set, &[ty, other, nine]);
        assert_eq!(api::call(&mut rt, same, &[s, other]), V::I(0));

        // Saved by a block entity (`ItemStack.save`) and read back (`parseOptional`): the same.
        let saved_stack = BTreeMap::from([
            ("id".to_owned(), Nbt::String("minecraft:stick".into())),
            ("count".to_owned(), Nbt::Int(1)),
            ("components".to_owned(), Nbt::Compound(patch(&mut rt, s))),
        ]);
        let parsed = api_common::parse_stack(&mut rt, &saved_stack).expect("a stick");
        assert_eq!(saved(&mut rt, parsed), carried);

        // `set` with an empty tag takes it off.
        api::call(&mut rt, set, &[ty, s, none]);
        assert_eq!(api::call(&mut rt, get, &[s, ty]), V::NULL);
        assert!(saved(&mut rt, s).is_empty());
    }

    #[test]
    fn a_stack_keeps_the_components_set_on_it_and_its_copies_carry_them() {
        let mut rt = Rt::new();
        crate::install();
        // A mod's component type, as Create builds its own.
        let b = api::call(&mut rt, "net/minecraft/core/component/DataComponentType.builder.()Lnet/minecraft/core/component/DataComponentType$Builder;", &[]);
        let codec = api::get_static(&mut rt, "com/mojang/serialization/Codec.INT");
        let b = api::call(&mut rt, "net/minecraft/core/component/DataComponentType$Builder.persistent.(Lcom/mojang/serialization/Codec;)Lnet/minecraft/core/component/DataComponentType$Builder;", &[b, codec]);
        let progress = api::call(&mut rt, "net/minecraft/core/component/DataComponentType$Builder.build.()Lnet/minecraft/core/component/DataComponentType;", &[b]);
        let damage = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.DAMAGE");
        assert_eq!(damage, api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.DAMAGE"), "one object per vanilla type");

        let set = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let has = "net/minecraft/core/component/DataComponentHolder.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
        let get = "net/minecraft/core/component/DataComponentHolder.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let or = "net/minecraft/core/component/DataComponentHolder.getOrDefault.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let remove = "net/minecraft/world/item/ItemStack.remove.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let same = "net/minecraft/world/item/ItemStack.isSameItemSameComponents.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";

        let s = api_common::stack(&mut rt, "minecraft:iron_ingot", 3);
        assert_eq!(api::call(&mut rt, has, &[s, progress]), V::I(0));
        let seven = boxed(&mut rt, 7);
        assert_eq!(api::call(&mut rt, or, &[s, progress, seven]), seven);
        let two = boxed(&mut rt, 2);
        assert_eq!(api::call(&mut rt, set, &[s, progress, two]), V::NULL, "nothing was set before");
        let five = boxed(&mut rt, 5);
        assert_eq!(api::call(&mut rt, set, &[s, progress, five]), two, "set returns the value it replaced");
        assert_eq!(api::call(&mut rt, has, &[s, progress]), V::I(1));
        assert_eq!(api::call(&mut rt, has, &[s, damage]), V::I(0));

        // A copy carries the components; an equal value counts as the same, another doesn't.
        let copy = api::call(&mut rt, "net/minecraft/world/item/ItemStack.copyWithCount.(I)Lnet/minecraft/world/item/ItemStack;", &[s, V::I(1)]);
        assert_eq!(api::call(&mut rt, get, &[copy, progress]), five);
        let plain = api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        assert_eq!(api::call(&mut rt, same, &[s, plain]), V::I(0));
        let other = api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        let five_again = boxed(&mut rt, 5);
        api::call(&mut rt, set, &[other, progress, five_again]);
        assert_eq!(api::call(&mut rt, same, &[s, other]), V::I(1));

        assert_eq!(api::call(&mut rt, remove, &[s, progress]), five);
        assert_eq!(api::call(&mut rt, has, &[s, progress]), V::I(0));
        assert_eq!(api::call(&mut rt, get, &[copy, progress]), five, "the copy keeps its own");
        let empty = api::get_static(&mut rt, "net/minecraft/world/item/ItemStack.EMPTY");
        api::call(&mut rt, set, &[empty, progress, two]);
        assert_eq!(api::call(&mut rt, has, &[empty, progress]), V::I(0), "the shared empty stack takes nothing");
    }

    /// A stack saved as a block entity keeps one (`ItemStack.save`) and read back (`parseOptional`)
    /// keeps the mod's component, under vanilla's `components`; `ItemStack.CODEC` writes and reads it
    /// the same way.
    #[test]
    fn a_saved_stack_keeps_the_mods_components() {
        let mut rt = Rt::new();
        crate::install();
        let b = api::call(&mut rt, "net/minecraft/core/component/DataComponentType.builder.()Lnet/minecraft/core/component/DataComponentType$Builder;", &[]);
        let codec = api::get_static(&mut rt, "com/mojang/serialization/Codec.STRING");
        let b = api::call(&mut rt, "net/minecraft/core/component/DataComponentType$Builder.persistent.(Lcom/mojang/serialization/Codec;)Lnet/minecraft/core/component/DataComponentType$Builder;", &[b, codec]);
        let address = api::call(&mut rt, "net/minecraft/core/component/DataComponentType$Builder.build.()Lnet/minecraft/core/component/DataComponentType;", &[b]);
        rt.registered.insert((REGISTRY.into(), "test:address".into()), address);
        let set = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let get = "net/minecraft/core/component/DataComponentHolder.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";

        let s = api_common::stack(&mut rt, "minecraft:paper", 2);
        let mill = rt.string("Mill");
        api::call(&mut rt, set, &[s, address, mill]);
        let tag = api::call(&mut rt, "net/minecraft/world/item/ItemStack.save.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/Tag;", &[s, V::NULL]);
        let saved = crate::nbt::from_runtime(&rt, tag);
        assert_eq!(
            saved.get("components"),
            Some(&Nbt::Compound([("test:address".to_owned(), Nbt::String("Mill".into()))].into_iter().collect()))
        );
        let back = api::call(&mut rt, "net/minecraft/world/item/ItemStack.parseOptional.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/world/item/ItemStack;", &[V::NULL, tag]);
        let read = api::call(&mut rt, get, &[back, address]);
        assert_eq!(rt.str_of(read), Some("Mill"));
        assert_eq!(rt.get_field(back, "$count").i(), 2);

        let stack_codec = api::get_static(&mut rt, "net/minecraft/world/item/ItemStack.CODEC");
        let written = crate::codecs::encode(&mut rt, stack_codec, s).unwrap();
        let again = crate::codecs::decode(&mut rt, stack_codec, &crate::recipes::nbt_json(&written)).unwrap();
        let read = api::call(&mut rt, get, &[again, address]);
        assert_eq!(rt.str_of(read), Some("Mill"));

        // Its patch (`getComponentsPatch`) holds what was set; a plain stack's is `EMPTY`.
        let patch_of = "net/minecraft/world/item/ItemStack.getComponentsPatch.()Lnet/minecraft/core/component/DataComponentPatch;";
        let is_empty = "net/minecraft/core/component/DataComponentPatch.isEmpty.()Z";
        let p = api::call(&mut rt, patch_of, &[s]);
        assert_eq!(api::call(&mut rt, is_empty, &[p]), V::I(0));
        let plain = api_common::stack(&mut rt, "minecraft:paper", 1);
        let p = api::call(&mut rt, patch_of, &[plain]);
        assert_eq!(p, api::get_static(&mut rt, "net/minecraft/core/component/DataComponentPatch.EMPTY"));
        assert_eq!(api::call(&mut rt, is_empty, &[p]), V::I(1));
    }

    /// The test predicate: whether the type is the one it captured.
    fn is_captured(rt: &mut Rt, _key: &'static str, short: &'static str, args: &[V]) -> V {
        assert_eq!(short, "test.(Ljava/lang/Object;)Z");
        let c = rt.captured(args[0]);
        V::I((c.first() == args.get(1)) as i32)
    }

    /// `patch.forget(predicate)` (Create's backtank's drops): the types the predicate takes are left
    /// out, whether the patch sets or removes them; with nothing left, the one `EMPTY`.
    #[test]
    fn a_patch_forgets_the_types_a_predicate_takes() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(is_captured);
        let mut types = Vec::new();
        for _ in 0..2 {
            let b = api::call(&mut rt, "net/minecraft/core/component/DataComponentType.builder.()Lnet/minecraft/core/component/DataComponentType$Builder;", &[]);
            types.push(api::call(&mut rt, "net/minecraft/core/component/DataComponentType$Builder.build.()Lnet/minecraft/core/component/DataComponentType;", &[b]));
        }
        let (air, label) = (types[0], types[1]);
        let pb = api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch.builder.()Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[]);
        let full = api_common::integer(&mut rt, 900);
        api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[pb, air, full]);
        api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.remove.(Lnet/minecraft/core/component/DataComponentType;)Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[pb, label]);
        let patch = api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.build.()Lnet/minecraft/core/component/DataComponentPatch;", &[pb]);
        let forget = "net/minecraft/core/component/DataComponentPatch.forget.(Ljava/util/function/Predicate;)Lnet/minecraft/core/component/DataComponentPatch;";
        let size = "net/minecraft/core/component/DataComponentPatch.size.()I";
        let get = "net/minecraft/core/component/DataComponentPatch.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/util/Optional;";
        let not_air = rt.lambda("test/Pred.test", vec![air]);
        let left = api::call(&mut rt, forget, &[patch, not_air]);
        assert_eq!(api::call(&mut rt, size, &[left]), V::I(1));
        assert_eq!(api::call(&mut rt, get, &[left, air]), V::NULL, "forgotten");
        assert_ne!(api::call(&mut rt, get, &[left, label]).r(), 0, "still removed");
        assert_eq!(api::call(&mut rt, size, &[patch]), V::I(2), "the patch itself stays");
        let not_label = rt.lambda("test/Pred.test", vec![label]);
        let none = api::call(&mut rt, forget, &[left, not_label]);
        assert_eq!(none, api::get_static(&mut rt, "net/minecraft/core/component/DataComponentPatch.EMPTY"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A stack's components read as a map (`getComponents`, live), a map built with
    /// `DataComponentMap.builder()` and a patch with `DataComponentPatch.builder()` put on stacks
    /// (`applyComponents`), and one stack's patch put on another carrying what was set on it, a
    /// transient type's value too.
    #[test]
    fn component_maps_and_patches_carry_a_stacks_components() {
        let mut rt = Rt::new();
        crate::install();
        let new_type = |rt: &mut Rt, codec: Option<&'static str>| {
            let mut b = api::call(rt, "net/minecraft/core/component/DataComponentType.builder.()Lnet/minecraft/core/component/DataComponentType$Builder;", &[]);
            if let Some(c) = codec {
                let c = api::get_static(rt, c);
                b = api::call(rt, "net/minecraft/core/component/DataComponentType$Builder.persistent.(Lcom/mojang/serialization/Codec;)Lnet/minecraft/core/component/DataComponentType$Builder;", &[b, c]);
            }
            api::call(rt, "net/minecraft/core/component/DataComponentType$Builder.build.()Lnet/minecraft/core/component/DataComponentType;", &[b])
        };
        let progress = new_type(&mut rt, Some("com/mojang/serialization/Codec.INT"));
        rt.registered.insert((REGISTRY.into(), "test:progress".into()), progress);
        let label = new_type(&mut rt, None);
        let set = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let get = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let has = "net/minecraft/world/item/ItemStack.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
        let map_get = "net/minecraft/core/component/DataComponentMap.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let map_has = "net/minecraft/core/component/DataComponentMap.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
        let map_empty = "net/minecraft/core/component/DataComponentMap.isEmpty.()Z";
        let apply_map = "net/minecraft/world/item/ItemStack.applyComponents.(Lnet/minecraft/core/component/DataComponentMap;)V";
        let apply_patch = "net/minecraft/world/item/ItemStack.applyComponents.(Lnet/minecraft/core/component/DataComponentPatch;)V";

        // The stack's map reads what is set on it, as it is when read.
        let s = api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        let (five, hot) = (boxed(&mut rt, 5), rt.string("hot"));
        api::call(&mut rt, set, &[s, progress, five]);
        api::call(&mut rt, set, &[s, label, hot]);
        let view = api::call(&mut rt, "net/minecraft/world/item/ItemStack.getComponents.()Lnet/minecraft/core/component/DataComponentMap;", &[s]);
        assert_eq!(api::call(&mut rt, map_get, &[view, progress]), five);
        let damage = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.DAMAGE");
        assert_eq!(api::call(&mut rt, map_has, &[view, damage]), V::I(0), "an ingot takes no damage");
        let seven = boxed(&mut rt, 7);
        api::call(&mut rt, set, &[s, progress, seven]);
        assert_eq!(api::call(&mut rt, map_get, &[view, progress]), seven, "the map is the stack's, live");
        // Its components one by one, each a type with its value.
        let it = api::call(&mut rt, "net/minecraft/core/component/DataComponentMap.iterator.()Ljava/util/Iterator;", &[view]);
        let mut seen = Vec::new();
        while api::call(&mut rt, "java/util/Iterator.hasNext.()Z", &[it]) == V::I(1) {
            let t = api::call(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
            let ty = api::call(&mut rt, "net/minecraft/core/component/TypedDataComponent.type.()Lnet/minecraft/core/component/DataComponentType;", &[t]);
            let v = api::call(&mut rt, "net/minecraft/core/component/TypedDataComponent.value.()Ljava/lang/Object;", &[t]);
            seen.push((ty, v));
        }
        assert!(seen.contains(&(progress, seven)) && seen.contains(&(label, hot)), "{seen:?}");

        // A built map, put on a stack.
        let b = api::call(&mut rt, "net/minecraft/core/component/DataComponentMap.builder.()Lnet/minecraft/core/component/DataComponentMap$Builder;", &[]);
        let nine = boxed(&mut rt, 9);
        api::call(&mut rt, "net/minecraft/core/component/DataComponentMap$Builder.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Lnet/minecraft/core/component/DataComponentMap$Builder;", &[b, progress, nine]);
        let built = api::call(&mut rt, "net/minecraft/core/component/DataComponentMap$Builder.build.()Lnet/minecraft/core/component/DataComponentMap;", &[b]);
        assert_eq!(api::call(&mut rt, map_get, &[built, progress]), nine);
        assert_eq!(api::call(&mut rt, map_has, &[built, label]), V::I(0));
        let t = api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        api::call(&mut rt, apply_map, &[t, built]);
        assert_eq!(api::call(&mut rt, get, &[t, progress]), nine);
        let none = api::get_static(&mut rt, "net/minecraft/core/component/DataComponentMap.EMPTY");
        assert_eq!(api::call(&mut rt, map_empty, &[none]), V::I(1));
        assert_eq!(api::call(&mut rt, map_get, &[none, progress]), V::NULL);

        // A built patch sets one type and takes another off; one that does nothing is empty.
        let pb = api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch.builder.()Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[]);
        let cold = rt.string("cold");
        api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[pb, label, cold]);
        api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.remove.(Lnet/minecraft/core/component/DataComponentType;)Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[pb, progress]);
        let patch = api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.build.()Lnet/minecraft/core/component/DataComponentPatch;", &[pb]);
        let is_empty = "net/minecraft/core/component/DataComponentPatch.isEmpty.()Z";
        assert_eq!(api::call(&mut rt, is_empty, &[patch]), V::I(0));
        assert_eq!(api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch.size.()I", &[patch]), V::I(2));
        api::call(&mut rt, apply_patch, &[t, patch]);
        assert_eq!(api::call(&mut rt, has, &[t, progress]), V::I(0));
        let now = api::call(&mut rt, get, &[t, label]);
        assert_eq!(rt.str_of(now), Some("cold"));
        let pb = api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch.builder.()Lnet/minecraft/core/component/DataComponentPatch$Builder;", &[]);
        let nothing = api::call(&mut rt, "net/minecraft/core/component/DataComponentPatch$Builder.build.()Lnet/minecraft/core/component/DataComponentPatch;", &[pb]);
        assert_eq!(api::call(&mut rt, is_empty, &[nothing]), V::I(1));

        // One stack's patch put on another: what was set on it, the transient label too.
        let u = api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        let from = api::call(&mut rt, "net/minecraft/world/item/ItemStack.getComponentsPatch.()Lnet/minecraft/core/component/DataComponentPatch;", &[s]);
        api::call(&mut rt, apply_patch, &[u, from]);
        assert_eq!(api::call(&mut rt, get, &[u, progress]), seven);
        assert_eq!(api::call(&mut rt, get, &[u, label]), hot);
        // A saved stack's patch (as a recipe's result carries one) is read with the types' codecs.
        let written = crate::recipes::component_patch(&mut rt, Some(&serde_json::json!({"test:progress": 12, "minecraft:damage": 0})));
        let w = api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        api::call(&mut rt, apply_patch, &[w, written]);
        let read = api::call(&mut rt, get, &[w, progress]);
        assert_eq!(rt.get_field(read, "value"), V::I(12));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A stack a mod makes of a vanilla tool wears as the game's: its durability is the game's
    /// (`max_damage`), and a map or another stack's patch carries its damage.
    #[test]
    fn a_vanilla_tools_stack_has_the_games_durability() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        host.max_damage.insert("minecraft:iron_sword".into(), 250);
        let damage = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.DAMAGE");
        let max = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.MAX_DAMAGE");
        let (sword, copy, built) = rt.with_host(&mut host, |rt| {
            let sword = api_common::stack(rt, "minecraft:iron_sword", 1);
            api::call(rt, "net/minecraft/world/item/ItemStack.setDamageValue.(I)V", &[sword, V::I(5)]);
            let copy = api_common::stack(rt, "minecraft:iron_sword", 1);
            let patch = api::call(rt, "net/minecraft/world/item/ItemStack.getComponentsPatch.()Lnet/minecraft/core/component/DataComponentPatch;", &[sword]);
            api::call(rt, "net/minecraft/world/item/ItemStack.applyComponents.(Lnet/minecraft/core/component/DataComponentPatch;)V", &[copy, patch]);
            let b = api::call(rt, "net/minecraft/core/component/DataComponentMap.builder.()Lnet/minecraft/core/component/DataComponentMap$Builder;", &[]);
            let seven = boxed(rt, 7);
            api::call(rt, "net/minecraft/core/component/DataComponentMap$Builder.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Lnet/minecraft/core/component/DataComponentMap$Builder;", &[b, damage, seven]);
            let map = api::call(rt, "net/minecraft/core/component/DataComponentMap$Builder.build.()Lnet/minecraft/core/component/DataComponentMap;", &[b]);
            let built = api_common::stack(rt, "minecraft:iron_sword", 1);
            api::call(rt, "net/minecraft/world/item/ItemStack.applyComponents.(Lnet/minecraft/core/component/DataComponentMap;)V", &[built, map]);
            (sword, copy, built)
        });
        let read = |rt: &mut Rt, host: &mut crate::host::MemoryHost, key: &'static str, s: V| rt.with_host(host, |rt| api::call(rt, key, &[s]));
        let value = "net/minecraft/world/item/ItemStack.getDamageValue.()I";
        assert_eq!(read(&mut rt, &mut host, "net/minecraft/world/item/ItemStack.getMaxDamage.()I", sword), V::I(250));
        assert_eq!(read(&mut rt, &mut host, "net/minecraft/world/item/ItemStack.isDamageableItem.()Z", sword), V::I(1));
        assert_eq!(read(&mut rt, &mut host, value, sword), V::I(5));
        assert_eq!(read(&mut rt, &mut host, value, copy), V::I(5), "the patch carries the damage");
        assert_eq!(read(&mut rt, &mut host, value, built), V::I(7), "the map sets it");
        // The stack's map holds both, as vanilla's does for a damaged sword.
        let view = read(&mut rt, &mut host, "net/minecraft/world/item/ItemStack.getComponents.()Lnet/minecraft/core/component/DataComponentMap;", sword);
        let has = "net/minecraft/core/component/DataComponentMap.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
        let held = rt.with_host(&mut host, |rt| (api::call(rt, has, &[view, damage]), api::call(rt, has, &[view, max])));
        assert_eq!(held, (V::I(1), V::I(1)));
        // A stick takes none.
        let stick = rt.with_host(&mut host, |rt| api_common::stack(rt, "minecraft:stick", 1));
        assert_eq!(read(&mut rt, &mut host, "net/minecraft/world/item/ItemStack.isDamageableItem.()Z", stick), V::I(0));
    }
}
