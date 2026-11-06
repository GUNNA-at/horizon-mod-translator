//! Vanilla's projectile classes for a mod's projectile: a mod entity whose class extends
//! `Projectile` or a class below it (Create's potato, a spell's bolt, a cannon's shell). How one is
//! made (`ThrowableProjectile(type, shooter, level)`...), who shot it (`setOwner`, `getOwner`), and
//! what it hits: `EntityHitResult`, `ProjectileUtil`'s searches along a path (the blocks through the
//! level's `clip`, then the entities whose boxes the path crosses), and `onHit`, which hands an
//! entity hit to the projectile's `onHitEntity` and a block hit to its `onHitBlock`, the mod's own
//! where its class has them. As 1.21.1 does; a game event (`PROJECTILE_LAND`), a deflecting target
//! (a breeze) and a vanilla block's own answer to a hit (a bell, a target block) are not modelled.
use crate::{Rt, V, api, jutil};

const ENTITY: &str = "net/minecraft/world/entity/Entity";
pub(crate) const PROJECTILE: &str = "net/minecraft/world/entity/projectile/Projectile";
const HURTING: &str = "net/minecraft/world/entity/projectile/AbstractHurtingProjectile";
const UTIL: &str = "net/minecraft/world/entity/projectile/ProjectileUtil";
const DEFLECTION: &str = "net/minecraft/world/entity/projectile/ProjectileDeflection";
pub(crate) const ENTITY_HIT: &str = "net/minecraft/world/phys/EntityHitResult";
const HIT_RESULT: &str = "net/minecraft/world/phys/HitResult";
const HIT_TYPE: &str = "net/minecraft/world/phys/HitResult$Type";

/// A call the runtime makes as the JVM dispatches it ([`api::virtual_call`]): its key, and its
/// `name.desc`.
type Virtual = (&'static str, &'static str);
const ON_HIT: Virtual = ("net/minecraft/world/entity/projectile/Projectile.onHit.(Lnet/minecraft/world/phys/HitResult;)V", "onHit.(Lnet/minecraft/world/phys/HitResult;)V");
const ON_HIT_ENTITY: Virtual = (
    "net/minecraft/world/entity/projectile/Projectile.onHitEntity.(Lnet/minecraft/world/phys/EntityHitResult;)V",
    "onHitEntity.(Lnet/minecraft/world/phys/EntityHitResult;)V",
);
const ON_HIT_BLOCK: Virtual = (
    "net/minecraft/world/entity/projectile/Projectile.onHitBlock.(Lnet/minecraft/world/phys/BlockHitResult;)V",
    "onHitBlock.(Lnet/minecraft/world/phys/BlockHitResult;)V",
);
const HIT_GET_TYPE: Virtual = ("net/minecraft/world/phys/HitResult.getType.()Lnet/minecraft/world/phys/HitResult$Type;", "getType.()Lnet/minecraft/world/phys/HitResult$Type;");
const CAN_BE_HIT: Virtual = ("net/minecraft/world/entity/Entity.canBeHitByProjectile.()Z", "canBeHitByProjectile.()Z");
const PICK_RADIUS: Virtual = ("net/minecraft/world/entity/Entity.getPickRadius.()F", "getPickRadius.()F");
const IS_ALIVE: Virtual = ("net/minecraft/world/entity/Entity.isAlive.()Z", "isAlive.()Z");
const IS_REMOVED: Virtual = ("net/minecraft/world/entity/Entity.isRemoved.()Z", "isRemoved.()Z");
const IS_PICKABLE: Virtual = ("net/minecraft/world/entity/Entity.isPickable.()Z", "isPickable.()Z");
const IS_SPECTATOR: Virtual = ("net/minecraft/world/entity/Entity.isSpectator.()Z", "isSpectator.()Z");
const GET_UUID: Virtual = ("net/minecraft/world/entity/Entity.getUUID.()Ljava/util/UUID;", "getUUID.()Ljava/util/UUID;");
const GET_X: Virtual = ("net/minecraft/world/entity/Entity.getX.()D", "getX.()D");
const GET_Y: Virtual = ("net/minecraft/world/entity/Entity.getY.()D", "getY.()D");
const GET_Z: Virtual = ("net/minecraft/world/entity/Entity.getZ.()D", "getZ.()D");
const GET_EYE_Y: Virtual = ("net/minecraft/world/entity/Entity.getEyeY.()D", "getEyeY.()D");
const GET_X_ROT: Virtual = ("net/minecraft/world/entity/Entity.getXRot.()F", "getXRot.()F");
const GET_Y_ROT: Virtual = ("net/minecraft/world/entity/Entity.getYRot.()F", "getYRot.()F");
const SET_ROT: Virtual = ("net/minecraft/world/entity/Entity.setRot.(FF)V", "setRot.(FF)V");
const POSITION: Virtual = ("net/minecraft/world/entity/Entity.position.()Lnet/minecraft/world/phys/Vec3;", "position.()Lnet/minecraft/world/phys/Vec3;");
const EYE_POSITION: Virtual = ("net/minecraft/world/entity/Entity.getEyePosition.()Lnet/minecraft/world/phys/Vec3;", "getEyePosition.()Lnet/minecraft/world/phys/Vec3;");
const VIEW_VECTOR: Virtual = ("net/minecraft/world/entity/Entity.getViewVector.(F)Lnet/minecraft/world/phys/Vec3;", "getViewVector.(F)Lnet/minecraft/world/phys/Vec3;");
const DELTA: Virtual = ("net/minecraft/world/entity/Entity.getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;", "getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;");
const SET_DELTA: Virtual = ("net/minecraft/world/entity/Entity.setDeltaMovement.(Lnet/minecraft/world/phys/Vec3;)V", "setDeltaMovement.(Lnet/minecraft/world/phys/Vec3;)V");
const SET_POS: Virtual = ("net/minecraft/world/entity/Entity.setPos.(DDD)V", "setPos.(DDD)V");
const MOVE_TO: Virtual = ("net/minecraft/world/entity/Entity.moveTo.(DDDFF)V", "moveTo.(DDDFF)V");
const BOUNDING_BOX: Virtual = ("net/minecraft/world/entity/Entity.getBoundingBox.()Lnet/minecraft/world/phys/AABB;", "getBoundingBox.()Lnet/minecraft/world/phys/AABB;");
const LEVEL: Virtual = ("net/minecraft/world/entity/Entity.level.()Lnet/minecraft/world/level/Level;", "level.()Lnet/minecraft/world/level/Level;");
const SET_OWNER: Virtual = ("net/minecraft/world/entity/projectile/Projectile.setOwner.(Lnet/minecraft/world/entity/Entity;)V", "setOwner.(Lnet/minecraft/world/entity/Entity;)V");
const GET_ENTITIES: &str = "net/minecraft/world/level/Level.getEntities.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/phys/AABB;Ljava/util/function/Predicate;)Ljava/util/List;";
const BY_UUID: &str = "net/minecraft/server/level/ServerLevel.getEntity.(Ljava/util/UUID;)Lnet/minecraft/world/entity/Entity;";

/// `ProjectileUtil`'s margin around an entity's box (`DEFAULT_ENTITY_HIT_RESULT_MARGIN`, a float).
const MARGIN: f32 = 0.3;

/// Whether `class` is vanilla's `Projectile` or a class below it (`ThrowableItemProjectile`,
/// `AbstractArrow`...), by the table of 1.21.1's entity classes.
pub(crate) fn is_projectile_class(class: &str) -> bool {
    crate::vanilla_entities::is_a(class, PROJECTILE)
}

/// The entity classes `Entity`'s methods below are called through: vanilla's entities and players.
fn is_entity_class(class: &str) -> bool {
    crate::vanilla_entities::is_a(class, ENTITY) || crate::api_common::is_player(class)
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, UTIL | ENTITY_HIT | HIT_RESULT)
        || is_projectile_class(owner)
        || (matches!(name, "canBeHitByProjectile" | "getPickRadius" | "isPassengerOfSameVehicle" | "canRiderInteract" | "deflection") && is_entity_class(owner))
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (DEFLECTION, "NONE" | "REVERSE" | "AIM_DEFLECT" | "MOMENTUM_DEFLECT") => Some(deflection(rt, field)),
        _ => None,
    }
}

/// One of `ProjectileDeflection`'s constants, the same object each time (mods compare them).
fn deflection(rt: &mut Rt, name: &str) -> V {
    let name = name.to_owned();
    rt.intern(format!("compat:deflection/{name}"), move |rt| {
        let d = rt.new_object(DEFLECTION);
        let n = rt.string(&name);
        rt.put_field(d, "$name", n);
        d
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // `new EntityHitResult(entity[, location])`: where the entity stands, unless told.
        (ENTITY_HIT, "<init>") => {
            let location = if desc.contains("Vec3") { a(2) } else { api::virtual_call(rt, POSITION, &[a(1)]) };
            rt.put_field(a(0), "$entity", a(1));
            rt.put_field(a(0), "$loc", location);
            V::Void
        }
        (ENTITY_HIT, "getEntity") => rt.get_field(a(0), "$entity"),
        (ENTITY_HIT, "getLocation") => rt.get_field(a(0), "$loc"),
        (ENTITY_HIT | HIT_RESULT, "getType") if rt.class_of(a(0)) == ENTITY_HIT => api::api_enum(rt, HIT_TYPE, "ENTITY", 2),
        // `HitResult.distanceTo(entity)`: the square of the distance from the hit to the entity's feet.
        (ENTITY_HIT | HIT_RESULT, "distanceTo") => {
            let at = xyz(rt, rt.get_field(a(0), "$loc"));
            let p = place(rt, a(1));
            V::D((0..3).map(|i| (at[i] - p[i]).powi(2)).sum())
        }
        (UTIL, _) => return util_call(rt, name, desc, args),
        // Vanilla's answers for any entity (a mod class's own come first, through its dispatch).
        (_, "canBeHitByProjectile") => V::I(i32::from(can_be_hit(rt, a(0)))),
        (_, "getPickRadius") => V::F(0.0),
        (_, "isPassengerOfSameVehicle") => V::I(i32::from(same_vehicle(a(0), a(1)))),
        (_, "canRiderInteract") => V::I(0),
        (_, "deflection") => deflection(rt, "NONE"),
        // A mod's projectile being made: the entity, then what its vanilla class's constructor sets.
        (o, "<init>") if is_projectile_class(o) && rt.supers.contains_key(rt.class_of(a(0))) => return construct(rt, o, desc, args),
        (o, _) if is_projectile_class(o) && crate::mod_entities::is_mod_entity(rt, a(0)) => return projectile_call(rt, name, args),
        _ => return None,
    })
}

/// `Projectile`'s own methods, for a mod's projectile.
fn projectile_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    Some(match name {
        "setOwner" => {
            set_owner(rt, e, a(1));
            V::Void
        }
        "getOwner" => owner_of(rt, e),
        "ownedBy" => {
            let uuid = api::virtual_call(rt, GET_UUID, &[a(1)]);
            let own = rt.get_field(e, "$ownerUUID");
            V::I(i32::from(own.r() != 0 && crate::tags::uuid_bits(rt, own) == crate::tags::uuid_bits(rt, uuid)))
        }
        "getEffectSource" => match owner_of(rt, e) {
            o if o.r() != 0 => o,
            _ => e,
        },
        "onHit" => {
            on_hit(rt, e, a(1));
            V::Void
        }
        // Vanilla's own do nothing for an entity; for a block, it asks the block (not modelled).
        "onHitEntity" | "onHitBlock" | "onDeflection" => V::Void,
        "hitTargetOrDeflectSelf" => {
            api::virtual_call(rt, ON_HIT, &[e, a(1)]);
            deflection(rt, "NONE")
        }
        "canHitEntity" => V::I(i32::from(can_hit(rt, e, a(1)))),
        // Vanilla's projectiles' own synched data (an item's stack, an arrow's flags) isn't kept: a
        // mod's comes after the entity's on both sides alike.
        "defineSynchedData" => V::Void,
        _ => return None,
    })
}

/// The constructors of vanilla's projectile classes, by their arguments (the same in each class
/// that has them): `(type, level)`; `(type, x, y, z, level)`, there; `(type, shooter, level)`, at
/// the shooter's eyes less a tenth, shot by it (a thrown one's); `(type, x, y, z, movement, level)`
/// and `(type, shooter, movement, level)`, moving `movement` scaled to its acceleration (a hurting
/// one's: a fireball's).
fn construct(rt: &mut Rt, owner: &str, desc: &str, args: &[V]) -> Option<V> {
    const TYPE_LEVEL: &str = "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
    const AT: &str = "(Lnet/minecraft/world/entity/EntityType;DDDLnet/minecraft/world/level/Level;)V";
    const BY: &str = "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/level/Level;)V";
    const AT_MOVING: &str = "(Lnet/minecraft/world/entity/EntityType;DDDLnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/level/Level;)V";
    const BY_MOVING: &str =
        "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/level/Level;)V";
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (e, ty) = (a(0), a(1));
    let level = match desc {
        TYPE_LEVEL => a(2),
        AT => a(5),
        BY => a(3),
        AT_MOVING => a(6),
        BY_MOVING => a(4),
        _ => return None,
    };
    crate::mod_entities::construct(rt, e, ty, level);
    rt.put_field(e, "$leftOwner", V::I(0));
    let hurting = crate::vanilla_entities::is_a(owner, HURTING);
    if hurting {
        rt.put_field(e, "accelerationPower", V::D(0.1));
    }
    match desc {
        AT => {
            api::virtual_call(rt, SET_POS, &[e, a(2), a(3), a(4)]);
        }
        BY => {
            let shooter = a(2);
            let (x, eye, z) = (api::virtual_call(rt, GET_X, &[shooter]), api::virtual_call(rt, GET_EYE_Y, &[shooter]), api::virtual_call(rt, GET_Z, &[shooter]));
            api::virtual_call(rt, SET_POS, &[e, x, V::D(eye.d() - f64::from(0.1f32)), z]);
            api::virtual_call(rt, SET_OWNER, &[e, shooter]);
        }
        AT_MOVING => moving(rt, e, [a(2), a(3), a(4)], a(5)),
        BY_MOVING => {
            let shooter = a(2);
            let at = [GET_X, GET_Y, GET_Z].map(|k| api::virtual_call(rt, k, &[shooter]));
            moving(rt, e, at, a(3));
            api::virtual_call(rt, SET_OWNER, &[e, shooter]);
            let (y_rot, x_rot) = (api::virtual_call(rt, GET_Y_ROT, &[shooter]), api::virtual_call(rt, GET_X_ROT, &[shooter]));
            api::virtual_call(rt, SET_ROT, &[e, y_rot, x_rot]);
        }
        _ => {}
    }
    Some(V::Void)
}

/// `AbstractHurtingProjectile(type, x, y, z, movement, level)` past `this(type, level)`: there
/// (`moveTo` with its own rotation), moving along `movement` at its acceleration
/// (`assignDirectionalMovement`: `movement.normalize().scale(accelerationPower)`).
fn moving(rt: &mut Rt, e: V, at: [V; 3], movement: V) {
    let (y_rot, x_rot) = (api::virtual_call(rt, GET_Y_ROT, &[e]), api::virtual_call(rt, GET_X_ROT, &[e]));
    api::virtual_call(rt, MOVE_TO, &[e, at[0], at[1], at[2], y_rot, x_rot]);
    let m = xyz(rt, movement);
    let length = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    // `Vec3.normalize`: zero below a float's hundred-thousandth.
    let unit = if length < f64::from(1.0e-5f32) { [0.0; 3] } else { m.map(|v| v / length) };
    let power = rt.get_field(e, "accelerationPower").d();
    let v = vec3(rt, unit.map(|u| u * power));
    api::virtual_call(rt, SET_DELTA, &[e, v]);
}

/// `Projectile.setOwner`: nothing for null.
fn set_owner(rt: &mut Rt, e: V, owner: V) {
    if owner.r() != 0 {
        let uuid = api::virtual_call(rt, GET_UUID, &[owner]);
        rt.put_field(e, "$ownerUUID", uuid);
        rt.put_field(e, "$owner", owner);
    }
}

/// `Projectile.getOwner`: the entity it was given while that is in the level, else on a server the
/// level's entity of its UUID (an owner the save named), else none.
fn owner_of(rt: &mut Rt, e: V) -> V {
    let cached = rt.get_field(e, "$owner");
    if cached.r() != 0 && !removed(rt, cached) {
        return cached;
    }
    let uuid = rt.get_field(e, "$ownerUUID");
    if uuid.r() != 0 && rt.host().is_some_and(|h| h.is_server()) {
        let level = rt.get_field(e, "$level");
        let found = api::call(rt, BY_UUID, &[level, uuid]);
        rt.put_field(e, "$owner", found);
        return found;
    }
    V::NULL
}

/// Whether entity `e` has left the level (a player's object stands for them while they play).
fn removed(rt: &mut Rt, e: V) -> bool {
    !crate::api_common::is_player_class(rt.class_of(e)) && api::virtual_call(rt, IS_REMOVED, &[e]).i() != 0
}

/// `Entity.canBeHitByProjectile`: alive and pickable (a living entity is while in the level, a mob
/// or a boat the game has always is), and for a player, not a spectator.
fn can_be_hit(rt: &mut Rt, e: V) -> bool {
    if e.r() == 0 {
        return false;
    }
    if crate::api_common::is_player_class(rt.class_of(e)) {
        return api::virtual_call(rt, IS_SPECTATOR, &[e]).i() == 0;
    }
    api::virtual_call(rt, IS_ALIVE, &[e]).i() != 0 && api::virtual_call(rt, IS_PICKABLE, &[e]).i() != 0
}

/// `Entity.isPassengerOfSameVehicle`: the same root vehicle. Nothing rides here (the game tells no
/// vehicles, a mod entity carries no passengers), so each entity is its own.
fn same_vehicle(a: V, b: V) -> bool {
    a == b
}

/// `Projectile.canHitEntity`: what can be hit (`canBeHitByProjectile`, the target's own answer
/// first), but not the projectile's owner until it has left it.
fn can_hit(rt: &mut Rt, e: V, target: V) -> bool {
    if target.r() == 0 || api::virtual_call(rt, CAN_BE_HIT, &[target]).i() == 0 {
        return false;
    }
    let owner = owner_of(rt, e);
    owner.r() == 0 || rt.get_field(e, "$leftOwner").i() != 0 || !same_vehicle(owner, target)
}

/// `Projectile.onHit`: an entity hit to the projectile's `onHitEntity`, a block hit to its
/// `onHitBlock`, a miss to neither.
fn on_hit(rt: &mut Rt, e: V, hit: V) {
    if hit.r() == 0 {
        return;
    }
    let kind = api::virtual_call(rt, HIT_GET_TYPE, &[hit]);
    match rt.get_field(kind, "$ordinal").i() {
        2 => {
            api::virtual_call(rt, ON_HIT_ENTITY, &[e, hit]);
        }
        1 => {
            api::virtual_call(rt, ON_HIT_BLOCK, &[e, hit]);
        }
        _ => {}
    }
}

/// `ProjectileUtil`'s searches.
fn util_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        // `(level, projectile, from, to, box, filter[, inflation])`
        "getEntityHitResult" if desc.starts_with("(Lnet/minecraft/world/level/Level;") => {
            let inflation = if desc.contains("Predicate;F)") { a(6).f() } else { MARGIN };
            let (from, to) = (xyz(rt, a(2)), xyz(rt, a(3)));
            nearest_hit(rt, a(0), a(1), from, to, a(4), a(5), inflation)
        }
        // `(shooter, from, to, box, filter, maxDistanceSq)`
        "getEntityHitResult" => {
            let (from, to) = (xyz(rt, a(1)), xyz(rt, a(2)));
            shooter_hit(rt, a(0), from, to, a(3), a(4), a(5).d())
        }
        // `(projectile, filter[, clip])`: along its motion from where it is.
        "getHitResultOnMoveVector" => {
            let e = a(0);
            let clip = if desc.contains("ClipContext$Block") { a(2) } else { clip_block(rt) };
            let (from, motion) = (api::virtual_call(rt, POSITION, &[e]), api::virtual_call(rt, DELTA, &[e]));
            let (from, motion) = (xyz(rt, from), xyz(rt, motion));
            path_hit(rt, e, a(1), from, motion, MARGIN, clip)
        }
        // `(projectile, filter, scale)`: from its eyes along where it looks, `scale` far, no margin.
        "getHitResultOnViewVector" => {
            let e = a(0);
            let view = api::virtual_call(rt, VIEW_VECTOR, &[e, V::F(0.0)]);
            let scale = a(2).d();
            let motion = xyz(rt, view).map(|v| v * scale);
            let from = api::virtual_call(rt, EYE_POSITION, &[e]);
            let from = xyz(rt, from);
            let clip = clip_block(rt);
            path_hit(rt, e, a(1), from, motion, 0.0, clip)
        }
        _ => return None,
    })
}

/// `ClipContext.Block.COLLIDER`.
fn clip_block(rt: &mut Rt) -> V {
    api::api_enum(rt, "net/minecraft/world/level/ClipContext$Block", "COLLIDER", 0)
}

/// `ProjectileUtil.getHitResult`: the block the path from `from` along `motion` meets first (the
/// level's `clip` with `clip`'s shapes, no fluids), else where the path ends; then the nearest
/// entity the path to there crosses, searched in the projectile's box stretched along the motion and
/// grown by a block, which wins over the block.
fn path_hit(rt: &mut Rt, e: V, filter: V, from: [f64; 3], motion: [f64; 3], margin: f32, clip: V) -> V {
    let level = api::virtual_call(rt, LEVEL, &[e]);
    let mut to = [from[0] + motion[0], from[1] + motion[1], from[2] + motion[2]];
    // `level.clip(new ClipContext(from, to, clip, Fluid.NONE, projectile))`.
    let block = crate::shapes::clip_through(rt, level, from, to, rt.get_field(clip, "$ordinal").i(), 0);
    if block.r() != 0 && rt.get_field(block, "$miss").i() == 0 {
        to = xyz(rt, rt.get_field(block, "$loc"));
    }
    let bb = api::virtual_call(rt, BOUNDING_BOX, &[e]);
    let mut area = crate::shapes::aabb_of(rt, bb);
    // `expandTowards(motion).inflate(1.0)`.
    for i in 0..3 {
        if motion[i] < 0.0 {
            area[i] += motion[i];
        } else if motion[i] > 0.0 {
            area[i + 3] += motion[i];
        }
        area[i] -= 1.0;
        area[i + 3] += 1.0;
    }
    let area = crate::shapes::aabb(rt, area);
    match nearest_hit(rt, level, e, from, to, area, filter, margin) {
        hit if hit.r() != 0 => hit,
        _ => block,
    }
}

/// `ProjectileUtil.getEntityHitResult(level, projectile, from, to, box, filter, inflation)`: of the
/// level's entities in `area` that pass `filter` (the projectile aside), the one whose box, grown by
/// `inflation`, the segment enters nearest `from`; a hit at that entity's feet. Null for none.
#[allow(clippy::too_many_arguments)]
fn nearest_hit(rt: &mut Rt, level: V, projectile: V, from: [f64; 3], to: [f64; 3], area: V, filter: V, inflation: f32) -> V {
    let grow = f64::from(inflation);
    let mut best: Option<(f64, V)> = None;
    for target in entities(rt, level, projectile, area, filter) {
        let bb = api::virtual_call(rt, BOUNDING_BOX, &[target]);
        let b = grown(crate::shapes::aabb_of(rt, bb), grow);
        if let Some(at) = crate::shapes::clip(b, from, to) {
            let d = distance_sq(from, at);
            if best.is_none_or(|(nearest, _)| d < nearest) {
                best = Some((d, target));
            }
        }
    }
    match best {
        Some((_, target)) => {
            let at = api::virtual_call(rt, POSITION, &[target]);
            entity_hit(rt, target, at)
        }
        None => V::NULL,
    }
}

/// `ProjectileUtil.getEntityHitResult(shooter, from, to, box, filter, maxDistanceSq)`: as
/// [`nearest_hit`], each box grown by its entity's pick radius and the hit where the segment enters
/// it, nearer than `max_sq`; an entity whose box holds `from` is hit there at once (a later one
/// entered nearer still wins). Nothing rides, so the shooter's own vehicle needs no exception.
fn shooter_hit(rt: &mut Rt, shooter: V, from: [f64; 3], to: [f64; 3], area: V, filter: V, max_sq: f64) -> V {
    let level = api::virtual_call(rt, LEVEL, &[shooter]);
    let mut nearest = max_sq;
    let mut found: Option<(V, [f64; 3])> = None;
    for target in entities(rt, level, shooter, area, filter) {
        let bb = api::virtual_call(rt, BOUNDING_BOX, &[target]);
        let radius = api::virtual_call(rt, PICK_RADIUS, &[target]).f();
        let b = grown(crate::shapes::aabb_of(rt, bb), f64::from(radius));
        let at = crate::shapes::clip(b, from, to);
        if (0..3).all(|i| b[i] <= from[i] && from[i] < b[i + 3]) {
            if nearest >= 0.0 {
                found = Some((target, at.unwrap_or(from)));
                nearest = 0.0;
            }
        } else if let Some(at) = at {
            let d = distance_sq(from, at);
            if d < nearest || nearest == 0.0 {
                found = Some((target, at));
                nearest = d;
            }
        }
    }
    match found {
        Some((target, at)) => {
            let at = vec3(rt, at);
            entity_hit(rt, target, at)
        }
        None => V::NULL,
    }
}

/// `level.getEntities(except, area, filter)`.
fn entities(rt: &mut Rt, level: V, except: V, area: V, filter: V) -> Vec<V> {
    let list = api::call(rt, GET_ENTITIES, &[level, except, area, filter]);
    jutil::elements(rt, list)
}

fn entity_hit(rt: &mut Rt, entity: V, location: V) -> V {
    let h = rt.new_object(ENTITY_HIT);
    rt.put_field(h, "$entity", entity);
    rt.put_field(h, "$loc", location);
    h
}

fn grown(b: crate::shapes::Box3, by: f64) -> crate::shapes::Box3 {
    [b[0] - by, b[1] - by, b[2] - by, b[3] + by, b[4] + by, b[5] + by]
}

fn distance_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

/// Where entity `e` stands: its own `getX/Y/Z`.
fn place(rt: &mut Rt, e: V) -> [f64; 3] {
    [GET_X, GET_Y, GET_Z].map(|k| api::virtual_call(rt, k, &[e]).d())
}

fn xyz(rt: &Rt, v: V) -> [f64; 3] {
    ["x", "y", "z"].map(|k| rt.get_field(v, k).d())
}

fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
    crate::api_common::vec3(rt, p[0], p[1], p[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityView, MemoryHost};

    const BOLT: &str = "test/Bolt";
    const THROWN: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile";
    const AT: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.<init>.(Lnet/minecraft/world/entity/EntityType;DDDLnet/minecraft/world/level/Level;)V";
    const BY: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/level/Level;)V";
    const MOVE_TO: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.setPos.(DDD)V";
    const MOTION: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.setDeltaMovement.(DDD)V";
    const GET_POS: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.position.()Lnet/minecraft/world/phys/Vec3;";
    const ON_MOVE: &str =
        "net/minecraft/world/entity/projectile/ProjectileUtil.getHitResultOnMoveVector.(Lnet/minecraft/world/entity/Entity;Ljava/util/function/Predicate;)Lnet/minecraft/world/phys/HitResult;";
    const BY_SHOOTER: &str = "net/minecraft/world/entity/projectile/ProjectileUtil.getEntityHitResult.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/phys/AABB;Ljava/util/function/Predicate;D)Lnet/minecraft/world/phys/EntityHitResult;";
    const HIT_OR_DEFLECT: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.hitTargetOrDeflectSelf.(Lnet/minecraft/world/phys/HitResult;)Lnet/minecraft/world/entity/projectile/ProjectileDeflection;";
    const CAN_HIT: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.canHitEntity.(Lnet/minecraft/world/entity/Entity;)Z";
    const GET_OWNER: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.getOwner.()Lnet/minecraft/world/entity/Entity;";
    const GET_TYPE: &str = "net/minecraft/world/phys/HitResult.getType.()Lnet/minecraft/world/phys/HitResult$Type;";
    const GET_LOCATION: &str = "net/minecraft/world/phys/HitResult.getLocation.()Lnet/minecraft/world/phys/Vec3;";
    const GET_ENTITY: &str = "net/minecraft/world/phys/EntityHitResult.getEntity.()Lnet/minecraft/world/entity/Entity;";
    const DISTANCE_TO: &str = "net/minecraft/world/phys/HitResult.distanceTo.(Lnet/minecraft/world/entity/Entity;)D";
    const BLOCK_POS_OF: &str = "net/minecraft/world/phys/BlockHitResult.getBlockPos.()Lnet/minecraft/core/BlockPos;";

    /// The test's projectile has its own `onHitEntity` and `onHitBlock`, which note what they get.
    fn overrides(class: &str, short: &str) -> bool {
        class == BOLT && (short == ON_HIT_ENTITY.1 || short == ON_HIT_BLOCK.1)
    }

    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) == BOLT && short == ON_HIT_ENTITY.1 {
            let target = api::call(rt, GET_ENTITY, &[args[1]]);
            rt.statics.insert("t.entity", target);
            return V::Void;
        }
        if rt.class_of(args[0]) == BOLT && short == ON_HIT_BLOCK.1 {
            let pos = api::call(rt, BLOCK_POS_OF, &[args[1]]);
            rt.statics.insert("t.block", pos);
            return V::Void;
        }
        api::call(rt, key, args)
    }

    fn kind(rt: &mut Rt, hit: V) -> String {
        let t = api::call(rt, GET_TYPE, &[hit]);
        rt.str_of(rt.get_field(t, "$name")).unwrap_or("").to_owned()
    }

    /// A mod's thrown projectile (its class below `ThrowableItemProjectile`, made by that class's
    /// constructors) finds what it hits as vanilla's `ProjectileUtil` does: along its motion, the
    /// nearest entity whose box grown by 0.3 it enters, at that entity's feet; else the block its path
    /// meets; else a miss where the path ends. `hitTargetOrDeflectSelf` hands an entity hit to the
    /// projectile's own `onHitEntity`, a block hit to its `onHitBlock`. One thrown by a cow starts at
    /// the cow's eyes less a tenth, is the cow's, and can't hit it until it has left it, though it
    /// can hit a pig.
    #[test]
    fn a_mod_projectile_hits_the_nearest_entity_on_its_path_else_the_block() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(BOLT, THROWN);
        let ty = rt.new_object("net/minecraft/world/entity/EntityType");
        rt.put_field(ty, "$width", V::F(0.25));
        rt.put_field(ty, "$height", V::F(0.25));
        let mut host = MemoryHost::default();
        host.entities.push(EntityView { id: 7, kind: "minecraft:cow".into(), pos: [0.5, 64.0, 4.5], yaw: 0.0, health: Some((10.0, 10.0)) });
        host.entities.push(EntityView { id: 8, kind: "minecraft:pig".into(), pos: [3.5, 64.0, 4.5], yaw: 0.0, health: Some((10.0, 10.0)) });
        host.blocks.insert([0, 64, 9], "minecraft:stone".into());
        host.collisions.insert([0, 64, 9], vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]);
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let bolt = rt.new_object(BOLT);
            api::call(rt, AT, &[bolt, ty, V::D(0.5), V::D(64.5), V::D(0.5), level]);
            assert!(crate::mod_entities::is_mod_entity(rt, bolt));
            api::call(rt, MOTION, &[bolt, V::D(0.0), V::D(0.0), V::D(1.0)]);

            // Short of the cow (its box grown by 0.3 starts at z 3.75): a miss where the path ends.
            let miss = api::call(rt, ON_MOVE, &[bolt, V::NULL]);
            assert_eq!(kind(rt, miss), "MISS");
            let at = api::call(rt, GET_LOCATION, &[miss]);
            assert_eq!(xyz(rt, at), [0.5, 64.5, 1.5]);

            // Into the cow: hit at its feet, which the projectile's own `onHitEntity` gets.
            api::call(rt, MOVE_TO, &[bolt, V::D(0.5), V::D(64.5), V::D(3.0)]);
            let hit = api::call(rt, ON_MOVE, &[bolt, V::NULL]);
            assert_eq!(kind(rt, hit), "ENTITY");
            assert!(rt.instance_of(hit, HIT_RESULT));
            let cow = api::call(rt, GET_ENTITY, &[hit]);
            assert_eq!(api::call(rt, "net/minecraft/world/entity/Entity.getId.()I", &[cow]), V::I(7));
            let at = api::call(rt, GET_LOCATION, &[hit]);
            assert_eq!(xyz(rt, at), [0.5, 64.0, 4.5]);
            assert_eq!(api::call(rt, DISTANCE_TO, &[hit, bolt]), V::D(0.25 + 2.25));
            let none = api::get_static(rt, "net/minecraft/world/entity/projectile/ProjectileDeflection.NONE");
            assert_eq!(api::call(rt, HIT_OR_DEFLECT, &[bolt, hit]), none);
            assert_eq!(rt.statics.get("t.entity"), Some(&cow));

            // Past the cow, into the stone: the block, where the path meets its north face.
            api::call(rt, MOVE_TO, &[bolt, V::D(0.5), V::D(64.5), V::D(8.25)]);
            let block = api::call(rt, ON_MOVE, &[bolt, V::NULL]);
            assert_eq!(kind(rt, block), "BLOCK");
            let at = api::call(rt, GET_LOCATION, &[block]);
            assert_eq!(xyz(rt, at), [0.5, 64.5, 9.0]);
            api::call(rt, HIT_OR_DEFLECT, &[bolt, block]);
            let pos = rt.statics.get("t.block").copied().unwrap_or_default();
            assert_eq!(api::pos_of(rt, pos), [0, 64, 9]);

            // From inside the cow's box, the shooter's search hits it where it starts.
            let (from, to) = (vec3(rt, [0.5, 64.5, 4.5]), vec3(rt, [0.5, 64.5, 10.0]));
            let area = crate::shapes::aabb(rt, [-1.0, 63.0, 3.0, 2.0, 66.0, 11.0]);
            let inside = api::call(rt, BY_SHOOTER, &[bolt, from, to, area, V::NULL, V::D(100.0)]);
            assert_eq!(api::call(rt, GET_ENTITY, &[inside]), cow);
            let at = api::call(rt, GET_LOCATION, &[inside]);
            assert_eq!(xyz(rt, at), [0.5, 64.5, 4.5]);

            // Thrown by the cow.
            let thrown = rt.new_object(BOLT);
            api::call(rt, BY, &[thrown, ty, cow, level]);
            let eye = api::call(rt, "net/minecraft/world/entity/Entity.getEyeY.()D", &[cow]).d();
            let p = api::call(rt, GET_POS, &[thrown]);
            assert_eq!(xyz(rt, p), [0.5, eye - f64::from(0.1f32), 4.5]);
            assert_eq!(api::call(rt, GET_OWNER, &[thrown]), cow);
            assert_eq!(api::call(rt, CAN_HIT, &[thrown, cow]), V::I(0), "not its thrower yet");
            let pig = crate::vanilla_entities::of(rt, 8, "minecraft:pig", Some([3.5, 64.0, 4.5]));
            assert_eq!(api::call(rt, CAN_HIT, &[thrown, pig]), V::I(1));
            rt.put_field(thrown, "$leftOwner", V::I(1));
            assert_eq!(api::call(rt, CAN_HIT, &[thrown, cow]), V::I(1), "once it has left it");
        });
    }
}
