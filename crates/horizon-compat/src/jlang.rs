//! `java.lang` in native Rust: `String` (indexes and hashes in UTF-16 code units, as Java
//! counts them), `StringBuilder`, the boxed number classes, `Character`, `Math` and `Object`.
use crate::{Data, Rt, V};

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn from16(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

pub(crate) fn text(rt: &Rt, v: V) -> String {
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
    // A class of the mod's that lists none, a primitive or an array is no enum; nor is a class
    // the API was asked about before and had no constants for (a config library asks
    // `isEnum()` of every field's class, each time it reads one).
    if rt.is_mod_class(class) || !class.contains('/') || class.starts_with('[') {
        return None;
    }
    let known = format!("compat:not-an-enum/{class}");
    if rt.interned.contains_key(&known) {
        return None;
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
        rt.interned.insert(known, V::NULL);
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

/// `Enum.valueOf(type, name)`, which a mod enum's own `valueOf(String)` calls: the constant of
/// enum class `type` named `name`, as its `values()` lists them, else the JDK's exceptions.
fn enum_value_of(rt: &mut Rt, class: V, name: V) -> V {
    if class.r() == 0 {
        rt.throw_new("java/lang/NullPointerException");
        return V::NULL;
    }
    let c = rt.str_of(class).unwrap_or("").to_owned();
    let Some(values) = enum_constants(rt, &c) else {
        return throw_with(
            rt,
            "java/lang/IllegalArgumentException",
            &format!("{} is not an enum class", c.replace('/', ".")),
        );
    };
    let wanted = rt.str_of(name).map(str::to_owned);
    for e in crate::jutil::elements(rt, values) {
        let n = crate::api_common::enum_name(rt, e, false);
        if wanted.is_some() && rt.str_of(n) == wanted.as_deref() {
            return e;
        }
    }
    match wanted {
        None => throw_with(rt, "java/lang/NullPointerException", "Name is null"),
        // `getCanonicalName`: a nested enum's outer class joined with a dot.
        Some(w) => throw_with(
            rt,
            "java/lang/IllegalArgumentException",
            &format!("No enum constant {}.{w}", c.replace(['/', '$'], ".")),
        ),
    }
}

/// `Class.getCanonicalName()` of class `name` (internal form, or an array's descriptor): dotted, a
/// member class after its outer one; an array's is its element's with `[]`. None (Java's null)
/// for an anonymous or local class (`Outer$1`, `Outer$1Local`).
pub(crate) fn canonical_name(name: &str) -> Option<String> {
    if let Some(element) = name.strip_prefix('[') {
        let inner = match element.as_bytes().first()? {
            b'L' => canonical_name(element[1..].strip_suffix(';')?)?,
            b'[' => canonical_name(element)?,
            &b => match b {
                b'I' => "int",
                b'J' => "long",
                b'Z' => "boolean",
                b'B' => "byte",
                b'C' => "char",
                b'S' => "short",
                b'F' => "float",
                b'D' => "double",
                _ => return None,
            }
            .to_owned(),
        };
        return Some(inner + "[]");
    }
    if name
        .split('$')
        .skip(1)
        .any(|p| p.starts_with(|c: char| c.is_ascii_digit()))
    {
        return None;
    }
    Some(name.replace(['/', '$'], "."))
}

/// `Object.clone()`, which a `Cloneable` class's `super.clone()` reaches: a new object of the
/// same class holding the same fields (a shallow copy, as the JVM's); any other class throws
/// `CloneNotSupportedException` naming it.
fn object_clone(rt: &mut Rt, this: V) -> V {
    if this.r() == 0 {
        rt.throw_new("java/lang/NullPointerException");
        return V::NULL;
    }
    let class = rt.class_of(this);
    if !class.starts_with('[') && !rt.is_a(class, "java/lang/Cloneable") {
        return throw_with(rt, "java/lang/CloneNotSupportedException", &class.replace('/', "."));
    }
    let Some((cid, data)) = rt.heap.get(this.r() as usize).map(|o| (o.cid, o.data.clone())) else { return V::NULL };
    let side = rt.side_fields.get(&this.r()).cloned();
    let copy = rt.alloc_cid(class, cid, data);
    if let Some(f) = side {
        rt.side_fields.insert(copy.r(), f);
    }
    copy
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
pub(crate) fn descriptor_of(name: &str) -> String {
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
    Some(
        typed_fields(rt, class)?
            .into_iter()
            .map(|(access, name, desc, _)| (access, name, desc))
            .collect(),
    )
}

/// A mod class's declared fields: access, name, descriptor, and the generic signature where the
/// field has one (`Ljava/util/List<Lcom/example/Entry;>;`). None for a class that isn't the mod's.
pub(crate) fn typed_fields(
    rt: &mut Rt,
    class: &str,
) -> Option<Vec<(u16, String, String, Option<String>)>> {
    if !rt.is_mod_class(class) {
        return None;
    }
    let Some(v) = rt.reflect(class, "fields", &[]) else { return Some(Vec::new()) };
    let lines = rt.str_of(v).unwrap_or("").to_owned();
    Some(
        lines
            .lines()
            // `access name descriptor [signature]`; a name in angle brackets is no field (the
            // class's own signature).
            .filter_map(|l| {
                let mut p = l.splitn(4, ' ');
                Some((
                    p.next()?.parse().ok()?,
                    p.next()?.to_owned(),
                    p.next()?.to_owned(),
                    p.next().map(str::to_owned),
                ))
            })
            .filter(|f: &(u16, String, String, Option<String>)| !f.1.starts_with('<'))
            .collect(),
    )
}

/// The class a field descriptor names, as `Class.getName` has it (`I` is `int`, `Lfoo/Bar;`
/// `foo/Bar`, an array its descriptor).
pub(crate) fn descriptor_class(desc: &str) -> &str {
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
pub(crate) fn find_field(rt: &mut Rt, class: &str, name: &str) -> Option<(u16, String)> {
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
pub(crate) fn field_object(rt: &mut Rt, class: V, name: V) -> V {
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
pub(crate) fn field_value(rt: &mut Rt, field: V, obj: V) -> V {
    let (class, name, _) = field_parts(rt, field);
    if obj.r() != 0 {
        return rt.get_field(obj, name);
    }
    static_of(rt, &class, name)
}

/// Static `name` of class `class` (internal form): a mod class's once its class is initialised
/// (an enum's constant), else the API's.
pub(crate) fn static_of(rt: &mut Rt, class: &str, name: &str) -> V {
    let key = rt.static_key(&format!("{class}.{name}"));
    if rt.is_mod_class(class) {
        rt.reflect(class, "<clinit>", &[]);
        return rt.statics.get(key).copied().unwrap_or(V::NULL);
    }
    crate::api::get_static(rt, key)
}

/// `field = v` in `obj`, or the class's static given no object; a box set into a field holding
/// a primitive is unboxed.
pub(crate) fn field_store(rt: &mut Rt, field: V, obj: V, v: V) {
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
pub(crate) fn box_raw(rt: &mut Rt, v: V) -> V {
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
pub(crate) fn return_kind(desc: &str) -> char {
    desc.rsplit(')').next().and_then(|r| r.chars().next()).unwrap_or('V')
}

/// `method.invoke(obj, args)` on one of the mod's objects: the instance method its class has by
/// that name and parameter types, given its arguments unboxed, its result boxed. None (a miss) for
/// a static method or one no mod class has.
pub(crate) fn invoke_method(rt: &mut Rt, method: V, obj: V, args: &[V]) -> Option<V> {
    let class = text(rt, rt.get_field(method, "$class")).replace('.', "/");
    let name = text(rt, rt.get_field(method, "$name"));
    let params = rt.get_field(method, "$params");
    let mut prefix = format!("{name}.(");
    for p in crate::jutil::elements(rt, params) {
        prefix.push_str(&descriptor_of(rt.str_of(p).unwrap_or("java/lang/Object")));
    }
    prefix.push(')');
    // A method of the game's or of a library's, private ones included (Moonlight loads a config
    // early through FML's own `ConfigTracker.loadConfig`): the runtime's answer for it, where it
    // has one. Only the parameters are known of such a method, so it is asked for as one that
    // returns nothing, then as one that returns an object; its receiver goes first when given.
    if !class.is_empty() && !rt.is_mod_class(&class) {
        let mut all = Vec::with_capacity(args.len() + 1);
        if obj.r() != 0 {
            all.push(obj);
        }
        for (v, k) in args
            .iter()
            .zip(kinds(&prefix[prefix.find('(').unwrap_or(0)..]))
        {
            all.push(crate::mixin::unbox(rt, *v, k));
        }
        for returned in ["V", "Ljava/lang/Object;"] {
            let key = rt.static_key(&format!("{class}.{prefix}{returned}"));
            let before = rt.unmapped.get(key).copied().unwrap_or(0);
            let r = crate::api::call(rt, key, &all);
            if rt.unmapped.get(key).copied().unwrap_or(0) == before {
                return Some(if matches!(r, V::Void) { V::NULL } else { r });
            }
            // Not answered: the miss is this lookup's, not the mod's call.
            match rt.unmapped.get_mut(key) {
                Some(n) if *n > 1 => *n -= 1,
                _ => {
                    rt.unmapped.remove(key);
                }
            }
        }
        return None;
    }
    if obj.r() == 0 {
        return None;
    }
    let short = rt.vkeys.range(prefix.as_str()..).next().copied().filter(|k| k.starts_with(&prefix))?;
    let mut all = Vec::with_capacity(args.len());
    for (v, k) in args.iter().zip(kinds(&short[short.find('(').unwrap_or(0)..])) {
        all.push(crate::mixin::unbox(rt, *v, k));
    }
    let key = rt.static_key(&format!("{class}.{short}"));
    let r = rt.call_functional(key, obj, &all);
    Some(match return_kind(short) {
        'V' => V::NULL,
        'L' | '[' => r,
        kind => box_as(rt, r, kind),
    })
}

/// A primitive boxed as what declares it says (`kind`: a field's or an array's type, a
/// method's return type): a `boolean` as a `Boolean`, a `char` as a `Character`. The runtime
/// holds those, a `byte` and a `short` as an int, which alone says `Integer`.
fn box_as(rt: &mut Rt, v: V, kind: char) -> V {
    match (v, kind) {
        (V::I(_), 'Z' | 'B' | 'S' | 'C') => crate::mixin::box_value(rt, v, kind),
        _ => box_raw(rt, v),
    }
}

/// The constructors and methods class `class` declares, as the bundle's table lists them
/// ([`crate::MEMBERS`]): access, name (`<init>` for a constructor) and descriptor. None listed
/// for a class that isn't the mods'.
fn members(rt: &mut Rt, class: &str) -> Vec<(u16, String, String)> {
    let Some(lines) = rt.reflect(crate::MEMBERS, class, &[]) else {
        return Vec::new();
    };
    let lines = rt.str_of(lines).unwrap_or("").to_owned();
    lines
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ' ');
            Some((
                parts.next()?.parse().ok()?,
                parts.next()?.to_owned(),
                parts.next()?.to_owned(),
            ))
        })
        .collect()
}

/// The classes of a descriptor's parameters, as an array (`getParameterTypes`).
fn parameter_classes(rt: &mut Rt, desc: &str) -> V {
    let list = desc
        .strip_prefix('(')
        .and_then(|d| d.split(')').next())
        .unwrap_or("");
    let (mut out, mut at) = (Vec::new(), 0);
    while at < list.len() {
        let rest = &list[at..];
        let dims = rest.len() - rest.trim_start_matches('[').len();
        let len = dims
            + if rest[dims..].starts_with('L') {
                rest[dims..].find(';').map_or(rest.len() - dims, |i| i + 1)
            } else {
                1
            };
        out.push(rt.class_literal(descriptor_class(&rest[..len])));
        at += len;
    }
    rt.alloc("[Ljava/lang/Class;", Data::Array(out))
}

/// A `Constructor` of class object `class` with descriptor `desc`.
fn constructor_object(rt: &mut Rt, class: V, desc: &str) -> V {
    let c = rt.new_object("java/lang/reflect/Constructor");
    rt.put_field(c, "$class", class);
    let d = rt.string(desc);
    rt.put_field(c, "$desc", d);
    c
}

/// A `Method` of class object `class`, as a class file lists it: its name, its parameters'
/// classes, and its descriptor and access for what asks them.
fn method_object(rt: &mut Rt, class: V, name: &str, desc: &str) -> V {
    let m = rt.new_object("java/lang/reflect/Method");
    let (n, params, d) = (
        rt.string(name),
        parameter_classes(rt, desc),
        rt.string(desc),
    );
    let access = members_access(rt, class, name, desc);
    for (field, v) in [
        ("$class", class),
        ("$name", n),
        ("$params", params),
        ("$desc", d),
        ("$access", V::I(access)),
    ] {
        rt.put_field(m, field, v);
    }
    m
}

fn members_access(rt: &mut Rt, class: V, name: &str, desc: &str) -> i32 {
    let class = rt.str_of(class).unwrap_or("").replace('.', "/");
    members(rt, &class)
        .into_iter()
        .find(|m| m.1 == name && m.2 == desc)
        .map_or(1, |m| i32::from(m.0))
}

/// A file of a mod's jar at `path` ([`crate::resources::jar_file`]) as a stream of its bytes, or
/// (`url`) as the URL that opens one; null for a file no jar has.
pub(crate) fn jar_resource(rt: &mut Rt, path: &str, url: bool) -> V {
    let Some(bytes) = crate::resources::jar_file(rt, path) else {
        return V::NULL;
    };
    if !url {
        return crate::resources::input_stream(rt, bytes);
    }
    let u = rt.new_object("java/net/URL");
    let spec = rt.string(&format!("modjar:/{}", path.trim_start_matches('/')));
    rt.put_field(u, "$spec", spec);
    u
}

/// `new C(args)` by reflection: the mod's constructor with descriptor `desc`.
pub(crate) fn construct(rt: &mut Rt, class: &str, desc: &str, args: &[V]) -> V {
    match rt.reflect(class, &format!("<init>.{desc}"), args) {
        Some(o) => return o,
        None if rt.thrown.is_some() => return V::NULL,
        None => {}
    }
    // A class of the game's or a library's (`LevelResource(String)` made by reflection): its
    // constructor as a mod's `new` calls it, where the runtime answers that.
    if !rt.is_mod_class(class) {
        let key = rt.static_key(&format!("{class}.<init>.{desc}"));
        let (owner, before) = (
            rt.static_key(class),
            rt.unmapped.get(key).copied().unwrap_or(0),
        );
        let object = rt.new_object(owner);
        let mut all = vec![object];
        all.extend_from_slice(args);
        crate::api::call(rt, key, &all);
        if rt.unmapped.get(key).copied().unwrap_or(0) == before {
            return if rt.thrown.is_some() { V::NULL } else { object };
        }
        // Not answered: the miss is this lookup's, not the mod's call.
        match rt.unmapped.get_mut(key) {
            Some(n) if *n > 1 => *n -= 1,
            _ => {
                rt.unmapped.remove(key);
            }
        }
    }
    throw_with(
        rt,
        "java/lang/NoSuchMethodException",
        &format!("{}.<init>{desc}", class.replace('/', ".")),
    )
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

/// `String.compareTo`: the difference of the first UTF-16 units that differ, else of the lengths.
/// `String.compareTo`: the difference of the first two UTF-16 units that differ, else of the
/// lengths. Nothing is copied for it: a sorted map compares its keys at every step.
pub(crate) fn java_compare(x: &str, y: &str) -> i32 {
    // What the two start with alike, as far as it is ASCII, is alike in UTF-16 too.
    let same = x
        .bytes()
        .zip(y.bytes())
        .take_while(|(a, b)| a == b && a.is_ascii())
        .count();
    let (mut a, mut b) = (x[same..].encode_utf16(), y[same..].encode_utf16());
    loop {
        match (a.next(), b.next()) {
            (Some(p), Some(q)) if p == q => {}
            (Some(p), Some(q)) => return i32::from(p) - i32::from(q),
            (Some(_), None) => return 1 + a.count() as i32,
            (None, Some(_)) => return -1 - b.count() as i32,
            (None, None) => return 0,
        }
    }
}

/// `String.compareToIgnoreCase`, which `String.CASE_INSENSITIVE_ORDER` compares by: character
/// by character, the first two that differ upper-cased and then lower-cased too, their difference;
/// else that of the lengths.
pub(crate) fn compare_ignore_case(x: &str, y: &str) -> i32 {
    // Java's mapping of one character to one: a character whose other case is several is itself.
    fn one(mut mapped: impl Iterator<Item = char>, c: char) -> char {
        match (mapped.next(), mapped.next()) {
            (Some(m), None) => m,
            _ => c,
        }
    }
    for (a, b) in x.chars().zip(y.chars()) {
        if a == b {
            continue;
        }
        let (a, b) = (one(a.to_uppercase(), a), one(b.to_uppercase(), b));
        if a != b {
            let (a, b) = (one(a.to_lowercase(), a), one(b.to_lowercase(), b));
            if a != b {
                return a as i32 - b as i32;
            }
        }
    }
    x.encode_utf16().count() as i32 - y.encode_utf16().count() as i32
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
        || owner == "org/apache/commons/text/WordUtils"
        || owner.ends_with("Pair")
        || owner.ends_with("Triple")
        || matches!(
            owner,
            "java/util/ServiceLoader"
                | PROVIDER
                | PRINT_STREAM
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
    if owner == PRINT_STREAM {
        return print_stream(rt, name, desc, args);
    }
    if !owner.starts_with("java/lang/") {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ks = kinds(desc);
    match owner {
        "java/lang/String" | "java/lang/CharSequence" => string(rt, name, desc, &ks, args),
        "java/lang/StringBuilder" | "java/lang/StringBuffer" | "java/lang/AbstractStringBuilder" => builder(rt, name, desc, &ks, args),
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
            // `Boolean.valueOf(String)`: the text read as `parseBoolean` reads it, boxed.
            "valueOf" if desc.starts_with("(Ljava/lang/String;)") => {
                let on = rt
                    .str_of(a(0))
                    .is_some_and(|s| s.eq_ignore_ascii_case("true"));
                crate::api_common::boolean(rt, on)
            }
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
                // By the character's general category, as Java's: a digit of any script, a
                // letter of any (`Alphabetic` is wider: marks and letter numbers too).
                "isDigit" => V::I((general_category(a(0).i()) == 9) as i32),
                "isLetter" => V::I(matches!(general_category(a(0).i()), 1..=5) as i32),
                "isAlphabetic" => V::I(c.is_alphabetic() as i32),
                "isLetterOrDigit" => V::I(matches!(general_category(a(0).i()), 1..=5 | 9) as i32),
                "getType" => V::I(i32::from(general_category(a(0).i()))),
                "isDefined" => V::I((general_category(a(0).i()) != 0) as i32),
                "isTitleCase" => V::I((general_category(a(0).i()) == 3) as i32),
                "isISOControl" => V::I(matches!(a(0).i(), 0..=0x1F | 0x7F..=0x9F) as i32),
                "isIdentifierIgnorable" => V::I(identifier_ignorable(a(0).i()) as i32),
                "isJavaIdentifierStart" | "isJavaLetter" => {
                    V::I(java_identifier(a(0).i(), false) as i32)
                }
                "isJavaIdentifierPart" | "isJavaLetterOrDigit" => {
                    V::I(java_identifier(a(0).i(), true) as i32)
                }
                "isUnicodeIdentifierStart" => V::I(unicode_identifier(a(0).i(), false) as i32),
                "isUnicodeIdentifierPart" => V::I(unicode_identifier(a(0).i(), true) as i32),
                "isBmpCodePoint" => V::I((a(0).i() >> 16 == 0) as i32),
                "isSupplementaryCodePoint" => V::I((0x10000..=0x10FFFF).contains(&a(0).i()) as i32),
                "isValidCodePoint" => V::I((0..=0x10FFFF).contains(&a(0).i()) as i32),
                "isWhitespace" => V::I(java_whitespace(c) as i32),
                "isSpaceChar" => V::I((c.is_whitespace() && !c.is_control()) as i32),
                "isUpperCase" => V::I(c.is_uppercase() as i32),
                "isLowerCase" => V::I(c.is_lowercase() as i32),
                "toUpperCase" => V::I(c.to_uppercase().next().unwrap_or(c) as i32),
                "toLowerCase" => V::I(c.to_lowercase().next().unwrap_or(c) as i32),
                "getNumericValue" => V::I(c.to_digit(36).map_or(-1, |d| d as i32)),
                // `digit(c, radix)`: -1 for a radix out of 2..=36 or a character that isn't one of
                // its digits.
                "digit" => V::I(
                    u32::try_from(a(1).i())
                        .ok()
                        .filter(|r| (2..=36).contains(r))
                        .and_then(|r| c.to_digit(r))
                        .map_or(-1, |d| d as i32),
                ),
                "forDigit" => {
                    let (d, r) = (a(0).i(), a(1).i());
                    V::I(if (2..=36).contains(&r) && (0..r).contains(&d) {
                        char::from_digit(d as u32, r as u32).map_or(0, |c| c as i32)
                    } else {
                        0
                    })
                }
                // A code point's UTF-16 units; one outside Unicode throws.
                "toChars" => {
                    let cp = a(0).i();
                    let Some(ch) = u32::try_from(cp).ok().filter(|&u| u <= 0x10FFFF) else {
                        return Some(throw_with(
                            rt,
                            "java/lang/IllegalArgumentException",
                            &format!("Not a valid Unicode code point: 0x{:X}", cp as u32),
                        ));
                    };
                    let units: Vec<V> = if ch < 0x10000 {
                        vec![V::I(ch as i32)]
                    } else {
                        let s = ch - 0x10000;
                        vec![
                            V::I((0xD800 + (s >> 10)) as i32),
                            V::I((0xDC00 + (s & 0x3FF)) as i32),
                        ]
                    };
                    rt.alloc("[C", Data::Array(units))
                }
                // Unchecked, as the JDK computes it.
                "toCodePoint" => V::I(
                    (a(0).i() << 10)
                        .wrapping_add(a(1).i())
                        .wrapping_add(0x10000 - (0xD800 << 10) - 0xDC00),
                ),
                "charCount" => V::I(if a(0).i() >= 0x10000 { 2 } else { 1 }),
                "isHighSurrogate" => V::I((0xD800..0xDC00).contains(&a(0).i()) as i32),
                "isLowSurrogate" => V::I((0xDC00..0xE000).contains(&a(0).i()) as i32),
                "isSurrogate" => V::I((0xD800..0xE000).contains(&a(0).i()) as i32),
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
        // `Math.random()`: uniform in [0, 1), from the runtime's random stream (53 bits, as
        // `Random.nextDouble` gives).
        "java/lang/Math" | "java/lang/StrictMath" if name == "random" => Some(V::D(
            (crate::api_common::next_random(rt) >> 11) as f64 * (1.0 / (1u64 << 53) as f64),
        )),
        "java/lang/Math" | "java/lang/StrictMath" => integer_math(rt, name, desc, args).or_else(|| math(name, &ks, args)),
        "java/lang/Object" => Some(match name {
            // A vanilla block's or item's is its 1.21.1 class (`IceBlock`), as `instanceof` answers.
            // A lambda's is its own (the JVM makes one for each): it names the method that is
            // its body, which is how its parameter's type is found (`TypeResolver`).
            "getClass" => {
                let c = if rt.cid(a(0)) & crate::LAMBDA != 0 {
                    rt.class_of(a(0))
                } else {
                    rt.type_of(a(0))
                };
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
            // One thread runs a runtime: nobody waits on a monitor to be woken.
            "notify" | "notifyAll" => V::Void,
            "clone" => object_clone(rt, a(0)),
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
            // No `Unsafe` here: JOML is told so by its own switch, and copies its numbers field
            // by field.
            "getProperty" if rt.str_of(a(0)) == Some("joml.nounsafe") => rt.string("true"),
            // No system properties or environment here: a property's default, else null.
            "getProperty" if ks.len() == 2 => a(1),
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
            "valueOf" => enum_value_of(rt, a(0), a(1)),
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
                match fields_array(rt, &n, name == "getFields") {
                    Some(fields) => fields,
                    // A class of Java's own (a box, a string, an array, a primitive) lists none:
                    // none a mod looks for there, its annotated ones (a config library asks it
                    // of the class of every value it walks).
                    None if n.starts_with("java/") || n.starts_with('[') || !n.contains('/') => {
                        rt.alloc("[Ljava/lang/reflect/Field;", Data::Array(Vec::new()))
                    }
                    None => return None,
                }
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
            "getSuperclass" | "getGenericSuperclass" => {
                let n = rt.str_of(a(0)).unwrap_or("").to_owned();
                // With its type arguments, where the class names any for it.
                if name == "getGenericSuperclass"
                    && let Some(up) = crate::jtypes::generic_superclass(rt, &n)
                {
                    return Some(up);
                }
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
            // A file of the mod's jar: by its path from the jar's root (`/defaults.json`), or
            // from the class's package. Null for one no jar has.
            "getResourceAsStream" | "getResource" => {
                let class = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                let asked = text(rt, a(1));
                let path = match asked.strip_prefix('/') {
                    Some(rooted) => rooted.to_owned(),
                    None => format!(
                        "{}{asked}",
                        class
                            .rsplit_once('/')
                            .map_or(String::new(), |p| format!("{}/", p.0))
                    ),
                };
                jar_resource(rt, &path, name == "getResource")
            }
            // The class's constructors and methods, as its class file declares them (`getMethods`
            // and `getConstructors`: the public ones, a superclass's methods among them).
            "getConstructors" | "getDeclaredConstructors" | "getDeclaredMethods" | "getMethods" => {
                let class = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                let (constructors, public) = (
                    name.ends_with("Constructors"),
                    matches!(name, "getConstructors" | "getMethods"),
                );
                let mut out = Vec::new();
                let mut at = Some(class.clone());
                while let Some(c) = at {
                    for (access, member, desc) in members(rt, &c) {
                        if (member == "<init>") != constructors || (public && access & 0x0001 == 0)
                        {
                            continue;
                        }
                        let owner = rt.class_literal(&c);
                        out.push(if constructors {
                            constructor_object(rt, owner, &desc)
                        } else {
                            method_object(rt, owner, &member, &desc)
                        });
                    }
                    // Only `getMethods` goes up, and only through the mod's own classes.
                    at = rt
                        .supers
                        .get(c.as_str())
                        .copied()
                        .filter(|s| name == "getMethods" && rt.is_mod_class(s))
                        .map(str::to_owned);
                }
                let array = if constructors {
                    "[Ljava/lang/reflect/Constructor;"
                } else {
                    "[Ljava/lang/reflect/Method;"
                };
                rt.alloc(array, Data::Array(out))
            }
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
            // The interfaces a mod's class names, its package, and the class it is nested in.
            "getInterfaces" | "getGenericInterfaces" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                if name == "getGenericInterfaces"
                    && let Some(all) = crate::jtypes::generic_interfaces(rt, &n)
                {
                    return Some(all);
                }
                let names: Vec<&'static str> = rt
                    .interfaces
                    .get(n.as_str())
                    .map(|i| i.to_vec())
                    .unwrap_or_default();
                let classes: Vec<V> = names.into_iter().map(|i| class_object(rt, i)).collect();
                rt.alloc("[Ljava/lang/Class;", Data::Array(classes))
            }
            "getTypeParameters" => {
                let n = rt.str_of(a(0)).unwrap_or("").to_owned();
                crate::jtypes::type_parameters(rt, &n)
            }
            "getPackage" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('/', ".");
                let package = n.rsplit_once('.').map_or("", |p| p.0).to_owned();
                let p = rt.new_object("java/lang/Package");
                let name = rt.string(&package);
                rt.put_field(p, "$name", name);
                p
            }
            "getEnclosingClass" | "getDeclaringClass" => {
                let n = rt.str_of(a(0)).unwrap_or("").to_owned();
                match n.rsplit_once('$') {
                    Some((outer, _)) if !outer.is_empty() => class_object(rt, outer),
                    _ => V::NULL,
                }
            }
            "isAnnotation" | "isAnonymousClass" | "isSynthetic" | "isLocalClass" => V::I(0),
            "isMemberClass" => V::I(rt.str_of(a(0)).is_some_and(|n| n.contains('$')) as i32),
            // A record of a mod's: its class extends `Record`, and its components are its
            // fields, in their order, each read by the method of its name.
            "isRecord" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                V::I((rt.supers.get(n.as_str()).copied() == Some("java/lang/Record")) as i32)
            }
            "getRecordComponents" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                if rt.supers.get(n.as_str()).copied() != Some("java/lang/Record") {
                    return Some(V::NULL);
                }
                let mut components = Vec::new();
                for (access, name, desc, signature) in typed_fields(rt, &n).unwrap_or_default() {
                    if access & 0x0008 != 0 {
                        continue;
                    }
                    let c = rt.new_object("java/lang/reflect/RecordComponent");
                    let (name, desc, signature) = (
                        rt.string(&name),
                        rt.string(&desc),
                        signature.map_or(V::NULL, |s| rt.string(&s)),
                    );
                    for (field, v) in [
                        ("$class", a(0)),
                        ("$name", name),
                        ("$desc", desc),
                        ("$signature", signature),
                    ] {
                        rt.put_field(c, field, v);
                    }
                    components.push(c);
                }
                rt.alloc(
                    "[Ljava/lang/reflect/RecordComponent;",
                    Data::Array(components),
                )
            }
            "isInterface" => V::I(0),
            // Every class a mod can name is one it may use: public.
            "getModifiers" => V::I(1),
            // A mod class's annotations (`@Inherited` ones from its superclasses unless asked
            // for the declared ones).
            "isAnnotationPresent" | "getAnnotation" | "getDeclaredAnnotation" | "getAnnotations" | "getDeclaredAnnotations" | "getAnnotationsByType" | "getDeclaredAnnotationsByType" => {
                let n = rt.str_of(a(0)).unwrap_or("").replace('.', "/");
                let found = crate::annotations::on_class(rt, &n, !name.starts_with("getDeclared"));
                crate::annotations::query(rt, name, found, a(1))?
            }
            _ => return None,
        }),
        // `java.lang.reflect.Array`: an array made, measured, read and written by reflection
        // (Fabric's events keep their listeners in `Array.newInstance(type, 0)`).
        "java/lang/reflect/Array" => Some(match name {
            "newInstance" => {
                // A primitive's array starts as zeros of its kind, any other as nulls.
                let zero = match rt.str_of(a(0)).unwrap_or("") {
                    "int" | "short" | "byte" | "char" | "boolean" => V::I(0),
                    "long" => V::J(0),
                    "float" => V::F(0.0),
                    "double" => V::D(0.0),
                    _ => V::NULL,
                };
                let dims: Vec<i32> = if desc.starts_with("(Ljava/lang/Class;I)") {
                    vec![a(1).i()]
                } else {
                    crate::jutil::elements(rt, a(1))
                        .iter()
                        .map(|d| d.i())
                        .collect()
                };
                fn make(rt: &mut Rt, dims: &[i32], zero: V) -> V {
                    let Some((&len, rest)) = dims.split_first() else {
                        return V::NULL;
                    };
                    let array = rt.new_array(len, if rest.is_empty() { zero } else { V::NULL });
                    for i in 0..len.max(0) {
                        if !rest.is_empty() {
                            let inner = make(rt, rest, zero);
                            rt.array_set(array, i, inner);
                        }
                    }
                    array
                }
                make(rt, &dims, zero)
            }
            "getLength" => V::I(rt.array_len(a(0))),
            "get" => {
                let v = rt.array_get(a(0), a(1).i());
                let kind = rt.class_of(a(0)).chars().nth(1).unwrap_or('L');
                box_as(rt, v, kind)
            }
            "getInt" | "getLong" | "getFloat" | "getDouble" | "getBoolean" | "getByte"
            | "getChar" | "getShort" => rt.array_get(a(0), a(1).i()),
            // A box set into a primitive's array is unboxed, as reflection does.
            "set" => {
                let v = match rt.array_get(a(0), a(1).i()) {
                    V::I(_) => crate::mixin::unbox(rt, a(2), 'I'),
                    V::J(_) => crate::mixin::unbox(rt, a(2), 'J'),
                    V::F(_) => crate::mixin::unbox(rt, a(2), 'F'),
                    V::D(_) => crate::mixin::unbox(rt, a(2), 'D'),
                    _ => a(2),
                };
                rt.array_set(a(0), a(1).i(), v);
                V::Void
            }
            "setInt" | "setLong" | "setFloat" | "setDouble" | "setBoolean" | "setByte"
            | "setChar" | "setShort" => {
                rt.array_set(a(0), a(1).i(), a(2));
                V::Void
            }
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
            "getParameterCount" => V::I(kinds(&text(rt, rt.get_field(a(0), "$desc"))).len() as i32),
            "getParameterTypes" => {
                let desc = text(rt, rt.get_field(a(0), "$desc"));
                parameter_classes(rt, &desc)
            }
            "getModifiers" => {
                let (class, desc) = (
                    text(rt, rt.get_field(a(0), "$class")).replace('.', "/"),
                    text(rt, rt.get_field(a(0), "$desc")),
                );
                let access = members(rt, &class)
                    .into_iter()
                    .find(|m| m.1 == "<init>" && m.2 == desc)
                    .map_or(1, |m| m.0);
                V::I(i32::from(access & 0x0007))
            }
            _ => return None,
        }),
        // A field a mod looks up by name: reading it reads the object's field, or with no object
        // the class's static (the class initialised first); `get` boxes a primitive, `set` unboxes
        // into a field that holds one, as Java's do.
        "java/lang/reflect/RecordComponent" => Some(match name {
            "getName" => rt.get_field(a(0), "$name"),
            "getDeclaringRecord" => rt.get_field(a(0), "$class"),
            "getGenericSignature" => rt.get_field(a(0), "$signature"),
            "getType" => {
                let desc = text(rt, rt.get_field(a(0), "$desc"));
                rt.class_literal(descriptor_class(&desc))
            }
            // With its type arguments where the record declares any (`List<Entry>`).
            "getGenericType" => {
                let sig = match rt.str_of(rt.get_field(a(0), "$signature")) {
                    Some(sig) if !sig.is_empty() => sig.to_owned(),
                    _ => text(rt, rt.get_field(a(0), "$desc")),
                };
                crate::jtypes::of(rt, &sig)
            }
            // The component's accessor: the method of its name, with no parameters.
            "getAccessor" => {
                let m = rt.new_object("java/lang/reflect/Method");
                let (class, name, none) = (
                    rt.get_field(a(0), "$class"),
                    rt.get_field(a(0), "$name"),
                    rt.new_array(0, V::NULL),
                );
                for (field, v) in [("$class", class), ("$name", name), ("$params", none)] {
                    rt.put_field(m, field, v);
                }
                m
            }
            "isAnnotationPresent" => V::I(0),
            "getAnnotation" | "getDeclaredAnnotation" => V::NULL,
            _ => return None,
        }),
        "java/lang/reflect/Field" => Some(match name {
            "getName" => rt.get_field(a(0), "$name"),
            // What a mod class's field holds and its modifiers, as the class declares it.
            "getType" => {
                let (class, name, _) = field_parts(rt, a(0));
                let (_, desc) = find_field(rt, &class, name)?;
                rt.class_literal(descriptor_class(&desc))
            }
            // The field's type with its type arguments, where the class declares any.
            "getGenericType" => {
                let (class, name, _) = field_parts(rt, a(0));
                let mut at = class.clone();
                let sig = loop {
                    let fields = typed_fields(rt, &at)?;
                    if let Some(f) = fields.into_iter().find(|f| f.1 == name) {
                        break f.3.unwrap_or(f.2);
                    }
                    at = rt.supers.get(at.as_str()).copied()?.to_owned();
                };
                crate::jtypes::of(rt, &sig)
            }
            "getModifiers" => {
                let (class, name, _) = field_parts(rt, a(0));
                match find_field(rt, &class, name) {
                    Some((access, _)) => V::I(i32::from(access & 0x00df)),
                    // A field of the game's or a library's class, which carries no table here:
                    // by its name's shape, a constant (`LOGGER`: public static final) or a
                    // public field.
                    None if !rt.is_mod_class(&class) => {
                        let constant = name.chars().any(|c| c.is_ascii_uppercase())
                            && !name.chars().any(|c| c.is_ascii_lowercase());
                        V::I(if constant { 0x19 } else { 0x1 })
                    }
                    None => return None,
                }
            }
            "getDeclaringClass" => rt.get_field(a(0), "$class"),
            "setAccessible" => V::Void,
            "trySetAccessible" | "canAccess" => V::I(1),
            "isAnnotationPresent" | "getAnnotation" | "getDeclaredAnnotation" | "getAnnotations" | "getDeclaredAnnotations" | "getAnnotationsByType" | "getDeclaredAnnotationsByType" => {
                let (class, field, _) = field_parts(rt, a(0));
                let found = crate::annotations::on_field(rt, &class, field);
                crate::annotations::query(rt, name, found, a(1))?
            }
            "get" => {
                let v = field_value(rt, a(0), a(1));
                let (class, name, _) = field_parts(rt, a(0));
                let kind = find_field(rt, &class, name).and_then(|(_, desc)| desc.chars().next());
                box_as(rt, v, kind.unwrap_or('L'))
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
            // What the class file says of it, where the method was listed (`getDeclaredMethods`).
            "getModifiers" | "getReturnType" | "getGenericReturnType" => {
                let desc = text(rt, rt.get_field(a(0), "$desc"));
                if desc.is_empty() {
                    return None;
                }
                if name == "getModifiers" {
                    V::I(rt.get_field(a(0), "$access").i() & 0x053f)
                } else {
                    let returned = desc.rsplit(')').next().unwrap_or("V").to_owned();
                    rt.class_literal(descriptor_class(&returned))
                }
            }
            "getParameterTypes" | "getGenericParameterTypes" => {
                let params = rt.get_field(a(0), "$params");
                let classes = crate::jutil::elements(rt, params);
                rt.alloc("[Ljava/lang/Class;", Data::Array(classes))
            }
            "isSynthetic" | "isBridge" => {
                V::I((rt.get_field(a(0), "$access").i() & 0x1040 != 0) as i32)
            }
            "isDefault" | "isVarArgs" => V::I(0),
            "isAnnotationPresent" | "getAnnotation" | "getDeclaredAnnotation" | "getAnnotations" | "getDeclaredAnnotations" | "getAnnotationsByType" | "getDeclaredAnnotationsByType" => {
                let class = text(rt, rt.get_field(a(0), "$class")).replace('.', "/");
                let method = text(rt, rt.get_field(a(0), "$name"));
                let mut params = String::from("(");
                for p in crate::jutil::elements(rt, rt.get_field(a(0), "$params")) {
                    params.push_str(&descriptor_of(rt.str_of(p).unwrap_or("java/lang/Object")));
                }
                params.push(')');
                let found = crate::annotations::on_method(rt, &class, &method, &params);
                crate::annotations::query(rt, name, found, a(1))?
            }
            _ => return None,
        }),
        o if o.starts_with("java/lang/invoke/") => crate::invoke::call(rt, o, name, desc, args),

        // One runtime runs on one game thread, so it is the same `Thread` each time it asks: ESL's
        // transactions (Create: New Age's energy) check `this.thread != Thread.currentThread()`.
        // A thread a mod starts runs its body there at once.
        "java/lang/Thread" => crate::jsystem::thread(rt, name, desc, args),
        // The mods' class loader: one for all of them. It finds the files of their jars by their
        // paths from a jar's root, and a class by its name as `Class.forName` does.
        "java/lang/ClassLoader" => Some(match name {
            "getSystemClassLoader" | "getPlatformClassLoader" | "getParent" => rt
                .intern("compat:class-loader".into(), |rt| {
                    rt.new_object("java/lang/ClassLoader")
                }),
            "getResourceAsStream"
            | "getResource"
            | "getSystemResourceAsStream"
            | "getSystemResource" => {
                let path = text(rt, a(usize::from(!name.starts_with("getSystem"))));
                jar_resource(rt, &path, name.ends_with("Resource"))
            }
            "getResources" | "resources" => {
                let path = text(rt, a(1));
                let found = jar_resource(rt, &path, true);
                let all = if found.r() == 0 {
                    Vec::new()
                } else {
                    vec![found]
                };
                if name == "resources" {
                    crate::jutil::new_list(rt, all)
                } else {
                    crate::jutil::new_iter(rt, all)
                }
            }
            "loadClass" | "findClass" => {
                let n = a(1);
                crate::api::call(
                    rt,
                    "java/lang/Class.forName.(Ljava/lang/String;)Ljava/lang/Class;",
                    &[n],
                )
            }
            "getName" => rt.string("app"),
            _ => return None,
        }),
        // A class's package: its name; a mod reads no version out of a manifest here.
        "java/lang/Package" => Some(match name {
            "getName" | "toString" => rt.get_field(a(0), "$name"),
            "getImplementationVersion"
            | "getSpecificationVersion"
            | "getImplementationTitle"
            | "getImplementationVendor"
            | "getSpecificationTitle" => V::NULL,
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

/// Java's `(offset, count)` range of a `length`-long array, kept inside it.
fn span(offset: V, count: V, len: usize) -> std::ops::Range<usize> {
    let start = (offset.i().max(0) as usize).min(len);
    start..(start + count.i().max(0) as usize).min(len)
}

/// `bytes` read in `charset` (its canonical name, upper case): a char a byte in ISO-8859-1, US-ASCII
/// with U+FFFD past its range, UTF-16 by its byte-order mark (big-endian without one), else UTF-8
/// with U+FFFD for a malformed sequence, as Java's decoders replace them.
fn decode(bytes: &[u8], charset: &str) -> String {
    let utf16 = |b: &[u8], big: bool| from16(&b.chunks_exact(2).map(|p| if big { u16::from_be_bytes([p[0], p[1]]) } else { u16::from_le_bytes([p[0], p[1]]) }).collect::<Vec<_>>());
    match charset {
        "ISO-8859-1" | "ISO8859_1" | "LATIN1" => bytes.iter().map(|&b| char::from(b)).collect(),
        "US-ASCII" | "ASCII" => bytes.iter().map(|&b| if b < 0x80 { char::from(b) } else { '\u{FFFD}' }).collect(),
        "UTF-16BE" => utf16(bytes, true),
        "UTF-16LE" => utf16(bytes, false),
        "UTF-16" => match bytes {
            [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
            [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
            _ => utf16(bytes, true),
        },
        _ => String::from_utf8_lossy(bytes).into_owned(),
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
                    // `valueOf(chars, offset, count)`: that part.
                    let range = if desc.starts_with("([CII)") {
                        span(a(1), a(2), chars.len())
                    } else {
                        0..chars.len()
                    };
                    from16(&chars[range])
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
        // `new String(...)`: the object `new` made becomes the text its arguments spell (chars or
        // code points with an offset and count, bytes in a charset, another string or builder).
        "<init>" => {
            let s = match desc {
                "()V" => String::new(),
                "(Ljava/lang/String;)V" | "(Ljava/lang/StringBuilder;)V" | "(Ljava/lang/StringBuffer;)V" => text(rt, a(1)),
                d if d.starts_with("([C") => {
                    let chars: Vec<u16> = crate::jutil::elements(rt, a(1)).iter().map(|v| v.i() as u16).collect();
                    let range = if d == "([CII)V" { span(a(2), a(3), chars.len()) } else { 0..chars.len() };
                    from16(chars.get(range).unwrap_or(&[]))
                }
                "([III)V" => {
                    let points: Vec<i32> = crate::jutil::elements(rt, a(1)).iter().map(|v| v.i()).collect();
                    let range = span(a(2), a(3), points.len());
                    points.get(range).unwrap_or(&[]).iter().map(|&p| char::from_u32(p as u32).unwrap_or('\u{FFFD}')).collect()
                }
                // `new String(ascii, hibyte, offset, count)`: each byte a character, under the
                // high byte given (the JDK's own code builds its ASCII strings so).
                "([BIII)V" => {
                    let bytes: Vec<u16> = crate::jutil::elements(rt, a(1))
                        .iter()
                        .map(|v| (a(2).i() as u16) << 8 | u16::from(v.i() as u8))
                        .collect();
                    from16(bytes.get(span(a(3), a(4), bytes.len())).unwrap_or(&[]))
                }
                d if d.starts_with("([B") => {
                    let bytes: Vec<u8> = crate::jutil::elements(rt, a(1)).iter().map(|v| v.i() as u8).collect();
                    let (range, charset) = if d.starts_with("([BII") { (span(a(2), a(3), bytes.len()), a(4)) } else { (0..bytes.len(), a(2)) };
                    let charset = if d.contains("Ljava/nio/charset/Charset;") || d.contains("ILjava/lang/String;)") || d == "([BLjava/lang/String;)V" {
                        rt.str_of(rt.get_field(charset, "$name")).or_else(|| rt.str_of(charset)).unwrap_or("UTF-8").to_ascii_uppercase()
                    } else {
                        "UTF-8".to_owned()
                    };
                    decode(bytes.get(range).unwrap_or(&[]), &charset)
                }
                _ => return None,
            };
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Str(s);
            }
            return Some(V::Void);
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
        "isBlank" => V::I(s.chars().all(java_whitespace) as i32),
        "hashCode" => V::I(string_hash(&s)),
        "equals" | "contentEquals" => V::I((Some(s.as_str()) == rt.str_of(a(1))) as i32),
        "equalsIgnoreCase" => V::I(rt.str_of(a(1)).is_some_and(|o| o.to_lowercase() == s.to_lowercase()) as i32),
        "compareTo" => V::I(java_compare(&s, &arg_s(rt, 1))),
        "compareToIgnoreCase" => V::I(compare_ignore_case(&s, &arg_s(rt, 1))),
        "charAt" | "codePointAt" => V::I(u.get(a(1).i().max(0) as usize).copied().unwrap_or(0) as i32),
        "startsWith" => {
            let p = utf16(&arg_s(rt, 1));
            let from = if ks.len() > 1 { a(2).i().max(0) as usize } else { 0 };
            V::I(u.get(from..).is_some_and(|t| t.starts_with(&p)) as i32)
        }
        "endsWith" => V::I(s.ends_with(&arg_s(rt, 1)) as i32),
        "contains" => V::I(s.contains(&arg_s(rt, 1)) as i32),
        "indexOf" | "lastIndexOf" => {
            let needle = match ks.first() {
                Some('I') => code_point_units(a(1).i()),
                _ => Some(utf16(&arg_s(rt, 1))),
            };
            let from = ks.get(1).map(|_| a(2).i());
            V::I(needle.map_or(-1, |n| index_of(&u, &n, from, name == "lastIndexOf")))
        }
        "substring" | "subSequence" => {
            let lo = (a(1).i().max(0) as usize).min(u.len());
            let hi = if ks.len() > 1 { (a(2).i().max(0) as usize).min(u.len()) } else { u.len() };
            new_str(rt, from16(&u[lo..hi.max(lo)]))
        }
        "toLowerCase" => new_str(rt, s.to_lowercase()),
        "toUpperCase" => new_str(rt, s.to_uppercase()),
        // `trim` drops the characters up to a space; `strip` Java's whitespace.
        "trim" => new_str(rt, s.trim_matches(|c: char| c <= ' ').to_owned()),
        "strip" => new_str(rt, s.trim_matches(java_whitespace).to_owned()),
        "stripLeading" => new_str(rt, s.trim_start_matches(java_whitespace).to_owned()),
        "stripTrailing" => new_str(rt, s.trim_end_matches(java_whitespace).to_owned()),
        // `lines()`: split at `\n`, `\r` and `\r\n`, with no empty line after a last terminator.
        "lines" => {
            let mut items = Vec::new();
            let mut rest = s.as_str();
            while !rest.is_empty() {
                let end = rest.find(['\n', '\r']).unwrap_or(rest.len());
                items.push(new_str(rt, rest[..end].to_owned()));
                rest = &rest[end..];
                rest = rest
                    .strip_prefix("\r\n")
                    .or_else(|| rest.strip_prefix(['\n', '\r']))
                    .unwrap_or(rest);
            }
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
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

/// `String.format` for the conversions mods use: `%s %d %f %.Nf %x %c %b %%` and `%n`, an argument
/// by its index (`%2$s`, from 1) or the one before again (`%<s`), and `,` grouping thousands.
pub(crate) fn format_java(rt: &Rt, f: &str, vals: &[V]) -> String {
    let mut out = String::new();
    let mut it = f.chars().peekable();
    let mut next = 0;
    let mut last = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek().filter(|d| d.is_ascii_digit()) {
            spec.push(d);
            it.next();
        }
        let mut index = None;
        if it.peek() == Some(&'$') && !spec.is_empty() {
            it.next();
            index = spec.parse::<usize>().ok().map(|n| n.saturating_sub(1));
            spec.clear();
        } else if spec.is_empty() && it.peek() == Some(&'<') {
            it.next();
            index = Some(last);
        }
        while let Some(&d) = it.peek() {
            if d.is_ascii_digit() || matches!(d, '.' | '-' | ',' | '+' | '#' | ' ' | '(') {
                spec.push(d);
                it.next();
            } else {
                break;
            }
        }
        let Some(conv) = it.next() else { break };
        // An ordinary specifier takes the next argument; one with an index doesn't move it on.
        let k = index.unwrap_or(next);
        if !matches!(conv, '%' | 'n') {
            last = k;
            if index.is_none() {
                next += 1;
            }
        }
        let v = vals.get(k).copied().unwrap_or_default();
        let unbox = |v: V| match v {
            V::R(_) if rt.class_of(v).starts_with("java/lang/") && rt.str_of(v).is_none() => rt.get_field(v, "value"),
            v => v,
        };
        let grouped = spec.contains(',');
        let piece = match conv {
            '%' => "%".to_owned(),
            'n' => "\n".to_owned(),
            's' | 'S' => {
                let t = text(rt, v);
                if conv == 'S' { t.to_uppercase() } else { t }
            }
            'd' => {
                let t = match unbox(v) {
                    V::J(j) => j.to_string(),
                    x => x.i().to_string(),
                };
                if grouped { thousands(&t) } else { t }
            }
            'x' | 'X' => {
                let t = match unbox(v) {
                    V::J(j) => format!("{j:x}"),
                    x => format!("{:x}", x.i()),
                };
                if conv == 'X' { t.to_uppercase() } else { t }
            }
            'f' | 'e' | 'g' => {
                let x = match unbox(v) {
                    V::F(x) => x as f64,
                    V::D(x) => x,
                    V::I(i) => i as f64,
                    V::J(j) => j as f64,
                    _ => 0.0,
                };
                let prec = spec.split_once('.').and_then(|(_, p)| p.parse().ok()).unwrap_or(6);
                let t = fixed_half_up(x, prec);
                if grouped { thousands(&t) } else { t }
            }
            'c' => char::from_u32(unbox(v).i() as u32)
                .map(String::from)
                .unwrap_or_default(),
            // `%b`: null is false, a `Boolean` its value, anything else true.
            'b' | 'B' => {
                let b = match v {
                    V::R(0) => false,
                    V::R(_) if rt.class_of(v) == "java/lang/Boolean" => {
                        rt.get_field(v, "value").i() != 0
                    }
                    V::R(_) => true,
                    x => x.i() != 0,
                };
                if conv == 'B' {
                    b.to_string().to_uppercase()
                } else {
                    b.to_string()
                }
            }
            other => format!("%{spec}{other}"),
        };
        // Width (`%5d`, `%-5s`, `%05d`).
        let width: usize = spec
            .split('.')
            .next()
            .unwrap_or("")
            .trim_start_matches(['-', '0', '+', ',', '#', ' ', '('])
            .parse()
            .unwrap_or(0);
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

/// `t`'s whole part with a `,` between each three digits (`%,d`, `%,.2f`), as an English locale
/// groups them.
fn thousands(t: &str) -> String {
    let (sign, rest) = t.strip_prefix('-').map_or(("", t), |r| ("-", r));
    let (whole, frac) = rest
        .split_once('.')
        .map_or((rest, None), |(w, f)| (w, Some(f)));
    let mut out = String::from(sign);
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
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

fn builder(rt: &mut Rt, name: &str, desc: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    // A `char[]` argument's text: an array has no class of its elements unless a string made it.
    let chars = |rt: &Rt, v: V| {
        from16(
            &crate::jutil::elements(rt, v)
                .iter()
                .map(|v| v.i() as u16)
                .collect::<Vec<_>>(),
        )
    };
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
                Some('L') if desc.starts_with("([C") || rt.class_of(a(1)) == "[C" => {
                    chars(rt, a(1))
                }
                Some(&k) => rt.to_java_string(a(1), k),
                None => String::new(),
            };
            // `append(CharSequence, start, end)` and `append(char[], offset, count)`: that part.
            let piece = if ks.len() == 3 {
                let u = utf16(&piece);
                let (x, y) = (a(2).i().max(0) as usize, a(3).i().max(0) as usize);
                let to = if desc.starts_with("([C") { x + y } else { y }.min(u.len());
                from16(&u[x.min(to)..to])
            } else {
                piece
            };
            let s = cur(rt) + &piece;
            set(rt, s);
            this
        }
        "insert" => {
            let mut u = utf16(&cur(rt));
            let at = (a(1).i().max(0) as usize).min(u.len());
            let piece = if desc.starts_with("(I[C") {
                chars(rt, a(2))
            } else {
                rt.to_java_string(a(2), ks.get(1).copied().unwrap_or('L'))
            };
            let piece = utf16(&piece);
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
        // How much room the builder keeps is its own business: the text is the same.
        "trimToSize" | "ensureCapacity" => V::Void,
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
        // `indexOf(str[, from])`, `lastIndexOf(str[, from])`: as `String`'s.
        "indexOf" | "lastIndexOf" => {
            let (hay, needle) = (utf16(&cur(rt)), utf16(&text(rt, a(1))));
            let from = ks.get(1).map(|_| a(2).i());
            V::I(index_of(&hay, &needle, from, name == "lastIndexOf"))
        }
        _ => return None,
    })
}

/// The UTF-16 units of a code point `indexOf(int)` looks for: a char's own unit (a lone surrogate
/// too), a supplementary one's pair; None for no code point, which is never found.
fn code_point_units(cp: i32) -> Option<Vec<u16>> {
    match cp {
        0..=0xFFFF => Some(vec![cp as u16]),
        _ => char::from_u32(cp as u32).map(|c| utf16(&c.to_string())),
    }
}

/// Java's `String.indexOf(str, from)` and `lastIndexOf(str, from)` over UTF-16 units, `from`
/// clamped as the JDK clamps it: an empty needle is found where the search starts (at most the
/// length), and `from` past the end finds nothing else.
fn index_of(hay: &[u16], needle: &[u16], from: Option<i32>, last: bool) -> i32 {
    let len = hay.len() as i64;
    let n = needle.len() as i64;
    let at = |i: i64| hay[i as usize..(i + n) as usize] == *needle;
    if last {
        let mut i = from.map_or(len, i64::from).min(len - n);
        while i >= 0 {
            if at(i) {
                return i as i32;
            }
            i -= 1;
        }
        -1
    } else {
        let start = from.map_or(0, i64::from).max(0);
        if start >= len {
            return if n == 0 { len as i32 } else { -1 };
        }
        (start..=len - n).find(|&i| at(i)).map_or(-1, |i| i as i32)
    }
}

fn number(rt: &mut Rt, owner: &str, name: &str, desc: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // A number class of a mod's or a library's (`BigInteger` as the JDK's own code) starts as any
    // object does.
    if owner == "java/lang/Number" && name == "<init>" {
        return Some(V::Void);
    }
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
    let radix = if ks.len() > 1 { a(1).i() } else { 10 };
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
            let v = parse_number(rt, simple, a(0), radix);
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            boxed(rt, v)
        }
        "valueOf" => boxed(rt, a(0)),
        // `Integer.getInteger(name[, default])` and `Long.getLong`: a system property, of which the
        // runtime has none: the default, boxed, else null.
        "getInteger" | "getLong" => match ks.get(1) {
            Some('L') => a(1),
            Some(_) => boxed(rt, a(1)),
            None => V::NULL,
        },
        // `parseInt(text, begin, end, radix)`: that part of the text.
        "parseInt" | "parseLong" if desc.starts_with("(Ljava/lang/CharSequence;III)") => {
            let u = utf16(&text(rt, a(0)));
            let part = from16(
                u.get(span(a(1), V::I(a(2).i() - a(1).i()), u.len()))
                    .unwrap_or(&[]),
            );
            let part = new_str(rt, part);
            parse_number(rt, simple, part, a(3).i())
        }
        "parseInt" | "parseLong" | "parseDouble" | "parseFloat" | "parseShort" | "parseByte" => parse_number(rt, simple, a(0), radix),
        "parseUnsignedInt" | "parseUnsignedLong" => {
            let long = name == "parseUnsignedLong";
            let s = rt.str_of(a(0)).map(str::to_owned);
            match parse_unsigned(s.as_deref(), radix, long) {
                Ok(v) if long => V::J(v),
                Ok(v) => V::I(v as i32),
                Err(m) => {
                    throw_with(rt, "java/lang/NumberFormatException", &m);
                    if long { V::J(0) } else { V::I(0) }
                }
            }
        }
        // `decode`: a sign, then `0x`, `#` or `0` for hexadecimal or octal digits.
        "decode" => {
            let Some(s) = rt.str_of(a(0)).map(str::to_owned) else {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::NULL);
            };
            match decode_integer(&s, if simple == "Long" { 64 } else { 32 }) {
                Ok(v) => {
                    let v = if simple == "Long" {
                        V::J(v)
                    } else {
                        V::I(v as i32)
                    };
                    boxed(rt, v)
                }
                Err(m) => throw_with(rt, "java/lang/NumberFormatException", &m),
            }
        }
        "toString" if ks.len() == 2 => {
            let v = match a(0) {
                V::J(j) => j,
                v => i64::from(v.i()),
            };
            new_str(rt, radix_string(i128::from(v), a(1).i()))
        }
        "toUnsignedString" => {
            let v = match a(0) {
                V::J(j) => i128::from(j as u64),
                v => i128::from(v.i() as u32),
            };
            new_str(rt, radix_string(v, radix))
        }
        "toUnsignedInt" | "toUnsignedLong" => {
            let x = i64::from(a(0).i());
            let m = match simple {
                "Byte" => x & 0xff,
                "Short" => x & 0xffff,
                _ => x & 0xffff_ffff,
            };
            if name == "toUnsignedInt" {
                V::I(m as i32)
            } else {
                V::J(m)
            }
        }
        "compareUnsigned" => V::I(match (a(0), a(1)) {
            (V::J(x), V::J(y)) => (x as u64).cmp(&(y as u64)) as i32,
            (x, y) => (x.i() as u32).cmp(&(y.i() as u32)) as i32,
        }),
        "divideUnsigned" | "remainderUnsigned" => {
            let (x, y, long) = match (a(0), a(1)) {
                (V::J(x), V::J(y)) => (x as u64, y as u64, true),
                (x, y) => (u64::from(x.i() as u32), u64::from(y.i() as u32), false),
            };
            if y == 0 {
                throw_with(rt, "java/lang/ArithmeticException", "/ by zero");
                return Some(if long { V::J(0) } else { V::I(0) });
            }
            let r = if name == "divideUnsigned" {
                x / y
            } else {
                x % y
            };
            if long {
                V::J(r as i64)
            } else {
                V::I(r as u32 as i32)
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
        "highestOneBit" => match a(0) {
            V::J(j) => V::J(if j == 0 {
                0
            } else {
                1i64 << (63 - j.leading_zeros())
            }),
            v => V::I(if v.i() == 0 {
                0
            } else {
                1i32 << (31 - v.i().leading_zeros())
            }),
        },
        "lowestOneBit" => match a(0) {
            V::J(j) => V::J(j & j.wrapping_neg()),
            v => V::I(v.i() & v.i().wrapping_neg()),
        },
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

/// `NumberFormatException.forInputString`: the text, then the radix when it isn't 10.
fn for_input(s: &str, radix: i32) -> String {
    if radix == 10 {
        format!("For input string: \"{s}\"")
    } else {
        format!("For input string: \"{s}\" under radix {radix}")
    }
}

/// `Character.getType`: a character's general category as Java numbers them
/// (`UPPERCASE_LETTER` 1 ... `FINAL_QUOTE_PUNCTUATION` 30; 0 unassigned). ASCII by rule, the rest
/// by the Unicode tables the regular expressions read.
fn general_category(cp: i32) -> u8 {
    const CLASSES: [(&str, u8); 29] = [
        ("Lu", 1),
        ("Ll", 2),
        ("Lt", 3),
        ("Lm", 4),
        ("Lo", 5),
        ("Mn", 6),
        ("Me", 7),
        ("Mc", 8),
        ("Nd", 9),
        ("Nl", 10),
        ("No", 11),
        ("Zs", 12),
        ("Zl", 13),
        ("Zp", 14),
        ("Cc", 15),
        ("Cf", 16),
        ("Co", 18),
        ("Pd", 20),
        ("Ps", 21),
        ("Pe", 22),
        ("Pc", 23),
        ("Po", 24),
        ("Sm", 25),
        ("Sc", 26),
        ("Sk", 27),
        ("So", 28),
        ("Pi", 29),
        ("Pf", 30),
        ("Cn", 0),
    ];
    if (0xD800..0xE000).contains(&cp) {
        return 19;
    }
    let Some(c) = u32::try_from(cp).ok().and_then(char::from_u32) else {
        return 0;
    };
    if c.is_ascii() {
        return match c {
            'A'..='Z' => 1,
            'a'..='z' => 2,
            '0'..='9' => 9,
            ' ' => 12,
            '\0'..='\x1f' | '\x7f' => 15,
            '-' => 20,
            '(' | '[' | '{' => 21,
            ')' | ']' | '}' => 22,
            '_' => 23,
            '+' | '<' | '=' | '>' | '|' | '~' => 25,
            '$' => 26,
            '^' | '`' => 27,
            _ => 24,
        };
    }
    static TABLES: std::sync::OnceLock<Vec<(regex::Regex, u8)>> = std::sync::OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        CLASSES
            .iter()
            .filter_map(|(class, n)| {
                Some((regex::Regex::new(&format!(r"^\p{{{class}}}$")).ok()?, *n))
            })
            .collect()
    });
    let mut text = [0u8; 4];
    let text = c.encode_utf8(&mut text);
    tables
        .iter()
        .find(|(re, _)| re.is_match(text))
        .map_or(0, |(_, n)| *n)
}

/// `Character.isIdentifierIgnorable`: the controls that are no whitespace, and the format
/// characters.
fn identifier_ignorable(cp: i32) -> bool {
    matches!(cp, 0..=8 | 0xE..=0x1B | 0x7F..=0x9F) || general_category(cp) == 16
}

/// `Character.isJavaIdentifierStart` (`part` false) and `isJavaIdentifierPart`: a letter, a
/// letter number, a currency symbol or a connecting punctuation mark; in a part also a digit, a
/// combining or non-spacing mark, or an ignorable character.
fn java_identifier(cp: i32, part: bool) -> bool {
    match general_category(cp) {
        1..=5 | 10 | 23 | 26 => true,
        6 | 8 | 9 => part,
        _ => part && identifier_ignorable(cp),
    }
}

/// `Character.isUnicodeIdentifierStart` (`part` false) and `isUnicodeIdentifierPart`: a letter,
/// a letter number or one of Unicode's `Other_ID_Start`; in a part also a connecting punctuation
/// mark, a digit, a mark, an ignorable character or one of `Other_ID_Continue`.
fn unicode_identifier(cp: i32, part: bool) -> bool {
    if matches!(cp, 0x1885 | 0x1886 | 0x2118 | 0x212E | 0x309B | 0x309C) {
        return true;
    }
    match general_category(cp) {
        1..=5 | 10 => true,
        6 | 8 | 9 | 23 => part,
        _ => {
            part && (identifier_ignorable(cp)
                || matches!(cp, 0xB7 | 0x387 | 0x1369..=0x1371 | 0x19DA))
        }
    }
}

/// `Character.isWhitespace`: the Unicode separators but the no-break spaces, and the tab, line,
/// form-feed and file-to-unit separator controls.
fn java_whitespace(c: char) -> bool {
    matches!(c, '\t'..='\r' | '\u{1C}'..='\u{1F}')
        || (c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}'))
}

/// The length Java gives a string: its UTF-16 units.
fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `Integer.parseInt(s, radix)` (`bits` 32) and `Long.parseLong` (64) as the JDK parses: an
/// optional sign, then only digits of the radix (no spaces), in range; else the message of the
/// JDK's `NumberFormatException`.
fn parse_integer(s: Option<&str>, radix: i32, bits: u32) -> Result<i64, String> {
    let Some(s) = s else { return Err("Cannot parse null string".into()) };
    if radix < 2 {
        return Err(format!("radix {radix} less than Character.MIN_RADIX"));
    }
    if radix > 36 {
        return Err(format!("radix {radix} greater than Character.MAX_RADIX"));
    }
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    if digits.is_empty() {
        return Err(for_input(s, radix));
    }
    let max = (1i128 << (bits - 1)) - 1;
    let mut v: i128 = 0;
    for c in digits.chars() {
        let d = c
            .to_digit(radix as u32)
            .ok_or_else(|| for_input(s, radix))?;
        v = v * i128::from(radix) + i128::from(d);
        if v > max + 1 {
            return Err(for_input(s, radix));
        }
    }
    let v = if s.starts_with('-') { -v } else { v };
    if v > max {
        return Err(for_input(s, radix));
    }
    Ok(v as i64)
}

/// `Integer.parseUnsignedInt(s, radix)` and `Long.parseUnsignedLong` (`long`): no minus sign, up
/// to the unsigned type's largest; the value's bits as the signed type holds them.
fn parse_unsigned(s: Option<&str>, radix: i32, long: bool) -> Result<i64, String> {
    let Some(s) = s else { return Err("Cannot parse null string".into()) };
    let len = len16(s);
    if len == 0 {
        return Err(for_input(s, radix));
    }
    if s.starts_with('-') {
        return Err(format!(
            "Illegal leading minus sign on unsigned string {s}."
        ));
    }
    // Short enough to be in the signed type's range: parsed as one.
    let short = if long {
        len <= 12 || (radix == 10 && len <= 18)
    } else {
        len <= 5 || (radix == 10 && len <= 9)
    };
    if short {
        return parse_integer(Some(s), radix, if long { 64 } else { 32 });
    }
    if !long {
        let v = parse_integer(Some(s), radix, 64)?;
        return if v >> 32 == 0 {
            Ok(v)
        } else {
            Err(format!("String value {s} exceeds range of unsigned int."))
        };
    }
    if !(2..=36).contains(&radix) {
        return parse_integer(Some(s), radix, 64);
    }
    let (head, last) = s.split_at(s.len() - s.chars().last().map_or(0, char::len_utf8));
    let first = parse_integer(Some(head), radix, 64).map_err(|_| {
        let at = head
            .chars()
            .position(|c| c.to_digit(radix as u32).is_none() && c != '+')
            .unwrap_or(0);
        format!("Error at index {at} in: \"{head}\"")
    })?;
    let Some(d) = last.chars().next().and_then(|c| c.to_digit(radix as u32)) else {
        return Err(format!("Bad digit at end of {s}"));
    };
    let v = i128::from(first) * i128::from(radix) + i128::from(d);
    if v > i128::from(u64::MAX) {
        return Err(format!("String value {s} exceeds range of unsigned long."));
    }
    Ok(v as u64 as i64)
}

/// `Integer.decode` / `Long.decode` (`bits`): a sign, then `0x`, `0X` or `#` for hexadecimal
/// digits, a leading `0` for octal ones.
fn decode_integer(s: &str, bits: u32) -> Result<i64, String> {
    if s.is_empty() {
        return Err("Zero length string".into());
    }
    let negative = s.starts_with('-');
    let rest = s.strip_prefix(['-', '+']).unwrap_or(s);
    let (radix, digits) = if let Some(d) = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .or_else(|| rest.strip_prefix('#'))
    {
        (16, d)
    } else if rest.len() > 1
        && let Some(d) = rest.strip_prefix('0')
    {
        (8, d)
    } else {
        (10, rest)
    };
    if digits.starts_with(['-', '+']) {
        return Err("Sign character in wrong position".into());
    }
    let constant = if negative {
        format!("-{digits}")
    } else {
        digits.to_owned()
    };
    parse_integer(Some(&constant), radix, bits)
}

/// `Double.parseDouble` / `Float.parseFloat` text as the JDK reads it: trimmed of the characters
/// up to a space, an optional sign, then `NaN`, `Infinity` or a decimal number with an optional
/// exponent and an optional `f`/`d` suffix. The text Rust's parser reads the same (its decimal
/// grammar is Java's), or the JDK's message. Hexadecimal floating-point text isn't read.
fn floating_text(s: &str) -> Result<String, String> {
    let t = s.trim_matches(|c: char| c <= ' ');
    if t.is_empty() {
        return Err("empty String".into());
    }
    let (sign, body) = match t.as_bytes()[0] {
        b'-' => ("-", &t[1..]),
        b'+' => ("", &t[1..]),
        _ => ("", t),
    };
    match body {
        "NaN" => return Ok("NaN".into()),
        "Infinity" => return Ok(format!("{sign}inf")),
        _ => {}
    }
    let num = body.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(body);
    let decimal = num
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && !num.starts_with(['+', '-']);
    if !decimal || num.parse::<f64>().is_err() {
        return Err(format!("For input string: \"{t}\""));
    }
    Ok(format!("{sign}{num}"))
}

/// `parseX(s[, radix])` and `valueOf(s[, radix])` of the boxed number class `simple` (`Integer`,
/// `Long`, `Short`, `Byte`, `Double`, `Float`) as the JDK parses: the value, else a
/// `NumberFormatException` with the JDK's message (a null floating text, a
/// `NullPointerException`), the type's zero left.
fn parse_number(rt: &mut Rt, simple: &str, s: V, radix: i32) -> V {
    let text = rt.str_of(s).map(str::to_owned);
    let parsed = match simple {
        "Double" | "Float" => {
            let Some(t) = text else {
                rt.throw_new("java/lang/NullPointerException");
                return if simple == "Float" { V::F(0.0) } else { V::D(0.0) };
            };
            floating_text(&t).map(|n| if simple == "Float" { V::F(n.parse().unwrap_or(0.0)) } else { V::D(n.parse().unwrap_or(0.0)) })
        }
        "Long" => parse_integer(text.as_deref(), radix, 64).map(V::J),
        "Short" | "Byte" => parse_integer(text.as_deref(), radix, 32).and_then(|v| {
            let fits = if simple == "Short" {
                i16::try_from(v).is_ok()
            } else {
                i8::try_from(v).is_ok()
            };
            if fits {
                Ok(V::I(v as i32))
            } else {
                Err(format!(
                    "Value out of range. Value:\"{}\" Radix:{radix}",
                    text.as_deref().unwrap_or("")
                ))
            }
        }),
        _ => parse_integer(text.as_deref(), radix, 32).map(|v| V::I(v as i32)),
    };
    parsed.unwrap_or_else(|m| {
        throw_with(rt, "java/lang/NumberFormatException", &m);
        match simple {
            "Long" => V::J(0),
            "Double" => V::D(0.0),
            "Float" => V::F(0.0),
            _ => V::I(0),
        }
    })
}

/// `Integer.toString(v, radix)` / `Long.toString` (and the unsigned forms, given the unsigned
/// value): lowercase digits after a minus sign; a radix out of 2..=36 is 10.
fn radix_string(v: i128, radix: i32) -> String {
    let radix = if (2..=36).contains(&radix) {
        radix as u32
    } else {
        10
    };
    let mut n = v.unsigned_abs();
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((n % u128::from(radix)) as u32, radix).unwrap_or('0'));
        n /= u128::from(radix);
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

/// `Math`'s integer arithmetic that can fail, as the JDK's: the `*Exact` methods (an overflow
/// throws `ArithmeticException`, "integer overflow" or "long overflow" by the result's type), and
/// `floorDiv`, `floorMod`, `ceilDiv`, `ceilMod` in each overload (`floorMod(long, int)` is an
/// int), a zero divisor throwing "/ by zero". None for the other methods.
fn integer_math(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let ret = desc.rsplit(')').next()?;
    if !matches!(ret, "I" | "J") {
        return None;
    }
    let long = ret == "J";
    let a = |i: usize| match args.get(i) {
        Some(V::I(x)) => i128::from(*x),
        Some(V::J(x)) => i128::from(*x),
        _ => 0,
    };
    let (x, y) = (a(0), a(1));
    let fail = |rt: &mut Rt, msg: &str| {
        throw_with(rt, "java/lang/ArithmeticException", msg);
        Some(if long { V::J(0) } else { V::I(0) })
    };
    let exact = |rt: &mut Rt, r: i128| {
        let fits = if long {
            i64::try_from(r).is_ok()
        } else {
            i32::try_from(r).is_ok()
        };
        if !fits {
            return fail(
                rt,
                if long {
                    "long overflow"
                } else {
                    "integer overflow"
                },
            );
        }
        Some(if long { V::J(r as i64) } else { V::I(r as i32) })
    };
    let divides = matches!(
        name,
        "floorDiv" | "floorMod" | "ceilDiv" | "ceilMod" | "divideExact"
    );
    if divides && y == 0 {
        return fail(rt, "/ by zero");
    }
    // Wrapped to the result's type, as Java's division wraps `MIN_VALUE / -1`.
    let wrap = |r: i128| Some(if long { V::J(r as i64) } else { V::I(r as i32) });
    match name {
        "addExact" => exact(rt, x + y),
        "subtractExact" => exact(rt, x - y),
        "multiplyExact" => exact(rt, x * y),
        "divideExact" => exact(rt, x / y),
        "incrementExact" => exact(rt, x + 1),
        "decrementExact" => exact(rt, x - 1),
        "negateExact" => exact(rt, -x),
        "toIntExact" => match i32::try_from(x) {
            Ok(v) => Some(V::I(v)),
            Err(_) => fail(rt, "integer overflow"),
        },
        "absExact"
            if x == if long {
                i128::from(i64::MIN)
            } else {
                i128::from(i32::MIN)
            } =>
        {
            fail(
                rt,
                if long {
                    "Overflow to represent absolute value of Long.MIN_VALUE"
                } else {
                    "Overflow to represent absolute value of Integer.MIN_VALUE"
                },
            )
        }
        "absExact" => exact(rt, x.abs()),
        "floorDiv" => wrap(x.div_euclid(y) - i128::from(y < 0 && x.rem_euclid(y) != 0)),
        "floorMod" => {
            let r = x % y;
            wrap(if r != 0 && (r < 0) != (y < 0) {
                r + y
            } else {
                r
            })
        }
        "ceilDiv" => wrap(x / y + i128::from(x % y != 0 && (x < 0) == (y < 0))),
        "ceilMod" => {
            let r = x % y;
            wrap(if r != 0 && (x < 0) == (y < 0) {
                r - y
            } else {
                r
            })
        }
        "multiplyFull" => wrap(x * y),
        "multiplyHigh" => wrap((x * y) >> 64),
        _ => None,
    }
}

/// `Math.round(float)` as the JDK computes it: floor(a + 1/2) exactly, from the bits (0.49999997
/// rounds to 0, where adding a half in floats would round it up), NaN to 0, beyond the ints
/// saturated.
pub(crate) fn round_f32(a: f32) -> i32 {
    let bits = a.to_bits() as i32;
    let biased_exp = (bits & 0x7F80_0000) >> 23;
    // SIGNIFICAND_WIDTH - 2 + EXP_BIAS: 24 - 2 + 127.
    let shift = 149 - biased_exp;
    if (shift & -32) == 0 {
        let mut r = (bits & 0x007F_FFFF) | 0x0080_0000;
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        // Java's `(int) a`, as Rust's cast: NaN to 0, saturating.
        a as i32
    }
}

/// `Math.round(double)` as the JDK computes it ([`round_f32`]'s, on 64 bits).
pub(crate) fn round_f64(a: f64) -> i64 {
    let bits = a.to_bits() as i64;
    let biased_exp = (bits & 0x7FF0_0000_0000_0000) >> 52;
    // SIGNIFICAND_WIDTH - 2 + EXP_BIAS: 53 - 2 + 1023.
    let shift = 1074 - biased_exp;
    if (shift & -64) == 0 {
        let mut r = (bits & 0x000F_FFFF_FFFF_FFFF) | 0x0010_0000_0000_0000;
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        a as i64
    }
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
            'F' => V::I(round_f32(a(0).f())),
            _ => V::J(round_f64(d(0))),
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
        // As HotSpot rounds them on this machine ([`crate::jmath`]), not as the platform's libm does.
        "cbrt" => V::D(crate::jmath::cbrt(d(0))),
        "pow" => V::D(crate::jmath::pow(d(0), d(1))),
        "exp" => V::D(crate::jmath::exp(d(0))),
        "log" => V::D(crate::jmath::log(d(0))),
        "log10" => V::D(crate::jmath::log10(d(0))),
        "sin" => V::D(crate::jmath::sin(d(0))),
        "cos" => V::D(crate::jmath::cos(d(0))),
        "tan" => V::D(crate::jmath::tan(d(0))),
        "asin" => V::D(crate::jmath::asin(d(0))),
        "acos" => V::D(crate::jmath::acos(d(0))),
        "atan" => V::D(crate::jmath::atan(d(0))),
        "atan2" => V::D(crate::jmath::atan2(d(0), d(1))),
        "toRadians" => V::D(d(0).to_radians()),
        "toDegrees" => V::D(d(0).to_degrees()),
        "hypot" => V::D(crate::jmath::hypot(d(0), d(1))),
        // One rounding for the product and the sum together.
        "fma" => match k {
            'F' => V::F(a(0).f().mul_add(a(1).f(), a(2).f())),
            _ => V::D(d(0).mul_add(d(1), d(2))),
        },
        "signum" => match k {
            'F' => V::F(if a(0).f() == 0.0 { 0.0 } else { a(0).f().signum() }),
            _ => V::D(if d(0) == 0.0 { 0.0 } else { d(0).signum() }),
        },
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

    /// `String.compareTo`'s own numbers: the difference of the first UTF-16 units that differ,
    /// else of the lengths (a character outside the basic plane is two units).
    #[test]
    fn strings_compare_as_javas() {
        assert_eq!(java_compare("apple", "apple"), 0);
        assert_eq!(java_compare("apple", "apricot"), 'p' as i32 - 'r' as i32);
        assert_eq!(java_compare("gear", "gearbox"), -3);
        assert_eq!(java_compare("gearbox", "gear"), 3);
        assert_eq!(java_compare("\u{e9}", "e"), 0xe9 - 0x65);
        assert_eq!(java_compare("a\u{1f600}", "a"), 2);
        assert_eq!(java_compare("z", "\u{1f600}"), 'z' as i32 - 0xd83d);
        assert_eq!(compare_ignore_case("Gear", "gEAR"), 0);
        assert_eq!(compare_ignore_case("gear", "GEARBOX"), -3);
        assert_eq!(
            compare_ignore_case("Apple", "apricot"),
            'p' as i32 - 'r' as i32
        );
    }

    /// `new MutablePair<>()` holds null and null, as its fields start (Create's contraption code
    /// reads its `left` field itself), until `setLeft` puts something in.
    #[test]
    fn an_empty_mutable_pair_holds_nulls() {
        const PAIR: &str = "org/apache/commons/lang3/tuple/MutablePair";
        const LEFT: &str = "org/apache/commons/lang3/tuple/MutablePair.$left.()Ljava/lang/Object;";
        const RIGHT: &str =
            "org/apache/commons/lang3/tuple/MutablePair.$right.()Ljava/lang/Object;";
        let mut rt = Rt::new();
        crate::install();
        let p = rt.new_object(PAIR);
        crate::api::call(
            &mut rt,
            "org/apache/commons/lang3/tuple/MutablePair.<init>.()V",
            &[p],
        );
        assert_eq!(
            crate::api::game_field(&mut rt, p, "left", "$left", LEFT),
            V::NULL
        );
        assert_eq!(
            crate::api::game_field(&mut rt, p, "right", "$right", RIGHT),
            V::NULL
        );
        let s = rt.string("x");
        crate::api::call(
            &mut rt,
            "org/apache/commons/lang3/tuple/MutablePair.setLeft.(Ljava/lang/Object;)V",
            &[p, s],
        );
        assert_eq!(crate::api::game_field(&mut rt, p, "left", "$left", LEFT), s);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Math.round` rounds as the JDK's does: half up, exactly (adding a half in floats would round
    /// the float just below a half, and 2^23 + 1, the wrong way), NaN to 0, beyond the range
    /// saturated.
    #[test]
    fn math_round_is_the_jdks() {
        let below_half = f32::from_bits(0x3EFF_FFFF);
        let floats = [(0.5f32, 1), (-0.5, 0), (2.5, 3), (-2.5, -2), (-1.5, -1), (below_half, 0), (8_388_609.0, 8_388_609), (f32::NAN, 0), (f32::INFINITY, i32::MAX), (-1e20, i32::MIN)];
        for (x, want) in floats {
            assert_eq!(round_f32(x), want, "{x}");
        }
        let below_half_d = f64::from_bits(0x3FDF_FFFF_FFFF_FFFF);
        let doubles = [(0.5f64, 1i64), (-0.5, 0), (below_half_d, 0), (4_503_599_627_370_497.0, 4_503_599_627_370_497), (f64::NAN, 0), (-1e300, i64::MIN)];
        for (x, want) in doubles {
            assert_eq!(round_f64(x), want, "{x}");
        }
        let mut rt = Rt::new();
        assert_eq!(c(&mut rt, "java/lang/Math.round.(F)I", &[V::F(below_half)]), V::I(0));
        assert_eq!(c(&mut rt, "java/lang/Math.round.(D)J", &[V::D(4_503_599_627_370_497.0)]), V::J(4_503_599_627_370_497));
    }

    /// `new String(...)` makes the object `new` gave the text its arguments spell: bytes in the
    /// charset named (UTF-8 by default, Latin-1 a char a byte, UTF-16 by its mark), a range of
    /// chars, code points.
    #[test]
    fn new_string_spells_its_arguments() {
        let mut rt = Rt::new();
        let made = |rt: &mut Rt, key: &'static str, args: &[V]| {
            let s = rt.new_object("java/lang/String");
            let mut all = vec![s];
            all.extend_from_slice(args);
            assert_eq!(c(rt, key, &all), V::Void);
            rt.str_of(s).map(str::to_owned)
        };
        let bytes = |rt: &mut Rt, b: &[u8]| rt.alloc("[B", Data::Array(b.iter().map(|&x| V::I(i32::from(x as i8))).collect()));
        // "héllo" in UTF-8: é is two bytes.
        let utf8 = bytes(&mut rt, &[0x68, 0xC3, 0xA9, 0x6C, 0x6C, 0x6F]);
        let utf8_name = rt.string("UTF-8");
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([B)V", &[utf8]).as_deref(), Some("héllo"));
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([BLjava/lang/String;)V", &[utf8, utf8_name]).as_deref(), Some("héllo"));
        let charset = crate::api::get_static(&mut rt, "java/nio/charset/StandardCharsets.UTF_8");
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([BLjava/nio/charset/Charset;)V", &[utf8, charset]).as_deref(), Some("héllo"));
        // The same bytes in Latin-1: a char each.
        let latin = rt.string("ISO-8859-1");
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([BLjava/lang/String;)V", &[utf8, latin]).as_deref(), Some("hÃ©llo"));
        // Bytes 1 to 3 of them.
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([BII)V", &[utf8, V::I(3), V::I(3)]).as_deref(), Some("llo"));
        // UTF-16, little-endian by its mark.
        let wide = bytes(&mut rt, &[0xFF, 0xFE, 0x68, 0x00, 0x69, 0x00]);
        let utf16 = rt.string("UTF-16");
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([BLjava/lang/String;)V", &[wide, utf16]).as_deref(), Some("hi"));
        let chars = rt.alloc("[C", Data::Array("crate".encode_utf16().map(|u| V::I(i32::from(u))).collect()));
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([CII)V", &[chars, V::I(1), V::I(3)]).as_deref(), Some("rat"));
        let points = rt.alloc("[I", Data::Array(vec![V::I(0x1F600), V::I(0x41)]));
        assert_eq!(made(&mut rt, "java/lang/String.<init>.([III)V", &[points, V::I(0), V::I(2)]).as_deref(), Some("\u{1F600}A"));
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
            if short == "isActive.()Z" {
                return rt.get_field(args[0], "active");
            }
            assert_eq!(short, "getEnergy.()J");
            rt.get_field(args[0], "energy")
        }
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            if (class, what) == ("t/Machine", "<clinit>") && !rt.statics.contains_key("t/Machine.COUNT") {
                rt.statics.insert("t/Machine.COUNT", V::I(3));
            }
            if (class, what) == ("t/Machine", "fields") {
                return Some(
                    rt.string("1 energy J\n1 active Z\n1 symbol C\n1 tier B\n25 COUNT I\n"),
                );
            }
            Some(V::Void)
        }
        let mut rt = Rt::new();
        rt.supers.insert("t/Machine", "java/lang/Object");
        rt.vcall = Some(vcall);
        rt.reflect = Some(reflect);
        rt.vkeys.insert("getEnergy.()J");
        rt.vkeys.insert("isActive.()Z");
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

        // A `boolean`, a `char` and a `byte` are ints here; each is boxed as its field, its
        // method or its array declares it (a config library took a `boolean` field read as an
        // `Integer` for no boolean, and wrote `false` for every one).
        for (field, value, boxed_as) in [
            ("active", 1, "java/lang/Boolean"),
            ("symbol", 0x263a, "java/lang/Character"),
            ("tier", 4, "java/lang/Byte"),
        ] {
            rt.put_field(machine, field, V::I(value));
            let name = rt.string(field);
            let f = c(
                &mut rt,
                "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;",
                &[class, name],
            );
            let boxed = c(
                &mut rt,
                "java/lang/reflect/Field.get.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[f, machine],
            );
            assert_eq!(
                (rt.class_of(boxed), rt.get_field(boxed, "value")),
                (boxed_as, V::I(value)),
                "{field}"
            );
        }
        let name = rt.string("isActive");
        let no_params = rt.new_array(0, V::NULL);
        let method = c(
            &mut rt,
            "java/lang/Class.getDeclaredMethod.(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
            &[class, name, no_params],
        );
        let no_args = rt.new_array(0, V::NULL);
        let boxed = c(
            &mut rt,
            "java/lang/reflect/Method.invoke.(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
            &[method, machine, no_args],
        );
        assert_eq!(rt.class_of(boxed), "java/lang/Boolean");
        // And the box's own class has no fields a mod looks for.
        let box_class = class_object(&mut rt, "java/lang/Boolean");
        let none = c(
            &mut rt,
            "java/lang/Class.getDeclaredFields.()[Ljava/lang/reflect/Field;",
            &[box_class],
        );
        assert_eq!(
            (rt.class_of(none), rt.array_len(none)),
            ("[Ljava/lang/reflect/Field;", 0)
        );
        let flags = rt.alloc("[Z", Data::Array(vec![V::I(0), V::I(1)]));
        let boxed = c(
            &mut rt,
            "java/lang/reflect/Array.get.(Ljava/lang/Object;I)Ljava/lang/Object;",
            &[flags, V::I(1)],
        );
        assert_eq!(
            (rt.class_of(boxed), rt.get_field(boxed, "value")),
            ("java/lang/Boolean", V::I(1))
        );
    }

    #[test]
    fn a_record_tells_its_components_and_a_game_class_its_field_and_constructor() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            match (class, what) {
                ("t/Msg", "fields") => Some(rt.string("18 id I\n18 names Ljava/util/List; Ljava/util/List<Ljava/lang/String;>;\n25 CODEC Ljava/lang/Object;")),
                _ => None,
            }
        }
        let mut rt = Rt::new();
        rt.supers.insert("t/Msg", "java/lang/Record");
        rt.supers.insert("t/Plain", "java/lang/Object");
        rt.reflect = Some(reflect);
        let (msg, plain) = (
            class_object(&mut rt, "t/Msg"),
            class_object(&mut rt, "t/Plain"),
        );
        assert_eq!(c(&mut rt, "java/lang/Class.isRecord.()Z", &[msg]), V::I(1));
        assert_eq!(
            c(&mut rt, "java/lang/Class.isRecord.()Z", &[plain]),
            V::I(0)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Class.getRecordComponents.()[Ljava/lang/reflect/RecordComponent;",
                &[plain]
            ),
            V::NULL
        );
        let components = c(
            &mut rt,
            "java/lang/Class.getRecordComponents.()[Ljava/lang/reflect/RecordComponent;",
            &[msg],
        );
        let components = crate::jutil::elements(&rt, components);
        assert_eq!(components.len(), 2, "the static field is no component");
        let name = c(
            &mut rt,
            "java/lang/reflect/RecordComponent.getName.()Ljava/lang/String;",
            &[components[1]],
        );
        assert_eq!(rt.str_of(name), Some("names"));
        let ty = c(
            &mut rt,
            "java/lang/reflect/RecordComponent.getType.()Ljava/lang/Class;",
            &[components[0]],
        );
        assert_eq!(
            ty,
            crate::api::get_static(&mut rt, "java/lang/Integer.TYPE")
        );
        let accessor = c(
            &mut rt,
            "java/lang/reflect/RecordComponent.getAccessor.()Ljava/lang/reflect/Method;",
            &[components[1]],
        );
        let accessor_name = c(
            &mut rt,
            "java/lang/reflect/Method.getName.()Ljava/lang/String;",
            &[accessor],
        );
        assert_eq!(rt.str_of(accessor_name), Some("names"));
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Method.getParameterCount.()I",
                &[accessor]
            ),
            V::I(0)
        );
        // A field of the game's class, which carries no table: a constant by its name's shape.
        let spec = class_object(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec");
        let (logger, values) = (rt.string("LOGGER"), rt.string("childConfig"));
        let field = c(
            &mut rt,
            "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;",
            &[spec, logger],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Field.getModifiers.()I",
                &[field]
            ),
            V::I(0x19)
        );
        let field = c(
            &mut rt,
            "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;",
            &[spec, values],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Field.getModifiers.()I",
                &[field]
            ),
            V::I(1)
        );
        // A class of the JDK's made by reflection, through its constructor as `new` calls it; one
        // nothing answers is no such method, and no miss of the mod's.
        let made = construct(&mut rt, "java/lang/StringBuilder", "()V", &[]);
        assert_eq!(rt.class_of(made), "java/lang/StringBuilder");
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        construct(&mut rt, "net/minecraft/hztest/Nothing", "(I)V", &[V::I(1)]);
        let thrown = rt.thrown.take().expect("no such constructor");
        assert_eq!(rt.class_of(thrown), "java/lang/NoSuchMethodException");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_jars_files_and_a_classs_members_come_from_the_bundles_tables() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            match (class, what) {
                (crate::FILES, "defaults/mod.json") => Some(rt.alloc("[", Data::Array(b"{\"a\":1}".iter().map(|b| V::I(i32::from(*b))).collect()))),
                (crate::FILES, "t/pkg/near.txt") => Some(rt.alloc("[", Data::Array(b"near".iter().map(|b| V::I(i32::from(*b))).collect()))),
                (crate::MEMBERS, "t/pkg/Service") => Some(rt.string("1 <init> (Lt/pkg/Grid;I)V\n2 <init> ()V\n1 tick (I)V\n10 helper ()Ljava/lang/String;\n4161 bridge (Ljava/lang/Object;)V")),
                (crate::MEMBERS, "t/pkg/Base") => Some(rt.string("1 <init> ()V\n1 name ()Ljava/lang/String;")),
                _ => None,
            }
        }
        let mut rt = Rt::new();
        rt.supers.insert("t/pkg/Service", "t/pkg/Base");
        rt.supers.insert("t/pkg/Base", "java/lang/Object");
        rt.reflect = Some(reflect);
        let class = class_object(&mut rt, "t/pkg/Service");
        // Through the whole dispatcher: a URL and an enumeration are other modules'.
        let c = crate::api::call;
        // A file by its path from the jar's root, or from the class's package; null for none.
        let read = |rt: &mut Rt, path: &str| {
            let p = rt.string(path);
            let stream = c(
                rt,
                "java/lang/Class.getResourceAsStream.(Ljava/lang/String;)Ljava/io/InputStream;",
                &[class, p],
            );
            crate::resources::take(rt, stream).map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        assert_eq!(
            read(&mut rt, "/defaults/mod.json").as_deref(),
            Some("{\"a\":1}")
        );
        assert_eq!(read(&mut rt, "near.txt").as_deref(), Some("near"));
        assert_eq!(read(&mut rt, "/absent.txt"), None);
        let (loader, path) = (
            c(
                &mut rt,
                "java/lang/Class.getClassLoader.()Ljava/lang/ClassLoader;",
                &[class],
            ),
            rt.string("defaults/mod.json"),
        );
        let url = c(
            &mut rt,
            "java/lang/ClassLoader.getResource.(Ljava/lang/String;)Ljava/net/URL;",
            &[loader, path],
        );
        let opened = c(
            &mut rt,
            "java/net/URL.openStream.()Ljava/io/InputStream;",
            &[url],
        );
        assert_eq!(
            crate::resources::take(&mut rt, opened).map(|b| b.len()),
            Some(7)
        );
        let all = c(
            &mut rt,
            "java/lang/ClassLoader.getResources.(Ljava/lang/String;)Ljava/util/Enumeration;",
            &[loader, path],
        );
        assert_eq!(
            c(&mut rt, "java/util/Enumeration.hasMoreElements.()Z", &[all]),
            V::I(1)
        );
        // Its constructors: all it declares, or the public ones.
        let declared = c(
            &mut rt,
            "java/lang/Class.getDeclaredConstructors.()[Ljava/lang/reflect/Constructor;",
            &[class],
        );
        assert_eq!(rt.array_len(declared), 2);
        let public = c(
            &mut rt,
            "java/lang/Class.getConstructors.()[Ljava/lang/reflect/Constructor;",
            &[class],
        );
        let public = crate::jutil::elements(&rt, public);
        assert_eq!(public.len(), 1);
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Constructor.getParameterCount.()I",
                &[public[0]]
            ),
            V::I(2)
        );
        let types = c(
            &mut rt,
            "java/lang/reflect/Constructor.getParameterTypes.()[Ljava/lang/Class;",
            &[public[0]],
        );
        let types = crate::jutil::elements(&rt, types);
        assert_eq!(
            (rt.str_of(types[0]), rt.str_of(types[1])),
            (Some("t/pkg/Grid"), Some("int"))
        );
        // Its methods: the class's own, or the public ones with its superclass's.
        let own = c(
            &mut rt,
            "java/lang/Class.getDeclaredMethods.()[Ljava/lang/reflect/Method;",
            &[class],
        );
        let own = crate::jutil::elements(&rt, own);
        assert_eq!(own.len(), 3);
        let names = |rt: &mut Rt, methods: &[V]| -> Vec<String> {
            methods
                .iter()
                .map(|&m| {
                    let n = c(
                        rt,
                        "java/lang/reflect/Method.getName.()Ljava/lang/String;",
                        &[m],
                    );
                    rt.str_of(n).unwrap_or("?").to_owned()
                })
                .collect()
        };
        assert_eq!(names(&mut rt, &own), ["tick", "helper", "bridge"]);
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Method.getModifiers.()I",
                &[own[1]]
            ),
            V::I(10),
            "private static"
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Method.isSynthetic.()Z",
                &[own[2]]
            ),
            V::I(1)
        );
        let returned = c(
            &mut rt,
            "java/lang/reflect/Method.getReturnType.()Ljava/lang/Class;",
            &[own[1]],
        );
        assert_eq!(rt.str_of(returned), Some("java/lang/String"));
        let all = c(
            &mut rt,
            "java/lang/Class.getMethods.()[Ljava/lang/reflect/Method;",
            &[class],
        );
        let all = crate::jutil::elements(&rt, all);
        assert_eq!(names(&mut rt, &all), ["tick", "bridge", "name"]);
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    #[test]
    fn arrays_are_made_and_filled_by_reflection() {
        let mut rt = Rt::new();
        let int = class_object(&mut rt, "int");
        let ints = c(
            &mut rt,
            "java/lang/reflect/Array.newInstance.(Ljava/lang/Class;I)Ljava/lang/Object;",
            &[int, V::I(3)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/reflect/Array.getLength.(Ljava/lang/Object;)I",
                &[ints]
            ),
            V::I(3)
        );
        let seven = c(
            &mut rt,
            "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;",
            &[V::I(7)],
        );
        c(
            &mut rt,
            "java/lang/reflect/Array.set.(Ljava/lang/Object;ILjava/lang/Object;)V",
            &[ints, V::I(1), seven],
        );
        c(
            &mut rt,
            "java/lang/reflect/Array.setInt.(Ljava/lang/Object;II)V",
            &[ints, V::I(2), V::I(9)],
        );
        assert_eq!(
            (
                rt.array_get(ints, 0),
                rt.array_get(ints, 1),
                rt.array_get(ints, 2)
            ),
            (V::I(0), V::I(7), V::I(9))
        );
        let boxed = c(
            &mut rt,
            "java/lang/reflect/Array.get.(Ljava/lang/Object;I)Ljava/lang/Object;",
            &[ints, V::I(1)],
        );
        assert_eq!(
            (rt.class_of(boxed), rt.get_field(boxed, "value")),
            ("java/lang/Integer", V::I(7))
        );
        // An interface's array is one of nulls (no object of the interface is made), and a grid is
        // arrays of arrays.
        let listener = class_object(&mut rt, "net/fabricmc/fabric/api/event/Listener");
        let none = c(
            &mut rt,
            "java/lang/reflect/Array.newInstance.(Ljava/lang/Class;I)Ljava/lang/Object;",
            &[listener, V::I(0)],
        );
        assert!(rt.thrown.is_none() && rt.array_len(none) == 0);
        let dims = rt.new_array(2, V::I(0));
        rt.array_set(dims, 0, V::I(2));
        rt.array_set(dims, 1, V::I(4));
        let grid = c(
            &mut rt,
            "java/lang/reflect/Array.newInstance.(Ljava/lang/Class;[I)Ljava/lang/Object;",
            &[int, dims],
        );
        let row = rt.array_get(grid, 1);
        assert_eq!(
            (rt.array_len(grid), rt.array_len(row), rt.array_get(row, 3)),
            (2, 4, V::I(0))
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod class's fields as its table declares them (Mekanism's `SyncMapper` picks how to sync a
    /// field by its `getType()`): class objects are one per class, so `type == long.class` holds as
    /// in Java; an array field's class tells what it holds; a superclass declares what it inherits.
    #[test]
    fn a_mod_classs_fields_tell_their_types() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            let fields = match (class, what) {
                ("t/Base", "fields") => "1 owner Ljava/util/UUID;",
                ("t/Tank", "fields") => {
                    "0 <class> Lt/Base;Ljava/util/function/Supplier<Ljava/lang/String;>;\n2 stored J\n1 levels [I Ljava/util/List<Ljava/lang/String;>;\n25 MAX I"
                }
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

    /// A lambda is an instance of the interface it was made for and of that interface's own
    /// supertypes (`Class.isInstance`, `instanceof`): GuideME checks each extension a mod
    /// registers against its extension point's interface, and AE2's were lambdas.
    #[test]
    fn a_lambda_is_an_instance_of_its_interface() {
        let mut rt = Rt::new();
        rt.interfaces.insert("t/Mapper", &["t/Named"]);
        let lambda = rt.lambda_id(
            "t/Mods.lambda$register$0.(Ljava/lang/Object;)V",
            7,
            Vec::new(),
        );
        rt.lambda_info.insert(lambda.r(), ("t/Mapper", "map"));
        let is_instance = "java/lang/Class.isInstance.(Ljava/lang/Object;)Z";
        for (class, expected) in [
            ("t/Mapper", 1),
            ("t/Named", 1),
            ("java/lang/Object", 1),
            ("java/lang/Runnable", 0),
        ] {
            let c = rt.class_literal(class);
            assert_eq!(
                crate::api::call(&mut rt, is_instance, &[c, lambda]),
                V::I(expected),
                "{class}"
            );
            assert_eq!(
                rt.instance_of_static(lambda, class),
                expected == 1,
                "{class}"
            );
        }
    }

    /// `WordUtils.capitalize` and `capitalizeFully` (commons-lang3 and commons-text), as commons
    /// gives them: each word's first letter in upper case, words being what whitespace parts.
    #[test]
    fn words_are_capitalized_as_commons_does() {
        let mut rt = Rt::new();
        let cap = |rt: &mut Rt, key: &'static str, text: &str| {
            let s = rt.string(text);
            let r = crate::api::call(rt, key, &[s]);
            rt.str_of(r).map(str::to_owned)
        };
        let capitalize = "org/apache/commons/lang3/text/WordUtils.capitalize.(Ljava/lang/String;)Ljava/lang/String;";
        let fully = "org/apache/commons/text/WordUtils.capitalizeFully.(Ljava/lang/String;)Ljava/lang/String;";
        assert_eq!(
            cap(&mut rt, capitalize, "i am FINE").as_deref(),
            Some("I Am FINE")
        );
        assert_eq!(
            cap(&mut rt, fully, "i am FINE").as_deref(),
            Some("I Am Fine")
        );
        assert_eq!(
            cap(&mut rt, capitalize, "oak_log  and\ttwo").as_deref(),
            Some("Oak_log  And\tTwo")
        );
        assert_eq!(cap(&mut rt, capitalize, "").as_deref(), Some(""));
        assert_eq!(crate::api::call(&mut rt, capitalize, &[V::NULL]), V::NULL);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Boolean.valueOf(String)` reads the text as `parseBoolean` does ("true" in any case, and
    /// nothing else, is true), and gives the box: it gave the text back, so `booleanValue()` of it
    /// was false whatever a mod's option said (JOML's `-Djoml.nounsafe`, read so).
    #[test]
    fn a_boolean_is_read_from_its_text() {
        let mut rt = Rt::new();
        let value_of = "java/lang/Boolean.valueOf.(Ljava/lang/String;)Ljava/lang/Boolean;";
        let read = |rt: &mut Rt, text: Option<&str>| {
            let s = text.map_or(V::NULL, |t| rt.string(t));
            let b = crate::api::call(rt, value_of, &[s]);
            crate::api::call(rt, "java/lang/Boolean.booleanValue.()Z", &[b])
        };
        assert_eq!(read(&mut rt, Some("true")), V::I(1));
        assert_eq!(read(&mut rt, Some("TRUE")), V::I(1));
        assert_eq!(read(&mut rt, Some("false")), V::I(0));
        assert_eq!(read(&mut rt, Some("yes")), V::I(0));
        assert_eq!(read(&mut rt, Some(" true")), V::I(0));
        assert_eq!(read(&mut rt, None), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
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

    /// Create's `/create trains` writes its lines with arguments by index
    /// (`"┬%1$s: %2$s, %3$d Wagons"`, `"├─In %1$s near [%2$s]".formatted(...)`); what the JDK
    /// prints for each.
    #[test]
    fn a_format_takes_arguments_by_index_and_groups_thousands() {
        let mut rt = Rt::new();
        let format = |rt: &mut Rt, f: &str, vals: Vec<V>| {
            let f = rt.string(f);
            let arr = rt.alloc("[Ljava/lang/Object;", Data::Array(vals));
            let out = c(
                rt,
                "java/lang/String.format.(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
                &[f, arr],
            );
            rt.str_of(out).unwrap_or("?").to_owned()
        };
        let id = rt.string("fbfc9");
        let name = rt.string("Train");
        let three = c(
            &mut rt,
            "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;",
            &[V::I(3)],
        );
        assert_eq!(
            format(&mut rt, "┬%1$s: %2$s, %3$d Wagons", vec![id, name, three]),
            "┬fbfc9: Train, 3 Wagons"
        );
        // Out of order, one argument twice, an ordinary one after them starting from the first.
        assert_eq!(
            format(&mut rt, "%2$s/%1$s/%2$s %s", vec![id, name]),
            "Train/fbfc9/Train fbfc9"
        );
        assert_eq!(
            format(&mut rt, "%s %<s %s", vec![id, name]),
            "fbfc9 fbfc9 Train"
        );
        assert_eq!(
            format(&mut rt, "[%1$-7s|%2$5s]", vec![id, name]),
            "[fbfc9  |Train]"
        );
        let big = c(
            &mut rt,
            "java/lang/Long.valueOf.(J)Ljava/lang/Long;",
            &[V::J(-1_234_567)],
        );
        let small = c(
            &mut rt,
            "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;",
            &[V::I(999)],
        );
        let d = c(
            &mut rt,
            "java/lang/Double.valueOf.(D)Ljava/lang/Double;",
            &[V::D(12345.678)],
        );
        assert_eq!(
            format(&mut rt, "%,d %,d %,.1f", vec![big, small, d]),
            "-1,234,567 999 12,345.7"
        );
        let yes = crate::api::call(
            &mut rt,
            "java/lang/Boolean.valueOf.(Z)Ljava/lang/Boolean;",
            &[V::I(1)],
        );
        let no = crate::api::call(
            &mut rt,
            "java/lang/Boolean.valueOf.(Z)Ljava/lang/Boolean;",
            &[V::I(0)],
        );
        assert_eq!(
            format(&mut rt, "%b %b %b %B", vec![yes, no, V::NULL, id]),
            "true false false TRUE"
        );
        let near = rt.string("├─In %1$s near [%2$s]");
        let dim = rt.string("minecraft:overworld");
        let pos = rt.string("20, 100, 20");
        let arr = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![dim, pos]));
        let out = c(
            &mut rt,
            "java/lang/String.formatted.([Ljava/lang/Object;)Ljava/lang/String;",
            &[near, arr],
        );
        assert_eq!(
            rt.str_of(out),
            Some("├─In minecraft:overworld near [20, 100, 20]")
        );
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
        for (pattern, x, want) in [("#.##", 0.456, ".46"), ("#.##", 0.0, "0"), ("0.00", 1.23456, "1.23"), ("#,##0.0", 1234.56, "1,234.6"), ("00", 7.0, "07")] {
            let d = rt.new_object("java/text/DecimalFormat");
            let p = rt.string(pattern);
            c(&mut rt, "java/text/DecimalFormat.<init>.(Ljava/lang/String;)V", &[d, p]);
            assert_eq!(fmt(&mut rt, d, x), want, "{pattern}");
        }
    }

    /// A call's value, or what it threw: the exception's class and message.
    fn outcome(rt: &mut Rt, key: &'static str, args: &[V]) -> Result<V, String> {
        let v = crate::api::call(rt, key, args);
        match rt.thrown.take() {
            Some(e) => {
                let m = rt.get_field(e, "message");
                Err(format!(
                    "{}: {}",
                    rt.class_of(e),
                    rt.str_of(m).unwrap_or("")
                ))
            }
            None => Ok(v),
        }
    }

    /// The boxed numbers parse as JDK 21's (the values and `NumberFormatException` messages are
    /// its): an optional sign and only digits of the radix, in range, the radix checked; floating
    /// text trimmed, with `NaN`, `Infinity` and a type suffix; `decode`'s prefixes; the unsigned
    /// forms; and print in a radix.
    #[test]
    fn numbers_parse_and_print_as_the_jdks() {
        fn parse(rt: &mut Rt, key: &'static str, s: Option<&str>, more: &[V]) -> Result<V, String> {
            let mut args = vec![s.map_or(V::NULL, |s| rt.string(s))];
            args.extend_from_slice(more);
            outcome(rt, key, &args)
        }
        let rt = &mut Rt::new();
        let nfe = |m: &str| Err(format!("java/lang/NumberFormatException: {m}"));
        const INT: &str = "java/lang/Integer.parseInt.(Ljava/lang/String;)I";
        const INT_RADIX: &str = "java/lang/Integer.parseInt.(Ljava/lang/String;I)I";
        assert_eq!(parse(rt, INT, Some("+7"), &[]), Ok(V::I(7)));
        assert_eq!(parse(rt, INT, Some("-2147483648"), &[]), Ok(V::I(i32::MIN)));
        assert_eq!(
            parse(rt, INT, Some("2147483648"), &[]),
            nfe("For input string: \"2147483648\"")
        );
        assert_eq!(
            parse(rt, INT, Some(" 42"), &[]),
            nfe("For input string: \" 42\"")
        );
        assert_eq!(
            parse(rt, INT, Some("-"), &[]),
            nfe("For input string: \"-\"")
        );
        assert_eq!(parse(rt, INT, None, &[]), nfe("Cannot parse null string"));
        assert_eq!(
            parse(rt, INT_RADIX, Some("-FF"), &[V::I(16)]),
            Ok(V::I(-255))
        );
        assert_eq!(parse(rt, INT_RADIX, Some("z"), &[V::I(36)]), Ok(V::I(35)));
        assert_eq!(
            parse(rt, INT_RADIX, Some("12"), &[V::I(2)]),
            nfe("For input string: \"12\" under radix 2")
        );
        assert_eq!(
            parse(rt, INT_RADIX, Some("ff"), &[V::I(0)]),
            nfe("radix 0 less than Character.MIN_RADIX")
        );
        assert_eq!(
            parse(rt, INT_RADIX, Some("1"), &[V::I(37)]),
            nfe("radix 37 greater than Character.MAX_RADIX")
        );
        let ten = parse(
            rt,
            "java/lang/Integer.valueOf.(Ljava/lang/String;I)Ljava/lang/Integer;",
            Some("12"),
            &[V::I(8)],
        )
        .unwrap();
        assert_eq!(rt.get_field(ten, "value"), V::I(10));
        const LONG: &str = "java/lang/Long.parseLong.(Ljava/lang/String;)J";
        assert_eq!(
            parse(rt, LONG, Some("9223372036854775807"), &[]),
            Ok(V::J(i64::MAX))
        );
        assert_eq!(
            parse(rt, LONG, Some("9223372036854775808"), &[]),
            nfe("For input string: \"9223372036854775808\"")
        );
        assert_eq!(
            parse(rt, LONG, Some("10L"), &[]),
            nfe("For input string: \"10L\"")
        );
        assert_eq!(
            parse(
                rt,
                "java/lang/Short.parseShort.(Ljava/lang/String;)S",
                Some("40000"),
                &[]
            ),
            nfe("Value out of range. Value:\"40000\" Radix:10")
        );
        assert_eq!(
            parse(
                rt,
                "java/lang/Byte.parseByte.(Ljava/lang/String;I)B",
                Some("7f"),
                &[V::I(16)]
            ),
            Ok(V::I(127))
        );
        assert_eq!(
            parse(
                rt,
                "java/lang/Byte.parseByte.(Ljava/lang/String;I)B",
                Some("80"),
                &[V::I(16)]
            ),
            nfe("Value out of range. Value:\"80\" Radix:16")
        );
        const UINT: &str = "java/lang/Integer.parseUnsignedInt.(Ljava/lang/String;I)I";
        assert_eq!(parse(rt, UINT, Some("ffffffff"), &[V::I(16)]), Ok(V::I(-1)));
        assert_eq!(
            parse(rt, UINT, Some("1ffffffff"), &[V::I(16)]),
            nfe("String value 1ffffffff exceeds range of unsigned int.")
        );
        assert_eq!(
            parse(rt, UINT, Some("-1"), &[V::I(10)]),
            nfe("Illegal leading minus sign on unsigned string -1.")
        );
        const ULONG: &str = "java/lang/Long.parseUnsignedLong.(Ljava/lang/String;)J";
        assert_eq!(
            parse(rt, ULONG, Some("18446744073709551615"), &[]),
            Ok(V::J(-1))
        );
        assert_eq!(
            parse(rt, ULONG, Some("18446744073709551616"), &[]),
            nfe("String value 18446744073709551616 exceeds range of unsigned long.")
        );
        const DECODE: &str = "java/lang/Integer.decode.(Ljava/lang/String;)Ljava/lang/Integer;";
        for (text, want) in [
            ("0x1F", 31),
            ("#ff", 255),
            ("-010", -8),
            ("0", 0),
            ("-0x80000000", i32::MIN),
        ] {
            let v = parse(rt, DECODE, Some(text), &[]).unwrap();
            assert_eq!(rt.get_field(v, "value"), V::I(want), "{text}");
        }
        assert_eq!(parse(rt, DECODE, Some(""), &[]), nfe("Zero length string"));
        assert_eq!(
            parse(rt, DECODE, Some("0x"), &[]),
            nfe("For input string: \"\" under radix 16")
        );
        assert_eq!(
            parse(rt, DECODE, Some("0x-1"), &[]),
            nfe("Sign character in wrong position")
        );
        assert_eq!(
            parse(rt, DECODE, Some("0xZZ"), &[]),
            nfe("For input string: \"ZZ\" under radix 16")
        );
        const DOUBLE: &str = "java/lang/Double.parseDouble.(Ljava/lang/String;)D";
        for (text, want) in [
            (" 1.5 ", 1.5),
            ("1e5f", 100000.0),
            ("-Infinity", f64::NEG_INFINITY),
            ("1.", 1.0),
            (".5", 0.5),
            ("0.1", 0.1),
        ] {
            assert_eq!(parse(rt, DOUBLE, Some(text), &[]), Ok(V::D(want)), "{text}");
        }
        assert!(parse(rt, DOUBLE, Some("+NaN"), &[]).is_ok_and(|v| v.d().is_nan()));
        assert_eq!(parse(rt, DOUBLE, Some("   "), &[]), nfe("empty String"));
        assert_eq!(
            parse(rt, DOUBLE, Some(" x "), &[]),
            nfe("For input string: \"x\"")
        );
        for text in ["infinity", "1e", "1.0dd"] {
            assert_eq!(
                parse(rt, DOUBLE, Some(text), &[]),
                nfe(&format!("For input string: \"{text}\""))
            );
        }
        assert_eq!(
            parse(
                rt,
                "java/lang/Float.parseFloat.(Ljava/lang/String;)F",
                Some("3.4028236e38"),
                &[]
            ),
            Ok(V::F(f32::INFINITY))
        );
        assert_eq!(
            parse(
                rt,
                "java/lang/Float.parseFloat.(Ljava/lang/String;)F",
                Some("0.1"),
                &[]
            ),
            Ok(V::F(0.1))
        );

        let mut print = |key: &'static str, args: &[V]| {
            let v = crate::api::call(rt, key, args);
            rt.str_of(v).unwrap_or("?").to_owned()
        };
        assert_eq!(
            print(
                "java/lang/Integer.toString.(II)Ljava/lang/String;",
                &[V::I(-255), V::I(16)]
            ),
            "-ff"
        );
        assert_eq!(
            print(
                "java/lang/Integer.toString.(II)Ljava/lang/String;",
                &[V::I(255), V::I(99)]
            ),
            "255"
        );
        assert_eq!(
            print(
                "java/lang/Integer.toString.(II)Ljava/lang/String;",
                &[V::I(i32::MIN), V::I(2)]
            ),
            "-10000000000000000000000000000000"
        );
        assert_eq!(
            print(
                "java/lang/Long.toString.(JI)Ljava/lang/String;",
                &[V::J(i64::MAX), V::I(36)]
            ),
            "1y2p0ij32e8e7"
        );
        assert_eq!(
            print(
                "java/lang/Integer.toUnsignedString.(I)Ljava/lang/String;",
                &[V::I(-1)]
            ),
            "4294967295"
        );
        assert_eq!(
            crate::api::call(rt, "java/lang/Byte.toUnsignedInt.(B)I", &[V::I(-1)]),
            V::I(255)
        );
        assert_eq!(
            crate::api::call(rt, "java/lang/Short.toUnsignedInt.(S)I", &[V::I(-1)]),
            V::I(65535)
        );
        assert_eq!(
            crate::api::call(rt, "java/lang/Integer.toUnsignedLong.(I)J", &[V::I(-1)]),
            V::J(4294967295)
        );
        assert_eq!(
            crate::api::call(
                rt,
                "java/lang/Integer.compareUnsigned.(II)I",
                &[V::I(-1), V::I(1)]
            ),
            V::I(1)
        );
        assert_eq!(
            crate::api::call(
                rt,
                "java/lang/Long.compareUnsigned.(JJ)I",
                &[V::J(1), V::J(-1)]
            ),
            V::I(-1)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Math`'s integer methods as JDK 21's: floor and ceiling division and modulus in each
    /// overload, exact where `/` would round toward zero or wrap, a zero divisor throwing; the
    /// `*Exact` ones throwing on overflow where `Integer.sum` wraps.
    #[test]
    fn math_integer_methods_fail_as_the_jdks() {
        let mut rt = Rt::new();
        let mut m = |key: &'static str, args: &[V]| outcome(&mut rt, key, args);
        let ae = |msg: &str| Err(format!("java/lang/ArithmeticException: {msg}"));
        assert_eq!(
            m("java/lang/Math.floorMod.(II)I", &[V::I(-7), V::I(3)]),
            Ok(V::I(2))
        );
        assert_eq!(
            m("java/lang/Math.floorMod.(II)I", &[V::I(7), V::I(-3)]),
            Ok(V::I(-2))
        );
        assert_eq!(
            m("java/lang/Math.floorMod.(II)I", &[V::I(i32::MIN), V::I(-1)]),
            Ok(V::I(0))
        );
        assert_eq!(
            m("java/lang/Math.floorMod.(II)I", &[V::I(1), V::I(0)]),
            ae("/ by zero")
        );
        assert_eq!(
            m("java/lang/Math.floorMod.(JI)I", &[V::J(-7), V::I(3)]),
            Ok(V::I(2))
        );
        assert_eq!(
            m("java/lang/Math.floorMod.(JJ)J", &[V::J(i64::MAX), V::J(7)]),
            Ok(V::J(0))
        );
        assert_eq!(
            m("java/lang/Math.floorDiv.(II)I", &[V::I(-7), V::I(3)]),
            Ok(V::I(-3))
        );
        assert_eq!(
            m("java/lang/Math.floorDiv.(II)I", &[V::I(i32::MIN), V::I(-1)]),
            Ok(V::I(i32::MIN))
        );
        assert_eq!(
            m("java/lang/Math.floorDiv.(JJ)J", &[V::J(7), V::J(-2)]),
            Ok(V::J(-4))
        );
        assert_eq!(
            m(
                "java/lang/Math.floorDiv.(JJ)J",
                &[V::J(9007199254740993), V::J(1)]
            ),
            Ok(V::J(9007199254740993))
        );
        assert_eq!(
            m(
                "java/lang/Math.floorDiv.(JI)J",
                &[V::J(-9007199254740993), V::I(2)]
            ),
            Ok(V::J(-4503599627370497))
        );
        assert_eq!(
            m("java/lang/Math.floorDiv.(JJ)J", &[V::J(i64::MIN), V::J(-1)]),
            Ok(V::J(i64::MIN))
        );
        assert_eq!(
            m("java/lang/Math.floorDiv.(JJ)J", &[V::J(1), V::J(0)]),
            ae("/ by zero")
        );
        assert_eq!(
            m("java/lang/Math.ceilDiv.(II)I", &[V::I(-7), V::I(2)]),
            Ok(V::I(-3))
        );
        assert_eq!(
            m("java/lang/Math.ceilDiv.(II)I", &[V::I(7), V::I(2)]),
            Ok(V::I(4))
        );
        assert_eq!(
            m("java/lang/Math.ceilDiv.(II)I", &[V::I(i32::MIN), V::I(-1)]),
            Ok(V::I(i32::MIN))
        );
        assert_eq!(
            m("java/lang/Math.ceilMod.(II)I", &[V::I(7), V::I(2)]),
            Ok(V::I(-1))
        );
        assert_eq!(
            m("java/lang/Math.addExact.(II)I", &[V::I(i32::MAX), V::I(1)]),
            ae("integer overflow")
        );
        assert_eq!(
            m("java/lang/Math.addExact.(II)I", &[V::I(2), V::I(3)]),
            Ok(V::I(5))
        );
        assert_eq!(
            m("java/lang/Math.addExact.(JJ)J", &[V::J(i64::MAX), V::J(1)]),
            ae("long overflow")
        );
        assert_eq!(
            m(
                "java/lang/Math.multiplyExact.(II)I",
                &[V::I(65536), V::I(65536)]
            ),
            ae("integer overflow")
        );
        assert_eq!(
            m(
                "java/lang/Math.multiplyExact.(JI)J",
                &[V::J(i64::MAX), V::I(2)]
            ),
            ae("long overflow")
        );
        assert_eq!(
            m("java/lang/Math.toIntExact.(J)I", &[V::J(1 << 31)]),
            ae("integer overflow")
        );
        assert_eq!(
            m("java/lang/Math.toIntExact.(J)I", &[V::J(-5)]),
            Ok(V::I(-5))
        );
        assert_eq!(
            m("java/lang/Math.absExact.(I)I", &[V::I(i32::MIN)]),
            ae("Overflow to represent absolute value of Integer.MIN_VALUE")
        );
        assert_eq!(
            m("java/lang/Integer.sum.(II)I", &[V::I(i32::MAX), V::I(1)]),
            Ok(V::I(i32::MIN))
        );
    }

    /// `Enum.valueOf(type, name)`, which a mod enum's `valueOf(String)` is: the constant its
    /// `values()` names so, else what JDK 21 throws (the class by its canonical name).
    #[test]
    fn enum_value_of_finds_a_mod_enums_constant_by_name() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            if (class, what) != ("t/Oracle$Mode", "values") {
                return None;
            }
            let mut values = Vec::new();
            for (i, n) in ["SPEED", "TORQUE"].into_iter().enumerate() {
                let e = rt.new_object("t/Oracle$Mode");
                let name = rt.string(n);
                crate::api::call(
                    rt,
                    "java/lang/Enum.<init>.(Ljava/lang/String;I)V",
                    &[e, name, V::I(i as i32)],
                );
                values.push(rt.intern(format!("test/mode/{n}"), |_| e));
            }
            Some(rt.alloc("[Lt/Oracle$Mode;", Data::Array(values)))
        }
        let mut rt = Rt::new();
        rt.supers.insert("t/Oracle$Mode", "java/lang/Enum");
        rt.reflect = Some(reflect);
        let class = class_object(&mut rt, "t/Oracle$Mode");
        const VALUE_OF: &str =
            "java/lang/Enum.valueOf.(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/Enum;";
        let mut value_of = |name: Option<&str>| {
            let n = name.map_or(V::NULL, |n| rt.string(n));
            outcome(&mut rt, VALUE_OF, &[class, n])
        };
        let torque = value_of(Some("TORQUE")).unwrap();
        let name = crate::api_common::enum_name(&mut rt, torque, false);
        assert_eq!(rt.str_of(name), Some("TORQUE"));
        let mut value_of = |name: Option<&str>| {
            let n = name.map_or(V::NULL, |n| rt.string(n));
            outcome(&mut rt, VALUE_OF, &[class, n])
        };
        assert_eq!(
            value_of(Some("FAST")),
            Err("java/lang/IllegalArgumentException: No enum constant t.Oracle.Mode.FAST".into())
        );
        assert_eq!(
            value_of(Some("speed")),
            Err("java/lang/IllegalArgumentException: No enum constant t.Oracle.Mode.speed".into())
        );
        assert_eq!(
            value_of(None),
            Err("java/lang/NullPointerException: Name is null".into())
        );
    }

    /// `indexOf` and `lastIndexOf` of `String` and `StringBuilder`, with and without where to
    /// start, as JDK 21 answers them: the start clamped (an empty needle is found where the
    /// search starts, at most the length; one before the start isn't), a code point by its units.
    #[test]
    fn index_of_and_last_index_of_are_the_jdks() {
        let rt = &mut Rt::new();
        let sb = rt.new_object("java/lang/StringBuilder");
        let abcabc = rt.string("abcabc");
        c(rt, "java/lang/StringBuilder.<init>.(Ljava/lang/String;)V", &[sb, abcabc]);
        let [bc, empty, x] = ["bc", "", "x"].map(|t| rt.string(t));
        let on = |rt: &mut Rt, key: &'static str, args: &[V]| c(rt, key, args).i();
        const SB_LAST: &str = "java/lang/StringBuilder.lastIndexOf.(Ljava/lang/String;)I";
        const SB_LAST_FROM: &str = "java/lang/StringBuilder.lastIndexOf.(Ljava/lang/String;I)I";
        const SB_FROM: &str = "java/lang/StringBuilder.indexOf.(Ljava/lang/String;I)I";
        const SB: &str = "java/lang/StringBuilder.indexOf.(Ljava/lang/String;)I";
        assert_eq!(on(rt, SB_LAST, &[sb, bc]), 4);
        assert_eq!(on(rt, SB_LAST_FROM, &[sb, bc, V::I(3)]), 1);
        assert_eq!(on(rt, SB_FROM, &[sb, bc, V::I(2)]), 4);
        assert_eq!(on(rt, SB_FROM, &[sb, empty, V::I(10)]), 6);
        assert_eq!(on(rt, SB_LAST_FROM, &[sb, empty, V::I(-1)]), -1);
        assert_eq!(on(rt, SB_LAST_FROM, &[sb, empty, V::I(99)]), 6);
        assert_eq!(on(rt, SB, &[sb, x]), -1);
        let [abc, smile, aaa, abab, ab] = ["abc", "a\u{1F600}b", "aaa", "abab", "ab"].map(|t| rt.string(t));
        const CHAR_FROM: &str = "java/lang/String.indexOf.(II)I";
        const CHAR: &str = "java/lang/String.indexOf.(I)I";
        assert_eq!(on(rt, CHAR_FROM, &[abc, V::I('b' as i32), V::I(5)]), -1);
        assert_eq!(on(rt, CHAR, &[smile, V::I(0x1F600)]), 1);
        assert_eq!(on(rt, CHAR, &[smile, V::I(0xD83D)]), 1);
        assert_eq!(on(rt, CHAR, &[abc, V::I(-1)]), -1);
        assert_eq!(on(rt, "java/lang/String.lastIndexOf.(II)I", &[aaa, V::I('a' as i32), V::I(-5)]), -1);
        assert_eq!(on(rt, "java/lang/String.indexOf.(Ljava/lang/String;I)I", &[abc, empty, V::I(7)]), 3);
        assert_eq!(on(rt, "java/lang/String.lastIndexOf.(Ljava/lang/String;I)I", &[abab, ab, V::I(0)]), 0);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A character's general category (`getType`) and what follows from it, checked against a
    /// Java 21 for every code point it defines: a letter and a digit of any script, what starts
    /// and continues a Java and a Unicode identifier (a JSON5 parser reads unquoted keys by the
    /// latter: unanswered, a config a mod had just written failed to read back).
    #[test]
    fn a_characters_category_and_identifier_rules_are_javas() {
        let mut rt = Rt::new();
        // (code point, `getType`, then whether: a letter, a digit, a Java identifier's start and
        // part, a Unicode identifier's start and part, ignorable in one), as Java 21 answers.
        let cases: [(i32, i32, [bool; 7]); 11] = [
            ('A' as i32, 1, [true, false, true, true, true, true, false]),
            (0x663, 9, [false, true, false, true, false, true, false]),
            (
                '$' as i32,
                26,
                [false, false, true, true, false, false, false],
            ),
            (
                '_' as i32,
                23,
                [false, false, true, true, false, true, false],
            ),
            (0xB7, 24, [false, false, false, false, false, true, false]),
            (0x200B, 16, [false, false, false, true, false, true, true]),
            (0x2160, 10, [false, false, true, true, true, true, false]),
            (0x300, 6, [false, false, false, true, false, true, false]),
            (0x1885, 6, [false, false, false, true, true, true, false]),
            (0x1F600, 28, [false; 7]),
            (' ' as i32, 12, [false; 7]),
        ];
        const ASKED: [&str; 7] = [
            "isLetter",
            "isDigit",
            "isJavaIdentifierStart",
            "isJavaIdentifierPart",
            "isUnicodeIdentifierStart",
            "isUnicodeIdentifierPart",
            "isIdentifierIgnorable",
        ];
        for (cp, category, answers) in cases {
            assert_eq!(
                c(&mut rt, "java/lang/Character.getType.(I)I", &[V::I(cp)]),
                V::I(category),
                "U+{cp:04X}"
            );
            for (name, expected) in ASKED.iter().zip(answers) {
                let key = rt.static_key(&format!("java/lang/Character.{name}.(I)Z"));
                assert_eq!(
                    c(&mut rt, key, &[V::I(cp)]),
                    V::I(expected as i32),
                    "{name} of U+{cp:04X}"
                );
            }
        }
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Character.isISOControl.(C)Z",
                &[V::I(0x85)]
            ),
            V::I(1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Character.isSupplementaryCodePoint.(I)Z",
                &[V::I(0x1F600)]
            ),
            V::I(1)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Character`'s digits and surrogates, `String.lines`, `trim` and `strip`, and
    /// `Class.getCanonicalName`, as JDK 21 answers them.
    #[test]
    fn characters_lines_and_canonical_names_are_the_jdks() {
        let mut rt = Rt::new();
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Character.digit.(CI)I",
                &[V::I('f' as i32), V::I(16)]
            ),
            V::I(15)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Character.digit.(CI)I",
                &[V::I('g' as i32), V::I(16)]
            ),
            V::I(-1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Character.digit.(CI)I",
                &[V::I('7' as i32), V::I(0)]
            ),
            V::I(-1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Character.toCodePoint.(CC)I",
                &[V::I(0xD83D), V::I(0xDE00)]
            ),
            V::I(128512)
        );
        let units = c(
            &mut rt,
            "java/lang/Character.toChars.(I)[C",
            &[V::I(0x1F600)],
        );
        assert_eq!(
            crate::jutil::elements(&rt, units),
            [V::I(0xD83D), V::I(56832)]
        );
        let text = rt.string("a\nb\r\nc\r\n\nd");
        let lines = c(
            &mut rt,
            "java/lang/String.lines.()Ljava/util/stream/Stream;",
            &[text],
        );
        let got: Vec<String> = crate::jutil::elements(&rt, lines)
            .into_iter()
            .map(|v| rt.str_of(v).unwrap_or("?").to_owned())
            .collect();
        assert_eq!(got, ["a", "b", "c", "", "d"]);
        let one = rt.string("a\n");
        let lines = c(
            &mut rt,
            "java/lang/String.lines.()Ljava/util/stream/Stream;",
            &[one],
        );
        assert_eq!(crate::jutil::elements(&rt, lines).len(), 1);
        // `trim` drops what is up to a space (a control), not a no-break or an em space; `strip`
        // Java's whitespace (the em space, the tab), not a control.
        let padded = rt.string("\u{1}\u{A0}x\u{2003}\t");
        let trimmed = c(
            &mut rt,
            "java/lang/String.trim.()Ljava/lang/String;",
            &[padded],
        );
        assert_eq!(rt.str_of(trimmed), Some("\u{A0}x\u{2003}"));
        let stripped = c(
            &mut rt,
            "java/lang/String.strip.()Ljava/lang/String;",
            &[padded],
        );
        assert_eq!(rt.str_of(stripped), Some("\u{1}\u{A0}x"));
        for (class, want) in [
            ("t/Oracle$Mode", Some("t.Oracle.Mode")),
            ("[[I", Some("int[][]")),
            ("[Lt/Oracle$Mode;", Some("t.Oracle.Mode[]")),
            ("t/Oracle$2", None),
        ] {
            let k = class_object(&mut rt, class);
            let v = crate::api::call(
                &mut rt,
                "java/lang/Class.getCanonicalName.()Ljava/lang/String;",
                &[k],
            );
            assert_eq!(rt.str_of(v), want, "{class}");
        }
    }

    /// A throwable keeps what it suppressed (try-with-resources' closing failures), refusing null
    /// and itself as JDK 21 does; `printStackTrace` and the stack's methods are answered; prints
    /// to `System.out` and `System.err` go through their streams.
    #[test]
    fn throwables_keep_what_they_suppress_and_streams_print() {
        let mut rt = Rt::new();
        let e = rt.new_object("java/lang/RuntimeException");
        let x = rt.string("x");
        crate::api::call(
            &mut rt,
            "java/lang/RuntimeException.<init>.(Ljava/lang/String;)V",
            &[e, x],
        );
        const ADD: &str = "java/lang/Throwable.addSuppressed.(Ljava/lang/Throwable;)V";
        assert_eq!(
            outcome(&mut rt, ADD, &[e, V::NULL]),
            Err("java/lang/NullPointerException: Cannot suppress a null exception.".into())
        );
        crate::api::call(&mut rt, ADD, &[e, e]);
        let iae = rt.thrown.take().unwrap();
        assert_eq!(rt.class_of(iae), "java/lang/IllegalArgumentException");
        assert_eq!(rt.get_field(iae, "cause"), e);
        const GET: &str = "java/lang/Throwable.getSuppressed.()[Ljava/lang/Throwable;";
        let none = crate::api::call(&mut rt, GET, &[e]);
        assert_eq!(rt.array_len(none), 0);
        let (a, b) = (
            rt.new_object("java/lang/IllegalStateException"),
            rt.new_object("java/lang/IllegalStateException"),
        );
        crate::api::call(&mut rt, ADD, &[e, a]);
        crate::api::call(&mut rt, ADD, &[e, b]);
        let all = crate::api::call(&mut rt, GET, &[e]);
        assert_eq!(crate::jutil::elements(&rt, all), [a, b]);
        assert_eq!(
            crate::api::call(
                &mut rt,
                "java/lang/Throwable.fillInStackTrace.()Ljava/lang/Throwable;",
                &[e]
            ),
            e
        );
        crate::api::call(&mut rt, "java/lang/Exception.printStackTrace.()V", &[e]);
        let out = crate::api::get_static(&mut rt, "java/lang/System.out");
        assert_eq!(
            crate::api::get_static(&mut rt, "java/lang/System.out"),
            out,
            "one System.out"
        );
        crate::api::call(
            &mut rt,
            "java/lang/Throwable.printStackTrace.(Ljava/io/PrintStream;)V",
            &[e, out],
        );
        crate::api::call(
            &mut rt,
            "java/io/PrintStream.println.(Ljava/lang/String;)V",
            &[out, x],
        );
        let f = rt.string("%d%n");
        let values = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![V::I(3)]));
        assert_eq!(
            crate::api::call(
                &mut rt,
                "java/io/PrintStream.printf.(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
                &[out, f, values]
            ),
            out
        );
        assert!(
            rt.unmapped.is_empty() && rt.thrown.is_none(),
            "{:?}",
            rt.unmapped
        );
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

const PRINT_STREAM: &str = "java/io/PrintStream";

/// `System.out` and `System.err` (`err`): one stream each.
pub(crate) fn std_stream(rt: &mut Rt, err: bool) -> V {
    rt.intern(format!("compat:std/{err}"), |rt| {
        let s = rt.new_object(PRINT_STREAM);
        rt.put_field(s, "$err", V::I(err as i32));
        s
    })
}

/// What a mod prints to `System.out` or `System.err` (`err`), which NeoForge's console takes as
/// its log: printed when tracing (`HORIZON_COMPAT_TRACE`), as the mods' log is.
pub(crate) fn print_std(err: bool, text: &str) {
    if Rt::tracing() {
        let to = if err { "stderr" } else { "stdout" };
        for line in text.strip_suffix('\n').unwrap_or(text).split('\n') {
            eprintln!("horizon-compat: mod {to}: {line}");
        }
    }
}

/// A `PrintStream` (`System.out`, `System.err`): `print`, `println`, `printf`, `format`,
/// `append` and `write` print what they are given ([`print_std`]); `flush` and `close` have
/// nothing to do.
fn print_stream(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let ks = kinds(desc);
    let text = match name {
        "print" | "println" | "append" => {
            let mut s = match ks.first() {
                None => String::new(),
                Some('[') => crate::jutil::elements(rt, a(1))
                    .iter()
                    .map(|v| char::from_u32(v.i() as u32 & 0xffff).unwrap_or('\u{fffd}'))
                    .collect(),
                Some(&k) => rt.to_java_string(a(1), k),
            };
            if name == "println" {
                s.push('\n');
            }
            s
        }
        // `printf(format, args...)`, with a locale first or not.
        "printf" | "format" => {
            let at = if desc.starts_with("(Ljava/util/Locale;") {
                2
            } else {
                1
            };
            let f = text(rt, a(at));
            let values = crate::jutil::elements(rt, a(at + 1));
            format_java(rt, &f, &values)
        }
        "write" => match ks.as_slice() {
            ['I'] => char::from(a(1).i() as u8).to_string(),
            _ => {
                let bytes: Vec<u8> = crate::jutil::elements(rt, a(1))
                    .iter()
                    .map(|v| v.i() as u8)
                    .collect();
                let (from, len) = if ks.len() == 3 {
                    (a(2).i().max(0) as usize, a(3).i().max(0) as usize)
                } else {
                    (0, bytes.len())
                };
                String::from_utf8_lossy(
                    bytes
                        .get(from..from.saturating_add(len))
                        .unwrap_or_default(),
                )
                .into_owned()
            }
        },
        "flush" | "close" => return Some(V::Void),
        "checkError" => return Some(V::I(0)),
        _ => return None,
    };
    print_std(rt.get_field(this, "$err").i() != 0, &text);
    Some(if desc.ends_with(")V") { V::Void } else { this })
}

/// Small libraries mods lean on: loggers (printed only when tracing), commons-lang `Pair`,
/// `Record`'s constructor.
/// Vanilla's `Util.offsetByCodepoints`: `cursor` (a UTF-16 index of `text`) moved `by` code points,
/// a surrogate pair counting once, stopping at either end.
fn offset_by_codepoints(text: &str, mut cursor: i32, by: i32) -> i32 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let len = units.len() as i32;
    let unit = |i: i32| {
        usize::try_from(i)
            .ok()
            .and_then(|i| units.get(i))
            .copied()
            .unwrap_or(0)
    };
    let high = |u: u16| (0xD800..0xDC00).contains(&u);
    let low = |u: u16| (0xDC00..0xE000).contains(&u);
    if by >= 0 {
        let mut moved = 0;
        while cursor < len && moved < by {
            let c = unit(cursor);
            cursor += 1;
            if high(c) && cursor < len && low(unit(cursor)) {
                cursor += 1;
            }
            moved += 1;
        }
    } else {
        let mut left = by;
        while cursor > 0 && left < 0 {
            cursor -= 1;
            if low(unit(cursor)) && cursor > 0 && high(unit(cursor - 1)) {
                cursor -= 1;
            }
            left += 1;
        }
    }
    cursor
}

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
        ("org/apache/logging/log4j/LogManager" | "org/slf4j/LoggerFactory" | "com/mojang/logging/LogUtils", "getLogger" | "getFormatterLogger" | "getRootLogger") => {
            rt.intern("compat:logger".into(), |rt| rt.new_object("org/apache/logging/log4j/Logger"))
        }
        ("org/apache/logging/log4j/MarkerManager", "getMarker") => a(0),
        ("org/apache/commons/lang3/tuple/Pair" | "org/apache/commons/lang3/tuple/ImmutablePair" | "org/apache/commons/lang3/tuple/MutablePair" | "com/mojang/datafixers/util/Pair", "of" | "<init>") => {
            let made = if owner.ends_with("MutablePair") { "org/apache/commons/lang3/tuple/MutablePair" } else { "org/apache/commons/lang3/tuple/Pair" };
            // `new MutablePair<>()` holds null and null, as its fields start.
            let half = |v: V| if matches!(v, V::Void) { V::NULL } else { v };
            let (p, l, r) = if name == "<init>" {
                (a(0), half(a(1)), half(a(2)))
            } else {
                (rt.new_object(made), a(0), a(1))
            };
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
        ("java/text/BreakIterator", _) => return crate::breaks::call(rt, name, desc, args),
        ("java/text/StringCharacterIterator" | "java/text/CharacterIterator", _) => return crate::breaks::string_iterator_call(rt, name, desc, args),
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
        // `WordUtils.capitalize(text)`: the first letter of each word (what follows whitespace)
        // in upper case; `capitalizeFully` puts the rest in lower case first.
        ("org/apache/commons/lang3/text/WordUtils" | "org/apache/commons/text/WordUtils", "capitalize" | "capitalizeFully") if desc.starts_with("(Ljava/lang/String;)") => {
            if a(0).r() == 0 {
                return Some(V::NULL);
            }
            let t = if name == "capitalizeFully" { text(rt, a(0)).to_lowercase() } else { text(rt, a(0)) };
            let mut out = String::with_capacity(t.len());
            let mut starts = true;
            for c in t.chars() {
                if java_whitespace(c) {
                    starts = true;
                    out.push(c);
                } else if starts {
                    starts = false;
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
            }
            new_str(rt, out)
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
        // The system the game runs on, as vanilla reads it from `os.name` (a BSD is `UNKNOWN`).
        ("net/minecraft/Util", "getPlatform") => {
            let os = if cfg!(target_os = "windows") {
                "WINDOWS"
            } else if cfg!(target_os = "macos") {
                "OSX"
            } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
                "SOLARIS"
            } else if cfg!(target_os = "linux") {
                "LINUX"
            } else {
                "UNKNOWN"
            };
            crate::records::enum_constant(rt, &format!("net/minecraft/Util$OS.{os}"))?
        }
        // Vanilla's shared pools, one each: what they are given runs at once (one runtime, one thread).
        ("net/minecraft/Util", "backgroundExecutor" | "ioPool" | "nonCriticalIoPool") => {
            rt.intern("compat:util-executor".into(), |rt| crate::api::call(rt, "java/util/concurrent/Executors.newCachedThreadPool.()Ljava/util/concurrent/ExecutorService;", &[]))
        }
        // `getRandom(values, random)`: `values[random.nextInt(values.length)]`; `getRandomSafe` an
        // `Optional`, empty for no values.
        ("net/minecraft/Util", "getRandom" | "getRandomSafe") => {
            let items = crate::jutil::elements(rt, a(0));
            let picked = if items.is_empty() {
                V::NULL
            } else {
                let i = crate::api::call(rt, "net/minecraft/util/RandomSource.nextInt.(I)I", &[a(1), V::I(items.len() as i32)]).i();
                items.get(i as usize).copied().unwrap_or(V::NULL)
            };
            if name == "getRandomSafe" { crate::api_common::optional(rt, picked) } else { picked }
        }
        // `shuffle(list, random)`: vanilla's Fisher-Yates, from the end, in place.
        ("net/minecraft/Util", "shuffle") => {
            let mut items = crate::jutil::elements(rt, a(0));
            for j in (2..=items.len()).rev() {
                let k = crate::api::call(rt, "net/minecraft/util/RandomSource.nextInt.(I)I", &[a(1), V::I(j as i32)]).i();
                items.swap(j - 1, (k.max(0) as usize).min(j - 1));
            }
            if let Some(Data::List(v)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data) {
                *v = items;
            }
            V::Void
        }
        // `offsetByCodepoints(text, cursor, by)`: the cursor moved by code points, in UTF-16 units.
        ("net/minecraft/Util", "offsetByCodepoints") => {
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            V::I(offset_by_codepoints(&text, a(1).i(), a(2).i()))
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
        "setRoundingMode"
        | "setMaximumIntegerDigits"
        | "setParseIntegerOnly"
        | "setDecimalFormatSymbols"
        | "setDecimalSeparatorAlwaysShown"
        | "setParseBigDecimal"
        | "setNegativePrefix"
        | "setPositivePrefix"
        | "setCurrency" => V::Void,
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
