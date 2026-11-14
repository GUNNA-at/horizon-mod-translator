//! `java.nio`'s byte buffers on the heap: `ByteBuffer.allocate`, `wrap`, and what a mod does
//! with one (a UUID's two longs through `wrap(bytes).getLong()`, a packet's or a file's bytes
//! put together and read back, a hash's input).
//!
//! A buffer is an object over a Java `byte[]` it shares with whoever wrapped it or asks for its
//! `array()`: the array, where in it the buffer begins, and Java's position, limit, capacity,
//! mark and byte order. Every read and write goes to the array, so a duplicate or a slice sees
//! its buffer's bytes, as Java's do. A direct buffer is the same here: nothing outside the
//! runtime reads its memory.
//!
//! A buffer can also lie over memory that is no Java array: an image's pixels, or a block a mod
//! allocated by its address (LWJGL's `memByteBuffer(address, capacity)`,
//! [`crate::native_memory`]). Such a buffer ([`view`]) reads and writes that memory, has no
//! array to give, and says it is direct, as Java's buffer over native memory does.
use crate::{Data, Rt, V};

pub(crate) const BYTE_BUFFER: &str = "java/nio/ByteBuffer";
const BUFFER: &str = "java/nio/Buffer";
const ORDER: &str = "java/nio/ByteOrder";

const ARRAY: &str = "$array";
const OFFSET: &str = "$offset";
const POSITION: &str = "$position";
const LIMIT: &str = "$limit";
const CAPACITY: &str = "$capacity";
const MARK: &str = "$mark";
const LITTLE: &str = "$little";
const READ_ONLY: &str = "$readOnly";
/// Set on a buffer LWJGL allocated: it says it is direct and gives no array, as on a JVM.
const DIRECT: &str = "$direct";

/// Whether this module answers `owner.name desc`.
pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        BYTE_BUFFER | BUFFER | ORDER | "java/nio/MappedByteBuffer"
    )
}

/// Whether byte order `order` is `LITTLE_ENDIAN`.
pub(crate) fn is_little(rt: &Rt, order: V) -> bool {
    rt.get_field(order, LITTLE).i() != 0
}

/// A byte order's constant: `BIG_ENDIAN`, or `LITTLE_ENDIAN` (`little`).
pub(crate) fn order(rt: &mut Rt, little: bool) -> V {
    let name = if little {
        "LITTLE_ENDIAN"
    } else {
        "BIG_ENDIAN"
    };
    rt.intern(format!("compat:byte-order/{name}"), |rt| {
        let o = rt.new_object(ORDER);
        let n = rt.string(name);
        rt.put_field(o, "$name", n);
        rt.put_field(o, LITTLE, V::I(i32::from(little)));
        o
    })
}

/// A buffer's state, as its object holds it.
#[derive(Clone, Copy)]
struct Buf {
    array: V,
    offset: i32,
    position: i32,
    limit: i32,
    capacity: i32,
    mark: i32,
    little: bool,
    read_only: bool,
    direct: bool,
}

/// The state of buffer `b`; None for what is no buffer of this module's.
fn state(rt: &Rt, b: V) -> Option<Buf> {
    let array = rt.get_field(b, ARRAY);
    if b.r() == 0 || array.r() == 0 {
        return None;
    }
    let int = |f: &'static str| rt.get_field(b, f).i();
    Some(Buf {
        array,
        offset: int(OFFSET),
        position: int(POSITION),
        limit: int(LIMIT),
        capacity: int(CAPACITY),
        mark: int(MARK),
        little: int(LITTLE) != 0,
        read_only: int(READ_ONLY) != 0,
        direct: int(DIRECT) != 0,
    })
}

/// A new buffer in state `s` (its byte order big-endian, as every new buffer's is).
fn make(rt: &mut Rt, s: Buf) -> V {
    let b = rt.new_object(BYTE_BUFFER);
    rt.put_field(b, ARRAY, s.array);
    for (field, v) in [
        (OFFSET, s.offset),
        (POSITION, s.position),
        (LIMIT, s.limit),
        (CAPACITY, s.capacity),
        (MARK, s.mark),
        (LITTLE, i32::from(s.little)),
        (READ_ONLY, i32::from(s.read_only)),
        (DIRECT, i32::from(s.direct)),
    ] {
        rt.put_field(b, field, V::I(v));
    }
    b
}

/// A buffer of `capacity` zero bytes (`ByteBuffer.allocate`).
pub(crate) fn allocate(rt: &mut Rt, capacity: i32) -> V {
    let array = rt.alloc("[B", Data::Array(vec![V::I(0); capacity.max(0) as usize]));
    make(
        rt,
        Buf {
            array,
            offset: 0,
            position: 0,
            limit: capacity,
            capacity,
            mark: -1,
            little: false,
            read_only: false,
            direct: false,
        },
    )
}

/// A buffer over `bytes`, ready to be read from its start (`ByteBuffer.wrap`).
pub(crate) fn of_bytes(rt: &mut Rt, bytes: &[i8]) -> V {
    let capacity = bytes.len() as i32;
    let array = rt.alloc(
        "[B",
        Data::Array(bytes.iter().map(|&b| V::I(i32::from(b))).collect()),
    );
    make(
        rt,
        Buf {
            array,
            offset: 0,
            position: 0,
            limit: capacity,
            capacity,
            mark: -1,
            little: false,
            read_only: false,
            direct: false,
        },
    )
}

/// The bytes buffer `b` has left to read, from its position to its limit, the buffer left as
/// it is. None for what is no buffer.
pub(crate) fn left_to_read(rt: &Rt, b: V) -> Option<Vec<i8>> {
    let s = state(rt, b)?;
    Some(remaining(rt, &s).into_iter().map(|b| b as i8).collect())
}

/// How many bytes buffer `b` has room for, from its position to its limit: what a channel's
/// read may put into it. None for what is no buffer.
pub(crate) fn room(rt: &Rt, b: V) -> Option<usize> {
    let s = state(rt, b)?;
    Some((s.limit - s.position).max(0) as usize)
}

/// Puts `bytes` into buffer `b` at its position, which moves past them, as a channel's read
/// does; false for more than it has room for.
pub(crate) fn fill(rt: &mut Rt, b: V, bytes: &[u8]) -> bool {
    let Some(s) = state(rt, b) else {
        return false;
    };
    if bytes.len() > (s.limit - s.position).max(0) as usize
        || !write(rt, s.array, s.offset + s.position, bytes)
    {
        return false;
    }
    rt.put_field(b, POSITION, V::I(s.position + bytes.len() as i32));
    true
}

/// Takes what buffer `b` has left to read, its position moved to its limit, as a channel's
/// write does. None for what is no buffer.
pub(crate) fn take_left(rt: &mut Rt, b: V) -> Option<Vec<u8>> {
    let s = state(rt, b)?;
    let bytes = remaining(rt, &s);
    rt.put_field(b, POSITION, V::I(s.limit));
    Some(bytes)
}

/// Takes what was put into buffer `b` (its start to its position) and empties it for more,
/// as `flip()`, a read of it all and `clear()` do.
pub(crate) fn take_put(rt: &mut Rt, b: V) -> Vec<u8> {
    let Some(s) = state(rt, b) else {
        return Vec::new();
    };
    let bytes = read(rt, s.array, s.offset, s.position.max(0) as usize).unwrap_or_default();
    rt.put_field(b, POSITION, V::I(0));
    rt.put_field(b, LIMIT, V::I(s.capacity));
    bytes
}

fn throw(rt: &mut Rt, class: &'static str) -> V {
    rt.throw_new(class);
    V::Void
}

/// The bytes of `array` at `at..at + n`, or None past its end. `array` is a Java `byte[]`, or
/// other memory a buffer lies over ([`view`]).
pub(crate) fn read(rt: &Rt, array: V, at: i32, n: usize) -> Option<Vec<u8>> {
    let at = usize::try_from(at).ok()?;
    let end = at.checked_add(n)?;
    match rt.heap.get(array.r() as usize).map(|o| &o.data) {
        Some(Data::Array(items)) => items
            .get(at..end)
            .map(|s| s.iter().map(|v| v.i() as u8).collect()),
        Some(Data::Bytes(bytes, _)) => bytes.get(at..end).map(<[u8]>::to_vec),
        _ => None,
    }
}

/// Writes `bytes` into `array` from `at`; false past its end.
pub(crate) fn write(rt: &mut Rt, array: V, at: i32, bytes: &[u8]) -> bool {
    let Ok(at) = usize::try_from(at) else {
        return false;
    };
    let Some(end) = at.checked_add(bytes.len()) else {
        return false;
    };
    match rt.heap.get_mut(array.r() as usize).map(|o| &mut o.data) {
        Some(Data::Array(items)) => match items.get_mut(at..end) {
            Some(slots) => {
                for (slot, b) in slots.iter_mut().zip(bytes) {
                    *slot = V::I(i32::from(*b as i8));
                }
                true
            }
            None => false,
        },
        Some(Data::Bytes(held, _)) => match held.get_mut(at..end) {
            Some(slots) => {
                slots.copy_from_slice(bytes);
                true
            }
            None => false,
        },
        _ => false,
    }
}

/// Whether `array` is a Java `byte[]`: the memory under a [`view`] is not.
fn is_array(rt: &Rt, array: V) -> bool {
    matches!(
        rt.heap.get(array.r() as usize).map(|o| &o.data),
        Some(Data::Array(_))
    )
}

/// A buffer over `capacity` bytes of `memory` from its byte `offset` on, in the machine's byte
/// order (little-endian), ready to be read or written from its start: what LWJGL's
/// `memByteBuffer(address, capacity)` gives for memory the runtime holds.
pub(crate) fn view(rt: &mut Rt, memory: V, offset: i32, capacity: i32) -> V {
    make(
        rt,
        Buf {
            array: memory,
            offset,
            position: 0,
            limit: capacity,
            capacity,
            mark: -1,
            little: true,
            read_only: false,
            direct: false,
        },
    )
}

/// A buffer of `capacity` zero bytes in the machine's byte order, as LWJGL allocates one
/// (`memAlloc`, `memCalloc`).
pub(crate) fn allocate_native(rt: &mut Rt, capacity: i32) -> V {
    let b = allocate(rt, capacity);
    rt.put_field(b, LITTLE, V::I(1));
    rt.put_field(b, DIRECT, V::I(1));
    b
}

/// Whether a buffer in state `s` is a direct one: over memory that is no array ([`view`]), or
/// of LWJGL's allocating (and so are its duplicates and slices).
fn is_direct(rt: &Rt, s: &Buf) -> bool {
    s.direct || !is_array(rt, s.array)
}

/// The memory under buffer `b`, the byte of it the buffer begins at, and the buffer's position
/// and limit; None for what is no buffer of this module's.
pub(crate) fn memory(rt: &Rt, b: V) -> Option<(V, i32, i32, i32)> {
    let s = state(rt, b)?;
    Some((s.array, s.offset, s.position, s.limit))
}

/// Buffer `b`'s capacity (0 for what is no buffer of this module's).
pub(crate) fn capacity(rt: &Rt, b: V) -> i32 {
    state(rt, b).map_or(0, |s| s.capacity)
}

/// Moves buffer `b`'s position to `to`, a place within its limit.
pub(crate) fn set_position(rt: &mut Rt, b: V, to: i32) {
    rt.put_field(b, POSITION, V::I(to));
}

/// The value of kind `kind` (`B`, `C`, `S`, `I`, `J`, `F`, `D`) `bytes` hold in the buffer's order.
pub(crate) fn decode(kind: char, bytes: &[u8], little: bool) -> V {
    let mut wide = [0u8; 8];
    let n = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        // Into the low bytes of a big-endian long.
        wide[8 - n + if little { n - 1 - i } else { i }] = *b;
    }
    let raw = u64::from_be_bytes(wide);
    match kind {
        'B' => V::I(i32::from(raw as u8 as i8)),
        'C' => V::I(i32::from(raw as u16)),
        'S' => V::I(i32::from(raw as u16 as i16)),
        'I' => V::I(raw as u32 as i32),
        'J' => V::J(raw as i64),
        'F' => V::F(f32::from_bits(raw as u32)),
        _ => V::D(f64::from_bits(raw)),
    }
}

/// The bytes of `v`, a value of kind `kind`, in the buffer's order.
pub(crate) fn encode(kind: char, v: V, little: bool) -> Vec<u8> {
    let (raw, n) = match kind {
        'B' => (v.i() as u8 as u64, 1),
        'C' | 'S' => (v.i() as u16 as u64, 2),
        'I' => (v.i() as u32 as u64, 4),
        'J' => (v.j() as u64, 8),
        'F' => (u64::from(v.f().to_bits()), 4),
        _ => (v.d().to_bits(), 8),
    };
    let mut bytes = raw.to_be_bytes()[8 - n..].to_vec();
    if little {
        bytes.reverse();
    }
    bytes
}

/// How many bytes a value of kind `kind` takes.
pub(crate) fn width(kind: char) -> i32 {
    match kind {
        'B' => 1,
        'C' | 'S' => 2,
        'I' | 'F' => 4,
        _ => 8,
    }
}

/// The kind a typed accessor reads or writes (`getInt`, `putLong`: by the name after the verb;
/// plain `get` and `put` move bytes).
fn kind_of(name: &str) -> Option<char> {
    Some(match name.get(3..)? {
        "" => 'B',
        "Char" => 'C',
        "Short" => 'S',
        "Int" => 'I',
        "Long" => 'J',
        "Float" => 'F',
        "Double" => 'D',
        _ => return None,
    })
}

/// The remaining bytes of buffer `b` (position to limit): what its `equals`, `hashCode` and
/// `compareTo` go by.
fn remaining(rt: &Rt, s: &Buf) -> Vec<u8> {
    read(
        rt,
        s.array,
        s.offset + s.position,
        (s.limit - s.position).max(0) as usize,
    )
    .unwrap_or_default()
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == ORDER {
        return Some(match name {
            // The machines this runs on (x86-64, arm64) are little-endian.
            "nativeOrder" => order(rt, true),
            "toString" => rt.get_field(a(0), "$name"),
            _ => return None,
        });
    }
    // The static makers.
    match (name, desc) {
        ("allocate" | "allocateDirect", "(I)Ljava/nio/ByteBuffer;") => {
            if a(0).i() < 0 {
                return Some(throw(rt, "java/lang/IllegalArgumentException"));
            }
            return Some(allocate(rt, a(0).i()));
        }
        ("wrap", "([B)Ljava/nio/ByteBuffer;" | "([BII)Ljava/nio/ByteBuffer;") => {
            let array = a(0);
            if array.r() == 0 {
                return Some(crate::linked::null_receiver(rt, key));
            }
            let capacity = rt.array_len(array);
            let (from, len) = if desc.starts_with("([BII)") {
                (a(1).i(), a(2).i())
            } else {
                (0, capacity)
            };
            if from < 0 || len < 0 || from > capacity - len {
                return Some(throw(rt, "java/lang/IndexOutOfBoundsException"));
            }
            return Some(make(
                rt,
                Buf {
                    array,
                    offset: 0,
                    position: from,
                    limit: from + len,
                    capacity,
                    mark: -1,
                    little: false,
                    read_only: false,
                    direct: false,
                },
            ));
        }
        _ => {}
    }
    let this = a(0);
    let s = state(rt, this)?;
    let put = |rt: &mut Rt, field: &'static str, v: i32| rt.put_field(this, field, V::I(v));
    Some(match (name, args.len() - 1) {
        ("capacity", 0) => V::I(s.capacity),
        ("position", 0) => V::I(s.position),
        ("limit", 0) => V::I(s.limit),
        ("remaining", 0) => V::I((s.limit - s.position).max(0)),
        ("hasRemaining", 0) => V::I(i32::from(s.position < s.limit)),
        ("isReadOnly", 0) => V::I(i32::from(s.read_only)),
        ("isDirect", 0) => V::I(i32::from(is_direct(rt, &s))),
        ("hasArray", 0) => V::I(i32::from(!s.read_only && !is_direct(rt, &s))),
        ("array" | "arrayOffset", 0) if s.read_only => {
            throw(rt, "java/nio/ReadOnlyBufferException")
        }
        ("array" | "arrayOffset", 0) if is_direct(rt, &s) => {
            throw(rt, "java/lang/UnsupportedOperationException")
        }
        ("array", 0) => s.array,
        ("arrayOffset", 0) => V::I(s.offset),
        ("position", 1) => {
            let to = a(1).i();
            if to < 0 || to > s.limit {
                return Some(throw(rt, "java/lang/IllegalArgumentException"));
            }
            put(rt, POSITION, to);
            if s.mark > to {
                put(rt, MARK, -1);
            }
            this
        }
        ("limit", 1) => {
            let to = a(1).i();
            if to < 0 || to > s.capacity {
                return Some(throw(rt, "java/lang/IllegalArgumentException"));
            }
            put(rt, LIMIT, to);
            if s.position > to {
                put(rt, POSITION, to);
            }
            if s.mark > to {
                put(rt, MARK, -1);
            }
            this
        }
        ("mark", 0) => {
            put(rt, MARK, s.position);
            this
        }
        ("reset", 0) => {
            if s.mark < 0 {
                return Some(throw(rt, "java/nio/InvalidMarkException"));
            }
            put(rt, POSITION, s.mark);
            this
        }
        ("clear", 0) => {
            put(rt, POSITION, 0);
            put(rt, LIMIT, s.capacity);
            put(rt, MARK, -1);
            this
        }
        ("flip", 0) => {
            put(rt, LIMIT, s.position);
            put(rt, POSITION, 0);
            put(rt, MARK, -1);
            this
        }
        ("rewind", 0) => {
            put(rt, POSITION, 0);
            put(rt, MARK, -1);
            this
        }
        ("order", 0) => order(rt, s.little),
        ("order", 1) => {
            let little = rt.get_field(a(1), LITTLE).i() != 0;
            put(rt, LITTLE, i32::from(little));
            this
        }
        // A new buffer over the same bytes: all of this one's, or what is left of it.
        ("duplicate" | "asReadOnlyBuffer", 0) => make(
            rt,
            Buf {
                little: false,
                read_only: s.read_only || name == "asReadOnlyBuffer",
                ..s
            },
        ),
        ("slice", 0 | 2) => {
            let (from, len) = if args.len() == 3 {
                (a(1).i(), a(2).i())
            } else {
                (s.position, (s.limit - s.position).max(0))
            };
            if from < 0 || len < 0 || from > s.limit - len {
                return Some(throw(rt, "java/lang/IndexOutOfBoundsException"));
            }
            make(
                rt,
                Buf {
                    offset: s.offset + from,
                    position: 0,
                    limit: len,
                    capacity: len,
                    mark: -1,
                    little: false,
                    ..s
                },
            )
        }
        ("compact", 0) => {
            if s.read_only {
                return Some(throw(rt, "java/nio/ReadOnlyBufferException"));
            }
            let left = remaining(rt, &s);
            write(rt, s.array, s.offset, &left);
            put(rt, POSITION, left.len() as i32);
            put(rt, LIMIT, s.capacity);
            put(rt, MARK, -1);
            this
        }
        // Bytes out into an array: `get(dst)`, `get(dst, from, len)`, and from an index of the
        // buffer's own, which leaves its position (`get(index, dst)`, `get(index, dst, from, len)`).
        ("get", 1..=4) if desc.contains("[B") => {
            let indexed = desc.starts_with("(I[B");
            let dst = a(if indexed { 2 } else { 1 });
            let rest = if indexed { 3 } else { 2 };
            let (from, len) = if args.len() - 1 > rest - 1 {
                (a(rest).i(), a(rest + 1).i())
            } else {
                (0, rt.array_len(dst))
            };
            if dst.r() == 0 {
                return Some(crate::linked::null_receiver(rt, key));
            }
            if from < 0 || len < 0 || from > rt.array_len(dst) - len {
                return Some(throw(rt, "java/lang/IndexOutOfBoundsException"));
            }
            let at = if indexed { a(1).i() } else { s.position };
            if at < 0 || len > s.limit - at {
                return Some(throw(
                    rt,
                    if indexed {
                        "java/lang/IndexOutOfBoundsException"
                    } else {
                        "java/nio/BufferUnderflowException"
                    },
                ));
            }
            let bytes = read(rt, s.array, s.offset + at, len as usize)?;
            write(rt, dst, from, &bytes);
            if !indexed {
                put(rt, POSITION, at + len);
            }
            this
        }
        // Bytes in from an array or from another buffer's remaining bytes.
        ("put", 1..=4)
            if desc.contains("[B") || desc.starts_with("(Ljava/nio/ByteBuffer;") =>
        {
            if s.read_only {
                return Some(throw(rt, "java/nio/ReadOnlyBufferException"));
            }
            let indexed = desc.starts_with("(I[B");
            let src = a(if indexed { 2 } else { 1 });
            if src.r() == 0 {
                return Some(crate::linked::null_receiver(rt, key));
            }
            let from = state(rt, src);
            let bytes = match &from {
                Some(from) => remaining(rt, from),
                None => {
                    let rest = if indexed { 3 } else { 2 };
                    let (from, len) = if args.len() - 1 > rest - 1 {
                        (a(rest).i(), a(rest + 1).i())
                    } else {
                        (0, rt.array_len(src))
                    };
                    if from < 0 || len < 0 || from > rt.array_len(src) - len {
                        return Some(throw(rt, "java/lang/IndexOutOfBoundsException"));
                    }
                    read(rt, src, from, len as usize)?
                }
            };
            let at = if indexed { a(1).i() } else { s.position };
            if at < 0 || bytes.len() as i32 > s.limit - at {
                return Some(throw(
                    rt,
                    if indexed {
                        "java/lang/IndexOutOfBoundsException"
                    } else {
                        "java/nio/BufferOverflowException"
                    },
                ));
            }
            write(rt, s.array, s.offset + at, &bytes);
            if !indexed {
                put(rt, POSITION, at + bytes.len() as i32);
            }
            // The buffer the bytes came from has given all it had left.
            if let Some(from) = from {
                rt.put_field(src, POSITION, V::I(from.limit.max(from.position)));
            }
            this
        }
        // One value, at the position (which it moves) or at an index.
        (n, count @ (0 | 1)) if n.starts_with("get") && kind_of(n).is_some() => {
            let kind = kind_of(n)?;
            let indexed = count == 1;
            let at = if indexed { a(1).i() } else { s.position };
            if at < 0 || width(kind) > s.limit - at {
                return Some(throw(
                    rt,
                    if indexed {
                        "java/lang/IndexOutOfBoundsException"
                    } else {
                        "java/nio/BufferUnderflowException"
                    },
                ));
            }
            let bytes = read(rt, s.array, s.offset + at, width(kind) as usize)?;
            if !indexed {
                put(rt, POSITION, at + width(kind));
            }
            decode(kind, &bytes, s.little)
        }
        (n, count @ (1 | 2)) if n.starts_with("put") && kind_of(n).is_some() => {
            if s.read_only {
                return Some(throw(rt, "java/nio/ReadOnlyBufferException"));
            }
            let kind = kind_of(n)?;
            let indexed = count == 2;
            let at = if indexed { a(1).i() } else { s.position };
            if at < 0 || width(kind) > s.limit - at {
                return Some(throw(
                    rt,
                    if indexed {
                        "java/lang/IndexOutOfBoundsException"
                    } else {
                        "java/nio/BufferOverflowException"
                    },
                ));
            }
            let bytes = encode(kind, a(count), s.little);
            write(rt, s.array, s.offset + at, &bytes);
            if !indexed {
                put(rt, POSITION, at + width(kind));
            }
            this
        }
        ("equals", 1) => {
            let same =
                state(rt, a(1)).is_some_and(|other| remaining(rt, &other) == remaining(rt, &s));
            V::I(i32::from(same))
        }
        ("hashCode", 0) => {
            let hash = remaining(rt, &s).iter().rev().fold(1i32, |h, b| {
                h.wrapping_mul(31).wrapping_add(i32::from(*b as i8))
            });
            V::I(hash)
        }
        ("compareTo", 1) => {
            let other = state(rt, a(1))?;
            // The first bytes that differ, as Java's signed `byte`s (`Byte.compare`: their
            // difference); else the one with fewer left comes first.
            let (mine, theirs) = (remaining(rt, &s), remaining(rt, &other));
            let differ = mine.iter().zip(&theirs).find(|(x, y)| x != y);
            V::I(match differ {
                Some((x, y)) => i32::from(*x as i8) - i32::from(*y as i8),
                None => mine.len() as i32 - theirs.len() as i32,
            })
        }
        ("toString", 0) => {
            let text = format!(
                "java.nio.HeapByteBuffer{}[pos={} lim={} cap={}]",
                if s.read_only { "R" } else { "" },
                s.position,
                s.limit,
                s.capacity
            );
            rt.string(&text)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    fn bytes(rt: &Rt, array: V) -> Vec<i32> {
        crate::jutil::elements(rt, array)
            .iter()
            .map(|b| b.i())
            .collect()
    }

    /// Values as a JVM gives them: a long put big-endian is its bytes from the top, read back
    /// as two ints after a flip; the same bytes read little-endian; the position and the limit
    /// move as Java's, and a read past the limit throws `BufferUnderflowException`.
    #[test]
    fn a_buffer_puts_and_gets_in_its_byte_order() {
        crate::install();
        let mut rt = Rt::new();
        let b = c(
            &mut rt,
            "java/nio/ByteBuffer.allocate.(I)Ljava/nio/ByteBuffer;",
            &[V::I(8)],
        );
        let same = c(
            &mut rt,
            "java/nio/ByteBuffer.putLong.(J)Ljava/nio/ByteBuffer;",
            &[b, V::J(0x0102_0304_0506_0708)],
        );
        assert_eq!(same, b);
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.remaining.()I", &[b]),
            V::I(0)
        );
        let array = c(&mut rt, "java/nio/ByteBuffer.array.()[B", &[b]);
        assert_eq!(bytes(&rt, array), [1, 2, 3, 4, 5, 6, 7, 8]);
        c(
            &mut rt,
            "java/nio/ByteBuffer.flip.()Ljava/nio/ByteBuffer;",
            &[b],
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.getInt.()I", &[b]),
            V::I(0x0102_0304)
        );
        let little = crate::api::get_static(&mut rt, "java/nio/ByteOrder.LITTLE_ENDIAN");
        c(
            &mut rt,
            "java/nio/ByteBuffer.order.(Ljava/nio/ByteOrder;)Ljava/nio/ByteBuffer;",
            &[b, little],
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.getInt.()I", &[b]),
            V::I(0x0807_0605)
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.getShort.(I)S", &[b, V::I(6)]),
            V::I(0x0807)
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.position.()I", &[b]),
            V::I(8)
        );
        c(&mut rt, "java/nio/ByteBuffer.get.()B", &[b]);
        let thrown = rt.thrown.take().expect("past the limit");
        assert_eq!(rt.class_of(thrown), "java/nio/BufferUnderflowException");
        // A double and a negative byte, as their IEEE and two's complement bytes.
        let d = c(
            &mut rt,
            "java/nio/ByteBuffer.allocate.(I)Ljava/nio/ByteBuffer;",
            &[V::I(9)],
        );
        c(
            &mut rt,
            "java/nio/ByteBuffer.putDouble.(D)Ljava/nio/ByteBuffer;",
            &[d, V::D(-2.5)],
        );
        c(
            &mut rt,
            "java/nio/ByteBuffer.put.(B)Ljava/nio/ByteBuffer;",
            &[d, V::I(-1)],
        );
        let array = c(&mut rt, "java/nio/ByteBuffer.array.()[B", &[d]);
        assert_eq!(bytes(&rt, array), [-64, 4, 0, 0, 0, 0, 0, 0, -1]);
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.getDouble.(I)D", &[d, V::I(0)]),
            V::D(-2.5)
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[d, V::I(8)]),
            V::I(-1)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `wrap` shares the array (a UUID's bytes read as two longs; a write shows in the array);
    /// a slice begins at the position and shares it too; `wrap(array, 2, 3)` is the whole
    /// array with its position and limit set; buffers with the same remaining bytes are equal.
    #[test]
    fn a_wrapped_buffer_shares_its_array() {
        crate::install();
        let mut rt = Rt::new();
        let raw: Vec<u8> = (1..=16).collect();
        let array = crate::resources::byte_array(&mut rt, &raw);
        let b = c(
            &mut rt,
            "java/nio/ByteBuffer.wrap.([B)Ljava/nio/ByteBuffer;",
            &[array],
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.getLong.()J", &[b]),
            V::J(0x0102_0304_0506_0708)
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.getLong.()J", &[b]),
            V::J(0x090A_0B0C_0D0E_0F10)
        );
        c(
            &mut rt,
            "java/nio/ByteBuffer.putInt.(II)Ljava/nio/ByteBuffer;",
            &[b, V::I(0), V::I(-1)],
        );
        assert_eq!(bytes(&rt, array)[..5], [-1, -1, -1, -1, 5]);
        let part = c(
            &mut rt,
            "java/nio/ByteBuffer.wrap.([BII)Ljava/nio/ByteBuffer;",
            &[array, V::I(2), V::I(3)],
        );
        let state = |rt: &mut Rt, b: V| {
            ["position", "limit", "capacity", "arrayOffset"].map(|n| {
                let key: &'static str = rt.static_key(&format!("java/nio/ByteBuffer.{n}.()I"));
                c(rt, key, &[b]).i()
            })
        };
        assert_eq!(state(&mut rt, part), [2, 5, 16, 0]);
        let slice = c(
            &mut rt,
            "java/nio/ByteBuffer.slice.()Ljava/nio/ByteBuffer;",
            &[part],
        );
        assert_eq!(state(&mut rt, slice), [0, 3, 3, 2]);
        c(
            &mut rt,
            "java/nio/ByteBuffer.put.(IB)Ljava/nio/ByteBuffer;",
            &[slice, V::I(1), V::I(77)],
        );
        assert_eq!(bytes(&rt, array)[3], 77);
        let copy = crate::resources::byte_array(&mut rt, &[255, 77, 5]);
        let other = c(
            &mut rt,
            "java/nio/ByteBuffer.wrap.([B)Ljava/nio/ByteBuffer;",
            &[copy],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/nio/ByteBuffer.equals.(Ljava/lang/Object;)Z",
                &[slice, other]
            ),
            V::I(1)
        );
        let (h1, h2) = (
            c(&mut rt, "java/nio/ByteBuffer.hashCode.()I", &[slice]),
            c(&mut rt, "java/nio/ByteBuffer.hashCode.()I", &[other]),
        );
        // 31 * (31 * (31 * 1 + 5) + 77) + (-1), from the last byte to the first.
        assert_eq!((h1, h2), (V::I(36_982), V::I(36_982)));
        // What is left of one goes into another, and both move.
        let into = c(
            &mut rt,
            "java/nio/ByteBuffer.allocate.(I)Ljava/nio/ByteBuffer;",
            &[V::I(4)],
        );
        c(
            &mut rt,
            "java/nio/ByteBuffer.put.(Ljava/nio/ByteBuffer;)Ljava/nio/ByteBuffer;",
            &[into, other],
        );
        assert_eq!((state(&mut rt, into)[0], state(&mut rt, other)[0]), (3, 3));
        let text = c(
            &mut rt,
            "java/nio/ByteBuffer.toString.()Ljava/lang/String;",
            &[into],
        );
        assert_eq!(
            rt.str_of(text),
            Some("java.nio.HeapByteBuffer[pos=3 lim=4 cap=4]")
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }
}
