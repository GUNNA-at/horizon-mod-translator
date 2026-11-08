//! The mods' own entities as the game's commands see them (docs/native-mods.md, "Mod entities in
//! commands"): those of a level with what a selector matches them by ([`seen`],
//! `native_mod_entities`), `/kill` and `/tp` on one ([`act`], `native_mod_entity_act`), and the
//! compound `/data get entity` and a selector's `nbt=` read ([`compound`],
//! `native_mod_entity_nbt`).

use crate::api;
use crate::host::Nbt;
use crate::{Rt, V};
use std::collections::BTreeMap;

const PATHFINDER_MOB: &str = "net/minecraft/world/entity/PathfinderMob";
const GET_Y_ROT: (&str, &str) = ("net/minecraft/world/entity/Entity.getYRot.()F", "getYRot.()F");
const GET_X_ROT: (&str, &str) = ("net/minecraft/world/entity/Entity.getXRot.()F", "getXRot.()F");
const KILL: (&str, &str) = ("net/minecraft/world/entity/Entity.kill.()V", "kill.()V");
const MOVE_TO: (&str, &str) = ("net/minecraft/world/entity/Entity.moveTo.(DDDFF)V", "moveTo.(DDDFF)V");
const SET_Y_HEAD_ROT: (&str, &str) = ("net/minecraft/world/entity/Entity.setYHeadRot.(F)V", "setYHeadRot.(F)V");
const GET_MOTION: (&str, &str) = (
    "net/minecraft/world/entity/Entity.getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;",
    "getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;",
);
const SET_MOTION: (&str, &str) = ("net/minecraft/world/entity/Entity.setDeltaMovement.(DDD)V", "setDeltaMovement.(DDD)V");
const SET_ON_GROUND: (&str, &str) = ("net/minecraft/world/entity/Entity.setOnGround.(Z)V", "setOnGround.(Z)V");
const GET_NAVIGATION: (&str, &str) = (
    "net/minecraft/world/entity/Mob.getNavigation.()Lnet/minecraft/world/entity/ai/navigation/PathNavigation;",
    "getNavigation.()Lnet/minecraft/world/entity/ai/navigation/PathNavigation;",
);
const STOP: (&str, &str) = ("net/minecraft/world/entity/ai/navigation/PathNavigation.stop.()V", "stop.()V");
const SAVE_WITHOUT_ID: (&str, &str) = (
    "net/minecraft/world/entity/Entity.saveWithoutId.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;",
    "saveWithoutId.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;",
);

/// One of the mods' entities as a selector matches it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    /// Its network id.
    pub id: i32,
    /// Its type's id (`waddles:adelie_penguin`).
    pub kind: String,
    /// Its feet.
    pub position: [f64; 3],
    /// Its yaw and pitch (`getYRot`, `getXRot`).
    pub rotation: [f32; 2],
    pub uuid: u128,
    /// Its box's width and height, and its eyes' height above its feet (`getDimensions(getPose())`).
    pub size: [f32; 3],
    /// `isAlive`: not removed, and a living one's health above 0.
    pub alive: bool,
    /// Its custom name, when it has one. Not modelled for a mod's entity yet: none.
    pub custom_name: Option<String>,
    /// `getTags`. Not modelled for a mod's entity yet: none.
    pub tags: Vec<String>,
}

/// What a command does to one of the mods' entities.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// `/kill`: `Entity.kill()`; a living one dies of `generic_kill`, with its death and drops.
    Kill,
    /// `/tp`: `Entity.teleportTo` the position, at the rotation (yaw, pitch) when one is given,
    /// else its own.
    Teleport { position: [f64; 3], rotation: Option<[f32; 2]> },
}

/// Whether `e` is in the level `dimension` (one that never said which is in every level, as the
/// chunk saves take it).
fn in_level(rt: &Rt, e: V, dimension: &str) -> bool {
    rt.str_of(rt.get_field(e, "$dimension")).is_none_or(|d| d == dimension)
}

/// The level `dimension`'s mod entities, those not removed, in the order they joined.
fn here(rt: &Rt, dimension: &str) -> Vec<V> {
    crate::mod_entities::live(rt)
        .into_iter()
        .filter(|&e| rt.get_field(e, "removalReason").r() == 0 && in_level(rt, e, dimension))
        .collect()
}

/// The level `dimension`'s mod entity of network id `id`.
fn by_id(rt: &Rt, dimension: &str, id: i32) -> Option<V> {
    here(rt, dimension).into_iter().find(|&e| rt.get_field(e, "$id").i() == id)
}

/// The mod entities of the level `dimension` as a selector matches them, in the order they joined.
pub fn seen(rt: &mut Rt, dimension: &str) -> Vec<Seen> {
    let mut out = Vec::new();
    for e in here(rt, dimension) {
        let kind = crate::mod_entities::type_id(rt, rt.get_field(e, "$type")).unwrap_or_default().to_owned();
        let rotation = [api::virtual_call(rt, GET_Y_ROT, &[e]).f(), api::virtual_call(rt, GET_X_ROT, &[e]).f()];
        let (msb, lsb) = crate::tags::uuid_bits(rt, rt.get_field(e, "uuid"));
        let size = crate::mod_entities::own_size(rt, e);
        let alive = crate::mobs::is_alive(rt, e);
        rt.report_thrown("a command's look at a mod's entity");
        out.push(Seen {
            id: rt.get_field(e, "$id").i(),
            kind,
            position: crate::mod_entities::xyz(rt, e),
            rotation,
            uuid: (u128::from(msb as u64) << 64) | u128::from(lsb as u64),
            size,
            alive,
            custom_name: None,
            tags: Vec::new(),
        });
    }
    out
}

/// Does `what` to the level `dimension`'s mod entity of network id `id`: whether there is one.
/// A teleport is `TeleportCommand.performTeleport`'s within its level: `moveTo` the place and
/// rotation (both angles wrapped to ±180°, the pitch then kept within ±90°), its head turned to
/// the yaw, its fall stopped (`setDeltaMovement` without the vertical, `setOnGround(true)`), and a
/// pathfinding mob's path dropped.
pub fn act(rt: &mut Rt, dimension: &str, id: i32, what: &Act) -> bool {
    let Some(e) = by_id(rt, dimension, id) else {
        return false;
    };
    match *what {
        Act::Kill => {
            api::virtual_call(rt, KILL, &[e]);
        }
        Act::Teleport { position: [x, y, z], rotation } => {
            let [yaw, pitch] = rotation.unwrap_or_else(|| {
                [api::virtual_call(rt, GET_Y_ROT, &[e]).f(), api::virtual_call(rt, GET_X_ROT, &[e]).f()]
            });
            let wrap = crate::command_args::wrap_degrees;
            let (yaw, pitch) = (wrap(yaw), wrap(pitch).clamp(-90.0, 90.0));
            api::virtual_call(rt, MOVE_TO, &[e, V::D(x), V::D(y), V::D(z), V::F(yaw), V::F(pitch)]);
            api::virtual_call(rt, SET_Y_HEAD_ROT, &[e, V::F(yaw)]);
            let motion = api::virtual_call(rt, GET_MOTION, &[e]);
            let (mx, mz) = match motion.r() {
                0 => (0.0, 0.0),
                _ => (rt.get_field(motion, "x").d(), rt.get_field(motion, "z").d()),
            };
            api::virtual_call(rt, SET_MOTION, &[e, V::D(mx), V::D(0.0), V::D(mz)]);
            // Only a living one keeps whether it is on the ground here.
            if crate::mobs::is_mod_living(rt, e) {
                api::virtual_call(rt, SET_ON_GROUND, &[e, V::I(1)]);
            }
            if rt.instance_of_static(e, PATHFINDER_MOB) {
                let nav = api::virtual_call(rt, GET_NAVIGATION, &[e]);
                if nav.r() != 0 {
                    api::virtual_call(rt, STOP, &[nav]);
                }
            }
        }
    }
    rt.report_thrown(match what {
        Act::Kill => "Entity.kill",
        Act::Teleport { .. } => "Entity.teleportTo",
    });
    true
}

/// The level `dimension`'s mod entity of network id `id` as `/data get entity` reads it
/// (`NbtPredicate.getEntityTagForPredicate`: `saveWithoutId`, no `id`); none without one.
pub fn compound(rt: &mut Rt, dimension: &str, id: i32) -> Option<BTreeMap<String, Nbt>> {
    let e = by_id(rt, dimension, id)?;
    let tag = crate::tags::new_compound(rt);
    api::virtual_call(rt, SAVE_WITHOUT_ID, &[e, tag]);
    rt.report_thrown("Entity.saveWithoutId");
    Some(crate::nbt::from_runtime(rt, tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const PENGUIN: &str = "test/Penguin";
    const CART: &str = "test/Cart";
    const ANIMAL_INIT: &str = "net/minecraft/world/entity/animal/Animal.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
    const ENTITY_INIT: &str = "net/minecraft/world/entity/Entity.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";

    /// A runtime with a mod's animal (a penguin, 0.6 by 1 block, no code of its own) and a plain
    /// entity (a cart), their types registered.
    fn runtime() -> (Rt, V, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.supers.insert(PENGUIN, "net/minecraft/world/entity/animal/Animal");
        rt.supers.insert(CART, "net/minecraft/world/entity/Entity");
        let mut types = [V::NULL; 2];
        for (i, (id, width, height)) in [("test:penguin", 0.6, 1.0), ("test:cart", 1.0, 0.5)].into_iter().enumerate() {
            let ty = rt.new_object("net/minecraft/world/entity/EntityType");
            rt.put_field(ty, "$width", V::F(width));
            rt.put_field(ty, "$height", V::F(height));
            rt.registered.insert(("entity_type".into(), id.into()), ty);
            types[i] = ty;
        }
        (rt, types[0], types[1])
    }

    /// One of `class` made by its type and joined to the level at `at`.
    fn joined(rt: &mut Rt, class: &'static str, init: &'static str, ty: V, at: [f64; 3]) -> V {
        let level = crate::glue::level(rt);
        let e = rt.new_object(class);
        api::call(rt, init, &[e, ty, level]);
        crate::mod_entities::set_pos(rt, e, at);
        assert!(crate::mod_entities::join(rt, e, false));
        e
    }

    fn id_of(rt: &Rt, e: V) -> i32 {
        rt.get_field(e, "$id").i()
    }

    /// A selector sees each mod entity of the level with its type, place, size and UUID; `/tp`
    /// moves one there at the rotation given, its pitch kept within 90°, its fall stopped; `/kill`
    /// leaves a mob dying (not alive, still there) and removes a plain entity; `/data get` reads
    /// a mob's compound without an `id`. Another level's commands find none of them.
    #[test]
    fn a_command_sees_moves_kills_and_reads_a_mods_entities() {
        let (mut rt, penguin_type, cart_type) = runtime();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let penguin = joined(rt, PENGUIN, ANIMAL_INIT, penguin_type, [0.5, 1.0, 0.5]);
            let cart = joined(rt, CART, ENTITY_INIT, cart_type, [4.0, 1.0, -2.0]);
            let (p, c) = (id_of(rt, penguin), id_of(rt, cart));

            let seen = seen(rt, "minecraft:overworld");
            assert_eq!(seen.iter().map(|s| (s.id, s.kind.as_str())).collect::<Vec<_>>(), [(p, "test:penguin"), (c, "test:cart")]);
            assert_eq!(seen[0].position, [0.5, 1.0, 0.5]);
            assert_eq!(seen[0].size, [0.6, 1.0, 0.85], "the type's box, its eyes 85% up");
            assert!(seen.iter().all(|s| s.alive));
            let (msb, lsb) = crate::tags::uuid_bits(rt, rt.get_field(penguin, "uuid"));
            assert_eq!(seen[0].uuid, (u128::from(msb as u64) << 64) | u128::from(lsb as u64));
            assert_ne!(seen[0].uuid, seen[1].uuid);
            assert!(super::seen(rt, "minecraft:the_nether").is_empty(), "the overworld's only");

            api::call(rt, "net/minecraft/world/entity/Entity.setDeltaMovement.(DDD)V", &[penguin, V::D(0.2), V::D(-0.5), V::D(0.1)]);
            let to = Act::Teleport { position: [3.5, 2.0, -1.5], rotation: Some([180.0, 120.0]) };
            assert!(act(rt, "minecraft:overworld", p, &to));
            let moved = &super::seen(rt, "minecraft:overworld")[0];
            assert_eq!((moved.position, moved.rotation), ([3.5, 2.0, -1.5], [-180.0, 90.0]), "wrapDegrees, then the pitch clamped");
            assert_eq!(rt.get_field(penguin, "yHeadRot").f(), -180.0, "its head turned too");
            let motion = api::call(rt, GET_MOTION.0, &[penguin]);
            assert_eq!([rt.get_field(motion, "x").d(), rt.get_field(motion, "y").d(), rt.get_field(motion, "z").d()], [0.2, 0.0, 0.1]);
            assert!(!act(rt, "minecraft:the_nether", p, &to), "not in the nether");
            assert!(!act(rt, "minecraft:overworld", c + 100, &Act::Kill), "no such entity");

            let nbt = compound(rt, "minecraft:overworld", p).expect("the penguin's compound");
            assert_eq!(nbt.get("id"), None, "saveWithoutId");
            assert_eq!(nbt.get("Pos"), Some(&Nbt::List(vec![Nbt::Double(3.5), Nbt::Double(2.0), Nbt::Double(-1.5)])));
            assert_eq!(nbt.get("Rotation"), Some(&Nbt::List(vec![Nbt::Float(-180.0), Nbt::Float(90.0)])));
            assert_eq!(nbt.get("Health"), Some(&Nbt::Float(20.0)));

            assert!(act(rt, "minecraft:overworld", p, &Act::Kill));
            assert!(act(rt, "minecraft:overworld", c, &Act::Kill));
            let after = super::seen(rt, "minecraft:overworld");
            assert_eq!(after.len(), 1, "the cart is gone: {after:?}");
            assert_eq!((after[0].id, after[0].alive), (p, false), "the penguin falls over, dead");
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A host that numbers the entities of every runtime it serves from one counter.
    struct Counting {
        next: i32,
    }

    impl crate::host::Host for Counting {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn mod_entity_id(&mut self) -> Option<i32> {
            self.next += 1;
            Some(self.next)
        }
    }

    /// Two mods' runtimes (Create's, Waddles') each counting their entities' ids would both start
    /// at 2^30; the game's one counter gives them different ones.
    #[test]
    fn two_runtimes_entities_take_their_ids_from_the_games_one_counter() {
        let mut ids = Vec::new();
        for _ in 0..2 {
            let (mut rt, penguin_type, _) = runtime();
            let e = rt.with_host(&mut MemoryHost::default(), |rt| joined(rt, PENGUIN, ANIMAL_INIT, penguin_type, [0.5, 1.0, 0.5]));
            ids.push(id_of(&rt, e));
        }
        assert_eq!(ids, [1 << 30, 1 << 30], "each runtime's own count");
        let mut host = Counting { next: (1 << 30) + 6 };
        ids.clear();
        for _ in 0..2 {
            let (mut rt, penguin_type, _) = runtime();
            let e = rt.with_host(&mut host, |rt| joined(rt, PENGUIN, ANIMAL_INIT, penguin_type, [0.5, 1.0, 0.5]));
            ids.push(id_of(&rt, e));
        }
        assert_eq!(ids, [(1 << 30) + 7, (1 << 30) + 8]);
    }
}
