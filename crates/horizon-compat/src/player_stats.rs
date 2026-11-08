//! A player's statistics as a mod reaches them: `Player.awardStat` and `resetStat`, the counters
//! `ServerPlayer.getStats()` and `LocalPlayer.getStats()` give, `Stats`' statistic types and their
//! statistics (`Stats.CUSTOM.get(id)`), and NeoForge's `StatAwardEvent`.
//!
//! A server's player's counter is the game's own: what a mod awards is kept with the game's
//! statistics (saved with them, sent to the player's client) and counted by the scoreboard's
//! objectives of that statistic, and what it reads is what the game counted, the player's walks,
//! kills and blocks mined among them ([`Host::stat_value`], [`Host::set_stat`],
//! [`Host::score_stat`]). Each value set first goes to the listeners of `StatAwardEvent`, which may
//! change the statistic or the value, or cancel it, as NeoForge's `StatsCounter.setValue` posts it;
//! the scoreboard hears of an award all the same, as `ServerPlayer.awardStat` tells it after the
//! counter.
//!
//! A counter the host can't reach (a client's `LocalPlayer.getStats()`, a fake player's, a player
//! of no server) keeps its values itself, as a new `StatsCounter`: each zero until set. A player
//! that isn't a server's awards nothing, as `Player.awardStat` is empty.
//!
//! [`Host::stat_value`]: crate::host::Host::stat_value
//! [`Host::set_stat`]: crate::host::Host::set_stat
//! [`Host::score_stat`]: crate::host::Host::score_stat

use crate::{Rt, V, api, jutil};

const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const COUNTER: &str = "net/minecraft/stats/StatsCounter";
const SERVER_COUNTER: &str = "net/minecraft/stats/ServerStatsCounter";
const STAT: &str = "net/minecraft/stats/Stat";
const STAT_TYPE: &str = "net/minecraft/stats/StatType";
const STATS: &str = "net/minecraft/stats/Stats";
const FORMATTER: &str = "net/minecraft/stats/StatFormatter";
const AWARD: &str = "net/neoforged/neoforge/event/StatAwardEvent";
const PLAYER_EVENT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent";
const LIVING_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent";
const ENTITY_EVENT: &str = "net/neoforged/neoforge/event/entity/EntityEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
const CANCELLABLE: &str = "net/neoforged/bus/api/ICancellableEvent";
const GET_KEY: &str = "net/minecraft/core/Registry.getKey.(Ljava/lang/Object;)Lnet/minecraft/resources/ResourceLocation;";
const ITERATOR: &str = "java/util/List.iterator.()Ljava/util/Iterator;";
const RESOURCE_LOCATION: &str = "(Lnet/minecraft/resources/ResourceLocation;";

/// `Stats`' statistic types: field, registry id, and the registry their values are of
/// (registries are their names here).
const TYPES: [(&str, &str, &str); 9] = [
    ("BLOCK_MINED", "minecraft:mined", "block"),
    ("ITEM_CRAFTED", "minecraft:crafted", "item"),
    ("ITEM_USED", "minecraft:used", "item"),
    ("ITEM_BROKEN", "minecraft:broken", "item"),
    ("ITEM_PICKED_UP", "minecraft:picked_up", "item"),
    ("ITEM_DROPPED", "minecraft:dropped", "item"),
    ("ENTITY_KILLED", "minecraft:killed", "entity_type"),
    ("ENTITY_KILLED_BY", "minecraft:killed_by", "entity_type"),
    ("CUSTOM", "minecraft:custom", "custom_stat"),
];

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    match owner {
        STAT_TYPE => matches!(name, "get" | "contains" | "getRegistry" | "iterator"),
        STAT => matches!(name, "getType" | "getValue"),
        COUNTER | SERVER_COUNTER => matches!(name, "getValue" | "setValue" | "increment"),
        AWARD => true,
        EVENT | CANCELLABLE => matches!(name, "isCanceled" | "setCanceled"),
        PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT => name == "getEntity",
        _ => {
            crate::api_common::is_player(owner)
                && matches!(name, "awardStat" | "resetStat" | "getStats")
        }
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        STAT_TYPE => type_call(rt, name, desc, args),
        STAT => Some(match name {
            "getType" => rt.get_field(a(0), "type"),
            _ => rt.get_field(a(0), "value"),
        }),
        COUNTER | SERVER_COUNTER => counter_call(rt, name, desc, args),
        _ if rt.class_of(a(0)) == AWARD => event_call(rt, name, args),
        AWARD | EVENT | CANCELLABLE | PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT => None,
        _ => player_call(rt, name, desc, args),
    }
}

/// `Stats`' statistic types (`Stats.CUSTOM`), one object each, and its custom statistics' ids
/// (`Stats.JUMP` is `minecraft:jump`); `StatFormatter`'s constants.
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        STATS => Some(match TYPES.iter().find(|t| t.0 == field) {
            Some(&(_, id, _)) => stat_type(rt, id),
            None if field
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') =>
            {
                rt.string(&format!("minecraft:{}", field.to_ascii_lowercase()))
            }
            None => return None,
        }),
        FORMATTER if matches!(field, "DEFAULT" | "DIVIDE_BY_TEN" | "DISTANCE" | "TIME") => {
            Some(rt.intern(format!("compat:stat-formatter/{field}"), |rt| {
                let f = rt.new_object(FORMATTER);
                let n = rt.string(field);
                rt.put_field(f, "$name", n);
                f
            }))
        }
        _ => None,
    }
}

/// The statistic type of registry id `id`, one object.
fn stat_type(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:stat-type/{id}"), |rt| {
        let t = rt.new_object(STAT_TYPE);
        let n = rt.string(id);
        rt.put_field(t, "$id", n);
        t
    })
}

// ---- statistics ----

fn type_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let t = a(0);
    Some(match name {
        // `get(value[, formatter])`: the statistic of that value, one per type and value.
        "get" => {
            let formatter = if desc.contains("Lnet/minecraft/stats/StatFormatter;") {
                a(2)
            } else {
                get_static(rt, FORMATTER, "DEFAULT").unwrap_or(V::NULL)
            };
            stat(rt, t, a(1), formatter)
        }
        // Whether its statistic of that value was made (`get`).
        "contains" => {
            let key = stat_key(rt, t, a(1));
            V::I(i32::from(rt.interned.contains_key(&key)))
        }
        // The registry its values are of (a mod's own type: the item registry).
        "getRegistry" => {
            let id = type_id(rt, t).unwrap_or_default();
            let registry = TYPES.iter().find(|ty| ty.1 == id).map_or("item", |ty| ty.2);
            rt.string(registry)
        }
        // Its statistics made so far, in the order they were.
        "iterator" => {
            let made = made(rt, t);
            api::call(rt, ITERATOR, &[made])
        }
        _ => return None,
    })
}

/// Type `t`'s statistic of `value`: made once, then the same object.
fn stat(rt: &mut Rt, t: V, value: V, formatter: V) -> V {
    let key = stat_key(rt, t, value);
    if let Some(&s) = rt.interned.get(&key) {
        return s;
    }
    let s = rt.intern(key, |rt| {
        let s = rt.new_object(STAT);
        rt.put_field(s, "type", t);
        rt.put_field(s, "value", value);
        rt.put_field(s, "formatter", formatter);
        s
    });
    let made = made(rt, t);
    jutil::list_add(rt, made, s);
    s
}

/// The list of type `t`'s statistics made so far.
fn made(rt: &mut Rt, t: V) -> V {
    match rt.get_field(t, "$made") {
        l if l.r() != 0 => l,
        _ => {
            let l = jutil::new_list(rt, Vec::new());
            if t.r() != 0 {
                rt.put_field(t, "$made", l);
            }
            l
        }
    }
}

/// What type `t`'s statistic of `value` is kept by: the type's id (else the object), and the
/// value's id (a `ResourceLocation` itself, else the object).
fn stat_key(rt: &Rt, t: V, value: V) -> String {
    let ty = type_id(rt, t).unwrap_or_else(|| format!("#{}", t.r()));
    let value = match rt.str_of(value) {
        Some(_) => crate::neo::location(rt, value),
        None => format!("#{}", value.r()),
    };
    format!("compat:stat/{ty}/{value}")
}

fn type_id(rt: &Rt, t: V) -> Option<String> {
    rt.str_of(rt.get_field(t, "$id")).map(str::to_owned)
}

/// The ids the game keeps a statistic of type `t` and value `value` by: the type's
/// (`minecraft:used`), and the value's in the type's registry (`minecraft:diamond`; a custom
/// statistic's is the `ResourceLocation` itself). None for a mod's own type, or a value of none.
fn ids(rt: &mut Rt, t: V, value: V) -> Option<(String, String)> {
    let ty = type_id(rt, t)?;
    if value.is_null() {
        return None;
    }
    if rt.str_of(value).is_some() {
        return Some((ty, crate::neo::location(rt, value)));
    }
    let registry = TYPES.iter().find(|t| t.1 == ty)?.2;
    let r = rt.string(registry);
    let key = api::call(rt, GET_KEY, &[r, value]);
    let key = rt
        .str_of(key)
        .is_some()
        .then(|| crate::neo::location(rt, key))?;
    Some((ty, key))
}

// ---- players ----

fn player_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let p = a(0);
    Some(match name {
        "getStats" => counter(rt, p),
        // `awardStat(id[, n])` is `awardStat(Stats.CUSTOM.get(id)[, n])`; `awardStat(stat)` one.
        "awardStat" => {
            let stat = if desc.starts_with(RESOURCE_LOCATION) {
                let custom = stat_type(rt, "minecraft:custom");
                let formatter = get_static(rt, FORMATTER, "DEFAULT").unwrap_or(V::NULL);
                self::stat(rt, custom, a(1), formatter)
            } else {
                a(1)
            };
            let n = if desc.ends_with(";I)V") { a(2).i() } else { 1 };
            if server_side(rt, p) {
                award(rt, p, stat, n);
            }
            V::Void
        }
        "resetStat" => {
            if server_side(rt, p) {
                let c = counter(rt, p);
                set_value(rt, c, p, a(1), 0);
                score(rt, c, a(1), None);
            }
            V::Void
        }
        _ => return None,
    })
}

/// Whether `p` is a server's player, whose awards count (`ServerPlayer.awardStat`); a fake one's
/// count nowhere.
fn server_side(rt: &Rt, p: V) -> bool {
    rt.instance_of(p, SERVER_PLAYER) && !crate::fake_players::is_fake(rt, p)
}

/// `ServerPlayer.awardStat(stat, n)`: the counter's `increment`, then the scoreboard's objectives
/// of that statistic, whatever the listeners did to the value.
fn award(rt: &mut Rt, p: V, stat: V, n: i32) {
    let c = counter(rt, p);
    increment(rt, c, p, stat, n);
    score(rt, c, stat, Some(n));
}

/// Player `p`'s counter: a server's player's is the game's, one object for each player (the
/// acting player's for an object that names none); a client's player's, a fake one's or one of no
/// server, a counter of its own.
fn counter(rt: &mut Rt, p: V) -> V {
    let uuid = if server_side(rt, p) {
        crate::network::player_uuid(rt, p).or_else(|| rt.host().and_then(|h| h.player_id()))
    } else {
        None
    };
    if let Some(uuid) = uuid {
        return rt.intern(format!("compat:stats/{uuid:x}"), |rt| {
            let c = rt.new_object(SERVER_COUNTER);
            let u = crate::tags::uuid(rt, (uuid >> 64) as i64, uuid as i64);
            rt.put_field(c, "$uuid", u);
            c
        });
    }
    match rt.get_field(p, "$stats") {
        c if c.r() != 0 => c,
        _ => {
            let class = if rt.instance_of(p, SERVER_PLAYER) {
                SERVER_COUNTER
            } else {
                COUNTER
            };
            let c = rt.new_object(class);
            if p.r() != 0 {
                rt.put_field(p, "$stats", c);
            }
            c
        }
    }
}

// ---- counters ----

fn counter_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    Some(match name {
        // `getValue(type, value)`: the statistic's value, zero for one never set.
        "getValue" if desc.starts_with("(Lnet/minecraft/stats/StatType;") => {
            V::I(value(rt, c, a(1), a(2)))
        }
        "getValue" => {
            let (t, v) = parts(rt, a(1));
            V::I(value(rt, c, t, v))
        }
        "setValue" => {
            set_value(rt, c, a(1), a(2), a(3).i());
            V::Void
        }
        "increment" => {
            increment(rt, c, a(1), a(2), a(3).i());
            V::Void
        }
        _ => return None,
    })
}

/// A statistic's type and value.
fn parts(rt: &Rt, stat: V) -> (V, V) {
    (rt.get_field(stat, "type"), rt.get_field(stat, "value"))
}

/// `StatsCounter.increment`: the value plus `n`, no more than `Integer.MAX_VALUE`.
fn increment(rt: &mut Rt, c: V, p: V, stat: V, n: i32) {
    let (t, v) = parts(rt, stat);
    let sum = (i64::from(value(rt, c, t, v)) + i64::from(n)).min(i64::from(i32::MAX));
    set_value(rt, c, p, stat, sum as i32);
}

/// `StatsCounter.setValue` as NeoForge has it: `StatAwardEvent`'s listeners hear the new value
/// first, and may change the statistic or the value, or cancel it.
fn set_value(rt: &mut Rt, c: V, p: V, stat: V, value: i32) {
    let Some((stat, value)) = award_event(rt, p, stat, value) else {
        return;
    };
    let (t, v) = parts(rt, stat);
    if let Some((uuid, ty, key)) = reach(rt, c, t, v)
        && rt
            .host_mut()
            .is_some_and(|h| h.set_stat(uuid, &ty, &key, value))
    {
        return;
    }
    let (table, key) = own(rt, c, t, v);
    jutil::table_put(rt, table, key, V::I(value));
}

/// Counter `c`'s value of type `t`'s statistic of `v`: the game's for a server's player, else the
/// counter's own.
fn value(rt: &mut Rt, c: V, t: V, v: V) -> i32 {
    if let Some((uuid, ty, key)) = reach(rt, c, t, v)
        && let Some(n) = rt.host_mut().and_then(|h| h.stat_value(uuid, &ty, &key))
    {
        return n;
    }
    let (table, key) = own(rt, c, t, v);
    jutil::table_get(rt, table, key).map_or(0, V::i)
}

/// The scoreboard's part of an award (`change`) or a reset (None), for a server's player.
fn score(rt: &mut Rt, c: V, stat: V, change: Option<i32>) {
    let (t, v) = parts(rt, stat);
    if let Some((uuid, ty, key)) = reach(rt, c, t, v)
        && let Some(h) = rt.host_mut()
    {
        h.score_stat(uuid, &ty, &key, change);
    }
}

/// The player of counter `c` and the statistic's ids, when the counter is a server's player's.
fn reach(rt: &mut Rt, c: V, t: V, v: V) -> Option<(u128, String, String)> {
    let uuid = crate::network::player_uuid(rt, c)?;
    let (ty, key) = ids(rt, t, v)?;
    Some((uuid, ty, key))
}

/// Counter `c`'s own values and the key of type `t`'s statistic of `v` in them.
fn own(rt: &mut Rt, c: V, t: V, v: V) -> (V, V) {
    let table = match rt.get_field(c, "$values") {
        m if m.r() != 0 => m,
        _ => {
            let m = rt.alloc("java/util/HashMap", crate::Data::Table(Default::default()));
            rt.put_field(c, "$values", m);
            m
        }
    };
    let key = stat_key(rt, t, v);
    let key = rt.string(&key);
    (table, key)
}

// ---- StatAwardEvent ----

/// The event's supers, so a listener for `PlayerEvent` (or any event) hears it.
fn event_supers(rt: &mut Rt) {
    for (class, sup) in [
        (AWARD, PLAYER_EVENT),
        (PLAYER_EVENT, LIVING_EVENT),
        (LIVING_EVENT, ENTITY_EVENT),
        (ENTITY_EVENT, EVENT),
    ] {
        if !rt.supers.contains_key(class) {
            rt.supers.insert(class, sup);
        }
    }
}

/// `EventHooks.onStatAward(player, stat, value)`: the statistic and value its listeners leave,
/// None when one cancels it. Posted only when someone listens.
fn award_event(rt: &mut Rt, p: V, stat: V, value: i32) -> Option<(V, i32)> {
    event_supers(rt);
    if crate::neo::hearing(rt, AWARD).is_empty() {
        return Some((stat, value));
    }
    let ev = rt.new_object(AWARD);
    rt.put_field(ev, "$player", p);
    rt.put_field(ev, "stat", stat);
    rt.put_field(ev, "value", V::I(value));
    crate::neo::post(rt, AWARD, ev);
    if rt.get_field(ev, "$canceled").i() != 0 {
        return None;
    }
    Some((rt.get_field(ev, "stat"), rt.get_field(ev, "value").i()))
}

fn event_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(ev, "$player", a(1));
            rt.put_field(ev, "stat", a(2));
            rt.put_field(ev, "value", V::I(a(3).i()));
            V::Void
        }
        "getStat" => rt.get_field(ev, "stat"),
        "setStat" => {
            rt.put_field(ev, "stat", a(1));
            V::Void
        }
        "getValue" => V::I(rt.get_field(ev, "value").i()),
        "setValue" => {
            rt.put_field(ev, "value", V::I(a(1).i()));
            V::Void
        }
        "getEntity" => rt.get_field(ev, "$player"),
        "isCanceled" => V::I(rt.get_field(ev, "$canceled").i()),
        "setCanceled" => {
            rt.put_field(ev, "$canceled", V::I(a(1).i()));
            V::Void
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const STEVE: u128 = 0x5157_e7e5;
    const ALEX: u128 = 0xa1e7;
    const AWARD_ID: &str = "net/minecraft/world/entity/player/Player.awardStat.(Lnet/minecraft/resources/ResourceLocation;)V";
    const AWARD_ID_N: &str = "net/minecraft/world/entity/player/Player.awardStat.(Lnet/minecraft/resources/ResourceLocation;I)V";
    const AWARD_STAT: &str =
        "net/minecraft/world/entity/player/Player.awardStat.(Lnet/minecraft/stats/Stat;)V";
    const AWARD_STAT_N: &str =
        "net/minecraft/server/level/ServerPlayer.awardStat.(Lnet/minecraft/stats/Stat;I)V";
    const RESET: &str =
        "net/minecraft/server/level/ServerPlayer.resetStat.(Lnet/minecraft/stats/Stat;)V";
    const GET_STATS: &str = "net/minecraft/server/level/ServerPlayer.getStats.()Lnet/minecraft/stats/ServerStatsCounter;";
    const VALUE: &str =
        "net/minecraft/stats/ServerStatsCounter.getValue.(Lnet/minecraft/stats/Stat;)I";
    const VALUE_BY_TYPE: &str = "net/minecraft/stats/StatsCounter.getValue.(Lnet/minecraft/stats/StatType;Ljava/lang/Object;)I";
    const SET_VALUE: &str = "net/minecraft/stats/ServerStatsCounter.setValue.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/stats/Stat;I)V";
    const INCREMENT: &str = "net/minecraft/stats/StatsCounter.increment.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/stats/Stat;I)V";
    const TYPE_GET: &str =
        "net/minecraft/stats/StatType.get.(Ljava/lang/Object;)Lnet/minecraft/stats/Stat;";

    fn call(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let key = rt.static_key(key);
        api::call(rt, key, args)
    }

    fn st(rt: &mut Rt, key: &str) -> V {
        let key = rt.static_key(key);
        api::get_static(rt, key)
    }

    /// Statistic `field` of `Stats` (a type) of `value`.
    fn stat_of(rt: &mut Rt, field: &str, value: V) -> V {
        let t = st(rt, &format!("{STATS}.{field}"));
        call(rt, TYPE_GET, &[t, value])
    }

    fn entry(player: u128, ty: &str, key: &str, value: i32) -> (u128, String, String, i32) {
        (player, ty.to_owned(), key.to_owned(), value)
    }

    fn scored(
        player: u128,
        ty: &str,
        key: &str,
        change: Option<i32>,
    ) -> (u128, String, String, Option<i32>) {
        (player, ty.to_owned(), key.to_owned(), change)
    }

    /// A mod's awards to a server's player land in the game's counters, by the ids the game keeps
    /// them by, and each award tells the scoreboard of its amount; what the mod reads back is the
    /// game's count, the game's own statistics among them (blocks the player mined). A value set
    /// outright or incremented on the counter isn't scored, a reset is, and no count goes past
    /// `Integer.MAX_VALUE`. Each player has a counter of their own.
    #[test]
    fn a_servers_players_awards_reach_the_games_counters_and_scoreboard() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost {
            player_id: Some(STEVE),
            players: vec![(ALEX, [0.0; 3])],
            ..MemoryHost::default()
        };
        host.stats
            .push(entry(STEVE, "minecraft:mined", "minecraft:stone", 12));
        let (steve, alex) = (
            crate::menus::server_player(&mut rt, STEVE),
            crate::menus::server_player(&mut rt, ALEX),
        );
        rt.with_host(&mut host, |rt| {
            let pipes = rt.string("test:pipes_placed");
            call(rt, AWARD_ID, &[steve, pipes]);
            call(rt, AWARD_ID, &[steve, pipes]);
            let diamond = st(rt, "net/minecraft/world/item/Items.DIAMOND");
            let used = stat_of(rt, "ITEM_USED", diamond);
            call(rt, AWARD_STAT_N, &[steve, used, V::I(5)]);
            let jump = st(rt, "net/minecraft/stats/Stats.JUMP");
            call(rt, AWARD_ID_N, &[steve, jump, V::I(3)]);
            let zombie = st(rt, "net/minecraft/world/entity/EntityType.ZOMBIE");
            let killed = stat_of(rt, "ENTITY_KILLED", zombie);
            call(rt, AWARD_STAT, &[alex, killed]);

            let counter = call(rt, GET_STATS, &[steve]);
            assert_eq!(
                call(rt, GET_STATS, &[steve]),
                counter,
                "one counter a player"
            );
            assert_ne!(call(rt, GET_STATS, &[alex]), counter);
            let placed = stat_of(rt, "CUSTOM", pipes);
            assert_eq!(call(rt, VALUE, &[counter, placed]), V::I(2));
            assert_eq!(call(rt, VALUE, &[counter, used]), V::I(5));
            let (mined, stone) = (
                st(rt, &format!("{STATS}.BLOCK_MINED")),
                st(rt, "net/minecraft/world/level/block/Blocks.STONE"),
            );
            assert_eq!(
                call(rt, VALUE_BY_TYPE, &[counter, mined, stone]),
                V::I(12),
                "the game's own count"
            );
            let alexs = call(rt, GET_STATS, &[alex]);
            assert_eq!(call(rt, VALUE, &[alexs, placed]), V::I(0));
            assert_eq!(call(rt, VALUE, &[alexs, killed]), V::I(1));

            call(rt, RESET, &[steve, placed]);
            assert_eq!(call(rt, VALUE, &[counter, placed]), V::I(0));
            call(rt, SET_VALUE, &[counter, steve, placed, V::I(i32::MAX - 1)]);
            call(rt, AWARD_STAT_N, &[steve, placed, V::I(5)]);
            assert_eq!(call(rt, VALUE, &[counter, placed]), V::I(i32::MAX));
            call(rt, INCREMENT, &[counter, steve, used, V::I(2)]);
            assert_eq!(call(rt, VALUE, &[counter, used]), V::I(7));
        });
        let mut stats = host.stats.clone();
        stats.sort();
        let mut expected = vec![
            entry(STEVE, "minecraft:mined", "minecraft:stone", 12),
            entry(STEVE, "minecraft:custom", "test:pipes_placed", i32::MAX),
            entry(STEVE, "minecraft:used", "minecraft:diamond", 7),
            entry(STEVE, "minecraft:custom", "minecraft:jump", 3),
            entry(ALEX, "minecraft:killed", "minecraft:zombie", 1),
        ];
        expected.sort();
        assert_eq!(stats, expected);
        assert_eq!(
            host.stat_scores,
            vec![
                scored(STEVE, "minecraft:custom", "test:pipes_placed", Some(1)),
                scored(STEVE, "minecraft:custom", "test:pipes_placed", Some(1)),
                scored(STEVE, "minecraft:used", "minecraft:diamond", Some(5)),
                scored(STEVE, "minecraft:custom", "minecraft:jump", Some(3)),
                scored(ALEX, "minecraft:killed", "minecraft:zombie", Some(1)),
                scored(STEVE, "minecraft:custom", "test:pipes_placed", None),
                scored(STEVE, "minecraft:custom", "test:pipes_placed", Some(5)),
            ]
        );
    }

    thread_local! {
        /// What the listener heard: the player, the statistic's value and the value set.
        static HEARD: std::cell::RefCell<Vec<(V, String, i32)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// A listener for any player's event: it doubles each value set, and cancels any of jumps.
    fn listener(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        let ev = args.get(1).copied().unwrap_or_default();
        let player = call(
            rt,
            "net/neoforged/neoforge/event/entity/player/PlayerEvent.getEntity.()Lnet/minecraft/world/entity/player/Player;",
            &[ev],
        );
        let stat = call(
            rt,
            "net/neoforged/neoforge/event/StatAwardEvent.getStat.()Lnet/minecraft/stats/Stat;",
            &[ev],
        );
        let value = call(
            rt,
            "net/neoforged/neoforge/event/StatAwardEvent.getValue.()I",
            &[ev],
        )
        .i();
        let of = call(
            rt,
            "net/minecraft/stats/Stat.getValue.()Ljava/lang/Object;",
            &[stat],
        );
        let of = rt.str_of(of).unwrap_or("").to_owned();
        if of == "minecraft:jump" {
            call(
                rt,
                "net/neoforged/bus/api/ICancellableEvent.setCanceled.(Z)V",
                &[ev, V::I(1)],
            );
        } else {
            call(
                rt,
                "net/neoforged/neoforge/event/StatAwardEvent.setValue.(I)V",
                &[ev, V::I(value * 2)],
            );
        }
        HEARD.with_borrow_mut(|h| h.push((player, of, value)));
        Some(V::Void)
    }

    /// `StatAwardEvent` reaches a listener for any player's event with each value the counter is to
    /// take (the total, not the award): one it changes is kept as changed, one it cancels isn't
    /// kept at all, while the scoreboard still counts each award as the mod made it.
    #[test]
    fn a_listener_changes_or_cancels_what_a_counter_keeps() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(listener);
        let l = rt.lambda_id("java/util/function/Consumer", 1, Vec::new());
        rt.listeners.push((PLAYER_EVENT.to_owned(), l));
        let mut host = MemoryHost {
            player_id: Some(STEVE),
            ..MemoryHost::default()
        };
        let steve = crate::menus::server_player(&mut rt, STEVE);
        rt.with_host(&mut host, |rt| {
            let pipes = rt.string("test:pipes_placed");
            call(rt, AWARD_ID_N, &[steve, pipes, V::I(3)]);
            call(rt, AWARD_ID_N, &[steve, pipes, V::I(1)]);
            let jump = st(rt, "net/minecraft/stats/Stats.JUMP");
            call(rt, AWARD_ID, &[steve, jump]);
        });
        assert_eq!(
            HEARD.with_borrow(Clone::clone),
            vec![
                (steve, "test:pipes_placed".to_owned(), 3),
                (steve, "test:pipes_placed".to_owned(), 7),
                (steve, "minecraft:jump".to_owned(), 1)
            ]
        );
        assert_eq!(
            host.stats,
            vec![entry(STEVE, "minecraft:custom", "test:pipes_placed", 14)]
        );
        assert_eq!(
            host.stat_scores,
            vec![
                scored(STEVE, "minecraft:custom", "test:pipes_placed", Some(3)),
                scored(STEVE, "minecraft:custom", "test:pipes_placed", Some(1)),
                scored(STEVE, "minecraft:custom", "minecraft:jump", Some(1)),
            ]
        );
    }

    /// A client's player awards nothing (`Player.awardStat` is empty) and its counter keeps what is
    /// set on it itself; a server's player the game can't reach keeps its awards in its own counter.
    /// A type's statistics are made once each, listed in the order they were.
    #[test]
    fn counters_the_game_cant_reach_keep_their_own_values() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let local = rt.new_object("net/minecraft/client/player/LocalPlayer");
        let away = crate::menus::server_player(&mut rt, ALEX);
        rt.with_host(&mut host, |rt| {
            let custom = st(rt, &format!("{STATS}.CUSTOM"));
            let jump = st(rt, "net/minecraft/stats/Stats.JUMP");
            let contains = "net/minecraft/stats/StatType.contains.(Ljava/lang/Object;)Z";
            assert_eq!(call(rt, contains, &[custom, jump]), V::I(0));
            call(rt, AWARD_ID, &[local, jump]);
            let counter = call(rt, "net/minecraft/client/player/LocalPlayer.getStats.()Lnet/minecraft/stats/StatsCounter;", &[local]);
            assert_eq!(rt.class_of(counter), COUNTER);
            let jumps = stat_of(rt, "CUSTOM", jump);
            assert_eq!(call(rt, contains, &[custom, jump]), V::I(1));
            assert_eq!(call(rt, VALUE, &[counter, jumps]), V::I(0), "a client's player awards nothing");
            call(rt, SET_VALUE, &[counter, local, jumps, V::I(7)]);
            assert_eq!(call(rt, VALUE_BY_TYPE, &[counter, custom, jump]), V::I(7));

            let pipes = rt.string("test:pipes_placed");
            call(rt, AWARD_ID_N, &[away, pipes, V::I(4)]);
            let theirs = call(rt, GET_STATS, &[away]);
            let placed = stat_of(rt, "CUSTOM", pipes);
            assert_eq!(call(rt, VALUE, &[theirs, placed]), V::I(4));
            assert_eq!(call(rt, VALUE, &[theirs, jumps]), V::I(0));

            assert_eq!(stat_of(rt, "CUSTOM", jump), jumps, "one statistic a value");
            let iterator = call(rt, "net/minecraft/stats/StatType.iterator.()Ljava/util/Iterator;", &[custom]);
            let mut made = Vec::new();
            while call(rt, "java/util/Iterator.hasNext.()Z", &[iterator]).i() != 0 {
                made.push(call(rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[iterator]));
            }
            assert_eq!(made, vec![jumps, placed]);
        });
        assert!(host.stats.is_empty() && host.stat_scores.is_empty());
    }
}
