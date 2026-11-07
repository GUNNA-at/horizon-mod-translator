//! Damage types: a content mod's `data/<ns>/damage_type/<path>.json` join the
//! game's damage catalog (`damage-catalog.json`, what a player's hurt reads:
//! armor, exhaustion, the hurt sound, the death message), and its damage type
//! tags (`bypasses_armor`, `is_fire`) join the catalog's tag bundle
//! (`damage-tags.json`). A server sends both to its clients with the
//! synchronized registries (`lan::data`), and the commands take the types
//! (`/damage @s 5 create:crush`).
//!
//! A type the catalog can't read (a scaling it doesn't know) is left out with
//! a warning, and so are tag entries naming a type or a tag the root lacks:
//! the catalog refuses a tag naming what it lacks as a whole, as vanilla's tag
//! loader does.

use super::{J, Result, merge_tag};
use horizon_world::{DATA_VERSION, damage_source::Catalog};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The catalog and its tag bundle (tag id → the tag file's text) in a
/// prepared root.
pub(super) const CATALOG: &str = "damage-catalog.json";
pub(super) const TAGS: &str = "damage-tags.json";

/// `data/<ns>/damage_type/<path>.json`'s damage type, `<ns>:<path>`.
pub(super) fn type_id(file: &str) -> Option<String> {
    let (ns, path) = file.strip_prefix("data/")?.split_once("/damage_type/")?;
    Some(format!("{ns}:{}", path.strip_suffix(".json")?))
}

/// `data/<ns>/tags/damage_type/<path>.json`'s tag, `<ns>:<path>`.
pub(super) fn tag_id(file: &str) -> Option<String> {
    let (ns, path) = file
        .strip_prefix("data/")?
        .split_once("/tags/damage_type/")?;
    Some(format!("{ns}:{}", path.strip_suffix(".json")?))
}

/// A damage type file as the catalog's row: `DamageType.CODEC`'s fields,
/// `effects` and `death_message_type` defaulting as its optional fields do,
/// the exhaustion as its float's bits. The catalog's own reading checks the
/// values.
fn row(id: &str, bytes: &[u8]) -> std::result::Result<Value, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let text = |key: &str, default: Option<&str>| match doc.get(key) {
        None => default.map(str::to_owned).ok_or(format!("no {key}")),
        Some(v) => v
            .as_str()
            .map(str::to_owned)
            .ok_or(format!("{key} is not a string")),
    };
    let exhaustion = doc
        .get("exhaustion")
        .and_then(Value::as_f64)
        .ok_or("no exhaustion")? as f32;
    let row = json!({
        "id": id,
        "message_id": text("message_id", None)?,
        "exhaustion": format!("{:08x}", exhaustion.to_bits()),
        "scaling": text("scaling", None)?,
        "effects": text("effects", Some("hurt"))?,
        "death_message_type": text("death_message_type", Some("default"))?,
    });
    let alone = json!({"data_version": DATA_VERSION, "types": [row]});
    Catalog::from_json(&alone, &json!({})).map_err(|e| e.to_string())?;
    Ok(row)
}

/// The base's catalog and tag bundle with the mods' damage types and tags.
pub(super) struct Damage {
    catalog: Value,
    tags: BTreeMap<String, String>,
    /// Every type the catalog holds: the base's and the mods' it reads.
    pub(super) ids: BTreeSet<String>,
    /// The base's tags.
    pub(super) base_tags: BTreeSet<String>,
    /// The mods' types it reads, and the tags the mods add, in id order:
    /// the commands' lists take them.
    pub(super) new_ids: Vec<String>,
    pub(super) new_tags: Vec<String>,
}

impl Damage {
    /// The base's `catalog` and tag bundle (`tags`) with the mods' types
    /// (`types`: id → the mod and its file, a later mod's over an earlier's).
    /// A mod's file for one of the base's types replaces its row, as a data
    /// pack's does.
    pub(super) fn new(
        mut catalog: Value,
        tags: BTreeMap<String, String>,
        types: &BTreeMap<String, (String, Vec<u8>)>,
        warnings: &mut Vec<String>,
    ) -> Result<Self> {
        let rows = catalog["types"]
            .as_array_mut()
            .ok_or("damage catalog: no types")?;
        let mut ids: BTreeSet<String> = rows
            .iter()
            .filter_map(|r| r["id"].as_str().map(str::to_owned))
            .collect();
        let mut new_ids = Vec::new();
        for (id, (from, bytes)) in types {
            let row = match row(id, bytes) {
                Ok(row) => row,
                Err(e) => {
                    warnings.push(format!("mod {from}: damage type {id} is left out: {e}"));
                    continue;
                }
            };
            match rows.iter_mut().find(|r| r["id"] == id.as_str()) {
                Some(old) => *old = row,
                None => {
                    rows.push(row);
                    ids.insert(id.clone());
                    new_ids.push(id.clone());
                }
            }
        }
        Ok(Self {
            catalog,
            base_tags: tags.keys().cloned().collect(),
            tags,
            ids,
            new_ids,
            new_tags: Vec::new(),
        })
    }

    /// Merges the mods' entries of tag `id` (`values`, already left with
    /// known types and tags) into the bundle: after the base's, or alone
    /// when a mod `replaced` the tag.
    pub(super) fn merge(&mut self, id: &str, values: &[J], replaced: bool) -> Result<()> {
        let base = self.tags.get(id).filter(|_| !replaced);
        let merged = merge_tag(base.map(String::as_str), values, TAGS)?;
        if !self.base_tags.contains(id) {
            self.new_tags.push(id.to_owned());
        }
        self.tags.insert(id.to_owned(), merged);
        Ok(())
    }

    /// The catalog's and the bundle's files, once the catalog reads them as
    /// the game will. None, with a warning, if it doesn't: the base's stay,
    /// and the mods' types hurt as `minecraft:generic`.
    pub(super) fn files(&self, warnings: &mut Vec<String>) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        let tags = serde_json::to_value(&self.tags)?;
        if let Err(e) = Catalog::from_json(&self.catalog, &tags) {
            warnings.push(format!(
                "the mods' damage types and tags are left out: the catalog doesn't read them: {e}"
            ));
            return Ok(None);
        }
        Ok(Some((
            serde_json::to_vec(&self.catalog)?,
            serde_json::to_vec(&tags)?,
        )))
    }
}
