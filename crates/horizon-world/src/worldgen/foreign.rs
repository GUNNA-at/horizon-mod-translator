//! Worldgen types a mod defines in code rather than as data: a NeoForge mod's own placement
//! modifiers (Create's `create:config_filter`) and feature types (Create's `create:layered_ore`).
//! A translated mod registers a [`Handler`]; the decoders ask the handlers for a type vanilla
//! doesn't have, and the generator runs what one returns on the world and random stream vanilla's
//! own modifiers and features get, so the mod's code draws from the feature's random as it would
//! in Java.
use super::feature::FeatureWorld;
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
    /// Whether block `block` (`minecraft:stone`) is in block tag `tag`, as the generator's tags
    /// hold it (vanilla's and the packs'): a state's `is(TagKey)` where no position is at hand.
    fn named_block_in_tag(&mut self, block: &str, tag: &str) -> bool;
    /// `ensureCanWrite(position)`: whether the region being generated may write there.
    fn can_write(&mut self, position: [i32; 3]) -> bool;
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
    /// `setBlock(position, state, flags)`, the state as [`World::block`] gives it: false where
    /// the world refused it (outside the region being generated, an unknown state) or can't be
    /// written (a placement modifier's world).
    fn set_block(&mut self, position: [i32; 3], state: &str, flags: i32) -> bool;
    /// A raw write into the chunk section (`LevelChunkSection.setBlockState`, as features write
    /// through `BulkSectionAccess`): no heightmaps, no neighbour effects.
    fn set_section_block(&mut self, position: [i32; 3], state: &str) -> bool;
    /// `PlacedFeature.place(level, generator, random, position)` from the mod's code: the placed
    /// feature `json` (an inline object, or a name as a JSON string) read with the generator's
    /// feature data and placed at `position` on this world, drawing from the stream the mod's code
    /// draws from. Whether it placed anything; an error where this world can't place features (a
    /// placement modifier's) or the feature can't be read.
    fn place_feature(&mut self, json: &str, position: [i32; 3]) -> Result<bool, String> {
        let _ = (json, position);
        Err("this world places no features".into())
    }
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

/// A mod's feature, read from its configured feature's JSON: what its `Feature.place` does at a
/// position (true when it placed something).
pub trait Feature: Send + Sync {
    fn place(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<bool, String>;
}

/// The content root's registries as a mod's biome modifier reads them while the root builds (no
/// generator exists yet).
pub trait Biomes {
    /// The ids holder set `set` names (its JSON: an id, `#tag`, a list of ids, NeoForge's `any`,
    /// `or`, `and` and `not` objects) in registry `registry` (`biome`, `placed_feature`), in order;
    /// an error for an id or tag the root doesn't have.
    fn resolve(&self, registry: &str, set: &str) -> Result<Vec<String>, String>;
    /// Biome `id`'s file (`temperature`, `has_precipitation`...).
    fn biome(&self, id: &str) -> Option<String>;
}

/// NeoForge's `BiomeModifier.Phase`s, in order: every biome's modifiers run phase by phase.
pub const PHASES: [&str; 5] = ["BEFORE_EVERYTHING", "ADD", "REMOVE", "MODIFY", "AFTER_EVERYTHING"];

/// A biome's placed features by generation step (vanilla's `GenerationStep.Decoration` order):
/// each an id (`minecraft:patch_grass_plain`), or an inline placed feature's JSON.
pub type StepFeatures = Vec<Vec<String>>;

/// A mod's biome modifier, read from its file: what its `modify(biome, phase, builder)` does to
/// a biome's placed features.
pub trait BiomeModifier: Send + Sync {
    /// Runs the modifier on biome `biome` in phase `phase` (an index of [`PHASES`]), editing
    /// `features` as the builder's lists let it.
    fn modify(&self, biomes: &dyn Biomes, biome: &str, phase: usize, features: &mut StepFeatures) -> Result<(), String>;
}

/// What a mod registers: its own worldgen types, by id.
pub trait Handler: Send + Sync {
    /// The modifier `json` (the whole object, `type` included) describes when `kind` is one of
    /// the mod's placement modifier types; `None` when it isn't the mod's.
    fn placement(&self, kind: &str, json: &str) -> Option<Result<Arc<dyn Placement>, String>>;
    /// The configured feature `json` (`type` and `config`) describes when `kind` is one of the
    /// mod's feature types; `None` when it isn't the mod's.
    fn feature(&self, _kind: &str, _json: &str) -> Option<Result<Arc<dyn Feature>, String>> {
        None
    }
    /// The biome modifier file `json` describes when `kind` (its `type`) is one of the mod's
    /// biome modifier serializers; `None` when it isn't the mod's.
    fn biome_modifier(&self, _kind: &str, _json: &str) -> Option<Result<Arc<dyn BiomeModifier>, String>> {
        None
    }
}

static HANDLERS: RwLock<Vec<(String, Arc<dyn Handler>)>> = RwLock::new(Vec::new());
static BLOCKS: RwLock<Option<&'static BlockRegistry>> = RwLock::new(None);
static NAMES: RwLock<Option<Arc<Names>>> = RwLock::new(None);

/// What the generator names that mods' code reads by name: its biomes by id, and the biome and
/// block tags by tag name with their members.
struct Names {
    biomes: Vec<Arc<str>>,
    biome_tags: BTreeMap<String, Vec<String>>,
    block_tags: BTreeMap<String, Vec<String>>,
}

/// A tag's name as the generator keys them: `minecraft:` where it names no namespace.
fn tag_name(tag: &str) -> String {
    if tag.contains(':') { tag.to_owned() } else { format!("minecraft:{tag}") }
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

/// The biomes mods' code names (the generator's, by [`super::biome::BiomeId`]), and the biome
/// and block tags by tag name (`minecraft:is_overworld`) with what each holds.
pub fn set_names(
    biomes: Vec<Arc<str>>,
    biome_tags: BTreeMap<String, Vec<String>>,
    block_tags: BTreeMap<String, Vec<String>>,
) {
    *NAMES.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Names { biomes, biome_tags, block_tags }));
}

fn handlers() -> Vec<Arc<dyn Handler>> {
    HANDLERS.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, h)| h.clone()).collect()
}

/// The first registered handler that knows placement modifier type `kind`.
pub(crate) fn placement(kind: &str, json: &str) -> Option<Result<Arc<dyn Placement>, String>> {
    handlers().iter().find_map(|h| h.placement(kind, json))
}

/// The first registered handler that knows feature type `kind`.
pub(crate) fn feature(kind: &str, json: &str) -> Option<Result<Arc<dyn Feature>, String>> {
    handlers().iter().find_map(|h| h.feature(kind, json))
}

/// The first registered handler that knows biome modifier type `kind`: the modifier its file
/// `json` describes, or why the mod's codec refused it; `None` where no mod registered the type.
pub fn biome_modifier(kind: &str, json: &str) -> Option<Result<Arc<dyn BiomeModifier>, String>> {
    handlers().iter().find_map(|h| h.biome_modifier(kind, json))
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
        // A modifier reads the world; it doesn't write it.
        let mut world = ContextWorld::new(context, |_, _, _, _| false);
        self.inner
            .positions(&mut world, &mut RandomBridge(random), origin)
            .map_err(|e| PlacementQueryError::Failed(format!("{}: {e}", self.kind)))
    }
}

/// A mod's feature in a configured feature: its type and what its handler made of it.
#[derive(Clone)]
pub struct ForeignFeature {
    pub kind: String,
    pub(crate) inner: Arc<dyn Feature>,
}

impl std::fmt::Debug for ForeignFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ForeignFeature({})", self.kind)
    }
}

impl ForeignFeature {
    /// The mod's `place` at `origin` on `world`, which it reads and writes.
    pub(crate) fn place<W: FeatureWorld + PlacementContext, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        // The mod's code and the vanilla placed features it places (`PlacedFeature.place`) draw
        // from the one stream, in the order they ask.
        let random = std::cell::RefCell::new(random);
        let mut place = |world: &mut W, json: &str, position: [i32; 3]| -> Result<bool, String> {
            let mut random = random.try_borrow_mut().map_err(|_| "the feature's random stream is in use".to_owned())?;
            match world.place_feature_json(json, &mut **random, position) {
                Ok(Some(placed)) => Ok(placed),
                Ok(None) => Err("this world places no features".into()),
                Err(e) => Err(e.to_string()),
            }
        };
        let mut world = ContextWorld::new(world, write_feature_world::<W>).with_place(&mut place);
        self.inner
            .place(&mut world, &mut CellRandom(&random), origin)
            .map_err(|e| PlacementQueryError::Failed(format!("{}: {e}", self.kind)))
    }
}

/// How a mod's code writes a block: `setBlock` with its flags, or into the section.
#[derive(Clone, Copy)]
enum Write {
    Block(i32),
    Section,
}

/// A feature's world takes both writes.
fn write_feature_world<W: FeatureWorld + PlacementContext>(world: &mut W, position: [i32; 3], state: BlockStateId, write: Write) -> bool {
    match write {
        Write::Block(flags) => FeatureWorld::set_block(world, position, state, flags).unwrap_or(false),
        Write::Section => PlacementContext::set_section_block(world, position, state).is_ok(),
    }
}

/// How a feature's world places a vanilla placed feature for the mod's code: its JSON at a
/// position.
type PlaceFeature<'a, C> = &'a mut dyn FnMut(&mut C, &str, [i32; 3]) -> Result<bool, String>;

/// A placement context as the [`World`] a mod's code reads, writing through `write` and placing
/// vanilla's placed features through `place` (a feature's world; a modifier's has none).
struct ContextWorld<'a, C: PlacementContext + ?Sized> {
    context: &'a mut C,
    write: fn(&mut C, [i32; 3], BlockStateId, Write) -> bool,
    place: Option<PlaceFeature<'a, C>>,
    registry: Option<&'static BlockRegistry>,
    names: Option<Arc<Names>>,
}

impl<'a, C: PlacementContext + ?Sized> ContextWorld<'a, C> {
    fn new(context: &'a mut C, write: fn(&mut C, [i32; 3], BlockStateId, Write) -> bool) -> Self {
        let registry = context.block_registry().or_else(|| *BLOCKS.read().unwrap_or_else(|e| e.into_inner()));
        let names = NAMES.read().unwrap_or_else(|e| e.into_inner()).clone();
        Self { context, write, place: None, registry, names }
    }
    fn with_place(mut self, place: PlaceFeature<'a, C>) -> Self {
        self.place = Some(place);
        self
    }
    fn write(&mut self, position: [i32; 3], state: &str, how: Write) -> bool {
        let Some(id) = self.registry.and_then(|r| super::structure::template::parse_state_string(r, state)) else {
            return false;
        };
        (self.write)(self.context, position, id, how)
    }
}

impl<C: PlacementContext + ?Sized> World for ContextWorld<'_, C> {
    fn block(&mut self, position: [i32; 3]) -> Option<String> {
        let id = self.context.block_state(position).ok()?;
        state_text(self.registry?, id)
    }
    fn block_in_tag(&mut self, position: [i32; 3], tag: &str) -> bool {
        self.context.block_matches_tag(position, tag).unwrap_or(false)
    }
    fn named_block_in_tag(&mut self, block: &str, tag: &str) -> bool {
        let names = self.names.as_ref();
        names.and_then(|n| n.block_tags.get(&tag_name(tag))).is_some_and(|members| members.iter().any(|m| m == block))
    }
    fn can_write(&mut self, position: [i32; 3]) -> bool {
        self.context.ensure_can_write(position).unwrap_or(false)
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
        self.names.as_ref()?.biomes.get(id.0 as usize).map(|name| name.to_string())
    }
    fn biome_in_tag(&mut self, biome: &str, tag: &str) -> bool {
        let names = self.names.as_ref();
        names.and_then(|n| n.biome_tags.get(&tag_name(tag))).is_some_and(|members| members.iter().any(|m| m == biome))
    }
    fn set_block(&mut self, position: [i32; 3], state: &str, flags: i32) -> bool {
        self.write(position, state, Write::Block(flags))
    }
    fn set_section_block(&mut self, position: [i32; 3], state: &str) -> bool {
        self.write(position, state, Write::Section)
    }
    fn place_feature(&mut self, json: &str, position: [i32; 3]) -> Result<bool, String> {
        match self.place.as_mut() {
            Some(place) => place(&mut *self.context, json, position),
            None => Err("a placement modifier's world places no features".into()),
        }
    }
}

/// The feature's random stream, which the vanilla features the mod's code places draw from too,
/// as the [`Random`] the mod draws from.
struct CellRandom<'a, 'b, R: RandomSource>(&'a std::cell::RefCell<&'b mut R>);

impl<R: RandomSource> Random for CellRandom<'_, '_, R> {
    fn next_int(&mut self) -> i32 {
        self.0.borrow_mut().next_int()
    }
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.0.borrow_mut().next_int_bound(bound).unwrap_or(0)
    }
    fn next_long(&mut self) -> i64 {
        self.0.borrow_mut().next_long()
    }
    fn next_boolean(&mut self) -> bool {
        self.0.borrow_mut().next_boolean()
    }
    fn next_float(&mut self) -> f32 {
        self.0.borrow_mut().next_float()
    }
    fn next_double(&mut self) -> f64 {
        self.0.borrow_mut().next_double()
    }
    fn next_gaussian(&mut self) -> f64 {
        self.0.borrow_mut().next_gaussian()
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
    use crate::nbt::Tag;
    use crate::worldgen::{
        DecodeError, anchor::WorldGenerationContext, feature::FeatureBlocks, feature_graph::FeatureDefinition,
        placement::PlacementModifier,
    };
    use horizon_core::random::LegacyRandom;
    use std::collections::HashMap;

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

    /// A mod's feature: when the block under its origin is stone, a column of `state` up from the
    /// origin, one to `height` blocks (a draw), written into the section, capped with stone through
    /// `setBlock`; nothing elsewhere. A state the world doesn't have is refused.
    struct Column {
        height: i32,
        state: String,
    }
    impl Feature for Column {
        fn place(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<bool, String> {
            let [x, y, z] = origin;
            if world.block([x, y - 1, z]).as_deref() != Some("minecraft:stone") {
                return Ok(false);
            }
            assert!(!world.set_block([x, y - 2, z], "foreign_test:nothing", 3));
            let n = 1 + random.next_int_bound(self.height);
            for i in 0..n {
                world.set_section_block([x, y + i, z], &self.state);
            }
            Ok(world.set_block([x, y + n, z], "minecraft:stone", 2))
        }
    }
    struct ColumnHandler;
    impl Handler for ColumnHandler {
        fn placement(&self, _: &str, _: &str) -> Option<Result<Arc<dyn Placement>, String>> {
            None
        }
        fn feature(&self, kind: &str, json: &str) -> Option<Result<Arc<dyn Feature>, String>> {
            (kind == "foreign_test:column").then(|| {
                let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
                let config = &v["config"];
                let height = config["height"].as_i64().ok_or("No key height in MapLike")? as i32;
                let state = config["state"].as_str().ok_or("No key state in MapLike")?.to_owned();
                Ok(Arc::new(Column { height, state }) as Arc<dyn Feature>)
            })
        }
    }

    /// Air, stone, and `foreign_test:pipe` along `x` or `y`.
    fn registry() -> &'static BlockRegistry {
        let state = |id: &str, axis: Option<&str>| {
            let mut entries = vec![("id", Tag::String(id.into()))];
            if let Some(axis) = axis {
                entries.push(("properties", Tag::compound([("axis", Tag::String(axis.into()))])));
            }
            Tag::compound(entries)
        };
        let registry = BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(crate::DATA_VERSION)),
            (
                "states",
                Tag::List(vec![
                    state("minecraft:air", None),
                    state("minecraft:stone", None),
                    state("foreign_test:pipe", Some("x")),
                    state("foreign_test:pipe", Some("y")),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:air", Tag::Int(0)),
                    ("minecraft:stone", Tag::Int(1)),
                    ("foreign_test:pipe", Tag::Int(3)),
                ]),
            ),
        ]))
        .unwrap();
        Box::leak(Box::new(registry))
    }

    /// A region of air with the writes kept in order, each with how it was made.
    struct Grid {
        registry: &'static BlockRegistry,
        blocks: HashMap<[i32; 3], BlockStateId>,
        writes: Vec<([i32; 3], &'static str, &'static str)>,
        /// The placed features asked for: the JSON, where, and the draw the feature took.
        placed: Vec<(String, [i32; 3], i32)>,
    }
    impl Grid {
        fn get(&self, p: [i32; 3]) -> BlockStateId {
            self.blocks.get(&p).copied().unwrap_or(BlockStateId(0))
        }
        fn put(&mut self, p: [i32; 3], state: BlockStateId, how: &'static str) {
            self.blocks.insert(p, state);
            let name = ["air", "stone", "pipe[axis=x]", "pipe[axis=y]"][state.0 as usize];
            self.writes.push((p, name, how));
        }
    }
    impl FeatureWorld for Grid {
        type Error = PlacementQueryError;
        fn block(&mut self, p: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
            Ok(self.get(p))
        }
        fn set_block(&mut self, p: [i32; 3], state: BlockStateId, _flags: i32) -> Result<bool, PlacementQueryError> {
            self.put(p, state, "block");
            Ok(true)
        }
    }
    impl PlacementContext for Grid {
        fn generation(&self) -> WorldGenerationContext {
            WorldGenerationContext::new(-64, 384, 63)
        }
        fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
            Ok(-64)
        }
        fn height(&mut self, _: crate::heightmap::HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
            Ok(70)
        }
        fn block_state(&mut self, p: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
            Ok(self.get(p))
        }
        fn set_section_block(&mut self, p: [i32; 3], state: BlockStateId) -> Result<(), PlacementQueryError> {
            self.put(p, state, "section");
            Ok(())
        }
        fn block_registry(&self) -> Option<&'static BlockRegistry> {
            Some(self.registry)
        }
        /// A placed feature takes one draw of a hundred from the stream it is handed.
        fn place_feature_json<R: RandomSource>(&mut self, json: &str, random: &mut R, origin: [i32; 3]) -> Result<Option<bool>, PlacementQueryError> {
            let draw = random.next_int_bound(100).unwrap();
            self.placed.push((json.to_owned(), origin, draw));
            Ok(Some(draw % 2 == 0))
        }
    }

    /// A feature type a mod registered decodes through its handler; placed, the mod's code reads
    /// the world, draws from the feature's stream, and writes the states it names as text through
    /// the section or `setBlock` as it asked. A type nobody registered is vanilla's to read.
    #[test]
    fn a_mods_feature_writes_the_states_it_names_the_way_it_asks() {
        register("foreign_test_feature", Arc::new(ColumnHandler));
        let json = r#"{"type": "foreign_test:column", "config": {"height": 4, "state": "foreign_test:pipe[axis=x]"}}"#;
        let feature = FeatureDefinition::from_json(json).unwrap();
        assert!(matches!(&feature, FeatureDefinition::Foreign(f) if f.kind == "foreign_test:column"));
        let blocks = FeatureBlocks {
            air: BlockStateId(0),
            stone: BlockStateId(1),
            cobblestone: BlockStateId(1),
            end_stone: BlockStateId(1),
            obsidian: BlockStateId(1),
        };
        for seed in (0..8i64).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)) {
            let mut grid = Grid { registry: registry(), blocks: HashMap::new(), writes: Vec::new(), placed: Vec::new() };
            grid.blocks.insert([2, 9, 5], BlockStateId(1));
            let mut random = LegacyRandom::new(seed);
            assert!(feature.place(&mut grid, &mut random, [2, 10, 5], blocks).unwrap());
            let mut expected_random = LegacyRandom::new(seed);
            let n = 1 + expected_random.next_int_bound(4).unwrap();
            let mut expected: Vec<([i32; 3], &str, &str)> = (0..n).map(|i| ([2, 10 + i, 5], "pipe[axis=x]", "section")).collect();
            expected.push(([2, 10 + n, 5], "stone", "block"));
            assert_eq!(grid.writes, expected, "seed {seed}");
            assert_eq!(random.next_long(), expected_random.next_long(), "seed {seed}");
            // Over air, the mod's code places nothing.
            let mut empty = Grid { registry: registry(), blocks: HashMap::new(), writes: Vec::new(), placed: Vec::new() };
            assert!(!feature.place(&mut empty, &mut LegacyRandom::new(seed), [2, 10, 5], blocks).unwrap());
            assert!(empty.writes.is_empty());
        }
        match FeatureDefinition::from_json(r#"{"type": "foreign_test:column", "config": {"height": 4}}"#) {
            Err(DecodeError::Registry(e)) => assert!(e.contains("No key state"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(FeatureDefinition::from_json(r#"{"type": "foreign_test:other", "config": {}}"#).is_err());
    }

    /// A mod's feature that places a vanilla placed feature (Farmer's Delight's wild crops call
    /// `PlacedFeature.place` on the ones their configuration holds): a draw of four lifts the
    /// position, the placed feature goes there, then one more draw.
    struct Planter;
    impl Feature for Planter {
        fn place(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<bool, String> {
            let lift = random.next_int_bound(4);
            let placed = world.place_feature(r#"{"feature": "test:crop", "placement": []}"#, [origin[0], origin[1] + lift, origin[2]])?;
            random.next_int_bound(4);
            Ok(placed)
        }
    }
    struct PlanterHandler;
    impl Handler for PlanterHandler {
        fn placement(&self, _: &str, _: &str) -> Option<Result<Arc<dyn Placement>, String>> {
            None
        }
        fn feature(&self, kind: &str, _: &str) -> Option<Result<Arc<dyn Feature>, String>> {
            (kind == "foreign_test:planter").then(|| Ok(Arc::new(Planter) as Arc<dyn Feature>))
        }
    }

    /// The vanilla placed feature a mod's code places is the world's to read and place, at the
    /// position the code names, and draws from the feature's own stream between the code's
    /// draws, in the order they come; what it placed is the code's answer. A modifier's world
    /// places none.
    #[test]
    fn a_mods_feature_places_vanilla_features_on_its_own_random_stream() {
        register("foreign_test_planter", Arc::new(PlanterHandler));
        let feature = FeatureDefinition::from_json(r#"{"type": "foreign_test:planter", "config": {}}"#).unwrap();
        let blocks = FeatureBlocks {
            air: BlockStateId(0),
            stone: BlockStateId(1),
            cobblestone: BlockStateId(1),
            end_stone: BlockStateId(1),
            obsidian: BlockStateId(1),
        };
        for seed in (0..8i64).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)) {
            let mut grid = Grid { registry: registry(), blocks: HashMap::new(), writes: Vec::new(), placed: Vec::new() };
            let mut random = LegacyRandom::new(seed);
            let placed = feature.place(&mut grid, &mut random, [4, 20, -3], blocks).unwrap();
            let mut expected_random = LegacyRandom::new(seed);
            let lift = expected_random.next_int_bound(4).unwrap();
            let draw = expected_random.next_int_bound(100).unwrap();
            expected_random.next_int_bound(4).unwrap();
            assert_eq!(grid.placed, vec![(r#"{"feature": "test:crop", "placement": []}"#.to_owned(), [4, 20 + lift, -3], draw)], "seed {seed}");
            assert_eq!(placed, draw % 2 == 0, "seed {seed}");
            assert_eq!(random.next_long(), expected_random.next_long(), "seed {seed}");
        }
        let mut grid = Grid { registry: registry(), blocks: HashMap::new(), writes: Vec::new(), placed: Vec::new() };
        let mut world = ContextWorld::new(&mut grid, |_, _, _, _| false);
        assert!(world.place_feature("{}", [0, 0, 0]).is_err());
    }
}
