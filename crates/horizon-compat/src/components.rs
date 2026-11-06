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
//! stored ones) here and back; of an item's default components (vanilla's prototype) only
//! `enchantments` is modelled (`ItemEnchantments.EMPTY`: `get(MAX_STACK_SIZE)` is null).
//!
//! Component maps: a stack's as a live view (`ItemStack.getComponents()`), maps built by
//! `DataComponentMap.builder()` and patches by `DataComponentPatch.builder()`, which
//! `ItemStack.applyComponents` puts on a stack.
use crate::nbt::Nbt;
use crate::{Data, Rt, V};

pub(crate) const TYPE: &str = "net/minecraft/core/component/DataComponentType";
const BUILDER: &str = "net/minecraft/core/component/DataComponentType$Builder";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
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
        load_one(rt, stack, name, &crate::recipes::nbt_json(value));
    }
}

/// [`load`] from a saved stack's `components` (vanilla's patch, as `ItemStack.CODEC` writes it):
/// the mod's own types and the damage; a removal (`!name`) and vanilla's other types are left out.
pub(crate) fn load_saved(rt: &mut Rt, stack: V, patch: &serde_json::Map<String, serde_json::Value>) {
    for (name, value) in patch {
        if name == "minecraft:damage" {
            set_damage(rt, stack, value.as_i64().unwrap_or(0) as i32);
        } else if !name.starts_with('!') {
            load_one(rt, stack, name, value);
        }
    }
}

/// `stack`'s patch as `ItemStack.CODEC` writes it: [`saved`] and its damage, when it has some.
pub(crate) fn patch(rt: &mut Rt, stack: V) -> std::collections::BTreeMap<String, Nbt> {
    let mut out: std::collections::BTreeMap<String, Nbt> = saved(rt, stack).into_iter().collect();
    let n = damage(rt, stack);
    if n > 0 {
        out.insert("minecraft:damage".into(), Nbt::Int(n));
    }
    out
}

fn load_one(rt: &mut Rt, stack: V, name: &str, value: &serde_json::Value) {
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
/// codec: the mod's, and vanilla's enchantments ([`CARRIED_VANILLA`], none when there are no
/// levels). Vanilla's other types, types without a codec and values the codec can't write are left
/// out (the last told once).
pub(crate) fn saved(rt: &mut Rt, stack: V) -> Vec<(String, Nbt)> {
    let mut out = Vec::new();
    for p in pairs(rt, stack).chunks(2) {
        let (ty, value) = (p[0], p[1]);
        let id = rt.str_of(rt.get_field(ty, "$id")).unwrap_or("").to_owned();
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
    let id = rt.str_of(rt.get_field(ty, "$id")).unwrap_or("").to_owned();
    let defaulted = match id.as_str() {
        "minecraft:enchantments" => true,
        "minecraft:stored_enchantments" => crate::api_common::item_like_name(rt, rt.get_field(stack, "$item")) == "minecraft:enchanted_book",
        _ => false,
    };
    (defaulted && stack.r() != 0 && !crate::api_common::stack_empty(rt, stack)).then(|| crate::enchantments::empty(rt))
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
    // `ItemStack.EMPTY` is one shared object: nothing is written onto it.
    let empty = crate::api_common::get_static(rt, ITEM_STACK, "EMPTY").unwrap_or(V::NULL);
    if stack.r() == 0 || stack == empty || ty.r() == 0 {
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

/// `stack.applyComponents(patch)`: what the patch sets is set on the stack, what it removes taken
/// off.
fn apply_patch(rt: &mut Rt, stack: V, patch: V) {
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
    rt.call_functional("java/lang/Object.equals.(Ljava/lang/Object;)Z", x, &[y]).i() != 0
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

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        "net/minecraft/core/component/DataComponents" if !NOT_TYPES.contains(&field) && !field.ends_with("CODEC") => {
            Some(vanilla_type(rt, &format!("minecraft:{}", field.to_ascii_lowercase())))
        }
        MAP if field == "EMPTY" => Some(rt.intern("compat:component-map/empty".into(), |rt| plain_map(rt, Vec::new()))),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    let stack_method = matches!(
        name,
        "isComponentsPatchEmpty" | "getComponentsPatch" | "getDamageValue" | "setDamageValue" | "getMaxDamage" | "isDamageableItem" | "isDamaged" | "hurtAndBreak" | "hurtWithoutBreaking"
    );
    let typed = desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;") || desc.starts_with("(Ljava/util/function/Supplier;");
    let maps = matches!(owner, MAP | MAP_BUILDER | PATCH_BUILDER)
        || owner.ends_with("/PatchedDataComponentMap")
        || (owner == COMPONENT_PATCH && matches!(name, "builder" | "size" | "get" | "entrySet"))
        || (name == "components" && desc == "()Lnet/minecraft/core/component/DataComponentMap;");
    let stack_maps = matches!(name, "getComponents" | "applyComponents" | "applyComponentsAndValidate");
    matches!(owner, TYPE | BUILDER) || maps || ((owner == ITEM_STACK || HOLDERS.contains(&owner)) && (stack_method || stack_maps || typed))
}

pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on component types, their builders, and a stack's components; None for anything else.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
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
        (o, "getComponents") if (o == ITEM_STACK || HOLDERS.contains(&o)) && rt.class_of(a(0)) == ITEM_STACK => Some(stack_view(rt, a(0))),
        // `applyComponents(map)`: each of the map's components set on the stack; `(patch)`: the
        // patch's sets and removals made.
        (ITEM_STACK, "applyComponents") if desc == "(Lnet/minecraft/core/component/DataComponentMap;)V" => {
            let l = set_pairs(rt, a(1));
            for p in l.chunks(2) {
                call(rt, ITEM_STACK, "set", SET_DESC, &[a(0), p[0], p[1]]);
            }
            Some(V::Void)
        }
        // `applyComponentsAndValidate(patch)`: the patch applied (the stack's own checks aside).
        (ITEM_STACK, "applyComponents" | "applyComponentsAndValidate") => {
            apply_patch(rt, a(0), a(1));
            Some(V::Void)
        }
        // `Item.components()`: the item's default components, as a new stack of it reads them.
        (_, "components") if desc == "()Lnet/minecraft/core/component/DataComponentMap;" && rt.instance_of(a(0), "net/minecraft/world/item/Item") => {
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
        (COMPONENT_PATCH, n @ ("size" | "get" | "entrySet")) => patch_call(rt, n, args),
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
        // What was set on the stack, as `ItemStack.CODEC` writes its `components` (empty: the one
        // `EMPTY`).
        // The values themselves stay on it too (a transient type's among them), for a stack it is
        // applied to (`applyComponents(other.getComponentsPatch())`).
        (ITEM_STACK, "getComponentsPatch") => {
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
        (ITEM_STACK, "hurtAndBreak") => {
            crate::hand::hurt(rt, a(0), a(1).i());
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
        (o, _) if (o == ITEM_STACK || HOLDERS.contains(&o)) && rt.class_of(a(0)) == ITEM_STACK => {
            let typed = desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;") || desc.starts_with("(Ljava/util/function/Supplier;");
            if !typed {
                return None;
            }
            let (stack, ty) = (a(0), type_of(rt, a(1)));
            if let Some(v) = damage_component(rt, name, stack, ty, args) {
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
