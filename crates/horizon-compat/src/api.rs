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
const PACKET_FLOW: &str = "net/minecraft/network/protocol/PacketFlow";
const MUTABLE_POS: &str = "net/minecraft/core/BlockPos$MutableBlockPos";

/// Minecraft's superclasses a missed call climbs past the mod's own (the API's classes aren't in
/// `rt.supers`): what a `MutableBlockPos` inherits answers as `BlockPos`'s, then `Vec3i`'s; a
/// `ZombieModel`'s as `HumanoidModel`'s, a `MobRenderer`'s as `LivingEntityRenderer`'s. `instanceof`
/// climbs them too ([`crate::api_common::api_is_a`]).
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    match class {
        MUTABLE_POS => Some(BLOCK_POS),
        BLOCK_POS | "net/minecraft/core/SectionPos" => Some("net/minecraft/core/Vec3i"),
        // A mod's render types extend `RenderType` and read the shards it inherits from
        // `RenderStateShard` (catnip's `PonderRenderTypes.BLOCK_SHEET_MIPPED`).
        "net/minecraft/client/renderer/RenderType" => Some("net/minecraft/client/renderer/RenderStateShard"),
        // A space's glyph draws nothing (Create's display boards skip it by `instanceof`).
        crate::gui::EMPTY_GLYPH => Some("net/minecraft/client/gui/font/glyphs/BakedGlyph"),
        NOISE_GENERATOR => Some(CHUNK_GENERATOR),
        _ => crate::models::api_super(class)
            .or_else(|| crate::entity_render::api_super(class))
            .or_else(|| crate::camera::api_super(class))
            .or_else(|| crate::item_wrappers::api_super(class))
            .or_else(|| crate::vanilla_entities::api_super(class)),
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

thread_local! {
    /// How many API calls are running on this thread, one inside another.
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// API calls nested deeper than this are a loop (a handler calling a functional object that
/// leads back to the same handler): the call throws `StackOverflowError`, as the JVM would at its
/// stack's end, instead of overflowing the game's.
const MAX_DEPTH: u32 = 512;

/// A game class's field `name` read by the mod's code: the field when the object has it, else what
/// the runtime keeps for it (`own`), else the accessor `key` answers. Generated code names
/// `api::game_field` whether it links the core or carries this runtime inlined, where the core's
/// entry points are left out: so this is the runtime's own, not a call into the core.
pub fn game_field(rt: &mut Rt, o: V, name: &'static str, own: &'static str, key: &'static str) -> V {
    match rt.get_field(o, name) {
        V::Void if o.r() != 0 => match rt.get_field(o, own) {
            V::Void => call(rt, key, &[o]),
            v => v,
        },
        v => v,
    }
}

/// Calls the API method `key` (`owner.name.desc`). For instance methods
/// `args[0]` is the receiver. A key naming one of the mod's own classes (a
/// member it inherits from the API, as `javac` writes inherited calls) is
/// resolved up the class's superclasses, as the JVM resolves it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let depth = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get()
    });
    let r = if depth > MAX_DEPTH {
        rt.throw_new("java/lang/StackOverflowError");
        V::Void
    } else {
        call_nested(rt, key, args)
    };
    DEPTH.with(|d| d.set(d.get() - 1));
    r
}

fn call_nested(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let since = crate::misses();
    let r = call_exact(rt, key, args);
    if crate::misses() == since || !crate::missed_since(key, since) {
        return r;
    }
    let Some((owner, rest)) = key.split_once('.') else { return r };
    // `getClass()` through any type (`wrapper.getClass() == FluidBucketWrapper.class`): it is
    // final in `Object`, so always `Object`'s own.
    if rest == "getClass.()Ljava/lang/Class;" {
        uncount(rt, key);
        return call_exact(rt, "java/lang/Object.getClass.()Ljava/lang/Class;", args);
    }
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
    // What every class takes from `Object` where nothing up its chain declares it (an entity's,
    // a block entity's): Java's own `equals`, `hashCode` and `toString`, as the JVM resolves a
    // method no class below `Object` has. Create finds the contraptions around one by
    // `e -> !e.equals(self)`: unanswered, a contraption counted itself and stopped at once.
    if matches!(
        rest,
        "equals.(Ljava/lang/Object;)Z" | "hashCode.()I" | "toString.()Ljava/lang/String;"
    ) {
        uncount(rt, key);
        let object = rt.static_key(&format!("java/lang/Object.{rest}"));
        return call_exact(rt, object, args);
    }
    trace_unmapped(rt, key, args);
    if runaway(rt) {
        let secs = rt.hook_first_miss.map_or(0, |t| t.elapsed().as_secs());
        let n = rt.hook_misses;
        crate::jlang::throw_with(rt, "java/lang/Error", &format!("horizon-compat: {key} isn't translated, and this hook kept calling what isn't ({n} calls in {secs} s): a loop waiting on its stand-in answer"));
        return r;
    }
    chained(rt, key, args).unwrap_or(r)
}

/// Calls nothing maps, in one hook, past which the hook is taken to be stuck ([`runaway`]).
const RUNAWAY_MISSES: u64 = 1_000_000;
/// How long a hook goes on making such calls before it is taken to be stuck.
const RUNAWAY_TIME: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether the hook running now (one call of the game's into the mod: a tick, a screen's frame)
/// is stuck in a loop on what nothing maps: an unmapped call answers a stand-in (0, null), and a
/// loop that waits for another answer (`while (it.next() != BreakIterator.DONE)`) never ends. Past
/// [`RUNAWAY_MISSES`] such calls over [`RUNAWAY_TIME`], each throws `java.lang.Error`, which unwinds
/// the mod's code to the hook's end, as a `catch (Exception e)` doesn't stop it: the game goes on
/// without what the hook would have done, and the error names the call. Outside hooks (loading)
/// there is no game to keep going.
fn runaway(rt: &mut Rt) -> bool {
    if rt.host().is_none() {
        return false;
    }
    rt.hook_misses += 1;
    let first = *rt.hook_first_miss.get_or_insert_with(std::time::Instant::now);
    rt.hook_misses >= RUNAWAY_MISSES && first.elapsed() >= RUNAWAY_TIME
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
    // Then the interfaces those classes implement, as the JVM finds a default method after the
    // superclasses': a mod's `CreativeModeTab.Output` takes `accept(ItemLike)` from the interface.
    let mut next = 0;
    let mut classes: Vec<&'static str> = std::iter::once(owner).chain(out.iter().copied()).collect();
    while next < classes.len() {
        let c = classes[next];
        next += 1;
        for &i in rt.interfaces.get(c).copied().unwrap_or_default() {
            if i != owner && !classes.contains(&i) {
                classes.push(i);
                out.push(i);
            }
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
/// `Throwable.toString()`: the class's binary name, then `: message` when it has one.
fn throwable_string(rt: &Rt, e: V) -> String {
    let class = rt.class_of(e).replace('/', ".");
    let m = rt.get_field(e, "message");
    match rt.str_of(m) {
        Some(m) => format!("{class}: {m}"),
        None => class,
    }
}

fn exception_class(key: &str) -> bool {
    if !key.starts_with("java/") {
        return false;
    }
    let owner = key.split('.').next().unwrap_or("");
    owner.ends_with("Exception") || owner.ends_with("Error") || owner.ends_with("Throwable")
}

fn trace_unmapped(rt: &Rt, key: &'static str, args: &[V]) {
    if !Rt::tracing() {
        return;
    }
    let n = rt.unmapped.get(key).copied().unwrap_or(0);
    if n == 1 {
        let on = match args.first() {
            Some(&v @ V::R(_)) => format!(" (on {})", rt.class_of(v)),
            Some(v) => format!(" (on {v:?})"),
            None => String::new(),
        };
        eprintln!("horizon-compat: unmapped {key}{on}");
    }
    // `HORIZON_COMPAT_BACKTRACE=<part of a key>[,<another>...]`: who makes the first few such
    // calls, by the translated functions on the stack.
    static PART: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    if let Some(part) = PART.get_or_init(|| std::env::var("HORIZON_COMPAT_BACKTRACE").ok())
        && n <= 3
        && part.split(',').any(|p| !p.is_empty() && key.contains(p))
    {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let frames: Vec<&str> = bt
            .lines()
            .map(str::trim)
            .filter(|l| l.contains(": ") && !l.starts_with("at "))
            .filter_map(|l| l.split_once(": ").map(|(_, f)| f))
            .filter(|f| !f.starts_with("std::") && !f.starts_with("core::") && !f.contains("trace_unmapped") && !f.contains("backtrace"))
            .take(14)
            .collect();
        eprintln!("horizon-compat: backtrace of {key} #{n}:\n    {}", frames.join("\n    "));
    }
}

thread_local! {
    /// The keys (by address and length) none of [`call_exact`]'s own arms takes: its arms and
    /// their guards read the key alone, so such a key goes straight to the modules next time.
    static TO_CHAIN: std::cell::RefCell<crate::HashMap<(usize, usize), ()>> = std::cell::RefCell::new(crate::HashMap::default());
}

fn call_exact(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let id = (key.as_ptr() as usize, key.len());
    if TO_CHAIN.with(|c| c.borrow().contains_key(&id)) {
        return through_chain(rt, key, args);
    }
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
        // Guava's `DoubleMath.fuzzyEquals` and `fuzzyCompare`: within the tolerance (or both NaN) is
        // equal; otherwise by value, NaN last.
        "com/google/common/math/DoubleMath.fuzzyEquals.(DDD)Z" => V::I(fuzzy_equals(a(0).d(), a(1).d(), a(2).d()) as i32),
        "com/google/common/math/DoubleMath.fuzzyCompare.(DDD)I" => {
            let (x, y) = (a(0).d(), a(1).d());
            V::I(if fuzzy_equals(x, y, a(2).d()) {
                0
            } else if x < y {
                -1
            } else if x > y {
                1
            } else {
                i32::from(x.is_nan()) - i32::from(y.is_nan())
            })
        }
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
        "net/minecraft/network/protocol/PacketFlow.getOpposite.()Lnet/minecraft/network/protocol/PacketFlow;" => {
            let key = if rt.get_field(a(0), "$ordinal").i() == 0 { "net/minecraft/network/protocol/PacketFlow.CLIENTBOUND" } else { "net/minecraft/network/protocol/PacketFlow.SERVERBOUND" };
            get_static(rt, key)
        }
        "net/minecraft/network/protocol/PacketFlow.id.()Ljava/lang/String;" => {
            let id = if rt.get_field(a(0), "$ordinal").i() == 0 { "serverbound" } else { "clientbound" };
            rt.string(id)
        }

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
        // `Vec3i`'s own arithmetic (Copycats+ walks its parts so): a `Vec3i`, or a `BlockPos` when
        // that is what it was called on, as `BlockPos` overrides each; `relative(axis, 0)` is itself.
        k if k.starts_with("net/minecraft/core/Vec3i.") && k.ends_with(")Lnet/minecraft/core/Vec3i;") => {
            let (_, name, desc) = crate::parts(key).unwrap_or_default();
            let p = native(rt, a(0));
            let unit = |rt: &Rt, v: V| -> [i64; 3] {
                let d = DIRECTIONS[native(rt, v)[0].clamp(0, 5) as usize];
                [d[0], d[1], d[2]]
            };
            let (d, n): ([i64; 3], i64) = match (name, desc) {
                ("relative", "(Lnet/minecraft/core/Direction$Axis;I)Lnet/minecraft/core/Vec3i;") => {
                    let mut d = [0; 3];
                    d[native(rt, a(1))[0].clamp(0, 2) as usize] = 1;
                    (d, a(2).i() as i64)
                }
                ("relative", "(Lnet/minecraft/core/Direction;I)Lnet/minecraft/core/Vec3i;") => (unit(rt, a(1)), a(2).i() as i64),
                ("relative", "(Lnet/minecraft/core/Direction;)Lnet/minecraft/core/Vec3i;") => (unit(rt, a(1)), 1),
                ("offset", "(III)Lnet/minecraft/core/Vec3i;") => ([a(1).i() as i64, a(2).i() as i64, a(3).i() as i64], 1),
                ("offset" | "subtract", "(Lnet/minecraft/core/Vec3i;)Lnet/minecraft/core/Vec3i;") => {
                    let q = native(rt, a(1));
                    ([q[0], q[1], q[2]], if name == "subtract" { -1 } else { 1 })
                }
                ("above" | "below" | "north" | "south" | "west" | "east", _) => {
                    let d = match name {
                        "above" => [0, 1, 0],
                        "below" => [0, -1, 0],
                        "north" => [0, 0, -1],
                        "south" => [0, 0, 1],
                        "west" => [-1, 0, 0],
                        _ => [1, 0, 0],
                    };
                    (d, if desc.starts_with("(I)") { a(1).i() as i64 } else { 1 })
                }
                ("multiply", "(I)Lnet/minecraft/core/Vec3i;") => {
                    let k = a(1).i() as i64;
                    ([p[0] * (k - 1), p[1] * (k - 1), p[2] * (k - 1)], 1)
                }
                ("cross", _) => {
                    let q = native(rt, a(1));
                    let c = [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
                    ([c[0] - p[0], c[1] - p[1], c[2] - p[2]], 1)
                }
                _ => {
                    crate::miss(rt, key);
                    return V::NULL;
                }
            };
            if n == 0 || d == [0; 3] {
                return a(0);
            }
            let q = [p[0] + d[0] * n, p[1] + d[1] * n, p[2] + d[2] * n].map(|c| c as i32 as i64);
            let class = if rt.instance_of(a(0), BLOCK_POS) { BLOCK_POS } else { "net/minecraft/core/Vec3i" };
            rt.alloc(class, Data::Native("BlockPos", [q[0], q[1], q[2], 0]))
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
                    crate::miss(rt, key);
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
        // `BlockPos.getCenter()` and `getBottomCenter()`: `Vec3.atCenterOf(this)`, `atBottomCenterOf(this)`.
        "net/minecraft/core/BlockPos.getCenter.()Lnet/minecraft/world/phys/Vec3;" | "net/minecraft/core/BlockPos.getBottomCenter.()Lnet/minecraft/world/phys/Vec3;" => {
            let p = native(rt, a(0));
            let up = if key.contains(".getCenter.") { 0.5 } else { 0.0 };
            crate::api_common::vec3(rt, p[0] as f64 + 0.5, p[1] as f64 + up, p[2] as f64 + 0.5)
        }
        "net/minecraft/core/BlockPos.atY.(I)Lnet/minecraft/core/BlockPos;" => {
            let p = native(rt, a(0));
            block_pos(rt, p[0], i64::from(a(1).i()), p[2])
        }
        // `BlockPos.rotate(rotation)` about the origin: a quarter turn clockwise takes (x, z) to
        // (-z, x); `NONE` gives the position itself.
        "net/minecraft/core/BlockPos.rotate.(Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/core/BlockPos;" => {
            let p = native(rt, a(0)).map(|c| i64::from(c as i32));
            let neg = |c: i64| i64::from((c as i32).wrapping_neg());
            match rt.get_field(a(1), "$ordinal").i() {
                1 => block_pos(rt, neg(p[2]), p[1], p[0]),
                2 => block_pos(rt, neg(p[0]), p[1], neg(p[2])),
                3 => block_pos(rt, p[2], p[1], neg(p[0])),
                _ => a(0),
            }
        }
        // `BlockPos.clampLocationWithin(v)`: the point kept a hair inside the block, the low bound
        // summed in floats as vanilla's is.
        "net/minecraft/core/BlockPos.clampLocationWithin.(Lnet/minecraft/world/phys/Vec3;)Lnet/minecraft/world/phys/Vec3;" => {
            let p = native(rt, a(0));
            let v = [rt.get_field(a(1), "x").d(), rt.get_field(a(1), "y").d(), rt.get_field(a(1), "z").d()];
            let c: [f64; 3] = std::array::from_fn(|i| {
                let (low, high) = (f64::from(p[i] as i32 as f32 + 1.0e-5), p[i] as f64 + 1.0 - 9.999_999_747_378_752e-6);
                if v[i] < low { low } else { v[i].min(high) }
            });
            crate::api_common::vec3(rt, c[0], c[1], c[2])
        }
        // `Vec3i`'s text: `toShortString` "x, y, z"; `toString` Guava's `toStringHelper`, as the
        // runtime writes any position (`BlockPos{x=1, y=2, z=3}`).
        "net/minecraft/core/Vec3i.toShortString.()Ljava/lang/String;" => {
            let p = native(rt, a(0));
            rt.string(&format!("{}, {}, {}", p[0], p[1], p[2]))
        }
        "net/minecraft/core/Vec3i.toString.()Ljava/lang/String;" => {
            let text = rt.to_java_string(a(0), 'L');
            rt.string(&text)
        }
        // `Vec3i.compareTo`: by y, then z, then x.
        "net/minecraft/core/Vec3i.compareTo.(Lnet/minecraft/core/Vec3i;)I" | "net/minecraft/core/Vec3i.compareTo.(Ljava/lang/Object;)I" => {
            let (p, q) = (native(rt, a(0)), native(rt, a(1)));
            let d = |i: usize| (p[i] as i32).wrapping_sub(q[i] as i32);
            V::I(if p[1] != q[1] {
                d(1)
            } else if p[2] != q[2] {
                d(2)
            } else {
                d(0)
            })
        }
        // `distToCenterSqr` from the block's centre to a point (or a `Position`: a `Vec3`), and
        // `closerToCenterThan(position, distance)`: that below `distance²`.
        "net/minecraft/core/Vec3i.distToCenterSqr.(DDD)D"
        | "net/minecraft/core/Vec3i.distToCenterSqr.(Lnet/minecraft/core/Position;)D"
        | "net/minecraft/core/Vec3i.closerToCenterThan.(Lnet/minecraft/core/Position;D)Z" => {
            let p = native(rt, a(0));
            let to = if key.contains("(DDD)") { [a(1).d(), a(2).d(), a(3).d()] } else { ["x", "y", "z"].map(|f| rt.get_field(a(1), f).d()) };
            let d: [f64; 3] = std::array::from_fn(|i| p[i] as f64 + 0.5 - to[i]);
            let sq = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if key.contains(".closerToCenterThan.") { V::I((sq < a(2).d() * a(2).d()) as i32) } else { V::D(sq) }
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
            rt.class_literal(c)
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
        // Java's constructors: `(String)`, `(String, Throwable)`, `(Throwable)` (whose message is
        // the cause's `toString()`, as `Throwable(Throwable)` sets it) and `()`.
        k if exception_class(k) && k.contains(".<init>.") => {
            let desc = k.rsplit('.').next().unwrap_or("");
            if desc.starts_with("(Ljava/lang/String;") {
                rt.put_field(a(0), "message", a(1));
            }
            if desc.starts_with("(Ljava/lang/String;Ljava/lang/Throwable;") {
                rt.put_field(a(0), "cause", a(2));
            }
            if desc.starts_with("(Ljava/lang/Throwable;)") {
                let cause = a(1);
                rt.put_field(a(0), "cause", cause);
                if cause.r() != 0 {
                    let text = throwable_string(rt, cause);
                    let m = rt.string(&text);
                    rt.put_field(a(0), "message", m);
                }
            }
            V::Void
        }
        k if exception_class(k) && (k.contains(".getMessage.") || k.contains(".getLocalizedMessage.")) => rt.get_field(a(0), "message"),
        k if exception_class(k) && k.contains(".getCause.") => match rt.get_field(a(0), "cause") {
            V::Void => V::NULL,
            c => c,
        },
        k if exception_class(k) && k.contains(".initCause.") => {
            rt.put_field(a(0), "cause", a(1));
            a(0)
        }
        k if exception_class(k) && k.contains(".toString.()") => {
            let text = throwable_string(rt, a(0));
            rt.string(&text)
        }
        // `array.clone()` (an enum's `values()`): a shallow copy.
        k if k.starts_with('[') && k.contains(".clone.") => {
            let this = a(0);
            match rt.heap.get(this.r() as usize).map(|o| (o.class, o.data.clone())) {
                Some((class, d @ Data::Array(_))) => rt.alloc(class, d),
                _ => V::NULL,
            }
        }
        _ => {
            TO_CHAIN.with(|c| c.borrow_mut().insert(id, ()));
            through_chain(rt, key, args)
        }
    }
}

/// A call no arm of [`call_exact`] takes: the modules that claim its key, then what is left.
fn through_chain(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    match chain(rt, key, args) {
        Some(v) => v,
        None if plain_super_init(rt, key, args) => V::Void,
        None => match crate::functions::invoke_through(rt, key, args).or_else(|| crate::annotations::element(rt, key, args)) {
            Some(v) => v,
            None => {
                crate::miss(rt, key);
                V::Void
            }
        },
    }
}

/// A vanilla constructor without arguments that a mod class's constructor runs as its `super()`
/// (`SimpleCriterionTrigger()`, DFU's `MapCodec()` and `MapDecoder.Implementation()`): what it
/// would set is vanilla's own state, read only by vanilla's methods, which answer (or miss) on
/// their own; nothing is lost by keeping none. So is a criterion trigger the mod makes itself
/// (`new PlayerTrigger()`, registered): its listeners are the advancements' state.
fn plain_super_init(rt: &Rt, key: &str, args: &[V]) -> bool {
    let Some(owner) = key.strip_suffix(".<init>.()V") else { return false };
    let Some(&this) = args.first() else { return false };
    let class = rt.class_of(this);
    let game = owner.starts_with("net/minecraft/") || owner.starts_with("net/neoforged/") || owner.starts_with("com/mojang/serialization/");
    (game && class != owner && rt.supers.contains_key(class))
        || (class == owner && owner.starts_with("net/minecraft/advancements/critereon/") && owner.ends_with("Trigger"))
}

type Module = fn(&mut Rt, &'static str, &[V]) -> Option<V>;

/// `Block.getSeed(state, pos)`: a mod block overrides it by name and descriptor.
const BLOCK_SEED: &str = "getSeed.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;)J";
const BLOCK_SEED_KEY: &str = "net/minecraft/world/level/block/Block.getSeed.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;)J";

/// `Mth.getSeed(pos)`: the seed vanilla draws a block's random look from.
fn pos_seed(rt: &mut Rt, pos: V) -> V {
    let p = pos_of(rt, pos);
    call(rt, "net/minecraft/util/Mth.getSeed.(III)J", &[V::I(p[0]), V::I(p[1]), V::I(p[2])])
}

/// The modules a call is offered to, in order, when no arm above takes its key: the first that
/// answers does.
const CHAIN: [Module; 80] = [
    crate::diodes::call,
    crate::rails::call,
    crate::fake_players::call,
    crate::attribute_suppliers::call,
    crate::block_breaking::call,
    crate::loot_params::call,
    mutable_pos,
    crate::worldgen::call,
    crate::mixin::call,
    crate::attachments::call,
    crate::sounds::call,
    crate::particles::call,
    crate::food::call,
    crate::attributes::call,
    crate::mob_effects::call,
    crate::vitals::call,
    crate::level_events::call,
    crate::enchantments::call,
    crate::item_use::call,
    crate::network::call,
    crate::fluids::call,
    crate::client_extensions::call,
    crate::colors::call,
    crate::options::call,
    crate::tanks::call_key,
    crate::energy::call_key,
    crate::data_maps::call,
    crate::block_entity_data::call,
    crate::state_queries::call,
    crate::fluid_wrappers::call_key,
    crate::codecs::call,
    crate::recipes::call,
    crate::handlers::call_key,
    crate::item_wrappers::call_key,
    crate::components::call_key,
    crate::capabilities::call_key,
    crate::projectiles::call,
    crate::mod_entities::call_key,
    crate::entities::call_key,
    crate::creative::call,
    crate::tooltips::call,
    crate::damage::call,
    crate::player_advancements::call,
    crate::world_events::call,
    crate::made_entities::call,
    crate::vanilla_entities::call,
    crate::commands::call,
    crate::brigadier::call,
    crate::functions::call,
    crate::gamerules::call,
    crate::value_providers::call,
    crate::nightconfig::call,
    crate::rotation::call,
    crate::structures::call,
    crate::crops::call,
    crate::blocks::call,
    crate::shapes::call,
    crate::events::call,
    crate::menus::call,
    crate::container_screens::call,
    crate::widgets::call,
    crate::screens::call,
    crate::gui::call,
    crate::neo::call,
    crate::files::call,
    crate::resources::call,
    crate::gson::call,
    crate::jregex::call,
    crate::jlang::call,
    crate::jutil::call,
    crate::tags::call,
    crate::baked_models::call,
    crate::quads::call,
    crate::models::call,
    crate::entity_render::call,
    crate::camera::call,
    crate::render::call,
    world,
    crate::palette::call,
    crate::records::call,
];

/// Which keys each module of [`CHAIN`] may answer, in the same order ([`crate::sounds::claims`]...):
/// a pure function of the key, so its answer is kept per key.
type Claims = fn(&str, &str, &str) -> bool;
const CLAIMS: [Claims; 80] = [
    crate::diodes::claims,
    crate::rails::claims,
    crate::fake_players::claims,
    crate::attribute_suppliers::claims,
    crate::block_breaking::claims,
    crate::loot_params::claims,
    |owner, _, _| owner == MUTABLE_POS,
    crate::worldgen::claims,
    crate::mixin::claims,
    crate::attachments::claims,
    crate::sounds::claims,
    crate::particles::claims,
    crate::food::claims,
    crate::attributes::claims,
    crate::mob_effects::claims,
    crate::vitals::claims,
    crate::level_events::claims,
    crate::enchantments::claims,
    crate::item_use::claims,
    crate::network::claims,
    crate::fluids::claims,
    crate::client_extensions::claims,
    crate::colors::claims,
    crate::options::claims,
    crate::tanks::claims,
    crate::energy::claims,
    crate::data_maps::claims,
    crate::block_entity_data::claims,
    crate::state_queries::claims,
    crate::fluid_wrappers::claims,
    crate::codecs::claims,
    crate::recipes::claims,
    crate::handlers::claims,
    crate::item_wrappers::claims,
    crate::components::claims,
    crate::capabilities::claims,
    crate::projectiles::claims,
    crate::mod_entities::claims,
    crate::entities::claims,
    crate::creative::claims,
    crate::tooltips::claims,
    crate::damage::claims,
    crate::player_advancements::claims,
    crate::world_events::claims,
    crate::made_entities::claims,
    crate::vanilla_entities::claims,
    crate::commands::claims,
    crate::brigadier::claims,
    crate::functions::claims,
    crate::gamerules::claims,
    crate::value_providers::claims,
    crate::nightconfig::claims,
    crate::rotation::claims,
    crate::structures::claims,
    crate::crops::claims,
    crate::blocks::claims,
    crate::shapes::claims,
    crate::events::claims,
    crate::menus::claims,
    crate::container_screens::claims,
    crate::widgets::claims,
    crate::screens::claims,
    crate::gui::claims,
    crate::neo::claims,
    crate::files::claims,
    crate::resources::claims,
    crate::gson::claims,
    crate::jregex::claims,
    crate::jlang::claims,
    crate::jutil::claims,
    crate::tags::claims,
    crate::baked_models::claims,
    crate::quads::claims,
    crate::models::claims,
    crate::entity_render::claims,
    crate::camera::claims,
    crate::render::claims,
    // The world's calls end in `api_common`'s, which take most of vanilla's classes.
    |_, _, _| true,
    crate::palette::claims,
    crate::records::claims,
];

const MODULE_NAMES: [&str; 80] = [
    "diodes",
    "rails",
    "fake_players",
    "attribute_suppliers",
    "block_breaking",
    "loot_params",
    "mutable_pos",
    "worldgen",
    "mixin",
    "attachments",
    "sounds",
    "particles",
    "food",
    "attributes",
    "mob_effects",
    "vitals",
    "level_events",
    "enchantments",
    "item_use",
    "network",
    "fluids",
    "client_extensions",
    "colors",
    "options",
    "tanks",
    "energy",
    "data_maps",
    "block_entity_data",
    "state_queries",
    "fluid_wrappers",
    "codecs",
    "recipes",
    "handlers",
    "item_wrappers",
    "components",
    "capabilities",
    "projectiles",
    "mod_entities",
    "entities",
    "creative",
    "tooltips",
    "damage",
    "player_advancements",
    "world_events",
    "made_entities",
    "vanilla_entities",
    "commands",
    "brigadier",
    "functions",
    "gamerules",
    "value_providers",
    "nightconfig",
    "rotation",
    "structures",
    "crops",
    "blocks",
    "shapes",
    "events",
    "menus",
    "container_screens",
    "widgets",
    "screens",
    "gui",
    "neo",
    "files",
    "resources",
    "gson",
    "jregex",
    "jlang",
    "jutil",
    "tags",
    "baked_models",
    "quads",
    "models",
    "entity_render",
    "camera",
    "render",
    "world",
    "palette",
    "records",
];

thread_local! {
    /// By key (its address and length): the modules of [`CHAIN`] that claim it, as bits.
    static CLAIMED: std::cell::RefCell<crate::HashMap<(usize, usize), u128>> = std::cell::RefCell::new(crate::HashMap::default());
}

/// The modules of [`CHAIN`] that claim `key`, as bits (all of them for a key that isn't
/// `owner.name.desc`).
fn claimed(key: &'static str) -> u128 {
    let id = (key.as_ptr() as usize, key.len());
    if let Some(bits) = CLAIMED.with(|c| c.borrow().get(&id).copied()) {
        return bits;
    }
    let bits = match crate::parts(key) {
        Some((owner, name, desc)) => CLAIMS.iter().enumerate().filter(|(_, claims)| claims(owner, name, desc)).fold(0u128, |bits, (i, _)| bits | 1 << i),
        None => (1u128 << CHAIN.len()) - 1,
    };
    CLAIMED.with(|c| c.borrow_mut().insert(id, bits));
    bits
}

/// Whether every call checks that the module answering it claims its key
/// (`HORIZON_COMPAT_CHECK_DISPATCH=1`; always in tests): it is then offered to every module.
fn checking() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(test) || *CHECK.get_or_init(|| std::env::var_os("HORIZON_COMPAT_CHECK_DISPATCH").is_some_and(|v| v == "1"))
}

/// Offers the call to each module of [`CHAIN`] that claims its key, in order: the first that
/// answers does.
fn chain(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let claimed = claimed(key);
    if checking() {
        return checked_chain(rt, key, args, claimed);
    }
    let mut left = claimed;
    while left != 0 {
        let i = left.trailing_zeros() as usize;
        left &= left - 1;
        if let Some(v) = CHAIN[i](rt, key, args) {
            return Some(v);
        }
    }
    None
}

/// [`chain`] past every module, telling (or, in tests, failing) when one that answers doesn't
/// claim the key: the claims would then have kept the call from it.
fn checked_chain(rt: &mut Rt, key: &'static str, args: &[V], claimed: u128) -> Option<V> {
    for (i, module) in CHAIN.iter().enumerate() {
        if let Some(v) = module(rt, key, args) {
            if claimed & (1u128 << i) == 0 {
                let message = format!("horizon-compat: {} answered {key} without claiming it", MODULE_NAMES[i]);
                assert!(!cfg!(test), "{message}");
                thread_local! {
                    static TOLD: std::cell::RefCell<crate::HashMap<(usize, usize), ()>> = std::cell::RefCell::new(crate::HashMap::default());
                }
                if TOLD.with(|t| t.borrow_mut().insert((key.as_ptr() as usize, i), ()).is_none()) {
                    eprintln!("{message}");
                }
            }
            return Some(v);
        }
    }
    None
}

const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const BLOCK: &str = "net/minecraft/world/level/block/Block";
const STATE_DEFINITION: &str = "net/minecraft/world/level/block/state/StateDefinition";
const DATA_STORAGE: &str = "net/minecraft/world/level/storage/DimensionDataStorage";
const SET_PLACED_BY_DESC: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;)V";
pub(crate) const UPDATE_SHAPE: &str = "updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const UPDATE_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
pub(crate) const CAN_SURVIVE: &str = "canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
pub(crate) const CAN_SURVIVE_KEY: &str =
    "net/minecraft/world/level/block/state/BlockBehaviour.canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
const SAVED_DATA: &str = "net/minecraft/world/level/saveddata/SavedData";
const LEVEL_DATA: &str = "net/minecraft/world/level/storage/LevelData";
const LEVEL_CHUNK: &str = "net/minecraft/world/level/chunk/LevelChunk";
const CHUNK_ACCESS: &str = "net/minecraft/world/level/chunk/ChunkAccess";
const SAVED_DATA_FACTORY: &str = "net/minecraft/world/level/saveddata/SavedData$Factory";
const CHUNK_CACHE: &str = "net/minecraft/server/level/ServerChunkCache";
const CHUNK_SOURCE: &str = "net/minecraft/world/level/chunk/ChunkSource";
const CHUNK_GENERATOR: &str = "net/minecraft/world/level/chunk/ChunkGenerator";
const NOISE_GENERATOR: &str = "net/minecraft/world/level/levelgen/NoiseBasedChunkGenerator";

/// A vanilla dimension's sea level, lowest y and height, as its noise settings give them
/// (`NoiseGeneratorSettings.overworld/nether/end`).
fn noise_heights(dimension: &str) -> Option<(i32, i32, i32)> {
    match dimension {
        "minecraft:overworld" => Some((63, -64, 384)),
        "minecraft:the_nether" => Some((32, 0, 128)),
        "minecraft:the_end" => Some((0, 0, 128)),
        _ => None,
    }
}

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

const STATE_USE_ITEM_ON: &str = "(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
const STATE_USE_WITHOUT_ITEM: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/InteractionResult;";
const STATE_ATTACK: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)V";
const USE_ITEM_ON: &str = "useItemOn.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
const USE_ITEM_ON_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.useItemOn.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
const USE_WITHOUT_ITEM: &str = "useWithoutItem.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/InteractionResult;";
const USE_WITHOUT_ITEM_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.useWithoutItem.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/InteractionResult;";
const ATTACK: &str = "attack.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)V";
const ATTACK_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.attack.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)V";
const CAN_BE_REPLACED: &str = "canBeReplaced.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/item/context/BlockPlaceContext;)Z";
const CAN_BE_REPLACED_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.canBeReplaced.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/item/context/BlockPlaceContext;)Z";
const GET_OCCLUSION: &str = "getOcclusionShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_OCCLUSION_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getOcclusionShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_SHAPE: &str = "getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";

/// `state.getCollisionShape(level, pos)` and `state.getShape(level, pos)`, args [state, level, pos,
/// context]: for a collision, the game's own boxes when `state` is the block at `pos` (vanilla's
/// and the content mods' shapes); else the mod block's own `getShape`; else, for an outline, the
/// game's when `state` is the block at `pos`; else a full block (air, none).
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
    // A vanilla block's outline, as the game has it where it stands.
    if !collision
        && let Some(h) = rt.host()
        && h.block_state(at) == text
        && let Some(boxes) = h.outline_boxes(at)
    {
        return crate::shapes::shape(rt, &boxes);
    }
    crate::shapes::shape(rt, &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]])
}

const GET_SUPPORT_SHAPE: &str = "getBlockSupportShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_SUPPORT_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getBlockSupportShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_COLLISION: &str = "getCollisionShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_COLLISION_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getCollisionShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_BLOCK_STATE: &str = "net/minecraft/world/level/BlockGetter.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
/// `SupportType`'s ordinals and `Direction.UP`'s.
const SUPPORT_CENTER: usize = 1;
const SUPPORT_RIGID: usize = 2;
const UP: usize = 1;

/// `state.isFaceSturdy(level, pos, direction, support)` for state `text` (`support` by
/// `SupportType`'s ordinal). A vanilla block's is the game's (vanilla's own answer, which doesn't
/// depend on where the block stands). A mod block's comes from its [`support_shape`], tested as
/// `SupportType` tests a face.
pub(crate) fn face_sturdy(rt: &mut Rt, text: &str, [state, level, pos]: [V; 3], direction: usize, support: usize) -> bool {
    if own_block(rt, text).is_none()
        && let Some(f) = rt.host().and_then(|h| h.block_facts(text))
    {
        return f.sturdy & 1 << (support.min(2) * 6 + direction.min(5)) != 0;
    }
    let shape = support_shape(rt, text, [state, level, pos]);
    let boxes = crate::shapes::boxes(rt, shape).unwrap_or_default();
    crate::shapes::sturdy(&boxes, direction.min(5), support.min(2))
}

/// The mod's own block object behind state `text` and its class: one the mod made itself.
fn own_block(rt: &Rt, text: &str) -> Option<(V, &'static str)> {
    crate::glue::mod_block(rt, text).filter(|(b, _)| rt.str_of(*b).is_none())
}

/// `state.getBlockSupportShape(level, pos)` for state `text`, as 1.21.1 finds it: a mod block's
/// own `getBlockSupportShape`; else that of the vanilla class it is or extends, for a class with
/// its own ([`crate::blocks::SUPPORT_SHAPED`]: leaves hold nothing up, a snow layer by its
/// layers); else, as `BlockBehaviour`'s, its [`collision_shape`].
fn support_shape(rt: &mut Rt, text: &str, [state, level, pos]: [V; 3]) -> V {
    let own = own_block(rt, text);
    if let Some((blk, class)) = own
        && let Some(s) = own_shape(rt, class, GET_SUPPORT_SHAPE_KEY, GET_SUPPORT_SHAPE, &[blk, state, level, pos])
    {
        return s;
    }
    let class = match own {
        Some((_, class)) => crate::rotation::vanilla_base(rt, class),
        None => {
            let (name, _) = crate::host::split_state(text);
            let table = &crate::vanilla_classes::BLOCKS;
            table.binary_search_by(|(id, _)| (*id).cmp(name)).map_or("net/minecraft/world/level/block/Block", |i| table[i].1)
        }
    };
    match crate::blocks::declaring_class(class, &crate::blocks::SUPPORT_SHAPED) {
        Some(c) if c != "BlockBehaviour" => declared_support_shape(rt, c, text, [state, level, pos]),
        _ => collision_shape(rt, text, own, [state, level, pos]),
    }
}

/// The support shape vanilla class `class` (one of [`crate::blocks::SUPPORT_SHAPED`]) declares,
/// for state `text`: 1.21.1's, or a hanging sign's, its shape.
fn declared_support_shape(rt: &mut Rt, class: &str, text: &str, [state, level, pos]: [V; 3]) -> V {
    let (_, props) = crate::host::split_state(text);
    match crate::blocks::vanilla_support_shape(class, &props) {
        Some(pixels) => {
            let boxes: Vec<crate::shapes::Box3> = pixels.iter().map(|b| b.map(|v| v / 16.0)).collect();
            crate::shapes::shape(rt, &boxes)
        }
        None => {
            let ctx = crate::shapes::empty_context(rt);
            state_shape(rt, text, false, [state, level, pos, ctx])
        }
    }
}

/// `block.getCollisionShape(state, level, pos, CollisionContext.empty())` for state `text`, of
/// `own`, the mod's block object and class when it made one: its own `getCollisionShape`; else
/// [`behaviour_collision`] when its vanilla class has `BlockBehaviour`'s; else [`state_shape`]'s.
fn collision_shape(rt: &mut Rt, text: &str, own: Option<(V, &'static str)>, [state, level, pos]: [V; 3]) -> V {
    let ctx = crate::shapes::empty_context(rt);
    if let Some((blk, class)) = own {
        if let Some(s) = own_shape(rt, class, GET_COLLISION_KEY, GET_COLLISION, &[blk, state, level, pos, ctx]) {
            return s;
        }
        let base = crate::rotation::vanilla_base(rt, class);
        if crate::blocks::declaring_class(base, &crate::blocks::COLLISION_SHAPED) == Some("BlockBehaviour") {
            return behaviour_collision(rt, blk, [state, level, pos]);
        }
    }
    state_shape(rt, text, true, [state, level, pos, ctx])
}

/// `BlockBehaviour.getCollisionShape(state, level, pos, context)` of block `blk`: none when its
/// properties say `noCollission`, else the state's shape (`state.getShape(level, pos)`, the
/// block's own `getShape`).
fn behaviour_collision(rt: &mut Rt, blk: V, [state, level, pos]: [V; 3]) -> V {
    let props = rt.get_field(blk, "$properties");
    if props.r() != 0 && rt.get_field(props, "$noCollision").i() != 0 {
        return crate::shapes::shape(rt, &[]);
    }
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    let ctx = crate::shapes::empty_context(rt);
    state_shape(rt, &text, false, [state, level, pos, ctx])
}

/// The mod's own `short` (`key`) on its block of class `class`, when the class overrides it and
/// it answers with a shape.
fn own_shape(rt: &mut Rt, class: &str, key: &'static str, short: &'static str, args: &[V]) -> Option<V> {
    if !rt.overrides.is_some_and(|o| o(class, short)) {
        return None;
    }
    let shape = (rt.vcall?)(rt, key, short, args);
    if rt.thrown.is_none() && crate::shapes::boxes(rt, shape).is_some() {
        return Some(shape);
    }
    rt.report_thrown(short.split('.').next().unwrap_or(short));
    None
}

/// The block state `level` has at `pos` (a mod's own level answers for itself) and its text.
fn level_state(rt: &mut Rt, level: V, pos: V) -> (V, String) {
    let state = crate::menus::virt(rt, GET_BLOCK_STATE, level, &[pos]);
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    (state, text)
}

/// Guava's `DoubleMath.fuzzyEquals`.
fn fuzzy_equals(x: f64, y: f64, tolerance: f64) -> bool {
    (x - y).abs() <= tolerance || x == y || (x.is_nan() && y.is_nan())
}

/// The 26.3 id of the block or item a 1.21.1 `Blocks` or `Items` field holds: its lower-cased name,
/// but for the chain 26.3 renamed and 1.21.1's own odd names (`Blocks.POTTED_AZALEA` is
/// `potted_azalea_bush`, `Items.CUT_STANDSTONE_SLAB` is `cut_sandstone_slab`). Checked against every
/// static field of the two classes in 1.21.1's jar: no other differs.
pub fn field_id(field: &str) -> String {
    match field {
        "CHAIN" => "minecraft:iron_chain".into(),
        "POTTED_AZALEA" => "minecraft:potted_azalea_bush".into(),
        "POTTED_FLOWERING_AZALEA" => "minecraft:potted_flowering_azalea_bush".into(),
        "CUT_STANDSTONE_SLAB" => "minecraft:cut_sandstone_slab".into(),
        f => format!("minecraft:{}", f.to_ascii_lowercase()),
    }
}

/// A block value: the mod's own Block object when it registered one by that name.
pub fn block(rt: &mut Rt, name: &str) -> V {
    if let Some(v) = rt.registered.get(&("block".to_owned(), name.to_owned())) {
        return *v;
    }
    if let Some(b) = unregistered_block(rt, name) {
        return b;
    }
    // Interned, so `state.getBlock() == Blocks.STONE` holds as in Java.
    let v = rt.alloc(BLOCK, Data::Str(name.to_owned()));
    rt.registered.insert(("block".to_owned(), name.to_owned()), v);
    v
}

/// The name of a block value (vanilla or the mod's): a mod block not registered yet has a name of its
/// own until it is ([`UNREGISTERED`]).
pub(crate) fn block_name(rt: &Rt, b: V) -> String {
    match rt.str_of(b).map(str::to_owned).or_else(|| rt.registered_name(b, "block").map(str::to_owned)) {
        Some(n) => n,
        None if b.r() != 0 => format!("{UNREGISTERED}{}", b.r()),
        None => String::new(),
    }
}

/// The name a mod block has before it is registered, its object's number after it. A block's states
/// are its name and properties, and a block exists before its name does: Registrate makes the block,
/// then registers it, and constructors take `defaultBlockState()` (Copycats+ keeps them). In Java a
/// state points at its block, so `state.getBlock()` finds the block either way; here the name does,
/// and registering renames those states in place ([`adopt_names`]): the same objects, so what kept
/// them sees the registered name.
pub(crate) const UNREGISTERED: &str = "horizon_unregistered:block_";

/// The mod block an [`UNREGISTERED`] name stands for.
pub(crate) fn unregistered_block(rt: &Rt, name: &str) -> Option<V> {
    let r: u32 = name.strip_prefix(UNREGISTERED)?.parse().ok()?;
    (r != 0 && rt.heap.get(r as usize).is_some_and(|o| matches!(o.data, Data::Fields(_)))).then_some(V::R(r))
}

/// Keeps a new state of a block not registered yet, for [`adopt_names`] (and the block alive with it).
pub(crate) fn note_unregistered(rt: &mut Rt, text: &str, state: V) {
    let (name, _) = crate::host::split_state(text);
    let Some(b) = unregistered_block(rt, name) else { return };
    let all = rt.intern("compat:unregistered-states".into(), |rt| rt.alloc("java/util/ArrayList", Data::List(Vec::new())));
    let pair = rt.alloc("java/util/List", Data::List(vec![b, state]));
    if let Some(Data::List(v)) = rt.heap.get_mut(all.r() as usize).map(|o| &mut o.data) {
        v.push(pair);
    }
}

/// Renames the states kept under an [`UNREGISTERED`] name to their block's registered name, for
/// every block registered since. A state with that name already made keeps its place in the table.
pub(crate) fn adopt_names(rt: &mut Rt) {
    let Some(&all) = rt.interned.get("compat:unregistered-states") else { return };
    let pairs = match rt.heap.get(all.r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) if !v.is_empty() => v.clone(),
        _ => return,
    };
    let mut waiting = Vec::new();
    for pair in pairs {
        let (b, state) = match rt.heap.get(pair.r() as usize).map(|o| &o.data) {
            Some(Data::List(v)) if v.len() == 2 => (v[0], v[1]),
            _ => continue,
        };
        let Some(name) = rt.registered_name(b, "block").map(str::to_owned) else {
            waiting.push(pair);
            continue;
        };
        let Some(old) = rt.str_of(state).map(str::to_owned) else { continue };
        let Some(rest) = old.strip_prefix(&format!("{UNREGISTERED}{}", b.r())) else { continue };
        let new = format!("{name}{rest}");
        if rt.interned.get(&format!("state/{old}")) == Some(&state) {
            rt.interned.remove(&format!("state/{old}"));
        }
        rt.interned.entry(format!("state/{new}")).or_insert(state);
        if let Some(o) = rt.heap.get_mut(state.r() as usize) {
            o.data = Data::Str(new);
        }
    }
    if let Some(Data::List(v)) = rt.heap.get_mut(all.r() as usize).map(|o| &mut o.data) {
        *v = waiting;
    }
}

/// `BlockState.updateShape(direction, neighbour, level, pos, neighbourPos)` (`args`, the state
/// first): the mod block's own `updateShape`; a block without one keeps its state.
pub(crate) fn update_shape(rt: &mut Rt, args: [V; 6]) -> V {
    let text = rt.str_of(args[0]).unwrap_or("").to_owned();
    match crate::glue::mod_block(rt, &text) {
        Some((b, class)) if rt.overrides.is_some_and(|o| o(class, UPDATE_SHAPE)) => match rt.vcall {
            Some(vc) => vc(rt, UPDATE_SHAPE_KEY, UPDATE_SHAPE, &[b, args[0], args[1], args[2], args[3], args[4], args[5]]),
            None => args[0],
        },
        _ => args[0],
    }
}

/// `recv.method(args)` (the receiver first) as the JVM dispatches it: the mod class's override
/// where the receiver's class has one, else the API's. `(owner_key, key)`: the call's full key and
/// its `name.desc`.
pub(crate) fn virtual_call(rt: &mut Rt, (owner_key, key): (&'static str, &'static str), args: &[V]) -> V {
    match rt.vcall {
        Some(vc) => vc(rt, owner_key, key, args),
        None => call(rt, owner_key, args),
    }
}

pub(crate) fn pos_of(rt: &Rt, v: V) -> [i32; 3] {
    let h = native(rt, v);
    [h[0] as i32, h[1] as i32, h[2] as i32]
}

fn qualified(rt: &Rt, name: &str) -> String {
    if name.contains(':') { name.to_owned() } else { format!("{}:{name}", rt.namespace) }
}

/// A name registered through `register`, a `DeferredRegister`: in the register's own namespace (the
/// mod id it was created with; an engine holding several mods has several).
fn qualified_in(rt: &Rt, register: V, name: &str) -> String {
    match rt.str_of(rt.get_field(register, "$ns")).filter(|ns| !ns.is_empty()) {
        Some(ns) if !name.contains(':') => format!("{ns}:{name}"),
        _ => qualified(rt, name),
    }
}

/// Whether `owner` is one of the level's interfaces or classes (`Level`, `LevelAccessor`,
/// `BlockGetter`...): what reads and writes the game's blocks.
pub(crate) fn is_level(owner: &str) -> bool {
    owner.starts_with("net/minecraft/")
        && (owner.ends_with("Level")
            || owner.ends_with("LevelAccessor")
            || owner.ends_with("LevelReader")
            || owner.ends_with("BlockGetter")
            || owner.ends_with("LevelWriter")
            || owner.ends_with("BlockAndTintGetter")
            || owner.ends_with("SignalGetter")
            || owner.ends_with("LevelTimeAccess"))
}

const CHUNK_STATUS: &str = "net/minecraft/world/level/chunk/status/ChunkStatus";
/// 1.21.1's chunk statuses in their order, by their registry names (`ChunkStatus`' fields).
const CHUNK_STATUSES: [&str; 12] =
    ["empty", "structure_starts", "structure_references", "biomes", "noise", "surface", "carvers", "features", "initialize_light", "light", "spawn", "full"];

/// The chunk status at `index` in vanilla's order, one object each (`ChunkStatus.FULL`...).
fn chunk_status(rt: &mut Rt, index: usize) -> V {
    let index = index.min(CHUNK_STATUSES.len() - 1);
    rt.intern(format!("compat:chunk-status/{index}"), |rt| {
        let o = rt.new_object(CHUNK_STATUS);
        rt.put_field(o, "$index", V::I(index as i32));
        o
    })
}

/// A chunk status's calls: where it stands in vanilla's order, its name and its parent; `byName`
/// gives `empty` for a name it doesn't know, as vanilla's defaulted registry does.
fn chunk_status_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let index = |rt: &Rt, v: V| rt.get_field(v, "$index").i();
    Some(match name {
        "getIndex" => V::I(index(rt, a(0))),
        "getParent" => chunk_status(rt, (index(rt, a(0)) - 1).max(0) as usize),
        "isOrAfter" => V::I((index(rt, a(0)) >= index(rt, a(1))) as i32),
        "isAfter" => V::I((index(rt, a(0)) > index(rt, a(1))) as i32),
        "isOrBefore" => V::I((index(rt, a(0)) <= index(rt, a(1))) as i32),
        "isBefore" => V::I((index(rt, a(0)) < index(rt, a(1))) as i32),
        "max" => {
            if index(rt, a(0)) > index(rt, a(1)) {
                a(0)
            } else {
                a(1)
            }
        }
        "getName" | "toString" => {
            let i = (index(rt, a(0)).max(0) as usize).min(CHUNK_STATUSES.len() - 1);
            rt.string(&format!("minecraft:{}", CHUNK_STATUSES[i]))
        }
        "byName" => {
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            let path = text.strip_prefix("minecraft:").unwrap_or(&text);
            chunk_status(rt, CHUNK_STATUSES.iter().position(|s| *s == path).unwrap_or(0))
        }
        "getChunkType" => {
            let full = index(rt, a(0)) as usize == CHUNK_STATUSES.len() - 1;
            let (constant, ordinal) = if full { ("LEVELCHUNK", 1) } else { ("PROTOCHUNK", 0) };
            api_enum(rt, "net/minecraft/world/level/chunk/status/ChunkType", constant, ordinal)
        }
        "getStatusList" => {
            let all = (0..CHUNK_STATUSES.len()).map(|i| chunk_status(rt, i)).collect();
            crate::jutil::new_list(rt, all)
        }
        _ => return None,
    })
}

/// The chunk of `level` at chunk coordinates `x`, `z`: a `LevelChunk` its level answers for.
fn level_chunk(rt: &mut Rt, level: V, x: i32, z: i32) -> V {
    let c = rt.new_object(LEVEL_CHUNK);
    rt.put_field(c, "$level", level);
    rt.put_field(c, "$x", V::I(x));
    rt.put_field(c, "$z", V::I(z));
    c
}

/// Registration, the level and block states: the API that reaches the game.
fn world(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    let level = is_level(owner);
    // Light, the sky, weather, redstone and scheduled ticks, from the host's world.
    if level && let Some(v) = crate::level_env::call(rt, name, desc, args) {
        return Some(v);
    }
    if crate::level_env::is_ticks(owner) {
        return crate::level_env::ticks_call(rt, name, args);
    }
    if owner == crate::level_env::BLOCK_IN_WORLD {
        return crate::level_env::block_in_world(rt, name, args);
    }
    if owner == CHUNK_STATUS {
        return chunk_status_call(rt, name, args);
    }
    Some(match (owner, name) {
        // NeoForge registration
        // `createDataComponents(registryKey, modid)`, or `(modid)` alone in older NeoForge.
        ("net/neoforged/neoforge/registries/DeferredRegister", "create" | "createDataComponents") => {
            let keyed = name == "create" || desc.starts_with("(Lnet/minecraft/resources/ResourceKey;");
            let registry = if keyed { rt.str_of(a(0)).unwrap_or("unknown").to_owned() } else { "data_component_type".to_owned() };
            let ns = rt.str_of(if keyed { a(1) } else { a(0) }).unwrap_or("").to_owned();
            if rt.namespace.is_empty() {
                rt.namespace = ns.clone();
            }
            let r = rt.new_object(owner);
            let reg = rt.string(&registry);
            rt.put_field(r, "$registry", reg);
            let ns = rt.string(&ns);
            rt.put_field(r, "$ns", ns);
            r
        }
        ("net/neoforged/neoforge/registries/DeferredRegister", "createBlocks" | "createItems") => {
            let ns = rt.str_of(a(0)).unwrap_or("").to_owned();
            if rt.namespace.is_empty() {
                rt.namespace = ns.clone();
            }
            let kind = if name == "createBlocks" { "block" } else { "item" };
            let r = rt.new_object(owner);
            let reg = rt.string(kind);
            rt.put_field(r, "$registry", reg);
            let ns = rt.string(&ns);
            rt.put_field(r, "$ns", ns);
            r
        }
        (o, "register") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") && desc.starts_with("(Ljava/lang/String;") => {
            let registry = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("unknown").to_owned();
            let id = qualified_in(rt, a(0), rt.str_of(a(1)).unwrap_or(""));
            let h = register_holder(rt, a(0), &registry, &id);
            // `register(name, Function<ResourceLocation, I>)`: made of the entry's own name.
            let make = if desc.starts_with("(Ljava/lang/String;Ljava/util/function/Function;") {
                let location = rt.string(&id);
                rt.lambda("compat:apply", vec![a(2), location])
            } else {
                a(2)
            };
            rt.suppliers.push((registry.clone(), id.clone(), make));
            h
        }
        (o, "registerBlock" | "registerItem") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let registry = if name == "registerBlock" { "block" } else { "item" };
            let id = qualified_in(rt, a(0), rt.str_of(a(1)).unwrap_or(""));
            // Without properties, NeoForge's own: `BlockBehaviour.Properties.of()`, `new Item.Properties()`.
            let props = if desc.contains("Properties;)") || args.len() > 3 {
                a(3)
            } else {
                rt.new_object(if registry == "block" { crate::blocks::BLOCK_PROPERTIES } else { crate::blocks::ITEM_PROPERTIES })
            };
            let apply = rt.lambda("compat:apply", vec![a(2), props]);
            let h = register_holder(rt, a(0), registry, &id);
            rt.suppliers.push((registry.into(), id.clone(), apply));
            h
        }
        // `registerSimpleItem(name[, properties])` is `registerItem(name, Item::new, properties)`,
        // `registerSimpleBlock(name, properties)` `registerBlock(name, Block::new, properties)`.
        (o, "registerSimpleItem" | "registerSimpleBlock") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let (registry, class, props_class) = if name == "registerSimpleBlock" {
                ("block", "net/minecraft/world/level/block/Block", crate::blocks::BLOCK_PROPERTIES)
            } else {
                ("item", "net/minecraft/world/item/Item", crate::blocks::ITEM_PROPERTIES)
            };
            let id = qualified_in(rt, a(0), rt.str_of(a(1)).unwrap_or(""));
            let props = if args.len() > 2 { a(2) } else { rt.new_object(props_class) };
            let make = crate::functions::constructor(rt, class, &format!("(L{props_class};)V"));
            let apply = rt.lambda("compat:apply", vec![make, props]);
            let h = register_holder(rt, a(0), registry, &id);
            rt.suppliers.push((registry.into(), id.clone(), apply));
            h
        }
        // `registerSimpleBlockItem(name, block[, properties])`, or `(blockHolder[, properties])`
        // under the block's own name: a `BlockItem` of the block, asked for when items are made.
        (o, "registerSimpleBlockItem") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let named = desc.starts_with("(Ljava/lang/String;");
            let (path, block, props) = if named {
                (rt.str_of(a(1)).unwrap_or("").to_owned(), a(2), a(3))
            } else {
                let id = rt.str_of(rt.get_field(a(1), "$id")).unwrap_or("").to_owned();
                (id.rsplit(':').next().unwrap_or("").to_owned(), a(1), a(2))
            };
            let given = if named { args.len() > 3 } else { args.len() > 2 };
            let props = if given { props } else { rt.new_object(crate::blocks::ITEM_PROPERTIES) };
            let id = qualified_in(rt, a(0), &path);
            let make = crate::functions::block_item(rt, block, props);
            let h = register_holder(rt, a(0), "item", &id);
            rt.suppliers.push(("item".into(), id.clone(), make));
            h
        }
        // NeoForge's `DataComponents.registerComponentType(name, builder -> ...)`: built now, as the
        // type is a plain object the mod configures, and registered under its name.
        (o, "registerComponentType") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let registry = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("data_component_type").to_owned();
            let id = qualified_in(rt, a(0), rt.str_of(a(1)).unwrap_or(""));
            let ty = crate::components::build_with(rt, a(2));
            rt.registered.insert((registry.clone(), id.clone()), ty);
            register_holder(rt, a(0), &registry, &id)
        }
        (o, "register") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => V::Void, // (IEventBus)
        (o, "getNamespace") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => rt.get_field(a(0), "$ns"),
        // Every holder made through it, in order (NeoForge's is an unmodifiable view).
        (o, "getEntries") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let entries = rt.get_field(a(0), "$entries");
            let items = if entries.r() == 0 { Vec::new() } else { crate::jutil::elements(rt, entries) };
            crate::jutil::new_list(rt, items)
        }
        ("net/neoforged/neoforge/registries/DeferredRegister", "getRegistryKey") => rt.get_field(a(0), "$registry"),
        ("net/neoforged/neoforge/registries/DeferredRegister", "getRegistryName") => {
            let r = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("").to_owned();
            let s = if r.contains(':') { r } else { format!("minecraft:{r}") };
            rt.string(&s)
        }
        // `addAlias(from, to)`: an old name that reads as the new one, as the registry's own.
        (o, "addAlias") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let registry = rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("unknown").to_owned();
            let (from, to) = (crate::neo::location(rt, a(1)), crate::neo::location(rt, a(2)));
            let to = rt.string(&to);
            rt.interned.insert(format!("alias/{registry}/{from}"), to);
            V::Void
        }
        (o, "get" | "value") if o.starts_with("net/neoforged/neoforge/registries/Deferred") => {
            let id = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned();
            if rt.registered.keys().all(|(_, n)| *n != id) {
                crate::neo::resolve(rt);
            }
            rt.registered.iter().find(|((_, n), _)| *n == id).map(|(_, v)| *v).unwrap_or(V::NULL)
        }

        // The level
        (_, "getBlockState") if level => {
            let p = pos_of(rt, a(1));
            let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
            state(rt, &text)
        }
        // `LevelReader.isEmptyBlock(pos)`: the block there is air (Farmer's Delight's rice grows its
        // panicles only into an empty block).
        (_, "isEmptyBlock") if level => {
            let p = pos_of(rt, a(1));
            let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
            let b = text.split('[').next().unwrap_or("");
            V::I(matches!(b, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") as i32)
        }
        // A mod block the write replaced hears of it (`onRemove`) before the call returns, as in
        // vanilla's `LevelChunk.setBlockState`.
        (_, "setBlock" | "setBlockAndUpdate") if level => {
            let p = pos_of(rt, a(1));
            let text = rt.str_of(a(2)).unwrap_or("minecraft:air").to_owned();
            let set = rt.host_call(|h| h.set_block_state(p, &text)).unwrap_or(false);
            crate::glue::drain_replaced(rt);
            V::I(set as i32)
        }
        // What a block removed leaves is its fluid's block (a waterlogged one leaves water), as
        // vanilla's `setBlock(pos, fluidState.createLegacyBlock(), ...)`.
        (_, "removeBlock") if level => {
            let p = pos_of(rt, a(1));
            let here = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
            let left = crate::block_breaking::fluid_block(rt, &here);
            let set = rt.host_call(|h| h.set_block_state(p, &left)).unwrap_or(false);
            crate::glue::drain_replaced(rt);
            V::I(set as i32)
        }
        // `destroyBlock(pos, drop[, entity[, recursionLeft]])`: the break's effect and, when asked,
        // the block's drops (`Block.dropResources`), then the fluid's block.
        (_, "destroyBlock") if level => {
            let entity = if desc.contains("Lnet/minecraft/world/entity/Entity;") { a(3) } else { V::NULL };
            V::I(crate::block_breaking::destroy_block(rt, a(0), a(1), a(2).i() != 0, entity) as i32)
        }
        // NeoForge's: no block snapshots are captured or restored here.
        (_, "$restoringBlockSnapshots" | "$captureBlockSnapshots") if level => V::I(0),
        // A level's side is its own (vanilla's `isClientSide` field: the client's level, a mod's
        // wrapper made on either side); one that doesn't carry it is on the host's.
        (_, "isClientSide" | "$isClientSide") if level => match rt.get_field(a(0), "isClientSide") {
            V::I(c) => V::I(c),
            _ => V::I(rt.host().is_some_and(|h| !h.is_server()) as i32),
        },
        (_, "isLoaded" | "hasChunkAt" | "hasChunk" | "isAreaLoaded" | "hasChunksAt" | "isInWorldBounds") if level => V::I(1),
        // `mayInteract(player, pos)`: vanilla's `Level` says yes; its `ServerLevel` asks the server's
        // spawn protection and world border, neither of which mods get here, so it says yes too.
        (_, "mayInteract") if level && desc == "(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/core/BlockPos;)Z" => V::I(1),
        (_, "getGameTime") if level => V::J(rt.host().map_or(0, |h| h.game_time())),
        // What a mod's own level takes from the one it copies (Create's and Ponder's virtual
        // worlds, `super(..., level.getProfilerSupplier(), ...)`): vanilla's inactive profiler,
        // which records nothing, as the client's (`Minecraft.getProfiler`).
        (_, "getProfilerSupplier") if level && desc == "()Ljava/util/function/Supplier;" => {
            let profiler = get_static(rt, "net/minecraft/util/profiling/InactiveProfiler.INSTANCE");
            crate::functions::constant(rt, profiler)
        }
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
        // A mod level's own light engine (Create's and Ponder's virtual worlds make one, and run
        // its updates after placing their blocks): light here is the game's, which every level's
        // `getBrightness` reads where the blocks are, so the engine keeps nothing and has no work.
        ("net/minecraft/world/level/lighting/LevelLightEngine", n) => match n {
            "<init>"
            | "checkBlock"
            | "updateSectionStatus"
            | "setLightEnabled"
            | "propagateLightSources"
            | "retainData"
            | "queueSectionData" => V::Void,
            "runLightUpdates" | "hasLightWork" => V::I(0),
            "getRawBrightness" => V::I(crate::level_env::raw_brightness(
                rt,
                pos_of(rt, a(1)),
                a(2).i(),
            )),
            "getLayerListener" => get_static(
                rt,
                "net/minecraft/world/level/lighting/LayerLightEventListener$DummyLightLayerEventListener.INSTANCE",
            ),
            _ => return None,
        },
        (_, "getChunkSource") if level => rt.intern("compat:chunk-source".into(), |rt| rt.new_object("net/minecraft/server/level/ServerChunkCache")),
        // The level's generator, a noise-based one as every dimension of a normal world has (Create's
        // belts ask whether it is the debug world's `DebugLevelSource`, whose blocks may not change).
        (CHUNK_CACHE | CHUNK_SOURCE, "getGenerator") => {
            let make = |rt: &mut Rt| rt.new_object(NOISE_GENERATOR);
            rt.intern("compat:chunk-generator".into(), make)
        }
        // Every chunk a mod reaches here is loaded (the level's `hasChunkAt` says the same).
        (CHUNK_CACHE | CHUNK_SOURCE, "hasChunk") => V::I(1),
        // The generator's heights are its dimension's noise settings (`NoiseGeneratorSettings`); a
        // dimension a datapack adds has its own, which aren't read here.
        (CHUNK_GENERATOR | NOISE_GENERATOR, "getSeaLevel" | "getMinY" | "getGenDepth") => {
            let dimension = rt.host().map(|h| h.dimension()).unwrap_or_default();
            let (sea, min_y, depth) = noise_heights(&dimension)?;
            V::I(match name {
                "getSeaLevel" => sea,
                "getMinY" => min_y,
                _ => depth,
            })
        }
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
        // Neighbour updates the mod asks for (a redstone link's receiver changing its signal, a
        // vault telling comparators its fill changed): the server's level runs them.
        (_, "updateNeighborsAt" | "blockUpdated" | "updateNeighborsAtExceptFromFacing" | "neighborChanged" | "updateNeighbourForOutputSignal")
            if level && desc.starts_with("(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;") =>
        {
            use crate::host::NeighborUpdate;
            let (pos, block) = (pos_of(rt, a(1)), block_name(rt, a(2)));
            let update = match name {
                "updateNeighborsAtExceptFromFacing" => NeighborUpdate::Around { pos, skip: Some(crate::api_common::direction_ordinal(rt, a(3))) },
                "neighborChanged" => NeighborUpdate::One { pos, from: pos_of(rt, a(3)) },
                "updateNeighbourForOutputSignal" => NeighborUpdate::Output { pos },
                _ => NeighborUpdate::Around { pos, skip: None },
            };
            // As the game's call: the neighbours' own code (a mod's nixie tube) runs inside it.
            rt.host_call(|h| h.update_neighbors(update, &block));
            V::Void
        }
        (_, "blockEntityChanged") if level => V::Void,
        // `Clearable.tryClear(object)`: a container (a mod's block entity, the runtime's own) empties
        // by its `clearContent`; anything else stays.
        ("net/minecraft/world/Clearable", "tryClear") => {
            if rt.instance_of(a(0), "net/minecraft/world/Clearable") {
                virtual_call(rt, ("net/minecraft/world/Container.clearContent.()V", "clearContent.()V"), &[a(0)]);
            }
            V::Void
        }
        (_, "sendParticles") if level => crate::particles::send_particles(rt, desc, args),
        (_, "addParticle" | "addAlwaysVisibleParticle") if level => crate::particles::level_particle(rt, name, desc, args),
        (_, "playSound" | "playSeededSound" | "playLocalSound") if level => crate::sounds::level_sound(rt, name, desc, args),
        (_, "gameEvent") if level => V::Void,
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
            "getGameTime" => V::J(rt.host().map_or(0, |h| h.game_time())),
            "getDayTime" => V::J(rt.host().map_or(0, |h| h.day_time())),
            "isRaining" => V::I(rt.host().is_some_and(|h| f64::from(h.weather().0) > 0.2) as i32),
            "isThundering" => V::I(rt.host().is_some_and(|h| f64::from(h.weather().1) > 0.9) as i32),
            "isHardcore" => V::I(0),
            _ => return None,
        },
        // The chunk holding a position (`getChunkAt`, `getChunk(pos)`), or at chunk coordinates
        // (`getChunk(x, z)`, also with a status and whether it must be there: Create's pipes ask
        // `getChunk(x, z, FULL, false)` before they look for fluid next door, and do nothing on null).
        // As `isLoaded` says, every chunk the level reaches is there; its level answers for it.
        (_, "getChunkAt") if level => {
            let p = pos_of(rt, a(1));
            level_chunk(rt, a(0), p[0] >> 4, p[2] >> 4)
        }
        (_, "getChunk") if level && desc.starts_with("(Lnet/minecraft/core/BlockPos;)") => {
            let p = pos_of(rt, a(1));
            level_chunk(rt, a(0), p[0] >> 4, p[2] >> 4)
        }
        (_, "getChunk") if level && desc.starts_with("(II") => level_chunk(rt, a(0), a(1).i(), a(2).i()),
        (LEVEL_CHUNK, "getLevel") => rt.get_field(a(0), "$level"),
        (LEVEL_CHUNK, "getBlockState") => {
            let level = rt.get_field(a(0), "$level");
            call(rt, "net/minecraft/world/level/Level.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;", &[level, a(1)])
        }
        // Where the chunk is, and what its level answers for it: its fluids, block entities and the
        // world's height. The game keeps its sections and saves it: nothing is unsaved here.
        (LEVEL_CHUNK | CHUNK_ACCESS, "getPos") => {
            let (x, z) = (rt.get_field(a(0), "$x").i(), rt.get_field(a(0), "$z").i());
            crate::api_common::chunk_pos(rt, x, z)
        }
        (LEVEL_CHUNK | CHUNK_ACCESS, "getFluidState")
            if desc.starts_with("(Lnet/minecraft/core/BlockPos;)") =>
        {
            let level = rt.get_field(a(0), "$level");
            call(
                rt,
                "net/minecraft/world/level/Level.getFluidState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;",
                &[level, a(1)],
            )
        }
        (LEVEL_CHUNK | CHUNK_ACCESS, "getBlockEntity")
            if desc.starts_with("(Lnet/minecraft/core/BlockPos;") =>
        {
            let level = rt.get_field(a(0), "$level");
            call(
                rt,
                "net/minecraft/world/level/Level.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
                &[level, a(1)],
            )
        }
        // The mods' block entities in it, by position (the game's own are not objects here).
        (LEVEL_CHUNK | CHUNK_ACCESS, "getBlockEntities" | "getBlockEntitiesPos") => {
            let (x, z) = (rt.get_field(a(0), "$x").i(), rt.get_field(a(0), "$z").i());
            let mut at: Vec<[i32; 3]> = rt
                .block_entities
                .keys()
                .copied()
                .filter(|p| p[0] >> 4 == x && p[2] >> 4 == z)
                .collect();
            at.sort_unstable();
            let mut found = Vec::new();
            for p in at {
                let be = crate::glue::block_entity_at(rt, p);
                if be.r() != 0 {
                    let k = block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]));
                    found.push((k, be));
                }
            }
            if name == "getBlockEntitiesPos" {
                let keys: Vec<V> = found.iter().map(|(k, _)| *k).collect();
                return Some(crate::jutil::new_set_of(rt, &keys));
            }
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
            for (k, be) in found {
                crate::jutil::table_put(rt, m, k, be);
            }
            m
        }
        (LEVEL_CHUNK | CHUNK_ACCESS, "setUnsaved" | "markUnsaved") => V::Void,
        (LEVEL_CHUNK | CHUNK_ACCESS, "isUnsaved" | "isEmpty") => V::I(0),
        // `LevelHeightAccessor`'s, from the world's lowest y and height.
        (LEVEL_CHUNK | CHUNK_ACCESS, n)
            if matches!(
                n,
                "getMinBuildHeight"
                    | "getMaxBuildHeight"
                    | "getHeight"
                    | "getMinSection"
                    | "getMaxSection"
                    | "getSectionsCount"
                    | "getSectionIndex"
                    | "getSectionIndexFromSectionY"
                    | "getSectionYFromSectionIndex"
                    | "isOutsideBuildHeight"
            ) =>
        {
            let (min, height) = rt.host().map_or((-64, 384), |h| h.world_height());
            let (min_section, max_section) = (min >> 4, ((min + height - 1) >> 4) + 1);
            let y = a(1).i();
            V::I(match n {
                "getMinBuildHeight" => min,
                "getMaxBuildHeight" => min + height,
                "getHeight" => height,
                "getMinSection" => min_section,
                "getMaxSection" => max_section,
                "getSectionsCount" => max_section - min_section,
                "getSectionIndex" => (y >> 4) - min_section,
                "getSectionIndexFromSectionY" => y - min_section,
                "getSectionYFromSectionIndex" => y + min_section,
                _ => i32::from(y < min || y >= min + height),
            })
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
            crate::glue::remove_block_entity(rt, p);
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
        (_, "getSeaLevel") if level => V::I(63),
        (_, "isOutsideBuildHeight") if level => {
            let y = if desc.starts_with("(I)") { a(1).i() } else { pos_of(rt, a(1))[1] };
            let (bottom, height) = rt.host().map_or((-64, 384), |h| h.world_height());
            V::I(!(bottom..bottom + height).contains(&y) as i32)
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
            "getAllLevels" => {
                let l = crate::glue::level(rt);
                crate::jutil::new_list(rt, vec![l])
            }
            "isDedicatedServer" | "isPublished" | "isHardcore" => V::I(0),
            "isSameThread" => V::I(1),
            "getPlayerList" => crate::events::player_list(rt),
            _ => return None,
        },

        // `Block.getSeed(state, pos)`, a mod block's `super.` call: `Mth.getSeed(pos)`.
        ("net/minecraft/world/level/block/Block", "getSeed") if BLOCK_SEED_KEY.ends_with(desc) => pos_seed(rt, a(2)),
        // `Block.canSupportRigidBlock(level, pos)`: the top of the block there holds a rigid one
        // (a rail, redstone dust). On any block class, as `javac` names the class a call was
        // written on.
        (o, "canSupportRigidBlock") if o.starts_with("net/minecraft/world/level/block/") && desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z" => {
            let (state, text) = level_state(rt, a(0), a(1));
            V::I(i32::from(face_sturdy(rt, &text, [state, a(0), a(1)], UP, SUPPORT_RIGID)))
        }
        // `Block.canSupportCenter(level, pos, direction)`: the middle of that face holds a post (a
        // torch, a lantern), but never the bottom of an `unstable_bottom_center` block.
        (o, "canSupportCenter")
            if o.starts_with("net/minecraft/world/level/block/") && desc == "(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Z" =>
        {
            let (state, text) = level_state(rt, a(0), a(1));
            let direction = usize::from(crate::api_common::direction_ordinal(rt, a(2)));
            let (b, _) = crate::host::split_state(&text);
            let unstable = direction == 0 && rt.host().is_some_and(|h| h.block_in_tag(b, "minecraft:unstable_bottom_center"));
            V::I(i32::from(!unstable && face_sturdy(rt, &text, [state, a(0), a(1)], direction, SUPPORT_CENTER)))
        }
        // `Block.shouldRenderFace(state, level, pos, face, neighbourPos)`: whether a face shows past
        // its neighbour (`faces`). On any block class, as `javac` names the class a call was written
        // on.
        (o, "shouldRenderFace") if o.starts_with("net/minecraft/world/level/block/") && desc == crate::faces::SHOULD_RENDER_FACE => {
            V::I(i32::from(crate::faces::should_render_face(rt, [a(0), a(1), a(2), a(3), a(4)])))
        }
        // `super.skipRendering(state, neighbour, face)` to a vanilla block class: that class's rule.
        (o, "skipRendering") if o.starts_with("net/minecraft/world/level/block/") && desc == crate::faces::SKIP_RENDERING => {
            V::I(i32::from(crate::faces::class_skips(rt, o, a(1), a(2), a(3))))
        }
        // `super.getCollisionShape(state, level, pos, context)` to a class with `BlockBehaviour`'s.
        (o, "getCollisionShape")
            if GET_COLLISION_KEY.ends_with(desc) && crate::blocks::declaring_class(o, &crate::blocks::COLLISION_SHAPED) == Some("BlockBehaviour") =>
        {
            behaviour_collision(rt, a(0), [a(1), a(2), a(3)])
        }
        // `super.getBlockSupportShape(state, level, pos)` to a vanilla block class: its own (a
        // leaf's), else `BlockBehaviour`'s, the block's collision shape.
        (o, "getBlockSupportShape") if GET_SUPPORT_SHAPE_KEY.ends_with(desc) && crate::blocks::declaring_class(o, &crate::blocks::SUPPORT_SHAPED).is_some() => {
            let text = rt.str_of(a(1)).unwrap_or("minecraft:air").to_owned();
            match crate::blocks::declaring_class(o, &crate::blocks::SUPPORT_SHAPED).unwrap_or_default() {
                "BlockBehaviour" => {
                    let class = rt.class_of(a(0));
                    let own = rt.is_mod_class(class).then_some((a(0), class));
                    collision_shape(rt, &text, own, [a(1), a(2), a(3)])
                }
                c => declared_support_shape(rt, c, &text, [a(1), a(2), a(3)]),
            }
        }

        // Block states
        (STATE | "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase", n) => {
            let text = rt.str_of(a(0)).unwrap_or("minecraft:air").to_owned();
            let (b, _) = crate::host::split_state(&text);
            match n {
                // `StateHolder.owner`, read as a field where an access transformer opens it: the
                // state's block.
                "getBlock" | "$owner" => block(rt, &b.to_owned()),
                // `handleNeighborChanged(level, pos, block, fromPos, movedByPiston)`: the block at
                // `pos` hears of the change at `fromPos`, as `Level.neighborChanged` has it do (Create's
                // sliding door tells its other half): the game runs it, a mod block's own code inside.
                "handleNeighborChanged" if desc.starts_with("(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;") => {
                    use crate::host::NeighborUpdate;
                    let (pos, from, changed) = (pos_of(rt, a(2)), pos_of(rt, a(4)), block_name(rt, a(3)));
                    rt.host_call(|h| h.update_neighbors(NeighborUpdate::One { pos, from }, &changed));
                    V::Void
                }
                // The block's registry entry; a mod block not registered yet learns its name when
                // it is.
                "getBlockHolder" => {
                    let blk = block(rt, &b.to_owned());
                    let id = if b.starts_with(UNREGISTERED) { "" } else { b };
                    crate::neo::registry_holder(rt, "block", &id.to_owned(), blk)
                }
                // `BlockStateBase`'s own view of itself as a `BlockState` (a mixin's `@Shadow`).
                "asState" => a(0),
                "isAir" => V::I(matches!(b, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") as i32),
                "hasBlockEntity" => V::I(crate::glue::block_entity_id(rt, b).is_some() as i32),
                "is" if desc.starts_with("(Lnet/minecraft/world/level/block/Block;") => V::I((block_name(rt, a(1)) == b) as i32),
                // A block's registry entry (`Holder<Block>`, a `DeferredBlock`): the one holding
                // this state's block.
                "is" if desc.starts_with("(Lnet/minecraft/core/Holder;") => {
                    let held = call(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[a(1)]);
                    V::I((held.r() != 0 && block_name(rt, held) == b) as i32)
                }
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
                // `getSeed(pos)`, what random model offsets and textures draw from: the block's own
                // `getSeed(state, pos)`, else `Mth.getSeed(pos)`.
                "getSeed" if desc == "(Lnet/minecraft/core/BlockPos;)J" => match crate::block_breaking::block_override(rt, &text, BLOCK_SEED_KEY, BLOCK_SEED, &[a(0), a(1)]) {
                    Some(v) => v,
                    None => pos_seed(rt, a(1)),
                },
                "isSlimeBlock" | "isStickyBlock" if desc == "()Z" => V::I(sticky(rt, n, a(0), V::NULL) as i32),
                "canStickTo" if desc == "(Lnet/minecraft/world/level/block/state/BlockState;)Z" => V::I(sticky(rt, n, a(0), a(1)) as i32),
                "getDestroySpeed" => V::F(crate::blocks::state_facts(rt, &text).hardness),
                // What the renderer asks of a state's faces (`faces`).
                "canOcclude" if desc == "()Z" => V::I(i32::from(crate::faces::can_occlude(rt, &text))),
                "isSolidRender" if desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z" => {
                    V::I(i32::from(crate::faces::solid_render(rt, a(0), a(1), a(2))))
                }
                "skipRendering" if desc == "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z" => {
                    V::I(i32::from(crate::faces::skip_rendering(rt, a(0), a(1), a(2))))
                }
                "getFaceOcclusionShape" if desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Lnet/minecraft/world/phys/shapes/VoxelShape;" => {
                    let direction = usize::from(crate::api_common::direction_ordinal(rt, a(3)));
                    let face = crate::faces::face_occlusion(rt, a(0), a(1), a(2), direction);
                    crate::shapes::shape(rt, &face)
                }
                "hidesNeighborFace" if desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;)Z" => {
                    V::I(i32::from(crate::faces::hides_neighbor_face(rt, a(0), a(1), a(2), a(3), a(4))))
                }
                "supportsExternalFaceHiding" if desc == "()Z" => V::I(i32::from(crate::faces::supports_external_face_hiding(rt, a(0)))),
                // `isFaceSturdy(level, pos, direction[, support])`: FULL unless it says.
                "isFaceSturdy" if desc.starts_with("(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;") => {
                    let direction = usize::from(crate::api_common::direction_ordinal(rt, a(3)));
                    let support = if desc.contains("SupportType;)") { rt.get_field(a(4), "$ordinal").i().clamp(0, 2) as usize } else { 0 };
                    V::I(i32::from(face_sturdy(rt, &text, [a(0), a(1), a(2)], direction, support)))
                }
                "getBlockSupportShape" if desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;" => {
                    support_shape(rt, &text, [a(0), a(1), a(2)])
                }
                "canBeReplaced" if desc == "()Z" => V::I(crate::blocks::state_facts(rt, &text).replaceable as i32),
                // `canBeReplaced(context)`: the mod block's own, else `BlockBehaviour`'s: a
                // replaceable block, unless the stack placed is its own item.
                "canBeReplaced" if desc == "(Lnet/minecraft/world/item/context/BlockPlaceContext;)Z" => {
                    if let Some((blk, class)) = crate::glue::mod_block(rt, &text)
                        && rt.overrides.is_some_and(|o| o(class, CAN_BE_REPLACED))
                        && let Some(vc) = rt.vcall
                    {
                        let r = vc(rt, CAN_BE_REPLACED_KEY, CAN_BE_REPLACED, &[blk, a(0), a(1)]);
                        if rt.thrown.is_none() {
                            return Some(r);
                        }
                        rt.report_thrown("canBeReplaced");
                    }
                    let held = rt.get_field(a(1), "$item");
                    let own = !crate::api_common::stack_empty(rt, held) && crate::api_common::item_like_name(rt, rt.get_field(held, "$item")) == crate::host::split_state(&text).0;
                    V::I(i32::from(crate::blocks::state_facts(rt, &text).replaceable && !own))
                }
                // A block's use and its punch, as a mod asks them for a player of its own (Create's
                // deployer): `useItemOn(stack, level, player, hand, hit)`, `useWithoutItem(level,
                // player, hit)` and `attack(level, pos, player)` reach the mod block's own, at the
                // hit's position. A vanilla block's aren't run for a mod's player yet: they pass.
                "useItemOn" | "useWithoutItem" | "attack" if matches!(desc, STATE_USE_ITEM_ON | STATE_USE_WITHOUT_ITEM | STATE_ATTACK) => {
                    let (nd, key) = match n {
                        "useItemOn" => (USE_ITEM_ON, USE_ITEM_ON_KEY),
                        "useWithoutItem" => (USE_WITHOUT_ITEM, USE_WITHOUT_ITEM_KEY),
                        _ => (ATTACK, ATTACK_KEY),
                    };
                    if let Some((blk, class)) = crate::glue::mod_block(rt, &text)
                        && rt.overrides.is_some_and(|o| o(class, nd))
                        && let Some(vc) = rt.vcall
                    {
                        let all = match n {
                            "useItemOn" => {
                                let pos = rt.get_field(a(5), "$pos");
                                vec![blk, a(1), a(0), a(2), pos, a(3), a(4), a(5)]
                            }
                            "useWithoutItem" => {
                                let pos = rt.get_field(a(3), "$pos");
                                vec![blk, a(0), a(1), pos, a(2), a(3)]
                            }
                            _ => vec![blk, a(0), a(1), a(2), a(3)],
                        };
                        let r = vc(rt, key, nd, &all);
                        if rt.thrown.is_none() {
                            return Some(r);
                        }
                        rt.report_thrown(n);
                    }
                    match n {
                        "useItemOn" => crate::api_common::result(rt, "net/minecraft/world/ItemInteractionResult", "PASS_TO_DEFAULT_BLOCK_INTERACTION"),
                        "useWithoutItem" => crate::api_common::result(rt, "net/minecraft/world/InteractionResult", "PASS"),
                        _ => V::Void,
                    }
                }
                "updateShape" if desc.starts_with("(Lnet/minecraft/core/Direction;") => update_shape(rt, [a(0), a(1), a(2), a(3), a(4), a(5)]),
                "getCollisionShape" | "getShape" if desc.starts_with("(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;") => {
                    let ctx = if desc.contains("CollisionContext;)") { a(3) } else { crate::shapes::empty_context(rt) };
                    state_shape(rt, &text, n == "getCollisionShape", [a(0), a(1), a(2), ctx])
                }
                // `isCollisionShapeFullBlock(level, pos)`: whether that collision shape is the whole
                // block (`Block.isShapeFullBlock`), as the state's cache has it.
                "isCollisionShapeFullBlock" if desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z" => {
                    let ctx = crate::shapes::empty_context(rt);
                    let shape = state_shape(rt, &text, true, [a(0), a(1), a(2), ctx]);
                    V::I(i32::from(crate::shapes::boxes(rt, shape).is_some_and(|b| crate::shapes::full_block(&b))))
                }
                // `getOcclusionShape(level, pos)`: the mod block's own, else its shape, as
                // `BlockBehaviour`'s default gives it.
                "getOcclusionShape" if desc == "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;" => {
                    if let Some((blk, class)) = crate::glue::mod_block(rt, &text)
                        && rt.overrides.is_some_and(|o| o(class, GET_OCCLUSION))
                        && let Some(vc) = rt.vcall
                    {
                        let s = vc(rt, GET_OCCLUSION_KEY, GET_OCCLUSION, &[blk, a(0), a(1), a(2)]);
                        if rt.thrown.is_none() && crate::shapes::boxes(rt, s).is_some() {
                            return Some(s);
                        }
                        rt.report_thrown("getOcclusionShape");
                    }
                    let ctx = crate::shapes::empty_context(rt);
                    state_shape(rt, &text, false, [a(0), a(1), a(2), ctx])
                }
                // The mod block's own `canSurvive`, else its vanilla plant base's (with its own
                // `mayPlaceOn`); vanilla's default answers true.
                "canSurvive" => match crate::glue::mod_block(rt, &text) {
                    Some((blk, class)) => V::I(crate::blocks::survives_by_code(rt, blk, class, a(0), a(1), a(2)).unwrap_or(true) as i32),
                    None => V::I(1),
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
        // A block's states, as far as the runtime knows them: its default one. Its field
        // (`this.stateDefinition.any()` in a mod block's constructor) is the same object.
        (_, "getStateDefinition" | "$stateDefinition")
            if desc == "()Lnet/minecraft/world/level/block/state/StateDefinition;" && (name == "getStateDefinition" || is_block_class(owner)) =>
        {
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
                    let states = possible_states(rt, b);
                    crate::jutil::new_list(rt, states)
                }
                "getProperties" => {
                    let props = definition_properties(rt, b);
                    crate::jutil::new_list(rt, props)
                }
                // One of them by name, null for none (a mod finds another mod's `facing` so).
                "getProperty" => {
                    let wanted = rt.str_of(a(1)).unwrap_or("").to_owned();
                    definition_properties(rt, b)
                        .into_iter()
                        .find(|p| crate::api_common::prop_parts(rt, *p).0 == wanted)
                        .unwrap_or(V::NULL)
                }
                _ => return crate::api_common::call(rt, owner, name, desc, args),
            }
        }
        // `getShapeForEachState(shapeGetter)`: every state's shape, worked out once (the cache a
        // block's `getShape` reads back).
        (_, "getShapeForEachState") if is_block_class(owner) && desc == "(Ljava/util/function/Function;)Lcom/google/common/collect/ImmutableMap;" => {
            let m = rt.alloc("com/google/common/collect/ImmutableMap", Data::Table(Default::default()));
            for s in possible_states(rt, a(0)) {
                let shape = rt.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(1), &[s]);
                if rt.thrown.is_some() {
                    return Some(V::NULL);
                }
                crate::jutil::table_put(rt, m, s, shape);
            }
            m
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
        // `Block.byItem(item)`: the block a block item places, air for any other item.
        (BLOCK, "byItem") => block_of_item(rt, a(0)),
        // `super.setPlacedBy(...)` from a mod's block: `Block`'s does nothing, and what the classes
        // that override it do (a bed's head, a door's top half) the game's template did as the block
        // was placed. (A diode's is `diodes`'.)
        (o, "setPlacedBy")
            if o.starts_with("net/minecraft/world/level/block/")
                && crate::api_common::vanilla_is_a(o, BLOCK)
                && SET_PLACED_BY_DESC == desc =>
        {
            V::Void
        }
        // `super.updateShape(...)` reaching `BlockBehaviour`'s: the state as it is.
        (o, "updateShape")
            if UPDATE_SHAPE.ends_with(desc)
                && crate::blocks::update_shape_declarer(o) == Some("state/BlockBehaviour") =>
        {
            a(1)
        }
        // `super.updateShape(...)` reaching a vanilla class's own (a fence joining its neighbours, a
        // slab's water, a plant's survival): the game's rule, run from the shape update's hook.
        (o, "updateShape")
            if UPDATE_SHAPE.ends_with(desc)
                && crate::blocks::update_shape_declarer(o).is_some() =>
        {
            class_update_shape(rt, o, args)
        }
        (BLOCK, "<init>") => V::Void,
        // The API's base constructors a mod's own classes call (`super(properties)`): their state
        // lives in the runtime's fields, so nothing more to do.
        (o, "<init>") if o.starts_with("net/minecraft/world/level/block/") && (o.ends_with("Block") || o.ends_with("BlockBehaviour")) => V::Void,
        (o, "<init>") if o.starts_with("net/minecraft/world/item/") && o.ends_with("Item") => V::Void,
        _ => return crate::api_common::call(rt, owner, name, desc, args),
    })
}

/// A vanilla class's `updateShape` for a mod block's state: `super.updateShape(state, direction,
/// neighbour, level, pos, neighbourPos)` from a class extending `class`. The host runs the rule of
/// the nearest of `class` and the classes above it, up to the declaring one, that the state's block
/// has among its classes (a mod block's are its template's); the state stays as it is where the
/// host runs none (outside a shape update, or a template without the class).
fn class_update_shape(rt: &mut Rt, class: &str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::Void);
    let current = a(1);
    let classes = crate::blocks::update_shape_ancestry(class);
    let (Some(text), Some(neighbour)) = (
        rt.str_of(current).map(str::to_owned),
        rt.str_of(a(3)).map(str::to_owned),
    ) else {
        return current;
    };
    if classes.is_empty() {
        return current;
    }
    let direction = usize::from(crate::api_common::direction_ordinal(rt, a(2)));
    let (pos, neighbour_pos) = (pos_of(rt, a(5)), pos_of(rt, a(6)));
    let new = rt.host_mut().and_then(|h| {
        h.vanilla_update_shape(&classes, pos, &text, direction, neighbour_pos, &neighbour)
    });
    match new {
        Some(new) if new != text => state(rt, &new),
        _ => current,
    }
}

/// The most states [`possible_states`] spells out; a definition with more (vanilla's largest,
/// redstone wire, has 1296) keeps its default state alone.
const MAX_STATES: usize = 1 << 16;

/// Block `b`'s states in `StateDefinition.getPossibleStates()`'s order: every combination of its
/// properties' values, the property first by name varying slowest, each property's values in its
/// own order. A vanilla block, whose definition the game keeps, answers its default state.
pub(crate) fn possible_states(rt: &mut Rt, b: V) -> Vec<V> {
    let name = block_name(rt, b);
    let list = match rt.heap.get(b.r() as usize).map(|o| &o.data) {
        Some(Data::Fields(_)) => rt.get_field(b, "$state-props"),
        _ => V::NULL,
    };
    let mut props: Vec<(String, Vec<String>)> = Vec::new();
    for p in crate::jutil::elements(rt, list) {
        let (pname, _) = crate::api_common::prop_parts(rt, p);
        let values = crate::blocks::property_values(rt, p);
        if !values.is_empty() && !props.iter().any(|(n, _)| *n == pname) {
            props.push((pname, values));
        }
    }
    props.sort_by(|x, y| x.0.cmp(&y.0));
    let count = props.iter().try_fold(1usize, |n, (_, v)| n.checked_mul(v.len()).filter(|&n| n <= MAX_STATES));
    if props.is_empty() || count.is_none() {
        let default = match rt.str_of(rt.get_field(b, "$default")).map(str::to_owned) {
            Some(d) if !d.is_empty() && !props.is_empty() => format!("{name}[{d}]"),
            _ => name.clone(),
        };
        return vec![state(rt, &default)];
    }
    let mut combos: Vec<Vec<(&str, &str)>> = vec![Vec::new()];
    for (pname, values) in &props {
        combos = combos
            .into_iter()
            .flat_map(|c| values.iter().map(move |v| c.iter().copied().chain([(pname.as_str(), v.as_str())]).collect()))
            .collect();
    }
    let texts: Vec<String> = combos.into_iter().map(|c| crate::host::join_state(&name, c)).collect();
    texts.iter().map(|t| state(rt, t)).collect()
}

/// 1.21.1's items that place a block of another name (`ItemNameBlockItem`s, a solid bucket).
const ITEM_BLOCKS: [(&str, &str); 14] = [
    ("minecraft:beetroot_seeds", "minecraft:beetroots"),
    ("minecraft:carrot", "minecraft:carrots"),
    ("minecraft:cocoa_beans", "minecraft:cocoa"),
    ("minecraft:glow_berries", "minecraft:cave_vines"),
    ("minecraft:melon_seeds", "minecraft:melon_stem"),
    ("minecraft:pitcher_pod", "minecraft:pitcher_crop"),
    ("minecraft:potato", "minecraft:potatoes"),
    ("minecraft:powder_snow_bucket", "minecraft:powder_snow"),
    ("minecraft:pumpkin_seeds", "minecraft:pumpkin_stem"),
    ("minecraft:redstone", "minecraft:redstone_wire"),
    ("minecraft:string", "minecraft:tripwire"),
    ("minecraft:sweet_berries", "minecraft:sweet_berry_bush"),
    ("minecraft:torchflower_seeds", "minecraft:torchflower_crop"),
    ("minecraft:wheat_seeds", "minecraft:wheat"),
];

/// `Block.byItem(item)`: the block a block item places (a mod's `BlockItem` its own), air for any
/// other item.
fn block_of_item(rt: &mut Rt, item: V) -> V {
    let item = crate::creative::as_item(rt, item);
    let own = rt.get_field(item, "$block");
    if own.r() != 0 {
        return own;
    }
    let name = crate::api_common::item_like_name(rt, item);
    let renamed = ITEM_BLOCKS
        .iter()
        .find(|(i, _)| *i == name)
        .map(|(_, b)| *b);
    let places = renamed.is_some()
        || rt.class_of(item) == "net/minecraft/world/item/BlockItem"
        || rt.host().is_some_and(|h| h.item_is_block(&name));
    match renamed {
        Some(b) => block(rt, b),
        None if places && name != "minecraft:air" => block(rt, &name),
        None => block(rt, "minecraft:air"),
    }
}

/// The properties of block `b`'s states (`StateDefinition.getProperties()`), once each: a mod
/// block's own, as its definition added them; a vanilla block's, its default state's in the game.
fn definition_properties(rt: &mut Rt, b: V) -> Vec<V> {
    let list = match rt.heap.get(b.r() as usize).map(|o| &o.data) {
        Some(Data::Fields(_)) => rt.get_field(b, "$state-props"),
        _ => V::NULL,
    };
    if list.r() != 0 {
        let mut out: Vec<V> = Vec::new();
        for p in crate::jutil::elements(rt, list) {
            let n = crate::api_common::prop_parts(rt, p).0;
            if !out
                .iter()
                .any(|q| crate::api_common::prop_parts(rt, *q).0 == n)
            {
                out.push(p);
            }
        }
        return out;
    }
    let name = block_name(rt, b);
    let text = rt
        .host()
        .and_then(|h| h.state_id(&name).and_then(|id| h.state_of_id(id)))
        .unwrap_or(name);
    let st = state(rt, &text);
    crate::api_common::state_properties(rt, st)
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

/// The holder `register` (a DeferredRegister) makes for `id` in `registry`, as NeoForge's
/// `register` does: through the register's `createHolder`, which a mod's own register class may
/// override (Mekanism's makes its `TileEntityTypeRegistryObject`s), else the runtime's. Kept in
/// the register's entries (`getEntries`).
fn register_holder(rt: &mut Rt, register: V, registry: &str, id: &str) -> V {
    const CREATE_HOLDER: &str = "createHolder.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/neoforged/neoforge/registries/DeferredHolder;";
    const CREATE_HOLDER_KEY: &str =
        "net/neoforged/neoforge/registries/DeferredRegister.createHolder.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/neoforged/neoforge/registries/DeferredHolder;";
    let class = rt.class_of(register);
    let mut h = V::NULL;
    if rt.overrides.is_some_and(|o| o(class, CREATE_HOLDER))
        && let Some(vc) = rt.vcall
    {
        let (r, l) = (rt.string(registry), rt.string(id));
        h = vc(rt, CREATE_HOLDER_KEY, CREATE_HOLDER, &[register, r, l]);
        if rt.thrown.is_some() {
            return V::NULL;
        }
    }
    if h.r() == 0 {
        h = holder(rt, registry, id);
    }
    let mut entries = rt.get_field(register, "$entries");
    if entries.r() == 0 {
        entries = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(register, "$entries", entries);
    }
    crate::jutil::list_add(rt, entries, h);
    h
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
fn mutable_pos(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
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
    // A game class's static a mod wrote (an access transformer opens it: FarmersDelight's
    // `Villager.FOOD_POINTS = ImmutableMap.copyOf(...)`): what it wrote, as the field holds it.
    if let Some(&v) = rt.statics.get(key)
        && v != V::Void
    {
        return v;
    }
    if let Some((o, f)) = key.split_once('.')
        && let Some(v) = crate::network::get_static(rt, o, f)
            .or_else(|| crate::worldgen::get_static(rt, o, f))
            .or_else(|| crate::api_common::get_static(rt, o, f))
            .or_else(|| crate::render::get_static(rt, o, f))
            .or_else(|| crate::models::get_static(rt, o, f))
            .or_else(|| crate::entity_render::get_static(rt, o, f))
            .or_else(|| crate::camera::get_static(rt, o, f))
            .or_else(|| crate::client_extensions::get_static(rt, o, f))
            .or_else(|| crate::colors::get_static(rt, o, f))
            .or_else(|| crate::quads::get_static(rt, o, f))
            .or_else(|| crate::blocks::get_static(rt, o, f))
            .or_else(|| crate::shapes::get_static(rt, o, f))
            .or_else(|| crate::fluids::get_static(rt, o, f))
            .or_else(|| crate::neo_tags::get_static(rt, o, f))
            .or_else(|| crate::mod_entities::get_static(rt, o, f))
            .or_else(|| crate::projectiles::get_static(rt, o, f))
            .or_else(|| crate::tanks::get_static(rt, o, f))
            .or_else(|| crate::energy::get_static(rt, o, f))
            .or_else(|| crate::creative::get_static(rt, o, f))
            .or_else(|| crate::tooltips::get_static(rt, o, f))
            .or_else(|| crate::gamerules::get_static(rt, o, f))
            .or_else(|| crate::value_providers::get_static(rt, o, f))
            .or_else(|| crate::nightconfig::get_static(rt, o, f))
            .or_else(|| crate::codecs::get_static(rt, o, f))
            .or_else(|| crate::recipes::get_static(rt, o, f))
            .or_else(|| crate::components::get_static(rt, o, f))
            .or_else(|| crate::capabilities::get_static(rt, o, f))
            .or_else(|| crate::sounds::get_static(rt, o, f))
            .or_else(|| crate::particles::get_static(rt, o, f))
            .or_else(|| crate::food::get_static(rt, o, f))
            .or_else(|| crate::attributes::get_static(rt, o, f))
            .or_else(|| crate::enchantments::get_static(rt, o, f))
            .or_else(|| crate::events::get_static(rt, o, f))
            .or_else(|| crate::damage::get_static(rt, o, f))
            .or_else(|| crate::gui::get_static(rt, o, f))
            .or_else(|| crate::screens::get_static(rt, o, f))
            .or_else(|| crate::menus::get_static(rt, o, f))
            .or_else(|| crate::handlers::get_static(rt, o, f))
            .or_else(|| crate::neo::get_static(rt, o, f))
            .or_else(|| crate::files::get_static(rt, o, f))
            .or_else(|| crate::resources::get_static(rt, o, f))
            .or_else(|| crate::brigadier::get_static(rt, o, f))
            .or_else(|| (o == crate::gson::NULL && f == "INSTANCE").then(|| crate::gson::null(rt)))
    {
        return v;
    }
    if let Some(field) = key.strip_prefix(CHUNK_STATUS).and_then(|k| k.strip_prefix('.'))
        && let Some(index) = CHUNK_STATUSES.iter().position(|s| s.eq_ignore_ascii_case(field))
    {
        return chunk_status(rt, index);
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
        // A player's game is FML's production (as `FMLLoader.isProduction()` answers): no dev tools.
        "net/neoforged/fml/loading/FMLEnvironment.production" => V::I(1),
        "net/neoforged/api/distmarker/Dist.DEDICATED_SERVER" => api_enum(rt, "net/neoforged/api/distmarker/Dist", "DEDICATED_SERVER", 1),
        // Which way a packet goes (vanilla's order).
        "net/minecraft/network/protocol/PacketFlow.SERVERBOUND" => api_enum(rt, PACKET_FLOW, "SERVERBOUND", 0),
        "net/minecraft/network/protocol/PacketFlow.CLIENTBOUND" => api_enum(rt, PACKET_FLOW, "CLIENTBOUND", 1),
        // Which file a config goes to (FML's order).
        k if k.starts_with("net/neoforged/fml/config/ModConfig$Type.") => {
            const TYPES: [&str; 4] = ["COMMON", "CLIENT", "SERVER", "STARTUP"];
            let f = k.rsplit('.').next().unwrap_or("");
            match TYPES.iter().position(|t| *t == f) {
                Some(i) => api_enum(rt, "net/neoforged/fml/config/ModConfig$Type", TYPES[i], i as i32),
                None => {
                    crate::miss(rt, key);
                    V::NULL
                }
            }
        }
        // NeoForge's listener priorities, highest first.
        k if k.starts_with("net/neoforged/bus/api/EventPriority.") => {
            const NAMES: [&str; 5] = ["HIGHEST", "HIGH", "NORMAL", "LOW", "LOWEST"];
            let n = k.rsplit('.').next().unwrap_or("");
            match NAMES.iter().position(|&x| x == n) {
                Some(i) => api_enum(rt, "net/neoforged/bus/api/EventPriority", NAMES[i], i as i32),
                None => {
                    crate::miss(rt, key);
                    V::NULL
                }
            }
        }
        k if k.starts_with("java/util/Locale.") => rt.intern("compat:locale".into(), |rt| rt.new_object("java/util/Locale")),
        // The data fixer's type references (`References.BLOCK_ENTITY`), by their type names.
        k if k.starts_with("net/minecraft/util/datafix/fixes/References.") => {
            let n = k.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            rt.string(&n)
        }
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
                    crate::miss(rt, key);
                    V::NULL
                }
            }
        }
        // `LightLayer`: the sky's light, then the blocks'.
        "net/minecraft/world/level/LightLayer.SKY" => api_enum(rt, crate::level_env::LIGHT_LAYER, "SKY", 0),
        "net/minecraft/world/level/LightLayer.BLOCK" => api_enum(rt, crate::level_env::LIGHT_LAYER, "BLOCK", 1),
        // `ClipContext`'s modes, what a ray stops at (`getPlayerPOVHitResult`, `Level.clip`).
        k if k.starts_with("net/minecraft/world/level/ClipContext$Fluid.") || k.starts_with("net/minecraft/world/level/ClipContext$Block.") => {
            let fluid = k.contains("$Fluid.");
            let names: &[&'static str] = if fluid { &["NONE", "SOURCE_ONLY", "ANY", "WATER"] } else { &["COLLIDER", "OUTLINE", "VISUAL", "FALLDAMAGE_RESETTING"] };
            let class = if fluid { "net/minecraft/world/level/ClipContext$Fluid" } else { "net/minecraft/world/level/ClipContext$Block" };
            let f = k.rsplit('.').next().unwrap_or("");
            match names.iter().position(|n| *n == f) {
                Some(i) => api_enum(rt, class, names[i], i as i32),
                None => {
                    crate::miss(rt, key);
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
                    crate::miss(rt, key);
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
                    crate::miss(rt, key);
                    V::NULL
                }
            }
        }
        k if k.starts_with("net/minecraft/world/level/block/Blocks.") => {
            let n = field_id(k.rsplit('.').next().unwrap_or(""));
            block(rt, &n)
        }
        // A stream codec of the game's this runtime doesn't model (`DyeColor.STREAM_CODEC`): the
        // class's codec over the wire, as `ByteBufCodecs.fromCodecWithRegistries` sends it; both
        // ends of Horizon read it alike. `OPTIONAL_STREAM_CODEC` takes `OPTIONAL_CODEC`, else
        // `CODEC`, else `FLAT_CODEC` (NeoForge's sized ingredients have no `CODEC`). A class whose
        // codec isn't modelled either is a miss of its stream codec.
        k if (k.starts_with("net/minecraft/") || k.starts_with("net/neoforged/")) && k.ends_with("STREAM_CODEC") => {
            let id = format!("compat:stream-of/{k}");
            if let Some(&c) = rt.interned.get(&id) {
                return c;
            }
            let (class, field) = k.rsplit_once('.').unwrap_or((k, ""));
            let prefix = field.strip_suffix("STREAM_CODEC").unwrap_or("");
            for name in [format!("{prefix}CODEC"), "CODEC".to_owned(), "FLAT_CODEC".to_owned()] {
                let ck = rt.static_key(&format!("{class}.{name}"));
                let since = crate::misses();
                let codec = get_static(rt, ck);
                if crate::missed_since(ck, since) {
                    uncount(rt, ck);
                } else if codec.r() != 0 {
                    let made = crate::network::from_codec(rt, codec);
                    rt.interned.insert(id, made);
                    return made;
                }
            }
            crate::miss(rt, key);
            let unknown = crate::codecs::native(rt, &format!("unmodelled/{k}"));
            crate::network::from_codec(rt, unknown)
        }
        // A codec of the game's this runtime doesn't model yet (`FoodProperties.DIRECT_CODEC`,
        // `Biome.LIST_CODEC`, `X.CODEC_NONEMPTY`): still a miss, but a codec, so what a mod builds on
        // it stays whole; reading with it fails.
        k if (k.starts_with("net/minecraft/") || k.starts_with("net/neoforged/"))
            && k.rsplit('.').next().is_some_and(|f| f.contains("CODEC") && !f.contains("STREAM_CODEC")) =>
        {
            crate::miss(rt, key);
            crate::codecs::native(rt, &format!("unmodelled/{k}"))
        }
        // A constant of a vanilla enum no module models (`NoteBlockInstrument.BASS`), from its class.
        // Else the field of the game's superclass that declares it, as the JVM resolves a static
        // inherited by the class it is read through: the translator names the first of the game's
        // classes a mod's class extends, which may only inherit it.
        _ => match crate::records::enum_constant(rt, key) {
            Some(v) => v,
            None => match key.rsplit_once('.').and_then(|(o, f)| Some((api_super(o)?, f))) {
                Some((declaring, field)) => {
                    let inherited = rt.static_key(&format!("{declaring}.{field}"));
                    get_static(rt, inherited)
                }
                None => {
                    crate::miss(rt, key);
                    V::NULL
                }
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `getClass()` called through a type other than `Object` (Create's spout checks
    /// `handler.getClass() == FluidBucketWrapper.class` on an `IFluidHandlerItem`): `Object`'s own,
    /// the object's class.
    #[test]
    fn get_class_through_any_type_is_the_objects_class() {
        let mut rt = Rt::new();
        crate::install();
        let wrapper = "net/neoforged/neoforge/fluids/capability/wrappers/FluidBucketWrapper";
        let w = rt.new_object(wrapper);
        let key = "net/neoforged/neoforge/fluids/capability/IFluidHandlerItem.getClass.()Ljava/lang/Class;";
        let class = call(&mut rt, key, &[w]);
        assert_eq!(class, rt.class_literal(wrapper));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `equals` and `hashCode` through a class that doesn't declare them (a mod's entity, called
    /// as Create's `ControlledContraptionEntity.equals`): `Object`'s, by identity. Create's
    /// contraption looks for the others around it by `e -> !e.equals(self)`, and must not find
    /// itself.
    #[test]
    fn equals_through_a_class_without_its_own_is_identity() {
        const EQUALS: &str = "test/Contraption.equals.(Ljava/lang/Object;)Z";
        const HASH: &str = "test/Contraption.hashCode.()I";
        let mut rt = Rt::new();
        crate::install();
        rt.supers
            .insert("test/Contraption", "net/minecraft/world/entity/Entity");
        let (a, b) = (
            rt.new_object("test/Contraption"),
            rt.new_object("test/Contraption"),
        );
        assert_eq!(call(&mut rt, EQUALS, &[a, a]).i(), 1);
        assert_eq!(call(&mut rt, EQUALS, &[a, b]).i(), 0);
        assert_eq!(call(&mut rt, EQUALS, &[a, V::NULL]).i(), 0);
        assert_eq!(call(&mut rt, HASH, &[a]), call(&mut rt, HASH, &[a]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own level (Create's virtual world for a contraption's blocks) takes its profiler
    /// from the level it copies (vanilla's inactive one) and makes a light engine whose updates
    /// it runs; the light is the game's, read where the blocks are.
    #[test]
    fn a_mods_virtual_level_gets_a_profiler_and_a_light_engine() {
        const PROFILERS: &str =
            "net/minecraft/world/level/Level.getProfilerSupplier.()Ljava/util/function/Supplier;";
        const ENGINE: &str = "net/minecraft/world/level/lighting/LevelLightEngine";
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let supplier = call(rt, PROFILERS, &[level]);
            // The supplier as the mod's code calls it (`supplier.get()`, through the API).
            let profiler = call(rt, "java/util/function/Supplier.get.()Ljava/lang/Object;", &[supplier]);
            assert_eq!(profiler, get_static(rt, "net/minecraft/util/profiling/InactiveProfiler.INSTANCE"));
            let engine = rt.new_object(ENGINE);
            call(rt, "net/minecraft/world/level/lighting/LevelLightEngine.<init>.(Lnet/minecraft/world/level/chunk/LightChunkGetter;ZZ)V", &[engine, level, V::I(1), V::I(0)]);
            assert_eq!(call(rt, "net/minecraft/world/level/lighting/LevelLightEngine.runLightUpdates.()I", &[engine]).i(), 0);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `StateHolder.owner` read as a field (an access transformer opens it) on a block state: the
    /// state's block, as `getBlock()` gives it.
    #[test]
    fn a_states_owner_is_its_block() {
        let mut rt = Rt::new();
        crate::install();
        let state = crate::api::state(&mut rt, "minecraft:oak_planks");
        let owner = call(
            &mut rt,
            "net/minecraft/world/level/block/state/StateHolder.$owner.()Ljava/lang/Object;",
            &[state],
        );
        let block = call(
            &mut rt,
            "net/minecraft/world/level/block/state/BlockState.getBlock.()Lnet/minecraft/world/level/block/Block;",
            &[state],
        );
        assert_eq!(owner, block);
        assert!(owner.r() != 0);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `state.handleNeighborChanged(level, pos, block, fromPos, moved)` is the level's
    /// `neighborChanged(pos, block, fromPos)`: the game tells the block at `pos`.
    #[test]
    fn a_states_neighbour_change_reaches_the_level() {
        use crate::host::{MemoryHost, NeighborUpdate};
        const HANDLE: &str = "net/minecraft/world/level/block/state/BlockState.handleNeighborChanged.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;Lnet/minecraft/core/BlockPos;Z)V";
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let l = crate::glue::level(rt);
            let door = state(rt, "minecraft:oak_door[half=upper]");
            let (at, below) = (block_pos(rt, 4, 65, 0), block_pos(rt, 4, 64, 0));
            let changed = block(rt, "minecraft:oak_door");
            call(rt, HANDLE, &[door, l, at, changed, below, V::I(0)]);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        assert_eq!(
            host.neighbor_updates,
            [(
                NeighborUpdate::One {
                    pos: [4, 65, 0],
                    from: [4, 64, 0]
                },
                "minecraft:oak_door".to_owned()
            )]
        );
    }

    /// `level.getChunkSource().getGenerator()` is a noise-based generator, not the debug world's,
    /// with the overworld's heights from its noise settings.
    #[test]
    fn a_levels_generator_is_its_dimensions_noise_generator() {
        const SOURCE: &str = "net/minecraft/world/level/Level.getChunkSource.()Lnet/minecraft/world/level/chunk/ChunkSource;";
        const GENERATOR: &str = "net/minecraft/server/level/ServerChunkCache.getGenerator.()Lnet/minecraft/world/level/chunk/ChunkGenerator;";
        const HEIGHTS: [&str; 3] = [
            "net/minecraft/world/level/chunk/ChunkGenerator.getSeaLevel.()I",
            "net/minecraft/world/level/chunk/ChunkGenerator.getMinY.()I",
            "net/minecraft/world/level/chunk/ChunkGenerator.getGenDepth.()I",
        ];
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        let (generator, heights) = rt.with_host(&mut host, |rt| {
            let l = crate::glue::level(rt);
            let source = call(rt, SOURCE, &[l]);
            let generator = call(rt, GENERATOR, &[source]);
            let heights = HEIGHTS.map(|key| call(rt, key, &[generator]));
            (generator, heights)
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        assert!(rt.instance_of(generator, "net/minecraft/world/level/chunk/ChunkGenerator"));
        assert!(!rt.instance_of(
            generator,
            "net/minecraft/world/level/levelgen/DebugLevelSource"
        ));
        assert_eq!(heights, [V::I(63), V::I(-64), V::I(384)]);
    }

    /// `BlockState.getSeed(pos)`: a mod block's own `getSeed(state, pos)` first; else vanilla's
    /// `Mth.getSeed` of the position (its value at 10, 64, -20 as vanilla computes it).
    #[test]
    fn a_states_seed_is_its_blocks_own_else_its_positions() {
        fn fixed_seed(class: &str, nd: &str) -> bool {
            class == "test/FixedSeedBlock" && nd == BLOCK_SEED
        }
        fn fixed_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            if short == BLOCK_SEED { V::J(42) } else { call(rt, key, args) }
        }
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(fixed_seed);
        rt.vcall = Some(fixed_code);
        let block = rt.new_object("test/FixedSeedBlock");
        rt.registered.insert(("block".into(), "test:fixed".into()), block);
        let get_seed = "net/minecraft/world/level/block/state/BlockState.getSeed.(Lnet/minecraft/core/BlockPos;)J";
        let pos = block_pos(&mut rt, 10, 64, -20);
        let stone = state(&mut rt, "minecraft:stone");
        assert_eq!(call(&mut rt, get_seed, &[stone, pos]), V::J(-62_212_282_568_415));
        let fixed = state(&mut rt, "test:fixed");
        assert_eq!(call(&mut rt, get_seed, &[fixed, pos]), V::J(42));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `BlockState.is(Holder)`: the state's block is the one the holder holds (FramedBlocks checks
    /// an item model's source state against its `DeferredBlock`). A level's side is its own: the
    /// client's level is on the client with no game around (Mekanism's level load then makes no
    /// saved data).
    #[test]
    fn a_state_is_its_blocks_holder_and_a_level_its_own_side() {
        let mut rt = Rt::new();
        crate::install();
        let (cube, slope) = (rt.new_object("test/Cube"), rt.new_object("test/Slope"));
        rt.registered.insert(("block".into(), "fb:cube".into()), cube);
        rt.registered.insert(("block".into(), "fb:slope".into()), slope);
        let held = crate::neo::registry_holder(&mut rt, "block", "fb:cube", cube);
        let other = crate::neo::registry_holder(&mut rt, "block", "fb:slope", slope);
        let st = state(&mut rt, "fb:cube[solid=false]");
        let is = "net/minecraft/world/level/block/state/BlockState.is.(Lnet/minecraft/core/Holder;)Z";
        assert_eq!(call(&mut rt, is, &[st, held]), V::I(1));
        assert_eq!(call(&mut rt, is, &[st, other]), V::I(0));

        rt.interned.insert("compat:client".into(), V::I(1));
        let level = crate::glue::level(&mut rt);
        assert_eq!(call(&mut rt, "net/minecraft/world/level/Level.isClientSide.()Z", &[level]), V::I(1));
    }

    /// The neighbour updates a mod asks the level for reach it in the order asked, with the block
    /// that changed: `updateNeighborsAt` (and `blockUpdated`, the same on a level), one neighbour
    /// left out (`updateNeighborsAtExceptFromFacing`), one block's `neighborChanged` with where the
    /// change is, `updateNeighbourForOutputSignal`, and a block entity's `setChanged` once it is in
    /// a level (comparators read it again). A client's level asks for none.
    #[test]
    fn a_mods_neighbour_updates_reach_the_level() {
        use crate::host::{MemoryHost, NeighborUpdate};
        const AROUND: &str = "net/minecraft/world/level/Level.updateNeighborsAt.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V";
        const UPDATED: &str = "net/minecraft/world/level/Level.blockUpdated.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V";
        const EXCEPT: &str = "net/minecraft/world/level/Level.updateNeighborsAtExceptFromFacing.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;Lnet/minecraft/core/Direction;)V";
        const ONE: &str = "net/minecraft/world/level/Level.neighborChanged.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;Lnet/minecraft/core/BlockPos;)V";
        const OUTPUT: &str = "net/minecraft/world/level/Level.updateNeighbourForOutputSignal.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V";
        const SET_CHANGED: &str = "net/minecraft/world/level/block/entity/BlockEntity.setChanged.()V";
        let ask = |client: bool| {
            let mut rt = Rt::new();
            crate::install();
            let mut host = MemoryHost { client, ..MemoryHost::default() };
            rt.with_host(&mut host, |rt| {
                let l = crate::glue::level(rt);
                let (at, above, vault_at) = (block_pos(rt, 1, 64, 2), block_pos(rt, 1, 65, 2), block_pos(rt, 3, 64, 2));
                let (lamp, east) = (block(rt, "minecraft:redstone_lamp"), direction(rt, 5));
                call(rt, AROUND, &[l, at, lamp]);
                call(rt, UPDATED, &[l, at, lamp]);
                call(rt, EXCEPT, &[l, at, lamp, east]);
                call(rt, ONE, &[l, at, lamp, above]);
                call(rt, OUTPUT, &[l, at, lamp]);
                let vault = rt.new_object("test/VaultBlockEntity");
                let vault_state = state(rt, "test:vault[large=false]");
                rt.put_field(vault, "worldPosition", vault_at);
                rt.put_field(vault, "blockState", vault_state);
                call(rt, SET_CHANGED, &[vault]);
                rt.put_field(vault, "level", l);
                call(rt, SET_CHANGED, &[vault]);
            });
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
            host.neighbor_updates
        };
        let lamp = |u: NeighborUpdate| (u, "minecraft:redstone_lamp".to_owned());
        let at = [1, 64, 2];
        assert_eq!(
            ask(false),
            [
                lamp(NeighborUpdate::Around { pos: at, skip: None }),
                lamp(NeighborUpdate::Around { pos: at, skip: None }),
                lamp(NeighborUpdate::Around { pos: at, skip: Some(5) }),
                lamp(NeighborUpdate::One { pos: at, from: [1, 65, 2] }),
                lamp(NeighborUpdate::Output { pos: at }),
                (NeighborUpdate::Output { pos: [3, 64, 2] }, "test:vault".to_owned()),
            ]
        );
        assert_eq!(ask(true), []);
    }

    /// The game answers some of a mod's calls by running the mod's own code (a neighbour update
    /// reaches a nixie tube's `neighborChanged`, a signal read an analog lever's `getSignal`): the
    /// runtime, busy with the call, is there for those hooks while the game works on it.
    #[test]
    fn the_game_reaches_the_mods_own_code_inside_its_updates_and_signal_reads() {
        use crate::host::{Host, NeighborUpdate};
        struct Reentrant {
            at: usize,
            reached: Vec<&'static str>,
        }
        impl Reentrant {
            fn reach(&mut self, what: &'static str) -> bool {
                let ran = crate::reenter(self.at, |_| ()).is_some();
                if ran {
                    self.reached.push(what);
                }
                ran
            }
        }
        impl Host for Reentrant {
            fn block_state(&self, _: [i32; 3]) -> String {
                "minecraft:stone".into()
            }
            fn set_block_state(&mut self, _: [i32; 3], _: &str) -> bool {
                false
            }
            fn update_neighbors(&mut self, _: NeighborUpdate, _: &str) {
                self.reach("updateNeighborsAt");
            }
            fn signal(&mut self, _: [i32; 3], _: u8, direct: bool) -> i32 {
                if self.reach(if direct { "getDirectSignal" } else { "getSignal" }) { 7 } else { 0 }
            }
            fn neighbor_signal(&mut self, _: [i32; 3]) -> i32 {
                if self.reach("hasNeighborSignal") { 3 } else { 0 }
            }
        }
        const AROUND: &str = "net/minecraft/world/level/Level.updateNeighborsAt.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V";
        const SET_CHANGED: &str = "net/minecraft/world/level/block/entity/BlockEntity.setChanged.()V";
        const SIGNAL: &str = "net/minecraft/world/level/Level.getSignal.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)I";
        const NEIGHBOR: &str = "net/minecraft/world/level/Level.hasNeighborSignal.(Lnet/minecraft/core/BlockPos;)Z";
        const STRONG: &str = "net/minecraft/world/level/Level.getDirectSignalTo.(Lnet/minecraft/core/BlockPos;)I";
        let mut rt = Rt::new();
        crate::install();
        let mut host = Reentrant { at: rt.address(), reached: Vec::new() };
        let answers = rt.with_host(&mut host, |rt| {
            let l = crate::glue::level(rt);
            let (at, lamp, west) = (block_pos(rt, 1, 64, 2), block(rt, "minecraft:redstone_lamp"), direction(rt, 4));
            call(rt, AROUND, &[l, at, lamp]);
            let be = rt.new_object("test/LeverBlockEntity");
            let lever = state(rt, "test:analog_lever");
            for (field, v) in [("level", l), ("worldPosition", at), ("blockState", lever)] {
                rt.put_field(be, field, v);
            }
            call(rt, SET_CHANGED, &[be]);
            [call(rt, SIGNAL, &[l, at, west]).i(), call(rt, NEIGHBOR, &[l, at]).i(), call(rt, STRONG, &[l, at]).i()]
        });
        assert_eq!(answers, [7, 1, 7]);
        // The strong signal into a block is its six neighbours' (none of them 15 here).
        let mut expected = vec!["updateNeighborsAt", "updateNeighborsAt", "getSignal", "hasNeighborSignal"];
        expected.extend(["getDirectSignal"; 6]);
        assert_eq!(host.reached, expected);
    }

    /// Create's pipes ask for the chunk next door (`getChunk(x, z, ChunkStatus.FULL, false)`) and
    /// look for the fluid there only when one comes back: every chunk the level reaches is there and
    /// reads the level's blocks, by chunk coordinates or by a position. Chunk statuses keep
    /// vanilla's order and names.
    #[test]
    fn a_pipe_finds_the_chunk_next_door_and_the_water_in_it() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        host.blocks.insert([-34, 69, 9], "minecraft:water[level=0]".into());
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let full = get_static(rt, "net/minecraft/world/level/chunk/status/ChunkStatus.FULL");
            let get_chunk = "net/minecraft/world/level/Level.getChunk.(IILnet/minecraft/world/level/chunk/status/ChunkStatus;Z)Lnet/minecraft/world/level/chunk/ChunkAccess;";
            // -34 >> 4 is -3, 9 >> 4 is 0.
            let chunk = call(rt, get_chunk, &[level, V::I(-3), V::I(0), full, V::I(0)]);
            assert_ne!(chunk.r(), 0);
            let pos = block_pos(rt, -34, 69, 9);
            let read = "net/minecraft/world/level/chunk/LevelChunk.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
            let water = call(rt, read, &[chunk, pos]);
            assert_eq!(rt.str_of(water), Some("minecraft:water[level=0]"));
            let at = call(rt, "net/minecraft/world/level/LevelReader.getChunk.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/chunk/ChunkAccess;", &[level, pos]);
            let water = call(rt, read, &[at, pos]);
            assert_eq!(rt.str_of(water), Some("minecraft:water[level=0]"));
            assert_eq!(call(rt, "net/minecraft/world/level/Level.hasChunk.(II)Z", &[level, V::I(-3), V::I(0)]), V::I(1));
            // `FULL` is the last status, after `LIGHT`; `byName` knows the registry's names and gives
            // `empty` for others; `empty` is its own parent.
            let status = |rt: &mut Rt, f: &str| get_static(rt, Box::leak(format!("{CHUNK_STATUS}.{f}").into_boxed_str()));
            let (light, empty) = (status(rt, "LIGHT"), status(rt, "EMPTY"));
            let s = |rt: &mut Rt, n: &str, d: &str, args: &[V]| call(rt, Box::leak(format!("{CHUNK_STATUS}.{n}.{d}").into_boxed_str()), args);
            let order = "(Lnet/minecraft/world/level/chunk/status/ChunkStatus;)Z";
            assert_eq!(s(rt, "isOrAfter", order, &[full, light]), V::I(1));
            assert_eq!(s(rt, "isOrAfter", order, &[light, full]), V::I(0));
            assert_eq!(s(rt, "getIndex", "()I", &[full]), V::I(11));
            let name = s(rt, "getName", "()Ljava/lang/String;", &[full]);
            assert_eq!(rt.str_of(name), Some("minecraft:full"));
            let by_name = "(Ljava/lang/String;)Lnet/minecraft/world/level/chunk/status/ChunkStatus;";
            let light_name = rt.string("minecraft:light");
            assert_eq!(s(rt, "byName", by_name, &[light_name]), light);
            let unknown = rt.string("mymod:baked");
            assert_eq!(s(rt, "byName", by_name, &[unknown]), empty);
            assert_eq!(s(rt, "getParent", "()Lnet/minecraft/world/level/chunk/status/ChunkStatus;", &[empty]), empty);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own register class (Mekanism's `MekanismDeferredRegister`), made through NeoForge's
    /// constructor: its `createHolder` override makes the holders (its own class, whose `super(key)`
    /// names the entry). They are what `register` returns and `getEntries` lists, and they resolve to
    /// what the supplier made, in the register's registry.
    #[test]
    fn a_mods_own_register_makes_its_own_holders() {
        const REGISTER: &str = "test/MyRegister";
        const MY_HOLDER: &str = "test/MyHolder";
        fn overrides(class: &str, key: &str) -> bool {
            class == REGISTER && key.starts_with("createHolder.")
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            if short.starts_with("createHolder.") {
                let k = call(rt, "net/minecraft/resources/ResourceKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[args[1], args[2]]);
                let h = rt.new_object(MY_HOLDER);
                call(rt, "net/neoforged/neoforge/registries/DeferredHolder.<init>.(Lnet/minecraft/resources/ResourceKey;)V", &[h, k]);
                return h;
            }
            call(rt, key, args)
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        let register = rt.new_object(REGISTER);
        let registry = get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK_ENTITY_TYPE");
        let ns = rt.string("mek");
        call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister.<init>.(Lnet/minecraft/resources/ResourceKey;Ljava/lang/String;)V", &[register, registry, ns]);
        let (name, value) = (rt.string("chamber"), rt.new_object("test/ChamberType"));
        let supplier = call(&mut rt, "com/google/common/base/Suppliers.ofInstance.(Ljava/lang/Object;)Lcom/google/common/base/Supplier;", &[value]);
        let h = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister.register.(Ljava/lang/String;Ljava/util/function/Supplier;)Lnet/neoforged/neoforge/registries/DeferredHolder;", &[register, name, supplier]);
        assert_eq!(rt.class_of(h), MY_HOLDER);
        let entries = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister.getEntries.()Ljava/util/Collection;", &[register]);
        assert_eq!(crate::jutil::elements(&rt, entries), vec![h]);
        assert_eq!(call(&mut rt, "net/neoforged/neoforge/registries/DeferredHolder.get.()Ljava/lang/Object;", &[h]), value);
        let reg = rt.str_of(registry).unwrap_or("").to_owned();
        assert_eq!(rt.registered.get(&(reg, "mek:chamber".to_owned())), Some(&value));
    }

    /// NeoForge's shorthands, as `DeferredRegister.Blocks`/`Items` define them: `registerSimpleBlock`
    /// is `registerBlock(name, Block::new, properties)`, `registerSimpleItem(name)` an `Item` with
    /// new properties, `registerSimpleBlockItem(holder)` a `BlockItem` of the holder's block under
    /// the block's name (SecurityCraft's), made when registries fire.
    #[test]
    fn neoforges_simple_registrations_make_what_neoforge_makes() {
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            call(rt, key, args)
        }
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        let ns = rt.string("sc");
        let blocks = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister.createBlocks.(Ljava/lang/String;)Lnet/neoforged/neoforge/registries/DeferredRegister$Blocks;", &[ns]);
        let items = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister.createItems.(Ljava/lang/String;)Lnet/neoforged/neoforge/registries/DeferredRegister$Items;", &[ns]);
        let (plate, gem) = (rt.string("plate"), rt.string("gem"));
        let props = rt.new_object(crate::blocks::BLOCK_PROPERTIES);
        let block = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister$Blocks.registerSimpleBlock.(Ljava/lang/String;Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)Lnet/neoforged/neoforge/registries/DeferredBlock;", &[blocks, plate, props]);
        let item = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister$Items.registerSimpleBlockItem.(Lnet/minecraft/core/Holder;)Lnet/neoforged/neoforge/registries/DeferredItem;", &[items, block]);
        let gem = call(&mut rt, "net/neoforged/neoforge/registries/DeferredRegister$Items.registerSimpleItem.(Ljava/lang/String;)Lnet/neoforged/neoforge/registries/DeferredItem;", &[items, gem]);
        crate::neo::resolve(&mut rt);
        let got = |rt: &mut Rt, h: V| call(rt, "net/neoforged/neoforge/registries/DeferredHolder.get.()Ljava/lang/Object;", &[h]);
        let (b, i, g) = (got(&mut rt, block), got(&mut rt, item), got(&mut rt, gem));
        assert_eq!(rt.class_of(b), "net/minecraft/world/level/block/Block");
        assert_eq!(rt.class_of(i), "net/minecraft/world/item/BlockItem");
        assert_eq!(rt.get_field(i, "$block"), b);
        assert_eq!(rt.class_of(g), "net/minecraft/world/item/Item");
        assert_eq!(rt.registered.get(&("item".to_owned(), "sc:plate".to_owned())), Some(&i));
        assert_eq!(rt.registered.get(&("item".to_owned(), "sc:gem".to_owned())), Some(&g));
        assert!(rt.thrown.is_none());
    }

    /// A function that maps an optional by mapping it again, forever: the JVM would end it with a
    /// `StackOverflowError` the code may catch; here too, past [`MAX_DEPTH`] nested API calls,
    /// and the calls unwind instead of overflowing the thread's stack.
    #[test]
    fn an_api_call_looping_back_into_itself_throws_stack_overflow_error() {
        const MAP: &str = "java/util/Optional.map.(Ljava/util/function/Function;)Ljava/util/Optional;";
        thread_local! {
            static AGAIN: std::cell::Cell<(V, V)> = const { std::cell::Cell::new((V::Void, V::Void)) };
        }
        fn again(rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
            let (optional, f) = AGAIN.with(|a| a.get());
            Some(call(rt, MAP, &[optional, f]))
        }
        let run = || {
            let mut rt = Rt::new();
            rt.lambda_call = Some(again);
            let f = rt.lambda_id("java/util/function/Function", 1, Vec::new());
            let x = rt.string("x");
            let optional = call(&mut rt, "java/util/Optional.of.(Ljava/lang/Object;)Ljava/util/Optional;", &[x]);
            AGAIN.with(|a| a.set((optional, f)));
            call(&mut rt, MAP, &[optional, f]);
            let e = rt.thrown.take().expect("thrown");
            // Every level came back out: the next call starts at the top again.
            (rt.class_of(e).to_owned(), DEPTH.with(|d| d.get()))
        };
        // Each level is a few frames; a debug build's are large.
        let (thrown, depth) = std::thread::Builder::new().stack_size(256 << 20).spawn(run).unwrap().join().unwrap();
        assert_eq!(thrown, "java/lang/StackOverflowError");
        assert_eq!(depth, 0);
    }

    /// `new RuntimeException(cause)` as Java makes it (Max Henkel's corelib wraps every
    /// reflection failure so): its message is the cause's `toString()`, and the cause is kept.
    #[test]
    fn an_exception_made_from_a_cause_says_what_the_cause_was() {
        let mut rt = Rt::new();
        let inner = rt.new_object("java/lang/IllegalStateException");
        let boom = rt.string("boom");
        call(&mut rt, "java/lang/IllegalStateException.<init>.(Ljava/lang/String;)V", &[inner, boom]);
        let outer = rt.new_object("java/lang/RuntimeException");
        call(&mut rt, "java/lang/RuntimeException.<init>.(Ljava/lang/Throwable;)V", &[outer, inner]);
        let m = call(&mut rt, "java/lang/RuntimeException.getMessage.()Ljava/lang/String;", &[outer]);
        assert_eq!(rt.str_of(m), Some("java.lang.IllegalStateException: boom"));
        assert_eq!(call(&mut rt, "java/lang/RuntimeException.getCause.()Ljava/lang/Throwable;", &[outer]), inner);
        assert_eq!(call(&mut rt, "java/lang/IllegalStateException.getCause.()Ljava/lang/Throwable;", &[inner]), V::NULL);
    }

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
    fn a_vec3i_moves_as_vec3i_and_a_block_pos_as_itself() {
        let mut rt = Rt::new();
        let v = rt.new_object("net/minecraft/core/Vec3i");
        call(&mut rt, "net/minecraft/core/Vec3i.<init>.(III)V", &[v, V::I(1), V::I(2), V::I(3)]);
        let y = get_static(&mut rt, "net/minecraft/core/Direction$Axis.Y");
        let up = call(&mut rt, "net/minecraft/core/Vec3i.relative.(Lnet/minecraft/core/Direction$Axis;I)Lnet/minecraft/core/Vec3i;", &[v, y, V::I(2)]);
        assert_eq!((rt.class_of(up), native(&rt, up)[..3].to_vec()), ("net/minecraft/core/Vec3i", vec![1, 4, 3]));
        // `relative(axis, 0)` is the same object, as vanilla returns `this`.
        assert_eq!(call(&mut rt, "net/minecraft/core/Vec3i.relative.(Lnet/minecraft/core/Direction$Axis;I)Lnet/minecraft/core/Vec3i;", &[v, y, V::I(0)]), v);
        let w = rt.new_object("net/minecraft/core/Vec3i");
        call(&mut rt, "net/minecraft/core/Vec3i.<init>.(III)V", &[w, V::I(0), V::I(0), V::I(1)]);
        let cross = call(&mut rt, "net/minecraft/core/Vec3i.cross.(Lnet/minecraft/core/Vec3i;)Lnet/minecraft/core/Vec3i;", &[v, w]);
        assert_eq!(native(&rt, cross)[..3], [2, -1, 0]);
        // Called as `Vec3i`'s on a position, the answer is a position (`BlockPos` overrides it).
        let p = block_pos(&mut rt, 5, 5, 5);
        let q = call(&mut rt, "net/minecraft/core/Vec3i.offset.(III)Lnet/minecraft/core/Vec3i;", &[p, V::I(1), V::I(0), V::I(-1)]);
        assert_eq!((rt.class_of(q), native(&rt, q)[..3].to_vec()), (BLOCK_POS, vec![6, 5, 4]));
        // A block-state enum gives its state name.
        let double = get_static(&mut rt, "net/minecraft/world/level/block/state/properties/SlabType.DOUBLE");
        let n = call(&mut rt, "net/minecraft/world/level/block/state/properties/SlabType.getSerializedName.()Ljava/lang/String;", &[double]);
        assert_eq!(rt.str_of(n), Some("double"));
    }

    /// An engine holding a mod and an addon of it: each `DeferredRegister` names what it registers
    /// in the mod id it was created with, whichever registered first.
    #[test]
    fn each_deferred_register_names_in_its_own_mod_id() {
        let mut rt = Rt::new();
        crate::install();
        const CREATE: &str = "net/neoforged/neoforge/registries/DeferredRegister.create.(Lnet/minecraft/resources/ResourceKey;Ljava/lang/String;)Lnet/neoforged/neoforge/registries/DeferredRegister;";
        const REGISTER: &str = "net/neoforged/neoforge/registries/DeferredRegister.register.(Ljava/lang/String;Ljava/util/function/Supplier;)Lnet/neoforged/neoforge/registries/DeferredHolder;";
        let blocks = get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK");
        let register = |rt: &mut Rt, ns: &str, name: &str| {
            let ns = rt.string(ns);
            let r = call(rt, CREATE, &[blocks, ns]);
            let name = rt.string(name);
            let h = call(rt, REGISTER, &[r, name, V::NULL]);
            rt.str_of(rt.get_field(h, "$id")).unwrap_or("").to_owned()
        };
        assert_eq!(register(&mut rt, "create", "shaft"), "create:shaft");
        assert_eq!(register(&mut rt, "create_new_age", "generator_coil"), "create_new_age:generator_coil");
        assert_eq!(register(&mut rt, "create_new_age", "create:named"), "create:named");
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

    /// A hook stuck in a loop on a call nothing maps (its stand-in answer never ends the loop) is
    /// stopped with a `java.lang.Error` naming the call, once it has made a million such calls over
    /// ten seconds; the next hook starts afresh, and code outside hooks (loading) is never stopped.
    #[test]
    fn a_hook_looping_on_an_unmapped_call_is_stopped() {
        struct Game;
        impl crate::host::Host for Game {
            fn block_state(&self, _pos: [i32; 3]) -> String {
                "minecraft:air".into()
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
        }
        let key = "net/minecraft/Nope.next.()I";
        let long_ago = || std::time::Instant::now().checked_sub(RUNAWAY_TIME).expect("a clock older than the limit");
        let mut rt = Rt::new();
        rt.hook_misses = RUNAWAY_MISSES;
        rt.hook_first_miss = Some(long_ago());
        call(&mut rt, key, &[]);
        assert!(rt.thrown.is_none(), "loading is never stopped");
        let mut game = Game;
        rt.with_host(&mut game, |rt| {
            call(rt, key, &[]);
            assert!(rt.thrown.is_none(), "a hook's first unmapped calls answer their stand-in");
            rt.hook_misses = RUNAWAY_MISSES - 1;
            rt.hook_first_miss = Some(long_ago());
            call(rt, key, &[]);
            let e = rt.thrown.take().expect("a hook stuck on an unmapped call is stopped");
            assert_eq!(rt.class_of(e), "java/lang/Error");
            assert!(rt.str_of(rt.get_field(e, "message")).is_some_and(|m| m.contains(key)));
        });
        rt.with_host(&mut game, |rt| {
            call(rt, key, &[]);
            assert!(rt.thrown.is_none(), "the next hook starts afresh");
        });
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

    const IS_FACE_STURDY: &str = "net/minecraft/world/level/block/state/BlockState.isFaceSturdy.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Z";
    const IS_FACE_STURDY_AS: &str = "net/minecraft/world/level/block/state/BlockState.isFaceSturdy.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/SupportType;)Z";
    const SUPPORT_SHAPE_OF_STATE: &str = "net/minecraft/world/level/block/state/BlockState.getBlockSupportShape.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
    const PX: f64 = 1.0 / 16.0;

    /// Mod blocks for the support tests: a post, a bottom slab, a thin pillar the mod says holds
    /// as a whole block, and a plain block.
    fn shaped_blocks(rt: &mut Rt) {
        fn overrides(class: &str, short: &str) -> bool {
            matches!(class, "test/Post" | "test/Slab" | "test/Pillar") && short == GET_SHAPE || class == "test/Pillar" && short == GET_SUPPORT_SHAPE
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match (rt.class_of(args[0]), short) {
                ("test/Post" | "test/Pillar", GET_SHAPE) => crate::shapes::shape(rt, &[[6.0 * PX, 0.0, 6.0 * PX, 10.0 * PX, 1.0, 10.0 * PX]]),
                ("test/Slab", GET_SHAPE) => crate::shapes::shape(rt, &[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]),
                ("test/Pillar", GET_SUPPORT_SHAPE) => crate::shapes::shape(rt, &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]),
                _ => call(rt, key, args),
            }
        }
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        for (class, name) in [("test/Post", "test:post"), ("test/Slab", "test:slab"), ("test/Pillar", "test:pillar"), ("test/Gate", "test:gate")] {
            rt.supers.insert(class, "net/minecraft/world/level/block/Block");
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), name.into()), b);
        }
    }

    /// `isFaceSturdy`: a vanilla block's answer is the game's; a mod block's comes from its
    /// support shape, tested as 1.21.1's `SupportType` tests a face. A post's top holds a torch's
    /// middle but not a rail's rim, a bottom slab's side not even a torch. `canSupportCenter` and
    /// `canSupportRigidBlock` ask about the block where they look, but nothing hangs under an
    /// `unstable_bottom_center` block.
    #[test]
    fn a_faces_sturdiness_is_the_games_or_the_mod_blocks_support_shape() {
        let mut rt = Rt::new();
        crate::install();
        shaped_blocks(&mut rt);
        let mut host = crate::host::MemoryHost::default();
        // An unconnected fence, as the game exports it: only the middles of its top and bottom.
        let fence = crate::host::BlockFacts { hardness: 2.0, push: 0, replaceable: false, sturdy: 1 << (6 + 1) | 1 << 6 };
        host.facts.insert("minecraft:oak_fence".into(), fence);
        host.facts.insert("minecraft:stone".into(), crate::host::BlockFacts { hardness: 1.5, push: 0, replaceable: false, sturdy: (1 << 18) - 1 });
        host.blocks.insert([0, 64, 0], "minecraft:stone".into());
        host.blocks.insert([1, 64, 0], "test:gate".into());
        host.blocks.insert([2, 64, 0], "test:post".into());
        host.block_tags.insert("minecraft:unstable_bottom_center".into(), vec!["test:gate".into()]);
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let pos = block_pos(rt, 0, 64, 0);
            let (down, up, north) = (direction(rt, 0), direction(rt, 1), direction(rt, 2));
            let center = get_static(rt, "net/minecraft/world/level/block/SupportType.CENTER");
            let rigid = get_static(rt, "net/minecraft/world/level/block/SupportType.RIGID");
            let sturdy = |rt: &mut Rt, text: &str, face: V, support: Option<V>| {
                let st = state(rt, text);
                let held = match support {
                    None => call(rt, IS_FACE_STURDY, &[st, level, pos, face]),
                    Some(s) => call(rt, IS_FACE_STURDY_AS, &[st, level, pos, face, s]),
                };
                held == V::I(1)
            };
            let fence = "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]";
            assert!(!sturdy(rt, fence, up, None));
            assert!(sturdy(rt, fence, up, Some(center)));
            assert!(!sturdy(rt, "test:post", up, None));
            assert!(sturdy(rt, "test:post", up, Some(center)));
            assert!(!sturdy(rt, "test:post", up, Some(rigid)));
            assert!(!sturdy(rt, "test:post", north, Some(center)));
            assert!(sturdy(rt, "test:slab", down, None));
            assert!(!sturdy(rt, "test:slab", up, Some(center)));
            assert!(!sturdy(rt, "test:slab", north, Some(center)));
            assert!(sturdy(rt, "test:pillar", up, None));
            assert!(sturdy(rt, "test:pillar", north, Some(rigid)));

            let (stone, gate, post, air) = (pos, block_pos(rt, 1, 64, 0), block_pos(rt, 2, 64, 0), block_pos(rt, 5, 64, 0));
            let center_at = |rt: &mut Rt, at: V, face: V| call(rt, "net/minecraft/world/level/block/Block.canSupportCenter.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Z", &[level, at, face]) == V::I(1);
            assert!(center_at(rt, stone, down));
            assert!(!center_at(rt, gate, down));
            assert!(center_at(rt, gate, up));
            let rigid_at = |rt: &mut Rt, at: V| call(rt, "net/minecraft/world/level/block/Block.canSupportRigidBlock.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z", &[level, at]) == V::I(1);
            assert!(rigid_at(rt, stone));
            assert!(!rigid_at(rt, post));
            assert!(!rigid_at(rt, air));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `getBlockSupportShape`, as 1.21.1 finds it: a vanilla class's own (leaves hold nothing up, a
    /// snow layer by its layers, a closed gate a bar above its middle), also for a mod block
    /// extending the class; else, as `BlockBehaviour`'s, the collision shape: none for a block
    /// without collision, whatever its outline. A mod block's `super.` calls reach the same.
    #[test]
    fn a_support_shape_is_the_classes_own_else_the_collision_shape() {
        let mut rt = Rt::new();
        crate::install();
        shaped_blocks(&mut rt);
        rt.supers.insert("test/Hedge", "net/minecraft/world/level/block/LeavesBlock");
        rt.supers.insert("test/Ghost", "net/minecraft/world/level/block/Block");
        let (hedge, ghost) = (rt.new_object("test/Hedge"), rt.new_object("test/Ghost"));
        let props = rt.new_object(crate::blocks::BLOCK_PROPERTIES);
        call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.noCollission.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[props]);
        rt.put_field(ghost, "$properties", props);
        rt.registered.insert(("block".into(), "test:hedge".into()), hedge);
        rt.registered.insert(("block".into(), "test:ghost".into()), ghost);
        let level = crate::glue::level(&mut rt);
        let pos = block_pos(&mut rt, 0, 64, 0);
        let support = |rt: &mut Rt, text: &str| {
            let st = state(rt, text);
            let shape = call(rt, SUPPORT_SHAPE_OF_STATE, &[st, level, pos]);
            crate::shapes::boxes(rt, shape).expect("a shape")
        };
        let none: Vec<crate::shapes::Box3> = Vec::new();
        assert_eq!(support(&mut rt, "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]"), none);
        assert_eq!(support(&mut rt, "minecraft:snow[layers=3]"), vec![[0.0, 0.0, 0.0, 1.0, 6.0 * PX, 1.0]]);
        let gate = "minecraft:oak_fence_gate[facing=east,in_wall=false,open=false,powered=false]";
        assert_eq!(support(&mut rt, gate), vec![[6.0 * PX, 5.0 * PX, 0.0, 10.0 * PX, 1.5, 1.0]]);
        assert_eq!(support(&mut rt, "minecraft:stone"), vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]);
        assert_eq!(support(&mut rt, "test:hedge[distance=7,persistent=false,waterlogged=false]"), none);
        assert_eq!(support(&mut rt, "test:ghost"), none);
        assert_eq!(support(&mut rt, "test:post"), vec![[6.0 * PX, 0.0, 6.0 * PX, 10.0 * PX, 1.0, 10.0 * PX]]);

        let ctx = crate::shapes::empty_context(&mut rt);
        let ghost_state = state(&mut rt, "test:ghost");
        let collision = "net/minecraft/world/level/block/Block.getCollisionShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
        let shape = call(&mut rt, collision, &[ghost, ghost_state, level, pos, ctx]);
        assert_eq!(crate::shapes::boxes(&rt, shape), Some(none.clone()));
        let hedge_state = state(&mut rt, "test:hedge[distance=7,persistent=false,waterlogged=false]");
        let leaves = "net/minecraft/world/level/block/LeavesBlock.getBlockSupportShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
        let shape = call(&mut rt, leaves, &[hedge, hedge_state, level, pos]);
        assert_eq!(crate::shapes::boxes(&rt, shape), Some(none));
        let post = rt.registered[&("block".to_owned(), "test:post".to_owned())];
        let post_state = state(&mut rt, "test:post");
        let behaviours = "net/minecraft/world/level/block/Block.getBlockSupportShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
        let shape = call(&mut rt, behaviours, &[post, post_state, level, pos]);
        assert_eq!(crate::shapes::boxes(&rt, shape), Some(vec![[6.0 * PX, 0.0, 6.0 * PX, 10.0 * PX, 1.0, 10.0 * PX]]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `state.isCollisionShapeFullBlock(level, pos)`: whether the state's collision shape where it
    /// stands is the whole block, its boxes together (a double slab's two halves) as well as one.
    #[test]
    fn a_collision_shape_is_a_full_block_only_when_it_fills_the_block() {
        const FULL: &str = "net/minecraft/world/level/block/state/BlockState.isCollisionShapeFullBlock.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
        let mut rt = Rt::new();
        crate::install();
        let mut h = crate::host::MemoryHost::default();
        let bottom = [0.0, 0.0, 0.0, 1.0, 0.5, 1.0];
        let top = [0.0, 0.5, 0.0, 1.0, 1.0, 1.0];
        for (x, block, boxes) in [
            (0, "minecraft:stone", None),
            (
                1,
                "minecraft:oak_slab[type=bottom,waterlogged=false]",
                Some(vec![bottom]),
            ),
            (
                2,
                "minecraft:oak_slab[type=double,waterlogged=false]",
                Some(vec![bottom, top]),
            ),
            (3, "minecraft:air", None),
        ] {
            h.blocks.insert([x, 64, 0], block.to_owned());
            if let Some(b) = boxes {
                h.collisions.insert([x, 64, 0], b);
            }
        }
        let full = rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            (0..4)
                .map(|x| {
                    let text = rt
                        .host()
                        .map(|h| h.block_state([x, 64, 0]))
                        .unwrap_or_default();
                    let (s, p) = (state(rt, &text), block_pos(rt, i64::from(x), 64, 0));
                    call(rt, FULL, &[s, level, p])
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(full, [V::I(1), V::I(0), V::I(1), V::I(0)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
