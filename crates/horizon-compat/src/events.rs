//! NeoForge's player interaction events (`PlayerInteractEvent`), posted to a mod's listeners from
//! the game's hooks as NeoForge's `CommonHooks` posts them on a server: a right click on a block
//! ([`right_click_block`], `RightClickBlock`: before the block's own use and the item's) and with
//! an item ([`right_click_item`], `RightClickItem`: before the item's `use`). Create applies its
//! manual recipes this way (andesite alloy on a stripped log makes a casing), sets filters and
//! values on its blocks, and links redstone links.
//!
//! A listener may cancel the event (`setCanceled(true)`): the game's own handling of the click then
//! stops, and the result the listener set (`setCancellationResult`, `PASS` unless set) says whether
//! the click was used. As NeoForge's bus does, listeners after a cancel don't hear the event.
//! `setUseBlock`/`setUseItem` with `FALSE` on both is a cancel too; one alone is kept on the event
//! for the listeners after it.
//!
//! Also a server player's own events (`PlayerEvent`'s), in the order a server posts them: joining
//! ([`logged_in`]: `OnDatapackSyncEvent` for them, then `PlayerLoggedInEvent`), leaving
//! ([`logged_out`]), respawning after a death ([`respawned`]: `Clone`, then `PlayerRespawnEvent`),
//! and the server's data going to every player after `/reload` ([`datapack_sync`]). A mod sends its
//! players what it keeps for them here (Create its train maps, a mod its loaded data).
use crate::host::Host;
use crate::{api, api_common, hand, Rt, V};

const INTERACT: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent";
pub(crate) const RIGHT_CLICK_BLOCK: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$RightClickBlock";
pub(crate) const RIGHT_CLICK_ITEM: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$RightClickItem";
const PLAYER_EVENT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent";
const LIVING_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent";
const ENTITY_EVENT: &str = "net/neoforged/neoforge/event/entity/EntityEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
const CANCELLABLE: &str = "net/neoforged/bus/api/ICancellableEvent";
const TRI_STATE: &str = "net/neoforged/neoforge/common/util/TriState";
const LOGICAL_SIDE: &str = "net/neoforged/fml/LogicalSide";
const RESULT: &str = "net/minecraft/world/InteractionResult";
pub(crate) const LOGGED_IN: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent$PlayerLoggedInEvent";
pub(crate) const LOGGED_OUT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent$PlayerLoggedOutEvent";
pub(crate) const RESPAWN: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent$PlayerRespawnEvent";
pub(crate) const CLONE: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent$Clone";
pub(crate) const DATAPACK_SYNC: &str = "net/neoforged/neoforge/event/OnDatapackSyncEvent";
const PLAYER_LIST: &str = "net/minecraft/server/players/PlayerList";

/// The events' classes above them, so a listener for `PlayerInteractEvent` (or `PlayerEvent`, or
/// any event) hears them.
const SUPERS: [(&str, &str); 16] = [
    (RIGHT_CLICK_BLOCK, INTERACT),
    (RIGHT_CLICK_ITEM, INTERACT),
    ("net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$LeftClickBlock", INTERACT),
    ("net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$EntityInteract", INTERACT),
    ("net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$EntityInteractSpecific", INTERACT),
    ("net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$RightClickEmpty", INTERACT),
    ("net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$LeftClickEmpty", INTERACT),
    (INTERACT, PLAYER_EVENT),
    (LOGGED_IN, PLAYER_EVENT),
    (LOGGED_OUT, PLAYER_EVENT),
    (RESPAWN, PLAYER_EVENT),
    (CLONE, PLAYER_EVENT),
    (DATAPACK_SYNC, EVENT),
    (PLAYER_EVENT, LIVING_EVENT),
    (LIVING_EVENT, ENTITY_EVENT),
    (ENTITY_EVENT, EVENT),
];

fn know_classes(rt: &mut Rt) {
    if !rt.supers.contains_key(RIGHT_CLICK_BLOCK) {
        for (class, sup) in SUPERS {
            rt.supers.insert(class, sup);
        }
    }
}

/// `TriState`'s constant `name` (`TRUE`, `DEFAULT`, `FALSE`).
fn tri_state(rt: &mut Rt, name: &'static str) -> V {
    let ordinal = match name {
        "TRUE" => 0,
        "DEFAULT" => 1,
        _ => 2,
    };
    api::api_enum(rt, TRI_STATE, name, ordinal)
}

fn tri_state_name(rt: &Rt, v: V) -> String {
    if v.r() == 0 {
        return "DEFAULT".into();
    }
    rt.str_of(rt.get_field(v, "$name")).unwrap_or("DEFAULT").to_owned()
}

/// A new event of `class` for the acting player using `hand`.
fn interaction(rt: &mut Rt, class: &'static str, hand: u8) -> V {
    know_classes(rt);
    let ev = rt.new_object(class);
    let player = api_common::acting_player(rt);
    let level = crate::glue::level(rt);
    let constant = hand::constant(rt, hand);
    let default = tri_state(rt, "DEFAULT");
    let pass = api_common::result(rt, RESULT, "PASS");
    for (field, v) in [
        ("$player", player),
        ("$level", level),
        ("$hand", constant),
        ("$handOrdinal", V::I(i32::from(hand))),
        ("$canceled", V::I(0)),
        ("$result", pass),
        ("$useBlock", default),
        ("$useItem", default),
    ] {
        rt.put_field(ev, field, v);
    }
    ev
}

/// Posts `ev` to the listeners: Some(the click was used) when one canceled it, None when the
/// game's own handling goes on.
fn post(rt: &mut Rt, class: &str, ev: V) -> Option<bool> {
    crate::neo::post(rt, class, ev);
    let both_off = tri_state_name(rt, rt.get_field(ev, "$useBlock")) == "FALSE" && tri_state_name(rt, rt.get_field(ev, "$useItem")) == "FALSE";
    if rt.get_field(ev, "$canceled").i() == 0 && !both_off {
        return None;
    }
    let result = rt.get_field(ev, "$result");
    let name = rt.str_of(result).unwrap_or("PASS").to_owned();
    Some(matches!(name.as_str(), "SUCCESS" | "CONSUME" | "SUCCESS_NO_ITEM_USED" | "CONSUME_PARTIAL"))
}

/// `PlayerInteractEvent.RightClickBlock`: the acting player right-clicks the block at `pos` on
/// `face` (a direction ordinal), the click landing at `hit`, with `hand`. Some(used) when a
/// listener canceled it (the block's own use and the item's don't run), None when it goes on.
pub fn right_click_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], face: i64, hit: [f64; 3], hand: u8) -> Option<bool> {
    rt.safe_point();
    if !listened(rt, RIGHT_CLICK_BLOCK) {
        return None;
    }
    rt.with_host(host, |rt| {
        hand::with_hand(rt, |rt| {
            let ev = interaction(rt, RIGHT_CLICK_BLOCK, hand);
            let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
            let direction = api_common::direction(rt, face);
            let hit_result = api_common::hit(rt, pos, face);
            let at = api_common::vec3(rt, hit[0], hit[1], hit[2]);
            rt.put_field(hit_result, "$loc", at);
            rt.put_field(ev, "$pos", p);
            rt.put_field(ev, "$face", direction);
            rt.put_field(ev, "$hit", hit_result);
            post(rt, RIGHT_CLICK_BLOCK, ev)
        })
    })
}

/// `PlayerInteractEvent.RightClickBlock` on a client (`MultiPlayerGameMode.performUseItemOn`):
/// the client's own player uses `hand` on the block at `pos` its crosshair is on, before the use
/// goes to the server, which posts its own. Create starts holding a value setting here. Some(used)
/// when a listener canceled it.
pub fn client_right_click_block(rt: &mut Rt, host: &mut dyn Host, pos: [i32; 3], face: i64, hit: [f64; 3], hand: u8) -> Option<bool> {
    rt.safe_point();
    if !listened(rt, RIGHT_CLICK_BLOCK) {
        return None;
    }
    rt.with_host(host, |rt| {
        hand::with_hand(rt, |rt| {
            let ev = interaction(rt, RIGHT_CLICK_BLOCK, hand);
            let player = crate::network::local_player(rt);
            rt.put_field(ev, "$player", player);
            rt.put_field(ev, "$client", V::I(1));
            let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
            let direction = api_common::direction(rt, face);
            let hit_result = api_common::hit(rt, pos, face);
            let at = api_common::vec3(rt, hit[0], hit[1], hit[2]);
            rt.put_field(hit_result, "$loc", at);
            rt.put_field(ev, "$pos", p);
            rt.put_field(ev, "$face", direction);
            rt.put_field(ev, "$hit", hit_result);
            post(rt, RIGHT_CLICK_BLOCK, ev)
        })
    })
}

/// `PlayerInteractEvent.RightClickItem`: the acting player uses the item in `hand` (in the air, or
/// on a block that let the click through). As [`right_click_block`].
pub fn right_click_item(rt: &mut Rt, host: &mut dyn Host, hand: u8) -> Option<bool> {
    rt.safe_point();
    if !listened(rt, RIGHT_CLICK_ITEM) {
        return None;
    }
    rt.with_host(host, |rt| {
        hand::with_hand(rt, |rt| {
            let ev = interaction(rt, RIGHT_CLICK_ITEM, hand);
            // Where the player stands (`player.blockPosition()`), as the game told it.
            let [x, y, z] = rt.host().and_then(|h| h.player_place()).map_or([0.0; 3], |(p, _, _)| p);
            let at = api::block_pos(rt, x.floor() as i64, y.floor() as i64, z.floor() as i64);
            rt.put_field(ev, "$pos", at);
            rt.put_field(ev, "$face", V::NULL);
            post(rt, RIGHT_CLICK_ITEM, ev)
        })
    })
}

/// `player` (a server's player, by UUID) joined (`PlayerList.placeNewPlayer`): the server's data
/// goes to them first (`OnDatapackSyncEvent` for them alone), then `PlayerLoggedInEvent`.
pub fn logged_in(rt: &mut Rt, player: u128, name: &str) {
    named(rt, player, name);
    datapack_sync(rt, Some(player));
    player_event(rt, LOGGED_IN, player, |_, _| {});
}

/// `player` left, by any path (`PlayerLoggedOutEvent`).
pub fn logged_out(rt: &mut Rt, player: u128, name: &str) {
    named(rt, player, name);
    player_event(rt, LOGGED_OUT, player, |_, _| {});
}

/// `player` respawned after dying (`PlayerList.respawn`): `PlayerEvent.Clone` (`ServerPlayer.restoreFrom`),
/// where a mod copies what it keeps on the old player to the new one, here one object, so what it
/// kept stays; then `PlayerRespawnEvent`, not from the End's exit.
pub fn respawned(rt: &mut Rt, player: u128, name: &str) {
    named(rt, player, name);
    player_event(rt, CLONE, player, |rt, ev| {
        let p = rt.get_field(ev, "$player");
        rt.put_field(ev, "$original", p);
        rt.put_field(ev, "$wasDeath", V::I(1));
    });
    player_event(rt, RESPAWN, player, |rt, ev| rt.put_field(ev, "$endConquered", V::I(0)));
}

/// The server's data goes to `player`, or to every player after `/reload` (`OnDatapackSyncEvent`,
/// from `PlayerList.placeNewPlayer` and `reloadResources`).
pub fn datapack_sync(rt: &mut Rt, player: Option<u128>) {
    if !listened(rt, DATAPACK_SYNC) {
        return;
    }
    let ev = rt.new_object(DATAPACK_SYNC);
    let p = player.map_or(V::NULL, |u| known_player(rt, u));
    rt.put_field(ev, "$player", p);
    crate::neo::post(rt, DATAPACK_SYNC, ev);
}

/// A player's own event of `class` for the server's player `player`, `fill` setting the rest,
/// posted when anyone listens.
fn player_event(rt: &mut Rt, class: &'static str, player: u128, fill: impl FnOnce(&mut Rt, V)) {
    if !listened(rt, class) {
        return;
    }
    let ev = rt.new_object(class);
    let p = known_player(rt, player);
    rt.put_field(ev, "$player", p);
    fill(rt, ev);
    crate::neo::post(rt, class, ev);
}

fn player_lifecycle(class: &str) -> bool {
    matches!(class, LOGGED_IN | LOGGED_OUT | RESPAWN | CLONE | DATAPACK_SYNC)
}

/// The server's player list (`MinecraftServer.getPlayerList`): its players are the host's.
pub(crate) fn player_list(rt: &mut Rt) -> V {
    rt.intern("compat:player-list".into(), |rt| rt.new_object(PLAYER_LIST))
}

/// The server's players, as their objects (`PlayerList.getPlayers`).
fn players(rt: &mut Rt) -> Vec<V> {
    let ids: Vec<u128> = rt.host().map(|h| h.players().into_iter().map(|(id, _)| id).collect()).unwrap_or_default();
    ids.into_iter().map(|id| known_player(rt, id)).collect()
}

/// The server's player `uuid` as its object, standing where the host says it stands (so
/// `blockPosition` and the like answer for it).
fn known_player(rt: &mut Rt, uuid: u128) -> V {
    let p = crate::menus::server_player(rt, uuid);
    if let Some((_, at)) = rt.host().and_then(|h| h.players().into_iter().find(|(id, _)| *id == uuid)) {
        for (field, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
            rt.put_field(p, field, V::D(v));
        }
    }
    p
}

/// The name the game gave `uuid`'s player, kept on their object (`getName`, `getGameProfile`).
fn named(rt: &mut Rt, uuid: u128, name: &str) {
    let p = crate::menus::server_player(rt, uuid);
    let n = rt.string(name);
    rt.put_field(p, "$name", n);
}

/// Whether any listener would hear an event of `class` (so a click nobody listens to costs
/// nothing).
fn listened(rt: &mut Rt, class: &str) -> bool {
    know_classes(rt);
    !crate::neo::hearing(rt, class).is_empty()
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner == TRI_STATE
        || owner.starts_with(INTERACT)
        || owner.starts_with(PLAYER_EVENT)
        || matches!(owner, LIVING_EVENT | ENTITY_EVENT | EVENT | CANCELLABLE | DATAPACK_SYNC | PLAYER_LIST)
}

/// The events' own calls, on the event or on its classes above (`event.getLevel()` compiles to
/// `PlayerInteractEvent$RightClickBlock.getLevel`, `PlayerEvent.getEntity`...); `TriState`'s.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == TRI_STATE {
        let n = tri_state_name(rt, a(0));
        return Some(match name {
            "isTrue" => V::I((n == "TRUE") as i32),
            "isFalse" => V::I((n == "FALSE") as i32),
            "isDefault" => V::I((n == "DEFAULT") as i32),
            "isAvailable" | "isNotFalse" => V::I((n != "FALSE") as i32),
            _ => return None,
        });
    }
    if owner == PLAYER_LIST {
        return Some(match name {
            "getPlayers" => {
                let all = players(rt);
                crate::jutil::new_list(rt, all)
            }
            "getPlayerCount" => V::I(players(rt).len() as i32),
            "getPlayer" if desc.starts_with("(Ljava/util/UUID;)") => {
                let (msb, lsb) = crate::tags::uuid_bits(rt, a(1));
                let id = (u128::from(msb as u64) << 64) | u128::from(lsb as u64);
                let here = rt.host().is_some_and(|h| h.players().iter().any(|(p, _)| *p == id));
                if here { crate::menus::server_player(rt, id) } else { V::NULL }
            }
            _ => return None,
        });
    }
    if player_lifecycle(rt.class_of(a(0))) && (owner.starts_with(PLAYER_EVENT) || matches!(owner, LIVING_EVENT | ENTITY_EVENT | EVENT | DATAPACK_SYNC)) {
        let ev = a(0);
        return Some(match name {
            "getEntity" | "getPlayer" => rt.get_field(ev, "$player"),
            "getOriginal" => rt.get_field(ev, "$original"),
            "isWasDeath" => rt.get_field(ev, "$wasDeath"),
            "isEndConquered" => rt.get_field(ev, "$endConquered"),
            "getPlayerList" => player_list(rt),
            "getRelevantPlayers" => {
                let p = rt.get_field(ev, "$player");
                let items = if p.r() == 0 { players(rt) } else { vec![p] };
                rt.alloc("java/util/stream/Stream", crate::Data::Stream(items))
            }
            "isCanceled" => V::I(0),
            _ => return None,
        });
    }
    let ours = owner.starts_with(INTERACT) || matches!(owner, PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT | EVENT | CANCELLABLE);
    if !ours || !rt.class_of(a(0)).starts_with(INTERACT) {
        return None;
    }
    let ev = a(0);
    Some(match name {
        "getEntity" => rt.get_field(ev, "$player"),
        "getLevel" => rt.get_field(ev, "$level"),
        "getPos" => rt.get_field(ev, "$pos"),
        "getHand" => rt.get_field(ev, "$hand"),
        "getFace" => rt.get_field(ev, "$face"),
        "getHitVec" => rt.get_field(ev, "$hit"),
        "getItemStack" => {
            let h = rt.get_field(ev, "$handOrdinal").i() as u8;
            hand::stack(rt, h)
        }
        "getSide" => match rt.get_field(ev, "$client").i() {
            0 => api::api_enum(rt, LOGICAL_SIDE, "SERVER", 1),
            _ => api::api_enum(rt, LOGICAL_SIDE, "CLIENT", 0),
        },
        "setCanceled" => {
            rt.put_field(ev, "$canceled", V::I(a(1).i()));
            V::Void
        }
        "isCanceled" => rt.get_field(ev, "$canceled"),
        "setCancellationResult" => {
            rt.put_field(ev, "$result", a(1));
            V::Void
        }
        "getCancellationResult" => rt.get_field(ev, "$result"),
        "setUseBlock" | "setUseItem" => {
            let field = if name == "setUseBlock" { "$useBlock" } else { "$useItem" };
            rt.put_field(ev, field, a(1));
            V::Void
        }
        "getUseBlock" => rt.get_field(ev, "$useBlock"),
        "getUseItem" => rt.get_field(ev, "$useItem"),
        _ => return None,
    })
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (TRI_STATE, "TRUE") => Some(tri_state(rt, "TRUE")),
        (TRI_STATE, "DEFAULT") => Some(tri_state(rt, "DEFAULT")),
        (TRI_STATE, "FALSE") => Some(tri_state(rt, "FALSE")),
        (LOGICAL_SIDE, "CLIENT") => Some(api::api_enum(rt, LOGICAL_SIDE, "CLIENT", 0)),
        (LOGICAL_SIDE, "SERVER") => Some(api::api_enum(rt, LOGICAL_SIDE, "SERVER", 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;
    use std::cell::RefCell;

    const ITEM_STACK: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent.getItemStack.()Lnet/minecraft/world/item/ItemStack;";
    const GET_ITEM: &str = "net/minecraft/world/item/ItemStack.getItem.()Lnet/minecraft/world/item/Item;";
    const SHRINK: &str = "net/minecraft/world/item/ItemStack.shrink.(I)V";
    const GET_POS: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent.getPos.()Lnet/minecraft/core/BlockPos;";
    const GET_X: &str = "net/minecraft/core/Vec3i.getX.()I";
    const SET_CANCELED: &str = "net/neoforged/bus/api/ICancellableEvent.setCanceled.(Z)V";
    const IS_CANCELED: &str = "net/neoforged/bus/api/ICancellableEvent.isCanceled.()Z";
    const SET_RESULT: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$RightClickBlock.setCancellationResult.(Lnet/minecraft/world/InteractionResult;)V";
    const SET_USE_BLOCK: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$RightClickBlock.setUseBlock.(Lnet/neoforged/neoforge/common/util/TriState;)V";
    const SET_USE_ITEM: &str = "net/neoforged/neoforge/event/entity/player/PlayerInteractEvent$RightClickBlock.setUseItem.(Lnet/neoforged/neoforge/common/util/TriState;)V";

    thread_local! {
        /// What the listeners heard, in order: "listener:event class:x of the clicked block".
        static HEARD: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn heard() -> Vec<String> {
        HEARD.with(|h| std::mem::take(&mut *h.borrow_mut()))
    }

    /// Listener 1 hears every `PlayerInteractEvent`. Listener 2 is a manual recipe as Create's: a
    /// stick on a block is used up and the click canceled as used; a barrier denies the block's use
    /// and the item's without canceling. Listener 3 only records, after 2.
    fn listener(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        let class = rt.class_of(ev).rsplit('$').next().unwrap_or_default().to_owned();
        let pos = api::call(rt, GET_POS, &[ev]);
        let x = api::call(rt, GET_X, &[pos]).i();
        HEARD.with(|h| h.borrow_mut().push(format!("{id}:{class}:{x}")));
        let stack = api::call(rt, ITEM_STACK, &[ev]);
        let item = api::call(rt, GET_ITEM, &[stack]);
        match (id, api_common::item_like_name(rt, item).as_str()) {
            (2, "minecraft:stick") if api::call(rt, IS_CANCELED, &[ev]).i() == 0 => {
                api::call(rt, SHRINK, &[stack, V::I(1)]);
                let success = api_common::result(rt, RESULT, "SUCCESS");
                api::call(rt, SET_RESULT, &[ev, success]);
                api::call(rt, SET_CANCELED, &[ev, V::I(1)]);
            }
            (2, "minecraft:barrier") => {
                let no = api::get_static(rt, "net/neoforged/neoforge/common/util/TriState.FALSE");
                api::call(rt, SET_USE_BLOCK, &[ev, no]);
                api::call(rt, SET_USE_ITEM, &[ev, no]);
            }
            _ => {}
        }
        Some(V::Void)
    }

    /// A right click on a block reaches the listeners of its class and of the classes above, in
    /// order, until one cancels it: a canceled click is used (the game's own handling doesn't run),
    /// what the listener did to the stack in hand reaches the game, and the listeners after it
    /// don't hear it. A click nobody cancels goes on; one denied both uses is refused.
    #[test]
    fn a_right_click_reaches_listeners_until_one_cancels_it() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(listener);
        for (id, class) in [(1, INTERACT), (2, RIGHT_CLICK_BLOCK), (3, RIGHT_CLICK_BLOCK)] {
            let l = rt.lambda_id("java/util/function/Consumer", id, Vec::new());
            rt.listeners.push((class.to_owned(), l));
        }
        let mut host = MemoryHost::default();
        host.blocks.insert([7, 64, 0], "minecraft:stone".into());
        let click = |rt: &mut Rt, host: &mut MemoryHost, held: &str| {
            host.held = Some((held.to_owned(), 3));
            right_click_block(rt, host, [7, 64, 0], 1, [7.5, 65.0, 0.5], hand::MAIN)
        };
        assert_eq!(click(&mut rt, &mut host, "minecraft:stick"), Some(true));
        assert_eq!(host.held, Some(("minecraft:stick".to_owned(), 2)), "the listener's shrink reached the game");
        assert_eq!(heard(), ["1:RightClickBlock:7", "2:RightClickBlock:7"], "listener 3 doesn't hear a canceled click");
        assert_eq!(click(&mut rt, &mut host, "minecraft:dirt"), None);
        assert_eq!(heard(), ["1:RightClickBlock:7", "2:RightClickBlock:7", "3:RightClickBlock:7"]);
        assert_eq!(click(&mut rt, &mut host, "minecraft:barrier"), Some(false), "both uses denied: refused");
        assert_eq!(heard().len(), 3);
        // Using an item: only the listener of every interaction hears it, at the player's place.
        host.held = Some(("minecraft:stick".to_owned(), 3));
        host.place = Some(([-3.5, 70.0, 2.5], 0.0, 0.0));
        assert_eq!(right_click_item(&mut rt, &mut host, hand::MAIN), None);
        assert_eq!(heard(), ["1:RightClickItem:-4"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a listener of a player's own events heard: the event, the player's UUID, and the rest.
    fn lifecycle_listener(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        let class = rt.class_of(ev).rsplit(['/', '$']).next().unwrap_or_default().to_owned();
        let uuid = |rt: &Rt, p: V| crate::network::player_uuid(rt, p).map_or("null".to_owned(), |u| format!("{u:x}"));
        let line = match class.as_str() {
            "OnDatapackSyncEvent" => {
                let one = api::call(rt, "net/neoforged/neoforge/event/OnDatapackSyncEvent.getPlayer.()Lnet/minecraft/server/level/ServerPlayer;", &[ev]);
                let stream = api::call(rt, "net/neoforged/neoforge/event/OnDatapackSyncEvent.getRelevantPlayers.()Ljava/util/stream/Stream;", &[ev]);
                let list = api::call(rt, "java/util/stream/Stream.toList.()Ljava/util/List;", &[stream]);
                let to: Vec<String> = crate::jutil::elements(rt, list).into_iter().map(|p| uuid(rt, p)).collect();
                format!("sync {} to {}", uuid(rt, one), to.join(","))
            }
            "Clone" => {
                let p = api::call(rt, "net/neoforged/neoforge/event/entity/player/PlayerEvent$Clone.getEntity.()Lnet/minecraft/world/entity/player/Player;", &[ev]);
                let old = api::call(rt, "net/neoforged/neoforge/event/entity/player/PlayerEvent$Clone.getOriginal.()Lnet/minecraft/world/entity/player/Player;", &[ev]);
                let death = api::call(rt, "net/neoforged/neoforge/event/entity/player/PlayerEvent$Clone.isWasDeath.()Z", &[ev]).i();
                format!("clone {} from {} death {death}", uuid(rt, p), uuid(rt, old))
            }
            "PlayerRespawnEvent" => {
                let p = api::call(rt, "net/neoforged/neoforge/event/entity/player/PlayerEvent$PlayerRespawnEvent.getEntity.()Lnet/minecraft/world/entity/player/Player;", &[ev]);
                let end = api::call(rt, "net/neoforged/neoforge/event/entity/player/PlayerEvent$PlayerRespawnEvent.isEndConquered.()Z", &[ev]).i();
                format!("respawn {} end {end}", uuid(rt, p))
            }
            _ => {
                // `getEntity` as `PlayerEvent` compiles it for a listener of the class above; who
                // they are and where they stand, as a mod greeting them reads it.
                let p = api::call(rt, "net/neoforged/neoforge/event/entity/player/PlayerEvent.getEntity.()Lnet/minecraft/world/entity/player/Player;", &[ev]);
                let profile = api::call(rt, "net/minecraft/server/level/ServerPlayer.getGameProfile.()Lcom/mojang/authlib/GameProfile;", &[p]);
                let name = api::call(rt, "com/mojang/authlib/GameProfile.getName.()Ljava/lang/String;", &[profile]);
                let name = rt.str_of(name).unwrap_or("null").to_owned();
                let id = api::call(rt, "net/minecraft/server/level/ServerPlayer.getStringUUID.()Ljava/lang/String;", &[p]);
                let id = rt.str_of(id).unwrap_or("null").to_owned();
                let at = api::call(rt, "net/minecraft/server/level/ServerPlayer.blockPosition.()Lnet/minecraft/core/BlockPos;", &[p]);
                format!("{class} {name} {id} at {:?}", api::pos_of(rt, at))
            }
        };
        HEARD.with(|h| h.borrow_mut().push(line));
        Some(V::Void)
    }

    /// A player's own events, in the order a server posts them: joining sends the server's data to
    /// the new player alone, then `PlayerLoggedInEvent`; a respawn after a death is a `Clone` (of
    /// the same player object, so what a mod keeps on it stays), then `PlayerRespawnEvent`; leaving
    /// is `PlayerLoggedOutEvent`. After `/reload` the data goes to every player. A listener of
    /// `PlayerEvent` hears each of the player's events; the server's player list lists them.
    #[test]
    fn a_players_join_respawn_and_leave_reach_listeners_in_a_servers_order() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lifecycle_listener);
        for class in [DATAPACK_SYNC, PLAYER_EVENT] {
            let l = rt.lambda_id("java/util/function/Consumer", 1, Vec::new());
            rt.listeners.push((class.to_owned(), l));
        }
        let mut host = MemoryHost::default();
        host.players = vec![(0xa, [10.5, 64.0, -3.2]), (0xb, [1.0; 3])];
        rt.with_host(&mut host, |rt| {
            logged_in(rt, 0xa, "Alex");
            respawned(rt, 0xa, "Alex");
            datapack_sync(rt, None);
            logged_out(rt, 0xb, "Steve");
            let list = api::call(rt, "net/minecraft/server/MinecraftServer.getPlayerList.()Lnet/minecraft/server/players/PlayerList;", &[V::NULL]);
            let players = api::call(rt, "net/minecraft/server/players/PlayerList.getPlayers.()Ljava/util/List;", &[list]);
            assert_eq!(crate::jutil::elements(rt, players).len(), 2);
            let b = crate::tags::uuid(rt, 0, 0xb);
            let found = api::call(rt, "net/minecraft/server/players/PlayerList.getPlayer.(Ljava/util/UUID;)Lnet/minecraft/server/level/ServerPlayer;", &[list, b]);
            assert_eq!(found, crate::menus::server_player(rt, 0xb));
        });
        assert_eq!(
            heard(),
            [
                "sync a to a",
                "PlayerLoggedInEvent Alex 00000000-0000-0000-0000-00000000000a at [10, 64, -4]",
                "clone a from a death 1",
                "respawn a end 0",
                "sync null to a,b",
                "PlayerLoggedOutEvent Steve 00000000-0000-0000-0000-00000000000b at [1, 1, 1]",
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Nobody listening: no event is made.
    #[test]
    fn a_click_nobody_listens_to_is_not_posted() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.held = Some(("minecraft:stick".to_owned(), 3));
        assert_eq!(right_click_block(&mut rt, &mut host, [0, 64, 0], 1, [0.5, 65.0, 0.5], hand::MAIN), None);
        assert_eq!(host.held, Some(("minecraft:stick".to_owned(), 3)));
    }
}
