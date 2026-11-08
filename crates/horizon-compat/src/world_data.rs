//! The world's folder and what the mods keep in it. A server's world is a folder (the host's
//! [`crate::host::Host::world_dir`], kept for the hooks whose host can't tell, a block's tick):
//! `MinecraftServer.getWorldPath(LevelResource)` gives its files by the ids 1.21.1 names them,
//! where the game keeps them (26.3 moved the players' files under `players/`), and
//! `LevelStorageAccess` and `DimensionType.getStorageFolder` its folders, as 1.21.1 lays them out.
//!
//! A level's saved data (`ServerLevel.getDataStorage()`: Create's railways, a mod's networks) is
//! NeoForge's `DimensionDataStorage`: one object a name, read from its file the first time a mod
//! asks for it (`get`, `computeIfAbsent`: the factory's deserializer gets the file's `data`), and
//! written back as the game saves the level (`level_saved`, then `LevelEvent.Save`) when a mod
//! marked it dirty: `{data, DataVersion}`, gzipped, into a file beside it moved over the old one,
//! as `SavedData.save(File, Provider)` writes it. A dimension's data is in the `data/` folder of
//! its 1.21.1 folder (`DimensionType.getStorageFolder`: the world's own for the Overworld, `DIM-1`
//! for the Nether), where the game's upgrade of a 1.21.1 world leaves a mod's files: a world
//! brought from 1.21.1 keeps its mods' data. What a mod couldn't read back (no world folder known
//! yet, a file that isn't NBT, a deserializer that threw) is never written over: the player's file
//! stays as it was. A client's runtime keeps its saved data in memory (a client has no world).
use crate::nbt::Nbt;
use crate::{Data, Rt, Table, V};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SERVER: &str = "net/minecraft/server/MinecraftServer";
const INTEGRATED: &str = "net/minecraft/client/server/IntegratedServer";
const DEDICATED: &str = "net/minecraft/server/dedicated/DedicatedServer";
const LEVEL_RESOURCE: &str = "net/minecraft/world/level/storage/LevelResource";
const ACCESS: &str = "net/minecraft/world/level/storage/LevelStorageSource$LevelStorageAccess";
const DIMENSION_TYPE: &str = "net/minecraft/world/level/dimension/DimensionType";
const SAVED_DATA: &str = "net/minecraft/world/level/saveddata/SavedData";
const FACTORY: &str = "net/minecraft/world/level/saveddata/SavedData$Factory";
const STORAGE: &str = "net/minecraft/world/level/storage/DimensionDataStorage";
const CHUNK_CACHE: &str = "net/minecraft/server/level/ServerChunkCache";
const LEVEL_SAVE: &str = "net/neoforged/neoforge/event/level/LevelEvent$Save";
const LEVEL_EVENT: &str = "net/neoforged/neoforge/event/level/LevelEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
/// `new SavedData.Factory(constructor, deserializer)`: no data fixer's type.
const SHORT_FACTORY: &str = "(Ljava/util/function/Supplier;Ljava/util/function/BiFunction;)V";
const FACTORY_INIT: &str = "net/minecraft/world/level/saveddata/SavedData$Factory.<init>.(Ljava/util/function/Supplier;Ljava/util/function/BiFunction;Lnet/minecraft/util/datafix/DataFixTypes;)V";
const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const APPLY: &str =
    "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
/// A saved data's methods as the JVM dispatches them: a mod's class has its own `save`.
type Virtual = (&'static str, &'static str);
const SAVE_TAG: Virtual = (
    "net/minecraft/world/level/saveddata/SavedData.save.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;",
    "save.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;",
);
const SAVE_FILE: Virtual = (
    "net/minecraft/world/level/saveddata/SavedData.save.(Ljava/io/File;Lnet/minecraft/core/HolderLookup$Provider;)V",
    "save.(Ljava/io/File;Lnet/minecraft/core/HolderLookup$Provider;)V",
);
const IS_DIRTY: Virtual = (
    "net/minecraft/world/level/saveddata/SavedData.isDirty.()Z",
    "isDirty.()Z",
);
const SET_DIRTY: Virtual = (
    "net/minecraft/world/level/saveddata/SavedData.setDirty.(Z)V",
    "setDirty.(Z)V",
);
/// 1.21.1's data version, which a saved data's file carries (`NbtUtils.addCurrentDataVersion`).
const DATA_VERSION: i32 = 3955;
/// Where the runtime keeps the world's folder once a host told it.
const WORLD: &str = "compat:world-dir";
const OVERWORLD: &str = "minecraft:overworld";
/// `LevelResource`'s constants and their ids, as 1.21.1 declares them.
const RESOURCES: [(&str, &str); 12] = [
    ("PLAYER_ADVANCEMENTS_DIR", "advancements"),
    ("PLAYER_STATS_DIR", "stats"),
    ("PLAYER_DATA_DIR", "playerdata"),
    ("PLAYER_OLD_DATA_DIR", "players"),
    ("LEVEL_DATA_FILE", "level.dat"),
    ("OLD_LEVEL_DATA_FILE", "level.dat_old"),
    ("ICON_FILE", "icon.png"),
    ("LOCK_FILE", "session.lock"),
    ("GENERATED_DIR", "generated"),
    ("DATAPACK_DIR", "datapacks"),
    ("MAP_RESOURCE_FILE", "resources.zip"),
    ("ROOT", "."),
];
/// Where 26.3 keeps what 1.21.1 kept at a `LevelResource`'s id (the game's upgrade of an older
/// world moves them there): a mod reading the players' files finds the game's.
const MOVED: [(&str, &str); 4] = [
    ("advancements", "players/advancements"),
    ("stats", "players/stats"),
    ("playerdata", "players/data"),
    ("resources.zip", "resourcepacks/resources.zip"),
];

pub fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        SERVER | INTEGRATED | DEDICATED => matches!(
            name,
            "getWorldPath" | "getServerDirectory" | "$storageSource"
        ),
        LEVEL_RESOURCE => matches!(name, "<init>" | "getId" | "toString"),
        ACCESS => matches!(
            name,
            "getLevelId" | "getLevelPath" | "getDimensionPath" | "getWorldDir"
        ),
        DIMENSION_TYPE => name == "getStorageFolder",
        SAVED_DATA => matches!(name, "<init>" | "setDirty" | "isDirty" | "save"),
        FACTORY => name == "<init>" && desc == SHORT_FACTORY,
        STORAGE => matches!(name, "computeIfAbsent" | "get" | "set" | "save"),
        CHUNK_CACHE => name == "getDataStorage",
        o => name == "getDataStorage" && crate::api::is_level(o),
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (SERVER | INTEGRATED | DEDICATED, "getWorldPath") | (ACCESS, "getLevelPath") => {
            world_path(rt, a(1))
        }
        // `Path.of("")`: the game's folder, where a dedicated server runs.
        (SERVER | INTEGRATED | DEDICATED, "getServerDirectory") => crate::files::new_path(rt, ""),
        // The server's `storageSource` (an access transformer opens it).
        (SERVER | INTEGRATED | DEDICATED, _) => {
            rt.intern("compat:level-storage".into(), |rt| rt.new_object(ACCESS))
        }
        (LEVEL_RESOURCE, "<init>") => {
            rt.put_field(a(0), "id", a(1));
            V::Void
        }
        (LEVEL_RESOURCE, "getId") => rt.get_field(a(0), "id"),
        (LEVEL_RESOURCE, _) => {
            let id = resource_id(rt, a(0));
            rt.string(&format!("/{id}"))
        }
        // The world's folder name, as `saves/` lists it.
        (ACCESS, "getLevelId") => match world_folder(rt) {
            Some(w) => {
                let id = Path::new(&w)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                rt.string(&id)
            }
            None => V::NULL,
        },
        (ACCESS, "getDimensionPath") => match world_folder(rt) {
            Some(w) => {
                let dimension = key_dimension(rt, a(1));
                crate::files::new_path(rt, &storage_folder(&w, &dimension))
            }
            None => V::NULL,
        },
        // `LevelStorageSource.baseDir`: the folder the worlds are in.
        (ACCESS, _) => match world_folder(rt) {
            Some(w) => {
                let saves = Path::new(&w)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                crate::files::new_path(rt, &saves)
            }
            None => V::NULL,
        },
        // `DimensionType.getStorageFolder(dimension, levelFolder)`.
        (DIMENSION_TYPE, _) => {
            let dimension = key_dimension(rt, a(0));
            let folder = rt.str_of(a(1)).unwrap_or("").to_owned();
            crate::files::new_path(rt, &storage_folder(&folder, &dimension))
        }
        (SAVED_DATA, "<init>") => V::Void,
        (SAVED_DATA, "setDirty") => {
            let dirty = if desc == "()V" { 1 } else { a(1).i() };
            rt.put_field(a(0), "$dirty", V::I(dirty));
            V::Void
        }
        (SAVED_DATA, "isDirty") => V::I(rt.get_field(a(0), "$dirty").i()),
        (SAVED_DATA, _) if desc.starts_with("(Ljava/io/File;") => {
            save_to_file(rt, a(0), a(1), a(2));
            V::Void
        }
        // `save(tag, provider)` of a saved data whose class has no `save` of its own: the tag as
        // given (`SavedData`'s is abstract).
        (SAVED_DATA, _) => a(1),
        (FACTORY, _) => {
            crate::api::call(rt, FACTORY_INIT, &[a(0), a(1), a(2), V::NULL]);
            V::Void
        }
        (STORAGE, "computeIfAbsent") => compute_if_absent(rt, a(0), a(1), a(2)),
        (STORAGE, "get") => get(rt, a(0), a(1), a(2)),
        (STORAGE, "set") => {
            let cache = rt.get_field(a(0), "$cache");
            crate::jutil::table_put(rt, cache, a(1), a(2));
            V::Void
        }
        (STORAGE, _) => {
            save_storage(rt, a(0));
            V::Void
        }
        // A level's chunk cache is its dimension's.
        (CHUNK_CACHE, _) => {
            let dimension = match rt.str_of(rt.get_field(a(0), "$dimension")) {
                Some(d) => d.to_owned(),
                None => crate::glue::level_dimension(rt, V::NULL),
            };
            storage(rt, &dimension)
        }
        _ => {
            let dimension = crate::glue::level_dimension(rt, a(0));
            storage(rt, &dimension)
        }
    })
}

/// `LevelResource`'s constants: one object each.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != LEVEL_RESOURCE {
        return None;
    }
    let &(_, id) = RESOURCES.iter().find(|(f, _)| *f == field)?;
    Some(rt.intern(format!("compat:level-resource/{id}"), |rt| {
        let r = rt.new_object(LEVEL_RESOURCE);
        let s = rt.string(id);
        rt.put_field(r, "id", s);
        r
    }))
}

/// The open world's folder: the one the hook's host gives (kept), else the one a host last gave
/// in this runtime; None in a client's runtime, or before any host told it.
pub(crate) fn world_folder(rt: &mut Rt) -> Option<String> {
    if crate::neo::is_client(rt) {
        return None;
    }
    if let Some(dir) = rt.host().and_then(|h| h.world_dir()) {
        let text = dir.to_string_lossy().into_owned();
        if rt.interned.get(WORLD).and_then(|&v| rt.str_of(v)) != Some(text.as_str()) {
            let v = rt.string(&text);
            rt.interned.insert(WORLD.into(), v);
        }
        return Some(text);
    }
    rt.interned
        .get(WORLD)
        .and_then(|&v| rt.str_of(v))
        .map(str::to_owned)
}

fn resource_id(rt: &Rt, resource: V) -> String {
    rt.str_of(rt.get_field(resource, "id"))
        .unwrap_or("")
        .to_owned()
}

/// `MinecraftServer.getWorldPath(resource)`: the world's folder resolved with the resource's id
/// (`.` for `ROOT`, kept as Java's `resolve` keeps it), where the game keeps that; null while no
/// world folder is known.
fn world_path(rt: &mut Rt, resource: V) -> V {
    let Some(world) = world_folder(rt) else {
        return V::NULL;
    };
    let id = resource_id(rt, resource);
    let id = MOVED
        .iter()
        .find(|(old, _)| *old == id)
        .map_or(id.as_str(), |(_, new)| new);
    crate::files::new_path(rt, &crate::files::resolve(&world, id))
}

/// The dimension of a level's key (`dimension#minecraft:the_nether`).
fn key_dimension(rt: &Rt, key: V) -> String {
    let text = rt.str_of(key).unwrap_or(OVERWORLD);
    text.rsplit_once('#').map_or(text, |(_, id)| id).to_owned()
}

/// `DimensionType.getStorageFolder`: a dimension's folder in world folder `world`, the world's own
/// for the Overworld, `DIM-1` for the Nether, `DIM1` for the End, `dimensions/<namespace>/<path>`
/// for any other.
fn storage_folder(world: &str, dimension: &str) -> String {
    use crate::files::resolve;
    match dimension {
        OVERWORLD => world.to_owned(),
        "minecraft:the_nether" => resolve(world, "DIM-1"),
        "minecraft:the_end" => resolve(world, "DIM1"),
        d => {
            let (namespace, path) = d.split_once(':').unwrap_or(("minecraft", d));
            resolve(&resolve(&resolve(world, "dimensions"), namespace), path)
        }
    }
}

/// The registries a saved data reads and writes with (`HolderLookup.Provider`).
fn registries(rt: &mut Rt) -> V {
    rt.intern("compat:registry-access".into(), |rt| {
        rt.new_object("net/minecraft/core/RegistryAccess")
    })
}

/// The saved data of `dimension`'s level (`ServerLevel.getDataStorage()`): one storage a
/// dimension, its data by name (`$cache`), and the names it couldn't read (`$unread`).
fn storage(rt: &mut Rt, dimension: &str) -> V {
    rt.intern(format!("compat:data-storage/{dimension}"), |rt| {
        let s = rt.new_object(STORAGE);
        let d = rt.string(dimension);
        rt.put_field(s, "$dimension", d);
        for field in ["$cache", "$unread"] {
            let map = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
            rt.put_field(s, field, map);
        }
        s
    })
}

/// The folder a storage's files are in: its dimension's `data/`, in the world's folder.
fn data_folder(rt: &mut Rt, storage: V) -> Option<PathBuf> {
    let dimension = rt.str_of(rt.get_field(storage, "$dimension"))?.to_owned();
    let world = world_folder(rt)?;
    let folder = crate::files::resolve(&storage_folder(&world, &dimension), "data");
    Some(crate::files::on_disk(rt, &folder))
}

/// `DimensionDataStorage.get(factory, name)`: the saved data of that name, read from its file the
/// first time; null when there is none. What it found is kept, null too, as its cache keeps it.
fn get(rt: &mut Rt, storage: V, factory: V, name: V) -> V {
    let cache = rt.get_field(storage, "$cache");
    if let Some(d) = crate::jutil::table_get(rt, cache, name) {
        return d;
    }
    let text = rt.str_of(name).unwrap_or("").to_owned();
    let d = match read(rt, storage, factory, &text) {
        Ok(d) => d,
        Err(why) => {
            // Not read: never written over, so the player's file stays as it was.
            let unread = rt.get_field(storage, "$unread");
            crate::jutil::table_put(rt, unread, name, V::I(1));
            eprintln!("horizon-compat: saved data {text} not read ({why}); it won't be saved over");
            V::NULL
        }
    };
    crate::jutil::table_put(rt, cache, name, d);
    d
}

/// Saved data `name` read back from its file (the file's `data`, as `tag.getCompound("data")`
/// gives it, to the factory's deserializer): null when the world has no such file; an error when
/// it can't be read (no world folder known, a file that isn't NBT, a deserializer that threw).
fn read(rt: &mut Rt, storage: V, factory: V, name: &str) -> Result<V, String> {
    // A client has no world: its saved data is its own, in memory.
    if crate::neo::is_client(rt) {
        return Ok(V::NULL);
    }
    let folder = data_folder(rt, storage).ok_or("no world folder")?;
    let bytes = match std::fs::read(folder.join(format!("{name}.dat"))) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(V::NULL),
        Err(e) => return Err(e.to_string()),
    };
    // `DimensionDataStorage.isGzip`: a gzip header, else plain NBT.
    let root = if bytes.starts_with(&[0x1f, 0x8b]) {
        crate::streams::compressed_root(&bytes).map_err(|(_, m)| m)?
    } else {
        crate::streams::root_compound(&bytes).map_err(str::to_owned)?
    };
    let data = match root.get("data") {
        Some(Nbt::Compound(c)) => c.clone(),
        _ => BTreeMap::new(),
    };
    let tag = crate::nbt::to_runtime(rt, &data);
    // The factory record's components are fields by their names (`crate::records`).
    let deserializer = rt.get_field(factory, "deserializer");
    let provider = registries(rt);
    let d = rt.call_functional(APPLY, deserializer, &[tag, provider]);
    match thrown(rt) {
        Some(e) => {
            rt.report_thrown(&format!("reading saved data {name}"));
            Err(e)
        }
        None => Ok(d),
    }
}

/// The exception pending, as its class and message.
fn thrown(rt: &Rt) -> Option<String> {
    let e = rt.thrown?;
    let message = rt
        .str_of(rt.get_field(e, "message"))
        .unwrap_or("")
        .to_owned();
    Some(format!("{}: {message}", rt.class_of(e)))
}

/// `DimensionDataStorage.computeIfAbsent(factory, name)`: the saved data of that name, read, or
/// else made by the factory's constructor and kept.
fn compute_if_absent(rt: &mut Rt, storage: V, factory: V, name: V) -> V {
    let d = get(rt, storage, factory, name);
    if d.r() != 0 {
        return d;
    }
    let constructor = rt.get_field(factory, "constructor");
    let made = rt.call_functional(SUPPLY, constructor, &[]);
    if rt.thrown.is_some() {
        return V::NULL;
    }
    let cache = rt.get_field(storage, "$cache");
    crate::jutil::table_put(rt, cache, name, made);
    made
}

/// `DimensionDataStorage.save()`: each saved data to its file (`SavedData.save(File, Provider)`,
/// which writes it when it is dirty), but those it couldn't read.
fn save_storage(rt: &mut Rt, storage: V) {
    let Some(folder) = data_folder(rt, storage) else {
        return;
    };
    let (cache, unread) = (
        rt.get_field(storage, "$cache"),
        rt.get_field(storage, "$unread"),
    );
    let provider = registries(rt);
    for (name, data) in crate::jutil::table_pairs(rt, cache) {
        if data.r() == 0 || crate::jutil::table_get(rt, unread, name).is_some() {
            continue;
        }
        let text = rt.str_of(name).unwrap_or("").to_owned();
        let file =
            crate::files::new_file(rt, &folder.join(format!("{text}.dat")).to_string_lossy());
        crate::api::virtual_call(rt, SAVE_FILE, &[data, file, provider]);
        if let Some(e) = thrown(rt) {
            eprintln!("horizon-compat: saved data {text} not saved: {e}");
            rt.report_thrown(&format!("saving saved data {text}"));
        }
    }
}

/// `SavedData.save(File, Provider)`: when dirty, `{data: save(new CompoundTag(), provider),
/// DataVersion}` written to the file, which is then clean.
fn save_to_file(rt: &mut Rt, data: V, file: V, provider: V) {
    if crate::api::virtual_call(rt, IS_DIRTY, &[data]).i() == 0 || rt.thrown.is_some() {
        return;
    }
    let tag = crate::nbt::to_runtime(rt, &BTreeMap::new());
    let saved = crate::api::virtual_call(rt, SAVE_TAG, &[data, tag, provider]);
    if rt.thrown.is_some() {
        return;
    }
    let root = BTreeMap::from([
        (
            "data".to_owned(),
            Nbt::Compound(crate::nbt::from_runtime(rt, saved)),
        ),
        ("DataVersion".to_owned(), Nbt::Int(DATA_VERSION)),
    ]);
    let text = rt.str_of(file).unwrap_or("").to_owned();
    if let Err(e) = write_compressed(&crate::files::on_disk(rt, &text), &Nbt::Compound(root)) {
        // As `SavedData.save` logs a failed write.
        eprintln!("horizon-compat: Could not save data {text}: {e}");
    }
    crate::api::virtual_call(rt, SET_DIRTY, &[data, V::I(0)]);
}

/// `IOUtilities.writeNbtCompressed`: the compound gzipped, into a file beside `path` then moved over
/// it, so a crash never leaves half a file.
fn write_compressed(path: &Path, root: &Nbt) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut raw = Vec::new();
    crate::nbt::write_unnamed(&mut raw, root);
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, crate::streams::gzip(&raw))?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The game saved `dimension`'s level (`ServerLevel.save`): its saved data first, as
/// `saveLevelData` saves its storage, then the mods hear `LevelEvent.Save`.
pub(crate) fn level_saved(rt: &mut Rt, dimension: &str) {
    world_folder(rt);
    if let Some(&s) = rt.interned.get(&format!("compat:data-storage/{dimension}")) {
        save_storage(rt, s);
    }
    for (class, up) in [(LEVEL_SAVE, LEVEL_EVENT), (LEVEL_EVENT, EVENT)] {
        rt.supers.entry(class).or_insert(up);
    }
    if crate::neo::hearing(rt, LEVEL_SAVE).is_empty() {
        return;
    }
    let level = crate::glue::server_level(rt, dimension);
    let ev = rt.new_object(LEVEL_SAVE);
    rt.put_field(ev, "$level", level);
    crate::neo::post(rt, LEVEL_SAVE, ev);
}

/// The server stopped: its world's folder and saved data go with it; the next world's are its own.
pub(crate) fn forget(rt: &mut Rt) {
    rt.interned.retain(|k, _| {
        !(k.starts_with("compat:data-storage/") || k == WORLD || k == "compat:level-storage")
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use std::io::{Read, Write};

    /// A hook's host: a server's level in `dimension`, of the world in `world` (none: a host that
    /// can't tell, as a block's tick).
    struct In {
        dimension: &'static str,
        world: Option<PathBuf>,
    }
    impl Host for In {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn dimension(&self) -> String {
            self.dimension.into()
        }
        fn world_dir(&self) -> Option<PathBuf> {
            self.world.clone()
        }
    }

    const TRACKS: &str = "test/Tracks";
    const LEVEL_LOAD: &str = "net/neoforged/neoforge/event/level/LevelEvent$Load";
    const OVERWORLD_OF: &str =
        "net/minecraft/server/MinecraftServer.overworld.()Lnet/minecraft/server/level/ServerLevel;";
    const GET_LEVEL: &str = "net/minecraft/server/MinecraftServer.getLevel.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/server/level/ServerLevel;";
    const ALL_LEVELS: &str =
        "net/minecraft/server/MinecraftServer.getAllLevels.()Ljava/lang/Iterable;";
    const WORLD_PATH: &str = "net/minecraft/server/MinecraftServer.getWorldPath.(Lnet/minecraft/world/level/storage/LevelResource;)Ljava/nio/file/Path;";
    const DIMENSION: &str =
        "net/minecraft/world/level/Level.dimension.()Lnet/minecraft/resources/ResourceKey;";
    const EVENT_LEVEL: &str = "net/neoforged/neoforge/event/level/LevelEvent.getLevel.()Lnet/minecraft/world/level/LevelAccessor;";
    const DATA_STORAGE: &str = "net/minecraft/server/level/ServerLevel.getDataStorage.()Lnet/minecraft/world/level/storage/DimensionDataStorage;";
    const COMPUTE: &str = "net/minecraft/world/level/storage/DimensionDataStorage.computeIfAbsent.(Lnet/minecraft/world/level/saveddata/SavedData$Factory;Ljava/lang/String;)Lnet/minecraft/world/level/saveddata/SavedData;";
    const GET: &str = "net/minecraft/world/level/storage/DimensionDataStorage.get.(Lnet/minecraft/world/level/saveddata/SavedData$Factory;Ljava/lang/String;)Lnet/minecraft/world/level/saveddata/SavedData;";
    /// A mod marking its data, as `javac` writes the call: on its own class.
    const MARK: &str = "test/Tracks.setDirty.()V";
    const IS_DIRTY_KEY: &str = "net/minecraft/world/level/saveddata/SavedData.isDirty.()Z";
    const NEW_FACTORY: &str = "net/minecraft/world/level/saveddata/SavedData$Factory.<init>.(Ljava/util/function/Supplier;Ljava/util/function/BiFunction;)V";
    const GET_INT: &str = "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I";
    const PUT_INT: &str = "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V";

    /// A mod's saved data, its count of trains kept under `Trains`.
    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) == TRACKS && short == SAVE_TAG.1 {
            let (k, n) = (rt.string("Trains"), rt.get_field(args[0], "trains"));
            crate::api::call(rt, PUT_INT, &[args[1], k, V::I(n.i())]);
            return args[1];
        }
        crate::api::call(rt, key, args)
    }

    /// The factory's constructor (1) and deserializer (2, which throws on data marked `Bad`), and
    /// a listener keeping what it hears (3).
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        match id {
            1 => Some(tracks(rt, 0)),
            2 => {
                let bad = rt.string("Bad");
                if crate::api::call(rt, GET_INT, &[args[1], bad]).i() != 0 {
                    rt.throw_new("java/lang/IllegalStateException");
                    return Some(V::NULL);
                }
                let k = rt.string("Trains");
                let n = crate::api::call(rt, GET_INT, &[args[1], k]).i();
                Some(tracks(rt, n))
            }
            3 => {
                let seen = rt.captured(args[0])[0];
                crate::jutil::list_add(rt, seen, args[1]);
                Some(V::Void)
            }
            _ => None,
        }
    }

    fn tracks(rt: &mut Rt, n: i32) -> V {
        let t = rt.new_object(TRACKS);
        rt.put_field(t, "trains", V::I(n));
        t
    }

    /// A server's runtime with the mod's factory (`new SavedData.Factory(Tracks::new, Tracks::load)`).
    fn runtime() -> (Rt, V) {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        rt.vcall = Some(vcall);
        rt.supers.insert(TRACKS, SAVED_DATA);
        let make = rt.lambda_id("java/util/function/Supplier", 1, Vec::new());
        let read = rt.lambda_id("java/util/function/BiFunction", 2, Vec::new());
        let factory = rt.new_object(FACTORY);
        crate::api::call(&mut rt, NEW_FACTORY, &[factory, make, read]);
        (rt, factory)
    }

    /// A world folder of its own for one test (`saves/<name>`).
    fn world(test: &str, name: &str) -> PathBuf {
        let saves = std::env::temp_dir()
            .join(format!(
                "horizon-compat-world-{test}-{}",
                std::process::id()
            ))
            .join("saves");
        let dir = saves.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        dir
    }

    /// `{data: {<key>: n}, DataVersion: 3955}` as NBT lays out an unnamed root compound, gzipped:
    /// what 1.21.1 writes for a saved data.
    fn data_file(key: &str, n: i32) -> Vec<u8> {
        let mut raw = vec![10, 0, 0, 10, 0, 4];
        raw.extend(b"data");
        raw.extend([3, 0, key.len() as u8]);
        raw.extend(key.as_bytes());
        raw.extend(n.to_be_bytes());
        raw.extend([0, 3, 0, 11]);
        raw.extend(b"DataVersion");
        raw.extend(3955i32.to_be_bytes());
        raw.push(0);
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&raw).unwrap();
        e.finish().unwrap()
    }

    fn unpacked(path: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    fn int_entry(name: &str, n: i32) -> Vec<u8> {
        let mut e = vec![3, 0, name.len() as u8];
        e.extend(name.as_bytes());
        e.extend(n.to_be_bytes());
        e
    }

    fn holds(raw: &[u8], part: &[u8]) -> bool {
        raw.windows(part.len()).any(|w| w == part)
    }

    /// The level's saved data of name `name` (`level.getDataStorage().computeIfAbsent`).
    fn compute(rt: &mut Rt, level: V, factory: V, name: &str) -> V {
        let storage = crate::api::call(rt, DATA_STORAGE, &[level]);
        let name = rt.string(name);
        crate::api::call(rt, COMPUTE, &[storage, factory, name])
    }

    /// Create keeps its railways as the Overworld's saved data (`create_tracks`): read from the
    /// world's `data/` the first time it asks, one object a name; what it changed and marked dirty
    /// is written back when the game saves the level, as 1.21.1 writes it, and is what the next
    /// server reads; data not marked isn't written. A server stopped forgets its world: the next
    /// world's data is its own.
    #[test]
    fn a_levels_saved_data_is_read_from_the_world_and_written_back_when_it_saves() {
        let dir = world("saved", "Railways");
        std::fs::write(dir.join("data/create_tracks.dat"), data_file("Trains", 3)).unwrap();
        let mut here = In {
            dimension: OVERWORLD,
            world: Some(dir.clone()),
        };
        let (mut rt, factory) = runtime();
        let tracks = rt.with_host(&mut here, |rt| {
            let server = crate::neo::server(rt);
            let level = crate::api::call(rt, OVERWORLD_OF, &[server]);
            let tracks = compute(rt, level, factory, "create_tracks");
            assert_eq!(
                rt.get_field(tracks, "trains"),
                V::I(3),
                "read from the world's file"
            );
            assert_eq!(compute(rt, level, factory, "create_tracks"), tracks);
            let storage = crate::api::call(rt, DATA_STORAGE, &[level]);
            let other = rt.string("create_logistics");
            assert_eq!(
                crate::api::call(rt, GET, &[storage, factory, other]),
                V::NULL,
                "no file, no data"
            );
            let made = compute(rt, level, factory, "create_logistics");
            assert_eq!(
                rt.get_field(made, "trains"),
                V::I(0),
                "made by the constructor"
            );
            assert_eq!(compute(rt, level, factory, "create_logistics"), made);
            rt.put_field(tracks, "trains", V::I(5));
            crate::api::call(rt, MARK, &[tracks]);
            tracks
        });
        crate::glue::level_save(&mut rt, &mut here, OVERWORLD);
        assert_eq!(
            rt.with_host(&mut here, |rt| crate::api::call(
                rt,
                IS_DIRTY_KEY,
                &[tracks]
            )),
            V::I(0),
            "clean once saved"
        );
        let raw = unpacked(&dir.join("data/create_tracks.dat"));
        assert!(
            raw.starts_with(&[10, 0, 0])
                && holds(&raw, &int_entry("Trains", 5))
                && holds(&raw, &int_entry("DataVersion", 3955))
        );
        assert!(
            !dir.join("data/create_logistics.dat").exists(),
            "never marked dirty"
        );
        assert!(
            std::fs::read_dir(dir.join("data")).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );

        // The next server, in a runtime of its own, reads what this one saved.
        let (mut next, next_factory) = runtime();
        next.with_host(&mut here, |rt| {
            let level = crate::glue::level(rt);
            let tracks = compute(rt, level, next_factory, "create_tracks");
            assert_eq!(rt.get_field(tracks, "trains"), V::I(5));
        });
        // The same runtime's next world: its own file, not what the last one kept.
        crate::neo::server_stopped(&mut rt);
        let other = world("saved", "Another");
        std::fs::write(other.join("data/create_tracks.dat"), data_file("Trains", 7)).unwrap();
        let mut there = In {
            dimension: OVERWORLD,
            world: Some(other),
        };
        rt.with_host(&mut there, |rt| {
            let level = crate::glue::level(rt);
            let tracks = compute(rt, level, factory, "create_tracks");
            assert_eq!(rt.get_field(tracks, "trains"), V::I(7));
        });
        assert!(
            rt.unmapped.is_empty() && next.unmapped.is_empty(),
            "{:?} {:?}",
            rt.unmapped,
            next.unmapped
        );
    }

    /// What a mod couldn't read back is never written over, though the mod marks what it got
    /// instead: a file that isn't NBT, one whose data the mod's deserializer throws on (the
    /// exception goes no further), and any asked for while no world folder is known (a block's
    /// tick before the server told it). The player's files stay as they were.
    #[test]
    fn data_a_mod_couldnt_read_is_never_written_over() {
        let dir = world("unread", "Kept");
        let files = [
            ("garbled", b"not nbt".to_vec()),
            ("thrown", data_file("Bad", 1)),
            ("early", data_file("Trains", 4)),
        ];
        for (name, bytes) in &files {
            std::fs::write(dir.join(format!("data/{name}.dat")), bytes).unwrap();
        }
        let (mut rt, factory) = runtime();
        let mut blind = In {
            dimension: OVERWORLD,
            world: None,
        };
        rt.with_host(&mut blind, |rt| {
            let level = crate::glue::level(rt);
            let early = compute(rt, level, factory, "early");
            assert_eq!(
                rt.get_field(early, "trains"),
                V::I(0),
                "not read: no world folder yet"
            );
            crate::api::call(rt, MARK, &[early]);
        });
        let mut here = In {
            dimension: OVERWORLD,
            world: Some(dir.clone()),
        };
        rt.with_host(&mut here, |rt| {
            let level = crate::glue::level(rt);
            for name in ["garbled", "thrown"] {
                let d = compute(rt, level, factory, name);
                assert_eq!(rt.get_field(d, "trains"), V::I(0), "{name}: made anew");
                assert!(rt.thrown.is_none(), "{name}: the mod goes on");
                crate::api::call(rt, MARK, &[d]);
            }
        });
        crate::glue::level_save(&mut rt, &mut here, OVERWORLD);
        for (name, bytes) in &files {
            assert_eq!(
                &std::fs::read(dir.join(format!("data/{name}.dat"))).unwrap(),
                bytes,
                "{name} kept"
            );
        }
    }

    /// The server has a level a dimension: the hook's own, after the Overworld (which loads
    /// first, as vanilla's does), each its own `dimension()` and saved data, kept in its 1.21.1
    /// folder (the Nether's in `DIM-1/data`). `getLevel` finds vanilla's three and no other,
    /// `getAllLevels` lists those loaded; the level saved, mods hear `LevelEvent.Save` about it.
    #[test]
    fn each_dimension_is_a_level_of_its_own_with_its_own_saved_data() {
        let dir = world("dimensions", "Nether");
        let (mut rt, factory) = runtime();
        let seen = crate::jutil::new_list(&mut rt, Vec::new());
        for event in [LEVEL_LOAD, LEVEL_SAVE] {
            let l = rt.lambda_id("java/util/function/Consumer", 3, vec![seen]);
            rt.listeners.push((event.to_owned(), l));
        }
        let mut nether = In {
            dimension: "minecraft:the_nether",
            world: Some(dir.clone()),
        };
        let (level, overworld) = rt.with_host(&mut nether, |rt| {
            let level = crate::glue::level(rt);
            let server = crate::neo::server(rt);
            let overworld = crate::api::call(rt, OVERWORLD_OF, &[server]);
            assert_ne!(level, overworld);
            for (l, id) in [
                (level, "dimension#minecraft:the_nether"),
                (overworld, "dimension#minecraft:overworld"),
            ] {
                let d = crate::api::call(rt, DIMENSION, &[l]);
                assert_eq!(rt.str_of(d), Some(id));
            }
            let key = crate::api::get_static(rt, "net/minecraft/world/level/Level.NETHER");
            assert_eq!(crate::api::call(rt, GET_LEVEL, &[server, key]), level);
            let moon = rt.string("dimension#mymod:moon");
            assert_eq!(
                crate::api::call(rt, GET_LEVEL, &[server, moon]),
                V::NULL,
                "no such level"
            );
            let all = crate::api::call(rt, ALL_LEVELS, &[server]);
            assert_eq!(crate::jutil::elements(rt, all), [overworld, level]);
            let pocket = compute(rt, level, factory, "pocket");
            crate::api::call(rt, MARK, &[pocket]);
            let storage = crate::api::call(rt, DATA_STORAGE, &[overworld]);
            let name = rt.string("pocket");
            assert_eq!(
                crate::api::call(rt, GET, &[storage, factory, name]),
                V::NULL,
                "the Overworld's own"
            );
            (level, overworld)
        });
        crate::glue::level_save(&mut rt, &mut nether, "minecraft:the_nether");
        assert!(dir.join("DIM-1/data/pocket.dat").is_file());
        assert!(!dir.join("data/pocket.dat").exists());
        let heard: Vec<(String, V)> = crate::jutil::elements(&rt, seen)
            .into_iter()
            .map(|e| {
                (
                    rt.class_of(e).to_owned(),
                    crate::api::call(&mut rt, EVENT_LEVEL, &[e]),
                )
            })
            .collect();
        let expected = [
            (LEVEL_LOAD, overworld),
            (LEVEL_LOAD, level),
            (LEVEL_SAVE, level),
        ]
        .map(|(c, l)| (c.to_owned(), l));
        assert_eq!(heard, expected);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod finds the world's files by 1.21.1's ids where the game keeps them (the players' data
    /// under `players/`), its own beside them (`new LevelResource("serverconfig")`), and the
    /// world's folders as 1.21.1 names them (`LevelStorageAccess`, `DimensionType.getStorageFolder`);
    /// a client's runtime has no world.
    #[test]
    fn a_mod_finds_the_worlds_files_where_the_game_keeps_them() {
        let dir = world("paths", "New World");
        let w = dir.to_string_lossy().into_owned();
        let mut here = In {
            dimension: OVERWORLD,
            world: Some(dir.clone()),
        };
        let mut rt = Rt::new();
        let text = |rt: &Rt, v: V| rt.str_of(v).map(str::to_owned);
        let resource = |rt: &mut Rt, field: &str| {
            let key = rt.static_key(&format!("{LEVEL_RESOURCE}.{field}"));
            crate::api::get_static(rt, key)
        };
        rt.with_host(&mut here, |rt| {
            let server = crate::neo::server(rt);
            for (field, at) in [("ROOT", "."), ("PLAYER_DATA_DIR", "players/data"), ("PLAYER_ADVANCEMENTS_DIR", "players/advancements"), ("DATAPACK_DIR", "datapacks")] {
                let r = resource(rt, field);
                let p = crate::api::call(rt, WORLD_PATH, &[server, r]);
                assert_eq!(text(rt, p), Some(format!("{w}/{at}")), "{field}");
            }
            assert_eq!(resource(rt, "ROOT"), resource(rt, "ROOT"));
            let own = rt.new_object(LEVEL_RESOURCE);
            let id = rt.string("serverconfig");
            crate::api::call(rt, "net/minecraft/world/level/storage/LevelResource.<init>.(Ljava/lang/String;)V", &[own, id]);
            let p = crate::api::call(rt, WORLD_PATH, &[server, own]);
            assert_eq!(text(rt, p), Some(format!("{w}/serverconfig")));
            let got = crate::api::call(rt, "net/minecraft/world/level/storage/LevelResource.getId.()Ljava/lang/String;", &[own]);
            assert_eq!(text(rt, got).as_deref(), Some("serverconfig"));
            let shown = crate::api::call(rt, "net/minecraft/world/level/storage/LevelResource.toString.()Ljava/lang/String;", &[own]);
            assert_eq!(text(rt, shown).as_deref(), Some("/serverconfig"));

            // The server's `storageSource`, as an access transformer opens it.
            let access = crate::api::game_field(
                rt,
                server,
                "storageSource",
                "$storageSource",
                "net/minecraft/server/MinecraftServer.$storageSource.()Lnet/minecraft/world/level/storage/LevelStorageSource$LevelStorageAccess;",
            );
            let id = crate::api::call(rt, "net/minecraft/world/level/storage/LevelStorageSource$LevelStorageAccess.getLevelId.()Ljava/lang/String;", &[access]);
            assert_eq!(text(rt, id).as_deref(), Some("New World"));
            let saves = crate::api::call(rt, "net/minecraft/world/level/storage/LevelStorageSource$LevelStorageAccess.getWorldDir.()Ljava/nio/file/Path;", &[access]);
            assert_eq!(text(rt, saves), Some(dir.parent().unwrap().to_string_lossy().into_owned()));
            for (key, at) in [("NETHER", "DIM-1"), ("END", "DIM1")] {
                let k = rt.static_key(&format!("net/minecraft/world/level/Level.{key}"));
                let k = crate::api::get_static(rt, k);
                let p = crate::api::call(rt, "net/minecraft/world/level/storage/LevelStorageSource$LevelStorageAccess.getDimensionPath.(Lnet/minecraft/resources/ResourceKey;)Ljava/nio/file/Path;", &[access, k]);
                assert_eq!(text(rt, p), Some(format!("{w}/{at}")));
            }
            let (moon, folder) = (rt.string("dimension#mymod:moon"), crate::files::new_path(rt, "/w"));
            let p = crate::api::call(rt, "net/minecraft/world/level/dimension/DimensionType.getStorageFolder.(Lnet/minecraft/resources/ResourceKey;Ljava/nio/file/Path;)Ljava/nio/file/Path;", &[moon, folder]);
            assert_eq!(text(rt, p).as_deref(), Some("/w/dimensions/mymod/moon"));
            let p = crate::api::call(rt, "net/minecraft/server/MinecraftServer.getServerDirectory.()Ljava/nio/file/Path;", &[server]);
            assert_eq!(text(rt, p).as_deref(), Some(""));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);

        let mut client = Rt::new();
        client.interned.insert("compat:client".into(), V::I(1));
        client.with_host(&mut here, |rt| {
            let server = crate::neo::server(rt);
            let root = resource(rt, "ROOT");
            assert_eq!(
                crate::api::call(rt, WORLD_PATH, &[server, root]),
                V::NULL,
                "a client has no world"
            );
        });
    }
}
