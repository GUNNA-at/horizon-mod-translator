//! `install`: drop in whatever the player has (one mod jar, a folder of
//! jars, a `.mrpack`, a CurseForge zip or instance, a Crash Assistant mod
//! list) and get the parts Horizon runs installed into a game folder.
//!
//! One jar or a folder of jars goes through [`crate::check`], one by one; a
//! pack goes through the whole pipeline ([`pipeline::port`]). Either way the
//! carried-over folder mods the game can load together are copied into
//! `<game>/horizon-mods/`, and a pack's resource packs into
//! `<game>/resourcepacks/`. Java code is never run or translated: what only
//! code does is listed, per mod, in the result.
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

/// Translates the jar's code into a native mod bundle in `<game>/horizon-native-mods/<id>_translated/`
/// (docs/mod-translator.md); its content mod for code-registered blocks goes to `horizon-mods/`
/// unless the carry-over already made one.
fn translate(jar: &Path, game: &Path, log: Log) -> Option<PathBuf> {
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
            if content.is_dir() && !mods.exists() {
                let _ = copy_dir(&content, &mods);
            }
            let _ = fs::remove_dir_all(&content);
            Some(out)
        }
        Err(e) => {
            log(format!("{id}: code not translated: {e}"));
            None
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
        let native = translate(jar, game, log);
        match check::check(jar, &dir) {
            Ok(c) => {
                log(format!("{}: {}. {}", c.mod_id, c.verdict.label(), c.summary));
                let carry = dir.join("carry");
                if c.verdict != Verdict::No && carry.join(horizon_mods::manifest::MANIFEST).is_file() {
                    carries.push(carry.clone());
                }
                checked.push((c, carry, native));
            }
            Err(e) => {
                log(format!("{}: {e}", jar.display()));
                out.mods.push(Installed {
                    mod_id: stem,
                    native,
                    verdict: Verdict::No,
                    summary: e,
                    path: None,
                });
            }
        }
    }
    let placed = place(&carries, game, &mut out)?;
    for (c, carry, native) in checked {
        let path = placed.iter().find(|(from, _)| *from == carry).map(|(_, to)| to.clone());
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
            let Some(native) = translate(&jar, game, log) else { continue };
            let id = horizon_mod_translator::bundle::mod_id(&jar).unwrap_or_default();
            match out.mods.iter_mut().find(|m| m.mod_id == id) {
                Some(m) => m.native = Some(native),
                None => out.mods.push(Installed {
                    mod_id: id,
                    native: Some(native),
                    verdict: Verdict::Partly,
                    summary: "code translated".into(),
                    path: None,
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
