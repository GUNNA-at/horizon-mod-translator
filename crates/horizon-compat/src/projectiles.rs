//! Vanilla's projectile classes for a mod's projectile: a mod entity whose class extends
//! `Projectile` or a class below it (Create's potato, a spell's bolt, a cannon's shell). How one is
//! made (`ThrowableProjectile(type, shooter, level)`...), who shot it (`setOwner`, `getOwner`), what
//! it hits: `EntityHitResult`, `ProjectileUtil`'s searches along a path (the blocks through the
//! level's `clip`, then the entities whose boxes the path crosses), and `onHit`, which hands an
//! entity hit to the projectile's `onHitEntity` and a block hit to its `onHitBlock`, the mod's own
//! where its class has them; and how it flies: `shoot`, and the ticks of `Projectile`,
//! `ThrowableProjectile` and `AbstractHurtingProjectile`, which move it along its motion through the
//! air or water. And the ammo a weapon shoots: what an entity finds for a mod's
//! `ProjectileWeaponItem` (`getProjectile`), in its hands, then in a player's inventory. As 1.21.1
//! does; game events (`PROJECTILE_SHOOT`, `PROJECTILE_LAND`), a deflecting target (a breeze), a
//! vanilla block's own answer to a hit (a bell, a target block), the blocks a projectile passes
//! through (`checkInsideBlocks`), an arrow's own tick and vanilla's bow and crossbow as weapons are
//! not modelled.
use crate::{Rt, V, api, jutil};

const ENTITY: &str = "net/minecraft/world/entity/Entity";
pub(crate) const PROJECTILE: &str = "net/minecraft/world/entity/projectile/Projectile";
const HURTING: &str = "net/minecraft/world/entity/projectile/AbstractHurtingProjectile";
const THROWABLE: &str = "net/minecraft/world/entity/projectile/ThrowableProjectile";
const THROWN_ITEM: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile";
const ARROW: &str = "net/minecraft/world/entity/projectile/AbstractArrow";
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
const HIT_OR_DEFLECT: Virtual = (
    "net/minecraft/world/entity/projectile/Projectile.hitTargetOrDeflectSelf.(Lnet/minecraft/world/phys/HitResult;)Lnet/minecraft/world/entity/projectile/ProjectileDeflection;",
    "hitTargetOrDeflectSelf.(Lnet/minecraft/world/phys/HitResult;)Lnet/minecraft/world/entity/projectile/ProjectileDeflection;",
);
const CAN_HIT_ENTITY: Virtual = ("net/minecraft/world/entity/projectile/Projectile.canHitEntity.(Lnet/minecraft/world/entity/Entity;)Z", "canHitEntity.(Lnet/minecraft/world/entity/Entity;)Z");
const MOVEMENT_TO_SHOOT: Virtual =
    ("net/minecraft/world/entity/projectile/Projectile.getMovementToShoot.(DDDFF)Lnet/minecraft/world/phys/Vec3;", "getMovementToShoot.(DDDFF)Lnet/minecraft/world/phys/Vec3;");
const SHOOT: Virtual = ("net/minecraft/world/entity/projectile/Projectile.shoot.(DDDFF)V", "shoot.(DDDFF)V");
const UPDATE_ROTATION: Virtual = ("net/minecraft/world/entity/projectile/Projectile.updateRotation.()V", "updateRotation.()V");
const SET_Y_ROT: Virtual = ("net/minecraft/world/entity/Entity.setYRot.(F)V", "setYRot.(F)V");
const SET_X_ROT: Virtual = ("net/minecraft/world/entity/Entity.setXRot.(F)V", "setXRot.(F)V");
const KNOWN_MOVEMENT: Virtual = ("net/minecraft/world/entity/Entity.getKnownMovement.()Lnet/minecraft/world/phys/Vec3;", "getKnownMovement.()Lnet/minecraft/world/phys/Vec3;");
const ON_GROUND: Virtual = ("net/minecraft/world/entity/Entity.onGround.()Z", "onGround.()Z");
const IS_IN_WATER: Virtual = ("net/minecraft/world/entity/Entity.isInWater.()Z", "isInWater.()Z");
const BASE_TICK: Virtual = ("net/minecraft/world/entity/Entity.baseTick.()V", "baseTick.()V");
const DISCARD: Virtual = ("net/minecraft/world/entity/Entity.discard.()V", "discard.()V");
const BLOCK_POSITION: Virtual = ("net/minecraft/world/entity/Entity.blockPosition.()Lnet/minecraft/core/BlockPos;", "blockPosition.()Lnet/minecraft/core/BlockPos;");
const IGNITE: Virtual = ("net/minecraft/world/entity/Entity.igniteForSeconds.(F)V", "igniteForSeconds.(F)V");
const HAS_CHUNK_AT: &str = "net/minecraft/world/level/Level.hasChunkAt.(Lnet/minecraft/core/BlockPos;)Z";
const ADD_PARTICLE: &str = "net/minecraft/world/level/Level.addParticle.(Lnet/minecraft/core/particles/ParticleOptions;DDDDDD)V";
const NEXT_DOUBLE: &str = "net/minecraft/util/RandomSource.nextDouble.()D";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const GET_ENTITIES: &str = "net/minecraft/world/level/Level.getEntities.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/phys/AABB;Ljava/util/function/Predicate;)Ljava/util/List;";
const WEAPON: &str = "net/minecraft/world/item/ProjectileWeaponItem";
const MONSTER: &str = "net/minecraft/world/entity/monster/Monster";
/// `getProjectile(weapon)`'s descriptor, on any living entity.
const GET_PROJECTILE: &str = "(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/ItemStack;";
const HELD_AMMO: Virtual = (
    "net/minecraft/world/item/ProjectileWeaponItem.getSupportedHeldProjectiles.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/function/Predicate;",
    "getSupportedHeldProjectiles.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/function/Predicate;",
);
const HELD_AMMO_PLAIN: Virtual =
    ("net/minecraft/world/item/ProjectileWeaponItem.getSupportedHeldProjectiles.()Ljava/util/function/Predicate;", "getSupportedHeldProjectiles.()Ljava/util/function/Predicate;");
const ALL_AMMO: Virtual = (
    "net/minecraft/world/item/ProjectileWeaponItem.getAllSupportedProjectiles.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/function/Predicate;",
    "getAllSupportedProjectiles.(Lnet/minecraft/world/item/ItemStack;)Ljava/util/function/Predicate;",
);
const ALL_AMMO_PLAIN: Virtual =
    ("net/minecraft/world/item/ProjectileWeaponItem.getAllSupportedProjectiles.()Ljava/util/function/Predicate;", "getAllSupportedProjectiles.()Ljava/util/function/Predicate;");
const CREATIVE_AMMO: Virtual = (
    "net/minecraft/world/item/ProjectileWeaponItem.getDefaultCreativeAmmo.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/ItemStack;",
    "getDefaultCreativeAmmo.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/ItemStack;",
);
const IN_HAND: Virtual = (
    "net/minecraft/world/entity/LivingEntity.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;",
    "getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;",
);
const GET_ITEM: &str = "net/minecraft/world/item/ItemStack.getItem.()Lnet/minecraft/world/item/Item;";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const OR: &str = "java/util/function/Predicate.or.(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;";
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
    matches!(owner, UTIL | ENTITY_HIT | HIT_RESULT | WEAPON)
        || is_projectile_class(owner)
        || (matches!(name, "canBeHitByProjectile" | "getPickRadius" | "isPassengerOfSameVehicle" | "canRiderInteract" | "deflection" | "getProjectile") && is_entity_class(owner))
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
        // The ammo an entity shoots from a weapon, and a mod weapon's defaults.
        (_, "getProjectile") if desc == GET_PROJECTILE => ammo(rt, a(0), a(1)),
        (WEAPON, _) => return weapon_call(rt, name, desc, args),
        // Vanilla's answers for any entity (a mod class's own come first, through its dispatch).
        (_, "canBeHitByProjectile") => V::I(i32::from(can_be_hit(rt, a(0)))),
        (_, "getPickRadius") => V::F(0.0),
        (_, "isPassengerOfSameVehicle") => V::I(i32::from(same_vehicle(a(0), a(1)))),
        (_, "canRiderInteract") => V::I(0),
        (_, "deflection") => deflection(rt, "NONE"),
        // A mod's projectile being made: the entity, then what its vanilla class's constructor sets.
        (o, "<init>") if is_projectile_class(o) && rt.supers.contains_key(rt.class_of(a(0))) => return construct(rt, o, desc, args),
        (o, "lerpRotation") if is_projectile_class(o) && desc == "(FF)F" => V::F(lerp_rotation(a(0).f(), a(1).f())),
        (o, _) if is_projectile_class(o) && crate::mod_entities::is_mod_entity(rt, a(0)) => return projectile_call(rt, o, name, desc, args),
        _ => return None,
    })
}

/// `Projectile`'s own methods and its subclasses' (by the class the call names), for a mod's
/// projectile.
fn projectile_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    let is = |class: &str| crate::vanilla_entities::is_a(owner, class);
    Some(match name {
        "tick" => {
            tick(rt, owner, e);
            V::Void
        }
        "shoot" if desc == "(DDDFF)V" => {
            shoot(rt, e, [a(1).d(), a(2).d(), a(3).d()], a(4).f(), a(5).f());
            V::Void
        }
        "getMovementToShoot" => movement_to_shoot(rt, e, [a(1).d(), a(2).d(), a(3).d()], a(4).f(), a(5).f()),
        "shootFromRotation" => {
            shoot_from_rotation(rt, e, a(1), [a(2).f(), a(3).f(), a(4).f()], a(5).f(), a(6).f());
            V::Void
        }
        "updateRotation" => {
            update_rotation(rt, e);
            V::Void
        }
        // A class's gravity: a thrown one's 0.03, an arrow's 0.05 (`Entity`'s none, below).
        "getDefaultGravity" if is(THROWABLE) => V::D(0.03),
        "getDefaultGravity" if is(ARROW) => V::D(0.05),
        // A hurting projectile's: how it slows in the air and in water, that it burns, the smoke it
        // trails, the shapes its path meets.
        "getInertia" if is(HURTING) => V::F(0.95),
        "getLiquidInertia" if is(HURTING) => V::F(0.8),
        "shouldBurn" if is(HURTING) => V::I(1),
        "getTrailParticle" if is(HURTING) => api::get_static(rt, "net/minecraft/core/particles/ParticleTypes.SMOKE"),
        "getClipType" if is(HURTING) => clip_block(rt),
        // A thrown item's stack: one of what it is given, else one of its class's default item.
        "setItem" if is(THROWN_ITEM) => {
            let one = api::call(rt, "net/minecraft/world/item/ItemStack.copyWithCount.(I)Lnet/minecraft/world/item/ItemStack;", &[a(1), V::I(1)]);
            rt.put_field(e, "$item", one);
            V::Void
        }
        "getItem" if is(THROWN_ITEM) => match rt.get_field(e, "$item") {
            s if s.r() != 0 => s,
            _ => {
                let item = crate::mod_entities::on_base(rt, e, "getDefaultItem.()Lnet/minecraft/world/item/Item;", &[]);
                let s = rt.new_object(ITEM_STACK);
                api::call(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[s, item]);
                rt.put_field(e, "$item", s);
                s
            }
        },
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
        // Its saved data, as 1.21.1's classes write it: a projectile's owner (`Owner`, when it has
        // one), `LeftOwner` (only once it has), `HasBeenShot`; a hurting one's
        // `acceleration_power`; a thrown item's stack (`Item`). An arrow's isn't modelled.
        "addAdditionalSaveData" if !is(ARROW) => {
            save(rt, owner, e, a(1));
            V::Void
        }
        "readAdditionalSaveData" if !is(ARROW) => {
            load(rt, owner, e, a(1));
            V::Void
        }
        _ => return None,
    })
}

const GET_THROWN_ITEM: Virtual = ("net/minecraft/world/entity/projectile/ThrowableItemProjectile.getItem.()Lnet/minecraft/world/item/ItemStack;", "getItem.()Lnet/minecraft/world/item/ItemStack;");
const SET_ITEM: Virtual = ("net/minecraft/world/entity/projectile/ThrowableItemProjectile.setItem.(Lnet/minecraft/world/item/ItemStack;)V", "setItem.(Lnet/minecraft/world/item/ItemStack;)V");

fn registries_of(rt: &mut Rt, e: V) -> V {
    let level = rt.get_field(e, "$level");
    api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[level])
}

/// `addAdditionalSaveData` of vanilla's projectile class `owner` (and those above it) for `e`.
fn save(rt: &mut Rt, owner: &str, e: V, tag: V) {
    let own = rt.get_field(e, "$ownerUUID");
    if own.r() != 0 {
        let k = rt.string("Owner");
        api::call(rt, "net/minecraft/nbt/CompoundTag.putUUID.(Ljava/lang/String;Ljava/util/UUID;)V", &[tag, k, own]);
    }
    if rt.get_field(e, "$leftOwner").i() != 0 {
        let k = rt.string("LeftOwner");
        api::call(rt, "net/minecraft/nbt/CompoundTag.putBoolean.(Ljava/lang/String;Z)V", &[tag, k, V::I(1)]);
    }
    let (k, shot) = (rt.string("HasBeenShot"), V::I(rt.get_field(e, "$hasBeenShot").i()));
    api::call(rt, "net/minecraft/nbt/CompoundTag.putBoolean.(Ljava/lang/String;Z)V", &[tag, k, shot]);
    if crate::vanilla_entities::is_a(owner, HURTING) {
        let (k, power) = (rt.string("acceleration_power"), V::D(rt.get_field(e, "accelerationPower").d()));
        api::call(rt, "net/minecraft/nbt/CompoundTag.putDouble.(Ljava/lang/String;D)V", &[tag, k, power]);
    }
    if crate::vanilla_entities::is_a(owner, THROWN_ITEM) {
        let item = api::virtual_call(rt, GET_THROWN_ITEM, &[e]);
        if rt.thrown.is_some() {
            return;
        }
        let registries = registries_of(rt, e);
        let saved = api::call(rt, "net/minecraft/world/item/ItemStack.save.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/Tag;", &[item, registries]);
        if rt.thrown.is_some() {
            return;
        }
        let k = rt.string("Item");
        api::call(rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[tag, k, saved]);
    }
}

/// `readAdditionalSaveData` of vanilla's projectile class `owner` (and those above it) for `e`:
/// [`save`]'s keys back (an owner named is found again by its UUID); a thrown item without a
/// readable `Item` holds its class's default item.
fn load(rt: &mut Rt, owner: &str, e: V, tag: V) {
    let k = rt.string("Owner");
    if api::call(rt, "net/minecraft/nbt/CompoundTag.hasUUID.(Ljava/lang/String;)Z", &[tag, k]).i() != 0 {
        let uuid = api::call(rt, "net/minecraft/nbt/CompoundTag.getUUID.(Ljava/lang/String;)Ljava/util/UUID;", &[tag, k]);
        rt.put_field(e, "$ownerUUID", uuid);
        rt.put_field(e, "$owner", V::NULL);
    }
    for (key, field) in [("LeftOwner", "$leftOwner"), ("HasBeenShot", "$hasBeenShot")] {
        let k = rt.string(key);
        let b = api::call(rt, "net/minecraft/nbt/CompoundTag.getBoolean.(Ljava/lang/String;)Z", &[tag, k]);
        rt.put_field(e, field, V::I(b.i()));
    }
    if crate::vanilla_entities::is_a(owner, HURTING) {
        let k = rt.string("acceleration_power");
        if api::call(rt, "net/minecraft/nbt/CompoundTag.contains.(Ljava/lang/String;I)Z", &[tag, k, V::I(6)]).i() != 0 {
            let power = api::call(rt, "net/minecraft/nbt/CompoundTag.getDouble.(Ljava/lang/String;)D", &[tag, k]);
            rt.put_field(e, "accelerationPower", V::D(power.d()));
        }
    }
    if crate::vanilla_entities::is_a(owner, THROWN_ITEM) {
        let k = rt.string("Item");
        let read = if api::call(rt, "net/minecraft/nbt/CompoundTag.contains.(Ljava/lang/String;I)Z", &[tag, k, V::I(10)]).i() != 0 {
            let registries = registries_of(rt, e);
            let saved = api::call(rt, "net/minecraft/nbt/CompoundTag.getCompound.(Ljava/lang/String;)Lnet/minecraft/nbt/CompoundTag;", &[tag, k]);
            let parsed = api::call(rt, "net/minecraft/world/item/ItemStack.parse.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/Tag;)Ljava/util/Optional;", &[registries, saved]);
            rt.get_field(parsed, "value")
        } else {
            V::NULL
        };
        let stack = if read.r() != 0 {
            read
        } else {
            let item = crate::mod_entities::on_base(rt, e, "getDefaultItem.()Lnet/minecraft/world/item/Item;", &[]);
            let s = rt.new_object(ITEM_STACK);
            api::call(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[s, item]);
            s
        };
        if rt.thrown.is_none() {
            api::virtual_call(rt, SET_ITEM, &[e, stack]);
        }
    }
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
    let unit = normalize(xyz(rt, movement));
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

/// A projectile's vanilla tick, by the class the call names (its class's `super.tick()`, or the
/// vanilla class it extends when it has no tick of its own): a hurting projectile's, a thrown one's,
/// else `Projectile`'s own (an arrow's isn't modelled: it is `Projectile`'s, and the arrow stays).
fn tick(rt: &mut Rt, owner: &str, e: V) {
    if crate::vanilla_entities::is_a(owner, HURTING) {
        hurting_tick(rt, e);
    } else if crate::vanilla_entities::is_a(owner, THROWABLE) {
        throwable_tick(rt, e);
    } else {
        projectile_tick(rt, e);
    }
}

/// `Projectile.tick`: shot (its game event isn't modelled), whether it has left its owner yet, then
/// `Entity.tick`'s base tick (virtual: a class's own `baseTick` first).
fn projectile_tick(rt: &mut Rt, e: V) {
    rt.put_field(e, "$hasBeenShot", V::I(1));
    if rt.get_field(e, "$leftOwner").i() == 0 {
        let left = left_owner(rt, e);
        rt.put_field(e, "$leftOwner", V::I(i32::from(left)));
    }
    api::virtual_call(rt, BASE_TICK, &[e]);
}

/// `Projectile.checkLeftOwner`: no owner, or none of the entities its box stretched along its
/// motion and grown by a block meets that can be picked (not a spectator) is its owner (nothing
/// rides here: each entity is its own vehicle).
fn left_owner(rt: &mut Rt, e: V) -> bool {
    let owner = owner_of(rt, e);
    if owner.r() == 0 {
        return true;
    }
    let motion = api::virtual_call(rt, DELTA, &[e]);
    let motion = xyz(rt, motion);
    let level = api::virtual_call(rt, LEVEL, &[e]);
    let area = swept_area(rt, e, motion);
    for target in entities(rt, level, e, area, Filter::Predicate(V::NULL)) {
        if same_vehicle(target, owner) && pickable(rt, target) {
            return false;
        }
    }
    true
}

/// `!entity.isSpectator() && entity.isPickable()`: a player is while in the level (a living
/// entity's `isPickable`).
fn pickable(rt: &mut Rt, e: V) -> bool {
    if api::virtual_call(rt, IS_SPECTATOR, &[e]).i() != 0 {
        return false;
    }
    crate::api_common::is_player_class(rt.class_of(e)) || api::virtual_call(rt, IS_PICKABLE, &[e]).i() != 0
}

/// `ThrowableProjectile.tick`: `Projectile`'s, what its path hits this tick, then on to the end of
/// that path, turned toward it (`updateRotation`), slowed by the air (0.99) or water (0.8, four
/// bubbles behind it), and pulled down by its gravity (`applyGravity`: a thrown one's 0.03).
fn throwable_tick(rt: &mut Rt, e: V) {
    projectile_tick(rt, e);
    let clip = clip_block(rt);
    hit_along_motion(rt, e, clip);
    let m = api::virtual_call(rt, DELTA, &[e]);
    let m = xyz(rt, m);
    let p = place(rt, e);
    let next = [p[0] + m[0], p[1] + m[1], p[2] + m[2]];
    api::virtual_call(rt, UPDATE_ROTATION, &[e]);
    let drag = if api::virtual_call(rt, IS_IN_WATER, &[e]).i() != 0 {
        bubbles(rt, e, next, m);
        0.8f32
    } else {
        0.99f32
    };
    let v = vec3(rt, m.map(|c| c * f64::from(drag)));
    api::virtual_call(rt, SET_DELTA, &[e, v]);
    crate::mod_entities::on_base(rt, e, "applyGravity.()V", &[]);
    api::virtual_call(rt, SET_POS, &[e, V::D(next[0]), V::D(next[1]), V::D(next[2])]);
}

/// `AbstractHurtingProjectile.tick` (a fireball's): on a server, gone where its chunk isn't loaded
/// (or its owner was removed); else `Projectile`'s tick, alight for a second (`shouldBurn`), what
/// its path hits (its `getClipType`'s shapes), then on to the end of that path, turned toward it
/// (`rotateTowardsMovement`, a fifth of the way), speeding up along its heading by its
/// `accelerationPower` and slowed by its inertia (`getInertia`, 0.95; in water `getLiquidInertia`,
/// 0.8, four bubbles behind it), its trail (`getTrailParticle`, smoke) half a block above.
fn hurting_tick(rt: &mut Rt, e: V) {
    if rt.host().is_none_or(|h| h.is_server()) {
        let owner = owner_of(rt, e);
        let level = api::virtual_call(rt, LEVEL, &[e]);
        let at = api::virtual_call(rt, BLOCK_POSITION, &[e]);
        let loaded = api::call(rt, HAS_CHUNK_AT, &[level, at]).i() != 0;
        if (owner.r() != 0 && removed(rt, owner)) || !loaded {
            api::virtual_call(rt, DISCARD, &[e]);
            return;
        }
    }
    projectile_tick(rt, e);
    if crate::mod_entities::on_base(rt, e, "shouldBurn.()Z", &[]).i() != 0 {
        api::virtual_call(rt, IGNITE, &[e, V::F(1.0)]);
    }
    let clip = crate::mod_entities::on_base(rt, e, "getClipType.()Lnet/minecraft/world/level/ClipContext$Block;", &[]);
    hit_along_motion(rt, e, clip);
    let m = api::virtual_call(rt, DELTA, &[e]);
    let m = xyz(rt, m);
    let p = place(rt, e);
    let next = [p[0] + m[0], p[1] + m[1], p[2] + m[2]];
    rotate_towards_movement(rt, e, 0.2);
    let inertia = if api::virtual_call(rt, IS_IN_WATER, &[e]).i() != 0 {
        bubbles(rt, e, next, m);
        crate::mod_entities::on_base(rt, e, "getLiquidInertia.()F", &[]).f()
    } else {
        crate::mod_entities::on_base(rt, e, "getInertia.()F", &[]).f()
    };
    let power = rt.get_field(e, "accelerationPower").d();
    let heading = normalize(m);
    let v = vec3(rt, [0, 1, 2].map(|i| (m[i] + heading[i] * power) * f64::from(inertia)));
    api::virtual_call(rt, SET_DELTA, &[e, v]);
    let trail = crate::mod_entities::on_base(rt, e, "getTrailParticle.()Lnet/minecraft/core/particles/ParticleOptions;", &[]);
    if trail.r() != 0 {
        add_particle(rt, e, trail, [next[0], next[1] + 0.5, next[2]], [0.0; 3]);
    }
    api::virtual_call(rt, SET_POS, &[e, V::D(next[0]), V::D(next[1]), V::D(next[2])]);
}

/// What projectile `e`'s path hits this tick (`getHitResultOnMoveVector` with its own
/// `canHitEntity`, `clip`'s shapes), handed to its `hitTargetOrDeflectSelf` unless a listener of
/// `ProjectileImpactEvent` cancels the hit.
fn hit_along_motion(rt: &mut Rt, e: V, clip: V) {
    let hit = move_hit(rt, e, Filter::CanHit(e), clip);
    if hit.r() == 0 {
        return;
    }
    let kind = api::virtual_call(rt, HIT_GET_TYPE, &[hit]);
    if rt.get_field(kind, "$ordinal").i() != 0 && !crate::world_events::projectile_impact(rt, e, hit) {
        api::virtual_call(rt, HIT_OR_DEFLECT, &[e, hit]);
    }
}

/// Four bubbles behind a projectile in water, a quarter of its motion back from where it is going,
/// moving as it does.
fn bubbles(rt: &mut Rt, e: V, next: [f64; 3], m: [f64; 3]) {
    let bubble = api::get_static(rt, "net/minecraft/core/particles/ParticleTypes.BUBBLE");
    for _ in 0..4 {
        add_particle(rt, e, bubble, [next[0] - m[0] * 0.25, next[1] - m[1] * 0.25, next[2] - m[2] * 0.25], m);
    }
}

/// `level.addParticle(options, at, delta)` in entity `e`'s level (a client's draws it).
fn add_particle(rt: &mut Rt, e: V, options: V, at: [f64; 3], delta: [f64; 3]) {
    let level = api::virtual_call(rt, LEVEL, &[e]);
    let args = [level, options, V::D(at[0]), V::D(at[1]), V::D(at[2]), V::D(delta[0]), V::D(delta[1]), V::D(delta[2])];
    api::call(rt, ADD_PARTICLE, &args);
}

/// `Projectile.shoot`: moving as its `getMovementToShoot` gives, facing along that motion, with no
/// turn to make from the rotation it had (`yRotO`, `xRotO`).
fn shoot(rt: &mut Rt, e: V, direction: [f64; 3], velocity: f32, inaccuracy: f32) {
    let [x, y, z] = direction.map(V::D);
    let m = api::virtual_call(rt, MOVEMENT_TO_SHOOT, &[e, x, y, z, V::F(velocity), V::F(inaccuracy)]);
    api::virtual_call(rt, SET_DELTA, &[e, m]);
    let m = xyz(rt, m);
    let horizontal = (m[0] * m[0] + m[2] * m[2]).sqrt();
    let (yaw, pitch) = (degrees(crate::api_common::mth_atan2(m[0], m[2])), degrees(crate::api_common::mth_atan2(m[1], horizontal)));
    api::virtual_call(rt, SET_Y_ROT, &[e, V::F(yaw)]);
    api::virtual_call(rt, SET_X_ROT, &[e, V::F(pitch)]);
    let (yaw, pitch) = (api::virtual_call(rt, GET_Y_ROT, &[e]), api::virtual_call(rt, GET_X_ROT, &[e]));
    rt.put_field(e, "yRotO", yaw);
    rt.put_field(e, "xRotO", pitch);
}

/// `Projectile.getMovementToShoot`: the direction made a unit long, each axis put off by a
/// triangle draw of the projectile's random spread by `inaccuracy` (0.0172275 each), times the
/// speed.
fn movement_to_shoot(rt: &mut Rt, e: V, direction: [f64; 3], velocity: f32, inaccuracy: f32) -> V {
    let unit = normalize(direction);
    let spread = 0.0172275 * f64::from(inaccuracy);
    let random = rt.get_field(e, "random");
    let mut m = [0.0; 3];
    for (i, u) in unit.into_iter().enumerate() {
        // `RandomSource.triangle(0, spread)`: two draws.
        let (a, b) = (api::call(rt, NEXT_DOUBLE, &[random]).d(), api::call(rt, NEXT_DOUBLE, &[random]).d());
        m[i] = (u + spread * (a - b)) * f64::from(velocity);
    }
    vec3(rt, m)
}

/// `Projectile.shootFromRotation(shooter, xRot, yRot, roll, velocity, inaccuracy)`: shot where that
/// rotation looks (`Mth`'s table sines; the roll tips it up), then the shooter's own motion added
/// (`getKnownMovement`, its fall only while it is off the ground).
fn shoot_from_rotation(rt: &mut Rt, e: V, shooter: V, [x, y, roll]: [f32; 3], velocity: f32, inaccuracy: f32) {
    use crate::api_common::{mth_cos, mth_sin};
    // `(float) (Math.PI / 180.0)`.
    let rad = (std::f64::consts::PI / 180.0) as f32;
    let dx = -mth_sin(V::F(y * rad)) * mth_cos(V::F(x * rad));
    let dy = -mth_sin(V::F((x + roll) * rad));
    let dz = mth_cos(V::F(y * rad)) * mth_cos(V::F(x * rad));
    let [dx, dy, dz] = [dx, dy, dz].map(|c| V::D(f64::from(c)));
    api::virtual_call(rt, SHOOT, &[e, dx, dy, dz, V::F(velocity), V::F(inaccuracy)]);
    let known = api::virtual_call(rt, KNOWN_MOVEMENT, &[shooter]);
    let known = if known.r() != 0 { xyz(rt, known) } else { [0.0; 3] };
    let grounded = api::virtual_call(rt, ON_GROUND, &[shooter]).i() != 0;
    let m = api::virtual_call(rt, DELTA, &[e]);
    let m = xyz(rt, m);
    let v = vec3(rt, [m[0] + known[0], m[1] + if grounded { 0.0 } else { known[1] }, m[2] + known[2]]);
    api::virtual_call(rt, SET_DELTA, &[e, v]);
}

/// `Projectile.updateRotation`: a fifth of the way from its rotation a tick ago to its motion's
/// heading.
fn update_rotation(rt: &mut Rt, e: V) {
    let m = api::virtual_call(rt, DELTA, &[e]);
    let m = xyz(rt, m);
    let horizontal = (m[0] * m[0] + m[2] * m[2]).sqrt();
    let (pitch_o, yaw_o) = (rt.get_field(e, "xRotO").f(), rt.get_field(e, "yRotO").f());
    let pitch = lerp_rotation(pitch_o, degrees(crate::api_common::mth_atan2(m[1], horizontal)));
    let yaw = lerp_rotation(yaw_o, degrees(crate::api_common::mth_atan2(m[0], m[2])));
    api::virtual_call(rt, SET_X_ROT, &[e, V::F(pitch)]);
    api::virtual_call(rt, SET_Y_ROT, &[e, V::F(yaw)]);
}

/// `Projectile.lerpRotation`: from `current`, turned by whole turns to within half a turn of
/// `target`, a fifth of the way to it.
fn lerp_rotation(mut current: f32, target: f32) -> f32 {
    if current.is_finite() && target.is_finite() {
        while target - current < -180.0 {
            current -= 360.0;
        }
        while target - current >= 180.0 {
            current += 360.0;
        }
    }
    current + 0.2 * (target - current)
}

/// `ProjectileUtil.rotateTowardsMovement(projectile, speed)`: facing its motion (a hurting
/// projectile's own convention: yaw from x toward z plus a quarter turn, pitch from straight up less
/// a quarter), its rotation a tick ago turned by whole turns to within half a turn of that, then
/// `speed` of the way there. Nothing for no motion.
fn rotate_towards_movement(rt: &mut Rt, e: V, speed: f32) {
    use crate::api_common::mth_atan2;
    let m = api::virtual_call(rt, DELTA, &[e]);
    let m = xyz(rt, m);
    if m[0] * m[0] + m[1] * m[1] + m[2] * m[2] == 0.0 {
        return;
    }
    let horizontal = (m[0] * m[0] + m[2] * m[2]).sqrt();
    api::virtual_call(rt, SET_Y_ROT, &[e, V::F(degrees(mth_atan2(m[2], m[0])) + 90.0)]);
    api::virtual_call(rt, SET_X_ROT, &[e, V::F(degrees(mth_atan2(horizontal, m[1])) - 90.0)]);
    let (yaw, pitch) = (api::virtual_call(rt, GET_Y_ROT, &[e]).f(), api::virtual_call(rt, GET_X_ROT, &[e]).f());
    let turned = |was: f32, now: f32| {
        let mut was = was;
        if was.is_finite() && now.is_finite() {
            while now - was < -180.0 {
                was -= 360.0;
            }
            while now - was >= 180.0 {
                was += 360.0;
            }
        }
        was
    };
    let pitch_o = turned(rt.get_field(e, "xRotO").f(), pitch);
    let yaw_o = turned(rt.get_field(e, "yRotO").f(), yaw);
    rt.put_field(e, "xRotO", V::F(pitch_o));
    rt.put_field(e, "yRotO", V::F(yaw_o));
    api::virtual_call(rt, SET_X_ROT, &[e, V::F(pitch_o + speed * (pitch - pitch_o))]);
    api::virtual_call(rt, SET_Y_ROT, &[e, V::F(yaw_o + speed * (yaw - yaw_o))]);
}

/// Radians to degrees as vanilla writes it: `(float)(angle * 180.0F / (float)Math.PI)`.
fn degrees(radians: f64) -> f32 {
    (radians * 180.0 / f64::from(std::f32::consts::PI)) as f32
}

/// `Vec3.normalize`: a unit vector, or zero below a float's hundred-thousandth.
fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length < f64::from(1.0e-5f32) { [0.0; 3] } else { v.map(|c| c / length) }
}

/// `ProjectileUtil`'s searches.
fn util_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        // `(level, projectile, from, to, box, filter[, inflation])`
        "getEntityHitResult" if desc.starts_with("(Lnet/minecraft/world/level/Level;") => {
            let inflation = if desc.contains("Predicate;F)") { a(6).f() } else { MARGIN };
            let (from, to) = (xyz(rt, a(2)), xyz(rt, a(3)));
            nearest_hit(rt, a(0), a(1), from, to, a(4), Filter::Predicate(a(5)), inflation)
        }
        // `(shooter, from, to, box, filter, maxDistanceSq)`
        "getEntityHitResult" => {
            let (from, to) = (xyz(rt, a(1)), xyz(rt, a(2)));
            shooter_hit(rt, a(0), from, to, a(3), Filter::Predicate(a(4)), a(5).d())
        }
        // `(projectile, filter[, clip])`: along its motion from where it is.
        "getHitResultOnMoveVector" => {
            let e = a(0);
            let clip = if desc.contains("ClipContext$Block") { a(2) } else { clip_block(rt) };
            move_hit(rt, e, Filter::Predicate(a(1)), clip)
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
            path_hit(rt, e, Filter::Predicate(a(1)), from, motion, 0.0, clip)
        }
        "rotateTowardsMovement" => {
            rotate_towards_movement(rt, a(0), a(1).f());
            V::Void
        }
        _ => return None,
    })
}

/// `ProjectileUtil.getHitResultOnMoveVector`: along projectile `e`'s motion from where it is.
fn move_hit(rt: &mut Rt, e: V, filter: Filter, clip: V) -> V {
    let (from, motion) = (api::virtual_call(rt, POSITION, &[e]), api::virtual_call(rt, DELTA, &[e]));
    let (from, motion) = (xyz(rt, from), xyz(rt, motion));
    path_hit(rt, e, filter, from, motion, MARGIN, clip)
}

/// `ClipContext.Block.COLLIDER`.
fn clip_block(rt: &mut Rt) -> V {
    api::api_enum(rt, "net/minecraft/world/level/ClipContext$Block", "COLLIDER", 0)
}

/// `ProjectileUtil.getHitResult`: the block the path from `from` along `motion` meets first (the
/// level's `clip` with `clip`'s shapes, no fluids), else where the path ends; then the nearest
/// entity the path to there crosses, searched in the projectile's box stretched along the motion and
/// grown by a block, which wins over the block.
fn path_hit(rt: &mut Rt, e: V, filter: Filter, from: [f64; 3], motion: [f64; 3], margin: f32, clip: V) -> V {
    let level = api::virtual_call(rt, LEVEL, &[e]);
    let mut to = [from[0] + motion[0], from[1] + motion[1], from[2] + motion[2]];
    // `level.clip(new ClipContext(from, to, clip, Fluid.NONE, projectile))`.
    let block = crate::shapes::clip_through(rt, level, from, to, rt.get_field(clip, "$ordinal").i(), 0);
    if block.r() != 0 && rt.get_field(block, "$miss").i() == 0 {
        to = xyz(rt, rt.get_field(block, "$loc"));
    }
    let area = swept_area(rt, e, motion);
    match nearest_hit(rt, level, e, from, to, area, filter, margin) {
        hit if hit.r() != 0 => hit,
        _ => block,
    }
}

/// Entity `e`'s box stretched along `motion` and grown by a block each way
/// (`expandTowards(motion).inflate(1.0)`): where a projectile looks for what it may hit or still
/// touch this tick.
fn swept_area(rt: &mut Rt, e: V, motion: [f64; 3]) -> V {
    let bb = api::virtual_call(rt, BOUNDING_BOX, &[e]);
    let mut area = crate::shapes::aabb_of(rt, bb);
    for i in 0..3 {
        if motion[i] < 0.0 {
            area[i] += motion[i];
        } else if motion[i] > 0.0 {
            area[i + 3] += motion[i];
        }
        area[i] -= 1.0;
        area[i + 3] += 1.0;
    }
    crate::shapes::aabb(rt, area)
}

/// `ProjectileUtil.getEntityHitResult(level, projectile, from, to, box, filter, inflation)`: of the
/// level's entities in `area` that pass `filter` (the projectile aside), the one whose box, grown by
/// `inflation`, the segment enters nearest `from`; a hit at that entity's feet. Null for none.
#[allow(clippy::too_many_arguments)]
fn nearest_hit(rt: &mut Rt, level: V, projectile: V, from: [f64; 3], to: [f64; 3], area: V, filter: Filter, inflation: f32) -> V {
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
fn shooter_hit(rt: &mut Rt, shooter: V, from: [f64; 3], to: [f64; 3], area: V, filter: Filter, max_sq: f64) -> V {
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

/// What a search for entities keeps: those a mod's predicate passes (`ProjectileUtil`'s callers'),
/// or those projectile `e`'s own `canHitEntity` can hit (the vanilla ticks' `this::canHitEntity`).
#[derive(Clone, Copy)]
enum Filter {
    Predicate(V),
    CanHit(V),
}

/// `level.getEntities(except, area, filter)`.
fn entities(rt: &mut Rt, level: V, except: V, area: V, filter: Filter) -> Vec<V> {
    let predicate = match filter {
        Filter::Predicate(p) => p,
        Filter::CanHit(_) => V::NULL,
    };
    let list = api::call(rt, GET_ENTITIES, &[level, except, area, predicate]);
    let found = jutil::elements(rt, list);
    match filter {
        Filter::Predicate(_) => found,
        Filter::CanHit(e) => {
            let mut kept = Vec::with_capacity(found.len());
            for target in found {
                if api::virtual_call(rt, CAN_HIT_ENTITY, &[e, target]).i() != 0 {
                    kept.push(target);
                }
            }
            kept
        }
    }
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

// ---- a weapon's ammo ----

/// `getProjectile(weapon)` (NeoForge 1.21.1's): the ammo entity `e` shoots from `weapon`, as
/// `LivingGetProjectileEvent`'s listeners leave it (`CommonHooks.getProjectile`). A player's: what
/// its weapon takes from a hand (`getSupportedHeldProjectiles`: the off hand, then the main one),
/// else the first stack in its inventory it takes (`getAllSupportedProjectiles`), else in creative
/// the weapon's default (`getDefaultCreativeAmmo`), each the weapon's own where its class has one;
/// none at all, unheard, for an item that is no `ProjectileWeaponItem`. A monster's: what it holds
/// that the weapon takes, else an arrow. Any other entity's: none. An exception the weapon's own
/// methods throw (a predicate reading the registries before there are any) ends the search
/// there, as it leaves the JVM's: pending, with nothing found and nobody told.
fn ammo(rt: &mut Rt, e: V, weapon: V) -> V {
    let item = api::call(rt, GET_ITEM, &[weapon]);
    let shoots = rt.instance_of(item, WEAPON);
    let found = if crate::api_common::is_player_class(rt.class_of(e)) {
        if !shoots {
            return crate::menus::empty(rt);
        }
        player_ammo(rt, e, item, weapon)
    } else if shoots && rt.instance_of(e, MONSTER) {
        let takes = api::virtual_call(rt, HELD_AMMO, &[item, weapon]);
        let held = if rt.thrown.is_some() { V::NULL } else { held_ammo(rt, e, takes) };
        if rt.thrown.is_some() || !crate::api_common::stack_empty(rt, held) { held } else { crate::api_common::stack(rt, "minecraft:arrow", 1) }
    } else {
        crate::menus::empty(rt)
    };
    if rt.thrown.is_some() {
        return V::NULL;
    }
    crate::world_events::get_projectile(rt, e, weapon, found)
}

/// A player's ammo for weapon `item` (`weapon` its stack), before the event: see [`ammo`]. The
/// inventory's slots in vanilla's order (`Inventory.getContainerSize()`: the 36 of its items, its
/// armour, its off hand), each the slot's own stack, so what the mod takes from it is taken there.
fn player_ammo(rt: &mut Rt, player: V, item: V, weapon: V) -> V {
    let takes = api::virtual_call(rt, HELD_AMMO, &[item, weapon]);
    if rt.thrown.is_some() {
        return V::NULL;
    }
    let held = held_ammo(rt, player, takes);
    if rt.thrown.is_some() || !crate::api_common::stack_empty(rt, held) {
        return held;
    }
    let takes = api::virtual_call(rt, ALL_AMMO, &[item, weapon]);
    if rt.thrown.is_some() {
        return V::NULL;
    }
    for slot in 0..=40 {
        let s = crate::menus::inventory_get(rt, slot);
        let taken = rt.call_functional(TEST, takes, &[s]).i() != 0;
        if rt.thrown.is_some() {
            return V::NULL;
        }
        if taken {
            return s;
        }
    }
    if rt.host().is_some_and(|h| h.player_creative()) {
        api::virtual_call(rt, CREATIVE_AMMO, &[item, player, weapon])
    } else {
        crate::menus::empty(rt)
    }
}

/// `ProjectileWeaponItem.getHeldProjectile(shooter, isAmmo)`: the stack in the shooter's off hand
/// if it is ammo, else the one in its main hand if that is, else none.
fn held_ammo(rt: &mut Rt, shooter: V, takes: V) -> V {
    for hand in ["net/minecraft/world/InteractionHand.OFF_HAND", "net/minecraft/world/InteractionHand.MAIN_HAND"] {
        let hand = api::get_static(rt, hand);
        let s = api::virtual_call(rt, IN_HAND, &[shooter, hand]);
        let taken = rt.call_functional(TEST, takes, &[s]).i() != 0;
        if rt.thrown.is_some() {
            return V::NULL;
        }
        if taken {
            return s;
        }
    }
    crate::menus::empty(rt)
}

/// `ProjectileWeaponItem`'s own methods, for a mod's weapon whose class leaves them to it: its
/// ammo from a hand is its whole ammo's (with the stack) or its held ammo's, which is its whole
/// ammo's; its whole ammo with the stack is its whole ammo (`getAllSupportedProjectiles()`, the
/// mod's own: vanilla's is abstract); its creative ammo an arrow.
fn weapon_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let with_stack = desc.starts_with("(Lnet/minecraft/world/item/ItemStack;)");
    Some(match name {
        "getHeldProjectile" => held_ammo(rt, a(0), a(1)),
        "getSupportedHeldProjectiles" if with_stack => {
            let all = api::virtual_call(rt, ALL_AMMO, &[a(0), a(1)]);
            let held = api::virtual_call(rt, HELD_AMMO_PLAIN, &[a(0)]);
            api::call(rt, OR, &[all, held])
        }
        "getSupportedHeldProjectiles" | "getAllSupportedProjectiles" if with_stack || name == "getSupportedHeldProjectiles" => api::virtual_call(rt, ALL_AMMO_PLAIN, &[a(0)]),
        "getDefaultCreativeAmmo" => crate::api_common::stack(rt, "minecraft:arrow", 1),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityView, HandStack, MemoryHost};

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

    const PEBBLE: &str = "test/Pebble";
    const EMBER: &str = "test/Ember";

    /// The tests' projectiles have their own `onHitEntity` and `onHitBlock`, which note what they
    /// get (and which projectile got it); nothing else of their own.
    fn overrides(class: &str, short: &str) -> bool {
        matches!(class, BOLT | PEBBLE) && (short == ON_HIT_ENTITY.1 || short == ON_HIT_BLOCK.1)
    }

    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if matches!(rt.class_of(args[0]), BOLT | PEBBLE) && short == ON_HIT_ENTITY.1 {
            let target = api::call(rt, GET_ENTITY, &[args[1]]);
            rt.statics.insert("t.entity", target);
            rt.statics.insert("t.by", args[0]);
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

    fn pos(rt: &mut Rt, e: V) -> [f64; 3] {
        let p = api::call(rt, GET_POS, &[e]);
        xyz(rt, p)
    }

    fn motion(rt: &mut Rt, e: V) -> [f64; 3] {
        let m = api::call(rt, "net/minecraft/world/entity/Entity.getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;", &[e]);
        xyz(rt, m)
    }

    /// Yaw and pitch.
    fn rotation(rt: &mut Rt, e: V) -> [f32; 2] {
        let yaw = api::call(rt, "net/minecraft/world/entity/Entity.getYRot.()F", &[e]).f();
        [yaw, api::call(rt, "net/minecraft/world/entity/Entity.getXRot.()F", &[e]).f()]
    }

    /// A mod's projectiles whose classes have no tick of their own fly as vanilla's ticks move
    /// them. A thrown one shot along +z at 1.5 moves by its motion each tick, which the air slows
    /// by 0.99 (a float) and its gravity pulls down by 0.03 (none after `setNoGravity`), turning a
    /// fifth of the way toward its path; on the tick its path reaches a cow (the cow's box grown by
    /// 0.3 starts at z 3.75) it hits it. One a player throws doesn't hit them while beside them. A
    /// hurting one gains 0.1 along its heading, loses 5% (a float) and burns. One below the world is
    /// gone.
    #[test]
    fn a_mod_projectile_flies_as_vanillas_ticks_move_it() {
        const ADD: &str = "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z";
        const SHOOT: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.shoot.(DDDFF)V";
        const NO_GRAVITY: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.setNoGravity.(Z)V";
        const SET_ITEM: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.setItem.(Lnet/minecraft/world/item/ItemStack;)V";
        const GET_ITEM: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.getItem.()Lnet/minecraft/world/item/ItemStack;";
        const EMBER_AT: &str = "net/minecraft/world/entity/projectile/AbstractHurtingProjectile.<init>.(Lnet/minecraft/world/entity/EntityType;DDDLnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/level/Level;)V";
        const ON_FIRE: &str = "net/minecraft/world/entity/Entity.isOnFire.()Z";
        const REMOVED: &str = "net/minecraft/world/entity/Entity.isRemoved.()Z";
        const DISCARD: &str = "net/minecraft/world/entity/Entity.discard.()V";
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(PEBBLE, THROWN);
        rt.supers.insert(EMBER, HURTING);
        let ty = rt.new_object("net/minecraft/world/entity/EntityType");
        rt.put_field(ty, "$width", V::F(0.25));
        rt.put_field(ty, "$height", V::F(0.25));
        const STEVE: u128 = 0x5;
        let steve_at = [-10.5, 64.0, -10.5];
        let mut host = MemoryHost::default();
        host.entities.push(EntityView { id: 7, kind: "minecraft:cow".into(), pos: [0.5, 64.0, 4.5], yaw: 0.0, health: Some((10.0, 10.0)) });
        host.players.push((STEVE, steve_at));
        host.player_entity_ids.insert(STEVE, 9);
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let tick = |rt: &mut Rt| crate::mod_entities::tick_all(rt, "minecraft:overworld");
            let drag = f64::from(0.99f32);

            let pebble = rt.new_object(PEBBLE);
            api::call(rt, AT, &[pebble, ty, V::D(0.5), V::D(64.5), V::D(0.5), level]);
            api::call(rt, ADD, &[level, pebble]);
            api::call(rt, SHOOT, &[pebble, V::D(0.0), V::D(0.0), V::D(2.0), V::F(1.5), V::F(0.0)]);
            assert_eq!(motion(rt, pebble), [0.0, 0.0, 1.5]);
            assert_eq!(rotation(rt, pebble), [0.0, 0.0], "facing along +z, level");
            let snow = crate::api_common::stack(rt, "minecraft:snowball", 5);
            api::call(rt, SET_ITEM, &[pebble, snow]);
            let held = api::call(rt, GET_ITEM, &[pebble]);
            assert_eq!(api::call(rt, "net/minecraft/world/item/ItemStack.getCount.()I", &[held]), V::I(1), "one of the stack");

            tick(rt);
            assert_eq!(pos(rt, pebble), [0.5, 64.5, 0.5 + 1.5]);
            assert_eq!(motion(rt, pebble), [0.0, -0.03, 1.5 * drag]);
            tick(rt);
            assert_eq!(pos(rt, pebble), [0.5, 64.5 - 0.03, 0.5 + 1.5 + 1.5 * drag]);
            assert_eq!(motion(rt, pebble), [0.0, -0.03 * drag - 0.03, 1.5 * drag * drag]);
            // Turned a fifth of the way toward the path it had, now dipping.
            let toward = (-0.03f64).atan2(1.5 * drag).to_degrees() as f32;
            let pitch = rotation(rt, pebble)[1];
            assert!((pitch - 0.2 * toward).abs() < 1e-3, "pitch {pitch}, toward {toward}");
            assert!(!rt.statics.contains_key("t.entity"), "short of the cow");
            tick(rt);
            let cow = rt.statics.get("t.entity").copied().unwrap_or_default();
            assert_eq!(api::call(rt, "net/minecraft/world/entity/Entity.getId.()I", &[cow]), V::I(7));
            assert_eq!(rt.statics.get("t.by"), Some(&pebble));
            api::call(rt, DISCARD, &[pebble]);

            // Thrown by a player, from their eyes along +z, from inside their box: not a hit on them
            // while their box is still in its reach.
            rt.statics.remove("t.entity");
            let steve = crate::vanilla_entities::player(rt, 9, STEVE, steve_at);
            let thrown = rt.new_object(PEBBLE);
            api::call(rt, BY, &[thrown, ty, steve, level]);
            api::call(rt, ADD, &[level, thrown]);
            api::call(rt, SHOOT, &[thrown, V::D(0.0), V::D(0.0), V::D(1.0), V::F(1.5), V::F(0.0)]);
            tick(rt);
            assert!(!rt.statics.contains_key("t.entity"), "no hit on its thrower");
            assert_eq!(api::call(rt, CAN_HIT, &[thrown, steve]), V::I(0), "still beside them");
            for _ in 0..3 {
                tick(rt);
            }
            assert_eq!(api::call(rt, CAN_HIT, &[thrown, steve]), V::I(1), "it has left them");

            // No gravity: only the air slows it.
            let floating = rt.new_object(PEBBLE);
            api::call(rt, AT, &[floating, ty, V::D(10.5), V::D(70.5), V::D(0.5), level]);
            api::call(rt, ADD, &[level, floating]);
            api::call(rt, NO_GRAVITY, &[floating, V::I(1)]);
            api::call(rt, SHOOT, &[floating, V::D(1.0), V::D(0.0), V::D(0.0), V::F(1.0), V::F(0.0)]);
            assert!((rotation(rt, floating)[0] - 90.0).abs() < 1e-3, "facing along +x");
            tick(rt);
            assert_eq!(pos(rt, floating), [10.5 + 1.0, 70.5, 0.5]);
            assert_eq!(motion(rt, floating), [drag, 0.0, 0.0]);

            // A hurting one, made moving along +z: at its acceleration (0.1), then faster.
            let ember = rt.new_object(EMBER);
            let along = vec3(rt, [0.0, 0.0, 2.0]);
            api::call(rt, EMBER_AT, &[ember, ty, V::D(20.5), V::D(70.5), V::D(0.5), along, level]);
            api::call(rt, ADD, &[level, ember]);
            assert_eq!(motion(rt, ember), [0.0, 0.0, 0.1]);
            let inertia = f64::from(0.95f32);
            tick(rt);
            assert_eq!(pos(rt, ember), [20.5, 70.5, 0.5 + 0.1]);
            assert_eq!(motion(rt, ember), [0.0, 0.0, (0.1 + 0.1) * inertia]);
            assert_eq!(api::call(rt, ON_FIRE, &[ember]), V::I(1), "it burns");
            tick(rt);
            assert_eq!(pos(rt, ember), [20.5, 70.5, 0.5 + 0.1 + (0.1 + 0.1) * inertia]);
            assert_eq!(motion(rt, ember), [0.0, 0.0, ((0.1 + 0.1) * inertia + 0.1) * inertia]);

            // Below the world (64 under its bottom, y -128): gone.
            let lost = rt.new_object(PEBBLE);
            api::call(rt, AT, &[lost, ty, V::D(0.5), V::D(-130.0), V::D(0.5), level]);
            api::call(rt, ADD, &[level, lost]);
            tick(rt);
            assert_eq!(api::call(rt, REMOVED, &[lost]), V::I(1));
            assert!(!crate::mod_entities::live(rt).contains(&lost));
        });
    }

    const SLING: &str = "test/Sling";
    const SNOWBALLS: &str = "test/SnowballsOnly";
    const CURSED: &str = "test/CursedSling";
    const REGISTRY_LESS: &str = "test/RegistryLessAmmo";
    const SWAP: &str = "test/AmmoSwap";
    const SNOWBALL_TEST: &str = "test.(Ljava/lang/Object;)Z";
    const ACCEPT: &str = "accept.(Ljava/lang/Object;)V";
    const PLAYER_GET_PROJECTILE: &str = "net/minecraft/world/entity/player/Player.getProjectile.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/ItemStack;";
    const SET_AMMO: &str = "net/neoforged/neoforge/event/entity/living/LivingGetProjectileEvent.setProjectileItemStack.(Lnet/minecraft/world/item/ItemStack;)V";
    const GET_WEAPON: &str = "net/neoforged/neoforge/event/entity/living/LivingGetProjectileEvent.getProjectileWeaponItemStack.()Lnet/minecraft/world/item/ItemStack;";

    /// The tests' weapon, a sling, takes snowballs (its own `getAllSupportedProjectiles()`, the
    /// one `ProjectileWeaponItem` leaves abstract); a listener of `LivingGetProjectileEvent` puts
    /// two arrows in place of what was found, noting the weapon it was told of. A cursed sling's
    /// ammo test throws, as Create's does where it finds no registries, counting its runs.
    fn ammo_vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match (rt.class_of(args[0]), short) {
            (SLING, s) if s == ALL_AMMO_PLAIN.1 => rt.new_object(SNOWBALLS),
            (CURSED, s) if s == ALL_AMMO_PLAIN.1 => rt.new_object(REGISTRY_LESS),
            (REGISTRY_LESS, SNOWBALL_TEST) => {
                let runs = rt.statics.get("t.tests").map_or(0, |v| v.i());
                rt.statics.insert("t.tests", V::I(runs + 1));
                rt.throw_new("java/lang/IllegalStateException");
                V::I(0)
            }
            (SNOWBALLS, SNOWBALL_TEST) => {
                let item = api::call(rt, GET_ITEM, &[args[1]]);
                V::I(i32::from(crate::api_common::item_like_name(rt, item) == "minecraft:snowball"))
            }
            (SWAP, ACCEPT) => {
                let weapon = api::call(rt, GET_WEAPON, &[args[1]]);
                rt.statics.insert("t.weapon", weapon);
                let arrows = crate::api_common::stack(rt, "minecraft:arrow", 2);
                api::call(rt, SET_AMMO, &[args[1], arrows])
            }
            _ => api::call(rt, key, args),
        }
    }

    fn stack_of(rt: &Rt, s: V) -> Option<(String, i32)> {
        crate::menus::stack_view(rt, s)
    }

    /// A player's ammo for a mod's projectile weapon (`getProjectile`), as NeoForge 1.21.1 finds
    /// it: what the weapon takes from the off hand first; else the first slot of the inventory, in
    /// vanilla's order, whose stack it takes, that very stack (so what the mod takes from it is
    /// gone from the slot); else nothing, but in creative the weapon's default, an arrow. An item
    /// that is no weapon finds nothing. A `LivingGetProjectileEvent` listener is told the weapon and
    /// may hand other ammo.
    #[test]
    fn a_players_weapon_finds_its_ammo_in_a_hand_then_the_inventory() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(ammo_vcall);
        rt.supers.insert(SLING, WEAPON);
        let sling = rt.new_object(SLING);
        rt.registered.insert(("item".into(), "testmod:sling".into()), sling);
        let snowballs = |count| HandStack { item: "minecraft:snowball".into(), count, ..HandStack::default() };
        let mut host = MemoryHost { held: Some(("testmod:sling".into(), 1)), inventory: true, ..MemoryHost::default() };
        host.slots.insert(7, snowballs(3));
        host.slots.insert(5, snowballs(16));
        host.slots.insert(2, HandStack { item: "minecraft:stick".into(), count: 1, ..HandStack::default() });
        let ammo = |rt: &mut Rt| {
            let player = crate::api_common::acting_player(rt);
            let weapon = crate::hand::stack(rt, crate::hand::MAIN);
            api::call(rt, PLAYER_GET_PROJECTILE, &[player, weapon])
        };
        rt.with_host(&mut host, |rt| {
            crate::hand::with_hand(rt, |rt| {
                let found = ammo(rt);
                assert_eq!(stack_of(rt, found), Some(("minecraft:snowball".into(), 16)), "the first slot that holds some");
                assert_eq!(found, crate::menus::inventory_get(rt, 5), "the slot's own stack");
                api::call(rt, "net/minecraft/world/item/ItemStack.shrink.(I)V", &[found, V::I(1)]);
            })
        });
        assert_eq!(host.slots[&5].count, 15, "a shot taken from it is gone from the slot");

        host.offhand = Some(snowballs(1));
        rt.with_host(&mut host, |rt| {
            let found = ammo(rt);
            assert_eq!(found, crate::hand::stack(rt, crate::hand::OFF), "the off hand's comes first");
        });

        host.offhand = None;
        host.slots.retain(|_, s| s.item != "minecraft:snowball");
        rt.with_host(&mut host, |rt| {
            let found = ammo(rt);
            assert!(crate::api_common::stack_empty(rt, found), "none left");
        });
        host.creative = true;
        rt.with_host(&mut host, |rt| {
            let found = ammo(rt);
            assert_eq!(stack_of(rt, found), Some(("minecraft:arrow".into(), 1)), "in creative, the weapon's default");
        });

        // Not a weapon: nothing, and no listener hears of it.
        host.held = Some(("minecraft:stick".into(), 1));
        let swap = rt.new_object(SWAP);
        rt.listeners.push(("net/neoforged/neoforge/event/entity/living/LivingGetProjectileEvent".into(), swap));
        rt.with_host(&mut host, |rt| {
            let found = ammo(rt);
            assert!(crate::api_common::stack_empty(rt, found));
        });
        assert!(!rt.statics.contains_key("t.weapon"));

        // A listener hands two arrows in place of the creative default, told of the sling.
        host.held = Some(("testmod:sling".into(), 1));
        rt.with_host(&mut host, |rt| {
            let found = ammo(rt);
            assert_eq!(stack_of(rt, found), Some(("minecraft:arrow".into(), 2)));
            let weapon = crate::hand::stack(rt, crate::hand::MAIN);
            assert_eq!(rt.statics.get("t.weapon"), Some(&weapon));
        });
    }

    /// An exception a weapon's ammo test throws ends the search as it leaves the JVM's: it is the
    /// caller's, the test ran once (on the off hand's stack, the first one asked), and no
    /// listener of `LivingGetProjectileEvent` was told.
    #[test]
    fn an_exception_in_a_weapons_ammo_test_ends_the_search() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(ammo_vcall);
        rt.supers.insert(CURSED, WEAPON);
        let cursed = rt.new_object(CURSED);
        rt.registered.insert(("item".into(), "testmod:cursed_sling".into()), cursed);
        let swap = rt.new_object(SWAP);
        rt.listeners.push(("net/neoforged/neoforge/event/entity/living/LivingGetProjectileEvent".into(), swap));
        let mut host = MemoryHost { held: Some(("testmod:cursed_sling".into(), 1)), inventory: true, creative: true, ..MemoryHost::default() };
        host.slots.insert(5, HandStack { item: "minecraft:snowball".into(), count: 16, ..HandStack::default() });
        rt.with_host(&mut host, |rt| {
            let player = crate::api_common::acting_player(rt);
            let weapon = crate::hand::stack(rt, crate::hand::MAIN);
            api::call(rt, PLAYER_GET_PROJECTILE, &[player, weapon]);
        });
        let thrown = rt.thrown.take().expect("the exception is the caller's");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalStateException");
        assert_eq!(rt.statics.get("t.tests"), Some(&V::I(1)));
        assert!(!rt.statics.contains_key("t.weapon"), "no listener told");
    }

    /// A blow a mod's projectile deals tells the game the projectile's movement (vanilla knocks
    /// the target back along it) and the thrower it is credited to; the projectile saves and loads
    /// what 1.21.1's thrown item does: its owner by UUID, `LeftOwner` once it has, `HasBeenShot`
    /// and its stack (`Item`); one loaded with no stack holds its class's default item.
    #[test]
    fn a_mod_projectiles_blow_follows_its_path_and_it_saves_its_owner_and_stack() {
        const HURT: &str = "net/minecraft/world/entity/LivingEntity.hurt.(Lnet/minecraft/world/damagesource/DamageSource;F)Z";
        const NEW: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
        const SAVE: &str = "net/minecraft/world/entity/Entity.saveWithoutId.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;";
        const LOAD: &str = "net/minecraft/world/entity/Entity.load.(Lnet/minecraft/nbt/CompoundTag;)V";
        const ITEM: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.getItem.()Lnet/minecraft/world/item/ItemStack;";
        const SET: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.setItem.(Lnet/minecraft/world/item/ItemStack;)V";
        const OWNER: &str = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.setOwner.(Lnet/minecraft/world/entity/Entity;)V";
        let mut rt = Rt::new();
        crate::install();
        rt.supers.insert(PEBBLE, THROWN);
        let ty = rt.new_object("net/minecraft/world/entity/EntityType");
        rt.put_field(ty, "$width", V::F(0.25));
        rt.put_field(ty, "$height", V::F(0.25));
        rt.registered.insert(("entity_type".into(), "test:pebble".into()), ty);
        let mut host = MemoryHost::default();
        host.entities.push(EntityView { id: 7, kind: "minecraft:cow".into(), pos: [0.5, 64.0, 4.5], yaw: 0.0, health: Some((10.0, 10.0)) });
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let player = crate::menus::server_player(rt, 42);
            let pebble = rt.new_object(PEBBLE);
            api::call(rt, AT, &[pebble, ty, V::D(0.5), V::D(64.5), V::D(2.5), level]);
            api::call(rt, MOTION, &[pebble, V::D(0.25), V::D(-0.1), V::D(1.0)]);
            api::call(rt, OWNER, &[pebble, player]);
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            let source = crate::damage::source(rt, "minecraft:thrown", pebble, player, V::NULL);
            assert_eq!(api::call(rt, HURT, &[cow, source, V::F(2.0)]), V::I(1));

            let snowballs = crate::api_common::stack(rt, "minecraft:snowball", 3);
            api::call(rt, SET, &[pebble, snowballs]);
            rt.put_field(pebble, "$leftOwner", V::I(1));
            let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
            api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
            api::call(rt, SAVE, &[pebble, tag]);
            let key = |rt: &mut Rt, k: &str| rt.string(k);
            let k = key(rt, "Owner");
            let owner = api::call(rt, "net/minecraft/nbt/CompoundTag.getUUID.(Ljava/lang/String;)Ljava/util/UUID;", &[tag, k]);
            assert_eq!(crate::tags::uuid_bits(rt, owner), (0, 42));
            for (k, want) in [("LeftOwner", 1), ("HasBeenShot", 0)] {
                let k = key(rt, k);
                assert_eq!(api::call(rt, "net/minecraft/nbt/CompoundTag.getBoolean.(Ljava/lang/String;)Z", &[tag, k]), V::I(want));
            }

            let again = rt.new_object(PEBBLE);
            api::call(rt, NEW, &[again, ty, level]);
            api::call(rt, LOAD, &[again, tag]);
            assert!(rt.thrown.is_none());
            let stack = api::call(rt, ITEM, &[again]);
            let snowball = api::get_static(rt, "net/minecraft/world/item/Items.SNOWBALL");
            assert_eq!(api::call(rt, "net/minecraft/world/item/ItemStack.is.(Lnet/minecraft/world/item/Item;)Z", &[stack, snowball]), V::I(1));
            assert_eq!(api::call(rt, "net/minecraft/world/item/ItemStack.getCount.()I", &[stack]), V::I(1));
            let owned_by = "net/minecraft/world/entity/projectile/ThrowableItemProjectile.ownedBy.(Lnet/minecraft/world/entity/Entity;)Z";
            assert_eq!(api::call(rt, owned_by, &[again, player]), V::I(1));
            // Saved again, it writes what it read.
            let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
            api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
            api::call(rt, SAVE, &[again, tag]);
            let k = key(rt, "LeftOwner");
            assert_eq!(api::call(rt, "net/minecraft/nbt/CompoundTag.getBoolean.(Ljava/lang/String;)Z", &[tag, k]), V::I(1));
        });
        let blow = crate::host::HurtBy {
            damage_type: "minecraft:thrown".into(),
            player: Some(42),
            mob: None,
            from: Some([0.5, 64.5, 2.5]),
            motion: Some([0.25, 1.0]),
            direct: Some("test:pebble".into()),
        };
        assert_eq!(host.blows, vec![(7, 2.0, blow)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
