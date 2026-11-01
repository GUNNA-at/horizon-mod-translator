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
            // Lists and arrays stay out of the runtime for now: kept as they were by the glue.
            _ => continue,
        };
        map.insert(k.clone(), value);
        map.insert(kind_key(k), V::I(kind));
    }
    rt.alloc("net/minecraft/nbt/CompoundTag", Data::Map(map))
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
