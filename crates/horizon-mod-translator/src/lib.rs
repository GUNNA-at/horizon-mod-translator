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

/// Every class in a jar (nested jars in `META-INF/jarjar` left out).
pub fn classes(jar: &Path) -> Result<Vec<ClassFile>, String> {
    let f = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = e.name().map_err(|e| e.to_string())?.into_owned();
        if !name.ends_with(".class") || name.starts_with("META-INF/") {
            continue;
        }
        let mut b = Vec::new();
        e.read_to_end(&mut b).map_err(|e| e.to_string())?;
        if let Ok(c) = ClassFile::parse(&b) {
            out.push(c);
        }
    }
    // Libraries the mod bundles (`META-INF/jarjar/*.jar`, NeoForge's jar-in-jar) run inside it:
    // translate them with it. A class the mod itself has wins.
    let have: std::collections::HashSet<String> = out.iter().map(|c| c.name.clone()).collect();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = e.name().map_err(|e| e.to_string())?.into_owned();
        if !(name.starts_with("META-INF/jarjar/") && name.ends_with(".jar")) {
            continue;
        }
        let mut b = Vec::new();
        e.read_to_end(&mut b).map_err(|e| e.to_string())?;
        let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) else { continue };
        for j in 0..inner.len() {
            let Ok(mut f) = inner.by_index(j) else { continue };
            let Ok(n) = f.name().map(|n| n.into_owned()) else { continue };
            if !n.ends_with(".class") || n.starts_with("META-INF/") {
                continue;
            }
            let mut cb = Vec::new();
            if f.read_to_end(&mut cb).is_ok()
                && let Ok(c) = ClassFile::parse(&cb)
                && !have.contains(&c.name)
            {
                out.push(c);
            }
        }
    }
    Ok(out)
}

/// `META-INF/services` of a jar and the jars it bundles: interface → implementations, in
/// internal names (`a/b/C`), as `ServiceLoader` finds them. The jar's own entries come first.
pub fn services(jar: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    fn read(zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>, out: &mut BTreeMap<String, Vec<String>>, nested: &mut Vec<Vec<u8>>) {
        for i in 0..zip.len() {
            let Ok(mut e) = zip.by_index(i) else { continue };
            let Ok(name) = e.name().map(|n| n.into_owned()) else { continue };
            let mut b = Vec::new();
            if name.starts_with("META-INF/jarjar/") && name.ends_with(".jar") {
                if e.read_to_end(&mut b).is_ok() {
                    nested.push(b);
                }
                continue;
            }
            let Some(iface) = name.strip_prefix("META-INF/services/").filter(|n| !n.is_empty() && !n.contains('/')) else { continue };
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
    let f = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let (mut out, mut nested) = (BTreeMap::new(), Vec::new());
    read(&mut zip, &mut out, &mut nested);
    for b in nested {
        if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) {
            read(&mut inner, &mut out, &mut Vec::new());
        }
    }
    Ok(out)
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
