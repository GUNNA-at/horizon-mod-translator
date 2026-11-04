//! Item data components, natively. This covers the component types a mod registers
//! (`DataComponentType.builder().persistent(codec).build()`, NeoForge's
//! `DeferredRegister.DataComponents` too), vanilla's `DataComponents`, and a stack's own components:
//! `has/get/getOrDefault/set/remove/update`, including NeoForge's forms that take the type's holder.
//!
//! A stack keeps what was set on it (vanilla's component patch) in `$patch`, as (type, value)
//! pairs, and its copies keep them too. Not modelled yet: an item's default components (vanilla's
//! prototype: `get(MAX_STACK_SIZE)` is null), and components in a saved stack (that needs encoding
//! through the type's codec).
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

/// `to` gets `from`'s components (a copy of the stack keeps them).
pub(crate) fn copy_patch(rt: &mut Rt, from: V, to: V) {
    let l = pairs(rt, from);
    if !l.is_empty() {
        store(rt, to, l);
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
    if pa.len() != pb.len() {
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

pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
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
        (o, _) if (o == ITEM_STACK || HOLDERS.contains(&o)) && rt.class_of(a(0)) == ITEM_STACK => {
            let typed = desc.starts_with("(Lnet/minecraft/core/component/DataComponentType;") || desc.starts_with("(Ljava/util/function/Supplier;");
            if !typed {
                return None;
            }
            let (stack, ty) = (a(0), type_of(rt, a(1)));
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
}
