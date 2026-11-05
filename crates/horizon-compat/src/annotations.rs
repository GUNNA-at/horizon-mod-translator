//! Annotations as reflection sees them: `isAnnotationPresent`, `getAnnotation` and the rest on a
//! mod's classes, fields and methods, from the bundle's table of the `RUNTIME` ones (generated
//! `ANNOTATIONS`, through [`Rt::reflect`]). Each annotation is an object of its own type (so a
//! call on it reaches [`element`]) answering its elements from the values the class file gives,
//! else from the annotation type's defaults.
//!
//! SecurityCraft registers the block items of its content this way (`@Reinforced(itemGroup = ...)`,
//! `@RegisterItemBlock` on `SCContent`'s fields).
use crate::{Data, Rt, V};

const ANNOTATION_ARRAY: &str = "[Ljava/lang/annotation/Annotation;";
const INHERITED: &str = "java/lang/annotation/Inherited";

/// An annotation of type `ty` (a descriptor) with `values`, its elements as JSON.
fn annotation(rt: &mut Rt, ty: &str, values: &str) -> V {
    let class: &'static str = rt.static_key(ty.strip_prefix('L').and_then(|t| t.strip_suffix(';')).unwrap_or(ty));
    let a = rt.alloc(class, Data::Fields(Default::default()));
    rt.put_field(a, "$annotation", V::I(1));
    let v = rt.string(values);
    rt.put_field(a, "$values", v);
    a
}

/// Class `class`'s annotations by where they are (`TYPE\t`, `FIELD\tname`, `METHOD\tname+desc`),
/// each a list, and its elements' defaults (`DEFAULT\tname`, as JSON); read from the mod's table
/// once. None for a class that isn't the mod's or has none.
fn index(rt: &mut Rt, class: &str) -> Option<V> {
    if !rt.is_mod_class(class) {
        return None;
    }
    let key = format!("compat:annotations/{class}");
    if let Some(&v) = rt.interned.get(&key) {
        return (v.r() != 0).then_some(v);
    }
    let text = rt.reflect(class, "annotations", &[]).and_then(|v| rt.str_of(v).map(str::to_owned));
    let Some(text) = text else {
        rt.interned.insert(key, V::NULL);
        return None;
    };
    let mut lists: Vec<(String, Vec<V>)> = Vec::new();
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let mut p = line.splitn(4, '\t');
        let (Some(target), Some(member), Some(ty), Some(values)) = (p.next(), p.next(), p.next(), p.next()) else { continue };
        if target == "DEFAULT" {
            let v = rt.string(values);
            map.insert(format!("DEFAULT\t{member}"), v);
            continue;
        }
        let a = annotation(rt, ty, values);
        let at = format!("{target}\t{member}");
        match lists.iter_mut().find(|(k, _)| *k == at) {
            Some((_, l)) => l.push(a),
            None => lists.push((at, vec![a])),
        }
    }
    for (at, items) in lists {
        let l = crate::jutil::new_list(rt, items);
        map.insert(at, l);
    }
    let v = rt.alloc("compat:annotations", Data::Map(map));
    rt.interned.insert(key, v);
    Some(v)
}

/// The annotations on the members of `class` whose keys start with `at` (`FIELD\tname`, or
/// `METHOD\tname(params)` for a method of any return type).
fn found(rt: &mut Rt, class: &str, at: &str, exact: bool) -> Vec<V> {
    let Some(index) = index(rt, class) else { return Vec::new() };
    let lists: Vec<V> = match rt.heap.get(index.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) if exact => m.get(at).copied().into_iter().collect(),
        Some(Data::Map(m)) => m.iter().filter(|(k, _)| k.starts_with(at)).map(|(_, v)| *v).collect(),
        _ => Vec::new(),
    };
    lists.into_iter().flat_map(|l| crate::jutil::elements(rt, l)).collect()
}

/// Whether annotation type `ty` is `@Inherited` (a class's subclasses have it too).
fn inherited(rt: &mut Rt, ty: &str) -> bool {
    found(rt, ty, "TYPE\t", true).iter().any(|&a| rt.class_of(a) == INHERITED)
}

/// What class `class` (internal form) carries: its own annotations, and with `inherit` the
/// `@Inherited` ones of its superclasses it doesn't carry itself.
pub(crate) fn on_class(rt: &mut Rt, class: &str, inherit: bool) -> Vec<V> {
    let mut out = found(rt, class, "TYPE\t", true);
    let mut c = class.to_owned();
    while inherit && let Some(&sup) = rt.supers.get(c.as_str()) {
        for a in found(rt, sup, "TYPE\t", true) {
            let ty = rt.class_of(a);
            if !out.iter().any(|&o| rt.class_of(o) == ty) && inherited(rt, ty) {
                out.push(a);
            }
        }
        c = sup.to_owned();
    }
    out
}

/// What field `name` of `class` carries, from the class that declares it.
pub(crate) fn on_field(rt: &mut Rt, class: &str, name: &str) -> Vec<V> {
    let mut c = class.to_owned();
    loop {
        let here = found(rt, &c, &format!("FIELD\t{name}"), true);
        if !here.is_empty() {
            return here;
        }
        match rt.supers.get(c.as_str()) {
            Some(&sup) if rt.is_mod_class(sup) => c = sup.to_owned(),
            _ => return Vec::new(),
        }
    }
}

/// What method `name` with parameter descriptors `params` (`(I)`) of `class` carries, from the
/// class that declares it.
pub(crate) fn on_method(rt: &mut Rt, class: &str, name: &str, params: &str) -> Vec<V> {
    let mut c = class.to_owned();
    loop {
        let here = found(rt, &c, &format!("METHOD\t{name}{params}"), false);
        if !here.is_empty() {
            return here;
        }
        match rt.supers.get(c.as_str()) {
            Some(&sup) if rt.is_mod_class(sup) => c = sup.to_owned(),
            _ => return Vec::new(),
        }
    }
}

/// `isAnnotationPresent`, `getAnnotation` (and the declared forms), `getAnnotations` and
/// `getAnnotationsByType` on an element carrying `annotations`; `ty` the class asked for. None
/// for any other method.
pub(crate) fn query(rt: &mut Rt, name: &str, annotations: Vec<V>, ty: V) -> Option<V> {
    let wanted = rt.str_of(ty).unwrap_or("").to_owned();
    let of_type: Vec<V> = annotations.iter().copied().filter(|&a| rt.class_of(a) == wanted).collect();
    Some(match name {
        "isAnnotationPresent" => V::I(!of_type.is_empty() as i32),
        "getAnnotation" | "getDeclaredAnnotation" => of_type.first().copied().unwrap_or(V::NULL),
        "getAnnotations" | "getDeclaredAnnotations" => rt.alloc(ANNOTATION_ARRAY, Data::Array(annotations)),
        "getAnnotationsByType" | "getDeclaredAnnotationsByType" => {
            let class: &'static str = rt.static_key(&format!("[L{wanted};"));
            rt.alloc(class, Data::Array(of_type))
        }
        _ => return None,
    })
}

/// A call on one of these annotations (its receiver `args[0]`): `annotationType()`, or an
/// element by name, as its return type takes it. None for anything else.
pub(crate) fn element(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let this = args.first().copied()?;
    if !matches!(this, V::R(r) if r != 0) || rt.get_field(this, "$annotation") != V::I(1) {
        return None;
    }
    let (_, name, desc) = crate::parts(key)?;
    let ty = rt.class_of(this);
    let values = rt.str_of(rt.get_field(this, "$values")).unwrap_or("{}").to_owned();
    Some(match name {
        "annotationType" => rt.class_literal(ty),
        "hashCode" => V::I(crate::jlang::string_hash(&values) ^ crate::jlang::string_hash(ty)),
        "equals" => {
            let other = args.get(1).copied().unwrap_or(V::NULL);
            let same = rt.class_of(other) == ty && rt.get_field(other, "$annotation") == V::I(1) && rt.str_of(rt.get_field(other, "$values")) == Some(values.as_str());
            V::I(same as i32)
        }
        "toString" => {
            let s = format!("@{}({values})", ty.replace('/', "."));
            rt.string(&s)
        }
        _ => {
            let ret = desc.rsplit(')').next().unwrap_or("V");
            let parsed: serde_json::Value = serde_json::from_str(&values).unwrap_or_default();
            match parsed.get(name) {
                Some(v) => value(rt, v, ret),
                None => match default(rt, ty, name) {
                    Some(v) => value(rt, &v, ret),
                    None => zero(ret),
                },
            }
        }
    })
}

/// Annotation type `ty`'s default for element `name`, when the mod's class gives one.
fn default(rt: &mut Rt, ty: &str, name: &str) -> Option<serde_json::Value> {
    let index = index(rt, ty)?;
    let s = match rt.heap.get(index.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) => m.get(&format!("DEFAULT\t{name}")).copied()?,
        _ => return None,
    };
    serde_json::from_str(rt.str_of(s)?).ok()
}

/// What an element of type `desc` holds when nothing gives it a value.
fn zero(desc: &str) -> V {
    match desc.as_bytes().first() {
        Some(b'J') => V::J(0),
        Some(b'F') => V::F(0.0),
        Some(b'D') => V::D(0.0),
        Some(b'Z' | b'I' | b'B' | b'C' | b'S') => V::I(0),
        _ => V::NULL,
    }
}

/// An element value (typed as the translator writes it: a string, `{"e": desc, "v": name}` an
/// enum constant, `{"c": desc}` a class, `{"I": 1}` a primitive, an array, `{"@": desc, "v":
/// {...}}` an annotation) as a value of type `desc`.
fn value(rt: &mut Rt, v: &serde_json::Value, desc: &str) -> V {
    use serde_json::Value;
    match v {
        Value::String(s) => rt.string(s),
        Value::Array(items) => {
            let (class, inner) = match desc.strip_prefix('[') {
                Some(inner) => (desc, inner),
                None => ("[Ljava/lang/Object;", "Ljava/lang/Object;"),
            };
            let class: &'static str = rt.static_key(class);
            let vals: Vec<V> = items.iter().map(|i| value(rt, i, inner)).collect();
            rt.alloc(class, Data::Array(vals))
        }
        Value::Object(m) => {
            if let (Some(Value::String(ty)), Some(Value::String(name))) = (m.get("e"), m.get("v")) {
                let class = ty.strip_prefix('L').and_then(|t| t.strip_suffix(';')).unwrap_or(ty);
                return crate::jlang::static_of(rt, class, name);
            }
            if let Some(Value::String(d)) = m.get("c") {
                return rt.class_literal(crate::jlang::descriptor_class(d));
            }
            if let Some(Value::String(ty)) = m.get("@") {
                let values = m.get("v").map_or_else(|| "{}".to_owned(), |v| v.to_string());
                return annotation(rt, ty, &values);
            }
            let Some((kind, n)) = m.iter().next() else { return V::NULL };
            match kind.as_str() {
                "J" => V::J(n.as_i64().unwrap_or(0)),
                "F" => V::F(n.as_f64().unwrap_or(0.0) as f32),
                "D" => V::D(n.as_f64().unwrap_or(0.0)),
                _ => V::I(n.as_i64().unwrap_or(0) as i32),
            }
        }
        _ => zero(desc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REINFORCED: &str = "net/geforcemods/securitycraft/util/Reinforced";
    const GROUP: &str = "net/geforcemods/securitycraft/util/SCItemGroup";
    const CONTENT: &str = "net/geforcemods/securitycraft/SCContent";

    /// A bundle's reflection, as the translator writes it for SecurityCraft's `SCContent` (two
    /// fields, one `@Reinforced` with an item group and one with its defaults), the annotation
    /// type's defaults, and its item group enum.
    fn reflect(rt: &mut Rt, class: &str, what: &str, _args: &[V]) -> Option<V> {
        let text = match (class, what) {
            (CONTENT, "annotations") => concat!(
                "FIELD\tREINFORCED_STONE\tLnet/geforcemods/securitycraft/util/Reinforced;\t{\"itemGroup\":{\"e\":\"Lnet/geforcemods/securitycraft/util/SCItemGroup;\",\"v\":\"DECORATION\"}}\n",
                "FIELD\tREINFORCED_GLASS\tLnet/geforcemods/securitycraft/util/Reinforced;\t{\"hasReinforcedTint\":{\"Z\":0},\"tint\":{\"I\":16777215},\"names\":[\"a\",\"b\"]}\n",
                "TYPE\t\tLnet/geforcemods/securitycraft/util/Marked;\t{}\n",
                "METHOD\tsetup(I)V\tLnet/geforcemods/securitycraft/util/Marked;\t{}\n",
            ),
            (REINFORCED, "annotations") => concat!(
                "TYPE\t\tLjava/lang/annotation/Retention;\t{\"value\":{\"e\":\"Ljava/lang/annotation/RetentionPolicy;\",\"v\":\"RUNTIME\"}}\n",
                "DEFAULT\tregisterBlockItem\t\t{\"Z\":1}\n",
                "DEFAULT\thasReinforcedTint\t\t{\"Z\":1}\n",
                "DEFAULT\titemGroup\t\t{\"e\":\"Lnet/geforcemods/securitycraft/util/SCItemGroup;\",\"v\":\"MANUAL\"}\n",
            ),
            _ => return None,
        };
        Some(rt.string(text))
    }

    fn content() -> Rt {
        let mut rt = Rt::default();
        rt.reflect = Some(reflect);
        for c in [CONTENT, REINFORCED, GROUP, "net/geforcemods/securitycraft/util/Marked", "compat:test/Sub"] {
            rt.supers.insert(c, "java/lang/Object");
        }
        rt.supers.insert("compat:test/Sub", CONTENT);
        rt.initialized.insert(GROUP);
        for (i, n) in ["MANUAL", "DECORATION"].into_iter().enumerate() {
            let k = rt.static_key(&format!("{GROUP}.{n}"));
            let e = rt.new_object(GROUP);
            rt.put_field(e, "$ordinal", V::I(i as i32));
            rt.statics.insert(k, e);
        }
        rt
    }

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    #[test]
    fn a_fields_annotation_answers_its_values_and_its_types_defaults() {
        let mut rt = content();
        let class = rt.class_literal(CONTENT);
        let reinforced = rt.class_literal(REINFORCED);
        let name = rt.string("REINFORCED_STONE");
        let stone = c(&mut rt, "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;", &[class, name]);
        assert_eq!(c(&mut rt, "java/lang/reflect/Field.isAnnotationPresent.(Ljava/lang/Class;)Z", &[stone, reinforced]), V::I(1));
        let a = c(&mut rt, "java/lang/reflect/Field.getAnnotation.(Ljava/lang/Class;)Ljava/lang/annotation/Annotation;", &[stone, reinforced]);
        assert_eq!(rt.class_of(a), REINFORCED);
        // Given: the enum constant it names. Not given: the type's default.
        let group = c(&mut rt, "net/geforcemods/securitycraft/util/Reinforced.itemGroup.()Lnet/geforcemods/securitycraft/util/SCItemGroup;", &[a]);
        let decoration = rt.static_key(&format!("{GROUP}.DECORATION"));
        assert_eq!(group, rt.statics[decoration]);
        assert_eq!(c(&mut rt, "net/geforcemods/securitycraft/util/Reinforced.registerBlockItem.()Z", &[a]), V::I(1));
        let ty = c(&mut rt, "java/lang/annotation/Annotation.annotationType.()Ljava/lang/Class;", &[a]);
        assert_eq!(ty, reinforced);
        // The other field: primitives and an array as given, the enum default.
        let name = rt.string("REINFORCED_GLASS");
        let glass = c(&mut rt, "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;", &[class, name]);
        let a = c(&mut rt, "java/lang/reflect/Field.getAnnotation.(Ljava/lang/Class;)Ljava/lang/annotation/Annotation;", &[glass, reinforced]);
        assert_eq!(c(&mut rt, "net/geforcemods/securitycraft/util/Reinforced.hasReinforcedTint.()Z", &[a]), V::I(0));
        assert_eq!(c(&mut rt, "net/geforcemods/securitycraft/util/Reinforced.tint.()I", &[a]), V::I(0xffffff));
        let names = c(&mut rt, "net/geforcemods/securitycraft/util/Reinforced.names.()[Ljava/lang/String;", &[a]);
        assert_eq!(rt.class_of(names), "[Ljava/lang/String;");
        let names: Vec<String> = crate::jutil::elements(&rt, names).iter().map(|&n| rt.str_of(n).unwrap_or("").to_owned()).collect();
        assert_eq!(names, ["a", "b"]);
        let manual = rt.static_key(&format!("{GROUP}.MANUAL"));
        let group = c(&mut rt, "net/geforcemods/securitycraft/util/Reinforced.itemGroup.()Lnet/geforcemods/securitycraft/util/SCItemGroup;", &[a]);
        assert_eq!(group, rt.statics[manual]);
        // A field without one, and a class without any.
        let name = rt.string("OTHER");
        let other = c(&mut rt, "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;", &[class, name]);
        assert_eq!(c(&mut rt, "java/lang/reflect/Field.isAnnotationPresent.(Ljava/lang/Class;)Z", &[other, reinforced]), V::I(0));
        assert_eq!(c(&mut rt, "java/lang/reflect/Field.getAnnotation.(Ljava/lang/Class;)Ljava/lang/annotation/Annotation;", &[other, reinforced]), V::NULL);
        let all = c(&mut rt, "java/lang/reflect/Field.getAnnotations.()[Ljava/lang/annotation/Annotation;", &[other]);
        assert!(crate::jutil::elements(&rt, all).is_empty());
    }

    #[test]
    fn classes_and_methods_carry_theirs_and_only_inherited_ones_pass_down() {
        let mut rt = content();
        let class = rt.class_literal(CONTENT);
        let sub = rt.class_literal("compat:test/Sub");
        let marked = rt.class_literal("net/geforcemods/securitycraft/util/Marked");
        assert_eq!(c(&mut rt, "java/lang/Class.isAnnotationPresent.(Ljava/lang/Class;)Z", &[class, marked]), V::I(1));
        // `Marked` isn't `@Inherited`: the subclass doesn't have it.
        assert_eq!(c(&mut rt, "java/lang/Class.isAnnotationPresent.(Ljava/lang/Class;)Z", &[sub, marked]), V::I(0));
        let all = c(&mut rt, "java/lang/Class.getAnnotations.()[Ljava/lang/annotation/Annotation;", &[class]);
        assert_eq!(crate::jutil::elements(&rt, all).len(), 1);
        let name = rt.string("setup");
        let int = rt.class_literal("int");
        let params = rt.alloc("[Ljava/lang/Class;", Data::Array(vec![int]));
        let m = c(&mut rt, "java/lang/Class.getDeclaredMethod.(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;", &[class, name, params]);
        assert_eq!(c(&mut rt, "java/lang/reflect/Method.isAnnotationPresent.(Ljava/lang/Class;)Z", &[m, marked]), V::I(1));
        let none = rt.alloc("[Ljava/lang/Class;", Data::Array(Vec::new()));
        let m = c(&mut rt, "java/lang/Class.getDeclaredMethod.(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;", &[class, name, none]);
        assert_eq!(c(&mut rt, "java/lang/reflect/Method.isAnnotationPresent.(Ljava/lang/Class;)Z", &[m, marked]), V::I(0));
    }
}
