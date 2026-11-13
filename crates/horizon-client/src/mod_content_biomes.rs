//! Biome modifiers: a content mod's placed features and mob spawns in biomes
//! it does not own (vanilla's), as NeoForge's `add_features`,
//! `remove_features`, `add_spawns`, `remove_spawns`, `add_spawn_costs` and
//! `remove_spawn_costs` biome modifiers do. Each is a file in
//! `data/<ns>/neoforge/biome_modifier/` (older Forge mods:
//! `data/<ns>/forge/biome_modifier/`, type `forge:...`).
//!
//! They rewrite the biomes' `features` lists and
//! `minecraft:gameplay/natural_mob_spawns` attributes when the content root
//! builds: an added feature goes at the end of its step's list, as NeoForge
//! adds it, and the generator orders every biome's lists as vanilla's
//! `FeatureSorter` does; an added spawner goes at the end of its entity type's
//! spawn category with `minCount` and `maxCount` converted to 26.3's `count`.
//! Biomes no modifier names keep their text byte for byte, and a root with no
//! modifiers keeps vanilla's bundle, so vanilla features keep their indices
//! and their random seeds.
//!
//! A modifier of a translated mod's own type (Farmer's Delight's
//! `farmersdelight:add_features_by_filter`, Waddles' `penguin_spawn`) is read
//! by the mod's codec and run by its code (`BiomeModifier.modify`), biome by
//! biome and phase by phase as NeoForge runs it, on the root's biomes, tags,
//! placed features and mob spawns ([`own_types`]).
use super::{J, Result};
use horizon_world::worldgen::foreign::{self, BiomeModifier};
use horizon_world::worldgen::structure::start::STEPS;
use serde_json::{Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The folders a biome modifier sits in, under `data/<ns>`.
const FOLDERS: [&str; 2] = ["/neoforge/biome_modifier/", "/forge/biome_modifier/"];

/// Whether a mod file is a biome modifier.
pub(super) fn is_modifier(file: &str) -> bool {
    file.starts_with("data/") && file.ends_with(".json") && FOLDERS.iter().any(|f| file.contains(f))
}

/// One mob spawner entry in an `add_spawns` modifier.
#[derive(Debug, PartialEq)]
struct Spawner {
    entity: String,
    weight: i64,
    min_count: i64,
    max_count: i64,
}

/// What a modifier does to its biomes.
#[derive(Debug, PartialEq)]
enum Action {
    /// Appends features to one generation step.
    AddFeatures { features: Value, step: usize },
    /// Takes features out of these steps (every step when the file names none).
    RemoveFeatures { features: Value, steps: Vec<usize> },
    /// Appends spawners to their entity types' spawn categories.
    AddSpawns(Vec<Spawner>),
    /// Takes these entity types out of every spawn category.
    RemoveSpawns(Value),
    /// Sets `spawn_costs` for these entity types.
    AddSpawnCosts {
        entity_types: Value,
        energy_budget: Number,
        charge: Number,
    },
    /// Takes these entity types out of `spawn_costs`.
    RemoveSpawnCosts(Value),
    /// A translated mod's own type: its code edits the biomes it picks
    /// itself ([`Modifier::code`]).
    Code,
}

impl Action {
    fn is_add(&self) -> bool {
        matches!(
            self,
            Self::AddFeatures { .. } | Self::AddSpawns(_) | Self::AddSpawnCosts { .. }
        )
    }
}

/// A translated mod's biome modifier, as its codec read it.
struct Code(Arc<dyn BiomeModifier>);

impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the mod's code")
    }
}

/// One biome modifier file.
#[derive(Debug)]
pub(super) struct Modifier {
    /// `<mod>: <file>`, for errors.
    name: String,
    biomes: Value,
    action: Action,
    /// What runs an [`Action::Code`].
    code: Option<Code>,
    /// The mod, the file and its text, kept in the content root so the
    /// modifier is applied again over a world's data packs (`over_packs`).
    pub source: [String; 3],
}

/// A generation step's index from its name.
fn step(v: &Value) -> std::result::Result<usize, String> {
    let name = v.as_str().ok_or("step: not a name")?;
    STEPS
        .iter()
        .position(|s| *s == name)
        .ok_or_else(|| format!("unknown generation step {name:?}"))
}

/// One NeoForge `SpawnerData` object from `spawners`.
fn spawner(v: &Value) -> std::result::Result<Spawner, String> {
    let o = v.as_object().ok_or("spawner: not an object")?;
    let entity = o
        .get("type")
        .and_then(Value::as_str)
        .ok_or("spawner: no type")?;
    let int = |k: &str| {
        o.get(k)
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("spawner: no {k}"))
    };
    let weight = int("weight")?;
    let min_count = int("minCount")?;
    let max_count = int("maxCount")?;
    if weight < 0 {
        return Err(format!("spawner: weight ({weight}) < 0"));
    }
    if min_count <= 0 {
        return Err(format!("spawner: minCount ({min_count}) must be positive"));
    }
    if min_count > max_count {
        return Err(format!(
            "spawner: minCount ({min_count}) > maxCount ({max_count})"
        ));
    }
    Ok(Spawner {
        entity: full(entity),
        weight,
        min_count,
        max_count,
    })
}

impl Modifier {
    /// The modifier in `text`; `None` for `neoforge:none`, which does nothing,
    /// and for a type Horizon doesn't read yet (`add_carvers`, a mod's own
    /// serializer), which is skipped with a line in `warnings`: real mods ship
    /// those beside their feature and spawn modifiers, and one must not drop
    /// every content mod.
    pub(super) fn read(
        mod_id: &str,
        file: &str,
        text: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Option<Self>> {
        let name = format!("mod {mod_id}: {file}");
        let at = |e: String| format!("{name}: {e}");
        let doc: Value = serde_json::from_str(text).map_err(|e| at(e.to_string()))?;
        let kind = doc["type"].as_str().ok_or_else(|| at("no type".into()))?;
        let full_kind = kind;
        let kind = kind
            .strip_prefix("neoforge:")
            .or_else(|| kind.strip_prefix("forge:"))
            .unwrap_or(kind);
        let field = |key: &str| -> std::result::Result<Value, String> {
            doc.get(key).cloned().ok_or_else(|| at(format!("no {key}")))
        };
        let action = match kind {
            "none" => return Ok(None),
            "add_features" => Action::AddFeatures {
                features: field("features")?,
                step: step(&field("step")?).map_err(at)?,
            },
            "remove_features" => Action::RemoveFeatures {
                features: field("features")?,
                steps: match doc.get("steps") {
                    None => (0..STEPS.len()).collect(),
                    Some(Value::Array(steps)) => steps
                        .iter()
                        .map(step)
                        .collect::<std::result::Result<_, _>>()
                        .map_err(at)?,
                    Some(one) => vec![step(one).map_err(at)?],
                },
            },
            "add_spawns" => {
                let raw = field("spawners")?;
                let spawners = match &raw {
                    Value::Array(list) => list
                        .iter()
                        .map(spawner)
                        .collect::<std::result::Result<_, _>>()
                        .map_err(at)?,
                    one @ Value::Object(_) => vec![spawner(one).map_err(at)?],
                    _ => return Err(at("spawners: not a spawner or list".into()).into()),
                };
                Action::AddSpawns(spawners)
            }
            "remove_spawns" => Action::RemoveSpawns(field("entity_types")?),
            "add_spawn_costs" => {
                let entity_types = field("entity_types")?;
                let cost = field("spawn_cost")?;
                let num = |k: &str| {
                    cost.get(k)
                        .and_then(Value::as_number)
                        .cloned()
                        .ok_or_else(|| at(format!("spawn_cost: no {k}")))
                };
                Action::AddSpawnCosts {
                    entity_types,
                    energy_budget: num("energy_budget")?,
                    charge: num("charge")?,
                }
            }
            "remove_spawn_costs" => Action::RemoveSpawnCosts(field("entity_types")?),
            other => {
                match foreign::biome_modifier(full_kind, text) {
                    Some(Ok(code)) => {
                        return Ok(Some(Self {
                            biomes: Value::Null,
                            action: Action::Code,
                            code: Some(Code(code)),
                            name,
                            source: [mod_id.to_owned(), file.to_owned(), text.to_owned()],
                        }));
                    }
                    Some(Err(e)) => warnings.push(at(format!(
                        "biome modifier type {full_kind:?} skipped: the mod's codec refused it: {e}"
                    ))),
                    None => warnings.push(at(format!(
                        "biome modifier type {other:?} skipped: only feature, spawn and none \
                         modifiers are read, and no translated mod registers it"
                    ))),
                }
                return Ok(None);
            }
        };
        Ok(Some(Self {
            biomes: field("biomes")?,
            action,
            code: None,
            name,
            source: [mod_id.to_owned(), file.to_owned(), text.to_owned()],
        }))
    }
}

/// `ns:path` for an id that may leave out `minecraft:`.
fn full(id: &str) -> String {
    match id.contains(':') {
        true => id.to_owned(),
        false => format!("minecraft:{id}"),
    }
}

/// A registry the modifiers name entries of: its ids and its tags (tag id →
/// the tag file's text), as the worldgen loader's bundles hold them.
pub(super) struct Registry<'a> {
    pub what: &'static str,
    pub ids: &'a BTreeSet<String>,
    pub tags: &'a BTreeMap<String, String>,
}

impl Registry<'_> {
    /// The ids a holder set names, in its order without repeats: an id,
    /// `#tag`, a list of ids, or one of NeoForge's `any`, `or`, `and` and
    /// `not` objects. Unknown ids and tags are errors; inside a tag, entries
    /// nobody registers are left out, as the worldgen loader leaves them out.
    fn resolve(&self, set: &Value) -> std::result::Result<Vec<String>, String> {
        let mut out = Vec::new();
        match set {
            Value::String(s) => match s.strip_prefix('#') {
                Some(tag) => self.expand(&full(tag), &mut out, &mut BTreeSet::new())?,
                None => {
                    let id = full(s);
                    if !self.ids.contains(&id) {
                        return Err(format!("unknown {} {id}", self.what));
                    }
                    out.push(id);
                }
            },
            Value::Array(list) => {
                for v in list {
                    match v {
                        Value::String(s) if !s.starts_with('#') => {
                            extend(&mut out, self.resolve(v)?)
                        }
                        _ => return Err(format!("a {} list holds ids only", self.what)),
                    }
                }
            }
            Value::Object(o) => {
                let kind = o.get("type").and_then(Value::as_str).unwrap_or_default();
                let kind = kind.strip_prefix("neoforge:").unwrap_or(kind);
                let sets = || -> std::result::Result<Vec<Vec<String>>, String> {
                    o.get("values")
                        .and_then(Value::as_array)
                        .ok_or(format!("{kind}: no values"))?
                        .iter()
                        .map(|v| self.resolve(v))
                        .collect()
                };
                match kind {
                    "any" => out.extend(self.ids.iter().cloned()),
                    "or" => sets()?.into_iter().for_each(|s| extend(&mut out, s)),
                    "and" => {
                        let mut sets = sets()?.into_iter();
                        out = sets.next().unwrap_or_default();
                        for s in sets {
                            out.retain(|id| s.contains(id));
                        }
                    }
                    "not" => {
                        let not = self.resolve(o.get("value").ok_or("not: no value")?)?;
                        out.extend(self.ids.iter().filter(|id| !not.contains(id)).cloned());
                    }
                    _ => return Err(format!("{} set type {kind:?} is not read", self.what)),
                }
            }
            _ => return Err(format!("not a {} id, tag or list", self.what)),
        }
        Ok(out)
    }

    fn expand(
        &self,
        tag: &str,
        out: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) -> std::result::Result<(), String> {
        if !seen.insert(tag.to_owned()) {
            return Ok(());
        }
        let text = self
            .tags
            .get(tag)
            .ok_or_else(|| format!("unknown {} tag #{tag}", self.what))?;
        let doc: Value = serde_json::from_str(text).map_err(|e| format!("#{tag}: {e}"))?;
        for v in doc["values"].as_array().into_iter().flatten() {
            let (id, required) = match v {
                Value::String(s) => (s.as_str(), true),
                Value::Object(o) => match o.get("id").and_then(Value::as_str) {
                    Some(id) => (id, o.get("required") != Some(&Value::Bool(false))),
                    None => continue,
                },
                _ => continue,
            };
            match id.strip_prefix('#') {
                Some(inner) if required || self.tags.contains_key(&full(inner)) => {
                    self.expand(&full(inner), out, seen)?
                }
                Some(_) => {}
                None if self.ids.contains(&full(id)) => extend(out, [full(id)]),
                None => {}
            }
        }
        Ok(())
    }
}

/// Appends the ids `out` lacks.
fn extend(out: &mut Vec<String>, ids: impl IntoIterator<Item = String>) {
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
}

/// Applies `modifiers` to the biome bundle `biomes` (id → the biome's text):
/// every addition first, then every removal, each in mod and file order (as
/// NeoForge's add phase runs before its remove phase). A modifier naming a
/// biome, feature, entity type or tag the root doesn't have (another mod's, a
/// `#c:` convention tag) is skipped with a line in `warnings`, as NeoForge
/// skips a holder set it can't resolve, instead of dropping every content mod.
///
/// A translated mod's own modifiers run in each of NeoForge's five phases
/// ([`foreign::PHASES`]) on every biome, after the files of that phase.
pub(super) fn apply(
    biomes: &mut J,
    modifiers: &[Modifier],
    biome_reg: &Registry<'_>,
    feature_reg: &Registry<'_>,
    entity_reg: &Registry<'_>,
    entity_categories: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> Result<Vec<(String, String)>> {
    let code: Vec<&Modifier> = modifiers.iter().filter(|m| m.code.is_some()).collect();
    let mut own = own_types::Pass::new(&code, biome_reg, feature_reg, entity_categories);
    // Each biome changed, parsed once, in the bundle's order.
    let mut changed: BTreeMap<String, J> = BTreeMap::new();
    own.phase(0, biomes, &mut changed)?;
    for add in [true, false] {
    for m in modifiers
        .iter()
        .filter(|m| m.code.is_none() && m.action.is_add() == add)
    {
        let at = |e: String| format!("{}: {e}", m.name);
        let resolved: std::result::Result<(Vec<String>, Vec<String>), String> =
            biome_reg.resolve(&m.biomes).and_then(|targets| {
                let ids = match &m.action {
                    Action::AddFeatures { features, .. }
                    | Action::RemoveFeatures { features, .. } => feature_reg.resolve(features)?,
                    Action::AddSpawns(spawners) => {
                        if let Some(s) = spawners
                            .iter()
                            .find(|s| !entity_reg.ids.contains(&s.entity))
                        {
                            return Err(format!("unknown {} {}", entity_reg.what, s.entity));
                        }
                        Vec::new()
                    }
                    Action::RemoveSpawns(entity_types)
                    | Action::AddSpawnCosts { entity_types, .. }
                    | Action::RemoveSpawnCosts(entity_types) => entity_reg.resolve(entity_types)?,
                    Action::Code => Vec::new(),
                };
                Ok((targets, ids))
            });
        let (targets, ids) = match resolved {
            Ok(both) => both,
            Err(e) => {
                warnings.push(at(format!("{e}: skipped")));
                continue;
            }
        };
        if matches!(&m.action, Action::AddSpawns(s) if s.is_empty()) {
            continue;
        }
        for biome in targets {
            let doc = match changed.entry(biome.clone()) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => {
                    let Some(J::Str(text)) = biomes.get(&biome) else {
                        continue;
                    };
                    e.insert(serde_json::from_str(text).map_err(|e| at(format!("{biome}: {e}")))?)
                }
            };
            edit(doc, &m.action, &ids, entity_categories)
                .map_err(|e| at(format!("{biome}: {e}")))?;
        }
    }
        // The add phase, then the remove phase.
        own.phase(if add { 1 } else { 2 }, biomes, &mut changed)?;
    }
    own.phase(3, biomes, &mut changed)?;
    own.phase(4, biomes, &mut changed)?;
    warnings.extend(own.notes);
    let mut out = Vec::with_capacity(changed.len());
    for (id, doc) in changed {
        let text = serde_json::to_string(&doc)?;
        biomes.set(&id, J::Str(text.clone()));
        out.push((id, text));
    }
    Ok(out)
}

/// One modifier's change to one biome.
fn edit(
    biome: &mut J,
    action: &Action,
    ids: &[String],
    entity_categories: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    match action {
        Action::AddFeatures { step, .. } => {
            edit_features(biome, std::slice::from_ref(step), true, ids)
        }
        Action::RemoveFeatures { steps, .. } => edit_features(biome, steps, false, ids),
        Action::AddSpawns(spawners) => add_spawns(biome, spawners, entity_categories),
        Action::RemoveSpawns(_) => remove_spawns(biome, ids),
        Action::AddSpawnCosts {
            energy_budget,
            charge,
            ..
        } => add_spawn_costs(biome, ids, energy_budget, charge),
        Action::RemoveSpawnCosts(_) => remove_spawn_costs(biome, ids),
        // (Run by `own_types::Pass`, never here.)
        Action::Code => Ok(()),
    }
}

/// One feature modifier's change to one biome's `features` (a list per step).
fn edit_features(
    biome: &mut J,
    wanted: &[usize],
    add: bool,
    features: &[String],
) -> std::result::Result<(), String> {
    if biome.get("features").is_none() {
        biome.set("features", J::Arr(Vec::new()));
    }
    let Some(J::Arr(steps)) = biome.get_mut("features") else {
        return Err("features: not a list of steps".into());
    };
    if add && let Some(&step) = wanted.first() {
        while steps.len() <= step {
            steps.push(J::Arr(Vec::new()));
        }
    }
    for &i in wanted {
        let Some(list) = steps.get_mut(i) else {
            continue;
        };
        // A step may be one id; a tag cannot take more features.
        if let J::Str(one) = list {
            if one.starts_with('#') {
                return Err(format!("step {} is the feature tag {one}", STEPS[i]));
            }
            *list = J::Arr(vec![J::Str(one.clone())]);
        }
        let J::Arr(list) = list else {
            return Err(format!("step {}: not a list", STEPS[i]));
        };
        let held = |v: &J| match v {
            J::Str(s) => Some(full(s)),
            _ => None,
        };
        if add {
            for f in features {
                // Twice in one step would be a cycle for the sorter.
                if !list.iter().any(|v| held(v).as_ref() == Some(f)) {
                    list.push(J::Str(f.clone()));
                }
            }
        } else {
            list.retain(|v| held(v).is_none_or(|id| !features.contains(&id)));
        }
    }
    Ok(())
}

/// The `minecraft:gameplay/natural_mob_spawns` argument object on `biome`,
/// creating it when the biome does not yet have one.
fn natural_spawns(biome: &mut J) -> std::result::Result<&mut J, String> {
    if biome.get("attributes").is_none() {
        biome.set("attributes", J::Obj(Vec::new()));
    }
    let Some(attrs @ J::Obj(_)) = biome.get_mut("attributes") else {
        return Err("attributes: not an object".into());
    };
    const KEY: &str = "minecraft:gameplay/natural_mob_spawns";
    if attrs.get(KEY).is_none() {
        attrs.set(
            KEY,
            J::Obj(vec![
                (
                    "argument".into(),
                    J::Obj(vec![
                        ("spawn_costs".into(), J::Obj(Vec::new())),
                        ("spawns_by_category".into(), J::Obj(Vec::new())),
                    ]),
                ),
                ("modifier".into(), J::Str("overlay".into())),
            ]),
        );
    }
    let Some(nat) = attrs.get_mut(KEY) else {
        unreachable!()
    };
    if nat.get("argument").is_some() {
        match nat.get_mut("argument") {
            Some(arg @ J::Obj(_)) => Ok(arg),
            _ => Err(format!("{KEY}.argument: not an object")),
        }
    } else {
        match nat {
            J::Obj(_) => Ok(nat),
            _ => Err(format!("{KEY}: not an object")),
        }
    }
}

fn add_spawns(
    biome: &mut J,
    spawners: &[Spawner],
    entity_categories: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    let nat = natural_spawns(biome)?;
    if nat.get("spawns_by_category").is_none() {
        nat.set("spawns_by_category", J::Obj(Vec::new()));
    }
    let Some(by_cat @ J::Obj(_)) = nat.get_mut("spawns_by_category") else {
        return Err("spawns_by_category: not an object".into());
    };
    for s in spawners {
        let cat = entity_categories
            .get(&s.entity)
            .map_or("misc", String::as_str);
        if by_cat.get(cat).is_none() {
            by_cat.set(cat, J::Arr(Vec::new()));
        }
        let Some(J::Arr(list)) = by_cat.get_mut(cat) else {
            return Err(format!("spawns_by_category.{cat}: not a list"));
        };
        let count = if s.min_count == s.max_count {
            J::Num(s.min_count.into())
        } else {
            J::Obj(vec![
                ("type".into(), J::Str("minecraft:uniform".into())),
                ("min_inclusive".into(), J::Num(s.min_count.into())),
                ("max_inclusive".into(), J::Num(s.max_count.into())),
            ])
        };
        list.push(J::Obj(vec![
            ("type".into(), J::Str(s.entity.clone())),
            ("count".into(), count),
            ("weight".into(), J::Num(s.weight.into())),
        ]));
    }
    Ok(())
}

fn remove_spawns(biome: &mut J, entities: &[String]) -> std::result::Result<(), String> {
    let Some(nat) = biome
        .get_mut("attributes")
        .and_then(|a| a.get_mut("minecraft:gameplay/natural_mob_spawns"))
    else {
        return Ok(());
    };
    let nat = if nat.get("argument").is_some() {
        nat.get_mut("argument").unwrap()
    } else {
        nat
    };
    let Some(by_cat) = nat.get_mut("spawns_by_category") else {
        return Ok(());
    };
    let J::Obj(cats) = by_cat else {
        return Err("spawns_by_category: not an object".into());
    };
    for (cat, list) in cats {
        let J::Arr(entries) = list else {
            return Err(format!("spawns_by_category.{cat}: not a list"));
        };
        entries.retain(|entry| {
            entry
                .get("type")
                .and_then(J::as_str)
                .is_none_or(|t| !entities.contains(&full(t)))
        });
    }
    Ok(())
}

fn add_spawn_costs(
    biome: &mut J,
    entities: &[String],
    energy_budget: &Number,
    charge: &Number,
) -> std::result::Result<(), String> {
    let nat = natural_spawns(biome)?;
    if nat.get("spawn_costs").is_none() {
        nat.set("spawn_costs", J::Obj(Vec::new()));
    }
    let Some(costs @ J::Obj(_)) = nat.get_mut("spawn_costs") else {
        return Err("spawn_costs: not an object".into());
    };
    for entity in entities {
        if let J::Obj(entries) = costs {
            entries.retain(|(k, _)| k == entity || full(k) != *entity);
        }
        costs.set(
            entity,
            J::Obj(vec![
                ("energy_budget".into(), J::Num(energy_budget.clone())),
                ("charge".into(), J::Num(charge.clone())),
            ]),
        );
    }
    Ok(())
}

fn remove_spawn_costs(biome: &mut J, entities: &[String]) -> std::result::Result<(), String> {
    let Some(nat) = biome
        .get_mut("attributes")
        .and_then(|a| a.get_mut("minecraft:gameplay/natural_mob_spawns"))
    else {
        return Ok(());
    };
    let nat = if nat.get("argument").is_some() {
        nat.get_mut("argument").unwrap()
    } else {
        nat
    };
    let Some(costs) = nat.get_mut("spawn_costs") else {
        return Ok(());
    };
    let J::Obj(entries) = costs else {
        return Err("spawn_costs: not an object".into());
    };
    entries.retain(|(k, _)| !entities.contains(&full(k)));
    Ok(())
}

/// A translated mod's own biome modifiers: its code edits a biome's placed
/// features and mob spawns as NeoForge's builder holds them, and what it
/// changed is written back into the biome's 26.3 form.
mod own_types {
    use super::{Code, Modifier, Registry, full};
    use crate::mod_content::{J, Result};
    use horizon_world::worldgen::foreign::{self, Spawner, Spawns, StepFeatures};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};

    /// The mods' modifiers over a root's biomes, phase by phase.
    pub(super) struct Pass<'a> {
        code: &'a [&'a Modifier],
        biome_reg: &'a Registry<'a>,
        feature_reg: &'a Registry<'a>,
        /// Each entity type's category by its serialized name (`creature`).
        categories: &'a BTreeMap<String, String>,
        /// The biomes parsed for the mods' code that nothing changed yet.
        seen: BTreeMap<String, J>,
        /// The biomes whose text doesn't parse: skipped.
        unread: BTreeSet<String>,
        /// What the root's warnings say of it, each line once.
        pub(super) notes: Vec<String>,
    }

    impl<'a> Pass<'a> {
        pub(super) fn new(
            code: &'a [&'a Modifier],
            biome_reg: &'a Registry<'a>,
            feature_reg: &'a Registry<'a>,
            categories: &'a BTreeMap<String, String>,
        ) -> Self {
            Self {
                code,
                biome_reg,
                feature_reg,
                categories,
                seen: BTreeMap::new(),
                unread: BTreeSet::new(),
                notes: Vec::new(),
            }
        }

        /// Phase `phase` (an index of [`foreign::PHASES`]) of every modifier a mod's code runs,
        /// on every biome of the bundle `biomes`: a mod's code picks its biomes itself. A biome
        /// it changes joins `changed`.
        pub(super) fn phase(
            &mut self,
            phase: usize,
            biomes: &J,
            changed: &mut BTreeMap<String, J>,
        ) -> Result<()> {
            let J::Obj(all) = biomes else {
                return Ok(());
            };
            if self.code.is_empty() {
                return Ok(());
            }
            let (biome_reg, feature_reg, categories) =
                (self.biome_reg, self.feature_reg, self.categories);
            let root = Root {
                biomes,
                biome_reg,
                feature_reg,
                errors: RefCell::new(Vec::new()),
            };
            let notes = &mut self.notes;
            let mut note = |line: String| {
                if !notes.contains(&line) {
                    notes.push(line);
                }
            };
            for (id, text) in all {
                let J::Str(text) = text else { continue };
                if self.unread.contains(id) {
                    continue;
                }
                // The biome as the modifiers before left it.
                let (mut doc, was_changed) = match (changed.remove(id), self.seen.remove(id)) {
                    (Some(doc), _) => (doc, true),
                    (None, Some(doc)) => (doc, false),
                    (None, None) => match serde_json::from_str::<J>(text) {
                        Ok(doc) => (doc, false),
                        Err(e) => {
                            self.unread.insert(id.clone());
                            note(format!(
                                "biome {id}: {e}: the mods' biome modifier code skips it"
                            ));
                            continue;
                        }
                    },
                };
                let mut touched = false;
                for m in self.code {
                    let Some(Code(code)) = &m.code else { continue };
                    let at = |e: String| format!("{}: {id}: {e}", m.name);
                    let before = match steps_of(&doc, feature_reg) {
                        Ok(steps) => steps,
                        Err(e) => {
                            note(at(format!("{e}: the mod's code skips this biome")));
                            continue;
                        }
                    };
                    let kept = spawns_of(&doc);
                    let (mut after, mut now) = (before.clone(), kept.clone());
                    let result = code.modify(&root, id, phase, &mut after, &mut now);
                    for e in root.errors.take() {
                        note(format!("{}: {e}: the mod's code finds it empty", m.name));
                    }
                    if let Err(e) = result {
                        note(at(e));
                        continue;
                    }
                    // A spawner or cost of an entity type the root doesn't have (another
                    // mod's) would name nothing: left out.
                    let known = |t: &String| {
                        categories.contains_key(t)
                            || kept.costs.contains_key(t)
                            || kept.categories.values().flatten().any(|s| s.entity == *t)
                    };
                    for list in now.categories.values_mut() {
                        list.retain(|s| {
                            let keep = known(&s.entity);
                            if !keep {
                                note(format!(
                                    "{}: unknown entity type {}: not added",
                                    m.name, s.entity
                                ));
                            }
                            keep
                        });
                    }
                    now.costs.retain(|t, _| known(t));
                    // Lists the builder grew for nothing are left out.
                    while after.len() > before.len() && after.last().is_some_and(Vec::is_empty) {
                        after.pop();
                    }
                    // A placed feature the root doesn't have (one that didn't port) would
                    // stop the generator: left out, as NeoForge's codec refuses one it lacks.
                    for list in &mut after {
                        list.retain(|e| {
                            let keep = e.starts_with('{')
                                || feature_reg.ids.contains(e)
                                || before.iter().any(|l| l.contains(e));
                            if !keep {
                                note(format!(
                                    "{}: unknown placed feature {e}: not added",
                                    m.name
                                ));
                            }
                            keep
                        });
                    }
                    if after != before {
                        set_steps(&mut doc, &before, &after).map_err(at)?;
                        touched = true;
                    }
                    if !same_spawns(&kept, &now) {
                        set_spawns(&mut doc, &kept, &now).map_err(at)?;
                        touched = true;
                    }
                }
                match was_changed || touched {
                    true => changed.insert(id.clone(), doc),
                    false => self.seen.insert(id.clone(), doc),
                };
            }
            Ok(())
        }
    }

    /// The 26.3 attributes a biome's mob spawns are: its spawners and costs, and
    /// the chance of each creature pack chunk generation places.
    const NATURAL: &str = "minecraft:gameplay/natural_mob_spawns";
    const PROBABILITY: &str = "minecraft:gameplay/creature_world_gen_spawn_probability";

    /// The value of a biome's `natural_mob_spawns`: an `overlay` modifier's
    /// argument (over the dimension's empty default), or the plain value.
    fn natural(biome: &J) -> Option<&J> {
        let v = biome.get("attributes")?.get(NATURAL)?;
        match v.get("argument") {
            Some(argument @ J::Obj(_)) => Some(argument),
            _ => Some(v),
        }
    }

    /// A 26.3 spawn entry's spawner (`{type, count, weight}`), read as the game
    /// reads it: a count is a number, a `constant` or a `uniform` range.
    fn entry_spawner(e: &J) -> Option<Spawner> {
        let entity = match e.get("type")? {
            J::Str(s) => full(s),
            _ => return None,
        };
        let int = |v: Option<&J>| v.and_then(J::as_u32).unwrap_or(1);
        let (min, max) = match e.get("count") {
            Some(n @ J::Num(_)) => (int(Some(n)), int(Some(n))),
            Some(c) if c.get("type") == Some(&J::Str("minecraft:constant".into())) => {
                (int(c.get("value")), int(c.get("value")))
            }
            c => (
                int(c.and_then(|c| c.get("min_inclusive"))),
                int(c.and_then(|c| c.get("max_inclusive"))),
            ),
        };
        Some(Spawner {
            entity,
            weight: int(e.get("weight")),
            min,
            max,
        })
    }

    /// A biome's mob spawns as NeoForge's builder holds them, from its 26.3
    /// attributes.
    fn spawns_of(biome: &J) -> Spawns {
        let number = |v: Option<&J>| match v {
            Some(J::Num(n)) => n.as_f64(),
            _ => None,
        };
        let mut spawns = Spawns {
            probability: number(biome.get("attributes").and_then(|a| a.get(PROBABILITY))).unwrap_or(0.1) as f32,
            ..Spawns::default()
        };
        let Some(natural) = natural(biome) else {
            return spawns;
        };
        if let Some(J::Obj(categories)) = natural.get("spawns_by_category") {
            for (category, list) in categories {
                let list = match list {
                    J::Arr(list) => list.iter().filter_map(entry_spawner).collect(),
                    _ => Vec::new(),
                };
                spawns.categories.insert(category.clone(), list);
            }
        }
        if let Some(J::Obj(costs)) = natural.get("spawn_costs") {
            for (entity, cost) in costs {
                let value = |key: &str| number(cost.get(key)).unwrap_or(0.0);
                spawns.costs.insert(full(entity), (value("energy_budget"), value("charge")));
            }
        }
        spawns
    }

    /// Whether two biomes' mob spawns are the same; a category left out has none.
    fn same_spawns(a: &Spawns, b: &Spawns) -> bool {
        let none = Vec::new();
        a.probability == b.probability
            && a.costs == b.costs
            && a.categories
                .keys()
                .chain(b.categories.keys())
                .all(|c| a.categories.get(c).unwrap_or(&none) == b.categories.get(c).unwrap_or(&none))
    }

    /// A new spawn entry in 26.3's form: a plain count where the pack's size is
    /// one value (as 26.3's upgrade writes a 1.21 spawner), else a `uniform`.
    fn spawn_entry(s: &Spawner) -> J {
        let count = match s.min == s.max {
            true => J::Num(s.min.into()),
            false => J::Obj(vec![
                ("type".into(), J::Str("minecraft:uniform".into())),
                ("max_inclusive".into(), J::Num(s.max.into())),
                ("min_inclusive".into(), J::Num(s.min.into())),
            ]),
        };
        J::Obj(vec![
            ("type".into(), J::Str(s.entity.clone())),
            ("count".into(), count),
            ("weight".into(), J::Num(s.weight.into())),
        ])
    }

    /// Writes what the modifiers changed of a biome's mob spawns into its 26.3
    /// attributes: `natural_mob_spawns` (an `overlay` over the dimension's empty
    /// default, as vanilla's biomes hold it) and the creature probability. A
    /// category or cost they left alone keeps its text, and so does each entry
    /// the lists still hold (a one-value `uniform` count still draws).
    fn set_spawns(biome: &mut J, before: &Spawns, after: &Spawns) -> std::result::Result<(), String> {
        if biome.get("attributes").is_none() {
            biome.set("attributes", J::Obj(Vec::new()));
        }
        let Some(attributes @ J::Obj(_)) = biome.get_mut("attributes") else {
            return Err("attributes: not an object".into());
        };
        if after.probability != before.probability {
            // The float as written (0.07), not its widened bits.
            let p = after.probability.to_string().parse().unwrap_or(f64::from(after.probability));
            attributes.set(PROBABILITY, J::f64(p));
        }
        let none = Vec::new();
        let names: BTreeSet<&String> = before.categories.keys().chain(after.categories.keys()).collect();
        let changed: Vec<&String> = names
            .into_iter()
            .filter(|c| before.categories.get(*c).unwrap_or(&none) != after.categories.get(*c).unwrap_or(&none))
            .collect();
        if changed.is_empty() && before.costs == after.costs {
            return Ok(());
        }
        if attributes.get(NATURAL).is_none() {
            let empty = || J::Obj(Vec::new());
            attributes.set(
                NATURAL,
                J::Obj(vec![
                    (
                        "argument".into(),
                        J::Obj(vec![("spawn_costs".into(), empty()), ("spawns_by_category".into(), empty())]),
                    ),
                    ("modifier".into(), J::Str("overlay".into())),
                ]),
            );
        }
        let natural = attributes.get_mut(NATURAL).ok_or("no natural_mob_spawns")?;
        let value = match matches!(natural.get("argument"), Some(J::Obj(_))) {
            true => natural.get_mut("argument").ok_or("natural_mob_spawns: no argument")?,
            false => natural,
        };
        if !changed.is_empty() {
            if value.get("spawns_by_category").is_none() {
                value.set("spawns_by_category", J::Obj(Vec::new()));
            }
            let Some(categories @ J::Obj(_)) = value.get_mut("spawns_by_category") else {
                return Err("spawns_by_category: not an object".into());
            };
            for c in changed {
                let old = match categories.get(c) {
                    Some(J::Arr(list)) => list.clone(),
                    _ => Vec::new(),
                };
                let mut used = vec![false; old.len()];
                let entries = after
                    .categories
                    .get(c)
                    .unwrap_or(&none)
                    .iter()
                    .map(|s| match (0..old.len()).find(|&i| !used[i] && entry_spawner(&old[i]).as_ref() == Some(s)) {
                        Some(i) => {
                            used[i] = true;
                            old[i].clone()
                        }
                        None => spawn_entry(s),
                    })
                    .collect();
                categories.set(c, J::Arr(entries));
            }
        }
        if before.costs != after.costs {
            if value.get("spawn_costs").is_none() {
                value.set("spawn_costs", J::Obj(Vec::new()));
            }
            let Some(J::Obj(costs)) = value.get_mut("spawn_costs") else {
                return Err("spawn_costs: not an object".into());
            };
            let cost = |(budget, charge): (f64, f64)| {
                J::Obj(vec![("charge".into(), J::f64(charge)), ("energy_budget".into(), J::f64(budget))])
            };
            let mut out = Vec::new();
            for (key, v) in std::mem::take(costs) {
                let id = full(&key);
                if let Some(&now) = after.costs.get(&id) {
                    let kept = before.costs.get(&id) == Some(&now);
                    out.push((key, if kept { v } else { cost(now) }));
                }
            }
            for (id, &now) in &after.costs {
                if !out.iter().any(|(key, _)| full(key) == *id) {
                    out.push((id.clone(), cost(now)));
                }
            }
            *costs = out;
        }
        Ok(())
    }

    /// The root's biomes, their tags and its placed features, as a mod's biome
    /// modifier code asks for them. Sets it can't resolve are noted in `errors`.
    struct Root<'a> {
        biomes: &'a J,
        biome_reg: &'a Registry<'a>,
        feature_reg: &'a Registry<'a>,
        errors: RefCell<Vec<String>>,
    }

    impl foreign::Biomes for Root<'_> {
        fn resolve(&self, registry: &str, set: &str) -> std::result::Result<Vec<String>, String> {
            let name = registry.strip_prefix("minecraft:").unwrap_or(registry);
            let reg = match name.strip_prefix("worldgen/").unwrap_or(name) {
                "biome" => self.biome_reg,
                "placed_feature" => self.feature_reg,
                _ => return Err(format!("biome modifiers read no {registry} registry")),
            };
            let resolved = serde_json::from_str(set)
                .map_err(|e| format!("{set}: {e}"))
                .and_then(|set| reg.resolve(&set));
            if let Err(e) = &resolved {
                let mut errors = self.errors.borrow_mut();
                if !errors.contains(e) {
                    errors.push(e.clone());
                }
            }
            resolved
        }

        fn biome(&self, id: &str) -> Option<String> {
            match self.biomes.get(&full(id)) {
                Some(J::Str(text)) => Some(text.clone()),
                _ => None,
            }
        }
    }

    /// A biome's placed features by step, as NeoForge's builder holds them: ids,
    /// a `#tag` step's members, and inline features as their text.
    fn steps_of(biome: &J, feature_reg: &Registry<'_>) -> std::result::Result<StepFeatures, String> {
        let entry = |v: &J| match v {
            J::Str(id) => Ok(full(id)),
            other => serde_json::to_string(other).map_err(|e| e.to_string()),
        };
        match biome.get("features") {
            None => Ok(Vec::new()),
            Some(J::Arr(steps)) => steps
                .iter()
                .map(|step| -> std::result::Result<Vec<String>, String> {
                    match step {
                        J::Arr(list) => list.iter().map(entry).collect(),
                        J::Str(s) if s.starts_with('#') => {
                            let mut out = Vec::new();
                            feature_reg.expand(&full(&s[1..]), &mut out, &mut BTreeSet::new())?;
                            Ok(out)
                        }
                        one => Ok(vec![entry(one)?]),
                    }
                })
                .collect(),
            Some(_) => Err("features: not a list of steps".into()),
        }
    }

    /// Writes the steps a mod's code changed into the biome's `features`; the
    /// others keep their text.
    fn set_steps(
        biome: &mut J,
        before: &StepFeatures,
        after: &StepFeatures,
    ) -> std::result::Result<(), String> {
        if biome.get("features").is_none() {
            biome.set("features", J::Arr(Vec::new()));
        }
        let Some(J::Arr(steps)) = biome.get_mut("features") else {
            return Err("features: not a list of steps".into());
        };
        for (i, list) in after.iter().enumerate() {
            if before.get(i) == Some(list) {
                continue;
            }
            let entries = list
                .iter()
                .map(|e| match e.starts_with('{') {
                    true => serde_json::from_str(e).map_err(|err| format!("{e}: {err}")),
                    false => Ok(J::Str(e.clone())),
                })
                .collect::<std::result::Result<Vec<J>, String>>()?;
            while steps.len() <= i {
                steps.push(J::Arr(Vec::new()));
            }
            steps[i] = J::Arr(entries);
        }
        Ok(())
    }
}

#[cfg(test)]
mod own_type_tests {
    use super::*;
    use horizon_world::worldgen::foreign::{
        Biomes, Handler, PHASES, Placement, Spawner, Spawns, StepFeatures,
    };
    use serde_json::json;

    /// A mod's own modifier type, as its code runs it (Farmer's Delight's
    /// `add_features_by_filter`): its features, in the add phase, into the
    /// biomes of one set and not of another that are warm enough.
    struct WarmPatch {
        allowed: String,
        denied: String,
        min: f64,
        step: usize,
        features: Vec<String>,
    }

    impl BiomeModifier for WarmPatch {
        fn modify(
            &self,
            biomes: &dyn Biomes,
            biome: &str,
            phase: usize,
            features: &mut StepFeatures,
            _: &mut Spawns,
        ) -> std::result::Result<(), String> {
            if PHASES[phase] != "ADD" {
                return Ok(());
            }
            // A set the root can't resolve is empty to the mod's code.
            let has = |set: &str| biomes.resolve("biome", set).unwrap_or_default().iter().any(|b| b == biome);
            let text = biomes.biome(biome).ok_or("no such biome")?;
            let warm = serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?["temperature"]
                .as_f64()
                .is_some_and(|t| t >= self.min);
            if has(&self.allowed) && !has(&self.denied) && warm {
                while features.len() <= self.step {
                    features.push(Vec::new());
                }
                features[self.step].extend(self.features.iter().cloned());
            }
            Ok(())
        }
    }

    /// The translated mod: the codec of its one modifier type.
    struct WarmMod;

    impl Handler for WarmMod {
        fn placement(&self, _: &str, _: &str) -> Option<std::result::Result<Arc<dyn Placement>, String>> {
            None
        }

        fn biome_modifier(
            &self,
            kind: &str,
            json: &str,
        ) -> Option<std::result::Result<Arc<dyn BiomeModifier>, String>> {
            if kind != "test:warm_patch" {
                return None;
            }
            let v: Value = serde_json::from_str(json).ok()?;
            let read = || -> std::result::Result<Arc<dyn BiomeModifier>, String> {
                Ok(Arc::new(WarmPatch {
                    allowed: v["allowed_biomes"].to_string(),
                    denied: v["denied_biomes"].to_string(),
                    min: v["min_temperature"].as_f64().ok_or("No key min_temperature")?,
                    step: step(&v["step"])?,
                    features: v["features"].as_array().ok_or("No key features")?.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
                }))
            };
            Some(read())
        }
    }

    #[test]
    fn a_mods_own_modifier_type_runs_its_code_between_neoforges_phases() {
        foreign::register("mod_content_biomes::tests", Arc::new(WarmMod));
        let biome = |t: f64, features: &str| J::Str(format!(r#"{{"temperature": {t}, "features": {features}}}"#));
        let mut biomes = J::Obj(vec![
            ("minecraft:desert".into(), biome(2.0, r#"[[], ["minecraft:lake"]]"#)),
            ("minecraft:badlands".into(), biome(2.0, r##"[[], "#test:lakes"]"##)),
            ("minecraft:plains".into(), biome(0.8, r#"[[], ["minecraft:lake"]]"#)),
            ("minecraft:snowy_plains".into(), biome(0.0, "[]")),
        ]);
        let vanilla = biomes.clone();
        let ids: BTreeSet<String> = ["minecraft:desert", "minecraft:badlands", "minecraft:plains", "minecraft:snowy_plains"].map(String::from).into();
        let biome_tags = BTreeMap::from([("test:warm".to_owned(), r#"{"values": ["desert", "badlands", "plains"]}"#.to_owned())]);
        let placed: BTreeSet<String> = ["minecraft:lake", "test:patch", "test:rock"].map(String::from).into();
        let feature_tags = BTreeMap::from([("test:lakes".to_owned(), r#"{"values": ["minecraft:lake"]}"#.to_owned())]);
        let mut warnings = Vec::new();
        let files = [
            // First in file order, yet NeoForge's remove phase runs after the add phase.
            ("a", "no_rock", r#"{"type": "neoforge:remove_features", "biomes": ["desert", "badlands"], "features": "test:rock", "steps": "vegetal_decoration"}"#),
            ("b", "warm", r##"{"type": "test:warm_patch", "allowed_biomes": "#test:warm", "denied_biomes": "#c:is_cold", "min_temperature": 1.0, "features": ["test:patch", "test:rock", "test:unported"], "step": "vegetal_decoration"}"##),
            ("b", "no_step", r##"{"type": "test:warm_patch", "allowed_biomes": "#test:warm", "features": []}"##),
        ];
        let mut modifiers = Vec::new();
        for (m, name, text) in files {
            let file = format!("data/{m}/neoforge/biome_modifier/{name}.json");
            modifiers.extend(Modifier::read(m, &file, text, &mut warnings).unwrap());
        }
        assert_eq!(modifiers.len(), 2, "{warnings:?}");
        let biome_reg = Registry { what: "biome", ids: &ids, tags: &biome_tags };
        let feature_reg = Registry { what: "placed feature", ids: &placed, tags: &feature_tags };
        let (types, type_tags, categories) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new());
        let entity_reg = Registry { what: "entity type", ids: &types, tags: &type_tags };
        apply(&mut biomes, &modifiers, &biome_reg, &feature_reg, &entity_reg, &categories, &mut warnings).unwrap();

        let features = |id: &str| match biomes.get(id) {
            Some(J::Str(text)) => serde_json::from_str::<Value>(text).unwrap()["features"].clone(),
            other => panic!("{id}: {other:?}"),
        };
        assert_eq!(features("minecraft:desert"), json!([[], ["minecraft:lake"], [], [], [], [], [], [], [], ["test:patch"]]));
        // The tag step the code left alone keeps its text.
        assert_eq!(features("minecraft:badlands"), json!([[], "#test:lakes", [], [], [], [], [], [], [], ["test:patch"]]));
        // Too cold, and not in the set: as they were, byte for byte.
        for id in ["minecraft:plains", "minecraft:snowy_plains"] {
            assert_eq!(biomes.get(id), vanilla.get(id), "{id}");
        }
        let said = |what: &str| warnings.iter().filter(|w| w.contains(what)).count();
        assert_eq!(said("no_step.json: biome modifier type \"test:warm_patch\" skipped: the mod's codec refused it"), 1, "{warnings:?}");
        assert_eq!(said("warm.json: unknown biome tag #c:is_cold: the mod's code finds it empty"), 1, "{warnings:?}");
        assert_eq!(said("warm.json: unknown placed feature test:unported: not added"), 1, "{warnings:?}");
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    /// A mod's own spawn modifier (Waddles' `penguin_spawn`), as its code runs
    /// it: in the add phase, a pack of its mob in the cold biomes, and creature
    /// packs more often there; one entry names a mob no mod registers.
    struct ColdPack;

    impl BiomeModifier for ColdPack {
        fn modify(
            &self,
            biomes: &dyn Biomes,
            biome: &str,
            phase: usize,
            _: &mut StepFeatures,
            spawns: &mut Spawns,
        ) -> std::result::Result<(), String> {
            let text = biomes.biome(biome).ok_or("no such biome")?;
            let cold = serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?["temperature"].as_f64() <= Some(0.0);
            if PHASES[phase] == "ADD" && cold {
                let creature = spawns.categories.entry("creature".into()).or_default();
                creature.push(Spawner { entity: "test:penguin".into(), weight: 2, min: 1, max: 1 });
                creature.push(Spawner { entity: "other:seal".into(), weight: 1, min: 1, max: 2 });
                spawns.probability = 0.2;
            }
            Ok(())
        }
    }

    struct ColdMod;

    impl Handler for ColdMod {
        fn placement(&self, _: &str, _: &str) -> Option<std::result::Result<Arc<dyn Placement>, String>> {
            None
        }

        fn biome_modifier(&self, kind: &str, _: &str) -> Option<std::result::Result<Arc<dyn BiomeModifier>, String>> {
            (kind == "test:cold_pack").then(|| Ok(Arc::new(ColdPack) as Arc<dyn BiomeModifier>))
        }
    }

    #[test]
    fn spawn_modifiers_file_spawners_under_their_types_category_and_keep_vanillas_entries() {
        foreign::register("mod_content_biomes::spawn_tests", Arc::new(ColdMod));
        let biome = |temperature: f64, spawns: Value, costs: Value, probability: f64| {
            let natural = json!({"argument": {"spawn_costs": costs, "spawns_by_category": spawns}, "modifier": "overlay"});
            J::Str(json!({"temperature": temperature, "attributes": {
                "minecraft:gameplay/creature_world_gen_spawn_probability": probability,
                "minecraft:gameplay/natural_mob_spawns": natural}}).to_string())
        };
        let rabbit = json!({"type": "minecraft:rabbit", "count": {"type": "minecraft:uniform", "max_inclusive": 3, "min_inclusive": 2}, "weight": 10});
        let bear = json!({"type": "minecraft:polar_bear", "count": {"type": "minecraft:uniform", "max_inclusive": 2, "min_inclusive": 1}, "weight": 1});
        let zombie = json!({"type": "minecraft:zombie", "count": 4, "weight": 95});
        let sheep = json!({"type": "minecraft:sheep", "count": 4, "weight": 12});
        let mut biomes = J::Obj(vec![
            ("minecraft:snowy_plains".into(), biome(0.0, json!({"creature": [rabbit, bear], "monster": [zombie]}), json!({}), 0.07)),
            ("minecraft:plains".into(), biome(0.8, json!({"creature": [sheep]}), json!({"minecraft:sheep": {"charge": 0.5, "energy_budget": 0.2}}), 0.1)),
            ("minecraft:desert".into(), biome(2.0, json!({"monster": [zombie]}), json!({}), 0.1)),
        ]);
        let vanilla = biomes.clone();
        let ids: BTreeSet<String> = ["minecraft:snowy_plains", "minecraft:plains", "minecraft:desert"].map(String::from).into();
        let categories: BTreeMap<String, String> = [
            ("minecraft:rabbit", "creature"),
            ("minecraft:polar_bear", "creature"),
            ("minecraft:sheep", "creature"),
            ("minecraft:zombie", "monster"),
            ("test:penguin", "creature"),
            ("test:yeti", "monster"),
        ]
        .map(|(t, c)| (t.to_owned(), c.to_owned()))
        .into();
        let types: BTreeSet<String> = categories.keys().cloned().collect();
        let type_tags = BTreeMap::from([("test:cold_mobs".to_owned(), r#"{"values": ["test:yeti", "polar_bear", "other:walrus"]}"#.to_owned())]);
        let mut warnings = Vec::new();
        let files = [
            ("a", "add", r#"{"type": "neoforge:add_spawns", "biomes": "snowy_plains", "spawners": [{"type": "test:penguin", "weight": 5, "minCount": 1, "maxCount": 4}, {"type": "test:yeti", "weight": 3, "minCount": 2, "maxCount": 2}]}"#),
            // First in file order, yet NeoForge's remove phase runs after the add phase.
            ("a", "remove", r##"{"type": "neoforge:remove_spawns", "biomes": "snowy_plains", "entity_types": "#test:cold_mobs"}"##),
            ("a", "cost", r#"{"type": "neoforge:add_spawn_costs", "biomes": ["snowy_plains"], "entity_types": "test:penguin", "spawn_cost": {"energy_budget": 0.12, "charge": 0.7}}"#),
            ("b", "no_cost", r#"{"type": "forge:remove_spawn_costs", "biomes": "plains", "entity_types": ["sheep"]}"#),
            ("c", "walrus", r#"{"type": "neoforge:add_spawns", "biomes": "plains", "spawners": {"type": "other:walrus", "weight": 1, "minCount": 1, "maxCount": 1}}"#),
            ("d", "cold", r#"{"type": "test:cold_pack"}"#),
        ];
        let mut modifiers = Vec::new();
        for (m, name, text) in files {
            let file = format!("data/{m}/neoforge/biome_modifier/{name}.json");
            modifiers.extend(Modifier::read(m, &file, text, &mut warnings).unwrap());
        }
        assert_eq!(modifiers.len(), 6, "{warnings:?}");
        let (no_ids, no_tags) = (BTreeSet::new(), BTreeMap::new());
        let biome_reg = Registry { what: "biome", ids: &ids, tags: &no_tags };
        let feature_reg = Registry { what: "placed feature", ids: &no_ids, tags: &no_tags };
        let entity_reg = Registry { what: "entity type", ids: &types, tags: &type_tags };
        apply(&mut biomes, &modifiers, &biome_reg, &feature_reg, &entity_reg, &categories, &mut warnings).unwrap();

        let attributes = |id: &str| match biomes.get(id) {
            Some(J::Str(text)) => serde_json::from_str::<Value>(text).unwrap()["attributes"].clone(),
            other => panic!("{id}: {other:?}"),
        };
        let snowy = attributes("minecraft:snowy_plains");
        let spawns = &snowy["minecraft:gameplay/natural_mob_spawns"]["argument"];
        // The rabbit keeps its own entry; the bear and the yeti went in the remove phase.
        assert_eq!(
            spawns["spawns_by_category"]["creature"],
            json!([
                {"type": "minecraft:rabbit", "count": {"type": "minecraft:uniform", "max_inclusive": 3, "min_inclusive": 2}, "weight": 10},
                {"type": "test:penguin", "count": {"type": "minecraft:uniform", "max_inclusive": 4, "min_inclusive": 1}, "weight": 5},
                {"type": "test:penguin", "count": 1, "weight": 2},
            ])
        );
        assert_eq!(spawns["spawns_by_category"]["monster"], json!([{"type": "minecraft:zombie", "count": 4, "weight": 95}]));
        assert_eq!(spawns["spawn_costs"], json!({"test:penguin": {"charge": 0.7, "energy_budget": 0.12}}));
        assert_eq!(snowy["minecraft:gameplay/creature_world_gen_spawn_probability"], json!(0.2));
        let plains = attributes("minecraft:plains");
        let spawns = &plains["minecraft:gameplay/natural_mob_spawns"]["argument"];
        assert_eq!(spawns["spawn_costs"], json!({}));
        assert_eq!(spawns["spawns_by_category"], json!({"creature": [{"type": "minecraft:sheep", "count": 4, "weight": 12}]}));
        // Not cold and named by none: as it was, byte for byte.
        assert_eq!(biomes.get("minecraft:desert"), vanilla.get("minecraft:desert"));
        let said = |what: &str| warnings.iter().filter(|w| w.contains(what)).count();
        assert_eq!(said("walrus.json: unknown entity type other:walrus: skipped"), 1, "{warnings:?}");
        assert_eq!(said("cold.json: unknown entity type other:seal: not added"), 1, "{warnings:?}");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }
}
