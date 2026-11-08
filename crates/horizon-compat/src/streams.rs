//! Java's byte streams past a resource's ([`crate::resources`]), and Minecraft's NBT files over
//! them: files' streams (`FileInputStream`, `FileOutputStream`, `Files.newInputStream` and
//! `newOutputStream`), gzip's and zlib's (`GZIPInputStream`, `GZIPOutputStream`,
//! `InflaterInputStream`, `DeflaterOutputStream`), `DataInputStream` and `DataOutputStream`
//! (`DataInput`, `DataOutput`), `ByteArrayOutputStream` and the filters over them; and `NbtIo`,
//! which reads and writes NBT through them (Create's schematics, a mod's own files, a structure's
//! `.nbt`).
//!
//! An input stream is its bytes and where reading is ([`Data::Bytes`]), as a resource's is: a
//! stream around another takes what is left of that one when it is made (inflated, for gzip's and
//! zlib's), and a file's reads the file then. An output stream hands on what it is given at once:
//! into its bytes (`ByteArrayOutputStream`), onto the end of its file (field `$path`), or to the
//! stream it is around (field `$out`); gzip's and zlib's keep it until `finish` or `close`, then
//! hand it on deflated. A mod's own stream is read and written through its own `read` and `write`,
//! and a mod's class under one of Java's acts as that one where it doesn't override it.
use crate::{Data, Rt, V, api, nbt};
use std::collections::BTreeMap;
use std::io::{Read, Write};

const INPUT_STREAM: &str = "java/io/InputStream";
const DATA_IN: &str = "java/io/DataInputStream";
const DATA_INPUT: &str = "java/io/DataInput";
const GZIP_IN: &str = "java/util/zip/GZIPInputStream";
const INFLATER_IN: &str = "java/util/zip/InflaterInputStream";
const FILE_IN: &str = "java/io/FileInputStream";
const OUTPUT_STREAM: &str = "java/io/OutputStream";
const BYTES_OUT: &str = "java/io/ByteArrayOutputStream";
const FILE_OUT: &str = "java/io/FileOutputStream";
const FILTER_OUT: &str = "java/io/FilterOutputStream";
const BUFFERED_OUT: &str = "java/io/BufferedOutputStream";
const DATA_OUT: &str = "java/io/DataOutputStream";
const DATA_OUTPUT: &str = "java/io/DataOutput";
const GZIP_OUT: &str = "java/util/zip/GZIPOutputStream";
const DEFLATER_OUT: &str = "java/util/zip/DeflaterOutputStream";
const FLUSHABLE: &str = "java/io/Flushable";
const CLOSEABLE: &str = "java/io/Closeable";
const AUTO_CLOSEABLE: &str = "java/lang/AutoCloseable";
const NBT_IO: &str = "net/minecraft/nbt/NbtIo";
const IO_EXCEPTION: &str = "java/io/IOException";
const EOF: &str = "java/io/EOFException";
const ZIP_EXCEPTION: &str = "java/util/zip/ZipException";
const NPE: &str = "java/lang/NullPointerException";

/// Calls into a mod's own stream (its override, else Java's class it is under, here).
const READ_BYTES: (&str, &str) = ("java/io/InputStream.read.([BII)I", "read.([BII)I");
const READ_BYTE: (&str, &str) = ("java/io/InputStream.read.()I", "read.()I");
const CLOSE_IN: (&str, &str) = ("java/io/InputStream.close.()V", "close.()V");
const WRITE_BYTES: (&str, &str) = ("java/io/OutputStream.write.([BII)V", "write.([BII)V");
const WRITE_BYTE: (&str, &str) = ("java/io/OutputStream.write.(I)V", "write.(I)V");
const FLUSH: (&str, &str) = ("java/io/OutputStream.flush.()V", "flush.()V");
const CLOSE_OUT: (&str, &str) = ("java/io/OutputStream.close.()V", "close.()V");

/// The most of a mod's own stream read in one go (it may not end).
const DRAIN_LIMIT: usize = 1 << 30;

fn in_class(c: &str) -> bool {
    matches!(c, DATA_IN | GZIP_IN | INFLATER_IN | FILE_IN)
}

fn out_class(c: &str) -> bool {
    matches!(
        c,
        OUTPUT_STREAM
            | BYTES_OUT
            | FILE_OUT
            | FILTER_OUT
            | BUFFERED_OUT
            | DATA_OUT
            | GZIP_OUT
            | DEFLATER_OUT
    )
}

pub fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    in_class(owner)
        || out_class(owner)
        || matches!(owner, DATA_INPUT | DATA_OUTPUT | FLUSHABLE | NBT_IO)
        || (matches!(owner, CLOSEABLE | AUTO_CLOSEABLE) && name == "close")
        || (owner == INPUT_STREAM && name == "nullInputStream")
        || (name == "transferTo"
            && (owner.starts_with("java/io/") || owner.starts_with("java/util/zip/")))
}

/// Java's stream classes for `instanceof`.
pub(crate) fn is_a(c: &str, class: &str) -> bool {
    match class {
        INPUT_STREAM => in_class(c),
        "java/io/FilterInputStream" => matches!(c, DATA_IN | GZIP_IN | INFLATER_IN),
        INFLATER_IN => c == GZIP_IN,
        DATA_INPUT => c == DATA_IN,
        OUTPUT_STREAM | CLOSEABLE | AUTO_CLOSEABLE | FLUSHABLE => {
            out_class(c) || (class != OUTPUT_STREAM && class != FLUSHABLE && in_class(c))
        }
        FILTER_OUT => matches!(c, BUFFERED_OUT | DATA_OUT | GZIP_OUT | DEFLATER_OUT),
        DEFLATER_OUT => c == GZIP_OUT,
        DATA_OUTPUT => c == DATA_OUT,
        _ => false,
    }
}

/// Whether `v` is an object of a mod's own class: its overrides answer first.
pub(crate) fn is_mod(rt: &Rt, v: V) -> bool {
    rt.heap.get(v.r() as usize).is_some_and(|o| o.cid != 0)
}

/// Java's stream class `v` is, or the one a mod's own stream class is under; "" for none.
fn base(rt: &Rt, v: V) -> &'static str {
    let mut c = rt.class_of(v);
    for _ in 0..64 {
        if in_class(c) || out_class(c) || c == INPUT_STREAM {
            return c;
        }
        match rt.supers.get(c) {
            Some(&up) => c = up,
            None => break,
        }
    }
    ""
}

/// Throws `class` with `message`.
fn throw(rt: &mut Rt, class: &'static str, message: &str) {
    crate::files::throw(rt, class, message);
}

fn bytes_of(rt: &Rt, v: V) -> Option<&Vec<u8>> {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Bytes(b, _)) => Some(b),
        _ => None,
    }
}

fn bytes_mut(rt: &mut Rt, v: V) -> Option<&mut Vec<u8>> {
    match rt.heap.get_mut(v.r() as usize).map(|o| &mut o.data) {
        Some(Data::Bytes(b, _)) => Some(b),
        _ => None,
    }
}

// Deflate, as `java.util.zip` writes and reads it.

/// What `GZIPOutputStream` writes for `raw`.
pub(crate) fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(raw);
    e.finish().unwrap_or_default()
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(raw);
    e.finish().unwrap_or_default()
}

/// What `GZIPInputStream` reads from `packed` (each member, as Java reads concatenated ones), or
/// the exception it throws: `EOFException` for no bytes, `ZipException` past the header.
pub(crate) fn gunzip(packed: &[u8]) -> Result<Vec<u8>, (&'static str, String)> {
    if packed.is_empty() {
        return Err((EOF, String::new()));
    }
    if !packed.starts_with(&[0x1f, 0x8b]) {
        return Err((ZIP_EXCEPTION, "Not in GZIP format".into()));
    }
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(packed)
        .read_to_end(&mut out)
        .map_err(inflate_error)?;
    Ok(out)
}

fn unzlib(packed: &[u8]) -> Result<Vec<u8>, (&'static str, String)> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(packed)
        .read_to_end(&mut out)
        .map_err(inflate_error)?;
    Ok(out)
}

fn inflate_error(e: std::io::Error) -> (&'static str, String) {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        (EOF, "Unexpected end of ZLIB input stream".into())
    } else {
        (ZIP_EXCEPTION, e.to_string())
    }
}

// Input.

/// What is left of input stream `v`, taken (reading is then at its end): a mod's own stream's
/// through its `read`, the runtime's from its bytes. None for what isn't a stream.
pub(crate) fn drain(rt: &mut Rt, v: V) -> Option<Vec<u8>> {
    if !is_mod(rt, v) {
        return crate::resources::take(rt, v);
    }
    let buf = rt.new_array(8192, V::I(0));
    let mut out = Vec::new();
    while out.len() < DRAIN_LIMIT {
        let n = api::virtual_call(rt, READ_BYTES, &[v, buf, V::I(0), V::I(8192)]).i();
        if rt.thrown.is_some() || n <= 0 {
            break;
        }
        out.extend((0..n).map(|i| rt.array_get(buf, i).i() as u8));
    }
    Some(out)
}

/// Closes input stream `v` as its owner does: a mod's own through its `close`.
fn close_in(rt: &mut Rt, v: V) {
    if is_mod(rt, v) {
        api::virtual_call(rt, CLOSE_IN, &[v]);
    }
}

/// `InputStream`'s own methods on a mod's stream that has no bytes of the runtime's (one
/// `extends InputStream`): through its `read()`, as Java's defaults are.
pub(crate) fn mod_read(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let next = |rt: &mut Rt| {
        let c = api::virtual_call(rt, READ_BYTE, &[this]).i();
        (rt.thrown.is_none() && c >= 0).then_some(c as u8)
    };
    Some(match (name, desc) {
        // `read()` itself is the mod's to give (it's abstract): none here is the end.
        ("read", "()I") => V::I(-1),
        ("read", "([B)I" | "([BII)I") | ("readNBytes", "([BII)I") => {
            let (off, len) = if desc == "([B)I" {
                (0, rt.array_len(a(1)))
            } else {
                (a(2).i(), a(3).i())
            };
            let mut n = 0;
            while n < len {
                let Some(b) = next(rt) else { break };
                rt.array_set(a(1), off + n, V::I(i32::from(b as i8)));
                n += 1;
            }
            V::I(if n == 0 && len > 0 && name == "read" {
                -1
            } else {
                n
            })
        }
        ("readNBytes", "(I)[B") => {
            let mut got = Vec::new();
            while got.len() < a(1).i().max(0) as usize {
                let Some(b) = next(rt) else { break };
                got.push(b);
            }
            crate::resources::byte_array(rt, &got)
        }
        ("readAllBytes", _) => {
            let got = drain(rt, this).unwrap_or_default();
            crate::resources::byte_array(rt, &got)
        }
        ("skip", _) => {
            let mut n = 0i64;
            while n < a(1).j() && next(rt).is_some() {
                n += 1;
            }
            V::J(n)
        }
        ("available" | "markSupported", _) => V::I(0),
        ("close" | "mark", _) => V::Void,
        ("reset", _) => {
            throw(rt, IO_EXCEPTION, "mark/reset not supported");
            V::Void
        }
        _ => return None,
    })
}

/// The next `n` bytes of input stream `v`; past its end, `EOFException` (reading at the end).
fn read_n(rt: &mut Rt, v: V, n: usize) -> Option<Vec<u8>> {
    if v.r() == 0 {
        rt.throw_new(NPE);
        return None;
    }
    let got = match rt.heap.get_mut(v.r() as usize).map(|o| &mut o.data) {
        Some(Data::Bytes(b, at)) => {
            let end = at.saturating_add(n);
            let part = b.get(*at..end).map(<[u8]>::to_vec);
            *at = if part.is_some() { end } else { b.len() };
            part
        }
        _ => None,
    };
    if got.is_none() {
        rt.throw_new(EOF);
    }
    got
}

/// `DataInput.readUTF`: a u16 length, then that many bytes of Java's modified UTF-8.
fn read_utf(rt: &mut Rt, v: V) -> V {
    let Some(len) = read_n(rt, v, 2) else {
        return V::NULL;
    };
    let Some(b) = read_n(rt, v, usize::from(u16::from_be_bytes([len[0], len[1]]))) else {
        return V::NULL;
    };
    match nbt::from_mutf8(&b) {
        Some(s) => rt.string(&s),
        None => {
            throw(rt, "java/io/UTFDataFormatException", "malformed input");
            V::NULL
        }
    }
}

/// The class's own or `DataInput`'s methods on input stream `this` (a `DataInputStream`, a gzip
/// one...); the rest are `InputStream`'s ([`crate::resources`]).
fn in_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let width = match name {
        "readBoolean" | "readByte" | "readUnsignedByte" => 1,
        "readShort" | "readUnsignedShort" | "readChar" => 2,
        "readInt" | "readFloat" => 4,
        "readLong" | "readDouble" => 8,
        _ => 0,
    };
    if width > 0 {
        let Some(b) = read_n(rt, this, width) else {
            return Some(match name {
                "readLong" => V::J(0),
                "readFloat" => V::F(0.0),
                "readDouble" => V::D(0.0),
                _ => V::I(0),
            });
        };
        let int = |n: usize| {
            b[..n]
                .iter()
                .fold(0i64, |acc, &x| (acc << 8) | i64::from(x))
        };
        return Some(match name {
            "readBoolean" => V::I((b[0] != 0) as i32),
            "readByte" => V::I(i32::from(b[0] as i8)),
            "readUnsignedByte" => V::I(i32::from(b[0])),
            "readShort" => V::I(i32::from(int(2) as u16 as i16)),
            "readUnsignedShort" | "readChar" => V::I(int(2) as i32),
            "readInt" => V::I(int(4) as u32 as i32),
            "readFloat" => V::F(f32::from_bits(int(4) as u32)),
            "readLong" => V::J(int(8)),
            _ => V::D(f64::from_bits(int(8) as u64)),
        });
    }
    Some(match (name, desc) {
        ("readFully", "([B)V" | "([BII)V") => {
            let (off, len) = if desc == "([B)V" {
                (0, rt.array_len(a(1)))
            } else {
                (a(2).i(), a(3).i())
            };
            if let Some(b) = read_n(rt, this, len.max(0) as usize) {
                for (i, x) in b.into_iter().enumerate() {
                    rt.array_set(a(1), off + i as i32, V::I(i32::from(x as i8)));
                }
            }
            V::Void
        }
        ("skipBytes", _) => {
            let n = a(1).i().max(0) as usize;
            V::I(
                match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                    Some(Data::Bytes(b, at)) => {
                        let end = at.saturating_add(n).min(b.len());
                        let skipped = end - *at;
                        *at = end;
                        skipped as i32
                    }
                    _ => 0,
                },
            )
        }
        ("readUTF", "()Ljava/lang/String;") => read_utf(rt, this),
        // `readLine`: bytes as Latin-1 up to `\n`, `\r` or `\r\n`; null at the end.
        ("readLine", _) => {
            let line = match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                Some(Data::Bytes(b, at)) if *at < b.len() => {
                    let rest = &b[*at..];
                    let end = rest
                        .iter()
                        .position(|&c| c == b'\n' || c == b'\r')
                        .unwrap_or(rest.len());
                    let line: String = rest[..end].iter().map(|&c| char::from(c)).collect();
                    let mut next = *at + end;
                    if b.get(next) == Some(&b'\r') {
                        next += 1;
                    }
                    if b.get(next) == Some(&b'\n') && (next == *at + end || b[next - 1] == b'\r') {
                        next += 1;
                    }
                    *at = next;
                    Some(line)
                }
                _ => None,
            };
            match line {
                Some(l) => rt.string(&l),
                None => V::NULL,
            }
        }
        // Closing a stream around another closes that one.
        ("close", _) => {
            let inner = rt.get_field(this, "$in");
            close_in(rt, inner);
            V::Void
        }
        _ => return crate::resources::stream_call(rt, name, desc, args),
    })
}

/// A path's (`File`'s, `String`'s) text, or None (with `NullPointerException` thrown for null).
fn path_text(rt: &mut Rt, v: V) -> Option<String> {
    match rt.str_of(v) {
        Some(s) => Some(s.to_owned()),
        None => {
            rt.throw_new(NPE);
            None
        }
    }
}

/// `FileNotFoundException`'s message: the path and the system's reason, as Java gives them.
fn not_found(rt: &mut Rt, path: &str, e: &std::io::Error) {
    use std::io::ErrorKind as K;
    let why = match e.kind() {
        K::NotFound => "No such file or directory".to_owned(),
        K::IsADirectory => "Is a directory".to_owned(),
        K::PermissionDenied => "Permission denied".to_owned(),
        _ => e.to_string(),
    };
    throw(
        rt,
        "java/io/FileNotFoundException",
        &format!("{path} ({why})"),
    );
}

/// A stream over a file's bytes (`Files.newInputStream`).
pub(crate) fn file_input(rt: &mut Rt, bytes: Vec<u8>) -> V {
    rt.alloc(INPUT_STREAM, Data::Bytes(bytes, 0))
}

// Output.

/// `Files.newOutputStream(path, options...)`: opened as Java opens it (no options: made, and
/// emptied when it is there; else made only with `CREATE` or `CREATE_NEW`, onto its end with
/// `APPEND`), then written onto its end.
pub(crate) fn open_output(rt: &mut Rt, text: &str, options: V) -> V {
    let disk = crate::files::on_disk(rt, text);
    let given = crate::jutil::elements(rt, options);
    let has = |n: &str| crate::files::has_option(rt, options, n);
    let (create, create_new, append, truncate) = if given.is_empty() {
        (true, false, false, true)
    } else {
        (
            has("CREATE"),
            has("CREATE_NEW"),
            has("APPEND"),
            has("TRUNCATE_EXISTING"),
        )
    };
    let mut o = std::fs::OpenOptions::new();
    o.write(true).append(append).truncate(truncate && !append);
    if create_new {
        o.create_new(true);
    } else if create {
        o.create(true);
    }
    match o.open(&disk) {
        Ok(_) => {
            let s = rt.new_object(FILE_OUT);
            let p = rt.string(&disk.to_string_lossy());
            rt.put_field(s, "$path", p);
            s
        }
        Err(e) => crate::files::throw_io(rt, &e, text),
    }
}

/// Writes `bytes` to output stream `out` as its caller does: a mod's own stream through its
/// `write(byte[], int, int)`, one of Java's as that class writes.
pub(crate) fn write_to(rt: &mut Rt, out: V, bytes: &[u8]) {
    if out.r() == 0 {
        rt.throw_new(NPE);
    } else if is_mod(rt, out) {
        let arr = crate::resources::byte_array(rt, bytes);
        api::virtual_call(
            rt,
            WRITE_BYTES,
            &[out, arr, V::I(0), V::I(bytes.len() as i32)],
        );
    } else {
        let class = base(rt, out);
        write_as(rt, out, class, bytes);
    }
}

/// `write(bytes)` as Java's class `class` does it on `this`.
fn write_as(rt: &mut Rt, this: V, class: &str, bytes: &[u8]) {
    match class {
        BYTES_OUT => {
            if let Some(b) = bytes_mut(rt, this) {
                b.extend_from_slice(bytes);
            }
        }
        FILE_OUT => {
            if rt.get_field(this, "$closed").i() != 0 {
                return throw(rt, IO_EXCEPTION, "Stream Closed");
            }
            let Some(path) = rt.str_of(rt.get_field(this, "$path")).map(str::to_owned) else {
                return;
            };
            if let Err(e) = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .and_then(|mut f| f.write_all(bytes))
            {
                throw(rt, IO_EXCEPTION, &e.to_string());
            }
        }
        GZIP_OUT | DEFLATER_OUT => {
            if rt.get_field(this, "$finished").i() != 0 {
                return throw(rt, IO_EXCEPTION, "write beyond end of stream");
            }
            if let Some(b) = bytes_mut(rt, this) {
                b.extend_from_slice(bytes);
            }
        }
        DATA_OUT | FILTER_OUT | BUFFERED_OUT => {
            if class == DATA_OUT {
                // `DataOutputStream.size()`: what it wrote, up to `Integer.MAX_VALUE`.
                let n = rt
                    .get_field(this, "$written")
                    .i()
                    .saturating_add(bytes.len().min(i32::MAX as usize) as i32);
                rt.put_field(this, "$written", V::I(n));
            }
            let inner = rt.get_field(this, "$out");
            write_to(rt, inner, bytes);
        }
        // `OutputStream.nullOutputStream()`; a mod's stream with no `write(int)` of its own.
        _ => {}
    }
}

/// Flushes output stream `out` as its caller does (a mod's own through its `flush`).
fn flush_out(rt: &mut Rt, out: V) {
    if is_mod(rt, out) {
        api::virtual_call(rt, FLUSH, &[out]);
    } else {
        let class = base(rt, out);
        flush_as(rt, out, class);
    }
}

fn flush_as(rt: &mut Rt, this: V, class: &str) {
    if matches!(
        class,
        FILTER_OUT | BUFFERED_OUT | DATA_OUT | GZIP_OUT | DEFLATER_OUT
    ) {
        let inner = rt.get_field(this, "$out");
        if inner.r() != 0 {
            flush_out(rt, inner);
        }
    }
}

/// Closes output stream `out` as its caller does (a mod's own through its `close`).
pub(crate) fn close_out(rt: &mut Rt, out: V) {
    if is_mod(rt, out) {
        api::virtual_call(rt, CLOSE_OUT, &[out]);
    } else {
        let class = base(rt, out);
        close_as(rt, out, class);
    }
}

/// `close()` as Java's class `class` does it: a filter flushes, then closes the stream it is around;
/// gzip's and zlib's finish first.
fn close_as(rt: &mut Rt, this: V, class: &str) {
    match class {
        FILE_OUT => rt.put_field(this, "$closed", V::I(1)),
        FILTER_OUT | BUFFERED_OUT | DATA_OUT | GZIP_OUT | DEFLATER_OUT => {
            if rt.get_field(this, "$closed").i() != 0 {
                return;
            }
            rt.put_field(this, "$closed", V::I(1));
            if matches!(class, GZIP_OUT | DEFLATER_OUT) {
                finish(rt, this, class);
            } else {
                flush_out(rt, this);
            }
            let inner = rt.get_field(this, "$out");
            if inner.r() != 0 && rt.thrown.is_none() {
                close_out(rt, inner);
            }
        }
        _ => {}
    }
}

/// `finish()`: what gzip's or zlib's stream kept, deflated, to the stream it is around; once.
fn finish(rt: &mut Rt, this: V, class: &str) {
    if rt.get_field(this, "$finished").i() != 0 {
        return;
    }
    rt.put_field(this, "$finished", V::I(1));
    let raw = bytes_mut(rt, this).map(std::mem::take).unwrap_or_default();
    let packed = if class == GZIP_OUT {
        gzip(&raw)
    } else {
        zlib(&raw)
    };
    let inner = rt.get_field(this, "$out");
    write_to(rt, inner, &packed);
}

/// The class's own, `DataOutput`'s and `OutputStream`'s methods on output stream `this`, whose
/// Java class (or the one its mod class is under) is `class`.
fn out_call(rt: &mut Rt, class: &'static str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let text = |rt: &Rt| rt.str_of(a(1)).unwrap_or("").to_owned();
    let bytes: Vec<u8> = match (name, desc) {
        ("write", "(I)V") | ("writeByte", _) => vec![a(1).i() as u8],
        ("writeBoolean", _) => vec![u8::from(a(1).i() != 0)],
        ("writeShort" | "writeChar", _) => (a(1).i() as u16).to_be_bytes().to_vec(),
        ("writeInt", _) => a(1).i().to_be_bytes().to_vec(),
        ("writeLong", _) => a(1).j().to_be_bytes().to_vec(),
        ("writeFloat", _) => a(1).f().to_bits().to_be_bytes().to_vec(),
        ("writeDouble", _) => a(1).d().to_bits().to_be_bytes().to_vec(),
        // `writeBytes(String)`: each char's low byte; `writeChars`: each char's two.
        ("writeBytes", "(Ljava/lang/String;)V") => {
            text(rt).encode_utf16().map(|c| c as u8).collect()
        }
        ("writeChars", _) => text(rt).encode_utf16().flat_map(u16::to_be_bytes).collect(),
        ("writeUTF", _) => {
            let b = nbt::mutf8(&text(rt));
            if b.len() > usize::from(u16::MAX) {
                throw(
                    rt,
                    "java/io/UTFDataFormatException",
                    &format!("encoded string too long: {} bytes", b.len()),
                );
                return Some(V::Void);
            }
            let mut out = (b.len() as u16).to_be_bytes().to_vec();
            out.extend_from_slice(&b);
            out
        }
        ("write" | "writeBytes", "([B)V") => crate::resources::array_bytes(rt, a(1)),
        ("write", "([BII)V") => {
            let all = crate::resources::array_bytes(rt, a(1));
            let (off, len) = (a(2).i(), a(3).i());
            match usize::try_from(off)
                .ok()
                .zip(usize::try_from(len).ok())
                .and_then(|(o, l)| all.get(o..o.checked_add(l)?))
            {
                Some(part) => part.to_vec(),
                None => {
                    rt.throw_new("java/lang/IndexOutOfBoundsException");
                    return Some(V::Void);
                }
            }
        }
        _ => return out_other(rt, class, name, desc, args),
    };
    if a(0).r() == 0 {
        rt.throw_new(NPE);
    } else if name == "write"
        && desc.starts_with("([B")
        && is_mod(rt, this)
        && matches!(class, OUTPUT_STREAM | FILTER_OUT)
    {
        // Java's `OutputStream` and `FilterOutputStream` write an array byte by byte through
        // `write(int)`: a mod's own stream's override gets each.
        for b in bytes {
            api::virtual_call(rt, WRITE_BYTE, &[this, V::I(i32::from(b))]);
            if rt.thrown.is_some() {
                break;
            }
        }
    } else {
        write_as(rt, this, class, &bytes);
    }
    Some(V::Void)
}

fn out_other(rt: &mut Rt, class: &'static str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match (name, desc) {
        ("flush", _) => {
            flush_as(rt, this, class);
            V::Void
        }
        ("close", _) => {
            close_as(rt, this, class);
            V::Void
        }
        ("finish", _) if matches!(class, GZIP_OUT | DEFLATER_OUT) => {
            finish(rt, this, class);
            V::Void
        }
        ("size", _) if class == DATA_OUT => rt.get_field(this, "$written"),
        ("size", _) if class == BYTES_OUT => V::I(bytes_of(rt, this).map_or(0, Vec::len) as i32),
        ("toByteArray", _) if class == BYTES_OUT => {
            let b = bytes_of(rt, this).cloned().unwrap_or_default();
            crate::resources::byte_array(rt, &b)
        }
        ("reset", _) if class == BYTES_OUT => {
            if let Some(b) = bytes_mut(rt, this) {
                b.clear();
            }
            V::Void
        }
        // `toString([charset])`: the bytes as UTF-8, Java's default charset.
        ("toString", _) if class == BYTES_OUT => {
            let s = String::from_utf8_lossy(bytes_of(rt, this).map_or(&[][..], Vec::as_slice))
                .into_owned();
            rt.string(&s)
        }
        ("writeTo", _) if class == BYTES_OUT => {
            let b = bytes_of(rt, this).cloned().unwrap_or_default();
            write_to(rt, a(1), &b);
            V::Void
        }
        _ => return None,
    })
}

/// `new X(...)` for the classes here; a mod's own stream's `super(...)` too.
fn init(rt: &mut Rt, owner: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    match owner {
        DATA_IN | GZIP_IN | INFLATER_IN => {
            let inner = a(1);
            let taken = drain(rt, inner).unwrap_or_default();
            let bytes = match owner {
                GZIP_IN => gunzip(&taken),
                INFLATER_IN => unzlib(&taken),
                _ => Ok(taken),
            };
            match bytes {
                Ok(b) => {
                    crate::resources::become_bytes(rt, this, b);
                    rt.put_field(this, "$in", inner);
                }
                Err((class, message)) => throw(rt, class, &message),
            }
        }
        FILE_IN if !desc.contains("FileDescriptor") => {
            let p = path_text(rt, a(1))?;
            let disk = crate::files::on_disk(rt, &p);
            match std::fs::read(&disk) {
                Ok(b) => crate::resources::become_bytes(rt, this, b),
                Err(e) => not_found(rt, &p, &e),
            }
        }
        // `new FileOutputStream(file[, append])`: made (emptied unless appending) now.
        FILE_OUT if !desc.contains("FileDescriptor") => {
            let p = path_text(rt, a(1))?;
            let disk = crate::files::on_disk(rt, &p);
            let append = desc.ends_with("Z)V") && a(2).i() != 0;
            match std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .append(append)
                .truncate(!append)
                .open(&disk)
            {
                Ok(_) => {
                    let s = rt.string(&disk.to_string_lossy());
                    rt.put_field(this, "$path", s);
                }
                Err(e) => not_found(rt, &p, &e),
            }
        }
        BYTES_OUT => crate::resources::become_bytes(rt, this, Vec::new()),
        FILTER_OUT | BUFFERED_OUT | DATA_OUT => rt.put_field(this, "$out", a(1)),
        GZIP_OUT | DEFLATER_OUT => {
            crate::resources::become_bytes(rt, this, Vec::new());
            rt.put_field(this, "$out", a(1));
        }
        // A mod's own stream's `super()`.
        OUTPUT_STREAM if desc == "()V" => {}
        _ => return None,
    }
    Some(V::Void)
}

// `NbtIo`: a file's NBT is its root compound, unnamed (type, an empty name, payload); a
// compressed one is that gzipped. Reading from or writing to a stream closes it, as `NbtIo`'s
// try-with-resources does.

/// The root compound of an NBT file's bytes (`NbtIo.read(DataInput)`), or the message of the
/// `IOException` Java throws.
pub(crate) fn root_compound(bytes: &[u8]) -> Result<BTreeMap<String, nbt::Nbt>, &'static str> {
    match nbt::Reader::new(bytes).unnamed() {
        Some(Some(nbt::Nbt::Compound(c))) => Ok(c),
        Some(_) => Err("Root tag must be a named compound tag"),
        None => Err("Malformed NBT"),
    }
}

/// [`root_compound`] of a gzipped file (`NbtIo.readCompressed`).
pub(crate) fn compressed_root(
    packed: &[u8],
) -> Result<BTreeMap<String, nbt::Nbt>, (&'static str, String)> {
    let raw = gunzip(packed)?;
    root_compound(&raw).map_err(|m| (IO_EXCEPTION, m.to_owned()))
}

/// A compound tag's file bytes (`NbtIo.write(CompoundTag, DataOutput)`).
fn file_bytes(rt: &Rt, tag: V) -> Vec<u8> {
    let mut out = Vec::new();
    nbt::write_unnamed(&mut out, &nbt::Nbt::Compound(nbt::from_runtime(rt, tag)));
    out
}

/// The compound read from `bytes`, as a `CompoundTag` (null, with the exception thrown, when it
/// isn't one).
fn compound_or_throw(
    rt: &mut Rt,
    read: Result<BTreeMap<String, nbt::Nbt>, (&'static str, String)>,
) -> V {
    match read {
        Ok(c) => nbt::to_runtime(rt, &c),
        Err((class, message)) => {
            throw(rt, class, &message);
            V::NULL
        }
    }
}

/// Reads with `read` at data input `input`'s reading position, moving it past what was read.
fn read_at<T>(
    rt: &mut Rt,
    input: V,
    read: impl FnOnce(&mut nbt::Reader) -> Option<T>,
) -> Option<T> {
    if input.r() == 0 {
        rt.throw_new(NPE);
        return None;
    }
    let got = match rt.heap.get_mut(input.r() as usize).map(|o| &mut o.data) {
        Some(Data::Bytes(b, at)) => {
            let mut r = nbt::Reader::new(b.get(*at..).unwrap_or_default());
            let got = read(&mut r);
            *at += r.at;
            got
        }
        _ => None,
    };
    if got.is_none() && rt.thrown.is_none() {
        throw(rt, IO_EXCEPTION, "Malformed NBT");
    }
    got
}

/// A file's bytes (`Files.newInputStream`), or None with Java's exception thrown.
fn read_path(rt: &mut Rt, path: V) -> Option<Vec<u8>> {
    let p = path_text(rt, path)?;
    let disk = crate::files::on_disk(rt, &p);
    match std::fs::read(&disk) {
        Ok(b) => Some(b),
        Err(e) => {
            crate::files::throw_io(rt, &e, &p);
            None
        }
    }
}

/// Writes a file whole (`Files.newOutputStream(path)`: made, or emptied first).
fn write_path(rt: &mut Rt, path: V, bytes: &[u8]) {
    let Some(p) = path_text(rt, path) else { return };
    let disk = crate::files::on_disk(rt, &p);
    if let Err(e) = std::fs::write(&disk, bytes) {
        crate::files::throw_io(rt, &e, &p);
    }
}

fn nbt_io(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let first = desc
        .get(1..)
        .and_then(|d| d.split(';').next())
        .unwrap_or("");
    Some(match (name, first) {
        // `read(path)`: null when there's no file.
        ("read", "Ljava/nio/file/Path") => {
            let Some(p) = path_text(rt, a(0)) else {
                return Some(V::NULL);
            };
            if !crate::files::on_disk(rt, &p).exists() {
                return Some(V::NULL);
            }
            let Some(bytes) = read_path(rt, a(0)) else {
                return Some(V::NULL);
            };
            compound_or_throw(
                rt,
                root_compound(&bytes).map_err(|m| (IO_EXCEPTION, m.to_owned())),
            )
        }
        ("readCompressed", "Ljava/nio/file/Path") => {
            let Some(bytes) = read_path(rt, a(0)) else {
                return Some(V::NULL);
            };
            compound_or_throw(rt, compressed_root(&bytes))
        }
        ("readCompressed", "Ljava/io/InputStream") => {
            if a(0).r() == 0 {
                rt.throw_new(NPE);
                return Some(V::NULL);
            }
            let bytes = drain(rt, a(0)).unwrap_or_default();
            let tag = compound_or_throw(rt, compressed_root(&bytes));
            close_in(rt, a(0));
            tag
        }
        ("read", "Ljava/io/DataInput") => match read_at(rt, a(0), |r| r.unnamed()) {
            Some(Some(nbt::Nbt::Compound(c))) => nbt::to_runtime(rt, &c),
            Some(_) => {
                throw(rt, IO_EXCEPTION, "Root tag must be a named compound tag");
                V::NULL
            }
            None => V::NULL,
        },
        // `readAnyTag(input, accounter)`: `EndTag` for none.
        ("readAnyTag", _) => match read_at(rt, a(0), |r| r.any()) {
            Some(Some(n)) => nbt::tag_of(rt, &n),
            Some(None) => crate::ops::end(rt),
            None => V::NULL,
        },
        ("write", "Lnet/minecraft/nbt/CompoundTag") => {
            let bytes = file_bytes(rt, a(0));
            if desc.contains("Ljava/nio/file/Path;") {
                write_path(rt, a(1), &bytes);
            } else {
                write_to(rt, a(1), &bytes);
            }
            V::Void
        }
        ("writeCompressed", "Lnet/minecraft/nbt/CompoundTag") => {
            let packed = gzip(&file_bytes(rt, a(0)));
            if desc.contains("Ljava/nio/file/Path;") {
                write_path(rt, a(1), &packed);
            } else {
                write_to(rt, a(1), &packed);
                if rt.thrown.is_none() {
                    close_out(rt, a(1));
                }
            }
            V::Void
        }
        // `writeAnyTag(tag, output)`: its type, then its payload; `writeUnnamedTag` puts an empty
        // name between. `EndTag` is its type alone.
        ("writeAnyTag" | "writeUnnamedTag", _) => {
            let n = nbt::of_tag(rt, a(0));
            let mut out = Vec::new();
            match &n {
                Some(n) if name == "writeUnnamedTag" => nbt::write_unnamed(&mut out, n),
                _ => nbt::write_any(&mut out, n.as_ref()),
            }
            write_to(rt, a(1), &out);
            V::Void
        }
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == NBT_IO {
        return nbt_io(rt, name, desc, args);
    }
    match (owner, name) {
        (OUTPUT_STREAM, "nullOutputStream") => return Some(rt.new_object(OUTPUT_STREAM)),
        (INPUT_STREAM, "nullInputStream") => {
            return Some(crate::resources::input_stream(rt, Vec::new()));
        }
        // `DataInputStream.readUTF(input)`, the static one.
        (DATA_IN, "readUTF") if desc.starts_with("(Ljava/io/DataInput;)") => {
            return Some(read_utf(rt, a(0)));
        }
        // `transferTo(out)`: the rest of this stream, written to `out`.
        (_, "transferTo") => {
            let bytes = drain(rt, a(0)).unwrap_or_default();
            write_to(rt, a(1), &bytes);
            return Some(V::J(bytes.len() as i64));
        }
        (_, "<init>") => return init(rt, owner, desc, args),
        _ => {}
    }
    let class = base(rt, a(0));
    let closing = matches!(owner, CLOSEABLE | AUTO_CLOSEABLE);
    if out_class(owner) || matches!(owner, DATA_OUTPUT | FLUSHABLE) || (closing && out_class(class))
    {
        return out_call(rt, class, name, desc, args);
    }
    if in_class(owner) || owner == DATA_INPUT || (closing && in_class(class)) {
        return in_call(rt, name, desc, args);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::Nbt;

    fn call(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let k = rt.static_key(key);
        api::call(rt, k, args)
    }

    fn new(rt: &mut Rt, class: &'static str, desc: &str, args: &[V]) -> V {
        let o = rt.new_object(class);
        let mut all = vec![o];
        all.extend_from_slice(args);
        call(rt, &format!("{class}.<init>.{desc}"), &all);
        assert!(rt.thrown.is_none(), "new {class}{desc}");
        o
    }

    /// The class of the exception thrown, taken.
    fn thrown(rt: &mut Rt) -> &'static str {
        let e = rt.thrown.take().expect("an exception");
        rt.class_of(e)
    }

    fn bytes_out(rt: &mut Rt, out: V) -> Vec<u8> {
        let arr = call(rt, "java/io/ByteArrayOutputStream.toByteArray.()[B", &[out]);
        crate::resources::array_bytes(rt, arr)
    }

    /// A folder of its own for one test, as the game's.
    fn game(rt: &mut Rt, test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-compat-streams-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let v = rt.string(&dir.to_string_lossy());
        rt.statics.insert(crate::files::GAME_DIR, v);
        dir
    }

    fn schematic() -> BTreeMap<String, Nbt> {
        let mut block = BTreeMap::new();
        block.insert(
            "pos".to_owned(),
            Nbt::List(vec![Nbt::Int(1), Nbt::Int(2), Nbt::Int(3)]),
        );
        block.insert("state".to_owned(), Nbt::Int(0));
        let mut root = BTreeMap::new();
        root.insert("blocks".to_owned(), Nbt::List(vec![Nbt::Compound(block)]));
        root.insert("size".to_owned(), Nbt::IntArray(vec![3, 4, 5]));
        root.insert("author".to_owned(), Nbt::String("Zoë \u{0} 😀".to_owned()));
        root.insert("DataVersion".to_owned(), Nbt::Int(3955));
        root.insert("seed".to_owned(), Nbt::Long(-7));
        root.insert("scale".to_owned(), Nbt::Double(0.5));
        root.insert("flag".to_owned(), Nbt::Byte(1));
        root
    }

    #[test]
    fn a_schematic_written_compressed_reads_back_through_javas_streams() {
        let mut rt = Rt::new();
        let dir = game(&mut rt, "schematic");
        let tag = nbt::to_runtime(&mut rt, &schematic());
        let name = rt.string("house.nbt");
        let path = call(
            &mut rt,
            "java/nio/file/Paths.get.(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
            &[name, V::NULL],
        );
        // Create's `SchematicExport`: `NbtIo.writeCompressed(tag, Files.newOutputStream(file))`.
        let none = rt.new_array(0, V::NULL);
        let out = call(
            &mut rt,
            "java/nio/file/Files.newOutputStream.(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/OutputStream;",
            &[path, none],
        );
        call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.writeCompressed.(Lnet/minecraft/nbt/CompoundTag;Ljava/io/OutputStream;)V",
            &[tag, out],
        );
        assert!(rt.thrown.is_none());
        // The file is gzip's, and inflates to the unnamed root compound NBT's spec gives.
        let packed = std::fs::read(dir.join("house.nbt")).unwrap();
        assert_eq!(packed[..2], [0x1f, 0x8b]);
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(&packed[..])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw[..3], [10, 0, 0], "a compound, named \"\"");
        // "author" (type 8): its text in Java's modified UTF-8 (NUL as C0 80, 😀 as two surrogates).
        let author = [
            &[8, 0, 6][..],
            b"author",
            &[0, 14],
            "Zoë ".as_bytes(),
            &[0xc0, 0x80, b' ', 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80],
        ]
        .concat();
        assert!(raw.windows(author.len()).any(|w| w == author), "{raw:?}");
        // Writing closed the stream, as `NbtIo`'s try-with-resources does.
        call(&mut rt, "java/io/OutputStream.write.(I)V", &[out, V::I(1)]);
        assert_eq!(thrown(&mut rt), IO_EXCEPTION, "closed");

        // Create's `SchematicItem.loadSchematic`: through `GZIPInputStream` and `DataInputStream`.
        let file_in = call(
            &mut rt,
            "java/nio/file/Files.newInputStream.(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/InputStream;",
            &[path, none],
        );
        let buffered = new(
            &mut rt,
            "java/io/BufferedInputStream",
            "(Ljava/io/InputStream;)V",
            &[file_in],
        );
        let gz = new(&mut rt, GZIP_IN, "(Ljava/io/InputStream;)V", &[buffered]);
        let data = new(&mut rt, DATA_IN, "(Ljava/io/InputStream;)V", &[gz]);
        let accounter = call(
            &mut rt,
            "net/minecraft/nbt/NbtAccounter.unlimitedHeap.()Lnet/minecraft/nbt/NbtAccounter;",
            &[],
        );
        let read = call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.read.(Ljava/io/DataInput;Lnet/minecraft/nbt/NbtAccounter;)Lnet/minecraft/nbt/CompoundTag;",
            &[data, accounter],
        );
        assert!(rt.thrown.is_none());
        assert_eq!(nbt::from_runtime(&rt, read), schematic());
        call(&mut rt, "java/io/DataInputStream.close.()V", &[data]);
        // ... and by path, as `NbtIo.readCompressed(path, accounter)`.
        let read = call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.readCompressed.(Ljava/nio/file/Path;Lnet/minecraft/nbt/NbtAccounter;)Lnet/minecraft/nbt/CompoundTag;",
            &[path, accounter],
        );
        assert_eq!(nbt::from_runtime(&rt, read), schematic());

        // Uncompressed: `NbtIo.write(tag, path)` and `read(path)`; no file reads as null.
        let plain = rt.string("plain.nbt");
        let plain = call(
            &mut rt,
            "java/nio/file/Path.of.(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
            &[plain, none],
        );
        call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.write.(Lnet/minecraft/nbt/CompoundTag;Ljava/nio/file/Path;)V",
            &[tag, plain],
        );
        assert_eq!(std::fs::read(dir.join("plain.nbt")).unwrap(), raw);
        let read = call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.read.(Ljava/nio/file/Path;)Lnet/minecraft/nbt/CompoundTag;",
            &[plain],
        );
        assert_eq!(nbt::from_runtime(&rt, read), schematic());
        let gone = rt.string("gone.nbt");
        let gone = call(
            &mut rt,
            "java/nio/file/Path.of.(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
            &[gone, none],
        );
        assert_eq!(
            call(
                &mut rt,
                "net/minecraft/nbt/NbtIo.read.(Ljava/nio/file/Path;)Lnet/minecraft/nbt/CompoundTag;",
                &[gone]
            ),
            V::NULL
        );
        assert!(rt.thrown.is_none());
        call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.readCompressed.(Ljava/nio/file/Path;Lnet/minecraft/nbt/NbtAccounter;)Lnet/minecraft/nbt/CompoundTag;",
            &[gone, accounter],
        );
        assert_eq!(thrown(&mut rt), "java/nio/file/NoSuchFileException");
        // A file that isn't gzip's: `ZipException`, an `IOException`.
        call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.readCompressed.(Ljava/nio/file/Path;Lnet/minecraft/nbt/NbtAccounter;)Lnet/minecraft/nbt/CompoundTag;",
            &[plain, accounter],
        );
        let e = rt.thrown.take().unwrap();
        assert_eq!(rt.class_of(e), ZIP_EXCEPTION);
        assert!(crate::api_common::api_is_a(rt.class_of(e), IO_EXCEPTION));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn data_streams_write_and_read_javas_encodings() {
        let mut rt = Rt::new();
        let bytes = new(&mut rt, BYTES_OUT, "()V", &[]);
        let out = new(&mut rt, DATA_OUT, "(Ljava/io/OutputStream;)V", &[bytes]);
        call(
            &mut rt,
            "java/io/DataOutputStream.writeInt.(I)V",
            &[out, V::I(-2)],
        );
        call(
            &mut rt,
            "java/io/DataOutput.writeShort.(I)V",
            &[out, V::I(0x1_2345)],
        );
        let s = rt.string("é\u{0}😀");
        call(
            &mut rt,
            "java/io/DataOutputStream.writeUTF.(Ljava/lang/String;)V",
            &[out, s],
        );
        call(
            &mut rt,
            "java/io/DataOutputStream.writeLong.(J)V",
            &[out, V::J(1 << 40)],
        );
        call(
            &mut rt,
            "java/io/DataOutputStream.writeDouble.(D)V",
            &[out, V::D(1.5)],
        );
        call(
            &mut rt,
            "java/io/DataOutputStream.writeBoolean.(Z)V",
            &[out, V::I(1)],
        );
        call(
            &mut rt,
            "java/io/DataOutputStream.writeFloat.(F)V",
            &[out, V::F(-0.25)],
        );
        let written: Vec<u8> = [
            &[0xff, 0xff, 0xff, 0xfe][..],
            &[0x23, 0x45],
            &[
                0, 10, 0xc3, 0xa9, 0xc0, 0x80, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80,
            ],
            &[0, 0, 1, 0, 0, 0, 0, 0],
            &[0x3f, 0xf8, 0, 0, 0, 0, 0, 0],
            &[1],
            &[0xbe, 0x80, 0, 0],
        ]
        .concat();
        assert_eq!(bytes_out(&mut rt, bytes), written);
        assert_eq!(
            call(&mut rt, "java/io/DataOutputStream.size.()I", &[out]),
            V::I(written.len() as i32)
        );

        let arr = crate::resources::byte_array(&mut rt, &written);
        let src = new(&mut rt, "java/io/ByteArrayInputStream", "([B)V", &[arr]);
        let input = new(&mut rt, DATA_IN, "(Ljava/io/InputStream;)V", &[src]);
        assert_eq!(
            call(&mut rt, "java/io/DataInputStream.readInt.()I", &[input]),
            V::I(-2)
        );
        assert_eq!(
            call(&mut rt, "java/io/DataInput.readUnsignedShort.()I", &[input]),
            V::I(0x2345)
        );
        let s = call(
            &mut rt,
            "java/io/DataInputStream.readUTF.()Ljava/lang/String;",
            &[input],
        );
        assert_eq!(rt.str_of(s), Some("é\u{0}😀"));
        assert_eq!(
            call(&mut rt, "java/io/DataInputStream.readLong.()J", &[input]),
            V::J(1 << 40)
        );
        assert_eq!(
            call(&mut rt, "java/io/DataInputStream.readDouble.()D", &[input]),
            V::D(1.5)
        );
        assert_eq!(
            call(&mut rt, "java/io/DataInputStream.readBoolean.()Z", &[input]),
            V::I(1)
        );
        assert_eq!(
            call(&mut rt, "java/io/DataInputStream.readFloat.()F", &[input]),
            V::F(-0.25)
        );
        // At the end: `read()` gives -1, a `DataInput` read throws `EOFException`.
        assert_eq!(
            call(&mut rt, "java/io/DataInputStream.read.()I", &[input]),
            V::I(-1)
        );
        call(&mut rt, "java/io/DataInputStream.readByte.()B", &[input]);
        assert_eq!(thrown(&mut rt), EOF);

        // Gzip over a byte array: finished on `close`, read back whole.
        let packed = new(&mut rt, BYTES_OUT, "()V", &[]);
        let gz = new(&mut rt, GZIP_OUT, "(Ljava/io/OutputStream;)V", &[packed]);
        let text = rt.string("hello, ".repeat(50).as_str());
        let raw = rt.str_of(text).unwrap().as_bytes().to_vec();
        let arr = crate::resources::byte_array(&mut rt, &raw);
        call(&mut rt, "java/io/OutputStream.write.([B)V", &[gz, arr]);
        assert!(bytes_out(&mut rt, packed).is_empty(), "kept until finished");
        call(&mut rt, "java/util/zip/GZIPOutputStream.close.()V", &[gz]);
        let got = bytes_out(&mut rt, packed);
        assert!(got.len() < raw.len() && got.starts_with(&[0x1f, 0x8b]));
        assert_eq!(gunzip(&got).unwrap(), raw);
        // Not gzip's: `ZipException` when made.
        let src = new(&mut rt, "java/io/ByteArrayInputStream", "([B)V", &[arr]);
        let o = rt.new_object(GZIP_IN);
        call(
            &mut rt,
            "java/util/zip/GZIPInputStream.<init>.(Ljava/io/InputStream;)V",
            &[o, src],
        );
        let e = rt.thrown.take().unwrap();
        assert_eq!(
            (rt.class_of(e), rt.str_of(rt.get_field(e, "message"))),
            (ZIP_EXCEPTION, Some("Not in GZIP format"))
        );
    }

    /// A mod's own `OutputStream` (`test/Recorder`), writing each byte into its list `got` through
    /// its `write(int)`, closed through its `close`.
    fn recorder_vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if args
            .first()
            .is_some_and(|&v| v.r() != 0 && rt.class_of(v) == "test/Recorder")
        {
            match short {
                "write.(I)V" => {
                    let got = rt.get_field(args[0], "got");
                    crate::jutil::list_add(rt, got, args[1]);
                    return V::Void;
                }
                "close.()V" => {
                    rt.put_field(args[0], "closed", V::I(1));
                    return V::Void;
                }
                _ => {}
            }
        }
        api::call(rt, key, args)
    }

    /// A mod's own `InputStream` (`test/Counter`): `read()` gives 1, 2, 3, then the end.
    fn counter_vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if short == "read.()I"
            && args
                .first()
                .is_some_and(|&v| v.r() != 0 && rt.class_of(v) == "test/Counter")
        {
            let n = rt.get_field(args[0], "n").i();
            rt.put_field(args[0], "n", V::I(n + 1));
            return V::I(if n < 3 { n + 1 } else { -1 });
        }
        api::call(rt, key, args)
    }

    #[test]
    fn a_mods_own_input_stream_is_read_through_its_read() {
        let mut rt = Rt::new();
        rt.vcall = Some(counter_vcall);
        rt.supers.insert("test/Counter", INPUT_STREAM);
        let counter = rt.new_object("test/Counter");
        rt.heap[counter.r() as usize].cid = 8;
        rt.put_field(counter, "n", V::I(0));
        // Its `super()` leaves it its own: no bytes of the runtime's in place of its fields.
        call(&mut rt, "java/io/InputStream.<init>.()V", &[counter]);
        assert_eq!(rt.get_field(counter, "n"), V::I(0));
        // `InputStream.read(byte[])`, Java's default, reads it through `read()`.
        let buf = rt.new_array(2, V::I(0));
        assert_eq!(
            call(&mut rt, "java/io/InputStream.read.([B)I", &[counter, buf]),
            V::I(2)
        );
        assert_eq!(crate::resources::array_bytes(&rt, buf), [1, 2]);
        // A data stream around it takes the rest.
        let data = new(&mut rt, DATA_IN, "(Ljava/io/InputStream;)V", &[counter]);
        assert_eq!(
            call(
                &mut rt,
                "java/io/DataInputStream.readUnsignedByte.()I",
                &[data]
            ),
            V::I(3)
        );
        call(
            &mut rt,
            "java/io/DataInputStream.readUnsignedByte.()I",
            &[data],
        );
        assert_eq!(thrown(&mut rt), EOF);
    }

    #[test]
    fn a_mods_own_stream_gets_what_is_written_through_its_write() {
        let mut rt = Rt::new();
        rt.vcall = Some(recorder_vcall);
        rt.supers.insert("test/Recorder", OUTPUT_STREAM);
        let recorder = rt.new_object("test/Recorder");
        rt.heap[recorder.r() as usize].cid = 7;
        let got = crate::jutil::new_list(&mut rt, Vec::new());
        rt.put_field(recorder, "got", got);
        let tag = nbt::to_runtime(&mut rt, &schematic());
        call(
            &mut rt,
            "net/minecraft/nbt/NbtIo.writeCompressed.(Lnet/minecraft/nbt/CompoundTag;Ljava/io/OutputStream;)V",
            &[tag, recorder],
        );
        assert!(rt.thrown.is_none());
        let written: Vec<u8> = crate::jutil::elements(&rt, got)
            .iter()
            .map(|b| b.i() as u8)
            .collect();
        assert_eq!(compressed_root(&written).unwrap(), schematic());
        assert_eq!(
            rt.get_field(recorder, "closed"),
            V::I(1),
            "closed, as NbtIo's try-with-resources does"
        );
        // A `DataOutputStream` around it hands on each write.
        if let Some(Data::List(items)) = rt.heap.get_mut(got.r() as usize).map(|o| &mut o.data) {
            items.clear();
        }
        let out = new(&mut rt, DATA_OUT, "(Ljava/io/OutputStream;)V", &[recorder]);
        call(
            &mut rt,
            "java/io/DataOutputStream.writeShort.(I)V",
            &[out, V::I(0x0102)],
        );
        let written: Vec<i32> = crate::jutil::elements(&rt, got)
            .iter()
            .map(|b| b.i())
            .collect();
        assert_eq!(written, [1, 2]);
    }
}
