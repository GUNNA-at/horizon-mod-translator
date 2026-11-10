//! A mark-and-sweep collector for the runtime's heap. Objects reachable from the runtime's roots
//! stay; the rest are freed, and their slots are reused by later allocations (references are heap
//! indices, so live objects never move).
//!
//! The whole collection runs only at a safe point: when a hook enters translated code from the
//! game and no translated frame is running (generated code keeps values in Rust locals the
//! collector cannot see). Anything that holds a value across hook calls keeps it in a root: the
//! runtime's statics, registries, interned values and strings, listeners, block entities, pending
//! suppliers, or [`Rt::pinned`].
//!
//! Inside a hook only the objects of a call in progress are collected ([`Rt::collect_young`]):
//! those made since the runtime's own code last called into the mods' ([`Rt::entered`]). What was
//! there before is kept whole, so whatever the runtime's code above holds in its locals stays;
//! and of the newer objects, nothing but the mods' own frames, the older objects and the roots
//! can hold one. When the call ends there are no such frames left, and the collection is exact
//! (what it returns is a root). While it runs, its frames are read as they lie on the stack: any
//! word that could be a reference to one of the call's objects keeps that object. One pack's
//! reload listener made 43 GB of garbage in a single hook before this.
use crate::{Data, LAMBDA, Object, Rt, V};

/// Allocations between collections, at least: below it a collection costs more than it frees.
const MIN_INTERVAL: usize = 200_000;

/// Objects made by a call, at least, before it is collected for itself: below it the look
/// through the heap costs more than it frees.
pub const YOUNG_MIN: u64 = 400_000;

/// [`YOUNG_MIN`], or what `HORIZON_COMPAT_GC_MIN` says instead, and whether that is all a call
/// waits for (it is set): the tests of the collector run the translated fixtures with a
/// collection every few hundred objects, where a value it was not told of shows at once.
pub(crate) fn young_min() -> (u64, bool) {
    static MIN: std::sync::OnceLock<(u64, bool)> = std::sync::OnceLock::new();
    *MIN.get_or_init(|| {
        match std::env::var("HORIZON_COMPAT_GC_MIN")
            .ok()
            .and_then(|n| n.parse().ok())
        {
            Some(n) => (n, true),
            None => (YOUNG_MIN, false),
        }
    })
}

/// A call from the runtime's own code into the mods' that is in progress ([`Rt::entered`]).
#[derive(Clone, Debug)]
pub struct Entry {
    /// [`Object::born`] of the first object made under it: its objects have that or more.
    born: u32,
    /// Where the thread's stack was when it began: the frames of the code it runs are below.
    stack: usize,
    /// [`Rt::native`] as it was.
    native: u32,
    /// [`Rt::made`] when its objects were last collected (when it began, at first).
    examined: u64,
    /// How many of its objects that collection left: as many more are made before the next
    /// (a call that builds something large is not looked through again for each part of it).
    survived: u64,
}

/// What `HORIZON_COMPAT_GC` asks for of the collections inside a hook: `off` for none, `exit`
/// for those at a call's end alone (none reads the stack); every kind otherwise.
fn young_mode() -> u8 {
    static MODE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("HORIZON_COMPAT_GC").as_deref() {
        Ok("off") => 0,
        Ok("exit") => 1,
        _ => 2,
    })
}

/// Whether `HORIZON_COMPAT_GC_CHECK` is set: what a collection inside a hook frees is then kept
/// marked ([`COLLECTED`]) instead of given to the next allocation, and a call that is handed
/// such an object says so. For finding a value the collector was not told of.
pub fn checking() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("HORIZON_COMPAT_GC_CHECK").is_some())
}

/// The class of a slot freed inside a hook while [`checking`].
pub const COLLECTED: &str = "compat:collected";

/// One bit for each slot of the heap.
struct Bits(Vec<u64>);

impl Bits {
    fn new(n: usize) -> Bits {
        Bits(vec![0; n.div_ceil(64)])
    }
    #[inline]
    fn get(&self, i: usize) -> bool {
        self.0.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }
    #[inline]
    fn set(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }
}

/// The references an object's data holds.
#[inline]
fn refs(data: &Data, each: &mut impl FnMut(u32)) {
    let mut one = |v: &V| {
        if let V::R(r) = *v
            && r != 0
        {
            each(r);
        }
    };
    match data {
        Data::Fields(f) => f.values().for_each(&mut one),
        Data::Array(a) | Data::List(a) | Data::Stream(a) | Data::Iter(a, _) => {
            a.iter().for_each(&mut one)
        }
        Data::Map(m) => m.values().for_each(&mut one),
        Data::Table(t) => {
            for (k, v) in &t.entries {
                one(k);
                one(v);
            }
            one(&t.comparator);
            one(&t.strategy);
        }
        Data::Str(_) | Data::Native(..) | Data::Bytes(..) => {}
    }
}

/// The stack pointer here, where this build knows how to read it.
#[inline(always)]
fn stack_pointer() -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        let sp: usize;
        // SAFETY: reads a register.
        unsafe {
            core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags))
        };
        Some(sp)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let sp: usize;
        // SAFETY: reads a register.
        unsafe {
            core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags))
        };
        Some(sp)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        None
    }
}

/// The registers a call leaves as it found them: a caller's value may be in one and nowhere on
/// the stack yet. The general ones, and the floating-point ones a call keeps (their low halves
/// on ARM), though no reference is kept there by any code seen.
#[inline(always)]
fn kept_registers() -> [usize; 40] {
    let mut kept = [0usize; 40];
    #[cfg(target_arch = "aarch64")]
    // SAFETY: stores registers into `kept`, which has room for the twenty.
    unsafe {
        core::arch::asm!(
            "stp x19, x20, [{p}]",
            "stp x21, x22, [{p}, #16]",
            "stp x23, x24, [{p}, #32]",
            "stp x25, x26, [{p}, #48]",
            "stp x27, x28, [{p}, #64]",
            "stp x29, x30, [{p}, #80]",
            "stp d8, d9, [{p}, #96]",
            "stp d10, d11, [{p}, #112]",
            "stp d12, d13, [{p}, #128]",
            "stp d14, d15, [{p}, #144]",
            p = in(reg) kept.as_mut_ptr(),
            options(nostack, preserves_flags)
        );
    }
    #[cfg(target_arch = "x86_64")]
    // SAFETY: stores registers into `kept`, which has room for them.
    unsafe {
        core::arch::asm!(
            "mov [{p}], rbx",
            "mov [{p} + 8], rbp",
            "mov [{p} + 16], r12",
            "mov [{p} + 24], r13",
            "mov [{p} + 32], r14",
            "mov [{p} + 40], r15",
            "mov [{p} + 48], rdi",
            "mov [{p} + 56], rsi",
            p = in(reg) kept.as_mut_ptr(),
            options(nostack, preserves_flags)
        );
        #[cfg(windows)]
        core::arch::asm!(
            "movups [{p} + 64], xmm6",
            "movups [{p} + 80], xmm7",
            "movups [{p} + 96], xmm8",
            "movups [{p} + 112], xmm9",
            "movups [{p} + 128], xmm10",
            "movups [{p} + 144], xmm11",
            "movups [{p} + 160], xmm12",
            "movups [{p} + 176], xmm13",
            "movups [{p} + 192], xmm14",
            "movups [{p} + 208], xmm15",
            p = in(reg) kept.as_mut_ptr(),
            options(nostack, preserves_flags)
        );
    }
    kept
}

/// Every word of this thread's stack from here up to `top`, and of the registers a call keeps,
/// each half of it as a number a reference could be. False where the stack cannot be read
/// (another kind of machine, or a `top` that is not above): nothing may be freed then.
#[inline(never)]
fn stack_words(top: usize, each: &mut dyn FnMut(u32)) -> bool {
    let kept = kept_registers();
    let Some(sp) = stack_pointer() else {
        return false;
    };
    if top <= sp || top - sp > 1 << 30 {
        return false;
    }
    let mut at = (sp + 3) & !3;
    while at + 4 <= top {
        // SAFETY: between this frame and one above it on the same stack; read as bytes of
        // memory, whatever they are.
        each(unsafe { std::ptr::read_volatile(at as *const u32) });
        at += 4;
    }
    for r in std::hint::black_box(kept) {
        each(r as u32);
        each((r as u64 >> 32) as u32);
    }
    true
}

impl Rt {
    /// [`Object::born`] of the next object made: 1 and up (0 is an object that was there at
    /// the last whole collection), and no further than the count goes.
    #[inline]
    pub(crate) fn next_born(&self) -> u32 {
        (self.made - self.born_base + 1).min(u64::from(u32::MAX)) as u32
    }

    /// Runs `f`, a call from the runtime's own code into the mods' (a method of theirs, a
    /// lambda, a constructor through reflection), as the entry it is: the objects made until
    /// it ends are the call's own, and when it has made many, those nothing reaches are freed
    /// as it ends. `kept` is the value of its result, which the caller is about to hold.
    #[inline]
    pub fn entered<R>(&mut self, f: impl FnOnce(&mut Rt) -> R, kept: impl FnOnce(&R) -> V) -> R {
        let here = 0u8;
        let stack = std::ptr::from_ref(std::hint::black_box(&here)) as usize;
        // An entry further down the stack than this one is of a call that never came back.
        while self.entries.last().is_some_and(|e| e.stack <= stack) {
            self.entries.pop();
            self.native = self.native.max(1);
        }
        self.entries.push(Entry {
            born: self.next_born(),
            stack,
            native: self.native,
            examined: self.made,
            survived: 0,
        });
        self.native = 0;
        let r = f(self);
        if let Some(e) = self.entries.pop() {
            self.native = e.native;
            if self.made - e.examined >= self.young_min {
                let kept = kept(&r);
                self.ended(&e, kept);
            }
        }
        r
    }

    /// The end of an entry that made many objects: collects them when they are enough for
    /// the heap as it is, and no collection has just looked through them.
    #[cold]
    #[inline(never)]
    fn ended(&mut self, e: &Entry, kept: V) {
        let interval = self.young_interval(e.survived);
        if young_mode() >= 1
            && self.made - e.examined >= interval
            && self.made - self.young_last >= interval / 4
        {
            self.collect_young(e.born, &[kept], None, "a call's end");
        }
    }

    /// The end of a call from the mods' code into the runtime's own (`made` objects had been
    /// made when it began): what it made, and what the mods' code it called made, is held by
    /// nothing but its result, the older objects and the roots, as at an entry's end. A
    /// `forEach` over thousands of elements leaves its garbage here.
    #[cold]
    #[inline(never)]
    pub fn call_ended(&mut self, made: u64, result: V) {
        let interval = self.young_interval(0);
        if young_mode() >= 1
            && self.made - made >= interval
            && self.made - self.young_last >= interval / 4
            && made >= self.born_base
        {
            let born = (made - self.born_base + 1).min(u64::from(u32::MAX)) as u32;
            self.collect_young(born, &[result], None, "the end of a call into the runtime");
        }
    }

    /// Objects a call makes before it is collected for itself: [`YOUNG_MIN`]; as many as its
    /// last collection left (`survived`); and an eighth of what the last whole collection
    /// left, so that looking through a large heap is paid for by what was made since. Not by
    /// what the heap holds now: that is the garbage too, and a call that made much of it
    /// would wait the longer for it.
    fn young_interval(&self, survived: u64) -> u64 {
        match young_min() {
            (min, true) => min,
            (min, false) => min.max(survived).max(self.live_at_collection / 8),
        }
    }

    /// A call from the mods' code into the runtime's, with none of the runtime's own in
    /// progress under the innermost entry: collects that entry's objects when it has made
    /// enough of them, reading the mods' frames from the stack for what they hold.
    #[cold]
    #[inline(never)]
    pub fn young_point(&mut self) {
        let Some(e) = self.entries.last() else {
            self.young_due = self.made + self.young_interval(0);
            return;
        };
        let interval = self.young_interval(e.survived);
        let (born, top, since) = (e.born, e.stack, self.made - e.examined);
        if young_mode() >= 2 && since >= interval && self.made - self.young_last >= interval / 4 {
            let (before, freed) = (
                self.young_runs.3,
                self.collect_young(born, &[], Some(top), "a call"),
            );
            let (made, survived) = (
                self.made,
                (self.young_runs.3 - before).saturating_sub(freed as u64),
            );
            if let Some(e) = self.entries.last_mut() {
                e.examined = made;
                e.survived = survived;
            }
            self.young_due = made + self.young_interval(survived);
        } else {
            self.young_due = self.made + (interval - since.min(interval)).max(interval / 8);
        }
    }

    /// While [`checking`]: says so when a call is handed an object a collection inside a hook
    /// freed.
    #[cold]
    #[inline(never)]
    pub fn check_collected(&self, key: &str, args: &[V]) {
        for (i, v) in args.iter().enumerate() {
            if let V::R(r) = *v
                && self
                    .heap
                    .get(r as usize)
                    .is_some_and(|o| o.class == COLLECTED)
            {
                eprintln!(
                    "horizon-compat: GC CHECK: {key} was handed a collected object (argument {i}, object {r})"
                );
            }
        }
    }

    /// Frees the objects born at `born` or later that nothing reaches: not the older objects
    /// (each of them is kept, and what it refers to), not the roots, not `roots`, and, with
    /// `stack_top`, not any word of the stack from here up to there. Returns how many it freed.
    /// Those objects are a call's own ([`Rt::entered`]): `stack_top` is where that call began,
    /// and is left out when the call has ended and none of its frames is left.
    pub fn collect_young(
        &mut self,
        born: u32,
        roots: &[V],
        stack_top: Option<usize>,
        when: &str,
    ) -> usize {
        // Past what `born` counts nothing tells one object from an older one: no more of these
        // until the next whole collection.
        if born == 0 || self.made - self.born_base >= u64::from(u32::MAX) - 1 {
            return 0;
        }
        let started = std::time::Instant::now();
        let n = self.heap.len();
        // Several threads read a large heap (nothing is written to it meanwhile).
        let threads = match n {
            0..400_000 => 1,
            _ => std::thread::available_parallelism()
                .map_or(4, |p| p.get())
                .clamp(1, 8),
        };
        let mut young = Bits::new(n);
        let count: usize = {
            let heap = &self.heap;
            let per = young.0.len().div_ceil(threads).max(1);
            // Each takes whole words of the bits, and the slots those stand for.
            let fill = |part: usize, words: &mut [u64]| {
                let mut count = 0;
                for (w, word) in words.iter_mut().enumerate() {
                    let from = (part * per + w) * 64;
                    for (bit, o) in heap[from..(from + 64).min(n)].iter().enumerate() {
                        if o.born >= born && from + bit != 0 {
                            *word |= 1 << bit;
                            count += 1;
                        }
                    }
                }
                count
            };
            if threads == 1 {
                fill(0, &mut young.0)
            } else {
                let fill = &fill;
                std::thread::scope(|s| {
                    let parts: Vec<_> = young
                        .0
                        .chunks_mut(per)
                        .enumerate()
                        .map(|(part, words)| s.spawn(move || fill(part, words)))
                        .collect();
                    parts.into_iter().map(|p| p.join().unwrap_or(0)).sum()
                })
            }
        };
        self.young_last = self.made;
        self.young_runs.3 += count as u64;
        if count == 0 {
            return 0;
        }
        let mut todo: Vec<u32> = Vec::new();
        // What the stack holds, first: where it cannot be read, nothing is freed.
        if let Some(top) = stack_top
            && !stack_words(top, &mut |w| {
                if young.get(w as usize) {
                    todo.push(w);
                }
            })
        {
            return 0;
        }
        // The older objects, every one: those of them that refer to a young object. Read
        // by several threads where the heap is large (nothing is written meanwhile).
        {
            let (heap, young) = (&self.heap, &young);
            let scan = |from: usize, to: usize| {
                let mut found: Vec<u32> = Vec::new();
                for o in &heap[from..to] {
                    if o.born < born {
                        refs(&o.data, &mut |r| {
                            if young.get(r as usize) {
                                found.push(r);
                            }
                        });
                    }
                }
                found
            };
            if threads == 1 {
                todo.extend(scan(1, n));
            } else {
                let scan = &scan;
                let found: Vec<Vec<u32>> = std::thread::scope(|s| {
                    let parts: Vec<_> = (0..threads)
                        .map(|t| {
                            let (from, to) =
                                (1 + (n - 1) * t / threads, 1 + (n - 1) * (t + 1) / threads);
                            s.spawn(move || scan(from, to))
                        })
                        .collect();
                    parts
                        .into_iter()
                        .map(|p| p.join().unwrap_or_default())
                        .collect()
                });
                todo.extend(found.into_iter().flatten());
            }
        }
        let young_ref = |v: V| match v {
            V::R(r) if young.get(r as usize) => Some(r),
            _ => None,
        };
        for (o, fields) in &self.side_fields {
            if !young.get(*o as usize) {
                todo.extend(fields.values().filter_map(|v| young_ref(*v)));
            }
        }
        self.roots(&mut |v| todo.extend(young_ref(v)));
        todo.extend(roots.iter().filter_map(|v| young_ref(*v)));
        // Mark through the young objects alone.
        let mut marked = Bits::new(n);
        loop {
            while let Some(r) = todo.pop() {
                let i = r as usize;
                if !young.get(i) || marked.get(i) {
                    continue;
                }
                marked.set(i);
                refs(&self.heap[i].data, &mut |x| {
                    if young.get(x as usize) && !marked.get(x as usize) {
                        todo.push(x);
                    }
                });
                if let Some(fields) = self.side_fields.get(&r) {
                    todo.extend(
                        fields
                            .values()
                            .filter_map(|v| young_ref(*v))
                            .filter(|x| !marked.get(*x as usize)),
                    );
                }
            }
            // An iterator's source stays while the iterator does: an older iterator may be
            // in use, a young one is when it is marked.
            for (&it, &src) in &self.iter_sources {
                if (!young.get(it as usize) || marked.get(it as usize))
                    && let Some(s) = young_ref(src)
                    && !marked.get(s as usize)
                {
                    todo.push(s);
                }
            }
            if todo.is_empty() {
                break;
            }
        }
        let gone = |r: u32| young.get(r as usize) && !marked.get(r as usize);
        let class = if checking() { COLLECTED } else { FREE };
        let mut freed = 0usize;
        for (w, (y, m)) in young.0.iter().zip(&marked.0).enumerate() {
            let mut dead = y & !m;
            while dead != 0 {
                let i = w * 64 + dead.trailing_zeros() as usize;
                dead &= dead - 1;
                self.heap[i] = Object {
                    class,
                    cid: 0,
                    born: 0,
                    data: Data::Native(FREE, [0; 4]),
                };
                if !checking() {
                    self.free.push(i as u32);
                }
                freed += 1;
            }
        }
        if freed > 0 {
            self.iter_sources.retain(|it, _| !gone(*it));
            self.side_fields.retain(|o, _| !gone(*o));
            self.lambda_info.retain(|l, _| !gone(*l));
        }
        match stack_top {
            Some(_) => self.young_runs.0 += 1,
            None => self.young_runs.1 += 1,
        }
        self.young_runs.2 += freed as u64;
        if Rt::tracing() {
            thread_local!(static TOLD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
            let told = TOLD.with(|t| t.replace(t.get() + 1));
            if told < 200 || told.is_multiple_of(50) {
                eprintln!(
                    "horizon-compat: collected {freed} of {count} objects at {when}, {n} in the heap ({} ms)",
                    started.elapsed().as_millis()
                );
            }
        }
        freed
    }

    /// The runtime's roots, each: what holds a value across hooks.
    fn roots(&self, each: &mut dyn FnMut(V)) {
        for &r in self.strings.values() {
            each(V::R(r));
        }
        for &v in self
            .statics
            .values()
            .chain(self.registered.values())
            .chain(self.interned.values())
            .chain(self.block_entities.values())
            .chain(
                self.parked_block_entities
                    .values()
                    .flat_map(|(live, _)| live.values()),
            )
            .chain(self.pinned.iter())
        {
            each(v);
        }
        for (_, v) in &self.listeners {
            each(*v);
        }
        for (_, _, v) in &self.suppliers {
            each(*v);
        }
        if let Some(e) = self.thrown {
            each(e);
        }
    }

    /// Collects when enough was allocated since the last collection. Call only at a safe point.
    pub fn maybe_collect(&mut self) {
        let live = self.heap.len() - self.free.len();
        if live >= self.collect_at {
            let survivors = self.collect();
            self.collect_at = (survivors * 2).max(survivors + MIN_INTERVAL);
        }
    }

    /// Frees every object the roots don't reach; returns how many objects stay. Call only at a safe
    /// point (no translated frame running).
    pub fn collect(&mut self) -> usize {
        let n = self.heap.len();
        // A call into the mods' code is in progress: its frames hold values no root names.
        if !self.entries.is_empty() {
            return n - self.free.len();
        }
        let mut marked = vec![false; n];
        let mut todo: Vec<u32> = Vec::new();
        self.roots(&mut |v| {
            if let V::R(r) = v
                && r != 0
            {
                todo.push(r);
            }
        });
        self.mark(&mut marked, &mut todo);
        // An iterator's source stays while the iterator does (weak keys, as an ephemeron).
        // Only sources not marked yet: pushing marked ones again would never settle.
        loop {
            for (&it, &src) in &self.iter_sources {
                if marked.get(it as usize).copied().unwrap_or(false)
                    && let V::R(s) = src
                    && s != 0
                    && !marked.get(s as usize).copied().unwrap_or(true)
                {
                    todo.push(s);
                }
            }
            if todo.is_empty() {
                break;
            }
            self.mark(&mut marked, &mut todo);
        }
        self.iter_sources.retain(|it, _| marked.get(*it as usize).copied().unwrap_or(false));
        self.side_fields.retain(|o, _| marked.get(*o as usize).copied().unwrap_or(false));
        self.lambda_info.retain(|l, _| marked.get(*l as usize).copied().unwrap_or(false));
        let mut survivors = 1;
        for (r, live) in marked.iter().enumerate().skip(1) {
            if *live {
                survivors += 1;
                // Every object from here on is younger than these.
                self.heap[r].born = 0;
            } else if self.heap[r].class != FREE {
                self.heap[r] = Object {
                    class: FREE,
                    cid: 0,
                    born: 0,
                    data: Data::Native(FREE, [0; 4]),
                };
                self.free.push(r as u32);
            }
        }
        self.born_base = self.made;
        self.young_last = self.made;
        self.young_due = self.made + young_min().0;
        self.live_at_collection = survivors as u64;
        survivors
    }

    fn mark(&self, marked: &mut [bool], todo: &mut Vec<u32>) {
        while let Some(r) = todo.pop() {
            let Some(seen) = marked.get_mut(r as usize) else { continue };
            if *seen {
                continue;
            }
            *seen = true;
            let refs = |v: &V, todo: &mut Vec<u32>| {
                if let V::R(x) = *v
                    && x != 0
                {
                    todo.push(x);
                }
            };
            match &self.heap[r as usize].data {
                Data::Fields(f) => f.values().for_each(|v| refs(v, todo)),
                Data::Array(a) | Data::List(a) | Data::Stream(a) | Data::Iter(a, _) => a.iter().for_each(|v| refs(v, todo)),
                Data::Map(m) => m.values().for_each(|v| refs(v, todo)),
                Data::Table(t) => {
                    for (k, v) in &t.entries {
                        refs(k, todo);
                        refs(v, todo);
                    }
                    refs(&t.comparator, todo);
                    refs(&t.strategy, todo);
                }
                Data::Str(_) | Data::Native(..) | Data::Bytes(..) => {}
            }
            if let Some(f) = self.side_fields.get(&r) {
                f.values().for_each(|v| refs(v, todo));
            }
        }
    }
}

/// The class of a freed slot.
pub const FREE: &str = "compat:free";

/// Whether `cid` is a lambda's (kept here so the collector's users need not know the bit).
pub fn is_lambda(cid: u32) -> bool {
    cid & LAMBDA != 0
}

#[cfg(test)]
mod tests {
    use crate::{Data, Rt, V};

    /// A mod's `LinkedList` subclass is the list and keeps its own field beside it, which holds
    /// what it refers to as long as the list stays, and goes with it.
    #[test]
    fn a_collection_keeps_fields_of_its_own_while_it_lives() {
        let mut rt = Rt::new();
        let list = rt.alloc("mod/UniqueLinkedList", Data::List(Vec::new()));
        let seen = rt.alloc("java/util/HashSet", Data::Table(Default::default()));
        rt.put_field(list, "contained", seen);
        assert_eq!(rt.get_field(list, "contained"), seen);
        rt.statics.insert("mod/Search.FRONTIER", list);
        rt.collect();
        assert_eq!(rt.get_field(list, "contained"), seen);
        assert_eq!(rt.class_of(seen), "java/util/HashSet");
        rt.statics.clear();
        rt.collect();
        assert!(rt.side_fields.is_empty());
        assert_eq!(rt.class_of(seen), super::FREE);
    }

    use super::{Entry, YOUNG_MIN};

    /// What a call from the runtime's own code into the mods' made is collected as the call
    /// ends: its garbage goes; what its result holds, what an older object was given, what a
    /// root names and the source of an iterator that stays all stay.
    #[test]
    fn a_calls_garbage_goes_as_it_ends() {
        let mut rt = Rt::new();
        let old = rt.new_object("t/Old");
        let mut junk = V::NULL;
        let mut source = V::NULL;
        let result = rt.entered(
            |rt| {
                let given = rt.new_object("t/GivenToOld");
                rt.put_field(old, "held", given);
                let result = rt.new_object("t/Result");
                let inner = rt.new_object("t/Inner");
                rt.put_field(result, "inner", inner);
                source = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
                let it = rt.new_object("java/util/Iterator");
                rt.iter_sources.insert(it.r(), source);
                rt.put_field(result, "it", it);
                let named = rt.new_object("t/Named");
                rt.statics.insert("t/S.NAMED", named);
                for _ in 0..YOUNG_MIN + 10 {
                    junk = rt.new_object("t/Junk");
                    let lost = rt.new_object("java/util/Iterator");
                    rt.iter_sources.insert(lost.r(), junk);
                }
                result
            },
            |r| *r,
        );
        assert_eq!(rt.class_of(junk), super::FREE);
        assert!(
            rt.free.len() as u64 >= 2 * YOUNG_MIN,
            "{} slots are free",
            rt.free.len()
        );
        assert_eq!(rt.class_of(rt.get_field(old, "held")), "t/GivenToOld");
        assert_eq!(rt.class_of(rt.get_field(result, "inner")), "t/Inner");
        assert_eq!(rt.class_of(source), "java/util/ArrayList");
        assert_eq!(rt.class_of(rt.statics["t/S.NAMED"]), "t/Named");
        assert_eq!(
            rt.iter_sources.len(),
            1,
            "the lost iterators' notes went with them"
        );
        assert!(rt.entries.is_empty());
        assert_eq!(rt.native, 1);
    }

    /// A call from the mods' code into the runtime's own leaves, as it ends, only its result
    /// of all it made (a `forEach` and the garbage of every element's turn).
    #[test]
    fn what_a_call_into_the_runtime_made_goes_as_it_ends() {
        let mut rt = Rt::new();
        let before = rt.new_object("t/Before");
        let made = rt.made;
        let result = rt.new_object("t/Result");
        let inner = rt.new_object("t/Inner");
        rt.put_field(result, "inner", inner);
        for _ in 0..YOUNG_MIN + 10 {
            rt.new_object("t/Junk");
        }
        rt.call_ended(made, result);
        assert!(
            rt.free.len() as u64 >= YOUNG_MIN,
            "{} slots are free",
            rt.free.len()
        );
        assert_eq!(rt.class_of(rt.get_field(result, "inner")), "t/Inner");
        assert_eq!(rt.class_of(before), "t/Before");
        assert_eq!(rt.young_runs.1, 1);
    }

    /// A frame as the mods' code has them: it holds an object in a local while it makes
    /// garbage, then calls into the runtime, where a collection reads the stack.
    #[inline(never)]
    fn a_frame_of_the_mods(rt: &mut Rt) -> V {
        let held = rt.new_object("t/Held");
        for _ in 0..YOUNG_MIN + 10 {
            rt.new_object("t/Junk");
        }
        rt.young_due = 0;
        if rt.native == 0 && rt.made >= rt.young_due {
            rt.young_point();
        }
        std::hint::black_box(held)
    }

    /// While a call into the mods' code goes on, its garbage is collected at a call it makes
    /// into the runtime: what one of its frames holds in a local stays, by the stack. With no
    /// stack read (a call's end, where no frame is left) the same object is garbage.
    #[test]
    fn a_collection_inside_a_call_keeps_what_its_frames_hold() {
        let mut rt = Rt::new();
        let born = rt.next_born();
        let held = rt.entered(a_frame_of_the_mods, |r| *r);
        assert_eq!(rt.class_of(held), "t/Held");
        // (A number on the stack that could be a reference keeps the object it would name:
        // a few of the garbage stay for that.)
        assert!(
            rt.free.len() as u64 >= YOUNG_MIN - 1000,
            "{} slots are free",
            rt.free.len()
        );
        // (Its result was not collected again as it ended: that had just been done.)
        rt.collect_young(born, &[], None, "a test");
        assert_eq!(rt.class_of(held), super::FREE);
    }

    /// An object that was there at the last whole collection is older than every call after
    /// it: a call's collection leaves it, named by nothing as it is, to the next whole one.
    #[test]
    fn a_calls_collection_leaves_the_older_objects() {
        let mut rt = Rt::new();
        let before = rt.new_object("t/Before");
        rt.pinned.push(before);
        rt.collect();
        rt.pinned.clear();
        assert_eq!(rt.heap[before.r() as usize].born, 0);
        rt.entered(
            |rt| {
                for _ in 0..YOUNG_MIN + 10 {
                    rt.new_object("t/Junk");
                }
                V::NULL
            },
            |r| *r,
        );
        assert!(rt.free.len() as u64 >= YOUNG_MIN);
        assert_eq!(rt.class_of(before), "t/Before");
        rt.collect();
        assert_eq!(rt.class_of(before), super::FREE);
    }

    /// An entry left by a call that never came back (it panicked) is further down the stack
    /// than any call after it: the next entry drops it, and the runtime's own code is what
    /// runs again.
    #[test]
    fn an_entry_that_never_ended_is_dropped() {
        let mut rt = Rt::new();
        rt.entries.push(Entry {
            born: 1,
            stack: 16,
            native: 1,
            examined: 0,
            survived: 0,
        });
        rt.native = 0;
        rt.entered(
            |rt| {
                assert_eq!(rt.entries.len(), 1);
                assert_eq!(rt.native, 0);
            },
            |_| V::NULL,
        );
        assert!(rt.entries.is_empty());
        assert_eq!(rt.native, 1);
        // And a whole collection waits while a call is in progress.
        let junk = rt.new_object("t/Junk");
        rt.entered(
            |rt| {
                rt.collect();
            },
            |_| V::NULL,
        );
        assert_eq!(rt.class_of(junk), "t/Junk");
        rt.collect();
        assert_eq!(rt.class_of(junk), super::FREE);
    }

    #[test]
    fn unreachable_objects_are_freed_and_their_slots_reused() {
        let mut rt = Rt::new();
        // Reachable: a static holding an object that holds another, and a registered object.
        let inner = rt.new_object("t/Inner");
        let outer = rt.new_object("t/Outer");
        rt.put_field(outer, "next", inner);
        rt.statics.insert("t/Outer.ROOT", outer);
        let kept = rt.alloc("[I", Data::Array(vec![V::I(7)]));
        rt.registered.insert(("block".into(), "t:kept".into()), kept);
        // Unreachable: a cycle and a list of garbage.
        let a = rt.new_object("t/A");
        let b = rt.new_object("t/B");
        rt.put_field(a, "b", b);
        rt.put_field(b, "a", a);
        let junk = rt.new_object("t/Junk");
        let list = rt.alloc("java/util/ArrayList", Data::List(vec![junk]));
        let before = rt.heap.len();
        let survivors = rt.collect();
        assert_eq!(survivors, before - 4);
        assert_eq!(rt.class_of(rt.get_field(outer, "next")), "t/Inner");
        assert_eq!(rt.class_of(kept), "[I");
        for gone in [a, b, junk, list] {
            assert_eq!(rt.class_of(gone), super::FREE);
        }
        // New objects take the freed slots; the heap doesn't grow.
        let fresh: Vec<V> = (0..4).map(|_| rt.new_object("t/Fresh")).collect();
        assert_eq!(rt.heap.len(), before);
        assert!(fresh.iter().all(|v| [a, b, junk, list].contains(v)));
    }

    #[test]
    fn a_live_iterator_keeps_its_source_and_the_collection_ends() {
        let mut rt = Rt::new();
        let item = rt.new_object("t/Item");
        let source = rt.alloc("java/util/ArrayList", Data::List(vec![item]));
        let it = rt.new_object("java/util/Iterator");
        rt.iter_sources.insert(it.r(), source);
        // The iterator is held, its source is reachable only through it; a second iterator over
        // the same source (marked already) must not keep the collector going.
        let it2 = rt.new_object("java/util/Iterator");
        rt.iter_sources.insert(it2.r(), source);
        rt.pinned.extend([it, it2]);
        rt.collect();
        assert_eq!(rt.class_of(source), "java/util/ArrayList");
        assert_eq!(rt.class_of(item), "t/Item");
    }
}
