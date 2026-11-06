//! A mod's datapack registries (NeoForge 1.21.1's `DataPackRegistryEvent.NewRegistry`): registries
//! whose entries are data files, as a world's loading reads them (`RegistryDataLoader`). Each file
//! `data/<namespace>/<folder>/<path>.json` of the game's data, where the folder is the registry's
//! (`Registries.elementsDirPath`: `<registry namespace>/<registry path>`, the path alone for
//! `minecraft`'s), is entry `<namespace>:<path>`, decoded with the codec the mod gave
//! (`ConditionalOps`: a file whose `neoforge:conditions` don't hold is left out), and registered in
//! the order `ResourceLocation`s sort the files in: by path, then namespace. Create's potato cannon
//! finds the type of its ammo in one (`create:potato_projectile/type`, which its addons add to),
//! and 14 more of the pack's mods make one.
//!
//! The entries are read once, the first time the registry is reached with the game's resources at
//! hand (a hook's host): registries are their names here, and the entries land in
//! [`Rt::registered`] under that name, so `lookupOrThrow(key).listElements()`, `get`, `getHolder`
//! and holders' values find them as they find a mod's own registrations. A file that doesn't decode
//! is left out (vanilla's loading fails the world instead), told when tracing. Not modelled: the
//! registries' tags (`data/<namespace>/tags/<folder>/`), their network codecs (a client reads the
//! same files), the `RegistryBuilder` a mod may configure for one.
use crate::{Rt, V};
use serde_json::Value as Json;

/// Set once a mod made a datapack registry: the registry lookups skip all of this without one.
const ANY: &str = "compat:datapack";
/// The codec of each datapack registry, by its name (`compat:datapack/<registry>`).
const CODEC: &str = "compat:datapack/";
/// Set once a registry's entries were read.
const READ: &str = "compat:datapack-read/";
/// The namespace of a registry made by key (`ResourceKey.createRegistryKey`), whose name here is its
/// path.
const NAMESPACE: &str = "compat:registry-namespace/";
/// The key NeoForge reads a data file's load conditions from.
const CONDITIONS: &str = "neoforge:conditions";

/// Keeps the namespace of a registry made by key, `location` (`create:potato_projectile/type`):
/// the registry's files are in a folder of that name.
pub(crate) fn key_made(rt: &mut Rt, location: &str) {
    if let Some((ns, path)) = location.split_once(':') {
        let ns = rt.string(ns);
        rt.interned.insert(format!("{NAMESPACE}{path}"), ns);
    }
}

/// `dataPackRegistry(key, codec, ...)`: `registry`'s entries are data files, read with `codec`.
pub(crate) fn add(rt: &mut Rt, registry: &str, codec: V) {
    rt.interned.insert(ANY.into(), V::I(1));
    rt.interned.insert(format!("{CODEC}{registry}"), codec);
}

/// Whether `registry` is a datapack registry a mod made.
pub(crate) fn is_datapack(rt: &Rt, registry: &str) -> bool {
    rt.interned.contains_key(ANY) && rt.interned.contains_key(&format!("{CODEC}{registry}"))
}

/// The folder of `registry`'s files below a data namespace (`create/potato_projectile/type`).
fn folder(rt: &Rt, registry: &str) -> String {
    match rt.interned.get(&format!("{NAMESPACE}{registry}")).and_then(|ns| rt.str_of(*ns)) {
        Some(ns) if ns != "minecraft" => format!("{ns}/{registry}"),
        _ => registry.to_owned(),
    }
}

/// Reads datapack registry `registry`'s entries, once, when there are resources to read them from:
/// see the module's doc.
pub(crate) fn read(rt: &mut Rt, registry: &str) {
    if !rt.interned.contains_key(ANY) {
        return;
    }
    let Some(&codec) = rt.interned.get(&format!("{CODEC}{registry}")) else { return };
    let done = format!("{READ}{registry}");
    if rt.interned.contains_key(&done) || rt.host().is_none() {
        return;
    }
    rt.interned.insert(done, V::I(1));
    let folder = folder(rt, registry);
    let namespaces = rt.host().map(|h| h.resource_namespaces("data")).unwrap_or_default();
    // (the file below the folder, its namespace, its path).
    let mut files: Vec<(String, String, String)> = Vec::new();
    for ns in &namespaces {
        let under = format!("data/{ns}/{folder}/");
        let paths = rt.host().map(|h| h.resource_paths(&under[..under.len() - 1])).unwrap_or_default();
        for path in paths {
            if let Some(file) = path.strip_prefix(&under).filter(|f| f.ends_with(".json")) {
                files.push((file.to_owned(), ns.clone(), path.clone()));
            }
        }
    }
    files.sort();
    for (file, ns, path) in files {
        let id = format!("{ns}:{}", &file[..file.len() - ".json".len()]);
        let Some(bytes) = rt.host().and_then(|h| h.resource(&path)) else { continue };
        let mut json = match serde_json::from_slice::<Json>(&bytes) {
            Ok(j) => j,
            Err(e) => {
                left_out(registry, &id, &e.to_string());
                continue;
            }
        };
        if !json.get(CONDITIONS).is_none_or(|c| c.as_array().is_some_and(|all| all.iter().all(|c| holds(rt, c, &namespaces)))) {
            continue;
        }
        if let Some(o) = json.as_object_mut() {
            o.remove(CONDITIONS);
        }
        match crate::codecs::decode(rt, codec, &json) {
            Ok(v) => {
                rt.registered.insert((registry.to_owned(), id.clone()), v);
                crate::neo::registry_holder(rt, registry, &id, v);
            }
            Err(e) => left_out(registry, &id, &e),
        }
    }
}

fn left_out(registry: &str, id: &str, why: &str) {
    if Rt::tracing() {
        eprintln!("horizon-compat: {registry} entry {id} left out: {why}");
    }
}

/// The order `registry`'s entries were registered in: their files' (`ResourceLocation.compareTo`:
/// the path, `.json` and all, then the namespace).
pub(crate) fn registration_order(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |id: &str| -> (String, String) {
        match id.split_once(':') {
            Some((ns, path)) => (format!("{path}.json"), ns.to_owned()),
            None => (format!("{id}.json"), "minecraft".to_owned()),
        }
    };
    parts(a).cmp(&parts(b))
}

/// Whether load condition `c` holds (NeoForge's `ICondition`s a data file names most): a mod
/// loaded (one the runtime carries, or one whose data is in the game's: a mod that is off leaves
/// none), always, never, and their `not`, `and` and `or`. Any other kind, anywhere in the tree,
/// doesn't hold: NeoForge couldn't read the file either.
fn holds(rt: &Rt, c: &Json, namespaces: &[String]) -> bool {
    let values = || c.get("values").and_then(Json::as_array).into_iter().flatten();
    match c.get("type").and_then(Json::as_str) {
        Some("neoforge:true") => true,
        Some("neoforge:false") => false,
        Some("neoforge:mod_loaded") => c.get("modid").and_then(Json::as_str).is_some_and(|m| {
            matches!(m, "minecraft" | "neoforge") || crate::neo::mod_loaded(rt, m) || namespaces.iter().any(|n| n == m)
        }),
        Some("neoforge:not") => c.get("value").is_some_and(|v| known(v) && !holds(rt, v, namespaces)),
        Some("neoforge:and") => values().all(|v| holds(rt, v, namespaces)),
        Some("neoforge:or") => values().all(known) && values().any(|v| holds(rt, v, namespaces)),
        _ => false,
    }
}

/// Whether every condition in `c`'s tree is of a kind [`holds`] knows.
fn known(c: &Json) -> bool {
    match c.get("type").and_then(Json::as_str) {
        Some("neoforge:true" | "neoforge:false" | "neoforge:mod_loaded") => true,
        Some("neoforge:not") => c.get("value").is_some_and(known),
        Some("neoforge:and" | "neoforge:or") => c.get("values").and_then(Json::as_array).into_iter().flatten().all(known),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::host::MemoryHost;

    const CREATE_KEY: &str = "net/minecraft/resources/ResourceKey.createRegistryKey.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;";
    const NEW_REGISTRY: &str = "net/neoforged/neoforge/registries/DataPackRegistryEvent$NewRegistry";
    const DATAPACK_REGISTRY: &str = "net/neoforged/neoforge/registries/DataPackRegistryEvent$NewRegistry.dataPackRegistry.(Lnet/minecraft/resources/ResourceKey;Lcom/mojang/serialization/Codec;)V";
    const LOOKUP: &str = "net/minecraft/core/RegistryAccess.lookupOrThrow.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/HolderLookup$RegistryLookup;";
    const LIST: &str = "net/minecraft/core/HolderLookup$RegistryLookup.listElements.()Ljava/util/stream/Stream;";
    const NAME: &str = "net/minecraft/core/Holder$Reference.getRegisteredName.()Ljava/lang/String;";
    const VALUE: &str = "net/minecraft/core/Holder$Reference.value.()Ljava/lang/Object;";
    const CONTAINS: &str = "net/minecraft/core/HolderSet.contains.(Lnet/minecraft/core/Holder;)Z";
    const GET: &str = "net/minecraft/core/Registry.get.(Lnet/minecraft/resources/ResourceLocation;)Ljava/lang/Object;";

    fn file(host: &mut MemoryHost, path: &str, json: &str) {
        host.resources.insert(path.into(), json.as_bytes().to_vec());
    }

    fn listed(rt: &mut Rt, key: V) -> Vec<(String, V)> {
        let access = rt.new_object("net/minecraft/core/RegistryAccess");
        let lookup = api::call(rt, LOOKUP, &[access, key]);
        let stream = api::call(rt, LIST, &[lookup]);
        crate::jutil::elements(rt, stream)
            .into_iter()
            .map(|h| {
                let name = api::call(rt, NAME, &[h]);
                (rt.str_of(name).unwrap_or("").to_owned(), h)
            })
            .collect()
    }

    /// A mod's datapack registry `testmod:gadget/type` whose entries are sets of items (as a potato
    /// cannon's ammo types are): read from every data namespace's `testmod/gadget/type/` folder
    /// (a nested file named by its path below it; the folder of another registry namespace, or of
    /// another registry, not), in the order vanilla registers the files (by path, then namespace),
    /// leaving out a file whose conditions don't hold (another mod not loaded; a kind of condition
    /// unknown) and one its codec refuses. Reached outside a hook, with no resources to read, it is
    /// empty and read later; read once. An entry is found by id, and Create's search for an item's
    /// type (the first entry whose set holds the item) finds each item's.
    #[test]
    fn a_mods_datapack_registry_reads_its_entries_from_the_data_files() {
        let mut rt = Rt::new();
        crate::install();
        let loc = rt.string("testmod:gadget/type");
        let key = api::call(&mut rt, CREATE_KEY, &[loc]);
        let items = api::get_static(&mut rt, "net/minecraft/core/registries/Registries.ITEM");
        let set = api::call(&mut rt, "net/minecraft/core/RegistryCodecs.homogeneousList.(Lnet/minecraft/resources/ResourceKey;)Lcom/mojang/serialization/Codec;", &[items]);
        let field = rt.string("items");
        let field = api::call(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[set, field]);
        let codec = api::call(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[field]);
        let event = rt.new_object(NEW_REGISTRY);
        api::call(&mut rt, DATAPACK_REGISTRY, &[event, key, codec]);

        let mut host = MemoryHost::default();
        file(&mut host, "data/testmod/testmod/gadget/type/potato.json", r#"{"items": "minecraft:potato"}"#);
        file(&mut host, "data/addon/testmod/gadget/type/potato.json", r#"{"items": ["minecraft:poisonous_potato", "minecraft:potato"]}"#);
        file(&mut host, "data/addon/testmod/gadget/type/apple.json", r#"{"items": "minecraft:apple", "neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "testmod"}]}"#);
        file(&mut host, "data/addon/testmod/gadget/type/carrot.json", r#"{"items": "minecraft:carrot", "neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "absent"}]}"#);
        file(&mut host, "data/testmod/testmod/gadget/type/odd.json", r#"{"items": "minecraft:beetroot", "neoforge:conditions": [{"type": "neoforge:not", "value": {"type": "other:unknown"}}]}"#);
        file(&mut host, "data/testmod/testmod/gadget/type/broken.json", r#"{"items": 5}"#);
        file(&mut host, "data/testmod/testmod/gadget/type/nested/melon.json", r#"{"items": "minecraft:melon_slice"}"#);
        file(&mut host, "data/testmod/testmod/gadget/types/pear.json", r#"{"items": "minecraft:apple"}"#);
        file(&mut host, "data/testmod/othermod/gadget/type/plum.json", r#"{"items": "minecraft:apple"}"#);

        assert!(listed(&mut rt, key).is_empty(), "no resources outside a hook");
        rt.with_host(&mut host, |rt| {
            let entries = listed(rt, key);
            let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, ["addon:apple", "testmod:nested/melon", "addon:potato", "testmod:potato"]);
            assert_eq!(listed(rt, key), entries, "read once: the same holders");

            // Found by id, the same value its holder has.
            let id = rt.string("testmod:potato");
            let value = api::call(rt, GET, &[key, id]);
            assert_eq!(api::call(rt, VALUE, &[entries[3].1]), value);

            // Create's `getTypeForItem`: the first entry whose items hold the item.
            let mut type_of = |item: &str| {
                let holder = crate::neo::registry_holder(rt, "item", item, V::NULL);
                entries.iter().find(|(_, h)| {
                    let set = api::call(rt, VALUE, &[*h]);
                    api::call(rt, CONTAINS, &[set, holder]).i() != 0
                })
                .map(|(n, _)| n.clone())
            };
            assert_eq!(type_of("minecraft:potato").as_deref(), Some("addon:potato"));
            assert_eq!(type_of("minecraft:poisonous_potato").as_deref(), Some("addon:potato"));
            assert_eq!(type_of("minecraft:melon_slice").as_deref(), Some("testmod:nested/melon"));
            assert_eq!(type_of("minecraft:carrot"), None);
        });
    }
}
