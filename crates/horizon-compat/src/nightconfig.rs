//! Night Config (`com.electronwill.nightconfig`), which NeoForge ships and some mods' config
//! libraries use themselves (SpectreLib, inside comforts): its configs in memory, natively. A
//! config is an object holding its values (`$map`, a `LinkedHashMap`, where a nested config is a
//! value) and its comments (`$comments`, by key: a nested value's comment is its own level's, as
//! Night Config keeps them); a path is a list of keys or a dotted string. NeoForge's mod configs
//! and their TOML files are `mod_configs.rs`'s; a file config a mod opens through Night Config
//! itself is a config with a path: a TOML one's `load` reads its file as the mod said to (what
//! to do when there is none, what to keep of the values it has) and its `save` writes it; one
//! of another format (JSON, YAML, HOCON: Night Config's other modules, which NeoForge does not
//! ship) keeps what the mod puts in. A mod may also read and write TOML itself, through the
//! format's parser and writer (`TomlFormat.instance().createParser().parse(text)`,
//! `createWriter().writeToString(config)`): nine of the pack's mods do.
use crate::{Data, Rt, Table, V, jutil};
use std::path::Path;

const NC: &str = "com/electronwill/nightconfig/";
const COMMENTED: &str = "com/electronwill/nightconfig/core/CommentedConfig";
const FILE_CONFIG: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfig";
const BUILDER: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder";
const IN_MEMORY: &str = "com/electronwill/nightconfig/core/InMemoryFormat";
const ENUM_GET: &str = "com/electronwill/nightconfig/core/EnumGetMethod";
const SUPPLIER: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const CONFIG: &str = "com/electronwill/nightconfig/core/Config";
const TOML_FORMAT: &str = "com/electronwill/nightconfig/toml/TomlFormat";
const TOML_PARSER: &str = "com/electronwill/nightconfig/toml/TomlParser";
const TOML_WRITER: &str = "com/electronwill/nightconfig/toml/TomlWriter";
const CONFIG_PARSER: &str = "com/electronwill/nightconfig/core/io/ConfigParser";
const CONFIG_WRITER: &str = "com/electronwill/nightconfig/core/io/ConfigWriter";
const FILE_WATCHER: &str = "com/electronwill/nightconfig/core/file/FileWatcher";
const NOT_FOUND: &str = "com/electronwill/nightconfig/core/file/FileNotFoundAction";
const PARSING: &str = "com/electronwill/nightconfig/core/io/ParsingException";
const WRITING: &str = "com/electronwill/nightconfig/core/io/WritingException";
const NOT_FOUND_RUN: &str = "com/electronwill/nightconfig/core/file/FileNotFoundAction.run.(Ljava/nio/file/Path;Lcom/electronwill/nightconfig/core/ConfigFormat;)Z";
const WRITER_WRITE: (&str, &str) = (
    "java/io/Writer.write.(Ljava/lang/String;)V",
    "write.(Ljava/lang/String;)V",
);

/// Night Config's enums: (class, constants in order).
const ENUMS: [(&str, &[&str]); 4] = [
    (ENUM_GET, &["NAME", "NAME_IGNORECASE", "ORDINAL_OR_NAME", "ORDINAL_OR_NAME_IGNORECASE"]),
    ("com/electronwill/nightconfig/core/ConfigSpec$CorrectionAction", &["ADD", "REPLACE", "REMOVE"]),
    ("com/electronwill/nightconfig/core/io/WritingMode", &["REPLACE", "REPLACE_ATOMIC", "APPEND"]),
    ("com/electronwill/nightconfig/core/io/ParsingMode", &["REPLACE", "MERGE", "ADD"]),
];

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner.starts_with(NC)
}

/// Night Config's enum constants (`EnumGetMethod.NAME_IGNORECASE`) and `FileNotFoundAction`'s.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if !owner.starts_with(NC) {
        return None;
    }
    if let Some((class, names)) = ENUMS.iter().find(|(c, _)| *c == owner) {
        let i = names.iter().position(|n| *n == field)?;
        return Some(crate::api::api_enum(rt, class, names[i], i as i32));
    }
    if owner == NOT_FOUND {
        let f = field.to_owned();
        return Some(rt.intern(format!("compat:nc-file-action/{f}"), move |rt| {
            let a = rt.new_object(NOT_FOUND);
            let n = rt.string(&f);
            rt.put_field(a, "$name", n);
            a
        }));
    }
    None
}

fn new_map(rt: &mut Rt) -> V {
    rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()))
}

/// A config of `class` holding `map` (none: a new one).
pub(crate) fn new_config(rt: &mut Rt, class: &'static str, map: Option<V>) -> V {
    let c = rt.new_object(class);
    init(rt, c, map);
    c
}

fn init(rt: &mut Rt, c: V, map: Option<V>) {
    let m = match map {
        Some(m) if m.r() != 0 => m,
        _ => new_map(rt),
    };
    let comments = new_map(rt);
    rt.put_field(c, "$map", m);
    rt.put_field(c, "$comments", comments);
}

pub(crate) fn is_config(rt: &Rt, v: V) -> bool {
    v.r() != 0 && rt.get_field(v, "$map").r() != 0
}

/// `c`'s values, made if a mod's own config class never ran a constructor of Night Config's.
pub(crate) fn values(rt: &mut Rt, c: V) -> V {
    if !is_config(rt, c) {
        init(rt, c, None);
    }
    rt.get_field(c, "$map")
}

/// A path as Night Config takes it: a list of keys, or a string split at its dots.
fn path(rt: &Rt, p: V) -> Vec<String> {
    if let Some(s) = rt.str_of(p) {
        return s.split('.').map(str::to_owned).collect();
    }
    jutil::elements(rt, p).into_iter().map(|k| rt.str_of(k).unwrap_or("").to_owned()).collect()
}

/// The class of a level made under config `c`: a plain config's (`Config.inMemory()`) is
/// plain, any other's holds comments.
fn sub_kind(rt: &Rt, c: V) -> &'static str {
    if rt.class_of(c) == CONFIG {
        CONFIG
    } else {
        COMMENTED
    }
}

/// The config holding `path`'s last key, and that key: nested configs made on the way when `make`,
/// else None where a level is missing.
pub(crate) fn parent(rt: &mut Rt, config: V, path: &[String], make: bool) -> Option<(V, String)> {
    let (last, levels) = path.split_last()?;
    let mut c = config;
    for k in levels {
        let map = values(rt, c);
        let key = rt.string(k);
        c = match jutil::table_get(rt, map, key) {
            Some(sub) if is_config(rt, sub) => sub,
            // (Of its parent's kind: a plain config's levels are plain, which the TOML writer
            // tells from commented ones.)
            _ if make => {
                let kind = sub_kind(rt, c);
                let sub = new_config(rt, kind, None);
                jutil::table_put(rt, map, key, sub);
                sub
            }
            _ => return None,
        };
    }
    Some((c, last.clone()))
}

/// The value at `p`, None when nothing is there.
fn get(rt: &mut Rt, config: V, p: V) -> Option<V> {
    let path = path(rt, p);
    let (c, k) = parent(rt, config, &path, false)?;
    let map = values(rt, c);
    let key = rt.string(&k);
    jutil::table_get(rt, map, key).filter(|v| v.r() != 0 || !matches!(v, V::R(_)))
}

/// Puts `v` at `p`, making the levels on the way; the value it replaces (null for none).
fn set(rt: &mut Rt, config: V, p: V, v: V) -> V {
    let path = path(rt, p);
    let Some((c, k)) = parent(rt, config, &path, true) else { return V::NULL };
    let map = values(rt, c);
    let key = rt.string(&k);
    let old = jutil::table_get(rt, map, key).unwrap_or(V::NULL);
    jutil::table_put(rt, map, key, v);
    old
}

fn remove(rt: &mut Rt, config: V, p: V) -> V {
    let path = path(rt, p);
    let Some((c, k)) = parent(rt, config, &path, false) else { return V::NULL };
    let map = values(rt, c);
    let key = rt.string(&k);
    jutil::table_remove(rt, map, key).unwrap_or(V::NULL)
}

/// A boxed number's value (`Integer.valueOf(5)`), or the raw one.
fn number(rt: &Rt, v: V) -> V {
    match v {
        V::R(_) => rt.get_field(v, "value"),
        raw => raw,
    }
}

fn as_i64(v: V) -> Option<i64> {
    match v {
        V::I(i) => Some(i64::from(i)),
        V::J(j) => Some(j),
        V::F(f) => Some(f as i64),
        V::D(d) => Some(d as i64),
        _ => None,
    }
}

/// The constant of enum `class` that `value` names (its name, or its ordinal where `method` allows),
/// as `EnumGetMethod.get` finds it; a constant is itself.
pub(crate) fn enum_of(rt: &mut Rt, value: V, class: &str, method: V) -> Option<V> {
    let how = if method.r() != 0 { crate::api_common::enum_name(rt, method, false) } else { V::NULL };
    let how = rt.str_of(how).unwrap_or("NAME").to_owned();
    let constants = crate::jlang::enum_constants(rt, class)?;
    let constants = jutil::elements(rt, constants);
    if constants.contains(&value) {
        return Some(value);
    }
    if let Some(name) = rt.str_of(value).map(str::to_owned) {
        let ignore = how.ends_with("IGNORECASE");
        return constants.into_iter().find(|&c| {
            let n = crate::api_common::enum_name(rt, c, false);
            rt.str_of(n).is_some_and(|n| if ignore { n.eq_ignore_ascii_case(&name) } else { n == name })
        });
    }
    if how.starts_with("ORDINAL")
        && let Some(i) = as_i64(number(rt, value))
    {
        return usize::try_from(i).ok().and_then(|i| constants.get(i).copied());
    }
    None
}

/// The class of the enum `getEnum*` asks for: its `Class` argument's, or its default constant's.
fn enum_class(rt: &Rt, desc: &str, args: &[V]) -> String {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if desc.contains(";Ljava/lang/Class;") {
        rt.str_of(a(2)).unwrap_or("").replace('.', "/")
    } else {
        rt.class_of(a(2)).to_owned()
    }
}

/// `c`'s comments, by key (made with its values if need be).
pub(crate) fn comments(rt: &mut Rt, c: V) -> V {
    values(rt, c);
    match rt.get_field(c, "$comments") {
        m if m.r() != 0 => m,
        _ => {
            let m = new_map(rt);
            rt.put_field(c, "$comments", m);
            m
        }
    }
}

/// Where the comment of `p` is kept: the comments of the level holding its last key (made on the
/// way when `make`), and that key; None where a level is missing.
fn comment_slot(rt: &mut Rt, config: V, p: V, make: bool) -> Option<(V, V)> {
    let path = path(rt, p);
    let (c, k) = parent(rt, config, &path, make)?;
    let comments = comments(rt, c);
    Some((comments, rt.string(&k)))
}

/// A format of Night Config's (`InMemoryFormat.withUniversalSupport()`, `TomlFormat.instance()`):
/// one object per class.
fn format(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("compat:nc-format/{class}"), move |rt| rt.new_object(class))
}

/// Throws Night Config's exception `class` (both are `RuntimeException`s) with `message`.
fn throw(rt: &mut Rt, class: &'static str, message: &str) -> V {
    rt.supers.insert(class, "java/lang/RuntimeException");
    crate::files::throw(rt, class, message)
}

/// Night Config's exception for an error of the disk's, reading or writing: a
/// `WritingException` ("An I/O error occured", its own spelling) whose cause is Java's exception
/// for the error at the file `text`.
fn io_failed(rt: &mut Rt, e: &std::io::Error, text: &str) -> V {
    crate::files::throw_io(rt, e, text);
    let cause = rt.thrown.take().unwrap_or_default();
    let thrown = throw(rt, WRITING, "An I/O error occured");
    if let Some(wrapped) = rt.thrown {
        rt.put_field(wrapped, "cause", cause);
    }
    thrown
}

/// How hard a parser is on what the config it reads into already holds ([`put_parsed`]).
#[derive(Clone, Copy, PartialEq)]
enum Twice {
    /// Nothing is "declared twice": a file config that reads its file into a new config first
    /// (Night Config's default kind, which writes in the background).
    Allowed,
    /// A table the config has already is: a file config that reads straight into itself
    /// (`builder.sync()`).
    Tables,
    /// A table is, and under `MERGE` a value too: the parser asked to read into a config.
    All,
}

/// Puts what config `parsed` holds into `into` as parsing mode `mode` says (none: `REPLACE`):
/// `REPLACE` empties it first, `MERGE` puts each value over what is there, `ADD` only where
/// nothing is; one level deep, each value's comment with it. What the config holds already
/// under a key the file has may be an error, as Night Config 3.8's parser has it (`twice`):
/// false then, its `ParsingException` thrown. (A table written inline in the file is told
/// from one under a header no more once it is read: both count as tables here.)
fn put_parsed(rt: &mut Rt, into: V, parsed: V, mode: V, twice: Twice) -> bool {
    let how = rt
        .str_of(rt.get_field(mode, "$name"))
        .unwrap_or("REPLACE")
        .to_owned();
    if how == "REPLACE" {
        let (values, comments) = (new_map(rt), new_map(rt));
        rt.put_field(into, "$map", values);
        rt.put_field(into, "$comments", comments);
    }
    let (to, from) = (values(rt, into), values(rt, parsed));
    let (to_comments, from_comments) = (comments(rt, into), comments(rt, parsed));
    let pairs = jutil::table_pairs(rt, from);
    if twice != Twice::Allowed {
        for &(k, v) in &pairs {
            if jutil::table_get(rt, to, k).is_none() {
                continue;
            }
            let key = rt.str_of(k).unwrap_or("").to_owned();
            if is_config(rt, v) {
                throw(
                    rt,
                    PARSING,
                    &format!("Table with path [{key}] has been declared twice."),
                );
                return false;
            }
            if how == "MERGE" && twice == Twice::All {
                throw(
                    rt,
                    PARSING,
                    &format!("Invalid TOML data: entry \"[{key}]\" defined twice in its table."),
                );
                return false;
            }
        }
    }
    for (k, v) in pairs {
        if how == "ADD" && jutil::table_get(rt, to, k).is_some() {
            continue;
        }
        jutil::table_put(rt, to, k, v);
        if let Some(comment) = jutil::table_get(rt, from_comments, k) {
            jutil::table_put(rt, to_comments, k, comment);
        }
    }
    true
}

/// What a parser or a file config does about a file that is not there, as its
/// `FileNotFoundAction` says (none: `CREATE_EMPTY`, a file config's own default): whether the
/// file is now there to be read. `CREATE_EMPTY` makes it and reads nothing, `READ_NOTHING`
/// leaves it, `THROW_ERROR` throws (Java's `NoSuchFileException`, inside Night Config's
/// `WritingException`); a mod's own action is run. A thrown error is left in `rt.thrown`.
fn file_missing(rt: &mut Rt, text: &str, action: V) -> bool {
    let disk = crate::files::on_disk(rt, text);
    let named = rt.str_of(rt.get_field(action, "$name")).map(str::to_owned);
    match named.as_deref() {
        Some("READ_NOTHING") => false,
        Some("THROW_ERROR") => {
            let e = std::io::Error::from(std::io::ErrorKind::NotFound);
            io_failed(rt, &e, &disk.to_string_lossy());
            false
        }
        None if action.r() != 0 => {
            let path = crate::files::new_path(rt, text);
            let format = format(rt, TOML_FORMAT);
            rt.call_functional(NOT_FOUND_RUN, action, &[path, format])
                .i()
                != 0
        }
        _ => {
            if let Err(e) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&disk)
            {
                io_failed(rt, &e, text);
            }
            false
        }
    }
}

/// Whether the file at `text` is one of TOML by its name (Night Config's `FormatDetector`).
fn is_toml(text: &str) -> bool {
    text.rsplit('/')
        .next()
        .is_some_and(|name| name.ends_with(".toml"))
}

/// The text of the file at `text`; a thrown `WritingException` for one that cannot be read
/// (Night Config's own name for any I/O error, reading too).
fn file_text(rt: &mut Rt, text: &str) -> Option<String> {
    let disk = crate::files::on_disk(rt, text);
    match std::fs::read(&disk) {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) => {
            io_failed(rt, &e, text);
            None
        }
    }
}

/// TOML `text` as a config; a thrown `ParsingException` for what is none.
fn parsed(rt: &mut Rt, text: &str) -> Option<V> {
    match crate::mod_configs::toml_config(rt, text) {
        Ok(c) => Some(c),
        Err(e) => {
            throw(rt, PARSING, &e);
            None
        }
    }
}

/// `parser.parse(source[, destination, mode][, onFileNotFound][, charset])`: the source's TOML
/// as a new config, or put into `destination`. The source is a string, a reader, a stream, a
/// file or a path; a file that is not there is what its `FileNotFoundAction` makes of it.
fn parse(rt: &mut Rt, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let source = a(1);
    let params = desc.split(')').next().unwrap_or("");
    let into = params.contains("Lcom/electronwill/nightconfig/core/Config;");
    let from_file =
        desc.starts_with("(Ljava/io/File;") || desc.starts_with("(Ljava/nio/file/Path;");
    let empty = |rt: &mut Rt| {
        if into {
            V::Void
        } else {
            let c = new_config(rt, COMMENTED, None);
            let f = format(rt, TOML_FORMAT);
            rt.put_field(c, "$format", f);
            c
        }
    };
    let text = if from_file {
        let path = rt.str_of(source)?.to_owned();
        if !crate::files::on_disk(rt, &path).exists() {
            let action = if into { a(4) } else { a(2) };
            if !file_missing(rt, &path, action) || rt.thrown.is_some() {
                return Some(empty(rt));
            }
        }
        match file_text(rt, &path) {
            Some(text) => text,
            None => return Some(V::Void),
        }
    } else if desc.starts_with("(Ljava/io/InputStream;") {
        String::from_utf8_lossy(&crate::streams::drain(rt, source)?).into_owned()
    } else if desc.starts_with("(Ljava/lang/String;") || desc.starts_with("(Ljava/io/Reader;") {
        crate::resources::take_text(rt, source)?
    } else {
        return None;
    };
    let Some(config) = parsed(rt, &text) else {
        return Some(V::Void);
    };
    if into {
        put_parsed(rt, a(2), config, a(3), Twice::All);
        return Some(V::Void);
    }
    let f = format(rt, TOML_FORMAT);
    rt.put_field(config, "$format", f);
    Some(config)
}

/// `writer.write(config, target[, mode][, charset])` and `writeToString(config)`: the config's
/// TOML into a writer, a stream, a file or a path (written whole, or onto its end with
/// `WritingMode.APPEND`), or as a string. What TOML cannot hold is a `WritingException`.
fn write(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (config, target) = (a(1), a(2));
    let text = match crate::mod_configs::toml_text(rt, config) {
        Ok(text) => text,
        Err(e) => return Some(throw(rt, WRITING, &e)),
    };
    if name == "writeToString" {
        return Some(rt.string(&text));
    }
    let params = desc.split(')').next().unwrap_or("");
    if params.contains("Ljava/io/Writer;") {
        let s = rt.string(&text);
        crate::api::virtual_call(rt, WRITER_WRITE, &[target, s]);
    } else if params.contains("Ljava/io/OutputStream;") {
        crate::streams::write_to(rt, target, text.as_bytes());
    } else if params.contains("Ljava/io/File;") || params.contains("Ljava/nio/file/Path;") {
        let path = rt.str_of(target)?.to_owned();
        let disk = crate::files::on_disk(rt, &path);
        let mode = rt
            .str_of(rt.get_field(a(3), "$name"))
            .unwrap_or("REPLACE")
            .to_owned();
        let appends = mode == "APPEND";
        let written = if appends {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&disk)
                .and_then(|mut file| file.write_all(text.as_bytes()))
        } else {
            std::fs::write(&disk, text.as_bytes())
        };
        if let Err(e) = written {
            io_failed(rt, &e, &path);
            // (The writer's own words for it.)
            if let Some(thrown) = rt.thrown {
                let message = rt.string(&format!("Failed to write ({mode}) the config to: {path}"));
                rt.put_field(thrown, "message", message);
            }
            return Some(V::Void);
        }
    } else {
        return None;
    }
    Some(V::Void)
}

/// A file config's `load()`, as Night Config 3.8's two kinds do it. The default kind parses
/// its TOML file into a new config and puts that into itself as its parsing mode says
/// (`REPLACE` unless asked: what it held is gone, also when there was no file to read). One
/// built `sync()` reads straight into itself: with no file read it keeps what it holds, and a
/// table it holds already that the file declares is an error under `MERGE` and `ADD`. A file
/// that is not there is what the builder said of that: by default it is made, empty, and
/// nothing is read; a default file or resource given is copied there first and read.
fn load_file(rt: &mut Rt, config: V) {
    let Some(path) = rt.str_of(rt.get_field(config, "$path")).map(str::to_owned) else {
        return;
    };
    if !is_toml(&path) {
        return;
    }
    let sync = rt.get_field(config, "$sync").i() != 0;
    let disk = crate::files::on_disk(rt, &path);
    let mut there = disk.exists();
    if !there {
        let default = rt.get_field(config, "$default");
        let copied = match rt.str_of(default).map(str::to_owned) {
            // A resource of the mod's (`defaultResource("/default.toml")`), or a file.
            Some(from) if rt.get_field(config, "$defaultIsResource").i() != 0 => {
                crate::resources::jar_file(rt, from.trim_start_matches('/'))
                    .map(|bytes| std::fs::write(&disk, bytes))
            }
            Some(from) => {
                let source = crate::files::on_disk(rt, &from);
                source
                    .exists()
                    .then(|| std::fs::copy(&source, &disk).map(|_| ()))
            }
            None => None,
        };
        there = match copied {
            Some(Ok(())) => true,
            Some(Err(e)) => {
                io_failed(rt, &e, &path);
                return;
            }
            None => {
                let action = rt.get_field(config, "$missing");
                file_missing(rt, &path, action)
            }
        };
        if rt.thrown.is_some() {
            return;
        }
    }
    let read = if there {
        let Some(text) = file_text(rt, &path) else {
            return;
        };
        let Some(read) = parsed(rt, &text) else {
            return;
        };
        read
    } else if sync {
        return;
    } else {
        new_config(rt, COMMENTED, None)
    };
    let mode = rt.get_field(config, "$parsing");
    let twice = if sync { Twice::Tables } else { Twice::Allowed };
    put_parsed(rt, config, read, mode, twice);
}

/// A file config's `save()`: its TOML file written whole. (Night Config's default kind writes
/// a moment later, in the background; here the file is written as the call ends.)
fn save_file(rt: &mut Rt, config: V) {
    let Some(path) = rt.str_of(rt.get_field(config, "$path")).map(str::to_owned) else {
        return;
    };
    if !is_toml(&path) {
        return;
    }
    let disk = crate::files::on_disk(rt, &path);
    // (Its folder is the mod's to make: Night Config does not, and neither does this.)
    if !disk.parent().is_none_or(Path::is_dir) {
        let e = std::io::Error::from(std::io::ErrorKind::NotFound);
        io_failed(rt, &e, &path);
        if let Some(thrown) = rt.thrown {
            let message = rt.string(&format!("Failed to write (REPLACE) the config to: {path}"));
            rt.put_field(thrown, "message", message);
        }
        return;
    }
    if let Err(e) = crate::mod_configs::write_toml_file(rt, &disk, config) {
        throw(rt, WRITING, &e);
    }
}

/// Saves `config` after a change, where it is a file config built to save itself
/// (`builder.autosave()`).
fn autosave(rt: &mut Rt, config: V) {
    if rt.get_field(config, "$autosave").i() != 0 && rt.thrown.is_none() {
        save_file(rt, config);
    }
}

/// `config.addAll(other)`: the values of `other` under the keys this one has nothing for (one
/// level: a table both have stays this one's, as Night Config's `putIfAbsent` leaves it).
fn add_all(rt: &mut Rt, config: V, other: V) {
    let (to, from) = (values(rt, config), values(rt, other));
    for (k, v) in jutil::table_pairs(rt, from) {
        if jutil::table_get(rt, to, k).is_none() {
            jutil::table_put(rt, to, k, v);
        }
    }
}

/// `config.putAllComments(other)`: each comment `other` has, set on this one's key, and those
/// of the tables both have, level by level.
fn put_all_comments(rt: &mut Rt, config: V, other: V) {
    let (to, from) = (comments(rt, config), comments(rt, other));
    for (k, comment) in jutil::table_pairs(rt, from) {
        if !comment.is_null() {
            jutil::table_put(rt, to, k, comment);
        }
    }
    let (mine, theirs) = (values(rt, config), values(rt, other));
    for (k, v) in jutil::table_pairs(rt, theirs) {
        if is_config(rt, v)
            && let Some(sub) = jutil::table_get(rt, mine, k).filter(|&sub| is_config(rt, sub))
        {
            put_all_comments(rt, sub, v);
        }
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !owner.starts_with(NC) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Its enums' `values()` and `valueOf(name)` (a `switch` over one builds its table from
    // `values().length`: unanswered, the table had one place and the second case fell outside).
    if let Some((class, names)) = ENUMS.iter().find(|(c, _)| *c == owner) {
        match name {
            "values" if desc.starts_with("()[") => {
                let all: Vec<V> = names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| crate::api::api_enum(rt, class, n, i as i32))
                    .collect();
                let array = rt.static_key(&format!("[L{class};"));
                return Some(rt.alloc(array, Data::Array(all)));
            }
            "valueOf" if desc.starts_with("(Ljava/lang/String;)") => {
                let wanted = rt.str_of(a(0)).unwrap_or("").to_owned();
                return Some(match names.iter().position(|n| *n == wanted) {
                    Some(i) => crate::api::api_enum(rt, class, names[i], i as i32),
                    None => crate::jlang::throw_with(
                        rt,
                        "java/lang/IllegalArgumentException",
                        &format!(
                            "No enum constant {}.{wanted}",
                            class.replace(['/', '$'], ".")
                        ),
                    ),
                });
            }
            _ => {}
        }
    }
    let commented = owner.contains("Commented") || owner.contains("toml/");
    let config_class: &'static str = if commented { COMMENTED } else { CONFIG };
    // Static factories first: their first argument is no config.
    let made = match name {
        "getDefaultMapCreator" => Some(crate::functions::constructor(rt, "java/util/LinkedHashMap", "()V")),
        "withUniversalSupport" | "withSupport" | "defaultInstance" if owner == IN_MEMORY => Some(format(rt, IN_MEMORY)),
        "instance" if owner.ends_with("Format") => Some(format(rt, owner)),
        "setInsertionOrderPreserved" => Some(V::Void),
        // The watcher of files changed on disk: one for all, which watches nothing (a file
        // edited while the game runs is read when the game next starts).
        "defaultInstance" if owner == FILE_WATCHER => {
            Some(rt.intern("compat:nc-file-watcher".into(), |rt| {
                rt.new_object(FILE_WATCHER)
            }))
        }
        "isInsertionOrderPreserved" => Some(V::I(1)),
        "inMemory" | "inMemoryUniversal" | "inMemoryConcurrent" | "inMemoryUniversalConcurrent" => Some(new_config(rt, config_class, None)),
        // `TomlFormat.newConfig()` and `newConfig(mapCreator)` (the concurrent ones too): an
        // empty config of the format's kind. (Moonlight makes the default of a config entry
        // that holds an object in one: unanswered, the default was null and the whole file
        // could not be written, "TOML doesn't support null values".)
        "newConfig" | "newConcurrentConfig" if owner.ends_with("Format") => {
            let map = if desc.starts_with("(Ljava/util/function/Supplier;") {
                Some(rt.call_functional(SUPPLIER, a(0), &[]))
            } else {
                None
            };
            let c = new_config(rt, config_class, map);
            let f = format(rt, owner);
            rt.put_field(c, "$format", f);
            Some(c)
        }
        // `Config.of(mapCreator, format)`, `Config.of(format)`, `Config.wrap(map, format)`.
        "of" | "wrap" if owner.ends_with("core/Config") || owner.ends_with("core/CommentedConfig") => {
            let map = if desc.starts_with("(Ljava/util/function/Supplier;") {
                Some(rt.call_functional(SUPPLIER, a(0), &[]))
            } else if desc.starts_with("(Ljava/util/Map;") {
                Some(a(0))
            } else {
                None
            };
            let c = new_config(rt, config_class, map);
            if desc.contains("ConfigFormat;)")
                && let Some(&f) = args.last()
            {
                rt.put_field(c, "$format", f);
            }
            Some(c)
        }
        // `Config.copy(config[, format])`: a new config of the same values (nested ones shared).
        "copy" if owner.ends_with("core/Config") || owner.ends_with("core/CommentedConfig") => {
            let from = values(rt, a(0));
            let c = new_config(rt, config_class, None);
            let to = values(rt, c);
            for (k, v) in jutil::table_pairs(rt, from) {
                jutil::table_put(rt, to, k, v);
            }
            Some(c)
        }
        // A file config and its builder: the path kept, nothing read or written.
        "of" | "builder" if owner.contains("/file/") => {
            let b = rt.new_object(if name == "of" { FILE_CONFIG } else { BUILDER });
            if name == "of" {
                init(rt, b, None);
            }
            rt.put_field(b, "$path", a(0));
            Some(b)
        }
        _ => None,
    };
    if made.is_some() {
        return made;
    }
    let this = a(0);
    // A file config's builder: `build()` makes the config; the rest set how it reads and writes.
    if owner.contains("Builder") && owner.contains("/file/") {
        return Some(match name {
            "<init>" => {
                rt.put_field(this, "$path", a(1));
                V::Void
            }
            "build" => {
                let c = new_config(rt, FILE_CONFIG, None);
                for field in [
                    "$path",
                    "$sync",
                    "$autosave",
                    "$missing",
                    "$default",
                    "$defaultIsResource",
                    "$parsing",
                ] {
                    let v = rt.get_field(this, field);
                    rt.put_field(c, field, v);
                }
                c
            }
            // What `load` and `save` go by.
            "sync" => {
                rt.put_field(this, "$sync", V::I(1));
                this
            }
            "autosave" => {
                rt.put_field(this, "$autosave", V::I(1));
                this
            }
            "onFileNotFound" => {
                rt.put_field(this, "$missing", a(1));
                this
            }
            "parsingMode" => {
                rt.put_field(this, "$parsing", a(1));
                this
            }
            "defaultResource" => {
                rt.put_field(this, "$default", a(1));
                rt.put_field(this, "$defaultIsResource", V::I(1));
                this
            }
            "defaultData" if !desc.starts_with("(Ljava/net/URL;") => {
                rt.put_field(this, "$default", a(1));
                rt.put_field(this, "$defaultIsResource", V::I(0));
                this
            }
            _ if desc.ends_with("Builder;") => this,
            _ => return None,
        });
    }
    // TOML read and written by the mod itself, through the format's parser and writer.
    if matches!(owner, TOML_PARSER | CONFIG_PARSER) {
        return match name {
            "<init>" => Some(V::Void),
            "parse" => parse(rt, desc, args),
            "getFormat" => Some(format(rt, TOML_FORMAT)),
            // (How lenient it is with keys: it reads what Night Config's reads.)
            _ if name.starts_with("set") && desc.ends_with(")V") => Some(V::Void),
            _ if name.starts_with("set") => Some(this),
            _ => None,
        };
    }
    if matches!(owner, TOML_WRITER | CONFIG_WRITER) {
        return match name {
            "<init>" => Some(V::Void),
            "write" | "writeToString" => write(rt, name, desc, args),
            // (How it indents and where it puts tables: it writes as NeoForge's writer does.)
            _ if name.starts_with("set") && desc.ends_with(")V") => Some(V::Void),
            _ if name.starts_with("set") => Some(this),
            _ => None,
        };
    }
    if owner == FILE_WATCHER {
        return match name {
            "<init>" | "addWatch" | "setWatch" | "removeWatch" | "stop" => Some(V::Void),
            _ => None,
        };
    }
    // Formats: in memory or TOML, each making configs.
    if owner.ends_with("Format") {
        return Some(match name {
            "createConfig" | "createConcurrentConfig" => {
                let map = if desc.starts_with("(Ljava/util/function/Supplier;") { Some(rt.call_functional(SUPPLIER, a(1), &[])) } else { None };
                new_config(rt, if owner.contains("toml/") { COMMENTED } else { CONFIG }, map)
            }
            "createParser" if owner.contains("toml/") || rt.class_of(this).contains("toml/") => {
                rt.new_object(TOML_PARSER)
            }
            "createWriter" if owner.contains("toml/") || rt.class_of(this).contains("toml/") => {
                rt.new_object(TOML_WRITER)
            }
            "supportsComments" => V::I(owner.contains("toml/") as i32),
            "supportsType" | "isInMemory" => V::I(1),
            "initEmptyFile" => V::Void,
            _ => return None,
        });
    }
    if owner == ENUM_GET {
        return Some(match name {
            "get" => {
                let class = rt.str_of(a(2)).unwrap_or("").replace('.', "/");
                enum_of(rt, a(1), &class, this).unwrap_or(V::NULL)
            }
            "isEnumValue" => {
                let class = rt.str_of(a(2)).unwrap_or("").replace('.', "/");
                V::I(enum_of(rt, a(1), &class, this).is_some() as i32)
            }
            // `validate(value, class)`: whether `get` would find a constant; nothing at all
            // passes (Night Config's own: a null is no wrong value).
            "validate" => {
                let class = rt.str_of(a(2)).unwrap_or("").replace('.', "/");
                V::I((a(1).is_null() || enum_of(rt, a(1), &class, this).is_some()) as i32)
            }
            _ => return None,
        });
    }
    // An entry of a config's `entrySet()`: the map's own entries.
    if owner.ends_with("$Entry") && !is_config(rt, this) {
        let k = match name {
            "getKey" => "java/util/Map$Entry.getKey.()Ljava/lang/Object;",
            "getValue" | "getRawValue" => "java/util/Map$Entry.getValue.()Ljava/lang/Object;",
            "setValue" => "java/util/Map$Entry.setValue.(Ljava/lang/Object;)Ljava/lang/Object;",
            _ => return None,
        };
        return Some(crate::api::call(rt, k, args));
    }
    Some(match name {
        // `super(...)` from a mod's own config class (`AbstractConfig(map)`, `(mapCreator)`, `(concurrent)`).
        "<init>" => {
            let map = if desc.starts_with("(Ljava/util/Map;") {
                Some(a(1))
            } else if desc.starts_with("(Ljava/util/function/Supplier;") {
                Some(rt.call_functional(SUPPLIER, a(1), &[]))
            } else {
                None
            };
            init(rt, this, map);
            V::Void
        }
        "set" | "update" if args.len() == 3 => {
            let old = set(rt, this, a(1), a(2));
            autosave(rt, this);
            old
        }
        "add" if args.len() == 3 => {
            let there = get(rt, this, a(1)).is_some();
            if !there {
                set(rt, this, a(1), a(2));
                autosave(rt, this);
            }
            V::I(!there as i32)
        }
        // `putAll(other)`: each of its values put over this one's; `addAll(other)`: those this
        // one has none for; `removeAll(other)`: this one's under the keys the other has.
        "putAll" | "addAll" | "removeAll" if is_config(rt, a(1)) => {
            if name == "addAll" {
                add_all(rt, this, a(1));
            } else {
                let (to, from) = (values(rt, this), values(rt, a(1)));
                for (k, v) in jutil::table_pairs(rt, from) {
                    if name == "putAll" {
                        jutil::table_put(rt, to, k, v);
                    } else {
                        jutil::table_remove(rt, to, k);
                    }
                }
            }
            autosave(rt, this);
            V::Void
        }
        "putAllComments" if is_config(rt, a(1)) => {
            put_all_comments(rt, this, a(1));
            V::Void
        }
        "get" | "getRaw" => get(rt, this, a(1)).unwrap_or(V::NULL),
        "getOptional" => {
            let v = get(rt, this, a(1)).unwrap_or(V::NULL);
            crate::api_common::optional(rt, v)
        }
        "getOrElse" => match get(rt, this, a(1)) {
            Some(v) => v,
            None if desc.contains(";Ljava/util/function/Supplier;)") => rt.call_functional(SUPPLIER, a(2), &[]),
            None => a(2),
        },
        "getInt" | "getLong" | "getByte" | "getShort" | "getChar" | "getIntOrElse" | "getLongOrElse" | "getByteOrElse" | "getShortOrElse" | "getCharOrElse" => {
            let long = name.starts_with("getLong");
            let found = get(rt, this, a(1)).and_then(|v| as_i64(number(rt, v)));
            let n = match found {
                Some(n) => n,
                None if name.ends_with("OrElse") => {
                    let d = if desc.contains("Supplier;)") {
                        let k = if long { "java/util/function/LongSupplier.getAsLong.()J" } else { "java/util/function/IntSupplier.getAsInt.()I" };
                        rt.call_functional(k, a(2), &[])
                    } else {
                        a(2)
                    };
                    as_i64(d).unwrap_or(0)
                }
                None => 0,
            };
            if long { V::J(n) } else { V::I(n as i32) }
        }
        "getEnum" | "getEnumOrElse" | "getOptionalEnum" => {
            let class = enum_class(rt, desc, args);
            let method = args.iter().copied().find(|&v| v.r() != 0 && rt.class_of(v) == ENUM_GET).unwrap_or(V::NULL);
            let found = get(rt, this, a(1)).and_then(|v| enum_of(rt, v, &class, method));
            match (name, found) {
                ("getOptionalEnum", v) => crate::api_common::optional(rt, v.unwrap_or(V::NULL)),
                (_, Some(v)) => v,
                ("getEnumOrElse", None) if desc.contains("Supplier;)") => rt.call_functional(SUPPLIER, args[args.len() - 1], &[]),
                ("getEnumOrElse", None) => a(2),
                _ => V::NULL,
            }
        }
        "contains" => V::I(get(rt, this, a(1)).is_some() as i32),
        "isNull" => V::I(get(rt, this, a(1)).is_none() as i32),
        "remove" if args.len() == 2 => {
            let old = remove(rt, this, a(1));
            autosave(rt, this);
            old
        }
        "clear" => {
            let fresh = new_map(rt);
            rt.put_field(this, "$map", fresh);
            autosave(rt, this);
            V::Void
        }
        "valueMap" => values(rt, this),
        "entrySet" => {
            let map = values(rt, this);
            crate::api::call(rt, "java/util/Map.entrySet.()Ljava/util/Set;", &[map])
        }
        "size" => {
            let map = values(rt, this);
            V::I(jutil::table_pairs(rt, map).len() as i32)
        }
        "isEmpty" => {
            let map = values(rt, this);
            V::I(jutil::table_pairs(rt, map).is_empty() as i32)
        }
        "createSubConfig" => {
            let kind = sub_kind(rt, this);
            new_config(rt, kind, None)
        }
        "configFormat" => match rt.get_field(this, "$format") {
            f if f.r() != 0 => f,
            _ => format(rt, IN_MEMORY),
        },
        // The comment kept by the level holding the path's last key; a null one removed. The
        // previous comment.
        "setComment" => match comment_slot(rt, this, a(1), true) {
            Some((comments, k)) if a(2).is_null() => {
                jutil::table_remove(rt, comments, k).unwrap_or(V::NULL)
            }
            Some((comments, k)) => jutil::table_put(rt, comments, k, a(2)),
            None => V::NULL,
        },
        "getComment" | "removeComment" | "containsComment" => {
            match (name, comment_slot(rt, this, a(1), false)) {
                ("getComment", Some((comments, k))) => {
                    jutil::table_get(rt, comments, k).unwrap_or(V::NULL)
                }
                ("removeComment", Some((comments, k))) => {
                    jutil::table_remove(rt, comments, k).unwrap_or(V::NULL)
                }
                ("containsComment", Some((comments, k))) => {
                    V::I(jutil::table_get(rt, comments, k).is_some_and(|c| !c.is_null()) as i32)
                }
                ("containsComment", None) => V::I(0),
                _ => V::NULL,
            }
        }
        // This level's comments, by key.
        "commentMap" => comments(rt, this),
        "clearComments" => {
            let fresh = new_map(rt);
            rt.put_field(this, "$comments", fresh);
            V::Void
        }
        "checked" | "unmodifiable" => this,
        // A file config: its file read into it, and written of it.
        "load" => {
            load_file(rt, this);
            V::Void
        }
        "save" => {
            save_file(rt, this);
            V::Void
        }
        "close" => V::Void,
        "getNioPath" | "getFile" => rt.get_field(this, "$path"),
        "toString" => {
            let map = values(rt, this);
            let n = jutil::table_pairs(rt, map).len();
            rt.string(&format!("{} with {n} values", owner.rsplit('/').next().unwrap_or("Config")))
        }
        "hashCode" => V::I(this.r() as i32),
        "equals" => V::I((this == a(1)) as i32),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: &str = "com/electronwill/nightconfig/toml/TomlFormat.instance.()Lcom/electronwill/nightconfig/toml/TomlFormat;";
    const NEW_CONFIG: &str = "com/electronwill/nightconfig/toml/TomlFormat.newConfig.()Lcom/electronwill/nightconfig/core/CommentedConfig;";
    const IN_MEMORY_CONFIG: &str = "com/electronwill/nightconfig/core/Config.inMemory.()Lcom/electronwill/nightconfig/core/Config;";
    const SET: &str = "com/electronwill/nightconfig/core/Config.set.(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;";
    const GET: &str =
        "com/electronwill/nightconfig/core/Config.get.(Ljava/lang/String;)Ljava/lang/Object;";
    const SET_COMMENT: &str = "com/electronwill/nightconfig/core/CommentedConfig.setComment.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;";
    const GET_COMMENT: &str = "com/electronwill/nightconfig/core/CommentedConfig.getComment.(Ljava/lang/String;)Ljava/lang/String;";
    const PARSE_TEXT: &str = "com/electronwill/nightconfig/toml/TomlParser.parse.(Ljava/lang/String;)Lcom/electronwill/nightconfig/core/Config;";
    const PARSE_INTO: &str = "com/electronwill/nightconfig/toml/TomlParser.parse.(Ljava/io/Reader;Lcom/electronwill/nightconfig/core/Config;Lcom/electronwill/nightconfig/core/io/ParsingMode;)V";
    const PARSE_PATH: &str = "com/electronwill/nightconfig/toml/TomlParser.parse.(Ljava/nio/file/Path;Lcom/electronwill/nightconfig/core/file/FileNotFoundAction;)Lcom/electronwill/nightconfig/core/Config;";
    const WRITE_TEXT: &str = "com/electronwill/nightconfig/toml/TomlWriter.writeToString.(Lcom/electronwill/nightconfig/core/UnmodifiableConfig;)Ljava/lang/String;";
    const WRITE_PATH: &str = "com/electronwill/nightconfig/toml/TomlWriter.write.(Lcom/electronwill/nightconfig/core/UnmodifiableConfig;Ljava/nio/file/Path;Lcom/electronwill/nightconfig/core/io/WritingMode;)V";
    const BUILDER_OF: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfig.builder.(Ljava/nio/file/Path;)Lcom/electronwill/nightconfig/core/file/CommentedFileConfigBuilder;";
    const BUILD: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder.build.()Lcom/electronwill/nightconfig/core/file/CommentedFileConfig;";
    const LOAD: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfig.load.()V";
    const SAVE: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfig.save.()V";

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// A runtime whose game folder is a new one of the test's.
    fn game(rt: &mut Rt, test: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hz-nightconfig-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = rt.string(&dir.to_string_lossy());
        rt.statics.insert(crate::GAME_DIR, text);
        dir
    }

    fn set(rt: &mut Rt, config: V, path: &str, value: V) {
        let p = rt.string(path);
        c(rt, SET, &[config, p, value]);
    }

    fn int(rt: &mut Rt, i: i32) -> V {
        crate::mixin::box_value(rt, V::I(i), 'I')
    }

    fn comment(rt: &mut Rt, config: V, path: &str) -> Option<String> {
        let p = rt.string(path);
        let text = c(rt, GET_COMMENT, &[config, p]);
        rt.str_of(text).map(str::to_owned)
    }

    /// Night Config's constant `name` of `class` (an enum's, a `FileNotFoundAction`).
    fn constant(rt: &mut Rt, class: &str, name: &str) -> V {
        let key = rt.static_key(&format!("com/electronwill/nightconfig/core/{class}.{name}"));
        crate::api::get_static(rt, key)
    }

    /// A config as the oracle prints one: `{key=value:Class key={...} }`.
    fn dump(rt: &mut Rt, config: V) -> String {
        let map = values(rt, config);
        let mut out = String::from("{");
        for (k, v) in jutil::table_pairs(rt, map) {
            let key = rt.str_of(k).unwrap_or("?").to_owned();
            let shown = if is_config(rt, v) {
                dump(rt, v)
            } else if let Some(text) = rt.str_of(v) {
                format!("{text}:String")
            } else {
                let class = rt.class_of(v).rsplit('/').next().unwrap_or("");
                let text = match rt.get_field(v, "value") {
                    V::I(i) if class == "Boolean" => (i != 0).to_string(),
                    V::I(i) => i.to_string(),
                    V::J(j) => j.to_string(),
                    V::F(f) => f.to_string(),
                    V::D(d) => d.to_string(),
                    _ => "?".to_owned(),
                };
                format!("{text}:{class}")
            };
            out.push_str(&format!("{key}={shown} "));
        }
        out.push('}');
        out
    }

    /// What was thrown, as the oracle prints it: the class's last name and the message.
    fn thrown(rt: &mut Rt) -> String {
        let e = rt.thrown.take().expect("an exception");
        let class = rt.class_of(e).rsplit('/').next().unwrap_or("").to_owned();
        let message = rt.get_field(e, "message");
        format!("{class}: {}", rt.str_of(message).unwrap_or("null"))
    }

    fn written(rt: &mut Rt, writer: V, config: V) -> String {
        let text = c(rt, WRITE_TEXT, &[writer, config]);
        rt.str_of(text).unwrap_or("").replace("\r\n", "\n")
    }

    fn file(dir: &std::path::Path, name: &str) -> Option<String> {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .map(|text| text.replace("\r\n", "\n"))
    }

    const TEXT: &str = "# top\nname = \"x\"\ncount = 3\nratio = 1.5\non = true\nlist = [1, 2]\n\n[sub]\n\t#c\n\tdeep = \"y\"\n";

    // What these two tests expect is what Night Config 3.8.3 itself printed (the version the
    // game ships), with `Config.setInsertionOrderPreserved(true)` as NeoForge sets it.

    /// A mod reads and writes TOML itself through the format's parser and writer (nine of the
    /// pack's mods): a string, a reader, a file; a new config or one of the mod's; a string, a
    /// file. A plain config's tables are written inline, a commented one's under headers.
    #[test]
    fn toml_is_parsed_and_written_through_the_format() {
        crate::install();
        let mut rt = Rt::new();
        let dir = game(&mut rt, "by-hand");
        let format = c(&mut rt, FORMAT, &[]);
        let parser = c(
            &mut rt,
            "com/electronwill/nightconfig/toml/TomlFormat.createParser.()Lcom/electronwill/nightconfig/toml/TomlParser;",
            &[format],
        );
        let writer = c(
            &mut rt,
            "com/electronwill/nightconfig/toml/TomlFormat.createWriter.()Lcom/electronwill/nightconfig/toml/TomlWriter;",
            &[format],
        );
        let text = rt.string(TEXT);
        let config = c(&mut rt, PARSE_TEXT, &[parser, text]);
        assert!(rt.thrown.is_none());
        let name = rt.string("name");
        let read = c(&mut rt, GET, &[config, name]);
        assert_eq!(rt.str_of(read), Some("x"));
        assert_eq!(comment(&mut rt, config, "name").as_deref(), Some(" top"));
        assert_eq!(comment(&mut rt, config, "sub.deep").as_deref(), Some("c"));
        assert_eq!(
            c(
                &mut rt,
                "com/electronwill/nightconfig/core/Config.configFormat.()Lcom/electronwill/nightconfig/core/ConfigFormat;",
                &[config]
            ),
            format
        );
        assert_eq!(written(&mut rt, writer, config), TEXT);

        // A plain config's levels are plain, and written inline; a commented config's are
        // tables under headers, an empty one inline, a list of configs an array of tables.
        let plain = c(&mut rt, IN_MEMORY_CONFIG, &[]);
        for (path, i) in [("a.b", 1), ("c", 2), ("e.f.g", 3)] {
            let v = int(&mut rt, i);
            set(&mut rt, plain, path, v);
        }
        assert_eq!(
            written(&mut rt, writer, plain),
            "a = {b = 1}\nc = 2\ne = {f = {g = 3}}\n"
        );
        let mixed = c(&mut rt, NEW_CONFIG, &[]);
        set(&mut rt, mixed, "plain", plain);
        let one = int(&mut rt, 1);
        set(&mut rt, mixed, "s.t", one);
        let empty = c(&mut rt, NEW_CONFIG, &[]);
        set(&mut rt, mixed, "empty", empty);
        let (first, second) = (
            c(&mut rt, IN_MEMORY_CONFIG, &[]),
            c(&mut rt, NEW_CONFIG, &[]),
        );
        let list = jutil::new_list(&mut rt, vec![first, second]);
        set(&mut rt, mixed, "list", list);
        assert_eq!(
            written(&mut rt, writer, mixed),
            "plain = {a = {b = 1}, c = 2, e = {f = {g = 3}}}\nempty = {}\n\n[s]\n\tt = 1\n\n[[list]]\n[[list]]\n"
        );

        // Read into a config of the mod's: `REPLACE` empties it first; `ADD` keeps what is
        // there; under `MERGE` a value the config has already is "defined twice", and a table
        // it has is "declared twice" in either.
        let mode = |rt: &mut Rt, name: &'static str| constant(rt, "io/ParsingMode", name);
        let held = |rt: &mut Rt| {
            let dest = c(rt, NEW_CONFIG, &[]);
            let old = rt.string("old");
            set(rt, dest, "name", old);
            dest
        };
        let (replace, merge, add) = (
            mode(&mut rt, "REPLACE"),
            mode(&mut rt, "MERGE"),
            mode(&mut rt, "ADD"),
        );
        let two = rt.string("name = \"x\"\nother = 1\n");
        let dest = held(&mut rt);
        c(&mut rt, PARSE_INTO, &[parser, two, dest, add]);
        assert_eq!(dump(&mut rt, dest), "{name=old:String other=1:Integer }");
        let dest = held(&mut rt);
        c(&mut rt, PARSE_INTO, &[parser, two, dest, merge]);
        assert_eq!(
            thrown(&mut rt),
            "ParsingException: Invalid TOML data: entry \"[name]\" defined twice in its table."
        );
        let dest = held(&mut rt);
        c(&mut rt, PARSE_INTO, &[parser, two, dest, replace]);
        assert_eq!(dump(&mut rt, dest), "{name=x:String other=1:Integer }");
        for how in [merge, add] {
            let dest = c(&mut rt, NEW_CONFIG, &[]);
            let v = int(&mut rt, 2);
            set(&mut rt, dest, "sub.other", v);
            c(&mut rt, PARSE_INTO, &[parser, text, dest, how]);
            assert_eq!(
                thrown(&mut rt),
                "ParsingException: Table with path [sub] has been declared twice."
            );
        }
        let apart = c(&mut rt, NEW_CONFIG, &[]);
        let v = int(&mut rt, 1);
        set(&mut rt, apart, "keep", v);
        let (key, kept) = (rt.string("keep"), rt.string("kept"));
        c(&mut rt, SET_COMMENT, &[apart, key, kept]);
        let small = rt.string("on = true\n[sub]\ndeep = \"y\"\n");
        c(&mut rt, PARSE_INTO, &[parser, small, apart, merge]);
        assert_eq!(
            dump(&mut rt, apart),
            "{keep=1:Integer on=true:Boolean sub={deep=y:String } }"
        );
        assert_eq!(comment(&mut rt, apart, "keep").as_deref(), Some("kept"));

        // A file that is not there: nothing read and nothing made, or made empty, or an error
        // of writing (Night Config's name for any error of the disk) around Java's.
        let action =
            |rt: &mut Rt, name: &'static str| constant(rt, "file/FileNotFoundAction", name);
        let missing = crate::files::new_path(&mut rt, "missing.toml");
        let nothing = action(&mut rt, "READ_NOTHING");
        let read = c(&mut rt, PARSE_PATH, &[parser, missing, nothing]);
        assert_eq!(dump(&mut rt, read), "{}");
        assert!(!dir.join("missing.toml").exists());
        let create = action(&mut rt, "CREATE_EMPTY");
        let read = c(&mut rt, PARSE_PATH, &[parser, missing, create]);
        assert_eq!(dump(&mut rt, read), "{}");
        assert_eq!(file(&dir, "missing.toml").as_deref(), Some(""));
        let none = crate::files::new_path(&mut rt, "none.toml");
        let error = action(&mut rt, "THROW_ERROR");
        c(&mut rt, PARSE_PATH, &[parser, none, error]);
        let e = rt.thrown.expect("a missing file thrown");
        let cause = rt.get_field(e, "cause");
        assert_eq!(rt.class_of(cause), "java/nio/file/NoSuchFileException");
        assert_eq!(thrown(&mut rt), "WritingException: An I/O error occured");
        let broken = rt.string("a = ");
        c(&mut rt, PARSE_TEXT, &[parser, broken]);
        assert!(thrown(&mut rt).starts_with("ParsingException: "));

        // Written to a file whole, then onto its end; what TOML cannot hold, and a folder
        // that is not there, are errors of writing.
        let small = c(&mut rt, IN_MEMORY_CONFIG, &[]);
        let v = int(&mut rt, 1);
        set(&mut rt, small, "k", v);
        let y = rt.string("y");
        set(&mut rt, small, "t.x", y);
        let out = crate::files::new_path(&mut rt, "out.toml");
        let writing = |rt: &mut Rt, name: &'static str| constant(rt, "io/WritingMode", name);
        let (whole, onto) = (writing(&mut rt, "REPLACE"), writing(&mut rt, "APPEND"));
        c(&mut rt, WRITE_PATH, &[writer, small, out, whole]);
        assert_eq!(
            file(&dir, "out.toml").as_deref(),
            Some("k = 1\nt = {x = \"y\"}\n")
        );
        c(&mut rt, WRITE_PATH, &[writer, small, out, onto]);
        assert_eq!(
            file(&dir, "out.toml").as_deref(),
            Some("k = 1\nt = {x = \"y\"}\nk = 1\nt = {x = \"y\"}\n")
        );
        let nowhere = crate::files::new_path(&mut rt, "nodir/x.toml");
        c(&mut rt, WRITE_PATH, &[writer, small, nowhere, whole]);
        assert_eq!(
            thrown(&mut rt),
            "WritingException: Failed to write (REPLACE) the config to: nodir/x.toml"
        );
        set(&mut rt, small, "n", V::NULL);
        c(&mut rt, WRITE_TEXT, &[writer, small]);
        assert_eq!(
            thrown(&mut rt),
            "WritingException: TOML doesn't support null values"
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A mod's own config file through Night Config: read by `load`, written by `save`. The
    /// default kind takes what the file held in place of what it had (nothing, where there was
    /// no file); one built `sync()` reads into itself and keeps what it had where nothing is
    /// read; each puts what is read in as its parsing mode says.
    #[test]
    fn a_file_config_reads_and_writes_its_file() {
        crate::install();
        let mut rt = Rt::new();
        let dir = game(&mut rt, "file-config");
        let built = |rt: &mut Rt, name: &str, sync: bool, with: &[(&'static str, V)]| {
            let path = crate::files::new_path(rt, name);
            let mut b = c(rt, BUILDER_OF, &[path]);
            if sync {
                b = c(
                    rt,
                    "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder.sync.()Lcom/electronwill/nightconfig/core/file/CommentedFileConfigBuilder;",
                    &[b],
                );
            }
            for &(key, arg) in with {
                b = c(rt, key, &[b, arg]);
            }
            c(rt, BUILD, &[b])
        };
        const AUTOSAVE: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder.autosave.()Lcom/electronwill/nightconfig/core/file/CommentedFileConfigBuilder;";
        const ON_MISSING: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder.onFileNotFound.(Lcom/electronwill/nightconfig/core/file/FileNotFoundAction;)Lcom/electronwill/nightconfig/core/file/CommentedFileConfigBuilder;";
        const PARSING_MODE: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder.parsingMode.(Lcom/electronwill/nightconfig/core/io/ParsingMode;)Lcom/electronwill/nightconfig/core/file/CommentedFileConfigBuilder;";
        const DEFAULT_DATA: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder.defaultData.(Ljava/nio/file/Path;)Lcom/electronwill/nightconfig/core/file/CommentedFileConfigBuilder;";

        // No file yet: it is made, empty. The default kind is emptied with it; a `sync()`
        // one keeps what it holds.
        let path = crate::files::new_path(&mut rt, "conf.toml");
        let f = c(
            &mut rt,
            "com/electronwill/nightconfig/core/file/CommentedFileConfig.of.(Ljava/nio/file/Path;)Lcom/electronwill/nightconfig/core/file/CommentedFileConfig;",
            &[path],
        );
        let v = int(&mut rt, 1);
        set(&mut rt, f, "a", v);
        c(&mut rt, LOAD, &[f]);
        assert_eq!(dump(&mut rt, f), "{}");
        assert_eq!(file(&dir, "conf.toml").as_deref(), Some(""));
        let kept = built(&mut rt, "sm.toml", true, &[]);
        let v = int(&mut rt, 1);
        set(&mut rt, kept, "kept", v);
        c(&mut rt, LOAD, &[kept]);
        assert_eq!(dump(&mut rt, kept), "{kept=1:Integer }");
        assert_eq!(file(&dir, "sm.toml").as_deref(), Some(""));
        // Told to read nothing of a file that is not there: none is made; the default kind
        // is emptied all the same.
        let nothing = crate::api::get_static(
            &mut rt,
            "com/electronwill/nightconfig/core/file/FileNotFoundAction.READ_NOTHING",
        );
        let unread = built(&mut rt, "arn.toml", false, &[(ON_MISSING, nothing)]);
        let v = int(&mut rt, 1);
        set(&mut rt, unread, "kept", v);
        c(&mut rt, LOAD, &[unread]);
        assert_eq!(dump(&mut rt, unread), "{}");
        assert!(!dir.join("arn.toml").exists());

        // Saved, and read back by another: a float comes back a double, a long that fits an
        // int; comments are kept, each line of one its own.
        let saved = built(&mut rt, "s.toml", true, &[]);
        let q = crate::mixin::box_value(&mut rt, V::F(1.5), 'F');
        set(&mut rt, saved, "q", q);
        let w = crate::mixin::box_value(&mut rt, V::J(2), 'J');
        set(&mut rt, saved, "w", w);
        let (e, d) = (rt.string("c"), rt.string("d"));
        set(&mut rt, saved, "e", e);
        set(&mut rt, saved, "b.c", d);
        for (key, text) in [("q", "the q"), ("b", " about b"), ("b.c", "c!\nsecond")] {
            let (key, text) = (rt.string(key), rt.string(text));
            c(&mut rt, SET_COMMENT, &[saved, key, text]);
        }
        c(&mut rt, SAVE, &[saved]);
        assert_eq!(
            file(&dir, "s.toml").as_deref(),
            Some(
                "#the q\nq = 1.5\nw = 2\ne = \"c\"\n\n# about b\n[b]\n\t#c!\n\t#second\n\tc = \"d\"\n"
            )
        );
        let back = built(&mut rt, "s.toml", true, &[]);
        c(&mut rt, LOAD, &[back]);
        assert_eq!(
            dump(&mut rt, back),
            "{q=1.5:Double w=2:Integer e=c:String b={c=d:String } }"
        );
        assert_eq!(comment(&mut rt, back, "q").as_deref(), Some("the q"));
        assert_eq!(comment(&mut rt, back, "b").as_deref(), Some(" about b"));
        assert_eq!(comment(&mut rt, back, "b.c").as_deref(), Some("c!\nsecond"));

        // One that saves itself writes at every change.
        let auto = built(&mut rt, "sync.toml", true, &[(AUTOSAVE, V::NULL)]);
        assert!(!dir.join("sync.toml").exists());
        let v = int(&mut rt, 1);
        set(&mut rt, auto, "x", v);
        assert_eq!(file(&dir, "sync.toml").as_deref(), Some("x = 1\n"));
        let x = rt.string("x");
        c(
            &mut rt,
            "com/electronwill/nightconfig/core/Config.remove.(Ljava/lang/String;)Ljava/lang/Object;",
            &[auto, x],
        );
        assert_eq!(file(&dir, "sync.toml").as_deref(), Some(""));

        // What is read, put in as the parsing mode says, by each kind.
        std::fs::write(dir.join("mg.toml"), "a = 5\nc = 6\n[t]\nx = 1\n").unwrap();
        let loaded = |rt: &mut Rt, sync: bool, mode: &'static str, table: bool| {
            let mode = constant(rt, "io/ParsingMode", mode);
            let f = built(rt, "mg.toml", sync, &[(PARSING_MODE, mode)]);
            for (path, i) in [("a", 1), ("b", 2), ("t.y", 3)] {
                if path != "t.y" || table {
                    let v = int(rt, i);
                    set(rt, f, path, v);
                }
            }
            let (key, text) = (rt.string("b"), rt.string("cb"));
            c(rt, SET_COMMENT, &[f, key, text]);
            c(rt, LOAD, &[f]);
            if rt.thrown.is_some() {
                return thrown(rt);
            }
            let comment = comment(rt, f, "b").unwrap_or_else(|| "null".to_owned());
            format!("{} cb={comment}", dump(rt, f))
        };
        for (sync, mode, table, expected) in [
            (
                true,
                "REPLACE",
                true,
                "{a=5:Integer c=6:Integer t={x=1:Integer } } cb=null",
            ),
            (
                true,
                "MERGE",
                true,
                "ParsingException: Table with path [t] has been declared twice.",
            ),
            (
                true,
                "MERGE",
                false,
                "{a=5:Integer b=2:Integer c=6:Integer t={x=1:Integer } } cb=cb",
            ),
            (
                true,
                "ADD",
                true,
                "ParsingException: Table with path [t] has been declared twice.",
            ),
            (
                true,
                "ADD",
                false,
                "{a=1:Integer b=2:Integer c=6:Integer t={x=1:Integer } } cb=cb",
            ),
            (
                false,
                "REPLACE",
                true,
                "{a=5:Integer c=6:Integer t={x=1:Integer } } cb=null",
            ),
            (
                false,
                "MERGE",
                true,
                "{a=5:Integer b=2:Integer t={x=1:Integer } c=6:Integer } cb=cb",
            ),
            (
                false,
                "MERGE",
                false,
                "{a=5:Integer b=2:Integer c=6:Integer t={x=1:Integer } } cb=cb",
            ),
            (
                false,
                "ADD",
                true,
                "{a=1:Integer b=2:Integer t={y=3:Integer } c=6:Integer } cb=cb",
            ),
            (
                false,
                "ADD",
                false,
                "{a=1:Integer b=2:Integer c=6:Integer t={x=1:Integer } } cb=cb",
            ),
        ] {
            assert_eq!(
                loaded(&mut rt, sync, mode, table),
                expected,
                "sync {sync}, {mode}, table {table}"
            );
        }

        // A default file given is copied where none is, and read.
        std::fs::write(dir.join("f.toml"), "k = \"v\"").unwrap();
        let from = crate::files::new_path(&mut rt, "f.toml");
        let defaulted = built(&mut rt, "dd.toml", false, &[(DEFAULT_DATA, from)]);
        c(&mut rt, LOAD, &[defaulted]);
        assert_eq!(dump(&mut rt, defaulted), "{k=v:String }");
        assert_eq!(file(&dir, "dd.toml").as_deref(), Some("k = \"v\""));

        // A folder that is not there is the mod's to make: neither made by `load` nor `save`.
        let nowhere = built(&mut rt, "nodir/x.toml", true, &[]);
        c(&mut rt, LOAD, &[nowhere]);
        let e = rt.thrown.expect("no folder to make the file in");
        let cause = rt.get_field(e, "cause");
        assert_eq!(rt.class_of(cause), "java/nio/file/NoSuchFileException");
        assert_eq!(thrown(&mut rt), "WritingException: An I/O error occured");
        let v = int(&mut rt, 1);
        set(&mut rt, nowhere, "k", v);
        c(&mut rt, SAVE, &[nowhere]);
        assert_eq!(
            thrown(&mut rt),
            "WritingException: Failed to write (REPLACE) the config to: nodir/x.toml"
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `putAll` puts each value over this config's, `addAll` those it has none for and
    /// `removeAll` takes the other's keys out, each one level deep; `putAllComments` goes into
    /// the tables both have; `EnumGetMethod.validate` lets nothing at all pass.
    #[test]
    fn configs_are_put_together_as_night_configs() {
        crate::install();
        let mut rt = Rt::new();
        let made = |rt: &mut Rt, values: &[(&str, i32)]| {
            let config = c(rt, NEW_CONFIG, &[]);
            for &(path, i) in values {
                let v = int(rt, i);
                set(rt, config, path, v);
            }
            config
        };
        let one = made(&mut rt, &[("a", 1), ("t.x", 1), ("t.y", 2)]);
        for (key, text) in [("a", "ca"), ("t.x", "cx")] {
            let (key, text) = (rt.string(key), rt.string(text));
            c(&mut rt, SET_COMMENT, &[one, key, text]);
        }
        let put = made(&mut rt, &[("a", 7), ("b", 8), ("t.x", 9), ("t.z", 10)]);
        c(
            &mut rt,
            "com/electronwill/nightconfig/core/CommentedConfig.putAll.(Lcom/electronwill/nightconfig/core/UnmodifiableConfig;)V",
            &[put, one],
        );
        assert_eq!(
            dump(&mut rt, put),
            "{a=1:Integer b=8:Integer t={x=1:Integer y=2:Integer } }"
        );
        let added = made(&mut rt, &[("a", 7), ("t.x", 9)]);
        c(
            &mut rt,
            "com/electronwill/nightconfig/core/Config.addAll.(Lcom/electronwill/nightconfig/core/UnmodifiableConfig;)V",
            &[added, one],
        );
        assert_eq!(dump(&mut rt, added), "{a=7:Integer t={x=9:Integer } }");
        let removed = made(&mut rt, &[("a", 7), ("b", 8), ("t.x", 9), ("t.z", 10)]);
        c(
            &mut rt,
            "com/electronwill/nightconfig/core/Config.removeAll.(Lcom/electronwill/nightconfig/core/UnmodifiableConfig;)V",
            &[removed, one],
        );
        assert_eq!(dump(&mut rt, removed), "{b=8:Integer }");
        let commented = made(&mut rt, &[("a", 7), ("t.x", 9), ("t.z", 10)]);
        c(
            &mut rt,
            "com/electronwill/nightconfig/core/CommentedConfig.putAllComments.(Lcom/electronwill/nightconfig/core/UnmodifiableCommentedConfig;)V",
            &[commented, one],
        );
        assert_eq!(comment(&mut rt, commented, "a").as_deref(), Some("ca"));
        assert_eq!(comment(&mut rt, commented, "t.x").as_deref(), Some("cx"));
        assert_eq!(comment(&mut rt, commented, "t.z"), None);

        // `validate`: RED and red by name, red ignoring case, 1 and 5 by ordinal or name, 1 by
        // name, nothing, a constant itself.
        const VALIDATE: &str = "com/electronwill/nightconfig/core/EnumGetMethod.validate.(Ljava/lang/Object;Ljava/lang/Class;)Z";
        let class = crate::jlang::class_object(&mut rt, "net/minecraft/world/entity/EquipmentSlot");
        let method = |rt: &mut Rt, name: &'static str| constant(rt, "EnumGetMethod", name);
        let (by_name, ignoring, ordinal) = (
            method(&mut rt, "NAME"),
            method(&mut rt, "NAME_IGNORECASE"),
            method(&mut rt, "ORDINAL_OR_NAME"),
        );
        let (upper, lower) = (rt.string("HEAD"), rt.string("head"));
        let (first, far) = (int(&mut rt, 1), int(&mut rt, 50));
        let constant =
            crate::api::get_static(&mut rt, "net/minecraft/world/entity/EquipmentSlot.HEAD");
        let answers: Vec<V> = [
            (by_name, upper),
            (by_name, lower),
            (ignoring, lower),
            (ordinal, first),
            (by_name, first),
            (by_name, V::NULL),
            (by_name, constant),
            (ordinal, far),
        ]
        .into_iter()
        .map(|(how, value)| c(&mut rt, VALIDATE, &[how, value, class]))
        .collect();
        assert_eq!(answers, [1, 0, 1, 1, 0, 1, 1, 0].map(V::I));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Moonlight's way (Amendments' configs): the default of an entry that holds an object is a
    /// config its format makes, `TomlFormat.newConfig()`: empty, of its own, and taking values.
    #[test]
    fn a_format_makes_an_empty_config_of_its_own() {
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args);
        const NEW: &str = "com/electronwill/nightconfig/toml/TomlFormat.newConfig.()Lcom/electronwill/nightconfig/core/CommentedConfig;";
        const SIZE: &str = "com/electronwill/nightconfig/core/Config.size.()I";
        let made = c(&mut rt, NEW, &[]);
        assert_ne!(made, V::NULL);
        assert_eq!(c(&mut rt, SIZE, &[made]), V::I(0));
        let (key, value) = (rt.string("fluid.color"), rt.string("red"));
        c(
            &mut rt,
            "com/electronwill/nightconfig/core/Config.set.(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
            &[made, key, value],
        );
        assert_eq!(
            c(
                &mut rt,
                "com/electronwill/nightconfig/core/Config.get.(Ljava/lang/String;)Ljava/lang/Object;",
                &[made, key]
            ),
            value
        );
        assert_eq!(c(&mut rt, SIZE, &[made]), V::I(1));
        let other = c(&mut rt, NEW, &[]);
        assert_ne!(other, made);
        assert_eq!(c(&mut rt, SIZE, &[other]), V::I(0));
        // Its format is the one that made it.
        let format = c(
            &mut rt,
            "com/electronwill/nightconfig/toml/TomlFormat.instance.()Lcom/electronwill/nightconfig/toml/TomlFormat;",
            &[],
        );
        assert_eq!(
            c(
                &mut rt,
                "com/electronwill/nightconfig/core/Config.configFormat.()Lcom/electronwill/nightconfig/core/ConfigFormat;",
                &[made]
            ),
            format
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// SpectreLib's way (comforts): a config of the default map creator, values set by path lists
    /// and dotted strings into nested configs, read back with defaults for what isn't there; an enum
    /// stored as its name read back as the constant, ignoring case as asked; comments by path, each
    /// kept by its own level.
    #[test]
    fn a_config_keeps_values_by_path_and_reads_enums_by_name() {
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args);
        let creator = c(&mut rt, "com/electronwill/nightconfig/core/Config.getDefaultMapCreator.(ZZ)Ljava/util/function/Supplier;", &[V::I(0), V::I(0)]);
        let universal = c(&mut rt, "com/electronwill/nightconfig/core/InMemoryFormat.withUniversalSupport.()Lcom/electronwill/nightconfig/core/InMemoryFormat;", &[]);
        let config = c(&mut rt, "com/electronwill/nightconfig/core/Config.of.(Ljava/util/function/Supplier;Lcom/electronwill/nightconfig/core/ConfigFormat;)Lcom/electronwill/nightconfig/core/Config;", &[creator, universal]);
        let other = c(&mut rt, "com/electronwill/nightconfig/core/Config.of.(Ljava/util/function/Supplier;Lcom/electronwill/nightconfig/core/ConfigFormat;)Lcom/electronwill/nightconfig/core/Config;", &[creator, universal]);
        let (general, enabled) = (rt.string("general"), rt.string("enabled"));
        let path = crate::jutil::new_list(&mut rt, vec![general, enabled]);
        let on = crate::api_common::boolean(&mut rt, true);
        let old = c(&mut rt, "com/electronwill/nightconfig/core/Config.set.(Ljava/util/List;Ljava/lang/Object;)Ljava/lang/Object;", &[config, path, on]);
        assert_eq!(old, V::NULL);
        let dotted = rt.string("general.enabled");
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.get.(Ljava/lang/String;)Ljava/lang/Object;", &[config, dotted]), on);
        // The other config, of its own new map, has none of it.
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.contains.(Ljava/lang/String;)Z", &[other, dotted]), V::I(0));
        let general_only = rt.string("general");
        let sub = c(&mut rt, "com/electronwill/nightconfig/core/Config.get.(Ljava/lang/String;)Ljava/lang/Object;", &[config, general_only]);
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.size.()I", &[sub]), V::I(1));
        let missing = rt.string("general.radius");
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.getIntOrElse.(Ljava/lang/String;I)I", &[config, missing, V::I(16)]), V::I(16));

        let (slot_key, head) = (rt.string("slot"), rt.string("Head"));
        c(&mut rt, "com/electronwill/nightconfig/core/Config.set.(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;", &[config, slot_key, head]);
        let class = crate::jlang::class_object(&mut rt, "net/minecraft/world/entity/EquipmentSlot");
        let ignore_case = crate::api::get_static(&mut rt, "com/electronwill/nightconfig/core/EnumGetMethod.NAME_IGNORECASE");
        let slot = c(
            &mut rt,
            "com/electronwill/nightconfig/core/Config.getEnumOrElse.(Ljava/util/List;Ljava/lang/Class;Lcom/electronwill/nightconfig/core/EnumGetMethod;Ljava/util/function/Supplier;)Ljava/lang/Enum;",
            &[config, slot_key, class, ignore_case, V::NULL],
        );
        assert_eq!(slot, crate::api::get_static(&mut rt, "net/minecraft/world/entity/EquipmentSlot.HEAD"));
        let by_name = crate::api::get_static(&mut rt, "com/electronwill/nightconfig/core/EnumGetMethod.NAME");
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/EnumGetMethod.get.(Ljava/lang/Object;Ljava/lang/Class;)Ljava/lang/Enum;", &[by_name, head, class]), V::NULL);

        let note = rt.string("Whether sleeping bags work");
        c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.setComment.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[config, dotted, note]);
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.getComment.(Ljava/lang/String;)Ljava/lang/String;", &[config, dotted]), note);
        // Kept by the level holding it, under its own key, as Night Config keeps it; a null one
        // removes it.
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.getComment.(Ljava/lang/String;)Ljava/lang/String;", &[sub, enabled]), note);
        let previous = c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.setComment.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[config, dotted, V::NULL]);
        assert_eq!(previous, note);
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.containsComment.(Ljava/lang/String;)Z", &[sub, enabled]), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
