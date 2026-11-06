//! Shapes for translated mods: `VoxelShape` and `Shapes` (`Block.box`, joins), `AABB`, a
//! structure's `BoundingBox` and `CollisionContext`. A shape is the union of boxes, each `[min x, y, z, max x, y, z]`. Joins
//! work as vanilla's do: cell by cell on the grid of both shapes' coordinates, then merged into
//! boxes in the order vanilla's `forAllBoxes` merges them (strips along z, grown along x, then y).
use crate::{api, Data, Rt, V};

pub const VOXEL_SHAPE: &str = "net/minecraft/world/phys/shapes/VoxelShape";
const SHAPES: &str = "net/minecraft/world/phys/shapes/Shapes";
pub const AABB: &str = "net/minecraft/world/phys/AABB";
pub const BOUNDING_BOX: &str = "net/minecraft/world/level/levelgen/structure/BoundingBox";
const BOOLEAN_OP: &str = "net/minecraft/world/phys/shapes/BooleanOp";
const COLLISION_CONTEXT: &str = "net/minecraft/world/phys/shapes/CollisionContext";
pub const ENTITY_COLLISION_CONTEXT: &str = "net/minecraft/world/phys/shapes/EntityCollisionContext";
const VEC3: &str = "net/minecraft/world/phys/Vec3";
const DISCRETE: &str = "net/minecraft/world/phys/shapes/DiscreteVoxelShape";
const SLICE: &str = "net/minecraft/world/phys/shapes/SliceShape";
const CONSUME: &str = "net/minecraft/world/phys/shapes/Shapes$DoubleLineConsumer.consume.(DDDDDD)V";
const APPLY_OP: &str = "net/minecraft/world/phys/shapes/BooleanOp.apply.(ZZ)Z";

/// `[min x, y, z, max x, y, z]`.
pub type Box3 = [f64; 6];

const FULL: Box3 = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
/// Vanilla's `Shapes.EPSILON`: thinner than this is empty.
const EPSILON: f64 = 1.0e-7;

/// A shape object holding `boxes`.
pub fn shape(rt: &mut Rt, boxes: &[Box3]) -> V {
    let vals = boxes.iter().flat_map(|b| b.iter().map(|&d| V::D(d))).collect();
    rt.alloc(VOXEL_SHAPE, Data::Array(vals))
}

/// A shape object's boxes; None for anything else (null, or a shape the runtime didn't make).
pub fn boxes(rt: &Rt, v: V) -> Option<Vec<Box3>> {
    let o = rt.heap.get(v.r() as usize).filter(|_| v.r() != 0)?;
    match (&o.data, o.class) {
        (Data::Array(vals), VOXEL_SHAPE) => Some(
            vals.chunks_exact(6)
                .map(|c| [c[0].d(), c[1].d(), c[2].d(), c[3].d(), c[4].d(), c[5].d()])
                .collect(),
        ),
        _ => None,
    }
}

/// `Shapes.create`: one box, empty when thinner than vanilla's epsilon on any axis.
fn create(b: Box3) -> Vec<Box3> {
    if (0..3).any(|i| b[i + 3] - b[i] < EPSILON) { Vec::new() } else { vec![b] }
}

/// The sorted distinct coordinates of `boxes` on each axis; coordinates closer than vanilla's
/// epsilon are one, as its `IndirectMerger` takes them (Create's rotated shapes differ by float
/// noise).
fn grid(boxes: &[&[Box3]]) -> [Vec<f64>; 3] {
    let mut axes: [Vec<f64>; 3] = Default::default();
    for b in boxes.iter().flat_map(|s| s.iter()) {
        for (i, axis) in axes.iter_mut().enumerate() {
            axis.push(b[i]);
            axis.push(b[i + 3]);
        }
    }
    for axis in &mut axes {
        axis.sort_by(f64::total_cmp);
        axis.dedup_by(|b, a| *b - *a < EPSILON);
    }
    axes
}

/// Vanilla's `Shapes.findBits`: the fewest bits (0 to 3) that put both ends on the grid of
/// `2^bits` cells, -1 when none does.
fn find_bits(min: f64, max: f64) -> i32 {
    if min < -EPSILON || max > 1.0 + EPSILON {
        return -1;
    }
    (0..=3)
        .find(|&i| {
            let j = f64::from(1 << i);
            let on = |d: f64| (d * j - (d * j).round()).abs() < EPSILON * j;
            on(min) && on(max)
        })
        .unwrap_or(-1)
}

/// The grid lines of a shape's cells on each axis, as vanilla's `DiscreteVoxelShape` has them
/// (`getSize` counts its cells): a shape whose boxes all sit on power-of-two grids is a cube shape
/// (`Shapes.box` gives one, joins keep it), `2^bits` even cells on an axis, the finest grid a box
/// needs there; any other has a cell between each two of its coordinates; the empty shape none.
fn cell_lines(s: &[Box3]) -> [Vec<f64>; 3] {
    if s.is_empty() {
        return [vec![0.0], vec![0.0], vec![0.0]];
    }
    if !s.iter().all(|b| (0..3).all(|i| find_bits(b[i], b[i + 3]) >= 0)) {
        return grid(&[s]);
    }
    std::array::from_fn(|i| {
        let n = 1 << s.iter().map(|b| find_bits(b[i], b[i + 3])).max().unwrap_or(0);
        (0..=n).map(|k| f64::from(k) / f64::from(n)).collect()
    })
}

/// `new SliceShape(shape, axis, index)`: the shape's section through cell `index` on `axis`,
/// through the whole block along it (vanilla's face shapes, `Shapes.getFaceShape`).
pub fn slice(s: &[Box3], axis: usize, index: i32) -> Vec<Box3> {
    let lines = &cell_lines(s)[axis];
    let Some((lo, hi)) = usize::try_from(index).ok().and_then(|i| Some((*lines.get(i)?, *lines.get(i + 1)?))) else {
        return Vec::new();
    };
    s.iter()
        .filter(|b| b[axis] <= lo + EPSILON && b[axis + 3] >= hi - EPSILON)
        .map(|b| {
            let mut c = *b;
            c[axis] = 0.0;
            c[axis + 3] = 1.0;
            c
        })
        .collect()
}

/// A shape's `DiscreteVoxelShape` (its cells, a mixin accessor reads `VoxelShape.shape`): a view of
/// the shape's boxes on its [`cell_lines`].
fn discrete(rt: &mut Rt, shape: V) -> V {
    let d = rt.new_object(DISCRETE);
    rt.put_field(d, "$shape", shape);
    d
}

/// Marks the cells of `axes` that `boxes` cover.
fn fill(axes: &[Vec<f64>; 3], boxes: &[Box3]) -> Vec<bool> {
    let n = [axes[0].len() - 1, axes[1].len() - 1, axes[2].len() - 1];
    let mut cells = vec![false; n[0] * n[1] * n[2]];
    // The grid line a coordinate was merged into: the last one not past it by more than epsilon.
    let at = |i: usize, x: f64| axes[i].partition_point(|c| *c <= x + EPSILON).saturating_sub(1);
    for b in boxes {
        let (x0, x1, y0, y1, z0, z1) = (at(0, b[0]), at(0, b[3]), at(1, b[1]), at(1, b[4]), at(2, b[2]), at(2, b[5]));
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    cells[(x * n[1] + y) * n[2] + z] = true;
                }
            }
        }
    }
    cells
}

/// The cells as boxes, merged as vanilla's `BitSetDiscreteVoxelShape.forAllBoxes` merges.
fn merge(axes: &[Vec<f64>; 3], mut cells: Vec<bool>) -> Vec<Box3> {
    let n = [axes[0].len() - 1, axes[1].len() - 1, axes[2].len() - 1];
    let idx = |x: usize, y: usize, z: usize| (x * n[1] + y) * n[2] + z;
    let strip_full = |cells: &[bool], z0: usize, z1: usize, x: usize, y: usize| {
        x < n[0] && y < n[1] && (z0..z1).all(|z| cells[idx(x, y, z)])
    };
    let mut out = Vec::new();
    for y in 0..n[1] {
        for x in 0..n[0] {
            let mut start: Option<usize> = None;
            for z in 0..=n[2] {
                if z < n[2] && cells[idx(x, y, z)] {
                    start.get_or_insert(z);
                    continue;
                }
                let Some(z0) = start.take() else { continue };
                let clear = |cells: &mut [bool], x: usize, y: usize| (z0..z).for_each(|zz| cells[idx(x, y, zz)] = false);
                clear(&mut cells, x, y);
                let mut x1 = x;
                while strip_full(&cells, z0, z, x1 + 1, y) {
                    clear(&mut cells, x1 + 1, y);
                    x1 += 1;
                }
                let mut y1 = y;
                while (x..=x1).all(|xx| strip_full(&cells, z0, z, xx, y1 + 1)) {
                    for xx in x..=x1 {
                        clear(&mut cells, xx, y1 + 1);
                    }
                    y1 += 1;
                }
                out.push([axes[0][x], axes[1][y], axes[2][z0], axes[0][x1 + 1], axes[1][y1 + 1], axes[2][z]]);
            }
        }
    }
    out
}

/// `Shapes.join(a, b, op)`, `op` as its truth table (bit `a << 1 | b`).
pub fn join(a: &[Box3], b: &[Box3], op: u8) -> Vec<Box3> {
    let axes = grid(&[a, b]);
    if axes.iter().any(|axis| axis.len() < 2) {
        return Vec::new();
    }
    let (ca, cb) = (fill(&axes, a), fill(&axes, b));
    let cells = ca.iter().zip(&cb).map(|(&x, &y)| op >> ((x as u8) << 1 | y as u8) & 1 != 0).collect();
    merge(&axes, cells)
}

/// The boxes `forAllBoxes` and `toAabbs` give: merged on the shape's own grid.
pub fn optimized(boxes: &[Box3]) -> Vec<Box3> {
    join(boxes, &[], OR)
}

/// `BooleanOp`s by truth table: bit `a << 1 | b` is `op(a, b)`.
const OR: u8 = 0b1110;
const NOT_SAME: u8 = 0b0110;
pub(crate) const ONLY_FIRST: u8 = 0b0100;
const ONLY_SECOND: u8 = 0b0010;

/// `Block.isShapeFullBlock`: the shape is the whole block, nothing missing and nothing past it.
pub fn full_block(boxes: &[Box3]) -> bool {
    join(&[FULL], boxes, NOT_SAME).is_empty()
}

/// `VoxelShape.getFaceShape(direction)` (`direction` by ordinal): the shape's slice just inside
/// that face (the cells at 1e-7, or 0.9999999 on a positive face, along its axis), made as deep as
/// the block.
pub fn face_slice(boxes: &[Box3], direction: usize) -> Vec<Box3> {
    let axis = [1, 1, 2, 2, 0, 0][direction.min(5)];
    let at = if direction % 2 == 1 { 0.9999999 } else { 1.0e-7 };
    boxes
        .iter()
        .filter(|b| b[axis] <= at && at < b[axis + 3])
        .map(|b| {
            let mut s = *b;
            (s[axis], s[axis + 3]) = (0.0, 1.0);
            s
        })
        .collect()
}

/// `Block.isFaceFull(shape, direction)`: the shape's [`face_slice`] there is a full block, so it
/// covers the whole face and nothing past it.
pub fn face_full(boxes: &[Box3], direction: usize) -> bool {
    let slice = face_slice(boxes, direction);
    !slice.is_empty() && full_block(&slice)
}

/// `SupportType.isSupporting` of a support shape's face (`direction` by ordinal): FULL (0) the
/// whole face (`Block.isFaceFull`); CENTER (1) its middle, nothing of `Block.box(7, 0, 7, 9, 10,
/// 9)` outside the face's [`face_slice`]; RIGID (2) its rim, nothing of the block less
/// `Block.box(2, 0, 2, 14, 16, 14)` outside it.
pub fn sturdy(boxes: &[Box3], direction: usize, support: usize) -> bool {
    const PX: f64 = 1.0 / 16.0;
    let test = match support {
        0 => return face_full(boxes, direction),
        1 => vec![[7.0 * PX, 0.0, 7.0 * PX, 9.0 * PX, 10.0 * PX, 9.0 * PX]],
        _ => join(&[FULL], &[[2.0 * PX, 0.0, 2.0 * PX, 14.0 * PX, 1.0, 14.0 * PX]], ONLY_FIRST),
    };
    join(&face_slice(boxes, direction), &test, ONLY_SECOND).is_empty()
}

const OPS: [(&str, u8); 16] = [
    ("FALSE", 0b0000),
    ("NOT_OR", 0b0001),
    ("ONLY_SECOND", 0b0010),
    ("NOT_FIRST", 0b0011),
    ("ONLY_FIRST", 0b0100),
    ("NOT_SECOND", 0b0101),
    ("NOT_SAME", 0b0110),
    ("NOT_AND", 0b0111),
    ("AND", 0b1000),
    ("SAME", 0b1001),
    ("SECOND", 0b1010),
    ("CAUSES", 0b1011),
    ("FIRST", 0b1100),
    ("CAUSED_BY", 0b1101),
    ("OR", OR),
    ("TRUE", 0b1111),
];

/// A `BooleanOp`'s truth table: one of vanilla's, or a mod's own lambda asked all four cases.
fn op_table(rt: &mut Rt, op: V) -> u8 {
    if let Some(o) = rt.heap.get(op.r() as usize).filter(|_| op.r() != 0)
        && let Data::Native("BooleanOp", h) = &o.data
    {
        return h[0] as u8;
    }
    let mut t = 0;
    for bit in 0..4u8 {
        let r = rt.call_functional(APPLY_OP, op, &[V::I((bit >> 1) as i32), V::I((bit & 1) as i32)]);
        if r.i() != 0 {
            t |= 1 << bit;
        }
    }
    t
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        BOOLEAN_OP => {
            let &(name, t) = OPS.iter().find(|(n, _)| *n == field)?;
            Some(rt.intern(format!("boolean-op/{name}"), |rt| rt.alloc(BOOLEAN_OP, Data::Native("BooleanOp", [t as i64, 0, 0, 0]))))
        }
        AABB if field == "INFINITE" => Some(aabb(rt, [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::INFINITY, f64::INFINITY])),
        SHAPES if field == "INFINITY" => {
            let inf = [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::INFINITY, f64::INFINITY];
            Some(shape(rt, &[inf]))
        }
        _ => None,
    }
}

/// An `AABB` (its fields are public in vanilla, so they are fields here).
pub fn aabb(rt: &mut Rt, b: Box3) -> V {
    let o = rt.new_object(AABB);
    set_aabb(rt, o, b);
    o
}

fn set_aabb(rt: &mut Rt, o: V, b: Box3) {
    for (k, v) in ["minX", "minY", "minZ", "maxX", "maxY", "maxZ"].into_iter().zip(b) {
        rt.put_field(o, k, V::D(v));
    }
}

pub fn aabb_of(rt: &Rt, o: V) -> Box3 {
    ["minX", "minY", "minZ", "maxX", "maxY", "maxZ"].map(|k| rt.get_field(o, k).d())
}

/// `new AABB(x1, y1, z1, x2, y2, z2)`: corners in any order.
fn sorted(b: Box3) -> Box3 {
    [b[0].min(b[3]), b[1].min(b[4]), b[2].min(b[5]), b[0].max(b[3]), b[1].max(b[4]), b[2].max(b[5])]
}

fn xyz(rt: &Rt, v: V) -> [f64; 3] {
    [rt.get_field(v, "x").d(), rt.get_field(v, "y").d(), rt.get_field(v, "z").d()]
}

fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
    let v = rt.new_object(VEC3);
    for (k, x) in ["x", "y", "z"].into_iter().zip(p) {
        rt.put_field(v, k, V::D(x));
    }
    v
}

fn pos(rt: &Rt, v: V) -> [f64; 3] {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => [h[0] as f64, h[1] as f64, h[2] as f64],
        _ => [0.0; 3],
    }
}

/// Axis ordinal (X 0, Y 1, Z 2) of a `Direction$Axis`.
fn axis_of(rt: &Rt, v: V) -> usize {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => h[0].clamp(0, 2) as usize,
        _ => 0,
    }
}

/// `AABB.clip`: where the segment `from` → `to` enters the box, as vanilla's `getDirection`
/// finds it (the nearest face crossed going in).
pub(crate) fn clip(b: Box3, from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let mut scale = 1.0;
    enter(b, from, d, &mut scale)?;
    Some([from[0] + scale * d[0], from[1] + scale * d[1], from[2] + scale * d[2]])
}

/// Vanilla's `AABB.getDirection` (with its `clipPoint`): whether the segment from `from` along `d`
/// enters `b` sooner than `*scale` (a fraction of `d`), which then becomes that fraction, and the
/// face it enters by, as a 3D data value (0 down, 1 up, 2 north, 3 south, 4 west, 5 east).
fn enter(b: Box3, from: [f64; 3], d: [f64; 3], scale: &mut f64) -> Option<u8> {
    let mut face = None;
    // Each axis in vanilla's order (x, y, z), with the other two in its cycle, and the faces a
    // segment going up the axis and down it enters by.
    for (a, b1, c1, faces) in [(0, 1, 2, [4, 5]), (1, 2, 0, [0, 1]), (2, 0, 1, [2, 3])] {
        let (point, f) = if d[a] > EPSILON {
            (b[a], faces[0])
        } else if d[a] < -EPSILON {
            (b[a + 3], faces[1])
        } else {
            continue;
        };
        let s = (point - from[a]) / d[a];
        let pb = from[b1] + s * d[b1];
        let pc = from[c1] + s * d[c1];
        if 0.0 < s && s < *scale && b[b1] - EPSILON < pb && pb < b[b1 + 3] + EPSILON && b[c1] - EPSILON < pc && pc < b[c1 + 3] + EPSILON {
            *scale = s;
            face = Some(f);
        }
    }
    face
}

/// Vanilla's `AABB.clip(boxes, from, to, pos)`: where the segment first enters one of `boxes` moved
/// to `pos`, and the face it enters by; None for none.
fn clip_boxes(boxes: &[Box3], from: [f64; 3], to: [f64; 3], pos: [i32; 3]) -> Option<([f64; 3], u8)> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let o = pos.map(f64::from);
    let (mut scale, mut face) = (1.0, None);
    for b in boxes {
        let moved = [b[0] + o[0], b[1] + o[1], b[2] + o[2], b[3] + o[0], b[4] + o[1], b[5] + o[2]];
        if let Some(f) = enter(moved, from, d, &mut scale) {
            face = Some(f);
        }
    }
    Some(([from[0] + scale * d[0], from[1] + scale * d[1], from[2] + scale * d[2]], face?))
}

/// Vanilla's `VoxelShape.clip(from, to, pos)` for the shape of `boxes` at `pos`: a segment that
/// starts inside it (a thousandth of the way in) hits there, inside, on the face looking back along
/// it; else where it first enters a box. None for an empty shape, a segment shorter than vanilla's
/// least, or a miss.
pub(crate) fn clip_shape(boxes: &[Box3], from: [f64; 3], to: [f64; 3], pos: [i32; 3]) -> Option<([f64; 3], u8, bool)> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    if boxes.is_empty() || d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 1.0e-7 {
        return None;
    }
    let inner = [from[0] + d[0] * 0.001, from[1] + d[1] * 0.001, from[2] + d[2] * 0.001];
    let local = [inner[0] - f64::from(pos[0]), inner[1] - f64::from(pos[1]), inner[2] - f64::from(pos[2])];
    if boxes.iter().any(|b| (0..3).all(|i| b[i] <= local[i] && local[i] < b[i + 3])) {
        return Some((inner, nearest(d) ^ 1, true));
    }
    clip_boxes(boxes, from, to, pos).map(|(at, face)| (at, face, false))
}

/// `Direction.getApproximateNearest(x, y, z)`: the direction nearest the vector, as a 3D data value
/// (the first of the largest dot products, in floats; north for none).
pub(crate) fn nearest(v: [f64; 3]) -> u8 {
    const NORMALS: [[f32; 3]; 6] = [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let v = v.map(|x| x as f32);
    let (mut face, mut best) = (2, f32::from_bits(1));
    for (i, n) in NORMALS.iter().enumerate() {
        let dot = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
        if dot > best {
            (face, best) = (i as u8, dot);
        }
    }
    face
}

/// A `BlockHitResult` hitting block `pos` at `at` on face `face` (`inside`: from within the shape).
fn block_hit(rt: &mut Rt, at: [f64; 3], face: u8, pos: [i32; 3], inside: bool) -> V {
    let hit = rt.new_object("net/minecraft/world/phys/BlockHitResult");
    let (loc, dir) = (vec3(rt, at), crate::api_common::direction(rt, i64::from(face)));
    let p = api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
    rt.put_field(hit, "$loc", loc);
    rt.put_field(hit, "$dir", dir);
    rt.put_field(hit, "$pos", p);
    rt.put_field(hit, "$inside", V::I(inside as i32));
    hit
}

/// What a ray that clips fluids asks of a block's fluid ([`crate::fluids`]): whether there is one,
/// whether it is a source, how high it stands.
const FLUID_IS_EMPTY: &str = "net/minecraft/world/level/material/FluidState.isEmpty.()Z";
const FLUID_IS_SOURCE: &str = "net/minecraft/world/level/material/FluidState.isSource.()Z";
const FLUID_HEIGHT: &str = "net/minecraft/world/level/material/FluidState.getHeight.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
/// `BlockGetter.clip`'s descriptor, and `ClipContext`'s constructors: with an entity (whose
/// collision context it takes), and with a collision context (the table's, [`crate::records`]).
const LEVEL_CLIP: &str = "(Lnet/minecraft/world/level/ClipContext;)Lnet/minecraft/world/phys/BlockHitResult;";
const CLIP_CONTEXT_OF_ENTITY: &str = "(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/level/ClipContext$Block;Lnet/minecraft/world/level/ClipContext$Fluid;Lnet/minecraft/world/entity/Entity;)V";
const CLIP_CONTEXT_OF_CONTEXT: &str = "(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/level/ClipContext$Block;Lnet/minecraft/world/level/ClipContext$Fluid;Lnet/minecraft/world/phys/shapes/CollisionContext;)V";

/// `BlockGetter.clip(context)` on the level, for the context's `from`, `to` and modes (the
/// ordinals of `ClipContext.Block`: collider, outline, visual, fall damage resetting; of
/// `ClipContext.Fluid`: none, sources, any, water): vanilla's `traverseBlocks` walks the blocks the
/// segment crosses (from a hair before `from` to a hair past `to`), and in each the block's shape
/// the mode picks (its collision shape, its outline, or a full block for one that resets fall
/// damage) and, when the fluid mode takes it, the fluid's (as high as it stands) are clipped; the
/// nearer of the two hits. A miss at `to` when nothing is, facing back along the segment. Not
/// modelled: a block's interaction shape (a cauldron's inside), which vanilla prefers for the face
/// hit, a visual shape apart from the collision shape (glass's), and the context's entity, which
/// some collision shapes read (scaffolding).
fn level_clip(rt: &mut Rt, level: V, ctx: V) -> V {
    let (from, to) = (rt.get_field(ctx, "from"), rt.get_field(ctx, "to"));
    let (from, to) = (xyz(rt, from), xyz(rt, to));
    let ordinal = |rt: &Rt, v: V| rt.get_field(v, "$ordinal").i();
    let (block_mode, fluid_mode) = (ordinal(rt, rt.get_field(ctx, "block")), ordinal(rt, rt.get_field(ctx, "fluid")));
    clip_through(rt, level, from, to, block_mode, fluid_mode)
}

/// [`level_clip`] for a context's parts: the segment and the ordinals of its block and fluid modes.
pub(crate) fn clip_through(rt: &mut Rt, level: V, from: [f64; 3], to: [f64; 3], block_mode: i32, fluid_mode: i32) -> V {
    let test = |rt: &mut Rt, p: [i32; 3]| -> Option<V> {
        let host = rt.host()?;
        let text = host.block_state(p);
        let boxes = match block_mode {
            1 => host.outline_boxes(p).or_else(|| host.collision_boxes(p)),
            3 => {
                let (block, _) = crate::host::split_state(&text);
                Some(if host.block_in_tag(block, "minecraft:fall_damage_resetting") { vec![FULL] } else { Vec::new() })
            }
            _ => host.collision_boxes(p),
        }
        .unwrap_or_default();
        let block = clip_shape(&boxes, from, to, p);
        let fluid = if fluid_mode == 0 {
            None
        } else {
            let state = crate::fluids::of_block_state(rt, &text);
            let empty = api::call(rt, FLUID_IS_EMPTY, &[state]).i() != 0;
            let picks = !empty
                && match fluid_mode {
                    1 => api::call(rt, FLUID_IS_SOURCE, &[state]).i() != 0,
                    3 => {
                        let name = crate::fluids::name_of(rt, rt.get_field(state, "$fluid"));
                        rt.host().is_some_and(|h| h.fluid_in_tag(&name, "minecraft:water"))
                    }
                    _ => true,
                };
            if picks {
                let at = api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]));
                let height = api::call(rt, FLUID_HEIGHT, &[state, level, at]).f();
                clip_shape(&[[0.0, 0.0, 0.0, 1.0, f64::from(height), 1.0]], from, to, p)
            } else {
                None
            }
        };
        let distance = |h: &Option<([f64; 3], u8, bool)>| h.map_or(f64::MAX, |(at, _, _)| (0..3).map(|i| (at[i] - from[i]).powi(2)).sum());
        let (at, face, inside) = if distance(&block) <= distance(&fluid) { block } else { fluid }?;
        Some(block_hit(rt, at, face, p, inside))
    };
    if from != to {
        let lerp = |delta: f64, start: f64, end: f64| start + delta * (end - start);
        let end: [f64; 3] = std::array::from_fn(|i| lerp(-1.0e-7, to[i], from[i]));
        let start: [f64; 3] = std::array::from_fn(|i| lerp(-1.0e-7, from[i], to[i]));
        let mut p = start.map(|v| v.floor() as i32);
        if let Some(hit) = test(rt, p) {
            return hit;
        }
        let delta: [f64; 3] = std::array::from_fn(|i| end[i] - start[i]);
        let sign = delta.map(|v| if v == 0.0 { 0 } else if v > 0.0 { 1 } else { -1 });
        let step: [f64; 3] = std::array::from_fn(|i| if sign[i] == 0 { f64::MAX } else { f64::from(sign[i]) / delta[i] });
        let frac = |v: f64| v - v.floor();
        let mut t: [f64; 3] = std::array::from_fn(|i| step[i] * if sign[i] > 0 { 1.0 - frac(start[i]) } else { frac(start[i]) });
        while t[0] <= 1.0 || t[1] <= 1.0 || t[2] <= 1.0 {
            let axis = if t[0] < t[1] {
                if t[0] < t[2] { 0 } else { 2 }
            } else if t[1] < t[2] {
                1
            } else {
                2
            };
            p[axis] += sign[axis];
            t[axis] += step[axis];
            if let Some(hit) = test(rt, p) {
                return hit;
            }
        }
    }
    // `BlockHitResult.miss(to, the direction nearest from - to, the block holding to)`.
    let back = nearest([from[0] - to[0], from[1] - to[1], from[2] - to[2]]);
    let hit = block_hit(rt, to, back, to.map(|v| v.floor() as i32), false);
    rt.put_field(hit, "$miss", V::I(1));
    hit
}

fn optional(rt: &mut Rt, v: V) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", v);
    o
}

/// The empty collision context (`CollisionContext.empty()`): no entity.
pub fn empty_context(rt: &mut Rt) -> V {
    rt.intern("collision-context/empty".into(), |rt| rt.new_object(ENTITY_COLLISION_CONTEXT))
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, SHAPES | VOXEL_SHAPE | COLLISION_CONTEXT | ENTITY_COLLISION_CONTEXT | BOOLEAN_OP | AABB | BOUNDING_BOX | DISCRETE | SLICE)
        || (owner.starts_with("net/minecraft/world/level/block/") && matches!(name, "box" | "isFaceFull" | "isShapeFullBlock"))
        || (name == "clip" && api::is_level(owner))
        || (name == "<init>" && owner.ends_with("/level/ClipContext"))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let d = |i: usize| args.get(i).copied().unwrap_or_default().d();
    let (owner, name, desc) = crate::parts(key)?;
    let shape_of = |rt: &Rt, v: V| boxes(rt, v).unwrap_or_default();
    Some(match (owner, name) {
        // `BlockGetter.clip(context)`: where a ray through the level stops.
        (o, "clip") if api::is_level(o) && desc == LEVEL_CLIP => level_clip(rt, a(0), a(1)),
        // `new ClipContext(from, to, block, fluid, entity)`: the entity's collision context (the
        // empty one for none), then as the constructor taking a context, which the table models.
        (o, "<init>") if o.ends_with("/level/ClipContext") && desc == CLIP_CONTEXT_OF_ENTITY => {
            let entity = a(5);
            let context = if entity.r() == 0 {
                empty_context(rt)
            } else {
                let c = rt.new_object(ENTITY_COLLISION_CONTEXT);
                rt.put_field(c, "$entity", entity);
                c
            };
            let key = rt.static_key(&format!("{o}.<init>.{CLIP_CONTEXT_OF_CONTEXT}"));
            return crate::records::call(rt, key, &[a(0), a(1), a(2), a(3), a(4), context]);
        }
        // `AABB.clip(boxes, from, to, pos)`: where the segment first enters one of the boxes at `pos`.
        (AABB, "clip") if desc.starts_with("(Ljava/lang/Iterable;") => {
            let list: Vec<Box3> = crate::jutil::elements(rt, a(0)).into_iter().map(|b| aabb_of(rt, b)).collect();
            let (from, to, p) = (xyz(rt, a(1)), xyz(rt, a(2)), block_of(rt, a(3)));
            match clip_boxes(&list, from, to, p) {
                Some((at, face)) => block_hit(rt, at, face, p, false),
                None => V::NULL,
            }
        }
        // `Block.box`, in pixels; reached through any block class.
        (o, "box") if o.starts_with("net/minecraft/world/level/block/") && desc == "(DDDDDD)Lnet/minecraft/world/phys/shapes/VoxelShape;" => {
            let b = [d(0), d(1), d(2), d(3), d(4), d(5)].map(|x| x / 16.0);
            if (0..3).any(|i| b[i] > b[i + 3]) {
                rt.throw_new("java/lang/IllegalArgumentException");
                return Some(V::NULL);
            }
            boxed_shape(rt, b)
        }
        (SHAPES, "block") => boxed_shape(rt, FULL),
        // On any block class, as `javac` names the class a call was written on.
        (o, "isFaceFull") if o.starts_with("net/minecraft/world/level/block/") => {
            let s = shape_of(rt, a(0));
            let direction = rt.get_field(a(1), "$ordinal").i();
            V::I(face_full(&s, direction.clamp(0, 5) as usize) as i32)
        }
        (o, "isShapeFullBlock") if o.starts_with("net/minecraft/world/level/block/") => {
            let s = shape_of(rt, a(0));
            V::I(full_block(&s) as i32)
        }
        (SHAPES, "empty") => shape(rt, &[]),
        (SHAPES, "box") => {
            let b = [d(0), d(1), d(2), d(3), d(4), d(5)];
            if (0..3).any(|i| b[i] > b[i + 3]) {
                rt.throw_new("java/lang/IllegalArgumentException");
                return Some(V::NULL);
            }
            boxed_shape(rt, b)
        }
        (SHAPES, "create") if desc.starts_with("(DDDDDD)") => boxed_shape(rt, [d(0), d(1), d(2), d(3), d(4), d(5)]),
        (SHAPES, "create") => {
            let b = aabb_of(rt, a(0));
            boxed_shape(rt, b)
        }
        (SHAPES, "or") if desc.contains('[') => {
            let mut acc = shape_of(rt, a(0));
            for s in crate::jutil::elements(rt, a(1)) {
                let more = shape_of(rt, s);
                acc = join(&acc, &more, OR);
            }
            shape(rt, &acc)
        }
        (SHAPES, "or") => {
            let (x, y) = (shape_of(rt, a(0)), shape_of(rt, a(1)));
            let r = join(&x, &y, OR);
            shape(rt, &r)
        }
        (SHAPES, "join" | "joinUnoptimized") => {
            let (x, y) = (shape_of(rt, a(0)), shape_of(rt, a(1)));
            let t = op_table(rt, a(2));
            let r = join(&x, &y, t);
            shape(rt, &r)
        }
        (SHAPES, "joinIsNotEmpty") if desc.contains("BooleanOp") => {
            let (x, y) = (shape_of(rt, a(0)), shape_of(rt, a(1)));
            let t = op_table(rt, a(2));
            V::I(!join(&x, &y, t).is_empty() as i32)
        }
        (VOXEL_SHAPE, n) => {
            let s = shape_of(rt, a(0));
            match n {
                "isEmpty" => V::I(s.is_empty() as i32),
                "$shape" => discrete(rt, a(0)),
                // `clip(from, to, pos)`: where the segment hits the shape at `pos`; null for a miss.
                "clip" => {
                    let (from, to, p) = (xyz(rt, a(1)), xyz(rt, a(2)), block_of(rt, a(3)));
                    match clip_shape(&s, from, to, p) {
                        Some((at, face, inside)) => block_hit(rt, at, face, p, inside),
                        None => V::NULL,
                    }
                }
                "optimize" => shape(rt, &optimized(&s)),
                "move" => {
                    let o = match desc {
                        "(DDD)Lnet/minecraft/world/phys/shapes/VoxelShape;" => [d(1), d(2), d(3)],
                        _ if rt.class_of(a(1)) == VEC3 => xyz(rt, a(1)),
                        _ => pos(rt, a(1)),
                    };
                    let moved: Vec<Box3> = s.iter().map(|b| [b[0] + o[0], b[1] + o[1], b[2] + o[2], b[3] + o[0], b[4] + o[1], b[5] + o[2]]).collect();
                    shape(rt, &moved)
                }
                "bounds" => {
                    if s.is_empty() {
                        rt.throw_new("java/lang/UnsupportedOperationException");
                        return Some(V::NULL);
                    }
                    let b = bounds(&s);
                    aabb(rt, b)
                }
                "min" | "max" => {
                    let ax = axis_of(rt, a(1));
                    let max = n == "max";
                    let inside = |b: &Box3| {
                        if desc.contains("DD)") {
                            // The column through (primary, secondary): the axes after `ax` in its cycle.
                            let (p, q) = ((ax + 1) % 3, (ax + 2) % 3);
                            b[p] <= d(2) && d(2) < b[p + 3] && b[q] <= d(3) && d(3) < b[q + 3]
                        } else {
                            true
                        }
                    };
                    let it = s.iter().filter(|b| inside(b));
                    V::D(if max { it.map(|b| b[ax + 3]).fold(f64::NEG_INFINITY, f64::max) } else { it.map(|b| b[ax]).fold(f64::INFINITY, f64::min) })
                }
                "toAabbs" => {
                    let list: Vec<V> = optimized(&s).into_iter().map(|b| aabb(rt, b)).collect();
                    rt.alloc("java/util/ArrayList", Data::List(list))
                }
                "forAllBoxes" => {
                    for b in optimized(&s) {
                        rt.call_functional(CONSUME, a(1), &b.map(V::D));
                        if rt.thrown.is_some() {
                            break;
                        }
                    }
                    V::Void
                }
                "forAllEdges" => {
                    for b in optimized(&s) {
                        for e in edges(b) {
                            rt.call_functional(CONSUME, a(1), &e.map(V::D));
                            if rt.thrown.is_some() {
                                return Some(V::Void);
                            }
                        }
                    }
                    V::Void
                }
                _ => return None,
            }
        }
        (DISCRETE, n) => {
            let s = shape_of(rt, rt.get_field(a(0), "$shape"));
            let lines = cell_lines(&s);
            let size = |i: usize| (lines[i].len() - 1) as i32;
            match n {
                "getSize" => V::I(size(axis_of(rt, a(1)))),
                "getXSize" => V::I(size(0)),
                "getYSize" => V::I(size(1)),
                "getZSize" => V::I(size(2)),
                "isEmpty" => V::I(s.is_empty() as i32),
                // A cell is full when a box covers its middle.
                "isFull" | "isFullWide" => {
                    let c = [a(1).i(), a(2).i(), a(3).i()];
                    let mid: Option<Vec<f64>> = (0..3)
                        .map(|i| {
                            let k = usize::try_from(c[i]).ok()?;
                            Some((*lines[i].get(k)? + *lines[i].get(k + 1)?) / 2.0)
                        })
                        .collect();
                    let full = mid.is_some_and(|m| s.iter().any(|b| (0..3).all(|i| b[i] < m[i] && m[i] < b[i + 3])));
                    V::I(full as i32)
                }
                _ => return None,
            }
        }
        // `new SliceShape(shape, axis, index)`: the object `new` made becomes that section.
        (SLICE, "<init>") => {
            let s = shape_of(rt, a(1));
            let cut = slice(&s, axis_of(rt, a(2)), a(3).i());
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.class = VOXEL_SHAPE;
                o.data = Data::Array(cut.iter().flat_map(|b| b.iter().map(|&d| V::D(d))).collect());
            }
            V::Void
        }
        (COLLISION_CONTEXT | ENTITY_COLLISION_CONTEXT, n) => match n {
            "empty" => empty_context(rt),
            "of" => {
                let c = rt.new_object(ENTITY_COLLISION_CONTEXT);
                rt.put_field(c, "$entity", a(0));
                c
            }
            "getEntity" => rt.get_field(a(0), "$entity"),
            // Without an entity (the empty context) every answer is no, as vanilla's.
            "isHoldingItem" | "isDescending" | "isAbove" if rt.get_field(a(0), "$entity").r() == 0 => V::I(0),
            _ => return None,
        },
        (BOOLEAN_OP, "apply") => {
            let t = op_table(rt, a(0));
            V::I((t >> (((a(1).i() != 0) as u8) << 1 | (a(2).i() != 0) as u8) & 1) as i32)
        }
        (AABB, n) => return aabb_call(rt, n, desc, args),
        (BOUNDING_BOX, n) => return bounding_box_call(rt, n, desc, args),
        _ => return None,
    })
}

fn boxed_shape(rt: &mut Rt, b: Box3) -> V {
    let boxes = create(b);
    shape(rt, &boxes)
}

pub fn bounds(s: &[Box3]) -> Box3 {
    s.iter().fold(
        [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        |m, b| [m[0].min(b[0]), m[1].min(b[1]), m[2].min(b[2]), m[3].max(b[3]), m[4].max(b[4]), m[5].max(b[5])],
    )
}

/// A box's twelve edges, each `[x1, y1, z1, x2, y2, z2]`.
fn edges(b: Box3) -> Vec<Box3> {
    let mut out = Vec::with_capacity(12);
    for &y in &[b[1], b[4]] {
        for &z in &[b[2], b[5]] {
            out.push([b[0], y, z, b[3], y, z]);
        }
    }
    for &x in &[b[0], b[3]] {
        for &z in &[b[2], b[5]] {
            out.push([x, b[1], z, x, b[4], z]);
        }
    }
    for &x in &[b[0], b[3]] {
        for &y in &[b[1], b[4]] {
            out.push([x, y, b[2], x, y, b[5]]);
        }
    }
    out
}

fn aabb_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let d = |i: usize| args.get(i).copied().unwrap_or_default().d();
    if name == "<init>" {
        let b = match desc {
            "(DDDDDD)V" => [d(1), d(2), d(3), d(4), d(5), d(6)],
            "(Lnet/minecraft/core/BlockPos;)V" => {
                let p = pos(rt, a(1));
                [p[0], p[1], p[2], p[0] + 1.0, p[1] + 1.0, p[2] + 1.0]
            }
            "(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/phys/Vec3;)V" => {
                let (p, q) = (xyz(rt, a(1)), xyz(rt, a(2)));
                [p[0], p[1], p[2], q[0], q[1], q[2]]
            }
            _ => return None,
        };
        set_aabb(rt, a(0), sorted(b));
        return Some(V::Void);
    }
    let static_call = matches!(name, "ofSize" | "unitCubeFromLowerCorner" | "encapsulatingFullBlocks");
    let b = if static_call { [0.0; 6] } else { aabb_of(rt, a(0)) };
    let new = |rt: &mut Rt, x: Box3| aabb(rt, sorted(x));
    // A (DDD) triple, or the same from a Vec3 / BlockPos argument.
    let triple = |rt: &Rt| -> [f64; 3] {
        if desc.starts_with("(DDD)") {
            [d(1), d(2), d(3)]
        } else if desc.starts_with("(D)") {
            [d(1); 3]
        } else if rt.class_of(a(1)) == VEC3 {
            xyz(rt, a(1))
        } else {
            pos(rt, a(1))
        }
    };
    Some(match name {
        "move" => {
            let o = triple(rt);
            new(rt, [b[0] + o[0], b[1] + o[1], b[2] + o[2], b[3] + o[0], b[4] + o[1], b[5] + o[2]])
        }
        "inflate" | "deflate" => {
            let mut o = triple(rt);
            if name == "deflate" {
                o = o.map(|x| -x);
            }
            new(rt, [b[0] - o[0], b[1] - o[1], b[2] - o[2], b[3] + o[0], b[4] + o[1], b[5] + o[2]])
        }
        "expandTowards" | "contract" => {
            let o = triple(rt);
            // The side each component points to moves: out for expandTowards, in for contract.
            let sign = if name == "contract" { -1.0 } else { 1.0 };
            let mut r = b;
            for i in 0..3 {
                if o[i] < 0.0 {
                    r[i] += sign * o[i];
                } else if o[i] > 0.0 {
                    r[i + 3] += sign * o[i];
                }
            }
            new(rt, r)
        }
        "getCenter" => vec3(rt, [b[0] + (b[3] - b[0]) * 0.5, b[1] + (b[4] - b[1]) * 0.5, b[2] + (b[5] - b[2]) * 0.5]),
        "getXsize" => V::D(b[3] - b[0]),
        "getYsize" => V::D(b[4] - b[1]),
        "getZsize" => V::D(b[5] - b[2]),
        "getSize" => V::D((b[3] - b[0] + b[4] - b[1] + b[5] - b[2]) / 3.0),
        "getMinPosition" => vec3(rt, [b[0], b[1], b[2]]),
        "getMaxPosition" => vec3(rt, [b[3], b[4], b[5]]),
        "getBottomCenter" => vec3(rt, [b[0] + (b[3] - b[0]) * 0.5, b[1], b[2] + (b[5] - b[2]) * 0.5]),
        "hasNaN" => V::I(b.iter().any(|x| x.is_nan()) as i32),
        "intersects" => {
            let o = match desc {
                "(DDDDDD)Z" => [d(1), d(2), d(3), d(4), d(5), d(6)],
                "(Lnet/minecraft/world/phys/AABB;)Z" => aabb_of(rt, a(1)),
                _ => {
                    let (p, q) = (xyz(rt, a(1)), xyz(rt, a(2)));
                    sorted([p[0], p[1], p[2], q[0], q[1], q[2]])
                }
            };
            V::I((b[0] < o[3] && b[3] > o[0] && b[1] < o[4] && b[4] > o[1] && b[2] < o[5] && b[5] > o[2]) as i32)
        }
        "contains" => {
            let p = if desc == "(DDD)Z" { [d(1), d(2), d(3)] } else { xyz(rt, a(1)) };
            V::I((p[0] >= b[0] && p[0] < b[3] && p[1] >= b[1] && p[1] < b[4] && p[2] >= b[2] && p[2] < b[5]) as i32)
        }
        "minmax" => {
            let o = aabb_of(rt, a(1));
            new(rt, [b[0].min(o[0]), b[1].min(o[1]), b[2].min(o[2]), b[3].max(o[3]), b[4].max(o[4]), b[5].max(o[5])])
        }
        "intersect" => {
            let o = aabb_of(rt, a(1));
            new(rt, [b[0].max(o[0]), b[1].max(o[1]), b[2].max(o[2]), b[3].min(o[3]), b[4].min(o[4]), b[5].min(o[5])])
        }
        "ofSize" => {
            let c = xyz(rt, a(0));
            let (w, h, l) = (d(1), d(2), d(3));
            new(rt, [c[0] - w / 2.0, c[1] - h / 2.0, c[2] - l / 2.0, c[0] + w / 2.0, c[1] + h / 2.0, c[2] + l / 2.0])
        }
        "unitCubeFromLowerCorner" => {
            let c = xyz(rt, a(0));
            new(rt, [c[0], c[1], c[2], c[0] + 1.0, c[1] + 1.0, c[2] + 1.0])
        }
        "encapsulatingFullBlocks" => {
            let (p, q) = (pos(rt, a(0)), pos(rt, a(1)));
            new(rt, [p[0].min(q[0]), p[1].min(q[1]), p[2].min(q[2]), p[0].max(q[0]) + 1.0, p[1].max(q[1]) + 1.0, p[2].max(q[2]) + 1.0])
        }
        "clip" if desc == "(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/phys/Vec3;)Ljava/util/Optional;" => {
            let (from, to) = (xyz(rt, a(1)), xyz(rt, a(2)));
            match clip(b, from, to) {
                Some(p) => {
                    let v = vec3(rt, p);
                    optional(rt, v)
                }
                None => optional(rt, V::NULL),
            }
        }
        "equals" => {
            let same = a(1).r() != 0 && rt.class_of(a(1)) == AABB && aabb_of(rt, a(1)).iter().zip(&b).all(|(x, y)| x.total_cmp(y).is_eq());
            V::I(same as i32)
        }
        "toString" => rt.string(&format!("AABB[{}, {}, {}] -> [{}, {}, {}]", b[0], b[1], b[2], b[3], b[4], b[5])),
        _ => return None,
    })
}

/// A structure's `BoundingBox`: whole blocks, both corners inside, `[min x, y, z, max x, y, z]`.
pub type IntBox = [i32; 6];

const BOX_FIELDS: [&str; 6] = ["minX", "minY", "minZ", "maxX", "maxY", "maxZ"];

pub fn int_box_of(rt: &Rt, o: V) -> IntBox {
    BOX_FIELDS.map(|k| rt.get_field(o, k).i())
}

fn set_int_box(rt: &mut Rt, o: V, b: IntBox) {
    for (k, v) in BOX_FIELDS.into_iter().zip(b) {
        rt.put_field(o, k, V::I(v));
    }
}

/// `new BoundingBox(...)`: inverted bounds swap, as vanilla's constructor does outside a
/// development environment (it logs them).
fn ordered(b: IntBox) -> IntBox {
    [b[0].min(b[3]), b[1].min(b[4]), b[2].min(b[5]), b[0].max(b[3]), b[1].max(b[4]), b[2].max(b[5])]
}

/// A new `BoundingBox` over `b`.
pub fn bounding_box(rt: &mut Rt, b: IntBox) -> V {
    let o = rt.new_object(BOUNDING_BOX);
    set_int_box(rt, o, ordered(b));
    o
}

fn block_of(rt: &Rt, v: V) -> [i32; 3] {
    pos(rt, v).map(|x| x as i32)
}

/// `BoundingBox.orientBox`: a piece's box of size `w`, `h`, `d` at `offset` in a structure at
/// `at`, turned to `facing` (a direction ordinal; up and down face south, as vanilla's `default`).
fn orient_box(at: [i32; 3], offset: [i32; 3], size: [i32; 3], facing: u8) -> IntBox {
    let ([x, y, z], [ox, oy, oz], [w, h, d]) = (at, offset, size);
    let (y0, y1) = (y + oy, y + h - 1 + oy);
    match facing {
        2 => [x + ox, y0, z - d + 1 + oz, x + w - 1 + ox, y1, z + oz],
        4 => [x - d + 1 + oz, y0, z + ox, x + oz, y1, z + w - 1 + ox],
        5 => [x + oz, y0, z + ox, x + d - 1 + oz, y1, z + w - 1 + ox],
        _ => [x + ox, y0, z + oz, x + w - 1 + ox, y1, z + d - 1 + oz],
    }
}

/// `BoundingBox`, as 1.21.1's: mutable (`encapsulate` and `move` change it, `moved` and
/// `inflatedBy` make another), its sizes and moves in Java's wrapping `int` arithmetic (an
/// `infinite()` box spans 0).
fn bounding_box_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let i = |n: usize| a(n).i();
    let is_box = |rt: &Rt, v: V| v.r() != 0 && rt.class_of(v) == BOUNDING_BOX;
    // The static calls first: the rest read their box from the receiver.
    match (name, desc) {
        ("<init>", "(Lnet/minecraft/core/BlockPos;)V") => {
            let p = block_of(rt, a(1));
            set_int_box(rt, a(0), [p[0], p[1], p[2], p[0], p[1], p[2]]);
            return Some(V::Void);
        }
        ("<init>", "(IIIIII)V") => {
            set_int_box(rt, a(0), ordered([i(1), i(2), i(3), i(4), i(5), i(6)]));
            return Some(V::Void);
        }
        ("fromCorners", _) => {
            let (p, q) = (block_of(rt, a(0)), block_of(rt, a(1)));
            return Some(bounding_box(rt, [p[0], p[1], p[2], q[0], q[1], q[2]]));
        }
        ("infinite", _) => return Some(bounding_box(rt, [i32::MIN, i32::MIN, i32::MIN, i32::MAX, i32::MAX, i32::MAX])),
        ("orientBox", _) => {
            let facing = crate::api_common::direction_ordinal(rt, a(9));
            let b = orient_box([i(0), i(1), i(2)], [i(3), i(4), i(5)], [i(6), i(7), i(8)], facing);
            return Some(bounding_box(rt, b));
        }
        // The box around every position (or box) given, none for none.
        ("encapsulatingPositions" | "encapsulatingBoxes", _) => {
            let items = crate::jutil::elements(rt, a(0));
            let mut found: Option<IntBox> = None;
            for v in items {
                let b = if is_box(rt, v) {
                    int_box_of(rt, v)
                } else {
                    let p = block_of(rt, v);
                    [p[0], p[1], p[2], p[0], p[1], p[2]]
                };
                found = Some(match found {
                    None => b,
                    Some(f) => [f[0].min(b[0]), f[1].min(b[1]), f[2].min(b[2]), f[3].max(b[3]), f[4].max(b[4]), f[5].max(b[5])],
                });
            }
            let held = match found {
                Some(b) => bounding_box(rt, b),
                None => V::NULL,
            };
            return Some(optional(rt, held));
        }
        _ => {}
    }
    let this = a(0);
    let b = int_box_of(rt, this);
    // A Vec3i argument's coordinates, or three ints.
    let triple = |rt: &Rt| if desc.starts_with("(III)") { [i(1), i(2), i(3)] } else { block_of(rt, a(1)) };
    let shifted = |o: [i32; 3]| [b[0].wrapping_add(o[0]), b[1].wrapping_add(o[1]), b[2].wrapping_add(o[2]), b[3].wrapping_add(o[0]), b[4].wrapping_add(o[1]), b[5].wrapping_add(o[2])];
    let inside = |p: [i32; 3]| (0..3).all(|k| p[k] >= b[k] && p[k] <= b[k + 3]);
    Some(match name {
        "minX" => V::I(b[0]),
        "minY" => V::I(b[1]),
        "minZ" => V::I(b[2]),
        "maxX" => V::I(b[3]),
        "maxY" => V::I(b[4]),
        "maxZ" => V::I(b[5]),
        "getXSpan" => V::I(b[3].wrapping_sub(b[0]).wrapping_add(1)),
        "getYSpan" => V::I(b[4].wrapping_sub(b[1]).wrapping_add(1)),
        "getZSpan" => V::I(b[5].wrapping_sub(b[2]).wrapping_add(1)),
        "getLength" => {
            let l = [0, 1, 2].map(|k| i64::from(b[k + 3].wrapping_sub(b[k])));
            rt.alloc("net/minecraft/core/Vec3i", Data::Native("BlockPos", [l[0], l[1], l[2], 0]))
        }
        "getCenter" => {
            let c = [0, 1, 2].map(|k| i64::from(b[k].wrapping_add(b[k + 3].wrapping_sub(b[k]).wrapping_add(1) / 2)));
            api::block_pos(rt, c[0], c[1], c[2])
        }
        "isInside" => {
            let p = triple(rt);
            V::I(inside(p) as i32)
        }
        "intersects" if desc.starts_with("(IIII)") => {
            let (x0, z0, x1, z1) = (i(1), i(2), i(3), i(4));
            V::I((b[3] >= x0 && b[0] <= x1 && b[5] >= z0 && b[2] <= z1) as i32)
        }
        "intersects" => {
            let o = int_box_of(rt, a(1));
            V::I((0..3).all(|k| b[k + 3] >= o[k] && b[k] <= o[k + 3]) as i32)
        }
        // This box grown to hold a position or a box: itself, changed.
        "encapsulate" => {
            let o = if is_box(rt, a(1)) {
                int_box_of(rt, a(1))
            } else {
                let p = block_of(rt, a(1));
                [p[0], p[1], p[2], p[0], p[1], p[2]]
            };
            set_int_box(rt, this, [b[0].min(o[0]), b[1].min(o[1]), b[2].min(o[2]), b[3].max(o[3]), b[4].max(o[4]), b[5].max(o[5])]);
            this
        }
        "move" => {
            let o = triple(rt);
            set_int_box(rt, this, shifted(o));
            this
        }
        "moved" => {
            let o = [i(1), i(2), i(3)];
            bounding_box(rt, shifted(o))
        }
        "inflatedBy" => {
            let o = if desc.starts_with("(I)") { [i(1); 3] } else { [i(1), i(2), i(3)] };
            bounding_box(rt, [b[0].wrapping_sub(o[0]), b[1].wrapping_sub(o[1]), b[2].wrapping_sub(o[2]), b[3].wrapping_add(o[0]), b[4].wrapping_add(o[1]), b[5].wrapping_add(o[2])])
        }
        "equals" => V::I((is_box(rt, a(1)) && int_box_of(rt, a(1)) == b) as i32),
        // `Objects.hash` of the six bounds.
        "hashCode" => V::I(b.iter().fold(1i32, |h, &x| h.wrapping_mul(31).wrapping_add(x))),
        // Guava's `MoreObjects.toStringHelper`.
        "toString" => rt.string(&format!("BoundingBox{{minX={}, minY={}, minZ={}, maxX={}, maxY={}, maxZ={}}}", b[0], b[1], b[2], b[3], b[4], b[5])),
        _ => return None,
    })
}

/// `api::state`'s position argument for shape queries: the origin.
pub fn origin(rt: &mut Rt) -> V {
    rt.intern("shapes/origin".into(), |rt| api::block_pos(rt, 0, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(b: [f64; 6]) -> Box3 {
        b.map(|x| x / 16.0)
    }

    #[test]
    fn joins_are_exact_and_merged() {
        // A slab and the half above it: one full block.
        let lower = [px([0., 0., 0., 16., 8., 16.])];
        let upper = [px([0., 8., 0., 16., 16., 16.])];
        assert_eq!(join(&lower, &upper, OR), vec![FULL]);
        // A full block less its upper half is the slab; their intersection is empty.
        assert_eq!(join(&[FULL], &upper, 0b0100), lower.to_vec());
        assert!(join(&lower, &upper, 0b1000).is_empty());
        // A shaft along x crossing one along z: the cross, both arms whole.
        let x = px([0., 6., 6., 16., 10., 10.]);
        let z = px([6., 6., 0., 10., 10., 16.]);
        let cross = join(&[x], &[z], OR);
        let volume: f64 = cross.iter().map(|b| (b[3] - b[0]) * (b[4] - b[1]) * (b[5] - b[2])).sum();
        let arm = 16. * 4. * 4. / 4096.;
        assert!((volume - (2. * arm - 4. * 4. * 4. / 4096.)).abs() < 1e-12, "{cross:?}");
        // Their difference (NOT_SAME): the cross without its centre.
        let ring = join(&[x], &[z], 0b0110);
        let v2: f64 = ring.iter().map(|b| (b[3] - b[0]) * (b[4] - b[1]) * (b[5] - b[2])).sum();
        assert!((v2 - (2. * arm - 2. * 4. * 4. * 4. / 4096.)).abs() < 1e-12);
    }

    #[test]
    fn cells_and_sections_are_vanillas() {
        // Values by hand from 1.21.1's `Shapes.create` (`findBits`), `joinUnoptimized` and
        // `SliceShape`: a bottom slab has 2 cells up y, 1 across; a third of a block isn't on a
        // power-of-two grid, so its cells are its own coordinates'.
        let slab = [px([0., 0., 0., 16., 8., 16.])];
        assert_eq!(cell_lines(&slab)[1], vec![0.0, 0.5, 1.0]);
        assert_eq!(cell_lines(&slab)[0], vec![0.0, 1.0]);
        let third = [[0.0, 0.0, 0.0, 1.0, 1.0 / 3.0, 1.0]];
        assert_eq!(cell_lines(&third)[1], vec![0.0, 1.0 / 3.0]);
        assert_eq!(cell_lines(&[px([2., 0., 0., 4., 16., 16.])])[0].len(), 9);
        // A stair (slab and the north half above): its sections up y are the slab's square, then
        // the north half; along z, the north end is full, the south one the slab's side.
        let stair = [px([0., 0., 0., 16., 8., 16.]), px([0., 8., 0., 16., 16., 8.])];
        assert_eq!(slice(&stair, 1, 0), vec![FULL]);
        assert_eq!(slice(&stair, 1, 1), vec![px([0., 0., 0., 16., 16., 8.])]);
        assert_eq!(join(&slice(&stair, 2, 0), &[], OR), vec![FULL]);
        assert_eq!(join(&slice(&stair, 2, 1), &[], OR), vec![px([0., 0., 0., 16., 8., 16.])]);
        // Out of the cells: nothing.
        assert!(slice(&stair, 1, 2).is_empty() && slice(&stair, 1, -1).is_empty());
        // Through the runtime: a mixin accessor's `VoxelShape.shape`, its size, and a section.
        let mut rt = Rt::new();
        let v = shape(&mut rt, &stair);
        let d = call(&mut rt, "net/minecraft/world/phys/shapes/VoxelShape.$shape.()Lnet/minecraft/world/phys/shapes/DiscreteVoxelShape;", &[v]).unwrap();
        let y = api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.Y");
        assert_eq!(call(&mut rt, "net/minecraft/world/phys/shapes/DiscreteVoxelShape.getSize.(Lnet/minecraft/core/Direction$Axis;)I", &[d, y]), Some(V::I(2)));
        assert_eq!(call(&mut rt, "net/minecraft/world/phys/shapes/DiscreteVoxelShape.isFull.(III)Z", &[d, V::I(0), V::I(1), V::I(0)]), Some(V::I(1)));
        let cut = rt.new_object(SLICE);
        call(&mut rt, "net/minecraft/world/phys/shapes/SliceShape.<init>.(Lnet/minecraft/world/phys/shapes/VoxelShape;Lnet/minecraft/core/Direction$Axis;I)V", &[cut, v, y, V::I(1)]);
        assert_eq!(boxes(&rt, cut), Some(vec![px([0., 0., 0., 16., 16., 8.])]));
    }

    #[test]
    fn a_face_is_full_where_the_shape_covers_all_of_it() {
        // Directions by ordinal: down, up, north, south, west, east.
        let faces = |b: &[Box3]| (0..6).map(|d| face_full(b, d)).collect::<Vec<bool>>();
        assert_eq!(faces(&[FULL]), [true; 6]);
        // A bottom slab: a mob stands on its top, which isn't full; its bottom is.
        assert_eq!(faces(&[px([0., 0., 0., 16., 8., 16.])]), [true, false, false, false, false, false]);
        // A stair facing east (bottom slab, the east half above): full below, east and nowhere else.
        let stair = [px([0., 0., 0., 16., 8., 16.]), px([8., 8., 0., 16., 16., 16.])];
        assert_eq!(faces(&stair), [true, false, false, false, false, true]);
        // Two halves side by side make a full bottom; a hopper's top rim with its hole doesn't.
        assert!(face_full(&[px([0., 0., 0., 8., 4., 16.]), px([8., 0., 0., 16., 4., 16.])], 0));
        let rim = [px([0., 10., 0., 2., 16., 16.]), px([14., 10., 0., 16., 16., 16.]), px([2., 10., 0., 14., 16., 2.]), px([2., 10., 14., 14., 16., 16.])];
        assert!(!face_full(&rim, 1));
        // A carpet's bottom is full; a shape past the block on that face isn't one block's face.
        assert!(face_full(&[px([0., 0., 0., 16., 1., 16.])], 0));
        assert!(!face_full(&[px([-8., 0., 0., 16., 16., 16.])], 2));
        assert!(!face_full(&[], 0));
        assert!(full_block(&[px([0., 0., 0., 16., 8., 16.]), px([0., 8., 0., 16., 16., 16.])]));
        assert!(!full_block(&[px([0., 0., 0., 16., 8., 16.])]));
    }

    #[test]
    fn boxes_merge_as_vanilla_lists_them() {
        // Two halves side by side along z merge into one strip; vanilla's forAllBoxes gives one box.
        let a = px([0., 0., 0., 16., 16., 8.]);
        let b = px([0., 0., 8., 16., 16., 16.]);
        assert_eq!(optimized(&[a, b]), vec![FULL]);
        // An L: the strip along z first, then the rest.
        let l = optimized(&[px([0., 0., 0., 8., 8., 16.]), px([8., 0., 0., 16., 8., 8.])]);
        assert_eq!(l, vec![px([0., 0., 0., 8., 8., 16.]), px([8., 0., 0., 16., 8., 8.])]);
        // Thinner than vanilla's epsilon is nothing.
        assert!(create([0., 0., 0., 1., 0., 1.]).is_empty());
    }

    /// `BoundingBox` as 1.21.1's code computes it (the values worked by hand from its source):
    /// corners in any order, sizes counting both corners, the centre rounding down, `orientBox`
    /// for each facing, `moved` beside the mutating `move` and `encapsulate`, `Objects.hash`, the
    /// toString; and Create's `/create clone` sum, `destination.offset(box.getLength())`.
    #[test]
    fn a_bounding_box_is_vanillas() {
        fn call(rt: &mut Rt, n: &str, args: &[V]) -> V {
            let key: &'static str = Box::leak(format!("{BOUNDING_BOX}.{n}").into_boxed_str());
            api::call(rt, key, args)
        }
        const BOX_RET: &str = "Lnet/minecraft/world/level/levelgen/structure/BoundingBox;";
        let mut rt = Rt::new();
        let (a, b) = (api::block_pos(&mut rt, 5, 64, -3), api::block_pos(&mut rt, 2, 66, 1));
        let corners = format!("fromCorners.(Lnet/minecraft/core/Vec3i;Lnet/minecraft/core/Vec3i;){BOX_RET}");
        let bb = call(&mut rt, &corners, &[a, b]);
        assert_eq!(int_box_of(&rt, bb), [2, 64, -3, 5, 66, 1]);
        let spans = ["getXSpan", "getYSpan", "getZSpan"].map(|n| call(&mut rt, &format!("{n}.()I"), &[bb]).i());
        assert_eq!(spans, [4, 3, 5]);
        let length = call(&mut rt, "getLength.()Lnet/minecraft/core/Vec3i;", &[bb]);
        assert_eq!(api::pos_of(&rt, length), [3, 2, 4]);
        let destination = api::block_pos(&mut rt, 10, 70, 0);
        let end = api::call(&mut rt, "net/minecraft/core/BlockPos.offset.(Lnet/minecraft/core/Vec3i;)Lnet/minecraft/core/BlockPos;", &[destination, length]);
        assert_eq!(api::pos_of(&rt, end), [13, 72, 4]);
        let center = call(&mut rt, "getCenter.()Lnet/minecraft/core/BlockPos;", &[bb]);
        assert_eq!(api::pos_of(&rt, center), [4, 65, -1]);
        let (corner, past) = (api::block_pos(&mut rt, 5, 66, 1), api::block_pos(&mut rt, 6, 66, 1));
        assert_eq!(call(&mut rt, "isInside.(Lnet/minecraft/core/Vec3i;)Z", &[bb, corner]).i(), 1);
        assert_eq!(call(&mut rt, "isInside.(Lnet/minecraft/core/Vec3i;)Z", &[bb, past]).i(), 0);
        assert_eq!(call(&mut rt, "isInside.(III)Z", &[bb, V::I(2), V::I(64), V::I(-3)]).i(), 1);
        assert_eq!(call(&mut rt, "hashCode.()I", &[bb]).i(), 1_003_784_806);
        let text = call(&mut rt, "toString.()Ljava/lang/String;", &[bb]);
        assert_eq!(rt.str_of(text), Some("BoundingBox{minX=2, minY=64, minZ=-3, maxX=5, maxY=66, maxZ=1}"));
        let same = call(&mut rt, &corners, &[b, a]);
        assert_eq!(call(&mut rt, "equals.(Ljava/lang/Object;)Z", &[bb, same]).i(), 1);
        // Touching boxes intersect; one a block off doesn't.
        let touching = bounding_box(&mut rt, [5, 66, 1, 9, 70, 9]);
        let off = bounding_box(&mut rt, [6, 66, 1, 9, 70, 9]);
        let intersects = format!("intersects.({BOX_RET})Z");
        assert_eq!(call(&mut rt, &intersects, &[bb, touching]).i(), 1);
        assert_eq!(call(&mut rt, &intersects, &[bb, off]).i(), 0);
        assert_eq!(call(&mut rt, "intersects.(IIII)Z", &[bb, V::I(5), V::I(1), V::I(9), V::I(9)]).i(), 1);
        assert_eq!(call(&mut rt, "intersects.(IIII)Z", &[bb, V::I(6), V::I(1), V::I(9), V::I(9)]).i(), 0);
        let inflated = call(&mut rt, &format!("inflatedBy.(I){BOX_RET}"), &[bb, V::I(1)]);
        assert_eq!(int_box_of(&rt, inflated), [1, 63, -4, 6, 67, 2]);
        let moved = call(&mut rt, &format!("moved.(III){BOX_RET}"), &[bb, V::I(1), V::I(2), V::I(3)]);
        assert_eq!(int_box_of(&rt, moved), [3, 66, 0, 6, 68, 4]);
        assert_eq!(int_box_of(&rt, bb), [2, 64, -3, 5, 66, 1], "moved makes another box");
        let east = api::block_pos(&mut rt, 1, 0, 0);
        let itself = call(&mut rt, &format!("move.(Lnet/minecraft/core/Vec3i;){BOX_RET}"), &[bb, east]);
        assert_eq!((itself, int_box_of(&rt, bb)), (bb, [3, 64, -3, 6, 66, 1]));
        let far = api::block_pos(&mut rt, 10, 0, 0);
        call(&mut rt, &format!("encapsulate.(Lnet/minecraft/core/BlockPos;){BOX_RET}"), &[bb, far]);
        assert_eq!(int_box_of(&rt, bb), [3, 0, -3, 10, 66, 1]);
        // Inverted bounds swap; the infinite box's spans wrap to 0, as Java's ints do.
        let inverted = rt.new_object(BOUNDING_BOX);
        call(&mut rt, "<init>.(IIIIII)V", &[inverted, V::I(5), V::I(0), V::I(0), V::I(2), V::I(1), V::I(1)]);
        assert_eq!(int_box_of(&rt, inverted), [2, 0, 0, 5, 1, 1]);
        let infinite = call(&mut rt, &format!("infinite.(){BOX_RET}"), &[]);
        assert_eq!(call(&mut rt, "getXSpan.()I", &[infinite]).i(), 0);
        // A piece 4 wide, 5 high, 6 deep at (1, 2, 3) in a structure at (100, 64, 200).
        let orient = format!("orientBox.(IIIIIIIIILnet/minecraft/core/Direction;){BOX_RET}");
        for (facing, want) in [
            (3, [101, 66, 203, 104, 70, 208]),
            (2, [101, 66, 198, 104, 70, 203]),
            (4, [98, 66, 201, 103, 70, 204]),
            (5, [103, 66, 201, 108, 70, 204]),
            (1, [101, 66, 203, 104, 70, 208]),
        ] {
            let d = api::direction(&mut rt, facing);
            let ints = [100, 64, 200, 1, 2, 3, 4, 5, 6].map(V::I);
            let mut args = ints.to_vec();
            args.push(d);
            let o = call(&mut rt, &orient, &args);
            assert_eq!(int_box_of(&rt, o), want, "facing {facing}");
        }
        // The box around positions; none around none.
        let (p, q) = (api::block_pos(&mut rt, 1, 2, 3), api::block_pos(&mut rt, 0, 5, -1));
        let list = crate::jutil::new_list(&mut rt, vec![p, q]);
        let around = call(&mut rt, "encapsulatingPositions.(Ljava/lang/Iterable;)Ljava/util/Optional;", &[list]);
        let held = rt.get_field(around, "value");
        assert_eq!(int_box_of(&rt, held), [0, 2, -1, 1, 5, 3]);
        let empty = crate::jutil::new_list(&mut rt, Vec::new());
        let none = call(&mut rt, "encapsulatingPositions.(Ljava/lang/Iterable;)Ljava/util/Optional;", &[empty]);
        assert_eq!(rt.get_field(none, "value").r(), 0);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_ray_through_the_level_stops_where_vanillas_does() {
        use crate::host::MemoryHost;
        const CLIP_CONTEXT: &str = "net/minecraft/world/level/ClipContext";
        const HIT: &str = "net/minecraft/world/phys/BlockHitResult";
        fn key(owner: &str, name: &str, desc: &str) -> &'static str {
            format!("{owner}.{name}.{desc}").leak()
        }
        /// `level.clip(new ClipContext(from, to, block, fluid, null))`, read back through
        /// `BlockHitResult`'s getters: where, the face, the block, inside, a miss.
        fn ray(rt: &mut Rt, from: [f64; 3], to: [f64; 3], block: (&'static str, i32), fluid: (&'static str, i32)) -> ([f64; 3], u8, [i32; 3], bool, bool) {
            let ctx = rt.new_object(CLIP_CONTEXT);
            let (f, t) = (vec3(rt, from), vec3(rt, to));
            let b = api::api_enum(rt, "net/minecraft/world/level/ClipContext$Block", block.0, block.1);
            let fl = api::api_enum(rt, "net/minecraft/world/level/ClipContext$Fluid", fluid.0, fluid.1);
            api::call(rt, key(CLIP_CONTEXT, "<init>", CLIP_CONTEXT_OF_ENTITY), &[ctx, f, t, b, fl, V::NULL]);
            let level = crate::glue::level(rt);
            let hit = api::call(rt, key("net/minecraft/world/level/Level", "clip", LEVEL_CLIP), &[level, ctx]);
            let loc = api::call(rt, key(HIT, "getLocation", "()Lnet/minecraft/world/phys/Vec3;"), &[hit]);
            let dir = api::call(rt, key(HIT, "getDirection", "()Lnet/minecraft/core/Direction;"), &[hit]);
            let at = api::call(rt, key(HIT, "getBlockPos", "()Lnet/minecraft/core/BlockPos;"), &[hit]);
            let inside = api::call(rt, key(HIT, "isInside", "()Z"), &[hit]).i() != 0;
            let kind = api::call(rt, key(HIT, "getType", "()Lnet/minecraft/world/phys/HitResult$Type;"), &[hit]);
            let miss = rt.get_field(kind, "$name");
            let miss = rt.str_of(miss) == Some("MISS");
            (xyz(rt, loc), crate::api_common::direction_ordinal(rt, dir), block_of(rt, at), inside, miss)
        }
        fn near(a: [f64; 3], b: [f64; 3]) -> bool {
            (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
        }
        const COLLIDER: (&str, i32) = ("COLLIDER", 0);
        const OUTLINE: (&str, i32) = ("OUTLINE", 1);
        const NONE: (&str, i32) = ("NONE", 0);
        const SOURCE_ONLY: (&str, i32) = ("SOURCE_ONLY", 1);
        const ANY: (&str, i32) = ("ANY", 2);
        const WATER: (&str, i32) = ("WATER", 3);
        let (north, up) = (2, 1);
        // A poppy (nothing to collide with, a 6x10x6 pixel outline) at z 1, stone at z 3, a water
        // source at y 60 and a lava source beside it.
        let mut h = MemoryHost::default();
        h.blocks.insert([0, 64, 1], "minecraft:poppy".into());
        h.collisions.insert([0, 64, 1], Vec::new());
        h.outlines.insert([0, 64, 1], vec![[5. / 16., 0., 5. / 16., 11. / 16., 10. / 16., 11. / 16.]]);
        h.blocks.insert([0, 64, 3], "minecraft:stone".into());
        h.blocks.insert([0, 60, 0], "minecraft:water[level=0]".into());
        h.blocks.insert([2, 60, 0], "minecraft:lava[level=0]".into());
        h.fluid_tags.insert("minecraft:water".into(), vec!["minecraft:water".into(), "minecraft:flowing_water".into()]);
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            let (from, to) = ([0.5, 64.5, -0.5], [0.5, 64.5, 5.5]);
            // What collides: through the poppy to the stone's north face.
            assert!(matches!(ray(rt, from, to, COLLIDER, NONE), (at, f, [0, 64, 3], false, false) if near(at, [0.5, 64.5, 3.0]) && f == north));
            // What a player targets: the poppy's outline, 5 pixels into its block.
            assert!(matches!(ray(rt, from, to, OUTLINE, NONE), (at, f, [0, 64, 1], false, false) if near(at, [0.5, 64.5, 1.3125]) && f == north));
            // Nothing on the way: a miss at the end, facing back, in the block holding it.
            assert!(matches!(ray(rt, [10.5, 80.5, 0.5], [10.5, 80.5, 3.5], COLLIDER, NONE), (at, f, [10, 80, 3], false, true) if near(at, [10.5, 80.5, 3.5]) && f == north));
            // Down onto water: its surface stands 8/9 of a block high (a source's amount over 9, in
            // floats), unless the ray ignores fluids.
            let (above, below) = ([0.5, 62.5, 0.5], [0.5, 58.5, 0.5]);
            let surface = [0.5, 60.0 + f64::from(8f32 / 9f32), 0.5];
            for fluid in [ANY, SOURCE_ONLY, WATER] {
                assert!(matches!(ray(rt, above, below, COLLIDER, fluid), (at, f, [0, 60, 0], false, false) if near(at, surface) && f == up), "{fluid:?}");
            }
            assert!(matches!(ray(rt, above, below, COLLIDER, NONE), (at, _, _, false, true) if near(at, below)));
            // Lava isn't water.
            let (lava_above, lava_below) = ([2.5, 62.5, 0.5], [2.5, 58.5, 0.5]);
            assert!(ray(rt, lava_above, lava_below, COLLIDER, WATER).4);
            assert!(!ray(rt, lava_above, lava_below, COLLIDER, ANY).4);
            // From inside the stone: a hit a thousandth of the way along, inside, on the face
            // looking back.
            assert!(matches!(ray(rt, [0.5, 64.5, 3.5], [0.5, 64.5, 5.5], COLLIDER, NONE), (at, f, [0, 64, 3], true, false) if near(at, [0.5, 64.5, 3.502]) && f == north));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn clip_finds_the_face_a_ray_enters() {
        let b = [0., 0., 0., 1., 1., 1.];
        assert_eq!(clip(b, [-1., 0.5, 0.5], [2., 0.5, 0.5]), Some([0., 0.5, 0.5]));
        assert_eq!(clip(b, [0.5, 3., 0.5], [0.5, -3., 0.5]), Some([0.5, 1., 0.5]));
        assert_eq!(clip(b, [-1., 2., 0.5], [2., 2., 0.5]), None);
    }
}
