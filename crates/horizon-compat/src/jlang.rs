//! `java.lang` in native Rust: `String` (indexes and hashes in UTF-16 code units, as Java
//! counts them), `StringBuilder`, the boxed number classes, `Character`, `Math` and `Object`.
use crate::{Data, Rt, V};

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn from16(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn text(rt: &Rt, v: V) -> String {
    rt.str_of(v).map(str::to_owned).unwrap_or_else(|| rt.to_java_string(v, 'L'))
}

/// `Iterable.forEach(action)` (or fastutil's typed form) on `this`, a mod's object: its own
/// `iterator()`, each element given to `action` as the action's type takes it.
fn for_each(rt: &mut Rt, this: V, action: V, desc: &str) {
    let (next, accept): (&'static str, &'static str) = match desc {
        "(Lit/unimi/dsi/fastutil/longs/LongConsumer;)V" => ("it/unimi/dsi/fastutil/longs/LongIterator.nextLong.()J", "it/unimi/dsi/fastutil/longs/LongConsumer.accept.(J)V"),
        "(Ljava/util/function/LongConsumer;)V" => ("it/unimi/dsi/fastutil/longs/LongIterator.nextLong.()J", "java/util/function/LongConsumer.accept.(J)V"),
        "(Lit/unimi/dsi/fastutil/ints/IntConsumer;)V" => ("it/unimi/dsi/fastutil/ints/IntIterator.nextInt.()I", "it/unimi/dsi/fastutil/ints/IntConsumer.accept.(I)V"),
        "(Ljava/util/function/IntConsumer;)V" => ("it/unimi/dsi/fastutil/ints/IntIterator.nextInt.()I", "java/util/function/IntConsumer.accept.(I)V"),
        _ => ("java/util/Iterator.next.()Ljava/lang/Object;", "java/util/function/Consumer.accept.(Ljava/lang/Object;)V"),
    };
    let it = rt.call_functional("java/lang/Iterable.iterator.()Ljava/util/Iterator;", this, &[]);
    while it.r() != 0 && rt.thrown.is_none() && rt.call_functional("java/util/Iterator.hasNext.()Z", it, &[]).i() != 0 && rt.thrown.is_none() {
        let e = rt.call_functional(next, it, &[]);
        if rt.thrown.is_some() {
            return;
        }
        rt.call_functional(accept, action, &[e]);
    }
}

fn new_str(rt: &mut Rt, s: String) -> V {
    rt.alloc("java/lang/String", Data::Str(s))
}

/// A `Class` object for `name` (internal form, `a/b/C`).
pub fn class_object(rt: &mut Rt, name: &str) -> V {
    rt.class_literal(name)
}

/// An enum class's constants (`Class.getEnumConstants`): a mod enum's `values()`, or a vanilla
/// enum's as the API answers it (`Direction.Axis.values()`). None for a class that isn't one.
pub(crate) fn enum_constants(rt: &mut Rt, class: &str) -> Option<V> {
    if let Some(v) = rt.reflect(class, "values", &[]) {
        return Some(v);
    }
    let key = rt.static_key(&format!("{class}.values.()[L{class};"));
    let before = rt.unmapped.get(key).copied().unwrap_or(0);
    let v = crate::api::call(rt, key, &[]);
    if rt.unmapped.get(key).copied().unwrap_or(0) != before {
        // Not an enum the API knows: the miss isn't the mod's.
        match rt.unmapped.get_mut(key) {
            Some(n) if *n > 1 => *n -= 1,
            _ => {
                rt.unmapped.remove(key);
            }
        }
        return None;
    }
    (v.r() != 0).then_some(v)
}

/// Whether `Class.forName` finds `name` (internal form) outside the mod: the JDK's, the game's and
/// the loader's classes, and the libraries the game ships.
fn platform_class(name: &str) -> bool {
    [
        "java/", "javax/", "jdk/", "sun/", "net/minecraft/", "com/mojang/", "net/neoforged/", "org/slf4j/", "org/apache/",
        "com/google/", "it/unimi/", "io/netty/", "org/joml/", "org/lwjgl/",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

/// Throws a new `class` with `msg`; the value a throwing call leaves (null).
pub(crate) fn throw_with(rt: &mut Rt, class: &'static str, msg: &str) -> V {
    let e = rt.new_object(class);
    let m = rt.string(msg);
    rt.put_field(e, "message", m);
    rt.thrown = Some(e);
    V::NULL
}

/// The descriptor of a class named as `Class.getName` does (`int`, `[I`, `a/b/C`).
fn descriptor_of(name: &str) -> String {
    match name {
        "int" => "I".into(),
        "long" => "J".into(),
        "boolean" => "Z".into(),
        "byte" => "B".into(),
        "char" => "C".into(),
        "short" => "S".into(),
        "float" => "F".into(),
        "double" => "D".into(),
        "void" => "V".into(),
        n if n.starts_with('[') => n.replace('.', "/"),
        n => format!("L{};", n.replace('.', "/")),
    }
}

/// The fields class `class` declares, as the mod's table lists them: (access, name, descriptor).
/// None for a class that isn't the mod's.
fn declared_fields(rt: &mut Rt, class: &str) -> Option<Vec<(u16, String, String)>> {
    if !rt.is_mod_class(class) {
        return None;
    }
    let Some(v) = rt.reflect(class, "fields", &[]) else { return Some(Vec::new()) };
    let lines = rt.str_of(v).unwrap_or("").to_owned();
    Some(
        lines
            .lines()
            .filter_map(|l| {
                let mut p = l.splitn(3, ' ');
                Some((p.next()?.parse().ok()?, p.next()?.to_owned(), p.next()?.to_owned()))
            })
            .collect(),
    )
}

/// The class a field descriptor names, as `Class.getName` has it (`I` is `int`, `Lfoo/Bar;`
/// `foo/Bar`, an array its descriptor).
fn descriptor_class(desc: &str) -> &str {
    match desc {
        "I" => "int",
        "J" => "long",
        "Z" => "boolean",
        "B" => "byte",
        "C" => "char",
        "S" => "short",
        "F" => "float",
        "D" => "double",
        "V" => "void",
        d if d.starts_with('L') && d.ends_with(';') => &d[1..d.len() - 1],
        d => d,
    }
}

/// Field `name`'s access and descriptor, from class `class` or the nearest superclass declaring it.
fn find_field(rt: &mut Rt, class: &str, name: &str) -> Option<(u16, String)> {
    let mut c = class.to_owned();
    loop {
        if let Some((access, _, desc)) = declared_fields(rt, &c)?.into_iter().find(|f| f.1 == name) {
            return Some((access, desc));
        }
        c = rt.supers.get(c.as_str())?.to_string();
    }
}

/// `Class.getDeclaredFields` (`all` false) or `getFields` (the public ones of the class and its
/// superclasses): `Field`s of each, declared by the class that declares it. None for a class that
/// isn't the mod's.
fn fields_array(rt: &mut Rt, class: &str, all: bool) -> Option<V> {
    let mut out = Vec::new();
    let mut c = class.to_owned();
    loop {
        let declared = declared_fields(rt, &c)?;
        let owner = rt.class_literal(&c);
        for (access, name, _) in declared {
            if !all || access & 0x0001 != 0 {
                let n = rt.string(&name);
                out.push(field_object(rt, owner, n));
            }
        }
        match rt.supers.get(c.as_str()) {
            Some(&sup) if all && rt.is_mod_class(sup) => c = sup.to_owned(),
            _ => break,
        }
    }
    Some(rt.alloc("[Ljava/lang/reflect/Field;", Data::Array(out)))
}

/// A `Field` for field `name` (a string) of class object `class`.
fn field_object(rt: &mut Rt, class: V, name: V) -> V {
    let f = rt.new_object("java/lang/reflect/Field");
    rt.put_field(f, "$class", class);
    rt.put_field(f, "$name", name);
    f
}

/// The class (internal form) and name of a `Field`, and its static's key.
fn field_parts(rt: &mut Rt, field: V) -> (String, &'static str, &'static str) {
    let class = text(rt, rt.get_field(field, "$class")).replace('.', "/");
    let name = text(rt, rt.get_field(field, "$name"));
    let key = rt.static_key(&format!("{class}.{name}"));
    (class, rt.static_key(&name), key)
}

/// What `field` holds in `obj`, or, given no object, its class's static: a mod class's once its
/// class is initialised, else the API's.
fn field_value(rt: &mut Rt, field: V, obj: V) -> V {
    let (class, name, key) = field_parts(rt, field);
    if obj.r() != 0 {
        return rt.get_field(obj, name);
    }
    if rt.is_mod_class(&class) {
        rt.reflect(&class, "<clinit>", &[]);
        return rt.statics.get(key).copied().unwrap_or(V::NULL);
    }
    crate::api::get_static(rt, key)
}

/// `field = v` in `obj`, or the class's static given no object; a box set into a field holding
/// a primitive is unboxed.
fn field_store(rt: &mut Rt, field: V, obj: V, v: V) {
    let (class, name, key) = field_parts(rt, field);
    let old = if obj.r() != 0 { rt.get_field(obj, name) } else { rt.statics.get(key).copied().unwrap_or(V::Void) };
    let v = match old {
        V::I(_) => crate::mixin::unbox(rt, v, 'I'),
        V::J(_) => crate::mixin::unbox(rt, v, 'J'),
        V::F(_) => crate::mixin::unbox(rt, v, 'F'),
        V::D(_) => crate::mixin::unbox(rt, v, 'D'),
        _ => v,
    };
    if obj.r() != 0 {
        rt.put_field(obj, name, v);
    } else {
        if rt.is_mod_class(&class) {
            rt.reflect(&class, "<clinit>", &[]);
        }
        rt.statics.insert(key, v);
    }
}

/// A primitive as `Object` (its box); anything else as it is, nothing as null.
fn box_raw(rt: &mut Rt, v: V) -> V {
    match v {
        V::I(_) => crate::mixin::box_value(rt, v, 'I'),
        V::J(_) => crate::mixin::box_value(rt, v, 'J'),
        V::F(_) => crate::mixin::box_value(rt, v, 'F'),
        V::D(_) => crate::mixin::box_value(rt, v, 'D'),
        V::Void => V::NULL,
        v => v,
    }
}

/// The kind of what a method with descriptor `desc` returns (`I`, `L`, `V`...).
fn return_kind(desc: &str) -> char {
    desc.rsplit(')').next().and_then(|r| r.chars().next()).unwrap_or('V')
}

/// A `MethodHandle` of kind `getter`, `setter` or `method` reaching `target` (a `Field` or `Method`).
fn method_handle(rt: &mut Rt, kind: &str, target: V) -> V {
    let h = rt.new_object("java/lang/invoke/MethodHandle");
    let k = rt.string(kind);
    rt.put_field(h, "$kind", k);
    rt.put_field(h, "$target", target);
    h
}

/// `method.invoke(obj, args)` on one of the mod's objects: the instance method its class has by
/// that name and parameter types, given its arguments unboxed, its result boxed. None (a miss) for
/// a static method or one no mod class has.
fn invoke_method(rt: &mut Rt, method: V, obj: V, args: &[V]) -> Option<V> {
    let class = text(rt, rt.get_field(method, "$class")).replace('.', "/");
    let name = text(rt, rt.get_field(method, "$name"));
    let params = rt.get_field(method, "$params");
    let mut prefix = format!("{name}.(");
    for p in crate::jutil::elements(rt, params) {
        prefix.push_str(&descriptor_of(rt.str_of(p).unwrap_or("java/lang/Object")));
    }
    prefix.push(')');
    if obj.r() == 0 {
        return None;
    }
    let short = rt.vkeys.keys().copied().find(|k| k.starts_with(&prefix))?;
    let mut all = Vec::with_capacity(args.len());
    for (v, k) in args.iter().zip(kinds(&short[short.find('(').unwrap_or(0)..])) {
        all.push(crate::mixin::unbox(rt, *v, k));
    }
    let key = rt.static_key(&format!("{class}.{short}"));
    let r = rt.call_functional(key, obj, &all);
    Some(match return_kind(short) {
        'V' => V::NULL,
        'L' | '[' => r,
        _ => box_raw(rt, r),
    })
}

/// `new C(args)` by reflection: the mod's constructor with descriptor `desc`.
fn construct(rt: &mut Rt, class: &str, desc: &str, args: &[V]) -> V {
    match rt.reflect(class, &format!("<init>.{desc}"), args) {
        Some(o) => o,
        None if rt.thrown.is_some() => V::NULL,
        None => throw_with(rt, "java/lang/NoSuchMethodException", &format!("{}.<init>{desc}", class.replace('/', "."))),
    }
}

// `ServiceLoader`: the implementations `META-INF/services` names (translated with the
// mod), each made once with its no-argument constructor, in the file's order.
const PROVIDER: &str = "java/util/ServiceLoader$Provider";

fn service_loader(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "load" | "loadInstalled" => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            let names = match rt.reflect(&n, "services", &[]) {
                Some(v) => v,
                None => rt.new_array(0, V::NULL),
            };
            let l = rt.new_object("java/util/ServiceLoader");
            rt.put_field(l, "$names", names);
            l
        }
        "reload" => {
            rt.put_field(a(0), "$providers", V::NULL);
            V::Void
        }
        // A provider of `stream()`: its class, and the instance (made with the loader's others).
        "get" => rt.get_field(a(0), "$value"),
        "type" => rt.get_field(a(0), "$type"),
        "findFirst" | "iterator" | "forEach" | "stream" => {
            let providers = service_providers(rt, a(0));
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            match name {
                "findFirst" => {
                    let o = rt.new_object("java/util/Optional");
                    rt.put_field(o, "value", providers.first().copied().unwrap_or(V::NULL));
                    o
                }
                "iterator" => crate::jutil::new_iter(rt, providers),
                "stream" => {
                    let mut wrapped = Vec::with_capacity(providers.len());
                    for p in providers {
                        let w = rt.new_object(PROVIDER);
                        rt.put_field(w, "$value", p);
                        let class = rt.class_of(p).to_owned();
                        let class = class_object(rt, &class);
                        rt.put_field(w, "$type", class);
                        wrapped.push(w);
                    }
                    rt.alloc("java/util/stream/Stream", Data::Stream(wrapped))
                }
                _ => {
                    for p in providers {
                        rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[p]);
                        if rt.thrown.is_some() {
                            break;
                        }
                    }
                    V::Void
                }
            }
        }
        _ => return None,
    })
}

/// A `ServiceLoader`'s providers, made on first use and kept, as the JDK's loader caches them.
fn service_providers(rt: &mut Rt, loader: V) -> Vec<V> {
    let made = rt.get_field(loader, "$providers");
    if made.r() != 0 {
        return crate::jutil::elements(rt, made);
    }
    let names: Vec<String> = crate::jutil::elements(rt, rt.get_field(loader, "$names")).iter().map(|&n| text(rt, n)).collect();
    let mut out = Vec::new();
    for n in names {
        let p = construct(rt, &n, "()V", &[]);
        if rt.thrown.is_some() {
            return out;
        }
        out.push(p);
    }
    let list = crate::jutil::new_list(rt, out.clone());
    rt.put_field(loader, "$providers", list);
    out
}

/// The JVM's kind letter of each parameter in `desc`.
pub(crate) fn kinds(desc: &str) -> Vec<char> {
    let mut out = Vec::new();
    let params = desc.strip_prefix('(').and_then(|d| d.split(')').next()).unwrap_or("");
    let mut it = params.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            'L' => {
                for d in it.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
                out.push('L');
            }
            '[' => {
                while it.peek() == Some(&'[') {
                    it.next();
                }
                if it.next() == Some('L') {
                    for d in it.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                out.push('L');
            }
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner.starts_with("java/lang/")
        || owner.starts_with("java/text/")
        || owner.starts_with("org/apache/commons/lang3/")
        || owner.ends_with("Pair")
        || owner.ends_with("Triple")
        || matches!(
            owner,
            "java/util/ServiceLoader"
                | PROVIDER
                | "org/apache/logging/log4j/Logger"
                | "org/slf4j/Logger"
                | "java/util/logging/Logger"
                | "com/mojang/logging/LogUtils"
                | "joptsimple/internal/Strings"
                | "net/minecraft/Util"
                | "org/apache/logging/log4j/LogManager"
                | "org/apache/logging/log4j/MarkerManager"
                | "org/slf4j/LoggerFactory"
        )
}

/// A `java.lang` call (`owner.name.desc`), or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    if let Some(v) = libs(rt, key, args) {
        return Some(v);
    }
    let (owner, name, desc) = crate::parts(key)?;
    if owner == "java/util/ServiceLoader" || owner == PROVIDER {
        return service_loader(rt, name, args);
    }
    if !owner.starts_with("java/lang/") {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ks = kinds(desc);
    match owner {
        "java/lang/String" | "java/lang/CharSequence" => string(rt, name, desc, &ks, args),
        "java/lang/StringBuilder" | "java/lang/StringBuffer" | "java/lang/AbstractStringBuilder" => builder(rt, name, &ks, args),
        "java/lang/Integer" | "java/lang/Long" | "java/lang/Short" | "java/lang/Byte" | "java/lang/Double" | "java/lang/Float" | "java/lang/Number" => number(rt, owner, name, desc, &ks, args),
        // One runtime runs on one thread, so a thread-local is one value, made by its supplier on
        // the first `get` (`ThreadLocal.withInitial(ArrayList::new)`).
        "java/lang/ThreadLocal" | "java/lang/InheritableThreadLocal" => Some(match name {
            "withInitial" => {
                let t = rt.new_object("java/lang/ThreadLocal");
                rt.put_field(t, "$supplier", a(0));
                t
            }
            "<init>" => V::Void,
            "get" => {
                if rt.get_field(a(0), "$set").i() == 0 {
                    let supplier = rt.get_field(a(0), "$supplier");
                    let v = if supplier.r() != 0 { rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", supplier, &[]) } else { V::NULL };
                    rt.put_field(a(0), "$value", v);
                    rt.put_field(a(0), "$set", V::I(1));
                }
                rt.get_field(a(0), "$value")
            }
            "set" => {
                rt.put_field(a(0), "$value", a(1));
                rt.put_field(a(0), "$set", V::I(1));
                V::Void
            }
            "remove" => {
                rt.put_field(a(0), "$set", V::I(0));
                V::Void
            }
            _ => return None,
        }),
        // A `Cleaner`'s action runs when its `Cleanable` is cleaned, once: what a cleaner watches
        // stays while the runtime holds it, so it is never found unreachable first.
        "java/lang/ref/Cleaner" => Some(match name {
            "create" => rt.new_object("java/lang/ref/Cleaner"),
            "register" => {
                let c = rt.new_object("java/lang/ref/Cleaner$Cleanable");
                rt.put_field(c, "$action", a(2));
                c
            }
            _ => return None,
        }),
        "java/lang/ref/Cleaner$Cleanable" => Some(match name {
            "clean" => {
                let action = rt.get_field(a(0), "$action");
                if action.r() != 0 {
                    rt.put_field(a(0), "$action", V::NULL);
                    rt.call_functional("java/lang/Runnable.run.()V", action, &[]);
                }
                V::Void
            }
            _ => return None,
        }),
        // Nothing here is collected while a reference is held, so a weak or soft reference keeps
        // what it was given until cleared.
        "java/lang/ref/WeakReference" | "java/lang/ref/SoftReference" | "java/lang/ref/Reference" => Some(match name {
            "<init>" => {
                rt.put_field(a(0), "$referent", a(1));
                V::Void
            }
            "get" => match rt.get_field(a(0), "$referent") {
                V::Void => V::NULL,
                v => v,
            },
            "clear" => {
                rt.put_field(a(0), "$referent", V::NULL);
                V::Void
            }
            "refersTo" => V::I((rt.get_field(a(0), "$referent") == a(1) || (a(1) == V::NULL && rt.get_field(a(0), "$referent") == V::Void)) as i32),
            "enqueue" | "isEnqueued" => V::I(0),
            _ => return None,
        }),
        "java/lang/Boolean" => Some(match name {
            "parseBoolean" => V::I(rt.str_of(a(0)).is_some_and(|s| s.eq_ignore_ascii_case("true")) as i32),
            "toString" if desc.starts_with("(Z)") => {
                let s = (a(0).i() != 0).to_string();
                rt.string(&s)
            }
            "compare" => V::I(a(0).i().cmp(&a(1).i()) as i32),
            "hashCode" if desc.starts_with("(Z)") => V::I(if a(0).i() != 0 { 1231 } else { 1237 }),
            "hashCode" => V::I(if bool_value(rt, a(0)) == Some(1) { 1231 } else { 1237 }),
            // Either side may be a boolean held raw (a method reference's result), as
            // `booleanValue` takes it.
            "equals" => {
                let (x, y) = (bool_value(rt, a(0)), bool_value(rt, a(1)));
                V::I((x.is_some() && x == y) as i32)
            }
            "logicalAnd" => V::I(a(0).i() & a(1).i()),
            "logicalOr" => V::I(a(0).i() | a(1).i()),
            "logicalXor" => V::I(a(0).i() ^ a(1).i()),
            _ => return None,
        }),
        "java/lang/Character" => {
            let c = char::from_u32(a(0).i() as u32).unwrap_or('\0');
            Some(match name {
                "isDigit" => V::I(c.is_ascii_digit() as i32),
                "isLetter" | "isAlphabetic" => V::I(c.is_alphabetic() as i32),
                "isLetterOrDigit" => V::I(c.is_alphanumeric() as i32),
                "isWhitespace" | "isSpaceChar" => V::I(c.is_whitespace() as i32),
                "isUpperCase" => V::I(c.is_uppercase() as i32),
                "isLowerCase" => V::I(c.is_lowercase() as i32),
                "toUpperCase" => V::I(c.to_uppercase().next().unwrap_or(c) as i32),
                "toLowerCase" => V::I(c.to_lowercase().next().unwrap_or(c) as i32),
                "getNumericValue" => V::I(c.to_digit(36).map_or(-1, |d| d as i32)),
                "valueOf" => {
                    let o = rt.new_object("java/lang/Character");
                    rt.put_field(o, "value", a(0));
                    o
                }
                "charValue" => rt.get_field(a(0), "value"),
                "toString" => {
                    let c = if ks.is_empty() { char::from_u32(rt.get_field(a(0), "value").i() as u32).unwrap_or('\0') } else { c };
                    rt.string(&c.to_string())
                }
                _ => return None,
            })
        }
        "java/lang/Math" | "java/lang/StrictMath" => math(name, &ks, args),
        "java/lang/Object" => Some(match name {
            "getClass" => {
                let c = rt.class_of(a(0));
                rt.class_literal(c)
            }
            // Object's own: `super.hashCode()` from an override must not come back to it.
            "hashCode" => V::I(crate::jutil::hash_own(rt, a(0))),
            "equals" => V::I(crate::jutil::equals_own(rt, a(0), a(1)) as i32),
            "toString" => {
                let s = text(rt, a(0));
                new_str(rt, s)
            }
            // A library interface's default `forEach` on a mod's own iterable (electroenergetics'
            // `ChunkCuboidEntry implements LongIterable`): the object's `iterator()`, walked.
            "forEach" if desc.ends_with(")V") && ks.len() == 1 => {
                for_each(rt, a(0), a(1), desc);
                V::Void
            }
            _ => return None,
        }),
        "java/lang/System" => Some(match name {
            "currentTimeMillis" => V::J(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)),
            "nanoTime" => {
                static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                V::J(START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as i64)
            }
            "identityHashCode" => V::I(a(0).r() as i32),
            "arraycopy" => {
                let (src, sp, dst, dp, n) = (a(0), a(1).i().max(0) as usize, a(2), a(3).i().max(0) as usize, a(4).i().max(0) as usize);
                let part: Vec<V> = match rt.heap.get(src.r() as usize).map(|o| &o.data) {
                    Some(Data::Array(v)) => v.iter().skip(sp).take(n).copied().collect(),
                    _ => return Some(V::Void),
                };
                if let Some(Data::Array(d)) = rt.heap.get_mut(dst.r() as usize).map(|o| &mut o.data) {
                    for (i, v) in part.into_iter().enumerate() {
                        if let Some(slot) = d.get_mut(dp + i) {
                            *slot = v;
                        }
                    }
                }
                V::Void
            }
            "getProperty" | "getenv" => V::NULL,
            "lineSeparator" => rt.string("\n"),
            _ => return None,
        }),
        "java/lang/Enum" => Some(match name {
            "name" | "toString" => crate::api_common::enum_name(rt, a(0), name == "toString"),
            "compareTo" => V::I(crate::api_common::enum_ordinal(rt, a(0)) - crate::api_common::enum_ordinal(rt, a(1))),
            "hashCode" => V::I(a(0).r() as i32),
            "equals" => V::I((a(0) == a(1)) as i32),
            "getDeclaringClass" => {
                let c = rt.class_of(a(0));
                rt.class_literal(c)
            }
            _ => return None,
        }),
        "java/lang/Class" => Some(match name {
            // `Class.forName`: the mod's classes (initialised unless asked not to) and the
            // platform's; anything else is absent, as an optional dependency that isn't installed.
            "forName" => {
                let n = text(rt, a(0)).replace('.', "/");
                if rt.is_mod_class(&n) {
                    if ks.len() == 1 || a(1).i() != 0 {
                        rt.reflect(&n, "<clinit>", &[]);
                        if rt.thrown.is_some() {
                            return Some(V::NULL);
                        }
                    }
                    class_object(rt, &n)
                } else if platform_class(&n) {
                    class_object(rt, &n)
                } else {
                    throw_with(rt, "java/lang/ClassNotFoundException", &n.replace('/', "."))
                }
            }
            "newInstance" => {
                let n = rt.str_of(a(0)).unwrap_or("").to_owned();
                construct(rt, &n, "()V", &[])
            }
            "getConstructor" | "getDeclaredConstructor" => {
                let mut d = String::from("(");
                for p in crate::jutil::elements(rt, a(1)) {
                    d.push_str(&descriptor_of(rt.str_of(p).unwrap_or("java/lang/Object")));
                }
                d.push_str(")V");
                let c = rt.new_object("java/lang/reflect/Constructor");
                rt.put_field(c, "$class", a(0));
                let d = rt.string(&d);
                rt.put_field(c, "$desc", d);
                c
            }
            // A field or method by name: what it reaches is looked for when it is used.
            "getDeclaredField" | "getField" => field_object(rt, a(0), a(1)),
            "getDeclaredFields" | "getFields" => {
                let n = rt.str_of(a(0)).unwrap_or("").to_owned();
                fields_array(rt, &n, name == "getFields")?
            }
            // An array class (`int[].class`, `[I`) and what it holds; the primitives' own.
            "isArray" => V::I(rt.str_of(a(0)).is_some_and(|n| n.starts_with('[')) as i32),
            "getComponentType" => match rt.str_of(a(0)).and_then(|n| n.strip_prefix('[')).map(|d| descriptor_class(d).to_owned()) {
                Some(c) => rt.class_literal(&c),
                None => V::NULL,
            },
            "isPrimitive" => {
                let n = rt.str_of(a(0)).unwrap_or("");
                V::I(matches!(n, "int" | "long" | "boolean" | "byte" | "char" | "short" | "float" | "double" | "void") as i32)
            }
            "getDeclaredMethod" | "getMethod" => {
                let m = rt.new_object("java/lang/reflect/Method");
                rt.put_field(m, "$class", a(0));
                rt.put_field(m, "$name", a(1));
                rt.put_field(m, "$params", a(2));
                m
            }
            "getEnumConstants" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                enum_constants(rt, &n).unwrap_or(V::NULL)
            }
            "isEnum" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                V::I(enum_constants(rt, &n).is_some() as i32)
            }
            "getSuperclass" => {
                let n = rt.str_of(a(0)).unwrap_or("").to_owned();
                match rt.supers.get(n.as_str()).copied() {
                    Some(sup) => class_object(rt, sup),
                    None if n == "java/lang/Object" => V::NULL,
                    None => class_object(rt, "java/lang/Object"),
                }
            }
            "isInstance" => {
                let target = rt.str_of(a(0)).unwrap_or("").to_owned();
                V::I(rt.instance_of(a(1), &target) as i32)
            }
            "isAssignableFrom" => {
                let (this, other) = (rt.str_of(a(0)).unwrap_or("").to_owned(), rt.str_of(a(1)).unwrap_or("").to_owned());
                V::I(rt.is_a(&other, &this) as i32)
            }
            "cast" => a(1),
            // The loader a class came from: one for the mod's classes, which `ServiceLoader.load`
            // and `Class.forName(name, init, loader)` take.
            "getClassLoader" => rt.intern("compat:class-loader".into(), |rt| rt.new_object("java/lang/ClassLoader")),
            // `asSubclass(type)`: the class itself when it is one, as JEI checks its plugins.
            "asSubclass" => {
                let (this, other) = (rt.str_of(a(0)).unwrap_or("").to_owned(), rt.str_of(a(1)).unwrap_or("").to_owned());
                if rt.is_a(&this, &other) {
                    a(0)
                } else {
                    throw_with(rt, "java/lang/ClassCastException", &format!("class {} cannot be cast to {}", this.replace('/', "."), other.replace('/', ".")))
                }
            }
            "desiredAssertionStatus" => V::I(0),
            "hashCode" => V::I(string_hash(rt.str_of(a(0)).unwrap_or(""))),
            "equals" => V::I((rt.str_of(a(0)) == rt.str_of(a(1))) as i32),
            "isInterface" | "isRecord" => V::I(0),
            _ => return None,
        }),
        "java/lang/reflect/Constructor" => Some(match name {
            "newInstance" => {
                let class = rt.get_field(a(0), "$class");
                let n = rt.str_of(class).unwrap_or("").to_owned();
                let d = text(rt, rt.get_field(a(0), "$desc"));
                // Reflection unboxes what a primitive parameter is given, as Java's does.
                let mut vals = crate::jutil::elements(rt, a(1));
                for (v, k) in vals.iter_mut().zip(kinds(&d)) {
                    *v = crate::mixin::unbox(rt, *v, k);
                }
                construct(rt, &n, &d, &vals)
            }
            "getDeclaringClass" => rt.get_field(a(0), "$class"),
            "setAccessible" => V::Void,
            "trySetAccessible" => V::I(1),
            _ => return None,
        }),
        // A field a mod looks up by name: reading it reads the object's field, or with no object
        // the class's static (the class initialised first); `get` boxes a primitive, `set` unboxes
        // into a field that holds one, as Java's do.
        "java/lang/reflect/Field" => Some(match name {
            "getName" => rt.get_field(a(0), "$name"),
            // What a mod class's field holds and its modifiers, as the class declares it.
            "getType" => {
                let (class, name, _) = field_parts(rt, a(0));
                let (_, desc) = find_field(rt, &class, name)?;
                rt.class_literal(descriptor_class(&desc))
            }
            "getModifiers" => {
                let (class, name, _) = field_parts(rt, a(0));
                let (access, _) = find_field(rt, &class, name)?;
                V::I(i32::from(access & 0x00df))
            }
            "getDeclaringClass" => rt.get_field(a(0), "$class"),
            "setAccessible" => V::Void,
            "trySetAccessible" | "canAccess" => V::I(1),
            "get" => {
                let v = field_value(rt, a(0), a(1));
                box_raw(rt, v)
            }
            "getInt" | "getLong" | "getFloat" | "getDouble" | "getBoolean" | "getByte" | "getShort" | "getChar" => {
                let v = field_value(rt, a(0), a(1));
                crate::mixin::unbox(rt, v, return_kind(desc))
            }
            "set" | "setInt" | "setLong" | "setFloat" | "setDouble" | "setBoolean" | "setByte" | "setShort" | "setChar" => {
                field_store(rt, a(0), a(1), a(2));
                V::Void
            }
            "hashCode" => {
                let n = text(rt, rt.get_field(a(0), "$name"));
                V::I(string_hash(&n))
            }
            "equals" => {
                let same = |rt: &Rt, f: &'static str| text(rt, rt.get_field(a(0), f)) == text(rt, rt.get_field(a(1), f));
                V::I((rt.class_of(a(1)) == "java/lang/reflect/Field" && same(rt, "$class") && same(rt, "$name")) as i32)
            }
            _ => return None,
        }),
        // A method a mod looks up by name and parameter types: invoked on one of its objects, the
        // method its class has by that name and those parameters.
        "java/lang/reflect/Method" => Some(match name {
            "getName" => rt.get_field(a(0), "$name"),
            "getDeclaringClass" => rt.get_field(a(0), "$class"),
            "setAccessible" => V::Void,
            "trySetAccessible" | "canAccess" => V::I(1),
            "getParameterCount" => {
                let params = rt.get_field(a(0), "$params");
                V::I(crate::jutil::elements(rt, params).len() as i32)
            }
            "invoke" => {
                let vals = crate::jutil::elements(rt, a(2));
                return invoke_method(rt, a(0), a(1), &vals);
            }
            _ => return None,
        }),
        "java/lang/invoke/MethodHandles" => Some(match name {
            "lookup" | "publicLookup" | "privateLookupIn" => rt.intern("compat:lookup".into(), |rt| rt.new_object("java/lang/invoke/MethodHandles$Lookup")),
            _ => return None,
        }),
        // Handles to fields and methods (Mekanism's computer methods are getters of its own
        // fields): a handle keeps what it reaches, and invoking it reads, writes or calls that.
        "java/lang/invoke/MethodHandles$Lookup" => Some(match name {
            "in" | "dropLookupMode" => a(0),
            "unreflectGetter" => method_handle(rt, "getter", a(1)),
            "unreflectSetter" => method_handle(rt, "setter", a(1)),
            "unreflect" => method_handle(rt, "method", a(1)),
            "findGetter" | "findStaticGetter" | "findSetter" | "findStaticSetter" => {
                let f = field_object(rt, a(1), a(2));
                method_handle(rt, if name.contains("Getter") { "getter" } else { "setter" }, f)
            }
            _ => return None,
        }),
        "java/lang/invoke/MethodHandle" => Some(match name {
            "invoke" | "invokeExact" | "invokeWithArguments" => {
                let rest: Vec<V> = if name == "invokeWithArguments" { crate::jutil::elements(rt, a(1)) } else { args[1..].to_vec() };
                let kind = text(rt, rt.get_field(a(0), "$kind"));
                let target = rt.get_field(a(0), "$target");
                let r = match kind.as_str() {
                    "getter" => field_value(rt, target, rest.first().copied().unwrap_or(V::NULL)),
                    "setter" => {
                        let (obj, v) = match rest.as_slice() {
                            [v] => (V::NULL, *v),
                            [obj, v, ..] => (*obj, *v),
                            [] => (V::NULL, V::NULL),
                        };
                        field_store(rt, target, obj, v);
                        V::Void
                    }
                    _ => {
                        let (obj, vals) = rest.split_first().map_or((V::NULL, &[][..]), |(o, r)| (*o, r));
                        return invoke_method(rt, target, obj, vals);
                    }
                };
                // The call site's type: an object where it returns one (a box), else the value.
                if matches!(return_kind(desc), 'L' | '[') { box_raw(rt, r) } else { crate::mixin::unbox(rt, r, return_kind(desc)) }
            }
            _ => return None,
        }),

        // One runtime runs on one game thread, so it is the same `Thread` each time it asks: ESL's
        // transactions (Create: New Age's energy) check `this.thread != Thread.currentThread()`.
        "java/lang/Thread" => Some(match name {
            "currentThread" => rt.intern("compat:thread".into(), |rt| rt.new_object("java/lang/Thread")),
            "getName" => {
                let name = if crate::neo::is_client(rt) { "Render thread" } else { "Server thread" };
                rt.string(name)
            }
            "isAlive" => V::I(1),
            "isInterrupted" | "isDaemon" | "interrupted" => V::I(0),
            "sleep" | "yield" | "onSpinWait" | "interrupt" => V::Void,
            _ => return None,
        }),
        "java/lang/Runtime" => Some(match name {
            "getRuntime" => rt.new_object("java/lang/Runtime"),
            "availableProcessors" => V::I(std::thread::available_parallelism().map_or(4, |n| n.get() as i32)),
            "maxMemory" | "totalMemory" => V::J(4 << 30),
            "freeMemory" => V::J(1 << 30),
            _ => return None,
        }),
        _ => None,
    }
}

fn string(rt: &mut Rt, name: &str, desc: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Statics first.
    match name {
        "valueOf" | "copyValueOf" => {
            let s = match ks.first() {
                Some('L') if rt.class_of(a(0)).starts_with('[') => {
                    let chars: Vec<u16> = crate::jutil::elements(rt, a(0)).iter().map(|v| v.i() as u16).collect();
                    from16(&chars)
                }
                Some(&k) => rt.to_java_string(a(0), k),
                None => String::new(),
            };
            return Some(new_str(rt, s));
        }
        "join" => {
            let sep = text(rt, a(0));
            let parts: Vec<V> = if desc.contains("Iterable") { crate::jutil::elements(rt, a(1)) } else if desc.contains("[Ljava/lang/CharSequence;") { crate::jutil::elements(rt, a(1)) } else { args[1..].to_vec() };
            let s = parts.iter().map(|&p| text(rt, p)).collect::<Vec<_>>().join(&sep);
            return Some(new_str(rt, s));
        }
        "format" => {
            let (fmt, rest) = if desc.starts_with("(Ljava/util/Locale;") { (a(1), a(2)) } else { (a(0), a(1)) };
            let f = text(rt, fmt);
            let vals = crate::jutil::elements(rt, rest);
            let s = format_java(rt, &f, &vals);
            return Some(new_str(rt, s));
        }
        _ => {}
    }
    let this = a(0);
    let s = rt.str_of(this)?.to_owned();
    let u = utf16(&s);
    let arg_s = |rt: &Rt, i: usize| text(rt, a(i));
    Some(match name {
        "formatted" => {
            let vals = crate::jutil::elements(rt, a(1));
            let out = format_java(rt, &s, &vals);
            new_str(rt, out)
        }
        // `getBytes()`, `getBytes(charset)`: UTF-8 (the default, and what mods name); ISO-8859-1 and
        // US-ASCII a byte per char, `?` for one past their range; UTF-16 big-endian after its mark.
        "getBytes" if !desc.contains("II") => {
            let charset = match ks.first() {
                Some('L') => rt.str_of(rt.get_field(a(1), "$name")).or_else(|| rt.str_of(a(1))).unwrap_or("UTF-8").to_ascii_uppercase(),
                _ => "UTF-8".to_owned(),
            };
            let limit = match charset.as_str() {
                "ISO-8859-1" | "LATIN1" => Some(0xff),
                "US-ASCII" | "ASCII" => Some(0x7f),
                _ => None,
            };
            let bytes: Vec<u8> = match (limit, charset.as_str()) {
                (Some(max), _) => u.iter().map(|&c| if u32::from(c) <= max { c as u8 } else { b'?' }).collect(),
                (None, "UTF-16" | "UTF-16BE" | "UTF-16LE") => {
                    let le = charset == "UTF-16LE";
                    let mark: &[u8] = if charset == "UTF-16" { &[0xfe, 0xff] } else { &[] };
                    mark.iter().copied().chain(u.iter().flat_map(|c| if le { c.to_le_bytes() } else { c.to_be_bytes() })).collect()
                }
                _ => s.as_bytes().to_vec(),
            };
            rt.alloc("[B", crate::Data::Array(bytes.iter().map(|b| V::I(i32::from(*b as i8))).collect()))
        }
        "length" => V::I(u.len() as i32),
        "isEmpty" => V::I(u.is_empty() as i32),
        "isBlank" => V::I(s.trim().is_empty() as i32),
        "hashCode" => V::I(string_hash(&s)),
        "equals" | "contentEquals" => V::I((Some(s.as_str()) == rt.str_of(a(1))) as i32),
        "equalsIgnoreCase" => V::I(rt.str_of(a(1)).is_some_and(|o| o.to_lowercase() == s.to_lowercase()) as i32),
        "compareTo" => {
            let o = utf16(&arg_s(rt, 1));
            let n = u.len().min(o.len());
            V::I((0..n).find(|&i| u[i] != o[i]).map_or(u.len() as i32 - o.len() as i32, |i| u[i] as i32 - o[i] as i32))
        }
        "compareToIgnoreCase" => {
            let (x, y) = (s.to_lowercase(), arg_s(rt, 1).to_lowercase());
            V::I(x.cmp(&y) as i32)
        }
        "charAt" | "codePointAt" => V::I(u.get(a(1).i().max(0) as usize).copied().unwrap_or(0) as i32),
        "startsWith" => {
            let p = utf16(&arg_s(rt, 1));
            let from = if ks.len() > 1 { a(2).i().max(0) as usize } else { 0 };
            V::I(u.get(from..).is_some_and(|t| t.starts_with(&p)) as i32)
        }
        "endsWith" => V::I(s.ends_with(&arg_s(rt, 1)) as i32),
        "contains" => V::I(s.contains(&arg_s(rt, 1)) as i32),
        "indexOf" | "lastIndexOf" => {
            let needle: Vec<u16> = match ks.first() {
                Some('I') => char::from_u32(a(1).i() as u32).map(|c| utf16(&c.to_string())).unwrap_or_default(),
                _ => utf16(&arg_s(rt, 1)),
            };
            let from = ks.get(1).map(|_| a(2).i());
            let positions: Vec<usize> = (0..=u.len().saturating_sub(needle.len())).filter(|&i| u.len() >= needle.len() && u[i..i + needle.len()] == needle[..]).collect();
            let found = if name == "indexOf" {
                positions.into_iter().find(|&i| from.is_none_or(|f| i as i32 >= f))
            } else {
                positions.into_iter().rev().find(|&i| from.is_none_or(|f| i as i32 <= f))
            };
            V::I(found.map_or(-1, |i| i as i32))
        }
        "substring" | "subSequence" => {
            let lo = (a(1).i().max(0) as usize).min(u.len());
            let hi = if ks.len() > 1 { (a(2).i().max(0) as usize).min(u.len()) } else { u.len() };
            new_str(rt, from16(&u[lo..hi.max(lo)]))
        }
        "toLowerCase" => new_str(rt, s.to_lowercase()),
        "toUpperCase" => new_str(rt, s.to_uppercase()),
        "trim" | "strip" => new_str(rt, s.trim().to_owned()),
        "stripLeading" => new_str(rt, s.trim_start().to_owned()),
        "stripTrailing" => new_str(rt, s.trim_end().to_owned()),
        "intern" | "toString" => {
            let v = rt.string(&s);
            v
        }
        "concat" => {
            let t = format!("{s}{}", arg_s(rt, 1));
            new_str(rt, t)
        }
        "repeat" => new_str(rt, s.repeat(a(1).i().max(0) as usize)),
        "replace" => {
            let (from, to) = if ks.first() == Some(&'C') {
                (char::from_u32(a(1).i() as u32).unwrap_or('\0').to_string(), char::from_u32(a(2).i() as u32).unwrap_or('\0').to_string())
            } else {
                (arg_s(rt, 1), arg_s(rt, 2))
            };
            new_str(rt, if from.is_empty() { s } else { s.replace(&from, &to) })
        }
        // Java's regexes ([`crate::jregex`]); a literal pattern and a plain replacement skip the
        // engine.
        "replaceAll" | "replaceFirst" => {
            let (pat, to) = (arg_s(rt, 1), arg_s(rt, 2));
            match unescape_regex(&pat) {
                Some(lit) if !lit.is_empty() && !to.contains(['$', '\\']) => {
                    new_str(rt, if name == "replaceAll" { s.replace(&lit, &to) } else { s.replacen(&lit, &to, 1) })
                }
                _ => crate::jregex::replace(rt, &s, &pat, &to, name == "replaceFirst"),
            }
        }
        "split" => {
            let limit = if ks.len() < 2 { 0 } else { a(2).i() };
            crate::jregex::split(rt, &s, &arg_s(rt, 1), limit)
        }
        "toCharArray" => rt.alloc("[C", Data::Array(u.iter().map(|&c| V::I(c as i32)).collect())),
        "chars" => rt.alloc("java/util/stream/Stream", Data::Stream(u.iter().map(|&c| V::I(c as i32)).collect())),
        "matches" => crate::jregex::matches(rt, &s, &arg_s(rt, 1)),
        _ => return None,
    })
}

/// A regex that is a plain string (`_`, `\\.`, `\\s` excepted), unescaped; None for real patterns.
fn unescape_regex(p: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = p.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                let n = it.next()?;
                if n.is_ascii_alphanumeric() {
                    return None;
                }
                out.push(n);
            }
            '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '^' | '$' | '|' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// `String.format` for the conversions mods use: `%s %d %f %.Nf %x %c %b %%` and `%n`.
fn format_java(rt: &Rt, f: &str, vals: &[V]) -> String {
    let mut out = String::new();
    let mut it = f.chars().peekable();
    let mut next = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek() {
            if d.is_ascii_digit() || d == '.' || d == '-' || d == '0' || d == ',' || d == '+' {
                spec.push(d);
                it.next();
            } else {
                break;
            }
        }
        let Some(conv) = it.next() else { break };
        let v = vals.get(next).copied().unwrap_or_default();
        let unbox = |v: V| match v {
            V::R(_) if rt.class_of(v).starts_with("java/lang/") && rt.str_of(v).is_none() => rt.get_field(v, "value"),
            v => v,
        };
        let piece = match conv {
            '%' => "%".to_owned(),
            'n' => "\n".to_owned(),
            's' | 'S' => {
                next += 1;
                let t = text(rt, v);
                if conv == 'S' { t.to_uppercase() } else { t }
            }
            'd' => {
                next += 1;
                match unbox(v) {
                    V::J(j) => j.to_string(),
                    x => x.i().to_string(),
                }
            }
            'x' | 'X' => {
                next += 1;
                let t = match unbox(v) {
                    V::J(j) => format!("{j:x}"),
                    x => format!("{:x}", x.i()),
                };
                if conv == 'X' { t.to_uppercase() } else { t }
            }
            'f' | 'e' | 'g' => {
                next += 1;
                let x = match unbox(v) {
                    V::F(x) => x as f64,
                    V::D(x) => x,
                    V::I(i) => i as f64,
                    V::J(j) => j as f64,
                    _ => 0.0,
                };
                let prec = spec.split_once('.').and_then(|(_, p)| p.parse().ok()).unwrap_or(6);
                fixed_half_up(x, prec)
            }
            'c' => {
                next += 1;
                char::from_u32(unbox(v).i() as u32).map(String::from).unwrap_or_default()
            }
            'b' => {
                next += 1;
                (unbox(v).i() != 0).to_string()
            }
            other => format!("%{spec}{other}"),
        };
        // Width (`%5d`, `%-5s`, `%05d`).
        let width: usize = spec.split('.').next().unwrap_or("").trim_start_matches(['-', '0', '+', ',']).parse().unwrap_or(0);
        if piece.chars().count() < width {
            let pad = width - piece.chars().count();
            if spec.starts_with('-') {
                out.push_str(&piece);
                out.push_str(&" ".repeat(pad));
            } else if spec.starts_with('0') {
                out.push_str(&"0".repeat(pad));
                out.push_str(&piece);
            } else {
                out.push_str(&" ".repeat(pad));
                out.push_str(&piece);
            }
        } else {
            out.push_str(&piece);
        }
    }
    out
}

/// `x` with `prec` decimals as Java's `Formatter` writes it: the shortest decimal that reads
/// back as `x`, rounded half up.
fn fixed_half_up(x: f64, prec: usize) -> String {
    if !x.is_finite() {
        return if x.is_nan() { "NaN".into() } else if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let neg = x.is_sign_negative() && x != 0.0;
    let shortest = format!("{}", x.abs());
    let (int, frac) = shortest.split_once('.').unwrap_or((&shortest, ""));
    let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    let point = int.len();
    let keep = point + prec;
    if digits.len() > keep {
        let up = digits[keep] >= 5;
        digits.truncate(keep);
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    while digits.len() < keep {
        digits.push(0);
    }
    let int_len = digits.len() - prec;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.extend(digits[..int_len].iter().map(|d| (d + b'0') as char));
    if prec > 0 {
        out.push('.');
        out.extend(digits[int_len..].iter().map(|d| (d + b'0') as char));
    }
    out
}

fn builder(rt: &mut Rt, name: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let cur = |rt: &Rt| match rt.heap.get(this.r() as usize).map(|o| &o.data) {
        Some(Data::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let set = |rt: &mut Rt, s: String| {
        if let Some(o) = rt.heap.get_mut(this.r() as usize) {
            o.data = Data::Str(s);
        }
    };
    Some(match name {
        "<init>" => {
            let s = match ks.first() {
                Some('L') => text(rt, a(1)),
                _ => String::new(),
            };
            set(rt, s);
            V::Void
        }
        "append" => {
            let piece = match ks.first() {
                Some('L') if rt.class_of(a(1)) == "[C" => from16(&crate::jutil::elements(rt, a(1)).iter().map(|v| v.i() as u16).collect::<Vec<_>>()),
                Some(&k) => rt.to_java_string(a(1), k),
                None => String::new(),
            };
            let s = cur(rt) + &piece;
            set(rt, s);
            this
        }
        "insert" => {
            let mut u = utf16(&cur(rt));
            let at = (a(1).i().max(0) as usize).min(u.len());
            let piece = utf16(&rt.to_java_string(a(2), ks.get(1).copied().unwrap_or('L')));
            u.splice(at..at, piece);
            set(rt, from16(&u));
            this
        }
        "toString" => {
            let s = cur(rt);
            new_str(rt, s)
        }
        "length" => V::I(utf16(&cur(rt)).len() as i32),
        "isEmpty" => V::I(cur(rt).is_empty() as i32),
        "charAt" => V::I(utf16(&cur(rt)).get(a(1).i().max(0) as usize).copied().unwrap_or(0) as i32),
        "setLength" => {
            let mut u = utf16(&cur(rt));
            u.resize(a(1).i().max(0) as usize, 0);
            set(rt, from16(&u));
            V::Void
        }
        "deleteCharAt" => {
            let mut u = utf16(&cur(rt));
            let i = a(1).i().max(0) as usize;
            if i < u.len() {
                u.remove(i);
            }
            set(rt, from16(&u));
            this
        }
        "delete" | "replace" => {
            let mut u = utf16(&cur(rt));
            let lo = (a(1).i().max(0) as usize).min(u.len());
            let hi = (a(2).i().max(0) as usize).min(u.len()).max(lo);
            let with = if name == "replace" { utf16(&text(rt, a(3))) } else { Vec::new() };
            u.splice(lo..hi, with);
            set(rt, from16(&u));
            this
        }
        "reverse" => {
            let s: String = cur(rt).chars().rev().collect();
            set(rt, s);
            this
        }
        "indexOf" => V::I(cur(rt).find(&text(rt, a(1))).map_or(-1, |i| utf16(&cur(rt)[..i]).len() as i32)),
        _ => return None,
    })
}

fn number(rt: &mut Rt, owner: &str, name: &str, desc: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let simple = owner.rsplit('/').next().unwrap_or(owner);
    let boxed = |rt: &mut Rt, v: V| {
        let class: &'static str = match simple {
            "Long" => "java/lang/Long",
            "Double" => "java/lang/Double",
            "Float" => "java/lang/Float",
            "Short" => "java/lang/Short",
            "Byte" => "java/lang/Byte",
            _ => "java/lang/Integer",
        };
        let o = rt.new_object(class);
        rt.put_field(o, "value", v);
        o
    };
    let parse = |s: &str| -> V {
        let t = s.trim();
        match simple {
            "Long" => V::J(t.parse().unwrap_or(0)),
            "Double" => V::D(t.parse().unwrap_or(0.0)),
            "Float" => V::F(t.parse().unwrap_or(0.0)),
            _ => V::I(t.parse().unwrap_or(0)),
        }
    };
    // `xValue()` on any boxed number, with Java's narrowing; a number a collection holds raw
    // (fastutil's primitive collections) is its own value.
    if let Some(kind) = name.strip_suffix("Value") {
        let v = match a(0) {
            V::R(_) => rt.get_field(a(0), "value"),
            raw => raw,
        };
        let as_f64 = match v {
            V::I(i) => i as f64,
            V::J(j) => j as f64,
            V::F(f) => f as f64,
            V::D(d) => d,
            _ => 0.0,
        };
        return Some(match (kind, v) {
            ("int" | "short" | "byte", V::I(i)) => V::I(i),
            ("int", V::J(j)) => V::I(j as i32),
            ("int" | "short" | "byte", _) => V::I(crate::d2i(as_f64)),
            ("long", V::J(j)) => V::J(j),
            ("long", V::I(i)) => V::J(i as i64),
            ("long", _) => V::J(as_f64 as i64),
            ("float", _) => V::F(as_f64 as f32),
            ("double", _) => V::D(as_f64),
            _ => return None,
        });
    }
    Some(match name {
        "valueOf" if ks.first() == Some(&'L') => {
            let s = text(rt, a(0));
            let v = parse(&s);
            boxed(rt, v)
        }
        "valueOf" => boxed(rt, a(0)),
        "parseInt" | "parseLong" | "parseDouble" | "parseFloat" | "parseShort" | "parseByte" => {
            let s = text(rt, a(0));
            if ks.len() > 1 && simple == "Integer" {
                V::I(i32::from_str_radix(s.trim(), a(1).i() as u32).unwrap_or(0))
            } else {
                parse(&s)
            }
        }
        "toString" if ks.is_empty() => {
            let v = rt.get_field(a(0), "value");
            let k = match v {
                V::J(_) => 'J',
                V::F(_) => 'F',
                V::D(_) => 'D',
                _ => 'I',
            };
            let s = rt.to_java_string(v, k);
            new_str(rt, s)
        }
        "toString" => {
            let s = rt.to_java_string(a(0), ks[0]);
            new_str(rt, s)
        }
        "toHexString" | "toBinaryString" => {
            let s = match (a(0), name) {
                (V::J(j), "toHexString") => format!("{:x}", j as u64),
                (V::J(j), _) => format!("{:b}", j as u64),
                (v, "toHexString") => format!("{:x}", v.i() as u32),
                (v, _) => format!("{:b}", v.i() as u32),
            };
            new_str(rt, s)
        }
        "hashCode" if ks.is_empty() => {
            let v = rt.get_field(a(0), "value");
            V::I(rt.java_hash(v))
        }
        "hashCode" => V::I(rt.java_hash(a(0))),
        "equals" => V::I(crate::jutil::equals(rt, a(0), a(1)) as i32),
        "compare" | "compareTo" => {
            // A number a collection holds raw (`Integer::compareTo` over fastutil's) is its own value.
            let unbox = |rt: &Rt, v: V| match v {
                V::R(_) => rt.get_field(v, "value"),
                raw => raw,
            };
            let (x, y) = if name == "compareTo" { (unbox(rt, a(0)), unbox(rt, a(1))) } else { (a(0), a(1)) };
            V::I(match (x, y) {
                (V::I(p), V::I(q)) => p.cmp(&q) as i32,
                (V::J(p), V::J(q)) => p.cmp(&q) as i32,
                (V::F(p), V::F(q)) => p.total_cmp(&q) as i32,
                (V::D(p), V::D(q)) => p.total_cmp(&q) as i32,
                _ => 0,
            })
        }
        "max" | "min" | "sum" => math(if name == "sum" { "addExact" } else { name }, ks, args)?,
        "signum" => V::I(match a(0) {
            V::J(j) => j.signum() as i32,
            v => v.i().signum(),
        }),
        "bitCount" => V::I(match a(0) {
            V::J(j) => j.count_ones() as i32,
            v => v.i().count_ones() as i32,
        }),
        "numberOfLeadingZeros" => V::I(match a(0) {
            V::J(j) => j.leading_zeros() as i32,
            v => v.i().leading_zeros() as i32,
        }),
        "numberOfTrailingZeros" => V::I(match a(0) {
            V::J(j) => j.trailing_zeros() as i32,
            v => v.i().trailing_zeros() as i32,
        }),
        "highestOneBit" => V::I(match a(0).i() {
            0 => 0,
            i => 1i32.wrapping_shl(31 - (i as u32).leading_zeros()),
        }),
        "isNaN" => V::I(match a(0) {
            V::F(f) => f.is_nan(),
            V::D(d) => d.is_nan(),
            _ => false,
        } as i32),
        "isInfinite" | "isFinite" => {
            let x = match a(0) {
                V::F(f) => f as f64,
                V::D(d) => d,
                _ => 0.0,
            };
            V::I(if name == "isFinite" { x.is_finite() } else { x.is_infinite() } as i32)
        }
        "floatToIntBits" | "floatToRawIntBits" => V::I(a(0).f().to_bits() as i32),
        "intBitsToFloat" => V::F(f32::from_bits(a(0).i() as u32)),
        "doubleToLongBits" | "doubleToRawLongBits" => V::J(a(0).d().to_bits() as i64),
        "longBitsToDouble" => V::D(f64::from_bits(a(0).j() as u64)),
        _ => {
            let _ = desc;
            return None;
        }
    })
}

fn math(name: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let d = |i: usize| match a(i) {
        V::I(x) => x as f64,
        V::J(x) => x as f64,
        V::F(x) => x as f64,
        V::D(x) => x,
        _ => 0.0,
    };
    let k = ks.first().copied().unwrap_or('D');
    Some(match name {
        "max" | "min" => {
            let pick_max = name == "max";
            match (a(0), a(1)) {
                (V::I(x), V::I(y)) => V::I(if pick_max { x.max(y) } else { x.min(y) }),
                (V::J(x), V::J(y)) => V::J(if pick_max { x.max(y) } else { x.min(y) }),
                (V::F(x), V::F(y)) => V::F(if x.is_nan() || y.is_nan() { f32::NAN } else if pick_max { x.max(y) } else { x.min(y) }),
                (V::D(x), V::D(y)) => V::D(if x.is_nan() || y.is_nan() { f64::NAN } else if pick_max { x.max(y) } else { x.min(y) }),
                _ => return None,
            }
        }
        "abs" => match a(0) {
            V::I(x) => V::I(x.wrapping_abs()),
            V::J(x) => V::J(x.wrapping_abs()),
            V::F(x) => V::F(x.abs()),
            V::D(x) => V::D(x.abs()),
            _ => return None,
        },
        "addExact" => match (a(0), a(1)) {
            (V::I(x), V::I(y)) => V::I(x.wrapping_add(y)),
            (V::J(x), V::J(y)) => V::J(x.wrapping_add(y)),
            (V::F(x), V::F(y)) => V::F(x + y),
            (V::D(x), V::D(y)) => V::D(x + y),
            _ => return None,
        },
        "floor" => V::D(d(0).floor()),
        "ceil" => V::D(d(0).ceil()),
        "rint" => V::D(d(0).round_ties_even()),
        "round" => match k {
            'F' => V::I((a(0).f() + 0.5).floor() as i32),
            _ => V::J((d(0) + 0.5).floor() as i64),
        },
        // `ceilDiv`: the quotient rounded up.
        "ceilDiv" => match (a(0), a(1)) {
            (V::I(x), V::I(y)) if y != 0 => V::I(x.wrapping_div(y) + i32::from((x % y != 0) && ((x ^ y) >= 0))),
            (V::J(x), V::J(y)) if y != 0 => V::J(x.wrapping_div(y) + i64::from((x % y != 0) && ((x ^ y) >= 0))),
            _ => return None,
        },
        "floorDiv" | "floorMod" => match (a(0), a(1)) {
            (V::I(x), V::I(y)) if y != 0 => V::I(if name == "floorDiv" { x.div_euclid(y) - if (x.rem_euclid(y) != 0) && y < 0 { 1 } else { 0 } } else { ((x % y) + y) % y }),
            (V::J(x), V::J(y)) if y != 0 => V::J(if name == "floorDiv" { (x as f64 / y as f64).floor() as i64 } else { ((x % y) + y) % y }),
            _ => return None,
        },
        "clamp" => match (a(0), a(1), a(2)) {
            (V::J(v), V::I(lo), V::I(hi)) => V::I((v.clamp(lo as i64, hi as i64)) as i32),
            (V::J(v), V::J(lo), V::J(hi)) => V::J(v.clamp(lo, hi)),
            (V::F(v), V::F(lo), V::F(hi)) => V::F(v.clamp(lo, hi)),
            (V::D(v), V::D(lo), V::D(hi)) => V::D(v.clamp(lo, hi)),
            (V::I(v), V::I(lo), V::I(hi)) => V::I(v.clamp(lo, hi)),
            _ => return None,
        },
        "sqrt" => V::D(d(0).sqrt()),
        "cbrt" => V::D(d(0).cbrt()),
        "pow" => V::D(d(0).powf(d(1))),
        "exp" => V::D(d(0).exp()),
        "log" => V::D(d(0).ln()),
        "log10" => V::D(d(0).log10()),
        "sin" => V::D(d(0).sin()),
        "cos" => V::D(d(0).cos()),
        "tan" => V::D(d(0).tan()),
        "asin" => V::D(d(0).asin()),
        "acos" => V::D(d(0).acos()),
        "atan" => V::D(d(0).atan()),
        "atan2" => V::D(d(0).atan2(d(1))),
        "toRadians" => V::D(d(0).to_radians()),
        "toDegrees" => V::D(d(0).to_degrees()),
        "hypot" => V::D(d(0).hypot(d(1))),
        "signum" => match k {
            'F' => V::F(if a(0).f() == 0.0 { 0.0 } else { a(0).f().signum() }),
            _ => V::D(if d(0) == 0.0 { 0.0 } else { d(0).signum() }),
        },
        "random" => V::D(0.5),
        _ => return None,
    })
}

/// commons-lang3's mutable holders (`MutableObject`, `MutableBoolean`, `MutableInt`...): one value
/// each, in field `value`.
fn mutable(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let kind = owner.rsplit('/').next().unwrap_or(owner).trim_start_matches("Mutable");
    let zero = match kind {
        "Object" => V::NULL,
        "Long" => V::J(0),
        "Float" => V::F(0.0),
        "Double" => V::D(0.0),
        _ => V::I(0),
    };
    // A boxed number or boolean argument (`setValue(Number)`): its value.
    let unbox = |rt: &Rt, v: V| match v {
        V::R(r) if r != 0 && kind != "Object" => rt.get_field(v, "value"),
        v => v,
    };
    let get = |rt: &Rt| match rt.get_field(a(0), "value") {
        V::Void => zero,
        v => v,
    };
    let num = |v: V| -> f64 {
        match v {
            V::I(i) => i as f64,
            V::J(j) => j as f64,
            V::F(f) => f as f64,
            V::D(d) => d,
            _ => 0.0,
        }
    };
    let of = |x: f64| match kind {
        "Long" => V::J(x as i64),
        "Float" => V::F(x as f32),
        "Double" => V::D(x),
        _ => V::I(x as i64 as i32),
    };
    let add = |rt: &mut Rt, by: f64| -> (V, V) {
        let old = get(rt);
        let new = match (old, kind) {
            (V::I(i), "Int" | "Short" | "Byte") => V::I(i.wrapping_add(by as i32)),
            (V::J(j), "Long") => V::J(j.wrapping_add(by as i64)),
            (v, _) => of(num(v) + by),
        };
        rt.put_field(a(0), "value", new);
        (old, new)
    };
    Some(match name {
        "<init>" => {
            let v = if desc == "()V" { zero } else { unbox(rt, a(1)) };
            rt.put_field(a(0), "value", v);
            V::Void
        }
        "getValue" if kind == "Object" => get(rt),
        "getValue" => {
            let class: &'static str = match kind {
                "Long" => "java/lang/Long",
                "Float" => "java/lang/Float",
                "Double" => "java/lang/Double",
                "Boolean" => return Some(get(rt)),
                _ => "java/lang/Integer",
            };
            let v = get(rt);
            let o = rt.new_object(class);
            rt.put_field(o, "value", v);
            o
        }
        "setValue" => {
            let v = unbox(rt, a(1));
            rt.put_field(a(0), "value", v);
            V::Void
        }
        "booleanValue" | "isTrue" => get(rt),
        "isFalse" => V::I((get(rt).i() == 0) as i32),
        "setTrue" | "setFalse" => {
            rt.put_field(a(0), "value", V::I((name == "setTrue") as i32));
            V::Void
        }
        "intValue" => V::I(num(get(rt)) as i64 as i32),
        "longValue" => V::J(num(get(rt)) as i64),
        "floatValue" => V::F(num(get(rt)) as f32),
        "doubleValue" => V::D(num(get(rt))),
        "increment" | "decrement" => {
            add(rt, if name == "increment" { 1.0 } else { -1.0 });
            V::Void
        }
        "incrementAndGet" | "decrementAndGet" => add(rt, if name == "incrementAndGet" { 1.0 } else { -1.0 }).1,
        "getAndIncrement" | "getAndDecrement" => add(rt, if name == "getAndIncrement" { 1.0 } else { -1.0 }).0,
        "add" | "subtract" => {
            let by = num(unbox(rt, a(1)));
            add(rt, if name == "add" { by } else { -by });
            V::Void
        }
        "addAndGet" => {
            let by = num(unbox(rt, a(1)));
            add(rt, by).1
        }
        "getAndAdd" => {
            let by = num(unbox(rt, a(1)));
            add(rt, by).0
        }
        "toString" => {
            let v = get(rt);
            let t = match v {
                V::I(i) if kind == "Boolean" => (i != 0).to_string(),
                V::I(i) => i.to_string(),
                V::J(j) => j.to_string(),
                V::F(f) => f.to_string(),
                V::D(d) => d.to_string(),
                _ => return None,
            };
            rt.string(&t)
        }
        _ => return None,
    })
}

/// A `Boolean`'s value, boxed or held raw; None for anything else.
fn bool_value(rt: &Rt, v: V) -> Option<i32> {
    match v {
        V::R(_) => (rt.class_of(v) == "java/lang/Boolean").then(|| (rt.get_field(v, "value").i() != 0) as i32),
        V::I(i) => Some((i != 0) as i32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"))
    }

    /// A runtime is one game thread: `Thread.currentThread()` is the same object each time it
    /// asks (ESL's transactions compare it by reference), and the client's runtime is the render
    /// thread.
    #[test]
    fn a_runtime_is_one_thread() {
        let current = "java/lang/Thread.currentThread.()Ljava/lang/Thread;";
        let name = "java/lang/Thread.getName.()Ljava/lang/String;";
        let mut server = Rt::new();
        let first = c(&mut server, current, &[]);
        assert_eq!(c(&mut server, current, &[]), first);
        let n = c(&mut server, name, &[first]);
        assert_eq!(server.str_of(n), Some("Server thread"));
        let mut client = Rt::new();
        let marker = client.new_object("java/lang/Object");
        client.interned.insert("compat:client".into(), marker);
        let t = c(&mut client, current, &[]);
        let n = c(&mut client, name, &[t]);
        assert_eq!(client.str_of(n), Some("Render thread"));
    }

    /// Ponder keeps its scratch objects in `ThreadLocal.withInitial(...)`: made once, on the first
    /// `get`, then the same object each time. Create's event registrations pass `Triple.of(...)`.
    #[test]
    fn a_thread_local_makes_its_value_once_and_a_triple_keeps_three() {
        fn fresh(rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
            Some(rt.new_object("t/Scratch"))
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(fresh);
        let supplier = rt.lambda_id("java/util/function/Supplier", 1, vec![]);
        let local = c(&mut rt, "java/lang/ThreadLocal.withInitial.(Ljava/util/function/Supplier;)Ljava/lang/ThreadLocal;", &[supplier]);
        let first = c(&mut rt, "java/lang/ThreadLocal.get.()Ljava/lang/Object;", &[local]);
        assert_eq!(rt.class_of(first), "t/Scratch");
        assert_eq!(c(&mut rt, "java/lang/ThreadLocal.get.()Ljava/lang/Object;", &[local]), first);
        let (x, y, z) = (rt.string("x"), rt.string("y"), rt.string("z"));
        let t = c(&mut rt, "org/apache/commons/lang3/tuple/Triple.of.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Lorg/apache/commons/lang3/tuple/Triple;", &[x, y, z]);
        assert_eq!(c(&mut rt, "org/apache/commons/lang3/tuple/Triple.getMiddle.()Ljava/lang/Object;", &[t]), y);
        assert_eq!(c(&mut rt, "org/apache/commons/lang3/tuple/Triple.getRight.()Ljava/lang/Object;", &[t]), z);
    }

    /// `forEach(LongConsumer)` on a mod's own `LongIterable` (a default method of the library's
    /// interface, which the mod class doesn't declare): the mod's `iterator()` is walked with its
    /// own `hasNext`/`nextLong`, each long reaching the action in order.
    #[test]
    fn a_mods_iterable_walks_its_own_iterator_for_for_each() {
        const LONGS: [i64; 3] = [3, -1, 1 << 40];
        thread_local! {
            static GOT: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            let this = args[0];
            match (rt.class_of(this), short.split('.').next().unwrap_or("")) {
                ("t/Cuboid", "iterator") => rt.new_object("t/CuboidIterator"),
                ("t/CuboidIterator", "hasNext") => V::I((rt.get_field(this, "at").i() < 3) as i32),
                ("t/CuboidIterator", "nextLong") => {
                    let at = rt.get_field(this, "at").i();
                    rt.put_field(this, "at", V::I(at + 1));
                    V::J(LONGS[at as usize])
                }
                _ => panic!("unexpected {key}"),
            }
        }
        fn accept(_rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            GOT.with(|g| g.borrow_mut().push(args[1].j()));
            Some(V::Void)
        }
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(accept);
        let cuboid = rt.new_object("t/Cuboid");
        let action = rt.lambda_id("it/unimi/dsi/fastutil/longs/LongConsumer", 1, vec![]);
        c(&mut rt, "java/lang/Object.forEach.(Lit/unimi/dsi/fastutil/longs/LongConsumer;)V", &[cuboid, action]);
        assert_eq!(GOT.with(|g| g.borrow().clone()), LONGS);
    }

    /// Reflection over a mod's own class (Mekanism's computer methods are getters of its fields):
    /// a field by name reads and writes the object's field, boxed through `get`/`set` and raw
    /// through `getLong`; a static one reads the class's static once the class is initialised; a
    /// getter handle reads what the field holds as the call site types it; a method by name and
    /// parameter types runs the object's own method, its result boxed.
    #[test]
    fn reflection_reaches_a_mods_fields_and_methods_by_name() {
        fn vcall(rt: &mut Rt, _key: &'static str, short: &'static str, args: &[V]) -> V {
            assert_eq!(short, "getEnergy.()J");
            rt.get_field(args[0], "energy")
        }
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            if (class, what) == ("t/Machine", "<clinit>") && !rt.statics.contains_key("t/Machine.COUNT") {
                rt.statics.insert("t/Machine.COUNT", V::I(3));
            }
            Some(V::Void)
        }
        fn none(_: &mut Rt, _: u32, _: &'static str, _: &[V]) -> V {
            V::Void
        }
        let mut rt = Rt::new();
        rt.supers.insert("t/Machine", "java/lang/Object");
        rt.vcall = Some(vcall);
        rt.reflect = Some(reflect);
        rt.vkeys.insert("getEnergy.()J", (none, |_| true));
        let class = class_object(&mut rt, "t/Machine");
        let machine = rt.new_object("t/Machine");
        rt.put_field(machine, "energy", V::J(5));

        let name = rt.string("energy");
        let energy = c(&mut rt, "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;", &[class, name]);
        c(&mut rt, "java/lang/reflect/Field.setAccessible.(Z)V", &[energy, V::I(1)]);
        let boxed = c(&mut rt, "java/lang/reflect/Field.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[energy, machine]);
        assert_eq!((rt.class_of(boxed), rt.get_field(boxed, "value")), ("java/lang/Long", V::J(5)));
        let seven = crate::mixin::box_value(&mut rt, V::J(7), 'J');
        c(&mut rt, "java/lang/reflect/Field.set.(Ljava/lang/Object;Ljava/lang/Object;)V", &[energy, machine, seven]);
        assert_eq!(rt.get_field(machine, "energy"), V::J(7));
        assert_eq!(c(&mut rt, "java/lang/reflect/Field.getLong.(Ljava/lang/Object;)J", &[energy, machine]), V::J(7));

        let name = rt.string("COUNT");
        let count = c(&mut rt, "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;", &[class, name]);
        let boxed = c(&mut rt, "java/lang/reflect/Field.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[count, V::NULL]);
        assert_eq!((rt.class_of(boxed), rt.get_field(boxed, "value")), ("java/lang/Integer", V::I(3)));

        let lookup = c(&mut rt, "java/lang/invoke/MethodHandles.lookup.()Ljava/lang/invoke/MethodHandles$Lookup;", &[]);
        let getter = c(&mut rt, "java/lang/invoke/MethodHandles$Lookup.unreflectGetter.(Ljava/lang/reflect/Field;)Ljava/lang/invoke/MethodHandle;", &[lookup, energy]);
        assert_eq!(c(&mut rt, "java/lang/invoke/MethodHandle.invokeExact.(Lt/Machine;)J", &[getter, machine]), V::J(7));
        let boxed = c(&mut rt, "java/lang/invoke/MethodHandle.invoke.(Ljava/lang/Object;)Ljava/lang/Object;", &[getter, machine]);
        assert_eq!(rt.get_field(boxed, "value"), V::J(7));

        let name = rt.string("getEnergy");
        let no_params = rt.new_array(0, V::NULL);
        let method = c(&mut rt, "java/lang/Class.getDeclaredMethod.(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;", &[class, name, no_params]);
        let no_args = rt.new_array(0, V::NULL);
        let boxed = c(&mut rt, "java/lang/reflect/Method.invoke.(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;", &[method, machine, no_args]);
        assert_eq!((rt.class_of(boxed), rt.get_field(boxed, "value")), ("java/lang/Long", V::J(7)));
    }

    /// A mod class's fields as its table declares them (Mekanism's `SyncMapper` picks how to sync a
    /// field by its `getType()`): class objects are one per class, so `type == long.class` holds as
    /// in Java; an array field's class tells what it holds; a superclass declares what it inherits.
    #[test]
    fn a_mod_classs_fields_tell_their_types() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            let fields = match (class, what) {
                ("t/Base", "fields") => "1 owner Ljava/util/UUID;",
                ("t/Tank", "fields") => "2 stored J\n1 levels [I\n25 MAX I",
                _ => return None,
            };
            Some(rt.string(fields))
        }
        const GET_TYPE: &str = "java/lang/reflect/Field.getType.()Ljava/lang/Class;";
        let mut rt = Rt::new();
        rt.supers.insert("t/Tank", "t/Base");
        rt.supers.insert("t/Base", "java/lang/Object");
        rt.reflect = Some(reflect);
        let tank = class_object(&mut rt, "t/Tank");
        assert_eq!(class_object(&mut rt, "t/Tank"), tank, "one class object per class");
        let field = |rt: &mut Rt, name: &str| {
            let n = rt.string(name);
            c(rt, "java/lang/Class.getField.(Ljava/lang/String;)Ljava/lang/reflect/Field;", &[tank, n])
        };
        let stored = field(&mut rt, "stored");
        let long = crate::api::get_static(&mut rt, "java/lang/Long.TYPE");
        assert_eq!(c(&mut rt, GET_TYPE, &[stored]), long);
        assert_eq!(c(&mut rt, "java/lang/reflect/Field.getModifiers.()I", &[stored]), V::I(2), "private");
        let levels = field(&mut rt, "levels");
        let ints = c(&mut rt, GET_TYPE, &[levels]);
        assert_eq!(c(&mut rt, "java/lang/Class.isArray.()Z", &[ints]), V::I(1));
        let int = c(&mut rt, "java/lang/Class.getComponentType.()Ljava/lang/Class;", &[ints]);
        assert_eq!(int, crate::api::get_static(&mut rt, "java/lang/Integer.TYPE"));
        assert_eq!(c(&mut rt, "java/lang/Class.isPrimitive.()Z", &[int]), V::I(1));
        assert_eq!(c(&mut rt, "java/lang/Class.isArray.()Z", &[int]), V::I(0));
        let owner = field(&mut rt, "owner");
        let uuid = c(&mut rt, GET_TYPE, &[owner]);
        assert_eq!(rt.str_of(uuid), Some("java/util/UUID"));
        let names = |rt: &mut Rt, key: &'static str| {
            let all = c(rt, key, &[tank]);
            crate::jutil::elements(rt, all).into_iter().map(|f| text(rt, rt.get_field(f, "$name"))).collect::<Vec<_>>()
        };
        assert_eq!(names(&mut rt, "java/lang/Class.getDeclaredFields.()[Ljava/lang/reflect/Field;"), ["stored", "levels", "MAX"]);
        assert_eq!(names(&mut rt, "java/lang/Class.getFields.()[Ljava/lang/reflect/Field;"), ["levels", "MAX", "owner"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create's contraption keeps its level behind a `WeakReference`; nothing collects it here.
    #[test]
    fn a_weak_reference_gives_back_what_it_holds_until_cleared() {
        let mut rt = Rt::new();
        let r = rt.new_object("java/lang/ref/WeakReference");
        let level = rt.new_object("net/minecraft/world/level/Level");
        c(&mut rt, "java/lang/ref/WeakReference.<init>.(Ljava/lang/Object;)V", &[r, level]);
        assert_eq!(c(&mut rt, "java/lang/ref/WeakReference.get.()Ljava/lang/Object;", &[r]), level);
        c(&mut rt, "java/lang/ref/Reference.clear.()V", &[r]);
        assert_eq!(c(&mut rt, "java/lang/ref/WeakReference.get.()Ljava/lang/Object;", &[r]), V::NULL);
    }

    /// Create's `NBTHelper.readEnum(tag, key, Direction.Axis.class)` asks the class for its
    /// constants: a vanilla enum answers with the API's own, a class that isn't an enum with null.
    #[test]
    fn a_vanilla_enum_class_lists_its_constants() {
        let mut rt = Rt::new();
        let axis = class_object(&mut rt, "net/minecraft/core/Direction$Axis");
        let all = c(&mut rt, "java/lang/Class.getEnumConstants.()[Ljava/lang/Object;", &[axis]);
        let mut names = Vec::new();
        for v in crate::jutil::elements(&rt, all) {
            let s = c(&mut rt, "java/lang/Enum.name.()Ljava/lang/String;", &[v]);
            names.push(rt.str_of(s).unwrap_or("").to_owned());
        }
        assert_eq!(names, ["X", "Y", "Z"]);
        assert_eq!(c(&mut rt, "java/lang/Class.isEnum.()Z", &[axis]), V::I(1));
        let pos = class_object(&mut rt, "net/minecraft/core/BlockPos");
        assert_eq!(c(&mut rt, "java/lang/Class.isEnum.()Z", &[pos]), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn strings_count_utf16_and_hash_as_java() {
        let mut rt = Rt::new();
        let s = rt.string("com.simibubi.create.Create");
        let p = rt.string("com.simibubi.create");
        assert_eq!(c(&mut rt, "java/lang/String.startsWith.(Ljava/lang/String;)Z", &[s, p]), V::I(1));
        // "Fixture".hashCode() on the JVM.
        let f = rt.string("Fixture");
        assert_eq!(c(&mut rt, "java/lang/String.hashCode.()I", &[f]), V::I(821203433));
        let e = rt.string("a\u{1F600}b");
        assert_eq!(c(&mut rt, "java/lang/String.length.()I", &[e]), V::I(4));
        let sub = c(&mut rt, "java/lang/String.substring.(II)Ljava/lang/String;", &[s, V::I(4), V::I(12)]);
        assert_eq!(rt.str_of(sub), Some("simibubi"));
        let dot = rt.string("\\.");
        let parts = c(&mut rt, "java/lang/String.split.(Ljava/lang/String;)[Ljava/lang/String;", &[s, dot]);
        assert_eq!(crate::jutil::elements(&rt, parts).len(), 4);
    }

    /// `Boolean.TRUE.equals(x)` where `x` came back raw from a method reference used as a
    /// `Function` (Java would have boxed it).
    #[test]
    fn a_boolean_equals_its_value_boxed_or_raw() {
        let mut rt = Rt::new();
        let yes = crate::api_common::boolean(&mut rt, true);
        let eq = |rt: &mut Rt, x: V, y: V| c(rt, "java/lang/Boolean.equals.(Ljava/lang/Object;)Z", &[x, y]);
        assert_eq!((eq(&mut rt, yes, V::I(1)), eq(&mut rt, yes, V::I(0))), (V::I(1), V::I(0)));
        let text = rt.string("true");
        assert_eq!((eq(&mut rt, yes, text), eq(&mut rt, yes, V::NULL)), (V::I(0), V::I(0)));
        assert_eq!(c(&mut rt, "java/lang/Boolean.hashCode.()I", &[yes]), V::I(1231));
    }

    #[test]
    fn fixed_rounds_half_up_like_java() {
        assert_eq!(fixed_half_up(0.125, 2), "0.13");
        assert_eq!(fixed_half_up(2.5, 0), "3");
        assert_eq!(fixed_half_up(9.995, 2), "10.00");
        assert_eq!(fixed_half_up(-1.005, 2), "-1.01");
        assert_eq!(fixed_half_up(1e21, 1), "1000000000000000000000.0");
        assert_eq!(fixed_half_up(3.0, 3), "3.000");
    }

    #[test]
    fn builders_and_format() {
        let mut rt = Rt::new();
        let b = rt.new_object("java/lang/StringBuilder");
        c(&mut rt, "java/lang/StringBuilder.<init>.()V", &[b]);
        let x = rt.string("x=");
        c(&mut rt, "java/lang/StringBuilder.append.(Ljava/lang/String;)Ljava/lang/StringBuilder;", &[b, x]);
        c(&mut rt, "java/lang/StringBuilder.append.(I)Ljava/lang/StringBuilder;", &[b, V::I(-3)]);
        c(&mut rt, "java/lang/StringBuilder.append.(D)Ljava/lang/StringBuilder;", &[b, V::D(1.5)]);
        let s = c(&mut rt, "java/lang/StringBuilder.toString.()Ljava/lang/String;", &[b]);
        assert_eq!(rt.str_of(s), Some("x=-31.5"));
        let f = rt.string("%s has %03d rpm (%.2f)");
        let n = rt.string("shaft");
        let i = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(7)]);
        let d = c(&mut rt, "java/lang/Double.valueOf.(D)Ljava/lang/Double;", &[V::D(0.125)]);
        let arr = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![n, i, d]));
        let out = c(&mut rt, "java/lang/String.format.(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;", &[f, arr]);
        assert_eq!(rt.str_of(out), Some("shaft has 007 rpm (0.13)"));
    }

    #[test]
    fn number_formats_print_as_the_jdk_does_in_the_root_locale() {
        let mut rt = Rt::new();
        let fmt = |rt: &mut Rt, f: V, x: f64| {
            let s = c(rt, "java/text/NumberFormat.format.(D)Ljava/lang/String;", &[f, V::D(x)]);
            rt.str_of(s).unwrap_or("?").to_owned()
        };
        let f = c(&mut rt, "java/text/NumberFormat.getNumberInstance.(Ljava/util/Locale;)Ljava/text/NumberFormat;", &[V::NULL]);
        assert_eq!(fmt(&mut rt, f, 16384.0), "16,384");
        assert_eq!(fmt(&mut rt, f, 1234567.891), "1,234,567.891");
        assert_eq!(fmt(&mut rt, f, -0.0), "-0");
        // Create's `LangNumberFormat.update`: at most two decimals, half even on the exact value.
        c(&mut rt, "java/text/NumberFormat.setMaximumFractionDigits.(I)V", &[f, V::I(2)]);
        assert_eq!(fmt(&mut rt, f, 0.125), "0.12");
        assert_eq!(fmt(&mut rt, f, 1.005), "1");
        assert_eq!(fmt(&mut rt, f, 2.675), "2.67");
        let i = c(&mut rt, "java/text/NumberFormat.getIntegerInstance.()Ljava/text/NumberFormat;", &[]);
        assert_eq!(fmt(&mut rt, i, 2.5), "2");
        assert_eq!(fmt(&mut rt, i, 3.5), "4");
        let pct = c(&mut rt, "java/text/NumberFormat.getPercentInstance.()Ljava/text/NumberFormat;", &[]);
        assert_eq!(fmt(&mut rt, pct, 0.256), "26%");
        for (pattern, x, want) in [("#.##", 0.456, ".46"), ("#.##", 0.0, "0"), ("0.00", 3.14159, "3.14"), ("#,##0.0", 1234.56, "1,234.6"), ("00", 7.0, "07")] {
            let d = rt.new_object("java/text/DecimalFormat");
            let p = rt.string(pattern);
            c(&mut rt, "java/text/DecimalFormat.<init>.(Ljava/lang/String;)V", &[d, p]);
            assert_eq!(fmt(&mut rt, d, x), want, "{pattern}");
        }
    }
}

/// What a mod logs at `info`, `warn` or `error`, printed when tracing (`HORIZON_COMPAT_TRACE`): the
/// message with each `{}` filled from the arguments after it, in turn, as SLF4J and Log4j fill
/// them (a last `Object...` spread). A marker before the message is left out.
fn mod_log(rt: &Rt, level: &str, desc: &str, args: &[V]) {
    let marker = desc.starts_with("(Lorg/apache/logging/log4j/Marker;") || desc.starts_with("(Lorg/slf4j/Marker;");
    let args = args.get(usize::from(marker)..).unwrap_or_default();
    let Some((&message, rest)) = args.split_first() else { return };
    let Some(message) = rt.str_of(message) else { return };
    let spread = desc.contains("[Ljava/lang/Object;)");
    let mut values = Vec::new();
    for (i, &v) in rest.iter().enumerate() {
        if spread && i + 1 == rest.len() {
            values.extend(rt.captured(v));
        } else {
            values.push(v);
        }
    }
    let mut values = values.into_iter();
    let mut parts = message.split("{}");
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        match values.next() {
            Some(v) => out.push_str(&rt.to_java_string(v, 'L')),
            None => out.push_str("{}"),
        }
        out.push_str(part);
    }
    eprintln!("horizon-compat: mod {level}: {out}");
}

/// Small libraries mods lean on: loggers (printed only when tracing), commons-lang `Pair`,
/// `Record`'s constructor.
fn libs(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let logger = matches!(owner, "org/apache/logging/log4j/Logger" | "org/slf4j/Logger" | "java/util/logging/Logger");
    Some(match (owner, name) {
        (_, n) if logger && n.starts_with("is") && n.ends_with("Enabled") => V::I(0),
        (_, "info" | "warn" | "error") if logger && Rt::tracing() => {
            mod_log(rt, name, desc, args.get(1..).unwrap_or_default());
            if desc.ends_with(")V") { V::Void } else { V::NULL }
        }
        (_, _) if logger => {
            if desc.ends_with(")V") { V::Void } else { V::NULL }
        }
        ("org/apache/logging/log4j/LogManager" | "org/slf4j/LoggerFactory" | "com/mojang/logging/LogUtils", "getLogger") => {
            rt.intern("compat:logger".into(), |rt| rt.new_object("org/apache/logging/log4j/Logger"))
        }
        ("org/apache/logging/log4j/MarkerManager", "getMarker") => a(0),
        ("org/apache/commons/lang3/tuple/Pair" | "org/apache/commons/lang3/tuple/ImmutablePair" | "org/apache/commons/lang3/tuple/MutablePair" | "com/mojang/datafixers/util/Pair", "of" | "<init>") => {
            let made = if owner.ends_with("MutablePair") { "org/apache/commons/lang3/tuple/MutablePair" } else { "org/apache/commons/lang3/tuple/Pair" };
            let (p, l, r) = if name == "<init>" { (a(0), a(1), a(2)) } else { (rt.new_object(made), a(0), a(1)) };
            rt.put_field(p, "$left", l);
            rt.put_field(p, "$right", r);
            if name == "<init>" { V::Void } else { p }
        }
        (o, "getLeft" | "getKey" | "getFirst" | "left") if o.ends_with("Pair") => rt.get_field(a(0), "$left"),
        (o, "getRight" | "getValue" | "getSecond" | "right") if o.ends_with("Pair") => rt.get_field(a(0), "$right"),
        // commons-lang's `Triple` (`ImmutableTriple`, `MutableTriple`): three values, as a pair's two.
        ("org/apache/commons/lang3/tuple/Triple" | "org/apache/commons/lang3/tuple/ImmutableTriple" | "org/apache/commons/lang3/tuple/MutableTriple", "of" | "<init>") => {
            let made = if owner.ends_with("MutableTriple") { "org/apache/commons/lang3/tuple/MutableTriple" } else { "org/apache/commons/lang3/tuple/Triple" };
            let (t, at) = if name == "<init>" { (a(0), 1) } else { (rt.new_object(made), 0) };
            for (i, f) in ["$left", "$middle", "$right"].into_iter().enumerate() {
                rt.put_field(t, f, a(at + i));
            }
            if name == "<init>" { V::Void } else { t }
        }
        (o, "getLeft" | "left") if o.ends_with("Triple") => rt.get_field(a(0), "$left"),
        (o, "getMiddle" | "middle") if o.ends_with("Triple") => rt.get_field(a(0), "$middle"),
        (o, "getRight" | "right") if o.ends_with("Triple") => rt.get_field(a(0), "$right"),
        ("org/apache/commons/lang3/tuple/MutableTriple", "setLeft" | "setMiddle" | "setRight") => {
            let f = match name {
                "setLeft" => "$left",
                "setMiddle" => "$middle",
                _ => "$right",
            };
            rt.put_field(a(0), f, a(1));
            V::Void
        }
        ("org/apache/commons/lang3/tuple/MutablePair", "setLeft" | "setRight" | "setValue") => {
            let old = rt.get_field(a(0), if name == "setLeft" { "$left" } else { "$right" });
            rt.put_field(a(0), if name == "setLeft" { "$left" } else { "$right" }, a(1));
            if name == "setValue" { old } else { V::Void }
        }
        (o, _) if o.starts_with("org/apache/commons/lang3/mutable/Mutable") => return mutable(rt, o, name, desc, args),
        ("java/text/NumberFormat" | "java/text/DecimalFormat" | "java/text/Format", _) => return number_format(rt, name, desc, args),
        // The root locale's symbols, which every format here prints with.
        ("java/text/DecimalFormatSymbols", n) => match n {
            "getInstance" => rt.new_object("java/text/DecimalFormatSymbols"),
            "<init>" => V::Void,
            "getDecimalSeparator" | "getMonetaryDecimalSeparator" => V::I('.' as i32),
            "getGroupingSeparator" => V::I(',' as i32),
            "getMinusSign" => V::I('-' as i32),
            "getZeroDigit" => V::I('0' as i32),
            n if n.starts_with("set") => V::Void,
            _ => return None,
        },
        // A character or a string repeated (jopt-simple's and commons-lang's helpers: an indent).
        ("joptsimple/internal/Strings" | "org/apache/commons/lang3/StringUtils", "repeat") => {
            let unit = if desc.starts_with("(C") { char::from_u32(a(0).i() as u32).unwrap_or(' ').to_string() } else { text(rt, a(0)) };
            new_str(rt, unit.repeat(a(1).i().max(0) as usize))
        }
        ("org/apache/commons/lang3/StringUtils", "capitalize" | "uncapitalize") => {
            if a(0).r() == 0 {
                return Some(V::NULL);
            }
            let t = text(rt, a(0));
            let mut chars = t.chars();
            let out = match chars.next() {
                Some(c) if name == "capitalize" => c.to_uppercase().chain(chars).collect(),
                Some(c) => c.to_lowercase().chain(chars).collect(),
                None => String::new(),
            };
            new_str(rt, out)
        }
        ("java/lang/Record", "<init>") => V::Void,
        // Vanilla's `Util`: `make(supplier)` is what the supplier gives; `make(object, consumer)`
        // hands the object to the consumer and returns it (`Util.make(new HashMap<>(), m -> ...)`).
        ("net/minecraft/Util", "make") if desc.starts_with("(Ljava/util/function/Supplier;") => {
            rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", a(0), &[])
        }
        ("net/minecraft/Util", "make") => {
            rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[a(0)]);
            a(0)
        }
        // `memoize(function)`: the function itself (same answers; vanilla only caches them).
        ("net/minecraft/Util", "memoize") => a(0),
        // `toMap()`: map entries collected into a map, `Collectors.toMap(Entry::getKey, Entry::getValue)`.
        ("net/minecraft/Util", "toMap") => {
            let k = crate::functions::method(rt, "java/util/Map$Entry.getKey.()Ljava/lang/Object;");
            let v = crate::functions::method(rt, "java/util/Map$Entry.getValue.()Ljava/lang/Object;");
            rt.alloc("compat:collect-map", Data::List(vec![k, v]))
        }
        // The data fixer's type for a block entity (`Builder.build(type)` takes it): none here, as
        // vanilla answers when the fixer doesn't know the id.
        ("net/minecraft/Util", "fetchChoiceType") => V::NULL,
        // `Util.makeDescriptionId("block", create:shaft)`: `block.create.shaft`, unregistered without an id.
        ("net/minecraft/Util", "makeDescriptionId") => {
            let kind = rt.str_of(a(0)).unwrap_or("").to_owned();
            let id = crate::api_common::text_of(rt, a(1));
            let text = match id.split_once(':') {
                _ if a(1).is_null() => format!("{kind}.unregistered_sadface"),
                Some((ns, path)) => format!("{kind}.{ns}.{}", path.replace('/', ".")),
                None => format!("{kind}.minecraft.{}", id.replace('/', ".")),
            };
            rt.string(&text)
        }
        ("net/minecraft/Util", "getMillis" | "getEpochMillis") => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
            V::J(now)
        }
        ("net/minecraft/Util", "getNanos") => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
            V::J(now)
        }
        _ => return None,
    })
}

/// How a `NumberFormat` prints: `DecimalFormat`'s digit counts, grouping, and the text around.
struct Digits {
    min_int: usize,
    min_frac: usize,
    max_frac: usize,
    grouping: bool,
    prefix: String,
    suffix: String,
    /// 100 for a percent pattern.
    multiplier: f64,
}

impl Digits {
    fn read(rt: &Rt, f: V) -> Digits {
        let n = |k: &'static str, d: i32| match rt.get_field(f, k) {
            V::I(i) => i,
            _ => d,
        };
        Digits {
            min_int: n("$minInt", 1).max(0) as usize,
            min_frac: n("$minFrac", 0).max(0) as usize,
            max_frac: n("$maxFrac", 3).max(0) as usize,
            grouping: n("$grouping", 1) != 0,
            prefix: rt.str_of(rt.get_field(f, "$prefix")).unwrap_or("").to_owned(),
            suffix: rt.str_of(rt.get_field(f, "$suffix")).unwrap_or("").to_owned(),
            multiplier: f64::from(n("$multiplier", 1)),
        }
    }

    /// `DecimalFormat`'s pattern (`#,##0.00`, `0.#%`): the positive part's digits and text.
    fn pattern(p: &str) -> Digits {
        let positive = p.split(';').next().unwrap_or("");
        let is_digit = |c: char| matches!(c, '#' | '0' | ',' | '.');
        let start = positive.find(is_digit).unwrap_or(positive.len());
        let end = positive.rfind(is_digit).map_or(start, |i| i + 1);
        let unquote = |t: &str| t.replace('\'', "");
        let (body, prefix, suffix) = (&positive[start..end], unquote(&positive[..start]), unquote(&positive[end..]));
        let (int, frac) = body.split_once('.').unwrap_or((body, ""));
        let percent = prefix.contains('%') || suffix.contains('%');
        Digits {
            min_int: int.chars().filter(|&c| c == '0').count(),
            min_frac: frac.chars().filter(|&c| c == '0').count(),
            max_frac: frac.chars().filter(|&c| c == '0' || c == '#').count(),
            grouping: int.contains(','),
            prefix,
            suffix,
            multiplier: if percent { 100.0 } else { 1.0 },
        }
    }

    fn store(&self, rt: &mut Rt, f: V) {
        rt.put_field(f, "$minInt", V::I(self.min_int as i32));
        rt.put_field(f, "$minFrac", V::I(self.min_frac as i32));
        rt.put_field(f, "$maxFrac", V::I(self.max_frac as i32));
        rt.put_field(f, "$grouping", V::I(self.grouping as i32));
        let (p, s) = (rt.string(&self.prefix), rt.string(&self.suffix));
        rt.put_field(f, "$prefix", p);
        rt.put_field(f, "$suffix", s);
        rt.put_field(f, "$multiplier", V::I(self.multiplier as i32));
    }

    /// `x` as `DecimalFormat.format` prints it in the root locale: rounded half to even on its
    /// exact value, trailing zeros past the minimum dropped, thousands grouped with `,`.
    fn format(&self, x: f64) -> String {
        let x = x * self.multiplier;
        if x.is_nan() {
            return "NaN".into();
        }
        let sign = if x.is_sign_negative() { "-" } else { "" };
        if x.is_infinite() {
            return format!("{}{sign}\u{221E}{}", self.prefix, self.suffix);
        }
        let fixed = format!("{:.*}", self.max_frac, x.abs());
        let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
        let mut frac = frac.to_owned();
        while frac.len() > self.min_frac && frac.ends_with('0') {
            frac.pop();
        }
        let int = int.trim_start_matches('0');
        let mut int = format!("{}{int}", "0".repeat(self.min_int.saturating_sub(int.len())));
        if int.is_empty() && frac.is_empty() {
            int.push('0');
        }
        if self.grouping {
            let digits: Vec<char> = int.chars().collect();
            int = digits
                .iter()
                .enumerate()
                .flat_map(|(i, c)| ((i > 0 && (digits.len() - i) % 3 == 0).then_some(',')).into_iter().chain(std::iter::once(*c)))
                .collect();
        }
        let point = if frac.is_empty() { "" } else { "." };
        format!("{}{sign}{int}{point}{frac}{}", self.prefix, self.suffix)
    }
}

/// `java.text.NumberFormat` and `DecimalFormat`, in the root locale's symbols (`.` and `,`): how
/// mods print quantities (Create's `LangNumberFormat`, `16,384`).
fn number_format(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let made = |rt: &mut Rt, d: Digits| {
        let f = rt.new_object("java/text/DecimalFormat");
        d.store(rt, f);
        f
    };
    let number = Digits { min_int: 1, min_frac: 0, max_frac: 3, grouping: true, prefix: String::new(), suffix: String::new(), multiplier: 1.0 };
    let set = |rt: &mut Rt, key: &'static str, v: V| {
        rt.put_field(a(0), key, v);
        V::Void
    };
    Some(match name {
        "getInstance" | "getNumberInstance" => made(rt, number),
        "getIntegerInstance" => made(rt, Digits { max_frac: 0, ..number }),
        "getPercentInstance" => made(rt, Digits { max_frac: 0, suffix: "%".into(), multiplier: 100.0, ..number }),
        "<init>" => {
            let d = if desc.starts_with("(Ljava/lang/String;") { Digits::pattern(&text(rt, a(1))) } else { number };
            d.store(rt, a(0));
            V::Void
        }
        "applyPattern" => {
            Digits::pattern(&text(rt, a(1))).store(rt, a(0));
            V::Void
        }
        "setMaximumFractionDigits" => {
            let n = a(1).i().max(0);
            if rt.get_field(a(0), "$minFrac").i() > n {
                rt.put_field(a(0), "$minFrac", V::I(n));
            }
            set(rt, "$maxFrac", V::I(n))
        }
        "setMinimumFractionDigits" => {
            let n = a(1).i().max(0);
            if Digits::read(rt, a(0)).max_frac < n as usize {
                rt.put_field(a(0), "$maxFrac", V::I(n));
            }
            set(rt, "$minFrac", V::I(n))
        }
        "setMinimumIntegerDigits" => set(rt, "$minInt", V::I(a(1).i().max(0))),
        "setGroupingUsed" => set(rt, "$grouping", V::I(i32::from(a(1).i() != 0))),
        "getMaximumFractionDigits" => V::I(Digits::read(rt, a(0)).max_frac as i32),
        "getMinimumFractionDigits" => V::I(Digits::read(rt, a(0)).min_frac as i32),
        "isGroupingUsed" => V::I(Digits::read(rt, a(0)).grouping as i32),
        // Rounding is always half even here, as `DecimalFormat`'s default.
        "setRoundingMode" | "setMaximumIntegerDigits" | "setParseIntegerOnly" | "setDecimalFormatSymbols" => V::Void,
        "format" => {
            let x = match a(1) {
                V::I(i) => f64::from(i),
                V::J(j) => j as f64,
                V::F(f) => f64::from(f),
                V::D(d) => d,
                v @ V::R(_) if v.r() != 0 => match rt.get_field(v, "value") {
                    V::I(i) => f64::from(i),
                    V::J(j) => j as f64,
                    V::F(f) => f64::from(f),
                    V::D(d) => d,
                    _ => return None,
                },
                _ => return None,
            };
            let s = Digits::read(rt, a(0)).format(x);
            new_str(rt, s)
        }
        _ => return None,
    })
}
