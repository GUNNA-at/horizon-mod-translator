//! Holder sets over the game's own registries, for native mods: what a loaded tag holds, and
//! every entry of a registry.
use super::{Game, GameLevel};
use horizon_client::{atlas_sources::qualified, items::Items};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// What a holder set is resolved against: the tables in the caller's hand.
struct Listed<'a> {
    items: &'a Items,
    states: &'a BlockRegistry,
    /// The entity type tags, where the caller has the mobs' data.
    entity_tags: Option<&'a HashMap<String, HashSet<String>>>,
    /// Every entry of a registry other than the items' and the blocks' (no `minecraft:`), or
    /// none where the caller keeps no list of it.
    others: &'a dyn Fn(&str) -> Option<Vec<String>>,
}

impl Listed<'_> {
    fn set(&self, registry: &str, set: &Value) -> Option<Vec<String>> {
        match set {
            Value::String(entry) => match entry.strip_prefix('#') {
                Some(tag) => self.tag(registry, &qualified(tag)),
                None => Some(vec![qualified(entry)]),
            },
            Value::Array(entries) => self.joined(registry, entries),
            Value::Object(object) => {
                let kind = object.get("type")?.as_str()?;
                // Horizon's own question through the same door: the tags that hold an entry.
                if kind == "horizon:tags_of" {
                    return self.tags_of(registry, &qualified(object.get("id")?.as_str()?));
                }
                match kind.strip_prefix("neoforge:").unwrap_or(kind) {
                    "any" => self.all(registry),
                    "or" => self.joined(registry, object.get("values")?.as_array()?),
                    "and" => {
                        let mut sets = object.get("values")?.as_array()?.iter();
                        let mut kept = self.set(registry, sets.next()?)?;
                        for set in sets {
                            let also: HashSet<String> =
                                self.set(registry, set)?.into_iter().collect();
                            kept.retain(|id| also.contains(id));
                        }
                        Some(kept)
                    }
                    "not" => {
                        let left_out: HashSet<String> = self
                            .set(registry, object.get("value")?)?
                            .into_iter()
                            .collect();
                        let mut all = self.all(registry)?;
                        all.retain(|id| !left_out.contains(id));
                        Some(all)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The sets of `entries` one after another, an entry met again left out.
    fn joined(&self, registry: &str, entries: &[Value]) -> Option<Vec<String>> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for entry in entries {
            for id in self.set(registry, entry)? {
                if seen.insert(id.clone()) {
                    out.push(id);
                }
            }
        }
        Some(out)
    }

    /// The members of tag `tag` (`minecraft:logs`, no `#`), as the loaded tags hold them: an
    /// item tag's in the order vanilla resolves them, a block, fluid or entity type tag's by
    /// name (the game keeps those as sets). None for a tag the game does not have: a tag of a
    /// mod's that names nothing is not told from one that was never read, and the caller may
    /// know more.
    fn tag(&self, registry: &str, tag: &str) -> Option<Vec<String>> {
        let sorted = |members: &HashSet<String>| {
            let mut members: Vec<String> = members.iter().cloned().collect();
            members.sort();
            members
        };
        match registry {
            "item" => self
                .items
                .item_tag_order
                .get(tag)
                .cloned()
                .or_else(|| self.items.item_tags.get(tag).map(sorted)),
            "block" => self.items.block_tags.get(tag).map(sorted),
            "fluid" => self.items.fluid_tags.get(tag).map(sorted),
            "entity_type" => self.entity_tags?.get(tag).map(sorted),
            _ => None,
        }
    }

    /// The loaded tags of `registry` that hold its entry `id`, by name (`Holder.tags()`). None
    /// for a registry whose tags the caller does not have.
    fn tags_of(&self, registry: &str, id: &str) -> Option<Vec<String>> {
        let tags = match registry {
            "item" => &self.items.item_tags,
            "block" => &self.items.block_tags,
            "fluid" => &self.items.fluid_tags,
            "entity_type" => self.entity_tags?,
            _ => return None,
        };
        let mut held: Vec<String> = tags
            .iter()
            .filter(|(_, members)| members.contains(id))
            .map(|(tag, _)| tag.clone())
            .collect();
        held.sort();
        Some(held)
    }

    /// Every entry of `registry`: the items and the blocks in their registries' order, another
    /// registry's as the caller lists them.
    fn all(&self, registry: &str) -> Option<Vec<String>> {
        match registry {
            "item" => Some(
                self.items
                    .registry_order
                    .iter()
                    .map(|name| name.to_string())
                    .collect(),
            ),
            "block" => {
                // A block's states follow one another in the state table.
                let mut seen = HashSet::new();
                let mut names: Vec<String> = Vec::new();
                for i in 0..self.states.len() as u32 {
                    let Some(state) = self.states.state(BlockStateId(i)) else {
                        continue;
                    };
                    if names.last() != Some(&state.name) && seen.insert(state.name.as_str()) {
                        names.push(state.name.clone());
                    }
                }
                Some(names)
            }
            other => (self.others)(other),
        }
    }
}

// Native mods use these helpers.
#[allow(dead_code)]
impl Game<'_> {
    /// The entries holder set `set` names in registry `registry` (`block`, `item`, `fluid`,
    /// `entity_type`..., with `minecraft:` or without), each once, in order. The set is its
    /// JSON: an id (itself), `#` and a tag (the loaded tag's members), a list of those, or
    /// NeoForge's `{"type": "neoforge:any"}` (every entry of the registry), `neoforge:or` and
    /// `neoforge:and` (over `values`) and `neoforge:not` (over `value`). And one question of
    /// Horizon's own in that form, `{"type": "horizon:tags_of", "id": ...}`: the names of the
    /// loaded tags that hold that entry (`Holder.tags()`). None for a set the game cannot
    /// list: a registry it keeps no list of, a tag it does not have, another kind of object.
    /// API @API:holder_sets@.
    pub(super) fn native_holder_set(&self, registry: &str, set: &Value) -> Option<Vec<String>> {
        let others = |registry: &str| match registry {
            // The entity types the commands know, then the mods' mobs they do not name.
            "entity_type" => {
                let mut names = self.ui.registries.entity_types.clone();
                let known: HashSet<String> = names.iter().cloned().collect();
                let mut mods: Vec<String> = self
                    .res
                    .mob_types()
                    .keys()
                    .map(|name| qualified(name))
                    .filter(|name| !known.contains(name))
                    .collect();
                mods.sort();
                names.extend(mods);
                (!names.is_empty()).then_some(names)
            }
            _ => {
                let names = self.ui.registries.ids(&format!("minecraft:{registry}"));
                (!names.is_empty()).then(|| names.to_vec())
            }
        };
        Listed {
            items: self.items(),
            states: self.pack.assets.registry,
            entity_tags: Some(&self.res.mob_data().entity_tags),
            others: &others,
        }
        .set(registry.strip_prefix("minecraft:").unwrap_or(registry), set)
    }
}

// Native mods use these helpers.
#[allow(dead_code)]
impl GameLevel<'_> {
    /// [`Game::native_holder_set`] for hooks on a level, which has the items, the blocks and
    /// their tags in hand: an item, block or fluid tag's members and every item or block.
    /// None for another registry. API @API:holder_sets@.
    pub(super) fn native_holder_set(&self, registry: &str, set: &Value) -> Option<Vec<String>> {
        Listed {
            items: self.items,
            states: self.registry,
            entity_tags: None,
            others: &|_| None,
        }
        .set(registry.strip_prefix("minecraft:").unwrap_or(registry), set)
    }
}
