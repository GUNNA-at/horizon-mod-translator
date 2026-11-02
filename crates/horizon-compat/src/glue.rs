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
    let l = rt.new_object("net/minecraft/server/level/ServerLevel");
    rt.put_field(l, "isClientSide", V::I(0));
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
    run(rt, host, state, TICK, |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let r = rt.new_object("net/minecraft/util/RandomSource");
        vec![l, p, r]
    })
}

/// `random_tick`: `BlockBehaviour.randomTick`. A mod block's random tick is always the mod's,
/// even when it has none: the block's template (which gives it random ticks) must not act.
pub fn random_tick(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    let mine = mod_block(rt, state).is_some();
    let ran = run(rt, host, state, RANDOM_TICK, |rt| {
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let r = rt.new_object("net/minecraft/util/RandomSource");
        vec![l, p, r]
    });
    ran || mine
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
    let tag = crate::nbt::to_runtime(rt, nbt);
    let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
    vc(rt, LOAD, LOAD, &[be, tag, provider]);
    rt.report_thrown("loadAdditional");
    rt.block_entities.insert(pos, be);
    Some(be)
}

/// `native_block_entity_tick`: one server tick of the mod's block entity at `pos`, whose saved
/// compound is `nbt` (read before, written after).
pub fn block_entity_tick(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, nbt: &mut std::collections::BTreeMap<String, crate::nbt::Nbt>) -> bool {
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
        let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
        api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        vc(rt, SAVE, SAVE, &[be, tag, provider]);
        rt.report_thrown("saveAdditional");
        *nbt = crate::nbt::from_runtime(rt, tag);
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

const USE_ITEM_ON: &str = "useItemOn.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
const USE_WITHOUT_ITEM: &str = "useWithoutItem.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/InteractionResult;";

/// `right_click_block`: the acting player right clicks the mod block at `pos` on face `face` (a
/// direction ordinal), as `ServerPlayerGameMode.useItemOn` asks the block: `useItemOn` with the held
/// stack first, then, when it passes to the default, `useWithoutItem`. True when the block took the
/// click (the game's own handling then stops).
pub fn use_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, face: i64) -> bool {
    let Some((b, class)) = mod_block(rt, state) else { return false };
    let Some(overrides) = rt.overrides else { return false };
    let (with_item, without) = (overrides(class, USE_ITEM_ON), overrides(class, USE_WITHOUT_ITEM));
    if !with_item && !without {
        return false;
    }
    rt.with_host(host, |rt| {
        let Some(vc) = rt.vcall else { return false };
        let st = api::state(rt, state);
        let l = level(rt);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let player = rt.new_object("net/minecraft/server/level/ServerPlayer");
        let hit = crate::api_common::hit(rt, pos, face);
        let mut took = None;
        if with_item {
            let hand = api::get_static(rt, "net/minecraft/world/InteractionHand.MAIN_HAND");
            let stack = api::call(rt, "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;", &[player, hand]);
            let r = vc(rt, USE_ITEM_ON, USE_ITEM_ON, &[b, stack, st, l, p, player, hand, hit]);
            let name = rt.str_of(r).unwrap_or("PASS_TO_DEFAULT_BLOCK_INTERACTION").to_owned();
            if name != "PASS_TO_DEFAULT_BLOCK_INTERACTION" {
                took = Some(true); // consumed, skipped or failed: the click is the block's
            }
        }
        if took.is_none() && without {
            let r = vc(rt, USE_WITHOUT_ITEM, USE_WITHOUT_ITEM, &[b, st, l, p, player, hit]);
            let name = rt.str_of(r).unwrap_or("PASS").to_owned();
            took = Some(name != "PASS");
        }
        rt.report_thrown("a right click");
        took.unwrap_or(false)
    })
}
