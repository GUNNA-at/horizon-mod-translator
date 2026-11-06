//! The client's camera as vanilla's `Camera` keeps it, and the `GameRenderer` that holds it: where a
//! frame is drawn from and which way it looks, for the mods that draw relative to it, turn a
//! billboard to face it or measure a distance from it (`GameRenderer.getMainCamera`,
//! `EntityRenderDispatcher.camera` and `cameraOrientation`, `BlockEntityRenderDispatcher.camera`,
//! `Minecraft.getCameraEntity`); the getters of the events that hand a camera over (NeoForge's
//! viewport events, `RenderLevelStageEvent`, `RenderHighlightEvent`), and the level's render stages
//! the game passes each frame ([`render_stages`]).
//!
//! The main camera follows the game: set up on the client's player while the client is in a world,
//! at the place the game drew its last frame from ([`prepare`]; the player's eyes until it has
//! drawn one), turned to the player's look as `Camera.setRotation` turns it. The view is first
//! person: the game doesn't say when it is another.
use crate::{Data, Rt, V};

const CAMERA: &str = "net/minecraft/client/Camera";
const NEAR_PLANE: &str = "net/minecraft/client/Camera$NearPlane";
const GAME_RENDERER: &str = "net/minecraft/client/renderer/GameRenderer";
const MINECRAFT: &str = "net/minecraft/client/Minecraft";
const ENTITY_DISPATCHER: &str = "net/minecraft/client/renderer/entity/EntityRenderDispatcher";
const BLOCK_ENTITY_DISPATCHER: &str = "net/minecraft/client/renderer/blockentity/BlockEntityRenderDispatcher";
const LIGHT_TEXTURE: &str = "net/minecraft/client/renderer/LightTexture";
const OVERLAY_TEXTURE: &str = "net/minecraft/client/renderer/texture/OverlayTexture";
const SHADER: &str = "net/minecraft/client/renderer/ShaderInstance";
const UNIFORM: &str = "com/mojang/blaze3d/shaders/Uniform";
const ABSTRACT_UNIFORM: &str = "com/mojang/blaze3d/shaders/AbstractUniform";
const RENDER_SYSTEM: &str = "com/mojang/blaze3d/systems/RenderSystem";
const REGISTER_SHADERS: &str = "net/neoforged/neoforge/client/event/RegisterShadersEvent";
const MUTABLE_POS: &str = "net/minecraft/core/BlockPos$MutableBlockPos";
const SET_BLOCK: &str = "net/minecraft/core/BlockPos$MutableBlockPos.set.(III)Lnet/minecraft/core/BlockPos$MutableBlockPos;";
const FOG_TYPE: &str = "net/minecraft/world/level/material/FogType";
/// `FogType`'s constants, in its order.
const FOG_TYPES: [&str; 4] = ["LAVA", "WATER", "POWDER_SNOW", "NONE"];
const FLUID_IS: &str = "net/minecraft/world/level/material/FluidState.is.(Lnet/minecraft/tags/TagKey;)Z";
const FLUID_HEIGHT: &str = "net/minecraft/world/level/material/FluidState.getHeight.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
const AMBIENT_LIGHT: &str = "net/minecraft/world/level/dimension/DimensionType.ambientLight.()F";
const VIEWPORT: &str = "net/neoforged/neoforge/client/event/ViewportEvent";
const COMPUTE_ANGLES: &str = "net/neoforged/neoforge/client/event/ViewportEvent$ComputeCameraAngles";
const COMPUTE_FOV: &str = "net/neoforged/neoforge/client/event/ViewportEvent$ComputeFov";
const FOG_COLOR: &str = "net/neoforged/neoforge/client/event/ViewportEvent$ComputeFogColor";
const RENDER_FOG: &str = "net/neoforged/neoforge/client/event/ViewportEvent$RenderFog";
const VIEWPORT_EVENTS: [&str; 5] = [VIEWPORT, COMPUTE_ANGLES, COMPUTE_FOV, FOG_COLOR, RENDER_FOG];
const LEVEL_STAGE: &str = "net/neoforged/neoforge/client/event/RenderLevelStageEvent";
const STAGE: &str = "net/neoforged/neoforge/client/event/RenderLevelStageEvent$Stage";
/// `RenderLevelStageEvent.Stage`'s stages, in the order a frame passes them.
const STAGES: [&str; 11] = [
    "AFTER_SKY",
    "AFTER_SOLID_BLOCKS",
    "AFTER_CUTOUT_MIPPED_BLOCKS_BLOCKS",
    "AFTER_CUTOUT_BLOCKS",
    "AFTER_ENTITIES",
    "AFTER_BLOCK_ENTITIES",
    "AFTER_TRANSLUCENT_BLOCKS",
    "AFTER_TRIPWIRE_BLOCKS",
    "AFTER_PARTICLES",
    "AFTER_WEATHER",
    "AFTER_LEVEL",
];
/// Their names (`toString`): the vanilla location each was registered by.
const STAGE_NAMES: [&str; 11] = [
    "after_sky",
    "after_solid_blocks",
    "after_cutout_mipped_blocks",
    "after_cutout_blocks",
    "after_entities",
    "after_block_entities",
    "after_translucent_blocks",
    "after_tripwire_blocks",
    "after_particles",
    "after_weather",
    "after_level",
];
/// The block layers a stage follows (`Stage.fromRenderType`), by layer and stage.
const LAYER_STAGES: [(&str, usize); 5] = [("solid", 1), ("cutout_mipped", 2), ("cutout", 3), ("translucent", 6), ("tripwire", 7)];
const HIGHLIGHT: &str = "net/neoforged/neoforge/client/event/RenderHighlightEvent";
const HIGHLIGHT_BLOCK: &str = "net/neoforged/neoforge/client/event/RenderHighlightEvent$Block";
const HIGHLIGHT_ENTITY: &str = "net/neoforged/neoforge/client/event/RenderHighlightEvent$Entity";
const HIGHLIGHT_EVENTS: [&str; 3] = [HIGHLIGHT, HIGHLIGHT_BLOCK, HIGHLIGHT_ENTITY];
const EVENT: &str = "net/neoforged/bus/api/Event";
/// Where the game drew its last frame from: a `Vec3` [`prepare`] keeps.
const FRAME: &str = "compat:frame-camera";
/// `Camera`'s `FORWARDS`, `UP` and `LEFT`, which its rotation turns.
const FORWARDS: [f32; 3] = [0.0, 0.0, -1.0];
const UP: [f32; 3] = [0.0, 1.0, 0.0];
const LEFT: [f32; 3] = [-1.0, 0.0, 0.0];
/// `(float) Math.PI` and `(float) (Math.PI / 180.0)`.
const PI: f32 = std::f64::consts::PI as f32;
const DEG: f32 = (std::f64::consts::PI / 180.0) as f32;
/// The options the game doesn't hand over, at vanilla's defaults: the field of view in degrees and
/// the render distance in chunks.
const FOV: f64 = 70.0;
const RENDER_DISTANCE: i32 = 12;
/// `GameRenderer.PROJECTION_Z_NEAR`.
const Z_NEAR: f32 = 0.05;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

/// The client's one object of `class`, as the other client modules hand it out.
fn single(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("client/{class}"), |rt| rt.new_object(class))
}

/// Vanilla's classes past the ones a mod's code names, as the API climbs them.
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    match class {
        COMPUTE_ANGLES | COMPUTE_FOV | FOG_COLOR | RENDER_FOG => Some(VIEWPORT),
        HIGHLIGHT_BLOCK | HIGHLIGHT_ENTITY => Some(HIGHLIGHT),
        VIEWPORT | HIGHLIGHT | LEVEL_STAGE => Some(EVENT),
        UNIFORM => Some(ABSTRACT_UNIFORM),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        CAMERA | NEAR_PLANE | GAME_RENDERER | SHADER | UNIFORM | ABSTRACT_UNIFORM | REGISTER_SHADERS | LEVEL_STAGE => true,
        STAGE => matches!(name, "toString" | "fromRenderType"),
        _ if VIEWPORT_EVENTS.contains(&owner) || HIGHLIGHT_EVENTS.contains(&owner) => true,
        MINECRAFT => matches!(name, "getCameraEntity" | "setCameraEntity"),
        ENTITY_DISPATCHER => matches!(name, "cameraOrientation" | "overrideCameraOrientation" | "prepare" | "distanceToSqr"),
        BLOCK_ENTITY_DISPATCHER => name == "prepare",
        LIGHT_TEXTURE => matches!(name, "<init>" | "turnOnLightLayer" | "turnOffLightLayer" | "updateLightTexture" | "tick" | "close" | "getBrightness" | "getDarknessGamma"),
        OVERLAY_TEXTURE => matches!(name, "setupOverlayColor" | "teardownOverlayColor"),
        RENDER_SYSTEM => name == "getShader" && desc.starts_with("()"),
        _ => false,
    }
}

// ---- JOML's arithmetic, as vanilla's camera runs it ----

/// JOML's `Math.sin` (no fast math): the float angle's sine through doubles.
fn sin(angle: f32) -> f32 {
    f64::from(angle).sin() as f32
}

/// JOML's `Math.cosFromSin`: the cosine from the sine, its sign from the angle's quadrant.
fn cos_from_sin(sin: f32, angle: f32) -> f32 {
    let cos = f64::from(1.0 - sin * sin).sqrt() as f32;
    let (half_pi, two_pi) = ((std::f64::consts::PI * 0.5) as f32, (std::f64::consts::PI * 2.0) as f32);
    let a = angle + half_pi;
    let mut b = a - (a / two_pi) as i32 as f32 * two_pi;
    if b < 0.0 {
        b += two_pi;
    }
    if b >= PI { -cos } else { cos }
}

/// `Quaternionf.rotationYXZ(angleY, angleX, angleZ)`: about z, then x, then y.
fn rotation_yxz(angle_y: f32, angle_x: f32, angle_z: f32) -> [f32; 4] {
    let (sx, sy, sz) = (sin(angle_x * 0.5), sin(angle_y * 0.5), sin(angle_z * 0.5));
    let (cx, cy, cz) = (cos_from_sin(sx, angle_x * 0.5), cos_from_sin(sy, angle_y * 0.5), cos_from_sin(sz, angle_z * 0.5));
    let (x, y, z, w) = (cy * sx, sy * cx, sy * sx, cy * cx);
    [x * cz + y * sz, y * cz - x * sz, w * sz - z * cz, w * cz + z * sz]
}

/// `Quaternionf.transform(x, y, z, dest)` (what `Vector3f.rotate(q)` runs): `v` turned by `q`.
fn transform([qx, qy, qz, qw]: [f32; 4], [x, y, z]: [f32; 3]) -> [f32; 3] {
    let (xx, yy, zz, ww) = (qx * qx, qy * qy, qz * qz, qw * qw);
    let (xy, xz, yz, xw, zw, yw) = (qx * qy, qx * qz, qy * qz, qx * qw, qz * qw, qy * qw);
    let k = 1.0 / (xx + yy + zz + ww);
    [
        (xx - yy - zz + ww) * k * x + (2.0 * (xy - zw) * k * y + 2.0 * (xz + yw) * k * z),
        2.0 * (xy + zw) * k * x + ((yy - xx - zz + ww) * k * y + 2.0 * (yz - xw) * k * z),
        2.0 * (xz - yw) * k * x + (2.0 * (yz + xw) * k * y + (zz - xx - yy + ww) * k * z),
    ]
}

fn floats3(rt: &Rt, v: V) -> [f32; 3] {
    ["x", "y", "z"].map(|f| rt.get_field(v, f).f())
}

fn set_floats3(rt: &mut Rt, v: V, xyz: [f32; 3]) {
    for (f, c) in ["x", "y", "z"].into_iter().zip(xyz) {
        rt.put_field(v, f, V::F(c));
    }
}

fn vector3f(rt: &mut Rt, xyz: [f32; 3]) -> V {
    let v = rt.new_object("org/joml/Vector3f");
    set_floats3(rt, v, xyz);
    v
}

fn vec3(rt: &mut Rt, [x, y, z]: [f64; 3]) -> V {
    crate::api_common::vec3(rt, x, y, z)
}

fn doubles3(rt: &Rt, v: V) -> [f64; 3] {
    crate::api_common::xyz(rt, v)
}

// ---- the camera ----

/// `new Camera()`: not set up, at the origin, unturned (looking north).
fn init_camera(rt: &mut Rt, cam: V) {
    let rotation = crate::render::new_quat(rt, [0.0, 0.0, 0.0, 1.0]);
    rt.put_field(cam, "$rotation", rotation);
    for (field, v) in [("$forwards", FORWARDS), ("$up", UP), ("$left", LEFT)] {
        let o = vector3f(rt, v);
        rt.put_field(cam, field, o);
    }
    let origin = vec3(rt, [0.0; 3]);
    rt.put_field(cam, "$position", origin);
    let block = rt.alloc(MUTABLE_POS, Data::Native("BlockPos", [0; 4]));
    rt.put_field(cam, "$blockPosition", block);
    for f in ["$initialized", "$detached"] {
        rt.put_field(cam, f, V::I(0));
    }
    for f in ["$xRot", "$yRot", "$roll", "$partialTickTime"] {
        rt.put_field(cam, f, V::F(0.0));
    }
}

/// `Camera.setRotation(yRot, xRot, roll)`: the angles kept, the rotation
/// `rotationYXZ(PI - yRot, -xRot, -roll)` (in radians), and the forward, up and left vectors turned
/// by it, each in its own object as vanilla's final fields are.
fn set_rotation(rt: &mut Rt, cam: V, y_rot: f32, x_rot: f32, roll: f32) {
    for (f, v) in [("$xRot", x_rot), ("$yRot", y_rot), ("$roll", roll)] {
        rt.put_field(cam, f, V::F(v));
    }
    let q = rotation_yxz(PI - y_rot * DEG, -x_rot * DEG, -roll * DEG);
    let rotation = rt.get_field(cam, "$rotation");
    for (f, c) in ["x", "y", "z", "w"].into_iter().zip(q) {
        rt.put_field(rotation, f, V::F(c));
    }
    for (field, v) in [("$forwards", FORWARDS), ("$up", UP), ("$left", LEFT)] {
        let o = rt.get_field(cam, field);
        set_floats3(rt, o, transform(q, v));
    }
}

/// `Camera.setPosition`: the place (a new `Vec3` when it moved), and the block it is in.
fn set_position(rt: &mut Rt, cam: V, at: [f64; 3]) {
    let old = rt.get_field(cam, "$position");
    if old.r() == 0 || doubles3(rt, old) != at {
        let v = vec3(rt, at);
        rt.put_field(cam, "$position", v);
    }
    let block = rt.get_field(cam, "$blockPosition");
    let [x, y, z] = at.map(|c| c.floor() as i32);
    crate::api::call(rt, SET_BLOCK, &[block, V::I(x), V::I(y), V::I(z)]);
}

/// The main camera (`GameRenderer.getMainCamera`), brought up to the game (the module's comment).
pub(crate) fn main(rt: &mut Rt) -> V {
    let cam = rt.intern(format!("client/{CAMERA}"), |rt| {
        let c = rt.new_object(CAMERA);
        init_camera(rt, c);
        rt.put_field(c, "$main", V::I(1));
        c
    });
    follow(rt, cam);
    cam
}

/// Sets the main camera up as the game stands: on the client's player in a world (vanilla's
/// `Camera.setup` in first person), else reset (`Camera.reset`: no level, no entity, not set up).
fn follow(rt: &mut Rt, cam: V) {
    let in_world = crate::neo::is_client(rt) && crate::world_events::client_in_world(rt);
    let place = if in_world { rt.host().and_then(|h| h.player_place()) } else { None };
    let Some((feet, yaw, pitch)) = place else {
        rt.put_field(cam, "$level", V::NULL);
        rt.put_field(cam, "$entity", V::NULL);
        rt.put_field(cam, "$initialized", V::I(0));
        return;
    };
    let level = crate::glue::level(rt);
    let player = crate::network::local_player(rt);
    let partial = rt.host().map_or(0.0, |h| h.partial_tick());
    for (f, v) in [("$level", level), ("$entity", player), ("$initialized", V::I(1)), ("$detached", V::I(0)), ("$partialTickTime", V::F(partial))] {
        rt.put_field(cam, f, v);
    }
    set_rotation(rt, cam, yaw, pitch, 0.0);
    let at = match rt.interned.get(FRAME).copied() {
        Some(frame) => doubles3(rt, frame),
        None => [feet[0], feet[1] + crate::api_common::player_eye_height(rt), feet[2]],
    };
    set_position(rt, cam, at);
}

/// Before the game's renderers draw the mods' things: the main camera brought up to the frame,
/// drawn from `at` when the hook says where (from then on the camera's place), and handed to the
/// render dispatchers as vanilla's `EntityRenderDispatcher.prepare` and
/// `BlockEntityRenderDispatcher.prepare` hand it each frame (their `camera`, the orientation that
/// faces it).
pub(crate) fn prepare(rt: &mut Rt, at: Option<[f64; 3]>) {
    if let Some(at) = at {
        let frame = rt.intern(FRAME.into(), |rt| vec3(rt, [0.0; 3]));
        for (f, c) in ["x", "y", "z"].into_iter().zip(at) {
            rt.put_field(frame, f, V::D(c));
        }
    }
    let cam = main(rt);
    let level = rt.get_field(cam, "$level");
    for class in [ENTITY_DISPATCHER, BLOCK_ENTITY_DISPATCHER] {
        let d = single(rt, class);
        rt.put_field(d, "camera", cam);
        if level.r() != 0 {
            rt.put_field(d, "level", level);
        }
    }
    let rotation = rt.get_field(cam, "$rotation");
    let d = single(rt, ENTITY_DISPATCHER);
    rt.put_field(d, "$cameraOrientation", rotation);
}

/// The client left its world: the next one's frames place the camera afresh.
pub(crate) fn forget_frame(rt: &mut Rt) {
    rt.interned.remove(FRAME);
}

/// `Camera.setup(level, entity, detached, thirdPersonReverse, partialTick)` on a camera a mod made:
/// at the entity's eyes, turned to its look (behind it, or before it and turned round, when
/// detached; four blocks off, without the main camera's check for walls in between).
fn setup(rt: &mut Rt, cam: V, args: &[V]) {
    let a = |i| arg(args, i);
    let (level, entity, detached, reverse, partial) = (a(1), a(2), a(3).i() != 0, a(4).i() != 0, a(5).f());
    for (f, v) in [("$initialized", V::I(1)), ("$level", level), ("$entity", entity), ("$detached", V::I(i32::from(detached))), ("$partialTickTime", V::F(partial))] {
        rt.put_field(cam, f, v);
    }
    let key = |name: &str, desc: &str| -> &'static str { Box::leak(format!("net/minecraft/world/entity/Entity.{name}.{desc}").into_boxed_str()) };
    let yaw = crate::api::call(rt, key("getViewYRot", "(F)F"), &[entity, V::F(partial)]).f();
    let pitch = crate::api::call(rt, key("getViewXRot", "(F)F"), &[entity, V::F(partial)]).f();
    set_rotation(rt, cam, yaw, pitch, 0.0);
    let eye = crate::api::call(rt, key("getEyePosition", "(F)Lnet/minecraft/world/phys/Vec3;"), &[entity, V::F(partial)]);
    let at = if eye.r() != 0 { doubles3(rt, eye) } else { [0.0; 3] };
    set_position(rt, cam, at);
    if detached {
        if reverse {
            let (y, x, r) = (rt.get_field(cam, "$yRot").f(), rt.get_field(cam, "$xRot").f(), rt.get_field(cam, "$roll").f());
            set_rotation(rt, cam, y + 180.0, -x, -r);
        }
        move_camera(rt, cam, -4.0, 0.0, 0.0);
    }
}

/// `Camera.move(zoom, dy, dx)`: along its own axes (forward `zoom`, up `dy`, left `dx`).
fn move_camera(rt: &mut Rt, cam: V, zoom: f32, dy: f32, dx: f32) {
    let rotation = rt.get_field(cam, "$rotation");
    let q = ["x", "y", "z", "w"].map(|f| rt.get_field(rotation, f).f());
    let d = transform(q, [dx, dy, -zoom]);
    let p = doubles3(rt, rt.get_field(cam, "$position"));
    set_position(rt, cam, [p[0] + f64::from(d[0]), p[1] + f64::from(d[1]), p[2] + f64::from(d[2])]);
}

/// The window's width over its height (`Window.getWidth() / getHeight()`, as floats).
fn aspect(rt: &Rt) -> f32 {
    let size = rt.host().map_or([0, 0], |h| h.gui_size());
    let scale = rt.host().map_or(1.0, |h| h.gui_scale());
    let [w, h] = size.map(|s| (f64::from(s) * scale) as i32);
    if h <= 0 { 1.0 } else { w as f32 / h as f32 }
}

/// `Camera.getNearPlane`: the near plane's centre (0.05 ahead), and how far its edges reach left and
/// up, from the field of view and the window's shape.
fn near_plane_vectors(rt: &Rt, cam: V) -> [[f64; 3]; 3] {
    let up_reach = (f64::from(FOV as f32 * DEG) / 2.0).tan() * f64::from(0.05f32);
    let left_reach = up_reach * f64::from(aspect(rt));
    let scaled = |field: &'static str, by: f64| floats3(rt, rt.get_field(cam, field)).map(|c| f64::from(c) * by);
    [scaled("$forwards", f64::from(0.05f32)), scaled("$left", left_reach), scaled("$up", up_reach)]
}

fn near_plane(rt: &mut Rt, cam: V) -> V {
    let vectors = near_plane_vectors(rt, cam);
    let plane = rt.new_object(NEAR_PLANE);
    for (field, v) in ["forward", "left", "up"].into_iter().zip(vectors) {
        let o = vec3(rt, v);
        rt.put_field(plane, field, o);
    }
    plane
}

/// `a + b * k` for each axis.
fn add_scaled(a: [f64; 3], b: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

/// The near plane's centre and four corners: `forward ± up ± left`, top left first.
fn plane_points([forward, left, up]: [[f64; 3]; 3]) -> [[f64; 3]; 5] {
    let corner = |u: f64, l: f64| add_scaled(add_scaled(forward, up, u), left, l);
    [forward, corner(1.0, 1.0), corner(1.0, -1.0), corner(-1.0, 1.0), corner(-1.0, -1.0)]
}

fn near_plane_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let plane = arg(args, 0);
    let vectors = ["forward", "left", "up"].map(|f| doubles3(rt, rt.get_field(plane, f)));
    let points = plane_points(vectors);
    let at = match name {
        "<init>" => {
            for (i, f) in ["forward", "left", "up"].into_iter().enumerate() {
                rt.put_field(plane, f, arg(args, i + 1));
            }
            return Some(V::Void);
        }
        "getTopLeft" => points[1],
        "getTopRight" => points[2],
        "getBottomLeft" => points[3],
        "getBottomRight" => points[4],
        // `forward.add(up.scale(upScale)).subtract(left.scale(leftScale))`.
        "getPointOnPlane" => add_scaled(add_scaled(vectors[0], vectors[2], f64::from(arg(args, 2).f())), vectors[1], -f64::from(arg(args, 1).f())),
        _ => return None,
    };
    Some(vec3(rt, at))
}

/// The fluid state at `pos` and its height there (`FluidState.getHeight(level, pos)`).
fn fluid_at(rt: &mut Rt, level: V, pos: [i32; 3]) -> (V, f32) {
    let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(pos));
    let fluid = crate::fluids::of_block_state(rt, &text);
    let p = crate::api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
    let height = crate::api::call(rt, FLUID_HEIGHT, &[fluid, level, p]).f();
    (fluid, height)
}

fn fluid_in(rt: &mut Rt, fluid: V, tag: &str) -> bool {
    let key = crate::fluids::tag_key(rt, "fluid", tag);
    crate::api::call(rt, FLUID_IS, &[fluid, key]).i() != 0
}

/// `Camera.getFluidInCamera`: water when the camera is below the surface of water in its block;
/// else lava where the near plane's centre or a corner is below a lava surface, powder snow where
/// one is in it; else none.
fn fluid_in_camera(rt: &mut Rt, cam: V) -> V {
    let fog = |rt: &mut Rt, i: usize| crate::api::api_enum(rt, FOG_TYPE, FOG_TYPES[i], i as i32);
    if rt.get_field(cam, "$initialized").i() == 0 {
        return fog(rt, 3);
    }
    let level = rt.get_field(cam, "$level");
    let at = doubles3(rt, rt.get_field(cam, "$position"));
    let block = at.map(|c| c.floor() as i32);
    let (fluid, height) = fluid_at(rt, level, block);
    if fluid_in(rt, fluid, "minecraft:water") && at[1] < f64::from(block[1] as f32 + height) {
        return fog(rt, 1);
    }
    for v in plane_points(near_plane_vectors(rt, cam)) {
        let p = add_scaled(at, v, 1.0);
        let pos = p.map(|c| c.floor() as i32);
        let (fluid, height) = fluid_at(rt, level, pos);
        if fluid_in(rt, fluid, "minecraft:lava") {
            if p[1] <= f64::from(height + pos[1] as f32) {
                return fog(rt, 0);
            }
        } else if rt.host().is_some_and(|h| crate::host::split_state(&h.block_state(pos)).0 == "minecraft:powder_snow") {
            return fog(rt, 2);
        }
    }
    fog(rt, 3)
}

/// `Camera.getBlockAtCamera` (NeoForge's): the block state the camera is in; air before it is set up.
fn block_at_camera(rt: &mut Rt, cam: V) -> V {
    let at = doubles3(rt, rt.get_field(cam, "$position")).map(|c| c.floor() as i32);
    let text = match rt.get_field(cam, "$initialized").i() {
        0 => "minecraft:air".to_owned(),
        _ => rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(at)),
    };
    crate::api::state(rt, &text)
}

fn camera_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let cam = a(0);
    if name == "<init>" {
        init_camera(rt, cam);
        return Some(V::Void);
    }
    if rt.get_field(cam, "$main").i() != 0 && name != "setup" {
        follow(rt, cam);
    }
    let field = |rt: &Rt, f: &'static str| rt.get_field(cam, f);
    Some(match name {
        "getPosition" => field(rt, "$position"),
        "getBlockPosition" => field(rt, "$blockPosition"),
        "getXRot" => field(rt, "$xRot"),
        "getYRot" => field(rt, "$yRot"),
        "getRoll" => field(rt, "$roll"),
        "rotation" => field(rt, "$rotation"),
        "getLookVector" => field(rt, "$forwards"),
        "getUpVector" => field(rt, "$up"),
        "getLeftVector" => field(rt, "$left"),
        "getEntity" => field(rt, "$entity"),
        "isInitialized" => field(rt, "$initialized"),
        "isDetached" => field(rt, "$detached"),
        "getPartialTickTime" => field(rt, "$partialTickTime"),
        "getNearPlane" => near_plane(rt, cam),
        "getFluidInCamera" => fluid_in_camera(rt, cam),
        "getBlockAtCamera" => block_at_camera(rt, cam),
        "setup" => {
            setup(rt, cam, args);
            V::Void
        }
        "reset" => {
            for (f, v) in [("$level", V::NULL), ("$entity", V::NULL), ("$initialized", V::I(0))] {
                rt.put_field(cam, f, v);
            }
            V::Void
        }
        "tick" => V::Void,
        // What a mod's camera (a subclass) moves itself with.
        "setRotation" => {
            let roll = if desc == "(FFF)V" { a(3).f() } else { 0.0 };
            set_rotation(rt, cam, a(1).f(), a(2).f(), roll);
            V::Void
        }
        "setPosition" => {
            let at = if desc == "(DDD)V" { [a(1).d(), a(2).d(), a(3).d()] } else { doubles3(rt, a(1)) };
            set_position(rt, cam, at);
            V::Void
        }
        "move" => {
            move_camera(rt, cam, a(1).f(), a(2).f(), a(3).f());
            V::Void
        }
        _ => return None,
    })
}

// ---- the game renderer ----

/// A shader the game renders with, by name (`position_tex`): one object each, whose uniforms the
/// host sets itself.
fn shader(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("compat:shader/{name}"), move |rt| {
        let s = rt.new_object(SHADER);
        let name = rt.string(&n);
        rt.put_field(s, "$name", name);
        s
    })
}

/// The shader a `GameRenderer.get...Shader()` names: `getPositionTexShader` is `position_tex`.
fn shader_name(getter: &str) -> Option<String> {
    let stem = getter.strip_prefix("get")?.strip_suffix("Shader")?;
    let mut name = String::new();
    for (i, c) in stem.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            name.push('_');
        }
        name.push(c.to_ascii_lowercase());
    }
    (!name.is_empty()).then_some(name)
}

/// `GameRenderer.getProjectionMatrix(fov)`: `Matrix4f.perspective(fov, aspect, 0.05, depthFar)`,
/// unzoomed.
fn projection(rt: &mut Rt, fov_degrees: f64) -> V {
    let fov = (fov_degrees * f64::from(DEG)) as f32;
    let (near, far) = (Z_NEAR, depth_far());
    let h = f64::from(fov * 0.5).tan() as f32;
    let mut m = [0.0f32; 16];
    m[0] = 1.0 / (h * aspect(rt));
    m[5] = 1.0 / h;
    m[10] = (far + near) / (near - far);
    m[11] = -1.0;
    m[14] = (far + far) * near / (near - far);
    crate::render::new_mat4(rt, m)
}

/// `GameRenderer.getRenderDistance`: the render distance in blocks.
fn render_distance() -> f32 {
    (RENDER_DISTANCE * 16) as f32
}

/// `GameRenderer.getDepthFar`: four times the render distance.
fn depth_far() -> f32 {
    render_distance() * 4.0
}

fn game_renderer_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    if desc == "()Lnet/minecraft/client/renderer/ShaderInstance;" {
        let n = shader_name(name)?;
        return Some(shader(rt, &n));
    }
    Some(match name {
        "getMainCamera" => main(rt),
        "lightTexture" => single(rt, LIGHT_TEXTURE),
        "overlayTexture" => single(rt, OVERLAY_TEXTURE),
        "getMapRenderer" => single(rt, "net/minecraft/client/gui/MapRenderer"),
        "getMinecraft" => crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[]),
        "getRenderDistance" => V::F(render_distance()),
        "getDepthFar" => V::F(depth_far()),
        // `getFov(camera, partial, useFovSetting)`: the setting's (vanilla's default), without the
        // walk's and the water's change: the game doesn't hand its own over.
        "getFov" => V::D(FOV),
        "getProjectionMatrix" => projection(rt, arg(args, 1).d()),
        // No boss darkens the sky here.
        "getDarkenWorldAmount" => V::F(0.0),
        // The game draws its own frame, hand and outline.
        "resetProjectionMatrix" | "setRenderHand" | "setRenderBlockOutline" | "setPanoramicMode" | "resetData" | "processBlurEffect" | "renderLevel" | "tick" | "checkEntityPostEffect" | "shutdownEffect" => V::Void,
        "isPanoramicMode" => V::I(0),
        _ => return None,
    })
}

fn shader_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        // `new ShaderInstance(provider, location or name, format)`: named as NeoForge names it (a
        // vanilla shader by its path, a mod's by its whole location).
        (SHADER, "<init>") => {
            let at = rt.str_of(a(2)).unwrap_or("").to_owned();
            let named = at.strip_prefix("minecraft:").unwrap_or(&at).to_owned();
            let s = rt.string(&named);
            rt.put_field(a(0), "$name", s);
            V::Void
        }
        (SHADER, "getName") => rt.get_field(a(0), "$name"),
        (SHADER, "getUniform" | "safeGetUniform") => {
            let (s, u) = (a(0).r(), rt.str_of(a(1)).unwrap_or("").to_owned());
            rt.intern(format!("compat:uniform/{s}/{u}"), |rt| rt.new_object(UNIFORM))
        }
        (SHADER, "apply" | "clear" | "close" | "setSampler" | "setDefaultUniforms" | "markDirty" | "attachToProgram") => V::Void,
        (UNIFORM | ABSTRACT_UNIFORM, "<init>" | "upload" | "close" | "markDirty") => V::Void,
        (UNIFORM | ABSTRACT_UNIFORM, n) if n.starts_with("set") => V::Void,
        // `RegisterShadersEvent.registerShader(shader, onLoaded)`: the shader is the mod's at once.
        (REGISTER_SHADERS, "registerShader") => {
            rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(2), &[a(1)]);
            rt.report_thrown("a shader's registration");
            V::Void
        }
        (REGISTER_SHADERS, "getResourceProvider") => single(rt, "net/minecraft/server/packs/resources/ResourceManager"),
        // The shader `RenderSystem.setShader` chose, as one standing for every one.
        (RENDER_SYSTEM, "getShader") => shader(rt, "position_tex"),
        _ => return None,
    })
}

/// `LightTexture.getBrightness(dimensionType, level)`: how bright light `level` looks (a curve
/// lifted by the dimension's ambient light).
fn brightness(rt: &mut Rt, dimension: V, level: i32) -> f32 {
    let f = level as f32 / 15.0;
    let curve = f / (4.0 - 3.0 * f);
    let ambient = crate::api::call(rt, AMBIENT_LIGHT, &[dimension]).f();
    curve + ambient * (1.0 - curve)
}

// ---- events that hand the camera over ----

/// A getter or setter of NeoForge's camera events, on the field it keeps (the main camera for one
/// a mod didn't fill).
fn event_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let (ev, v) = (arg(args, 0), arg(args, 1));
    let get = |rt: &Rt, f: &'static str| rt.get_field(ev, f);
    let set = |rt: &mut Rt, f: &'static str| {
        rt.put_field(ev, f, v);
        V::Void
    };
    let viewport = VIEWPORT_EVENTS.contains(&owner);
    Some(match name {
        "getCamera" => match get(rt, "$camera") {
            c if c.r() != 0 => c,
            _ => main(rt),
        },
        "getRenderer" if viewport => single(rt, GAME_RENDERER),
        "getPartialTick" if viewport => V::D(get(rt, "$partialTick").d()),
        "getPartialTick" | "getDeltaTracker" => single(rt, "net/minecraft/client/DeltaTracker"),
        "getYaw" => get(rt, "$yaw"),
        "getPitch" => get(rt, "$pitch"),
        "getRoll" => get(rt, "$roll"),
        "setYaw" => set(rt, "$yaw"),
        "setPitch" => set(rt, "$pitch"),
        "setRoll" => set(rt, "$roll"),
        "getFOV" => get(rt, "$fov"),
        "setFOV" => set(rt, "$fov"),
        "usedConfiguredFov" => get(rt, "$usedConfiguredFov"),
        "getRed" => get(rt, "$red"),
        "getGreen" => get(rt, "$green"),
        "getBlue" => get(rt, "$blue"),
        "setRed" => set(rt, "$red"),
        "setGreen" => set(rt, "$green"),
        "setBlue" => set(rt, "$blue"),
        "getMode" => get(rt, "$mode"),
        "getType" => get(rt, "$type"),
        "getFarPlaneDistance" => get(rt, "$farPlaneDistance"),
        "getNearPlaneDistance" => get(rt, "$nearPlaneDistance"),
        "getFogShape" => get(rt, "$fogShape"),
        "setFarPlaneDistance" => set(rt, "$farPlaneDistance"),
        "setNearPlaneDistance" => set(rt, "$nearPlaneDistance"),
        "setFogShape" => set(rt, "$fogShape"),
        "scaleFarPlaneDistance" | "scaleNearPlaneDistance" => {
            let f = if name == "scaleFarPlaneDistance" { "$farPlaneDistance" } else { "$nearPlaneDistance" };
            let scaled = get(rt, f).f() * v.f();
            rt.put_field(ev, f, V::F(scaled));
            V::Void
        }
        "getStage" => get(rt, "$stage"),
        "getPoseStack" => get(rt, "$poseStack"),
        "getModelViewMatrix" => get(rt, "$modelViewMatrix"),
        "getProjectionMatrix" => get(rt, "$projectionMatrix"),
        "getRenderTick" => get(rt, "$renderTick"),
        "getFrustum" => get(rt, "$frustum"),
        "getLevelRenderer" => match get(rt, "$levelRenderer") {
            r if r.r() != 0 => r,
            _ => single(rt, "net/minecraft/client/renderer/LevelRenderer"),
        },
        "getTarget" => get(rt, "$target"),
        "getMultiBufferSource" => get(rt, "$multiBufferSource"),
        "setCanceled" => {
            rt.put_field(ev, "$canceled", V::I(v.i()));
            V::Void
        }
        "isCanceled" => V::I(get(rt, "$canceled").i()),
        _ => return None,
    })
}

// ---- the viewport events the game asks each frame ----

/// `FogRenderer.FogMode`: the sky's fog and the terrain's.
const FOG_MODE: &str = "net/minecraft/client/renderer/FogRenderer$FogMode";
const FOG_MODES: [&str; 2] = ["FOG_SKY", "FOG_TERRAIN"];
const FOG_SHAPE: &str = "com/mojang/blaze3d/shaders/FogShape";
const FOG_SHAPES: [&str; 2] = ["SPHERE", "CYLINDER"];

/// A viewport event of `class` the mods hear, made as NeoForge makes it for this frame (`partial`
/// ticks past the last), the main camera at `camera` when the game says where; None when no mod
/// listens, so a frame with no listener runs nothing.
fn viewport_event(rt: &mut Rt, class: &'static str, partial: f32, camera: Option<[f64; 3]>) -> Option<V> {
    if crate::neo::hearing(rt, class).is_empty() {
        return None;
    }
    prepare(rt, camera);
    let ev = rt.new_object(class);
    let cam = main(rt);
    rt.put_field(ev, "$camera", cam);
    rt.put_field(ev, "$partialTick", V::D(f64::from(partial)));
    Some(ev)
}

/// `ViewportEvent.ComputeFov` (`ClientHooks.getFieldOfView`, the world's): the field of view in
/// degrees as the mods listening leave it.
pub(crate) fn compute_fov(rt: &mut Rt, partial: f32, camera: [f64; 3], degrees: f32) -> f32 {
    let Some(ev) = viewport_event(rt, COMPUTE_FOV, partial, Some(camera)) else { return degrees };
    rt.put_field(ev, "$fov", V::D(f64::from(degrees)));
    rt.put_field(ev, "$usedConfiguredFov", V::I(1));
    crate::neo::post(rt, COMPUTE_FOV, ev);
    rt.report_thrown("a ComputeFov listener");
    rt.get_field(ev, "$fov").d() as f32
}

/// `ViewportEvent.ComputeFogColor` (`ClientHooks.getFogColor`): the fog's colour as the mods
/// listening leave it.
pub(crate) fn compute_fog_color(rt: &mut Rt, partial: f32, camera: [f64; 3], rgb: [f32; 3]) -> [f32; 3] {
    let Some(ev) = viewport_event(rt, FOG_COLOR, partial, Some(camera)) else { return rgb };
    for (f, c) in ["$red", "$green", "$blue"].into_iter().zip(rgb) {
        rt.put_field(ev, f, V::F(c));
    }
    crate::neo::post(rt, FOG_COLOR, ev);
    rt.report_thrown("a ComputeFogColor listener");
    ["$red", "$green", "$blue"].map(|f| rt.get_field(ev, f).f())
}

/// `ViewportEvent.RenderFog` (`ClientHooks.onFogRender`) for the fog of `mode` (0 the sky's, 1 the
/// terrain's) of type `fog_type` (`FogType`'s ordinal), from `near` to `far` blocks: the distances
/// a mod that cancels it sets, else None (vanilla's stand).
pub(crate) fn render_fog(rt: &mut Rt, partial: f32, camera: [f64; 3], mode: usize, fog_type: usize, near: f32, far: f32) -> Option<(f32, f32)> {
    let ev = viewport_event(rt, RENDER_FOG, partial, Some(camera))?;
    let mode_v = crate::api::api_enum(rt, FOG_MODE, FOG_MODES[mode.min(1)], mode.min(1) as i32);
    let type_v = crate::api::api_enum(rt, FOG_TYPE, FOG_TYPES[fog_type.min(3)], fog_type.min(3) as i32);
    // The sky's fog is a sphere, the terrain's a cylinder (`FogRenderer.setupFog`).
    let shape = crate::api::api_enum(rt, FOG_SHAPE, FOG_SHAPES[mode.min(1)], mode.min(1) as i32);
    for (f, v) in [("$mode", mode_v), ("$type", type_v), ("$fogShape", shape), ("$nearPlaneDistance", V::F(near)), ("$farPlaneDistance", V::F(far))] {
        rt.put_field(ev, f, v);
    }
    crate::neo::post(rt, RENDER_FOG, ev);
    rt.report_thrown("a RenderFog listener");
    (rt.get_field(ev, "$canceled").i() != 0).then(|| (rt.get_field(ev, "$nearPlaneDistance").f(), rt.get_field(ev, "$farPlaneDistance").f()))
}

/// `ViewportEvent.ComputeCameraAngles` (`GameRenderer.renderLevel`): the turn the mods listening
/// give the camera, as pitch, yaw and roll in degrees past the camera's own.
pub(crate) fn camera_angles(rt: &mut Rt, partial: f32) -> Option<[f32; 3]> {
    let ev = viewport_event(rt, COMPUTE_ANGLES, partial, None)?;
    let cam = main(rt);
    let (yaw, pitch) = (rt.get_field(cam, "$yRot").f(), rt.get_field(cam, "$xRot").f());
    for (f, v) in [("$yaw", yaw), ("$pitch", pitch), ("$roll", 0.0)] {
        rt.put_field(ev, f, V::F(v));
    }
    crate::neo::post(rt, COMPUTE_ANGLES, ev);
    rt.report_thrown("a ComputeCameraAngles listener");
    let [y, p, r] = ["$yaw", "$pitch", "$roll"].map(|f| rt.get_field(ev, f).f());
    Some([p - pitch, y - yaw, r])
}

// ---- the level's render stages ----

/// Stage `i` of [`STAGES`]: one object each, as NeoForge registers them.
fn stage(rt: &mut Rt, i: usize) -> V {
    rt.intern(format!("compat:stage/{}", STAGES[i]), |rt| {
        let s = rt.new_object(STAGE);
        let name = rt.string(&format!("minecraft:{}", STAGE_NAMES[i]));
        rt.put_field(s, "name", name);
        s
    })
}

/// `RenderLevelStageEvent.Stage.AFTER_PARTICLES`...
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != STAGE {
        return None;
    }
    let i = STAGES.iter().position(|s| *s == field)?;
    Some(stage(rt, i))
}

fn stage_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = arg(args, 0);
    Some(match name {
        "toString" => rt.get_field(a, "name"),
        // The stage after a block layer: none for any other type.
        "fromRenderType" => {
            let ty = rt.str_of(rt.get_field(a, "$name")).unwrap_or("").to_owned();
            match LAYER_STAGES.iter().find(|(layer, _)| *layer == ty) {
                Some(&(_, i)) => stage(rt, i),
                None => V::NULL,
            }
        }
        _ => return None,
    })
}

/// The level's render stages, as vanilla's `LevelRenderer.renderLevel` passes them each frame
/// (NeoForge's `RenderLevelStageEvent`), once the game has drawn the level: each stage in its order,
/// with a pose stack of its own at the camera (a mod moves it by minus the camera's place to draw at
/// a place in the world), the view's rotation (`getModelViewMatrix`: the camera's turned back), the
/// projection, the main camera. What a listener draws into the client's buffers (`renderBuffers()
/// .bufferSource()`) is drawn after it ([`crate::vertices`]). Nothing is made when no mod listens.
pub(crate) fn render_stages(rt: &mut Rt) {
    if crate::neo::hearing(rt, LEVEL_STAGE).is_empty() {
        return;
    }
    let cam = main(rt);
    let rotation = rt.get_field(cam, "$rotation");
    let [x, y, z, w] = ["x", "y", "z", "w"].map(|f| rt.get_field(rotation, f).f());
    let view = crate::render::new_mat4(rt, crate::render::quat_m4([-x, -y, -z, w]));
    let projection = projection(rt, FOV);
    let ticks = rt.host().map_or(0, |h| h.game_time()) as i32;
    let frustum = rt.intern("compat:frustum".into(), |rt| rt.new_object("net/minecraft/client/renderer/culling/Frustum"));
    let renderer = single(rt, "net/minecraft/client/renderer/LevelRenderer");
    for i in 0..STAGES.len() {
        let ev = rt.new_object(LEVEL_STAGE);
        let stage = stage(rt, i);
        let stack = crate::render::new_pose_stack(rt, crate::render::IDENTITY);
        let fields = [
            ("$stage", stage),
            ("$poseStack", stack),
            ("$modelViewMatrix", view),
            ("$projectionMatrix", projection),
            ("$renderTick", V::I(ticks)),
            ("$frustum", frustum),
            ("$levelRenderer", renderer),
            ("$camera", cam),
        ];
        for (f, v) in fields {
            rt.put_field(ev, f, v);
        }
        crate::neo::post(rt, LEVEL_STAGE, ev);
        crate::vertices::flush(rt, None);
    }
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i| arg(args, i);
    match owner {
        CAMERA => camera_call(rt, name, desc, args),
        STAGE => stage_call(rt, name, args),
        NEAR_PLANE => near_plane_call(rt, name, args),
        GAME_RENDERER => game_renderer_call(rt, name, desc, args),
        SHADER | UNIFORM | ABSTRACT_UNIFORM | REGISTER_SHADERS | RENDER_SYSTEM => shader_call(rt, owner, name, args),
        _ if VIEWPORT_EVENTS.contains(&owner) || HIGHLIGHT_EVENTS.contains(&owner) || owner == LEVEL_STAGE => event_call(rt, owner, name, args),
        // The entity the client views from: its player (a mod's choice of another isn't followed:
        // the game's camera stays on the player).
        MINECRAFT => match name {
            "getCameraEntity" => Some(match rt.get_field(a(0), "cameraEntity") {
                e if e.r() != 0 => e,
                _ if crate::neo::is_client(rt) && crate::world_events::client_in_world(rt) => crate::network::local_player(rt),
                _ => V::NULL,
            }),
            "setCameraEntity" => {
                rt.put_field(a(0), "cameraEntity", a(1));
                crate::glue::trace_once(rt, "camera-entity", |_| "client: a mod set the camera entity; the game's camera stays on the player".into());
                Some(V::Void)
            }
            _ => None,
        },
        ENTITY_DISPATCHER => Some(match name {
            "cameraOrientation" => match rt.get_field(a(0), "$cameraOrientation") {
                q if q.r() != 0 => q,
                _ => {
                    let cam = main(rt);
                    rt.get_field(cam, "$rotation")
                }
            },
            "overrideCameraOrientation" => {
                rt.put_field(a(0), "$cameraOrientation", a(1));
                V::Void
            }
            // `prepare(level, camera, crosshairPickEntity)`.
            "prepare" => {
                let rotation = rt.get_field(a(2), "$rotation");
                for (f, v) in [("level", a(1)), ("camera", a(2)), ("crosshairPickEntity", a(3)), ("$cameraOrientation", rotation)] {
                    rt.put_field(a(0), f, v);
                }
                V::Void
            }
            // From the camera: to a point, or to an entity's place.
            "distanceToSqr" => {
                let cam = match rt.get_field(a(0), "camera") {
                    c if c.r() != 0 => c,
                    _ => main(rt),
                };
                let from = doubles3(rt, rt.get_field(cam, "$position"));
                let to = if desc.starts_with("(DDD)") {
                    [a(1).d(), a(2).d(), a(3).d()]
                } else {
                    let p = crate::api::call(rt, "net/minecraft/world/entity/Entity.position.()Lnet/minecraft/world/phys/Vec3;", &[a(1)]);
                    doubles3(rt, p)
                };
                V::D((0..3).map(|i| (to[i] - from[i]).powi(2)).sum())
            }
            _ => return None,
        }),
        // `prepare(level, camera, cameraHitResult)`.
        BLOCK_ENTITY_DISPATCHER if name == "prepare" => {
            for (f, v) in [("level", a(1)), ("camera", a(2)), ("cameraHitResult", a(3))] {
                rt.put_field(a(0), f, v);
            }
            Some(V::Void)
        }
        // The lightmap and the overlay are the host's own.
        LIGHT_TEXTURE => match name {
            "getBrightness" => Some(V::F(brightness(rt, a(0), a(1).i()))),
            // No darkness effect dims the light here.
            "getDarknessGamma" => Some(V::F(0.0)),
            "<init>" | "turnOnLightLayer" | "turnOffLightLayer" | "updateLightTexture" | "tick" | "close" => Some(V::Void),
            _ => None,
        },
        OVERLAY_TEXTURE if matches!(name, "setupOverlayColor" | "teardownOverlayColor") => Some(V::Void),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn key(owner: &str, rest: &str) -> &'static str {
        Box::leak(format!("{owner}.{rest}").into_boxed_str())
    }

    /// A client runtime in a world, its player standing at `feet` looking `yaw`, `pitch`.
    fn client(feet: [f64; 3], yaw: f32, pitch: f32) -> (Rt, MemoryHost) {
        crate::install();
        let mut rt = Rt::new();
        rt.interned.insert("compat:client".into(), V::I(1));
        let host = MemoryHost { place: Some((feet, yaw, pitch)), gui: [320, 180], ..MemoryHost::default() };
        (rt, host)
    }

    fn main_camera(rt: &mut Rt) -> V {
        let renderer = single(rt, GAME_RENDERER);
        crate::api::call(rt, key(GAME_RENDERER, "getMainCamera.()Lnet/minecraft/client/Camera;"), &[renderer])
    }

    fn vector(rt: &mut Rt, cam: V, getter: &str) -> [f32; 3] {
        let v = crate::api::call(rt, key(CAMERA, &format!("{getter}.()Lorg/joml/Vector3f;")), &[cam]);
        floats3(rt, v)
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    /// Facing west (yaw 90), the camera is at the player's eyes (1.62 above the feet), looks along
    /// -x with south (+z) on its left; looking straight down, it looks along -y.
    #[test]
    fn the_main_camera_is_at_the_players_eyes_looking_their_way() {
        let (mut rt, mut host) = client([10.5, 64.0, -5.25], 90.0, 0.0);
        rt.with_host(&mut host, |rt| {
            let cam = main_camera(rt);
            let p = crate::api::call(rt, key(CAMERA, "getPosition.()Lnet/minecraft/world/phys/Vec3;"), &[cam]);
            let at = doubles3(rt, p);
            assert_eq!([at[0], at[2]], [10.5, -5.25]);
            assert!((at[1] - 65.62).abs() < 1e-6, "{at:?}");
            assert!(close(vector(rt, cam, "getLookVector"), [-1.0, 0.0, 0.0]));
            assert!(close(vector(rt, cam, "getUpVector"), [0.0, 1.0, 0.0]));
            assert!(close(vector(rt, cam, "getLeftVector"), [0.0, 0.0, 1.0]));
            let entity = crate::api::call(rt, key(CAMERA, "getEntity.()Lnet/minecraft/world/entity/Entity;"), &[cam]);
            assert_eq!(entity, crate::network::local_player(rt));
            assert_eq!(crate::api::call(rt, key(CAMERA, "isInitialized.()Z"), &[cam]).i(), 1);
        });
        host.place = Some(([0.0, 64.0, 0.0], 0.0, 90.0));
        rt.with_host(&mut host, |rt| {
            let cam = main_camera(rt);
            assert!(close(vector(rt, cam, "getLookVector"), [0.0, -1.0, 0.0]));
            // Looking down, the camera's top points the way the player faces (south, +z).
            assert!(close(vector(rt, cam, "getUpVector"), [0.0, 0.0, 1.0]));
        });
    }

    /// Once the game has drawn a frame, the camera is where the game drew it from, and the render
    /// dispatchers hold it; the entity dispatcher faces it until a mod overrides the orientation.
    #[test]
    fn the_frame_places_the_camera_the_dispatchers_hold() {
        let (mut rt, mut host) = client([0.0, 64.0, 0.0], 0.0, 0.0);
        rt.with_host(&mut host, |rt| {
            prepare(rt, Some([1.5, 70.25, -3.75]));
            let cam = main_camera(rt);
            let p = crate::api::call(rt, key(CAMERA, "getPosition.()Lnet/minecraft/world/phys/Vec3;"), &[cam]);
            assert_eq!(doubles3(rt, p), [1.5, 70.25, -3.75]);
            let block = crate::api::call(rt, key(CAMERA, "getBlockPosition.()Lnet/minecraft/core/BlockPos;"), &[cam]);
            let xyz = ["getX", "getY", "getZ"].map(|n| crate::api::call(rt, key("net/minecraft/core/Vec3i", &format!("{n}.()I")), &[block]).i());
            assert_eq!(xyz, [1, 70, -4]);
            let dispatcher = single(rt, ENTITY_DISPATCHER);
            assert_eq!(rt.get_field(dispatcher, "camera"), cam);
            let block_dispatcher = single(rt, BLOCK_ENTITY_DISPATCHER);
            assert_eq!(rt.get_field(block_dispatcher, "camera"), cam);
            let orientation = key(ENTITY_DISPATCHER, "cameraOrientation.()Lorg/joml/Quaternionf;");
            let rotation = crate::api::call(rt, key(CAMERA, "rotation.()Lorg/joml/Quaternionf;"), &[cam]);
            assert_eq!(crate::api::call(rt, orientation, &[dispatcher]), rotation);
            // Yaw 0 faces south: half a turn about y from north.
            let q = ["x", "y", "z", "w"].map(|f| rt.get_field(rotation, f).f());
            assert!(q[1].abs() > 0.999_999 && q[3].abs() < 1e-6, "{q:?}");
            let other = crate::render::new_quat(rt, [0.0, 0.0, 0.0, 1.0]);
            crate::api::call(rt, key(ENTITY_DISPATCHER, "overrideCameraOrientation.(Lorg/joml/Quaternionf;)V"), &[dispatcher, other]);
            assert_eq!(crate::api::call(rt, orientation, &[dispatcher]), other);
            let d = crate::api::call(rt, key(ENTITY_DISPATCHER, "distanceToSqr.(DDD)D"), &[dispatcher, V::D(4.5), V::D(70.25), V::D(0.25)]);
            assert_eq!(d.d(), 9.0 + 16.0);
        });
    }

    /// Out of a world the camera isn't set up: no entity, no fluid.
    #[test]
    fn out_of_a_world_the_camera_is_not_set_up() {
        let (mut rt, mut host) = client([0.0, 64.0, 0.0], 0.0, 0.0);
        rt.interned.remove("compat:client");
        rt.with_host(&mut host, |rt| {
            let cam = main_camera(rt);
            assert_eq!(crate::api::call(rt, key(CAMERA, "isInitialized.()Z"), &[cam]).i(), 0);
            assert_eq!(crate::api::call(rt, key(CAMERA, "getEntity.()Lnet/minecraft/world/entity/Entity;"), &[cam]), V::NULL);
            let fog = crate::api::call(rt, key(CAMERA, "getFluidInCamera.()Lnet/minecraft/world/level/material/FogType;"), &[cam]);
            assert_eq!(rt.str_of(rt.get_field(fog, "$name")), Some("NONE"));
        });
    }

    /// The fog the camera's place calls for: water below the water's surface (a source's is 8/9 up
    /// its block), lava where the near plane dips into lava, powder snow in powder snow.
    #[test]
    fn the_camera_sees_the_fluid_its_eyes_are_in() {
        let fog_at = |feet: [f64; 3], blocks: &[([i32; 3], &str)]| {
            let (mut rt, mut host) = client(feet, 0.0, 0.0);
            for (pos, state) in blocks {
                host.blocks.insert(*pos, (*state).to_owned());
            }
            rt.with_host(&mut host, |rt| {
                let cam = main_camera(rt);
                let fog = crate::api::call(rt, key(CAMERA, "getFluidInCamera.()Lnet/minecraft/world/level/material/FogType;"), &[cam]);
                rt.str_of(rt.get_field(fog, "$name")).unwrap_or("").to_owned()
            })
        };
        // Eyes at 64.82: under a source's surface (64.888...), above it at 64.92.
        assert_eq!(fog_at([0.5, 63.2, 0.5], &[([0, 64, 0], "minecraft:water[level=0]")]), "WATER");
        assert_eq!(fog_at([0.5, 63.3, 0.5], &[([0, 64, 0], "minecraft:water[level=0]")]), "NONE");
        assert_eq!(fog_at([0.5, 63.2, 0.5], &[([0, 64, 0], "minecraft:lava[level=0]")]), "LAVA");
        assert_eq!(fog_at([0.5, 63.2, 0.5], &[([0, 64, 0], "minecraft:powder_snow")]), "POWDER_SNOW");
        assert_eq!(fog_at([0.5, 63.2, 0.5], &[]), "NONE");
    }

    /// The projection a mod draws with: OpenGL's perspective of the field of view (70 degrees) on
    /// the window's shape (16:9 here), from 0.05 to four times the render distance.
    #[test]
    fn the_projection_is_the_perspective_of_the_view() {
        let (mut rt, mut host) = client([0.0, 64.0, 0.0], 0.0, 0.0);
        rt.with_host(&mut host, |rt| {
            let renderer = single(rt, GAME_RENDERER);
            let m = crate::api::call(rt, key(GAME_RENDERER, "getProjectionMatrix.(D)Lorg/joml/Matrix4f;"), &[renderer, V::D(70.0)]);
            let m = crate::render::mat4(rt, m);
            let t = (35.0f64).to_radians().tan();
            let (near, far) = (0.05, 768.0);
            assert!((f64::from(m[5]) - 1.0 / t).abs() < 1e-4, "{m:?}");
            assert!((f64::from(m[0]) - 1.0 / t / (16.0 / 9.0)).abs() < 1e-4);
            assert!((f64::from(m[10]) - (far + near) / (near - far)).abs() < 1e-5);
            assert!((f64::from(m[14]) - 2.0 * far * near / (near - far)).abs() < 1e-5);
            assert_eq!((m[11], m[15]), (-1.0, 0.0));
        });
    }

    /// A mod's shader registration hands the shader to its callback at once; the game renderer's
    /// shaders are one object each, by name.
    #[test]
    fn shaders_are_one_object_a_name() {
        assert_eq!(shader_name("getPositionTexColorShader").as_deref(), Some("position_tex_color"));
        assert_eq!(shader_name("getRendertypeEntityCutoutShader").as_deref(), Some("rendertype_entity_cutout"));
        let (mut rt, _) = client([0.0; 3], 0.0, 0.0);
        let get = key(GAME_RENDERER, "getPositionTexShader.()Lnet/minecraft/client/renderer/ShaderInstance;");
        let a = crate::api::call(&mut rt, get, &[]);
        assert_eq!(crate::api::call(&mut rt, get, &[]), a);
        let name = crate::api::call(&mut rt, key(SHADER, "getName.()Ljava/lang/String;"), &[a]);
        assert_eq!(rt.str_of(name), Some("position_tex"));
    }

    /// Mods listening to the viewport events change what the game draws with. Tested: a narrower
    /// field of view, a redder fog, the terrain's fog farther (NeoForge applies a `RenderFog` only when
    /// the listener cancels it, and leaves the sky's, set but not cancelled, as it was), and a camera
    /// turned and rolled past its own. With no listener, the game's own stand, and no event made.
    #[test]
    fn the_viewport_events_change_the_frame_as_their_listeners_say() {
        fn listener(rt: &mut Rt, _: u32, args: &[V]) -> Option<V> {
            let ev = args[1];
            match rt.class_of(ev).to_owned().as_str() {
                COMPUTE_FOV => {
                    let fov = crate::api::call(rt, key(COMPUTE_FOV, "getFOV.()D"), &[ev]).d();
                    crate::api::call(rt, key(COMPUTE_FOV, "setFOV.(D)V"), &[ev, V::D(fov * 0.5)]);
                }
                FOG_COLOR => {
                    crate::api::call(rt, key(FOG_COLOR, "setRed.(F)V"), &[ev, V::F(1.0)]);
                }
                RENDER_FOG => {
                    let mode = crate::api::call(rt, key(RENDER_FOG, "getMode.()Lnet/minecraft/client/renderer/FogRenderer$FogMode;"), &[ev]);
                    let terrain = crate::api::get_static(rt, "net/minecraft/client/renderer/FogRenderer$FogMode.FOG_TERRAIN");
                    if mode == terrain {
                        crate::api::call(rt, key(RENDER_FOG, "setFarPlaneDistance.(F)V"), &[ev, V::F(500.0)]);
                        crate::api::call(rt, key(RENDER_FOG, "setCanceled.(Z)V"), &[ev, V::I(1)]);
                    } else {
                        crate::api::call(rt, key(RENDER_FOG, "setFarPlaneDistance.(F)V"), &[ev, V::F(1.0)]);
                    }
                }
                COMPUTE_ANGLES => {
                    let yaw = crate::api::call(rt, key(COMPUTE_ANGLES, "getYaw.()F"), &[ev]).f();
                    crate::api::call(rt, key(COMPUTE_ANGLES, "setYaw.(F)V"), &[ev, V::F(yaw + 2.0)]);
                    crate::api::call(rt, key(COMPUTE_ANGLES, "setRoll.(F)V"), &[ev, V::F(5.0)]);
                }
                _ => {}
            }
            Some(V::Void)
        }
        let (mut rt, mut host) = client([0.0, 64.0, 0.0], 90.0, 0.0);
        let at = [0.5, 65.6, 0.5];
        rt.with_host(&mut host, |rt| {
            assert_eq!(compute_fov(rt, 0.5, at, 70.0), 70.0);
            assert_eq!(compute_fog_color(rt, 0.5, at, [0.5, 0.6, 0.7]), [0.5, 0.6, 0.7]);
            assert_eq!(render_fog(rt, 0.5, at, 1, 3, 10.0, 192.0), None);
            assert_eq!(camera_angles(rt, 0.5), None);
        });
        rt.lambda_call = Some(listener);
        for class in [COMPUTE_FOV, FOG_COLOR, RENDER_FOG, COMPUTE_ANGLES] {
            let l = rt.lambda_id("java/util/function/Consumer", 1, Vec::new());
            rt.listeners.push((class.to_owned(), l));
        }
        rt.with_host(&mut host, |rt| {
            assert_eq!(compute_fov(rt, 0.5, at, 70.0), 35.0);
            assert_eq!(compute_fog_color(rt, 0.5, at, [0.5, 0.6, 0.7]), [1.0, 0.6, 0.7]);
            assert_eq!(render_fog(rt, 0.5, at, 1, 3, 10.0, 192.0), Some((10.0, 500.0)));
            assert_eq!(render_fog(rt, 0.5, at, 0, 3, 0.0, 192.0), None);
            assert_eq!(camera_angles(rt, 0.5), Some([0.0, 2.0, 5.0]));
        });
    }

    /// A mod drawing in the level's stages (an outline's renderer, a ghost block's): it hears every
    /// stage in a frame's order, each one it can compare (`fromRenderType(translucent())` is
    /// `AFTER_TRANSLUCENT_BLOCKS`). What it draws into the client's buffers at a place in the world,
    /// its pose stack moved by minus the camera's place, is drawn there relative to the camera; the
    /// view matrix turns the camera's forward (west) to -z.
    #[test]
    fn the_level_stages_pass_in_order_and_what_they_draw_is_drawn() {
        const GET_STAGE: &str = "getStage.()Lnet/neoforged/neoforge/client/event/RenderLevelStageEvent$Stage;";
        const FROM_TYPE: &str = "fromRenderType.(Lnet/minecraft/client/renderer/RenderType;)Lnet/neoforged/neoforge/client/event/RenderLevelStageEvent$Stage;";
        fn listener(rt: &mut Rt, _: u32, args: &[V]) -> Option<V> {
            let ev = args[1];
            let stage = crate::api::call(rt, key(LEVEL_STAGE, GET_STAGE), &[ev]);
            let name = crate::api::call(rt, key(STAGE, "toString.()Ljava/lang/String;"), &[stage]);
            let heard = rt.intern("test:stages".into(), |rt| rt.alloc("java/util/ArrayList", Data::List(Vec::new())));
            if let Some(Data::List(l)) = rt.heap.get_mut(heard.r() as usize).map(|o| &mut o.data) {
                l.push(name);
            }
            let translucent = crate::api::call(rt, "net/minecraft/client/renderer/RenderType.translucent.()Lnet/minecraft/client/renderer/RenderType;", &[]);
            if stage != crate::api::call(rt, key(STAGE, FROM_TYPE), &[translucent]) {
                return Some(V::Void);
            }
            let view = crate::api::call(rt, key(LEVEL_STAGE, "getModelViewMatrix.()Lorg/joml/Matrix4f;"), &[ev]);
            rt.interned.insert("test:view".into(), view);
            // A white square on top of the block at (2, 64, 3).
            let camera = crate::api::call(rt, key(LEVEL_STAGE, "getCamera.()Lnet/minecraft/client/Camera;"), &[ev]);
            let at = crate::api::call(rt, key(CAMERA, "getPosition.()Lnet/minecraft/world/phys/Vec3;"), &[camera]);
            let [x, y, z] = doubles3(rt, at);
            let stack = crate::api::call(rt, key(LEVEL_STAGE, "getPoseStack.()Lcom/mojang/blaze3d/vertex/PoseStack;"), &[ev]);
            crate::api::call(rt, "com/mojang/blaze3d/vertex/PoseStack.translate.(DDD)V", &[stack, V::D(-x), V::D(-y), V::D(-z)]);
            let pose = crate::api::call(rt, "com/mojang/blaze3d/vertex/PoseStack.last.()Lcom/mojang/blaze3d/vertex/PoseStack$Pose;", &[stack]);
            let mc = crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[]);
            let buffers = crate::api::call(rt, "net/minecraft/client/Minecraft.renderBuffers.()Lnet/minecraft/client/renderer/RenderBuffers;", &[mc]);
            let source = crate::api::call(rt, "net/minecraft/client/renderer/RenderBuffers.bufferSource.()Lnet/minecraft/client/renderer/MultiBufferSource$BufferSource;", &[buffers]);
            let lightning = crate::api::call(rt, "net/minecraft/client/renderer/RenderType.lightning.()Lnet/minecraft/client/renderer/RenderType;", &[]);
            let get_buffer = "net/minecraft/client/renderer/MultiBufferSource$BufferSource.getBuffer.(Lnet/minecraft/client/renderer/RenderType;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
            let c = crate::api::call(rt, get_buffer, &[source, lightning]);
            for [px, pz] in [[2.0, 3.0], [2.0, 4.0], [3.0, 4.0], [3.0, 3.0]] {
                let add = "com/mojang/blaze3d/vertex/VertexConsumer.addVertex.(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
                let v = crate::api::call(rt, add, &[c, pose, V::F(px), V::F(65.0), V::F(pz)]);
                crate::api::call(rt, "com/mojang/blaze3d/vertex/VertexConsumer.setColor.(I)Lcom/mojang/blaze3d/vertex/VertexConsumer;", &[v, V::I(-1)]);
            }
            crate::api::call(rt, "net/minecraft/client/renderer/MultiBufferSource$BufferSource.endBatch.(Lnet/minecraft/client/renderer/RenderType;)V", &[source, lightning]);
            Some(V::Void)
        }
        let (mut rt, mut host) = client([0.0, 64.0, 0.0], 90.0, 0.0);
        rt.lambda_call = Some(listener);
        let l = rt.lambda_id("java/util/function/Consumer", 1, Vec::new());
        rt.listeners.push((LEVEL_STAGE.to_owned(), l));
        rt.with_host(&mut host, |rt| {
            prepare(rt, Some([0.5, 65.5, 0.5]));
            render_stages(rt);
        });
        let heard = rt.interned["test:stages"];
        let names: Vec<String> = match &rt.heap[heard.r() as usize].data {
            Data::List(l) => l.iter().map(|n| rt.str_of(*n).unwrap_or("").to_owned()).collect(),
            _ => Vec::new(),
        };
        assert_eq!(names, STAGE_NAMES.map(|n| format!("minecraft:{n}")));
        let after = crate::api::get_static(&mut rt, "net/neoforged/neoforge/client/event/RenderLevelStageEvent$Stage.AFTER_TRANSLUCENT_BLOCKS");
        assert_eq!(after, stage(&mut rt, 6));
        let quads: Vec<_> = host
            .drawn
            .iter()
            .flat_map(|(m, _, _, _)| match m {
                crate::host::Model::Quads(q) => q.to_vec(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(quads.len(), 1, "{quads:?}");
        assert_eq!(quads[0].positions, [[1.5, -0.5, 2.5], [1.5, -0.5, 3.5], [2.5, -0.5, 3.5], [2.5, -0.5, 2.5]]);
        assert_eq!(quads[0].sprite, "horizon/white");
        assert_eq!(quads[0].facing, 1);
        let m = crate::render::mat4(&rt, rt.interned["test:view"]);
        assert!(close([-m[0], -m[1], -m[2]], [0.0, 0.0, -1.0]), "{m:?}");
    }
}
