//! `install`: drop in whatever the player has (one mod jar, a folder of
//! jars, a `.mrpack`, a CurseForge zip or instance, a Crash Assistant mod
//! list) and get the parts Horizon runs installed into a game folder.
//!
//! One jar or a folder of jars goes through [`crate::check`], one by one; a
//! pack goes through the whole pipeline ([`pipeline::port`]). Either way the
//! carried-over folder mods the game can load together are copied into
//! `<game>/horizon-mods/`, and a pack's resource packs into
//! `<game>/resourcepacks/`. Java code is never run: the translator turns it
//! into native mods in `<game>/horizon-native-mods/` (docs/mod-translator.md).
use crate::{
    check::{self, Verdict},
    pipeline::{self, Log},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// One input mod's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub mod_id: String,
    /// The translated code's native mod bundle, in `<game>/horizon-native-mods/`.
    pub native: Option<PathBuf>,
    pub verdict: Verdict,
    pub summary: String,
    /// Where its folder mod went, when it was installed.
    pub path: Option<PathBuf>,
}

/// What an install did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub mods: Vec<Installed>,
    /// Mods carried over but left off (namespace clash, bad manifest).
    pub left_off: Vec<String>,
    pub resourcepacks: Vec<PathBuf>,
    /// The pack's coverage report, when a pack was ported.
    pub report: Option<PathBuf>,
}

fn is_jar(p: &Path) -> bool {
    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar"))
}

/// A folder of loose jars (a downloads folder, an instance's `mods/`), as
/// opposed to an instance, which has a game folder inside.
fn jar_folder(dir: &Path) -> Option<Vec<PathBuf>> {
    if !dir.is_dir() || crate::import::game_dir(dir).is_ok() {
        return None;
    }
    let mut jars: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_jar(p))
        .collect();
    jars.sort();
    (!jars.is_empty()).then_some(jars)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(to);
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// The folder name a carried mod takes in `horizon-mods/`: its manifest id.
fn mod_id(carry: &Path) -> String {
    fs::read_to_string(carry.join(horizon_mods::manifest::MANIFEST))
        .ok()
        .and_then(|t| horizon_mods::Manifest::parse(&t).ok())
        .map(|m| m.id)
        .unwrap_or_else(|| {
            carry
                .parent()
                .and_then(Path::file_name)
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}

/// What [`translate`] made of a jar.
struct Translated {
    /// The native mod bundle, in `<game>/horizon-native-mods/`.
    native: PathBuf,
    /// Where its content mod went when it stands over the carry-over's (a mod with machines).
    content: Option<PathBuf>,
}

/// Translates the jar's code into a native mod bundle in `<game>/horizon-native-mods/<id>_translated/`
/// (docs/mod-translator.md), with a content mod for the blocks its code registers. A mod with block
/// entities (machines) needs its code's own blocks, which a vanilla template would replace (a chute
/// would be a pressure plate): its content mod goes to `horizon-mods/` with the data of `carried`, the
/// jar's carry-over ([`check::check`]'s, made here when not given), and stands over the carry-over's.
/// Any other mod's content mod goes there only when nothing else is there.
fn translate(jar: &Path, game: &Path, carried: Option<&Path>, log: Log) -> Option<Translated> {
    let classes = horizon_mod_translator::classes(jar).ok()?;
    if classes.is_empty() {
        return None;
    }
    let id = horizon_mod_translator::bundle::mod_id(jar)?;
    let out = game.join("horizon-native-mods").join(format!("{id}_translated"));
    let _ = fs::remove_dir_all(&out);
    match horizon_mod_translator::bundle::write(jar, &classes, &out) {
        Ok((_, left)) => {
            for l in left {
                log(format!("{id}: not translated: {l}"));
            }
            let content = out.join("content");
            let mods = game.join("horizon-mods").join(&id);
            let machines = horizon_mod_translator::registrations(&classes)
                .iter()
                .any(|r| r.registry == "block_entity_type");
            let mut stands = None;
            if content.is_dir() && (machines || !mods.exists()) {
                let own = out.join("carry-over");
                let carried = match carried {
                    Some(c) => Some(c.to_path_buf()),
                    None if machines => check::check(jar, &own).ok().map(|_| own.join("carry")),
                    None => None,
                };
                if let Some(carry) = carried.filter(|c| c.is_dir()) {
                    merge_data(&carry, &content, &id);
                }
                let _ = fs::remove_dir_all(&own);
                if copy_dir(&content, &mods).is_ok() && machines {
                    stands = Some(mods);
                }
            }
            let _ = fs::remove_dir_all(&content);
            Some(Translated { native: out, content: stands })
        }
        Err(e) => {
            log(format!("{id}: code not translated: {e}"));
            None
        }
    }
}

/// Every file under `dir`, as paths relative to it.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        let Ok(rd) = fs::read_dir(dir.join(&rel)) else { continue };
        for e in rd.flatten() {
            let r = rel.join(e.file_name());
            if e.path().is_dir() {
                todo.push(r);
            } else {
                out.push(r);
            }
        }
    }
    out.sort();
    out
}

/// Copies a carry-over's data into a translated content mod: the mod's own data (recipes, loot
/// tables, tags, worldgen, advancements) but its block and item declarations (`data/<id>/horizon/`,
/// which the translated code makes), and its additions to vanilla's and the convention (`c`) tags.
/// A file the content mod has stays; other namespaces stay out (the translated manifest claims none).
fn merge_data(carry: &Path, content: &Path, id: &str) {
    for ns in [id, "minecraft", "c"] {
        let from = carry.join("data").join(ns);
        for rel in files_under(&from) {
            let wanted = if ns == id { !rel.starts_with("horizon") } else { rel.starts_with("tags") };
            if !wanted {
                continue;
            }
            let target = content.join("data").join(ns).join(&rel);
            if target.exists() {
                continue;
            }
            if let Some(parent) = target.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(from.join(&rel), &target);
        }
    }
}

/// Copies the loadable carries into `<game>/horizon-mods/<id>/`.
fn place(carries: &[PathBuf], game: &Path, out: &mut Outcome) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let (on, off) = pipeline::loadable(carries);
    out.left_off.extend(off);
    let mods = game.join("horizon-mods");
    let mut placed = Vec::new();
    for carry in on {
        let target = mods.join(mod_id(&carry));
        copy_dir(&carry, &target)?;
        placed.push((carry, target));
    }
    Ok(placed)
}

fn jars(jars: &[PathBuf], game: &Path, work: &Path, log: Log) -> Result<Outcome, String> {
    let mut out = Outcome::default();
    let mut carries = Vec::new();
    let mut checked = Vec::new();
    for jar in jars {
        let stem = jar.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let dir = work.join("mods").join(&stem);
        let _ = fs::remove_dir_all(&dir);
        match check::check(jar, &dir) {
            Ok(c) => {
                log(format!("{}: {}. {}", c.mod_id, c.verdict.label(), c.summary));
                let carry = dir.join("carry");
                let translated = translate(jar, game, Some(&carry), log);
                // A translated content mod that stands keeps the carry-over's data, not its blocks.
                let stands = translated.as_ref().is_some_and(|t| t.content.is_some());
                if c.verdict != Verdict::No && !stands && carry.join(horizon_mods::manifest::MANIFEST).is_file() {
                    carries.push(carry.clone());
                }
                checked.push((c, carry, translated));
            }
            Err(e) => {
                log(format!("{}: {e}", jar.display()));
                let translated = translate(jar, game, None, log);
                out.mods.push(Installed {
                    mod_id: stem,
                    path: translated.as_ref().and_then(|t| t.content.clone()),
                    native: translated.map(|t| t.native),
                    verdict: Verdict::No,
                    summary: e,
                });
            }
        }
    }
    let placed = place(&carries, game, &mut out)?;
    for (c, carry, translated) in checked {
        let path = placed
            .iter()
            .find(|(from, _)| *from == carry)
            .map(|(_, to)| to.clone())
            .or_else(|| translated.as_ref().and_then(|t| t.content.clone()));
        let native = translated.map(|t| t.native);
        out.mods.push(Installed {
            mod_id: c.mod_id,
            verdict: if path.is_none() && native.is_none() { Verdict::No } else { c.verdict },
            native,
            summary: c.summary,
            path,
        });
    }
    Ok(out)
}

fn pack(
    source: &Path,
    game: &Path,
    work: &Path,
    resources: Option<&Path>,
    from: Option<&Path>,
    offline: bool,
    log: Log,
) -> Result<Outcome, String> {
    pipeline::import(source, work, log)?;
    pipeline::inventory(work, offline, log)?;
    let (_, rows) = pipeline::port(work, resources, from, log)?;
    let mut out = Outcome {
        report: Some(work.join("coverage.md")),
        ..Outcome::default()
    };
    let mut carries: Vec<PathBuf> = fs::read_dir(work.join("mods"))
        .map(|rd| rd.flatten().map(|e| e.path().join("carry")).collect())
        .unwrap_or_default();
    carries.retain(|c| c.join(horizon_mods::manifest::MANIFEST).is_file());
    carries.sort();
    for (carry, to) in place(&carries, game, &mut out)? {
        out.mods.push(Installed {
            mod_id: mod_id(&carry),
            native: None,
            verdict: Verdict::Works,
            summary: String::new(),
            path: Some(to),
        });
    }
    for row in rows {
        if !out.mods.iter().any(|m| m.mod_id == row.key) && row.status != crate::report::Status::Ported {
            out.mods.push(Installed {
                mod_id: row.key.clone(),
                native: None,
                verdict: Verdict::No,
                summary: row.reason.clone(),
                path: None,
            });
        }
    }
    // Every jar's code, translated into native mod bundles.
    {
        let records: Vec<crate::inventory::Record> =
            pipeline::read_json(&work.join("inventory.json")).unwrap_or_default();
        let mut jars: Vec<PathBuf> = records
            .iter()
            .filter_map(|r| r.jar.as_ref())
            .map(|j| if Path::new(j).is_absolute() { PathBuf::from(j) } else { work.join(j) })
            .filter(|p| is_jar(p))
            .collect();
        jars.sort();
        jars.dedup();
        for jar in jars {
            let Some(t) = translate(&jar, game, None, log) else { continue };
            let id = horizon_mod_translator::bundle::mod_id(&jar).unwrap_or_default();
            match out.mods.iter_mut().find(|m| m.mod_id == id) {
                Some(m) => {
                    m.native = Some(t.native);
                    if t.content.is_some() {
                        m.path = t.content;
                    }
                }
                None => out.mods.push(Installed {
                    mod_id: id,
                    native: Some(t.native),
                    verdict: Verdict::Partly,
                    summary: "code translated".into(),
                    path: t.content,
                }),
            }
        }
    }
    let packs = work.join("resourcepacks");
    if let Ok(rd) = fs::read_dir(&packs) {
        let dest = game.join("resourcepacks");
        fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        for e in rd.flatten() {
            let target = dest.join(e.file_name());
            if target.exists() {
                continue; // never overwrite the player's own pack
            }
            if e.path().is_dir() {
                copy_dir(&e.path(), &target)?;
            } else if fs::copy(e.path(), &target).is_err() {
                continue;
            }
            out.resourcepacks.push(target);
        }
    }
    Ok(out)
}

/// Options for [`install`].
#[derive(Debug, Default, Clone)]
pub struct Options<'a> {
    pub work: Option<&'a Path>,
    pub resources: Option<&'a Path>,
    pub from: Option<&'a Path>,
    pub offline: bool,
}

/// Installs `source` into the game folder `game`. The work folder defaults
/// to `<game>/horizon-ports/<source name>/`.
pub fn install(source: &Path, game: &Path, opts: &Options, log: Log) -> Result<Outcome, String> {
    if !source.exists() {
        return Err(format!("{}: not found", source.display()));
    }
    let name = source.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let work = opts
        .work
        .map(Path::to_path_buf)
        .unwrap_or_else(|| game.join("horizon-ports").join(&name));
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    if is_jar(source) {
        return jars(&[source.to_path_buf()], game, &work, log);
    }
    if let Some(list) = jar_folder(source) {
        return jars(&list, game, &work, log);
    }
    pack(source, game, &work, opts.resources, opts.from, opts.offline, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(path: &Path, files: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn content_jar(path: &Path) {
        jar(
            path,
            &[
                (
                    "META-INF/neoforge.mods.toml",
                    "modLoader=\"javafml\"\nlicense=\"MIT\"\n[[mods]]\nmodId=\"rubies\"\nversion=\"1.0\"\ndisplayName=\"Rubies\"\n",
                ),
                (
                    "data/rubies/recipe/ruby_block.json",
                    r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:diamond"],"result":{"id":"minecraft:diamond_block","count":1}}"#,
                ),
                (
                    "data/minecraft/tags/item/gems.json",
                    r#"{"values":["minecraft:diamond"]}"#,
                ),
            ],
        );
    }

    #[test]
    fn a_dropped_jar_lands_in_horizon_mods() {
        let root = std::env::temp_dir().join(format!("port-install-jar-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("game")).unwrap();
        let path = root.join("rubies-1.0.jar");
        content_jar(&path);
        let out = install(&path, &root.join("game"), &Options::default(), &|_| {}).unwrap();
        assert_eq!(out.mods.len(), 1, "{out:?}");
        let m = &out.mods[0];
        assert_ne!(m.verdict, Verdict::No, "{m:?}");
        let dir = m.path.clone().expect("installed");
        assert!(dir.starts_with(root.join("game/horizon-mods")));
        assert!(dir.join("horizon-mod.toml").is_file());
        assert!(dir.join("data/rubies/recipe/ruby_block.json").is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_translated_content_mod_takes_the_carried_data_but_keeps_its_own_blocks() {
        let root = std::env::temp_dir().join(format!("port-install-merge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let put = |rel: &str, text: &str| {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        put("carry/data/rubies/recipe/ruby.json", "carried");
        put("carry/data/rubies/recipe/deep/gem.json", "carried");
        put("carry/data/rubies/loot_table/blocks/ruby_block.json", "carried");
        put("carry/data/rubies/horizon/block/ruby_block.json", "template");
        put("carry/data/rubies/horizon/block/ruby_ore.json", "template");
        put("carry/data/minecraft/tags/item/gems.json", "carried");
        put("carry/data/c/tags/item/gems/ruby.json", "carried");
        put("carry/data/minecraft/recipe/stone.json", "carried");
        put("carry/data/quark/tags/item/gems.json", "carried");
        put("content/data/rubies/horizon/block/ruby_block.json", "code");
        put("content/data/rubies/recipe/ruby.json", "own");
        merge_data(&root.join("carry"), &root.join("content"), "rubies");
        let read = |rel: &str| fs::read_to_string(root.join("content").join(rel)).ok();
        // The carried data comes along...
        assert_eq!(read("data/rubies/recipe/deep/gem.json").as_deref(), Some("carried"));
        assert_eq!(read("data/rubies/loot_table/blocks/ruby_block.json").as_deref(), Some("carried"));
        assert_eq!(read("data/minecraft/tags/item/gems.json").as_deref(), Some("carried"));
        assert_eq!(read("data/c/tags/item/gems/ruby.json").as_deref(), Some("carried"));
        // ...but the translated code's declarations and files stay, and no block comes from a template.
        assert_eq!(read("data/rubies/recipe/ruby.json").as_deref(), Some("own"));
        assert_eq!(read("data/rubies/horizon/block/ruby_block.json").as_deref(), Some("code"));
        assert_eq!(read("data/rubies/horizon/block/ruby_ore.json"), None);
        // Vanilla's files and other mods' namespaces stay out.
        assert_eq!(read("data/minecraft/recipe/stone.json"), None);
        assert_eq!(read("data/quark/tags/item/gems.json"), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_jar_with_nothing_to_port_is_not_installed() {
        let root = std::env::temp_dir().join(format!("port-install-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("drop")).unwrap();
        jar(
            &root.join("drop/lib.jar"),
            &[(
                "META-INF/neoforge.mods.toml",
                "modLoader=\"javafml\"\nlicense=\"MIT\"\n[[mods]]\nmodId=\"lib\"\nversion=\"1\"\n",
            )],
        );
        content_jar(&root.join("drop/rubies.jar"));
        let out = install(&root.join("drop"), &root.join("game"), &Options::default(), &|_| {}).unwrap();
        let by = |id: &str| out.mods.iter().find(|m| m.mod_id == id).unwrap();
        assert_eq!(by("lib").verdict, Verdict::No);
        assert!(by("lib").path.is_none());
        assert!(by("rubies").path.is_some());
        fs::remove_dir_all(&root).unwrap();
    }
}
