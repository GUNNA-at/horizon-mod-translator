//! Vanilla's entities a mod makes and adds itself: `EntityType.LIGHTNING_BOLT.create(level)` then
//! `level.addFreshEntity(bolt)`, `new PrimedTnt(level, x, y, z, owner)` (Create's mounted
//! dispenser), `EntityType.SKELETON_HORSE.create(level)` loaded from a horse's compound (Create's
//! haunting), `FallingBlockEntity.fall(level, pos, state)`. The object is of the type's 1.21.1
//! class, so `instanceof` answers as in Java, and keeps what the mod sets: where it is, its facing
//! and its motion as fields, the rest in the compound `saveWithoutId` would write (`CustomName`,
//! `NoGravity`, a TNT's `fuse`, an armor stand's `ShowArms`, a firework's `FireworksItem`...).
//! Adding it hands that compound to the game, which makes the entity from it as `/summon <kind>
//! <pos> <nbt>` does ([`Host::summon`]); a lightning bolt strikes ([`Host::strike_lightning`]).
//! From then on the object stands for the game's entity ([`vanilla_entities::adopt`]).
//!
//! The compound is in the game's form where the two differ (a custom name is a text component's
//! NBT, equipment is `equipment`). A compound a mod loads into one (`load`, `deserializeNBT`)
//! passes as given: a game entity's saved compound (`saveWithoutId` of the entity it replaces) is
//! already the game's.
//!
//! Not modelled: an area effect cloud (the game doesn't make one from its compound), a firework
//! attached to the entity it boosts, a TNT's owner (the game keeps none for a summoned one), a
//! falling block's damage set after `fall` added it, and what a mob's `finalizeSpawn` would have
//! set before it is added (it runs as the game adds the mob).
//!
//! [`Host::summon`]: crate::host::Host::summon
//! [`Host::strike_lightning`]: crate::host::Host::strike_lightning
//! [`vanilla_entities::adopt`]: crate::vanilla_entities::adopt

use crate::host::Nbt;
use crate::vanilla_entities::{class_for, is_a};
use crate::{Data, Rt, V, api};
use std::collections::BTreeMap;

const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
const MOB: &str = "net/minecraft/world/entity/Mob";
const AGEABLE: &str = "net/minecraft/world/entity/AgeableMob";
const PROJECTILE: &str = "net/minecraft/world/entity/projectile/Projectile";
const ARMOR_STAND: &str = "net/minecraft/world/entity/decoration/ArmorStand";
const TNT: &str = "net/minecraft/world/entity/item/PrimedTnt";
const FALLING: &str = "net/minecraft/world/entity/item/FallingBlockEntity";
const FIREWORK: &str = "net/minecraft/world/entity/projectile/FireworkRocketEntity";
const EVENT_HOOKS: &str = "net/neoforged/neoforge/event/EventHooks";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";

const TYPE_LEVEL: &str = "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
const AT: &str = "(Lnet/minecraft/world/level/Level;DDD)V";
const TNT_AT: &str = "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/entity/LivingEntity;)V";
const FALLING_AT: &str = "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/level/block/state/BlockState;)V";
const ROCKET_AT: &str = "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/item/ItemStack;)V";
const ROCKET_ANGLED: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/item/ItemStack;DDDZ)V";
const ROCKET_BY: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/Entity;DDDLnet/minecraft/world/item/ItemStack;)V";
const ROCKET_BY_ANGLED: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/Entity;DDDZ)V";
const FALL: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/entity/item/FallingBlockEntity;";
const ADD: &str = "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z";
const CREATE: &str = "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";
const MOVE_TO: (&str, &str) = ("net/minecraft/world/entity/Entity.moveTo.(DDDFF)V", "moveTo.(DDDFF)V");
const GET_UUID: &str = "net/minecraft/world/entity/Entity.getUUID.()Ljava/util/UUID;";

/// `$made`: held by the mod and not added yet, or discarded before it was.
const HELD: i32 = 1;
const DISCARDED: i32 = 2;

fn state(rt: &Rt, e: V) -> i32 {
    match e {
        V::R(n) if n != 0 => rt.get_field(e, "$made").i(),
        _ => 0,
    }
}

/// Whether `e` is a vanilla entity a mod made that it hasn't added (discarded or not).
pub(crate) fn is_made(rt: &Rt, e: V) -> bool {
    matches!(state(rt, e), HELD | DISCARDED)
}

/// A new entity of vanilla's type `kind` (`minecraft:skeleton_horse`) in `level`, as its
/// constructor leaves it: of its 1.21.1 class, at the origin, still (`EntityType.create`).
pub(crate) fn make(rt: &mut Rt, kind: &str, level: V) -> V {
    let e = rt.new_object(class_for(kind, false));
    begin(rt, e, kind, level);
    e
}

fn begin(rt: &mut Rt, e: V, kind: &str, level: V) {
    rt.put_field(e, "$made", V::I(HELD));
    let k = rt.string(kind);
    rt.put_field(e, "$kind", k);
    rt.put_field(e, "$level", level);
    for f in ["$x", "$y", "$z", "$mx", "$my", "$mz"] {
        rt.put_field(e, f, V::D(0.0));
    }
    for f in ["$yRot", "$xRot"] {
        rt.put_field(e, f, V::F(0.0));
    }
    let tag = crate::tags::new_compound(rt);
    rt.put_field(e, "$nbt", tag);
}

/// `EntityType.spawn(level, [consumer,] pos, spawnType, ...)` and the `create` of the same
/// arguments: the type's new entity (`create(level)`), at the middle of block `pos`'s floor facing
/// a random way, finalized as the game adds it when it is a mob, handed to `consumer`, then added
/// with its passengers when `add`. Null when the type makes none. The offset above the block a
/// spawn egg asks for, and the stack's own name and data, aren't applied.
pub(crate) fn spawn(rt: &mut Rt, ty: V, level: V, pos: V, consumer: V, add: bool) -> V {
    let e = api::call(rt, CREATE, &[ty, level]);
    if e.r() == 0 {
        return V::NULL;
    }
    let p = api::pos_of(rt, pos);
    // `Mth.wrapDegrees(level.random.nextFloat() * 360.0F)`.
    let yaw = wrap_degrees(unit(rt) as f32 * 360.0);
    api::virtual_call(rt, MOVE_TO, &[e, V::D(f64::from(p[0]) + 0.5), V::D(f64::from(p[1])), V::D(f64::from(p[2]) + 0.5), V::F(yaw), V::F(0.0)]);
    if is_made(rt, e) && is_a(rt.class_of(e), MOB) {
        rt.put_field(e, "$finalize", V::I(1));
    }
    if consumer.r() != 0 {
        rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", consumer, &[e]);
    }
    if add {
        api::call(rt, "net/minecraft/server/level/ServerLevel.addFreshEntityWithPassengers.(Lnet/minecraft/world/entity/Entity;)V", &[level, e]);
    }
    e
}

/// The keys [`call`] may answer: adding an entity to a level, `FallingBlockEntity.fall`,
/// NeoForge's `finalizeMobSpawn`, and vanilla's entity classes' own (their constructors, and any
/// method on an entity a mod made).
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(name, "addFreshEntity" | "tryAddFreshEntityWithPassengers" | "addFreshEntityWithPassengers")
        || (owner == EVENT_HOOKS && name == "finalizeMobSpawn")
        || crate::vanilla_entities::claims(owner, name, desc)
}

/// What vanilla answers for an entity a mod made, its constructors, and adding one to a level.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match name {
        "addFreshEntity" | "tryAddFreshEntityWithPassengers" | "addFreshEntityWithPassengers" => {
            let e = a(1);
            let added = if is_made(rt, e) {
                add(rt, a(0), e)
            } else if crate::vanilla_entities::is_proxy(rt, e) {
                // Already the game's: `addEntity` refuses an entity the level holds.
                false
            } else if name != "addFreshEntity" {
                // A mod's entity or an item entity: as `addFreshEntity` takes it (its passengers aren't
                // added with it).
                api::call(rt, ADD, &[a(0), e]).i() != 0
            } else {
                return None;
            };
            return Some(if desc.ends_with(")Z") { V::I(i32::from(added)) } else { V::Void });
        }
        "fall" if owner == FALLING && desc == FALL => return Some(fall(rt, a(0), a(1), a(2))),
        // NeoForge's `EventHooks.finalizeMobSpawn(mob, level, difficulty, spawnType, data)`.
        "finalizeMobSpawn" if owner == EVENT_HOOKS => {
            if !is_made(rt, a(0)) {
                return None;
            }
            rt.put_field(a(0), "$finalize", V::I(1));
            return Some(a(4));
        }
        "<init>" => return construct(rt, owner, desc, args),
        _ => {}
    }
    if !is_made(rt, a(0)) {
        return None;
    }
    made_call(rt, name, desc, args)
}

/// A vanilla class's constructor run on an object of that very class (`new PrimedTnt(...)`, not a
/// mod subclass's `super(...)`): the entity as it leaves it.
fn construct(rt: &mut Rt, owner: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let d = |i: usize| a(i).d();
    let e = a(0);
    if !matches!(e, V::R(n) if n != 0) || rt.class_of(e) != owner {
        return None;
    }
    match (owner, desc) {
        (_, TYPE_LEVEL) => {
            let kind = crate::mod_entities::type_id(rt, a(1)).map(str::to_owned)?;
            if !crate::vanilla_entities::is_vanilla_type(&kind) {
                return None;
            }
            begin(rt, e, &kind, a(2));
        }
        (ARMOR_STAND, AT) => {
            begin(rt, e, "minecraft:armor_stand", a(1));
            set_pos(rt, e, [d(2), d(3), d(4)]);
        }
        // `PrimedTnt(level, x, y, z, owner)`: lit for 80 ticks, hopping up and off a random way.
        (TNT, TNT_AT) => {
            begin(rt, e, "minecraft:tnt", a(1));
            set_pos(rt, e, [d(2), d(3), d(4)]);
            let angle = unit(rt) * f64::from(std::f64::consts::TAU as f32);
            set_motion(rt, e, [-angle.sin() * 0.02, f64::from(0.2f32), -angle.cos() * 0.02]);
            put(rt, e, "fuse", Nbt::Short(80));
        }
        // The private `FallingBlockEntity(level, x, y, z, state)` (mods reach it by an access
        // transformer): the block falling from there, not taken from the world.
        (FALLING, FALLING_AT) => {
            begin(rt, e, "minecraft:falling_block", a(1));
            set_pos(rt, e, [d(2), d(3), d(4)]);
            let state = rt.str_of(a(5)).unwrap_or("minecraft:sand").to_owned();
            put(rt, e, "BlockState", block_state_nbt(&state));
        }
        (FIREWORK, ROCKET_AT) => rocket(rt, e, a(1), [d(2), d(3), d(4)], a(5), V::NULL, false),
        (FIREWORK, ROCKET_ANGLED) => rocket(rt, e, a(1), [d(3), d(4), d(5)], a(2), V::NULL, a(6).i() != 0),
        (FIREWORK, ROCKET_BY) => rocket(rt, e, a(1), [d(3), d(4), d(5)], a(6), a(2), false),
        (FIREWORK, ROCKET_BY_ANGLED) => rocket(rt, e, a(1), [d(4), d(5), d(6)], a(2), a(3), a(7).i() != 0),
        _ => return None,
    }
    Some(V::Void)
}

/// `FireworkRocketEntity(level, x, y, z, stack)`: the rocket of `stack`, flying 10 ticks per its
/// flight duration plus one, and up to 11 more, rising 0.05 a tick with a little drift; owned by
/// `shooter` when there is one, angled when shot from a crossbow or a dispenser.
fn rocket(rt: &mut Rt, e: V, level: V, at: [f64; 3], stack: V, shooter: V, angled: bool) {
    begin(rt, e, "minecraft:firework_rocket", level);
    set_pos(rt, e, at);
    let mut flights = 1;
    if !crate::api_common::stack_empty(rt, stack) {
        let tag = crate::tags::new_compound(rt);
        crate::api_common::save_stack(rt, stack, tag);
        let saved = crate::nbt::from_runtime(rt, tag);
        let flight = match saved.get("components") {
            Some(Nbt::Compound(c)) => match c.get("minecraft:fireworks") {
                Some(Nbt::Compound(f)) => int(f.get("flight_duration")).unwrap_or(0),
                _ => 0,
            },
            _ => 0,
        };
        flights += flight;
        put(rt, e, "FireworksItem", Nbt::Compound(saved));
    }
    let side = triangle(rt, 0.0, 0.002297);
    let ahead = triangle(rt, 0.0, 0.002297);
    set_motion(rt, e, [side, 0.05, ahead]);
    let lifetime = 10 * flights + below(rt, 6) + below(rt, 7);
    put(rt, e, "Life", Nbt::Int(0));
    put(rt, e, "LifeTime", Nbt::Int(lifetime));
    put(rt, e, "ShotAtAngle", Nbt::Byte(i8::from(angled)));
    if let Some(u) = uuid_of(rt, shooter) {
        put(rt, e, "Owner", Nbt::IntArray(uuid_ints(u).to_vec()));
    }
}

/// `FallingBlockEntity.fall(level, pos, state)`: the block at `pos` starts falling from the middle
/// of its floor (no longer waterlogged), what its fluid leaves takes its place, and the falling
/// block is added. On a client, the entity alone.
fn fall(rt: &mut Rt, level: V, pos: V, state: V) -> V {
    let p = api::pos_of(rt, pos);
    let text = rt.str_of(state).unwrap_or("minecraft:sand").to_owned();
    let e = make(rt, "minecraft:falling_block", level);
    set_pos(rt, e, [f64::from(p[0]) + 0.5, f64::from(p[1]), f64::from(p[2]) + 0.5]);
    put(rt, e, "BlockState", block_state_nbt(&text));
    if client_side(rt, level) {
        return e;
    }
    let left = crate::block_breaking::fluid_block(rt, &text);
    rt.host_call(|h| h.set_block_state(p, &left));
    crate::glue::drain_replaced(rt);
    add(rt, level, e);
    e
}

/// A block state as an entity's compound keeps it (`NbtUtils.writeBlockState`: `Name` and
/// `Properties`), no longer waterlogged (a falling block's).
fn block_state_nbt(text: &str) -> Nbt {
    let (block, props) = crate::host::split_state(text);
    let mut tag = BTreeMap::from([("Name".to_owned(), Nbt::String(block.to_owned()))]);
    if !props.is_empty() {
        let props = props.into_iter().map(|(k, v)| (k.to_owned(), Nbt::String(if k == "waterlogged" { "false".to_owned() } else { v.to_owned() }))).collect();
        tag.insert("Properties".to_owned(), Nbt::Compound(props));
    }
    Nbt::Compound(tag)
}

/// Whether `level` is a client's (`isClientSide`), where adding an entity does nothing.
fn client_side(rt: &Rt, level: V) -> bool {
    match (level, rt.get_field(level, "isClientSide")) {
        (V::R(n), V::I(c)) if n != 0 => c != 0,
        _ => rt.host().is_some_and(|h| !h.is_server()),
    }
}

/// `level.addFreshEntity(e)` of an entity a mod made: the game makes it from its compound (a bolt
/// strikes), and `e` stands for the game's entity from then on. False when it didn't: a client's
/// level, an entity discarded or added before, one the game can't make here.
fn add(rt: &mut Rt, level: V, e: V) -> bool {
    if state(rt, e) != HELD || client_side(rt, level) {
        return false;
    }
    let kind = kind(rt, e);
    let at = xyz(rt, e);
    let id = if kind == "minecraft:lightning_bolt" {
        let visual = rt.get_field(e, "$visualOnly").i() != 0;
        let cause = uuid_of(rt, rt.get_field(e, "$cause"));
        rt.host_call(|h| h.strike_lightning(at, visual, cause)).flatten()
    } else {
        let nbt = compound(rt, e);
        rt.host_call(|h| h.summon(&kind, at, &nbt)).flatten()
    };
    let Some(id) = id else {
        return false;
    };
    rt.put_field(e, "$made", V::I(0));
    crate::vanilla_entities::adopt(rt, e, id, &kind);
    true
}

/// The compound the game makes the entity from: what the mod set and loaded, with its motion and
/// facing; its place is the summon's own.
fn compound(rt: &mut Rt, e: V) -> BTreeMap<String, Nbt> {
    let mut nbt = crate::nbt::from_runtime(rt, rt.get_field(e, "$nbt"));
    for k in ["id", "Pos"] {
        nbt.remove(k);
    }
    let m = motion(rt, e);
    nbt.insert("Motion".into(), Nbt::List(m.iter().map(|&v| Nbt::Double(v)).collect()));
    let rot = [rt.get_field(e, "$yRot").f(), rt.get_field(e, "$xRot").f()];
    nbt.insert("Rotation".into(), Nbt::List(rot.iter().map(|&v| Nbt::Float(v)).collect()));
    if rt.get_field(e, "$finalize").i() != 0 {
        // The game's request to finish a mob as `finalizeSpawn` does (its equipment, variant...).
        nbt.insert("horizon:finalize".into(), Nbt::Byte(1));
    }
    nbt
}

/// What `saveWithoutId` writes: the compound with its place, motion and facing, and the mod data
/// it carries (`NeoForgeData`).
fn saved(rt: &mut Rt, e: V) -> BTreeMap<String, Nbt> {
    let mut nbt = crate::nbt::from_runtime(rt, rt.get_field(e, "$nbt"));
    let p = xyz(rt, e);
    nbt.insert("Pos".into(), Nbt::List(p.iter().map(|&v| Nbt::Double(v)).collect()));
    let m = motion(rt, e);
    nbt.insert("Motion".into(), Nbt::List(m.iter().map(|&v| Nbt::Double(v)).collect()));
    let rot = [rt.get_field(e, "$yRot").f(), rt.get_field(e, "$xRot").f()];
    nbt.insert("Rotation".into(), Nbt::List(rot.iter().map(|&v| Nbt::Float(v)).collect()));
    let data = rt.get_field(e, "$data");
    if data.r() != 0 {
        let d = crate::nbt::from_runtime(rt, data);
        if !d.is_empty() {
            nbt.insert("NeoForgeData".into(), Nbt::Compound(d));
        }
    }
    nbt
}

/// `Entity.load(tag)`: the entity becomes what the compound says: its place, motion (none past 10
/// a tick) and facing where it gives them, its mod data (`NeoForgeData`), the rest its compound.
fn load(rt: &mut Rt, e: V, tag: V) {
    let mut nbt = crate::nbt::from_runtime(rt, tag);
    if let Some(p) = doubles(nbt.get("Pos")) {
        set_pos(rt, e, p);
    }
    if let Some(m) = doubles(nbt.get("Motion")) {
        set_motion(rt, e, m.map(|v| if v.abs() > 10.0 { 0.0 } else { v }));
    }
    if let Some(Nbt::List(r)) = nbt.get("Rotation")
        && let [Nbt::Float(y), Nbt::Float(x)] = r.as_slice()
    {
        rt.put_field(e, "$yRot", V::F(*y));
        rt.put_field(e, "$xRot", V::F(*x));
    }
    if let Some(Nbt::Compound(d)) = nbt.remove("NeoForgeData") {
        let data = crate::nbt::to_runtime(rt, &d);
        rt.put_field(e, "$data", data);
    }
    for k in ["Pos", "Motion", "Rotation"] {
        nbt.remove(k);
    }
    let fresh = crate::nbt::to_runtime(rt, &nbt);
    rt.put_field(e, "$nbt", fresh);
    rt.put_field(e, "$name", V::NULL);
}

fn doubles(n: Option<&Nbt>) -> Option<[f64; 3]> {
    match n? {
        Nbt::List(l) => match l.as_slice() {
            [Nbt::Double(x), Nbt::Double(y), Nbt::Double(z)] => Some([*x, *y, *z]),
            _ => None,
        },
        _ => None,
    }
}

fn int(n: Option<&Nbt>) -> Option<i32> {
    Some(match n? {
        Nbt::Byte(b) => i32::from(*b),
        Nbt::Short(s) => i32::from(*s),
        Nbt::Int(i) => *i,
        Nbt::Long(l) => *l as i32,
        _ => return None,
    })
}

/// Sets `key` of the entity's compound to `value`.
fn put(rt: &mut Rt, e: V, key: &str, value: Nbt) {
    let one = crate::nbt::to_runtime(rt, &BTreeMap::from([(key.to_owned(), value)]));
    merge(rt, rt.get_field(e, "$nbt"), one);
}

/// Sets `key` to a byte boolean when `on`, else leaves it out (as `saveWithoutId` writes
/// `NoGravity`, `Silent`, `Glowing`...).
fn flag(rt: &mut Rt, e: V, key: &str, on: bool) {
    if on {
        put(rt, e, key, Nbt::Byte(1));
    } else {
        remove(rt, e, key);
    }
}

fn remove(rt: &mut Rt, e: V, key: &str) {
    let tag = rt.get_field(e, "$nbt");
    if let Some(Data::Map(m)) = rt.heap.get_mut(tag.r() as usize).map(|o| &mut o.data) {
        m.remove(key);
        m.remove(&crate::nbt::kind_key(key));
    }
}

fn get(rt: &Rt, e: V, key: &str) -> Option<Nbt> {
    crate::nbt::from_runtime(rt, rt.get_field(e, "$nbt")).remove(key)
}

fn has(rt: &Rt, e: V, key: &str) -> bool {
    int(get(rt, e, key).as_ref()).is_some_and(|v| v != 0)
}

/// Copies compound `from`'s entries, with the kinds they were put as, into compound `to`.
pub(crate) fn merge(rt: &mut Rt, to: V, from: V) {
    let entries = match rt.heap.get(from.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) => m.clone(),
        _ => return,
    };
    if let Some(Data::Map(m)) = rt.heap.get_mut(to.r() as usize).map(|o| &mut o.data) {
        m.extend(entries);
    }
}

fn kind(rt: &Rt, e: V) -> String {
    rt.str_of(rt.get_field(e, "$kind")).unwrap_or("minecraft:pig").to_owned()
}

fn xyz(rt: &Rt, e: V) -> [f64; 3] {
    [rt.get_field(e, "$x").d(), rt.get_field(e, "$y").d(), rt.get_field(e, "$z").d()]
}

fn motion(rt: &Rt, e: V) -> [f64; 3] {
    [rt.get_field(e, "$mx").d(), rt.get_field(e, "$my").d(), rt.get_field(e, "$mz").d()]
}

fn set_pos(rt: &mut Rt, e: V, p: [f64; 3]) {
    for (f, v) in ["$x", "$y", "$z"].into_iter().zip(p) {
        rt.put_field(e, f, V::D(v));
    }
}

fn set_motion(rt: &mut Rt, e: V, m: [f64; 3]) {
    for (f, v) in ["$mx", "$my", "$mz"].into_iter().zip(m) {
        rt.put_field(e, f, V::D(v));
    }
}

/// `setYRot`, `setXRot`: a value that isn't finite is refused.
fn set_rot(rt: &mut Rt, e: V, field: &'static str, v: f32) {
    if v.is_finite() {
        rt.put_field(e, field, V::F(v));
    }
}

fn vec_of(rt: &Rt, v: V) -> [f64; 3] {
    ["x", "y", "z"].map(|k| rt.get_field(v, k).d())
}

/// A double in [0, 1) from the runtime's random (`RandomSource.nextDouble`).
fn unit(rt: &mut Rt) -> f64 {
    (crate::api_common::next_random(rt) >> 11) as f64 / (1u64 << 53) as f64
}

/// `RandomSource.nextInt(bound)`.
fn below(rt: &mut Rt, bound: i32) -> i32 {
    (unit(rt) * f64::from(bound)) as i32
}

/// `RandomSource.triangle(mode, deviation)`: `mode + deviation * (nextDouble() - nextDouble())`.
fn triangle(rt: &mut Rt, mode: f64, deviation: f64) -> f64 {
    mode + deviation * (unit(rt) - unit(rt))
}

/// `Mth.wrapDegrees(float)`: into [-180, 180).
fn wrap_degrees(v: f32) -> f32 {
    let w = v % 360.0;
    if w >= 180.0 {
        w - 360.0
    } else if w < -180.0 {
        w + 360.0
    } else {
        w
    }
}

/// The UUID of entity `e` (a player, a mod's or the game's entity), none for null.
fn uuid_of(rt: &mut Rt, e: V) -> Option<u128> {
    if e.r() == 0 {
        return None;
    }
    let u = api::call(rt, GET_UUID, &[e]);
    if u.r() == 0 {
        return None;
    }
    let (msb, lsb) = crate::tags::uuid_bits(rt, u);
    Some((u128::from(msb as u64) << 64) | u128::from(lsb as u64))
}

/// A UUID as a compound keeps it: four ints, most significant first.
fn uuid_ints(u: u128) -> [i32; 4] {
    [(u >> 96) as i32, (u >> 64) as i32, (u >> 32) as i32, u as i32]
}

/// `EquipmentSlot`'s name as the game's `equipment` compound keys it (`mainhand`, `head`...).
fn slot_name(rt: &Rt, slot: V) -> Option<String> {
    let name = rt.str_of(rt.get_field(slot, "$name"))?;
    Some(name.to_ascii_lowercase())
}

/// Vanilla's methods for an entity a mod made and hasn't added: what they read of it, and what
/// they set into its fields and compound.
fn made_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    let class = rt.class_of(e);
    let kind = kind(rt, e);
    let held = state(rt, e) == HELD;
    let p = xyz(rt, e);
    let xyz_arg = |rt: &Rt| if desc.starts_with("(DDD") { [a(1).d(), a(2).d(), a(3).d()] } else { vec_of(rt, a(1)) };
    Some(match name {
        "getX" if desc == "()D" => V::D(p[0]),
        "getY" if desc == "()D" => V::D(p[1]),
        "getZ" if desc == "()D" => V::D(p[2]),
        "getBlockX" | "getBlockY" | "getBlockZ" => V::I(p[usize::from(name.as_bytes()[8] - b'X')].floor() as i32),
        "position" | "getPosition" => crate::api_common::vec3(rt, p[0], p[1], p[2]),
        "blockPosition" => api::block_pos(rt, p[0].floor() as i64, p[1].floor() as i64, p[2].floor() as i64),
        "getYRot" | "getYHeadRot" | "getVisualRotationYInDegrees" => rt.get_field(e, "$yRot"),
        "getXRot" => rt.get_field(e, "$xRot"),
        "getDeltaMovement" => {
            let m = motion(rt, e);
            crate::api_common::vec3(rt, m[0], m[1], m[2])
        }
        "getType" => crate::mod_entities::vanilla_type(rt, &kind),
        "level" | "getCommandSenderWorld" => match rt.get_field(e, "$level") {
            l @ V::R(n) if n != 0 => l,
            _ => crate::glue::level(rt),
        },
        "registryAccess" => {
            let level = match rt.get_field(e, "$level") {
                l @ V::R(n) if n != 0 => l,
                _ => crate::glue::level(rt),
            };
            api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[level])
        }
        // An id comes with the game's entity, once added.
        "getId" => V::I(0),
        "getUUID" | "getStringUUID" => {
            if rt.get_field(e, "$uuid").r() == 0 {
                let (msb, lsb) = (crate::api_common::next_random(rt) as i64, crate::api_common::next_random(rt) as i64);
                // Version 4, IETF variant, as `UUID.randomUUID()`.
                let u = crate::tags::uuid(rt, (msb & !0xf000) | 0x4000, (lsb & 0x3fff_ffff_ffff_ffff) | i64::MIN);
                rt.put_field(e, "$uuid", u);
            }
            let u = rt.get_field(e, "$uuid");
            if name == "getUUID" {
                u
            } else {
                let (msb, lsb) = crate::tags::uuid_bits(rt, u);
                rt.string(&crate::tags::uuid_text(msb, lsb))
            }
        }
        "isAlive" => V::I(i32::from(held)),
        "isRemoved" => V::I(i32::from(!held)),
        "getRemovalReason" => match state(rt, e) {
            DISCARDED => crate::api::get_static(
                rt,
                "net/minecraft/world/entity/Entity$RemovalReason.DISCARDED",
            ),
            _ => V::NULL,
        },
        // Not in a level yet: nothing rides it and it rides nothing.
        "isPassenger" | "isVehicle" => V::I(0),
        "getVehicle" | "getControllingPassenger" | "getFirstPassenger"
            if desc.starts_with("()") =>
        {
            V::NULL
        }
        "getRootVehicle" => e,
        "getPassengers" => crate::jutil::new_list(rt, Vec::new()),
        "discard" | "remove" | "setRemoved" | "kill" => {
            rt.put_field(e, "$made", V::I(DISCARDED));
            V::Void
        }
        "getBoundingBox" => {
            let (w, h, _) = crate::vanilla_entities::dimensions(&kind);
            let hw = f64::from(w) / 2.0;
            crate::shapes::aabb(rt, [p[0] - hw, p[1], p[2] - hw, p[0] + hw, p[1] + f64::from(h), p[2] + hw])
        }
        "getBbWidth" => V::F(crate::vanilla_entities::dimensions(&kind).0),
        "getBbHeight" => V::F(crate::vanilla_entities::dimensions(&kind).1),
        "getEyeHeight" => V::F(crate::vanilla_entities::dimensions(&kind).2),
        "getEyeY" => V::D(p[1] + f64::from(crate::vanilla_entities::dimensions(&kind).2)),
        "getPose" => {
            crate::api_common::plain_enum(rt, "net/minecraft/world/entity/Pose", "STANDING")
                .unwrap_or(V::NULL)
        }
        "getDimensions" | "getDefaultDimensions"
            if desc == crate::vanilla_entities::POSE_DIMENSIONS =>
        {
            let living = crate::vanilla_entities::is_living_type(&kind);
            crate::vanilla_entities::pose_dimensions(rt, &kind, living, name, a(1))
        }

        // Where it is and which way it faces.
        "setPos" | "setPosRaw" if desc.starts_with("(DDD)") || desc.starts_with("(Lnet/minecraft/world/phys/Vec3;)") => {
            let to = xyz_arg(rt);
            set_pos(rt, e, to);
            V::Void
        }
        "moveTo" | "teleportTo" if desc.starts_with("(DDD") || desc.starts_with("(Lnet/minecraft/world/phys/Vec3;") => {
            let to = xyz_arg(rt);
            set_pos(rt, e, to);
            let rot = match desc {
                "(DDDFF)V" => Some((a(4).f(), a(5).f())),
                "(Lnet/minecraft/world/phys/Vec3;FF)V" => Some((a(2).f(), a(3).f())),
                _ => None,
            };
            if let Some((y, x)) = rot {
                set_rot(rt, e, "$yRot", y);
                set_rot(rt, e, "$xRot", x);
            }
            V::Void
        }
        // `moveTo(pos, yRot, xRot)`: the middle of the block's floor.
        "moveTo" if desc == "(Lnet/minecraft/core/BlockPos;FF)V" => {
            let b = api::pos_of(rt, a(1));
            set_pos(rt, e, [f64::from(b[0]) + 0.5, f64::from(b[1]), f64::from(b[2]) + 0.5]);
            set_rot(rt, e, "$yRot", a(2).f());
            set_rot(rt, e, "$xRot", a(3).f());
            V::Void
        }
        // `absMoveTo`: x and z within the world's 30 million, a pitch within ±90.
        "absMoveTo" if desc.starts_with("(DDD") => {
            set_pos(rt, e, [a(1).d().clamp(-3.0e7, 3.0e7), a(2).d(), a(3).d().clamp(-3.0e7, 3.0e7)]);
            if desc == "(DDDFF)V" {
                set_rot(rt, e, "$yRot", a(4).f() % 360.0);
                set_rot(rt, e, "$xRot", a(5).f().clamp(-90.0, 90.0) % 360.0);
            }
            V::Void
        }
        "copyPosition" => {
            let other = a(1);
            let at = ["getX.()D", "getY.()D", "getZ.()D"].map(|k| {
                let key = rt.static_key(&format!("net/minecraft/world/entity/Entity.{k}"));
                api::virtual_call(rt, (key, k), &[other]).d()
            });
            set_pos(rt, e, at);
            for (k, f) in [("getYRot.()F", "$yRot"), ("getXRot.()F", "$xRot")] {
                let key = rt.static_key(&format!("net/minecraft/world/entity/Entity.{k}"));
                let v = api::virtual_call(rt, (key, k), &[other]).f();
                set_rot(rt, e, f, v);
            }
            V::Void
        }
        "setYRot" | "setYHeadRot" | "setYBodyRot" if desc == "(F)V" => {
            if name == "setYRot" {
                set_rot(rt, e, "$yRot", a(1).f());
            }
            V::Void
        }
        "setXRot" if desc == "(F)V" => {
            set_rot(rt, e, "$xRot", a(1).f());
            V::Void
        }
        "setRot" if desc == "(FF)V" => {
            set_rot(rt, e, "$yRot", a(1).f() % 360.0);
            set_rot(rt, e, "$xRot", a(2).f() % 360.0);
            V::Void
        }

        // How it moves.
        "setDeltaMovement" => {
            let m = xyz_arg(rt);
            set_motion(rt, e, m);
            V::Void
        }
        "addDeltaMovement" | "push" if desc.starts_with("(DDD)") || desc.starts_with("(Lnet/minecraft/world/phys/Vec3;)") => {
            let by = xyz_arg(rt);
            let m = motion(rt, e);
            set_motion(rt, e, [m[0] + by[0], m[1] + by[1], m[2] + by[2]]);
            V::Void
        }
        // `Projectile.shoot(x, y, z, velocity, inaccuracy)`: along the direction, scattered by the
        // inaccuracy, facing the way it goes.
        "shoot" if desc == "(DDDFF)V" && is_a(class, PROJECTILE) => {
            let d = [a(1).d(), a(2).d(), a(3).d()];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let n = if len < f64::from(1.0e-5f32) { [0.0; 3] } else { d.map(|v| v / len) };
            let spread = 0.0172275 * f64::from(a(5).f());
            let mut m = [0.0; 3];
            for (out, v) in m.iter_mut().zip(n) {
                *out = (v + triangle(rt, 0.0, spread)) * f64::from(a(4).f());
            }
            set_motion(rt, e, m);
            let h = (m[0] * m[0] + m[2] * m[2]).sqrt();
            rt.put_field(e, "$yRot", V::F((m[0].atan2(m[2]) * 180.0 / std::f64::consts::PI) as f32));
            rt.put_field(e, "$xRot", V::F((m[1].atan2(h) * 180.0 / std::f64::consts::PI) as f32));
            V::Void
        }

        // Its name and flags, as its compound keeps them.
        "setCustomName" => {
            rt.put_field(e, "$name", a(1));
            if a(1).r() == 0 {
                remove(rt, e, "CustomName");
            } else {
                let json = crate::api_common::component_json(rt, a(1));
                if let Some(n) = crate::recipes::json_nbt(&json) {
                    put(rt, e, "CustomName", n);
                }
            }
            V::Void
        }
        "getCustomName" => rt.get_field(e, "$name"),
        "hasCustomName" => V::I(i32::from(rt.get_field(e, "$name").r() != 0)),
        "setCustomNameVisible" => {
            flag(rt, e, "CustomNameVisible", a(1).i() != 0);
            V::Void
        }
        "isCustomNameVisible" => V::I(i32::from(has(rt, e, "CustomNameVisible"))),
        "setNoGravity" | "setSilent" | "setInvisible" | "setGlowingTag" | "setInvulnerable" if desc == "(Z)V" => {
            let key = match name {
                "setNoGravity" => "NoGravity",
                "setSilent" => "Silent",
                "setInvisible" => "Invisible",
                "setGlowingTag" => "Glowing",
                _ => "Invulnerable",
            };
            flag(rt, e, key, a(1).i() != 0);
            V::Void
        }
        "isNoGravity" => V::I(i32::from(has(rt, e, "NoGravity"))),
        "isSilent" => V::I(i32::from(has(rt, e, "Silent"))),
        "isInvisible" => V::I(i32::from(has(rt, e, "Invisible"))),
        "isInvulnerable" => V::I(i32::from(has(rt, e, "Invulnerable"))),
        "isCurrentlyGlowing" | "hasGlowingTag" => V::I(i32::from(has(rt, e, "Glowing"))),
        "setRemainingFireTicks" => {
            put(rt, e, "Fire", Nbt::Short(a(1).i().clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16));
            V::Void
        }
        "setTicksFrozen" => {
            put(rt, e, "TicksFrozen", Nbt::Int(a(1).i()));
            V::Void
        }
        "setAirSupply" => {
            put(rt, e, "Air", Nbt::Short(a(1).i().clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16));
            V::Void
        }
        // `addTag`: up to 1024, each once.
        "addTag" | "removeTag" => {
            let tag = rt.str_of(a(1)).unwrap_or("").to_owned();
            let mut tags: Vec<String> = match get(rt, e, "Tags") {
                Some(Nbt::List(l)) => l.into_iter().filter_map(|t| if let Nbt::String(s) = t { Some(s) } else { None }).collect(),
                _ => Vec::new(),
            };
            let changed = if name == "addTag" {
                let add = tags.len() < 1024 && !tags.contains(&tag);
                if add {
                    tags.push(tag);
                }
                add
            } else {
                let had = tags.len();
                tags.retain(|t| *t != tag);
                tags.len() != had
            };
            if tags.is_empty() {
                remove(rt, e, "Tags");
            } else {
                put(rt, e, "Tags", Nbt::List(tags.into_iter().map(Nbt::String).collect()));
            }
            V::I(i32::from(changed))
        }
        "getPersistentData" => crate::entities::persistent_data(rt, e),

        // Its compound, in and out.
        "load" | "deserializeNBT" => {
            load(rt, e, a(args.len() - 1));
            V::Void
        }
        "saveWithoutId" => {
            let s = saved(rt, e);
            let out = crate::nbt::to_runtime(rt, &s);
            merge(rt, a(1), out);
            a(1)
        }
        "serializeNBT" => {
            let mut s = saved(rt, e);
            s.insert("id".into(), Nbt::String(kind.clone()));
            crate::nbt::to_runtime(rt, &s)
        }

        // A lightning bolt's: only its flash and sound, who called it down, its damage (the game's
        // bolt deals vanilla's).
        "setVisualOnly" => {
            rt.put_field(e, "$visualOnly", V::I(i32::from(a(1).i() != 0)));
            V::Void
        }
        "setCause" => {
            rt.put_field(e, "$cause", a(1));
            V::Void
        }
        "getCause" => rt.get_field(e, "$cause"),
        "setDamage" => {
            rt.put_field(e, "$damage", a(1));
            V::Void
        }
        // A TNT's fuse.
        "setFuse" if is_a(class, TNT) => {
            put(rt, e, "fuse", Nbt::Short(a(1).i().clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16));
            V::Void
        }
        "getFuse" if is_a(class, TNT) => V::I(int(get(rt, e, "fuse").as_ref()).unwrap_or(80)),
        // A falling block's damage to what it lands on.
        "setHurtsEntities" if is_a(class, FALLING) => {
            put(rt, e, "HurtEntities", Nbt::Byte(1));
            put(rt, e, "FallHurtAmount", Nbt::Float(a(1).f()));
            put(rt, e, "FallHurtMax", Nbt::Int(a(2).i()));
            V::Void
        }
        // An armor stand's look.
        "setShowArms" | "setNoBasePlate" | "setSmall" | "setMarker" if is_a(class, ARMOR_STAND) => {
            let key = match name {
                "setShowArms" => "ShowArms",
                "setNoBasePlate" => "NoBasePlate",
                "setSmall" => "Small",
                _ => "Marker",
            };
            flag(rt, e, key, a(1).i() != 0);
            V::Void
        }
        "isShowArms" | "isNoBasePlate" | "isSmall" | "isMarker" if is_a(class, ARMOR_STAND) => {
            let key = &name[2..];
            V::I(i32::from(has(rt, e, key)))
        }

        // A living entity's and a mob's.
        "setHealth" if is_a(class, LIVING) => {
            put(rt, e, "Health", Nbt::Float(a(1).f()));
            V::Void
        }
        "setItemSlot" if is_a(class, LIVING) => {
            let Some(slot) = slot_name(rt, a(1)) else { return Some(V::Void) };
            let mut equipment = match get(rt, e, "equipment") {
                Some(Nbt::Compound(c)) => c,
                _ => BTreeMap::new(),
            };
            if crate::api_common::stack_empty(rt, a(2)) {
                equipment.remove(&slot);
            } else {
                let tag = crate::tags::new_compound(rt);
                crate::api_common::save_stack(rt, a(2), tag);
                equipment.insert(slot, Nbt::Compound(crate::nbt::from_runtime(rt, tag)));
            }
            if equipment.is_empty() {
                remove(rt, e, "equipment");
            } else {
                put(rt, e, "equipment", Nbt::Compound(equipment));
            }
            V::Void
        }
        "getItemBySlot" if is_a(class, LIVING) => {
            let slot = slot_name(rt, a(1));
            let saved = match (get(rt, e, "equipment"), slot) {
                (Some(Nbt::Compound(mut c)), Some(s)) => match c.remove(&s) {
                    Some(Nbt::Compound(t)) => crate::api_common::parse_stack(rt, &t),
                    _ => None,
                },
                _ => None,
            };
            match saved {
                Some(s) => s,
                None => crate::api_common::get_static(rt, ITEM_STACK, "EMPTY").unwrap_or(V::NULL),
            }
        }
        "setPersistenceRequired" if is_a(class, MOB) => {
            put(rt, e, "PersistenceRequired", Nbt::Byte(1));
            V::Void
        }
        "setNoAi" | "setLeftHanded" if is_a(class, MOB) => {
            flag(rt, e, if name == "setNoAi" { "NoAI" } else { "LeftHanded" }, a(1).i() != 0);
            V::Void
        }
        // A baby: an ageable mob's age, a zombie's or piglin's flag.
        "setBaby" if is_a(class, MOB) => {
            let baby = a(1).i() != 0;
            if is_a(class, AGEABLE) {
                put(rt, e, "Age", Nbt::Int(if baby { -24000 } else { 0 }));
            } else {
                flag(rt, e, "IsBaby", baby);
            }
            V::Void
        }
        "setAge" if is_a(class, AGEABLE) => {
            put(rt, e, "Age", Nbt::Int(a(1).i()));
            V::Void
        }
        // `finalizeSpawn(level, difficulty, spawnType, data)`: done as the game adds the mob.
        "finalizeSpawn" if is_a(class, MOB) => {
            rt.put_field(e, "$finalize", V::I(1));
            a(4)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityView, MemoryHost};

    fn level(rt: &mut Rt) -> V {
        crate::glue::level(rt)
    }

    /// A mod calls lightning down on a block (Iron's Spellbooks, Mekanism): the bolt strikes there
    /// credited to the player who cast it, a visual one only flashes, and the bolt is then the
    /// game's entity; one discarded before it was added never strikes.
    #[test]
    fn a_mods_lightning_strikes_through_the_game() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let uuid = 0x1234_5678_9abc_def0_0fed_cba9_8765_4321u128;
        let (bolt_id, is_bolt) = rt.with_host(&mut host, |rt| {
            let level = level(rt);
            let ty = api::get_static(rt, "net/minecraft/world/entity/EntityType.LIGHTNING_BOLT");
            let bolt = api::call(rt, CREATE, &[ty, level]);
            assert!(rt.instance_of(bolt, "net/minecraft/world/entity/LightningBolt"));
            let at = crate::api_common::vec3(rt, 3.5, 64.0, -2.5);
            api::call(rt, "net/minecraft/world/entity/LightningBolt.moveTo.(Lnet/minecraft/world/phys/Vec3;)V", &[bolt, at]);
            let player = crate::menus::server_player(rt, uuid);
            api::call(rt, "net/minecraft/world/entity/LightningBolt.setCause.(Lnet/minecraft/server/level/ServerPlayer;)V", &[bolt, player]);
            let added = api::call(rt, ADD, &[level, bolt]);
            assert_eq!(added, V::I(1));
            let again = api::call(rt, ADD, &[level, bolt]);
            assert_eq!(again, V::I(0), "the level holds it already");

            let flash = api::call(rt, CREATE, &[ty, level]);
            api::call(rt, "net/minecraft/world/entity/LightningBolt.setVisualOnly.(Z)V", &[flash, V::I(1)]);
            api::call(rt, "net/minecraft/world/entity/Entity.setPos.(DDD)V", &[flash, V::D(0.5), V::D(70.0), V::D(0.5)]);
            api::call(rt, ADD, &[level, flash]);

            let gone = api::call(rt, CREATE, &[ty, level]);
            api::call(rt, "net/minecraft/world/entity/Entity.discard.()V", &[gone]);
            assert_eq!(api::call(rt, ADD, &[level, gone]), V::I(0));

            let id = api::call(rt, "net/minecraft/world/entity/Entity.getId.()I", &[bolt]);
            (id, rt.instance_of(bolt, "net/minecraft/world/entity/LightningBolt"))
        });
        assert_eq!(host.lightning, vec![([3.5, 64.0, -2.5], false, Some(uuid)), ([0.5, 70.0, 0.5], true, None)]);
        assert_eq!(bolt_id, V::I(1000), "the game's bolt");
        assert!(is_bolt);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create's mounted dispenser lights TNT: `new PrimedTnt(level, x, y, z, null)`, pushed with the
    /// contraption's motion, added. The game gets a TNT there with vanilla's 80-tick fuse and its
    /// hop: 0.2 up and 0.02 off a random way, plus the push.
    #[test]
    fn a_mods_tnt_is_summoned_lit_with_its_hop() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let tnt_class = rt.with_host(&mut host, |rt| {
            let level = level(rt);
            let tnt = rt.new_object(TNT);
            api::call(rt, format!("{TNT}.<init>.{TNT_AT}").leak(), &[tnt, level, V::D(10.5), V::D(65.5), V::D(-3.5), V::NULL]);
            api::call(rt, "net/minecraft/world/entity/item/PrimedTnt.push.(DDD)V", &[tnt, V::D(0.25), V::D(0.0), V::D(0.0)]);
            assert_eq!(api::call(rt, "net/minecraft/world/entity/item/PrimedTnt.getFuse.()I", &[tnt]), V::I(80));
            let added = api::call(rt, ADD, &[level, tnt]);
            assert_eq!(added, V::I(1));
            // Then read as Create does, for the sound: where the game's TNT is.
            let x = api::call(rt, "net/minecraft/world/entity/item/PrimedTnt.getX.()D", &[tnt]);
            assert_eq!(x, V::D(10.5));
            rt.class_of(tnt)
        });
        assert_eq!(tnt_class, TNT);
        let [(kind, at, nbt)] = host.summoned.as_slice() else { panic!("{:?}", host.summoned) };
        assert_eq!((kind.as_str(), *at), ("minecraft:tnt", [10.5, 65.5, -3.5]));
        assert_eq!(nbt.get("fuse"), Some(&Nbt::Short(80)));
        let Some(Nbt::List(m)) = nbt.get("Motion") else { panic!("{nbt:?}") };
        let [Nbt::Double(mx), Nbt::Double(my), Nbt::Double(mz)] = m.as_slice() else { panic!("{m:?}") };
        let off = ((mx - 0.25).powi(2) + mz.powi(2)).sqrt();
        assert!((off - 0.02).abs() < 1e-9, "a hop of 0.02 sideways: {off}");
        assert_eq!(*my, f64::from(0.2f32));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create's haunting turns a horse into a skeleton horse: the new one is loaded from the horse's
    /// saved compound (less its UUID), stands where the horse stood, and is added; the horse's armor
    /// drops, and the horse goes. The game makes the skeleton horse from that compound.
    #[test]
    fn a_horse_haunted_by_create_becomes_a_skeleton_horse() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.entities.push(EntityView { id: 7, kind: "minecraft:horse".into(), pos: [4.5, 64.0, 8.5], yaw: 30.0, health: Some((22.0, 30.0)) });
        let armor = BTreeMap::from([("id".to_owned(), Nbt::String("minecraft:iron_horse_armor".into())), ("count".to_owned(), Nbt::Int(1))]);
        host.entity_nbts.insert(
            7,
            BTreeMap::from([
                ("Health".to_owned(), Nbt::Float(22.0)),
                ("Tame".to_owned(), Nbt::Byte(1)),
                ("UUID".to_owned(), Nbt::IntArray(vec![1, 2, 3, 4])),
                ("Pos".to_owned(), Nbt::List(vec![Nbt::Double(4.5), Nbt::Double(64.0), Nbt::Double(8.5)])),
                ("equipment".to_owned(), Nbt::Compound(BTreeMap::from([("body".to_owned(), Nbt::Compound(armor))]))),
            ]),
        );
        rt.with_host(&mut host, |rt| {
            let level = level(rt);
            let horse = crate::vanilla_entities::of(rt, 7, "minecraft:horse", None);
            assert!(rt.instance_of(horse, "net/minecraft/world/entity/animal/horse/Horse"));
            let ty = api::get_static(rt, "net/minecraft/world/entity/EntityType.SKELETON_HORSE");
            let skeleton = api::call(rt, CREATE, &[ty, level]);
            let tag = crate::tags::new_compound(rt);
            api::call(rt, "net/minecraft/world/entity/animal/horse/Horse.saveWithoutId.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;", &[horse, tag]);
            let uuid = rt.string("UUID");
            api::call(rt, "net/minecraft/nbt/CompoundTag.remove.(Ljava/lang/String;)V", &[tag, uuid]);
            let body = api::call(rt, "net/minecraft/world/entity/animal/horse/Horse.getBodyArmorItem.()Lnet/minecraft/world/item/ItemStack;", &[horse]);
            assert_eq!(crate::api_common::item_like_name(rt, rt.get_field(body, "$item")), "minecraft:iron_horse_armor");
            api::call(rt, "net/minecraft/world/entity/animal/horse/Horse.spawnAtLocation.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/entity/item/ItemEntity;", &[horse, body]);
            let access = api::call(rt, "net/minecraft/world/entity/Entity.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[horse]);
            api::call(rt, "net/minecraft/world/entity/animal/horse/SkeletonHorse.deserializeNBT.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)V", &[skeleton, access, tag]);
            let at = api::call(rt, "net/minecraft/world/entity/animal/horse/Horse.getPosition.(F)Lnet/minecraft/world/phys/Vec3;", &[horse, V::F(0.0)]);
            api::call(rt, "net/minecraft/world/entity/animal/horse/SkeletonHorse.setPos.(Lnet/minecraft/world/phys/Vec3;)V", &[skeleton, at]);
            assert_eq!(api::call(rt, ADD, &[level, skeleton]), V::I(1));
            api::call(rt, "net/minecraft/world/entity/animal/horse/Horse.discard.()V", &[horse]);
        });
        let [(kind, at, nbt)] = host.summoned.as_slice() else { panic!("{:?}", host.summoned) };
        assert_eq!((kind.as_str(), *at), ("minecraft:skeleton_horse", [4.5, 64.0, 8.5]));
        assert_eq!(nbt.get("Health"), Some(&Nbt::Float(22.0)));
        assert_eq!(nbt.get("Tame"), Some(&Nbt::Byte(1)));
        assert!(!nbt.contains_key("UUID") && !nbt.contains_key("Pos"), "{nbt:?}");
        assert!(host.entities.iter().all(|e| e.id != 7), "the horse went");
        assert_eq!(host.item_entities.len(), 1, "its armor dropped");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `FallingBlockEntity.fall(level, pos, state)` (Create's potato cannon placing a block): the
    /// block falls from the middle of its floor, no longer waterlogged, and what its water leaves
    /// takes its place. `new ArmorStand(level, x, y, z)` keeps its look and name.
    #[test]
    fn a_block_falls_and_an_armor_stand_stands_as_set() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.blocks.insert([2, 70, 3], "minecraft:oak_slab[type=bottom,waterlogged=true]".into());
        rt.with_host(&mut host, |rt| {
            let level = level(rt);
            let pos = api::block_pos(rt, 2, 70, 3);
            let state = rt.string("minecraft:oak_slab[type=bottom,waterlogged=true]");
            let falling = api::call(rt, format!("{FALLING}.fall.{FALL}").leak(), &[level, pos, state]);
            assert!(rt.instance_of(falling, FALLING));
            assert_eq!(api::call(rt, "net/minecraft/world/entity/Entity.getId.()I", &[falling]), V::I(1000));

            let stand = rt.new_object(ARMOR_STAND);
            api::call(rt, format!("{ARMOR_STAND}.<init>.{AT}").leak(), &[stand, level, V::D(0.5), V::D(64.0), V::D(0.5)]);
            api::call(rt, "net/minecraft/world/entity/decoration/ArmorStand.setShowArms.(Z)V", &[stand, V::I(1)]);
            api::call(rt, "net/minecraft/world/entity/decoration/ArmorStand.setNoBasePlate.(Z)V", &[stand, V::I(1)]);
            api::call(rt, "net/minecraft/world/entity/decoration/ArmorStand.setInvisible.(Z)V", &[stand, V::I(0)]);
            let text = rt.string("Bob");
            let name = api::call(rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[text]);
            api::call(rt, "net/minecraft/world/entity/decoration/ArmorStand.setCustomName.(Lnet/minecraft/network/chat/Component;)V", &[stand, name]);
            api::call(rt, "net/minecraft/world/entity/decoration/ArmorStand.moveTo.(DDDFF)V", &[stand, V::D(1.5), V::D(64.0), V::D(2.5), V::F(90.0), V::F(0.0)]);
            api::call(rt, ADD, &[level, stand]);
        });
        assert_eq!(host.blocks.get(&[2, 70, 3]).map(String::as_str), Some("minecraft:water[level=0]"));
        let [(fall_kind, fall_at, fall_nbt), (stand_kind, stand_at, stand_nbt)] = host.summoned.as_slice() else { panic!("{:?}", host.summoned) };
        assert_eq!((fall_kind.as_str(), *fall_at), ("minecraft:falling_block", [2.5, 70.0, 3.5]));
        let state = BTreeMap::from([
            ("Name".to_owned(), Nbt::String("minecraft:oak_slab".into())),
            ("Properties".to_owned(), Nbt::Compound(BTreeMap::from([("type".to_owned(), Nbt::String("bottom".into())), ("waterlogged".to_owned(), Nbt::String("false".into()))]))),
        ]);
        assert_eq!(fall_nbt.get("BlockState"), Some(&Nbt::Compound(state)));
        assert_eq!((stand_kind.as_str(), *stand_at), ("minecraft:armor_stand", [1.5, 64.0, 2.5]));
        assert_eq!(stand_nbt.get("ShowArms"), Some(&Nbt::Byte(1)));
        assert_eq!(stand_nbt.get("NoBasePlate"), Some(&Nbt::Byte(1)));
        assert!(!stand_nbt.contains_key("Invisible"));
        assert_eq!(stand_nbt.get("Rotation"), Some(&Nbt::List(vec![Nbt::Float(90.0), Nbt::Float(0.0)])));
        assert_eq!(stand_nbt.get("CustomName"), Some(&Nbt::Compound(BTreeMap::from([("text".to_owned(), Nbt::String("Bob".into()))]))));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
