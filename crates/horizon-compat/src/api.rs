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
        // NeoForge's spawn egg of a type registered later is vanilla's spawn egg.
        "net/neoforged/neoforge/common/DeferredSpawnEggItem" => {
            Some("net/minecraft/world/item/SpawnEggItem")
        }
        _ => crate::models::api_super(class)
            .or_else(|| crate::entity_render::api_super(class))
            .or_else(|| crate::camera::api_super(class))
            .or_else(|| crate::item_wrappers::api_super(class))
            .or_else(|| crate::vanilla_entities::api_super(class))
            .or_else(|| crate::particle_engine::api_super(class))
            .or_else(|| crate::goals::api_super(class))
            .or_else(|| crate::mobs::api_super(class)),
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
    /// How many API calls this thread has answered.
    static CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// The piece of a mod's code that runs on this thread ([`timed_start`]).
    static PIECE: std::cell::Cell<Option<Piece>> = const { std::cell::Cell::new(None) };
}

/// A piece of a mod's code on its clock: when it started, how long it may run, and whether it
/// has been stopped for running longer.
#[derive(Clone, Copy)]
pub(crate) struct Piece {
    since: std::time::Instant,
    limit: std::time::Duration,
    stopped: bool,
}

/// How long one piece of a mod's code may run while the game loads (a mod's constructor, one
/// listener hearing one event) before it is stopped: on a JVM none takes a second, and one that
/// loops here without end (or a hundred times slower than there) would hold every other mod up.
/// The API call it is in then throws [`STOPPED`], which unwinds the piece as an error of its own
/// would, and so does every call it makes after that, whatever caught the first: the game goes
/// on to the next.
const PIECE_TIME: std::time::Duration = std::time::Duration::from_secs(90);
/// The clock is read once in this many API calls.
const PIECE_CHECK: u64 = 1 << 20;
/// What a piece stopped for its time is thrown: the error a JVM throws at code it cannot let go
/// on, and what the runtime throws at code that calls too deep. Not an `Exception`, so a
/// `catch (Exception e)` does not swallow it; and of the errors code that catches everything
/// tells apart, the one it lets go. (It was a plain `Error`: JourneyMap's map task ran past
/// its time, caught it as a `Throwable`, and looked for its kind in a loop that ends for a
/// stack overflow, an out of memory, a linkage error or an exception, and for nothing else.
/// No call of the runtime's is made in that loop, so nothing could stop it again.)
const STOPPED: &str = "java/lang/StackOverflowError";

/// Starts the clock of a piece of a mod's code ([`PIECE_TIME`]); gives back the one that ran, for
/// [`timed_end`] when this piece is done.
pub(crate) fn timed_start() -> Option<Piece> {
    timed_start_within(PIECE_TIME)
}

/// [`timed_start`] for a piece that may run `limit` ([`crate::tasks`]: a pool's work in a
/// running game, which holds the game's thread while it runs).
pub(crate) fn timed_start_within(limit: std::time::Duration) -> Option<Piece> {
    PIECE.replace(Some(Piece {
        since: std::time::Instant::now(),
        limit,
        stopped: false,
    }))
}

/// The next API call on this thread reads the clock, whatever the count is at.
#[cfg(test)]
pub(crate) fn next_call_reads_the_clock() {
    CALLS.set(PIECE_CHECK - 1);
}

/// Whether the piece that runs now has been stopped for its time.
pub(crate) fn timed_out() -> bool {
    PIECE.get().is_some_and(|piece| piece.stopped)
}

/// The piece [`timed_start`] began is done: the one around it goes on.
pub(crate) fn timed_end(outer: Option<Piece>) {
    PIECE.set(outer);
}

/// Counts an API call; whether the piece of code it is in has run past its time (read once in
/// [`PIECE_CHECK`] calls, and at every call once it has).
fn over_time() -> Option<u64> {
    let n = CALLS.get().wrapping_add(1);
    CALLS.set(n);
    let piece = PIECE.get()?;
    if !piece.stopped {
        if !n.is_multiple_of(PIECE_CHECK) {
            return None;
        }
        if piece.since.elapsed() < piece.limit {
            return None;
        }
        PIECE.set(Some(Piece {
            stopped: true,
            ..piece
        }));
    }
    Some(piece.since.elapsed().as_secs())
}

/// A point in the runtime's own code between two pieces of the mods' code it runs one after
/// another (two reload listeners): garbage is collected there when enough was made. Only where
/// none of the mods' code is running below, which is where no API call is in progress on this
/// thread (every call from their code into the runtime is one), and only from a caller whose
/// own callers hold no value of their own: a collection frees what nothing reachable names,
/// and a value a caller keeps in a local of its own is named by nothing. `keep` are the values
/// the caller itself still holds. The collector otherwise runs between hooks alone, so one
/// hook that runs many listeners (the client's first resource load) kept every listener's
/// garbage until its last.
pub(crate) fn between(rt: &mut Rt, keep: &[V], after: V) {
    if DEPTH.with(|d| d.get()) != 0 {
        return;
    }
    let pinned = rt.pinned.len();
    rt.pinned.extend_from_slice(keep);
    if census_asked() {
        // Every time, and counted: what the piece that just ran (`after`) left behind.
        let before = census(rt);
        rt.collect();
        let live = census(rt);
        let what = rt.class_of(after).to_owned();
        tell_census(&what, &before, &live);
    } else {
        rt.maybe_collect();
    }
    rt.pinned.truncate(pinned);
}

/// When the census is asked for: everything the heap holds at this point, garbage with the
/// rest (no collection: the callers here hold values of their own).
pub(crate) fn census_of_all(rt: &Rt, after: &str) {
    if census_asked() {
        let all = census(rt);
        tell_census(after, &all, &all);
    }
}

/// Whether `HORIZON_COMPAT_CENSUS` asks for the heap to be counted at each [`between`].
fn census_asked() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| std::env::var_os("HORIZON_COMPAT_CENSUS").is_some())
}

/// A class's objects and about how many bytes they take, by class.
type Census = std::collections::HashMap<&'static str, (u64, u64)>;

/// What the heap holds: each class's objects and about the bytes they take (the object and what
/// its data owns), freed slots apart.
pub(crate) fn census(rt: &Rt) -> Census {
    use std::mem::size_of;
    let mut by = Census::new();
    for o in rt.heap.iter().skip(1).filter(|o| o.class != "compat:free") {
        let class = by.entry(o.class).or_default();
        class.0 += 1;
        class.1 += (size_of::<crate::Object>() + owned(o)) as u64;
    }
    by
}

/// About the bytes an object's data owns beside the object itself.
pub(crate) fn owned(o: &crate::Object) -> usize {
    use std::mem::size_of;
    match &o.data {
        Data::Fields(f) => f.capacity() * (size_of::<(&str, V)>() + 1),
        Data::Str(s) => s.capacity(),
        Data::Array(a) | Data::List(a) | Data::Stream(a) | Data::Iter(a, _) => {
            a.capacity() * size_of::<V>()
        }
        Data::Map(m) => {
            m.capacity() * (size_of::<(String, V)>() + 1)
                + m.keys().map(String::capacity).sum::<usize>()
        }
        Data::Table(t) => {
            t.entries.capacity() * size_of::<(V, V)>()
                + t.index.capacity() * (size_of::<(i32, Vec<usize>)>() + 1)
                + t.index
                    .values()
                    .map(|at| at.capacity() * size_of::<usize>())
                    .sum::<usize>()
        }
        Data::Native(..) => 0,
        Data::Bytes(b, _) => b.capacity(),
    }
}

/// The largest objects of the heap, told with what holds each (a field of an object, an
/// element of an array or a collection, a static), two holders up: a pack's memory is often
/// a few of them, and the class alone (an array) does not say whose.
pub(crate) fn tell_largest(rt: &Rt, how_many: usize, at_least: usize) {
    let mut big: Vec<(usize, usize)> = rt
        .heap
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, o)| o.class != "compat:free")
        .map(|(r, o)| (owned(o), r))
        .filter(|(bytes, _)| *bytes >= at_least)
        .collect();
    if big.is_empty() {
        return;
    }
    // How the bytes spread over sizes: whether it is a few giants or many of a kind.
    let mut spread = [(0usize, 0usize); 5];
    for (bytes, _) in &big {
        let at = match bytes >> 20 {
            64.. => 0,
            16..64 => 1,
            4..16 => 2,
            1..4 => 3,
            _ => 4,
        };
        spread[at].0 += 1;
        spread[at].1 += bytes;
    }
    eprintln!(
        "    large: {} of 64 MB or more ({} MB), {} of 16 ({} MB), {} of 4 ({} MB), {} of 1 ({} MB), {} smaller ({} MB)",
        spread[0].0,
        spread[0].1 >> 20,
        spread[1].0,
        spread[1].1 >> 20,
        spread[2].0,
        spread[2].1 >> 20,
        spread[3].0,
        spread[3].1 >> 20,
        spread[4].0,
        spread[4].1 >> 20
    );
    big.sort_unstable_by_key(|b| std::cmp::Reverse(b.0));
    big.truncate(how_many);
    // What holds object `r`: the first few holders found in one pass over the heap.
    let holders = |wanted: &[usize]| -> Vec<Vec<String>> {
        let mut found: Vec<Vec<String>> = vec![Vec::new(); wanted.len()];
        let is = |v: &V| wanted.iter().position(|w| v.r() as usize == *w && *w != 0);
        for (name, v) in &rt.statics {
            if let Some(i) = is(v) {
                found[i].push(format!("static {name}"));
            }
        }
        for (name, v) in &rt.interned {
            if let Some(i) = is(v) {
                found[i].push(format!("kept as {name}"));
            }
        }
        for (r, o) in rt.heap.iter().enumerate().skip(1) {
            let mut hit = |i: usize, how: String| {
                if found[i].len() < 3 {
                    found[i].push(format!("{how} of {} #{r}", o.class));
                }
            };
            match &o.data {
                Data::Fields(f) => f.iter().for_each(|(name, v)| {
                    if let Some(i) = is(v) {
                        hit(i, format!("field {name}"));
                    }
                }),
                Data::Array(a) | Data::List(a) | Data::Stream(a) | Data::Iter(a, _) => {
                    if let Some(i) = a.iter().find_map(&is) {
                        hit(i, "an element".to_owned());
                    }
                }
                Data::Map(m) => {
                    if let Some(i) = m.values().find_map(&is) {
                        hit(i, "a value".to_owned());
                    }
                }
                Data::Table(t) => {
                    if let Some(i) = t.entries.iter().find_map(|(k, v)| is(k).or_else(|| is(v))) {
                        hit(i, "an entry".to_owned());
                    }
                }
                _ => {}
            }
        }
        found
    };
    let first: Vec<usize> = big.iter().map(|b| b.1).collect();
    let held = holders(&first);
    // One holder up again, for the first holder of each.
    let above: Vec<usize> = held
        .iter()
        .map(|h| {
            h.first()
                .and_then(|h| h.rsplit_once('#'))
                .and_then(|(_, r)| r.parse().ok())
                .unwrap_or(0)
        })
        .collect();
    let held_above = holders(&above);
    for (i, (bytes, r)) in big.iter().enumerate() {
        let o = &rt.heap[*r];
        let shape = match &o.data {
            Data::Array(a) | Data::List(a) => {
                let kinds: Vec<&str> = a
                    .iter()
                    .take(3)
                    .map(|v| match v {
                        V::I(_) => "int",
                        V::J(_) => "long",
                        V::F(_) => "float",
                        V::D(_) => "double",
                        V::R(0) | V::Void => "null",
                        V::R(_) => rt.class_of(*v),
                    })
                    .collect();
                format!("{} elements ({})", a.len(), kinds.join(", "))
            }
            Data::Table(t) => format!("{} entries", t.entries.len()),
            Data::Bytes(b, _) => format!("{} bytes", b.len()),
            Data::Str(s) => format!("{} chars", s.len()),
            _ => String::new(),
        };
        eprintln!(
            "    largest: {} MB {} #{r} {shape}; held by {}; that by {}",
            bytes >> 20,
            o.class,
            if held[i].is_empty() {
                "nothing found".to_owned()
            } else {
                held[i].join(", ")
            },
            if held_above[i].is_empty() {
                "nothing found".to_owned()
            } else {
                held_above[i].join(", ")
            }
        );
    }
}

/// One line for a collection: what stayed and what went, with the classes that take the most.
fn tell_census(after: &str, before: &Census, live: &Census) {
    let total = |c: &Census| c.values().fold((0u64, 0u64), |t, v| (t.0 + v.0, t.1 + v.1));
    let top = |c: &Census| {
        let mut all: Vec<(&&'static str, &(u64, u64))> =
            c.iter().filter(|(_, v)| v.1 > 0).collect();
        all.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
        let parts: Vec<String> = all
            .iter()
            .take(10)
            .map(|(k, v)| format!("{k} {} {}M", v.0, v.1 >> 20))
            .collect();
        parts.join(", ")
    };
    let freed: Census = before
        .iter()
        .map(|(k, v)| {
            let l = live.get(k).copied().unwrap_or_default();
            (*k, (v.0.saturating_sub(l.0), v.1.saturating_sub(l.1)))
        })
        .collect();
    let (b, l) = (total(before), total(live));
    eprintln!(
        "horizon-compat: census after {after}: {} objects {} MB live; freed {} objects {} MB\n    live: {}\n    freed: {}",
        l.0,
        l.1 >> 20,
        b.0 - l.0,
        (b.1.saturating_sub(l.1)) >> 20,
        top(live),
        top(&freed)
    );
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
    // A field of `null`: the JVM's exception, as for a call on it.
    if crate::linked::null_throws(o) {
        return crate::linked::null_receiver(rt, key);
    }
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
    if crate::linked::diagnosing() {
        return crate::linked::observed(rt, "API", key, args, |rt| call_unobserved(rt, key, args));
    }
    call_unobserved(rt, key, args)
}

/// What of a thread's stack is left to the runtime's own code under the mods': an API call
/// made with less than this below it throws `StackOverflowError`. The mods' code that calls
/// itself without end through its own methods alone never nests an API call
/// ([`MAX_DEPTH`]), but it makes one at every turn: JourneyMap's minimap, made for the first
/// time with a player in the world, asks for the minimap being made, and the game's stack
/// ended where a JVM throws an error the mod catches.
const STACK_RESERVE: usize = 1 << 20;

/// The lowest address on this thread's stack an API call may be made at ([`STACK_RESERVE`]
/// above the stack's end, a quarter of a small stack), found once for the thread.
fn stack_floor() -> usize {
    thread_local! {
        static FLOOR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    FLOOR.with(|floor| {
        if floor.get() == 0 {
            floor.set(match stack_bounds() {
                Some((low, size)) => low + STACK_RESERVE.min(size / 4),
                // No telling where it ends: nothing is too deep.
                None => 1,
            });
        }
        floor.get()
    })
}

/// This thread's stack: its lowest address and its size.
#[cfg(target_vendor = "apple")]
fn stack_bounds() -> Option<(usize, usize)> {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn pthread_self() -> *mut c_void;
        fn pthread_get_stackaddr_np(thread: *mut c_void) -> *mut c_void;
        fn pthread_get_stacksize_np(thread: *mut c_void) -> usize;
    }
    // SAFETY: both ask about the calling thread, which is alive; they read and return.
    let (top, size) = unsafe {
        let me = pthread_self();
        (
            pthread_get_stackaddr_np(me) as usize,
            pthread_get_stacksize_np(me),
        )
    };
    (size != 0 && top > size).then(|| (top - size, size))
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn stack_bounds() -> Option<(usize, usize)> {
    use std::ffi::{c_int, c_void};
    // `pthread_attr_t`: 56 bytes on x86-64, 64 on aarch64 (glibc and musl).
    #[repr(C, align(8))]
    struct Attr([u8; 64]);
    unsafe extern "C" {
        fn pthread_self() -> usize;
        fn pthread_getattr_np(thread: usize, attr: *mut Attr) -> c_int;
        fn pthread_attr_getstack(
            attr: *const Attr,
            low: *mut *mut c_void,
            size: *mut usize,
        ) -> c_int;
        fn pthread_attr_destroy(attr: *mut Attr) -> c_int;
    }
    let mut attr = Attr([0; 64]);
    let (mut low, mut size) = (std::ptr::null_mut(), 0usize);
    // SAFETY: the attributes are the calling thread's, filled by the first call, read by the
    // second and released by the third; `low` and `size` are this function's own.
    let found = unsafe {
        if pthread_getattr_np(pthread_self(), &mut attr) != 0 {
            return None;
        }
        let found = pthread_attr_getstack(&attr, &mut low, &mut size) == 0;
        pthread_attr_destroy(&mut attr);
        found
    };
    (found && size != 0).then_some((low as usize, size))
}

#[cfg(not(unix))]
fn stack_bounds() -> Option<(usize, usize)> {
    None
}

fn call_unobserved(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let depth = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get()
    });
    // (Where this call's own frame lies: a local's address.)
    let here = std::ptr::from_ref(&depth) as usize;
    let r = if depth > MAX_DEPTH || here < stack_floor() {
        // (One already thrown is on its way up: the code that unwinds asks nothing new.)
        if rt.thrown.is_none() {
            rt.throw_new("java/lang/StackOverflowError");
        }
        V::Void
    } else if let Some(secs) = over_time().filter(|_| rt.thrown.is_none()) {
        crate::jlang::throw_with(
            rt,
            STOPPED,
            &format!(
                "horizon-compat: this part of the mod ran for {secs} s without coming back to the game and was stopped (in {key})"
            ),
        );
        V::Void
    } else {
        let r = call_nested(rt, key, args);
        keep_null(rt, key);
        // A constructor of the game's has ended: what mixins inject at its end runs on the
        // object made, as in the constructor the mods' code called ([`crate::places`]).
        if is_constructor(key) {
            crate::places::constructor_ended(rt, key, args);
        }
        r
    };
    DEPTH.with(|d| d.set(d.get() - 1));
    r
}

/// Java's `null` is the heap's first object, an empty array, and stays one. A call on a `null`
/// receiver that writes its receiver's data in place wrote there: a `BitSet.set` on `null` left
/// its words where a lambda with nothing captured reads what it captured, and from then on every
/// handler a mixin injects ran on a number. Java throws at such a call; the runtime answers it,
/// so what it wrote is dropped as the call ends (told of when tracing, by the call that wrote).
fn keep_null(rt: &mut Rt, key: &str) {
    let Some(null) = rt.heap.first_mut() else {
        return;
    };
    if matches!(&null.data, Data::Array(a) if a.is_empty()) && null.class == "null" {
        return;
    }
    null.class = "null";
    null.cid = 0;
    null.data = Data::Array(Vec::new());
    thread_local!(static TOLD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
    if Rt::tracing() && TOLD.with(|t| t.replace(t.get() + 1)) < 40 {
        eprintln!("horizon-compat: {key} wrote to null; dropped");
    }
}

/// Whether API key `key` is a constructor's (`<class>.<init>.<descriptor>`): asked of every
/// call, so by the one character no other key has.
fn is_constructor(key: &str) -> bool {
    // (It returns nothing: only such a key is read through for the name.)
    key.ends_with(")V")
        && key
            .find('<')
            .is_some_and(|at| key[at..].starts_with("<init>.("))
}

thread_local! {
    /// How many calls are being made on another's behalf, to learn whether anything answers
    /// them ([`call_nested`]'s field through its getter): their misses are theirs alone.
    static PROBING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
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
    // A call on a proxy: its handler's.
    if let Some(v) = crate::jlang::proxy_call(rt, owner, rest, args) {
        uncount(rt, key);
        return v;
    }
    // A mixin's accessor onto a field of the game's object (`$player` of `Minecraft`: the field
    // read through an `@Accessor` interface) that no module answers for itself: what the object
    // holds under the field's name, as a plain read of the field gets it. Unanswered where it
    // holds nothing: the miss is told as before.
    if let Some(field) = rest
        .strip_prefix('$')
        .and_then(|r| r.split_once(".()"))
        .map(|(f, _)| f)
        && let Some(&this) = args.first()
    {
        let field = rt.static_key(field);
        let held = rt.get_field(this, field);
        if held != V::Void && this.r() != 0 {
            uncount(rt, key);
            return held;
        }
    }
    // A field of the game's object read as the field it is, which nothing keeps under its own
    // name: what the object's method of the field's name answers, where it has one that
    // returns the field's type (`level` and `level()` of an entity, `onGround` and
    // `onGround()`), or its getter (`server` and `getServer()`; `is...` for a flag).
    if let Some((field, returned)) = rest.strip_prefix('$').and_then(|r| r.split_once(".()"))
        && args.first().is_some_and(|v| v.r() != 0)
        && !field.is_empty()
    {
        let mut titled = field.to_owned();
        titled[..1].make_ascii_uppercase();
        let getters = [
            field.to_owned(),
            format!("get{titled}"),
            if returned == "Z" {
                format!("is{titled}")
            } else {
                String::new()
            },
        ];
        for getter in getters.iter().filter(|g| !g.is_empty()) {
            let asked = rt.static_key(&format!("{owner}.{getter}.(){returned}"));
            let seen = rt.unmapped.get(asked).copied().unwrap_or(0);
            // (Asked on the field's behalf: a miss of it is not told of, nor counted.)
            PROBING.with(|p| p.set(p.get() + 1));
            let r2 = call_nested(rt, asked, args);
            PROBING.with(|p| p.set(p.get() - 1));
            if rt.unmapped.get(asked).copied().unwrap_or(0) == seen {
                uncount(rt, key);
                return r2;
            }
            uncount(rt, asked);
        }
    }
    // An enum of the game's no table here lists, by the constants the translator carried for it:
    // before a class above is tried, or its `valueOf(name)` would be asked of the name's own class.
    if let Some(v) = crate::api_common::carried_enum_call(rt, owner, rest, args) {
        uncount(rt, key);
        return v;
    }
    for retry in retries(rt, key, owner, rest, args).iter() {
        if retry.dead.get() {
            continue;
        }
        let up = retry.key;
        let seen = rt.unmapped.get(up).copied().unwrap_or(0);
        let r2 = call_exact(rt, up, args);
        if rt.unmapped.get(up).copied().unwrap_or(0) == seen {
            // Resolved: the miss on the name the class file used doesn't count.
            uncount(rt, key);
            return r2;
        }
        uncount(rt, up);
        if unclaimed(up) {
            retry.dead.set(true);
        }
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
    // `chunkPosition()` of whatever has a block position (an entity of any kind, a player): the
    // chunk that position is in, as the game keeps the two together.
    if rest == "chunkPosition.()Lnet/minecraft/world/level/ChunkPos;"
        && args.first().is_some_and(|v| v.r() != 0)
    {
        let at = rt.static_key(&format!(
            "{owner}.blockPosition.()Lnet/minecraft/core/BlockPos;"
        ));
        let seen = rt.unmapped.get(at).copied().unwrap_or(0);
        let pos = call(rt, at, args);
        if rt.unmapped.get(at).copied().unwrap_or(0) == seen && pos.r() != 0 {
            uncount(rt, key);
            let p = pos_of(rt, pos);
            return crate::api_common::chunk_pos(rt, p[0] >> 4, p[2] >> 4);
        }
        // (No position either: that miss is not the mod's.)
        if rt.unmapped.get(at).copied().unwrap_or(0) != seen {
            uncount(rt, at);
        }
    }
    // The game's own code for it, which the translator carried where it asks only what is mapped.
    if let Some(f) = derived(rt, key, owner, rest, args) {
        uncount(rt, key);
        // (From the runtime's own code into code as the mods' is: an entry.)
        return rt.entered(|rt| f(rt, args), |r| *r);
    }
    if PROBING.with(|p| p.get()) > 0 {
        return r;
    }
    trace_unmapped(rt, key, args);
    if runaway(rt, key) {
        let secs = rt
            .hook_first_miss
            .map_or(RUNAWAY_TIME.as_secs(), |t| t.elapsed().as_secs());
        // (A hook's own count; while the mods load there is none, and the loop's was the limit.)
        let n = if rt.hook_misses > 0 {
            rt.hook_misses
        } else {
            RUNAWAY_MISSES
        };
        // What a JVM throws at a method that is not there, which is what this is: code that
        // falls back on such an error does, and code that sorts errors by kind knows this one.
        crate::jlang::throw_with(
            rt,
            "java/lang/NoSuchMethodError",
            &format!(
                "horizon-compat: {key} isn't translated, and this hook kept calling what isn't ({n} calls in {secs} s): a loop waiting on its stand-in answer"
            ),
        );
        return r;
    }
    chained(rt, key, args).unwrap_or(r)
}

/// The game's own method for API key `key` (`owner.rest`) among the linked code
/// ([`crate::linked::Linked::derived`]): the one the receiver's class has (`args[0]`'s, or the
/// first class above it the code carries), found once per key, class and thread.
fn derived(
    rt: &Rt,
    key: &'static str,
    owner: &str,
    rest: &str,
    args: &[V],
) -> Option<crate::linked::Method> {
    type Found =
        std::collections::HashMap<(usize, usize, usize, usize), Option<crate::linked::Method>>;
    thread_local! {
        static DERIVED: std::cell::RefCell<Found> = std::cell::RefCell::new(Found::new());
    }
    let linked = rt.linked?;
    let class = match args.first() {
        Some(&v @ V::R(n)) if n != 0 => rt.class_of(v),
        _ => "",
    };
    let id = (
        std::ptr::from_ref(linked) as usize,
        key.as_ptr() as usize,
        key.len(),
        class.as_ptr() as usize,
    );
    if let Some(f) = DERIVED.with(|d| d.borrow().get(&id).copied()) {
        return f;
    }
    // The receiver's class as the code has it: itself, or the first above it that is carried.
    let mut receiver = (!class.is_empty()).then_some(class);
    let mut steps = 0;
    while let Some(c) = receiver.filter(|c| !linked.has_derived(c)) {
        steps += 1;
        receiver = rt
            .supers
            .get(c)
            .copied()
            .or_else(|| api_super(c))
            .filter(|_| steps < 64);
    }
    let f = linked.derived(receiver, owner, rest);
    DERIVED.with(|d| d.borrow_mut().insert(id, f));
    f
}

/// Calls nothing maps, in one hook, past which the hook is taken to be stuck ([`runaway`]).
const RUNAWAY_MISSES: u64 = 1_000_000;
/// How long a hook goes on making such calls before it is taken to be stuck.
const RUNAWAY_TIME: std::time::Duration = std::time::Duration::from_secs(10);
/// Far fewer such calls are enough when the loop has allocated this much meanwhile
/// ([`RUNAWAY_BYTES`]): a loop that grows a string on each round (Sound Physics' `escape`, which
/// asked `Character.UnicodeBlock.of` of each character and escaped every one, the escapes too)
/// takes all the machine's memory in seconds, long before a million rounds.
const RUNAWAY_FEW: u64 = 1_000;
/// (One text or array of this size, made meanwhile: what the loop allocated in all says
/// nothing now that garbage is collected while a hook runs, and a listener that reads a
/// thousand files allocates more than a gigabyte in passing.)
const RUNAWAY_BYTES: u64 = 256 << 20;
/// Or made this many objects: a loop that makes one on each round (a server thread's accept loop,
/// run on the game's thread) holds them all until its hook ends, which it never does.
const RUNAWAY_OBJECTS: usize = 8_000_000;
/// And this many are enough in a hook that makes a call a stuck hook was stopped on before
/// ([`STOPPED_ON`]): the same loop, started again. (A map mod's write of its chunks, begun anew
/// at every tick: each took [`RUNAWAY_TIME`] to be stopped, and the game stood at ten seconds a
/// tick for as long as it ran.)
const RUNAWAY_AGAIN: u64 = 10_000;

thread_local! {
    /// What the runtime had allocated at the running hook's first unmapped call: bytes of text
    /// and arrays, and objects.
    static HOOK_ALLOCATED: std::cell::Cell<(u64, usize)> = const { std::cell::Cell::new((0, 0)) };
    /// The calls nothing maps that this thread's hooks were making when they were stopped as
    /// stuck ([`RUNAWAY_AGAIN`]).
    static STOPPED_ON: std::cell::RefCell<std::collections::HashSet<&'static str>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Whether the hook running now (one call of the game's into the mod: a tick, a screen's frame)
/// is stuck in a loop on what nothing maps: an unmapped call answers a stand-in (0, null), and a
/// loop that waits for another answer (`while (it.next() != BreakIterator.DONE)`) never ends. Past
/// [`RUNAWAY_MISSES`] such calls over [`RUNAWAY_TIME`], each throws `java.lang.Error`, which unwinds
/// the mod's code to the hook's end, as a `catch (Exception e)` doesn't stop it: the game goes on
/// without what the hook would have done, and the error names the call. While the mods load there
/// is no hook: a loop is known there by the few keys it goes round ([`Run`]), and the error ends
/// the mod's constructor or listener it is in, so the mods after it load.
fn runaway(rt: &mut Rt, key: &'static str) -> bool {
    if rt.host().is_none() {
        let stuck = LOADING.with(|run| run.borrow_mut().stuck(key, (rt.largest, rt.heap.len())));
        if stuck {
            tell_most_asked(rt);
        }
        return stuck;
    }
    rt.hook_misses += 1;
    if rt.hook_misses == 1 {
        rt.largest = 0;
        HOOK_ALLOCATED.set((0, rt.heap.len()));
    }
    let first = *rt.hook_first_miss.get_or_insert_with(std::time::Instant::now);
    let (_, objects) = HOOK_ALLOCATED.get();
    let grown =
        rt.largest >= RUNAWAY_BYTES || rt.heap.len().saturating_sub(objects) >= RUNAWAY_OBJECTS;
    let again =
        rt.hook_misses >= RUNAWAY_AGAIN && STOPPED_ON.with(|keys| keys.borrow().contains(key));
    let stuck = again
        || (rt.hook_misses >= RUNAWAY_MISSES && first.elapsed() >= RUNAWAY_TIME)
        || (rt.hook_misses >= RUNAWAY_FEW && grown);
    if stuck {
        STOPPED_ON.with(|keys| keys.borrow_mut().insert(key));
        tell_most_asked(rt);
    }
    stuck
}

/// When tracing: the calls nothing maps that were made most, where a loop on one was just
/// stopped: the trace names each such call once, at its first, and the one a loop went
/// round a million times may be far up the log.
fn tell_most_asked(rt: &Rt) {
    thread_local!(static TOLD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
    if !Rt::tracing() || TOLD.with(|t| t.replace(t.get() + 1)) >= 20 {
        return;
    }
    let mut most: Vec<(&&'static str, &u64)> = rt.unmapped.iter().collect();
    most.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    let text: Vec<String> = most
        .iter()
        .take(5)
        .map(|(key, n)| format!("{n} x {key}"))
        .collect();
    eprintln!(
        "horizon-compat: stopped a loop on calls nothing maps; asked most so far: {}",
        text.join("; ")
    );
}

/// The unmapped calls of a loop while the mods load, where no hook bounds them: the few keys the
/// loop goes round, how many calls of them in a row, and since when. A mod's constructor that
/// drained a queue nothing mapped (`while (!queue.isEmpty()) queue.poll().build()`) never ended,
/// and no mod after it loaded.
#[derive(Default)]
struct Run {
    keys: Vec<&'static str>,
    calls: u64,
    since: Option<std::time::Instant>,
    /// What the runtime had allocated when the count began: bytes of text and arrays, and
    /// objects.
    allocated: Option<(u64, usize)>,
}

/// How many unmapped keys a loop may go round and still be told from loading going on: KubeJS's
/// web server, started on the game's thread, went round more than a dozen.
const RUN_KEYS: usize = 64;

impl Run {
    /// Counts an unmapped call of `key`; whether its loop is stuck ([`RUNAWAY_MISSES`] calls of the
    /// same few keys over [`RUNAWAY_TIME`]). A key past [`RUN_KEYS`] starts the count again:
    /// loading that goes on meets new ones.
    fn stuck(&mut self, key: &'static str, allocated: (u64, usize)) -> bool {
        if !self.keys.contains(&key) {
            if self.keys.len() >= RUN_KEYS {
                *self = Run::default();
            }
            self.keys.push(key);
        }
        self.calls += 1;
        let since = *self.since.get_or_insert_with(std::time::Instant::now);
        let (_, objects) = *self.allocated.get_or_insert(allocated);
        // (The largest single text or array so far, and how far the heap grew.)
        let grown =
            allocated.0 >= RUNAWAY_BYTES || allocated.1.saturating_sub(objects) >= RUNAWAY_OBJECTS;
        let stuck = (self.calls >= RUNAWAY_MISSES && since.elapsed() >= RUNAWAY_TIME)
            || (self.calls >= RUNAWAY_FEW && grown);
        if stuck {
            *self = Run::default();
        }
        stuck
    }
}

thread_local! {
    static LOADING: std::cell::RefCell<Run> = std::cell::RefCell::new(Run::default());
}

/// A new piece of the mods' code begins where no hook bounds it (a listener of an event while
/// the mods load, the game's question about one block): the unmapped calls of the piece before
/// are not its loop. The game asks how each of a pack's block states draws, one after another,
/// and a method nothing mapped that a thousand of them call was taken for one loop: every block
/// after the millionth call was answered an error.
pub(crate) fn new_piece() {
    LOADING.with(|run| *run.borrow_mut() = Run::default());
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
    if this.r() != 0 && rt.class_of(this) == owner {
        return Some(this);
    }
    // An object of the class with nothing of it: what is asked of it next goes unanswered
    // too, and the trace then names this call ([`trace_unmapped`]).
    let made = rt.new_object(owner);
    if Rt::tracing() {
        let origin = rt.string(key);
        rt.put_field(made, STAND_IN, origin);
    }
    Some(made)
}

/// Where a stand-in object keeps the key of the unmapped call that made it ([`chained`]).
const STAND_IN: &str = "$standIn";

/// The class above the game's or the loader's class `class` ("" for none) and the interfaces it
/// declares, as their class files have them ([`crate::game_classes`]); None for a class
/// that is not one of theirs.
pub(crate) fn game_parents(class: &str) -> Option<(&'static str, &'static [&'static str])> {
    let all = &crate::game_classes::CLASSES;
    all.binary_search_by(|e| e.0.cmp(class))
        .ok()
        .map(|i| (all[i].1, all[i].2))
}

/// Whether one of the game's or the loader's classes names `class` as an interface of its own:
/// what the table of their classes says of a class being an interface (one nothing implements
/// reads as a class). Found once, by looking through all of them.
pub(crate) fn game_interface(class: &str) -> bool {
    static NAMED: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    NAMED
        .get_or_init(|| {
            crate::game_classes::CLASSES
                .iter()
                .flat_map(|c| c.2.iter().copied())
                .collect()
        })
        .contains(class)
}

/// The hash `tools/vanilla_classes.py` files a declared method under: FNV-1a (32 bits) of its
/// `name.descriptor`, which is what follows the class in a key.
fn method_hash(rest: &str) -> u32 {
    rest.bytes().fold(0x811C_9DC5, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Whether the game's or the loader's class `class` declares the method of hash `hash` itself
/// ([`crate::game_classes::DECLARED`]); None for a class that is not one of theirs.
fn declares(class: &str, hash: u32) -> Option<bool> {
    let i = crate::game_classes::CLASSES
        .binary_search_by(|e| e.0.cmp(class))
        .ok()?;
    Some(
        crate::game_classes::DECLARED[i]
            .binary_search(&hash)
            .is_ok(),
    )
}

/// Whether the game's or the loader's class `class`, or a class above it, declares the method
/// `rest` (`name.descriptor`) itself, by their class files ([`crate::game_classes`]). What a
/// class has of its own no interface's default method replaces: the translator asks before it
/// gives a class of the game's the default methods of an interface a mixin adds to it. False
/// for a class that is not one of theirs.
pub fn game_class_declares(class: &str, rest: &str) -> bool {
    let hash = method_hash(rest);
    let mut c = class;
    loop {
        match declares(c, hash) {
            Some(true) => return true,
            Some(false) => {}
            None => return false,
        }
        match game_parents(c) {
            Some((above, _)) if !above.is_empty() => c = above,
            _ => return false,
        }
    }
}

/// The class whose method a call of the method of hash `hash` through the game's class `class`
/// runs, as the JVM resolves it, where that is not `class` itself: the nearest class above it
/// that declares the method, its superclasses before the interfaces. None where `class`
/// declares it (nothing above is the method called then), where nothing above does, and for a
/// class that is not the game's.
fn resolved_above(class: &'static str, hash: u32) -> Option<&'static str> {
    if declares(class, hash)? {
        return None;
    }
    let mut chain = vec![class];
    let mut c = class;
    while let Some((above, _)) = game_parents(c).filter(|(above, _)| !above.is_empty()) {
        if declares(above, hash) == Some(true) {
            return Some(above);
        }
        chain.push(above);
        c = above;
    }
    let mut next = 0;
    while next < chain.len() {
        let c = chain[next];
        next += 1;
        for &i in game_parents(c)
            .map(|(_, interfaces)| interfaces)
            .unwrap_or_default()
        {
            if declares(i, hash) == Some(true) {
                return Some(i);
            }
            if !chain.contains(&i) {
                chain.push(i);
            }
        }
    }
    None
}

/// Where a missed call of `owner.rest` is asked next. First up from the named class, a
/// translated subclass to its API superclass. Then from the receiver's own class up, the way the
/// JVM dispatches an interface call: `Supplier.get` on a DeferredHolder answers as
/// `DeferredHolder.get`, and `CharSequence.length` on a String as `String.length`. Then the
/// interfaces those classes implement. And past those, the class whose method the call is by
/// the game's own class files ([`resolved_above`]), from the receiver's class and from the
/// named one: `player.getBlockY()` names `Player`, or the client's `LocalPlayer`, and the
/// method is `Entity`'s; `level.isEmptyBlock(pos)` names `ServerLevel`, and it is
/// `LevelReader`'s. A class that declares the method itself is asked nowhere above: `super.tick`
/// to a falling block is that block's own tick, not the empty one of the classes above it. Nor
/// is a constructor, which no class takes from another. A class is asked once.
fn retry_classes(rt: &Rt, owner: &'static str, rest: &str, args: &[V]) -> Vec<&'static str> {
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
    let mut receiver = None;
    if let Some(&v @ V::R(n)) = args.first()
        && n != 0
    {
        let class = rt.class_of(v);
        receiver = Some(class);
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
    // (`HORIZON_COMPAT_CLASS_FILES=off` leaves the class files out, as before they were read.)
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if rest.starts_with("<init>.")
        || *OFF.get_or_init(|| {
            std::env::var_os("HORIZON_COMPAT_CLASS_FILES").is_some_and(|v| v == "off")
        })
    {
        return out;
    }
    let hash = method_hash(rest);
    for class in receiver.into_iter().chain([owner]) {
        if let Some(found) = resolved_above(class, hash)
            && !classes.contains(&found)
        {
            classes.push(found);
            out.push(found);
        }
    }
    out
}

/// One more key a missed call is asked under ([`retries`]): the same method of another class.
struct Retry {
    key: &'static str,
    /// Nothing answers the key for a receiver of this kind ([`unclaimed`]): it is not asked
    /// again.
    dead: std::cell::Cell<bool>,
}

thread_local! {
    /// The keys a missed call was asked under, by its own key and what its receiver is: its
    /// class, whether it is an enum's constant, and how many classes the runtime knows of
    /// (a class that came since may be one more to ask).
    static RETRIES: std::cell::RefCell<crate::HashMap<(usize, usize, usize, usize, bool, usize), std::rc::Rc<[Retry]>>> =
        std::cell::RefCell::new(crate::HashMap::default());
}

/// The keys the missed call `key` (`owner.rest`) is asked again under ([`retry_classes`]),
/// found once for the call's key and its receiver's class: a call nothing maps is made again
/// and again, and the classes above a class of the game's are many.
fn retries(
    rt: &mut Rt,
    key: &'static str,
    owner: &'static str,
    rest: &str,
    args: &[V],
) -> std::rc::Rc<[Retry]> {
    let (class, constant) = match args.first() {
        Some(&v @ V::R(n)) if n != 0 => (
            rt.class_of(v),
            !matches!(rt.get_field(v, "$ordinal"), V::Void),
        ),
        _ => ("", false),
    };
    let known = rt
        .supers
        .len()
        .wrapping_mul(31)
        .wrapping_add(rt.interfaces.len());
    let id = (
        key.as_ptr() as usize,
        key.len(),
        class.as_ptr() as usize,
        class.len(),
        constant,
        known,
    );
    if let Some(found) = RETRIES.with(|r| r.borrow().get(&id).cloned()) {
        return found;
    }
    let found: std::rc::Rc<[Retry]> = retry_classes(rt, owner, rest, args)
        .into_iter()
        .map(|class| Retry {
            key: rt.static_key(&format!("{class}.{rest}")),
            dead: std::cell::Cell::new(false),
        })
        .collect();
    RETRIES.with(|r| r.borrow_mut().insert(id, found.clone()));
    found
}

/// Whether nothing answers `key` but by what its receiver is: no arm of [`call_exact`]'s own
/// took it, it has no intrinsic and no module claims it ([`Route`], known once it was called).
/// What is left to answer it reads the key and the receiver's class alone (a function object,
/// an annotation, a mod's class under a constructor of the game's), so a miss for one receiver
/// of a class is a miss for every one.
fn unclaimed(key: &'static str) -> bool {
    ROUTES.with(|r| {
        r.borrow()
            .get(&(key.as_ptr() as usize, key.len()))
            .is_some_and(|route| route.intrinsic.is_none() && route.claimed == 0)
    })
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

const THROWABLES: &str = "[Ljava/lang/Throwable;";

/// What throwable `e` suppressed (`addSuppressed`), in order.
fn suppressed(rt: &Rt, e: V) -> Vec<V> {
    match rt.get_field(e, "$suppressed") {
        v @ V::R(r) if r != 0 => crate::jutil::elements(rt, v),
        _ => Vec::new(),
    }
}

/// `printStackTrace`'s text, without frames: `e` after `caption`, what it suppressed (indented,
/// `Suppressed: `), then its cause (`Caused by: `), as the JDK prints them; one seen before is a
/// circular reference.
fn stack_trace(rt: &Rt, e: V, caption: &str, indent: &str, out: &mut String, seen: &mut Vec<V>) {
    if seen.contains(&e) {
        out.push_str(&format!(
            "{indent}{caption}[CIRCULAR REFERENCE: {}]\n",
            throwable_string(rt, e)
        ));
        return;
    }
    seen.push(e);
    out.push_str(&format!("{indent}{caption}{}\n", throwable_string(rt, e)));
    for s in suppressed(rt, e) {
        stack_trace(rt, s, "Suppressed: ", &format!("{indent}\t"), out, seen);
    }
    let cause = rt.get_field(e, "cause");
    if cause.r() != 0 && cause != e {
        stack_trace(rt, cause, "Caused by: ", indent, out, seen);
    }
}

/// `Throwable`'s own methods, as `name.descriptor`: what every throwable answers alike.
const THROWABLE_METHODS: &[&str] = &[
    "<init>.()V",
    "<init>.(Ljava/lang/String;)V",
    "<init>.(Ljava/lang/String;Ljava/lang/Throwable;)V",
    "<init>.(Ljava/lang/Throwable;)V",
    "getMessage.()Ljava/lang/String;",
    "getLocalizedMessage.()Ljava/lang/String;",
    "getCause.()Ljava/lang/Throwable;",
    "initCause.(Ljava/lang/Throwable;)Ljava/lang/Throwable;",
    "toString.()Ljava/lang/String;",
    "addSuppressed.(Ljava/lang/Throwable;)V",
    "getSuppressed.()[Ljava/lang/Throwable;",
    "printStackTrace.()V",
    "printStackTrace.(Ljava/io/PrintStream;)V",
    "printStackTrace.(Ljava/io/PrintWriter;)V",
    "fillInStackTrace.()Ljava/lang/Throwable;",
    "getStackTrace.()[Ljava/lang/StackTraceElement;",
    "setStackTrace.([Ljava/lang/StackTraceElement;)V",
];

/// A call of `Throwable`'s own on a throwable of a library's or the game's that no module models
/// (Gson's `JsonParseException`, Netty's `DecoderException`, Commons' `NotImplementedException`,
/// `javax`'s, the game's `ResourceLocationException`): the same call on `Throwable`, which sets
/// and reads the message and the cause on the object, of its own class. None for a mod's class,
/// which climbs its own superclasses ([`retry_classes`]).
///
/// Known by its name, as the JVM's classes name theirs. In the game's packages only a name ending
/// in `Exception` says so: 1.21.1's Realms `RealmsError` records and DFU's `DataResult.Error` are
/// no throwables, and its one throwable named so (`OldUsersConverter.ConversionError`) is never a
/// mod's.
fn foreign_throwable(rt: &mut Rt, key: &str) -> Option<&'static str> {
    let (owner, rest) = key.split_once('.')?;
    if owner.starts_with("java/")
        || rt.supers.contains_key(owner)
        || !THROWABLE_METHODS.contains(&rest)
    {
        return None;
    }
    let simple = owner.rsplit(['/', '$']).next().unwrap_or(owner);
    let game = ["net/minecraft/", "net/neoforged/", "com/mojang/"]
        .iter()
        .any(|p| owner.starts_with(p));
    let named =
        simple.ends_with("Exception") || (!game && simple.ends_with("Error") && simple != "Error");
    named.then(|| rt.static_key(&format!("java/lang/Throwable.{rest}")))
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
            // A stand-in's calls go unanswered because of the call that made it, which is the
            // one to map: named with it.
            Some(&v @ V::R(_)) => match rt.str_of(rt.get_field(v, STAND_IN)) {
                Some(origin) => format!(" (on {}, the stand-in {origin} gave)", rt.class_of(v)),
                None => format!(" (on {})", rt.class_of(v)),
            },
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

/// Where the calls of a key none of [`call_exact`]'s own arms takes go: the API's intrinsic for
/// it, if it has one, then the modules of [`CHAIN`] that claim it (as bits). Those arms and their
/// guards read the key alone, and so do the claims, so it is found once per key.
#[derive(Clone, Copy)]
struct Route {
    intrinsic: Option<(Intrinsic, bool)>,
    claimed: u128,
}

thread_local! {
    /// The routes found so far, by key (its address and length): one lookup per call.
    static ROUTES: std::cell::RefCell<crate::HashMap<(usize, usize), Route>> = std::cell::RefCell::new(crate::HashMap::default());
}

fn call_exact(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let id = (key.as_ptr() as usize, key.len());
    if let Some(route) = ROUTES.with(|r| r.borrow().get(&id).copied()) {
        return routed(rt, key, args, route);
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
        // A character of a string, where a parser asks for every one of them in turn: read
        // where it lies (its byte, while the text up to it is ASCII), not out of a copy of
        // the whole string made for the one call.
        "java/lang/String.charAt.(I)C" | "java/lang/CharSequence.charAt.(I)C"
            if rt.str_of(a(0)).is_some() =>
        {
            let (s, at) = (rt.str_of(a(0)).unwrap_or(""), a(1).i());
            match crate::jlang::char_at(s, at) {
                Some(c) => V::I(i32::from(c)),
                None => {
                    let length = s.encode_utf16().count();
                    crate::jlang::throw_char_index(rt, at, length)
                }
            }
        }
        "java/lang/String.isEmpty.()Z" => V::I(rt.str_of(a(0)).is_some_and(str::is_empty) as i32),
        "java/lang/CharSequence.length.()I" if rt.str_of(a(0)).is_some() => V::I(
            rt.str_of(a(0))
                .map_or(0, |s| s.encode_utf16().count() as i32),
        ),
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
        // A walk of the stack sees no frame: none is kept here to show. What it is asked of
        // every frame holds (Lucene's `ensureCaller`, which lets only its own classes call:
        // unanswered, the mods that carry it stopped in their constructors), and what it would
        // take from one is not there (Chisels & Bits' calling method's name: "unknown").
        "java/lang/StackWalker.walk.(Ljava/util/function/Function;)Ljava/lang/Object;" => {
            let frames = rt.alloc("java/util/stream/Stream", Data::Stream(Vec::new()));
            rt.call_functional(
                "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;",
                a(1),
                &[frames],
            )
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
        "java/lang/Class.getCanonicalName.()Ljava/lang/String;" => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            match crate::jlang::canonical_name(&n) {
                Some(c) => rt.string(&c),
                None => V::NULL,
            }
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
        k if exception_class(k) && (k.contains(".getCause.") || k.contains(".getTargetException.")) => match rt.get_field(a(0), "cause") {
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
        // `addSuppressed(e)`, kept for `getSuppressed()` (try-with-resources adds the failure of
        // closing a resource to the one it rethrows): the throwable itself or null throws, as the
        // JDK's does.
        k if exception_class(k) && k.contains(".addSuppressed.") => {
            let (this, e) = (a(0), a(1));
            if e == this {
                let iae = rt.new_object("java/lang/IllegalArgumentException");
                let m = rt.string("Self-suppression not permitted");
                rt.put_field(iae, "message", m);
                rt.put_field(iae, "cause", this);
                rt.thrown = Some(iae);
            } else if e.r() == 0 {
                crate::jlang::throw_with(
                    rt,
                    "java/lang/NullPointerException",
                    "Cannot suppress a null exception.",
                );
            } else {
                let mut all = suppressed(rt, this);
                all.push(e);
                let arr = rt.alloc(THROWABLES, Data::Array(all));
                rt.put_field(this, "$suppressed", arr);
            }
            V::Void
        }
        k if exception_class(k) && k.contains(".getSuppressed.") => {
            let all = suppressed(rt, a(0));
            rt.alloc(THROWABLES, Data::Array(all))
        }
        // No frames are kept: the trace is the throwable, what it suppressed and its causes,
        // printed to `System.err` (or the `System.out` it is given).
        k if exception_class(k) && k.contains(".printStackTrace.") => {
            let mut trace = String::new();
            stack_trace(rt, a(0), "", "", &mut trace, &mut Vec::new());
            // Into the stream or the writer it is given, where that is one of the mod's own
            // making (JourneyMap writes a trace into a string so, for its log).
            if k.contains("(Ljava/io/PrintWriter;)") && a(1).r() != 0 {
                let text = rt.string(&trace);
                call(
                    rt,
                    "java/io/PrintWriter.write.(Ljava/lang/String;)V",
                    &[a(1), text],
                );
            } else if k.contains("(Ljava/io/PrintStream;)") && rt.get_field(a(1), "$out").r() != 0 {
                let text = rt.string(&trace);
                call(
                    rt,
                    "java/io/PrintStream.print.(Ljava/lang/String;)V",
                    &[a(1), text],
                );
            } else {
                let out =
                    k.contains("(Ljava/io/PrintStream;)") && rt.get_field(a(1), "$err") == V::I(0);
                crate::jlang::print_std(!out, &trace);
            }
            V::Void
        }
        k if exception_class(k) && k.contains(".fillInStackTrace.") => a(0),
        k if exception_class(k) && k.contains(".getStackTrace.") => crate::jlang::stack_frames(rt),
        k if exception_class(k) && k.contains(".setStackTrace.") => V::Void,
        // `array.clone()` (an enum's `values()`): a shallow copy.
        k if k.starts_with('[') && k.contains(".clone.") => {
            let this = a(0);
            match rt.heap.get(this.r() as usize).map(|o| (o.class, o.data.clone())) {
                Some((class, d @ Data::Array(_))) => rt.alloc(class, d),
                _ => V::NULL,
            }
        }
        _ => {
            let route = Route { intrinsic: intrinsic(key), claimed: claimed(key) };
            ROUTES.with(|r| r.borrow_mut().insert(id, route));
            routed(rt, key, args, route)
        }
    }
}

/// A call no arm of [`call_exact`] takes: its intrinsic's answer, else the modules that claim its
/// key, then what is left.
fn routed(rt: &mut Rt, key: &'static str, args: &[V], route: Route) -> V {
    if let Some((f, reads)) = route.intrinsic
        && let Some(v) = f(rt, args)
    {
        if reads && checking() {
            check_intrinsic(rt, key, args, route.claimed, v);
        }
        return v;
    }
    if let Some(v) = chain(rt, key, args, route.claimed) {
        return v;
    }
    if plain_super_init(rt, key, args) {
        return V::Void;
    }
    if let Some(general) = foreign_throwable(rt, key) {
        return call_exact(rt, general, args);
    }
    match crate::functions::invoke_through(rt, key, args)
        .or_else(|| crate::annotations::element(rt, key, args))
    {
        Some(v) => v,
        None => {
            crate::miss(rt, key);
            V::Void
        }
    }
}

/// An intrinsic: the API's answer to one key's call, given at once, as a JVM's JIT answers its
/// intrinsics, instead of offering the call to the modules in turn; None leaves it to them.
type Intrinsic = fn(&mut Rt, &[V]) -> Option<V>;

const BE_GET_LEVEL: &str = "net/minecraft/world/level/block/entity/BlockEntity.getLevel.()Lnet/minecraft/world/level/Level;";
const BE_HAS_LEVEL: &str = "net/minecraft/world/level/block/entity/BlockEntity.hasLevel.()Z";
const BE_GET_POS: &str = "net/minecraft/world/level/block/entity/BlockEntity.getBlockPos.()Lnet/minecraft/core/BlockPos;";
const BE_GET_STATE: &str = "net/minecraft/world/level/block/entity/BlockEntity.getBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
const MAP_VALUES: &str = "java/util/Map.values.()Ljava/util/Collection;";
const COLLECTION_FOR_EACH: &str = "java/util/Collection.forEach.(Ljava/util/function/Consumer;)V";
const ITERATOR_HAS_NEXT: &str = "java/util/Iterator.hasNext.()Z";
const ITERATOR_NEXT: &str = "java/util/Iterator.next.()Ljava/lang/Object;";
const ENTRY_GET_KEY: &str = "java/util/Map$Entry.getKey.()Ljava/lang/Object;";
const ENTRY_GET_VALUE: &str = "java/util/Map$Entry.getValue.()Ljava/lang/Object;";

/// The API's intrinsics, and whether each only reads (then tests check its every answer against
/// the chain's): the calls mods make most, in their block entities' ticks (Create's kinetic
/// block entity reads its level twice and walks its behaviours, a map's values, at every tick),
/// where offering each to the modules in turn, through their long matches, took a quarter of the
/// tick. Each answers what the module chain answers, with the same arm's code, for the receivers
/// that arm takes, and leaves every other receiver to the chain (a mod's own map class, a heap);
/// no module claims its key before the one whose arm it is (`intrinsics_come_first`).
fn intrinsic(key: &str) -> Option<(Intrinsic, bool)> {
    fn this(args: &[V]) -> V {
        args.first().copied().unwrap_or_default()
    }
    Some(match key {
        // `api_common`'s block entity arms, which `world` passes these to.
        BE_GET_LEVEL => (|rt, args| Some(rt.get_field(this(args), "level")), true),
        BE_HAS_LEVEL => (|rt, args| Some(V::I((rt.get_field(this(args), "level").r() != 0) as i32)), true),
        BE_GET_POS => (|rt, args| Some(rt.get_field(this(args), "$pos")), true),
        BE_GET_STATE => (|rt, args| Some(rt.get_field(this(args), "$state")), true),
        MAP_VALUES => (crate::jutil::plain_map_values, false),
        COLLECTION_FOR_EACH => (crate::jutil::plain_list_for_each, false),
        // A walk over a collection: every loop of the mods' code makes these for each element.
        ITERATOR_HAS_NEXT => (crate::jutil::iterator_has_next, true),
        ITERATOR_NEXT => (crate::jutil::iterator_next, false),
        ENTRY_GET_KEY => (|rt, args| crate::jutil::entry_part(rt, args, false), true),
        ENTRY_GET_VALUE => (|rt, args| crate::jutil::entry_part(rt, args, true), true),
        _ => return None,
    })
}

/// Tests (and `HORIZON_COMPAT_CHECK_DISPATCH=1`): a reading intrinsic's answer `v` is the one the
/// module chain gives.
fn check_intrinsic(rt: &mut Rt, key: &'static str, args: &[V], claimed: u128, v: V) {
    let chained = chain(rt, key, args, claimed);
    if chained != Some(v) {
        let message = format!("horizon-compat: the intrinsic for {key} answered {v:?}, the modules {chained:?}");
        if cfg!(test) {
            panic!("{message}");
        }
        thread_local! {
            static TOLD: std::cell::RefCell<crate::HashMap<(usize, usize), ()>> = std::cell::RefCell::new(crate::HashMap::default());
        }
        if TOLD.with(|t| t.borrow_mut().insert((key.as_ptr() as usize, key.len()), ()).is_none()) {
            eprintln!("{message}");
        }
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
const CHAIN: [Module; 99] = [
    crate::diodes::call,
    crate::rails::call,
    crate::fake_players::call,
    crate::attribute_suppliers::call,
    crate::spawning::call,
    crate::block_breaking::call,
    crate::loot_params::call,
    mutable_pos,
    crate::worldgen::call,
    crate::mixin::call,
    crate::attachments::call,
    crate::sounds::call,
    crate::particles::call,
    crate::particle_engine::call,
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
    crate::goals::call,
    crate::pathing::call,
    crate::mobs::call,
    crate::mod_entities::call_key,
    crate::entities::call_key,
    crate::creative::call,
    crate::tooltips::call,
    crate::damage::call,
    crate::player_advancements::call,
    crate::player_stats::call,
    crate::world_data::call,
    crate::world_events::call,
    crate::made_entities::call,
    crate::vanilla_entities::call,
    crate::entity_saves::call,
    crate::commands::call,
    crate::brigadier::call,
    crate::functions::call,
    crate::gamerules::call,
    crate::value_providers::call,
    crate::weighted::call,
    crate::nightconfig::call,
    crate::mod_configs::call,
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
    crate::zips::call,
    crate::guava_io::call,
    crate::streams::call,
    crate::gson::call,
    crate::jtypes::call,
    crate::jregex::call,
    crate::jsystem::call,
    crate::writers::call,
    crate::jdk::call,
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
    crate::nio::call,
    crate::channels::call,
    crate::records::call,
];

/// Which keys each module of [`CHAIN`] may answer, in the same order ([`crate::sounds::claims`]...):
/// a pure function of the key, so its answer is kept per key.
type Claims = fn(&str, &str, &str) -> bool;
const CLAIMS: [Claims; 99] = [
    crate::diodes::claims,
    crate::rails::claims,
    crate::fake_players::claims,
    crate::attribute_suppliers::claims,
    crate::spawning::claims,
    crate::block_breaking::claims,
    crate::loot_params::claims,
    |owner, _, _| owner == MUTABLE_POS,
    crate::worldgen::claims,
    crate::mixin::claims,
    crate::attachments::claims,
    crate::sounds::claims,
    crate::particles::claims,
    crate::particle_engine::claims,
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
    crate::goals::claims,
    crate::pathing::claims,
    crate::mobs::claims,
    crate::mod_entities::claims,
    crate::entities::claims,
    crate::creative::claims,
    crate::tooltips::claims,
    crate::damage::claims,
    crate::player_advancements::claims,
    crate::player_stats::claims,
    crate::world_data::claims,
    crate::world_events::claims,
    crate::made_entities::claims,
    crate::vanilla_entities::claims,
    crate::entity_saves::claims,
    crate::commands::claims,
    crate::brigadier::claims,
    crate::functions::claims,
    crate::gamerules::claims,
    crate::value_providers::claims,
    crate::weighted::claims,
    crate::nightconfig::claims,
    crate::mod_configs::claims,
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
    crate::zips::claims,
    crate::guava_io::claims,
    crate::streams::claims,
    crate::gson::claims,
    crate::jtypes::claims,
    crate::jregex::claims,
    crate::jsystem::claims,
    crate::writers::claims,
    crate::jdk::claims,
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
    crate::nio::claims,
    crate::channels::claims,
    crate::records::claims,
];

const MODULE_NAMES: [&str; 99] = [
    "diodes",
    "rails",
    "fake_players",
    "attribute_suppliers",
    "spawning",
    "block_breaking",
    "loot_params",
    "mutable_pos",
    "worldgen",
    "mixin",
    "attachments",
    "sounds",
    "particles",
    "particle_engine",
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
    "goals",
    "pathing",
    "mobs",
    "mod_entities",
    "entities",
    "creative",
    "tooltips",
    "damage",
    "player_advancements",
    "player_stats",
    "world_data",
    "world_events",
    "made_entities",
    "vanilla_entities",
    "entity_saves",
    "commands",
    "brigadier",
    "functions",
    "gamerules",
    "value_providers",
    "weighted",
    "nightconfig",
    "mod_configs",
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
    "zips",
    "guava_io",
    "streams",
    "gson",
    "jtypes",
    "jregex",
    "jsystem",
    "writers",
    "jdk",
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
    "nio",
    "channels",
    "records",
];

/// The modules of [`CHAIN`] that claim `key`, as bits (all of them for a key that isn't
/// `owner.name.desc`): worked out once per key, with its [`Route`].
fn claimed(key: &'static str) -> u128 {
    match crate::parts(key) {
        Some((owner, name, desc)) => CLAIMS.iter().enumerate().filter(|(_, claims)| claims(owner, name, desc)).fold(0u128, |bits, (i, _)| bits | 1 << i),
        None => (1u128 << CHAIN.len()) - 1,
    }
}

/// Whether every call checks that the module answering it claims its key
/// (`HORIZON_COMPAT_CHECK_DISPATCH=1`; always in tests): it is then offered to every module.
fn checking() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(test) || *CHECK.get_or_init(|| std::env::var_os("HORIZON_COMPAT_CHECK_DISPATCH").is_some_and(|v| v == "1"))
}

/// Offers the call to each module of [`CHAIN`] that claims its key, in order: the first that
/// answers does.
fn chain(rt: &mut Rt, key: &'static str, args: &[V], claimed: u128) -> Option<V> {
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
const SET_PLACED_BY_DESC: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;)V";
pub(crate) const UPDATE_SHAPE: &str = "updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const UPDATE_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
pub(crate) const CAN_SURVIVE: &str = "canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
pub(crate) const CAN_SURVIVE_KEY: &str =
    "net/minecraft/world/level/block/state/BlockBehaviour.canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
const LEVEL_DATA: &str = "net/minecraft/world/level/storage/LevelData";
const LEVEL_CHUNK: &str = "net/minecraft/world/level/chunk/LevelChunk";
const CHUNK_ACCESS: &str = "net/minecraft/world/level/chunk/ChunkAccess";
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
    let name = crate::host::state_block(text);
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
    let b = crate::host::state_block(text);
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
            let name = crate::host::state_block(text);
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
    if let Some(v) = rt.registry_entry("block", name) {
        return v;
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
    let name = crate::host::state_block(text);
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
    adopt(rt, None);
}

/// [`adopt_names`] for the one block `b`, registered just now as `name`: its states have its
/// name before anything can ask for one of them by it (a state asked for by a registered name
/// that the block's own states did not have yet was a second object of the same state).
pub(crate) fn adopt_block(rt: &mut Rt, b: V, name: &str) {
    adopt(rt, Some((b, name)));
}

fn adopt(rt: &mut Rt, only: Option<(V, &str)>) {
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
        let name = match only {
            Some((block, name)) if block == b => Some(name.to_owned()),
            Some(_) => None,
            None => rt.registered_name(b, "block").map(str::to_owned),
        };
        let Some(name) = name else {
            waiting.push(pair);
            continue;
        };
        let Some(old) = rt.str_of(state).map(str::to_owned) else { continue };
        let Some(rest) = old.strip_prefix(&format!("{UNREGISTERED}{}", b.r())) else { continue };
        let new = format!("{name}{rest}");
        if rt.interned.get(&format!("state/{old}")) == Some(&state) {
            rt.interned.remove(&format!("state/{old}"));
        }
        // Spelled in full, as every state of a registered block is ([`spelled`]): the first
        // state made of a spelling is that state, and the name it was made under reads as it.
        let full = spelled(rt, b, &new).unwrap_or_else(|| new.clone());
        let one = *rt.interned.entry(format!("state/{full}")).or_insert(state);
        rt.interned.entry(format!("state/{new}")).or_insert(one);
        if let Some(o) = rt.heap.get_mut(state.r() as usize) {
            o.data = Data::Str(full);
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
    if let Some(v) = crate::level_env::difficulty_call(rt, owner, name, desc, args) {
        return Some(v);
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
        // The register's own fields, as a mixin shadows them (Veil adds a
        // `register(ResourceLocation, Function)` that takes any namespace: it makes the holder
        // with `createHolder` and puts it with its supplier in `entries`): nothing is
        // registered yet while the mods load, and what is put in the map is registered.
        (o, "$seenRegisterEvent")
            if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister")
                && desc.starts_with("()") =>
        {
            V::I(0)
        }
        (o, "$entries")
            if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister")
                && desc.starts_with("()") =>
        {
            let held = rt.get_field(a(0), "$entries-map");
            if held.r() != 0 {
                return Some(held);
            }
            let map = rt.alloc(
                "java/util/LinkedHashMap",
                Data::Table(crate::Table::default()),
            );
            rt.put_field(map, DEFERRED_ENTRIES, a(0));
            rt.put_field(a(0), "$entries-map", map);
            map
        }
        (o, "createHolder")
            if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister")
                && args.len() == 3 =>
        {
            let (registry, id) = (
                rt.str_of(a(1)).unwrap_or("unknown").to_owned(),
                rt.str_of(a(2)).unwrap_or("").to_owned(),
            );
            holder(rt, &registry, &id)
        }
        // Every holder made through it, in order (NeoForge's is an unmodifiable view).
        (o, "getEntries") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let entries = rt.get_field(a(0), "$holders");
            let items = if entries.r() == 0 { Vec::new() } else { crate::jutil::elements(rt, entries) };
            crate::jutil::new_list(rt, items)
        }
        ("net/neoforged/neoforge/registries/DeferredRegister", "getRegistryKey") => rt.get_field(a(0), "$registry"),
        // `getRegistry()`: a supplier of the registry, which is its key here.
        ("net/neoforged/neoforge/registries/DeferredRegister", "getRegistry") => {
            let registry = rt.get_field(a(0), "$registry");
            crate::functions::constant(rt, registry)
        }
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
            rt.interned.insert(crate::neo::ALIASED.into(), V::I(1));
            V::Void
        }
        (o, "get" | "value") if o.starts_with("net/neoforged/neoforge/registries/Deferred") => {
            let id = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned();
            if rt.registered.keys().all(|(_, n)| *n != id) {
                crate::neo::resolve_named(rt, &id);
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
        // The thread a level is ticked on (`Level.thread`): the one asking, as the server's
        // running thread is, since a level's hooks run on its own thread. Immersive Portals
        // reads it through a mixin at every packet and chunk ticket, and logs an error with a
        // stack trace where it is another thread's.
        (_, "$thread") if level => call(
            rt,
            "java/lang/Thread.currentThread.()Ljava/lang/Thread;",
            &[],
        ),
        // A level's side is its own (vanilla's `isClientSide` field: the client's level, a mod's
        // wrapper made on either side); one that doesn't carry it is on the host's.
        (_, "isClientSide" | "$isClientSide") if level => match rt.get_field(a(0), "isClientSide") {
            V::I(c) => V::I(c),
            _ => V::I(rt.host().is_some_and(|h| !h.is_server()) as i32),
        },
        (_, "isLoaded" | "hasChunkAt" | "hasChunk" | "isAreaLoaded" | "hasChunksAt" | "isInWorldBounds" | "areEntitiesLoaded") if level => V::I(1),
        // Whether the chunk ticks its blocks (`shouldTickBlocksAt`, of a position or a chunk's
        // `ChunkPos.asLong`) or its entities (`isPositionEntityTicking`, `isNaturalSpawningAllowed`),
        // as the game's tickets say: Create keeps a train's carriages as entities only there.
        (_, "shouldTickBlocksAt" | "isPositionEntityTicking" | "isNaturalSpawningAllowed")
            if level && desc.ends_with(")Z") =>
        {
            let p = match a(1) {
                V::J(chunk) => [(chunk as i32) << 4, 0, ((chunk >> 32) as i32) << 4],
                pos if desc.starts_with("(Lnet/minecraft/core/BlockPos;") => pos_of(rt, pos),
                _ => return None,
            };
            let (blocks, entities) = rt.host().map_or((true, true), |h| h.ticking(p));
            V::I(i32::from(if name == "shouldTickBlocksAt" {
                blocks
            } else {
                entities
            }))
        }
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
            "isFrozen" | "isSteppingForward" | "isSprinting" | "isEntityFrozen" => V::I(0),
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
        // A level's chunk cache, its dimension's (its saved data is that level's).
        (_, "getChunkSource") if level => {
            let d = crate::glue::level_dimension(rt, a(0));
            rt.intern(format!("compat:chunk-source/{d}"), |rt| {
                let cache = rt.new_object("net/minecraft/server/level/ServerChunkCache");
                let dimension = rt.string(&d);
                rt.put_field(cache, "$dimension", dimension);
                cache
            })
        }
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
        // Players see the block entity's new data (Create's `sendData`), and the game holds it.
        ("net/minecraft/server/level/ServerChunkCache" | "net/minecraft/world/level/chunk/ChunkSource", "blockChanged") => {
            let p = pos_of(rt, a(1));
            crate::glue::block_entity_dirty(rt, p);
            if let Some(h) = rt.host_mut() {
                h.block_changed(p);
            }
            V::Void
        }
        (_, "sendBlockUpdated") if level => {
            let p = pos_of(rt, a(1));
            crate::glue::block_entity_dirty(rt, p);
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
        // `Level.blockEntityChanged`: its chunk is to be saved, with the block entity's data.
        (_, "blockEntityChanged") if level => {
            let p = pos_of(rt, a(1));
            crate::glue::block_entity_dirty(rt, p);
            V::Void
        }
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
            let id = crate::glue::level_dimension(rt, a(0));
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
            let d = crate::glue::level_dimension(rt, a(0));
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
        (_, "getServer") if level => crate::neo::server(rt),
        ("net/minecraft/server/MinecraftServer" | "net/minecraft/server/dedicated/DedicatedServer", n) => match n {
            "getTickCount" => V::I(rt.host().map_or(0, |h| h.game_time()) as i32),
            // A client's runtime has its one level (a client can't reach the server's).
            "overworld" | "getLevel" | "getAllLevels" if crate::neo::is_client(rt) => {
                let l = crate::glue::level(rt);
                if n == "getAllLevels" {
                    crate::jutil::new_list(rt, vec![l])
                } else {
                    l
                }
            }
            // The server's levels, one a dimension (`crate::glue::server_level`).
            "overworld" => crate::glue::server_level(rt, "minecraft:overworld"),
            // A level the server has (vanilla's three, or the one the hook runs in); null for another,
            // as `MinecraftServer.getLevel` answers for a dimension it lacks.
            "getLevel" => {
                let key = rt.str_of(a(1)).unwrap_or("").to_owned();
                let id = key
                    .rsplit_once('#')
                    .map_or(key.as_str(), |(_, id)| id)
                    .to_owned();
                let here = rt.host().map(|h| h.dimension());
                if matches!(
                    id.as_str(),
                    "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
                ) || here.as_deref() == Some(id.as_str())
                {
                    crate::glue::server_level(rt, &id)
                } else {
                    V::NULL
                }
            }
            // The levels the server has loaded, the Overworld first.
            "getAllLevels" => {
                crate::glue::server_level(rt, "minecraft:overworld");
                let made = rt
                    .interned
                    .get(crate::glue::SERVER_LEVELS)
                    .copied()
                    .unwrap_or_default();
                let all = crate::jutil::elements(rt, made);
                crate::jutil::new_list(rt, all)
            }
            "isDedicatedServer" | "isPublished" | "isHardcore" | "isStopped" => V::I(0),
            // The window's own server, as a world opened from the menu has it.
            "isSingleplayer" | "isSingleplayerOwner" | "isRunning" => V::I(1),
            "isSameThread" => V::I(1),
            // The thread the server's hooks run on is the one asking (Immersive Portals checks
            // `Thread.currentThread() == server.getRunningThread()` at every packet).
            "getRunningThread" => call(
                rt,
                "java/lang/Thread.currentThread.()Ljava/lang/Thread;",
                &[],
            ),
            "getPlayerList" => crate::events::player_list(rt),
            "tickRateManager" => rt.intern("compat:tick-rate".into(), |rt| {
                rt.new_object("net/minecraft/server/ServerTickRateManager")
            }),
            // The times of the last hundred ticks (the server's, or NeoForge's for one level):
            // none is measured for the mods.
            "getTickTimesNanos" | "getTickTime" => rt.alloc("[J", Data::Array(vec![V::J(0); 100])),
            "getAverageTickTimeNanos" => V::J(0),
            "getCurrentSmoothedTickTime" => V::F(0.0),
            // The keys of the levels it has (`getAllLevels`).
            "levelKeys" => {
                let levels = call(
                    rt,
                    "net/minecraft/server/MinecraftServer.getAllLevels.()Ljava/lang/Iterable;",
                    &[a(0)],
                );
                let mut keys = Vec::new();
                for level in crate::jutil::elements(rt, levels) {
                    keys.push(call(rt, "net/minecraft/world/level/Level.dimension.()Lnet/minecraft/resources/ResourceKey;", &[level]));
                }
                crate::jutil::new_set_of(rt, &keys)
            }
            "getProfiler" => {
                get_static(rt, "net/minecraft/util/profiling/InactiveProfiler.INSTANCE")
            }
            _ => return None,
        },

        // `Block.getSeed(state, pos)`, a mod block's `super.` call: `Mth.getSeed(pos)`.
        ("net/minecraft/world/level/block/Block", "getSeed") if BLOCK_SEED_KEY.ends_with(desc) => pos_seed(rt, a(2)),
        // `hasCollision` of a block, read as the field it is (an access transformer opens it):
        // a mod's block as its properties say (`noCollission()`); a block of the game's by
        // what the game tells of its default state, which is solid or has a face that holds
        // a block up where it has a shape to walk into (a plant, a torch and a rail have
        // neither). Structurize lists the blocks that stand without support by it at a
        // level's tick, and listed them again at every tick while the list stayed empty.
        (o, "$hasCollision")
            if o.starts_with("net/minecraft/world/level/block/") && desc == "()Z" =>
        {
            let properties = rt.get_field(a(0), "$properties");
            if properties.r() != 0 {
                V::I(i32::from(rt.get_field(properties, "$noCollision").i() == 0))
            } else {
                let state = call(
                    rt,
                    "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;",
                    &[a(0)],
                );
                let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
                let holds = rt
                    .host()
                    .and_then(|h| h.block_facts(&text))
                    .is_some_and(|f| f.sturdy != 0);
                // (A cobweb and a bamboo sapling are solid by decree and have no collision:
                // the two blocks `blocksMotion()` leaves out for that.)
                let passable = matches!(
                    crate::host::state_block(&text),
                    "minecraft:cobweb" | "minecraft:bamboo_sapling"
                );
                V::I(i32::from(
                    holds || (!passable && crate::block_breaking::solid(rt, &text)),
                ))
            }
        }
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
            let b = crate::host::state_block(&text);
            let unstable = direction == 0 && rt.host().is_some_and(|h| h.block_in_tag(b, "minecraft:unstable_bottom_center"));
            V::I(i32::from(!unstable && face_sturdy(rt, &text, [state, a(0), a(1)], direction, SUPPORT_CENTER)))
        }
        // `Block.shouldRenderFace(state, level, pos, face, neighbourPos)`: whether a face shows past
        // its neighbour (`faces`). On any block class, as `javac` names the class a call was written
        // on.
        (o, "shouldRenderFace") if o.starts_with("net/minecraft/world/level/block/") && desc == crate::faces::SHOULD_RENDER_FACE => {
            V::I(i32::from(crate::faces::should_render_face(rt, [a(0), a(1), a(2), a(3), a(4)])))
        }
        // `super.getRenderShape(state)` to a vanilla block class: that class's own (a block with
        // an entity is invisible unless it says otherwise, a chest is drawn by its block entity,
        // the rest are models).
        (o, "getRenderShape") if o.starts_with("net/minecraft/world/level/block/") && GET_RENDER_SHAPE.ends_with(desc) => {
            let ordinal = crate::vanilla_shapes::CLASSES.binary_search_by(|(c, _)| (*c).cmp(o)).map_or(2, |i| crate::vanilla_shapes::CLASSES[i].1);
            let ordinal = usize::from(ordinal).min(2);
            api_enum(rt, "net/minecraft/world/level/block/RenderShape", RENDER_SHAPES[ordinal], ordinal as i32)
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
            let b = crate::host::state_block(&text);
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
                // `getTags()`: the block's tags.
                "getTags" => {
                    let b = b.to_owned();
                    let tags = crate::tag_files::tags_of(rt, "block", &b);
                    rt.alloc("java/util/stream/Stream", Data::Stream(tags))
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
                // Whether it breaks and cracks into crumbs (`ParticleEngine.destroy`, `crack`).
                "shouldSpawnTerrainParticles" if desc == "()Z" => V::I(i32::from(crate::blocks::spawns_terrain_particles(rt, &text))),
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
                // One list for a block, as the game's definition keeps one (an
                // `ImmutableList`): a mod asks for it again for each thing it looks up among
                // the states (Resourceful Lib, for every variant of every highlight it loads),
                // and a list of every state made anew each time was gigabytes within one
                // reload. Made again should the block's properties have changed since (a block
                // asked while it is being constructed).
                "getPossibleStates" => {
                    let declared = match rt.heap.get(b.r() as usize).map(|o| &o.data) {
                        Some(Data::Fields(_)) => {
                            let props = rt.get_field(b, "$state-props");
                            match rt.heap.get(props.r() as usize).map(|o| &o.data) {
                                Some(Data::List(l)) => l.len(),
                                _ => 0,
                            }
                        }
                        _ => 0,
                    };
                    let key = format!("possible-states/{}/{declared}", b.r());
                    match rt.interned.get(&key) {
                        Some(&list) => list,
                        None => {
                            let states = possible_states(rt, b);
                            let list = rt.alloc(
                                "com/google/common/collect/ImmutableList",
                                Data::List(states),
                            );
                            rt.interned.insert(key, list);
                            list
                        }
                    }
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

/// Mod block `b`'s state properties as its states spell them: each by name with its values,
/// in the order of the names (the game's order). What [`possible_states`] makes its states of
/// and [`spelled`] completes a state with.
fn state_props(rt: &mut Rt, b: V) -> Vec<(String, Vec<String>)> {
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
    props
}

/// The state of registered mod block `b` that `text` names, with every property spelled: one
/// the text leaves out has the value of the block's default state (the one its constructor
/// registered, else the property's first value, as `StateDefinition.any()` has it). None for a
/// block without properties.
///
/// A state is one object however it is named ([`crate::api_common::state`]): the bare name
/// `defaultBlockState()` reads as, `any().setValue(...)` and the state a game spells in full
/// are the state `getPossibleStates()` lists. A mod keeps what it knows of a state on that
/// object (FramedBlocks its cache, through a mixin's field), or in a map by it, and found
/// nothing where the same state went by another name.
pub(crate) fn spelled(rt: &mut Rt, b: V, text: &str) -> Option<String> {
    let props = state_props(rt, b);
    if props.is_empty() {
        return None;
    }
    let (block, given) = crate::host::split_state(text);
    let registered = rt
        .str_of(rt.get_field(b, "$default"))
        .unwrap_or("")
        .to_owned();
    let registered: Vec<(&str, &str)> = registered
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .collect();
    let mut all: Vec<(&str, &str)> = Vec::with_capacity(props.len());
    fn said<'a>(from: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
        from.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }
    for (name, values) in &props {
        let value = said(&given, name)
            .or_else(|| said(&registered, name).filter(|v| values.iter().any(|x| x == v)))
            .unwrap_or(values[0].as_str());
        all.push((name, value));
    }
    // (What the text says of a property the definition doesn't have stays said.)
    all.extend(
        given
            .iter()
            .filter(|(k, _)| !props.iter().any(|(n, _)| n == k))
            .copied(),
    );
    Some(crate::host::join_state(block, all))
}

/// The most states [`possible_states`] spells out; a definition with more (vanilla's largest,
/// redstone wire, has 1296) keeps its default state alone.
const MAX_STATES: usize = 1 << 16;

/// Block `b`'s states in `StateDefinition.getPossibleStates()`'s order: every combination of its
/// properties' values, the property first by name varying slowest, each property's values in its
/// own order. A vanilla block, whose definition the game keeps, answers its default state.
pub(crate) fn possible_states(rt: &mut Rt, b: V) -> Vec<V> {
    let name = block_name(rt, b);
    let props = state_props(rt, b);
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
    held(rt, register, h);
    h
}

/// Adds holder `h` to those made through `register` (`getEntries`).
fn held(rt: &mut Rt, register: V, h: V) {
    let mut entries = rt.get_field(register, "$holders");
    if entries.r() == 0 {
        entries = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(register, "$holders", entries);
    }
    crate::jutil::list_add(rt, entries, h);
}

/// Where a `DeferredRegister`'s `entries` map, as a mixin reads the field (holder to
/// supplier), keeps its register: what is put in such a map registers ([`deferred_entry`]).
pub(crate) const DEFERRED_ENTRIES: &str = "$deferred-register";

/// A holder and its supplier put in a register's `entries` map (`map`) by a mixin's own code:
/// registered as `DeferredRegister.register` registers one, unless the map holds the holder
/// already (the mod's code then throws, as NeoForge's does for a name registered twice).
pub(crate) fn deferred_entry(rt: &mut Rt, map: V, h: V, supplier: V) {
    if h.r() == 0 || crate::jutil::table_get(rt, map, h).is_some() {
        return;
    }
    let (registry, id) = (
        rt.str_of(rt.get_field(h, "$registry"))
            .unwrap_or("unknown")
            .to_owned(),
        rt.str_of(rt.get_field(h, "$id")).unwrap_or("").to_owned(),
    );
    let register = rt.get_field(map, DEFERRED_ENTRIES);
    held(rt, register, h);
    rt.suppliers.push((registry, id, supplier));
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
    // An enum's `$VALUES`, which a mixin into the enum shadows to add constants of its own
    // ([`crate::enum_ext`]): its constants.
    if let Some(class) = key.strip_suffix(".$VALUES")
        && let Some(all) = crate::jlang::enum_constants(rt, class)
    {
        return all;
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
            .or_else(|| crate::spawning::get_static(rt, o, f))
            .or_else(|| crate::pathing::get_static(rt, o, f))
            .or_else(|| crate::projectiles::get_static(rt, o, f))
            .or_else(|| crate::tanks::get_static(rt, o, f))
            .or_else(|| crate::energy::get_static(rt, o, f))
            .or_else(|| crate::creative::get_static(rt, o, f))
            .or_else(|| crate::tooltips::get_static(rt, o, f))
            .or_else(|| crate::gamerules::get_static(rt, o, f))
            .or_else(|| crate::value_providers::get_static(rt, o, f))
            .or_else(|| crate::nightconfig::get_static(rt, o, f))
            .or_else(|| crate::mod_configs::get_static(rt, o, f))
            .or_else(|| crate::player_stats::get_static(rt, o, f))
            .or_else(|| crate::world_data::get_static(rt, o, f))
            .or_else(|| crate::codecs::get_static(rt, o, f))
            .or_else(|| crate::recipes::get_static(rt, o, f))
            .or_else(|| crate::components::get_static(rt, o, f))
            .or_else(|| crate::capabilities::get_static(rt, o, f))
            .or_else(|| crate::sounds::get_static(rt, o, f))
            .or_else(|| crate::particles::get_static(rt, o, f))
            .or_else(|| crate::particle_engine::get_static(rt, o, f))
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
        // The empty collections as fields, older than the methods that give them.
        "java/util/Collections.EMPTY_LIST" => call(
            rt,
            "java/util/Collections.emptyList.()Ljava/util/List;",
            &[],
        ),
        "java/util/Collections.EMPTY_SET" => {
            call(rt, "java/util/Collections.emptySet.()Ljava/util/Set;", &[])
        }
        "java/util/Collections.EMPTY_MAP" => {
            call(rt, "java/util/Collections.emptyMap.()Ljava/util/Map;", &[])
        }
        k if k.starts_with("java/util/logging/Level.") => {
            match crate::jutil::logging_level(rt, k.rsplit('.').next().unwrap_or("")) {
                Some(level) => level,
                None => {
                    crate::miss(rt, key);
                    V::NULL
                }
            }
        }
        // `Unsafe.theUnsafe`, which a mod takes by reflection to write a field it may not
        // ([`crate::jlang`]'s `sun.misc.Unsafe`).
        "sun/misc/Unsafe.theUnsafe" => crate::jlang::the_unsafe(rt),
        // A byte buffer's two orders ([`crate::nio`]).
        "java/nio/ByteOrder.BIG_ENDIAN" => crate::nio::order(rt, false),
        "java/nio/ByteOrder.LITTLE_ENDIAN" => crate::nio::order(rt, true),
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
        _ => match crate::records::enum_constant(rt, key)
            .or_else(|| crate::api_common::carried_constant(rt, key))
        {
            Some(v) => v,
            None => match key.rsplit_once('.').and_then(|(o, f)| Some((api_super(o)?, f))) {
                Some((declaring, field)) => {
                    let inherited = rt.static_key(&format!("{declaring}.{field}"));
                    get_static(rt, inherited)
                }
                // Nothing known of it: not Java's `null`, which a call on throws for
                // (`linked::dispatch`), but what the code goes on with, as with an unmapped
                // call's answer.
                None => {
                    crate::miss(rt, key);
                    trace_unmapped(rt, key, &[]);
                    V::Void
                }
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A call through a class of the game's is the method of the nearest class above it that
    /// declares one, as the game's class files have it (a player's block position is
    /// `Entity`'s, a level's empty-block test `LevelReader`'s default); the class's own where
    /// it declares one, whatever is above (`FallingBlock.tick`); nothing for a class that is
    /// not the game's, or a method nothing declares.
    #[test]
    fn a_call_is_resolved_as_the_games_class_files_declare_it() {
        let above = |class: &'static str, rest: &str| resolved_above(class, method_hash(rest));
        const ENTITY: Option<&str> = Some("net/minecraft/world/entity/Entity");
        assert_eq!(
            above("net/minecraft/world/entity/player/Player", "getBlockY.()I"),
            ENTITY
        );
        assert_eq!(
            above("net/minecraft/client/player/LocalPlayer", "getBlockY.()I"),
            ENTITY
        );
        assert_eq!(
            above("net/minecraft/world/entity/Entity", "getBlockY.()I"),
            None
        );
        assert_eq!(
            above(
                "net/minecraft/server/level/ServerLevel",
                "isEmptyBlock.(Lnet/minecraft/core/BlockPos;)Z"
            ),
            Some("net/minecraft/world/level/LevelReader")
        );
        let tick = "tick.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)V";
        assert_eq!(
            above("net/minecraft/world/level/block/FallingBlock", tick),
            None
        );
        assert_eq!(
            above("net/minecraft/world/level/block/BushBlock", tick),
            Some("net/minecraft/world/level/block/state/BlockBehaviour")
        );
        assert_eq!(above("java/util/ArrayList", "size.()I"), None);
        assert_eq!(
            above(
                "net/minecraft/world/entity/player/Player",
                "noSuchMethod.()V"
            ),
            None
        );
    }

    /// The census counts what the heap holds by class, and a collection's garbage is the
    /// difference of two: a list nothing names is counted before it and gone after.
    #[test]
    fn the_census_counts_what_a_collection_frees() {
        let mut rt = Rt::new();
        let kept = crate::jutil::new_list(&mut rt, vec![V::I(1); 100]);
        rt.pinned.push(kept);
        crate::jutil::new_list(&mut rt, vec![V::I(2); 1000]);
        let before = census(&rt);
        let lists = before["java/util/ArrayList"];
        assert_eq!(lists.0, 2);
        assert!(
            lists.1 >= 1100 * 16,
            "the two lists' elements are counted: {} bytes",
            lists.1
        );
        rt.collect();
        let after = census(&rt);
        assert_eq!(after["java/util/ArrayList"].0, 1);
        assert!(after["java/util/ArrayList"].1 < 1000 * 16);
        tell_census("a test", &before, &after);
    }

    /// Java's `null` stays empty whatever a call on a `null` receiver writes in place: a lambda
    /// with nothing captured (a mixin's handler, run with no lambda object) reads nothing from
    /// it afterwards, where a `BitSet.set` on `null` once left its words.
    #[test]
    fn a_call_on_null_leaves_null_empty() {
        let mut rt = Rt::new();
        call(
            &mut rt,
            "java/util/BitSet.set.(II)V",
            &[V::NULL, V::I(0), V::I(5)],
        );
        call(&mut rt, "java/util/BitSet.set.(I)V", &[V::NULL, V::I(70)]);
        assert!(
            rt.captured(V::NULL).is_empty(),
            "a bit set on null wrote {:?}",
            rt.captured(V::NULL)
        );
        // A constructor that makes its object over in place, called on nothing.
        let inner = rt.new_object("com/mojang/serialization/MapCodec");
        call(
            &mut rt,
            "com/mojang/serialization/MapCodec$MapCodecCodec.<init>.(Lcom/mojang/serialization/MapCodec;)V",
            &[V::NULL, inner],
        );
        assert!(rt.captured(V::NULL).is_empty());
        assert_eq!(rt.heap[0].class, "null");
        // And a bit set that is one still holds its bits.
        let bits = rt.new_object("java/util/BitSet");
        call(&mut rt, "java/util/BitSet.<init>.()V", &[bits]);
        call(
            &mut rt,
            "java/util/BitSet.set.(II)V",
            &[bits, V::I(0), V::I(5)],
        );
        assert_eq!(
            call(&mut rt, "java/util/BitSet.cardinality.()I", &[bits]),
            V::I(5)
        );
    }

    /// `StackWalker.walk` applies the function to a stream of no frames and gives back what it
    /// answers: a check of every frame it looks at passes (Lucene's `skip(2).limit(1).allMatch`),
    /// a search for a frame finds none.
    #[test]
    fn a_stack_walk_sees_no_frames() {
        fn walked(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            let frames = args[1];
            Some(if id == 1 {
                let s = call(
                    rt,
                    "java/util/stream/Stream.skip.(J)Ljava/util/stream/Stream;",
                    &[frames, V::J(2)],
                );
                let s = call(
                    rt,
                    "java/util/stream/Stream.limit.(J)Ljava/util/stream/Stream;",
                    &[s, V::J(1)],
                );
                let never = rt.lambda_id("java/util/function/Predicate", 3, vec![]);
                let all = call(
                    rt,
                    "java/util/stream/Stream.allMatch.(Ljava/util/function/Predicate;)Z",
                    &[s, never],
                );
                call(
                    rt,
                    "java/lang/Boolean.valueOf.(Z)Ljava/lang/Boolean;",
                    &[all],
                )
            } else if id == 2 {
                call(
                    rt,
                    "java/util/stream/Stream.findFirst.()Ljava/util/Optional;",
                    &[frames],
                )
            } else {
                // The predicate: no frame is one of its own.
                V::I(0)
            })
        }
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(walked);
        let walker = call(
            &mut rt,
            "java/lang/StackWalker.getInstance.()Ljava/lang/StackWalker;",
            &[],
        );
        const WALK: &str =
            "java/lang/StackWalker.walk.(Ljava/util/function/Function;)Ljava/lang/Object;";
        let check = rt.lambda_id("java/util/function/Function", 1, vec![]);
        let valid = call(&mut rt, WALK, &[walker, check]);
        assert_eq!(
            call(&mut rt, "java/lang/Boolean.booleanValue.()Z", &[valid]),
            V::I(1)
        );
        let first = rt.lambda_id("java/util/function/Function", 2, vec![]);
        let found = call(&mut rt, WALK, &[walker, first]);
        assert_eq!(
            call(&mut rt, "java/util/Optional.isPresent.()Z", &[found]),
            V::I(0)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A call nothing maps runs the game's own method the translator carried for it: the one of
    /// the receiver's class, or of a class above it (a static one too), and it is no miss. Where
    /// a class below the receiver's has code for the method too (the translator's mark), the
    /// object may be of that class: the call stays a miss, as one nothing carried does.
    #[test]
    fn an_unmapped_call_runs_the_games_own_code_carried_for_it() {
        use crate::linked::{ClassDef, Linked, Method, Registry};
        fn twice(_: &mut Rt, args: &[V]) -> V {
            V::I(args[0].i() * 2)
        }
        fn plus_one(_: &mut Rt, args: &[V]) -> V {
            V::I(args[1].i() + 1)
        }
        fn plus_hundred(_: &mut Rt, args: &[V]) -> V {
            V::I(args[1].i() + 100)
        }
        const fn class(
            name: &'static str,
            id: u32,
            superclass: Option<&'static str>,
            methods: &'static [(&'static str, Method, bool)],
            fields: &'static str,
        ) -> ClassDef {
            ClassDef {
                name,
                id,
                superclass,
                interfaces: &[],
                methods,
                mixed: &[],
                reflect: None,
                fields,
                annotations: "",
                subscribers: &[],
            }
        }
        const THING: &str = "hz/derived/net/minecraft/hztest/Thing";
        static CLASSES: [ClassDef; 3] = [
            class(
                "hz/derived/net/minecraft/hztest/Above",
                1,
                None,
                &[("twice.(I)I", twice, false)],
                "",
            ),
            class(
                THING,
                2,
                Some("hz/derived/net/minecraft/hztest/Above"),
                &[("plusOne.(I)I", plus_one, false)],
                "0 !plusOne (I)I",
            ),
            class(
                "hz/derived/net/minecraft/hztest/Lower",
                3,
                Some(THING),
                &[("plusOne.(I)I", plus_hundred, false)],
                "",
            ),
        ];
        static REGISTRY: Registry = Registry {
            classes: &CLASSES,
            lambdas: &[],
            added: &[],
            game_interfaces: &[],
            subscriber_options: &[],
        };
        static LINKED: std::sync::OnceLock<Linked> = std::sync::OnceLock::new();
        crate::install();
        let mut rt = Rt::new();
        rt.link(LINKED.get_or_init(|| Linked::new([&REGISTRY], &[])));
        assert_eq!(
            call(
                &mut rt,
                "net/minecraft/hztest/Thing.twice.(I)I",
                &[V::I(21)]
            )
            .i(),
            42
        );
        let lower = rt.new_object("net/minecraft/hztest/Lower");
        assert_eq!(
            call(
                &mut rt,
                "net/minecraft/hztest/Thing.plusOne.(I)I",
                &[lower, V::I(6)]
            )
            .i(),
            106
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        let thing = rt.new_object("net/minecraft/hztest/Thing");
        call(
            &mut rt,
            "net/minecraft/hztest/Thing.plusOne.(I)I",
            &[thing, V::I(6)],
        );
        assert_eq!(rt.unmapped.len(), 1);
        call(&mut rt, "net/minecraft/hztest/Thing.other.()I", &[lower]);
        assert_eq!(rt.unmapped.len(), 2);
    }

    /// No module of the chain claims an intrinsic's key before the one whose arm it answers with:
    /// a module that did would be asked first through the chain, and the intrinsic would skip it.
    #[test]
    fn intrinsics_come_first() {
        let first = |key: &'static str| MODULE_NAMES[claimed(key).trailing_zeros() as usize];
        for key in [BE_GET_LEVEL, BE_HAS_LEVEL, BE_GET_POS, BE_GET_STATE] {
            assert!(intrinsic(key).is_some(), "{key}");
            assert_eq!(first(key), "world", "{key}");
        }
        for key in [
            MAP_VALUES,
            COLLECTION_FOR_EACH,
            ITERATOR_HAS_NEXT,
            ITERATOR_NEXT,
            ENTRY_GET_KEY,
            ENTRY_GET_VALUE,
        ] {
            assert!(intrinsic(key).is_some(), "{key}");
            assert_eq!(first(key), "jutil", "{key}");
        }
    }

    thread_local! {
        static SEEN: std::cell::RefCell<Vec<V>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// The test's one lambda, a `Consumer`: keeps what it is given.
    fn keep(_: &mut Rt, _: u32, args: &[V]) -> Option<V> {
        SEEN.with(|s| s.borrow_mut().push(args[1]));
        Some(V::Void)
    }

    /// The intrinsics answer as the module chain does, receiver by receiver: a block entity's
    /// level, position and state as its constructor and `setLevel` set them (tests check every
    /// reading intrinsic's answer against the chain's as they call it); a map's values, in its
    /// order, and none for a map the runtime doesn't hold (a mod's own class); a list's elements
    /// to `forEach`'s action, in order, and nothing for a receiver that isn't Java's `ArrayList`.
    #[test]
    fn intrinsics_answer_as_the_chain() {
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(keep);
        rt.supers.insert("test/DialBlockEntity", "net/minecraft/world/level/block/entity/BlockEntity");
        let be = rt.new_object("test/DialBlockEntity");
        let (ty, pos, state) = (rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType"), block_pos(&mut rt, 1, 2, 3), state(&mut rt, "minecraft:stone"));
        call(&mut rt, "net/minecraft/world/level/block/entity/BlockEntity.<init>.(Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V", &[be, ty, pos, state]);
        assert_eq!(call(&mut rt, BE_HAS_LEVEL, &[be]), V::I(0));
        assert_eq!(call(&mut rt, BE_GET_LEVEL, &[be]), V::NULL);
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        call(&mut rt, "net/minecraft/world/level/block/entity/BlockEntity.setLevel.(Lnet/minecraft/world/level/Level;)V", &[be, level]);
        assert_eq!(call(&mut rt, BE_HAS_LEVEL, &[be]), V::I(1));
        assert_eq!(call(&mut rt, BE_GET_LEVEL, &[be]), level);
        assert_eq!(call(&mut rt, BE_GET_POS, &[be]), pos);
        assert_eq!(call(&mut rt, BE_GET_STATE, &[be]), state);

        let values = |rt: &mut Rt, m: V, through_chain: bool| -> Option<Vec<V>> {
            let v = if through_chain { chain(rt, MAP_VALUES, &[m], claimed(MAP_VALUES)) } else { crate::jutil::plain_map_values(rt, &[m]) };
            v.map(|l| crate::jutil::elements(rt, l))
        };
        for class in ["java/util/HashMap", "java/util/LinkedHashMap", "java/util/TreeMap"] {
            let m = rt.new_object(class);
            let init = rt.static_key(&format!("{class}.<init>.()V"));
            call(&mut rt, init, &[m]);
            for (k, v) in [("b", 2), ("a", 1), ("c", 3)] {
                let k = rt.string(k);
                call(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[m, k, V::I(v)]);
            }
            let fast = values(&mut rt, m, false);
            assert_eq!(fast.as_ref().map(Vec::len), Some(3), "{class}");
            assert_eq!(fast, values(&mut rt, m, true), "{class}");
        }
        let own = rt.new_object("test/OwnMap");
        assert_eq!(values(&mut rt, own, false), None);

        let action = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        let each = |rt: &mut Rt, l: V, through_chain: bool| -> (Option<V>, Vec<V>) {
            SEEN.with(|s| s.borrow_mut().clear());
            let r = if through_chain { chain(rt, COLLECTION_FOR_EACH, &[l, action], claimed(COLLECTION_FOR_EACH)) } else { crate::jutil::plain_list_for_each(rt, &[l, action]) };
            (r, SEEN.with(|s| s.borrow().clone()))
        };
        let list = crate::jutil::new_list(&mut rt, vec![V::I(7), V::I(8), V::I(9)]);
        let fast = each(&mut rt, list, false);
        assert_eq!(fast, (Some(V::Void), vec![V::I(7), V::I(8), V::I(9)]));
        assert_eq!(fast, each(&mut rt, list, true));
        let queue = rt.alloc("java/util/PriorityQueue", Data::List(vec![V::I(1)]));
        assert_eq!(each(&mut rt, queue, false), (None, vec![]));
    }

    /// The libraries' and the game's throwables that no module models answer `Throwable`'s own
    /// calls: Gson's `JsonParseException(message)`, Netty's `DecoderException(message, cause)`, the
    /// game's `ResourceLocationException`. They are what Gson 2.10, Netty 4.1 and Commons Lang 3
    /// declare them under (`catch (RuntimeException e)` catches a `DecoderException`). A mod's own
    /// subclass climbs to them; DFU's `DataResult.Error` and Realms' `RealmsError` records only
    /// look like throwables.
    #[test]
    fn the_libraries_and_the_games_throwables_are_throwables() {
        const MESSAGE: &str = "getMessage.()Ljava/lang/String;";
        let mut rt = Rt::new();
        crate::install();
        let k = |owner: &str, rest: &str| -> &'static str {
            Box::leak(format!("{owner}.{rest}").into_boxed_str())
        };
        let text = |rt: &mut Rt, v: V| rt.str_of(v).map(str::to_owned);
        let gson = "com/google/gson/JsonParseException";
        let e = rt.new_object(gson);
        let bad = rt.string("Expected a JSON object");
        call(&mut rt, k(gson, "<init>.(Ljava/lang/String;)V"), &[e, bad]);
        let m = call(&mut rt, k(gson, MESSAGE), &[e]);
        assert_eq!(text(&mut rt, m).as_deref(), Some("Expected a JSON object"));
        let s = call(&mut rt, k(gson, "toString.()Ljava/lang/String;"), &[e]);
        assert_eq!(
            text(&mut rt, s).as_deref(),
            Some("com.google.gson.JsonParseException: Expected a JSON object")
        );
        let netty = "io/netty/handler/codec/DecoderException";
        let d = rt.new_object(netty);
        let short = rt.string("Packet was too short");
        call(
            &mut rt,
            k(netty, "<init>.(Ljava/lang/String;Ljava/lang/Throwable;)V"),
            &[d, short, e],
        );
        assert_eq!(
            call(&mut rt, k(netty, "getCause.()Ljava/lang/Throwable;"), &[d]),
            e
        );
        let game = "net/minecraft/ResourceLocationException";
        let r = rt.new_object(game);
        let odd = rt.string("Non [a-z0-9_.-] character in namespace of location: T:x");
        call(&mut rt, k(game, "<init>.(Ljava/lang/String;)V"), &[r, odd]);
        let m = call(&mut rt, k(game, MESSAGE), &[r]);
        assert_eq!(rt.str_of(m), rt.str_of(odd));
        // A mod's own, over Gson's: its constructor's `super(message)`, then the inherited call.
        rt.supers.insert("t/BadConfigException", gson);
        let mine = rt.new_object("t/BadConfigException");
        call(
            &mut rt,
            k(gson, "<init>.(Ljava/lang/String;)V"),
            &[mine, bad],
        );
        let m = call(&mut rt, k("t/BadConfigException", MESSAGE), &[mine]);
        assert_eq!(text(&mut rt, m).as_deref(), Some("Expected a JSON object"));
        assert!(
            rt.unmapped.is_empty() && rt.thrown.is_none(),
            "{:?}",
            rt.unmapped
        );
        // What the libraries declare them under.
        for (c, class, is) in [
            (netty, "java/lang/RuntimeException", true),
            (
                "io/netty/handler/codec/TooLongFrameException",
                "io/netty/handler/codec/DecoderException",
                true,
            ),
            (
                "io/netty/handler/codec/EncoderException",
                "io/netty/handler/codec/CodecException",
                true,
            ),
            (
                "io/netty/handler/codec/EncoderException",
                "io/netty/handler/codec/DecoderException",
                false,
            ),
            (
                "org/apache/commons/lang3/NotImplementedException",
                "java/lang/UnsupportedOperationException",
                true,
            ),
            (
                "org/apache/commons/lang3/NotImplementedException",
                "java/lang/RuntimeException",
                true,
            ),
        ] {
            assert_eq!(
                crate::api_common::api_is_a(c, class),
                is,
                "{c} instanceof {class}"
            );
        }
        // Names that only look so, and a mod's class, aren't taken for one here.
        for key in [
            "com/mojang/serialization/DataResult$Error.toString.()Ljava/lang/String;",
            "com/mojang/realmsclient/client/RealmsError$CustomError.toString.()Ljava/lang/String;",
            "t/BadConfigException.getMessage.()Ljava/lang/String;",
            "com/google/gson/JsonParseException.getAsString.()Ljava/lang/String;",
        ] {
            assert_eq!(foreign_throwable(&mut rt, key), None, "{key}");
        }
    }

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

        rt.interned.insert("side:client".into(), V::I(1));
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
        // `getRegistry()` supplies the registry the register is for.
        let supplied = call(
            &mut rt,
            "net/neoforged/neoforge/registries/DeferredRegister.getRegistry.()Ljava/util/function/Supplier;",
            &[register],
        );
        assert_eq!(
            call(
                &mut rt,
                "java/util/function/Supplier.get.()Ljava/lang/Object;",
                &[supplied]
            ),
            registry
        );
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

    /// The mods' code calling itself without end through its own methods alone, as JourneyMap's
    /// minimap does when it is first made inside a world: no API call nests in another, but
    /// one is made at every turn, and the one made too deep on the thread's stack throws
    /// `StackOverflowError`. The thread's stack holds, and the error unwinds the turns.
    #[test]
    fn code_that_calls_itself_without_end_gets_a_stack_overflow_error() {
        fn turn(rt: &mut Rt, depth: u32) -> u32 {
            call(rt, "java/lang/Math.abs.(I)I", &[V::I(-1)]);
            if rt.thrown.is_some() {
                return depth;
            }
            std::hint::black_box(turn(rt, depth + 1))
        }
        let run = || {
            let mut rt = Rt::new();
            let depth = turn(&mut rt, 0);
            let thrown = rt.thrown.take().map(|e| rt.class_of(e).to_owned());
            // Back at the top, the API answers again.
            let answer = call(&mut rt, "java/lang/Math.abs.(I)I", &[V::I(-7)]);
            (depth, thrown, answer, rt.thrown.is_none())
        };
        let (depth, thrown, answer, clear) = std::thread::Builder::new()
            .stack_size(4 << 20)
            .spawn(run)
            .unwrap()
            .join()
            .expect("the thread's stack held");
        assert_eq!(thrown.as_deref(), Some("java/lang/StackOverflowError"));
        assert!(depth > 1000, "{depth}");
        assert_eq!((answer, clear), (V::I(7), true));
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

    /// A mixin's own `register` (Veil's, by full name) makes its holder with `createHolder` and
    /// puts it with its supplier in the register's `entries`: registered as any other entry,
    /// listed among the register's holders, and a second put of the same holder answers what
    /// is there (the mod's code throws on that).
    #[test]
    fn a_mixin_registers_by_putting_into_a_deferred_registers_entries() {
        // What generated code does for an object no mod class is: the API call by the method's key.
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            call(rt, key, args)
        }
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        const CREATE: &str = "net/neoforged/neoforge/registries/DeferredRegister.create.(Lnet/minecraft/resources/ResourceKey;Ljava/lang/String;)Lnet/neoforged/neoforge/registries/DeferredRegister;";
        const PUT: &str =
            "java/util/Map.putIfAbsent.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        let (blocks, ns) = (
            get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK"),
            rt.string("veil"),
        );
        let register = call(&mut rt, CREATE, &[blocks, ns]);
        assert_eq!(
            call(
                &mut rt,
                "net/neoforged/neoforge/registries/DeferredRegister.$seenRegisterEvent.()Z",
                &[register]
            ),
            V::I(0)
        );
        let entries = call(
            &mut rt,
            "net/neoforged/neoforge/registries/DeferredRegister.$entries.()Ljava/util/Map;",
            &[register],
        );
        assert_eq!(
            call(
                &mut rt,
                "net/neoforged/neoforge/registries/DeferredRegister.$entries.()Ljava/util/Map;",
                &[register]
            ),
            entries
        );
        let id = rt.string("other:lamp");
        let holder = call(
            &mut rt,
            "net/neoforged/neoforge/registries/DeferredRegister.createHolder.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/neoforged/neoforge/registries/DeferredHolder;",
            &[register, blocks, id],
        );
        let lamp = rt.new_object("test/Lamp");
        let supplier = crate::functions::constant(&mut rt, lamp);
        assert!(call(&mut rt, PUT, &[entries, holder, supplier]).is_null());
        assert_eq!(call(&mut rt, PUT, &[entries, holder, supplier]), supplier);
        assert_eq!(rt.suppliers.len(), 1);
        let held = call(
            &mut rt,
            "net/neoforged/neoforge/registries/DeferredRegister.getEntries.()Ljava/util/Collection;",
            &[register],
        );
        assert_eq!(crate::jutil::elements(&rt, held), [holder]);
        rt.resolve_registrations();
        assert_eq!(rt.registry_entry("block", "other:lamp"), Some(lamp));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
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
        // A mod's block asks its vanilla class (`super.getRenderShape(state)`): a block with an
        // entity is invisible there, a plain block a model.
        let bell = rt.new_object("test/WindBell");
        let planks = state(&mut rt, "minecraft:oak_planks");
        for (class, expected) in [("BaseEntityBlock", "INVISIBLE"), ("Block", "MODEL"), ("ChestBlock", "ENTITYBLOCK_ANIMATED")] {
            let key = rt.static_key(&format!(
                "net/minecraft/world/level/block/{class}.getRenderShape.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/RenderShape;"
            ));
            let shape = call(&mut rt, key, &[bell, planks]);
            assert_eq!(name(&rt, shape), expected, "{class}");
        }
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

    /// One piece of a mod's loading code (a constructor, a listener hearing an event) that has run
    /// past its time is stopped at an API call: the call throws an error instead of answering, and
    /// so does every call the piece makes after it. A piece within its time, and code outside any
    /// piece (the game's hooks), go on; the clock is read once in [`PIECE_CHECK`] calls, and a
    /// piece done gives the clock back to the one around.
    #[test]
    fn a_piece_of_loading_code_past_its_time_is_stopped() {
        let key = "java/lang/Math.abs.(I)I";
        let mut rt = Rt::new();
        let next_checks = || CALLS.set(PIECE_CHECK - 1);
        let long_ago = std::time::Instant::now()
            .checked_sub(PIECE_TIME)
            .expect("a clock older than the limit");
        // No piece runs: nothing is timed.
        PIECE.set(None);
        next_checks();
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::I(3));
        // A piece within its time.
        let outer = timed_start();
        next_checks();
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::I(3));
        // The same piece, started long ago: between checks it goes on, at one it is stopped.
        PIECE.set(Some(Piece {
            since: long_ago,
            limit: PIECE_TIME,
            stopped: false,
        }));
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::I(3));
        assert!(!timed_out());
        next_checks();
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::Void);
        let e = rt.thrown.take().expect("stopped");
        // Not an exception, and of the errors the one that code sorting them lets go.
        assert_eq!(rt.class_of(e), "java/lang/StackOverflowError");
        assert!(!rt.instance_of(e, "java/lang/Exception") && rt.instance_of(e, "java/lang/Error"));
        assert!(
            rt.str_of(rt.get_field(e, "message"))
                .is_some_and(|m| m.contains("was stopped"))
        );
        // Whatever caught that, the piece's next call is stopped too, with no check waited for.
        assert!(timed_out());
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::Void);
        assert!(rt.thrown.take().is_some());
        // A piece with less time is stopped when that is over.
        timed_start_within(std::time::Duration::ZERO);
        next_checks();
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::Void);
        assert!(rt.thrown.take().is_some());
        // Done: the clock is the outer piece's again (none here).
        timed_end(outer);
        next_checks();
        assert_eq!(call(&mut rt, key, &[V::I(-3)]), V::I(3));
        assert!(rt.thrown.is_none());
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
        assert!(rt.thrown.is_none(), "loading that goes on is not stopped");
        // A loop on the same few keys while the mods load is.
        let mut run = Run {
            keys: vec![key],
            calls: RUNAWAY_MISSES - 1,
            since: Some(long_ago()),
            allocated: None,
        };
        assert!(run.stuck(key, (0, 0)) && run.calls == 0);
        let mut run = Run {
            keys: vec![key],
            calls: RUNAWAY_MISSES - 1,
            since: Some(std::time::Instant::now()),
            allocated: None,
        };
        assert!(
            !run.stuck(key, (0, 0)),
            "a million calls in no time are not a stuck loop"
        );
        let mut run = Run {
            keys: (0..RUN_KEYS).map(|_| "other").collect(),
            calls: RUNAWAY_MISSES - 1,
            since: Some(long_ago()),
            allocated: None,
        };
        assert!(
            !run.stuck(key, (0, 0)) && run.calls == 1,
            "a new key past the loop's few starts the count again"
        );
        // One that made eight million objects meanwhile is too.
        let mut run = Run {
            keys: vec![key],
            calls: RUNAWAY_FEW - 1,
            since: Some(std::time::Instant::now()),
            allocated: Some((0, 10)),
        };
        assert!(run.stuck(key, (0, 10 + RUNAWAY_OBJECTS)));
        // And one that has made a single text of a quarter of a gigabyte meanwhile (a string
        // doubled each round), after far fewer calls and at once.
        let mut run = Run {
            keys: vec![key],
            calls: RUNAWAY_FEW - 1,
            since: Some(std::time::Instant::now()),
            allocated: Some((5, 0)),
        };
        assert!(run.stuck(key, (RUNAWAY_BYTES, 0)) && run.calls == 0);
        let mut run = Run {
            keys: vec![key],
            calls: RUNAWAY_FEW - 1,
            since: Some(std::time::Instant::now()),
            allocated: Some((5, 0)),
        };
        assert!(
            !run.stuck(key, (RUNAWAY_BYTES - 1, RUNAWAY_OBJECTS - 1)),
            "a thousand calls that made nothing so large are loading going on"
        );
        LOADING.with(|run| {
            *run.borrow_mut() = Run {
                keys: vec![key],
                calls: RUNAWAY_MISSES - 1,
                since: Some(long_ago()),
                allocated: None,
            }
        });
        call(&mut rt, key, &[]);
        let e = rt
            .thrown
            .take()
            .expect("a constructor stuck on an unmapped call is stopped");
        assert_eq!(rt.class_of(e), "java/lang/NoSuchMethodError");
        let mut game = Game;
        rt.with_host(&mut game, |rt| {
            call(rt, key, &[]);
            assert!(rt.thrown.is_none(), "a hook's first unmapped calls answer their stand-in");
            rt.hook_misses = RUNAWAY_MISSES - 1;
            rt.hook_first_miss = Some(long_ago());
            call(rt, key, &[]);
            let e = rt.thrown.take().expect("a hook stuck on an unmapped call is stopped");
            // As a method that is not there: an error of linkage, and an `Error`.
            assert_eq!(rt.class_of(e), "java/lang/NoSuchMethodError");
            assert!(
                rt.instance_of(e, "java/lang/LinkageError")
                    && rt.instance_of(e, "java/lang/Error")
                    && rt.instance_of(e, "java/lang/Throwable")
            );
            assert!(!rt.instance_of(e, "java/lang/Exception"));
            assert!(rt.str_of(rt.get_field(e, "message")).is_some_and(|m| m.contains(key)));
        });
        rt.with_host(&mut game, |rt| {
            call(rt, key, &[]);
            assert!(rt.thrown.is_none(), "the next hook starts afresh");
            // Back in the loop it was stopped in, it is stopped without the wait.
            rt.hook_misses = RUNAWAY_AGAIN - 1;
            rt.hook_first_miss = Some(std::time::Instant::now());
            call(rt, key, &[]);
            let e = rt
                .thrown
                .take()
                .expect("a hook back in the loop it was stopped in is stopped at once");
            assert_eq!(rt.class_of(e), "java/lang/NoSuchMethodError");
        });
        rt.with_host(&mut game, |rt| {
            rt.hook_misses = RUNAWAY_AGAIN - 1;
            rt.hook_first_miss = Some(std::time::Instant::now());
            call(rt, "horizon/test/Other.never.()V", &[]);
            assert!(
                rt.thrown.is_none(),
                "ten thousand calls of what no loop was stopped on are a hook going on"
            );
        });
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
