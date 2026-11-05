//! A mod's own worldgen code, run by Horizon's world generator: NeoForge placement modifier types
//! a mod registers (`Registries.PLACEMENT_MODIFIER_TYPE`: Create's `create:config_filter`), and
//! feature types (`Registries.FEATURE`: Create's `create:layered_ore`). The generator hands the
//! modifier's JSON to [`placement`], which reads it with the type's own codec, and asks
//! [`positions`] for what it makes of a position: the mod's `getPositions`, or vanilla's
//! `PlacementFilter` and `RepeatingPlacement` around the mod's `shouldPlace` and `count`. A
//! configured feature's JSON goes to [`feature`], read with the codec the feature was made with
//! (`Feature(Codec)`), and [`place`] runs the mod's `place` on it. The world they read and write
//! is the host's (the generator's region), and the `RandomSource` they get draws from the host's
//! random (`Host::random`): the feature's own stream, as vanilla hands it.
//!
//! What features use of vanilla's worldgen is here too: `BulkSectionAccess` and the sections it
//! gives (raw section writes, as the host keeps them), `SectionPos`'s coordinates,
//! `SimplexNoise`, the rule tests ore targets are made of (`RuleTest.CODEC`, `test`) and
//! `OreConfiguration.TargetBlockState`, and `Feature`'s static helpers.
use crate::{Rt, V};
use serde_json::Value as Json;

/// The random a modifier is handed (vanilla's `WorldgenRandom`): its draws are the host's stream.
pub const WORLDGEN_RANDOM: &str = "net/minecraft/world/level/levelgen/WorldgenRandom";
const RANDOM_SOURCE: &str = "net/minecraft/util/RandomSource";
const CONTEXT: &str = "net/minecraft/world/level/levelgen/placement/PlacementContext";
const LEVEL: &str = "net/minecraft/server/level/WorldGenRegion";
const MODIFIER: &str = "net/minecraft/world/level/levelgen/placement/PlacementModifier";
const FILTER: &str = "net/minecraft/world/level/levelgen/placement/PlacementFilter";
const REPEATING: &str = "net/minecraft/world/level/levelgen/placement/RepeatingPlacement";
const GET_POSITIONS: &str = "getPositions.(Lnet/minecraft/world/level/levelgen/placement/PlacementContext;Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;)Ljava/util/stream/Stream;";
const GET_POSITIONS_KEY: &str = "net/minecraft/world/level/levelgen/placement/PlacementModifier.getPositions.(Lnet/minecraft/world/level/levelgen/placement/PlacementContext;Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;)Ljava/util/stream/Stream;";
const SHOULD_PLACE: &str = "shouldPlace.(Lnet/minecraft/world/level/levelgen/placement/PlacementContext;Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;)Z";
const SHOULD_PLACE_KEY: &str = "net/minecraft/world/level/levelgen/placement/PlacementFilter.shouldPlace.(Lnet/minecraft/world/level/levelgen/placement/PlacementContext;Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;)Z";
const COUNT: &str = "count.(Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;)I";
const COUNT_KEY: &str = "net/minecraft/world/level/levelgen/placement/RepeatingPlacement.count.(Lnet/minecraft/util/RandomSource;Lnet/minecraft/core/BlockPos;)I";
const TYPE_CODEC: &str = "net/minecraft/world/level/levelgen/placement/PlacementModifierType.codec.()Lcom/mojang/serialization/MapCodec;";

/// The registry a mod's placement modifier types are in, as this runtime names it
/// (`Registries.PLACEMENT_MODIFIER_TYPE`'s field, or the registry's path).
const TYPE_REGISTRIES: [&str; 2] = ["placement_modifier_type", "worldgen/placement_modifier_type"];

const FEATURE: &str = "net/minecraft/world/level/levelgen/feature/Feature";
const PLACE_CONTEXT: &str = "net/minecraft/world/level/levelgen/feature/FeaturePlaceContext";
const CONFIGURED: &str = "net/minecraft/world/level/levelgen/feature/ConfiguredFeature";
const PLACE: &str = "place.(Lnet/minecraft/world/level/levelgen/feature/FeaturePlaceContext;)Z";
const PLACE_KEY: &str = "net/minecraft/world/level/levelgen/feature/Feature.place.(Lnet/minecraft/world/level/levelgen/feature/FeaturePlaceContext;)Z";
/// The registry a mod's feature types are in (`Registries.FEATURE`'s field, or its path).
const FEATURE_REGISTRIES: [&str; 2] = ["feature", "worldgen/feature"];
const BULK: &str = "net/minecraft/world/level/chunk/BulkSectionAccess";
const SECTION: &str = "net/minecraft/world/level/chunk/LevelChunkSection";
const SECTION_POS: &str = "net/minecraft/core/SectionPos";
const SIMPLEX: &str = "net/minecraft/world/level/levelgen/synth/SimplexNoise";
const ORE_CONFIGURATION: &str = "net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration";
const TARGET: &str = "net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration$TargetBlockState";
const RULE_TEST: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/RuleTest";
const TAG_MATCH: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/TagMatchTest";
const BLOCK_MATCH: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/BlockMatchTest";
const STATE_MATCH: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/BlockStateMatchTest";
const RANDOM_BLOCK_MATCH: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/RandomBlockMatchTest";
const RANDOM_STATE_MATCH: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/RandomBlockStateMatchTest";
const ALWAYS_TRUE: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/AlwaysTrueTest";
const IS_AIR: &str = "net/minecraft/world/level/block/state/BlockState.isAir.()Z";
const STATE_IN_TAG: &str = "net/minecraft/world/level/block/state/BlockState.is.(Lnet/minecraft/tags/TagKey;)Z";
const NEXT_FLOAT: &str = "net/minecraft/util/RandomSource.nextFloat.()F";
const NEXT_DOUBLE: &str = "net/minecraft/util/RandomSource.nextDouble.()D";
const NEXT_INT_BOUND: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const GET_BLOCK_STATE: &str = "net/minecraft/world/level/LevelReader.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const SET_BLOCK: &str = "net/minecraft/world/level/LevelWriter.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z";
/// `FeaturePlaceContext`'s fields, in its constructor's order.
const PLACE_CONTEXT_FIELDS: [&str; 6] = ["topFeature", "level", "chunkGenerator", "random", "origin", "config"];
/// `Direction.values()`'s offsets, in its order: down, up, north, south, west, east.
const DIRECTIONS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

/// An exception thrown out of the mod's code, as a message.
fn thrown(rt: &mut Rt, e: V) -> String {
    let m = rt.get_field(e, "message");
    format!("{} thrown: {}", rt.class_of(e), rt.str_of(m).unwrap_or(""))
}

/// The modifier `json` describes (the whole object, `type` included) when `kind` is one of the
/// mod's placement modifier types; `None` when it isn't. The type's codec reads the object (its
/// `type` key is the dispatch's, the rest the modifier's fields), once per text.
pub fn placement(rt: &mut Rt, kind: &str, json: &str) -> Option<Result<V, String>> {
    let ty = TYPE_REGISTRIES.iter().map(|r| crate::neo::registry_get(rt, r, kind)).find(|v| v.r() != 0)?;
    let key = format!("compat:worldgen/placement/{json}");
    if let Some(&v) = rt.interned.get(&key) {
        return Some(Ok(v));
    }
    let read = (|| {
        let Json::Object(object) = serde_json::from_str::<Json>(json).map_err(|e| e.to_string())? else {
            return Err(format!("Not a map: {json}"));
        };
        let codec = rt.call_functional(TYPE_CODEC, ty, &[]);
        if let Some(e) = rt.thrown.take() {
            return Err(thrown(rt, e));
        }
        let v = crate::codecs::decode_map(rt, codec, &object)?;
        if let Some(e) = rt.thrown.take() {
            return Err(thrown(rt, e));
        }
        Ok(v)
    })();
    if let Ok(v) = read {
        rt.interned.insert(key, v);
    }
    Some(read)
}

/// The positions modifier `modifier` makes of `origin`, as `getPositions` streams them, on the
/// host's world and random stream.
pub fn positions(rt: &mut Rt, modifier: V, origin: [i32; 3]) -> Result<Vec<[i32; 3]>, String> {
    let context = rt.intern("compat:worldgen/context".into(), |rt| {
        let c = rt.new_object(CONTEXT);
        let level = level(rt);
        rt.put_field(c, "$level", level);
        c
    });
    let random = rt.intern("compat:worldgen/random".into(), |rt| rt.new_object(WORLDGEN_RANDOM));
    let pos = crate::api::block_pos(rt, i64::from(origin[0]), i64::from(origin[1]), i64::from(origin[2]));
    let Some(vcall) = rt.vcall else { return Err("no mod code to run".into()) };
    let stream = vcall(rt, GET_POSITIONS_KEY, GET_POSITIONS, &[modifier, context, random, pos]);
    if let Some(e) = rt.thrown.take() {
        return Err(thrown(rt, e));
    }
    Ok(crate::jutil::elements(rt, stream).into_iter().map(|p| crate::api::pos_of(rt, p)).collect())
}

/// The configured feature `json` describes when `kind` is one of the mod's feature types; `None`
/// when it isn't. The feature's own codec (`Feature(Codec)`) reads its configuration: the
/// object's `config` (1.21.1's shape), or the object itself (26.3's, `config` unwrapped). Once
/// per text.
pub fn feature(rt: &mut Rt, kind: &str, json: &str) -> Option<Result<V, String>> {
    let feature = FEATURE_REGISTRIES.iter().map(|r| crate::neo::registry_get(rt, r, kind)).find(|v| v.r() != 0)?;
    let key = format!("compat:worldgen/feature/{json}");
    if let Some(&v) = rt.interned.get(&key) {
        return Some(Ok(v));
    }
    let read = (|| {
        let object = serde_json::from_str::<Json>(json).map_err(|e| e.to_string())?;
        if !object.is_object() {
            return Err(format!("Not a map: {json}"));
        }
        let codec = rt.get_field(feature, "$configuredCodec");
        if codec.r() == 0 {
            return Err(format!("feature type {kind} was made with no codec"));
        }
        let config = crate::codecs::decode(rt, codec, object.get("config").unwrap_or(&object))?;
        if let Some(e) = rt.thrown.take() {
            return Err(thrown(rt, e));
        }
        Ok(crate::records::make(rt, CONFIGURED, &[feature, config]))
    })();
    if let Ok(v) = read {
        rt.interned.insert(key, v);
    }
    Some(read)
}

/// The mod's `place` of configured feature `configured` at `origin`, on the host's world and
/// random stream: whether it placed anything.
pub fn place(rt: &mut Rt, configured: V, origin: [i32; 3]) -> Result<bool, String> {
    // `Feature.place(config, level, generator, random, origin)`: none where the region can't write.
    if !rt.host().is_none_or(|h| h.can_write(origin)) {
        return Ok(false);
    }
    let level = level(rt);
    let random = rt.intern("compat:worldgen/random".into(), |rt| rt.new_object(WORLDGEN_RANDOM));
    let (feature, config) = (rt.get_field(configured, "feature"), rt.get_field(configured, "config"));
    let pos = crate::api::block_pos(rt, i64::from(origin[0]), i64::from(origin[1]), i64::from(origin[2]));
    let none = rt.new_object("java/util/Optional");
    rt.put_field(none, "value", V::NULL);
    let context = rt.new_object(PLACE_CONTEXT);
    store(rt, context, &PLACE_CONTEXT_FIELDS.into_iter().zip([none, level, V::NULL, random, pos, config]).collect::<Vec<_>>());
    let Some(vcall) = rt.vcall else { return Err("no mod code to run".into()) };
    let placed = vcall(rt, PLACE_KEY, PLACE, &[feature, context]);
    if let Some(e) = rt.thrown.take() {
        return Err(thrown(rt, e));
    }
    Ok(placed.i() != 0)
}

/// The generator's region, as the level a modifier's context and a feature are handed.
fn level(rt: &mut Rt) -> V {
    rt.intern("compat:worldgen/level".into(), |rt| rt.new_object(LEVEL))
}

/// The keys [`call`] answers: vanilla's placement classes and the worldgen random's draws, and what
/// features use of vanilla's worldgen.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        MODIFIER
            | FILTER
            | REPEATING
            | CONTEXT
            | RANDOM_SOURCE
            | WORLDGEN_RANDOM
            | FEATURE
            | PLACE_CONTEXT
            | TARGET
            | BULK
            | SECTION
            | SECTION_POS
            | SIMPLEX
            | ORE_CONFIGURATION
            | RULE_TEST
            | TAG_MATCH
            | BLOCK_MATCH
            | STATE_MATCH
            | RANDOM_BLOCK_MATCH
            | RANDOM_STATE_MATCH
            | ALWAYS_TRUE
    )
}

pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if matches!(owner, RANDOM_SOURCE | WORLDGEN_RANDOM) {
        return random_call(rt, name, desc, args);
    }
    if matches!(owner, FEATURE | PLACE_CONTEXT | BULK | SECTION | SECTION_POS | SIMPLEX | ORE_CONFIGURATION | TARGET) || owner.starts_with("net/minecraft/world/level/levelgen/structure/templatesystem/") {
        return feature_call(rt, owner, name, desc, args);
    }
    let stream = |rt: &mut Rt, items: Vec<V>| rt.alloc("java/util/stream/Stream", crate::Data::Stream(items));
    Some(match (owner, name) {
        // `getPositions` a mod's modifier doesn't write itself: a filter's position when its
        // `shouldPlace` holds (`PlacementFilter`), or the position `count` times
        // (`RepeatingPlacement`).
        (MODIFIER | FILTER | REPEATING, "getPositions") => {
            let class = rt.class_of(a(0)).to_owned();
            let vcall = rt.vcall?;
            if rt.is_a(&class, FILTER) {
                let keep = vcall(rt, SHOULD_PLACE_KEY, SHOULD_PLACE, &[a(0), a(1), a(2), a(3)]);
                let items = if rt.thrown.is_none() && keep.i() != 0 { vec![a(3)] } else { Vec::new() };
                stream(rt, items)
            } else if rt.is_a(&class, REPEATING) {
                let n = vcall(rt, COUNT_KEY, COUNT, &[a(0), a(2), a(3)]).i();
                let items = if rt.thrown.is_none() { vec![a(3); n.max(0) as usize] } else { Vec::new() };
                stream(rt, items)
            } else {
                return None;
            }
        }
        // The context a modifier reads: the generator's region (the host), its heightmaps and
        // bounds. No feature is above it for a mod's own modifier to see.
        (CONTEXT, "getLevel") => rt.get_field(a(0), "$level"),
        (CONTEXT, "getHeight") if desc == "(Lnet/minecraft/world/level/levelgen/Heightmap$Types;II)I" => height(rt, a(1), a(2).i(), a(3).i()),
        (CONTEXT, "getMinBuildHeight" | "getMinY") => V::I(rt.host().map_or(-64, |h| h.world_height().0)),
        (CONTEXT, "getGenDepth") => V::I(rt.host().map_or(384, |h| h.world_height().1)),
        (CONTEXT, "getBlockState") => {
            let level = rt.get_field(a(0), "$level");
            crate::api::call(rt, "net/minecraft/world/level/WorldGenLevel.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;", &[level, a(1)])
        }
        (CONTEXT, "topFeature") => {
            let o = rt.new_object("java/util/Optional");
            rt.put_field(o, "value", V::NULL);
            o
        }
        _ => return None,
    })
}

/// `getHeight(type, x, z)` from the host's heightmaps, the world's bottom where it keeps none.
pub(crate) fn height(rt: &mut Rt, kind: V, x: i32, z: i32) -> V {
    let name = rt.str_of(rt.get_field(kind, "$name")).unwrap_or("WORLD_SURFACE_WG").to_owned();
    let bottom = rt.host().map_or(-64, |h| h.world_height().0);
    V::I(rt.host_mut().and_then(|h| h.height(&name, x, z)).unwrap_or(bottom))
}

/// A state's text (`create:shaft[axis=y]`), air for none.
fn state_text(rt: &Rt, state: V) -> String {
    rt.str_of(state).unwrap_or("minecraft:air").to_owned()
}

/// The state at `pos` in the host's world.
fn host_state(rt: &mut Rt, pos: [i32; 3]) -> V {
    let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(pos));
    crate::api::state(rt, &text)
}

/// Whether `pos` is in the host's world's height (`isOutsideBuildHeight`'s negation).
fn inside_height(rt: &Rt, y: i32) -> bool {
    let (bottom, height) = rt.host().map_or((-64, 384), |h| h.world_height());
    y >= bottom && y < bottom + height
}

/// What features use of vanilla's worldgen classes: `Feature`'s helpers, `BulkSectionAccess` and
/// its sections, `SectionPos`, `SimplexNoise`, rule tests and ore targets.
fn feature_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // `Feature`'s helpers (a mod's feature calls them on itself).
        (FEATURE, "setBlock") if desc == "(Lnet/minecraft/world/level/LevelWriter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V" => {
            crate::api::call(rt, SET_BLOCK, &[a(1), a(2), a(3), V::I(3)]);
            V::Void
        }
        (FEATURE, "safeSetBlock") => {
            let old = crate::api::call(rt, GET_BLOCK_STATE, &[a(1), a(2)]);
            if rt.call_functional(TEST, a(4), &[old]).i() != 0 {
                crate::api::call(rt, SET_BLOCK, &[a(1), a(2), a(3), V::I(2)]);
            }
            V::Void
        }
        (FEATURE, "markAboveForPostProcessing") => V::Void,
        (FEATURE, "isStone" | "isDirt") => {
            let tag = crate::fluids::tag_key(rt, "block", if name == "isStone" { "minecraft:base_stone_overworld" } else { "minecraft:dirt" });
            crate::api::call(rt, STATE_IN_TAG, &[a(0), tag])
        }
        // `checkNeighbors(getter, pos, predicate)`: whether any of the six neighbours' states, in
        // `Direction.values()` order, passes; `isAdjacentToAir` asks for air.
        (FEATURE, "checkNeighbors" | "isAdjacentToAir") => {
            let p = crate::api::pos_of(rt, a(1));
            for d in DIRECTIONS {
                let n = crate::api::block_pos(rt, i64::from(p[0] + d[0]), i64::from(p[1] + d[1]), i64::from(p[2] + d[2]));
                let state = rt.call_functional(APPLY, a(0), &[n]);
                let pass = if name == "isAdjacentToAir" { crate::api::call(rt, IS_AIR, &[state]) } else { rt.call_functional(TEST, a(2), &[state]) };
                if rt.thrown.is_some() || pass.i() != 0 {
                    return Some(V::I(rt.thrown.is_none() as i32));
                }
            }
            V::I(0)
        }
        // `BulkSectionAccess` over the region: a position's section (none outside the world's
        // height), and its states as the host has them.
        (BULK, "<init>") => {
            rt.put_field(a(0), "$level", a(1));
            V::Void
        }
        (BULK, "getSection") => {
            let [x, y, z] = crate::api::pos_of(rt, a(1));
            if !inside_height(rt, y) {
                return Some(V::NULL);
            }
            let section = rt.new_object(SECTION);
            for (field, v) in [("$x", x & !15), ("$y", y & !15), ("$z", z & !15)] {
                rt.put_field(section, field, V::I(v));
            }
            section
        }
        (BULK, "getBlockState") => {
            let p = crate::api::pos_of(rt, a(1));
            host_state(rt, p)
        }
        (BULK, "close") => V::Void,
        // A section's states by their coordinates in it; a write is the host's raw section write,
        // giving the state it replaced.
        (SECTION, "getBlockState") => {
            let p = section_pos(rt, a(0), a(1).i(), a(2).i(), a(3).i());
            host_state(rt, p)
        }
        (SECTION, "setBlockState") => {
            let p = section_pos(rt, a(0), a(1).i(), a(2).i(), a(3).i());
            let old = host_state(rt, p);
            let text = state_text(rt, a(4));
            if let Some(h) = rt.host_mut() {
                h.set_section_state(p, &text);
            }
            old
        }
        (SECTION, "acquire" | "release") => V::Void,
        (SECTION, "hasOnlyAir") => V::I(0),
        // `SectionPos`'s block and section coordinates.
        (SECTION_POS, "sectionRelative") => V::I(a(0).i() & 15),
        (SECTION_POS, "blockToSectionCoord") => V::I(if desc == "(D)I" { (a(0).d().floor() as i32) >> 4 } else { a(0).i() >> 4 }),
        (SECTION_POS, "sectionToBlockCoord") => V::I((a(0).i() << 4).wrapping_add(if desc == "(II)I" { a(1).i() } else { 0 })),
        (SECTION_POS, "asLong") if desc == "(III)J" => {
            let (x, y, z) = (i64::from(a(0).i()), i64::from(a(1).i()), i64::from(a(2).i()));
            V::J(((x & 0x3F_FFFF) << 42) | (y & 0xF_FFFF) | ((z & 0x3F_FFFF) << 20))
        }
        (SECTION_POS, "x") if desc == "(J)I" => V::I((a(0).j() >> 42) as i32),
        (SECTION_POS, "y") if desc == "(J)I" => V::I(((a(0).j() << 44) >> 44) as i32),
        (SECTION_POS, "z") if desc == "(J)I" => V::I(((a(0).j() << 22) >> 42) as i32),
        // `SimplexNoise(random)`: its offsets and permutation, drawn from the random as vanilla
        // draws them; `getValue` in two and three dimensions.
        (SIMPLEX, "<init>") => {
            let mut offsets = [0.0; 3];
            for o in &mut offsets {
                *o = crate::api::call(rt, NEXT_DOUBLE, &[a(1)]).d() * 256.0;
            }
            let mut p: Vec<u8> = (0..=255).collect();
            for i in 0..256usize {
                let k = crate::api::call(rt, NEXT_INT_BOUND, &[a(1), V::I(256 - i as i32)]).i();
                p.swap(i, i + k.clamp(0, 255 - i as i32) as usize);
            }
            for (field, o) in ["xo", "yo", "zo"].into_iter().zip(offsets) {
                rt.put_field(a(0), field, V::D(o));
            }
            let table = rt.alloc("[I", crate::Data::Bytes(p, 0));
            rt.put_field(a(0), "$p", table);
            V::Void
        }
        (SIMPLEX, "getValue") => {
            let table = rt.get_field(a(0), "$p");
            let p: [u8; 256] = match rt.heap.get(table.r() as usize).map(|o| &o.data) {
                Some(crate::Data::Bytes(b, _)) if b.len() == 256 => b[..].try_into().unwrap_or([0; 256]),
                _ => [0; 256],
            };
            V::D(if desc == "(DD)D" { simplex2(&p, a(1).d(), a(2).d()) } else { simplex3(&p, a(1).d(), a(2).d(), a(3).d()) })
        }
        // The context a feature places in, as `place` makes it: what it was made with, read back.
        (PLACE_CONTEXT, "<init>") => {
            let fields: Vec<_> = PLACE_CONTEXT_FIELDS.into_iter().enumerate().map(|(i, f)| (f, a(i + 1))).collect();
            store(rt, a(0), &fields)
        }
        (PLACE_CONTEXT, name) if desc.starts_with("()") => match PLACE_CONTEXT_FIELDS.into_iter().find(|f| *f == name) {
            Some(field) => rt.get_field(a(0), field),
            None => return None,
        },
        // Ores: their targets (`OreConfiguration.target(rule, state)`, or one made in a
        // constructor), size and chance to be left where air touches them.
        (ORE_CONFIGURATION, "target") => target(rt, a(0), a(1)),
        (TARGET, "<init>") => store(rt, a(0), &[("target", a(1)), ("state", a(2))]),
        (ORE_CONFIGURATION, "<init>") => {
            let (targets, rest) = if desc.starts_with("(Ljava/util/List;") {
                (a(1), 2)
            } else {
                let one = target(rt, a(1), a(2));
                (crate::jutil::new_list(rt, vec![one]), 3)
            };
            let discard = if desc.contains("IF)") { a(rest + 1) } else { V::F(0.0) };
            store(rt, a(0), &[("targetStates", targets), ("size", a(rest)), ("discardChanceOnAirExposure", discard)])
        }
        // Rule tests made in code: their fields, as the codec reads them.
        (TAG_MATCH, "<init>") => store(rt, a(0), &[("tag", a(1))]),
        (BLOCK_MATCH, "<init>") => store(rt, a(0), &[("block", a(1))]),
        (STATE_MATCH, "<init>") => store(rt, a(0), &[("blockState", a(1))]),
        (RANDOM_BLOCK_MATCH, "<init>") => store(rt, a(0), &[("block", a(1)), ("probability", a(2))]),
        (RANDOM_STATE_MATCH, "<init>") => store(rt, a(0), &[("blockState", a(1)), ("probability", a(2))]),
        (ALWAYS_TRUE, "<init>") => V::Void,
        // `RuleTest.test(state, random)`, by the test's class.
        (_, "test") if desc == "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/util/RandomSource;)Z" => {
            let class = rt.class_of(a(0)).to_owned();
            let state = a(1);
            let block_is = |rt: &mut Rt, block: V| crate::api::block_name(rt, block) == state_text(rt, state).split('[').next().unwrap_or("");
            let chance = |rt: &mut Rt| crate::api::call(rt, NEXT_FLOAT, &[a(2)]).f() < rt.get_field(a(0), "probability").f();
            V::I(match class.as_str() {
                TAG_MATCH => {
                    let tag = rt.get_field(a(0), "tag");
                    crate::api::call(rt, STATE_IN_TAG, &[state, tag]).i() != 0
                }
                BLOCK_MATCH => {
                    let block = rt.get_field(a(0), "block");
                    block_is(rt, block)
                }
                STATE_MATCH => same_state(&state_text(rt, rt.get_field(a(0), "blockState")), &state_text(rt, state)),
                RANDOM_BLOCK_MATCH => {
                    let block = rt.get_field(a(0), "block");
                    block_is(rt, block) && chance(rt)
                }
                RANDOM_STATE_MATCH => same_state(&state_text(rt, rt.get_field(a(0), "blockState")), &state_text(rt, state)) && chance(rt),
                ALWAYS_TRUE => true,
                _ => return None,
            } as i32)
        }
        _ => return None,
    })
}

/// Whether world state `state` is rule state `rule` (`BlockStateMatchTest`'s `==`): the same block
/// with the properties the rule names. A rule state read without properties is its block's default
/// in vanilla; here, where blocks' defaults are the game's, it is any state of the block.
fn same_state(rule: &str, state: &str) -> bool {
    let (rule_block, rule_properties) = crate::host::split_state(rule);
    let (block, properties) = crate::host::split_state(state);
    rule_block == block && rule_properties.iter().all(|p| properties.contains(p))
}

/// Fields set on a new object (a constructor that only stores its arguments).
fn store(rt: &mut Rt, object: V, fields: &[(&'static str, V)]) -> V {
    for &(field, v) in fields {
        rt.put_field(object, field, v);
    }
    V::Void
}

/// An ore target (`OreConfiguration.TargetBlockState`): its rule test and the state it places.
fn target(rt: &mut Rt, rule: V, state: V) -> V {
    let t = rt.new_object(TARGET);
    rt.put_field(t, "target", rule);
    rt.put_field(t, "state", state);
    t
}

/// The world position of coordinates `x`, `y`, `z` in section `section`.
fn section_pos(rt: &Rt, section: V, x: i32, y: i32, z: i32) -> [i32; 3] {
    let at = |f| rt.get_field(section, f).i();
    [at("$x") + (x & 15), at("$y") + (y & 15), at("$z") + (z & 15)]
}

/// Reads what `RuleTest.CODEC` (by `predicate_type`) and `OreConfiguration.TargetBlockState.CODEC`
/// (`{target, state}`) write.
pub(crate) fn decode_native(rt: &mut Rt, name: &str, j: &Json) -> Result<V, String> {
    let o = j.as_object().ok_or_else(|| format!("Not a map: {j}"))?;
    let field = |key: &str| o.get(key).ok_or_else(|| format!("No key {key} in MapLike[{j}]"));
    match name {
        // `OreConfiguration.CODEC`, for a mod's feature made with it.
        "ore_configuration" => {
            let mut targets = Vec::new();
            for t in field("targets")?.as_array().ok_or_else(|| format!("Not a list: {j}"))? {
                targets.push(decode_native(rt, "target_block_state", t)?);
            }
            let size = field("size")?.as_i64().filter(|s| (0..=64).contains(s)).ok_or_else(|| format!("Value outside of range [0:64]: {j}"))?;
            let discard = o.get("discard_chance_on_air_exposure").map_or(Some(0.0), Json::as_f64).filter(|d| (0.0..=1.0).contains(d));
            let discard = discard.ok_or_else(|| format!("Value outside of range [0.0:1.0]: {j}"))?;
            let list = crate::jutil::new_list(rt, targets);
            let ore = rt.new_object(ORE_CONFIGURATION);
            store(rt, ore, &[("targetStates", list), ("size", V::I(size as i32)), ("discardChanceOnAirExposure", V::F(discard as f32))]);
            Ok(ore)
        }
        "target_block_state" => {
            let rule = decode_native(rt, "rule_test", field("target")?)?;
            let state = crate::recipes::decode_native(rt, "block_state", field("state")?)?;
            Ok(target(rt, rule, state))
        }
        "rule_test" => {
            let kind = field("predicate_type")?.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let kind = kind.strip_prefix("minecraft:").unwrap_or(kind);
            let probability = || -> Result<V, String> {
                let p = field("probability")?.as_f64().ok_or_else(|| format!("Not a number: {j}"))?;
                Ok(V::F(p as f32))
            };
            let block = |rt: &mut Rt| -> Result<V, String> {
                let id = field("block")?.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
                Ok(crate::api::block(rt, &if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }))
            };
            let (class, fields): (&'static str, Vec<(&'static str, V)>) = match kind {
                "tag_match" => {
                    let tag = field("tag")?.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
                    let tag = if tag.contains(':') { tag.to_owned() } else { format!("minecraft:{tag}") };
                    (TAG_MATCH, vec![("tag", crate::fluids::tag_key(rt, "block", &tag))])
                }
                "block_match" => (BLOCK_MATCH, vec![("block", block(rt)?)]),
                "blockstate_match" => (STATE_MATCH, vec![("blockState", crate::recipes::decode_native(rt, "block_state", field("block_state")?)?)]),
                "random_block_match" => (RANDOM_BLOCK_MATCH, vec![("block", block(rt)?), ("probability", probability()?)]),
                "random_blockstate_match" => (
                    RANDOM_STATE_MATCH,
                    vec![("blockState", crate::recipes::decode_native(rt, "block_state", field("block_state")?)?), ("probability", probability()?)],
                ),
                "always_true" => (ALWAYS_TRUE, Vec::new()),
                other => return Err(format!("Unknown rule test type: {other}")),
            };
            let test = rt.new_object(class);
            store(rt, test, &fields);
            Ok(test)
        }
        other => Err(format!("codec {other} can't read")),
    }
}

/// Vanilla's `SimplexNoise` gradients (`GRADIENT`): Perlin's twelve edges, four repeated.
const GRADIENTS: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [0.0, -1.0, 1.0],
    [-1.0, 1.0, 0.0],
    [0.0, -1.0, -1.0],
];

/// `SimplexNoise.getCornerNoise3D`: a corner's contribution, none past `radius`.
fn corner(gradient: usize, x: f64, y: f64, z: f64, radius: f64) -> f64 {
    let d = radius - x * x - y * y - z * z;
    if d < 0.0 {
        0.0
    } else {
        let d = d * d;
        let g = GRADIENTS[gradient];
        d * d * (g[0] * x + g[1] * y + g[2] * z)
    }
}

/// `Mth.floor` of a double.
fn floor(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

/// `SimplexNoise.getValue(x, y)` over permutation `p` (the offsets are the caller's to add).
fn simplex2(p: &[u8; 256], x: f64, y: f64) -> f64 {
    let perm = |i: i32| i32::from(p[(i & 255) as usize]);
    let sqrt3 = 3.0_f64.sqrt();
    let (f2, g2) = (0.5 * (sqrt3 - 1.0), (3.0 - sqrt3) / 6.0);
    let s = (x + y) * f2;
    let (i, j) = (floor(x + s), floor(y + s));
    let t = f64::from(i.wrapping_add(j)) * g2;
    let (x0, y0) = (x - (f64::from(i) - t), y - (f64::from(j) - t));
    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let (x1, y1) = (x0 - f64::from(i1) + g2, y0 - f64::from(j1) + g2);
    let (x2, y2) = (x0 - 1.0 + 2.0 * g2, y0 - 1.0 + 2.0 * g2);
    let (ii, jj) = (i & 255, j & 255);
    let g0 = (perm(ii + perm(jj)) % 12) as usize;
    let g1 = (perm(ii + i1 + perm(jj + j1)) % 12) as usize;
    let g2i = (perm(ii + 1 + perm(jj + 1)) % 12) as usize;
    70.0 * (corner(g0, x0, y0, 0.0, 0.5) + corner(g1, x1, y1, 0.0, 0.5) + corner(g2i, x2, y2, 0.0, 0.5))
}

/// `SimplexNoise.getValue(x, y, z)` over permutation `p`.
fn simplex3(p: &[u8; 256], x: f64, y: f64, z: f64) -> f64 {
    let perm = |i: i32| i32::from(p[(i & 255) as usize]);
    let s = (x + y + z) * (1.0 / 3.0);
    let (i, j, k) = (floor(x + s), floor(y + s), floor(z + s));
    let t = f64::from(i.wrapping_add(j).wrapping_add(k)) * (1.0 / 6.0);
    let (x0, y0, z0) = (x - (f64::from(i) - t), y - (f64::from(j) - t), z - (f64::from(k) - t));
    let ([i1, j1, k1], [i2, j2, k2]) = if x0 >= y0 {
        if y0 >= z0 {
            ([1, 0, 0], [1, 1, 0])
        } else if x0 >= z0 {
            ([1, 0, 0], [1, 0, 1])
        } else {
            ([0, 0, 1], [1, 0, 1])
        }
    } else if y0 < z0 {
        ([0, 0, 1], [0, 1, 1])
    } else if x0 < z0 {
        ([0, 1, 0], [0, 1, 1])
    } else {
        ([0, 1, 0], [1, 1, 0])
    };
    let (x1, y1, z1) = (x0 - f64::from(i1) + 1.0 / 6.0, y0 - f64::from(j1) + 1.0 / 6.0, z0 - f64::from(k1) + 1.0 / 6.0);
    let (x2, y2, z2) = (x0 - f64::from(i2) + 1.0 / 3.0, y0 - f64::from(j2) + 1.0 / 3.0, z0 - f64::from(k2) + 1.0 / 3.0);
    let (x3, y3, z3) = (x0 - 1.0 + 0.5, y0 - 1.0 + 0.5, z0 - 1.0 + 0.5);
    let (ii, jj, kk) = (i & 255, j & 255, k & 255);
    let g0 = (perm(ii + perm(jj + perm(kk))) % 12) as usize;
    let g1 = (perm(ii + i1 + perm(jj + j1 + perm(kk + k1))) % 12) as usize;
    let g2 = (perm(ii + i2 + perm(jj + j2 + perm(kk + k2))) % 12) as usize;
    let g3 = (perm(ii + 1 + perm(jj + 1 + perm(kk + 1))) % 12) as usize;
    32.0 * (corner(g0, x0, y0, z0, 0.6) + corner(g1, x1, y1, z1, 0.6) + corner(g2, x2, y2, z2, 0.6) + corner(g3, x3, y3, z3, 0.6))
}

/// A draw from the worldgen random (`RandomSource`'s methods on it): the host's stream, as
/// vanilla's default methods compose them. Other sources are the runtime's own.
fn random_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if rt.class_of(a(0)) != WORLDGEN_RANDOM {
        return None;
    }
    let r = rt.host_mut()?.random()?;
    Some(match (name, desc) {
        ("nextInt", "()I") => V::I(r.next_int()),
        ("nextInt", "(I)I") => V::I(r.next_int_bound(a(1).i())),
        // `origin + nextInt(bound - origin)`, `min + nextInt(max - min + 1)`.
        ("nextInt", "(II)I") => V::I(a(1).i().wrapping_add(r.next_int_bound(a(2).i().wrapping_sub(a(1).i())))),
        ("nextIntBetweenInclusive", "(II)I") => V::I(a(1).i().wrapping_add(r.next_int_bound(a(2).i().wrapping_sub(a(1).i()).wrapping_add(1)))),
        ("nextLong", "()J") => V::J(r.next_long()),
        ("nextBoolean", "()Z") => V::I(r.next_boolean() as i32),
        ("nextFloat", "()F") => V::F(r.next_float()),
        ("nextDouble", "()D") => V::D(r.next_double()),
        ("nextGaussian", "()D") => V::D(r.next_gaussian()),
        ("triangle", "(DD)D") => V::D(a(1).d() + a(2).d() * (r.next_double() - r.next_double())),
        ("consumeCount", "(I)V") => {
            for _ in 0..a(1).i() {
                r.next_int();
            }
            V::Void
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, HostRandom};

    /// `java.util.Random`, as the generator's legacy stream draws (an oracle apart from the runtime).
    struct JavaRandom(u64);
    impl JavaRandom {
        fn new(seed: i64) -> Self {
            Self((seed as u64 ^ 0x5_DEEC_E66D) & ((1 << 48) - 1))
        }
        fn next(&mut self, bits: u32) -> i32 {
            self.0 = (self.0.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
            (self.0 >> (48 - bits)) as i32
        }
    }
    impl HostRandom for JavaRandom {
        fn next_int(&mut self) -> i32 {
            self.next(32)
        }
        fn next_int_bound(&mut self, bound: i32) -> i32 {
            if bound <= 0 {
                return 0;
            }
            if bound & -bound == bound {
                return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
            }
            loop {
                let bits = self.next(31);
                let v = bits % bound;
                if bits - v + (bound - 1) >= 0 {
                    return v;
                }
            }
        }
        fn next_long(&mut self) -> i64 {
            (i64::from(self.next(32)) << 32).wrapping_add(i64::from(self.next(32)))
        }
        fn next_boolean(&mut self) -> bool {
            self.next(1) != 0
        }
        fn next_float(&mut self) -> f32 {
            self.next(24) as f32 / (1 << 24) as f32
        }
        fn next_double(&mut self) -> f64 {
            ((i64::from(self.next(26)) << 27) + i64::from(self.next(27))) as f64 * (1.0 / (1u64 << 53) as f64)
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    /// The generator's region: stone everywhere, its surface at y 70, its random lent.
    struct Region(JavaRandom);
    impl Host for Region {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:stone".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn random(&mut self) -> Option<&mut dyn HostRandom> {
            Some(&mut self.0)
        }
        fn height(&mut self, heightmap: &str, _x: i32, _z: i32) -> Option<i32> {
            (heightmap == "WORLD_SURFACE_WG").then_some(70)
        }
        // Desert west of x 0, plains east of it; tag `test:warm` holds the desert.
        fn biome(&self, pos: [i32; 3]) -> Option<String> {
            Some(if pos[0] < 0 { "minecraft:desert" } else { "minecraft:plains" }.into())
        }
        fn biome_in_tag(&self, biome: &str, tag: &str) -> bool {
            tag == "test:warm" && biome == "minecraft:desert"
        }
    }

    const APPLY_SHORT: &str = "apply.(Ljava/lang/Object;)Ljava/lang/Object;";

    /// The mod's code: a filter whose `shouldPlace` takes a position below the surface when a draw
    /// of two is 0 (`random.nextInt(2) == 0 && context.getHeight(WORLD_SURFACE_WG, x, z) > y`), and
    /// a repeating modifier whose `count` is 3; vanilla's own `getPositions` runs around both.
    /// `test/WarmFilter`'s `shouldPlace` is Farmer's Delight's `BiomeTagFilter`'s:
    /// `context.getLevel().getBiome(pos).is(TagKey.create(Registries.BIOME, "test:warm"))`.
    fn mod_code(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        match key {
            SHOULD_PLACE if rt.class_of(args[0]) == "test/WarmFilter" => {
                let level = crate::api::call(rt, "net/minecraft/world/level/levelgen/placement/PlacementContext.getLevel.()Lnet/minecraft/world/level/WorldGenLevel;", &[args[1]]);
                let biome = crate::api::call(rt, "net/minecraft/world/level/WorldGenLevel.getBiome.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/Holder;", &[level, args[3]]);
                let registry = crate::api::get_static(rt, "net/minecraft/core/registries/Registries.BIOME");
                let name = rt.string("test:warm");
                let location = crate::api::call(rt, "net/minecraft/resources/ResourceLocation.parse.(Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;", &[name]);
                let tag = crate::api::call(rt, "net/minecraft/tags/TagKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;", &[registry, location]);
                crate::api::call(rt, "net/minecraft/core/Holder.is.(Lnet/minecraft/tags/TagKey;)Z", &[biome, tag])
            }
            SHOULD_PLACE => {
                let n = crate::api::call(rt, "net/minecraft/util/RandomSource.nextInt.(I)I", &[args[2], V::I(2)]);
                if n.i() != 0 {
                    return V::I(0);
                }
                let surface = crate::api::get_static(rt, "net/minecraft/world/level/levelgen/Heightmap$Types.WORLD_SURFACE_WG");
                let [x, y, z] = crate::api::pos_of(rt, args[3]);
                let top = crate::api::call(rt, "net/minecraft/world/level/levelgen/placement/PlacementContext.getHeight.(Lnet/minecraft/world/level/levelgen/Heightmap$Types;II)I", &[args[1], surface, V::I(x), V::I(z)]);
                V::I((top.i() > y) as i32)
            }
            COUNT => V::I(3),
            PLACE if rt.class_of(args[0]) == "test/Vein" => vein(rt, args[1]),
            APPLY_SHORT if rt.class_of(args[0]) == "test/Getter" => {
                let level = rt.get_field(args[0], "level");
                crate::api::call(rt, "net/minecraft/world/level/WorldGenLevel.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;", &[level, args[1]])
            }
            _ => crate::api::call(rt, owner_key, args),
        }
    }

    /// `test/Vein.place(context)`: a column of seven blocks centred on the origin, written through
    /// `BulkSectionAccess` and its sections as Create's layered ores write. A position next to air
    /// (`Feature.isAdjacentToAir`, inherited) is left; otherwise the first of the config's targets
    /// (the config is the target list) whose rule takes the old state places its state. Placed when
    /// a section write gave back a state that wasn't air.
    fn vein(rt: &mut Rt, context: V) -> V {
        let call = crate::api::call;
        let context_key = |m: &str| -> &'static str {
            Box::leak(format!("net/minecraft/world/level/levelgen/feature/FeaturePlaceContext.{m}").into_boxed_str())
        };
        let level = call(rt, context_key("level.()Lnet/minecraft/world/level/WorldGenLevel;"), &[context]);
        let origin = call(rt, context_key("origin.()Lnet/minecraft/core/BlockPos;"), &[context]);
        let random = call(rt, context_key("random.()Lnet/minecraft/util/RandomSource;"), &[context]);
        let targets = call(rt, context_key("config.()Lnet/minecraft/world/level/levelgen/feature/configurations/FeatureConfiguration;"), &[context]);
        let access = rt.new_object(BULK);
        call(rt, "net/minecraft/world/level/chunk/BulkSectionAccess.<init>.(Lnet/minecraft/world/level/LevelAccessor;)V", &[access, level]);
        let getter = rt.new_object("test/Getter");
        rt.put_field(getter, "level", level);
        let [x, y, z] = crate::api::pos_of(rt, origin);
        let mut replaced = 0;
        for dy in -3..=3 {
            let pos = crate::api::block_pos(rt, i64::from(x), i64::from(y + dy), i64::from(z));
            let section = call(rt, "net/minecraft/world/level/chunk/BulkSectionAccess.getSection.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/chunk/LevelChunkSection;", &[access, pos]);
            if section.is_null() {
                continue;
            }
            let [rx, ry, rz] = [x, y + dy, z].map(|c| call(rt, "net/minecraft/core/SectionPos.sectionRelative.(I)I", &[V::I(c)]));
            let old = call(rt, "net/minecraft/world/level/chunk/LevelChunkSection.getBlockState.(III)Lnet/minecraft/world/level/block/state/BlockState;", &[section, rx, ry, rz]);
            if call(rt, "test/Vein.isAdjacentToAir.(Ljava/util/function/Function;Lnet/minecraft/core/BlockPos;)Z", &[getter, pos]).i() != 0 {
                continue;
            }
            for i in 0..call(rt, "java/util/List.size.()I", &[targets]).i() {
                let target = call(rt, "java/util/List.get.(I)Ljava/lang/Object;", &[targets, V::I(i)]);
                let rule = rt.get_field(target, "target");
                if call(rt, "net/minecraft/world/level/levelgen/structure/templatesystem/RuleTest.test.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/util/RandomSource;)Z", &[rule, old, random]).i() != 0 {
                    let state = rt.get_field(target, "state");
                    let was = call(rt, "net/minecraft/world/level/chunk/LevelChunkSection.setBlockState.(IIILnet/minecraft/world/level/block/state/BlockState;Z)Lnet/minecraft/world/level/block/state/BlockState;", &[section, rx, ry, rz, state, V::I(0)]);
                    if call(rt, "net/minecraft/world/level/block/state/BlockState.isAir.()Z", &[was]).i() == 0 {
                        replaced += 1;
                    }
                    break;
                }
            }
        }
        call(rt, "net/minecraft/world/level/chunk/BulkSectionAccess.close.()V", &[access]);
        V::I((replaced > 0) as i32)
    }

    /// A region of rock: deepslate below y 0, stone up to y 9, air above, and the edge of what may
    /// be written at x 100. Writes are kept apart by how they were made (a section's raw write, a
    /// level's `setBlock`); its random is lent.
    struct Rock {
        random: JavaRandom,
        sections: std::collections::BTreeMap<[i32; 3], String>,
        blocks: std::collections::BTreeMap<[i32; 3], String>,
    }
    impl Rock {
        fn new(seed: i64) -> Self {
            Self { random: JavaRandom::new(seed), sections: Default::default(), blocks: Default::default() }
        }
    }
    impl Host for Rock {
        fn block_state(&self, pos: [i32; 3]) -> String {
            if let Some(s) = self.sections.get(&pos).or_else(|| self.blocks.get(&pos)) {
                return s.clone();
            }
            match pos[1] {
                ..0 => "minecraft:deepslate[axis=y]",
                0..=9 => "minecraft:stone",
                _ => "minecraft:air",
            }
            .into()
        }
        fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {
            self.blocks.insert(pos, state.into());
            true
        }
        fn set_section_state(&mut self, pos: [i32; 3], state: &str) -> bool {
            self.sections.insert(pos, state.into());
            true
        }
        fn random(&mut self) -> Option<&mut dyn HostRandom> {
            Some(&mut self.random)
        }
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {
            tag == "minecraft:stone_ore_replaceables" && block == "minecraft:stone"
        }
        fn can_write(&self, pos: [i32; 3]) -> bool {
            pos[0] < 100
        }
    }

    fn register_feature(rt: &mut Rt, id: &str, class: &'static str, codec: V) -> V {
        rt.supers.insert(class, FEATURE);
        let feature = rt.new_object(class);
        crate::api::call(rt, "net/minecraft/world/level/levelgen/feature/Feature.<init>.(Lcom/mojang/serialization/Codec;)V", &[feature, codec]);
        rt.registered.insert(("feature".into(), id.into()), feature);
        feature
    }

    /// The targets of an ore, in both of the shapes a configured feature comes in: 1.21.1's (a
    /// `config` object, states by `Name`) and 26.3's (fields inline, states by `id`).
    const VEIN_121: &str = r#"{"type": "test:vein", "config": {"targets": [
        {"target": {"predicate_type": "minecraft:tag_match", "tag": "minecraft:stone_ore_replaceables"}, "state": {"Name": "test:ore"}},
        {"target": {"predicate_type": "minecraft:block_match", "block": "minecraft:deepslate"}, "state": {"Name": "test:deep_ore"}}]}}"#;
    const VEIN_263: &str = r#"{"type": "test:vein", "targets": [
        {"target": {"predicate_type": "tag_match", "tag": "stone_ore_replaceables"}, "state": {"id": "test:ore"}},
        {"target": {"predicate_type": "minecraft:block_match", "block": "deepslate"}, "state": "test:deep_ore"}]}"#;

    /// A feature type a mod registered reads its configured features with the codec it was made
    /// with, in either shape, and places through sections: what it writes is the host's raw
    /// section writes, at the positions its rule tests take, nothing outside the world's height,
    /// nothing where the region can't write.
    #[test]
    fn a_mods_feature_places_by_its_rule_tests_through_sections() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(mod_code);
        let target = crate::api::get_static(&mut rt, "net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration$TargetBlockState.CODEC");
        let list = crate::api::call(&mut rt, "com/mojang/serialization/Codec.listOf.()Lcom/mojang/serialization/Codec;", &[target]);
        let name = rt.string("targets");
        let field = crate::api::call(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[list, name]);
        let codec = crate::api::call(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[field]);
        let vein = register_feature(&mut rt, "test:vein", "test/Vein", codec);
        assert!(feature(&mut rt, "test:other", VEIN_121).is_none());
        let shapes = [VEIN_121, VEIN_263].map(|json| feature(&mut rt, "test:vein", json).unwrap().unwrap());
        assert_eq!(rt.get_field(shapes[0], "feature"), vein);
        let column = |from: i32, to: i32, state: &str| (from..=to).map(|y| ([5, y, 5], state.to_owned())).collect::<Vec<_>>();
        for configured in shapes {
            let mut place_at = |origin: [i32; 3]| {
                let mut rock = Rock::new(3);
                let placed = rt.with_host(&mut rock, |rt| place(rt, configured, origin)).unwrap();
                assert!(rock.blocks.is_empty(), "{origin:?}: a section write isn't a level's setBlock");
                (placed, rock.sections.into_iter().collect::<Vec<_>>())
            };
            // Stone up to y 8: y 9 is next to the air above it, y 10 is air.
            assert_eq!(place_at([5, 7, 5]), (true, column(4, 8, "test:ore")));
            // Deepslate below y 0 takes the second target.
            assert_eq!(place_at([5, -1, 5]), (true, [column(-4, -1, "test:deep_ore"), column(0, 2, "test:ore")].concat()));
            // Below the world's bottom there is no section.
            assert_eq!(place_at([5, -63, 5]), (true, column(-64, -60, "test:deep_ore")));
            assert_eq!(place_at([5, 50, 5]), (false, vec![]));
            assert_eq!(place_at([150, 5, 5]), (false, vec![]), "the region can't write there");
        }
        let broken = feature(&mut rt, "test:vein", r#"{"type": "test:vein", "targets": [{"target": {"predicate_type": "test:nope"}, "state": "test:ore"}]}"#).unwrap();
        assert!(broken.is_err_and(|e| e.contains("test:nope")));
    }

    /// The rule tests read by `RuleTest.CODEC` test a state as vanilla's classes do: a tag by the
    /// host's tags, a block by its name, a state by its properties, and a random one also by a
    /// draw of a float from the stream the host lends (`random.nextFloat() < probability`).
    #[test]
    fn rule_tests_test_states_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let codec = crate::api::get_static(&mut rt, "net/minecraft/world/level/levelgen/structure/templatesystem/RuleTest.CODEC");
        let mut read = |json: &str| crate::codecs::decode(&mut rt, codec, &serde_json::from_str(json).unwrap());
        let tag = read(r#"{"predicate_type": "minecraft:tag_match", "tag": "minecraft:stone_ore_replaceables"}"#).unwrap();
        let block = read(r#"{"predicate_type": "minecraft:block_match", "block": "minecraft:deepslate"}"#).unwrap();
        let state = read(r#"{"predicate_type": "minecraft:blockstate_match", "block_state": {"Name": "minecraft:deepslate", "Properties": {"axis": "x"}}}"#).unwrap();
        let chance = read(r#"{"predicate_type": "minecraft:random_block_match", "block": "minecraft:stone", "probability": 0.3}"#).unwrap();
        let always = read(r#"{"predicate_type": "minecraft:always_true"}"#).unwrap();
        assert!(read(r#"{"predicate_type": "minecraft:tag_match"}"#).is_err());
        let mut rock = Rock::new(77);
        let (fixed, draws) = rt.with_host(&mut rock, |rt| {
            let random = rt.new_object(WORLDGEN_RANDOM);
            let mut test = |rule: V, text: &str| {
                let s = crate::api::state(rt, text);
                crate::api::call(rt, "net/minecraft/world/level/levelgen/structure/templatesystem/RuleTest.test.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/util/RandomSource;)Z", &[rule, s, random]).i() != 0
            };
            let fixed = [
                test(tag, "minecraft:stone"),
                test(tag, "minecraft:dirt"),
                test(block, "minecraft:deepslate[axis=z]"),
                test(block, "minecraft:stone"),
                test(state, "minecraft:deepslate[axis=x]"),
                test(state, "minecraft:deepslate[axis=y]"),
                test(always, "minecraft:air"),
                test(chance, "minecraft:dirt"),
            ];
            let draws: Vec<bool> = (0..32).map(|_| test(chance, "minecraft:stone")).collect();
            (fixed, draws)
        });
        assert_eq!(fixed, [true, false, true, false, true, false, true, false]);
        let mut oracle = JavaRandom::new(77);
        let expected: Vec<bool> = (0..32).map(|_| oracle.next_float() < 0.3).collect();
        assert_eq!(draws, expected);
        assert!(expected.contains(&true) && expected.contains(&false));
        assert_eq!(rock.random.next_long(), oracle.next_long(), "a draw for each test of stone, none for the rest");
    }

    /// `OreConfiguration` (modelled here, not by the data-class table) as vanilla's codec reads it
    /// for a mod's feature made with it, and as its constructors make it: targets, size, and the
    /// chance to be left where air touches it (0 when a constructor doesn't take one).
    #[test]
    fn ore_configurations_read_and_made_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let codec = crate::api::get_static(&mut rt, "net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration.CODEC");
        let first_target = |rt: &mut Rt, ore: V| {
            let targets = rt.get_field(ore, "targetStates");
            assert_eq!(crate::api::call(rt, "java/util/List.size.()I", &[targets]), V::I(1));
            crate::api::call(rt, "java/util/List.get.(I)Ljava/lang/Object;", &[targets, V::I(0)])
        };
        let json = serde_json::json!({"targets": [{"target": {"predicate_type": "minecraft:always_true"}, "state": {"Name": "test:ore"}}], "size": 9, "discard_chance_on_air_exposure": 0.5});
        let read = crate::codecs::decode(&mut rt, codec, &json).unwrap();
        assert_eq!([rt.get_field(read, "size"), rt.get_field(read, "discardChanceOnAirExposure")], [V::I(9), V::F(0.5)]);
        let target = first_target(&mut rt, read);
        assert_eq!(rt.str_of(rt.get_field(target, "state")), Some("test:ore"));
        assert_eq!(rt.class_of(rt.get_field(target, "target")), ALWAYS_TRUE);
        assert!(crate::codecs::decode(&mut rt, codec, &serde_json::json!({"targets": [], "size": 65})).is_err(), "size is 0 to 64");
        let (rule, state) = (rt.new_object(TAG_MATCH), crate::api::state(&mut rt, "test:ore"));
        let made = rt.new_object(ORE_CONFIGURATION);
        crate::api::call(&mut rt, "net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration.<init>.(Lnet/minecraft/world/level/levelgen/structure/templatesystem/RuleTest;Lnet/minecraft/world/level/block/state/BlockState;I)V", &[made, rule, state, V::I(4)]);
        assert_eq!([rt.get_field(made, "size"), rt.get_field(made, "discardChanceOnAirExposure")], [V::I(4), V::F(0.0)]);
        let target = first_target(&mut rt, made);
        assert_eq!([rt.get_field(target, "target"), rt.get_field(target, "state")], [rule, state]);
    }

    /// Vanilla 1.21.1's `SimplexNoise(new LegacyRandomSource(seed))` (the oracle run on the
    /// Mojmapped 1.21.1 classes, docs/mod-translator.md): its offsets' bits, the stream's next long
    /// after it, and at each point `getValue(x, y)`'s and `getValue(x, y, z)`'s bits.
    #[allow(clippy::type_complexity)]
    const SIMPLEX_ORACLE: [(i64, [i64; 3], i64, [(f64, f64, f64, i64, i64); 5]); 3] = [
        (
            0,
            [4640787989073939081, 4633863055005396500, 4639945362322812801],
            -6819959017335327829,
            [
                (0.0, 0.0, 0.0, 0, 0),
                (0.3, -1.7, 2.25, -4653009924132586597, 4606446865970192322),
                (123.456, 7.89, -45.6, -4618366171218036683, -4620327100580967916),
                (-1000.5, 64.0, 0.125, -4626847780491364372, 4606387765019498900),
                (17.0, 17.0, 17.0, 4604189417275839225, 0),
            ],
        ),
        (
            42,
            [4640757327600809047, 4640357946509897217, 4635261812232862644],
            -4463560979761954064,
            [
                (0.0, 0.0, 0.0, 0, 0),
                (0.3, -1.7, 2.25, 4601245221622339552, 4606682270023997854),
                (123.456, 7.89, -45.6, -4633565012616449741, -4623196827317606730),
                (-1000.5, 64.0, 0.125, -4623075997505528546, 4603235062462477622),
                (17.0, 17.0, 17.0, 4602657814698697358, 0),
            ],
        ),
        (
            -7,
            [4634548484457662648, 4640267493679634287, 4638901177636610382],
            8936525092777621889,
            [
                (0.0, 0.0, 0.0, 0, 0),
                (0.3, -1.7, 2.25, -4653009924132586597, -4624521527753148201),
                (123.456, 7.89, -45.6, -4625264214537578603, 4605286082144231362),
                (-1000.5, 64.0, 0.125, 4601319526336890361, 4578327352411211712),
                (17.0, 17.0, 17.0, 4602657814698697358, 0),
            ],
        ),
    ];

    /// `SimplexNoise` draws its offsets and table from the random it is made with, and reads, as
    /// vanilla's.
    #[test]
    fn simplex_noise_draws_and_reads_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        for (seed, offsets, next, points) in SIMPLEX_ORACLE {
            let mut region = Region(JavaRandom::new(seed));
            rt.with_host(&mut region, |rt| {
                let random = rt.new_object(WORLDGEN_RANDOM);
                let noise = rt.new_object(SIMPLEX);
                crate::api::call(rt, "net/minecraft/world/level/levelgen/synth/SimplexNoise.<init>.(Lnet/minecraft/util/RandomSource;)V", &[noise, random]);
                for (field, bits) in ["xo", "yo", "zo"].into_iter().zip(offsets) {
                    assert_eq!(rt.get_field(noise, field).d().to_bits() as i64, bits, "seed {seed}: {field}");
                }
                for (x, y, z, flat, solid) in points {
                    let two = crate::api::call(rt, "net/minecraft/world/level/levelgen/synth/SimplexNoise.getValue.(DD)D", &[noise, V::D(x), V::D(y)]);
                    let three = crate::api::call(rt, "net/minecraft/world/level/levelgen/synth/SimplexNoise.getValue.(DDD)D", &[noise, V::D(x), V::D(y), V::D(z)]);
                    assert_eq!(two.d().to_bits() as i64, flat, "seed {seed}: ({x}, {y})");
                    assert_eq!(three.d().to_bits() as i64, solid, "seed {seed}: ({x}, {y}, {z})");
                }
            });
            assert_eq!(region.0.next_long(), next, "seed {seed}: the stream after the table");
        }
    }

    fn register_type(rt: &mut Rt, id: &str, class: &'static str, parent: &'static str) -> V {
        rt.supers.insert(class, parent);
        let instance = rt.new_object(class);
        let codec = crate::api::call(rt, "com/mojang/serialization/MapCodec.unit.(Ljava/lang/Object;)Lcom/mojang/serialization/MapCodec;", &[instance]);
        let ty = crate::functions::constant(rt, codec);
        rt.registered.insert(("placement_modifier_type".into(), id.into()), ty);
        instance
    }

    /// A placement modifier type a mod registered reads with its type's codec and places as
    /// vanilla's `PlacementFilter` does around the mod's `shouldPlace`, drawing from the stream the
    /// host lends; a type nobody registered isn't the runtime's to read.
    #[test]
    fn a_mods_filter_keeps_the_positions_its_should_place_takes_from_the_hosts_random() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(mod_code);
        let coin = register_type(&mut rt, "test:coin", "test/CoinFilter", FILTER);
        let read = placement(&mut rt, "test:coin", r#"{"type": "test:coin"}"#).unwrap().unwrap();
        assert_eq!(read, coin);
        assert!(placement(&mut rt, "test:other", r#"{"type": "test:other"}"#).is_none());
        let mut kept = 0;
        for seed in 0..16i64 {
            let seed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64);
            let mut region = Region(JavaRandom::new(seed));
            let below = rt.with_host(&mut region, |rt| positions(rt, read, [4, 30, -7])).unwrap();
            let above = rt.with_host(&mut region, |rt| positions(rt, read, [4, 80, -7])).unwrap();
            let mut oracle = JavaRandom::new(seed);
            let expected: Vec<[i32; 3]> = if oracle.next_int_bound(2) == 0 { vec![[4, 30, -7]] } else { vec![] };
            assert_eq!(below, expected, "seed {seed}");
            assert!(above.is_empty(), "seed {seed}: above the surface");
            // Each call drew once from the lent stream: what follows is the oracle's third draw.
            oracle.next_int_bound(2);
            assert_eq!(region.0.next_long(), oracle.next_long(), "seed {seed}");
            kept += below.len();
        }
        assert!(kept > 0 && kept < 16);
    }

    /// A repeating modifier gives its position as many times as the mod's `count` says, as
    /// vanilla's `RepeatingPlacement` streams it.
    #[test]
    fn a_mods_repeating_modifier_gives_its_position_count_times() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(mod_code);
        register_type(&mut rt, "test:thrice", "test/Thrice", REPEATING);
        let read = placement(&mut rt, "test:thrice", r#"{"type": "test:thrice"}"#).unwrap().unwrap();
        let mut region = Region(JavaRandom::new(1));
        let out = rt.with_host(&mut region, |rt| positions(rt, read, [0, 5, 0])).unwrap();
        assert_eq!(out, vec![[0, 5, 0]; 3]);
    }

    /// A filter that asks whether a biome tag holds the biome at its position (Farmer's Delight's
    /// wild crops' `BiomeTagFilter`) is answered from the host's biomes and tags.
    #[test]
    fn a_mods_filter_reads_the_biome_and_its_tags_from_the_host() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(mod_code);
        register_type(&mut rt, "test:warm", "test/WarmFilter", FILTER);
        let read = placement(&mut rt, "test:warm", r#"{"type": "test:warm"}"#).unwrap().unwrap();
        let mut region = Region(JavaRandom::new(1));
        let desert = rt.with_host(&mut region, |rt| positions(rt, read, [-8, 64, 3])).unwrap();
        let plains = rt.with_host(&mut region, |rt| positions(rt, read, [8, 64, 3])).unwrap();
        assert_eq!(desert, vec![[-8, 64, 3]]);
        assert!(plains.is_empty());
    }
}
