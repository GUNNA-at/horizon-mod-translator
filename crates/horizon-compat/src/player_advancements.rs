//! A server's players' advancements as a mod reaches them (`ServerPlayer.getAdvancements()`,
//! `MinecraftServer.getAdvancements()`). A mod's own criterion triggers hear of a player as on a
//! NeoForge server: there `PlayerAdvancements` registers a listener on each criterion's trigger as
//! the player joins (`registerListeners`), and a trigger that fires runs the listeners that match
//! (`Listener.run`, `PlayerAdvancements.award`). Here the criteria are the game's, its tree's
//! advancements whose trigger is a mod's ([`Host::mod_criteria`]), each read with its trigger's
//! codec, and a listener run grants its criterion through the game, which awards an advancement
//! done with its toast, chat line and rewards. Vanilla's `SimpleCriterionTrigger`, which a mod's
//! trigger may extend, keeps its listeners here.
//!
//! A listener is registered on every criterion, granted or not, once a player: one runs only while
//! its criterion is open (not granted, of an advancement not done), as vanilla keeps a listener
//! only then, so a criterion revoked (`/advancement revoke`) is heard of again. What a mod asks of
//! a player's progress (`getOrStartProgress(holder).isDone()`, `getRemainingCriteria()`) the game
//! answers when asked ([`Host::advancement_progress`]).
//!
//! [`Host::mod_criteria`]: crate::host::Host::mod_criteria
//! [`Host::advancement_progress`]: crate::host::Host::advancement_progress
use crate::host::AdvancementProgressView;
use crate::{Data, Rt, V};

const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const SERVER: &str = "net/minecraft/server/MinecraftServer";
const MANAGER: &str = "net/minecraft/server/ServerAdvancementManager";
const PLAYER_ADVANCEMENTS: &str = "net/minecraft/server/PlayerAdvancements";
const PROGRESS: &str = "net/minecraft/advancements/AdvancementProgress";
const CRITERION_PROGRESS: &str = "net/minecraft/advancements/CriterionProgress";
const HOLDER: &str = "net/minecraft/advancements/AdvancementHolder";
const LISTENER: &str = "net/minecraft/advancements/CriterionTrigger$Listener";
const TRIGGER: &str = "net/minecraft/advancements/CriterionTrigger";
const SIMPLE_TRIGGER: &str = "net/minecraft/advancements/critereon/SimpleCriterionTrigger";
const CODEC: &str =
    "net/minecraft/advancements/CriterionTrigger.codec.()Lcom/mojang/serialization/Codec;";
const ADD_LISTENER: &str = "net/minecraft/advancements/CriterionTrigger.addPlayerListener.(Lnet/minecraft/server/PlayerAdvancements;Lnet/minecraft/advancements/CriterionTrigger$Listener;)V";
const PREDICATE_TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    match owner {
        SERVER_PLAYER | crate::fake_players::FAKE_PLAYER | SERVER => name == "getAdvancements",
        MANAGER => name == "get",
        PLAYER_ADVANCEMENTS => matches!(name, "getOrStartProgress" | "award" | "stopListening"),
        PROGRESS => matches!(
            name,
            "isDone"
                | "hasProgress"
                | "getRemainingCriteria"
                | "getCompletedCriteria"
                | "getCriterion"
        ),
        CRITERION_PROGRESS => matches!(name, "<init>" | "isDone" | "getObtained"),
        LISTENER => name == "run",
        TRIGGER | SIMPLE_TRIGGER => matches!(
            name,
            "addPlayerListener" | "removePlayerListener" | "removePlayerListeners" | "trigger"
        ),
        _ => false,
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (SERVER_PLAYER | crate::fake_players::FAKE_PLAYER, "getAdvancements") => {
            player_advancements(rt, a(0))
        }
        (SERVER, "getAdvancements") => {
            rt.intern("compat:advancements".into(), |rt| rt.new_object(MANAGER))
        }
        // `ServerAdvancementManager.get(id)`: the advancement's holder, null for one the server
        // lacks.
        (MANAGER, "get") => {
            let id = crate::neo::location(rt, a(1));
            match rt.host_mut().is_some_and(|h| h.has_advancement(&id)) {
                true => holder(rt, &id),
                false => V::NULL,
            }
        }
        (PLAYER_ADVANCEMENTS, "getOrStartProgress") => {
            let p = rt.new_object(PROGRESS);
            let id = holder_id(rt, a(1));
            let id = rt.string(&id);
            rt.put_field(p, "$advancements", a(0));
            rt.put_field(p, "$id", id);
            p
        }
        (PLAYER_ADVANCEMENTS, "award") => {
            let (id, criterion) = (
                holder_id(rt, a(1)),
                rt.str_of(a(2)).unwrap_or("").to_owned(),
            );
            let granted = player_of_advancements(rt, a(0)).is_some_and(|uuid| {
                rt.host_mut()
                    .is_some_and(|h| h.award_criterion(uuid, &id, &criterion))
            });
            V::I(i32::from(granted))
        }
        // The listeners stay: a player back on the server is heard of again with them.
        (PLAYER_ADVANCEMENTS, "stopListening") => V::Void,
        (PROGRESS, _) => return progress_call(rt, name, a(0), a(1)),
        // One criterion's progress: granted when the game said so, or when it was obtained (a
        // mod's own, `new CriterionProgress(instant)`).
        (CRITERION_PROGRESS, "<init>") => {
            if desc == "(Ljava/time/Instant;)V" {
                rt.put_field(a(0), "obtained", a(1));
            }
            V::Void
        }
        (CRITERION_PROGRESS, "getObtained") => rt.get_field(a(0), "obtained"),
        (CRITERION_PROGRESS, "isDone") => V::I(i32::from(
            rt.get_field(a(0), "$done").i() != 0 || rt.get_field(a(0), "obtained").r() != 0,
        )),
        (LISTENER, "run") => {
            let (holder, criterion) = (
                rt.get_field(a(0), "advancement"),
                rt.get_field(a(0), "criterion"),
            );
            run(rt, a(1), holder, criterion);
            V::Void
        }
        // `SimpleCriterionTrigger`'s own: its listeners by player (a mod's trigger extends it).
        (TRIGGER | SIMPLE_TRIGGER, "addPlayerListener") => {
            let list = listeners(rt, a(0));
            let known = list_items(rt, list)
                .chunks(2)
                .any(|p| p[0] == a(1) && p[1] == a(2));
            if !known {
                list_push(rt, list, &[a(1), a(2)]);
            }
            V::Void
        }
        (TRIGGER | SIMPLE_TRIGGER, "removePlayerListener" | "removePlayerListeners") => {
            let list = listeners(rt, a(0));
            let (pa, one) = (a(1), name == "removePlayerListener");
            let kept: Vec<V> = list_items(rt, list)
                .chunks(2)
                .filter(|p| p[0] != pa || (one && p[1] != a(2)))
                .flatten()
                .copied()
                .collect();
            if let Some(Data::List(v)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
                *v = kept;
            }
            V::Void
        }
        (SIMPLE_TRIGGER, "trigger")
            if desc
                == "(Lnet/minecraft/server/level/ServerPlayer;Ljava/util/function/Predicate;)V" =>
        {
            trigger(rt, a(0), a(1), a(2));
            V::Void
        }
        _ => return None,
    })
}

/// A server's player's advancements (`ServerPlayer.getAdvancements()`): one object a player, made
/// with a listener on each criterion of the mods' triggers this runtime registered, as
/// `PlayerAdvancements` registers them as the player joins. A fake player's (NeoForge's
/// `FakePlayer`, a mod's own: Create's deployer) is one for them all, which grants nothing.
fn player_advancements(rt: &mut Rt, player: V) -> V {
    let Some(uuid) = player_uuid(rt, player) else {
        return rt.intern("compat:advancements/none".into(), |rt| {
            rt.new_object(PLAYER_ADVANCEMENTS)
        });
    };
    let pa = rt.intern(format!("compat:advancements/{uuid:x}"), |rt| {
        let pa = rt.new_object(PLAYER_ADVANCEMENTS);
        let u = crate::tags::uuid(rt, (uuid >> 64) as i64, uuid as i64);
        rt.put_field(pa, "$uuid", u);
        pa
    });
    // Once the game can tell the criteria: a host that can't (inside a hit) leaves it for later.
    if rt.get_field(pa, "$listening").i() == 0 {
        listen(rt, pa);
    }
    pa
}

/// The server's player a player object is: its own UUID, else the acting player's. None for a
/// fake player.
fn player_uuid(rt: &mut Rt, p: V) -> Option<u128> {
    if p.r() == 0 || rt.instance_of(p, crate::fake_players::FAKE_PLAYER) {
        return None;
    }
    match rt.get_field(p, "$uuid") {
        u if u.r() != 0 => Some(uuid_of(rt, u)),
        _ => rt.host().and_then(|h| h.player_id()),
    }
}

fn uuid_of(rt: &Rt, u: V) -> u128 {
    let (msb, lsb) = crate::tags::uuid_bits(rt, u);
    (u128::from(msb as u64) << 64) | u128::from(lsb as u64)
}

/// The player whose advancements `pa` are; None for a fake player's.
fn player_of_advancements(rt: &Rt, pa: V) -> Option<u128> {
    match rt.get_field(pa, "$uuid") {
        u if u.r() != 0 => Some(uuid_of(rt, u)),
        _ => None,
    }
}

/// `PlayerAdvancements.registerListeners`: a listener on each criterion of the game's
/// advancements whose trigger this runtime's mods registered (another bundle's triggers are its
/// own runtime's), its instance read from the criterion's conditions with the trigger's codec. A
/// criterion that can't be read is left without one, as vanilla leaves out an advancement it
/// can't read.
fn listen(rt: &mut Rt, pa: V) {
    let Some(criteria) = rt.host_mut().and_then(|h| h.mod_criteria()) else {
        return;
    };
    rt.put_field(pa, "$listening", V::I(1));
    for c in criteria {
        let trigger = crate::neo::registry_get(rt, "trigger_type", &c.trigger);
        if trigger.r() == 0 {
            continue;
        }
        let read =
            serde_json::from_str::<serde_json::Value>(&c.conditions).map_err(|e| e.to_string());
        let codec = crate::menus::virt(rt, CODEC, trigger, &[]);
        if dropped_exception(rt) || codec.r() == 0 {
            continue;
        }
        let instance = read.and_then(|json| {
            crate::codecs::decode(rt, codec, &json).map(|v| (v, json.get("player").is_some()))
        });
        let (instance, has_player) = match instance {
            Ok(v) => v,
            Err(e) => {
                if Rt::tracing() {
                    eprintln!(
                        "horizon-compat: criterion {} of advancement {} has no listener: {e}",
                        c.criterion, c.advancement
                    );
                }
                continue;
            }
        };
        let (holder, name) = (holder(rt, &c.advancement), rt.string(&c.criterion));
        let listener = crate::records::make(rt, LISTENER, &[instance, holder, name]);
        // Whether its `player` predicate is the game's to test (`SimpleCriterionTrigger.trigger`).
        rt.put_field(listener, "$player", V::I(i32::from(has_player)));
        crate::menus::virt(rt, ADD_LISTENER, trigger, &[pa, listener]);
        dropped_exception(rt);
    }
}

/// Whether the mod's code threw: reported (when tracing) and dropped, so the other criteria are
/// still listened for.
fn dropped_exception(rt: &mut Rt) -> bool {
    if rt.thrown.is_none() {
        return false;
    }
    rt.report_thrown("a criterion listener");
    true
}

/// The holder of the advancement `id`: one an advancement, so it compares as vanilla's do (by id).
fn holder(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:advancement/{id}"), |rt| {
        let rl = rt.string(id);
        crate::records::make(rt, HOLDER, &[rl, V::NULL])
    })
}

fn holder_id(rt: &Rt, holder: V) -> String {
    match holder.r() {
        0 => String::new(),
        _ => crate::neo::location(rt, rt.get_field(holder, "id")),
    }
}

/// `Listener.run(advancements)`: its criterion granted to the player, while it is open, as vanilla
/// keeps a listener only then.
fn run(rt: &mut Rt, pa: V, holder: V, criterion: V) {
    let Some(uuid) = player_of_advancements(rt, pa) else {
        return;
    };
    let (id, criterion) = (
        holder_id(rt, holder),
        rt.str_of(criterion).unwrap_or("").to_owned(),
    );
    let Some(host) = rt.host_mut() else { return };
    if host
        .advancement_progress(uuid, &id)
        .is_some_and(|p| open(&p, &criterion))
    {
        host.award_criterion(uuid, &id, &criterion);
    }
}

/// Whether `criterion` is open in `progress`: not granted, of an advancement not done.
fn open(progress: &AdvancementProgressView, criterion: &str) -> bool {
    !progress.done
        && progress
            .criteria
            .iter()
            .any(|(name, granted)| name == criterion && !granted)
}

/// `SimpleCriterionTrigger.trigger(player, test)`: the player's listeners on `trigger` whose
/// criterion is open, whose instance passes `test` and whose `player` predicate the player passes
/// run, once all are tested. An exception the test throws goes on to the mod.
fn trigger(rt: &mut Rt, trigger: V, player: V, test: V) {
    let pa = player_advancements(rt, player);
    let Some(uuid) = player_of_advancements(rt, pa) else {
        return;
    };
    let list = listeners(rt, trigger);
    let mine: Vec<V> = list_items(rt, list)
        .chunks(2)
        .filter(|p| p[0] == pa)
        .map(|p| p[1])
        .collect();
    let mut passed = Vec::new();
    for listener in mine {
        let id = holder_id(rt, rt.get_field(listener, "advancement"));
        let criterion = rt.get_field(listener, "criterion");
        let criterion = rt.str_of(criterion).unwrap_or("").to_owned();
        let is_open = rt
            .host_mut()
            .and_then(|h| h.advancement_progress(uuid, &id))
            .is_some_and(|p| open(&p, &criterion));
        if !is_open {
            continue;
        }
        let instance = rt.get_field(listener, "trigger");
        let hit = rt.call_functional(PREDICATE_TEST, test, &[instance]).i() != 0;
        if rt.thrown.is_some() {
            return;
        }
        let no_player_predicate = rt.get_field(listener, "$player").i() == 0;
        if hit
            && (no_player_predicate
                || rt
                    .host_mut()
                    .is_some_and(|h| h.criterion_player_holds(uuid, &id, &criterion)))
        {
            passed.push((id, criterion));
        }
    }
    for (id, criterion) in passed {
        if let Some(h) = rt.host_mut() {
            h.award_criterion(uuid, &id, &criterion);
        }
    }
}

/// A `SimpleCriterionTrigger`'s listeners, (advancements, listener) pairs one after the other.
fn listeners(rt: &mut Rt, trigger: V) -> V {
    match rt.get_field(trigger, "$listeners") {
        l if l.r() != 0 => l,
        _ => {
            let l = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
            rt.put_field(trigger, "$listeners", l);
            l
        }
    }
}

fn list_items(rt: &Rt, list: V) -> Vec<V> {
    match rt.heap.get(list.r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.clone(),
        _ => Vec::new(),
    }
}

fn list_push(rt: &mut Rt, list: V, items: &[V]) {
    if let Some(Data::List(v)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        v.extend_from_slice(items);
    }
}

/// `AdvancementProgress`'s questions, which the game answers for the player and the advancement
/// the progress was asked for (`getOrStartProgress`).
fn progress_call(rt: &mut Rt, name: &str, this: V, arg: V) -> Option<V> {
    let pa = rt.get_field(this, "$advancements");
    let id = rt.get_field(this, "$id");
    let id = rt.str_of(id).unwrap_or("").to_owned();
    let progress = match player_of_advancements(rt, pa) {
        Some(uuid) => rt
            .host_mut()
            .and_then(|h| h.advancement_progress(uuid, &id))
            .unwrap_or_default(),
        None => AdvancementProgressView::default(),
    };
    let names = |granted: bool| {
        progress
            .criteria
            .iter()
            .filter(|(_, g)| *g == granted)
            .map(|(n, _)| n.clone())
            .collect::<Vec<_>>()
    };
    Some(match name {
        "isDone" => V::I(i32::from(progress.done)),
        "hasProgress" => V::I(i32::from(progress.criteria.iter().any(|(_, g)| *g))),
        "getRemainingCriteria" | "getCompletedCriteria" => {
            let list: Vec<V> = names(name == "getCompletedCriteria")
                .iter()
                .map(|n| rt.string(n))
                .collect();
            rt.alloc("java/util/ArrayList", Data::List(list))
        }
        // One criterion's progress; null for a name the advancement lacks.
        "getCriterion" => {
            let wanted = rt.str_of(arg).unwrap_or("").to_owned();
            match progress.criteria.iter().find(|(n, _)| *n == wanted) {
                Some((_, granted)) => {
                    let c = rt.new_object(CRITERION_PROGRESS);
                    rt.put_field(c, "$done", V::I(i32::from(*granted)));
                    c
                }
                None => V::NULL,
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::host::MemoryHost;

    /// A mod's trigger as Create writes its own (`CriterionTriggerBase`): it implements
    /// `CriterionTrigger` itself, keeps its listeners by player, and when it fires runs each of the
    /// player's (`Listener.run`). Its instance reads from anything (`Codec.unit`).
    const PIPE: &str = "test/PipeTrigger";
    /// A mod's trigger extending vanilla's `SimpleCriterionTrigger`: only its codec is its own, the
    /// least charge its instance asks for (`{"min": 3}`).
    const CHARGE: &str = "test/ChargeTrigger";
    /// The test a charge trigger fires with: the player's charge reaches the instance's least.
    const CHARGED: &str = "test/Charged";
    const CODEC_ND: &str = "codec.()Lcom/mojang/serialization/Codec;";
    const ADD_ND: &str = "addPlayerListener.(Lnet/minecraft/server/PlayerAdvancements;Lnet/minecraft/advancements/CriterionTrigger$Listener;)V";
    const TEST_ND: &str = "test.(Ljava/lang/Object;)Z";
    const STEVE: u128 = 0x5157_e7e5;
    const ALEX: u128 = 0xa1e7;

    thread_local! {
        /// The pipe trigger's listeners: (advancements, listener).
        static PIPE_LISTENERS: std::cell::RefCell<Vec<(V, V)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn overrides(class: &str, key: &str) -> bool {
        matches!(
            (class, key),
            (PIPE, CODEC_ND) | (PIPE, ADD_ND) | (CHARGE, CODEC_ND) | (CHARGED, TEST_ND)
        )
    }

    fn call(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let key = rt.static_key(key);
        api::call(rt, key, args)
    }

    fn int_of(rt: &Rt, v: V) -> i32 {
        match v {
            V::I(i) => i,
            o => rt.get_field(o, "value").i(),
        }
    }

    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let class = rt.class_of(args[0]);
        if !overrides(class, key) {
            return api::call(rt, owner_key, args);
        }
        match (class, key) {
            (PIPE, CODEC_ND) => {
                let instance = rt.new_object("test/PipeTrigger$Instance");
                call(
                    rt,
                    "com/mojang/serialization/Codec.unit.(Ljava/lang/Object;)Lcom/mojang/serialization/Codec;",
                    &[instance],
                )
            }
            (PIPE, ADD_ND) => {
                PIPE_LISTENERS.with_borrow_mut(|l| l.push((args[1], args[2])));
                V::Void
            }
            (CHARGE, CODEC_ND) => {
                let int = crate::codecs::get_static(rt, "com/mojang/serialization/Codec", "INT")
                    .expect("Codec.INT");
                let min = rt.string("min");
                let field = call(
                    rt,
                    "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;",
                    &[int, min],
                );
                call(
                    rt,
                    "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;",
                    &[field],
                )
            }
            (CHARGED, TEST_ND) => {
                let charge = rt.get_field(args[0], "$charge").i();
                V::I(i32::from(int_of(rt, args[1]) <= charge))
            }
            _ => unreachable!(),
        }
    }

    /// The two triggers registered as Create registers its own (`AllTriggers.register`).
    fn runtime() -> (Rt, V, V) {
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(PIPE, "java/lang/Object");
        rt.supers.insert(CHARGE, SIMPLE_TRIGGER);
        rt.supers.insert(CHARGED, "java/lang/Object");
        let registry = api::get_static(
            &mut rt,
            "net/minecraft/core/registries/BuiltInRegistries.TRIGGER_TYPES",
        );
        let mut triggers = Vec::new();
        for (class, id) in [(PIPE, "test:pipe_connected"), (CHARGE, "test:charged")] {
            let t = rt.new_object(class);
            let id = rt.string(id);
            call(
                &mut rt,
                "net/minecraft/core/Registry.register.(Lnet/minecraft/core/Registry;Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)Ljava/lang/Object;",
                &[registry, id, t],
            );
            triggers.push(t);
        }
        PIPE_LISTENERS.with_borrow_mut(Vec::clear);
        (rt, triggers[0], triggers[1])
    }

    fn criteria(list: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
        list.iter()
            .map(|(n, t, c)| ((*n).to_owned(), (*t).to_owned(), (*c).to_owned()))
            .collect()
    }

    fn get_advancements(rt: &mut Rt, player: V) -> V {
        call(
            rt,
            "net/minecraft/server/level/ServerPlayer.getAdvancements.()Lnet/minecraft/server/PlayerAdvancements;",
            &[player],
        )
    }

    /// The pipe trigger fired for `player`, as Create's `CriterionTriggerBase.trigger` fires: each of
    /// the player's listeners runs.
    fn fire_pipe(rt: &mut Rt, player: V) {
        let pa = get_advancements(rt, player);
        let mine: Vec<V> = PIPE_LISTENERS.with_borrow(|l| {
            l.iter()
                .filter(|(p, _)| *p == pa)
                .map(|(_, l)| *l)
                .collect()
        });
        for l in mine {
            call(
                rt,
                "net/minecraft/advancements/CriterionTrigger$Listener.run.(Lnet/minecraft/server/PlayerAdvancements;)V",
                &[l, pa],
            );
        }
    }

    fn is_done(rt: &mut Rt, player: V, id: &str) -> bool {
        let server = rt.intern("compat:server".into(), |rt| {
            rt.new_object("net/minecraft/server/MinecraftServer")
        });
        let manager = call(
            rt,
            "net/minecraft/server/MinecraftServer.getAdvancements.()Lnet/minecraft/server/ServerAdvancementManager;",
            &[server],
        );
        let id = rt.string(id);
        let holder = call(
            rt,
            "net/minecraft/server/ServerAdvancementManager.get.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/advancements/AdvancementHolder;",
            &[manager, id],
        );
        assert_ne!(holder.r(), 0, "the server has it");
        let pa = get_advancements(rt, player);
        let progress = call(
            rt,
            "net/minecraft/server/PlayerAdvancements.getOrStartProgress.(Lnet/minecraft/advancements/AdvancementHolder;)Lnet/minecraft/advancements/AdvancementProgress;",
            &[pa, holder],
        );
        call(
            rt,
            "net/minecraft/advancements/AdvancementProgress.isDone.()Z",
            &[progress],
        )
        .i() != 0
    }

    #[test]
    fn a_mods_own_trigger_hears_of_each_player_and_grants_open_criteria() {
        let (mut rt, _, _) = runtime();
        let mut host = MemoryHost {
            player_id: Some(STEVE),
            ..MemoryHost::default()
        };
        host.advancements = vec![
            (
                "test:first_pipe".into(),
                criteria(&[("0", "test:pipe_connected", "{}")]),
            ),
            // Two criteria, both required: the pipe's alone doesn't do it.
            (
                "test:pipe_and_gold".into(),
                criteria(&[
                    ("pipe", "test:pipe_connected", "{}"),
                    ("gold", "minecraft:inventory_changed", "{}"),
                ]),
            ),
            // Another runtime's trigger, and vanilla's: no listener here.
            (
                "test:elsewhere".into(),
                criteria(&[("0", "other:trigger", "{}")]),
            ),
        ];
        let (steve, alex) = (
            crate::menus::server_player(&mut rt, STEVE),
            crate::menus::server_player(&mut rt, ALEX),
        );
        rt.with_host(&mut host, |rt| {
            let pa = get_advancements(rt, steve);
            assert_eq!(get_advancements(rt, steve), pa, "one object a player");
            assert_ne!(get_advancements(rt, alex), pa);
            let heard: Vec<(V, String, String)> = PIPE_LISTENERS
                .with_borrow(|l| l.clone())
                .into_iter()
                .map(|(p, l)| {
                    let (holder, name) =
                        (rt.get_field(l, "advancement"), rt.get_field(l, "criterion"));
                    (
                        p,
                        holder_id(rt, holder),
                        rt.str_of(name).unwrap_or("").to_owned(),
                    )
                })
                .collect();
            assert_eq!(
                heard,
                vec![
                    (pa, "test:first_pipe".to_owned(), "0".to_owned()),
                    (pa, "test:pipe_and_gold".to_owned(), "pipe".to_owned()),
                    (
                        get_advancements(rt, alex),
                        "test:first_pipe".to_owned(),
                        "0".to_owned()
                    ),
                    (
                        get_advancements(rt, alex),
                        "test:pipe_and_gold".to_owned(),
                        "pipe".to_owned()
                    ),
                ],
                "a listener on each of the pipe's criteria, once a player"
            );
            assert!(!is_done(rt, steve, "test:first_pipe"));
            fire_pipe(rt, steve);
            assert!(is_done(rt, steve, "test:first_pipe"));
            assert!(
                !is_done(rt, steve, "test:pipe_and_gold"),
                "the gold is still to get"
            );
            assert!(!is_done(rt, alex, "test:first_pipe"), "Alex's own");
            // The granted criteria are closed: firing again grants nothing more.
            fire_pipe(rt, steve);
        });
        assert_eq!(
            host.granted,
            vec![
                (STEVE, "test:first_pipe".to_owned(), "0".to_owned()),
                (STEVE, "test:pipe_and_gold".to_owned(), "pipe".to_owned()),
            ]
        );
        // A revoked criterion is open again, and heard of.
        host.granted.retain(|(_, a, _)| a != "test:first_pipe");
        rt.with_host(&mut host, |rt| fire_pipe(rt, steve));
        assert_eq!(
            host.granted.last(),
            Some(&(STEVE, "test:first_pipe".to_owned(), "0".to_owned()))
        );
        assert_eq!(host.granted.len(), 2);
    }

    #[test]
    fn a_vanilla_simple_trigger_tests_each_listener_and_the_player_predicate() {
        let (mut rt, _, charge) = runtime();
        let mut host = MemoryHost {
            player_id: Some(STEVE),
            ..MemoryHost::default()
        };
        host.advancements = vec![
            (
                "test:charged".into(),
                criteria(&[
                    ("low", "test:charged", r#"{"min": 3}"#),
                    ("high", "test:charged", r#"{"min": 8}"#),
                ]),
            ),
            // Charged enough, but only for a player passing its `player` predicate.
            (
                "test:picky".into(),
                criteria(&[(
                    "0",
                    "test:charged",
                    r#"{"min": 1, "player": [{"condition": "minecraft:entity_properties"}]}"#,
                )]),
            ),
        ];
        host.player_fails
            .push((STEVE, "test:picky".into(), "0".into()));
        let steve = crate::menus::server_player(&mut rt, STEVE);
        let fire = |rt: &mut Rt, charge_now: i32| {
            let test = rt.new_object(CHARGED);
            rt.put_field(test, "$charge", V::I(charge_now));
            call(
                rt,
                "net/minecraft/advancements/critereon/SimpleCriterionTrigger.trigger.(Lnet/minecraft/server/level/ServerPlayer;Ljava/util/function/Predicate;)V",
                &[charge, steve, test],
            );
        };
        rt.with_host(&mut host, |rt| fire(rt, 5));
        assert_eq!(
            host.granted,
            vec![(STEVE, "test:charged".to_owned(), "low".to_owned())],
            "the charge reaches 3, not 8; the picky one's player fails"
        );
        host.player_fails.clear();
        rt.with_host(&mut host, |rt| fire(rt, 9));
        assert_eq!(
            host.granted,
            vec![
                (STEVE, "test:charged".to_owned(), "low".to_owned()),
                (STEVE, "test:charged".to_owned(), "high".to_owned()),
                (STEVE, "test:picky".to_owned(), "0".to_owned()),
            ]
        );
        rt.with_host(&mut host, |rt| {
            assert!(is_done(rt, steve, "test:charged"));
            // `removePlayerListeners`: the player is no longer heard of.
            let pa = get_advancements(rt, steve);
            call(rt, "net/minecraft/advancements/critereon/SimpleCriterionTrigger.removePlayerListeners.(Lnet/minecraft/server/PlayerAdvancements;)V", &[charge, pa]);
            let list = listeners(rt, charge);
            assert!(list_items(rt, list).is_empty());
        });
    }

    #[test]
    fn a_mod_awards_and_reads_progress_by_hand() {
        let (mut rt, _, _) = runtime();
        let mut host = MemoryHost {
            player_id: Some(STEVE),
            ..MemoryHost::default()
        };
        host.advancements = vec![(
            "test:book".into(),
            criteria(&[
                ("read", "minecraft:impossible", "{}"),
                ("kept", "minecraft:impossible", "{}"),
            ]),
        )];
        let steve = crate::menus::server_player(&mut rt, STEVE);
        rt.with_host(&mut host, |rt| {
            let pa = get_advancements(rt, steve);
            let manager = call(rt, "net/minecraft/server/MinecraftServer.getAdvancements.()Lnet/minecraft/server/ServerAdvancementManager;", &[V::NULL]);
            let missing = rt.string("test:missing");
            let none = call(rt, "net/minecraft/server/ServerAdvancementManager.get.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/advancements/AdvancementHolder;", &[manager, missing]);
            assert_eq!(none.r(), 0, "an advancement the server lacks");
            let id = rt.string("test:book");
            let book = call(rt, "net/minecraft/server/ServerAdvancementManager.get.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/advancements/AdvancementHolder;", &[manager, id]);
            let progress = call(rt, "net/minecraft/server/PlayerAdvancements.getOrStartProgress.(Lnet/minecraft/advancements/AdvancementHolder;)Lnet/minecraft/advancements/AdvancementProgress;", &[pa, book]);
            // The common way to grant a whole advancement: each remaining criterion awarded.
            let remaining = call(rt, "net/minecraft/advancements/AdvancementProgress.getRemainingCriteria.()Ljava/lang/Iterable;", &[progress]);
            let names: Vec<String> = list_items(rt, remaining).iter().map(|v| rt.str_of(*v).unwrap_or("").to_owned()).collect();
            assert_eq!(names, ["read", "kept"]);
            let read = rt.string("read");
            let award = "net/minecraft/server/PlayerAdvancements.award.(Lnet/minecraft/advancements/AdvancementHolder;Ljava/lang/String;)Z";
            assert_eq!(call(rt, award, &[pa, book, read]).i(), 1);
            assert_eq!(call(rt, award, &[pa, book, read]).i(), 0, "had it");
            let has = call(rt, "net/minecraft/advancements/AdvancementProgress.hasProgress.()Z", &[progress]);
            assert_eq!(has.i(), 1, "the progress reads the game's, as it is now");
            let criterion = call(rt, "net/minecraft/advancements/AdvancementProgress.getCriterion.(Ljava/lang/String;)Lnet/minecraft/advancements/CriterionProgress;", &[progress, read]);
            assert_eq!(call(rt, "net/minecraft/advancements/CriterionProgress.isDone.()Z", &[criterion]).i(), 1);
            assert_eq!(call(rt, "net/minecraft/advancements/AdvancementProgress.isDone.()Z", &[progress]).i(), 0);
            // A fake player's advancements grant nothing.
            let fake = rt.new_object(crate::fake_players::FAKE_PLAYER);
            let fake_pa = get_advancements(rt, fake);
            let kept = rt.string("kept");
            assert_eq!(call(rt, award, &[fake_pa, book, kept]).i(), 0);
        });
        assert_eq!(
            host.granted,
            vec![(STEVE, "test:book".to_owned(), "read".to_owned())]
        );
    }
}
