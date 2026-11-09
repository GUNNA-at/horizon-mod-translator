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
const MOB_INTERACT: Key = key!(
    "net/minecraft/world/entity/Mob",
    "mobInteract.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResult;"
);
const IS_FOOD: Key = key!("net/minecraft/world/entity/animal/Animal", "isFood.(Lnet/minecraft/world/item/ItemStack;)Z");
const USE_PLAYER_ITEM: Key = key!(
    "net/minecraft/world/entity/animal/Animal",
    "usePlayerItem.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;)V"
);
const SET_IN_LOVE: Key = key!("net/minecraft/world/entity/animal/Animal", "setInLove.(Lnet/minecraft/world/entity/player/Player;)V");
const CAN_FALL_IN_LOVE: Key = key!("net/minecraft/world/entity/animal/Animal", "canFallInLove.()Z");
const GET_AGE: Key = key!("net/minecraft/world/entity/AgeableMob", "getAge.()I");
const AGE_UP: Key = key!("net/minecraft/world/entity/AgeableMob", "ageUp.(IZ)V");
const SET_AGE: Key = key!("net/minecraft/world/entity/AgeableMob", "setAge.(I)V");
const SET_BABY: Key = key!("net/minecraft/world/entity/AgeableMob", "setBaby.(Z)V");
const BREED_OFFSPRING: Key = key!(
    "net/minecraft/world/entity/AgeableMob",
    "getBreedOffspring.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/entity/AgeableMob;)Lnet/minecraft/world/entity/AgeableMob;"
);
const IS_IN_LOVE: Key = key!("net/minecraft/world/entity/animal/Animal", "isInLove.()Z");
const RESET_LOVE: Key = key!("net/minecraft/world/entity/animal/Animal", "resetLove.()V");
const LOVE_CAUSE: Key = key!("net/minecraft/world/entity/animal/Animal", "getLoveCause.()Lnet/minecraft/server/level/ServerPlayer;");
const CAN_MATE: Key = key!("net/minecraft/world/entity/animal/Animal", "canMate.(Lnet/minecraft/world/entity/animal/Animal;)Z");
const SPAWN_CHILD: Key = key!(
    "net/minecraft/world/entity/animal/Animal",
    "spawnChildFromBreeding.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/entity/animal/Animal;)V"
);
const FINALIZE_CHILD: Key = key!(
    "net/minecraft/world/entity/animal/Animal",
    "finalizeSpawnChildFromBreeding.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/entity/animal/Animal;Lnet/minecraft/world/entity/AgeableMob;)V"
);
const IS_PANICKING: Key = key!("net/minecraft/world/entity/PathfinderMob", "isPanicking.()Z");
const MOVE_TO: Key = key!("net/minecraft/world/entity/Entity", "moveTo.(DDDFF)V");
const AWARD_STAT_ID: &str = "net/minecraft/world/entity/player/Player.awardStat.(Lnet/minecraft/resources/ResourceLocation;)V";
const ADD_WITH_PASSENGERS: &str = "net/minecraft/server/level/ServerLevel.addFreshEntityWithPassengers.(Lnet/minecraft/world/entity/Entity;)V";
/// `Entity.interact(Player, InteractionHand)`'s descriptor (`Mob.interact`, final).
const INTERACT_DESC: &str = "(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResult;";
const GET_ITEM_IN_HAND: &str =
    "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;";
const CONSUME: &str = "net/minecraft/world/item/ItemStack.consume.(ILnet/minecraft/world/entity/LivingEntity;)V";

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

pub(crate) fn is_mod_mob(rt: &Rt, e: V) -> bool {
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
    own_or(rt, mob, MELEE_RANGE, &[target], |v| v.i() != 0, |rt| melee_range(rt, mob, target))
}

fn melee_range(rt: &mut Rt, mob: V, target: V) -> bool {
    let reach = f64::from(2.04f32).sqrt() - f64::from(0.6f32);
    let b = bounds(rt, mob);
    let a = [b[0] - reach, b[1], b[2] - reach, b[3] + reach, b[4], b[5] + reach];
    let t = bounds(rt, target);
    a[0] < t[3] && a[3] > t[0] && a[1] < t[4] && a[4] > t[1] && a[2] < t[5] && a[5] > t[2]
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
    let level = obj(rt, mob, "$level");
    line_of_sight(rt, level, mob, e)
}

/// `LivingEntity.hasLineOfSight(e)` of `viewer` in `level`: nothing that collides between its eyes
/// and `e`'s, within 128 blocks.
pub(crate) fn line_of_sight(rt: &mut Rt, level: V, viewer: V, e: V) -> bool {
    if e.r() == 0 {
        return false;
    }
    let (from, to) = (eye_pos(rt, viewer), eye_pos(rt, e));
    let d = ((from[0] - to[0]).powi(2) + (from[1] - to[1]).powi(2) + (from[2] - to[2]).powi(2)).sqrt();
    if d > 128.0 {
        return false;
    }
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
    own_or(rt, mob, MAX_FALL, &[], V::i, |rt| fall_distance(rt, mob))
}

fn fall_distance(rt: &mut Rt, mob: V) -> i32 {
    if !rt.instance_of_static(mob, MOB) || target(rt, mob).r() == 0 {
        return 3;
    }
    let i = (health(rt, mob) - max_health(rt, mob) * 0.33) as i32 - 4;
    i.max(0) + 3
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
    own_or(rt, e, SET_SPEED, &[V::F(s)], |_| (), |rt| put_speed(rt, e, s));
}

fn put_speed(rt: &mut Rt, e: V, s: f32) {
    put_f(rt, e, "$speed", s);
    if rt.instance_of_static(e, MOB) {
        put_f(rt, e, "zza", s);
    }
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
    // `getPathfindingCostFromLightLevels`: the brightness of `getMaxLocalRawBrightness` (the sky's
    // light darkened by the time of day and the weather) less 0.5.
    let light = || {
        let level = crate::level_env::raw_brightness(rt, p, crate::level_env::sky_darken(rt)).clamp(0, 15);
        let f = level as f32 / 15.0;
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
            reset_ambient_time(rt, e);
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
    let b = rt.registry_entry("block", name)?;
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

/// `AgeableMob.finalizeSpawn`'s group data: a pack's first mob starts it (`new
/// AgeableMobGroupData(true)`: babies by a chance of 0.05); each after the first may be a baby by
/// that chance; the group counts each one.
fn ageable_group(rt: &mut Rt, e: V, group: V) -> V {
    let group = if group.r() == 0 { crate::spawning::ageable_group_data(rt, true, 0.05) } else { group };
    if !rt.instance_of_static(group, crate::spawning::AGEABLE_GROUP) {
        return group;
    }
    let size = rt.get_field(group, "groupSize").i();
    if rt.get_field(group, "shouldSpawnBaby").i() != 0 && size > 0 {
        let chance = rt.get_field(group, "babySpawnChance").f();
        if random_float(rt, e) <= chance {
            set_age(rt, e, -24000);
        }
    }
    rt.put_field(group, "groupSize", V::I(size + 1));
    group
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
        own_or(rt, mv, MOVE_TICK, &[], |_| (), |rt| move_control_tick(rt, mv));
    }
    let look = obj(rt, mob, "lookControl");
    if look.r() != 0 {
        own_or(rt, look, LOOK_TICK, &[], |_| (), |rt| look_control_tick(rt, look));
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
    } else {
        play_step_sound(rt, e, &state);
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

/// The vanilla control class `class` is or extends: a mod's control's first vanilla class, or the
/// one this models that it climbs to.
fn control_kind(rt: &Rt, class: &str) -> Option<&'static str> {
    let mut c = class;
    while let Some(&up) = rt.supers.get(c) {
        c = up;
    }
    loop {
        if let Some(k) = [FLYING_MOVE, SMOOTH_MOVE, SMOOTH_LOOK, MOVE_CONTROL, LOOK_CONTROL, JUMP_CONTROL, BODY_CONTROL].into_iter().find(|k| *k == c) {
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

// ---- sounds ----

const SOUND_SOURCE: Key = key!("net/minecraft/world/entity/Entity", "getSoundSource.()Lnet/minecraft/sounds/SoundSource;");
const PLAY_HURT_SOUND: Key = key!("net/minecraft/world/entity/LivingEntity", "playHurtSound.(Lnet/minecraft/world/damagesource/DamageSource;)V");
const PLAY_ATTACK_SOUND: Key = key!("net/minecraft/world/entity/Mob", "playAttackSound.()V");
const SOUND_SOURCES: &str = "net/minecraft/sounds/SoundSource";

/// `getSoundSource()` (virtual), as its ordinal: a monster's `HOSTILE` (5), any other's `NEUTRAL` (6).
fn sound_source(rt: &mut Rt, e: V) -> usize {
    if overridden(rt, e, SOUND_SOURCE.1) {
        let s = vcall(rt, SOUND_SOURCE, &[e]);
        if s.r() != 0 {
            return crate::api_common::enum_ordinal(rt, s).clamp(0, 9) as usize;
        }
    }
    if rt.instance_of_static(e, MONSTER) { 5 } else { 6 }
}

/// `playSound` of sound `id` at it: a server's, which the players near it hear (vanilla's
/// `level.playSound(null, ...)`; a client's would be its own player's only, and a mob is none); a
/// silent entity's, none.
fn play_named(rt: &mut Rt, e: V, id: &str, volume: f32, pitch: f32) {
    if !server(rt) || id.is_empty() || truth(rt, e, "$silent") {
        return;
    }
    let at = position(rt, e);
    let source = sound_source(rt, e);
    crate::sounds::play_id(rt, crate::host::Sound { event: id.to_owned(), at, source, volume, pitch });
}

/// `playSound(event, volume, pitch)` (virtual) of a `SoundEvent` or a holder of one; none plays
/// nothing.
fn play_event(rt: &mut Rt, e: V, event: V, volume: f32, pitch: f32) {
    if event.r() == 0 {
        return;
    }
    if overridden(rt, e, PLAY_SOUND.1) {
        vcall(rt, PLAY_SOUND, &[e, event, V::F(volume), V::F(pitch)]);
        return;
    }
    if let Some(id) = crate::sounds::sound_id(rt, event) {
        play_named(rt, e, &id, volume, pitch);
    }
}

/// `isBaby()` (virtual): an ageable mob below age 0.
pub(crate) fn is_baby(rt: &mut Rt, e: V) -> bool {
    own_or(rt, e, IS_BABY, &[], |v| v.i() != 0, |rt| rt.instance_of_static(e, AGEABLE) && int(rt, e, "$age") < 0)
}

/// `getVoicePitch()`: about 1, a baby's about 1.5.
fn voice_pitch(rt: &mut Rt, e: V) -> f32 {
    let (a, b) = (random_float(rt, e), random_float(rt, e));
    (a - b) * 0.2 + if is_baby(rt, e) { 1.5 } else { 1.0 }
}

/// `LivingEntity.makeSound(event)`: at its volume and its voice's pitch (both virtual).
fn make_sound(rt: &mut Rt, e: V, event: V) {
    if event.r() == 0 {
        return;
    }
    let volume = own_or(rt, e, SOUND_VOLUME, &[], V::f, |_| 1.0);
    let pitch = own_or(rt, e, VOICE_PITCH, &[], V::f, |rt| voice_pitch(rt, e));
    play_event(rt, e, event, volume, pitch);
}

fn vanilla_sound(rt: &mut Rt, id: &str) -> V {
    crate::sounds::vanilla_event(rt, id).unwrap_or(V::NULL)
}

/// `LivingEntity.getHurtSound(source)`: a monster's `entity.hostile.hurt`, any other's
/// `entity.generic.hurt`.
fn default_hurt_sound(rt: &mut Rt, e: V) -> V {
    let id = if rt.instance_of_static(e, MONSTER) { "minecraft:entity.hostile.hurt" } else { "minecraft:entity.generic.hurt" };
    vanilla_sound(rt, id)
}

fn default_death_sound(rt: &mut Rt, e: V) -> V {
    let id = if rt.instance_of_static(e, MONSTER) { "minecraft:entity.hostile.death" } else { "minecraft:entity.generic.death" };
    vanilla_sound(rt, id)
}

fn death_sound(rt: &mut Rt, e: V) -> V {
    own_or(rt, e, DEATH_SOUND, &[], |v| v, |rt| default_death_sound(rt, e))
}

/// `Mob.playHurtSound(source)`: its ambient sound put off, then `LivingEntity`'s, its hurt sound
/// (virtual).
fn play_hurt_sound(rt: &mut Rt, e: V, source: V) {
    if rt.instance_of_static(e, MOB) {
        reset_ambient_time(rt, e);
    }
    let s = own_or(rt, e, HURT_SOUND, &[source], |v| v, |rt| default_hurt_sound(rt, e));
    make_sound(rt, e, s);
}

fn reset_ambient_time(rt: &mut Rt, e: V) {
    let interval = own_or(rt, e, AMBIENT_INTERVAL, &[], V::i, |rt| ambient_interval(rt, e));
    put_i(rt, e, "ambientSoundTime", -interval);
}

/// `getAmbientSoundInterval()`: 80 ticks, an animal's 120.
fn ambient_interval(rt: &Rt, e: V) -> i32 {
    if rt.instance_of_static(e, ANIMAL) { 120 } else { 80 }
}

/// `Mob.playAmbientSound()`: its ambient sound (virtual: none unless the class has one).
fn play_ambient(rt: &mut Rt, e: V) {
    let s = own_or(rt, e, AMBIENT_SOUND, &[], |v| v, |_| V::NULL);
    make_sound(rt, e, s);
}

/// `Entity.playStepSound(pos, state)`: the block's step sound at 0.15 of its volume, through the
/// class's own `playSound` where it has one (and the sound is vanilla's).
fn play_step_sound(rt: &mut Rt, e: V, state: &str) {
    let Some(sounds) = rt.host().and_then(|h| h.block_sounds(state)) else { return };
    let (volume, pitch) = (sounds.volume * 0.15, sounds.pitch);
    let event = if overridden(rt, e, PLAY_SOUND.1) { vanilla_sound(rt, &sounds.events[1]) } else { V::NULL };
    if event.r() != 0 {
        play_event(rt, e, event, volume, pitch);
    } else {
        play_named(rt, e, &sounds.events[1], volume, pitch);
    }
}

// ---- fights ----

const INVULNERABLE_TO: Key = key!("net/minecraft/world/entity/Entity", "isInvulnerableTo.(Lnet/minecraft/world/damagesource/DamageSource;)Z");
const FIRE_IMMUNE: Key = key!("net/minecraft/world/entity/Entity", "fireImmune.()Z");
const SET_LAST_HURT_BY: Key = key!("net/minecraft/world/entity/LivingEntity", "setLastHurtByMob.(Lnet/minecraft/world/entity/LivingEntity;)V");
const DROP_CUSTOM: Key = key!(
    "net/minecraft/world/entity/LivingEntity",
    "dropCustomDeathLoot.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/damagesource/DamageSource;Z)V"
);
const BASE_XP: Key = key!("net/minecraft/world/entity/LivingEntity", "getBaseExperienceReward.()I");
const SOURCE_ENTITY: &str = "net/minecraft/world/damagesource/DamageSource.getEntity.()Lnet/minecraft/world/entity/Entity;";
const SOURCE_DIRECT: &str = "net/minecraft/world/damagesource/DamageSource.getDirectEntity.()Lnet/minecraft/world/entity/Entity;";
const SOURCE_POSITION: &str = "net/minecraft/world/damagesource/DamageSource.getSourcePosition.()Lnet/minecraft/world/phys/Vec3;";
const PROJECTILE: &str = "net/minecraft/world/entity/projectile/Projectile";
const AWARD_XP: &str = "net/minecraft/world/entity/ExperienceOrb.award.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/phys/Vec3;I)V";
/// What the swing of an arm lasts (`getCurrentSwingDuration`, without haste or fatigue).
const SWING_DURATION: i32 = 6;

/// A source of damage type `id` (`minecraft:mob_attack`) dealt and credited to `by` (null: none).
fn damage_source(rt: &mut Rt, id: &str, by: V) -> V {
    crate::damage::source(rt, id, by, by, V::NULL)
}

fn source_type(rt: &Rt, s: V) -> String {
    rt.str_of(rt.get_field(s, "$type")).unwrap_or("").to_owned()
}

/// `source.is(tag)`: its damage type in damage type tag `tag`.
fn source_in(rt: &mut Rt, s: V, tag: &str) -> bool {
    let id = source_type(rt, s);
    !id.is_empty() && crate::damage::in_tag(rt, &id, tag)
}

/// `source.getEntity()`: whom the blow is credited to.
fn source_entity(rt: &mut Rt, s: V) -> V {
    if s.r() == 0 {
        return V::NULL;
    }
    api::call(rt, SOURCE_ENTITY, &[s])
}

/// `e.hurt(source, amount)` (virtual): whether it took the blow. A mod's living entity's own, else
/// vanilla's here; any other entity's as it answers (a player's and the game's mobs' through the
/// game).
pub(crate) fn hurt(rt: &mut Rt, e: V, source: V, amount: f32) -> bool {
    if e.r() == 0 {
        return false;
    }
    if is_mod_living(rt, e) {
        return own_or(rt, e, HURT, &[source, V::F(amount)], |v| v.i() != 0, |rt| living_hurt(rt, e, source, amount));
    }
    vcall(rt, HURT, &[e, source, V::F(amount)]).i() != 0
}

fn hurt_by_type(rt: &mut Rt, e: V, id: &str, amount: f32) -> bool {
    let s = damage_source(rt, id, V::NULL);
    hurt(rt, e, s, amount)
}

/// The type's `fireImmune()`, or the class's own (virtual).
fn fire_immune(rt: &mut Rt, e: V) -> bool {
    own_or(rt, e, FIRE_IMMUNE, &[], |v| v.i() != 0, |rt| {
        let ty = obj(rt, e, "$type");
        ty.r() != 0 && truth(rt, ty, "$fireImmune")
    })
}

/// `Entity.isInvulnerableTo(source)`: a removed entity, an invulnerable one (but to what bypasses
/// that, or a creative player), a fire-immune one to fire, one immune to falls to a fall.
fn invulnerable_to(rt: &mut Rt, e: V, source: V) -> bool {
    if removed(rt, e) {
        return true;
    }
    if truth(rt, e, "$invulnerable") && !source_in(rt, source, "minecraft:bypasses_invulnerability") {
        let by = source_entity(rt, source);
        if !(is_player(rt, by) && is_creative(rt, by)) {
            return true;
        }
    }
    if source_in(rt, source, "minecraft:is_fire") && fire_immune(rt, e) {
        return true;
    }
    source_in(rt, source, "minecraft:is_fall") && type_in_tag(rt, e, "minecraft:fall_damage_immune")
}

fn is_invulnerable_to(rt: &mut Rt, e: V, source: V) -> bool {
    own_or(rt, e, INVULNERABLE_TO, &[source], |v| v.i() != 0, |rt| invulnerable_to(rt, e, source))
}

/// `LivingEntity.hurt(source, amount)` on a server: nothing for one invulnerable to it, dead, or
/// burning under fire resistance; frozen extra types take five times a freezing blow, a helmet
/// three quarters of a falling block's. Within its frames after a blow (`invulnerableTime` past 10)
/// only what tops the last blow hurts it; else the whole blow does, and it flashes, is knocked back
/// away from the source (a projectile's way) and cries out, or dies with its death sound. The
/// blow's living cause is what last hurt it, a player its last attacker for a hundred ticks.
fn living_hurt(rt: &mut Rt, e: V, source: V, amount: f32) -> bool {
    if is_invulnerable_to(rt, e, source) || !server(rt) || health(rt, e) <= 0.0 {
        return false;
    }
    if source_in(rt, source, "minecraft:is_fire") && crate::mob_effects::has(rt, e, "minecraft:fire_resistance") {
        return false;
    }
    put_i(rt, e, "noActionTime", 0);
    let mut amount = amount;
    if source_in(rt, source, "minecraft:is_freezing") && type_in_tag(rt, e, "minecraft:freeze_hurts_extra_types") {
        amount *= 5.0;
    }
    let helmet = equipment(rt, e, 5);
    if source_in(rt, source, "minecraft:damages_helmet") && !crate::api_common::stack_empty(rt, helmet) {
        amount *= 0.75;
    }
    let walk = obj(rt, e, "walkAnimation");
    if walk.r() != 0 {
        put_f(rt, walk, "speed", 1.5);
    }
    let mut full = true;
    if int(rt, e, "invulnerableTime") as f32 > 10.0 && !source_in(rt, source, "minecraft:bypasses_cooldown") {
        let last = flt(rt, e, "lastHurt");
        if amount <= last {
            return false;
        }
        actually_hurt_virtual(rt, e, source, amount - last);
        put_f(rt, e, "lastHurt", amount);
        full = false;
    } else {
        put_f(rt, e, "lastHurt", amount);
        put_i(rt, e, "invulnerableTime", 20);
        actually_hurt_virtual(rt, e, source, amount);
        put_i(rt, e, "hurtDuration", 10);
        put_i(rt, e, "hurtTime", 10);
    }
    let by = source_entity(rt, source);
    if by.r() != 0 {
        let wind = source_type(rt, source) == "minecraft:wind_charge" && type_in_tag(rt, e, "minecraft:no_anger_from_wind_charge");
        if rt.instance_of(by, LIVING) && !source_in(rt, source, "minecraft:no_anger") && !wind {
            own_or(rt, e, SET_LAST_HURT_BY, &[by], |_| (), |rt| set_last_hurt_by(rt, e, by));
        }
        if is_player(rt, by) {
            put_i(rt, e, "lastHurtByPlayerTime", 100);
            rt.put_field(e, "$lastHurtByPlayer", by);
        }
    }
    if full {
        crate::mod_entities::send_living(rt, e, &[HURT_OP]);
        if !source_in(rt, source, "minecraft:no_impact") {
            put_bool(rt, e, "hurtMarked", true);
        }
        if !source_in(rt, source, "minecraft:no_knockback") {
            let (dx, dz) = knock_direction(rt, e, source);
            knock_back(rt, e, 0.4, dx, dz);
        }
    }
    if health(rt, e) <= 0.0 {
        if full {
            let s = death_sound(rt, e);
            make_sound(rt, e, s);
        }
        own_or(rt, e, DIE, &[source], |_| (), |rt| die(rt, e, source));
    } else if full {
        own_or(rt, e, PLAY_HURT_SOUND, &[source], |_| (), |rt| play_hurt_sound(rt, e, source));
    }
    rt.put_field(e, "$lastDamageSource", source);
    rt.put_field(e, "$lastDamageStamp", V::J(game_time(rt)));
    true
}

/// Which way a blow pushes: back along a projectile's flight, else away from where it came from.
fn knock_direction(rt: &mut Rt, e: V, source: V) -> (f64, f64) {
    let direct = api::call(rt, SOURCE_DIRECT, &[source]);
    let projectile = direct.r() != 0
        && ((crate::mod_entities::is_mod_entity(rt, direct) && crate::projectiles::is_projectile_class(crate::mod_entities::vanilla_base(rt, direct)))
            || rt.instance_of(direct, PROJECTILE));
    if projectile {
        let m = motion(rt, direct);
        return (-m[0], -m[2]);
    }
    let at = api::call(rt, SOURCE_POSITION, &[source]);
    if at.r() == 0 {
        return (0.0, 0.0);
    }
    let (from, p) = (crate::api_common::xyz(rt, at), position(rt, e));
    (from[0] - p[0], from[2] - p[2])
}

/// `setLastHurtByMob(by)`: what last hurt it, and when.
fn set_last_hurt_by(rt: &mut Rt, e: V, by: V) {
    rt.put_field(e, "$lastHurtBy", by);
    let t = int(rt, e, "tickCount");
    put_i(rt, e, "$lastHurtByTimestamp", t);
}

/// `setLastHurtMob(target)`: the living entity it last hit, and when.
fn set_last_hurt_mob(rt: &mut Rt, e: V, target: V) {
    let living = target.r() != 0 && rt.instance_of(target, LIVING);
    rt.put_field(e, "$lastHurtMob", if living { target } else { V::NULL });
    let t = int(rt, e, "tickCount");
    put_i(rt, e, "$lastHurtMobTimestamp", t);
}

fn actually_hurt_virtual(rt: &mut Rt, e: V, source: V, amount: f32) {
    own_or(rt, e, ACTUALLY_HURT, &[source, V::F(amount)], |_| (), |rt| actually_hurt(rt, e, source, amount));
}

/// `LivingEntity.actuallyHurt(source, amount)`: less what its armor takes (`CombatRules`) and a
/// resistance effect, then its absorption hearts first, its health the rest.
fn actually_hurt(rt: &mut Rt, e: V, source: V, amount: f32) {
    if is_invulnerable_to(rt, e, source) {
        return;
    }
    let mut damage = amount;
    if !source_in(rt, source, "minecraft:bypasses_armor") {
        let armor = attribute(rt, e, "minecraft:generic.armor").floor() as f32;
        let toughness = attribute(rt, e, "minecraft:generic.armor_toughness") as f32;
        damage = after_armor(damage, armor, toughness);
    }
    let absorption = flt(rt, e, "$absorption");
    let absorbed = damage.min(absorption);
    set_absorption(rt, e, (absorption - absorbed).max(0.0));
    let rest = damage - absorbed;
    if rest != 0.0 {
        let h = health(rt, e);
        set_health(rt, e, h - rest);
    }
}

/// `CombatRules.getDamageAfterAbsorb`: armor takes up to 80% (its points over 25, at least a fifth
/// of them, less for a heavy blow unless tough).
fn after_armor(damage: f32, armor: f32, toughness: f32) -> f32 {
    let f = 2.0 + toughness / 4.0;
    let v = armor - damage / f;
    let lo = armor * 0.2;
    let f1 = if v < lo { lo } else { v.min(20.0) };
    damage * (1.0 - f1 / 25.0)
}

/// `setAbsorptionAmount(f)`: kept between 0 and its most absorption.
fn set_absorption(rt: &mut Rt, e: V, f: f32) {
    let max = attribute(rt, e, "minecraft:generic.max_absorption") as f32;
    put_f(rt, e, "$absorption", f.clamp(0.0, max.max(0.0)));
}

/// `e.knockback(strength, x, z)` (virtual) for a living entity: a mod's here, the game's mobs
/// through the game; a player's comes with the blow the game deals them.
pub(crate) fn knock_back(rt: &mut Rt, e: V, strength: f64, x: f64, z: f64) {
    if is_mod_living(rt, e) {
        own_or(rt, e, KNOCKBACK, &[V::D(strength), V::D(x), V::D(z)], |_| (), |rt| knockback(rt, e, strength, x, z));
    } else if !is_player(rt, e) {
        vcall(rt, KNOCKBACK, &[e, V::D(strength), V::D(x), V::D(z)]);
    }
}

/// `LivingEntity.knockback(strength, x, z)`: less its knockback resistance, half its motion kept
/// and the rest pushed away from `(x, z)`; on the ground, up too (at most 0.4).
fn knockback(rt: &mut Rt, e: V, strength: f64, x: f64, z: f64) {
    let strength = strength * (1.0 - attribute(rt, e, "minecraft:generic.knockback_resistance"));
    if strength <= 0.0 {
        return;
    }
    put_bool(rt, e, "hasImpulse", true);
    let m = motion(rt, e);
    let (mut x, mut z) = (x, z);
    while x * x + z * z < f64::from(1.0e-5f32) {
        x = (random_double(rt, e) - random_double(rt, e)) * 0.01;
        z = (random_double(rt, e) - random_double(rt, e)) * 0.01;
    }
    let len = (x * x + z * z).sqrt();
    let (px, pz) = (x / len * strength, z / len * strength);
    let y = if truth(rt, e, "$onGround") { (m[1] / 2.0 + strength).min(0.4) } else { m[1] };
    set_motion(rt, e, [m[0] / 2.0 - px, y, m[2] / 2.0 - pz]);
}

/// `LivingEntity.die(source)`: once; a server's drops its loot and experience and tells its
/// players (event 3: they see it die); then it lies dying (`Pose.DYING`).
fn die(rt: &mut Rt, e: V, source: V) {
    if removed(rt, e) || truth(rt, e, "dead") {
        return;
    }
    put_bool(rt, e, "dead", true);
    if server(rt) {
        drop_all_death_loot(rt, e, source);
        crate::mod_entities::broadcast_event(rt, e, 3);
    }
    if let Some(dying) = crate::api_common::plain_enum(rt, POSE, "DYING") {
        rt.put_field(e, "$pose", dying);
    }
}

/// `dropAllDeathLoot(level, source)`: with mob drops on, the class's own death loot
/// (`dropCustomDeathLoot`, told whether a player hit it lately); then, if a player did, its
/// experience. Not modelled: the type's loot table and its equipment.
fn drop_all_death_loot(rt: &mut Rt, e: V, source: V) {
    let recently = int(rt, e, "lastHurtByPlayerTime") > 0;
    let drops = rt.host().and_then(|h| h.game_rule("mob_drops")).is_none_or(|v| v != 0);
    let level = obj(rt, e, "$level");
    if drops {
        own_or(rt, e, DROP_CUSTOM, &[level, source, V::I(i32::from(recently))], |_| (), |_| ());
    }
    if recently && drops {
        let xp = own_or(rt, e, BASE_XP, &[], V::i, |rt| base_experience(rt, e));
        if xp > 0 {
            let p = position(rt, e);
            let at = crate::api_common::vec3(rt, p[0], p[1], p[2]);
            api::call(rt, AWARD_XP, &[level, at, V::I(xp)]);
        }
    }
}

/// `getBaseExperienceReward()`: a mob's `xpReward` (a monster's 5), an animal's 1 to 3.
fn base_experience(rt: &mut Rt, e: V) -> i32 {
    if rt.instance_of_static(e, ANIMAL) {
        return 1 + random_int(rt, e, 3);
    }
    if rt.instance_of_static(e, MOB) { int(rt, e, "xpReward") } else { 0 }
}

/// `Mob.doHurtTarget(target)`: a blow of its attack damage (`mob_attack`); one that lands knocks the
/// target back by its attack knockback (slowing it too), and it remembers whom it hit.
fn hurt_target(rt: &mut Rt, mob: V, target: V) -> bool {
    if target.r() == 0 {
        return false;
    }
    let damage = attribute(rt, mob, "minecraft:generic.attack_damage") as f32;
    let source = damage_source(rt, "minecraft:mob_attack", mob);
    let hit = hurt(rt, target, source, damage);
    if hit {
        let knock = attribute(rt, mob, "minecraft:generic.attack_knockback") as f32;
        if knock > 0.0 && rt.instance_of(target, LIVING) {
            let yaw = flt(rt, mob, "$yRot");
            knock_back(rt, target, f64::from(knock * 0.5), f64::from(sin_deg(yaw)), f64::from(-cos_deg(yaw)));
            let m = motion(rt, mob);
            set_motion(rt, mob, [m[0] * 0.6, m[1], m[2] * 0.6]);
        }
        set_last_hurt_mob(rt, mob, target);
        own_or(rt, mob, PLAY_ATTACK_SOUND, &[], |_| (), |_| ());
    }
    hit
}

/// `LivingEntity.swing(hand, updateSelf)`: a swing starts unless one is under way and not yet half
/// done; a server tells its players (vanilla's animate packet).
fn swing(rt: &mut Rt, e: V, hand: i32, _update_self: bool) {
    let t = int(rt, e, "swingTime");
    if truth(rt, e, "swinging") && t < SWING_DURATION / 2 && t >= 0 {
        return;
    }
    put_i(rt, e, "swingTime", -1);
    put_bool(rt, e, "swinging", true);
    let h = hand_value(rt, hand);
    rt.put_field(e, "swingingArm", h);
    if server(rt) {
        crate::mod_entities::send_living(rt, e, &[SWING_OP, hand.clamp(0, 1) as u8]);
    }
}

fn hand_value(rt: &mut Rt, hand: i32) -> V {
    crate::api_common::get_static(rt, HAND, if hand == 1 { "OFF_HAND" } else { "MAIN_HAND" }).unwrap_or(V::NULL)
}

/// `updateSwingTime()`: the swing's ticks counted to its end, its share the attack animation.
fn update_swing_time(rt: &mut Rt, e: V) {
    let mut t = int(rt, e, "swingTime");
    if truth(rt, e, "swinging") {
        t += 1;
        if t >= SWING_DURATION {
            t = 0;
            put_bool(rt, e, "swinging", false);
        }
    } else {
        t = 0;
    }
    put_i(rt, e, "swingTime", t);
    put_f(rt, e, "attackAnim", t as f32 / SWING_DURATION as f32);
}

// ---- the client's copy ----

/// What a server tells a mod's living entity's clients, after [`crate::mod_entities`]'s message
/// and its id (0 is an entity event, which that module reads): a hurt (its flash), a swing (which
/// hand), and its state ([`state_bytes`]).
const HURT_OP: u8 = 1;
const SWING_OP: u8 = 2;
const STATE_OP: u8 = 3;
const ADD_PARTICLE: &str = "net/minecraft/world/level/Level.addParticle.(Lnet/minecraft/core/particles/ParticleOptions;DDDDDD)V";
const RANDOM_GAUSSIAN: &str = "net/minecraft/util/RandomSource.nextGaussian.()D";

/// `LivingEntity.lerpTo(x, y, z, yRot, xRot, steps)`: where the server put it, reached over `steps`
/// ticks of its AI step.
fn lerp_to(rt: &mut Rt, e: V, p: [f64; 3], y_rot: f32, x_rot: f32, steps: i32) {
    for (f, v) in ["lerpX", "lerpY", "lerpZ"].into_iter().zip(p) {
        rt.put_field(e, f, V::D(v));
    }
    rt.put_field(e, "lerpYRot", V::D(f64::from(y_rot)));
    rt.put_field(e, "lerpXRot", V::D(f64::from(x_rot)));
    put_i(rt, e, "lerpSteps", steps);
}

/// `lerpPositionAndRotationStep(steps, ...)`: a `1/steps` share of the way to where the server put
/// it, turning the short way round.
fn lerp_step(rt: &mut Rt, e: V, steps: i32) {
    let d = 1.0 / f64::from(steps.max(1));
    let p = position(rt, e);
    let to = [dbl(rt, e, "lerpX"), dbl(rt, e, "lerpY"), dbl(rt, e, "lerpZ")];
    let at = [p[0] + d * (to[0] - p[0]), p[1] + d * (to[1] - p[1]), p[2] + d * (to[2] - p[2])];
    let (y, x) = (f64::from(flt(rt, e, "$yRot")), f64::from(flt(rt, e, "$xRot")));
    let yaw = (y + d * wrap_d(dbl(rt, e, "lerpYRot") - y)) as f32;
    let pitch = (x + d * (dbl(rt, e, "lerpXRot") - x)) as f32;
    crate::mod_entities::set_pos(rt, e, at);
    put_f(rt, e, "$yRot", yaw % 360.0);
    put_f(rt, e, "$xRot", pitch % 360.0);
}

/// A client's new copy faces where the server's does, its head and body too
/// (`recreateFromPacket`).
pub(crate) fn client_spawned(rt: &mut Rt, e: V) {
    if !is_mod_living(rt, e) {
        return;
    }
    let yaw = flt(rt, e, "$yRot");
    for f in ["yHeadRot", "yHeadRotO", "yBodyRot", "yBodyRotO"] {
        put_f(rt, e, f, yaw);
    }
}

/// Where the server's head yaw goes to a client (with its moves): its `yHeadRot`; 0 for an entity
/// with no head.
pub(crate) fn head_yaw(rt: &Rt, e: V) -> f32 {
    if is_mod_living(rt, e) { flt(rt, e, "yHeadRot") } else { 0.0 }
}

/// A client's move of a mod's living entity: `lerpTo` over three ticks and `lerpHeadTo` (both
/// virtual).
pub(crate) fn client_move(rt: &mut Rt, e: V, p: [f64; 3], rot: [f32; 2], head: f32) {
    let lerp = key!("net/minecraft/world/entity/Entity", "lerpTo.(DDDFFI)V");
    let args = [V::D(p[0]), V::D(p[1]), V::D(p[2]), V::F(rot[0]), V::F(rot[1]), V::I(3)];
    own_or(rt, e, lerp, &args, |_| (), |rt| lerp_to(rt, e, p, rot[0], rot[1], 3));
    let lerp_head = key!("net/minecraft/world/entity/Entity", "lerpHeadTo.(FI)V");
    own_or(rt, e, lerp_head, &[V::F(head), V::I(3)], |_| (), |rt| {
        rt.put_field(e, "lerpYHeadRot", V::D(f64::from(head)));
        put_i(rt, e, "lerpHeadSteps", 3);
    });
}

/// A message about a mod's living entity a client's runtime got (its op past the entity's id).
pub(crate) fn receive(rt: &mut Rt, e: V, body: &[u8]) {
    if !is_mod_living(rt, e) {
        return;
    }
    match body {
        [HURT_OP, ..] => client_hurt(rt, e),
        [SWING_OP, hand, ..] => {
            let hand = i32::from(*hand);
            let h = hand_value(rt, hand);
            own_or(rt, e, SWING, &[h], |_| (), |rt| swing(rt, e, hand, false));
        }
        [STATE_OP, rest @ ..] => apply_state(rt, e, rest),
        _ => {}
    }
}

/// `LivingEntity.handleDamageEvent` on a client: its walk shudders and it flashes red for ten ticks.
fn client_hurt(rt: &mut Rt, e: V) {
    let walk = obj(rt, e, "walkAnimation");
    if walk.r() != 0 {
        put_f(rt, walk, "speed", 1.5);
    }
    put_i(rt, e, "invulnerableTime", 20);
    put_i(rt, e, "hurtDuration", 10);
    put_i(rt, e, "hurtTime", 10);
}

/// `handleEntityEvent(event)` for a mod's living entity on a client: 3 it dies (no health left, a
/// generic death), 20 a mob's spawn puff, 60 the puff it leaves in, 18 an animal's hearts.
pub(crate) fn handle_event(rt: &mut Rt, e: V, event: i32) {
    match event {
        3 => {
            put_f(rt, e, "$health", 0.0);
            let generic = damage_source(rt, "minecraft:generic", V::NULL);
            own_or(rt, e, DIE, &[generic], |_| (), |rt| die(rt, e, generic));
        }
        20 | 60 => poof(rt, e),
        18 if rt.instance_of_static(e, ANIMAL) => {
            for _ in 0..7 {
                let speed = [gaussian(rt, e) * 0.02, gaussian(rt, e) * 0.02, gaussian(rt, e) * 0.02];
                let at = [random_x(rt, e, 1.0), random_y(rt, e) + 0.5, random_z(rt, e, 1.0)];
                add_particle(rt, e, "net/minecraft/core/particles/ParticleTypes.HEART", at, speed);
            }
        }
        _ => {}
    }
}

/// `makePoofParticles()`: twenty puffs about its box.
fn poof(rt: &mut Rt, e: V) {
    for _ in 0..20 {
        let d = [gaussian(rt, e) * 0.02, gaussian(rt, e) * 0.02, gaussian(rt, e) * 0.02];
        let at = [random_x(rt, e, 1.0) - d[0] * 10.0, random_y(rt, e) - d[1] * 10.0, random_z(rt, e, 1.0) - d[2] * 10.0];
        add_particle(rt, e, "net/minecraft/core/particles/ParticleTypes.POOF", at, d);
    }
}

fn add_particle(rt: &mut Rt, e: V, kind: &'static str, at: [f64; 3], speed: [f64; 3]) {
    let options = api::get_static(rt, kind);
    let mut level = obj(rt, e, "$level");
    if level.r() == 0 {
        level = crate::glue::level(rt);
    }
    let args = [level, options, V::D(at[0]), V::D(at[1]), V::D(at[2]), V::D(speed[0]), V::D(speed[1]), V::D(speed[2])];
    api::call(rt, ADD_PARTICLE, &args);
}

fn gaussian(rt: &mut Rt, e: V) -> f64 {
    let r = random(rt, e);
    api::call(rt, RANDOM_GAUSSIAN, &[r]).d()
}

/// `getRandomX(scale)`: somewhere across its width.
fn random_x(rt: &mut Rt, e: V, scale: f64) -> f64 {
    let w = f64::from(crate::mod_entities::own_size(rt, e)[0]);
    position(rt, e)[0] + w * ((2.0 * random_double(rt, e) - 1.0) * scale)
}

fn random_y(rt: &mut Rt, e: V) -> f64 {
    let h = f64::from(crate::mod_entities::own_size(rt, e)[1]);
    position(rt, e)[1] + h * random_double(rt, e)
}

fn random_z(rt: &mut Rt, e: V, scale: f64) -> f64 {
    let w = f64::from(crate::mod_entities::own_size(rt, e)[0]);
    position(rt, e)[2] + w * ((2.0 * random_double(rt, e) - 1.0) * scale)
}

/// What a client draws of a mod's living entity that its synched data would carry in vanilla: its
/// health, whether it is a baby, aggressive (a zombie's raised arms), left-handed and without AI,
/// and what it holds and wears (each slot's item and count).
pub(crate) fn state_bytes(rt: &mut Rt, e: V) -> Vec<u8> {
    let mut out = health(rt, e).to_be_bytes().to_vec();
    let baby = rt.instance_of_static(e, AGEABLE) && int(rt, e, "$age") < 0;
    let flags = u8::from(baby) | u8::from(truth(rt, e, "$aggressive")) << 1 | u8::from(truth(rt, e, "$leftHanded")) << 2 | u8::from(truth(rt, e, "$noAi")) << 3;
    out.push(flags);
    for slot in EQUIPMENT {
        let s = obj(rt, e, slot);
        let (id, count) = if s.r() == 0 || crate::api_common::stack_empty(rt, s) {
            (String::new(), 0)
        } else {
            (crate::api_common::item_like_name(rt, rt.get_field(s, "$item")), rt.get_field(s, "$count").i().max(0))
        };
        put_var(&mut out, id.len() as u64);
        out.extend_from_slice(id.as_bytes());
        put_var(&mut out, count as u64);
    }
    out
}

/// Whether `state` differs from what its players were last sent (which it now is).
pub(crate) fn state_changed(rt: &mut Rt, e: V, state: &[u8]) -> bool {
    let hex: String = state.iter().map(|b| format!("{b:02x}")).collect();
    if rt.str_of(rt.get_field(e, "$sentState")) == Some(hex.as_str()) {
        return false;
    }
    let s = rt.string(&hex);
    rt.put_field(e, "$sentState", s);
    true
}

/// The message of `state` for a client.
pub(crate) fn state_message(state: &[u8]) -> Vec<u8> {
    let mut m = vec![STATE_OP];
    m.extend_from_slice(state);
    m
}

/// [`state_bytes`] on a client.
fn apply_state(rt: &mut Rt, e: V, b: &[u8]) {
    let Some(&[h0, h1, h2, h3, flags]) = b.get(0..5).and_then(|s| <&[u8; 5]>::try_from(s).ok()) else { return };
    put_f(rt, e, "$health", f32::from_be_bytes([h0, h1, h2, h3]));
    if rt.instance_of_static(e, AGEABLE) {
        // A client's age is only whether it is a baby (`AgeableMob.getAge`: -1 or 1).
        put_i(rt, e, "$age", if flags & 1 != 0 { -1 } else { 1 });
    }
    put_bool(rt, e, "$aggressive", flags & 2 != 0);
    put_bool(rt, e, "$leftHanded", flags & 4 != 0);
    put_bool(rt, e, "$noAi", flags & 8 != 0);
    let mut at = 5;
    for slot in EQUIPMENT {
        let Some(len) = read_var(b, &mut at) else { return };
        let Some(id) = b.get(at..at + len as usize).and_then(|s| std::str::from_utf8(s).ok()).map(str::to_owned) else { return };
        at += len as usize;
        let Some(count) = read_var(b, &mut at) else { return };
        let stack = if id.is_empty() || count == 0 { empty_stack(rt) } else { crate::api_common::stack(rt, &id, count as i32) };
        rt.put_field(e, slot, stack);
    }
}

fn put_var(out: &mut Vec<u8>, mut u: u64) {
    loop {
        if u < 0x80 {
            out.push(u as u8);
            return;
        }
        out.push((u as u8 & 0x7F) | 0x80);
        u >>= 7;
    }
}

fn read_var(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut u = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        u |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(u);
        }
    }
    None
}

// ---- despawning, health, equipment, saving ----

const DESPAWN_DISTANCE: &str = "net/minecraft/world/entity/MobCategory.getDespawnDistance.()I";
const NO_DESPAWN_DISTANCE: &str = "net/minecraft/world/entity/MobCategory.getNoDespawnDistance.()I";
/// Its slots' stacks, by `EquipmentSlot` ordinal: main hand, off hand, feet, legs, chest, head, body.
const EQUIPMENT: [&str; 7] = ["$equip0", "$equip1", "$equip2", "$equip3", "$equip4", "$equip5", "$equip6"];

/// `Mob.checkDespawn()` on a server, before its tick: one that needn't stay is gone when no player is
/// within its category's despawn distance (128), or now and then once idle for 30 seconds beyond 32
/// blocks; within 32 it stays busy. An animal never goes (`removeWhenFarAway`). Not modelled: a
/// monster's leaving on peaceful (the runtime doesn't know the difficulty), NeoForge's
/// `MobDespawnEvent`.
fn despawn_check(rt: &mut Rt, e: V) {
    if !is_mod_mob(rt, e) || !server(rt) || removed(rt, e) {
        return;
    }
    let persistent = own_or(rt, e, IS_PERSISTENT, &[], |v| v.i() != 0, |rt| truth(rt, e, "$persistenceRequired"))
        || own_or(rt, e, CUSTOM_PERSISTENCE, &[], |v| v.i() != 0, |_| false);
    if persistent {
        put_i(rt, e, "noActionTime", 0);
        return;
    }
    let p = position(rt, e);
    let players = rt.host().map(|h| h.players()).unwrap_or_default();
    let Some(d) = players.iter().map(|(_, q)| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).reduce(f64::min) else {
        return;
    };
    let ty = obj(rt, e, "$type");
    let category = if ty.r() != 0 { obj(rt, ty, "$category") } else { V::NULL };
    let (far, near) = if category.r() != 0 {
        (api::call(rt, DESPAWN_DISTANCE, &[category]).i(), api::call(rt, NO_DESPAWN_DISTANCE, &[category]).i())
    } else {
        (128, 32)
    };
    let remove_far = |rt: &mut Rt| own_or(rt, e, REMOVE_FAR, &[V::D(d)], |v| v.i() != 0, |rt| !rt.instance_of_static(e, ANIMAL));
    let discard = |rt: &mut Rt| {
        api::call(rt, "net/minecraft/world/entity/Entity.discard.()V", &[e]);
    };
    if d > f64::from(far * far) && remove_far(rt) {
        discard(rt);
    }
    let l = f64::from(near * near);
    if no_action_time(rt, e) > 600 && random_int(rt, e, 800) == 0 && d > l && remove_far(rt) {
        discard(rt);
    } else if d < l {
        put_i(rt, e, "noActionTime", 0);
    }
}

/// `onAttributeUpdated(MAX_HEALTH)`: health above the new most comes down to it.
pub(crate) fn max_health_changed(rt: &mut Rt, e: V) {
    let max = max_health(rt, e);
    if health(rt, e) > max {
        put_f(rt, e, "$health", max);
    }
}

fn empty_stack(rt: &mut Rt) -> V {
    api::get_static(rt, "net/minecraft/world/item/ItemStack.EMPTY")
}

/// The stack in its slot `slot` (an `EquipmentSlot` ordinal).
fn equipment(rt: &mut Rt, e: V, slot: usize) -> V {
    let s = obj(rt, e, EQUIPMENT[slot.min(6)]);
    if s.r() != 0 { s } else { empty_stack(rt) }
}

fn slot_of(rt: &Rt, slot: V) -> usize {
    crate::api_common::enum_ordinal(rt, slot).clamp(0, 6) as usize
}

/// The slot of an `InteractionHand`: the main hand's 0, the off hand's 1.
fn hand_slot(rt: &Rt, hand: V) -> usize {
    usize::from(crate::api_common::enum_ordinal(rt, hand) == 1)
}

/// The stacks of slots `slots`, as a list.
fn stacks(rt: &mut Rt, e: V, slots: std::ops::RangeInclusive<usize>) -> V {
    let mut items = Vec::new();
    for s in slots {
        items.push(equipment(rt, e, s));
    }
    crate::jutil::new_list(rt, items)
}

const PUT_FLOAT: &str = "net/minecraft/nbt/CompoundTag.putFloat.(Ljava/lang/String;F)V";
const PUT_SHORT: &str = "net/minecraft/nbt/CompoundTag.putShort.(Ljava/lang/String;S)V";
const PUT_INT: &str = "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V";
const PUT_BOOLEAN: &str = "net/minecraft/nbt/CompoundTag.putBoolean.(Ljava/lang/String;Z)V";
const GET_FLOAT: &str = "net/minecraft/nbt/CompoundTag.getFloat.(Ljava/lang/String;)F";
const GET_SHORT: &str = "net/minecraft/nbt/CompoundTag.getShort.(Ljava/lang/String;)S";
const GET_INT: &str = "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I";
const GET_BOOLEAN: &str = "net/minecraft/nbt/CompoundTag.getBoolean.(Ljava/lang/String;)Z";
const CONTAINS: &str = "net/minecraft/nbt/CompoundTag.contains.(Ljava/lang/String;)Z";

fn tag_put(rt: &mut Rt, tag: V, key: &'static str, name: &str, v: V) {
    let k = rt.string(name);
    api::call(rt, key, &[tag, k, v]);
}

fn tag_get(rt: &mut Rt, tag: V, key: &'static str, name: &str) -> V {
    let k = rt.string(name);
    api::call(rt, key, &[tag, k])
}

fn tag_int(rt: &mut Rt, tag: V, key: &'static str, name: &str) -> i32 {
    match tag_get(rt, tag, key, name) {
        V::I(i) => i,
        V::J(j) => j as i32,
        _ => 0,
    }
}

/// `addAdditionalSaveData(tag)` of `LivingEntity` and the classes under it: its health, hurt and
/// death times, absorption; a mob's pickup, persistence, handedness and AI; an ageable mob's age; an
/// animal's love.
fn save_living(rt: &mut Rt, e: V, tag: V) {
    let (health, absorption) = (health(rt, e), flt(rt, e, "$absorption"));
    let (hurt, death, by) = (int(rt, e, "hurtTime"), int(rt, e, "deathTime"), int(rt, e, "$lastHurtByTimestamp"));
    tag_put(rt, tag, PUT_FLOAT, "Health", V::F(health));
    tag_put(rt, tag, PUT_SHORT, "HurtTime", V::I(hurt));
    tag_put(rt, tag, PUT_INT, "HurtByTimestamp", V::I(by));
    tag_put(rt, tag, PUT_SHORT, "DeathTime", V::I(death));
    tag_put(rt, tag, PUT_FLOAT, "AbsorptionAmount", V::F(absorption));
    if rt.instance_of_static(e, MOB) {
        for (name, field) in [("CanPickUpLoot", "$canPickUpLoot"), ("PersistenceRequired", "$persistenceRequired"), ("LeftHanded", "$leftHanded")] {
            let b = int(rt, e, field);
            tag_put(rt, tag, PUT_BOOLEAN, name, V::I(i32::from(b != 0)));
        }
        if truth(rt, e, "$noAi") {
            tag_put(rt, tag, PUT_BOOLEAN, "NoAI", V::I(1));
        }
    }
    if rt.instance_of_static(e, AGEABLE) {
        let (age, forced) = (int(rt, e, "$age"), int(rt, e, "forcedAge"));
        tag_put(rt, tag, PUT_INT, "Age", V::I(age));
        tag_put(rt, tag, PUT_INT, "ForcedAge", V::I(forced));
    }
    if rt.instance_of_static(e, ANIMAL) {
        let love = int(rt, e, "inLove");
        tag_put(rt, tag, PUT_INT, "InLove", V::I(love));
    }
}

/// `readAdditionalSaveData(tag)`: [`save_living`]'s keys back.
fn load_living(rt: &mut Rt, e: V, tag: V) {
    let absorption = tag_get(rt, tag, GET_FLOAT, "AbsorptionAmount").f();
    set_absorption(rt, e, absorption);
    let k = rt.string("Health");
    if api::call(rt, CONTAINS, &[tag, k]).i() != 0 {
        let h = tag_get(rt, tag, GET_FLOAT, "Health").f();
        set_health(rt, e, h);
    }
    for (name, field, key) in [("HurtTime", "hurtTime", GET_SHORT), ("DeathTime", "deathTime", GET_SHORT), ("HurtByTimestamp", "$lastHurtByTimestamp", GET_INT)] {
        let v = tag_int(rt, tag, key, name);
        put_i(rt, e, field, v);
    }
    if rt.instance_of_static(e, MOB) {
        for (name, field) in [("CanPickUpLoot", "$canPickUpLoot"), ("PersistenceRequired", "$persistenceRequired"), ("LeftHanded", "$leftHanded"), ("NoAI", "$noAi")] {
            let b = tag_int(rt, tag, GET_BOOLEAN, name);
            put_bool(rt, e, field, b != 0);
        }
    }
    if rt.instance_of_static(e, AGEABLE) {
        let age = tag_int(rt, tag, GET_INT, "Age");
        set_age(rt, e, age);
        let forced = tag_int(rt, tag, GET_INT, "ForcedAge");
        put_i(rt, e, "forcedAge", forced);
    }
    if rt.instance_of_static(e, ANIMAL) {
        let love = tag_int(rt, tag, GET_INT, "InLove");
        put_i(rt, e, "inLove", love);
    }
}

// ---- flying and swimming controls ----

const FLYING_MOVE: &str = "net/minecraft/world/entity/ai/control/FlyingMoveControl";
const SMOOTH_MOVE: &str = "net/minecraft/world/entity/ai/control/SmoothSwimmingMoveControl";
const SMOOTH_LOOK: &str = "net/minecraft/world/entity/ai/control/SmoothSwimmingLookControl";

/// The vanilla control control `c` is of ([`control_kind`] of its class).
fn control_class(rt: &Rt, c: V) -> Option<&'static str> {
    control_kind(rt, rt.class_of(c))
}

/// A move control's tick as its vanilla class runs it: a flying one's, a swimming one's, or
/// `MoveControl`'s.
fn move_control_tick(rt: &mut Rt, c: V) {
    match control_class(rt, c) {
        Some(FLYING_MOVE) => flying_tick(rt, c),
        Some(SMOOTH_MOVE) => smooth_move_tick(rt, c),
        _ => move_tick(rt, c),
    }
}

fn look_control_tick(rt: &mut Rt, c: V) {
    match control_class(rt, c) {
        Some(SMOOTH_LOOK) => smooth_look_tick(rt, c),
        _ => look_tick(rt, c),
    }
}

/// `FlyingMoveControl.tick()`: toward where it wants to be, up or down too, at its flying speed
/// off the ground, without gravity; waiting, it sinks again (unless it hovers in place).
fn flying_tick(rt: &mut Rt, c: V) {
    let mob = obj(rt, c, "mob");
    if mob.r() == 0 {
        return;
    }
    if operation_of(rt, c) != 1 {
        if !truth(rt, c, "hoversInPlace") {
            put_bool(rt, mob, "$noGravity", false);
        }
        put_f(rt, mob, "yya", 0.0);
        put_f(rt, mob, "zza", 0.0);
        return;
    }
    set_operation(rt, c, 0);
    put_bool(rt, mob, "$noGravity", true);
    let p = position(rt, mob);
    let (dx, dy, dz) = (dbl(rt, c, "wantedX") - p[0], dbl(rt, c, "wantedY") - p[1], dbl(rt, c, "wantedZ") - p[2]);
    if dx * dx + dy * dy + dz * dz < MIN_SPEED_SQR {
        put_f(rt, mob, "yya", 0.0);
        put_f(rt, mob, "zza", 0.0);
        return;
    }
    let f = atan2_deg(dz, dx) - 90.0;
    let yaw = flt(rt, mob, "$yRot");
    put_f(rt, mob, "$yRot", rotlerp(yaw, f, 90.0));
    let speed_id = if truth(rt, mob, "$onGround") { "minecraft:generic.movement_speed" } else { "minecraft:generic.flying_speed" };
    let f1 = (dbl(rt, c, "speedModifier") * attribute(rt, mob, speed_id)) as f32;
    set_speed(rt, mob, f1);
    let flat = (dx * dx + dz * dz).sqrt();
    if dy.abs() > EQUAL_EPSILON || flat.abs() > EQUAL_EPSILON {
        let f2 = -atan2_deg(dy, flat);
        let pitch = flt(rt, mob, "$xRot");
        put_f(rt, mob, "$xRot", rotlerp(pitch, f2, int(rt, c, "maxTurn") as f32));
        put_f(rt, mob, "yya", if dy > 0.0 { f1 } else { -f1 });
    }
}

/// `SmoothSwimmingMoveControl.tick()`: in water it swims toward where it wants to be, pitching up
/// and down, kept afloat a little; out of water it crawls, slower the more it must turn.
fn smooth_move_tick(rt: &mut Rt, c: V) {
    let mob = obj(rt, c, "mob");
    if mob.r() == 0 {
        return;
    }
    let water = truth(rt, mob, "$inWater");
    if truth(rt, c, "applyGravity") && water {
        let m = motion(rt, mob);
        set_motion(rt, mob, [m[0], m[1] + 0.005, m[2]]);
    }
    let nav = navigation(rt, mob);
    let done = nav.r() == 0 || own_or(rt, nav, NAV_DONE, &[], |v| v.i() != 0, |rt| pathing::is_done(rt, nav));
    if operation_of(rt, c) != 1 || done {
        set_speed(rt, mob, 0.0);
        for f in ["xxa", "yya", "zza"] {
            put_f(rt, mob, f, 0.0);
        }
        return;
    }
    let p = position(rt, mob);
    let (dx, dy, dz) = (dbl(rt, c, "wantedX") - p[0], dbl(rt, c, "wantedY") - p[1], dbl(rt, c, "wantedZ") - p[2]);
    if dx * dx + dy * dy + dz * dz < MIN_SPEED_SQR {
        put_f(rt, mob, "zza", 0.0);
        return;
    }
    let f = atan2_deg(dz, dx) - 90.0;
    let yaw = rotlerp(flt(rt, mob, "$yRot"), f, int(rt, c, "maxTurnY") as f32);
    put_f(rt, mob, "$yRot", yaw);
    put_f(rt, mob, "yBodyRot", yaw);
    put_f(rt, mob, "yHeadRot", yaw);
    let f1 = (dbl(rt, c, "speedModifier") * attribute(rt, mob, "minecraft:generic.movement_speed")) as f32;
    if water {
        set_speed(rt, mob, f1 * flt(rt, c, "inWaterSpeedModifier"));
        let flat = (dx * dx + dz * dz).sqrt();
        if dy.abs() > EQUAL_EPSILON || flat.abs() > EQUAL_EPSILON {
            let max = int(rt, c, "maxTurnX") as f32;
            let f3 = wrap(-atan2_deg(dy, flat)).clamp(-max, max);
            let pitch = flt(rt, mob, "$xRot");
            put_f(rt, mob, "$xRot", rotlerp(pitch, f3, 5.0));
        }
        let pitch = flt(rt, mob, "$xRot");
        put_f(rt, mob, "zza", cos_deg(pitch) * f1);
        put_f(rt, mob, "yya", -sin_deg(pitch) * f1);
    } else {
        let turn = wrap(yaw - f).abs();
        let factor = 1.0 - ((turn - 10.0) / 50.0).clamp(0.0, 1.0);
        set_speed(rt, mob, f1 * flt(rt, c, "outsideWaterSpeedModifier") * factor);
    }
}

/// `SmoothSwimmingLookControl.tick()`: the head turned a little past what it looks at (20 degrees
/// over, 10 down), back to the body otherwise (level again when it doesn't swim a path); the body
/// follows a head turned past its reach, four degrees a tick.
fn smooth_look_tick(rt: &mut Rt, look: V) {
    let mob = obj(rt, look, "mob");
    if mob.r() == 0 {
        return;
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
            put_f(rt, mob, "yHeadRot", rotate_towards(head, y + 20.0, flt(rt, look, "yMaxRotSpeed")));
        }
        let pitch = if overridden(rt, look, X_ROT_D.1) {
            let o = vcall(rt, X_ROT_D, &[look]);
            of_optional_float(rt, o)
        } else {
            wanted_pitch(rt, look, mob)
        };
        if let Some(x) = pitch {
            let now = flt(rt, mob, "$xRot");
            put_f(rt, mob, "$xRot", rotate_towards(now, x + 10.0, flt(rt, look, "xMaxRotAngle")));
        }
    } else {
        let nav = navigation(rt, mob);
        if nav.r() == 0 || own_or(rt, nav, NAV_DONE, &[], |v| v.i() != 0, |rt| pathing::is_done(rt, nav)) {
            let now = flt(rt, mob, "$xRot");
            put_f(rt, mob, "$xRot", rotate_towards(now, 0.0, 5.0));
        }
        let (head, body) = (flt(rt, mob, "yHeadRot"), flt(rt, mob, "yBodyRot"));
        put_f(rt, mob, "yHeadRot", rotate_towards(head, body, flt(rt, look, "yMaxRotSpeed")));
    }
    let f = wrap(flt(rt, mob, "yHeadRot") - flt(rt, mob, "yBodyRot"));
    let max = int(rt, look, "maxYRotFromCenter") as f32;
    let body = flt(rt, mob, "yBodyRot");
    if f < -max {
        put_f(rt, mob, "yBodyRot", body - 4.0);
    } else if f > max {
        put_f(rt, mob, "yBodyRot", body + 4.0);
    }
}

/// `Mob.lookAt(entity, maxYaw, maxPitch)`: its facing turned toward `target`'s eyes (a non-living
/// one's middle), at most so far each way.
fn mob_look_at(rt: &mut Rt, mob: V, target: V, max_yaw: f32, max_pitch: f32) {
    let (p, q) = (position(rt, mob), position(rt, target));
    let (dx, dz) = (q[0] - p[0], q[2] - p[2]);
    let to = if rt.instance_of(target, LIVING) {
        eye_y(rt, target)
    } else {
        let b = bounds(rt, target);
        (b[1] + b[4]) / 2.0
    };
    let dy = to - eye_y(rt, mob);
    let flat = (dx * dx + dz * dz).sqrt();
    let yaw = atan2_deg(dz, dx) - 90.0;
    let pitch = -atan2_deg(dy, flat);
    let x = flt(rt, mob, "$xRot");
    put_f(rt, mob, "$xRot", rotate_towards(x, pitch, max_pitch));
    let y = flt(rt, mob, "$yRot");
    put_f(rt, mob, "$yRot", rotate_towards(y, yaw, max_yaw));
}

// ---- what the class's code asks ----

const TYPE_LEVEL: &str = "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
const SOUND_SOURCE_NAMES: [&str; 10] = ["MASTER", "MUSIC", "RECORDS", "WEATHER", "BLOCKS", "HOSTILE", "NEUTRAL", "PLAYERS", "AMBIENT", "VOICE"];
const ARM: &str = "net/minecraft/world/entity/HumanoidArm";
const CHECK_DESPAWN: Key = key!("net/minecraft/world/entity/Entity", "checkDespawn.()V");
const HEART_TICKS: i32 = 600;

/// Whether `class` is `LivingEntity` or a vanilla class under it (`Animal`, `Zombie`...).
fn living_tree(class: &str) -> bool {
    let mut c = class;
    for _ in 0..16 {
        if c == LIVING {
            return true;
        }
        match crate::vanilla_entities::api_super(c) {
            Some(up) => c = up,
            None => return false,
        }
    }
    false
}

fn control_owner(owner: &str) -> bool {
    matches!(owner, MOVE_CONTROL | LOOK_CONTROL | JUMP_CONTROL | BODY_CONTROL | FLYING_MOVE | SMOOTH_MOVE | SMOOTH_LOOK)
}

/// `Entity`'s methods a living entity has its own of (`LivingEntity.hurt`, `tick`...), which a call
/// naming `Entity` (another module's, the game's) on a mod's living entity runs.
fn living_entity_method(name: &str) -> bool {
    matches!(
        name,
        "tick"
            | "baseTick"
            | "hurt"
            | "isAlive"
            | "isPickable"
            | "isPushable"
            | "isAttackable"
            | "kill"
            | "move"
            | "lerpTo"
            | "lerpHeadTo"
            | "getYHeadRot"
            | "setYHeadRot"
            | "setYBodyRot"
            | "getVisualRotationYInDegrees"
            | "getPreciseBodyRotation"
            | "handleEntityEvent"
            | "isInvulnerableTo"
            | "fireImmune"
            | "getSoundSource"
            | "checkDespawn"
            | "causeFallDamage"
            | "playStepSound"
            | "isAlliedTo"
            | "getRandomX"
            | "getRandomY"
            | "getRandomZ"
            | "interact"
            | "addAdditionalSaveData"
            | "readAdditionalSaveData"
            | "onGround"
            | "setOnGround"
            | "resetFallDistance"
            | "getMaxFallDistance"
            | "maxUpStep"
            | "getAirSupply"
            | "setAirSupply"
            | "getMaxAirSupply"
    )
}

/// The superclass of a vanilla control class this module models: what a call on a control of one
/// it doesn't climbs to ([`crate::api::api_super`]).
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    match class {
        FLYING_MOVE | SMOOTH_MOVE => Some(MOVE_CONTROL),
        SMOOTH_LOOK => Some(LOOK_CONTROL),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    control_owner(owner) || owner == WALK_ANIMATION || (owner == ENTITY && living_entity_method(name)) || living_tree(owner)
}

/// A mod's living entity's calls into vanilla's classes above it (its constructor, `super.tick()`,
/// `getNavigation()`...), and its controls' and walk animation's. A call this doesn't answer goes to
/// what `Entity` answers for any mod entity ([`crate::mod_entities`]); one on another living entity
/// (a player, the game's own mobs) to the modules after this.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if control_owner(owner) {
        return control_call(rt, owner, name, desc, args);
    }
    if owner == WALK_ANIMATION {
        return walk_call(rt, name, desc, args);
    }
    let e = a(0);
    if name == "<init>" {
        if owner == ENTITY || desc != TYPE_LEVEL || !is_mod_living(rt, e) {
            return None;
        }
        construct(rt, e, a(1), a(2));
        return Some(V::Void);
    }
    if !is_mod_living(rt, e) {
        return None;
    }
    if let Some(v) = living_call(rt, name, desc, args) {
        return Some(v);
    }
    if owner == ENTITY {
        return None;
    }
    crate::mod_entities::call(rt, ENTITY, name, desc, args)
}

fn unit(_: ()) -> V {
    V::Void
}

fn bool_v(b: bool) -> V {
    V::I(i32::from(b))
}

/// What vanilla's `LivingEntity`, `Mob`, `PathfinderMob`, `AgeableMob` and `Animal` answer for a
/// mod's living entity (their own code, not the class's overrides: a call here is the class's
/// `super`, or the vanilla method it doesn't override).
fn living_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    let mob = rt.instance_of_static(e, MOB);
    Some(match (name, desc) {
        // Its tick.
        ("tick", "()V") => unit(tick(rt, e)),
        ("baseTick", "()V") => unit(base_tick(rt, e)),
        ("aiStep", "()V") => unit(ai_step(rt, e)),
        ("travel", "(Lnet/minecraft/world/phys/Vec3;)V") => {
            let input = crate::api_common::xyz(rt, a(1));
            unit(travel(rt, e, input))
        }
        ("tickDeath", "()V") => unit(tick_death(rt, e)),
        ("customServerAiStep" | "registerGoals" | "playAttackSound" | "onSyncedDataUpdated" | "defineSynchedData", _) => V::Void,
        ("startSeenByPlayer" | "stopSeenByPlayer" | "setDropChance" | "ageBoundaryReached", _) => V::Void,
        ("checkDespawn", "()V") => unit(despawn_check(rt, e)),
        ("isEffectiveAi", "()Z") => bool_v(effective_ai(rt, e)),
        ("isNoAi", "()Z") => bool_v(truth(rt, e, "$noAi")),
        ("setNoAi", "(Z)V") => unit(put_bool(rt, e, "$noAi", a(1).i() != 0)),
        ("serverAiStep", "()V") if mob => unit(server_ai_step(rt, e)),
        // Its health.
        ("getHealth", "()F") => V::F(health(rt, e)),
        ("setHealth", "(F)V") => unit(set_health(rt, e, a(1).f())),
        ("getMaxHealth", "()F") => V::F(max_health(rt, e)),
        ("heal", "(F)V") => {
            let h = health(rt, e);
            if h > 0.0 {
                set_health(rt, e, h + a(1).f());
            }
            V::Void
        }
        ("isDeadOrDying", "()Z") => bool_v(health(rt, e) <= 0.0),
        ("isAlive", "()Z") => bool_v(living_alive(rt, e)),
        ("isHurt", "()Z") => {
            let h = health(rt, e);
            bool_v(h > 0.0 && h < max_health(rt, e))
        }
        ("getAbsorptionAmount", "()F") => V::F(flt(rt, e, "$absorption")),
        ("setAbsorptionAmount", "(F)V") => unit(set_absorption(rt, e, a(1).f())),
        ("getArmorValue", "()I") => V::I(attribute(rt, e, "minecraft:generic.armor").floor() as i32),
        ("getScale", "()F") => V::F(scale(rt, e)),
        ("getAgeScale", "()F") => V::F(if is_baby(rt, e) { 0.5 } else { 1.0 }),
        // Its fights.
        ("hurt", "(Lnet/minecraft/world/damagesource/DamageSource;F)Z") => bool_v(living_hurt(rt, e, a(1), a(2).f())),
        ("actuallyHurt", "(Lnet/minecraft/world/damagesource/DamageSource;F)V") => unit(actually_hurt(rt, e, a(1), a(2).f())),
        ("die", "(Lnet/minecraft/world/damagesource/DamageSource;)V") => unit(die(rt, e, a(1))),
        ("dropAllDeathLoot", _) => unit(drop_all_death_loot(rt, e, a(2))),
        ("dropCustomDeathLoot", _) => V::Void,
        // `EntityType.getDefaultLootTable()`: `<namespace>:entities/<path>` of its type's id.
        ("getDefaultLootTable" | "getLootTable", "()Lnet/minecraft/resources/ResourceKey;") => {
            let id = type_name(rt, e);
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id.as_str()));
            rt.string(&format!("loot_table#{ns}:entities/{path}"))
        }
        ("kill", "()V") => {
            let s = damage_source(rt, "minecraft:generic_kill", V::NULL);
            hurt(rt, e, s, f32::MAX);
            V::Void
        }
        ("knockback", "(DDD)V") => unit(knockback(rt, e, a(1).d(), a(2).d(), a(3).d())),
        ("isInvulnerableTo", "(Lnet/minecraft/world/damagesource/DamageSource;)Z") => bool_v(invulnerable_to(rt, e, a(1))),
        ("fireImmune", "()Z") => {
            let ty = obj(rt, e, "$type");
            bool_v(ty.r() != 0 && truth(rt, ty, "$fireImmune"))
        }
        ("doHurtTarget", "(Lnet/minecraft/world/entity/Entity;)Z") if mob => bool_v(hurt_target(rt, e, a(1))),
        ("isWithinMeleeAttackRange", _) if mob => bool_v(melee_range(rt, e, a(1))),
        ("swing", "(Lnet/minecraft/world/InteractionHand;)V") => {
            let hand = crate::api_common::enum_ordinal(rt, a(1)).max(0);
            unit(swing(rt, e, hand, false))
        }
        ("swing", "(Lnet/minecraft/world/InteractionHand;Z)V") => {
            let hand = crate::api_common::enum_ordinal(rt, a(1)).max(0);
            unit(swing(rt, e, hand, a(2).i() != 0))
        }
        ("getLastHurtByMob", _) => last_hurt_by_mob(rt, e),
        ("setLastHurtByMob", _) => unit(set_last_hurt_by(rt, e, a(1))),
        ("getLastHurtByMobTimestamp", "()I") => V::I(last_hurt_by_mob_timestamp(rt, e)),
        ("getLastHurtMob", _) => obj(rt, e, "$lastHurtMob"),
        ("setLastHurtMob", _) => unit(set_last_hurt_mob(rt, e, a(1))),
        ("getLastHurtMobTimestamp", "()I") => V::I(int(rt, e, "$lastHurtMobTimestamp")),
        ("setLastHurtByPlayer", _) => {
            rt.put_field(e, "$lastHurtByPlayer", a(1));
            put_i(rt, e, "lastHurtByPlayerTime", 100);
            V::Void
        }
        ("getLastDamageSource", _) => last_damage_source(rt, e),
        ("getKillCredit", _) => {
            let by = obj(rt, e, "$lastHurtByPlayer");
            if by.r() != 0 { by } else { last_hurt_by_mob(rt, e) }
        }
        ("canAttack", "(Lnet/minecraft/world/entity/LivingEntity;)Z") => bool_v(a(1).r() != 0 && can_be_seen_as_enemy(rt, a(1))),
        ("isAlliedTo", _) => V::I(0),
        ("getExperienceReward", _) => V::I(own_or(rt, e, BASE_XP, &[], V::i, |rt| base_experience(rt, e))),
        ("getBaseExperienceReward", "()I") => V::I(base_experience(rt, e)),
        ("isBlocking" | "isUsingItem" | "isFallFlying" | "isAutoSpinAttack" | "isSleeping" | "isInvertedHealAndHarm", "()Z") => V::I(0),
        ("isAffectedByPotions" | "attackable" | "isAttackable", "()Z") => V::I(1),
        ("isPickable", "()Z") => bool_v(!removed(rt, e)),
        ("isPushable", "()Z") => {
            let alive = living_alive(rt, e);
            bool_v(alive && !on_climbable(rt, e))
        }
        ("getArrowCount", "()I") => V::I(int(rt, e, "$arrows")),
        ("setArrowCount", "(I)V") => unit(put_i(rt, e, "$arrows", a(1).i())),
        // Its sounds.
        ("getSoundSource", _) => {
            let i = if rt.instance_of_static(e, MONSTER) { 5 } else { 6 };
            crate::api_common::plain_enum(rt, SOUND_SOURCES, SOUND_SOURCE_NAMES[i]).unwrap_or(V::NULL)
        }
        ("getHurtSound", _) => default_hurt_sound(rt, e),
        ("getDeathSound", _) => default_death_sound(rt, e),
        ("getAmbientSound", _) => V::NULL,
        ("playHurtSound", _) => unit(play_hurt_sound(rt, e, a(1))),
        ("playAmbientSound", "()V") => unit(play_ambient(rt, e)),
        ("getAmbientSoundInterval", "()I") => V::I(ambient_interval(rt, e)),
        ("getSoundVolume", "()F") => V::F(1.0),
        ("getVoicePitch", "()F") => V::F(voice_pitch(rt, e)),
        ("makeSound", _) => unit(make_sound(rt, e, a(1))),
        ("playStepSound", _) => {
            let state = rt.str_of(a(2)).unwrap_or("").to_owned();
            unit(play_step_sound(rt, e, &state))
        }
        // How it moves.
        ("move", "(Lnet/minecraft/world/entity/MoverType;Lnet/minecraft/world/phys/Vec3;)V") => {
            let d = crate::api_common::xyz(rt, a(2));
            unit(move_self(rt, e, d))
        }
        ("jumpFromGround", "()V") => unit(jump_from_ground(rt, e)),
        ("getJumpPower", "()F") => V::F(jump_power(rt, e)),
        ("setJumping", "(Z)V") => unit(put_bool(rt, e, "jumping", a(1).i() != 0)),
        ("getSpeed", "()F") => V::F(flt(rt, e, "$speed")),
        ("setSpeed", "(F)V") => unit(put_speed(rt, e, a(1).f())),
        ("setXxa", "(F)V") => unit(put_f(rt, e, "xxa", a(1).f())),
        ("setYya", "(F)V") => unit(put_f(rt, e, "yya", a(1).f())),
        ("setZza", "(F)V") => unit(put_f(rt, e, "zza", a(1).f())),
        ("moveRelative", "(FLnet/minecraft/world/phys/Vec3;)V") => {
            let input = crate::api_common::xyz(rt, a(2));
            unit(move_relative(rt, e, a(1).f(), input))
        }
        ("onClimbable", "()Z") => bool_v(on_climbable(rt, e)),
        ("onGround", "()Z") => bool_v(truth(rt, e, "$onGround")),
        ("setOnGround", "(Z)V") => unit(put_bool(rt, e, "$onGround", a(1).i() != 0)),
        ("resetFallDistance", "()V") => unit(put_f(rt, e, "fallDistance", 0.0)),
        ("causeFallDamage", "(FFLnet/minecraft/world/damagesource/DamageSource;)Z") => bool_v(cause_fall_damage(rt, e, a(1).f(), a(2).f(), a(3))),
        ("getMaxFallDistance", "()I") => V::I(fall_distance(rt, e)),
        ("maxUpStep", "()F") => V::F(attribute(rt, e, "minecraft:generic.step_height") as f32),
        ("canBreatheUnderwater", "()Z") => bool_v(type_in_tag(rt, e, "minecraft:can_breathe_under_water")),
        ("getAirSupply", "()I") => V::I(int(rt, e, "$air")),
        ("setAirSupply", "(I)V") => unit(put_i(rt, e, "$air", a(1).i())),
        ("getMaxAirSupply", "()I") => V::I(MAX_AIR),
        ("lerpTo", "(DDDFFI)V") => unit(lerp_to(rt, e, [a(1).d(), a(2).d(), a(3).d()], a(4).f(), a(5).f(), a(6).i())),
        ("lerpHeadTo", "(FI)V") => {
            rt.put_field(e, "lerpYHeadRot", V::D(f64::from(a(1).f())));
            unit(put_i(rt, e, "lerpHeadSteps", a(2).i()))
        }
        ("getYHeadRot", "()F") => V::F(flt(rt, e, "yHeadRot")),
        ("setYHeadRot", "(F)V") => unit(put_f(rt, e, "yHeadRot", a(1).f())),
        ("setYBodyRot", "(F)V") => unit(put_f(rt, e, "yBodyRot", a(1).f())),
        ("getVisualRotationYInDegrees", "()F") => V::F(flt(rt, e, "yBodyRot")),
        ("getPreciseBodyRotation", "(F)F") => {
            let (old, now) = (flt(rt, e, "yBodyRotO"), flt(rt, e, "yBodyRot"));
            V::F(old + a(1).f() * (now - old))
        }
        ("getMaxHeadXRot", "()I") => V::I(40),
        ("getMaxHeadYRot", "()I") => V::I(75),
        ("getHeadRotSpeed", "()I") => V::I(10),
        ("lookAt", "(Lnet/minecraft/world/entity/Entity;FF)V") => unit(mob_look_at(rt, e, a(1), a(2).f(), a(3).f())),
        ("getRandomX", "(D)D") => V::D(random_x(rt, e, a(1).d())),
        ("getRandomY", "()D") => V::D(random_y(rt, e)),
        ("getRandomZ", "(D)D") => V::D(random_z(rt, e, a(1).d())),
        ("handleEntityEvent", "(B)V") => unit(handle_event(rt, e, a(1).i())),
        ("spawnAnim", "()V") => {
            if server(rt) {
                crate::mod_entities::broadcast_event(rt, e, 20);
            } else {
                poof(rt, e);
            }
            V::Void
        }
        // What it holds and wears.
        ("getItemBySlot", _) => {
            let slot = slot_of(rt, a(1));
            equipment(rt, e, slot)
        }
        ("setItemSlot", _) => {
            let slot = slot_of(rt, a(1));
            unit(rt.put_field(e, EQUIPMENT[slot], a(2)))
        }
        ("getItemInHand", _) => {
            let slot = hand_slot(rt, a(1));
            equipment(rt, e, slot)
        }
        ("setItemInHand", _) => {
            let slot = hand_slot(rt, a(1));
            unit(rt.put_field(e, EQUIPMENT[slot], a(2)))
        }
        ("getMainHandItem", _) => equipment(rt, e, 0),
        ("getOffhandItem", _) => equipment(rt, e, 1),
        ("getHandSlots", "()Ljava/lang/Iterable;") => stacks(rt, e, 0..=1),
        ("getArmorSlots", "()Ljava/lang/Iterable;") => stacks(rt, e, 2..=5),
        ("getAllSlots", "()Ljava/lang/Iterable;") => stacks(rt, e, 0..=5),
        ("getArmorAndBodyArmorSlots", "()Ljava/lang/Iterable;") => stacks(rt, e, 2..=6),
        ("getUseItem", _) => empty_stack(rt),
        ("isHolding", "(Lnet/minecraft/world/item/Item;)Z") => {
            let want = crate::api_common::item_like_name(rt, a(1));
            let held = (0..2).any(|i| {
                let s = obj(rt, e, EQUIPMENT[i]);
                s.r() != 0 && crate::api_common::item_like_name(rt, rt.get_field(s, "$item")) == want
            });
            bool_v(held)
        }
        ("getMainArm", _) => {
            let arm = if truth(rt, e, "$leftHanded") { "LEFT" } else { "RIGHT" };
            crate::api_common::plain_enum(rt, ARM, arm).unwrap_or(V::NULL)
        }
        ("isLeftHanded", "()Z") => bool_v(truth(rt, e, "$leftHanded")),
        ("setLeftHanded", "(Z)V") => unit(put_bool(rt, e, "$leftHanded", a(1).i() != 0)),
        ("canPickUpLoot", "()Z") => bool_v(truth(rt, e, "$canPickUpLoot")),
        ("setCanPickUpLoot", "(Z)V") => unit(put_bool(rt, e, "$canPickUpLoot", a(1).i() != 0)),
        // A mob's AI.
        ("getNavigation", _) if mob => obj(rt, e, "navigation"),
        ("getMoveControl", _) if mob => obj(rt, e, "moveControl"),
        ("getLookControl", _) if mob => obj(rt, e, "lookControl"),
        ("getJumpControl", _) if mob => obj(rt, e, "jumpControl"),
        ("getSensing", _) if mob => obj(rt, e, "$sensing"),
        ("getTarget", _) if mob => obj(rt, e, "$target"),
        ("setTarget", _) if mob => unit(rt.put_field(e, "$target", a(1))),
        ("isAggressive", "()Z") => bool_v(truth(rt, e, "$aggressive")),
        ("setAggressive", "(Z)V") => unit(put_bool(rt, e, "$aggressive", a(1).i() != 0)),
        ("isPersistenceRequired", "()Z") => bool_v(truth(rt, e, "$persistenceRequired")),
        ("setPersistenceRequired", "()V") => unit(put_bool(rt, e, "$persistenceRequired", true)),
        ("requiresCustomPersistence", "()Z") => V::I(0),
        ("removeWhenFarAway", "(D)Z") => bool_v(!rt.instance_of_static(e, ANIMAL)),
        ("getMaxSpawnClusterSize", "()I") => V::I(4),
        // Where a natural spawn may stand (`Mob.checkSpawnRules`, `checkSpawnObstruction`), and a
        // pack that is never full but by its cluster size (`isMaxGroupSizeReached`).
        ("checkSpawnRules", "(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;)Z") if mob => {
            bool_v(crate::spawning::mob_check_spawn_rules(rt, e))
        }
        ("checkSpawnObstruction", "(Lnet/minecraft/world/level/LevelReader;)Z") if mob => bool_v(crate::spawning::mob_check_obstruction(rt, e)),
        ("isMaxGroupSizeReached", "(I)Z") if mob => V::I(0),
        ("getPathfindingMalus", _) if mob => {
            let k = crate::api_common::enum_ordinal(rt, a(1)).clamp(0, 25) as usize;
            V::F(pathing::malus(rt, e, k))
        }
        ("setPathfindingMalus", _) if mob => {
            let k = crate::api_common::enum_ordinal(rt, a(1)).clamp(0, 25) as usize;
            unit(pathing::set_malus(rt, e, k, a(2).f()))
        }
        ("getWalkTargetValue", _) if mob => {
            let p = api::pos_of(rt, a(1));
            V::F(walk_target_default(rt, e, p))
        }
        ("isPathFinding", "()Z") if mob => {
            let nav = obj(rt, e, "navigation");
            bool_v(nav.r() != 0 && !pathing::is_done(rt, nav))
        }
        ("isWithinRestriction", "()Z") if mob => {
            let p = block_position(rt, e);
            bool_v(within_restriction(rt, e, p))
        }
        ("isWithinRestriction", "(Lnet/minecraft/core/BlockPos;)Z") if mob => {
            let p = api::pos_of(rt, a(1));
            bool_v(within_restriction(rt, e, p))
        }
        ("restrictTo", "(Lnet/minecraft/core/BlockPos;I)V") if mob => {
            let p = api::pos_of(rt, a(1));
            for (f, v) in ["$restrictX", "$restrictY", "$restrictZ"].into_iter().zip(p) {
                put_i(rt, e, f, v);
            }
            unit(put_f(rt, e, "$restrictRadius", a(2).i() as f32))
        }
        ("getRestrictCenter", _) if mob => {
            let (c, _) = restriction(rt, e).unwrap_or(([0; 3], -1.0));
            api::block_pos(rt, i64::from(c[0]), i64::from(c[1]), i64::from(c[2]))
        }
        ("getRestrictRadius", "()F") if mob => V::F(flt_or(rt, e, "$restrictRadius", -1.0)),
        ("hasRestriction", "()Z") if mob => bool_v(restriction(rt, e).is_some()),
        ("clearRestriction", "()V") if mob => unit(put_f(rt, e, "$restrictRadius", -1.0)),
        ("finalizeSpawn", _) if mob => {
            let group = if rt.instance_of_static(e, AGEABLE) { ageable_group(rt, e, a(4)) } else { a(4) };
            // `Mob.finalizeSpawn`: one in twenty left-handed.
            let left = random_float(rt, e) < 0.05;
            put_bool(rt, e, "$leftHanded", left);
            group
        }
        // A player's click on it: `Mob.interact` (final), whose `mobInteract` is an animal's food here.
        ("interact", INTERACT_DESC) if mob => interact(rt, e, a(1), a(2)),
        ("interact", INTERACT_DESC) => interaction_result(rt, "PASS"),
        ("mobInteract", _) if rt.instance_of_static(e, ANIMAL) => animal_interact(rt, e, a(1), a(2)),
        ("mobInteract", _) if mob => interaction_result(rt, "PASS"),
        ("usePlayerItem", _) if rt.instance_of_static(e, ANIMAL) => {
            api::call(rt, CONSUME, &[a(3), V::I(1), a(1)]);
            V::Void
        }
        ("isFood", "(Lnet/minecraft/world/item/ItemStack;)Z") => V::I(0),
        ("isLeashed", "()Z") => V::I(0),
        ("getLeashHolder", _) => V::NULL,
        // An ageable mob's age, an animal's love.
        ("getAge", "()I") => V::I(int(rt, e, "$age")),
        ("setAge", "(I)V") => unit(set_age(rt, e, a(1).i())),
        ("isBaby", "()Z") => bool_v(rt.instance_of_static(e, AGEABLE) && int(rt, e, "$age") < 0),
        ("setBaby", "(Z)V") => unit(if rt.instance_of_static(e, AGEABLE) { set_age(rt, e, if a(1).i() != 0 { -24000 } else { 0 }) }),
        ("ageUp", "(I)V" | "(IZ)V") => unit(age_up(rt, e, a(1).i())),
        ("canBreed", "()Z") => V::I(1),
        ("isInLove", "()Z") => bool_v(int(rt, e, "inLove") > 0),
        ("canFallInLove", "()Z") => bool_v(int(rt, e, "inLove") <= 0),
        ("getInLoveTime", "()I") => V::I(int(rt, e, "inLove")),
        ("setInLoveTime", "(I)V") => unit(put_i(rt, e, "inLove", a(1).i())),
        ("resetLove", "()V") => unit(put_i(rt, e, "inLove", 0)),
        ("setInLove", _) => unit(set_in_love(rt, e, a(1))),
        ("getLoveCause", _) => obj(rt, e, "$loveCause"),
        // Breeding.
        ("canMate", "(Lnet/minecraft/world/entity/animal/Animal;)Z") => bool_v(vanilla_can_mate(rt, e, a(1))),
        ("isPanicking", "()Z") if mob => bool_v(crate::goals::panicking(rt, e)),
        ("getBreedOffspring", _) => V::NULL,
        ("spawnChildFromBreeding", _) => unit(spawn_child(rt, e, a(1), a(2))),
        ("finalizeSpawnChildFromBreeding", _) => unit(finalize_child(rt, e, a(1), a(2), a(3))),
        // What it saves.
        ("addAdditionalSaveData", "(Lnet/minecraft/nbt/CompoundTag;)V") => unit(save_living(rt, e, a(1))),
        ("readAdditionalSaveData", "(Lnet/minecraft/nbt/CompoundTag;)V") => unit(load_living(rt, e, a(1))),
        _ => return None,
    })
}

/// `InteractionResult`'s constant `name` (`PASS`, `SUCCESS`, `CONSUME`).
fn interaction_result(rt: &mut Rt, name: &str) -> V {
    crate::api_common::result(rt, "net/minecraft/world/InteractionResult", name)
}

/// Whether `r` (an `InteractionResult`) used the click (`consumesAction`).
fn consumes(rt: &Rt, r: V) -> bool {
    r.r() != 0 && matches!(rt.str_of(r), Some("SUCCESS" | "SUCCESS_NO_ITEM_USED" | "CONSUME" | "CONSUME_PARTIAL"))
}

/// `Mob.interact(player, hand)` (final): a mob that isn't alive passes; else what it does with the
/// click itself (`mobInteract`, virtual: an animal's food). Not modelled: a leash, a name tag and
/// a spawn egg of its kind (`checkAndHandleImportantInteractions`).
fn interact(rt: &mut Rt, e: V, player: V, hand: V) -> V {
    if !is_alive(rt, e) {
        return interaction_result(rt, "PASS");
    }
    let r = own_or(rt, e, MOB_INTERACT, &[player, hand], |v| v, |rt| vanilla_mob_interact(rt, e, player, hand));
    if consumes(rt, r) { r } else { interaction_result(rt, "PASS") }
}

/// `mobInteract` as vanilla's class above a mod's mob answers it: an animal's food, else nothing.
fn vanilla_mob_interact(rt: &mut Rt, e: V, player: V, hand: V) -> V {
    if rt.instance_of_static(e, ANIMAL) {
        animal_interact(rt, e, player, hand)
    } else {
        interaction_result(rt, "PASS")
    }
}

/// `Animal.mobInteract(player, hand)` (1.21.1): with its food in hand (`isFood`), a grown one not
/// in love on a server takes one (`usePlayerItem`) and falls in love with the player (`setInLove`:
/// `SUCCESS`); a baby takes one and grows a tenth of the time it has left (`ageUp`: `SUCCESS` on a
/// client, `CONSUME` on a server); a client's grown one answers `CONSUME`. Anything else passes.
fn animal_interact(rt: &mut Rt, e: V, player: V, hand: V) -> V {
    let stack = api::call(rt, GET_ITEM_IN_HAND, &[player, hand]);
    let food = !crate::api_common::stack_empty(rt, stack) && own_or(rt, e, IS_FOOD, &[stack], |v| v.i() != 0, |_| false);
    if !food {
        return interaction_result(rt, "PASS");
    }
    let age = own_or(rt, e, GET_AGE, &[], V::i, |rt| int(rt, e, "$age"));
    let server = server(rt);
    if server && age == 0 && own_or(rt, e, CAN_FALL_IN_LOVE, &[], |v| v.i() != 0, |rt| int(rt, e, "inLove") <= 0) {
        use_player_item(rt, e, player, hand, stack);
        own_or(rt, e, SET_IN_LOVE, &[player], |_| (), |rt| set_in_love(rt, e, player));
        return interaction_result(rt, "SUCCESS");
    }
    if is_baby(rt, e) {
        use_player_item(rt, e, player, hand, stack);
        // `getSpeedUpSecondsWhenFeeding(-age)`: a tenth of the seconds left.
        let seconds = ((-age / 20) as f32 * 0.1) as i32;
        own_or(rt, e, AGE_UP, &[V::I(seconds), V::I(1)], |_| (), |rt| age_up(rt, e, seconds));
        return interaction_result(rt, if server { "CONSUME" } else { "SUCCESS" });
    }
    interaction_result(rt, if server { "PASS" } else { "CONSUME" })
}

/// `usePlayerItem(player, hand, stack)` (virtual): one of the stack used (`consume(1, player)`,
/// none from a creative player's).
fn use_player_item(rt: &mut Rt, e: V, player: V, hand: V, stack: V) {
    own_or(rt, e, USE_PLAYER_ITEM, &[player, hand, stack], |_| (), |rt| {
        api::call(rt, CONSUME, &[stack, V::I(1), player]);
    });
}

/// `Animal.setInLove(player)`: in love for 600 ticks, for `player` (`loveCause`, none for null),
/// and its players see the hearts (event 18).
fn set_in_love(rt: &mut Rt, e: V, player: V) {
    put_i(rt, e, "inLove", HEART_TICKS);
    rt.put_field(e, "$loveCause", player);
    crate::mod_entities::broadcast_event(rt, e, 18);
}

/// `AgeableMob.ageUp(seconds)`: `seconds` older, a baby no older than grown.
fn age_up(rt: &mut Rt, e: V, seconds: i32) {
    let age = int(rt, e, "$age");
    let grown = age + seconds * 20;
    set_age(rt, e, if age < 0 { grown.min(0) } else { grown });
}

// ---- breeding ----

/// The level a mod's entity is in (`level()`): the one it was made in, or the runtime's.
pub(crate) fn level(rt: &mut Rt, e: V) -> V {
    match obj(rt, e, "$level") {
        l if l.r() != 0 => l,
        _ => crate::glue::level(rt),
    }
}

/// `isInLove()` (virtual).
pub(crate) fn in_love(rt: &mut Rt, e: V) -> bool {
    own_or(rt, e, IS_IN_LOVE, &[], |v| v.i() != 0, |rt| int(rt, e, "inLove") > 0)
}

/// `e.canMate(other)` (virtual).
pub(crate) fn can_mate(rt: &mut Rt, e: V, other: V) -> bool {
    own_or(rt, e, CAN_MATE, &[other], |v| v.i() != 0, |rt| vanilla_can_mate(rt, e, other))
}

/// `Animal.canMate(other)`: another animal of its very class, both in love.
fn vanilla_can_mate(rt: &mut Rt, e: V, other: V) -> bool {
    other.r() != 0 && other != e && rt.class_of(other) == rt.class_of(e) && in_love(rt, e) && in_love(rt, other)
}

/// `isPanicking()` (virtual): a mob's panic goal runs.
pub(crate) fn is_panicking(rt: &mut Rt, e: V) -> bool {
    if !is_mod_mob(rt, e) {
        return false;
    }
    own_or(rt, e, IS_PANICKING, &[], |v| v.i() != 0, |rt| crate::goals::panicking(rt, e))
}

/// `e.spawnChildFromBreeding(level, mate)` (virtual), as `BreedGoal.breed` calls it.
pub(crate) fn spawn_child_from_breeding(rt: &mut Rt, e: V, level: V, mate: V) {
    own_or(rt, e, SPAWN_CHILD, &[level, mate], |_| (), |rt| spawn_child(rt, e, level, mate));
}

/// `getLoveCause()` (virtual): the player who fed it, null when none (or not an animal).
pub(crate) fn love_cause(rt: &mut Rt, e: V) -> V {
    if e.r() == 0 || !is_mod_living(rt, e) || !rt.instance_of_static(e, ANIMAL) {
        return V::NULL;
    }
    own_or(rt, e, LOVE_CAUSE, &[], |v| v, |rt| obj(rt, e, "$loveCause"))
}

/// `Animal.spawnChildFromBreeding(level, mate)` (NeoForge 1.21.1): the class's baby
/// (`getBreedOffspring`), told to `BabyEntitySpawnEvent` first. A listener canceling it leaves both
/// parents resting as after a breeding (age 6000, out of love) and no baby; one may give another
/// baby. The baby, made one (`setBaby`), joins the level where this parent stands, after
/// `finalizeSpawnChildFromBreeding`.
fn spawn_child(rt: &mut Rt, e: V, level: V, mate: V) {
    let proposed = own_or(rt, e, BREED_OFFSPRING, &[level, mate], |v| v, |_| V::NULL);
    let (canceled, child) = crate::world_events::baby_spawn(rt, e, mate, proposed);
    if canceled {
        for p in [e, mate] {
            set_age_virtual(rt, p, 6000);
        }
        for p in [e, mate] {
            reset_love_virtual(rt, p);
        }
        return;
    }
    if child.r() == 0 {
        return;
    }
    api::virtual_call(rt, SET_BABY, &[child, V::I(1)]);
    rt.report_thrown(SET_BABY.1);
    let at = position(rt, e);
    api::virtual_call(rt, MOVE_TO, &[child, V::D(at[0]), V::D(at[1]), V::D(at[2]), V::F(0.0), V::F(0.0)]);
    rt.report_thrown(MOVE_TO.1);
    own_or(rt, e, FINALIZE_CHILD, &[level, mate, child], |_| (), |rt| finalize_child(rt, e, level, mate, child));
    api::call(rt, ADD_WITH_PASSENGERS, &[level, child]);
    rt.report_thrown("addFreshEntityWithPassengers");
}

/// `Animal.finalizeSpawnChildFromBreeding(level, mate, baby)`: the player who fed this parent (or
/// else its mate) has bred an animal (the `animals_bred` statistic, the `bred_animals`
/// advancement trigger); both parents rest 6000 ticks out of love, this one's players see the
/// hearts (event 18), and with `doMobLoot` an experience orb of 1 to 7 points drops where it
/// stands.
fn finalize_child(rt: &mut Rt, e: V, _level: V, mate: V, child: V) {
    let cause = match love_cause(rt, e) {
        c if c.r() != 0 => c,
        _ => love_cause(rt, mate),
    };
    if cause.r() != 0 {
        let id = rt.string("minecraft:animals_bred");
        api::call(rt, AWARD_STAT_ID, &[cause, id]);
        rt.report_thrown("awardStat");
        trigger_bred(rt, cause, e, mate, child);
    }
    set_age_virtual(rt, e, 6000);
    set_age_virtual(rt, mate, 6000);
    reset_love_virtual(rt, e);
    reset_love_virtual(rt, mate);
    crate::mod_entities::broadcast_event(rt, e, 18);
    if crate::gamerules::mobs_drop(rt) {
        let value = random_int(rt, e, 7) + 1;
        let at = position(rt, e);
        rt.host_call(|h| h.spawn_experience_orb(at, value as u32, None));
    }
}

/// `CriteriaTriggers.BRED_ANIMALS.trigger(player, parent, partner, child)`: the game tests its
/// criteria on the animals' types, places and ages.
fn trigger_bred(rt: &mut Rt, player: V, parent: V, partner: V, child: V) {
    let Some(uuid) = crate::network::player_uuid(rt, player) else {
        return;
    };
    let (parent, partner) = (bred_entity(rt, parent), bred_entity(rt, partner));
    let child = (child.r() != 0).then(|| bred_entity(rt, child));
    rt.host_call(|h| h.trigger_bred(uuid, &parent, &partner, child.as_ref()));
}

fn bred_entity(rt: &mut Rt, e: V) -> crate::host::BredEntity {
    let ty = rt.get_field(e, "$type");
    let kind = crate::mod_entities::type_id(rt, ty).unwrap_or_default().to_owned();
    let baby = is_mod_living(rt, e) && is_baby(rt, e);
    crate::host::BredEntity { kind, position: position(rt, e), baby }
}

/// `setAge(age)` (virtual).
fn set_age_virtual(rt: &mut Rt, e: V, age: i32) {
    own_or(rt, e, SET_AGE, &[V::I(age)], |_| (), |rt| set_age(rt, e, age));
}

/// `resetLove()` (virtual).
fn reset_love_virtual(rt: &mut Rt, e: V) {
    own_or(rt, e, RESET_LOVE, &[], |_| (), |rt| put_i(rt, e, "inLove", 0));
}

/// `LivingEntity.getScale()` (final) of a mod's living entity: its scale attribute, kept within
/// 1/16 to 16 (`sanitizeScale`). A baby's half size is its dimensions' (`getAgeScale`) and its
/// model's.
pub(crate) fn scale(rt: &mut Rt, e: V) -> f32 {
    (attribute(rt, e, "minecraft:generic.scale") as f32).clamp(0.0625, 16.0)
}

/// `checkDespawn()` (virtual) of a mod's mob on a server, before its tick.
pub(crate) fn check_despawn(rt: &mut Rt, e: V) {
    if is_mod_mob(rt, e) && server(rt) && !removed(rt, e) {
        own_or(rt, e, CHECK_DESPAWN, &[], |_| (), |rt| despawn_check(rt, e));
    }
}

/// The calls on a control: its constructor (a flying one's turn and hovering, a swimming one's
/// turns and speeds), and vanilla's methods by the control it is.
fn control_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    if c.r() == 0 {
        return None;
    }
    if name == "<init>" {
        let kind = control_kind(rt, owner)?;
        let base = match kind {
            FLYING_MOVE | SMOOTH_MOVE => MOVE_CONTROL,
            SMOOTH_LOOK => LOOK_CONTROL,
            k => k,
        };
        init_control(rt, c, base, a(1));
        match kind {
            FLYING_MOVE => {
                put_i(rt, c, "maxTurn", a(2).i());
                put_bool(rt, c, "hoversInPlace", a(3).i() != 0);
            }
            SMOOTH_MOVE => {
                put_i(rt, c, "maxTurnX", a(2).i());
                put_i(rt, c, "maxTurnY", a(3).i());
                put_f(rt, c, "inWaterSpeedModifier", a(4).f());
                put_f(rt, c, "outsideWaterSpeedModifier", a(5).f());
                put_bool(rt, c, "applyGravity", a(6).i() != 0);
            }
            SMOOTH_LOOK => put_i(rt, c, "maxYRotFromCenter", a(2).i()),
            _ => {}
        }
        return Some(V::Void);
    }
    let kind = control_class(rt, c)?;
    let wanted = |rt: &Rt, f: &'static str| V::D(dbl(rt, c, f));
    Some(match kind {
        MOVE_CONTROL | FLYING_MOVE | SMOOTH_MOVE => match (name, desc) {
            ("tick", "()V") => unit(move_control_tick(rt, c)),
            ("setWantedPosition", "(DDDD)V") => unit(set_wanted(rt, c, [a(1).d(), a(2).d(), a(3).d()], a(4).d())),
            ("hasWanted", "()Z") => bool_v(operation_of(rt, c) == 1),
            ("getSpeedModifier", "()D") => wanted(rt, "speedModifier"),
            ("getWantedX", "()D") => wanted(rt, "wantedX"),
            ("getWantedY", "()D") => wanted(rt, "wantedY"),
            ("getWantedZ", "()D") => wanted(rt, "wantedZ"),
            ("strafe", "(FF)V") => {
                set_operation(rt, c, 2);
                put_f(rt, c, "strafeForwards", a(1).f());
                put_f(rt, c, "strafeRight", a(2).f());
                unit(rt.put_field(c, "speedModifier", V::D(0.25)))
            }
            ("setWait", "()V") => unit(set_operation(rt, c, 0)),
            ("rotlerp", "(FFF)F") => V::F(rotlerp(a(1).f(), a(2).f(), a(3).f())),
            _ => return None,
        },
        LOOK_CONTROL | SMOOTH_LOOK => {
            let mob = obj(rt, c, "mob");
            match (name, desc) {
                ("tick", "()V") => unit(look_control_tick(rt, c)),
                ("setLookAt", "(DDDFF)V") => unit(set_look_at(rt, c, [a(1).d(), a(2).d(), a(3).d()], a(4).f(), a(5).f())),
                ("setLookAt", "(DDD)V") => {
                    let (yaw, pitch) = (head_rot_speed(rt, mob) as f32, max_head_x_rot(rt, mob) as f32);
                    unit(set_look_at(rt, c, [a(1).d(), a(2).d(), a(3).d()], yaw, pitch))
                }
                ("setLookAt", "(Lnet/minecraft/world/phys/Vec3;)V") => {
                    let at = crate::api_common::xyz(rt, a(1));
                    let (yaw, pitch) = (head_rot_speed(rt, mob) as f32, max_head_x_rot(rt, mob) as f32);
                    unit(set_look_at(rt, c, at, yaw, pitch))
                }
                ("setLookAt", "(Lnet/minecraft/world/entity/Entity;)V") => {
                    let at = look_target(rt, a(1));
                    let (yaw, pitch) = (head_rot_speed(rt, mob) as f32, max_head_x_rot(rt, mob) as f32);
                    unit(set_look_at(rt, c, at, yaw, pitch))
                }
                ("setLookAt", "(Lnet/minecraft/world/entity/Entity;FF)V") => {
                    let at = look_target(rt, a(1));
                    unit(set_look_at(rt, c, at, a(2).f(), a(3).f()))
                }
                ("isLookingAtTarget", "()Z") => bool_v(int(rt, c, "lookAtCooldown") > 0),
                ("getWantedX", "()D") => wanted(rt, "wantedX"),
                ("getWantedY", "()D") => wanted(rt, "wantedY"),
                ("getWantedZ", "()D") => wanted(rt, "wantedZ"),
                ("resetXRotOnTick", "()Z") => V::I(1),
                ("clampHeadRotationToBody", "()V") => unit(clamp_head(rt, mob)),
                ("getYRotD", _) => {
                    let y = wanted_yaw(rt, c, mob);
                    optional_float(rt, y)
                }
                ("getXRotD", _) => {
                    let x = wanted_pitch(rt, c, mob);
                    optional_float(rt, x)
                }
                ("rotateTowards", "(FFF)F") => V::F(rotate_towards(a(1).f(), a(2).f(), a(3).f())),
                _ => return None,
            }
        }
        JUMP_CONTROL => match name {
            "jump" => unit(put_bool(rt, c, "jump", true)),
            "tick" => {
                let mob = obj(rt, c, "mob");
                let j = truth(rt, c, "jump");
                if mob.r() != 0 {
                    put_bool(rt, mob, "jumping", j);
                }
                unit(put_bool(rt, c, "jump", false))
            }
            _ => return None,
        },
        _ => match name {
            "clientTick" => unit(body_tick(rt, c)),
            _ => return None,
        },
    })
}

/// `WalkAnimationState`'s: how fast a limb swings (a tick ago and now, between them), how far it
/// has swung, and its update.
fn walk_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    if w.r() == 0 {
        return None;
    }
    Some(match (name, desc) {
        ("<init>", "()V") => {
            for f in ["speedOld", "speed", "position"] {
                put_f(rt, w, f, 0.0);
            }
            V::Void
        }
        ("setSpeed", "(F)V") => unit(put_f(rt, w, "speed", a(1).f())),
        ("update", "(FF)V") => unit(walk_update(rt, w, a(1).f(), a(2).f())),
        ("speed", "()F") => V::F(flt(rt, w, "speed")),
        ("speed", "(F)F") => {
            let (old, now) = (flt(rt, w, "speedOld"), flt(rt, w, "speed"));
            V::F(old + a(1).f() * (now - old))
        }
        ("position", "()F") => V::F(flt(rt, w, "position")),
        ("position", "(F)F") => V::F(flt(rt, w, "position") - flt(rt, w, "speed") * (1.0 - a(1).f())),
        ("isMoving", "()Z") => bool_v(flt(rt, w, "speed") > 1.0e-5),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const PENGUIN: &str = "test/Penguin";
    const INIT: &str = "net/minecraft/world/entity/animal/Animal.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
    const SAVE: &str = "net/minecraft/world/entity/animal/Animal.addAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
    const LOAD: &str = "net/minecraft/world/entity/animal/Animal.readAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";

    /// A runtime with a mod's animal class that has no code of its own, and its type: 0.6 by 1 block.
    fn runtime() -> (Rt, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.supers.insert(PENGUIN, ANIMAL);
        let ty = rt.new_object("net/minecraft/world/entity/EntityType");
        rt.put_field(ty, "$width", V::F(0.6));
        rt.put_field(ty, "$height", V::F(1.0));
        rt.registered.insert(("entity_type".into(), "test:penguin".into()), ty);
        (rt, ty)
    }

    /// A server's world: a stone floor at y 0, 17 by 17 blocks.
    fn floor() -> MemoryHost {
        let mut host = MemoryHost::default();
        for x in -8..=8 {
            for z in -8..=8 {
                host.blocks.insert([x, 0, z], "minecraft:stone".into());
            }
        }
        host
    }

    fn penguin(rt: &mut Rt, ty: V, at: [f64; 3]) -> V {
        let level = crate::glue::level(rt);
        let e = rt.new_object(PENGUIN);
        api::call(rt, INIT, &[e, ty, level]);
        crate::mod_entities::set_pos(rt, e, at);
        e
    }

    /// One level tick of `e`: last tick's place kept, then its tick.
    fn step(rt: &mut Rt, e: V) {
        let p = position(rt, e);
        for (f, v) in ["xo", "yo", "zo"].into_iter().zip(p) {
            rt.put_field(e, f, V::D(v));
        }
        let t = int(rt, e, "tickCount");
        put_i(rt, e, "tickCount", t + 1);
        tick(rt, e);
    }

    #[test]
    fn a_mods_animal_walks_where_its_move_control_sends_it_on_the_floor() {
        let (mut rt, ty) = runtime();
        let mut host = floor();
        rt.with_host(&mut host, |rt| {
            let e = penguin(rt, ty, [0.5, 1.0, 0.5]);
            let mv = obj(rt, e, "moveControl");
            for _ in 0..30 {
                set_wanted_position(rt, mv, [4.5, 1.0, 0.5], 0.35);
                step(rt, e);
            }
            let p = position(rt, e);
            assert!(p[0] > 2.5 && p[0] < 5.0, "walked east: {p:?}");
            assert!((p[2] - 0.5).abs() < 0.3, "kept its line: {p:?}");
            assert_eq!(p[1], 1.0, "stands on the floor");
            assert!(truth(rt, e, "$onGround"));
            // Facing east (yaw 270), its legs swinging.
            assert!(wrap(flt(rt, e, "$yRot") - 270.0).abs() < 1.0, "{}", flt(rt, e, "$yRot"));
            let walk = obj(rt, e, "walkAnimation");
            assert!(api::call(rt, "net/minecraft/world/entity/WalkAnimationState.isMoving.()Z", &[walk]).i() != 0);
        });
    }

    #[test]
    fn a_blow_within_its_frames_hurts_only_by_what_tops_the_last_and_death_takes_twenty_ticks() {
        let (mut rt, ty) = runtime();
        let mut host = floor();
        rt.with_host(&mut host, |rt| {
            let e = penguin(rt, ty, [0.5, 1.0, 0.5]);
            assert_eq!(health(rt, e), 20.0);
            let generic = damage_source(rt, "minecraft:generic", V::NULL);
            assert!(hurt(rt, e, generic, 5.0));
            assert_eq!((health(rt, e), int(rt, e, "hurtTime"), int(rt, e, "invulnerableTime")), (15.0, 10, 20));
            assert!(!hurt(rt, e, generic, 3.0), "no more than the last blow");
            assert_eq!(health(rt, e), 15.0);
            assert!(hurt(rt, e, generic, 8.0));
            assert_eq!(health(rt, e), 12.0, "8 less the 5 it took");
            assert!(hurt(rt, e, generic, 40.0));
            assert_eq!(health(rt, e), 0.0);
            assert!(truth(rt, e, "dead"));
            for _ in 0..19 {
                step(rt, e);
            }
            assert!(!removed(rt, e), "still falling over");
            step(rt, e);
            assert!(removed(rt, e), "gone after twenty ticks");
        });
        assert!(host.orbs.is_empty(), "no player killed it: no experience");
    }

    #[test]
    fn a_clients_copy_gets_its_health_age_handedness_and_what_it_holds() {
        let (mut rt, ty) = runtime();
        let mut host = floor();
        rt.with_host(&mut host, |rt| {
            let server = penguin(rt, ty, [0.5, 1.0, 0.5]);
            set_health(rt, server, 12.5);
            set_age(rt, server, -24000);
            put_bool(rt, server, "$leftHanded", true);
            let sword = crate::api_common::stack(rt, "minecraft:iron_sword", 1);
            let main = api::get_static(rt, "net/minecraft/world/entity/EquipmentSlot.MAINHAND");
            api::call(rt, "net/minecraft/world/entity/Mob.setItemSlot.(Lnet/minecraft/world/entity/EquipmentSlot;Lnet/minecraft/world/item/ItemStack;)V", &[server, main, sword]);
            let state = state_bytes(rt, server);
            assert!(state_changed(rt, server, &state));
            assert!(!state_changed(rt, server, &state), "sent once");
            let client = penguin(rt, ty, [0.5, 1.0, 0.5]);
            receive(rt, client, &state_message(&state));
            assert_eq!(health(rt, client), 12.5);
            assert!(is_baby(rt, client));
            assert!(truth(rt, client, "$leftHanded"));
            let held = api::call(rt, "net/minecraft/world/entity/LivingEntity.getMainHandItem.()Lnet/minecraft/world/item/ItemStack;", &[client]);
            assert_eq!(crate::api_common::item_like_name(rt, rt.get_field(held, "$item")), "minecraft:iron_sword");
            let off = api::call(rt, "net/minecraft/world/entity/LivingEntity.getOffhandItem.()Lnet/minecraft/world/item/ItemStack;", &[client]);
            assert!(crate::api_common::stack_empty(rt, off));
        });
    }

    #[test]
    fn what_a_mods_animal_saves_loads_back_into_a_new_one() {
        let (mut rt, ty) = runtime();
        let mut host = floor();
        rt.with_host(&mut host, |rt| {
            let e = penguin(rt, ty, [0.5, 1.0, 0.5]);
            set_health(rt, e, 7.0);
            set_age(rt, e, -100);
            put_i(rt, e, "inLove", 300);
            put_bool(rt, e, "$leftHanded", true);
            put_bool(rt, e, "$persistenceRequired", true);
            let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
            api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
            api::call(rt, SAVE, &[e, tag]);
            assert_eq!(tag_get(rt, tag, GET_FLOAT, "Health"), V::F(7.0), "vanilla's key");
            assert_eq!(tag_int(rt, tag, GET_INT, "Age"), -100);
            let fresh = penguin(rt, ty, [0.5, 1.0, 0.5]);
            api::call(rt, LOAD, &[fresh, tag]);
            assert_eq!(health(rt, fresh), 7.0);
            assert_eq!(int(rt, fresh, "$age"), -100);
            assert_eq!(int(rt, fresh, "inLove"), 300);
            assert!(truth(rt, fresh, "$leftHanded") && truth(rt, fresh, "$persistenceRequired"));
        });
    }

    #[test]
    fn a_flying_move_control_lifts_it_toward_a_point_above_without_gravity() {
        let (mut rt, ty) = runtime();
        let mut host = floor();
        rt.with_host(&mut host, |rt| {
            let e = penguin(rt, ty, [0.5, 1.0, 0.5]);
            // Still the first tick (it hasn't moved yet), on the ground the next.
            step(rt, e);
            step(rt, e);
            assert!(truth(rt, e, "$onGround"), "landed");
            let c = rt.new_object(FLYING_MOVE);
            api::call(rt, "net/minecraft/world/entity/ai/control/FlyingMoveControl.<init>.(Lnet/minecraft/world/entity/Mob;IZ)V", &[c, e, V::I(20), V::I(1)]);
            rt.put_field(e, "moveControl", c);
            // Off the ground at its walking speed (0.7, the attribute's default): up by 0.7 * 0.7
            // in the first tick, less the fall it had from standing (gravity 0.08, less 2%); then
            // in the air at 0.02 a tick, its rise slowing by 2% a tick.
            set_wanted_position(rt, c, [0.5, 6.0, 0.5], 1.0);
            step(rt, e);
            let y = position(rt, e)[1];
            assert!((y - (1.0 + 0.7 * 0.7 - 0.08 * 0.98)).abs() < 1.0e-3, "{y}");
            for _ in 0..3 {
                set_wanted_position(rt, c, [0.5, 6.0, 0.5], 1.0);
                step(rt, e);
            }
            assert!(position(rt, e)[1] > 2.6, "kept rising: {:?}", position(rt, e));
            assert!(truth(rt, e, "$noGravity"));
        });
    }

    #[test]
    fn calls_naming_its_vanilla_classes_reach_the_mobs_state_and_the_entitys() {
        let (mut rt, ty) = runtime();
        let mut host = floor();
        rt.with_host(&mut host, |rt| {
            let e = penguin(rt, ty, [0.5, 1.0, 0.5]);
            let nav = api::call(rt, "net/minecraft/world/entity/PathfinderMob.getNavigation.()Lnet/minecraft/world/entity/ai/navigation/PathNavigation;", &[e]);
            assert!(nav.r() != 0 && nav == obj(rt, e, "navigation"));
            assert_eq!(api::call(rt, "net/minecraft/world/entity/animal/Animal.getHealth.()F", &[e]), V::F(20.0));
            assert_eq!(api::call(rt, "net/minecraft/world/entity/animal/Animal.getX.()D", &[e]), V::D(0.5));
            let other = penguin(rt, ty, [2.5, 1.0, 0.5]);
            api::call(rt, "net/minecraft/world/entity/Mob.setTarget.(Lnet/minecraft/world/entity/LivingEntity;)V", &[e, other]);
            assert_eq!(target(rt, e), other);
            // A control's own class climbs to the vanilla one it extends.
            assert_eq!(api_super(SMOOTH_LOOK), Some(LOOK_CONTROL));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const BREED_GOAL: &str = "net/minecraft/world/entity/ai/goal/BreedGoal";
    const BABY_SPAWN: &str = "net/neoforged/neoforge/event/entity/living/BabyEntitySpawnEvent";

    /// The penguin's own `getBreedOffspring` (Waddles' makes one of its type); nothing else of its
    /// class is its own.
    fn breeding_overrides(class: &str, key: &str) -> bool {
        class == PENGUIN && key.starts_with("getBreedOffspring.")
    }

    fn breeding_vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if short.starts_with("getBreedOffspring.") {
            let ty = rt.get_field(args[0], "$type");
            let baby = rt.new_object(PENGUIN);
            api::call(rt, INIT, &[baby, ty, args[1]]);
            return baby;
        }
        api::call(rt, key, args)
    }

    /// Penguins with a `BreedGoal` each, in the level at `at`.
    fn breeders(rt: &mut Rt, ty: V, at: &[[f64; 3]]) -> Vec<V> {
        let mut all = Vec::new();
        for &p in at {
            let e = penguin(rt, ty, p);
            assert!(crate::mod_entities::join(rt, e, false));
            let g = rt.new_object(BREED_GOAL);
            api::call(rt, "net/minecraft/world/entity/ai/goal/BreedGoal.<init>.(Lnet/minecraft/world/entity/animal/Animal;D)V", &[g, e, V::D(1.0)]);
            let sel = obj(rt, e, "goalSelector");
            api::call(rt, "net/minecraft/world/entity/ai/goal/GoalSelector.addGoal.(ILnet/minecraft/world/entity/ai/goal/Goal;)V", &[sel, V::I(2), g]);
            all.push(e);
        }
        all
    }

    fn babies(rt: &mut Rt) -> Vec<V> {
        let all = crate::mod_entities::in_box(rt, [-8.0, 0.0, -8.0, 8.0, 4.0, 8.0]);
        all.into_iter().filter(|&e| int(rt, e, "$age") < 0).collect()
    }

    /// Two penguins in love, one fed by a player, find each other (the nearer one out of love
    /// isn't taken) and, three seconds of the goal's ticks on, have a baby where one of them
    /// stands; both rest 6000 ticks out of love, an orb of 1 to 7 points drops, and the player
    /// has bred an animal (the statistic, the advancement trigger with the three of them).
    #[test]
    fn two_animals_in_love_have_a_baby_and_rest() {
        let (mut rt, ty) = runtime();
        rt.overrides = Some(breeding_overrides);
        rt.vcall = Some(breeding_vcall);
        let mut host = floor();
        host.player_id = Some(7);
        rt.with_host(&mut host, |rt| {
            let all = breeders(rt, ty, &[[0.5, 1.0, 0.5], [2.5, 1.0, 0.5], [1.5, 1.0, 1.5]]);
            let (a, b, c) = (all[0], all[1], all[2]);
            let player = crate::menus::server_player(rt, 7);
            set_in_love(rt, a, player);
            set_in_love(rt, b, V::NULL);
            let mut born = None;
            for t in 1..=80 {
                for &e in &all {
                    step(rt, e);
                }
                if let Some(&baby) = babies(rt).first() {
                    born = Some((t, baby));
                    break;
                }
            }
            let (t, baby) = born.expect("a baby");
            assert!((52..=64).contains(&t), "born on tick {t}: 30 of the goal's every-other-tick runs");
            assert_eq!(babies(rt), [baby], "one baby");
            assert_eq!(int(rt, baby, "$age"), -24000);
            assert_eq!(rt.class_of(baby), PENGUIN);
            let at = position(rt, baby);
            let near = |e: V| {
                let p = position(rt, e);
                (p[0] - at[0]).abs() < 0.5 && (p[2] - at[2]).abs() < 0.5
            };
            assert!(near(a) || near(b), "where a parent stands: {at:?}");
            for e in [a, b] {
                assert!((5999..=6000).contains(&int(rt, e, "$age")), "resting: {}", int(rt, e, "$age"));
                assert_eq!(int(rt, e, "inLove"), 0);
            }
            assert_eq!((int(rt, c, "$age"), int(rt, c, "inLove")), (0, 0), "the one out of love has no part");
        });
        assert_eq!(host.orbs.len(), 1);
        assert!((1..=7).contains(&host.orbs[0].1), "{:?}", host.orbs);
        assert_eq!(host.orbs[0].0[1], 1.0);
        assert!(host.stats.contains(&(7, "minecraft:custom".into(), "minecraft:animals_bred".into(), 1)), "{:?}", host.stats);
        assert_eq!(host.bred.len(), 1);
        let (player, parent, partner, child) = &host.bred[0];
        assert_eq!(*player, 7);
        assert_eq!((parent.kind.as_str(), parent.baby, partner.kind.as_str(), partner.baby), ("test:penguin", false, "test:penguin", false));
        assert!(child.as_ref().is_some_and(|c| c.kind == "test:penguin" && c.baby));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    fn cancel_babies(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        let cause = api::call(rt, "net/neoforged/neoforge/event/entity/living/BabyEntitySpawnEvent.getCausedByPlayer.()Lnet/minecraft/world/entity/player/Player;", &[ev]);
        let child = api::call(rt, "net/neoforged/neoforge/event/entity/living/BabyEntitySpawnEvent.getChild.()Lnet/minecraft/world/entity/AgeableMob;", &[ev]);
        rt.interned.insert("test:cause".into(), cause);
        rt.interned.insert("test:child".into(), child);
        api::call(rt, "net/neoforged/bus/api/ICancellableEvent.setCanceled.(Z)V", &[ev, V::I(1)]);
        Some(V::Void)
    }

    /// A listener canceling `BabyEntitySpawnEvent` (it was told of the player who fed them and the
    /// baby proposed) leaves the parents resting out of love with no baby, orb or credit.
    #[test]
    fn a_listener_cancels_the_baby() {
        let (mut rt, ty) = runtime();
        rt.overrides = Some(breeding_overrides);
        rt.vcall = Some(breeding_vcall);
        rt.lambda_call = Some(cancel_babies);
        let l = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        rt.listeners.push((BABY_SPAWN.to_owned(), l));
        let mut host = floor();
        host.player_id = Some(7);
        rt.with_host(&mut host, |rt| {
            let all = breeders(rt, ty, &[[0.5, 1.0, 0.5], [1.5, 1.0, 0.5]]);
            let player = crate::menus::server_player(rt, 7);
            set_in_love(rt, all[0], V::NULL);
            set_in_love(rt, all[1], player);
            for _ in 0..80 {
                for &e in &all {
                    step(rt, e);
                }
            }
            assert!(babies(rt).is_empty());
            for &e in &all {
                assert!((5900..=6000).contains(&int(rt, e, "$age")), "resting: {}", int(rt, e, "$age"));
                assert_eq!(int(rt, e, "inLove"), 0);
            }
            assert_eq!(rt.interned.get("test:cause").copied(), Some(player), "the second parent's player");
            assert!(rt.interned.get("test:child").is_some_and(|c| c.r() != 0));
        });
        assert!(host.orbs.is_empty() && host.bred.is_empty());
        assert!(!host.stats.iter().any(|s| s.2 == "minecraft:animals_bred"));
    }
}
