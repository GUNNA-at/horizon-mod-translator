//! Worldgen types a mod defines in code rather than as data: a NeoForge mod's own placement
//! modifiers (Create's `create:config_filter`, Mekanism's `mekanism:disableable`). A translated
//! mod registers a [`Handler`]; the placement decoder asks the handlers for a type vanilla doesn't
//! have, and the generator runs what one returns on the world and random stream vanilla's own
//! modifiers get, so the mod's code draws from the feature's random as it would in Java.
use super::placement::{PlacementContext, PlacementQueryError};
use crate::heightmap::HeightmapType;
use crate::registry::{BlockRegistry, BlockStateId};
use horizon_core::random::RandomSource;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// The world a mod's code reads while the generator runs it: `PlacementContext`'s level.
/// Each answer is `None` where the generator's world can't give it.
pub trait World {
    /// The block state at `position` as text: `minecraft:stone`, `create:shaft[axis=y]`
    /// (properties sorted by name).
    fn block(&mut self, position: [i32; 3]) -> Option<String>;
    /// Whether the block at `position` is in block tag `tag` (`minecraft:base_stone_overworld`,
    /// no `#`).
    fn block_in_tag(&mut self, position: [i32; 3], tag: &str) -> bool;
    /// `getHeight(heightmap, x, z)`, the heightmap by vanilla's name (`WORLD_SURFACE_WG`).
    fn height(&mut self, heightmap: &str, x: i32, z: i32) -> Option<i32>;
    /// The lowest y and the highest one.
    fn y_range(&mut self) -> Option<(i32, i32)>;
    /// `WorldGenLevel.getSeed()`.
    fn seed(&mut self) -> Option<i64>;
    /// The world's sea level.
    fn sea_level(&mut self) -> Option<i32>;
    /// The biome at `position` (`minecraft:plains`).
    fn biome(&mut self, position: [i32; 3]) -> Option<String>;
    /// Whether biome `biome` is in biome tag `tag` (`minecraft:is_overworld`, no `#`), as the
    /// generator's tags hold it (vanilla's and the packs').
    fn biome_in_tag(&mut self, biome: &str, tag: &str) -> bool;
}

/// The random stream the generator hands a modifier (the feature's), as `RandomSource` draws.
pub trait Random {
    fn next_int(&mut self) -> i32;
    /// `nextInt(bound)`; 0 for a bound below 1 (vanilla throws).
    fn next_int_bound(&mut self, bound: i32) -> i32;
    fn next_long(&mut self) -> i64;
    fn next_boolean(&mut self) -> bool;
    fn next_float(&mut self) -> f32;
    fn next_double(&mut self) -> f64;
    fn next_gaussian(&mut self) -> f64;
}

/// A mod's placement modifier, read from its JSON: the positions it makes of one, as
/// `PlacementModifier.getPositions` streams them (a filter gives the position or none).
pub trait Placement: Send + Sync {
    fn positions(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<Vec<[i32; 3]>, String>;
}

/// What a mod registers: its own worldgen types, by id.
pub trait Handler: Send + Sync {
    /// The modifier `json` (the whole object, `type` included) describes when `kind` is one of
    /// the mod's placement modifier types; `None` when it isn't the mod's.
    fn placement(&self, kind: &str, json: &str) -> Option<Result<Arc<dyn Placement>, String>>;
}

static HANDLERS: RwLock<Vec<(String, Arc<dyn Handler>)>> = RwLock::new(Vec::new());
static BLOCKS: RwLock<Option<&'static BlockRegistry>> = RwLock::new(None);
static BIOMES: RwLock<Option<Arc<Biomes>>> = RwLock::new(None);

/// The generator's biomes: names by id, and the biome tags by name with the biomes in each.
struct Biomes {
    names: Vec<Arc<str>>,
    tags: BTreeMap<String, Vec<String>>,
}

/// Registers mod `id`'s handler, replacing one it registered before (a resource reload asks again).
pub fn register(id: &str, handler: Arc<dyn Handler>) {
    let mut handlers = HANDLERS.write().unwrap_or_else(|e| e.into_inner());
    handlers.retain(|(other, _)| other != id);
    handlers.push((id.to_owned(), handler));
}

/// The block registry states are named from for mods' code: the generator's.
pub fn set_blocks(registry: &'static BlockRegistry) {
    *BLOCKS.write().unwrap_or_else(|e| e.into_inner()) = Some(registry);
}

/// The biomes mods' code names (the generator's, by [`super::biome::BiomeId`]) and the biome
/// tags holding them, by tag name (`minecraft:is_overworld`).
pub fn set_biomes(names: Vec<Arc<str>>, tags: BTreeMap<String, Vec<String>>) {
    *BIOMES.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Biomes { names, tags }));
}

/// The first registered handler that knows placement modifier type `kind`.
pub(crate) fn placement(kind: &str, json: &str) -> Option<Result<Arc<dyn Placement>, String>> {
    let handlers: Vec<Arc<dyn Handler>> = HANDLERS.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, h)| h.clone()).collect();
    handlers.iter().find_map(|h| h.placement(kind, json))
}

/// A state as text, as [`World::block`] gives it.
pub fn state_text(registry: &BlockRegistry, id: BlockStateId) -> Option<String> {
    let state = registry.state(id)?;
    if state.properties.is_empty() {
        return Some(state.name.to_string());
    }
    let props: Vec<String> = state.properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
    Some(format!("{}[{}]", state.name, props.join(",")))
}

/// A mod's modifier in a placement chain: its type and what its handler made of it.
#[derive(Clone)]
pub struct ForeignPlacement {
    pub kind: String,
    pub(crate) inner: Arc<dyn Placement>,
}

impl std::fmt::Debug for ForeignPlacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ForeignPlacement({})", self.kind)
    }
}

impl ForeignPlacement {
    /// The positions the mod's modifier makes of `origin` on `context`'s world.
    pub(crate) fn positions<C: PlacementContext + ?Sized, R: RandomSource>(
        &self,
        context: &mut C,
        random: &mut R,
        origin: [i32; 3],
    ) -> Result<Vec<[i32; 3]>, PlacementQueryError> {
        let registry = *BLOCKS.read().unwrap_or_else(|e| e.into_inner());
        let biomes = BIOMES.read().unwrap_or_else(|e| e.into_inner()).clone();
        self.inner
            .positions(&mut ContextWorld { context, registry, biomes }, &mut RandomBridge(random), origin)
            .map_err(|e| PlacementQueryError::Failed(format!("{}: {e}", self.kind)))
    }
}

/// A placement context as the [`World`] a mod's code reads.
struct ContextWorld<'a, C: PlacementContext + ?Sized> {
    context: &'a mut C,
    registry: Option<&'static BlockRegistry>,
    biomes: Option<Arc<Biomes>>,
}

impl<C: PlacementContext + ?Sized> World for ContextWorld<'_, C> {
    fn block(&mut self, position: [i32; 3]) -> Option<String> {
        let id = self.context.block_state(position).ok()?;
        state_text(self.registry?, id)
    }
    fn block_in_tag(&mut self, position: [i32; 3], tag: &str) -> bool {
        self.context.block_matches_tag(position, tag).unwrap_or(false)
    }
    fn height(&mut self, heightmap: &str, x: i32, z: i32) -> Option<i32> {
        let kind = HeightmapType::ALL.into_iter().find(|k| k.name() == heightmap)?;
        self.context.height(kind, x, z).ok()
    }
    fn y_range(&mut self) -> Option<(i32, i32)> {
        Some((self.context.min_y().ok()?, self.context.max_y().ok()?))
    }
    fn seed(&mut self) -> Option<i64> {
        self.context.world_seed().ok()
    }
    fn sea_level(&mut self) -> Option<i32> {
        self.context.sea_level().ok()
    }
    fn biome(&mut self, position: [i32; 3]) -> Option<String> {
        let id = self.context.biome_identity(position).ok()?;
        self.biomes.as_ref()?.names.get(id.0 as usize).map(|name| name.to_string())
    }
    fn biome_in_tag(&mut self, biome: &str, tag: &str) -> bool {
        let tag = if tag.contains(':') { tag.to_owned() } else { format!("minecraft:{tag}") };
        let biomes = self.biomes.as_ref();
        biomes.and_then(|b| b.tags.get(&tag)).is_some_and(|members| members.iter().any(|m| m == biome))
    }
}

/// The generator's random stream as the [`Random`] a mod draws from.
struct RandomBridge<'a, R: RandomSource>(&'a mut R);

impl<R: RandomSource> Random for RandomBridge<'_, R> {
    fn next_int(&mut self) -> i32 {
        self.0.next_int()
    }
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.0.next_int_bound(bound).unwrap_or(0)
    }
    fn next_long(&mut self) -> i64 {
        self.0.next_long()
    }
    fn next_boolean(&mut self) -> bool {
        self.0.next_boolean()
    }
    fn next_float(&mut self) -> f32 {
        self.0.next_float()
    }
    fn next_double(&mut self) -> f64 {
        self.0.next_double()
    }
    fn next_gaussian(&mut self) -> f64 {
        self.0.next_gaussian()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{DecodeError, anchor::WorldGenerationContext, placement::PlacementModifier};
    use horizon_core::random::LegacyRandom;

    /// A mod's modifier: the origin, then `step` blocks above it when a draw of four comes up 0,
    /// as a mod's `getPositions` would stream them.
    struct Twice(i32);
    impl Placement for Twice {
        fn positions(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<Vec<[i32; 3]>, String> {
            let (low, high) = world.y_range().ok_or("no world height")?;
            let mut out = vec![origin];
            let above = origin[1] + self.0;
            if random.next_int_bound(4) == 0 && (low..=high).contains(&above) {
                out.push([origin[0], above, origin[2]]);
            }
            Ok(out)
        }
    }
    struct TwiceHandler;
    impl Handler for TwiceHandler {
        fn placement(&self, kind: &str, json: &str) -> Option<Result<Arc<dyn Placement>, String>> {
            (kind == "foreign_test:twice").then(|| {
                let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
                let step = v["step"].as_i64().ok_or("No key step in MapLike")?;
                Ok(Arc::new(Twice(step as i32)) as Arc<dyn Placement>)
            })
        }
    }

    /// A modifier type a mod registered decodes through its handler and draws from the feature's
    /// random stream, in the chain's order; one nobody registered is still refused, and the
    /// handler's own reading error is the decoder's.
    #[test]
    fn a_mods_modifier_places_through_its_handler_with_the_features_random() {
        register("foreign_test", Arc::new(TwiceHandler));
        let modifier = PlacementModifier::from_json(r#"{"type": "foreign_test:twice", "step": 3}"#).unwrap();
        assert!(matches!(&modifier, PlacementModifier::Foreign(f) if f.kind == "foreign_test:twice"));
        let context = WorldGenerationContext::new(-64, 384, 63);
        let mut placed_both = false;
        // Spread seeds: Java's `Random` gives nearly the same first draw for seeds close together.
        for seed in (0..32i64).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)) {
            let mut random = LegacyRandom::new(seed);
            let mut out = Vec::new();
            modifier
                .modify(context, &mut random, [5, 70, -9], |_, p| {
                    out.push(p);
                    Ok::<(), ()>(())
                })
                .unwrap();
            // The same stream, drawn by hand: one draw of four, then the stream goes on from there.
            let mut expected_random = LegacyRandom::new(seed);
            let both = expected_random.next_int_bound(4).unwrap() == 0;
            placed_both |= both;
            let expected = if both { vec![[5, 70, -9], [5, 73, -9]] } else { vec![[5, 70, -9]] };
            assert_eq!(out, expected, "seed {seed}");
            assert_eq!(random.next_long(), expected_random.next_long(), "seed {seed}");
        }
        assert!(placed_both);
        assert!(matches!(
            PlacementModifier::from_json(r#"{"type": "foreign_test:other"}"#),
            Err(DecodeError::Field("placement modifier type"))
        ));
        match PlacementModifier::from_json(r#"{"type": "foreign_test:twice"}"#) {
            Err(DecodeError::Registry(e)) => assert!(e.contains("No key step"), "{e}"),
            other => panic!("{other:?}"),
        }
    }
}
