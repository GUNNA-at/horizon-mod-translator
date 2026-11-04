//! The Java API surface translated mods call, keyed by
//! `owner.name.descriptor` exactly as the class file names it. Each entry
//! is native Rust. Anything not here is counted in [`Rt::unmapped`] and
//! returns a zero value, so a mod runs with that one feature missing
//! instead of not running at all; `horizon-mod-translator coverage` lists
//! what to map next.
use crate::{Data, Rt, V};

/// `net/minecraft/core/Direction` ordinals, as vanilla orders them.
pub const DIRECTIONS: [[i64; 3]; 6] = [
    [0, -1, 0], // DOWN
    [0, 1, 0],  // UP
    [0, 0, -1], // NORTH
    [0, 0, 1],  // SOUTH
    [-1, 0, 0], // WEST
    [1, 0, 0],  // EAST
];
const OPPOSITE: [i64; 6] = [1, 0, 3, 2, 5, 4];
/// Direction ordinal → Axis ordinal (X 0, Y 1, Z 2).
const AXIS: [i32; 6] = [1, 1, 2, 2, 0, 0];

const BLOCK_POS: &str = "net/minecraft/core/BlockPos";
const MUTABLE_POS: &str = "net/minecraft/core/BlockPos$MutableBlockPos";

/// Minecraft's superclasses a missed call climbs past the mod's own (the API's classes aren't in
/// `rt.supers`): what a `MutableBlockPos` inherits answers as `BlockPos`'s, then `Vec3i`'s.
fn api_super(class: &str) -> Option<&'static str> {
    match class {
        MUTABLE_POS => Some(BLOCK_POS),
        BLOCK_POS => Some("net/minecraft/core/Vec3i"),
        _ => None,
    }
}

pub fn block_pos(rt: &mut Rt, x: i64, y: i64, z: i64) -> V {
    rt.alloc(BLOCK_POS, Data::Native("BlockPos", [x, y, z, 0]))
}

pub fn direction(rt: &mut Rt, ordinal: i64) -> V {
    crate::api_common::direction(rt, ordinal)
}

fn native(rt: &Rt, v: V) -> [i64; 4] {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => *h,
        _ => [0; 4],
    }
}

/// Calls the API method `key` (`owner.name.desc`). For instance methods
/// `args[0]` is the receiver. A key naming one of the mod's own classes (a
/// member it inherits from the API, as `javac` writes inherited calls) is
/// resolved up the class's superclasses, as the JVM resolves it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let before = rt.unmapped.get(key).copied().unwrap_or(0);
    let r = call_exact(rt, key, args);
    if rt.unmapped.get(key).copied().unwrap_or(0) == before {
        return r;
    }
    let Some((owner, rest)) = key.split_once('.') else { return r };
    for class in retry_classes(rt, owner, args) {
        let up = rt.static_key(&format!("{class}.{rest}"));
        let seen = rt.unmapped.get(up).copied().unwrap_or(0);
        let r2 = call_exact(rt, up, args);
        if rt.unmapped.get(up).copied().unwrap_or(0) == seen {
            // Resolved: the miss on the name the class file used doesn't count.
            uncount(rt, key);
            return r2;
        }
        uncount(rt, up);
    }
    trace_unmapped(rt, key, args);
    chained(rt, key, args).unwrap_or(r)
}

/// What an unmapped method returning its own class gives back, so a builder chain
/// (`Properties.of().strength(1f).noOcclusion()`) goes on past the link nothing maps: the receiver,
/// or a new object for a static factory. It stays a miss: the value is a stand-in, not vanilla's
/// answer (an unmapped `BlockPos.containing` read as a new, zero position).
fn chained(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    if !rest.ends_with(&format!(")L{owner};")) {
        return None;
    }
    let this = args.first().copied().unwrap_or_default();
    Some(if this.r() != 0 && rt.class_of(this) == owner { this } else { rt.new_object(owner) })
}

/// Where a missed call is asked next. First up from the named class, a translated subclass to its
/// API superclass. Then from the receiver's own class up, the way the JVM dispatches an interface
/// call: `Supplier.get` on a DeferredHolder answers as `DeferredHolder.get`, and
/// `CharSequence.length` on a String as `String.length`.
fn retry_classes(rt: &Rt, owner: &'static str, args: &[V]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let climb = |out: &mut Vec<&'static str>, mut c: &'static str| {
        while let Some(sup) = rt.supers.get(c).copied().or_else(|| api_super(c)) {
            if sup == owner || out.contains(&sup) {
                break;
            }
            out.push(sup);
            c = sup;
        }
    };
    climb(&mut out, owner);
    if let Some(&v @ V::R(n)) = args.first()
        && n != 0
    {
        let class = rt.class_of(v);
        if class != owner && !out.contains(&class) {
            out.push(class);
            climb(&mut out, class);
        }
        // A vanilla enum constant (`DyeColor.ordinal()`): what every enum has.
        if !matches!(rt.get_field(v, "$ordinal"), V::Void) && !out.contains(&"java/lang/Enum") {
            out.push("java/lang/Enum");
        }
    }
    out
}

fn uncount(rt: &mut Rt, key: &'static str) {
    if let Some(n) = rt.unmapped.get_mut(key) {
        *n -= 1;
        if *n == 0 {
            rt.unmapped.remove(key);
        }
    }
}

/// With `HORIZON_COMPAT_TRACE` set, each API method the mod calls that isn't mapped is printed
/// once, the first time it runs: what a mod needs next, from a real game.
/// A key on a JDK exception class (`java/lang/IllegalStateException.<init>...`).
fn exception_class(key: &str) -> bool {
    let owner = key.split('.').next().unwrap_or("");
    owner.starts_with("java/") && (owner.ends_with("Exception") || owner.ends_with("Error") || owner.ends_with("Throwable"))
}

fn trace_unmapped(rt: &Rt, key: &'static str, args: &[V]) {
    if Rt::tracing() && rt.unmapped.get(key) == Some(&1) {
        let on = match args.first() {
            Some(&v @ V::R(_)) => format!(" (on {})", rt.class_of(v)),
            _ => String::new(),
        };
        eprintln!("horizon-compat: unmapped {key}{on}");
    }
}

fn call_exact(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match key {
        // java.lang
        "java/lang/Object.<init>.()V" => V::Void,
        "java/lang/Math.abs.(I)I" => V::I(a(0).i().wrapping_abs()),
        "java/lang/Math.max.(II)I" => V::I(a(0).i().max(a(1).i())),
        "java/lang/Math.min.(II)I" => V::I(a(0).i().min(a(1).i())),
        "java/lang/Math.abs.(F)F" => V::F(a(0).f().abs()),
        "java/lang/Math.abs.(D)D" => V::D(a(0).d().abs()),
        "java/lang/Math.max.(FF)F" => V::F(a(0).f().max(a(1).f())),
        "java/lang/Math.min.(FF)F" => V::F(a(0).f().min(a(1).f())),
        "java/lang/Math.max.(DD)D" => V::D(a(0).d().max(a(1).d())),
        "java/lang/Math.min.(DD)D" => V::D(a(0).d().min(a(1).d())),
        "java/lang/Math.sqrt.(D)D" => V::D(a(0).d().sqrt()),
        "java/lang/Math.floor.(D)D" => V::D(a(0).d().floor()),
        "java/lang/Math.ceil.(D)D" => V::D(a(0).d().ceil()),
        "java/lang/String.equals.(Ljava/lang/Object;)Z" => {
            V::I((rt.str_of(a(0)).is_some() && rt.str_of(a(0)) == rt.str_of(a(1))) as i32)
        }
        "java/lang/String.length.()I" => V::I(rt.str_of(a(0)).map_or(0, |s| s.encode_utf16().count() as i32)),
        "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;" => {
            let o = rt.new_object("java/lang/Integer");
            rt.put_field(o, "value", a(0));
            o
        }
        // A collection's int is held raw (fastutil's `IntIterator.next()` boxes it in Java).
        "java/lang/Integer.intValue.()I" => match a(0) {
            V::R(_) => rt.get_field(a(0), "value"),
            raw => V::I(raw.i()),
        },
        "java/lang/Enum.ordinal.()I" => V::I(rt.get_field(a(0), "$ordinal").i()),

        // net.minecraft.core.BlockPos
        "net/minecraft/core/BlockPos.<init>.(III)V" | "net/minecraft/core/Vec3i.<init>.(III)V" => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Native("BlockPos", [a(1).i() as i64, a(2).i() as i64, a(3).i() as i64, 0]);
            }
            V::Void
        }
        // `new BlockPos(vec3i)`: the same place.
        "net/minecraft/core/BlockPos.<init>.(Lnet/minecraft/core/Vec3i;)V" => {
            let h = native(rt, a(1));
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Native("BlockPos", [h[0], h[1], h[2], 0]);
            }
            V::Void
        }
        "net/minecraft/core/BlockPos.getX.()I" | "net/minecraft/core/Vec3i.getX.()I" => V::I(native(rt, a(0))[0] as i32),
        "net/minecraft/core/BlockPos.getY.()I" | "net/minecraft/core/Vec3i.getY.()I" => V::I(native(rt, a(0))[1] as i32),
        "net/minecraft/core/BlockPos.getZ.()I" | "net/minecraft/core/Vec3i.getZ.()I" => V::I(native(rt, a(0))[2] as i32),
        // `Vec3i.get(Axis)`: `axis.choose(x, y, z)`.
        "net/minecraft/core/BlockPos.get.(Lnet/minecraft/core/Direction$Axis;)I" | "net/minecraft/core/Vec3i.get.(Lnet/minecraft/core/Direction$Axis;)I" => {
            let axis = native(rt, a(1))[0].clamp(0, 2) as usize;
            V::I(native(rt, a(0))[axis] as i32)
        }
        "net/minecraft/core/BlockPos.above.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 1, 0], 1),
        "net/minecraft/core/BlockPos.below.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, -1, 0], 1),
        "net/minecraft/core/BlockPos.north.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 0, -1], 1),
        "net/minecraft/core/BlockPos.south.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 0, 1], 1),
        "net/minecraft/core/BlockPos.west.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [-1, 0, 0], 1),
        "net/minecraft/core/BlockPos.east.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [1, 0, 0], 1),
        "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction;)Lnet/minecraft/core/BlockPos;" => {
            let d = DIRECTIONS[native(rt, a(1))[0].clamp(0, 5) as usize];
            offset(rt, a(0), d, 1)
        }
        "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction;I)Lnet/minecraft/core/BlockPos;" => {
            let d = DIRECTIONS[native(rt, a(1))[0].clamp(0, 5) as usize];
            offset(rt, a(0), d, a(2).i() as i64)
        }
        "net/minecraft/core/BlockPos.offset.(III)Lnet/minecraft/core/BlockPos;" => {
            offset(rt, a(0), [a(1).i() as i64, a(2).i() as i64, a(3).i() as i64], 1)
        }
        // The block a point is in: `Mth.floor` of each coordinate (a `Position` is a `Vec3` here).
        "net/minecraft/core/BlockPos.containing.(DDD)Lnet/minecraft/core/BlockPos;" => {
            let [x, y, z] = [a(0), a(1), a(2)].map(|c| c.d().floor() as i32 as i64);
            block_pos(rt, x, y, z)
        }
        "net/minecraft/core/BlockPos.containing.(Lnet/minecraft/core/Position;)Lnet/minecraft/core/BlockPos;" => {
            let [x, y, z] = ["x", "y", "z"].map(|c| rt.get_field(a(0), c).d().floor() as i32 as i64);
            block_pos(rt, x, y, z)
        }
        "net/minecraft/core/BlockPos.equals.(Ljava/lang/Object;)Z" | "net/minecraft/core/Vec3i.equals.(Ljava/lang/Object;)Z" => {
            V::I((native(rt, a(1))[3] == 0 && native(rt, a(0))[..3] == native(rt, a(1))[..3] && rt.heap.get(a(1).r() as usize).is_some_and(|o| matches!(o.data, Data::Native("BlockPos", _)))) as i32)
        }
        // `Vec3i.hashCode`: `(y + z * 31) * 31 + x`.
        "net/minecraft/core/BlockPos.hashCode.()I" | "net/minecraft/core/Vec3i.hashCode.()I" => {
            let p = native(rt, a(0));
            let (x, y, z) = (p[0] as i32, p[1] as i32, p[2] as i32);
            V::I(y.wrapping_add(z.wrapping_mul(31)).wrapping_mul(31).wrapping_add(x))
        }
        // Packed positions: x in 26 bits from bit 38, z in 26 from bit 12, y in the low 12.
        "net/minecraft/core/BlockPos.asLong.()J" => {
            let p = native(rt, a(0));
            V::J(pack_pos(p[0], p[1], p[2]))
        }
        "net/minecraft/core/BlockPos.asLong.(III)J" => V::J(pack_pos(a(0).i() as i64, a(1).i() as i64, a(2).i() as i64)),
        "net/minecraft/core/BlockPos.of.(J)Lnet/minecraft/core/BlockPos;" => {
            let l = a(0).j();
            block_pos(rt, l >> 38, (l << 52) >> 52, (l << 26) >> 38)
        }
        "net/minecraft/core/BlockPos.getX.(J)I" => V::I((a(0).j() >> 38) as i32),
        "net/minecraft/core/BlockPos.getY.(J)I" => V::I(((a(0).j() << 52) >> 52) as i32),
        "net/minecraft/core/BlockPos.getZ.(J)I" => V::I(((a(0).j() << 26) >> 38) as i32),
        "net/minecraft/core/BlockPos.immutable.()Lnet/minecraft/core/BlockPos;" => {
            let p = native(rt, a(0));
            block_pos(rt, p[0], p[1], p[2])
        }
        "net/minecraft/core/BlockPos.offset.(Lnet/minecraft/core/Vec3i;)Lnet/minecraft/core/BlockPos;" | "net/minecraft/core/BlockPos.subtract.(Lnet/minecraft/core/Vec3i;)Lnet/minecraft/core/BlockPos;" => {
            let d = native(rt, a(1));
            let n = if key.contains(".subtract.") { -1 } else { 1 };
            offset(rt, a(0), [d[0], d[1], d[2]], n)
        }
        "net/minecraft/core/BlockPos.multiply.(I)Lnet/minecraft/core/BlockPos;" => {
            let p = native(rt, a(0));
            let k = a(1).i() as i64;
            block_pos(rt, p[0] * k, p[1] * k, p[2] * k)
        }
        "net/minecraft/core/BlockPos.above.(I)Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 1, 0], a(1).i() as i64),
        "net/minecraft/core/BlockPos.below.(I)Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, -1, 0], a(1).i() as i64),
        "net/minecraft/core/BlockPos.north.(I)Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 0, -1], a(1).i() as i64),
        "net/minecraft/core/BlockPos.south.(I)Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 0, 1], a(1).i() as i64),
        "net/minecraft/core/BlockPos.west.(I)Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [-1, 0, 0], a(1).i() as i64),
        "net/minecraft/core/BlockPos.east.(I)Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [1, 0, 0], a(1).i() as i64),
        "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction$Axis;I)Lnet/minecraft/core/BlockPos;" => {
            let mut d = [0i64; 3];
            d[native(rt, a(1))[0].clamp(0, 2) as usize] = 1;
            offset(rt, a(0), d, a(2).i() as i64)
        }
        // Every position in a box, x fastest, then y, then z, as vanilla's iterator walks it: an
        // `Iterable` (a list) or a `Stream`. Vanilla hands out one mutable cursor; these are
        // separate positions, which a mod that calls `immutable()` can't tell apart.
        k if k.starts_with("net/minecraft/core/BlockPos.betweenClosed") => {
            let corners = match &k[k.find(".(").map_or(0, |i| i + 1)..] {
                d if d.starts_with("(IIIIII)") => [a(0).i(), a(1).i(), a(2).i(), a(3).i(), a(4).i(), a(5).i()],
                d if d.starts_with("(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)") => {
                    let (p, q) = (native(rt, a(0)), native(rt, a(1)));
                    [p[0].min(q[0]), p[1].min(q[1]), p[2].min(q[2]), p[0].max(q[0]), p[1].max(q[1]), p[2].max(q[2])].map(|c| c as i32)
                }
                d if d.starts_with("(Lnet/minecraft/world/phys/AABB;)") => {
                    let b = crate::shapes::aabb_of(rt, a(0));
                    b.map(|c| c.floor() as i32)
                }
                _ => {
                    *rt.unmapped.entry(key).or_insert(0) += 1;
                    return V::NULL;
                }
            };
            let [x0, y0, z0, x1, y1, z1] = corners;
            let (w, h, d) = ((x1 - x0 + 1).max(0) as i64, (y1 - y0 + 1).max(0) as i64, (z1 - z0 + 1).max(0) as i64);
            // A box bigger than a few chunks is a mod's mistake or a scan the game can't afford here.
            let n = (w * h * d).min(1 << 20);
            let mut out = Vec::with_capacity(n as usize);
            for i in 0..n {
                let (x, rest) = (i % w, i / w);
                let (y, z) = (rest % h, rest / h);
                out.push(block_pos(rt, x0 as i64 + x, y0 as i64 + y, z0 as i64 + z));
            }
            if k.contains("Stream") {
                rt.alloc("java/util/stream/Stream", Data::Stream(out))
            } else {
                crate::jutil::new_list(rt, out)
            }
        }
        "net/minecraft/core/Vec3i.distManhattan.(Lnet/minecraft/core/Vec3i;)I" | "net/minecraft/core/BlockPos.distManhattan.(Lnet/minecraft/core/Vec3i;)I" => {
            let (p, q) = (native(rt, a(0)), native(rt, a(1)));
            V::I(((p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs()) as i32)
        }
        "net/minecraft/core/Vec3i.distSqr.(Lnet/minecraft/core/Vec3i;)D" | "net/minecraft/core/BlockPos.distSqr.(Lnet/minecraft/core/Vec3i;)D" => {
            let (p, q) = (native(rt, a(0)), native(rt, a(1)));
            let d = |i: usize| (p[i] - q[i]) as f64;
            V::D(d(0) * d(0) + d(1) * d(1) + d(2) * d(2))
        }
        // `distSqr(pos) < distance²`.
        "net/minecraft/core/Vec3i.closerThan.(Lnet/minecraft/core/Vec3i;D)Z" | "net/minecraft/core/BlockPos.closerThan.(Lnet/minecraft/core/Vec3i;D)Z" => {
            let (p, q) = (native(rt, a(0)), native(rt, a(1)));
            let d = |i: usize| (p[i] - q[i]) as f64;
            V::I((d(0) * d(0) + d(1) * d(1) + d(2) * d(2) < a(2).d() * a(2).d()) as i32)
        }
        "net/minecraft/core/BlockPos.mutable.()Lnet/minecraft/core/BlockPos$MutableBlockPos;" => {
            let p = native(rt, a(0));
            rt.alloc(MUTABLE_POS, Data::Native("BlockPos", [p[0], p[1], p[2], 0]))
        }

        // net.minecraft.core.Direction
        "net/minecraft/core/Direction.getOpposite.()Lnet/minecraft/core/Direction;" => {
            let o = OPPOSITE[native(rt, a(0))[0].clamp(0, 5) as usize];
            direction(rt, o)
        }
        "net/minecraft/core/Direction.ordinal.()I" | "net/minecraft/core/Direction.get3DDataValue.()I" => {
            V::I(native(rt, a(0))[0] as i32)
        }
        // 2D values: SOUTH 0, WEST 1, NORTH 2, EAST 3; up and down -1.
        "net/minecraft/core/Direction.get2DDataValue.()I" => V::I([-1, -1, 2, 0, 1, 3][native(rt, a(0))[0].clamp(0, 5) as usize]),
        // `(data2d & 3) * 90`: south 0, west 90, north 180, east 270 (up and down: 270).
        "net/minecraft/core/Direction.toYRot.()F" => V::F((([-1i32, -1, 2, 0, 1, 3][native(rt, a(0))[0].clamp(0, 5) as usize] & 3) * 90) as f32),
        "net/minecraft/core/Direction.from3DDataValue.(I)Lnet/minecraft/core/Direction;" => direction(rt, (a(0).i() % 6).abs() as i64),
        "net/minecraft/core/Direction.from2DDataValue.(I)Lnet/minecraft/core/Direction;" => direction(rt, [3, 4, 2, 5][(a(0).i() % 4).unsigned_abs() as usize]),
        // Around the vertical axis: north, east, south, west.
        "net/minecraft/core/Direction.getClockWise.()Lnet/minecraft/core/Direction;" | "net/minecraft/core/Direction.getCounterClockWise.()Lnet/minecraft/core/Direction;" => {
            let o = native(rt, a(0))[0];
            let ring = [2, 5, 3, 4];
            match ring.iter().position(|&r| r == o) {
                Some(i) => {
                    let step = if key.contains("getClockWise") { 1 } else { 3 };
                    direction(rt, ring[(i + step) % 4])
                }
                None => a(0),
            }
        }
        // About any axis (`getClockWise(Axis)`), as vanilla's tables: a direction along the axis stays.
        "net/minecraft/core/Direction.getClockWise.(Lnet/minecraft/core/Direction$Axis;)Lnet/minecraft/core/Direction;"
        | "net/minecraft/core/Direction.getCounterClockWise.(Lnet/minecraft/core/Direction$Axis;)Lnet/minecraft/core/Direction;" => {
            // By ordinal (down, up, north, south, west, east), for X, Y and Z; -1 keeps the direction.
            const CLOCKWISE: [[i64; 6]; 3] = [[3, 2, 0, 1, -1, -1], [-1, -1, 5, 4, 2, 3], [4, 5, -1, -1, 1, 0]];
            const COUNTER: [[i64; 6]; 3] = [[2, 3, 1, 0, -1, -1], [-1, -1, 4, 5, 3, 2], [5, 4, -1, -1, 0, 1]];
            let o = native(rt, a(0))[0].clamp(0, 5) as usize;
            let ax = native(rt, a(1))[0].clamp(0, 2) as usize;
            let to = if key.contains("getClockWise") { CLOCKWISE } else { COUNTER }[ax][o];
            if to < 0 { a(0) } else { direction(rt, to) }
        }
        "net/minecraft/core/Direction.getStepX.()I" => V::I(DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize][0] as i32),
        "net/minecraft/core/Direction.getStepY.()I" => V::I(DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize][1] as i32),
        "net/minecraft/core/Direction.getStepZ.()I" => V::I(DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize][2] as i32),
        "net/minecraft/core/Direction.getAxis.()Lnet/minecraft/core/Direction$Axis;" => {
            let ax = AXIS[native(rt, a(0))[0].clamp(0, 5) as usize];
            crate::api_common::axis(rt, ax as i64)
        }
        // Caller-sensitive code: the translator records the calling class before such calls.
        "java/lang/StackWalker.getInstance.()Ljava/lang/StackWalker;" | "java/lang/StackWalker.getInstance.(Ljava/lang/StackWalker$Option;)Ljava/lang/StackWalker;" => {
            rt.new_object("java/lang/StackWalker")
        }
        "java/lang/StackWalker.getCallerClass.()Ljava/lang/Class;" => {
            let c = rt.caller_class;
            rt.alloc("java/lang/Class", Data::Str(c.into()))
        }
        "java/lang/Class.getName.()Ljava/lang/String;" | "java/lang/Class.getTypeName.()Ljava/lang/String;" | "java/lang/Class.getPackageName.()Ljava/lang/String;" | "java/lang/Class.getSimpleName.()Ljava/lang/String;" => {
            let n = rt.str_of(a(0)).unwrap_or("").replace('/', ".");
            let out = if key.contains("getPackageName") {
                n.rsplit_once('.').map_or(String::new(), |(p, _)| p.to_owned())
            } else if key.contains("getSimpleName") {
                n.rsplit(['.', '$']).next().unwrap_or("").to_owned()
            } else {
                n
            };
            rt.string(&out)
        }
        k if exception_class(k) && k.contains(".<init>.(Ljava/lang/String;") => {
            rt.put_field(a(0), "message", a(1));
            V::Void
        }
        k if exception_class(k) && k.contains(".<init>.") => V::Void,
        k if exception_class(k) && (k.contains(".getMessage.") || k.contains(".getLocalizedMessage.")) => rt.get_field(a(0), "message"),
        // `array.clone()` (an enum's `values()`): a shallow copy.
        k if k.starts_with('[') && k.contains(".clone.") => {
            let this = a(0);
            match rt.heap.get(this.r() as usize).map(|o| (o.class, o.data.clone())) {
                Some((class, d @ Data::Array(_))) => rt.alloc(class, d),
                _ => V::NULL,
            }
        }
        _ => match mutable_pos(rt, key, args)
            .or_else(|| crate::sounds::call(rt, key, args))
            .or_else(|| crate::particles::call(rt, key, args))
            .or_else(|| crate::network::call(rt, key, args))
            .or_else(|| crate::fluids::call(rt, key, args))
            .or_else(|| crate::tanks::call_key(rt, key, args))
            .or_else(|| crate::codecs::call(rt, key, args))
            .or_else(|| crate::recipes::call(rt, key, args))
            .or_else(|| crate::handlers::call_key(rt, key, args))
            .or_else(|| crate::components::call_key(rt, key, args))
            .or_else(|| crate::capabilities::call_key(rt, key, args))
            .or_else(|| crate::mod_entities::call_key(rt, key, args))
            .or_else(|| crate::entities::call_key(rt, key, args))
            .or_else(|| crate::creative::call(rt, key, args))
            .or_else(|| crate::functions::call(rt, key, args))
            .or_else(|| crate::rotation::call(rt, key, args))
            .or_else(|| crate::blocks::call(rt, key, args))
            .or_else(|| crate::shapes::call(rt, key, args))
            .or_else(|| crate::neo::call(rt, key, args))
            .or_else(|| crate::jlang::call(rt, key, args))
            .or_else(|| crate::jutil::call(rt, key, args))
            .or_else(|| crate::tags::call(rt, key, args))
            .or_else(|| crate::quads::call(rt, key, args))
            .or_else(|| crate::render::call(rt, key, args))
            .or_else(|| world(rt, key, args))
            .or_else(|| crate::palette::call(rt, key, args))
            .or_else(|| crate::records::call(rt, key, args))
        {
            Some(v) => v,
            None => {
                *rt.unmapped.entry(key).or_insert(0) += 1;
                V::Void
            }
        },
    }
}

const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const BLOCK: &str = "net/minecraft/world/level/block/Block";
const STATE_DEFINITION: &str = "net/minecraft/world/level/block/state/StateDefinition";
const DATA_STORAGE: &str = "net/minecraft/world/level/storage/DimensionDataStorage";
const UPDATE_SHAPE: &str = "updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const UPDATE_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const CAN_SURVIVE: &str = "canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
const CAN_SURVIVE_KEY: &str =
    "net/minecraft/world/level/block/state/BlockBehaviour.canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
const SAVED_DATA: &str = "net/minecraft/world/level/saveddata/SavedData";
const LEVEL_DATA: &str = "net/minecraft/world/level/storage/LevelData";
const LEVEL_CHUNK: &str = "net/minecraft/world/level/chunk/LevelChunk";
const SAVED_DATA_FACTORY: &str = "net/minecraft/world/level/saveddata/SavedData$Factory";
/// 1.21.1's `RenderShape`, in declaration order (ordinals).
pub const RENDER_SHAPES: [&str; 3] = ["INVISIBLE", "ENTITYBLOCK_ANIMATED", "MODEL"];

/// A block state value (its text, `minecraft:stone` or `create:shaft[axis=y]`).
pub fn state(rt: &mut Rt, text: &str) -> V {
    crate::api_common::state(rt, text)
}

const IS_STICKY: &str = "isStickyBlock.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const IS_STICKY_KEY: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension.isStickyBlock.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const IS_SLIME: &str = "isSlimeBlock.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const IS_SLIME_KEY: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension.isSlimeBlock.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const CAN_STICK_TO: &str = "canStickTo.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;)Z";
const CAN_STICK_TO_KEY: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension.canStickTo.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/state/BlockState;)Z";

/// NeoForge's sticky blocks (`IBlockExtension`): a mod block's own `isSlimeBlock`/`isStickyBlock`/
/// `canStickTo` when it overrides them; otherwise slime and honey stick, but not to each other.
/// Create's contraptions take what sticks to their blocks along.
fn sticky(rt: &mut Rt, how: &str, state: V, other: V) -> bool {
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    let (short, key) = match how {
        "isSlimeBlock" => (IS_SLIME, IS_SLIME_KEY),
        "isStickyBlock" => (IS_STICKY, IS_STICKY_KEY),
        _ => (CAN_STICK_TO, CAN_STICK_TO_KEY),
    };
    if let Some((b, class)) = crate::glue::mod_block(rt, &text)
        && rt.overrides.is_some_and(|o| o(class, short))
        && let Some(vc) = rt.vcall
    {
        let args: &[V] = if how == "canStickTo" { &[b, state, other] } else { &[b, state] };
        return vc(rt, key, short, args).i() != 0;
    }
    let name = |rt: &Rt, s: V| crate::host::split_state(rt.str_of(s).unwrap_or("minecraft:air")).0.to_owned();
    let mine = name(rt, state);
    match how {
        "isSlimeBlock" => mine == "minecraft:slime_block",
        "isStickyBlock" => matches!(mine.as_str(), "minecraft:slime_block" | "minecraft:honey_block"),
        _ => {
            let theirs = name(rt, other);
            match (mine.as_str(), theirs.as_str()) {
                ("minecraft:honey_block", "minecraft:slime_block") | ("minecraft:slime_block", "minecraft:honey_block") => false,
                _ => sticky(rt, "isStickyBlock", state, V::NULL) || sticky(rt, "isStickyBlock", other, V::NULL),
            }
        }
    }
}

const GET_RENDER_SHAPE: &str = "getRenderShape.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/RenderShape;";
const GET_RENDER_SHAPE_KEY: &str =
    "net/minecraft/world/level/block/state/BlockBehaviour.getRenderShape.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/RenderShape;";

/// `state.getRenderShape()`, as the version the mod was built against answers it: a mod block's own
/// `getRenderShape`, or the vanilla class it extends (`BaseEntityBlock`: invisible); a vanilla
/// block's by its class (`vanilla_shapes.rs`: chests and beds animated, signs and air invisible).
/// Everything else is a model.
fn render_shape(rt: &mut Rt, state: V, text: &str) -> V {
    let (name, _) = crate::host::split_state(text);
    let ordinal = match crate::glue::mod_block(rt, text) {
        Some((b, class)) => {
            if rt.overrides.is_some_and(|o| o(class, GET_RENDER_SHAPE))
                && let Some(vc) = rt.vcall
            {
                return vc(rt, GET_RENDER_SHAPE_KEY, GET_RENDER_SHAPE, &[b, state]);
            }
            let base = crate::rotation::vanilla_base(rt, class);
            crate::vanilla_shapes::CLASSES.binary_search_by(|(c, _)| (*c).cmp(base)).map_or(2, |i| crate::vanilla_shapes::CLASSES[i].1)
        }
        None => {
            let path = name.strip_prefix("minecraft:").unwrap_or(name);
            crate::vanilla_shapes::BLOCKS.binary_search_by(|(n, _)| (*n).cmp(path)).map_or(2, |i| crate::vanilla_shapes::BLOCKS[i].1)
        }
    };
    let ordinal = usize::from(ordinal).min(2);
    api_enum(rt, "net/minecraft/world/level/block/RenderShape", RENDER_SHAPES[ordinal], ordinal as i32)
}

/// A vanilla enum constant: one object per constant (Java compares them by identity), with the
/// `$name` and `$ordinal` that `name()`, `ordinal()` and `compareTo` read.
pub fn api_enum(rt: &mut Rt, class: &'static str, name: &'static str, ordinal: i32) -> V {
    rt.intern(format!("enum/{class}.{name}"), |rt| {
        let o = rt.new_object(class);
        let n = rt.string(name);
        rt.put_field(o, "$name", n);
        rt.put_field(o, "$ordinal", V::I(ordinal));
        o
    })
}

const GET_SHAPE: &str = "getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";

/// `state.getCollisionShape(level, pos)` and `state.getShape(level, pos)`, args [state, level, pos,
/// context]: for a collision, the game's own boxes when `state` is the block at `pos` (vanilla's
/// and the content mods' shapes); else the mod block's own `getShape`; else a full block (air, none).
fn state_shape(rt: &mut Rt, text: &str, collision: bool, args: [V; 4]) -> V {
    let at = pos_of(rt, args[2]);
    if collision
        && let Some(h) = rt.host()
        && h.block_state(at) == text
        && let Some(boxes) = h.collision_boxes(at)
    {
        return crate::shapes::shape(rt, &boxes);
    }
    let (b, _) = crate::host::split_state(text);
    if matches!(b, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") {
        return crate::shapes::shape(rt, &[]);
    }
    if let Some((blk, _)) = crate::glue::mod_block(rt, text)
        && let Some(vc) = rt.vcall
    {
        let shape = vc(rt, GET_SHAPE_KEY, GET_SHAPE, &[blk, args[0], args[1], args[2], args[3]]);
        if rt.thrown.is_none() && crate::shapes::boxes(rt, shape).is_some() {
            return shape;
        }
        rt.report_thrown("getShape");
    }
    crate::shapes::shape(rt, &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]])
}

/// A block value: the mod's own Block object when it registered one by that name.
pub fn block(rt: &mut Rt, name: &str) -> V {
    if let Some(v) = rt.registered.get(&("block".to_owned(), name.to_owned())) {
        return *v;
    }
    // Interned, so `state.getBlock() == Blocks.STONE` holds as in Java.
    let v = rt.alloc(BLOCK, Data::Str(name.to_owned()));
    rt.registered.insert(("block".to_owned(), name.to_owned()), v);
    v
}

/// The name of a block value (vanilla or the mod's).
pub(crate) fn block_name(rt: &Rt, b: V) -> String {
    rt.str_of(b).map(str::to_owned).or_else(|| rt.registered_name(b, "block").map(str::to_owned)).unwrap_or_default()
}

/// `BlockState.updateShape(direction, neighbour, level, pos, neighbourPos)` (`args`, the state
/// first): the mod block's own `updateShape`; a block without one keeps its state.
fn update_shape(rt: &mut Rt, args: [V; 6]) -> V {
    let text = rt.str_of(args[0]).unwrap_or("").to_owned();
    match crate::glue::mod_block(rt, &text) {
        Some((b, class)) if rt.overrides.is_some_and(|o| o(class, UPDATE_SHAPE)) => match rt.vcall {
            Some(vc) => vc(rt, UPDATE_SHAPE_KEY, UPDATE_SHAPE, &[b, args[0], args[1], args[2], args[3], args[4], args[5]]),
            None => args[0],
        },
        _ => args[0],
    }
}

pub(crate) fn pos_of(rt: &Rt, v: V) -> [i32; 3] {
    let h = native(rt, v);
    [h[0] as i32, h[1] as i32, h[2] as i32]
}

fn qualified(rt: &Rt, name: &str) -> String {
    if name.contains(':') { name.to_owned() } else { format!("{}:{name}", rt.namespace) }
}

/// Whether `owner` is one of the level's interfaces or classes (`Level`, `LevelAccessor`,
/// `BlockGetter`...): what reads and writes the game's blocks.
pub(crate) fn is_level(owner: &str) -> bool {
    owner.starts_with("net/minecraft/")
        && (owner.ends_with("Level") || owner.ends_with("LevelAccessor") || owner.ends_with("LevelReader") || owner.ends_with("BlockGetter") || owner.ends_with("LevelWriter"))
}

/// Registration, the level and block states: the API that reaches the game.
fn world(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    let level = is_level(owner);
    Some(match (owner, name) {
        // NeoForge registration
        // `createDataComponents(registryKey, modid)`, or `(modid)` alone in older NeoForge.
        ("net/neoforged/neoforge/registries/DeferredRegister", "create" | "createDataComponents") => {
            let keyed = name == "create" || desc.starts_with("(Lnet/minecraft/resources/ResourceKey;");
            let registry = if keyed { rt.str_of(a(0)).unwrap_or("unknown").to_owned() } else { "data_component_type".to_owned() };
            let ns = rt.str_of(if keyed { a(1) } else { a(0) }).unwrap_or("").to_owned();
            if rt.namespace.is_empty() {
                rt.namespace = ns;
            }
            let r = rt.new_object(owner);
            let reg = rt.string(&registry);
            rt.put_field(r, "$registry", reg);
            r
        }
        ("net/neoforged/neoforge/registries/DeferredRegister", "createBlocks" | "createItems") => {
            let ns = rt.str_of(a(0)).unwrap_or("").to_owned();
            if rt.namespace.is_empty() {
                rt.namespace = ns;
            }
            let kind = if name == "createBlocks" { "block" } else { "item" };
            let r = rt.new_object(owner);
            let reg = rt.string(kind);
            rt.put_field(r, "$registry", reg);
            r
        }
        (o, "register") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") && desc.starts_with("(Ljava/lang/String;") => {
            let registry = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("unknown").to_owned();
            let id = qualified(rt, rt.str_of(a(1)).unwrap_or(""));
            rt.suppliers.push((registry.clone(), id.clone(), a(2)));
            holder(rt, &registry, &id)
        }
        (o, "registerBlock" | "registerItem") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let registry = if name == "registerBlock" { "block" } else { "item" };
            let id = qualified(rt, rt.str_of(a(1)).unwrap_or(""));
            let props = if desc.contains("Properties;)") || args.len() > 3 { a(3) } else { V::NULL };
            let apply = rt.lambda("compat:apply", vec![a(2), props]);
            rt.suppliers.push((registry.into(), id.clone(), apply));
            holder(rt, registry, &id)
        }
        // NeoForge's `DataComponents.registerComponentType(name, builder -> ...)`: built now, as the
        // type is a plain object the mod configures, and registered under its name.
        (o, "registerComponentType") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let registry = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("data_component_type").to_owned();
            let id = qualified(rt, rt.str_of(a(1)).unwrap_or(""));
            let ty = crate::components::build_with(rt, a(2));
            rt.registered.insert((registry.clone(), id.clone()), ty);
            holder(rt, &registry, &id)
        }
        (o, "register") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => V::Void, // (IEventBus)
        (o, "get" | "value") if o.starts_with("net/neoforged/neoforge/registries/Deferred") => {
            let id = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned();
            if rt.registered.keys().all(|(_, n)| *n != id) {
                rt.resolve_registrations();
            }
            rt.registered.iter().find(|((_, n), _)| *n == id).map(|(_, v)| *v).unwrap_or(V::NULL)
        }

        // The level
        (_, "getBlockState") if level => {
            let p = pos_of(rt, a(1));
            let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
            state(rt, &text)
        }
        (_, "setBlock" | "setBlockAndUpdate") if level => {
            let p = pos_of(rt, a(1));
            let text = rt.str_of(a(2)).unwrap_or("minecraft:air").to_owned();
            V::I(rt.host_mut().is_some_and(|h| h.set_block_state(p, &text)) as i32)
        }
        (_, "removeBlock" | "destroyBlock") if level => {
            let p = pos_of(rt, a(1));
            V::I(rt.host_mut().is_some_and(|h| h.set_block_state(p, "minecraft:air")) as i32)
        }
        (_, "isClientSide") if level => V::I(rt.host().is_some_and(|h| !h.is_server()) as i32),
        (_, "isLoaded" | "hasChunkAt" | "isAreaLoaded" | "hasChunksAt" | "isInWorldBounds") if level => V::I(1),
        (_, "getGameTime") if level => V::J(rt.host().map_or(0, |h| h.game_time())),
        // The level's server services. Ticking is never frozen or stepped here (`/tick` isn't modelled
        // for mods), and the game syncs changed blocks and block entities itself.
        (_, "tickRateManager") if level => rt.intern("compat:tick-rate".into(), |rt| rt.new_object("net/minecraft/server/ServerTickRateManager")),
        ("net/minecraft/world/TickRateManager" | "net/minecraft/server/ServerTickRateManager", n) => match n {
            "isFrozen" | "isSteppingForward" | "isSprinting" => V::I(0),
            "runsNormally" => V::I(1),
            "tickrate" => V::F(20.0),
            "nanosecondsPerTick" => V::J(50_000_000),
            "millisecondsPerTick" => V::F(50.0),
            _ => return None,
        },
        (_, "getChunkSource") if level => rt.intern("compat:chunk-source".into(), |rt| rt.new_object("net/minecraft/server/level/ServerChunkCache")),
        // Players see the block entity's new data (Create's `sendData`).
        ("net/minecraft/server/level/ServerChunkCache" | "net/minecraft/world/level/chunk/ChunkSource", "blockChanged") => {
            let p = pos_of(rt, a(1));
            if let Some(h) = rt.host_mut() {
                h.block_changed(p);
            }
            V::Void
        }
        (_, "sendBlockUpdated") if level => {
            let p = pos_of(rt, a(1));
            if let Some(h) = rt.host_mut() {
                h.block_changed(p);
            }
            V::Void
        }
        (_, "blockUpdated" | "updateNeighbourForOutputSignal" | "updateNeighborsAt" | "blockEntityChanged") if level => V::Void,
        (_, "sendParticles") if level => crate::particles::send_particles(rt, desc, args),
        (_, "addParticle" | "addAlwaysVisibleParticle") if level => crate::particles::level_particle(rt, name, desc, args),
        (_, "playSound" | "playSeededSound" | "playLocalSound") if level => crate::sounds::level_sound(rt, name, desc, args),
        (_, "levelEvent" | "gameEvent") if level => V::Void,
        (_, "dimensionType") if level => rt.intern("compat:dimension-type".into(), |rt| rt.new_object("net/minecraft/world/level/dimension/DimensionType")),
        // Its dimension type's holder: vanilla's three dimensions have types of their own names.
        (_, "dimensionTypeRegistration") if level => {
            let id = rt.host().map(|h| h.dimension()).unwrap_or_else(|| "minecraft:overworld".to_owned());
            let ty = call(rt, "net/minecraft/world/level/Level.dimensionType.()Lnet/minecraft/world/level/dimension/DimensionType;", &[a(0)]);
            rt.intern(format!("compat:dimension-type-holder/{id}"), |rt| crate::neo::registry_holder(rt, "dimension_type", &id, ty))
        }
        // A mod's own level (Create's `ContraptionWorld`, a wrapper of the real one) built on
        // `Level`'s: which side it is on, as mods read it (`level.isClientSide`).
        ("net/minecraft/world/level/Level", "<init>") => {
            let client = if desc.contains(";ZZJI)") { a(6) } else { V::I(crate::neo::is_client(rt) as i32) };
            rt.put_field(a(0), "isClientSide", client);
            V::Void
        }
        // The level's key (`Level.OVERWORLD`): `dimension#minecraft:overworld`.
        (_, "dimension") if level && desc == "()Lnet/minecraft/resources/ResourceKey;" => {
            let d = rt.host().map(|h| h.dimension()).unwrap_or_else(|| "minecraft:overworld".to_owned());
            rt.string(&format!("dimension#{d}"))
        }
        (_, "isDebug") if level => V::I(0),
        (_, "getLevelData") if level => rt.intern("compat:level-data".into(), |rt| rt.new_object(LEVEL_DATA)),
        (LEVEL_DATA, n) => match n {
            "getGameTime" | "getDayTime" => V::J(rt.host().map_or(0, |h| h.game_time())),
            "isRaining" | "isThundering" | "isHardcore" => V::I(0),
            _ => return None,
        },
        // The chunk holding a position: its level answers for it.
        (_, "getChunkAt") if level => {
            let p = pos_of(rt, a(1));
            let c = rt.new_object(LEVEL_CHUNK);
            rt.put_field(c, "$level", a(0));
            rt.put_field(c, "$x", V::I(p[0] >> 4));
            rt.put_field(c, "$z", V::I(p[2] >> 4));
            c
        }
        (LEVEL_CHUNK, "getLevel") => rt.get_field(a(0), "$level"),
        (LEVEL_CHUNK, "getBlockState") => {
            let level = rt.get_field(a(0), "$level");
            call(rt, "net/minecraft/world/level/Level.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;", &[level, a(1)])
        }
        // What `setBlock` would have done after the change (Create sets blocks quietly, then calls it):
        // the game already updated the neighbours; clients are told when the flags say so.
        (_, "markAndNotifyBlock") if level => {
            if a(5).i() & 2 != 0 {
                let p = pos_of(rt, a(1));
                if let Some(h) = rt.host_mut() {
                    h.block_changed(p);
                }
            }
            V::Void
        }
        (_, "removeBlockEntity") if level => {
            let p = pos_of(rt, a(1));
            crate::glue::block_entity_removed(rt, p);
            V::Void
        }
        // The overworld's type (`DimensionTypes.OVERWORLD`).
        ("net/minecraft/world/level/dimension/DimensionType", n) => match n {
            "minY" => V::I(-64),
            "height" | "logicalHeight" => V::I(384),
            "ultraWarm" | "hasCeiling" | "piglinSafe" | "respawnAnchorWorks" | "hasFixedTime" => V::I(0),
            "natural" | "hasSkyLight" | "bedWorks" | "hasRaids" => V::I(1),
            "coordinateScale" => V::D(1.0),
            "ambientLight" => V::F(0.0),
            _ => return None,
        },
        (_, "getMinY" | "getMinBuildHeight") if level => V::I(-64),
        (_, "getMaxY") if level => V::I(319),
        (_, "getMaxBuildHeight") if level => V::I(320),
        (_, "getHeight") if level && desc == "()I" => V::I(384),
        (_, "getSeaLevel") if level => V::I(63),
        (_, "isOutsideBuildHeight") if level => {
            let y = if desc.starts_with("(I)") { a(1).i() } else { pos_of(rt, a(1))[1] };
            V::I(!(-64..320).contains(&y) as i32)
        }
        // A client level has no server (`ClientLevel.getServer` is null).
        (_, "getServer") if level && rt.get_field(a(0), "isClientSide").i() != 0 => V::NULL,
        (_, "getServer") if level => rt.intern("compat:server".into(), |rt| rt.new_object("net/minecraft/server/MinecraftServer")),
        // Saved data (`DimensionDataStorage`): one object per name, made by its factory's
        // constructor the first time it's asked for. Kept in memory: not yet written to the world's
        // `data/` folder, so a mod's networks (Create's trains) start over with the game.
        (_, "getDataStorage") if level => rt.intern("compat:data-storage".into(), |rt| rt.new_object(DATA_STORAGE)),
        (SAVED_DATA_FACTORY, "<init>") => {
            rt.put_field(a(0), "$constructor", a(1));
            rt.put_field(a(0), "$deserializer", a(2));
            V::Void
        }
        (DATA_STORAGE, n @ ("computeIfAbsent" | "get")) => {
            let key = format!("saved-data/{}", rt.str_of(a(2)).unwrap_or_default());
            match rt.interned.get(&key) {
                Some(d) => *d,
                None if n == "get" => V::NULL,
                None => {
                    let make = rt.get_field(a(1), "$constructor");
                    let d = rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", make, &[]);
                    if d.r() != 0 {
                        rt.interned.insert(key, d);
                    }
                    d
                }
            }
        }
        (SAVED_DATA, "setDirty") => V::Void,
        (SAVED_DATA, "isDirty") => V::I(0),
        (SAVED_DATA, "<init>") => V::Void,
        ("net/minecraft/server/MinecraftServer" | "net/minecraft/server/dedicated/DedicatedServer", n) => match n {
            "getTickCount" => V::I(rt.host().map_or(0, |h| h.game_time()) as i32),
            // One level is modelled: the one the hook runs in.
            "overworld" | "getLevel" => crate::glue::level(rt),
            "isDedicatedServer" | "isPublished" | "isHardcore" => V::I(0),
            "isSameThread" => V::I(1),
            _ => return None,
        },

        // Block states
        (STATE | "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase", n) => {
            let text = rt.str_of(a(0)).unwrap_or("minecraft:air").to_owned();
            let (b, _) = crate::host::split_state(&text);
            match n {
                "getBlock" => block(rt, &b.to_owned()),
                "isAir" => V::I(matches!(b, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") as i32),
                "hasBlockEntity" => V::I(crate::glue::block_entity_id(rt, b).is_some() as i32),
                "is" if desc.starts_with("(Lnet/minecraft/world/level/block/Block;") => V::I((block_name(rt, a(1)) == b) as i32),
                // A block tag, as the game's loaded tags hold it (`BlockTags.LEAVES`, a mod's own).
                "is" if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
                    let registry = rt.str_of(rt.get_field(a(1), "$registry")).unwrap_or("").to_owned();
                    let tag = rt.str_of(rt.get_field(a(1), "$location")).unwrap_or("").to_owned();
                    let b = b.to_owned();
                    V::I((registry.trim_start_matches("minecraft:") == "block" && rt.host().is_some_and(|h| h.block_in_tag(&b, &tag))) as i32)
                }
                "equals" => V::I((rt.str_of(a(1)) == Some(text.as_str())) as i32),
                // What the block's properties say for the state (`BlockStateBase` keeps them).
                "getPistonPushReaction" => crate::blocks::push_reaction(rt, &text),
                // Turned or mirrored as the block turns its states (NeoForge's `rotate(level, pos, rotation)` too).
                "rotate" if desc.ends_with("Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/world/level/block/state/BlockState;") => {
                    crate::rotation::turn_state(rt, a(0), args.last().copied().unwrap_or_default(), false)
                }
                "mirror" if desc == "(Lnet/minecraft/world/level/block/Mirror;)Lnet/minecraft/world/level/block/state/BlockState;" => crate::rotation::turn_state(rt, a(0), a(1), true),
                // The game updates the shapes around a block it sets.
                "updateIndirectNeighbourShapes" | "updateNeighbourShapes" => V::Void,
                "getRenderShape" if desc == "()Lnet/minecraft/world/level/block/RenderShape;" => render_shape(rt, a(0), &text),
                "isSlimeBlock" | "isStickyBlock" if desc == "()Z" => V::I(sticky(rt, n, a(0), V::NULL) as i32),
                "canStickTo" if desc == "(Lnet/minecraft/world/level/block/state/BlockState;)Z" => V::I(sticky(rt, n, a(0), a(1)) as i32),
                "getDestroySpeed" => V::F(crate::blocks::state_facts(rt, &text).hardness),
                "canBeReplaced" if desc == "()Z" => V::I(crate::blocks::state_facts(rt, &text).replaceable as i32),
                "updateShape" if desc.starts_with("(Lnet/minecraft/core/Direction;") => update_shape(rt, [a(0), a(1), a(2), a(3), a(4), a(5)]),
                "getCollisionShape" | "getShape" if desc.starts_with("(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;") => {
                    let ctx = if desc.contains("CollisionContext;)") { a(3) } else { crate::shapes::empty_context(rt) };
                    state_shape(rt, &text, n == "getCollisionShape", [a(0), a(1), a(2), ctx])
                }
                // The mod block's own `canSurvive`; vanilla's default answers true.
                "canSurvive" => match crate::glue::mod_block(rt, &text) {
                    Some((blk, class)) if rt.overrides.is_some_and(|o| o(class, CAN_SURVIVE)) => match rt.vcall {
                        Some(vc) => vc(rt, CAN_SURVIVE_KEY, CAN_SURVIVE, &[blk, a(0), a(1), a(2)]),
                        None => V::I(1),
                    },
                    _ => V::I(1),
                },
                _ => return crate::api_common::call(rt, owner, name, desc, args),
            }
        }
        // Vanilla's order: west, east, north, south, down, up; each neighbour as the level has it.
        (BLOCK, "updateFromNeighbourShapes") => {
            let (mut state, level, pos) = (a(0), a(1), a(2));
            let p = pos_of(rt, pos);
            for d in [4, 5, 2, 3, 0, 1] {
                if state.r() == 0 {
                    break;
                }
                let step = DIRECTIONS[d];
                let np = block_pos(rt, p[0] as i64 + step[0], p[1] as i64 + step[1], p[2] as i64 + step[2]);
                let neighbour = call(rt, "net/minecraft/world/level/LevelAccessor.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;", &[level, np]);
                let dir = direction(rt, d as i64);
                state = update_shape(rt, [state, dir, neighbour, level, pos, np]);
            }
            state
        }
        (BLOCK, "defaultBlockState") | (_, "defaultBlockState") => {
            let n = block_name(rt, a(0));
            state(rt, &n)
        }
        // A block's states, as far as the runtime knows them: its default one.
        (_, "getStateDefinition") if desc == "()Lnet/minecraft/world/level/block/state/StateDefinition;" => {
            let b = a(0);
            rt.intern(format!("state-definition/{}", b.r()), |rt| {
                let d = rt.new_object(STATE_DEFINITION);
                rt.put_field(d, "$block", b);
                d
            })
        }
        (STATE_DEFINITION, n) => {
            let b = rt.get_field(a(0), "$block");
            match n {
                "getOwner" => b,
                "any" => {
                    let name = block_name(rt, b);
                    state(rt, &name)
                }
                "getPossibleStates" => {
                    let name = block_name(rt, b);
                    let s = state(rt, &name);
                    crate::jutil::new_list(rt, vec![s])
                }
                _ => return crate::api_common::call(rt, owner, name, desc, args),
            }
        }
        (_, "getDescriptionId") if is_block_class(owner) && desc == "()Ljava/lang/String;" => {
            let id = description_id(&block_name(rt, a(0)));
            rt.string(&id)
        }
        (_, "getName") if is_block_class(owner) && desc == "()Lnet/minecraft/network/chat/MutableComponent;" => {
            let id = description_id(&block_name(rt, a(0)));
            let key = rt.string(&id);
            crate::api_common::component(rt, key, true, V::NULL)
        }
        // The registry entry a block or item is (`builtInRegistryHolder().is(tag)`).
        (_, "builtInRegistryHolder") if is_block_class(owner) => {
            let n = block_name(rt, a(0));
            crate::neo::registry_holder(rt, "block", &n, a(0))
        }
        (o, "builtInRegistryHolder") if o.starts_with("net/minecraft/world/item/") && o.ends_with("Item") => {
            let n = crate::api_common::item_like_name(rt, a(0));
            crate::neo::registry_holder(rt, "item", &n, a(0))
        }
        // Points of interest (a bed's, a bell's) are the game's; a mod's moved block has none here.
        ("net/minecraft/world/entity/ai/village/poi/PoiTypes", "forState") => {
            let o = rt.new_object("java/util/Optional");
            rt.put_field(o, "value", V::NULL);
            o
        }
        (BLOCK, "<init>") => V::Void,
        // The API's base constructors a mod's own classes call (`super(properties)`): their state
        // lives in the runtime's fields, so nothing more to do.
        (o, "<init>") if o.starts_with("net/minecraft/world/level/block/") && (o.ends_with("Block") || o.ends_with("BlockBehaviour")) => V::Void,
        (o, "<init>") if o.starts_with("net/minecraft/world/item/") && o.ends_with("Item") => V::Void,
        _ => return crate::api_common::call(rt, owner, name, desc, args),
    })
}

/// Whether `owner` is vanilla's `Block` or one of its block classes (`RotatedPillarBlock`).
fn is_block_class(owner: &str) -> bool {
    owner.starts_with("net/minecraft/world/level/block/") && owner.ends_with("Block") && !owner[32..].contains('/')
}

/// A block's translation key (`Block.getDescriptionId`): `block.create.shaft` for `create:shaft`.
fn description_id(name: &str) -> String {
    let (ns, path) = name.split_once(':').unwrap_or(("minecraft", name));
    format!("block.{ns}.{}", path.replace('/', "."))
}

/// A DeferredRegister's holder: its registry and name, resolved when asked (`create:pressing` is
/// both a recipe type and a recipe serializer).
fn holder(rt: &mut Rt, registry: &str, id: &str) -> V {
    let h = rt.new_object("net/neoforged/neoforge/registries/DeferredHolder");
    let (r, s) = (rt.string(registry), rt.string(id));
    rt.put_field(h, "$registry", r);
    rt.put_field(h, "$id", s);
    h
}

/// `BlockPos$MutableBlockPos`, the position a renderer or a scan moves along instead of making a new
/// one each step. It holds its place as a `BlockPos` does; `set`, `move` and `clamp` change it and
/// hand it back. What vanilla makes immutable (`offset`, `relative(d, n)`, `immutable()`) and what it
/// only reads (`getX`, `above()`) answer as `BlockPos`'s, through [`api_super`].
fn mutable_pos(rt: &mut Rt, key: &str, args: &[V]) -> Option<V> {
    let (name, desc) = key.strip_prefix(MUTABLE_POS)?.strip_prefix('.')?.split_once('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ints = |s: usize| [a(s).i() as i64, a(s + 1).i() as i64, a(s + 2).i() as i64];
    // `Mth.floor`: Java's `(int)` saturates as Rust's `as i32` does.
    let floors = |s: usize| [s, s + 1, s + 2].map(|i| a(i).d().floor() as i32 as i64);
    let at = native(rt, a(0));
    let step = |rt: &Rt, d: V, n: i64| DIRECTIONS[native(rt, d)[0].clamp(0, 5) as usize].map(|c| c * n);
    let plus = |p: [i64; 4], d: [i64; 3]| [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
    let args_of = &desc[..desc.find(')').map_or(desc.len(), |i| i + 1)];
    let to = match (name, args_of) {
        ("<init>", "()") => [0; 3],
        ("<init>" | "set", "(III)") => ints(1),
        ("<init>" | "set", "(DDD)") => floors(1),
        ("set", "(Lnet/minecraft/core/Vec3i;)") => plus(native(rt, a(1)), [0; 3]),
        ("set", "(J)") => {
            let l = a(1).j();
            [l >> 38, (l << 52) >> 52, (l << 26) >> 38]
        }
        ("setX", "(I)") => [a(1).i() as i64, at[1], at[2]],
        ("setY", "(I)") => [at[0], a(1).i() as i64, at[2]],
        ("setZ", "(I)") => [at[0], at[1], a(1).i() as i64],
        ("setWithOffset", "(Lnet/minecraft/core/Vec3i;Lnet/minecraft/core/Direction;)") => plus(native(rt, a(1)), step(rt, a(2), 1)),
        ("setWithOffset", "(Lnet/minecraft/core/Vec3i;III)") => plus(native(rt, a(1)), ints(2)),
        ("setWithOffset", "(Lnet/minecraft/core/Vec3i;Lnet/minecraft/core/Vec3i;)") => {
            let d = native(rt, a(2));
            plus(native(rt, a(1)), [d[0], d[1], d[2]])
        }
        ("move", "(Lnet/minecraft/core/Direction;)") => plus(at, step(rt, a(1), 1)),
        ("move", "(Lnet/minecraft/core/Direction;I)") => plus(at, step(rt, a(1), a(2).i() as i64)),
        ("move", "(III)") => plus(at, ints(1)),
        ("move", "(Lnet/minecraft/core/Vec3i;)") => {
            let d = native(rt, a(1));
            plus(at, [d[0], d[1], d[2]])
        }
        // `Mth.clamp(coordinate, min, max)` on one axis (X 0, Y 1, Z 2).
        ("clamp", "(Lnet/minecraft/core/Direction$Axis;II)") => {
            let mut p = [at[0], at[1], at[2]];
            let i = native(rt, a(1))[0].clamp(0, 2) as usize;
            let (lo, hi) = (a(2).i() as i64, a(3).i() as i64);
            p[i] = if p[i] < lo { lo } else { p[i].min(hi) };
            p
        }
        _ => return None,
    };
    // Coordinates are Java ints: a step past the edge wraps as the JVM's does.
    let to = to.map(|c| c as i32 as i64);
    if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
        o.data = Data::Native("BlockPos", [to[0], to[1], to[2], 0]);
    }
    Some(if name == "<init>" { V::Void } else { a(0) })
}

/// `BlockPos.asLong(x, y, z)`.
fn pack_pos(x: i64, y: i64, z: i64) -> i64 {
    ((x & 0x3FF_FFFF) << 38) | ((z & 0x3FF_FFFF) << 12) | (y & 0xFFF)
}

fn offset(rt: &mut Rt, p: V, d: [i64; 3], n: i64) -> V {
    let h = native(rt, p);
    block_pos(rt, h[0] + d[0] * n, h[1] + d[1] * n, h[2] + d[2] * n)
}

/// Static fields of API classes (`Direction.UP`...).
pub fn get_static(rt: &mut Rt, key: &'static str) -> V {
    if let Some((o, f)) = key.split_once('.')
        && let Some(v) = crate::network::get_static(rt, o, f)
            .or_else(|| crate::api_common::get_static(rt, o, f))
            .or_else(|| crate::render::get_static(rt, o, f))
            .or_else(|| crate::quads::get_static(rt, o, f))
            .or_else(|| crate::blocks::get_static(rt, o, f))
            .or_else(|| crate::shapes::get_static(rt, o, f))
            .or_else(|| crate::fluids::get_static(rt, o, f))
            .or_else(|| crate::neo_tags::get_static(rt, o, f))
            .or_else(|| crate::mod_entities::get_static(rt, o, f))
            .or_else(|| crate::tanks::get_static(rt, o, f))
            .or_else(|| crate::creative::get_static(rt, o, f))
            .or_else(|| crate::codecs::get_static(rt, o, f))
            .or_else(|| crate::recipes::get_static(rt, o, f))
            .or_else(|| crate::components::get_static(rt, o, f))
            .or_else(|| crate::capabilities::get_static(rt, o, f))
            .or_else(|| crate::sounds::get_static(rt, o, f))
            .or_else(|| crate::particles::get_static(rt, o, f))
    {
        return v;
    }
    let dir = |rt: &mut Rt, o| direction(rt, o);
    match key {
        "net/minecraft/core/Direction.DOWN" => dir(rt, 0),
        "net/minecraft/core/Direction.UP" => dir(rt, 1),
        "net/minecraft/core/Direction.NORTH" => dir(rt, 2),
        "net/minecraft/core/Direction.SOUTH" => dir(rt, 3),
        "net/minecraft/core/Direction.WEST" => dir(rt, 4),
        "net/minecraft/core/Direction.EAST" => dir(rt, 5),
        "net/minecraft/core/BlockPos.ZERO" | "net/minecraft/core/Vec3i.ZERO" => block_pos(rt, 0, 0, 0),
        // The dimensions' keys, as `Level.dimension()` gives them.
        "net/minecraft/world/level/Level.OVERWORLD" => rt.string("dimension#minecraft:overworld"),
        "net/minecraft/world/level/Level.NETHER" => rt.string("dimension#minecraft:the_nether"),
        "net/minecraft/world/level/Level.END" => rt.string("dimension#minecraft:the_end"),
        // `BuiltInRegistries.TRIGGER_TYPES` is registry `trigger_type`, as its key says.
        k if k.starts_with("net/minecraft/core/registries/BuiltInRegistries.") => {
            let n = k.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let n = if n.ends_with("_types") || n == "attributes" { n.trim_end_matches('s').to_owned() } else { n };
            rt.string(&n)
        }
        // NeoForge's game event bus: one object, as the mod bus (events route by class).
        "net/neoforged/neoforge/common/NeoForge.EVENT_BUS" => rt.intern("compat:game-bus".into(), |rt| rt.new_object("net/neoforged/bus/api/IEventBus")),
        // The side, one object per constant: catnip compares `FMLLoader.getDist() == Dist.CLIENT`.
        "net/neoforged/api/distmarker/Dist.CLIENT" | "net/neoforged/fml/loading/FMLEnvironment.dist" => api_enum(rt, "net/neoforged/api/distmarker/Dist", "CLIENT", 0),
        "net/neoforged/api/distmarker/Dist.DEDICATED_SERVER" => api_enum(rt, "net/neoforged/api/distmarker/Dist", "DEDICATED_SERVER", 1),
        k if k.starts_with("net/neoforged/bus/api/EventPriority.") => {
            let n = k.rsplit('.').next().unwrap_or("");
            rt.string(n)
        }
        k if k.starts_with("java/util/Locale.") => rt.intern("compat:locale".into(), |rt| rt.new_object("java/util/Locale")),
        // Vanilla enums the mod reads constants of: one object per constant, named and ordered as Java's.
        k if k.starts_with("net/minecraft/ChatFormatting.") => {
            const NAMES: [&str; 22] = [
                "BLACK", "DARK_BLUE", "DARK_GREEN", "DARK_AQUA", "DARK_RED", "DARK_PURPLE", "GOLD", "GRAY", "DARK_GRAY", "BLUE", "GREEN", "AQUA", "RED",
                "LIGHT_PURPLE", "YELLOW", "WHITE", "OBFUSCATED", "BOLD", "STRIKETHROUGH", "UNDERLINE", "ITALIC", "RESET",
            ];
            let f = k.rsplit('.').next().unwrap_or("");
            match NAMES.iter().position(|n| *n == f) {
                Some(i) => api_enum(rt, "net/minecraft/ChatFormatting", NAMES[i], i as i32),
                None => {
                    *rt.unmapped.entry(key).or_insert(0) += 1;
                    V::NULL
                }
            }
        }
        // `SoundSource`, 1.21.1's constants in order (what a sound is played under).
        k if k.starts_with("net/minecraft/sounds/SoundSource.") => {
            const NAMES: [&str; 10] = ["MASTER", "MUSIC", "RECORDS", "WEATHER", "BLOCKS", "HOSTILE", "NEUTRAL", "PLAYERS", "AMBIENT", "VOICE"];
            let f = k.rsplit('.').next().unwrap_or("");
            match NAMES.iter().position(|n| *n == f) {
                Some(i) => api_enum(rt, "net/minecraft/sounds/SoundSource", NAMES[i], i as i32),
                None => {
                    *rt.unmapped.entry(key).or_insert(0) += 1;
                    V::NULL
                }
            }
        }
        k if k.starts_with("net/minecraft/core/registries/Registries.") => {
            let n = k.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            rt.string(&n)
        }
        k if k.starts_with("net/minecraft/world/level/block/RenderShape.") => {
            let f = k.rsplit('.').next().unwrap_or("");
            match RENDER_SHAPES.iter().position(|n| *n == f) {
                Some(i) => api_enum(rt, "net/minecraft/world/level/block/RenderShape", RENDER_SHAPES[i], i as i32),
                None => {
                    *rt.unmapped.entry(key).or_insert(0) += 1;
                    V::NULL
                }
            }
        }
        k if k.starts_with("net/minecraft/world/level/block/Blocks.") => {
            let n = format!("minecraft:{}", k.rsplit('.').next().unwrap_or("").to_ascii_lowercase());
            block(rt, &n)
        }
        _ => {
            *rt.unmapped.entry(key).or_insert(0) += 1;
            V::NULL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_pos_relative_and_opposite() {
        let mut rt = Rt::new();
        let p = block_pos(&mut rt, 10, 64, -3);
        let north = get_static(&mut rt, "net/minecraft/core/Direction.NORTH");
        let q = call(&mut rt, "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction;I)Lnet/minecraft/core/BlockPos;", &[p, north, V::I(4)]);
        assert_eq!(call(&mut rt, "net/minecraft/core/BlockPos.getZ.()I", &[q]), V::I(-7));
        let south = call(&mut rt, "net/minecraft/core/Direction.getOpposite.()Lnet/minecraft/core/Direction;", &[north]);
        assert_eq!(call(&mut rt, "net/minecraft/core/Direction.getStepZ.()I", &[south]), V::I(1));
    }

    /// Create tesselates the blocks of a contraption whose `getRenderShape()` is `MODEL`, compared
    /// with `==`: a plank is one; in 1.21.1 a chest is drawn by its block entity (animated), a sign
    /// and air not at all.
    /// NeoForge's `canStickTo` (what Create's assembly asks of neighbours): slime and honey stick to
    /// anything but each other; two plain blocks don't.
    #[test]
    fn slime_and_honey_stick_but_not_to_each_other() {
        let mut rt = Rt::new();
        crate::install();
        let s = |rt: &mut Rt, t: &str| state(rt, t);
        let (slime, honey, stone, planks) = (s(&mut rt, "minecraft:slime_block"), s(&mut rt, "minecraft:honey_block"), s(&mut rt, "minecraft:stone"), s(&mut rt, "minecraft:oak_planks"));
        let stick = |rt: &mut Rt, a: V, b: V| call(rt, "net/minecraft/world/level/block/state/BlockState.canStickTo.(Lnet/minecraft/world/level/block/state/BlockState;)Z", &[a, b]);
        assert_eq!(stick(&mut rt, slime, stone), V::I(1));
        assert_eq!(stick(&mut rt, stone, honey), V::I(1));
        assert_eq!(stick(&mut rt, slime, honey), V::I(0));
        assert_eq!(stick(&mut rt, honey, slime), V::I(0));
        assert_eq!(stick(&mut rt, slime, slime), V::I(1));
        assert_eq!(stick(&mut rt, stone, planks), V::I(0));
        let slimy = |rt: &mut Rt, a: V| call(rt, "net/minecraft/world/level/block/state/BlockState.isSlimeBlock.()Z", &[a]);
        assert_eq!((slimy(&mut rt, slime), slimy(&mut rt, honey)), (V::I(1), V::I(0)));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_states_render_shape_is_the_one_its_vanilla_class_gives() {
        let mut rt = Rt::new();
        let shape = |rt: &mut Rt, s: &str| {
            let st = state(rt, s);
            call(rt, "net/minecraft/world/level/block/state/BlockState.getRenderShape.()Lnet/minecraft/world/level/block/RenderShape;", &[st])
        };
        let name = |rt: &Rt, v: V| rt.str_of(rt.get_field(v, "$name")).unwrap_or("").to_owned();
        let model = get_static(&mut rt, "net/minecraft/world/level/block/RenderShape.MODEL");
        assert_eq!(shape(&mut rt, "minecraft:oak_planks"), model);
        let chest = shape(&mut rt, "minecraft:chest[facing=north,type=single,waterlogged=false]");
        assert_eq!(name(&rt, chest), "ENTITYBLOCK_ANIMATED");
        let sign = shape(&mut rt, "minecraft:oak_sign[rotation=0,waterlogged=false]");
        assert_eq!(name(&rt, sign), "INVISIBLE");
        let air = shape(&mut rt, "minecraft:air");
        assert_eq!(name(&rt, air), "INVISIBLE");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create puts a contraption's blocks back at `BlockPos.containing(anchor + 0.5)`: the block the
    /// point is in, flooring negative coordinates down. A factory nothing maps still counts as a
    /// miss, though its chain goes on with a stand-in.
    #[test]
    fn the_block_a_point_is_in() {
        let mut rt = Rt::new();
        let v = rt.new_object("net/minecraft/world/phys/Vec3");
        call(&mut rt, "net/minecraft/world/phys/Vec3.<init>.(DDD)V", &[v, V::D(0.5), V::D(65.5), V::D(-0.5)]);
        let p = call(&mut rt, "net/minecraft/core/BlockPos.containing.(Lnet/minecraft/core/Position;)Lnet/minecraft/core/BlockPos;", &[v]);
        assert_eq!(pos_of(&rt, p), [0, 65, -1]);
        let q = call(&mut rt, "net/minecraft/core/BlockPos.containing.(DDD)Lnet/minecraft/core/BlockPos;", &[V::D(-3.2), V::D(1.0), V::D(7.9)]);
        assert_eq!(pos_of(&rt, q), [-4, 1, 7]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        let made = call(&mut rt, "net/minecraft/core/BlockPos.notAFactory.()Lnet/minecraft/core/BlockPos;", &[]);
        assert_eq!(rt.class_of(made), BLOCK_POS);
        assert_eq!(rt.unmapped.get("net/minecraft/core/BlockPos.notAFactory.()Lnet/minecraft/core/BlockPos;"), Some(&1));
    }

    /// `Direction.getClockWise(Axis)` and its counter-clockwise twin, against vanilla 26.3's
    /// `getClockWiseX/Z` tables (read from its bytecode). Create's wrench turns blocks with them.
    #[test]
    fn directions_turn_about_each_axis() {
        let mut rt = Rt::new();
        let names = ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"];
        let mut turn = |from: &str, axis: &str, clockwise: bool| -> &'static str {
            let d = get_static(&mut rt, Box::leak(format!("net/minecraft/core/Direction.{from}").into_boxed_str()));
            let ax = get_static(&mut rt, Box::leak(format!("net/minecraft/core/Direction$Axis.{axis}").into_boxed_str()));
            let key = if clockwise {
                "net/minecraft/core/Direction.getClockWise.(Lnet/minecraft/core/Direction$Axis;)Lnet/minecraft/core/Direction;"
            } else {
                "net/minecraft/core/Direction.getCounterClockWise.(Lnet/minecraft/core/Direction$Axis;)Lnet/minecraft/core/Direction;"
            };
            let r = call(&mut rt, key, &[d, ax]);
            names[native(&rt, r)[0] as usize]
        };
        let got = [
            turn("EAST", "Y", true),
            turn("UP", "X", true),
            turn("NORTH", "X", true),
            turn("DOWN", "X", false),
            turn("UP", "Z", true),
            turn("EAST", "Z", false),
            turn("WEST", "X", true),
        ];
        assert_eq!(got, ["SOUTH", "NORTH", "DOWN", "NORTH", "EAST", "UP", "WEST"]);
    }

    /// `MutableBlockPos` as Create's belt renderer uses it: one object moved in place, read and
    /// compared as the `BlockPos` it stands at, while `immutable()` keeps where it was.
    #[test]
    fn a_mutable_position_moves_in_place_and_reads_as_a_block_pos() {
        let mut rt = Rt::new();
        let k = |s: &str| -> &'static str { Box::leak(format!("net/minecraft/core/BlockPos$MutableBlockPos.{s}").into_boxed_str()) };
        let xyz = |rt: &mut Rt, v: V| ["getX", "getY", "getZ"].map(|n| call(rt, k(&format!("{n}.()I")), &[v]).i());
        let m = rt.new_object(MUTABLE_POS);
        call(&mut rt, k("<init>.()V"), &[m]);
        let p = block_pos(&mut rt, 4, 70, -9);
        let east = get_static(&mut rt, "net/minecraft/core/Direction.EAST");
        let back = call(&mut rt, k("set.(Lnet/minecraft/core/Vec3i;)Lnet/minecraft/core/BlockPos$MutableBlockPos;"), &[m, p]);
        assert_eq!(back, m);
        call(&mut rt, k("move.(Lnet/minecraft/core/Direction;I)Lnet/minecraft/core/BlockPos$MutableBlockPos;"), &[m, east, V::I(3)]);
        call(&mut rt, k("move.(III)Lnet/minecraft/core/BlockPos$MutableBlockPos;"), &[m, V::I(0), V::I(-2), V::I(1)]);
        assert_eq!(xyz(&mut rt, m), [7, 68, -8]);
        let kept = call(&mut rt, k("immutable.()Lnet/minecraft/core/BlockPos;"), &[m]);
        call(&mut rt, k("setY.(I)Lnet/minecraft/core/BlockPos$MutableBlockPos;"), &[m, V::I(0)]);
        assert_eq!((xyz(&mut rt, kept), xyz(&mut rt, m)), ([7, 68, -8], [7, 0, -8]));
        let here = block_pos(&mut rt, 7, 0, -8);
        assert_eq!(call(&mut rt, "net/minecraft/core/BlockPos.equals.(Ljava/lang/Object;)Z", &[here, m]), V::I(1));
        let next = block_pos(&mut rt, 8, 0, -8);
        let closer = |rt: &mut Rt, d: f64| call(rt, k("closerThan.(Lnet/minecraft/core/Vec3i;D)Z"), &[m, next, V::D(d)]);
        assert_eq!((closer(&mut rt, 1.5), closer(&mut rt, 1.0)), (V::I(1), V::I(0)));
        let along = |rt: &mut Rt, axis: &'static str| {
            let a = get_static(rt, axis);
            call(rt, k("get.(Lnet/minecraft/core/Direction$Axis;)I"), &[m, a]).i()
        };
        let axes = ["net/minecraft/core/Direction$Axis.X", "net/minecraft/core/Direction$Axis.Y", "net/minecraft/core/Direction$Axis.Z"];
        assert_eq!(axes.map(|a| along(&mut rt, a)), [7, 0, -8]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn unmapped_calls_are_counted() {
        let mut rt = Rt::new();
        call(&mut rt, "net/minecraft/Nope.x.()V", &[]);
        call(&mut rt, "net/minecraft/Nope.x.()V", &[]);
        assert_eq!(rt.unmapped["net/minecraft/Nope.x.()V"], 2);
    }

    #[test]
    fn saved_data_is_made_once_per_name() {
        // A mod's `SavedData.Factory` constructor (a lambda): each call makes a new object.
        fn construct(rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
            Some(rt.new_object("test/Tracks"))
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(construct);
        let make = rt.lambda_id("java/util/function/Supplier", 1, Vec::new());
        let factory = rt.new_object(SAVED_DATA_FACTORY);
        call(&mut rt, "net/minecraft/world/level/saveddata/SavedData$Factory.<init>.(Ljava/util/function/Supplier;Ljava/util/function/BiFunction;)V", &[factory, make, V::NULL]);
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let storage = call(&mut rt, "net/minecraft/server/level/ServerLevel.getDataStorage.()Lnet/minecraft/world/level/storage/DimensionDataStorage;", &[level]);
        let compute = "net/minecraft/world/level/storage/DimensionDataStorage.computeIfAbsent.(Lnet/minecraft/world/level/saveddata/SavedData$Factory;Ljava/lang/String;)Lnet/minecraft/world/level/saveddata/SavedData;";
        let get = "net/minecraft/world/level/storage/DimensionDataStorage.get.(Lnet/minecraft/world/level/saveddata/SavedData$Factory;Ljava/lang/String;)Lnet/minecraft/world/level/saveddata/SavedData;";
        let name = rt.string("create_tracks");
        let other = rt.string("create_logistics");
        assert_eq!(call(&mut rt, get, &[storage, factory, name]), V::NULL);
        let first = call(&mut rt, compute, &[storage, factory, name]);
        assert_eq!(rt.class_of(first), "test/Tracks");
        assert_eq!(call(&mut rt, compute, &[storage, factory, name]), first);
        assert_eq!(call(&mut rt, get, &[storage, factory, name]), first);
        let second = call(&mut rt, compute, &[storage, factory, other]);
        assert_ne!(second, first);
    }
}
