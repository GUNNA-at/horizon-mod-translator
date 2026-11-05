//! Attributes and what items carry of them: vanilla's `Attributes` holders, the modifiers a mod
//! puts on an item (`ItemAttributeModifiers.builder()...build()`, `Item.BASE_ATTACK_DAMAGE_ID`), and
//! the keys of vanilla's enchantments (`Enchantments.SILK_TOUCH`).
//!
//! An `ItemAttributeModifiers` is vanilla's record (`modifiers`, `showInTooltip`), its entries
//! `ItemAttributeModifiers.Entry` records ([`crate::records`] reads them back); the attack damage and
//! speed a main hand gets are kept beside them (`$attackDamage`, `$attackSpeed`), as
//! `SwordItem.createAttributes` keeps them.
use crate::{Data, Rt, V};

const ATTRIBUTES: &str = "net/minecraft/world/entity/ai/attributes/Attributes";
const MODIFIERS: &str = "net/minecraft/world/item/component/ItemAttributeModifiers";
const BUILDER: &str = "net/minecraft/world/item/component/ItemAttributeModifiers$Builder";
const ENTRY: &str = "net/minecraft/world/item/component/ItemAttributeModifiers$Entry";
const ENCHANTMENTS: &str = "net/minecraft/world/item/enchantment/Enchantments";

/// 1.21.1's `Attributes` fields and the ids its initialiser registers them under.
const VANILLA: [(&str, &str); 31] = [
    ("ARMOR", "generic.armor"),
    ("ARMOR_TOUGHNESS", "generic.armor_toughness"),
    ("ATTACK_DAMAGE", "generic.attack_damage"),
    ("ATTACK_KNOCKBACK", "generic.attack_knockback"),
    ("ATTACK_SPEED", "generic.attack_speed"),
    ("BLOCK_BREAK_SPEED", "player.block_break_speed"),
    ("BLOCK_INTERACTION_RANGE", "player.block_interaction_range"),
    ("BURNING_TIME", "generic.burning_time"),
    ("ENTITY_INTERACTION_RANGE", "player.entity_interaction_range"),
    ("EXPLOSION_KNOCKBACK_RESISTANCE", "generic.explosion_knockback_resistance"),
    ("FALL_DAMAGE_MULTIPLIER", "generic.fall_damage_multiplier"),
    ("FLYING_SPEED", "generic.flying_speed"),
    ("FOLLOW_RANGE", "generic.follow_range"),
    ("GRAVITY", "generic.gravity"),
    ("JUMP_STRENGTH", "generic.jump_strength"),
    ("KNOCKBACK_RESISTANCE", "generic.knockback_resistance"),
    ("LUCK", "generic.luck"),
    ("MAX_ABSORPTION", "generic.max_absorption"),
    ("MAX_HEALTH", "generic.max_health"),
    ("MINING_EFFICIENCY", "player.mining_efficiency"),
    ("MOVEMENT_EFFICIENCY", "generic.movement_efficiency"),
    ("MOVEMENT_SPEED", "generic.movement_speed"),
    ("OXYGEN_BONUS", "generic.oxygen_bonus"),
    ("SAFE_FALL_DISTANCE", "generic.safe_fall_distance"),
    ("SCALE", "generic.scale"),
    ("SNEAKING_SPEED", "player.sneaking_speed"),
    ("SPAWN_REINFORCEMENTS_CHANCE", "zombie.spawn_reinforcements"),
    ("STEP_HEIGHT", "generic.step_height"),
    ("SUBMERGED_MINING_SPEED", "player.submerged_mining_speed"),
    ("SWEEPING_DAMAGE_RATIO", "player.sweeping_damage_ratio"),
    ("WATER_MOVEMENT_EFFICIENCY", "generic.water_movement_efficiency"),
];

/// 1.21.1's `Enchantments` fields: each the key of the enchantment named by its lowercase name
/// (`SILK_TOUCH` is `minecraft:silk_touch`), as its initialiser makes them.
const ENCHANTMENT_FIELDS: [&str; 42] = [
    "PROTECTION", "FIRE_PROTECTION", "FEATHER_FALLING", "BLAST_PROTECTION", "PROJECTILE_PROTECTION", "RESPIRATION", "AQUA_AFFINITY", "THORNS",
    "DEPTH_STRIDER", "FROST_WALKER", "BINDING_CURSE", "SOUL_SPEED", "SWIFT_SNEAK", "SHARPNESS", "SMITE", "BANE_OF_ARTHROPODS", "KNOCKBACK",
    "FIRE_ASPECT", "LOOTING", "SWEEPING_EDGE", "EFFICIENCY", "SILK_TOUCH", "UNBREAKING", "FORTUNE", "POWER", "PUNCH", "FLAME", "INFINITY",
    "LUCK_OF_THE_SEA", "LURE", "LOYALTY", "IMPALING", "RIPTIDE", "CHANNELING", "MULTISHOT", "QUICK_CHARGE", "PIERCING", "DENSITY", "BREACH",
    "WIND_BURST", "MENDING", "VANISHING_CURSE",
];

/// The holder of vanilla's attribute `id` (`minecraft:generic.attack_damage`): one per attribute.
pub(crate) fn attribute(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("attribute-holder/{id}"), |rt| {
        let value = rt.new_object("net/minecraft/world/entity/ai/attributes/Attribute");
        let s = rt.string(id);
        rt.put_field(value, "$id", s);
        crate::neo::registry_holder(rt, "attribute", id, value)
    })
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        ATTRIBUTES => {
            let (_, id) = VANILLA.iter().find(|(f, _)| *f == field)?;
            Some(attribute(rt, &format!("minecraft:{id}")))
        }
        ENCHANTMENTS if ENCHANTMENT_FIELDS.contains(&field) => Some(rt.string(&format!("enchantment#minecraft:{}", field.to_ascii_lowercase()))),
        // The ids of the main hand's base attack modifiers, which the tooltip shows as the item's own;
        // also through vanilla's items, which inherit them (`BlockItem.BASE_ATTACK_DAMAGE_ID`).
        o if o.starts_with("net/minecraft/world/item/") && o.ends_with("Item") && !o["net/minecraft/world/item/".len()..].contains('/') => match field {
            "BASE_ATTACK_DAMAGE_ID" => Some(rt.string("minecraft:base_attack_damage")),
            "BASE_ATTACK_SPEED_ID" => Some(rt.string("minecraft:base_attack_speed")),
            _ => None,
        },
        MODIFIERS if field == "EMPTY" => Some(rt.intern("compat:item-attribute-modifiers/empty".into(), |rt| made(rt, Vec::new()))),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner == BUILDER || (owner == MODIFIERS && matches!(name, "builder" | "withModifierAdded"))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, _desc) = crate::parts(key)?;
    match (owner, name) {
        (MODIFIERS, "builder") => {
            let b = rt.new_object(BUILDER);
            let entries = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(b, "$entries", entries);
            Some(b)
        }
        // `add(attribute, modifier, slot)`: one entry more, in order; the builder again.
        (BUILDER, "add") => {
            let e = entry(rt, a(1), a(2), a(3));
            let entries = rt.get_field(a(0), "$entries");
            push(rt, entries, e);
            Some(a(0))
        }
        (BUILDER, "build") => {
            let entries = crate::jutil::elements(rt, rt.get_field(a(0), "$entries"));
            Some(made(rt, entries))
        }
        // A copy with one entry more (vanilla drops an equal one first: same attribute and modifier id).
        (MODIFIERS, "withModifierAdded") => {
            let mut entries = crate::jutil::elements(rt, rt.get_field(a(0), "modifiers"));
            let id = modifier_id(rt, a(2));
            entries.retain(|&e| !(rt.get_field(e, "attribute") == a(1) && modifier_id(rt, rt.get_field(e, "modifier")) == id));
            let e = entry(rt, a(1), a(2), a(3));
            entries.push(e);
            let m = made(rt, entries);
            let shown = rt.get_field(a(0), "showInTooltip");
            rt.put_field(m, "showInTooltip", shown);
            Some(m)
        }
        _ => None,
    }
}

fn entry(rt: &mut Rt, attribute: V, modifier: V, slot: V) -> V {
    let e = rt.new_object(ENTRY);
    rt.put_field(e, "attribute", attribute);
    rt.put_field(e, "modifier", modifier);
    rt.put_field(e, "slot", slot);
    e
}

fn push(rt: &mut Rt, list: V, v: V) {
    if let Some(crate::Object { data: Data::List(l), .. }) = rt.heap.get_mut(list.r() as usize) {
        l.push(v);
    }
}

/// An `AttributeModifier` record's id (its location).
fn modifier_id(rt: &Rt, modifier: V) -> String {
    crate::neo::location(rt, rt.get_field(modifier, "id"))
}

/// An `ItemAttributeModifiers` of `entries`, shown in the tooltip, with the main hand's attack
/// damage and speed (the amounts of their modifiers) where an entry gives them.
fn made(rt: &mut Rt, entries: Vec<V>) -> V {
    let m = rt.new_object(MODIFIERS);
    for &e in &entries {
        let holder = rt.get_field(e, "attribute");
        let id = rt.str_of(rt.get_field(holder, "$id")).unwrap_or("").to_owned();
        let field = match id.as_str() {
            "minecraft:generic.attack_damage" => "$attackDamage",
            "minecraft:generic.attack_speed" => "$attackSpeed",
            _ => continue,
        };
        let amount = rt.get_field(rt.get_field(e, "modifier"), "amount");
        rt.put_field(m, field, amount);
    }
    let list = crate::jutil::new_list(rt, entries);
    rt.put_field(m, "modifiers", list);
    rt.put_field(m, "showInTooltip", V::I(1));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mod's own weapon: `ItemAttributeModifiers.builder().add(ATTACK_DAMAGE, new AttributeModifier(
    /// BASE_ATTACK_DAMAGE_ID, 5, ADD_VALUE), MAINHAND).add(ATTACK_SPEED, ..., -2.4 ...).build()`, read back
    /// as vanilla's record: its entries in order, each with its attribute, modifier and slot.
    #[test]
    fn a_weapons_modifiers_read_back_as_built() {
        let mut rt = Rt::new();
        let get = |rt: &mut Rt, k: &'static str| crate::api::get_static(rt, k);
        let damage = get(&mut rt, "net/minecraft/world/entity/ai/attributes/Attributes.ATTACK_DAMAGE");
        let speed = get(&mut rt, "net/minecraft/world/entity/ai/attributes/Attributes.ATTACK_SPEED");
        assert_ne!(damage, speed);
        assert_eq!(get(&mut rt, "net/minecraft/world/entity/ai/attributes/Attributes.ATTACK_DAMAGE"), damage, "one holder per attribute");
        let add_value = get(&mut rt, "net/minecraft/world/entity/ai/attributes/AttributeModifier$Operation.ADD_VALUE");
        let mainhand = get(&mut rt, "net/minecraft/world/entity/EquipmentSlotGroup.MAINHAND");
        let modifier = |rt: &mut Rt, id: &'static str, amount: f64| {
            let m = rt.new_object("net/minecraft/world/entity/ai/attributes/AttributeModifier");
            let id = crate::api::get_static(rt, id);
            crate::api::call(
                rt,
                "net/minecraft/world/entity/ai/attributes/AttributeModifier.<init>.(Lnet/minecraft/resources/ResourceLocation;DLnet/minecraft/world/entity/ai/attributes/AttributeModifier$Operation;)V",
                &[m, id, V::D(amount), add_value],
            );
            m
        };
        let hit = modifier(&mut rt, "net/minecraft/world/item/Item.BASE_ATTACK_DAMAGE_ID", 5.0);
        let swing = modifier(&mut rt, "net/minecraft/world/item/Item.BASE_ATTACK_SPEED_ID", -2.4);
        let b = crate::api::call(&mut rt, "net/minecraft/world/item/component/ItemAttributeModifiers.builder.()Lnet/minecraft/world/item/component/ItemAttributeModifiers$Builder;", &[]);
        let add = "net/minecraft/world/item/component/ItemAttributeModifiers$Builder.add.(Lnet/minecraft/core/Holder;Lnet/minecraft/world/entity/ai/attributes/AttributeModifier;Lnet/minecraft/world/entity/EquipmentSlotGroup;)Lnet/minecraft/world/item/component/ItemAttributeModifiers$Builder;";
        let b = crate::api::call(&mut rt, add, &[b, damage, hit, mainhand]);
        let b = crate::api::call(&mut rt, add, &[b, speed, swing, mainhand]);
        let built = crate::api::call(&mut rt, "net/minecraft/world/item/component/ItemAttributeModifiers$Builder.build.()Lnet/minecraft/world/item/component/ItemAttributeModifiers;", &[b]);
        let list = crate::api::call(&mut rt, "net/minecraft/world/item/component/ItemAttributeModifiers.modifiers.()Ljava/util/List;", &[built]);
        let entries = crate::jutil::elements(&rt, list);
        assert_eq!(entries.len(), 2);
        let first_attribute = crate::api::call(&mut rt, "net/minecraft/world/item/component/ItemAttributeModifiers$Entry.attribute.()Lnet/minecraft/core/Holder;", &[entries[0]]);
        assert_eq!(first_attribute, damage);
        let second = crate::api::call(&mut rt, "net/minecraft/world/item/component/ItemAttributeModifiers$Entry.modifier.()Lnet/minecraft/world/entity/ai/attributes/AttributeModifier;", &[entries[1]]);
        let amount = crate::api::call(&mut rt, "net/minecraft/world/entity/ai/attributes/AttributeModifier.amount.()D", &[second]);
        assert_eq!(amount, V::D(-2.4));
        let slot = crate::api::call(&mut rt, "net/minecraft/world/item/component/ItemAttributeModifiers$Entry.slot.()Lnet/minecraft/world/entity/EquipmentSlotGroup;", &[entries[1]]);
        assert_eq!(slot, mainhand);
        assert_eq!(rt.get_field(built, "$attackDamage"), V::D(5.0));
        let key = crate::api::call(&mut rt, "net/minecraft/core/Holder.getRegisteredName.()Ljava/lang/String;", &[damage]);
        assert_eq!(rt.str_of(key), Some("minecraft:generic.attack_damage"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn enchantment_constants_are_the_keys_of_vanillas_enchantments() {
        let mut rt = Rt::new();
        let silk = crate::api::get_static(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.SILK_TOUCH");
        assert_eq!(rt.str_of(silk), Some("enchantment#minecraft:silk_touch"));
        let bane = crate::api::get_static(&mut rt, "net/minecraft/world/item/enchantment/Enchantments.BANE_OF_ARTHROPODS");
        assert_eq!(rt.str_of(bane), Some("enchantment#minecraft:bane_of_arthropods"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
