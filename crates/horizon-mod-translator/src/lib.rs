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
