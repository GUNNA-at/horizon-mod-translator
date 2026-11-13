//! Threads of the mods' own.
//!
//! One runtime runs on one thread of the game's, and a mod's code counts on more: a thread it
//! starts, a pool it gives work to. Run on the game's thread, such work held the game for as
//! long as it took (a map mod draws its map in a pool, 82 seconds a piece) and could not wait
//! for anything, since the thread it waited on was the one it ran on.
//!
//! Here a thread of the mods' is a thread of the machine's with a stack of its own, and still
//! only one of them runs the runtime at a time: the game's thread lends it ([`Rt::run_threads`]),
//! with the game it is in, for a slice; the thread that takes it runs until it waits for
//! something ([`Rt::thread_wait`]), until its slice is over ([`Rt::slice_point`]) or until it
//! ends, and hands it back. So nothing of the runtime is shared between two that run, the game's
//! thread is held for a slice and no longer, and a thread that sleeps, waits for a lock or for
//! work to come does so on its own stack, in the middle of its own code.
//!
//! What a thread is in the middle of goes with it: the exception it may be throwing, the calls
//! from the runtime's code into the mods' it has in progress and what the collector keeps for
//! them ([`Context`]). The collector knows of the threads that wait ([`crate::gc`]): a
//! collection inside a call frees only what was made since the thread that runs took the
//! runtime, which no other can hold; a whole one reads the stacks of the waiting threads, and is
//! put off while one of them waits under code of the runtime's own, whose containers no stack
//! shows.
use crate::{Rt, V, gc};
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

/// How long a thread runs before the next has its turn or the game goes on.
pub const SLICE: Duration = Duration::from_millis(3);
/// Calls into the runtime between two looks at the clock by a thread that runs.
const CHECK_EVERY: u64 = 64;
/// How far past its slice a thread runs on to leave a monitor or lock it holds: another thread
/// would find its work half done.
const LOCK_GRACE: Duration = Duration::from_millis(50);
/// How far past its slice a thread runs on to come back from under the runtime's own code: a
/// whole collection waits for every thread to be out of it.
const SHALLOW_GRACE: Duration = Duration::from_millis(2);
/// The calls from the runtime into the mods' code a thread's body is, by itself.
const BODY_ENTRIES: usize = 1;

/// What a piece of the mods' code is in the middle of, apart from the heap: its own while it
/// runs, put away while another thread does.
#[derive(Default)]
struct Context {
    thrown: Option<V>,
    unwinding: Vec<&'static str>,
    entries: Vec<gc::Entry>,
    native: u32,
    scopes: Vec<gc::Scope>,
    scope_kept: Vec<u32>,
    pinned: Vec<V>,
    hook_misses: u64,
    hook_first_miss: Option<Instant>,
    caller_class: &'static str,
    locks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// It runs, or waits for its turn.
    Ready,
    /// It waits for a wake of object `on` (0: none comes), until `until` at the latest.
    Waiting { on: u32, until: Option<Instant> },
    Done,
}

struct ModThread {
    id: u32,
    /// Its `java.lang.Thread`, or what stands for it.
    object: V,
    state: State,
    /// A wake came while it waited.
    woken: bool,
    ctx: Context,
    /// Where its stack lies while it waits in the middle of its code: from the lowest address
    /// in use to where its body began. (0, 0) before it first ran.
    stack: (usize, usize),
}

/// Whose turn it is: 0 the game's thread, else a thread's id; [`QUIT`] when the runtime is gone.
struct Turn {
    runner: u32,
    /// The runtime, while a thread has it.
    rt: usize,
}
const QUIT: u32 = u32::MAX;

struct Baton {
    turn: Mutex<Turn>,
    changed: Condvar,
}

/// The mods' threads of one runtime.
pub struct Threads {
    list: Vec<ModThread>,
    /// The thread that runs: 0 the game's own.
    current: u32,
    next_id: u32,
    /// [`Rt::next_born`] when the thread that runs took the runtime: what was made before may
    /// be held by a thread that waits, where no collection inside a call can see it.
    pub(crate) resumed_born: u32,
    baton: Arc<Baton>,
    /// The game's thread's own context, while a thread of the mods' runs.
    game: Context,
    slice_end: Option<Instant>,
    /// [`Rt::calls`] at which the thread that runs next looks at the clock (never, on the
    /// game's).
    pub(crate) check_at: u64,
    /// The monitors and locks the thread that runs holds.
    pub locks: u32,
    /// The thread that ran last: the next round starts after it.
    cursor: usize,
    /// Whole collections put off because a thread waited under the runtime's own code.
    pub put_off: u64,
    /// How many times the game's thread came to where it lends the runtime: none yet, and no
    /// thread of the mods' would ever run on it.
    pub lends: u64,
    /// How long the mods' threads have run, and how many turns they had.
    pub ran: (Duration, u64),
    /// The game's thread lends from between two pieces of its own work, where it holds no
    /// value of its own ([`Rt::lend_threads`]), and not from the middle of a call that waits.
    clean: bool,
}

impl Default for Threads {
    fn default() -> Threads {
        Threads {
            list: Vec::new(),
            current: 0,
            next_id: 1,
            resumed_born: 0,
            baton: Arc::new(Baton {
                turn: Mutex::new(Turn { runner: 0, rt: 0 }),
                changed: Condvar::new(),
            }),
            game: Context::default(),
            slice_end: None,
            check_at: u64::MAX,
            locks: 0,
            cursor: 0,
            put_off: 0,
            lends: 0,
            ran: (Duration::ZERO, 0),
            clean: false,
        }
    }
}

/// A copy of a runtime ([`Rt::fork`]) has no threads: they are the first one's.
impl Clone for Threads {
    fn clone(&self) -> Threads {
        Threads::default()
    }
}

/// The runtime is gone: its threads never run again. One in the middle of its code stays where
/// it waits (its frames are of code that would use the runtime); one that never began ends.
impl Drop for Threads {
    fn drop(&mut self) {
        let mut turn = self.baton.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        turn.runner = QUIT;
        self.baton.changed.notify_all();
    }
}

impl std::fmt::Debug for Threads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Threads({} of the mods', {} runs)", self.list.len(), self.current)
    }
}

thread_local! {
    /// On a thread of the mods': where the runtime it runs is ([`Rt::address`]); 0 on any other.
    static LENT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Where the runtime this thread runs for is, when it is a thread of the mods' own; 0 on a
/// thread of the game's. The game runs a hook on whatever thread asked it something: on one of
/// these the hook's runtime is the one lent to it, which is in the middle of that call (as
/// [`crate::reenter`] finds it), and never one of the thread's own.
pub fn lent_at() -> usize {
    LENT.with(|l| l.get())
}

/// Hands the runtime at `rt` to thread `id` and waits until it is handed back. Not inlined, and
/// the runtime goes in as a bare address: the caller's own reference is not used while another
/// thread writes there.
#[inline(never)]
fn lend(baton: &Baton, id: u32, rt: *mut Rt) {
    let mut turn = baton.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    turn.runner = id;
    turn.rt = std::hint::black_box(rt) as usize;
    baton.changed.notify_all();
    while turn.runner == id {
        turn = baton.changed.wait(turn).unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

/// Hands the runtime back to the game's thread and waits for the next turn of thread `id`.
/// False when the runtime is gone: there is nothing to come back to.
#[inline(never)]
fn hand_back(baton: &Baton, id: u32, rt: *mut Rt, wait: bool) -> bool {
    std::hint::black_box(rt);
    let mut turn = baton.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if turn.runner == id {
        turn.runner = 0;
        baton.changed.notify_all();
    }
    while wait && turn.runner != id {
        if turn.runner == QUIT {
            return false;
        }
        turn = baton.changed.wait(turn).unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    true
}

/// A thread's own stack: it waits for its first turn, runs its body, and is done.
fn thread_main(baton: Arc<Baton>, id: u32, body: Box<dyn FnOnce(&mut Rt) + Send>) {
    let mark = 0usize;
    let top = std::ptr::from_ref(std::hint::black_box(&mark)) as usize + std::mem::size_of::<usize>();
    let rt = {
        let mut turn = baton.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            match turn.runner {
                QUIT => return,
                r if r == id => break turn.rt as *mut Rt,
                _ => turn = baton.changed.wait(turn).unwrap_or_else(std::sync::PoisonError::into_inner),
            }
        }
    };
    // SAFETY: the game's thread lent the runtime to this thread and waits until it is handed
    // back ([`lend`]); it stays where it is for as long as it has threads.
    let rt: &mut Rt = unsafe { &mut *rt };
    LENT.with(|l| l.set(rt.address()));
    if let Some(t) = rt.threads.list.iter_mut().find(|t| t.id == id) {
        t.stack = (0, top);
    }
    // A panic in its code ends this thread alone: the game's thread, which waits for the
    // runtime, has it back all the same.
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut *rt)));
    if ended.is_err() {
        eprintln!("horizon-compat: a thread of the mods' own (thread {id}) panicked and is done");
    }
    rt.thread_ended();
    let rt: *mut Rt = rt;
    hand_back(&baton, id, rt, false);
}

/// What became of a wait ([`Rt::thread_wait`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waited {
    /// What it waited on was woken ([`Rt::thread_wake`]).
    Woken,
    /// Its time came.
    TimedOut,
    /// It was asked on the game's thread, which waits for nothing: the other threads had a
    /// round instead.
    NotWaited,
}

impl Rt {
    fn take_context(&mut self) -> Context {
        Context {
            thrown: self.thrown.take(),
            unwinding: std::mem::take(&mut self.unwinding),
            entries: std::mem::take(&mut self.entries),
            native: std::mem::replace(&mut self.native, 1),
            scopes: std::mem::take(&mut self.scopes),
            scope_kept: std::mem::take(&mut self.scope_kept),
            pinned: std::mem::take(&mut self.pinned),
            hook_misses: std::mem::take(&mut self.hook_misses),
            hook_first_miss: self.hook_first_miss.take(),
            caller_class: std::mem::take(&mut self.caller_class),
            locks: std::mem::take(&mut self.threads.locks),
        }
    }

    fn put_context(&mut self, c: Context) {
        self.thrown = c.thrown;
        self.unwinding = c.unwinding;
        self.entries = c.entries;
        self.native = c.native;
        self.scopes = c.scopes;
        self.scope_kept = c.scope_kept;
        self.pinned = c.pinned;
        self.hook_misses = c.hook_misses;
        self.hook_first_miss = c.hook_first_miss;
        self.caller_class = c.caller_class;
        self.threads.locks = c.locks;
    }

    /// The id of the mods' thread that runs now; 0 on the game's own.
    #[inline]
    pub fn thread_id(&self) -> u32 {
        self.threads.current
    }

    /// The object of the mods' thread that runs now (its `Thread`); null on the game's own.
    pub fn thread_object(&self) -> V {
        let id = self.threads.current;
        self.threads.list.iter().find(|t| t.id == id).map_or(V::NULL, |t| t.object)
    }

    /// The object of thread `id` (its `Thread`), while it is not done.
    pub fn thread_object_of(&self, id: u32) -> Option<V> {
        self.threads
            .list
            .iter()
            .find(|t| t.id == id && t.state != State::Done)
            .map(|t| t.object)
    }

    /// Whether the mods have threads that are not done.
    #[inline]
    pub fn has_threads(&self) -> bool {
        !self.threads.list.is_empty()
    }

    /// Whether thread `id` is not done yet.
    pub fn thread_alive(&self, id: u32) -> bool {
        self.threads.list.iter().any(|t| t.id == id && t.state != State::Done)
    }

    /// Starts a thread of the mods': `body` runs on a stack of its own, a slice at a time, when
    /// the game's thread lends the runtime ([`Rt::run_threads`]). `object` stands for it (its
    /// `Thread`), and is kept while it lives. Its id; 0 where the machine gives no thread (the
    /// body is dropped unrun).
    pub fn thread_start(&mut self, object: V, body: Box<dyn FnOnce(&mut Rt) + Send>) -> u32 {
        let id = self.threads.next_id;
        let baton = self.threads.baton.clone();
        let made = std::thread::Builder::new()
            .name(format!("mod thread {id}"))
            .stack_size(64 << 20)
            .spawn(move || thread_main(baton, id, body));
        if made.is_err() {
            return 0;
        }
        self.threads.next_id += 1;
        self.threads.list.push(ModThread {
            id,
            object,
            state: State::Ready,
            woken: false,
            ctx: Context {
                native: 1,
                ..Context::default()
            },
            stack: (0, 0),
        });
        id
    }

    /// Lends the runtime to the mods' threads, each that has something to do in its turn, for
    /// `budget` in all and [`SLICE`] each at most. Only the game's thread lends; whether any
    /// thread ran. The game the runtime is in ([`Rt::with_host`]) is theirs meanwhile.
    pub fn run_threads(&mut self, budget: Duration) -> bool {
        if self.threads.current != 0 || self.threads.list.is_empty() {
            return false;
        }
        let deadline = Instant::now() + budget;
        let (mut ran, n) = (false, self.threads.list.len());
        for k in 1..=n {
            let at = (self.threads.cursor + k) % n;
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            if !self.thread_ready(at, now) {
                continue;
            }
            self.switch_to(at, deadline.min(now + SLICE));
            self.threads.cursor = at;
            ran = true;
        }
        self.threads.list.retain(|t| t.state != State::Done);
        ran
    }

    /// [`Rt::run_threads`] from between two pieces of the game thread's own work (a tick ended,
    /// a frame drawn), where that thread holds no value of the runtime's in a frame or a
    /// container of its own: what a thread of the mods' collects meanwhile is then all that
    /// was made since its call began, whoever made it ([`Rt::collect_young`]).
    pub fn lend_threads(&mut self, budget: Duration) -> bool {
        self.threads.clean = true;
        let ran = self.run_threads(budget);
        self.threads.clean = false;
        ran
    }

    /// Whether every value a thread other than the one that runs may hold can be read: the
    /// game's lent from between its own work ([`Rt::lend_threads`]), and none of the mods'
    /// waits under the runtime's own code.
    pub(crate) fn threads_clean(&self) -> bool {
        self.threads.clean && self.threads.game.entries.is_empty() && !self.threads_deep()
    }

    /// A whole collection ran: every object left is as old as any, so each call in progress
    /// on a waiting thread, and each scope, counts what is made from here as its own.
    pub(crate) fn threads_rebased(&mut self) {
        for t in &mut self.threads.list {
            for e in &mut t.ctx.entries {
                e.rebase();
            }
            for s in &mut t.ctx.scopes {
                s.rebase();
            }
        }
    }

    /// Whether the thread at `at` has something to do: its turn, a wake, or its time come.
    fn thread_ready(&mut self, at: usize, now: Instant) -> bool {
        let t = &mut self.threads.list[at];
        match t.state {
            State::Ready => true,
            State::Waiting { until, .. } if t.woken || until.is_some_and(|u| now >= u) => {
                t.state = State::Ready;
                true
            }
            _ => false,
        }
    }

    /// Whether some thread of the mods' has something to do now.
    pub fn threads_ready(&self) -> bool {
        let now = Instant::now();
        self.threads.list.iter().any(|t| match t.state {
            State::Ready => true,
            State::Waiting { until, .. } => t.woken || until.is_some_and(|u| now >= u),
            State::Done => false,
        })
    }

    /// When a thread of the mods' next has something to do: now or before (it is ready, woken,
    /// or its time has come), or at a time one of them waits until. None when none of them goes
    /// on unless something wakes it, which only the thread that runs can do.
    pub fn threads_next(&self) -> Option<Instant> {
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for t in &self.threads.list {
            let at = match t.state {
                State::Ready => now,
                State::Waiting { .. } if t.woken => now,
                State::Waiting { until: Some(u), .. } => u,
                _ => continue,
            };
            next = Some(next.map_or(at, |n| n.min(at)));
        }
        next
    }

    /// How many threads of the mods' are not done.
    pub fn thread_count(&self) -> usize {
        self.threads
            .list
            .iter()
            .filter(|t| t.state != State::Done)
            .count()
    }

    /// The game's thread gives the runtime to the thread at `at` until `slice_end`, and has it
    /// back when that thread waits, is out of time or is done.
    fn switch_to(&mut self, at: usize, slice_end: Instant) {
        self.threads.game = self.take_context();
        let (id, ctx) = {
            let t = &mut self.threads.list[at];
            (t.id, std::mem::take(&mut t.ctx))
        };
        self.put_context(ctx);
        self.threads.current = id;
        self.threads.slice_end = Some(slice_end);
        self.threads.check_at = self.calls + CHECK_EVERY;
        self.threads.resumed_born = self.next_born();
        let baton = self.threads.baton.clone();
        let began = Instant::now();
        let rt: *mut Rt = self;
        lend(&baton, id, rt);
        // The thread put its own context away and this one back before it handed over.
        self.threads.resumed_born = self.next_born();
        self.threads.ran.0 += began.elapsed();
        self.threads.ran.1 += 1;
    }

    /// The thread that runs gives the runtime back to the game's thread, in `state`, and goes on
    /// from here at its next turn. Everything it holds is on its stack from here up, the
    /// registers a call keeps with it.
    #[inline(never)]
    fn thread_park(&mut self, state: State) {
        let id = self.threads.current;
        let Some(at) = self.threads.list.iter().position(|t| t.id == id) else {
            return;
        };
        let kept = gc::kept_registers();
        let sp = gc::stack_pointer().unwrap_or(0);
        let ctx = self.take_context();
        {
            let t = &mut self.threads.list[at];
            t.ctx = ctx;
            t.state = state;
            t.woken = false;
            t.stack.0 = sp;
        }
        let game = std::mem::take(&mut self.threads.game);
        self.put_context(game);
        self.threads.current = 0;
        self.threads.slice_end = None;
        self.threads.check_at = u64::MAX;
        let baton = self.threads.baton.clone();
        let rt: *mut Rt = self;
        if !hand_back(&baton, id, rt, true) {
            // The runtime is gone, and this stack is of code that would use it.
            loop {
                std::thread::park();
            }
        }
        std::hint::black_box(&kept);
    }

    /// The body of the thread that runs has ended: it is done, whoever waits for it is woken,
    /// and the game's thread has its own context back.
    fn thread_ended(&mut self) {
        let id = self.threads.current;
        let object = self.thread_object();
        // What ended it is its own, and told of by whoever ran its body.
        drop(self.take_context());
        if let Some(t) = self.threads.list.iter_mut().find(|t| t.id == id) {
            t.state = State::Done;
            t.stack = (0, 0);
        }
        let game = std::mem::take(&mut self.threads.game);
        self.put_context(game);
        self.threads.current = 0;
        self.threads.slice_end = None;
        self.threads.check_at = u64::MAX;
        self.thread_wake(object, true);
    }

    /// The thread that runs waits: for a wake of `on` ([`Rt::thread_wake`]; null for none),
    /// until `until` at the latest (None: for as long as it takes). The other threads and the
    /// game go on meanwhile. On the game's own thread nothing is waited for: the mods' threads
    /// have a round, as they would have run beside it, and the caller goes on.
    pub fn thread_wait(&mut self, on: V, until: Option<Instant>) -> Waited {
        if self.threads.current == 0 {
            self.run_threads(SLICE);
            return Waited::NotWaited;
        }
        let id = self.threads.current;
        self.thread_park(State::Waiting {
            on: on.r(),
            until,
        });
        match self.threads.list.iter().find(|t| t.id == id) {
            Some(t) if t.woken => Waited::Woken,
            _ => Waited::TimedOut,
        }
    }

    /// Wakes the threads that wait on `on`: the first of them, or with `all` each.
    pub fn thread_wake(&mut self, on: V, all: bool) {
        let on = on.r();
        if on == 0 {
            return;
        }
        for t in &mut self.threads.list {
            if matches!(t.state, State::Waiting { on: o, .. } if o == on) && !t.woken {
                t.woken = true;
                if !all {
                    return;
                }
            }
        }
    }

    /// Wakes thread `id` whatever it waits for (`LockSupport.unpark`, an interrupt).
    pub fn thread_unpark(&mut self, id: u32) {
        if let Some(t) = self.threads.list.iter_mut().find(|t| t.id == id)
            && matches!(t.state, State::Waiting { .. })
        {
            t.woken = true;
        }
    }

    /// The thread that runs lets the others have their turn (`Thread.yield`), and goes on at
    /// its next. Nothing on the game's thread.
    pub fn thread_yield(&mut self) {
        if self.threads.current != 0 {
            self.thread_park(State::Ready);
        }
    }

    /// A call from the mods' code into the runtime, on a thread of the mods' whose turn to look
    /// at the clock it is: past its slice it gives the runtime back. Not while it holds a
    /// monitor or lock, nor under the runtime's own code, for a little longer each.
    #[cold]
    #[inline(never)]
    pub(crate) fn slice_point(&mut self) {
        self.threads.check_at = self.calls + CHECK_EVERY;
        let Some(end) = self.threads.slice_end else {
            return;
        };
        let now = Instant::now();
        if now < end {
            return;
        }
        let over = now - end;
        let holding = self.threads.locks > 0 && over < LOCK_GRACE;
        let under = self.entries.len() > BODY_ENTRIES && over < SHALLOW_GRACE;
        if !holding && !under {
            // Held that long, a lock is no reason to hold the game any more: the others go on
            // beside it from here (and a count that came off, a lock never given back, stops
            // costing every turn its grace).
            self.threads.locks = 0;
            self.thread_park(State::Ready);
        }
    }

    /// The mods' code enters a monitor (`synchronized`, a lock taken): the thread that runs is
    /// not made to give the runtime up in the middle of what it guards, so no other thread
    /// finds that half done ([`LOCK_GRACE`]). One runs at a time, so nothing is waited for.
    #[inline]
    pub fn monitor_enter(&mut self, _of: V) {
        self.threads.locks += 1;
    }

    /// The mods' code leaves a monitor ([`Rt::monitor_enter`]).
    #[inline]
    pub fn monitor_exit(&mut self, _of: V) {
        self.threads.locks = self.threads.locks.saturating_sub(1);
    }

    /// [`Rt::monitor_enter`] for a class's own monitor (a `static synchronized` method).
    #[inline]
    pub fn monitor_enter_class(&mut self, _class: &'static str) {
        self.threads.locks += 1;
    }

    /// [`Rt::monitor_exit`] for a class's own monitor.
    #[inline]
    pub fn monitor_exit_class(&mut self, _class: &'static str) {
        self.threads.locks = self.threads.locks.saturating_sub(1);
    }

    /// Whether a thread waits under the runtime's own code (a lambda a `forEach` of the
    /// runtime's called, in the middle of a sort): what that code holds in containers of its
    /// own no stack shows, and a whole collection waits.
    pub(crate) fn threads_deep(&self) -> bool {
        self.threads
            .list
            .iter()
            .any(|t| t.stack.0 != 0 && t.state != State::Done && t.ctx.entries.len() > BODY_ENTRIES)
    }

    /// What the threads that wait hold where the collector can name it: their objects, what
    /// each may be throwing, and what the runtime's code keeps for them.
    pub(crate) fn thread_roots(&self, each: &mut dyn FnMut(V)) {
        for t in &self.threads.list {
            each(t.object);
            for c in [&t.ctx, &self.threads.game] {
                c.thrown.into_iter().for_each(&mut *each);
                c.pinned.iter().copied().for_each(&mut *each);
                c.scope_kept.iter().for_each(|r| each(V::R(*r)));
            }
        }
    }

    /// Every word of the stacks of the threads that wait in the middle of their code, each half
    /// of it as a number a reference could be (as [`gc`] reads the stack of a call in progress).
    pub(crate) fn thread_stack_words(&self, each: &mut dyn FnMut(u32)) {
        for t in &self.threads.list {
            let (low, high) = t.stack;
            if low == 0 || high <= low || high - low > 1 << 30 || t.state == State::Done {
                continue;
            }
            let mut at = (low + 3) & !3;
            while at + 4 <= high {
                // SAFETY: the thread waits ([`hand_back`]) and its stack stays as it left it;
                // read as bytes of memory, whatever they are.
                each(unsafe { std::ptr::read_volatile(at as *const u32) });
                at += 4;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn live(rt: &Rt, v: V) -> bool {
        rt.heap[v.r() as usize].class != gc::FREE
    }

    /// A thread of the mods' runs only while the game's thread lends the runtime, a slice at a
    /// time; where it waits for a time it gives the runtime back and goes on in the middle of
    /// its code when the time has come; what it was throwing is its own. When its body ends it
    /// is done, and whoever waits for it is woken.
    #[test]
    fn a_thread_runs_when_the_runtime_is_lent_and_goes_on_where_it_waited() {
        static STEP: AtomicU32 = AtomicU32::new(0);
        let mut rt = Rt::new();
        let object = rt.new_object("java/lang/Thread");
        rt.pinned.push(object);
        let id = rt.thread_start(
            object,
            Box::new(|rt| {
                STEP.store(1, Ordering::SeqCst);
                let e = rt.new_object("java/lang/IllegalStateException");
                rt.thrown = Some(e);
                let waited = rt.thread_wait(V::NULL, Some(Instant::now() + Duration::from_millis(30)));
                assert_eq!(waited, Waited::TimedOut);
                assert_eq!(rt.thrown, Some(e), "what it was throwing is still its own");
                rt.thrown = None;
                assert_ne!(rt.thread_id(), 0);
                STEP.store(2, Ordering::SeqCst);
            }),
        );
        assert_ne!(id, 0);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(STEP.load(Ordering::SeqCst), 0, "not before the runtime is lent");
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert_eq!(STEP.load(Ordering::SeqCst), 1);
        assert_eq!((rt.thrown, rt.thread_id()), (None, 0), "the game's thread has its own back");
        assert!(rt.thread_alive(id));
        assert!(!rt.run_threads(Duration::from_millis(5)), "its time has not come");
        std::thread::sleep(Duration::from_millis(40));
        assert!(rt.threads_ready());
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert_eq!(STEP.load(Ordering::SeqCst), 2);
        assert!(!rt.thread_alive(id) && !rt.has_threads());
    }

    /// A thread that waits on an object goes on when the object is woken, and one of two when
    /// one is; a wake of the game's thread's own waits for nothing.
    #[test]
    fn a_wait_ends_with_its_wake() {
        static WOKEN: AtomicU32 = AtomicU32::new(0);
        let mut rt = Rt::new();
        let lock = rt.new_object("java/lang/Object");
        rt.pinned.push(lock);
        for _ in 0..2 {
            rt.thread_start(
                V::NULL,
                Box::new(move |rt| {
                    if rt.thread_wait(lock, None) == Waited::Woken {
                        WOKEN.fetch_add(1, Ordering::SeqCst);
                    }
                }),
            );
        }
        assert_eq!(rt.thread_wait(lock, None), Waited::NotWaited);
        // (Both have begun and wait, however long the machine took to start them.)
        while rt.run_threads(Duration::from_millis(20)) {}
        assert_eq!(WOKEN.load(Ordering::SeqCst), 0);
        assert!(!rt.threads_ready(), "both wait");
        rt.thread_wake(lock, false);
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert_eq!(WOKEN.load(Ordering::SeqCst), 1);
        rt.thread_wake(lock, true);
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert_eq!(WOKEN.load(Ordering::SeqCst), 2);
        assert!(!rt.has_threads());
    }

    /// A thread past its slice gives the runtime back at a call into the runtime, and the game's
    /// thread is held no longer than that; the thread goes on at its next turn.
    #[test]
    fn a_thread_past_its_slice_hands_the_runtime_back() {
        static TURNS: AtomicU32 = AtomicU32::new(0);
        let mut rt = Rt::new();
        rt.thread_start(
            V::NULL,
            Box::new(|rt| {
                // Work that never waits: calls into the runtime, as the mods' code makes them.
                let began = Instant::now();
                while began.elapsed() < Duration::from_millis(40) {
                    rt.calls += 1;
                    if rt.calls >= rt.threads.check_at {
                        TURNS.fetch_add(1, Ordering::SeqCst);
                        rt.slice_point();
                    }
                }
            }),
        );
        let began = Instant::now();
        assert!(rt.run_threads(Duration::from_millis(4)));
        assert!(began.elapsed() < Duration::from_millis(30), "{:?}", began.elapsed());
        assert!(rt.has_threads(), "its work is not done in one slice");
        let mut rounds = 1;
        while rt.has_threads() && rounds < 200 {
            rt.run_threads(Duration::from_millis(4));
            rounds += 1;
        }
        assert!(!rt.has_threads() && rounds > 3, "{rounds} rounds");
    }

    /// A thread that holds a monitor keeps its turn past its slice until it leaves it, so no
    /// other finds what it guards half done; one that holds none hands over when its slice is up.
    #[test]
    fn a_thread_in_a_monitor_keeps_its_turn_until_it_leaves() {
        fn work(rt: &mut Rt, guarded: bool) {
            if guarded {
                rt.monitor_enter(V::NULL);
            }
            let began = Instant::now();
            while began.elapsed() < Duration::from_millis(20) {
                rt.calls += 1;
                if rt.calls >= rt.threads.check_at {
                    rt.slice_point();
                }
            }
            if guarded {
                rt.monitor_exit(V::NULL);
            }
        }
        for guarded in [true, false] {
            let mut rt = Rt::new();
            rt.thread_start(V::NULL, Box::new(move |rt| work(rt, guarded)));
            let began = Instant::now();
            assert!(rt.run_threads(Duration::from_millis(3)));
            let held = began.elapsed();
            if guarded {
                assert!(held >= Duration::from_millis(19), "{held:?}");
                assert!(!rt.has_threads(), "its work was done in the one turn");
            } else {
                assert!(held < Duration::from_millis(15), "{held:?}");
                assert!(rt.has_threads());
                while rt.has_threads() {
                    rt.run_threads(Duration::from_millis(3));
                }
            }
        }
    }

    /// A whole collection asked for on a thread of the mods' own is put off: the game's thread
    /// waits below it with frames of its own, which that thread cannot read.
    #[test]
    fn a_whole_collection_is_the_games_threads_to_run() {
        static FREED: AtomicU32 = AtomicU32::new(u32::MAX);
        let mut rt = Rt::new();
        let lost = rt.new_object("java/lang/Object");
        rt.thread_start(
            V::NULL,
            Box::new(|rt| {
                let before = rt.heap.len() - rt.free.len();
                let after = rt.collect();
                FREED.store((before - after) as u32, Ordering::SeqCst);
            }),
        );
        while rt.run_threads(Duration::from_millis(5)) {}
        assert_eq!(FREED.load(Ordering::SeqCst), 0);
        assert_eq!(rt.threads.put_off, 1);
        assert!(live(&rt, lost));
        rt.collect();
        assert!(!live(&rt, lost), "on the game's own thread it runs");
    }

    /// A thread whose code panics is done, and the game's thread has its runtime back.
    #[test]
    fn a_thread_that_panics_hands_the_runtime_back() {
        let mut rt = Rt::new();
        let id = rt.thread_start(
            V::NULL,
            Box::new(|rt| {
                rt.thrown = Some(V::NULL);
                panic!("a test's panic, on a thread of the mods' own");
            }),
        );
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert!(!rt.thread_alive(id) && !rt.has_threads());
        assert_eq!(
            (rt.thrown, rt.thread_id()),
            (None, 0),
            "the game's own context"
        );
    }

    /// Work on a thread of the mods' that runs for many turns leaves its garbage to no whole
    /// collection: a collection inside its call frees what it made in the turns before too,
    /// while the game's thread lends from between its own work, and keeps what another thread
    /// holds in its frames.
    #[test]
    fn a_threads_own_collection_frees_what_its_earlier_turns_made() {
        static HELD: AtomicU32 = AtomicU32::new(0);
        static FREED: AtomicU32 = AtomicU32::new(0);
        let mut rt = Rt::new();
        rt.young_min = 1;
        // A thread that holds one object in its frame, and waits.
        rt.thread_start(
            V::NULL,
            Box::new(|rt| {
                rt.entered(
                    |rt| {
                        // (Far up the heap: a small number on a stack is not taken for it.)
                        for _ in 0..70_000 {
                            rt.new_object("java/lang/Object");
                        }
                        let mine = std::hint::black_box(rt.new_object("java/lang/Object"));
                        HELD.store(mine.r(), Ordering::SeqCst);
                        rt.thread_wait(V::NULL, Some(Instant::now() + Duration::from_millis(200)));
                        assert_eq!(rt.class_of(std::hint::black_box(mine)), "java/lang/Object");
                    },
                    |_| V::Void,
                );
            }),
        );
        // And one that makes garbage over several turns, then collects inside its call.
        rt.thread_start(
            V::NULL,
            Box::new(|rt| {
                rt.entered(
                    |rt| {
                        let top = rt.entries.last().map(gc::Entry::began);
                        for _ in 0..3 {
                            for _ in 0..1000 {
                                rt.new_object("java/lang/Object");
                            }
                            rt.thread_yield();
                        }
                        let (born, stack) = top.unwrap();
                        let freed = rt.collect_young(born, &[], None, Some(stack), "a test");
                        FREED.store(freed as u32, Ordering::SeqCst);
                    },
                    |_| V::Void,
                );
            }),
        );
        let began = Instant::now();
        while FREED.load(Ordering::SeqCst) == 0 && began.elapsed() < Duration::from_secs(5) {
            rt.lend_threads(Duration::from_millis(5));
        }
        assert!(
            FREED.load(Ordering::SeqCst) >= 3000,
            "{}",
            FREED.load(Ordering::SeqCst)
        );
        assert!(
            live(&rt, V::R(HELD.load(Ordering::SeqCst))),
            "what the other thread holds stays"
        );
        while rt.has_threads() {
            rt.lend_threads(Duration::from_millis(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A collection inside a call of the game's thread frees what that call made, and not what a
    /// waiting thread made before and holds in a container of its own, which no stack shows.
    #[test]
    fn a_calls_collection_leaves_what_a_waiting_thread_holds() {
        let mut rt = Rt::new();
        rt.young_min = 1;
        let held = Arc::new(Mutex::new(Vec::<V>::new()));
        let theirs = held.clone();
        rt.thread_start(
            V::NULL,
            Box::new(move |rt| {
                // Its body, and under it the runtime's own code calling the mods' again (a
                // lambda a `forEach` called): two calls deep.
                rt.entered(
                    |rt| {
                        rt.entered(
                            |rt| {
                                let made = rt.new_object("java/lang/Object");
                                theirs.lock().unwrap().push(made);
                                rt.thread_wait(V::NULL, Some(Instant::now() + Duration::from_millis(10)));
                                assert_eq!(rt.class_of(made), "java/lang/Object");
                            },
                            |_| V::Void,
                        );
                    },
                    |_| V::Void,
                );
            }),
        );
        assert!(rt.run_threads(Duration::from_millis(5)));
        let theirs = held.lock().unwrap()[0];
        // A call of the game's thread that makes garbage and is collected as it ends.
        let garbage = rt.entered(
            |rt| {
                let g = rt.new_object("java/lang/Object");
                for _ in 0..8 {
                    rt.new_object("java/lang/Object");
                }
                g
            },
            |_| V::Void,
        );
        let freed = rt.collect_young(1, &[], None, None, "a test");
        assert!(freed > 0 && !live(&rt, garbage), "the call's own garbage goes");
        assert!(live(&rt, theirs), "what the waiting thread holds stays");
        // And a whole collection waits while that thread is under the runtime's code.
        let before = rt.heap.len() - rt.free.len();
        assert_eq!(rt.collect(), before);
        assert_eq!(rt.threads.put_off, 1);
        std::thread::sleep(Duration::from_millis(15));
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert!(!rt.has_threads());
    }

    /// A whole collection with a thread waiting in its own code (under none of the runtime's)
    /// keeps what that thread's frames hold, and frees what nothing holds.
    #[test]
    fn a_whole_collection_keeps_what_a_waiting_threads_frames_hold() {
        static KEPT: AtomicU32 = AtomicU32::new(0);
        let mut rt = Rt::new();
        rt.thread_start(
            V::NULL,
            Box::new(|rt| {
                let mine = std::hint::black_box(rt.new_object("java/lang/Object"));
                KEPT.store(mine.r(), Ordering::SeqCst);
                rt.thread_wait(V::NULL, Some(Instant::now() + Duration::from_millis(10)));
                assert_eq!(rt.class_of(std::hint::black_box(mine)), "java/lang/Object");
            }),
        );
        assert!(rt.run_threads(Duration::from_millis(5)));
        // (Far up the heap: a small number on a stack is not taken for it.)
        for _ in 0..70_000 {
            rt.new_object("java/lang/Object");
        }
        let lost = rt.new_object("java/lang/Object");
        rt.collect();
        assert_eq!(rt.threads.put_off, 0);
        assert!(!live(&rt, lost), "what nothing holds goes");
        assert!(live(&rt, V::R(KEPT.load(Ordering::SeqCst))), "the frame's object stays");
        std::thread::sleep(Duration::from_millis(15));
        assert!(rt.run_threads(Duration::from_millis(5)));
        assert!(!rt.has_threads());
    }
}
