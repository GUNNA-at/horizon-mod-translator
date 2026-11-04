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

/// What FML reads of each mod a jar declares (`[[mods]]`): id, version and display name, with
/// `${file.jarVersion}` as the jar's manifest gives it (`Implementation-Version`).
pub fn mod_metadata(jar: &Path) -> Vec<(String, String, String)> {
    let Some(text) = mods_toml(jar) else { return Vec::new() };
    let jar_version = jar_manifest_version(jar).unwrap_or_default();
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut in_mod = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_mod = line == "[[mods]]";
            if in_mod {
                out.push(Default::default());
            }
            continue;
        }
        let (Some((k, v)), true, Some(m)) = (line.split_once('='), in_mod, out.last_mut()) else { continue };
        let v = bundle::toml_string(v).replace("${file.jarVersion}", &jar_version);
        match k.trim() {
            "modId" => m.0 = v,
            "version" => m.1 = v,
            "displayName" => m.2 = v,
            _ => {}
        }
    }
    out.retain(|m| !m.0.is_empty());
    out
}

/// A mixin configuration a jar declares: the mixin classes it applies and how many places each
/// of their injectors needs by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MixinConfig {
    /// Its file in the jar (`create_connected.mixins.json`).
    pub file: String,
    /// The mixins it lists (`mixins`, `client`, `server`), as internal names (`a/b/C`).
    pub classes: Vec<String>,
    /// `injectors.defaultRequire`: an injector without its own `require` needs this many places,
    /// else Mixin stops the game; 0 (Mixin's default) lets it find none.
    pub default_require: i64,
    /// Its plugin (`plugin`), which may turn mixins off as the game loads, as an internal name.
    pub plugin: Option<String>,
}

/// The mixin configurations of `jars` and of the jars they bundle, as FML hands them to Mixin:
/// each `[[mixins]] config = "..."` of a jar's mods.toml, and the manifest's `MixinConfigs`.
pub fn mixin_configs_of(jars: &[&Path]) -> Vec<MixinConfig> {
    fn entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
        let mut text = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut text).ok()?;
        Some(text)
    }
    fn read<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, out: &mut Vec<MixinConfig>) {
        let mut files: Vec<String> = Vec::new();
        for toml in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
            let Some(text) = entry(zip, toml) else { continue };
            let mut in_mixins = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_mixins = line == "[[mixins]]";
                    continue;
                }
                if let (true, Some((k, v))) = (in_mixins, line.split_once('='))
                    && k.trim() == "config"
                {
                    files.push(bundle::toml_string(v).to_owned());
                }
            }
            break;
        }
        if let Some(manifest) = entry(zip, "META-INF/MANIFEST.MF")
            && let Some(list) = manifest.lines().find_map(|l| l.strip_prefix("MixinConfigs:"))
        {
            files.extend(list.split(',').map(|f| f.trim().to_owned()).filter(|f| !f.is_empty()));
        }
        for file in files {
            if out.iter().any(|c| c.file == file) {
                continue;
            }
            let Some(json) = entry(zip, &file).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
            let package = json["package"].as_str().unwrap_or("").to_owned();
            let classes = ["mixins", "client", "server"]
                .iter()
                .flat_map(|k| json[*k].as_array().cloned().unwrap_or_default())
                .filter_map(|v| v.as_str().map(|n| format!("{package}.{n}").trim_start_matches('.').replace('.', "/")))
                .collect();
            out.push(MixinConfig {
                file,
                classes,
                default_require: json["injectors"]["defaultRequire"].as_i64().unwrap_or(0),
                plugin: json["plugin"].as_str().map(|p| p.replace('.', "/")),
            });
        }
    }
    let mut out = Vec::new();
    for jar in jars {
        let Ok(mut zip) = std::fs::File::open(jar).map_err(|_| ()).and_then(|f| zip::ZipArchive::new(f).map_err(|_| ())) else { continue };
        read(&mut zip, &mut out);
        for b in nested_jars(&mut zip) {
            if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) {
                read(&mut inner, &mut out);
            }
        }
    }
    out
}

/// The `Implementation-Version` in a jar's `META-INF/MANIFEST.MF`.
fn jar_manifest_version(jar: &Path) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let mut text = String::new();
    zip.by_name("META-INF/MANIFEST.MF").ok()?.read_to_string(&mut text).ok()?;
    text.lines().find_map(|l| l.strip_prefix("Implementation-Version:")).map(|v| v.trim().to_owned())
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
pub mod weave;

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

/// One thing a mixin does to another class: an injector (`@Inject`, `@Redirect`, MixinExtras'
/// `@WrapOperation`...) with the methods it selects and where (`@At`), an `@Overwrite`, an
/// `@Accessor` or `@Invoker`, or a method it adds (`Added`: an interface the target gains).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Injection {
    /// The annotation's simple name (`Inject`, `WrapOperation`...), `Overwrite`, or `Added`.
    pub kind: String,
    /// The class it changes: `net/minecraft/world/level/block/Block`.
    pub target: String,
    /// The methods it selects (`tick`, `render(Lnet/...;F)V`), `,` between them; for an
    /// accessor or invoker, the member it opens; for `Overwrite` or `Added`, the mixin's own method.
    pub method: String,
    /// Where: `HEAD`, `RETURN`, `INVOKE Lnet/minecraft/world/level/Level;setBlock(...)Z`; empty
    /// when the kind has no `@At`.
    pub at: String,
    /// The mixin class.
    pub mixin: String,
}

/// What every mixin among `classes` does (`@Mixin` classes: the class-file-only annotations are
/// read). The translator applies accessors and invokers; this tells what the others need from
/// the game: which of its methods mods hook, and where.
pub fn mixin_injections(classes: &[ClassFile]) -> Vec<Injection> {
    const SPONGE: &str = "Lorg/spongepowered/asm/mixin/";
    const EXTRAS: &str = "Lcom/llamalad7/mixinextras/";
    let simple = |ty: &str| ty.trim_end_matches(';').rsplit('/').next().unwrap_or("").to_owned();
    // `Lorg/.../At;(value=INVOKE|target=Lnet/...;foo()V)` -> `INVOKE Lnet/...;foo()V`.
    let at_of = |v: &str| -> String {
        let inner = v.split_once('(').map_or("", |(_, r)| r.strip_suffix(')').unwrap_or(r));
        let part = |k: &str| inner.split('|').find_map(|p| p.strip_prefix(k)).unwrap_or("");
        let (value, target) = (part("value="), part("target="));
        if target.is_empty() { value.to_owned() } else { format!("{value} {target}") }
    };
    let mut out = Vec::new();
    for c in classes {
        let Some(target) = c.mixin_target() else { continue };
        for m in &c.methods {
            let mut injected = false;
            for a in &m.annotations {
                let kind = simple(&a.ty);
                let injector = (a.ty.starts_with(SPONGE) && a.ty.contains("/injection/") && kind != "At" && kind != "Slice" && kind != "Coerce")
                    || a.ty.starts_with(EXTRAS) && a.ty.contains("/injector/");
                let member = matches!(kind.as_str(), "Accessor" | "Invoker" | "Overwrite") && a.ty.starts_with(SPONGE);
                if kind == "Shadow" || kind == "Unique" || kind == "Final" || kind == "Mutable" {
                    injected = true;
                }
                if !injector && !member {
                    continue;
                }
                injected = true;
                let method = match kind.as_str() {
                    "Accessor" | "Invoker" => a.value("value").map(str::to_owned).unwrap_or_else(|| m.name.clone()),
                    "Overwrite" => m.name.clone(),
                    _ => a.value("method").unwrap_or("").to_owned(),
                };
                let at = a.value("at").map(at_of).unwrap_or_default();
                out.push(Injection { kind, target: target.clone(), method, at, mixin: c.name.clone() });
            }
            // A method of its own the mixin adds (an interface it makes the target implement).
            let synthetic = m.access & 0x1000 != 0;
            if !injected && !synthetic && !m.name.starts_with('<') && !m.name.starts_with("lambda$") {
                out.push(Injection {
                    kind: "Added".into(),
                    target: target.clone(),
                    method: format!("{}{}", m.name, m.desc),
                    at: String::new(),
                    mixin: c.name.clone(),
                });
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod mixin_census_tests {
    use super::*;

    /// A class file as javac writes a mixin: `@Mixin(Block.class)` on the class and, on one method,
    /// `@Inject(method = "tick", at = @At(value = "INVOKE", target = "...Level;setBlock(...)Z"))`,
    /// both class-file-only (`RuntimeInvisibleAnnotations`); a second method has none.
    fn mixin_class() -> Vec<u8> {
        let mut pool: Vec<Vec<u8>> = Vec::new();
        let utf8 = |pool: &mut Vec<Vec<u8>>, s: &str| -> u16 {
            let mut e = vec![1];
            e.extend((s.len() as u16).to_be_bytes());
            e.extend(s.as_bytes());
            pool.push(e);
            pool.len() as u16
        };
        let this_name = utf8(&mut pool, "example/mixin/BlockMixin");
        pool.push([vec![7], this_name.to_be_bytes().to_vec()].concat());
        let this = pool.len() as u16;
        let object = utf8(&mut pool, "java/lang/Object");
        pool.push([vec![7], object.to_be_bytes().to_vec()].concat());
        let sup = pool.len() as u16;
        let invisible = utf8(&mut pool, "RuntimeInvisibleAnnotations");
        let mixin = utf8(&mut pool, "Lorg/spongepowered/asm/mixin/Mixin;");
        let value = utf8(&mut pool, "value");
        let block = utf8(&mut pool, "Lnet/minecraft/world/level/block/Block;");
        let inject = utf8(&mut pool, "Lorg/spongepowered/asm/mixin/injection/Inject;");
        let method = utf8(&mut pool, "method");
        let tick = utf8(&mut pool, "tick");
        let at_name = utf8(&mut pool, "at");
        let at = utf8(&mut pool, "Lorg/spongepowered/asm/mixin/injection/At;");
        let invoke = utf8(&mut pool, "INVOKE");
        let target = utf8(&mut pool, "target");
        let set_block = utf8(&mut pool, "Lnet/minecraft/world/level/Level;setBlock(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z");
        let on_tick = utf8(&mut pool, "onTick");
        let added = utf8(&mut pool, "mod$extra");
        let desc = utf8(&mut pool, "()V");
        let u2 = |v: u16| v.to_be_bytes().to_vec();
        let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
        b.extend(u2(pool.len() as u16 + 1));
        for e in &pool {
            b.extend(e);
        }
        b.extend(u2(0x0001));
        b.extend(u2(this));
        b.extend(u2(sup));
        b.extend(u2(0)); // interfaces
        b.extend(u2(0)); // fields
        b.extend(u2(2)); // methods
        // onTick: @Inject(method = "tick", at = @At(value = "INVOKE", target = ...)).
        let mut ann = u2(1);
        ann.extend(u2(inject));
        ann.extend(u2(2));
        ann.extend(u2(method));
        ann.push(b's');
        ann.extend(u2(tick));
        ann.extend(u2(at_name));
        ann.push(b'@');
        ann.extend(u2(at));
        ann.extend(u2(2));
        ann.extend(u2(value));
        ann.push(b's');
        ann.extend(u2(invoke));
        ann.extend(u2(target));
        ann.push(b's');
        ann.extend(u2(set_block));
        b.extend(u2(0x0002));
        b.extend(u2(on_tick));
        b.extend(u2(desc));
        b.extend(u2(1));
        b.extend(u2(invisible));
        b.extend((ann.len() as u32).to_be_bytes());
        b.extend(ann);
        // mod$extra: no annotation, a method the mixin adds.
        b.extend(u2(0x0001));
        b.extend(u2(added));
        b.extend(u2(desc));
        b.extend(u2(0));
        // The class: @Mixin({Block.class}).
        let mut ann = u2(1);
        ann.extend(u2(mixin));
        ann.extend(u2(1));
        ann.extend(u2(value));
        ann.push(b'[');
        ann.extend(u2(1));
        ann.push(b'c');
        ann.extend(u2(block));
        b.extend(u2(1));
        b.extend(u2(invisible));
        b.extend((ann.len() as u32).to_be_bytes());
        b.extend(ann);
        b
    }

    /// FML's view of a jar's mods: each `[[mods]]` table's id, version and name, a version of
    /// `${file.jarVersion}` read from the manifest, and dependency tables' `modId`s left out.
    #[test]
    fn mod_metadata_is_read_as_fml_reads_it() {
        let dir = std::env::temp_dir().join(format!("hz-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("m.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("META-INF/MANIFEST.MF", opts).unwrap();
        std::io::Write::write_all(&mut zip, b"Manifest-Version: 1.0\r\nImplementation-Version: 2.4.12\r\n").unwrap();
        zip.start_file("META-INF/neoforge.mods.toml", opts).unwrap();
        let toml = "modLoader=\"javafml\"\n[[mods]]\nmodId=\"guardvillagers\" #mandatory\nversion=\"${file.jarVersion}\"\ndisplayName='Guard Villagers'\n\n[[mods]]\nmodId = \"extra\"\n[[dependencies.guardvillagers]]\nmodId=\"neoforge\"\nversion=\"[21,)\"\n";
        std::io::Write::write_all(&mut zip, toml.as_bytes()).unwrap();
        zip.finish().unwrap();
        let found = mod_metadata(&jar);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            found,
            vec![
                ("guardvillagers".to_owned(), "2.4.12".to_owned(), "Guard Villagers".to_owned()),
                ("extra".to_owned(), String::new(), String::new()),
            ]
        );
    }

    /// The mixin configurations FML hands Mixin: a mods.toml's `[[mixins]]` tables and the
    /// manifest's `MixinConfigs`, each config's listed classes (common, client, server) under its
    /// package, its `defaultRequire` (Mixin's 0 when absent) and plugin.
    #[test]
    fn mixin_configs_are_found_as_fml_finds_them() {
        let dir = std::env::temp_dir().join(format!("hz-mixcfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("m.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, text: &str| {
            zip.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zip, text.as_bytes()).unwrap();
        };
        put("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\nMixinConfigs: old.mixins.json\r\n");
        put(
            "META-INF/neoforge.mods.toml",
            "modLoader=\"javafml\"\n[[mods]]\nmodId=\"m\"\n[[mixins]]\nconfig=\"m.mixins.json\" # common\n[[dependencies.m]]\nmodId=\"create\"\n",
        );
        put(
            "m.mixins.json",
            r#"{"package": "com.m.mixin", "plugin": "com.m.mixin.Plugin", "mixins": ["KineticMixin", "compat.TrackMixin"],
                "client": ["ScreenMixin"], "injectors": {"defaultRequire": 1}}"#,
        );
        put("old.mixins.json", r#"{"package": "com.m.old", "server": ["ServerMixin"]}"#);
        put("unlisted.mixins.json", r#"{"package": "com.m.unused", "mixins": ["NeverMixin"]}"#);
        zip.finish().unwrap();
        let found = mixin_configs_of(&[jar.as_path()]);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            found,
            vec![
                MixinConfig {
                    file: "m.mixins.json".into(),
                    classes: vec!["com/m/mixin/KineticMixin".into(), "com/m/mixin/compat/TrackMixin".into(), "com/m/mixin/ScreenMixin".into()],
                    default_require: 1,
                    plugin: Some("com/m/mixin/Plugin".into()),
                },
                MixinConfig { file: "old.mixins.json".into(), classes: vec!["com/m/old/ServerMixin".into()], default_require: 0, plugin: None },
            ]
        );
    }

    #[test]
    fn a_mixins_injection_and_added_method_are_read_from_its_class_file() {
        let class = ClassFile::parse(&mixin_class()).expect("parse");
        let found = mixin_injections(&[class]);
        let block = "net/minecraft/world/level/block/Block";
        assert_eq!(
            found,
            vec![
                Injection { kind: "Added".into(), target: block.into(), method: "mod$extra()V".into(), at: String::new(), mixin: "example/mixin/BlockMixin".into() },
                Injection {
                    kind: "Inject".into(),
                    target: block.into(),
                    method: "tick".into(),
                    at: "INVOKE Lnet/minecraft/world/level/Level;setBlock(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z".into(),
                    mixin: "example/mixin/BlockMixin".into(),
                },
            ]
        );
    }
}
