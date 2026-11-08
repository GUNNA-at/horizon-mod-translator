//! Entities to NBT and back, as vanilla's `Entity` and `EntityType` do it: an entity saves under
//! its type's id (`save`, `saveAsPassenger`, `getEncodeId`; none for a passenger, which its
//! vehicle saves, for one removed for good, or of a type that isn't saved), a type is found by its
//! id (`EntityType.by`, `byString`), a new entity is made from a compound (`create(tag, level)`),
//! and one with its passengers (`loadEntityRecursive`). Also what a spawn egg spawns
//! (`SpawnEggItem.getType`: the type its stack's entity data names, else its own) and the egg of
//! a type (`byId`).
//!
//! Each step is the entity's own call (`saveWithoutId`, `load`, `startRiding`), so a mod's entity,
//! an entity a mod made of vanilla's type and the game's own each answer as they do.

use crate::api;
use crate::host::Nbt;
use crate::{Rt, V};

const ENTITY: &str = "net/minecraft/world/entity/Entity";
const ENTITY_TYPE: &str = "net/minecraft/world/entity/EntityType";
const REMOVAL: &str = "net/minecraft/world/entity/Entity$RemovalReason";
const SPAWN_EGG: &str = "net/minecraft/world/item/SpawnEggItem";
const DEFERRED_SPAWN_EGG: &str = "net/neoforged/neoforge/common/DeferredSpawnEggItem";
const NPE: &str = "java/lang/NullPointerException";

const IS_PASSENGER: (&str, &str) = (
    "net/minecraft/world/entity/Entity.isPassenger.()Z",
    "isPassenger.()Z",
);
const IS_REMOVED: (&str, &str) = (
    "net/minecraft/world/entity/Entity.isRemoved.()Z",
    "isRemoved.()Z",
);
const GET_TYPE: (&str, &str) = (
    "net/minecraft/world/entity/Entity.getType.()Lnet/minecraft/world/entity/EntityType;",
    "getType.()Lnet/minecraft/world/entity/EntityType;",
);
const SAVE_AS_PASSENGER: (&str, &str) = (
    "net/minecraft/world/entity/Entity.saveAsPassenger.(Lnet/minecraft/nbt/CompoundTag;)Z",
    "saveAsPassenger.(Lnet/minecraft/nbt/CompoundTag;)Z",
);
const SAVE_WITHOUT_ID: (&str, &str) = (
    "net/minecraft/world/entity/Entity.saveWithoutId.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;",
    "saveWithoutId.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;",
);
const LOAD: (&str, &str) = (
    "net/minecraft/world/entity/Entity.load.(Lnet/minecraft/nbt/CompoundTag;)V",
    "load.(Lnet/minecraft/nbt/CompoundTag;)V",
);
const START_RIDING: (&str, &str) = (
    "net/minecraft/world/entity/Entity.startRiding.(Lnet/minecraft/world/entity/Entity;Z)Z",
    "startRiding.(Lnet/minecraft/world/entity/Entity;Z)Z",
);
const EGG_TYPE: (&str, &str) = (
    "net/minecraft/world/item/SpawnEggItem.getType.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/entity/EntityType;",
    "getType.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/entity/EntityType;",
);
const DEFAULT_TYPE: (&str, &str) = (
    "net/minecraft/world/item/SpawnEggItem.getDefaultType.()Lnet/minecraft/world/entity/EntityType;",
    "getDefaultType.()Lnet/minecraft/world/entity/EntityType;",
);
const CREATE_IN: &str = "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";
const TYPE_KEY: &str = "net/minecraft/world/entity/EntityType.getKey.(Lnet/minecraft/world/entity/EntityType;)Lnet/minecraft/resources/ResourceLocation;";
const DESCRIPTION_ID: &str =
    "net/minecraft/world/entity/EntityType.getDescriptionId.()Ljava/lang/String;";
const WIDTH: &str = "net/minecraft/world/entity/EntityType.getWidth.()F";
const HEIGHT: &str = "net/minecraft/world/entity/EntityType.getHeight.()F";
const KEY_CREATE: &str = "net/minecraft/resources/ResourceKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;";
const GET_STRING: &str =
    "net/minecraft/nbt/CompoundTag.getString.(Ljava/lang/String;)Ljava/lang/String;";
const PUT_STRING: &str =
    "net/minecraft/nbt/CompoundTag.putString.(Ljava/lang/String;Ljava/lang/String;)V";
const CONTAINS: &str = "net/minecraft/nbt/CompoundTag.contains.(Ljava/lang/String;I)Z";
const GET_LIST: &str =
    "net/minecraft/nbt/CompoundTag.getList.(Ljava/lang/String;I)Lnet/minecraft/nbt/ListTag;";
const LIST_SIZE: &str = "net/minecraft/nbt/ListTag.size.()I";
const LIST_COMPOUND: &str =
    "net/minecraft/nbt/ListTag.getCompound.(I)Lnet/minecraft/nbt/CompoundTag;";
const STACK_GET: &str = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const SUPPLIER_GET: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";

/// Vanilla's types that aren't saved (`EntityType.Builder.noSave`), and those of them a command
/// can't summon (`noSummon`).
const UNSAVED: [&str; 4] = [
    "minecraft:fishing_bobber",
    "minecraft:leash_knot",
    "minecraft:lightning_bolt",
    "minecraft:player",
];
const UNSUMMONED: [&str; 2] = ["minecraft:fishing_bobber", "minecraft:player"];
/// Vanilla's types whose spawn checks a box four times their size (`spawnDimensionsScale(4.0F)`).
const SPAWN_SCALED: [&str; 2] = ["minecraft:magma_cube", "minecraft:slime"];

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    (owner == ENTITY && matches!(name, "save" | "saveAsPassenger" | "getEncodeId"))
        || (owner == ENTITY_TYPE
            && matches!(
                name,
                "by" | "byString"
                    | "create"
                    | "loadEntityRecursive"
                    | "canSerialize"
                    | "canSummon"
                    | "getDefaultLootTable"
                    | "toShortString"
                    | "getSpawnAABB"
            ))
        || (owner == REMOVAL && matches!(name, "shouldSave" | "shouldDestroy"))
        || (matches!(owner, SPAWN_EGG | DEFERRED_SPAWN_EGG)
            && matches!(
                name,
                "getType" | "getDefaultType" | "spawnsEntity" | "getColor" | "byId"
            ))
}

/// Saving and loading entities, their types by id, and spawn eggs' types.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let receiver_null = a(0).r() == 0;
    Some(match (owner, name) {
        (ENTITY, "save" | "saveAsPassenger") if desc == "(Lnet/minecraft/nbt/CompoundTag;)Z" => {
            if receiver_null {
                return npe(rt);
            }
            V::I(i32::from(save(rt, a(0), a(1), name == "saveAsPassenger")))
        }
        (ENTITY, "getEncodeId") if desc == "()Ljava/lang/String;" => {
            if receiver_null {
                return npe(rt);
            }
            match encode_id(rt, a(0)) {
                Some(id) => rt.string(&id),
                None => V::NULL,
            }
        }
        (ENTITY_TYPE, "by") if desc == "(Lnet/minecraft/nbt/CompoundTag;)Ljava/util/Optional;" => {
            match by_tag(rt, a(0)) {
                Some(ty) => crate::api_common::optional(rt, ty.unwrap_or(V::NULL)),
                None => V::NULL,
            }
        }
        // `byString(key)`: `ResourceLocation.tryParse`, so an id vanilla can't read names none.
        (ENTITY_TYPE, "byString") if desc == "(Ljava/lang/String;)Ljava/util/Optional;" => {
            if receiver_null {
                return npe(rt);
            }
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            let ty = crate::neo::checked_location(&text, ':')
                .ok()
                .and_then(|id| type_named(rt, &id));
            crate::api_common::optional(rt, ty.unwrap_or(V::NULL))
        }
        (ENTITY_TYPE, "create")
            if desc
                == "(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/world/level/Level;)Ljava/util/Optional;" =>
        {
            let e = create(rt, a(0), a(1));
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            crate::api_common::optional(rt, e)
        }
        (ENTITY_TYPE, "loadEntityRecursive")
            if desc
                == "(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/world/level/Level;Ljava/util/function/Function;)Lnet/minecraft/world/entity/Entity;" =>
        {
            load_recursive(rt, a(0), a(1), a(2))
        }
        (
            ENTITY_TYPE,
            "canSerialize" | "canSummon" | "getDefaultLootTable" | "toShortString" | "getSpawnAABB",
        ) if receiver_null => return npe(rt),
        (ENTITY_TYPE, "canSerialize") => V::I(i32::from(can_serialize(rt, a(0)))),
        (ENTITY_TYPE, "canSummon") => {
            let unsummoned = rt.get_field(a(0), "$noSummon").i() != 0
                || crate::mod_entities::type_id(rt, a(0))
                    .is_some_and(|id| UNSUMMONED.contains(&id));
            V::I(i32::from(!unsummoned))
        }
        // `getDefaultLootTable()`: the loot table under its id's `entities/` (`minecraft:entities/cow`).
        (ENTITY_TYPE, "getDefaultLootTable") => {
            let id = api::call(rt, TYPE_KEY, &[a(0)]);
            let id = rt.str_of(id).unwrap_or("minecraft:pig").to_owned();
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            let location = rt.string(&format!("{ns}:entities/{path}"));
            let registry =
                api::get_static(rt, "net/minecraft/core/registries/Registries.LOOT_TABLE");
            api::call(rt, KEY_CREATE, &[registry, location])
        }
        // `toShortString()`: its description id's last part (`entity.minecraft.cow`'s `cow`).
        (ENTITY_TYPE, "toShortString") => {
            let d = api::call(rt, DESCRIPTION_ID, &[a(0)]);
            let d = rt.str_of(d).unwrap_or("").to_owned();
            let short = d.rsplit_once('.').map_or(d.as_str(), |(_, s)| s).to_owned();
            rt.string(&short)
        }
        // `getSpawnAABB(x, y, z)`: the box a spawn there needs, its size by the type's spawn scale,
        // centred on x and z, from y up (floats, as vanilla's).
        (ENTITY_TYPE, "getSpawnAABB") if desc == "(DDD)Lnet/minecraft/world/phys/AABB;" => {
            let scale = spawn_scale(rt, a(0));
            let half = f64::from(scale * api::call(rt, WIDTH, &[a(0)]).f() / 2.0);
            let height = f64::from(scale * api::call(rt, HEIGHT, &[a(0)]).f());
            let (x, y, z) = (a(1).d(), a(2).d(), a(3).d());
            crate::shapes::aabb(rt, [x - half, y, z - half, x + half, y + height, z + half])
        }
        (REMOVAL, "shouldSave" | "shouldDestroy") => {
            if receiver_null {
                return npe(rt);
            }
            let reason = reason_name(rt, a(0));
            V::I(i32::from(if name == "shouldSave" {
                reason == "UNLOADED_TO_CHUNK"
            } else {
                matches!(reason.as_str(), "KILLED" | "DISCARDED")
            }))
        }
        (SPAWN_EGG | DEFERRED_SPAWN_EGG, _) => return egg_call(rt, name, desc, args),
        _ => return None,
    })
}

fn npe(rt: &mut Rt) -> Option<V> {
    rt.throw_new(NPE);
    Some(V::NULL)
}

/// `Entity.save` (none for a passenger: its vehicle saves it) and `saveAsPassenger`: none for an
/// entity removed for good or of a type that isn't saved, else the type's id, then
/// `saveWithoutId`'s.
fn save(rt: &mut Rt, e: V, tag: V, as_passenger: bool) -> bool {
    if !as_passenger {
        if api::virtual_call(rt, IS_PASSENGER, &[e]).i() != 0 || rt.thrown.is_some() {
            return false;
        }
        return api::virtual_call(rt, SAVE_AS_PASSENGER, &[e, tag]).i() != 0;
    }
    if removed_for_good(rt, e) {
        return false;
    }
    let Some(id) = encode_id(rt, e) else {
        return false;
    };
    if tag.r() == 0 {
        rt.throw_new(NPE);
        return false;
    }
    let (k, v) = (rt.string("id"), rt.string(&id));
    api::call(rt, PUT_STRING, &[tag, k, v]);
    api::virtual_call(rt, SAVE_WITHOUT_ID, &[e, tag]);
    rt.thrown.is_none()
}

/// Whether `e` left its level for good: vanilla saves an entity removed only with its chunk
/// (`RemovalReason.shouldSave`). A mod's entity keeps its reason; the game's own, or one a mod
/// made of vanilla's type, is gone for good once removed (killed or discarded).
fn removed_for_good(rt: &mut Rt, e: V) -> bool {
    if crate::mod_entities::is_mod_entity(rt, e) {
        let reason = rt.get_field(e, "removalReason");
        return reason.r() != 0 && reason_name(rt, reason) != "UNLOADED_TO_CHUNK";
    }
    api::virtual_call(rt, IS_REMOVED, &[e]).i() != 0
}

fn reason_name(rt: &Rt, reason: V) -> String {
    rt.str_of(rt.get_field(reason, "$name"))
        .unwrap_or("")
        .to_owned()
}

/// `Entity.getEncodeId`: the registry id of its type (`getType`, its own), none for a type that
/// isn't saved.
fn encode_id(rt: &mut Rt, e: V) -> Option<String> {
    let ty = api::virtual_call(rt, GET_TYPE, &[e]);
    if ty.r() == 0 || rt.thrown.is_some() || !can_serialize(rt, ty) {
        return None;
    }
    let id = api::call(rt, TYPE_KEY, &[ty]);
    rt.str_of(id).map(str::to_owned)
}

/// `EntityType.canSerialize`: false for a type built `noSave`.
fn can_serialize(rt: &Rt, ty: V) -> bool {
    rt.get_field(ty, "$noSave").i() == 0
        && !crate::mod_entities::type_id(rt, ty).is_some_and(|id| UNSAVED.contains(&id))
}

/// How much bigger than the type its spawn's box is: a mod's type's builder sets it
/// (`spawnDimensionsScale`), vanilla's are 1 but for slimes and magma cubes.
fn spawn_scale(rt: &Rt, ty: V) -> f32 {
    match rt.get_field(ty, "$spawnScale") {
        V::F(scale) => scale,
        _ if crate::mod_entities::type_id(rt, ty).is_some_and(|id| SPAWN_SCALED.contains(&id)) => {
            4.0
        }
        _ => 1.0,
    }
}

/// The entity type the registry holds under `id` (a mod's, vanilla's), as its `getOptional` finds
/// it: none for an id it doesn't hold.
fn type_named(rt: &mut Rt, id: &str) -> Option<V> {
    let ty = crate::neo::registry_get(rt, "entity_type", id);
    let known = ty.r() != 0
        && (rt.registered_name(ty, "entity_type").is_some()
            || crate::mod_entities::type_id(rt, ty)
                .is_some_and(crate::vanilla_entities::is_vanilla_type));
    known.then_some(ty)
}

/// `EntityType.by(tag)`: the type its `id` names (None inside for one the registry doesn't hold).
/// None when it threw: a null tag, or an id vanilla can't read (`ResourceLocation.parse` throws
/// `ResourceLocationException`).
fn by_tag(rt: &mut Rt, tag: V) -> Option<Option<V>> {
    if tag.r() == 0 {
        rt.throw_new(NPE);
        return None;
    }
    let k = rt.string("id");
    let id = api::call(rt, GET_STRING, &[tag, k]);
    let id = rt.str_of(id).unwrap_or("").to_owned();
    match crate::neo::checked_location(&id, ':') {
        Ok(id) => Some(type_named(rt, &id)),
        Err(message) => {
            let ex = rt.new_object("net/minecraft/ResourceLocationException");
            let m = rt.string(&message);
            rt.put_field(ex, "message", m);
            rt.thrown = Some(ex);
            None
        }
    }
}

/// `EntityType.create(tag, level)`: a new entity of the type the tag names (`create(level)`), which
/// then loads the tag (`load`, its own); null for a type it doesn't name or that makes none.
fn create(rt: &mut Rt, tag: V, level: V) -> V {
    let Some(Some(ty)) = by_tag(rt, tag) else {
        return V::NULL;
    };
    let e = api::call(rt, CREATE_IN, &[ty, level]);
    if e.r() == 0 || rt.thrown.is_some() {
        return V::NULL;
    }
    api::virtual_call(rt, LOAD, &[e, tag]);
    e
}

/// `EntityType.loadEntityRecursive(tag, level, function)`: the entity the tag makes (none when
/// making it threw a `RuntimeException`, which vanilla logs and drops) as the function gives it
/// back, then each of its passengers (`Passengers`) the same way, riding it
/// (`startRiding(vehicle, true)`).
fn load_recursive(rt: &mut Rt, tag: V, level: V, function: V) -> V {
    let e = create(rt, tag, level);
    if let Some(ex) = rt.thrown {
        if rt.instance_of(ex, "java/lang/RuntimeException") {
            rt.thrown = None;
        }
        return V::NULL;
    }
    // `Optional.map` asks for its function first.
    if function.r() == 0 {
        rt.throw_new(NPE);
        return V::NULL;
    }
    if e.r() == 0 {
        return V::NULL;
    }
    let e = rt.call_functional(APPLY, function, &[e]);
    if e.r() == 0 || matches!(e, V::Void) || rt.thrown.is_some() {
        return V::NULL;
    }
    let k = rt.string("Passengers");
    if api::call(rt, CONTAINS, &[tag, k, V::I(9)]).i() != 0 {
        let list = api::call(rt, GET_LIST, &[tag, k, V::I(10)]);
        let n = api::call(rt, LIST_SIZE, &[list]).i();
        for i in 0..n {
            let passenger_tag = api::call(rt, LIST_COMPOUND, &[list, V::I(i)]);
            let passenger = load_recursive(rt, passenger_tag, level, function);
            if rt.thrown.is_some() {
                return V::NULL;
            }
            if passenger.r() != 0 {
                api::virtual_call(rt, START_RIDING, &[passenger, e, V::I(1)]);
                if rt.thrown.is_some() {
                    return V::NULL;
                }
            }
        }
    }
    e
}

/// A spawn egg's calls: the type it spawns (`getType`: the one its stack's entity data names by
/// `id`, else its own, `getDefaultType`), whether a stack spawns a type, its two colours, and the
/// egg of a type (`byId`).
fn egg_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if name == "byId" {
        return Some(egg_of(rt, a(0)));
    }
    if a(0).r() == 0 {
        return npe(rt);
    }
    Some(match name {
        "getType"
            if desc
                == "(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/entity/EntityType;" =>
        {
            if a(1).r() == 0 {
                return npe(rt);
            }
            egg_type(rt, a(0), a(1))
        }
        "getDefaultType" if desc == "()Lnet/minecraft/world/entity/EntityType;" => {
            default_type(rt, a(0))
        }
        // `spawnsEntity(stack, type)`: whether `getType(stack)` is that type.
        "spawnsEntity" => {
            let ty = api::virtual_call(rt, EGG_TYPE, &[a(0), a(1)]);
            V::I(i32::from(ty == a(2)))
        }
        "getColor" if desc == "(I)I" => V::I(
            rt.get_field(
                a(0),
                if a(1).i() == 0 {
                    "backgroundColor"
                } else {
                    "highlightColor"
                },
            )
            .i(),
        ),
        _ => return None,
    })
}

/// `SpawnEggItem.getType(stack)`: the type the stack's entity data names (`id`), when the registry
/// holds it; else the egg's own (`getDefaultType`, its own).
fn egg_type(rt: &mut Rt, egg: V, stack: V) -> V {
    let component = api::get_static(
        rt,
        "net/minecraft/core/component/DataComponents.ENTITY_DATA",
    );
    let data = api::call(rt, STACK_GET, &[stack, component]);
    if data.r() != 0 {
        let named = match crate::components::custom_nbt(rt, data).get("id") {
            Some(Nbt::String(id)) => crate::neo::checked_location(id, ':').ok(),
            _ => None,
        };
        if let Some(ty) = named.and_then(|id| type_named(rt, &id)) {
            return ty;
        }
    }
    api::virtual_call(rt, DEFAULT_TYPE, &[egg])
}

/// `SpawnEggItem.getDefaultType`: NeoForge's `DeferredSpawnEggItem`'s from its supplier, the type a
/// mod's egg was made with, vanilla's egg's by its name (`minecraft:cow_spawn_egg`'s cow).
fn default_type(rt: &mut Rt, egg: V) -> V {
    let supplier = rt.get_field(egg, "typeSupplier");
    if supplier.r() != 0 {
        return rt.call_functional(SUPPLIER_GET, supplier, &[]);
    }
    let own = rt.get_field(egg, "defaultType");
    if own.r() != 0 {
        return own;
    }
    match rt
        .str_of(egg)
        .and_then(|n| n.strip_suffix("_spawn_egg"))
        .map(str::to_owned)
    {
        Some(id) if crate::vanilla_entities::is_vanilla_type(&id) => {
            crate::mod_entities::vanilla_type(rt, &id)
        }
        _ => V::NULL,
    }
}

/// `SpawnEggItem.byId(type)`: the egg NeoForge maps the type to (a `DeferredSpawnEggItem`'s
/// supplier gives it), else the egg made with it, else vanilla's of its id; null for none.
fn egg_of(rt: &mut Rt, ty: V) -> V {
    if ty.r() == 0 {
        return V::NULL;
    }
    let mut eggs: Vec<(String, V)> = rt
        .registered
        .iter()
        .filter(|((registry, _), egg)| registry == "item" && rt.instance_of(**egg, SPAWN_EGG))
        .map(|((_, name), egg)| (name.clone(), *egg))
        .collect();
    eggs.sort_by(|x, y| x.0.cmp(&y.0));
    for (_, egg) in &eggs {
        let supplier = rt.get_field(*egg, "typeSupplier");
        if supplier.r() != 0 && rt.call_functional(SUPPLIER_GET, supplier, &[]) == ty {
            return *egg;
        }
    }
    if let Some((_, egg)) = eggs
        .iter()
        .find(|(_, egg)| rt.get_field(*egg, "defaultType") == ty)
    {
        return *egg;
    }
    let Some(name) = crate::mod_entities::type_id(rt, ty).map(|id| format!("{id}_spawn_egg"))
    else {
        return V::NULL;
    };
    let table = &crate::vanilla_classes::ITEMS;
    match table.binary_search_by(|(i, _)| (*i).cmp(name.as_str())) {
        Ok(i) if table[i].1 == SPAWN_EGG => crate::api_common::item(rt, &name),
        _ => V::NULL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityView, MemoryHost};
    use std::collections::BTreeMap;

    const CART: &str = "test/Cart";
    const ENTITY_INIT: &str = "net/minecraft/world/entity/Entity.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
    const FACTORY_CREATE: &str = "create.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";
    const ADD_SAVE: &str = "addAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
    const READ_SAVE: &str = "readAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
    const SAVE: &str = "net/minecraft/world/entity/Entity.save.(Lnet/minecraft/nbt/CompoundTag;)Z";
    const ENCODE_ID: &str = "net/minecraft/world/entity/Entity.getEncodeId.()Ljava/lang/String;";
    const BY: &str = "net/minecraft/world/entity/EntityType.by.(Lnet/minecraft/nbt/CompoundTag;)Ljava/util/Optional;";
    const BY_STRING: &str =
        "net/minecraft/world/entity/EntityType.byString.(Ljava/lang/String;)Ljava/util/Optional;";
    const CREATE_FROM: &str = "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/world/level/Level;)Ljava/util/Optional;";
    const LOAD_RECURSIVE: &str = "net/minecraft/world/entity/EntityType.loadEntityRecursive.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/world/level/Level;Ljava/util/function/Function;)Lnet/minecraft/world/entity/Entity;";
    const SET_POS: &str = "net/minecraft/world/entity/Entity.setPos.(DDD)V";
    const GET_X: &str = "net/minecraft/world/entity/Entity.getX.()D";
    const GET_Z: &str = "net/minecraft/world/entity/Entity.getZ.()D";
    const UUID_TEXT: &str = "net/minecraft/world/entity/Entity.getStringUUID.()Ljava/lang/String;";
    const PUT_INT: &str = "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V";
    const GET_INT: &str = "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I";
    const PUT: &str = "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;";

    /// A cart as a mod writes one: it saves its laps (`Laps`) and reads them back, and keeps what
    /// it was told to ride (the runtime doesn't seat entities yet). Its factory makes one; a
    /// supplier gives its type; a function notes each entity it is handed.
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let e = args[0];
        let class = rt.class_of(e);
        match key {
            FACTORY_CREATE if class == "test/CartFactory" => {
                let cart = rt.new_object(CART);
                api::call(rt, ENTITY_INIT, &[cart, args[1], args[2]]);
                cart
            }
            ADD_SAVE if class == CART => {
                let (k, laps) = (rt.string("Laps"), rt.get_field(e, "laps"));
                api::call(rt, PUT_INT, &[args[1], k, V::I(laps.i())]);
                V::Void
            }
            READ_SAVE if class == CART => {
                let k = rt.string("Laps");
                let laps = api::call(rt, GET_INT, &[args[1], k]);
                rt.put_field(e, "laps", laps);
                V::Void
            }
            "startRiding.(Lnet/minecraft/world/entity/Entity;Z)Z" if class == CART => {
                rt.put_field(e, "rides", args[1]);
                rt.put_field(e, "forced", args[2]);
                V::I(1)
            }
            "isPassenger.()Z" if class == CART => {
                V::I(i32::from(rt.get_field(e, "rides").r() != 0))
            }
            "get.()Ljava/lang/Object;" if class == "test/CartSupplier" => rt.statics["t.cart"],
            "apply.(Ljava/lang/Object;)Ljava/lang/Object;" if class == "test/Noter" => {
                let seen = rt.statics["t.seen"];
                crate::jutil::list_add(rt, seen, args[1]);
                args[1]
            }
            _ => api::call(rt, owner_key, args),
        }
    }

    fn overrides(class: &str, key: &str) -> bool {
        class == CART && matches!(key, ADD_SAVE | READ_SAVE)
    }

    /// A runtime as the mod's: the cart's class, its type registered as `test:cart`.
    fn runtime() -> (Rt, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        rt.supers.insert(CART, ENTITY);
        let ty = rt.new_object(ENTITY_TYPE);
        let factory = rt.new_object("test/CartFactory");
        rt.put_field(ty, "$factory", factory);
        rt.registered
            .insert(("entity_type".into(), "test:cart".into()), ty);
        rt.statics.insert("t.cart", ty);
        (rt, ty)
    }

    fn cart(rt: &mut Rt, ty: V, at: [f64; 3], laps: i32) -> V {
        let level = crate::glue::level(rt);
        let cart = api::call(rt, CREATE_IN, &[ty, level]);
        api::call(rt, SET_POS, &[cart, V::D(at[0]), V::D(at[1]), V::D(at[2])]);
        rt.put_field(cart, "laps", V::I(laps));
        cart
    }

    fn saved(rt: &mut Rt, e: V) -> (bool, V) {
        let tag = crate::tags::new_compound(rt);
        (api::call(rt, SAVE, &[e, tag]).i() != 0, tag)
    }

    /// What a call giving an `Optional` holds (null for none).
    fn found(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        let optional = api::call(rt, key, args);
        rt.get_field(optional, "value")
    }

    fn text(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<String> {
        let v = api::call(rt, key, args);
        rt.str_of(v).map(str::to_owned)
    }

    fn compound(rt: &mut Rt, nbt: BTreeMap<String, Nbt>) -> V {
        crate::nbt::to_runtime(rt, &nbt)
    }

    /// A mod's entity saves under its type's id (then its place, its UUID and its own data), and
    /// `EntityType.create(tag, level)` makes a new one of that type from the compound: where the
    /// first was, with its UUID and its laps. The type is found by the compound and by its id,
    /// vanilla's too (a bare id is vanilla's); an id no registry holds finds none, and one vanilla
    /// can't read throws from `by` (`ResourceLocation.parse`) but finds none by `byString`.
    #[test]
    fn a_mod_entity_saves_under_its_types_id_and_a_new_one_loads_from_it() {
        let (mut rt, ty) = runtime();
        let level = crate::glue::level(&mut rt);
        let first = cart(&mut rt, ty, [1.5, 64.0, -3.25], 7);
        assert_eq!(
            text(&mut rt, ENCODE_ID, &[first]).as_deref(),
            Some("test:cart")
        );
        let (ok, tag) = saved(&mut rt, first);
        assert!(ok);
        let nbt = crate::nbt::from_runtime(&rt, tag);
        assert_eq!(nbt.get("id"), Some(&Nbt::String("test:cart".into())));
        assert_eq!(nbt.get("Laps"), Some(&Nbt::Int(7)));
        assert_eq!(
            nbt.get("Pos"),
            Some(&Nbt::List(vec![
                Nbt::Double(1.5),
                Nbt::Double(64.0),
                Nbt::Double(-3.25)
            ]))
        );

        let second = found(&mut rt, CREATE_FROM, &[tag, level]);
        assert!(second.r() != 0 && second != first, "a new entity");
        assert_eq!(rt.class_of(second), CART);
        assert_eq!(api::call(&mut rt, GET_X, &[second]).d(), 1.5);
        assert_eq!(api::call(&mut rt, GET_Z, &[second]).d(), -3.25);
        assert_eq!(rt.get_field(second, "laps").i(), 7);
        assert_eq!(
            text(&mut rt, UUID_TEXT, &[second]),
            text(&mut rt, UUID_TEXT, &[first])
        );

        assert_eq!(found(&mut rt, BY, &[tag]), ty);
        let id = rt.string("test:cart");
        assert_eq!(found(&mut rt, BY_STRING, &[id]), ty);
        let cow = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.COW");
        let bare = rt.string("cow");
        assert_eq!(found(&mut rt, BY_STRING, &[bare]), cow);
        for unknown in ["test:boat", "minecraft:unicorn", "Test:Cart"] {
            let s = rt.string(unknown);
            assert_eq!(found(&mut rt, BY_STRING, &[s]), V::NULL, "{unknown}");
        }
        let bad = compound(
            &mut rt,
            BTreeMap::from([("id".to_owned(), Nbt::String("Test:Cart".into()))]),
        );
        api::call(&mut rt, BY, &[bad]);
        let thrown = rt.thrown.take().expect("parse throws");
        assert_eq!(
            rt.class_of(thrown),
            "net/minecraft/ResourceLocationException"
        );
        let none = compound(
            &mut rt,
            BTreeMap::from([("id".to_owned(), Nbt::String("test:boat".into()))]),
        );
        assert_eq!(found(&mut rt, CREATE_FROM, &[none, level]), V::NULL);
        assert!(rt.thrown.is_none());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What `save` leaves out: a passenger (its vehicle saves it), though `saveAsPassenger` saves
    /// it; an entity discarded or killed, though one unloaded with its chunk saves; and an entity
    /// of a type built `noSave`, which has no id to save under, as vanilla's player and lightning
    /// bolts have none (a command can't summon the player either).
    #[test]
    fn passengers_entities_removed_for_good_and_unsaved_types_are_left_out() {
        let (mut rt, ty) = runtime();
        let (vehicle, rider) = (
            cart(&mut rt, ty, [0.0; 3], 1),
            cart(&mut rt, ty, [0.0; 3], 2),
        );
        rt.put_field(rider, "rides", vehicle);
        assert!(!saved(&mut rt, rider).0, "its vehicle saves it");
        let tag = crate::tags::new_compound(&mut rt);
        let as_passenger =
            "net/minecraft/world/entity/Entity.saveAsPassenger.(Lnet/minecraft/nbt/CompoundTag;)Z";
        assert_eq!(api::call(&mut rt, as_passenger, &[rider, tag]).i(), 1);

        let remove = "net/minecraft/world/entity/Entity.remove.(Lnet/minecraft/world/entity/Entity$RemovalReason;)V";
        let unloaded = api::get_static(
            &mut rt,
            "net/minecraft/world/entity/Entity$RemovalReason.UNLOADED_TO_CHUNK",
        );
        api::call(&mut rt, remove, &[vehicle, unloaded]);
        assert!(saved(&mut rt, vehicle).0, "unloaded with its chunk");
        let discarded = cart(&mut rt, ty, [0.0; 3], 3);
        api::call(
            &mut rt,
            "net/minecraft/world/entity/Entity.discard.()V",
            &[discarded],
        );
        assert!(!saved(&mut rt, discarded).0, "discarded");
        let should_save = "net/minecraft/world/entity/Entity$RemovalReason.shouldSave.()Z";
        let should_destroy = "net/minecraft/world/entity/Entity$RemovalReason.shouldDestroy.()Z";
        let killed = api::get_static(
            &mut rt,
            "net/minecraft/world/entity/Entity$RemovalReason.KILLED",
        );
        assert_eq!(
            (
                api::call(&mut rt, should_save, &[unloaded]).i(),
                api::call(&mut rt, should_destroy, &[unloaded]).i()
            ),
            (1, 0)
        );
        assert_eq!(
            (
                api::call(&mut rt, should_save, &[killed]).i(),
                api::call(&mut rt, should_destroy, &[killed]).i()
            ),
            (0, 1)
        );

        let builder_of = "net/minecraft/world/entity/EntityType$Builder.of.(Lnet/minecraft/world/entity/EntityType$EntityFactory;Lnet/minecraft/world/entity/MobCategory;)Lnet/minecraft/world/entity/EntityType$Builder;";
        let factory = rt.get_field(ty, "$factory");
        let misc = api::get_static(&mut rt, "net/minecraft/world/entity/MobCategory.MISC");
        let b = api::call(&mut rt, builder_of, &[factory, misc]);
        let b = api::call(
            &mut rt,
            "net/minecraft/world/entity/EntityType$Builder.noSave.()Lnet/minecraft/world/entity/EntityType$Builder;",
            &[b],
        );
        let name = rt.string("ghost");
        let ghost = api::call(
            &mut rt,
            "net/minecraft/world/entity/EntityType$Builder.build.(Ljava/lang/String;)Lnet/minecraft/world/entity/EntityType;",
            &[b, name],
        );
        rt.registered
            .insert(("entity_type".into(), "test:ghost".into()), ghost);
        let can_save = "net/minecraft/world/entity/EntityType.canSerialize.()Z";
        let can_summon = "net/minecraft/world/entity/EntityType.canSummon.()Z";
        assert_eq!(
            (
                api::call(&mut rt, can_save, &[ghost]).i(),
                api::call(&mut rt, can_summon, &[ghost]).i()
            ),
            (0, 1)
        );
        let unsaved = cart(&mut rt, ghost, [0.0; 3], 0);
        assert!(!saved(&mut rt, unsaved).0);
        assert_eq!(api::call(&mut rt, ENCODE_ID, &[unsaved]), V::NULL);
        for (field, save, summon) in [("PLAYER", 0, 0), ("LIGHTNING_BOLT", 0, 1), ("COW", 1, 1)] {
            let key = rt.static_key(&format!("net/minecraft/world/entity/EntityType.{field}"));
            let t = api::get_static(&mut rt, key);
            assert_eq!(
                (
                    api::call(&mut rt, can_save, &[t]).i(),
                    api::call(&mut rt, can_summon, &[t]).i()
                ),
                (save, summon),
                "{field}"
            );
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `loadEntityRecursive`: the vehicle the compound makes, handed to the function, then its
    /// passengers the same way, each told to ride it (forced); a compound whose id vanilla can't
    /// read makes none, the exception dropped as vanilla logs it.
    #[test]
    fn an_entity_loads_with_its_passengers_riding_it() {
        let (mut rt, ty) = runtime();
        let level = crate::glue::level(&mut rt);
        let (vehicle, rider) = (
            cart(&mut rt, ty, [4.0, 70.0, 4.0], 10),
            cart(&mut rt, ty, [4.0, 71.0, 4.0], 20),
        );
        let (_, tag) = saved(&mut rt, vehicle);
        let (_, rider_tag) = saved(&mut rt, rider);
        let list = rt.new_object("net/minecraft/nbt/ListTag");
        api::call(&mut rt, "net/minecraft/nbt/ListTag.<init>.()V", &[list]);
        api::call(
            &mut rt,
            "net/minecraft/nbt/ListTag.add.(Ljava/lang/Object;)Z",
            &[list, rider_tag],
        );
        let k = rt.string("Passengers");
        api::call(&mut rt, PUT, &[tag, k, list]);
        let seen = crate::jutil::new_list(&mut rt, Vec::new());
        rt.statics.insert("t.seen", seen);
        let noter = rt.new_object("test/Noter");

        let loaded = api::call(&mut rt, LOAD_RECURSIVE, &[tag, level, noter]);
        let order = crate::jutil::elements(&rt, seen);
        assert_eq!(order.len(), 2, "the vehicle, then its passenger");
        assert_eq!(order[0], loaded);
        assert_eq!(rt.get_field(loaded, "laps").i(), 10);
        let passenger = order[1];
        assert_eq!(rt.get_field(passenger, "laps").i(), 20);
        assert_eq!(rt.get_field(passenger, "rides"), loaded);
        assert_eq!(rt.get_field(passenger, "forced").i(), 1);

        let bad = compound(
            &mut rt,
            BTreeMap::from([("id".to_owned(), Nbt::String("Not Valid".into()))]),
        );
        assert_eq!(
            api::call(&mut rt, LOAD_RECURSIVE, &[bad, level, noter]),
            V::NULL
        );
        assert!(rt.thrown.is_none(), "dropped");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The game's own entity saves under its type's id what the game saves of it, but not one of a
    /// type vanilla never saves (a lightning bolt); it loads what the game lets a mod set (its
    /// place, its motion but a component over 10, its health, the mod data); its default loot table
    /// is under `entities/`.
    #[test]
    fn the_games_entity_saves_under_its_id_and_loads_what_the_game_lets_a_mod_set() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.entities.push(EntityView {
            id: 7,
            kind: "minecraft:cow".into(),
            pos: [1.5, 64.0, -2.5],
            yaw: 0.0,
            health: Some((10.0, 10.0)),
        });
        host.entities.push(EntityView {
            id: 9,
            kind: "minecraft:lightning_bolt".into(),
            pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            health: None,
        });
        host.entity_nbts.insert(
            7,
            BTreeMap::from([
                ("Health".to_owned(), Nbt::Float(10.0)),
                ("Air".to_owned(), Nbt::Short(300)),
            ]),
        );
        rt.with_host(&mut host, |rt| {
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            let (ok, tag) = saved(rt, cow);
            assert!(ok);
            let nbt = crate::nbt::from_runtime(rt, tag);
            assert_eq!(nbt.get("id"), Some(&Nbt::String("minecraft:cow".into())));
            assert_eq!(nbt.get("Air"), Some(&Nbt::Short(300)));
            let bolt = crate::vanilla_entities::of(rt, 9, "minecraft:lightning_bolt", None);
            assert!(!saved(rt, bolt).0);
            assert_eq!(api::call(rt, ENCODE_ID, &[bolt]), V::NULL);

            let ty = api::call(rt, "net/minecraft/world/entity/Entity.getType.()Lnet/minecraft/world/entity/EntityType;", &[cow]);
            let table = api::call(rt, "net/minecraft/world/entity/EntityType.getDefaultLootTable.()Lnet/minecraft/resources/ResourceKey;", &[ty]);
            let registry = api::get_static(rt, "net/minecraft/core/registries/Registries.LOOT_TABLE");
            let location = rt.string("minecraft:entities/cow");
            let want = api::call(rt, KEY_CREATE, &[registry, location]);
            assert!(rt.str_of(want).is_some());
            assert_eq!(rt.str_of(table), rt.str_of(want));
            let short = api::call(rt, "net/minecraft/world/entity/EntityType.toShortString.()Ljava/lang/String;", &[ty]);
            assert_eq!(rt.str_of(short), Some("cow"));

            let data = BTreeMap::from([("seen".to_owned(), Nbt::Byte(1))]);
            let tag = compound(
                rt,
                BTreeMap::from([
                    ("Pos".to_owned(), Nbt::List(vec![Nbt::Double(3.5), Nbt::Double(70.0), Nbt::Double(4.5)])),
                    ("Motion".to_owned(), Nbt::List(vec![Nbt::Double(0.25), Nbt::Double(12.0), Nbt::Double(0.0)])),
                    ("Health".to_owned(), Nbt::Float(4.0)),
                    ("NeoForgeData".to_owned(), Nbt::Compound(data)),
                ]),
            );
            api::call(rt, "net/minecraft/world/entity/Entity.load.(Lnet/minecraft/nbt/CompoundTag;)V", &[cow, tag]);
            let persistent = api::call(rt, "net/minecraft/world/entity/Entity.getPersistentData.()Lnet/minecraft/nbt/CompoundTag;", &[cow]);
            let k = rt.string("seen");
            assert_eq!(api::call(rt, "net/minecraft/nbt/CompoundTag.getBoolean.(Ljava/lang/String;)Z", &[persistent, k]).i(), 1);
        });
        let cow = host.entities.iter().find(|e| e.id == 7).expect("the cow");
        assert_eq!(cow.pos, [3.5, 70.0, 4.5]);
        assert_eq!(cow.health, Some((4.0, 10.0)));
        assert_eq!(host.motions.get(&7), Some(&[0.25, 0.0, 0.0]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a spawn egg spawns: the type its stack's entity data names (`id`) when the registry
    /// holds it, else the egg's own — a NeoForge egg's from its supplier, vanilla's by its name —
    /// and the egg of a type: the mod's for its type, vanilla's for vanilla's, none for a type
    /// without one.
    #[test]
    fn a_spawn_egg_spawns_its_own_type_or_the_one_its_stack_names() {
        let (mut rt, ty) = runtime();
        let egg = rt.new_object(DEFERRED_SPAWN_EGG);
        let supplier = rt.new_object("test/CartSupplier");
        let props = rt.new_object("net/minecraft/world/item/Item$Properties");
        api::call(
            &mut rt,
            "net/neoforged/neoforge/common/DeferredSpawnEggItem.<init>.(Ljava/util/function/Supplier;IILnet/minecraft/world/item/Item$Properties;)V",
            &[egg, supplier, V::I(0x11_2233), V::I(0x44_5566), props],
        );
        rt.registered
            .insert(("item".into(), "test:cart_spawn_egg".into()), egg);
        let stack = rt.new_object("net/minecraft/world/item/ItemStack");
        api::call(
            &mut rt,
            "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;I)V",
            &[stack, egg, V::I(1)],
        );
        let get_type = EGG_TYPE.0;
        assert_eq!(api::call(&mut rt, get_type, &[egg, stack]), ty);
        let color = "net/minecraft/world/item/SpawnEggItem.getColor.(I)I";
        assert_eq!(
            (
                api::call(&mut rt, color, &[egg, V::I(0)]).i(),
                api::call(&mut rt, color, &[egg, V::I(1)]).i()
            ),
            (0x11_2233, 0x44_5566)
        );

        let cow = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.COW");
        let set = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        let of = "net/minecraft/world/item/component/CustomData.of.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/world/item/component/CustomData;";
        let entity_data = api::get_static(
            &mut rt,
            "net/minecraft/core/component/DataComponents.ENTITY_DATA",
        );
        for (id, want) in [("minecraft:cow", cow), ("test:boat", ty)] {
            let tag = compound(
                &mut rt,
                BTreeMap::from([("id".to_owned(), Nbt::String(id.into()))]),
            );
            let data = api::call(&mut rt, of, &[tag]);
            api::call(&mut rt, set, &[stack, entity_data, data]);
            assert_eq!(api::call(&mut rt, get_type, &[egg, stack]), want, "{id}");
        }
        let spawns = "net/minecraft/world/item/SpawnEggItem.spawnsEntity.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/EntityType;)Z";
        assert_eq!(api::call(&mut rt, spawns, &[egg, stack, ty]).i(), 1);

        let vanilla = crate::api_common::item(&mut rt, "minecraft:cow_spawn_egg");
        let plain = crate::api_common::stack(&mut rt, "minecraft:cow_spawn_egg", 1);
        assert_eq!(api::call(&mut rt, get_type, &[vanilla, plain]), cow);
        let by_id = "net/minecraft/world/item/SpawnEggItem.byId.(Lnet/minecraft/world/entity/EntityType;)Lnet/minecraft/world/item/SpawnEggItem;";
        assert_eq!(api::call(&mut rt, by_id, &[ty]), egg);
        assert_eq!(api::call(&mut rt, by_id, &[cow]), vanilla);
        let player = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.PLAYER");
        assert_eq!(api::call(&mut rt, by_id, &[player]), V::NULL);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
