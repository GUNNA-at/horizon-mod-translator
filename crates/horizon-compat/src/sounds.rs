//! Sounds a mod plays: its own `SoundEvent`s (`SoundEvent.createVariableRangeEvent(id)`, registered
//! like any registry value), vanilla's (`SoundEvents.WOODEN_TRAPDOOR_OPEN`: the id each field
//! registers, from `vanilla_sounds.rs`), and the level's and entities' `playSound` family, which
//! hand the event's id, the place, source, volume and pitch to the game
//! ([`crate::host::Host::play_sound`]).
//!
//! As in vanilla, a server's level plays `playSound` for the players near it and a client's level
//! plays `playLocalSound`; the other two are no-ops (a client's `playSound` plays only the local
//! player's own). One difference: vanilla's server leaves out the player named in `playSound`
//! (their client played it already), but here only the server runs a mod's interactions, so that
//! player hears it from the server too.
use crate::host::{InstanceSound, Sound};
use crate::{Rt, V};

const SOUND_EVENT: &str = "net/minecraft/sounds/SoundEvent";
const SOUND_EVENTS: &str = "net/minecraft/sounds/SoundEvents";
const LOCAL_PLAYER: &str = "net/minecraft/client/player/LocalPlayer";
/// `SoundSource.NEUTRAL` and `PLAYERS`' ordinals: what an entity's and a player's sounds play under.
const NEUTRAL: usize = 6;
const PLAYERS: usize = 7;

/// A `SoundEvent` for `id`, with a fixed range when given (`createFixedRangeEvent`).
pub(crate) fn sound_event(rt: &mut Rt, id: &str, range: Option<f32>) -> V {
    let e = rt.new_object(SOUND_EVENT);
    let location = rt.string(id);
    rt.put_field(e, "$location", location);
    if let Some(r) = range {
        rt.put_field(e, "$range", V::F(r));
    }
    e
}

/// `SoundEvents.FIELD`: vanilla's event (or its holder) for the id that field registers. One object
/// per field: a mod may compare them.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != SOUND_EVENTS {
        return None;
    }
    let i = crate::vanilla_sounds::SOUND_EVENTS.binary_search_by(|(f, _, _)| (*f).cmp(field)).ok()?;
    let (_, path, holder) = crate::vanilla_sounds::SOUND_EVENTS[i];
    let id = format!("minecraft:{path}");
    let event = rt.intern(format!("sound/{id}"), |rt| sound_event(rt, &id, None));
    Some(if holder { rt.intern(format!("sound-holder/{id}"), |rt| crate::neo::registry_holder(rt, "sound_event", &id, event)) } else { event })
}

/// The id a `SoundEvent`, or a holder of one, plays.
pub(crate) fn sound_id(rt: &Rt, v: V) -> Option<String> {
    let held = rt.get_field(v, "$value");
    let event = if rt.class_of(v) != SOUND_EVENT && held.r() != 0 { held } else { v };
    if let Some(id) = rt.str_of(rt.get_field(event, "$location")) {
        return Some(id.to_owned());
    }
    // A holder not bound yet: the id it stands for.
    rt.str_of(rt.get_field(v, "$id")).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Plays `sound` (an event or a holder of one) under `source` (a `SoundSource`, or its ordinal).
fn play(rt: &mut Rt, sound: V, at: [f64; 3], source: usize, volume: f32, pitch: f32) {
    let Some(event) = sound_id(rt, sound) else { return };
    play_id(rt, Sound { event, at, source, volume, pitch });
}

/// Plays `sound` on the host; a host that only reads (a screen's drawing and input) has it played
/// at the client's next tick.
pub(crate) fn play_id(rt: &mut Rt, sound: Sound) {
    if crate::screens::defer_sound(rt, &sound) {
        return;
    }
    if let Some(h) = rt.host_mut() {
        h.play_sound(&sound);
    }
}

const SIMPLE_SOUND: &str = "net/minecraft/client/resources/sounds/SimpleSoundInstance";
const SOUND_MANAGER: &str = "net/minecraft/client/sounds/SoundManager";
/// `SoundSource.MASTER`'s ordinal: a UI sound's.
const MASTER: usize = 0;

/// A `SimpleSoundInstance` of `sound` (an event or a holder): where it plays (None: relative to the
/// listener, a UI sound), its source, volume and pitch.
fn instance(rt: &mut Rt, sound: V, at: Option<[f64; 3]>, source: usize, volume: f32, pitch: f32) -> V {
    let i = rt.new_object(SIMPLE_SOUND);
    let id = sound_id(rt, sound).unwrap_or_default();
    let id = rt.string(&id);
    rt.put_field(i, "$event", id);
    if let Some([x, y, z]) = at {
        for (f, v) in ["$x", "$y", "$z"].into_iter().zip([x, y, z]) {
            rt.put_field(i, f, V::D(v));
        }
    }
    rt.put_field(i, "$source", V::I(source as i32));
    rt.put_field(i, "$volume", V::F(volume));
    rt.put_field(i, "$pitch", V::F(pitch));
    i
}

const SOUND_INSTANCE: &str = "net/minecraft/client/resources/sounds/SoundInstance";
const ABSTRACT_INSTANCE: &str = "net/minecraft/client/resources/sounds/AbstractSoundInstance";
const ABSTRACT_TICKABLE: &str = "net/minecraft/client/resources/sounds/AbstractTickableSoundInstance";
const TICKABLE: &str = "net/minecraft/client/resources/sounds/TickableSoundInstance";
const ATTENUATION: &str = "net/minecraft/client/resources/sounds/SoundInstance$Attenuation";
/// The mods' own instances the game plays, each with its id (`$soundId`).
const PLAYING: &str = "compat:mod-sounds";

/// `new AbstractSoundInstance(event or location, source, random)`, as a mod's own instance class
/// starts (Create's `ContinuousSound`, a fan's wind): vanilla's fields, under their own names,
/// which the mod's code reads and writes too (`this.looping = true`).
fn instance_init(rt: &mut Rt, i: V, desc: &str, args: &[V]) {
    let a = |k: usize| args.get(k).copied().unwrap_or_default();
    let id = if desc.starts_with("(Lnet/minecraft/sounds/SoundEvent;") { sound_id(rt, a(1)) } else { rt.str_of(a(1)).map(str::to_owned) };
    let location = rt.string(&id.unwrap_or_default());
    let linear = crate::api::api_enum(rt, ATTENUATION, "LINEAR", 1);
    let fields = [
        ("location", location),
        ("source", a(2)),
        ("random", a(3)),
        ("volume", V::F(1.0)),
        ("pitch", V::F(1.0)),
        ("attenuation", linear),
        ("x", V::D(0.0)),
        ("y", V::D(0.0)),
        ("z", V::D(0.0)),
        ("looping", V::I(0)),
        ("delay", V::I(0)),
        ("relative", V::I(0)),
    ];
    for (f, v) in fields {
        rt.put_field(i, f, v);
    }
}

/// A sound instance's own calls (`AbstractSoundInstance`'s getters, `AbstractTickableSoundInstance`'s
/// `stop`), reading the fields its constructor set and the mod's code changes.
fn instance_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |k: usize| args.get(k).copied().unwrap_or_default();
    let field = |rt: &Rt, f: &'static str, zero: V| match rt.get_field(a(0), f) {
        V::Void => zero,
        v => v,
    };
    Some(match name {
        "<init>" if owner != SOUND_INSTANCE && owner != TICKABLE => {
            instance_init(rt, a(0), desc, args);
            V::Void
        }
        "createUnseededRandom" => rt.new_object("net/minecraft/util/RandomSource"),
        "getLocation" => field(rt, "location", V::NULL),
        "getSource" => field(rt, "source", V::NULL),
        "getVolume" => field(rt, "volume", V::F(1.0)),
        "getPitch" => field(rt, "pitch", V::F(1.0)),
        "getX" => field(rt, "x", V::D(0.0)),
        "getY" => field(rt, "y", V::D(0.0)),
        "getZ" => field(rt, "z", V::D(0.0)),
        "isLooping" => field(rt, "looping", V::I(0)),
        "getDelay" => field(rt, "delay", V::I(0)),
        "isRelative" => field(rt, "relative", V::I(0)),
        "getAttenuation" => match rt.get_field(a(0), "attenuation") {
            V::Void => crate::api::api_enum(rt, ATTENUATION, "LINEAR", 1),
            v => v,
        },
        "canStartSilent" => V::I(0),
        "canPlaySound" => V::I(1),
        "isStopped" => field(rt, "stopped", V::I(0)),
        // `AbstractTickableSoundInstance.stop()`: it stops, and loops no more.
        "stop" if desc == "()V" => {
            rt.put_field(a(0), "stopped", V::I(1));
            rt.put_field(a(0), "looping", V::I(0));
            V::Void
        }
        _ => return None,
    })
}

/// `instance.name()` as the JVM dispatches it: the mod's override, else the getters above.
fn ask(rt: &mut Rt, i: V, key: &'static str) -> V {
    crate::menus::virt(rt, key, i, &[])
}

const CAN_PLAY: &str = "net/minecraft/client/resources/sounds/SoundInstance.canPlaySound.()Z";
const GET_LOCATION: &str = "net/minecraft/client/resources/sounds/SoundInstance.getLocation.()Lnet/minecraft/resources/ResourceLocation;";
const GET_SOURCE: &str = "net/minecraft/client/resources/sounds/SoundInstance.getSource.()Lnet/minecraft/sounds/SoundSource;";
const GET_VOLUME: &str = "net/minecraft/client/resources/sounds/SoundInstance.getVolume.()F";
const GET_PITCH: &str = "net/minecraft/client/resources/sounds/SoundInstance.getPitch.()F";
const GET_X: &str = "net/minecraft/client/resources/sounds/SoundInstance.getX.()D";
const GET_Y: &str = "net/minecraft/client/resources/sounds/SoundInstance.getY.()D";
const GET_Z: &str = "net/minecraft/client/resources/sounds/SoundInstance.getZ.()D";
const IS_LOOPING: &str = "net/minecraft/client/resources/sounds/SoundInstance.isLooping.()Z";
const GET_DELAY: &str = "net/minecraft/client/resources/sounds/SoundInstance.getDelay.()I";
const GET_ATTENUATION: &str = "net/minecraft/client/resources/sounds/SoundInstance.getAttenuation.()Lnet/minecraft/client/resources/sounds/SoundInstance$Attenuation;";
const IS_RELATIVE: &str = "net/minecraft/client/resources/sounds/SoundInstance.isRelative.()Z";
const CAN_START_SILENT: &str = "net/minecraft/client/resources/sounds/SoundInstance.canStartSilent.()Z";
const TICK: &str = "net/minecraft/client/resources/sounds/TickableSoundInstance.tick.()V";
const IS_STOPPED: &str = "net/minecraft/client/resources/sounds/TickableSoundInstance.isStopped.()Z";

/// Where instance `i` is now (`getX()`, `getY()`, `getZ()`).
fn place(rt: &mut Rt, i: V) -> [f64; 3] {
    [ask(rt, i, GET_X).d(), ask(rt, i, GET_Y).d(), ask(rt, i, GET_Z).d()]
}

/// The mods' instances playing now.
fn playing(rt: &Rt) -> Vec<V> {
    rt.interned.get(PLAYING).map(|l| crate::jutil::elements(rt, *l)).unwrap_or_default()
}

fn set_playing(rt: &mut Rt, list: Vec<V>) {
    let l = crate::jutil::new_list(rt, list);
    rt.interned.insert(PLAYING.into(), l);
}

fn is_playing(rt: &Rt, i: V) -> bool {
    i.r() != 0 && playing(rt).contains(&i)
}

/// `SoundManager.play(instance)` of a mod's own instance, as vanilla's `SoundEngine.play` plays
/// it: when it can play (`canPlaySound`), with what its getters give, `start_delay` ticks from
/// now (`playDelayed`). One already playing isn't started twice.
fn start(rt: &mut Rt, i: V, start_delay: i32) {
    if i.r() == 0 || is_playing(rt, i) || ask(rt, i, CAN_PLAY).i() == 0 {
        return;
    }
    let location = ask(rt, i, GET_LOCATION);
    let event = rt.str_of(location).unwrap_or("").to_owned();
    let source = ask(rt, i, GET_SOURCE);
    let source = ordinal(rt, source);
    let at = place(rt, i);
    let (volume, pitch) = (ask(rt, i, GET_VOLUME).f(), ask(rt, i, GET_PITCH).f());
    let attenuation = ask(rt, i, GET_ATTENUATION);
    let sound = InstanceSound {
        sound: Sound { event, at, source, volume, pitch },
        looping: ask(rt, i, IS_LOOPING).i() != 0,
        delay: ask(rt, i, GET_DELAY).i(),
        start_delay,
        linear: rt.get_field(attenuation, "$ordinal").i() == 1,
        relative: ask(rt, i, IS_RELATIVE).i() != 0,
        can_start_silent: ask(rt, i, CAN_START_SILENT).i() != 0,
    };
    if rt.thrown.is_some() {
        rt.report_thrown("SoundManager.play");
        return;
    }
    let started = rt.host_mut().and_then(|h| h.start_sound(&sound));
    let class = rt.class_of(i);
    crate::glue::trace_some(rt, "mod-sound", &format!("{class}/{}", sound.sound.event), 24, |_| {
        let s = &sound.sound;
        let state = if started.is_some() { "started" } else { "not started by the game" };
        format!("client: {class}'s own sound {} (volume {}, pitch {}, looping {}) {state}", s.event, s.volume, s.pitch, sound.looping)
    });
    let Some(id) = started else { return };
    rt.put_field(i, "$soundId", V::J(id as i64));
    let mut list = playing(rt);
    list.push(i);
    set_playing(rt, list);
}

/// `SoundManager.stop(instance)`: the game stops it, and it is let go.
fn stop(rt: &mut Rt, i: V) {
    if !is_playing(rt, i) {
        return;
    }
    let id = rt.get_field(i, "$soundId").j() as u64;
    if let Some(h) = rt.host_mut() {
        h.stop_sound(id);
    }
    let list = playing(rt).into_iter().filter(|&p| p != i).collect();
    set_playing(rt, list);
}

/// The sound manager's tick for the mods' own instances (`SoundEngine.tick`): a ticking one
/// stops if it can't play any more; else it ticks, and stops if it stopped itself, or gives the
/// game where it is with its volume and pitch. One the game no longer plays (it ended) is let go.
pub(crate) fn tick(rt: &mut Rt) {
    let list = playing(rt);
    if list.is_empty() {
        return;
    }
    let mut kept = Vec::with_capacity(list.len());
    for i in list {
        let id = rt.get_field(i, "$soundId").j() as u64;
        if rt.instance_of(i, ABSTRACT_TICKABLE) || rt.instance_of(i, TICKABLE) {
            let stopped = ask(rt, i, CAN_PLAY).i() == 0 || {
                ask(rt, i, TICK);
                ask(rt, i, IS_STOPPED).i() != 0
            };
            if rt.thrown.is_some() {
                rt.report_thrown("TickableSoundInstance.tick");
            }
            if stopped {
                if let Some(h) = rt.host_mut() {
                    h.stop_sound(id);
                }
                continue;
            }
        }
        let at = place(rt, i);
        let (volume, pitch) = (ask(rt, i, GET_VOLUME).f(), ask(rt, i, GET_PITCH).f());
        if rt.host_mut().is_some_and(|h| h.update_sound(id, at, volume, pitch)) {
            kept.push(i);
        }
    }
    set_playing(rt, kept);
}

/// The client's sounds a mod plays itself (`Minecraft.getSoundManager().play(instance)`): the
/// game's own instances (`SimpleSoundInstance.forUI`, one at a place), played once, and the mod's
/// own instance classes, which the game plays as they say, ticking them ([`tick`]).
fn manager_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        ("net/minecraft/client/Minecraft", "getSoundManager") => rt.intern("compat:sound-manager".into(), |rt| rt.new_object(SOUND_MANAGER)),
        // `forUI(sound, pitch[, volume])`: volume 0.25 unless given; relative, so at the listener.
        (SIMPLE_SOUND, "forUI") => {
            let volume = if desc.contains("FF)") { a(2).f() } else { 0.25 };
            instance(rt, a(0), None, MASTER, volume, a(1).f())
        }
        // `new SimpleSoundInstance(event, source, volume, pitch, random, pos)` and the one at x, y, z.
        (SIMPLE_SOUND, "<init>") if desc.starts_with("(Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FFLnet/minecraft/util/RandomSource;") => {
            let at = if desc.contains("BlockPos") {
                crate::api::pos_of(rt, a(6)).map(|c| f64::from(c) + 0.5)
            } else {
                [a(6).d(), a(7).d(), a(8).d()]
            };
            let made = instance(rt, a(1), Some(at), ordinal(rt, a(2)), a(3).f(), a(4).f());
            for f in ["$event", "$x", "$y", "$z", "$source", "$volume", "$pitch"] {
                let v = rt.get_field(made, f);
                rt.put_field(a(0), f, v);
            }
            V::Void
        }
        (SOUND_MANAGER, "play" | "playDelayed" | "queueTickingSound") if rt.is_mod_class(rt.class_of(a(1))) => {
            start(rt, a(1), if name == "playDelayed" { a(2).i() } else { 0 });
            V::Void
        }
        (SOUND_MANAGER, "stop") if desc == "(Lnet/minecraft/client/resources/sounds/SoundInstance;)V" => {
            stop(rt, a(1));
            V::Void
        }
        (SOUND_MANAGER, "isActive") => V::I(i32::from(is_playing(rt, a(1)))),
        (SOUND_MANAGER, "play" | "playDelayed") => {
            let i = a(1);
            let Some(event) = rt.str_of(rt.get_field(i, "$event")).map(str::to_owned) else {
                let class = rt.class_of(i);
                crate::glue::trace_once(rt, "sound-instance", |_| format!("client: a mod's own sound instance ({class}) isn't played"));
                return Some(V::Void);
            };
            let at = match rt.get_field(i, "$x") {
                V::D(x) => [x, rt.get_field(i, "$y").d(), rt.get_field(i, "$z").d()],
                // Relative to the listener: at the player's eyes.
                _ => match rt.host().and_then(|h| h.player_place()) {
                    Some(([x, y, z], _, _)) => [x, y + 1.62, z],
                    None => [0.0; 3],
                },
            };
            let source = rt.get_field(i, "$source").i().max(0) as usize;
            let (volume, pitch) = (rt.get_field(i, "$volume").f(), rt.get_field(i, "$pitch").f());
            play_id(rt, Sound { event, at, source, volume, pitch });
            V::Void
        }
        (SOUND_MANAGER, "stop" | "pause" | "resume" | "tick" | "destroy" | "reload") => V::Void,
        (SOUND_INSTANCE | ABSTRACT_INSTANCE | ABSTRACT_TICKABLE | TICKABLE, _) => return instance_call(rt, owner, name, desc, args),
        _ => return None,
    })
}

fn ordinal(rt: &Rt, source: V) -> usize {
    rt.get_field(source, "$ordinal").i().max(0) as usize
}

/// A level's `playSound`, `playSeededSound` and `playLocalSound` (`name`, `desc`; the level first
/// in `args`).
pub fn level_sound(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let client = crate::neo::is_client(rt);
    let local = name == "playLocalSound";
    // A server plays no local sound; a client plays a `playSound` only for its own player.
    let plays = if local { client } else { !client || rt.class_of(a(1)) == LOCAL_PLAYER };
    if !plays {
        return V::Void;
    }
    let params = crate::jlang::kinds(desc);
    let center = |rt: &Rt, p: V| crate::api::pos_of(rt, p).map(|c| f64::from(c) + 0.5);
    // Where it plays, and where its sound is in `args` (its source next, then volume and pitch
    // when the overload has them).
    let first = if local { 1 } else { 2 };
    let (at, sound) = if desc[1..].starts_with("D") || desc.starts_with("(Lnet/minecraft/world/entity/player/Player;D") {
        ([a(first).d(), a(first + 1).d(), a(first + 2).d()], first + 3)
    } else if desc.contains("Lnet/minecraft/core/BlockPos;") {
        (center(rt, a(first)), first + 1)
    } else {
        let Some(at) = crate::entities::position(rt, a(first)) else { return V::Void };
        (at, first + 1)
    };
    // Volume and pitch follow the source, as floats.
    let (volume, pitch) = match params.get(sound + 1) {
        Some('F') => (a(sound + 2).f(), a(sound + 3).f()),
        _ => (1.0, 1.0),
    };
    let source = ordinal(rt, a(sound + 1));
    play(rt, a(sound), at, source, volume, pitch);
    V::Void
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, SOUND_EVENT | SIMPLE_SOUND | SOUND_MANAGER | SOUND_INSTANCE | ABSTRACT_INSTANCE | ABSTRACT_TICKABLE | TICKABLE)
        || matches!(name, "getSoundManager" | "playSound")
}

/// `SoundEvent`'s own calls and `Entity.playSound`; None for anything else.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if let Some(v) = manager_call(rt, owner, name, desc, args) {
        return Some(v);
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (SOUND_EVENT, "createVariableRangeEvent") => {
            let id = crate::neo::location(rt, a(0));
            sound_event(rt, &id, None)
        }
        (SOUND_EVENT, "createFixedRangeEvent") => {
            let id = crate::neo::location(rt, a(0));
            sound_event(rt, &id, Some(a(1).f()))
        }
        (SOUND_EVENT, "getLocation" | "location") => {
            let id = sound_id(rt, a(0)).unwrap_or_default();
            rt.string(&id)
        }
        // How far it carries: its fixed range, else 16 blocks, more when louder.
        (SOUND_EVENT, "getRange") => match rt.get_field(a(0), "$range") {
            V::F(r) => V::F(r),
            _ => V::F(if a(1).f() > 1.0 { 16.0 * a(1).f() } else { 16.0 }),
        },
        // `Entity.playSound(event[, volume, pitch])`, at the entity: a server's for the players near
        // it, a client's only for its own player (vanilla's `level.playSound(null, ...)`, or
        // `LocalPlayer`'s own). No entity here is silent (`isSilent` answers false).
        (_, "playSound") if desc.starts_with("(Lnet/minecraft/sounds/SoundEvent;") && !crate::api::is_level(owner) => {
            let e = a(0);
            let player = rt.class_of(e) == LOCAL_PLAYER;
            if crate::neo::is_client(rt) && !player {
                return Some(V::Void);
            }
            let Some(at) = crate::entities::position(rt, e) else { return Some(V::Void) };
            let source = if player || owner.ends_with("Player") { PLAYERS } else { NEUTRAL };
            let (volume, pitch) = if args.len() > 2 { (a(2).f(), a(3).f()) } else { (1.0, 1.0) };
            play(rt, a(1), at, source, volume, pitch);
            V::Void
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn sound(event: &str, at: [f64; 3], source: usize, volume: f32, pitch: f32) -> Sound {
        Sound { event: event.to_owned(), at, source, volume, pitch }
    }

    /// A mod's own event, registered and played at a block (at its centre), and vanilla ones by
    /// field (the id the field registers, not its name), as an event and as a holder.
    #[test]
    fn a_mods_sound_and_vanilla_ones_reach_the_game_with_their_ids() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let id = rt.string("create:contraption_assemble");
        let event = crate::api::call(&mut rt, "net/minecraft/sounds/SoundEvent.createVariableRangeEvent.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/sounds/SoundEvent;", &[id]);
        let trapdoor = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundEvents.WOODEN_TRAPDOOR_OPEN");
        let click = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundEvents.UI_BUTTON_CLICK");
        let blocks = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundSource.BLOCKS");
        let master = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundSource.MASTER");
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let pos = crate::api::block_pos(rt, 0, 64, 0);
            crate::api::call(rt, "net/minecraft/world/level/Level.playSound.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/core/BlockPos;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FF)V", &[level, V::NULL, pos, event, blocks, V::F(0.5), V::F(0.75)]);
            crate::api::call(rt, "net/minecraft/world/level/Level.playSound.(Lnet/minecraft/world/entity/player/Player;DDDLnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FF)V", &[level, V::NULL, V::D(1.0), V::D(2.0), V::D(3.0), trapdoor, blocks, V::F(1.0), V::F(1.25)]);
            crate::api::call(rt, "net/minecraft/world/level/Level.playSeededSound.(Lnet/minecraft/world/entity/player/Player;DDDLnet/minecraft/core/Holder;Lnet/minecraft/sounds/SoundSource;FFJ)V", &[level, V::NULL, V::D(0.0), V::D(70.0), V::D(0.0), click, master, V::F(0.25), V::F(1.0), V::J(7)]);
            // No volume or pitch: 1 and 1.
            crate::api::call(rt, "net/minecraft/world/level/Level.playSound.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/core/BlockPos;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;)V", &[level, V::NULL, pos, trapdoor, blocks]);
            // A server's level plays no local sound.
            crate::api::call(rt, "net/minecraft/world/level/Level.playLocalSound.(DDDLnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FFZ)V", &[level, V::D(4.0), V::D(5.0), V::D(6.0), event, blocks, V::F(1.0), V::F(1.0), V::I(0)]);
        });
        assert_eq!(
            host.sounds,
            [
                sound("create:contraption_assemble", [0.5, 64.5, 0.5], 4, 0.5, 0.75),
                sound("minecraft:block.wooden_trapdoor.open", [1.0, 2.0, 3.0], 4, 1.0, 1.25),
                sound("minecraft:ui.button.click", [0.0, 70.0, 0.0], 0, 0.25, 1.0),
                sound("minecraft:block.wooden_trapdoor.open", [0.5, 64.5, 0.5], 4, 1.0, 1.0),
            ]
        );
        let location = crate::api::call(&mut rt, "net/minecraft/sounds/SoundEvent.getLocation.()Lnet/minecraft/resources/ResourceLocation;", &[event]);
        assert_eq!(rt.str_of(location), Some("create:contraption_assemble"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A client's level plays local sounds, and a `playSound` only for its own player.
    #[test]
    fn a_client_plays_local_sounds_and_its_own_players() {
        let mut rt = Rt::new();
        crate::install();
        crate::neo::fire_client_setup(&mut rt);
        let mut host = MemoryHost { client: true, ..MemoryHost::default() };
        let pling = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundEvents.EXPERIENCE_ORB_PICKUP");
        let players = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundSource.PLAYERS");
        let me = crate::network::local_player(&mut rt);
        let key = "net/minecraft/world/level/Level.playSound.(Lnet/minecraft/world/entity/player/Player;DDDLnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FF)V";
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            crate::api::call(rt, key, &[level, V::NULL, V::D(1.0), V::D(2.0), V::D(3.0), pling, players, V::F(1.0), V::F(1.0)]);
            crate::api::call(rt, key, &[level, me, V::D(1.0), V::D(2.0), V::D(3.0), pling, players, V::F(1.0), V::F(0.5)]);
            crate::api::call(rt, "net/minecraft/world/level/Level.playLocalSound.(DDDLnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FFZ)V", &[level, V::D(4.0), V::D(5.0), V::D(6.0), pling, players, V::F(0.5), V::F(2.0), V::I(0)]);
        });
        assert_eq!(
            host.sounds,
            [
                sound("minecraft:entity.experience_orb.pickup", [1.0, 2.0, 3.0], 7, 1.0, 0.5),
                sound("minecraft:entity.experience_orb.pickup", [4.0, 5.0, 6.0], 7, 0.5, 2.0),
            ]
        );
    }

    /// A mod's own ticking instance (Create's `ContinuousSound`): `play` starts it as its getters
    /// say (its own `getVolume` override among them), each tick gives the game where it is and how
    /// loud, and once it stops itself (`stop()` in its `tick`) the game stops it. `isActive`
    /// follows, and `SoundManager.stop` stops another.
    #[test]
    fn a_mods_ticking_sound_plays_as_it_says_until_it_stops() {
        fn hum(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                "tick.()V" => {
                    let t = rt.get_field(args[0], "ticks").i() + 1;
                    rt.put_field(args[0], "ticks", V::I(t));
                    if t == 3 {
                        crate::api::call(rt, "net/minecraft/client/resources/sounds/AbstractTickableSoundInstance.stop.()V", &[args[0]]);
                    }
                    V::Void
                }
                "getVolume.()F" => rt.get_field(args[0], "level"),
                _ => crate::api::call(rt, key, args),
            }
        }
        let mut rt = Rt::new();
        rt.interned.insert("compat:client".into(), V::I(1));
        rt.vcall = Some(hum);
        rt.supers.insert("test/Hum", ABSTRACT_TICKABLE);
        let event = sound_event(&mut rt, "create:steam", None);
        let ambient = crate::api::get_static(&mut rt, "net/minecraft/sounds/SoundSource.AMBIENT");
        let init = "net/minecraft/client/resources/sounds/AbstractTickableSoundInstance.<init>.(Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;Lnet/minecraft/util/RandomSource;)V";
        let make = |rt: &mut Rt| {
            let random = crate::api::call(rt, "net/minecraft/client/resources/sounds/SoundInstance.createUnseededRandom.()Lnet/minecraft/util/RandomSource;", &[]);
            let h = rt.new_object("test/Hum");
            crate::api::call(rt, init, &[h, event, ambient, random]);
            rt.put_field(h, "looping", V::I(1));
            rt.put_field(h, "x", V::D(1.5));
            rt.put_field(h, "level", V::F(0.5));
            h
        };
        let (a, b) = (make(&mut rt), make(&mut rt));
        let manager = "net/minecraft/client/Minecraft.getSoundManager.()Lnet/minecraft/client/sounds/SoundManager;";
        let play = "net/minecraft/client/sounds/SoundManager.play.(Lnet/minecraft/client/resources/sounds/SoundInstance;)V";
        let active = "net/minecraft/client/sounds/SoundManager.isActive.(Lnet/minecraft/client/resources/sounds/SoundInstance;)Z";
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let m = crate::api::call(rt, manager, &[V::NULL]);
            crate::api::call(rt, play, &[m, a]);
            crate::api::call(rt, play, &[m, a]);
            crate::api::call(rt, play, &[m, b]);
            assert_eq!(crate::api::call(rt, active, &[m, a]), V::I(1));
            crate::api::call(rt, "net/minecraft/client/sounds/SoundManager.stop.(Lnet/minecraft/client/resources/sounds/SoundInstance;)V", &[m, b]);
            assert_eq!(crate::api::call(rt, active, &[m, b]), V::I(0));
            rt.put_field(a, "level", V::F(0.25));
            rt.put_field(a, "x", V::D(2.5));
            tick(rt);
        });
        let started = InstanceSound {
            sound: Sound { event: "create:steam".into(), at: [1.5, 0.0, 0.0], source: 8, volume: 0.5, pitch: 1.0 },
            looping: true,
            delay: 0,
            start_delay: 0,
            linear: true,
            relative: false,
            can_start_silent: false,
        };
        assert_eq!(host.started_sounds, [started.clone(), started]);
        assert_eq!(host.playing_sounds.get(&1), Some(&([2.5, 0.0, 0.0], 0.25, 1.0)));
        assert!(!host.playing_sounds.contains_key(&2));
        // Its third tick stops it.
        rt.with_host(&mut host, |rt| {
            tick(rt);
            tick(rt);
            let m = crate::api::call(rt, manager, &[V::NULL]);
            assert_eq!(crate::api::call(rt, active, &[m, a]), V::I(0));
        });
        assert!(host.playing_sounds.is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
