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
/// even when it has none: the block's template (which gives it random ticks) must not act.
pub fn random_tick(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
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
/// absent: Create's creative motor would lose its default speed).
fn load_saved(rt: &mut Rt, be: V, nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) {
    let Some(vc) = rt.vcall else { return };
    if !nbt.is_empty() {
        let tag = crate::nbt::to_runtime(rt, nbt);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        vc(rt, LOAD, LOAD, &[be, tag, provider]);
        rt.report_thrown("loadAdditional");
    }
}

/// The block entity at `pos` as `Level.getBlockEntity` finds it: the one this runtime holds, or,
/// for a mod block that has one, made now. Vanilla makes it with its block; here one is otherwise
/// made on its first tick or draw, and Create's belt asks for its segments' before they tick (and
/// breaks itself when one is missing). It takes the game's saved data on its first tick.
pub(crate) fn block_entity_at(rt: &mut Rt, pos: [i32; 3]) -> V {
    if let Some(be) = rt.block_entities.get(&pos).copied() {
        return be;
    }
    let Some(state) = rt.host().map(|h| h.block_state(pos)) else { return V::NULL };
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

const USE_ITEM_ON: &str = "useItemOn.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
const USE_WITHOUT_ITEM: &str = "useWithoutItem.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/InteractionResult;";

/// `right_click_block`: the acting player right clicks the mod block at `pos` on face `face` (a
/// direction ordinal), as `ServerPlayerGameMode.useItemOn` asks the block: `useItemOn` with the held
/// stack first, then, when it passes to the default, `useWithoutItem`. True when the block took the
/// click (the game's own handling then stops).
pub fn use_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], state: &str, face: i64) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
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
    rt.with_host(host, |rt| {
        let Some(vc) = rt.vcall else { return false };
        let ctx = crate::api_common::use_on_context(rt, click);
        let r = vc(rt, USE_ON_KEY, USE_ON, &[item, ctx]);
        let thrown = rt.thrown.is_some();
        rt.report_thrown("useOn");
        !thrown && rt.str_of(r).is_some_and(|name| name != "PASS")
    })
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
/// (the update tag the server sent) whenever that changes, as `handleUpdateTag` does in vanilla.
fn client_block_entity(rt: &mut Rt, pos: [i32; 3], state: &str, nbt: &std::collections::BTreeMap<String, crate::nbt::Nbt>) -> Option<V> {
    let (block, _) = crate::host::split_state(state);
    let synced = crate::nbt::hash(nbt) as i64;
    let vc = rt.vcall?;
    let be = match rt.block_entities.get(&pos).copied() {
        Some(be) if rt.block_entity_meta.get(&pos).is_some_and(|(b, _)| b == block) => {
            if rt.get_field(be, "$synced").j() == synced && rt.get_field(be, "$loaded").i() != 0 {
                return Some(be);
            }
            be
        }
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
            be
        }
    };
    // An empty update tag isn't sent (vanilla's chunk data carries none), so nothing is loaded.
    if !nbt.is_empty() {
        let brief = nbt_brief(nbt);
        trace_some(rt, &format!("client-tag/{pos:?}"), &brief, 8, |_| format!("client: {block} at {pos:?} loads an update tag: {brief}"));
        let tag = crate::nbt::to_runtime(rt, nbt);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        vc(rt, HANDLE_UPDATE_TAG_KEY, HANDLE_UPDATE_TAG, &[be, tag, provider]);
        rt.report_thrown("handleUpdateTag");
    }
    rt.put_field(be, "$synced", V::J(synced));
    rt.put_field(be, "$loaded", V::I(1));
    Some(be)
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
    crate::neo::fire_client_setup(rt);
    if mod_block(rt, state).is_none() {
        return false;
    }
    rt.with_host(host, |rt| {
        let Some(be) = client_block_entity(rt, pos, state, nbt) else { return false };
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

/// `native_level_submit` (a client): the client's mod entities, each drawn by the renderer its mod
/// registered for its type (`EntityRenderersEvent.RegisterRenderers`), as vanilla's
/// `EntityRenderDispatcher.render` draws one: when the renderer's `shouldRender` agrees, under a
/// pose stack at its place this frame (between its last two ticks') relative to `camera`, with its
/// turn this frame. The number drawn.
pub fn render_mod_entities(rt: &mut Rt, host: &mut dyn Host, camera: [f64; 3]) -> usize {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    crate::neo::fire_client_setup(rt);
    rt.with_host(host, |rt| {
        let Some(vc) = rt.vcall else { return 0 };
        let partial = rt.host().map_or(0.0, |h| h.partial_tick());
        let mut drawn = 0;
        for e in crate::mod_entities::live(rt) {
            if rt.get_field(e, "removalReason").r() != 0 {
                continue;
            }
            let ty = rt.get_field(e, "$type");
            let Some(id) = rt.registered_name(ty, "entity_type").map(str::to_owned) else { continue };
            let Some(provider) = rt.interned.get(&format!("er/{id}")).copied() else {
                trace_once(rt, &format!("no-er/{id}"), |_| format!("client: {id} has no entity renderer"));
                continue;
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
            if renderer.r() == 0 {
                continue;
            }
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
            drawn += 1;
        }
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
        let class = if post { SERVER_TICK_POST } else { SERVER_TICK_PRE };
        let ev = rt.new_object(class);
        crate::neo::post(rt, class, ev);
    });
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
    crate::neo::fire_client_setup(rt);
    if channel == crate::mod_entities::CHANNEL {
        rt.with_host(host, |rt| crate::mod_entities::receive(rt, data));
        return true;
    }
    rt.with_host(host, |rt| crate::network::receive(rt, channel, data, None))
}

/// A mod payload `player`'s client sent the server (`native_net_from_client`), to its handler.
pub fn payload_from_client(rt: &mut Rt, host: &mut dyn Host, player: u128, channel: &str, data: &[u8]) -> bool {
    rt.safe_point();
    rt.with_host(host, |rt| crate::network::receive(rt, channel, data, Some(player)))
}

/// The client's tick, in `Minecraft.tick`'s order: `ClientTickEvent.Pre` (animation clocks advance
/// here: catnip's `AnimationTickHolder`), the level's block entities, then `ClientTickEvent.Post`.
pub fn client_tick(rt: &mut Rt, host: &mut dyn Host) {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    crate::neo::fire_client_setup(rt);
    rt.with_host(host, |rt| {
        let ev = rt.new_object(CLIENT_TICK_PRE);
        crate::neo::post(rt, CLIENT_TICK_PRE, ev);
        // `ClientLevel.tickEntities`: the entities, then the block entities.
        let dimension = rt.host().map(|h| h.dimension()).unwrap_or_default();
        crate::mod_entities::tick_all(rt, &dimension);
        tick_client_block_entities(rt);
        let ev = rt.new_object(CLIENT_TICK_POST);
        crate::neo::post(rt, CLIENT_TICK_POST, ev);
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
}
