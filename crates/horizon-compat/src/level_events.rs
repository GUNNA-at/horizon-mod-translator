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
const ID_MAPPER: &str = "net/minecraft/core/IdMapper";
const ID_MAP: &str = "net/minecraft/core/IdMap";
/// The game's table of block states (`Block.BLOCK_STATE_REGISTRY`): the one `IdMapper` whose
/// values and ids are the game's own.
pub(crate) const STATE_IDS: &str = "compat:block-state-ids";
/// A mod's own `IdMapper`'s values, in the order of their ids (a list).
const VALUES: &str = "$values";
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
        "getId" if owner == BLOCK => desc == GET_ID,
        "stateById" => owner == BLOCK && desc == STATE_BY_ID,
        "<init>" | "getId" | "getIdOrThrow" | "byId" | "byIdOrThrow" | "size" | "contains"
        | "add" | "addMapping" | "iterator" | "spliterator" | "forEach" => {
            matches!(owner, ID_MAPPER | ID_MAP)
        }
        _ => false,
    }
}

/// How many block states the game has: its ids are 0 to one less. Past its table the game
/// answers air (`Host::state_of_id`), and air itself is id 0, so the table ends where the
/// answer turns to air; found once.
fn state_count(rt: &mut Rt) -> i32 {
    const KEY: &str = "compat:block-state-count";
    if let Some(V::I(n)) = rt.interned.get(KEY) {
        return *n;
    }
    let Some(h) = rt.host() else {
        return 0;
    };
    let past = |id: i32| h.state_of_id(id).is_none_or(|s| s == "minecraft:air");
    let n = if h.state_of_id(0).is_none() {
        0
    } else {
        let mut high = 1i32;
        while high < 1 << 26 && !past(high) {
            high *= 2;
        }
        let mut low = high / 2;
        // The first id past the table is in (low, high].
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if past(mid) {
                high = mid;
            } else {
                low = mid;
            }
        }
        if past(1) { 1 } else { high }
    };
    rt.interned.insert(KEY.into(), V::I(n));
    n
}

/// Every block state of the game's, in the order of their ids (`Block.BLOCK_STATE_REGISTRY`
/// walked: a map mod makes its colours from them all). Made once, and kept.
fn all_states(rt: &mut Rt) -> Vec<V> {
    const KEY: &str = "compat:block-states";
    if let Some(&list) = rt.interned.get(KEY) {
        return crate::jutil::elements(rt, list);
    }
    let n = state_count(rt);
    let mut all = Vec::with_capacity(n.max(0) as usize);
    for id in 0..n {
        let Some(text) = rt.host().and_then(|h| h.state_of_id(id)) else {
            break;
        };
        all.push(crate::api::state(rt, &text));
    }
    // (With no game to ask there is none: asked again when there is one.)
    if !all.is_empty() {
        let list = crate::jutil::new_list(rt, all.clone());
        rt.interned.insert(KEY.into(), list);
    }
    all
}

/// `IdMapper`: values by their ids. The game's table of block states answers from the game
/// (`getId` as `Block.getId`, -1 for a state it doesn't have); one a mod makes holds what the
/// mod adds to it.
fn id_mapper(rt: &mut Rt, name: &str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let states = rt.interned.get(STATE_IDS) == Some(&this);
    let values = |rt: &mut Rt| {
        if states {
            return all_states(rt);
        }
        let list = rt.get_field(this, VALUES);
        crate::jutil::elements(rt, list)
    };
    match name {
        "<init>" => {
            let list = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(this, VALUES, list);
            V::Void
        }
        "getId" | "getIdOrThrow" => {
            let id = if states {
                let text = crate::api_common::name_of(rt, a(1));
                rt.host().and_then(|h| h.state_id(&text)).unwrap_or(-1)
            } else {
                let v = a(1);
                let all = values(rt);
                all.iter()
                    .position(|x| *x == v || (x.r() != 0 && v.r() != 0 && rt.java_equals(*x, v)))
                    .map_or(-1, |i| i as i32)
            };
            if id < 0 && name == "getIdOrThrow" {
                return crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalArgumentException",
                    "Can't find id for the value in the map",
                );
            }
            V::I(id)
        }
        "byId" | "byIdOrThrow" => {
            let id = a(1).i();
            let found = if states {
                let text = (0..state_count(rt))
                    .contains(&id)
                    .then(|| rt.host().and_then(|h| h.state_of_id(id)))
                    .flatten();
                text.map(|t| crate::api::state(rt, &t))
            } else {
                usize::try_from(id)
                    .ok()
                    .and_then(|i| values(rt).get(i).copied())
                    .filter(|v| v.r() != 0)
            };
            match found {
                Some(v) => v,
                None if name == "byIdOrThrow" => crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalArgumentException",
                    &format!("No value with id {id}"),
                ),
                None => V::NULL,
            }
        }
        "size" => V::I(if states {
            state_count(rt)
        } else {
            values(rt).len() as i32
        }),
        "contains" => {
            let id = a(1).i();
            let n = if states {
                state_count(rt)
            } else {
                values(rt).len() as i32
            };
            V::I((0..n).contains(&id) as i32)
        }
        // `add(value)` takes the next id; `addMapping(value, id)` that one.
        "add" | "addMapping" if !states => {
            let list = rt.get_field(this, VALUES);
            let mut all = crate::jutil::elements(rt, list);
            match (name, usize::try_from(a(2).i())) {
                ("addMapping", Ok(id)) => {
                    if all.len() <= id {
                        all.resize(id + 1, V::NULL);
                    }
                    all[id] = a(1);
                }
                _ => all.push(a(1)),
            }
            let list = crate::jutil::new_list(rt, all);
            rt.put_field(this, VALUES, list);
            V::Void
        }
        "add" | "addMapping" => V::Void,
        "iterator" => {
            let all: Vec<V> = values(rt).into_iter().filter(|v| v.r() != 0).collect();
            crate::jutil::new_iter(rt, all)
        }
        "spliterator" => {
            let all: Vec<V> = values(rt).into_iter().filter(|v| v.r() != 0).collect();
            rt.alloc("java/util/Spliterator", crate::Data::Stream(all))
        }
        // `forEach(consumer)`.
        _ => {
            for v in values(rt) {
                if v.r() == 0 {
                    continue;
                }
                rt.call_functional(
                    "java/util/function/Consumer.accept.(Ljava/lang/Object;)V",
                    a(1),
                    &[v],
                );
                if rt.thrown.is_some() {
                    break;
                }
            }
            V::Void
        }
    }
}

/// A level event, or a block state's id either way; None for any other key.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if matches!(owner, ID_MAPPER | ID_MAP) {
        return Some(id_mapper(rt, name, args));
    }
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

    /// The game's table of block states, as a mod walks it (`Block.BLOCK_STATE_REGISTRY`: a map
    /// mod makes a colour for every state): as many as the game has, each by its id and back,
    /// and nothing past the table. A mapper a mod makes holds what the mod adds.
    #[test]
    fn the_games_block_states_are_walked_by_their_ids() {
        let names = [
            "minecraft:air",
            "minecraft:stone",
            "minecraft:lever[face=wall,facing=north,powered=false]",
            "minecraft:dirt",
            "minecraft:cave_air",
        ];
        let mut h = MemoryHost {
            state_ids: names.iter().map(|s| (*s).to_owned()).collect(),
            ..MemoryHost::default()
        };
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            let table = crate::api::get_static(
                rt,
                "net/minecraft/world/level/block/Block.BLOCK_STATE_REGISTRY",
            );
            let size = crate::api::call(rt, key(ID_MAPPER, "size", "()I"), &[table]);
            assert_eq!(size, V::I(5));
            let split = crate::api::call(
                rt,
                key(ID_MAPPER, "spliterator", "()Ljava/util/Spliterator;"),
                &[table],
            );
            let walked: Vec<String> = crate::jutil::elements(rt, split)
                .into_iter()
                .map(|s| crate::api_common::name_of(rt, s))
                .collect();
            assert_eq!(walked, names);
            let dirt = crate::api::state(rt, "minecraft:dirt");
            assert_eq!(
                crate::api::call(
                    rt,
                    key(ID_MAPPER, "getId", "(Ljava/lang/Object;)I"),
                    &[table, dirt]
                ),
                V::I(3)
            );
            let third = crate::api::call(
                rt,
                key(ID_MAPPER, "byId", "(I)Ljava/lang/Object;"),
                &[table, V::I(3)],
            );
            assert_eq!(third, dirt);
            assert_eq!(
                crate::api::call(
                    rt,
                    key(ID_MAPPER, "byId", "(I)Ljava/lang/Object;"),
                    &[table, V::I(5)]
                ),
                V::NULL
            );
            let unknown = crate::api::state(rt, "hzcheck:nothing");
            assert_eq!(
                crate::api::call(
                    rt,
                    key(ID_MAP, "getId", "(Ljava/lang/Object;)I"),
                    &[table, unknown]
                ),
                V::I(-1)
            );
            // One of a mod's own.
            let own = rt.new_object(ID_MAPPER);
            crate::api::call(rt, key(ID_MAPPER, "<init>", "(I)V"), &[own, V::I(16)]);
            crate::api::call(
                rt,
                key(ID_MAPPER, "add", "(Ljava/lang/Object;)V"),
                &[own, dirt],
            );
            crate::api::call(
                rt,
                key(ID_MAPPER, "addMapping", "(Ljava/lang/Object;I)V"),
                &[own, unknown, V::I(3)],
            );
            assert_eq!(
                crate::api::call(rt, key(ID_MAPPER, "size", "()I"), &[own]),
                V::I(4)
            );
            assert_eq!(
                crate::api::call(
                    rt,
                    key(ID_MAPPER, "getId", "(Ljava/lang/Object;)I"),
                    &[own, unknown]
                ),
                V::I(3)
            );
            assert_eq!(
                crate::api::call(
                    rt,
                    key(ID_MAPPER, "byId", "(I)Ljava/lang/Object;"),
                    &[own, V::I(0)]
                ),
                dirt
            );
            assert_eq!(
                crate::api::call(
                    rt,
                    key(ID_MAPPER, "byId", "(I)Ljava/lang/Object;"),
                    &[own, V::I(1)]
                ),
                V::NULL
            );
            let it = crate::api::call(
                rt,
                key(ID_MAPPER, "iterator", "()Ljava/util/Iterator;"),
                &[own],
            );
            assert_eq!(crate::jutil::elements(rt, it), [dirt, unknown]);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}

#[cfg(test)]
mod height_tests {
    use crate::host::MemoryHost;
    use crate::{Rt, V};

    fn key(text: &str) -> &'static str {
        text.to_owned().leak()
    }

    /// A column's top in a running game's level, which keeps no heightmap for the mods: read
    /// from its blocks as each of vanilla's heightmaps counts them. Stone, water over it, and
    /// leaves above: the leaves are the world's surface, and block motion (the ocean's floor
    /// counts them too); without leaves the water is the top. With a plant in place of the
    /// leaves, which blocks nothing: the surface is the plant, motion blocking the water, the
    /// ocean's floor the stone. The chunk's own `getHeight` names the top block itself, one
    /// lower; and a column with nothing in it is the world's bottom.
    #[test]
    fn a_columns_top_is_read_from_its_blocks() {
        // (Leaves are solid to the game, as stone is: their shape is a whole cube.)
        let mut h = MemoryHost {
            solid: vec!["minecraft:stone".into(), "minecraft:oak_leaves".into()],
            ..MemoryHost::default()
        };
        // In chunk (1, -1): x 21 and 23, z -3.
        for x in [21, 23] {
            h.blocks.insert([x, 60, -3], "minecraft:stone".into());
            h.blocks
                .insert([x, 61, -3], "minecraft:water[level=0]".into());
        }
        h.blocks.insert(
            [21, 70, -3],
            "minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]".into(),
        );
        h.blocks
            .insert([23, 62, -3], "minecraft:short_grass".into());
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            let kind = |rt: &mut Rt, name: &str| {
                crate::api::get_static(rt, key(&format!("net/minecraft/world/level/levelgen/Heightmap$Types.{name}")))
            };
            let of_level = "net/minecraft/world/level/Level.getHeight.(Lnet/minecraft/world/level/levelgen/Heightmap$Types;II)I";
            for (name, under_leaves, under_a_plant) in [
                ("WORLD_SURFACE", 71, 63),
                ("MOTION_BLOCKING", 71, 62),
                ("MOTION_BLOCKING_NO_LEAVES", 62, 62),
                ("OCEAN_FLOOR", 71, 61),
            ] {
                let k = kind(rt, name);
                assert_eq!(crate::api::call(rt, of_level, &[level, k, V::I(21), V::I(-3)]), V::I(under_leaves), "{name}");
                assert_eq!(crate::api::call(rt, of_level, &[level, k, V::I(23), V::I(-3)]), V::I(under_a_plant), "{name}");
            }
            let surface = kind(rt, "WORLD_SURFACE");
            assert_eq!(crate::api::call(rt, of_level, &[level, surface, V::I(22), V::I(-3)]), V::I(-64), "an empty column");
            // The chunk's: the top block itself, the column by its place in the chunk.
            let chunk = crate::api::call(
                rt,
                "net/minecraft/world/level/Level.getChunk.(II)Lnet/minecraft/world/level/chunk/LevelChunk;",
                &[level, V::I(1), V::I(-1)],
            );
            let of_chunk = "net/minecraft/world/level/chunk/LevelChunk.getHeight.(Lnet/minecraft/world/level/levelgen/Heightmap$Types;II)I";
            assert_eq!(crate::api::call(rt, of_chunk, &[chunk, surface, V::I(5), V::I(13)]), V::I(70));
            assert_eq!(crate::api::call(rt, of_chunk, &[chunk, surface, V::I(21), V::I(-3)]), V::I(70), "or in the world");
            // And the world's height, asked with no column, is still that.
            assert_eq!(
                crate::api::call(rt, "net/minecraft/world/level/chunk/LevelChunk.getHeight.()I", &[chunk]),
                V::I(384)
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}

#[cfg(test)]
mod light_tests {
    use crate::host::MemoryHost;
    use crate::{Rt, V};

    /// A real level's light engine, as a map mod asks it for a block's light
    /// (`level.getLightEngine().getLayerListener(LightLayer.BLOCK).getLightValue(pos)`): the
    /// game's light there, each layer its own. An engine a mod makes for a world of its own
    /// keeps no light, as before.
    #[test]
    fn a_levels_light_engine_tells_each_layers_light() {
        let mut h = MemoryHost::default();
        h.light.insert([3, 70, -2], (11, 4));
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            let pos = crate::api::block_pos(rt, 3, 70, -2);
            let engine = crate::api::call(rt, "net/minecraft/world/level/Level.getLightEngine.()Lnet/minecraft/world/level/lighting/LevelLightEngine;", &[level]);
            let layer = |rt: &mut Rt, name: &str| {
                let key: &'static str = format!("net/minecraft/world/level/LightLayer.{name}").leak();
                crate::api::get_static(rt, key)
            };
            const LISTENER: &str = "net/minecraft/world/level/lighting/LevelLightEngine.getLayerListener.(Lnet/minecraft/world/level/LightLayer;)Lnet/minecraft/world/level/lighting/LayerLightEventListener;";
            const VALUE: &str = "net/minecraft/world/level/lighting/LayerLightEventListener.getLightValue.(Lnet/minecraft/core/BlockPos;)I";
            for (name, light) in [("BLOCK", 11), ("SKY", 4)] {
                let l = layer(rt, name);
                let listener = crate::api::call(rt, LISTENER, &[engine, l]);
                assert_eq!(crate::api::call(rt, VALUE, &[listener, pos]), V::I(light), "{name}");
            }
            assert_eq!(
                crate::api::call(rt, "net/minecraft/world/level/lighting/LevelLightEngine.getRawBrightness.(Lnet/minecraft/core/BlockPos;I)I", &[engine, pos, V::I(0)]),
                V::I(11)
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
