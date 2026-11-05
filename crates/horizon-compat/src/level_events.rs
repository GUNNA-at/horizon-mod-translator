//! Level events as translated mods make them, natively: `Level.levelEvent(player, event, pos, data)`,
//! `LevelAccessor.levelEvent(event, pos, data)` and `globalLevelEvent`, with vanilla's `LevelEvent`
//! codes (a dispenser's click 1000, a door's sound 1005..., smoke 2000, a block breaking 2001, bone
//! meal 1505...), made by the game as its own (`Host::level_event`): the players near hear and see
//! them. A block breaking names its state by the game's id, which `Block.getId(state)` gives and
//! `Block.stateById(id)` reads back (`Host::state_id`, `Host::state_of_id`).
//!
//! Not modelled: the player a mod names isn't left out of those who get the event (vanilla's server
//! leaves out the player whose client made it itself), and a global event (a wither's spawn, an end
//! portal's opening) reaches the players near only.
use crate::{Rt, V};

const BLOCK: &str = "net/minecraft/world/level/block/Block";
const GET_ID: &str = "(Lnet/minecraft/world/level/block/state/BlockState;)I";
const STATE_BY_ID: &str = "(I)Lnet/minecraft/world/level/block/state/BlockState;";
/// `levelEvent`'s first form: the player whose client already made it, then the event.
const BY_PLAYER: &str = "(Lnet/minecraft/world/entity/player/Player;ILnet/minecraft/core/BlockPos;I)V";
const PLAIN: &str = "(ILnet/minecraft/core/BlockPos;I)V";

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match name {
        "levelEvent" => crate::api::is_level(owner) && (desc == BY_PLAYER || desc == PLAIN),
        "globalLevelEvent" => crate::api::is_level(owner) && desc == PLAIN,
        "getId" => owner == BLOCK && desc == GET_ID,
        "stateById" => owner == BLOCK && desc == STATE_BY_ID,
        _ => false,
    }
}

/// A level event, or a block state's id either way; None for any other key.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        // Static: the state is the first argument. Air (0) for one the game doesn't know, or no
        // game to ask.
        "getId" => {
            let text = crate::api_common::name_of(rt, a(0));
            V::I(rt.host().and_then(|h| h.state_id(&text)).unwrap_or(0))
        }
        "stateById" => {
            let text = rt.host().and_then(|h| h.state_of_id(a(0).i())).unwrap_or_else(|| "minecraft:air".to_owned());
            crate::api::state(rt, &text)
        }
        _ => {
            let (event, pos, data) = if desc == BY_PLAYER { (a(2), a(3), a(4)) } else { (a(1), a(2), a(3)) };
            let at = crate::api::pos_of(rt, pos);
            if let Some(h) = rt.host_mut() {
                h.level_event(event.i(), at, data.i());
            }
            V::Void
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        format!("{owner}.{name}.{desc}").leak()
    }

    #[test]
    fn a_mod_breaks_a_block_with_its_sound_and_particles() {
        let stone = "minecraft:stone";
        let lever = "minecraft:lever[face=wall,facing=north,powered=false]";
        let mut h = MemoryHost { state_ids: vec!["minecraft:air".into(), stone.into(), lever.into()], ..MemoryHost::default() };
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            // Create's drill: `level.levelEvent(2001, pos, Block.getId(state))`.
            let state = crate::api::state(rt, lever);
            let id = crate::api::call(rt, key(BLOCK, "getId", GET_ID), &[state]);
            assert_eq!(id, V::I(2));
            let level = crate::glue::level(rt);
            let pos = crate::api::block_pos(rt, 4, 64, -2);
            crate::api::call(rt, key("net/minecraft/world/level/Level", "levelEvent", PLAIN), &[level, V::I(2001), pos, id]);
            // A door's sound, the form that names the player.
            let player = crate::menus::server_player(rt, 0xabc);
            crate::api::call(rt, key("net/minecraft/world/level/LevelAccessor", "levelEvent", BY_PLAYER), &[level, player, V::I(1005), pos, V::I(0)]);

            // Read back: the state of id 1, and air past the table.
            let back = crate::api::call(rt, key(BLOCK, "stateById", STATE_BY_ID), &[V::I(1)]);
            assert_eq!(crate::api_common::name_of(rt, back), stone);
            let none = crate::api::call(rt, key(BLOCK, "stateById", STATE_BY_ID), &[V::I(99)]);
            assert_eq!(crate::api_common::name_of(rt, none), "minecraft:air");
            // A state the game doesn't know is air's.
            let unknown = crate::api::state(rt, "hzcheck:nothing");
            assert_eq!(crate::api::call(rt, key(BLOCK, "getId", GET_ID), &[unknown]), V::I(0));
        });
        assert_eq!(h.level_events, [(2001, [4, 64, -2], 2), (1005, [4, 64, -2], 0)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
