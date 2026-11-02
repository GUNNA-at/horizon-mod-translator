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
    let Some((mut owner, rest)) = key.split_once('.') else { return r };
    while let Some(&sup) = rt.supers.get(owner) {
        let up = rt.static_key(&format!("{sup}.{rest}"));
        let seen = rt.unmapped.get(up).copied().unwrap_or(0);
        let r2 = call_exact(rt, up, args);
        if rt.unmapped.get(up).copied().unwrap_or(0) == seen {
            // Resolved: the miss on the mod's own name doesn't count.
            if let Some(n) = rt.unmapped.get_mut(key) {
                *n -= 1;
                if *n == 0 {
                    rt.unmapped.remove(key);
                }
            }
            return r2;
        }
        if let Some(n) = rt.unmapped.get_mut(up) {
            *n -= 1;
            if *n == 0 {
                rt.unmapped.remove(up);
            }
        }
        owner = sup;
    }
    trace_unmapped(rt, key, args);
    r
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
        "java/lang/Integer.intValue.()I" => rt.get_field(a(0), "value"),
        "java/lang/Enum.ordinal.()I" => V::I(rt.get_field(a(0), "$ordinal").i()),

        // net.minecraft.core.BlockPos
        "net/minecraft/core/BlockPos.<init>.(III)V" => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Native("BlockPos", [a(1).i() as i64, a(2).i() as i64, a(3).i() as i64, 0]);
            }
            V::Void
        }
        "net/minecraft/core/BlockPos.getX.()I" | "net/minecraft/core/Vec3i.getX.()I" => V::I(native(rt, a(0))[0] as i32),
        "net/minecraft/core/BlockPos.getY.()I" | "net/minecraft/core/Vec3i.getY.()I" => V::I(native(rt, a(0))[1] as i32),
        "net/minecraft/core/BlockPos.getZ.()I" | "net/minecraft/core/Vec3i.getZ.()I" => V::I(native(rt, a(0))[2] as i32),
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
        "net/minecraft/core/Vec3i.distManhattan.(Lnet/minecraft/core/Vec3i;)I" | "net/minecraft/core/BlockPos.distManhattan.(Lnet/minecraft/core/Vec3i;)I" => {
            let (p, q) = (native(rt, a(0)), native(rt, a(1)));
            V::I(((p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs()) as i32)
        }
        "net/minecraft/core/Vec3i.distSqr.(Lnet/minecraft/core/Vec3i;)D" | "net/minecraft/core/BlockPos.distSqr.(Lnet/minecraft/core/Vec3i;)D" => {
            let (p, q) = (native(rt, a(0)), native(rt, a(1)));
            let d = |i: usize| (p[i] - q[i]) as f64;
            V::D(d(0) * d(0) + d(1) * d(1) + d(2) * d(2))
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
        _ => match crate::neo::call(rt, key, args).or_else(|| crate::jlang::call(rt, key, args)).or_else(|| crate::jutil::call(rt, key, args)).or_else(|| world(rt, key, args)) {
            Some(v) => v,
            None => {
                // Builder chains (`Properties.of().strength(1f).noOcclusion()`): a method
                // returning its own class hands back the receiver.
                if let Some((owner, rest)) = key.split_once('.')
                    && rest.ends_with(&format!(")L{owner};"))
                {
                    // A static factory (`Properties.of()`) makes one; a chained call returns its receiver.
                    return if a(0).r() != 0 && rt.class_of(a(0)) == owner { a(0) } else { rt.new_object(owner) };
                }
                *rt.unmapped.entry(key).or_insert(0) += 1;
                V::Void
            }
        },
    }
}

const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const BLOCK: &str = "net/minecraft/world/level/block/Block";

/// A block state value (its text, `minecraft:stone` or `create:shaft[axis=y]`).
pub fn state(rt: &mut Rt, text: &str) -> V {
    crate::api_common::state(rt, text)
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
fn block_name(rt: &Rt, b: V) -> String {
    rt.str_of(b).map(str::to_owned).or_else(|| rt.registered_name(b, "block").map(str::to_owned)).unwrap_or_default()
}

fn pos_of(rt: &Rt, v: V) -> [i32; 3] {
    let h = native(rt, v);
    [h[0] as i32, h[1] as i32, h[2] as i32]
}

fn qualified(rt: &Rt, name: &str) -> String {
    if name.contains(':') { name.to_owned() } else { format!("{}:{name}", rt.namespace) }
}

/// Registration, the level and block states: the API that reaches the game.
fn world(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    let level = owner.starts_with("net/minecraft/") && (owner.ends_with("Level") || owner.ends_with("LevelAccessor") || owner.ends_with("LevelReader") || owner.ends_with("BlockGetter") || owner.ends_with("LevelWriter"));
    Some(match (owner, name) {
        // NeoForge registration
        ("net/neoforged/neoforge/registries/DeferredRegister", "create") => {
            let registry = rt.str_of(a(0)).unwrap_or("unknown").to_owned();
            let ns = rt.str_of(a(1)).unwrap_or("").to_owned();
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
            rt.suppliers.push((registry, id.clone(), a(2)));
            holder(rt, &id)
        }
        (o, "registerBlock" | "registerItem") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let registry = if name == "registerBlock" { "block" } else { "item" };
            let id = qualified(rt, rt.str_of(a(1)).unwrap_or(""));
            let props = if desc.contains("Properties;)") || args.len() > 3 { a(3) } else { V::NULL };
            let apply = rt.lambda("compat:apply", vec![a(2), props]);
            rt.suppliers.push((registry.into(), id.clone(), apply));
            holder(rt, &id)
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

        // Block states
        (STATE | "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase", n) => {
            let text = rt.str_of(a(0)).unwrap_or("minecraft:air").to_owned();
            let (b, _) = crate::host::split_state(&text);
            match n {
                "getBlock" => block(rt, &b.to_owned()),
                "isAir" => V::I(matches!(b, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") as i32),
                "hasBlockEntity" => V::I(crate::glue::block_entity_id(rt, b).is_some() as i32),
                "is" if desc.starts_with("(Lnet/minecraft/world/level/block/Block;") => V::I((block_name(rt, a(1)) == b) as i32),
                "equals" => V::I((rt.str_of(a(1)) == Some(text.as_str())) as i32),
                _ => return crate::api_common::call(rt, owner, name, desc, args),
            }
        }
        (BLOCK, "defaultBlockState") | (_, "defaultBlockState") => {
            let n = block_name(rt, a(0));
            state(rt, &n)
        }
        (BLOCK, "<init>") => V::Void,
        // The API's base constructors a mod's own classes call (`super(properties)`): their state
        // lives in the runtime's fields, so nothing more to do.
        (o, "<init>") if o.starts_with("net/minecraft/world/level/block/") && (o.ends_with("Block") || o.ends_with("BlockBehaviour")) => V::Void,
        (o, "<init>") if o.starts_with("net/minecraft/world/item/") && o.ends_with("Item") => V::Void,
        _ => return crate::api_common::call(rt, owner, name, desc, args),
    })
}

fn holder(rt: &mut Rt, id: &str) -> V {
    let h = rt.new_object("net/neoforged/neoforge/registries/DeferredHolder");
    let s = rt.string(id);
    rt.put_field(h, "$id", s);
    h
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
        && let Some(v) = crate::api_common::get_static(rt, o, f)
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
        "net/minecraft/core/BlockPos.ZERO" => block_pos(rt, 0, 0, 0),
        // `BuiltInRegistries.TRIGGER_TYPES` is registry `trigger_type`, as its key says.
        k if k.starts_with("net/minecraft/core/registries/BuiltInRegistries.") => {
            let n = k.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let n = if n.ends_with("_types") || n == "attributes" { n.trim_end_matches('s').to_owned() } else { n };
            rt.string(&n)
        }
        // NeoForge's game event bus: one object, as the mod bus (events route by class).
        "net/neoforged/neoforge/common/NeoForge.EVENT_BUS" => rt.intern("compat:game-bus".into(), |rt| rt.new_object("net/neoforged/bus/api/IEventBus")),
        "net/neoforged/api/distmarker/Dist.CLIENT" | "net/neoforged/fml/loading/FMLEnvironment.dist" => rt.string("CLIENT"),
        "net/neoforged/api/distmarker/Dist.DEDICATED_SERVER" => rt.string("DEDICATED_SERVER"),
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
        k if k.starts_with("net/minecraft/core/registries/Registries.") => {
            let n = k.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            rt.string(&n)
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

    #[test]
    fn unmapped_calls_are_counted() {
        let mut rt = Rt::new();
        call(&mut rt, "net/minecraft/Nope.x.()V", &[]);
        call(&mut rt, "net/minecraft/Nope.x.()V", &[]);
        assert_eq!(rt.unmapped["net/minecraft/Nope.x.()V"], 2);
    }
}
