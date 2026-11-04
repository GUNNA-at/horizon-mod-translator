//! Item entities for translated mods (`ItemEntity`), natively. This covers:
//! - the server level's item entities as objects (`Level.getEntitiesOfClass`, `getEntities`): one
//!   per entity, read through the Host and written back through it (`setItem`, `discard`,
//!   `setDeltaMovement`, `setPos`), with the NeoForge persistent data a mod keeps on them;
//! - new ones (`new ItemEntity(...)` and `addFreshEntity`, `Block.popResource`,
//!   `Containers.dropItemStack`);
//! - the contacts vanilla's `Entity.move` reports to the blocks an item entity rests on or is in
//!   (`updateEntityAfterFallOn`, `entityInside`), for a mod block with a block entity, asked from
//!   the block entity's tick ([`item_contacts`]). Create's basins, belts, depots, chutes, saws,
//!   millstones, crushing wheels and funnels take what lands on them this way.
use crate::host::{ItemEntity as View, boxes_meet};
use crate::{Rt, V, api};

pub const ITEM_ENTITY: &str = "net/minecraft/world/entity/item/ItemEntity";
const LIST: &str = "compat:item-entities";
const PREDICATE_TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
/// Proxies of entities no query has returned for this long are let go.
const FORGET_AFTER: i64 = 200;
const AFTER_FALL_ON: &str = "updateEntityAfterFallOn.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/world/entity/Entity;)V";
const AFTER_FALL_ON_KEY: &str = "net/minecraft/world/level/block/Block.updateEntityAfterFallOn.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/world/entity/Entity;)V";
const ENTITY_INSIDE: &str = "entityInside.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)V";
const ENTITY_INSIDE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.entityInside.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)V";

fn game_time(rt: &Rt) -> i64 {
    rt.host().map_or(0, |h| h.game_time())
}

/// The proxies made so far, in a list object (kept from collection by the intern table).
fn proxies(rt: &mut Rt) -> V {
    rt.intern(LIST.into(), |rt| crate::jutil::new_list(rt, Vec::new()))
}

fn list_push(rt: &mut Rt, list: V, v: V) {
    if let Some(crate::Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.push(v);
    }
}

fn list_set(rt: &mut Rt, list: V, items: Vec<V>) {
    if let Some(crate::Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        *l = items;
    }
}

/// The object for item entity `view`, its fields brought up to date: the same one for an entity
/// each time while queries keep returning it.
pub fn proxy(rt: &mut Rt, view: &View) -> V {
    let list = proxies(rt);
    let now = game_time(rt);
    let known = crate::jutil::elements(rt, list).into_iter().find(|&e| rt.get_field(e, "$id").i() == view.id as i32);
    let e = match known {
        Some(e) => e,
        None => {
            let e = rt.new_object(ITEM_ENTITY);
            rt.put_field(e, "$id", V::I(view.id as i32));
            rt.put_field(e, "lifespan", V::I(6000));
            list_push(rt, list, e);
            e
        }
    };
    for (k, v) in ["$x", "$y", "$z"].into_iter().zip(view.pos) {
        rt.put_field(e, k, V::D(v));
    }
    for (k, v) in ["$mx", "$my", "$mz"].into_iter().zip(view.motion) {
        rt.put_field(e, k, V::D(v));
    }
    rt.put_field(e, "$onGround", V::I(view.on_ground as i32));
    rt.put_field(e, "$pickupDelay", V::I(view.pickup_delay as i32));
    rt.put_field(e, "$age", V::I(view.age as i32));
    rt.put_field(e, "$removed", V::I(0));
    rt.put_field(e, "$seen", V::J(now));
    // The stack object is kept while it holds what the game has (a mod may hold on to it).
    let old = rt.get_field(e, "$item");
    let same = old.r() != 0
        && rt.get_field(old, "$count").i() == view.count as i32
        && crate::api_common::item_like_name(rt, rt.get_field(old, "$item")) == view.item;
    if !same {
        let s = crate::api_common::stack(rt, &view.item, view.count as i32);
        rt.put_field(e, "$item", s);
    }
    e
}

/// Lets go of proxies no query has returned for a while (picked up, merged, despawned).
fn forget_stale(rt: &mut Rt) {
    let list = proxies(rt);
    let now = game_time(rt);
    let keep: Vec<V> = crate::jutil::elements(rt, list)
        .into_iter()
        .filter(|&e| rt.get_field(e, "$removed").i() == 0 && now - rt.get_field(e, "$seen").j() <= FORGET_AFTER)
        .collect();
    list_set(rt, list, keep);
}

/// The server level's item entities whose box meets `bounds`, as objects.
pub fn item_entities_in(rt: &mut Rt, bounds: [f64; 6]) -> Vec<V> {
    let views = rt.host().map(|h| h.item_entities(bounds)).unwrap_or_default();
    forget_stale(rt);
    views.iter().map(|v| proxy(rt, v)).collect()
}

fn id(rt: &Rt, e: V) -> u32 {
    rt.get_field(e, "$id").i().max(0) as u32
}

fn pos(rt: &Rt, e: V) -> [f64; 3] {
    [rt.get_field(e, "$x").d(), rt.get_field(e, "$y").d(), rt.get_field(e, "$z").d()]
}

/// Where an entity the game or a mod placed is; None for one with no place here (the player a
/// hook acts for).
pub(crate) fn position(rt: &Rt, e: V) -> Option<[f64; 3]> {
    matches!(rt.get_field(e, "$x"), V::D(_)).then(|| pos(rt, e))
}

fn motion(rt: &Rt, e: V) -> [f64; 3] {
    [rt.get_field(e, "$mx").d(), rt.get_field(e, "$my").d(), rt.get_field(e, "$mz").d()]
}

fn bounds(rt: &Rt, e: V) -> [f64; 6] {
    let [x, y, z] = pos(rt, e);
    [x - 0.125, y, z - 0.125, x + 0.125, y + 0.25, z + 0.125]
}

fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
    let v = rt.new_object("net/minecraft/world/phys/Vec3");
    for (k, x) in ["x", "y", "z"].into_iter().zip(p) {
        rt.put_field(v, k, V::D(x));
    }
    v
}

fn xyz(rt: &Rt, v: V) -> [f64; 3] {
    [rt.get_field(v, "x").d(), rt.get_field(v, "y").d(), rt.get_field(v, "z").d()]
}

/// The item and count of a stack object.
fn stack_parts(rt: &Rt, s: V) -> (String, u32) {
    if crate::api_common::stack_empty(rt, s) {
        return ("minecraft:air".into(), 0);
    }
    (crate::api_common::item_like_name(rt, rt.get_field(s, "$item")), rt.get_field(s, "$count").i().max(0) as u32)
}

/// Whether `e` is in the level yet (a new `ItemEntity` isn't until `addFreshEntity`).
fn spawned(rt: &Rt, e: V) -> bool {
    id(rt, e) != 0
}

/// `setItem`: the entity's stack, sent to the game.
fn set_item(rt: &mut Rt, e: V, stack: V) {
    rt.put_field(e, "$item", stack);
    if !spawned(rt, e) {
        return;
    }
    let (item, count) = stack_parts(rt, stack);
    let i = id(rt, e);
    if let Some(h) = rt.host_mut() {
        h.set_item_entity(i, &item, count);
    }
    if count == 0 {
        rt.put_field(e, "$removed", V::I(1));
    }
}

/// `discard`, `kill`, `remove`: the entity leaves the level.
fn remove(rt: &mut Rt, e: V) {
    if rt.get_field(e, "$removed").i() != 0 {
        return;
    }
    rt.put_field(e, "$removed", V::I(1));
    if spawned(rt, e) {
        let i = id(rt, e);
        if let Some(h) = rt.host_mut() {
            h.set_item_entity(i, "minecraft:air", 0);
        }
    }
}

fn move_to(rt: &mut Rt, e: V, p: Option<[f64; 3]>, m: Option<[f64; 3]>) {
    if let Some(p) = p {
        for (k, v) in ["$x", "$y", "$z"].into_iter().zip(p) {
            rt.put_field(e, k, V::D(v));
        }
    }
    if let Some(m) = m {
        for (k, v) in ["$mx", "$my", "$mz"].into_iter().zip(m) {
            rt.put_field(e, k, V::D(v));
        }
    }
    if spawned(rt, e) {
        let i = id(rt, e);
        if let Some(h) = rt.host_mut() {
            h.move_item_entity(i, p, m);
        }
    }
}

/// `addFreshEntity` for a new item entity: the game adds it.
fn add(rt: &mut Rt, e: V) -> bool {
    if spawned(rt, e) || rt.get_field(e, "$removed").i() != 0 {
        return false;
    }
    let (item, count) = stack_parts(rt, rt.get_field(e, "$item"));
    if count == 0 {
        return false;
    }
    let (p, m, delay) = (pos(rt, e), motion(rt, e), rt.get_field(e, "$pickupDelay").i().max(0) as u32);
    let Some(new) = rt.host_mut().and_then(|h| h.spawn_item_entity(&item, count, p, m, delay)) else { return false };
    rt.put_field(e, "$id", V::I(new as i32));
    rt.put_field(e, "$seen", V::J(game_time(rt)));
    let list = proxies(rt);
    list_push(rt, list, e);
    true
}

/// A new item entity at `p` with motion `m` (`new ItemEntity(level, x, y, z, stack, dx, dy, dz)`),
/// not in the level yet.
fn new_item_entity(rt: &mut Rt, e: V, p: [f64; 3], stack: V, m: [f64; 3]) {
    rt.put_field(e, "$id", V::I(0));
    rt.put_field(e, "lifespan", V::I(6000));
    move_to(rt, e, Some(p), Some(m));
    rt.put_field(e, "$item", stack);
}

/// A double in `[lo, hi)` from the runtime's random stream (`Mth.nextDouble`).
fn random_between(rt: &mut Rt, lo: f64, hi: f64) -> f64 {
    let d = (crate::api_common::next_random(rt) >> 11) as f64 / (1u64 << 53) as f64;
    lo + d * (hi - lo)
}

/// `Block.popResource(level, pos, stack)`: the stack falls out of the block.
fn pop_resource(rt: &mut Rt, pos: [i32; 3], stack: V) {
    if crate::api_common::stack_empty(rt, stack) {
        return;
    }
    let half = 0.25 / 2.0;
    let p = [
        pos[0] as f64 + 0.5 + random_between(rt, -0.25, 0.25),
        pos[1] as f64 + 0.5 + random_between(rt, -0.25, 0.25) - half,
        pos[2] as f64 + 0.5 + random_between(rt, -0.25, 0.25),
    ];
    let m = [random_between(rt, -0.1, 0.1), 0.2, random_between(rt, -0.1, 0.1)];
    let e = rt.new_object(ITEM_ENTITY);
    new_item_entity(rt, e, p, stack, m);
    rt.put_field(e, "$pickupDelay", V::I(10));
    add(rt, e);
}

/// The entity's NeoForge persistent data (`getPersistentData`): kept with its object.
fn persistent_data(rt: &mut Rt, e: V) -> V {
    let d = rt.get_field(e, "$data");
    if d.r() != 0 {
        return d;
    }
    let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
    api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
    rt.put_field(e, "$data", tag);
    tag
}

fn is_item_entity(rt: &Rt, v: V) -> bool {
    v.r() != 0 && !matches!(v, V::Void) && rt.class_of(v) == ITEM_ENTITY
}

/// Whether a `Class` argument asks for item entities: `ItemEntity.class`, `Entity.class`.
fn wants_items(rt: &Rt, class: V) -> bool {
    let name = rt.str_of(class).unwrap_or("").replace('.', "/");
    name.is_empty() || rt.is_a(ITEM_ENTITY, &name)
}

/// `getEntitiesOfClass(class, box[, predicate])` and `getEntities(except, box[, predicate])`.
fn entities_of(rt: &mut Rt, class: Option<V>, area: V, predicate: Option<V>) -> V {
    let items = class.is_none_or(|c| wants_items(rt, c));
    let b = crate::shapes::aabb_of(rt, area);
    let mut found = if items { item_entities_in(rt, b) } else { Vec::new() };
    // A mod's own entities (Create's contraptions), of the class asked for.
    let wanted = class.map(|c| rt.str_of(c).unwrap_or("").replace('.', "/"));
    found.extend(crate::mod_entities::in_box(rt, b).into_iter().filter(|&e| wanted.as_deref().is_none_or(|w| w.is_empty() || rt.instance_of(e, w))));
    let mut out = Vec::new();
    for e in found {
        let keep = match predicate {
            Some(p) if p.r() != 0 => rt.call_functional(PREDICATE_TEST, p, &[e]).i() != 0,
            _ => true,
        };
        if keep {
            out.push(e);
        }
    }
    crate::jutil::new_list(rt, out)
}

pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    call(rt, owner, name, desc, args)
}

/// Item entity calls; None for anything else.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // The level's entity queries.
    if matches!(name, "getEntitiesOfClass" | "getEntities")
        && (owner.ends_with("/Level") || owner.ends_with("/ServerLevel") || owner.ends_with("/ClientLevel") || owner.ends_with("/EntityGetter") || owner.ends_with("/LevelAccessor") || owner.ends_with("/WorldGenLevel"))
        && desc.starts_with("(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;")
    {
        let predicate = desc.contains("Predicate").then(|| a(3));
        return Some(entities_of(rt, Some(a(1)), a(2), predicate));
    }
    if name == "getEntities" && desc.starts_with("(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/phys/AABB;") {
        let predicate = desc.contains("Predicate").then(|| a(3));
        let except = a(1);
        let list = entities_of(rt, None, a(2), predicate);
        if except.r() != 0 {
            let kept: Vec<V> = crate::jutil::elements(rt, list).into_iter().filter(|&e| e != except).collect();
            list_set(rt, list, kept);
        }
        return Some(list);
    }
    if name == "addFreshEntity" && is_item_entity(rt, a(1)) {
        return Some(V::I(add(rt, a(1)) as i32));
    }
    match (owner, name) {
        // Vanilla's own: the game's physics stops the fall; nothing is inside a plain block.
        ("net/minecraft/world/level/block/Block" | "net/minecraft/world/level/block/state/BlockBehaviour", "updateEntityAfterFallOn" | "entityInside" | "stepOn") => {
            return Some(V::Void);
        }
        (ITEM_ENTITY, "<init>") => {
            let e = a(0);
            match desc {
                "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/item/ItemStack;)V" => {
                    let m = [random_between(rt, -0.1, 0.1), 0.2, random_between(rt, -0.1, 0.1)];
                    new_item_entity(rt, e, [a(2).d(), a(3).d(), a(4).d()], a(5), m);
                }
                "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/item/ItemStack;DDD)V" => {
                    new_item_entity(rt, e, [a(2).d(), a(3).d(), a(4).d()], a(5), [a(6).d(), a(7).d(), a(8).d()]);
                }
                // `ItemEntity(EntityType, Level)`: placed later.
                _ => {
                    let empty = crate::api_common::stack(rt, "minecraft:air", 0);
                    new_item_entity(rt, e, [0.0; 3], empty, [0.0; 3]);
                }
            }
            return Some(V::Void);
        }
        ("net/minecraft/world/level/block/Block", "popResource") if desc == "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/item/ItemStack;)V" => {
            let p = api::pos_of(rt, a(1));
            pop_resource(rt, p, a(2));
            return Some(V::Void);
        }
        ("net/minecraft/world/Containers", "dropItemStack") if desc == "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/item/ItemStack;)V" => {
            // Containers.dropItemStack: the stack in pieces of 10 to 30 (`random.nextInt(21) + 10`).
            let stack = a(4);
            let (item, mut count) = stack_parts(rt, stack);
            let (x, y, z) = (a(1).d(), a(2).d(), a(3).d());
            let size = 0.25;
            let spread = 1.0 - size;
            let half = size / 2.0;
            let p = [
                x.floor() + random_between(rt, 0.0, 1.0) * spread + half,
                y.floor() + random_between(rt, 0.0, 1.0) * spread,
                z.floor() + random_between(rt, 0.0, 1.0) * spread + half,
            ];
            while count > 0 {
                let piece = (10 + (random_between(rt, 0.0, 21.0) as u32)).min(count);
                count -= piece;
                let s = crate::api_common::stack(rt, &item, piece as i32);
                let m = [random_between(rt, -0.05, 0.05) * 1.1485, random_between(rt, -0.05, 0.05) * 1.1485 + 0.2, random_between(rt, -0.05, 0.05) * 1.1485];
                let e = rt.new_object(ITEM_ENTITY);
                new_item_entity(rt, e, p, s, m);
                add(rt, e);
            }
            return Some(V::Void);
        }
        _ => {}
    }
    // An item entity's own methods, whichever class names them (`Entity`, `ItemEntity`,
    // NeoForge's `IEntityExtension`).
    let e = a(0);
    if !is_item_entity(rt, e) {
        return None;
    }
    Some(match name {
        "getItem" => {
            let s = rt.get_field(e, "$item");
            if s.r() == 0 { crate::api_common::stack(rt, "minecraft:air", 0) } else { s }
        }
        "setItem" => {
            set_item(rt, e, a(1));
            V::Void
        }
        "discard" | "kill" | "remove" | "setRemoved" | "onClientRemoval" => {
            remove(rt, e);
            V::Void
        }
        "isAlive" => V::I((rt.get_field(e, "$removed").i() == 0) as i32),
        "isRemoved" => V::I((rt.get_field(e, "$removed").i() != 0) as i32),
        "getId" => V::I(id(rt, e) as i32),
        "level" | "getCommandSenderWorld" => crate::glue::level(rt),
        "blockPosition" | "getOnPos" | "getOnPosLegacy" => {
            let [x, y, z] = pos(rt, e);
            let y = if name == "blockPosition" { y } else { y - 0.2 };
            api::block_pos(rt, x.floor() as i64, y.floor() as i64, z.floor() as i64)
        }
        "position" => {
            let p = pos(rt, e);
            vec3(rt, p)
        }
        "getX" => V::D(pos(rt, e)[0]),
        "getY" => V::D(pos(rt, e)[1]),
        "getZ" => V::D(pos(rt, e)[2]),
        "getDeltaMovement" => {
            let m = motion(rt, e);
            vec3(rt, m)
        }
        "setDeltaMovement" => {
            let m = if desc == "(DDD)V" { [a(1).d(), a(2).d(), a(3).d()] } else { xyz(rt, a(1)) };
            move_to(rt, e, None, Some(m));
            V::Void
        }
        "addDeltaMovement" | "push" if desc == "(Lnet/minecraft/world/phys/Vec3;)V" || desc == "(DDD)V" => {
            let d = if desc == "(DDD)V" { [a(1).d(), a(2).d(), a(3).d()] } else { xyz(rt, a(1)) };
            let m = motion(rt, e);
            move_to(rt, e, None, Some([m[0] + d[0], m[1] + d[1], m[2] + d[2]]));
            V::Void
        }
        "setPos" | "moveTo" | "teleportTo" | "absMoveTo" => {
            let p = if desc.starts_with("(Lnet/minecraft/world/phys/Vec3;") { xyz(rt, a(1)) } else { [a(1).d(), a(2).d(), a(3).d()] };
            move_to(rt, e, Some(p), None);
            V::Void
        }
        "onGround" => rt.get_field(e, "$onGround"),
        "getBoundingBox" => {
            let b = bounds(rt, e);
            crate::shapes::aabb(rt, b)
        }
        "getBbWidth" | "getBbHeight" => V::F(0.25),
        "getAge" => rt.get_field(e, "$age"),
        "hasPickUpDelay" => V::I((rt.get_field(e, "$pickupDelay").i() > 0) as i32),
        "setPickUpDelay" => {
            rt.put_field(e, "$pickupDelay", a(1));
            V::Void
        }
        "setDefaultPickUpDelay" => {
            rt.put_field(e, "$pickupDelay", V::I(10));
            V::Void
        }
        "setNoPickUpDelay" => {
            rt.put_field(e, "$pickupDelay", V::I(0));
            V::Void
        }
        "getPersistentData" => persistent_data(rt, e),
        "isOnFire" | "isInWater" | "isInLava" | "isSpectator" | "isShiftKeyDown" | "isPassenger" | "isVehicle" | "fireImmune" => V::I(0),
        "getOwner" | "getTarget" | "getVehicle" | "getControllingPassenger" => V::NULL,
        _ => return None,
    })
}

/// What vanilla's `Entity.move` tells blocks of item entities on or in them, for the mod block at
/// `pos` (whose object is `block`, in state `state`): `updateEntityAfterFallOn` for an item resting
/// on it, in its cell or on its top (an item `Entity.getOnPosLegacy` names it for, or one the
/// game's collision box holds lower in the cell than the mod's item-only shape would: a basin's),
/// then `entityInside` for an item whose box meets its cell. Only for blocks overriding either.
pub fn item_contacts(rt: &mut Rt, at: [i32; 3], block: V, state: V) {
    let class = rt.class_of(block);
    let overrides = |k: &str| rt.overrides.is_some_and(|o| o(class, k));
    let (falls, inside) = (overrides(AFTER_FALL_ON), overrides(ENTITY_INSIDE));
    if !falls && !inside {
        return;
    }
    let Some(vc) = rt.vcall else { return };
    let [x, y, z] = at.map(f64::from);
    let cell = [x, y, z, x + 1.0, y + 1.0, z + 1.0];
    // An item on the block's top has its feet at y + 1, its box above the cell.
    let around = [x, y, z, x + 1.0, y + 1.25, z + 1.0];
    let level = crate::glue::level(rt);
    let p = api::block_pos(rt, at[0] as i64, at[1] as i64, at[2] as i64);
    for e in item_entities_in(rt, around) {
        let [ex, ey, ez] = pos(rt, e);
        let column = ex.floor() == x && ez.floor() == z;
        if falls && column && rt.get_field(e, "$onGround").i() != 0 && ((ey - 0.2).floor() == y || ey.floor() == y) {
            vc(rt, AFTER_FALL_ON_KEY, AFTER_FALL_ON, &[block, level, e]);
            rt.report_thrown("updateEntityAfterFallOn");
        }
        if inside && rt.get_field(e, "$removed").i() == 0 && boxes_meet(&bounds(rt, e), &cell) {
            vc(rt, ENTITY_INSIDE_KEY, ENTITY_INSIDE, &[block, state, level, p, e]);
            rt.report_thrown("entityInside");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, MemoryHost};

    const BASIN: &str = "test/BasinBlock";
    const BLOCK_POSITION: &str = "net/minecraft/world/entity/Entity.blockPosition.()Lnet/minecraft/core/BlockPos;";
    const GET_ITEM: &str = "net/minecraft/world/entity/item/ItemEntity.getItem.()Lnet/minecraft/world/item/ItemStack;";
    const DISCARD: &str = "net/minecraft/world/entity/Entity.discard.()V";

    /// A basin as Create writes it: an item entity whose block position is the basin's goes in
    /// (the test keeps its stack in `t.took`), and leaves the level.
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) == BASIN && key == AFTER_FALL_ON {
            let e = args[2];
            let at = api::call(rt, BLOCK_POSITION, &[e]);
            if api::pos_of(rt, at) == [0, 64, 0] {
                let stack = api::call(rt, GET_ITEM, &[e]);
                let mut took = crate::jutil::elements(rt, rt.statics.get("t.took").copied().unwrap_or_default());
                took.push(stack);
                let list = crate::jutil::new_list(rt, took);
                rt.statics.insert("t.took", list);
                api::call(rt, DISCARD, &[e]);
            }
            return V::Void;
        }
        api::call(rt, owner_key, args)
    }

    fn overrides(class: &str, key: &str) -> bool {
        class == BASIN && key == AFTER_FALL_ON
    }

    fn item(id: u32, pos: [f64; 3], on_ground: bool, what: &str) -> View {
        View { id, pos, on_ground, item: what.into(), count: 2, ..View::default() }
    }

    #[test]
    fn an_item_resting_in_a_mod_block_is_handed_to_it_and_one_falling_past_is_not() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        let basin = rt.new_object(BASIN);
        let state = rt.new_object("net/minecraft/world/level/block/state/BlockState");
        let mut host = MemoryHost::default();
        host.item_entities = vec![
            // In the basin, on its floor (2/16 up): the game's collision box holds it there.
            item(1, [0.5, 64.125, 0.5], true, "minecraft:andesite"),
            // Falling through the basin's space, not landed yet.
            item(2, [0.4, 64.6, 0.4], false, "minecraft:iron_nugget"),
            // On the block beside it.
            item(3, [1.5, 64.125, 0.5], true, "minecraft:cobblestone"),
        ];
        rt.with_host(&mut host, |rt| item_contacts(rt, [0, 64, 0], basin, state));
        let took = crate::jutil::elements(&rt, rt.statics.get("t.took").copied().unwrap_or_default());
        let names: Vec<(String, u32)> = took.iter().map(|&s| stack_parts(&rt, s)).collect();
        assert_eq!(names, [("minecraft:andesite".to_owned(), 2)]);
        let left: Vec<u32> = host.item_entities.iter().map(|e| e.id).collect();
        assert_eq!(left, [2, 3]);
    }

    #[test]
    fn a_mod_finds_item_entities_by_class_and_box_and_adds_new_ones() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.item_entities = vec![item(7, [0.5, 64.0, 0.5], true, "minecraft:iron_ingot"), item(8, [5.5, 64.0, 0.5], true, "minecraft:gold_ingot")];
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let class = crate::jlang::class_object(&mut rt, ITEM_ENTITY);
        let area = crate::shapes::aabb(&mut rt, [0.0, 63.0, 0.0, 2.0, 66.0, 2.0]);
        let query = "net/minecraft/world/level/Level.getEntitiesOfClass.(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;)Ljava/util/List;";
        let (found, again) = rt.with_host(&mut host, |rt| {
            let found = api::call(rt, query, &[level, class, area]);
            let again = api::call(rt, query, &[level, class, area]);
            (crate::jutil::elements(rt, found), crate::jutil::elements(rt, again))
        });
        assert_eq!(found.len(), 1);
        // The same entity is the same object while it lives.
        assert_eq!(found, again);
        let e = found[0];
        let stack = api::call(&mut rt, GET_ITEM, &[e]);
        assert_eq!(stack_parts(&rt, stack), ("minecraft:iron_ingot".to_owned(), 2));
        // Half of it taken: the game's entity keeps the rest.
        let rest = crate::api_common::stack(&mut rt, "minecraft:iron_ingot", 1);
        rt.with_host(&mut host, |rt| api::call(rt, "net/minecraft/world/entity/item/ItemEntity.setItem.(Lnet/minecraft/world/item/ItemStack;)V", &[e, rest]));
        assert_eq!(host.item_entities[0].count, 1);

        // A new one: made, then added.
        let fresh = rt.new_object(ITEM_ENTITY);
        let sheet = crate::api_common::stack(&mut rt, "create:iron_sheet", 1);
        let added = rt.with_host(&mut host, |rt| {
            api::call(rt, "net/minecraft/world/entity/item/ItemEntity.<init>.(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/item/ItemStack;DDD)V", &[fresh, level, V::D(1.5), V::D(65.0), V::D(1.5), sheet, V::D(0.0), V::D(0.1), V::D(0.0)]);
            api::call(rt, "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z", &[level, fresh])
        });
        assert_eq!(added, V::I(1));
        let new = host.item_entities.last().cloned().unwrap();
        assert_eq!((new.item.as_str(), new.count, new.pos, new.motion), ("create:iron_sheet", 1, [1.5, 65.0, 1.5], [0.0, 0.1, 0.0]));
        assert_eq!(rt.get_field(fresh, "$id").i() as u32, new.id);
        let _ = host.block_state([0, 0, 0]);
    }
}
