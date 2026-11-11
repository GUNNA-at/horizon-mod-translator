//! Files read and written in place, and folders gone through: `FileChannel` (and what
//! `Files.newByteChannel` opens), `RandomAccessFile`, a stream as a channel
//! (`Channels.newChannel`), file locks; `DirectoryStream`, `Files.walkFileTree`, `Files.find`,
//! a file's attributes, glob matchers; temporary files and folders.
//!
//! A file opened in place is an object that keeps its path, where it reads and writes next and
//! how it was opened. Each read and write opens the file on disk for itself: nothing of the
//! system's is held between calls, so a channel a mod never closes costs nothing and every
//! thread's runtime sees the same file. (A map mod keeps its chunks in such files and writes
//! each "until the buffer is empty": unanswered, that loop never ended, and the game's thread
//! with it.)
use crate::{Data, Rt, V, api};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const FILE_CHANNEL: &str = "java/nio/channels/FileChannel";
const SEEKABLE: &str = "java/nio/channels/SeekableByteChannel";
const BYTE_CHANNEL: &str = "java/nio/channels/ByteChannel";
const READABLE: &str = "java/nio/channels/ReadableByteChannel";
const WRITABLE: &str = "java/nio/channels/WritableByteChannel";
const GATHERING: &str = "java/nio/channels/GatheringByteChannel";
const SCATTERING: &str = "java/nio/channels/ScatteringByteChannel";
const CHANNEL: &str = "java/nio/channels/Channel";
const INTERRUPTIBLE: &str = "java/nio/channels/spi/AbstractInterruptibleChannel";
const FILE_LOCK: &str = "java/nio/channels/FileLock";
const CHANNELS: &str = "java/nio/channels/Channels";
const RANDOM_ACCESS: &str = "java/io/RandomAccessFile";
const DESCRIPTOR: &str = "java/io/FileDescriptor";
const DIRECTORY_STREAM: &str = "java/nio/file/DirectoryStream";
const SIMPLE_VISITOR: &str = "java/nio/file/SimpleFileVisitor";
const ATTRIBUTES: &str = "java/nio/file/attribute/BasicFileAttributes";
const PATH_MATCHER: &str = "java/nio/file/PathMatcher";
const FILE_SYSTEM: &str = "java/nio/file/FileSystem";
const FILES: &str = "java/nio/file/Files";
const FILE: &str = "java/io/File";
const FILE_OUT: &str = "java/io/FileOutputStream";
const FILE_IN: &str = "java/io/FileInputStream";
const CLOSEABLE: &str = "java/io/Closeable";
const AUTO_CLOSEABLE: &str = "java/lang/AutoCloseable";

const IAE: &str = "java/lang/IllegalArgumentException";
const IO: &str = "java/io/IOException";
const CLOSED_CHANNEL: &str = "java/nio/channels/ClosedChannelException";
const NOT_READABLE: &str = "java/nio/channels/NonReadableChannelException";
const NOT_WRITABLE: &str = "java/nio/channels/NonWritableChannelException";

/// The path of a file opened in place, as the mod gave it (its object's field).
const OPENED: &str = "$file";
/// Where it reads and writes next.
const AT: &str = "$at";
/// How it was opened: [`READS`], [`WRITES`], [`APPENDS`], [`GONE_WHEN_CLOSED`].
const MODE: &str = "$mode";
const CLOSED: &str = "$closed";
const READS: i32 = 1;
const WRITES: i32 = 2;
const APPENDS: i32 = 4;
const GONE_WHEN_CLOSED: i32 = 8;
/// The stream a channel made of one reads (`Channels.newChannel(in)`), and the one it writes.
const STREAM_IN: &str = "$in";
const STREAM_OUT: &str = "$out";

const STREAM_READ: (&str, &str) = ("java/io/InputStream.read.([BII)I", "read.([BII)I");
const STREAM_CLOSE_IN: (&str, &str) = ("java/io/InputStream.close.()V", "close.()V");
const STREAM_CLOSE_OUT: (&str, &str) = ("java/io/OutputStream.close.()V", "close.()V");
const CHANNEL_WRITE: (&str, &str) = (
    "java/nio/channels/WritableByteChannel.write.(Ljava/nio/ByteBuffer;)I",
    "write.(Ljava/nio/ByteBuffer;)I",
);
const CHANNEL_READ: (&str, &str) = (
    "java/nio/channels/ReadableByteChannel.read.(Ljava/nio/ByteBuffer;)I",
    "read.(Ljava/nio/ByteBuffer;)I",
);
const VISIT_FILE: (&str, &str) = (
    "java/nio/file/SimpleFileVisitor.visitFile.(Ljava/lang/Object;Ljava/nio/file/attribute/BasicFileAttributes;)Ljava/nio/file/FileVisitResult;",
    "visitFile.(Ljava/lang/Object;Ljava/nio/file/attribute/BasicFileAttributes;)Ljava/nio/file/FileVisitResult;",
);
const VISIT_FAILED: (&str, &str) = (
    "java/nio/file/SimpleFileVisitor.visitFileFailed.(Ljava/lang/Object;Ljava/io/IOException;)Ljava/nio/file/FileVisitResult;",
    "visitFileFailed.(Ljava/lang/Object;Ljava/io/IOException;)Ljava/nio/file/FileVisitResult;",
);
const PRE_VISIT: (&str, &str) = (
    "java/nio/file/SimpleFileVisitor.preVisitDirectory.(Ljava/lang/Object;Ljava/nio/file/attribute/BasicFileAttributes;)Ljava/nio/file/FileVisitResult;",
    "preVisitDirectory.(Ljava/lang/Object;Ljava/nio/file/attribute/BasicFileAttributes;)Ljava/nio/file/FileVisitResult;",
);
const POST_VISIT: (&str, &str) = (
    "java/nio/file/SimpleFileVisitor.postVisitDirectory.(Ljava/lang/Object;Ljava/io/IOException;)Ljava/nio/file/FileVisitResult;",
    "postVisitDirectory.(Ljava/lang/Object;Ljava/io/IOException;)Ljava/nio/file/FileVisitResult;",
);
const FILTER_ACCEPT: &str = "java/nio/file/DirectoryStream$Filter.accept.(Ljava/lang/Object;)Z";
const MATCHER_TEST: &str =
    "java/util/function/BiPredicate.test.(Ljava/lang/Object;Ljava/lang/Object;)Z";

/// Whether this module answers `owner.name desc`. (`Files`, `File` and the closing interfaces
/// are other modules' first: this one takes what they leave.)
pub fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        FILE_CHANNEL
            | SEEKABLE
            | BYTE_CHANNEL
            | READABLE
            | WRITABLE
            | GATHERING
            | SCATTERING
            | CHANNEL
            | INTERRUPTIBLE
            | FILE_LOCK
            | CHANNELS
            | RANDOM_ACCESS
            | DESCRIPTOR
            | DIRECTORY_STREAM
            | SIMPLE_VISITOR
            | ATTRIBUTES
            | PATH_MATCHER
    ) || (owner == FILES
        && matches!(
            name,
            "newByteChannel"
                | "newDirectoryStream"
                | "walkFileTree"
                | "find"
                | "readAttributes"
                | "createTempFile"
                | "createTempDirectory"
        ))
        || (owner == FILE && name == "createTempFile")
        || (owner == FILE_SYSTEM && name == "getPathMatcher")
        || (matches!(owner, FILE_OUT | FILE_IN) && matches!(name, "getFD" | "getChannel"))
        || (matches!(owner, CLOSEABLE | AUTO_CLOSEABLE) && name == "close")
}

/// Whether `v` is a file opened in place (a channel onto a file, a `RandomAccessFile`).
pub(crate) fn is_file(rt: &Rt, v: V) -> bool {
    v.r() != 0 && rt.get_field(v, OPENED).r() != 0
}

fn throw(rt: &mut Rt, class: &'static str, message: &str) -> V {
    crate::files::throw(rt, class, message)
}

/// A file opened in place, as its object has it now.
struct Open {
    text: String,
    disk: std::path::PathBuf,
    at: i64,
    mode: i32,
}

/// The state of `v`, a file opened in place; a closed one throws what `closed` names and gives
/// none (a channel's `ClosedChannelException`, a `RandomAccessFile`'s "Stream Closed").
fn open_of(rt: &mut Rt, v: V) -> Option<Open> {
    let text = rt.str_of(rt.get_field(v, OPENED))?.to_owned();
    if rt.get_field(v, CLOSED).i() != 0 {
        if rt.instance_of(v, RANDOM_ACCESS) {
            throw(rt, IO, "Stream Closed");
        } else {
            rt.throw_new(CLOSED_CHANNEL);
        }
        return None;
    }
    Some(Open {
        disk: crate::files::on_disk(rt, &text),
        text,
        at: rt.get_field(v, AT).j(),
        mode: rt.get_field(v, MODE).i(),
    })
}

/// Up to `n` bytes of the file from `at`: fewer where it ends, none past its end.
fn read_at(disk: &Path, at: i64, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(disk)?;
    file.seek(SeekFrom::Start(at.max(0) as u64))?;
    let mut out = Vec::with_capacity(n.min(1 << 16));
    file.take(n as u64).read_to_end(&mut out)?;
    Ok(out)
}

/// Writes `bytes` into the file from `at`; one written past its end grows to there, the gap
/// zeros.
fn write_at(disk: &Path, at: i64, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(disk)?;
    file.seek(SeekFrom::Start(at.max(0) as u64))?;
    file.write_all(bytes)
}

fn length(disk: &Path) -> std::io::Result<i64> {
    Ok(disk.metadata()?.len() as i64)
}

/// Reads up to `n` bytes where `o` reads next, and moves it past them (when `moves`). None at
/// the file's end; a thrown error for a file that cannot be read.
fn take_bytes(rt: &mut Rt, this: V, o: &Open, at: i64, n: usize, moves: bool) -> Option<Vec<u8>> {
    match read_at(&o.disk, at, n) {
        Ok(bytes) if bytes.is_empty() && n > 0 => None,
        Ok(bytes) => {
            if moves {
                rt.put_field(this, AT, V::J(at + bytes.len() as i64));
            }
            Some(bytes)
        }
        Err(e) => {
            crate::files::throw_io(rt, &e, &o.text);
            None
        }
    }
}

/// Writes `bytes` where `o` writes next (its end, when it appends), and moves it past them.
fn put_bytes(rt: &mut Rt, this: V, o: &Open, bytes: &[u8]) {
    let at = if o.mode & APPENDS != 0 {
        length(&o.disk).unwrap_or(o.at)
    } else {
        o.at
    };
    match write_at(&o.disk, at, bytes) {
        Ok(()) => rt.put_field(this, AT, V::J(at + bytes.len() as i64)),
        Err(e) => {
            crate::files::throw_io(rt, &e, &o.text);
        }
    }
}

/// `FileChannel.open(path, options...)`, `Files.newByteChannel`: the file opened as Java's
/// `UnixChannelFactory` opens it. No `READ` or `WRITE`: to read, or to write with `APPEND`. It
/// is made only when written (`CREATE`, `CREATE_NEW`), emptied only when written
/// (`TRUNCATE_EXISTING`).
fn open(rt: &mut Rt, text: &str, options: V) -> V {
    let has = |n: &str| crate::files::has_option(rt, options, n);
    let (mut reads, mut writes, appends) = (has("READ"), has("WRITE"), has("APPEND"));
    let (truncates, creates, creates_new) =
        (has("TRUNCATE_EXISTING"), has("CREATE"), has("CREATE_NEW"));
    let gone_when_closed = has("DELETE_ON_CLOSE");
    if !reads && !writes {
        if appends {
            writes = true;
        } else {
            reads = true;
        }
    }
    if reads && appends {
        return throw(rt, IAE, "READ + APPEND not allowed");
    }
    if appends && truncates {
        return throw(rt, IAE, "APPEND + TRUNCATE_EXISTING not allowed");
    }
    let disk = crate::files::on_disk(rt, text);
    let mut o = std::fs::OpenOptions::new();
    o.read(reads);
    if writes {
        o.write(true).truncate(truncates);
        if creates_new {
            o.create_new(true);
        } else if creates {
            o.create(true);
        }
    }
    if let Err(e) = o.open(&disk) {
        return crate::files::throw_io(rt, &e, text);
    }
    let mut mode = 0;
    for (on, bit) in [
        (reads, READS),
        (writes, WRITES),
        (appends, APPENDS),
        (gone_when_closed, GONE_WHEN_CLOSED),
    ] {
        if on {
            mode |= bit;
        }
    }
    opened(rt, FILE_CHANNEL, text, mode)
}

/// A new object of `class` onto the file at `text`, opened as `mode` says.
fn opened(rt: &mut Rt, class: &'static str, text: &str, mode: i32) -> V {
    let c = rt.new_object(class);
    set_open(rt, c, text, mode);
    c
}

fn set_open(rt: &mut Rt, this: V, text: &str, mode: i32) {
    let path = rt.string(text);
    rt.put_field(this, OPENED, path);
    rt.put_field(this, AT, V::J(0));
    rt.put_field(this, MODE, V::I(mode));
    rt.put_field(this, CLOSED, V::I(0));
}

/// Closes file `this`: one opened to be gone when closed is deleted. (The JDK unlinks such a
/// file as it opens it, on Unix, and goes on writing to what it holds; here it is there until
/// it is closed.)
fn close(rt: &mut Rt, this: V) {
    if rt.get_field(this, CLOSED).i() != 0 {
        return;
    }
    rt.put_field(this, CLOSED, V::I(1));
    if rt.get_field(this, MODE).i() & GONE_WHEN_CLOSED != 0
        && let Some(text) = rt.str_of(rt.get_field(this, OPENED)).map(str::to_owned)
    {
        let _ = std::fs::remove_file(crate::files::on_disk(rt, &text));
    }
}

/// The buffers of a scattering read's or a gathering write's arguments: the array's, or its
/// `length` from `offset`.
fn buffers(rt: &Rt, array: V, desc: &str, from: V, count: V) -> Vec<V> {
    let all = crate::jutil::elements(rt, array);
    if desc.starts_with("([Ljava/nio/ByteBuffer;II)") {
        let from = from.i().max(0) as usize;
        all.into_iter()
            .skip(from)
            .take(count.i().max(0) as usize)
            .collect()
    } else {
        all
    }
}

/// A channel's own calls on `this`, a file opened in place.
fn channel_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    match name {
        "isOpen" => return Some(V::I(i32::from(rt.get_field(this, CLOSED).i() == 0))),
        "close" => {
            close(rt, this);
            return Some(V::Void);
        }
        _ => {}
    }
    let Some(o) = open_of(rt, this) else {
        return Some(V::Void);
    };
    let unreadable = |rt: &mut Rt| {
        rt.throw_new(NOT_READABLE);
        Some(V::I(0))
    };
    let unwritable = |rt: &mut Rt| {
        rt.throw_new(NOT_WRITABLE);
        Some(V::I(0))
    };
    Some(match (name, desc) {
        ("size", "()J") => match length(&o.disk) {
            Ok(n) => V::J(n),
            Err(e) => crate::files::throw_io(rt, &e, &o.text),
        },
        // (One that appends is at its file's end, as `FileChannelImpl.position` has it.)
        ("position", "()J") => V::J(if o.mode & APPENDS != 0 {
            length(&o.disk).unwrap_or(o.at)
        } else {
            o.at
        }),
        ("position", _) => {
            if a(1).j() < 0 {
                rt.throw_new(IAE);
                return Some(V::NULL);
            }
            rt.put_field(this, AT, V::J(a(1).j()));
            this
        }
        ("read", "(Ljava/nio/ByteBuffer;)I" | "(Ljava/nio/ByteBuffer;J)I") => {
            let placed = desc.ends_with("J)I");
            if placed && a(2).j() < 0 {
                return Some(throw(rt, IAE, "Negative position"));
            }
            if o.mode & READS == 0 {
                return unreadable(rt);
            }
            let room = crate::nio::room(rt, a(1))?;
            let at = if placed { a(2).j() } else { o.at };
            match take_bytes(rt, this, &o, at, room, !placed) {
                Some(bytes) => {
                    crate::nio::fill(rt, a(1), &bytes);
                    V::I(bytes.len() as i32)
                }
                None => V::I(-1),
            }
        }
        ("write", "(Ljava/nio/ByteBuffer;)I" | "(Ljava/nio/ByteBuffer;J)I") => {
            let placed = desc.ends_with("J)I");
            if placed && a(2).j() < 0 {
                return Some(throw(rt, IAE, "Negative position"));
            }
            if o.mode & WRITES == 0 {
                return unwritable(rt);
            }
            let bytes = crate::nio::take_left(rt, a(1))?;
            if placed {
                if let Err(e) = write_at(&o.disk, a(2).j(), &bytes) {
                    return Some(crate::files::throw_io(rt, &e, &o.text));
                }
            } else {
                put_bytes(rt, this, &o, &bytes);
            }
            V::I(bytes.len() as i32)
        }
        // A scattering read fills its buffers in turn; a gathering write empties them in turn.
        ("read", _) if desc.starts_with("([Ljava/nio/ByteBuffer;") => {
            if o.mode & READS == 0 {
                return unreadable(rt);
            }
            let mut total = 0i64;
            let mut at = o.at;
            for b in buffers(rt, a(1), desc, a(2), a(3)) {
                let room = crate::nio::room(rt, b)?;
                let Some(bytes) = take_bytes(rt, this, &o, at, room, true) else {
                    if total == 0 && rt.thrown.is_none() {
                        total = -1;
                    }
                    break;
                };
                crate::nio::fill(rt, b, &bytes);
                at += bytes.len() as i64;
                total += bytes.len() as i64;
                if bytes.len() < room {
                    break;
                }
            }
            V::J(total)
        }
        ("write", _) if desc.starts_with("([Ljava/nio/ByteBuffer;") => {
            if o.mode & WRITES == 0 {
                return unwritable(rt);
            }
            let mut all = Vec::new();
            for b in buffers(rt, a(1), desc, a(2), a(3)) {
                all.extend(crate::nio::take_left(rt, b)?);
            }
            put_bytes(rt, this, &o, &all);
            V::J(all.len() as i64)
        }
        ("truncate", _) => {
            let size = a(1).j();
            if size < 0 {
                return Some(throw(rt, IAE, "Negative size"));
            }
            if o.mode & WRITES == 0 {
                return unwritable(rt);
            }
            let cut = length(&o.disk).and_then(|n| {
                if size < n {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(&o.disk)?
                        .set_len(size as u64)?;
                }
                Ok(())
            });
            if let Err(e) = cut {
                return Some(crate::files::throw_io(rt, &e, &o.text));
            }
            if o.at > size {
                rt.put_field(this, AT, V::J(size));
            }
            this
        }
        // (Every write is in the file when its call ends.)
        ("force", _) => V::Void,
        // `transferTo(position, count, target)`: that part of the file, written to `target`;
        // this channel stays where it is.
        ("transferTo", _) => {
            let (from, count) = (a(1).j(), a(2).j());
            if from < 0 || count < 0 {
                return Some(throw(rt, IAE, "Negative position or count"));
            }
            if o.mode & READS == 0 {
                return unreadable(rt).map(|_| V::J(0));
            }
            let bytes = match read_at(&o.disk, from, count.min(i64::from(i32::MAX)) as usize) {
                Ok(bytes) => bytes,
                Err(e) => return Some(crate::files::throw_io(rt, &e, &o.text)),
            };
            write_channel(rt, a(3), &bytes);
            V::J(bytes.len() as i64)
        }
        // `transferFrom(source, position, count)`: up to `count` bytes of `source`, written at
        // `position` (past the file's end too, which grows to there: Java 21's, where older
        // ones moved nothing); this channel stays where it is.
        ("transferFrom", _) => {
            let (to, count) = (a(2).j(), a(3).j());
            if to < 0 || count < 0 {
                return Some(throw(rt, IAE, "Negative position or count"));
            }
            if o.mode & WRITES == 0 {
                return unwritable(rt).map(|_| V::J(0));
            }
            let bytes = read_channel(rt, a(1), count.min(i64::from(i32::MAX)) as usize);
            if rt.thrown.is_some() {
                return Some(V::J(0));
            }
            if let Err(e) = write_at(&o.disk, to, &bytes) {
                return Some(crate::files::throw_io(rt, &e, &o.text));
            }
            V::J(bytes.len() as i64)
        }
        // A lock on the file, or on a part of it: always had (no other game shares the folder
        // the runtime knows of), and kept until it is released or its channel closed.
        ("lock" | "tryLock", _) => {
            let shared = desc.starts_with("(JJZ)") && a(3).i() != 0;
            if !shared && o.mode & WRITES == 0 {
                rt.throw_new(NOT_WRITABLE);
                return Some(V::NULL);
            }
            if shared && o.mode & READS == 0 {
                rt.throw_new(NOT_READABLE);
                return Some(V::NULL);
            }
            let lock = rt.new_object(FILE_LOCK);
            rt.put_field(lock, "$channel", this);
            rt.put_field(lock, "$valid", V::I(1));
            rt.put_field(lock, "$shared", V::I(i32::from(shared)));
            let (from, size) = if desc.starts_with("(JJZ)") {
                (a(1).j(), a(2).j())
            } else {
                (0, i64::MAX)
            };
            rt.put_field(lock, "$position", V::J(from));
            rt.put_field(lock, "$size", V::J(size));
            lock
        }
        // `map(mode, position, size)`: that part of the file in a buffer. One mapped to be
        // read is its bytes now; a mapping written through would have to reach the file, which
        // a buffer of the runtime's does not: refused aloud, not lost in silence.
        ("map", "(Ljava/nio/channels/FileChannel$MapMode;JJ)Ljava/nio/MappedByteBuffer;") => {
            let mode = rt
                .str_of(rt.get_field(a(1), "$name"))
                .unwrap_or("")
                .to_owned();
            if mode == "READ_WRITE" {
                return Some(throw(
                    rt,
                    IO,
                    "horizon-compat: a file mapped to be written through is not supported",
                ));
            }
            match read_at(&o.disk, a(2).j(), a(3).j().max(0) as usize) {
                Ok(mut bytes) => {
                    bytes.resize(a(3).j().max(0) as usize, 0);
                    let signed: Vec<i8> = bytes.into_iter().map(|b| b as i8).collect();
                    crate::nio::of_bytes(rt, &signed)
                }
                Err(e) => crate::files::throw_io(rt, &e, &o.text),
            }
        }
        _ => return None,
    })
}

/// Writes `bytes` to channel `target` as it takes them: a file's where it writes next, a
/// stream's channel into its stream, a mod's own through its `write(ByteBuffer)`.
fn write_channel(rt: &mut Rt, target: V, bytes: &[u8]) {
    if is_file(rt, target) {
        if let Some(o) = open_of(rt, target) {
            put_bytes(rt, target, &o, bytes);
        }
        return;
    }
    let out = rt.get_field(target, STREAM_OUT);
    if out.r() != 0 {
        crate::streams::write_to(rt, out, bytes);
        return;
    }
    let signed: Vec<i8> = bytes.iter().map(|&b| b as i8).collect();
    let buffer = crate::nio::of_bytes(rt, &signed);
    // (A channel may take a part at a time: asked until the buffer is empty, as its callers do.)
    for _ in 0..bytes.len().max(1) {
        if crate::nio::room(rt, buffer).unwrap_or(0) == 0 || rt.thrown.is_some() {
            break;
        }
        if api::virtual_call(rt, CHANNEL_WRITE, &[target, buffer]).i() <= 0 {
            break;
        }
    }
}

/// Up to `n` bytes read from channel `source` as it gives them.
fn read_channel(rt: &mut Rt, source: V, n: usize) -> Vec<u8> {
    if is_file(rt, source) {
        return match open_of(rt, source) {
            Some(o) => take_bytes(rt, source, &o, o.at, n, true).unwrap_or_default(),
            None => Vec::new(),
        };
    }
    let stream = rt.get_field(source, STREAM_IN);
    if stream.r() != 0 {
        return read_stream(rt, stream, n).unwrap_or_default();
    }
    let buffer = crate::nio::allocate(rt, n.min(1 << 20) as i32);
    let mut out = Vec::new();
    while out.len() < n {
        let got = api::virtual_call(rt, CHANNEL_READ, &[source, buffer]).i();
        if got <= 0 || rt.thrown.is_some() {
            break;
        }
        out.extend(crate::nio::take_put(rt, buffer));
    }
    out.truncate(n);
    out
}

/// Up to `n` bytes of stream `stream` through its `read(byte[], int, int)`; None at its end.
fn read_stream(rt: &mut Rt, stream: V, n: usize) -> Option<Vec<u8>> {
    let array = rt.new_array(n as i32, V::I(0));
    let got = api::virtual_call(rt, STREAM_READ, &[stream, array, V::I(0), V::I(n as i32)]).i();
    if got < 0 || rt.thrown.is_some() || (got == 0 && n > 0) {
        return None;
    }
    Some((0..got).map(|i| rt.array_get(array, i).i() as u8).collect())
}

/// A channel over a stream (`Channels.newChannel`): its `read`, `write`, `close`, `isOpen`.
fn stream_channel_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let (input, output) = (
        rt.get_field(this, STREAM_IN),
        rt.get_field(this, STREAM_OUT),
    );
    if input.r() == 0 && output.r() == 0 {
        return None;
    }
    let closed = rt.get_field(this, CLOSED).i() != 0;
    Some(match (name, desc) {
        ("isOpen", _) => V::I(i32::from(!closed)),
        ("close", _) => {
            if !closed {
                rt.put_field(this, CLOSED, V::I(1));
                if input.r() != 0 {
                    api::virtual_call(rt, STREAM_CLOSE_IN, &[input]);
                } else {
                    api::virtual_call(rt, STREAM_CLOSE_OUT, &[output]);
                }
            }
            V::Void
        }
        _ if closed => {
            rt.throw_new(CLOSED_CHANNEL);
            V::I(0)
        }
        ("read", "(Ljava/nio/ByteBuffer;)I") if input.r() != 0 => {
            let room = crate::nio::room(rt, a(1))?;
            if room == 0 {
                return Some(V::I(0));
            }
            match read_stream(rt, input, room) {
                Some(bytes) => {
                    crate::nio::fill(rt, a(1), &bytes);
                    V::I(bytes.len() as i32)
                }
                None => V::I(-1),
            }
        }
        ("write", "(Ljava/nio/ByteBuffer;)I") if output.r() != 0 => {
            let bytes = crate::nio::take_left(rt, a(1))?;
            crate::streams::write_to(rt, output, &bytes);
            V::I(bytes.len() as i32)
        }
        _ => return None,
    })
}

/// Java's exception for a file a `RandomAccessFile` cannot open.
fn not_found(rt: &mut Rt, text: &str, e: &std::io::Error) -> V {
    let why = match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory",
        std::io::ErrorKind::PermissionDenied => "Permission denied",
        std::io::ErrorKind::IsADirectory => "Is a directory",
        _ => "cannot be opened",
    };
    throw(
        rt,
        "java/io/FileNotFoundException",
        &format!("{text} ({why})"),
    )
}

/// `new RandomAccessFile(file, mode)`: `r` reads a file that is there; `rw` (`rws`, `rwd`)
/// reads and writes one, made when it is not.
fn random_access_init(rt: &mut Rt, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if a(1).r() == 0 {
        rt.throw_new("java/lang/NullPointerException");
        return Some(V::Void);
    }
    let text = rt.str_of(a(1))?.to_owned();
    let mode = rt.str_of(a(2)).unwrap_or("").to_owned();
    let bits = match mode.as_str() {
        "r" => READS,
        "rw" | "rws" | "rwd" => READS | WRITES,
        _ => {
            return Some(throw(
                rt,
                IAE,
                &format!(
                    "Illegal mode \"{mode}\" must be one of \"r\", \"rw\", \"rws\", or \"rwd\""
                ),
            ));
        }
    };
    let disk = crate::files::on_disk(rt, &text);
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    if bits & WRITES != 0 {
        o.write(true).create(true);
    }
    match o.open(&disk) {
        Ok(file) if file.metadata().is_ok_and(|m| m.is_dir()) => {
            let e = std::io::Error::from(std::io::ErrorKind::IsADirectory);
            Some(not_found(rt, &text, &e))
        }
        Ok(_) => {
            set_open(rt, a(0), &text, bits);
            Some(V::Void)
        }
        Err(e) => Some(not_found(rt, &text, &e)),
    }
}

/// `n` bytes where file `this` reads next, all of them or `EOFException` (`readFully`, a
/// `DataInput`'s values).
fn exactly(rt: &mut Rt, this: V, o: &Open, n: usize) -> Option<Vec<u8>> {
    let bytes = take_bytes(rt, this, o, o.at, n, true);
    match bytes {
        Some(b) if b.len() == n => Some(b),
        _ => {
            if rt.thrown.is_none() {
                rt.throw_new("java/io/EOFException");
            }
            None
        }
    }
}

/// `RandomAccessFile`'s calls on `this`: its own, and `DataInput`'s and `DataOutput`'s, which
/// read and write where the file is, most significant byte first.
pub(crate) fn data_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    if name == "close" {
        close(rt, this);
        return Some(V::Void);
    }
    let Some(o) = open_of(rt, this) else {
        return Some(V::Void);
    };
    let fixed = |name: &str| -> Option<usize> {
        Some(match name {
            "readBoolean" | "readByte" | "readUnsignedByte" => 1,
            "readShort" | "readUnsignedShort" | "readChar" => 2,
            "readInt" | "readFloat" => 4,
            "readLong" | "readDouble" => 8,
            _ => return None,
        })
    };
    if let Some(width) = fixed(name) {
        let Some(b) = exactly(rt, this, &o, width) else {
            return Some(V::I(0));
        };
        let mut wide = [0u8; 8];
        wide[8 - width..].copy_from_slice(&b);
        let bits = u64::from_be_bytes(wide);
        return Some(match name {
            "readBoolean" => V::I(i32::from(b[0] != 0)),
            "readByte" => V::I(i32::from(b[0] as i8)),
            "readUnsignedByte" => V::I(i32::from(b[0])),
            "readShort" => V::I(i32::from(bits as u16 as i16)),
            "readUnsignedShort" | "readChar" => V::I(i32::from(bits as u16)),
            "readInt" => V::I(bits as u32 as i32),
            "readFloat" => V::F(f32::from_bits(bits as u32)),
            "readLong" => V::J(bits as i64),
            _ => V::D(f64::from_bits(bits)),
        });
    }
    let text_of = |rt: &Rt| rt.str_of(a(1)).unwrap_or("").to_owned();
    let written: Option<Vec<u8>> = match (name, desc) {
        ("write", "(I)V") | ("writeByte" | "writeBoolean", _) => Some(vec![a(1).i() as u8]),
        ("writeShort" | "writeChar", _) => Some((a(1).i() as u16).to_be_bytes().to_vec()),
        ("writeInt", _) => Some(a(1).i().to_be_bytes().to_vec()),
        ("writeLong", _) => Some(a(1).j().to_be_bytes().to_vec()),
        ("writeFloat", _) => Some(a(1).f().to_bits().to_be_bytes().to_vec()),
        ("writeDouble", _) => Some(a(1).d().to_bits().to_be_bytes().to_vec()),
        ("write", "([B)V") => Some(crate::resources::array_bytes(rt, a(1))),
        ("write", "([BII)V") => {
            let all = crate::resources::array_bytes(rt, a(1));
            let (from, len) = (a(2).i(), a(3).i());
            if from < 0 || len < 0 || from as usize + len as usize > all.len() {
                rt.throw_new("java/lang/IndexOutOfBoundsException");
                return Some(V::Void);
            }
            Some(all[from as usize..(from + len) as usize].to_vec())
        }
        // `writeBytes(String)`: each char's low byte; `writeChars`: each char's two.
        ("writeBytes", _) => Some(text_of(rt).encode_utf16().map(|c| c as u8).collect()),
        ("writeChars", _) => Some(
            text_of(rt)
                .encode_utf16()
                .flat_map(u16::to_be_bytes)
                .collect(),
        ),
        ("writeUTF", _) => {
            let b = crate::nbt::mutf8(&text_of(rt));
            if b.len() > usize::from(u16::MAX) {
                return Some(throw(
                    rt,
                    "java/io/UTFDataFormatException",
                    &format!("encoded string too long: {} bytes", b.len()),
                ));
            }
            let mut out = (b.len() as u16).to_be_bytes().to_vec();
            out.extend_from_slice(&b);
            Some(out)
        }
        _ => None,
    };
    if let Some(bytes) = written {
        if o.mode & WRITES == 0 {
            return Some(throw(rt, IO, "Bad file descriptor"));
        }
        put_bytes(rt, this, &o, &bytes);
        return Some(V::Void);
    }
    Some(match (name, desc) {
        ("length", _) => match length(&o.disk) {
            Ok(n) => V::J(n),
            Err(e) => crate::files::throw_io(rt, &e, &o.text),
        },
        ("getFilePointer", _) => V::J(o.at),
        ("seek", _) => {
            if a(1).j() < 0 {
                return Some(throw(rt, IO, "Negative seek offset"));
            }
            rt.put_field(this, AT, V::J(a(1).j()));
            V::Void
        }
        // `setLength(n)`: cut to there (what reads next past it comes back to it), or grown
        // with zeros.
        ("setLength", _) => {
            if o.mode & WRITES == 0 {
                return Some(throw(rt, IO, "Bad file descriptor"));
            }
            let size = a(1).j().max(0);
            let set = std::fs::OpenOptions::new()
                .write(true)
                .open(&o.disk)
                .and_then(|f| f.set_len(size as u64));
            if let Err(e) = set {
                return Some(crate::files::throw_io(rt, &e, &o.text));
            }
            if o.at > size {
                rt.put_field(this, AT, V::J(size));
            }
            V::Void
        }
        // `skipBytes(n)`: on by `n`, to the file's end at most. (One already past its end
        // comes back to it and tells a negative count, as Java's does.)
        ("skipBytes", _) => {
            if a(1).i() <= 0 {
                return Some(V::I(0));
            }
            let to = (o.at + i64::from(a(1).i())).min(length(&o.disk).unwrap_or(o.at));
            rt.put_field(this, AT, V::J(to));
            V::I((to - o.at) as i32)
        }
        ("read", "()I") => match take_bytes(rt, this, &o, o.at, 1, true) {
            Some(b) => V::I(i32::from(b[0])),
            None => V::I(-1),
        },
        ("read", "([B)I" | "([BII)I") => {
            let size = rt.array_len(a(1));
            let (from, len) = if desc == "([BII)I" {
                (a(2).i(), a(3).i())
            } else {
                (0, size)
            };
            if from < 0 || len < 0 || from > size - len {
                rt.throw_new("java/lang/IndexOutOfBoundsException");
                return Some(V::I(0));
            }
            if len == 0 {
                return Some(V::I(0));
            }
            match take_bytes(rt, this, &o, o.at, len as usize, true) {
                Some(bytes) => {
                    for (i, b) in bytes.iter().enumerate() {
                        rt.array_set(a(1), from + i as i32, V::I(i32::from(*b as i8)));
                    }
                    V::I(bytes.len() as i32)
                }
                None => V::I(-1),
            }
        }
        ("readFully", "([B)V" | "([BII)V") => {
            let size = rt.array_len(a(1));
            let (from, len) = if desc == "([BII)V" {
                (a(2).i(), a(3).i())
            } else {
                (0, size)
            };
            if from < 0 || len < 0 || from > size - len {
                rt.throw_new("java/lang/IndexOutOfBoundsException");
                return Some(V::Void);
            }
            if let Some(bytes) = exactly(rt, this, &o, len as usize) {
                for (i, b) in bytes.iter().enumerate() {
                    rt.array_set(a(1), from + i as i32, V::I(i32::from(*b as i8)));
                }
            }
            V::Void
        }
        ("readUTF", _) => {
            let Some(len) = exactly(rt, this, &o, 2) else {
                return Some(V::NULL);
            };
            let after = Open { at: o.at + 2, ..o };
            let n = usize::from(u16::from_be_bytes([len[0], len[1]]));
            let Some(b) = exactly(rt, this, &after, n) else {
                return Some(V::NULL);
            };
            match crate::nbt::from_mutf8(&b) {
                Some(s) => rt.string(&s),
                None => throw(rt, "java/io/UTFDataFormatException", "malformed input"),
            }
        }
        // `readLine()`: the bytes up to a line's end as chars, the end (`\n`, `\r`, `\r\n`)
        // left out; null at the file's end.
        ("readLine", _) => {
            let mut line = String::new();
            let mut at = o.at;
            let mut any = false;
            loop {
                let chunk = match read_at(&o.disk, at, 4096) {
                    Ok(c) => c,
                    Err(e) => return Some(crate::files::throw_io(rt, &e, &o.text)),
                };
                if chunk.is_empty() {
                    break;
                }
                any = true;
                let end = chunk.iter().position(|&b| b == b'\n' || b == b'\r');
                line.extend(
                    chunk[..end.unwrap_or(chunk.len())]
                        .iter()
                        .map(|&b| char::from(b)),
                );
                match end {
                    Some(i) => {
                        at += i as i64 + 1;
                        if chunk[i] == b'\r' {
                            let next = match chunk.get(i + 1) {
                                Some(&b) => Some(b),
                                None => read_at(&o.disk, at, 1)
                                    .ok()
                                    .and_then(|b| b.first().copied()),
                            };
                            if next == Some(b'\n') {
                                at += 1;
                            }
                        }
                        break;
                    }
                    None => at += chunk.len() as i64,
                }
            }
            rt.put_field(this, AT, V::J(at));
            if any { rt.string(&line) } else { V::NULL }
        }
        // The file as a channel is the file itself: one place to read and write next, as
        // Java's two share theirs.
        ("getChannel", _) => this,
        ("getFD", _) => rt.new_object(DESCRIPTOR),
        _ => return None,
    })
}

/// What Java tells of a file: its kind, size and times, as they are now.
fn attributes(rt: &mut Rt, meta: &std::fs::Metadata) -> V {
    let millis = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64)
    };
    let kind = meta.file_type();
    let o = rt.new_object(ATTRIBUTES);
    rt.put_field(o, "$dir", V::I(i32::from(kind.is_dir())));
    rt.put_field(o, "$regular", V::I(i32::from(kind.is_file())));
    rt.put_field(o, "$link", V::I(i32::from(kind.is_symlink())));
    rt.put_field(o, "$size", V::J(meta.len() as i64));
    rt.put_field(o, "$modified", V::J(millis(meta.modified())));
    rt.put_field(o, "$accessed", V::J(millis(meta.accessed())));
    // (A file system that keeps no time of birth gives the time of the last change, as Java's.)
    rt.put_field(
        o,
        "$created",
        V::J(millis(meta.created().or_else(|_| meta.modified()))),
    );
    o
}

fn attributes_call(rt: &mut Rt, name: &str, this: V) -> Option<V> {
    let flag = |rt: &Rt, f: &'static str| V::I(rt.get_field(this, f).i());
    let time = |rt: &mut Rt, f: &'static str| {
        let t = rt.new_object("java/nio/file/attribute/FileTime");
        let millis = rt.get_field(this, f);
        rt.put_field(t, "$millis", millis);
        t
    };
    Some(match name {
        "isDirectory" => flag(rt, "$dir"),
        "isRegularFile" => flag(rt, "$regular"),
        "isSymbolicLink" => flag(rt, "$link"),
        "isOther" => V::I(i32::from(
            ["$dir", "$regular", "$link"]
                .iter()
                .all(|f| rt.get_field(this, f).i() == 0),
        )),
        "size" => V::J(rt.get_field(this, "$size").j()),
        "lastModifiedTime" => time(rt, "$modified"),
        "lastAccessTime" => time(rt, "$accessed"),
        "creationTime" => time(rt, "$created"),
        "fileKey" => V::NULL,
        _ => return None,
    })
}

/// The alternatives of glob `glob`, its `{a,b}` groups written out (they do not nest).
fn alternatives(glob: &str) -> Vec<String> {
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            // (A brace in a class of characters is itself.)
            '[' => {
                i = (i + 1..chars.len())
                    .find(|&j| chars[j] == ']')
                    .unwrap_or(chars.len())
            }
            '{' => {
                let Some(end) =
                    (i + 1..chars.len()).find(|&j| chars[j] == '}' && chars[j - 1] != '\\')
                else {
                    break;
                };
                let head: String = chars[..i].iter().collect();
                let tail: String = chars[end + 1..].iter().collect();
                let mut parts = vec![String::new()];
                let mut j = i + 1;
                while j < end {
                    match chars[j] {
                        '\\' if j + 1 < end => {
                            let last = parts.last_mut().expect("one part at least");
                            last.push('\\');
                            last.push(chars[j + 1]);
                            j += 1;
                        }
                        ',' => parts.push(String::new()),
                        c => parts.last_mut().expect("one part at least").push(c),
                    }
                    j += 1;
                }
                return parts
                    .iter()
                    .flat_map(|part| alternatives(&format!("{head}{part}{tail}")))
                    .collect();
            }
            _ => i += 1,
        }
    }
    vec![glob.to_owned()]
}

/// Whether `text` matches pattern `p` of a glob without groups: `*` any run of characters of
/// one name, `**` of any names, `?` one character, `[a-z]` and `[!abc]` one of a class, `\` the
/// next character itself.
fn wild(p: &[char], text: &[char]) -> bool {
    match p.first() {
        None => text.is_empty(),
        Some('*') => {
            let deep = p.get(1) == Some(&'*');
            let rest = &p[if deep { 2 } else { 1 }..];
            (0..=text.len()).any(|n| (deep || !text[..n].contains(&'/')) && wild(rest, &text[n..]))
        }
        Some('?') => text.first().is_some_and(|&c| c != '/') && wild(&p[1..], &text[1..]),
        Some('[') => {
            let Some(end) = (1..p.len()).find(|&j| p[j] == ']') else {
                return false;
            };
            let (negated, class) = match p.get(1) {
                Some('!') => (true, &p[2..end]),
                _ => (false, &p[1..end]),
            };
            let Some(&c) = text.first().filter(|&&c| c != '/') else {
                return false;
            };
            let mut hit = false;
            let mut i = 0;
            while i < class.len() {
                if i + 2 < class.len() && class[i + 1] == '-' {
                    hit |= class[i] <= c && c <= class[i + 2];
                    i += 3;
                } else {
                    hit |= class[i] == c;
                    i += 1;
                }
            }
            hit != negated && wild(&p[end + 1..], &text[1..])
        }
        Some('\\') if p.len() > 1 => text.first() == Some(&p[1]) && wild(&p[2..], &text[1..]),
        Some(c) => text.first() == Some(c) && wild(&p[1..], &text[1..]),
    }
}

/// Whether `text` (a file's name, a path) matches `glob` as Java's `glob:` matcher has it.
fn glob_matches(glob: &str, text: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    alternatives(glob).iter().any(|p| {
        let p: Vec<char> = p.chars().collect();
        wild(&p, &text)
    })
}

/// The name of a path's last part.
fn file_name(text: &str) -> &str {
    text.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(text)
}

/// The entries of folder `text`, each as its path under the folder's, by name (Java's order
/// is the disk's, which is not fixed). A thrown error for what is no folder.
fn children(rt: &mut Rt, text: &str) -> Option<Vec<String>> {
    let disk = crate::files::on_disk(rt, text);
    if !disk.exists() {
        throw(rt, "java/nio/file/NoSuchFileException", text);
        return None;
    }
    if !disk.is_dir() {
        throw(rt, "java/nio/file/NotDirectoryException", text);
        return None;
    }
    let read = std::fs::read_dir(&disk).map(|entries| {
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    });
    match read {
        Ok(names) => Some(
            names
                .iter()
                .map(|n| crate::files::resolve(text, n))
                .collect(),
        ),
        Err(e) => {
            crate::files::throw_io(rt, &e, text);
            None
        }
    }
}

/// How a walk goes on after a visit.
#[derive(PartialEq)]
enum Walk {
    On,
    PastSiblings,
    Ended,
}

/// What a visitor answered, read as `Files.walkFileTree` reads it. No answer at all is Java's
/// `NullPointerException`; an exception of the visitor's ends the walk and goes on up.
fn answered(rt: &mut Rt, result: V) -> Walk {
    if rt.thrown.is_some() {
        return Walk::Ended;
    }
    if result.r() == 0 {
        rt.throw_new("java/lang/NullPointerException");
        return Walk::Ended;
    }
    match rt.str_of(rt.get_field(result, "$name")) {
        Some("TERMINATE") => Walk::Ended,
        Some("SKIP_SIBLINGS") => Walk::PastSiblings,
        _ => Walk::On,
    }
}

/// One step of `Files.walkFileTree`: `text` visited, at `depth` of at most `deepest`. A folder
/// is entered (`preVisitDirectory`), its entries visited, and left (`postVisitDirectory`); a
/// file, a folder at the deepest level and a link not followed are files (`visitFile`); what
/// cannot be read is told as failed (`visitFileFailed`).
fn visit(rt: &mut Rt, visitor: V, text: &str, depth: i32, deepest: i32, follows: bool) -> Walk {
    let disk = crate::files::on_disk(rt, text);
    let path = crate::files::new_path(rt, text);
    let meta = if follows {
        disk.metadata()
    } else {
        disk.symlink_metadata()
    };
    let meta = match meta {
        Ok(meta) => meta,
        Err(e) => {
            crate::files::throw_io(rt, &e, text);
            let failure = rt.thrown.take().unwrap_or_default();
            let r = api::virtual_call(rt, VISIT_FAILED, &[visitor, path, failure]);
            return answered(rt, r);
        }
    };
    let attrs = attributes(rt, &meta);
    if !meta.is_dir() || depth >= deepest {
        let r = api::virtual_call(rt, VISIT_FILE, &[visitor, path, attrs]);
        return answered(rt, r);
    }
    let Some(entries) = children(rt, text) else {
        let failure = rt.thrown.take().unwrap_or_default();
        let r = api::virtual_call(rt, VISIT_FAILED, &[visitor, path, failure]);
        return answered(rt, r);
    };
    let r = api::virtual_call(rt, PRE_VISIT, &[visitor, path, attrs]);
    let skipped =
        rt.thrown.is_none() && rt.str_of(rt.get_field(r, "$name")) == Some("SKIP_SUBTREE");
    match answered(rt, r) {
        Walk::On if !skipped => {}
        // (A folder passed over is not left either: no `postVisitDirectory`.)
        went => return went,
    }
    for entry in entries {
        match visit(rt, visitor, &entry, depth + 1, deepest, follows) {
            Walk::Ended => return Walk::Ended,
            Walk::PastSiblings => break,
            Walk::On => {}
        }
    }
    let r = api::virtual_call(rt, POST_VISIT, &[visitor, path, V::NULL]);
    // (After a folder, "past its siblings" means nothing: the walk goes on.)
    match answered(rt, r) {
        Walk::Ended => Walk::Ended,
        _ => Walk::On,
    }
}

/// A new name in `dir` for a temporary file or folder: the prefix, a number no other has, the
/// suffix.
fn temporary(dir: &Path, prefix: &str, suffix: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ (u64::from(std::process::id()) << 32);
    loop {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(n) >> 1;
        let path = dir.join(format!("{prefix}{unique}{suffix}"));
        if !path.exists() {
            return path;
        }
    }
}

/// `Files`' and `File`'s calls this module answers.
fn files_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let text_of = |rt: &Rt, v: V| rt.str_of(v).map(str::to_owned);
    Some(match (owner, name) {
        (FILES, "newByteChannel") => {
            let p = text_of(rt, a(0))?;
            open(rt, &p, a(1))
        }
        // `newDirectoryStream(dir[, glob | filter])`: the folder's entries, those whose names
        // match the glob or that the filter takes. (A list here: it is gone through once and
        // closed.)
        (FILES, "newDirectoryStream") => {
            let p = text_of(rt, a(0))?;
            let Some(mut entries) = children(rt, &p) else {
                return Some(V::NULL);
            };
            if desc.starts_with("(Ljava/nio/file/Path;Ljava/lang/String;)") {
                let glob = text_of(rt, a(1))?;
                entries.retain(|e| glob_matches(&glob, file_name(e)));
            }
            let mut paths: Vec<V> = entries
                .iter()
                .map(|e| crate::files::new_path(rt, e))
                .collect();
            if desc.starts_with("(Ljava/nio/file/Path;Ljava/nio/file/DirectoryStream$Filter;)") {
                let mut kept = Vec::new();
                for path in paths {
                    let takes = rt.call_functional(FILTER_ACCEPT, a(1), &[path]).i() != 0;
                    if rt.thrown.is_some() {
                        return Some(V::NULL);
                    }
                    if takes {
                        kept.push(path);
                    }
                }
                paths = kept;
            }
            crate::jutil::new_list(rt, paths)
        }
        // `walkFileTree(start, visitor)` and `(start, options, maxDepth, visitor)`.
        (FILES, "walkFileTree") => {
            let p = text_of(rt, a(0))?;
            let (visitor, deepest, follows) =
                if desc.starts_with("(Ljava/nio/file/Path;Ljava/util/Set;I") {
                    (
                        a(3),
                        a(2).i(),
                        crate::files::has_option(rt, a(1), "FOLLOW_LINKS"),
                    )
                } else {
                    (a(1), i32::MAX, false)
                };
            if visitor.r() == 0 {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::NULL);
            }
            if deepest < 0 {
                return Some(throw(rt, IAE, "'maxDepth' is negative"));
            }
            visit(rt, visitor, &p, 0, deepest, follows);
            a(0)
        }
        // `find(start, maxDepth, matcher, options...)`: of the tree under `start`, what the
        // matcher takes by its path and attributes.
        (FILES, "find") => {
            let p = text_of(rt, a(0))?;
            if !crate::files::on_disk(rt, &p).exists() {
                return Some(throw(rt, "java/nio/file/NoSuchFileException", &p));
            }
            let mut found = Vec::new();
            crate::files::walk(rt, &p, a(1).i(), &mut found);
            let follows = crate::files::has_option(rt, a(3), "FOLLOW_LINKS");
            let mut kept = Vec::new();
            for entry in found {
                let disk = crate::files::on_disk(rt, &entry);
                let meta = if follows {
                    disk.metadata()
                } else {
                    disk.symlink_metadata()
                };
                let Ok(meta) = meta else { continue };
                let path = crate::files::new_path(rt, &entry);
                let attrs = attributes(rt, &meta);
                let takes = rt.call_functional(MATCHER_TEST, a(2), &[path, attrs]).i() != 0;
                if rt.thrown.is_some() {
                    return Some(V::NULL);
                }
                if takes {
                    kept.push(path);
                }
            }
            rt.alloc("java/util/stream/Stream", Data::Stream(kept))
        }
        // `readAttributes(path, BasicFileAttributes.class, options...)`.
        (FILES, "readAttributes") if desc.starts_with("(Ljava/nio/file/Path;Ljava/lang/Class;") => {
            let p = text_of(rt, a(0))?;
            let disk = crate::files::on_disk(rt, &p);
            let meta = if crate::files::has_option(rt, a(2), "NOFOLLOW_LINKS") {
                disk.symlink_metadata()
            } else {
                disk.metadata()
            };
            match meta {
                Ok(meta) => attributes(rt, &meta),
                Err(e) => crate::files::throw_io(rt, &e, &p),
            }
        }
        // A temporary file or folder: in the folder given, or the system's for them.
        (FILES, "createTempFile" | "createTempDirectory") => {
            let in_dir = desc.starts_with("(Ljava/nio/file/Path;");
            let dir = if in_dir {
                let p = text_of(rt, a(0))?;
                crate::files::on_disk(rt, &p)
            } else {
                std::env::temp_dir()
            };
            let first = usize::from(in_dir);
            let prefix = text_of(rt, a(first)).unwrap_or_default();
            let made = if name == "createTempFile" {
                let suffix = text_of(rt, a(first + 1)).unwrap_or_else(|| ".tmp".to_owned());
                let path = temporary(&dir, &prefix, &suffix);
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map(|_| path)
            } else {
                let path = temporary(&dir, &prefix, "");
                std::fs::create_dir(&path).map(|()| path)
            };
            match made {
                Ok(path) => crate::files::new_path(rt, &path.to_string_lossy()),
                Err(e) => crate::files::throw_io(rt, &e, &dir.to_string_lossy()),
            }
        }
        // `File.createTempFile(prefix, suffix[, directory])`.
        (FILE, "createTempFile") => {
            let prefix = text_of(rt, a(0))?;
            if prefix.chars().count() < 3 {
                return Some(throw(
                    rt,
                    IAE,
                    &format!("Prefix string \"{prefix}\" too short: length must be at least 3"),
                ));
            }
            let suffix = text_of(rt, a(1)).unwrap_or_else(|| ".tmp".to_owned());
            let dir = match text_of(rt, a(2)) {
                Some(d) if a(2).r() != 0 => crate::files::on_disk(rt, &d),
                _ => std::env::temp_dir(),
            };
            let path = temporary(&dir, &prefix, &suffix);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => crate::files::new_file(rt, &path.to_string_lossy()),
                Err(e) => throw(rt, IO, &e.to_string()),
            }
        }
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
        (FILE_CHANNEL, "open") => {
            let p = rt.str_of(a(0))?.to_owned();
            return Some(open(rt, &p, a(1)));
        }
        (FILES | FILE, _) => return files_call(rt, owner, name, desc, args),
        (RANDOM_ACCESS, "<init>") => return random_access_init(rt, args),
        // `Channels.newChannel(stream)`: the stream as a channel; `newInputStream(channel)`
        // and `newOutputStream(channel)`: back, or a file's rest as a stream.
        (CHANNELS, "newChannel") => {
            let (class, field) = if desc.starts_with("(Ljava/io/InputStream;)") {
                (READABLE, STREAM_IN)
            } else {
                (WRITABLE, STREAM_OUT)
            };
            if a(0).r() == 0 {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::NULL);
            }
            let c = rt.new_object(class);
            rt.put_field(c, field, a(0));
            return Some(c);
        }
        (CHANNELS, "newInputStream") => {
            let stream = rt.get_field(a(0), STREAM_IN);
            if stream.r() != 0 {
                return Some(stream);
            }
            if !is_file(rt, a(0)) {
                return None;
            }
            let o = open_of(rt, a(0))?;
            let rest = take_bytes(rt, a(0), &o, o.at, usize::MAX >> 1, true).unwrap_or_default();
            return Some(crate::streams::file_input(rt, rest));
        }
        (CHANNELS, "newOutputStream") => {
            let stream = rt.get_field(a(0), STREAM_OUT);
            return (stream.r() != 0).then_some(stream);
        }
        // A matcher of paths by a glob (`getPathMatcher("glob:*.json")`).
        (FILE_SYSTEM, "getPathMatcher") => {
            let syntax = rt.str_of(a(1))?.to_owned();
            let glob = syntax.strip_prefix("glob:")?;
            let m = rt.new_object(PATH_MATCHER);
            let g = rt.string(glob);
            rt.put_field(m, "$glob", g);
            return Some(m);
        }
        (PATH_MATCHER, "matches") => {
            let glob = rt.str_of(rt.get_field(a(0), "$glob"))?.to_owned();
            let text = rt.str_of(a(1))?.to_owned();
            return Some(V::I(i32::from(glob_matches(&glob, &text))));
        }
        (ATTRIBUTES, _) => return attributes_call(rt, name, a(0)),
        // A folder's entries are a list here: closing it is nothing, going through it the
        // list's.
        (DIRECTORY_STREAM, "close") => return Some(V::Void),
        (DIRECTORY_STREAM, "iterator") => {
            return Some(api::call(
                rt,
                "java/util/List.iterator.()Ljava/util/Iterator;",
                &[a(0)],
            ));
        }
        // What a visitor does that says nothing else: goes on, and throws on what a failure
        // handed it.
        (SIMPLE_VISITOR, "<init>") => return Some(V::Void),
        (SIMPLE_VISITOR, "preVisitDirectory" | "visitFile") => {
            return crate::api_common::plain_enum(rt, "java/nio/file/FileVisitResult", "CONTINUE");
        }
        (SIMPLE_VISITOR, "visitFileFailed" | "postVisitDirectory") => {
            if a(2).r() != 0 {
                rt.thrown = Some(a(2));
                return Some(V::NULL);
            }
            return crate::api_common::plain_enum(rt, "java/nio/file/FileVisitResult", "CONTINUE");
        }
        (FILE_LOCK, _) => {
            let this = a(0);
            let channel = rt.get_field(this, "$channel");
            let valid =
                rt.get_field(this, "$valid").i() != 0 && rt.get_field(channel, CLOSED).i() == 0;
            return Some(match name {
                "release" | "close" => {
                    rt.put_field(this, "$valid", V::I(0));
                    V::Void
                }
                "isValid" => V::I(i32::from(valid)),
                "isShared" => V::I(rt.get_field(this, "$shared").i()),
                "channel" | "acquiredBy" => channel,
                "position" => V::J(rt.get_field(this, "$position").j()),
                "size" => V::J(rt.get_field(this, "$size").j()),
                _ => return None,
            });
        }
        (DESCRIPTOR, "sync") => return Some(V::Void),
        (DESCRIPTOR, "valid") => return Some(V::I(1)),
        (FILE_OUT | FILE_IN, "getFD") => return Some(rt.new_object(DESCRIPTOR)),
        // `FileOutputStream.getChannel()`: a channel onto the stream's file, at its end as the
        // stream is.
        (FILE_OUT, "getChannel") => {
            let p = rt.str_of(rt.get_field(a(0), "$path"))?.to_owned();
            return Some(opened(rt, FILE_CHANNEL, &p, WRITES | APPENDS));
        }
        _ => {}
    }
    let this = a(0);
    if is_file(rt, this) {
        if owner == RANDOM_ACCESS || rt.instance_of(this, RANDOM_ACCESS) {
            // (A `RandomAccessFile` asked as its channel answers as one.)
            if owner != RANDOM_ACCESS
                && let Some(v) = channel_call(rt, name, desc, args)
            {
                return Some(v);
            }
            return data_call(rt, name, desc, args);
        }
        return channel_call(rt, name, desc, args);
    }
    stream_channel_call(rt, name, desc, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// A runtime whose game folder is a new one of the test's.
    fn game(rt: &mut Rt, test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hz-channels-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = rt.string(&dir.to_string_lossy());
        rt.statics.insert(crate::GAME_DIR, text);
        dir
    }

    fn options(rt: &mut Rt, names: &[&'static str]) -> V {
        let array = rt.new_array(names.len() as i32, V::NULL);
        for (i, n) in names.iter().enumerate() {
            let o =
                crate::api_common::plain_enum(rt, "java/nio/file/StandardOpenOption", n).unwrap();
            rt.array_set(array, i as i32, o);
        }
        array
    }

    fn buffer(rt: &mut Rt, bytes: &[i8]) -> V {
        crate::nio::of_bytes(rt, bytes)
    }

    const OPEN: &str = "java/nio/channels/FileChannel.open.(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/nio/channels/FileChannel;";
    const WRITE_AT: &str = "java/nio/channels/FileChannel.write.(Ljava/nio/ByteBuffer;J)I";
    const READ_AT: &str = "java/nio/channels/FileChannel.read.(Ljava/nio/ByteBuffer;J)I";
    const SIZE: &str = "java/nio/channels/FileChannel.size.()J";

    // The values in these tests are Java's: JDK 21 ran the same calls on files of its own
    // (the fixture's `files` case runs them again, translated, beside the JVM).
    #[test]
    fn a_file_is_written_and_read_in_place() {
        crate::install();
        let mut rt = Rt::new();
        let dir = game(&mut rt, "in-place");
        let path = crate::files::new_path(&mut rt, "region.dat");
        let how = options(&mut rt, &["READ", "WRITE", "CREATE"]);
        let ch = c(&mut rt, OPEN, &[path, how]);
        assert!(rt.thrown.is_none());
        assert_eq!(c(&mut rt, SIZE, &[ch]), V::J(0));
        // Written past the end: the file grows to there, the gap zeros.
        let b = buffer(&mut rt, &[1, 2, 3, 4]);
        assert_eq!(c(&mut rt, WRITE_AT, &[ch, b, V::J(6)]), V::I(4));
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.hasRemaining.()Z", &[b]),
            V::I(0)
        );
        assert_eq!(c(&mut rt, SIZE, &[ch]), V::J(10));
        assert_eq!(
            std::fs::read(dir.join("region.dat")).unwrap(),
            [0, 0, 0, 0, 0, 0, 1, 2, 3, 4]
        );
        // A write at a place leaves the channel where it was.
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[ch]),
            V::J(0)
        );
        // Read back from a place: what is there, fewer at the end, -1 past it.
        let into = crate::nio::allocate(&mut rt, 8);
        assert_eq!(c(&mut rt, READ_AT, &[ch, into, V::J(5)]), V::I(5));
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.position.()I", &[into]),
            V::I(5)
        );
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[into, V::I(1)]),
            V::I(1)
        );
        let more = crate::nio::allocate(&mut rt, 8);
        assert_eq!(c(&mut rt, READ_AT, &[ch, more, V::J(10)]), V::I(-1));
        // One after another: the channel moves.
        let b = buffer(&mut rt, &[9, 8]);
        assert_eq!(
            c(
                &mut rt,
                "java/nio/channels/FileChannel.write.(Ljava/nio/ByteBuffer;)I",
                &[ch, b]
            ),
            V::I(2)
        );
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[ch]),
            V::J(2)
        );
        c(
            &mut rt,
            "java/nio/channels/FileChannel.truncate.(J)Ljava/nio/channels/FileChannel;",
            &[ch, V::J(1)],
        );
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[ch]),
            V::J(1)
        );
        assert_eq!(std::fs::read(dir.join("region.dat")).unwrap(), [9]);
        // Cut to more than it has: as it was. Put past its end: nothing to read there, and a
        // write grows the file to it.
        c(
            &mut rt,
            "java/nio/channels/FileChannel.truncate.(J)Ljava/nio/channels/FileChannel;",
            &[ch, V::J(50)],
        );
        assert_eq!(c(&mut rt, SIZE, &[ch]), V::J(1));
        c(
            &mut rt,
            "java/nio/channels/FileChannel.position.(J)Ljava/nio/channels/FileChannel;",
            &[ch, V::J(40)],
        );
        let none = crate::nio::allocate(&mut rt, 4);
        assert_eq!(
            c(
                &mut rt,
                "java/nio/channels/FileChannel.read.(Ljava/nio/ByteBuffer;)I",
                &[ch, none]
            ),
            V::I(-1)
        );
        let b = buffer(&mut rt, &[7]);
        c(
            &mut rt,
            "java/nio/channels/FileChannel.write.(Ljava/nio/ByteBuffer;)I",
            &[ch, b],
        );
        assert_eq!(c(&mut rt, SIZE, &[ch]), V::J(41));
        // A buffer with no room takes nothing, at the end too.
        let full = crate::nio::allocate(&mut rt, 0);
        assert_eq!(c(&mut rt, READ_AT, &[ch, full, V::J(41)]), V::I(0));
        // A part of it sent to another channel, and taken from one: each at the place given,
        // the channel written to or read from moved, this one where it was.
        let other = crate::files::new_path(&mut rt, "other.dat");
        let how = options(&mut rt, &["READ", "WRITE", "CREATE"]);
        let to = c(&mut rt, OPEN, &[other, how]);
        let b = buffer(&mut rt, &[1, 2, 3, 4, 5]);
        c(
            &mut rt,
            "java/nio/channels/FileChannel.write.(Ljava/nio/ByteBuffer;)I",
            &[to, b],
        );
        let t2 = crate::files::new_path(&mut rt, "t2.dat");
        let t = c(&mut rt, OPEN, &[t2, how]);
        let b = buffer(&mut rt, &[9, 9]);
        c(
            &mut rt,
            "java/nio/channels/FileChannel.write.(Ljava/nio/ByteBuffer;)I",
            &[t, b],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/nio/channels/FileChannel.transferTo.(JJLjava/nio/channels/WritableByteChannel;)J",
                &[to, V::J(1), V::J(3), t]
            ),
            V::J(3)
        );
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[to]),
            V::J(5)
        );
        assert_eq!(std::fs::read(dir.join("t2.dat")).unwrap(), [9, 9, 2, 3, 4]);
        c(
            &mut rt,
            "java/nio/channels/FileChannel.position.(J)Ljava/nio/channels/FileChannel;",
            &[to, V::J(0)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/nio/channels/FileChannel.transferFrom.(Ljava/nio/channels/ReadableByteChannel;JJ)J",
                &[t, to, V::J(1), V::J(2)]
            ),
            V::J(2)
        );
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[to]),
            V::J(2)
        );
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[t]),
            V::J(5)
        );
        assert_eq!(std::fs::read(dir.join("t2.dat")).unwrap(), [9, 1, 2, 3, 4]);
        assert_eq!(
            c(
                &mut rt,
                "java/nio/channels/FileChannel.transferFrom.(Ljava/nio/channels/ReadableByteChannel;JJ)J",
                &[t, to, V::J(99), V::J(2)]
            ),
            V::J(2)
        );
        assert_eq!(c(&mut rt, SIZE, &[t]), V::J(101));
        c(&mut rt, "java/nio/channels/FileChannel.close.()V", &[ch]);
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.isOpen.()Z", &[ch]),
            V::I(0)
        );
        c(&mut rt, SIZE, &[ch]);
        let thrown = rt.thrown.take().expect("a closed channel throws");
        assert_eq!(rt.class_of(thrown), CLOSED_CHANNEL);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_channel_is_opened_as_javas_options_say() {
        crate::install();
        let mut rt = Rt::new();
        let dir = game(&mut rt, "options");
        let path = crate::files::new_path(&mut rt, "missing.dat");
        // Not there, and not to be made: no such file.
        let how = options(&mut rt, &["READ", "WRITE"]);
        c(&mut rt, OPEN, &[path, how]);
        let thrown = rt.thrown.take().expect("a missing file throws");
        assert_eq!(rt.class_of(thrown), "java/nio/file/NoSuchFileException");
        // Opened to read, it is not written.
        std::fs::write(dir.join("there.dat"), [5, 6, 7]).unwrap();
        let path = crate::files::new_path(&mut rt, "there.dat");
        let how = options(&mut rt, &[]);
        let ch = c(&mut rt, OPEN, &[path, how]);
        let b = buffer(&mut rt, &[1]);
        c(&mut rt, WRITE_AT, &[ch, b, V::J(0)]);
        let thrown = rt
            .thrown
            .take()
            .expect("a channel that reads does not write");
        assert_eq!(rt.class_of(thrown), NOT_WRITABLE);
        // To be made anew where it is: already there.
        let how = options(&mut rt, &["WRITE", "CREATE_NEW"]);
        c(&mut rt, OPEN, &[path, how]);
        let thrown = rt
            .thrown
            .take()
            .expect("a file that is there is not made anew");
        assert_eq!(
            rt.class_of(thrown),
            "java/nio/file/FileAlreadyExistsException"
        );
        // Appending: every write at the end, wherever the channel was put.
        let how = options(&mut rt, &["APPEND"]);
        let ch = c(&mut rt, OPEN, &[path, how]);
        let b = buffer(&mut rt, &[8]);
        c(
            &mut rt,
            "java/nio/channels/FileChannel.write.(Ljava/nio/ByteBuffer;)I",
            &[ch, b],
        );
        assert_eq!(std::fs::read(dir.join("there.dat")).unwrap(), [5, 6, 7, 8]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_random_access_file_reads_what_it_wrote() {
        crate::install();
        let mut rt = Rt::new();
        let dir = game(&mut rt, "random-access");
        let raf = rt.new_object(RANDOM_ACCESS);
        let name = rt.string("index.bin");
        let mode = rt.string("rw");
        c(
            &mut rt,
            "java/io/RandomAccessFile.<init>.(Ljava/lang/String;Ljava/lang/String;)V",
            &[raf, name, mode],
        );
        assert!(rt.thrown.is_none());
        c(
            &mut rt,
            "java/io/RandomAccessFile.writeInt.(I)V",
            &[raf, V::I(0x0102_0304)],
        );
        c(
            &mut rt,
            "java/io/RandomAccessFile.writeLong.(J)V",
            &[raf, V::J(-2)],
        );
        let text = rt.string("é\u{0}z");
        c(
            &mut rt,
            "java/io/RandomAccessFile.writeUTF.(Ljava/lang/String;)V",
            &[raf, text],
        );
        // (`é` is two bytes and NUL two in Java's modified UTF-8: 2 + 2 + 2 + 1 after 12.)
        assert_eq!(
            c(&mut rt, "java/io/RandomAccessFile.length.()J", &[raf]),
            V::J(19)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/io/RandomAccessFile.getFilePointer.()J",
                &[raf]
            ),
            V::J(19)
        );
        assert_eq!(
            std::fs::read(dir.join("index.bin")).unwrap()[..12],
            [1, 2, 3, 4, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe]
        );
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(2)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/io/RandomAccessFile.readUnsignedShort.()I",
                &[raf]
            ),
            V::I(0x0304)
        );
        assert_eq!(
            c(&mut rt, "java/io/RandomAccessFile.readLong.()J", &[raf]),
            V::J(-2)
        );
        let read = c(
            &mut rt,
            "java/io/RandomAccessFile.readUTF.()Ljava/lang/String;",
            &[raf],
        );
        assert_eq!(rt.str_of(read), Some("é\u{0}z"));
        assert_eq!(
            c(&mut rt, "java/io/RandomAccessFile.read.()I", &[raf]),
            V::I(-1)
        );
        // Through `DataInput`, as a reader handed the file asks it.
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(0)],
        );
        assert_eq!(
            c(&mut rt, "java/io/DataInput.readInt.()I", &[raf]),
            V::I(0x0102_0304)
        );
        // Past the end: `EOFException`.
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(18)],
        );
        c(&mut rt, "java/io/RandomAccessFile.readInt.()I", &[raf]);
        let thrown = rt.thrown.take().expect("a read past the end throws");
        assert_eq!(rt.class_of(thrown), "java/io/EOFException");
        // (What it could read before the end is read: the file is at its end after.)
        assert_eq!(
            c(
                &mut rt,
                "java/io/RandomAccessFile.getFilePointer.()J",
                &[raf]
            ),
            V::J(19)
        );
        // Put past its end, it skips back to it, and says by how much.
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(30)],
        );
        assert_eq!(
            c(&mut rt, "java/io/RandomAccessFile.read.()I", &[raf]),
            V::I(-1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/io/RandomAccessFile.skipBytes.(I)I",
                &[raf, V::I(5)]
            ),
            V::I(-11)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/io/RandomAccessFile.getFilePointer.()J",
                &[raf]
            ),
            V::J(19)
        );
        // Cut short: what reads next comes back to the end.
        c(
            &mut rt,
            "java/io/RandomAccessFile.setLength.(J)V",
            &[raf, V::J(4)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/io/RandomAccessFile.getFilePointer.()J",
                &[raf]
            ),
            V::J(4)
        );
        // Lines end at `\n`, `\r` or the two together; none is left at the end.
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(0)],
        );
        let text = rt.string("ab\r\ncd\ne\rf");
        c(
            &mut rt,
            "java/io/RandomAccessFile.writeBytes.(Ljava/lang/String;)V",
            &[raf, text],
        );
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(0)],
        );
        for line in [Some("ab"), Some("cd"), Some("e"), Some("f"), None] {
            let read = c(
                &mut rt,
                "java/io/RandomAccessFile.readLine.()Ljava/lang/String;",
                &[raf],
            );
            assert_eq!(rt.str_of(read), line);
        }
        c(
            &mut rt,
            "java/io/RandomAccessFile.setLength.(J)V",
            &[raf, V::J(4)],
        );
        c(
            &mut rt,
            "java/io/RandomAccessFile.seek.(J)V",
            &[raf, V::J(4)],
        );
        // Its channel is the file: one place for both.
        let ch = c(
            &mut rt,
            "java/io/RandomAccessFile.getChannel.()Ljava/nio/channels/FileChannel;",
            &[raf],
        );
        assert_eq!(
            c(&mut rt, "java/nio/channels/FileChannel.position.()J", &[ch]),
            V::J(4)
        );
        assert_eq!(c(&mut rt, SIZE, &[ch]), V::J(4));
        // One that only reads is not made, and is not written.
        let missing = rt.new_object(RANDOM_ACCESS);
        let name = rt.string("none.bin");
        let mode = rt.string("r");
        c(
            &mut rt,
            "java/io/RandomAccessFile.<init>.(Ljava/lang/String;Ljava/lang/String;)V",
            &[missing, name, mode],
        );
        let thrown = rt.thrown.take().expect("a missing file is not read");
        assert_eq!(rt.class_of(thrown), "java/io/FileNotFoundException");
        let message = rt.get_field(thrown, "message");
        assert_eq!(
            rt.str_of(message),
            Some("none.bin (No such file or directory)")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    // The matches are Java's (`FileSystems.getDefault().getPathMatcher("glob:" + g)` on JDK 21).
    #[test]
    fn a_glob_matches_as_javas() {
        for (glob, name, matches) in [
            ("*.dat", "r.0.0.dat", true),
            ("*.dat", "r.0.0.dat.tmp", false),
            ("*.{dat,tmp}", "r.0.0.dat.tmp", true),
            ("*.{dat,tmp}", "r.json", false),
            ("r.?.[0-9].dat", "r.1.7.dat", true),
            ("r.?.[0-9].dat", "r.12.7.dat", false),
            ("[!a-c]*", "data", true),
            ("[!a-c]*", "book", false),
            ("*", "a/b", false),
            ("**", "a/b", true),
            ("a\\*b", "a*b", true),
            ("a\\*b", "axb", false),
            ("{a,b}{1,2}", "b2", true),
            ("", "", true),
            ("*.[ch]", "x.c", true),
            ("[a-]", "-", true),
            ("x[[]y", "x[y", true),
            ("{*.a,b?}", "bz", true),
            ("*", ".hidden", true),
            ("a,b", "a,b", true),
        ] {
            assert_eq!(glob_matches(glob, name), matches, "{glob} on {name}");
        }
    }

    #[test]
    fn a_folder_is_listed_and_walked() {
        crate::install();
        let mut rt = Rt::new();
        let dir = game(&mut rt, "walk");
        for f in ["a/one.dat", "a/two.txt", "a/deep/three.dat", "b/four.dat"] {
            std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
            std::fs::write(dir.join(f), f).unwrap();
        }
        let a = crate::files::new_path(&mut rt, "a");
        let glob = rt.string("*.{dat,json}");
        let listed = c(
            &mut rt,
            "java/nio/file/Files.newDirectoryStream.(Ljava/nio/file/Path;Ljava/lang/String;)Ljava/nio/file/DirectoryStream;",
            &[a, glob],
        );
        let names: Vec<String> = crate::jutil::elements(&rt, listed)
            .into_iter()
            .map(|p| rt.str_of(p).unwrap().to_owned())
            .collect();
        assert_eq!(names, ["a/one.dat"]);
        let all = c(
            &mut rt,
            "java/nio/file/Files.newDirectoryStream.(Ljava/nio/file/Path;)Ljava/nio/file/DirectoryStream;",
            &[a],
        );
        assert_eq!(crate::jutil::elements(&rt, all).len(), 3);
        // A visitor that says nothing goes on through the whole tree, and the walk gives its
        // start back.
        let visitor = rt.new_object(SIMPLE_VISITOR);
        let start = crate::files::new_path(&mut rt, ".");
        let back = c(
            &mut rt,
            "java/nio/file/Files.walkFileTree.(Ljava/nio/file/Path;Ljava/nio/file/FileVisitor;)Ljava/nio/file/Path;",
            &[start, visitor],
        );
        assert!(rt.thrown.is_none());
        assert_eq!(back, start);
        // A tree that is not there fails at its start: the default visitor throws what failed.
        let none = crate::files::new_path(&mut rt, "none");
        c(
            &mut rt,
            "java/nio/file/Files.walkFileTree.(Ljava/nio/file/Path;Ljava/nio/file/FileVisitor;)Ljava/nio/file/Path;",
            &[none, visitor],
        );
        let thrown = rt.thrown.take().expect("a missing start throws");
        assert_eq!(rt.class_of(thrown), "java/nio/file/NoSuchFileException");
        // A file's attributes.
        let one = crate::files::new_path(&mut rt, "a/one.dat");
        let attrs = c(
            &mut rt,
            "java/nio/file/Files.readAttributes.(Ljava/nio/file/Path;Ljava/lang/Class;[Ljava/nio/file/LinkOption;)Ljava/nio/file/attribute/BasicFileAttributes;",
            &[one, V::NULL, V::NULL],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/nio/file/attribute/BasicFileAttributes.size.()J",
                &[attrs]
            ),
            V::J(9)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/nio/file/attribute/BasicFileAttributes.isRegularFile.()Z",
                &[attrs]
            ),
            V::I(1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/nio/file/attribute/BasicFileAttributes.isDirectory.()Z",
                &[attrs]
            ),
            V::I(0)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stream_is_read_and_written_as_a_channel() {
        crate::install();
        let mut rt = Rt::new();
        let input = crate::streams::file_input(&mut rt, vec![1, 2, 3, 4, 5]);
        let ch = c(
            &mut rt,
            "java/nio/channels/Channels.newChannel.(Ljava/io/InputStream;)Ljava/nio/channels/ReadableByteChannel;",
            &[input],
        );
        let into = crate::nio::allocate(&mut rt, 3);
        const READ: &str = "java/nio/channels/ReadableByteChannel.read.(Ljava/nio/ByteBuffer;)I";
        assert_eq!(c(&mut rt, READ, &[ch, into]), V::I(3));
        // A full buffer takes nothing; an emptied one the rest; then the stream is at its end.
        assert_eq!(c(&mut rt, READ, &[ch, into]), V::I(0));
        c(
            &mut rt,
            "java/nio/ByteBuffer.clear.()Ljava/nio/ByteBuffer;",
            &[into],
        );
        assert_eq!(c(&mut rt, READ, &[ch, into]), V::I(2));
        assert_eq!(
            c(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[into, V::I(1)]),
            V::I(5)
        );
        c(
            &mut rt,
            "java/nio/ByteBuffer.clear.()Ljava/nio/ByteBuffer;",
            &[into],
        );
        assert_eq!(c(&mut rt, READ, &[ch, into]), V::I(-1));
        let out = rt.new_object("java/io/ByteArrayOutputStream");
        c(&mut rt, "java/io/ByteArrayOutputStream.<init>.()V", &[out]);
        let ch = c(
            &mut rt,
            "java/nio/channels/Channels.newChannel.(Ljava/io/OutputStream;)Ljava/nio/channels/WritableByteChannel;",
            &[out],
        );
        let from = buffer(&mut rt, &[7, 8, 9]);
        assert_eq!(
            c(
                &mut rt,
                "java/nio/channels/WritableByteChannel.write.(Ljava/nio/ByteBuffer;)I",
                &[ch, from]
            ),
            V::I(3)
        );
        assert_eq!(
            c(&mut rt, "java/io/ByteArrayOutputStream.size.()I", &[out]),
            V::I(3)
        );
    }
}
