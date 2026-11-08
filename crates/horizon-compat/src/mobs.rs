//! Mods' living entities and mobs, as vanilla's `LivingEntity`, `Mob`, `PathfinderMob`,
//! `AgeableMob`, `Animal`, `Monster` and `WaterAnimal` run a mod's class that extends them: its
//! health and attributes, its tick (the base tick: air, hurt and death times; the AI step: the
//! goals, the navigation and the controls, then `travel`: walking with block friction, swimming,
//! gravity, collisions with stepping up), its controls (`MoveControl`, `LookControl`, `JumpControl`,
//! `BodyRotationControl`), its body and head turning, its walk animation, and its fights: being
//! hurt (armour, knockback, the hurt and death sounds), dying, falling, attacking.
//!
//! A mob is the object of the mod's class; its state is kept in fields of vanilla's names where a
//! mod's code or a renderer reads them (`goalSelector`, `navigation`, `moveControl`, `xxa`,
//! `yBodyRot`, `yHeadRot`, `walkAnimation`, `hurtTime`, `deathTime`...), the rest under `$` names.
//! Every method a mod may override is called virtually, as the JVM does: the mod's own where its
//! class has one, else vanilla's here (a call to `super` lands here too, on the class it names).
//!
//! The server runs the AI; a client's copy follows the server's moves (`lerpTo`, three ticks), turns
//! its body and animates its walk as vanilla's client does, and hears what the server tells it of
//! (`handleEntityEvent`, a hurt's flash, a swing, the health and baby state) through
//! [`crate::mod_entities`]'s messages.
//!
//! Not modelled yet: effects, equipment drops and loot tables, riding mobs, leashes, breeding,
//! brains (`Brain`), natural spawning, the fluid current's push, entities pushing each other.
use crate::{Rt, V, api, goals, pathing};

pub(crate) const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
pub(crate) const MOB: &str = "net/minecraft/world/entity/Mob";
const ENTITY: &str = "net/minecraft/world/entity/Entity";
const AGEABLE: &str = "net/minecraft/world/entity/AgeableMob";
const ANIMAL: &str = "net/minecraft/world/entity/animal/Animal";
const MONSTER: &str = "net/minecraft/world/entity/monster/Monster";
const WATER_ANIMAL: &str = "net/minecraft/world/entity/animal/WaterAnimal";
const FLYING_ANIMAL: &str = "net/minecraft/world/entity/animal/FlyingAnimal";
const PLAYER: &str = "net/minecraft/world/entity/player/Player";
pub(crate) const LOOK_CONTROL: &str = "net/minecraft/world/entity/ai/control/LookControl";
pub(crate) const MOVE_CONTROL: &str = "net/minecraft/world/entity/ai/control/MoveControl";
pub(crate) const JUMP_CONTROL: &str = "net/minecraft/world/entity/ai/control/JumpControl";
const BODY_CONTROL: &str = "net/minecraft/world/entity/ai/control/BodyRotationControl";
const OPERATION: &str = "net/minecraft/world/entity/ai/control/MoveControl$Operation";
const OPERATIONS: [&str; 4] = ["WAIT", "MOVE_TO", "STRAFE", "JUMPING"];
const WALK_ANIMATION: &str = "net/minecraft/world/entity/WalkAnimationState";
const REMOVAL: &str = "net/minecraft/world/entity/Entity$RemovalReason";
const POSE: &str = "net/minecraft/world/entity/Pose";
const HAND: &str = "net/minecraft/world/InteractionHand";
const RANDOM_FLOAT: &str = "net/minecraft/util/RandomSource.nextFloat.()F";
const RANDOM_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const RANDOM_DOUBLE: &str = "net/minecraft/util/RandomSource.nextDouble.()D";
const RANDOM_CREATE: &str = "net/minecraft/util/RandomSource.create.()Lnet/minecraft/util/RandomSource;";
const GET_BOX: &str = "net/minecraft/world/entity/Entity.getBoundingBox.()Lnet/minecraft/world/phys/AABB;";
const GET_EYE_Y: &str = "net/minecraft/world/entity/Entity.getEyeY.()D";

/// A virtual method: the key its vanilla owner answers under, and its name and descriptor.
type Key = (&'static str, &'static str);

macro_rules! key {
    ($owner:literal, $short:literal) => {
        (concat!($owner, ".", $short), $short)
    };
}

const BASE_TICK: Key = key!("net/minecraft/world/entity/Entity", "baseTick.()V");
const AI_STEP: Key = key!("net/minecraft/world/entity/LivingEntity", "aiStep.()V");
const TRAVEL: Key = key!("net/minecraft/world/entity/LivingEntity", "travel.(Lnet/minecraft/world/phys/Vec3;)V");
const CUSTOM_AI: Key = key!("net/minecraft/world/entity/Mob", "customServerAiStep.()V");
const REGISTER_GOALS: Key = key!("net/minecraft/world/entity/Mob", "registerGoals.()V");
const CREATE_NAVIGATION: Key = key!(
    "net/minecraft/world/entity/Mob",
    "createNavigation.(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/ai/navigation/PathNavigation;"
);
const CREATE_BODY: Key = key!("net/minecraft/world/entity/Mob", "createBodyControl.()Lnet/minecraft/world/entity/ai/control/BodyRotationControl;");
const HURT: Key = key!("net/minecraft/world/entity/Entity", "hurt.(Lnet/minecraft/world/damagesource/DamageSource;F)Z");
const ACTUALLY_HURT: Key = key!("net/minecraft/world/entity/LivingEntity", "actuallyHurt.(Lnet/minecraft/world/damagesource/DamageSource;F)V");
const DIE: Key = key!("net/minecraft/world/entity/LivingEntity", "die.(Lnet/minecraft/world/damagesource/DamageSource;)V");
const TICK_DEATH: Key = key!("net/minecraft/world/entity/LivingEntity", "tickDeath.()V");
const KNOCKBACK: Key = key!("net/minecraft/world/entity/LivingEntity", "knockback.(DDD)V");
const HURT_SOUND: Key = key!(
    "net/minecraft/world/entity/LivingEntity",
    "getHurtSound.(Lnet/minecraft/world/damagesource/DamageSource;)Lnet/minecraft/sounds/SoundEvent;"
);
const DEATH_SOUND: Key = key!("net/minecraft/world/entity/LivingEntity", "getDeathSound.()Lnet/minecraft/sounds/SoundEvent;");
const AMBIENT_SOUND: Key = key!("net/minecraft/world/entity/Mob", "getAmbientSound.()Lnet/minecraft/sounds/SoundEvent;");
const PLAY_AMBIENT: Key = key!("net/minecraft/world/entity/Mob", "playAmbientSound.()V");
const AMBIENT_INTERVAL: Key = key!("net/minecraft/world/entity/Mob", "getAmbientSoundInterval.()I");
const SOUND_VOLUME: Key = key!("net/minecraft/world/entity/LivingEntity", "getSoundVolume.()F");
const VOICE_PITCH: Key = key!("net/minecraft/world/entity/LivingEntity", "getVoicePitch.()F");
const PLAY_SOUND: Key = key!("net/minecraft/world/entity/Entity", "playSound.(Lnet/minecraft/sounds/SoundEvent;FF)V");
const STEP_SOUND: Key = key!(
    "net/minecraft/world/entity/Entity",
    "playStepSound.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V"
);
const MAX_HEAD_X: Key = key!("net/minecraft/world/entity/Mob", "getMaxHeadXRot.()I");
const MAX_HEAD_Y: Key = key!("net/minecraft/world/entity/Mob", "getMaxHeadYRot.()I");
const HEAD_ROT_SPEED: Key = key!("net/minecraft/world/entity/Mob", "getHeadRotSpeed.()I");
const JUMP_FROM_GROUND: Key = key!("net/minecraft/world/entity/LivingEntity", "jumpFromGround.()V");
const JUMP_POWER: Key = key!("net/minecraft/world/entity/LivingEntity", "getJumpPower.()F");
const FLUID_JUMP: Key = key!("net/minecraft/world/entity/LivingEntity", "getFluidJumpThreshold.()D");
const CAUSE_FALL: Key = key!("net/minecraft/world/entity/Entity", "causeFallDamage.(FFLnet/minecraft/world/damagesource/DamageSource;)Z");
const WALK_TARGET: Key = key!(
    "net/minecraft/world/entity/PathfinderMob",
    "getWalkTargetValue.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/LevelReader;)F"
);
const CAN_BREATHE: Key = key!("net/minecraft/world/entity/LivingEntity", "canBreatheUnderwater.()Z");
const CAN_ATTACK: Key = key!("net/minecraft/world/entity/LivingEntity", "canAttack.(Lnet/minecraft/world/entity/LivingEntity;)Z");
const DO_HURT_TARGET: Key = key!("net/minecraft/world/entity/Mob", "doHurtTarget.(Lnet/minecraft/world/entity/Entity;)Z");
const MELEE_RANGE: Key = key!("net/minecraft/world/entity/Mob", "isWithinMeleeAttackRange.(Lnet/minecraft/world/entity/LivingEntity;)Z");
const GET_TARGET: Key = key!("net/minecraft/world/entity/Mob", "getTarget.()Lnet/minecraft/world/entity/LivingEntity;");
const SET_TARGET: Key = key!("net/minecraft/world/entity/Mob", "setTarget.(Lnet/minecraft/world/entity/LivingEntity;)V");
const GET_NAVIGATION: Key = key!("net/minecraft/world/entity/Mob", "getNavigation.()Lnet/minecraft/world/entity/ai/navigation/PathNavigation;");
const GET_MOVE_CONTROL: Key = key!("net/minecraft/world/entity/Mob", "getMoveControl.()Lnet/minecraft/world/entity/ai/control/MoveControl;");
const GET_LOOK_CONTROL: Key = key!("net/minecraft/world/entity/Mob", "getLookControl.()Lnet/minecraft/world/entity/ai/control/LookControl;");
const GET_JUMP_CONTROL: Key = key!("net/minecraft/world/entity/Mob", "getJumpControl.()Lnet/minecraft/world/entity/ai/control/JumpControl;");
const GET_SPEED: Key = key!("net/minecraft/world/entity/LivingEntity", "getSpeed.()F");
const SET_SPEED: Key = key!("net/minecraft/world/entity/LivingEntity", "setSpeed.(F)V");
const IS_PERSISTENT: Key = key!("net/minecraft/world/entity/Mob", "isPersistenceRequired.()Z");
const CUSTOM_PERSISTENCE: Key = key!("net/minecraft/world/entity/Mob", "requiresCustomPersistence.()Z");
const REMOVE_FAR: Key = key!("net/minecraft/world/entity/Mob", "removeWhenFarAway.(D)Z");
const HANDLE_EVENT: Key = key!("net/minecraft/world/entity/Entity", "handleEntityEvent.(B)V");
const IS_BABY: Key = key!("net/minecraft/world/entity/LivingEntity", "isBaby.()Z");
const MAX_UP_STEP: Key = key!("net/minecraft/world/entity/Entity", "maxUpStep.()F");
const MAX_FALL: Key = key!("net/minecraft/world/entity/Entity", "getMaxFallDistance.()I");
const DEFAULT_GRAVITY: Key = key!("net/minecraft/world/entity/Entity", "getDefaultGravity.()D");
const NO_GRAVITY: Key = key!("net/minecraft/world/entity/Entity", "isNoGravity.()Z");
const REMOVE: Key = key!("net/minecraft/world/entity/Entity", "remove.(Lnet/minecraft/world/entity/Entity$RemovalReason;)V");
const AGE_BOUNDARY: Key = key!("net/minecraft/world/entity/AgeableMob", "ageBoundaryReached.()V");
const IS_ALIVE: Key = key!("net/minecraft/world/entity/Entity", "isAlive.()Z");
const IS_ALLIED: Key = key!("net/minecraft/world/entity/Entity", "isAlliedTo.(Lnet/minecraft/world/entity/Entity;)Z");
const SWING: Key = key!("net/minecraft/world/entity/LivingEntity", "swing.(Lnet/minecraft/world/InteractionHand;)V");
const NAV_TICK: Key = key!("net/minecraft/world/entity/ai/navigation/PathNavigation", "tick.()V");
const NAV_DONE: Key = key!("net/minecraft/world/entity/ai/navigation/PathNavigation", "isDone.()Z");
const MOVE_TICK: Key = key!("net/minecraft/world/entity/ai/control/MoveControl", "tick.()V");
const LOOK_TICK: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "tick.()V");
const JUMP_TICK: Key = key!("net/minecraft/world/entity/ai/control/JumpControl", "tick.()V");
const BODY_TICK: Key = key!("net/minecraft/world/entity/ai/control/BodyRotationControl", "clientTick.()V");
const SET_WANTED: Key = key!("net/minecraft/world/entity/ai/control/MoveControl", "setWantedPosition.(DDDD)V");
const LOOK_AT_POINT: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "setLookAt.(DDDFF)V");
const LOOK_AT_ENTITY: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "setLookAt.(Lnet/minecraft/world/entity/Entity;FF)V");
const JUMP: Key = key!("net/minecraft/world/entity/ai/control/JumpControl", "jump.()V");
const RESET_X_ROT: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "resetXRotOnTick.()Z");
const CLAMP_HEAD: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "clampHeadRotationToBody.()V");
const Y_ROT_D: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "getYRotD.()Ljava/util/Optional;");
const X_ROT_D: Key = key!("net/minecraft/world/entity/ai/control/LookControl", "getXRotD.()Ljava/util/Optional;");

/// `LivingEntity`'s air: what it holds at most (`getMaxAirSupply`).
const MAX_AIR: i32 = 300;
/// `Mth.equal`'s and `MoveControl`'s smallest distances, as vanilla's floats.
const EQUAL_EPSILON: f64 = 1.0e-5f32 as f64;
const MIN_SPEED_SQR: f64 = 2.500_000_3e-7f32 as f64;

// ---- fields ----

fn int(rt: &Rt, o: V, f: &'static str) -> i32 {
    match rt.get_field(o, f) {
        V::I(i) => i,
        V::J(j) => j as i32,
        V::F(x) => x as i32,
        V::D(d) => d as i32,
        _ => 0,
    }
}

fn long(rt: &Rt, o: V, f: &'static str) -> i64 {
    match rt.get_field(o, f) {
        V::J(j) => j,
        V::I(i) => i64::from(i),
        _ => 0,
    }
}

fn flt(rt: &Rt, o: V, f: &'static str) -> f32 {
    match rt.get_field(o, f) {
        V::F(x) => x,
        V::D(d) => d as f32,
        V::I(i) => i as f32,
        _ => 0.0,
    }
}

fn flt_or(rt: &Rt, o: V, f: &'static str, default: f32) -> f32 {
    match rt.get_field(o, f) {
        V::F(x) => x,
        V::D(d) => d as f32,
        V::I(i) => i as f32,
        _ => default,
    }
}

fn dbl(rt: &Rt, o: V, f: &'static str) -> f64 {
    match rt.get_field(o, f) {
        V::D(d) => d,
        V::F(x) => f64::from(x),
        V::I(i) => f64::from(i),
        _ => 0.0,
    }
}

fn obj(rt: &Rt, o: V, f: &'static str) -> V {
    match rt.get_field(o, f) {
        v @ V::R(_) => v,
        _ => V::NULL,
    }
}

fn truth(rt: &Rt, o: V, f: &'static str) -> bool {
    int(rt, o, f) != 0
}

fn put_bool(rt: &mut Rt, o: V, f: &'static str, b: bool) {
    rt.put_field(o, f, V::I(i32::from(b)));
}

fn put_f(rt: &mut Rt, o: V, f: &'static str, x: f32) {
    rt.put_field(o, f, V::F(x));
}

fn put_i(rt: &mut Rt, o: V, f: &'static str, i: i32) {
    rt.put_field(o, f, V::I(i));
}

/// Whether `e`'s class (a mod's) has its own `short` (`name.desc`).
fn overridden(rt: &Rt, e: V, short: &str) -> bool {
    let class = rt.class_of(e);
    rt.overrides.is_some_and(|o| o(class, short))
}

/// A virtual call into a mod's code, an exception it throws reported and dropped.
fn vcall(rt: &mut Rt, key: Key, args: &[V]) -> V {
    let v = api::virtual_call(rt, key, args);
    rt.report_thrown(key.1);
    v
}

/// [`vcall`] where `e`'s class has its own `key`, else `vanilla`.
fn own_or<T>(rt: &mut Rt, e: V, key: Key, args: &[V], from: impl FnOnce(V) -> T, vanilla: impl FnOnce(&mut Rt) -> T) -> T {
    if overridden(rt, e, key.1) {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(e);
        all.extend_from_slice(args);
        from(vcall(rt, key, &all))
    } else {
        vanilla(rt)
    }
}

/// `Mth.wrapDegrees(float)`.
fn wrap(v: f32) -> f32 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `Mth.wrapDegrees(double)`.
fn wrap_d(v: f64) -> f64 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `Mth.rotateIfNecessary(angle, target, max)`: `angle` kept within `max` degrees of `target`.
fn rotate_if_necessary(angle: f32, target: f32, max: f32) -> f32 {
    let f = wrap(target - angle);
    let f1 = f.clamp(-max, max);
    target - f1
}

/// `Mth.sin`/`Mth.cos` of an angle in degrees, as vanilla's floats.
fn sin_deg(deg: f32) -> f32 {
    crate::api_common::mth_sin(V::F(deg * (std::f32::consts::PI / 180.0)))
}

fn cos_deg(deg: f32) -> f32 {
    crate::api_common::mth_cos(V::F(deg * (std::f32::consts::PI / 180.0)))
}

/// `Mth.atan2(y, x)` in degrees, as a float (`(float)(Mth.atan2(y, x) * 180.0F / (float)Math.PI)`).
fn atan2_deg(y: f64, x: f64) -> f32 {
    (crate::api_common::mth_atan2(y, x) * f64::from(180.0f32) / f64::from(std::f32::consts::PI)) as f32
}

// ---- which entities ----

/// Whether `e` is a living entity of a mod's class: its state is this module's. The game's own
/// living entities, players and the runtime's fake players answer elsewhere.
pub(crate) fn is_mod_living(rt: &Rt, e: V) -> bool {
    matches!(e, V::R(n) if n != 0)
        && rt.instance_of_static(e, LIVING)
        && !rt.instance_of_static(e, PLAYER)
        && !crate::vanilla_entities::is_proxy(rt, e)
        && !crate::fake_players::is_fake(rt, e)
        && !crate::made_entities::is_made(rt, e)
}

fn is_mod_mob(rt: &Rt, e: V) -> bool {
    is_mod_living(rt, e) && rt.instance_of_static(e, MOB)
}

/// Whether `e` is a mob (`instanceof Mob`), a mod's or the game's.
pub(crate) fn is_mob(rt: &Rt, e: V) -> bool {
    matches!(e, V::R(n) if n != 0) && rt.instance_of_static(e, MOB)
}

fn is_player(rt: &Rt, e: V) -> bool {
    matches!(e, V::R(n) if n != 0) && (rt.instance_of_static(e, PLAYER) || crate::api_common::is_player_class(rt.class_of(e)))
}

fn server(rt: &Rt) -> bool {
    !crate::neo::is_client(rt)
}

fn removed(rt: &Rt, e: V) -> bool {
    rt.get_field(e, "removalReason").r() != 0
}

/// The first synched data id vanilla's class `class` leaves its subclasses (`Entity`'s eight,
/// `LivingEntity`'s seven more...), climbing to the nearest class this knows; none past `Entity`.
pub(crate) fn data_start(class: &str) -> Option<i32> {
    let mut c = class;
    loop {
        let start = match c {
            ENTITY => 8,
            LIVING => 15,
            MOB
            | "net/minecraft/world/entity/PathfinderMob"
            | MONSTER
            | WATER_ANIMAL
            | "net/minecraft/world/entity/animal/AbstractGolem"
            | "net/minecraft/world/entity/FlyingMob"
            | "net/minecraft/world/entity/ambient/AmbientCreature" => 16,
            AGEABLE | ANIMAL | "net/minecraft/world/entity/animal/AbstractFish" => 17,
            "net/minecraft/world/entity/animal/horse/AbstractHorse" | "net/minecraft/world/entity/npc/AbstractVillager" => 18,
            "net/minecraft/world/entity/TamableAnimal" | "net/minecraft/world/entity/animal/ShoulderRidingEntity" => 19,
            _ => {
                c = crate::vanilla_entities::api_super(c)?;
                continue;
            }
        };
        return Some(start);
    }
}

// ---- what goals and paths read of a mob, and of the entities round it ----

pub(crate) fn position(rt: &Rt, e: V) -> [f64; 3] {
    if crate::mod_entities::is_mod_entity(rt, e) || is_mod_living(rt, e) {
        return [dbl(rt, e, "$x"), dbl(rt, e, "$y"), dbl(rt, e, "$z")];
    }
    crate::entities::position(rt, e).unwrap_or([dbl(rt, e, "$x"), dbl(rt, e, "$y"), dbl(rt, e, "$z")])
}

pub(crate) fn block_position(rt: &Rt, e: V) -> [i32; 3] {
    position(rt, e).map(|c| c.floor() as i32)
}

fn eye_height(rt: &mut Rt, e: V) -> f64 {
    f64::from(crate::mod_entities::own_size(rt, e)[2])
}

/// `getEyeY()`: a mod's entity's from its size in its pose, any other's as it answers.
pub(crate) fn eye_y(rt: &mut Rt, e: V) -> f64 {
    if crate::mod_entities::is_mod_entity(rt, e) || is_mod_living(rt, e) {
        return dbl(rt, e, "$y") + eye_height(rt, e);
    }
    api::call(rt, GET_EYE_Y, &[e]).d()
}

pub(crate) fn eye_pos(rt: &mut Rt, e: V) -> [f64; 3] {
    let p = position(rt, e);
    [p[0], eye_y(rt, e), p[2]]
}

/// `getBoundingBox()` as min x, y, z, max x, y, z.
pub(crate) fn bounds(rt: &mut Rt, e: V) -> [f64; 6] {
    let own = if crate::mod_entities::is_mod_entity(rt, e) { obj(rt, e, "$bb") } else { V::NULL };
    let bb = if own.r() != 0 { own } else { api::call(rt, GET_BOX, &[e]) };
    crate::shapes::aabb_of(rt, bb)
}

pub(crate) fn distance_sqr(rt: &Rt, a: V, b: V) -> f64 {
    let (p, q) = (position(rt, a), position(rt, b));
    (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)
}

/// `getDeltaMovement()`.
pub(crate) fn motion(rt: &mut Rt, e: V) -> [f64; 3] {
    let m = if crate::mod_entities::is_mod_entity(rt, e) || is_mod_living(rt, e) {
        obj(rt, e, "$motion")
    } else {
        api::call(rt, "net/minecraft/world/entity/Entity.getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;", &[e])
    };
    if m.r() == 0 {
        return [0.0; 3];
    }
    crate::api_common::xyz(rt, m)
}

pub(crate) fn set_motion(rt: &mut Rt, e: V, m: [f64; 3]) {
    let v = crate::api_common::vec3(rt, m[0], m[1], m[2]);
    if crate::mod_entities::is_mod_entity(rt, e) || is_mod_living(rt, e) {
        rt.put_field(e, "$motion", v);
    } else {
        api::call(rt, "net/minecraft/world/entity/Entity.setDeltaMovement.(Lnet/minecraft/world/phys/Vec3;)V", &[e, v]);
    }
}

pub(crate) fn on_ground(rt: &mut Rt, e: V) -> bool {
    if is_mod_living(rt, e) {
        return truth(rt, e, "$onGround");
    }
    api::call(rt, "net/minecraft/world/entity/Entity.onGround.()Z", &[e]).i() != 0
}

pub(crate) fn in_water(rt: &mut Rt, e: V) -> bool {
    if is_mod_living(rt, e) {
        return truth(rt, e, "$inWater");
    }
    api::call(rt, "net/minecraft/world/entity/Entity.isInWater.()Z", &[e]).i() != 0
}

pub(crate) fn in_lava(rt: &mut Rt, e: V) -> bool {
    if is_mod_living(rt, e) {
        return truth(rt, e, "$inLava");
    }
    api::call(rt, "net/minecraft/world/entity/Entity.isInLava.()Z", &[e]).i() != 0
}

/// `getFluidHeight(FluidTags.WATER)` (or lava's): how deep its box is in it, as its last base
/// tick found.
pub(crate) fn fluid_height(rt: &Rt, e: V, water: bool) -> f64 {
    dbl(rt, e, if water { "$waterHeight" } else { "$lavaHeight" })
}

/// `getFluidJumpThreshold()` (virtual): an eye below 0.4 swims up at once, others when deeper.
pub(crate) fn fluid_jump_threshold(rt: &mut Rt, e: V) -> f64 {
    own_or(rt, e, FLUID_JUMP, &[], V::d, |rt| if eye_height(rt, e) < 0.4 { 0.0 } else { 0.4 })
}

pub(crate) fn on_fire(rt: &mut Rt, e: V) -> bool {
    if crate::mod_entities::is_mod_entity(rt, e) {
        return int(rt, e, "$fire") > 0;
    }
    api::call(rt, "net/minecraft/world/entity/Entity.isOnFire.()Z", &[e]).i() != 0
}

/// `e.random`, made when a probe's bare object has none.
fn random(rt: &mut Rt, e: V) -> V {
    let r = obj(rt, e, "random");
    if r.r() != 0 {
        return r;
    }
    let r = api::call(rt, RANDOM_CREATE, &[]);
    rt.put_field(e, "random", r);
    r
}

pub(crate) fn random_float(rt: &mut Rt, e: V) -> f32 {
    let r = random(rt, e);
    api::call(rt, RANDOM_FLOAT, &[r]).f()
}

pub(crate) fn random_int(rt: &mut Rt, e: V, bound: i32) -> i32 {
    let r = random(rt, e);
    api::call(rt, RANDOM_INT, &[r, V::I(bound.max(1))]).i()
}

pub(crate) fn random_double(rt: &mut Rt, e: V) -> f64 {
    let r = random(rt, e);
    api::call(rt, RANDOM_DOUBLE, &[r]).d()
}

pub(crate) fn game_time(rt: &Rt) -> i64 {
    rt.host().map_or(0, |h| h.game_time())
}

/// `AgeableMob.getAge()`: below 0 a baby's ticks to grow up; 0 for a mob that doesn't age.
pub(crate) fn age(rt: &mut Rt, e: V) -> i32 {
    if is_mod_living(rt, e) {
        return int(rt, e, "$age");
    }
    api::call(rt, "net/minecraft/world/entity/AgeableMob.getAge.()I", &[e]).i()
}

pub(crate) fn no_action_time(rt: &Rt, e: V) -> i32 {
    int(rt, e, "noActionTime")
}

pub(crate) fn has_controlling_passenger(rt: &mut Rt, e: V) -> bool {
    api::virtual_call(rt, key!("net/minecraft/world/entity/Entity", "getControllingPassenger.()Lnet/minecraft/world/entity/LivingEntity;"), &[e]).r() != 0
}

pub(crate) fn is_alive(rt: &mut Rt, e: V) -> bool {
    if e.r() == 0 {
        return false;
    }
    if is_mod_living(rt, e) && !overridden(rt, e, IS_ALIVE.1) {
        return living_alive(rt, e);
    }
    api::virtual_call(rt, IS_ALIVE, &[e]).i() != 0
}

/// `LivingEntity.isAlive()`: not removed, with health left.
fn living_alive(rt: &Rt, e: V) -> bool {
    !removed(rt, e) && health(rt, e) > 0.0
}

pub(crate) fn is_spectator(rt: &mut Rt, e: V) -> bool {
    is_player(rt, e) && api::call(rt, "net/minecraft/world/entity/player/Player.isSpectator.()Z", &[e]).i() != 0
}

pub(crate) fn is_creative(rt: &mut Rt, e: V) -> bool {
    is_player(rt, e) && api::call(rt, "net/minecraft/world/entity/player/Player.isCreative.()Z", &[e]).i() != 0
}

/// `Mob.getTarget()` (virtual).
pub(crate) fn target(rt: &mut Rt, mob: V) -> V {
    own_or(rt, mob, GET_TARGET, &[], |v| v, |rt| obj(rt, mob, "$target"))
}

/// `Mob.setTarget(target)` (virtual).
pub(crate) fn set_target(rt: &mut Rt, mob: V, t: V) {
    own_or(rt, mob, SET_TARGET, &[t], |_| (), |rt| rt.put_field(mob, "$target", t));
}

/// `Mob.setAggressive(b)`: a flag a client draws (a zombie's raised arms).
pub(crate) fn set_aggressive(rt: &mut Rt, mob: V, b: bool) {
    put_bool(rt, mob, "$aggressive", b);
}

/// `swing(InteractionHand.MAIN_HAND)` (virtual).
pub(crate) fn swing_main_hand(rt: &mut Rt, mob: V) {
    let hand = crate::api_common::get_static(rt, HAND, "MAIN_HAND").unwrap_or(V::NULL);
    own_or(rt, mob, SWING, &[hand], |_| (), |rt| swing(rt, mob, 0, false));
}

/// `Mob.doHurtTarget(target)` (virtual): whether the blow landed.
pub(crate) fn do_hurt_target(rt: &mut Rt, mob: V, target: V) -> bool {
    own_or(rt, mob, DO_HURT_TARGET, &[target], |v| v.i() != 0, |rt| hurt_target(rt, mob, target))
}

/// `Mob.isWithinMeleeAttackRange(target)` (virtual): its attack box (its box widened by
/// `sqrt(2.04) - 0.6` each way across) meets the target's.
pub(crate) fn within_melee_range(rt: &mut Rt, mob: V, target: V) -> bool {
    own_or(rt, mob, MELEE_RANGE, &[target], |v| v.i() != 0, |rt| {
        let reach = f64::from(2.04f32).sqrt() - f64::from(0.6f32);
        let b = bounds(rt, mob);
        let a = [b[0] - reach, b[1], b[2] - reach, b[3] + reach, b[4], b[5] + reach];
        let t = bounds(rt, target);
        a[0] < t[3] && a[3] > t[0] && a[1] < t[4] && a[4] > t[1] && a[2] < t[5] && a[5] > t[2]
    })
}

/// `getSensing().hasLineOfSight(e)`: once a tick for each entity.
pub(crate) fn sees(rt: &mut Rt, mob: V, e: V) -> bool {
    let mut s = obj(rt, mob, "$sensing");
    if s.r() == 0 {
        s = goals::new_sensing(rt, mob);
        rt.put_field(mob, "$sensing", s);
    }
    goals::sensing_sees(rt, s, e)
}

pub(crate) fn last_hurt_by_mob(rt: &Rt, e: V) -> V {
    obj(rt, e, "$lastHurtBy")
}

pub(crate) fn last_hurt_by_mob_timestamp(rt: &Rt, e: V) -> i32 {
    int(rt, e, "$lastHurtByTimestamp")
}

/// `getLastDamageSource()`: the last blow's, forgotten after two seconds.
pub(crate) fn last_damage_source(rt: &mut Rt, e: V) -> V {
    if game_time(rt) - long(rt, e, "$lastDamageStamp") > 40 {
        rt.put_field(e, "$lastDamageSource", V::NULL);
    }
    obj(rt, e, "$lastDamageSource")
}

/// `a.isAlliedTo(b)` (virtual): a mod's entity is on no team here.
pub(crate) fn is_allied(rt: &mut Rt, a: V, b: V) -> bool {
    if a.r() == 0 || b.r() == 0 {
        return false;
    }
    if is_mod_living(rt, a) && !overridden(rt, a, IS_ALLIED.1) {
        return false;
    }
    api::virtual_call(rt, IS_ALLIED, &[a, b]).i() != 0
}

/// `LivingEntity.canAttack(target)` (virtual): one it may see as an enemy (a player on peaceful
/// too: the runtime doesn't know the difficulty).
pub(crate) fn can_attack(rt: &mut Rt, mob: V, target: V) -> bool {
    if target.r() == 0 {
        return false;
    }
    own_or(rt, mob, CAN_ATTACK, &[target], |v| v.i() != 0, |rt| can_be_seen_as_enemy(rt, target))
}

/// `canBeSeenAsEnemy()`: not invulnerable (a player in creative is) and seen by anyone.
pub(crate) fn can_be_seen_as_enemy(rt: &mut Rt, t: V) -> bool {
    if is_player(rt, t) {
        return !is_creative(rt, t) && can_be_seen_by_anyone(rt, t);
    }
    if is_mod_living(rt, t) {
        return !truth(rt, t, "$invulnerable") && can_be_seen_by_anyone(rt, t);
    }
    api::call(rt, "net/minecraft/world/entity/LivingEntity.canBeSeenAsEnemy.()Z", &[t]).i() != 0
}

/// `canBeSeenByAnyone()`: alive and not a spectator.
pub(crate) fn can_be_seen_by_anyone(rt: &mut Rt, t: V) -> bool {
    !is_spectator(rt, t) && is_alive(rt, t)
}

/// `target.getVisibilityPercent(looker)`: a sneaking player's is 0.8 (invisibility and mob heads
/// aren't modelled).
pub(crate) fn visibility_percent(rt: &mut Rt, target: V, _looker: V) -> f64 {
    if is_player(rt, target) && api::call(rt, "net/minecraft/world/entity/Entity.isDiscrete.()Z", &[target]).i() != 0 {
        return 0.8;
    }
    1.0
}

/// `LivingEntity.hasLineOfSight(e)`: nothing a collider blocks between their eyes, within 128.
pub(crate) fn has_line_of_sight(rt: &mut Rt, mob: V, e: V) -> bool {
    if e.r() == 0 {
        return false;
    }
    let (from, to) = (eye_pos(rt, mob), eye_pos(rt, e));
    let d = ((from[0] - to[0]).powi(2) + (from[1] - to[1]).powi(2) + (from[2] - to[2]).powi(2)).sqrt();
    if d > 128.0 {
        return false;
    }
    let level = obj(rt, mob, "$level");
    // `ClipContext.Block.COLLIDER`, `ClipContext.Fluid.NONE`.
    let hit = crate::shapes::clip_through(rt, level, from, to, 0, 0);
    rt.get_field(hit, "$miss").i() != 0
}

pub(crate) fn navigation(rt: &mut Rt, mob: V) -> V {
    own_or(rt, mob, GET_NAVIGATION, &[], |v| v, |rt| obj(rt, mob, "navigation"))
}

pub(crate) fn move_control(rt: &mut Rt, mob: V) -> V {
    own_or(rt, mob, GET_MOVE_CONTROL, &[], |v| v, |rt| obj(rt, mob, "moveControl"))
}

pub(crate) fn look_control(rt: &mut Rt, mob: V) -> V {
    own_or(rt, mob, GET_LOOK_CONTROL, &[], |v| v, |rt| obj(rt, mob, "lookControl"))
}

pub(crate) fn jump_control(rt: &mut Rt, mob: V) -> V {
    own_or(rt, mob, GET_JUMP_CONTROL, &[], |v| v, |rt| obj(rt, mob, "jumpControl"))
}

/// `jumpControl.jump()` (virtual).
pub(crate) fn jump(rt: &mut Rt, j: V) {
    if j.r() != 0 {
        own_or(rt, j, JUMP, &[], |_| (), |rt| put_bool(rt, j, "jump", true));
    }
}

/// `lookControl.setLookAt(x, y, z)`: at the mob's own head speeds.
pub(crate) fn look_at(rt: &mut Rt, look: V, at: [f64; 3]) {
    if look.r() == 0 {
        return;
    }
    let mob = obj(rt, look, "mob");
    let (yaw, pitch) = (head_rot_speed(rt, mob) as f32, max_head_x_rot(rt, mob) as f32);
    look_at_point(rt, look, at, yaw, pitch);
}

/// `lookControl.setLookAt(x, y, z, yaw speed, pitch limit)` (virtual).
fn look_at_point(rt: &mut Rt, look: V, at: [f64; 3], yaw: f32, pitch: f32) {
    let args = [V::D(at[0]), V::D(at[1]), V::D(at[2]), V::F(yaw), V::F(pitch)];
    own_or(rt, look, LOOK_AT_POINT, &args, |_| (), |rt| set_look_at(rt, look, at, yaw, pitch));
}

/// `lookControl.setLookAt(entity, yaw speed, pitch limit)` (virtual): at a living one's eyes, any
/// other's middle.
pub(crate) fn look_at_entity(rt: &mut Rt, look: V, e: V, yaw: f32, pitch: f32) {
    if look.r() == 0 || e.r() == 0 {
        return;
    }
    own_or(rt, look, LOOK_AT_ENTITY, &[e, V::F(yaw), V::F(pitch)], |_| (), |rt| {
        let at = look_target(rt, e);
        set_look_at(rt, look, at, yaw, pitch);
    });
}

/// Where a mob looks at `e` (`LookControl.getWantedY`): its eyes, or a non-living one's middle.
fn look_target(rt: &mut Rt, e: V) -> [f64; 3] {
    let p = position(rt, e);
    let y = if rt.instance_of_static(e, LIVING) {
        eye_y(rt, e)
    } else {
        let b = bounds(rt, e);
        (b[1] + b[4]) / 2.0
    };
    [p[0], y, p[2]]
}

/// `Mob.getHeadRotSpeed()` (virtual): 10.
pub(crate) fn head_rot_speed(rt: &mut Rt, mob: V) -> i32 {
    own_or(rt, mob, HEAD_ROT_SPEED, &[], V::i, |_| 10)
}

/// `Mob.getMaxHeadXRot()` (virtual): 40.
pub(crate) fn max_head_x_rot(rt: &mut Rt, mob: V) -> i32 {
    own_or(rt, mob, MAX_HEAD_X, &[], V::i, |_| 40)
}

/// `Mob.getMaxHeadYRot()` (virtual): 75.
fn max_head_y_rot(rt: &mut Rt, mob: V) -> i32 {
    own_or(rt, mob, MAX_HEAD_Y, &[], V::i, |_| 75)
}

/// The value of attribute `id` (`minecraft:generic.movement_speed`) of a living entity: a mod's
/// as [`crate::attributes`] keeps it, any other's as it answers.
pub(crate) fn attribute(rt: &mut Rt, e: V, id: &str) -> f64 {
    if is_mod_living(rt, e) {
        return crate::attributes::mob_value(rt, e, id, false);
    }
    let holder = crate::attributes::attribute(rt, id);
    api::call(rt, "net/minecraft/world/entity/LivingEntity.getAttributeValue.(Lnet/minecraft/core/Holder;)D", &[e, holder]).d()
}

pub(crate) fn attribute_base(rt: &mut Rt, e: V, id: &str) -> f64 {
    if is_mod_living(rt, e) {
        return crate::attributes::mob_value(rt, e, id, true);
    }
    let holder = crate::attributes::attribute(rt, id);
    api::call(rt, "net/minecraft/world/entity/LivingEntity.getAttributeBaseValue.(Lnet/minecraft/core/Holder;)D", &[e, holder]).d()
}

/// `maxUpStep()` (virtual): its step height attribute.
pub(crate) fn max_up_step(rt: &mut Rt, e: V) -> f32 {
    own_or(rt, e, MAX_UP_STEP, &[], V::f, |rt| attribute(rt, e, "minecraft:generic.step_height") as f32)
}

/// `getMaxFallDistance()` (virtual): `Mob`'s, 3 blocks without a target, else as far as its
/// health spares (on normal difficulty).
pub(crate) fn max_fall_distance(rt: &mut Rt, mob: V) -> i32 {
    own_or(rt, mob, MAX_FALL, &[], V::i, |rt| {
        if target(rt, mob).r() == 0 {
            return 3;
        }
        let i = (health(rt, mob) - max_health(rt, mob) * 0.33) as i32 - 4;
        i.max(0) + 3
    })
}

/// `moveControl.setWantedPosition(x, y, z, speed)` (virtual).
pub(crate) fn set_wanted_position(rt: &mut Rt, control: V, at: [f64; 3], speed: f64) {
    if control.r() == 0 {
        return;
    }
    let args = [V::D(at[0]), V::D(at[1]), V::D(at[2]), V::D(speed)];
    own_or(rt, control, SET_WANTED, &args, |_| (), |rt| set_wanted(rt, control, at, speed));
}

/// `getSpeed()` (virtual): what its move control last set.
pub(crate) fn speed(rt: &mut Rt, e: V) -> f32 {
    own_or(rt, e, GET_SPEED, &[], V::f, |rt| flt(rt, e, "$speed"))
}

/// `setSpeed(speed)` (virtual): a mob's forward input follows it (`Mob.setSpeed`).
fn set_speed(rt: &mut Rt, e: V, s: f32) {
    own_or(rt, e, SET_SPEED, &[V::F(s)], |_| (), |rt| {
        put_f(rt, e, "$speed", s);
        if rt.instance_of_static(e, MOB) {
            put_f(rt, e, "zza", s);
        }
    });
}

/// `PathfinderMob.getWalkTargetValue(pos, level)` (virtual): how much it likes to walk to `p`.
pub(crate) fn walk_target_value(rt: &mut Rt, mob: V, p: [i32; 3]) -> f32 {
    if overridden(rt, mob, WALK_TARGET.1) {
        let pos = api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]));
        let level = obj(rt, mob, "$level");
        return vcall(rt, WALK_TARGET, &[mob, pos, level]).f();
    }
    walk_target_default(rt, mob, p)
}

/// Vanilla's: an animal's grass below is worth 10, else the light; a monster likes the dark.
fn walk_target_default(rt: &Rt, mob: V, p: [i32; 3]) -> f32 {
    let light = || {
        let (block, sky) = rt.host().map_or((0, 15), |h| h.light(p));
        let f = f32::from(block.max(sky)) / 15.0;
        f / (4.0 - 3.0 * f) - 0.5
    };
    if rt.instance_of_static(mob, ANIMAL) {
        let below = rt.host().map(|h| h.block_state([p[0], p[1] - 1, p[2]])).unwrap_or_default();
        if crate::host::split_state(&below).0 == "minecraft:grass_block" {
            return 10.0;
        }
        return light();
    }
    if rt.instance_of_static(mob, MONSTER) {
        return -light();
    }
    0.0
}

/// Where and how far it keeps to (`restrictTo`); none without a restriction.
pub(crate) fn restriction(rt: &Rt, mob: V) -> Option<([i32; 3], f32)> {
    let r = flt_or(rt, mob, "$restrictRadius", -1.0);
    (r != -1.0).then(|| ([int(rt, mob, "$restrictX"), int(rt, mob, "$restrictY"), int(rt, mob, "$restrictZ")], r))
}

/// `isWithinRestriction(pos)`.
pub(crate) fn within_restriction(rt: &Rt, mob: V, p: [i32; 3]) -> bool {
    match restriction(rt, mob) {
        None => true,
        Some((c, r)) => {
            let d = (0..3).map(|i| f64::from(c[i] - p[i]).powi(2)).sum::<f64>();
            d < f64::from(r * r)
        }
    }
}

pub(crate) fn health(rt: &Rt, e: V) -> f32 {
    flt(rt, e, "$health")
}

fn max_health(rt: &mut Rt, e: V) -> f32 {
    attribute(rt, e, "minecraft:generic.max_health") as f32
}

/// `setHealth(h)`: kept between 0 and its most.
fn set_health(rt: &mut Rt, e: V, h: f32) {
    let max = max_health(rt, e);
    put_f(rt, e, "$health", h.clamp(0.0, max));
}

// ---- construction ----

/// `new LivingEntity(type, level)` and the constructors under it, for a mod's class: the entity's
/// own ([`crate::mod_entities::construct`]), then its health at its most, a random facing and its
/// walk animation; a mob's goal and target selectors, controls, navigation (the class's own
/// `createNavigation`) and senses, then on a server its goals (`registerGoals`); an animal's fear of
/// fire, a monster's experience, a water animal's ease in water.
fn construct(rt: &mut Rt, e: V, ty: V, level: V) {
    crate::mod_entities::construct(rt, e, ty, level);
    init_living(rt, e);
    if rt.instance_of_static(e, MOB) {
        init_mob(rt, e, level);
    }
}

fn init_living(rt: &mut Rt, e: V) {
    for f in [
        "yBodyRot", "yBodyRotO", "yHeadRot", "yHeadRotO", "attackAnim", "oAttackAnim", "xxa", "yya", "zza", "animStep", "animStepO", "run", "oRun",
        "lastHurt", "$speed", "$absorption",
    ] {
        put_f(rt, e, f, 0.0);
    }
    for f in [
        "hurtTime", "hurtDuration", "deathTime", "swingTime", "noJumpDelay", "lerpSteps", "lerpHeadSteps", "noActionTime", "lastHurtByPlayerTime",
        "$lastHurtByTimestamp", "$lastHurtMobTimestamp", "jumping", "swinging", "dead", "$onGround", "$age",
    ] {
        put_i(rt, e, f, 0);
    }
    put_i(rt, e, "$air", MAX_AIR);
    put_f(rt, e, "$restrictRadius", -1.0);
    let walk = rt.new_object(WALK_ANIMATION);
    for f in ["speedOld", "speed", "position"] {
        put_f(rt, walk, f, 0.0);
    }
    rt.put_field(e, "walkAnimation", walk);
    let max = max_health(rt, e);
    put_f(rt, e, "$health", max);
    // `setYRot((float)(Math.random() * (float)(Math.PI * 2)))`: a random facing, in vanilla's own
    // unit slip (a share of a turn read as degrees); the head along.
    let r = random_double(rt, e);
    let yaw = (r * f64::from(std::f32::consts::TAU)) as f32;
    put_f(rt, e, "$yRot", yaw);
    put_f(rt, e, "yHeadRot", yaw);
}

fn init_mob(rt: &mut Rt, e: V, level: V) {
    let sel = goals::new_selector(rt);
    rt.put_field(e, "goalSelector", sel);
    let sel = goals::new_selector(rt);
    rt.put_field(e, "targetSelector", sel);
    for (field, class) in [("lookControl", LOOK_CONTROL), ("moveControl", MOVE_CONTROL), ("jumpControl", JUMP_CONTROL)] {
        let c = new_control(rt, class, e);
        rt.put_field(e, field, c);
    }
    let body = own_or(rt, e, CREATE_BODY, &[], |v| v, |rt| new_control(rt, BODY_CONTROL, e));
    rt.put_field(e, "$bodyControl", body);
    let nav = own_or(rt, e, CREATE_NAVIGATION, &[level], |v| v, |rt| pathing::new_ground(rt, e, level));
    rt.put_field(e, "navigation", nav);
    let sensing = goals::new_sensing(rt, e);
    rt.put_field(e, "$sensing", sensing);
    put_i(rt, e, "xpReward", 0);
    put_i(rt, e, "ambientSoundTime", 0);
    if server(rt) {
        own_or(rt, e, REGISTER_GOALS, &[], |_| (), |_| ());
    }
    if rt.instance_of_static(e, ANIMAL) {
        set_malus(rt, e, "DANGER_FIRE", 16.0);
        set_malus(rt, e, "DAMAGE_FIRE", -1.0);
    }
    if rt.instance_of_static(e, MONSTER) {
        put_i(rt, e, "xpReward", 5);
    }
    if rt.instance_of_static(e, WATER_ANIMAL) {
        set_malus(rt, e, "WATER", 0.0);
    }
}

fn set_malus(rt: &mut Rt, mob: V, kind: &str, value: f32) {
    if let Some(k) = pathing::kind_named(kind) {
        pathing::set_malus(rt, mob, k, value);
    }
}

// ---- the tick ----

/// `Mob.tick()` (with `LivingEntity.tick()` under it) for a mod's living entity: the base tick, the
/// AI step, then its body turned toward where it walks (`tickHeadTurn`: a mob's body control) and
/// its angles kept within a turn of last tick's.
fn tick(rt: &mut Rt, e: V) {
    own_or(rt, e, BASE_TICK, &[], |_| (), |rt| base_tick(rt, e));
    if !removed(rt, e) {
        own_or(rt, e, AI_STEP, &[], |_| (), |rt| ai_step(rt, e));
    }
    let p = position(rt, e);
    let (dx, dz) = (p[0] - dbl(rt, e, "xo"), p[2] - dbl(rt, e, "zo"));
    let f = (dx * dx + dz * dz) as f32;
    let yaw = flt(rt, e, "$yRot");
    let mut body = flt(rt, e, "yBodyRot");
    let mut anim = 0.0f32;
    let run = flt(rt, e, "run");
    put_f(rt, e, "oRun", run);
    let mut moving = 0.0f32;
    if f > 0.002_500_000_2 {
        moving = 1.0;
        anim = f64::from(f).sqrt() as f32 * 3.0;
        let f4 = crate::api_common::mth_atan2(dz, dx) as f32 * (180.0 / std::f32::consts::PI) - 90.0;
        let f5 = (wrap(yaw) - f4).abs();
        body = if 95.0 < f5 && f5 < 265.0 { f4 - 180.0 } else { f4 };
    }
    if flt(rt, e, "attackAnim") > 0.0 {
        body = yaw;
    }
    if !truth(rt, e, "$onGround") {
        moving = 0.0;
    }
    put_f(rt, e, "run", run + (moving - run) * 0.3);
    let anim = head_turn(rt, e, body, anim);
    for (now, old) in [("$yRot", "yRotO"), ("yBodyRot", "yBodyRotO"), ("$xRot", "xRotO"), ("yHeadRot", "yHeadRotO")] {
        let n = flt(rt, e, now);
        let mut o = flt(rt, e, old);
        if !n.is_finite() || !o.is_finite() || (n - o).abs() > 3600.0 {
            o = n;
        }
        while n - o < -180.0 {
            o -= 360.0;
        }
        while n - o >= 180.0 {
            o += 360.0;
        }
        put_f(rt, e, old, o);
    }
    let step = flt(rt, e, "animStep");
    put_f(rt, e, "animStep", step + anim);
    if rt.instance_of_static(e, MOB) && server(rt) && int(rt, e, "tickCount") % 5 == 0 {
        update_control_flags(rt, e);
    }
}

/// `tickHeadTurn(bodyTarget, animStep)`: a mob's body control turns its body; any other living
/// entity's body eases a third of the way toward where it walks, at most 50 degrees from its facing.
fn head_turn(rt: &mut Rt, e: V, target: f32, anim: f32) -> f32 {
    if rt.instance_of_static(e, MOB) {
        let body = obj(rt, e, "$bodyControl");
        if body.r() != 0 {
            own_or(rt, body, BODY_TICK, &[], |_| (), |rt| body_tick(rt, body));
        }
        return anim;
    }
    let body = flt(rt, e, "yBodyRot");
    let mut body = body + wrap(target - body) * 0.3;
    let f1 = wrap(flt(rt, e, "$yRot") - body);
    if f1.abs() > 50.0 {
        body += f1 - f1.signum() * 50.0;
    }
    put_f(rt, e, "yBodyRot", body);
    if f1 < -90.0 || f1 >= 90.0 { -anim } else { anim }
}

/// `Mob.updateControlFlags()`: its goals may move, look and jump unless a mob rides it (or, to
/// jump, it sits in a boat).
fn update_control_flags(rt: &mut Rt, mob: V) {
    let controller = api::virtual_call(rt, key!("net/minecraft/world/entity/Entity", "getControllingPassenger.()Lnet/minecraft/world/entity/LivingEntity;"), &[mob]);
    let free = !is_mob(rt, controller);
    let vehicle = api::call(rt, "net/minecraft/world/entity/Entity.getVehicle.()Lnet/minecraft/world/entity/Entity;", &[mob]);
    let boat = vehicle.r() != 0 && rt.instance_of(vehicle, "net/minecraft/world/entity/vehicle/Boat");
    let sel = obj(rt, mob, "goalSelector");
    if sel.r() == 0 {
        return;
    }
    goals::set_control(rt, sel, goals::MOVE, free);
    goals::set_control(rt, sel, goals::JUMP, free && !boat);
    goals::set_control(rt, sel, goals::LOOK, free);
}

/// The type's `fireImmune()`.
fn fire_immune(rt: &Rt, e: V) -> bool {
    let ty = obj(rt, e, "$type");
    ty.r() != 0 && truth(rt, ty, "$fireImmune")
}

fn type_name(rt: &Rt, e: V) -> String {
    crate::mod_entities::type_id(rt, obj(rt, e, "$type")).unwrap_or("").to_owned()
}

fn type_in_tag(rt: &Rt, e: V, tag: &str) -> bool {
    let name = type_name(rt, e);
    !name.is_empty() && rt.host().is_some_and(|h| h.entity_type_in_tag(&name, tag))
}

/// `Mob.baseTick()` with `LivingEntity.baseTick()`: the entity's own ([`crate::mod_entities`]:
/// fluids, fire, below the world), burning and lava hurting it, suffocating in a wall, its air
/// under water (a water animal's out of it), its hurt and invulnerable times running down, dying,
/// what hurt it forgotten, last tick's angles kept; then a mob's ambient sound, now and then.
fn base_tick(rt: &mut Rt, e: V) {
    let attack = flt(rt, e, "attackAnim");
    put_f(rt, e, "oAttackAnim", attack);
    let air_before = int(rt, e, "$air");
    let fire_before = int(rt, e, "$fire");
    crate::mod_entities::base_tick(rt, e);
    let server = server(rt);
    let (water, lava) = (truth(rt, e, "$inWater"), truth(rt, e, "$inLava"));
    if water {
        put_f(rt, e, "fallDistance", 0.0);
    }
    if server && fire_before > 0 && !fire_immune(rt, e) && fire_before % 20 == 0 && !lava {
        hurt_by_type(rt, e, "minecraft:on_fire", 1.0);
    }
    if lava {
        let fall = flt(rt, e, "fallDistance");
        put_f(rt, e, "fallDistance", fall * 0.5);
        if server && !fire_immune(rt, e) && hurt_by_type(rt, e, "minecraft:lava", 4.0) {
            let pitch = 2.0 + random_float(rt, e) * 0.4;
            play_named(rt, e, "minecraft:entity.generic.burn", 0.4, pitch);
        }
    }
    if fire_immune(rt, e) {
        put_i(rt, e, "$fire", 0);
    }
    if living_alive(rt, e) {
        if server && in_wall(rt, e) {
            hurt_by_type(rt, e, "minecraft:in_wall", 1.0);
        }
        if eye_in_water(rt, e) {
            if !can_breathe(rt, e) {
                let air = int(rt, e, "$air") - 1;
                put_i(rt, e, "$air", air);
                if air == -20 {
                    put_i(rt, e, "$air", 0);
                    hurt_by_type(rt, e, "minecraft:drown", 2.0);
                }
            }
        } else if int(rt, e, "$air") < MAX_AIR {
            let air = (int(rt, e, "$air") + 4).min(MAX_AIR);
            put_i(rt, e, "$air", air);
        }
    }
    for f in ["hurtTime", "invulnerableTime"] {
        let t = int(rt, e, f);
        if t > 0 {
            put_i(rt, e, f, t - 1);
        }
    }
    if health(rt, e) <= 0.0 {
        own_or(rt, e, TICK_DEATH, &[], |_| (), |rt| tick_death(rt, e));
    }
    let by_player = int(rt, e, "lastHurtByPlayerTime");
    if by_player > 0 {
        put_i(rt, e, "lastHurtByPlayerTime", by_player - 1);
    } else {
        rt.put_field(e, "$lastHurtByPlayer", V::NULL);
    }
    let last = obj(rt, e, "$lastHurtMob");
    if last.r() != 0 && !is_alive(rt, last) {
        rt.put_field(e, "$lastHurtMob", V::NULL);
    }
    let by = obj(rt, e, "$lastHurtBy");
    if by.r() != 0 && (!is_alive(rt, by) || int(rt, e, "tickCount") - int(rt, e, "$lastHurtByTimestamp") > 100) {
        rt.put_field(e, "$lastHurtBy", V::NULL);
    }
    for (now, old) in [("animStep", "animStepO"), ("yBodyRot", "yBodyRotO"), ("yHeadRot", "yHeadRotO"), ("$yRot", "yRotO"), ("$xRot", "xRotO")] {
        let v = flt(rt, e, now);
        put_f(rt, e, old, v);
    }
    // `WaterAnimal.handleAirSupply`: out of water its air runs out, in it it is full.
    if rt.instance_of_static(e, WATER_ANIMAL) && living_alive(rt, e) {
        if water {
            put_i(rt, e, "$air", MAX_AIR);
        } else {
            put_i(rt, e, "$air", air_before - 1);
            if air_before - 1 == -20 {
                put_i(rt, e, "$air", 0);
                hurt_by_type(rt, e, "minecraft:drown", 2.0);
            }
        }
    }
    if rt.instance_of_static(e, MOB) && living_alive(rt, e) {
        let t = int(rt, e, "ambientSoundTime");
        put_i(rt, e, "ambientSoundTime", t + 1);
        if random_int(rt, e, 1000) < t {
            let interval = own_or(rt, e, AMBIENT_INTERVAL, &[], V::i, |_| 80);
            put_i(rt, e, "ambientSoundTime", -interval);
            own_or(rt, e, PLAY_AMBIENT, &[], |_| (), |rt| play_ambient(rt, e));
        }
    }
}

/// The block state at `p`, as the host has it.
fn state_at(rt: &Rt, p: [i32; 3]) -> String {
    rt.host().map(|h| h.block_state(p)).unwrap_or_default()
}

fn name_of(state: &str) -> &str {
    crate::host::split_state(state).0
}

/// `isInWall()`: a suffocating block where its eyes are (a box four fifths its width across).
fn in_wall(rt: &mut Rt, e: V) -> bool {
    if truth(rt, e, "noPhysics") {
        return false;
    }
    let half = f64::from(crate::mod_entities::own_size(rt, e)[0] * 0.8) / 2.0;
    let eye = eye_pos(rt, e);
    let b = [eye[0] - half, eye[1] - 5.0e-7, eye[2] - half, eye[0] + half, eye[1] + 5.0e-7, eye[2] + half];
    for x in b[0].floor() as i32..=b[3].floor() as i32 {
        for y in b[1].floor() as i32..=b[4].floor() as i32 {
            for z in b[2].floor() as i32..=b[5].floor() as i32 {
                if suffocating(rt, [x, y, z]) {
                    return true;
                }
            }
        }
    }
    false
}

/// `BlockState.isSuffocating`: a full cube that blocks movement, but glass, leaves and the like
/// (a mod's block that lets light through: `noOcclusion`).
fn suffocating(rt: &Rt, p: [i32; 3]) -> bool {
    let state = state_at(rt, p);
    let name = name_of(&state);
    if matches!(name, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:barrier") || name.contains("glass") || name.ends_with("_leaves") {
        return false;
    }
    if let Some(props) = mod_block_properties(rt, name)
        && truth(rt, props, "$noOcclusion")
    {
        return false;
    }
    let boxes = rt.host().and_then(|h| h.collision_boxes(p)).unwrap_or_default();
    crate::shapes::full_block(&boxes)
}

/// A mod's block's `BlockBehaviour.Properties`.
fn mod_block_properties(rt: &Rt, name: &str) -> Option<V> {
    let b = rt.registered.get(&("block".to_owned(), name.to_owned())).copied()?;
    let props = rt.get_field(b, "$properties");
    (props.r() != 0).then_some(props)
}

/// The fluid at `p` if it is water (`true`) or lava, with its surface's height.
fn fluid_at(rt: &mut Rt, p: [i32; 3]) -> Option<(bool, f64)> {
    let text = state_at(rt, p);
    if matches!(name_of(&text), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "") {
        return None;
    }
    let state = crate::fluids::of_block_state(rt, &text);
    let name = crate::fluids::name_of(rt, rt.get_field(state, "$fluid"));
    let (water, lava) = rt.host().map_or((false, false), |h| (h.fluid_in_tag(&name, "minecraft:water"), h.fluid_in_tag(&name, "minecraft:lava")));
    if !water && !lava {
        return None;
    }
    let at = api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]));
    let level = crate::glue::level(rt);
    let height = api::call(rt, "net/minecraft/world/level/material/FluidState.getHeight.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F", &[state, level, at]).f();
    Some((water, f64::from(p[1] as f32 + height)))
}

/// `isEyeInFluid(FluidTags.WATER)`: water above its eyes (less a ninth), not in a bubble column.
fn eye_in_water(rt: &mut Rt, e: V) -> bool {
    let eye = eye_pos(rt, e);
    let y = eye[1] - f64::from(0.111_111_11f32);
    let p = [eye[0].floor() as i32, y.floor() as i32, eye[2].floor() as i32];
    match fluid_at(rt, p) {
        Some((true, surface)) => surface > y && name_of(&state_at(rt, [p[0], eye[1].floor() as i32, p[2]])) != "minecraft:bubble_column",
        _ => false,
    }
}

/// `canBreatheUnderwater()` (virtual): a water animal can, and a type in
/// `minecraft:can_breathe_under_water`.
fn can_breathe(rt: &mut Rt, e: V) -> bool {
    own_or(rt, e, CAN_BREATHE, &[], |v| v.i() != 0, |rt| rt.instance_of_static(e, WATER_ANIMAL) || type_in_tag(rt, e, "minecraft:can_breathe_under_water"))
}

/// `LivingEntity.tickDeath()`: twenty ticks after dying it leaves in a puff (event 60).
fn tick_death(rt: &mut Rt, e: V) {
    let t = int(rt, e, "deathTime") + 1;
    put_i(rt, e, "deathTime", t);
    if t >= 20 && server(rt) && !removed(rt, e) {
        crate::mod_entities::broadcast_event(rt, e, 60);
        let killed = api::api_enum(rt, REMOVAL, "KILLED", 0);
        vcall(rt, REMOVE, &[e, killed]);
    }
}

/// `isEffectiveAi()`: a server's, unless its AI is off (`NoAI`).
fn effective_ai(rt: &Rt, e: V) -> bool {
    server(rt) && !truth(rt, e, "$noAi")
}

/// `aiStep()` for a mod's living entity: a monster's swing first; the living entity's; then an
/// ageable mob's growing up and an animal's love running out.
fn ai_step(rt: &mut Rt, e: V) {
    if rt.instance_of_static(e, MONSTER) {
        update_swing_time(rt, e);
    }
    living_ai_step(rt, e);
    if rt.instance_of_static(e, AGEABLE) && server(rt) && living_alive(rt, e) {
        let age = int(rt, e, "$age");
        if age != 0 {
            set_age(rt, e, age - age.signum());
        }
    }
    if rt.instance_of_static(e, ANIMAL) {
        if int(rt, e, "$age") != 0 {
            put_i(rt, e, "inLove", 0);
        }
        let love = int(rt, e, "inLove");
        if love > 0 {
            put_i(rt, e, "inLove", love - 1);
        }
    }
}

/// `AgeableMob.setAge(age)`: crossing from baby to grown (or back) tells the mob
/// (`ageBoundaryReached`) and its clients.
fn set_age(rt: &mut Rt, e: V, age: i32) {
    let before = int(rt, e, "$age");
    put_i(rt, e, "$age", age);
    if (before < 0) != (age < 0) {
        own_or(rt, e, AGE_BOUNDARY, &[], |_| (), |_| ());
    }
}

/// `LivingEntity.aiStep()`: a client's copy eases toward the server's place; a server's runs its AI
/// (`serverAiStep`), jumps (or swims up), then moves (`travel`) by what its controls asked.
fn living_ai_step(rt: &mut Rt, e: V) {
    let delay = int(rt, e, "noJumpDelay");
    if delay > 0 {
        put_i(rt, e, "noJumpDelay", delay - 1);
    }
    let effective = effective_ai(rt, e);
    let steps = int(rt, e, "lerpSteps");
    if steps > 0 {
        lerp_step(rt, e, steps);
        put_i(rt, e, "lerpSteps", steps - 1);
    } else if !effective {
        let m = motion(rt, e);
        set_motion(rt, e, m.map(|c| c * 0.98));
    }
    let head_steps = int(rt, e, "lerpHeadSteps");
    if head_steps > 0 {
        let head = f64::from(flt(rt, e, "yHeadRot"));
        let to = dbl(rt, e, "lerpYHeadRot");
        put_f(rt, e, "yHeadRot", (head + wrap_d(to - head) / f64::from(head_steps)) as f32);
        put_i(rt, e, "lerpHeadSteps", head_steps - 1);
    }
    let m = motion(rt, e);
    let small = m.map(|c| if c.abs() < 0.003 { 0.0 } else { c });
    if small != m {
        set_motion(rt, e, small);
    }
    if health(rt, e) <= 0.0 {
        put_bool(rt, e, "jumping", false);
        put_f(rt, e, "xxa", 0.0);
        put_f(rt, e, "zza", 0.0);
    } else if effective && rt.instance_of_static(e, MOB) {
        server_ai_step(rt, e);
    }
    if truth(rt, e, "jumping") {
        let (water, lava) = (truth(rt, e, "$inWater"), truth(rt, e, "$inLava"));
        let depth = if lava { fluid_height(rt, e, false) } else { fluid_height(rt, e, true) };
        let swimming = water && depth > 0.0;
        let threshold = fluid_jump_threshold(rt, e);
        let ground = truth(rt, e, "$onGround");
        if swimming && !(ground && depth <= threshold) || lava && !(ground && depth <= threshold) {
            let m = motion(rt, e);
            let up = f64::from(0.04f32) * attribute(rt, e, "neoforge:swim_speed");
            set_motion(rt, e, [m[0], m[1] + up, m[2]]);
        } else if (ground || swimming && depth <= threshold) && int(rt, e, "noJumpDelay") == 0 {
            own_or(rt, e, JUMP_FROM_GROUND, &[], |_| (), |rt| jump_from_ground(rt, e));
            put_i(rt, e, "noJumpDelay", 10);
        }
    } else {
        put_i(rt, e, "noJumpDelay", 0);
    }
    let (xxa, zza) = (flt(rt, e, "xxa") * 0.98, flt(rt, e, "zza") * 0.98);
    put_f(rt, e, "xxa", xxa);
    put_f(rt, e, "zza", zza);
    let input = [f64::from(xxa), f64::from(flt(rt, e, "yya")), f64::from(zza)];
    if overridden(rt, e, TRAVEL.1) {
        let v = crate::api_common::vec3(rt, input[0], input[1], input[2]);
        vcall(rt, TRAVEL, &[e, v]);
    } else {
        travel(rt, e, input);
    }
}

/// `Mob.serverAiStep()` (final): its senses refreshed, the target and goal selectors (in full every
/// other tick, else only the running goals), its path, the mod's own step (`customServerAiStep`),
/// then its move, look and jump controls.
fn server_ai_step(rt: &mut Rt, mob: V) {
    let idle = int(rt, mob, "noActionTime");
    put_i(rt, mob, "noActionTime", idle + 1);
    let sensing = obj(rt, mob, "$sensing");
    if sensing.r() != 0 {
        goals::sensing_clear(rt, sensing);
    }
    let tick = int(rt, mob, "tickCount");
    let id = int(rt, mob, "$id");
    let partial = tick.wrapping_add(id) % 2 != 0 && tick > 1;
    for field in ["targetSelector", "goalSelector"] {
        let sel = obj(rt, mob, field);
        if sel.r() == 0 {
            continue;
        }
        if partial {
            goals::tick_running(rt, sel, false);
        } else {
            goals::selector_tick(rt, sel);
        }
    }
    let nav = obj(rt, mob, "navigation");
    if nav.r() != 0 {
        own_or(rt, nav, NAV_TICK, &[], |_| (), |rt| pathing::tick(rt, nav));
    }
    own_or(rt, mob, CUSTOM_AI, &[], |_| (), |_| ());
    let mv = obj(rt, mob, "moveControl");
    if mv.r() != 0 {
        own_or(rt, mv, MOVE_TICK, &[], |_| (), |rt| move_tick(rt, mv));
    }
    let look = obj(rt, mob, "lookControl");
    if look.r() != 0 {
        own_or(rt, look, LOOK_TICK, &[], |_| (), |rt| look_tick(rt, look));
    }
    let jump = obj(rt, mob, "jumpControl");
    if jump.r() != 0 {
        own_or(rt, jump, JUMP_TICK, &[], |_| (), |rt| {
            let j = truth(rt, jump, "jump");
            let mob = obj(rt, jump, "mob");
            put_bool(rt, mob, "jumping", j);
            put_bool(rt, jump, "jump", false);
        });
    }
}

/// `jumpFromGround()`: up at its jump power, which the block under it may lessen (honey).
fn jump_from_ground(rt: &mut Rt, e: V) {
    let power = own_or(rt, e, JUMP_POWER, &[], V::f, |rt| jump_power(rt, e));
    if power <= 1.0e-5 {
        return;
    }
    let m = motion(rt, e);
    set_motion(rt, e, [m[0], f64::from(power), m[2]]);
    put_bool(rt, e, "hasImpulse", true);
}

/// `getJumpPower()`: its jump strength attribute times the block's jump factor.
fn jump_power(rt: &mut Rt, e: V) -> f32 {
    attribute(rt, e, "minecraft:generic.jump_strength") as f32 * block_jump_factor(rt, e)
}

/// `getGravity()`: none without gravity, else its class's (`getDefaultGravity`: its gravity
/// attribute).
fn gravity(rt: &mut Rt, e: V) -> f64 {
    if own_or(rt, e, NO_GRAVITY, &[], |v| v.i() != 0, |rt| truth(rt, e, "$noGravity")) {
        return 0.0;
    }
    own_or(rt, e, DEFAULT_GRAVITY, &[], V::d, |rt| attribute(rt, e, "minecraft:generic.gravity"))
}

/// `LivingEntity.travel(input)` for one this side moves: swimming in water, wading in lava, else
/// walking on what is under it (its friction) or falling through the air; then its walk animation.
fn travel(rt: &mut Rt, e: V, input: [f64; 3]) {
    let flying = rt.instance_of_static(e, FLYING_ANIMAL);
    if effective_ai(rt, e) {
        let gravity = gravity(rt, e);
        let falling = motion(rt, e)[1] <= 0.0;
        if truth(rt, e, "$inWater") {
            let y0 = position(rt, e)[1];
            let mut f4 = own_or(rt, e, key!("net/minecraft/world/entity/LivingEntity", "getWaterSlowDown.()F"), &[], V::f, |_| 0.8);
            let mut f5 = 0.02f32;
            let mut f6 = attribute(rt, e, "minecraft:generic.water_movement_efficiency") as f32;
            if !truth(rt, e, "$onGround") {
                f6 *= 0.5;
            }
            if f6 > 0.0 {
                f4 += (0.546_000_06 - f4) * f6;
                f5 += (speed(rt, e) - f5) * f6;
            }
            f5 *= attribute(rt, e, "neoforge:swim_speed") as f32;
            move_relative(rt, e, f5, input);
            let m = motion(rt, e);
            move_self(rt, e, m);
            let mut v = motion(rt, e);
            if truth(rt, e, "horizontalCollision") && on_climbable(rt, e) {
                v[1] = 0.2;
            }
            let v = fluid_falling(gravity, falling, [v[0] * f64::from(f4), v[1] * f64::from(0.8f32), v[2] * f64::from(f4)]);
            set_motion(rt, e, v);
            if truth(rt, e, "horizontalCollision") && is_free(rt, e, [v[0], v[1] + f64::from(0.6f32) - position(rt, e)[1] + y0, v[2]]) {
                set_motion(rt, e, [v[0], f64::from(0.3f32), v[2]]);
            }
        } else if truth(rt, e, "$inLava") {
            let y0 = position(rt, e)[1];
            move_relative(rt, e, 0.02, input);
            let m = motion(rt, e);
            move_self(rt, e, m);
            let mut v = motion(rt, e);
            if fluid_height(rt, e, false) <= fluid_jump_threshold(rt, e) {
                v = fluid_falling(gravity, falling, [v[0] * 0.5, v[1] * f64::from(0.8f32), v[2] * 0.5]);
            } else {
                v = v.map(|c| c * 0.5);
            }
            if gravity != 0.0 {
                v[1] -= gravity / 4.0;
            }
            set_motion(rt, e, v);
            if truth(rt, e, "horizontalCollision") && is_free(rt, e, [v[0], v[1] + f64::from(0.6f32) - position(rt, e)[1] + y0, v[2]]) {
                set_motion(rt, e, [v[0], f64::from(0.3f32), v[2]]);
            }
        } else {
            let p = position(rt, e);
            let below = [p[0].floor() as i32, (p[1] - 0.500_001).floor() as i32, p[2].floor() as i32];
            let f2 = block_friction(rt, below);
            let ground = truth(rt, e, "$onGround");
            let f3 = if ground { f2 * 0.91 } else { 0.91 };
            let s = if ground { speed(rt, e) * (0.216_000_02 / (f2 * f2 * f2)) } else { 0.02 };
            move_relative(rt, e, s, input);
            let m = motion(rt, e);
            let m = climbing(rt, e, m);
            set_motion(rt, e, m);
            move_self(rt, e, m);
            let mut v = motion(rt, e);
            if (truth(rt, e, "horizontalCollision") || truth(rt, e, "jumping")) && on_climbable(rt, e) {
                v[1] = 0.2;
            }
            let y = v[1] - gravity;
            let f3 = f64::from(f3);
            set_motion(rt, e, [v[0] * f3, if flying { y * f3 } else { y * f64::from(0.98f32) }, v[2] * f3]);
        }
    }
    calculate_entity_animation(rt, e, flying);
}

/// `getFluidFallingAdjustedMovement(gravity, falling, motion)`: sinking slowly in a fluid.
fn fluid_falling(gravity: f64, falling: bool, v: [f64; 3]) -> [f64; 3] {
    if gravity == 0.0 {
        return v;
    }
    let y = if falling && (v[1] - 0.005).abs() >= 0.003 && (v[1] - gravity / 16.0).abs() < 0.003 { -0.003 } else { v[1] - gravity / 16.0 };
    [v[0], y, v[2]]
}

/// `moveRelative(amount, input)`: the input (no longer than 1) scaled and turned to its facing,
/// added to its motion.
fn move_relative(rt: &mut Rt, e: V, amount: f32, input: [f64; 3]) {
    let len2 = input[0] * input[0] + input[1] * input[1] + input[2] * input[2];
    if len2 < 1.0e-7 {
        return;
    }
    let v = if len2 > 1.0 {
        let l = len2.sqrt();
        if l < 1.0e-5f32 as f64 { [0.0; 3] } else { input.map(|c| c / l) }
    } else {
        input
    };
    let v = v.map(|c| c * f64::from(amount));
    let yaw = flt(rt, e, "$yRot");
    let (s, c) = (f64::from(sin_deg(yaw)), f64::from(cos_deg(yaw)));
    let m = motion(rt, e);
    set_motion(rt, e, [m[0] + (v[0] * c - v[2] * s), m[1] + v[1], m[2] + (v[2] * c + v[0] * s)]);
}

/// `onClimbable()`: in a block of `minecraft:climbable` (a ladder, vines).
fn on_climbable(rt: &mut Rt, e: V) -> bool {
    let p = block_position(rt, e);
    let state = state_at(rt, p);
    let name = name_of(&state);
    !name.is_empty() && rt.host().is_some_and(|h| h.block_in_tag(name, "minecraft:climbable"))
}

/// `handleOnClimbable(motion)`: on a ladder it moves slowly and falls no faster than 0.15.
fn climbing(rt: &mut Rt, e: V, m: [f64; 3]) -> [f64; 3] {
    if !on_climbable(rt, e) {
        return m;
    }
    put_f(rt, e, "fallDistance", 0.0);
    let limit = f64::from(0.15f32);
    [m[0].clamp(-limit, limit), m[1].max(-limit), m[2].clamp(-limit, limit)]
}

/// `isFree(dx, dy, dz)`: its box moved by that meets no block and no fluid.
fn is_free(rt: &mut Rt, e: V, d: [f64; 3]) -> bool {
    let b = bounds(rt, e);
    let b = [b[0] + d[0], b[1] + d[1], b[2] + d[2], b[3] + d[0], b[4] + d[1], b[5] + d[2]];
    let shapes = crate::particle_engine::shapes_in(rt, b);
    if shapes.iter().any(|s| s[0] < b[3] && s[3] > b[0] && s[1] < b[4] && s[4] > b[1] && s[2] < b[5] && s[5] > b[2]) {
        return false;
    }
    for x in b[0].floor() as i32..b[3].ceil() as i32 {
        for y in b[1].floor() as i32..b[4].ceil() as i32 {
            for z in b[2].floor() as i32..b[5].ceil() as i32 {
                if fluid_at(rt, [x, y, z]).is_some() {
                    return false;
                }
            }
        }
    }
    true
}

/// `calculateEntityAnimation(includeHeight)`: its walk animation sped by how far it moved.
fn calculate_entity_animation(rt: &mut Rt, e: V, height: bool) {
    let p = position(rt, e);
    let d = [p[0] - dbl(rt, e, "xo"), if height { p[1] - dbl(rt, e, "yo") } else { 0.0 }, p[2] - dbl(rt, e, "zo")];
    let f = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32;
    let walk = obj(rt, e, "walkAnimation");
    if walk.r() != 0 {
        walk_update(rt, walk, (f * 4.0).min(1.0), 0.4);
    }
}

/// `WalkAnimationState.update(speed, partial)`.
fn walk_update(rt: &mut Rt, w: V, to: f32, partial: f32) {
    let speed = flt(rt, w, "speed");
    put_f(rt, w, "speedOld", speed);
    let s = speed + (to - speed) * partial;
    put_f(rt, w, "speed", s);
    let p = flt(rt, w, "position");
    put_f(rt, w, "position", p + s);
}

// ---- moving ----

/// `Entity.move(MoverType.SELF, d)` for a mod's living entity: `d` cut short by the blocks in the
/// way (stepping up what is no higher than its step height), its place moved, which way it collided
/// and whether it stands on something, its fall measured (and hurt on landing), what it fell on
/// bouncing it, steps sounded, and the block it is in slowing it (soul sand, honey).
fn move_self(rt: &mut Rt, e: V, d: [f64; 3]) {
    if truth(rt, e, "noPhysics") {
        let p = position(rt, e);
        crate::mod_entities::set_pos(rt, e, [p[0] + d[0], p[1] + d[1], p[2] + d[2]]);
        return;
    }
    let b = bounds(rt, e);
    let moved = collide(rt, e, b, d);
    if moved[0] * moved[0] + moved[1] * moved[1] + moved[2] * moved[2] > 1.0e-7 {
        let p = position(rt, e);
        crate::mod_entities::set_pos(rt, e, [p[0] + moved[0], p[1] + moved[1], p[2] + moved[2]]);
    }
    let (hx, hz) = ((d[0] - moved[0]).abs() >= EQUAL_EPSILON, (d[2] - moved[2]).abs() >= EQUAL_EPSILON);
    put_bool(rt, e, "horizontalCollision", hx || hz);
    let vertical = d[1] != moved[1];
    put_bool(rt, e, "verticalCollision", vertical);
    let below = vertical && d[1] < 0.0;
    put_bool(rt, e, "verticalCollisionBelow", below);
    put_bool(rt, e, "$onGround", below);
    let p = position(rt, e);
    let on = [p[0].floor() as i32, (p[1] - f64::from(0.2f32)).floor() as i32, p[2].floor() as i32];
    let on_state = state_at(rt, on);
    check_fall_damage(rt, e, moved[1], below, &on_state);
    if removed(rt, e) {
        return;
    }
    if hx || hz {
        let m = motion(rt, e);
        set_motion(rt, e, [if hx { 0.0 } else { m[0] }, m[1], if hz { 0.0 } else { m[2] }]);
    }
    if vertical {
        after_fall_on(rt, e, name_of(&on_state));
    }
    steps(rt, e, moved, below);
    let f = f64::from(block_speed_factor(rt, e));
    if f != 1.0 {
        let m = motion(rt, e);
        set_motion(rt, e, [m[0] * f, m[1], m[2] * f]);
    }
}

/// `Entity.collide(d)`: how far its box `b` goes along `d` before blocks stop it; where they stop it
/// across and it stands (or lands), up a step instead if that takes it further.
fn collide(rt: &mut Rt, e: V, b: [f64; 6], d: [f64; 3]) -> [f64; 3] {
    if d == [0.0; 3] {
        return d;
    }
    let shapes = crate::particle_engine::shapes_in(rt, expand(b, d));
    let v = crate::particle_engine::collide_with(b, d, &shapes);
    let (cx, cy, cz) = (d[0] != v[0], d[1] != v[1], d[2] != v[2]);
    let down = cy && d[1] < 0.0;
    if !(cx || cz) || !(down || truth(rt, e, "$onGround")) {
        return v;
    }
    let step = max_up_step(rt, e);
    if step <= 0.0 {
        return v;
    }
    let from = if down { [b[0], b[1] + v[1], b[2], b[3], b[4] + v[1], b[5]] } else { b };
    let mut area = expand(from, [d[0], f64::from(step), d[2]]);
    if !down {
        area = expand(area, [0.0, f64::from(-1.0e-5f32), 0.0]);
    }
    let around = crate::particle_engine::shapes_in(rt, area);
    let landed = v[1] as f32;
    let mut heights: Vec<f32> = Vec::new();
    for s in &around {
        for y in [s[1], s[4]] {
            let h = (y - from[1]) as f32;
            if h >= 0.0 && h != landed && h <= step && !heights.contains(&h) {
                heights.push(h);
            }
        }
    }
    heights.sort_by(f32::total_cmp);
    for h in heights {
        let w = crate::particle_engine::collide_with(from, [d[0], f64::from(h), d[2]], &around);
        if w[0] * w[0] + w[2] * w[2] > v[0] * v[0] + v[2] * v[2] {
            return [w[0], w[1] - (b[1] - from[1]), w[2]];
        }
    }
    v
}

/// `AABB.expandTowards(d)`.
fn expand(b: [f64; 6], d: [f64; 3]) -> [f64; 6] {
    let mut out = b;
    for i in 0..3 {
        if d[i] < 0.0 {
            out[i] += d[i];
        } else {
            out[i + 3] += d[i];
        }
    }
    out
}

/// `LivingEntity.checkFallDamage`: landing, it takes what it fell (`Block.fallOn`: the block may
/// soften it); falling, the fall grows.
fn check_fall_damage(rt: &mut Rt, e: V, dy: f64, ground: bool, on: &str) {
    let fall = flt(rt, e, "fallDistance");
    if ground {
        if fall > 0.0 && server(rt) {
            let multiplier = fall_multiplier(rt, e, name_of(on));
            if multiplier > 0.0 {
                let source = damage_source(rt, "minecraft:fall", V::NULL);
                let args = [V::F(fall), V::F(multiplier), source];
                own_or(rt, e, CAUSE_FALL, &args, |v| v.i() != 0, |rt| cause_fall_damage(rt, e, fall, multiplier, source));
            }
        }
        put_f(rt, e, "fallDistance", 0.0);
    } else if dy < 0.0 {
        put_f(rt, e, "fallDistance", fall - dy as f32);
    }
}

/// What a block's `fallOn` multiplies a fall's damage by: hay and honey a fifth, a bed half, slime
/// none (unless it sneaks).
fn fall_multiplier(rt: &mut Rt, e: V, block: &str) -> f32 {
    match block {
        "minecraft:hay_block" | "minecraft:honey_block" => 0.2,
        b if b.ends_with("_bed") => 0.5,
        "minecraft:slime_block" if !suppressing_bounce(rt, e) => 0.0,
        _ => 1.0,
    }
}

fn suppressing_bounce(rt: &mut Rt, e: V) -> bool {
    api::call(rt, "net/minecraft/world/entity/Entity.isSuppressingBounce.()Z", &[e]).i() != 0
}

/// `LivingEntity.causeFallDamage(distance, multiplier, source)`: past its safe fall distance, hurt
/// by the rest (by its fall damage multiplier), with the small or big fall sound.
fn cause_fall_damage(rt: &mut Rt, e: V, fall: f32, multiplier: f32, source: V) -> bool {
    if type_in_tag(rt, e, "minecraft:fall_damage_immune") {
        return false;
    }
    let safe = attribute(rt, e, "minecraft:generic.safe_fall_distance") as f32;
    let scaled = f64::from((fall - safe) * multiplier) * attribute(rt, e, "minecraft:generic.fall_damage_multiplier");
    let n = scaled.ceil() as i32;
    if n <= 0 {
        return false;
    }
    let sound = if n > 4 { "minecraft:entity.generic.big_fall" } else { "minecraft:entity.generic.small_fall" };
    play_named(rt, e, sound, 1.0, 1.0);
    hurt(rt, e, source, n as f32);
    true
}

/// `Block.updateEntityAfterFallOn`: slime bounces it back up, a bed two thirds, anything else stops
/// its fall.
fn after_fall_on(rt: &mut Rt, e: V, block: &str) {
    let m = motion(rt, e);
    let y = match block {
        "minecraft:slime_block" if !suppressing_bounce(rt, e) => {
            if m[1] < 0.0 {
                -m[1]
            } else {
                m[1]
            }
        }
        b if b.ends_with("_bed") && !suppressing_bounce(rt, e) => {
            if m[1] < 0.0 {
                -m[1] * f64::from(0.66f32)
            } else {
                m[1]
            }
        }
        _ => 0.0,
    };
    if y != m[1] {
        set_motion(rt, e, [m[0], y, m[2]]);
    }
}

/// Its steps: every block walked, the block under it sounds (`playStepSound`, the mod's own).
fn steps(rt: &mut Rt, e: V, moved: [f64; 3], ground: bool) {
    let walked = flt(rt, e, "moveDist") + ((moved[0] * moved[0] + moved[2] * moved[2]).sqrt() as f32) * 0.6;
    put_f(rt, e, "moveDist", walked);
    let next = flt(rt, e, "nextStep");
    if walked <= next || !ground {
        return;
    }
    let p = position(rt, e);
    let at = [p[0].floor() as i32, (p[1] - 1.0e-5).floor() as i32, p[2].floor() as i32];
    let state = state_at(rt, at);
    if matches!(name_of(&state), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "") {
        return;
    }
    put_f(rt, e, "nextStep", (walked as i32 + 1) as f32);
    if overridden(rt, e, STEP_SOUND.1) {
        let pos = api::block_pos(rt, i64::from(at[0]), i64::from(at[1]), i64::from(at[2]));
        let block_state = crate::api_common::state(rt, &state);
        vcall(rt, STEP_SOUND, &[e, pos, block_state]);
    }
}

/// A vanilla block's friction (ice, slime), else a mod block's own (`Properties.friction`), else 0.6.
fn block_friction(rt: &Rt, p: [i32; 3]) -> f32 {
    let state = state_at(rt, p);
    let name = name_of(&state);
    match name {
        "minecraft:ice" | "minecraft:packed_ice" | "minecraft:frosted_ice" => 0.98,
        "minecraft:blue_ice" => 0.989,
        "minecraft:slime_block" => 0.8,
        _ => mod_block_properties(rt, name).map_or(0.6, |props| flt_or(rt, props, "$friction", 0.6)),
    }
}

/// A block's speed factor (soul sand and honey slow), a mod block's own.
fn speed_factor(rt: &Rt, name: &str) -> f32 {
    match name {
        "minecraft:soul_sand" | "minecraft:honey_block" => 0.4,
        _ => mod_block_properties(rt, name).map_or(1.0, |props| flt_or(rt, props, "$speedFactor", 1.0)),
    }
}

/// `getBlockSpeedFactor()`: the block it is in, else (where that one doesn't slow and isn't water)
/// the one under it.
fn block_speed_factor(rt: &mut Rt, e: V) -> f32 {
    let p = position(rt, e);
    let feet = [p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32];
    let state = state_at(rt, feet);
    let f = speed_factor(rt, name_of(&state));
    if matches!(name_of(&state), "minecraft:water" | "minecraft:bubble_column") || f != 1.0 {
        return f;
    }
    let below = state_at(rt, [feet[0], (p[1] - 0.500_001).floor() as i32, feet[2]]);
    speed_factor(rt, name_of(&below))
}

/// `getBlockJumpFactor()`: honey halves a jump.
fn block_jump_factor(rt: &mut Rt, e: V) -> f32 {
    let factor = |rt: &Rt, name: &str| match name {
        "minecraft:honey_block" => 0.5,
        _ => mod_block_properties(rt, name).map_or(1.0, |props| flt_or(rt, props, "$jumpFactor", 1.0)),
    };
    let p = position(rt, e);
    let feet = state_at(rt, [p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32]);
    let f = factor(rt, name_of(&feet));
    if f != 1.0 {
        return f;
    }
    let below = state_at(rt, [p[0].floor() as i32, (p[1] - 0.500_001).floor() as i32, p[2].floor() as i32]);
    factor(rt, name_of(&below))
}

// ---- controls ----

/// A control of `class` for `mob` (`new MoveControl(mob)`...), as vanilla's constructor leaves it.
fn new_control(rt: &mut Rt, class: &'static str, mob: V) -> V {
    let c = rt.new_object(class);
    init_control(rt, c, class, mob);
    c
}

fn operation(rt: &mut Rt, i: usize) -> V {
    api::api_enum(rt, OPERATION, OPERATIONS[i], i as i32)
}

fn operation_of(rt: &Rt, c: V) -> usize {
    let op = rt.get_field(c, "operation");
    if op.r() == 0 {
        return 0;
    }
    (rt.get_field(op, "$ordinal").i().clamp(0, 3)) as usize
}

fn set_operation(rt: &mut Rt, c: V, i: usize) {
    let op = operation(rt, i);
    rt.put_field(c, "operation", op);
}

/// The vanilla control `class` is or extends: its own, or the one it climbs to.
fn control_kind(class: &str) -> Option<&'static str> {
    let mut c = class;
    loop {
        if let Some(k) = [MOVE_CONTROL, LOOK_CONTROL, JUMP_CONTROL, BODY_CONTROL].into_iter().find(|k| *k == c) {
            return Some(k);
        }
        c = api_super(c)?;
    }
}

fn init_control(rt: &mut Rt, c: V, kind: &str, mob: V) {
    rt.put_field(c, "mob", mob);
    match kind {
        MOVE_CONTROL => {
            set_operation(rt, c, 0);
            for f in ["wantedX", "wantedY", "wantedZ", "speedModifier"] {
                rt.put_field(c, f, V::D(0.0));
            }
            put_f(rt, c, "strafeForwards", 0.0);
            put_f(rt, c, "strafeRight", 0.0);
        }
        LOOK_CONTROL => {
            for f in ["wantedX", "wantedY", "wantedZ"] {
                rt.put_field(c, f, V::D(0.0));
            }
            put_f(rt, c, "yMaxRotSpeed", 0.0);
            put_f(rt, c, "xMaxRotAngle", 0.0);
            put_i(rt, c, "lookAtCooldown", 0);
        }
        JUMP_CONTROL => put_bool(rt, c, "jump", false),
        _ => {
            put_i(rt, c, "headStableTime", 0);
            put_f(rt, c, "lastStableYHeadRot", 0.0);
        }
    }
}

/// `MoveControl.setWantedPosition(x, y, z, speed)`: where to go next, and how fast; a jump under way
/// runs on.
fn set_wanted(rt: &mut Rt, c: V, at: [f64; 3], speed: f64) {
    for (f, v) in ["wantedX", "wantedY", "wantedZ"].into_iter().zip(at) {
        rt.put_field(c, f, V::D(v));
    }
    rt.put_field(c, "speedModifier", V::D(speed));
    if operation_of(rt, c) != 3 {
        set_operation(rt, c, 1);
    }
}

/// `MoveControl.rotlerp(from, to, max)`: at most `max` degrees toward `to`, kept in 0 to 360.
fn rotlerp(from: f32, to: f32, max: f32) -> f32 {
    let f = wrap(to - from).clamp(-max, max);
    let mut r = from + f;
    if r < 0.0 {
        r += 360.0;
    } else if r > 360.0 {
        r -= 360.0;
    }
    r
}

/// `MoveControl.tick()`: strafing, or turning toward where it wants to be and walking there (with a
/// jump where a block stands in the way or the place is above its step), or waiting.
fn move_tick(rt: &mut Rt, c: V) {
    let mob = obj(rt, c, "mob");
    if mob.r() == 0 {
        return;
    }
    let base_speed = |rt: &mut Rt| attribute(rt, mob, "minecraft:generic.movement_speed");
    match operation_of(rt, c) {
        2 => {
            let f1 = (dbl(rt, c, "speedModifier") as f32) * base_speed(rt) as f32;
            let (mut fwd, mut right) = (flt(rt, c, "strafeForwards"), flt(rt, c, "strafeRight"));
            let mut f4 = (fwd * fwd + right * right).sqrt();
            if f4 < 1.0 {
                f4 = 1.0;
            }
            f4 = f1 / f4;
            fwd *= f4;
            right *= f4;
            let yaw = flt(rt, mob, "$yRot");
            let (s, co) = (sin_deg(yaw), cos_deg(yaw));
            let (dx, dz) = (fwd * co - right * s, right * co + fwd * s);
            if !walkable(rt, mob, dx, dz) {
                put_f(rt, c, "strafeForwards", 1.0);
                put_f(rt, c, "strafeRight", 0.0);
            }
            set_speed(rt, mob, f1);
            let (fwd, right) = (flt(rt, c, "strafeForwards"), flt(rt, c, "strafeRight"));
            put_f(rt, mob, "zza", fwd);
            put_f(rt, mob, "xxa", right);
            set_operation(rt, c, 0);
        }
        1 => {
            set_operation(rt, c, 0);
            let p = position(rt, mob);
            let (dx, dz, dy) = (dbl(rt, c, "wantedX") - p[0], dbl(rt, c, "wantedZ") - p[2], dbl(rt, c, "wantedY") - p[1]);
            if dx * dx + dy * dy + dz * dz < MIN_SPEED_SQR {
                put_f(rt, mob, "zza", 0.0);
                return;
            }
            let f9 = atan2_deg(dz, dx) - 90.0;
            let yaw = flt(rt, mob, "$yRot");
            put_f(rt, mob, "$yRot", rotlerp(yaw, f9, 90.0));
            let s = (dbl(rt, c, "speedModifier") * base_speed(rt)) as f32;
            set_speed(rt, mob, s);
            let at = block_position(rt, mob);
            let boxes = rt.host().and_then(|h| h.collision_boxes(at)).unwrap_or_default();
            let top = boxes.iter().map(|b| b[4]).reduce(f64::max);
            let name = name_of(&state_at(rt, at)).to_owned();
            let in_door_or_fence = rt.host().is_some_and(|h| h.block_in_tag(&name, "minecraft:doors") || h.block_in_tag(&name, "minecraft:fences"));
            let width = crate::mod_entities::own_size(rt, mob)[0];
            let step = max_up_step(rt, mob);
            if dy > f64::from(step) && dx * dx + dz * dz < f64::from(1.0f32.max(width))
                || top.is_some_and(|t| p[1] < t + f64::from(at[1])) && !in_door_or_fence
            {
                let j = jump_control(rt, mob);
                jump(rt, j);
                set_operation(rt, c, 3);
            }
        }
        3 => {
            let s = (dbl(rt, c, "speedModifier") * base_speed(rt)) as f32;
            set_speed(rt, mob, s);
            if truth(rt, mob, "$onGround") {
                set_operation(rt, c, 0);
            }
        }
        _ => put_f(rt, mob, "zza", 0.0),
    }
}

/// `MoveControl.isWalkable(dx, dz)`: the block that way is one its path would walk on (any, for a
/// mob without a navigation).
fn walkable(rt: &mut Rt, mob: V, dx: f32, dz: f32) -> bool {
    if navigation(rt, mob).r() == 0 {
        return true;
    }
    let p = position(rt, mob);
    let at = [(p[0] + f64::from(dx)).floor() as i32, p[1].floor() as i32, (p[2] + f64::from(dz)).floor() as i32];
    pathing::static_kind(rt, at) == "WALKABLE"
}

/// `LookControl.setLookAt(x, y, z, yaw speed, pitch limit)`: looked at for the next two ticks.
fn set_look_at(rt: &mut Rt, look: V, at: [f64; 3], yaw: f32, pitch: f32) {
    for (f, v) in ["wantedX", "wantedY", "wantedZ"].into_iter().zip(at) {
        rt.put_field(look, f, V::D(v));
    }
    put_f(rt, look, "yMaxRotSpeed", yaw);
    put_f(rt, look, "xMaxRotAngle", pitch);
    put_i(rt, look, "lookAtCooldown", 2);
}

/// `LookControl.rotateTowards(from, to, max)`.
fn rotate_towards(from: f32, to: f32, max: f32) -> f32 {
    from + wrap(to - from).clamp(-max, max)
}

/// `LookControl.getYRotD()`: the yaw toward what it looks at; none right above or below.
fn wanted_yaw(rt: &mut Rt, look: V, mob: V) -> Option<f32> {
    let p = position(rt, mob);
    let (dx, dz) = (dbl(rt, look, "wantedX") - p[0], dbl(rt, look, "wantedZ") - p[2]);
    (dz.abs() > EQUAL_EPSILON || dx.abs() > EQUAL_EPSILON).then(|| atan2_deg(dz, dx) - 90.0)
}

/// `LookControl.getXRotD()`: the pitch toward what it looks at.
fn wanted_pitch(rt: &mut Rt, look: V, mob: V) -> Option<f32> {
    let p = position(rt, mob);
    let dx = dbl(rt, look, "wantedX") - p[0];
    let dy = dbl(rt, look, "wantedY") - eye_y(rt, mob);
    let dz = dbl(rt, look, "wantedZ") - p[2];
    let flat = (dx * dx + dz * dz).sqrt();
    (dy.abs() > EQUAL_EPSILON || flat.abs() > EQUAL_EPSILON).then(|| -atan2_deg(dy, flat))
}

/// `Optional.of(f)` of a boxed float, or `Optional.empty()`.
fn optional_float(rt: &mut Rt, f: Option<f32>) -> V {
    let v = match f {
        Some(f) => crate::mixin::box_value(rt, V::F(f), 'F'),
        None => V::NULL,
    };
    crate::api_common::optional(rt, v)
}

/// The float an `Optional<Float>` holds, boxed or raw.
fn of_optional_float(rt: &Rt, o: V) -> Option<f32> {
    if o.r() == 0 {
        return None;
    }
    match rt.get_field(o, "value") {
        V::R(0) | V::Void => None,
        v => Some(crate::mixin::unbox(rt, v, 'F').f()),
    }
}

/// `LookControl.tick()`: the head turned toward what it looks at (the pitch too), else back toward
/// its body; while it walks a path, kept within its reach of the body.
fn look_tick(rt: &mut Rt, look: V) {
    let mob = obj(rt, look, "mob");
    if mob.r() == 0 {
        return;
    }
    if own_or(rt, look, RESET_X_ROT, &[], |v| v.i() != 0, |_| true) {
        put_f(rt, mob, "$xRot", 0.0);
    }
    let cooldown = int(rt, look, "lookAtCooldown");
    if cooldown > 0 {
        put_i(rt, look, "lookAtCooldown", cooldown - 1);
        let yaw = if overridden(rt, look, Y_ROT_D.1) {
            let o = vcall(rt, Y_ROT_D, &[look]);
            of_optional_float(rt, o)
        } else {
            wanted_yaw(rt, look, mob)
        };
        if let Some(y) = yaw {
            let head = flt(rt, mob, "yHeadRot");
            put_f(rt, mob, "yHeadRot", rotate_towards(head, y, flt(rt, look, "yMaxRotSpeed")));
        }
        let pitch = if overridden(rt, look, X_ROT_D.1) {
            let o = vcall(rt, X_ROT_D, &[look]);
            of_optional_float(rt, o)
        } else {
            wanted_pitch(rt, look, mob)
        };
        if let Some(x) = pitch {
            let now = flt(rt, mob, "$xRot");
            put_f(rt, mob, "$xRot", rotate_towards(now, x, flt(rt, look, "xMaxRotAngle")));
        }
    } else {
        let (head, body) = (flt(rt, mob, "yHeadRot"), flt(rt, mob, "yBodyRot"));
        put_f(rt, mob, "yHeadRot", rotate_towards(head, body, 10.0));
    }
    own_or(rt, look, CLAMP_HEAD, &[], |_| (), |rt| clamp_head(rt, mob));
}

/// `LookControl.clampHeadRotationToBody()`: while it walks a path, its head within its reach of
/// its body.
fn clamp_head(rt: &mut Rt, mob: V) {
    let nav = navigation(rt, mob);
    let done = nav.r() == 0 || own_or(rt, nav, NAV_DONE, &[], |v| v.i() != 0, |rt| pathing::is_done(rt, nav));
    if !done {
        let (head, body) = (flt(rt, mob, "yHeadRot"), flt(rt, mob, "yBodyRot"));
        let max = max_head_y_rot(rt, mob) as f32;
        put_f(rt, mob, "yHeadRot", rotate_if_necessary(head, body, max));
    }
}

/// `BodyRotationControl.clientTick()`: walking, the body faces where it goes and the head stays
/// within reach; standing, the body follows a head turned far, and after a while turns to it.
fn body_tick(rt: &mut Rt, c: V) {
    let mob = obj(rt, c, "mob");
    if mob.r() == 0 {
        return;
    }
    let p = position(rt, mob);
    let (dx, dz) = (p[0] - dbl(rt, mob, "xo"), p[2] - dbl(rt, mob, "zo"));
    let max = max_head_y_rot(rt, mob) as f32;
    if dx * dx + dz * dz > MIN_SPEED_SQR {
        let yaw = flt(rt, mob, "$yRot");
        put_f(rt, mob, "yBodyRot", yaw);
        let head = flt(rt, mob, "yHeadRot");
        let head = rotate_if_necessary(head, yaw, max);
        put_f(rt, mob, "yHeadRot", head);
        put_f(rt, c, "lastStableYHeadRot", head);
        put_i(rt, c, "headStableTime", 0);
        return;
    }
    let rider = api::call(rt, "net/minecraft/world/entity/Entity.getFirstPassenger.()Lnet/minecraft/world/entity/Entity;", &[mob]);
    if is_mob(rt, rider) {
        return;
    }
    let (head, body) = (flt(rt, mob, "yHeadRot"), flt(rt, mob, "yBodyRot"));
    if (head - flt(rt, c, "lastStableYHeadRot")).abs() > 15.0 {
        put_i(rt, c, "headStableTime", 0);
        put_f(rt, c, "lastStableYHeadRot", head);
        put_f(rt, mob, "yBodyRot", rotate_if_necessary(body, head, max));
    } else {
        let t = int(rt, c, "headStableTime") + 1;
        put_i(rt, c, "headStableTime", t);
        if t > 10 {
            let f = ((t - 10) as f32 / 10.0).clamp(0.0, 1.0);
            put_f(rt, mob, "yBodyRot", rotate_if_necessary(body, head, max * (1.0 - f)));
        }
    }
}
