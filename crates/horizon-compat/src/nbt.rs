//! NBT between the runtime and the game: [`Nbt`] is the neutral form the glue
//! hands over (the game converts it to and from its own compounds), and the
//! runtime's `CompoundTag`s are maps of values with the kind each was put as.
use crate::{Data, Rt, V};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    List(Vec<Nbt>),
    Compound(BTreeMap<String, Nbt>),
    ByteArray(Vec<i8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

/// The hidden entry beside `key` that says what it was put as.
pub fn kind_key(key: &str) -> String {
    format!("\u{1}{key}")
}

/// Kinds, as `put*` records them.
pub const BYTE: i32 = 1;
pub const SHORT: i32 = 2;
pub const INT: i32 = 3;
pub const LONG: i32 = 4;
pub const FLOAT: i32 = 5;
pub const DOUBLE: i32 = 6;
pub const STRING: i32 = 8;
pub const TAG: i32 = 10;

/// A runtime `CompoundTag` from game NBT.
pub fn to_runtime(rt: &mut Rt, tag: &BTreeMap<String, Nbt>) -> V {
    let mut map = std::collections::HashMap::new();
    for (k, v) in tag {
        let (value, kind) = match v {
            Nbt::Byte(b) => (V::I(*b as i32), BYTE),
            Nbt::Short(s) => (V::I(*s as i32), SHORT),
            Nbt::Int(i) => (V::I(*i), INT),
            Nbt::Long(l) => (V::J(*l), LONG),
            Nbt::Float(f) => (V::F(*f), FLOAT),
            Nbt::Double(d) => (V::D(*d), DOUBLE),
            Nbt::String(s) => (rt.string(s), STRING),
            Nbt::Compound(c) => (to_runtime(rt, c), TAG),
            other => match element_to_runtime(rt, other) {
                Some(v) => (v, crate::tags::kind_of(rt, v)),
                None => continue,
            },
        };
        map.insert(k.clone(), value);
        map.insert(kind_key(k), V::I(kind));
    }
    rt.alloc("net/minecraft/nbt/CompoundTag", Data::Map(map))
}

/// A tag object for a list element or an array (lists hold tag objects, as Java's `ListTag` does).
pub(crate) fn element_to_runtime(rt: &mut Rt, n: &Nbt) -> Option<V> {
    let numeric = |rt: &mut Rt, kind: i32, class: &'static str, bits: i64| rt.alloc(class, Data::Native("nbt", [kind as i64, bits, 0, 0]));
    Some(match n {
        Nbt::Byte(b) => numeric(rt, BYTE, "net/minecraft/nbt/ByteTag", *b as i64),
        Nbt::Short(x) => numeric(rt, SHORT, "net/minecraft/nbt/ShortTag", *x as i64),
        Nbt::Int(i) => numeric(rt, INT, "net/minecraft/nbt/IntTag", *i as i64),
        Nbt::Long(l) => numeric(rt, LONG, "net/minecraft/nbt/LongTag", *l),
        Nbt::Float(f) => numeric(rt, FLOAT, "net/minecraft/nbt/FloatTag", f.to_bits() as i64),
        Nbt::Double(d) => numeric(rt, DOUBLE, "net/minecraft/nbt/DoubleTag", d.to_bits() as i64),
        Nbt::String(s) => rt.alloc("net/minecraft/nbt/StringTag", Data::Str(s.clone())),
        Nbt::Compound(c) => to_runtime(rt, c),
        Nbt::List(items) => {
            let items: Vec<V> = items.iter().filter_map(|i| element_to_runtime(rt, i)).collect();
            crate::tags::new_list(rt, items)
        }
        Nbt::IntArray(a) => rt.alloc("net/minecraft/nbt/IntArrayTag", Data::Array(a.iter().map(|&i| V::I(i)).collect())),
        Nbt::LongArray(a) => rt.alloc("net/minecraft/nbt/LongArrayTag", Data::Array(a.iter().map(|&i| V::J(i)).collect())),
        Nbt::ByteArray(a) => rt.alloc("net/minecraft/nbt/ByteArrayTag", Data::Array(a.iter().map(|&i| V::I(i as i32)).collect())),
    })
}

/// Game NBT from a tag object (a list element, an array, a compound).
pub(crate) fn element_from_runtime(rt: &Rt, v: V) -> Option<Nbt> {
    let o = rt.heap.get(v.r() as usize)?;
    Some(match (&o.data, o.class) {
        (Data::Map(_), _) => Nbt::Compound(from_runtime(rt, v)),
        (Data::List(items), _) => Nbt::List(items.iter().filter_map(|&i| element_from_runtime(rt, i)).collect()),
        (Data::Array(a), "net/minecraft/nbt/LongArrayTag") => Nbt::LongArray(a.iter().map(|x| x.j()).collect()),
        (Data::Array(a), "net/minecraft/nbt/ByteArrayTag") => Nbt::ByteArray(a.iter().map(|x| x.i() as i8).collect()),
        (Data::Array(a), _) => Nbt::IntArray(a.iter().map(|x| x.i()).collect()),
        (Data::Native("nbt", h), _) => match h[0] as i32 {
            BYTE => Nbt::Byte(h[1] as i8),
            SHORT => Nbt::Short(h[1] as i16),
            INT => Nbt::Int(h[1] as i32),
            LONG => Nbt::Long(h[1]),
            FLOAT => Nbt::Float(f32::from_bits(h[1] as u32)),
            _ => Nbt::Double(f64::from_bits(h[1] as u64)),
        },
        (Data::Str(s), _) => Nbt::String(s.clone()),
        _ => return None,
    })
}

/// Game NBT from a runtime `CompoundTag`.
pub fn from_runtime(rt: &Rt, v: V) -> BTreeMap<String, Nbt> {
    let mut out = BTreeMap::new();
    let Some(Data::Map(map)) = rt.heap.get(v.r() as usize).map(|o| &o.data) else {
        return out;
    };
    for (k, value) in map.iter().filter(|(k, _)| !k.starts_with('\u{1}')) {
        let kind = map.get(&kind_key(k)).map_or(0, |k| k.i());
        let nbt = match (kind, value) {
            (BYTE, v) => Nbt::Byte(v.i() as i8),
            (SHORT, v) => Nbt::Short(v.i() as i16),
            (LONG, v) => Nbt::Long(v.j()),
            (FLOAT, v) => Nbt::Float(v.f()),
            (DOUBLE, v) => Nbt::Double(v.d()),
            (STRING, v) => Nbt::String(rt.str_of(*v).unwrap_or("").to_owned()),
            (TAG, v) if matches!(rt.heap.get(v.r() as usize).map(|o| &o.data), Some(Data::Map(_))) => {
                Nbt::Compound(from_runtime(rt, *v))
            }
            (crate::tags::K_LIST | crate::tags::K_INT_ARRAY | crate::tags::K_LONG_ARRAY | crate::tags::K_BYTE_ARRAY, v) => match element_from_runtime(rt, *v) {
                Some(n) => n,
                None => continue,
            },
            (_, V::I(i)) => Nbt::Int(*i),
            (_, V::J(l)) => Nbt::Long(*l),
            (_, V::F(f)) => Nbt::Float(*f),
            (_, V::D(d)) => Nbt::Double(*d),
            (_, v) => match rt.str_of(*v) {
                Some(s) => Nbt::String(s.to_owned()),
                None => continue,
            },
        };
        out.insert(k.clone(), nbt);
    }
    out
}

/// A hash of a compound's contents (keys, kinds and values), to tell when data changed.
pub fn hash(tag: &BTreeMap<String, Nbt>) -> u64 {
    use std::hash::{Hash, Hasher};
    fn walk(n: &Nbt, h: &mut std::collections::hash_map::DefaultHasher) {
        std::mem::discriminant(n).hash(h);
        match n {
            Nbt::Byte(v) => v.hash(h),
            Nbt::Short(v) => v.hash(h),
            Nbt::Int(v) => v.hash(h),
            Nbt::Long(v) => v.hash(h),
            Nbt::Float(v) => v.to_bits().hash(h),
            Nbt::Double(v) => v.to_bits().hash(h),
            Nbt::String(v) => v.hash(h),
            Nbt::List(l) => {
                l.len().hash(h);
                l.iter().for_each(|e| walk(e, h));
            }
            Nbt::Compound(c) => {
                c.len().hash(h);
                for (k, v) in c {
                    k.hash(h);
                    walk(v, h);
                }
            }
            Nbt::ByteArray(a) => a.hash(h),
            Nbt::IntArray(a) => a.hash(h),
            Nbt::LongArray(a) => a.hash(h),
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tag.len().hash(&mut h);
    for (k, v) in tag {
        k.hash(&mut h);
        walk(v, &mut h);
    }
    h.finish()
}
