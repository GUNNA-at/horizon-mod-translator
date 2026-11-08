//! Which of a block's faces are drawn, as 1.21.1's `BlockStateBase` and NeoForge answer a mod that
//! asks: a renderer drawing a part on a face only where that face shows (Create's gauges put their
//! dial on the faces `Block.shouldRenderFace` lets through), a contraption's seat letting go of its
//! passenger in a block that occludes. Whether a state can hide its neighbours' faces
//! (`canOcclude`), is an opaque full cube to the renderer (`isSolidRender`), leaves out its face
//! toward a neighbour (`skipRendering`: glass beside glass), its occlusion shape's face toward a
//! direction (`getFaceOcclusionShape`), NeoForge's face hiding (`hidesNeighborFace`,
//! `supportsExternalFaceHiding`), and `Block.shouldRenderFace`, which puts them together.
//!
//! A mod's block answers from the properties it was made with (`noOcclusion`, a copy of a vanilla
//! block's) and from its own shapes and overrides; a vanilla block from the game
//! ([`crate::host::Host::block_occlusion`], [`crate::host::Host::face_occlusion`]) and from its
//! 1.21.1 class. Vanilla caches a shape-independent state's occlusion shape from an empty level;
//! here it is asked where the block stands, which is the same for a block whose shape doesn't read
//! the level.
use crate::shapes::{self, Box3};
use crate::{api, Rt, V};

const SKIP: &str = "skipRendering.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z";
const SKIP_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.skipRendering.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z";
const HIDES: &str = "hidesNeighborFace.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z";
const HIDES_KEY: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension.hidesNeighborFace.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z";
const SUPPORTS: &str = "supportsExternalFaceHiding.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const SUPPORTS_KEY: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension.supportsExternalFaceHiding.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const OCCLUSION_SHAPE: &str = "net/minecraft/world/level/block/state/BlockState.getOcclusionShape.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_BLOCK_STATE: &str = "net/minecraft/world/level/BlockGetter.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const FLUID_STATE: &str = "net/minecraft/world/level/block/state/BlockState.getFluidState.()Lnet/minecraft/world/level/material/FluidState;";
const FLUID_TYPE: &str = "net/minecraft/world/level/material/FluidState.getType.()Lnet/minecraft/world/level/material/Fluid;";
const FLUID_SAME: &str = "net/minecraft/world/level/material/Fluid.isSame.(Lnet/minecraft/world/level/material/Fluid;)Z";
/// `Block.shouldRenderFace(state, level, pos, face, neighbourPos)`'s descriptor.
pub(crate) const SHOULD_RENDER_FACE: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/core/BlockPos;)Z";
/// `Block.skipRendering(state, neighbour, face)`'s descriptor (a mod block's `super.` call).
pub(crate) const SKIP_RENDERING: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z";
/// `PipeBlock.PROPERTY_BY_DIRECTION`'s names, by direction ordinal: the sides bars join on.
const SIDES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

fn text_of(rt: &Rt, state: V) -> String {
    rt.str_of(state).unwrap_or("minecraft:air").to_owned()
}

fn is_air(text: &str) -> bool {
    matches!(crate::host::split_state(text).0, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
}

/// The block object a mod made for state `text` (with the properties it was given) and its class;
/// None for a vanilla block, which the runtime stands for by its name.
pub(crate) fn made_by_mod(rt: &Rt, text: &str) -> Option<(V, &'static str)> {
    let (name, _) = crate::host::split_state(text);
    let b = rt.registered.get(&("block".to_owned(), name.to_owned())).copied().or_else(|| api::unregistered_block(rt, name))?;
    rt.str_of(b).is_none().then(|| (b, rt.class_of(b)))
}

/// The 1.21.1 class whose `skipRendering` a state's block has: a mod block's first vanilla class
/// above its own, a vanilla block's own class.
fn vanilla_class(rt: &Rt, text: &str) -> &'static str {
    match made_by_mod(rt, text) {
        Some((_, class)) => crate::rotation::vanilla_base(rt, class),
        None => {
            let (name, _) = crate::host::split_state(text);
            let table = &crate::vanilla_classes::BLOCKS;
            table.binary_search_by(|(id, _)| (*id).cmp(name)).map_or("net/minecraft/world/level/block/Block", |i| table[i].1)
        }
    }
}

/// What the game says of a vanilla state's faces: whether it can occlude and is solid to the
/// renderer. Air hides nothing.
fn game_occlusion(rt: &Rt, text: &str) -> Option<(bool, bool)> {
    if is_air(text) {
        return Some((false, false));
    }
    rt.host().and_then(|h| h.block_occlusion(text))
}

/// `state.canOcclude()`: a mod's block as its properties say (`noOcclusion`, `noCollission`, or a
/// copy of a vanilla block that can't), a vanilla block as the game has it; where neither says,
/// `Properties.of()`'s: it can.
pub(crate) fn can_occlude(rt: &Rt, text: &str) -> bool {
    if let Some((b, _)) = made_by_mod(rt, text) {
        let props = rt.get_field(b, "$properties");
        if props.r() == 0 {
            return true;
        }
        if rt.get_field(props, "$noOcclusion").i() != 0 {
            return false;
        }
        let copy = rt.str_of(rt.get_field(props, "$copy")).map(str::to_owned);
        return copy.is_none_or(|c| game_occlusion(rt, &c).is_none_or(|o| o.0));
    }
    game_occlusion(rt, text).is_none_or(|o| o.0)
}

/// `state.getOcclusionShape(level, pos)`, as the runtime answers it (the mod block's own, else its
/// shape).
fn occlusion_shape(rt: &mut Rt, state: V, level: V, pos: V) -> Vec<Box3> {
    let s = api::call(rt, OCCLUSION_SHAPE, &[state, level, pos]);
    shapes::boxes(rt, s).unwrap_or_default()
}

/// `state.isSolidRender(level, pos)`: a state that can occlude whose occlusion shape is the whole
/// block; a vanilla state as the game has it.
pub(crate) fn solid_render(rt: &mut Rt, state: V, level: V, pos: V) -> bool {
    let text = text_of(rt, state);
    if made_by_mod(rt, &text).is_none()
        && let Some((_, solid)) = game_occlusion(rt, &text)
    {
        return solid;
    }
    can_occlude(rt, &text) && shapes::full_block(&occlusion_shape(rt, state, level, pos))
}

/// `state.getFaceOcclusionShape(level, pos, direction)`: its occlusion shape's face toward
/// `direction` (`Shapes.getFaceShape`). A vanilla state that can occlude hides faces as the game
/// hides them with it, where the game says.
pub(crate) fn face_occlusion(rt: &mut Rt, state: V, level: V, pos: V, direction: usize) -> Vec<Box3> {
    let text = text_of(rt, state);
    if made_by_mod(rt, &text).is_none()
        && can_occlude(rt, &text)
        && let Some(face) = rt.host().and_then(|h| h.face_occlusion(&text, direction.min(5) as u8))
    {
        return face;
    }
    let shape = occlusion_shape(rt, state, level, pos);
    shapes::face_slice(&shape, direction.min(5))
}

/// `Block.skipRendering(state, neighbour, face)` as the vanilla class `class` (or the first of its
/// superclasses with one) answers: glass (any `HalfTransparentBlock`) and powder snow leave out
/// their faces toward the same block, bars and panes theirs toward the same block above or below or
/// where both join, a liquid toward the same fluid, mangrove roots theirs above or below mangrove
/// roots; any other block draws them.
pub(crate) fn class_skips(rt: &mut Rt, class: &str, state: V, neighbour: V, face: V) -> bool {
    let is = |c: &str| crate::api_common::vanilla_is_a(class, &format!("net/minecraft/world/level/block/{c}"));
    let (text, other) = (text_of(rt, state), text_of(rt, neighbour));
    let d = usize::from(crate::api_common::direction_ordinal(rt, face)).min(5);
    let same = crate::host::split_state(&text).0 == crate::host::split_state(&other).0;
    if is("HalfTransparentBlock") || is("PowderSnowBlock") {
        same
    } else if is("IronBarsBlock") {
        let joins = |t: &str, side: &str| crate::host::split_state(t).1.iter().any(|(k, v)| *k == side && *v == "true");
        same && (d < 2 || joins(&text, SIDES[d]) && joins(&other, SIDES[d ^ 1]))
    } else if is("MangroveRootsBlock") {
        d < 2 && crate::host::split_state(&other).0 == "minecraft:mangrove_roots"
    } else if is("LiquidBlock") {
        let own = api::call(rt, FLUID_STATE, &[state]);
        let own = api::call(rt, FLUID_TYPE, &[own]);
        let theirs = api::call(rt, FLUID_STATE, &[neighbour]);
        let theirs = api::call(rt, FLUID_TYPE, &[theirs]);
        theirs.r() != 0 && api::call(rt, FLUID_SAME, &[theirs, own]).i() != 0
    } else {
        false
    }
}

/// `state.skipRendering(neighbour, face)`: whether the block leaves out its face toward
/// `neighbour`: a mod block's own answer where its class has one, else its 1.21.1 class's
/// ([`class_skips`]).
pub(crate) fn skip_rendering(rt: &mut Rt, state: V, neighbour: V, face: V) -> bool {
    let text = text_of(rt, state);
    if let Some((b, class)) = made_by_mod(rt, &text)
        && rt.overrides.is_some_and(|o| o(class, SKIP))
        && let Some(vc) = rt.vcall
    {
        let r = vc(rt, SKIP_KEY, SKIP, &[b, state, neighbour, face]);
        if rt.thrown.is_none() {
            return r.i() != 0;
        }
        rt.report_thrown("skipRendering");
        return false;
    }
    let class = vanilla_class(rt, &text);
    class_skips(rt, class, state, neighbour, face)
}

/// NeoForge's `state.hidesNeighborFace(level, pos, neighbour, direction)`: whether the block hides
/// its neighbour's face toward it (a copycat showing the neighbour's own material): the mod block's
/// own answer, else no.
pub(crate) fn hides_neighbor_face(rt: &mut Rt, state: V, level: V, pos: V, neighbour: V, direction: V) -> bool {
    let text = text_of(rt, state);
    if let Some((b, class)) = made_by_mod(rt, &text)
        && rt.overrides.is_some_and(|o| o(class, HIDES))
        && let Some(vc) = rt.vcall
    {
        let r = vc(rt, HIDES_KEY, HIDES, &[b, level, pos, state, neighbour, direction]);
        if rt.thrown.is_none() {
            return r.i() != 0;
        }
        rt.report_thrown("hidesNeighborFace");
    }
    false
}

/// NeoForge's `state.supportsExternalFaceHiding()`: whether a neighbour may hide the block's faces.
/// The mod block's own answer; else NeoForge's: yes on a server, and on a client for a block not
/// drawn in the solid layer. The layer isn't known here: a block is taken as drawn in it, as most
/// are.
pub(crate) fn supports_external_face_hiding(rt: &mut Rt, state: V) -> bool {
    let text = text_of(rt, state);
    if let Some((b, class)) = made_by_mod(rt, &text)
        && rt.overrides.is_some_and(|o| o(class, SUPPORTS))
        && let Some(vc) = rt.vcall
    {
        let r = vc(rt, SUPPORTS_KEY, SUPPORTS, &[b, state]);
        if rt.thrown.is_none() {
            return r.i() != 0;
        }
        rt.report_thrown("supportsExternalFaceHiding");
    }
    rt.host().is_some_and(|h| h.is_server())
}

/// `Block.shouldRenderFace(state, level, pos, face, neighbourPos)`, NeoForge's: whether the face of
/// `state` (at `pos`) toward `face` shows past the block the level has at `neighbourPos`. Not where
/// the block leaves it out toward that neighbour, nor where the neighbour hides it and the block
/// lets it; else wherever the neighbour can't occlude, the block's own face is empty, or part of the
/// block's face is outside the neighbour's face toward it.
pub(crate) fn should_render_face(rt: &mut Rt, [state, level, pos, face, at]: [V; 5]) -> bool {
    let neighbour = crate::menus::virt(rt, GET_BLOCK_STATE, level, &[at]);
    let d = usize::from(crate::api_common::direction_ordinal(rt, face)).min(5);
    if skip_rendering(rt, state, neighbour, face) {
        return false;
    }
    let back = api::direction(rt, (d ^ 1) as i64);
    if hides_neighbor_face(rt, neighbour, level, at, state, back) && supports_external_face_hiding(rt, state) {
        return false;
    }
    if !can_occlude(rt, &text_of(rt, neighbour)) {
        return true;
    }
    let own = face_occlusion(rt, state, level, pos, d);
    if own.is_empty() {
        return true;
    }
    let theirs = face_occlusion(rt, neighbour, level, at, d ^ 1);
    !shapes::join(&own, &theirs, shapes::ONLY_FIRST).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{block_pos, call, direction, state};

    const FULL: Box3 = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    const SHOULD: &str = "net/minecraft/world/level/block/Block.shouldRenderFace.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/core/BlockPos;)Z";
    const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
    const PROPS: &str = "net/minecraft/world/level/block/state/BlockBehaviour$Properties";

    /// Mod blocks: a dial (a full cube, `Properties.of()`), a pane of glass (`TransparentBlock`,
    /// `noOcclusion`), a copycat that hides its neighbours' faces and can't occlude, a block that
    /// lets neighbours hide its faces, one that leaves out its top face, and a block made as a copy
    /// of vanilla glass.
    fn blocks(rt: &mut Rt) {
        fn overrides(class: &str, short: &str) -> bool {
            matches!((class, short), ("test/Copycat", HIDES) | ("test/Open", SUPPORTS) | ("test/Shy", SKIP))
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match (rt.class_of(args[0]), short) {
                ("test/Copycat", HIDES) | ("test/Open", SUPPORTS) => V::I(1),
                ("test/Shy", SKIP) => V::I(i32::from(crate::api_common::direction_ordinal(rt, args[3]) == 1)),
                _ => call(rt, key, args),
            }
        }
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        let glass = api::block(rt, "minecraft:glass");
        for (class, sup, name, how) in [
            ("test/Dial", "net/minecraft/world/level/block/Block", "test:dial", ""),
            ("test/Pane", "net/minecraft/world/level/block/TransparentBlock", "test:pane", "noOcclusion"),
            ("test/Copycat", "net/minecraft/world/level/block/Block", "test:copycat", "noOcclusion"),
            ("test/Open", "net/minecraft/world/level/block/Block", "test:open", ""),
            ("test/Shy", "net/minecraft/world/level/block/Block", "test:shy", ""),
            ("test/Glassy", "net/minecraft/world/level/block/Block", "test:glassy", "copy"),
        ] {
            rt.supers.insert(class, sup);
            let props = if how == "copy" {
                call(rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.ofFullCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[glass])
            } else {
                rt.new_object(PROPS)
            };
            if how == "noOcclusion" {
                call(rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.noOcclusion.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[props]);
            }
            let b = rt.new_object(class);
            rt.put_field(b, "$properties", props);
            rt.registered.insert(("block".into(), name.into()), b);
        }
    }

    /// The renderer's facts of three vanilla blocks, as the game exports them: stone hides every
    /// face; a bottom slab can occlude but isn't a full cube, its sides covering their lower half
    /// and its top nothing; glass can't occlude.
    fn host() -> crate::host::MemoryHost {
        let mut host = crate::host::MemoryHost { client: true, ..Default::default() };
        host.occlusion.insert("minecraft:stone".into(), (true, true));
        host.occlusion.insert("minecraft:stone_slab".into(), (true, false));
        host.occlusion.insert("minecraft:glass".into(), (false, false));
        for d in 0..6 {
            host.occlusion_faces.insert(("minecraft:stone".into(), d), vec![FULL]);
            let slab = match d {
                0 => vec![FULL],
                1 => vec![],
                _ => vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]],
            };
            host.occlusion_faces.insert(("minecraft:stone_slab".into(), d), slab);
        }
        host
    }

    /// `Block.shouldRenderFace`, as NeoForge's puts a state's faces together: a dial's face shows
    /// beside air, glass, a slab's side (half of it is past the slab) and above a bottom slab, not
    /// against stone. A pane of glass leaves out its face toward the same pane, and stone or a
    /// solid mod block hides it too. A copycat beside a block hides that block's face only where
    /// the block lets it (`supportsExternalFaceHiding`, which a client's solid block doesn't by
    /// default). A block's own `skipRendering` is its answer.
    #[test]
    fn a_face_shows_unless_its_block_or_its_neighbour_hides_it() {
        crate::install();
        let mut rt = Rt::new();
        blocks(&mut rt);
        let mut host = host();
        let slab = "minecraft:stone_slab[type=bottom,waterlogged=false]";
        let at = |x: i32| [x * 4, 64, 0];
        // Each block under test at x * 4, its neighbours around it: north, up, south, east, west,
        // down.
        for (x, own, around) in [
            (0, "test:dial", ["minecraft:stone", "minecraft:air", "minecraft:glass", slab, "test:copycat", slab]),
            (1, "test:pane", ["test:pane", "minecraft:stone", "minecraft:air", "test:copycat", "test:dial", "minecraft:air"]),
            (2, "test:open", ["test:copycat", "minecraft:air", "minecraft:air", "minecraft:air", "minecraft:air", "minecraft:air"]),
            (3, "test:shy", ["minecraft:air"; 6]),
        ] {
            let [x, y, z] = at(x);
            host.blocks.insert([x, y, z], own.into());
            for (p, b) in [[x, y, z - 1], [x, y + 1, z], [x, y, z + 1], [x + 1, y, z], [x - 1, y, z], [x, y - 1, z]].into_iter().zip(around) {
                host.blocks.insert(p, b.into());
            }
        }
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            // Whether block `x`'s face toward direction `d` (down, up, north, south, west, east)
            // shows.
            let shows = |rt: &mut Rt, x: i32, d: usize| {
                let [x, y, z] = at(x);
                let own = rt.host().map(|h| h.block_state([x, y, z])).unwrap_or_default();
                let st = state(rt, &own);
                let pos = block_pos(rt, x.into(), y.into(), z.into());
                let step = api::DIRECTIONS[d];
                let next = block_pos(rt, i64::from(x) + step[0], i64::from(y) + step[1], i64::from(z) + step[2]);
                let face = direction(rt, d as i64);
                call(rt, SHOULD, &[st, level, pos, face, next]) == V::I(1)
            };
            let (down, up, north, south, west, east) = (0, 1, 2, 3, 4, 5);
            assert!(!shows(rt, 0, north), "stone covers the dial's face");
            assert!(shows(rt, 0, up), "beside air");
            assert!(shows(rt, 0, south), "glass can't occlude");
            assert!(shows(rt, 0, east), "the bottom slab's side covers only its lower half");
            assert!(shows(rt, 0, down), "the bottom slab's top is below the face");
            assert!(shows(rt, 0, west), "a solid block doesn't let the copycat hide its face");
            assert!(!shows(rt, 1, north), "glass beside the same glass");
            assert!(!shows(rt, 1, up), "stone covers the pane's face");
            assert!(shows(rt, 1, south), "beside air");
            assert!(shows(rt, 1, east), "the copycat can't occlude");
            assert!(!shows(rt, 1, west), "the dial is a full cube that occludes");
            assert!(!shows(rt, 2, north), "a block that lets the copycat hide its face");
            assert!(shows(rt, 2, up), "beside air");
            assert!(!shows(rt, 3, up), "the block's own skipRendering");
            assert!(shows(rt, 3, north), "the block's own skipRendering lets it");
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a state says of its faces when a mod asks it: whether it can occlude (its properties;
    /// a copy of glass can't; a vanilla block as the game has it), is solid to the renderer, its
    /// occlusion shape's face, and the vanilla classes' `skipRendering`: glass and ice beside
    /// themselves, bars toward bars where both join (and above and below), water beside water but
    /// not lava, mangrove roots above or below roots, stone never. A mod block's `super.` call to
    /// its vanilla class gets that class's answer.
    #[test]
    fn a_states_faces_are_its_properties_the_games_and_its_classes() {
        crate::install();
        let mut rt = Rt::new();
        blocks(&mut rt);
        let mut host = host();
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let pos = block_pos(rt, 0, 64, 0);
            let z = |rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]| {
                let key = rt.static_key(&format!("{owner}.{name}.{desc}"));
                call(rt, key, args)
            };
            for (text, occludes, solid) in [
                ("test:dial", true, true),
                ("test:pane", false, false),
                ("test:glassy", false, false),
                ("minecraft:stone", true, true),
                ("minecraft:stone_slab[type=bottom,waterlogged=false]", true, false),
                ("minecraft:glass", false, false),
                ("minecraft:air", false, false),
            ] {
                let st = state(rt, text);
                assert_eq!(z(rt, STATE, "canOcclude", "()Z", &[st]), V::I(occludes.into()), "{text} canOcclude");
                let solid_render = z(rt, STATE, "isSolidRender", "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z", &[st, level, pos]);
                assert_eq!(solid_render, V::I(solid.into()), "{text} isSolidRender");
            }
            let face = |rt: &mut Rt, text: &str, d: i64| {
                let (st, dir) = (state(rt, text), direction(rt, d));
                let s = z(rt, STATE, "getFaceOcclusionShape", "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Lnet/minecraft/world/phys/shapes/VoxelShape;", &[st, level, pos, dir]);
                shapes::boxes(rt, s).unwrap_or_default()
            };
            assert_eq!(face(rt, "minecraft:stone_slab[type=bottom,waterlogged=false]", 2), vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]);
            assert_eq!(face(rt, "minecraft:stone_slab[type=bottom,waterlogged=false]", 1), Vec::<Box3>::new());
            assert_eq!(face(rt, "test:dial", 1), vec![FULL]);

            let skips = |rt: &mut Rt, own: &str, other: &str, d: i64| {
                let (a, b, dir) = (state(rt, own), state(rt, other), direction(rt, d));
                z(rt, STATE, "skipRendering", "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z", &[a, b, dir]) == V::I(1)
            };
            let bars = |e: bool, w: bool| format!("minecraft:iron_bars[east={e},north=false,south=false,waterlogged=false,west={w}]");
            assert!(skips(rt, "minecraft:glass", "minecraft:glass", 2));
            assert!(skips(rt, "minecraft:ice", "minecraft:ice", 1));
            assert!(!skips(rt, "minecraft:glass", "minecraft:stone", 2));
            assert!(!skips(rt, "minecraft:stone", "minecraft:stone", 2));
            assert!(skips(rt, &bars(true, false), &bars(false, true), 5), "both join");
            assert!(!skips(rt, &bars(true, false), &bars(false, false), 5), "the other doesn't join");
            assert!(skips(rt, &bars(false, false), &bars(false, false), 1), "above");
            assert!(skips(rt, "minecraft:water[level=0]", "minecraft:water[level=3]", 2));
            assert!(!skips(rt, "minecraft:water[level=0]", "minecraft:lava[level=0]", 2));
            let roots = "minecraft:mangrove_roots[waterlogged=false]";
            assert!(skips(rt, roots, roots, 0));
            assert!(!skips(rt, roots, roots, 2));
            let pane_block = rt.registered[&("block".to_owned(), "test:pane".to_owned())];
            let (pane, north) = (state(rt, "test:pane"), direction(rt, 2));
            let sup = z(rt, "net/minecraft/world/level/block/HalfTransparentBlock", "skipRendering", SKIP_RENDERING, &[pane_block, pane, pane, north]);
            assert_eq!(sup, V::I(1));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
