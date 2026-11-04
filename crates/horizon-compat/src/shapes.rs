//! Shapes for translated mods: `VoxelShape` and `Shapes` (`Block.box`, joins), `AABB` and
//! `CollisionContext`. A shape is the union of boxes, each `[min x, y, z, max x, y, z]`. Joins
//! work as vanilla's do: cell by cell on the grid of both shapes' coordinates, then merged into
//! boxes in the order vanilla's `forAllBoxes` merges them (strips along z, grown along x, then y).
use crate::{api, Data, Rt, V};

pub const VOXEL_SHAPE: &str = "net/minecraft/world/phys/shapes/VoxelShape";
const SHAPES: &str = "net/minecraft/world/phys/shapes/Shapes";
pub const AABB: &str = "net/minecraft/world/phys/AABB";
const BOOLEAN_OP: &str = "net/minecraft/world/phys/shapes/BooleanOp";
const COLLISION_CONTEXT: &str = "net/minecraft/world/phys/shapes/CollisionContext";
pub const ENTITY_COLLISION_CONTEXT: &str = "net/minecraft/world/phys/shapes/EntityCollisionContext";
const VEC3: &str = "net/minecraft/world/phys/Vec3";
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

/// `Block.isShapeFullBlock`: the shape is the whole block, nothing missing and nothing past it.
pub fn full_block(boxes: &[Box3]) -> bool {
    join(&[FULL], boxes, NOT_SAME).is_empty()
}

/// `Block.isFaceFull(shape, direction)` (`direction` by ordinal): the shape's slice just inside
/// that face (`VoxelShape.getFaceShape`: the cells at 1e-7, or 0.9999999 on a positive face, along
/// its axis, made as deep as the block) is a full block, so it covers the whole face and nothing
/// past it.
pub fn face_full(boxes: &[Box3], direction: usize) -> bool {
    let axis = [1, 1, 2, 2, 0, 0][direction.min(5)];
    let at = if direction % 2 == 1 { 0.9999999 } else { 1.0e-7 };
    let slice: Vec<Box3> = boxes
        .iter()
        .filter(|b| b[axis] <= at && at < b[axis + 3])
        .map(|b| {
            let mut s = *b;
            (s[axis], s[axis + 3]) = (0.0, 1.0);
            s
        })
        .collect();
    !slice.is_empty() && full_block(&slice)
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
fn clip(b: Box3, from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let mut scale = 1.0;
    let mut hit = false;
    // Each axis in vanilla's order (x, y, z), with the other two in its cycle.
    for (a, b1, c1) in [(0, 1, 2), (1, 2, 0), (2, 0, 1)] {
        let point = if d[a] > EPSILON { b[a] } else if d[a] < -EPSILON { b[a + 3] } else { continue };
        let s = (point - from[a]) / d[a];
        let pb = from[b1] + s * d[b1];
        let pc = from[c1] + s * d[c1];
        if 0.0 < s && s < scale && b[b1] - EPSILON < pb && pb < b[b1 + 3] + EPSILON && b[c1] - EPSILON < pc && pc < b[c1 + 3] + EPSILON {
            scale = s;
            hit = true;
        }
    }
    hit.then(|| [from[0] + scale * d[0], from[1] + scale * d[1], from[2] + scale * d[2]])
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
    matches!(owner, SHAPES | VOXEL_SHAPE | COLLISION_CONTEXT | ENTITY_COLLISION_CONTEXT | BOOLEAN_OP | AABB)
        || (owner.starts_with("net/minecraft/world/level/block/") && matches!(name, "box" | "isFaceFull" | "isShapeFullBlock"))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let d = |i: usize| args.get(i).copied().unwrap_or_default().d();
    let (owner, name, desc) = crate::parts(key)?;
    let shape_of = |rt: &Rt, v: V| boxes(rt, v).unwrap_or_default();
    Some(match (owner, name) {
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

    #[test]
    fn clip_finds_the_face_a_ray_enters() {
        let b = [0., 0., 0., 1., 1., 1.];
        assert_eq!(clip(b, [-1., 0.5, 0.5], [2., 0.5, 0.5]), Some([0., 0.5, 0.5]));
        assert_eq!(clip(b, [0.5, 3., 0.5], [0.5, -3., 0.5]), Some([0.5, 1., 0.5]));
        assert_eq!(clip(b, [-1., 2., 0.5], [2., 2., 0.5]), None);
    }
}
