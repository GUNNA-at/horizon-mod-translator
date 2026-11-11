//! Zip and jar files as `java.util.zip` and `java.util.jar` read and write them, and a mod's
//! own jar.
//!
//! A zip is read from its bytes: the entries its central directory lists, each stored or
//! deflated. A `ZipFile` or `JarFile` keeps its file's bytes; a `ZipInputStream` those of the
//! stream it is around, and is itself the stream of the entry it is at. A `ZipOutputStream`
//! keeps each entry it is given and writes the file as it finishes ([`crate::streams`] has
//! the stream classes' side of both).
//!
//! A translated mod's jar is not on the player's disk: its code is in the game, and its files
//! are a content mod's. A mod that opens its own jar asks for those files (JourneyMap copies
//! its themes out of it at every start), so the jar is made where FML would have it,
//! `mods/<id>.jar`, the first time a file is looked for there ([`mod_jar`]): the content mod's
//! `assets` and `data` as they lie, stored, with a manifest that says its version.
use crate::{Data, Rt, V};
use std::path::{Path, PathBuf};

const ZIP_FILE: &str = "java/util/zip/ZipFile";
const JAR_FILE: &str = "java/util/jar/JarFile";
const ZIP_ENTRY: &str = "java/util/zip/ZipEntry";
const JAR_ENTRY: &str = "java/util/jar/JarEntry";
const MANIFEST: &str = "java/util/jar/Manifest";
const ATTRIBUTES: &str = "java/util/jar/Attributes";
const ZIP_EXCEPTION: &str = "java/util/zip/ZipException";
/// Where a jar keeps its manifest.
const MANIFEST_PATH: &str = "META-INF/MANIFEST.MF";

/// The bytes of the zip a `ZipFile` or a zip stream reads (an object holding them).
const BYTES: &str = "$zip";
/// The entry a zip stream reads next.
const NEXT: &str = "$zipNext";
/// What a `ZipOutputStream` was given so far: names and their bytes, in turn.
const WRITTEN: &str = "$zipWritten";
/// The name of the entry a `ZipOutputStream` is writing.
const WRITING: &str = "$zipWriting";

/// One entry of a zip's central directory.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub name: String,
    /// 0 stored, 8 deflated.
    method: u16,
    crc: u32,
    packed: usize,
    pub size: usize,
    /// Where its local header begins.
    header: usize,
    /// Its DOS time and date.
    time: (u16, u16),
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        b.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// The entries of zip `zip`, in its central directory's order, or what is wrong with it.
pub(crate) fn entries(zip: &[u8]) -> Result<Vec<Entry>, String> {
    // The end record: 22 bytes and a comment of up to 65,535, at the very end.
    let end = (0..=zip.len().saturating_sub(22))
        .rev()
        .take(65_536)
        .find(|&at| zip[at..].starts_with(b"PK\x05\x06"))
        .ok_or("zip END header not found")?;
    let truncated = || "invalid END header (bad central directory offset)".to_owned();
    let (count, mut at) = (
        u16_at(zip, end + 10).ok_or_else(truncated)?,
        u32_at(zip, end + 16).ok_or_else(truncated)?,
    );
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if !zip.get(at..).is_some_and(|b| b.starts_with(b"PK\x01\x02")) {
            return Err("invalid CEN header (bad signature)".to_owned());
        }
        let field = |n: usize| u16_at(zip, at + n).ok_or_else(truncated);
        let long = |n: usize| u32_at(zip, at + n).ok_or_else(truncated);
        let (name, extra, comment) = (field(28)?, field(30)?, field(32)?);
        let text = zip.get(at + 46..at + 46 + name).ok_or_else(truncated)?;
        out.push(Entry {
            name: String::from_utf8_lossy(text).into_owned(),
            method: field(10)? as u16,
            crc: long(16)? as u32,
            packed: long(20)?,
            size: long(24)?,
            header: long(42)?,
            time: (field(12)? as u16, field(14)? as u16),
        });
        at += 46 + name + extra + comment;
    }
    Ok(out)
}

/// The bytes of entry `e` of zip `zip`, inflated where they were deflated.
pub(crate) fn entry_bytes(zip: &[u8], e: &Entry) -> Result<Vec<u8>, String> {
    let bad = || format!("invalid LOC header of {}", e.name);
    if !zip
        .get(e.header..)
        .is_some_and(|b| b.starts_with(b"PK\x03\x04"))
    {
        return Err(bad());
    }
    let start = e.header
        + 30
        + u16_at(zip, e.header + 26).ok_or_else(bad)?
        + u16_at(zip, e.header + 28).ok_or_else(bad)?;
    let packed = zip.get(start..start + e.packed).ok_or_else(bad)?;
    match e.method {
        0 => Ok(packed.to_vec()),
        8 => {
            use std::io::Read;
            let mut out = Vec::with_capacity(e.size);
            flate2::read::DeflateDecoder::new(packed)
                .read_to_end(&mut out)
                .map_err(|err| format!("invalid entry {}: {err}", e.name))?;
            Ok(out)
        }
        other => Err(format!(
            "unsupported compression method {other} of {}",
            e.name
        )),
    }
}

/// A zip of `files` (name, bytes), each stored as it is, in their order.
pub(crate) fn build(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        files
            .iter()
            .map(|(n, b)| n.len() * 2 + b.len() + 76)
            .sum::<usize>()
            + 22,
    );
    let mut central = Vec::new();
    // 1980-01-01, 00:00: the first date a zip can say.
    let (time, date) = (0u16, 33u16);
    for (name, bytes) in files {
        let mut crc = flate2::Crc::new();
        crc.update(bytes);
        let (at, crc, size) = (out.len() as u32, crc.sum(), bytes.len() as u32);
        // What the local header and the directory's entry both say: version needed, flags
        // (bit 11: the name is UTF-8), method, time, date, CRC, both sizes, the name's length
        // and the extra field's.
        let mut common = Vec::with_capacity(26);
        for field in [20u16, 0x0800, 0, time, date] {
            common.extend(field.to_le_bytes());
        }
        for field in [crc, size, size] {
            common.extend(field.to_le_bytes());
        }
        common.extend((name.len() as u16).to_le_bytes());
        common.extend(0u16.to_le_bytes());
        out.extend(b"PK\x03\x04");
        out.extend(&common);
        out.extend(name.as_bytes());
        out.extend(bytes);
        central.extend(b"PK\x01\x02");
        central.extend(20u16.to_le_bytes());
        central.extend(&common);
        // Comment length, disk, internal and external attributes (10 bytes), then where
        // the entry is.
        central.extend([0u8; 10]);
        central.extend(at.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let (count, start, size) = (files.len() as u16, out.len() as u32, central.len() as u32);
    out.extend(central);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// The mods' content folders by mod id (their manifests' `id`), and each one's version.
fn content_mods() -> &'static std::collections::HashMap<String, (PathBuf, String)> {
    static MODS: std::sync::OnceLock<std::collections::HashMap<String, (PathBuf, String)>> =
        std::sync::OnceLock::new();
    MODS.get_or_init(|| {
        let mut out = std::collections::HashMap::new();
        for folder in crate::resources::content_folders() {
            let Ok(manifest) = std::fs::read_to_string(folder.join("horizon-mod.toml")) else {
                continue;
            };
            let value = |key: &str| {
                manifest.lines().find_map(|l| {
                    let (k, v) = l.split_once('=')?;
                    (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
                })
            };
            if let Some(id) = value("id") {
                out.entry(id)
                    .or_insert((folder.clone(), value("version").unwrap_or_default()));
            }
        }
        out
    })
}

/// The files of a mod's jar as its content folder has them: what is under `assets` and
/// `data` but the declarations the translator wrote for the game (`data/<ns>/horizon/`),
/// sorted, after a manifest saying the mod's version.
fn jar_files(folder: &Path, version: &str) -> Vec<(String, Vec<u8>)> {
    let manifest = format!("Manifest-Version: 1.0\r\nImplementation-Version: {version}\r\n\r\n");
    let mut files = vec![(MANIFEST_PATH.to_owned(), manifest.into_bytes())];
    let mut found = Vec::new();
    let mut todo = vec![PathBuf::from("assets"), PathBuf::from("data")];
    while let Some(rel) = todo.pop() {
        let Ok(listing) = std::fs::read_dir(folder.join(&rel)) else {
            continue;
        };
        for e in listing.flatten() {
            let rel = rel.join(e.file_name());
            if e.path().is_dir() {
                // (`data/<namespace>/horizon`: the game's, not the jar's.)
                let ours = rel.starts_with("data")
                    && rel.components().count() == 3
                    && rel.ends_with("horizon");
                if !ours {
                    todo.push(rel);
                }
            } else if let Some(name) = rel.to_str() {
                found.push(name.replace('\\', "/"));
            }
        }
    }
    found.sort();
    for name in found {
        if let Ok(bytes) = std::fs::read(folder.join(&name)) {
            files.push((name, bytes));
        }
    }
    files
}

/// `path` is looked for on disk: where it is a loaded mod's jar as FML would have it
/// (`<game>/mods/<id>.jar`) and no file is there, the jar is made of the mod's content
/// folder (see the module's doc). Anything else is left as it is.
pub(crate) fn mod_jar(path: &Path) {
    let named = path.extension().is_some_and(|e| e == "jar")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|d| d == "mods");
    if !named || path.exists() {
        return;
    }
    let Some((folder, version)) = path
        .file_stem()
        .and_then(|id| id.to_str())
        .and_then(|id| content_mods().get(id))
    else {
        return;
    };
    let jar = build(&jar_files(folder, version));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // (Under another name first: a reader on another thread finds the whole jar or none.)
    let part = path.with_extension(format!("jar.{}.part", std::process::id()));
    if std::fs::write(&part, jar).is_ok() && std::fs::rename(&part, path).is_err() {
        let _ = std::fs::remove_file(&part);
    }
}

pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        ZIP_FILE | JAR_FILE | ZIP_ENTRY | JAR_ENTRY | MANIFEST | ATTRIBUTES
    )
}

/// The zip bytes object `holder` (a `ZipFile`, a zip stream) keeps.
fn held(rt: &Rt, holder: V) -> Vec<u8> {
    let bytes = rt.get_field(holder, BYTES);
    match rt.heap.get(bytes.r() as usize).map(|o| &o.data) {
        Some(Data::Bytes(b, _)) if bytes.r() != 0 => b.clone(),
        _ => Vec::new(),
    }
}

fn keep(rt: &mut Rt, holder: V, zip: Vec<u8>) {
    let bytes = rt.alloc("[B", Data::Bytes(zip, 0));
    rt.put_field(holder, BYTES, bytes);
}

/// A `ZipEntry` (of `class`) for entry `e`.
fn entry_object(rt: &mut Rt, class: &'static str, e: &Entry) -> V {
    let o = rt.new_object(class);
    let name = rt.string(&e.name);
    for (field, v) in [
        ("$name", name),
        ("$size", V::J(e.size as i64)),
        ("$packed", V::J(e.packed as i64)),
        ("$crc", V::J(i64::from(e.crc))),
        ("$method", V::I(i32::from(e.method))),
        ("$time", V::J(dos_millis(e.time))),
    ] {
        rt.put_field(o, field, v);
    }
    o
}

/// A DOS time and date as Java's milliseconds (UTC, as far as a zip says).
fn dos_millis((time, date): (u16, u16)) -> i64 {
    let (year, month, day) = (
        1980 + i64::from(date >> 9),
        i64::from(date >> 5 & 15).max(1),
        i64::from(date & 31).max(1),
    );
    let (hour, minute, second) = (
        i64::from(time >> 11),
        i64::from(time >> 5 & 63),
        i64::from(time & 31) * 2,
    );
    // Days since 1970-01-01 (the civil calendar's arithmetic).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    ((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000
}

/// A jar's manifest: the main section's `Name: value` lines (a line beginning with a space
/// goes on the one before), by name in lower case as `Attributes` finds them whatever their
/// case.
fn manifest_object(rt: &mut Rt, text: &str) -> V {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        match (line.strip_prefix(' '), lines.last_mut()) {
            (Some(more), Some(last)) => last.push_str(more),
            _ => lines.push(line.to_owned()),
        }
    }
    let main = rt.alloc(ATTRIBUTES, Data::Table(crate::Table::default()));
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let (k, v) = (
                rt.string(&name.trim().to_ascii_lowercase()),
                rt.string(value.trim()),
            );
            crate::jutil::table_put(rt, main, k, v);
        }
    }
    let manifest = rt.new_object(MANIFEST);
    rt.put_field(manifest, "$main", main);
    manifest
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match owner {
        ZIP_FILE | JAR_FILE => {
            let entry_class = if owner == JAR_FILE {
                JAR_ENTRY
            } else {
                ZIP_ENTRY
            };
            match name {
                // `new ZipFile(file)`, `(name)`, `(file, mode)`, `new JarFile(file, verify)`...
                "<init>" => {
                    let path = crate::jlang::text(rt, a(1));
                    let disk = crate::files::on_disk(rt, &path);
                    match std::fs::read(&disk)
                        .map_err(|e| e.to_string())
                        .and_then(|b| entries(&b).map(|_| b))
                    {
                        Ok(zip) => {
                            keep(rt, this, zip);
                            let p = rt.string(&path);
                            rt.put_field(this, "$path", p);
                        }
                        Err(why) if disk.is_file() => {
                            crate::jlang::throw_with(rt, ZIP_EXCEPTION, &why);
                        }
                        Err(_) => {
                            crate::jlang::throw_with(
                                rt,
                                "java/nio/file/NoSuchFileException",
                                &path,
                            );
                        }
                    }
                    V::Void
                }
                "getName" => rt.get_field(this, "$path"),
                "close" => V::Void,
                "size" => V::I(entries(&held(rt, this)).map_or(0, |e| e.len()) as i32),
                "getEntry" | "getJarEntry" => {
                    let wanted = crate::jlang::text(rt, a(1));
                    let all = entries(&held(rt, this)).unwrap_or_default();
                    // (A folder is found by its name with or without the `/` after it.)
                    match all.iter().find(|e| {
                        e.name == wanted || e.name.strip_suffix('/') == Some(wanted.as_str())
                    }) {
                        Some(e) => entry_object(rt, entry_class, e),
                        None => V::NULL,
                    }
                }
                "entries" | "stream" => {
                    let all = entries(&held(rt, this)).unwrap_or_default();
                    let objects: Vec<V> = all
                        .iter()
                        .map(|e| entry_object(rt, entry_class, e))
                        .collect();
                    if name == "stream" {
                        rt.alloc("java/util/stream/Stream", Data::Stream(objects))
                    } else {
                        crate::jutil::new_iter(rt, objects)
                    }
                }
                "getInputStream" => {
                    let wanted = crate::jlang::text(rt, rt.get_field(a(1), "$name"));
                    let zip = held(rt, this);
                    let read =
                        entries(&zip).and_then(|all| match all.iter().find(|e| e.name == wanted) {
                            Some(e) => entry_bytes(&zip, e).map(Some),
                            None => Ok(None),
                        });
                    match read {
                        Ok(Some(bytes)) => crate::resources::input_stream(rt, bytes),
                        Ok(None) => V::NULL,
                        Err(why) => crate::jlang::throw_with(rt, ZIP_EXCEPTION, &why),
                    }
                }
                "getManifest" => {
                    let zip = held(rt, this);
                    let text = entries(&zip).ok().and_then(|all| {
                        all.iter()
                            .find(|e| e.name == MANIFEST_PATH)
                            .and_then(|e| entry_bytes(&zip, e).ok())
                    });
                    match text {
                        Some(bytes) => manifest_object(rt, &String::from_utf8_lossy(&bytes)),
                        None => V::NULL,
                    }
                }
                _ => return None,
            }
        }
        ZIP_ENTRY | JAR_ENTRY => match name {
            "<init>" => {
                // `new ZipEntry(name)`, or a copy of another entry.
                let named = match rt.str_of(a(1)) {
                    Some(_) => a(1),
                    None => rt.get_field(a(1), "$name"),
                };
                rt.put_field(this, "$name", named);
                V::Void
            }
            "getName" | "toString" => rt.get_field(this, "$name"),
            "isDirectory" => V::I(i32::from(
                rt.str_of(rt.get_field(this, "$name"))
                    .is_some_and(|n| n.ends_with('/')),
            )),
            // What an entry made by hand has not been told is -1, as Java's.
            "getSize" | "getCompressedSize" | "getCrc" | "getTime" => {
                let field = match name {
                    "getSize" => "$size",
                    "getCompressedSize" => "$packed",
                    "getCrc" => "$crc",
                    _ => "$time",
                };
                match rt.get_field(this, field) {
                    V::J(n) => V::J(n),
                    _ => V::J(-1),
                }
            }
            "getMethod" => match rt.get_field(this, "$method") {
                V::I(m) => V::I(m),
                _ => V::I(-1),
            },
            "setSize" | "setCompressedSize" | "setCrc" | "setTime" | "setMethod" => {
                let field = match name {
                    "setSize" => "$size",
                    "setCompressedSize" => "$packed",
                    "setCrc" => "$crc",
                    "setTime" => "$time",
                    _ => "$method",
                };
                rt.put_field(this, field, a(1));
                V::Void
            }
            "getComment" | "getExtra" | "getLastModifiedTime" | "getAttributes" => V::NULL,
            _ if name.starts_with("set") => V::Void,
            _ => return None,
        },
        MANIFEST => match name {
            // `new Manifest()`: empty; `new Manifest(stream)`: what the stream holds.
            "<init>" => {
                let text = match args.get(1) {
                    Some(&stream) => crate::streams::drain(rt, stream).unwrap_or_default(),
                    None => Vec::new(),
                };
                let read = manifest_object(rt, &String::from_utf8_lossy(&text));
                let main = rt.get_field(read, "$main");
                rt.put_field(this, "$main", main);
                V::Void
            }
            "getMainAttributes" => rt.get_field(this, "$main"),
            // The sections of single entries: none is kept.
            "getEntries" => rt.alloc("java/util/HashMap", Data::Table(crate::Table::default())),
            "getAttributes" => V::NULL,
            _ => return None,
        },
        _ => match name {
            "<init>" => {
                if let Some(o) = rt.heap.get_mut(this.r() as usize) {
                    o.data = Data::Table(crate::Table::default());
                }
                V::Void
            }
            // By name whatever its case, a `String` or an `Attributes.Name`.
            "getValue" | "get" | "containsKey" | "putValue" | "put" => {
                let named = match rt.str_of(a(1)) {
                    Some(n) => n.to_ascii_lowercase(),
                    None => {
                        crate::jlang::text(rt, rt.get_field(a(1), "$name")).to_ascii_lowercase()
                    }
                };
                let k = rt.string(&named);
                let was = crate::jutil::table_get(rt, this, k).unwrap_or(V::NULL);
                match name {
                    "containsKey" => V::I(i32::from(!was.is_null())),
                    "putValue" | "put" => {
                        crate::jutil::table_put(rt, this, k, a(2));
                        was
                    }
                    _ => was,
                }
            }
            "size" => V::I(crate::jutil::elements(rt, this).len() as i32),
            "isEmpty" => V::I(i32::from(crate::jutil::elements(rt, this).is_empty())),
            _ => return None,
        },
    })
}

// ---- The streams' side ([`crate::streams`]) ----

/// `new ZipInputStream(inner)`: the zip `inner` holds, before its first entry.
pub(crate) fn open_stream(rt: &mut Rt, this: V, inner: V) {
    let zip = crate::streams::drain(rt, inner).unwrap_or_default();
    crate::resources::become_bytes(rt, this, Vec::new());
    keep(rt, this, zip);
    rt.put_field(this, NEXT, V::I(0));
    rt.put_field(this, "$in", inner);
}

/// A zip input stream's own methods: `getNextEntry` makes the stream that entry's bytes
/// (null and an empty stream after the last), `closeEntry` ends it where it is.
pub(crate) fn stream_call(rt: &mut Rt, jar: bool, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    Some(match name {
        "getNextEntry" | "getNextJarEntry" => {
            let zip = held(rt, this);
            let next = rt.get_field(this, NEXT).i().max(0) as usize;
            let read = entries(&zip).and_then(|all| match all.get(next) {
                Some(e) => entry_bytes(&zip, e).map(|b| Some((e.clone(), b))),
                None => Ok(None),
            });
            match read {
                Ok(Some((e, bytes))) => {
                    crate::resources::become_bytes(rt, this, bytes);
                    rt.put_field(this, NEXT, V::I(next as i32 + 1));
                    entry_object(rt, if jar { JAR_ENTRY } else { ZIP_ENTRY }, &e)
                }
                Ok(None) => {
                    crate::resources::become_bytes(rt, this, Vec::new());
                    V::NULL
                }
                Err(why) => crate::jlang::throw_with(rt, ZIP_EXCEPTION, &why),
            }
        }
        "closeEntry" => {
            crate::resources::become_bytes(rt, this, Vec::new());
            V::Void
        }
        "getManifest" => {
            let zip = held(rt, this);
            let text = entries(&zip).ok().and_then(|all| {
                all.iter()
                    .find(|e| e.name == MANIFEST_PATH)
                    .and_then(|e| entry_bytes(&zip, e).ok())
            });
            match text {
                Some(bytes) => manifest_object(rt, &String::from_utf8_lossy(&bytes)),
                None => V::NULL,
            }
        }
        _ => return None,
    })
}

/// The entry a `ZipOutputStream` writes is done: its bytes are kept under its name.
fn close_written(rt: &mut Rt, this: V) {
    let writing = rt.get_field(this, WRITING);
    if rt.str_of(writing).is_none() {
        return;
    }
    let bytes = match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
        Some(Data::Bytes(b, _)) => std::mem::take(b),
        _ => Vec::new(),
    };
    let kept = rt.alloc("[B", Data::Bytes(bytes, 0));
    let written = match rt.get_field(this, WRITTEN) {
        list if list.r() != 0 => list,
        _ => {
            let list = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(this, WRITTEN, list);
            list
        }
    };
    if let Some(Data::List(l)) = rt.heap.get_mut(written.r() as usize).map(|o| &mut o.data) {
        l.extend([writing, kept]);
    }
    rt.put_field(this, WRITING, V::Void);
}

/// A zip output stream's own methods: `putNextEntry` begins an entry (ending the one before),
/// `closeEntry` ends it; how hard to press is not asked (entries are stored).
pub(crate) fn out_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    Some(match name {
        "putNextEntry" => {
            close_written(rt, this);
            let named = rt.get_field(args.get(1).copied().unwrap_or_default(), "$name");
            rt.put_field(this, WRITING, named);
            V::Void
        }
        "closeEntry" => {
            close_written(rt, this);
            V::Void
        }
        "setLevel" | "setMethod" | "setComment" => V::Void,
        _ => return None,
    })
}

/// `finish()`: the zip of what a `ZipOutputStream` was given, for the stream it is around.
pub(crate) fn finished(rt: &mut Rt, this: V) -> Vec<u8> {
    close_written(rt, this);
    let written = crate::jutil::elements(rt, rt.get_field(this, WRITTEN));
    let files: Vec<(String, Vec<u8>)> = written
        .chunks(2)
        .filter_map(|pair| {
            let name = rt.str_of(pair[0])?.to_owned();
            let bytes = match rt.heap.get(pair.get(1)?.r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b.clone(),
                _ => Vec::new(),
            };
            Some((name, bytes))
        })
        .collect();
    build(&files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// A zip written is read back: stored entries as `build` writes them, a deflated one as
    /// any zip tool writes it, in the directory's order, with a folder's entry and the sizes.
    #[test]
    fn a_zip_built_is_read_back() {
        let files = vec![
            ("assets/gears/".to_owned(), Vec::new()),
            (
                "assets/gears/lang/en_us.json".to_owned(),
                b"{\"a\":\"b\"}".to_vec(),
            ),
            ("data/gears/recipe/cog.json".to_owned(), vec![7u8; 4000]),
        ];
        let zip = build(&files);
        let listed = entries(&zip).unwrap();
        assert_eq!(
            listed.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(listed[2].size, 4000);
        for (e, (_, bytes)) in listed.iter().zip(&files) {
            assert_eq!(&entry_bytes(&zip, e).unwrap(), bytes);
        }
        // The same entry deflated: its packed bytes in place of the stored ones.
        use std::io::Write;
        let mut packer =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        packer.write_all(&files[2].1).unwrap();
        let packed = packer.finish().unwrap();
        let mut deflated = Entry {
            method: 8,
            packed: packed.len(),
            ..listed[2].clone()
        };
        let mut zip2 = zip[..listed[2].header + 30 + deflated.name.len()].to_vec();
        deflated.header = listed[2].header;
        zip2.extend(&packed);
        assert_eq!(entry_bytes(&zip2, &deflated).unwrap(), files[2].1);
        assert!(entries(b"not a zip at all, whatever its length is").is_err());
        assert_eq!(dos_millis((0, 33)), 315_532_800_000);
    }

    /// A mod's own jar is made of its content folder where FML would have the jar, once
    /// something looks for it: its assets and data but the game's declarations, and a
    /// manifest. `ZipFile`, `ZipInputStream` and `JarFile` read it as any zip, and a
    /// `ZipOutputStream` writes one they read.
    #[test]
    fn a_mods_jar_is_made_of_its_content_and_read_as_a_zip() {
        let dir = std::env::temp_dir().join(format!("hz-zips-{}", std::process::id()));
        let content = dir.join("content-maps");
        for (file, text) in [
            ("assets/maps/theme/flat/icon.png", "png"),
            ("assets/maps/lang/en_us.json", "{}"),
            ("data/maps/recipe/atlas.json", "{\"type\":\"x\"}"),
            ("data/maps/horizon/block/table.json", "{}"),
            ("horizon-mod.toml", "id = \"maps\"\nversion = \"6.0.2\"\n"),
        ] {
            std::fs::create_dir_all(content.join(file).parent().unwrap()).unwrap();
            std::fs::write(content.join(file), text).unwrap();
        }
        let files = jar_files(&content, "6.0.2");
        assert_eq!(
            files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
            [
                MANIFEST_PATH,
                "assets/maps/lang/en_us.json",
                "assets/maps/theme/flat/icon.png",
                "data/maps/recipe/atlas.json"
            ]
        );
        let jar = dir.join("mods/maps.jar");
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, build(&files)).unwrap();

        let mut rt = Rt::new();
        crate::install();
        let path = rt.string(jar.to_str().unwrap());
        let zip = rt.new_object(ZIP_FILE);
        c(
            &mut rt,
            "java/util/zip/ZipFile.<init>.(Ljava/lang/String;)V",
            &[zip, path],
        );
        assert!(rt.thrown.is_none());
        assert_eq!(
            c(&mut rt, "java/util/zip/ZipFile.size.()I", &[zip]),
            V::I(4)
        );
        let name = rt.string("assets/maps/theme/flat/icon.png");
        let entry = c(
            &mut rt,
            "java/util/zip/ZipFile.getEntry.(Ljava/lang/String;)Ljava/util/zip/ZipEntry;",
            &[zip, name],
        );
        assert_eq!(
            c(&mut rt, "java/util/zip/ZipEntry.getSize.()J", &[entry]),
            V::J(3)
        );
        assert_eq!(
            c(&mut rt, "java/util/zip/ZipEntry.isDirectory.()Z", &[entry]),
            V::I(0)
        );
        let stream = c(
            &mut rt,
            "java/util/zip/ZipFile.getInputStream.(Ljava/util/zip/ZipEntry;)Ljava/io/InputStream;",
            &[zip, entry],
        );
        assert_eq!(
            crate::streams::drain(&mut rt, stream),
            Some(b"png".to_vec())
        );
        let none = rt.string("assets/maps/none.png");
        assert!(
            c(
                &mut rt,
                "java/util/zip/ZipFile.getEntry.(Ljava/lang/String;)Ljava/util/zip/ZipEntry;",
                &[zip, none]
            )
            .is_null()
        );

        // The same file through a stream, entry by entry, as JourneyMap walks its jar.
        let file_in = rt.new_object("java/io/FileInputStream");
        c(
            &mut rt,
            "java/io/FileInputStream.<init>.(Ljava/lang/String;)V",
            &[file_in, path],
        );
        let zin = rt.new_object("java/util/zip/ZipInputStream");
        c(
            &mut rt,
            "java/util/zip/ZipInputStream.<init>.(Ljava/io/InputStream;)V",
            &[zin, file_in],
        );
        let mut walked = Vec::new();
        loop {
            let e = c(
                &mut rt,
                "java/util/zip/ZipInputStream.getNextEntry.()Ljava/util/zip/ZipEntry;",
                &[zin],
            );
            if e.is_null() {
                break;
            }
            let n = c(
                &mut rt,
                "java/util/zip/ZipEntry.getName.()Ljava/lang/String;",
                &[e],
            );
            let bytes = crate::streams::drain(&mut rt, zin).unwrap_or_default();
            walked.push((rt.str_of(n).unwrap_or("").to_owned(), bytes.len()));
            c(
                &mut rt,
                "java/util/zip/ZipInputStream.closeEntry.()V",
                &[zin],
            );
        }
        assert_eq!(walked.len(), 4);
        assert_eq!(walked[3], ("data/maps/recipe/atlas.json".to_owned(), 12));

        // The jar's manifest says the version.
        let jar_file = rt.new_object(JAR_FILE);
        let file = rt.new_object("java/io/File");
        c(
            &mut rt,
            "java/io/File.<init>.(Ljava/lang/String;)V",
            &[file, path],
        );
        c(
            &mut rt,
            "java/util/jar/JarFile.<init>.(Ljava/io/File;)V",
            &[jar_file, file],
        );
        let manifest = c(
            &mut rt,
            "java/util/jar/JarFile.getManifest.()Ljava/util/jar/Manifest;",
            &[jar_file],
        );
        let main = c(
            &mut rt,
            "java/util/jar/Manifest.getMainAttributes.()Ljava/util/jar/Attributes;",
            &[manifest],
        );
        let wanted = rt.string("Implementation-Version");
        let version = c(
            &mut rt,
            "java/util/jar/Attributes.getValue.(Ljava/lang/String;)Ljava/lang/String;",
            &[main, wanted],
        );
        assert_eq!(rt.str_of(version), Some("6.0.2"));

        // And a zip a mod writes: two entries into a stream of bytes, read back.
        let out = rt.new_object("java/io/ByteArrayOutputStream");
        c(&mut rt, "java/io/ByteArrayOutputStream.<init>.()V", &[out]);
        let zout = rt.new_object("java/util/zip/ZipOutputStream");
        c(
            &mut rt,
            "java/util/zip/ZipOutputStream.<init>.(Ljava/io/OutputStream;)V",
            &[zout, out],
        );
        for (entry_name, text) in [("a.txt", "first"), ("b/c.txt", "second one")] {
            let e = rt.new_object(ZIP_ENTRY);
            let n = rt.string(entry_name);
            c(
                &mut rt,
                "java/util/zip/ZipEntry.<init>.(Ljava/lang/String;)V",
                &[e, n],
            );
            c(
                &mut rt,
                "java/util/zip/ZipOutputStream.putNextEntry.(Ljava/util/zip/ZipEntry;)V",
                &[zout, e],
            );
            let bytes = crate::resources::byte_array(&mut rt, text.as_bytes());
            c(
                &mut rt,
                "java/util/zip/ZipOutputStream.write.([B)V",
                &[zout, bytes],
            );
            c(
                &mut rt,
                "java/util/zip/ZipOutputStream.closeEntry.()V",
                &[zout],
            );
        }
        c(&mut rt, "java/util/zip/ZipOutputStream.close.()V", &[zout]);
        let made = c(
            &mut rt,
            "java/io/ByteArrayOutputStream.toByteArray.()[B",
            &[out],
        );
        let made = crate::resources::array_bytes(&rt, made);
        let listed = entries(&made).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|e| (e.name.as_str(), e.size))
                .collect::<Vec<_>>(),
            [("a.txt", 5), ("b/c.txt", 10)]
        );
        assert_eq!(entry_bytes(&made, &listed[1]).unwrap(), b"second one");
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
