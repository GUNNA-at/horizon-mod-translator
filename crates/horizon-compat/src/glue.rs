//! From Horizon's native hooks to the mod's own overrides. Each function is
//! called by the generated native mod at one hook point; it finds the mod
//! object behind the block or item and runs the translated Java method.
//! They return whether the mod handled the event (the hook's `handled` /
//! `claimed`), so vanilla's own behaviour runs otherwise.
use crate::{api, host::Host, Rt, V};

const TICK: &str = "tick.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)V";
const RANDOM_TICK: &str = "randomTick.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)V";
const NEIGHBOR: &str = "neighborChanged.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;Lnet/minecraft/core/BlockPos;Z)V";

/// The mod's Block object behind a state, and its class, when the mod registered it.
fn mod_block(rt: &Rt, state: &str) -> Option<(V, &'static str)> {
    let (name, _) = crate::host::split_state(state);
    let b = *rt.registered.get(&("block".to_owned(), name.to_owned()))?;
    let class = rt.class_of(b);
    (class != "net/minecraft/world/level/block/Block").then_some((b, class))
}

/// Runs `key` on the block's object with the game as host. Returns whether it ran.
fn run(rt: &mut Rt, host: &mut dyn Host, state: &str, key: &'static str, extra: impl FnOnce(&mut Rt) -> Vec<V>) -> bool {
    let Some((b, class)) = mod_block(rt, state) else { return false };
    if !rt.overrides.is_some_and(|o| o(class, key)) {
        return false;
    }
    rt.with_host(host, |rt| {
        let st = api::state(rt, state);
        let mut args = vec![b, st];
        args.extend(extra(rt));
        if let Some(vc) = rt.vcall {
            vc(rt, key, key, &args);
        }
        rt.thrown = None; // an exception escaping a hook ends that call only, as the game logs and goes on
    });
    true
}

fn level(rt: &mut Rt) -> V {
    rt.new_object("net/minecraft/server/level/ServerLevel")
}

/// `tick_block` (a scheduled tick): `BlockBehaviour.tick`.
pub fn tick_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    run(rt, host, state, TICK, |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let r = rt.new_object("net/minecraft/util/RandomSource");
        vec![l, p, r]
    })
}

/// `random_tick`: `BlockBehaviour.randomTick`.
pub fn random_tick(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    run(rt, host, state, RANDOM_TICK, |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let r = rt.new_object("net/minecraft/util/RandomSource");
        vec![l, p, r]
    })
}

/// `neighbor_changed`: `BlockBehaviour.neighborChanged`.
pub fn neighbor_changed(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, from: [i32; 3], changed_block: &str, moved: bool) -> bool {
    let changed = changed_block.to_owned();
    run(rt, host, state, NEIGHBOR, move |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let b = api::block(rt, &changed);
        let f = api::block_pos(rt, from[0] as i64, from[1] as i64, from[2] as i64);
        vec![l, p, b, f, V::I(moved as i32)]
    })
}
