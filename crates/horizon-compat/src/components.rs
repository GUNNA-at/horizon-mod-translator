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
//! `components` map. Not modelled yet: an item's default components (vanilla's prototype:
//! `get(MAX_STACK_SIZE)` is null), and vanilla's own types on a game stack.
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
/// `DataComponents` statics that aren't component types.
const NOT_TYPES: [&str; 2] = ["COMMON_ITEM_COMPONENTS", "ITEM_STACK_CODEC"];

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
    rt.resolve_registrations();
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
/// codec. Vanilla's types, types without a codec and values the codec can't write are left out (the
/// last told once).
pub(crate) fn saved(rt: &mut Rt, stack: V) -> Vec<(String, Nbt)> {
    let mut out = Vec::new();
    for p in pairs(rt, stack).chunks(2) {
        let (ty, value) = (p[0], p[1]);
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
/// hand, else its mod item's `durability`; 0 for none.
pub(crate) fn max_damage(rt: &Rt, stack: V) -> i32 {
    if stack.r() == 0 {
        return 0;
    }
    if let V::I(n) = rt.get_field(stack, "$maxDamage") {
        return n;
    }
    let item = rt.get_field(stack, "$item");
    let props = if item.r() != 0 { rt.get_field(item, "$properties") } else { V::NULL };
    if props.r() == 0 { 0 } else { rt.get_field(props, "$durability").i() }
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
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    let stack_method = matches!(
        name,
        "isComponentsPatchEmpty" | "getDamageValue" | "setDamageValue" | "getMaxDamage" | "isDamageableItem" | "isDamaged" | "hurtAndBreak" | "hurtWithoutBreaking"
    );
    let typed = desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;") || desc.starts_with("(Ljava/util/function/Supplier;");
    matches!(owner, TYPE | BUILDER) || ((owner == ITEM_STACK || HOLDERS.contains(&owner)) && (stack_method || typed))
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
                "has" => V::I(get(rt, stack, ty).is_some() as i32),
                "get" => get(rt, stack, ty).unwrap_or(V::NULL),
                "getOrDefault" => get(rt, stack, ty).unwrap_or(a(2)),
                "set" => put(rt, stack, ty, a(2)),
                "remove" => put(rt, stack, ty, V::NULL),
                // (type, default, UnaryOperator) or (type, default, argument, BiFunction): the
                // operator's result is set, and the value before it returned, as vanilla does.
                "update" => {
                    let current = get(rt, stack, ty).unwrap_or(a(2));
                    let next = if desc.contains("Ljava/util/function/UnaryOperator;") {
                        rt.call_functional("java/util/function/UnaryOperator.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(3), &[current])
                    } else {
                        rt.call_functional("java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", a(4), &[current, a(3)])
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
    }
}
