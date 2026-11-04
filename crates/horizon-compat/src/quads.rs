//! Baked quads, natively: the faces a mod reads, changes and builds models from (`BakedQuad`, its
//! vertex data in `DefaultVertexFormat.BLOCK`, `SimpleBakedModel` and its builder). A block state's
//! or a named model's faces come from the game ([`Host::model_quads`]); a model a mod put together
//! from faces (Create's water wheel: its template's faces given the wheel's wood) is drawn as the
//! faces it lists, read once when catnip makes a buffer of it ([`drawable`]).
//!
//! A sprite here spans 0 to 1 of its own (see `render`), so a face's texture coordinates are
//! fractions of its sprite, and a mod moving a face from one sprite to another
//! (`newSprite.getU(getUnInterpolatedU(oldSprite, u))`) keeps them and changes the sprite.
//!
//! [`Host::model_quads`]: crate::host::Host::model_quads
use crate::host::{Model, Quad};
use crate::{Data, Rt, V};
use std::sync::{Arc, Mutex};

pub const BAKED_QUAD: &str = "net/minecraft/client/renderer/block/model/BakedQuad";
pub const SIMPLE_MODEL: &str = "net/minecraft/client/resources/model/SimpleBakedModel";
const SIMPLE_BUILDER: &str = "net/minecraft/client/resources/model/SimpleBakedModel$Builder";
const BAKED_MODEL: &str = "net/minecraft/client/resources/model/BakedModel";
const VERTEX_FORMAT: &str = "com/mojang/blaze3d/vertex/VertexFormat";
const ITEM_TRANSFORMS: &str = "net/minecraft/client/renderer/block/model/ItemTransforms";
/// Ints a vertex of `DefaultVertexFormat.BLOCK`: position (three floats), colour, texture (two
/// floats), light, normal.
pub const STRIDE: usize = 8;

/// `DefaultVertexFormat`'s formats a mod may read the size of, in bytes a vertex.
const FORMATS: [(&str, i32); 12] = [
    ("BLOCK", 32),
    ("NEW_ENTITY", 36),
    ("PARTICLE", 28),
    ("POSITION", 12),
    ("POSITION_COLOR", 16),
    ("POSITION_COLOR_NORMAL", 20),
    ("POSITION_COLOR_LIGHTMAP", 20),
    ("POSITION_TEX", 20),
    ("POSITION_TEX_COLOR", 24),
    ("POSITION_COLOR_TEX_LIGHTMAP", 28),
    ("POSITION_TEX_LIGHTMAP_COLOR", 28),
    ("POSITION_TEX_COLOR_NORMAL", 28),
];

/// Faces read from models mods built, by the number a drawable handle keeps (`$mesh`, from 1).
static MESHES: Mutex<Vec<Arc<[Quad]>>> = Mutex::new(Vec::new());

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != "com/mojang/blaze3d/vertex/DefaultVertexFormat" {
        return None;
    }
    let size = FORMATS.iter().find(|(n, _)| *n == field)?.1;
    Some(rt.intern(format!("compat:vertex-format/{field}"), |rt| {
        let f = rt.new_object(VERTEX_FORMAT);
        rt.put_field(f, "$size", V::I(size));
        let name = rt.string(field);
        rt.put_field(f, "$name", name);
        f
    }))
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, VERTEX_FORMAT | BAKED_QUAD | SIMPLE_MODEL | SIMPLE_BUILDER)
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (VERTEX_FORMAT, "getVertexSize") => rt.get_field(a(0), "$size"),
        (VERTEX_FORMAT, "getIntegerSize") => V::I(rt.get_field(a(0), "$size").i() / 4),
        // `BakedQuad(vertices, tintIndex, direction, sprite, shade[, hasAmbientOcclusion])`.
        (BAKED_QUAD, "<init>") => {
            let this = a(0);
            for (i, field) in ["vertices", "tintIndex", "direction", "sprite", "shade"].into_iter().enumerate() {
                rt.put_field(this, field, a(i + 1));
            }
            let ao = if desc.starts_with("([IILnet/minecraft/core/Direction;Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;ZZ") { a(6) } else { V::I(1) };
            rt.put_field(this, "hasAmbientOcclusion", ao);
            V::Void
        }
        (BAKED_QUAD, n) => match n {
            "getVertices" => rt.get_field(a(0), "vertices"),
            "getTintIndex" => V::I(rt.get_field(a(0), "tintIndex").i()),
            "isTinted" => V::I((rt.get_field(a(0), "tintIndex").i() != -1) as i32),
            "getDirection" => rt.get_field(a(0), "direction"),
            "getSprite" => rt.get_field(a(0), "sprite"),
            "isShade" => V::I(rt.get_field(a(0), "shade").i()),
            "hasAmbientOcclusion" => V::I(rt.get_field(a(0), "hasAmbientOcclusion").i()),
            _ => return None,
        },
        // `SimpleBakedModel(unculled, culled, ao, blockLight, gui3d, particle, transforms, overrides[,
        // renderTypes])`: the faces it lists, by the side that hides them.
        (SIMPLE_MODEL, "<init>") => {
            let this = a(0);
            for (i, field) in ["$unculled", "$culled", "$ao", "$blockLight", "$gui3d", "$particle", "$transforms", "$overrides"].into_iter().enumerate() {
                rt.put_field(this, field, a(i + 1));
            }
            V::Void
        }
        (SIMPLE_MODEL, _) => return model_call(rt, name, args),
        // NeoForge's `Builder(ao, blockLight, gui3d, transforms, overrides)`: faces added one by one.
        (SIMPLE_BUILDER, "<init>") => {
            let this = a(0);
            let unculled = crate::jutil::new_list(rt, Vec::new());
            let culled = rt.alloc("java/util/EnumMap", Data::Table(Default::default()));
            rt.put_field(this, "$unculled", unculled);
            rt.put_field(this, "$culled", culled);
            if desc.starts_with("(ZZZ") {
                for (i, field) in ["$ao", "$blockLight", "$gui3d", "$transforms", "$overrides"].into_iter().enumerate() {
                    rt.put_field(this, field, a(i + 1));
                }
            } else {
                // Vanilla's `Builder(BlockModel, ItemOverrides, gui3d)`: lit and shaded as a block.
                for (field, v) in [("$ao", V::I(1)), ("$blockLight", V::I(1)), ("$gui3d", a(3)), ("$overrides", a(2))] {
                    rt.put_field(this, field, v);
                }
            }
            V::Void
        }
        (SIMPLE_BUILDER, n) => match n {
            "addUnculledFace" => {
                let list = rt.get_field(a(0), "$unculled");
                push(rt, list, a(1));
                a(0)
            }
            "addCulledFace" => {
                let culled = rt.get_field(a(0), "$culled");
                let list = match crate::jutil::table_get(rt, culled, a(1)) {
                    Some(l) if l.r() != 0 => l,
                    _ => {
                        let l = crate::jutil::new_list(rt, Vec::new());
                        crate::jutil::table_put(rt, culled, a(1), l);
                        l
                    }
                };
                push(rt, list, a(2));
                a(0)
            }
            "particle" => {
                rt.put_field(a(0), "$particle", a(1));
                a(0)
            }
            "item" => a(0),
            "build" => {
                let this = a(0);
                let m = rt.new_object(SIMPLE_MODEL);
                for field in ["$unculled", "$culled", "$ao", "$blockLight", "$gui3d", "$particle", "$transforms", "$overrides"] {
                    let v = rt.get_field(this, field);
                    rt.put_field(m, field, v);
                }
                m
            }
            _ => return None,
        },
        _ => return None,
    })
}

fn push(rt: &mut Rt, list: V, v: V) {
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.push(v);
    }
}

/// What a model answers that it keeps itself, a `SimpleBakedModel` (or a mod's subclass of one):
/// the faces it lists, its flags and particle sprite. None for any other model.
pub fn model_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    if matches!(rt.get_field(this, "$unculled"), V::Void) {
        return None;
    }
    Some(match name {
        "getQuads" => {
            let side = args.get(2).copied().unwrap_or_default();
            let list = if side.r() == 0 {
                rt.get_field(this, "$unculled")
            } else {
                let culled = rt.get_field(this, "$culled");
                crate::jutil::table_get(rt, culled, side).unwrap_or(V::NULL)
            };
            let items = crate::jutil::elements(rt, list);
            crate::jutil::new_list(rt, items)
        }
        "useAmbientOcclusion" => V::I(rt.get_field(this, "$ao").i()),
        "usesBlockLight" => V::I(rt.get_field(this, "$blockLight").i()),
        "isGui3d" => V::I(rt.get_field(this, "$gui3d").i()),
        "isCustomRenderer" => V::I(0),
        "getParticleIcon" => rt.get_field(this, "$particle"),
        "getTransforms" => match rt.get_field(this, "$transforms") {
            t if t.r() != 0 => t,
            _ => transforms(rt),
        },
        "getOverrides" => rt.get_field(this, "$overrides"),
        _ => return None,
    })
}

/// `ItemTransforms.NO_TRANSFORMS`-like: what a model the game draws hands a mod copying it.
pub fn transforms(rt: &mut Rt) -> V {
    rt.intern("compat:item-transforms".into(), |rt| rt.new_object(ITEM_TRANSFORMS))
}

/// The game's model a `BakedModel` object stands for: a block state's (`$state`), a named one
/// (`$name`), or faces read from a mod's model (`$mesh`).
pub fn handle(rt: &Rt, model: V) -> Option<Model> {
    if let Some(s) = rt.str_of(rt.get_field(model, "$state")) {
        return Some(Model::State(s.to_owned()));
    }
    if let Some(n) = rt.str_of(rt.get_field(model, "$name")) {
        return Some(Model::Named(n.to_owned()));
    }
    let i = rt.get_field(model, "$mesh").i();
    let meshes = MESHES.lock().ok()?;
    meshes.get(usize::try_from(i).ok()?.checked_sub(1)?).cloned().map(Model::Quads)
}

/// `getQuads(state, side, ...)` of a model the game has: its faces hidden by `side` (all those no
/// side hides for null), each a new `BakedQuad` the mod may change.
pub fn handle_quads(rt: &mut Rt, model: &Model, side: V) -> V {
    let quads = rt.host().and_then(|h| h.model_quads(model));
    let side = (side.r() != 0).then(|| crate::api_common::direction_ordinal(rt, side));
    let items = quads.map_or_else(Vec::new, |quads| {
        quads.iter().filter(|q| q.cull == side).map(|q| baked_quad(rt, q)).collect()
    });
    crate::jutil::new_list(rt, items)
}

/// A host model's particle sprite (`getParticleIcon`), the missing sprite when it has none.
pub fn handle_particle(rt: &mut Rt, model: &Model) -> V {
    let name = rt.host().and_then(|h| h.model_particle(model)).unwrap_or_else(|| "minecraft:missingno".into());
    crate::render::sprite_named(rt, &name)
}

/// A `BakedQuad` of `q`, its vertices as vanilla's `FaceBakery` writes them (white, the face's
/// normal, its emission as light).
pub fn baked_quad(rt: &mut Rt, q: &Quad) -> V {
    let normal = pack_normal(face_normal(&q.positions, q.facing));
    let e = i32::from(q.light_emission.min(15));
    let light = e << 4 | e << 20;
    let bits = |f: f32| V::I(f.to_bits() as i32);
    // Its UVs in the atlas: across its sprite's cell.
    let sprite = crate::render::sprite_named(rt, &q.sprite);
    let (u0, v0) = crate::render::sprite_origin(rt, sprite).unwrap_or_default();
    let cell = crate::render::CELL;
    let mut ints = Vec::with_capacity(4 * STRIDE);
    for k in 0..4 {
        let [x, y, z] = q.positions[k];
        let [u, v] = q.uv[k];
        ints.extend([bits(x), bits(y), bits(z), V::I(abgr(q.colors[k]) as i32), bits(u0 + u * cell), bits(v0 + v * cell), V::I(light), V::I(normal)]);
    }
    let vertices = rt.alloc("[I", Data::Array(ints));
    let direction = crate::api_common::direction(rt, i64::from(q.facing));
    let o = rt.new_object(BAKED_QUAD);
    for (field, v) in [
        ("vertices", vertices),
        ("tintIndex", V::I(q.tint_index)),
        ("direction", direction),
        ("sprite", sprite),
        ("shade", V::I(q.shade as i32)),
        ("hasAmbientOcclusion", V::I(1)),
    ] {
        rt.put_field(o, field, v);
    }
    o
}

/// A `BakedQuad` as the face it draws, listed under `cull`: the sprite its UVs are in, which a mod
/// may have moved them to ([`crate::render::locate`]). None when it has no four vertices.
pub fn read_quad(rt: &Rt, quad: V, cull: Option<u8>) -> Option<Quad> {
    let vertices = rt.get_field(quad, "vertices");
    let Some(Data::Array(ints)) = rt.heap.get(vertices.r() as usize).map(|o| &o.data) else {
        return None;
    };
    if ints.len() < 4 * STRIDE {
        return None;
    }
    let int = |k: usize, i: usize| ints[k * STRIDE + i].i();
    let float = |k: usize, i: usize| f32::from_bits(int(k, i) as u32);
    let atlas_uv = std::array::from_fn(|k| [float(k, 4), float(k, 5)]);
    let (sprite, uv) = crate::render::locate(rt, rt.get_field(quad, "sprite"), &atlas_uv);
    let direction = rt.get_field(quad, "direction");
    Some(Quad {
        positions: std::array::from_fn(|k| [float(k, 0), float(k, 1), float(k, 2)]),
        uv,
        sprite: rt.str_of(rt.get_field(sprite, "$name")).unwrap_or("minecraft:missingno").to_owned(),
        facing: if direction.r() == 0 { 1 } else { crate::api_common::direction_ordinal(rt, direction) },
        cull,
        tint_index: rt.get_field(quad, "tintIndex").i(),
        shade: !matches!(rt.get_field(quad, "shade"), V::I(0)),
        // Block light the face lights itself with (`QuadTransformers.settingEmissivity`).
        light_emission: (0..4).map(|k| (int(k, 6) & 0xFFFF) >> 4 & 15).max().unwrap_or(0) as u8,
        colors: std::array::from_fn(|k| abgr(int(k, 3) as u32)),
    })
}

/// What catnip's `createForBlock` buffers of `model` drawn as `state`: the model itself when the game
/// has it, else its faces (`getQuads` for each side and none, as `ModelBlockRenderer` asks) read once
/// into a handle the game draws. The model itself when it has none.
pub fn drawable(rt: &mut Rt, model: V, state: V) -> V {
    if model.r() == 0 || handle(rt, model).is_some() {
        return model;
    }
    let Some(vc) = rt.vcall else { return model };
    let random = rt.new_object("net/minecraft/util/RandomSource");
    let data = crate::api::get_static(rt, "net/neoforged/neoforge/client/model/data/ModelData.EMPTY");
    let mut quads = Vec::new();
    for side in [None, Some(0u8), Some(1), Some(2), Some(3), Some(4), Some(5)] {
        let dir = side.map_or(V::NULL, |s| crate::api_common::direction(rt, i64::from(s)));
        let list = vc(rt, crate::render::GET_QUADS_KEY, crate::render::GET_QUADS, &[model, state, dir, random, data, V::NULL]);
        if rt.thrown.is_some() {
            rt.report_thrown("a model's getQuads, buffering it");
            return model;
        }
        for q in crate::jutil::elements(rt, list) {
            quads.extend(read_quad(rt, q, side));
        }
    }
    if quads.is_empty() {
        return model;
    }
    let Ok(mut meshes) = MESHES.lock() else { return model };
    meshes.push(quads.into());
    let id = meshes.len() as i32;
    drop(meshes);
    let h = rt.new_object(BAKED_MODEL);
    rt.put_field(h, "$mesh", V::I(id));
    h
}

/// 0xAARRGGBB as the vertex data's bytes (R, G, B, A in memory), and back: the swap is its own inverse.
fn abgr(c: u32) -> u32 {
    c & 0xFF00_FF00 | (c >> 16 & 0xFF) | (c & 0xFF) << 16
}

/// The face's own normal (NeoForge's `ClientHooks.fillNormal`: `(p2 - p0) x (p3 - p1)`), its
/// side's when the corners are degenerate.
fn face_normal(p: &[[f32; 3]; 4], facing: u8) -> [f32; 3] {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (a, b) = (sub(p[2], p[0]), sub(p[3], p[1]));
    let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 1e-6 {
        return n.map(|v| v / len);
    }
    match facing {
        0 => [0.0, -1.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        2 => [0.0, 0.0, -1.0],
        3 => [0.0, 0.0, 1.0],
        4 => [-1.0, 0.0, 0.0],
        _ => [1.0, 0.0, 0.0],
    }
}

/// A normal as `BLOCK`'s normal element: three signed bytes, x lowest.
fn pack_normal(n: [f32; 3]) -> i32 {
    let b = |v: f32| ((v.clamp(-1.0, 1.0) * 127.0).round() as i8 as u8) as i32;
    b(n[0]) | b(n[1]) << 8 | b(n[2]) << 16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;
    use crate::render::{GET_QUADS_KEY, sprite_named};

    const GET_SPRITE: &str = "net/minecraft/client/renderer/block/model/BakedQuad.getSprite.()Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
    const SPRITE_U: [&str; 3] = [
        "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU0.()F",
        "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU1.()F",
        "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU.(F)F",
    ];
    const SPRITE_V: [&str; 3] = [
        "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV0.()F",
        "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV1.()F",
        "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV.(F)F",
    ];
    const QUAD_INIT: &str = "net/minecraft/client/renderer/block/model/BakedQuad.<init>.([IILnet/minecraft/core/Direction;Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;Z)V";
    const SIMPLE_INIT: &str = "net/minecraft/client/resources/model/SimpleBakedModel.<init>.(Ljava/util/List;Ljava/util/Map;ZZZLnet/minecraft/client/renderer/texture/TextureAtlasSprite;Lnet/minecraft/client/renderer/block/model/ItemTransforms;Lnet/minecraft/client/renderer/block/model/ItemOverrides;)V";

    fn face(sprite: &str, facing: u8, cull: Option<u8>) -> Quad {
        Quad {
            positions: [[0.0, 0.25, 0.0], [0.0, 0.25, 1.0], [1.0, 0.5, 1.0], [1.0, 0.5, 0.0]],
            uv: [[0.0, 0.0], [0.0, 1.0], [0.5, 1.0], [0.5, 0.0]],
            sprite: sprite.into(),
            facing,
            cull,
            tint_index: -1,
            shade: true,
            light_emission: 0,
            colors: [u32::MAX, 0xFF80_4020, u32::MAX, u32::MAX],
        }
    }

    fn model(rt: &mut Rt, field: &'static str, name: &str) -> V {
        let m = rt.new_object(BAKED_MODEL);
        let n = rt.string(name);
        rt.put_field(m, field, n);
        m
    }

    fn list(rt: &mut Rt, model: V, side: Option<i64>) -> Vec<V> {
        let side = side.map_or(V::NULL, |s| crate::api_common::direction(rt, s));
        let l = crate::api::call(rt, GET_QUADS_KEY, &[model, V::NULL, side, V::NULL, V::NULL, V::NULL]);
        crate::jutil::elements(rt, l)
    }

    /// Create's water wheel (`WaterWheelRenderer.generateModel`, `BakedModelHelper.swapSprites`):
    /// the template's faces on oak planks copied onto the sprite the wheel's wood shows on top,
    /// put in a `SimpleBakedModel`, which catnip's buffer then draws.
    #[test]
    fn a_mod_retextures_a_models_faces_and_its_buffer_draws_them() {
        let mut rt = Rt::new();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let mut host = MemoryHost::default();
        let template = face("minecraft:block/oak_planks", 0, None);
        host.models.insert("create:block/water_wheel/wheel".into(), (vec![template.clone(), face("create:block/axis", 1, Some(1))], "minecraft:block/oak_planks".into()));
        host.models.insert("minecraft:spruce_planks".into(), (vec![face("minecraft:block/spruce_planks", 1, Some(1))], "minecraft:block/spruce_planks".into()));
        rt.with_host(&mut host, |rt| {
            // The sprite on the planks' top, the same object the atlas hands out for it.
            let planks = model(rt, "$state", "minecraft:spruce_planks");
            let top = list(rt, planks, Some(1))[0];
            let spruce = crate::api::call(rt, GET_SPRITE, &[top]);
            assert_eq!(spruce, sprite_named(rt, "minecraft:block/spruce_planks"));
            assert!(list(rt, planks, None).is_empty());
            let wheel = model(rt, "$name", "create:block/water_wheel/wheel");
            let faces = list(rt, wheel, None);
            assert_eq!(faces.len(), 1);
            let q = faces[0];
            assert_eq!(read_quad(rt, q, None), Some(template.clone()));
            // `BakedQuadHelper.clone`: the same sprite, the vertices copied. Then each corner goes to
            // the same place in the new sprite's part of the atlas (`SpriteShiftEntry
            // .getUnInterpolatedU` in the old one, `getU` in the new), the quad's sprite left as it was.
            let oak = rt.get_field(q, "sprite");
            let vertices = rt.get_field(q, "vertices");
            let copy = crate::api::call(rt, "java/util/Arrays.copyOf.([II)[I", &[vertices, V::I(32)]);
            let swapped = rt.new_object(BAKED_QUAD);
            let (tint, dir) = (rt.get_field(q, "tintIndex"), rt.get_field(q, "direction"));
            crate::api::call(rt, QUAD_INIT, &[swapped, copy, tint, dir, oak, V::I(1)]);
            let f = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args).f();
            for k in 0..4 {
                for (at, [lo, hi, to]) in [(4, SPRITE_U), (5, SPRITE_V)] {
                    let i = k * STRIDE + at;
                    let Data::Array(ints) = &rt.heap[copy.r() as usize].data else { panic!("no vertices") };
                    let was = f32::from_bits(ints[i].i() as u32);
                    let (lo, hi) = (f(rt, lo, &[oak]), f(rt, hi, &[oak]));
                    let moved = f(rt, to, &[spruce, V::F((was - lo) / (hi - lo))]);
                    let Data::Array(ints) = &mut rt.heap[copy.r() as usize].data else { panic!("no vertices") };
                    ints[i] = V::I(moved.to_bits() as i32);
                }
            }
            assert_ne!(rt.get_field(swapped, "sprite"), spruce);
            let unculled = crate::jutil::new_list(rt, vec![swapped]);
            let culled = rt.alloc("java/util/EnumMap", Data::Table(Default::default()));
            let built = rt.new_object(SIMPLE_MODEL);
            crate::api::call(rt, SIMPLE_INIT, &[built, unculled, culled, V::I(1), V::I(1), V::I(1), spruce, V::NULL, V::NULL]);
            assert_eq!(list(rt, built, None), vec![swapped]);
            assert!(list(rt, built, Some(1)).is_empty());
            // catnip's `createForBlock`: the faces read once, drawn as they are.
            let drawn = drawable(rt, built, V::NULL);
            let Some(Model::Quads(read)) = handle(rt, drawn) else { panic!("not drawable") };
            assert_eq!(read.to_vec(), vec![Quad { sprite: "minecraft:block/spruce_planks".into(), ..template }]);
            // The game's own models stay what they are.
            assert_eq!(drawable(rt, wheel, V::NULL), wheel);
        });
    }

    #[test]
    fn a_builder_lists_faces_by_the_side_that_hides_them() {
        const BUILDER_INIT: &str = "net/minecraft/client/resources/model/SimpleBakedModel$Builder.<init>.(ZZZLnet/minecraft/client/renderer/block/model/ItemTransforms;Lnet/minecraft/client/renderer/block/model/ItemOverrides;)V";
        let mut rt = Rt::new();
        let b = rt.new_object(SIMPLE_BUILDER);
        crate::api::call(&mut rt, BUILDER_INIT, &[b, V::I(0), V::I(1), V::I(0), V::NULL, V::NULL]);
        let (a, c) = (baked_quad(&mut rt, &face("minecraft:block/stone", 2, None)), baked_quad(&mut rt, &face("minecraft:block/dirt", 2, Some(2))));
        let north = crate::api_common::direction(&mut rt, 2);
        crate::api::call(&mut rt, "net/minecraft/client/resources/model/SimpleBakedModel$Builder.addUnculledFace.(Lnet/minecraft/client/renderer/block/model/BakedQuad;)Lnet/minecraft/client/resources/model/SimpleBakedModel$Builder;", &[b, a]);
        crate::api::call(&mut rt, "net/minecraft/client/resources/model/SimpleBakedModel$Builder.addCulledFace.(Lnet/minecraft/core/Direction;Lnet/minecraft/client/renderer/block/model/BakedQuad;)Lnet/minecraft/client/resources/model/SimpleBakedModel$Builder;", &[b, north, c]);
        let m = crate::api::call(&mut rt, "net/minecraft/client/resources/model/SimpleBakedModel$Builder.build.()Lnet/minecraft/client/resources/model/BakedModel;", &[b]);
        assert_eq!(list(&mut rt, m, None), vec![a]);
        assert_eq!(list(&mut rt, m, Some(2)), vec![c]);
        assert!(list(&mut rt, m, Some(3)).is_empty());
        let ao = crate::api::call(&mut rt, "net/minecraft/client/resources/model/BakedModel.useAmbientOcclusion.()Z", &[m]);
        assert_eq!(ao, V::I(0));
    }
}
