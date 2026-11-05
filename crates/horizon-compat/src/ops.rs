//! DFU's `DynamicOps` over this runtime's NBT tags, as Minecraft's `NbtOps` is: what a mod's own
//! codec reads and writes through (`ops.getList(input)`, `ops.createString(s)`,
//! `ops.listBuilder()`), with `MapLike`, `ListBuilder`, `RecordBuilder` and `Lifecycle`. A value
//! is a tag object (`IntTag`, `StringTag`, `ListTag`, `CompoundTag`...), and `empty()` is
//! `EndTag`. A `RegistryOps` here is the ops it wraps ([`crate::codecs`]), so it is this too.
use crate::nbt::{self, Nbt};
use crate::{Data, Rt, V};
use std::collections::BTreeMap;

pub(crate) const NBT_OPS: &str = "net/minecraft/nbt/NbtOps";
/// `MapLike` over a compound: [compound].
pub(crate) const MAP_LIKE: &str = "compat:ops/map_like";
/// `ListBuilder`: fields `$items` (a list of tags), `$error`.
const LIST_BUILDER: &str = "compat:ops/list_builder";
/// `RecordBuilder`: fields `$entries` (a compound being filled), `$error`.
const MAP_BUILDER: &str = "compat:ops/map_builder";
const END: &str = "net/minecraft/nbt/EndTag";
const LIFECYCLE: &str = "com/mojang/serialization/Lifecycle";
const STREAM: &str = "java/util/stream/Stream";
const PAIR: &str = "com/mojang/datafixers/util/Pair";
const DATA_RESULT: &str = "com/mojang/serialization/DataResult";
const ENCODE_START: &str = "com/mojang/serialization/Encoder.encodeStart.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;";

/// `NbtOps.INSTANCE`, the one ops object (what `codecs::get_static` answers).
pub(crate) fn nbt_ops(rt: &mut Rt) -> V {
    rt.intern("compat:nbt-ops".into(), |rt| rt.new_object(NBT_OPS))
}

/// `EndTag.INSTANCE`: `NbtOps.empty()`, nothing.
pub(crate) fn end(rt: &mut Rt) -> V {
    rt.intern("compat:end-tag".into(), |rt| rt.new_object(END))
}

/// Whether this module answers a call on `receiver` (or a static of `owner`).
pub(crate) fn takes(rt: &Rt, owner: &str, receiver: V) -> bool {
    owner == LIFECYCLE || matches!(rt.class_of(receiver), NBT_OPS | MAP_LIKE | LIST_BUILDER | MAP_BUILDER | LIFECYCLE)
}

/// A tag as game NBT: None for `EndTag` and what isn't a tag.
fn to_nbt(rt: &Rt, v: V) -> Option<Nbt> {
    if rt.class_of(v) == END {
        return None;
    }
    nbt::element_from_runtime(rt, v)
}

fn tag(rt: &mut Rt, n: &Nbt) -> V {
    nbt::element_to_runtime(rt, n).unwrap_or(V::NULL)
}

/// A tag read from JSON, as `JsonOps` converts to `NbtOps` (`null` is `EndTag`).
pub(crate) fn json_tag(rt: &mut Rt, j: &serde_json::Value) -> V {
    match crate::recipes::json_nbt(j) {
        Some(n) => tag(rt, &n),
        None => end(rt),
    }
}

fn boxed(rt: &mut Rt, class: &'static str, v: V) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// A tag's number, boxed as `NumericTag.getAsNumber()` boxes it.
fn number_of(rt: &mut Rt, n: &Nbt) -> Option<V> {
    Some(match *n {
        Nbt::Byte(b) => boxed(rt, "java/lang/Byte", V::I(i32::from(b))),
        Nbt::Short(s) => boxed(rt, "java/lang/Short", V::I(i32::from(s))),
        Nbt::Int(i) => boxed(rt, "java/lang/Integer", V::I(i)),
        Nbt::Long(l) => boxed(rt, "java/lang/Long", V::J(l)),
        Nbt::Float(f) => boxed(rt, "java/lang/Float", V::F(f)),
        Nbt::Double(d) => boxed(rt, "java/lang/Double", V::D(d)),
        _ => return None,
    })
}

/// A boxed Java number as the tag `createNumeric` makes of it.
fn numeric_nbt(rt: &Rt, number: V) -> Nbt {
    let raw = match number {
        V::R(_) => rt.get_field(number, "value"),
        raw => raw,
    };
    match (rt.class_of(number), raw) {
        ("java/lang/Byte", v) => Nbt::Byte(v.i() as i8),
        ("java/lang/Short", v) => Nbt::Short(v.i() as i16),
        (_, V::J(l)) => Nbt::Long(l),
        (_, V::F(f)) => Nbt::Float(f),
        (_, V::D(d)) => Nbt::Double(d),
        (_, v) => Nbt::Int(v.i()),
    }
}

fn success(rt: &mut Rt, v: V) -> V {
    crate::codecs::success(rt, v)
}

fn error(rt: &mut Rt, message: &str) -> V {
    crate::codecs::error(rt, message)
}

fn stream(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc(STREAM, Data::Stream(items))
}

fn pair(rt: &mut Rt, a: V, b: V) -> V {
    let p = rt.new_object(PAIR);
    rt.put_field(p, "$first", a);
    rt.put_field(p, "$second", b);
    p
}

/// What a stream, a collection or an array holds.
fn items(rt: &Rt, v: V) -> Vec<V> {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Stream(items)) => items.clone(),
        _ => crate::jutil::elements(rt, v),
    }
}

/// A list's or an array's elements as tags (`NbtOps.getStream`).
fn elements(n: &Nbt) -> Option<Vec<Nbt>> {
    Some(match n {
        Nbt::List(l) => l.clone(),
        Nbt::ByteArray(a) => a.iter().map(|&b| Nbt::Byte(b)).collect(),
        Nbt::IntArray(a) => a.iter().map(|&i| Nbt::Int(i)).collect(),
        Nbt::LongArray(a) => a.iter().map(|&l| Nbt::Long(l)).collect(),
        _ => return None,
    })
}

/// `NbtOps.createList`: bytes, ints or longs alone make their array tag, anything else a list.
fn list_of(items: Vec<Nbt>) -> Nbt {
    if !items.is_empty() {
        if items.iter().all(|n| matches!(n, Nbt::Byte(_))) {
            return Nbt::ByteArray(items.iter().map(|n| if let Nbt::Byte(b) = n { *b } else { 0 }).collect());
        }
        if items.iter().all(|n| matches!(n, Nbt::Int(_))) {
            return Nbt::IntArray(items.iter().map(|n| if let Nbt::Int(i) = n { *i } else { 0 }).collect());
        }
        if items.iter().all(|n| matches!(n, Nbt::Long(_))) {
            return Nbt::LongArray(items.iter().map(|n| if let Nbt::Long(l) = n { *l } else { 0 }).collect());
        }
    }
    Nbt::List(items)
}

/// A key tag's text (a map's keys are strings in NBT).
fn key_text(rt: &Rt, key: V) -> Option<String> {
    match to_nbt(rt, key) {
        Some(Nbt::String(s)) => Some(s),
        _ => rt.str_of(key).map(str::to_owned),
    }
}

/// A compound's entries, or None for anything else (`EndTag` is no entries, as a prefix).
fn compound(rt: &Rt, v: V) -> Option<BTreeMap<String, Nbt>> {
    match to_nbt(rt, v) {
        Some(Nbt::Compound(m)) => Some(m),
        None if rt.class_of(v) == END => Some(BTreeMap::new()),
        _ => None,
    }
}

/// `MapLike` over `compound`.
pub(crate) fn map_like(rt: &mut Rt, compound: V) -> V {
    rt.alloc(MAP_LIKE, Data::Array(vec![compound]))
}

/// The compound a `MapLike` of these reads.
pub(crate) fn map_like_tag(rt: &Rt, m: V) -> Option<V> {
    (rt.class_of(m) == MAP_LIKE).then(|| rt.captured(m).first().copied().unwrap_or_default())
}

pub(crate) fn map_builder(rt: &mut Rt) -> V {
    let b = rt.new_object(MAP_BUILDER);
    let entries = crate::tags::new_compound(rt);
    rt.put_field(b, "$entries", entries);
    b
}

fn list_builder(rt: &mut Rt) -> V {
    let b = rt.new_object(LIST_BUILDER);
    let items = crate::jutil::new_list(rt, Vec::new());
    rt.put_field(b, "$items", items);
    b
}

/// A `DataResult`'s value, or its error.
fn result(rt: &mut Rt, r: V) -> Result<V, String> {
    crate::codecs::result_of(rt, r)
}

/// Keeps a builder's first error (later ones join it, as DFU's builders do).
pub(crate) fn fail(rt: &mut Rt, builder: V, message: &str) {
    let text = match rt.str_of(rt.get_field(builder, "$error")) {
        Some(before) => format!("{before}; {message}"),
        None => message.to_owned(),
    };
    let s = rt.string(&text);
    rt.put_field(builder, "$error", s);
}

/// What a builder's `add(..., encoder)` writes: the encoder's `encodeStart(ops, value)`.
fn encoded(rt: &mut Rt, encoder: V, value: V) -> V {
    let ops = nbt_ops(rt);
    rt.call_functional(ENCODE_START, encoder, &[ops, value])
}

/// `mergeToList(list, values)`: `EndTag` or a list (or an array) gets them appended.
fn merge_to_list(rt: &mut Rt, list: V, values: &[V]) -> V {
    let mut out = match to_nbt(rt, list) {
        None if rt.class_of(list) == END => Vec::new(),
        Some(n) => match elements(&n) {
            Some(e) => e,
            None => return error(rt, &format!("mergeToList called with not a list: {n:?}")),
        },
        None => return error(rt, "mergeToList called with not a list"),
    };
    for &v in values {
        match to_nbt(rt, v) {
            Some(n) => out.push(n),
            None => return error(rt, "mergeToList called with not a tag"),
        }
    }
    let t = tag(rt, &list_of(out));
    success(rt, t)
}

/// `mergeToMap(map, entries)`: `EndTag` or a compound gets them put.
fn merge_to_map(rt: &mut Rt, map: V, entries: Vec<(V, V)>) -> V {
    let Some(mut out) = compound(rt, map) else { return error(rt, "mergeToMap called with not a map") };
    for (k, v) in entries {
        let Some(key) = key_text(rt, k) else { return error(rt, "key is not a string") };
        match to_nbt(rt, v) {
            Some(n) => {
                out.insert(key, n);
            }
            None => return error(rt, &format!("{key}: not a tag")),
        }
    }
    let t = tag(rt, &Nbt::Compound(out));
    success(rt, t)
}

/// A map's entries as key and value tags (`MapLike.entries`, `getMapValues`, a `Map<T, T>`).
fn entry_tags(rt: &mut Rt, m: &BTreeMap<String, Nbt>) -> Vec<(V, V)> {
    m.iter()
        .map(|(k, n)| {
            let key = tag(rt, &Nbt::String(k.clone()));
            let value = tag(rt, n);
            (key, value)
        })
        .collect()
}

/// The pairs a `createMap`/`mergeToMap` argument holds: a stream of `Pair`s, a `Map`, a `MapLike`.
fn pairs_of(rt: &mut Rt, v: V) -> Vec<(V, V)> {
    if let Some(c) = map_like_tag(rt, v) {
        let m = compound(rt, c).unwrap_or_default();
        return entry_tags(rt, &m);
    }
    if matches!(rt.heap.get(v.r() as usize).map(|o| &o.data), Some(Data::Table(_))) {
        return crate::jutil::table_pairs(rt, v);
    }
    items(rt, v).into_iter().map(|p| (rt.get_field(p, "$first"), rt.get_field(p, "$second"))).collect()
}

/// A call on `NbtOps`, a `MapLike`, a builder or a `Lifecycle` of these; None for what isn't.
pub(crate) fn call(rt: &mut Rt, method: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match rt.class_of(a(0)) {
        NBT_OPS => ops_method(rt, method, desc, args),
        MAP_LIKE => {
            let c = map_like_tag(rt, a(0))?;
            let m = compound(rt, c).unwrap_or_default();
            Some(match method {
                "get" => {
                    let key = if desc.starts_with("(Ljava/lang/String;)") { rt.str_of(a(1)).map(str::to_owned) } else { key_text(rt, a(1)) };
                    match key.and_then(|k| m.get(&k).cloned()) {
                        Some(n) => tag(rt, &n),
                        None => V::NULL,
                    }
                }
                "entries" => {
                    let pairs = entry_tags(rt, &m);
                    let items = pairs.into_iter().map(|(k, v)| pair(rt, k, v)).collect();
                    stream(rt, items)
                }
                "toString" => rt.string(&format!("MapLike[{m:?}]")),
                _ => return None,
            })
        }
        LIST_BUILDER => list_builder_method(rt, method, desc, args),
        MAP_BUILDER => map_builder_method(rt, method, desc, args),
        // `Lifecycle.stable().add(other)`: lifecycles aren't kept.
        LIFECYCLE => Some(a(0)),
        _ if desc.starts_with("()") || desc.starts_with("(I)") => Some(rt.intern("compat:lifecycle".into(), |rt| rt.new_object(LIFECYCLE))),
        _ => None,
    }
}

fn ops_method(rt: &mut Rt, method: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let read = |rt: &Rt, i: usize| to_nbt(rt, args.get(i).copied().unwrap_or_default());
    Some(match method {
        "empty" => end(rt),
        "emptyMap" => crate::tags::new_compound(rt),
        "emptyList" => crate::tags::new_list(rt, Vec::new()),
        "createString" => {
            let s = rt.str_of(a(1)).unwrap_or("").to_owned();
            tag(rt, &Nbt::String(s))
        }
        "createByte" => tag(rt, &Nbt::Byte(a(1).i() as i8)),
        "createShort" => tag(rt, &Nbt::Short(a(1).i() as i16)),
        "createInt" => tag(rt, &Nbt::Int(a(1).i())),
        "createLong" => tag(rt, &Nbt::Long(a(1).j())),
        "createFloat" => tag(rt, &Nbt::Float(a(1).f())),
        "createDouble" => tag(rt, &Nbt::Double(a(1).d())),
        "createBoolean" => tag(rt, &Nbt::Byte((a(1).i() != 0) as i8)),
        "createNumeric" => {
            let n = numeric_nbt(rt, a(1));
            tag(rt, &n)
        }
        "getStringValue" => match read(rt, 1) {
            Some(Nbt::String(s)) => {
                let s = rt.string(&s);
                success(rt, s)
            }
            _ => error(rt, "Not a string"),
        },
        "getNumberValue" => {
            let n = read(rt, 1).and_then(|n| number_of(rt, &n));
            match (n, desc.starts_with("(Ljava/lang/Object;Ljava/lang/Number;)")) {
                (Some(n), true) => n,
                (None, true) => a(2),
                (Some(n), false) => success(rt, n),
                (None, false) => error(rt, "Not a number"),
            }
        }
        "getBooleanValue" => match read(rt, 1).and_then(|n| number_of(rt, &n)) {
            Some(n) => {
                let raw = rt.get_field(n, "value");
                let b = crate::api_common::boolean(rt, !matches!(raw, V::I(0)) && raw != V::J(0));
                success(rt, b)
            }
            None => error(rt, "Not a boolean"),
        },
        "getStream" | "getList" => match read(rt, 1).as_ref().and_then(elements) {
            Some(items) => {
                let tags: Vec<V> = items.iter().map(|n| tag(rt, n)).collect();
                let v = if method == "getStream" {
                    stream(rt, tags)
                } else {
                    let list = crate::jutil::new_list(rt, tags);
                    crate::functions::feed(rt, list, false)
                };
                success(rt, v)
            }
            None => error(rt, "Not a list"),
        },
        "createList" => {
            let tags: Vec<Nbt> = items(rt, a(1)).into_iter().filter_map(|v| to_nbt(rt, v)).collect();
            tag(rt, &list_of(tags))
        }
        "getMap" => match compound(rt, a(1)).filter(|_| rt.class_of(a(1)) != END) {
            Some(_) => {
                let m = map_like(rt, a(1));
                success(rt, m)
            }
            None => error(rt, "Not a map"),
        },
        "getMapValues" | "getMapEntries" => match compound(rt, a(1)).filter(|_| rt.class_of(a(1)) != END) {
            Some(m) => {
                let pairs = entry_tags(rt, &m);
                let v = if method == "getMapValues" {
                    let items = pairs.into_iter().map(|(k, v)| pair(rt, k, v)).collect();
                    stream(rt, items)
                } else {
                    let items = pairs.into_iter().map(|(k, v)| pair(rt, k, v)).collect();
                    let list = crate::jutil::new_list(rt, items);
                    crate::functions::feed(rt, list, true)
                };
                success(rt, v)
            }
            None => error(rt, "Not a map"),
        },
        "createMap" => {
            let pairs = pairs_of(rt, a(1));
            let empty = end(rt);
            let r = merge_to_map(rt, empty, pairs);
            result(rt, r).unwrap_or(V::NULL)
        }
        "mergeToList" => {
            let values = if desc.starts_with("(Ljava/lang/Object;Ljava/util/List;)") { items(rt, a(2)) } else { vec![a(2)] };
            merge_to_list(rt, a(1), &values)
        }
        "mergeToMap" => {
            let entries = if desc.starts_with("(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)") { vec![(a(2), a(3))] } else { pairs_of(rt, a(2)) };
            merge_to_map(rt, a(1), entries)
        }
        "mergeToPrimitive" => {
            if rt.class_of(a(1)) == END {
                success(rt, a(2))
            } else {
                error(rt, "Do not know how to append a primitive value to a prefix")
            }
        }
        "get" | "getGeneric" => {
            let key = if method == "get" { rt.str_of(a(2)).map(str::to_owned) } else { key_text(rt, a(2)) };
            match (compound(rt, a(1)), key) {
                (Some(m), Some(k)) => match m.get(&k) {
                    Some(n) => {
                        let t = tag(rt, n);
                        success(rt, t)
                    }
                    None => error(rt, &format!("No element {k} in the map")),
                },
                _ => error(rt, "Not a map"),
            }
        }
        "set" => {
            let key = rt.str_of(a(2)).unwrap_or("").to_owned();
            let mut m = compound(rt, a(1)).unwrap_or_default();
            if let Some(n) = to_nbt(rt, a(3)) {
                m.insert(key, n);
            }
            tag(rt, &Nbt::Compound(m))
        }
        "remove" => {
            let key = rt.str_of(a(2)).unwrap_or("").to_owned();
            match compound(rt, a(1)) {
                Some(mut m) => {
                    m.remove(&key);
                    tag(rt, &Nbt::Compound(m))
                }
                None => a(1),
            }
        }
        "listBuilder" => list_builder(rt),
        "mapBuilder" => map_builder(rt),
        // One representation: converting to the other ops (`JsonOps`) keeps the tag.
        "convertTo" | "convertList" | "convertMap" => a(2),
        "compressMaps" => V::I(0),
        "toString" => rt.string("NBT"),
        _ => return None,
    })
}

fn list_builder_method(rt: &mut Rt, method: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let b = a(0);
    let push = |rt: &mut Rt, v: V| {
        let items = rt.get_field(b, "$items");
        crate::jutil::list_add(rt, items, v);
    };
    let add_result = |rt: &mut Rt, r: V| match result(rt, r) {
        Ok(v) => push(rt, v),
        Err(e) => fail(rt, b, &e),
    };
    Some(match method {
        "ops" => nbt_ops(rt),
        "add" if desc.starts_with("(Lcom/mojang/serialization/DataResult;") => {
            add_result(rt, a(1));
            b
        }
        "add" if desc.starts_with("(Ljava/lang/Object;Lcom/mojang/serialization/Encoder;)") => {
            let r = encoded(rt, a(2), a(1));
            add_result(rt, r);
            b
        }
        "add" => {
            push(rt, a(1));
            b
        }
        "addAll" => {
            for v in items(rt, a(1)) {
                let r = encoded(rt, a(2), v);
                add_result(rt, r);
            }
            b
        }
        "withErrorsFrom" => {
            if let Err(e) = result(rt, a(1)) {
                fail(rt, b, &e);
            }
            b
        }
        "mapError" => b,
        "build" => {
            let prefix = if rt.class_of(a(1)) == DATA_RESULT {
                match result(rt, a(1)) {
                    Ok(p) => p,
                    Err(e) => return Some(error(rt, &e)),
                }
            } else {
                a(1)
            };
            if let Some(e) = rt.str_of(rt.get_field(b, "$error")).map(str::to_owned) {
                return Some(error(rt, &e));
            }
            let items = rt.get_field(b, "$items");
            let values = crate::jutil::elements(rt, items);
            merge_to_list(rt, prefix, &values)
        }
        _ => return None,
    })
}

fn map_builder_method(rt: &mut Rt, method: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let b = a(0);
    // `add(key, value)` in any of its forms, once both are tags.
    let put = |rt: &mut Rt, key: Result<String, String>, value: Result<V, String>| match (key, value) {
        (Ok(k), Ok(v)) => match to_nbt(rt, v) {
            Some(n) => {
                let entries = rt.get_field(b, "$entries");
                let mut m = compound(rt, entries).unwrap_or_default();
                m.insert(k, n);
                let t = tag(rt, &Nbt::Compound(m));
                rt.put_field(b, "$entries", t);
            }
            None => fail(rt, b, "a value that is not a tag"),
        },
        (Err(e), _) | (_, Err(e)) => fail(rt, b, &e),
    };
    Some(match method {
        "ops" => nbt_ops(rt),
        "add" => {
            let string_key = desc.starts_with("(Ljava/lang/String;");
            let key = if string_key {
                rt.str_of(a(1)).map(str::to_owned).ok_or_else(|| "a null key".to_owned())
            } else if desc.starts_with("(Lcom/mojang/serialization/DataResult;") {
                result(rt, a(1)).and_then(|k| key_text(rt, k).ok_or_else(|| "key is not a string".to_owned()))
            } else {
                key_text(rt, a(1)).ok_or_else(|| "key is not a string".to_owned())
            };
            let value = if desc.ends_with("Lcom/mojang/serialization/Encoder;)Lcom/mojang/serialization/RecordBuilder;") {
                let r = encoded(rt, a(3), a(2));
                result(rt, r)
            } else if desc.contains(";Lcom/mojang/serialization/DataResult;)") {
                result(rt, a(2))
            } else {
                Ok(a(2))
            };
            put(rt, key, value);
            b
        }
        "withErrorsFrom" => {
            if let Err(e) = result(rt, a(1)) {
                fail(rt, b, &e);
            }
            b
        }
        "setLifecycle" | "mapError" => b,
        "build" => {
            let prefix = if rt.class_of(a(1)) == DATA_RESULT {
                match result(rt, a(1)) {
                    Ok(p) => p,
                    Err(e) => return Some(error(rt, &e)),
                }
            } else {
                a(1)
            };
            if let Some(e) = rt.str_of(rt.get_field(b, "$error")).map(str::to_owned) {
                return Some(error(rt, &e));
            }
            let entries = rt.get_field(b, "$entries");
            let m = compound(rt, entries).unwrap_or_default();
            let pairs = entry_tags(rt, &m);
            merge_to_map(rt, prefix, pairs)
        }
        _ => return None,
    })
}

/// A `RecordBuilder` this module made, filled from a map codec's fields (`MapCodec.encode(value,
/// ops, builder)` on one of the runtime's codecs).
pub(crate) fn builder_add_all(rt: &mut Rt, builder: V, fields: &BTreeMap<String, Nbt>) {
    if rt.class_of(builder) != MAP_BUILDER {
        return;
    }
    let entries = rt.get_field(builder, "$entries");
    let mut m = compound(rt, entries).unwrap_or_default();
    m.extend(fields.iter().map(|(k, v)| (k.clone(), v.clone())));
    let t = tag(rt, &Nbt::Compound(m));
    rt.put_field(builder, "$entries", t);
}

/// What a builder of these built (`builder.build(ops.empty())`): the compound, or the error.
pub(crate) fn built(rt: &mut Rt, builder: V) -> Result<BTreeMap<String, Nbt>, String> {
    if rt.class_of(builder) != MAP_BUILDER {
        return Err(format!("not a RecordBuilder of this runtime: {}", rt.class_of(builder)));
    }
    if let Some(e) = rt.str_of(rt.get_field(builder, "$error")) {
        return Err(e.to_owned());
    }
    let entries = rt.get_field(builder, "$entries");
    Ok(compound(rt, entries).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    fn c(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let k = rt.static_key(key);
        api::call(rt, k, args)
    }

    const GET_STRING: &str = "com/mojang/serialization/DynamicOps.getStringValue.(Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;";
    const CREATE_STRING: &str = "com/mojang/serialization/DynamicOps.createString.(Ljava/lang/String;)Ljava/lang/Object;";
    const OR_THROW: &str = "com/mojang/serialization/DataResult.getOrThrow.()Ljava/lang/Object;";
    const SUCCESS: &str = "com/mojang/serialization/DataResult.success.(Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;";

    /// A mod's own codec class as translated code runs it: `test/Upper` reads a string through the
    /// ops it is given and answers it in capitals; it writes the value in small letters.
    fn upper(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) != "test/Upper" {
            return api::call(rt, key, args);
        }
        match short {
            "decode.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;" => {
                let (ops, input) = (args[1], args[2]);
                let r = c(rt, GET_STRING, &[ops, input]);
                let s = c(rt, OR_THROW, &[r]);
                let up = rt.str_of(s).unwrap_or("").to_uppercase();
                let up = rt.string(&up);
                let p = c(rt, "com/mojang/datafixers/util/Pair.of.(Ljava/lang/Object;Ljava/lang/Object;)Lcom/mojang/datafixers/util/Pair;", &[up, input]);
                c(rt, SUCCESS, &[p])
            }
            "encode.(Ljava/lang/Object;Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;" => {
                let low = rt.str_of(args[1]).unwrap_or("").to_lowercase();
                let low = rt.string(&low);
                let t = c(rt, CREATE_STRING, &[args[2], low]);
                c(rt, SUCCESS, &[t])
            }
            _ => api::call(rt, key, args),
        }
    }

    /// A codec class of the mod's own gets DFU's combinators (`listOf`, `fieldOf`), and what they
    /// read and write runs its own `decode` and `encode` through `NbtOps`.
    #[test]
    fn a_mods_own_codec_reads_and_writes_through_the_combinators() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(upper);
        rt.supers.insert("test/Upper", "java/lang/Object");
        rt.interfaces.insert("test/Upper", &["com/mojang/serialization/Codec"]);
        let own = rt.new_object("test/Upper");
        let list = c(&mut rt, "com/mojang/serialization/Codec.listOf.()Lcom/mojang/serialization/Codec;", &[own]);
        let read = crate::codecs::decode(&mut rt, list, &serde_json::json!(["ab", "c"])).unwrap();
        let names: Vec<String> = crate::jutil::elements(&rt, read).iter().map(|&v| rt.str_of(v).unwrap_or("").to_owned()).collect();
        assert_eq!(names, ["AB", "C"]);
        assert_eq!(crate::codecs::encode(&mut rt, list, read), Ok(Nbt::List(vec![Nbt::String("ab".into()), Nbt::String("c".into())])));
        assert!(crate::codecs::decode(&mut rt, list, &serde_json::json!([3])).is_err(), "not a string: its getOrThrow throws");

        let name = rt.string("name");
        let field = c(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[own, name]);
        let as_codec = c(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[field]);
        let x = crate::codecs::decode(&mut rt, as_codec, &serde_json::json!({"name": "xy"})).unwrap();
        assert_eq!(rt.str_of(x), Some("XY"));
        assert_eq!(crate::codecs::encode(&mut rt, as_codec, x), Ok(Nbt::Compound(BTreeMap::from([("name".to_owned(), Nbt::String("xy".into()))]))));
        rt.thrown = None;
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's `PrimitiveCodec` (Curios' attribute codec): `test/Shout` has only `read` and
    /// `write`; DFU's `decode` and `encode` run them.
    fn shout(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) != "test/Shout" {
            return api::call(rt, key, args);
        }
        match short {
            "read.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;" => {
                let r = c(rt, GET_STRING, &[args[1], args[2]]);
                let s = c(rt, OR_THROW, &[r]);
                let up = rt.str_of(s).unwrap_or("").to_uppercase();
                let up = rt.string(&up);
                c(rt, SUCCESS, &[up])
            }
            "write.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Ljava/lang/Object;" => {
                let low = rt.str_of(args[2]).unwrap_or("").to_lowercase();
                let low = rt.string(&low);
                c(rt, CREATE_STRING, &[args[1], low])
            }
            _ => api::call(rt, key, args),
        }
    }

    #[test]
    fn a_mods_primitive_codec_reads_and_writes_through_read_and_write() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(shout);
        rt.supers.insert("test/Shout", "java/lang/Object");
        rt.interfaces.insert("test/Shout", &["com/mojang/serialization/codecs/PrimitiveCodec"]);
        let own = rt.new_object("test/Shout");
        let name = rt.string("name");
        let field = c(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[own, name]);
        let as_codec = c(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[field]);
        let x = crate::codecs::decode(&mut rt, as_codec, &serde_json::json!({"name": "xy"})).unwrap();
        assert_eq!(rt.str_of(x), Some("XY"));
        assert_eq!(crate::codecs::encode(&mut rt, as_codec, x), Ok(Nbt::Compound(BTreeMap::from([("name".to_owned(), Nbt::String("xy".into()))]))));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `NbtOps` as a mod's codec uses it: builders, maps, lists, numbers, as Minecraft's makes them
    /// (ints alone are an int array; `getList` hands each element to a consumer).
    #[test]
    fn nbt_ops_build_and_read_tags_as_minecrafts() {
        let mut rt = Rt::new();
        crate::install();
        let ops = nbt_ops(&mut rt);
        let empty = c(&mut rt, "com/mojang/serialization/DynamicOps.empty.()Ljava/lang/Object;", &[ops]);
        let one = c(&mut rt, "com/mojang/serialization/DynamicOps.createInt.(I)Ljava/lang/Object;", &[ops, V::I(1)]);
        let two = c(&mut rt, "com/mojang/serialization/DynamicOps.createInt.(I)Ljava/lang/Object;", &[ops, V::I(2)]);
        let lb = c(&mut rt, "com/mojang/serialization/DynamicOps.listBuilder.()Lcom/mojang/serialization/ListBuilder;", &[ops]);
        c(&mut rt, "com/mojang/serialization/ListBuilder.add.(Ljava/lang/Object;)Lcom/mojang/serialization/ListBuilder;", &[lb, one]);
        let ok_two = c(&mut rt, SUCCESS, &[two]);
        c(&mut rt, "com/mojang/serialization/ListBuilder.add.(Lcom/mojang/serialization/DataResult;)Lcom/mojang/serialization/ListBuilder;", &[lb, ok_two]);
        let built = c(&mut rt, "com/mojang/serialization/ListBuilder.build.(Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;", &[lb, empty]);
        let ints = c(&mut rt, OR_THROW, &[built]);
        assert_eq!(nbt::element_from_runtime(&rt, ints), Some(Nbt::IntArray(vec![1, 2])));

        let mb = c(&mut rt, "com/mojang/serialization/DynamicOps.mapBuilder.()Lcom/mojang/serialization/RecordBuilder;", &[ops]);
        let a = rt.string("a");
        c(&mut rt, "com/mojang/serialization/RecordBuilder.add.(Ljava/lang/String;Ljava/lang/Object;)Lcom/mojang/serialization/RecordBuilder;", &[mb, a, ints]);
        let x = rt.string("x");
        let xt = c(&mut rt, CREATE_STRING, &[ops, x]);
        let (b, xr) = (rt.string("b"), c(&mut rt, SUCCESS, &[xt]));
        c(&mut rt, "com/mojang/serialization/RecordBuilder.add.(Ljava/lang/String;Lcom/mojang/serialization/DataResult;)Lcom/mojang/serialization/RecordBuilder;", &[mb, b, xr]);
        let r = c(&mut rt, "com/mojang/serialization/RecordBuilder.build.(Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;", &[mb, empty]);
        let map = c(&mut rt, OR_THROW, &[r]);
        let expected = BTreeMap::from([("a".to_owned(), Nbt::IntArray(vec![1, 2])), ("b".to_owned(), Nbt::String("x".into()))]);
        assert_eq!(nbt::element_from_runtime(&rt, map), Some(Nbt::Compound(expected)));

        let ml = c(&mut rt, "com/mojang/serialization/DynamicOps.getMap.(Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;", &[ops, map]);
        let ml = c(&mut rt, OR_THROW, &[ml]);
        let got = c(&mut rt, "com/mojang/serialization/MapLike.get.(Ljava/lang/String;)Ljava/lang/Object;", &[ml, b]);
        let s = c(&mut rt, GET_STRING, &[ops, got]);
        let s = c(&mut rt, OR_THROW, &[s]);
        assert_eq!(rt.str_of(s), Some("x"));
        let none = rt.string("none");
        assert_eq!(c(&mut rt, "com/mojang/serialization/MapLike.get.(Ljava/lang/String;)Ljava/lang/Object;", &[ml, none]), V::NULL);

        // `getList(ints)`: a consumer of consumers, each element handed on.
        fn collect(rt: &mut Rt, key: &'static str, _: &'static str, args: &[V]) -> V {
            if rt.class_of(args[0]) == "test/Collect" {
                let into = rt.get_field(args[0], "into");
                crate::jutil::list_add(rt, into, args[1]);
                return V::Void;
            }
            api::call(rt, key, args)
        }
        rt.vcall = Some(collect);
        let feed = c(&mut rt, "com/mojang/serialization/DynamicOps.getList.(Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;", &[ops, ints]);
        let feed = c(&mut rt, OR_THROW, &[feed]);
        let sink = rt.new_object("test/Collect");
        let into = crate::jutil::new_list(&mut rt, Vec::new());
        rt.put_field(sink, "into", into);
        c(&mut rt, "java/util/function/Consumer.accept.(Ljava/lang/Object;)V", &[feed, sink]);
        let got: Vec<Option<Nbt>> = crate::jutil::elements(&rt, into).iter().map(|&t| nbt::element_from_runtime(&rt, t)).collect();
        assert_eq!(got, [Some(Nbt::Int(1)), Some(Nbt::Int(2))]);
        let not = c(&mut rt, GET_STRING, &[ops, one]);
        let is_error = c(&mut rt, "com/mojang/serialization/DataResult.isError.()Z", &[not]);
        assert_eq!(is_error, V::I(1));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
