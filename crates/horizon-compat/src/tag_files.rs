//! Tags read from their files, for the registries the game knows nothing of: a mod's own
//! (Mekanism's chemicals: `data/<namespace>/tags/mekanism/chemical/<path>.json`) and the
//! datapack registries mods make. The game's own registries' tags are the game's to tell
//! ([`crate::neo::in_tag`]): its files are not all in reach here (vanilla's own copy of a tag a
//! mod adds to is under the mod's).
//!
//! A tag's files are read as vanilla's `TagLoader` reads them: every pack's copy of
//! `data/<namespace>/tags/<folder>/<path>.json`, the lowest first ([`crate::resources::copies`]);
//! a copy that says `"replace": true` drops what the ones before it gave; an entry is an id, `#`
//! and a tag (its entries in its place), or `{"id": ..., "required": false}` for one that may be
//! missing. An entry that names nothing registered is left out (vanilla refuses the whole tag
//! for a required one, with an error in its log).
use crate::{Rt, V};
use serde_json::Value as Json;

/// What a tag's files gave, kept by registry and tag (`compat:tag-file/<registry>/<tag>`): a list
/// of the ids, or 0 for a tag no file is.
const KEPT: &str = "compat:tag-file/";

/// The entries of `registry` (by its name here) that the files of `tag` (`mekanism:water_vapor`,
/// no `#`) give, in the files' order, each once; of a mod's own registry, those that are
/// registered. None when no file is that tag.
pub(crate) fn members(rt: &mut Rt, registry: &str, tag: &str) -> Option<Vec<String>> {
    let key = format!("{KEPT}{registry}/{tag}");
    if let Some(&kept) = rt.interned.get(&key) {
        return (kept.r() != 0).then(|| {
            crate::jutil::elements(rt, kept)
                .into_iter()
                .filter_map(|v| rt.str_of(v).map(str::to_owned))
                .collect()
        });
    }
    // A datapack registry's entries are files too, read when first asked for.
    crate::datapack::read(rt, registry);
    let folder = crate::datapack::folder(rt, registry);
    // (A registry of the game's that the runtime keeps no entries of has its files' word.)
    let own = folder != registry || rt.registered.keys().any(|(r, _)| r == registry);
    let found = read(rt, &folder, tag, &mut Vec::new()).map(|ids| {
        ids.into_iter()
            .filter(|id| {
                !own || rt
                    .registered
                    .contains_key(&(registry.to_owned(), id.clone()))
            })
            .collect::<Vec<String>>()
    });
    let kept = match &found {
        Some(ids) => {
            let values: Vec<V> = ids.iter().map(|i| rt.string(i)).collect();
            crate::jutil::new_list(rt, values)
        }
        None => V::I(0),
    };
    rt.interned.insert(key, kept);
    found
}

/// The tags of `registry` that hold its entry `id` (`Holder.tags()`), as tag keys: the game's
/// answer for a registry of its own (asked as a holder set of Horizon's own kind,
/// `horizon:tags_of`), the files' for a mod's own. None where no game is there to tell of its
/// registries.
pub(crate) fn tags_of(rt: &mut Rt, registry: &str, id: &str) -> Vec<V> {
    let registry = registry.strip_prefix("minecraft:").unwrap_or(registry);
    let asked = serde_json::json!({"type": "horizon:tags_of", "id": id}).to_string();
    let names = match rt.host().and_then(|h| h.holder_set(registry, &asked)) {
        Some(names) => names,
        None if crate::neo::games_registry(registry) => Vec::new(),
        None => {
            let mut held = Vec::new();
            for tag in names(rt, registry) {
                if members(rt, registry, &tag).is_some_and(|ids| ids.iter().any(|i| i == id)) {
                    held.push(tag);
                }
            }
            held
        }
    };
    names
        .iter()
        .map(|tag| crate::fluids::tag_key(rt, registry, tag))
        .collect()
}

/// The tags of a mod's own `registry` there are files for, by name, sorted.
fn names(rt: &mut Rt, registry: &str) -> Vec<String> {
    let folder = crate::datapack::folder(rt, registry);
    let mut out = Vec::new();
    for namespace in crate::resources::namespaces(rt, "data") {
        let prefix = format!("data/{namespace}/tags/{folder}/");
        for path in crate::resources::paths(rt, &prefix) {
            if let Some(name) = path
                .strip_prefix(&prefix)
                .and_then(|p| p.strip_suffix(".json"))
            {
                out.push(format!("{namespace}:{name}"));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// What was read of the tags' files is forgotten: the data was loaded again.
pub(crate) fn forget(rt: &mut Rt) {
    rt.interned.retain(|k, _| !k.starts_with(KEPT));
}

/// What a tag's files hold together: each copy's `values` after the last copy that replaces
/// (`"replace": true`), an id or a tag (`#`) each, as written.
fn entries(copies: &[Vec<u8>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for copy in copies {
        let Ok(json) = serde_json::from_slice::<Json>(copy) else {
            continue;
        };
        if json.get("replace").and_then(Json::as_bool) == Some(true) {
            out.clear();
        }
        for value in json
            .get("values")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(entry) = value
                .as_str()
                .or_else(|| value.get("id").and_then(Json::as_str))
            {
                out.push(entry.to_owned());
            }
        }
    }
    out
}

/// The ids the files of `tag` in `folder` give, tags within it followed (`walking`: those being
/// read, which one of them naming itself in the end adds nothing to).
fn read(rt: &Rt, folder: &str, tag: &str, walking: &mut Vec<String>) -> Option<Vec<String>> {
    let (ns, path) = tag.split_once(':').unwrap_or(("minecraft", tag));
    let copies = crate::resources::copies(rt, &format!("data/{ns}/tags/{folder}/{path}.json"));
    if copies.is_empty() || walking.iter().any(|w| w == tag) {
        return None;
    }
    walking.push(tag.to_owned());
    let named = |entry: &str| {
        if entry.contains(':') {
            entry.to_owned()
        } else {
            format!("minecraft:{entry}")
        }
    };
    let mut out: Vec<String> = Vec::new();
    for entry in entries(&copies) {
        let ids = match entry.strip_prefix('#') {
            Some(inner) => read(rt, folder, &named(inner), walking).unwrap_or_default(),
            None => vec![named(&entry)],
        };
        for id in ids {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    walking.pop();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag of a mod's own registry is its files: an entry, a tag within it, an entry that may
    /// be missing; what names nothing registered is left out; a tag no file is, is none. The
    /// registry's tags list and tell membership by them (Mekanism's recipes name a chemical by
    /// its tag: with every tag empty, 69 were left out as incomplete).
    #[test]
    fn a_mods_own_registrys_tag_is_its_files() {
        const GET_TAG: &str =
            "net/minecraft/core/Registry.getTag.(Lnet/minecraft/tags/TagKey;)Ljava/util/Optional;";
        crate::install();
        let mut rt = Rt::new();
        // The registry `t:gas`, made by key as a mod makes its own, with three gases.
        crate::datapack::key_made(&mut rt, "t:gas");
        for id in ["t:steam", "t:vapor", "t:smoke"] {
            let gas = rt.new_object("t/Gas");
            rt.registered.insert(("gas".into(), id.into()), gas);
        }
        let mut host = crate::host::MemoryHost::default();
        for (path, json) in [
            (
                "data/t/tags/t/gas/hot.json",
                r##"{"values": ["t:steam", "#t:wet", {"id": "t:plasma", "required": false}]}"##,
            ),
            (
                "data/t/tags/t/gas/wet.json",
                r##"{"values": ["t:vapor", "t:steam", "#t:hot"]}"##,
            ),
        ] {
            host.resources.insert(path.into(), json.as_bytes().to_vec());
        }
        rt.with_host(&mut host, |rt| {
            assert_eq!(
                members(rt, "gas", "t:hot"),
                Some(vec!["t:steam".to_owned(), "t:vapor".to_owned()])
            );
            assert_eq!(
                members(rt, "gas", "t:wet"),
                Some(vec!["t:vapor".to_owned(), "t:steam".to_owned()])
            );
            assert_eq!(members(rt, "gas", "t:cold"), None);
            // Through the registry, as a mod asks.
            let registry = rt.string("gas");
            let (hot, cold) = (
                crate::fluids::tag_key(rt, "gas", "t:hot"),
                crate::fluids::tag_key(rt, "gas", "t:cold"),
            );
            let found = crate::api::call(rt, GET_TAG, &[registry, hot]);
            let set = rt.get_field(found, "value");
            assert_eq!(
                crate::api::call(rt, "net/minecraft/core/HolderSet.size.()I", &[set]),
                V::I(2)
            );
            let none = crate::api::call(rt, GET_TAG, &[registry, cold]);
            assert_eq!(rt.get_field(none, "value"), V::NULL);
            assert_eq!(crate::neo::in_tag(rt, "gas", "t:vapor", hot), Some(true));
            assert_eq!(crate::neo::in_tag(rt, "gas", "t:smoke", hot), Some(false));
            assert_eq!(crate::neo::in_tag(rt, "gas", "t:smoke", cold), None);
            // The key names its registry in full, the mod's namespace before it.
            let text = crate::api::call(
                rt,
                "net/minecraft/tags/TagKey.toString.()Ljava/lang/String;",
                &[hot],
            );
            assert_eq!(rt.str_of(text), Some("TagKey[t:gas / t:hot]"));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Each pack's copy of a tag's file adds to the ones below it, and one that replaces drops
    /// them, as vanilla's loader puts a tag together.
    #[test]
    fn a_tags_copies_add_up_until_one_replaces() {
        let copy = |json: &str| json.as_bytes().to_vec();
        let lower = copy(r##"{"values": ["a:one", "#a:more"]}"##);
        let adds = copy(r##"{"replace": false, "values": [{"id": "b:two", "required": false}]}"##);
        let replaces = copy(r##"{"replace": true, "values": ["c:three"]}"##);
        assert_eq!(
            entries(&[lower.clone(), adds.clone()]),
            ["a:one", "#a:more", "b:two"]
        );
        assert_eq!(
            entries(&[lower.clone(), replaces.clone(), adds]),
            ["c:three", "b:two"]
        );
        assert_eq!(entries(&[copy("not json"), lower]), ["a:one", "#a:more"]);
        assert!(entries(&[]).is_empty());
    }
}
