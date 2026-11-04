//! Drawing on the client, natively. Minecraft's rendering API as translated code meets it:
//! `PoseStack`, JOML's matrices, vectors and quaternions, `Axis`, `RenderType`,
//! `MultiBufferSource`, the client's `Minecraft` and its block renderer, light and overlay
//! packing, and block entity renderers' registration (`BlockEntityRenderers.register`).
//!
//! Libraries mods draw with get native halves where the translated code would run once per
//! vertex (the translator's intrinsics, [`INTRINSICS`]): catnip's `SuperByteBuffer` keeps the model
//! it was made from, its transforms run as catnip wrote them, and `renderInto` is one
//! [`Host::draw_model`](crate::host::Host::draw_model).
use crate::host::Model;
use crate::{Data, Rt, V};

/// A 4x4 matrix, column-major as JOML stores it (`m00 m01 m02 m03` is the first column).
pub type M4 = [f32; 16];
type M3 = [f32; 9];

pub const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const IDENTITY3: M3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// `OverlayTexture.NO_OVERLAY`: `pack(0, 10)`.
pub const NO_OVERLAY: i32 = 10 << 16;
/// `LightTexture.FULL_BRIGHT`: block and sky light 15.
pub const FULL_BRIGHT: i32 = 15 << 4 | 15 << 20;

/// Methods the translator replaces with this module's native ones (their Java bodies would run
/// per vertex, or reach a GPU backend Horizon draws without).
pub const INTRINSICS: &[&str] = &[
    CREATE_FOR_BLOCK,
    RENDER_INTO,
    SBB_IS_EMPTY,
    DEFAULT_RENDER_INTO,
    DEFAULT_IS_EMPTY,
    SBB_BUILDER_BEGIN,
    SBB_BUILDER_END,
    PARTIAL_GET,
    SUPPORTS_VISUALIZATION,
    SHIFTS[0],
    SHIFTS[1],
    SHIFTS[2],
    SHIFTS[3],
    SHIFTS[4],
    SHIFTS[5],
    SHIFTS[6],
    SHIFTS[7],
    SHIFTS[8],
    SHIFTS[9],
];

/// catnip's texture moves (`shiftUV`, `shiftUVScrolling`, `shiftUVtoSheet`), which set a function
/// run on each vertex: kept on the buffer as what they ask instead, for `renderInto`'s one draw.
const SHIFTS: [&str; 10] = [
    "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.shiftUV.(Lnet/createmod/catnip/render/SpriteShiftEntry;)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.shiftUVScrolling.(Lnet/createmod/catnip/render/SpriteShiftEntry;F)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.shiftUVScrolling.(Lnet/createmod/catnip/render/SpriteShiftEntry;FF)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.shiftUVtoSheet.(Lnet/createmod/catnip/render/SpriteShiftEntry;FFI)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/DefaultSuperByteBuffer.shiftUV.(Lnet/createmod/catnip/render/SpriteShiftEntry;)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/DefaultSuperByteBuffer.shiftUV.(Lnet/createmod/catnip/render/SpriteShiftEntry;)Lnet/createmod/catnip/render/DefaultSuperByteBuffer;",
    "net/createmod/catnip/render/DefaultSuperByteBuffer.shiftUVScrolling.(Lnet/createmod/catnip/render/SpriteShiftEntry;FF)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/DefaultSuperByteBuffer.shiftUVScrolling.(Lnet/createmod/catnip/render/SpriteShiftEntry;FF)Lnet/createmod/catnip/render/DefaultSuperByteBuffer;",
    "net/createmod/catnip/render/DefaultSuperByteBuffer.shiftUVtoSheet.(Lnet/createmod/catnip/render/SpriteShiftEntry;FFI)Lnet/createmod/catnip/render/SuperByteBuffer;",
    "net/createmod/catnip/render/DefaultSuperByteBuffer.shiftUVtoSheet.(Lnet/createmod/catnip/render/SpriteShiftEntry;FFI)Lnet/createmod/catnip/render/DefaultSuperByteBuffer;",
];
const ORIGINAL_LOCATION: &str = "net/createmod/catnip/render/SpriteShiftEntry.getOriginalResourceLocation.()Lnet/minecraft/resources/ResourceLocation;";
const TARGET_LOCATION: &str = "net/createmod/catnip/render/SpriteShiftEntry.getTargetResourceLocation.()Lnet/minecraft/resources/ResourceLocation;";

/// The texture move a buffer was asked for since it last drew.
fn shift_of(rt: &Rt, sbb: V) -> Option<crate::host::UvShift> {
    use crate::host::UvShiftKind;
    let f = |k| rt.get_field(sbb, k).f();
    let kind = match rt.get_field(sbb, "$shiftKind").i() {
        1 => UvShiftKind::Retarget,
        2 => UvShiftKind::Scroll { u: f("$shiftU"), v: f("$shiftV") },
        3 => UvShiftKind::Sheet { u: f("$shiftU"), v: f("$shiftV"), size: f("$shiftSize") },
        _ => return None,
    };
    let from = rt.str_of(rt.get_field(sbb, "$shiftFrom"))?.to_owned();
    let to = rt.str_of(rt.get_field(sbb, "$shiftTo"))?.to_owned();
    Some(crate::host::UvShift { from, to, kind })
}
const CREATE_FOR_BLOCK: &str = "net/createmod/catnip/render/SuperBufferFactory.createForBlock.(Lnet/minecraft/client/resources/model/BakedModel;Lnet/minecraft/world/level/block/state/BlockState;Lcom/mojang/blaze3d/vertex/PoseStack;)Lnet/createmod/catnip/render/SuperByteBuffer;";
/// catnip's buffer of a baked model (what `createForBlock` makes).
const RENDER_INTO: &str = "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.renderInto.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;)V";
const SBB_IS_EMPTY: &str = "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.isEmpty.()Z";
/// catnip's older buffer of mesh data.
const DEFAULT_RENDER_INTO: &str = "net/createmod/catnip/render/DefaultSuperByteBuffer.renderInto.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;)V";
const DEFAULT_IS_EMPTY: &str = "net/createmod/catnip/render/DefaultSuperByteBuffer.isEmpty.()Z";
/// catnip's buffer of the blocks a renderer tesselates into it (Create's contraptions): it keeps
/// each block's model and pose (`$parts`) instead of their vertices.
const SBB_BUILDER_BEGIN: &str = "net/createmod/catnip/render/ShadedBlockSbbBuilder.begin.()V";
const SBB_BUILDER_END: &str = "net/createmod/catnip/render/ShadedBlockSbbBuilder.end.()Lnet/createmod/catnip/render/SuperByteBuffer;";
const PARTIAL_GET: &str = "dev/engine_room/flywheel/lib/model/baked/PartialModel.get.()Lnet/minecraft/client/resources/model/BakedModel;";
/// Flywheel's instancing needs its GPU backend: off, so mods draw with their block entity renderers.
const SUPPORTS_VISUALIZATION: &str = "dev/engine_room/flywheel/api/visualization/VisualizationManager.supportsVisualization.(Lnet/minecraft/world/level/LevelAccessor;)Z";
const SBB: &str = "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer";
const SBB_RESET: &str = "reset.()Lnet/createmod/catnip/render/SuperByteBuffer;";
const SBB_RESET_KEY: &str = "net/createmod/catnip/render/ShadeSeparatingSuperByteBuffer.reset.()Lnet/createmod/catnip/render/SuperByteBuffer;";
const DEFAULT_RESET: &str = "reset.()Lnet/createmod/catnip/render/DefaultSuperByteBuffer;";
const DEFAULT_RESET_KEY: &str = "net/createmod/catnip/render/DefaultSuperByteBuffer.reset.()Lnet/createmod/catnip/render/DefaultSuperByteBuffer;";

// ---- matrix arithmetic ----

/// `a * b`.
pub fn mul4(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[c * 4 + k]).sum();
        }
    }
    r
}

fn mul3(a: &M3, b: &M3) -> M3 {
    let mut r = [0.0; 9];
    for c in 0..3 {
        for row in 0..3 {
            r[c * 3 + row] = (0..3).map(|k| a[k * 3 + row] * b[c * 3 + k]).sum();
        }
    }
    r
}

fn translation(x: f32, y: f32, z: f32) -> M4 {
    let mut m = IDENTITY;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

fn scaling(x: f32, y: f32, z: f32) -> M4 {
    let mut m = IDENTITY;
    m[0] = x;
    m[5] = y;
    m[10] = z;
    m
}

/// A unit quaternion's rotation as a 3x3 matrix (JOML's `Matrix3f.rotation(Quaternionfc)`).
fn quat_m3([x, y, z, w]: [f32; 4]) -> M3 {
    let (w2, x2, y2, z2) = (w * w, x * x, y * y, z * z);
    let (dzw, dxy, dxz, dyw, dyz, dxw) = (2.0 * z * w, 2.0 * x * y, 2.0 * x * z, 2.0 * y * w, 2.0 * y * z, 2.0 * x * w);
    [w2 + x2 - z2 - y2, dxy + dzw, dxz - dyw, dxy - dzw, y2 - z2 + w2 - x2, dyz + dxw, dyw + dxz, dyz - dxw, z2 - y2 - x2 + w2]
}

fn m3_to_m4(m: &M3) -> M4 {
    [m[0], m[1], m[2], 0.0, m[3], m[4], m[5], 0.0, m[6], m[7], m[8], 0.0, 0.0, 0.0, 0.0, 1.0]
}

fn m4_to_m3(m: &M4) -> M3 {
    [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]]
}

fn quat_m4(q: [f32; 4]) -> M4 {
    m3_to_m4(&quat_m3(q))
}

/// `a * b` (JOML's `a.mul(b)`): `b`'s rotation first.
fn quat_mul([ax, ay, az, aw]: [f32; 4], [bx, by, bz, bw]: [f32; 4]) -> [f32; 4] {
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The rotation by `angle` radians about `(x, y, z)` (`rotationAxis`).
fn quat_axis(angle: f32, x: f32, y: f32, z: f32) -> [f32; 4] {
    let (s, c) = (angle * 0.5).sin_cos();
    let len = (x * x + y * y + z * z).sqrt();
    if len == 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [x / len * s, y / len * s, z / len * s, c]
}

fn quat_x(angle: f32) -> [f32; 4] {
    quat_axis(angle, 1.0, 0.0, 0.0)
}
fn quat_y(angle: f32) -> [f32; 4] {
    quat_axis(angle, 0.0, 1.0, 0.0)
}
fn quat_z(angle: f32) -> [f32; 4] {
    quat_axis(angle, 0.0, 0.0, 1.0)
}

/// `v` turned by unit quaternion `q`.
fn quat_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    transform3(&quat_m3(q), v)
}

fn transform3(m: &M3, [x, y, z]: [f32; 3]) -> [f32; 3] {
    [m[0] * x + m[3] * y + m[6] * z, m[1] * x + m[4] * y + m[7] * z, m[2] * x + m[5] * y + m[8] * z]
}

fn transform4(m: &M4, [x, y, z, w]: [f32; 4]) -> [f32; 4] {
    [
        m[0] * x + m[4] * y + m[8] * z + m[12] * w,
        m[1] * x + m[5] * y + m[9] * z + m[13] * w,
        m[2] * x + m[6] * y + m[10] * z + m[14] * w,
        m[3] * x + m[7] * y + m[11] * z + m[15] * w,
    ]
}

/// The general inverse (a singular matrix gives the identity).
fn invert4(m: &M4) -> M4 {
    let mut inv = [0.0f32; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det == 0.0 {
        return IDENTITY;
    }
    inv.map(|v| v / det)
}

fn invert3(m: &M3) -> M3 {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let det = a * (e * i - f * h) - d * (b * i - c * h) + g * (b * f - c * e);
    if det == 0.0 {
        return IDENTITY3;
    }
    let r = [e * i - f * h, c * h - b * i, b * f - c * e, f * g - d * i, a * i - c * g, c * d - a * f, d * h - e * g, b * g - a * h, a * e - b * d];
    r.map(|v| v / det)
}

fn transpose3(m: &M3) -> M3 {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}

/// The normal matrix of `m` (its upper 3x3's inverse transpose, `Matrix4f.normal`).
fn normal_of(m: &M4) -> M3 {
    transpose3(&invert3(&m4_to_m3(m)))
}

// ---- JOML objects: matrices as float arrays, vectors and quaternions with their public fields ----

fn floats<const N: usize>(rt: &Rt, v: V, default: [f32; N]) -> [f32; N] {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Array(a)) if a.len() == N => std::array::from_fn(|i| a[i].f()),
        _ => default,
    }
}

fn set_floats<const N: usize>(rt: &mut Rt, v: V, m: [f32; N]) {
    if let Some(o) = rt.heap.get_mut(v.r() as usize) {
        match &mut o.data {
            Data::Array(a) if a.len() == N => {
                for (slot, f) in a.iter_mut().zip(m) {
                    *slot = V::F(f);
                }
            }
            data => *data = Data::Array(m.iter().map(|f| V::F(*f)).collect()),
        }
    }
}

pub fn mat4(rt: &Rt, v: V) -> M4 {
    floats(rt, v, IDENTITY)
}
fn set_mat4(rt: &mut Rt, v: V, m: M4) {
    set_floats(rt, v, m);
}
pub fn new_mat4(rt: &mut Rt, m: M4) -> V {
    rt.alloc("org/joml/Matrix4f", Data::Array(m.iter().map(|f| V::F(*f)).collect()))
}
fn mat3(rt: &Rt, v: V) -> M3 {
    floats(rt, v, IDENTITY3)
}
fn set_mat3(rt: &mut Rt, v: V, m: M3) {
    set_floats(rt, v, m);
}
fn new_mat3(rt: &mut Rt, m: M3) -> V {
    rt.alloc("org/joml/Matrix3f", Data::Array(m.iter().map(|f| V::F(*f)).collect()))
}

fn quat(rt: &Rt, v: V) -> [f32; 4] {
    [rt.get_field(v, "x").f(), rt.get_field(v, "y").f(), rt.get_field(v, "z").f(), rt.get_field(v, "w").f()]
}
fn set_quat(rt: &mut Rt, v: V, [x, y, z, w]: [f32; 4]) {
    rt.put_field(v, "x", V::F(x));
    rt.put_field(v, "y", V::F(y));
    rt.put_field(v, "z", V::F(z));
    rt.put_field(v, "w", V::F(w));
}
pub fn new_quat(rt: &mut Rt, q: [f32; 4]) -> V {
    let o = rt.new_object("org/joml/Quaternionf");
    set_quat(rt, o, q);
    o
}

fn vec3(rt: &Rt, v: V) -> [f32; 3] {
    [rt.get_field(v, "x").f(), rt.get_field(v, "y").f(), rt.get_field(v, "z").f()]
}
fn set_vec3(rt: &mut Rt, v: V, [x, y, z]: [f32; 3]) {
    rt.put_field(v, "x", V::F(x));
    rt.put_field(v, "y", V::F(y));
    rt.put_field(v, "z", V::F(z));
}
fn vec4(rt: &Rt, v: V) -> [f32; 4] {
    [rt.get_field(v, "x").f(), rt.get_field(v, "y").f(), rt.get_field(v, "z").f(), rt.get_field(v, "w").f()]
}
fn set_vec4(rt: &mut Rt, v: V, [x, y, z, w]: [f32; 4]) {
    rt.put_field(v, "x", V::F(x));
    rt.put_field(v, "y", V::F(y));
    rt.put_field(v, "z", V::F(z));
    rt.put_field(v, "w", V::F(w));
}

/// The client's calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    match owner {
        "org/joml/Matrix4f" | "org/joml/Matrix4fc" => matrix4(rt, name, desc, args),
        "org/joml/Matrix3f" | "org/joml/Matrix3fc" => matrix3(rt, name, desc, args),
        "org/joml/Quaternionf" | "org/joml/Quaternionfc" => quaternion(rt, name, desc, args),
        "org/joml/Vector3f" | "org/joml/Vector3fc" => vector3(rt, name, desc, args),
        "org/joml/Vector4f" | "org/joml/Vector4fc" => vector4(rt, name, desc, args),
        "com/mojang/blaze3d/vertex/PoseStack" => pose_stack(rt, name, desc, args),
        "com/mojang/blaze3d/vertex/PoseStack$Pose" => pose(rt, name, desc, args),
        "com/mojang/math/Axis" => axis(rt, name, args),
        _ if INTRINSICS.contains(&key) => intrinsic(rt, key, args),
        _ => client(rt, owner, name, desc, args),
    }
}

/// Static fields this module owns.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    Some(match (owner, field) {
        ("com/mojang/math/Axis", f) => {
            let i = ["XN", "XP", "YN", "YP", "ZN", "ZP"].iter().position(|n| *n == f)? as i32;
            rt.intern(format!("axis/{i}"), |rt| {
                let o = rt.new_object("com/mojang/math/Axis");
                rt.put_field(o, "$axis", V::I(i));
                o
            })
        }
        // The block atlas (a ResourceLocation, which the runtime keeps as its string).
        ("net/minecraft/world/inventory/InventoryMenu", "BLOCK_ATLAS") | ("net/minecraft/client/renderer/texture/TextureAtlas", "LOCATION_BLOCKS") => {
            rt.string("minecraft:textures/atlas/blocks.png")
        }
        ("net/minecraft/world/item/ItemDisplayContext", f) => {
            let i = DISPLAY_CONTEXTS.iter().position(|n| *n == f)?;
            crate::api::api_enum(rt, "net/minecraft/world/item/ItemDisplayContext", DISPLAY_CONTEXTS[i], i as i32)
        }
        ("net/minecraft/client/renderer/texture/OverlayTexture", "NO_OVERLAY") => V::I(NO_OVERLAY),
        ("net/minecraft/client/renderer/texture/OverlayTexture", "NO_WHITE_U") => V::I(0),
        ("net/minecraft/client/renderer/texture/OverlayTexture", "RED_OVERLAY_V") => V::I(3),
        ("net/minecraft/client/renderer/texture/OverlayTexture", "WHITE_OVERLAY_V") => V::I(10),
        ("net/minecraft/client/renderer/LightTexture", "FULL_BRIGHT") => V::I(FULL_BRIGHT),
        ("net/minecraft/client/renderer/LightTexture", "FULL_SKY") => V::I(15 << 20),
        ("net/minecraft/client/renderer/LightTexture", "FULL_BLOCK") => V::I(15 << 4),
        ("net/neoforged/neoforge/client/model/data/ModelData", "EMPTY") => {
            rt.intern("compat:model-data".into(), |rt| rt.new_object("net/neoforged/neoforge/client/model/data/ModelData"))
        }
        _ => return None,
    })
}

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

const BAKED_MODEL_WRAPPER: &str = "net/neoforged/neoforge/client/model/BakedModelWrapper";
const MODEL_DATA: &str = "net/neoforged/neoforge/client/model/data/ModelData";
const GET_QUADS_DESC: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;Lnet/neoforged/neoforge/client/model/data/ModelData;Lnet/minecraft/client/renderer/RenderType;)Ljava/util/List;";
/// NeoForge's `getQuads` (with model data and render type), the one chunk meshing asks.
pub const GET_QUADS: &str = "getQuads.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;Lnet/neoforged/neoforge/client/model/data/ModelData;Lnet/minecraft/client/renderer/RenderType;)Ljava/util/List;";
pub const GET_QUADS_KEY: &str = "net/minecraft/client/resources/model/BakedModel.getQuads.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;Lnet/neoforged/neoforge/client/model/data/ModelData;Lnet/minecraft/client/renderer/RenderType;)Ljava/util/List;";
const GET_QUADS_VANILLA: &str = "getQuads.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;)Ljava/util/List;";
const GET_QUADS_VANILLA_KEY: &str = "net/minecraft/client/resources/model/BakedModel.getQuads.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;)Ljava/util/List;";

/// A block state's model location, as `BlockModelShaper.stateToModelLocation` names it:
/// `create:shaft#axis=x` (`name#` for a state without properties).
pub fn model_location(name: &str, state: &str) -> String {
    let props = state.split_once('[').map_or("", |(_, p)| p.trim_end_matches(']'));
    format!("{name}#{props}")
}

/// A `ModelData.Builder`, starting from `from`'s values (null: none).
fn model_data_builder(rt: &mut Rt, from: V) -> V {
    let b = rt.new_object("net/neoforged/neoforge/client/model/data/ModelData$Builder");
    let props = rt.alloc("java/util/HashMap", crate::Data::Table(crate::Table::default()));
    if from.r() != 0 {
        let old = rt.get_field(from, "$props");
        for (k, v) in crate::jutil::table_pairs(rt, old) {
            crate::jutil::table_put(rt, props, k, v);
        }
    }
    rt.put_field(b, "$props", props);
    b
}

/// The value `data` holds for `prop`, if it holds one (`ModelData.EMPTY` holds none).
fn model_data_get(rt: &mut Rt, data: V, prop: V) -> Option<V> {
    let props = rt.get_field(data, "$props");
    if props.r() == 0 {
        return None;
    }
    crate::jutil::table_get(rt, props, prop)
}

fn matrix4(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let m = mat4(rt, this);
    let set = |rt: &mut Rt, n: M4| {
        set_mat4(rt, this, n);
        this
    };
    Some(match (name, desc) {
        ("<init>", "()V") => {
            set(rt, IDENTITY);
            V::Void
        }
        ("<init>", "(Lorg/joml/Matrix4fc;)V") => {
            let src = mat4(rt, a(1));
            set(rt, src);
            V::Void
        }
        ("<init>", "(Lorg/joml/Matrix3fc;)V") => {
            let src = m3_to_m4(&mat3(rt, a(1)));
            set(rt, src);
            V::Void
        }
        ("identity", _) => set(rt, IDENTITY),
        ("set", "(Lorg/joml/Matrix4fc;)Lorg/joml/Matrix4f;") => {
            let src = mat4(rt, a(1));
            set(rt, src)
        }
        ("mul", "(Lorg/joml/Matrix4fc;)Lorg/joml/Matrix4f;") => {
            let r = mul4(&m, &mat4(rt, a(1)));
            set(rt, r)
        }
        ("mul", "(Lorg/joml/Matrix4fc;Lorg/joml/Matrix4f;)Lorg/joml/Matrix4f;") => {
            let r = mul4(&m, &mat4(rt, a(1)));
            set_mat4(rt, a(2), r);
            a(2)
        }
        ("mulLocal", "(Lorg/joml/Matrix4fc;)Lorg/joml/Matrix4f;") => {
            let r = mul4(&mat4(rt, a(1)), &m);
            set(rt, r)
        }
        ("translate", "(FFF)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &translation(a(1).f(), a(2).f(), a(3).f()))),
        ("translate", "(Lorg/joml/Vector3fc;)Lorg/joml/Matrix4f;") => {
            let [x, y, z] = vec3(rt, a(1));
            set(rt, mul4(&m, &translation(x, y, z)))
        }
        ("translateLocal", "(FFF)Lorg/joml/Matrix4f;") => set(rt, mul4(&translation(a(1).f(), a(2).f(), a(3).f()), &m)),
        ("translation", "(FFF)Lorg/joml/Matrix4f;") => set(rt, translation(a(1).f(), a(2).f(), a(3).f())),
        ("scale", "(FFF)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &scaling(a(1).f(), a(2).f(), a(3).f()))),
        ("scale", "(F)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &scaling(a(1).f(), a(1).f(), a(1).f()))),
        ("rotate", "(Lorg/joml/Quaternionfc;)Lorg/joml/Matrix4f;") => {
            let q = quat(rt, a(1));
            set(rt, mul4(&m, &quat_m4(q)))
        }
        ("rotate", "(FFFF)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &quat_m4(quat_axis(a(1).f(), a(2).f(), a(3).f(), a(4).f())))),
        ("rotateX", "(F)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &quat_m4(quat_x(a(1).f())))),
        ("rotateY", "(F)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &quat_m4(quat_y(a(1).f())))),
        ("rotateZ", "(F)Lorg/joml/Matrix4f;") => set(rt, mul4(&m, &quat_m4(quat_z(a(1).f())))),
        ("rotation", "(Lorg/joml/Quaternionfc;)Lorg/joml/Matrix4f;") => {
            let q = quat(rt, a(1));
            set(rt, quat_m4(q))
        }
        ("rotateAround", "(Lorg/joml/Quaternionfc;FFF)Lorg/joml/Matrix4f;") => {
            let q = quat(rt, a(1));
            let (x, y, z) = (a(2).f(), a(3).f(), a(4).f());
            set(rt, mul4(&mul4(&mul4(&m, &translation(x, y, z)), &quat_m4(q)), &translation(-x, -y, -z)))
        }
        ("invert", "()Lorg/joml/Matrix4f;") => set(rt, invert4(&m)),
        ("invert", "(Lorg/joml/Matrix4f;)Lorg/joml/Matrix4f;") => {
            set_mat4(rt, a(1), invert4(&m));
            a(1)
        }
        ("normal", "(Lorg/joml/Matrix3f;)Lorg/joml/Matrix3f;") => {
            set_mat3(rt, a(1), normal_of(&m));
            a(1)
        }
        ("transformPosition", "(Lorg/joml/Vector3f;)Lorg/joml/Vector3f;") => {
            let [x, y, z] = vec3(rt, a(1));
            let [x, y, z, _] = transform4(&m, [x, y, z, 1.0]);
            set_vec3(rt, a(1), [x, y, z]);
            a(1)
        }
        ("transformDirection", "(Lorg/joml/Vector3f;)Lorg/joml/Vector3f;") => {
            let [x, y, z] = vec3(rt, a(1));
            let [x, y, z, _] = transform4(&m, [x, y, z, 0.0]);
            set_vec3(rt, a(1), [x, y, z]);
            a(1)
        }
        ("transform", "(Lorg/joml/Vector4f;)Lorg/joml/Vector4f;") => {
            let v = transform4(&m, vec4(rt, a(1)));
            set_vec4(rt, a(1), v);
            a(1)
        }
        ("getTranslation", "(Lorg/joml/Vector3f;)Lorg/joml/Vector3f;") => {
            set_vec3(rt, a(1), [m[12], m[13], m[14]]);
            a(1)
        }
        ("get", "([F)[F") => {
            if let Some(Data::Array(out)) = rt.heap.get_mut(a(1).r() as usize).map(|o| &mut o.data) {
                for (slot, f) in out.iter_mut().zip(m) {
                    *slot = V::F(f);
                }
            }
            a(1)
        }
        (n, "()F") if n.len() == 3 && n.starts_with('m') => {
            let b = n.as_bytes();
            let (c, r) = ((b[1] - b'0') as usize, (b[2] - b'0') as usize);
            V::F(*m.get(c * 4 + r)?)
        }
        _ => return None,
    })
}

fn matrix3(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let m = mat3(rt, this);
    let set = |rt: &mut Rt, n: M3| {
        set_mat3(rt, this, n);
        this
    };
    Some(match (name, desc) {
        ("<init>", "()V") => {
            set(rt, IDENTITY3);
            V::Void
        }
        ("<init>", "(Lorg/joml/Matrix3fc;)V") => {
            let src = mat3(rt, a(1));
            set(rt, src);
            V::Void
        }
        ("<init>", "(Lorg/joml/Matrix4fc;)V") => {
            let src = m4_to_m3(&mat4(rt, a(1)));
            set(rt, src);
            V::Void
        }
        ("identity", _) => set(rt, IDENTITY3),
        ("set", "(Lorg/joml/Matrix3fc;)Lorg/joml/Matrix3f;") => {
            let src = mat3(rt, a(1));
            set(rt, src)
        }
        ("set", "(Lorg/joml/Matrix4fc;)Lorg/joml/Matrix3f;") => {
            let src = m4_to_m3(&mat4(rt, a(1)));
            set(rt, src)
        }
        ("mul", "(Lorg/joml/Matrix3fc;)Lorg/joml/Matrix3f;") => {
            let r = mul3(&m, &mat3(rt, a(1)));
            set(rt, r)
        }
        ("rotate", "(Lorg/joml/Quaternionfc;)Lorg/joml/Matrix3f;") => {
            let q = quat(rt, a(1));
            set(rt, mul3(&m, &quat_m3(q)))
        }
        ("rotation", "(Lorg/joml/Quaternionfc;)Lorg/joml/Matrix3f;") => {
            let q = quat(rt, a(1));
            set(rt, quat_m3(q))
        }
        ("scale", "(FFF)Lorg/joml/Matrix3f;") => set(rt, mul3(&m, &[a(1).f(), 0.0, 0.0, 0.0, a(2).f(), 0.0, 0.0, 0.0, a(3).f()])),
        ("scale", "(F)Lorg/joml/Matrix3f;") => set(rt, mul3(&m, &[a(1).f(), 0.0, 0.0, 0.0, a(1).f(), 0.0, 0.0, 0.0, a(1).f()])),
        ("invert", "()Lorg/joml/Matrix3f;") => set(rt, invert3(&m)),
        ("transpose", "()Lorg/joml/Matrix3f;") => set(rt, transpose3(&m)),
        ("transform", "(Lorg/joml/Vector3f;)Lorg/joml/Vector3f;") => {
            let v = transform3(&m, vec3(rt, a(1)));
            set_vec3(rt, a(1), v);
            a(1)
        }
        (n, "()F") if n.len() == 3 && n.starts_with('m') => {
            let b = n.as_bytes();
            let (c, r) = ((b[1] - b'0') as usize, (b[2] - b'0') as usize);
            V::F(*m.get(c * 3 + r)?)
        }
        _ => return None,
    })
}

fn quaternion(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let q = quat(rt, this);
    let set = |rt: &mut Rt, n: [f32; 4]| {
        set_quat(rt, this, n);
        this
    };
    let f = |i| a(i).f();
    Some(match (name, desc) {
        ("<init>", "()V") => {
            set(rt, [0.0, 0.0, 0.0, 1.0]);
            V::Void
        }
        ("<init>", "(FFFF)V") => {
            set(rt, [f(1), f(2), f(3), f(4)]);
            V::Void
        }
        ("<init>", "(Lorg/joml/Quaternionfc;)V") => {
            let src = quat(rt, a(1));
            set(rt, src);
            V::Void
        }
        ("identity", _) => set(rt, [0.0, 0.0, 0.0, 1.0]),
        ("set", "(Lorg/joml/Quaternionfc;)Lorg/joml/Quaternionf;") => {
            let src = quat(rt, a(1));
            set(rt, src)
        }
        ("set", "(FFFF)Lorg/joml/Quaternionf;") => set(rt, [f(1), f(2), f(3), f(4)]),
        ("mul", "(Lorg/joml/Quaternionfc;)Lorg/joml/Quaternionf;") => {
            let r = quat_mul(q, quat(rt, a(1)));
            set(rt, r)
        }
        ("mul", "(Lorg/joml/Quaternionfc;Lorg/joml/Quaternionf;)Lorg/joml/Quaternionf;") => {
            let r = quat_mul(q, quat(rt, a(1)));
            set_quat(rt, a(2), r);
            a(2)
        }
        ("premul", "(Lorg/joml/Quaternionfc;)Lorg/joml/Quaternionf;") => {
            let r = quat_mul(quat(rt, a(1)), q);
            set(rt, r)
        }
        ("rotationX", _) => set(rt, quat_x(f(1))),
        ("rotationY", _) => set(rt, quat_y(f(1))),
        ("rotationZ", _) => set(rt, quat_z(f(1))),
        ("rotateX", "(F)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_x(f(1)))),
        ("rotateY", "(F)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_y(f(1)))),
        ("rotateZ", "(F)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_z(f(1)))),
        ("rotationXYZ", "(FFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(quat_mul(quat_x(f(1)), quat_y(f(2))), quat_z(f(3)))),
        ("rotationYXZ", "(FFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(quat_mul(quat_y(f(1)), quat_x(f(2))), quat_z(f(3)))),
        ("rotationZYX", "(FFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(quat_mul(quat_z(f(1)), quat_y(f(2))), quat_x(f(3)))),
        ("rotateXYZ", "(FFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_mul(quat_mul(quat_x(f(1)), quat_y(f(2))), quat_z(f(3))))),
        ("rotateYXZ", "(FFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_mul(quat_mul(quat_y(f(1)), quat_x(f(2))), quat_z(f(3))))),
        ("rotateZYX", "(FFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_mul(quat_mul(quat_z(f(1)), quat_y(f(2))), quat_x(f(3))))),
        ("rotationAxis", "(FFFF)Lorg/joml/Quaternionf;") => set(rt, quat_axis(f(1), f(2), f(3), f(4))),
        // JOML's `setAngleAxis`: the axis as given (not normalized), the angle in radians.
        ("setAngleAxis", "(FFFF)Lorg/joml/Quaternionf;") => {
            let (s, c) = (f(1) * 0.5).sin_cos();
            set(rt, [f(2) * s, f(3) * s, f(4) * s, c])
        }
        ("setAngleAxis", "(DDDD)Lorg/joml/Quaternionf;") => {
            let d = |i| a(i).d();
            let (s, c) = (d(1) * 0.5).sin_cos();
            set(rt, [(d(2) * s) as f32, (d(3) * s) as f32, (d(4) * s) as f32, c as f32])
        }
        ("rotationAxis", "(FLorg/joml/Vector3fc;)Lorg/joml/Quaternionf;") => {
            let [x, y, z] = vec3(rt, a(2));
            set(rt, quat_axis(f(1), x, y, z))
        }
        ("rotateAxis", "(FFFF)Lorg/joml/Quaternionf;") => set(rt, quat_mul(q, quat_axis(f(1), f(2), f(3), f(4)))),
        ("rotateAxis", "(FLorg/joml/Vector3fc;)Lorg/joml/Quaternionf;") => {
            let [x, y, z] = vec3(rt, a(2));
            set(rt, quat_mul(q, quat_axis(f(1), x, y, z)))
        }
        ("fromAxisAngleRad", "(FFFF)Lorg/joml/Quaternionf;") => set(rt, quat_axis(f(4), f(1), f(2), f(3))),
        ("fromAxisAngleDeg", "(FFFF)Lorg/joml/Quaternionf;") => set(rt, quat_axis(f(4).to_radians(), f(1), f(2), f(3))),
        ("conjugate", "()Lorg/joml/Quaternionf;") => set(rt, [-q[0], -q[1], -q[2], q[3]]),
        ("invert", "()Lorg/joml/Quaternionf;") => {
            let n = q.iter().map(|v| v * v).sum::<f32>();
            let n = if n == 0.0 { 1.0 } else { n };
            set(rt, [-q[0] / n, -q[1] / n, -q[2] / n, q[3] / n])
        }
        ("normalize", "()Lorg/joml/Quaternionf;") => {
            let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
            let n = if n == 0.0 { 1.0 } else { n };
            set(rt, q.map(|v| v / n))
        }
        ("transform", "(Lorg/joml/Vector3f;)Lorg/joml/Vector3f;") => {
            let v = quat_rotate(q, vec3(rt, a(1)));
            set_vec3(rt, a(1), v);
            a(1)
        }
        ("x", "()F") => V::F(q[0]),
        ("y", "()F") => V::F(q[1]),
        ("z", "()F") => V::F(q[2]),
        ("w", "()F") => V::F(q[3]),
        _ => return None,
    })
}

fn vector3(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let v = vec3(rt, this);
    let set = |rt: &mut Rt, n: [f32; 3]| {
        set_vec3(rt, this, n);
        this
    };
    let f = |i| a(i).f();
    Some(match (name, desc) {
        ("<init>", "()V") => {
            set(rt, [0.0; 3]);
            V::Void
        }
        ("<init>", "(FFF)V") => {
            set(rt, [f(1), f(2), f(3)]);
            V::Void
        }
        ("<init>", "(F)V") => {
            set(rt, [f(1); 3]);
            V::Void
        }
        ("<init>", "(Lorg/joml/Vector3fc;)V") => {
            let src = vec3(rt, a(1));
            set(rt, src);
            V::Void
        }
        ("set", "(FFF)Lorg/joml/Vector3f;") => set(rt, [f(1), f(2), f(3)]),
        ("set", "(Lorg/joml/Vector3fc;)Lorg/joml/Vector3f;") => {
            let src = vec3(rt, a(1));
            set(rt, src)
        }
        ("zero", _) => set(rt, [0.0; 3]),
        ("x", "()F") => V::F(v[0]),
        ("y", "()F") => V::F(v[1]),
        ("z", "()F") => V::F(v[2]),
        ("add", "(FFF)Lorg/joml/Vector3f;") => set(rt, [v[0] + f(1), v[1] + f(2), v[2] + f(3)]),
        ("add", "(Lorg/joml/Vector3fc;)Lorg/joml/Vector3f;") => {
            let o = vec3(rt, a(1));
            set(rt, [v[0] + o[0], v[1] + o[1], v[2] + o[2]])
        }
        ("sub", "(FFF)Lorg/joml/Vector3f;") => set(rt, [v[0] - f(1), v[1] - f(2), v[2] - f(3)]),
        ("sub", "(Lorg/joml/Vector3fc;)Lorg/joml/Vector3f;") => {
            let o = vec3(rt, a(1));
            set(rt, [v[0] - o[0], v[1] - o[1], v[2] - o[2]])
        }
        ("mul", "(F)Lorg/joml/Vector3f;") => set(rt, v.map(|c| c * f(1))),
        ("mul", "(FFF)Lorg/joml/Vector3f;") => set(rt, [v[0] * f(1), v[1] * f(2), v[2] * f(3)]),
        ("mul", "(Lorg/joml/Vector3fc;)Lorg/joml/Vector3f;") => {
            let o = vec3(rt, a(1));
            set(rt, [v[0] * o[0], v[1] * o[1], v[2] * o[2]])
        }
        ("mul", "(Lorg/joml/Matrix3fc;)Lorg/joml/Vector3f;") | ("transform", "(Lorg/joml/Matrix3fc;)Lorg/joml/Vector3f;") => {
            let m = mat3(rt, a(1));
            set(rt, transform3(&m, v))
        }
        ("rotate", "(Lorg/joml/Quaternionfc;)Lorg/joml/Vector3f;") => {
            let q = quat(rt, a(1));
            set(rt, quat_rotate(q, v))
        }
        ("negate", "()Lorg/joml/Vector3f;") => set(rt, v.map(|c| -c)),
        ("normalize", "()Lorg/joml/Vector3f;") => {
            let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            set(rt, if n == 0.0 { v } else { v.map(|c| c / n) })
        }
        ("length", "()F") => V::F((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()),
        ("lengthSquared", "()F") => V::F(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]),
        ("dot", "(Lorg/joml/Vector3fc;)F") => {
            let o = vec3(rt, a(1));
            V::F(v[0] * o[0] + v[1] * o[1] + v[2] * o[2])
        }
        ("cross", "(Lorg/joml/Vector3fc;)Lorg/joml/Vector3f;") => {
            let o = vec3(rt, a(1));
            set(rt, [v[1] * o[2] - v[2] * o[1], v[2] * o[0] - v[0] * o[2], v[0] * o[1] - v[1] * o[0]])
        }
        ("distance", "(Lorg/joml/Vector3fc;)F") => {
            let o = vec3(rt, a(1));
            V::F(((v[0] - o[0]).powi(2) + (v[1] - o[1]).powi(2) + (v[2] - o[2]).powi(2)).sqrt())
        }
        ("lerp", "(Lorg/joml/Vector3fc;F)Lorg/joml/Vector3f;") => {
            let o = vec3(rt, a(1));
            let t = f(2);
            set(rt, [v[0] + (o[0] - v[0]) * t, v[1] + (o[1] - v[1]) * t, v[2] + (o[2] - v[2]) * t])
        }
        _ => return None,
    })
}

fn vector4(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let v = vec4(rt, this);
    let set = |rt: &mut Rt, n: [f32; 4]| {
        set_vec4(rt, this, n);
        this
    };
    let f = |i| a(i).f();
    Some(match (name, desc) {
        ("<init>", "()V") => {
            set(rt, [0.0, 0.0, 0.0, 1.0]);
            V::Void
        }
        ("<init>", "(FFFF)V") => {
            set(rt, [f(1), f(2), f(3), f(4)]);
            V::Void
        }
        ("set", "(FFFF)Lorg/joml/Vector4f;") => set(rt, [f(1), f(2), f(3), f(4)]),
        ("mul", "(Lorg/joml/Matrix4fc;)Lorg/joml/Vector4f;") => {
            let m = mat4(rt, a(1));
            set(rt, transform4(&m, v))
        }
        ("x", "()F") => V::F(v[0]),
        ("y", "()F") => V::F(v[1]),
        ("z", "()F") => V::F(v[2]),
        ("w", "()F") => V::F(v[3]),
        _ => return None,
    })
}

// ---- PoseStack: a list of poses, each a pose matrix and a normal matrix ----

fn new_pose(rt: &mut Rt, m: M4, n: M3) -> V {
    let pm = new_mat4(rt, m);
    let nm = new_mat3(rt, n);
    let p = rt.new_object("com/mojang/blaze3d/vertex/PoseStack$Pose");
    rt.put_field(p, "pose", pm);
    rt.put_field(p, "normal", nm);
    p
}

/// A new `PoseStack` at `m` (the identity for vanilla's constructor).
pub fn new_pose_stack(rt: &mut Rt, m: M4) -> V {
    let p = new_pose(rt, m, normal_of(&m));
    rt.alloc("com/mojang/blaze3d/vertex/PoseStack", Data::List(vec![p]))
}

fn poses(rt: &Rt, stack: V) -> Vec<V> {
    match rt.heap.get(stack.r() as usize).map(|o| &o.data) {
        Some(Data::List(l)) => l.clone(),
        _ => Vec::new(),
    }
}

/// The top pose of `stack` (`PoseStack.last()`).
fn last(rt: &Rt, stack: V) -> V {
    match rt.heap.get(stack.r() as usize).map(|o| &o.data) {
        Some(Data::List(l)) => l.last().copied().unwrap_or_default(),
        _ => V::NULL,
    }
}

/// `stack.last().pose()` as a matrix.
pub fn last_pose(rt: &Rt, stack: V) -> M4 {
    let p = last(rt, stack);
    mat4(rt, rt.get_field(p, "pose"))
}

fn pose_mats(rt: &Rt, p: V) -> (V, V) {
    (rt.get_field(p, "pose"), rt.get_field(p, "normal"))
}

fn pose_stack(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    if name == "<init>" {
        let p = new_pose(rt, IDENTITY, IDENTITY3);
        if let Some(o) = rt.heap.get_mut(this.r() as usize) {
            o.data = Data::List(vec![p]);
        }
        return Some(V::Void);
    }
    let top = last(rt, this);
    let (pm, nm) = pose_mats(rt, top);
    let (m, n) = (mat4(rt, pm), mat3(rt, nm));
    Some(match (name, desc) {
        ("pushPose", _) => {
            let copy = new_pose(rt, m, n);
            if let Some(Data::List(l)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                l.push(copy);
            }
            V::Void
        }
        ("popPose", _) => {
            if let Some(Data::List(l)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data)
                && l.len() > 1
            {
                l.pop();
            }
            V::Void
        }
        ("last", _) => top,
        ("clear" | "isEmpty", "()Z") => V::I((poses(rt, this).len() <= 1) as i32),
        ("setIdentity", _) => {
            set_mat4(rt, pm, IDENTITY);
            set_mat3(rt, nm, IDENTITY3);
            V::Void
        }
        ("translate", "(DDD)V") => {
            set_mat4(rt, pm, mul4(&m, &translation(a(1).d() as f32, a(2).d() as f32, a(3).d() as f32)));
            V::Void
        }
        ("translate", "(FFF)V") => {
            set_mat4(rt, pm, mul4(&m, &translation(a(1).f(), a(2).f(), a(3).f())));
            V::Void
        }
        ("translate", "(Lnet/minecraft/world/phys/Vec3;)V") => {
            let to = ["x", "y", "z"].map(|c| rt.get_field(a(1), c).d() as f32);
            set_mat4(rt, pm, mul4(&m, &translation(to[0], to[1], to[2])));
            V::Void
        }
        ("scale", "(FFF)V") => {
            let (x, y, z) = (a(1).f(), a(2).f(), a(3).f());
            set_mat4(rt, pm, mul4(&m, &scaling(x, y, z)));
            // PoseStack.scale: a uniform scale keeps normals (but for their sign), any other
            // divides them.
            if x.abs() == y.abs() && y.abs() == z.abs() {
                if x < 0.0 || y < 0.0 || z < 0.0 {
                    set_mat3(rt, nm, mul3(&n, &[x.signum(), 0.0, 0.0, 0.0, y.signum(), 0.0, 0.0, 0.0, z.signum()]));
                }
            } else {
                set_mat3(rt, nm, mul3(&n, &[1.0 / x, 0.0, 0.0, 0.0, 1.0 / y, 0.0, 0.0, 0.0, 1.0 / z]));
            }
            V::Void
        }
        ("mulPose", "(Lorg/joml/Quaternionf;)V" | "(Lorg/joml/Quaternionfc;)V") => {
            let q = quat(rt, a(1));
            set_mat4(rt, pm, mul4(&m, &quat_m4(q)));
            set_mat3(rt, nm, mul3(&n, &quat_m3(q)));
            V::Void
        }
        ("mulPose", "(Lorg/joml/Matrix4f;)V" | "(Lorg/joml/Matrix4fc;)V") => {
            let by = mat4(rt, a(1));
            let r = mul4(&m, &by);
            set_mat4(rt, pm, r);
            set_mat3(rt, nm, normal_of(&r));
            V::Void
        }
        ("rotateAround", "(Lorg/joml/Quaternionf;FFF)V" | "(Lorg/joml/Quaternionfc;FFF)V") => {
            let q = quat(rt, a(1));
            let (x, y, z) = (a(2).f(), a(3).f(), a(4).f());
            set_mat4(rt, pm, mul4(&mul4(&mul4(&m, &translation(x, y, z)), &quat_m4(q)), &translation(-x, -y, -z)));
            set_mat3(rt, nm, mul3(&n, &quat_m3(q)));
            V::Void
        }
        _ => return None,
    })
}

fn pose(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let (pm, nm) = pose_mats(rt, this);
    Some(match (name, desc) {
        ("pose", _) => pm,
        ("normal", _) => nm,
        ("copy", _) => {
            let (m, n) = (mat4(rt, pm), mat3(rt, nm));
            new_pose(rt, m, n)
        }
        ("transformNormal", "(FFFLorg/joml/Vector3f;)Lorg/joml/Vector3f;") => {
            let n = mat3(rt, nm);
            let v = transform3(&n, [a(1).f(), a(2).f(), a(3).f()]);
            set_vec3(rt, a(4), v);
            a(4)
        }
        _ => return None,
    })
}

/// `Axis.XP.rotationDegrees(90)` and the like: a quaternion about a fixed axis.
fn axis(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let angle = match name {
        "rotation" => a(1).f(),
        "rotationDegrees" => a(1).f().to_radians(),
        "of" => {
            let [x, y, z] = vec3(rt, a(0));
            let o = rt.new_object("com/mojang/math/Axis");
            rt.put_field(o, "$axis", V::I(6));
            set_vec3(rt, o, [x, y, z]);
            return Some(o);
        }
        _ => return None,
    };
    let q = match rt.get_field(a(0), "$axis").i() {
        0 => quat_x(-angle),
        1 => quat_x(angle),
        2 => quat_y(-angle),
        3 => quat_y(angle),
        4 => quat_z(-angle),
        5 => quat_z(angle),
        _ => {
            let [x, y, z] = vec3(rt, a(0));
            quat_axis(angle, x, y, z)
        }
    };
    Some(new_quat(rt, q))
}

// ---- the client: Minecraft, its renderers, render types, buffers ----

const ITEM_RENDERER: &str = "net/minecraft/client/renderer/entity/ItemRenderer";
/// `ItemDisplayContext`'s constants, in their order.
const DISPLAY_CONTEXTS: [&str; 9] = [
    "NONE",
    "THIRD_PERSON_LEFT_HAND",
    "THIRD_PERSON_RIGHT_HAND",
    "FIRST_PERSON_LEFT_HAND",
    "FIRST_PERSON_RIGHT_HAND",
    "HEAD",
    "GUI",
    "GROUND",
    "FIXED",
];

/// An item a renderer draws: `stack` under `pose_stack`'s pose, in display context `context`.
fn draw_item(rt: &mut Rt, stack: V, context: V, pose_stack: V, light: i32, seed: i32) {
    if crate::api_common::stack_empty(rt, stack) {
        return;
    }
    let item = crate::api_common::item_like_name(rt, rt.get_field(stack, "$item"));
    let display = rt.get_field(context, "$ordinal").i().clamp(0, 8) as u8;
    let m = last_pose(rt, pose_stack);
    let drew = rt.host_mut().is_some_and(|h| h.draw_item(&item, display, &m, light as u32, seed));
    crate::glue::trace_once(rt, &format!("draw-item/{item}/{display}"), |_| {
        format!("client: item {item} (display {display}) at {:.3} {:.3} {:.3}, light {light:#x}: drawn {drew}", m[12], m[13], m[14])
    });
}

/// The interned object for a client singleton (`Minecraft`, its block renderer...).
fn single(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("client/{class}"), |rt| rt.new_object(class))
}

/// A `RenderType` by name (`solid`, `cutout_mipped`, `entity_cutout`...): one object each.
pub fn render_type(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("render-type/{name}"), move |rt| {
        let o = rt.new_object("net/minecraft/client/renderer/RenderType");
        let s = rt.string(&n);
        rt.put_field(o, "$name", s);
        o
    })
}

/// `RenderType.chunkBufferLayers()`, in vanilla's order.
const CHUNK_LAYERS: [&str; 5] = ["solid", "cutout_mipped", "cutout", "translucent", "tripwire"];

/// snake_case of a `RenderType` factory's name (`cutoutMipped` → `cutout_mipped`).
fn snake(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

const TEXTURE_ATLAS: &str = "net/minecraft/client/renderer/texture/TextureAtlas";
const SPRITE: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite";
const SPRITE_CONTENTS: &str = "net/minecraft/client/renderer/texture/SpriteContents";
pub const ATLAS_STITCHED: &str = "net/neoforged/neoforge/client/event/TextureAtlasStitchedEvent";

/// The block atlas, as `TextureAtlasStitchedEvent.getAtlas()` hands it to mods (catnip's
/// `StitchedSprite`s look their sprites up in it).
pub fn block_atlas(rt: &mut Rt) -> V {
    rt.intern("compat:block-atlas".into(), |rt| {
        let atlas = rt.new_object(TEXTURE_ATLAS);
        let location = rt.string("minecraft:textures/atlas/blocks.png");
        rt.put_field(atlas, "$location", location);
        atlas
    })
}

/// Sprites across (and down) this runtime's block atlas. Each sprite has a cell of its own, so a UV
/// names its sprite as it does in vanilla's stitched atlas: mods retexture a face by moving its UVs
/// into another sprite's cell (Create's water wheels and their wood), and the face drawn shows the
/// sprite its UVs are in. The game draws each sprite from its own texture, 0 to 1 across.
pub const CELLS: u32 = 256;
/// How wide (and high) a cell is.
pub const CELL: f32 = 1.0 / CELLS as f32;

/// The block atlas's sprite `location` (`create:block/belt_scroll`).
fn sprite(rt: &mut Rt, location: V) -> V {
    let name = rt.str_of(location).unwrap_or("minecraft:missingno").to_owned();
    sprite_named(rt, &name)
}

/// [`sprite`] by name: one object a sprite, as Java's atlas holds them (mods key maps by them),
/// in the next free cell.
pub fn sprite_named(rt: &mut Rt, name: &str) -> V {
    let name = if name.contains(':') { name.to_owned() } else { format!("minecraft:{name}") };
    rt.intern(format!("compat:sprite/{name}"), |rt| {
        let s = rt.new_object(SPRITE);
        let n = rt.string(&name);
        rt.put_field(s, "$name", n);
        let atlas = block_atlas(rt);
        let cell = rt.get_field(atlas, "$cells").i();
        rt.put_field(atlas, "$cells", V::I(cell + 1));
        rt.put_field(s, "$cell", V::I(cell));
        rt.interned.insert(format!("compat:cell/{}", cell as u32 % (CELLS * CELLS)), s);
        s
    })
}

/// Where `sprite`'s cell starts in the atlas (`getU0`, `getV0`); None for what is not a sprite.
pub fn sprite_origin(rt: &Rt, sprite: V) -> Option<(f32, f32)> {
    let V::I(cell) = rt.get_field(sprite, "$cell") else { return None };
    let cell = cell as u32 % (CELLS * CELLS);
    Some(((cell % CELLS) as f32 * CELL, (cell / CELLS) as f32 * CELL))
}

/// The sprite whose cell holds atlas coordinates `(u, v)`, if one does.
fn sprite_at(rt: &Rt, u: f32, v: f32) -> Option<V> {
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return None;
    }
    let (col, row) = ((u * CELLS as f32) as u32, (v * CELLS as f32) as u32);
    rt.interned.get(&format!("compat:cell/{}", row * CELLS + col)).copied()
}

/// The sprite a face's atlas coordinates are in, and where in it each corner is (0 to 1 across):
/// the face's own sprite while they stay in its cell, else whichever sprite's cell holds them (a mod
/// moved them there to retexture the face, keeping the sprite it was baked with, as Create's
/// `BakedModelHelper.swapSprites` does).
pub fn locate(rt: &Rt, own: V, uv: &[[f32; 2]; 4]) -> (V, [[f32; 2]; 4]) {
    const EDGE: f32 = CELL / 4096.0;
    let centre = [uv.iter().map(|c| c[0]).sum::<f32>() / 4.0, uv.iter().map(|c| c[1]).sum::<f32>() / 4.0];
    let holds = |(u0, v0): (f32, f32)| (u0 - EDGE..=u0 + CELL + EDGE).contains(&centre[0]) && (v0 - EDGE..=v0 + CELL + EDGE).contains(&centre[1]);
    let sprite = if sprite_origin(rt, own).is_some_and(holds) { own } else { sprite_at(rt, centre[0], centre[1]).unwrap_or(own) };
    let Some((u0, v0)) = sprite_origin(rt, sprite) else { return (sprite, *uv) };
    (sprite, uv.map(|[u, v]| [(u - u0) / CELL, (v - v0) / CELL]))
}

fn client(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        (ATLAS_STITCHED, "getAtlas") => rt.get_field(a(0), "$atlas"),
        (TEXTURE_ATLAS, "location") => rt.get_field(a(0), "$location"),
        (TEXTURE_ATLAS, "getSprite") => sprite(rt, a(1)),
        (SPRITE, n) if !matches!(n, "atlasLocation" | "contents") => {
            let (u0, v0) = sprite_origin(rt, a(0)).unwrap_or_default();
            V::F(match n {
                "getU0" => u0,
                "getV0" => v0,
                "getU1" => u0 + CELL,
                "getV1" => v0 + CELL,
                // `getU(0.5f)` is halfway across; `getUOffset` the way back.
                "getU" => u0 + a(1).f() * CELL,
                "getV" => v0 + a(1).f() * CELL,
                "getUOffset" => (a(1).f() - u0) / CELL,
                "getVOffset" => (a(1).f() - v0) / CELL,
                _ => return None,
            })
        }
        (SPRITE, n) => match n {
            "atlasLocation" => {
                let atlas = block_atlas(rt);
                rt.get_field(atlas, "$location")
            }
            "contents" => {
                let c = rt.new_object(SPRITE_CONTENTS);
                let n = rt.get_field(a(0), "$name");
                rt.put_field(c, "$name", n);
                c
            }
            _ => return None,
        },
        (SPRITE_CONTENTS, "name") => rt.get_field(a(0), "$name"),
        // The game culls a block entity before asking its renderer to draw it, so what the renderer
        // checks again (Create's belt, each item it carries) is in view.
        ("net/minecraft/client/renderer/culling/Frustum", "isVisible" | "pointInFrustum") => V::I(1),
        ("net/minecraft/client/Minecraft", "getInstance") => {
            let mc = single(rt, "net/minecraft/client/Minecraft");
            // Its public fields: the level this client draws, and its player (mods' client ticks
            // run only with both: Create's `ClientEvents.isGameActive`).
            if rt.get_field(mc, "level").r() == 0 && crate::neo::is_client(rt) {
                let level = crate::glue::level(rt);
                rt.put_field(mc, "level", level);
                let player = crate::network::local_player(rt);
                rt.put_field(mc, "player", player);
            }
            mc
        }
        ("net/minecraft/client/Minecraft", "getBlockRenderer") => single(rt, "net/minecraft/client/renderer/block/BlockRenderDispatcher"),
        ("net/minecraft/client/Minecraft", "getTimer") => single(rt, "net/minecraft/client/DeltaTracker"),
        ("net/minecraft/client/Minecraft", "getModelManager") => single(rt, "net/minecraft/client/resources/model/ModelManager"),
        ("net/minecraft/client/Minecraft", "getItemRenderer") => single(rt, "net/minecraft/client/renderer/entity/ItemRenderer"),
        // Items a block entity's renderer draws (Create's depot, belt, funnel, deployer...): the game
        // draws each one as it draws an item in that display context.
        (ITEM_RENDERER, "render") if desc.starts_with("(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemDisplayContext;Z") => {
            // (stack, context, leftHand, poseStack, buffers, light, overlay, model)
            draw_item(rt, arg(args, 1), arg(args, 2), arg(args, 4), arg(args, 6).i(), 0);
            V::Void
        }
        (ITEM_RENDERER, "renderStatic") => {
            // (stack, context, light, overlay, poseStack, buffers, level, seed), or with the holding
            // entity first: (entity, stack, context, leftHand, poseStack, buffers, level, light,
            // overlay, seed).
            let at = if desc.starts_with("(Lnet/minecraft/world/item/ItemStack;") { [1, 2, 5, 3, 8] } else { [2, 3, 5, 8, 10] };
            let [stack, context, pose, light, seed] = at.map(|i| arg(args, i));
            draw_item(rt, stack, context, pose, light.i(), seed.i());
            V::Void
        }
        // The stack's item model, standing for it: what `isGui3d` asks about.
        (ITEM_RENDERER, "getModel") => {
            let m = rt.new_object("net/minecraft/client/resources/model/BakedModel");
            let item = crate::api_common::item_like_name(rt, rt.get_field(arg(args, 1), "$item"));
            let s = rt.string(&item);
            rt.put_field(m, "$item", s);
            m
        }
        ("net/minecraft/client/resources/model/BakedModel", "isGui3d") if rt.get_field(arg(args, 0), "$item").r() != 0 => {
            let item = rt.str_of(rt.get_field(arg(args, 0), "$item")).unwrap_or("").to_owned();
            V::I(rt.host().is_some_and(|h| h.item_is_block(&item)) as i32)
        }
        ("net/minecraft/client/Minecraft", "getBlockEntityRenderDispatcher") => {
            single(rt, "net/minecraft/client/renderer/blockentity/BlockEntityRenderDispatcher")
        }
        ("net/minecraft/client/Minecraft", "getEntityRenderDispatcher") => single(rt, "net/minecraft/client/renderer/entity/EntityRenderDispatcher"),
        ("net/minecraft/client/Minecraft", "isPaused") => V::I(0),
        // F3: closed (debug overlays a mod draws over the world stay off).
        ("net/minecraft/client/Minecraft", "getDebugOverlay") => single(rt, "net/minecraft/client/gui/components/DebugScreenOverlay"),
        ("net/minecraft/client/gui/components/DebugScreenOverlay", _) if desc.ends_with(")Z") => V::I(0),
        ("net/minecraft/client/Minecraft", "isSameThread") => V::I(1),
        ("net/minecraft/client/DeltaTracker" | "net/minecraft/client/DeltaTracker$Timer", n) => match n {
            "getGameTimeDeltaPartialTick" => V::F(rt.host().map_or(0.0, |h| h.partial_tick())),
            "getGameTimeDeltaTicks" | "getRealtimeDeltaTicks" => V::F(1.0),
            _ => return None,
        },
        // A block tesselated into a buffer being built (Create's contraption, `ShadedBlockSbbBuilder`):
        // its model under the stack's pose, one of the buffer's parts; nothing for another consumer.
        ("net/minecraft/client/renderer/block/ModelBlockRenderer", "tesselateBlock" | "tesselateWithAO" | "tesselateWithoutAO") => {
            keep_block(rt, a(6), a(2), a(3), a(5));
            V::Void
        }
        ("net/minecraft/client/renderer/block/BlockRenderDispatcher", "renderBatched") => {
            let m = rt.new_object("net/minecraft/client/resources/model/BakedModel");
            rt.put_field(m, "$state", a(1));
            keep_block(rt, a(5), m, a(1), a(4));
            V::Void
        }
        ("net/minecraft/client/renderer/block/BlockRenderDispatcher", "getModelRenderer") => single(rt, "net/minecraft/client/renderer/block/ModelBlockRenderer"),
        // Its cache of neighbours' light and shade: the host lights what it draws.
        ("net/minecraft/client/renderer/block/ModelBlockRenderer", "enableCaching" | "clearCache") => V::Void,
        // A block state's model: drawn by the host when a buffer made from it is.
        ("net/minecraft/client/renderer/block/BlockRenderDispatcher", "getBlockModel") => {
            let m = rt.new_object("net/minecraft/client/resources/model/BakedModel");
            rt.put_field(m, "$state", a(1));
            m
        }
        // A model a mod built of faces answers from them (`SimpleBakedModel`).
        ("net/minecraft/client/resources/model/BakedModel", n) if !matches!(rt.get_field(a(0), "$unculled"), V::Void) => {
            return crate::quads::model_call(rt, n, args).or_else(|| match n {
                "getRenderTypes" => Some(single(rt, "net/neoforged/neoforge/client/ChunkRenderTypeSet")),
                "getModelData" => Some(a(4)),
                _ => None,
            });
        }
        ("net/minecraft/client/resources/model/BakedModel", n) => match n {
            "getRenderTypes" => single(rt, "net/neoforged/neoforge/client/ChunkRenderTypeSet"),
            "useAmbientOcclusion" | "isGui3d" | "usesBlockLight" => V::I(1),
            "isCustomRenderer" => V::I(0),
            // A probe model (`glue::render_shape`) has a quad on every side, so a wrapper passing
            // them through is told apart from one that draws nothing. Otherwise the game's faces of
            // the model, which the mod may read, change and build models from.
            "getQuads" => {
                let probing = rt.get_field(a(0), "$probe").i() != 0 || rt.interned.contains_key("compat:probing");
                match crate::quads::handle(rt, a(0)) {
                    _ if probing => {
                        let this = a(0);
                        crate::jutil::new_list(rt, vec![this])
                    }
                    Some(model) => crate::quads::handle_quads(rt, &model, a(2)),
                    None => crate::jutil::new_list(rt, Vec::new()),
                }
            }
            "getParticleIcon" => match crate::quads::handle(rt, a(0)) {
                Some(model) => crate::quads::handle_particle(rt, &model),
                None => sprite_named(rt, "minecraft:missingno"),
            },
            "getTransforms" => crate::quads::transforms(rt),
            "getModelData" => a(4),
            _ => return None,
        },
        // NeoForge's `BakedModelWrapper`: what the mod's subclass leaves alone, the model it wraps answers.
        (BAKED_MODEL_WRAPPER, "<init>") => {
            rt.put_field(a(0), "originalModel", a(1));
            V::Void
        }
        (BAKED_MODEL_WRAPPER, n) => {
            let inner = rt.get_field(a(0), "originalModel");
            let mut args = args.to_vec();
            if let Some(this) = args.first_mut() {
                *this = inner;
            }
            if n == "getQuads" && inner.r() != 0 {
                let vc = rt.vcall?;
                return Some(if desc == GET_QUADS_DESC {
                    vc(rt, GET_QUADS_KEY, GET_QUADS, &args)
                } else {
                    vc(rt, GET_QUADS_VANILLA_KEY, GET_QUADS_VANILLA, &args)
                });
            }
            return client(rt, "net/minecraft/client/resources/model/BakedModel", n, desc, &args);
        }
        // Model data: values by property (compared by identity, as Java's `ModelProperty` is).
        (MODEL_DATA, n) => match n {
            "builder" => model_data_builder(rt, V::NULL),
            "derive" => model_data_builder(rt, a(0)),
            "has" => V::I(model_data_get(rt, a(0), a(1)).is_some() as i32),
            "get" => model_data_get(rt, a(0), a(1)).unwrap_or(V::NULL),
            "getProperties" => {
                let props = rt.get_field(a(0), "$props");
                let keys: Vec<V> = crate::jutil::table_keys(rt, props);
                crate::jutil::new_set_of(rt, &keys)
            }
            _ => return None,
        },
        ("net/neoforged/neoforge/client/model/data/ModelData$Builder", n) => match n {
            "with" => {
                let props = rt.get_field(a(0), "$props");
                crate::jutil::table_put(rt, props, a(1), a(2));
                a(0)
            }
            "build" => {
                let data = rt.new_object(MODEL_DATA);
                let props = rt.get_field(a(0), "$props");
                let copy = rt.alloc("java/util/HashMap", crate::Data::Table(crate::Table::default()));
                for (k, v) in crate::jutil::table_pairs(rt, props) {
                    crate::jutil::table_put(rt, copy, k, v);
                }
                rt.put_field(data, "$props", copy);
                data
            }
            _ => return None,
        },
        ("net/neoforged/neoforge/client/model/data/ModelProperty", "<init>") => V::Void,
        ("net/neoforged/neoforge/client/model/data/ModelProperty", "test") => V::I(1),
        // The baked models a mod may replace (`glue::render_shape` posts it with probe models).
        ("net/neoforged/neoforge/client/event/ModelEvent$ModifyBakingResult", "getModels") => rt.get_field(a(0), "$models"),
        // A block state's model location (`create:shaft#axis=x`), kept as its string.
        ("net/minecraft/client/renderer/block/BlockModelShaper", "stateToModelLocation") => {
            let (name, state) = if desc.starts_with("(Lnet/minecraft/resources/ResourceLocation;") {
                (rt.str_of(a(0)).unwrap_or("").to_owned(), a(1))
            } else {
                let text = rt.str_of(a(0)).unwrap_or("").to_owned();
                (crate::host::split_state(&text).0.to_owned(), a(0))
            };
            let text = rt.str_of(state).unwrap_or("").to_owned();
            let location = model_location(&name, &text);
            rt.string(&location)
        }
        // Blocks draw in the solid layer here (the host picks its own passes).
        ("net/neoforged/neoforge/client/ChunkRenderTypeSet", n) => match n {
            "contains" => V::I((rt.str_of(rt.get_field(a(1), "$name")) == Some("solid")) as i32),
            "isEmpty" => V::I(0),
            "asList" => {
                let solid = render_type(rt, "solid");
                crate::jutil::new_list(rt, vec![solid])
            }
            "iterator" => {
                let solid = render_type(rt, "solid");
                crate::jutil::new_iter(rt, vec![solid])
            }
            _ => return None,
        },
        ("net/minecraft/client/renderer/RenderType", "chunkBufferLayers") => {
            let layers: Vec<V> = CHUNK_LAYERS.iter().map(|n| render_type(rt, n)).collect();
            crate::jutil::new_list(rt, layers)
        }
        // Any other factory (`solid()`, `entityCutout(texture)`...) names its type.
        ("net/minecraft/client/renderer/RenderType", n) if desc.ends_with(")Lnet/minecraft/client/renderer/RenderType;") => {
            let n = if n == "create" { rt.str_of(a(0)).unwrap_or("custom").to_owned() } else { snake(n) };
            render_type(rt, &n)
        }
        ("net/minecraft/client/renderer/RenderType", "toString" | "name") => rt.get_field(a(0), "$name"),
        ("net/minecraft/client/renderer/MultiBufferSource" | "net/minecraft/client/renderer/MultiBufferSource$BufferSource", "getBuffer") => {
            let ty = a(1);
            rt.intern(format!("consumer/{}", ty.r()), |rt| {
                let c = rt.new_object("com/mojang/blaze3d/vertex/VertexConsumer");
                rt.put_field(c, "$type", ty);
                c
            })
        }
        // Vertices written one by one are not drawn yet; the consumer chains as vanilla's does.
        ("com/mojang/blaze3d/vertex/VertexConsumer", _) => {
            if desc.ends_with(")V") {
                V::Void
            } else {
                a(0)
            }
        }
        ("net/minecraft/client/renderer/texture/OverlayTexture", "pack") => {
            if desc == "(II)I" {
                V::I(a(0).i() | a(1).i() << 16)
            } else {
                V::I((a(0).f() * 15.0) as i32 | (if a(1).i() != 0 { 3 } else { 10 }) << 16)
            }
        }
        ("net/minecraft/client/renderer/texture/OverlayTexture", "u") => V::I((a(0).f() * 15.0) as i32),
        ("net/minecraft/client/renderer/texture/OverlayTexture", "v") => V::I(if a(0).i() != 0 { 3 } else { 10 }),
        ("net/minecraft/client/renderer/LightTexture", "pack") => V::I(a(0).i() << 4 | a(1).i() << 20),
        ("net/minecraft/client/renderer/LightTexture", "block") => V::I(a(0).i() >> 4 & 0xFFFF),
        ("net/minecraft/client/renderer/LightTexture", "sky") => V::I(a(0).i() >> 20 & 0xFFFF),
        // Block entity renderers, by their block entity type's registered name.
        ("net/minecraft/client/renderer/blockentity/BlockEntityRenderers", "register")
        | ("net/neoforged/neoforge/client/event/EntityRenderersEvent$RegisterRenderers", "registerBlockEntityRenderer") => {
            let (ty, provider) = if owner.ends_with("BlockEntityRenderers") { (a(0), a(1)) } else { (a(1), a(2)) };
            if let Some(id) = rt.registered_name(ty, "block_entity_type").map(str::to_owned) {
                rt.interned.insert(format!("ber/{id}"), provider);
            }
            V::Void
        }
        // Entity renderers, by their entity type's registered name (drawn once entities are).
        ("net/neoforged/neoforge/client/event/EntityRenderersEvent$RegisterRenderers", "registerEntityRenderer")
        | ("net/minecraft/client/renderer/entity/EntityRenderers", "register") => {
            let (ty, provider) = if owner.ends_with("EntityRenderers") { (a(0), a(1)) } else { (a(1), a(2)) };
            if let Some(id) = rt.registered_name(ty, "entity_type").map(str::to_owned) {
                rt.interned.insert(format!("er/{id}"), provider);
            }
            V::Void
        }
        // Work for the render thread's next frame (GPU objects): there is no GPU state to set up.
        ("com/mojang/blaze3d/systems/RenderSystem", "recordRenderCall") => V::Void,
        ("net/minecraft/client/renderer/blockentity/BlockEntityRendererProvider$Context", n) => match n {
            "getBlockRenderDispatcher" => single(rt, "net/minecraft/client/renderer/block/BlockRenderDispatcher"),
            "getBlockEntityRenderDispatcher" => single(rt, "net/minecraft/client/renderer/blockentity/BlockEntityRenderDispatcher"),
            "getItemRenderer" => single(rt, "net/minecraft/client/renderer/entity/ItemRenderer"),
            "getEntityRenderer" => single(rt, "net/minecraft/client/renderer/entity/EntityRenderDispatcher"),
            "getModelSet" => single(rt, "net/minecraft/client/model/geom/EntityModelSet"),
            "getFont" => single(rt, "net/minecraft/client/gui/Font"),
            "bakeLayer" => rt.new_object("net/minecraft/client/model/geom/ModelPart"),
            _ => return None,
        },
        ("net/minecraft/client/renderer/entity/EntityRendererProvider$Context", n) => match n {
            "getBlockRenderDispatcher" => single(rt, "net/minecraft/client/renderer/block/BlockRenderDispatcher"),
            "getItemRenderer" => single(rt, "net/minecraft/client/renderer/entity/ItemRenderer"),
            "getEntityRenderDispatcher" => single(rt, "net/minecraft/client/renderer/entity/EntityRenderDispatcher"),
            "getModelSet" => single(rt, "net/minecraft/client/model/geom/EntityModelSet"),
            "getFont" => single(rt, "net/minecraft/client/gui/Font"),
            "getItemInHandRenderer" => single(rt, "net/minecraft/client/renderer/ItemInHandRenderer"),
            "bakeLayer" => rt.new_object("net/minecraft/client/model/geom/ModelPart"),
            _ => return None,
        },
        // Vanilla's base entity renderer, as a mod's renderer reaches it with `super`: its name tag
        // and leash aren't drawn here, and the host culls what is drawn, so it is always in view.
        ("net/minecraft/client/renderer/entity/EntityRenderer", n) => match n {
            "<init>" | "render" => V::Void,
            "shouldRender" => V::I(1),
            "getPackedLightCoords" => V::I(FULL_BRIGHT),
            _ => return None,
        },
        _ => return None,
    })
}

// ---- intrinsics ----

/// catnip's `ShadeSeparatingSuperByteBuffer` drawing `model` (a [`crate::quads::drawable`]) under
/// `base`: its fields as its constructor sets them; the caller runs its `reset()` (white, no
/// overlay, a fresh transform).
fn new_sbb(rt: &mut Rt, model: V, base: M4) -> V {
    let sbb = rt.new_object(SBB);
    let transforms = new_pose_stack(rt, IDENTITY);
    rt.put_field(sbb, "transforms", transforms);
    let model_mat = new_mat4(rt, IDENTITY);
    rt.put_field(sbb, "modelMat", model_mat);
    let normal_mat = rt.alloc("org/joml/Matrix3f", Data::Array(IDENTITY3.iter().map(|&f| V::F(f)).collect()));
    rt.put_field(sbb, "normalMat", normal_mat);
    for (field, class) in [("pos", "org/joml/Vector4f"), ("lightPos", "org/joml/Vector4f"), ("normal", "org/joml/Vector3f"), ("lightDir0", "org/joml/Vector3f"), ("lightDir1", "org/joml/Vector3f")] {
        let v = rt.new_object(class);
        for c in ["x", "y", "z", "w"].iter().take(if class.ends_with("4f") { 4 } else { 3 }) {
            rt.put_field(v, c, V::F(0.0));
        }
        rt.put_field(sbb, field, v);
    }
    let shift = rt.new_object("net/createmod/catnip/render/SuperByteBuffer$ShiftOutput");
    rt.put_field(sbb, "shiftOutput", shift);
    let swap = rt.alloc("[I", Data::Array(Vec::new()));
    rt.put_field(sbb, "shadeSwapVertices", swap);
    rt.put_field(sbb, "invertFakeDiffuseNormal", V::I(0));
    rt.put_field(sbb, "$model", model);
    let base = new_mat4(rt, base);
    rt.put_field(sbb, "$base", base);
    sbb
}

/// A block a renderer tesselates into a builder of [`SBB_BUILDER_BEGIN`] (`consumer`): its model
/// (`model` of `state`) under `stack`'s pose, kept as one of the buffer's parts. False for any
/// other consumer.
fn keep_block(rt: &mut Rt, consumer: V, model: V, state: V, stack: V) -> bool {
    let parts = rt.get_field(consumer, "$parts");
    if parts.r() == 0 || consumer.r() == 0 {
        return false;
    }
    let drawable = crate::quads::drawable(rt, model, state);
    let pose = last_pose(rt, stack);
    let pose = new_mat4(rt, pose);
    let part = rt.new_object("compat/SbbPart");
    rt.put_field(part, "$model", drawable);
    rt.put_field(part, "$pose", pose);
    if let Some(Data::List(l)) = rt.heap.get_mut(parts.r() as usize).map(|o| &mut o.data) {
        l.push(part);
    }
    true
}

fn intrinsic(rt: &mut Rt, key: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match key {
        // `SuperBufferFactory.createForBlock(model, state, poseStack)`: catnip's buffer, keeping the
        // model and the pose it was made under instead of its vertices; its other fields as its
        // constructor sets them, then its own `reset()` (white, no overlay, a fresh transform).
        CREATE_FOR_BLOCK => {
            let base = last_pose(rt, a(3));
            // A model the mod built of faces is read now, as catnip buffers its vertices now.
            let model = crate::quads::drawable(rt, a(1), a(2));
            let sbb = new_sbb(rt, model, base);
            if let Some(vc) = rt.vcall {
                vc(rt, SBB_RESET_KEY, SBB_RESET, &[sbb]);
            }
            sbb
        }
        // The builder's start: no blocks yet.
        SBB_BUILDER_BEGIN => {
            let parts = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(a(0), "$parts", parts);
            V::Void
        }
        // Its buffer: the blocks tesselated into it since `begin`, each with its model and pose.
        SBB_BUILDER_END => {
            let parts = rt.get_field(a(0), "$parts");
            rt.put_field(a(0), "$parts", V::NULL);
            let sbb = new_sbb(rt, V::NULL, IDENTITY);
            let parts = if parts.r() == 0 { crate::jutil::new_list(rt, Vec::new()) } else { parts };
            rt.put_field(sbb, "$parts", parts);
            if let Some(vc) = rt.vcall {
                vc(rt, SBB_RESET_KEY, SBB_RESET, &[sbb]);
            }
            sbb
        }
        // `renderInto(poseStack, consumer)`: the model once, under the stack's pose and the buffer's
        // transforms, coloured and lit as catnip would each vertex; then `reset()`, as catnip does.
        RENDER_INTO | DEFAULT_RENDER_INTO => {
            let this = a(0);
            let outer = last_pose(rt, a(1));
            let inner = last_pose(rt, rt.get_field(this, "transforms"));
            let base = mat4(rt, rt.get_field(this, "$base"));
            let m = mul4(&mul4(&outer, &inner), &base);
            let modern = key == RENDER_INTO;
            let color = if modern {
                // Float channels, 1 for untouched.
                let c = |rt: &Rt, f| (rt.get_field(this, f).f().clamp(0.0, 1.0) * 255.0).round() as u32;
                c(rt, "a") << 24 | c(rt, "r") << 16 | c(rt, "g") << 8 | c(rt, "b")
            } else if rt.get_field(this, "shouldColor").i() != 0 {
                let c = |rt: &Rt, f| rt.get_field(this, f).i().clamp(0, 255) as u32;
                c(rt, "a") << 24 | c(rt, "r") << 16 | c(rt, "g") << 8 | c(rt, "b")
            } else {
                0xFFFF_FFFF
            };
            let light_field = if modern { "packedLight" } else { "packedLightCoordinates" };
            let light = if rt.get_field(this, "hasCustomLight").i() != 0 { rt.get_field(this, light_field).i() } else { 0 };
            let model = rt.get_field(this, "$model");
            let shift = shift_of(rt, this);
            rt.put_field(this, "$shiftKind", V::I(0));
            if Rt::tracing() {
                // The first draws, each model under each pose once: what a renderer asked for.
                let name = match model_of(rt, model) {
                    Some(crate::host::Model::Named(n) | crate::host::Model::State(n)) => n,
                    Some(crate::host::Model::Quads(q)) => {
                        // The sprites they show, the most used first.
                        let mut used: Vec<(&str, usize)> = Vec::new();
                        for f in q.iter() {
                            match used.iter_mut().find(|(s, _)| *s == f.sprite) {
                                Some((_, n)) => *n += 1,
                                None => used.push((&f.sprite, 1)),
                            }
                        }
                        used.sort_by(|a, b| b.1.cmp(&a.1));
                        let top: Vec<String> = used.iter().take(4).map(|(s, n)| format!("{s} x{n}")).collect();
                        format!("{} faces ({})", q.len(), top.join(", "))
                    }
                    None => "-".to_owned(),
                };
                let r = |i: usize| (m[i] * 100.0).round() / 100.0;
                let cols = [[r(0), r(1), r(2)], [r(4), r(5), r(6)], [r(8), r(9), r(10)], [r(12), r(13), r(14)]];
                crate::glue::trace_some(rt, "draws", &format!("{name}/{cols:?}"), 60, |_| {
                    format!("client: draw {name}: x {:?} y {:?} z {:?} t {:?}", cols[0], cols[1], cols[2], cols[3])
                });
            }
            if let Some(model) = model_of(rt, model)
                && let Some(h) = rt.host_mut()
            {
                match &shift {
                    Some(shift) => h.draw_model_shifted(&model, &m, color, light as u32, shift),
                    None => h.draw_model(&model, &m, color, light as u32),
                };
            }
            // A buffer of blocks: each block's model under its own pose, inside the buffer's.
            let parts = rt.get_field(this, "$parts");
            if parts.r() != 0 {
                for part in crate::jutil::elements(rt, parts) {
                    let pose = mat4(rt, rt.get_field(part, "$pose"));
                    let Some(model) = model_of(rt, rt.get_field(part, "$model")) else { continue };
                    let pm = mul4(&m, &pose);
                    if let Some(h) = rt.host_mut() {
                        h.draw_model(&model, &pm, color, light as u32);
                    }
                }
            }
            if let Some(vc) = rt.vcall {
                let (k, short) = if modern { (SBB_RESET_KEY, SBB_RESET) } else { (DEFAULT_RESET_KEY, DEFAULT_RESET) };
                vc(rt, k, short, &[this]);
            }
            V::Void
        }
        SBB_IS_EMPTY | DEFAULT_IS_EMPTY => {
            let parts = rt.get_field(a(0), "$parts");
            let blocks = parts.r() != 0 && !crate::jutil::elements(rt, parts).is_empty();
            V::I((!blocks && model_of(rt, rt.get_field(a(0), "$model")).is_none()) as i32)
        }
        // `shiftUV(entry)`, `shiftUVScrolling(entry, [u,] v)`, `shiftUVtoSheet(entry, u, v, size)`:
        // the entry's two sprites and the offsets, as fractions of the target (a scroll comes in
        // atlas units, which a cell's width turns into fractions; a sheet's frame is one already).
        k if SHIFTS.contains(&k) => {
            let (this, entry) = (a(0), a(1));
            let Some(vc) = rt.vcall else { return Some(this) };
            let from = vc(rt, ORIGINAL_LOCATION, "getOriginalResourceLocation.()Lnet/minecraft/resources/ResourceLocation;", &[entry]);
            let to = vc(rt, TARGET_LOCATION, "getTargetResourceLocation.()Lnet/minecraft/resources/ResourceLocation;", &[entry]);
            let (kind, u, v, size) = if k.contains(".shiftUV.(") {
                (1, 0.0, 0.0, 1.0)
            } else if k.contains(".shiftUVtoSheet.(") {
                (3, a(2).f(), a(3).f(), a(4).i() as f32)
            } else if k.contains(";F)") {
                (2, 0.0, a(2).f() / CELL, 1.0)
            } else {
                (2, a(2).f() / CELL, a(3).f() / CELL, 1.0)
            };
            rt.put_field(this, "$shiftFrom", from);
            rt.put_field(this, "$shiftTo", to);
            rt.put_field(this, "$shiftKind", V::I(kind));
            rt.put_field(this, "$shiftU", V::F(u));
            rt.put_field(this, "$shiftV", V::F(v));
            rt.put_field(this, "$shiftSize", V::F(size));
            this
        }
        // `PartialModel.get()`: the model at its location, baked by the host on first draw.
        PARTIAL_GET => {
            let location = rt.get_field(a(0), "modelLocation");
            let m = rt.new_object("net/minecraft/client/resources/model/BakedModel");
            rt.put_field(m, "$name", location);
            m
        }
        SUPPORTS_VISUALIZATION => V::I(0),
        _ => return None,
    })
}

/// What a `BakedModel` object draws: a block state's model, a model by location, or faces read
/// from a model the mod built ([`crate::quads::handle`]).
fn model_of(rt: &Rt, model: V) -> Option<Model> {
    crate::quads::handle(rt, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    /// The translated catnip a buffer calls back: an entry's sprite locations, and `reset()`.
    fn catnip(rt: &mut Rt, key: &'static str, _: &'static str, args: &[V]) -> V {
        match key {
            ORIGINAL_LOCATION => rt.string("create:block/belt"),
            TARGET_LOCATION => rt.string("create:block/belt_scroll"),
            _ => args.first().copied().unwrap_or_default(),
        }
    }

    /// Create's contraption buffer: each block tesselated into catnip's builder (moved to its place
    /// in the structure) is drawn by its state's model under that place, inside the buffer's pose.
    #[test]
    fn a_buffer_of_tesselated_blocks_draws_each_block_where_it_was_put() {
        let mut rt = Rt::new();
        rt.vcall = Some(catnip);
        let mut host = crate::host::MemoryHost::default();
        let builder = rt.new_object("net/createmod/catnip/render/ShadedBlockSbbBuilder");
        intrinsic(&mut rt, SBB_BUILDER_BEGIN, &[builder]);
        let stack = new_pose_stack(&mut rt, IDENTITY);
        let tesselate = "net/minecraft/client/renderer/block/ModelBlockRenderer.tesselateBlock.(Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/client/resources/model/BakedModel;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;ZLnet/minecraft/util/RandomSource;JILnet/neoforged/neoforge/client/model/data/ModelData;Lnet/minecraft/client/renderer/RenderType;)V";
        let model_of_state = "net/minecraft/client/renderer/block/BlockRenderDispatcher.getBlockModel.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/client/resources/model/BakedModel;";
        for (state, y) in [("minecraft:oak_planks", 1.0), ("minecraft:stone", 2.0)] {
            let st = crate::api::state(&mut rt, state);
            let model = crate::api::call(&mut rt, model_of_state, &[V::NULL, st]);
            crate::api::call(&mut rt, "com/mojang/blaze3d/vertex/PoseStack.pushPose.()V", &[stack]);
            crate::api::call(&mut rt, "com/mojang/blaze3d/vertex/PoseStack.translate.(FFF)V", &[stack, V::F(0.0), V::F(y), V::F(0.0)]);
            crate::api::call(&mut rt, tesselate, &[V::NULL, V::NULL, model, st, V::NULL, stack, builder, V::I(1), V::NULL, V::J(0), V::I(NO_OVERLAY), V::NULL, V::NULL]);
            crate::api::call(&mut rt, "com/mojang/blaze3d/vertex/PoseStack.popPose.()V", &[stack]);
        }
        let sbb = intrinsic(&mut rt, SBB_BUILDER_END, &[builder]).unwrap();
        assert_eq!(intrinsic(&mut rt, SBB_IS_EMPTY, &[sbb]), Some(V::I(0)));
        // Drawn ten blocks east of the camera.
        let view = new_pose_stack(&mut rt, IDENTITY);
        crate::api::call(&mut rt, "com/mojang/blaze3d/vertex/PoseStack.translate.(FFF)V", &[view, V::F(10.0), V::F(0.0), V::F(0.0)]);
        rt.with_host(&mut host, |rt| {
            intrinsic(rt, RENDER_INTO, &[sbb, view, V::NULL]);
        });
        let drawn: Vec<(String, [f32; 3])> = host
            .drawn
            .iter()
            .map(|(m, pose, _, _)| {
                let name = match m {
                    Model::State(s) | Model::Named(s) => s.clone(),
                    Model::Quads(_) => "quads".into(),
                };
                (name, [pose[12], pose[13], pose[14]])
            })
            .collect();
        assert_eq!(drawn, vec![("minecraft:oak_planks".to_owned(), [10.0, 1.0, 0.0]), ("minecraft:stone".to_owned(), [10.0, 2.0, 0.0])]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_buffer_draws_once_with_the_texture_move_it_was_asked_for() {
        use crate::host::{UvShift, UvShiftKind};
        let mut rt = Rt::new();
        rt.vcall = Some(catnip);
        let mut host = crate::host::MemoryHost::default();
        let model = rt.new_object("net/minecraft/client/resources/model/BakedModel");
        let name = rt.string("create:block/belt/start");
        rt.put_field(model, "$name", name);
        let stack = new_pose_stack(&mut rt, IDENTITY);
        let sbb = intrinsic(&mut rt, CREATE_FOR_BLOCK, &[V::NULL, model, V::NULL, stack]).unwrap();
        let entry = rt.new_object("net/createmod/catnip/render/SpriteShiftEntry");
        // Create's belt: `shiftUVScrolling(entry, scroll)` a quarter of the way down the target
        // (`scroll * spriteSize`, in the atlas), which returns the buffer.
        assert_eq!(intrinsic(&mut rt, SHIFTS[1], &[sbb, entry, V::F(0.25 * CELL)]), Some(sbb));
        rt.with_host(&mut host, |rt| {
            intrinsic(rt, RENDER_INTO, &[sbb, stack, V::NULL]);
            // Drawn again with no new move: catnip's `reset()` after a draw drops it.
            intrinsic(rt, RENDER_INTO, &[sbb, stack, V::NULL]);
        });
        assert_eq!(host.drawn.len(), 2);
        let scroll = UvShift { from: "create:block/belt".into(), to: "create:block/belt_scroll".into(), kind: UvShiftKind::Scroll { u: 0.0, v: 0.25 } };
        assert_eq!(host.shifts, vec![(0, scroll)]);
        // A sheet's frame and a plain move keep what they were given.
        intrinsic(&mut rt, SHIFTS[3], &[sbb, entry, V::F(0.5), V::F(0.25), V::I(4)]);
        assert_eq!(shift_of(&rt, sbb).map(|s| s.kind), Some(UvShiftKind::Sheet { u: 0.5, v: 0.25, size: 4.0 }));
        intrinsic(&mut rt, SHIFTS[0], &[sbb, entry]);
        assert_eq!(shift_of(&rt, sbb).map(|s| s.kind), Some(UvShiftKind::Retarget));
    }

    #[test]
    fn the_stitched_block_atlas_gives_each_sprite_a_part_of_its_own() {
        const GET_SPRITE: &str = "net/minecraft/client/renderer/texture/TextureAtlas.getSprite.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
        let mut rt = Rt::new();
        let atlas = block_atlas(&mut rt);
        let location = call(&mut rt, "net/minecraft/client/renderer/texture/TextureAtlas.location.()Lnet/minecraft/resources/ResourceLocation;", &[atlas]).unwrap();
        assert_eq!(rt.str_of(location), Some("minecraft:textures/atlas/blocks.png"));
        let name = rt.string("create:block/belt_scroll");
        let sprite = call(&mut rt, GET_SPRITE, &[atlas, name]).unwrap();
        let again = rt.string("create:block/belt_scroll");
        assert_eq!(call(&mut rt, GET_SPRITE, &[atlas, again]), Some(sprite));
        let f = |rt: &mut Rt, m: &'static str, args: &[V]| call(rt, m, args).unwrap().f();
        let bounds = |rt: &mut Rt, s: V| {
            [
                f(rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU0.()F", &[s]),
                f(rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV0.()F", &[s]),
                f(rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU1.()F", &[s]),
                f(rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV1.()F", &[s]),
            ]
        };
        let [u0, v0, u1, v1] = bounds(&mut rt, sprite);
        assert!(u0 >= 0.0 && v0 >= 0.0 && u1 <= 1.0 && v1 <= 1.0 && u1 > u0 && v1 > v0);
        // Halfway across, and back.
        let half = f(&mut rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU.(F)F", &[sprite, V::F(0.5)]);
        assert_eq!(half, (u0 + u1) / 2.0);
        assert_eq!(f(&mut rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.getUOffset.(F)F", &[sprite, V::F(half)]), 0.5);
        // Another sprite's part overlaps none of it, and a corner placed in it reads back as that
        // sprite's, wherever the face was baked.
        let other = sprite_named(&mut rt, "minecraft:block/stone");
        let [a0, b0, a1, b1] = bounds(&mut rt, other);
        assert!(a1 <= u0 || a0 >= u1 || b1 <= v0 || b0 >= v1);
        let corners = [[a0, b0], [a0, b1], [a1, b1], [a1, b0]];
        assert_eq!(locate(&rt, sprite, &corners), (other, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]));
        let contents = call(&mut rt, "net/minecraft/client/renderer/texture/TextureAtlasSprite.contents.()Lnet/minecraft/client/renderer/texture/SpriteContents;", &[sprite]).unwrap();
        let named = call(&mut rt, "net/minecraft/client/renderer/texture/SpriteContents.name.()Lnet/minecraft/resources/ResourceLocation;", &[contents]).unwrap();
        assert_eq!(rt.str_of(named), Some("create:block/belt_scroll"));
    }

    #[test]
    fn model_data_holds_values_by_property() {
        let mut rt = Rt::new();
        let empty = get_static(&mut rt, MODEL_DATA, "EMPTY").unwrap();
        let prop = rt.new_object("net/neoforged/neoforge/client/model/data/ModelProperty");
        let other = rt.new_object("net/neoforged/neoforge/client/model/data/ModelProperty");
        let has = "net/neoforged/neoforge/client/model/data/ModelData.has.(Lnet/neoforged/neoforge/client/model/data/ModelProperty;)Z";
        let get = "net/neoforged/neoforge/client/model/data/ModelData.get.(Lnet/neoforged/neoforge/client/model/data/ModelProperty;)Ljava/lang/Object;";
        assert_eq!(call(&mut rt, has, &[empty, prop]), Some(V::I(0)));
        let b = call(&mut rt, "net/neoforged/neoforge/client/model/data/ModelData.builder.()Lnet/neoforged/neoforge/client/model/data/ModelData$Builder;", &[]).unwrap();
        let bracket = rt.string("bracket");
        let with = "net/neoforged/neoforge/client/model/data/ModelData$Builder.with.(Lnet/neoforged/neoforge/client/model/data/ModelProperty;Ljava/lang/Object;)Lnet/neoforged/neoforge/client/model/data/ModelData$Builder;";
        assert_eq!(call(&mut rt, with, &[b, prop, bracket]), Some(b));
        let data = call(&mut rt, "net/neoforged/neoforge/client/model/data/ModelData$Builder.build.()Lnet/neoforged/neoforge/client/model/data/ModelData;", &[b]).unwrap();
        assert_eq!(call(&mut rt, has, &[data, prop]), Some(V::I(1)));
        assert_eq!(call(&mut rt, has, &[data, other]), Some(V::I(0)));
        assert_eq!(call(&mut rt, get, &[data, prop]), Some(bracket));
        assert_eq!(call(&mut rt, get, &[empty, prop]), Some(V::NULL));
        // `derive` starts from the values it was made from; the original keeps its own.
        let d = call(&mut rt, "net/neoforged/neoforge/client/model/data/ModelData.derive.()Lnet/neoforged/neoforge/client/model/data/ModelData$Builder;", &[data]).unwrap();
        call(&mut rt, with, &[d, other, bracket]);
        let derived = call(&mut rt, "net/neoforged/neoforge/client/model/data/ModelData$Builder.build.()Lnet/neoforged/neoforge/client/model/data/ModelData;", &[d]).unwrap();
        assert_eq!(call(&mut rt, has, &[derived, prop]), Some(V::I(1)));
        assert_eq!(call(&mut rt, has, &[data, other]), Some(V::I(0)));
    }

    #[test]
    fn model_locations_name_the_state_as_block_model_shaper_does() {
        assert_eq!(model_location("create:shaft", "create:shaft[axis=x]"), "create:shaft#axis=x");
        assert_eq!(model_location("create:gearbox", "create:gearbox"), "create:gearbox#");
    }

    #[test]
    fn rotations_turn_counterclockwise_seen_from_the_positive_axis() {
        // A quarter turn about +Y takes +X to -Z (JOML, Minecraft's `Axis.YP`).
        assert!(close(&quat_rotate(quat_y(std::f32::consts::FRAC_PI_2), [1.0, 0.0, 0.0]), &[0.0, 0.0, -1.0]));
        // About +Z, +X goes to +Y; about +X, +Y goes to +Z.
        assert!(close(&quat_rotate(quat_z(std::f32::consts::FRAC_PI_2), [1.0, 0.0, 0.0]), &[0.0, 1.0, 0.0]));
        assert!(close(&quat_rotate(quat_x(std::f32::consts::FRAC_PI_2), [0.0, 1.0, 0.0]), &[0.0, 0.0, 1.0]));
    }

    #[test]
    fn pose_stack_composes_as_vanilla() {
        let mut rt = Rt::new();
        let ps = new_pose_stack(&mut rt, IDENTITY);
        pose_stack(&mut rt, "translate", "(FFF)V", &[ps, V::F(0.5), V::F(0.5), V::F(0.5)]);
        pose_stack(&mut rt, "pushPose", "()V", &[ps]);
        let q = new_quat(&mut rt, quat_z(std::f32::consts::FRAC_PI_2));
        pose_stack(&mut rt, "mulPose", "(Lorg/joml/Quaternionf;)V", &[ps, q]);
        pose_stack(&mut rt, "translate", "(FFF)V", &[ps, V::F(-0.5), V::F(-0.5), V::F(-0.5)]);
        // The block's corner (0, 0, 0) turned a quarter about the block's centre's z axis.
        let [x, y, z, _] = transform4(&last_pose(&rt, ps), [0.0, 0.0, 0.0, 1.0]);
        assert!(close(&[x, y, z], &[1.0, 0.0, 0.0]));
        pose_stack(&mut rt, "popPose", "()V", &[ps]);
        let [x, y, z, _] = transform4(&last_pose(&rt, ps), [0.0, 0.0, 0.0, 1.0]);
        assert!(close(&[x, y, z], &[0.5, 0.5, 0.5]));
        assert_eq!(pose_stack(&mut rt, "clear", "()Z", &[ps]), Some(V::I(1)));
    }

    #[test]
    fn inverse_undoes() {
        let m = mul4(&mul4(&translation(1.0, 2.0, 3.0), &quat_m4(quat_axis(0.7, 1.0, 2.0, 0.5))), &scaling(2.0, 2.0, 2.0));
        assert!(close(&mul4(&m, &invert4(&m)), &IDENTITY));
    }
}
