//! Ahead-of-time translator from NeoForge mod bytecode to native Rust
//! Horizon mods (`docs/mod-translator.md`).
pub mod class;

use class::{ClassFile, Member};
use std::{collections::BTreeMap, io::Read, path::Path};

/// A game or loader API the mod reaches into: what horizon-compat must map.
pub fn is_api(owner: &str) -> bool {
    ["net/minecraft/", "net/neoforged/", "com/mojang/"]
        .iter()
        .any(|p| owner.starts_with(p))
}

/// The jars a jar bundles for NeoForge's jar-in-jar, as bytes: each `.jar` under
/// `META-INF/jarjar/` and each path `META-INF/jarjar/metadata.json` lists (often
/// `META-INF/jars/...`). Mixin's helper library (MixinExtras) is left out: only mixins use it.
fn nested_jars<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Vec<Vec<u8>> {
    let mut paths: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let Ok(e) = zip.by_index(i) else { continue };
        let Ok(name) = e.name().map(|n| n.into_owned()) else { continue };
        if name.starts_with("META-INF/jarjar/") && name.ends_with(".jar") {
            paths.push(name);
        }
    }
    let mut skipped: Vec<String> = Vec::new();
    if let Ok(mut e) = zip.by_name("META-INF/jarjar/metadata.json") {
        let mut text = String::new();
        if e.read_to_string(&mut text).is_ok()
            && let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text)
        {
            for jar in meta["jars"].as_array().into_iter().flatten() {
                let Some(path) = jar["path"].as_str() else { continue };
                if jar["identifier"]["artifact"].as_str().is_some_and(|a| a.starts_with("mixinextras")) {
                    skipped.push(path.to_owned());
                } else if !paths.iter().any(|p| p == path) {
                    paths.push(path.to_owned());
                }
            }
        }
    }
    paths.retain(|p| !skipped.contains(p) && !p.rsplit('/').next().is_some_and(|f| f.starts_with("mixinextras")));
    paths
        .into_iter()
        .filter_map(|p| {
            let mut e = zip.by_name(&p).ok()?;
            let mut b = Vec::new();
            e.read_to_end(&mut b).ok()?;
            Some(b)
        })
        .collect()
}

/// The classes in a zip, `META-INF` left out, those `have` names already left out too.
fn zip_classes<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, have: &mut std::collections::HashSet<String>, out: &mut Vec<ClassFile>) {
    for i in 0..zip.len() {
        let Ok(mut e) = zip.by_index(i) else { continue };
        let Ok(name) = e.name().map(|n| n.into_owned()) else { continue };
        if !name.ends_with(".class") || name.starts_with("META-INF/") {
            continue;
        }
        let mut b = Vec::new();
        if e.read_to_end(&mut b).is_ok()
            && let Ok(c) = ClassFile::parse(&b)
            && have.insert(c.name.clone())
        {
            out.push(c);
        }
    }
}

/// The names of the classes in a jar and in the libraries it bundles, from their paths (nothing
/// parsed): which mod of several translated together a class is.
pub fn class_names(jar: &Path) -> std::collections::HashSet<String> {
    fn names<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, out: &mut std::collections::HashSet<String>) {
        for i in 0..zip.len() {
            let Ok(e) = zip.by_index(i) else { continue };
            if let Ok(name) = e.name()
                && !name.starts_with("META-INF/")
                && let Some(class) = name.strip_suffix(".class")
            {
                out.insert(class.to_owned());
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    let Ok(mut zip) = std::fs::File::open(jar).map_err(|_| ()).and_then(|f| zip::ZipArchive::new(f).map_err(|_| ())) else {
        return out;
    };
    names(&mut zip, &mut out);
    for b in nested_jars(&mut zip) {
        if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) {
            names(&mut inner, &mut out);
        }
    }
    out
}

/// Every class in a jar and in the libraries it bundles (NeoForge's jar-in-jar), which run inside
/// it: translated with it. A class the mod itself has wins.
pub fn classes(jar: &Path) -> Result<Vec<ClassFile>, String> {
    classes_of(&[jar])
}

/// Every class of `jars` as one program (a mod with the mods that build on it, each with the
/// libraries it bundles). A class an earlier jar has wins.
pub fn classes_of(jars: &[&Path]) -> Result<Vec<ClassFile>, String> {
    let mut out = Vec::new();
    let mut have = std::collections::HashSet::new();
    let mut nested = Vec::new();
    for jar in jars {
        let f = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
        let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", jar.display()))?;
        zip_classes(&mut zip, &mut have, &mut out);
        nested.extend(nested_jars(&mut zip));
    }
    for b in nested {
        if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) {
            zip_classes(&mut inner, &mut have, &mut out);
        }
    }
    Ok(out)
}

/// `META-INF/services` of a jar and the jars it bundles: interface → implementations, in
/// internal names (`a/b/C`), as `ServiceLoader` finds them. The jar's own entries come first.
pub fn services(jar: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    services_of(&[jar])
}

/// [`services`] of several jars, in order.
pub fn services_of(jars: &[&Path]) -> Result<BTreeMap<String, Vec<String>>, String> {
    fn read<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, out: &mut BTreeMap<String, Vec<String>>) {
        for i in 0..zip.len() {
            let Ok(mut e) = zip.by_index(i) else { continue };
            let Ok(name) = e.name().map(|n| n.into_owned()) else { continue };
            let Some(iface) = name.strip_prefix("META-INF/services/").filter(|n| !n.is_empty() && !n.contains('/')) else { continue };
            let mut b = Vec::new();
            if e.read_to_end(&mut b).is_err() {
                continue;
            }
            let list = out.entry(iface.replace('.', "/")).or_default();
            for line in String::from_utf8_lossy(&b).lines() {
                let class = line.split('#').next().unwrap_or("").trim();
                if !class.is_empty() && !list.iter().any(|c| *c == class.replace('.', "/")) {
                    list.push(class.replace('.', "/"));
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut nested = Vec::new();
    for jar in jars {
        let f = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
        let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", jar.display()))?;
        read(&mut zip, &mut out);
        nested.extend(nested_jars(&mut zip));
    }
    for b in nested {
        if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) {
            read(&mut inner, &mut out);
        }
    }
    Ok(out)
}

/// A jar's mod metadata (`META-INF/neoforge.mods.toml`, or Forge's older `mods.toml`) as text.
fn mods_toml(jar: &Path) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        let Ok(mut e) = zip.by_name(name) else { continue };
        let mut text = String::new();
        if e.read_to_string(&mut text).is_ok() {
            return Some(text);
        }
    }
    None
}

/// The mods a jar needs (its `[[dependencies.<id>]]` of type `required`, or `mandatory = true` in
/// the older form), the loader and the game left out.
pub fn dependencies(jar: &Path) -> Vec<String> {
    let Some(text) = mods_toml(jar) else { return Vec::new() };
    let mut out = Vec::new();
    let mut current: Option<(String, bool)> = None;
    let mut in_dependency = false;
    let finish = |current: &mut Option<(String, bool)>, out: &mut Vec<String>| {
        if let Some((id, true)) = current.take()
            && !matches!(id.as_str(), "neoforge" | "forge" | "minecraft" | "java")
            && !out.contains(&id)
        {
            out.push(id);
        }
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            finish(&mut current, &mut out);
            in_dependency = line.starts_with("[[dependencies.");
            if in_dependency {
                current = Some((String::new(), false));
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches('"').trim_matches('\''));
        if let (true, Some((id, required))) = (in_dependency, current.as_mut()) {
            match k {
                "modId" => *id = v.to_owned(),
                "type" => *required = v.eq_ignore_ascii_case("required"),
                "mandatory" => *required = v == "true",
                _ => {}
            }
        }
    }
    finish(&mut current, &mut out);
    out
}

/// API methods the mod calls, with the number of its classes calling each.
pub fn api_calls(classes: &[ClassFile]) -> BTreeMap<Member, usize> {
    let mut m = BTreeMap::new();
    for c in classes {
        let mut seen = std::collections::BTreeSet::new();
        for call in c.called().into_iter().filter(|m| is_api(&m.owner)) {
            if seen.insert(call.clone()) {
                *m.entry(call).or_insert(0) += 1;
            }
        }
    }
    m
}

pub mod bundle;
pub mod codegen;
pub mod insn;
pub mod props;

/// A registration the mod's code makes: registry and name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Registration {
    /// `block`, `item`, `block_entity_type`, ... (from the register call), or `unknown`.
    pub registry: String,
    pub name: String,
    /// The class the registering code lives in.
    pub class: String,
}

fn registry_of(m: &Member) -> String {
    if m.owner.contains("Registrate") {
        return match m.name.as_str() {
            "blockEntity" => "block_entity_type",
            "entity" => "entity_type",
            n => n,
        }
        .into();
    }
    let o = m.owner.rsplit('/').next().unwrap_or(&m.owner);
    let lower = format!("{o}.{}", m.name).to_ascii_lowercase();
    for (k, v) in [
        ("blockentit", "block_entity_type"),
        ("block", "block"),
        ("item", "item"),
        ("entit", "entity_type"),
        ("menu", "menu"),
        ("sound", "sound_event"),
        ("creativemodetab", "creative_mode_tab"),
        ("recipe", "recipe_serializer"),
        ("particle", "particle_type"),
    ] {
        if lower.contains(k) {
            return v.into();
        }
    }
    "unknown".into()
}

/// Whether `name` can be a registry name (`shaft`, `create:shaft`, `worldgen/ore`): a string
/// pushed before a register call can also be a message or a format.
fn valid_name(name: &str) -> bool {
    let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'/' | b'-'));
    match name.split_once(':') {
        Some((ns, path)) => ok(ns) && ok(path),
        None => ok(name),
    }
}

/// Registrate's builder entry points (`REGISTRATE.block("name", ...)`), used by Create and others.
const REGISTRATE: [&str; 6] = ["block", "item", "blockEntity", "entity", "menu", "fluid"];

fn registers(m: &Member) -> bool {
    (m.name.starts_with("register")
        && (m.owner.contains("DeferredRegister") || m.owner == "net/minecraft/core/Registry"))
        || (m.owner.contains("Registrate") && REGISTRATE.contains(&m.name.as_str()))
}

/// Names registered through `DeferredRegister.register*` and `Registry.register`:
/// the string constant pushed last before the call.
pub fn registrations(classes: &[ClassFile]) -> Vec<Registration> {
    let mut out = Vec::new();
    for c in classes {
        for m in &c.methods {
            let Some(code) = &m.code else { continue };
            let Ok(insns) = insn::decode(&code.bytes) else { continue };
            let mut last: Option<String> = None;
            for i in insns {
                match (i.op, &i.arg) {
                    (insn::op::LDC | insn::op::LDC_W, insn::Operand::Int(idx)) => {
                        last = match c.pool.get(*idx as usize) {
                            Some(class::Const::String(s)) => c.utf8(*s).ok(),
                            _ => last,
                        };
                    }
                    (insn::op::INVOKEVIRTUAL | insn::op::INVOKESTATIC | insn::op::INVOKEINTERFACE, arg) => {
                        let idx = match arg {
                            insn::Operand::Int(i) => *i as u16,
                            insn::Operand::IndexCount(i, _) => *i,
                            _ => continue,
                        };
                        if let Some(call) = c.member(idx).filter(registers)
                            && let Some(name) = last.take()
                            && valid_name(&name)
                        {
                            out.push(Registration { registry: registry_of(&call), name, class: c.name.clone() });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}
