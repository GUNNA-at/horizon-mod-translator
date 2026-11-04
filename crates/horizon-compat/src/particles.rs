//! Particles a mod makes: vanilla's types (`ParticleTypes.SMOKE`), the option classes that carry a
//! block, an item or a dust's colour (`BlockParticleOption`, `ItemParticleOption`,
//! `DustParticleOptions`), a mod's own types by the id they are registered under, and a level's
//! `addParticle` family and `ServerLevel.sendParticles`, which hand them to the game
//! ([`crate::host::Host::particles`]).
//!
//! As in vanilla, a client's level draws `addParticle` and a server's sends `sendParticles` to the
//! players near it; `addParticle` on a server's level and `sendParticles`' absence on a client's
//! make the other two nothing.
use crate::host::Particles;
use crate::{Rt, V};

const PARTICLE_TYPES: &str = "net/minecraft/core/particles/ParticleTypes";
const PARTICLE_TYPE: &str = "net/minecraft/core/particles/ParticleType";
const SIMPLE_TYPE: &str = "net/minecraft/core/particles/SimpleParticleType";
const BLOCK_OPTION: &str = "net/minecraft/core/particles/BlockParticleOption";
const ITEM_OPTION: &str = "net/minecraft/core/particles/ItemParticleOption";
const DUST_OPTIONS: &str = "net/minecraft/core/particles/DustParticleOptions";

/// Vanilla's particle type with id `id` (`minecraft:smoke`): one object per type.
fn vanilla_type(rt: &mut Rt, id: &str) -> V {
    let id = id.to_owned();
    rt.intern(format!("particle-type/{id}"), move |rt| {
        let t = rt.new_object(SIMPLE_TYPE);
        let s = rt.string(&id);
        rt.put_field(t, "$id", s);
        t
    })
}

/// `ParticleTypes.FIELD` (the field's name in lower case is its type's id, but for the trial
/// spawner's two, as 1.21.1's `ParticleTypes` registers them), and `DustParticleOptions.REDSTONE`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        PARTICLE_TYPES if field != "CODEC" && field != "STREAM_CODEC" => {
            let path = match field {
                "TRIAL_SPAWNER_DETECTED_PLAYER" => "trial_spawner_detection".to_owned(),
                "TRIAL_SPAWNER_DETECTED_PLAYER_OMINOUS" => "trial_spawner_detection_ominous".to_owned(),
                f => f.to_ascii_lowercase(),
            };
            Some(vanilla_type(rt, &format!("minecraft:{path}")))
        }
        DUST_OPTIONS if field == "REDSTONE" => Some(rt.intern("particle/redstone-dust".into(), |rt| dust(rt, [1.0, 0.0, 0.0], 1.0))),
        _ => None,
    }
}

fn dust(rt: &mut Rt, rgb: [f32; 3], scale: f32) -> V {
    let d = rt.new_object(DUST_OPTIONS);
    let ty = vanilla_type(rt, "minecraft:dust");
    rt.put_field(d, "$type", ty);
    for (f, c) in ["$r", "$g", "$b"].into_iter().zip(rgb) {
        rt.put_field(d, f, V::F(c));
    }
    rt.put_field(d, "$scale", V::F(scale));
    d
}

/// A type's id: vanilla's, or a mod's by the name it is registered under.
fn type_id(rt: &Rt, ty: V) -> Option<String> {
    rt.str_of(rt.get_field(ty, "$id")).map(str::to_owned).or_else(|| rt.registered_name(ty, "particle_type").map(str::to_owned))
}

/// What `options` (a type, or options of one) draws, at `at`.
fn particles(rt: &Rt, options: V) -> Option<Particles> {
    let ty = match rt.get_field(options, "$type") {
        t if t.r() != 0 => t,
        _ => options,
    };
    let mut p = Particles { kind: type_id(rt, ty)?, ..Particles::default() };
    match rt.class_of(options) {
        BLOCK_OPTION => p.block = Some(rt.str_of(rt.get_field(options, "$state"))?.to_owned()),
        ITEM_OPTION => p.item = Some(crate::api_common::item_like_name(rt, rt.get_field(rt.get_field(options, "$stack"), "$item"))),
        DUST_OPTIONS => p.dust = Some((["$r", "$g", "$b"].map(|f| rt.get_field(options, f).f()), rt.get_field(options, "$scale").f())),
        _ => {}
    }
    Some(p)
}

/// A level's `addParticle` and `addAlwaysVisibleParticle` (`name`, `desc`; the level first in
/// `args`): drawn by a client, nothing on a server.
pub fn level_particle(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if !crate::neo::is_client(rt) {
        return V::Void;
    }
    // `(options, [force,] x, y, z, dx, dy, dz)`.
    let force = desc.starts_with("(Lnet/minecraft/core/particles/ParticleOptions;Z");
    let at = if force { 3 } else { 2 };
    let Some(mut p) = particles(rt, a(1)) else { return V::Void };
    p.at = [a(at).d(), a(at + 1).d(), a(at + 2).d()];
    p.delta = [a(at + 3).d(), a(at + 4).d(), a(at + 5).d()];
    p.force = force && a(2).i() != 0;
    p.always = name == "addAlwaysVisibleParticle";
    if let Some(h) = rt.host_mut() {
        h.particles(&p);
    }
    V::Void
}

/// `ServerLevel.sendParticles(options, x, y, z, count, dx, dy, dz, speed)` (and to one player,
/// `(player, options, force, x, y, z, count, dx, dy, dz, speed)`): sent to the players near them by a
/// server; how many players the server had near (vanilla's count of those sent to).
pub fn send_particles(rt: &mut Rt, desc: &str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let to_one = desc.starts_with("(Lnet/minecraft/server/level/ServerPlayer;");
    let (o, at) = if to_one { (2, 4) } else { (1, 2) };
    let Some(mut p) = particles(rt, a(o)) else { return V::I(0) };
    p.at = [a(at).d(), a(at + 1).d(), a(at + 2).d()];
    p.count = Some(a(at + 3).i());
    p.delta = [a(at + 4).d(), a(at + 5).d(), a(at + 6).d()];
    p.speed = a(at + 7).d();
    p.force = to_one && a(3).i() != 0;
    let Some(h) = rt.host_mut() else { return V::I(0) };
    let near = h.players().iter().filter(|(_, q)| (0..3).map(|i| (q[i] - p.at[i]).powi(2)).sum::<f64>() < 32.0 * 32.0).count();
    h.particles(&p);
    if to_one { V::I((near > 0) as i32) } else { V::I(near as i32) }
}

/// The option classes' own calls; None for anything else.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // `new BlockParticleOption(type, state)`, `new ItemParticleOption(type, stack|item)`.
        (BLOCK_OPTION, "<init>") => {
            rt.put_field(a(0), "$type", a(1));
            rt.put_field(a(0), "$state", a(2));
            V::Void
        }
        (ITEM_OPTION, "<init>") => {
            let stack = if desc.contains("Lnet/minecraft/world/item/ItemStack;") {
                a(2)
            } else {
                let item = crate::api_common::item_like_name(rt, a(2));
                crate::api_common::stack(rt, &item, 1)
            };
            rt.put_field(a(0), "$type", a(1));
            rt.put_field(a(0), "$stack", stack);
            V::Void
        }
        // `new DustParticleOptions(color, scale)`, the colour a `Vector3f`.
        (DUST_OPTIONS, "<init>") => {
            let ty = vanilla_type(rt, "minecraft:dust");
            rt.put_field(a(0), "$type", ty);
            for (f, c) in ["$r", "$g", "$b"].into_iter().zip(["x", "y", "z"]) {
                let v = rt.get_field(a(1), c);
                rt.put_field(a(0), f, V::F(v.f()));
            }
            rt.put_field(a(0), "$scale", V::F(a(2).f()));
            V::Void
        }
        (BLOCK_OPTION, "getState") => rt.get_field(a(0), "$state"),
        (ITEM_OPTION, "getItem") => rt.get_field(a(0), "$stack"),
        (DUST_OPTIONS, "getScale") => rt.get_field(a(0), "$scale"),
        // A type is its own options (`SimpleParticleType`); options give their type.
        (BLOCK_OPTION | ITEM_OPTION | DUST_OPTIONS | SIMPLE_TYPE | PARTICLE_TYPE | "net/minecraft/core/particles/ParticleOptions", "getType") => match rt.get_field(a(0), "$type") {
            t if t.r() != 0 => t,
            _ => a(0),
        },
        // `new SimpleParticleType(overrideLimiter)`, or a mod's own `ParticleType` subclass calling
        // `super(overrideLimiter)`: named once registered.
        (SIMPLE_TYPE | PARTICLE_TYPE, "<init>") => {
            rt.put_field(a(0), "$overrideLimiter", a(1));
            V::Void
        }
        (SIMPLE_TYPE | PARTICLE_TYPE, "getOverrideLimiter") => V::I(rt.get_field(a(0), "$overrideLimiter").i()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// A client draws a smoke puff, an iron sheet's item particles and a block's; a server's level
    /// draws none.
    #[test]
    fn a_client_draws_a_mods_particles_and_a_server_draws_none() {
        let mut rt = Rt::new();
        crate::install();
        crate::neo::fire_client_setup(&mut rt);
        let mut host = MemoryHost { client: true, ..MemoryHost::default() };
        let smoke = crate::api::get_static(&mut rt, "net/minecraft/core/particles/ParticleTypes.LARGE_SMOKE");
        let item_type = crate::api::get_static(&mut rt, "net/minecraft/core/particles/ParticleTypes.ITEM");
        let block_type = crate::api::get_static(&mut rt, "net/minecraft/core/particles/ParticleTypes.BLOCK");
        let sheet = crate::api_common::stack(&mut rt, "create:iron_sheet", 1);
        let item = rt.new_object(ITEM_OPTION);
        crate::api::call(&mut rt, "net/minecraft/core/particles/ItemParticleOption.<init>.(Lnet/minecraft/core/particles/ParticleType;Lnet/minecraft/world/item/ItemStack;)V", &[item, item_type, sheet]);
        let stone = crate::api::state(&mut rt, "minecraft:stone");
        let block = rt.new_object(BLOCK_OPTION);
        crate::api::call(&mut rt, "net/minecraft/core/particles/BlockParticleOption.<init>.(Lnet/minecraft/core/particles/ParticleType;Lnet/minecraft/world/level/block/state/BlockState;)V", &[block, block_type, stone]);
        let add = "net/minecraft/world/level/Level.addParticle.(Lnet/minecraft/core/particles/ParticleOptions;DDDDDD)V";
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let d = |v: f64| V::D(v);
            crate::api::call(rt, add, &[level, smoke, d(1.0), d(2.0), d(3.0), d(0.0), d(0.05), d(0.0)]);
            crate::api::call(rt, add, &[level, item, d(0.5), d(65.0), d(0.5), d(0.1), d(0.2), d(0.3)]);
            crate::api::call(rt, "net/minecraft/world/level/Level.addAlwaysVisibleParticle.(Lnet/minecraft/core/particles/ParticleOptions;ZDDDDDD)V", &[level, block, V::I(1), d(4.0), d(5.0), d(6.0), d(0.0), d(0.0), d(0.0)]);
        });
        let kinds: Vec<_> = host.particles.iter().map(|p| (p.kind.as_str(), p.item.as_deref(), p.block.as_deref(), p.at, p.delta, p.force, p.always)).collect();
        assert_eq!(
            kinds,
            [
                ("minecraft:large_smoke", None, None, [1.0, 2.0, 3.0], [0.0, 0.05, 0.0], false, false),
                ("minecraft:item", Some("create:iron_sheet"), None, [0.5, 65.0, 0.5], [0.1, 0.2, 0.3], false, false),
                ("minecraft:block", None, Some("minecraft:stone"), [4.0, 5.0, 6.0], [0.0; 3], true, true),
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);

        // A server's level: `addParticle` draws nothing, `sendParticles` sends a dust cloud.
        let mut rt = Rt::new();
        let mut server = MemoryHost { players: vec![(1, [0.0, 64.0, 0.0]), (2, [100.0, 64.0, 0.0])], ..MemoryHost::default() };
        let smoke = crate::api::get_static(&mut rt, "net/minecraft/core/particles/ParticleTypes.SMOKE");
        let redstone = crate::api::get_static(&mut rt, "net/minecraft/core/particles/DustParticleOptions.REDSTONE");
        let sent = rt.with_host(&mut server, |rt| {
            let level = crate::glue::level(rt);
            let d = |v: f64| V::D(v);
            crate::api::call(rt, add, &[level, smoke, d(1.0), d(2.0), d(3.0), d(0.0), d(0.0), d(0.0)]);
            crate::api::call(rt, "net/minecraft/server/level/ServerLevel.sendParticles.(Lnet/minecraft/core/particles/ParticleOptions;DDDIDDDD)I", &[level, redstone, d(0.5), d(65.0), d(0.5), V::I(8), d(0.25), d(0.5), d(0.25), d(0.0)])
        });
        assert_eq!(sent, V::I(1), "one player within 32 blocks");
        assert_eq!(server.particles.len(), 1);
        let p = &server.particles[0];
        assert_eq!((p.kind.as_str(), p.dust, p.count, p.delta), ("minecraft:dust", Some(([1.0, 0.0, 0.0], 1.0)), Some(8), [0.25, 0.5, 0.25]));
    }
}
