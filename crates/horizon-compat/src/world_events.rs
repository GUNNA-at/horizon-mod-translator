//! NeoForge 1.21.1's events of a world in play, posted from the game's hooks where NeoForge posts
//! them:
//!
//! - A player breaking a block ([`block_break`], `BlockEvent.BreakEvent`): `CommonHooks.fireBlockBreak`
//!   at the start of `ServerPlayerGameMode.destroyBlock`, once the game allowed the break. A
//!   listener that cancels it keeps the block: nothing is destroyed or dropped.
//! - A player placing a block ([`block_place`], `BlockEvent.EntityPlaceEvent`): from
//!   `CommonHooks.onPlaceItemIntoWorld` once the item set the block, before the stack shrinks. The
//!   game tells it too late to take the block back, so a cancel isn't heard here.
//! - A player's tick ([`player_tick`] on a server, [`client_player_tick`] for the client's own
//!   player): `PlayerTickEvent.Pre`, then `.Post`. The game tells only the end of a server player's
//!   tick, so both come there.
//! - The client joining and leaving a server ([`client_level_load`], [`client_level_unload`]):
//!   `ClientPlayerNetworkEvent.LoggingIn` and `.LoggingOut`, and `.Clone` when a change of dimension
//!   makes the client's player anew. Leaving, the client's level unloads (`LevelEvent.Unload`), so
//!   the next server's is a new one.
//! - A mod's entity joining and leaving a level, on either side ([`entity_joins`],
//!   [`entity_leaves`]): `EntityJoinLevelEvent`, which a listener cancels to keep it out, and
//!   `EntityLeaveLevelEvent` (`onTrackingEnd`). Create keeps its contraptions' list for collisions
//!   from them.
//!
//! Create mirrors breaks and placements with its wand of symmetry, wears its extendo grip down on
//! them, glues a block placed with super glue in the off hand, pulses its haunted bell from a
//! player's tick, and drops its client's train data on leaving. Mods make the block events too
//! (`new BreakEvent(...)`: an area tool asks for each block of its area whether it may break), and
//! keep a block to put back as a `BlockSnapshot`.

use crate::{api, api_common, Rt, V};

const EVENT: &str = "net/neoforged/bus/api/Event";
const CANCELLABLE: &str = "net/neoforged/bus/api/ICancellableEvent";
const BLOCK_EVENT: &str = "net/neoforged/neoforge/event/level/BlockEvent";
pub(crate) const BREAK: &str = "net/neoforged/neoforge/event/level/BlockEvent$BreakEvent";
pub(crate) const PLACE: &str = "net/neoforged/neoforge/event/level/BlockEvent$EntityPlaceEvent";
const MULTI_PLACE: &str = "net/neoforged/neoforge/event/level/BlockEvent$EntityMultiPlaceEvent";
const SNAPSHOT: &str = "net/neoforged/neoforge/common/util/BlockSnapshot";
const PLAYER_EVENT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent";
const LIVING_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent";
const ENTITY_EVENT: &str = "net/neoforged/neoforge/event/entity/EntityEvent";
const PLAYER_TICK: &str = "net/neoforged/neoforge/event/tick/PlayerTickEvent";
pub(crate) const PLAYER_TICK_PRE: &str = "net/neoforged/neoforge/event/tick/PlayerTickEvent$Pre";
pub(crate) const PLAYER_TICK_POST: &str = "net/neoforged/neoforge/event/tick/PlayerTickEvent$Post";
const NETWORK_EVENT: &str = "net/neoforged/neoforge/client/event/ClientPlayerNetworkEvent";
pub(crate) const LOGGING_IN: &str = "net/neoforged/neoforge/client/event/ClientPlayerNetworkEvent$LoggingIn";
pub(crate) const LOGGING_OUT: &str = "net/neoforged/neoforge/client/event/ClientPlayerNetworkEvent$LoggingOut";
pub(crate) const CLONE: &str = "net/neoforged/neoforge/client/event/ClientPlayerNetworkEvent$Clone";
const LEVEL_UNLOAD: &str = "net/neoforged/neoforge/event/level/LevelEvent$Unload";
pub(crate) const ENTITY_JOIN: &str = "net/neoforged/neoforge/event/entity/EntityJoinLevelEvent";
pub(crate) const ENTITY_LEAVE: &str = "net/neoforged/neoforge/event/entity/EntityLeaveLevelEvent";
const PLAYER: &str = "net/minecraft/world/entity/player/Player";
const GAME_MODE: &str = "net/minecraft/client/multiplayer/MultiPlayerGameMode";
const CONNECTION: &str = "net/minecraft/network/Connection";
/// How many levels the client holds (one, or two while a change of dimension swaps them).
const CLIENT_LEVELS: &str = "compat:client-levels";
/// The acting player's last right click on a block: where, which face, and the states there and
/// beside it then (what a placement it leads to replaced).
const LAST_CLICK: &str = "compat:last-click";

/// The events' classes above them, so a listener for `BlockEvent` (or `PlayerEvent`, or any
/// event) hears them.
const SUPERS: [(&str, &str); 16] = [
    (ENTITY_JOIN, ENTITY_EVENT),
    (ENTITY_LEAVE, ENTITY_EVENT),
    (BREAK, BLOCK_EVENT),
    (PLACE, BLOCK_EVENT),
    (MULTI_PLACE, PLACE),
    (BLOCK_EVENT, EVENT),
    (PLAYER_TICK_PRE, PLAYER_TICK),
    (PLAYER_TICK_POST, PLAYER_TICK),
    (PLAYER_TICK, PLAYER_EVENT),
    (PLAYER_EVENT, LIVING_EVENT),
    (LIVING_EVENT, ENTITY_EVENT),
    (ENTITY_EVENT, EVENT),
    (LOGGING_IN, NETWORK_EVENT),
    (LOGGING_OUT, NETWORK_EVENT),
    (CLONE, NETWORK_EVENT),
    (NETWORK_EVENT, EVENT),
];

fn know_classes(rt: &mut Rt) {
    if rt.supers.get(BREAK) != Some(&BLOCK_EVENT) || rt.supers.get(LOGGING_IN) != Some(&NETWORK_EVENT) {
        for (class, sup) in SUPERS {
            rt.supers.insert(class, sup);
        }
    }
}

/// Whether any listener would hear an event of `class`.
fn listened(rt: &mut Rt, class: &str) -> bool {
    know_classes(rt);
    !crate::neo::hearing(rt, class).is_empty()
}

/// Whether any mod listens to placements (so a right click keeps what they ask of it).
pub fn places_heard(rt: &mut Rt) -> bool {
    listened(rt, PLACE)
}

/// Whether the client is in a world: from its level's load to its unload. Before the game first
/// told of one, it is taken to be (the level is made where the mods first reach for it).
pub fn client_in_world(rt: &Rt) -> bool {
    rt.interned.get(CLIENT_LEVELS).is_none_or(|v| v.i() > 0)
}

fn put_all(rt: &mut Rt, ev: V, fields: &[(&'static str, V)]) {
    for &(field, v) in fields {
        rt.put_field(ev, field, v);
    }
}

fn pos(rt: &mut Rt, p: [i32; 3]) -> V {
    api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]))
}

fn state_at(rt: &mut Rt, p: [i32; 3]) -> V {
    let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
    api::state(rt, &text)
}

/// Direction's 3D data value as a step: down, up, north, south, west, east.
fn step(face: i32) -> [i32; 3] {
    match face {
        0 => [0, -1, 0],
        1 => [0, 1, 0],
        2 => [0, 0, -1],
        3 => [0, 0, 1],
        4 => [-1, 0, 0],
        _ => [1, 0, 0],
    }
}

fn plus(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn minus(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A `BlockSnapshot` of the block `state` at `p` (`BlockSnapshot.create`, update flags 3).
fn snapshot(rt: &mut Rt, p: [i32; 3], state: V) -> V {
    let s = rt.new_object(SNAPSHOT);
    let level = crate::glue::level(rt);
    let at = pos(rt, p);
    put_all(rt, s, &[("$level", level), ("$pos", at), ("$state", state), ("$flags", V::I(3))]);
    s
}

// ---- breaking and placing ----

/// `BlockEvent.BreakEvent`: the acting player breaks the block `state` (its text) at `p`, the
/// game having allowed it. Whether a listener canceled it: the block then stays.
pub fn block_break(rt: &mut Rt, p: [i32; 3], state: &str) -> bool {
    if !listened(rt, BREAK) {
        return false;
    }
    let ev = rt.new_object(BREAK);
    let level = crate::glue::level(rt);
    let at = pos(rt, p);
    let st = api::state(rt, state);
    let player = api_common::acting_player(rt);
    put_all(rt, ev, &[("$level", level), ("$pos", at), ("$state", st), ("$player", player), ("$canceled", V::I(0))]);
    crate::neo::post(rt, BREAK, ev);
    rt.get_field(ev, "$canceled").i() != 0
}

/// Keeps the acting player's right click on the block at `p`, on face `face` (a direction
/// ordinal), with the states there and beside it: a placement it leads to was placed against
/// the block behind the face, and replaced one of those.
pub fn clicked(rt: &mut Rt, p: [i32; 3], face: i64) {
    let click = rt.intern(LAST_CLICK.into(), |rt| rt.new_object(LAST_CLICK));
    let face = face.clamp(0, 5) as i32;
    let here = state_at(rt, p);
    let beside = state_at(rt, plus(p, step(face)));
    let at = pos(rt, p);
    put_all(rt, click, &[("$pos", at), ("$face", V::I(face)), ("$state", here), ("$beside", beside)]);
}

/// The face of the last right click that placed a block at `p`, and the state it replaced: the
/// clicked block's (a replaceable one, grass) or the one beside it. Up and air without one.
fn placed_by_click(rt: &mut Rt, p: [i32; 3]) -> (i32, Option<V>) {
    let Some(&click) = rt.interned.get(LAST_CLICK) else { return (1, None) };
    let at = rt.get_field(click, "$pos");
    let clicked = api::pos_of(rt, at);
    let face = rt.get_field(click, "$face").i();
    if clicked == p {
        (face, Some(rt.get_field(click, "$state")))
    } else if plus(clicked, step(face)) == p {
        (face, Some(rt.get_field(click, "$beside")))
    } else {
        (1, None)
    }
}

/// `BlockEvent.EntityPlaceEvent`: the acting player placed the block `state` (its text) at `p`
/// with an item, before the stack shrinks: placed against the block behind the clicked face
/// (`getPlacedAgainst`), its snapshot holding the block it replaced.
pub fn block_place(rt: &mut Rt, p: [i32; 3], state: &str) {
    if !listened(rt, PLACE) {
        return;
    }
    let (face, replaced) = placed_by_click(rt, p);
    let replaced = replaced.unwrap_or_else(|| api::state(rt, "minecraft:air"));
    let snap = snapshot(rt, p, replaced);
    let ev = rt.new_object(PLACE);
    let level = crate::glue::level(rt);
    let at = pos(rt, p);
    let placed = api::state(rt, state);
    let against = state_at(rt, minus(p, step(face)));
    let player = api_common::acting_player(rt);
    put_all(
        rt,
        ev,
        &[
            ("$level", level),
            ("$pos", at),
            ("$state", placed),
            ("$placedBlock", placed),
            ("$placedAgainst", against),
            ("$blockSnapshot", snap),
            ("$entity", player),
            ("$canceled", V::I(0)),
        ],
    );
    crate::neo::post(rt, PLACE, ev);
}

// ---- a player's tick ----

/// Whether any mod listens to a player's tick.
pub fn player_ticks_heard(rt: &mut Rt) -> bool {
    listened(rt, PLAYER_TICK_PRE) || listened(rt, PLAYER_TICK_POST)
}

fn tick_events(rt: &mut Rt, player: V) {
    for class in [PLAYER_TICK_PRE, PLAYER_TICK_POST] {
        if !listened(rt, class) {
            continue;
        }
        let ev = rt.new_object(class);
        rt.put_field(ev, "$player", player);
        crate::neo::post(rt, class, ev);
    }
}

/// A server player's tick (`Player.tick`): `PlayerTickEvent.Pre`, then `.Post`, for the acting
/// player.
pub fn player_tick(rt: &mut Rt) {
    if !player_ticks_heard(rt) {
        return;
    }
    let player = api_common::acting_player(rt);
    tick_events(rt, player);
}

/// The client's own player's tick, among the client level's entities: `PlayerTickEvent.Pre`, then
/// `.Post`, while there is a player.
pub fn client_player_tick(rt: &mut Rt) {
    if rt.host().is_none_or(|h| h.client_player().is_none()) || !player_ticks_heard(rt) {
        return;
    }
    let player = crate::network::local_player(rt);
    tick_events(rt, player);
}

// ---- the client on a server ----

fn network_event(rt: &mut Rt, class: &'static str) {
    if !listened(rt, class) {
        return;
    }
    let ev = rt.new_object(class);
    let player = crate::network::local_player(rt);
    rt.put_field(ev, "$player", player);
    crate::neo::post(rt, class, ev);
}

/// The client made a level for `dimension` (`ClientLevel`'s constructor): joining a server,
/// `ClientPlayerNetworkEvent.LoggingIn` (`ClientPacketListener.handleLogin`); on a change of
/// dimension, the player made anew (`.Clone`, from `handleRespawn`), the old level's unload to
/// follow.
pub fn client_level_load(rt: &mut Rt, dimension: &str) {
    let held = rt.interned.get(CLIENT_LEVELS).map_or(0, |v| v.i());
    rt.interned.insert(CLIENT_LEVELS.into(), V::I(held + 1));
    if Rt::tracing() {
        eprintln!("horizon-compat: the client's level {dimension} loaded");
    }
    if held == 0 {
        // The level first (its `LevelEvent.Load`), then the player logs in.
        crate::glue::level(rt);
        network_event(rt, LOGGING_IN);
    } else {
        network_event(rt, CLONE);
    }
}

/// The client dropped its level for `dimension`: replaced by another dimension's, or leaving the
/// server, where `ClientPlayerNetworkEvent.LoggingOut` is posted (`Minecraft.disconnect`), then
/// the level's `LevelEvent.Unload`; the client's block entities go with it, and the next server's
/// level is a new one (its `LevelEvent.Load`).
pub fn client_level_unload(rt: &mut Rt, dimension: &str) {
    let held = rt.interned.get(CLIENT_LEVELS).map_or(0, |v| v.i());
    rt.interned.insert(CLIENT_LEVELS.into(), V::I((held - 1).max(0)));
    if Rt::tracing() {
        eprintln!("horizon-compat: the client's level {dimension} unloaded");
    }
    if held > 1 {
        return;
    }
    network_event(rt, LOGGING_OUT);
    if let Some(level) = rt.interned.remove("compat:level") {
        let ev = rt.new_object(LEVEL_UNLOAD);
        rt.put_field(ev, "$level", level);
        crate::neo::post(rt, LEVEL_UNLOAD, ev);
    }
    rt.block_entities.clear();
    rt.block_entity_meta.clear();
    // `Minecraft.level` and `.player` are null until the next server's.
    if let Some(&mc) = rt.interned.get("client/net/minecraft/client/Minecraft") {
        rt.put_field(mc, "level", V::NULL);
        rt.put_field(mc, "player", V::NULL);
    }
}

// ---- entities joining and leaving ----

/// The level a mod entity was made in (`EntityType.create(level)`), or the runtime's.
fn level_of(rt: &mut Rt, e: V) -> V {
    match rt.get_field(e, "$level") {
        V::R(n) if n != 0 => V::R(n),
        _ => crate::glue::level(rt),
    }
}

/// `EntityJoinLevelEvent`: the mod entity `e` joins the level (`PersistentEntitySectionManager.addEntity`
/// on a server, `ClientLevel.addEntity` on a client), not yet among its entities. Whether a listener
/// canceled it: it then stays out.
pub fn entity_joins(rt: &mut Rt, e: V, from_disk: bool) -> bool {
    if !listened(rt, ENTITY_JOIN) {
        return false;
    }
    let ev = rt.new_object(ENTITY_JOIN);
    let level = level_of(rt, e);
    put_all(rt, ev, &[("$entity", e), ("$level", level), ("$fromDisk", V::I(i32::from(from_disk))), ("$canceled", V::I(0))]);
    crate::neo::post(rt, ENTITY_JOIN, ev);
    rt.get_field(ev, "$canceled").i() != 0
}

/// `EntityLeaveLevelEvent`: the mod entity `e` left the level (`onTrackingEnd`: removed, or its
/// copy on a client let go).
pub fn entity_leaves(rt: &mut Rt, e: V) {
    if !listened(rt, ENTITY_LEAVE) {
        return;
    }
    let ev = rt.new_object(ENTITY_LEAVE);
    let level = level_of(rt, e);
    put_all(rt, ev, &[("$entity", e), ("$level", level)]);
    crate::neo::post(rt, ENTITY_LEAVE, ev);
}

// ---- the events' calls ----

fn block_event(rt: &Rt, v: V) -> bool {
    rt.instance_of(v, BREAK) || rt.instance_of(v, PLACE)
}

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, BREAK | PLACE | MULTI_PLACE | SNAPSHOT | PLAYER_TICK | PLAYER_TICK_PRE | PLAYER_TICK_POST | ENTITY_JOIN | ENTITY_LEAVE)
        || owner.starts_with(NETWORK_EVENT)
        || (owner == BLOCK_EVENT && name == "<init>")
        || (matches!(owner, EVENT | CANCELLABLE) && matches!(name, "isCanceled" | "setCanceled"))
        || (matches!(owner, PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT) && matches!(name, "getEntity"))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    if owner == SNAPSHOT {
        return snapshot_call(rt, name, desc, args);
    }
    if owner.starts_with(NETWORK_EVENT) {
        return Some(match name {
            "getPlayer" | "getNewPlayer" | "getOldPlayer" => rt.get_field(ev, "$player"),
            "getMultiPlayerGameMode" => rt.intern(format!("client/{GAME_MODE}"), |rt| rt.new_object(GAME_MODE)),
            "getConnection" => rt.intern(format!("client/{CONNECTION}"), |rt| rt.new_object(CONNECTION)),
            _ => return None,
        });
    }
    if owner.starts_with(PLAYER_TICK) || (matches!(owner, PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT) && rt.class_of(ev).starts_with(PLAYER_TICK)) {
        return (name == "getEntity").then(|| rt.get_field(ev, "$player"));
    }
    let joining = matches!(rt.class_of(ev), ENTITY_JOIN | ENTITY_LEAVE);
    if matches!(owner, ENTITY_JOIN | ENTITY_LEAVE) || (joining && matches!(owner, ENTITY_EVENT | EVENT | CANCELLABLE)) {
        if !joining {
            return None;
        }
        return Some(match name {
            "getEntity" => rt.get_field(ev, "$entity"),
            "getLevel" => rt.get_field(ev, "$level"),
            "loadedFromDisk" => V::I(rt.get_field(ev, "$fromDisk").i()),
            "isCanceled" => V::I(rt.get_field(ev, "$canceled").i()),
            "setCanceled" if rt.class_of(ev) == ENTITY_JOIN => {
                rt.put_field(ev, "$canceled", V::I(a(1).i()));
                V::Void
            }
            _ => return None,
        });
    }
    if name == "<init>" {
        return construct(rt, owner, args);
    }
    let ours = matches!(owner, BREAK | PLACE | MULTI_PLACE) || (matches!(owner, EVENT | CANCELLABLE) && block_event(rt, ev));
    if !ours {
        return None;
    }
    let field = match name {
        "getLevel" => "$level",
        "getPos" => "$pos",
        "getState" => "$state",
        "getPlayer" if owner == BREAK => "$player",
        "getEntity" => "$entity",
        "getPlacedBlock" => "$placedBlock",
        "getPlacedAgainst" => "$placedAgainst",
        "getBlockSnapshot" => "$blockSnapshot",
        "getReplacedBlockSnapshots" => {
            let all = match rt.get_field(ev, "$snapshots") {
                V::R(n) if n != 0 => crate::jutil::elements(rt, V::R(n)),
                _ => vec![rt.get_field(ev, "$blockSnapshot")],
            };
            return Some(crate::jutil::new_list(rt, all));
        }
        "isCanceled" => return Some(V::I(rt.get_field(ev, "$canceled").i())),
        "setCanceled" => {
            rt.put_field(ev, "$canceled", V::I(a(1).i()));
            return Some(V::Void);
        }
        _ => return None,
    };
    Some(rt.get_field(ev, field))
}

/// A block event a mod makes (`new BreakEvent(level, pos, state, player)`), or the part of its own
/// subclass's constructor `BlockEvent`'s does.
fn construct(rt: &mut Rt, owner: &str, args: &[V]) -> Option<V> {
    know_classes(rt);
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    match owner {
        BLOCK_EVENT => put_all(rt, this, &[("$level", a(1)), ("$pos", a(2)), ("$state", a(3))]),
        BREAK => put_all(rt, this, &[("$level", a(1)), ("$pos", a(2)), ("$state", a(3)), ("$player", a(4))]),
        PLACE | MULTI_PLACE => {
            // A multiple placement is about its first block, and keeps them all.
            let snap = if owner == MULTI_PLACE {
                let all = crate::jutil::elements(rt, a(1));
                let copy = crate::jutil::new_list(rt, all.clone());
                rt.put_field(this, "$snapshots", copy);
                all.first().copied().unwrap_or(V::NULL)
            } else {
                a(1)
            };
            let level = rt.get_field(snap, "$level");
            let at = rt.get_field(snap, "$pos");
            // A player's is the block now there; anyone else's the snapshot's.
            let placed = if rt.instance_of(a(3), PLAYER) {
                let p = api::pos_of(rt, at);
                state_at(rt, p)
            } else {
                rt.get_field(snap, "$state")
            };
            put_all(
                rt,
                this,
                &[
                    ("$level", level),
                    ("$pos", at),
                    ("$state", placed),
                    ("$placedBlock", placed),
                    ("$placedAgainst", a(2)),
                    ("$blockSnapshot", snap),
                    ("$entity", a(3)),
                ],
            );
        }
        _ => return None,
    }
    Some(V::Void)
}

/// `BlockSnapshot`'s: made of the block at a position (`create`), its parts, the block there now
/// (`getCurrentState`), and putting the kept block back (`restore`).
fn snapshot_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    Some(match name {
        "create" => {
            // (dimension, level, pos[, flags])
            let p = api::pos_of(rt, a(2));
            let state = state_at(rt, p);
            let snap = snapshot(rt, p, state);
            rt.put_field(snap, "$dimension", a(0));
            rt.put_field(snap, "$level", a(1));
            if desc.starts_with("(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;I") {
                rt.put_field(snap, "$flags", V::I(a(3).i()));
            }
            snap
        }
        "getPos" => rt.get_field(s, "$pos"),
        "getLevel" => rt.get_field(s, "$level"),
        "getDimension" => rt.get_field(s, "$dimension"),
        "getState" => rt.get_field(s, "$state"),
        "getFlags" => V::I(rt.get_field(s, "$flags").i()),
        "getCurrentState" => {
            let at = rt.get_field(s, "$pos");
            let p = api::pos_of(rt, at);
            state_at(rt, p)
        }
        "restore" => {
            let at = rt.get_field(s, "$pos");
            let p = api::pos_of(rt, at);
            let state = rt.get_field(s, "$state");
            let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
            let set = rt.host_mut().is_some_and(|h| h.set_block_state(p, &text));
            V::I(i32::from(set))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        Box::leak(format!("{owner}.{name}.{desc}").into_boxed_str())
    }

    fn heard(rt: &mut Rt) -> Vec<String> {
        let list = rt.interned["test:heard"];
        crate::jutil::elements(rt, list).into_iter().map(|v| rt.str_of(v).unwrap_or("").to_owned()).collect()
    }

    fn note(rt: &mut Rt, line: String) {
        let list = rt.interned["test:heard"];
        let s = rt.string(&line);
        crate::jutil::list_add(rt, list, s);
    }

    /// Listeners as mods write them: a break listener that keeps diamond ore (Create's symmetry and
    /// a claim mod read the player, position and state), a place listener that reads what was placed
    /// and against what, a tick listener, and the client's network events.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        let pos_text = |rt: &mut Rt, owner: &str| {
            let p = crate::api::call(rt, key(owner, "getPos", "()Lnet/minecraft/core/BlockPos;"), &[ev]);
            let [x, y, z] = crate::api::pos_of(rt, p);
            format!("{x},{y},{z}")
        };
        match id {
            1 => {
                let state = crate::api::call(rt, key(BREAK, "getState", "()Lnet/minecraft/world/level/block/state/BlockState;"), &[ev]);
                let state = rt.str_of(state).unwrap_or("").to_owned();
                let player = crate::api::call(rt, key(BREAK, "getPlayer", "()Lnet/minecraft/world/entity/player/Player;"), &[ev]);
                let has_player = rt.instance_of(player, PLAYER);
                let at = pos_text(rt, BREAK);
                note(rt, format!("break {state} at {at} by player {has_player}"));
                if state == "minecraft:diamond_ore" {
                    crate::api::call(rt, key(BREAK, "setCanceled", "(Z)V"), &[ev, V::I(1)]);
                }
            }
            2 => {
                let placed = crate::api::call(rt, key(PLACE, "getPlacedBlock", "()Lnet/minecraft/world/level/block/state/BlockState;"), &[ev]);
                let against = crate::api::call(rt, key(PLACE, "getPlacedAgainst", "()Lnet/minecraft/world/level/block/state/BlockState;"), &[ev]);
                let snap = crate::api::call(rt, key(PLACE, "getBlockSnapshot", "()Lnet/neoforged/neoforge/common/util/BlockSnapshot;"), &[ev]);
                let replaced = crate::api::call(rt, key(SNAPSHOT, "getState", "()Lnet/minecraft/world/level/block/state/BlockState;"), &[snap]);
                let entity = crate::api::call(rt, key(PLACE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[ev]);
                let at = pos_text(rt, PLACE);
                let line = format!(
                    "place {} at {at} against {} over {} by player {}",
                    rt.str_of(placed).unwrap_or(""),
                    rt.str_of(against).unwrap_or(""),
                    rt.str_of(replaced).unwrap_or(""),
                    rt.instance_of(entity, PLAYER)
                );
                note(rt, line);
            }
            3 => {
                let class = rt.class_of(ev).rsplit('$').next().unwrap_or("").to_owned();
                let player = crate::api::call(rt, key(PLAYER_TICK_POST, "getEntity", "()Lnet/minecraft/world/entity/player/Player;"), &[ev]);
                let has_player = rt.instance_of(player, PLAYER);
                note(rt, format!("tick {class} player {has_player}"));
            }
            4 => {
                let class = rt.class_of(ev).rsplit('$').next().unwrap_or("").to_owned();
                let player = crate::api::call(rt, key(NETWORK_EVENT, "getPlayer", "()Lnet/minecraft/client/player/LocalPlayer;"), &[ev]);
                let local = rt.class_of(player) == "net/minecraft/client/player/LocalPlayer";
                note(rt, format!("network {class} local player {local}"));
            }
            5 => {
                let level = crate::api::call(rt, key("net/neoforged/neoforge/event/level/LevelEvent", "getLevel", "()Lnet/minecraft/world/level/LevelAccessor;"), &[ev]);
                let client = rt.get_field(level, "isClientSide").i() != 0;
                note(rt, format!("level unload client {client}"));
            }
            _ => return None,
        }
        Some(V::Void)
    }

    fn listening(events: &[(&str, u32)]) -> Rt {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lambdas);
        for &(class, id) in events {
            let l = rt.lambda_id("java/util/function/Consumer", id, vec![]);
            rt.listeners.push((class.to_owned(), l));
        }
        let list = crate::jutil::new_list(&mut rt, Vec::new());
        rt.interned.insert("test:heard".into(), list);
        rt
    }

    /// A break reaches the mods with the player, position and state, and a mod's cancel keeps the
    /// block; a placement reaches them with what was placed, the block it was placed against (the
    /// one clicked) and the one it replaced. A listener for any `BlockEvent` hears both.
    #[test]
    fn breaking_and_placing_reach_the_mods_as_block_events() {
        let mut rt = listening(&[(BREAK, 1), (PLACE, 2)]);
        let mut h = MemoryHost { player_id: Some(7), place: Some(([0.5, 65.0, 0.5], 0.0, 0.0)), ..Default::default() };
        h.blocks.insert([0, 64, 0], "minecraft:stone".into());
        h.blocks.insert([0, 64, 1], "minecraft:short_grass".into());
        rt.with_host(&mut h, |rt| {
            assert!(!block_break(rt, [0, 64, 0], "minecraft:stone"), "a stone breaks");
            assert!(block_break(rt, [3, 10, 3], "minecraft:diamond_ore"), "the mod keeps diamond ore");
            // Right on the stone's top face, then the oak planks it placed above it.
            clicked(rt, [0, 64, 0], 1);
        });
        h.blocks.insert([0, 65, 0], "minecraft:oak_planks".into());
        rt.with_host(&mut h, |rt| {
            block_place(rt, [0, 65, 0], "minecraft:oak_planks");
            // Right on the grass's north face: the grass is replaced, against the block south of it.
            clicked(rt, [0, 64, 1], 2);
        });
        h.blocks.insert([0, 64, 1], "minecraft:dirt".into());
        h.blocks.insert([0, 64, 2], "minecraft:sand".into());
        rt.with_host(&mut h, |rt| block_place(rt, [0, 64, 1], "minecraft:dirt"));
        assert_eq!(
            heard(&mut rt),
            [
                "break minecraft:stone at 0,64,0 by player true",
                "break minecraft:diamond_ore at 3,10,3 by player true",
                "place minecraft:oak_planks at 0,65,0 against minecraft:stone over minecraft:air by player true",
                "place minecraft:dirt at 0,64,1 against minecraft:sand over minecraft:short_grass by player true",
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own `BreakEvent` (an area tool asking whether a block may break), posted on the bus,
    /// reaches the listeners for `BlockEvent`; a `BlockSnapshot` puts the block it kept back.
    #[test]
    fn a_mods_own_break_event_and_snapshot() {
        let mut rt = listening(&[(BLOCK_EVENT, 1)]);
        let mut h = MemoryHost { player_id: Some(7), ..Default::default() };
        h.blocks.insert([1, 2, 3], "minecraft:diamond_ore".into());
        rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            let p = pos(rt, [1, 2, 3]);
            let state = api::state(rt, "minecraft:diamond_ore");
            let player = crate::menus::server_player(rt, 7);
            let ev = rt.new_object(BREAK);
            crate::api::call(
                rt,
                key(BREAK, "<init>", "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/player/Player;)V"),
                &[ev, level, p, state, player],
            );
            crate::neo::post(rt, BREAK, ev);
            let canceled = crate::api::call(rt, key(BREAK, "isCanceled", "()Z"), &[ev]);
            assert_eq!(canceled.i(), 1, "the listener kept it");

            let dim = V::NULL;
            let snap = crate::api::call(
                rt,
                key(SNAPSHOT, "create", "(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;)Lnet/neoforged/neoforge/common/util/BlockSnapshot;"),
                &[dim, level, p],
            );
            rt.host_mut().unwrap().set_block_state([1, 2, 3], "minecraft:air");
            let now = crate::api::call(rt, key(SNAPSHOT, "getCurrentState", "()Lnet/minecraft/world/level/block/state/BlockState;"), &[snap]);
            assert_eq!(rt.str_of(now), Some("minecraft:air"));
            assert_eq!(crate::api::call(rt, key(SNAPSHOT, "restore", "(I)Z"), &[snap, V::I(3)]).i(), 1);
        });
        assert_eq!(h.blocks.get(&[1, 2, 3]).map(String::as_str), Some("minecraft:diamond_ore"), "restored");
        assert_eq!(heard(&mut rt), ["break minecraft:diamond_ore at 1,2,3 by player true"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A player's tick reaches the mods as `PlayerTickEvent.Pre` then `.Post`, with the player; the
    /// client's own player's only while there is one. Joining a server is `LoggingIn`, a change of
    /// dimension `Clone`, leaving `LoggingOut` and the client level's `LevelEvent.Unload`.
    #[test]
    fn a_players_tick_and_the_client_joining_and_leaving() {
        let mut rt = listening(&[(PLAYER_TICK, 3), (NETWORK_EVENT, 4), (LEVEL_UNLOAD, 5)]);
        let mut server = MemoryHost { player_id: Some(7), ..Default::default() };
        rt.with_host(&mut server, player_tick);
        assert_eq!(heard(&mut rt), ["tick Pre player true", "tick Post player true"]);

        let mut rt = listening(&[(PLAYER_TICK, 3), (NETWORK_EVENT, 4), (LEVEL_UNLOAD, 5)]);
        rt.interned.insert("compat:client".into(), V::I(1));
        let mut client = MemoryHost { client: true, ..Default::default() };
        rt.with_host(&mut client, |rt| {
            client_player_tick(rt);
            client_level_load(rt, "minecraft:overworld");
            crate::glue::level(rt);
            client_level_load(rt, "minecraft:the_nether");
            client_level_unload(rt, "minecraft:overworld");
            client_level_unload(rt, "minecraft:the_nether");
        });
        assert!(!rt.interned.contains_key("compat:level"), "the next server's level is a new one");
        assert_eq!(
            heard(&mut rt),
            [
                "network LoggingIn local player true",
                "network Clone local player true",
                "network LoggingOut local player true",
                "level unload client true",
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
