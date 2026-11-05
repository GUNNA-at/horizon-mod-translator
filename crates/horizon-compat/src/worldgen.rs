//! A mod's own worldgen code, run by Horizon's world generator: NeoForge placement modifier types
//! a mod registers (`Registries.PLACEMENT_MODIFIER_TYPE`: Create's `create:config_filter`). The
//! generator hands the modifier's JSON to [`placement`], which reads it with the type's own codec,
//! and asks [`positions`] for what it makes of a position: the mod's `getPositions`, or vanilla's
//! `PlacementFilter` and `RepeatingPlacement` around the mod's `shouldPlace` and `count`. The
//! world they read is the host's (the generator's region), and the `RandomSource` they get draws
//! from the host's random (`Host::random`): the feature's own stream, as vanilla hands it.
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
        let level = rt.new_object(LEVEL);
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

/// The keys [`call`] answers: vanilla's placement classes and the worldgen random's draws.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, MODIFIER | FILTER | REPEATING | CONTEXT | RANDOM_SOURCE | WORLDGEN_RANDOM)
}

pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if matches!(owner, RANDOM_SOURCE | WORLDGEN_RANDOM) {
        return random_call(rt, name, desc, args);
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
            _ => crate::api::call(rt, owner_key, args),
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
