//! NBT between the runtime and the game: [`Nbt`] is the neutral form the glue
//! hands over (the game converts it to and from its own compounds), and the
//! runtime's `CompoundTag`s are maps of values with the kind each was put as.
use crate::{Data, Rt, V};
use std::collections::BTreeMap;

pub use crate::host::Nbt;

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

/// `NbtUtils.compareNbt(expected, actual, partial_lists)`: a compound in `expected` matches one in
/// `actual` that has each of its keys, each matching; with `partial_lists` each element of a list
/// matches one of `actual`'s (an empty list only an empty one); anything else is equal (of the
/// same kind).
pub(crate) fn matches(expected: &Nbt, actual: &Nbt, partial_lists: bool) -> bool {
    match (expected, actual) {
        (Nbt::Compound(e), Nbt::Compound(a)) => a.len() >= e.len() && e.iter().all(|(k, v)| a.get(k).is_some_and(|x| matches(v, x, partial_lists))),
        (Nbt::List(e), Nbt::List(a)) if partial_lists => {
            if e.is_empty() {
                return a.is_empty();
            }
            a.len() >= e.len() && e.iter().all(|v| a.iter().any(|x| matches(v, x, partial_lists)))
        }
        _ => expected == actual,
    }
}

/// A hash of a compound's contents (keys, kinds and values), to tell when data changed.
pub fn hash(tag: &BTreeMap<String, Nbt>) -> u64 {
    hash_entries(tag.iter())
}

/// [`hash`] of what a block entity saves itself: without the position and id the game keeps beside
/// it (vanilla's `saveWithFullMetadata`).
pub fn data_hash(tag: &BTreeMap<String, Nbt>) -> u64 {
    hash_entries(tag.iter().filter(|(k, _)| !matches!(k.as_str(), "x" | "y" | "z" | "id")))
}

fn hash_entries<'a>(entries: impl Iterator<Item = (&'a String, &'a Nbt)> + Clone) -> u64 {
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
    entries.clone().count().hash(&mut h);
    for (k, v) in entries {
        k.hash(&mut h);
        walk(v, &mut h);
    }
    h.finish()
}

/// A tag object (a `CompoundTag`, or any tag) as NBT; None for null.
pub(crate) fn of_tag(rt: &Rt, tag: V) -> Option<Nbt> {
    if tag.r() == 0 {
        return None;
    }
    match rt.heap.get(tag.r() as usize).map(|o| &o.data) {
        Some(Data::Map(_)) => Some(Nbt::Compound(from_runtime(rt, tag))),
        _ => element_from_runtime(rt, tag),
    }
}

/// NBT as a tag object: a `CompoundTag` for a compound.
pub(crate) fn tag_of(rt: &mut Rt, n: &Nbt) -> V {
    match n {
        Nbt::Compound(c) => to_runtime(rt, c),
        other => element_to_runtime(rt, other).unwrap_or(V::NULL),
    }
}

// Binary NBT, as `Tag.write` writes it and `NbtIo` reads it back: big-endian numbers, strings as
// `DataOutput.writeUTF` writes them (Java's modified UTF-8 after a u16 length), a list's element
// type and count before its elements, a compound's entries (type, name, payload) up to `TAG_End`.

/// The type id a tag is written with (`Tag.getId`).
pub(crate) fn id(n: &Nbt) -> u8 {
    match n {
        Nbt::Byte(_) => 1,
        Nbt::Short(_) => 2,
        Nbt::Int(_) => 3,
        Nbt::Long(_) => 4,
        Nbt::Float(_) => 5,
        Nbt::Double(_) => 6,
        Nbt::ByteArray(_) => 7,
        Nbt::String(_) => 8,
        Nbt::List(_) => 9,
        Nbt::Compound(_) => 10,
        Nbt::IntArray(_) => 11,
        Nbt::LongArray(_) => 12,
    }
}

/// `s` in Java's modified UTF-8 (`DataOutput.writeUTF`'s bytes, without the length): U+0000 as two
/// bytes, a character past the basic plane as its two UTF-16 surrogates of three bytes each.
pub(crate) fn mutf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        match unit {
            0x0001..=0x007f => out.push(unit as u8),
            0x0000 | 0x0080..=0x07ff => {
                out.extend_from_slice(&[0xc0 | (unit >> 6) as u8, 0x80 | (unit & 0x3f) as u8])
            }
            _ => out.extend_from_slice(&[
                0xe0 | (unit >> 12) as u8,
                0x80 | ((unit >> 6) & 0x3f) as u8,
                0x80 | (unit & 0x3f) as u8,
            ]),
        }
    }
    out
}

/// Text from Java's modified UTF-8 (`DataInput.readUTF`); None for a malformed sequence, where
/// Java throws `UTFDataFormatException`. A lone surrogate reads as U+FFFD.
pub(crate) fn from_mutf8(b: &[u8]) -> Option<String> {
    let mut units = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = u16::from(b[i]);
        let cont = |k: usize| {
            b.get(i + k)
                .map(|&x| u16::from(x))
                .filter(|x| x & 0xc0 == 0x80)
        };
        match c >> 4 {
            0..=7 => {
                units.push(c);
                i += 1;
            }
            12 | 13 => {
                units.push(((c & 0x1f) << 6) | (cont(1)? & 0x3f));
                i += 2;
            }
            14 => {
                units.push(((c & 0x0f) << 12) | ((cont(1)? & 0x3f) << 6) | (cont(2)? & 0x3f));
                i += 3;
            }
            _ => return None,
        }
    }
    Some(String::from_utf16_lossy(&units))
}

/// A string as NBT writes it. One past `writeUTF`'s 65535 bytes is written empty, as
/// `NbtIo.StringFallbackDataOutput` does when `writeUTF` throws.
pub(crate) fn write_string(out: &mut Vec<u8>, s: &str) {
    let b = mutf8(s);
    let b = if b.len() > usize::from(u16::MAX) {
        Vec::new()
    } else {
        b
    };
    out.extend_from_slice(&(b.len() as u16).to_be_bytes());
    out.extend_from_slice(&b);
}

/// A tag's payload (`Tag.write`).
pub(crate) fn write_payload(out: &mut Vec<u8>, n: &Nbt) {
    match n {
        Nbt::Byte(b) => out.push(*b as u8),
        Nbt::Short(s) => out.extend_from_slice(&s.to_be_bytes()),
        Nbt::Int(i) => out.extend_from_slice(&i.to_be_bytes()),
        Nbt::Long(l) => out.extend_from_slice(&l.to_be_bytes()),
        Nbt::Float(f) => out.extend_from_slice(&f.to_be_bytes()),
        Nbt::Double(d) => out.extend_from_slice(&d.to_be_bytes()),
        Nbt::ByteArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            out.extend(a.iter().map(|&b| b as u8));
        }
        Nbt::String(s) => write_string(out, s),
        Nbt::List(items) => {
            out.push(items.first().map_or(0, id));
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for i in items {
                write_payload(out, i);
            }
        }
        Nbt::Compound(c) => {
            for (k, v) in c {
                out.push(id(v));
                write_string(out, k);
                write_payload(out, v);
            }
            out.push(0);
        }
        Nbt::IntArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            for i in a {
                out.extend_from_slice(&i.to_be_bytes());
            }
        }
        Nbt::LongArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            for l in a {
                out.extend_from_slice(&l.to_be_bytes());
            }
        }
    }
}

/// `NbtIo.writeAnyTag`: the type id, then the payload; `TAG_End` (0) alone for none.
pub(crate) fn write_any(out: &mut Vec<u8>, n: Option<&Nbt>) {
    match n {
        Some(n) => {
            out.push(id(n));
            write_payload(out, n);
        }
        None => out.push(0),
    }
}

/// `NbtIo.writeUnnamedTag` (a file's root): the type id, an empty name, the payload.
pub(crate) fn write_unnamed(out: &mut Vec<u8>, n: &Nbt) {
    out.push(id(n));
    write_string(out, "");
    write_payload(out, n);
}

/// A reader of binary NBT over bytes; `at` is how far it read.
pub(crate) struct Reader<'a> {
    b: &'a [u8],
    pub(crate) at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(b: &'a [u8]) -> Self {
        Reader { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
    /// A count before elements: no more than the bytes left could hold.
    fn len(&mut self) -> Option<usize> {
        let n = usize::try_from(self.i32()?).ok()?;
        (n <= self.b.len() - self.at).then_some(n)
    }
    fn string(&mut self) -> Option<String> {
        let n = usize::from(u16::from_be_bytes(self.take(2)?.try_into().ok()?));
        from_mutf8(self.take(n)?)
    }
    /// A payload of type `id`; None past the end, for an unknown type or nesting past
    /// `NbtAccounter`'s 512 levels.
    fn tag(&mut self, id: u8, depth: u32) -> Option<Nbt> {
        if depth > 512 {
            return None;
        }
        Some(match id {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_bits(self.i32()? as u32)),
            6 => Nbt::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Nbt::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let t = self.u8()?;
                let n = self.len()?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.tag(t, depth + 1)?);
                }
                Nbt::List(items)
            }
            10 => {
                let mut m = BTreeMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.string()?;
                    let v = self.tag(t, depth + 1)?;
                    m.insert(k, v);
                }
                Nbt::Compound(m)
            }
            11 => {
                let n = self.len()?;
                (0..n)
                    .map(|_| self.i32())
                    .collect::<Option<Vec<_>>>()
                    .map(Nbt::IntArray)?
            }
            12 => {
                let n = self.len()?;
                (0..n)
                    .map(|_| self.i64())
                    .collect::<Option<Vec<_>>>()
                    .map(Nbt::LongArray)?
            }
            _ => return None,
        })
    }
    /// `NbtIo.readAnyTag`'s form: Some(None) for `TAG_End`.
    pub(crate) fn any(&mut self) -> Option<Option<Nbt>> {
        match self.u8()? {
            0 => Some(None),
            t => Some(Some(self.tag(t, 0)?)),
        }
    }
    /// `NbtIo.readUnnamedTag`'s form (a file's root): as [`Reader::any`], a name skipped after the
    /// type.
    pub(crate) fn unnamed(&mut self) -> Option<Option<Nbt>> {
        match self.u8()? {
            0 => Some(None),
            t => {
                self.string()?;
                Some(Some(self.tag(t, 0)?))
            }
        }
    }
}
