//! Work given to a pool: `ExecutorService.execute` and `submit`, `CompletableFuture.runAsync`
//! and `supplyAsync`. On a JVM one of the pool's threads runs it while the code that gave it
//! goes on, so that code never finds it run inside the call that gave it. Here one thread runs
//! everything, and running it inside that call is what no JVM does: JourneyMap's minimap gives
//! the pool its grid's update from its own constructor, the update asks for the minimap, and
//! with no minimap kept yet that made another one, which gave the pool its update, without end.
//!
//! So the work waits ([`later`]) for the first of:
//! - its future being asked anything (`join`, `get`, `isDone`, a stage added to it): [`force`];
//! - the code that gave it waiting for something itself (a latch, a sleep, a queue's `take`,
//!   the pool's `awaitTermination`), as the pool's threads run while it waits: [`run_pending`];
//! - that code being done, back in the runtime's own with none of the mods' on the stack (a
//!   listener returned to the bus): [`at_rest`].
//!
//! It runs in the order it was given, as a pool of one thread would run it. What it throws with
//! nobody asking for its result is told of, as a pool's thread tells of what ends its task, and
//! kept for whoever asks later.
//!
//! Where the mods have threads of their own ([`crate::mod_threads::real`]: in a running game,
//! not while they load) a pool has one too ([`give`]): its work runs there, beside the game, a
//! slice at a time, for as long as it takes. Its future is pending meanwhile ([`pending`]):
//! not done when asked, waited for by whoever must have its value ([`force`]), and a stage
//! added to it is kept for when it is done ([`after`], [`settle`]). Work with a time of its own
//! (`schedule`, `scheduleAtFixedRate`) runs there when it is due ([`scheduled`]).
use crate::{Data, Rt, V};
use std::time::{Duration, Instant};

const FUTURE: &str = "java/util/concurrent/CompletableFuture";
const QUEUE: &str = "compat:pool-tasks";
/// A future's work while it has not run.
const TASK: &str = "$task";
/// How that work is called: [`RUN`], [`SUPPLY`] or [`CALL`].
const KIND: &str = "$taskKind";
/// What a `Runnable`'s future holds once it ran (`submit(task, result)`).
const RESULT: &str = "$taskResult";
/// What a future's work threw.
pub(crate) const FAILED: &str = "$failed";
pub(crate) const CANCELLED: &str = "$cancelled";
/// Set on a future whose outcome a thread of the mods' is still to bring: its work is in a
/// pool's own queue or runs there, or it is a stage after such a future.
const PENDING: &str = "$pending";
/// The pool whose queue holds a pending future's work.
const POOL: &str = "$pool";
/// The stages added to a future while it was pending: a list of records, each a list of what
/// to do (a stage's method name, or [`COPY`], [`ALL`], [`ANY`]), the future that stands for
/// its outcome, and the arguments.
const AFTER: &str = "$after";
/// The future a pending stage waits for.
const AWAITS: &str = "$awaits";
/// When scheduled work is due ([`clock`]), and how it repeats: every so many milliseconds from
/// one start to the next (positive), or from one end to the next start (negative).
const DUE: &str = "$due";
const PERIOD: &str = "$period";
/// Work only the game's own thread may run: it was given to the game's executor.
pub(crate) const HOME: &str = "$home";
/// A pool's own queue of futures, and the id of its thread.
const WORK: &str = "$work";
const WORKER: &str = "$worker";
/// The pool of work given to none (`CompletableFuture.runAsync(task)`).
const COMMON: &str = "compat:common-pool";
/// Records of [`AFTER`] that are no stage: the outcome is copied; one of `allOf`'s; of `anyOf`'s.
const COPY: &str = "$copy";
const ALL: &str = "$all";
const ANY: &str = "$any";
/// How long a pool's thread stays with nothing to do before it ends (the next work starts
/// another).
const IDLE: Duration = Duration::from_secs(30);

/// A `Runnable`.
pub(crate) const RUN: i32 = 0;
/// A `Supplier`, whose value the future holds.
pub(crate) const SUPPLY: i32 = 1;
/// A `Callable`, whose value the future holds.
pub(crate) const CALL: i32 = 2;

/// The rounds of waiting work one wait runs: what a round's work gives the pool is the next
/// round's, and work that gives the pool itself again without end must not hold the game.
const ROUNDS: usize = 64;

/// A future whose `work` (of `kind`) has not run and is in no pool's queue: whoever is handed
/// it runs it ([`force`]). `result` is what a `Runnable`'s future holds once it ran.
pub(crate) fn waiting(rt: &mut Rt, work: V, kind: i32, result: V) -> V {
    let future = rt.new_object(FUTURE);
    rt.put_field(future, TASK, work);
    rt.put_field(future, KIND, V::I(kind));
    if result.r() != 0 {
        rt.put_field(future, RESULT, result);
    }
    future
}

/// Gives a pool `work` (of `kind`) to run later, and the future of it: the common pool.
#[cfg(test)]
pub(crate) fn later(rt: &mut Rt, work: V, kind: i32, result: V) -> V {
    later_in(rt, V::NULL, work, kind, result)
}

/// Gives `pool` (null: the common one) `work` (of `kind`) to run later, and the future of it.
pub(crate) fn later_in(rt: &mut Rt, pool: V, work: V, kind: i32, result: V) -> V {
    let future = waiting(rt, work, kind, result);
    give(rt, pool, future);
    future
}

/// Puts a future whose work waits ([`waiting`]) in a pool's queue: `pool`'s own, run by its
/// own thread, where the mods have threads ([`crate::mod_threads::real`]); else the one the
/// game's thread runs when the mods' code is done ([`at_rest`]).
pub(crate) fn give(rt: &mut Rt, pool: V, future: V) {
    if crate::mod_threads::real(rt) && to_worker(rt, pool, future) {
        return;
    }
    queue(rt, future);
}

/// Puts a future whose work waits ([`waiting`]) in the queue the game's thread runs.
pub(crate) fn queue(rt: &mut Rt, future: V) {
    let queue = rt.intern(QUEUE.into(), |rt| crate::jutil::new_list(rt, Vec::new()));
    push(rt, queue, future);
}

fn push(rt: &mut Rt, list: V, v: V) {
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.push(v);
    }
}

fn remove(rt: &mut Rt, list: V, v: V) {
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data)
        && let Some(at) = l.iter().position(|x| *x == v)
    {
        l.remove(at);
    }
}

fn items(rt: &Rt, list: V) -> &[V] {
    match rt.heap.get(list.r() as usize).map(|o| &o.data) {
        Some(Data::List(l)) => l,
        _ => &[],
    }
}

/// Milliseconds on a clock that only goes forward, which scheduled work is due by.
fn clock() -> i64 {
    epoch().elapsed().as_millis() as i64
}

fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Whether `future`'s outcome is still to come from a thread of the mods' ([`PENDING`]).
pub(crate) fn pending(rt: &Rt, future: V) -> bool {
    future.r() != 0 && rt.get_field(future, PENDING).i() != 0
}

/// Puts `future` in `pool`'s own queue (null: the common pool's) and sees that the pool's
/// thread is there to run it. False where no thread is to be had: nothing was done.
fn to_worker(rt: &mut Rt, pool: V, future: V) -> bool {
    let pool = if pool.r() == 0 {
        rt.intern(COMMON.into(), |rt| rt.new_object("java/lang/Object"))
    } else {
        pool
    };
    let list = match rt.get_field(pool, WORK) {
        l if l.r() != 0 => l,
        _ => {
            let l = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(pool, WORK, l);
            l
        }
    };
    push(rt, list, future);
    rt.put_field(future, PENDING, V::I(1));
    rt.put_field(future, POOL, pool);
    let id = match rt.get_field(pool, WORKER) {
        V::I(n) if n > 0 => n as u32,
        _ => 0,
    };
    if id != 0 && rt.thread_alive(id) {
        crate::mod_threads::wake(rt, pool, true);
        return true;
    }
    let thread = crate::mod_threads::new_thread(rt, &format!("pool-{}-thread-1", pool.r()));
    // The thread holds its pool, and the pool its work, while there is any.
    rt.put_field(thread, POOL, pool);
    let id = rt.thread_start(thread, Box::new(move |rt| worker(rt, pool)));
    if id == 0 {
        remove(rt, list, future);
        rt.put_field(future, PENDING, V::Void);
        return false;
    }
    rt.put_field(thread, crate::mod_threads::ID, V::I(id as i32));
    rt.put_field(pool, WORKER, V::I(id as i32));
    true
}

/// What a pool's thread does next.
enum Next {
    Run(V),
    /// Nothing is due before then.
    At(Instant),
    Nothing,
}

/// A pool's own thread: runs what its queue holds, in order and when it is due, and waits for
/// more; with none for [`IDLE`] it ends.
fn worker(rt: &mut Rt, pool: V) {
    let mut idle = Instant::now();
    loop {
        match next(rt, pool) {
            Next::Run(future) => {
                work(rt, future);
                idle = Instant::now();
            }
            Next::At(when) => {
                rt.thread_wait(pool, Some(when));
            }
            Next::Nothing => {
                if idle.elapsed() >= IDLE {
                    break;
                }
                rt.thread_wait(pool, Some(Instant::now() + IDLE));
            }
        }
        // (An interrupt is a pool's way to stop its thread's work, not the thread.)
        crate::mod_threads::take_interrupt(rt);
    }
    rt.put_field(pool, WORKER, V::I(0));
}

/// The first work in `pool`'s queue that is due and no thread is in, or when the next is.
/// What was run or cancelled by another since it was queued leaves the queue here.
fn next(rt: &mut Rt, pool: V) -> Next {
    let list = rt.get_field(pool, WORK);
    let now = clock();
    let shut = rt.get_field(pool, "$shutdown").i() != 0;
    let mut earliest: Option<i64> = None;
    for future in items(rt, list).to_vec() {
        if rt.get_field(future, RUNNING).i() != 0 {
            continue;
        }
        if !waits(rt, future) {
            remove(rt, list, future);
            continue;
        }
        // A pool shut down repeats nothing more.
        if shut && rt.get_field(future, PERIOD).j() != 0 {
            cancel(rt, future);
            continue;
        }
        match rt.get_field(future, DUE) {
            V::J(due) if due > now => earliest = Some(earliest.map_or(due, |e| e.min(due))),
            _ => return Next::Run(future),
        }
    }
    match earliest {
        Some(due) => Next::At(epoch() + Duration::from_millis(due.max(0) as u64)),
        None => Next::Nothing,
    }
}

/// A pool's thread runs `future`'s work: for as long as it takes, the game going on beside it.
/// Work that repeats is due again after; any other is done, and whoever waits for it hears.
fn work(rt: &mut Rt, future: V) {
    let task = rt.get_field(future, TASK);
    rt.put_field(future, RUNNING, V::I(1));
    // The calls nothing maps are counted for this work alone, as for a listener.
    rt.hook_misses = 0;
    rt.hook_first_miss = None;
    crate::api::new_piece();
    run_timed(rt, future, task);
    rt.put_field(future, RUNNING, V::Void);
    rt.report_thrown("work given to a pool");
    // (A lock its code never gave back is no other work's to carry.)
    rt.threads.locks = 0;
    let period = rt.get_field(future, PERIOD).j();
    let over = rt.get_field(future, FAILED).r() != 0 || rt.get_field(future, CANCELLED).i() != 0;
    if period != 0 && !over {
        let due = if period > 0 {
            (rt.get_field(future, DUE).j() + period).max(clock())
        } else {
            clock() - period
        };
        rt.put_field(future, DUE, V::J(due));
        rt.put_field(future, TASK, task);
        return;
    }
    take_out(rt, future);
    settle(rt, future);
}

/// Takes a pending future out of its pool's queue.
fn take_out(rt: &mut Rt, future: V) {
    let pool = rt.get_field(future, POOL);
    if pool.r() != 0 {
        let list = rt.get_field(pool, WORK);
        remove(rt, list, future);
    }
}

/// Gives `pool` work to run on its own thread when it is due: after `delay` milliseconds, and
/// with `period` again and again ([`PERIOD`]). The object of class `class` that stands for it
/// (a `ScheduledFuture`); None where the mods have no threads, and nothing ever runs it.
pub(crate) fn scheduled(
    rt: &mut Rt,
    pool: V,
    class: &'static str,
    work: V,
    kind: i32,
    delay: i64,
    period: i64,
) -> Option<V> {
    if !crate::mod_threads::real(rt) || work.r() == 0 {
        return None;
    }
    let future = rt.new_object(class);
    rt.put_field(future, TASK, work);
    rt.put_field(future, KIND, V::I(kind));
    rt.put_field(future, DUE, V::J(clock() + delay.max(0)));
    rt.put_field(future, PERIOD, V::J(period));
    to_worker(rt, pool, future).then_some(future)
}

/// Whether `future` is work a pool's thread runs when it is due ([`scheduled`]).
pub(crate) fn was_scheduled(rt: &Rt, future: V) -> bool {
    matches!(rt.get_field(future, DUE), V::J(_))
}

/// How long until scheduled work is due, in milliseconds (`ScheduledFuture.getDelay`).
pub(crate) fn delay(rt: &Rt, future: V) -> i64 {
    match rt.get_field(future, DUE) {
        V::J(due) => due - clock(),
        _ => 0,
    }
}

/// Whether `pool`'s own thread has nothing to run and nothing waiting (`isTerminated`,
/// `awaitTermination`).
pub(crate) fn idle(rt: &Rt, pool: V) -> bool {
    let list = rt.get_field(pool, WORK);
    items(rt, list).is_empty()
}

/// `shutdownNow`: what `pool`'s thread has not begun never runs, and the work it is in is
/// interrupted.
pub(crate) fn stop(rt: &mut Rt, pool: V) {
    let list = rt.get_field(pool, WORK);
    for future in items(rt, list).to_vec() {
        if rt.get_field(future, RUNNING).i() == 0 {
            cancel(rt, future);
        } else if rt.get_field(future, PERIOD).j() != 0 {
            rt.put_field(future, CANCELLED, V::I(1));
        }
    }
    if let Some(thread) = worker_thread(rt, pool) {
        crate::mod_threads::interrupt(rt, thread);
    }
}

/// The `Thread` of `pool`'s own thread, while it has one.
fn worker_thread(rt: &Rt, pool: V) -> Option<V> {
    match rt.get_field(pool, WORKER) {
        V::I(n) if n > 0 => rt.thread_object_of(n as u32),
        _ => None,
    }
}

/// Whether `future`'s work has not run yet.
pub(crate) fn waits(rt: &Rt, future: V) -> bool {
    future.r() != 0 && rt.get_field(future, TASK).r() != 0
}

/// Something is asked of `future` that needs its outcome. Work that still waits runs now,
/// where it is asked, as a pool's thread would have run it by now; what a thread of the mods'
/// is still to bring ([`pending`]) is waited for ([`wait_for`]). What the work throws is the
/// future's ([`FAILED`]), not thrown at whoever asked.
pub(crate) fn force(rt: &mut Rt, future: V) {
    if pending(rt, future) {
        wait_for(rt, future, None);
        return;
    }
    if !waits(rt, future) {
        return;
    }
    run(rt, future);
    if rt.thrown.take().is_some() {
        rt.unwinding.clear();
    }
}

/// Whether the thread that runs is the one to run pending `future`'s work, where it stands
/// and now: no thread has begun it, it is due, and it is this thread's own to run. That is the
/// game's thread for what was given to the game's executor ([`HOME`]), and a pool's own thread
/// for what waits in its queue behind the work it is in: asking for that, it would wait for
/// itself (a pool here has one thread, whatever it was made with). Any other work a pool's
/// thread has is waited for: whoever asks is not the one it was given to.
fn takeable(rt: &Rt, future: V) -> bool {
    if !waits(rt, future)
        || rt.get_field(future, RUNNING).i() != 0
        || matches!(rt.get_field(future, DUE), V::J(_))
    {
        return false;
    }
    let me = rt.thread_id();
    if rt.get_field(future, HOME).i() != 0 {
        return me == 0;
    }
    let pool = rt.get_field(future, POOL);
    me != 0 && rt.get_field(pool, WORKER) == V::I(me as i32)
}

/// Whether pending `future` is done; if not, what this thread can do towards it here is done
/// first: the work it waits for that no thread has begun is run where it is asked for, as one
/// more thread of its pool would run it (a pool's one thread that asks for work queued behind
/// its own would wait for itself).
fn advance(rt: &mut Rt, future: V) -> bool {
    for _ in 0..64 {
        if !pending(rt, future) {
            return true;
        }
        // The first of the futures it waits for, one after another, that is not done.
        let mut at = future;
        for _ in 0..64 {
            let source = rt.get_field(at, AWAITS);
            if !pending(rt, source) {
                break;
            }
            at = source;
        }
        if !takeable(rt, at) {
            return false;
        }
        take_out(rt, at);
        run(rt, at);
        if rt.thrown.take().is_some() {
            rt.unwinding.clear();
        }
        settle(rt, at);
    }
    !pending(rt, future)
}

/// Waits for pending `future`'s outcome, until `until` at the latest (none: for as long as it
/// takes on a thread of the mods' own, and as long as the game can stand still on its own:
/// [`crate::mod_threads::wait_until`]). Whether it is done.
pub(crate) fn wait_for(rt: &mut Rt, future: V, until: Option<Instant>) -> bool {
    if advance(rt, future) {
        return true;
    }
    crate::mod_threads::wait_until(rt, future, until, |rt| advance(rt, future))
}

/// `Future.cancel`: work that has not begun never runs, work that repeats is not due again,
/// and a stage whose future is still to come is given up. False for what is done or runs.
pub(crate) fn cancel(rt: &mut Rt, future: V) -> bool {
    let running = rt.get_field(future, RUNNING).i() != 0;
    if running && rt.get_field(future, PERIOD).j() != 0 {
        rt.put_field(future, CANCELLED, V::I(1));
        return true;
    }
    let stage = pending(rt, future) && rt.get_field(future, AWAITS).r() != 0;
    if running || !(waits(rt, future) || stage) {
        return false;
    }
    rt.put_field(future, TASK, V::Void);
    rt.put_field(future, CANCELLED, V::I(1));
    if pending(rt, future) {
        take_out(rt, future);
        settle(rt, future);
    }
    true
}

/// A pending future has its outcome (its work ran, it was cancelled or completed by hand):
/// whoever waits for it is woken, and the stages added to it meanwhile run now, in the order
/// they were added, as a JVM runs them on the thread that completes it. One whose own future
/// is still to come (it gave a pool more work) hands that on when it is there.
pub(crate) fn settle(rt: &mut Rt, future: V) {
    if !pending(rt, future) {
        return;
    }
    rt.put_field(future, PENDING, V::Void);
    rt.put_field(future, AWAITS, V::Void);
    crate::mod_threads::wake(rt, future, true);
    let after = rt.get_field(future, AFTER);
    if after.r() == 0 {
        return;
    }
    // (The list holds each record while its stage runs.)
    let mut at = 0;
    while let Some(&record) = items(rt, after).get(at) {
        at += 1;
        // A stage for the game's own executor is the game's thread's to run.
        let parts = items(rt, record);
        let to_game = rt.thread_id() != 0
            && parts
                .first()
                .and_then(|n| rt.str_of(*n))
                .is_some_and(|n| n.ends_with("Async"))
            && parts
                .iter()
                .skip(3)
                .any(|e| crate::screens::game_executor(rt, *e));
        if to_game {
            let task = crate::functions::after_task(rt, future, record);
            crate::screens::give_game(rt, task);
        } else {
            replay(rt, future, record);
        }
    }
    rt.put_field(future, AFTER, V::Void);
}

/// Runs one record of [`AFTER`] for `source`, which is done.
pub(crate) fn replay(rt: &mut Rt, source: V, record: V) {
    let parts = items(rt, record).to_vec();
    let (Some(what), Some(&stage)) = (parts.first().and_then(|n| rt.str_of(*n)), parts.get(1))
    else {
        return;
    };
    let what = what.to_owned();
    match what.as_str() {
        COPY => link(rt, source, stage),
        ALL => {
            let all = crate::jutil::elements(rt, parts.get(2).copied().unwrap_or_default());
            if pending(rt, stage) && !all.iter().any(|f| pending(rt, *f)) {
                // The first failure among them is the whole's.
                let failed = all
                    .iter()
                    .map(|f| rt.get_field(*f, FAILED))
                    .find(|e| e.r() != 0);
                match failed {
                    Some(e) => rt.put_field(stage, FAILED, e),
                    None => rt.put_field(stage, "$value", V::NULL),
                }
                settle(rt, stage);
            }
        }
        ANY => {
            if pending(rt, stage) {
                link(rt, source, stage);
            }
        }
        name => {
            let made = crate::jutil::future_stage(rt, name, &parts[2..]);
            if rt.thrown.is_some() {
                rt.report_thrown("a stage after work given to a pool");
            }
            match made {
                Some(made) if made.r() != 0 => link(rt, made, stage),
                _ => {
                    rt.put_field(stage, "$value", V::NULL);
                    settle(rt, stage);
                }
            }
        }
    }
}

/// `stage` takes `from`'s outcome: now when it is there, else when it is.
fn link(rt: &mut Rt, from: V, stage: V) {
    if from == stage || !pending(rt, stage) {
        return;
    }
    if pending(rt, from) {
        let what = rt.string(COPY);
        let record = crate::jutil::new_list(rt, vec![what, stage]);
        add_after(rt, from, record);
        rt.put_field(stage, AWAITS, from);
        return;
    }
    for field in ["$value", FAILED, CANCELLED] {
        let v = rt.get_field(from, field);
        rt.put_field(stage, field, v);
    }
    settle(rt, stage);
}

fn add_after(rt: &mut Rt, source: V, record: V) {
    let list = match rt.get_field(source, AFTER) {
        l if l.r() != 0 => l,
        _ => {
            let l = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(source, AFTER, l);
            l
        }
    };
    push(rt, list, record);
}

/// A stage (`name`, with all its arguments) added to `source`, which is pending: the future
/// that stands for the stage's own, done when `source` is and the stage has run ([`settle`]).
pub(crate) fn after(rt: &mut Rt, source: V, name: &str, args: &[V]) -> V {
    let stage = rt.new_object(FUTURE);
    rt.put_field(stage, PENDING, V::I(1));
    rt.put_field(stage, AWAITS, source);
    let mut record = vec![rt.string(name), stage];
    record.extend_from_slice(args);
    let record = crate::jutil::new_list(rt, record);
    add_after(rt, source, record);
    stage
}

/// `allOf` (`any`: `anyOf`) of futures some of which are pending: the future done when they
/// all are (when the first is).
pub(crate) fn after_all(rt: &mut Rt, futures: V, any: bool) -> V {
    let stage = rt.new_object(FUTURE);
    rt.put_field(stage, PENDING, V::I(1));
    let what = rt.string(if any { ANY } else { ALL });
    for f in crate::jutil::elements(rt, futures) {
        if pending(rt, f) {
            let record = crate::jutil::new_list(rt, vec![what, stage, futures]);
            add_after(rt, f, record);
            rt.put_field(stage, AWAITS, f);
        }
    }
    stage
}

/// `future`'s work was given to the game's own executor from a thread of the mods': the
/// game's thread runs it, and its outcome is pending until then.
pub(crate) fn home(rt: &mut Rt, future: V) {
    rt.put_field(future, PENDING, V::I(1));
    rt.put_field(future, HOME, V::I(1));
}

/// A future completed by hand while it was pending (`complete`, `completeExceptionally`): its
/// work, if no thread has begun it, never runs, and it is done with what it was given.
pub(crate) fn completed_by_hand(rt: &mut Rt, future: V) {
    if !pending(rt, future) {
        return;
    }
    if rt.get_field(future, RUNNING).i() == 0 {
        rt.put_field(future, TASK, V::Void);
        take_out(rt, future);
    }
    settle(rt, future);
}

/// Set on a future while its work runs: a round of the queue begun inside that work (the work
/// waits for something itself) leaves it where it is, for the call running it to take out.
const RUNNING: &str = "$taskRunning";

/// How long one piece of a pool's work may run once the window is in a world. A pool's own
/// thread would run it beside the game; here the game's thread runs it, and stands still
/// meanwhile. (JourneyMap draws its map in such work, all the chunks around the player in one
/// piece: 82 s for the first, more than the 90 a piece has for the second, and the window
/// answered nothing for either.)
const IN_WORLD_TIME: std::time::Duration = std::time::Duration::from_secs(5);
thread_local! {
    /// [`IN_WORLD_TIME`] on this thread (a test's is shorter).
    static WORLD_TIME: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(IN_WORLD_TIME) };
}
/// How many times work of one kind may be stopped for that before no more of its kind is run.
const OVERRUNS: u32 = 2;

thread_local! {
    /// How many times this thread stopped work of each kind (its class: a lambda's names the
    /// method it runs) for running past [`IN_WORLD_TIME`].
    static OVERRAN: std::cell::RefCell<std::collections::HashMap<&'static str, u32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Whether work run now holds the window's thread in a world: where it has [`IN_WORLD_TIME`].
fn in_world(rt: &Rt) -> bool {
    crate::neo::is_client(rt)
        && !crate::neo::loading(rt)
        && crate::world_events::client_in_world(rt)
}

/// Runs the work of `future`, which waits no longer whatever comes of it. The work is a
/// piece of the mods' code on a clock of its own, as a listener is: one that never ends is
/// stopped as any other piece is. In a world it has [`IN_WORLD_TIME`], and work of a kind
/// stopped [`OVERRUNS`] times for that is run no more: its future is cancelled. The mod's
/// background work is then off (said once), and the game goes on.
fn run(rt: &mut Rt, future: V) {
    let work = rt.get_field(future, TASK);
    // (The runtime's own functions are no kind of a mod's work: each stands for many.)
    let kind = rt.class_of(work);
    // (On a thread of the mods' own it holds nothing up.)
    let timed = rt.thread_id() == 0 && in_world(rt) && !kind.starts_with("compat:");
    if timed && OVERRAN.with(|o| o.borrow().get(kind).copied().unwrap_or(0)) >= OVERRUNS {
        cancel(rt, future);
        return;
    }
    rt.put_field(future, RUNNING, V::I(1));
    let outer = if timed {
        crate::api::timed_start_within(WORLD_TIME.get())
    } else {
        crate::api::timed_start()
    };
    run_timed(rt, future, work);
    let stopped = crate::api::timed_out();
    crate::api::timed_end(outer);
    rt.put_field(future, RUNNING, V::Void);
    if timed && stopped {
        let times = OVERRAN.with(|o| {
            let mut o = o.borrow_mut();
            let times = o.entry(kind).or_insert(0);
            *times += 1;
            *times
        });
        if times == OVERRUNS {
            eprintln!(
                "horizon-compat: work given to a pool ({kind}) held the window's thread past {} s {OVERRUNS} times and is run no more: a pool's own thread would run it beside the game, and here the game's thread does",
                WORLD_TIME.get().as_secs()
            );
        }
    }
}

fn run_timed(rt: &mut Rt, future: V, work: V) {
    rt.put_field(future, TASK, V::Void);
    let value = match rt.get_field(future, KIND).i() {
        SUPPLY => rt.call_functional(
            "java/util/function/Supplier.get.()Ljava/lang/Object;",
            work,
            &[],
        ),
        CALL => rt.call_functional(
            "java/util/concurrent/Callable.call.()Ljava/lang/Object;",
            work,
            &[],
        ),
        _ => {
            rt.call_functional("java/lang/Runnable.run.()V", work, &[]);
            match rt.get_field(future, RESULT) {
                V::Void => V::NULL,
                result => result,
            }
        }
    };
    // (Completed by hand while its work ran: that stays its outcome.)
    if !matches!(rt.get_field(future, "$value"), V::Void) || rt.get_field(future, FAILED).r() != 0 {
        return;
    }
    match rt.thrown {
        Some(thrown) => {
            let held = completion_of(rt, thrown);
            rt.put_field(future, FAILED, held);
        }
        None => rt.put_field(future, "$value", value),
    }
}

pub(crate) const COMPLETION: &str = "java/util/concurrent/CompletionException";

/// What a future holds for work or a stage that threw `thrown`: a `CompletionException` around
/// it, its message the cause's text as Java's is (`CompletableFuture` wraps what its own work
/// throws; what is one already stays).
pub(crate) fn completion_of(rt: &mut Rt, thrown: V) -> V {
    if rt.class_of(thrown) == COMPLETION {
        return thrown;
    }
    let message = rt.get_field(thrown, "message");
    let text = match rt.str_of(message) {
        Some(m) => format!("{}: {m}", rt.class_of(thrown).replace('/', ".")),
        None => rt.class_of(thrown).replace('/', "."),
    };
    let held = rt.new_object(COMPLETION);
    let text = rt.string(&text);
    rt.put_field(held, "message", text);
    rt.put_field(held, "cause", thrown);
    held
}

/// One round of the pools' queue: the work waiting in it now, in order. What that work gives
/// the pools is the next round's. False when none of it ran.
///
/// Each future stays in the queue while its work runs: the queue is what holds it then (a
/// list of this function's own would hide it from a collection inside the hook, which frees
/// what the mods' code made and nothing reaches). So a round begun inside that work passes
/// over it ([`RUNNING`]).
fn round(rt: &mut Rt) -> bool {
    let Some(&queue) = rt.interned.get(QUEUE) else {
        return false;
    };
    let waiting = items(rt, queue).len();
    let mut ran = false;
    for _ in 0..waiting {
        let next = items(rt, queue)
            .iter()
            .copied()
            .find(|f| rt.get_field(*f, RUNNING).i() == 0);
        let Some(future) = next else {
            break;
        };
        // (Forced or cancelled since it was queued: done with.)
        if waits(rt, future) {
            // The calls nothing maps are counted for this work alone, as for a listener.
            let misses = (
                std::mem::take(&mut rt.hook_misses),
                rt.hook_first_miss.take(),
            );
            crate::api::new_piece();
            run(rt, future);
            (rt.hook_misses, rt.hook_first_miss) = misses;
            rt.report_thrown("work given to a pool");
            ran = true;
        }
        remove(rt, queue, future);
    }
    ran
}

/// The code that gave the pools their work waits for something itself: the pools' threads run
/// meanwhile, so everything waiting runs now, and what that gives the pools in turn.
pub(crate) fn run_pending(rt: &mut Rt) {
    for _ in 0..ROUNDS {
        if !round(rt) {
            return;
        }
    }
}

/// The mods' code is done for now (the runtime's own called it and has it back, none of
/// theirs on the stack): the work it gave the pools runs, as a pool's thread would have run it
/// alongside. Inside a call of the mods' it goes on waiting, for that call to end.
pub(crate) fn at_rest(rt: &mut Rt) {
    if rt.entries.is_empty() {
        round(rt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call;

    const EXECUTE: &str = "java/util/concurrent/ExecutorService.execute.(Ljava/lang/Runnable;)V";
    const SUBMIT: &str = "java/util/concurrent/ExecutorService.submit.(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;";
    const POOL: &str = "java/util/concurrent/Executors.newCachedThreadPool.()Ljava/util/concurrent/ExecutorService;";
    const RUN_ASYNC: &str = "java/util/concurrent/CompletableFuture.runAsync.(Ljava/lang/Runnable;Ljava/util/concurrent/Executor;)Ljava/util/concurrent/CompletableFuture;";
    const SUPPLY_ASYNC: &str = "java/util/concurrent/CompletableFuture.supplyAsync.(Ljava/util/function/Supplier;)Ljava/util/concurrent/CompletableFuture;";
    const COUNT_DOWN: &str = "java/util/concurrent/CountDownLatch.countDown.()V";
    const COUNT: &str = "java/util/concurrent/CountDownLatch.getCount.()J";

    /// What generated code does for an object no mod class is: the API call by the method's key.
    fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
        call(rt, key, args)
    }

    fn runtime() -> Rt {
        let rt = Rt::new();
        crate::install();
        rt
    }

    /// A latch of `count` and the task that counts it down once: what tells that the task ran.
    fn counting(rt: &mut Rt, count: i32) -> (V, V) {
        let latch = rt.new_object("java/util/concurrent/CountDownLatch");
        call(
            rt,
            "java/util/concurrent/CountDownLatch.<init>.(I)V",
            &[latch, V::I(count)],
        );
        let task = crate::functions::bound(rt, COUNT_DOWN, latch);
        (latch, task)
    }

    /// In a world, a pool's work holds the window's thread while it runs: past its time it is
    /// stopped, and work of a kind stopped twice is run no more (its future is cancelled),
    /// while other work, and that kind outside a world, goes on.
    #[test]
    fn work_that_holds_the_window_is_stopped_then_run_no_more() {
        thread_local!(static RAN: std::cell::Cell<[u32; 2]> = const { std::cell::Cell::new([0; 2]) });
        // Lambda 1 asks the game something and counts when it was answered; lambda 2 counts.
        fn lambda(rt: &mut Rt, id: u32, _args: &[V]) -> Option<V> {
            if id == 1 {
                call(rt, "java/lang/Math.abs.(I)I", &[V::I(-1)]);
                if rt.thrown.is_some() {
                    return Some(V::Void);
                }
            }
            RAN.with(|r| {
                let mut ran = r.get();
                ran[(id - 1) as usize] += 1;
                r.set(ran);
            });
            Some(V::Void)
        }
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(lambda);
        rt.interned.insert("side:client".into(), V::I(1));
        assert!(in_world(&rt));
        let give = |rt: &mut Rt, id: u32, class: &'static str| {
            let work = rt.lambda_id(class, id, Vec::new());
            later(rt, work, RUN, V::NULL)
        };
        // With time, it runs.
        let first = give(&mut rt, 1, "t/Map.draw");
        round(&mut rt);
        assert_eq!(RAN.get(), [1, 0]);
        assert!(!waits(&rt, first));
        // With none left, it is stopped at the call it makes: twice, and a line says so.
        WORLD_TIME.set(std::time::Duration::ZERO);
        for _ in 0..OVERRUNS {
            give(&mut rt, 1, "t/Map.draw");
            crate::api::next_call_reads_the_clock();
            round(&mut rt);
            assert!(
                rt.thrown.is_none(),
                "what stopped it is the work's, not the game's"
            );
        }
        assert_eq!(RAN.get(), [1, 0]);
        // Its kind is run no more: cancelled, as a future whose work never ran.
        WORLD_TIME.set(IN_WORLD_TIME);
        let dropped = give(&mut rt, 1, "t/Map.draw");
        round(&mut rt);
        assert_eq!(RAN.get(), [1, 0]);
        assert_eq!(rt.get_field(dropped, CANCELLED), V::I(1));
        // Work of another kind goes on, and so does that kind where no world is open.
        give(&mut rt, 2, "t/Cache.save");
        round(&mut rt);
        assert_eq!(RAN.get(), [1, 1]);
        rt.interned.remove("side:client");
        give(&mut rt, 1, "t/Map.draw");
        round(&mut rt);
        assert_eq!(RAN.get(), [2, 1]);
    }

    /// A pool's work doesn't run inside the call that gave it, nor inside anything that call
    /// goes on to do (posting an event on the bus), as a JVM's pool thread runs it alongside:
    /// it runs once the mods' code is back in the runtime's own. JourneyMap's minimap gives the
    /// pool its grid's update from its constructor, and the update asks for that very minimap.
    #[test]
    fn work_given_to_a_pool_runs_once_the_code_that_gave_it_is_done() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        let pool = call(&mut rt, POOL, &[]);
        let (latch, task) = counting(&mut rt, 2);
        // A call of the mods' code, as the runtime enters one.
        rt.entered(
            |rt| {
                call(rt, EXECUTE, &[pool, task]);
                call(rt, RUN_ASYNC, &[task, pool]);
                assert_eq!(call(rt, COUNT, &[latch]), V::J(2));
                at_rest(rt);
                assert_eq!(call(rt, COUNT, &[latch]), V::J(2));
            },
            |_| V::Void,
        );
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(2));
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0));
        // And once only.
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Asking a future for its value runs its work there, and waiting on a latch runs what
    /// the pools hold: the code waiting is what a JVM's pool threads would run beside.
    #[test]
    fn work_runs_where_its_result_is_waited_for() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        let pool = call(&mut rt, POOL, &[]);
        let seven = rt.string("seven");
        let supplier = crate::functions::constant(&mut rt, seven);
        let (latch, task) = counting(&mut rt, 1);
        rt.entered(
            |rt| {
                let supplied = call(rt, SUPPLY_ASYNC, &[supplier]);
                let called = call(rt, SUBMIT, &[pool, supplier]);
                assert!(waits(rt, supplied) && waits(rt, called));
                assert_eq!(
                    call(
                        rt,
                        "java/util/concurrent/CompletableFuture.join.()Ljava/lang/Object;",
                        &[supplied]
                    ),
                    seven
                );
                // (The other's work waits still: nothing asked for it.)
                assert!(waits(rt, called));
                assert_eq!(
                    call(rt, "java/util/concurrent/Future.isDone.()Z", &[called]),
                    V::I(1)
                );
                assert_eq!(
                    call(
                        rt,
                        "java/util/concurrent/Future.get.()Ljava/lang/Object;",
                        &[called]
                    ),
                    seven
                );
                call(rt, EXECUTE, &[pool, task]);
                assert_eq!(call(rt, COUNT, &[latch]), V::J(1));
                call(
                    rt,
                    "java/util/concurrent/CountDownLatch.await.()V",
                    &[latch],
                );
                assert_eq!(call(rt, COUNT, &[latch]), V::J(0));
            },
            |_| V::Void,
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Work cancelled before it ran never runs, and its future says so; cancelling work that
    /// ran changes nothing.
    #[test]
    fn work_cancelled_before_it_ran_never_runs() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        let pool = call(&mut rt, POOL, &[]);
        let (latch, task) = counting(&mut rt, 1);
        const CANCEL: &str = "java/util/concurrent/Future.cancel.(Z)Z";
        const IS_CANCELLED: &str = "java/util/concurrent/Future.isCancelled.()Z";
        let future = rt.entered(
            |rt| {
                let future = call(rt, RUN_ASYNC, &[task, pool]);
                assert_eq!(call(rt, CANCEL, &[future, V::I(1)]), V::I(1));
                future
            },
            |f| *f,
        );
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(1));
        assert_eq!(call(&mut rt, IS_CANCELLED, &[future]), V::I(1));
        let ran = call(&mut rt, RUN_ASYNC, &[task, pool]);
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0));
        assert_eq!(call(&mut rt, CANCEL, &[ran, V::I(1)]), V::I(0));
        assert_eq!(call(&mut rt, IS_CANCELLED, &[ran]), V::I(0));
    }

    /// What work throws is its future's, as on a JVM where no caller is there to catch it:
    /// nothing is thrown where the work ran, the stages after it don't run, `join` throws it
    /// in a `CompletionException` and `get` in an `ExecutionException`, and `exceptionally`
    /// is handed it and answers in its place.
    #[test]
    fn what_work_throws_is_its_futures() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        const FAILED_SO: &str =
            "java/util/concurrent/CompletableFuture.isCompletedExceptionally.()Z";
        const JOIN: &str = "java/util/concurrent/CompletableFuture.join.()Ljava/lang/Object;";
        // `Integer.parseInt("x")` as a supplier.
        let x = rt.string("x");
        let thrower = crate::functions::bound(
            &mut rt,
            "java/lang/Integer.parseInt.(Ljava/lang/String;)I",
            x,
        );
        let future = call(&mut rt, SUPPLY_ASYNC, &[thrower]);
        at_rest(&mut rt);
        assert!(rt.thrown.is_none());
        assert_eq!(call(&mut rt, FAILED_SO, &[future]), V::I(1));
        let (latch, task) = counting(&mut rt, 1);
        let next = call(
            &mut rt,
            "java/util/concurrent/CompletableFuture.thenRun.(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;",
            &[future, task],
        );
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(1));
        assert_eq!(call(&mut rt, FAILED_SO, &[next]), V::I(1));
        call(&mut rt, JOIN, &[future]);
        let thrown = rt.thrown.take().expect("join throws what the work threw");
        assert_eq!(rt.class_of(thrown), COMPLETION);
        let cause = rt.get_field(thrown, "cause");
        assert_eq!(rt.class_of(cause), "java/lang/NumberFormatException");
        call(
            &mut rt,
            "java/util/concurrent/Future.get.()Ljava/lang/Object;",
            &[future],
        );
        let thrown = rt.thrown.take().expect("get throws what the work threw");
        assert_eq!(
            rt.class_of(thrown),
            "java/util/concurrent/ExecutionException"
        );
        assert_eq!(rt.get_field(thrown, "cause"), cause);
        let instead = rt.string("instead");
        let answer = crate::functions::constant(&mut rt, instead);
        let recovered = call(
            &mut rt,
            "java/util/concurrent/CompletableFuture.exceptionally.(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;",
            &[future, answer],
        );
        assert_eq!(call(&mut rt, JOIN, &[recovered]), instead);
        assert!(rt.thrown.is_none());
    }

    /// An executor that is no pool is handed the work to run as it chooses: a direct one
    /// (`Runnable::run`) runs it where it is given, inside the call.
    #[test]
    fn an_executor_of_the_mods_own_runs_the_work_itself() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        let direct = crate::functions::method(&mut rt, "java/lang/Runnable.run.()V");
        let (latch, task) = counting(&mut rt, 1);
        rt.entered(
            |rt| {
                let future = call(rt, RUN_ASYNC, &[task, direct]);
                assert_eq!(call(rt, COUNT, &[latch]), V::J(0));
                assert!(!waits(rt, future));
            },
            |_| V::Void,
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Work that waits for other work of the pool: that runs inside its wait, and the work
    /// waiting is still the pool's own when it goes on (a collection that runs meanwhile
    /// finds it in the queue).
    #[test]
    fn work_that_waits_lets_the_rest_of_the_pool_run() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        let pool = call(&mut rt, POOL, &[]);
        let (latch, task) = counting(&mut rt, 1);
        // The first work waits on the latch the second counts down.
        let waits_first = crate::functions::bound(
            &mut rt,
            "java/util/concurrent/CountDownLatch.await.()V",
            latch,
        );
        let first = call(
            &mut rt,
            "java/util/concurrent/ExecutorService.submit.(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;",
            &[pool, waits_first],
        );
        call(&mut rt, EXECUTE, &[pool, task]);
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0));
        assert!(!waits(&rt, first));
        assert_eq!(rt.class_of(first), FUTURE);
        assert_eq!(
            call(&mut rt, "java/util/concurrent/Future.isDone.()Z", &[first]),
            V::I(1)
        );
        // Nothing is left in the queue.
        assert!(!round(&mut rt));
        assert!(rt.thrown.is_none());
    }

    /// The mods' code of the tests below, where the pools have threads of their own.
    fn pool_lambda(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        Some(match id {
            // A supplier that takes its time, then tells which thread ran it.
            1 => {
                call(rt, "java/lang/Thread.sleep.(J)V", &[V::J(20)]);
                V::I(rt.thread_id() as i32)
            }
            // A function of that: one more.
            2 => V::I(args.last().map_or(-100, |v| v.i()) + 1),
            // Counts the latch kept for it down.
            3 => {
                let latch = rt.interned.get("t:latch").copied().unwrap_or_default();
                call(rt, COUNT_DOWN, &[latch]);
                V::Void
            }
            // Work that takes far longer than a window may be held: the mods' code as the
            // linked runtime enters it, its calls into the runtime through the core.
            4 => rt.entered(
                |rt| {
                    let began = std::time::Instant::now();
                    while began.elapsed() < Duration::from_millis(60) && rt.thrown.is_none() {
                        horizon_compat_core::api::call(rt, "java/lang/Math.abs.(I)I", &[V::I(-1)]);
                    }
                    let latch = rt.interned.get("t:latch").copied().unwrap_or_default();
                    call(rt, COUNT_DOWN, &[latch]);
                    V::Void
                },
                |_| V::Void,
            ),
            _ => V::Void,
        })
    }

    /// A runtime in a running game: its thread lends it, and its pools have threads.
    fn threaded() -> Rt {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(pool_lambda);
        rt.threads.lends = 1;
        rt
    }

    const IS_DONE: &str = "java/util/concurrent/Future.isDone.()Z";
    const JOIN: &str = "java/util/concurrent/CompletableFuture.join.()Ljava/lang/Object;";
    const AWAIT: &str =
        "java/util/concurrent/CountDownLatch.await.(JLjava/util/concurrent/TimeUnit;)Z";

    /// In a running game a pool's work runs on the pool's own thread: its future is not done
    /// when the work is given, nor is a stage added to it; who asks for the stage's value
    /// waits for both, and the work ran on a thread that is not the game's.
    #[test]
    fn a_pools_work_runs_on_its_own_thread_and_its_stages_after_it() {
        let mut rt = threaded();
        let pool = call(&mut rt, POOL, &[]);
        let supplier = rt.lambda_id("t/Work.get", 1, Vec::new());
        let plus_one = rt.lambda_id("t/Work.next", 2, Vec::new());
        let future = call(
            &mut rt,
            "java/util/concurrent/CompletableFuture.supplyAsync.(Ljava/util/function/Supplier;Ljava/util/concurrent/Executor;)Ljava/util/concurrent/CompletableFuture;",
            &[supplier, pool],
        );
        assert!(pending(&rt, future));
        assert_eq!(call(&mut rt, IS_DONE, &[future]), V::I(0));
        let stage = call(
            &mut rt,
            "java/util/concurrent/CompletableFuture.thenApply.(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;",
            &[future, plus_one],
        );
        assert_eq!(call(&mut rt, IS_DONE, &[stage]), V::I(0));
        assert_eq!(
            call(&mut rt, IS_DONE, &[future]),
            V::I(0),
            "asking the stage ran nothing"
        );
        let began = std::time::Instant::now();
        let value = call(&mut rt, JOIN, &[stage]);
        assert!(
            began.elapsed() >= Duration::from_millis(18),
            "{:?}",
            began.elapsed()
        );
        let ran_on = call(&mut rt, JOIN, &[future]);
        assert!(ran_on.i() > 0, "on the pool's thread: {ran_on:?}");
        assert_eq!(value, V::I(ran_on.i() + 1));
        assert_eq!(call(&mut rt, IS_DONE, &[stage]), V::I(1));
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// Work that takes long holds no window: it is not stopped for its time, however often it
    /// is given, and the game's thread goes on between its slices.
    #[test]
    fn long_work_on_a_pools_thread_is_not_stopped() {
        let mut rt = threaded();
        rt.interned.insert("side:client".into(), V::I(1));
        assert!(in_world(&rt));
        WORLD_TIME.set(Duration::from_millis(1));
        let pool = call(&mut rt, POOL, &[]);
        let latch = rt.new_object("java/util/concurrent/CountDownLatch");
        call(
            &mut rt,
            "java/util/concurrent/CountDownLatch.<init>.(I)V",
            &[latch, V::I(3)],
        );
        rt.interned.insert("t:latch".into(), latch);
        for _ in 0..3 {
            let work = rt.lambda_id("t/Map.draw", 4, Vec::new());
            call(&mut rt, EXECUTE, &[pool, work]);
        }
        // The game's thread between two frames: a slice for them each time, and no more.
        let began = std::time::Instant::now();
        let mut longest = Duration::ZERO;
        while call(&mut rt, COUNT, &[latch]) != V::J(0) && began.elapsed() < Duration::from_secs(20)
        {
            let lent = std::time::Instant::now();
            rt.run_threads(Duration::from_millis(3));
            longest = longest.max(lent.elapsed());
        }
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0), "all of it ran");
        assert!(
            longest < Duration::from_millis(40),
            "the game was held {longest:?}"
        );
        assert!(rt.thrown.is_none());
    }

    /// Work with a time of its own runs when it is due, again and again, until it is cancelled.
    #[test]
    fn scheduled_work_runs_when_it_is_due_until_cancelled() {
        let mut rt = threaded();
        let pool = call(
            &mut rt,
            "java/util/concurrent/Executors.newScheduledThreadPool.(I)Ljava/util/concurrent/ScheduledExecutorService;",
            &[V::I(1)],
        );
        let latch = rt.new_object("java/util/concurrent/CountDownLatch");
        call(
            &mut rt,
            "java/util/concurrent/CountDownLatch.<init>.(I)V",
            &[latch, V::I(3)],
        );
        rt.interned.insert("t:latch".into(), latch);
        let tick = rt.lambda_id("t/Tick.run", 3, Vec::new());
        let ms = crate::api::get_static(&mut rt, "java/util/concurrent/TimeUnit.MILLISECONDS");
        let seconds = crate::api::get_static(&mut rt, "java/util/concurrent/TimeUnit.SECONDS");
        let began = std::time::Instant::now();
        let ticking = call(
            &mut rt,
            "java/util/concurrent/ScheduledExecutorService.scheduleAtFixedRate.(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;",
            &[pool, tick, V::J(10), V::J(10), ms],
        );
        assert_eq!(
            call(
                &mut rt,
                "java/util/concurrent/ScheduledFuture.isDone.()Z",
                &[ticking]
            ),
            V::I(0)
        );
        assert_eq!(call(&mut rt, AWAIT, &[latch, V::J(5), seconds]), V::I(1));
        assert!(
            began.elapsed() >= Duration::from_millis(28),
            "three periods: {:?}",
            began.elapsed()
        );
        assert_eq!(
            call(
                &mut rt,
                "java/util/concurrent/ScheduledFuture.cancel.(Z)Z",
                &[ticking, V::I(0)]
            ),
            V::I(1)
        );
        assert_eq!(
            call(
                &mut rt,
                "java/util/concurrent/ScheduledFuture.isDone.()Z",
                &[ticking]
            ),
            V::I(1)
        );
        // Cancelled, it runs no more: the pool's thread has nothing left.
        for _ in 0..10 {
            rt.run_threads(Duration::from_millis(3));
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(idle(&rt, pool));
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// Work that gives the pool more work: the pool runs that after it, in the order given,
    /// when the code waiting for it all is the one that waits.
    #[test]
    fn work_that_gives_the_pool_more_runs_it_after() {
        let mut rt = runtime();
        rt.vcall = Some(vcall);
        let pool = call(&mut rt, POOL, &[]);
        let (latch, task) = counting(&mut rt, 1);
        // The first work hands the common pool the second: `CompletableFuture.runAsync(task)`.
        let hands_on = crate::functions::bound(
            &mut rt,
            "java/util/concurrent/CompletableFuture.runAsync.(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;",
            task,
        );
        call(&mut rt, EXECUTE, &[pool, hands_on]);
        // One round runs the first, which queues the second.
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(1));
        at_rest(&mut rt);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0));
    }
}
