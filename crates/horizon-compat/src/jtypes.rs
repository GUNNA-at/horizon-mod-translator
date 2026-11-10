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

/// Guava's `TypeToken`: a type kept as a value, which mods key their tables by (MineColonies'
/// request system finds a factory by the token of what it makes).
const GUAVA_TOKEN: &str = "com/google/common/reflect/TypeToken";
const TYPE_SET: &str = "com/google/common/reflect/TypeToken$TypeSet";

pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        PARAMETERIZED | GENERIC_ARRAY | WILDCARD | VARIABLE | TYPE | GUAVA_TOKEN | TYPE_SET
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

/// The `TypeToken` of a signature: one object for each, so two tokens of a type are the same key.
fn token_of(rt: &mut Rt, sig: &str) -> V {
    let key = format!("compat:type-token/{sig}");
    if let Some(&t) = rt.interned.get(&key) {
        return t;
    }
    let t = rt.new_object(GUAVA_TOKEN);
    let text = rt.string(sig);
    rt.put_field(t, "$signature", text);
    rt.interned.insert(key, t);
    t
}

/// The class above `class` and the interfaces it names: a mod's class by its own definition, the
/// game's by its class file. `Object` is above a class only: an interface's types end at
/// interfaces, as reflection has them (one is told from a class by being named as an interface).
fn parents(rt: &Rt, class: &str) -> Vec<String> {
    let mut up: Vec<String> = Vec::new();
    let named_interface = |rt: &Rt, c: &str| {
        rt.interfaces.values().any(|all| all.contains(&c)) || crate::api::game_interface(c)
    };
    if let Some(s) = rt.supers.get(class) {
        up.push((*s).to_owned());
        up.extend(
            rt.interfaces
                .get(class)
                .copied()
                .unwrap_or_default()
                .iter()
                .map(|i| (*i).to_owned()),
        );
    } else if let Some((above, interfaces)) = crate::api::game_parents(class) {
        // The table names no class above one that is right under `Object`.
        up.push(
            if above.is_empty() {
                "java/lang/Object"
            } else {
                above
            }
            .to_owned(),
        );
        up.extend(interfaces.iter().map(|i| (*i).to_owned()));
    } else if class != "java/lang/Object" {
        up.push("java/lang/Object".to_owned());
    }
    if named_interface(rt, class) {
        up.retain(|c| c != "java/lang/Object");
    }
    up
}

/// Every class and interface `class` is or is under, each before the ones above it, as
/// `TypeToken.getTypes()` lists them (the order among unrelated ones is not promised there).
fn supertypes(rt: &Rt, class: &str) -> Vec<String> {
    let mut all = vec![class.to_owned()];
    let mut above: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let up = parents(rt, &all[i]);
        for c in &up {
            if !all.contains(c) {
                all.push(c.clone());
            }
        }
        above.push(up);
        i += 1;
    }
    // How many types are above each, itself included: one under another has more than it.
    let count = |start: usize| {
        let mut seen = vec![start];
        let mut at = 0;
        while at < seen.len() {
            for c in &above[seen[at]] {
                let j = all.iter().position(|x| x == c).unwrap_or(start);
                if !seen.contains(&j) {
                    seen.push(j);
                }
            }
            at += 1;
        }
        seen.len()
    };
    let mut order: Vec<usize> = (0..all.len()).collect();
    let counts: Vec<usize> = order.iter().map(|&i| count(i)).collect();
    order.sort_by(|a, b| counts[*b].cmp(&counts[*a]));
    order.into_iter().map(|i| all[i].clone()).collect()
}

/// The raw class of a signature: an array's is the array.
fn raw_class(sig: &str) -> String {
    match sig.as_bytes().first() {
        Some(b'[') => sig.to_owned(),
        Some(b'L') => parts(sig).0,
        _ => crate::jlang::descriptor_class(sig).to_owned(),
    }
}

/// `TypeToken` and its `TypeSet`. Types here compare by their raw classes: a token is a subtype
/// of another when its class is under the other's, whatever the two name as type arguments.
fn guava(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    if owner == TYPE_SET {
        let tokens = crate::jutil::elements(rt, this);
        return Some(match name {
            "rawTypes" => {
                let classes: Vec<V> = tokens
                    .iter()
                    .map(|t| {
                        let raw = raw_class(&signature(rt, *t));
                        rt.class_literal(&raw)
                    })
                    .collect();
                crate::jutil::new_set_of(rt, &classes)
            }
            _ => return None,
        });
    }
    let own = |rt: &Rt, v: V| {
        rt.str_of(rt.get_field(v, "$signature"))
            .unwrap_or(OBJECT)
            .to_owned()
    };
    Some(match name {
        "of" => {
            let sig = signature(rt, this);
            token_of(rt, &sig)
        }
        // `new TypeToken<Type>() {}`: the type argument the object's class gives `TypeToken`,
        // found up its classes; a wildcard or a type variable there stands for its bound.
        "<init>" => {
            let mut class = rt.class_of(this).to_owned();
            let mut sig = OBJECT.to_owned();
            loop {
                if let Some((_, up, _)) = class_signature(rt, &class)
                    && up.starts_with("Lcom/google/common/reflect/TypeToken<")
                {
                    sig = parts(&up).1.first().cloned().unwrap_or(sig);
                    break;
                }
                match rt.supers.get(class.as_str()).copied() {
                    Some(up) if up != GUAVA_TOKEN => class = up.to_owned(),
                    _ => break,
                }
            }
            let sig = match sig.as_bytes().first() {
                Some(b'+' | b'-') => sig[1..].to_owned(),
                Some(b'*' | b'T') => OBJECT.to_owned(),
                _ => sig,
            };
            let text = rt.string(&sig);
            rt.put_field(this, "$signature", text);
            V::Void
        }
        "getType" => {
            let sig = own(rt, this);
            of(rt, &sig)
        }
        "getRawType" => {
            let raw = raw_class(&own(rt, this));
            rt.class_literal(&raw)
        }
        // The token itself, then its class's supertypes, each raw.
        "getTypes" => {
            let sig = own(rt, this);
            let mut tokens = vec![this];
            for class in supertypes(rt, &raw_class(&sig)).into_iter().skip(1) {
                tokens.push(token_of(rt, &format!("L{class};")));
            }
            crate::jutil::new_set_of(rt, &tokens)
        }
        "isSubtypeOf" | "isSupertypeOf" => {
            let (mine, theirs) = (raw_class(&own(rt, this)), raw_class(&signature(rt, a(1))));
            let (under, over) = if name == "isSubtypeOf" {
                (mine, theirs)
            } else {
                (theirs, mine)
            };
            V::I(rt.is_a(&under, &over) as i32)
        }
        "isArray" => V::I(own(rt, this).starts_with('[') as i32),
        "getComponentType" => match own(rt, this).strip_prefix('[') {
            Some(of) => token_of(rt, of),
            None => V::NULL,
        },
        "equals" if desc == "(Ljava/lang/Object;)Z" => {
            let other = a(1);
            let same = this == other
                || (other.r() != 0
                    && rt.instance_of(other, GUAVA_TOKEN)
                    && own(rt, this) == own(rt, other));
            V::I(same as i32)
        }
        "hashCode" => V::I(crate::jlang::string_hash(&own(rt, this))),
        "toString" => {
            let text = type_name(&own(rt, this));
            rt.string(&text)
        }
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    if matches!(owner, GUAVA_TOKEN | TYPE_SET) {
        return guava(rt, owner, name, desc, args);
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

    /// Guava's `TypeToken` as MineColonies' request system uses it: one token for a class, so a
    /// table keyed by tokens finds what was put under another token of the class; its types are
    /// the class's supertypes, each before those above it; and one type is under another by
    /// their classes.
    #[test]
    fn a_type_token_is_its_class_and_lists_what_is_above_it() {
        crate::install();
        let mut rt = Rt::new();
        const OF: &str = "com/google/common/reflect/TypeToken.of.(Ljava/lang/Class;)Lcom/google/common/reflect/TypeToken;";
        let level = rt.class_literal("net/minecraft/server/level/ServerLevel");
        let token = call(&mut rt, OF, &[level]);
        assert_eq!(token, call(&mut rt, OF, &[level]), "one token for a class");
        assert_eq!(
            call(
                &mut rt,
                "com/google/common/reflect/TypeToken.getRawType.()Ljava/lang/Class;",
                &[token]
            ),
            level
        );
        let name = call(
            &mut rt,
            "com/google/common/reflect/TypeToken.toString.()Ljava/lang/String;",
            &[token],
        );
        assert_eq!(text(&rt, name), "net.minecraft.server.level.ServerLevel");

        // A table keyed by tokens (`primaryInputMappings.putIfAbsent(factory.getFactoryInputType(), ...)`).
        let table = rt.new_object("java/util/HashMap");
        call(&mut rt, "java/util/HashMap.<init>.()V", &[table]);
        let held = rt.string("held");
        call(
            &mut rt,
            "java/util/Map.putIfAbsent.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[table, token, held],
        );
        let again = call(&mut rt, OF, &[level]);
        assert_eq!(
            call(
                &mut rt,
                "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[table, again]
            ),
            held
        );

        // Its types: itself first, `Level` before the interfaces `Level` names, `Object` among them.
        let types = call(
            &mut rt,
            "com/google/common/reflect/TypeToken.getTypes.()Lcom/google/common/reflect/TypeToken$TypeSet;",
            &[token],
        );
        let names: Vec<String> = crate::jutil::elements(&rt, types)
            .into_iter()
            .map(|t| {
                let n = call(
                    &mut rt,
                    "com/google/common/reflect/TypeToken.toString.()Ljava/lang/String;",
                    &[t],
                );
                text(&rt, n)
            })
            .collect();
        assert_eq!(names[0], "net.minecraft.server.level.ServerLevel");
        let at = |n: &str| {
            names
                .iter()
                .position(|x| x == n)
                .unwrap_or_else(|| panic!("{n} is not among {names:?}"))
        };
        assert!(
            at("net.minecraft.world.level.Level") < at("net.minecraft.world.level.LevelAccessor")
        );
        assert!(
            at("net.minecraft.world.level.LevelAccessor")
                < at("net.minecraft.world.level.LevelReader")
        );
        at("java.lang.Object");
        let raw = call(
            &mut rt,
            "com/google/common/reflect/TypeToken$TypeSet.rawTypes.()Ljava/util/Set;",
            &[types],
        );
        let world = rt.class_literal("net/minecraft/world/level/Level");
        assert_eq!(
            call(
                &mut rt,
                "java/util/Set.contains.(Ljava/lang/Object;)Z",
                &[raw, world]
            ),
            V::I(1)
        );

        // An interface's types end at interfaces.
        let reader = rt.class_literal("net/minecraft/world/level/LevelReader");
        let reader = call(&mut rt, OF, &[reader]);
        let above = call(
            &mut rt,
            "com/google/common/reflect/TypeToken.getTypes.()Lcom/google/common/reflect/TypeToken$TypeSet;",
            &[reader],
        );
        let object = rt.class_literal("java/lang/Object");
        let object = call(&mut rt, OF, &[object]);
        assert_eq!(
            call(
                &mut rt,
                "java/util/Set.contains.(Ljava/lang/Object;)Z",
                &[above, object]
            ),
            V::I(0)
        );

        const UNDER: &str = "com/google/common/reflect/TypeToken.isSubtypeOf.(Lcom/google/common/reflect/TypeToken;)Z";
        let world_token = call(&mut rt, OF, &[world]);
        assert_eq!(call(&mut rt, UNDER, &[token, world_token]), V::I(1));
        assert_eq!(call(&mut rt, UNDER, &[world_token, token]), V::I(0));
        assert_eq!(
            call(
                &mut rt,
                "com/google/common/reflect/TypeToken.isSubtypeOf.(Ljava/lang/reflect/Type;)Z",
                &[token, world]
            ),
            V::I(1)
        );
        assert_eq!(
            call(
                &mut rt,
                "com/google/common/reflect/TypeToken.equals.(Ljava/lang/Object;)Z",
                &[token, again]
            ),
            V::I(1)
        );
        assert_eq!(
            call(
                &mut rt,
                "com/google/common/reflect/TypeToken.equals.(Ljava/lang/Object;)Z",
                &[token, world_token]
            ),
            V::I(0)
        );
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
