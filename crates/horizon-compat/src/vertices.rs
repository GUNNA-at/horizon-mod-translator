//! Vertices a mod writes one at a time into the world's buffers (`MultiBufferSource.getBuffer(type)`
//! in a block entity's or an entity's renderer: a fluid's surface in a tank, a beam, a glowing
//! face), drawn as vanilla's `BufferBuilder` draws them: in a `QUADS` type every four vertices are
//! a quad, which the host draws as a model face where the renderer's pose put it (triangles, strips
//! and fans likewise; lines aren't drawn).
//!
//! A face's sprite is the one its consumer was wrapped in (`Material.buffer`,
//! `TextureAtlasSprite.wrap`: UVs 0 to 1 across the sprite); else, for a type on the block atlas
//! (`translucent()`, `entitySolid(atlas)`, a mod's own type with `BLOCK_SHEET_MIPPED`), the sprite
//! whose cell its UVs are in (`sprite.getU` gives a UV in the sprite's cell, see `render`); else its
//! type's texture; else white (an untextured type: `lightning()`, `debugQuads()`).
use crate::host::{Model, Quad};
use crate::{Data, Rt, V};
use std::sync::Arc;

/// Values a vertex: x, y, z, u, v, colour (`0xAARRGGBB`), light (`LightTexture.pack`).
const VERTEX: usize = 7;
/// The consumers with vertices not drawn yet.
const PENDING: &str = "compat:pending-consumers";
/// The game's white sprite, which an untextured face is drawn from.
const WHITE_SPRITE: &str = "horizon/white";
const WHITE: i32 = -1;
const BLOCK_ATLAS: &str = "minecraft:textures/atlas/blocks.png";
/// The block atlas's render types, which name no texture of their own.
const BLOCK_TYPES: [&str; 7] = ["solid", "cutout_mipped", "cutout", "translucent", "tripwire", "translucent_moving_block", "translucent_no_crumbling"];
const CONSUMER: &str = "Lcom/mojang/blaze3d/vertex/VertexConsumer;";
const POSE: &str = "Lcom/mojang/blaze3d/vertex/PoseStack$Pose;";
const BAKED_QUAD: &str = "Lnet/minecraft/client/renderer/block/model/BakedQuad;";
const TEXTURE_SHARD: &str = "net/minecraft/client/renderer/RenderStateShard$TextureStateShard";

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

/// How a type's vertices make faces (`VertexFormat.Mode`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Faces {
    /// Every four vertices a quad: nearly every type.
    Quads = 1,
    /// Every three a triangle (`dragonRays()`).
    Triangles,
    /// Each vertex after the second a triangle with the two before it (`leash()`).
    Strip,
    /// Each vertex after the second a triangle with the first and the one before it.
    Fan,
    /// Lines (`lines()`, `lineStrip()`, the debug lines): not drawn.
    Lines,
}

impl Faces {
    /// The vertices of a face.
    fn size(self) -> usize {
        if self == Faces::Quads { 4 } else { 3 }
    }
}

/// How `consumer`'s type makes faces: the mode a mod's own type was made with
/// (`RenderType.create(name, format, mode, ...)`), else vanilla's type's. Kept on the consumer.
fn faces(rt: &mut Rt, consumer: V) -> Faces {
    const ALL: [Faces; 5] = [Faces::Quads, Faces::Triangles, Faces::Strip, Faces::Fan, Faces::Lines];
    if let V::I(k) = rt.get_field(consumer, "$faces") {
        return ALL[(k as usize).clamp(1, 5) - 1];
    }
    let ty = rt.get_field(consumer, "$type");
    let mode = rt.get_field(ty, "$mode");
    let f = match rt.str_of(rt.get_field(mode, "$name")) {
        Some("QUADS") => Faces::Quads,
        Some("TRIANGLES") => Faces::Triangles,
        Some("TRIANGLE_STRIP") => Faces::Strip,
        Some("TRIANGLE_FAN") => Faces::Fan,
        Some(_) => Faces::Lines,
        None => match rt.str_of(rt.get_field(ty, "$name")).unwrap_or("") {
            "lines" | "line_strip" | "debug_line_strip" | "debug_lines" | "debug_line" => Faces::Lines,
            "leash" | "debug_filled_box" => Faces::Strip,
            "dragon_rays" | "dragon_rays_depth" => Faces::Triangles,
            _ => Faces::Quads,
        },
    };
    rt.put_field(consumer, "$faces", V::I(f as i32));
    f
}

/// The consumer's vertices so far, in a list of its own.
fn list(rt: &mut Rt, consumer: V) -> V {
    match rt.get_field(consumer, "$verts") {
        l if l.r() != 0 => l,
        _ => {
            let l = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
            rt.put_field(consumer, "$verts", l);
            l
        }
    }
}

fn values(rt: &mut Rt, list: V) -> Option<&mut Vec<V>> {
    match rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(l)) if list.r() != 0 => Some(l),
        _ => None,
    }
}

/// A point under a pose matrix (column-major).
fn transform(m: &[f32; 16], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]]
}

/// `addVertex`: a vertex begins at `at` (white, unlit until set). The face before it, whose values
/// are all set now, is drawn.
fn begin(rt: &mut Rt, consumer: V, at: [f32; 3]) {
    let faces = faces(rt, consumer);
    if faces == Faces::Lines {
        let ty = rt.get_field(consumer, "$type");
        let name = rt.str_of(rt.get_field(ty, "$name")).unwrap_or("?").to_owned();
        crate::glue::trace_once(rt, &format!("lines/{name}"), |_| format!("client: lines drawn into {name} aren't drawn"));
        return;
    }
    let l = list(rt, consumer);
    let pending = values(rt, l).map_or(0, |v| v.len() / VERTEX);
    if pending >= faces.size() {
        draw_face(rt, consumer, faces, false);
    } else if pending == 0 {
        mark_pending(rt, consumer);
    }
    if let Some(v) = values(rt, l) {
        v.extend([V::F(at[0]), V::F(at[1]), V::F(at[2]), V::F(0.0), V::F(0.0), V::I(WHITE), V::I(0)]);
    }
}

fn mark_pending(rt: &mut Rt, consumer: V) {
    let pending = rt.intern(PENDING.into(), |rt| rt.alloc("java/util/ArrayList", Data::List(Vec::new())));
    if let Some(p) = values(rt, pending)
        && !p.contains(&consumer)
    {
        p.push(consumer);
    }
}

/// Sets values `at` onward (3 the UV, 5 the colour, 6 the light) of the consumer's last vertex.
fn set_last(rt: &mut Rt, consumer: V, at: usize, set: &[V]) {
    let l = rt.get_field(consumer, "$verts");
    if let Some(v) = values(rt, l)
        && v.len() >= VERTEX
    {
        let start = v.len() - VERTEX + at;
        v[start..start + set.len()].copy_from_slice(set);
    }
}

fn argb(r: i32, g: i32, b: i32, a: i32) -> i32 {
    let c = |v: i32| v.clamp(0, 255) as u32;
    (c(a) << 24 | c(r) << 16 | c(g) << 8 | c(b)) as i32
}

/// The world consumers' vertex calls (a consumer of a render type's, maybe wrapped in a sprite);
/// None for any other consumer, and for the calls that change nothing drawn here (overlay,
/// normal), which chain as vanilla's do.
pub(crate) fn call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let c = a(0);
    if rt.get_field(c, "$type").r() == 0 {
        return None;
    }
    let (params, tail) = desc.strip_prefix('(')?.split_once(')')?;
    match (name, params) {
        ("addVertex", "FFF") => begin(rt, c, [a(1).f(), a(2).f(), a(3).f()]),
        ("addVertex", "Lorg/joml/Vector3f;") => {
            let v = ["x", "y", "z"].map(|f| rt.get_field(a(1), f).f());
            begin(rt, c, v);
        }
        ("addVertex", "Lorg/joml/Matrix4f;FFF") => {
            let m = crate::render::mat4(rt, a(1));
            begin(rt, c, transform(&m, [a(2).f(), a(3).f(), a(4).f()]));
        }
        ("addVertex", p) if p.strip_prefix(POSE) == Some("FFF") => {
            let m = crate::render::mat4(rt, rt.get_field(a(1), "pose"));
            begin(rt, c, transform(&m, [a(2).f(), a(3).f(), a(4).f()]));
        }
        ("addVertex", p) if p.strip_prefix(POSE) == Some("Lorg/joml/Vector3f;") => {
            let m = crate::render::mat4(rt, rt.get_field(a(1), "pose"));
            let v = ["x", "y", "z"].map(|f| rt.get_field(a(2), f).f());
            begin(rt, c, transform(&m, v));
        }
        // The whole vertex at once: position, colour, UV, overlay, light, normal.
        ("addVertex", "FFFIFFIIFFF") => {
            begin(rt, c, [a(1).f(), a(2).f(), a(3).f()]);
            set_last(rt, c, 3, &[V::F(a(5).f()), V::F(a(6).f()), V::I(a(4).i()), V::I(a(8).i())]);
        }
        ("setColor", "IIII") => set_last(rt, c, 5, &[V::I(argb(a(1).i(), a(2).i(), a(3).i(), a(4).i()))]),
        // Vanilla's: each `(int) (f * 255.0F)`.
        ("setColor", "FFFF") => {
            let f = |v: V| (v.f() * 255.0) as i32;
            set_last(rt, c, 5, &[V::I(argb(f(a(1)), f(a(2)), f(a(3)), f(a(4))))]);
        }
        ("setColor", "I") => set_last(rt, c, 5, &[V::I(a(1).i())]),
        ("setWhiteAlpha", "I") => set_last(rt, c, 5, &[V::I(argb(255, 255, 255, a(1).i()))]),
        ("setUv", "FF") => set_last(rt, c, 3, &[V::F(a(1).f()), V::F(a(2).f())]),
        ("setLight", "I") => set_last(rt, c, 6, &[V::I(a(1).i())]),
        // `setUv2(block, sky)`: the light's two halves.
        ("setUv2", "II") => set_last(rt, c, 6, &[V::I(a(1).i() & 0xFFFF | a(2).i() << 16)]),
        ("putBulkData", p) if p.starts_with(&format!("{POSE}{BAKED_QUAD}")) => bulk(rt, args, &p[POSE.len() + BAKED_QUAD.len()..]),
        _ => return None,
    }
    Some(if tail == CONSUMER { c } else { V::Void })
}

/// `putBulkData(pose, quad, ...)`: a baked face under the pose, tinted by the colour given (a float
/// each, red first; times each corner's brightness when given) and lit by the light given (one, or
/// the first corner's).
fn bulk(rt: &mut Rt, args: &[V], rest: &str) {
    let a = |i| arg(args, i);
    let Some(mut quad) = crate::quads::read_quad(rt, a(2), None) else { return };
    // (r, g, b, a, light, overlay[, readExistingColor]) or (brightness[], r, g, b, a, lightmap[],
    // overlay, readAlpha).
    let (rgb, brightness, light) = if rest.starts_with("[F") {
        let lights = crate::jutil::elements(rt, a(8));
        let shades = crate::jutil::elements(rt, a(3));
        let shade = |k: usize| shades.get(k).map_or(1.0, |s| s.f());
        ([a(4).f(), a(5).f(), a(6).f()], [0, 1, 2, 3].map(shade), lights.first().map_or(0, |l| l.i()))
    } else {
        ([a(3).f(), a(4).f(), a(5).f()], [1.0; 4], a(7).i())
    };
    for (k, color) in quad.colors.iter_mut().enumerate() {
        let channel = |c: f32, shift: u32| ((c * brightness[k]).clamp(0.0, 1.0) * 255.0) as u32 * (*color >> shift & 0xFF) / 255 << shift;
        *color = *color & 0xFF00_0000 | channel(rgb[0], 16) | channel(rgb[1], 8) | channel(rgb[2], 0);
    }
    quad.cull = None;
    let pose = crate::render::mat4(rt, rt.get_field(a(1), "pose"));
    if let Some(h) = rt.host_mut() {
        h.draw_model(&Model::Quads(Arc::from([quad])), &pose, u32::MAX, light as u32);
    }
}

/// The side a face looks to, from its corners' winding (the largest axis of its normal), as
/// `Direction`'s data value.
fn facing(p: &[[f32; 3]; 4]) -> u8 {
    let (a, b) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
    let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let [x, y, z] = n.map(f32::abs);
    if x >= y && x >= z && x > 0.0 {
        if n[0] > 0.0 { 5 } else { 4 }
    } else if z >= y && z > 0.0 {
        if n[2] > 0.0 { 3 } else { 2 }
    } else if n[1] < 0.0 {
        0
    } else {
        1
    }
}

/// The texture `ty` draws with: its own (`entityTranslucent(texture)`), or, for a mod's own type,
/// its state's (`setTextureState(BLOCK_SHEET_MIPPED)`, `new TextureStateShard(texture, ...)`).
fn texture_of(rt: &Rt, ty: V) -> Option<String> {
    if let Some(t) = rt.str_of(rt.get_field(ty, "$texture")) {
        return Some(t.to_owned());
    }
    let builder = rt.get_field(rt.get_field(ty, "$state"), "$builder");
    let shard = rt.get_field(builder, "$setTextureState");
    match rt.str_of(rt.get_field(shard, "$name")) {
        Some("BLOCK_SHEET" | "BLOCK_SHEET_MIPPED") => Some(BLOCK_ATLAS.to_owned()),
        _ if rt.class_of(shard) == TEXTURE_SHARD => Some(crate::neo::location(rt, rt.get_field(shard, "$arg1"))),
        _ => None,
    }
}

/// UVs past their texture's edge (a scrolling beam's): vanilla repeats the texture, which one
/// sprite of an atlas can't; moved back by whole textures, they're kept inside it.
fn inside(uv: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    let shift = [0, 1].map(|i| {
        let low = uv.iter().map(|c| c[i]).fold(f32::INFINITY, f32::min);
        if (-1e-3..1.0).contains(&low) { 0.0 } else { low.floor() }
    });
    uv.map(|[u, v]| [(u - shift[0]).clamp(0.0, 1.0), (v - shift[1]).clamp(0.0, 1.0)])
}

/// The sprite a face of `consumer` is drawn from and where each corner is in it (see the module's
/// comment); the white sprite, its corners' colours alone, when the game's atlas lacks that sprite
/// (a mod's `textures/special/`, an outline's blank texture). None when its UVs are in no sprite's
/// cell on the atlas.
fn sprite_and_uv(rt: &Rt, consumer: V, uv: [[f32; 2]; 4]) -> Option<(String, [[f32; 2]; 4])> {
    let (sprite, uv) = named_sprite_and_uv(rt, consumer, uv)?;
    if rt.host().is_some_and(|h| !h.has_sprite(&sprite)) {
        return Some((WHITE_SPRITE.to_owned(), [[0.5, 0.5]; 4]));
    }
    Some((sprite, uv))
}

fn named_sprite_and_uv(rt: &Rt, consumer: V, uv: [[f32; 2]; 4]) -> Option<(String, [[f32; 2]; 4])> {
    if let Some(sprite) = rt.str_of(rt.get_field(consumer, "$sprite")) {
        return Some((sprite.to_owned(), inside(uv)));
    }
    let ty = rt.get_field(consumer, "$type");
    let texture = texture_of(rt, ty);
    let on_atlas = match &texture {
        Some(t) => t.contains(":textures/atlas/"),
        None => rt.get_field(ty, "$state").r() == 0 && rt.str_of(rt.get_field(ty, "$name")).is_some_and(|n| BLOCK_TYPES.contains(&n)),
    };
    if on_atlas {
        let centre = [0, 1].map(|i| uv.iter().map(|c| c[i]).sum::<f32>() / 4.0);
        let sprite = crate::render::sprite_at(rt, centre[0], centre[1])?;
        let (u0, v0) = crate::render::sprite_origin(rt, sprite)?;
        let name = rt.str_of(rt.get_field(sprite, "$name"))?.to_owned();
        let cell = crate::render::CELL;
        return Some((name, inside(uv.map(|[u, v]| [(u - u0) / cell, (v - v0) / cell]))));
    }
    Some(match texture {
        Some(t) => (crate::models::sprite_of_texture(&t), inside(uv)),
        None => (WHITE_SPRITE.to_owned(), [[0.5, 0.5]; 4]),
    })
}

/// Draws the face of the consumer's first vertices (a triangle as a quad whose last two corners
/// are one), keeping those the next face shares (a strip's last two, a fan's first and last), or
/// none when `last`.
fn draw_face(rt: &mut Rt, consumer: V, faces: Faces, last: bool) {
    let l = rt.get_field(consumer, "$verts");
    let Some(v) = values(rt, l).map(std::mem::take) else { return };
    let n = v.len() / VERTEX;
    if n < faces.size() {
        return;
    }
    // A strip's every other triangle winds the other way: its first two swapped, it faces as the rest.
    let odd = faces == Faces::Strip && rt.get_field(consumer, "$strip").i() % 2 == 1;
    let order = match faces {
        Faces::Quads => [0, 1, 2, 3],
        _ if odd => [1, 0, 2, 2],
        _ => [0, 1, 2, 2],
    };
    let at = |k: usize, i: usize| v[order[k] * VERTEX + i];
    let positions: [[f32; 3]; 4] = std::array::from_fn(|k| [at(k, 0).f(), at(k, 1).f(), at(k, 2).f()]);
    let uv: [[f32; 2]; 4] = std::array::from_fn(|k| [at(k, 3).f(), at(k, 4).f()]);
    let colors: [u32; 4] = std::array::from_fn(|k| at(k, 5).i() as u32);
    let light = at(0, 6).i();
    let kept: Vec<V> = match faces {
        _ if last => Vec::new(),
        Faces::Strip => v[VERTEX..3 * VERTEX].to_vec(),
        Faces::Fan => v[..VERTEX].iter().chain(&v[2 * VERTEX..3 * VERTEX]).copied().collect(),
        _ => Vec::new(),
    };
    let strip = if faces == Faces::Strip && !last { rt.get_field(consumer, "$strip").i() + 1 } else { 0 };
    rt.put_field(consumer, "$strip", V::I(strip));
    if let Some(list) = values(rt, l) {
        *list = kept;
    }
    let Some((sprite, uv)) = sprite_and_uv(rt, consumer, uv) else { return };
    // A particle's face (the particle engine's buffer): the game draws it with its own particles.
    if rt.get_field(consumer, "$particles").i() != 0 {
        let translucent = rt.get_field(consumer, "$translucent").i() != 0;
        let quad = crate::host::ParticleQuad {
            positions,
            sprite,
            uv,
            colors,
            light: light as u32,
            translucent,
        };
        if let Some(h) = rt.host_mut() {
            h.particle_quad(&quad);
        }
        return;
    }
    let quad = Quad { positions, uv, sprite, facing: facing(&positions), cull: None, tint_index: -1, shade: false, light_emission: 0, colors };
    if let Some(h) = rt.host_mut() {
        h.draw_model(&Model::Quads(Arc::from([quad])), &crate::render::IDENTITY, u32::MAX, light as u32);
    }
}

/// Draws what every world consumer (or every one of render type `only`) has gathered: a renderer
/// is done, or a mod ended the batch. A face left unfinished is dropped.
pub(crate) fn flush(rt: &mut Rt, only: Option<V>) {
    let Some(&pending) = rt.interned.get(PENDING) else { return };
    let consumers = values(rt, pending).map(std::mem::take).unwrap_or_default();
    let mut kept = Vec::new();
    for c in consumers {
        if only.is_some_and(|ty| rt.get_field(c, "$type") != ty) {
            kept.push(c);
            continue;
        }
        let faces = faces(rt, c);
        draw_face(rt, c, faces, true);
        let l = rt.get_field(c, "$verts");
        if let Some(v) = values(rt, l) {
            v.clear();
        }
    }
    if let Some(p) = values(rt, pending) {
        *p = kept;
    }
}

/// A whole vertex at once (`addVertex(at).setUv(uv).setColor(argb).setLight(light)`): what a
/// particle's quad writes for each corner.
pub(crate) fn vertex(rt: &mut Rt, consumer: V, at: [f32; 3], uv: [f32; 2], argb: i32, light: i32) {
    begin(rt, consumer, at);
    set_last(
        rt,
        consumer,
        3,
        &[V::F(uv[0]), V::F(uv[1]), V::I(argb), V::I(light)],
    );
}

/// Forgets the vertices of `consumer`'s face that wasn't finished (what threw while drawing it).
pub(crate) fn drop_unfinished(rt: &mut Rt, consumer: V) {
    let size = faces(rt, consumer).size();
    let l = rt.get_field(consumer, "$verts");
    if let Some(v) = values(rt, l) {
        let n = v.len() / VERTEX;
        v.truncate((n - n % size) * VERTEX);
    }
}

/// Forgets the vertices written outside the world's hooks (a screen's, into its buffer source),
/// before a world hook writes its own.
pub(crate) fn discard(rt: &mut Rt) {
    let Some(&pending) = rt.interned.get(PENDING) else { return };
    for c in values(rt, pending).map(std::mem::take).unwrap_or_default() {
        let l = rt.get_field(c, "$verts");
        if let Some(v) = values(rt, l) {
            v.clear();
        }
        rt.put_field(c, "$strip", V::I(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const GET_BUFFER: &str = "net/minecraft/client/renderer/MultiBufferSource.getBuffer.(Lnet/minecraft/client/renderer/RenderType;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const LAST: &str = "com/mojang/blaze3d/vertex/PoseStack.last.()Lcom/mojang/blaze3d/vertex/PoseStack$Pose;";
    const ADD_POSE: &str = "com/mojang/blaze3d/vertex/VertexConsumer.addVertex.(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const SET_COLOR: &str = "com/mojang/blaze3d/vertex/VertexConsumer.setColor.(IIII)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const SET_UV: &str = "com/mojang/blaze3d/vertex/VertexConsumer.setUv.(FF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const SET_LIGHT: &str = "com/mojang/blaze3d/vertex/VertexConsumer.setLight.(I)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const SET_NORMAL: &str = "com/mojang/blaze3d/vertex/VertexConsumer.setNormal.(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const GET_U: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU.(F)F";
    const GET_V: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV.(F)F";
    const WRAP: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.wrap.(Lcom/mojang/blaze3d/vertex/VertexConsumer;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const LIGHT: i32 = 15 << 4 | 7 << 20;

    fn consumer(rt: &mut Rt, ty: V) -> V {
        let buffers = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        crate::api::call(rt, GET_BUFFER, &[buffers, ty])
    }

    /// A pose stack moved half a block up, its last pose.
    fn lifted_pose(rt: &mut Rt) -> V {
        let mut m = crate::render::IDENTITY;
        m[13] = 0.5;
        let stack = crate::render::new_pose_stack(rt, m);
        crate::api::call(rt, LAST, &[stack])
    }

    /// The square from (0, 0, 0) to (1, 0, 1), its corners wound to face up, each coloured, given
    /// UVs `uv(k)`, lit and given a normal as a fluid renderer writes them.
    fn write_square(rt: &mut Rt, c: V, pose: V, uv: impl Fn(&mut Rt, usize) -> [f32; 2]) {
        let corners = [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
        for (k, p) in corners.into_iter().enumerate() {
            let chained = crate::api::call(rt, ADD_POSE, &[c, pose, V::F(p[0]), V::F(p[1]), V::F(p[2])]);
            assert_eq!(chained, c);
            crate::api::call(rt, SET_COLOR, &[c, V::I(255), V::I(128), V::I(0), V::I(200)]);
            let [u, v] = uv(rt, k);
            crate::api::call(rt, SET_UV, &[c, V::F(u), V::F(v)]);
            crate::api::call(rt, SET_LIGHT, &[c, V::I(LIGHT)]);
            crate::api::call(rt, SET_NORMAL, &[c, pose, V::F(0.0), V::F(1.0), V::F(0.0)]);
        }
    }

    fn quads(host: &MemoryHost) -> Vec<(Quad, u32)> {
        host.drawn
            .iter()
            .flat_map(|(m, _, _, light)| match m {
                Model::Quads(q) => q.iter().map(|q| (q.clone(), *light)).collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// A fluid's surface as Create's `FluidRenderer` writes it: its still sprite from
    /// `Minecraft.getTextureAtlas(BLOCK_ATLAS).apply(texture)`, four vertices into a translucent
    /// buffer, each UV `sprite.getU/getV` of it. One face, lifted by the pose, facing up, drawn from
    /// the water's sprite at the fractions the mod asked for, in the colour and light it gave.
    #[test]
    fn four_vertices_on_atlas_uvs_are_a_face_of_their_sprite() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            crate::render::sprite_named(rt, "minecraft:block/lava_still");
            let mc = crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[]);
            let atlas_location = rt.string(BLOCK_ATLAS);
            let get_atlas = "net/minecraft/client/Minecraft.getTextureAtlas.(Lnet/minecraft/resources/ResourceLocation;)Ljava/util/function/Function;";
            let atlas = crate::api::call(rt, get_atlas, &[mc, atlas_location]);
            let still = rt.string("minecraft:block/water_still");
            // As the mod's `function.apply(still)` reaches an API object.
            let water = crate::api::call(rt, "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", &[atlas, still]);
            assert_eq!(water, crate::render::sprite_named(rt, "minecraft:block/water_still"));
            let ty = crate::render::render_type(rt, "translucent");
            let c = consumer(rt, ty);
            let pose = lifted_pose(rt);
            let fractions = [[0.0, 0.0], [0.0, 0.5], [0.5, 0.5], [0.5, 0.0]];
            write_square(rt, c, pose, |rt, k| {
                let u = crate::api::call(rt, GET_U, &[water, V::F(fractions[k][0])]).f();
                let v = crate::api::call(rt, GET_V, &[water, V::F(fractions[k][1])]).f();
                [u, v]
            });
            flush(rt, None);
        });
        let drawn = quads(&host);
        assert_eq!(drawn.len(), 1);
        let (q, light) = &drawn[0];
        assert_eq!(q.sprite, "minecraft:block/water_still");
        assert_eq!(q.positions, [[0.0, 0.5, 0.0], [0.0, 0.5, 1.0], [1.0, 0.5, 1.0], [1.0, 0.5, 0.0]]);
        for (got, want) in q.uv.iter().zip([[0.0, 0.0], [0.0, 0.5], [0.5, 0.5], [0.5, 0.0]]) {
            assert!((got[0] - want[0]).abs() < 1e-3 && (got[1] - want[1]).abs() < 1e-3, "{:?}", q.uv);
        }
        assert_eq!(q.facing, 1);
        assert_eq!(q.colors, [0xC8FF_8000; 4]);
        assert_eq!(*light, LIGHT as u32);
    }

    /// A consumer wrapped in a sprite draws from that sprite, its UVs 0 to 1 across it, the face
    /// drawn as soon as the next one starts; an untextured type's faces are white; a line type's
    /// vertices draw nothing; a face left unfinished is dropped.
    #[test]
    fn wrapped_untextured_and_line_consumers() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let wrapped = rt.with_host(&mut host, |rt| {
            let pose = lifted_pose(rt);
            let atlas = rt.string(BLOCK_ATLAS);
            let ty = crate::render::textured_render_type(rt, "entity_cutout", atlas);
            let plain = consumer(rt, ty);
            let sprite = crate::render::sprite_named(rt, "mymod:block/crate_lid");
            let wrapped = crate::api::call(rt, WRAP, &[sprite, plain]);
            write_square(rt, wrapped, pose, |_, k| [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]][k]);
            // The next face's first vertex: the square is whole.
            crate::api::call(rt, ADD_POSE, &[wrapped, pose, V::F(0.0), V::F(0.0), V::F(0.0)]);
            wrapped
        });
        assert_eq!(quads(&host).len(), 1);
        rt.with_host(&mut host, |rt| {
            let pose = lifted_pose(rt);
            let lightning = crate::render::render_type(rt, "lightning");
            let white = consumer(rt, lightning);
            write_square(rt, white, pose, |_, _| [0.0, 0.0]);
            let lines = crate::render::render_type(rt, "lines");
            let line = consumer(rt, lines);
            write_square(rt, line, pose, |_, _| [0.0, 0.0]);
            flush(rt, None);
            // Its unfinished face was dropped: three more vertices don't make one.
            for _ in 0..3 {
                crate::api::call(rt, ADD_POSE, &[wrapped, pose, V::F(0.0), V::F(0.0), V::F(0.0)]);
            }
            flush(rt, None);
        });
        let drawn = quads(&host);
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert_eq!(drawn[0].0.sprite, "mymod:block/crate_lid");
        assert_eq!(drawn[0].0.uv, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        assert_eq!(drawn[1].0.sprite, WHITE_SPRITE);
        assert_eq!(drawn[1].0.colors, [0xC8FF_8000; 4]);
    }

    /// A mod's own type made in `TRIANGLE_STRIP` mode on the block atlas (as a trail or ribbon
    /// renderer's): each vertex after the second draws a triangle with the two before it, every
    /// other one turned to wind as the first.
    #[test]
    fn a_strip_draws_a_triangle_a_vertex() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let stone = crate::render::sprite_named(rt, "minecraft:block/stone");
            let mode = crate::api::get_static(rt, "com/mojang/blaze3d/vertex/VertexFormat$Mode.TRIANGLE_STRIP");
            let builder = crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState.builder.()Lnet/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder;", &[]);
            let sheet = crate::api::get_static(rt, "net/minecraft/client/renderer/RenderStateShard.BLOCK_SHEET_MIPPED");
            crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder.setTextureState.(Lnet/minecraft/client/renderer/RenderStateShard$EmptyTextureStateShard;)Lnet/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder;", &[builder, sheet]);
            let state = crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder.createCompositeState.(Z)Lnet/minecraft/client/renderer/RenderType$CompositeState;", &[builder, V::I(0)]);
            let name = rt.string("mymod:trail");
            let create = "net/minecraft/client/renderer/RenderType.create.(Ljava/lang/String;Lcom/mojang/blaze3d/vertex/VertexFormat;Lcom/mojang/blaze3d/vertex/VertexFormat$Mode;IZZLnet/minecraft/client/renderer/RenderType$CompositeState;)Lnet/minecraft/client/renderer/RenderType$CompositeRenderType;";
            let ty = crate::api::call(rt, create, &[name, V::NULL, mode, V::I(256), V::I(0), V::I(0), state]);
            let c = consumer(rt, ty);
            let pose = lifted_pose(rt);
            let u = crate::api::call(rt, GET_U, &[stone, V::F(0.5)]).f();
            let v = crate::api::call(rt, GET_V, &[stone, V::F(0.5)]).f();
            // A ribbon up the north face: bottom west, top west, bottom east, top east.
            for p in [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]] {
                crate::api::call(rt, ADD_POSE, &[c, pose, V::F(p[0]), V::F(p[1]), V::F(p[2])]);
                crate::api::call(rt, SET_UV, &[c, V::F(u), V::F(v)]);
            }
            flush(rt, None);
        });
        let drawn = quads(&host);
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert!(drawn.iter().all(|(q, _)| q.sprite == "minecraft:block/stone"), "{drawn:?}");
        assert_eq!(drawn[0].0.positions[..3], [[0.0, 0.5, 0.0], [0.0, 1.5, 0.0], [1.0, 0.5, 0.0]]);
        assert_eq!(drawn[1].0.positions[..3], [[1.0, 0.5, 0.0], [0.0, 1.5, 0.0], [1.0, 1.5, 0.0]]);
        assert_eq!(drawn[0].0.facing, drawn[1].0.facing);
    }

    /// A mod's render type that reads the block sheet through `RenderType`, the class it extends
    /// (catnip's `PonderRenderTypes.fluid()`, which Create's tanks draw their fluid with): its faces
    /// are drawn from the sprite their UVs are in, not white.
    #[test]
    fn a_type_reading_the_sheet_through_render_type_draws_its_sprites() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let water = crate::render::sprite_named(rt, "minecraft:block/water_still");
            let mode = crate::api::get_static(rt, "com/mojang/blaze3d/vertex/VertexFormat$Mode.QUADS");
            let builder = crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState.builder.()Lnet/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder;", &[]);
            let sheet = crate::api::get_static(rt, "net/minecraft/client/renderer/RenderType.BLOCK_SHEET_MIPPED");
            crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder.setTextureState.(Lnet/minecraft/client/renderer/RenderStateShard$EmptyTextureStateShard;)Lnet/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder;", &[builder, sheet]);
            let state = crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder.createCompositeState.(Z)Lnet/minecraft/client/renderer/RenderType$CompositeState;", &[builder, V::I(1)]);
            let name = rt.string("ponder:fluid");
            let create = "net/minecraft/client/renderer/RenderType.create.(Ljava/lang/String;Lcom/mojang/blaze3d/vertex/VertexFormat;Lcom/mojang/blaze3d/vertex/VertexFormat$Mode;IZZLnet/minecraft/client/renderer/RenderType$CompositeState;)Lnet/minecraft/client/renderer/RenderType$CompositeRenderType;";
            let ty = crate::api::call(rt, create, &[name, V::NULL, mode, V::I(256), V::I(0), V::I(1), state]);
            let c = consumer(rt, ty);
            let pose = lifted_pose(rt);
            write_square(rt, c, pose, |rt, k| {
                let [u, v] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]][k];
                [crate::api::call(rt, GET_U, &[water, V::F(u)]).f(), crate::api::call(rt, GET_V, &[water, V::F(v)]).f()]
            });
            flush(rt, None);
        });
        let drawn = quads(&host);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].0.sprite, "minecraft:block/water_still");
        assert_eq!(drawn[0].0.uv, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
    }

    /// A mod's own buffer source (`MultiBufferSource.immediate(new ByteBufferBuilder(256))`, as
    /// catnip's outline buffers make theirs) hands out consumers that draw what is written into them
    /// when its batch ends.
    #[test]
    fn a_mods_own_buffer_source_draws_when_its_batch_ends() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let memory = rt.new_object("com/mojang/blaze3d/vertex/ByteBufferBuilder");
            crate::api::call(rt, "com/mojang/blaze3d/vertex/ByteBufferBuilder.<init>.(I)V", &[memory, V::I(256)]);
            let immediate = "net/minecraft/client/renderer/MultiBufferSource.immediate.(Lcom/mojang/blaze3d/vertex/ByteBufferBuilder;)Lnet/minecraft/client/renderer/MultiBufferSource$BufferSource;";
            let source = crate::api::call(rt, immediate, &[memory]);
            let ty = crate::render::render_type(rt, "lightning");
            let get_buffer = "net/minecraft/client/renderer/MultiBufferSource$BufferSource.getBuffer.(Lnet/minecraft/client/renderer/RenderType;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
            let c = crate::api::call(rt, get_buffer, &[source, ty]);
            let pose = lifted_pose(rt);
            write_square(rt, c, pose, |_, _| [0.0, 0.0]);
            crate::api::call(rt, "net/minecraft/client/renderer/MultiBufferSource$BufferSource.endBatch.()V", &[source]);
        });
        let drawn = quads(&host);
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert_eq!(drawn[0].0.sprite, WHITE_SPRITE);
    }

    /// Create's nixie tubes end their text's batch after each digit they draw
    /// (`endBatch(font.getFontSet(DEFAULT_FONT).whiteGlyph().renderType(NORMAL))`): that ends the
    /// text's type only, and a face gathered for another type (lightning's, untextured) waits for
    /// its own batch's end.
    #[test]
    fn ending_the_text_batch_leaves_the_other_types_faces() {
        const END_TYPE: &str = "net/minecraft/client/renderer/MultiBufferSource$BufferSource.endBatch.(Lnet/minecraft/client/renderer/RenderType;)V";
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let source = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        let lightning = crate::render::render_type(&mut rt, "lightning");
        rt.with_host(&mut host, |rt| {
            let c = consumer(rt, lightning);
            let pose = lifted_pose(rt);
            write_square(rt, c, pose, |_, _| [0.0, 0.0]);
            let font = rt.new_object("net/minecraft/client/gui/Font");
            let default = rt.string("minecraft:default");
            let get_font_set = "net/minecraft/client/gui/Font.getFontSet.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/gui/font/FontSet;";
            let set = crate::api::call(rt, get_font_set, &[font, default]);
            let white = crate::api::call(rt, "net/minecraft/client/gui/font/FontSet.whiteGlyph.()Lnet/minecraft/client/gui/font/glyphs/BakedGlyph;", &[set]);
            let normal = crate::api::api_enum(rt, "net/minecraft/client/gui/Font$DisplayMode", "NORMAL", 0);
            let render_type = "net/minecraft/client/gui/font/glyphs/BakedGlyph.renderType.(Lnet/minecraft/client/gui/Font$DisplayMode;)Lnet/minecraft/client/renderer/RenderType;";
            let text = crate::api::call(rt, render_type, &[white, normal]);
            crate::api::call(rt, END_TYPE, &[source, text]);
        });
        assert!(quads(&host).is_empty());
        rt.with_host(&mut host, |rt| {
            crate::api::call(rt, END_TYPE, &[source, lightning]);
        });
        assert_eq!(quads(&host).len(), 1);
    }

    /// A mod's own type drawn with a texture the game's atlas lacks (Create's outlines, on
    /// `create:textures/special/blank.png`) draws its faces white in their corners' colours, where
    /// the game would leave them out; a texture the atlas has is drawn from.
    #[test]
    fn a_texture_the_atlas_lacks_is_drawn_white_in_the_vertices_colours() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost { sprites: vec!["block/stone".into(), "mymod:entity/beam".into()], ..MemoryHost::default() };
        rt.with_host(&mut host, |rt| {
            let builder = crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState.builder.()Lnet/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder;", &[]);
            let shard = rt.new_object(TEXTURE_SHARD);
            let location = rt.string("create:textures/special/blank.png");
            crate::api::call(rt, "net/minecraft/client/renderer/RenderStateShard$TextureStateShard.<init>.(Lnet/minecraft/resources/ResourceLocation;ZZ)V", &[shard, location, V::I(0), V::I(0)]);
            crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder.setTextureState.(Lnet/minecraft/client/renderer/RenderStateShard$EmptyTextureStateShard;)Lnet/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder;", &[builder, shard]);
            let state = crate::api::call(rt, "net/minecraft/client/renderer/RenderType$CompositeState$CompositeStateBuilder.createCompositeState.(Z)Lnet/minecraft/client/renderer/RenderType$CompositeState;", &[builder, V::I(0)]);
            let name = rt.string("create:outline_solid");
            let create = "net/minecraft/client/renderer/RenderType.create.(Ljava/lang/String;Lcom/mojang/blaze3d/vertex/VertexFormat;Lcom/mojang/blaze3d/vertex/VertexFormat$Mode;IZZLnet/minecraft/client/renderer/RenderType$CompositeState;)Lnet/minecraft/client/renderer/RenderType$CompositeRenderType;";
            let outline = crate::api::call(rt, create, &[name, V::NULL, V::NULL, V::I(256), V::I(0), V::I(0), state]);
            let pose = lifted_pose(rt);
            let c = consumer(rt, outline);
            write_square(rt, c, pose, |_, k| [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]][k]);
            let beam_texture = rt.string("mymod:textures/entity/beam.png");
            let beam = crate::render::textured_render_type(rt, "entity_translucent", beam_texture);
            let c = consumer(rt, beam);
            write_square(rt, c, pose, |_, k| [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]][k]);
            flush(rt, None);
        });
        let drawn = quads(&host);
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert_eq!(drawn[0].0.sprite, WHITE_SPRITE);
        assert_eq!(drawn[0].0.colors, [0xC8FF_8000; 4]);
        assert_eq!(drawn[1].0.sprite, "mymod:entity/beam");
        assert_eq!(drawn[1].0.uv, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
    }
}
