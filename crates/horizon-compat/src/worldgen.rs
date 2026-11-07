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
const RANDOM_PATCH: &str = "net/minecraft/world/level/levelgen/feature/configurations/RandomPatchConfiguration";
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
/// The registries a mod's biome modifier serializer is registered in
/// (`NeoForgeRegistries.BIOME_MODIFIER_SERIALIZERS`, as this runtime names registries).
const MODIFIER_REGISTRIES: [&str; 2] = ["biome_modifier_serializers", "neoforge:biome_modifier_serializers"];
const MODIFY: &str = "modify.(Lnet/minecraft/core/Holder;Lnet/neoforged/neoforge/common/world/BiomeModifier$Phase;Lnet/neoforged/neoforge/common/world/ModifiableBiomeInfo$BiomeInfo$Builder;)V";
const MODIFY_KEY: &str = "net/neoforged/neoforge/common/world/BiomeModifier.modify.(Lnet/minecraft/core/Holder;Lnet/neoforged/neoforge/common/world/BiomeModifier$Phase;Lnet/neoforged/neoforge/common/world/ModifiableBiomeInfo$BiomeInfo$Builder;)V";
const PHASE: &str = "net/neoforged/neoforge/common/world/BiomeModifier$Phase";
/// NeoForge's `BiomeModifier.Phase`s in ordinal order: every biome's modifiers run phase by phase.
pub const PHASES: [&str; 5] = ["BEFORE_EVERYTHING", "ADD", "REMOVE", "MODIFY", "AFTER_EVERYTHING"];
const BIOME_INFO_BUILDER: &str = "net/neoforged/neoforge/common/world/ModifiableBiomeInfo$BiomeInfo$Builder";
const GENERATION_BUILDER: &str = "net/neoforged/neoforge/common/world/BiomeGenerationSettingsBuilder";
const PLAIN_BUILDER: &str = "net/minecraft/world/level/biome/BiomeGenerationSettings$PlainBuilder";
const BIOME: &str = "net/minecraft/world/level/biome/Biome";
const PLACED: &str = "net/minecraft/world/level/levelgen/placement/PlacedFeature";
const HOLDER_SET: &str = "net/minecraft/core/HolderSet";
const HOLDER_SET_DIRECT: &str = "net/minecraft/core/HolderSet$Direct";
const HOLDER_SET_NAMED: &str = "net/minecraft/core/HolderSet$Named";
const HOLDER_SET_LIST_BACKED: &str = "net/minecraft/core/HolderSet$ListBacked";
const CONSUMER: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
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

/// Whether the mod block `state` survives at `position` on the host's region by the mod's own code
/// ([`crate::blocks::mod_survives`]): a feature asks before it places it (`simple_block`, a
/// `would_survive` predicate). None where the block isn't the mod's or its code leaves it to the
/// block's rule.
pub fn survives(rt: &mut Rt, state: &str, position: [i32; 3]) -> Option<bool> {
    let (b, class) = crate::glue::mod_block(rt, state)?;
    let level = level(rt);
    let st = crate::api::state(rt, state);
    let pos = crate::api::block_pos(rt, i64::from(position[0]), i64::from(position[1]), i64::from(position[2]));
    crate::blocks::mod_survives(rt, b, class, st, level, pos)
}

/// The generator's region, as the level a modifier's context and a feature are handed.
fn level(rt: &mut Rt) -> V {
    rt.intern("compat:worldgen/level".into(), |rt| rt.new_object(LEVEL))
}

/// The biome modifier `json` describes (the whole file, `type` included) when `kind` is one of the
/// mod's biome modifier serializers (`NeoForgeRegistries.BIOME_MODIFIER_SERIALIZERS`: Farmer's
/// Delight's `farmersdelight:add_features_by_filter`); `None` when it isn't. The serializer is
/// the map codec itself, which reads the object as NeoForge's dispatch hands it. Once per text.
pub fn biome_modifier(rt: &mut Rt, kind: &str, json: &str) -> Option<Result<V, String>> {
    let codec = MODIFIER_REGISTRIES.iter().map(|r| crate::neo::registry_get(rt, r, kind)).find(|v| v.r() != 0)?;
    let key = format!("compat:worldgen/biome_modifier/{json}");
    if let Some(&v) = rt.interned.get(&key) {
        return Some(Ok(v));
    }
    let read = (|| {
        let Json::Object(object) = serde_json::from_str::<Json>(json).map_err(|e| e.to_string())? else {
            return Err(format!("Not a map: {json}"));
        };
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

/// The mod's `modify(biome, phase, builder)` of biome modifier `modifier`, for biome `biome` in
/// phase `phase` (an index of [`PHASES`]). `features` is the biome's placed features by
/// generation step, as the builder's lists hold them (`BiomeGenerationSettingsBuilder`): an id, or
/// an inline placed feature's JSON; after the call, what the lists hold then. The host answers the
/// biome's data and the holder sets' members.
pub fn modify_biome(rt: &mut Rt, modifier: V, biome: &str, phase: usize, features: &mut Vec<Vec<String>>) -> Result<(), String> {
    let phase = PHASES.get(phase).map(|_| phase_constant(rt, phase)).ok_or_else(|| format!("no biome modifier phase {phase}"))?;
    let value = biome_value(rt, biome);
    let holder = crate::neo::registry_holder(rt, "biome", biome, value);
    // The biome's data as this call's host has it, not as an earlier one did.
    rt.put_field(holder, "$value", value);
    let mut steps = Vec::with_capacity(features.len());
    for list in features.iter() {
        let holders = list.iter().map(|entry| placed_holder(rt, entry)).collect();
        steps.push(crate::jutil::new_list(rt, holders));
    }
    let steps = crate::jutil::new_list(rt, steps);
    let generation = rt.new_object(GENERATION_BUILDER);
    rt.put_field(generation, "$features", steps);
    let builder = rt.new_object(BIOME_INFO_BUILDER);
    rt.put_field(builder, "$generation", generation);
    let Some(vcall) = rt.vcall else { return Err("no mod code to run".into()) };
    vcall(rt, MODIFY_KEY, MODIFY, &[modifier, holder, phase, builder]);
    if let Some(e) = rt.thrown.take() {
        return Err(thrown(rt, e));
    }
    let mut out = Vec::new();
    for list in crate::jutil::elements(rt, steps) {
        out.push(crate::jutil::elements(rt, list).into_iter().map(|h| placed_entry(rt, h)).collect::<Result<Vec<_>, _>>()?);
    }
    *features = out;
    Ok(())
}

/// `BiomeModifier.Phase`'s constant `PHASES[i]`.
fn phase_constant(rt: &mut Rt, i: usize) -> V {
    crate::api::api_enum(rt, PHASE, PHASES[i], i as i32)
}

/// The statics answered here: `BiomeModifier.Phase`'s constants.
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != PHASE {
        return None;
    }
    let i = PHASES.iter().position(|p| *p == field)?;
    Some(phase_constant(rt, i))
}

/// Biome `id` as `Holder<Biome>.value()` gives it: what modifiers read of it, from its data.
pub(crate) fn biome_value(rt: &mut Rt, id: &str) -> V {
    let data: Json = rt.host().and_then(|h| h.biome_json(id)).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Json::Null);
    let b = rt.new_object(BIOME);
    let name = rt.string(id);
    rt.put_field(b, "$id", name);
    // `Biome.ClimateSettings`: no temperature or downfall is 0.5 in a biome's codec only where
    // it says so; a biome's file always has them.
    let number = |key: &str| data.get(key).and_then(Json::as_f64).map_or(0.0, |v| v as f32);
    rt.put_field(b, "$temperature", V::F(number("temperature")));
    rt.put_field(b, "$downfall", V::F(number("downfall")));
    let rains = data.get("has_precipitation").and_then(Json::as_bool).unwrap_or(false);
    rt.put_field(b, "$precipitation", V::I(rains as i32));
    b
}

/// A placed feature as `PlacedFeature.CODEC` reads it, kept as its text: `place` hands the text
/// to the host's world generator, which reads it with vanilla's codecs. One per text.
fn placed(rt: &mut Rt, json: &str) -> V {
    let text = json.to_owned();
    rt.intern(format!("compat:worldgen/placed/{json}"), move |rt| {
        let p = rt.new_object(PLACED);
        let s = rt.string(&text);
        rt.put_field(p, "$json", s);
        p
    })
}

/// The holder of a placed feature an entry of a biome's lists names: the registry's holder for an
/// id, a direct holder for an inline feature (its JSON).
fn placed_holder(rt: &mut Rt, entry: &str) -> V {
    if entry.trim_start().starts_with('{') {
        let value = placed(rt, entry);
        let h = rt.new_object("net/minecraft/core/Holder$Direct");
        rt.put_field(h, "value", value);
        return h;
    }
    let id = if entry.contains(':') { entry.to_owned() } else { format!("minecraft:{entry}") };
    let value = placed(rt, &Json::String(id.clone()).to_string());
    crate::neo::registry_holder(rt, "placed_feature", &id, value)
}

/// The entry of a biome's lists a placed feature holder is: its id, or its inline JSON.
fn placed_entry(rt: &mut Rt, holder: V) -> Result<String, String> {
    if let Some(id) = rt.str_of(rt.get_field(holder, "$id")).filter(|id| !id.is_empty()) {
        return Ok(id.to_owned());
    }
    let value = if rt.class_of(holder) == "net/minecraft/core/Holder$Direct" { rt.get_field(holder, "value") } else { V::NULL };
    let json = rt.str_of(rt.get_field(value, "$json")).map(str::to_owned);
    match json.as_deref().map(|j| (j, serde_json::from_str::<Json>(j))) {
        Some((_, Ok(Json::String(id)))) => Ok(id),
        Some((j, Ok(_))) => Ok(j.to_owned()),
        _ => Err(format!("a placed feature holder this runtime didn't make ({})", rt.class_of(holder))),
    }
}

/// A holder set as `RegistryCodecs.homogeneousList(registry)` reads it (`Biome.LIST_CODEC`,
/// `PlacedFeature.LIST_CODEC`): an id, `#tag`, a list of ids, or one of NeoForge's holder set
/// objects (`neoforge:any`, `or`, `and`, `not`), kept as written; the host answers its members.
fn holder_set(rt: &mut Rt, registry: &str, j: &Json) -> Result<V, String> {
    let named = j.as_str().is_some_and(|s| s.starts_with('#'));
    let valid = match j {
        Json::String(_) => true,
        Json::Array(items) => items.iter().all(|v| v.as_str().is_some_and(|s| !s.starts_with('#'))),
        Json::Object(o) => o.get("type").is_some_and(Json::is_string),
        _ => false,
    };
    if !valid {
        return Err(format!("Not a {registry} id, tag or list: {j}"));
    }
    let set = rt.new_object(if named { HOLDER_SET_NAMED } else { HOLDER_SET_DIRECT });
    let (r, text) = (rt.string(registry), rt.string(&j.to_string()));
    rt.put_field(set, "$registry", r);
    rt.put_field(set, "$set", text);
    Ok(set)
}

/// The holders of holder set `set`: those it was made of (`HolderSet.direct`), or the members the
/// host's registries give the set's JSON, in order. A list of ids reads as written where the host
/// keeps no registries; a tag has no members there.
fn set_holders(rt: &mut Rt, set: V) -> Vec<V> {
    let made = rt.get_field(set, "$holders");
    if made.r() != 0 {
        return crate::jutil::elements(rt, made);
    }
    let registry = rt.str_of(rt.get_field(set, "$registry")).unwrap_or("").to_owned();
    let text = rt.str_of(rt.get_field(set, "$set")).unwrap_or("").to_owned();
    let ids = rt.host().and_then(|h| h.holder_set(&registry, &text)).unwrap_or_else(|| match serde_json::from_str::<Json>(&text) {
        Ok(Json::String(id)) if !id.starts_with('#') => vec![id],
        Ok(Json::Array(items)) => items.iter().filter_map(Json::as_str).map(str::to_owned).collect(),
        _ => Vec::new(),
    });
    ids.iter()
        .map(|id| {
            let id = if id.contains(':') { id.clone() } else { format!("minecraft:{id}") };
            let value = if registry == "placed_feature" { placed(rt, &Json::String(id.clone()).to_string()) } else { V::NULL };
            crate::neo::registry_holder(rt, &registry, &id, value)
        })
        .collect()
}

/// Whether holder `holder` is one of `set`'s: the same holder, or a registry holder of the same
/// entry.
fn set_contains(rt: &mut Rt, set: V, holder: V) -> bool {
    let id = rt.str_of(rt.get_field(holder, "$id")).map(str::to_owned);
    set_holders(rt, set).into_iter().any(|h| h == holder || id.as_ref().is_some_and(|id| rt.str_of(rt.get_field(h, "$id")) == Some(id.as_str())))
}

/// `HolderSet`'s methods on the sets made here, `Biome`'s on biomes, `PlacedFeature.place`, and the
/// builder a biome modifier edits.
fn holder_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let set_made_here = |rt: &Rt| matches!(rt.class_of(a(0)), HOLDER_SET_DIRECT | HOLDER_SET_NAMED);
    Some(match (owner, name) {
        // `HolderSet.direct(holders...)`, `direct(list)`, `empty()`: a set of those holders.
        (HOLDER_SET, "direct" | "empty") if !desc.starts_with("(Ljava/util/function/Function;") => {
            let holders = if name == "empty" { Vec::new() } else { crate::jutil::elements(rt, a(0)) };
            let set = rt.new_object(HOLDER_SET_DIRECT);
            let list = crate::jutil::new_list(rt, holders);
            rt.put_field(set, "$holders", list);
            set
        }
        (HOLDER_SET | HOLDER_SET_DIRECT | HOLDER_SET_NAMED | HOLDER_SET_LIST_BACKED, _) if set_made_here(rt) => match name {
            "contains" => V::I(set_contains(rt, a(0), a(1)) as i32),
            "size" => V::I(set_holders(rt, a(0)).len() as i32),
            "get" => set_holders(rt, a(0)).get(a(1).i().max(0) as usize).copied().unwrap_or(V::NULL),
            "stream" => {
                let items = set_holders(rt, a(0));
                rt.alloc("java/util/stream/Stream", crate::Data::Stream(items))
            }
            "iterator" => {
                let items = set_holders(rt, a(0));
                crate::jutil::new_iter(rt, items)
            }
            "forEach" => {
                for h in set_holders(rt, a(0)) {
                    rt.call_functional(CONSUMER, a(1), &[h]);
                    if rt.thrown.is_some() {
                        break;
                    }
                }
                V::Void
            }
            // `getRandomElement(random)`: one of them, drawn as `Util.getRandomSafe` draws.
            "getRandomElement" => {
                let items = set_holders(rt, a(0));
                let o = rt.new_object("java/util/Optional");
                if !items.is_empty() {
                    let i = crate::api::call(rt, NEXT_INT_BOUND, &[a(1), V::I(items.len() as i32)]).i();
                    rt.put_field(o, "value", items.get(i.max(0) as usize).copied().unwrap_or(V::NULL));
                }
                o
            }
            "isBound" => V::I(1),
            "unwrapKey" => {
                let o = rt.new_object("java/util/Optional");
                if rt.class_of(a(0)) == HOLDER_SET_NAMED {
                    let registry = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("").to_owned();
                    let text = rt.str_of(rt.get_field(a(0), "$set")).unwrap_or("").to_owned();
                    let tag = serde_json::from_str::<Json>(&text).ok().and_then(|j| j.as_str().map(|s| s.trim_start_matches('#').to_owned())).unwrap_or_default();
                    let key = crate::fluids::tag_key(rt, &registry, &tag);
                    rt.put_field(o, "value", key);
                }
                o
            }
            _ => return None,
        },
        // A biome: its climate, from its data.
        (BIOME, "getBaseTemperature") => rt.get_field(a(0), "$temperature"),
        (BIOME, "hasPrecipitation") => rt.get_field(a(0), "$precipitation"),
        // `PlacedFeature.place(level, generator, random, pos)` from the mod's code: the host's
        // generator places it on its region, drawing from the feature's stream.
        (PLACED, "place" | "placeWithBiomeCheck") if rt.get_field(a(0), "$json").r() != 0 => {
            let json = rt.str_of(rt.get_field(a(0), "$json")).unwrap_or("").to_owned();
            let pos = crate::api::pos_of(rt, a(4));
            match rt.host_call(|h| h.place_feature(&json, pos)) {
                Some(Ok(placed)) => V::I(placed as i32),
                Some(Err(e)) => {
                    let ex = rt.new_object("java/lang/IllegalStateException");
                    let m = rt.string(&format!("placed feature {json}: {e}"));
                    rt.put_field(ex, "message", m);
                    rt.thrown = Some(ex);
                    V::I(0)
                }
                None => V::I(0),
            }
        }
        // The builder a biome modifier edits: the biome's generation settings, each step's list of
        // placed feature holders, which grow to the step added to (`addFeatureStepsUpTo`).
        (BIOME_INFO_BUILDER, "getGenerationSettings") => rt.get_field(a(0), "$generation"),
        (GENERATION_BUILDER | PLAIN_BUILDER, "addFeature" | "getFeatures") if rt.get_field(a(0), "$features").r() != 0 => {
            let step = if desc.starts_with("(I") { a(1).i() } else { rt.get_field(a(1), "$ordinal").i() };
            let steps = rt.get_field(a(0), "$features");
            let mut lists = crate::jutil::elements(rt, steps);
            while lists.len() <= step.max(0) as usize {
                let l = crate::jutil::new_list(rt, Vec::new());
                crate::jutil::list_add(rt, steps, l);
                lists.push(l);
            }
            let list = lists[step.max(0) as usize];
            if name == "getFeatures" {
                list
            } else {
                crate::jutil::list_add(rt, list, a(2));
                a(0)
            }
        }
        _ => return None,
    })
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
            | HOLDER_SET
            | HOLDER_SET_DIRECT
            | HOLDER_SET_NAMED
            | HOLDER_SET_LIST_BACKED
            | BIOME
            | PLACED
            | BIOME_INFO_BUILDER
            | GENERATION_BUILDER
            | PLAIN_BUILDER
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
    if matches!(owner, HOLDER_SET | HOLDER_SET_DIRECT | HOLDER_SET_NAMED | HOLDER_SET_LIST_BACKED | BIOME | PLACED | BIOME_INFO_BUILDER | GENERATION_BUILDER | PLAIN_BUILDER) {
        return holder_call(rt, owner, name, desc, args);
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
            rt.host_call(|h| h.set_section_state(p, &text));
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
    // Holder sets of a registry (`Biome.LIST_CODEC`, `PlacedFeature.LIST_CODEC`), kept as written.
    if let Some(registry) = name.strip_prefix("holder_set/") {
        return holder_set(rt, registry, j);
    }
    match (name, j) {
        // `PlacedFeature.CODEC`: an id (the registry's holder) or an inline placed feature (a
        // direct holder); `DIRECT_CODEC` the inline one itself.
        ("placed_feature", Json::String(id)) => return Ok(placed_holder(rt, id)),
        ("placed_feature", Json::Object(_)) => return Ok(placed_holder(rt, &j.to_string())),
        ("placed_feature_direct", Json::Object(_)) => return Ok(placed(rt, &j.to_string())),
        ("placed_feature" | "placed_feature_direct", _) => return Err(format!("Not a placed feature: {j}")),
        _ => {}
    }
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
        // `RandomPatchConfiguration.CODEC` (1.21.1's; 26.3 has no random patch), for a mod's feature
        // made with it (Farmer's Delight's wild rice). A missing or bad count reads as its default,
        // as `orElse` reads it; the placed feature is required.
        "random_patch_configuration" => {
            let count = |key: &str, min: i64, default: i32| {
                o.get(key).and_then(Json::as_i64).filter(|n| (min..=i64::from(i32::MAX)).contains(n)).map_or(default, |n| n as i32)
            };
            let (tries, xz, y) = (count("tries", 1, 128), count("xz_spread", 0, 7), count("y_spread", 0, 3));
            let feature = decode_native(rt, "placed_feature", field("feature")?)?;
            Ok(crate::records::make(rt, RANDOM_PATCH, &[V::I(tries), V::I(xz), V::I(y), feature]))
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
            APPLY_SHORT if rt.class_of(args[0]) == "test/WarmCodec" => warm_codec(rt, args[1]),
            FUNCTION4_SHORT if rt.class_of(args[0]) == "test/MakeWarm" => {
                let warm = rt.new_object("test/Warm");
                for (field, v) in ["allowed", "denied", "features", "step"].into_iter().zip(&args[1..]) {
                    rt.put_field(warm, field, *v);
                }
                warm
            }
            MODIFY if rt.class_of(args[0]) == "test/Warm" => warm_modify(rt, args[0], args[1], args[2], args[3]),
            "accept.(Ljava/lang/Object;)V" if rt.class_of(args[0]) == "test/AddTo" => {
                let (generation, step) = (rt.get_field(args[0], "generation"), rt.get_field(args[0], "step"));
                crate::api::call(rt, "net/neoforged/neoforge/common/world/BiomeGenerationSettingsBuilder.addFeature.(Lnet/minecraft/world/level/levelgen/GenerationStep$Decoration;Lnet/minecraft/core/Holder;)Lnet/minecraft/world/level/biome/BiomeGenerationSettings$PlainBuilder;", &[generation, step, args[1]]);
                V::Void
            }
            "test.(Ljava/lang/Object;)Z" if rt.class_of(args[0]) == "test/IsGone" => {
                let name = crate::api::call(rt, "net/minecraft/core/Holder.getRegisteredName.()Ljava/lang/String;", &[args[1]]);
                V::I((rt.str_of(name) == Some("test:gone")) as i32)
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

    /// `RandomPatchConfiguration.CODEC` as vanilla 1.21.1's reads it: its counts, each its default
    /// (128, 7, 3) when missing or out of range (`orElse`), and the placed feature, which it
    /// needs; the record's accessors read them back.
    #[test]
    fn random_patch_configurations_read_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let codec = crate::api::get_static(&mut rt, "net/minecraft/world/level/levelgen/feature/configurations/RandomPatchConfiguration.CODEC");
        let get = |rt: &mut Rt, c: V, accessor: &'static str| crate::api::call(rt, accessor, &[c]);
        const TRIES: &str = "net/minecraft/world/level/levelgen/feature/configurations/RandomPatchConfiguration.tries.()I";
        const XZ: &str = "net/minecraft/world/level/levelgen/feature/configurations/RandomPatchConfiguration.xzSpread.()I";
        const Y: &str = "net/minecraft/world/level/levelgen/feature/configurations/RandomPatchConfiguration.ySpread.()I";
        const FEATURE: &str = "net/minecraft/world/level/levelgen/feature/configurations/RandomPatchConfiguration.feature.()Lnet/minecraft/core/Holder;";
        let inline = serde_json::json!({"feature": {"type": "minecraft:simple_block", "config": {}}, "placement": []});
        let read = crate::codecs::decode(&mut rt, codec, &serde_json::json!({"tries": 96, "xz_spread": 6, "y_spread": 2, "feature": inline})).unwrap();
        assert_eq!([TRIES, XZ, Y].map(|a| get(&mut rt, read, a)), [V::I(96), V::I(6), V::I(2)]);
        let feature = get(&mut rt, read, FEATURE);
        assert_eq!(placed_entry(&mut rt, feature), Ok(inline.to_string()));
        let read = crate::codecs::decode(&mut rt, codec, &serde_json::json!({"tries": 0, "y_spread": -1, "feature": "minecraft:patch_grass"})).unwrap();
        assert_eq!([TRIES, XZ, Y].map(|a| get(&mut rt, read, a)), [V::I(128), V::I(7), V::I(3)]);
        let feature = get(&mut rt, read, FEATURE);
        assert_eq!(placed_entry(&mut rt, feature).as_deref(), Ok("minecraft:patch_grass"));
        assert!(crate::codecs::decode(&mut rt, codec, &serde_json::json!({"tries": 4})).is_err(), "the feature is required");
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

    const FUNCTION4_SHORT: &str = "apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";

    /// `test/Warm`'s codec, as Farmer's Delight writes its biome modifier's:
    /// `RecordCodecBuilder.mapCodec(i -> i.group(Biome.LIST_CODEC.fieldOf("allowed_biomes"),
    /// Biome.LIST_CODEC.optionalFieldOf("denied_biomes"), PlacedFeature.LIST_CODEC.fieldOf("features"),
    /// Decoration.CODEC.fieldOf("step")).apply(i, Warm::new))`.
    fn warm_codec(rt: &mut Rt, instance: V) -> V {
        let call = crate::api::call;
        let biomes = crate::api::get_static(rt, "net/minecraft/world/level/biome/Biome.LIST_CODEC");
        let features = crate::api::get_static(rt, "net/minecraft/world/level/levelgen/placement/PlacedFeature.LIST_CODEC");
        let steps = crate::api::get_static(rt, "net/minecraft/world/level/levelgen/GenerationStep$Decoration.CODEC");
        let getter = rt.new_object("test/Unused");
        let mut fields = Vec::new();
        for (codec, name, optional) in [(biomes, "allowed_biomes", false), (biomes, "denied_biomes", true), (features, "features", false), (steps, "step", false)] {
            let n = rt.string(name);
            let field = if optional {
                call(rt, "com/mojang/serialization/Codec.optionalFieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[codec, n])
            } else {
                call(rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[codec, n])
            };
            fields.push(call(rt, "com/mojang/serialization/MapCodec.forGetter.(Ljava/util/function/Function;)Lcom/mojang/serialization/codecs/RecordCodecBuilder;", &[field, getter]));
        }
        let app = "Lcom/mojang/datafixers/kinds/App;";
        let group: &'static str = Box::leak(format!("com/mojang/serialization/codecs/RecordCodecBuilder$Instance.group.({})Lcom/mojang/datafixers/Products$P4;", app.repeat(4)).into_boxed_str());
        let product = call(rt, group, &[instance, fields[0], fields[1], fields[2], fields[3]]);
        let make = rt.new_object("test/MakeWarm");
        call(rt, "com/mojang/datafixers/Products$P4.apply.(Lcom/mojang/datafixers/kinds/Applicative;Lcom/mojang/datafixers/util/Function4;)Lcom/mojang/datafixers/kinds/App;", &[product, instance, make])
    }

    /// `test/Warm.modify(biome, phase, builder)`: in the add phase, a biome of its allowed set and
    /// not of its denied one (when it has one), warmer than 0.5, gets its features at its step; in
    /// the remove phase, every biome loses `test:gone` from that step.
    fn warm_modify(rt: &mut Rt, warm: V, biome: V, phase: V, builder: V) -> V {
        let call = crate::api::call;
        let add = crate::api::get_static(rt, "net/neoforged/neoforge/common/world/BiomeModifier$Phase.ADD");
        let remove = crate::api::get_static(rt, "net/neoforged/neoforge/common/world/BiomeModifier$Phase.REMOVE");
        let step = rt.get_field(warm, "step");
        let generation = call(rt, "net/neoforged/neoforge/common/world/ModifiableBiomeInfo$BiomeInfo$Builder.getGenerationSettings.()Lnet/neoforged/neoforge/common/world/BiomeGenerationSettingsBuilder;", &[builder]);
        const CONTAINS: &str = "net/minecraft/core/HolderSet.contains.(Lnet/minecraft/core/Holder;)Z";
        if phase == add {
            if call(rt, CONTAINS, &[rt.get_field(warm, "allowed"), biome]).i() == 0 {
                return V::Void;
            }
            let denied = rt.get_field(warm, "denied");
            if call(rt, "java/util/Optional.isPresent.()Z", &[denied]).i() != 0 {
                let set = call(rt, "java/util/Optional.get.()Ljava/lang/Object;", &[denied]);
                if call(rt, CONTAINS, &[set, biome]).i() != 0 {
                    return V::Void;
                }
            }
            let value = call(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[biome]);
            if call(rt, "net/minecraft/world/level/biome/Biome.getBaseTemperature.()F", &[value]).f() <= 0.5 {
                return V::Void;
            }
            let add_to = rt.new_object("test/AddTo");
            rt.put_field(add_to, "generation", generation);
            rt.put_field(add_to, "step", step);
            call(rt, "net/minecraft/core/HolderSet.forEach.(Ljava/util/function/Consumer;)V", &[rt.get_field(warm, "features"), add_to]);
        } else if phase == remove {
            let list = call(rt, "net/neoforged/neoforge/common/world/BiomeGenerationSettingsBuilder.getFeatures.(Lnet/minecraft/world/level/levelgen/GenerationStep$Decoration;)Ljava/util/List;", &[generation, step]);
            let gone = rt.new_object("test/IsGone");
            call(rt, "java/util/List.removeIf.(Ljava/util/function/Predicate;)Z", &[list, gone]);
        }
        V::Void
    }

    /// The content root's biomes as a biome modifier's host sees them: `test:warm` holds the dunes
    /// and the jungle, `test:wet` the jungle; the dunes are hot, the jungle warm, the tundra cold.
    struct Meadows;
    impl Host for Meadows {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn holder_set(&self, registry: &str, set: &str) -> Option<Vec<String>> {
            let ids = |s: &[&str]| s.iter().map(|id| id.to_string()).collect();
            Some(match (registry, serde_json::from_str::<Json>(set).ok()?) {
                ("biome", Json::String(s)) if s == "#test:warm" => ids(&["test:dunes", "test:jungle"]),
                ("biome", Json::String(s)) if s == "#test:wet" => ids(&["test:jungle"]),
                (_, Json::String(s)) if !s.starts_with('#') => vec![s],
                (_, Json::Array(items)) => items.iter().filter_map(Json::as_str).map(str::to_owned).collect(),
                _ => return None,
            })
        }
        fn biome_json(&self, id: &str) -> Option<String> {
            let temperature = match id {
                "test:dunes" => 2.0,
                "test:jungle" => 0.95,
                _ => 0.0,
            };
            Some(format!(r#"{{"temperature": {temperature}, "downfall": 0.5, "has_precipitation": true}}"#))
        }
    }

    /// A mod's biome modifier type (Farmer's Delight's `add_features_by_filter`): its serializer, a
    /// record codec over holder sets of biomes and placed features, reads the file; run on each
    /// biome phase by phase, the mod's code adds its features where its sets and the biome's
    /// temperature say, through the builder's lists, and takes features out of them in its remove
    /// phase. What the lists hold afterwards is the biome's new features, steps it added included.
    #[test]
    fn a_mods_biome_modifier_edits_the_biomes_its_code_picks() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(mod_code);
        let builder = rt.new_object("test/WarmCodec");
        let codec = crate::api::call(&mut rt, "com/mojang/serialization/codecs/RecordCodecBuilder.mapCodec.(Ljava/util/function/Function;)Lcom/mojang/serialization/MapCodec;", &[builder]);
        rt.registered.insert(("biome_modifier_serializers".into(), "test:warm_features".into()), codec);
        let json = r##"{"type": "test:warm_features", "allowed_biomes": "#test:warm", "denied_biomes": "#test:wet",
            "features": ["test:cactus_patch", "minecraft:patch_dead_bush"], "step": "vegetal_decoration"}"##;
        assert!(biome_modifier(&mut rt, "test:other", json).is_none());
        let modifier = biome_modifier(&mut rt, "test:warm_features", json).unwrap().unwrap();
        let mut host = Meadows;
        let mut run = |rt: &mut Rt, biome: &str, mut features: Vec<Vec<String>>| {
            rt.with_host(&mut host, |rt| {
                for phase in 0..PHASES.len() {
                    modify_biome(rt, modifier, biome, phase, &mut features).unwrap();
                }
            });
            features
        };
        let v = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // The dunes: allowed, not wet, hot. The step's list is made (the file named three steps).
        let dunes = run(&mut rt, "test:dunes", vec![v(&["minecraft:lake"]), v(&[]), v(&["test:gone", "minecraft:ore"])]);
        let mut expected = vec![v(&["minecraft:lake"]), v(&[]), v(&["test:gone", "minecraft:ore"])];
        expected.resize(10, Vec::new());
        expected[9] = v(&["test:cactus_patch", "minecraft:patch_dead_bush"]);
        assert_eq!(dunes, expected);
        // The jungle is denied; the tundra isn't allowed. Both keep their features, except what
        // the remove phase takes from the step.
        let jungle = run(&mut rt, "test:jungle", vec![Vec::new(); 11]);
        assert_eq!(jungle, vec![Vec::<String>::new(); 11]);
        let mut tundra_features = vec![Vec::new(); 11];
        tundra_features[9] = v(&["test:gone", "minecraft:patch_grass", "test:gone"]);
        let tundra = run(&mut rt, "test:tundra", tundra_features);
        assert_eq!(tundra[9], v(&["minecraft:patch_grass"]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // A file the codec refuses is the codec's error.
        let bad = r#"{"type": "test:warm_features", "allowed_biomes": 3, "features": [], "step": "vegetal_decoration"}"#;
        assert!(biome_modifier(&mut rt, "test:warm_features", bad).unwrap().is_err());
    }
}
