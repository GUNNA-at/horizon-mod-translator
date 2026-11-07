//! NeoForge's data attachments: `AttachmentType.builder(default)...build()` (and `serializable`),
//! and a holder's `getData`, `setData`, `hasData`, `removeData`, `getExistingData` and
//! `hasAttachments` (entities, block entities, chunks and levels hold attachments; a mod names the
//! type itself or the `DeferredHolder` it registered). Each holder keeps its attachments by type
//! (the field `$attachments`, a map); `getData` makes the default the first time it is asked, as
//! NeoForge's `AttachmentHolder` does, from a supplier or from a function of the holder. They
//! aren't written with the world yet.
use crate::{Data, Rt, V};

const TYPE: &str = "net/neoforged/neoforge/attachment/AttachmentType";
const BUILDER: &str = "net/neoforged/neoforge/attachment/AttachmentType$Builder";
const SUPPLIER: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const FUNCTION: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const HOLDER_METHODS: [&str; 6] = ["getData", "setData", "hasData", "removeData", "getExistingData", "hasAttachments"];

pub fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(owner, TYPE | BUILDER)
        || (HOLDER_METHODS.contains(&name)
            && (owner.starts_with("net/minecraft/") || owner.starts_with("net/neoforged/"))
            && (name == "hasAttachments" || desc.contains("attachment/AttachmentType;") || desc.starts_with("(Ljava/util/function/Supplier;")))
}

/// The attachment type `v` names: the type, or what its holder (`DeferredHolder`, a supplier) gives.
fn attachment_type(rt: &mut Rt, v: V) -> V {
    if v.r() == 0 || rt.class_of(v) == TYPE {
        return v;
    }
    rt.call_functional(SUPPLIER, v, &[])
}

/// The holder's attachments by type, made when `make` and there are none yet.
fn attachments(rt: &mut Rt, holder: V, make: bool) -> Option<V> {
    let t = rt.get_field(holder, "$attachments");
    if t.r() != 0 {
        return Some(t);
    }
    if !make || holder.r() == 0 {
        return None;
    }
    let t = rt.alloc("java/util/IdentityHashMap", Data::Table(Default::default()));
    rt.put_field(holder, "$attachments", t);
    Some(t)
}

fn builder(rt: &mut Rt, default: V, takes_holder: bool, serializable: bool) -> V {
    let b = rt.new_object(BUILDER);
    rt.put_field(b, "$default", default);
    rt.put_field(b, "$takesHolder", V::I(takes_holder as i32));
    rt.put_field(b, "$serializable", V::I(serializable as i32));
    b
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    // A holder's methods of another kind (`Holder.getData(DataMapType)`) are another module's.
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
        (TYPE, "builder" | "serializable") => {
            let takes_holder = desc.starts_with("(Ljava/util/function/Function;");
            Some(builder(rt, a(0), takes_holder, name == "serializable"))
        }
        (BUILDER, "serialize") => {
            rt.put_field(a(0), "$serializer", a(1));
            if desc.contains("Ljava/util/function/Predicate;") {
                rt.put_field(a(0), "$shouldSerialize", a(2));
            }
            Some(a(0))
        }
        (BUILDER, "copyOnDeath") => {
            rt.put_field(a(0), "$copyOnDeath", V::I(1));
            Some(a(0))
        }
        (BUILDER, "copyHandler" | "sync") => {
            let field = if name == "copyHandler" { "$copyHandler" } else { "$sync" };
            rt.put_field(a(0), field, a(1));
            Some(a(0))
        }
        (BUILDER, "build") => {
            let t = rt.new_object(TYPE);
            for f in ["$default", "$takesHolder", "$serializable", "$serializer", "$shouldSerialize", "$copyOnDeath", "$copyHandler", "$sync"] {
                let v = rt.get_field(a(0), f);
                if !matches!(v, V::Void) {
                    rt.put_field(t, f, v);
                }
            }
            Some(t)
        }
        (_, "hasAttachments") => {
            let any = attachments(rt, a(0), false).is_some_and(|t| !crate::jutil::table_pairs(rt, t).is_empty());
            Some(V::I(any as i32))
        }
        (_, "getData") => {
            let (holder, t) = (a(0), attachment_type(rt, a(1)));
            let table = attachments(rt, holder, true)?;
            if let Some(v) = crate::jutil::table_get(rt, table, t) {
                return Some(v);
            }
            let default = rt.get_field(t, "$default");
            let v = if rt.get_field(t, "$takesHolder").i() != 0 {
                rt.call_functional(FUNCTION, default, &[holder])
            } else {
                rt.call_functional(SUPPLIER, default, &[])
            };
            if rt.thrown.is_none() && v.r() != 0 {
                crate::jutil::table_put(rt, table, t, v);
            }
            Some(v)
        }
        (_, "setData") => {
            let (holder, t) = (a(0), attachment_type(rt, a(1)));
            let table = attachments(rt, holder, true)?;
            Some(crate::jutil::table_put(rt, table, t, a(2)))
        }
        (_, "hasData") => {
            let (holder, t) = (a(0), attachment_type(rt, a(1)));
            let has = match attachments(rt, holder, false) {
                Some(table) => crate::jutil::table_get(rt, table, t).is_some(),
                None => false,
            };
            Some(V::I(has as i32))
        }
        (_, "getExistingData") => {
            let (holder, t) = (a(0), attachment_type(rt, a(1)));
            let v = match attachments(rt, holder, false) {
                Some(table) => crate::jutil::table_get(rt, table, t).unwrap_or(V::NULL),
                None => V::NULL,
            };
            Some(crate::api_common::optional(rt, v))
        }
        (_, "removeData") => {
            let (holder, t) = (a(0), attachment_type(rt, a(1)));
            let old = match attachments(rt, holder, false) {
                Some(table) => crate::jutil::table_remove(rt, table, t).unwrap_or(V::NULL),
                None => V::NULL,
            };
            Some(old)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// The mod's side: a default value's supplier, made fresh each time.
    fn mod_code(rt: &mut Rt, _key: &'static str, _: &'static str, args: &[V]) -> V {
        match rt.class_of(args[0]) {
            "test/NewController" => rt.new_object("test/Controller"),
            _ => V::Void,
        }
    }

    #[test]
    fn a_holder_keeps_each_attachment_and_makes_the_default_once() {
        let mut rt = Rt::new();
        rt.vcall = Some(mod_code);
        // Create's `AllAttachmentTypes.MINECART_CONTROLLER`.
        let make = rt.new_object("test/NewController");
        let b = c(&mut rt, "net/neoforged/neoforge/attachment/AttachmentType.builder.(Ljava/util/function/Supplier;)Lnet/neoforged/neoforge/attachment/AttachmentType$Builder;", &[make]);
        let codec = rt.new_object("test/Codec");
        let b = c(&mut rt, "net/neoforged/neoforge/attachment/AttachmentType$Builder.serialize.(Lnet/neoforged/neoforge/attachment/IAttachmentSerializer;)Lnet/neoforged/neoforge/attachment/AttachmentType$Builder;", &[b, codec]);
        let kind = c(&mut rt, "net/neoforged/neoforge/attachment/AttachmentType$Builder.build.()Lnet/neoforged/neoforge/attachment/AttachmentType;", &[b]);
        let cart = rt.new_object("test/Minecart");
        const HAS: &str = "net/minecraft/world/entity/Entity.hasData.(Lnet/neoforged/neoforge/attachment/AttachmentType;)Z";
        const GET: &str = "net/minecraft/world/entity/Entity.getData.(Lnet/neoforged/neoforge/attachment/AttachmentType;)Ljava/lang/Object;";
        assert_eq!(c(&mut rt, HAS, &[cart, kind]), V::I(0));
        let first = c(&mut rt, GET, &[cart, kind]);
        assert_eq!(rt.class_of(first), "test/Controller");
        assert_eq!(c(&mut rt, GET, &[cart, kind]), first, "made once, then kept");
        assert_eq!(c(&mut rt, HAS, &[cart, kind]), V::I(1));
        // Another holder has its own.
        let other = rt.new_object("test/Minecart");
        assert_ne!(c(&mut rt, GET, &[other, kind]), first);
        let replacement = rt.new_object("test/Controller");
        let old = c(&mut rt, "net/minecraft/world/entity/Entity.setData.(Lnet/neoforged/neoforge/attachment/AttachmentType;Ljava/lang/Object;)Ljava/lang/Object;", &[cart, kind, replacement]);
        assert_eq!(old, first);
        let removed = c(&mut rt, "net/minecraft/world/entity/Entity.removeData.(Lnet/neoforged/neoforge/attachment/AttachmentType;)Ljava/lang/Object;", &[cart, kind]);
        assert_eq!(removed, replacement);
        assert_eq!(c(&mut rt, HAS, &[cart, kind]), V::I(0));
    }
}
