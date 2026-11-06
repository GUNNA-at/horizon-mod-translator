//! Entity models as vanilla builds and draws them, natively: a mod's layer definitions
//! (`CubeListBuilder`, `CubeDeformation`, `PartPose`, `PartDefinition`, `MeshDefinition`,
//! `LayerDefinition`), registered for its model layers (`EntityRenderersEvent
//! .RegisterLayerDefinitions`) and baked into `ModelPart`s when a renderer asks for one
//! (`EntityModelSet.bakeLayer`, a renderer context's `bakeLayer`); a part's pose, children and
//! cubes; and its drawing (`ModelPart.render`): each cube's faces laid out on the texture as
//! vanilla's `ModelPart.Cube` lays them, under the pose stack's pose, drawn by the game with the
//! texture of the render type its vertex consumer was asked for (`RenderType.entityCutoutNoCull
//! (texture)`: a mod's `textures/entity/x.png` is its `<ns>:entity/x` sprite).
//!
//! And the model classes a mod's model is built on (`Model`, `EntityModel`, `HierarchicalModel`):
//! the render type function, the fields a renderer sets (`attackTime`, `riding`, `young`), and a
//! hierarchical model's drawing of its root.
//!
//! A layer no mod registered (one of vanilla's, `ModelLayers.PLAYER`) bakes as an empty part whose
//! children are empty parts too: a renderer built on it draws nothing of it instead of failing.
//!
//! The objects keep vanilla's field names where a mod reads them (a part's `x`, `xRot`, `visible`;
//! a pose's; a deformation's `growX`), as [`crate::records`] makes the data classes among them
//! (`PartPose`, `CubeDeformation`, `LayerDefinition`, `ModelLayerLocation`).

use crate::host::{Model, Quad};
use crate::{Data, Rt, Table, V, jutil};
use std::sync::Arc;

const CUBE_LIST: &str = "net/minecraft/client/model/geom/builders/CubeListBuilder";
const CUBE_DEFINITION: &str = "net/minecraft/client/model/geom/builders/CubeDefinition";
const DEFORMATION: &str = "net/minecraft/client/model/geom/builders/CubeDeformation";
const PART_DEFINITION: &str = "net/minecraft/client/model/geom/builders/PartDefinition";
const MESH: &str = "net/minecraft/client/model/geom/builders/MeshDefinition";
const LAYER_DEFINITION: &str = "net/minecraft/client/model/geom/builders/LayerDefinition";
const MATERIAL: &str = "net/minecraft/client/model/geom/builders/MaterialDefinition";
const POSE: &str = "net/minecraft/client/model/geom/PartPose";
const PART: &str = "net/minecraft/client/model/geom/ModelPart";
const CUBE: &str = "net/minecraft/client/model/geom/ModelPart$Cube";
const MODEL_SET: &str = "net/minecraft/client/model/geom/EntityModelSet";
pub(crate) const REGISTER_LAYERS: &str = "net/neoforged/neoforge/client/event/EntityRenderersEvent$RegisterLayerDefinitions";
const MODEL: &str = "net/minecraft/client/model/Model";
const ENTITY_MODEL: &str = "net/minecraft/client/model/EntityModel";
const HIERARCHICAL: &str = "net/minecraft/client/model/HierarchicalModel";
const SUPPLIER_GET: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const FUNCTION_APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const PUSH: &str = "com/mojang/blaze3d/vertex/PoseStack.pushPose.()V";
const POP: &str = "com/mojang/blaze3d/vertex/PoseStack.popPose.()V";
const LAST: &str = "com/mojang/blaze3d/vertex/PoseStack.last.()Lcom/mojang/blaze3d/vertex/PoseStack$Pose;";
const TRANSLATE: &str = "com/mojang/blaze3d/vertex/PoseStack.translate.(FFF)V";
const MUL_POSE: &str = "com/mojang/blaze3d/vertex/PoseStack.mulPose.(Lorg/joml/Quaternionf;)V";
const SCALE: &str = "com/mojang/blaze3d/vertex/PoseStack.scale.(FFF)V";
const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const RENDER_5: &str = "renderToBuffer.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;III)V";
const RENDER_5_KEY: &str = "net/minecraft/client/model/Model.renderToBuffer.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;III)V";
const ROOT: &str = "root.()Lnet/minecraft/client/model/geom/ModelPart;";
const ROOT_KEY: &str = "net/minecraft/client/model/HierarchicalModel.root.()Lnet/minecraft/client/model/geom/ModelPart;";
const VISIT: &str = "net/minecraft/client/model/geom/ModelPart$Visitor.visit.(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;Ljava/lang/String;ILnet/minecraft/client/model/geom/ModelPart$Cube;)V";
const DEFORMATION_DESC: &str = "Lnet/minecraft/client/model/geom/builders/CubeDeformation;";

/// `Direction`'s data values, each a bit of a cube's visible faces.
const DOWN: u8 = 0;
const UP: u8 = 1;
const NORTH: u8 = 2;
const SOUTH: u8 = 3;
const WEST: u8 = 4;
const EAST: u8 = 5;
const ALL_FACES: i32 = 0b11_1111;

/// White, a model's colour unless the renderer tints it (`-1`).
const WHITE: i32 = -1;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

/// Float field `field` of `o`, 0 when unset.
fn float(rt: &Rt, o: V, field: &'static str) -> f32 {
    match rt.get_field(o, field) {
        V::I(i) => i as f32,
        v => v.f(),
    }
}

/// A scale field, 1 when unset.
fn scale(rt: &Rt, o: V, field: &'static str) -> f32 {
    match rt.get_field(o, field) {
        V::Void => 1.0,
        v => v.f(),
    }
}

fn put_floats(rt: &mut Rt, o: V, fields: &[(&'static str, f32)]) {
    for &(f, v) in fields {
        rt.put_field(o, f, V::F(v));
    }
}

fn new_map(rt: &mut Rt) -> V {
    rt.alloc("java/util/HashMap", Data::Table(Table::default()))
}

/// The parameter types of method descriptor `desc` (`F`, `Ljava/util/Set;`).
fn params(desc: &str) -> Vec<&str> {
    let inner = desc.strip_prefix('(').and_then(|d| d.split_once(')')).map_or("", |(p, _)| p);
    let mut out = Vec::new();
    let mut rest = inner;
    while !rest.is_empty() {
        let dims = rest.len() - rest.trim_start_matches('[').len();
        let len = match rest.as_bytes()[dims] {
            b'L' => rest.find(';').map_or(rest.len(), |e| e + 1),
            _ => dims + 1,
        };
        out.push(&rest[..len]);
        rest = &rest[len..];
    }
    out
}

// ---- builders ----

const POSE_FIELDS: [&str; 6] = ["x", "y", "z", "xRot", "yRot", "zRot"];

/// `PartPose.offsetAndRotation(x, y, z, xRot, yRot, zRot)`.
fn part_pose(rt: &mut Rt, p: [f32; 6]) -> V {
    let o = rt.new_object(POSE);
    for (f, v) in POSE_FIELDS.into_iter().zip(p) {
        rt.put_field(o, f, V::F(v));
    }
    o
}

/// `PartPose.ZERO`: one object.
fn zero_pose(rt: &mut Rt) -> V {
    rt.intern("compat:part-pose-zero".into(), |rt| part_pose(rt, [0.0; 6]))
}

/// A pose's offset and rotation (all 0 for null).
fn pose_of(rt: &Rt, pose: V) -> [f32; 6] {
    POSE_FIELDS.map(|f| float(rt, pose, f))
}

/// `new CubeDeformation(growX, growY, growZ)`.
fn deformation(rt: &mut Rt, grow: [f32; 3]) -> V {
    let o = rt.new_object(DEFORMATION);
    put_floats(rt, o, &[("growX", grow[0]), ("growY", grow[1]), ("growZ", grow[2])]);
    o
}

fn grow_of(rt: &Rt, d: V) -> [f32; 3] {
    ["growX", "growY", "growZ"].map(|f| float(rt, d, f))
}

/// A `CubeListBuilder` as `create()` makes it: no cubes, texture offset (0, 0), unmirrored.
fn init_cube_list(rt: &mut Rt, o: V) {
    let cubes = jutil::new_list(rt, Vec::new());
    rt.put_field(o, "$cubes", cubes);
    for f in ["$u", "$v", "$mirror"] {
        rt.put_field(o, f, V::I(0));
    }
}

/// A cube as `CubeDefinition` keeps it and `ModelPart.Cube` draws it: its texture offset, its
/// corner and size, how far it grows each way, whether it is mirrored, its texture's size (for a
/// definition its scale, `(1, 1)` unless the mod scales it) and its visible faces.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CubeShape {
    uv: [f32; 2],
    origin: [f32; 3],
    size: [f32; 3],
    grow: [f32; 3],
    mirror: bool,
    tex: [f32; 2],
    faces: i32,
}

const SHAPE_FIELDS: [&str; 13] = ["$u", "$v", "$ox", "$oy", "$oz", "$dx", "$dy", "$dz", "$gx", "$gy", "$gz", "$tw", "$th"];

fn put_shape(rt: &mut Rt, o: V, s: &CubeShape) {
    let values = [s.uv[0], s.uv[1], s.origin[0], s.origin[1], s.origin[2], s.size[0], s.size[1], s.size[2], s.grow[0], s.grow[1], s.grow[2], s.tex[0], s.tex[1]];
    for (f, v) in SHAPE_FIELDS.into_iter().zip(values) {
        rt.put_field(o, f, V::F(v));
    }
    rt.put_field(o, "$mirror", V::I(i32::from(s.mirror)));
    rt.put_field(o, "$faces", V::I(s.faces));
}

fn shape_of(rt: &Rt, o: V) -> CubeShape {
    let v = SHAPE_FIELDS.map(|f| float(rt, o, f));
    CubeShape {
        uv: [v[0], v[1]],
        origin: [v[2], v[3], v[4]],
        size: [v[5], v[6], v[7]],
        grow: [v[8], v[9], v[10]],
        mirror: rt.get_field(o, "$mirror").i() != 0,
        tex: [v[11], v[12]],
        faces: match rt.get_field(o, "$faces") {
            V::I(f) => f,
            _ => ALL_FACES,
        },
    }
}

/// A `Set<Direction>`'s faces as bits of their data values.
fn faces_of(rt: &Rt, set: V) -> i32 {
    jutil::elements(rt, set).into_iter().fold(0, |bits, d| bits | 1 << crate::api_common::direction_ordinal(rt, d))
}

/// `CubeListBuilder.addBox`'s overloads: `[comment,] x, y, z, w, h, d` (the size as floats, or as
/// ints with a comment), then nothing, a deformation, a mirror, visible faces, a deformation and
/// texture scales, or (with a comment) a texture offset the builder takes. The cube takes the
/// builder's texture offset and mirror.
fn add_box(rt: &mut Rt, desc: &str, args: &[V]) -> Option<()> {
    let a = |i| arg(args, i);
    let this = a(0);
    let ps = params(desc);
    let named = ps.first() == Some(&"Ljava/lang/String;");
    let first = 1 + usize::from(named);
    let ints = ps.get(usize::from(named) + 3) == Some(&"I");
    let num = |v: V| if ints { v.i() as f32 } else { v.f() };
    let origin = [a(first).f(), a(first + 1).f(), a(first + 2).f()];
    let size = [num(a(first + 3)), num(a(first + 4)), num(a(first + 5))];
    let tail = ps.get(usize::from(named) + 6..).unwrap_or_default().concat();
    let at = first + 6;
    let mut mirror = rt.get_field(this, "$mirror").i() != 0;
    let (mut grow, mut tex, mut faces) = ([0.0; 3], [1.0, 1.0], ALL_FACES);
    let deformed = format!("{DEFORMATION_DESC}II");
    let scaled = format!("{DEFORMATION_DESC}FF");
    match tail.as_str() {
        "" => {}
        t if t == DEFORMATION_DESC => grow = grow_of(rt, a(at)),
        t if t == deformed => {
            grow = grow_of(rt, a(at));
            rt.put_field(this, "$u", a(at + 1));
            rt.put_field(this, "$v", a(at + 2));
        }
        "II" => {
            rt.put_field(this, "$u", a(at));
            rt.put_field(this, "$v", a(at + 1));
        }
        "Z" => mirror = a(at).i() != 0,
        "Ljava/util/Set;" => faces = faces_of(rt, a(at)),
        t if t == scaled => {
            grow = grow_of(rt, a(at));
            tex = [a(at + 1).f(), a(at + 2).f()];
        }
        _ => return None,
    }
    let shape = CubeShape {
        uv: [rt.get_field(this, "$u").i() as f32, rt.get_field(this, "$v").i() as f32],
        origin,
        size,
        grow,
        mirror,
        tex,
        faces,
    };
    let def = rt.new_object(CUBE_DEFINITION);
    put_shape(rt, def, &shape);
    let cubes = rt.get_field(this, "$cubes");
    jutil::list_add(rt, cubes, def);
    Some(())
}

/// A new `PartDefinition` of `cubes` (`CubeDefinition`s) at `pose`, with no children yet.
fn part_definition(rt: &mut Rt, cubes: Vec<V>, pose: V) -> V {
    let o = rt.new_object(PART_DEFINITION);
    let list = jutil::new_list(rt, cubes);
    let children = new_map(rt);
    rt.put_field(o, "$cubes", list);
    rt.put_field(o, "$pose", pose);
    rt.put_field(o, "$children", children);
    o
}

/// `PartDefinition.addOrReplaceChild(name, cubes, pose)`: the new child keeps the children of
/// the one it replaces.
fn add_or_replace_child(rt: &mut Rt, parent: V, name: V, builder: V, pose: V) -> V {
    let cubes = jutil::elements(rt, rt.get_field(builder, "$cubes"));
    let child = part_definition(rt, cubes, pose);
    let children = rt.get_field(parent, "$children");
    let old = jutil::table_put(rt, children, name, child);
    if old.r() != 0 {
        let mine = rt.get_field(child, "$children");
        for (k, v) in jutil::table_pairs(rt, rt.get_field(old, "$children")) {
            jutil::table_put(rt, mine, k, v);
        }
    }
    child
}

/// `new MeshDefinition()`: its root, a part with no cubes at `PartPose.ZERO`.
fn init_mesh(rt: &mut Rt, o: V) {
    let zero = zero_pose(rt);
    let root = part_definition(rt, Vec::new(), zero);
    rt.put_field(o, "$root", root);
}

/// `LayerDefinition.create(mesh, width, height)`: the mesh and its texture's size, in the fields
/// vanilla's constructor fills.
fn layer_definition(rt: &mut Rt, mesh: V, width: V, height: V) -> V {
    let material = rt.new_object(MATERIAL);
    rt.put_field(material, "xTexSize", width);
    rt.put_field(material, "yTexSize", height);
    let o = rt.new_object(LAYER_DEFINITION);
    rt.put_field(o, "mesh", mesh);
    rt.put_field(o, "material", material);
    o
}

// ---- baking ----

/// `PartDefinition.bake(width, height)`: the part, its cubes laid out on a `width` by `height`
/// texture, its children baked the same way, at its pose (its initial pose too).
fn bake(rt: &mut Rt, def: V, width: i32, height: i32) -> V {
    let children = new_map(rt);
    for (name, child) in jutil::table_pairs(rt, rt.get_field(def, "$children")) {
        let baked = bake(rt, child, width, height);
        jutil::table_put(rt, children, name, baked);
    }
    let mut cubes = Vec::new();
    for c in jutil::elements(rt, rt.get_field(def, "$cubes")) {
        let mut s = shape_of(rt, c);
        // CubeDefinition.bake: the offset as ints, the texture's size times the cube's scale.
        s.uv = [s.uv[0].trunc(), s.uv[1].trunc()];
        s.tex = [width as f32 * s.tex[0], height as f32 * s.tex[1]];
        cubes.push(new_cube(rt, &s));
    }
    let part = rt.new_object(PART);
    init_part(rt, part, cubes, children);
    let pose = match rt.get_field(def, "$pose") {
        V::R(0) | V::Void => zero_pose(rt),
        p => p,
    };
    rt.put_field(part, "initialPose", pose);
    let p = pose_of(rt, pose);
    load_pose(rt, part, p);
    part
}

/// A `ModelPart.Cube`: its bounds as vanilla keeps them (`minX`... ungrown) and the shape its
/// faces are made of when drawn.
fn new_cube(rt: &mut Rt, s: &CubeShape) -> V {
    let c = rt.new_object(CUBE);
    put_cube(rt, c, s);
    c
}

fn put_cube(rt: &mut Rt, c: V, s: &CubeShape) {
    put_shape(rt, c, s);
    let [x, y, z] = s.origin;
    let [w, h, d] = s.size;
    put_floats(rt, c, &[("minX", x), ("minY", y), ("minZ", z), ("maxX", x + w), ("maxY", y + h), ("maxZ", z + d)]);
}

/// `new ModelPart(cubes, children)` on `part`: at the origin, unrotated, unscaled, visible, its
/// initial pose `PartPose.ZERO`.
fn init_part(rt: &mut Rt, part: V, cubes: Vec<V>, children: V) {
    let list = jutil::new_list(rt, cubes);
    rt.put_field(part, "$cubes", list);
    rt.put_field(part, "$children", children);
    load_pose(rt, part, [0.0; 6]);
    rt.put_field(part, "visible", V::I(1));
    rt.put_field(part, "skipDraw", V::I(0));
    let zero = zero_pose(rt);
    rt.put_field(part, "initialPose", zero);
}

/// An empty part: what a layer nobody registered bakes to, and what such a part's children are.
fn empty_part(rt: &mut Rt) -> V {
    let part = rt.new_object(PART);
    let children = new_map(rt);
    init_part(rt, part, Vec::new(), children);
    rt.put_field(part, "$hollow", V::I(1));
    part
}

/// `ModelPart.loadPose(pose)`: its offset and rotation, its scale 1.
fn load_pose(rt: &mut Rt, part: V, p: [f32; 6]) {
    for (f, v) in POSE_FIELDS.into_iter().zip(p) {
        rt.put_field(part, f, V::F(v));
    }
    put_floats(rt, part, &[("xScale", 1.0), ("yScale", 1.0), ("zScale", 1.0)]);
}

/// `LayerDefinition.bakeRoot()`.
fn bake_root(rt: &mut Rt, layer: V) -> V {
    let mesh = rt.get_field(layer, "mesh");
    let root = rt.get_field(mesh, "$root");
    if root.r() == 0 {
        return empty_part(rt);
    }
    let material = rt.get_field(layer, "material");
    let (w, h) = (rt.get_field(material, "xTexSize").i(), rt.get_field(material, "yTexSize").i());
    bake(rt, root, w, h)
}

/// A model layer's name as `ModelLayerLocation.toString` gives it, `ns:path#layer`.
fn layer_name(rt: &Rt, location: V) -> String {
    let model = crate::neo::location(rt, rt.get_field(location, "model"));
    let layer = rt.str_of(rt.get_field(location, "layer")).unwrap_or("main");
    format!("{model}#{layer}")
}

/// `EntityModelSet.bakeLayer(location)`: the layer definition its mod registered for it, baked
/// afresh (each renderer gets parts of its own). A layer nobody registered (vanilla's own) bakes as
/// an empty part.
pub fn bake_layer(rt: &mut Rt, location: V) -> V {
    let name = layer_name(rt, location);
    let Some(supplier) = rt.interned.get(&format!("layer/{name}")).copied() else {
        crate::glue::trace_once(rt, &format!("no-layer/{name}"), |_| format!("client: no layer definition for {name}: drawn empty"));
        return empty_part(rt);
    };
    let layer = rt.call_functional(SUPPLIER_GET, supplier, &[]);
    if rt.thrown.is_some() || layer.r() == 0 {
        rt.report_thrown("a layer definition");
        return empty_part(rt);
    }
    bake_root(rt, layer)
}

// ---- drawing ----

/// A cube's faces as `ModelPart.Cube` lays them out, in pixels: each face's direction (its data
/// value; on a mirrored cube east and west swap, as its normal's x turns) and its four corners, each
/// a position and its place on the texture (0 to 1 across it), in vanilla's order.
fn cube_faces(s: &CubeShape) -> Vec<(u8, [([f32; 3], [f32; 2]); 4])> {
    let [ox, oy, oz] = s.origin;
    let [dx, dy, dz] = s.size;
    let [gx, gy, gz] = s.grow;
    let (mut x0, y0, z0) = (ox - gx, oy - gy, oz - gz);
    let (mut x1, y1, z1) = (ox + dx + gx, oy + dy + gy, oz + dz + gz);
    if s.mirror {
        std::mem::swap(&mut x0, &mut x1);
    }
    // The corners as vanilla numbers them (`vertex7` is the one at the origin).
    let v7 = [x0, y0, z0];
    let v0 = [x1, y0, z0];
    let v1 = [x1, y1, z0];
    let v2 = [x0, y1, z0];
    let v3 = [x0, y0, z1];
    let v4 = [x1, y0, z1];
    let v5 = [x1, y1, z1];
    let v6 = [x0, y1, z1];
    let [u, v] = s.uv;
    let (u0, u1, u2, u3) = (u, u + dz, u + dz + dx, u + dz + dx + dx);
    let (u4, u5) = (u + dz + dx + dz, u + dz + dx + dz + dx);
    let (w0, w1, w2) = (v, v + dz, v + dz + dy);
    // Each face's corners and its rectangle on the texture (u1, v1, u2, v2).
    let faces: [(u8, [[f32; 3]; 4], [f32; 4]); 6] = [
        (DOWN, [v4, v3, v7, v0], [u1, w0, u2, w1]),
        (UP, [v1, v2, v6, v5], [u2, w1, u3, w0]),
        (WEST, [v7, v3, v6, v2], [u0, w1, u1, w2]),
        (NORTH, [v0, v7, v2, v1], [u1, w1, u2, w2]),
        (EAST, [v4, v0, v1, v5], [u2, w1, u4, w2]),
        (SOUTH, [v3, v4, v5, v6], [u4, w1, u5, w2]),
    ];
    let [tw, th] = s.tex;
    faces
        .into_iter()
        .filter(|(d, _, _)| s.faces & 1 << d != 0)
        .map(|(d, p, [a, b, c, e])| {
            // ModelPart.Polygon: each corner's place, then on a mirrored cube their order reversed.
            let mut corners = [(p[0], [c / tw, b / th]), (p[1], [a / tw, b / th]), (p[2], [a / tw, e / th]), (p[3], [c / tw, e / th])];
            let facing = match d {
                WEST if s.mirror => EAST,
                EAST if s.mirror => WEST,
                other => other,
            };
            if s.mirror {
                corners.reverse();
            }
            (facing, corners)
        })
        .collect()
}

/// The sprite a texture is drawn from: `ns:textures/entity/x.png` is `ns:entity/x` (vanilla's
/// without its namespace, as the game names its own).
pub(crate) fn sprite_of_texture(texture: &str) -> String {
    let (ns, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
    let path = path.strip_prefix("textures/").unwrap_or(path);
    let path = path.strip_suffix(".png").unwrap_or(path);
    if ns == "minecraft" { path.to_owned() } else { format!("{ns}:{path}") }
}

/// The sprite a vertex consumer draws with: the one it was wrapped in (`Material.buffer`,
/// `TextureAtlasSprite.wrap`), else its render type's texture. None when it has neither, or its
/// texture is an atlas (a model drawn on the whole atlas, with no sprite to map it into).
fn consumer_sprite(rt: &Rt, consumer: V) -> Option<String> {
    if let Some(sprite) = rt.str_of(rt.get_field(consumer, "$sprite")) {
        return Some(sprite.strip_prefix("minecraft:").unwrap_or(sprite).to_owned());
    }
    let ty = rt.get_field(consumer, "$type");
    let texture = rt.str_of(rt.get_field(ty, "$texture"))?;
    if texture.contains(":textures/atlas/") {
        return None;
    }
    Some(sprite_of_texture(texture))
}

/// Draws `cubes` under `pose` with `consumer`'s sprite, `color` (0xAARRGGBB) and packed `light`.
fn draw_cubes(rt: &mut Rt, cubes: &[V], pose: &crate::render::M4, consumer: V, light: i32, color: i32) {
    let Some(sprite) = consumer_sprite(rt, consumer) else {
        crate::glue::trace_once(rt, "model-no-texture", |_| "client: a model part drawn with no texture: left out".to_owned());
        return;
    };
    let mut quads = Vec::new();
    for &c in cubes {
        let s = shape_of(rt, c);
        for (facing, corners) in cube_faces(&s) {
            quads.push(Quad {
                positions: corners.map(|(p, _)| p.map(|x| x / 16.0)),
                uv: corners.map(|(_, uv)| uv),
                sprite: sprite.clone(),
                facing,
                cull: None,
                tint_index: -1,
                shade: true,
                light_emission: 0,
                colors: [u32::MAX; 4],
            });
        }
    }
    if quads.is_empty() {
        return;
    }
    let model = Model::Quads(Arc::from(quads));
    if let Some(h) = rt.host_mut() {
        h.draw_model(&model, pose, color as u32, light as u32);
    }
}

/// JOML's `Quaternionf.rotationZYX(z, y, x)` as `[x, y, z, w]`: about x first, then y, then z.
fn rotation_zyx(z: f32, y: f32, x: f32) -> [f32; 4] {
    let (sx, cx) = (x * 0.5).sin_cos();
    let (sy, cy) = (y * 0.5).sin_cos();
    let (sz, cz) = (z * 0.5).sin_cos();
    let (cycz, sysz, sycz, cysz) = (cy * cz, sy * sz, sy * cz, cy * sz);
    [sx * cycz - cx * sysz, cx * sycz + sx * cysz, cx * cysz - sx * sycz, cx * cycz + sx * sysz]
}

/// `ModelPart.translateAndRotate(poseStack)`: to its offset (in pixels), turned, scaled.
fn translate_and_rotate(rt: &mut Rt, part: V, stack: V) {
    let [x, y, z, xr, yr, zr] = pose_of(rt, part);
    crate::api::call(rt, TRANSLATE, &[stack, V::F(x / 16.0), V::F(y / 16.0), V::F(z / 16.0)]);
    if xr != 0.0 || yr != 0.0 || zr != 0.0 {
        let q = crate::render::new_quat(rt, rotation_zyx(zr, yr, xr));
        crate::api::call(rt, MUL_POSE, &[stack, q]);
    }
    let (sx, sy, sz) = (scale(rt, part, "xScale"), scale(rt, part, "yScale"), scale(rt, part, "zScale"));
    if sx != 1.0 || sy != 1.0 || sz != 1.0 {
        crate::api::call(rt, SCALE, &[stack, V::F(sx), V::F(sy), V::F(sz)]);
    }
}

fn children_of(rt: &Rt, part: V) -> Vec<(V, V)> {
    jutil::table_pairs(rt, rt.get_field(part, "$children"))
}

/// `ModelPart.render(poseStack, consumer, light, overlay, color)`: a visible part with cubes or
/// children, moved to its pose: its own cubes (unless `skipDraw`), then its children.
pub fn render_part(rt: &mut Rt, part: V, stack: V, consumer: V, light: i32, color: i32) {
    if part.r() == 0 || rt.get_field(part, "visible").i() == 0 {
        return;
    }
    let cubes = jutil::elements(rt, rt.get_field(part, "$cubes"));
    let children = children_of(rt, part);
    if cubes.is_empty() && children.is_empty() {
        return;
    }
    crate::api::call(rt, PUSH, &[stack]);
    translate_and_rotate(rt, part, stack);
    if rt.get_field(part, "skipDraw").i() == 0 && !cubes.is_empty() {
        let pose = crate::render::last_pose(rt, stack);
        draw_cubes(rt, &cubes, &pose, consumer, light, color);
    }
    for (_, child) in children {
        render_part(rt, child, stack, consumer, light, color);
    }
    crate::api::call(rt, POP, &[stack]);
}

/// `ModelPart.visit(poseStack, visitor)`: each cube with the pose it is drawn at, its part's path
/// (`/body/head`) and its index there.
fn visit(rt: &mut Rt, part: V, stack: V, visitor: V, path: &str) {
    let cubes = jutil::elements(rt, rt.get_field(part, "$cubes"));
    let children = children_of(rt, part);
    if cubes.is_empty() && children.is_empty() {
        return;
    }
    crate::api::call(rt, PUSH, &[stack]);
    translate_and_rotate(rt, part, stack);
    let pose = crate::api::call(rt, LAST, &[stack]);
    let p = rt.string(path);
    for (i, c) in cubes.into_iter().enumerate() {
        rt.call_functional(VISIT, visitor, &[pose, p, V::I(i as i32), c]);
    }
    for (name, child) in children {
        let sub = format!("{path}/{}", rt.str_of(name).unwrap_or(""));
        visit(rt, child, stack, visitor, &sub);
    }
    crate::api::call(rt, POP, &[stack]);
}

/// Every part of `part`'s tree, itself first (`getAllParts`).
fn all_parts(rt: &Rt, part: V, out: &mut Vec<V>) {
    out.push(part);
    for (_, c) in children_of(rt, part) {
        all_parts(rt, c, out);
    }
}

/// `ModelPart.getChild(name)`. Vanilla throws for a part it hasn't; here that part is an empty one
/// (told once, unless its parent is itself empty: a layer nobody registered), so a renderer built on
/// a layer it can't have still draws the rest.
fn child(rt: &mut Rt, part: V, name: V) -> V {
    let children = rt.get_field(part, "$children");
    if let Some(c) = jutil::table_get(rt, children, name) {
        return c;
    }
    if rt.get_field(part, "$hollow").i() == 0 {
        let n = rt.str_of(name).unwrap_or("?").to_owned();
        crate::glue::trace_once(rt, &format!("no-part/{n}"), |_| format!("client: a model has no part {n}: drawn empty"));
    }
    let empty = empty_part(rt);
    jutil::table_put(rt, children, name, empty);
    empty
}

fn part_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let vec3 = |rt: &Rt, v: V| ["x", "y", "z"].map(|f| float(rt, v, f));
    Some(match name {
        "<init>" => {
            let cubes = jutil::elements(rt, a(1));
            init_part(rt, this, cubes, a(2));
            V::Void
        }
        "storePose" => {
            let p = pose_of(rt, this);
            part_pose(rt, p)
        }
        "getInitialPose" => match rt.get_field(this, "initialPose") {
            V::R(0) | V::Void => zero_pose(rt),
            p => p,
        },
        "setInitialPose" => {
            rt.put_field(this, "initialPose", a(1));
            V::Void
        }
        "resetPose" => {
            let p = pose_of(rt, rt.get_field(this, "initialPose"));
            load_pose(rt, this, p);
            V::Void
        }
        "loadPose" => {
            let p = pose_of(rt, a(1));
            load_pose(rt, this, p);
            V::Void
        }
        "copyFrom" => {
            for f in ["xScale", "yScale", "zScale", "xRot", "yRot", "zRot", "x", "y", "z"] {
                let v = rt.get_field(a(1), f);
                rt.put_field(this, f, v);
            }
            V::Void
        }
        "hasChild" => {
            let children = rt.get_field(this, "$children");
            V::I(i32::from(jutil::table_get(rt, children, a(1)).is_some()))
        }
        "getChild" => child(rt, this, a(1)),
        "setPos" => {
            put_floats(rt, this, &[("x", a(1).f()), ("y", a(2).f()), ("z", a(3).f())]);
            V::Void
        }
        "setRotation" => {
            put_floats(rt, this, &[("xRot", a(1).f()), ("yRot", a(2).f()), ("zRot", a(3).f())]);
            V::Void
        }
        "render" => {
            let color = if desc.ends_with("III)V") { a(5).i() } else { WHITE };
            render_part(rt, this, a(1), a(2), a(3).i(), color);
            V::Void
        }
        "visit" => {
            visit(rt, this, a(1), a(2), "");
            V::Void
        }
        "translateAndRotate" => {
            translate_and_rotate(rt, this, a(1));
            V::Void
        }
        "getRandomCube" => {
            let cubes = jutil::elements(rt, rt.get_field(this, "$cubes"));
            let i = crate::api::call(rt, NEXT_INT, &[a(1), V::I(cubes.len().max(1) as i32)]).i();
            cubes.get(i.max(0) as usize).copied().unwrap_or(V::NULL)
        }
        "isEmpty" => V::I(i32::from(jutil::elements(rt, rt.get_field(this, "$cubes")).is_empty())),
        "offsetPos" | "offsetRotation" | "offsetScale" => {
            let by = vec3(rt, a(1));
            let fields = match name {
                "offsetPos" => ["x", "y", "z"],
                "offsetRotation" => ["xRot", "yRot", "zRot"],
                _ => ["xScale", "yScale", "zScale"],
            };
            for (f, d) in fields.into_iter().zip(by) {
                let now = if name == "offsetScale" { scale(rt, this, f) } else { float(rt, this, f) };
                rt.put_field(this, f, V::F(now + d));
            }
            V::Void
        }
        "getAllParts" => {
            let mut parts = Vec::new();
            all_parts(rt, this, &mut parts);
            rt.alloc("java/util/stream/Stream", Data::Stream(parts))
        }
        _ => return None,
    })
}

/// `Model`'s, `EntityModel`'s and `HierarchicalModel`'s own methods, on a mod's model.
fn model_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match name {
        // `Model(renderType)`. An entity model made without one draws with
        // `RenderType::entityCutoutNoCull` (kept as null); it starts young, unridden, its attack
        // not begun.
        "<init>" => {
            let f = if desc == "(Ljava/util/function/Function;)V" { a(1) } else { V::NULL };
            rt.put_field(this, "renderType", f);
            if owner != MODEL {
                rt.put_field(this, "attackTime", V::F(0.0));
                rt.put_field(this, "riding", V::I(0));
                rt.put_field(this, "young", V::I(1));
            }
            V::Void
        }
        "renderType" => render_type_of(rt, this, a(1)),
        // The final four-argument form: the five-argument one in white, the mod's own where its
        // model has it.
        "renderToBuffer" if desc.ends_with("II)V") && !desc.ends_with("III)V") => {
            crate::api::virtual_call(rt, (RENDER_5_KEY, RENDER_5), &[this, a(1), a(2), a(3), a(4), V::I(WHITE)]);
            V::Void
        }
        // A hierarchical model draws its root, a list model its parts; another model's is abstract
        // (its mod draws it).
        "renderToBuffer" => {
            if rt.instance_of(this, HIERARCHICAL) {
                let root = crate::api::virtual_call(rt, (ROOT_KEY, ROOT), &[this]);
                render_part(rt, root, a(1), a(2), a(3).i(), a(5).i());
            } else if rt.instance_of(this, AGEABLE_LIST) {
                render_ageable(rt, this, a(1), a(2), a(3).i(), a(5).i());
            } else if rt.instance_of(this, LIST_MODEL) {
                for p in parts_of(rt, this, (PARTS_KEY, PARTS)) {
                    render_part(rt, p, a(1), a(2), a(3).i(), a(5).i());
                }
            }
            V::Void
        }
        "prepareMobModel" | "setupAnim" => V::Void,
        "copyPropertiesTo" => {
            for f in ["attackTime", "riding", "young"] {
                let v = rt.get_field(this, f);
                rt.put_field(a(1), f, v);
            }
            V::Void
        }
        "getAnyDescendantWithName" => {
            let root = crate::api::virtual_call(rt, (ROOT_KEY, ROOT), &[this]);
            let found = if rt.str_of(a(1)) == Some("root") {
                root
            } else {
                let mut parts = Vec::new();
                all_parts(rt, root, &mut parts);
                let mut found = V::NULL;
                for p in parts {
                    let children = rt.get_field(p, "$children");
                    if let Some(c) = jutil::table_get(rt, children, a(1)) {
                        found = c;
                        break;
                    }
                }
                found
            };
            crate::api_common::optional(rt, found)
        }
        _ => return None,
    })
}

/// `Model.renderType(texture)`: the model's render type function applied to it,
/// `entityCutoutNoCull` for an entity model made without one.
fn render_type_of(rt: &mut Rt, model: V, texture: V) -> V {
    match rt.get_field(model, "renderType") {
        V::R(0) | V::Void => crate::render::textured_render_type(rt, "entity_cutout_no_cull", texture),
        f => rt.call_functional(FUNCTION_APPLY, f, &[texture]),
    }
}

// ---- vanilla's model classes mods build on: the list models, the humanoid, the zombie ----

const AGEABLE_LIST: &str = "net/minecraft/client/model/AgeableListModel";
const LIST_MODEL: &str = "net/minecraft/client/model/ListModel";
const HUMANOID: &str = "net/minecraft/client/model/HumanoidModel";
const ABSTRACT_ZOMBIE: &str = "net/minecraft/client/model/AbstractZombieModel";
const ZOMBIE: &str = "net/minecraft/client/model/ZombieModel";
const ARM_POSE: &str = "net/minecraft/client/model/HumanoidModel$ArmPose";
const ANIMATION_UTILS: &str = "net/minecraft/client/model/AnimationUtils";
const HEAD_PARTS: &str = "headParts.()Ljava/lang/Iterable;";
const HEAD_PARTS_KEY: &str = "net/minecraft/client/model/AgeableListModel.headParts.()Ljava/lang/Iterable;";
const BODY_PARTS: &str = "bodyParts.()Ljava/lang/Iterable;";
const BODY_PARTS_KEY: &str = "net/minecraft/client/model/AgeableListModel.bodyParts.()Ljava/lang/Iterable;";
const PARTS: &str = "parts.()Ljava/lang/Iterable;";
const PARTS_KEY: &str = "net/minecraft/client/model/ListModel.parts.()Ljava/lang/Iterable;";
const IS_AGGRESSIVE: &str = "isAggressive.(Lnet/minecraft/world/entity/monster/Monster;)Z";
const IS_AGGRESSIVE_KEY: &str = "net/minecraft/client/model/AbstractZombieModel.isAggressive.(Lnet/minecraft/world/entity/monster/Monster;)Z";
const MOB_IS_AGGRESSIVE: (&str, &str) = ("net/minecraft/world/entity/Mob.isAggressive.()Z", "isAggressive.()Z");
const MAIN_ARM: &str = "getMainArm.()Lnet/minecraft/world/entity/HumanoidArm;";
const MAIN_ARM_KEY: &str = "net/minecraft/world/entity/LivingEntity.getMainArm.()Lnet/minecraft/world/entity/HumanoidArm;";
/// `HumanoidModel`'s parts: its fields, and the root's children they are.
const HUMANOID_PARTS: [(&str, &str); 7] =
    [("head", "head"), ("hat", "hat"), ("body", "body"), ("rightArm", "right_arm"), ("leftArm", "left_arm"), ("rightLeg", "right_leg"), ("leftLeg", "left_leg")];
/// `AgeableListModel`'s settings for a young one: whether its head is scaled, how far its head moves
/// down and back, how much smaller its head and body are, how far its body moves down.
const AGEABLE_FIELDS: [&str; 6] = ["$scaleHead", "$babyYHeadOffset", "$babyZHeadOffset", "$babyHeadScale", "$babyBodyScale", "$bodyYOffset"];
/// The entity a model's `setupAnim` and `prepareMobModel` take at each level of vanilla's classes
/// (`EntityModel`'s, `HumanoidModel`'s, `AbstractZombieModel`'s erasures), and those classes.
const ENTITY_ARGS: [&str; 3] = ["Lnet/minecraft/world/entity/Entity;", "Lnet/minecraft/world/entity/LivingEntity;", "Lnet/minecraft/world/entity/monster/Monster;"];
const LEVEL_OWNERS: [&str; 3] = [ENTITY_MODEL, HUMANOID, ABSTRACT_ZOMBIE];

const PI: f32 = std::f32::consts::PI;
/// `(float) (Math.PI / 180.0)` and the other fractions of pi vanilla's models round from doubles.
fn pi_by(d: f64) -> f32 {
    (std::f64::consts::PI / d) as f32
}

/// Vanilla's model classes past the ones a mod extends, as the API climbs them (a miss on
/// `ZombieModel` retries `AbstractZombieModel`'s, then `HumanoidModel`'s...).
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    Some(match class {
        ZOMBIE => ABSTRACT_ZOMBIE,
        ABSTRACT_ZOMBIE => HUMANOID,
        HUMANOID => AGEABLE_LIST,
        AGEABLE_LIST | LIST_MODEL | HIERARCHICAL => ENTITY_MODEL,
        ENTITY_MODEL => MODEL,
        _ => return None,
    })
}

/// `EntityModel(renderType)`'s fields: the render type function (null for `entityCutoutNoCull`),
/// its attack not begun, unridden, young.
fn init_model(rt: &mut Rt, this: V, render_type: V) {
    rt.put_field(this, "renderType", render_type);
    rt.put_field(this, "attackTime", V::F(0.0));
    rt.put_field(this, "riding", V::I(0));
    rt.put_field(this, "young", V::I(1));
}

/// `AgeableListModel`'s constructors: the render type and the young one's settings
/// ([`AGEABLE_FIELDS`]; the defaults are vanilla's).
fn init_ageable(rt: &mut Rt, desc: &str, args: &[V]) -> Option<()> {
    let a = |i| arg(args, i);
    let (render_type, s) = match desc {
        "()V" => (V::NULL, [0.0, 5.0, 2.0, 2.0, 2.0, 24.0]),
        "(ZFF)V" => (V::NULL, [a(1).i() as f32, a(2).f(), a(3).f(), 2.0, 2.0, 24.0]),
        "(ZFFFFF)V" => (V::NULL, [a(1).i() as f32, a(2).f(), a(3).f(), a(4).f(), a(5).f(), a(6).f()]),
        "(Ljava/util/function/Function;ZFFFFF)V" => (a(1), [a(2).i() as f32, a(3).f(), a(4).f(), a(5).f(), a(6).f(), a(7).f()]),
        _ => return None,
    };
    init_model(rt, a(0), render_type);
    for (f, v) in AGEABLE_FIELDS.into_iter().zip(s) {
        rt.put_field(a(0), f, V::F(v));
    }
    Some(())
}

/// `HumanoidModel(root[, renderType])`: an ageable model whose young head is scaled, 16 down; its
/// parts the root's children; both arms empty, standing.
fn init_humanoid(rt: &mut Rt, this: V, root: V, render_type: V) {
    init_model(rt, this, render_type);
    for (f, v) in AGEABLE_FIELDS.into_iter().zip([1.0, 16.0, 0.0, 2.0, 2.0, 24.0]) {
        rt.put_field(this, f, V::F(v));
    }
    for (field, name) in HUMANOID_PARTS {
        let n = rt.string(name);
        let part = child(rt, root, n);
        rt.put_field(this, field, part);
    }
    let empty = crate::api::api_enum(rt, ARM_POSE, "EMPTY", 0);
    rt.put_field(this, "leftArmPose", empty);
    rt.put_field(this, "rightArmPose", empty);
    rt.put_field(this, "crouching", V::I(0));
    rt.put_field(this, "swimAmount", V::F(0.0));
}

/// Adds a cube at `origin` of `size`, grown by `grow`, to builder `this`, at its texture offset.
fn push_cube(rt: &mut Rt, this: V, origin: [f32; 3], size: [f32; 3], grow: [f32; 3]) {
    let shape = CubeShape {
        uv: [rt.get_field(this, "$u").i() as f32, rt.get_field(this, "$v").i() as f32],
        origin,
        size,
        grow,
        mirror: rt.get_field(this, "$mirror").i() != 0,
        tex: [1.0, 1.0],
        faces: ALL_FACES,
    };
    let def = rt.new_object(CUBE_DEFINITION);
    put_shape(rt, def, &shape);
    let cubes = rt.get_field(this, "$cubes");
    jutil::list_add(rt, cubes, def);
}

/// `HumanoidModel.createMesh(deformation, yOffset)`: the player's shape (64 by 64 texture), its
/// parts `yOffset` lower, the hat half a pixel bigger.
fn create_mesh(rt: &mut Rt, deformation_arg: V, y: f32) -> V {
    let mesh = rt.new_object(MESH);
    init_mesh(rt, mesh);
    let root = rt.get_field(mesh, "$root");
    let grow = grow_of(rt, deformation_arg);
    let hat = grow.map(|g| g + 0.5);
    let parts: [(&str, [i32; 2], bool, [f32; 3], [f32; 3], [f32; 3], [f32; 3]); 7] = [
        ("head", [0, 0], false, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], grow, [0.0, y, 0.0]),
        ("hat", [32, 0], false, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], hat, [0.0, y, 0.0]),
        ("body", [16, 16], false, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], grow, [0.0, y, 0.0]),
        ("right_arm", [40, 16], false, [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], grow, [-5.0, 2.0 + y, 0.0]),
        ("left_arm", [40, 16], true, [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], grow, [5.0, 2.0 + y, 0.0]),
        ("right_leg", [0, 16], false, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], grow, [-1.9, 12.0 + y, 0.0]),
        ("left_leg", [0, 16], true, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], grow, [1.9, 12.0 + y, 0.0]),
    ];
    for (name, uv, mirror, origin, size, grow, offset) in parts {
        let builder = rt.new_object(CUBE_LIST);
        init_cube_list(rt, builder);
        rt.put_field(builder, "$u", V::I(uv[0]));
        rt.put_field(builder, "$v", V::I(uv[1]));
        rt.put_field(builder, "$mirror", V::I(i32::from(mirror)));
        push_cube(rt, builder, origin, size, grow);
        let pose = part_pose(rt, [offset[0], offset[1], offset[2], 0.0, 0.0, 0.0]);
        let n = rt.string(name);
        add_or_replace_child(rt, root, n, builder, pose);
    }
    mesh
}

/// The parts a list model's `headParts()`, `bodyParts()` or `parts()` gives (the mod's own, or a
/// humanoid's).
fn parts_of(rt: &mut Rt, this: V, key: (&'static str, &'static str)) -> Vec<V> {
    let list = crate::api::virtual_call(rt, key, &[this]);
    jutil::elements(rt, list)
}

/// `AgeableListModel.renderToBuffer`: its head parts and body parts, a young one's scaled down and
/// moved as its settings say.
fn render_ageable(rt: &mut Rt, this: V, stack: V, consumer: V, light: i32, color: i32) {
    let heads = parts_of(rt, this, (HEAD_PARTS_KEY, HEAD_PARTS));
    let bodies = parts_of(rt, this, (BODY_PARTS_KEY, BODY_PARTS));
    let draw = |rt: &mut Rt, parts: &[V]| {
        for &p in parts {
            render_part(rt, p, stack, consumer, light, color);
        }
    };
    if rt.get_field(this, "young").i() == 0 {
        draw(rt, &heads);
        draw(rt, &bodies);
        return;
    }
    let [scale_head, head_y, head_z, head_scale, body_scale, body_y] = AGEABLE_FIELDS.map(|f| float(rt, this, f));
    crate::api::call(rt, PUSH, &[stack]);
    if scale_head != 0.0 {
        let f = 1.5 / head_scale;
        crate::api::call(rt, SCALE, &[stack, V::F(f), V::F(f), V::F(f)]);
    }
    crate::api::call(rt, TRANSLATE, &[stack, V::F(0.0), V::F(head_y / 16.0), V::F(head_z / 16.0)]);
    draw(rt, &heads);
    crate::api::call(rt, POP, &[stack]);
    crate::api::call(rt, PUSH, &[stack]);
    let f = 1.0 / body_scale;
    crate::api::call(rt, SCALE, &[stack, V::F(f), V::F(f), V::F(f)]);
    crate::api::call(rt, TRANSLATE, &[stack, V::F(0.0), V::F(body_y / 16.0), V::F(0.0)]);
    draw(rt, &bodies);
    crate::api::call(rt, POP, &[stack]);
}

/// The level of [`ENTITY_ARGS`] whose `setupAnim` (`prepareMobModel` when `prepare`) a model's own
/// class has: a zombie's `Monster`, a humanoid's `LivingEntity`, `EntityModel`'s `Entity`.
fn model_level(rt: &Rt, this: V, prepare: bool) -> usize {
    if !prepare && rt.instance_of(this, ABSTRACT_ZOMBIE) {
        2
    } else if rt.instance_of(this, HUMANOID) {
        1
    } else {
        0
    }
}

/// The level of [`ENTITY_ARGS`] a call's descriptor takes its entity at.
fn desc_level(desc: &str) -> usize {
    ENTITY_ARGS.iter().position(|t| desc.strip_prefix('(').is_some_and(|d| d.starts_with(t))).unwrap_or(0)
}

/// Vanilla's bridge methods: `setupAnim` or `prepareMobModel` called at level `at` of a model whose
/// class has the method at a higher level runs the receiver's most specific one: a mod's override
/// there (through its bridge), as the JVM would. None when the mod overrides none of them, for
/// vanilla's own at the model's level.
fn bridged(rt: &mut Rt, this: V, name: &str, tail: &str, at: usize, own: usize, args: &[V]) -> Option<V> {
    let class = rt.class_of(this);
    for level in (at + 1..=own).rev() {
        let short = rt.static_key(&format!("{name}.({}{tail}", ENTITY_ARGS[level]));
        if rt.overrides.is_some_and(|o| o(class, short)) {
            let key = rt.static_key(&format!("{}.{short}", LEVEL_OWNERS[level]));
            return Some(crate::api::virtual_call(rt, (key, short), args));
        }
    }
    None
}

/// `setupAnim(entity, limbSwing, limbSwingAmount, ageInTicks, netHeadYaw, headPitch)` on a model
/// whose own (or whose mod's override) it reaches: a humanoid's walk, a zombie's raised arms.
fn setup_anim(rt: &mut Rt, this: V, desc: &str, args: &[V]) -> V {
    let own = model_level(rt, this, false);
    let at = desc_level(desc);
    if let Some(v) = bridged(rt, this, "setupAnim", "FFFFF)V", at, own, args) {
        return v;
    }
    let a = |i| arg(args, i).f();
    let e = arg(args, 1);
    if own >= 1 {
        humanoid_setup_anim(rt, this, e, [a(2), a(3), a(4), a(5), a(6)]);
    }
    if own == 2 {
        let aggressive = crate::api::virtual_call(rt, (IS_AGGRESSIVE_KEY, IS_AGGRESSIVE), &[this, e]).i() != 0;
        let (left, right) = (rt.get_field(this, "leftArm"), rt.get_field(this, "rightArm"));
        let attack = float(rt, this, "attackTime");
        animate_zombie_arms(rt, left, right, aggressive, attack, a(4));
    }
    V::Void
}

/// `prepareMobModel(entity, limbSwing, limbSwingAmount, partialTick)`: a humanoid takes how far
/// the entity is into swimming (its `swimAmount`, lerped; 0 for one that doesn't swim).
fn prepare_mob_model(rt: &mut Rt, this: V, desc: &str, args: &[V]) -> V {
    let own = model_level(rt, this, true);
    if let Some(v) = bridged(rt, this, "prepareMobModel", "FFF)V", desc_level(desc), own, args) {
        return v;
    }
    if own >= 1 {
        let e = arg(args, 1);
        let partial = arg(args, 4).f();
        let (old, now) = (float(rt, e, "swimAmountO"), float(rt, e, "swimAmount"));
        rt.put_field(this, "swimAmount", V::F(old + partial * (now - old)));
    }
    V::Void
}

/// Whether `e` holds its main hand on the left (`getMainArm() == HumanoidArm.LEFT`): what its
/// mod's class answers; right-handed otherwise.
fn main_arm_left(rt: &mut Rt, e: V) -> bool {
    let class = rt.class_of(e);
    if e.r() == 0 || !rt.overrides.is_some_and(|o| o(class, MAIN_ARM)) {
        return false;
    }
    let arm = crate::api::virtual_call(rt, (MAIN_ARM_KEY, MAIN_ARM), &[e]);
    rt.str_of(rt.get_field(arm, "$name")) == Some("LEFT")
}

/// The arm `e` swings (`HumanoidModel.getAttackArm`): its main arm, the other one for its off hand.
fn attack_arm_left(rt: &mut Rt, e: V) -> bool {
    let main_left = main_arm_left(rt, e);
    let off = rt.str_of(rt.get_field(rt.get_field(e, "swingingArm"), "$name")) == Some("OFF_HAND");
    main_left != off
}

/// `Mth.sin`/`Mth.cos` of a float angle (vanilla's table).
fn sin(x: f32) -> f32 {
    crate::api_common::mth_sin(V::F(x))
}
fn cos(x: f32) -> f32 {
    crate::api_common::mth_cos(V::F(x))
}
fn lerp(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * (end - start)
}

/// `HumanoidModel.rotlerpRad`: from `max` toward `mul` by `angle`, the short way round.
fn rotlerp_rad(angle: f32, max: f32, mul: f32) -> f32 {
    let tau = (std::f64::consts::PI * 2.0) as f32;
    let mut f = (mul - max) % tau;
    if f < -PI {
        f += tau;
    }
    if f >= PI {
        f -= tau;
    }
    max + angle * f
}

/// A model part's rotation and position fields, read and written as `ModelPart`'s public fields.
fn get(rt: &Rt, part: V, f: &'static str) -> f32 {
    float(rt, part, f)
}
fn set(rt: &mut Rt, part: V, f: &'static str, v: f32) {
    rt.put_field(part, f, V::F(v));
}

/// `ModelPart.copyFrom`: the scale, rotation and position of `from`.
fn copy_part(rt: &mut Rt, to: V, from: V) {
    for f in ["xScale", "yScale", "zScale", "xRot", "yRot", "zRot", "x", "y", "z"] {
        let v = rt.get_field(from, f);
        rt.put_field(to, f, v);
    }
}

/// `HumanoidModel.setupAnim`: the head turned to look, arms and legs swinging with the walk, a
/// ridden one's legs forward, each arm as its pose holds it, the attack's swing, crouching, the
/// arms' idle bob, a swimmer's strokes; the hat follows the head. `l` is limb swing, its
/// amount, age in ticks, net head yaw and head pitch.
fn humanoid_setup_anim(rt: &mut Rt, this: V, e: V, l: [f32; 5]) {
    let [limb, amount, age, head_yaw, head_pitch] = l;
    let [head, hat, body, right_arm, left_arm, right_leg, left_leg] = HUMANOID_PARTS.map(|(f, _)| rt.get_field(this, f));
    // Fall flying (an elytra) and the swimming pose are the player's: a mod's mob does neither.
    let swim = float(rt, this, "swimAmount");
    set(rt, head, "yRot", head_yaw * pi_by(180.0));
    let pitch = head_pitch * pi_by(180.0);
    let x = if swim > 0.0 { rotlerp_rad(swim, get(rt, head, "xRot"), pitch) } else { pitch };
    set(rt, head, "xRot", x);
    set(rt, body, "yRot", 0.0);
    set(rt, right_arm, "z", 0.0);
    set(rt, right_arm, "x", -5.0);
    set(rt, left_arm, "z", 0.0);
    set(rt, left_arm, "x", 5.0);
    // Vanilla divides the swing by how fast an elytra flies, 1 on foot.
    set(rt, right_arm, "xRot", cos(limb * 0.6662 + PI) * 2.0 * amount * 0.5);
    set(rt, left_arm, "xRot", cos(limb * 0.6662) * 2.0 * amount * 0.5);
    set(rt, right_arm, "zRot", 0.0);
    set(rt, left_arm, "zRot", 0.0);
    set(rt, right_leg, "xRot", cos(limb * 0.6662) * 1.4 * amount);
    set(rt, left_leg, "xRot", cos(limb * 0.6662 + PI) * 1.4 * amount);
    set(rt, right_leg, "yRot", 0.005);
    set(rt, left_leg, "yRot", -0.005);
    set(rt, right_leg, "zRot", 0.005);
    set(rt, left_leg, "zRot", -0.005);
    if rt.get_field(this, "riding").i() != 0 {
        let x = get(rt, right_arm, "xRot");
        set(rt, right_arm, "xRot", x + -pi_by(5.0));
        let x = get(rt, left_arm, "xRot");
        set(rt, left_arm, "xRot", x + -pi_by(5.0));
        set(rt, right_leg, "xRot", -1.4137167);
        set(rt, right_leg, "yRot", pi_by(10.0));
        set(rt, right_leg, "zRot", 0.07853982);
        set(rt, left_leg, "xRot", -1.4137167);
        set(rt, left_leg, "yRot", -pi_by(10.0));
        set(rt, left_leg, "zRot", -0.07853982);
    }
    set(rt, right_arm, "yRot", 0.0);
    set(rt, left_arm, "yRot", 0.0);
    // Not using an item (a mod's mob draws no bow back here): the two-handed pose goes last.
    let right_handed = !main_arm_left(rt, e);
    let (left_pose, right_pose) = (arm_pose(rt, this, "leftArmPose"), arm_pose(rt, this, "rightArmPose"));
    let two_handed = if right_handed { is_two_handed(&left_pose) } else { is_two_handed(&right_pose) };
    let parts = Humanoid { head, right_arm, left_arm };
    if right_handed != two_handed {
        pose_arm(rt, &parts, &left_pose, false);
        pose_arm(rt, &parts, &right_pose, true);
    } else {
        pose_arm(rt, &parts, &right_pose, true);
        pose_arm(rt, &parts, &left_pose, false);
    }
    setup_attack_animation(rt, this, e, &parts, body);
    if rt.get_field(this, "crouching").i() != 0 {
        set(rt, body, "xRot", 0.5);
        let x = get(rt, right_arm, "xRot");
        set(rt, right_arm, "xRot", x + 0.4);
        let x = get(rt, left_arm, "xRot");
        set(rt, left_arm, "xRot", x + 0.4);
        set(rt, right_leg, "z", 4.0);
        set(rt, left_leg, "z", 4.0);
        set(rt, right_leg, "y", 12.2);
        set(rt, left_leg, "y", 12.2);
        set(rt, head, "y", 4.2);
        set(rt, body, "y", 3.2);
        set(rt, left_arm, "y", 5.2);
        set(rt, right_arm, "y", 5.2);
    } else {
        set(rt, body, "xRot", 0.0);
        set(rt, right_leg, "z", 0.0);
        set(rt, left_leg, "z", 0.0);
        set(rt, right_leg, "y", 12.0);
        set(rt, left_leg, "y", 12.0);
        set(rt, head, "y", 0.0);
        set(rt, body, "y", 0.0);
        set(rt, left_arm, "y", 2.0);
        set(rt, right_arm, "y", 2.0);
    }
    if right_pose != "SPYGLASS" {
        bob_model_part(rt, right_arm, age, 1.0);
    }
    if left_pose != "SPYGLASS" {
        bob_model_part(rt, left_arm, age, -1.0);
    }
    if swim > 0.0 {
        swim_strokes(rt, this, e, limb, swim, [right_arm, left_arm, right_leg, left_leg]);
    }
    copy_part(rt, hat, head);
}

/// The parts an arm pose moves.
struct Humanoid {
    head: V,
    right_arm: V,
    left_arm: V,
}

/// The name of the model's arm pose in `field` (`EMPTY` when unset).
fn arm_pose(rt: &Rt, this: V, field: &'static str) -> String {
    rt.str_of(rt.get_field(rt.get_field(this, field), "$name")).unwrap_or("EMPTY").to_owned()
}

/// `ArmPose.isTwoHanded`: drawing a bow and holding or charging a crossbow.
fn is_two_handed(pose: &str) -> bool {
    matches!(pose, "BOW_AND_ARROW" | "CROSSBOW_CHARGE" | "CROSSBOW_HOLD")
}

/// `HumanoidModel.poseRightArm` (`right`) and `poseLeftArm`: the arm as its pose holds it. A pose a
/// mod adds (NeoForge's extensible enum) moves nothing here.
fn pose_arm(rt: &mut Rt, p: &Humanoid, pose: &str, right: bool) {
    let arm = if right { p.right_arm } else { p.left_arm };
    let side = if right { 1.0 } else { -1.0 };
    let (head_x, head_y) = (get(rt, p.head, "xRot"), get(rt, p.head, "yRot"));
    match pose {
        "EMPTY" => set(rt, arm, "yRot", 0.0),
        "ITEM" => {
            let x = get(rt, arm, "xRot");
            set(rt, arm, "xRot", x * 0.5 - pi_by(10.0));
            set(rt, arm, "yRot", 0.0);
        }
        "BLOCK" => {
            let x = get(rt, arm, "xRot");
            set(rt, arm, "xRot", x * 0.5 - 0.9424779 + head_x.clamp((-std::f64::consts::PI * 4.0 / 9.0) as f32, 0.43633232));
            let base = if right { -30.0 } else { 30.0 };
            set(rt, arm, "yRot", base * pi_by(180.0) + head_y.clamp(-pi_by(6.0), pi_by(6.0)));
        }
        "BOW_AND_ARROW" => {
            if right {
                set(rt, p.right_arm, "yRot", -0.1 + head_y);
                set(rt, p.left_arm, "yRot", 0.1 + head_y + 0.4);
            } else {
                set(rt, p.right_arm, "yRot", -0.1 + head_y - 0.4);
                set(rt, p.left_arm, "yRot", 0.1 + head_y);
            }
            set(rt, p.right_arm, "xRot", -pi_by(2.0) + head_x);
            set(rt, p.left_arm, "xRot", -pi_by(2.0) + head_x);
        }
        "THROW_SPEAR" => {
            let x = get(rt, arm, "xRot");
            set(rt, arm, "xRot", x * 0.5 - PI);
            set(rt, arm, "yRot", 0.0);
        }
        "CROSSBOW_CHARGE" => animate_crossbow_charge(rt, p.right_arm, p.left_arm, right),
        "CROSSBOW_HOLD" => animate_crossbow_hold(rt, p.right_arm, p.left_arm, p.head, right),
        "SPYGLASS" => {
            // Crouching is the player's: a mod's mob stands.
            set(rt, arm, "xRot", (head_x - 1.9198622).clamp(-2.4, 3.3));
            set(rt, arm, "yRot", head_y - side * pi_by(12.0));
        }
        "TOOT_HORN" => {
            set(rt, arm, "xRot", head_x.clamp(-1.2, 1.2) - 1.4835298);
            set(rt, arm, "yRot", head_y - side * pi_by(6.0));
        }
        "BRUSH" => {
            let x = get(rt, arm, "xRot");
            set(rt, arm, "xRot", x * 0.5 - pi_by(5.0));
            set(rt, arm, "yRot", 0.0);
        }
        _ => {}
    }
}

/// `HumanoidModel.setupAttackAnimation`: while `attackTime` runs, the body twists toward the arm
/// that swings, and that arm sweeps down and across.
fn setup_attack_animation(rt: &mut Rt, this: V, e: V, p: &Humanoid, body: V) {
    let attack = float(rt, this, "attackTime");
    if attack <= 0.0 {
        return;
    }
    let left = attack_arm_left(rt, e);
    let arm = if left { p.left_arm } else { p.right_arm };
    let mut turn = sin(attack.sqrt() * (std::f64::consts::PI * 2.0) as f32) * 0.2;
    if left {
        turn *= -1.0;
    }
    set(rt, body, "yRot", turn);
    set(rt, p.right_arm, "z", sin(turn) * 5.0);
    set(rt, p.right_arm, "x", -cos(turn) * 5.0);
    set(rt, p.left_arm, "z", -sin(turn) * 5.0);
    set(rt, p.left_arm, "x", cos(turn) * 5.0);
    let y = get(rt, p.right_arm, "yRot");
    set(rt, p.right_arm, "yRot", y + turn);
    let y = get(rt, p.left_arm, "yRot");
    set(rt, p.left_arm, "yRot", y + turn);
    let x = get(rt, p.left_arm, "xRot");
    set(rt, p.left_arm, "xRot", x + turn);
    let mut f = 1.0 - attack;
    f *= f;
    f *= f;
    f = 1.0 - f;
    let f1 = sin(f * PI);
    let f2 = sin(attack * PI) * -(get(rt, p.head, "xRot") - 0.7) * 0.75;
    let x = get(rt, arm, "xRot");
    set(rt, arm, "xRot", x - (f1 * 1.2 + f2));
    let y = get(rt, arm, "yRot");
    set(rt, arm, "yRot", y + get(rt, body, "yRot") * 2.0);
    let z = get(rt, arm, "zRot");
    set(rt, arm, "zRot", z + sin(attack * PI) * -0.4);
}

/// The swimming half of `HumanoidModel.setupAnim`: the arms' strokes through a 26-tick cycle (not
/// the arm that is attacking), the legs' kick.
fn swim_strokes(rt: &mut Rt, this: V, e: V, limb: f32, swim: f32, [right_arm, left_arm, right_leg, left_leg]: [V; 4]) {
    let quadratic = |x: f32| -65.0 * x + x * x;
    let f5 = limb % 26.0;
    let attacking = float(rt, this, "attackTime") > 0.0;
    let left = attack_arm_left(rt, e);
    let f1 = if !left && attacking { 0.0 } else { swim };
    let f2 = if left && attacking { 0.0 } else { swim };
    let half = pi_by(2.0);
    let (lx, ly, lz) = (get(rt, left_arm, "xRot"), get(rt, left_arm, "yRot"), get(rt, left_arm, "zRot"));
    let (rx, ry, rz) = (get(rt, right_arm, "xRot"), get(rt, right_arm, "yRot"), get(rt, right_arm, "zRot"));
    let targets = if f5 < 14.0 {
        Some([0.0, PI, PI + 1.8707964 * quadratic(f5) / quadratic(14.0), 0.0, PI, PI - 1.8707964 * quadratic(f5) / quadratic(14.0)])
    } else if f5 < 22.0 {
        let f6 = (f5 - 14.0) / 8.0;
        Some([half * f6, PI, 5.012389 - 1.8707964 * f6, half * f6, PI, 1.2707963 + 1.8707964 * f6])
    } else if f5 < 26.0 {
        let f3 = (f5 - 22.0) / 4.0;
        Some([half - half * f3, PI, PI, half - half * f3, PI, PI])
    } else {
        None
    };
    if let Some([tlx, tly, tlz, trx, try_, trz]) = targets {
        set(rt, left_arm, "xRot", rotlerp_rad(f2, lx, tlx));
        set(rt, right_arm, "xRot", lerp(f1, rx, trx));
        set(rt, left_arm, "yRot", rotlerp_rad(f2, ly, tly));
        set(rt, right_arm, "yRot", lerp(f1, ry, try_));
        set(rt, left_arm, "zRot", rotlerp_rad(f2, lz, tlz));
        set(rt, right_arm, "zRot", lerp(f1, rz, trz));
    }
    let x = get(rt, left_leg, "xRot");
    set(rt, left_leg, "xRot", lerp(swim, x, 0.3 * cos(limb * 0.33333334 + PI)));
    let x = get(rt, right_leg, "xRot");
    set(rt, right_leg, "xRot", lerp(swim, x, 0.3 * cos(limb * 0.33333334)));
}

/// `AnimationUtils.bobModelPart`: an arm's idle sway with age.
fn bob_model_part(rt: &mut Rt, part: V, age: f32, multiplier: f32) {
    let z = get(rt, part, "zRot");
    set(rt, part, "zRot", z + multiplier * (cos(age * 0.09) * 0.05 + 0.05));
    let x = get(rt, part, "xRot");
    set(rt, part, "xRot", x + multiplier * sin(age * 0.067) * 0.05);
}

/// `AnimationUtils.animateCrossbowHold`: both arms raised to aim along the head.
fn animate_crossbow_hold(rt: &mut Rt, right_arm: V, left_arm: V, head: V, right: bool) {
    let (main, other) = if right { (right_arm, left_arm) } else { (left_arm, right_arm) };
    let (hx, hy) = (get(rt, head, "xRot"), get(rt, head, "yRot"));
    set(rt, main, "yRot", (if right { -0.3 } else { 0.3 }) + hy);
    set(rt, other, "yRot", (if right { 0.6 } else { -0.6 }) + hy);
    set(rt, main, "xRot", -pi_by(2.0) + hx + 0.1);
    set(rt, other, "xRot", -1.5 + hx);
}

/// `AnimationUtils.animateCrossbowCharge` at the charge's start: the entity isn't drawing it (no
/// ticks of use), so the drawing arm hasn't moved yet.
fn animate_crossbow_charge(rt: &mut Rt, right_arm: V, left_arm: V, right: bool) {
    let (main, other) = if right { (right_arm, left_arm) } else { (left_arm, right_arm) };
    set(rt, main, "yRot", if right { -0.8 } else { 0.8 });
    set(rt, main, "xRot", -0.97079635);
    set(rt, other, "xRot", -0.97079635);
    let f2 = 0.0;
    set(rt, other, "yRot", lerp(f2, 0.4, 0.85) * if right { 1.0 } else { -1.0 });
    let x = get(rt, other, "xRot");
    set(rt, other, "xRot", lerp(f2, x, -pi_by(2.0)));
}

/// `AnimationUtils.animateZombieArms`: both arms held out (higher while aggressive), swinging with
/// the attack, bobbing.
fn animate_zombie_arms(rt: &mut Rt, left_arm: V, right_arm: V, aggressive: bool, attack: f32, age: f32) {
    let f = sin(attack * PI);
    let f1 = sin((1.0 - (1.0 - attack) * (1.0 - attack)) * PI);
    set(rt, right_arm, "zRot", 0.0);
    set(rt, left_arm, "zRot", 0.0);
    set(rt, right_arm, "yRot", -(0.1 - f * 0.6));
    set(rt, left_arm, "yRot", 0.1 - f * 0.6);
    let f2 = -PI / if aggressive { 1.5 } else { 2.25 };
    set(rt, right_arm, "xRot", f2 + (f * 1.2 - f1 * 0.4));
    set(rt, left_arm, "xRot", f2 + (f * 1.2 - f1 * 0.4));
    bob_model_part(rt, right_arm, age, 1.0);
    bob_model_part(rt, left_arm, age, -1.0);
}

/// `AnimationUtils.swingWeaponDown`: a mob's weapon arm raised high and brought down with the
/// attack, the other arm low; both bob.
fn swing_weapon_down(rt: &mut Rt, right_arm: V, left_arm: V, mob: V, attack: f32, age: f32) {
    let f = sin(attack * PI);
    let f1 = sin((1.0 - (1.0 - attack) * (1.0 - attack)) * PI);
    set(rt, right_arm, "zRot", 0.0);
    set(rt, left_arm, "zRot", 0.0);
    set(rt, right_arm, "yRot", pi_by(20.0));
    set(rt, left_arm, "yRot", -pi_by(20.0));
    let (main, other) = if main_arm_left(rt, mob) { (left_arm, right_arm) } else { (right_arm, left_arm) };
    set(rt, main, "xRot", -1.8849558 + cos(age * 0.09) * 0.15 + (f * 2.2 - f1 * 0.4));
    set(rt, other, "xRot", -0.0 + cos(age * 0.19) * 0.5 + (f * 1.2 - f1 * 0.4));
    bob_model_part(rt, right_arm, age, 1.0);
    bob_model_part(rt, left_arm, age, -1.0);
}

/// `AnimationUtils`' static helpers.
fn animation_utils(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    match name {
        "bobModelPart" => bob_model_part(rt, a(0), a(1).f(), a(2).f()),
        "bobArms" => {
            bob_model_part(rt, a(0), a(2).f(), 1.0);
            bob_model_part(rt, a(1), a(2).f(), -1.0);
        }
        "animateZombieArms" => animate_zombie_arms(rt, a(0), a(1), a(2).i() != 0, a(3).f(), a(4).f()),
        "swingWeaponDown" => swing_weapon_down(rt, a(0), a(1), a(2), a(3).f(), a(4).f()),
        "animateCrossbowHold" => animate_crossbow_hold(rt, a(0), a(1), a(2), a(3).i() != 0),
        "animateCrossbowCharge" => animate_crossbow_charge(rt, a(0), a(1), a(3).i() != 0),
        _ => return None,
    }
    Some(V::Void)
}

/// The vanilla model classes' own calls past `Model`'s (`HumanoidModel`'s parts, mesh and poses,
/// the list models' drawing); None for the rest.
fn vanilla_model_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match name {
        "<init>" => match owner {
            LIST_MODEL => {
                init_model(rt, this, if desc == "(Ljava/util/function/Function;)V" { a(1) } else { V::NULL });
                V::Void
            }
            AGEABLE_LIST => {
                init_ageable(rt, desc, args)?;
                V::Void
            }
            HUMANOID | ABSTRACT_ZOMBIE | ZOMBIE => {
                let render_type = if desc.ends_with("Ljava/util/function/Function;)V") { a(2) } else { V::NULL };
                init_humanoid(rt, this, a(1), render_type);
                V::Void
            }
            _ => return None,
        },
        "createMesh" if owner == HUMANOID => create_mesh(rt, a(0), a(1).f()),
        "headParts" | "bodyParts" if rt.instance_of(this, HUMANOID) => {
            let fields: &[&'static str] = if name == "headParts" { &["head"] } else { &["body", "rightArm", "leftArm", "rightLeg", "leftLeg", "hat"] };
            let parts = fields.iter().map(|&f| rt.get_field(this, f)).collect();
            jutil::new_list(rt, parts)
        }
        "setupAnim" if desc.ends_with("FFFFF)V") => setup_anim(rt, this, desc, args),
        "prepareMobModel" if desc.ends_with("FFF)V") => prepare_mob_model(rt, this, desc, args),
        "copyPropertiesTo" if desc == "(Lnet/minecraft/client/model/HumanoidModel;)V" => {
            for f in ["attackTime", "riding", "young", "leftArmPose", "rightArmPose", "crouching"] {
                let v = rt.get_field(this, f);
                rt.put_field(a(1), f, v);
            }
            for (f, _) in HUMANOID_PARTS {
                let (to, from) = (rt.get_field(a(1), f), rt.get_field(this, f));
                copy_part(rt, to, from);
            }
            V::Void
        }
        "setAllVisible" => {
            for (f, _) in HUMANOID_PARTS {
                let p = rt.get_field(this, f);
                rt.put_field(p, "visible", V::I(a(1).i()));
            }
            V::Void
        }
        "getHead" => rt.get_field(this, "head"),
        "getArm" | "translateToHand" => {
            let left = rt.str_of(rt.get_field(a(1), "$name")) == Some("LEFT");
            let arm = rt.get_field(this, if left { "leftArm" } else { "rightArm" });
            if name == "getArm" {
                return Some(arm);
            }
            translate_and_rotate(rt, arm, a(2));
            V::Void
        }
        "isAggressive" if owner == ZOMBIE || rt.instance_of(this, ZOMBIE) => crate::api::virtual_call(rt, MOB_IS_AGGRESSIVE, &[a(1)]),
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        CUBE_LIST
            | DEFORMATION
            | POSE
            | PART_DEFINITION
            | MESH
            | LAYER_DEFINITION
            | MATERIAL
            | PART
            | CUBE
            | MODEL_SET
            | REGISTER_LAYERS
            | MODEL
            | ENTITY_MODEL
            | HIERARCHICAL
            | AGEABLE_LIST
            | LIST_MODEL
            | HUMANOID
            | ABSTRACT_ZOMBIE
            | ZOMBIE
            | ANIMATION_UTILS
    ) || (owner == ARM_POSE && name == "isTwoHanded")
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        (CUBE_LIST, "create") => {
            let o = rt.new_object(CUBE_LIST);
            init_cube_list(rt, o);
            o
        }
        (CUBE_LIST, "<init>") => {
            init_cube_list(rt, a(0));
            V::Void
        }
        (CUBE_LIST, "texOffs") => {
            rt.put_field(a(0), "$u", a(1));
            rt.put_field(a(0), "$v", a(2));
            a(0)
        }
        (CUBE_LIST, "mirror") => {
            let on = if desc.starts_with("(Z)") { a(1).i() } else { 1 };
            rt.put_field(a(0), "$mirror", V::I(on));
            a(0)
        }
        (CUBE_LIST, "addBox") => {
            add_box(rt, desc, args)?;
            a(0)
        }
        (CUBE_LIST, "getCubes") => {
            let cubes = jutil::elements(rt, rt.get_field(a(0), "$cubes"));
            jutil::new_list(rt, cubes)
        }
        (DEFORMATION, "<init>") => {
            let g = if desc == "(F)V" { [a(1).f(); 3] } else { [a(1).f(), a(2).f(), a(3).f()] };
            put_floats(rt, a(0), &[("growX", g[0]), ("growY", g[1]), ("growZ", g[2])]);
            V::Void
        }
        (DEFORMATION, "extend") => {
            let [x, y, z] = grow_of(rt, a(0));
            let by = if desc.starts_with("(F)") { [a(1).f(); 3] } else { [a(1).f(), a(2).f(), a(3).f()] };
            deformation(rt, [x + by[0], y + by[1], z + by[2]])
        }
        (POSE, "<init>") => {
            for (i, f) in POSE_FIELDS.into_iter().enumerate() {
                rt.put_field(a(0), f, V::F(a(i + 1).f()));
            }
            V::Void
        }
        (POSE, "offset") => part_pose(rt, [a(0).f(), a(1).f(), a(2).f(), 0.0, 0.0, 0.0]),
        (POSE, "rotation") => part_pose(rt, [0.0, 0.0, 0.0, a(0).f(), a(1).f(), a(2).f()]),
        (POSE, "offsetAndRotation") => part_pose(rt, [a(0).f(), a(1).f(), a(2).f(), a(3).f(), a(4).f(), a(5).f()]),
        (PART_DEFINITION, "addOrReplaceChild") => add_or_replace_child(rt, a(0), a(1), a(2), a(3)),
        (PART_DEFINITION, "getChild") => {
            let children = rt.get_field(a(0), "$children");
            jutil::table_get(rt, children, a(1)).unwrap_or(V::NULL)
        }
        (PART_DEFINITION, "bake") => bake(rt, a(0), a(1).i(), a(2).i()),
        (MESH, "<init>") => {
            init_mesh(rt, a(0));
            V::Void
        }
        (MESH, "getRoot") => rt.get_field(a(0), "$root"),
        (LAYER_DEFINITION, "create") => layer_definition(rt, a(0), a(1), a(2)),
        (LAYER_DEFINITION, "<init>") => {
            rt.put_field(a(0), "mesh", a(1));
            rt.put_field(a(0), "material", a(2));
            V::Void
        }
        (MATERIAL, "<init>") => {
            rt.put_field(a(0), "xTexSize", a(1));
            rt.put_field(a(0), "yTexSize", a(2));
            V::Void
        }
        (LAYER_DEFINITION, "bakeRoot") => bake_root(rt, a(0)),
        (MODEL_SET, "bakeLayer") => bake_layer(rt, a(1)),
        // The supplier is kept; the layer is built each time a renderer bakes it.
        (REGISTER_LAYERS, "registerLayerDefinition") => {
            let name = layer_name(rt, a(1));
            rt.interned.insert(format!("layer/{name}"), a(2));
            V::Void
        }
        (PART, _) => return part_call(rt, name, desc, args),
        (CUBE, "<init>") => {
            let s = CubeShape {
                uv: [a(1).i() as f32, a(2).i() as f32],
                origin: [a(3).f(), a(4).f(), a(5).f()],
                size: [a(6).f(), a(7).f(), a(8).f()],
                grow: [a(9).f(), a(10).f(), a(11).f()],
                mirror: a(12).i() != 0,
                tex: [a(13).f(), a(14).f()],
                faces: faces_of(rt, a(15)),
            };
            put_cube(rt, a(0), &s);
            V::Void
        }
        (MODEL | ENTITY_MODEL | HIERARCHICAL | AGEABLE_LIST | LIST_MODEL | HUMANOID | ABSTRACT_ZOMBIE | ZOMBIE, _) => {
            return vanilla_model_call(rt, owner, name, desc, args).or_else(|| model_call(rt, owner, name, desc, args));
        }
        (ARM_POSE, "isTwoHanded") => {
            let pose = rt.str_of(rt.get_field(a(0), "$name")).unwrap_or("EMPTY").to_owned();
            V::I(i32::from(is_two_handed(&pose)))
        }
        (ANIMATION_UTILS, _) => return animation_utils(rt, name, args),
        _ => return None,
    })
}

/// `PartPose.ZERO` and `CubeDeformation.NONE`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (POSE, "ZERO") => Some(zero_pose(rt)),
        (DEFORMATION, "NONE") => Some(rt.intern("compat:deformation-none".into(), |rt| deformation(rt, [0.0; 3]))),
        _ => None,
    }
}

/// Models the other modules' tests draw: a one-cube part, vanilla's humanoid.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// A part of one 2-pixel cube at the origin, on a 16 by 16 texture.
    pub(crate) fn cube_part(rt: &mut Rt) -> V {
        let mesh = rt.new_object(MESH);
        init_mesh(rt, mesh);
        let root = rt.get_field(mesh, "$root");
        let builder = rt.new_object(CUBE_LIST);
        init_cube_list(rt, builder);
        push_cube(rt, builder, [0.0; 3], [2.0; 3], [0.0; 3]);
        let name = rt.string("cube");
        let zero = zero_pose(rt);
        add_or_replace_child(rt, root, name, builder, zero);
        let (w, h) = (V::I(16), V::I(16));
        let layer = layer_definition(rt, mesh, w, h);
        bake_root(rt, layer)
    }

    /// `new HumanoidModel(LayerDefinition.create(HumanoidModel.createMesh(NONE, 0), 64, 64).bakeRoot())`.
    pub(crate) fn humanoid(rt: &mut Rt) -> V {
        let none = deformation(rt, [0.0; 3]);
        let mesh = create_mesh(rt, none, 0.0);
        let (w, h) = (V::I(64), V::I(64));
        let layer = layer_definition(rt, mesh, w, h);
        let root = bake_root(rt, layer);
        let model = rt.new_object(HUMANOID);
        init_humanoid(rt, model, root, V::NULL);
        model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const ADD_BOX: &str = "net/minecraft/client/model/geom/builders/CubeListBuilder.addBox.(FFFFFF)Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
    const ADD_CHILD: &str = "net/minecraft/client/model/geom/builders/PartDefinition.addOrReplaceChild.(Ljava/lang/String;Lnet/minecraft/client/model/geom/builders/CubeListBuilder;Lnet/minecraft/client/model/geom/PartPose;)Lnet/minecraft/client/model/geom/builders/PartDefinition;";
    const CREATE: &str = "net/minecraft/client/model/geom/builders/CubeListBuilder.create.()Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
    const TEX_OFFS: &str = "net/minecraft/client/model/geom/builders/CubeListBuilder.texOffs.(II)Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
    const MIRROR: &str = "net/minecraft/client/model/geom/builders/CubeListBuilder.mirror.()Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
    const OFFSET: &str = "net/minecraft/client/model/geom/PartPose.offset.(FFF)Lnet/minecraft/client/model/geom/PartPose;";
    const OFFSET_ROTATION: &str = "net/minecraft/client/model/geom/PartPose.offsetAndRotation.(FFFFFF)Lnet/minecraft/client/model/geom/PartPose;";
    const MESH_INIT: &str = "net/minecraft/client/model/geom/builders/MeshDefinition.<init>.()V";
    const GET_ROOT: &str = "net/minecraft/client/model/geom/builders/MeshDefinition.getRoot.()Lnet/minecraft/client/model/geom/builders/PartDefinition;";
    const LAYER_CREATE: &str = "net/minecraft/client/model/geom/builders/LayerDefinition.create.(Lnet/minecraft/client/model/geom/builders/MeshDefinition;II)Lnet/minecraft/client/model/geom/builders/LayerDefinition;";
    const BAKE_ROOT: &str = "net/minecraft/client/model/geom/builders/LayerDefinition.bakeRoot.()Lnet/minecraft/client/model/geom/ModelPart;";
    const GET_CHILD: &str = "net/minecraft/client/model/geom/ModelPart.getChild.(Ljava/lang/String;)Lnet/minecraft/client/model/geom/ModelPart;";
    const RENDER: &str = "net/minecraft/client/model/geom/ModelPart.render.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;II)V";
    const ENTITY_CUTOUT: &str = "net/minecraft/client/renderer/RenderType.entityCutoutNoCull.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/renderer/RenderType;";
    const GET_BUFFER: &str = "net/minecraft/client/renderer/MultiBufferSource.getBuffer.(Lnet/minecraft/client/renderer/RenderType;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const LAYER_INIT: &str = "net/minecraft/client/model/geom/ModelLayerLocation.<init>.(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/String;)V";
    const REGISTER: &str = "net/neoforged/neoforge/client/event/EntityRenderersEvent$RegisterLayerDefinitions.registerLayerDefinition.(Lnet/minecraft/client/model/geom/ModelLayerLocation;Ljava/util/function/Supplier;)V";
    const CONTEXT_BAKE: &str = "net/minecraft/client/renderer/entity/EntityRendererProvider$Context.bakeLayer.(Lnet/minecraft/client/model/geom/ModelLayerLocation;)Lnet/minecraft/client/model/geom/ModelPart;";

    /// A mod's layer as a mob's `createBodyLayer` writes it: a head, an 8-pixel cube at texture
    /// offset (0, 0) six pixels below the root, on a 64 by 32 texture; with `mirror`, a mirrored
    /// copy at (1, 0, 0) beside it too.
    fn head_layer(rt: &mut Rt, mirror: bool) -> V {
        let mesh = rt.new_object(MESH);
        crate::api::call(rt, MESH_INIT, &[mesh]);
        let root = crate::api::call(rt, GET_ROOT, &[mesh]);
        let cubes = crate::api::call(rt, CREATE, &[]);
        let cubes = crate::api::call(rt, TEX_OFFS, &[cubes, V::I(0), V::I(0)]);
        crate::api::call(rt, ADD_BOX, &[cubes, V::F(-4.0), V::F(-8.0), V::F(-4.0), V::F(8.0), V::F(8.0), V::F(8.0)]);
        let pose = crate::api::call(rt, OFFSET, &[V::F(0.0), V::F(6.0), V::F(0.0)]);
        let head = rt.string("head");
        crate::api::call(rt, ADD_CHILD, &[root, head, cubes, pose]);
        if mirror {
            let cubes = crate::api::call(rt, CREATE, &[]);
            let cubes = crate::api::call(rt, MIRROR, &[cubes]);
            crate::api::call(rt, ADD_BOX, &[cubes, V::F(-4.0), V::F(-8.0), V::F(-4.0), V::F(8.0), V::F(8.0), V::F(8.0)]);
            let pose = crate::api::call(rt, OFFSET, &[V::F(16.0), V::F(6.0), V::F(0.0)]);
            let other = rt.string("mirrored");
            crate::api::call(rt, ADD_CHILD, &[root, other, cubes, pose]);
        }
        crate::api::call(rt, LAYER_CREATE, &[mesh, V::I(64), V::I(32)])
    }

    /// A consumer drawing with `texture` (`buffers.getBuffer(RenderType.entityCutoutNoCull(texture))`).
    fn consumer(rt: &mut Rt, texture: &str) -> V {
        let loc = rt.string(texture);
        let ty = crate::api::call(rt, ENTITY_CUTOUT, &[loc]);
        let buffers = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        crate::api::call(rt, GET_BUFFER, &[buffers, ty])
    }

    /// Each drawn face's direction with its rectangle on the texture in pixels (`[u0, v0, u1, v1]`,
    /// the lesser first) and its corners in pixels.
    fn faces(quads: &[Quad], size: [f32; 2]) -> Vec<(u8, [f32; 4], [[f32; 3]; 4])> {
        quads
            .iter()
            .map(|q| {
                let us = q.uv.map(|uv| (uv[0] * size[0]).round());
                let vs = q.uv.map(|uv| (uv[1] * size[1]).round());
                let lo = |xs: [f32; 4]| xs.into_iter().fold(f32::MAX, f32::min);
                let hi = |xs: [f32; 4]| xs.into_iter().fold(f32::MIN, f32::max);
                (q.facing, [lo(us), lo(vs), hi(us), hi(vs)], q.positions.map(|p| p.map(|x| x * 16.0)))
            })
            .collect()
    }

    fn quads(model: &Model) -> &[Quad] {
        match model {
            Model::Quads(q) => q,
            other => panic!("not faces: {other:?}"),
        }
    }

    /// A baked head draws its six faces where the player skin's head is laid out on the texture
    /// (the top at 8..16 by 0..8, the bottom at 16..24, the front at 8..16 by 8..16, the right side
    /// at 0..8, the left at 16..24, the back at 24..32; the model's y grows downwards, so its top is
    /// the faces at the least y), on the mod's `entity/` sprite, under its part's pose: six pixels
    /// down from the stack's.
    #[test]
    fn a_baked_layer_draws_its_cubes_as_vanilla_lays_them_out() {
        let mut rt = Rt::new();
        crate::install();
        let layer = head_layer(&mut rt, false);
        let root = crate::api::call(&mut rt, BAKE_ROOT, &[layer]);
        let name = rt.string("head");
        let head = crate::api::call(&mut rt, GET_CHILD, &[root, name]);
        assert_eq!(rt.get_field(head, "y"), V::F(6.0));
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let c = consumer(&mut rt, "mymod:textures/entity/golem.png");
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| crate::api::call(rt, RENDER, &[root, stack, c, V::I(crate::render::FULL_BRIGHT), V::I(crate::render::NO_OVERLAY)]));
        assert_eq!(host.drawn.len(), 1, "the head; the root has no cubes");
        let (model, pose, color, light) = &host.drawn[0];
        assert_eq!((*color, *light), (u32::MAX, crate::render::FULL_BRIGHT as u32));
        let mut moved = crate::render::IDENTITY;
        moved[13] = 6.0 / 16.0;
        assert_eq!(*pose, moved);
        let q = quads(model);
        assert!(q.iter().all(|q| q.sprite == "mymod:entity/golem"), "{:?}", q[0].sprite);
        let mut drawn = faces(q, [64.0, 32.0]);
        drawn.sort_by_key(|f| f.0);
        let rects: Vec<(u8, [f32; 4])> = drawn.iter().map(|f| (f.0, f.1)).collect();
        assert_eq!(
            rects,
            [
                (DOWN, [8.0, 0.0, 16.0, 8.0]),
                (UP, [16.0, 0.0, 24.0, 8.0]),
                (NORTH, [8.0, 8.0, 16.0, 16.0]),
                (SOUTH, [24.0, 8.0, 32.0, 16.0]),
                (WEST, [0.0, 8.0, 8.0, 16.0]),
                (EAST, [16.0, 8.0, 24.0, 16.0]),
            ]
        );
        // Each face lies on its side of the box, -4..4 by -8..0 by -4..4.
        for (facing, _, corners) in &drawn {
            let (axis, at) = match *facing {
                DOWN => (1, -8.0),
                UP => (1, 0.0),
                NORTH => (2, -4.0),
                SOUTH => (2, 4.0),
                WEST => (0, -4.0),
                _ => (0, 4.0),
            };
            assert!(corners.iter().all(|c| c[axis] == at), "{facing}: {corners:?}");
        }
        // The front's texture is unmirrored: its least u and v at the box's least y, on the side of
        // the least x.
        let front = q.iter().find(|q| q.facing == NORTH).unwrap();
        let corner = front.positions.iter().zip(front.uv).find(|(_, uv)| *uv == [8.0 / 64.0, 8.0 / 32.0]).map(|(p, _)| p.map(|x| x * 16.0));
        assert_eq!(corner, Some([-4.0, -8.0, -4.0]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `addBox`'s other forms, as the player's hat layer writes it (`addBox("hat", -4, -8, -4, 8, 8,
    /// 8, new CubeDeformation(0.5), 32, 0)`): the texture offset it names (the skin's hat front at
    /// 40..48 by 8..16), the box grown half a pixel each way; a mirror given with the box; and a box
    /// showing only the faces it lists.
    #[test]
    fn add_box_takes_a_named_offset_a_deformation_a_mirror_and_its_faces() {
        let mut rt = Rt::new();
        crate::install();
        let builder = crate::api::call(&mut rt, CREATE, &[]);
        let grow = rt.new_object(DEFORMATION);
        crate::api::call(&mut rt, "net/minecraft/client/model/geom/builders/CubeDeformation.<init>.(F)V", &[grow, V::F(0.5)]);
        let comment = rt.string("hat");
        let hat = "net/minecraft/client/model/geom/builders/CubeListBuilder.addBox.(Ljava/lang/String;FFFIIILnet/minecraft/client/model/geom/builders/CubeDeformation;II)Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
        crate::api::call(&mut rt, hat, &[builder, comment, V::F(-4.0), V::F(-8.0), V::F(-4.0), V::I(8), V::I(8), V::I(8), grow, V::I(32), V::I(0)]);
        let mirrored = "net/minecraft/client/model/geom/builders/CubeListBuilder.addBox.(FFFFFFZ)Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
        crate::api::call(&mut rt, mirrored, &[builder, V::F(0.0), V::F(0.0), V::F(0.0), V::F(2.0), V::F(2.0), V::F(2.0), V::I(1)]);
        let up = crate::api_common::direction(&mut rt, 1);
        let only_up = crate::api::call(&mut rt, "java/util/EnumSet.of.(Ljava/lang/Enum;)Ljava/util/EnumSet;", &[up]);
        let some = "net/minecraft/client/model/geom/builders/CubeListBuilder.addBox.(FFFFFFLjava/util/Set;)Lnet/minecraft/client/model/geom/builders/CubeListBuilder;";
        crate::api::call(&mut rt, some, &[builder, V::F(0.0), V::F(0.0), V::F(0.0), V::F(2.0), V::F(2.0), V::F(2.0), only_up]);
        let cubes: Vec<CubeShape> = jutil::elements(&rt, rt.get_field(builder, "$cubes")).into_iter().map(|c| shape_of(&rt, c)).collect();
        assert_eq!(cubes.len(), 3);
        let mut hat = cubes[0];
        hat.tex = [64.0, 64.0];
        let front = cube_faces(&hat).into_iter().find(|f| f.0 == NORTH).unwrap();
        let us: Vec<f32> = front.1.iter().map(|c| c.1[0] * 64.0).collect();
        let vs: Vec<f32> = front.1.iter().map(|c| c.1[1] * 64.0).collect();
        assert_eq!((us.iter().copied().fold(f32::MAX, f32::min), us.iter().copied().fold(f32::MIN, f32::max)), (40.0, 48.0));
        assert_eq!((vs.iter().copied().fold(f32::MAX, f32::min), vs.iter().copied().fold(f32::MIN, f32::max)), (8.0, 16.0));
        assert!(front.1.iter().all(|c| c.0[2] == -4.5), "grown half a pixel: {front:?}");
        assert!(cubes[1].mirror && cubes[1].uv == [32.0, 0.0], "the builder keeps the offset the hat named");
        assert_eq!(cube_faces(&cubes[2]).iter().map(|f| f.0).collect::<Vec<_>>(), [UP]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mirrored cube shows each side's texture on the other (the right side's at 0..8 on the side
    /// of the greatest x, which faces east) and every face's texture flipped left to right.
    #[test]
    fn a_mirrored_cube_swaps_its_sides_and_flips_its_faces() {
        let mut rt = Rt::new();
        crate::install();
        let layer = head_layer(&mut rt, true);
        let root = crate::api::call(&mut rt, BAKE_ROOT, &[layer]);
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let c = consumer(&mut rt, "mymod:textures/entity/golem.png");
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| crate::api::call(rt, RENDER, &[root, stack, c, V::I(0), V::I(0)]));
        assert_eq!(host.drawn.len(), 2);
        let mirrored = host.drawn.iter().find(|d| d.1[12] == 1.0).expect("the mirrored part, 16 pixels east");
        let drawn = faces(quads(&mirrored.0), [64.0, 32.0]);
        let side = |facing| drawn.iter().find(|f| f.0 == facing).unwrap();
        assert_eq!(side(EAST).1, [0.0, 8.0, 8.0, 16.0]);
        assert!(side(EAST).2.iter().all(|c| c[0] == 4.0));
        assert_eq!(side(WEST).1, [16.0, 8.0, 24.0, 16.0]);
        assert!(side(WEST).2.iter().all(|c| c[0] == -4.0));
        // The front's least u (8) is now on the side of the greatest x.
        let front = quads(&mirrored.0).iter().find(|q| q.facing == NORTH).unwrap();
        let at_least_u: Vec<f32> = front.positions.iter().zip(front.uv).filter(|(_, uv)| uv[0] * 64.0 == 8.0).map(|(p, _)| p[0] * 16.0).collect();
        assert_eq!(at_least_u, [4.0, 4.0]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A part's rotation turns it about its pivot as JOML's `rotationZYX` does: a quarter turn
    /// about y takes x to -z.
    #[test]
    fn a_rotated_part_turns_about_its_pivot() {
        let mut rt = Rt::new();
        crate::install();
        let mesh = rt.new_object(MESH);
        crate::api::call(&mut rt, MESH_INIT, &[mesh]);
        let root = crate::api::call(&mut rt, GET_ROOT, &[mesh]);
        let cubes = crate::api::call(&mut rt, CREATE, &[]);
        crate::api::call(&mut rt, ADD_BOX, &[cubes, V::F(0.0), V::F(0.0), V::F(0.0), V::F(1.0), V::F(1.0), V::F(1.0)]);
        let half_pi = std::f32::consts::FRAC_PI_2;
        let pose = crate::api::call(&mut rt, OFFSET_ROTATION, &[V::F(16.0), V::F(0.0), V::F(0.0), V::F(0.0), V::F(half_pi), V::F(0.0)]);
        let name = rt.string("arm");
        crate::api::call(&mut rt, ADD_CHILD, &[root, name, cubes, pose]);
        let layer = crate::api::call(&mut rt, LAYER_CREATE, &[mesh, V::I(16), V::I(16)]);
        let root = crate::api::call(&mut rt, BAKE_ROOT, &[layer]);
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let c = consumer(&mut rt, "minecraft:textures/entity/pig/pig.png");
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| crate::api::call(rt, RENDER, &[root, stack, c, V::I(0), V::I(0)]));
        let (model, pose, ..) = &host.drawn[0];
        assert!(quads(model).iter().all(|q| q.sprite == "entity/pig/pig"));
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        // Column 0 is where x goes: to -z. Column 3 is the pivot, a block east.
        assert!(near(pose[0], 0.0) && near(pose[1], 0.0) && near(pose[2], -1.0), "{pose:?}");
        assert!(near(pose[8], 1.0) && near(pose[10], 0.0), "{pose:?}");
        assert!(near(pose[12], 1.0) && near(pose[13], 0.0) && near(pose[14], 0.0), "{pose:?}");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    fn supplier(rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
        Some(head_layer(rt, false))
    }

    /// A layer its mod registered (`RegisterLayerDefinitions`) bakes afresh for each renderer that
    /// asks; one nobody registered (vanilla's) bakes as an empty part, its children empty too, and
    /// draws nothing.
    #[test]
    fn a_registered_layer_bakes_for_each_renderer_and_an_unknown_one_is_empty() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(supplier);
        let location = |rt: &mut Rt, model: &str| {
            let l = rt.new_object("net/minecraft/client/model/geom/ModelLayerLocation");
            let (m, layer) = (rt.string(model), rt.string("main"));
            crate::api::call(rt, LAYER_INIT, &[l, m, layer]);
            l
        };
        let golem = location(&mut rt, "mymod:golem");
        let event = rt.new_object(REGISTER_LAYERS);
        let made = rt.lambda_id("java/util/function/Supplier", 1, Vec::new());
        crate::api::call(&mut rt, REGISTER, &[event, golem, made]);
        let context = rt.new_object("net/minecraft/client/renderer/entity/EntityRendererProvider$Context");
        let again = location(&mut rt, "mymod:golem");
        let (first, second) = (crate::api::call(&mut rt, CONTEXT_BAKE, &[context, golem]), crate::api::call(&mut rt, CONTEXT_BAKE, &[context, again]));
        assert_ne!(first, second, "each renderer its own parts");
        let name = rt.string("head");
        let head = crate::api::call(&mut rt, GET_CHILD, &[second, name]);
        assert_eq!(rt.get_field(head, "y"), V::F(6.0));
        let zombie = location(&mut rt, "minecraft:zombie");
        let empty = crate::api::call(&mut rt, CONTEXT_BAKE, &[context, zombie]);
        let name = rt.string("left_arm");
        let arm = crate::api::call(&mut rt, GET_CHILD, &[empty, name]);
        assert_ne!(arm, V::NULL);
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let c = consumer(&mut rt, "minecraft:textures/entity/zombie/zombie.png");
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| crate::api::call(rt, RENDER, &[empty, stack, c, V::I(0), V::I(0)]));
        assert!(host.drawn.is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        // The mod's model keeps its root in a field and answers `root()` with it.
        if short == ROOT {
            return rt.get_field(args[0], "root");
        }
        crate::api::call(rt, key, args)
    }

    /// A mod's `HierarchicalModel` draws its root (`root()`, the mod's own) when its renderer calls
    /// `renderToBuffer`, in white unless the renderer gives a colour; its render type is
    /// `entityCutoutNoCull` of the texture unless the model was made with another.
    #[test]
    fn a_hierarchical_model_draws_its_root() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.supers.insert("mymod/GolemModel", HIERARCHICAL);
        let model = rt.new_object("mymod/GolemModel");
        crate::api::call(&mut rt, "net/minecraft/client/model/HierarchicalModel.<init>.()V", &[model]);
        assert_eq!(rt.get_field(model, "young"), V::I(1));
        let layer = head_layer(&mut rt, false);
        let root = crate::api::call(&mut rt, BAKE_ROOT, &[layer]);
        rt.put_field(model, "root", root);
        let texture = rt.string("mymod:textures/entity/golem.png");
        let ty = crate::api::call(&mut rt, "net/minecraft/client/model/Model.renderType.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/renderer/RenderType;", &[model, texture]);
        let named = crate::api::call(&mut rt, ENTITY_CUTOUT, &[texture]);
        assert_eq!(ty, named);
        let buffers = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        let c = crate::api::call(&mut rt, GET_BUFFER, &[buffers, ty]);
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            crate::api::call(rt, "net/minecraft/client/model/Model.renderToBuffer.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;II)V", &[model, stack, c, V::I(0), V::I(0)]);
            crate::api::call(rt, RENDER_5_KEY, &[model, stack, c, V::I(0), V::I(0), V::I(0x80FF_0000u32 as i32)]);
        });
        assert_eq!(host.drawn.len(), 2);
        assert_eq!(host.drawn[0].2, u32::MAX);
        assert_eq!(host.drawn[1].2, 0x80FF_0000);
        assert_eq!(quads(&host.drawn[0].0).len(), 6);
        let head = rt.string("head");
        let found = crate::api::call(&mut rt, "net/minecraft/client/model/HierarchicalModel.getAnyDescendantWithName.(Ljava/lang/String;)Ljava/util/Optional;", &[model, head]);
        assert_ne!(rt.get_field(found, "value"), V::NULL);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const CREATE_MESH: &str = "net/minecraft/client/model/HumanoidModel.createMesh.(Lnet/minecraft/client/model/geom/builders/CubeDeformation;F)Lnet/minecraft/client/model/geom/builders/MeshDefinition;";
    const HUMANOID_INIT: &str = "net/minecraft/client/model/HumanoidModel.<init>.(Lnet/minecraft/client/model/geom/ModelPart;)V";
    const SETUP_ANIM: &str = "net/minecraft/client/model/EntityModel.setupAnim.(Lnet/minecraft/world/entity/Entity;FFFFF)V";

    /// The face of `quads` looking `facing`, as [`faces`] gives it on a 64 by 64 texture.
    fn face(quads: &[Quad], facing: u8) -> [f32; 4] {
        faces(quads, [64.0, 64.0]).into_iter().find(|f| f.0 == facing).map(|f| f.1).expect("face")
    }

    /// `HumanoidModel.createMesh` is the player's shape on the 64 by 64 skin: the head drawn first,
    /// then the body, arms, legs and hat; the right arm's front on the skin's 44..48 by 20..32 and
    /// its pivot at the shoulder (-5, 2, 0); the left arm the right one's texture mirrored (its
    /// outer side, east, drawn from the right arm's west rectangle).
    #[test]
    fn a_humanoid_mesh_is_the_player_skin_layout() {
        let mut rt = Rt::new();
        crate::install();
        let none = crate::api::get_static(&mut rt, "net/minecraft/client/model/geom/builders/CubeDeformation.NONE");
        let mesh = crate::api::call(&mut rt, CREATE_MESH, &[none, V::F(0.0)]);
        let layer = crate::api::call(&mut rt, LAYER_CREATE, &[mesh, V::I(64), V::I(64)]);
        let root = crate::api::call(&mut rt, BAKE_ROOT, &[layer]);
        let model = rt.new_object(HUMANOID);
        crate::api::call(&mut rt, HUMANOID_INIT, &[model, root]);
        rt.put_field(model, "young", V::I(0));
        let c = consumer(&mut rt, "minecraft:textures/entity/zombie/zombie.png");
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| crate::api::call(rt, RENDER_5_KEY, &[model, stack, c, V::I(0), V::I(0), V::I(WHITE)]));
        assert_eq!(host.drawn.len(), 7);
        let (head, right_arm, left_arm, left_leg) = (quads(&host.drawn[0].0), quads(&host.drawn[2].0), quads(&host.drawn[3].0), quads(&host.drawn[5].0));
        assert_eq!(face(head, NORTH), [8.0, 8.0, 16.0, 16.0]);
        assert_eq!(face(right_arm, NORTH), [44.0, 20.0, 48.0, 32.0]);
        assert_eq!(face(right_arm, WEST), [40.0, 20.0, 44.0, 32.0]);
        assert_eq!(face(left_arm, EAST), [40.0, 20.0, 44.0, 32.0]);
        assert_eq!(face(left_leg, NORTH), [4.0, 20.0, 8.0, 32.0]);
        let (arm_pose, leg_pose) = (host.drawn[2].1, host.drawn[5].1);
        assert_eq!([arm_pose[12], arm_pose[13]], [-5.0 / 16.0, 2.0 / 16.0]);
        assert_eq!([leg_pose[12], leg_pose[13]], [1.9 / 16.0, 12.0 / 16.0]);
        assert!(host.drawn.iter().all(|d| quads(&d.0).iter().all(|q| q.sprite == "entity/zombie/zombie")));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A young humanoid's head is three quarters size and a block lower (16 pixels, scaled), its
    /// body half size and 24 pixels lower, as `AgeableListModel` draws a baby.
    #[test]
    fn a_young_humanoid_has_a_big_head_on_a_small_body() {
        let mut rt = Rt::new();
        crate::install();
        let model = tests_support::humanoid(&mut rt);
        assert_eq!(rt.get_field(model, "young"), V::I(1));
        let c = consumer(&mut rt, "minecraft:textures/entity/zombie/zombie.png");
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| crate::api::call(rt, RENDER_5_KEY, &[model, stack, c, V::I(0), V::I(0), V::I(WHITE)]));
        let (head, body, right_arm) = (host.drawn[0].1, host.drawn[1].1, host.drawn[2].1);
        assert_eq!([head[0], head[13]], [0.75, 0.75]);
        assert_eq!([body[0], body[13]], [0.5, 0.75]);
        assert_eq!([right_arm[12], right_arm[13]], [0.5 * -5.0 / 16.0, 0.75 + 0.5 * 2.0 / 16.0]);
    }

    /// `setupAnim` on a humanoid, through `EntityModel`'s bridge: the head looks where it is told,
    /// the legs swing with the walk, and a right-handed attack (`attackTime` 0.3) twists the body
    /// and sweeps the right arm, which then bobs with age, as vanilla's formulas give.
    #[test]
    fn a_humanoid_walks_looks_and_swings() {
        let mut rt = Rt::new();
        crate::install();
        let model = tests_support::humanoid(&mut rt);
        rt.put_field(model, "attackTime", V::F(0.3));
        let e = rt.new_object("mymod/Golem");
        let (limb, amount, age, yaw, pitch) = (0.5f64, 0.8f64, 10.0f64, 20.0f64, -10.0f64);
        crate::api::call(&mut rt, SETUP_ANIM, &[model, e, V::F(limb as f32), V::F(amount as f32), V::F(age as f32), V::F(yaw as f32), V::F(pitch as f32)]);
        let part = |rt: &Rt, f: &'static str| rt.get_field(model, f);
        let rot = |rt: &Rt, p: V| ["xRot", "yRot", "zRot"].map(|f| f64::from(rt.get_field(p, f).f()));
        let close = |got: f64, want: f64| assert!((got - want).abs() < 1e-3, "{got} != {want}");
        let pi = std::f64::consts::PI;
        let head = rot(&rt, part(&rt, "head"));
        close(head[0], pitch.to_radians());
        close(head[1], yaw.to_radians());
        close(rot(&rt, part(&rt, "rightLeg"))[0], (limb * 0.6662).cos() * 1.4 * amount);
        close(rot(&rt, part(&rt, "leftLeg"))[0], (limb * 0.6662 + pi).cos() * 1.4 * amount);
        let a = 0.3f64;
        let body = (a.sqrt() * 2.0 * pi).sin() * 0.2;
        close(rot(&rt, part(&rt, "body"))[1], body);
        let f = 1.0 - (1.0 - a).powi(4);
        let sweep = (f * pi).sin() * 1.2 + (a * pi).sin() * -(pitch.to_radians() - 0.7) * 0.75;
        let right = rot(&rt, part(&rt, "rightArm"));
        close(right[0], (limb * 0.6662 + pi).cos() * amount - sweep + (age * 0.067).sin() * 0.05);
        close(right[1], 3.0 * body);
        close(right[2], (a * pi).sin() * -0.4 + (age * 0.09).cos() * 0.05 + 0.05);
        let arm = rt.get_field(model, "rightArm");
        close(f64::from(rt.get_field(arm, "x").f()), -body.cos() * 5.0);
        let left = rot(&rt, part(&rt, "leftArm"));
        close(left[0], (limb * 0.6662).cos() * amount + body - (age * 0.067).sin() * 0.05);
        close(left[1], body);
        // The hat follows the head.
        assert_eq!(rot(&rt, part(&rt, "hat")), head);
    }

    fn aggressive_vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if short == "isAggressive.()Z" {
            return V::I(1);
        }
        crate::api::call(rt, key, args)
    }

    /// A zombie model holds its arms out, higher while its zombie is aggressive (`-pi / 1.5` rather
    /// than `-pi / 2.25`), over the humanoid's walk: `EntityModel`'s `setupAnim` reaches
    /// `AbstractZombieModel`'s through vanilla's bridges.
    #[test]
    fn a_zombie_model_raises_its_arms() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(aggressive_vcall);
        let humanoid = tests_support::humanoid(&mut rt);
        let model = rt.new_object(ZOMBIE);
        for (f, _) in HUMANOID_PARTS {
            let p = rt.get_field(humanoid, f);
            rt.put_field(model, f, p);
        }
        for f in ["attackTime", "riding", "young", "leftArmPose", "rightArmPose", "crouching", "swimAmount"] {
            let v = rt.get_field(humanoid, f);
            rt.put_field(model, f, v);
        }
        let e = rt.new_object("mymod/Ghoul");
        crate::api::call(&mut rt, SETUP_ANIM, &[model, e, V::F(0.0), V::F(0.0), V::F(0.0), V::F(0.0), V::F(0.0)]);
        let arm = rt.get_field(model, "rightArm");
        // At age 0 the bob adds sin(0) to x.
        let want = -std::f32::consts::PI / 1.5;
        assert!((rt.get_field(arm, "xRot").f() - want).abs() < 1e-4, "{}", rt.get_field(arm, "xRot").f());
        assert!((rt.get_field(arm, "yRot").f() - -0.1).abs() < 1e-6);
    }
}
