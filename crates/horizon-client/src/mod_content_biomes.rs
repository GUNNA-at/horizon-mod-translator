//! Biome modifiers: a content mod's placed features in biomes it does not
//! own (vanilla's), as NeoForge's `neoforge:add_features` and
//! `neoforge:remove_features` biome modifiers do. Each is a file in
//! `data/<ns>/neoforge/biome_modifier/` (older Forge mods:
//! `data/<ns>/forge/biome_modifier/`, type `forge:...`).
//!
//! They rewrite the biomes' `features` lists when the content root builds:
//! an added feature goes at the end of its step's list, as NeoForge adds it,
//! and the generator orders every biome's lists as vanilla's `FeatureSorter`
//! does. Biomes no modifier names keep their text byte for byte, and a root
//! with no modifiers keeps vanilla's bundle, so vanilla features keep their
//! indices and their random seeds.
//!
//! A modifier of a translated mod's own type (Farmer's Delight's
//! `farmersdelight:add_features_by_filter`) is read by the mod's codec and run
//! by its code (`BiomeModifier.modify`), biome by biome in NeoForge's phase
//! order, on the root's biomes, tags and placed features.
use super::{J, Result};
use horizon_world::worldgen::foreign::{self, BiomeModifier, PHASES, StepFeatures};
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

/// What a modifier file is: one of NeoForge's own types, read here, or a
/// translated mod's own, run by the mod's code.
enum Kind {
    Data {
        biomes: Value,
        features: Value,
        action: Action,
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
    /// and for a type Horizon doesn't read yet (`add_spawns`, `add_carvers`, a
    /// mod's own serializer no translated mod registers), which is skipped with
    /// a line in `warnings`: real mods ship those beside their feature
    /// modifiers, and one must not drop every content mod. A translated mod's
    /// own type is read by the mod's codec; one its codec refuses is skipped
    /// with its error.
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
                        "biome modifier type {other:?} skipped: only add_features, remove_features \
                         and none are read, and no translated mod registers it"
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

/// Applies `modifiers` to the biome bundle `biomes` (id → the biome's text),
/// biome by biome as NeoForge does: phase by phase ([`PHASES`]), every
/// modifier in mod and file order in each, so additions come before removals.
/// A modifier file naming a biome, feature or tag the root doesn't have
/// (another mod's, a `#c:` convention tag) is skipped with a line in
/// `warnings`, as NeoForge skips a holder set it can't resolve, instead of
/// dropping every content mod. A mod's code finds such a tag empty, as
/// NeoForge's missing tags are, and the root's warnings say so.
pub(super) fn apply(
    biomes: &mut J,
    modifiers: &[Modifier],
    biome_reg: &Registry<'_>,
    feature_reg: &Registry<'_>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    // Each file's biomes and features, resolved once (`None`: skipped, or the mod's code).
    let mut files: Vec<Option<(BTreeSet<String>, Vec<String>)>> = Vec::new();
    for m in modifiers {
        files.push(match &m.kind {
            Kind::Data {
                biomes: b,
                features: f,
                ..
            } => match biome_reg
                .resolve(b)
                .and_then(|targets| Ok((targets.into_iter().collect(), feature_reg.resolve(f)?)))
            {
                Ok(both) => Some(both),
                Err(e) => {
                    warnings.push(format!("{}: {e}: skipped", m.name));
                    None
                }
            },
            Kind::Code(_) => None,
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
                        let result = code.modify(&root, &id, phase, &mut after);
                        for e in root.errors.take() {
                            note(format!("{}: {e}: the mod's code finds it empty", m.name));
                        }
                        if let Err(e) = result {
                            note(at(e));
                            steps = Some(before);
                            continue;
                        }
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
        apply(&mut biomes, &modifiers, &biome_reg, &feature_reg, &mut warnings).unwrap();

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
}
