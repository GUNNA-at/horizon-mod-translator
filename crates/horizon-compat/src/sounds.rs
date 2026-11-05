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
use crate::host::Sound;
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

/// The client's sounds a mod plays itself (`Minecraft.getSoundManager().play(instance)`): the
/// game's own instances (`SimpleSoundInstance.forUI`, one at a place), played once. A mod's own
/// instance class (a looping, ticking one) isn't played yet.
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
        (SOUND_MANAGER, "isActive") => V::I(0),
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
    matches!(owner, SOUND_EVENT | SIMPLE_SOUND | SOUND_MANAGER) || matches!(name, "getSoundManager" | "playSound")
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
}
