//! Block-state properties read from bytecode, for the content mod a bundle
//! carries: each registered block's class (the `new` in its registration's
//! supplier), the properties its `createBlockStateDefinition` adds, and what
//! each property is (a vanilla `BlockStateProperties` field, or one the mod
//! makes in a static initialiser with `BooleanProperty.create` and friends).
use crate::{
    class::{ClassFile, Const},
    insn::{self, Operand, op},
};
use std::collections::BTreeMap;

/// A property: its name and values (`["false", "true"]`, `["0", ..., "15"]`, `["north", ...]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    pub name: String,
    pub values: Vec<String>,
}

const DIRECTIONS: [&str; 6] = ["north", "east", "south", "west", "up", "down"];
const HORIZONTAL: [&str; 4] = ["north", "south", "west", "east"];

fn vanilla(field: &str) -> Option<Prop> {
    let (name, kind) = horizon_compat::api_common::vanilla_property(field)?;
    let values: Vec<String> = match (kind, field) {
        ("bool", _) => vec!["false".into(), "true".into()],
        ("direction", "HORIZONTAL_FACING") => HORIZONTAL.iter().map(|s| s.to_string()).collect(),
        ("direction", "FACING_HOPPER") => ["down", "north", "south", "west", "east"].iter().map(|s| s.to_string()).collect(),
        ("direction", "VERTICAL_DIRECTION") => vec!["up".into(), "down".into()],
        ("direction", _) => DIRECTIONS.iter().map(|s| s.to_string()).collect(),
        ("axis", "HORIZONTAL_AXIS") => vec!["x".into(), "z".into()],
        ("axis", _) => vec!["x".into(), "y".into(), "z".into()],
        ("int", f) => {
            let max = match f {
                "AGE_1" => 1,
                "AGE_2" => 2,
                "AGE_3" => 3,
                "AGE_4" => 4,
                "AGE_5" => 5,
                "AGE_7" => 7,
                "AGE_25" => 25,
                "POWER" | "AGE_15" | "LEVEL" | "ROTATION_16" | "LEVEL_FLOWING" => 15,
                "DISTANCE" | "MOISTURE" => 7,
                _ => return None,
            };
            let min = if f == "DISTANCE" { 1 } else { 0 };
            (min..=max).map(|i| i.to_string()).collect()
        }
        _ => return None,
    };
    Some(Prop { name, values })
}

fn string_at(c: &ClassFile, idx: i32) -> Option<String> {
    match c.pool.get(idx as usize)? {
        Const::String(u) => c.utf8(*u).ok(),
        _ => None,
    }
}

fn int_of(i: &insn::Insn, c: &ClassFile) -> Option<i32> {
    match (i.op, &i.arg) {
        (0x02..=0x08, _) => Some(i.op as i32 - 3),
        (op::BIPUSH | op::SIPUSH, Operand::Int(v)) => Some(*v),
        (op::LDC | op::LDC_W, Operand::Int(idx)) => match c.pool.get(*idx as usize)? {
            Const::Int(v) => Some(*v),
            _ => None,
        },
        _ => None,
    }
}

/// A mod enum's constant names, lower case, from its static initialiser.
fn enum_values(classes: &BTreeMap<&str, &ClassFile>, class: &str) -> Vec<String> {
    let Some(c) = classes.get(class) else { return Vec::new() };
    let Some(code) = c.methods.iter().find(|m| m.name == "<clinit>").and_then(|m| m.code.as_ref()) else {
        return Vec::new();
    };
    let insns = insn::decode(&code.bytes).unwrap_or_default();
    let mut out = Vec::new();
    for w in insns.windows(2) {
        // `new E; dup; ldc "NAME"; iconst n; ...; invokespecial E.<init>` — the first string after `new E`.
        if w[0].op == op::NEW
            && let Operand::Int(i) = w[0].arg
            && c.class_name(i as u16).is_ok_and(|n| n == class)
        {
            let at = insns.iter().position(|x| x.at == w[0].at).unwrap_or(0);
            if let Some(name) = insns[at..].iter().take(4).find_map(|x| match (x.op, &x.arg) {
                (op::LDC | op::LDC_W, Operand::Int(idx)) => string_at(c, *idx),
                _ => None,
            }) {
                out.push(name.to_ascii_lowercase());
            }
        }
    }
    out
}

/// The properties a mod's static fields hold, by `Owner.FIELD`.
fn mod_properties(classes: &BTreeMap<&str, &ClassFile>) -> BTreeMap<String, Prop> {
    let mut out = BTreeMap::new();
    for c in classes.values() {
        let Some(code) = c.methods.iter().find(|m| m.name == "<clinit>").and_then(|m| m.code.as_ref()) else { continue };
        let insns = insn::decode(&code.bytes).unwrap_or_default();
        for (n, i) in insns.iter().enumerate() {
            let Operand::Int(idx) = i.arg else { continue };
            // `static final Property AXIS = BlockStateProperties.AXIS;`: an alias of a vanilla property.
            if i.op == op::GETSTATIC
                && let Some(next) = insns.get(n + 1)
                && next.op == op::PUTSTATIC
                && let (Some(from), Operand::Int(to)) = (c.member(idx as u16), &next.arg)
                && let Some(to) = c.member(*to as u16)
                && from.owner.starts_with("net/minecraft/world/level/block/")
                && let Some(p) = vanilla(&from.name)
            {
                out.insert(format!("{}.{}", to.owner, to.name), p);
                continue;
            }
            if i.op != op::INVOKESTATIC {
                continue;
            }
            let Some(m) = c.member(idx as u16) else { continue };
            if m.name != "create" || !m.owner.starts_with("net/minecraft/world/level/block/state/properties/") {
                continue;
            }
            // The field the result goes to, and the arguments before the call.
            let Some(field) = insns[n + 1..].iter().take(2).find(|x| x.op == op::PUTSTATIC).and_then(|x| match x.arg {
                Operand::Int(f) => c.member(f as u16),
                _ => None,
            }) else {
                continue;
            };
            let before = &insns[n.saturating_sub(6)..n];
            // The name is the last string pushed before the call (an earlier one belongs to the
            // previous property).
            let Some(name) = before.iter().rev().find_map(|x| match (x.op, &x.arg) {
                (op::LDC | op::LDC_W, Operand::Int(idx)) => string_at(c, *idx),
                _ => None,
            }) else {
                continue;
            };
            let values: Vec<String> = if m.owner.ends_with("BooleanProperty") {
                vec!["false".into(), "true".into()]
            } else if m.owner.ends_with("IntegerProperty") {
                let ints: Vec<i32> = before.iter().filter_map(|x| int_of(x, c)).collect();
                match ints[..] {
                    [.., lo, hi] if lo <= hi && hi - lo < 64 => (lo..=hi).map(|v| v.to_string()).collect(),
                    _ => continue,
                }
            } else if m.owner.ends_with("DirectionProperty") {
                DIRECTIONS.iter().map(|s| s.to_string()).collect()
            } else {
                // EnumProperty.create(name, Enum.class): the class literal before the call.
                let class = before.iter().rev().find_map(|x| match (x.op, &x.arg) {
                    (op::LDC | op::LDC_W, Operand::Int(idx)) => match c.pool.get(*idx as usize) {
                        Some(Const::Class(n)) => c.utf8(*n).ok(),
                        _ => None,
                    },
                    _ => None,
                });
                match class.as_deref() {
                    Some("net/minecraft/core/Direction$Axis") => vec!["x".into(), "y".into(), "z".into()],
                    Some("net/minecraft/core/Direction") => DIRECTIONS.iter().map(|s| s.to_string()).collect(),
                    Some(e) => enum_values(classes, e),
                    None => continue,
                }
            };
            if !values.is_empty() {
                out.insert(format!("{}.{}", field.owner, field.name), Prop { name, values });
            }
        }
    }
    out
}

/// The properties a block class adds in `createBlockStateDefinition`, walking up its mod superclasses.
fn class_properties(classes: &BTreeMap<&str, &ClassFile>, mod_props: &BTreeMap<String, Prop>, class: &str) -> Vec<Prop> {
    let mut out: Vec<Prop> = Vec::new();
    let mut at = classes.get(class).copied();
    while let Some(c) = at {
        for m in c.methods.iter().filter(|m| m.name == "createBlockStateDefinition") {
            let Some(code) = &m.code else { continue };
            for i in insn::decode(&code.bytes).unwrap_or_default() {
                if i.op != op::GETSTATIC {
                    continue;
                }
                let Operand::Int(idx) = i.arg else { continue };
                let Some(f) = c.member(idx as u16) else { continue };
                let prop = mod_props.get(&format!("{}.{}", f.owner, f.name)).cloned().or_else(|| {
                    f.owner.starts_with("net/minecraft/world/level/block/").then(|| vanilla(&f.name)).flatten()
                });
                if let Some(p) = prop
                    && !out.iter().any(|q| q.name == p.name)
                {
                    out.push(p);
                }
            }
        }
        at = c.superclass.as_deref().and_then(|s| classes.get(s).copied());
    }
    out
}

/// The class each registered block is made as: the first `new` in its supplier lambda.
fn block_classes(classes: &[ClassFile]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for c in classes {
        for m in &c.methods {
            let Some(code) = &m.code else { continue };
            let insns = insn::decode(&code.bytes).unwrap_or_default();
            let mut last_name: Option<String> = None;
            let mut last_lambda: Option<(String, String)> = None;
            for i in &insns {
                match (i.op, &i.arg) {
                    (op::LDC | op::LDC_W, Operand::Int(idx)) => {
                        if let Some(s) = string_at(c, *idx) {
                            last_name = Some(s);
                        }
                    }
                    (op::INVOKEDYNAMIC, Operand::Int(idx)) => {
                        let Some((bsm, _, _)) = c.indy(*idx as u16) else { continue };
                        let Some((_, args)) = c.bootstrap.get(bsm as usize) else { continue };
                        if let Some((kind, h)) = args.get(1).and_then(|&a| c.handle(a)) {
                            // A constructor reference (`ShaftBlock::new`, handle kind 8) names its class.
                            last_lambda = Some(if kind == 8 {
                                (h.owner.clone(), String::new())
                            } else {
                                (h.owner.clone(), format!("{}{}", h.name, h.desc))
                            });
                        }
                    }
                    (op::INVOKEVIRTUAL | op::INVOKESTATIC, Operand::Int(idx)) => {
                        let Some(call) = c.member(*idx as u16) else { continue };
                        let deferred = call.name.starts_with("register") && call.owner.contains("DeferredRegister");
                        let registrate = call.name == "block" && call.owner.contains("Registrate");
                        if !(deferred || registrate) {
                            continue;
                        }
                        if let (Some(name), Some((owner, method))) = (last_name.take(), last_lambda.take()) {
                            let made = if method.is_empty() { Some(owner) } else { first_new(classes, &owner, &method) };
                            if let Some(made) = made {
                                out.insert(name, made);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn first_new(classes: &[ClassFile], owner: &str, method: &str) -> Option<String> {
    let c = classes.iter().find(|c| c.name == owner)?;
    let m = c.methods.iter().find(|m| format!("{}{}", m.name, m.desc) == method)?;
    let insns = insn::decode(&m.code.as_ref()?.bytes).ok()?;
    insns.iter().find(|i| i.op == op::NEW).and_then(|i| match i.arg {
        Operand::Int(idx) => c.class_name(idx as u16).ok(),
        _ => None,
    })
}

/// Each registered block name (without namespace) → the state properties its class adds.
pub fn block_properties(classes: &[ClassFile]) -> BTreeMap<String, Vec<Prop>> {
    let by_name: BTreeMap<&str, &ClassFile> = classes.iter().map(|c| (c.name.as_str(), c)).collect();
    let mod_props = mod_properties(&by_name);
    block_classes(classes)
        .into_iter()
        .map(|(block, class)| {
            let props = class_properties(&by_name, &mod_props, &class);
            (block, props)
        })
        .filter(|(_, p)| !p.is_empty())
        .collect()
}
