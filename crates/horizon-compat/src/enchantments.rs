//! Enchantments on items, natively: `ItemEnchantments` (levels by enchantment, and whether the
//! tooltip shows them), its `Mutable` and codec, the stack methods that read and write them
//! (`EnchantmentHelper.getEnchantmentsForCrafting`, `ItemStack.enchant`...), and an enchantment's
//! line in a tooltip (`Enchantment.getFullname`).
//!
//! An enchantment is its registry entry here: a holder of registry `enchantment` named by its key
//! (`lookup.getOrThrow(Enchantments.EFFICIENCY)`, one per enchantment), whose value knows its name,
//! its description and, for vanilla's, its maximum level. An `ItemEnchantments` keeps its levels by
//! name, sorted (vanilla's hash map has no order a mod can count on), and is one object per
//! content, so equal ones are the same object, as `equals` has them.
use crate::nbt::Nbt;
use crate::{Data, Rt, V};
use serde_json::Value as Json;
use std::collections::BTreeMap;

pub(crate) const ITEM_ENCHANTMENTS: &str = "net/minecraft/world/item/enchantment/ItemEnchantments";
const MUTABLE: &str = "net/minecraft/world/item/enchantment/ItemEnchantments$Mutable";
const ENCHANTMENT: &str = "net/minecraft/world/item/enchantment/Enchantment";
const HELPER: &str = "net/minecraft/world/item/enchantment/EnchantmentHelper";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const STACK_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IItemStackExtension";
const ENCHANTMENTS: &str = "minecraft:enchantments";
const ITEM: &str = "net/minecraft/world/item/Item";
/// `Item.isFoil(stack)`, which a mod's item may override.
const IS_FOIL: &str = "isFoil.(Lnet/minecraft/world/item/ItemStack;)Z";
const IS_FOIL_KEY: &str =
    "net/minecraft/world/item/Item.isFoil.(Lnet/minecraft/world/item/ItemStack;)Z";
const GLINT: &str = "minecraft:enchantment_glint_override";
/// Vanilla's items whose `enchantment_glint_override` is `true` unless the stack says otherwise
/// (1.21.1's `Items`).
const SHINING: [&str; 7] = [
    "minecraft:enchanted_golden_apple",
    "minecraft:experience_bottle",
    "minecraft:written_book",
    "minecraft:nether_star",
    "minecraft:enchanted_book",
    "minecraft:end_crystal",
    "minecraft:debug_stick",
];
const STORED: &str = "minecraft:stored_enchantments";
const PREDICATE: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const CONSUMER: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const BI_CONSUMER: &str = "java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V";
const VISITOR: &str = "net/minecraft/world/item/enchantment/EnchantmentHelper$EnchantmentVisitor.accept.(Lnet/minecraft/core/Holder;I)V";

/// 1.21.1's enchantments and their maximum levels (`data/minecraft/enchantment/*.json`).
const MAX_LEVELS: [(&str, i32); 42] = [
    ("aqua_affinity", 1), ("bane_of_arthropods", 5), ("binding_curse", 1), ("blast_protection", 4), ("breach", 4), ("channeling", 1),
    ("density", 5), ("depth_strider", 3), ("efficiency", 5), ("feather_falling", 4), ("fire_aspect", 2), ("fire_protection", 4), ("flame", 1),
    ("fortune", 3), ("frost_walker", 2), ("impaling", 5), ("infinity", 1), ("knockback", 2), ("looting", 3), ("loyalty", 3),
    ("luck_of_the_sea", 3), ("lure", 3), ("mending", 1), ("multishot", 1), ("piercing", 4), ("power", 5), ("projectile_protection", 4),
    ("protection", 4), ("punch", 2), ("quick_charge", 3), ("respiration", 3), ("riptide", 3), ("sharpness", 5), ("silk_touch", 1),
    ("smite", 5), ("soul_speed", 3), ("sweeping_edge", 3), ("swift_sneak", 3), ("thorns", 3), ("unbreaking", 3), ("vanishing_curse", 1),
    ("wind_burst", 3),
];

/// How an enchantment's attribute modifier grows with its level: the `LevelBasedValue`s 1.21.1's
/// enchantments give theirs by.
#[derive(Clone, Copy)]
enum Amount {
    /// `base + per_level_above_first * (level - 1)`.
    Linear(f32, f32),
    /// `level * level + added`.
    LevelsSquared(f32),
    /// One linear amount over another, 0 where the second is.
    Fraction((f32, f32), (f32, f32)),
}

impl Amount {
    fn at(self, level: i32) -> f32 {
        let linear = |(base, per): (f32, f32)| base + per * (level - 1) as f32;
        match self {
            Amount::Linear(base, per) => linear((base, per)),
            Amount::LevelsSquared(added) => (level * level) as f32 + added,
            Amount::Fraction(n, d) => match linear(d) {
                0.0 => 0.0,
                d => linear(n) / d,
            },
        }
    }
}

/// 1.21.1's enchantments that give attribute modifiers (`effects."minecraft:attributes"` in
/// `data/minecraft/enchantment/*.json`): the enchantment, the slot groups it works in, the
/// attribute, the modifier's id (the slot's name goes after it), its amount and its operation's
/// ordinal.
const ATTRIBUTE_EFFECTS: [(&str, &[&str], &str, &str, Amount, u8); 8] = [
    ("aqua_affinity", &["head"], "minecraft:player.submerged_mining_speed", "minecraft:enchantment.aqua_affinity", Amount::Linear(4.0, 4.0), 2),
    ("blast_protection", &["armor"], "minecraft:generic.explosion_knockback_resistance", "minecraft:enchantment.blast_protection", Amount::Linear(0.15, 0.15), 0),
    ("depth_strider", &["feet"], "minecraft:generic.water_movement_efficiency", "minecraft:enchantment.depth_strider", Amount::Linear(0.333_333_34, 0.333_333_34), 0),
    ("efficiency", &["mainhand"], "minecraft:player.mining_efficiency", "minecraft:enchantment.efficiency", Amount::LevelsSquared(1.0), 0),
    ("fire_protection", &["armor"], "minecraft:generic.burning_time", "minecraft:enchantment.fire_protection", Amount::Linear(-0.15, -0.15), 1),
    ("respiration", &["head"], "minecraft:generic.oxygen_bonus", "minecraft:enchantment.respiration", Amount::Linear(1.0, 1.0), 0),
    ("sweeping_edge", &["mainhand"], "minecraft:player.sweeping_damage_ratio", "minecraft:enchantment.sweeping_edge", Amount::Fraction((1.0, 1.0), (2.0, 1.0)), 0),
    ("swift_sneak", &["legs"], "minecraft:player.sneaking_speed", "minecraft:enchantment.swift_sneak", Amount::Linear(0.15, 0.15), 0),
];

/// Whether slot group `group` holds equipment slot `slot` (both by their names), as
/// `EquipmentSlotGroup.test` has it.
fn group_holds(group: &str, slot: &str) -> bool {
    match group {
        "any" => true,
        "hand" => matches!(slot, "mainhand" | "offhand"),
        "armor" => matches!(slot, "feet" | "legs" | "chest" | "head" | "body"),
        g => g == slot,
    }
}

/// `#minecraft:curse`, which a tooltip shows red.
const CURSES: [&str; 2] = ["minecraft:binding_curse", "minecraft:vanishing_curse"];

/// Levels by enchantment name.
type Levels = Vec<(String, i32)>;

/// Vanilla enchantment `name`'s maximum level; None for one vanilla doesn't have.
fn max_level(name: &str) -> Option<i32> {
    let path = name.strip_prefix("minecraft:")?;
    MAX_LEVELS.iter().find(|(n, _)| *n == path).map(|&(_, l)| l)
}

/// Enchantment `name`'s value: its name, and its description as vanilla's data gives one
/// (`enchantment.<namespace>.<path>`); one object per enchantment.
pub(crate) fn value(rt: &mut Rt, name: &str) -> V {
    rt.intern(format!("compat:enchantment/{name}"), |rt| {
        let o = rt.new_object(ENCHANTMENT);
        let s = rt.string(name);
        rt.put_field(o, "$id", s);
        let d = description(rt, name);
        rt.put_field(o, "description", d);
        o
    })
}

fn description(rt: &mut Rt, name: &str) -> V {
    let key = rt.string(&format!("enchantment.{}", name.replace(':', ".")));
    crate::api_common::component(rt, key, true, V::NULL)
}

/// The holder of enchantment `name`: the registry's, one per enchantment.
fn holder(rt: &mut Rt, name: &str) -> V {
    let v = value(rt, name);
    crate::neo::registry_holder(rt, "enchantment", name, v)
}

/// The enchantment `h` names: a registry holder's entry, a direct holder's value's, or a key's.
fn name_of(rt: &Rt, h: V) -> Option<String> {
    if h.r() == 0 {
        return None;
    }
    if let Some(id) = rt.str_of(rt.get_field(h, "$id")).filter(|id| !id.is_empty()) {
        return Some(id.to_owned());
    }
    let direct = rt.get_field(h, "value");
    if direct.r() != 0
        && let Some(id) = rt.str_of(rt.get_field(direct, "$id"))
    {
        return Some(id.to_owned());
    }
    let key = rt.str_of(h)?;
    Some(key.split_once('#').map_or(key, |(_, l)| l).to_owned())
}

/// The levels `v` (an `ItemEnchantments` or its `Mutable`) holds, sorted by name.
pub(crate) fn levels(rt: &Rt, v: V) -> Levels {
    if v.r() == 0 {
        return Levels::new();
    }
    let text = rt.str_of(rt.get_field(v, "$levels")).unwrap_or("");
    text.split(',')
        .filter_map(|e| {
            let (name, level) = e.rsplit_once('=')?;
            Some((name.to_owned(), level.parse().ok()?))
        })
        .collect()
}

fn text(levels: &Levels) -> String {
    levels.iter().map(|(n, l)| format!("{n}={l}")).collect::<Vec<_>>().join(",")
}

fn shown(rt: &Rt, v: V) -> bool {
    rt.get_field(v, "$hidden").i() == 0
}

fn level(levels: &Levels, name: &str) -> i32 {
    levels.iter().find(|(n, _)| n == name).map_or(0, |&(_, l)| l)
}

/// `Mutable.set`: 0 or less removes, more is capped at 255.
fn set_level(levels: &mut Levels, name: &str, level: i32) {
    levels.retain(|(n, _)| n != name);
    if level > 0 {
        levels.push((name.to_owned(), level.min(255)));
        levels.sort();
    }
}

/// `Mutable.upgrade`: the higher of the two levels, for a level above 0.
fn upgrade(levels: &mut Levels, name: &str, to: i32) {
    if to > 0 {
        let higher = level(levels, name).max(to.min(255));
        set_level(levels, name, higher);
    }
}

/// The `ItemEnchantments` of `levels`, shown in the tooltip or not: one object per content.
pub(crate) fn made(rt: &mut Rt, mut levels: Levels, show: bool) -> V {
    levels.sort();
    levels.dedup_by(|a, b| a.0 == b.0);
    let t = text(&levels);
    rt.intern(format!("compat:item-enchantments/{}{t}", if show { "" } else { "hidden:" }), |rt| {
        let o = rt.new_object(ITEM_ENCHANTMENTS);
        let s = rt.string(&t);
        rt.put_field(o, "$levels", s);
        rt.put_field(o, "$hidden", V::I(!show as i32));
        o
    })
}

/// `ItemEnchantments.EMPTY`.
pub(crate) fn empty(rt: &mut Rt) -> V {
    made(rt, Levels::new(), true)
}

/// The component an enchanted book keeps its enchantments in, `stored_enchantments`, or every
/// other item's, `enchantments` (`EnchantmentHelper.getComponentType`).
fn component_of(rt: &Rt, stack: V) -> &'static str {
    if crate::api_common::item_like_name(rt, rt.get_field(stack, "$item")) == "minecraft:enchanted_book" { STORED } else { ENCHANTMENTS }
}

/// `ItemStack.hasFoil()`.
fn has_foil(rt: &mut Rt, stack: V) -> bool {
    if let Some(v) = crate::components::vanilla(rt, stack, GLINT)
        && !v.is_null()
    {
        return match v {
            V::R(_) => rt.get_field(v, "value").i() != 0,
            raw => raw.i() != 0,
        };
    }
    let item = rt.get_field(stack, "$item");
    if SHINING.contains(&crate::api_common::item_like_name(rt, item).as_str()) {
        return true;
    }
    if rt.class_overrides(rt.class_of(item), IS_FOIL)
        && let Some(vc) = rt.vcall
    {
        return vc(rt, IS_FOIL_KEY, IS_FOIL, &[item, stack]).i() != 0;
    }
    item_foil(rt, item, stack)
}

/// Vanilla's `Item.isFoil(stack)`: enchanted, or (`CompassItem`'s) a compass tracking a lodestone.
fn item_foil(rt: &mut Rt, item: V, stack: V) -> bool {
    if crate::api_common::item_like_name(rt, item) == "minecraft:compass"
        && crate::components::vanilla(rt, stack, "minecraft:lodestone_tracker")
            .is_some_and(|v| !v.is_null())
    {
        return true;
    }
    let e = on_stack(rt, stack, ENCHANTMENTS);
    !levels(rt, e).is_empty()
}

/// What `stack` holds of component `id`, `EMPTY` for none.
fn on_stack(rt: &mut Rt, stack: V, id: &str) -> V {
    crate::components::vanilla(rt, stack, id).unwrap_or_else(|| empty(rt))
}

/// `EnchantmentHelper.updateEnchantments(stack, change)`: `change` applied to the levels the stack
/// keeps (its book's or its own), set back; what was set.
fn update(rt: &mut Rt, stack: V, change: impl FnOnce(&mut Rt, &mut Levels)) -> V {
    let id = component_of(rt, stack);
    let before = on_stack(rt, stack, id);
    let mut levels = levels(rt, before);
    change(rt, &mut levels);
    let after = made(rt, levels, shown(rt, before));
    crate::components::set_vanilla(rt, stack, id, after);
    after
}

/// An enchantment's line in a tooltip (`Enchantment.getFullname`): its description, red for a
/// curse and grey otherwise, and the level unless the enchantment has only one.
fn fullname(rt: &mut Rt, name: &str, level: i32) -> V {
    let c = description(rt, name);
    let colour = if CURSES.contains(&name) { "RED," } else { "GRAY," };
    let style = rt.string(colour);
    rt.put_field(c, "$style", style);
    if level != 1 || max_level(name) != Some(1) {
        let (space, key) = (rt.string(" "), rt.string(&format!("enchantment.level.{level}")));
        let space = crate::api_common::component(rt, space, false, V::NULL);
        let number = crate::api_common::component(rt, key, true, V::NULL);
        let extra = crate::jutil::new_list(rt, vec![space, number]);
        rt.put_field(c, "$extra", extra);
    }
    c
}

/// `ItemEnchantments.CODEC` reading: `{levels, show_in_tooltip}`, else the levels alone (its
/// `withAlternative`); each level 0 to 255, under an enchantment's name.
pub(crate) fn decode(rt: &mut Rt, j: &Json) -> Result<V, String> {
    let o = j.as_object().ok_or_else(|| format!("Not a map: {j}"))?;
    let (map, show) = match o.get("levels") {
        Some(Json::Object(levels)) => {
            let show = match o.get("show_in_tooltip") {
                None => true,
                Some(Json::Bool(b)) => *b,
                Some(Json::Number(n)) => n.as_f64() != Some(0.0),
                Some(other) => return Err(format!("Not a boolean: {other}")),
            };
            (levels, show)
        }
        _ => (o, true),
    };
    let mut levels = Levels::new();
    for (name, n) in map {
        let n = n.as_i64().ok_or_else(|| format!("Not a number: {n}"))?;
        if !(0..=255).contains(&n) {
            return Err(format!("Value {n} outside of range [0:255]"));
        }
        if !crate::recipes::valid_location(name) {
            return Err(format!("Not a valid resource location: {name}"));
        }
        let name = if name.contains(':') { name.clone() } else { format!("minecraft:{name}") };
        levels.push((name, n as i32));
    }
    Ok(made(rt, levels, show))
}

/// `ItemEnchantments.CODEC` writing: `{levels}`, and `show_in_tooltip` when it isn't shown (its
/// first form, without the default).
pub(crate) fn encode(rt: &Rt, v: V) -> Result<Nbt, String> {
    if rt.class_of(v) != ITEM_ENCHANTMENTS {
        return Err("Not an ItemEnchantments".into());
    }
    let levels: BTreeMap<String, Nbt> = levels(rt, v).into_iter().map(|(n, l)| (n, Nbt::Int(l))).collect();
    let mut m = BTreeMap::from([("levels".to_owned(), Nbt::Compound(levels))]);
    if !shown(rt, v) {
        m.insert("show_in_tooltip".to_owned(), Nbt::Byte(0));
    }
    Ok(Nbt::Compound(m))
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (ITEM_ENCHANTMENTS, "EMPTY") => Some(empty(rt)),
        (ITEM_ENCHANTMENTS, "CODEC") => Some(crate::codecs::native(rt, "item_enchantments")),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, ITEM_ENCHANTMENTS | MUTABLE | HELPER)
        || (owner == ENCHANTMENT && matches!(name, "getFullname" | "getMaxLevel" | "description"))
        || (matches!(owner, ITEM_STACK | STACK_EXTENSION)
            && matches!(
                name,
                "getEnchantments"
                    | "getTagEnchantments"
                    | "getAllEnchantments"
                    | "isEnchanted"
                    | "enchant"
                    | "getEnchantmentLevel"
                    | "hasFoil"
            ))
        || (owner == ITEM && name == "isFoil")
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    Some(match (owner, name) {
        (ITEM_ENCHANTMENTS | MUTABLE, "getLevel") => {
            let n = name_of(rt, a(1)).unwrap_or_default();
            V::I(level(&levels(rt, a(0)), &n))
        }
        (ITEM_ENCHANTMENTS, "size") => V::I(levels(rt, a(0)).len() as i32),
        (ITEM_ENCHANTMENTS, "isEmpty") => V::I(levels(rt, a(0)).is_empty() as i32),
        (ITEM_ENCHANTMENTS, "withTooltip") => {
            let l = levels(rt, a(0));
            made(rt, l, a(1).i() != 0)
        }
        (ITEM_ENCHANTMENTS | MUTABLE, "keySet") => {
            let names = levels(rt, a(0));
            let hs: Vec<V> = names.iter().map(|(n, _)| holder(rt, n)).collect();
            crate::jutil::new_set_of(rt, &hs)
        }
        // `Object2IntMap.Entry`s: `getKey()` the holder, `getIntValue()` the level.
        (ITEM_ENCHANTMENTS, "entrySet") => {
            let names = levels(rt, a(0));
            let mut es = Vec::new();
            for (n, l) in names {
                let h = holder(rt, &n);
                es.push(rt.alloc("java/util/Map$Entry", Data::List(vec![h, V::I(l)])));
            }
            crate::jutil::new_set_of(rt, &es)
        }
        // One object per content: equal is the same.
        (ITEM_ENCHANTMENTS, "equals") => V::I((a(0) == a(1)) as i32),
        (ITEM_ENCHANTMENTS, "hashCode") => {
            let t = rt.get_field(a(0), "$levels");
            V::I(rt.java_hash(t).wrapping_mul(31).wrapping_add(shown(rt, a(0)) as i32))
        }
        (ITEM_ENCHANTMENTS, "toString") => {
            let l = levels(rt, a(0)).iter().map(|(n, l)| format!("Reference{{ResourceKey[minecraft:enchantment / {n}]}}={l}")).collect::<Vec<_>>().join(", ");
            let s = format!("ItemEnchantments{{enchantments={{{l}}}, showInTooltip={}}}", shown(rt, a(0)));
            rt.string(&s)
        }
        (MUTABLE, "<init>") => {
            let (t, hidden) = (rt.get_field(a(1), "$levels"), rt.get_field(a(1), "$hidden").i());
            let t = if t.r() == 0 { rt.string("") } else { t };
            rt.put_field(a(0), "$levels", t);
            rt.put_field(a(0), "$hidden", V::I(hidden));
            V::Void
        }
        (MUTABLE, "set" | "upgrade") => {
            let n = name_of(rt, a(1)).unwrap_or_default();
            let mut l = levels(rt, a(0));
            if name == "set" { set_level(&mut l, &n, a(2).i()) } else { upgrade(&mut l, &n, a(2).i()) }
            let t = rt.string(&text(&l));
            rt.put_field(a(0), "$levels", t);
            V::Void
        }
        (MUTABLE, "removeIf") => {
            let mut kept = Levels::new();
            for (n, l) in levels(rt, a(0)) {
                let h = holder(rt, &n);
                if rt.call_functional(PREDICATE, a(1), &[h]).i() == 0 {
                    kept.push((n, l));
                }
            }
            let t = rt.string(&text(&kept));
            rt.put_field(a(0), "$levels", t);
            V::Void
        }
        (MUTABLE, "toImmutable") => {
            let l = levels(rt, a(0));
            let show = shown(rt, a(0));
            made(rt, l, show)
        }
        (ENCHANTMENT, "getFullname") => {
            let n = name_of(rt, a(0)).unwrap_or_default();
            fullname(rt, &n, a(1).i())
        }
        (ENCHANTMENT, "getMaxLevel") => {
            let n = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned();
            V::I(max_level(&n).unwrap_or(1))
        }
        (ENCHANTMENT, "description") if rt.get_field(a(0), "$id").r() != 0 => rt.get_field(a(0), "description"),
        // The stack's own enchantments (`getTagEnchantments`, as NeoForge renamed vanilla's
        // `getEnchantments`; the gameplay ones are the same here).
        (ITEM_STACK | STACK_EXTENSION, "getEnchantments" | "getTagEnchantments" | "getAllEnchantments") => on_stack(rt, a(0), ENCHANTMENTS),
        (ITEM_STACK | STACK_EXTENSION, "isEnchanted") => {
            let e = on_stack(rt, a(0), ENCHANTMENTS);
            V::I(!levels(rt, e).is_empty() as i32)
        }
        // Whether the stack is drawn with the enchantment glint: its `enchantment_glint_override`
        // where it carries one (vanilla's nether star, written book... by default), else its
        // item's `isFoil(stack)`, a mod item's own where it has one.
        (ITEM_STACK | STACK_EXTENSION, "hasFoil") => V::I(has_foil(rt, a(0)) as i32),
        (ITEM, "isFoil") => V::I(item_foil(rt, a(0), a(1)) as i32),
        (ITEM_STACK | STACK_EXTENSION, "getEnchantmentLevel") => {
            let n = name_of(rt, a(1)).unwrap_or_default();
            let e = on_stack(rt, a(0), ENCHANTMENTS);
            V::I(level(&levels(rt, e), &n))
        }
        (ITEM_STACK | STACK_EXTENSION, "enchant") => {
            let (n, to) = (name_of(rt, a(1)).unwrap_or_default(), a(2).i());
            update(rt, a(0), |_, l| upgrade(l, &n, to));
            V::Void
        }
        // `processBlockExperience(level, tool, experience)`: what a block's break drops after the
        // tool's enchantments; of vanilla's, silk touch's `block_experience` effect sets it to 0.
        (HELPER, "processBlockExperience") => {
            let e = on_stack(rt, a(1), ENCHANTMENTS);
            V::I(if level(&levels(rt, e), "minecraft:silk_touch") > 0 { 0 } else { a(2).i() })
        }
        (HELPER, "getItemEnchantmentLevel" | "getTagEnchantmentLevel") => {
            let n = name_of(rt, a(0)).unwrap_or_default();
            let e = on_stack(rt, a(1), ENCHANTMENTS);
            V::I(level(&levels(rt, e), &n))
        }
        (HELPER, "getEnchantmentsForCrafting") => {
            let id = component_of(rt, a(0));
            on_stack(rt, a(0), id)
        }
        (HELPER, "setEnchantments") => {
            let id = component_of(rt, a(0));
            crate::components::set_vanilla(rt, a(0), id, a(1));
            V::Void
        }
        (HELPER, "updateEnchantments") => {
            let change = a(1);
            update(rt, a(0), |rt, l| {
                let m = rt.new_object(MUTABLE);
                let t = rt.string(&text(l));
                rt.put_field(m, "$levels", t);
                rt.call_functional(CONSUMER, change, &[m]);
                *l = levels(rt, m);
            })
        }
        (HELPER, "hasAnyEnchantments") => {
            let any = [ENCHANTMENTS, STORED].iter().any(|id| {
                let e = crate::components::vanilla(rt, a(0), id).unwrap_or(V::NULL);
                !levels(rt, e).is_empty()
            });
            V::I(any as i32)
        }
        (HELPER, "canStoreEnchantments") => {
            let id = component_of(rt, a(0));
            V::I(crate::components::vanilla(rt, a(0), id).is_some() as i32)
        }
        // `doPostAttackEffects(level, victim, source)`: the enchantments' effects after a blow, as
        // the game runs them (`Host::post_attack_effects`): the victim's worn ones, and those of the
        // weapon of the living entity the blow is credited to. `doPostAttackEffectsWithItemSource`
        // names that weapon (none: the victim's only). None for an entity of a mod's own.
        (HELPER, "doPostAttackEffects" | "doPostAttackEffectsWithItemSource") => {
            if let Some(victim) = crate::damage::struck(rt, a(1)) {
                let by = crate::damage::blow_of(rt, a(2));
                let weapon = match name {
                    "doPostAttackEffects" => crate::host::AttackWeapon::Held,
                    _ => crate::block_breaking::held(rt, a(3)).map_or(crate::host::AttackWeapon::None, crate::host::AttackWeapon::Stack),
                };
                rt.host_call(|h| h.post_attack_effects(victim, &by, &weapon));
            }
            V::Void
        }
        // `runIterationOnItem(stack, visitor)`: `visitor(enchantment, level)` for each enchantment
        // on the stack.
        (HELPER, "runIterationOnItem") if desc.ends_with("$EnchantmentVisitor;)V") => {
            let on = on_stack(rt, a(0), ENCHANTMENTS);
            for (name, level) in levels(rt, on) {
                let h = holder(rt, &name);
                rt.call_functional(VISITOR, a(1), &[h, V::I(level)]);
            }
            V::Void
        }
        // `forEachModifier(stack, slot, action)`: `action(attribute, modifier)` for each modifier
        // the stack's enchantments give in `slot`: an `EquipmentSlot` one of the enchantment's slot
        // groups holds, or an `EquipmentSlotGroup` it names. The modifier's id is the effect's with
        // the slot's name after a slash.
        (HELPER, "forEachModifier") => {
            let slot = rt.str_of(rt.get_field(a(1), "$name")).unwrap_or("").to_ascii_lowercase();
            let group = desc.contains("EquipmentSlotGroup;");
            let on = on_stack(rt, a(0), ENCHANTMENTS);
            for (name, level) in levels(rt, on) {
                let path = name.strip_prefix("minecraft:").unwrap_or("");
                for &(_, groups, attribute, id, amount, operation) in ATTRIBUTE_EFFECTS.iter().filter(|e| e.0 == path) {
                    let applies = if group { groups.contains(&slot.as_str()) } else { groups.iter().any(|g| group_holds(g, &slot)) };
                    if !applies {
                        continue;
                    }
                    let holder = crate::attributes::attribute(rt, attribute);
                    let view = crate::host::AttributeModifierView { id: format!("{id}/{slot}"), amount: f64::from(amount.at(level)), operation, permanent: false };
                    let modifier = crate::attributes::modifier(rt, &view);
                    rt.call_functional(BI_CONSUMER, a(2), &[holder, modifier]);
                }
            }
            V::Void
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    const EMPTY: &str = "net/minecraft/world/item/enchantment/ItemEnchantments.EMPTY";
    const GET_LEVEL: &str = "net/minecraft/world/item/enchantment/ItemEnchantments.getLevel.(Lnet/minecraft/core/Holder;)I";
    const IS_EMPTY: &str = "net/minecraft/world/item/enchantment/ItemEnchantments.isEmpty.()Z";

    /// `level.holderLookup(Registries.ENCHANTMENT).getOrThrow(key)`, as Create Enchantable Machinery
    /// asks for the enchantments its machines read.
    fn enchantment(rt: &mut Rt, field: &'static str) -> V {
        let level = rt.new_object("net/minecraft/world/level/Level");
        let registry = api::get_static(rt, "net/minecraft/core/registries/Registries.ENCHANTMENT");
        let lookup = api::call(rt, "net/minecraft/world/level/Level.holderLookup.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/HolderLookup;", &[level, registry]);
        let key = api::get_static(rt, field);
        api::call(rt, "net/minecraft/core/HolderLookup.getOrThrow.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/Holder$Reference;", &[lookup, key])
    }

    fn names(rt: &Rt, v: V) -> Vec<(String, i32)> {
        levels(rt, v)
    }

    /// The enchantable machines' block entity: `EMPTY` to start, levels set through a `Mutable`,
    /// written to its NBT with `ItemEnchantments.CODEC` and read back as the same enchantments.
    #[test]
    fn a_block_entitys_enchantments_are_set_saved_and_read_back() {
        let mut rt = Rt::new();
        crate::install();
        let none = api::get_static(&mut rt, EMPTY);
        assert!(none.r() != 0);
        assert_eq!(api::get_static(&mut rt, EMPTY), none, "one EMPTY");
        assert_eq!(api::call(&mut rt, IS_EMPTY, &[none]), V::I(1));

        let efficiency = enchantment(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.EFFICIENCY");
        assert_eq!(enchantment(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.EFFICIENCY"), efficiency, "one holder per enchantment");
        let name = api::call(&mut rt, "net/minecraft/core/Holder.getRegisteredName.()Ljava/lang/String;", &[efficiency]);
        assert_eq!(rt.str_of(name), Some("minecraft:efficiency"));
        let unbreaking = enchantment(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.UNBREAKING");
        assert_eq!(api::call(&mut rt, GET_LEVEL, &[none, efficiency]), V::I(0));

        let m = rt.new_object(MUTABLE);
        api::call(&mut rt, "net/minecraft/world/item/enchantment/ItemEnchantments$Mutable.<init>.(Lnet/minecraft/world/item/enchantment/ItemEnchantments;)V", &[m, none]);
        let set = "net/minecraft/world/item/enchantment/ItemEnchantments$Mutable.set.(Lnet/minecraft/core/Holder;I)V";
        let up = "net/minecraft/world/item/enchantment/ItemEnchantments$Mutable.upgrade.(Lnet/minecraft/core/Holder;I)V";
        api::call(&mut rt, set, &[m, efficiency, V::I(3)]);
        api::call(&mut rt, up, &[m, efficiency, V::I(2)]);
        api::call(&mut rt, up, &[m, unbreaking, V::I(300)]);
        let built = api::call(&mut rt, "net/minecraft/world/item/enchantment/ItemEnchantments$Mutable.toImmutable.()Lnet/minecraft/world/item/enchantment/ItemEnchantments;", &[m]);
        assert_eq!(api::call(&mut rt, GET_LEVEL, &[built, efficiency]), V::I(3), "an upgrade to a lower level keeps the higher");
        assert_eq!(api::call(&mut rt, GET_LEVEL, &[built, unbreaking]), V::I(255), "levels stop at 255");
        assert_eq!(api::call(&mut rt, IS_EMPTY, &[built]), V::I(0));
        assert_eq!(api::call(&mut rt, IS_EMPTY, &[none]), V::I(1), "EMPTY is left as it was");

        let codec = api::get_static(&mut rt, "net/minecraft/world/item/enchantment/ItemEnchantments.CODEC");
        let saved = crate::codecs::encode(&mut rt, codec, built).unwrap();
        let levels = Nbt::Compound(BTreeMap::from([("minecraft:efficiency".to_owned(), Nbt::Int(3)), ("minecraft:unbreaking".to_owned(), Nbt::Int(255))]));
        assert_eq!(saved, Nbt::Compound(BTreeMap::from([("levels".to_owned(), levels)])));
        let read = crate::codecs::decode(&mut rt, codec, &crate::recipes::nbt_json(&saved)).unwrap();
        assert_eq!(read, built, "read back, the same enchantments");

        // The levels alone (the codec's other form), and a hidden tooltip, which is written.
        let flat = crate::codecs::decode(&mut rt, codec, &serde_json::json!({"sharpness": 2})).unwrap();
        assert_eq!(names(&rt, flat), vec![("minecraft:sharpness".to_owned(), 2)]);
        let hidden = crate::codecs::decode(&mut rt, codec, &serde_json::json!({"levels": {"minecraft:sharpness": 2}, "show_in_tooltip": 0})).unwrap();
        assert_ne!(hidden, flat);
        let Nbt::Compound(m) = crate::codecs::encode(&mut rt, codec, hidden).unwrap() else { panic!() };
        assert_eq!(m.get("show_in_tooltip"), Some(&Nbt::Byte(0)));
        assert!(crate::codecs::decode(&mut rt, codec, &serde_json::json!({"minecraft:sharpness": 256})).is_err());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A stack's enchantments: none to start (`EMPTY`, every item's default), `enchant` raising a
    /// level, a book keeping its own in `stored_enchantments`; read through the helper, the stack and
    /// its components.
    #[test]
    fn a_stacks_enchantments_are_its_components() {
        let mut rt = Rt::new();
        crate::install();
        let efficiency = enchantment(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.EFFICIENCY");
        let pick = crate::api_common::stack(&mut rt, "minecraft:diamond_pickaxe", 1);
        let enchanted = "net/minecraft/world/item/ItemStack.isEnchanted.()Z";
        let level = "net/minecraft/world/item/ItemStack.getEnchantmentLevel.(Lnet/minecraft/core/Holder;)I";
        let enchant = "net/minecraft/world/item/ItemStack.enchant.(Lnet/minecraft/core/Holder;I)V";
        let crafting = "net/minecraft/world/item/enchantment/EnchantmentHelper.getEnchantmentsForCrafting.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/enchantment/ItemEnchantments;";
        let get = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let component = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.ENCHANTMENTS");
        let none = api::get_static(&mut rt, EMPTY);

        assert_eq!(api::call(&mut rt, get, &[pick, component]), none, "every item carries EMPTY");
        assert_eq!(api::call(&mut rt, enchanted, &[pick]), V::I(0));
        api::call(&mut rt, enchant, &[pick, efficiency, V::I(2)]);
        api::call(&mut rt, enchant, &[pick, efficiency, V::I(1)]);
        assert_eq!(api::call(&mut rt, enchanted, &[pick]), V::I(1));
        assert_eq!(api::call(&mut rt, level, &[pick, efficiency]), V::I(2));
        let held = api::call(&mut rt, get, &[pick, component]);
        assert_eq!(names(&rt, held), vec![("minecraft:efficiency".to_owned(), 2)]);
        assert_eq!(api::call(&mut rt, crafting, &[pick]), held);

        let book = crate::api_common::stack(&mut rt, "minecraft:enchanted_book", 1);
        api::call(&mut rt, enchant, &[book, efficiency, V::I(4)]);
        assert_eq!(api::call(&mut rt, enchanted, &[book]), V::I(0), "a book's are stored, not its own");
        let stored = api::call(&mut rt, crafting, &[book]);
        assert_eq!(names(&rt, stored), vec![("minecraft:efficiency".to_owned(), 4)]);

        // Walked as vanilla's entry set: holders and levels.
        let entries = api::call(&mut rt, "net/minecraft/world/item/enchantment/ItemEnchantments.entrySet.()Ljava/util/Set;", &[held]);
        let first = crate::jutil::elements(&rt, entries)[0];
        assert_eq!(api::call(&mut rt, "it/unimi/dsi/fastutil/objects/Object2IntMap$Entry.getKey.()Ljava/lang/Object;", &[first]), efficiency);
        assert_eq!(api::call(&mut rt, "it/unimi/dsi/fastutil/objects/Object2IntMap$Entry.getIntValue.()I", &[first]), V::I(2));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `hasFoil`: a plain pickaxe has no glint, an enchanted one has; a nether star shines by
    /// default, and a stack whose `enchantment_glint_override` is set to false doesn't.
    #[test]
    fn a_stack_shines_enchanted_or_where_its_glint_says() {
        let mut rt = Rt::new();
        crate::install();
        const FOIL: &str = "net/minecraft/world/item/ItemStack.hasFoil.()Z";
        const SET: &str = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let efficiency = enchantment(
            &mut rt,
            "net/minecraft/world/item/enchantment/Enchantments.EFFICIENCY",
        );
        let pick = crate::api_common::stack(&mut rt, "minecraft:diamond_pickaxe", 1);
        assert_eq!(api::call(&mut rt, FOIL, &[pick]), V::I(0));
        api::call(
            &mut rt,
            "net/minecraft/world/item/ItemStack.enchant.(Lnet/minecraft/core/Holder;I)V",
            &[pick, efficiency, V::I(1)],
        );
        assert_eq!(api::call(&mut rt, FOIL, &[pick]), V::I(1));
        let star = crate::api_common::stack(&mut rt, "minecraft:nether_star", 1);
        assert_eq!(api::call(&mut rt, FOIL, &[star]), V::I(1));
        let glint = api::get_static(
            &mut rt,
            "net/minecraft/core/component/DataComponents.ENCHANTMENT_GLINT_OVERRIDE",
        );
        let no = crate::mixin::box_value(&mut rt, V::I(0), 'Z');
        api::call(&mut rt, SET, &[star, glint, no]);
        assert_eq!(api::call(&mut rt, FOIL, &[star]), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Enchantment.getFullname`: the description, grey (red for a curse), and the level as vanilla's
    /// `enchantment.level.N` unless the enchantment has only one.
    #[test]
    fn an_enchantments_tooltip_line_names_its_level_unless_it_has_one() {
        let mut rt = Rt::new();
        crate::install();
        let line = |rt: &mut Rt, field: &'static str, n: i32| {
            let h = enchantment(rt, field);
            let c = api::call(rt, "net/minecraft/world/item/enchantment/Enchantment.getFullname.(Lnet/minecraft/core/Holder;I)Lnet/minecraft/network/chat/Component;", &[h, V::I(n)]);
            crate::api_common::component_json(rt, c)
        };
        let efficiency = line(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.EFFICIENCY", 3);
        assert_eq!(efficiency["translate"], "enchantment.minecraft.efficiency");
        assert_eq!(efficiency["color"], "gray");
        assert_eq!(efficiency["extra"][1]["translate"], "enchantment.level.3");
        let silk = line(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.SILK_TOUCH", 1);
        assert!(silk.get("extra").is_none(), "{silk}");
        let curse = line(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.BINDING_CURSE", 1);
        assert_eq!(curse["color"], "red");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Once a blow lands, mods' projectiles and bosses (Create's potato, Cataclysm's) have the game
    /// run its enchantments' effects: what was struck, by whom, through what, and whose weapon acts
    /// (the one in hand, a stack named, or none).
    #[test]
    fn a_blows_enchantment_effects_go_to_the_game_with_its_source_and_weapon() {
        use crate::host::{AttackWeapon, EntityView, MemoryHost, Struck};
        const AFTER: &str = "net/minecraft/world/item/enchantment/EnchantmentHelper.doPostAttackEffects.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/damagesource/DamageSource;)V";
        const WITH_ITEM: &str = "net/minecraft/world/item/enchantment/EnchantmentHelper.doPostAttackEffectsWithItemSource.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/damagesource/DamageSource;Lnet/minecraft/world/item/ItemStack;)V";
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        for (id, kind) in [(7, "minecraft:cow"), (9, "minecraft:zombie"), (11, "minecraft:arrow")] {
            host.entities.push(EntityView { id, kind: kind.into(), pos: [0.5, 64.0, 4.5], yaw: 0.0, health: None, ..Default::default() });
        }
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let player = crate::menus::server_player(rt, 42);
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            let zombie = crate::vanilla_entities::of(rt, 9, "minecraft:zombie", None);
            let arrow = crate::vanilla_entities::of(rt, 11, "minecraft:arrow", None);
            // The player's arrow in the cow; the zombie's blow on the player.
            let shot = crate::damage::source(rt, "minecraft:arrow", arrow, player, V::NULL);
            api::call(rt, AFTER, &[level, cow, shot]);
            let bite = crate::damage::source(rt, "minecraft:mob_attack", zombie, zombie, V::NULL);
            api::call(rt, AFTER, &[level, player, bite]);
            // The player's own blow, with no item, then with a fire aspect sword.
            let hit = crate::damage::source(rt, "minecraft:player_attack", player, player, V::NULL);
            api::call(rt, WITH_ITEM, &[level, cow, hit, V::NULL]);
            let sword = crate::api_common::stack(rt, "minecraft:diamond_sword", 1);
            let fire = enchantment(rt, "net/minecraft/world/item/enchantment/Enchantments.FIRE_ASPECT");
            api::call(rt, "net/minecraft/world/item/ItemStack.enchant.(Lnet/minecraft/core/Holder;I)V", &[sword, fire, V::I(2)]);
            api::call(rt, WITH_ITEM, &[level, cow, hit, sword]);
            // An entity of a mod's own has no worn enchantments the game knows.
            let own = rt.new_object("com/example/Golem");
            api::call(rt, AFTER, &[level, own, hit]);
        });
        let told: Vec<_> = host.post_attacks.iter().map(|(victim, by, weapon)| (*victim, by.player, by.mob, by.direct.clone(), weapon.clone())).collect();
        let AttackWeapon::Stack(sword) = &told[3].4 else { panic!("{:?}", told[3].4) };
        assert_eq!((sword.item.as_str(), sword.enchantments()), ("minecraft:diamond_sword", vec![("minecraft:fire_aspect".to_owned(), 2)]));
        assert_eq!(
            told[..3],
            [
                (Struck::Entity(7), Some(42), None, Some("minecraft:arrow".to_owned()), AttackWeapon::Held),
                (Struck::Player(42), None, Some(9), None, AttackWeapon::Held),
                (Struck::Entity(7), Some(42), None, None, AttackWeapon::None),
            ]
        );
        assert_eq!(told.len(), 4);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
