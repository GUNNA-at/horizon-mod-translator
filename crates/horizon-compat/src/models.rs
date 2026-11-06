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

/// The sprite a vertex consumer draws with: its render type's texture. None when it has none.
fn consumer_sprite(rt: &Rt, consumer: V) -> Option<String> {
    let ty = rt.get_field(consumer, "$type");
    let texture = rt.str_of(rt.get_field(ty, "$texture"))?;
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
        // A hierarchical model draws its root; another model's is abstract (its mod draws it).
        "renderToBuffer" => {
            if rt.instance_of(this, HIERARCHICAL) {
                let root = crate::api::virtual_call(rt, (ROOT_KEY, ROOT), &[this]);
                render_part(rt, root, a(1), a(2), a(3).i(), a(5).i());
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

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
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
    )
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
        (MODEL | ENTITY_MODEL | HIERARCHICAL, _) => return model_call(rt, owner, name, desc, args),
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
}
