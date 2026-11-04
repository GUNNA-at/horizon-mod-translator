//! Files a mod keeps beside the game (Create's schematics, a mod's caches): Java's
//! `java.nio.file` (`Path`, `Paths`, `Files`) and `java.io.File` over the real files, and FML's
//! folders. `FMLPaths.GAMEDIR` is the folder the game runs in: FML's game directory is the
//! launcher's working directory, and the game finds `horizon-mods` there too. A path is its text
//! ([`Data::Str`]) under the class `Path` or `File`, made as Java's `UnixPath` makes it (no
//! doubled or trailing `/`); relative ones are on disk against the game's folder, as Java's are
//! against its working directory. What fails throws the exception Java's would
//! (`NoSuchFileException`, `FileAlreadyExistsException`, `DirectoryNotEmptyException`...).
use crate::{Data, Rt, V};
use std::path::{Path, PathBuf};

const PATH: &str = "java/nio/file/Path";
const PATHS: &str = "java/nio/file/Paths";
const FILES: &str = "java/nio/file/Files";
const FILE: &str = "java/io/File";
const FILE_TIME: &str = "java/nio/file/attribute/FileTime";
const FML_PATHS: &str = "net/neoforged/fml/loading/FMLPaths";
/// FML's folders by constant, in its order: (name, path in the game's folder, a folder FML makes
/// when it starts).
const FML_FOLDERS: [(&str, &str, bool); 4] = [("GAMEDIR", "", true), ("MODSDIR", "mods", true), ("CONFIGDIR", "config", true), ("FMLCONFIG", "config/fml.toml", false)];
/// Where tests put the game's folder (`Rt::statics`); the game uses its working directory.
pub const GAME_DIR: &str = "compat:game-dir";

pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, PATH | PATHS | FILES | FILE | FILE_TIME | FML_PATHS)
}

/// The game's folder.
pub fn game_dir(rt: &Rt) -> PathBuf {
    match rt.statics.get(GAME_DIR).and_then(|&v| rt.str_of(v)) {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_dir().unwrap_or_default(),
    }
}

/// A path's text as `UnixPath` keeps it: no repeated `/`, none at the end but the root.
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

fn new_path(rt: &mut Rt, text: &str) -> V {
    rt.alloc(PATH, Data::Str(clean(text)))
}

fn new_file(rt: &mut Rt, text: &str) -> V {
    rt.alloc(FILE, Data::Str(clean(text)))
}

/// A path's, a file's or a string's text.
fn text(rt: &Rt, v: V) -> Option<String> {
    rt.str_of(v).map(str::to_owned)
}

/// Where `text` is on disk.
fn on_disk(rt: &Rt, text: &str) -> PathBuf {
    let p = Path::new(text);
    if p.is_absolute() { p.to_owned() } else { game_dir(rt).join(p) }
}

/// `a.resolve(b)`: `b` when it is absolute or `a` is empty, `a` when `b` is empty.
fn resolve(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        b.to_owned()
    } else if b.is_empty() {
        a.to_owned()
    } else if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// The names of a path, the root left out.
fn names(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return vec![""];
    }
    text.split('/').filter(|n| !n.is_empty()).collect()
}

/// `getParent()`: none for a single name or the root.
fn parent(text: &str) -> Option<String> {
    let names = names(text);
    let absolute = text.starts_with('/');
    match names.len() {
        0 => None,
        1 => absolute.then(|| "/".to_owned()),
        n => Some(format!("{}{}", if absolute { "/" } else { "" }, names[..n - 1].join("/"))),
    }
}

/// `normalize()`: `.` dropped, `name/..` folded, as `UnixPath` does without the disk.
fn normalize(text: &str) -> String {
    let absolute = text.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for n in names(text) {
        match n {
            "." => {}
            ".." if out.last().is_some_and(|l| *l != "..") => {
                out.pop();
            }
            ".." if absolute => {}
            _ => out.push(n),
        }
    }
    let body = out.join("/");
    if absolute { format!("/{body}") } else { body }
}

/// `a.relativize(b)`: the path from `a` to `b` (both absolute, or both relative).
fn relativize(a: &str, b: &str) -> String {
    let (x, y) = (names(a), names(b));
    let (x, y): (Vec<&str>, Vec<&str>) = (x.into_iter().filter(|n| !n.is_empty()).collect(), y.into_iter().filter(|n| !n.is_empty()).collect());
    let common = x.iter().zip(&y).take_while(|(p, q)| p == q).count();
    let mut out: Vec<&str> = vec![".."; x.len() - common];
    out.extend(&y[common..]);
    out.join("/")
}

/// Whether `a` starts (or ends) with `b`, name by name; an absolute `b` only starts an
/// absolute `a`.
fn starts_with(a: &str, b: &str) -> bool {
    if a.starts_with('/') != b.starts_with('/') {
        return false;
    }
    let (x, y) = (names(a), names(b));
    y.len() <= x.len() && x[..y.len()] == y[..]
}

fn ends_with(a: &str, b: &str) -> bool {
    if b.starts_with('/') {
        return a == b;
    }
    let (x, y) = (names(a), names(b));
    y.len() <= x.len() && x[x.len() - y.len()..] == y[..]
}

/// Throws `class` with `message`, as Java's file system does; the call's value is then unused.
fn throw(rt: &mut Rt, class: &'static str, message: &str) -> V {
    let e = rt.new_object(class);
    let m = rt.string(message);
    rt.put_field(e, "message", m);
    rt.thrown = Some(e);
    V::NULL
}

/// The exception Java's `Files` throws for an error of the disk's.
fn throw_io(rt: &mut Rt, e: &std::io::Error, text: &str) -> V {
    use std::io::ErrorKind as K;
    let class = match e.kind() {
        K::NotFound => "java/nio/file/NoSuchFileException",
        K::AlreadyExists => "java/nio/file/FileAlreadyExistsException",
        K::DirectoryNotEmpty => "java/nio/file/DirectoryNotEmptyException",
        K::NotADirectory => "java/nio/file/NotDirectoryException",
        K::PermissionDenied => "java/nio/file/AccessDeniedException",
        _ => "java/io/IOException",
    };
    let message = if class == "java/io/IOException" { format!("{text}: {e}") } else { text.to_owned() };
    throw(rt, class, &message)
}

fn stream(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc("java/util/stream/Stream", Data::Stream(items))
}

/// The varargs' enum constants by name (`StandardOpenOption.APPEND`, `REPLACE_EXISTING`).
fn has_option(rt: &Rt, options: V, name: &str) -> bool {
    crate::jutil::elements(rt, options).into_iter().any(|o| rt.str_of(rt.get_field(o, "$name")) == Some(name))
}

/// The entries of a folder, sorted by name (Java's order is the disk's, which isn't fixed).
fn entries(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    Ok(names)
}

/// `Files.walk(start, depth)`: the start, then each folder's entries before the next's,
/// depth first.
fn walk(rt: &Rt, start: &str, depth: i32, out: &mut Vec<String>) {
    out.push(start.to_owned());
    if depth <= 0 {
        return;
    }
    let disk = on_disk(rt, start);
    if !disk.is_dir() {
        return;
    }
    for n in entries(&disk).unwrap_or_default() {
        walk(rt, &resolve(start, &n), depth - 1, out);
    }
}

fn bytes_of(rt: &Rt, array: V) -> Vec<u8> {
    crate::jutil::elements(rt, array).into_iter().map(|b| b.i() as u8).collect()
}

fn byte_array(rt: &mut Rt, bytes: &[u8]) -> V {
    let a = rt.new_array(bytes.len() as i32, V::I(0));
    if let Some(crate::Object { data: Data::Array(items), .. }) = rt.heap.get_mut(a.r() as usize) {
        for (slot, b) in items.iter_mut().zip(bytes) {
            *slot = V::I(i32::from(*b as i8));
        }
    }
    a
}

fn file_time(rt: &mut Rt, modified: std::io::Result<std::time::SystemTime>) -> V {
    let millis = modified.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
    let t = rt.new_object(FILE_TIME);
    rt.put_field(t, "$millis", V::J(millis));
    t
}

fn write(rt: &mut Rt, text: &str, data: &[u8], options: V) -> V {
    use std::io::Write;
    let disk = on_disk(rt, text);
    let append = has_option(rt, options, "APPEND");
    if has_option(rt, options, "CREATE_NEW") && disk.exists() {
        return throw(rt, "java/nio/file/FileAlreadyExistsException", text);
    }
    let file = std::fs::OpenOptions::new().write(true).create(true).append(append).truncate(!append).open(&disk);
    match file.and_then(|mut f| f.write_all(data)) {
        Ok(()) => V::Void,
        Err(e) => throw_io(rt, &e, text),
    }
}

/// `Path`'s own methods, on the path `p` (its text).
fn path_method(rt: &mut Rt, name: &str, p: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "toString" => rt.string(p),
        "resolve" => {
            let other = text(rt, a(1))?;
            new_path(rt, &resolve(p, &other))
        }
        "resolveSibling" => {
            let other = text(rt, a(1))?;
            match parent(p) {
                Some(up) => new_path(rt, &resolve(&up, &other)),
                None => new_path(rt, &other),
            }
        }
        "getParent" => match parent(p) {
            Some(up) => new_path(rt, &up),
            None => V::NULL,
        },
        "getFileName" => match names(p).last() {
            Some(n) if p != "/" => new_path(rt, n),
            _ => V::NULL,
        },
        "getRoot" => {
            if p.starts_with('/') {
                new_path(rt, "/")
            } else {
                V::NULL
            }
        }
        "isAbsolute" => V::I(p.starts_with('/') as i32),
        "toAbsolutePath" => {
            let abs = on_disk(rt, p);
            new_path(rt, &abs.to_string_lossy())
        }
        "normalize" => new_path(rt, &normalize(p)),
        "relativize" => {
            let other = text(rt, a(1))?;
            new_path(rt, &relativize(p, &other))
        }
        "startsWith" | "endsWith" => {
            let other = text(rt, a(1))?;
            let r = if name == "startsWith" { starts_with(p, &other) } else { ends_with(p, &other) };
            V::I(r as i32)
        }
        "getNameCount" => V::I(if p == "/" { 0 } else { names(p).len() as i32 }),
        "getName" => {
            let n = names(p).get(a(1).i().max(0) as usize).map(|s| s.to_string());
            match n {
                Some(n) => new_path(rt, &n),
                None => return Some(throw(rt, "java/lang/IllegalArgumentException", "")),
            }
        }
        "subpath" => {
            let all = names(p);
            let (b, e) = (a(1).i().max(0) as usize, a(2).i().max(0) as usize);
            match all.get(b..e) {
                Some(part) if b < e => new_path(rt, &part.join("/")),
                _ => return Some(throw(rt, "java/lang/IllegalArgumentException", "")),
            }
        }
        "iterator" => {
            let parts: Vec<String> = if p == "/" { Vec::new() } else { names(p).into_iter().map(str::to_owned).collect() };
            let items: Vec<V> = parts.iter().map(|n| new_path(rt, n)).collect();
            rt.alloc("java/util/Iterator", Data::Iter(items, 0))
        }
        "toFile" => new_file(rt, p),
        "equals" => {
            let o = a(1);
            V::I((o.r() != 0 && rt.class_of(o) == PATH && rt.str_of(o) == Some(p)) as i32)
        }
        "hashCode" => V::I(rt.java_hash(a(0))),
        "compareTo" => {
            let other = text(rt, a(1))?;
            V::I(match p.as_bytes().cmp(other.as_bytes()) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        _ => return None,
    })
}

/// `File`'s methods, on the file `f` (its text).
fn file_method(rt: &mut Rt, name: &str, f: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let disk = on_disk(rt, f);
    Some(match name {
        "toPath" => new_path(rt, f),
        "getPath" | "toString" => rt.string(f),
        "getName" => {
            let n = names(f).last().copied().unwrap_or("").to_owned();
            rt.string(&n)
        }
        "getAbsolutePath" => rt.string(&disk.to_string_lossy()),
        "getAbsoluteFile" => new_file(rt, &disk.to_string_lossy()),
        "getParent" => match parent(f) {
            Some(up) => rt.string(&up),
            None => V::NULL,
        },
        "getParentFile" => match parent(f) {
            Some(up) => new_file(rt, &up),
            None => V::NULL,
        },
        "exists" => V::I(disk.exists() as i32),
        "isDirectory" => V::I(disk.is_dir() as i32),
        "isFile" => V::I(disk.is_file() as i32),
        "canRead" => V::I(disk.exists() as i32),
        "canWrite" => V::I(disk.metadata().is_ok_and(|m| !m.permissions().readonly()) as i32),
        "length" => V::J(disk.metadata().map_or(0, |m| m.len() as i64)),
        "lastModified" => V::J(disk.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)),
        "mkdir" => V::I(std::fs::create_dir(&disk).is_ok() as i32),
        "mkdirs" => V::I((!disk.exists() && std::fs::create_dir_all(&disk).is_ok()) as i32),
        "delete" => V::I(if disk.is_dir() { std::fs::remove_dir(&disk).is_ok() } else { std::fs::remove_file(&disk).is_ok() } as i32),
        "createNewFile" => match std::fs::OpenOptions::new().write(true).create_new(true).open(&disk) {
            Ok(_) => V::I(1),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => V::I(0),
            Err(e) => {
                let _ = throw_io(rt, &e, f);
                V::I(0)
            }
        },
        "renameTo" => {
            let to = text(rt, a(1))?;
            V::I(std::fs::rename(&disk, on_disk(rt, &to)).is_ok() as i32)
        }
        "list" | "listFiles" => match entries(&disk) {
            Ok(names) if disk.is_dir() => {
                let items: Vec<V> = names.iter().map(|n| if name == "list" { rt.string(n) } else { new_file(rt, &resolve(f, n)) }).collect();
                let arr = rt.new_array(items.len() as i32, V::NULL);
                for (i, v) in items.into_iter().enumerate() {
                    rt.array_set(arr, i as i32, v);
                }
                arr
            }
            _ => V::NULL,
        },
        "equals" => {
            let o = a(1);
            V::I((o.r() != 0 && rt.class_of(o) == FILE && rt.str_of(o) == Some(f)) as i32)
        }
        "hashCode" => V::I(rt.java_hash(a(0)) ^ 1234321),
        "compareTo" => {
            let other = text(rt, a(1))?;
            V::I(f.cmp(other.as_str()) as i32)
        }
        _ => return None,
    })
}

/// `Files`' static methods.
fn files_method(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let p = text(rt, a(0))?;
    let disk = on_disk(rt, &p);
    Some(match name {
        "exists" => V::I(disk.exists() as i32),
        "notExists" => V::I(!disk.exists() as i32),
        "isDirectory" => V::I(disk.is_dir() as i32),
        "isRegularFile" => V::I(disk.is_file() as i32),
        "isReadable" => V::I(disk.exists() as i32),
        "isWritable" => V::I(disk.metadata().is_ok_and(|m| !m.permissions().readonly()) as i32),
        "isHidden" => V::I(names(&p).last().is_some_and(|n| n.starts_with('.')) as i32),
        "isSymbolicLink" => V::I(disk.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) as i32),
        "isSameFile" => {
            let other = text(rt, a(1))?;
            V::I((on_disk(rt, &other) == disk) as i32)
        }
        "createDirectories" => {
            if disk.exists() && !disk.is_dir() {
                return Some(throw(rt, "java/nio/file/FileAlreadyExistsException", &p));
            }
            match std::fs::create_dir_all(&disk) {
                Ok(()) => a(0),
                Err(e) => throw_io(rt, &e, &p),
            }
        }
        "createDirectory" => match std::fs::create_dir(&disk) {
            Ok(()) => a(0),
            Err(e) => throw_io(rt, &e, &p),
        },
        "createFile" => match std::fs::OpenOptions::new().write(true).create_new(true).open(&disk) {
            Ok(_) => a(0),
            Err(e) => throw_io(rt, &e, &p),
        },
        "delete" | "deleteIfExists" => {
            if !disk.exists() && disk.symlink_metadata().is_err() {
                if name == "deleteIfExists" {
                    return Some(V::I(0));
                }
                return Some(throw(rt, "java/nio/file/NoSuchFileException", &p));
            }
            let r = if disk.is_dir() { std::fs::remove_dir(&disk) } else { std::fs::remove_file(&disk) };
            match r {
                Ok(()) if name == "deleteIfExists" => V::I(1),
                Ok(()) => V::Void,
                Err(e) => throw_io(rt, &e, &p),
            }
        }
        "list" => {
            if !disk.exists() {
                return Some(throw(rt, "java/nio/file/NoSuchFileException", &p));
            }
            if !disk.is_dir() {
                return Some(throw(rt, "java/nio/file/NotDirectoryException", &p));
            }
            match entries(&disk) {
                Ok(names) => {
                    let items: Vec<V> = names.iter().map(|n| new_path(rt, &resolve(&p, n))).collect();
                    stream(rt, items)
                }
                Err(e) => throw_io(rt, &e, &p),
            }
        }
        "walk" => {
            if !disk.exists() {
                return Some(throw(rt, "java/nio/file/NoSuchFileException", &p));
            }
            let depth = if desc.starts_with("(Ljava/nio/file/Path;I") { a(1).i() } else { i32::MAX };
            let mut found = Vec::new();
            walk(rt, &p, depth, &mut found);
            let items: Vec<V> = found.iter().map(|t| new_path(rt, t)).collect();
            stream(rt, items)
        }
        "size" => match disk.metadata() {
            Ok(m) => V::J(m.len() as i64),
            Err(e) => throw_io(rt, &e, &p),
        },
        "getLastModifiedTime" => match disk.metadata() {
            Ok(m) => file_time(rt, m.modified()),
            Err(e) => throw_io(rt, &e, &p),
        },
        "readString" | "readAllLines" | "lines" | "readAllBytes" => match std::fs::read(&disk) {
            Ok(bytes) if name == "readAllBytes" => byte_array(rt, &bytes),
            Ok(bytes) => {
                let s = String::from_utf8_lossy(&bytes).into_owned();
                match name {
                    "readString" => rt.string(&s),
                    _ => {
                        let lines: Vec<V> = s.lines().map(|l| rt.string(l)).collect();
                        if name == "lines" { stream(rt, lines) } else { crate::jutil::new_list(rt, lines) }
                    }
                }
            }
            Err(e) => throw_io(rt, &e, &p),
        },
        // `writeString(path, text[, charset], options...)`.
        "writeString" => {
            let s = text(rt, a(1)).unwrap_or_default();
            let options = if desc.contains("Charset;") { a(3) } else { a(2) };
            let r = write(rt, &p, s.as_bytes(), options);
            if rt.thrown.is_some() { r } else { a(0) }
        }
        // `write(path, bytes, options...)`, `write(path, lines[, charset], options...)`.
        "write" => {
            let (data, options) = if desc.starts_with("(Ljava/nio/file/Path;[B") {
                (bytes_of(rt, a(1)), a(2))
            } else {
                let lines: Vec<String> = crate::jutil::elements(rt, a(1)).into_iter().map(|l| text(rt, l).unwrap_or_default()).collect();
                let mut s = String::new();
                for l in lines {
                    s.push_str(&l);
                    s.push('\n');
                }
                (s.into_bytes(), if desc.contains("Charset;") { a(3) } else { a(2) })
            };
            let r = write(rt, &p, &data, options);
            if rt.thrown.is_some() { r } else { a(0) }
        }
        "copy" | "move" => {
            let to = text(rt, a(1))?;
            let dest = on_disk(rt, &to);
            if !disk.exists() {
                return Some(throw(rt, "java/nio/file/NoSuchFileException", &p));
            }
            if dest.exists() && !has_option(rt, a(2), "REPLACE_EXISTING") {
                return Some(throw(rt, "java/nio/file/FileAlreadyExistsException", &to));
            }
            let r = if name == "move" { std::fs::rename(&disk, &dest) } else { std::fs::copy(&disk, &dest).map(|_| ()) };
            match r {
                Ok(()) => a(1),
                Err(e) => throw_io(rt, &e, &p),
            }
        }
        _ => return None,
    })
}

/// `Paths.get(first, more...)`, `Path.of(first, more...)`: the parts joined, empty ones skipped.
fn join_parts(rt: &Rt, first: V, more: V) -> Option<String> {
    let mut out = text(rt, first)?;
    for m in crate::jutil::elements(rt, more) {
        let part = text(rt, m).unwrap_or_default();
        if part.is_empty() {
            continue;
        }
        out = if out.is_empty() { part } else { format!("{out}/{part}") };
    }
    Some(out)
}

fn fml_path(rt: &mut Rt, constant: V, absolute: bool) -> Option<V> {
    let name = rt.str_of(rt.get_field(constant, "$name"))?.to_owned();
    let &(_, relative, folder) = FML_FOLDERS.iter().find(|(n, _, _)| *n == name)?;
    if !absolute {
        return Some(new_path(rt, relative));
    }
    let dir = game_dir(rt).join(relative);
    // FML makes its folders when it starts; a mod may write into `config` straight away.
    if folder {
        let _ = std::fs::create_dir_all(&dir);
    }
    Some(new_path(rt, &normalize(&dir.to_string_lossy())))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        PATH | PATHS if name == "of" || name == "get" => {
            if !desc.starts_with("(Ljava/lang/String;") {
                return None;
            }
            let joined = join_parts(rt, a(0), a(1))?;
            Some(new_path(rt, &joined))
        }
        PATH => {
            let p = text(rt, a(0))?;
            path_method(rt, name, &p, args)
        }
        FILES => files_method(rt, name, desc, args),
        FILE if name == "<init>" => {
            let joined = match desc {
                "(Ljava/lang/String;)V" => text(rt, a(1))?,
                // `new File(parent, child)`: a null parent is the child alone.
                "(Ljava/io/File;Ljava/lang/String;)V" | "(Ljava/lang/String;Ljava/lang/String;)V" => {
                    let child = text(rt, a(2))?;
                    match text(rt, a(1)) {
                        Some(up) if a(1).r() != 0 => resolve(&up, &child),
                        _ => child,
                    }
                }
                _ => return None,
            };
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.class = FILE;
                o.data = Data::Str(clean(&joined));
            }
            Some(V::Void)
        }
        FILE => {
            let f = text(rt, a(0))?;
            file_method(rt, name, &f, args)
        }
        FILE_TIME => match name {
            "toMillis" => Some(rt.get_field(a(0), "$millis")),
            "compareTo" => {
                let (x, y) = (rt.get_field(a(0), "$millis").j(), rt.get_field(a(1), "$millis").j());
                Some(V::I(x.cmp(&y) as i32))
            }
            "fromMillis" => {
                let t = rt.new_object(FILE_TIME);
                rt.put_field(t, "$millis", a(0));
                Some(t)
            }
            _ => None,
        },
        FML_PATHS => match name {
            "get" => fml_path(rt, a(0), true),
            "relative" => fml_path(rt, a(0), false),
            "getOrCreateGameRelativePath" => {
                let p = text(rt, a(0))?;
                let dir = game_dir(rt).to_string_lossy().into_owned();
                let full = resolve(&dir, &p);
                let _ = std::fs::create_dir_all(&full);
                Some(new_path(rt, &full))
            }
            "values" => {
                let arr = rt.new_array(FML_FOLDERS.len() as i32, V::NULL);
                for (i, (n, _, _)) in FML_FOLDERS.iter().enumerate() {
                    let c = crate::api::api_enum(rt, FML_PATHS, n, i as i32);
                    rt.array_set(arr, i as i32, c);
                }
                Some(arr)
            }
            _ => None,
        },
        _ => None,
    }
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (FML_PATHS, _) => {
            let i = FML_FOLDERS.iter().position(|(n, _, _)| *n == field)?;
            Some(crate::api::api_enum(rt, FML_PATHS, FML_FOLDERS[i].0, i as i32))
        }
        (FILE, "separator") => Some(rt.string("/")),
        (FILE, "separatorChar") => Some(V::I('/' as i32)),
        (FILE, "pathSeparator") => Some(rt.string(":")),
        (FILE, "pathSeparatorChar") => Some(V::I(':' as i32)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// A game folder of its own for one test.
    fn game(rt: &mut Rt, test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-compat-files-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let v = rt.string(&dir.to_string_lossy());
        rt.statics.insert(GAME_DIR, v);
        dir
    }

    #[test]
    fn paths_resolve_and_split_as_javas_unix_paths_do() {
        assert_eq!(clean("a//b/"), "a/b");
        assert_eq!(clean("/"), "/");
        assert_eq!(resolve("a/b", "c"), "a/b/c");
        assert_eq!(resolve("a", "/x"), "/x");
        assert_eq!(resolve("", "c"), "c");
        assert_eq!(parent("a/b/c").as_deref(), Some("a/b"));
        assert_eq!(parent("/a").as_deref(), Some("/"));
        assert_eq!(parent("a"), None);
        assert_eq!(parent("/"), None);
        assert_eq!(normalize("a/./b/../c"), "a/c");
        assert_eq!(normalize("../a/.."), "..");
        assert_eq!(normalize("/../a"), "/a");
        assert_eq!(relativize("/a/b", "/a/c/d"), "../c/d");
        assert_eq!(relativize("a", "a/b/c"), "b/c");
        assert!(starts_with("/a/b", "/a"));
        assert!(!starts_with("/a/b", "a"));
        assert!(!starts_with("ab/c", "a"));
        assert!(ends_with("a/b/c", "b/c"));
        assert!(!ends_with("a/bc", "c"));
    }

    #[test]
    fn a_mod_keeps_its_files_in_the_game_folder() {
        let mut rt = Rt::new();
        let dir = game(&mut rt, "keeps");
        // Create's `CreatePaths`: `FMLPaths.GAMEDIR.get().resolve("schematics")`.
        let gamedir = crate::api::get_static(&mut rt, "net/neoforged/fml/loading/FMLPaths.GAMEDIR");
        let root = c(&mut rt, "net/neoforged/fml/loading/FMLPaths.get.()Ljava/nio/file/Path;", &[gamedir]);
        assert_eq!(rt.str_of(root), Some(dir.to_string_lossy().as_ref()));
        let config = crate::api::get_static(&mut rt, "net/neoforged/fml/loading/FMLPaths.CONFIGDIR");
        c(&mut rt, "net/neoforged/fml/loading/FMLPaths.get.()Ljava/nio/file/Path;", &[config]);
        assert!(dir.join("config").is_dir(), "FML's folders are there once asked for");
        let name = rt.string("schematics");
        let schematics = c(&mut rt, "java/nio/file/Path.resolve.(Ljava/lang/String;)Ljava/nio/file/Path;", &[root, name]);
        // Listing a folder that isn't there throws, as Java's does; `catch (IOException)` takes it.
        c(&mut rt, "java/nio/file/Files.list.(Ljava/nio/file/Path;)Ljava/util/stream/Stream;", &[schematics]);
        let thrown = rt.thrown.take().expect("NoSuchFileException");
        assert_eq!(rt.class_of(thrown), "java/nio/file/NoSuchFileException");
        assert!(crate::api_common::api_is_a(rt.class_of(thrown), "java/io/IOException"));
        let none = rt.new_array(0, V::NULL);
        c(&mut rt, "java/nio/file/Files.createDirectories.(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;", &[schematics, none]);
        assert!(dir.join("schematics").is_dir());
        for n in ["b.nbt", "a.nbt"] {
            let n = rt.string(n);
            let file = c(&mut rt, "java/nio/file/Path.resolve.(Ljava/lang/String;)Ljava/nio/file/Path;", &[schematics, n]);
            let body = rt.string("{}");
            c(&mut rt, "java/nio/file/Files.writeString.(Ljava/nio/file/Path;Ljava/lang/CharSequence;[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;", &[file, body, none]);
        }
        let listed = c(&mut rt, "java/nio/file/Files.list.(Ljava/nio/file/Path;)Ljava/util/stream/Stream;", &[schematics]);
        let names: Vec<String> = match &rt.heap[listed.r() as usize].data {
            Data::Stream(items) => items.iter().map(|&p| rt.str_of(p).unwrap().rsplit('/').next().unwrap().to_owned()).collect(),
            _ => panic!("a stream"),
        };
        assert_eq!(names, ["a.nbt", "b.nbt"]);
        let a = rt.string("a.nbt");
        let file = c(&mut rt, "java/nio/file/Path.resolve.(Ljava/lang/String;)Ljava/nio/file/Path;", &[schematics, a]);
        let read = c(&mut rt, "java/nio/file/Files.readString.(Ljava/nio/file/Path;)Ljava/lang/String;", &[file]);
        assert_eq!(rt.str_of(read), Some("{}"));
        let name = c(&mut rt, "java/nio/file/Path.getFileName.()Ljava/nio/file/Path;", &[file]);
        let shown = c(&mut rt, "java/nio/file/Path.toString.()Ljava/lang/String;", &[name]);
        assert_eq!(rt.str_of(shown), Some("a.nbt"));
        // The same file through `java.io.File`.
        let f = rt.new_object("java/io/File");
        let p = rt.string(&dir.join("schematics/a.nbt").to_string_lossy());
        c(&mut rt, "java/io/File.<init>.(Ljava/lang/String;)V", &[f, p]);
        assert_eq!(c(&mut rt, "java/io/File.length.()J", &[f]), V::J(2));
        assert_eq!(c(&mut rt, "java/io/File.delete.()Z", &[f]), V::I(1));
        c(&mut rt, "java/nio/file/Files.delete.(Ljava/nio/file/Path;)V", &[file]);
        let gone = rt.thrown.take().unwrap();
        assert_eq!(rt.class_of(gone), "java/nio/file/NoSuchFileException");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
