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
    /// The content mods of the mods a translated jar bundles (Create's Flywheel and Ponder), in
    /// `<game>/horizon-mods/`.
    pub bundled: Vec<PathBuf>,
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
    /// Where its content mod went, in `<game>/horizon-mods/`.
    content: Option<PathBuf>,
    /// Whether that content mod stands over the carry-over's (a mod with machines or mobs).
    stands: bool,
}

/// The jars in translation groups: a mod with the mods built on it (the jars that need it, by their
/// manifests' required dependencies), each group in loading order (a mod before the mods that need
/// it). Mods that reach into each other's classes share one engine, as they share one JVM; a mod
/// nothing else in the pack needs and that needs nothing in it stands alone.
fn groups(jars: &[PathBuf]) -> Vec<Vec<PathBuf>> {
    let ids: Vec<Option<String>> = jars.iter().map(|j| horizon_mod_translator::bundle::mod_id(j)).collect();
    let needs: Vec<Vec<usize>> = jars
        .iter()
        .enumerate()
        .map(|(n, j)| {
            horizon_mod_translator::dependencies(j)
                .iter()
                .filter_map(|d| ids.iter().position(|i| i.as_deref() == Some(d.as_str())))
                .filter(|&d| d != n)
                .collect()
        })
        .collect();
    // Connected by needs: each jar's group is the lowest index it reaches.
    let mut group: Vec<usize> = (0..jars.len()).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (n, ds) in needs.iter().enumerate() {
            for &d in ds {
                let low = group[n].min(group[d]);
                if group[n] != low || group[d] != low {
                    (group[n], group[d], changed) = (low, low, true);
                }
            }
        }
    }
    let mut out = Vec::new();
    for g in 0..jars.len() {
        let mut members: Vec<usize> = (0..jars.len()).filter(|&m| group[m] == g).collect();
        let mut order: Vec<usize> = Vec::new();
        while !members.is_empty() {
            // The next mod whose needs are in (a cycle takes the first left).
            let next = members.iter().position(|m| needs[*m].iter().all(|d| order.contains(d))).unwrap_or(0);
            order.push(members.remove(next));
        }
        if !order.is_empty() {
            out.push(order.into_iter().map(|m| jars[m].clone()).collect());
        }
    }
    out
}

/// Translates a group's code ([`groups`]) into one native mod bundle in
/// `<game>/horizon-native-mods/<id>_translated/` for the first mod's id (docs/mod-translator.md),
/// with a content mod for the blocks each of them registers. A mod with block entities (machines)
/// needs its code's own blocks, which a vanilla template would replace (a chute would be a pressure
/// plate), and a mod with entities (mobs) its code's own entity types and their look, which a
/// carry-over doesn't have: its content mod goes to `horizon-mods/` with the data of `carried(jar)`,
/// the jar's carry-over ([`check::check`]'s, made here when not given), and stands over the
/// carry-over's. Any other mod's content mod goes there only when nothing else is there. What each
/// jar became, by jar.
/// A mod in another language (Kotlin) runs with what its language loader's jar in `pack` carries
/// (Kotlin's library, Kotlin for Forge's), as far as the group's code reaches into it. The content
/// mods of the mods its jars bundle are added to `bundled`.
fn translate(
    group: &[PathBuf],
    pack: &[PathBuf],
    game: &Path,
    carried: &dyn Fn(&Path) -> Option<PathBuf>,
    bundled: &mut Vec<PathBuf>,
    log: Log,
) -> Vec<(PathBuf, Translated)> {
    let jars: Vec<&Path> = group.iter().map(PathBuf::as_path).collect();
    let Some(id) = jars.first().and_then(|j| horizon_mod_translator::bundle::mod_id(j)) else {
        return Vec::new();
    };
    let libraries = horizon_mod_translator::language_libraries(&jars, pack);
    let libraries: Vec<&Path> = libraries.iter().map(PathBuf::as_path).collect();
    let classes = match horizon_mod_translator::program_of(&jars, &libraries) {
        Ok((c, _, _)) if !c.is_empty() => c,
        _ => return Vec::new(),
    };
    for l in &libraries {
        log(format!("{id}: with what its language loader carries, from {}", l.display()));
    }
    let natives = game.join("horizon-native-mods");
    let out = natives.join(format!("{id}_translated"));
    let _ = fs::remove_dir_all(&out);
    let left = match horizon_mod_translator::bundle::write_group(&jars, &classes, &out, None) {
        Ok((_, left)) => left,
        Err(e) => {
            log(format!("{id}: code not translated: {e}"));
            return Vec::new();
        }
    };
    if jars.len() > 1 {
        let with: Vec<String> = jars[1..].iter().filter_map(|j| horizon_mod_translator::bundle::mod_id(j)).collect();
        log(format!("{id}: translated with the mods built on it: {}", with.join(", ")));
    }
    for l in left {
        log(format!("{id}: not translated: {l}"));
    }
    let mut done = Vec::new();
    for (n, jar) in jars.iter().enumerate() {
        let Some(ns) = horizon_mod_translator::bundle::mod_id(jar) else { continue };
        let content = if n == 0 { out.join("content") } else { out.join(format!("content-{ns}")) };
        if n > 0 {
            // Its code is in the group's engine: a bundle of its own from an earlier install would
            // declare its blocks twice.
            let _ = fs::remove_dir_all(natives.join(format!("{ns}_translated")));
        }
        let mods = game.join("horizon-mods").join(&ns);
        // What only its code declares: machines (block entities) and mobs (entity types), by its
        // registrations (Registrate's, DeferredRegister's) or by what its classes extend (a mod
        // registering through a wrapper of its own, as multi-loader mods do: Waddles).
        let registered = |classes: &[horizon_mod_translator::class::ClassFile]| {
            horizon_mod_translator::registrations(classes)
                .iter()
                .any(|r| r.registry == "block_entity_type" || r.registry == "entity_type")
        };
        let registered = if jars.len() == 1 {
            registered(&classes)
        } else {
            horizon_mod_translator::classes(jar).is_ok_and(|c| registered(&c))
        };
        let extends = || {
            check::code_of(jar).is_ok_and(|code| {
                [crate::classes::Feature::BlockEntity, crate::classes::Feature::Entity]
                    .iter()
                    .any(|f| code.features.get(f).is_some_and(|n| *n > 0))
            })
        };
        let machines = registered || extends();
        let mut placed = None;
        if content.is_dir() && (machines || !mods.exists()) {
            let own = out.join(format!("carry-over-{ns}"));
            let carried = match carried(jar) {
                Some(c) => Some(c),
                None if machines => check::check(jar, &own).ok().map(|_| own.join("carry")),
                None => None,
            };
            if let Some(carry) = carried.filter(|c| c.is_dir()) {
                merge_data(&carry, &content, &ns);
            }
            let _ = fs::remove_dir_all(&own);
            if copy_dir(&content, &mods).is_ok() {
                placed = Some(mods);
            }
        }
        let _ = fs::remove_dir_all(&content);
        let stands = machines && placed.is_some();
        done.push((jar.to_path_buf(), Translated { native: out.clone(), content: placed, stands }));
    }
    // The mods the group's jars bundle (Create's Flywheel and Ponder): their look, a content mod of
    // their own each ([`horizon_mod_translator::bundle::write_group`]'s `content-nested-<id>`).
    let mut nested: Vec<PathBuf> = fs::read_dir(&out).into_iter().flatten().flatten().map(|e| e.path()).collect();
    nested.sort();
    for dir in nested {
        let Some(nid) = dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("content-nested-")).map(str::to_owned) else {
            continue;
        };
        let target = game.join("horizon-mods").join(&nid);
        if copy_dir(&dir, &target).is_ok() {
            log(format!("{id}: with {nid}, which it bundles: its assets -> {}", target.display()));
            bundled.push(target);
        }
        let _ = fs::remove_dir_all(&dir);
    }
    done
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
/// which the translated code makes), and its additions to the tags of any other namespace:
/// vanilla's, the convention (`c`) tags, and other mods' (a mod puts its rings in Curios' `ring`
/// tag, its foods in Create's `upright_on_belt`), which a content mod may add to without owning
/// the namespace. A file the content mod has stays; another namespace's other files stay out
/// (the translated manifest claims none).
///
/// The mod's own data the carry-over set aside (`outside/data/<id>/` beside it) comes too: recipes
/// of the mod's own types and of types the game doesn't have, and those for the blocks a content
/// port leaves out, are nothing a content mod alone can use, but a translated mod has its code
/// for them and declares those blocks itself.
fn merge_data(carry: &Path, content: &Path, id: &str) {
    let mut namespaces: Vec<String> = fs::read_dir(carry.join("data"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    namespaces.sort();
    for ns in &namespaces {
        merge_namespace(&carry.join("data").join(ns), content, ns, ns == id);
    }
    if let Some(aside) = carry.parent().map(|out| out.join("outside").join("data").join(id)) {
        merge_namespace(&aside, content, id, true);
    }
}

/// One namespace's files from `from` into `content`'s: all of the mod's own but its declarations,
/// another's tags alone. A file the content mod has stays.
fn merge_namespace(from: &Path, content: &Path, ns: &str, own: bool) {
    for rel in files_under(from) {
        let wanted = if own { !rel.starts_with("horizon") } else { rel.starts_with("tags") };
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

/// The data a translated content mod lacks, merged into `content` from `jar`'s carry-over
/// ([`merge_data`]): what [`install`] does for each mod it translates, for a bundle made by
/// hand (`horizon-mod-translator bundle`, whose content mods carry a mod's look and the blocks
/// its code declares, not its recipes, loot tables, tags or worldgen). How many files came.
pub fn data(jar: &Path, content: &Path) -> Result<usize, String> {
    let id = horizon_mod_translator::bundle::mod_id(jar)
        .ok_or_else(|| format!("{}: no mod id", jar.display()))?;
    let own = std::env::temp_dir().join(format!("horizon-port-data-{}-{id}", std::process::id()));
    let _ = fs::remove_dir_all(&own);
    let carried = check::check(jar, &own).map(|_| own.join("carry"));
    let had = files_under(&content.join("data")).len();
    if let Ok(carry) = &carried {
        merge_data(carry, content, &id);
    }
    let _ = fs::remove_dir_all(&own);
    carried?;
    Ok(files_under(&content.join("data")).len() - had)
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
    // The loader's convention biome tags, when the work folder has its jar (a
    // pack's fetch keeps it in `loader/`): a mod's biome modifiers and
    // structures name them.
    let conventions = crate::worldgen::loader_biome_tags(work);
    // Each jar checked: its verdict and carry-over.
    let mut checks = Vec::new();
    for jar in jars {
        let stem = jar.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let dir = work.join("mods").join(&stem);
        let _ = fs::remove_dir_all(&dir);
        let c = check::check_with(jar, &dir, &conventions);
        match &c {
            Ok(c) => log(format!("{}: {}. {}", c.mod_id, c.verdict.label(), c.summary)),
            Err(e) => log(format!("{}: {e}", jar.display())),
        }
        checks.push((jar.clone(), stem, dir.join("carry"), c));
    }
    // Their code, a mod with the mods built on it as one engine.
    let carried = |j: &Path| checks.iter().find(|(jar, _, _, c)| jar == j && c.is_ok()).map(|(_, _, carry, _)| carry.clone());
    let mut translated: Vec<(PathBuf, Translated)> = Vec::new();
    for group in groups(jars) {
        translated.extend(translate(&group, jars, game, &carried, &mut out.bundled, log));
    }
    let mut carries = Vec::new();
    let mut checked = Vec::new();
    for (jar, stem, carry, c) in checks {
        let translated = translated.iter().position(|(j, _)| *j == jar).map(|i| translated.swap_remove(i).1);
        match c {
            Ok(c) => {
                // A translated content mod that stands keeps the carry-over's data, not its blocks.
                let stands = translated.as_ref().is_some_and(|t| t.stands);
                if c.verdict != Verdict::No && !stands && carry.join(horizon_mods::manifest::MANIFEST).is_file() {
                    carries.push(carry.clone());
                }
                checked.push((c, carry, translated));
            }
            Err(e) => out.mods.push(Installed {
                mod_id: stem,
                path: translated.as_ref().and_then(|t| t.content.clone()),
                native: translated.map(|t| t.native),
                verdict: Verdict::No,
                summary: e,
            }),
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
        let mut translated: Vec<(PathBuf, Translated)> = Vec::new();
        for group in groups(&jars) {
            translated.extend(translate(&group, &jars, game, &|_| None, &mut out.bundled, log));
        }
        for (jar, t) in translated {
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

    /// A jar installed where the work folder holds the loader's jar gets the loader's convention
    /// biome tags its biome modifiers name.
    #[test]
    fn a_dropped_jar_gets_the_loaders_biome_tags() {
        let root = std::env::temp_dir().join(format!("port-install-conv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("game")).unwrap();
        fs::create_dir_all(root.join("work/loader")).unwrap();
        jar(
            &root.join("work/loader/neoforge-21.1.0-universal.jar"),
            &[
                ("data/c/tags/worldgen/biome/is_swamp.json", r#"{"values":["minecraft:swamp"]}"#),
                ("data/c/tags/worldgen/biome/is_desert.json", r#"{"values":["minecraft:desert"]}"#),
            ],
        );
        let path = root.join("rice-1.0.jar");
        jar(
            &path,
            &[
                (
                    "META-INF/neoforge.mods.toml",
                    "modLoader=\"javafml\"\nlicense=\"MIT\"\n[[mods]]\nmodId=\"rice\"\nversion=\"1.0\"\n",
                ),
                (
                    "data/rice/neoforge/biome_modifier/wild_rice.json",
                    r##"{"type":"neoforge:add_features","biomes":"#c:is_swamp","features":"rice:patch","step":"vegetal_decoration"}"##,
                ),
                (
                    "data/rice/recipe/rice.json",
                    r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:wheat"],"result":{"id":"minecraft:bread","count":1}}"#,
                ),
            ],
        );
        let options = Options { work: Some(&root.join("work")), ..Options::default() };
        let out = install(&path, &root.join("game"), &options, &|_| {}).unwrap();
        let dir = out.mods[0].path.clone().unwrap_or_else(|| panic!("not installed: {out:?}"));
        let tags = dir.join("data/c/tags/worldgen/biome");
        assert!(tags.join("is_swamp.json").is_file(), "{out:?}");
        assert!(!tags.join("is_desert.json").exists());
        fs::remove_dir_all(&root).unwrap();
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

    /// A bundle made by hand gets its jar's data with one command: the recipes and the tag
    /// the jar carries come into the content mod, beside the blocks its code declares.
    #[test]
    fn a_bundles_content_mod_gets_its_jars_data() {
        let root = std::env::temp_dir().join(format!("port-install-data-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("content/data/rubies/horizon/block")).unwrap();
        fs::write(
            root.join("content/data/rubies/horizon/block/ruby_block.json"),
            "code",
        )
        .unwrap();
        let path = root.join("rubies-1.0.jar");
        content_jar(&path);
        let came = data(&path, &root.join("content")).unwrap();
        assert_eq!(came, 2);
        let has = |rel: &str| root.join("content").join(rel).is_file();
        assert!(
            has("data/rubies/recipe/ruby_block.json") && has("data/minecraft/tags/item/gems.json")
        );
        assert_eq!(
            fs::read_to_string(root.join("content/data/rubies/horizon/block/ruby_block.json"))
                .unwrap(),
            "code"
        );
        // (Again adds nothing: what is there stays.)
        assert_eq!(data(&path, &root.join("content")).unwrap(), 0);
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
        put("carry/data/quark/recipe/gem_block.json", "carried");
        // What the carry-over set aside: the mod's recipes of its own kind, and another
        // namespace's files, which are not the mod's.
        put("outside/data/rubies/recipe/polishing/ruby.json", "aside");
        put("outside/data/rubies/horizon/block/ruby_lamp.json", "aside");
        put("outside/data/quark/recipe/ruby_pillar.json", "aside");
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
        // What it adds to another mod's tags comes along too; vanilla's and other mods' other
        // files stay out.
        assert_eq!(
            read("data/quark/tags/item/gems.json").as_deref(),
            Some("carried")
        );
        assert_eq!(read("data/minecraft/recipe/stone.json"), None);
        assert_eq!(read("data/quark/recipe/gem_block.json"), None);
        // The mod's own data that was set aside is its data again, its declarations aside.
        assert_eq!(
            read("data/rubies/recipe/polishing/ruby.json").as_deref(),
            Some("aside")
        );
        assert_eq!(read("data/rubies/horizon/block/ruby_lamp.json"), None);
        assert_eq!(read("data/quark/recipe/ruby_pillar.json"), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_addon_is_translated_with_the_mod_it_needs_and_after_it() {
        let root = std::env::temp_dir().join(format!("port-install-groups-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let toml = |id: &str, needs: &[(&str, &str)]| {
            let mut t = format!("modLoader=\"javafml\"\nlicense=\"MIT\"\n[[mods]]\nmodId=\"{id}\"\nversion=\"1\"\n");
            for (dep, kind) in needs {
                t.push_str(&format!("[[dependencies.{id}]]\n    modId=\"{dep}\"\n    type=\"{kind}\"\n    versionRange=\"*\"\n"));
            }
            t
        };
        let make = |file: &str, id: &str, needs: &[(&str, &str)]| {
            let path = root.join(file);
            jar(&path, &[("META-INF/neoforge.mods.toml", &toml(id, needs))]);
            path
        };
        // The addon sorts first and needs a library its own jar carries (not in the pack); a mod
        // that only might use the base, and one on its own, stand alone.
        let addon = make("a-gears-addon.jar", "gears_addon", &[("neoforge", "required"), ("gears", "required"), ("esl", "required")]);
        let deeper = make("b-gears-more.jar", "gears_more", &[("gears_addon", "required")]);
        let maybe = make("c-maybe.jar", "maybe", &[("gears", "optional")]);
        let alone = make("d-alone.jar", "alone", &[]);
        let base = make("z-gears.jar", "gears", &[("minecraft", "required")]);
        let jars = vec![addon.clone(), deeper.clone(), maybe.clone(), alone.clone(), base.clone()];
        assert_eq!(groups(&jars), vec![vec![base, addon, deeper], vec![maybe], vec![alone]]);
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
