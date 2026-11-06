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
pub(crate) fn mod_block(rt: &Rt, state: &str) -> Option<(V, &'static str)> {
    let (name, _) = crate::host::split_state(state);
    let b = rt.registered.get(&("block".to_owned(), name.to_owned())).copied().or_else(|| api::unregistered_block(rt, name))?;
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
        rt.report_thrown(key); // an exception escaping a hook ends that call only, as the game logs and goes on
    });
    true
}

/// The level translated code sees: one object (so `level == level` holds), the authoritative
/// server's (`isClientSide` is a field mods read).
pub fn level(rt: &mut Rt) -> V {
    if let Some(l) = rt.interned.get("compat:level") {
        return *l;
    }
    // The client's runtime (the render thread's) draws a client level.
    let client = crate::neo::is_client(rt);
    let l = rt.new_object(if client { "net/minecraft/client/multiplayer/ClientLevel" } else { "net/minecraft/server/level/ServerLevel" });
    rt.put_field(l, "isClientSide", V::I(client as i32));
    rt.interned.insert("compat:level".into(), l);
    // Loading it, mods hear `LevelEvent.Load` (Create sets up its kinetic networks there).
    let ev = rt.new_object(LEVEL_LOAD);
    rt.put_field(ev, "$level", l);
    crate::neo::post(rt, LEVEL_LOAD, ev);
    l
}

const LEVEL_LOAD: &str = "net/neoforged/neoforge/event/level/LevelEvent$Load";

/// `tick_block` (a scheduled tick): `BlockBehaviour.tick`.
pub fn tick_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    run(rt, host, state, TICK, |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let r = rt.new_object("net/minecraft/util/RandomSource");
        vec![l, p, r]
    })
}

/// `random_tick`: `BlockBehaviour.randomTick`. A mod block's random tick is always the mod's,
/// even when it has none: the block's template (which gives it random ticks) must not act. A
/// block without one of its own ticks as its vanilla class does where the runtime has that
/// class's: a crop grows ([`crate::crops`]).
pub fn random_tick(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let Some((b, class)) = mod_block(rt, state) else { return false };
    let args = |rt: &mut Rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let r = rt.new_object("net/minecraft/util/RandomSource");
        vec![l, p, r]
    };
    if !rt.overrides.is_some_and(|o| o(class, RANDOM_TICK)) && crate::crops::grows_as_crop(rt, class) {
        rt.with_host(host, |rt| {
            let st = api::state(rt, state);
            let a = args(rt);
            crate::crops::random_tick(rt, b, st, a[0], a[1], a[2]);
            rt.report_thrown(RANDOM_TICK);
        });
        return true;
    }
    run(rt, host, state, RANDOM_TICK, args);
    true
}

const VALID_BONEMEAL: &str = "isValidBonemealTarget.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z";
const BONEMEAL_SUCCESS: &str = "isBonemealSuccess.(Lnet/minecraft/world/level/Level;Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z";
const PERFORM_BONEMEAL: &str = "performBonemeal.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V";

/// The key a bone meal call on the mod block of class `class` falls back to where the class has no
/// method of its own: its crop base's ([`crate::crops`]); None for a block whose code takes no bone
/// meal.
fn bonemeal_key(rt: &mut Rt, class: &'static str, short: &'static str) -> Option<&'static str> {
    if crate::crops::grows_as_crop(rt, class) {
        return Some(rt.static_key(&format!("net/minecraft/world/level/block/CropBlock.{short}")));
    }
    rt.overrides.is_some_and(|o| o(class, short)).then(|| rt.static_key(&format!("net/minecraft/world/level/block/BonemealableBlock.{short}")))
}

/// Runs the bone meal method `short` of the mod block `state` at `pos` (its own, or its crop
/// base's), with the level, a random source where `random` is set, the position and the state.
/// None where the block takes no bone meal by its code, or its code threw.
fn bonemeal(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, short: &'static str, random: bool) -> Option<V> {
    let (b, class) = mod_block(rt, state)?;
    let key = bonemeal_key(rt, class, short)?;
    let vc = rt.vcall?;
    rt.with_host(host, |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
        let st = api::state(rt, state);
        let mut args = vec![b, l];
        if random {
            args.push(rt.new_object("net/minecraft/util/RandomSource"));
        }
        args.extend([p, st]);
        let r = vc(rt, key, short, &args);
        if rt.thrown.is_some() {
            rt.report_thrown(short);
            return None;
        }
        Some(r)
    })
}

/// `native_is_valid_bonemeal_target`: whether bone meal works on the mod block `state` at `pos`
/// (`BonemealableBlock.isValidBonemealTarget`): its own code's answer, or its crop base's (short of
/// its last age). None for a block that takes no bone meal by its code.
pub fn is_valid_bonemeal_target(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> Option<bool> {
    bonemeal(rt, host, pos, state, VALID_BONEMEAL, false).map(|v| v.i() != 0)
}

/// `native_is_bonemeal_success`: whether this use of bone meal grows the block
/// (`isBonemealSuccess`, which a sapling, say, draws for; always for a crop).
pub fn is_bonemeal_success(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    bonemeal(rt, host, pos, state, BONEMEAL_SUCCESS, true).is_some_and(|v| v.i() != 0)
}

/// `native_perform_bonemeal`: the block grows (`performBonemeal`: a crop by 2 to 5 ages, up to its
/// last).
pub fn perform_bonemeal(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) {
    rt.safe_point();
    bonemeal(rt, host, pos, state, PERFORM_BONEMEAL, true);
}

/// `neighbor_changed`: `BlockBehaviour.neighborChanged`.
pub fn neighbor_changed(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, from: [i32; 3], changed_block: &str, moved: bool) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let changed = changed_block.to_owned();
    run(rt, host, state, NEIGHBOR, move |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let b = api::block(rt, &changed);
        let f = api::block_pos(rt, from[0] as i64, from[1] as i64, from[2] as i64);
        vec![l, p, b, f, V::I(moved as i32)]
    })
}

const NEW_BLOCK_ENTITY: &str = "newBlockEntity.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
const GET_TICKER: &str = "getTicker.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/entity/BlockEntityType;)Lnet/minecraft/world/level/block/entity/BlockEntityTicker;";
const TICKER_TICK: &str = "net/minecraft/world/level/block/entity/BlockEntityTicker.tick.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/entity/BlockEntity;)V";
const LOAD: &str = "loadAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";
const SAVE: &str = "saveAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";

fn implements(rt: &Rt, block: &str, key: &str) -> bool {
    mod_block(rt, block).is_some_and(|(_, class)| rt.overrides.is_some_and(|o| o(class, key)))
}

/// `native_block_entity`: the id of the block's block entity, when the mod's block makes one.
pub fn block_entity_id(rt: &Rt, block: &str) -> Option<String> {
    implements(rt, block, NEW_BLOCK_ENTITY).then(|| block.to_owned())
}

/// `native_block_entity_ticks`: whether the mod's block gives its block entity a ticker.
pub fn block_entity_ticks(rt: &Rt, block: &str) -> bool {
    implements(rt, block, GET_TICKER)
}

/// The mod's block entity object at `pos`, made with `newBlockEntity` and loaded from `nbt` the first time.
fn block_entity(rt: &mut Rt, pos: [i32; 3], state: &str, nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) -> Option<V> {
    let (block, _) = crate::host::split_state(state);
    if let Some(be) = rt.block_entities.get(&pos).copied() {
        // The same block entity, unless the block changed or the game hands a fresh one (an empty
        // compound where the live one had saved data): then it is made again.
        let fresh = rt.block_entity_meta.get(&pos).is_some_and(|(b, saved)| b != block || (*saved && nbt.is_empty()));
        if !fresh {
            // Made when something asked for it, before the game first ticked it: the game's saved
            // data goes in now, as vanilla's chunk load would have put it in.
            if rt.get_field(be, "$demand").i() != 0 {
                rt.put_field(be, "$demand", V::I(0));
                load_saved(rt, be, nbt);
            } else if let V::J(written) = rt.get_field(be, "$written")
                && written != crate::nbt::data_hash(nbt) as i64
            {
                // The game changed its data since it last saved (`/data merge block`): it loads
                // that and players are sent it, as vanilla's `BlockDataAccessor.setData` does
                // (`sendBlockUpdated`).
                load_saved(rt, be, nbt);
                if let Some(h) = rt.host_mut() {
                    h.block_changed(pos);
                }
            }
            return Some(be);
        }
        rt.block_entities.remove(&pos);
    }
    let (b, _) = mod_block(rt, state)?;
    let vc = rt.vcall?;
    let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
    let st = api::state(rt, state);
    let be = vc(rt, NEW_BLOCK_ENTITY, NEW_BLOCK_ENTITY, &[b, p, st]);
    rt.report_thrown("newBlockEntity");
    if be.r() == 0 {
        return None;
    }
    load_saved(rt, be, nbt);
    rt.block_entities.insert(pos, be);
    Some(be)
}

/// Loads the game's saved data into block entity `be`. Only saved data, as vanilla's: a block
/// entity just placed keeps what its constructor set (an empty compound would read every field as
/// absent: Create's creative motor would lose its default speed). What it kept of the stack it was
/// placed from comes back from `components` (`BlockEntity.loadWithComponents`).
fn load_saved(rt: &mut Rt, be: V, nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) {
    let Some(vc) = rt.vcall else { return };
    if !nbt.is_empty() {
        let tag = crate::nbt::to_runtime(rt, nbt);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        vc(rt, LOAD, LOAD, &[be, tag, provider]);
        rt.report_thrown("loadAdditional");
    }
    if let Some(crate::nbt::Nbt::Compound(kept)) = nbt.get(KEPT) {
        let kept: Vec<(String, crate::nbt::Nbt)> = kept.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        crate::components::load_kept(rt, be, &kept);
    }
}

/// The key a block entity's kept components are saved under (`BlockEntity.ComponentHelper`).
const KEPT: &str = "components";

/// The block entity at `pos` as `Level.getBlockEntity` finds it: the one this runtime holds, or,
/// for a mod block that has one, made now. Vanilla makes it with its block; here one is otherwise
/// made on its first tick or draw, and Create's belt asks for its segments' before they tick (and
/// breaks itself when one is missing). It takes the game's saved data on its first tick.
pub(crate) fn block_entity_at(rt: &mut Rt, pos: [i32; 3]) -> V {
    let state = rt.host().map(|h| h.block_state(pos));
    if let Some(be) = rt.block_entities.get(&pos).copied() {
        // Only while its block is there: a block replaced since (Create's belt connector breaks
        // the shafts it lays its ends on, then sets the belt there) took its block entity with it,
        // as vanilla's chunk removes it when the block changes.
        let block = state.as_deref().map(|s| crate::host::split_state(s).0);
        let gone = matches!((rt.block_entity_meta.get(&pos), block), (Some((b, _)), Some(now)) if b != now);
        if !gone {
            return be;
        }
        // The old block's `onRemove` runs with its block entity still there, as in vanilla's chunk.
        if let Some(&kept) = rt.interned.get(&removing_key(pos)) {
            return kept;
        }
        block_entity_removed(rt, pos);
    }
    let Some(state) = state else { return V::NULL };
    if !implements(rt, &state, NEW_BLOCK_ENTITY) {
        return V::NULL;
    }
    let (Some((b, _)), Some(vc)) = (mod_block(rt, &state), rt.vcall) else { return V::NULL };
    let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
    let st = api::state(rt, &state);
    let be = vc(rt, NEW_BLOCK_ENTITY, NEW_BLOCK_ENTITY, &[b, p, st]);
    rt.report_thrown("newBlockEntity");
    if be.r() == 0 {
        return V::NULL;
    }
    let l = level(rt);
    rt.put_field(be, "level", l);
    rt.put_field(be, "$demand", V::I(1));
    rt.block_entities.insert(pos, be);
    let (block, _) = crate::host::split_state(&state);
    rt.block_entity_meta.insert(pos, (block.to_owned(), false));
    be
}

/// `native_block_entity_tick`: one server tick of the mod's block entity at `pos`, whose saved
/// compound is `nbt` (read before, written after).
pub fn block_entity_tick(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, nbt: &mut std::collections::BTreeMap<String, crate::nbt::Nbt>) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let Some((b, _)) = mod_block(rt, state) else { return false };
    rt.with_host(host, |rt| {
        let Some(be) = block_entity(rt, pos, state, nbt) else { return false };
        let Some(vc) = rt.vcall else { return false };
        let level = level(rt);
        let st = api::state(rt, state);
        // In its level, with the block's current state, as the game's block entity is.
        if rt.get_field(be, "level").r() == 0 {
            rt.put_field(be, "level", level);
        }
        rt.put_field(be, "$state", st);
        rt.put_field(be, "blockState", st);
        let ty = rt.get_field(be, "$type");
        let ticker = vc(rt, GET_TICKER, GET_TICKER, &[b, level, st, ty]);
        if ticker.r() != 0 {
            let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
            rt.call_functional(TICKER_TICK, ticker, &[level, p, st, be]);
        }
        rt.report_thrown("the block entity's tick");
        // Item entities on or in the block, as their moves report them to it.
        if rt.host().is_some_and(|h| h.is_server()) {
            crate::entities::item_contacts(rt, pos, b, st);
        }
        let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
        api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        vc(rt, SAVE, SAVE, &[be, tag, provider]);
        rt.report_thrown("saveAdditional");
        *nbt = crate::nbt::from_runtime(rt, tag);
        // `saveWithoutMetadata`: what it kept of its stack goes beside its own data.
        let kept = crate::components::kept_saved(rt, be);
        if !kept.is_empty() {
            nbt.insert(KEPT.to_owned(), crate::nbt::Nbt::Compound(kept.into_iter().collect()));
        }
        rt.put_field(be, "$written", V::J(crate::nbt::data_hash(nbt) as i64));
        let (block, _) = crate::host::split_state(state);
        rt.block_entity_meta.insert(pos, (block.to_owned(), !nbt.is_empty()));
        true
    })
}

/// The block entity at `pos` was removed (its block broken or replaced).
pub fn block_entity_removed(rt: &mut Rt, pos: [i32; 3]) {
    rt.block_entities.remove(&pos);
    rt.block_entity_meta.remove(&pos);
}

/// `LevelChunk.removeBlockEntity(pos)`: the block entity the runtime holds there is let go, then
/// told so (`setRemoved`: Create's kinetic ones leave their network, its others let their
/// behaviours go).
pub(crate) fn remove_block_entity(rt: &mut Rt, pos: [i32; 3]) {
    let mark = removing_key(pos);
    let be = rt.interned.remove(&mark).or_else(|| rt.block_entities.get(&pos).copied());
    block_entity_removed(rt, pos);
    if let (Some(be), Some(vc)) = (be.filter(|b| b.r() != 0), rt.vcall) {
        vc(rt, SET_REMOVED_KEY, SET_REMOVED, &[be]);
        rt.report_thrown(SET_REMOVED);
    }
}

const SET_REMOVED: &str = "setRemoved.()V";
const SET_REMOVED_KEY: &str = "net/minecraft/world/level/block/entity/BlockEntity.setRemoved.()V";
const ON_REMOVE: &str = "onRemove.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V";
const ON_REMOVE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.onRemove.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V";
const ON_PLACE: &str = "onPlace.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V";
const ON_PLACE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.onPlace.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V";

/// Where the block entity an old block's `onRemove` still sees at `pos` is kept while it runs.
fn removing_key(pos: [i32; 3]) -> String {
    format!("compat:removing/{},{},{}", pos[0], pos[1], pos[2])
}

thread_local! {
    /// Mod blocks the game replaced while the mod's own code ran (the change was its own, or came of
    /// it): their `onRemove`, once that code is back in the runtime ([`drain_replaced`]).
    static DEFERRED: std::cell::RefCell<Vec<([i32; 3], String, String, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `native_block_replaced`: the state `old` at `pos` was replaced by `new` on the server, one of them
/// a mod's (any change of state; `moved` by a piston), as 1.21.1's `LevelChunk.setBlockState` tells
/// the blocks: the old one's `onRemove` (a Create machine drops what it held, a tank splits), with
/// the block entity it had still at `pos` while it runs, then the new one's `onPlace`.
pub fn block_replaced(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], old: &str, new: &str, moved: bool) {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    rt.with_host(host, |rt| {
        drain_replaced(rt);
        replaced(rt, pos, old, new, moved);
    });
}

/// [`block_replaced`] while the runtime is inside a call of the mod's own: kept until the change
/// is back in the runtime ([`drain_replaced`]).
pub fn defer_replaced(pos: [i32; 3], old: &str, new: &str, moved: bool) {
    DEFERRED.with(|d| d.borrow_mut().push((pos, old.to_owned(), new.to_owned(), moved)));
}

/// The replaced blocks [`defer_replaced`] kept, oldest first: each one's `onRemove`, the runtime's
/// host being the game's. Runs after each of the mod's own block writes, so a block it replaced
/// hears of it before the write returns, as in vanilla.
pub(crate) fn drain_replaced(rt: &mut Rt) {
    while let Some((pos, old, new, moved)) = DEFERRED.with(|d| {
        let mut d = d.borrow_mut();
        (!d.is_empty()).then(|| d.remove(0))
    }) {
        replaced(rt, pos, &old, &new, moved);
    }
}

/// A change of state at `pos` as 1.21.1's `LevelChunk.setBlockState` tells the blocks, each a mod's:
/// the old state's `onRemove`, then the new one's `onPlace`.
fn replaced(rt: &mut Rt, pos: [i32; 3], old: &str, new: &str, moved: bool) {
    if let Some((b, class)) = mod_block(rt, old) {
        removed(rt, b, class, pos, old, new, moved);
    }
    if let Some((b, class)) = mod_block(rt, new) {
        placed(rt, b, class, pos, old, new, moved);
    }
}

/// `oldState.onRemove(level, pos, newState, moved)` for a mod block `b`: its override (`super.`
/// reaches vanilla's: the block entity goes with its block), else vanilla's. The runtime's block
/// entity at `pos` stays the old block's while it runs; one its code left there when the block
/// changed is let go, as the chunk drops it.
fn removed(rt: &mut Rt, b: V, class: &'static str, pos: [i32; 3], old: &str, new: &str, moved: bool) {
    let (old_block, _) = crate::host::split_state(old);
    let changed = old_block != crate::host::split_state(new).0;
    let kept = rt.block_entities.get(&pos).copied().filter(|_| rt.block_entity_meta.get(&pos).is_some_and(|(held, _)| held == old_block));
    let mark = removing_key(pos);
    if let Some(be) = kept {
        rt.interned.insert(mark.clone(), be);
    }
    let st = api::state(rt, old);
    let to = api::state(rt, new);
    let l = level(rt);
    let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
    match rt.vcall.filter(|_| rt.overrides.is_some_and(|o| o(class, ON_REMOVE))) {
        Some(vc) => {
            vc(rt, ON_REMOVE_KEY, ON_REMOVE, &[b, st, l, p, to, V::I(moved as i32)]);
            rt.report_thrown(ON_REMOVE);
        }
        None => crate::block_breaking::on_remove(rt, st, l, p, to),
    }
    rt.interned.remove(&mark);
    if changed && kept.is_some() && rt.block_entities.get(&pos).copied() == kept {
        block_entity_removed(rt, pos);
    }
    // Gone with its block: kept for the drops the game rolls next (`Block.playerDestroy` gets the
    // block entity the block had).
    if let Some(be) = kept.filter(|_| changed) {
        let key = broken_key(pos);
        if rt.interned.insert(key.clone(), be).is_none() {
            BROKEN.with(|b| b.borrow_mut().push(key));
        }
    }
}

/// Where the block entity a broken block had is kept until its drops are rolled ([`loot_drops`]).
fn broken_key(pos: [i32; 3]) -> String {
    format!("compat:broken/{},{},{}", pos[0], pos[1], pos[2])
}

thread_local! {
    /// The [`broken_key`]s kept, let go at the next server tick whether or not drops came for them
    /// (a `/setblock` drops nothing).
    static BROKEN: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Lets go of the block entities kept for drops that never came.
fn forget_broken(rt: &mut Rt) {
    for key in BROKEN.with(|b| std::mem::take(&mut *b.borrow_mut())) {
        rt.interned.remove(&key);
    }
}

/// What [`loot_drops`] gives for each stack: the index of the game's own stack it left as it was
/// (the game keeps that one), else the stack.
pub type LootDrop = (Option<usize>, crate::host::HandStack);

/// `native_loot_drops` (server): the game rolled `rolled` from the loot table of the block `state`
/// broken at `pos`, by the acting player (`player`) with `tool`, or by an explosion of radius
/// `explosion`. A mod block with its own `getDrops(state, LootParams.Builder)` answers for its
/// drops, with the parameters `Block.getDrops` gives it: the block's centre, the tool, the player
/// and the block entity it had (the one its `onRemove` just let go, else the one still there), and
/// the explosion's radius. Its `super.` call gets the game's stacks back. None: the game's drops
/// stand.
#[allow(clippy::too_many_arguments)]
pub fn loot_drops(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    rolled: &[crate::host::HandStack],
    tool: Option<crate::host::HandStack>,
    player: Option<u128>,
    explosion: Option<f32>,
) -> Option<Vec<LootDrop>> {
    let (b, class) = mod_block(rt, state)?;
    if !rt.overrides.is_some_and(|o| o(class, crate::loot_params::DROPS)) {
        return None;
    }
    let vc = rt.vcall?;
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    rt.with_host(host, |rt| {
        let (block, _) = crate::host::split_state(state);
        let live = rt.block_entities.get(&pos).copied().filter(|_| rt.block_entity_meta.get(&pos).is_some_and(|(held, _)| held == block));
        let be = rt.interned.remove(&broken_key(pos)).or(live).unwrap_or(V::NULL);
        let level = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let tool = crate::hand::load(rt, tool);
        let entity = if player.is_some() { crate::api_common::acting_player(rt) } else { V::NULL };
        let builder = crate::loot_params::block_builder(rt, level, p, tool, entity, be);
        if let Some(radius) = explosion {
            crate::loot_params::with_explosion(rt, builder, radius);
        }
        let st = api::state(rt, state);
        let stacks: Vec<V> = rolled.iter().map(|s| crate::hand::load(rt, Some(s.clone()))).collect();
        // Each as the runtime reads it back, to tell a stack the mod left alone.
        let before: Vec<_> = stacks.iter().map(|&s| crate::hand::hand_stack_of(rt, s)).collect();
        crate::loot_params::rolled(rt, builder, st, stacks.clone());
        let out = vc(rt, DROPS_KEY, crate::loot_params::DROPS, &[b, st, builder]);
        if rt.thrown.is_some() {
            rt.report_thrown(crate::loot_params::DROPS);
            return None;
        }
        let mut drops = Vec::new();
        for s in crate::jutil::elements(rt, out) {
            if s.r() == 0 || crate::api_common::stack_empty(rt, s) {
                continue;
            }
            let Some(held) = crate::hand::hand_stack_of(rt, s) else { continue };
            let origin = stacks.iter().position(|&o| o == s).filter(|&i| before[i].as_ref() == Some(&held));
            drops.push((origin, held));
        }
        Some(drops)
    })
}

const DROPS_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getDrops.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/storage/loot/LootParams$Builder;)Ljava/util/List;";

/// `newState.onPlace(level, pos, oldState, moved)` for a mod block `b`: its override (vanilla's does
/// nothing), unless the old block's `onRemove` replaced it in turn. Create's pipes schedule their
/// tick here, its tanks and vaults join their neighbours, a kinetic block keeps its speed through a
/// change that leaves its connections alone.
fn placed(rt: &mut Rt, b: V, class: &'static str, pos: [i32; 3], old: &str, new: &str, moved: bool) {
    let Some(vc) = rt.vcall.filter(|_| rt.overrides.is_some_and(|o| o(class, ON_PLACE))) else { return };
    let here = rt.host().map(|h| h.block_state(pos)).unwrap_or_default();
    if crate::host::split_state(&here).0 != crate::host::split_state(new).0 {
        return;
    }
    let st = api::state(rt, new);
    let from = api::state(rt, old);
    let l = level(rt);
    let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
    vc(rt, ON_PLACE_KEY, ON_PLACE, &[b, st, l, p, from, V::I(moved as i32)]);
    rt.report_thrown(ON_PLACE);
}

const USE_ITEM_ON: &str = "useItemOn.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
const USE_WITHOUT_ITEM: &str = "useWithoutItem.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/InteractionResult;";

/// `right_click_block`: the acting player right clicks the mod block at `pos` on face `face` (a
/// direction ordinal), the click landing at `hit`, as `ServerPlayerGameMode.useItemOn` asks the
/// block: `useItemOn` with the held stack first, then, when it passes to the default,
/// `useWithoutItem`. Both read the hit result's location (Create's deployer takes an item only from
/// a click away from its front). True when the block took the click (the game's own handling then
/// stops). The game asks once per hand, as vanilla's `ServerPlayerGameMode.useItemOn`: `hand` is the
/// one acting, and only the main hand's click reaches `useWithoutItem`.
pub fn use_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, face: i64, hit: [f64; 3], hand: u8) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let Some((b, class)) = mod_block(rt, state) else { return false };
    let Some(overrides) = rt.overrides else { return false };
    let (with_item, without) = (overrides(class, USE_ITEM_ON), overrides(class, USE_WITHOUT_ITEM));
    // A mod's cauldron that keeps vanilla's `useItemOn`: its interaction map answers.
    let cauldron = !with_item
        && rt.get_field(b, "interactions").r() != 0
        && rt.str_of(rt.get_field(b, "$vanilla")).is_some_and(crate::blocks::cauldron_class);
    if !with_item && !without && !cauldron {
        return false;
    }
    rt.with_host(host, |rt| crate::hand::with_hand(rt, |rt| {
        let Some(vc) = rt.vcall else { return false };
        let st = api::state(rt, state);
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let player = crate::api_common::acting_player(rt);
        let at = crate::api_common::vec3(rt, hit[0], hit[1], hit[2]);
        let hit = crate::api_common::hit(rt, pos, face);
        rt.put_field(hit, "$loc", at);
        let mut took = None;
        if with_item || cauldron {
            let stack = crate::hand::stack(rt, hand);
            let hand = crate::hand::constant(rt, hand);
            let args = [b, stack, st, l, p, player, hand, hit];
            let r = if with_item { vc(rt, USE_ITEM_ON, USE_ITEM_ON, &args) } else { crate::blocks::cauldron_use(rt, &args) };
            let name = rt.str_of(r).unwrap_or("PASS_TO_DEFAULT_BLOCK_INTERACTION").to_owned();
            if name != "PASS_TO_DEFAULT_BLOCK_INTERACTION" {
                took = Some(true); // consumed, skipped or failed: the click is the block's
            }
        }
        if took.is_none() && without && hand == crate::hand::MAIN {
            let r = vc(rt, USE_WITHOUT_ITEM, USE_WITHOUT_ITEM, &[b, st, l, p, player, hit]);
            let name = rt.str_of(r).unwrap_or("PASS").to_owned();
            took = Some(name != "PASS");
        }
        rt.report_thrown("a right click");
        took.unwrap_or(false)
    }))
}

const USE_ON: &str = "useOn.(Lnet/minecraft/world/item/context/UseOnContext;)Lnet/minecraft/world/InteractionResult;";
const USE_ON_KEY: &str = "net/minecraft/world/item/Item.useOn.(Lnet/minecraft/world/item/context/UseOnContext;)Lnet/minecraft/world/InteractionResult;";

/// `use_item_on`: the player right-clicks a block with one of the mod's items, once the block let
/// the click through: the item's own `useOn` (Create's wrench turns the block). `click.item` is the
/// held item, `click.hit_pos` the block clicked. Returns whether the item took the click (anything
/// but `PASS`); an item that doesn't override `useOn` leaves it to the game.
pub fn use_item_on(rt: &mut Rt, host: &mut dyn Host, click: &Placement) -> bool {
    rt.safe_point();
    let Some(&item) = rt.registered.get(&("item".to_owned(), click.item.clone())) else { return false };
    let class = rt.class_of(item);
    if !rt.overrides.is_some_and(|o| o(class, USE_ON)) {
        return false;
    }
    rt.with_host(host, |rt| crate::hand::with_hand(rt, |rt| {
        let Some(vc) = rt.vcall else { return false };
        let ctx = crate::api_common::use_on_context(rt, click);
        let r = vc(rt, USE_ON_KEY, USE_ON, &[item, ctx]);
        let thrown = rt.thrown.is_some();
        rt.report_thrown("useOn");
        !thrown && rt.str_of(r).is_some_and(|name| name != "PASS")
    }))
}

const USE: &str = "use.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResultHolder;";
const USE_KEY: &str = "net/minecraft/world/item/Item.use.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResultHolder;";

/// `native_use_item` (server): a right click with one of the mod's items in `hand`, in the air or on
/// a block that let it through: its class's own `Item.use` (Create's goggles, filters, experience
/// nuggets). True when the use takes the click (its result consumes the action, so the game's own
/// use of the item doesn't run). What the use did to the stack in hand reaches the game's; a stack
/// it answered instead goes in that hand (`ServerPlayerGameMode.useItem`).
pub fn use_item(rt: &mut Rt, host: &mut dyn Host, item: &str, hand: u8) -> bool {
    rt.safe_point();
    let Some(&it) = rt.registered.get(&("item".to_owned(), item.to_owned())) else { return false };
    let class = rt.class_of(it);
    if !rt.overrides.is_some_and(|o| o(class, USE)) {
        return false;
    }
    rt.with_host(host, |rt| crate::hand::with_hand(rt, |rt| {
        let Some(vc) = rt.vcall else { return false };
        let held = crate::hand::stack(rt, hand);
        let l = level(rt);
        let player = crate::api_common::acting_player(rt);
        let constant = crate::hand::constant(rt, hand);
        crate::item_use::use_begins(rt, item);
        let r = vc(rt, USE_KEY, USE, &[it, l, player, constant]);
        let thrown = rt.thrown.is_some();
        rt.report_thrown("use");
        if thrown || r.r() == 0 {
            return false;
        }
        let result = rt.get_field(r, "$result");
        if Rt::tracing() {
            eprintln!("horizon-compat: {item} used: {}", rt.str_of(result).unwrap_or("?"));
        }
        let took = rt.str_of(result).is_some_and(|n| matches!(n, "SUCCESS" | "CONSUME" | "SUCCESS_NO_ITEM_USED" | "CONSUME_PARTIAL"));
        // A use over time it started (`startUsingItem`) is the game's to run: the click goes on to
        // it, whatever the result.
        let started = crate::item_use::use_started(rt);
        let answered = rt.get_field(r, "$object");
        if (took || started) && answered.r() != 0 && answered != held {
            crate::hand::replace(rt, hand, answered);
        }
        took && !started
    }))
}

const STATE_FOR_PLACEMENT: &str = "getStateForPlacement.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;";
const STATE_FOR_PLACEMENT_KEY: &str =
    "net/minecraft/world/level/block/Block.getStateForPlacement.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;";

/// A player placing a block: what vanilla's `BlockPlaceContext` holds.
#[derive(Clone, Debug, Default)]
pub struct Placement {
    /// Where the block goes (`getClickedPos`).
    pub pos: [i32; 3],
    /// The block that was clicked.
    pub hit_pos: [i32; 3],
    /// The clicked face, a direction ordinal (down, up, north, south, west, east).
    pub face: i64,
    /// Where the click landed.
    pub hit: [f64; 3],
    /// The click hit the inside of the clicked block's shape.
    pub inside: bool,
    /// The placer's rotation, in degrees.
    pub yaw: f32,
    pub pitch: f32,
    /// The item in hand (`create:shaft`); empty for none.
    pub item: String,
    /// The hand that holds it, as `InteractionHand`'s ordinal ([`crate::hand::MAIN`] or `OFF`).
    pub hand: u8,
    /// The block goes into the clicked position (`replacingClickedOnBlock`).
    pub replace_clicked: bool,
}

/// `Block.getStateForPlacement` of the mod's block whose default state is `state`: `Some(Some(text))`
/// is the state it goes in, `Some(None)` refuses the placement (Java's null), `None` leaves it to the
/// game (no placement of its own, or its code threw). The host answers the level's blocks and
/// whether the placer sneaks (`isSecondaryUseActive`, `isShiftKeyDown`).
pub fn state_for_placement(rt: &mut Rt, host: &mut dyn Host, state: &str, placement: &Placement) -> Option<Option<String>> {
    rt.safe_point();
    let (b, class) = mod_block(rt, state)?;
    if !rt.overrides.is_some_and(|o| o(class, STATE_FOR_PLACEMENT)) {
        return None;
    }
    rt.with_host(host, |rt| {
        let vc = rt.vcall?;
        let ctx = crate::api_common::place_context(rt, placement);
        let r = vc(rt, STATE_FOR_PLACEMENT_KEY, STATE_FOR_PLACEMENT, &[b, ctx]);
        if rt.thrown.is_some() {
            rt.report_thrown("getStateForPlacement");
            return None;
        }
        if r.r() == 0 {
            return Some(None);
        }
        // Only a state of the block placed is an answer (the game checks it too).
        let text = rt.str_of(r)?.to_owned();
        let (name, _) = crate::host::split_state(state);
        (crate::host::split_state(&text).0 == name).then_some(Some(text))
    })
}

/// `native_can_survive`: `BlockState.canSurvive` of the mod block `state` at `pos` by the mod's own
/// code ([`crate::blocks::mod_survives`]: the block's `canSurvive`, else its vanilla plant base's
/// with its `mayPlaceOn`). None where the block isn't the mod's or its code leaves it to the game's
/// rule for the block's template. The host answers the level's blocks and tags; nothing is written.
pub fn can_survive(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> Option<bool> {
    let (b, class) = mod_block(rt, state)?;
    rt.with_host(host, |rt| {
        let l = level(rt);
        let st = api::state(rt, state);
        let p = api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
        crate::blocks::mod_survives(rt, b, class, st, l, p)
    })
}

/// `native_may_place_on`: the mod plant `state`'s own `mayPlaceOn(ground, level, groundPos)` (the
/// blocks it stands on, which its template's survival asks). None where the plant has no
/// `mayPlaceOn` of its own, or isn't the mod's.
pub fn may_place_on(rt: &mut Rt, host: &mut dyn Host, state: &str, ground: &str, ground_pos: [i32; 3]) -> Option<bool> {
    let (b, class) = mod_block(rt, state)?;
    rt.with_host(host, |rt| {
        let l = level(rt);
        let g = api::state(rt, ground);
        let p = api::block_pos(rt, i64::from(ground_pos[0]), i64::from(ground_pos[1]), i64::from(ground_pos[2]));
        crate::blocks::mod_may_place_on(rt, b, class, g, l, p)
    })
}

const HANDLE_UPDATE_TAG: &str = "handleUpdateTag.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";
const HANDLE_UPDATE_TAG_KEY: &str =
    "net/minecraft/world/level/block/entity/BlockEntity.handleUpdateTag.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";
const GET_UPDATE_TAG: &str = "getUpdateTag.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;";
const GET_UPDATE_TAG_KEY: &str =
    "net/minecraft/world/level/block/entity/BlockEntity.getUpdateTag.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;";
const BER_RENDER: &str = "render.(Lnet/minecraft/world/level/block/entity/BlockEntity;FLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;II)V";
const BER_RENDER_KEY: &str = "net/minecraft/client/renderer/blockentity/BlockEntityRenderer.render.(Lnet/minecraft/world/level/block/entity/BlockEntity;FLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;II)V";
const PROVIDER_CREATE: &str = "net/minecraft/client/renderer/blockentity/BlockEntityRendererProvider.create.(Lnet/minecraft/client/renderer/blockentity/BlockEntityRendererProvider$Context;)Lnet/minecraft/client/renderer/blockentity/BlockEntityRenderer;";
const GET_RENDER_SHAPE: &str = "getRenderShape.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/RenderShape;";
const GET_RENDER_SHAPE_KEY: &str =
    "net/minecraft/world/level/block/state/BlockBehaviour.getRenderShape.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/RenderShape;";
const MODIFY_BAKING_RESULT: &str = "net/neoforged/neoforge/client/event/ModelEvent$ModifyBakingResult";

/// `native_content_block` (startup): what draws the mod block's `state` in the world, by the mod's
/// own code. `"invisible"` or `"entityblock_animated"` when its `getRenderShape` says so, and
/// `"entityblock_animated"` too when the model its client swaps in (`ModelEvent.ModifyBakingResult`,
/// as Create's `BracketedKineticBlockModel` on shafts and cogwheels) has no quads of its own: its
/// block entity renderer draws it. None when chunks draw its model as usual, or it isn't the mod's.
pub fn render_shape(rt: &mut Rt, state: &str) -> Option<&'static str> {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let (b, class) = mod_block(rt, state)?;
    let vc = rt.vcall?;
    if rt.overrides.is_some_and(|o| o(class, GET_RENDER_SHAPE)) {
        let st = api::state(rt, state);
        rt.interned.remove("compat:missing-property");
        let shape = vc(rt, GET_RENDER_SHAPE_KEY, GET_RENDER_SHAPE, &[b, st]);
        // Thrown, or read from a property the game's state lacks (a door the content declares
        // without its `visible`): unknown, so chunks keep drawing it.
        let unknown = rt.thrown.is_some() || rt.interned.remove("compat:missing-property").is_some();
        rt.report_thrown("getRenderShape");
        let name = rt.get_field(shape, "$name");
        match rt.str_of(name) {
            _ if unknown => return None,
            Some("INVISIBLE") => return Some("invisible"),
            Some("ENTITYBLOCK_ANIMATED") => return Some("entityblock_animated"),
            _ => {}
        }
    }
    let models = swapped_models(rt);
    let name = crate::host::split_state(state).0.to_owned();
    let location = crate::render::model_location(&name, &name);
    let key = rt.string(&location);
    let model = crate::jutil::table_get(rt, models, key)?;
    if model.r() == 0 || rt.get_field(model, "$probe").i() != 0 {
        return None;
    }
    let st = api::state(rt, state);
    let data = api::get_static(rt, "net/neoforged/neoforge/client/model/data/ModelData.EMPTY");
    // While probing, every model the runtime stands in for has quads: a wrapper drawing another
    // block's model (a copycat's material) isn't taken for one that draws nothing.
    rt.interned.insert("compat:probing".into(), V::I(1));
    let quads = vc(rt, crate::render::GET_QUADS_KEY, crate::render::GET_QUADS, &[model, st, V::NULL, V::NULL, data, V::NULL]);
    rt.interned.remove("compat:probing");
    let threw = rt.thrown.is_some();
    rt.report_thrown("a swapped block model's getQuads");
    let empty = !threw && quads.r() != 0 && crate::jutil::elements(rt, quads).is_empty();
    // A model drawn from its block entity's data (Create's factory gauge) has no quads without it
    // either: only a block whose renderer draws it is the renderer's.
    (empty && renderer_draws(rt, state)).then_some("entityblock_animated")
}

/// Whether the mod's block entity renderer draws anything for a fresh block entity at `state`.
/// Fires the client's setup: ask on a runtime of its own (the bundles' `with_probe_runtime`).
fn renderer_draws(rt: &mut Rt, state: &str) -> bool {
    const AT: [i32; 3] = [0, -4096, 0];
    struct Probe {
        state: String,
        draws: usize,
    }
    impl Host for Probe {
        fn block_state(&self, pos: [i32; 3]) -> String {
            if pos == AT { self.state.clone() } else { "minecraft:air".into() }
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn is_server(&self) -> bool {
            false
        }
        fn draw_model(&mut self, _model: &crate::host::Model, _pose: &[f32; 16], _color: u32, _light: u32) -> bool {
            self.draws += 1;
            true
        }
    }
    let mut probe = Probe { state: state.to_owned(), draws: 0 };
    render_block_entity(rt, &mut probe, AT, state, &std::collections::BTreeMap::new(), crate::render::FULL_BRIGHT);
    probe.draws > 0
}

/// The block models after the mod's `ModelEvent.ModifyBakingResult` listeners ran, posted once per
/// runtime over a probe model at each of the mod's blocks (its default state's location).
fn swapped_models(rt: &mut Rt) -> V {
    if let Some(&models) = rt.interned.get("compat:baked-models") {
        return models;
    }
    let models = rt.alloc("java/util/HashMap", crate::Data::Table(crate::Table::default()));
    rt.interned.insert("compat:baked-models".into(), models);
    let mut blocks: Vec<String> = rt
        .registered
        .iter()
        .filter(|((r, _), v)| r == "block" && rt.class_of(**v) != "net/minecraft/world/level/block/Block")
        .map(|((_, n), _)| n.clone())
        .collect();
    blocks.sort();
    for name in blocks {
        let probe = rt.new_object("net/minecraft/client/resources/model/BakedModel");
        rt.put_field(probe, "$probe", V::I(1));
        let key = rt.string(&crate::render::model_location(&name, &name));
        crate::jutil::table_put(rt, models, key, probe);
    }
    let ev = rt.new_object(MODIFY_BAKING_RESULT);
    rt.put_field(ev, "$models", models);
    crate::neo::post(rt, MODIFY_BAKING_RESULT, ev);
    models
}

const CLIENT_TICK_PRE: &str = "net/neoforged/neoforge/client/event/ClientTickEvent$Pre";
const CLIENT_TICK_POST: &str = "net/neoforged/neoforge/client/event/ClientTickEvent$Post";

/// `native_block_entity_update_tag` (server): what players are sent of the mod block entity at
/// `pos`, its `getUpdateTag`. None when its object isn't live here (players get its saved data).
pub fn block_entity_update_tag(rt: &mut Rt, pos: [i32; 3]) -> Option<std::collections::BTreeMap<String, crate::nbt::Nbt>> {
    rt.safe_point();
    let be = rt.block_entities.get(&pos).copied()?;
    let vc = rt.vcall?;
    let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
    let tag = vc(rt, GET_UPDATE_TAG_KEY, GET_UPDATE_TAG, &[be, provider]);
    rt.report_thrown("getUpdateTag");
    let nbt = (tag.r() != 0).then(|| crate::nbt::from_runtime(rt, tag))?;
    let brief = nbt_brief(&nbt);
    trace_some(rt, &format!("server-tag/{pos:?}"), &brief, 8, |_| format!("server: update tag sent for {pos:?}: {brief}"));
    Some(nbt)
}

/// `native_block_entity_components` (server): what the mod block entity at `pos` gives the item its
/// block drops (`BlockEntity.collectComponents()`), by component name, each as its codec writes it:
/// its object here, else one made from its saved data. None for a block that isn't the mod's.
pub fn block_entity_components(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>,
) -> Vec<(String, crate::nbt::Nbt)> {
    rt.safe_point();
    rt.with_host(host, |rt| {
        let Some(be) = block_entity(rt, pos, state, nbt) else { return Vec::new() };
        let map = crate::components::collect_components(rt, be);
        crate::components::saved(rt, map)
    })
}

/// With `HORIZON_COMPAT_TRACE` set, prints `msg` the first time `key` comes up on this runtime.
pub(crate) fn trace_once(rt: &mut Rt, key: &str, msg: impl FnOnce(&Rt) -> String) {
    trace_some(rt, "once", key, i32::MAX, msg);
}

/// [`trace_once`] for at most `limit` keys of `group` (a block entity's first update tags).
pub(crate) fn trace_some(rt: &mut Rt, group: &str, key: &str, limit: i32, msg: impl FnOnce(&Rt) -> String) {
    if !Rt::tracing() {
        return;
    }
    let key = format!("trace/{group}/{key}");
    let count_key = format!("trace-count/{group}");
    let n = rt.interned.get(&count_key).map_or(0, |v| v.i());
    if n >= limit || rt.interned.contains_key(&key) {
        return;
    }
    rt.interned.insert(count_key, V::I(n + 1));
    rt.interned.insert(key, V::I(1));
    eprintln!("horizon-compat: {}", msg(rt));
}

/// A compound's keys, and the `id`s anywhere in it (the items a block entity holds), for traces.
fn nbt_brief(nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) -> String {
    fn ids(n: &crate::nbt::Nbt, out: &mut Vec<String>) {
        match n {
            crate::nbt::Nbt::Compound(c) => {
                for (k, v) in c {
                    match v {
                        crate::nbt::Nbt::String(s) if k == "id" => out.push(s.clone()),
                        _ => ids(v, out),
                    }
                }
            }
            crate::nbt::Nbt::List(l) => l.iter().for_each(|v| ids(v, out)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    ids(&crate::nbt::Nbt::Compound(nbt.clone()), &mut found);
    let keys: Vec<&str> = nbt.keys().map(String::as_str).collect();
    format!("{} holding {:?}", keys.join(","), found)
}

/// The client's copy of the mod block entity at `pos`: made the first time, and loaded from `nbt`
/// (what the client keeps of the update tags the server sent) whenever that changes and no packet
/// loaded it as it came ([`block_entity_data`]).
fn client_block_entity(rt: &mut Rt, pos: [i32; 3], state: &str, nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) -> Option<V> {
    let synced = crate::nbt::hash(nbt) as i64;
    let (be, _) = client_block_entity_object(rt, pos, state)?;
    if rt.get_field(be, "$synced").j() == synced && rt.get_field(be, "$loaded").i() != 0 {
        return Some(be);
    }
    load_update_tag(rt, be, pos, state, nbt);
    rt.put_field(be, "$synced", V::J(synced));
    rt.put_field(be, "$loaded", V::I(1));
    Some(be)
}

/// `native_block_entity_data` (client): the client's copy of the block entity at `pos` loads the
/// update tag the server just sent (`packet`) as it arrives, as vanilla's `handleBlockEntityData`
/// does each one, so what the block entity does with it once (Create's press: the particles of what
/// it pressed) is done once per packet, also out of view. `merged` is what the client keeps of the
/// block entity now, so drawing doesn't load it again; a copy made just now loads that instead.
pub fn block_entity_data(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    packet: &std::collections::BTreeMap<String, crate::nbt::Nbt>,
    merged: &std::collections::BTreeMap<String, crate::nbt::Nbt>,
) {
    rt.safe_point();
    client_setup(rt, host);
    if mod_block(rt, state).is_none() {
        return;
    }
    rt.with_host(host, |rt| {
        let Some((be, fresh)) = client_block_entity_object(rt, pos, state) else { return };
        load_update_tag(rt, be, pos, state, if fresh { merged } else { packet });
        rt.put_field(be, "$synced", V::J(crate::nbt::hash(merged) as i64));
        rt.put_field(be, "$loaded", V::I(1));
    });
}

/// The client's copy of the mod block entity at `pos`, and whether it was made just now.
fn client_block_entity_object(rt: &mut Rt, pos: [i32; 3], state: &str) -> Option<(V, bool)> {
    let (block, _) = crate::host::split_state(state);
    let vc = rt.vcall?;
    Some(match rt.block_entities.get(&pos).copied() {
        Some(be) if rt.block_entity_meta.get(&pos).is_some_and(|(b, _)| b == block) => (be, false),
        _ => {
            let (b, _) = mod_block(rt, state)?;
            let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
            let st = api::state(rt, state);
            let be = vc(rt, NEW_BLOCK_ENTITY, NEW_BLOCK_ENTITY, &[b, p, st]);
            rt.report_thrown("newBlockEntity");
            if be.r() == 0 {
                return None;
            }
            let l = level(rt);
            rt.put_field(be, "level", l);
            rt.block_entities.insert(pos, be);
            rt.block_entity_meta.insert(pos, (block.to_owned(), true));
            (be, true)
        }
    })
}

/// The client's block entity `be` loads `nbt`, an update tag (`handleUpdateTag`).
fn load_update_tag(rt: &mut Rt, be: V, pos: [i32; 3], state: &str, nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) {
    let (block, _) = crate::host::split_state(state);
    let Some(vc) = rt.vcall else { return };
    // An empty update tag isn't sent (vanilla's chunk data carries none), so nothing is loaded.
    if !nbt.is_empty() {
        let brief = nbt_brief(nbt);
        trace_some(rt, &format!("client-tag/{pos:?}"), &brief, 8, |_| format!("client: {block} at {pos:?} loads an update tag: {brief}"));
        let tag = crate::nbt::to_runtime(rt, nbt);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        vc(rt, HANDLE_UPDATE_TAG_KEY, HANDLE_UPDATE_TAG, &[be, tag, provider]);
        rt.report_thrown("handleUpdateTag");
    }
}

/// `native_server_data_loaded` (server): the server's data loaded, on a world's first open
/// (`reload` false) and after each `/reload`: the mods' data reload listeners gather
/// (`AddReloadListenerEvent`) and reload from the server's resources ([`crate::resources::server_reload`]).
pub fn server_data_loaded(rt: &mut Rt, host: &mut dyn Host, reload: bool) {
    rt.safe_point();
    if Rt::tracing() {
        eprintln!("horizon-compat: the server's data {}", if reload { "reloaded" } else { "loaded" });
    }
    rt.with_host(host, |rt| {
        crate::resources::server_reload(rt);
        // After `/reload` the new data goes to every player (`PlayerList.reloadResources`); on
        // a world's first open nobody has joined yet, and each gets it as they join.
        if reload {
            crate::events::datapack_sync(rt, None);
        }
    });
}

/// `native_player_logged_in` (server): `player` joined ([`crate::events::logged_in`]).
pub fn player_logged_in(rt: &mut Rt, host: &mut dyn Host, player: u128, name: &str) {
    rt.safe_point();
    rt.with_host(host, |rt| crate::events::logged_in(rt, player, name));
}

/// `native_player_logged_out` (server): `player` left ([`crate::events::logged_out`]).
pub fn player_logged_out(rt: &mut Rt, host: &mut dyn Host, player: u128, name: &str) {
    rt.safe_point();
    rt.with_host(host, |rt| crate::events::logged_out(rt, player, name));
}

/// `native_player_respawned` (server): `player` respawned after dying ([`crate::events::respawned`]).
pub fn player_respawned(rt: &mut Rt, host: &mut dyn Host, player: u128, name: &str) {
    rt.safe_point();
    rt.with_host(host, |rt| crate::events::respawned(rt, player, name));
}

/// `native_destroy_block` (server): the acting player breaks the block `state` at `pos`, the game
/// having allowed it: the mods hear it ([`crate::world_events::block_break`]), then a mod block
/// does its own part before the game removes it ([`crate::block_breaking::player_breaks`]).
/// Whether the block stays: a mod canceled the break, or the block's `onDestroyedByPlayer` kept it.
pub fn destroy_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    rt.safe_point();
    rt.with_host(host, |rt| {
        crate::hand::with_hand(rt, |rt| crate::world_events::block_break(rt, pos, state) || !crate::block_breaking::player_breaks(rt, pos, state))
    })
}

/// `native_place_block` (server): the acting player placed the block `state` at `pos` with `item`.
/// As `BlockItem.place` goes on once the block is set: a mod's block entity reads the stack's
/// components ([`crate::components::apply_from_stack`]), the mod's block hears who placed it with
/// what (`setPlacedBy`), then the mods hear the placement ([`crate::world_events::block_place`]).
pub fn place_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, item: &str) {
    rt.safe_point();
    rt.with_host(host, |rt| {
        crate::hand::with_hand(rt, |rt| {
            placed_with(rt, pos, state, item);
            crate::world_events::block_place(rt, pos, state)
        })
    });
}

const SET_PLACED_BY: &str = "setPlacedBy.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;)V";
const SET_PLACED_BY_KEY: &str = "net/minecraft/world/level/block/Block.setPlacedBy.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;)V";

/// The mod's side of `BlockItem.place` once its block is set from the stack of `item` (the main
/// hand's when it holds that item, else the off hand's): the block entity reads the stack's
/// components (`applyComponentsFromItemStack`: Farmer's Delight's cooking pot its meal, Create's
/// toolbox its contents), then the block's `setPlacedBy` (Create's kinetic blocks note the player,
/// a deployer its owner).
fn placed_with(rt: &mut Rt, pos: [i32; 3], state: &str, item: &str) {
    let Some((b, class)) = mod_block(rt, state) else { return };
    let main = crate::hand::stack(rt, crate::hand::MAIN);
    let in_main = crate::api_common::item_like_name(rt, rt.get_field(main, "$item")) == item;
    let stack = if in_main { main } else { crate::hand::stack(rt, crate::hand::OFF) };
    let be = block_entity_at(rt, pos);
    if be.r() != 0 {
        crate::components::apply_from_stack(rt, be, stack);
    }
    if !rt.overrides.is_some_and(|o| o(class, SET_PLACED_BY)) {
        return;
    }
    let Some(vc) = rt.vcall else { return };
    let l = level(rt);
    let p = api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
    let st = api::state(rt, state);
    let player = crate::api_common::acting_player(rt);
    vc(rt, SET_PLACED_BY_KEY, SET_PLACED_BY, &[b, l, p, st, player, stack]);
    rt.report_thrown("setPlacedBy");
}

/// `native_client_level_load` (client): the client made a level for `dimension`, joining a server
/// or changing dimension ([`crate::world_events::client_level_load`]). The client's setup runs
/// first when this is its first hook.
pub fn client_level_load(rt: &mut Rt, host: &mut dyn Host, dimension: &str) {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::world_events::client_level_load(rt, dimension));
}

/// `native_client_level_unload` (client): the client dropped its level for `dimension`
/// ([`crate::world_events::client_level_unload`]).
pub fn client_level_unload(rt: &mut Rt, host: &mut dyn Host, dimension: &str) {
    rt.safe_point();
    if !crate::neo::is_client(rt) {
        return;
    }
    rt.with_host(host, |rt| crate::world_events::client_level_unload(rt, dimension));
}

/// The client's setup, once, on its first hook ([`crate::neo::fire_client_setup`]), then its first
/// resource load with the game as host: the reload listeners the mods registered read the game's
/// resources ([`crate::resources::reload`]).
fn client_setup(rt: &mut Rt, host: &mut dyn Host) {
    if crate::neo::is_client(rt) {
        return;
    }
    crate::neo::fire_client_setup(rt);
    rt.with_host(host, |rt| crate::resources::reload(rt, "assets"));
}

/// `native_block_entity_submit` (client): draws the mod block entity at `pos` with the renderer its
/// mod registered for its type (`BlockEntityRenderers.register`), under a pose stack at the block's
/// corner. `nbt` is the update tag the server sent; `light` is the block's, packed as
/// `LightTexture.pack`. False when the block entity has no renderer.
pub fn render_block_entity(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>,
    light: i32,
) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    client_setup(rt, host);
    if mod_block(rt, state).is_none() {
        return false;
    }
    rt.with_host(host, |rt| {
        let Some(be) = client_block_entity(rt, pos, state, nbt) else { return false };
        // The camera the renderer finds on its dispatcher (`context.getBlockEntityRenderDispatcher().camera`).
        crate::camera::prepare(rt, None);
        crate::vertices::discard(rt);
        let ty = rt.get_field(be, "$type");
        let Some(id) = rt.registered_name(ty, "block_entity_type").map(str::to_owned) else { return false };
        let Some(provider) = rt.interned.get(&format!("ber/{id}")).copied() else {
            trace_once(rt, &format!("no-ber/{id}"), |_| format!("client: {id} has no block entity renderer"));
            return false;
        };
        trace_once(rt, &format!("ber-used/{id}"), |_| format!("client: {id} drawn by its block entity renderer"));
        let renderer = match rt.interned.get(&format!("ber-made/{id}")).copied() {
            Some(r) => r,
            None => {
                let ctx = rt.new_object("net/minecraft/client/renderer/blockentity/BlockEntityRendererProvider$Context");
                let r = rt.call_functional(PROVIDER_CREATE, provider, &[ctx]);
                rt.report_thrown("a block entity renderer's constructor");
                rt.interned.insert(format!("ber-made/{id}"), r);
                r
            }
        };
        let Some(vc) = rt.vcall else { return false };
        if renderer.r() == 0 {
            return false;
        }
        let st = api::state(rt, state);
        rt.put_field(be, "$state", st);
        rt.put_field(be, "blockState", st);
        trace_some(rt, "drawn-states", &format!("{pos:?}"), 40, |_| format!("client: {pos:?} drawn as {state}"));
        let stack = crate::render::new_pose_stack(rt, crate::render::IDENTITY);
        let buffers = rt.intern("compat:buffers".into(), |rt| rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource"));
        let partial = rt.host().map_or(0.0, |h| h.partial_tick());
        vc(rt, BER_RENDER_KEY, BER_RENDER, &[renderer, be, V::F(partial), stack, buffers, V::I(light), V::I(crate::render::NO_OVERLAY)]);
        rt.report_thrown("a block entity renderer");
        // The vertices it wrote one by one, drawn where it put them.
        crate::vertices::flush(rt, None);
        true
    })
}

const ER_PROVIDER_CREATE: &str =
    "net/minecraft/client/renderer/entity/EntityRendererProvider.create.(Lnet/minecraft/client/renderer/entity/EntityRendererProvider$Context;)Lnet/minecraft/client/renderer/entity/EntityRenderer;";
const ER_RENDER: &str = "render.(Lnet/minecraft/world/entity/Entity;FFLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;I)V";
const ER_RENDER_KEY: &str =
    "net/minecraft/client/renderer/entity/EntityRenderer.render.(Lnet/minecraft/world/entity/Entity;FFLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;I)V";
const ER_SHOULD_RENDER: &str = "shouldRender.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/client/renderer/culling/Frustum;DDD)Z";
const ER_SHOULD_RENDER_KEY: &str =
    "net/minecraft/client/renderer/entity/EntityRenderer.shouldRender.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/client/renderer/culling/Frustum;DDD)Z";

/// The renderer a mod's entity `e` is drawn by: the one its mod registered for its type
/// (`EntityRenderersEvent.RegisterRenderers`), made from its provider the first time it is asked
/// for, as vanilla's `EntityRenderDispatcher` holds one renderer a type. None for an entity with no
/// renderer (told once), or one whose renderer couldn't be made.
pub(crate) fn entity_renderer(rt: &mut Rt, e: V) -> Option<V> {
    let ty = rt.get_field(e, "$type");
    let id = rt.registered_name(ty, "entity_type").map(str::to_owned)?;
    let Some(provider) = rt.interned.get(&format!("er/{id}")).copied() else {
        trace_once(rt, &format!("no-er/{id}"), |_| format!("client: {id} has no entity renderer"));
        return None;
    };
    let renderer = match rt.interned.get(&format!("er-made/{id}")).copied() {
        Some(r) => r,
        None => {
            let ctx = rt.new_object("net/minecraft/client/renderer/entity/EntityRendererProvider$Context");
            let r = rt.call_functional(ER_PROVIDER_CREATE, provider, &[ctx]);
            rt.report_thrown("an entity renderer's constructor");
            rt.interned.insert(format!("er-made/{id}"), r);
            r
        }
    };
    (renderer.r() != 0).then_some(renderer)
}

/// `native_level_submit` (a client): the client's mod entities, each drawn by the renderer its mod
/// registered for its type (`EntityRenderersEvent.RegisterRenderers`), as vanilla's
/// `EntityRenderDispatcher.render` draws one: when the renderer's `shouldRender` agrees, under a
/// pose stack at its place this frame (between its last two ticks') relative to `camera`, with its
/// turn this frame; then the level's render stages (`RenderLevelStageEvent`, see
/// [`crate::camera::render_stages`]). The number of entities drawn.
pub fn render_mod_entities(rt: &mut Rt, host: &mut dyn Host, camera: [f64; 3]) -> usize {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| {
        // The frame's camera: where the mods' renderers measure from (`Camera.getPosition`).
        crate::camera::prepare(rt, Some(camera));
        crate::vertices::discard(rt);
        let Some(vc) = rt.vcall else { return 0 };
        let partial = rt.host().map_or(0.0, |h| h.partial_tick());
        let mut drawn = 0;
        for e in crate::mod_entities::live(rt) {
            if rt.get_field(e, "removalReason").r() != 0 {
                continue;
            }
            let Some(renderer) = entity_renderer(rt, e) else { continue };
            let ty = rt.get_field(e, "$type");
            let id = rt.registered_name(ty, "entity_type").unwrap_or("?").to_owned();
            let frustum = rt.intern("compat:frustum".into(), |rt| rt.new_object("net/minecraft/client/renderer/culling/Frustum"));
            let shows = vc(rt, ER_SHOULD_RENDER_KEY, ER_SHOULD_RENDER, &[renderer, e, frustum, V::D(camera[0]), V::D(camera[1]), V::D(camera[2])]);
            rt.report_thrown("an entity renderer's shouldRender");
            if shows.i() == 0 {
                continue;
            }
            let lerp = |rt: &Rt, old: &'static str, now: &'static str| {
                let (o, n) = (rt.get_field(e, old).d(), rt.get_field(e, now).d());
                o + (n - o) * f64::from(partial)
            };
            let at = [lerp(rt, "xOld", "$x") - camera[0], lerp(rt, "yOld", "$y") - camera[1], lerp(rt, "zOld", "$z") - camera[2]];
            let mut pose = crate::render::IDENTITY;
            pose[12] = at[0] as f32;
            pose[13] = at[1] as f32;
            pose[14] = at[2] as f32;
            let stack = crate::render::new_pose_stack(rt, pose);
            let (yaw_old, yaw) = (rt.get_field(e, "yRotO").f(), rt.get_field(e, "$yRot").f());
            let yaw = yaw_old + (yaw - yaw_old) * partial;
            let buffers = rt.intern("compat:buffers".into(), |rt| rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource"));
            trace_once(rt, &format!("er-used/{id}"), |_| format!("client: {id} drawn by its entity renderer"));
            vc(rt, ER_RENDER_KEY, ER_RENDER, &[renderer, e, V::F(yaw), V::F(partial), stack, buffers, V::I(crate::render::FULL_BRIGHT)]);
            rt.report_thrown("an entity renderer");
            crate::vertices::flush(rt, None);
            drawn += 1;
        }
        // The level's render stages, which mods draw their outlines and effects in.
        crate::camera::render_stages(rt);
        drawn
    })
}

const SERVER_TICK_PRE: &str = "net/neoforged/neoforge/event/tick/ServerTickEvent$Pre";
const SERVER_TICK_POST: &str = "net/neoforged/neoforge/event/tick/ServerTickEvent$Post";

/// The server's tick (`MinecraftServer.tickServer`): `ServerTickEvent.Pre` before the levels tick,
/// `.Post` after them. Create sends its clients the server's tick rate there
/// (`ServerSpeedProvider`), which is what moves the items on their belts.
pub fn server_tick(rt: &mut Rt, host: &mut dyn Host, post: bool) {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    rt.with_host(host, |rt| {
        if !post {
            crate::damage::server_tick(rt);
            forget_broken(rt);
        }
        let class = if post { SERVER_TICK_POST } else { SERVER_TICK_PRE };
        let ev = rt.new_object(class);
        crate::neo::post(rt, class, ev);
    });
}

/// `native_server_about_to_start` (server): NeoForge's `ServerAboutToStartEvent`, before the
/// server loads anything; `ServerLifecycleHooks.getCurrentServer()` is the server from here on.
pub fn server_about_to_start(rt: &mut Rt, host: &mut dyn Host) {
    rt.safe_point();
    rt.with_host(host, crate::neo::server_about_to_start);
}

/// `native_world_lifecycle` (server): the world open (`ServerStartingEvent`, then
/// `ServerStartedEvent`), or the server gone after its world closed or failed to open
/// (`LevelEvent.Unload`, then `ServerStoppedEvent`). `phase` is the game's phase:
/// `open_ready`, `open_failed`, `close_final` or `close_failed`.
pub fn world_lifecycle(rt: &mut Rt, host: &mut dyn Host, phase: &str) {
    rt.safe_point();
    rt.with_host(host, |rt| match phase {
        "open_ready" => crate::neo::server_started(rt),
        "open_failed" | "close_final" | "close_failed" => crate::neo::server_stopped(rt),
        _ => {}
    });
}

/// `native_server_stopping` (server): NeoForge's `ServerStoppingEvent`, with the worlds still
/// loaded and the players still connected.
pub fn server_stopping(rt: &mut Rt, host: &mut dyn Host) {
    rt.safe_point();
    rt.with_host(host, crate::neo::server_stopping);
}

/// `native_register_commands` (server): the mods hear `RegisterCommandsEvent`, and their commands
/// come back as native usages with the operator level each needs ([`crate::commands::register`]).
pub fn register_commands(rt: &mut Rt, host: &mut dyn Host) -> Vec<(String, u8)> {
    rt.safe_point();
    rt.with_host(host, crate::commands::register)
}

/// `native_perform_command` (server): a command line, without its slash, of the player `player`
/// (their UUID and name; the console's for None) at `permission`, standing at `at` and looking
/// `rot` (yaw, pitch): whether a mod's command ran it ([`crate::commands::perform`]). What it says
/// reaches whoever ran it through [`Host::command_feedback`].
pub fn perform_command(rt: &mut Rt, host: &mut dyn Host, player: Option<(u128, &str)>, permission: u8, at: [f64; 3], rot: [f32; 2], line: &str) -> bool {
    rt.safe_point();
    rt.with_host(host, |rt| crate::commands::perform(rt, player, permission, at, rot, line))
}

/// `native_commands` (client): the mods hear `RegisterClientCommandsEvent`, and the roots of their
/// client tree come back ([`crate::commands::register_client`]).
pub fn client_commands(rt: &mut Rt, host: &mut dyn Host) -> Vec<String> {
    rt.safe_point();
    rt.with_host(host, crate::commands::register_client)
}

/// `native_command` (client): a line typed or clicked in chat, without its slash: whether the mods'
/// client tree ran it ([`crate::commands::perform_client`]); false leaves it to the server.
pub fn client_command(rt: &mut Rt, host: &mut dyn Host, line: &str) -> bool {
    rt.safe_point();
    rt.with_host(host, |rt| crate::commands::perform_client(rt, line))
}

/// `native_key` (client): a key or mouse button went down or up (`key.keyboard.g`), repeats left
/// out. With nothing over the world a press clicks the mods' key mappings bound to it
/// ([`crate::keys::key_input`]).
pub fn key_input(rt: &mut Rt, host: &mut dyn Host, key: &str, pressed: bool) {
    rt.safe_point();
    rt.with_host(host, |rt| crate::keys::key_input(rt, key, pressed));
}

/// `native_tooltip` (client): stack `held`'s tooltip lines `lines` (their text, the name first) as
/// the mods leave them ([`crate::tooltips::tooltip`]: their item's own lines, `ItemTooltipEvent`);
/// None when they changed none.
pub fn tooltip(rt: &mut Rt, host: &mut dyn Host, held: crate::host::HandStack, lines: &[String], advanced: bool) -> Option<Vec<crate::tooltips::Line>> {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::tooltips::tooltip(rt, held, lines, advanced))
}

/// `native_input_key` (client): `InputEvent.Key` for a key the window handled (`key.keyboard.g`, on
/// any screen), with GLFW's action (release, press, repeat) and modifier bits
/// ([`crate::input::key`]).
pub fn input_key(rt: &mut Rt, host: &mut dyn Host, key: &str, action: i32, modifiers: i32) {
    rt.safe_point();
    rt.with_host(host, |rt| crate::input::key(rt, key, action, modifiers));
}

/// `native_mouse_button_pre` and `_post` (client): `InputEvent.MouseButton.Pre` (`pre`) or `.Post`;
/// whether a mod cancelled the `Pre` ([`crate::input::button`]).
pub fn mouse_button(rt: &mut Rt, host: &mut dyn Host, button: i32, pressed: bool, modifiers: i32, pre: bool) -> bool {
    rt.safe_point();
    rt.with_host(host, |rt| crate::input::button(rt, button, pressed, modifiers, pre))
}

/// `native_mouse_scrolling` (client): `InputEvent.MouseScrollingEvent`; whether a mod cancelled it
/// ([`crate::input::scrolling`]).
pub fn mouse_scrolling(rt: &mut Rt, host: &mut dyn Host, delta: [f64; 2], buttons: [bool; 3], at: [f64; 2]) -> bool {
    rt.safe_point();
    rt.with_host(host, |rt| crate::input::scrolling(rt, delta, buttons, at))
}

/// `native_mods` (client), as the game starts: the bundle's row in Options > Mods. Its title is
/// its own mod's name and version (its first jar's), with how many more it holds; its description
/// every mod's; its keys the mappings the mods registered (`RegisterKeyMappingsEvent`, which
/// NeoForge posts as the client starts), each as its name, label and default key
/// ([`crate::keys::page_keys`]).
pub fn mod_page(rt: &mut Rt, host: &mut dyn Host) -> (String, String, Vec<[String; 3]>) {
    rt.safe_point();
    let mut ids = crate::neo::loaded_mods(rt);
    if let Some(i) = ids.iter().position(|m| *m == rt.namespace) {
        let own = ids.remove(i);
        ids.insert(0, own);
    }
    let mods: Vec<String> = ids
        .into_iter()
        .map(|id| {
            let text = |rt: &Rt, what: &str| rt.interned.get(&format!("compat:mod-{what}/{id}")).and_then(|&v| rt.str_of(v)).map(str::to_owned);
            let name = text(rt, "name").unwrap_or_else(|| id.clone());
            match text(rt, "version") {
                Some(v) => format!("{name} {v}"),
                None => name,
            }
        })
        .collect();
    let Some(first) = mods.first() else { return Default::default() };
    let title = match mods.len() {
        1 => first.clone(),
        n => format!("{first} + {}", n - 1),
    };
    let description = mods.join(", ");
    let keys = rt.with_host(host, crate::keys::page_keys);
    (title, description, keys)
}

const LEVEL_TICK_PRE: &str = "net/neoforged/neoforge/event/tick/LevelTickEvent$Pre";
const LEVEL_TICK_POST: &str = "net/neoforged/neoforge/event/tick/LevelTickEvent$Post";

/// A server level's tick (`ServerLevel.tick`): `LevelTickEvent.Pre` before it, `.Post` after it, on
/// the server's level. Create ticks its contraptions, couplings, trains and logistics there.
pub fn level_tick(rt: &mut Rt, host: &mut dyn Host, post: bool) {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    rt.with_host(host, |rt| {
        let class = if post { LEVEL_TICK_POST } else { LEVEL_TICK_PRE };
        let ev = rt.new_object(class);
        let l = level(rt);
        rt.put_field(ev, "$level", l);
        crate::neo::post(rt, class, ev);
        // The level's entities tick after the event before the level's own (`ServerLevel.tick`):
        // a mod's own among them.
        if !post {
            let dimension = rt.host().map(|h| h.dimension()).unwrap_or_default();
            crate::mod_entities::tick_all(rt, &dimension);
        }
    });
}

/// A mod payload the server sent the window's game (`native_net_from_server`), to its handler.
/// False when no payload type of this mod has that id.
pub fn payload_from_server(rt: &mut Rt, host: &mut dyn Host, channel: &str, data: &[u8]) -> bool {
    rt.safe_point();
    // This runtime is the client's, also when a payload comes before its first tick.
    client_setup(rt, host);
    if channel == crate::mod_entities::CHANNEL {
        rt.with_host(host, |rt| crate::mod_entities::receive(rt, data));
        return true;
    }
    if channel == crate::menus::CHANNEL {
        rt.with_host(host, |rt| crate::hand::with_hand(rt, |rt| crate::menus::receive_client(rt, data)));
        return true;
    }
    rt.with_host(host, |rt| crate::network::receive(rt, channel, data, None))
}

/// A mod payload `player`'s client sent the server (`native_net_from_client`), or a message about
/// its menu. Once the game calls [`player_tick`], it waits for that player's tick, where the host
/// acts for them (their inventory, where they stand); before, it runs now. False for a channel no
/// mod registered.
pub fn payload_from_client(rt: &mut Rt, host: &mut dyn Host, player: u128, channel: &str, data: &[u8]) -> bool {
    rt.safe_point();
    if channel != crate::menus::CHANNEL && !rt.interned.contains_key(&format!("payload/{channel}")) {
        return false;
    }
    if crate::menus::player_ticks_seen(rt) {
        crate::menus::queue(rt, player, channel, data);
        return true;
    }
    rt.with_host(host, |rt| crate::hand::with_hand(rt, |rt| crate::menus::handle_from_client(rt, player, channel, data)))
}

/// A player's tick on the server (`ServerPlayer.tick`), `host` acting for them: what their client
/// sent since runs, and the menu they have open keeps their client in step and closes when it
/// may no longer stay open ([`crate::menus::player_tick`]). Then the mods hear the tick
/// ([`crate::world_events::player_tick`]), as `Player.tick` posts it from the player's connection
/// tick, after the level's.
pub fn player_tick(rt: &mut Rt, host: &mut dyn Host) {
    rt.safe_point();
    crate::menus::seen_player_tick(rt);
    let Some(uuid) = host.player_id() else { return };
    let menus = crate::menus::has_work(rt, uuid);
    if !menus && !crate::world_events::player_ticks_heard(rt) {
        return;
    }
    rt.with_host(host, |rt| {
        crate::hand::with_hand(rt, |rt| {
            if menus {
                crate::menus::player_tick(rt, uuid);
            }
            crate::world_events::player_tick(rt);
        })
    });
}

/// The mods' HUD layers at the game's layer `layer` (`minecraft:hotbar`), drawn on `host`'s GUI
/// before it (`pre`) or after it ([`crate::gui::draw_layers`]). True when one stands in for the
/// layer: the game skips its own.
pub fn gui_layer(rt: &mut Rt, host: &mut dyn Host, layer: &str, pre: bool) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::gui::draw_layers(rt, layer, pre))
}

/// `native_compute_fov` (client): the world's field of view as the mods' `ViewportEvent.ComputeFov`
/// listeners leave it; None when no mod listens. `host` only reads.
pub fn compute_fov(rt: &mut Rt, host: &mut dyn Host, partial: f32, camera: [f64; 3], degrees: f32) -> Option<f32> {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| {
        let fov = crate::camera::compute_fov(rt, partial, camera, degrees);
        (fov != degrees).then_some(fov)
    })
}

/// `native_compute_fog_color` (client): the fog's colour as the mods' `ViewportEvent.ComputeFogColor`
/// listeners leave it; None when no mod listens. `host` only reads.
pub fn compute_fog_color(rt: &mut Rt, host: &mut dyn Host, partial: f32, camera: [f64; 3], rgb: [f32; 3]) -> Option<[f32; 3]> {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| {
        let out = crate::camera::compute_fog_color(rt, partial, camera, rgb);
        (out != rgb).then_some(out)
    })
}

/// `native_render_fog` (client): `ViewportEvent.RenderFog` for the sky's fog (`sky_end` blocks
/// away) and the terrain's (`near` to `far`, of type `fog_type`: `FogType`'s ordinal): the distances
/// of each a mod that cancels it sets. `host` only reads.
pub fn render_fog(rt: &mut Rt, host: &mut dyn Host, partial: f32, camera: [f64; 3], fog_type: usize, sky_end: f32, near: f32, far: f32) -> (Option<f32>, Option<(f32, f32)>) {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| {
        let sky = crate::camera::render_fog(rt, partial, camera, 0, fog_type, 0.0, sky_end).map(|(_, end)| end);
        let terrain = crate::camera::render_fog(rt, partial, camera, 1, fog_type, near, far);
        (sky, terrain)
    })
}

/// `native_camera_shake` (client): the turn the mods' `ViewportEvent.ComputeCameraAngles` listeners
/// give the camera (pitch, yaw, roll in degrees); None when no mod listens. `host` only reads.
pub fn camera_angles(rt: &mut Rt, host: &mut dyn Host, partial: f32) -> Option<[f32; 3]> {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::camera::camera_angles(rt, partial))
}

/// The mods' screen drawn over the game's (`native_screen_render_post` while it is open), the
/// pointer at `mouse` in GUI pixels ([`crate::screens::render`]). `host` only reads and draws.
pub fn screen_render(rt: &mut Rt, host: &mut dyn Host, mouse: [f32; 2], partial: f32) {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::screens::render(rt, mouse, partial));
}

/// The pointer, the wheel or a key over the mods' screen ([`crate::screens::input`]): true when
/// the screen took it. `host` only reads.
pub fn screen_input(rt: &mut Rt, host: &mut dyn Host, mouse: [f32; 2], input: crate::screens::Input<'_>, modifiers: u32) -> bool {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::screens::input(rt, mouse, input, modifiers))
}

/// The local player holding an item drawn for them (their hands, their inventory's slots), as an
/// item property asks them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Holder {
    pub at: [f64; 3],
    pub yaw: f32,
    /// The hand that uses the drawn stack, when it is the stack in use (`getUseItem() == stack`),
    /// with `remaining` of the use's `duration` ticks left.
    pub hand: Option<u8>,
    pub remaining: i32,
    pub duration: i32,
}

/// `native_item_property` (client): the mods' function for property `property` of a stack (`count`
/// of `item`, with the components `patch`: each set, or removed when None), asked as 1.21.1's
/// `ItemOverrides.resolve` asks it: with the client's level, the local player when `holder` says
/// they hold the stack, and `seed`. The stack in use is the hand's own stack object, so a bow's
/// `entity.getUseItem() == stack` holds as in vanilla. None when no mod registered the property
/// for the item (`ItemProperties.register`, or `registerGeneric` for every item), or it threw.
pub fn item_property(
    rt: &mut Rt,
    host: &mut dyn Host,
    item: &str,
    count: i32,
    patch: &std::collections::BTreeMap<String, Option<serde_json::Value>>,
    property: &str,
    holder: Option<&Holder>,
    seed: i32,
) -> Option<f32> {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    client_setup(rt, host);
    let f = crate::render::property_function(rt, item, property)?;
    rt.with_host(host, |rt| {
        let (entity, held) = match holder {
            Some(h) => {
                let p = crate::network::local_player(rt);
                for (field, v) in ["$x", "$y", "$z"].into_iter().zip(h.at) {
                    rt.put_field(p, field, V::D(v));
                }
                rt.put_field(p, "$yaw", V::F(h.yaw));
                // The use `LivingEntity.getUseItem` and its ticks answer: this stack's, or none.
                let hand = h.hand.map_or(V::NULL, |hand| crate::hand::constant(rt, hand));
                rt.put_field(p, "$usingHand", hand);
                rt.put_field(p, "$useLeft", V::I(h.remaining));
                rt.put_field(p, "$useTicks", V::I(if h.hand.is_some() { h.duration - h.remaining } else { 0 }));
                (p, h.hand.map(|hand| crate::hand::stack(rt, hand)))
            }
            None => (V::NULL, None),
        };
        let stack = match held {
            Some(s) if crate::api_common::item_like_name(rt, rt.get_field(s, "$item")) == item => s,
            _ => {
                let s = crate::api_common::stack(rt, item, count);
                let set = patch.iter().filter_map(|(k, v)| Some((k.clone(), v.clone()?))).collect();
                crate::components::load_saved(rt, s, &set);
                s
            }
        };
        let level = level(rt);
        crate::render::call_property(rt, f, stack, level, entity, seed)
    })
}

/// The client's interaction key `key_mapping` about to act with `hand` (0 main, 1 off): `button` 0
/// attacks, 1 uses, 2 picks a block (`InputEvent.InteractionKeyMappingTriggered`, as
/// [`crate::input::interaction_key`] posts it). Some(whether the hand swings) when a mod cancelled
/// it: the game skips the action, and a cancelled use the rest of the click.
pub fn interaction_key(rt: &mut Rt, host: &mut dyn Host, button: i32, key_mapping: &str, hand: u8) -> Option<bool> {
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| crate::input::interaction_key(rt, button, key_mapping, hand))
}

/// The client's player using `hand` on the block at `pos`, `face` the face clicked (a direction's
/// data value), the cursor meeting it at `hit`: `RightClickBlock` on the client, as
/// `MultiPlayerGameMode.performUseItemOn` posts it before the use goes to the server, which posts
/// its own. Some(the result a listener cancelled it with), which the client answers in the block's
/// and the item's place ([`crate::events::client_right_click_block`]).
pub fn client_use_item_on(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], face: i64, hit: [f64; 3], hand: u8) -> Option<crate::events::Answer> {
    rt.safe_point();
    client_setup(rt, host);
    crate::events::client_right_click_block(rt, host, pos, face, hit, hand)
}

/// The client's tick, in `Minecraft.tick`'s order: `ClientTickEvent.Pre` (animation clocks advance
/// here: catnip's `AnimationTickHolder`), the open screen's tick (and what screens left for it:
/// payloads, opening and closing), the level's block entities, then `ClientTickEvent.Post`, and
/// the mods' ticking sound instances.
pub fn client_tick(rt: &mut Rt, host: &mut dyn Host) {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    client_setup(rt, host);
    rt.with_host(host, |rt| {
        crate::gui::frame(rt);
        let ev = rt.new_object(CLIENT_TICK_PRE);
        crate::neo::post(rt, CLIENT_TICK_PRE, ev);
        crate::screens::tick(rt);
        // `ClientLevel.tickEntities`: the entities (the client's own player among them), then the
        // block entities.
        crate::world_events::client_player_tick(rt);
        let dimension = rt.host().map(|h| h.dimension()).unwrap_or_default();
        crate::mod_entities::tick_all(rt, &dimension);
        tick_client_block_entities(rt);
        let ev = rt.new_object(CLIENT_TICK_POST);
        crate::neo::post(rt, CLIENT_TICK_POST, ev);
        crate::keys::tick(rt);
        // The sound manager's: the mods' own instances that tick.
        crate::sounds::tick(rt);
    });
}

/// The client level's `tickBlockEntities`: every mod block entity the client holds whose block
/// gives it a ticker there. Between the server's updates this is what moves Create's mixer and
/// press heads and the items on belts and depots. One whose block is gone from the client's
/// world is let go, as vanilla removes it with its block.
fn tick_client_block_entities(rt: &mut Rt) {
    let Some(vc) = rt.vcall else { return };
    if rt.block_entities.is_empty() {
        return;
    }
    let mut held: Vec<([i32; 3], V)> = rt.block_entities.iter().map(|(p, be)| (*p, *be)).collect();
    held.sort_unstable_by_key(|(p, _)| *p);
    let level = level(rt);
    for (pos, be) in held {
        let Some(state) = rt.host().map(|h| h.block_state(pos)) else { return };
        let (block, _) = crate::host::split_state(&state);
        if rt.block_entity_meta.get(&pos).is_none_or(|(b, _)| b != block) {
            block_entity_removed(rt, pos);
            continue;
        }
        let Some((b, _)) = mod_block(rt, &state) else { continue };
        let st = api::state(rt, &state);
        rt.put_field(be, "$state", st);
        rt.put_field(be, "blockState", st);
        let ty = rt.get_field(be, "$type");
        let ticker = vc(rt, GET_TICKER, GET_TICKER, &[b, level, st, ty]);
        if ticker.r() != 0 {
            let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
            rt.call_functional(TICKER_TICK, ticker, &[level, p, st, be]);
        }
        rt.report_thrown("the block entity's client tick");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// The test block's `getTicker` gives a ticker (lambda 1) that counts its block entity's ticks.
    fn vcall(rt: &mut Rt, _owner_key: &'static str, key: &'static str, _args: &[V]) -> V {
        if key == GET_TICKER { rt.lambda_id("test/Ticker", 1, Vec::new()) } else { V::Void }
    }

    fn count_tick(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        let be = args[4];
        let n = rt.get_field(be, "ticks").i();
        rt.put_field(be, "ticks", V::I(n + 1));
        Some(V::Void)
    }

    #[test]
    fn the_client_ticks_the_block_entities_it_holds_and_lets_go_of_those_whose_block_is_gone() {
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(count_tick);
        let block = rt.new_object("test/MixerBlock");
        rt.registered.insert(("block".into(), "t:mixer".into()), block);
        let mut host = MemoryHost::default();
        let (here, gone) = ([0, 64, 0], [5, 64, 0]);
        host.blocks.insert(here, "t:mixer".into());
        for pos in [here, gone] {
            let be = rt.new_object("test/MixerBlockEntity");
            rt.block_entities.insert(pos, be);
            rt.block_entity_meta.insert(pos, ("t:mixer".into(), true));
        }
        let be = rt.block_entities[&here];
        for _ in 0..3 {
            client_tick(&mut rt, &mut host);
        }
        assert_eq!(rt.get_field(be, "ticks"), V::I(3));
        assert!(!rt.block_entities.contains_key(&gone));
    }

    const POT: &str = "test/PotBlock";
    const POT_ENTITY: &str = "test/PotBlockEntity";
    const INPUT_GET: &str = "net/minecraft/world/level/block/entity/BlockEntity$DataComponentInput.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    const BUILDER_SET: &str =
        "net/minecraft/core/component/DataComponentMap$Builder.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Lnet/minecraft/core/component/DataComponentMap$Builder;";

    /// The test pot: its block makes a block entity and hears `setPlacedBy`; its block entity reads
    /// the stack's meal as it is placed and gives it back for its item, as Farmer's Delight's
    /// cooking pot does.
    fn pot_overrides(class: &str, method: &str) -> bool {
        (class == POT && (method == SET_PLACED_BY || method == NEW_BLOCK_ENTITY))
            || (class == POT_ENTITY && (method.starts_with("applyImplicitComponents.") || method.starts_with("collectImplicitComponents.")))
    }

    fn pot_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        let meal = rt.registered.get(&("data_component_type".to_owned(), "test:meal".to_owned())).copied().unwrap_or(V::NULL);
        match short {
            NEW_BLOCK_ENTITY => rt.new_object(POT_ENTITY),
            // No ticker, and no data of its own to save or load.
            GET_TICKER => V::NULL,
            LOAD | SAVE => V::Void,
            SET_PLACED_BY => {
                rt.put_field(args[0], "placer", args[4]);
                rt.put_field(args[0], "stack", args[5]);
                V::Void
            }
            s if s.starts_with("applyImplicitComponents.") => {
                let got = api::call(rt, INPUT_GET, &[args[1], meal]);
                rt.put_field(args[0], "meal", got);
                V::Void
            }
            s if s.starts_with("collectImplicitComponents.") => {
                let m = rt.get_field(args[0], "meal");
                api::call(rt, BUILDER_SET, &[args[1], meal, m]);
                V::Void
            }
            _ => api::call(rt, key, args),
        }
    }

    /// The test pot's block and its two persistent component types: `test:meal` (a string, which
    /// its block entity reads) and `test:lid` (an int, which it doesn't).
    fn pot_runtime() -> (Rt, V, V, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(pot_overrides);
        rt.vcall = Some(pot_code);
        rt.supers.insert(POT, "net/minecraft/world/level/block/Block");
        rt.supers.insert(POT_ENTITY, "net/minecraft/world/level/block/entity/BlockEntity");
        let block = rt.new_object(POT);
        rt.registered.insert(("block".into(), "test:pot".into()), block);
        let new_type = |rt: &mut Rt, name: &str, codec: &'static str| {
            let b = api::call(rt, "net/minecraft/core/component/DataComponentType.builder.()Lnet/minecraft/core/component/DataComponentType$Builder;", &[]);
            let c = api::get_static(rt, codec);
            let b = api::call(rt, "net/minecraft/core/component/DataComponentType$Builder.persistent.(Lcom/mojang/serialization/Codec;)Lnet/minecraft/core/component/DataComponentType$Builder;", &[b, c]);
            let t = api::call(rt, "net/minecraft/core/component/DataComponentType$Builder.build.()Lnet/minecraft/core/component/DataComponentType;", &[b]);
            rt.registered.insert(("data_component_type".into(), name.into()), t);
            t
        };
        let meal = new_type(&mut rt, "test:meal", "com/mojang/serialization/Codec.STRING");
        let lid = new_type(&mut rt, "test:lid", "com/mojang/serialization/Codec.INT");
        (rt, block, meal, lid)
    }

    /// A host with the test pot at `pos` and one in the main hand carrying a meal and a lid.
    fn pot_host(pos: [i32; 3]) -> MemoryHost {
        let mut host = MemoryHost::default();
        host.blocks.insert(pos, "test:pot".into());
        host.held = Some(("test:pot".into(), 1));
        host.held_components.insert("test:meal".into(), crate::nbt::Nbt::String("stew".into()));
        host.held_components.insert("test:lid".into(), crate::nbt::Nbt::Int(2));
        host
    }

    /// What a placed block entity kept of its stack is saved with it under `components`, as
    /// vanilla's `saveWithoutMetadata` writes it, and the block entity made again from that data
    /// (the world loaded again) has it back.
    #[test]
    fn a_block_entitys_kept_components_are_saved_with_it() {
        use crate::nbt::Nbt;
        let (mut rt, _, _, _) = pot_runtime();
        let pos = [3, 64, 3];
        let mut host = pot_host(pos);
        place_block(&mut rt, &mut host, pos, "test:pot", "test:pot");
        let mut nbt = std::collections::BTreeMap::new();
        assert!(block_entity_tick(&mut rt, &mut host, pos, "test:pot", &mut nbt));
        let lid = || Nbt::Compound([("test:lid".to_owned(), Nbt::Int(2))].into_iter().collect());
        assert_eq!(nbt.get("components"), Some(&lid()), "the lid, which it didn't read, is kept: {nbt:?}");
        block_entity_removed(&mut rt, pos);
        let named = block_entity_components(&mut rt, &mut host, pos, "test:pot", &nbt);
        assert_eq!(named, [("test:lid".to_owned(), Nbt::Int(2))], "the test pot saves no meal of its own");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod block placed from a stack: its block entity reads the stack's components
    /// (`applyImplicitComponents`), keeps those it didn't read (`components()`) and gives both back
    /// for its item (`collectComponents()`); the block hears who placed it, with what
    /// (`setPlacedBy`).
    #[test]
    fn a_placed_mod_block_reads_its_stack_and_hears_who_placed_it() {
        let (mut rt, block, meal, lid) = pot_runtime();
        let pos = [3, 64, 3];
        let mut host = pot_host(pos);

        place_block(&mut rt, &mut host, pos, "test:pot", "test:pot");
        let be = rt.block_entities[&pos];
        assert_eq!(rt.str_of(rt.get_field(be, "meal")), Some("stew"));
        let (placer, stack) = (rt.get_field(block, "placer"), rt.get_field(block, "stack"));
        assert!(crate::api_common::is_player(rt.class_of(placer)), "{}", rt.class_of(placer));
        assert_eq!(crate::api_common::item_like_name(&rt, rt.get_field(stack, "$item")), "test:pot");

        let has = "net/minecraft/core/component/DataComponentMap.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
        let kept = api::call(&mut rt, "net/minecraft/world/level/block/entity/BlockEntity.components.()Lnet/minecraft/core/component/DataComponentMap;", &[be]);
        assert_eq!((api::call(&mut rt, has, &[kept, lid]), api::call(&mut rt, has, &[kept, meal])), (V::I(1), V::I(0)), "the lid wasn't read, the meal was");
        let all = api::call(&mut rt, "net/minecraft/world/level/block/entity/BlockEntity.collectComponents.()Lnet/minecraft/core/component/DataComponentMap;", &[be]);
        assert_eq!((api::call(&mut rt, has, &[all, lid]), api::call(&mut rt, has, &[all, meal])), (V::I(1), V::I(1)));
        let get = "net/minecraft/core/component/DataComponentMap.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
        let back = api::call(&mut rt, get, &[all, meal]);
        assert_eq!(rt.str_of(back), Some("stew"));

        // Broken, it names what its item keeps, each as its type's codec writes it; a block that
        // isn't the mod's names nothing.
        use crate::nbt::Nbt;
        let named = block_entity_components(&mut rt, &mut host, pos, "test:pot", &Default::default());
        assert_eq!(named, [("test:lid".to_owned(), Nbt::Int(2)), ("test:meal".to_owned(), Nbt::String("stew".into()))]);
        host.blocks.insert([9, 64, 9], "minecraft:stone".into());
        assert!(block_entity_components(&mut rt, &mut host, [9, 64, 9], "minecraft:stone", &Default::default()).is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Lambda 1 is vanilla 1.21.1's bow `pull`: how far the stack in use is drawn, in seconds of
    /// its 72000-tick use, nothing for a stack not in use. Lambda 2 answers -5 for any stack.
    fn properties(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        const USE_ITEM: &str = "net/minecraft/world/entity/LivingEntity.getUseItem.()Lnet/minecraft/world/item/ItemStack;";
        const LEFT: &str = "net/minecraft/world/entity/LivingEntity.getUseItemRemainingTicks.()I";
        let (stack, entity) = (args[1], args[3]);
        if id == 2 {
            return Some(V::F(-5.0));
        }
        if entity.r() == 0 || api::call(rt, USE_ITEM, &[entity]) != stack {
            return Some(V::F(0.0));
        }
        let left = api::call(rt, LEFT, &[entity]).i();
        Some(V::F((72000 - left) as f32 / 20.0))
    }

    #[test]
    fn item_properties_the_mods_register_answer_for_the_stack_in_use() {
        const REGISTER: &str = "net/minecraft/client/renderer/item/ItemProperties.register.(Lnet/minecraft/world/item/Item;Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/client/renderer/item/ItemPropertyFunction;)V";
        const GENERIC: &str = "net/minecraft/client/renderer/item/ItemProperties.registerGeneric.(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/client/renderer/item/ItemPropertyFunction;)Lnet/minecraft/client/renderer/item/ItemPropertyFunction;";
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(properties);
        let bow = rt.new_object("test/ModBow");
        rt.registered.insert(("item".into(), "t:bow".into()), bow);
        // The mod's `(stack, level, entity, seed) -> ...`, typed as the clamped kind; then a
        // generic one as a plain `ItemPropertyFunction`.
        let pull = rt.lambda_id("test/ModBow.pull", 1, Vec::new());
        rt.lambda_info.insert(pull.r(), ("net/minecraft/client/renderer/item/ClampedItemPropertyFunction", "unclampedCall"));
        let name = rt.string("pull");
        api::call(&mut rt, REGISTER, &[bow, name, pull]);
        let low = rt.lambda_id("test/Low.call", 2, Vec::new());
        rt.lambda_info.insert(low.r(), ("net/minecraft/client/renderer/item/ItemPropertyFunction", "call"));
        let name = rt.string("t:low");
        assert_eq!(api::call(&mut rt, GENERIC, &[name, low]), low);

        let mut host = MemoryHost { held: Some(("t:bow".into(), 1)), ..MemoryHost::default() };
        let patch = std::collections::BTreeMap::new();
        let mut ask = |rt: &mut Rt, item: &str, property: &str, holder: Option<&Holder>| {
            item_property(rt, &mut host, item, 1, &patch, property, holder, 0)
        };
        let drawing = |left: i32| Holder { hand: Some(crate::hand::MAIN), remaining: left, duration: 72000, ..Holder::default() };
        // Drawn back 10 ticks: half a second.
        assert_eq!(ask(&mut rt, "t:bow", "minecraft:pull", Some(&drawing(71990))), Some(0.5));
        // 100 ticks: 5, held to 1 (`ClampedItemPropertyFunction.call`).
        assert_eq!(ask(&mut rt, "t:bow", "minecraft:pull", Some(&drawing(71900))), Some(1.0));
        // A stack not in use, and one nobody holds (an item frame's): 0.
        let idle = Holder { hand: None, ..drawing(71990) };
        assert_eq!(ask(&mut rt, "t:bow", "minecraft:pull", Some(&idle)), Some(0.0));
        assert_eq!(ask(&mut rt, "t:bow", "minecraft:pull", None), Some(0.0));
        // Not registered for that item, or at all: the game's -inf.
        assert_eq!(ask(&mut rt, "t:arrow", "minecraft:pull", None), None);
        assert_eq!(ask(&mut rt, "t:bow", "minecraft:pulling", None), None);
        // A generic property answers for every item, unclamped.
        assert_eq!(ask(&mut rt, "t:arrow", "t:low", None), Some(-5.0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const BASIN: &str = "test/BasinBlock";
    const BASIN_ENTITY: &str = "test/BasinBlockEntity";
    const GET_BLOCK_ENTITY: &str = "net/minecraft/world/level/Level.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
    const SET_BLOCK: &str = "net/minecraft/world/level/Level.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z";

    /// The test basin: its block makes a block entity, and its `onRemove` notes the block entity
    /// the level holds at it while it runs, then goes on to vanilla's (`super.onRemove`), as
    /// Create's `IBE.onRemove` reads its block entity to drop what it held; its block entity counts
    /// the times it is told it was removed (`setRemoved`).
    fn basin_overrides(class: &str, method: &str) -> bool {
        (class == BASIN && (method == ON_REMOVE || method == NEW_BLOCK_ENTITY)) || (class == BASIN_ENTITY && method == SET_REMOVED)
    }

    fn basin_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match short {
            NEW_BLOCK_ENTITY => rt.new_object(BASIN_ENTITY),
            GET_TICKER => V::NULL,
            LOAD | SAVE => V::Void,
            ON_REMOVE => {
                let seen = api::call(rt, GET_BLOCK_ENTITY, &[args[2], args[3]]);
                rt.interned.insert("test:seen".into(), seen);
                api::call(rt, key, args)
            }
            SET_REMOVED => {
                let n = rt.get_field(args[0], "removed").i();
                rt.put_field(args[0], "removed", V::I(n + 1));
                V::Void
            }
            _ => api::call(rt, key, args),
        }
    }

    /// The game as the bundle's hook has it while the runtime is busy: a write that replaces a mod
    /// block is reported for later ([`defer_replaced`]).
    struct Reporting(MemoryHost);
    impl Host for Reporting {
        fn block_state(&self, pos: [i32; 3]) -> String {
            self.0.block_state(pos)
        }
        fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {
            let old = self.0.block_state(pos);
            let set = self.0.set_block_state(pos, state);
            if old.starts_with("test:") {
                defer_replaced(pos, &old, state, false);
            }
            set
        }
    }

    /// A mod block's `onRemove` as the game reports its block replaced: a change of state that
    /// keeps the block keeps its block entity; broken, the block's `onRemove` still finds its block
    /// entity at it, which is then let go and told so. Replaced by the mod's own `setBlock`, the
    /// block hears of it before that call returns.
    #[test]
    fn a_mod_blocks_on_remove_finds_its_block_entity_which_then_goes() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(basin_overrides);
        rt.vcall = Some(basin_code);
        rt.supers.insert(BASIN, "net/minecraft/world/level/block/Block");
        rt.supers.insert(BASIN_ENTITY, "net/minecraft/world/level/block/entity/BlockEntity");
        let block = rt.new_object(BASIN);
        rt.registered.insert(("block".into(), "test:basin".into()), block);
        let mut host = MemoryHost::default();
        let at = [2, 64, 2];
        host.blocks.insert(at, "test:basin[full=false]".into());
        let be = rt.with_host(&mut host, |rt| block_entity_at(rt, at));
        assert_ne!(be.r(), 0);
        host.blocks.insert(at, "test:basin[full=true]".into());
        block_replaced(&mut rt, &mut host, at, "test:basin[full=false]", "test:basin[full=true]", false);
        assert_eq!(rt.block_entities.get(&at), Some(&be), "a change of state keeps it");
        assert_eq!(rt.get_field(be, "removed").i(), 0);
        host.blocks.insert(at, "minecraft:air".into());
        block_replaced(&mut rt, &mut host, at, "test:basin[full=true]", "minecraft:air", false);
        assert_eq!(rt.interned.get("test:seen"), Some(&be), "onRemove found it");
        assert_eq!(rt.get_field(be, "removed").i(), 1);
        assert!(!rt.block_entities.contains_key(&at));

        let mut game = Reporting(host);
        game.0.blocks.insert(at, "test:basin[full=false]".into());
        let again = rt.with_host(&mut game, |rt| block_entity_at(rt, at));
        assert!(again.r() != 0 && again != be);
        rt.with_host(&mut game, |rt| {
            let (l, p) = (level(rt), api::block_pos(rt, at[0] as i64, at[1] as i64, at[2] as i64));
            let stone = api::state(rt, "minecraft:stone");
            assert_eq!(api::call(rt, SET_BLOCK, &[l, p, stone, V::I(3)]).i(), 1);
            assert_eq!(rt.interned.get("test:seen"), Some(&again), "before setBlock returned");
            assert_eq!(rt.get_field(again, "removed").i(), 1);
        });
        assert!(!rt.block_entities.contains_key(&at));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const PIPE: &str = "test/PipeBlock";
    const TRAP: &str = "test/TrapBlock";

    /// The test pipe notes each `onPlace` (its state and the one it replaced); the test trap's
    /// `onRemove` sets stone where it was.
    fn pipe_overrides(class: &str, method: &str) -> bool {
        (class == PIPE && method == ON_PLACE) || (class == TRAP && method == ON_REMOVE)
    }

    fn pipe_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match short {
            ON_PLACE => {
                let line = format!("{} over {}", rt.str_of(args[1]).unwrap_or(""), rt.str_of(args[4]).unwrap_or(""));
                let heard = rt.interned.get("test:placed").and_then(|v| rt.str_of(*v)).map(str::to_owned);
                let all = heard.map_or_else(|| line.clone(), |h| format!("{h}; {line}"));
                let s = rt.string(&all);
                rt.interned.insert("test:placed".into(), s);
                V::Void
            }
            ON_REMOVE => {
                let stone = api::state(rt, "minecraft:stone");
                api::call(rt, SET_BLOCK, &[args[2], args[3], stone, V::I(3)]);
                V::Void
            }
            _ => api::call(rt, key, args),
        }
    }

    /// A mod block's `onPlace` once it is set, over air or over its own state when a property
    /// changes, with the state it replaced; not when the old block's `onRemove` replaced it in turn.
    #[test]
    fn a_mod_blocks_on_place_hears_what_it_replaced() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(pipe_overrides);
        rt.vcall = Some(pipe_code);
        for (class, name) in [(PIPE, "test:pipe"), (TRAP, "test:trap")] {
            rt.supers.insert(class, "net/minecraft/world/level/block/Block");
            let block = rt.new_object(class);
            rt.registered.insert(("block".into(), name.into()), block);
        }
        let mut host = MemoryHost::default();
        let at = [0, 64, 0];
        let set = |rt: &mut Rt, host: &mut MemoryHost, from: &str, to: &str| {
            host.blocks.insert(at, to.into());
            block_replaced(rt, host, at, from, to, false);
        };
        set(&mut rt, &mut host, "minecraft:air", "test:pipe[axis=x]");
        set(&mut rt, &mut host, "test:pipe[axis=x]", "test:pipe[axis=y]");
        set(&mut rt, &mut host, "test:pipe[axis=y]", "test:trap");
        set(&mut rt, &mut host, "test:trap", "test:pipe[axis=z]");
        let heard = rt.interned.get("test:placed").and_then(|v| rt.str_of(*v)).unwrap_or("");
        assert_eq!(heard, "test:pipe[axis=x] over minecraft:air; test:pipe[axis=y] over test:pipe[axis=x]");
        assert_eq!(host.blocks.get(&at).map(String::as_str), Some("minecraft:stone"), "the trap's onRemove");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const BELT: &str = "test/BeltBlock";
    const BELT_ENTITY: &str = "test/BeltBlockEntity";
    const OPTIONAL_PARAM: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.getOptionalParameter.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Ljava/lang/Object;";
    const STATE_GET_DROPS: &str = "net/minecraft/world/level/block/state/BlockState.getDrops.(Lnet/minecraft/world/level/storage/loot/LootParams$Builder;)Ljava/util/List;";
    /// A mod block's `super.getDrops`, as the translator names it: the nearest vanilla class's.
    const SUPER_GET_DROPS: &str = "net/minecraft/world/level/block/Block.getDrops.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/storage/loot/LootParams$Builder;)Ljava/util/List;";
    const STATIC_GET_DROPS: &str = "net/minecraft/world/level/block/Block.getDrops.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)Ljava/util/List;";

    /// The test belt, as Create's `BeltBlock.getDrops`: its loot (`super.getDrops`), and the shaft's
    /// when its block entity holds a pulley. It notes the block entity and the entity it was given.
    fn belt_overrides(class: &str, method: &str) -> bool {
        class == BELT && (method == NEW_BLOCK_ENTITY || method == crate::loot_params::DROPS)
    }

    fn belt_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match short {
            NEW_BLOCK_ENTITY => rt.new_object(BELT_ENTITY),
            crate::loot_params::DROPS => {
                let drops = api::call(rt, SUPER_GET_DROPS, args);
                let param = api::get_static(rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParams.BLOCK_ENTITY");
                let be = api::call(rt, OPTIONAL_PARAM, &[args[2], param]);
                rt.interned.insert("test:seen".into(), be);
                let param = api::get_static(rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParams.THIS_ENTITY");
                let entity = api::call(rt, OPTIONAL_PARAM, &[args[2], param]);
                rt.interned.insert("test:entity".into(), entity);
                if be.r() != 0 && rt.class_of(be) == BELT_ENTITY && rt.get_field(be, "pulley").i() != 0 {
                    let shaft = api::state(rt, "test:shaft[axis=y]");
                    let more = api::call(rt, STATE_GET_DROPS, &[shaft, args[2]]);
                    api::call(rt, "java/util/List.addAll.(Ljava/util/Collection;)Z", &[drops, more]);
                }
                drops
            }
            _ => api::call(rt, key, args),
        }
    }

    fn items(rt: &mut Rt, list: V) -> Vec<(String, u32)> {
        crate::jutil::elements(rt, list)
            .into_iter()
            .filter_map(|s| crate::hand::hand_stack_of(rt, s))
            .map(|h| (h.item, h.count))
            .collect()
    }

    /// A mod block's own `getDrops(state, LootParams.Builder)`: through `Block.getDrops` in a mod's
    /// code, its `super.` call rolls its loot table; for the game's own break, after its `onRemove`
    /// let its block entity go, it still gets that block entity and the player, and its `super.`
    /// call gets the game's stacks back, which the game then keeps as they were. A block without
    /// one leaves the game's drops alone; a block entity kept for drops that never came is let go
    /// at the next server tick.
    #[test]
    fn a_mod_blocks_get_drops_answers_with_the_block_entity_it_had() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(belt_overrides);
        rt.vcall = Some(belt_code);
        rt.supers.insert(BELT, "net/minecraft/world/level/block/Block");
        rt.supers.insert(BELT_ENTITY, "net/minecraft/world/level/block/entity/BlockEntity");
        let block = rt.new_object(BELT);
        rt.registered.insert(("block".into(), "test:belt".into()), block);
        let mut host = MemoryHost { player_id: Some(7), ..MemoryHost::default() };
        host.loot.insert("test:belt".into(), vec![("test:belt_connector".into(), 2)]);
        host.loot.insert("test:shaft".into(), vec![("test:shaft".into(), 1)]);
        let at = [4, 64, -1];
        let belt = "test:belt[part=start]";
        host.blocks.insert(at, belt.into());
        let be = rt.with_host(&mut host, |rt| block_entity_at(rt, at));
        assert_ne!(be.r(), 0);
        rt.put_field(be, "pulley", V::I(1));

        // In a mod's own code (a drill's break): the loot table, then the shaft.
        let got = rt.with_host(&mut host, |rt| {
            let (l, p, st) = (level(rt), api::block_pos(rt, 4, 64, -1), api::state(rt, belt));
            let list = api::call(rt, STATIC_GET_DROPS, &[st, l, p, be, V::NULL, V::NULL]);
            items(rt, list)
        });
        assert_eq!(got, [("test:belt_connector".to_owned(), 2), ("test:shaft".to_owned(), 1)]);
        assert_eq!(rt.interned.get("test:seen"), Some(&be));
        assert_eq!(rt.interned.get("test:entity").map(|v| v.r()), Some(0), "no entity broke it");

        // The game's break: the block goes first, then the game rolls its loot (one connector).
        host.blocks.insert(at, "minecraft:air".into());
        block_replaced(&mut rt, &mut host, at, belt, "minecraft:air", false);
        assert!(!rt.block_entities.contains_key(&at), "gone with its block");
        let connector = crate::host::HandStack { item: "test:belt_connector".into(), count: 1, ..Default::default() };
        let answer = loot_drops(&mut rt, &mut host, at, belt, std::slice::from_ref(&connector), None, Some(7), None);
        let answer = answer.expect("the belt answers");
        assert_eq!(answer.len(), 2, "{answer:?}");
        assert_eq!(answer[0], (Some(0), connector.clone()), "the game's own stack, left as it was");
        assert_eq!((answer[1].0, answer[1].1.item.as_str(), answer[1].1.count), (None, "test:shaft", 1));
        assert_eq!(rt.interned.get("test:seen"), Some(&be), "the block entity it had");
        let entity = rt.interned.get("test:entity").copied().unwrap_or_default();
        assert_eq!(rt.class_of(entity), "net/minecraft/server/level/ServerPlayer");
        assert!(!rt.interned.contains_key(&broken_key(at)), "used");

        // A vanilla block, or none of the mod's own: the game's drops stand.
        assert!(loot_drops(&mut rt, &mut host, at, "minecraft:stone", std::slice::from_ref(&connector), None, None, None).is_none());

        // Kept for drops that never come (a `/setblock`): let go at the next server tick.
        host.blocks.insert(at, belt.into());
        let again = rt.with_host(&mut host, |rt| block_entity_at(rt, at));
        assert!(again.r() != 0 && again != be);
        host.blocks.insert(at, "minecraft:stone".into());
        block_replaced(&mut rt, &mut host, at, belt, "minecraft:stone", false);
        assert_eq!(rt.interned.get(&broken_key(at)), Some(&again));
        server_tick(&mut rt, &mut host, false);
        assert!(!rt.interned.contains_key(&broken_key(at)));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const PISTON: &str = "test/PistonBlock";
    const PANEL: &str = "test/PanelBlock";
    const WILL_DESTROY: &str = "playerWillDestroy.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/player/Player;)Lnet/minecraft/world/level/block/state/BlockState;";
    const DESTROYED: &str = "onDestroyedByPlayer.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;ZLnet/minecraft/world/level/material/FluidState;)Z";
    /// The test blocks' `super.` calls, as the translator names them: the nearest vanilla class's.
    const SUPER_WILL_DESTROY: &str = "net/minecraft/world/level/block/DirectionalBlock.playerWillDestroy.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/player/Player;)Lnet/minecraft/world/level/block/state/BlockState;";
    const SUPER_DESTROYED: &str = "net/minecraft/world/level/block/FaceAttachedHorizontalDirectionalBlock.onDestroyedByPlayer.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;ZLnet/minecraft/world/level/material/FluidState;)Z";

    /// The test piston, as Create's `MechanicalPistonBlock.playerWillDestroy`: the pole above it goes
    /// with it (`Level.destroyBlock`, dropped but for a creative player), then its `super.` call. The
    /// test panel, as Create's `FactoryPanelBlock.onDestroyedByPlayer`: with two gauges it loses one
    /// and stays, else its `super.` call; it notes whether the player harvests it.
    fn break_overrides(class: &str, method: &str) -> bool {
        (class == PISTON && method == WILL_DESTROY) || (class == PANEL && method == DESTROYED)
    }

    fn break_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match short {
            WILL_DESTROY => {
                let above = api::call(rt, "net/minecraft/core/BlockPos.above.()Lnet/minecraft/core/BlockPos;", &[args[2]]);
                let creative = api::call(rt, "net/minecraft/world/entity/player/Player.isCreative.()Z", &[args[4]]).i() != 0;
                api::call(rt, "net/minecraft/world/level/Level.destroyBlock.(Lnet/minecraft/core/BlockPos;Z)Z", &[args[1], above, V::I(i32::from(!creative))]);
                let left = api::call(rt, SUPER_WILL_DESTROY, args);
                rt.interned.insert("test:left".into(), left);
                left
            }
            DESTROYED => {
                rt.interned.insert("test:harvest".into(), args[5]);
                if rt.str_of(args[1]) == Some("test:panel[gauges=2]") {
                    let one = api::state(rt, "test:panel[gauges=1]");
                    api::call(rt, SET_BLOCK, &[args[2], args[3], one, V::I(3)]);
                    return V::I(0);
                }
                api::call(rt, SUPER_DESTROYED, args)
            }
            _ => api::call(rt, key, args),
        }
    }

    /// The game's player breaking a mod block: its `playerWillDestroy` first (the test piston takes
    /// the pole above with it, dropped in survival, not in creative; its `super.` call gives the
    /// state back), then its `onDestroyedByPlayer`, told whether the player harvests it, which keeps
    /// the test panel while it has two gauges; its `super.` call leaves the block to the game, which
    /// removes it.
    #[test]
    fn a_mod_block_does_its_part_when_the_player_breaks_it() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(break_overrides);
        rt.vcall = Some(break_code);
        for (class, name, sup) in [(PISTON, "test:piston", "net/minecraft/world/level/block/DirectionalBlock"), (PANEL, "test:panel", "net/minecraft/world/level/block/FaceAttachedHorizontalDirectionalBlock")] {
            rt.supers.insert(class, sup);
            let block = rt.new_object(class);
            rt.registered.insert(("block".into(), name.into()), block);
        }
        let mut host = MemoryHost { player_id: Some(7), ..MemoryHost::default() };
        host.loot.insert("test:pole".into(), vec![("test:pole".into(), 1)]);
        let (at, above) = ([2, 64, 2], [2, 65, 2]);
        let piston = "test:piston[facing=up]";
        let block = |host: &MemoryHost, p: [i32; 3]| host.blocks.get(&p).cloned().unwrap_or_default();
        for creative in [false, true] {
            host.creative = creative;
            host.item_entities.clear();
            host.blocks.insert(at, piston.into());
            host.blocks.insert(above, "test:pole[facing=up]".into());
            assert!(!destroy_block(&mut rt, &mut host, at, piston), "it goes");
            assert_eq!(block(&host, above), "minecraft:air", "the pole with it");
            let dropped: Vec<_> = host.item_entities.iter().map(|e| (e.item.clone(), e.count)).collect();
            assert_eq!(dropped, if creative { vec![] } else { vec![("test:pole".to_owned(), 1)] });
            assert_eq!(block(&host, at), piston, "the game removes it");
            let left = rt.interned.get("test:left").copied().unwrap_or_default();
            assert_eq!(rt.str_of(left), Some(piston), "its super.playerWillDestroy: the state");
        }

        host.creative = false;
        host.blocks.insert(at, "test:panel[gauges=2]".into());
        assert!(destroy_block(&mut rt, &mut host, at, "test:panel[gauges=2]"), "it stays");
        assert_eq!(block(&host, at), "test:panel[gauges=1]", "a gauge less");
        assert_eq!(rt.interned.get("test:harvest").map(|v| v.i()), Some(1), "a survival player harvests it");
        host.creative = true;
        assert!(!destroy_block(&mut rt, &mut host, at, "test:panel[gauges=1]"), "it goes");
        assert_eq!(block(&host, at), "test:panel[gauges=1]", "the game removes it");
        assert_eq!(rt.interned.get("test:harvest").map(|v| v.i()), Some(0), "a creative player never harvests");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
