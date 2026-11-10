//! `java.lang.reflect.Type` beside `Class`: a parameterized type, an array of one, a wildcard, a
//! type variable. Each is what a generic signature of the class file spells (`Ljava/util/List<
//! Lcom/example/Entry;>;`), which the translator keeps for a class, its fields and its record
//! components: `field.getGenericType()`, `getClass().getGenericSuperclass()` and Gson's
//! `new TypeToken<List<Entry>>() {}.getType()` all read one, and whoever takes a `Type` (Gson, a
//! config library) reads the signature back ([`signature`]).
//!
//! One object for each signature, so two types that spell the same are the same object.
use crate::{Rt, V};

pub(crate) const PARAMETERIZED: &str = "java/lang/reflect/ParameterizedType";
pub(crate) const GENERIC_ARRAY: &str = "java/lang/reflect/GenericArrayType";
pub(crate) const WILDCARD: &str = "java/lang/reflect/WildcardType";
pub(crate) const VARIABLE: &str = "java/lang/reflect/TypeVariable";
const TYPE: &str = "java/lang/reflect/Type";
const OBJECT: &str = "Ljava/lang/Object;";

pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        PARAMETERIZED | GENERIC_ARRAY | WILDCARD | VARIABLE | TYPE
    )
}

/// Where the type signature that starts at `at` ends (a type argument's `*`, `+` and `-` too).
fn end_of(sig: &str, at: usize) -> usize {
    let b = sig.as_bytes();
    match b.get(at) {
        Some(b'[' | b'+' | b'-') => end_of(sig, at + 1),
        Some(b'T') => sig[at..].find(';').map_or(sig.len(), |i| at + i + 1),
        Some(b'L') => {
            let mut depth = 0usize;
            for (i, c) in b.iter().enumerate().skip(at) {
                match c {
                    b'<' => depth += 1,
                    b'>' => depth = depth.saturating_sub(1),
                    b';' if depth == 0 => return i + 1,
                    _ => {}
                }
            }
            sig.len()
        }
        Some(_) => at + 1,
        None => at,
    }
}

/// The signatures written one after another in `list` (type arguments, a class's supertypes).
fn each(list: &str) -> Vec<String> {
    let (mut out, mut at) = (Vec::new(), 0);
    while at < list.len() {
        let end = end_of(list, at).max(at + 1).min(list.len());
        out.push(list[at..end].to_owned());
        at = end;
    }
    out
}

/// A class type's signature apart: its class (`java/util/Map$Entry`, an inner class's `.` as the
/// `$` its name has) and the type arguments of its last part, each as written (`*`, `+L...;`).
pub(crate) fn parts(sig: &str) -> (String, Vec<String>) {
    let inner = sig
        .strip_prefix('L')
        .and_then(|s| s.strip_suffix(';'))
        .unwrap_or(sig);
    let (mut class, mut args, mut depth, mut from) = (String::new(), Vec::new(), 0usize, 0usize);
    for (i, c) in inner.char_indices() {
        match c {
            '<' => {
                if depth == 0 {
                    from = i + 1;
                }
                depth += 1;
            }
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    args = each(&inner[from..i]);
                }
            }
            '.' if depth == 0 => {
                class.push('$');
                args.clear();
            }
            c if depth == 0 => class.push(c),
            _ => {}
        }
    }
    (class, args)
}

/// Whether a signature says more than a class does: it has type arguments or a type variable
/// somewhere.
fn generic(sig: &str) -> bool {
    let element = sig.trim_start_matches('[');
    element.contains('<') || element.starts_with('T')
}

/// The `Type` a signature spells: the class for one that says no more than a class, else one of
/// the four kinds, the same object each time.
pub(crate) fn of(rt: &mut Rt, sig: &str) -> V {
    let class = match sig.as_bytes().first() {
        Some(b'*' | b'+' | b'-') => WILDCARD,
        Some(b'T') => VARIABLE,
        Some(b'[') if generic(sig) => GENERIC_ARRAY,
        Some(b'L') if generic(sig) => PARAMETERIZED,
        // A class, a primitive, an array of either.
        Some(b'[') => return rt.class_literal(sig),
        _ => return rt.class_literal(crate::jlang::descriptor_class(sig)),
    };
    let key = format!("compat:type/{sig}");
    if let Some(v) = rt.interned.get(&key) {
        return *v;
    }
    let ty = rt.new_object(class);
    let text = rt.string(sig);
    rt.put_field(ty, "$signature", text);
    rt.interned.insert(key, ty);
    ty
}

/// The signature a `Type` spells: a class's descriptor, one of this module's types' own, a
/// wildcard's bound (what it stands for where a value is made), `Object` for nothing. Gson's
/// `TypeToken` keeps its type the same way, so one reads as its type.
pub(crate) fn signature(rt: &Rt, ty: V) -> String {
    if ty.r() == 0 {
        return OBJECT.to_owned();
    }
    if let Some(sig) = rt.str_of(rt.get_field(ty, "$signature")) {
        return match sig.as_bytes().first() {
            Some(b'*') => OBJECT.to_owned(),
            Some(b'+' | b'-') => sig[1..].to_owned(),
            _ => sig.to_owned(),
        };
    }
    match rt.str_of(ty) {
        Some(name) => crate::jlang::descriptor_of(name),
        None => OBJECT.to_owned(),
    }
}

/// `Type.getTypeName()` of what a signature spells: `java.util.List<com.example.Entry>`,
/// `? extends java.lang.Number`, `T[]`.
pub(crate) fn type_name(sig: &str) -> String {
    match sig.as_bytes().first() {
        Some(b'*') => "?".to_owned(),
        Some(b'+') if &sig[1..] == OBJECT => "?".to_owned(),
        Some(b'+') => format!("? extends {}", type_name(&sig[1..])),
        Some(b'-') => format!("? super {}", type_name(&sig[1..])),
        Some(b'T') => sig[1..].trim_end_matches(';').to_owned(),
        Some(b'[') => format!("{}[]", type_name(&sig[1..])),
        Some(b'L') => {
            let (class, args) = parts(sig);
            let name = class.replace('/', ".");
            if args.is_empty() {
                name
            } else {
                let args: Vec<String> = args.iter().map(|a| type_name(a)).collect();
                format!("{name}<{}>", args.join(", "))
            }
        }
        _ => crate::jlang::descriptor_class(sig).to_owned(),
    }
}

/// A class's own signature apart: its type parameters (each with its bounds), its superclass and
/// its interfaces, as written. None for a class that carries none (it isn't generic and extends
/// nothing that is, or it isn't a mod's).
pub(crate) fn class_signature(
    rt: &mut Rt,
    class: &str,
) -> Option<(Vec<(String, Vec<String>)>, String, Vec<String>)> {
    if !rt.is_mod_class(class) {
        return None;
    }
    let fields = rt.reflect(class, "fields", &[])?;
    let lines = rt.str_of(fields)?.to_owned();
    let sig = lines.lines().find_map(|l| l.strip_prefix("0 <class> "))?;
    let mut parameters = Vec::new();
    let mut rest = sig;
    if let Some(formals) = sig.strip_prefix('<') {
        // `Name:Bound:Bound...` one after another, up to the `>` that closes them.
        let mut at = 0;
        while at < formals.len() && !formals[at..].starts_with('>') {
            let colon = at + formals[at..].find(':')?;
            let name = formals[at..colon].to_owned();
            let mut bounds = Vec::new();
            at = colon;
            while formals[at..].starts_with(':') {
                at += 1;
                // A class bound may be left out (`T::Ljava/lang/Comparable<TT;>;`).
                if formals[at..].starts_with(':') {
                    continue;
                }
                let end = end_of(formals, at);
                bounds.push(formals[at..end].to_owned());
                at = end;
            }
            parameters.push((name, bounds));
        }
        rest = formals.get(at + 1..)?;
    }
    let mut supertypes = each(rest).into_iter();
    let superclass = supertypes.next()?;
    Some((parameters, superclass, supertypes.collect()))
}

/// An array of `Type`s for signatures.
fn types(rt: &mut Rt, sigs: &[String]) -> V {
    let all: Vec<V> = sigs.iter().map(|s| of(rt, s)).collect();
    rt.alloc("[Ljava/lang/reflect/Type;", crate::Data::Array(all))
}

/// `Class.getGenericSuperclass()`: the superclass with its type arguments where the class names
/// any; None for the plain superclass (the caller's own answer).
pub(crate) fn generic_superclass(rt: &mut Rt, class: &str) -> Option<V> {
    let (_, superclass, _) = class_signature(rt, class)?;
    Some(of(rt, &superclass))
}

/// `Class.getGenericInterfaces()`, where the class's signature names them.
pub(crate) fn generic_interfaces(rt: &mut Rt, class: &str) -> Option<V> {
    let (_, _, interfaces) = class_signature(rt, class)?;
    Some(types(rt, &interfaces))
}

/// `Class.getTypeParameters()`: the class's type variables, none for a class that has none.
pub(crate) fn type_parameters(rt: &mut Rt, class: &str) -> V {
    let names: Vec<String> = class_signature(rt, class)
        .map(|s| {
            s.0.into_iter()
                .map(|(name, _)| format!("T{name};"))
                .collect()
        })
        .unwrap_or_default();
    let all: Vec<V> = names.iter().map(|s| of(rt, s)).collect();
    rt.alloc("[Ljava/lang/reflect/TypeVariable;", crate::Data::Array(all))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let this = args.first().copied().unwrap_or_default();
    // A `Class` asked as a `Type` (`type.getTypeName()`): its name, an array's with `[]`.
    let Some(sig) = rt
        .str_of(rt.get_field(this, "$signature"))
        .map(str::to_owned)
    else {
        return match (name, rt.str_of(this)) {
            ("getTypeName", Some(class)) if rt.class_of(this) == "java/lang/Class" => {
                let text = type_name(&crate::jlang::descriptor_of(class));
                Some(rt.string(&text))
            }
            _ => None,
        };
    };
    let other = args.get(1).copied().unwrap_or_default();
    Some(match name {
        "getTypeName" | "toString" => {
            let text = type_name(&sig);
            rt.string(&text)
        }
        "equals" => V::I(
            (this == other || rt.str_of(rt.get_field(other, "$signature")) == Some(sig.as_str()))
                as i32,
        ),
        "hashCode" => V::I(crate::jlang::string_hash(&sig)),
        "getActualTypeArguments" => {
            let (_, args) = parts(&sig);
            types(rt, &args)
        }
        "getRawType" => {
            let (class, _) = parts(&sig);
            rt.class_literal(&class)
        }
        // The class a nested class is declared in; none for a top-level one.
        "getOwnerType" => match parts(&sig).0.rsplit_once('$') {
            Some((outer, _)) => rt.class_literal(outer),
            None => V::NULL,
        },
        "getGenericComponentType" => of(rt, &sig[1..]),
        "getUpperBounds" => {
            let bound = sig.strip_prefix('+').unwrap_or(OBJECT).to_owned();
            types(rt, &[bound])
        }
        "getLowerBounds" => {
            let bounds: Vec<String> = sig
                .strip_prefix('-')
                .map(str::to_owned)
                .into_iter()
                .collect();
            types(rt, &bounds)
        }
        "getName" => {
            let text = type_name(&sig);
            rt.string(&text)
        }
        // A type variable's bounds are its declaration's, which a signature that only names the
        // variable doesn't carry: `Object`, as an unbounded one's.
        "getBounds" => types(rt, &[OBJECT.to_owned()]),
        "getGenericDeclaration" => V::NULL,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    fn text(rt: &Rt, v: V) -> String {
        rt.str_of(v).unwrap_or("?").to_owned()
    }

    /// A signature read as `java.lang.reflect` shows it: a map of lists apart into its raw class
    /// and arguments, a wildcard's bounds, an array's component, the names Java prints.
    #[test]
    fn a_signature_is_the_type_java_reflects() {
        crate::install();
        let mut rt = Rt::new();
        let map = of(
            &mut rt,
            "Ljava/util/Map<Ljava/lang/String;Ljava/util/List<+Ljava/lang/Number;>;>;",
        );
        assert_eq!(rt.class_of(map), PARAMETERIZED);
        assert_eq!(
            map,
            of(
                &mut rt,
                "Ljava/util/Map<Ljava/lang/String;Ljava/util/List<+Ljava/lang/Number;>;>;"
            ),
            "one object for a type"
        );
        let name = call(
            &mut rt,
            "java/lang/reflect/Type.getTypeName.()Ljava/lang/String;",
            &[map],
        );
        assert_eq!(
            text(&rt, name),
            "java.util.Map<java.lang.String, java.util.List<? extends java.lang.Number>>"
        );
        let raw = call(
            &mut rt,
            "java/lang/reflect/ParameterizedType.getRawType.()Ljava/lang/reflect/Type;",
            &[map],
        );
        assert_eq!(raw, rt.class_literal("java/util/Map"));
        let args = call(
            &mut rt,
            "java/lang/reflect/ParameterizedType.getActualTypeArguments.()[Ljava/lang/reflect/Type;",
            &[map],
        );
        assert_eq!(rt.array_len(args), 2);
        assert_eq!(rt.array_get(args, 0), rt.class_literal("java/lang/String"));
        let list = rt.array_get(args, 1);
        let inner = call(
            &mut rt,
            "java/lang/reflect/ParameterizedType.getActualTypeArguments.()[Ljava/lang/reflect/Type;",
            &[list],
        );
        let wildcard = rt.array_get(inner, 0);
        assert_eq!(rt.class_of(wildcard), WILDCARD);
        let upper = call(
            &mut rt,
            "java/lang/reflect/WildcardType.getUpperBounds.()[Ljava/lang/reflect/Type;",
            &[wildcard],
        );
        assert_eq!(rt.array_get(upper, 0), rt.class_literal("java/lang/Number"));
        let lower = call(
            &mut rt,
            "java/lang/reflect/WildcardType.getLowerBounds.()[Ljava/lang/reflect/Type;",
            &[wildcard],
        );
        assert_eq!(rt.array_len(lower), 0);
        assert_eq!(
            signature(&rt, wildcard),
            "Ljava/lang/Number;",
            "a wildcard stands for its bound"
        );
        // A nested class's owner, an array of a parameterized type, a type variable.
        let entry = of(
            &mut rt,
            "Ljava/util/Map$Entry<TK;[Ljava/util/List<Ljava/lang/String;>;>;",
        );
        let owner = call(
            &mut rt,
            "java/lang/reflect/ParameterizedType.getOwnerType.()Ljava/lang/reflect/Type;",
            &[entry],
        );
        assert_eq!(owner, rt.class_literal("java/util/Map"));
        let name = call(
            &mut rt,
            "java/lang/reflect/Type.getTypeName.()Ljava/lang/String;",
            &[entry],
        );
        assert_eq!(
            text(&rt, name),
            "java.util.Map$Entry<K, java.util.List<java.lang.String>[]>"
        );
        let args = call(
            &mut rt,
            "java/lang/reflect/ParameterizedType.getActualTypeArguments.()[Ljava/lang/reflect/Type;",
            &[entry],
        );
        assert_eq!(rt.class_of(rt.array_get(args, 0)), VARIABLE);
        let array = rt.array_get(args, 1);
        assert_eq!(rt.class_of(array), GENERIC_ARRAY);
        let component = call(
            &mut rt,
            "java/lang/reflect/GenericArrayType.getGenericComponentType.()Ljava/lang/reflect/Type;",
            &[array],
        );
        assert_eq!(rt.class_of(component), PARAMETERIZED);
        // What says no more than a class is the class.
        assert_eq!(
            of(&mut rt, "Ljava/lang/String;"),
            rt.class_literal("java/lang/String")
        );
        assert_eq!(of(&mut rt, "I"), rt.class_literal("int"));
        assert_eq!(
            of(&mut rt, "[Ljava/lang/String;"),
            rt.class_literal("[Ljava/lang/String;")
        );
        let list = rt.class_literal("java/util/List");
        assert_eq!(signature(&rt, list), "Ljava/util/List;");
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// A class's own signature: its type variables with their bounds, its superclass and its
    /// interfaces with their arguments.
    #[test]
    fn a_classes_signature_names_its_supertypes_arguments() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            match (class, what) {
                ("t/Box", "fields") => Some(rt.string(
                    "0 <class> <T:Ljava/lang/Number;U::Ljava/lang/Comparable<TU;>;>Lt/Base<TT;>;Ljava/util/function/Supplier<Ljava/util/List<TU;>;>;\n2 held Ljava/lang/Object; TT;",
                )),
                ("t/Token", "fields") => Some(rt.string("0 <class> Lt/Base<Ljava/util/List<Ljava/lang/String;>;>;")),
                _ => None,
            }
        }
        let mut rt = Rt::new();
        for class in ["t/Box", "t/Token", "t/Plain"] {
            rt.supers.insert(class, "t/Base");
        }
        rt.reflect = Some(reflect);
        let (parameters, superclass, interfaces) = class_signature(&mut rt, "t/Box").unwrap();
        assert_eq!(
            parameters,
            [
                ("T".to_owned(), vec!["Ljava/lang/Number;".to_owned()]),
                (
                    "U".to_owned(),
                    vec!["Ljava/lang/Comparable<TU;>;".to_owned()]
                )
            ]
        );
        assert_eq!(superclass, "Lt/Base<TT;>;");
        assert_eq!(
            interfaces,
            ["Ljava/util/function/Supplier<Ljava/util/List<TU;>;>;"]
        );
        let up = generic_superclass(&mut rt, "t/Token").unwrap();
        assert_eq!(
            type_name(&signature(&rt, up)),
            "t.Base<java.util.List<java.lang.String>>"
        );
        let variables = type_parameters(&mut rt, "t/Box");
        assert_eq!(rt.array_len(variables), 2);
        assert!(
            class_signature(&mut rt, "t/Plain").is_none()
                && generic_superclass(&mut rt, "java/util/ArrayList").is_none()
        );
    }
}
