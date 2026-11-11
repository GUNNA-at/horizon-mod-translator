//! The game's resources as a mod reads them (`Minecraft.getResourceManager()`, a reload listener's
//! `ResourceManager`): the client's `assets/` (its resource packs over the mods' and vanilla's)
//! and the server's `data/`, file by file; the streams and readers they come through (`java.io`'s
//! `InputStream`, `InputStreamReader`, `BufferedReader`, commons-io's `IOUtils`); the client's
//! readers of them that mods call (`BlockModel.fromStream`, `NativeImage.read`); and the reload
//! listeners mods register (`registerReloadListener`, NeoForge's
//! `RegisterClientReloadListenersEvent`), run when the client's resources load.
//!
//! The files come from the host ([`crate::host::Host::resource`]: the game's resource root);
//! without one (a load test), from the folders `HORIZON_COMPAT_RESOURCES` lists (`:` between).
//! A stream or reader is its bytes and where reading is ([`Data::Bytes`]), read as UTF-8 text by
//! a reader; work runs at once, as `CompletableFuture` does here.
use crate::{Data, Rt, Table, V};
use std::path::{Path, PathBuf};

const MINECRAFT: &str = "net/minecraft/client/Minecraft";
const SERVER: &str = "net/minecraft/server/MinecraftServer";
const PROVIDER: &str = "net/minecraft/server/packs/resources/ResourceProvider";
const MANAGER: &str = "net/minecraft/server/packs/resources/ResourceManager";
pub(crate) const RELOADABLE: &str = "net/minecraft/server/packs/resources/ReloadableResourceManager";
const CLOSEABLE: &str = "net/minecraft/server/packs/resources/CloseableResourceManager";
pub(crate) const MULTI: &str = "net/minecraft/server/packs/resources/MultiPackResourceManager";
const RESOURCE: &str = "net/minecraft/server/packs/resources/Resource";
const LISTENER: &str = "net/minecraft/server/packs/resources/PreparableReloadListener";
const BARRIER: &str = "net/minecraft/server/packs/resources/PreparableReloadListener$PreparationBarrier";
const SIMPLE: &str = "net/minecraft/server/packs/resources/SimplePreparableReloadListener";
const SIMPLE_JSON: &str = "net/minecraft/server/packs/resources/SimpleJsonResourceReloadListener";
const FILE_TO_ID: &str = "net/minecraft/resources/FileToIdConverter";
const MANAGER_LISTENER: &str = "net/minecraft/server/packs/resources/ResourceManagerReloadListener";
pub(crate) const CLIENT_LISTENERS_EVENT: &str = "net/neoforged/neoforge/client/event/RegisterClientReloadListenersEvent";
pub(crate) const SERVER_LISTENERS_EVENT: &str = "net/neoforged/neoforge/event/AddReloadListenerEvent";
const SERVER_RESOURCES: &str = "net/minecraft/server/ReloadableServerResources";

/// The server's resources (`ReloadableServerResources`): one object, with what mixins inject
/// at the end of its constructor run as it is made (a mod takes its recipe manager there).
fn server_resources(rt: &mut Rt) -> V {
    if let Some(&resources) = rt.interned.get("compat:server-resources") {
        return resources;
    }
    let resources = rt.new_object(SERVER_RESOURCES);
    rt.interned
        .insert("compat:server-resources".into(), resources);
    // Its recipe manager, a field its constructor sets (Cucumber's mixin reads the field there).
    let recipes = crate::api::call(
        rt,
        "net/minecraft/server/ReloadableServerResources.getRecipeManager.()Lnet/minecraft/world/item/crafting/RecipeManager;",
        &[resources],
    );
    rt.put_field(resources, "recipes", recipes);
    if crate::places::constructors_heard(rt, SERVER_RESOURCES) {
        let registries = registry_access(rt);
        crate::places::constructed(
            rt,
            resources,
            SERVER_RESOURCES,
            &[("Lnet/minecraft/core/RegistryAccess$Frozen;", registries)],
        );
    }
    resources
}
const TAGS_UPDATED_EVENT: &str = "net/neoforged/neoforge/event/TagsUpdatedEvent";
const UPDATE_CAUSE: &str = "net/neoforged/neoforge/event/TagsUpdatedEvent$UpdateCause";
const UPDATE_CAUSES: [&str; 2] = ["SERVER_DATA_LOAD", "CLIENT_PACKET_RECEIVED"];
const PROFILER: &str = "net/minecraft/util/profiling/ProfilerFiller";
const INACTIVE_PROFILER: &str = "net/minecraft/util/profiling/InactiveProfiler";
const EXECUTOR: &str = "java/util/concurrent/Executor";
/// The executor reload listeners get: it runs each task at once.
const DIRECT_EXECUTOR: &str = "compat:direct-executor";
const BLOCK_MODEL: &str = "net/minecraft/client/renderer/block/model/BlockModel";
const BLOCK_ELEMENT: &str = "net/minecraft/client/renderer/block/model/BlockElement";
const ELEMENT_FACE: &str = "net/minecraft/client/renderer/block/model/BlockElementFace";
const FACE_UV: &str = "net/minecraft/client/renderer/block/model/BlockFaceUV";
const ELEMENT_ROTATION: &str = "net/minecraft/client/renderer/block/model/BlockElementRotation";
const NATIVE_IMAGE: &str = "com/mojang/blaze3d/platform/NativeImage";
const IMAGE_FORMAT: &str = "com/mojang/blaze3d/platform/NativeImage$Format";
const IOUTILS: &str = "org/apache/commons/io/IOUtils";
const INPUT_STREAM: &str = "java/io/InputStream";
const BYTES_IN: &str = "java/io/ByteArrayInputStream";
const BUFFERED_IN: &str = "java/io/BufferedInputStream";
const FILTER_IN: &str = "java/io/FilterInputStream";
const READER: &str = "java/io/Reader";
const STREAM_READER: &str = "java/io/InputStreamReader";
const BUFFERED_READER: &str = "java/io/BufferedReader";
const STRING_READER: &str = "java/io/StringReader";
const FILE_READER: &str = "java/io/FileReader";

const RELOAD: &str = "net/minecraft/server/packs/resources/PreparableReloadListener.reload.(Lnet/minecraft/server/packs/resources/PreparableReloadListener$PreparationBarrier;Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;Lnet/minecraft/util/profiling/ProfilerFiller;Ljava/util/concurrent/Executor;Ljava/util/concurrent/Executor;)Ljava/util/concurrent/CompletableFuture;";
const PREPARE: &str = "net/minecraft/server/packs/resources/SimplePreparableReloadListener.prepare.(Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)Ljava/lang/Object;";
const APPLY: &str = "net/minecraft/server/packs/resources/SimplePreparableReloadListener.apply.(Ljava/lang/Object;Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)V";
const ON_RELOAD: &str = "net/minecraft/server/packs/resources/ResourceManagerReloadListener.onResourceManagerReload.(Lnet/minecraft/server/packs/resources/ResourceManager;)V";
const RUN: &str = "java/lang/Runnable.run.()V";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";

/// A stream's classes: the ones this module makes, and those a mod wraps one in.
fn stream_class(c: &str) -> bool {
    matches!(c, INPUT_STREAM | BYTES_IN | BUFFERED_IN | FILTER_IN)
}

fn reader_class(c: &str) -> bool {
    matches!(
        c,
        READER | STREAM_READER | BUFFERED_READER | STRING_READER | FILE_READER
    )
}

pub fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        PROVIDER
            | MANAGER
            | RELOADABLE
            | CLOSEABLE
            | MULTI
            | RESOURCE
            | LISTENER
            | BARRIER
            | SIMPLE
            | SIMPLE_JSON
            | FILE_TO_ID
            | MANAGER_LISTENER
            | CLIENT_LISTENERS_EVENT
            | SERVER_LISTENERS_EVENT
            | TAGS_UPDATED_EVENT
            | UPDATE_CAUSE
            | PROFILER
            | INACTIVE_PROFILER
            | BLOCK_MODEL
            | FACE_UV
            | NATIVE_IMAGE
            | IMAGE_FORMAT
            | IOUTILS
    ) || stream_class(owner)
        || reader_class(owner)
        || (matches!(owner, MINECRAFT | SERVER) && name == "getResourceManager")
        || matches!(
            name,
            "getRegistryLookup" | "getContext" | "makeConditionalOps"
        )
        || (owner == EXECUTOR && name == "execute")
        || (matches!(owner, "java/io/Closeable" | "java/lang/AutoCloseable") && name == "close")
}

/// Minecraft's resource classes for `instanceof`: the client's manager is reloadable, the
/// server's a pack stack's; both are managers and providers.
pub(crate) fn is_a(c: &str, class: &str) -> bool {
    match class {
        MANAGER | PROVIDER => matches!(c, RELOADABLE | CLOSEABLE | MULTI),
        LISTENER => matches!(c, SIMPLE | SIMPLE_JSON | MANAGER_LISTENER),
        SIMPLE => c == SIMPLE_JSON,
        CLOSEABLE => c == MULTI,
        INPUT_STREAM | "java/io/Closeable" | "java/lang/AutoCloseable" => stream_class(c) || reader_class(c) && class != INPUT_STREAM,
        READER | "java/lang/Readable" => reader_class(c),
        FILTER_IN => c == BUFFERED_IN,
        EXECUTOR => c == DIRECT_EXECUTOR,
        PROFILER => c == INACTIVE_PROFILER,
        _ => false,
    }
}

/// Folders a load test reads resources from (`HORIZON_COMPAT_RESOURCES`).
fn fallback_dirs() -> &'static [PathBuf] {
    static DIRS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| std::env::var("HORIZON_COMPAT_RESOURCES").map(|v| v.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect()).unwrap_or_default())
}

/// Content folders by the namespace folders each has under `assets` and `data`.
type ByNamespace = std::collections::HashMap<(String, String), Vec<PathBuf>>;

/// [`ByNamespace`] of `folders`: those that are no folder (a packed mod) have none.
fn by_namespace(folders: &[PathBuf]) -> ByNamespace {
    let mut out = ByNamespace::new();
    for folder in folders {
        for root in ["assets", "data"] {
            let Ok(entries) = std::fs::read_dir(folder.join(root)) else {
                continue;
            };
            for e in entries.flatten().filter(|e| e.path().is_dir()) {
                if let Some(namespace) = e.file_name().to_str() {
                    out.entry((root.to_owned(), namespace.to_owned()))
                        .or_default()
                        .push(folder.clone());
                }
            }
        }
    }
    out
}

/// The mods' content folders that have files for `namespace` under `root` (`assets`, `data`):
/// the folders the game was started with (`--dev-mod`) and those in its `horizon-mods` folder, where the
/// translator put each jar's resource files as they were. A mod reads its own through its class
/// loader while it is being constructed (Sophisticated Backpacks its models, for its shapes),
/// before any game is there to ask for a resource.
fn content_dirs(root: &str, namespace: &str) -> &'static [PathBuf] {
    static DIRS: std::sync::OnceLock<ByNamespace> = std::sync::OnceLock::new();
    let all = DIRS.get_or_init(|| {
        let mut folders = Vec::new();
        let mut args = std::env::args_os();
        while let Some(arg) = args.next() {
            if arg == "--dev-mod"
                && let Some(path) = args.next()
            {
                folders.push(PathBuf::from(path));
            }
        }
        if let Ok(entries) = std::fs::read_dir("horizon-mods") {
            let mut installed: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            installed.sort();
            folders.extend(installed);
        }
        by_namespace(&folders)
    });
    all.get(&(root.to_owned(), namespace.to_owned()))
        .map_or(&[], Vec::as_slice)
}

/// The content folders that may hold resource file `path` (`assets/<namespace>/...`).
fn content_dirs_of(path: &str) -> &'static [PathBuf] {
    let mut parts = path.split('/');
    match (parts.next(), parts.next()) {
        (Some(root), Some(namespace)) => content_dirs(root, namespace),
        _ => &[],
    }
}

/// A resource path a mod may read: relative, no `..`, no empty part.
fn safe(path: &str) -> bool {
    !path.is_empty() && !path.contains('\\') && path.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

/// The bytes of resource file `path` (`assets/create/lang/en_us.json`): the host's, else those of
/// the first fallback folder that has it, else of the first of the mods' content folders
/// ([`content_dirs`]).
pub fn file(rt: &Rt, path: &str) -> Option<Vec<u8>> {
    if !safe(path) {
        return None;
    }
    if let Some(bytes) = rt.host().and_then(|h| h.resource(path)) {
        return Some(bytes);
    }
    fallback_dirs()
        .iter()
        .chain(content_dirs_of(path))
        .find_map(|d| std::fs::read(d.join(path)).ok())
}

/// A file of a mod's own jar by its path there (`config/defaults.json`), as
/// `Class.getResourceAsStream` finds it: one the bundle carries ([`crate::FILES`]: what a mod may
/// read for itself, at any time), else a resource file of the game's ([`file`]: under `assets/`
/// or `data/`, where a game is there to ask).
pub(crate) fn jar_file(rt: &mut Rt, path: &str) -> Option<Vec<u8>> {
    let path = path.trim_start_matches('/');
    if !safe(path) {
        return None;
    }
    if let Some(bytes) = rt.reflect(crate::FILES, path, &[]) {
        return Some(
            crate::jutil::elements(rt, bytes)
                .into_iter()
                .map(|b| b.i() as u8)
                .collect(),
        );
    }
    file(rt, path)
}

/// Where file or folder `path` of a mod's jar is on disk, for a `Path` into the jar
/// (`IModFile.findResource("data", "gems", "packs")`): in the mods' content folder that has it
/// (the translator put each jar's `assets` and `data` there as they were, so a folder is there
/// with what is under it), else among the game's cached files, where a file of the jar the
/// bundle carries ([`jar_file`]) is written the first time it is asked for. What no jar has is a
/// path there with nothing at it, as Java's is in a jar's file system.
pub(crate) fn jar_path(rt: &mut Rt, path: &str) -> PathBuf {
    let path = path.trim_matches('/');
    if safe(path)
        && let Some(found) = fallback_dirs()
            .iter()
            .chain(content_dirs_of(path))
            .map(|d| d.join(path))
            .find(|p| p.exists())
    {
        return found;
    }
    let kept = crate::files::game_dir(rt)
        .join(".horizon-cache")
        .join("mod-files")
        .join(path);
    // (Only beside a game: a runtime without a game's folder writes nothing.)
    if safe(path)
        && rt.statics.contains_key(crate::GAME_DIR)
        && !kept.exists()
        && let Some(bytes) = jar_file(rt, path)
    {
        if let Some(folder) = kept.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(&kept, bytes);
    }
    kept
}

/// The resource files under folder `prefix` (`assets/create/models`), as paths, sorted.
pub fn paths(rt: &Rt, prefix: &str) -> Vec<String> {
    let prefix = prefix.trim_end_matches('/');
    if !safe(prefix) {
        return Vec::new();
    }
    let mut out: std::collections::BTreeSet<String> = rt.host().map(|h| h.resource_paths(prefix)).unwrap_or_default().into_iter().collect();
    for d in fallback_dirs() {
        walk(d, &d.join(prefix), &mut out);
    }
    out.into_iter().filter(|p| p.starts_with(&format!("{prefix}/"))).collect()
}

/// The namespaces with resources under `root` (`assets`, `data`), sorted.
pub fn namespaces(rt: &Rt, root: &str) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> = rt.host().map(|h| h.resource_namespaces(root)).unwrap_or_default().into_iter().collect();
    for d in fallback_dirs() {
        let Ok(entries) = std::fs::read_dir(d.join(root)) else { continue };
        out.extend(entries.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().to_str().map(str::to_owned)));
    }
    out.into_iter().collect()
}

fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// The client's resource manager (`assets`) or the server's (`data`), one object each.
pub fn manager(rt: &mut Rt, root: &'static str) -> V {
    rt.intern(format!("compat:resource-manager/{root}"), |rt| {
        let m = rt.new_object(if root == "assets" { RELOADABLE } else { MULTI });
        let r = rt.string(root);
        rt.put_field(m, "$root", r);
        m
    })
}

fn root_of(rt: &Rt, m: V) -> String {
    rt.str_of(rt.get_field(m, "$root")).unwrap_or("assets").to_owned()
}

/// Where location `loc` (`create:models/block/shaft.json`) is under `root`.
fn path_of(rt: &Rt, root: &str, loc: V) -> String {
    let id = crate::neo::location(rt, loc);
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
    format!("{root}/{ns}/{path}")
}

/// A `Resource` for the file at `path` (read when it is opened).
fn resource(rt: &mut Rt, path: &str) -> V {
    let r = rt.new_object(RESOURCE);
    let p = rt.string(path);
    rt.put_field(r, "$path", p);
    r
}

fn exists(rt: &Rt, path: &str) -> bool {
    file(rt, path).is_some()
}

/// A stream over `bytes`.
pub fn input_stream(rt: &mut Rt, bytes: Vec<u8>) -> V {
    rt.alloc(BYTES_IN, Data::Bytes(bytes, 0))
}

/// What is left to read of a stream or reader of this module, taken: reading is then at its end.
pub(crate) fn take(rt: &mut Rt, v: V) -> Option<Vec<u8>> {
    match rt.heap.get_mut(v.r() as usize).map(|o| &mut o.data) {
        Some(Data::Bytes(b, at)) => {
            let rest = b.get(*at..).map(<[u8]>::to_vec).unwrap_or_default();
            *at = b.len();
            Some(rest)
        }
        _ => None,
    }
}

/// The text left in `v`: a reader or stream of this module (taken, as reading it would), or a
/// string.
pub fn take_text(rt: &mut Rt, v: V) -> Option<String> {
    if let Some(s) = rt.str_of(v) {
        return Some(s.to_owned());
    }
    take(rt, v).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Makes `this` (just allocated by `new`) a stream or reader over `bytes`. A mod's own stream
/// (`extends FilterInputStream`) keeps the fields it has (an inner class's `this$0`, set before
/// `super(in)`).
pub(crate) fn become_bytes(rt: &mut Rt, this: V, bytes: Vec<u8>) {
    let Some(o) = rt.heap.get_mut(this.r() as usize) else {
        return;
    };
    if let Data::Fields(fields) = std::mem::replace(&mut o.data, Data::Bytes(bytes, 0)) {
        if !fields.is_empty() {
            rt.side_fields.entry(this.r()).or_default().extend(fields);
        }
    }
}

pub(crate) fn byte_array(rt: &mut Rt, bytes: &[u8]) -> V {
    rt.alloc("[B", Data::Array(bytes.iter().map(|b| V::I(*b as i8 as i32)).collect()))
}

pub(crate) fn array_bytes(rt: &Rt, a: V) -> Vec<u8> {
    match rt.heap.get(a.r() as usize).map(|o| &o.data) {
        Some(Data::Array(v)) => v.iter().map(|b| b.i() as u8).collect(),
        _ => Vec::new(),
    }
}

/// The next char a reader gives (a UTF-16 unit; one past the basic plane reads as U+FFFD), or -1.
fn read_char(rt: &mut Rt, v: V) -> i32 {
    let Some(Data::Bytes(b, at)) = rt.heap.get_mut(v.r() as usize).map(|o| &mut o.data) else { return -1 };
    let Some(&first) = b.get(*at) else { return -1 };
    let len = match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    };
    let end = (*at + len).min(b.len());
    let c = std::str::from_utf8(&b[*at..end]).ok().and_then(|s| s.chars().next()).map_or(0xfffd, |c| if (c as u32) < 0x10000 { c as i32 } else { 0xfffd });
    *at = end;
    c
}

/// The next line a reader gives (`BufferedReader.readLine`: without its `\n` or `\r\n`), or None
/// at the end.
fn read_line(rt: &mut Rt, v: V) -> Option<String> {
    let Some(Data::Bytes(b, at)) = rt.heap.get_mut(v.r() as usize).map(|o| &mut o.data) else { return None };
    if *at >= b.len() {
        return None;
    }
    let rest = &b[*at..];
    let end = rest.iter().position(|c| *c == b'\n' || *c == b'\r').unwrap_or(rest.len());
    let line = String::from_utf8_lossy(&rest[..end]).into_owned();
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

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
        (MINECRAFT, "getResourceManager") => return Some(manager(rt, "assets")),
        (SERVER, "getResourceManager") => return Some(manager(rt, "data")),
        (EXECUTOR, "execute") if rt.class_of(a(0)) == DIRECT_EXECUTOR => {
            rt.call_functional(RUN, a(1), &[]);
            return Some(V::Void);
        }
        ("java/io/Closeable" | "java/lang/AutoCloseable", "close") => {
            let c = rt.class_of(a(0));
            return (stream_class(c) || reader_class(c) || c == NATIVE_IMAGE).then_some(V::Void);
        }
        _ => {}
    }
    if stream_class(owner) {
        return stream_call(rt, name, desc, args);
    }
    if reader_class(owner) {
        return reader_call(rt, owner, name, desc, args);
    }
    Some(match (owner, name) {
        (PROVIDER | MANAGER | RELOADABLE | CLOSEABLE | MULTI, _) => return manager_call(rt, name, desc, args),
        // A mod's own `new Resource(pack, streamSupplier[, metadataSupplier])`.
        (RESOURCE, "<init>") => {
            rt.put_field(a(0), "source", a(1));
            rt.put_field(a(0), "streamSupplier", a(2));
            rt.put_field(a(0), "metadataSupplier", a(3));
            V::Void
        }
        (RESOURCE, "source") => rt.get_field(a(0), "source"),
        // `super()` from a mod's listener: vanilla's constructor keeps nothing.
        (SIMPLE, "<init>") => V::Void,
        (RESOURCE, "open" | "openAsReader") => {
            let stream = resource_stream(rt, a(0));
            if name == "open" || rt.thrown.is_some() {
                stream
            } else {
                let bytes = take(rt, stream).unwrap_or_default();
                rt.alloc(BUFFERED_READER, Data::Bytes(bytes, 0))
            }
        }
        (FACE_UV, "<init>") => {
            rt.put_field(a(0), "uvs", a(1));
            rt.put_field(a(0), "rotation", a(2));
            V::Void
        }
        // The packs' own name for the mods' and vanilla's files.
        (RESOURCE, "sourcePackId") => rt.string("vanilla"),
        (RESOURCE, "isBuiltin") => V::I(1),
        (RESOURCE, "knownPackInfo") => crate::api_common::optional(rt, V::NULL),
        (LISTENER | SIMPLE | SIMPLE_JSON | MANAGER_LISTENER, "reload") => reload_one(rt, a(0), a(1), a(2), a(3)),
        // A JSON listener's own `prepare` (vanilla's, which its mod subclass keeps).
        (SIMPLE | SIMPLE_JSON, "prepare") if rt.instance_of(a(0), SIMPLE_JSON) => return json_listener_call(rt, name, args),
        (SIMPLE_JSON, _) => return json_listener_call(rt, name, args),
        (FILE_TO_ID, _) => return converter_call(rt, name, args),
        (LISTENER | SIMPLE | MANAGER_LISTENER, "getName") => {
            let c = rt.class_of(a(0)).to_owned();
            rt.string(&c)
        }
        (BARRIER, "wait") => crate::jutil::completed_future(rt, a(1)),
        (CLIENT_LISTENERS_EVENT, "registerReloadListener") => {
            register(rt, "assets", a(1));
            V::Void
        }
        // The server's: this load's listeners, after vanilla's.
        (SERVER_LISTENERS_EVENT, "addListener") => {
            register(rt, "data", a(1));
            V::Void
        }
        (SERVER_LISTENERS_EVENT, "getListeners") => {
            let all = listeners(rt, "data");
            let items = crate::jutil::elements(rt, all);
            crate::jutil::new_list(rt, items)
        }
        (SERVER_LISTENERS_EVENT, "getServerResources") => server_resources(rt),
        (SERVER_LISTENERS_EVENT, "getRegistryAccess") | (TAGS_UPDATED_EVENT, "getRegistryAccess") | (SERVER_RESOURCES, "getRegistryLookup") => registry_access(rt),
        // What NeoForge gives every reload listener (`ContextAwareReloadListener`, which the
        // game's listener classes extend there): the registries, the context its conditions
        // are judged in, and the ops that read a file with both. Create Big Cannons' listeners
        // read their block and munition properties with them.
        (_, "getRegistryLookup") if desc == "()Lnet/minecraft/core/HolderLookup$Provider;" => {
            registry_access(rt)
        }
        (_, "getContext")
            if desc == "()Lnet/neoforged/neoforge/common/conditions/ICondition$IContext;" =>
        {
            rt.intern("compat:condition-context".into(), |rt| {
                rt.new_object("net/neoforged/neoforge/common/conditions/ICondition$IContext")
            })
        }
        (_, "makeConditionalOps") if desc.starts_with("()") => {
            let (access, json) = (
                registry_access(rt),
                crate::api::get_static(rt, "com/mojang/serialization/JsonOps.INSTANCE"),
            );
            crate::api::call(
                rt,
                "net/minecraft/core/HolderLookup$Provider.createSerializationContext.(Lcom/mojang/serialization/DynamicOps;)Lnet/minecraft/resources/RegistryOps;",
                &[access, json],
            )
        }
        // The tags bound (`ReloadableServerResources.updateRegistryTags`): the server's own load,
        // after its listeners; one from a server's packet is a client's.
        (TAGS_UPDATED_EVENT, "getUpdateCause") => {
            let i = rt.get_field(a(0), "$cause").i().clamp(0, 1) as usize;
            update_cause(rt, i)
        }
        (TAGS_UPDATED_EVENT, "shouldUpdateStaticData") => {
            let server = rt.get_field(a(0), "$cause").i() == 0;
            V::I(i32::from(server || rt.get_field(a(0), "$integrated").i() == 0))
        }
        (UPDATE_CAUSE, "values") => {
            let all = (0..UPDATE_CAUSES.len()).map(|i| update_cause(rt, i)).collect();
            rt.alloc("[Lnet/neoforged/neoforge/event/TagsUpdatedEvent$UpdateCause;", Data::Array(all))
        }
        (UPDATE_CAUSE, "valueOf") => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            match UPDATE_CAUSES.iter().position(|c| *c == n) {
                Some(i) => update_cause(rt, i),
                None => return None,
            }
        }
        (UPDATE_CAUSE, "ordinal") => V::I(crate::api_common::enum_ordinal(rt, a(0))),
        (UPDATE_CAUSE, "name" | "toString") => rt.get_field(a(0), "$name"),
        (PROFILER | INACTIVE_PROFILER, _) => profiler_call(rt, name, desc),
        (BLOCK_MODEL, _) => return block_model_call(rt, name, args),
        (NATIVE_IMAGE, _) => return image_call(rt, name, desc, args),
        // An image's format: how many of a pixel's channels it holds, and which
        // (`RGBA`, `RGB`, `LUMINANCE_ALPHA`, `LUMINANCE`, in that order).
        (IMAGE_FORMAT, _) => {
            let format = rt.get_field(a(0), "$ordinal").i().clamp(0, 3);
            match name {
                "components" => V::I(4 - format),
                "hasAlpha" | "hasLuminanceOrAlpha" => V::I(i32::from(format != 1)),
                "hasLuminanceOrRed" | "hasLuminanceOrGreen" | "hasLuminanceOrBlue" => V::I(1),
                "hasRed" | "hasGreen" | "hasBlue" => V::I(i32::from(format < 2)),
                "hasLuminance" => V::I(i32::from(format >= 2)),
                "glFormat" => V::I([6408, 6407, 33319, 6403][format as usize]),
                "supportedByStb" => V::I(1),
                "setPackPixelStoreState" | "setUnpackPixelStoreState" => V::Void,
                _ => return None,
            }
        }
        (IOUTILS, _) => return ioutils_call(rt, name, desc, args),
        _ => return None,
    })
}

/// The stream resource `r` opens: its file's, or what the supplier a mod made it with gives.
fn resource_stream(rt: &mut Rt, r: V) -> V {
    let path = rt.get_field(r, "$path");
    match rt.str_of(path).map(str::to_owned) {
        Some(path) => match file(rt, &path) {
            Some(bytes) => input_stream(rt, bytes),
            None => crate::jlang::throw_with(rt, "java/io/FileNotFoundException", &path),
        },
        None => {
            let supplier = rt.get_field(r, "streamSupplier");
            rt.call_functional("net/minecraft/server/packs/resources/IoSupplier.get.()Ljava/lang/Object;", supplier, &[])
        }
    }
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (INACTIVE_PROFILER, "INSTANCE") => Some(profiler(rt)),
        (UPDATE_CAUSE, f) => UPDATE_CAUSES.iter().position(|c| *c == f).map(|i| update_cause(rt, i)),
        _ => None,
    }
}

fn manager_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let root = root_of(rt, a(0));
    Some(match name {
        "getResource" => {
            let path = path_of(rt, &root, a(1));
            let r = if exists(rt, &path) { resource(rt, &path) } else { V::NULL };
            crate::api_common::optional(rt, r)
        }
        "getResourceOrThrow" => {
            let path = path_of(rt, &root, a(1));
            if exists(rt, &path) {
                resource(rt, &path)
            } else {
                let id = crate::neo::location(rt, a(1));
                crate::jlang::throw_with(rt, "java/io/FileNotFoundException", &id)
            }
        }
        "open" | "openAsReader" => {
            let path = path_of(rt, &root, a(1));
            match file(rt, &path) {
                Some(bytes) => rt.alloc(if name == "open" { BYTES_IN } else { BUFFERED_READER }, Data::Bytes(bytes, 0)),
                None => {
                    let id = crate::neo::location(rt, a(1));
                    crate::jlang::throw_with(rt, "java/io/FileNotFoundException", &id)
                }
            }
        }
        // The stack of a location's files, one pack's each: here the one file the root has.
        "getResourceStack" => {
            let path = path_of(rt, &root, a(1));
            let items = if exists(rt, &path) { vec![resource(rt, &path)] } else { Vec::new() };
            crate::jutil::new_list(rt, items)
        }
        // The files under folder `a(1)` whose location the filter takes, by location, sorted.
        "listResources" | "listResourceStacks" => {
            let folder = rt.str_of(a(1)).unwrap_or("").to_owned();
            let filter = a(2);
            list(rt, &root, &folder, name == "listResourceStacks", &mut |rt, loc| {
                let keep = rt.call_functional(TEST, filter, &[loc]).i() != 0;
                rt.thrown = None;
                keep
            })
        }
        "getNamespaces" => {
            let spaces = namespaces(rt, &root);
            let set = rt.alloc("java/util/TreeSet", Data::Table(Table { sorted: true, ..Table::default() }));
            for s in spaces {
                let v = rt.string(&s);
                crate::jutil::table_put(rt, set, v, V::I(1));
            }
            set
        }
        "listPacks" => rt.alloc("java/util/stream/Stream", Data::Stream(Vec::new())),
        "registerReloadListener" => {
            register(rt, if root == "assets" { "assets" } else { "data" }, a(1));
            V::Void
        }
        "close" => V::Void,
        _ if desc.ends_with(")V") => V::Void,
        _ => return None,
    })
}

/// The files under folder `folder` of each namespace in `root`, as vanilla's `listResources`
/// answers them: a `TreeMap` by location (`create:train_hat_info/pig.json`) of those `keep` takes,
/// each its `Resource` (or a stack of it, `stacks`).
fn list(rt: &mut Rt, root: &str, folder: &str, stacks: bool, keep: &mut dyn FnMut(&mut Rt, V) -> bool) -> V {
    let folder = folder.trim_matches('/');
    let map = rt.alloc("java/util/TreeMap", Data::Table(Table { sorted: true, ..Table::default() }));
    let found: Vec<String> = namespaces(rt, root).iter().flat_map(|ns| paths(rt, &format!("{root}/{ns}/{folder}"))).collect();
    for path in found {
        let Some((ns, rest)) = path.strip_prefix(&format!("{root}/")).and_then(|p| p.split_once('/')) else { continue };
        let loc = rt.string(&format!("{ns}:{rest}"));
        if !keep(rt, loc) {
            continue;
        }
        let r = resource(rt, &path);
        let value = if stacks { crate::jutil::new_list(rt, vec![r]) } else { r };
        crate::jutil::table_put(rt, map, loc, value);
    }
    map
}

/// A `FileToIdConverter`'s folder and extension.
fn converter(rt: &Rt, c: V) -> (String, String) {
    let field = |f| rt.str_of(rt.get_field(c, f)).unwrap_or("").to_owned();
    (field("prefix"), field("extension"))
}

/// `FileToIdConverter`, 1.21.1's: a folder of files of one extension, whose names are the ids.
fn converter_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "<init>" => {
            rt.put_field(a(0), "prefix", a(1));
            rt.put_field(a(0), "extension", a(2));
            V::Void
        }
        "json" => {
            let c = rt.new_object(FILE_TO_ID);
            let ext = rt.string(".json");
            rt.put_field(c, "prefix", a(0));
            rt.put_field(c, "extension", ext);
            c
        }
        "idToFile" => {
            let (prefix, ext) = converter(rt, a(0));
            let id = crate::neo::location(rt, a(1));
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            rt.string(&format!("{ns}:{prefix}/{path}{ext}"))
        }
        "fileToId" => {
            let (prefix, ext) = converter(rt, a(0));
            let file = crate::neo::location(rt, a(1));
            let (ns, path) = file.split_once(':').unwrap_or(("minecraft", &file));
            match path.get(prefix.len() + 1..path.len().saturating_sub(ext.len())) {
                Some(id) => rt.string(&format!("{ns}:{id}")),
                None => {
                    let msg = format!("begin {}, end {}, length {}", prefix.len() + 1, path.len().saturating_sub(ext.len()), path.len());
                    crate::jlang::throw_with(rt, "java/lang/StringIndexOutOfBoundsException", &msg)
                }
            }
        }
        "listMatchingResources" | "listMatchingResourceStacks" | "listMatchingResourcesFromNamespace" | "listMatchingResourceStacksFromNamespace" => {
            let (prefix, ext) = converter(rt, a(0));
            let root = root_of(rt, a(1));
            let only = if name.ends_with("FromNamespace") { rt.str_of(a(2)).map(|n| format!("{n}:")) } else { None };
            list(rt, &root, &prefix, name.contains("Stacks"), &mut |rt, loc| {
                let id = rt.str_of(loc).unwrap_or("");
                id.ends_with(&ext) && only.as_ref().is_none_or(|n| id.starts_with(n.as_str()))
            })
        }
        _ => return None,
    })
}

/// `SimpleJsonResourceReloadListener.scanDirectory`: each `.json` file under `dir`, by its id,
/// read as Gson's tree into `out`; a file that isn't JSON is left out, as vanilla logs and skips it.
fn scan_directory(rt: &mut Rt, manager: V, dir: &str, out: V) {
    let root = root_of(rt, manager);
    let found = list(rt, &root, dir, false, &mut |rt, loc| rt.str_of(loc).is_some_and(|l| l.ends_with(".json")));
    for (loc, res) in crate::jutil::table_pairs(rt, found) {
        let file = crate::neo::location(rt, loc);
        let (ns, path) = file.split_once(':').unwrap_or(("minecraft", &file));
        let id = format!("{ns}:{}", &path[dir.len() + 1..path.len() - ".json".len()]);
        let path = rt.str_of(rt.get_field(res, "$path")).unwrap_or("").to_owned();
        let text = file_text(rt, &path);
        match crate::gson::parse(rt, &text, false) {
            Ok(Some((element, _))) => {
                let id = rt.string(&id);
                crate::jutil::table_put(rt, out, id, element);
            }
            Ok(None) => trace(&format!("Couldn't parse data file {id} from {file}: JSON data was null or empty")),
            Err(e) => trace(&format!("Couldn't parse data file {id} from {file}: {e}")),
        }
    }
}

fn file_text(rt: &Rt, path: &str) -> String {
    file(rt, path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

fn trace(msg: &str) {
    if Rt::tracing() {
        eprintln!("horizon-compat: {msg}");
    }
}

/// `SimpleJsonResourceReloadListener`: its folder's JSON files, read by `prepare` into a map of
/// ids to trees for the mod's `apply`.
fn json_listener_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "<init>" => {
            rt.put_field(a(0), "gson", a(1));
            rt.put_field(a(0), "directory", a(2));
            V::Void
        }
        "prepare" => {
            let dir = rt.str_of(rt.get_field(a(0), "directory")).unwrap_or("").to_owned();
            let out = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
            scan_directory(rt, a(1), &dir, out);
            out
        }
        "scanDirectory" => {
            let dir = rt.str_of(a(1)).unwrap_or("").to_owned();
            scan_directory(rt, a(0), &dir, a(3));
            V::Void
        }
        "getPreparedPath" => {
            let dir = rt.str_of(rt.get_field(a(0), "directory")).unwrap_or("").to_owned();
            let id = crate::neo::location(rt, a(1));
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            rt.string(&format!("{ns}:{dir}/{path}.json"))
        }
        _ => return None,
    })
}

/// The reload listeners registered for `root`, in order.
fn listeners(rt: &mut Rt, root: &str) -> V {
    rt.intern(format!("compat:reload-listeners/{root}"), |rt| crate::jutil::new_list(rt, Vec::new()))
}

fn register(rt: &mut Rt, root: &str, listener: V) {
    if listener.r() == 0 {
        return;
    }
    let list = listeners(rt, root);
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.push(listener);
    }
}

fn profiler(rt: &mut Rt) -> V {
    rt.intern("compat:profiler".into(), |rt| rt.new_object(INACTIVE_PROFILER))
}

fn profiler_call(rt: &mut Rt, name: &str, desc: &str) -> V {
    match name {
        // `ProfilerFiller.tee`: the one profiler.
        "tee" => profiler(rt),
        _ if desc.ends_with(")Z") || desc.ends_with(")I") => V::I(0),
        _ if desc.ends_with(")J") => V::J(0),
        _ if desc.ends_with(")V") => V::Void,
        _ => V::NULL,
    }
}

/// Runs reload listener `l` on manager `m` as a resource reload runs it: its own `reload` (a
/// lambda's, GeckoLib's), else vanilla's for the base it extends: `prepare` then `apply` for a
/// `SimplePreparableReloadListener`, `onResourceManagerReload` for a
/// `ResourceManagerReloadListener`. Answers the future it leaves (done).
fn reload_one(rt: &mut Rt, l: V, barrier: V, m: V, profiler: V) -> V {
    if rt.instance_of(l, SIMPLE) {
        let prepared = rt.call_functional(PREPARE, l, &[m, profiler]);
        if rt.thrown.is_none() {
            rt.call_functional(APPLY, l, &[prepared, m, profiler]);
        }
    } else if rt.instance_of(l, MANAGER_LISTENER) {
        rt.call_functional(ON_RELOAD, l, &[m]);
    } else {
        let _ = barrier;
    }
    crate::jutil::completed_future(rt, V::NULL)
}

/// The resources of `root` (`assets`: the client's) loaded: each listener registered for it
/// reloads from its manager, in the order they were registered. What one throws is reported, and
/// the next goes on.
pub fn reload(rt: &mut Rt, root: &'static str) {
    // The client's resources are there to read: what waited for its thread runs first.
    if root == "assets" {
        crate::screens::run_client_tasks(rt);
    }
    let list = listeners(rt, root);
    let all = crate::jutil::elements(rt, list);
    if all.is_empty() {
        return;
    }
    let m = manager(rt, root);
    let barrier = rt.intern("compat:reload-barrier".into(), |rt| rt.new_object(BARRIER));
    let profiler = profiler(rt);
    let executor = rt.intern("compat:direct-executor".into(), |rt| rt.new_object(DIRECT_EXECUTOR));
    let mut held = all.clone();
    held.extend([m, barrier, profiler, executor]);
    for l in all {
        // Each listener is a piece of the mods' code of its own, as each listener of an event
        // is: the calls nothing maps are counted for it alone. Counted for the hook, which
        // runs every listener of the pack, one that made a million of them left each one
        // after it an error at its first (and JourneyMap, handed that error, looked through
        // its causes without end).
        let misses = (
            std::mem::take(&mut rt.hook_misses),
            rt.hook_first_miss.take(),
        );
        crate::api::new_piece();
        // A mod's own listener or a lambda dispatches (to its `reload`, else to vanilla's base's,
        // which comes back here as `reload_one`).
        if rt.cid(l) != 0 {
            rt.call_functional(RELOAD, l, &[barrier, m, profiler, profiler, executor, executor]);
        } else {
            reload_one(rt, l, barrier, m, profiler);
        }
        (rt.hook_misses, rt.hook_first_miss) = misses;
        rt.report_thrown("resource reload listener");
        // What one listener left behind goes before the next begins: a pack's listeners
        // together make far more garbage than fits (those who run this hold nothing more).
        crate::api::between(rt, &held, l);
    }
}

/// The server's data loaded (a world's first open, `/reload`): NeoForge's `AddReloadListenerEvent`
/// gathers this load's listeners anew, as each `ReloadableServerResources` load asks for them, and
/// each reloads from the server's resources (`data/`) in the order they were added.
pub fn server_reload(rt: &mut Rt) {
    let list = listeners(rt, "data");
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.clear();
    }
    // Each load makes its `ReloadableServerResources` anew: what mixins inject at the end of
    // its constructor runs before the listeners are asked for, and what they inject into
    // `updateRegistryTags` around the tags' binding.
    rt.interned.remove("compat:server-resources");
    crate::recipes::reloaded(rt);
    let tagged = &crate::places::UPDATE_REGISTRY_TAGS;
    let resources = if crate::places::constructors_heard(rt, SERVER_RESOURCES) || tagged.heard(rt) {
        server_resources(rt)
    } else {
        V::NULL
    };
    let ev = rt.new_object(SERVER_LISTENERS_EVENT);
    crate::neo::post(rt, SERVER_LISTENERS_EVENT, ev);
    // Vanilla's own listeners load before the mods': its recipe manager among them.
    let (m, profiler) = (manager(rt, "data"), profiler(rt));
    crate::recipes::applied(rt, m, profiler);
    reload(rt, "data");
    // Then the loaded tags bind to the registries, and NeoForge says so: what a mod derives from
    // tags it derives again.
    tagged.head(rt, resources, &[]);
    let tags = rt.new_object(TAGS_UPDATED_EVENT);
    rt.put_field(tags, "$cause", V::I(0));
    crate::neo::post(rt, TAGS_UPDATED_EVENT, tags);
    tagged.end(rt, resources, &[]);
}

fn registry_access(rt: &mut Rt) -> V {
    crate::api::call(rt, "net/minecraft/server/MinecraftServer.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[V::NULL])
}

/// `TagsUpdatedEvent.UpdateCause`'s constant `i` (`SERVER_DATA_LOAD`, `CLIENT_PACKET_RECEIVED`).
fn update_cause(rt: &mut Rt, i: usize) -> V {
    crate::api::api_enum(rt, UPDATE_CAUSE, UPDATE_CAUSES[i], i as i32)
}

/// NeoForge's `RegisterClientReloadListenersEvent`, posted before the client's setup: the
/// listeners the mods add load with the client's resources, after it ([`reload`]).
pub fn client_listeners_event(rt: &mut Rt) -> V {
    let ev = rt.new_object(CLIENT_LISTENERS_EVENT);
    let m = manager(rt, "assets");
    rt.put_field(ev, "$manager", m);
    ev
}

pub(crate) fn stream_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    // A mod's own stream with no bytes here (`extends InputStream`): Java's `InputStream` reads it
    // through its own `read()`.
    if name != "<init>"
        && crate::streams::is_mod(rt, this)
        && !matches!(
            rt.heap.get(this.r() as usize).map(|o| &o.data),
            Some(Data::Bytes(..))
        )
    {
        return crate::streams::mod_read(rt, name, desc, args);
    }
    Some(match (name, desc) {
        // A mod's own stream's `super()`: nothing to take.
        ("<init>", "()V") => V::Void,
        // `new ByteArrayInputStream(bytes[, offset, length])`, or a stream around another.
        ("<init>", "([B)V") => {
            let bytes = array_bytes(rt, a(1));
            become_bytes(rt, this, bytes);
            V::Void
        }
        ("<init>", "([BII)V") => {
            let bytes = array_bytes(rt, a(1));
            let (from, len) = (a(2).i().max(0) as usize, a(3).i().max(0) as usize);
            let part = bytes.get(from..(from + len).min(bytes.len())).map(<[u8]>::to_vec).unwrap_or_default();
            become_bytes(rt, this, part);
            V::Void
        }
        ("<init>", _) => {
            let bytes = take(rt, a(1)).unwrap_or_default();
            become_bytes(rt, this, bytes);
            V::Void
        }
        ("read", "()I") => match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
            Some(Data::Bytes(b, at)) => match b.get(*at) {
                Some(&c) => {
                    *at += 1;
                    V::I(c as i32)
                }
                None => V::I(-1),
            },
            _ => V::I(-1),
        },
        ("read", "([B)I" | "([BII)I") | ("readNBytes", "([BII)I") => {
            let n = rt.array_len(a(1));
            let (off, len) = if desc == "([B)I" { (0, n) } else { (a(2).i(), a(3).i()) };
            let rest = match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                Some(Data::Bytes(b, at)) => {
                    let end = (*at + len.max(0) as usize).min(b.len());
                    let part = b[*at..end].to_vec();
                    *at = end;
                    part
                }
                _ => Vec::new(),
            };
            if rest.is_empty() && len > 0 {
                return Some(V::I(if name == "readNBytes" { 0 } else { -1 }));
            }
            for (i, byte) in rest.iter().enumerate() {
                rt.array_set(a(1), off + i as i32, V::I(*byte as i8 as i32));
            }
            V::I(rest.len() as i32)
        }
        ("readAllBytes", _) => {
            let rest = take(rt, this).unwrap_or_default();
            byte_array(rt, &rest)
        }
        ("readNBytes", "(I)[B") => {
            let n = a(1).i().max(0) as usize;
            let part = match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                Some(Data::Bytes(b, at)) => {
                    let end = (*at + n).min(b.len());
                    let part = b[*at..end].to_vec();
                    *at = end;
                    part
                }
                _ => Vec::new(),
            };
            byte_array(rt, &part)
        }
        ("available", _) => V::I(match rt.heap.get(this.r() as usize).map(|o| &o.data) {
            Some(Data::Bytes(b, at)) => b.len().saturating_sub(*at) as i32,
            _ => 0,
        }),
        ("skip", _) => {
            let n = a(1).j().max(0) as usize;
            V::J(match rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                Some(Data::Bytes(b, at)) => {
                    let end = (*at + n).min(b.len());
                    let skipped = end - *at;
                    *at = end;
                    skipped as i64
                }
                _ => 0,
            })
        }
        // `ByteArrayInputStream` and `BufferedInputStream` go back to a place they marked (the
        // first to its start, never marked); the others have no mark.
        ("markSupported", _) => V::I(marks(rt.class_of(this)) as i32),
        ("mark", _) => {
            mark(rt, this);
            V::Void
        }
        ("reset", _) => reset(rt, this),
        ("close", _) => V::Void,
        _ => return None,
    })
}

/// Whether a stream or a reader of `class` goes back to a mark, as the JDK's does: the buffered
/// ones and those over an array or a string.
fn marks(class: &str) -> bool {
    matches!(
        class,
        BUFFERED_READER
            | STRING_READER
            | BUFFERED_IN
            | BYTES_IN
            | "java/io/CharArrayReader"
            | "java/io/LineNumberReader"
    )
}

/// `mark(limit)`: where the reader or the stream at `this` is now, kept beside it.
fn mark(rt: &mut Rt, this: V) {
    if let Some(Data::Bytes(_, at)) = rt.heap.get(this.r() as usize).map(|o| &o.data) {
        let at = *at as i32;
        rt.put_field(this, "$mark", V::I(at));
    }
}

/// `reset()`: back to the mark. Without one, a reader over a string or an array and a stream
/// over an array go back to their start, and a buffered one throws, as the JDK's
/// (`IOException`: "Stream not marked", "Resetting to invalid mark").
fn reset(rt: &mut Rt, this: V) -> V {
    let class = rt.class_of(this);
    let to = match rt.get_field(this, "$mark") {
        V::I(at) => Some(at.max(0) as usize),
        _ if matches!(class, STRING_READER | BYTES_IN | "java/io/CharArrayReader") => Some(0),
        _ => None,
    };
    match to {
        Some(to) if marks(class) => {
            if let Some(Data::Bytes(b, at)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                *at = to.min(b.len());
            }
            V::Void
        }
        _ => {
            let message = if class == BUFFERED_IN { "Resetting to invalid mark" } else if marks(class) { "Stream not marked" } else { "mark/reset not supported" };
            crate::files::throw(rt, "java/io/IOException", message)
        }
    }
}

fn reader_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match (name, desc) {
        // `new StringReader(text)`; `new InputStreamReader(stream[, charset])` and `new
        // BufferedReader(reader[, size])` read what is left of the one they wrap.
        ("<init>", _) if owner == STRING_READER => {
            let text = rt.str_of(a(1)).unwrap_or("").as_bytes().to_vec();
            become_bytes(rt, this, text);
            V::Void
        }
        // `new FileReader(file | name[, charset])`: the file's text as it is now; one that is not
        // there is `FileNotFoundException`, as the JDK's.
        ("<init>", _) if owner == FILE_READER => {
            let path = rt.str_of(a(1)).unwrap_or("").to_owned();
            match crate::files::read_file(&crate::files::on_disk(rt, &path)) {
                Ok(bytes) => {
                    become_bytes(rt, this, bytes);
                    V::Void
                }
                Err(_) => crate::files::throw(
                    rt,
                    "java/io/FileNotFoundException",
                    &format!("{path} (No such file or directory)"),
                ),
            }
        }
        ("<init>", _) => {
            let bytes = take(rt, a(1)).unwrap_or_default();
            become_bytes(rt, this, bytes);
            V::Void
        }
        ("read", "()I") => V::I(read_char(rt, this)),
        ("read", "([C)I" | "([CII)I") => {
            let n = rt.array_len(a(1));
            let (off, len) = if desc == "([C)I" { (0, n) } else { (a(2).i(), a(3).i()) };
            let mut count = 0;
            while count < len {
                let c = read_char(rt, this);
                if c < 0 {
                    break;
                }
                rt.array_set(a(1), off + count, V::I(c));
                count += 1;
            }
            V::I(if count == 0 && len > 0 { -1 } else { count })
        }
        ("readLine", _) => match read_line(rt, this) {
            Some(line) => rt.string(&line),
            None => V::NULL,
        },
        ("lines", _) => {
            let mut lines = Vec::new();
            while let Some(line) = read_line(rt, this) {
                lines.push(rt.string(&line));
            }
            rt.alloc("java/util/stream/Stream", Data::Stream(lines))
        }
        ("ready", _) => V::I(match rt.heap.get(this.r() as usize).map(|o| &o.data) {
            Some(Data::Bytes(b, at)) => (*at < b.len()) as i32,
            _ => 0,
        }),
        // A buffered reader and one over a string go back to a place they marked (a JSON5 reader
        // looks at the next character so: with no mark kept, it lost every second one).
        ("markSupported", _) => V::I(marks(rt.class_of(this)) as i32),
        ("mark", _) => {
            mark(rt, this);
            V::Void
        }
        ("reset", _) => reset(rt, this),
        ("close", _) => V::Void,
        _ => return None,
    })
}

fn ioutils_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "closeQuietly" | "close" => V::Void,
        "toString" if desc.starts_with("(Ljava/io/InputStream;") || desc.starts_with("(Ljava/io/Reader;") => {
            let text = take_text(rt, a(0)).unwrap_or_default();
            rt.string(&text)
        }
        "toByteArray" if desc.starts_with("(Ljava/io/InputStream;") || desc.starts_with("(Ljava/io/Reader;") => {
            let bytes = take(rt, a(0)).unwrap_or_default();
            byte_array(rt, &bytes)
        }
        "readLines" => {
            let mut lines = Vec::new();
            if let Some(text) = take_text(rt, a(0)) {
                for l in text.lines() {
                    lines.push(rt.string(l));
                }
            }
            crate::jutil::new_list(rt, lines)
        }
        _ => return None,
    })
}

/// `BlockModel.fromStream(reader)` and `fromString(text)`: the model a JSON file declares, before
/// its parent is resolved (its own elements; its parent's location).
fn block_model_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "fromStream" | "fromString" => {
            let text = take_text(rt, a(0)).unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(json) if json.is_object() => block_model(rt, &json),
                Ok(_) => crate::jlang::throw_with(rt, "com/google/gson/JsonParseException", "Expected a JSON object for a block model"),
                Err(e) => crate::jlang::throw_with(rt, "com/google/gson/JsonSyntaxException", &e.to_string()),
            }
        }
        "getElements" => {
            let own = rt.get_field(a(0), "elements");
            let parent = rt.get_field(a(0), "parent");
            if crate::jutil::elements(rt, own).is_empty() && parent.r() != 0 {
                return Some(crate::api::call(rt, "net/minecraft/client/renderer/block/model/BlockModel.getElements.()Ljava/util/List;", &[parent]));
            }
            own
        }
        "getParentLocation" => rt.get_field(a(0), "parentLocation"),
        "hasAmbientOcclusion" => V::I((rt.get_field(a(0), "$ambientOcclusion") != V::I(0)) as i32),
        _ => return None,
    })
}

fn vector3f(rt: &mut Rt, v: &serde_json::Value) -> V {
    let o = rt.new_object("org/joml/Vector3f");
    for (i, f) in ["x", "y", "z"].into_iter().enumerate() {
        let c = v.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32;
        rt.put_field(o, f, V::F(c));
    }
    o
}

fn direction_named(rt: &mut Rt, name: &str) -> Option<V> {
    let i = crate::api_common::DIRECTION_NAMES.iter().position(|d| *d == name.to_ascii_lowercase())?;
    Some(crate::api_common::direction(rt, i as i64))
}

/// One of `ModelBakery`'s marker models (`GENERATION_MARKER`, `BLOCK_ENTITY_MARKER`): a model
/// with no elements and no parent, named for what it marks, which a mod compares a model's root
/// with to tell a generated item model or a block entity's from the rest.
pub(crate) fn marker_model(rt: &mut Rt, name: &str) -> V {
    let m = block_model(rt, &serde_json::Value::Object(serde_json::Map::new()));
    let n = rt.string(name);
    rt.put_field(m, "name", n);
    m
}

fn block_model(rt: &mut Rt, json: &serde_json::Value) -> V {
    let m = rt.new_object(BLOCK_MODEL);
    let mut elements = Vec::new();
    for e in json["elements"].as_array().into_iter().flatten() {
        let el = rt.new_object(BLOCK_ELEMENT);
        let (from, to) = (vector3f(rt, &e["from"]), vector3f(rt, &e["to"]));
        rt.put_field(el, "from", from);
        rt.put_field(el, "to", to);
        rt.put_field(el, "shade", V::I(e["shade"].as_bool().unwrap_or(true) as i32));
        let faces = rt.alloc("java/util/EnumMap", Data::Table(Table::default()));
        for (side, f) in e["faces"].as_object().into_iter().flatten() {
            let Some(dir) = direction_named(rt, side) else { continue };
            let face = rt.new_object(ELEMENT_FACE);
            let cull = match f["cullface"].as_str() {
                Some(c) => direction_named(rt, c).unwrap_or(V::NULL),
                None => V::NULL,
            };
            rt.put_field(face, "cullForDirection", cull);
            rt.put_field(face, "tintIndex", V::I(f["tintindex"].as_i64().unwrap_or(-1) as i32));
            let texture = rt.string(f["texture"].as_str().unwrap_or(""));
            rt.put_field(face, "texture", texture);
            let uv = rt.new_object(FACE_UV);
            let uvs = match f["uv"].as_array() {
                Some(u) => {
                    let vals: Vec<V> = (0..4).map(|i| V::F(u.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32)).collect();
                    rt.alloc("[F", Data::Array(vals))
                }
                None => V::NULL,
            };
            rt.put_field(uv, "uvs", uvs);
            rt.put_field(uv, "rotation", V::I(f["rotation"].as_i64().unwrap_or(0) as i32));
            rt.put_field(face, "uv", uv);
            crate::jutil::table_put(rt, faces, dir, face);
        }
        rt.put_field(el, "faces", faces);
        let rotation = match e.get("rotation") {
            Some(r) if r.is_object() => {
                let rot = rt.new_object(ELEMENT_ROTATION);
                let origin = vector3f(rt, &r["origin"]);
                rt.put_field(rot, "origin", origin);
                rt.put_field(rot, "angle", V::F(r["angle"].as_f64().unwrap_or(0.0) as f32));
                rt.put_field(rot, "rescale", V::I(r["rescale"].as_bool().unwrap_or(false) as i32));
                let axis = r["axis"].as_str().unwrap_or("y").to_ascii_uppercase();
                let ordinal = ["X", "Y", "Z"].iter().position(|x| *x == axis).unwrap_or(1);
                let axis = crate::api::api_enum(rt, "net/minecraft/core/Direction$Axis", ["X", "Y", "Z"][ordinal], ordinal as i32);
                rt.put_field(rot, "axis", axis);
                rot
            }
            _ => V::NULL,
        };
        rt.put_field(el, "rotation", rotation);
        elements.push(el);
    }
    let list = crate::jutil::new_list(rt, elements);
    rt.put_field(m, "elements", list);
    let parent = match json["parent"].as_str() {
        Some(p) => {
            let id = if p.contains(':') { p.to_owned() } else { format!("minecraft:{p}") };
            rt.string(&id)
        }
        None => V::NULL,
    };
    rt.put_field(m, "parentLocation", parent);
    rt.put_field(m, "parent", V::NULL);
    let name = rt.string("");
    rt.put_field(m, "name", name);
    rt.put_field(m, "$ambientOcclusion", V::I(json["ambientocclusion"].as_bool().unwrap_or(true) as i32));
    m
}

/// A PNG's pixels as RGBA bytes, with its size.
pub(crate) fn decode_png(bytes: &[u8]) -> Option<(usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width as usize, rgba))
}

/// An image of `width` pixels a row, its RGBA bytes held as a stream's are ([`Data::Bytes`]).
fn image(rt: &mut Rt, width: usize, rgba: Vec<u8>) -> V {
    rt.alloc(NATIVE_IMAGE, Data::Bytes(rgba, width))
}

fn image_size(rt: &Rt, v: V) -> (usize, usize) {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Bytes(b, w)) if *w > 0 => (*w, b.len() / 4 / *w),
        _ => (0, 0),
    }
}

/// `NativeImage`, 1.21.1's: `getPixelRGBA` answers a pixel as its bytes read little-endian
/// (ABGR, `0xAABBGGRR`), and `setPixelRGBA` takes one so.
fn image_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "read" => {
            let source = if desc.starts_with("(Lcom/mojang/blaze3d/platform/NativeImage$Format;") { a(1) } else { a(0) };
            let bytes = if rt.class_of(source) == "[B" { array_bytes(rt, source) } else { take(rt, source).unwrap_or_default() };
            match decode_png(&bytes) {
                Some((w, rgba)) => image(rt, w, rgba),
                None => crate::jlang::throw_with(rt, "java/io/IOException", "Could not load image: not a PNG"),
            }
        }
        "<init>" => {
            let (w, h) = if desc.starts_with("(Lcom/mojang/blaze3d/platform/NativeImage$Format;") { (a(2).i(), a(3).i()) } else { (a(1).i(), a(2).i()) };
            let (w, h) = (w.max(1) as usize, h.max(1) as usize);
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Bytes(vec![0; w * h * 4], w);
            }
            V::Void
        }
        "getWidth" => V::I(image_size(rt, a(0)).0 as i32),
        "getHeight" => V::I(image_size(rt, a(0)).1 as i32),
        "getPixelRGBA" | "setPixelRGBA" => {
            let (w, h) = image_size(rt, a(0));
            let (x, y) = (a(1).i(), a(2).i());
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                let msg = format!("({x}, {y}) outside of image bounds ({w}, {h})");
                return Some(crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &msg));
            }
            let at = (y as usize * w + x as usize) * 4;
            let Some(Data::Bytes(b, _)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data) else { return Some(V::I(0)) };
            if name == "setPixelRGBA" {
                b[at..at + 4].copy_from_slice(&a(3).i().to_le_bytes());
                V::Void
            } else {
                V::I(i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]))
            }
        }
        "getPixelsRGBA" => {
            let pixels: Vec<V> = match rt.heap.get(a(0).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b.chunks_exact(4).map(|p| V::I(i32::from_le_bytes([p[0], p[1], p[2], p[3]]))).collect(),
                _ => Vec::new(),
            };
            rt.alloc("[I", Data::Array(pixels))
        }
        // The address of its pixels in vanilla, read as the field it is to see whether the image
        // is still open (JourneyMap keeps a region's image while `pixels != 0`, and made a new
        // one of 512 by 512 pixels at every tick while this read as 0): not 0 until closed.
        "$pixels" => V::J(if rt.get_field(a(0), "$closed").i() != 0 {
            0
        } else {
            (1 << 32) | i64::from(a(0).r())
        }),
        "close" => {
            rt.put_field(a(0), "$closed", V::I(1));
            V::Void
        }
        "upload" | "untrack" | "downloadTexture" => V::Void,
        "format" => {
            crate::api::get_static(rt, "com/mojang/blaze3d/platform/NativeImage$Format.RGBA")
        }
        // A pixel's channels (alpha is the top byte of the pixel as an int).
        "getLuminanceOrAlpha"
        | "getRedOrLuminance"
        | "getGreenOrLuminance"
        | "getBlueOrLuminance" => {
            let (w, h) = image_size(rt, a(0));
            let (x, y) = (a(1).i(), a(2).i());
            let channel = match name {
                "getRedOrLuminance" => 0,
                "getGreenOrLuminance" => 1,
                "getBlueOrLuminance" => 2,
                _ => 3,
            };
            match rt.heap.get(a(0).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _))
                    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h =>
                {
                    V::I(i32::from(
                        b[(y as usize * w + x as usize) * 4 + channel] as i8,
                    ))
                }
                _ => V::I(0),
            }
        }
        // `fillRect(x, y, width, height, colour)`.
        "fillRect" => {
            let (w, h) = image_size(rt, a(0));
            let (x, y, wide, high, colour) = (
                a(1).i(),
                a(2).i(),
                a(3).i(),
                a(4).i(),
                a(5).i().to_le_bytes(),
            );
            if x < 0
                || y < 0
                || wide < 0
                || high < 0
                || (x + wide) as usize > w
                || (y + high) as usize > h
            {
                let msg = format!(
                    "({}, {}) outside of image bounds ({w}, {h})",
                    x + wide - 1,
                    y + high - 1
                );
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalArgumentException",
                    &msg,
                ));
            }
            if let Some(Data::Bytes(b, _)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data)
            {
                for row in y as usize..(y + high) as usize {
                    for pixel in b[(row * w + x as usize) * 4..(row * w + (x + wide) as usize) * 4]
                        .chunks_exact_mut(4)
                    {
                        pixel.copy_from_slice(&colour);
                    }
                }
            }
            V::Void
        }
        // `blendPixel(x, y, colour)`: the colour over the pixel by the colour's alpha, as
        // vanilla's floats work it out.
        "blendPixel" => {
            let (w, h) = image_size(rt, a(0));
            let (x, y, over) = (a(1).i(), a(2).i(), a(3).i().to_le_bytes());
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                let msg = format!("({x}, {y}) outside of image bounds ({w}, {h})");
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalArgumentException",
                    &msg,
                ));
            }
            let at = (y as usize * w + x as usize) * 4;
            if let Some(Data::Bytes(b, _)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data)
            {
                let part = |c: u8| f32::from(c) / 255.0;
                let (alpha, under) = (part(over[3]), 1.0 - part(over[3]));
                let mixed = |own: f32, held: u8| {
                    ((own * alpha + part(held) * under).min(1.0) * 255.0) as u8
                };
                let out = [
                    mixed(part(over[0]), b[at]),
                    mixed(part(over[1]), b[at + 1]),
                    mixed(part(over[2]), b[at + 2]),
                    mixed(alpha, b[at + 3]),
                ];
                b[at..at + 4].copy_from_slice(&out);
            }
            V::Void
        }
        // `copyFrom(other)`: the other's pixels, as many of each row and as many rows as both have.
        "copyFrom" => {
            let ((w, h), (ow, oh)) = (image_size(rt, a(0)), image_size(rt, a(1)));
            let from = match rt.heap.get(a(1).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b.clone(),
                _ => return Some(V::Void),
            };
            if let Some(Data::Bytes(b, _)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data)
            {
                let wide = w.min(ow) * 4;
                for row in 0..h.min(oh) {
                    b[row * w * 4..row * w * 4 + wide]
                        .copy_from_slice(&from[row * ow * 4..row * ow * 4 + wide]);
                }
            }
            V::Void
        }
        // `copyRect(xFrom, yFrom, xBy, yBy, width, height, mirrorX, mirrorY)` within the image, and
        // `copyRect(target, xFrom, yFrom, xTo, yTo, width, height, mirrorX, mirrorY)` into another.
        "copyRect" => {
            let other = desc.starts_with("(Lcom/mojang/blaze3d/platform/NativeImage;");
            let n = |i: usize| a(i + usize::from(other)).i();
            let (target, x_from, y_from) = (if other { a(1) } else { a(0) }, n(1), n(2));
            let (x_to, y_to) = if other {
                (n(3), n(4))
            } else {
                (x_from + n(3), y_from + n(4))
            };
            let (wide, high, mirror_x, mirror_y) = (n(5), n(6), n(7) != 0, n(8) != 0);
            let ((w, h), (tw, th)) = (image_size(rt, a(0)), image_size(rt, target));
            let inside = |x: i32, y: i32, w: usize, h: usize| {
                x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h
            };
            if wide > 0
                && high > 0
                && !(inside(x_from, y_from, w, h)
                    && inside(x_from + wide - 1, y_from + high - 1, w, h)
                    && inside(x_to, y_to, tw, th)
                    && inside(x_to + wide - 1, y_to + high - 1, tw, th))
            {
                let msg = format!("({x_to}, {y_to}) outside of image bounds ({tw}, {th})");
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalArgumentException",
                    &msg,
                ));
            }
            let from = match rt.heap.get(a(0).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b.clone(),
                _ => return Some(V::Void),
            };
            if let Some(Data::Bytes(b, _)) =
                rt.heap.get_mut(target.r() as usize).map(|o| &mut o.data)
            {
                for i in 0..high.max(0) {
                    for j in 0..wide.max(0) {
                        let (k, l) = (
                            if mirror_x { wide - 1 - j } else { j },
                            if mirror_y { high - 1 - i } else { i },
                        );
                        let (src, dst) = (
                            ((y_from + i) as usize * w + (x_from + j) as usize) * 4,
                            ((y_to + l) as usize * tw + (x_to + k) as usize) * 4,
                        );
                        b[dst..dst + 4].copy_from_slice(&from[src..src + 4]);
                    }
                }
            }
            V::Void
        }
        // `mappedCopy(function)`: a new image of what the function makes of each pixel;
        // `applyToAllPixels(function)`: the same in place.
        "mappedCopy" | "applyToAllPixels" => {
            let (w, _) = image_size(rt, a(0));
            let mut pixels = match rt.heap.get(a(0).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b.clone(),
                _ => Vec::new(),
            };
            for pixel in pixels.chunks_exact_mut(4) {
                let made = rt.call_functional(
                    "java/util/function/IntUnaryOperator.applyAsInt.(I)I",
                    a(1),
                    &[V::I(i32::from_le_bytes([
                        pixel[0], pixel[1], pixel[2], pixel[3],
                    ]))],
                );
                if rt.thrown.is_some() {
                    return Some(V::NULL);
                }
                pixel.copy_from_slice(&made.i().to_le_bytes());
            }
            if name == "mappedCopy" {
                image(rt, w, pixels)
            } else {
                if let Some(Data::Bytes(b, _)) =
                    rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data)
                {
                    *b = pixels;
                }
                V::Void
            }
        }
        // `makePixelArray()`: each pixel as ARGB, where the image holds it as ABGR.
        "makePixelArray" => {
            let pixels: Vec<V> = match rt.heap.get(a(0).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b
                    .chunks_exact(4)
                    .map(|p| V::I(i32::from_be_bytes([p[3], p[0], p[1], p[2]])))
                    .collect(),
                _ => Vec::new(),
            };
            rt.alloc("[I", Data::Array(pixels))
        }
        "flipY" => {
            let (w, _) = image_size(rt, a(0));
            if let Some(Data::Bytes(b, _)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data)
                && w > 0
            {
                let rows: Vec<u8> = b.chunks_exact(w * 4).rev().flatten().copied().collect();
                *b = rows;
            }
            V::Void
        }
        // The image as a PNG: its bytes, or written to a file (JourneyMap saves its regions so).
        "asByteArray" | "writeToFile" => {
            let (w, _) = image_size(rt, a(0));
            let png = match rt.heap.get(a(0).r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => encode_png(w, b),
                _ => None,
            };
            let Some(png) = png else {
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/io/IOException",
                    "Could not write image",
                ));
            };
            if name == "asByteArray" {
                crate::files::byte_array(rt, &png)
            } else {
                let text = rt.str_of(a(1)).unwrap_or("").to_owned();
                match std::fs::write(crate::files::on_disk(rt, &text), png) {
                    Ok(()) => V::Void,
                    Err(e) => crate::files::throw_io(rt, &e, &text),
                }
            }
        }
        _ => return None,
    })
}

/// RGBA bytes of `width` pixels a row as a PNG.
pub(crate) fn encode_png(width: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || rgba.is_empty() {
        return None;
    }
    let height = rgba.len() / 4 / width;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            &mut out,
            u32::try_from(width).ok()?,
            u32::try_from(height).ok()?,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba[..width * height * 4]).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// A reload listener of a mod's has what NeoForge gives each of them: the registries, and
    /// a context for its conditions.
    #[test]
    fn a_reload_listener_has_the_registries_and_its_context() {
        let mut rt = Rt::new();
        rt.supers.insert(
            "t/Listener",
            "net/minecraft/server/packs/resources/SimpleJsonResourceReloadListener",
        );
        let listener = rt.new_object("t/Listener");
        let access = crate::api::call(
            &mut rt,
            "net/minecraft/server/packs/resources/SimpleJsonResourceReloadListener.getRegistryLookup.()Lnet/minecraft/core/HolderLookup$Provider;",
            &[listener],
        );
        assert_eq!(access, registry_access(&mut rt));
        // (Written on a listener class that names no class of the game's above it.)
        assert_eq!(
            crate::api::call(
                &mut rt,
                "java/lang/Object.getRegistryLookup.()Lnet/minecraft/core/HolderLookup$Provider;",
                &[listener]
            ),
            access
        );
        let context = crate::api::call(
            &mut rt,
            "java/lang/Object.getContext.()Lnet/neoforged/neoforge/common/conditions/ICondition$IContext;",
            &[listener],
        );
        assert_eq!(
            rt.class_of(context),
            "net/neoforged/neoforge/common/conditions/ICondition$IContext"
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    fn png_2x1() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 2, 1);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 16, 32, 48, 128]).unwrap();
        }
        out
    }

    fn host() -> MemoryHost {
        let mut h = MemoryHost::default();
        h.resources.insert(
            "assets/drawers/models/block/geometry/area.json".into(),
            br##"{"parent": "block/block", "elements": [{"from": [1, 2, 0.5], "to": [15, 14, 1], "faces": {"north": {"uv": [0, 0, 16, 16], "texture": "#front", "cullface": "north"}}}]}"##.to_vec(),
        );
        h.resources.insert("assets/drawers/textures/colors.png".into(), png_2x1());
        h.resources.insert("assets/drawers/lang/en_us.json".into(), b"{\"a\": \"b\"}\nsecond line".to_vec());
        h.resources.insert("assets/other/geo/thing.geo.json".into(), b"{}".to_vec());
        h.resources.insert("assets/other/geo/notes.txt".into(), b"no".to_vec());
        h
    }

    /// StorageDrawers' read of a model's elements (`getResourceOrThrow`, `open`, an
    /// `InputStreamReader`, `BlockModel.fromStream`) and Mekanism's of a texture's pixels
    /// (`NativeImage.read(manager.open(...))`, ABGR as 1.21.1 answers them).
    #[test]
    fn a_mod_reads_the_games_resources_through_the_manager() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = host();
        rt.with_host(&mut h, |rt| {
            let mc = rt.new_object(MINECRAFT);
            let m = crate::api::call(rt, "net/minecraft/client/Minecraft.getResourceManager.()Lnet/minecraft/server/packs/resources/ResourceManager;", &[mc]);
            assert!(rt.instance_of(m, RELOADABLE) && rt.instance_of(m, MANAGER));
            let loc = rt.string("drawers:models/block/geometry/area.json");
            let res = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.getResourceOrThrow.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/server/packs/resources/Resource;", &[m, loc]);
            let stream = crate::api::call(rt, "net/minecraft/server/packs/resources/Resource.open.()Ljava/io/InputStream;", &[res]);
            let reader = rt.new_object(STREAM_READER);
            let utf8 = crate::api::get_static(rt, "java/nio/charset/StandardCharsets.UTF_8");
            crate::api::call(rt, "java/io/InputStreamReader.<init>.(Ljava/io/InputStream;Ljava/nio/charset/Charset;)V", &[reader, stream, utf8]);
            let model = crate::api::call(rt, "net/minecraft/client/renderer/block/model/BlockModel.fromStream.(Ljava/io/Reader;)Lnet/minecraft/client/renderer/block/model/BlockModel;", &[reader]);
            let elements = crate::api::call(rt, "net/minecraft/client/renderer/block/model/BlockModel.getElements.()Ljava/util/List;", &[model]);
            let all = crate::jutil::elements(rt, elements);
            assert_eq!(all.len(), 1);
            let (from, to) = (rt.get_field(all[0], "from"), rt.get_field(all[0], "to"));
            assert_eq!([rt.get_field(from, "x"), rt.get_field(from, "y"), rt.get_field(from, "z")], [V::F(1.0), V::F(2.0), V::F(0.5)]);
            assert_eq!([rt.get_field(to, "x"), rt.get_field(to, "y"), rt.get_field(to, "z")], [V::F(15.0), V::F(14.0), V::F(1.0)]);
            let parent = crate::api::call(rt, "net/minecraft/client/renderer/block/model/BlockModel.getParentLocation.()Lnet/minecraft/resources/ResourceLocation;", &[model]);
            assert_eq!(rt.str_of(parent), Some("minecraft:block/block"));

            let missing = rt.string("drawers:models/none.json");
            crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.getResourceOrThrow.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/server/packs/resources/Resource;", &[m, missing]);
            let thrown = rt.thrown.take().expect("a missing resource throws");
            assert_eq!(rt.class_of(thrown), "java/io/FileNotFoundException");
            let found = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.getResource.(Lnet/minecraft/resources/ResourceLocation;)Ljava/util/Optional;", &[m, missing]);
            assert_eq!(rt.get_field(found, "value"), V::NULL);

            let tex = rt.string("drawers:textures/colors.png");
            let input = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.open.(Lnet/minecraft/resources/ResourceLocation;)Ljava/io/InputStream;", &[m, tex]);
            let image = crate::api::call(rt, "com/mojang/blaze3d/platform/NativeImage.read.(Ljava/io/InputStream;)Lcom/mojang/blaze3d/platform/NativeImage;", &[input]);
            assert_eq!(crate::api::call(rt, "com/mojang/blaze3d/platform/NativeImage.getWidth.()I", &[image]), V::I(2));
            assert_eq!(crate::api::call(rt, "com/mojang/blaze3d/platform/NativeImage.getHeight.()I", &[image]), V::I(1));
            let pixel = |rt: &mut Rt, x: i32| crate::api::call(rt, "com/mojang/blaze3d/platform/NativeImage.getPixelRGBA.(II)I", &[image, V::I(x), V::I(0)]);
            assert_eq!(pixel(rt, 0), V::I(0xff0000ffu32 as i32));
            assert_eq!(pixel(rt, 1), V::I(0x80302010u32 as i32));
            pixel(rt, 2);
            assert_eq!(rt.thrown.take().map(|e| rt.class_of(e).to_owned()).as_deref(), Some("java/lang/IllegalArgumentException"));
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    /// An image a mod makes and draws into: open until closed (the `pixels` field vanilla keeps
    /// its address in), filled, blended, copied and flipped as vanilla's does, its pixels as
    /// ARGB for whoever asks so, and written as a PNG that reads back the same.
    #[test]
    fn an_image_is_drawn_into_and_written_as_a_png() {
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, name: &str, args: &[V]| {
            let key = rt.static_key(&format!("com/mojang/blaze3d/platform/NativeImage.{name}"));
            crate::api::call(rt, key, args)
        };
        let image = rt.new_object(NATIVE_IMAGE);
        c(
            &mut rt,
            "<init>.(IIZ)V",
            &[image, V::I(4), V::I(2), V::I(0)],
        );
        assert!(c(&mut rt, "$pixels.()J", &[image]).j() != 0);
        let format = c(
            &mut rt,
            "format.()Lcom/mojang/blaze3d/platform/NativeImage$Format;",
            &[image],
        );
        assert_eq!(
            crate::api::call(
                &mut rt,
                "com/mojang/blaze3d/platform/NativeImage$Format.components.()I",
                &[format]
            ),
            V::I(4)
        );
        // Opaque red (ABGR) over the left half, then half-transparent blue over one pixel of it.
        c(
            &mut rt,
            "fillRect.(IIIII)V",
            &[
                image,
                V::I(0),
                V::I(0),
                V::I(2),
                V::I(2),
                V::I(0xff0000ffu32 as i32),
            ],
        );
        c(
            &mut rt,
            "blendPixel.(III)V",
            &[image, V::I(1), V::I(0), V::I(0x80ff0000u32 as i32)],
        );
        let pixel = |rt: &mut Rt, of: V, x: i32, y: i32| {
            c(rt, "getPixelRGBA.(II)I", &[of, V::I(x), V::I(y)]).i() as u32
        };
        assert_eq!(pixel(&mut rt, image, 0, 1), 0xff0000ff);
        assert_eq!(pixel(&mut rt, image, 2, 0), 0);
        // (Vanilla's own arithmetic: alpha 128/255 over an opaque pixel.)
        assert_eq!(pixel(&mut rt, image, 1, 0), 0xbf80007e);
        assert_eq!(
            c(
                &mut rt,
                "getLuminanceOrAlpha.(II)B",
                &[image, V::I(0), V::I(0)]
            ),
            V::I(-1)
        );
        // The left half copied to the right, mirrored; then the rows turned over.
        c(
            &mut rt,
            "copyRect.(IIIIIIZZ)V",
            &[
                image,
                V::I(0),
                V::I(0),
                V::I(2),
                V::I(0),
                V::I(2),
                V::I(2),
                V::I(1),
                V::I(0),
            ],
        );
        assert_eq!(
            [pixel(&mut rt, image, 2, 0), pixel(&mut rt, image, 3, 0)],
            [0xbf80007e, 0xff0000ff]
        );
        c(&mut rt, "flipY.()V", &[image]);
        assert_eq!(
            [pixel(&mut rt, image, 1, 1), pixel(&mut rt, image, 1, 0)],
            [0xbf80007e, 0xff0000ff]
        );
        let argb = c(&mut rt, "makePixelArray.()[I", &[image]);
        assert_eq!(
            crate::jutil::elements(&rt, argb)[0],
            V::I(0xffff0000u32 as i32)
        );
        // As a PNG and back.
        let bytes = c(&mut rt, "asByteArray.()[B", &[image]);
        let png: Vec<u8> = crate::jutil::elements(&rt, bytes)
            .into_iter()
            .map(|b| b.i() as u8)
            .collect();
        let (width, rgba) = decode_png(&png).expect("a PNG");
        assert_eq!((width, rgba.len()), (4, 32));
        assert_eq!(&rgba[..4], &[0xff, 0, 0, 0xff]);
        // Another image takes its pixels; closed, the first says so.
        let other = rt.new_object(NATIVE_IMAGE);
        c(
            &mut rt,
            "<init>.(IIZ)V",
            &[other, V::I(4), V::I(2), V::I(0)],
        );
        c(
            &mut rt,
            "copyFrom.(Lcom/mojang/blaze3d/platform/NativeImage;)V",
            &[other, image],
        );
        assert_eq!(pixel(&mut rt, other, 1, 1), 0xbf80007e);
        c(&mut rt, "close.()V", &[image]);
        assert_eq!(c(&mut rt, "$pixels.()J", &[image]), V::J(0));
        assert!(c(&mut rt, "$pixels.()J", &[other]).j() != 0);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A reader's lines and chars; `listResources` finds the files under a folder that its filter
    /// takes, by location, in every namespace.
    /// Content folders are found by the namespaces they have files for, so a mod's own resource
    /// file is looked for only where one can be.
    #[test]
    fn content_folders_are_known_by_their_namespaces() {
        let dir = std::env::temp_dir().join(format!("hz-content-{}", std::process::id()));
        let (gears, belts, packed) = (
            dir.join("content-gears"),
            dir.join("content-belts"),
            dir.join("packed.hmod"),
        );
        std::fs::create_dir_all(gears.join("assets/gears/models/block")).unwrap();
        std::fs::create_dir_all(gears.join("data/gears/recipe")).unwrap();
        std::fs::create_dir_all(belts.join("assets/belts/lang")).unwrap();
        std::fs::create_dir_all(belts.join("assets/gears/lang")).unwrap();
        std::fs::write(&packed, b"PK").unwrap();
        let found = by_namespace(&[gears.clone(), belts.clone(), packed]);
        let of = |root: &str, namespace: &str| {
            found
                .get(&(root.to_owned(), namespace.to_owned()))
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(of("assets", "gears"), [gears.clone(), belts.clone()]);
        assert_eq!(of("assets", "belts"), [belts]);
        assert_eq!(of("data", "gears"), [gears]);
        assert!(of("data", "belts").is_empty() && of("assets", "minecraft").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn readers_and_listing_read_as_javas_do() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = host();
        rt.with_host(&mut h, |rt| {
            let m = manager(rt, "assets");
            let lang = rt.string("drawers:lang/en_us.json");
            let reader = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceProvider.openAsReader.(Lnet/minecraft/resources/ResourceLocation;)Ljava/io/BufferedReader;", &[m, lang]);
            assert_eq!(crate::api::call(rt, "java/io/Reader.read.()I", &[reader]), V::I('{' as i32));
            let line = crate::api::call(rt, "java/io/BufferedReader.readLine.()Ljava/lang/String;", &[reader]);
            assert_eq!(rt.str_of(line), Some("\"a\": \"b\"}"));
            let line = crate::api::call(rt, "java/io/BufferedReader.readLine.()Ljava/lang/String;", &[reader]);
            assert_eq!(rt.str_of(line), Some("second line"));
            assert_eq!(crate::api::call(rt, "java/io/BufferedReader.readLine.()Ljava/lang/String;", &[reader]), V::NULL);

            // A look at the next character and back (`mark(1)`, `read()`, `reset()`), as a
            // JSON5 reader looks ahead: the character is read again after it.
            let text = rt.string("{ab");
            let string_reader = rt.new_object("java/io/StringReader");
            crate::api::call(rt, "java/io/StringReader.<init>.(Ljava/lang/String;)V", &[string_reader, text]);
            let buffered = rt.new_object("java/io/BufferedReader");
            crate::api::call(rt, "java/io/BufferedReader.<init>.(Ljava/io/Reader;)V", &[buffered, string_reader]);
            assert_eq!(crate::api::call(rt, "java/io/Reader.markSupported.()Z", &[buffered]), V::I(1));
            let mut read = Vec::new();
            for _ in 0..3 {
                crate::api::call(rt, "java/io/Reader.mark.(I)V", &[buffered, V::I(1)]);
                let ahead = crate::api::call(rt, "java/io/Reader.read.()I", &[buffered]);
                crate::api::call(rt, "java/io/Reader.reset.()V", &[buffered]);
                let again = crate::api::call(rt, "java/io/Reader.read.()I", &[buffered]);
                read.push((ahead.i(), again.i()));
            }
            assert_eq!(read, [(123, 123), (97, 97), (98, 98)]);
            // A buffered reader that marked nothing has nowhere to go back to.
            let other = rt.new_object("java/io/BufferedReader");
            crate::api::call(rt, "java/io/BufferedReader.<init>.(Ljava/io/Reader;)V", &[other, buffered]);
            crate::api::call(rt, "java/io/Reader.reset.()V", &[other]);
            let thrown = rt.thrown.take().expect("no mark");
            assert_eq!(rt.class_of(thrown), "java/io/IOException");

            // A filter for `.json` files, as GeckoLib's.
            let json_only = rt.new_object("test/JsonOnly");
            rt.vcall = Some(|rt, _key, short, all| {
                if short == "test.(Ljava/lang/Object;)Z" {
                    return V::I(rt.str_of(all[1]).is_some_and(|s| s.ends_with(".json")) as i32);
                }
                V::Void
            });
            let folder = rt.string("geo");
            let found = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.listResources.(Ljava/lang/String;Ljava/util/function/Predicate;)Ljava/util/Map;", &[m, folder, json_only]);
            let keys: Vec<String> = crate::jutil::table_keys(rt, found).iter().map(|k| rt.str_of(*k).unwrap_or("").to_owned()).collect();
            assert_eq!(keys, ["other:geo/thing.geo.json"]);
        });
    }

    /// A mod's `SimplePreparableReloadListener`, registered through NeoForge's
    /// `RegisterClientReloadListenersEvent`, loads with the client's resources: `prepare` reads
    /// them, and `apply` gets what it made.
    #[test]
    fn a_registered_reload_listener_prepares_then_applies() {
        crate::install();
        let mut rt = Rt::new();
        rt.supers.insert("test/Colors", SIMPLE);
        // The mod's class as generated dispatch runs it: its own `prepare` and `apply`; `reload`,
        // which it doesn't override, is vanilla's.
        rt.vcall = Some(|rt, key, short, all| match short {
            "prepare.(Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)Ljava/lang/Object;" => {
                let loc = rt.string("drawers:lang/en_us.json");
                let reader = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.openAsReader.(Lnet/minecraft/resources/ResourceLocation;)Ljava/io/BufferedReader;", &[all[1], loc]);
                crate::api::call(rt, "java/io/BufferedReader.readLine.()Ljava/lang/String;", &[reader])
            }
            "apply.(Ljava/lang/Object;Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)V" => {
                rt.statics.insert("test/Colors.applied", all[1]);
                V::Void
            }
            _ => crate::api::call(rt, key, all),
        });
        let listener = rt.new_object_id("test/Colors", 7);
        let event = client_listeners_event(&mut rt);
        crate::api::call(&mut rt, "net/neoforged/neoforge/client/event/RegisterClientReloadListenersEvent.registerReloadListener.(Lnet/minecraft/server/packs/resources/PreparableReloadListener;)V", &[event, listener]);
        assert!(rt.statics.get("test/Colors.applied").is_none(), "nothing loads before the resources do");
        let mut h = host();
        rt.with_host(&mut h, |rt| reload(rt, "assets"));
        let applied = rt.statics.get("test/Colors.applied").copied().expect("apply ran");
        assert_eq!(rt.str_of(applied), Some("{\"a\": \"b\"}"));
        assert!(rt.thrown.is_none());
    }

    /// Create's train hat info: `FileToIdConverter.json(folder)` lists the folder's JSON files in
    /// every namespace, and a file reads through `openAsReader` into `GsonHelper.parse`. A mod's
    /// `SimpleJsonResourceReloadListener` gets the same files as trees by id in its `apply`.
    #[test]
    fn json_files_list_by_id_and_reach_a_json_listener() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = host();
        h.resources.insert("assets/create/train_hat_info/pig.json".into(), br#"{"part": "head", "offset": [0, 1, 0]}"#.to_vec());
        h.resources.insert("assets/create/train_hat_info/sub/cow.json".into(), br#"{"part": "body"}"#.to_vec());
        h.resources.insert("assets/create/train_hat_info/readme.txt".into(), b"not json".to_vec());
        h.resources.insert("assets/other/train_hat_info/bad.json".into(), b"{oops".to_vec());
        rt.supers.insert("test/Hats", SIMPLE_JSON);
        rt.vcall = Some(|rt, key, short, all| match short {
            "apply.(Ljava/lang/Object;Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)V" => {
                rt.statics.insert("test/Hats.applied", all[1]);
                V::Void
            }
            _ => crate::api::call(rt, key, all),
        });
        rt.with_host(&mut h, |rt| {
            let m = manager(rt, "assets");
            let folder = rt.string("train_hat_info");
            let converter = crate::api::call(rt, "net/minecraft/resources/FileToIdConverter.json.(Ljava/lang/String;)Lnet/minecraft/resources/FileToIdConverter;", &[folder]);
            let found = crate::api::call(rt, "net/minecraft/resources/FileToIdConverter.listMatchingResources.(Lnet/minecraft/server/packs/resources/ResourceManager;)Ljava/util/Map;", &[converter, m]);
            let keys: Vec<String> = crate::jutil::table_keys(rt, found).iter().map(|k| rt.str_of(*k).unwrap_or("").to_owned()).collect();
            assert_eq!(keys, ["create:train_hat_info/pig.json", "create:train_hat_info/sub/cow.json", "other:train_hat_info/bad.json"]);
            let cow = rt.string("create:train_hat_info/sub/cow.json");
            let id = crate::api::call(rt, "net/minecraft/resources/FileToIdConverter.fileToId.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceLocation;", &[converter, cow]);
            assert_eq!(rt.str_of(id), Some("create:sub/cow"));
            let pig = rt.string("create:train_hat_info/pig.json");
            let res = crate::jutil::table_get(rt, found, pig).expect("listed");
            let reader = crate::api::call(rt, "net/minecraft/server/packs/resources/Resource.openAsReader.()Ljava/io/BufferedReader;", &[res]);
            let json = crate::api::call(rt, "net/minecraft/util/GsonHelper.parse.(Ljava/io/Reader;)Lcom/google/gson/JsonObject;", &[reader]);
            let part = rt.string("part");
            let s = crate::api::call(rt, "net/minecraft/util/GsonHelper.getAsString.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Ljava/lang/String;", &[json, part]);
            assert_eq!(rt.str_of(s), Some("head"));

            // The listener: `super(gson, "train_hat_info")`, registered, then the resources load.
            let listener = rt.new_object_id("test/Hats", 9);
            let gson = rt.new_object(crate::gson::GSON);
            crate::api::call(rt, "net/minecraft/server/packs/resources/SimpleJsonResourceReloadListener.<init>.(Lcom/google/gson/Gson;Ljava/lang/String;)V", &[listener, gson, folder]);
            crate::api::call(rt, "net/minecraft/server/packs/resources/ReloadableResourceManager.registerReloadListener.(Lnet/minecraft/server/packs/resources/PreparableReloadListener;)V", &[m, listener]);
            reload(rt, "assets");
            let applied = rt.statics.get("test/Hats.applied").copied().expect("apply ran");
            let mut ids: Vec<String> = crate::jutil::table_keys(rt, applied).iter().map(|k| rt.str_of(*k).unwrap_or("").to_owned()).collect();
            ids.sort();
            // The file that isn't JSON is left out, as vanilla logs and skips it.
            assert_eq!(ids, ["create:pig", "create:sub/cow"]);
            let cow = rt.string("create:sub/cow");
            let tree = crate::jutil::table_get(rt, applied, cow).expect("read");
            assert_eq!(crate::gson::write(rt, tree, crate::gson::TO_STRING), r#"{"part":"body"}"#);
            assert!(rt.thrown.is_none());
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    /// The server's data: each load (a world's open, then `/reload`) gathers its listeners anew
    /// through NeoForge's `AddReloadListenerEvent`, where the mod adds a new one, and that one reads
    /// the server's files (`data/`, not the client's `assets/`). Two loads run it twice, not three
    /// times.
    #[test]
    fn data_listeners_gather_anew_on_each_server_load() {
        // The mod's handler, a lambda: `event.addListener(new Spouts())`.
        fn run(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            (id == 1).then(|| {
                let l = rt.new_object_id("test/Spouts", 7);
                crate::api::call(rt, "net/neoforged/neoforge/event/AddReloadListenerEvent.addListener.(Lnet/minecraft/server/packs/resources/PreparableReloadListener;)V", &[args[1], l])
            })
        }
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(run);
        rt.supers.insert("test/Spouts", SIMPLE);
        rt.vcall = Some(|rt, key, short, all| match short {
            "prepare.(Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)Ljava/lang/Object;" => {
                let loc = rt.string("create:fluids/honey.json");
                let reader = crate::api::call(rt, "net/minecraft/server/packs/resources/ResourceManager.openAsReader.(Lnet/minecraft/resources/ResourceLocation;)Ljava/io/BufferedReader;", &[all[1], loc]);
                crate::api::call(rt, "java/io/BufferedReader.readLine.()Ljava/lang/String;", &[reader])
            }
            "apply.(Ljava/lang/Object;Lnet/minecraft/server/packs/resources/ResourceManager;Lnet/minecraft/util/profiling/ProfilerFiller;)V" => {
                let runs = rt.statics.get("test/Spouts.runs").map_or(0, |v| v.i());
                rt.statics.insert("test/Spouts.runs", V::I(runs + 1));
                rt.statics.insert("test/Spouts.applied", all[1]);
                V::Void
            }
            _ => crate::api::call(rt, key, all),
        });
        let handler = rt.lambda_id("test/Mod.lambda$onReload$0.(Lnet/neoforged/neoforge/event/AddReloadListenerEvent;)V", 1, Vec::new());
        rt.listeners.push((SERVER_LISTENERS_EVENT.to_owned(), handler));
        let mut h = host();
        h.resources.insert("data/create/fluids/honey.json".into(), br#"{"thick": true}"#.to_vec());
        h.resources.insert("assets/create/fluids/honey.json".into(), br#"{"client": true}"#.to_vec());
        rt.with_host(&mut h, server_reload);
        let applied = rt.statics.get("test/Spouts.applied").copied().expect("apply ran");
        assert_eq!(rt.str_of(applied), Some(r#"{"thick": true}"#));
        rt.with_host(&mut h, server_reload);
        assert_eq!(rt.statics.get("test/Spouts.runs").map(|v| v.i()), Some(2));
        assert!(rt.thrown.is_none());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Each load of the server's data makes its `ReloadableServerResources` anew: what a mixin
    /// injects at the end of its constructor runs on each, handed the registries, and finds the
    /// recipe manager in the object's field (Cucumber takes it from there); what it injects into
    /// `updateRegistryTags` runs on the same object once the tags are bound.
    #[test]
    fn each_server_data_load_constructs_its_resources() {
        const INFO: &str = "Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;";
        fn handler(rt: &mut Rt, lid: u32, args: &[V]) -> Option<V> {
            // The lambda's slot, the object, the handler's arguments, the `CallbackInfo`.
            let seen = rt.interned["test:seen"];
            let recipes = crate::api::game_field(
                rt,
                args[1],
                "recipes",
                "$recipes",
                "net/minecraft/server/ReloadableServerResources.$recipes.()Lnet/minecraft/world/item/crafting/RecipeManager;",
            );
            let registries = if lid == 1 { args[2] } else { V::NULL };
            for v in [V::I(lid as i32), args[1], recipes, registries] {
                crate::jutil::list_add(rt, seen, v);
            }
            Some(V::Void)
        }
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(handler);
        let seen = crate::jutil::new_list(&mut rt, Vec::new());
        rt.interned.insert("test:seen".into(), seen);
        let made: &'static str = Box::leak(
            format!("t/ResourcesMixin.made.(Lnet/minecraft/core/RegistryAccess$Frozen;Lnet/minecraft/world/flag/FeatureFlagSet;Lnet/minecraft/commands/Commands$CommandSelection;I{INFO})V").into_boxed_str(),
        );
        let tagged: &'static str =
            Box::leak(format!("t/ResourcesMixin.tagged.({INFO})V").into_boxed_str());
        rt.subscribers.insert(
            "t/ResourcesMixin",
            vec![
                ("hz/inject:net/minecraft/server/ReloadableServerResources.<init>@RETURN", made, false, 1),
                ("hz/inject:net/minecraft/server/ReloadableServerResources.updateRegistryTags@TAIL", tagged, false, 2),
            ],
        );
        let mut h = host();
        rt.with_host(&mut h, server_reload);
        rt.with_host(&mut h, server_reload);
        assert!(rt.thrown.is_none());
        let seen = crate::jutil::elements(&rt, seen);
        let calls: Vec<&[V]> = seen.chunks(4).collect();
        assert_eq!(
            calls.iter().map(|c| c[0].i()).collect::<Vec<_>>(),
            [1, 2, 1, 2]
        );
        // One object a load, with its recipe manager; the constructor's handler got registries.
        assert!(
            calls[0][1] == calls[1][1] && calls[2][1] == calls[3][1] && calls[0][1] != calls[2][1]
        );
        assert!(
            calls
                .iter()
                .all(|c| rt.class_of(c[2]) == "net/minecraft/world/item/crafting/RecipeManager")
        );
        assert!(calls[0][3].r() != 0 && calls[2][3].r() != 0);
    }
}
