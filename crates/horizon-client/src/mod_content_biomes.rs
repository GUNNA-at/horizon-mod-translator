//! Biome modifiers: a content mod's placed features and mobs in biomes it
//! does not own (vanilla's), as NeoForge's `neoforge:add_features`,
//! `remove_features`, `add_spawns`, `remove_spawns`, `add_spawn_costs` and
//! `remove_spawn_costs` biome modifiers do. Each is a file in
//! `data/<ns>/neoforge/biome_modifier/` (older Forge mods:
//! `data/<ns>/forge/biome_modifier/`, type `forge:...`).
//!
//! They rewrite the biomes' `features` lists and their 26.3
//! `natural_mob_spawns` attribute when the content root builds: an added
//! feature goes at the end of its step's list, as NeoForge adds it, and the
//! generator orders every biome's lists as vanilla's `FeatureSorter` does; an
//! added spawner goes at the end of its type's category. Biomes no modifier
//! names keep their text byte for byte, and a root with no modifiers keeps
//! vanilla's bundle, so vanilla features keep their indices and their random
//! seeds; a spawn entry the lists still hold keeps its own text.
//!
//! A modifier of a translated mod's own type (Farmer's Delight's
//! `farmersdelight:add_features_by_filter`, Waddles' `penguin_spawn`) is read
//! by the mod's codec and run by its code (`BiomeModifier.modify`), biome by
//! biome in NeoForge's phase order, on the root's biomes, tags, placed
//! features and mob spawns.
use super::{J, Result};
use horizon_world::worldgen::foreign::{self, BiomeModifier, PHASES, Spawner, Spawns, StepFeatures};
use horizon_world::worldgen::structure::start::STEPS;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The folders a biome modifier sits in, under `data/<ns>`.
const FOLDERS: [&str; 2] = ["/neoforge/biome_modifier/", "/forge/biome_modifier/"];

/// Whether a mod file is a biome modifier.
pub(super) fn is_modifier(file: &str) -> bool {
    file.starts_with("data/") && file.ends_with(".json") && FOLDERS.iter().any(|f| file.contains(f))
}

/// What a modifier does to the features of its biomes.
#[derive(Debug, PartialEq)]
enum Action {
    /// Appends to one generation step.
    Add(usize),
    /// Takes out of these steps (every step when the file names none).
    Remove(Vec<usize>),
}

/// What a spawn modifier does to the mob spawns of its biomes.
#[derive(Debug, PartialEq)]
enum SpawnAction {
    /// `add_spawns` (add phase): each spawner joins the end of its type's
    /// category.
    Add(Vec<Spawner>),
    /// `remove_spawns` (remove phase): every category's spawners of these
    /// entity types go.
    Remove(Value),
    /// `add_spawn_costs` (add phase): these entity types' spawn cost, its
    /// energy budget and charge.
    AddCosts(Value, f64, f64),
    /// `remove_spawn_costs` (remove phase): these entity types' costs go.
    RemoveCosts(Value),
}

/// What a modifier file is: one of NeoForge's own types, read here, or a
/// translated mod's own, run by the mod's code.
enum Kind {
    Data {
        biomes: Value,
        features: Value,
        action: Action,
    },
    Spawns {
        biomes: Value,
        action: SpawnAction,
    },
    Code(Arc<dyn BiomeModifier>),
}

/// One biome modifier file.
pub(super) struct Modifier {
    /// `<mod>: <file>`, for errors.
    name: String,
    kind: Kind,
}

impl std::fmt::Debug for Modifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            Kind::Data { action, .. } => write!(f, "Modifier({}, {action:?})", self.name),
            Kind::Spawns { action, .. } => write!(f, "Modifier({}, {action:?})", self.name),
            Kind::Code(_) => write!(f, "Modifier({}, the mod's code)", self.name),
        }
    }
}

/// A generation step's index from its name.
fn step(v: &Value) -> std::result::Result<usize, String> {
    let name = v.as_str().ok_or("step: not a name")?;
    STEPS
        .iter()
        .position(|s| *s == name)
        .ok_or_else(|| format!("unknown generation step {name:?}"))
}

impl Modifier {
    /// The modifier in `text`; `None` for `neoforge:none`, which does nothing,
    /// and for a type Horizon doesn't read yet (`add_carvers`, a mod's own
    /// serializer no translated mod registers), which is skipped with a line
    /// in `warnings`: real mods ship those beside their feature modifiers, and
    /// one must not drop every content mod. A spawn modifier NeoForge's codec
    /// would refuse is skipped with its error, as is a translated mod's own
    /// type its codec refuses.
    pub(super) fn read(
        mod_id: &str,
        file: &str,
        text: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Option<Self>> {
        let name = format!("mod {mod_id}: {file}");
        let at = |e: String| format!("{name}: {e}");
        let doc: Value = serde_json::from_str(text).map_err(|e| at(e.to_string()))?;
        let full_kind = doc["type"].as_str().ok_or_else(|| at("no type".into()))?;
        let kind = full_kind
            .strip_prefix("neoforge:")
            .or_else(|| full_kind.strip_prefix("forge:"))
            .unwrap_or(full_kind);
        let field = |key: &str| -> std::result::Result<Value, String> {
            doc.get(key).cloned().ok_or_else(|| at(format!("no {key}")))
        };
        // The spawn modifiers, as NeoForge's codecs read them.
        let key = |key: &str| doc.get(key).cloned().ok_or_else(|| format!("No key {key}"));
        let spawns = match kind {
            "add_spawns" => Some(key("spawners").and_then(|v| spawners(&v)).map(SpawnAction::Add)),
            "remove_spawns" => Some(key("entity_types").map(SpawnAction::Remove)),
            "add_spawn_costs" => Some(key("entity_types").and_then(|types| {
                let cost = key("spawn_cost")?;
                let number = |k: &str| cost.get(k).and_then(Value::as_f64).ok_or(format!("spawn_cost: No key {k}"));
                Ok(SpawnAction::AddCosts(types, number("energy_budget")?, number("charge")?))
            })),
            "remove_spawn_costs" => Some(key("entity_types").map(SpawnAction::RemoveCosts)),
            _ => None,
        };
        if let Some(action) = spawns {
            return Ok(match action.and_then(|a| Ok((key("biomes")?, a))) {
                Ok((biomes, action)) => Some(Self {
                    name,
                    kind: Kind::Spawns { biomes, action },
                }),
                Err(e) => {
                    warnings.push(at(format!("{full_kind} skipped: {e}")));
                    None
                }
            });
        }
        let action = match kind {
            "none" => return Ok(None),
            "add_features" => Action::Add(step(&field("step")?).map_err(at)?),
            "remove_features" => Action::Remove(match doc.get("steps") {
                None => (0..STEPS.len()).collect(),
                Some(Value::Array(steps)) => steps
                    .iter()
                    .map(step)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(at)?,
                Some(one) => vec![step(one).map_err(at)?],
            }),
            other => {
                match foreign::biome_modifier(full_kind, text) {
                    Some(Ok(code)) => {
                        return Ok(Some(Self {
                            name,
                            kind: Kind::Code(code),
                        }));
                    }
                    Some(Err(e)) => warnings.push(at(format!(
                        "biome modifier type {full_kind:?} skipped: the mod's codec refused it: {e}"
                    ))),
                    None => warnings.push(at(format!(
                        "biome modifier type {other:?} skipped: only add_features, remove_features, \
                         add_spawns, remove_spawns, add_spawn_costs, remove_spawn_costs and none \
                         are read, and no translated mod registers it"
                    ))),
                }
                return Ok(None);
            }
        };
        Ok(Some(Self {
            kind: Kind::Data {
                biomes: field("biomes")?,
                features: field("features")?,
                action,
            },
            name,
        }))
    }
}

/// `add_spawns`' spawners: one, or a list (NeoForge's either codec), each as
/// `SpawnerData`'s codec reads it.
fn spawners(v: &Value) -> std::result::Result<Vec<Spawner>, String> {
    match v {
        Value::Array(list) => list.iter().map(spawner).collect(),
        one => Ok(vec![spawner(one)?]),
    }
}

/// A `SpawnerData`: its `type`, its `weight` (not negative), and its
/// `minCount` and `maxCount` (positive, the first not past the second).
fn spawner(v: &Value) -> std::result::Result<Spawner, String> {
    let entity = v.get("type").and_then(Value::as_str).ok_or("No key type")?;
    let int = |key: &str| {
        v.get(key)
            .and_then(Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or(format!("No key {key}"))
    };
    let weight = int("weight")?;
    if weight < 0 {
        return Err("Weight should be >= 0".into());
    }
    let (min, max) = (int("minCount")?, int("maxCount")?);
    if let Some(n) = [min, max].into_iter().find(|&n| n <= 0) {
        return Err(format!("Value must be positive: {n}"));
    }
    if min > max {
        return Err("minCount needs to be smaller or equal to maxCount".into());
    }
    Ok(Spawner {
        entity: full(entity),
        weight: weight as u32,
        min: min as u32,
        max: max as u32,
    })
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

/// The phases NeoForge's own types act in ([`PHASES`]).
const ADD: usize = 1;
const REMOVE: usize = 2;

/// The entity types spawn modifiers name: the root's ids and entity type
/// tags, and each type's category by its serialized name (`creature`).
pub(super) struct Entities<'a> {
    pub reg: Registry<'a>,
    pub categories: &'a BTreeMap<String, String>,
}

/// Applies `modifiers` to the biome bundle `biomes` (id → the biome's text),
/// biome by biome as NeoForge does: phase by phase ([`PHASES`]), every
/// modifier in mod and file order in each, so additions come before removals.
/// A modifier file naming a biome, feature, entity type or tag the root
/// doesn't have (another mod's, a `#c:` convention tag) is skipped with a line
/// in `warnings`, as NeoForge skips a holder set it can't resolve, instead of
/// dropping every content mod. A mod's code finds such a tag empty, as
/// NeoForge's missing tags are, and the root's warnings say so.
pub(super) fn apply(
    biomes: &mut J,
    modifiers: &[Modifier],
    biome_reg: &Registry<'_>,
    feature_reg: &Registry<'_>,
    entities: &Entities<'_>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    // Each file's biomes and the features or entity types it names, resolved
    // once (`None`: skipped, or the mod's code).
    let mut files: Vec<Option<(BTreeSet<String>, Vec<String>)>> = Vec::new();
    for m in modifiers {
        let resolved = match &m.kind {
            Kind::Data {
                biomes: b,
                features: f,
                ..
            } => Some(
                biome_reg
                    .resolve(b)
                    .and_then(|targets| Ok((targets, feature_reg.resolve(f)?))),
            ),
            Kind::Spawns { biomes: b, action } => Some(biome_reg.resolve(b).and_then(|targets| {
                let types = match action {
                    SpawnAction::Add(list) => list
                        .iter()
                        .map(|s| match entities.categories.contains_key(&s.entity) {
                            true => Ok(s.entity.clone()),
                            false => Err(format!("unknown entity type {}", s.entity)),
                        })
                        .collect::<std::result::Result<_, _>>()?,
                    SpawnAction::Remove(set)
                    | SpawnAction::AddCosts(set, ..)
                    | SpawnAction::RemoveCosts(set) => entities.reg.resolve(set)?,
                };
                Ok((targets, types))
            })),
            Kind::Code(_) => None,
        };
        files.push(match resolved {
            Some(Ok((targets, named))) => Some((targets.into_iter().collect(), named)),
            Some(Err(e)) => {
                warnings.push(format!("{}: {e}: skipped", m.name));
                None
            }
            None => None,
        });
    }
    // A mod's code picks its biomes itself: it sees every one.
    let code = modifiers.iter().any(|m| matches!(m.kind, Kind::Code(_)));
    let named = |id: &String| files.iter().flatten().any(|(targets, _)| targets.contains(id));
    let ids: Vec<String> = match &*biomes {
        J::Obj(o) => o
            .iter()
            .map(|(id, _)| id)
            .filter(|&id| code || named(id))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let root = Root {
        biomes: &*biomes,
        biome_reg,
        feature_reg,
        errors: RefCell::new(Vec::new()),
    };
    let mut noted = Vec::new();
    let mut note = |line: String| {
        if !noted.contains(&line) {
            noted.push(line);
        }
    };
    let mut changed = Vec::new();
    for id in ids {
        let Some(J::Str(text)) = root.biomes.get(&id) else {
            continue;
        };
        let mut doc: J = match serde_json::from_str(text) {
            Ok(doc) => doc,
            Err(e) => match modifiers.iter().zip(&files).find(|(_, f)| f.as_ref().is_some_and(|(t, _)| t.contains(&id))) {
                Some((m, _)) => return Err(format!("{}: {id}: {e}", m.name).into()),
                None => {
                    note(format!("biome {id}: {e}: the mods' biome modifier code skips it"));
                    continue;
                }
            },
        };
        let mut dirty = false;
        // The biome's features by step while the mods' code edits them.
        let mut steps: Option<StepFeatures> = None;
        // The biome's mob spawns as the root has them, and as the modifiers
        // leave them.
        let mut spawns: Option<(Spawns, Spawns)> = None;
        for phase in 0..PHASES.len() {
            for (m, file) in modifiers.iter().zip(&files) {
                let at = |e: String| format!("{}: {id}: {e}", m.name);
                match (&m.kind, file) {
                    (Kind::Data { action, .. }, Some((targets, features))) => {
                        let wanted = match action {
                            Action::Add(_) => ADD,
                            Action::Remove(_) => REMOVE,
                        };
                        if phase == wanted && targets.contains(&id) {
                            edit(&mut doc, action, features).map_err(at)?;
                            dirty = true;
                            steps = None;
                        }
                    }
                    (Kind::Spawns { action, .. }, Some((targets, types))) => {
                        let wanted = match action {
                            SpawnAction::Add(_) | SpawnAction::AddCosts(..) => ADD,
                            SpawnAction::Remove(_) | SpawnAction::RemoveCosts(_) => REMOVE,
                        };
                        if phase == wanted && targets.contains(&id) {
                            let (_, now) = spawns.get_or_insert_with(|| {
                                let s = spawns_of(&doc);
                                (s.clone(), s)
                            });
                            edit_spawns(now, action, types, entities.categories);
                        }
                    }
                    (Kind::Code(code), _) => {
                        let before = match steps.take() {
                            Some(steps) => steps,
                            None => match steps_of(&doc, feature_reg) {
                                Ok(steps) => steps,
                                Err(e) => {
                                    note(at(format!("{e}: the mod's code skips this biome")));
                                    continue;
                                }
                            },
                        };
                        let mut after = before.clone();
                        let (_, now) = spawns.get_or_insert_with(|| {
                            let s = spawns_of(&doc);
                            (s.clone(), s)
                        });
                        let kept = now.clone();
                        let result = code.modify(&root, &id, phase, &mut after, now);
                        for e in root.errors.take() {
                            note(format!("{}: {e}: the mod's code finds it empty", m.name));
                        }
                        if let Err(e) = result {
                            note(at(e));
                            steps = Some(before);
                            *now = kept;
                            continue;
                        }
                        // A spawner or cost of an entity type the root doesn't
                        // have (another mod's) would name nothing: left out.
                        let known = |t: &String| {
                            entities.categories.contains_key(t)
                                || kept.costs.contains_key(t)
                                || kept.categories.values().flatten().any(|s| s.entity == *t)
                        };
                        for list in now.categories.values_mut() {
                            list.retain(|s| {
                                let keep = known(&s.entity);
                                if !keep {
                                    note(format!("{}: unknown entity type {}: not added", m.name, s.entity));
                                }
                                keep
                            });
                        }
                        now.costs.retain(|t, _| known(t));
                        // Lists the builder grew for nothing are left out.
                        while after.len() > before.len() && after.last().is_some_and(Vec::is_empty) {
                            after.pop();
                        }
                        // A placed feature the root doesn't have (one that didn't port) would stop
                        // the generator: left out, as NeoForge's codec refuses one it lacks.
                        for list in &mut after {
                            list.retain(|e| {
                                let keep = e.starts_with('{')
                                    || feature_reg.ids.contains(e)
                                    || before.iter().any(|l| l.contains(e));
                                if !keep {
                                    note(format!("{}: unknown placed feature {e}: not added", m.name));
                                }
                                keep
                            });
                        }
                        if after != before {
                            set_steps(&mut doc, &before, &after).map_err(at)?;
                            dirty = true;
                        }
                        steps = Some(after);
                    }
                    _ => {}
                }
            }
        }
        if let Some((before, after)) = &spawns
            && !same_spawns(before, after)
        {
            set_spawns(&mut doc, before, after).map_err(|e| format!("biome {id}: {e}"))?;
            dirty = true;
        }
        if dirty {
            changed.push((id, serde_json::to_string(&doc)?));
        }
    }
    warnings.extend(noted);
    for (id, text) in changed {
        biomes.set(&id, J::Str(text));
    }
    Ok(())
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

/// One spawn modifier's change to a biome's mob spawns; `types` are the
/// entity types its file names.
fn edit_spawns(spawns: &mut Spawns, action: &SpawnAction, types: &[String], categories: &BTreeMap<String, String>) {
    match action {
        SpawnAction::Add(list) => {
            for s in list {
                if let Some(category) = categories.get(&s.entity) {
                    spawns.categories.entry(category.clone()).or_default().push(s.clone());
                }
            }
        }
        SpawnAction::Remove(_) => {
            for list in spawns.categories.values_mut() {
                list.retain(|s| !types.contains(&s.entity));
            }
        }
        SpawnAction::AddCosts(_, budget, charge) => {
            for t in types {
                spawns.costs.insert(t.clone(), (*budget, *charge));
            }
        }
        SpawnAction::RemoveCosts(_) => {
            for t in types {
                spawns.costs.remove(t);
            }
        }
    }
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

/// One modifier's change to one biome's `features` (a list per step).
fn edit(biome: &mut J, action: &Action, features: &[String]) -> std::result::Result<(), String> {
    if biome.get("features").is_none() {
        biome.set("features", J::Arr(Vec::new()));
    }
    let Some(J::Arr(steps)) = biome.get_mut("features") else {
        return Err("features: not a list of steps".into());
    };
    let wanted: &[usize] = match action {
        Action::Add(step) => std::slice::from_ref(step),
        Action::Remove(steps) => steps,
    };
    if let Action::Add(step) = action {
        while steps.len() <= *step {
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
        match action {
            Action::Add(_) => {
                for f in features {
                    // Twice in one step would be a cycle for the sorter.
                    if !list.iter().any(|v| held(v).as_ref() == Some(f)) {
                        list.push(J::Str(f.clone()));
                    }
                }
            }
            Action::Remove(_) => {
                list.retain(|v| held(v).is_none_or(|id| !features.contains(&id)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_world::worldgen::foreign::{Biomes, Handler, Placement};
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
        let entities = Entities { reg: Registry { what: "entity type", ids: &types, tags: &type_tags }, categories: &categories };
        apply(&mut biomes, &modifiers, &biome_reg, &feature_reg, &entities, &mut warnings).unwrap();

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
            ("c", "backwards", r#"{"type": "neoforge:add_spawns", "biomes": "plains", "spawners": {"type": "test:penguin", "weight": 1, "minCount": 3, "maxCount": 2}}"#),
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
        let entities = Entities { reg: Registry { what: "entity type", ids: &types, tags: &type_tags }, categories: &categories };
        apply(&mut biomes, &modifiers, &biome_reg, &feature_reg, &entities, &mut warnings).unwrap();

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
        assert_eq!(said("backwards.json: neoforge:add_spawns skipped: minCount needs to be smaller or equal to maxCount"), 1, "{warnings:?}");
        assert_eq!(said("cold.json: unknown entity type other:seal: not added"), 1, "{warnings:?}");
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }
}
