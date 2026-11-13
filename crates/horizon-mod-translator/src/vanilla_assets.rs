//! What a 1.21.1 mod's models take of the game's own that 26.3 no longer has.
//!
//! A mod's model names the game's models and textures: every 1.21.1 spawn egg's item model has
//! `item/template_spawn_egg` for its parent, a lantern's draws `block/chain`. 26.3 has neither
//! (each of its spawn eggs has a texture of its own, the chain is the iron chain, a block's item
//! draws the block's model with no item model between), and a model that names one draws nothing:
//! 146 item models of the pack's mods, all its spawn eggs, had no icon.
//!
//! The file 1.21.1 had is the answer, and the player has it: their NeoForge install keeps the
//! game's resources beside its classes ([`crate::find_game_assets`]). [`keep_dropped`] copies each
//! such model or texture into the mod's own namespace and names it there, with what a copied
//! model names in its turn. Which names 26.3 lacks is a table read from both games
//! (`tools/vanilla_dropped_assets.py`); nothing of the game's is in this repository.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[path = "vanilla_dropped.rs"]
mod dropped;

/// What a file names: a model (under `models/`, `.json`) or a texture (under `textures/`, `.png`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Model,
    Texture,
}

impl Kind {
    fn folder(self) -> &'static str {
        match self {
            Kind::Model => "models",
            Kind::Texture => "textures",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Kind::Model => "json",
            Kind::Texture => "png",
        }
    }
    /// Whether 1.21.1's game has `path` of this kind and 26.3's doesn't.
    fn dropped(self, path: &str) -> bool {
        match self {
            Kind::Model => dropped::MODELS.binary_search(&path).is_ok(),
            Kind::Texture => dropped::TEXTURES.binary_search(&path).is_ok(),
        }
    }
}

/// The game's own path of a model or texture as a file names it: `minecraft:item/x`, or `item/x`
/// (`ResourceLocation.parse`: a bare path is the game's). None for another namespace's, or a
/// texture variable (`#layer0`).
fn vanilla(name: &str) -> Option<&str> {
    match name.split_once(':') {
        Some(("minecraft", path)) => Some(path),
        Some(_) => None,
        None => (!name.starts_with('#')).then_some(name),
    }
}

/// The name mod `ns` keeps 1.21.1's `path` under: in the folder the game's was in (the atlas
/// takes a texture by its folder, `block/` or `item/`), marked as the game's of that version.
fn kept(ns: &str, path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, file)) => format!("{ns}:{dir}/minecraft_1_21_{file}"),
        None => format!("{ns}:minecraft_1_21_{path}"),
    }
}

struct Keeper<'a> {
    content: &'a Path,
    ns: &'a str,
    game: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    /// What was asked for, and the name it has now (None: the player's game has no such file).
    taken: BTreeMap<(Kind, String), Option<String>>,
}

impl Keeper<'_> {
    fn file(&self, kind: Kind, name: &str) -> PathBuf {
        let (ns, path) = name.split_once(':').unwrap_or((self.ns, name));
        self.content.join(format!("assets/{ns}/{}/{path}.{}", kind.folder(), kind.ext()))
    }

    /// The name to use for the game's `path`: the mod's own copy of what 1.21.1 had, made the
    /// first time it is asked for. None where 26.3 has it, or the player's game doesn't.
    fn take(&mut self, kind: Kind, path: &str) -> Option<String> {
        if !kind.dropped(path) {
            return None;
        }
        let key = (kind, path.to_owned());
        if let Some(known) = self.taken.get(&key) {
            return known.clone();
        }
        // Held before a model's own names are read: a model that named itself would not end.
        self.taken.insert(key.clone(), None);
        let from = format!("assets/minecraft/{}/{path}.{}", kind.folder(), kind.ext());
        let mut bytes = (self.game)(&from)?;
        let name = kept(self.ns, path);
        if kind == Kind::Model {
            let mut model: Value = serde_json::from_slice(&bytes).ok()?;
            self.model(&mut model);
            bytes = serde_json::to_vec_pretty(&model).ok()?;
        }
        let to = self.file(kind, &name);
        fs::create_dir_all(to.parent()?).ok()?;
        fs::write(&to, bytes).ok()?;
        // A texture's animation goes with it.
        if kind == Kind::Texture
            && let Some(meta) = (self.game)(&format!("{from}.mcmeta"))
        {
            fs::write(to.with_extension("png.mcmeta"), meta).ok()?;
        }
        self.taken.insert(key, Some(name.clone()));
        Some(name)
    }

    /// `name` (a string of a file) set to the mod's copy where it names one 26.3 lacks.
    fn name(&mut self, kind: Kind, name: &mut Value) -> bool {
        let Some(new) = name.as_str().and_then(vanilla).and_then(|path| self.take(kind, path)) else {
            return false;
        };
        *name = Value::String(new);
        true
    }

    /// A model's names: its parent, its textures, its overrides' models.
    fn model(&mut self, model: &mut Value) -> bool {
        let Some(m) = model.as_object_mut() else {
            return false;
        };
        let mut changed = m.get_mut("parent").is_some_and(|p| self.name(Kind::Model, p));
        if let Some(textures) = m.get_mut("textures").and_then(Value::as_object_mut) {
            for texture in textures.values_mut() {
                changed |= self.name(Kind::Texture, texture);
            }
        }
        for o in m.get_mut("overrides").and_then(Value::as_array_mut).into_iter().flatten() {
            changed |= o.get_mut("model").is_some_and(|p| self.name(Kind::Model, p));
        }
        changed
    }

    /// A block state file's names: each `model` of its variants and parts.
    fn states(&mut self, v: &mut Value) -> bool {
        match v {
            Value::Array(a) => a.iter_mut().fold(false, |c, x| self.states(x) | c),
            Value::Object(o) => o.iter_mut().fold(false, |c, (key, x)| {
                let changed = match key.as_str() {
                    "model" if x.is_string() => self.name(Kind::Model, x),
                    _ => self.states(x),
                };
                changed | c
            }),
            _ => false,
        }
    }
}

/// Every `.json` under `dir`.
fn jsons(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            jsons(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// Gives the models and block states of namespace `ns` in the content mod at `content` what they
/// name of 1.21.1's game that 26.3 lacks: each such model or texture is copied from the player's
/// 1.21.1 game into the mod's namespace (`<ns>:item/minecraft_1_21_template_spawn_egg`), with
/// what a copied model names in its turn, and the files name the copies. `game` reads a file of
/// that game's resources by its path (`assets/minecraft/models/item/template_spawn_egg.json`),
/// None where there is none; a name it has no file for is left as it was. The names given, each
/// once.
pub fn keep_dropped(content: &Path, ns: &str, game: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Vec<String> {
    let mut keeper = Keeper {
        content,
        ns,
        game,
        taken: BTreeMap::new(),
    };
    for (folder, model) in [("models", true), ("blockstates", false)] {
        let mut files = Vec::new();
        jsons(&content.join(format!("assets/{ns}/{folder}")), &mut files);
        files.sort();
        for file in files {
            // The copies made here were read as they were made.
            let copy = file.file_name().is_some_and(|n| n.to_string_lossy().starts_with("minecraft_1_21_"));
            let Some(mut v) = fs::read(&file).ok().filter(|_| !copy).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
                continue;
            };
            let changed = if model { keeper.model(&mut v) } else { keeper.states(&mut v) };
            if changed && let Ok(bytes) = serde_json::to_vec_pretty(&v) {
                let _ = fs::write(&file, bytes);
            }
        }
    }
    keeper.taken.into_values().flatten().collect()
}

/// The player's 1.21.1 game's resources, for [`keep_dropped`]: its files read from the jar a
/// NeoForge install keeps them in ([`crate::find_game_assets`]). Answers None for every file
/// where no such install is found.
pub fn player_game() -> impl FnMut(&str) -> Option<Vec<u8>> {
    let mut jar = crate::find_game_assets()
        .and_then(|path| fs::File::open(path).ok())
        .and_then(|file| zip::ZipArchive::new(file).ok());
    move |name: &str| {
        let mut entry = jar.as_mut()?.by_name(name).ok()?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spawn egg of a 1.21.1 mod: its model's parent is the game's template, which 26.3 lacks
    /// with both of its textures. The mod gets all three under its own name, the template naming
    /// the copied textures and still the parent 26.3 has; a second egg names the same copies. A
    /// model on what 26.3 has, on another mod's, or on a name the player's game has no file for
    /// stays as it was, and so does a block state but for the model 26.3 dropped.
    #[test]
    fn a_model_keeps_what_it_names_of_the_game_that_26_3_dropped() {
        let dir = std::env::temp_dir().join(format!("hz-vanilla-assets-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let put = |path: &str, text: &str| {
            let to = dir.join(path);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::write(to, text).unwrap();
        };
        put("assets/zoo/models/item/penguin_spawn_egg.json", r#"{"parent": "item/template_spawn_egg"}"#);
        put("assets/zoo/models/item/seal_spawn_egg.json", r#"{"parent": "minecraft:item/template_spawn_egg"}"#);
        put("assets/zoo/models/item/fish.json", r#"{"parent": "item/generated", "textures": {"layer0": "zoo:item/fish"}}"#);
        put("assets/zoo/models/block/lamp.json", r##"{"parent": "other:block/lamp", "textures": {"chain": "minecraft:block/chain", "side": "#chain", "glass": "block/glass", "gone": "block/purpur_pillar"}}"##);
        put("assets/zoo/blockstates/pen.json", r#"{"variants": {"": [{"model": "minecraft:block/template_farmland", "y": 90}, {"model": "zoo:block/lamp"}]}}"#);
        // The 1.21.1 game, as far as this mod reaches into it; it has no purpur pillar here.
        let mut asked = Vec::new();
        let mut game = |name: &str| -> Option<Vec<u8>> {
            asked.push(name.to_owned());
            let text = match name {
                "assets/minecraft/models/item/template_spawn_egg.json" => r#"{"parent": "item/generated", "textures": {"layer0": "item/spawn_egg", "layer1": "item/spawn_egg_overlay"}}"#,
                "assets/minecraft/models/block/template_farmland.json" => r##"{"parent": "block/block", "textures": {"particle": "#dirt"}}"##,
                "assets/minecraft/textures/item/spawn_egg.png" => "egg",
                "assets/minecraft/textures/item/spawn_egg_overlay.png" => "spots",
                "assets/minecraft/textures/block/chain.png" => "chain",
                "assets/minecraft/textures/block/chain.png.mcmeta" => "{}",
                _ => return None,
            };
            Some(text.as_bytes().to_vec())
        };
        let given = keep_dropped(&dir, "zoo", &mut game);
        assert_eq!(
            given,
            [
                "zoo:block/minecraft_1_21_template_farmland",
                "zoo:item/minecraft_1_21_template_spawn_egg",
                "zoo:block/minecraft_1_21_chain",
                "zoo:item/minecraft_1_21_spawn_egg",
                "zoo:item/minecraft_1_21_spawn_egg_overlay",
            ]
        );
        let json = |path: &str| -> Value { serde_json::from_slice(&fs::read(dir.join(path)).unwrap()).unwrap() };
        let egg = "zoo:item/minecraft_1_21_template_spawn_egg";
        assert_eq!(json("assets/zoo/models/item/penguin_spawn_egg.json"), serde_json::json!({"parent": egg}));
        assert_eq!(json("assets/zoo/models/item/seal_spawn_egg.json"), serde_json::json!({"parent": egg}));
        assert_eq!(
            json("assets/zoo/models/item/minecraft_1_21_template_spawn_egg.json"),
            serde_json::json!({"parent": "item/generated", "textures": {"layer0": "zoo:item/minecraft_1_21_spawn_egg", "layer1": "zoo:item/minecraft_1_21_spawn_egg_overlay"}})
        );
        assert_eq!(fs::read(dir.join("assets/zoo/textures/item/minecraft_1_21_spawn_egg.png")).unwrap(), b"egg");
        assert_eq!(fs::read(dir.join("assets/zoo/textures/item/minecraft_1_21_spawn_egg_overlay.png")).unwrap(), b"spots");
        assert_eq!(fs::read(dir.join("assets/zoo/textures/block/minecraft_1_21_chain.png")).unwrap(), b"chain");
        assert_eq!(fs::read(dir.join("assets/zoo/textures/block/minecraft_1_21_chain.png.mcmeta")).unwrap(), b"{}");
        assert_eq!(
            json("assets/zoo/models/item/fish.json"),
            serde_json::json!({"parent": "item/generated", "textures": {"layer0": "zoo:item/fish"}})
        );
        assert_eq!(
            json("assets/zoo/models/block/lamp.json"),
            serde_json::json!({"parent": "other:block/lamp", "textures": {"chain": "zoo:block/minecraft_1_21_chain", "side": "#chain", "glass": "block/glass", "gone": "block/purpur_pillar"}})
        );
        assert_eq!(
            json("assets/zoo/blockstates/pen.json"),
            serde_json::json!({"variants": {"": [{"model": "zoo:block/minecraft_1_21_template_farmland", "y": 90}, {"model": "zoo:block/lamp"}]}})
        );
        // Each file of the game's was asked for once, and none that 26.3 has.
        let once = |name: &str| asked.iter().filter(|a| *a == name).count();
        assert_eq!(once("assets/minecraft/models/item/template_spawn_egg.json"), 1);
        assert_eq!(once("assets/minecraft/textures/block/purpur_pillar.png"), 1);
        assert!(!asked.iter().any(|a| a.contains("generated") || a.contains("glass") || a.contains("block/block")), "{asked:?}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
