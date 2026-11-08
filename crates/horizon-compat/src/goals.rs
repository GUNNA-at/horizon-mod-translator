//! Mobs' goals, as a mod's mob runs them: vanilla's `Goal` (its flags and the defaults a mod's goal
//! inherits), `WrappedGoal` and `GoalSelector` (1.21.1's choice of the goals that run each tick: by
//! priority, each locking the controls it needs, `Goal.Flag`), `TargetingConditions`, `Sensing`,
//! and the vanilla goals mods build their mobs of: floating, strolling (also away from water),
//! looking at players and around, panicking, melee attacks, leaping at a target, following an item
//! held out and a parent, keeping away from an entity, and the target goals (the one that hurt it,
//! the nearest it may attack).
//!
//! A goal is the object of its class (a mod's, or vanilla's when the mod makes one: `new
//! RandomStrollGoal(this, 1.0)`), its state in fields of vanilla's names where subclasses read them
//! (`mob`, `speedModifier`, `target`...). A call on one runs the mod's own method when its class has
//! one, else the vanilla goal's its class extends ([`kind`]): the selector calls them virtually, as
//! the JVM does. A vanilla goal this module doesn't model is a plain `Goal` that never starts.
//!
//! The selector keeps its goals in a set (`getAvailableGoals()`, which mods read and filter), each
//! `WrappedGoal` with its priority and whether it runs, and which goal holds each flag.
use crate::{Rt, V, api, jutil, mobs};

pub(crate) const GOAL: &str = "net/minecraft/world/entity/ai/goal/Goal";
pub(crate) const WRAPPED: &str = "net/minecraft/world/entity/ai/goal/WrappedGoal";
pub(crate) const SELECTOR: &str = "net/minecraft/world/entity/ai/goal/GoalSelector";
pub(crate) const FLAG: &str = "net/minecraft/world/entity/ai/goal/Goal$Flag";
const FLAG_NAMES: [&str; 4] = ["MOVE", "LOOK", "JUMP", "TARGET"];
pub(crate) const MOVE: u8 = 1;
pub(crate) const LOOK: u8 = 2;
pub(crate) const JUMP: u8 = 4;
pub(crate) const TARGET: u8 = 8;

const FLOAT: &str = "net/minecraft/world/entity/ai/goal/FloatGoal";
const LOOK_AT_PLAYER: &str = "net/minecraft/world/entity/ai/goal/LookAtPlayerGoal";
const RANDOM_LOOK: &str = "net/minecraft/world/entity/ai/goal/RandomLookAroundGoal";
const STROLL: &str = "net/minecraft/world/entity/ai/goal/RandomStrollGoal";
const WATER_AVOIDING: &str = "net/minecraft/world/entity/ai/goal/WaterAvoidingRandomStrollGoal";
const PANIC: &str = "net/minecraft/world/entity/ai/goal/PanicGoal";
const MELEE: &str = "net/minecraft/world/entity/ai/goal/MeleeAttackGoal";
const LEAP: &str = "net/minecraft/world/entity/ai/goal/LeapAtTargetGoal";
const TEMPT: &str = "net/minecraft/world/entity/ai/goal/TemptGoal";
const FOLLOW_PARENT: &str = "net/minecraft/world/entity/ai/goal/FollowParentGoal";
const AVOID: &str = "net/minecraft/world/entity/ai/goal/AvoidEntityGoal";
const TARGET_GOAL: &str = "net/minecraft/world/entity/ai/goal/target/TargetGoal";
const HURT_BY: &str = "net/minecraft/world/entity/ai/goal/target/HurtByTargetGoal";
const NEAREST: &str = "net/minecraft/world/entity/ai/goal/target/NearestAttackableTargetGoal";
pub(crate) const CONDITIONS: &str = "net/minecraft/world/entity/ai/targeting/TargetingConditions";
pub(crate) const SENSING: &str = "net/minecraft/world/entity/ai/sensing/Sensing";
const PLAYER: &str = "net/minecraft/world/entity/player/Player";
const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const LIVING: &str = "net/minecraft/world/entity/LivingEntity";

/// The vanilla goals modelled here, `Goal`'s own defaults first.
const MODELLED: [&str; 15] =
    [GOAL, FLOAT, LOOK_AT_PLAYER, RANDOM_LOOK, STROLL, WATER_AVOIDING, PANIC, MELEE, LEAP, TEMPT, FOLLOW_PARENT, AVOID, TARGET_GOAL, HURT_BY, NEAREST];

pub(crate) const CAN_USE: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.canUse.()Z", "canUse.()Z");
const CAN_CONTINUE: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.canContinueToUse.()Z", "canContinueToUse.()Z");
const START: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.start.()V", "start.()V");
const STOP: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.stop.()V", "stop.()V");
const TICK: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.tick.()V", "tick.()V");
const EVERY_TICK: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.requiresUpdateEveryTick.()Z", "requiresUpdateEveryTick.()Z");
const INTERRUPTABLE: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.isInterruptable.()Z", "isInterruptable.()Z");
const GET_FLAGS: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.getFlags.()Ljava/util/EnumSet;", "getFlags.()Ljava/util/EnumSet;");
const ADJUSTED: (&str, &str) = ("net/minecraft/world/entity/ai/goal/Goal.adjustedTickDelay.(I)I", "adjustedTickDelay.(I)I");
const GET_POSITION: (&str, &str) = ("net/minecraft/world/entity/ai/goal/RandomStrollGoal.getPosition.()Lnet/minecraft/world/phys/Vec3;", "getPosition.()Lnet/minecraft/world/phys/Vec3;");
const CHECK_AND_ATTACK: (&str, &str) = (
    "net/minecraft/world/entity/ai/goal/MeleeAttackGoal.checkAndPerformAttack.(Lnet/minecraft/world/entity/LivingEntity;)V",
    "checkAndPerformAttack.(Lnet/minecraft/world/entity/LivingEntity;)V",
);
const CAN_PERFORM_ATTACK: (&str, &str) = (
    "net/minecraft/world/entity/ai/goal/MeleeAttackGoal.canPerformAttack.(Lnet/minecraft/world/entity/LivingEntity;)Z",
    "canPerformAttack.(Lnet/minecraft/world/entity/LivingEntity;)Z",
);
const RESET_COOLDOWN: (&str, &str) = ("net/minecraft/world/entity/ai/goal/MeleeAttackGoal.resetAttackCooldown.()V", "resetAttackCooldown.()V");
const FIND_TARGET: (&str, &str) = ("net/minecraft/world/entity/ai/goal/target/NearestAttackableTargetGoal.findTarget.()V", "findTarget.()V");
const SEARCH_AREA: (&str, &str) = (
    "net/minecraft/world/entity/ai/goal/target/NearestAttackableTargetGoal.getTargetSearchArea.(D)Lnet/minecraft/world/phys/AABB;",
    "getTargetSearchArea.(D)Lnet/minecraft/world/phys/AABB;",
);
const FOLLOW_DISTANCE: (&str, &str) = ("net/minecraft/world/entity/ai/goal/target/TargetGoal.getFollowDistance.()D", "getFollowDistance.()D");
const ALERT_OTHER: (&str, &str) = (
    "net/minecraft/world/entity/ai/goal/target/HurtByTargetGoal.alertOther.(Lnet/minecraft/world/entity/Mob;Lnet/minecraft/world/entity/LivingEntity;)V",
    "alertOther.(Lnet/minecraft/world/entity/Mob;Lnet/minecraft/world/entity/LivingEntity;)V",
);
const SHOULD_PANIC: (&str, &str) = ("net/minecraft/world/entity/ai/goal/PanicGoal.findRandomPosition.()Z", "findRandomPosition.()Z");
const PREDICATE_TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const NONE_OF: &str = "java/util/EnumSet.noneOf.(Ljava/lang/Class;)Ljava/util/EnumSet;";

// ---- small helpers ----

fn int(rt: &Rt, o: V, f: &'static str) -> i32 {
    match rt.get_field(o, f) {
        V::I(i) => i,
        V::J(j) => j as i32,
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

fn dbl(rt: &Rt, o: V, f: &'static str) -> f64 {
    match rt.get_field(o, f) {
        V::D(d) => d,
        V::F(x) => f64::from(x),
        V::I(i) => f64::from(i),
        _ => 0.0,
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

/// `Goal.reducedTickDelay(ticks)`: `Mth.positiveCeilDiv(ticks, 2)`.
pub(crate) fn reduced(ticks: i32) -> i32 {
    -((-ticks).div_euclid(2))
}

/// The goal's `adjustedTickDelay(ticks)`, virtually.
fn adjusted(rt: &mut Rt, goal: V, ticks: i32) -> i32 {
    api::virtual_call(rt, ADJUSTED, &[goal, V::I(ticks)]).i()
}

fn call_bool(rt: &mut Rt, key: (&'static str, &'static str), args: &[V]) -> bool {
    let r = api::virtual_call(rt, key, args).i() != 0;
    rt.report_thrown(key.1);
    r
}

fn call_void(rt: &mut Rt, key: (&'static str, &'static str), args: &[V]) {
    api::virtual_call(rt, key, args);
    rt.report_thrown(key.1);
}

/// A `Goal.Flag` constant (`MOVE`...), the one its static field gives.
pub(crate) fn flag(rt: &mut Rt, i: usize) -> V {
    api::api_enum(rt, FLAG, FLAG_NAMES[i.min(3)], i.min(3) as i32)
}

/// The `EnumSet` of the flags in `bits`.
fn flag_set(rt: &mut Rt, bits: u8) -> V {
    let class = rt.class_literal(FLAG);
    let set = api::call(rt, NONE_OF, &[class]);
    for i in 0..4 {
        if bits & (1 << i) != 0 {
            let f = flag(rt, i);
            jutil::table_put(rt, set, f, V::I(1));
        }
    }
    set
}

/// The bits of the flags in `set` (an `EnumSet` of `Goal.Flag`).
fn bits_of(rt: &Rt, set: V) -> u8 {
    jutil::elements(rt, set).into_iter().fold(0, |b, f| match rt.get_field(f, "$ordinal") {
        V::I(i @ 0..=3) => b | 1 << i,
        _ => b,
    })
}

/// A goal's own flags (`Goal.flags`), made when first asked for.
fn own_flags(rt: &mut Rt, goal: V) -> V {
    match rt.get_field(goal, "$flags") {
        v @ V::R(r) if r != 0 => v,
        _ => {
            let set = flag_set(rt, 0);
            rt.put_field(goal, "$flags", set);
            set
        }
    }
}

/// `goal.setFlags(...)` of the flags in `bits`, as a goal's constructor sets them.
fn set_flags(rt: &mut Rt, goal: V, bits: u8) {
    let set = flag_set(rt, bits);
    rt.put_field(goal, "$flags", set);
}

/// Whether `v`'s class (a mod's) has its own `key` (`name.desc`).
fn overridden(rt: &Rt, v: V, key: &str) -> bool {
    v.r() != 0 && rt.overrides.is_some_and(|o| o(rt.class_of(v), key))
}

/// A goal's flags: its `getFlags()` (virtual), as bits.
fn flags_of(rt: &mut Rt, goal: V) -> u8 {
    if overridden(rt, goal, GET_FLAGS.1) {
        let set = api::virtual_call(rt, GET_FLAGS, &[goal]);
        return bits_of(rt, set);
    }
    let set = own_flags(rt, goal);
    bits_of(rt, set)
}

/// The first vanilla class above `class` (a mod's goal's), or `class` itself.
fn first_vanilla(rt: &Rt, class: &'static str) -> &'static str {
    let mut c = class;
    while let Some(&up) = rt.supers.get(c) {
        c = up;
    }
    c
}

/// The vanilla goal whose behaviour goal `g` has: its first vanilla class, or the nearest modelled
/// one above that (a `ZombieAttackGoal` attacks as a `MeleeAttackGoal`); `Goal` for the rest.
fn kind(rt: &Rt, g: V) -> &'static str {
    let mut c = first_vanilla(rt, rt.class_of(g));
    loop {
        if MODELLED.contains(&c) {
            return c;
        }
        match api_super(c) {
            Some(s) => c = s,
            None => return GOAL,
        }
    }
}

/// The superclass of a vanilla goal class (1.21.1's): what a call on a goal of one this module
/// doesn't model climbs to (`retry_classes`, `api::api_super`).
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    const G: &str = "net/minecraft/world/entity/ai/goal/";
    let rest = class.strip_prefix(G)?;
    Some(match rest {
        "Goal" | "WrappedGoal" | "GoalSelector" => return None,
        "WaterAvoidingRandomStrollGoal" | "RandomSwimmingGoal" => STROLL,
        "WaterAvoidingRandomFlyingGoal" => WATER_AVOIDING,
        "ZombieAttackGoal" => MELEE,
        "OpenDoorGoal" | "BreakDoorGoal" => "net/minecraft/world/entity/ai/goal/DoorInteractGoal",
        "RemoveBlockGoal" => "net/minecraft/world/entity/ai/goal/MoveToBlockGoal",
        "target/TargetGoal" | "target/ResetUniversalAngerTargetGoal" => GOAL,
        "target/NonTameRandomTargetGoal" | "target/NearestAttackableWitchTargetGoal" | "target/NearestHealableRaiderTargetGoal" => NEAREST,
        r if r.starts_with("target/") => TARGET_GOAL,
        r if !r.contains('$') => GOAL,
        _ => return None,
    })
}

/// The mob a goal acts for (`this.mob`; a parent-follower's `animal`).
fn mob_of(rt: &Rt, g: V) -> V {
    match obj(rt, g, "mob") {
        m if m.r() != 0 => m,
        _ => obj(rt, g, "animal"),
    }
}

// ---- the selector ----

/// The selector's goals (`availableGoals`), made when first asked for: a set of `WrappedGoal`s, in
/// the order they were added.
fn available(rt: &mut Rt, sel: V) -> V {
    match rt.get_field(sel, "availableGoals") {
        v @ V::R(r) if r != 0 => v,
        _ => {
            let s = jutil::identity_set(rt, &[]);
            rt.put_field(sel, "availableGoals", s);
            s
        }
    }
}

fn goals(rt: &mut Rt, sel: V) -> Vec<V> {
    let set = available(rt, sel);
    jutil::elements(rt, set)
}

/// A new selector (`new GoalSelector(profiler)`).
pub(crate) fn new_selector(rt: &mut Rt) -> V {
    let s = rt.new_object(SELECTOR);
    available(rt, s);
    s
}

fn wrapped_goal(rt: &Rt, w: V) -> V {
    obj(rt, w, "goal")
}

fn running(rt: &Rt, w: V) -> bool {
    truth(rt, w, "isRunning")
}

fn priority(rt: &Rt, w: V) -> i32 {
    int(rt, w, "priority")
}

fn wrap(rt: &mut Rt, priority: i32, goal: V) -> V {
    let w = rt.new_object(WRAPPED);
    rt.put_field(w, "goal", goal);
    rt.put_field(w, "priority", V::I(priority));
    put_bool(rt, w, "isRunning", false);
    w
}

/// `WrappedGoal.start()`: the goal starts unless it runs.
fn start_wrapped(rt: &mut Rt, w: V) {
    if !running(rt, w) {
        put_bool(rt, w, "isRunning", true);
        let g = wrapped_goal(rt, w);
        call_void(rt, START, &[g]);
    }
}

/// `WrappedGoal.stop()`: a running goal stops.
fn stop_wrapped(rt: &mut Rt, w: V) {
    if running(rt, w) {
        put_bool(rt, w, "isRunning", false);
        let g = wrapped_goal(rt, w);
        call_void(rt, STOP, &[g]);
    }
}

/// The goal holding flag `i` (`lockedFlags`), null for none.
fn lock(rt: &Rt, sel: V, i: usize) -> V {
    obj(rt, sel, ["$lockMove", "$lockLook", "$lockJump", "$lockTarget"][i])
}

fn set_lock(rt: &mut Rt, sel: V, i: usize, w: V) {
    rt.put_field(sel, ["$lockMove", "$lockLook", "$lockJump", "$lockTarget"][i], w);
}

/// `addGoal(priority, goal)`: wrapped, unless the set has it (`WrappedGoal.equals` is its goal's).
fn add_goal(rt: &mut Rt, sel: V, priority: i32, goal: V) {
    if goals(rt, sel).iter().any(|&w| wrapped_goal(rt, w) == goal) {
        return;
    }
    let w = wrap(rt, priority, goal);
    let set = available(rt, sel);
    jutil::table_put(rt, set, w, V::I(1));
}

/// `removeGoal(goal)`: stopped if it runs, then gone.
fn remove_goal(rt: &mut Rt, sel: V, goal: V) {
    for w in goals(rt, sel) {
        if wrapped_goal(rt, w) == goal {
            stop_wrapped(rt, w);
            let set = available(rt, sel);
            jutil::table_remove(rt, set, w);
        }
    }
}

/// `removeAllGoals(filter)`: the goals the predicate takes, gone (not stopped, as vanilla's).
fn remove_all(rt: &mut Rt, sel: V, filter: V) {
    for w in goals(rt, sel) {
        let g = wrapped_goal(rt, w);
        if filter.r() == 0 || rt.call_functional(PREDICATE_TEST, filter, &[g]).i() != 0 {
            let set = available(rt, sel);
            jutil::table_remove(rt, set, w);
        }
    }
}

/// `GoalSelector.tick()`: running goals that a disabled flag or their `canContinueToUse` stops,
/// stop; the flags they held are freed; then each goal not running, with no disabled flag, that
/// may take every flag it needs from the goal holding it (a free flag always) and `canUse`, starts,
/// stopping those it takes them from; then the running goals tick.
pub(crate) fn selector_tick(rt: &mut Rt, sel: V) {
    let all = goals(rt, sel);
    let disabled = int(rt, sel, "$disabled") as u8;
    for &w in &all {
        if running(rt, w) {
            let g = wrapped_goal(rt, w);
            if flags_of(rt, g) & disabled != 0 || !call_bool(rt, CAN_CONTINUE, &[g]) {
                stop_wrapped(rt, w);
            }
        }
    }
    for i in 0..4 {
        let l = lock(rt, sel, i);
        if l.r() != 0 && !running(rt, l) {
            set_lock(rt, sel, i, V::NULL);
        }
    }
    for &w in &all {
        if running(rt, w) {
            continue;
        }
        let g = wrapped_goal(rt, w);
        let flags = flags_of(rt, g);
        if flags & disabled != 0 {
            continue;
        }
        let p = priority(rt, w);
        let mut replaceable = true;
        for i in 0..4 {
            if flags & (1 << i) == 0 {
                continue;
            }
            let holder = lock(rt, sel, i);
            // A free flag is held by vanilla's `NO_GOAL`, of the highest priority.
            let ok = if holder.r() == 0 {
                p < i32::MAX
            } else {
                let hg = wrapped_goal(rt, holder);
                call_bool(rt, INTERRUPTABLE, &[hg]) && p < priority(rt, holder)
            };
            if !ok {
                replaceable = false;
                break;
            }
        }
        if !replaceable || !call_bool(rt, CAN_USE, &[g]) {
            continue;
        }
        for i in 0..4 {
            if flags & (1 << i) != 0 {
                let old = lock(rt, sel, i);
                if old.r() != 0 {
                    stop_wrapped(rt, old);
                }
                set_lock(rt, sel, i, w);
            }
        }
        start_wrapped(rt, w);
    }
    tick_running(rt, sel, true);
}

/// `tickRunningGoals(tickAllRunning)`: each running goal ticks; on the ticks between the
/// selector's full ones, only those that ask to every tick (`requiresUpdateEveryTick`).
pub(crate) fn tick_running(rt: &mut Rt, sel: V, all: bool) {
    for w in goals(rt, sel) {
        if !running(rt, w) {
            continue;
        }
        let g = wrapped_goal(rt, w);
        if all || call_bool(rt, EVERY_TICK, &[g]) {
            call_void(rt, TICK, &[g]);
        }
    }
}

/// `setControlFlag(flag, enabled)` by bits.
pub(crate) fn set_control(rt: &mut Rt, sel: V, bits: u8, enabled: bool) {
    let d = int(rt, sel, "$disabled") as u8;
    let d = if enabled { d & !bits } else { d | bits };
    rt.put_field(sel, "$disabled", V::I(i32::from(d)));
}

/// The goals of `sel` running now (vanilla's `getRunningGoals()` of old, which mods still ask by
/// filtering the set).
pub(crate) fn running_goals(rt: &mut Rt, sel: V) -> Vec<V> {
    goals(rt, sel).into_iter().filter(|&w| running(rt, w)).map(|w| wrapped_goal(rt, w)).collect()
}

// ---- dispatch ----

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    owner == SELECTOR
        || owner == CONDITIONS
        || owner == SENSING
        || owner.starts_with("net/minecraft/world/entity/ai/goal/")
        || (api::is_level(owner) && matches!(name, "getNearestPlayer" | "getNearestEntity" | "getNearbyPlayers" | "getNearbyEntities") && desc.contains("TargetingConditions"))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if api::is_level(owner) {
        return level_call(rt, name, desc, args);
    }
    match owner {
        SELECTOR => return selector_call(rt, name, desc, args),
        CONDITIONS => return conditions_call(rt, name, desc, args),
        SENSING => return sensing_call(rt, name, args),
        WRAPPED => return wrapped_call(rt, name, desc, args),
        _ => {}
    }
    if name == "<init>" {
        return construct(rt, owner, desc, args);
    }
    if name == "reducedTickDelay" && desc == "(I)I" {
        return Some(V::I(reduced(a(0).i())));
    }
    let g = a(0);
    if g.r() == 0 {
        return goal_default(rt, GOAL, name, desc, args);
    }
    let k = kind(rt, g);
    goal_default(rt, k, name, desc, args)
}

/// `WrappedGoal`'s own: its goal's answers, its priority and whether it runs.
fn wrapped_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let w = a(0);
    let g = wrapped_goal(rt, w);
    Some(match name {
        "<init>" => {
            rt.put_field(w, "goal", a(2));
            rt.put_field(w, "priority", a(1));
            put_bool(rt, w, "isRunning", false);
            V::Void
        }
        "canBeReplacedBy" => V::I(i32::from(call_bool(rt, INTERRUPTABLE, &[g]) && priority(rt, a(1)) < priority(rt, w))),
        "canUse" => V::I(i32::from(call_bool(rt, CAN_USE, &[g]))),
        "canContinueToUse" => V::I(i32::from(call_bool(rt, CAN_CONTINUE, &[g]))),
        "isInterruptable" => V::I(i32::from(call_bool(rt, INTERRUPTABLE, &[g]))),
        "requiresUpdateEveryTick" => V::I(i32::from(call_bool(rt, EVERY_TICK, &[g]))),
        "start" => {
            start_wrapped(rt, w);
            V::Void
        }
        "stop" => {
            stop_wrapped(rt, w);
            V::Void
        }
        "tick" => {
            call_void(rt, TICK, &[g]);
            V::Void
        }
        "adjustedTickDelay" => V::I(adjusted(rt, g, a(1).i())),
        "setFlags" => {
            api::virtual_call(rt, ("net/minecraft/world/entity/ai/goal/Goal.setFlags.(Ljava/util/EnumSet;)V", "setFlags.(Ljava/util/EnumSet;)V"), &[g, a(1)]);
            V::Void
        }
        "getFlags" => api::virtual_call(rt, GET_FLAGS, &[g]),
        "isRunning" => V::I(i32::from(running(rt, w))),
        "getPriority" => V::I(priority(rt, w)),
        "getGoal" => g,
        "equals" => V::I(i32::from(a(1) == w || (a(1).r() != 0 && rt.class_of(a(1)) == WRAPPED && wrapped_goal(rt, a(1)) == g))),
        "hashCode" => V::I(g.r() as i32),
        "toString" => {
            let s = format!("{}", rt.class_of(g).rsplit('/').next().unwrap_or(""));
            rt.string(&s)
        }
        _ => return goal_default(rt, GOAL, name, desc, args),
    })
}

fn selector_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let sel = a(0);
    Some(match name {
        "<init>" => {
            available(rt, sel);
            V::Void
        }
        "addGoal" => {
            add_goal(rt, sel, a(1).i(), a(2));
            V::Void
        }
        "removeGoal" => {
            remove_goal(rt, sel, a(1));
            V::Void
        }
        "removeAllGoals" => {
            remove_all(rt, sel, a(1));
            V::Void
        }
        "tick" => {
            selector_tick(rt, sel);
            V::Void
        }
        "tickRunningGoals" => {
            tick_running(rt, sel, a(1).i() != 0);
            V::Void
        }
        "getAvailableGoals" => available(rt, sel),
        // 1.20's stream of the running ones, which old code still calls.
        "getRunningGoals" => {
            let items: Vec<V> = goals(rt, sel).into_iter().filter(|&w| running(rt, w)).collect();
            rt.alloc("java/util/stream/Stream", crate::Data::Stream(items))
        }
        "disableControlFlag" | "enableControlFlag" | "setControlFlag" => {
            let bit = match rt.get_field(a(1), "$ordinal") {
                V::I(i @ 0..=3) => 1u8 << i,
                _ => 0,
            };
            let enabled = match name {
                "enableControlFlag" => true,
                "disableControlFlag" => false,
                _ => a(2).i() != 0,
            };
            set_control(rt, sel, bit, enabled);
            V::Void
        }
        _ => {
            let _ = desc;
            return None;
        }
    })
}

// ---- goals' constructors ----

/// A vanilla goal's constructor, on a goal of its class or a mod's below it: its fields and flags.
fn construct(rt: &mut Rt, owner: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let g = a(0);
    let mob = a(1);
    match owner {
        GOAL => {
            own_flags(rt, g);
        }
        FLOAT => {
            rt.put_field(g, "mob", mob);
            set_flags(rt, g, JUMP);
            // `mob.getNavigation().setCanFloat(true)`.
            if mob.r() != 0 {
                let nav = mobs::navigation(rt, mob);
                crate::pathing::set_can_float(rt, nav, true);
            }
        }
        LOOK_AT_PLAYER => {
            // `(mob, type, distance[, probability[, onlyHorizontal]])`.
            let probability = if desc.contains("FF") { a(4).f() } else { 0.02 };
            let horizontal = desc.contains("FFZ") && a(5).i() != 0;
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "lookAtType", a(2));
            rt.put_field(g, "lookDistance", V::F(a(3).f()));
            rt.put_field(g, "probability", V::F(probability));
            put_bool(rt, g, "onlyHorizontal", horizontal);
            let c = new_conditions(rt, false);
            rt.put_field(c, "range", V::D(f64::from(a(3).f())));
            rt.put_field(g, "lookAtContext", c);
            set_flags(rt, g, LOOK);
        }
        RANDOM_LOOK => {
            rt.put_field(g, "mob", mob);
            set_flags(rt, g, MOVE | LOOK);
        }
        STROLL | WATER_AVOIDING => {
            // `(mob, speed[, interval[, checkNoActionTime]])`; water-avoiding: `(mob, speed[,
            // probability])`.
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "speedModifier", V::D(a(2).d()));
            let interval = if owner == STROLL && desc.contains("DI") { a(3).i() } else { 120 };
            rt.put_field(g, "interval", V::I(interval));
            let check = !(owner == STROLL && desc.contains("DIZ")) || a(4).i() != 0;
            put_bool(rt, g, "checkNoActionTime", check);
            put_bool(rt, g, "forceTrigger", false);
            if owner == WATER_AVOIDING {
                let p = if desc.contains("DF") { a(3).f() } else { 0.001 };
                rt.put_field(g, "probability", V::F(p));
            }
            set_flags(rt, g, MOVE);
        }
        PANIC => {
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "speedModifier", V::D(a(2).d()));
            // A predicate or a damage tag (or a function to one) of what panics it: vanilla's
            // `PANIC_CAUSES` by default.
            if desc.contains("Ljava/util/function/Predicate;") {
                rt.put_field(g, "shouldPanic", a(3));
            } else if desc.contains("Lnet/minecraft/tags/TagKey;") {
                let tag = rt.get_field(a(3), "$location");
                rt.put_field(g, "$panicTag", tag);
            } else if desc.contains("Ljava/util/function/Function;") {
                rt.put_field(g, "$panicTagOf", a(3));
            }
            set_flags(rt, g, MOVE);
        }
        MELEE => {
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "speedModifier", V::D(a(2).d()));
            put_bool(rt, g, "followingTargetEvenIfNotSeen", a(3).i() != 0);
            rt.put_field(g, "ticksUntilNextAttack", V::I(0));
            rt.put_field(g, "ticksUntilNextPathRecalculation", V::I(0));
            rt.put_field(g, "lastCanUseCheck", V::J(i64::MIN / 2));
            set_flags(rt, g, MOVE | LOOK);
        }
        LEAP => {
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "yd", V::F(a(2).f()));
            set_flags(rt, g, JUMP | MOVE);
        }
        TEMPT => {
            // `(mob, speed, items, canScare)`: items a predicate of stacks (1.21.1), or an
            // `Ingredient` (old code).
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "speedModifier", V::D(a(2).d()));
            rt.put_field(g, "items", a(3));
            put_bool(rt, g, "canScare", a(4).i() != 0);
            let c = new_conditions(rt, false);
            rt.put_field(c, "range", V::D(10.0));
            put_bool(rt, c, "checkLineOfSight", false);
            rt.put_field(g, "targetingConditions", c);
            set_flags(rt, g, MOVE | LOOK);
        }
        FOLLOW_PARENT => {
            rt.put_field(g, "animal", mob);
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "speedModifier", V::D(a(2).d()));
        }
        AVOID => {
            // `(mob, class, maxDist, walkSpeed, sprintSpeed[, predicate...])`.
            rt.put_field(g, "mob", mob);
            rt.put_field(g, "avoidClass", a(2));
            let (dist, walk, sprint) = if desc.contains("Ljava/lang/Class;FDD") {
                (a(3).f(), a(4).d(), a(5).d())
            } else if desc.contains("Ljava/util/function/Predicate;FDD") {
                (a(4).f(), a(5).d(), a(6).d())
            } else {
                (a(3).f(), a(4).d(), a(5).d())
            };
            rt.put_field(g, "maxDist", V::F(dist));
            rt.put_field(g, "walkSpeedModifier", V::D(walk));
            rt.put_field(g, "sprintSpeedModifier", V::D(sprint));
            let c = new_conditions(rt, true);
            rt.put_field(c, "range", V::D(f64::from(dist)));
            rt.put_field(g, "avoidEntityTargeting", c);
            set_flags(rt, g, MOVE);
        }
        TARGET_GOAL => {
            rt.put_field(g, "mob", mob);
            put_bool(rt, g, "mustSee", a(2).i() != 0);
            put_bool(rt, g, "mustReach", desc.contains("ZZ") && a(3).i() != 0);
            rt.put_field(g, "unseenMemoryTicks", V::I(60));
        }
        HURT_BY => {
            target_init(rt, g, mob, true, false);
            rt.put_field(g, "toIgnoreDamage", a(2));
            set_flags(rt, g, TARGET);
        }
        NEAREST => {
            // `(mob, type, mustSee[, predicate])`, `(mob, type, mustSee, mustReach)`, `(mob, type,
            // interval, mustSee, mustReach, predicate)`.
            let (interval, must_see, must_reach, predicate) = if desc.starts_with("(Lnet/minecraft/world/entity/Mob;Ljava/lang/Class;IZZ") {
                (a(3).i(), a(4).i() != 0, a(5).i() != 0, a(6))
            } else if desc.contains("Ljava/lang/Class;ZZ") {
                (10, a(3).i() != 0, a(4).i() != 0, V::NULL)
            } else if desc.contains("Ljava/lang/Class;ZLjava/util/function/Predicate;") {
                (10, a(3).i() != 0, false, a(4))
            } else {
                (10, a(3).i() != 0, false, V::NULL)
            };
            target_init(rt, g, mob, must_see, must_reach);
            rt.put_field(g, "targetType", a(2));
            rt.put_field(g, "randomInterval", V::I(reduced(interval)));
            set_flags(rt, g, TARGET);
            let c = new_conditions(rt, true);
            let range = follow_distance(rt, g);
            rt.put_field(c, "range", V::D(range));
            rt.put_field(c, "selector", predicate);
            rt.put_field(g, "targetConditions", c);
        }
        _ => return None,
    }
    Some(V::Void)
}

fn target_init(rt: &mut Rt, g: V, mob: V, must_see: bool, must_reach: bool) {
    rt.put_field(g, "mob", mob);
    put_bool(rt, g, "mustSee", must_see);
    put_bool(rt, g, "mustReach", must_reach);
    rt.put_field(g, "unseenMemoryTicks", V::I(60));
}

/// `TargetGoal.getFollowDistance()` (virtual): the mob's follow range.
fn follow_distance(rt: &mut Rt, g: V) -> f64 {
    if overridden(rt, g, FOLLOW_DISTANCE.1) {
        return api::virtual_call(rt, FOLLOW_DISTANCE, &[g]).d();
    }
    let mob = mob_of(rt, g);
    mobs::attribute(rt, mob, "minecraft:generic.follow_range")
}

// ---- goals' methods ----

/// What goal `args[0]`, of vanilla kind `k`, answers for `name`: its kind's own, else `Goal`'s.
fn goal_default(rt: &mut Rt, k: &'static str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let g = a(0);
    if k != GOAL
        && let Some(v) = kind_call(rt, k, name, desc, args)
    {
        return Some(v);
    }
    Some(match (name, desc) {
        ("canUse", "()Z") => V::I(0),
        ("canContinueToUse", "()Z") => V::I(i32::from(call_bool(rt, CAN_USE, &[g]))),
        ("isInterruptable", "()Z") => V::I(1),
        ("start" | "stop" | "tick", "()V") => V::Void,
        ("requiresUpdateEveryTick", "()Z") => V::I(0),
        ("setFlags", _) => {
            let bits = bits_of(rt, a(1));
            set_flags(rt, g, bits);
            V::Void
        }
        ("getFlags", _) => own_flags(rt, g),
        ("adjustedTickDelay", "(I)I") => {
            let every = call_bool(rt, EVERY_TICK, &[g]);
            V::I(if every { a(1).i() } else { reduced(a(1).i()) })
        }
        ("toString", "()Ljava/lang/String;") => {
            let s = rt.class_of(g).rsplit(['/', '$']).next().unwrap_or("").to_owned();
            rt.string(&s)
        }
        _ => return None,
    })
}

/// The vanilla goal `k`'s own methods.
fn kind_call(rt: &mut Rt, k: &'static str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let g = a(0);
    let mob = mob_of(rt, g);
    let b = |x: bool| V::I(i32::from(x));
    Some(match (k, name) {
        // `FloatGoal`: in water deeper than it can jump from, or lava, it jumps (80% of ticks).
        (FLOAT, "canUse") => {
            let water = mobs::in_water(rt, mob) && mobs::fluid_height(rt, mob, true) > mobs::fluid_jump_threshold(rt, mob);
            b(water || mobs::in_lava(rt, mob))
        }
        (FLOAT, "requiresUpdateEveryTick") => b(true),
        (FLOAT, "tick") => {
            if mobs::random_float(rt, mob) < 0.8 {
                let j = mobs::jump_control(rt, mob);
                crate::mobs::jump(rt, j);
            }
            V::Void
        }

        // `LookAtPlayerGoal`.
        (LOOK_AT_PLAYER, "canUse") => {
            if mobs::random_float(rt, mob) >= flt(rt, g, "probability") {
                return Some(b(false));
            }
            let target = mobs::target(rt, mob);
            if target.r() != 0 {
                rt.put_field(g, "lookAt", target);
            }
            let ty = rt.get_field(g, "lookAtType");
            let context = obj(rt, g, "lookAtContext");
            let eye = mobs::eye_pos(rt, mob);
            let found = if class_named(rt, ty, PLAYER) {
                nearest_player(rt, context, mob, eye)
            } else {
                let d = f64::from(flt(rt, g, "lookDistance"));
                let bb = mobs::bounds(rt, mob);
                let area = [bb[0] - d, bb[1] - 3.0, bb[2] - d, bb[3] + d, bb[4] + 3.0, bb[5] + d];
                let list = entities_of_class(rt, ty, area);
                nearest_of(rt, &list, context, mob, eye)
            };
            rt.put_field(g, "lookAt", found);
            b(found.r() != 0)
        }
        (LOOK_AT_PLAYER, "canContinueToUse") => {
            let at = obj(rt, g, "lookAt");
            if at.r() == 0 || !mobs::is_alive(rt, at) {
                return Some(b(false));
            }
            let d = f64::from(flt(rt, g, "lookDistance"));
            b(mobs::distance_sqr(rt, mob, at) <= d * d && int(rt, g, "lookTime") > 0)
        }
        (LOOK_AT_PLAYER, "start") => {
            let t = 40 + mobs::random_int(rt, mob, 40);
            let t = adjusted(rt, g, t);
            rt.put_field(g, "lookTime", V::I(t));
            V::Void
        }
        (LOOK_AT_PLAYER, "stop") => {
            rt.put_field(g, "lookAt", V::NULL);
            V::Void
        }
        (LOOK_AT_PLAYER, "tick") => {
            let at = obj(rt, g, "lookAt");
            if at.r() != 0 && mobs::is_alive(rt, at) {
                let y = if truth(rt, g, "onlyHorizontal") { mobs::eye_y(rt, mob) } else { mobs::eye_y(rt, at) };
                let p = mobs::position(rt, at);
                let look = mobs::look_control(rt, mob);
                mobs::look_at(rt, look, [p[0], y, p[2]]);
                let t = int(rt, g, "lookTime");
                rt.put_field(g, "lookTime", V::I(t - 1));
            }
            V::Void
        }

        // `RandomLookAroundGoal`: now and then, a second or two looking a random way.
        (RANDOM_LOOK, "canUse") => b(mobs::random_float(rt, mob) < 0.02),
        (RANDOM_LOOK, "canContinueToUse") => b(int(rt, g, "lookTime") >= 0),
        (RANDOM_LOOK, "start") => {
            let angle = std::f64::consts::TAU * mobs::random_double(rt, mob);
            rt.put_field(g, "relX", V::D(angle.cos()));
            rt.put_field(g, "relZ", V::D(angle.sin()));
            let t = 20 + mobs::random_int(rt, mob, 20);
            rt.put_field(g, "lookTime", V::I(t));
            V::Void
        }
        (RANDOM_LOOK, "requiresUpdateEveryTick") => b(true),
        (RANDOM_LOOK, "tick") => {
            let t = int(rt, g, "lookTime");
            rt.put_field(g, "lookTime", V::I(t - 1));
            let p = mobs::position(rt, mob);
            let y = mobs::eye_y(rt, mob);
            let (rx, rz) = (dbl(rt, g, "relX"), dbl(rt, g, "relZ"));
            let look = mobs::look_control(rt, mob);
            mobs::look_at(rt, look, [p[0] + rx, y, p[2] + rz]);
            V::Void
        }

        // `RandomStrollGoal` (and the water-avoiding one, its own position aside).
        (STROLL | WATER_AVOIDING, "canUse") => {
            if mobs::has_controlling_passenger(rt, mob) {
                return Some(b(false));
            }
            if !truth(rt, g, "forceTrigger") {
                if truth(rt, g, "checkNoActionTime") && mobs::no_action_time(rt, mob) >= 100 {
                    return Some(b(false));
                }
                let interval = reduced(int(rt, g, "interval")).max(1);
                if mobs::random_int(rt, mob, interval) != 0 {
                    return Some(b(false));
                }
            }
            let p = api::virtual_call(rt, GET_POSITION, &[g]);
            rt.report_thrown("getPosition");
            if p.r() == 0 {
                return Some(b(false));
            }
            for f in ["x", "y", "z"] {
                let v = rt.get_field(p, f);
                rt.put_field(g, ["wantedX", "wantedY", "wantedZ"][["x", "y", "z"].iter().position(|x| *x == f).unwrap_or(0)], v);
            }
            put_bool(rt, g, "forceTrigger", false);
            b(true)
        }
        (STROLL, "getPosition") => crate::pathing::default_random_pos(rt, mob, 10, 7).unwrap_or(V::NULL),
        (WATER_AVOIDING, "getPosition") => {
            let land = |rt: &mut Rt, r: i32| crate::pathing::land_random_pos(rt, mob, r, 7);
            if mobs::in_water(rt, mob) {
                match land(rt, 15) {
                    Some(p) => p,
                    None => crate::pathing::default_random_pos(rt, mob, 10, 7).unwrap_or(V::NULL),
                }
            } else if mobs::random_float(rt, mob) >= flt(rt, g, "probability") {
                land(rt, 10).unwrap_or(V::NULL)
            } else {
                crate::pathing::default_random_pos(rt, mob, 10, 7).unwrap_or(V::NULL)
            }
        }
        (STROLL | WATER_AVOIDING, "canContinueToUse") => {
            let nav = mobs::navigation(rt, mob);
            b(!crate::pathing::is_done(rt, nav) && !mobs::has_controlling_passenger(rt, mob))
        }
        (STROLL | WATER_AVOIDING, "start") => {
            let nav = mobs::navigation(rt, mob);
            let at = [dbl(rt, g, "wantedX"), dbl(rt, g, "wantedY"), dbl(rt, g, "wantedZ")];
            let speed = dbl(rt, g, "speedModifier");
            crate::pathing::move_to(rt, nav, at, speed);
            V::Void
        }
        (STROLL | WATER_AVOIDING, "stop") => {
            let nav = mobs::navigation(rt, mob);
            crate::pathing::stop(rt, nav);
            V::Void
        }
        (STROLL | WATER_AVOIDING, "trigger") => {
            put_bool(rt, g, "forceTrigger", true);
            V::Void
        }
        (STROLL | WATER_AVOIDING, "setInterval") => {
            rt.put_field(g, "interval", a(1));
            V::Void
        }

        // `PanicGoal`: hurt by what panics it within the last two seconds, it runs to a random
        // place near (on fire, to water near if there is some).
        (PANIC, "canUse") => {
            if !should_panic(rt, g, mob) {
                return Some(b(false));
            }
            if mobs::on_fire(rt, mob)
                && let Some(w) = crate::pathing::water_near(rt, mob, 5)
            {
                rt.put_field(g, "posX", V::D(f64::from(w[0])));
                rt.put_field(g, "posY", V::D(f64::from(w[1])));
                rt.put_field(g, "posZ", V::D(f64::from(w[2])));
                return Some(b(true));
            }
            b(call_bool(rt, SHOULD_PANIC, &[g]))
        }
        (PANIC, "findRandomPosition") => match crate::pathing::default_random_pos(rt, mob, 5, 4) {
            Some(p) => {
                for (f, c) in [("posX", "x"), ("posY", "y"), ("posZ", "z")] {
                    let v = rt.get_field(p, c);
                    rt.put_field(g, f, v);
                }
                b(true)
            }
            None => b(false),
        },
        (PANIC, "isRunning") => b(truth(rt, g, "isRunning")),
        (PANIC, "start") => {
            let nav = mobs::navigation(rt, mob);
            let at = [dbl(rt, g, "posX"), dbl(rt, g, "posY"), dbl(rt, g, "posZ")];
            let speed = dbl(rt, g, "speedModifier");
            crate::pathing::move_to(rt, nav, at, speed);
            put_bool(rt, g, "isRunning", true);
            V::Void
        }
        (PANIC, "stop") => {
            put_bool(rt, g, "isRunning", false);
            V::Void
        }
        (PANIC, "canContinueToUse") => {
            let nav = mobs::navigation(rt, mob);
            b(!crate::pathing::is_done(rt, nav))
        }

        // `MeleeAttackGoal`.
        (MELEE, "canUse") => {
            let now = mobs::game_time(rt);
            if now - long(rt, g, "lastCanUseCheck") < 20 {
                return Some(b(false));
            }
            rt.put_field(g, "lastCanUseCheck", V::J(now));
            let target = mobs::target(rt, mob);
            if target.r() == 0 || !mobs::is_alive(rt, target) {
                return Some(b(false));
            }
            let nav = mobs::navigation(rt, mob);
            let path = crate::pathing::create_path_to_entity(rt, nav, target, 0);
            rt.put_field(g, "path", path);
            b(path.r() != 0 || mobs::within_melee_range(rt, mob, target))
        }
        (MELEE, "canContinueToUse") => {
            let target = mobs::target(rt, mob);
            if target.r() == 0 || !mobs::is_alive(rt, target) {
                return Some(b(false));
            }
            if !truth(rt, g, "followingTargetEvenIfNotSeen") {
                let nav = mobs::navigation(rt, mob);
                return Some(b(!crate::pathing::is_done(rt, nav)));
            }
            let at = mobs::block_position(rt, target);
            if !mobs::within_restriction(rt, mob, at) {
                return Some(b(false));
            }
            b(!(is_player(rt, target) && (mobs::is_spectator(rt, target) || mobs::is_creative(rt, target))))
        }
        (MELEE, "start") => {
            let nav = mobs::navigation(rt, mob);
            let path = obj(rt, g, "path");
            let speed = dbl(rt, g, "speedModifier");
            crate::pathing::move_along(rt, nav, path, speed);
            mobs::set_aggressive(rt, mob, true);
            rt.put_field(g, "ticksUntilNextPathRecalculation", V::I(0));
            rt.put_field(g, "ticksUntilNextAttack", V::I(0));
            V::Void
        }
        (MELEE, "stop") => {
            let target = mobs::target(rt, mob);
            if target.r() == 0 || is_player(rt, target) && (mobs::is_spectator(rt, target) || mobs::is_creative(rt, target)) {
                mobs::set_target(rt, mob, V::NULL);
            }
            mobs::set_aggressive(rt, mob, false);
            let nav = mobs::navigation(rt, mob);
            crate::pathing::stop(rt, nav);
            V::Void
        }
        (MELEE, "requiresUpdateEveryTick") => b(true),
        (MELEE, "tick") => {
            melee_tick(rt, g, mob);
            V::Void
        }
        (MELEE, "checkAndPerformAttack") => {
            if call_bool(rt, CAN_PERFORM_ATTACK, &[g, a(1)]) {
                call_void(rt, RESET_COOLDOWN, &[g]);
                mobs::swing_main_hand(rt, mob);
                mobs::do_hurt_target(rt, mob, a(1));
            }
            V::Void
        }
        (MELEE, "canPerformAttack") => {
            let ready = int(rt, g, "ticksUntilNextAttack") <= 0;
            b(ready && mobs::within_melee_range(rt, mob, a(1)) && mobs::sees(rt, mob, a(1)))
        }
        (MELEE, "resetAttackCooldown") => {
            let t = adjusted(rt, g, 20);
            rt.put_field(g, "ticksUntilNextAttack", V::I(t));
            V::Void
        }
        (MELEE, "isTimeToAttack") => b(int(rt, g, "ticksUntilNextAttack") <= 0),
        (MELEE, "getTicksUntilNextAttack") => V::I(int(rt, g, "ticksUntilNextAttack")),
        (MELEE, "getAttackInterval") => V::I(adjusted(rt, g, 20)),

        // `LeapAtTargetGoal`: between two and four blocks from its target, on the ground, it leaps.
        (LEAP, "canUse") => {
            if mobs::has_controlling_passenger(rt, mob) {
                return Some(b(false));
            }
            let target = mobs::target(rt, mob);
            rt.put_field(g, "target", target);
            if target.r() == 0 {
                return Some(b(false));
            }
            let d = mobs::distance_sqr(rt, mob, target);
            if !(4.0..=16.0).contains(&d) || !mobs::on_ground(rt, mob) {
                return Some(b(false));
            }
            b(mobs::random_int(rt, mob, reduced(5)) == 0)
        }
        (LEAP, "canContinueToUse") => b(!mobs::on_ground(rt, mob)),
        (LEAP, "start") => {
            let target = obj(rt, g, "target");
            let m = mobs::motion(rt, mob);
            let (p, t) = (mobs::position(rt, mob), mobs::position(rt, target));
            let mut v = [t[0] - p[0], 0.0, t[2] - p[2]];
            let l2 = v[0] * v[0] + v[2] * v[2];
            if l2 > 1.0e-7 {
                let l = l2.sqrt();
                v = [v[0] / l * 0.4 + m[0] * 0.2, 0.0, v[2] / l * 0.4 + m[2] * 0.2];
            }
            mobs::set_motion(rt, mob, [v[0], f64::from(flt(rt, g, "yd")), v[2]]);
            V::Void
        }

        // `TemptGoal`: a player within ten blocks holding what tempts it, it follows them.
        (TEMPT, "canUse") => {
            let calm = int(rt, g, "calmDown");
            if calm > 0 {
                rt.put_field(g, "calmDown", V::I(calm - 1));
                return Some(b(false));
            }
            let c = obj(rt, g, "targetingConditions");
            let eye = mobs::eye_pos(rt, mob);
            let items = obj(rt, g, "items");
            let mut found = V::NULL;
            let mut best = f64::MAX;
            for p in players(rt) {
                if !tempted_by(rt, items, p) || !conditions_test(rt, c, mob, p) {
                    continue;
                }
                let at = mobs::position(rt, p);
                let d = (at[0] - eye[0]).powi(2) + (at[1] - eye[1]).powi(2) + (at[2] - eye[2]).powi(2);
                if d < best {
                    best = d;
                    found = p;
                }
            }
            rt.put_field(g, "player", found);
            b(found.r() != 0)
        }
        (TEMPT, "canContinueToUse") => b(call_bool(rt, CAN_USE, &[g])),
        (TEMPT, "start") => {
            put_bool(rt, g, "isRunning", true);
            V::Void
        }
        (TEMPT, "stop") => {
            rt.put_field(g, "player", V::NULL);
            let nav = mobs::navigation(rt, mob);
            crate::pathing::stop(rt, nav);
            rt.put_field(g, "calmDown", V::I(reduced(100)));
            put_bool(rt, g, "isRunning", false);
            V::Void
        }
        (TEMPT, "tick") => {
            let p = obj(rt, g, "player");
            if p.r() != 0 {
                let look = mobs::look_control(rt, mob);
                let speed = (mobs::head_rot_speed(rt, mob) + 20) as f32;
                let max = mobs::max_head_x_rot(rt, mob) as f32;
                mobs::look_at_entity(rt, look, p, speed, max);
                let nav = mobs::navigation(rt, mob);
                if mobs::distance_sqr(rt, mob, p) < 6.25 {
                    crate::pathing::stop(rt, nav);
                } else {
                    let speed = dbl(rt, g, "speedModifier");
                    crate::pathing::move_to_entity(rt, nav, p, speed);
                }
            }
            V::Void
        }
        (TEMPT, "isRunning") => b(truth(rt, g, "isRunning")),

        // `FollowParentGoal`: a baby keeps within three to sixteen blocks of the nearest grown one
        // of its class within eight.
        (FOLLOW_PARENT, "canUse") => {
            if mobs::age(rt, mob) >= 0 {
                return Some(b(false));
            }
            let bb = mobs::bounds(rt, mob);
            let area = [bb[0] - 8.0, bb[1] - 4.0, bb[2] - 8.0, bb[3] + 8.0, bb[4] + 4.0, bb[5] + 8.0];
            let class = rt.class_of(mob);
            let mut best = f64::MAX;
            let mut parent = V::NULL;
            for o in crate::mod_entities::in_box(rt, area) {
                if o == mob || rt.class_of(o) != class || mobs::age(rt, o) < 0 {
                    continue;
                }
                let d = mobs::distance_sqr(rt, mob, o);
                if d <= best {
                    best = d;
                    parent = o;
                }
            }
            if parent.r() == 0 || best < 9.0 {
                return Some(b(false));
            }
            rt.put_field(g, "parent", parent);
            b(true)
        }
        (FOLLOW_PARENT, "canContinueToUse") => {
            let parent = obj(rt, g, "parent");
            if mobs::age(rt, mob) >= 0 || parent.r() == 0 || !mobs::is_alive(rt, parent) {
                return Some(b(false));
            }
            let d = mobs::distance_sqr(rt, mob, parent);
            b((9.0..=256.0).contains(&d))
        }
        (FOLLOW_PARENT, "start") => {
            rt.put_field(g, "timeToRecalcPath", V::I(0));
            V::Void
        }
        (FOLLOW_PARENT, "stop") => {
            rt.put_field(g, "parent", V::NULL);
            V::Void
        }
        (FOLLOW_PARENT, "tick") => {
            let t = int(rt, g, "timeToRecalcPath") - 1;
            if t <= 0 {
                let t = adjusted(rt, g, 10);
                rt.put_field(g, "timeToRecalcPath", V::I(t));
                let parent = obj(rt, g, "parent");
                let nav = mobs::navigation(rt, mob);
                let speed = dbl(rt, g, "speedModifier");
                crate::pathing::move_to_entity(rt, nav, parent, speed);
            } else {
                rt.put_field(g, "timeToRecalcPath", V::I(t));
            }
            V::Void
        }

        // `AvoidEntityGoal`: the nearest of the class within reach seen, it walks away from it to a
        // place farther from it, and runs once it is within seven blocks.
        (AVOID, "canUse") => {
            let d = f64::from(flt(rt, g, "maxDist"));
            let bb = mobs::bounds(rt, mob);
            let area = [bb[0] - d, bb[1] - 3.0, bb[2] - d, bb[3] + d, bb[4] + 3.0, bb[5] + d];
            let class = rt.get_field(g, "avoidClass");
            let list = entities_of_class(rt, class, area);
            let c = obj(rt, g, "avoidEntityTargeting");
            let eye = mobs::eye_pos(rt, mob);
            let avoid = nearest_of(rt, &list, c, mob, eye);
            rt.put_field(g, "toAvoid", avoid);
            if avoid.r() == 0 {
                return Some(b(false));
            }
            let from = mobs::position(rt, avoid);
            let Some(away) = crate::pathing::random_pos_away(rt, mob, 16, 7, from) else {
                return Some(b(false));
            };
            let to = [rt.get_field(away, "x").d(), rt.get_field(away, "y").d(), rt.get_field(away, "z").d()];
            let dist = |p: [f64; 3]| (p[0] - from[0]).powi(2) + (p[1] - from[1]).powi(2) + (p[2] - from[2]).powi(2);
            let here = mobs::position(rt, mob);
            if dist(to) < dist(here) {
                return Some(b(false));
            }
            let nav = mobs::navigation(rt, mob);
            let path = crate::pathing::create_path_to(rt, nav, to, 0);
            rt.put_field(g, "path", path);
            b(path.r() != 0)
        }
        (AVOID, "canContinueToUse") => {
            let nav = mobs::navigation(rt, mob);
            b(!crate::pathing::is_done(rt, nav))
        }
        (AVOID, "start") => {
            let nav = mobs::navigation(rt, mob);
            let path = obj(rt, g, "path");
            let speed = dbl(rt, g, "walkSpeedModifier");
            crate::pathing::move_along(rt, nav, path, speed);
            V::Void
        }
        (AVOID, "stop") => {
            rt.put_field(g, "toAvoid", V::NULL);
            V::Void
        }
        (AVOID, "tick") => {
            let avoid = obj(rt, g, "toAvoid");
            let nav = mobs::navigation(rt, mob);
            let near = avoid.r() != 0 && mobs::distance_sqr(rt, mob, avoid) < 49.0;
            let speed = dbl(rt, g, if near { "sprintSpeedModifier" } else { "walkSpeedModifier" });
            crate::pathing::set_speed(rt, nav, speed);
            V::Void
        }

        // `TargetGoal` and the goals under it.
        (TARGET_GOAL | HURT_BY | NEAREST, "canContinueToUse") if k != HURT_BY || !overridden(rt, g, "canContinueToUse.()Z") => {
            b(target_continue(rt, g, mob))
        }
        (TARGET_GOAL | HURT_BY | NEAREST, "getFollowDistance") => V::D(mobs::attribute(rt, mob, "minecraft:generic.follow_range")),
        (TARGET_GOAL | HURT_BY | NEAREST, "start") if k == TARGET_GOAL || name != "start" => {
            target_start(rt, g);
            V::Void
        }
        (TARGET_GOAL | HURT_BY | NEAREST, "stop") => {
            mobs::set_target(rt, mob, V::NULL);
            rt.put_field(g, "targetMob", V::NULL);
            V::Void
        }
        (TARGET_GOAL | HURT_BY | NEAREST, "canAttack") => b(target_can_attack(rt, g, mob, a(1), a(2))),
        (TARGET_GOAL | HURT_BY | NEAREST, "setUnseenMemoryTicks") => {
            rt.put_field(g, "unseenMemoryTicks", a(1));
            g
        }
        (HURT_BY, "canUse") => {
            let stamp = mobs::last_hurt_by_mob_timestamp(rt, mob);
            let by = mobs::last_hurt_by_mob(rt, mob);
            if stamp == int(rt, g, "timestamp") || by.r() == 0 {
                return Some(b(false));
            }
            if is_player(rt, by) && game_rule(rt, "universal_anger") {
                return Some(b(false));
            }
            let ignore = jutil::elements(rt, obj(rt, g, "toIgnoreDamage"));
            for c in ignore {
                if class_matches(rt, c, by) {
                    return Some(b(false));
                }
            }
            let c = hurt_by_conditions(rt);
            b(target_can_attack(rt, g, mob, by, c))
        }
        (HURT_BY, "setAlertOthers") => {
            put_bool(rt, g, "alertSameType", true);
            rt.put_field(g, "toIgnoreAlert", a(1));
            g
        }
        (HURT_BY, "start") => {
            let by = mobs::last_hurt_by_mob(rt, mob);
            mobs::set_target(rt, mob, by);
            let t = mobs::target(rt, mob);
            rt.put_field(g, "targetMob", t);
            let stamp = mobs::last_hurt_by_mob_timestamp(rt, mob);
            rt.put_field(g, "timestamp", V::I(stamp));
            rt.put_field(g, "unseenMemoryTicks", V::I(300));
            if truth(rt, g, "alertSameType") {
                alert_others(rt, g, mob);
            }
            target_start(rt, g);
            V::Void
        }
        (HURT_BY, "alertOther") => {
            mobs::set_target(rt, a(1), a(2));
            V::Void
        }
        (NEAREST, "canUse") => {
            let interval = int(rt, g, "randomInterval");
            if interval > 0 && mobs::random_int(rt, mob, interval) != 0 {
                return Some(b(false));
            }
            call_void(rt, FIND_TARGET, &[g]);
            b(obj(rt, g, "target").r() != 0)
        }
        (NEAREST, "getTargetSearchArea") => {
            let d = a(1).d();
            let bb = mobs::bounds(rt, mob);
            crate::shapes::aabb(rt, [bb[0] - d, bb[1] - 4.0, bb[2] - d, bb[3] + d, bb[4] + 4.0, bb[5] + d])
        }
        (NEAREST, "findTarget") => {
            let c = obj(rt, g, "targetConditions");
            let eye = mobs::eye_pos(rt, mob);
            let ty = rt.get_field(g, "targetType");
            let found = if class_named(rt, ty, PLAYER) || class_named(rt, ty, SERVER_PLAYER) {
                nearest_player(rt, c, mob, eye)
            } else {
                let d = follow_distance(rt, g);
                let area = api::virtual_call(rt, SEARCH_AREA, &[g, V::D(d)]);
                let b = crate::shapes::aabb_of(rt, area);
                let list = entities_of_class(rt, ty, b);
                nearest_of(rt, &list, c, mob, eye)
            };
            rt.put_field(g, "target", found);
            V::Void
        }
        (NEAREST, "start") => {
            let t = obj(rt, g, "target");
            mobs::set_target(rt, mob, t);
            target_start(rt, g);
            V::Void
        }
        (NEAREST, "setTarget") => {
            rt.put_field(g, "target", a(1));
            V::Void
        }
        _ => {
            let _ = desc;
            return None;
        }
    })
}

/// `MeleeAttackGoal.tick()`: it looks at its target, walks after it (a new path now and then, more
/// rarely when far), and attacks when it may.
fn melee_tick(rt: &mut Rt, g: V, mob: V) {
    let target = mobs::target(rt, mob);
    if target.r() == 0 {
        return;
    }
    let look = mobs::look_control(rt, mob);
    mobs::look_at_entity(rt, look, target, 30.0, 30.0);
    let mut recalc = (int(rt, g, "ticksUntilNextPathRecalculation") - 1).max(0);
    let follow = truth(rt, g, "followingTargetEvenIfNotSeen");
    let at = mobs::position(rt, target);
    let pathed = [dbl(rt, g, "pathedTargetX"), dbl(rt, g, "pathedTargetY"), dbl(rt, g, "pathedTargetZ")];
    if (follow || mobs::sees(rt, mob, target)) && recalc <= 0 {
        let moved = pathed == [0.0; 3] || (at[0] - pathed[0]).powi(2) + (at[1] - pathed[1]).powi(2) + (at[2] - pathed[2]).powi(2) >= 1.0;
        if moved || mobs::random_float(rt, mob) < 0.05 {
            rt.put_field(g, "pathedTargetX", V::D(at[0]));
            rt.put_field(g, "pathedTargetY", V::D(at[1]));
            rt.put_field(g, "pathedTargetZ", V::D(at[2]));
            recalc = 4 + mobs::random_int(rt, mob, 7);
            let d = mobs::distance_sqr(rt, mob, target);
            if d > 1024.0 {
                recalc += 10;
            } else if d > 256.0 {
                recalc += 5;
            }
            let nav = mobs::navigation(rt, mob);
            let speed = dbl(rt, g, "speedModifier");
            if !crate::pathing::move_to_entity(rt, nav, target, speed) {
                recalc += 15;
            }
            recalc = adjusted(rt, g, recalc);
        }
    }
    rt.put_field(g, "ticksUntilNextPathRecalculation", V::I(recalc));
    let attack = (int(rt, g, "ticksUntilNextAttack") - 1).max(0);
    rt.put_field(g, "ticksUntilNextAttack", V::I(attack));
    call_void(rt, CHECK_AND_ATTACK, &[g, target]);
}

/// Whether `PanicGoal` panics its mob: its own predicate, else the damage that last hurt it
/// (within two seconds, `getLastDamageSource`) is in its tag (vanilla's `minecraft:panic_causes`).
fn should_panic(rt: &mut Rt, g: V, mob: V) -> bool {
    let predicate = obj(rt, g, "shouldPanic");
    if predicate.r() != 0 {
        return rt.call_functional(PREDICATE_TEST, predicate, &[mob]).i() != 0;
    }
    let source = mobs::last_damage_source(rt, mob);
    if source.r() == 0 {
        return false;
    }
    let tag = match rt.str_of(rt.get_field(g, "$panicTag")) {
        Some(t) => t.to_owned(),
        None => {
            let of = obj(rt, g, "$panicTagOf");
            if of.r() != 0 {
                let t = rt.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", of, &[mob]);
                rt.str_of(rt.get_field(t, "$location")).unwrap_or("minecraft:panic_causes").to_owned()
            } else {
                "minecraft:panic_causes".to_owned()
            }
        }
    };
    let id = rt.str_of(rt.get_field(source, "$type")).unwrap_or("").to_owned();
    !id.is_empty() && crate::damage::in_tag(rt, &id, &tag)
}

/// `TargetGoal.canContinueToUse()`.
fn target_continue(rt: &mut Rt, g: V, mob: V) -> bool {
    let mut t = mobs::target(rt, mob);
    if t.r() == 0 {
        t = obj(rt, g, "targetMob");
    }
    if t.r() == 0 || !mobs::can_attack(rt, mob, t) {
        return false;
    }
    let d = follow_distance(rt, g);
    if mobs::distance_sqr(rt, mob, t) > d * d {
        return false;
    }
    if truth(rt, g, "mustSee") {
        if mobs::sees(rt, mob, t) {
            rt.put_field(g, "unseenTicks", V::I(0));
        } else {
            let n = int(rt, g, "unseenTicks") + 1;
            rt.put_field(g, "unseenTicks", V::I(n));
            if n > reduced(int(rt, g, "unseenMemoryTicks")) {
                return false;
            }
        }
    }
    mobs::set_target(rt, mob, t);
    true
}

fn target_start(rt: &mut Rt, g: V) {
    rt.put_field(g, "reachCache", V::I(0));
    rt.put_field(g, "reachCacheTime", V::I(0));
    rt.put_field(g, "unseenTicks", V::I(0));
}

/// `TargetGoal.canAttack(target, conditions)`: the conditions take it, it is within the mob's
/// restriction, and, for a goal that must reach it, a path ends within one and a half blocks of it.
fn target_can_attack(rt: &mut Rt, g: V, mob: V, target: V, c: V) -> bool {
    if target.r() == 0 || !conditions_test(rt, c, mob, target) {
        return false;
    }
    let at = mobs::block_position(rt, target);
    if !mobs::within_restriction(rt, mob, at) {
        return false;
    }
    if truth(rt, g, "mustReach") {
        let time = int(rt, g, "reachCacheTime") - 1;
        rt.put_field(g, "reachCacheTime", V::I(time));
        if time <= 0 {
            rt.put_field(g, "reachCache", V::I(0));
        }
        if int(rt, g, "reachCache") == 0 {
            let reach = can_reach(rt, g, mob, target);
            rt.put_field(g, "reachCache", V::I(if reach { 1 } else { 2 }));
        }
        if int(rt, g, "reachCache") == 2 {
            return false;
        }
    }
    true
}

fn can_reach(rt: &mut Rt, g: V, mob: V, target: V) -> bool {
    let t = reduced(10 + mobs::random_int(rt, mob, 5));
    rt.put_field(g, "reachCacheTime", V::I(t));
    let nav = mobs::navigation(rt, mob);
    let path = crate::pathing::create_path_to_entity(rt, nav, target, 0);
    let Some(end) = crate::pathing::end_node(rt, path) else { return false };
    let at = mobs::block_position(rt, target);
    let (i, j) = (end[0] - at[0], end[2] - at[2]);
    f64::from(i * i + j * j) <= 2.25
}

/// `HurtByTargetGoal.alertOthers()`: the mobs of its class within its follow range (ten blocks up
/// and down) without a target, not allied to its attacker and not of a class it leaves out, take
/// its attacker as theirs (`alertOther`, virtual).
fn alert_others(rt: &mut Rt, g: V, mob: V) {
    let d = follow_distance(rt, g);
    let p = mobs::position(rt, mob);
    let area = [p[0].floor() - d, p[1].floor() - 10.0, p[2].floor() - d, p[0].floor() + 1.0 + d, p[1].floor() + 11.0, p[2].floor() + 1.0 + d];
    let class = rt.class_of(mob);
    let attacker = mobs::last_hurt_by_mob(rt, mob);
    let ignore = jutil::elements(rt, obj(rt, g, "toIgnoreAlert"));
    for other in crate::mod_entities::in_box(rt, area) {
        if other == mob || !rt.instance_of(other, class) || mobs::target(rt, other).r() != 0 {
            continue;
        }
        if mobs::is_allied(rt, other, attacker) {
            continue;
        }
        if ignore.iter().any(|&c| rt.str_of(c).map(|n| n.replace('.', "/")).as_deref() == Some(rt.class_of(other))) {
            continue;
        }
        call_void(rt, ALERT_OTHER, &[g, other, attacker]);
    }
}

// ---- targeting ----

fn new_conditions(rt: &mut Rt, combat: bool) -> V {
    let c = rt.new_object(CONDITIONS);
    put_bool(rt, c, "isCombat", combat);
    rt.put_field(c, "range", V::D(-1.0));
    put_bool(rt, c, "checkLineOfSight", true);
    put_bool(rt, c, "testInvisible", true);
    rt.put_field(c, "selector", V::NULL);
    c
}

/// `HurtByTargetGoal.HURT_BY_TARGETING`: combat, seen or not, invisible or not.
fn hurt_by_conditions(rt: &mut Rt) -> V {
    rt.intern("compat:hurt-by-targeting".into(), |rt| {
        let c = new_conditions(rt, true);
        put_bool(rt, c, "checkLineOfSight", false);
        put_bool(rt, c, "testInvisible", false);
        c
    })
}

fn conditions_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    Some(match name {
        "forCombat" => new_conditions(rt, true),
        "forNonCombat" => new_conditions(rt, false),
        "<init>" => {
            put_bool(rt, c, "isCombat", a(1).i() != 0);
            rt.put_field(c, "range", V::D(-1.0));
            put_bool(rt, c, "checkLineOfSight", true);
            put_bool(rt, c, "testInvisible", true);
            V::Void
        }
        "copy" => {
            let n = new_conditions(rt, truth(rt, c, "isCombat"));
            for f in ["range", "checkLineOfSight", "testInvisible", "selector"] {
                let v = rt.get_field(c, f);
                rt.put_field(n, f, v);
            }
            n
        }
        "range" => {
            rt.put_field(c, "range", V::D(a(1).d()));
            c
        }
        "ignoreLineOfSight" => {
            put_bool(rt, c, "checkLineOfSight", false);
            c
        }
        "ignoreInvisibilityTesting" => {
            put_bool(rt, c, "testInvisible", false);
            c
        }
        "selector" => {
            rt.put_field(c, "selector", a(1));
            c
        }
        "test" => V::I(i32::from(conditions_test(rt, c, a(1), a(2)))),
        _ => {
            let _ = desc;
            return None;
        }
    })
}

/// `TargetingConditions.test(attacker, target)`.
pub(crate) fn conditions_test(rt: &mut Rt, c: V, attacker: V, target: V) -> bool {
    if target.r() == 0 || attacker == target || !mobs::can_be_seen_by_anyone(rt, target) {
        return false;
    }
    let selector = obj(rt, c, "selector");
    if selector.r() != 0 && rt.call_functional(PREDICATE_TEST, selector, &[target]).i() == 0 {
        return false;
    }
    let combat = truth(rt, c, "isCombat");
    if attacker.r() == 0 {
        return !combat || mobs::can_be_seen_as_enemy(rt, target);
    }
    if combat && (!mobs::can_attack(rt, attacker, target) || mobs::is_allied(rt, attacker, target)) {
        return false;
    }
    let range = dbl(rt, c, "range");
    if range > 0.0 {
        let visibility = if truth(rt, c, "testInvisible") { mobs::visibility_percent(rt, target, attacker) } else { 1.0 };
        let r = (range * visibility).max(2.0);
        let (p, t) = (mobs::position(rt, attacker), mobs::position(rt, target));
        let d = (p[0] - t[0]).powi(2) + (p[1] - t[1]).powi(2) + (p[2] - t[2]).powi(2);
        if d > r * r {
            return false;
        }
    }
    if truth(rt, c, "checkLineOfSight") && mobs::is_mob(rt, attacker) && !mobs::sees(rt, attacker, target) {
        return false;
    }
    true
}

/// The level's players, as the runtime's objects for them.
fn players(rt: &mut Rt) -> Vec<V> {
    let list = rt.host().map(|h| h.player_entities()).unwrap_or_default();
    list.into_iter().map(|(id, uuid, at)| crate::vanilla_entities::player(rt, id, uuid, at)).collect()
}

/// `Level.getNearestPlayer(conditions, entity, x, y, z)`: of those the conditions take, the
/// nearest to the point.
fn nearest_player(rt: &mut Rt, c: V, from: V, at: [f64; 3]) -> V {
    let all = players(rt);
    nearest_of(rt, &all, c, from, at)
}

/// `Level.getNearestEntity(list, conditions, entity, x, y, z)`.
fn nearest_of(rt: &mut Rt, list: &[V], c: V, from: V, at: [f64; 3]) -> V {
    let mut best = f64::MAX;
    let mut found = V::NULL;
    for &e in list {
        if c.r() != 0 && !conditions_test(rt, c, from, e) {
            continue;
        }
        let p = mobs::position(rt, e);
        let d = (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2);
        if best == f64::MAX || d < best {
            best = d;
            found = e;
        }
    }
    found
}

/// `getEntitiesOfClass(class, box)` on the level.
fn entities_of_class(rt: &mut Rt, class: V, b: [f64; 6]) -> Vec<V> {
    let level = crate::glue::level(rt);
    let area = crate::shapes::aabb(rt, b);
    let list = api::call(rt, "net/minecraft/world/level/Level.getEntitiesOfClass.(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;)Ljava/util/List;", &[level, class, area]);
    jutil::elements(rt, list)
}

/// Whether `class` (a `Class` object) names `name`.
fn class_named(rt: &Rt, class: V, name: &str) -> bool {
    rt.str_of(class).is_some_and(|n| n.replace('.', "/") == name)
}

/// `class.isAssignableFrom(e.getClass())`.
fn class_matches(rt: &Rt, class: V, e: V) -> bool {
    rt.str_of(class).is_some_and(|n| rt.instance_of(e, &n.replace('.', "/")))
}

fn is_player(rt: &Rt, e: V) -> bool {
    e.r() != 0 && rt.instance_of(e, PLAYER)
}

fn game_rule(rt: &Rt, name: &str) -> bool {
    rt.host().and_then(|h| h.game_rule(name)).is_some_and(|v| v != 0)
}

/// Whether `TemptGoal`'s items take what player `p` holds in either hand.
fn tempted_by(rt: &mut Rt, items: V, p: V) -> bool {
    for hand in ["getMainHandItem", "getOffhandItem"] {
        let k = rt.static_key(&format!("{LIVING}.{hand}.()Lnet/minecraft/world/item/ItemStack;"));
        let stack = api::call(rt, k, &[p]);
        if stack.r() == 0 || crate::api_common::stack_empty(rt, stack) {
            continue;
        }
        let takes = if rt.class_of(items) == "net/minecraft/world/item/crafting/Ingredient" {
            api::call(rt, "net/minecraft/world/item/crafting/Ingredient.test.(Lnet/minecraft/world/item/ItemStack;)Z", &[items, stack]).i() != 0
        } else {
            items.r() != 0 && rt.call_functional(PREDICATE_TEST, items, &[stack]).i() != 0
        };
        if takes {
            return true;
        }
    }
    false
}

/// The level's searches by targeting conditions: `getNearestPlayer(conditions, entity[, x, y,
/// z])`, `getNearestEntity(list, conditions, entity, x, y, z)` and the nearby ones in a box.
fn level_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match name {
        "getNearestPlayer" if desc.starts_with("(Lnet/minecraft/world/entity/ai/targeting/TargetingConditions;Lnet/minecraft/world/entity/LivingEntity;DDD)") => {
            Some(nearest_player(rt, a(1), a(2), [a(3).d(), a(4).d(), a(5).d()]))
        }
        "getNearestPlayer" if desc.starts_with("(Lnet/minecraft/world/entity/ai/targeting/TargetingConditions;Lnet/minecraft/world/entity/LivingEntity;)") => {
            let at = if a(2).r() != 0 { mobs::position(rt, a(2)) } else { [0.0; 3] };
            Some(nearest_player(rt, a(1), a(2), at))
        }
        "getNearestPlayer" if desc.starts_with("(Lnet/minecraft/world/entity/ai/targeting/TargetingConditions;DDD)") => {
            Some(nearest_player(rt, a(1), V::NULL, [a(2).d(), a(3).d(), a(4).d()]))
        }
        "getNearestEntity" if desc.starts_with("(Ljava/util/List;") => {
            let list = jutil::elements(rt, a(1));
            Some(nearest_of(rt, &list, a(2), a(3), [a(4).d(), a(5).d(), a(6).d()]))
        }
        "getNearestEntity" if desc.starts_with("(Ljava/lang/Class;") => {
            let b = crate::shapes::aabb_of(rt, a(7));
            let list = entities_of_class(rt, a(1), b);
            Some(nearest_of(rt, &list, a(2), a(3), [a(4).d(), a(5).d(), a(6).d()]))
        }
        "getNearbyPlayers" => {
            let b = crate::shapes::aabb_of(rt, a(3));
            let mut out = Vec::new();
            for p in players(rt) {
                let at = mobs::position(rt, p);
                if (0..3).all(|i| at[i] >= b[i] && at[i] < b[i + 3]) && conditions_test(rt, a(1), a(2), p) {
                    out.push(p);
                }
            }
            Some(jutil::new_list(rt, out))
        }
        "getNearbyEntities" => {
            let b = crate::shapes::aabb_of(rt, a(4));
            let list = entities_of_class(rt, a(1), b);
            let mut out = Vec::new();
            for e in list {
                if conditions_test(rt, a(2), a(3), e) {
                    out.push(e);
                }
            }
            Some(jutil::new_list(rt, out))
        }
        _ => None,
    }
}

// ---- sensing ----

/// A mob's `Sensing` (`new Sensing(mob)`): what it has seen and not seen this tick.
pub(crate) fn new_sensing(rt: &mut Rt, mob: V) -> V {
    let s = rt.new_object(SENSING);
    rt.put_field(s, "mob", mob);
    s
}

fn sensing_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(s, "mob", a(1));
            V::Void
        }
        "tick" => {
            sensing_clear(rt, s);
            V::Void
        }
        "hasLineOfSight" => V::I(i32::from(sensing_sees(rt, s, a(1)))),
        _ => return None,
    })
}

pub(crate) fn sensing_clear(rt: &mut Rt, s: V) {
    for f in ["$seen", "$unseen"] {
        let l = rt.get_field(s, f);
        if let Some(crate::Object { data: crate::Data::List(items), .. }) = (l.r() != 0).then(|| rt.heap.get_mut(l.r() as usize)).flatten() {
            items.clear();
        }
    }
}

/// `Sensing.hasLineOfSight(entity)`: once a tick for each entity (`LivingEntity.hasLineOfSight`),
/// kept until the next tick (`seen`, `unseen`: lists the selector's tick clears).
pub(crate) fn sensing_sees(rt: &mut Rt, s: V, e: V) -> bool {
    if e.r() == 0 {
        return false;
    }
    let held = |rt: &Rt, f: &'static str| jutil::elements(rt, rt.get_field(s, f)).contains(&e);
    if held(rt, "$seen") {
        return true;
    }
    if held(rt, "$unseen") {
        return false;
    }
    let mob = obj(rt, s, "mob");
    let seen = mobs::has_line_of_sight(rt, mob, e);
    let f = if seen { "$seen" } else { "$unseen" };
    let mut l = rt.get_field(s, f);
    if l.r() == 0 {
        l = jutil::new_list(rt, Vec::new());
        rt.put_field(s, f, l);
    }
    jutil::list_add(rt, l, e);
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_tick_delay_rounds_half_up() {
        assert_eq!(reduced(0), 0);
        assert_eq!(reduced(1), 1);
        assert_eq!(reduced(5), 3);
        assert_eq!(reduced(20), 10);
        assert_eq!(reduced(120), 60);
    }

    #[test]
    fn unmodelled_goals_climb_to_what_they_extend() {
        assert_eq!(api_super("net/minecraft/world/entity/ai/goal/ZombieAttackGoal"), Some(MELEE));
        assert_eq!(api_super("net/minecraft/world/entity/ai/goal/target/OwnerHurtTargetGoal"), Some(TARGET_GOAL));
        assert_eq!(api_super("net/minecraft/world/entity/ai/goal/target/NonTameRandomTargetGoal"), Some(NEAREST));
        assert_eq!(api_super("net/minecraft/world/entity/ai/goal/BreedGoal"), Some(GOAL));
        assert_eq!(api_super(GOAL), None);
    }
}
