//! `java.util.zip`'s compressor and its reverse on their own: `Deflater` and `Inflater`.
//!
//! A mod packs a file's or a packet's bytes with the two directly (the streams built on them
//! are [`crate::streams`]'). Measured over a pack of 367 mods: five jars use a `Deflater` and
//! as many an `Inflater`, a map mod's store of chunks and of its far-away tiles among them.
//!
//! Neither is a zlib kept open between calls here. A `Deflater` gathers the input its
//! `deflate` calls take and packs all of it at the first `deflate` after `finish()`; before
//! that a `deflate` takes the input and gives no bytes yet, which zlib too may do. An `Inflater` unpacks all the input it has
//! whenever more came, and hands the result out as it is asked for. What a caller sees of
//! either through `needsInput`, `finished` and the counts is what a JVM shows for the same
//! calls (the translator's fixture `deflaters`). Two things are another zlib's: the packed
//! bytes themselves, and how many of them a `deflate` before `finish()` gives.
use crate::{Data, Rt, V};
use std::io::Write;

const DEFLATER: &str = "java/util/zip/Deflater";
const INFLATER: &str = "java/util/zip/Inflater";
const FORMAT: &str = "java/util/zip/DataFormatException";

/// Bytes one of the two keeps: all the input it was given, or what it made of it.
const BYTES: &str = "compat/ZipBytes";

/// All the input since the last reset.
const IN: &str = "$in";
/// The bytes made of it, and how many of them were handed out.
const OUT: &str = "$out";
/// How many bytes of the input were taken.
const TAKEN: &str = "$taken";
const LEVEL: &str = "$level";
const NOWRAP: &str = "$nowrap";
const FINISH: &str = "$finish";
/// An inflater: how long the input was when it was last unpacked, and whether the stream
/// ended in it.
const SEEN: &str = "$seen";
const END: &str = "$end";

/// Whether this module answers `owner.name desc`.
pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, DEFLATER | INFLATER)
}

/// The bytes kept in `field` of `this` and the count beside them; none before any were kept.
fn kept<'a>(rt: &'a Rt, this: V, field: &'static str) -> Option<(&'a Vec<u8>, usize)> {
    match rt
        .heap
        .get(rt.get_field(this, field).r() as usize)
        .map(|o| &o.data)
    {
        Some(Data::Bytes(bytes, at)) => Some((bytes, *at)),
        _ => None,
    }
}

fn keep(rt: &mut Rt, this: V, field: &'static str, bytes: Vec<u8>, at: usize) {
    let held = rt.alloc(BYTES, Data::Bytes(bytes, at));
    rt.put_field(this, field, held);
}

/// As new, or as `reset()` leaves one: no input, nothing made, not finishing.
fn clear(rt: &mut Rt, this: V) {
    keep(rt, this, IN, Vec::new(), 0);
    rt.put_field(this, OUT, V::NULL);
    for field in [TAKEN, FINISH, SEEN, END] {
        rt.put_field(this, field, V::I(0));
    }
}

/// `setInput(bytes)` or `setInput(bytes, from, length)`: the input from here on, after what
/// was taken already. Input given before and not yet taken is dropped, as Java drops it (the
/// new array takes the old one's place).
fn more_input(rt: &mut Rt, this: V, args: &[V]) -> Option<V> {
    let array = args.get(1).copied().unwrap_or_default();
    let (from, len) = match args.len() {
        2 => (0, rt.array_len(array)),
        4 => (args[2].i(), args[3].i()),
        _ => return None,
    };
    let Some(more) = crate::nio::read(rt, array, from, len.max(0) as usize) else {
        rt.throw_new("java/lang/ArrayIndexOutOfBoundsException");
        return Some(V::Void);
    };
    let taken = rt.get_field(this, TAKEN).i().max(0) as usize;
    let held = rt.get_field(this, IN);
    if let Some(Data::Bytes(bytes, _)) = rt.heap.get_mut(held.r() as usize).map(|o| &mut o.data) {
        bytes.truncate(taken);
        bytes.extend_from_slice(&more);
    }
    Some(V::Void)
}

/// Hands out up to `length` of the bytes made into `array` from `from`: how many.
fn hand_out(rt: &mut Rt, this: V, array: V, from: i32, length: i32) -> V {
    let Some((bytes, at)) = kept(rt, this, OUT) else {
        return V::I(0);
    };
    let n = (bytes.len() - at).min(length.max(0) as usize);
    let part = bytes[at..at + n].to_vec();
    if !crate::nio::write(rt, array, from, &part) {
        rt.throw_new("java/lang/ArrayIndexOutOfBoundsException");
        return V::I(0);
    }
    let held = rt.get_field(this, OUT);
    if let Some(Data::Bytes(_, at)) = rt.heap.get_mut(held.r() as usize).map(|o| &mut o.data) {
        *at += n;
    }
    V::I(n as i32)
}

/// The array, where in it, and how many bytes a `deflate` or `inflate` call may fill:
/// `(bytes)`, or `(bytes, from, length)` with whatever follows.
fn room(rt: &Rt, args: &[V]) -> (V, i32, i32) {
    let array = args.get(1).copied().unwrap_or_default();
    match args.len() {
        2 => (array, 0, rt.array_len(array)),
        _ => (array, args[2].i(), args[3].i()),
    }
}

/// `input` packed at `level` (zlib's 0 to 9, anything else its default), with zlib's wrapper
/// around it or bare (`nowrap`).
fn pack(input: &[u8], level: i32, nowrap: bool) -> Vec<u8> {
    let level = match u32::try_from(level) {
        Ok(level) if level <= 9 => flate2::Compression::new(level),
        _ => flate2::Compression::default(),
    };
    let packed = if nowrap {
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), level);
        e.write_all(input).and_then(|()| e.finish())
    } else {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), level);
        e.write_all(input).and_then(|()| e.finish())
    };
    // Writing into a `Vec` does not fail.
    packed.unwrap_or_default()
}

/// What `input` unpacks to so far, how many of its bytes that took, and whether the stream
/// ended in it; the reason where it is no such stream.
fn unpack(input: &[u8], nowrap: bool) -> Result<(Vec<u8>, usize, bool), String> {
    let mut d = flate2::Decompress::new(!nowrap);
    let mut out: Vec<u8> = Vec::with_capacity(input.len().saturating_mul(4).max(256));
    loop {
        let taken = d.total_in() as usize;
        let status = d
            .decompress_vec(&input[taken..], &mut out, flate2::FlushDecompress::None)
            .map_err(|e| e.to_string())?;
        let taken = d.total_in() as usize;
        if status == flate2::Status::StreamEnd {
            return Ok((out, taken, true));
        }
        if out.len() < out.capacity() {
            // Room was left: it stopped for want of input.
            return Ok((out, taken, false));
        }
        out.reserve(out.capacity().max(4096));
    }
}

fn deflater(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let int = |rt: &Rt, field: &'static str| rt.get_field(this, field).i();
    Some(match (name, args.len() - 1) {
        // `()`, `(level)`, `(level, nowrap)`.
        ("<init>", n) if n <= 2 => {
            clear(rt, this);
            rt.put_field(this, LEVEL, if n >= 1 { a(1) } else { V::I(-1) });
            rt.put_field(this, NOWRAP, V::I(i32::from(n == 2 && a(2).i() != 0)));
            V::Void
        }
        ("setInput", 1 | 3) if desc.starts_with("([B") => return more_input(rt, this, args),
        ("setLevel", 1) => {
            rt.put_field(this, LEVEL, a(1));
            V::Void
        }
        // How it looks for matches does not change what unpacks.
        ("setStrategy", 1) => V::Void,
        ("finish", 0) => {
            rt.put_field(this, FINISH, V::I(1));
            V::Void
        }
        ("needsInput", 0) => {
            let given = kept(rt, this, IN).map_or(0, |(bytes, _)| bytes.len());
            V::I(i32::from(int(rt, TAKEN) as usize >= given))
        }
        ("finished", 0) => V::I(i32::from(
            kept(rt, this, OUT).is_some_and(|(bytes, at)| at == bytes.len()),
        )),
        // `deflate(bytes)`, `(bytes, from, length)`, and the same with how to flush: a flush
        // that asks for the bytes of what was given so far is not answered before `finish()`.
        ("deflate", 1 | 3 | 4) if desc.starts_with("([B") => {
            let finishing = int(rt, FINISH) != 0;
            if args.len() == 5 && a(4).i() != 0 && !finishing {
                return None;
            }
            let (input, _) = kept(rt, this, IN)?;
            let given = input.len();
            if finishing && kept(rt, this, OUT).is_none() {
                let packed = pack(input, int(rt, LEVEL), int(rt, NOWRAP) != 0);
                keep(rt, this, OUT, packed, 0);
            }
            rt.put_field(this, TAKEN, V::I(given as i32));
            let (array, from, length) = room(rt, args);
            hand_out(rt, this, array, from, length)
        }
        ("getTotalIn", 0) => V::I(int(rt, TAKEN)),
        ("getBytesRead", 0) => V::J(i64::from(int(rt, TAKEN))),
        ("getTotalOut" | "getBytesWritten", 0) => {
            let out = kept(rt, this, OUT).map_or(0, |(_, at)| at);
            if name == "getTotalOut" {
                V::I(out as i32)
            } else {
                V::J(out as i64)
            }
        }
        ("reset", 0) => {
            clear(rt, this);
            V::Void
        }
        ("end", 0) => V::Void,
        _ => return None,
    })
}

fn inflater(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let int = |rt: &Rt, field: &'static str| rt.get_field(this, field).i();
    // The input's length, and how many bytes made are still to hand out.
    let given = |rt: &Rt| kept(rt, this, IN).map_or(0, |(bytes, _)| bytes.len());
    let waiting = |rt: &Rt| kept(rt, this, OUT).map_or(0, |(bytes, at)| bytes.len() - at);
    Some(match (name, args.len() - 1) {
        // `()`, `(nowrap)`.
        ("<init>", n) if n <= 1 => {
            clear(rt, this);
            rt.put_field(this, NOWRAP, V::I(i32::from(n == 1 && a(1).i() != 0)));
            V::Void
        }
        ("setInput", 1 | 3) if desc.starts_with("([B") => return more_input(rt, this, args),
        ("inflate", 1 | 3) if desc.starts_with("([B") => {
            let (input, _) = kept(rt, this, IN)?;
            if input.len() != int(rt, SEEN) as usize || kept(rt, this, OUT).is_none() {
                let seen = input.len();
                let (bytes, taken, end) = match unpack(input, int(rt, NOWRAP) != 0) {
                    Ok(unpacked) => unpacked,
                    Err(why) => return Some(crate::jlang::throw_with(rt, FORMAT, &why)),
                };
                let handed = kept(rt, this, OUT).map_or(0, |(_, at)| at);
                keep(rt, this, OUT, bytes, handed);
                rt.put_field(this, SEEN, V::I(seen as i32));
                rt.put_field(this, TAKEN, V::I(taken as i32));
                rt.put_field(this, END, V::I(i32::from(end)));
            }
            let (array, from, length) = room(rt, args);
            hand_out(rt, this, array, from, length)
        }
        ("finished", 0) => V::I(i32::from(int(rt, END) != 0 && waiting(rt) == 0)),
        // No input is left once all of it was unpacked and handed out.
        ("needsInput", 0) => V::I(i32::from(
            waiting(rt) == 0 && int(rt, TAKEN) as usize >= given(rt),
        )),
        // No stream here is made with a dictionary agreed beforehand.
        ("needsDictionary", 0) => V::I(0),
        ("getRemaining", 0) => V::I((given(rt) as i32 - int(rt, TAKEN)).max(0)),
        ("getTotalIn", 0) => V::I(int(rt, TAKEN)),
        ("getBytesRead", 0) => V::J(i64::from(int(rt, TAKEN))),
        ("getTotalOut" | "getBytesWritten", 0) => {
            let out = kept(rt, this, OUT).map_or(0, |(_, at)| at);
            if name == "getTotalOut" {
                V::I(out as i32)
            } else {
                V::J(out as i64)
            }
        }
        ("reset", 0) => {
            clear(rt, this);
            V::Void
        }
        ("end", 0) => V::Void,
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        DEFLATER => deflater(rt, name, desc, args),
        INFLATER => inflater(rt, name, desc, args),
        _ => None,
    }
}
