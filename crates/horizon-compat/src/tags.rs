//! Minecraft's NBT tags in native Rust: `CompoundTag` (a map holding each value with the kind it
//! was put as), `ListTag` (a list of tag objects), the numeric and string tags, the array tags,
//! `NbtUtils`, and `java.util.UUID` as NBT stores it (four ints).
//!
//! In a compound, numbers are kept raw (`V::I`, `V::F`...) and strings as Java strings, beside a
//! hidden kind entry ([`crate::nbt::kind_key`]); compounds, lists and arrays are tag objects. A
//! numeric tag object is `Data::Native("nbt", [kind, raw bits, 0, 0])`.
use crate::nbt::{self, kind_key};
use crate::{Data, Rt, V};
use std::collections::HashMap;

pub const COMPOUND: &str = "net/minecraft/nbt/CompoundTag";
pub const LIST: &str = "net/minecraft/nbt/ListTag";
const INT_ARRAY: &str = "net/minecraft/nbt/IntArrayTag";
const LONG_ARRAY: &str = "net/minecraft/nbt/LongArrayTag";
const BYTE_ARRAY: &str = "net/minecraft/nbt/ByteArrayTag";
const STRING_TAG: &str = "net/minecraft/nbt/StringTag";
const UUID: &str = "java/util/UUID";

/// Kind ids, as `Tag.getId()` answers.
pub const K_LIST: i32 = 9;
pub const K_BYTE_ARRAY: i32 = 7;
pub const K_INT_ARRAY: i32 = 11;
pub const K_LONG_ARRAY: i32 = 12;
/// `contains(key, 99)`: any number.
const ANY_NUMERIC: i32 = 99;

fn numeric_class(kind: i32) -> &'static str {
    match kind {
        nbt::BYTE => "net/minecraft/nbt/ByteTag",
        nbt::SHORT => "net/minecraft/nbt/ShortTag",
        nbt::INT => "net/minecraft/nbt/IntTag",
        nbt::LONG => "net/minecraft/nbt/LongTag",
        nbt::FLOAT => "net/minecraft/nbt/FloatTag",
        _ => "net/minecraft/nbt/DoubleTag",
    }
}

pub fn new_compound(rt: &mut Rt) -> V {
    rt.alloc(COMPOUND, Data::Map(HashMap::new()))
}

pub fn new_list(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc(LIST, Data::List(items))
}

fn data(rt: &Rt, v: V) -> Option<&Data> {
    match v {
        V::R(r) if r != 0 => rt.heap.get(r as usize).map(|o| &o.data),
        _ => None,
    }
}

/// A tag object's kind id (`Tag.getId`).
pub fn kind_of(rt: &Rt, tag: V) -> i32 {
    match (rt.class_of(tag), data(rt, tag)) {
        (_, Some(Data::Native("nbt", h))) => h[0] as i32,
        (COMPOUND, _) => nbt::TAG,
        (LIST, _) => K_LIST,
        (INT_ARRAY, _) => K_INT_ARRAY,
        (LONG_ARRAY, _) => K_LONG_ARRAY,
        (BYTE_ARRAY, _) => K_BYTE_ARRAY,
        (_, Some(Data::Str(_))) => nbt::STRING,
        (_, Some(Data::Map(_))) => nbt::TAG,
        _ => 0,
    }
}

/// A numeric tag of `kind` holding `raw` (a compound's raw value).
fn numeric(rt: &mut Rt, kind: i32, raw: V) -> V {
    let bits = match raw {
        V::F(f) => f.to_bits() as i64,
        V::D(d) => d.to_bits() as i64,
        V::J(j) => j,
        v => v.i() as i64,
    };
    rt.alloc(numeric_class(kind), Data::Native("nbt", [kind as i64, bits, 0, 0]))
}

/// A numeric tag's value as the raw value its kind keeps.
fn numeric_raw(h: &[i64; 4]) -> V {
    match h[0] as i32 {
        nbt::LONG => V::J(h[1]),
        nbt::FLOAT => V::F(f32::from_bits(h[1] as u32)),
        nbt::DOUBLE => V::D(f64::from_bits(h[1] as u64)),
        _ => V::I(h[1] as i32),
    }
}

/// A raw compound value as a tag object (`compound.get(key)`).
fn wrap(rt: &mut Rt, kind: i32, raw: V) -> V {
    match kind {
        nbt::BYTE | nbt::SHORT | nbt::INT | nbt::LONG | nbt::FLOAT | nbt::DOUBLE => numeric(rt, kind, raw),
        nbt::STRING => {
            let s = rt.str_of(raw).unwrap_or("").to_owned();
            rt.alloc(STRING_TAG, Data::Str(s))
        }
        _ => raw,
    }
}

/// A tag object as what a compound keeps: (kind, raw value).
fn unwrap(rt: &mut Rt, tag: V) -> (i32, V) {
    match data(rt, tag) {
        Some(Data::Native("nbt", h)) => (h[0] as i32, numeric_raw(h)),
        Some(Data::Str(s)) => {
            let s = s.clone();
            (nbt::STRING, rt.string(&s))
        }
        _ => (kind_of(rt, tag), tag),
    }
}

/// A number as `getAsInt`/`getAsDouble`... read it, from a numeric tag or a raw value.
fn number(rt: &Rt, v: V) -> f64 {
    match data(rt, v) {
        Some(Data::Native("nbt", h)) => match numeric_raw(h) {
            V::J(j) => j as f64,
            V::F(f) => f as f64,
            V::D(d) => d,
            x => x.i() as f64,
        },
        _ => match v {
            V::J(j) => j as f64,
            V::F(f) => f as f64,
            V::D(d) => d,
            x => x.i() as f64,
        },
    }
}

fn long_of(rt: &Rt, v: V) -> i64 {
    match data(rt, v) {
        Some(Data::Native("nbt", h)) => match numeric_raw(h) {
            V::J(j) => j,
            V::F(f) => f as i64,
            V::D(d) => d as i64,
            x => x.i() as i64,
        },
        _ => match v {
            V::J(j) => j,
            V::F(f) => f as i64,
            V::D(d) => d as i64,
            x => x.i() as i64,
        },
    }
}

fn entries(rt: &Rt, compound: V) -> Vec<(String, V)> {
    match data(rt, compound) {
        Some(Data::Map(m)) => m.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        _ => Vec::new(),
    }
}

fn get_raw(rt: &Rt, compound: V, key: &str) -> Option<(i32, V)> {
    match data(rt, compound) {
        Some(Data::Map(m)) => m.get(key).map(|v| (m.get(&kind_key(key)).map_or(0, |k| k.i()), *v)),
        _ => None,
    }
}

fn put_raw(rt: &mut Rt, compound: V, key: &str, kind: i32, raw: V) {
    if let Some(crate::Object { data: Data::Map(m), .. }) = rt.heap.get_mut(compound.r() as usize) {
        m.insert(kind_key(key), V::I(kind));
        m.insert(key.to_owned(), raw);
    }
}

fn array_values(rt: &Rt, v: V) -> Vec<V> {
    match data(rt, v) {
        Some(Data::Array(a) | Data::List(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// An array tag of `class` with `items` (copied, so the caller's array stays its own).
fn array_tag(rt: &mut Rt, class: &'static str, items: Vec<V>) -> V {
    rt.alloc(class, Data::Array(items))
}

/// A Java primitive array (`int[]`...) of `items`.
fn java_array(rt: &mut Rt, class: &'static str, items: Vec<V>) -> V {
    rt.alloc(class, Data::Array(items))
}

/// A deep copy of a tag object (compounds, lists and arrays copied; raw values as they are).
pub fn copy(rt: &mut Rt, tag: V) -> V {
    let class = rt.class_of(tag);
    match data(rt, tag).cloned() {
        Some(Data::Map(m)) => {
            let mut out = HashMap::new();
            for (k, v) in m {
                let v = if !k.starts_with('\u{1}') && matches!(data(rt, v), Some(Data::Map(_) | Data::List(_) | Data::Array(_))) { copy(rt, v) } else { v };
                out.insert(k, v);
            }
            rt.alloc(class, Data::Map(out))
        }
        Some(Data::List(items)) => {
            let items: Vec<V> = items.into_iter().map(|v| copy(rt, v)).collect();
            rt.alloc(class, Data::List(items))
        }
        Some(Data::Array(items)) => rt.alloc(class, Data::Array(items)),
        Some(Data::Native(k, h)) if k == "nbt" => rt.alloc(class, Data::Native(k, h)),
        Some(Data::Str(s)) if class == STRING_TAG => rt.alloc(class, Data::Str(s)),
        _ => tag,
    }
}

/// `UUID` from its two longs.
pub fn uuid(rt: &mut Rt, msb: i64, lsb: i64) -> V {
    rt.alloc(UUID, Data::Native("uuid", [msb, lsb, 0, 0]))
}

fn uuid_bits(rt: &Rt, v: V) -> (i64, i64) {
    match data(rt, v) {
        Some(Data::Native("uuid", h)) => (h[0], h[1]),
        _ => (0, 0),
    }
}

/// A UUID as NBT keeps it: four ints, most significant first.
fn uuid_ints(msb: i64, lsb: i64) -> Vec<V> {
    vec![V::I((msb >> 32) as i32), V::I(msb as i32), V::I((lsb >> 32) as i32), V::I(lsb as i32)]
}

fn uuid_from_ints(ints: &[V]) -> Option<(i64, i64)> {
    let [a, b, c, d] = ints else { return None };
    let msb = ((a.i() as i64) << 32) | (b.i() as u32 as i64);
    let lsb = ((c.i() as i64) << 32) | (d.i() as u32 as i64);
    Some((msb, lsb))
}

fn uuid_text(msb: i64, lsb: i64) -> String {
    let (m, l) = (msb as u64, lsb as u64);
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", m >> 32, (m >> 16) & 0xffff, m & 0xffff, l >> 48, l & 0xffff_ffff_ffff)
}

fn uuid_parse(s: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = s.split('-').collect();
    let [a, b, c, d, e] = parts.as_slice() else { return None };
    let p = |x: &str| u64::from_str_radix(x, 16).ok();
    let msb = (p(a)? << 32) | (p(b)? << 16) | p(c)?;
    let lsb = (p(d)? << 48) | p(e)?;
    Some((msb as i64, lsb as i64))
}

fn text(rt: &Rt, v: V) -> String {
    rt.str_of(v).unwrap_or("").to_owned()
}

fn optional(rt: &mut Rt, v: V) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", v);
    o
}

/// A block state's text from `{Name, Properties}` (`NbtUtils.readBlockState`).
fn state_from(rt: &Rt, tag: V) -> String {
    let name = get_raw(rt, tag, "Name").and_then(|(_, v)| rt.str_of(v).map(str::to_owned)).unwrap_or_else(|| "minecraft:air".into());
    let props = get_raw(rt, tag, "Properties").map(|(_, v)| v);
    let mut pairs: Vec<(String, String)> = props
        .map(|p| entries(rt, p).into_iter().filter(|(k, _)| !k.starts_with('\u{1}')).map(|(k, v)| (k, rt.str_of(v).unwrap_or("").to_owned())).collect())
        .unwrap_or_default();
    pairs.sort();
    if pairs.is_empty() {
        name
    } else {
        format!("{name}[{}]", pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(","))
    }
}

/// A Minecraft NBT call (or `java.util.UUID`), or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    if !(owner.starts_with("net/minecraft/nbt/") || owner == UUID) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // Compounds
        (COMPOUND, "<init>") => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Map(HashMap::new());
            }
            V::Void
        }
        (COMPOUND, "put") => {
            let (kind, raw) = unwrap(rt, a(2));
            let k = text(rt, a(1));
            put_raw(rt, a(0), &k, kind, raw);
            V::NULL
        }
        (COMPOUND, n) if n.starts_with("put") => {
            let k = text(rt, a(1));
            let (kind, raw) = match n {
                "putByte" => (nbt::BYTE, a(2)),
                "putBoolean" => (nbt::BYTE, V::I((a(2).i() != 0) as i32)),
                "putShort" => (nbt::SHORT, a(2)),
                "putInt" => (nbt::INT, a(2)),
                "putLong" => (nbt::LONG, a(2)),
                "putFloat" => (nbt::FLOAT, a(2)),
                "putDouble" => (nbt::DOUBLE, a(2)),
                "putString" => (nbt::STRING, a(2)),
                "putIntArray" => (K_INT_ARRAY, {
                    let items = crate::jutil::elements(rt, a(2)).iter().map(|v| V::I(v.i())).collect();
                    array_tag(rt, INT_ARRAY, items)
                }),
                "putLongArray" => (K_LONG_ARRAY, {
                    let items = crate::jutil::elements(rt, a(2)).iter().map(|v| V::J(v.j())).collect();
                    array_tag(rt, LONG_ARRAY, items)
                }),
                "putByteArray" => (K_BYTE_ARRAY, {
                    let items = crate::jutil::elements(rt, a(2));
                    array_tag(rt, BYTE_ARRAY, items)
                }),
                "putUUID" => {
                    let (m, l) = uuid_bits(rt, a(2));
                    (K_INT_ARRAY, array_tag(rt, INT_ARRAY, uuid_ints(m, l)))
                }
                _ => return None,
            };
            put_raw(rt, a(0), &k, kind, raw);
            V::Void
        }
        (COMPOUND, "contains") => {
            let k = text(rt, a(1));
            match get_raw(rt, a(0), &k) {
                None => V::I(0),
                Some(_) if desc.starts_with("(Ljava/lang/String;)") => V::I(1),
                Some((kind, _)) => {
                    let want = a(2).i();
                    V::I((kind == want || (want == ANY_NUMERIC && (1..=6).contains(&kind))) as i32)
                }
            }
        }
        (COMPOUND, "getTagType") => {
            let k = text(rt, a(1));
            V::I(get_raw(rt, a(0), &k).map_or(0, |(kind, _)| kind))
        }
        (COMPOUND, "get") => {
            let k = text(rt, a(1));
            match get_raw(rt, a(0), &k) {
                Some((kind, raw)) => wrap(rt, kind, raw),
                None => V::NULL,
            }
        }
        (COMPOUND, "getCompound") => {
            let k = text(rt, a(1));
            match get_raw(rt, a(0), &k) {
                Some((nbt::TAG, v)) => v,
                _ => new_compound(rt),
            }
        }
        (COMPOUND, "getList") => {
            let k = text(rt, a(1));
            match get_raw(rt, a(0), &k) {
                Some((K_LIST, v)) => v,
                _ => new_list(rt, Vec::new()),
            }
        }
        (COMPOUND, "getIntArray" | "getLongArray" | "getByteArray") => {
            let k = text(rt, a(1));
            let (want, class) = match name {
                "getIntArray" => (K_INT_ARRAY, "[I"),
                "getLongArray" => (K_LONG_ARRAY, "[J"),
                _ => (K_BYTE_ARRAY, "[B"),
            };
            let items = match get_raw(rt, a(0), &k) {
                Some((kind, v)) if kind == want => array_values(rt, v),
                _ => Vec::new(),
            };
            java_array(rt, class, items)
        }
        (COMPOUND, "hasUUID") => {
            let k = text(rt, a(1));
            V::I(matches!(get_raw(rt, a(0), &k), Some((K_INT_ARRAY, v)) if array_values(rt, v).len() == 4) as i32)
        }
        (COMPOUND, "getUUID") => {
            let k = text(rt, a(1));
            let ints = match get_raw(rt, a(0), &k) {
                Some((K_INT_ARRAY, v)) => array_values(rt, v),
                _ => Vec::new(),
            };
            let (m, l) = uuid_from_ints(&ints).unwrap_or((0, 0));
            uuid(rt, m, l)
        }
        (COMPOUND, n) if n.starts_with("get") && desc.starts_with("(Ljava/lang/String;)") => {
            let k = text(rt, a(1));
            let found = get_raw(rt, a(0), &k);
            match n {
                "getString" => match found {
                    Some((nbt::STRING, v)) => v,
                    _ => rt.string(""),
                },
                "getBoolean" => V::I(found.is_some_and(|(_, v)| number(rt, v) != 0.0) as i32),
                "getByte" | "getShort" | "getInt" => V::I(found.map_or(0, |(_, v)| long_of(rt, v) as i32)),
                "getLong" => V::J(found.map_or(0, |(_, v)| long_of(rt, v))),
                "getFloat" => V::F(found.map_or(0.0, |(_, v)| number(rt, v) as f32)),
                "getDouble" => V::D(found.map_or(0.0, |(_, v)| number(rt, v))),
                _ => return None,
            }
        }
        (COMPOUND, "remove") => {
            let k = text(rt, a(1));
            if let Some(crate::Object { data: Data::Map(m), .. }) = rt.heap.get_mut(a(0).r() as usize) {
                m.remove(&kind_key(&k));
                m.remove(&k);
            }
            V::Void
        }
        (COMPOUND, "isEmpty" | "size" | "getAllKeys" | "keySet") => {
            let mut keys: Vec<String> = entries(rt, a(0)).into_iter().map(|(k, _)| k).filter(|k| !k.starts_with('\u{1}')).collect();
            match name {
                "isEmpty" => V::I(keys.is_empty() as i32),
                "size" => V::I(keys.len() as i32),
                _ => {
                    keys.sort();
                    let items: Vec<V> = keys.iter().map(|k| rt.string(k)).collect();
                    crate::jutil::new_set_of(rt, &items)
                }
            }
        }
        (COMPOUND, "copy") => copy(rt, a(0)),
        (COMPOUND, "merge") => {
            merge(rt, a(0), a(1));
            a(0)
        }
        (COMPOUND, "equals") => V::I(crate::jutil::equals_own(rt, a(0), a(1)) as i32),
        // Lists of tags
        (LIST, "<init>") => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::List(Vec::new());
            }
            V::Void
        }
        (LIST, n) if matches!(n, "getCompound" | "getString" | "getInt" | "getShort" | "getFloat" | "getDouble" | "getList" | "getIntArray" | "getLongArray") => {
            let items = array_values(rt, a(0));
            let e = items.get(a(1).i().max(0) as usize).copied();
            match n {
                "getCompound" => match e {
                    Some(v) if rt.class_of(v) == COMPOUND => v,
                    _ => new_compound(rt),
                },
                "getList" => match e {
                    Some(v) if rt.class_of(v) == LIST => v,
                    _ => new_list(rt, Vec::new()),
                },
                "getString" => {
                    let s = e.map(|v| tag_string(rt, v)).unwrap_or_default();
                    rt.string(&s)
                }
                "getInt" | "getShort" => V::I(e.map_or(0, |v| long_of(rt, v) as i32)),
                "getFloat" => V::F(e.map_or(0.0, |v| number(rt, v) as f32)),
                "getDouble" => V::D(e.map_or(0.0, |v| number(rt, v))),
                _ => {
                    let (class, items) = if n == "getIntArray" { ("[I", e.map(|v| array_values(rt, v))) } else { ("[J", e.map(|v| array_values(rt, v))) };
                    java_array(rt, class, items.unwrap_or_default())
                }
            }
        }
        (LIST, "copy") => copy(rt, a(0)),
        (LIST, "getElementType" | "identifyRawElementType") => {
            let first = array_values(rt, a(0)).first().copied();
            V::I(first.map_or(0, |v| kind_of(rt, v)))
        }
        (LIST, "getId") => V::I(K_LIST),
        // Everything else a list does is `java.util.List`'s.
        (LIST, _) => {
            let k = rt.static_key(&format!("java/util/ArrayList.{name}.{desc}"));
            return crate::jutil::call(rt, k, args);
        }
        // Numbers and strings
        (o, "valueOf") if o.starts_with("net/minecraft/nbt/") && o != STRING_TAG => {
            let kind = match o {
                "net/minecraft/nbt/ByteTag" => nbt::BYTE,
                "net/minecraft/nbt/ShortTag" => nbt::SHORT,
                "net/minecraft/nbt/IntTag" => nbt::INT,
                "net/minecraft/nbt/LongTag" => nbt::LONG,
                "net/minecraft/nbt/FloatTag" => nbt::FLOAT,
                "net/minecraft/nbt/DoubleTag" => nbt::DOUBLE,
                _ => return None,
            };
            let raw = if desc.starts_with("(Z)") { V::I((a(0).i() != 0) as i32) } else { a(0) };
            numeric(rt, kind, raw)
        }
        (STRING_TAG, "valueOf") => {
            let s = text(rt, a(0));
            rt.alloc(STRING_TAG, Data::Str(s))
        }
        (_, "getAsString") => {
            let s = tag_string(rt, a(0));
            rt.string(&s)
        }
        (_, "getAsInt" | "getAsShort" | "getAsByte") => V::I(long_of(rt, a(0)) as i32),
        (_, "getAsLong") => V::J(long_of(rt, a(0))),
        (_, "getAsFloat") => V::F(number(rt, a(0)) as f32),
        (_, "getAsDouble") => V::D(number(rt, a(0))),
        (_, "getAsIntArray" | "getAsLongArray" | "getAsByteArray") => {
            let class = match name {
                "getAsIntArray" => "[I",
                "getAsLongArray" => "[J",
                _ => "[B",
            };
            let items = array_values(rt, a(0));
            java_array(rt, class, items)
        }
        (INT_ARRAY | LONG_ARRAY | BYTE_ARRAY, "<init>") => {
            let items = crate::jutil::elements(rt, a(1));
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Array(items);
            }
            V::Void
        }
        (INT_ARRAY | LONG_ARRAY | BYTE_ARRAY, "size") => V::I(array_values(rt, a(0)).len() as i32),
        (_, "getId") => V::I(kind_of(rt, a(0))),
        (_, "copy") => copy(rt, a(0)),
        // NbtUtils
        ("net/minecraft/nbt/NbtUtils", "writeBlockPos") => {
            let h = match data(rt, a(0)) {
                Some(Data::Native(_, h)) => *h,
                _ => [0; 4],
            };
            array_tag(rt, INT_ARRAY, vec![V::I(h[0] as i32), V::I(h[1] as i32), V::I(h[2] as i32)])
        }
        ("net/minecraft/nbt/NbtUtils", "readBlockPos") => {
            let k = text(rt, a(1));
            let pos = match get_raw(rt, a(0), &k) {
                Some((K_INT_ARRAY, v)) => {
                    let ints = array_values(rt, v);
                    match ints.as_slice() {
                        [x, y, z] => crate::api::block_pos(rt, x.i() as i64, y.i() as i64, z.i() as i64),
                        _ => V::NULL,
                    }
                }
                _ => V::NULL,
            };
            optional(rt, pos)
        }
        ("net/minecraft/nbt/NbtUtils", "createUUID") => {
            let (m, l) = uuid_bits(rt, a(0));
            array_tag(rt, INT_ARRAY, uuid_ints(m, l))
        }
        ("net/minecraft/nbt/NbtUtils", "loadUUID") => {
            let ints = array_values(rt, a(0));
            let (m, l) = uuid_from_ints(&ints).unwrap_or((0, 0));
            uuid(rt, m, l)
        }
        ("net/minecraft/nbt/NbtUtils", "writeBlockState") => {
            let s = text(rt, a(0));
            let (block, props) = crate::host::split_state(&s);
            let tag = new_compound(rt);
            let b = rt.string(block);
            put_raw(rt, tag, "Name", nbt::STRING, b);
            if !props.is_empty() {
                let p = new_compound(rt);
                for (k, v) in props {
                    let v = rt.string(&v);
                    put_raw(rt, p, &k, nbt::STRING, v);
                }
                put_raw(rt, tag, "Properties", nbt::TAG, p);
            }
            tag
        }
        ("net/minecraft/nbt/NbtUtils", "readBlockState") => {
            let s = state_from(rt, a(1));
            crate::api::state(rt, &s)
        }
        // UUID
        (UUID, "<init>") => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Native("uuid", [a(1).j(), a(2).j(), 0, 0]);
            }
            V::Void
        }
        (UUID, "randomUUID") => {
            let mut next = || {
                rt.random_state = rt.random_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                rt.random_state as i64
            };
            let (m, l) = (next(), next());
            // Version 4, IETF variant.
            uuid(rt, (m & !0xf000) | 0x4000, (l & 0x3fff_ffff_ffff_ffff) | i64::MIN)
        }
        (UUID, "fromString") => {
            let s = text(rt, a(0));
            match uuid_parse(&s) {
                Some((m, l)) => uuid(rt, m, l),
                None => {
                    let e = rt.new_object("java/lang/IllegalArgumentException");
                    let m = rt.string(&format!("Invalid UUID string: {s}"));
                    rt.put_field(e, "message", m);
                    rt.thrown = Some(e);
                    V::NULL
                }
            }
        }
        (UUID, "getMostSignificantBits") => V::J(uuid_bits(rt, a(0)).0),
        (UUID, "getLeastSignificantBits") => V::J(uuid_bits(rt, a(0)).1),
        (UUID, "toString") => {
            let (m, l) = uuid_bits(rt, a(0));
            rt.string(&uuid_text(m, l))
        }
        (UUID, "hashCode") => {
            let (m, l) = uuid_bits(rt, a(0));
            let x = m ^ l;
            V::I(((x >> 32) as i32) ^ (x as i32))
        }
        (UUID, "equals") => V::I((matches!(data(rt, a(1)), Some(Data::Native("uuid", _))) && uuid_bits(rt, a(0)) == uuid_bits(rt, a(1))) as i32),
        (UUID, "compareTo") => {
            let (p, q) = (uuid_bits(rt, a(0)), uuid_bits(rt, a(1)));
            V::I(p.cmp(&q) as i32)
        }
        _ => return None,
    })
}

/// A tag's text (`getAsString`): a string's own text, a number's digits.
fn tag_string(rt: &Rt, v: V) -> String {
    match data(rt, v) {
        Some(Data::Str(s)) => s.clone(),
        Some(Data::Native("nbt", h)) => match numeric_raw(h) {
            V::F(f) => rt.to_java_string(V::F(f), 'F'),
            V::D(d) => rt.to_java_string(V::D(d), 'D'),
            V::J(j) => j.to_string(),
            x => x.i().to_string(),
        },
        _ => String::new(),
    }
}

/// `into.merge(from)`: compounds merge recursively, everything else is copied over.
fn merge(rt: &mut Rt, into: V, from: V) {
    for (k, v) in entries(rt, from) {
        if k.starts_with('\u{1}') {
            continue;
        }
        let kind = get_raw(rt, from, &k).map_or(0, |(kind, _)| kind);
        match (kind, get_raw(rt, into, &k)) {
            (nbt::TAG, Some((nbt::TAG, existing))) => merge(rt, existing, v),
            _ => {
                let v = if matches!(data(rt, v), Some(Data::Map(_) | Data::List(_) | Data::Array(_))) { copy(rt, v) } else { v };
                put_raw(rt, into, &k, kind, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        super::call(rt, key, args).unwrap_or_else(|| panic!("unmapped {key}"))
    }

    #[test]
    fn positions_and_uuids_keep_the_layout_minecraft_saves() {
        let mut rt = Rt::new();
        // NbtUtils.writeBlockPos is an int array [x, y, z]; readBlockPos reads it back.
        let pos = crate::api::block_pos(&mut rt, 3, -64, 70000);
        let tag = call(&mut rt, "net/minecraft/nbt/NbtUtils.writeBlockPos.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/nbt/Tag;", &[pos]);
        let c = new_compound(&mut rt);
        let k = rt.string("Source");
        call(&mut rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[c, k, tag]);
        let saved = nbt::from_runtime(&rt, c);
        assert_eq!(saved.get("Source"), Some(&nbt::Nbt::IntArray(vec![3, -64, 70000])));
        let back = nbt::to_runtime(&mut rt, &saved);
        let read = call(&mut rt, "net/minecraft/nbt/NbtUtils.readBlockPos.(Lnet/minecraft/nbt/CompoundTag;Ljava/lang/String;)Ljava/util/Optional;", &[back, k]);
        let p = rt.get_field(read, "value");
        assert!(matches!(rt.heap[p.r() as usize].data, Data::Native(_, [3, -64, 70000, 0])));
        // A UUID is four ints, most significant first, and prints as Java prints it.
        let id = uuid(&mut rt, 0x1234_5678_9abc_def0, -2);
        let ku = rt.string("Owner");
        call(&mut rt, "net/minecraft/nbt/CompoundTag.putUUID.(Ljava/lang/String;Ljava/util/UUID;)V", &[c, ku, id]);
        let saved = nbt::from_runtime(&rt, c);
        assert_eq!(saved.get("Owner"), Some(&nbt::Nbt::IntArray(vec![0x1234_5678, 0x9abc_def0u32 as i32, -1, -2])));
        let text = call(&mut rt, "java/util/UUID.toString.()Ljava/lang/String;", &[id]);
        assert_eq!(rt.str_of(text), Some("12345678-9abc-def0-ffff-fffffffffffe"));
    }

    #[test]
    fn lists_of_compounds_survive_a_save() {
        let mut rt = Rt::new();
        let list = new_list(&mut rt, Vec::new());
        for i in 0..2 {
            let e = new_compound(&mut rt);
            let k = rt.string("Slot");
            call(&mut rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[e, k, V::I(i)]);
            if let Data::List(items) = &mut rt.heap[list.r() as usize].data {
                items.push(e);
            }
        }
        let c = new_compound(&mut rt);
        let k = rt.string("Items");
        call(&mut rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[c, k, list]);
        let saved = nbt::from_runtime(&rt, c);
        let back = nbt::to_runtime(&mut rt, &saved);
        let l = call(&mut rt, "net/minecraft/nbt/CompoundTag.getList.(Ljava/lang/String;I)Lnet/minecraft/nbt/ListTag;", &[back, k, V::I(10)]);
        let second = call(&mut rt, "net/minecraft/nbt/ListTag.getCompound.(I)Lnet/minecraft/nbt/CompoundTag;", &[l, V::I(1)]);
        let ks = rt.string("Slot");
        assert_eq!(call(&mut rt, "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I", &[second, ks]), V::I(1));
    }
}
