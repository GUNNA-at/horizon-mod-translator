//! The mods' own threads, as the runtime's answers use them ([`crate::threads`], the core's).
//!
//! A thread a mod starts, and the thread of a pool it gives work to, runs on a stack of its
//! own, a slice at a time, while the game's thread lends the runtime ([`lend`]: between two
//! frames, at a tick's end). So it can wait: a sleep, a latch, a lock's condition, a queue, a
//! future is waited for where it is asked, in the middle of the mod's code, and the game goes
//! on meanwhile. On the game's own thread a wait can't stop the game for good: the mods'
//! threads run inside it ([`game_wait`]), as they would have run beside it, until what is
//! waited for holds, and no longer than the game can stand still.
//!
//! Not everywhere: while the mods load there is no frame and no tick to lend at, and a runtime
//! whose thread never comes by [`lend`] (the one the mods load in, a probe's, a test's) runs
//! what is started on it where it is started, as before ([`real`]).
use crate::threads::{SLICE, Waited};
use crate::{Rt, V};
use std::time::{Duration, Instant};

const THREAD: &str = "java/lang/Thread";
const RUN: &str = "java/lang/Runnable.run.()V";
/// The id the runtime's core gave a `Thread` that runs on a stack of its own.
pub(crate) const ID: &str = "$tid";
/// A `Thread` asked to stop waiting (`interrupt`), until it is told so.
const INTERRUPTED: &str = "$interrupted";
/// A `Thread`'s permit to pass its next `LockSupport.park`.
const PERMIT: &str = "$permit";
/// The object the game's own thread waits on, and whether it was woken ([`wake`]).
const GAME_WAITS: &str = "compat:game-waits";
const GAME_WOKEN: &str = "compat:game-woken";

/// How long a wait that something should end, and nothing here is known to, goes on before
/// the thread looks again at what it waits for.
pub(crate) const RECHECK: Duration = Duration::from_millis(20);
/// How long the game's thread waits for something with no time given: the game stands still
/// meanwhile, and what it waits for may be something no thread here will ever do.
pub(crate) const PATIENCE: Duration = Duration::from_secs(5);
/// The same for a bare wait (`Object.wait()`, a condition's `await()`), which may end for no
/// reason at all, as Java's may.
const BARE_PATIENCE: Duration = Duration::from_millis(250);
/// How long the game's thread sleeps at most when a mod asks it to: the others run meanwhile.
const GAME_SLEEP: Duration = Duration::from_millis(50);

/// Whether threads of the mods' own run on this runtime: its game thread lends it (it has,
/// once), and the mods are not loading.
pub(crate) fn real(rt: &Rt) -> bool {
    rt.threads.lends > 0 && !crate::neo::loading(rt)
}

/// Starts `thread` (a `java.lang.Thread`) on a stack of its own. False where none is to be
/// had ([`real`]): the caller runs its body where it stands.
pub(crate) fn start(rt: &mut Rt, thread: V) -> bool {
    if !real(rt) {
        return false;
    }
    let id = rt.thread_start(thread, Box::new(move |rt| body(rt, thread)));
    if id == 0 {
        return false;
    }
    rt.put_field(thread, ID, V::I(id as i32));
    true
}

/// A started thread's body: a subclass's own `run`, else the `Runnable` it was made with.
fn body(rt: &mut Rt, thread: V) {
    let target = if rt.class_of(thread) == THREAD {
        rt.get_field(thread, "$target")
    } else {
        thread
    };
    if target.r() != 0 {
        rt.call_functional(RUN, target, &[]);
    }
    ended(rt, "a thread a mod started");
}

/// What ended a thread's body stays in it, as on a JVM: an interrupt silently, anything else
/// named.
pub(crate) fn ended(rt: &mut Rt, what: &str) {
    if let Some(t) = rt.thrown.take() {
        rt.unwinding.clear();
        if rt.class_of(t) != "java/lang/InterruptedException" {
            let message = rt.get_field(t, "message");
            eprintln!(
                "horizon-compat: {what} ended with {}: {}",
                rt.class_of(t),
                rt.str_of(message).unwrap_or("")
            );
        }
    }
}

/// A new `Thread` object for a thread the runtime starts itself (a pool's), named `name`.
pub(crate) fn new_thread(rt: &mut Rt, name: &str) -> V {
    let thread = rt.new_object(THREAD);
    let name = rt.string(name);
    rt.put_field(thread, "$name", name);
    rt.put_field(thread, "$daemon", V::I(1));
    rt.put_field(thread, "$started", V::I(1));
    thread
}

/// The id of the stack `thread` runs on; 0 for one that has none (not started, or run where it
/// was started).
pub(crate) fn id_of(rt: &Rt, thread: V) -> u32 {
    match rt.get_field(thread, ID) {
        V::I(n) if n > 0 => n as u32,
        _ => 0,
    }
}

/// Whether `thread` runs on a stack of its own and is not done.
pub(crate) fn alive(rt: &Rt, thread: V) -> bool {
    let id = id_of(rt, thread);
    id != 0 && rt.thread_alive(id)
}

/// `Thread.currentThread()` on a thread of the mods' own; None on the game's.
pub(crate) fn current(rt: &Rt) -> Option<V> {
    Some(rt.thread_object()).filter(|t| t.r() != 0)
}

/// `Thread.interrupt()`: the thread is told at its next wait, and woken from the one it is in.
pub(crate) fn interrupt(rt: &mut Rt, thread: V) {
    let id = id_of(rt, thread);
    if id != 0 {
        rt.put_field(thread, INTERRUPTED, V::I(1));
        rt.thread_unpark(id);
    }
}

/// Whether `thread` was interrupted and has not been told yet.
pub(crate) fn interrupted(rt: &Rt, thread: V) -> bool {
    thread.r() != 0 && rt.get_field(thread, INTERRUPTED).i() != 0
}

/// Whether the thread that runs was interrupted, told now (`Thread.interrupted()`).
pub(crate) fn take_interrupt(rt: &mut Rt) -> bool {
    let me = rt.thread_object();
    if !interrupted(rt, me) {
        return false;
    }
    rt.put_field(me, INTERRUPTED, V::I(0));
    true
}

fn throw_interrupted(rt: &mut Rt) -> V {
    crate::jlang::throw_with(
        rt,
        "java/lang/InterruptedException",
        "the thread was interrupted",
    )
}

/// A thread of the mods' own waits for a wake of `on` ([`wake`]; null for none), until
/// `until` at the latest. Interrupted before or meanwhile, it throws as Java's waits do
/// (`rt.thrown`), and the wait is over.
pub(crate) fn own_wait(rt: &mut Rt, on: V, until: Option<Instant>) -> Waited {
    if take_interrupt(rt) {
        throw_interrupted(rt);
        return Waited::Woken;
    }
    let waited = rt.thread_wait(on, until);
    if take_interrupt(rt) {
        throw_interrupted(rt);
    }
    waited
}

/// The game's own thread waits for `done`, asked again after each turn the mods' threads have
/// had: they run meanwhile, as they would beside it. Until it holds (true), `until` comes, no
/// thread of theirs can go on (nothing would ever change), or the piece of the mods' code that
/// waits is out of its own time.
pub(crate) fn game_wait(
    rt: &mut Rt,
    until: Option<Instant>,
    mut done: impl FnMut(&mut Rt) -> bool,
) -> bool {
    loop {
        if done(rt) {
            return true;
        }
        let now = Instant::now();
        if until.is_some_and(|u| now >= u) || rt.thrown.is_some() || crate::api::piece_over() {
            return false;
        }
        match rt.threads_next() {
            None => return false,
            Some(at) if at <= now => {
                rt.run_threads(SLICE * 2);
            }
            // They all wait for a time: the game's thread with them, a little at a time.
            Some(at) => {
                let to = until.map_or(at, |u| u.min(at));
                std::thread::sleep(
                    to.saturating_duration_since(now)
                        .min(Duration::from_millis(2)),
                );
            }
        }
    }
}

/// Waits until `done` holds, wherever it is asked: a thread of the mods' own waits for a wake
/// of `on` and looks again (also every [`RECHECK`]: not every change is told); the game's own
/// lets the mods' threads run meanwhile ([`game_wait`], for [`PATIENCE`] when no time is
/// given). Whether it held; false too when the thread was interrupted (`rt.thrown`).
pub(crate) fn wait_until(
    rt: &mut Rt,
    on: V,
    until: Option<Instant>,
    mut done: impl FnMut(&mut Rt) -> bool,
) -> bool {
    if rt.thread_id() == 0 {
        if crate::jsystem::in_started_thread() {
            if done(rt) {
                return true;
            }
            crate::jsystem::wait(rt);
            return false;
        }
        // What the pools hold with no thread of their own runs first, as beside a wait.
        if done(rt) {
            return true;
        }
        crate::tasks::run_pending(rt);
        let until = until.unwrap_or_else(|| Instant::now() + PATIENCE);
        return game_wait(rt, Some(until), done);
    }
    loop {
        if done(rt) {
            return true;
        }
        if rt.thrown.is_some() {
            return false;
        }
        let now = Instant::now();
        if until.is_some_and(|u| now >= u) {
            return false;
        }
        let look = now + RECHECK;
        own_wait(rt, on, Some(until.map_or(look, |u| u.min(look))));
    }
}

/// `Thread.sleep`: a thread of the mods' own sleeps where it is, the game going on; the game's
/// own lets the others run for as long ([`GAME_SLEEP`] at most), and goes on at once when
/// there are none.
pub(crate) fn sleep(rt: &mut Rt, time: Duration) -> V {
    if rt.thread_id() == 0 {
        if crate::jsystem::in_started_thread() {
            return crate::jsystem::wait(rt);
        }
        crate::tasks::run_pending(rt);
        if rt.has_threads() {
            game_wait(rt, Some(Instant::now() + time.min(GAME_SLEEP)), |_| false);
        }
        return V::Void;
    }
    let until = Instant::now() + time;
    loop {
        // (A wake that is no interrupt, `unpark`, leaves it asleep.)
        own_wait(rt, V::NULL, Some(until));
        if rt.thrown.is_some() || Instant::now() >= until {
            return V::Void;
        }
    }
}

/// `Object.wait`, a condition's `await`: until `on` is woken ([`wake`]) or `time` is over. On
/// the game's own thread the mods' threads run meanwhile, and a wait with no time given is a
/// short one ([`BARE_PATIENCE`]: it may end for no reason, as Java's may). Whether it was
/// woken.
pub(crate) fn wait_on(rt: &mut Rt, on: V, time: Option<Duration>) -> bool {
    let until = time.map(|t| Instant::now() + t);
    if rt.thread_id() != 0 {
        return own_wait(rt, on, until) == Waited::Woken && rt.thrown.is_none();
    }
    if crate::jsystem::in_started_thread() {
        crate::jsystem::wait(rt);
        return false;
    }
    crate::tasks::run_pending(rt);
    if !rt.has_threads() {
        return false;
    }
    rt.interned.insert(GAME_WAITS.into(), on);
    rt.interned.remove(GAME_WOKEN);
    let until = until.unwrap_or_else(|| Instant::now() + BARE_PATIENCE);
    let woken = game_wait(rt, Some(until), |rt| rt.interned.contains_key(GAME_WOKEN));
    rt.interned.remove(GAME_WAITS);
    rt.interned.remove(GAME_WOKEN);
    woken
}

/// Wakes who waits on `on` (`notify`, `signal`, a latch at zero, a future done): the first of
/// the mods' threads that does, or with `all` each, and the game's own thread when it is the
/// one waiting.
pub(crate) fn wake(rt: &mut Rt, on: V, all: bool) {
    if !rt.has_threads() || on.r() == 0 {
        return;
    }
    rt.thread_wake(on, all);
    if rt.thread_id() != 0 && rt.interned.get(GAME_WAITS) == Some(&on) {
        rt.interned.insert(GAME_WOKEN.into(), V::I(1));
    }
}

/// `LockSupport.park`: the thread that runs waits until it is unparked ([`unpark`]), `time` is
/// over, or it is interrupted (which stays to be told). A permit given before lets it pass at
/// once; and it may come back for no reason, as Java's may.
pub(crate) fn park(rt: &mut Rt, time: Option<Duration>) {
    if rt.thread_id() == 0 {
        if crate::jsystem::in_started_thread() {
            crate::jsystem::wait(rt);
            return;
        }
        crate::tasks::run_pending(rt);
        if rt.has_threads() {
            let time = time.unwrap_or(RECHECK).min(GAME_SLEEP);
            game_wait(rt, Some(Instant::now() + time), |_| false);
        }
        return;
    }
    let me = rt.thread_object();
    if rt.get_field(me, PERMIT).i() != 0 {
        rt.put_field(me, PERMIT, V::I(0));
        return;
    }
    if interrupted(rt, me) {
        return;
    }
    let look = time.unwrap_or(RECHECK).min(RECHECK * 5);
    rt.thread_wait(V::NULL, Some(Instant::now() + look));
    rt.put_field(me, PERMIT, V::I(0));
}

/// `LockSupport.unpark(thread)`.
pub(crate) fn unpark(rt: &mut Rt, thread: V) {
    let id = id_of(rt, thread);
    if id != 0 {
        rt.put_field(thread, PERMIT, V::I(1));
        rt.thread_unpark(id);
    }
}

/// `Thread.join`: until `thread` is done, or `time` is over. A thread that runs on no stack of
/// its own was done when it was started.
pub(crate) fn join(rt: &mut Rt, thread: V, time: Option<Duration>) {
    if !alive(rt, thread) {
        return;
    }
    let until = time.map(|t| Instant::now() + t);
    wait_until(rt, thread, until, |rt| !alive(rt, thread));
}

/// `Thread.yield`, `onSpinWait`: the others have their turn.
pub(crate) fn give_way(rt: &mut Rt) {
    if rt.thread_id() != 0 {
        rt.thread_yield();
    } else if !crate::jsystem::in_started_thread() {
        crate::tasks::run_pending(rt);
        if rt.has_threads() {
            rt.run_threads(SLICE);
        }
    }
}

thread_local! {
    /// When this thread of the game's last lent its runtime, and what it had told of by then.
    static LENT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    static TOLD: std::cell::Cell<Option<(Instant, Duration, u64)>> = const { std::cell::Cell::new(None) };
}

/// A number of milliseconds the environment gives under `name`, else `default`.
fn env_ms(name: &str, default: u64) -> Duration {
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default),
    )
}

/// How long the mods' threads run at least each time the game's thread lends them its runtime
/// (`HORIZON_COMPAT_THREAD_MS`, 3), and how long it goes on by itself between two such times
/// at least (`HORIZON_COMPAT_THREAD_EVERY_MS`, 10). They have a quarter of the time since the
/// last when that is more (a server lends once a tick): about a quarter of the thread for
/// them when they have work, none when they wait.
fn budget() -> (Duration, Duration) {
    static ASKED: std::sync::OnceLock<(Duration, Duration)> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| {
        (
            env_ms("HORIZON_COMPAT_THREAD_MS", 3),
            env_ms("HORIZON_COMPAT_THREAD_EVERY_MS", 10),
        )
    })
}

/// The game's thread is between two pieces of its own work (a frame drawn, a tick ended),
/// inside the game ([`Rt::with_host`]): the mods' threads have their turn, a bounded one. What
/// they gave the game's own thread to do meanwhile is done after.
pub(crate) fn lend(rt: &mut Rt) {
    rt.threads.lends += 1;
    if !rt.has_threads() {
        return;
    }
    let (run, every) = budget();
    let now = Instant::now();
    let since = LENT
        .get()
        .map_or(every, |last| now.saturating_duration_since(last));
    if since < every {
        return;
    }
    let run = run.max(since.min(Duration::from_millis(60)) / 4);
    // (From here the game's thread holds no value of its own: [`Rt::lend_threads`].)
    if rt.lend_threads(run) {
        // What they gave the game's own executor meanwhile is this thread's to run.
        crate::screens::run_client_tasks(rt);
        LENT.set(Some(Instant::now()));
    }
    tell(rt, now);
}

/// With `HORIZON_COMPAT_THREADS_LOG`: every ten seconds, how many threads the mods have, how
/// much of the game's thread they took, and how many whole collections waited for them.
fn tell(rt: &Rt, now: Instant) {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ASKED.get_or_init(|| std::env::var_os("HORIZON_COMPAT_THREADS_LOG").is_some()) {
        return;
    }
    let (ran, turns) = rt.threads.ran;
    let Some((since, ran_then, turns_then)) = TOLD.get() else {
        TOLD.set(Some((now, ran, turns)));
        return;
    };
    let over = now - since;
    if over < Duration::from_secs(10) {
        return;
    }
    TOLD.set(Some((now, ran, turns)));
    eprintln!(
        "horizon-compat: THREADS {}: {} of the mods', {:.1}% of this thread in {} turns over {:.0} s, {} whole collections put off, {} objects held ({} at the last whole collection)",
        if crate::neo::is_client(rt) {
            "client"
        } else {
            "server"
        },
        rt.thread_count(),
        (ran - ran_then).as_secs_f64() * 100.0 / over.as_secs_f64(),
        turns - turns_then,
        over.as_secs_f64(),
        rt.threads.put_off,
        rt.heap.len() - rt.free.len(),
        rt.live_at_collection,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call;

    const LATCH: &str = "java/util/concurrent/CountDownLatch";
    const COUNT_DOWN: &str = "java/util/concurrent/CountDownLatch.countDown.()V";
    const COUNT: &str = "java/util/concurrent/CountDownLatch.getCount.()J";
    const AWAIT: &str =
        "java/util/concurrent/CountDownLatch.await.(JLjava/util/concurrent/TimeUnit;)Z";
    const ALIVE: &str = "java/lang/Thread.isAlive.()Z";
    const MINECRAFT: &str = "net/minecraft/client/Minecraft";

    fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
        call(rt, key, args)
    }

    /// A runtime whose game thread lends it, as a running game's does.
    fn runtime() -> Rt {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(lambda);
        rt.threads.lends = 1;
        rt
    }

    fn kept(rt: &Rt, name: &str) -> V {
        rt.interned.get(name).copied().unwrap_or_default()
    }

    fn latch(rt: &mut Rt, count: i32) -> V {
        let latch = rt.new_object(LATCH);
        call(
            rt,
            "java/util/concurrent/CountDownLatch.<init>.(I)V",
            &[latch, V::I(count)],
        );
        latch
    }

    fn seconds(rt: &mut Rt) -> V {
        crate::api::get_static(rt, "java/util/concurrent/TimeUnit.SECONDS")
    }

    /// The mods' code of these tests, as the lambdas their threads and pools are given.
    fn lambda(rt: &mut Rt, id: u32, _args: &[V]) -> Option<V> {
        match id {
            // Sleeps, tells what thread it is on, and counts the latch down.
            1 => {
                call(rt, "java/lang/Thread.sleep.(J)V", &[V::J(30)]);
                let me = call(
                    rt,
                    "java/lang/Thread.currentThread.()Ljava/lang/Thread;",
                    &[],
                );
                rt.interned.insert("t:current".into(), me);
                let latch = kept(rt, "t:latch");
                call(rt, COUNT_DOWN, &[latch]);
            }
            // Waits on a monitor until it is told to go on, then counts the latch down.
            2 => {
                let lock = kept(rt, "t:lock");
                while kept(rt, "t:go").i() == 0 && rt.thrown.is_none() {
                    call(rt, "java/lang/Object.wait.()V", &[lock]);
                }
                let latch = kept(rt, "t:latch");
                call(rt, COUNT_DOWN, &[latch]);
            }
            // Gives the client's own executor a task, from a thread that is not the client's.
            3 => {
                let mc = call(
                    rt,
                    "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;",
                    &[],
                );
                let same = call(rt, "net/minecraft/client/Minecraft.isSameThread.()Z", &[mc]);
                rt.interned.insert("t:same".into(), same);
                let task = rt.lambda_id("t/Client.task", 4, Vec::new());
                call(
                    rt,
                    "net/minecraft/client/Minecraft.execute.(Ljava/lang/Runnable;)V",
                    &[mc, task],
                );
            }
            4 => {
                let on = V::I(rt.thread_id() as i32);
                rt.interned.insert("t:ran-on".into(), on);
            }
            // Sleeps until it is interrupted, and tells so.
            5 => {
                call(rt, "java/lang/Thread.sleep.(J)V", &[V::J(60_000)]);
                let how = match rt.thrown.take() {
                    Some(e) => rt.string(rt.class_of(e)),
                    None => V::NULL,
                };
                rt.unwinding.clear();
                rt.interned.insert("t:ended".into(), how);
            }
            // Reads its value of the thread-local kept for it, sets its own, and reads again.
            6 => {
                let local = kept(rt, "t:local");
                let before = call(rt, LOCAL_GET, &[local]);
                rt.interned.insert("t:before".into(), before);
                let mine = rt.string("the thread's");
                call(
                    rt,
                    "java/lang/ThreadLocal.set.(Ljava/lang/Object;)V",
                    &[local, mine],
                );
                let read = call(rt, LOCAL_GET, &[local]);
                rt.interned.insert("t:read".into(), read);
            }
            _ => {}
        }
        Some(V::Void)
    }

    const LOCAL_GET: &str = "java/lang/ThreadLocal.get.()Ljava/lang/Object;";

    /// A thread-local has a value for each thread: what the game's thread set is not what a
    /// thread of the mods' own reads, and what that thread sets is its own.
    #[test]
    fn a_thread_local_is_each_threads_own() {
        let mut rt = runtime();
        let local = rt.new_object("java/lang/ThreadLocal");
        rt.interned.insert("t:local".into(), local);
        let games = rt.string("the game's");
        call(
            &mut rt,
            "java/lang/ThreadLocal.set.(Ljava/lang/Object;)V",
            &[local, games],
        );
        rt.interned.insert("t:before".into(), games);
        let thread = start(&mut rt, "t/Local.run", 6);
        call(
            &mut rt,
            "java/lang/Thread.join.(J)V",
            &[thread, V::J(5_000)],
        );
        assert_eq!(kept(&rt, "t:before"), V::NULL, "none of its own yet");
        let read = kept(&rt, "t:read");
        assert_eq!(rt.str_of(read), Some("the thread's"));
        assert_eq!(call(&mut rt, LOCAL_GET, &[local]), games);
    }

    fn start(rt: &mut Rt, class: &'static str, id: u32) -> V {
        let body = rt.lambda_id(class, id, Vec::new());
        let thread = rt.new_object(THREAD);
        call(
            rt,
            "java/lang/Thread.<init>.(Ljava/lang/Runnable;)V",
            &[thread, body],
        );
        call(rt, "java/lang/Thread.start.()V", &[thread]);
        thread
    }

    /// A thread a mod starts in a running game does not run inside `start`: it runs beside the
    /// game, sleeps for as long as it asked to, and is its own `currentThread`. The game's own
    /// thread, waiting for what it does, lets it run meanwhile; `join` returns when it is done.
    #[test]
    fn a_started_thread_runs_beside_the_game_and_sleeps_where_it_asks() {
        let mut rt = runtime();
        let latch = latch(&mut rt, 1);
        rt.interned.insert("t:latch".into(), latch);
        let thread = start(&mut rt, "t/Body.run", 1);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(1), "not inside start");
        assert_eq!(call(&mut rt, ALIVE, &[thread]), V::I(1));
        let began = Instant::now();
        let unit = seconds(&mut rt);
        assert_eq!(call(&mut rt, AWAIT, &[latch, V::J(5), unit]), V::I(1));
        assert!(
            began.elapsed() >= Duration::from_millis(28),
            "{:?}",
            began.elapsed()
        );
        assert_eq!(kept(&rt, "t:current"), thread);
        call(&mut rt, "java/lang/Thread.join.()V", &[thread]);
        assert_eq!(call(&mut rt, ALIVE, &[thread]), V::I(0));
        let game = call(
            &mut rt,
            "java/lang/Thread.currentThread.()Ljava/lang/Thread;",
            &[],
        );
        assert_ne!(game, thread);
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// A thread that waits on a monitor goes on when it is notified, not before; and a latch
    /// nobody can count down is passed by the game's thread, which cannot stand still for it.
    #[test]
    fn a_wait_on_a_monitor_ends_with_its_notify() {
        let mut rt = runtime();
        let latch = latch(&mut rt, 1);
        let lock = rt.new_object("java/lang/Object");
        rt.interned.insert("t:latch".into(), latch);
        rt.interned.insert("t:lock".into(), lock);
        start(&mut rt, "t/Waiter.run", 2);
        for _ in 0..5 {
            lend_now(&mut rt);
        }
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(1), "it waits");
        // No thread can go on: the game's own does not wait for the latch.
        let began = Instant::now();
        let unit = seconds(&mut rt);
        assert_eq!(call(&mut rt, AWAIT, &[latch, V::J(5), unit]), V::I(0));
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "{:?}",
            began.elapsed()
        );
        rt.interned.insert("t:go".into(), V::I(1));
        call(&mut rt, "java/lang/Object.notifyAll.()V", &[lock]);
        assert_eq!(call(&mut rt, AWAIT, &[latch, V::J(5), unit]), V::I(1));
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// The game's thread lends its runtime, whatever the last time was.
    fn lend_now(rt: &mut Rt) {
        LENT.set(None);
        lend(rt);
    }

    /// What a thread of the mods' own gives the client's executor runs on the client's thread,
    /// when that next comes by, and the thread is told it is not the client's.
    #[test]
    fn a_task_for_the_client_runs_on_the_clients_thread() {
        let mut rt = runtime();
        rt.interned.insert("side:client".into(), V::I(1));
        rt.interned.insert("t:ran-on".into(), V::I(-1));
        start(&mut rt, "t/Asker.run", 3);
        for _ in 0..20 {
            lend_now(&mut rt);
            if kept(&rt, "t:ran-on").i() >= 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(kept(&rt, "t:same"), V::I(0));
        assert_eq!(kept(&rt, "t:ran-on"), V::I(0), "on the game's own thread");
        let mc = call(
            &mut rt,
            "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;",
            &[],
        );
        assert_eq!(rt.class_of(mc), MINECRAFT);
        assert_eq!(
            call(
                &mut rt,
                "net/minecraft/client/Minecraft.isSameThread.()Z",
                &[mc]
            ),
            V::I(1)
        );
        assert!(
            rt.thrown.is_none(),
            "{:?}",
            rt.thrown.map(|t| rt.class_of(t))
        );
    }

    /// A thread asleep is woken by its interrupt, which its sleep throws; and it is done.
    #[test]
    fn an_interrupt_ends_a_sleep() {
        let mut rt = runtime();
        let thread = start(&mut rt, "t/Sleeper.run", 5);
        for _ in 0..3 {
            lend_now(&mut rt);
        }
        assert_eq!(call(&mut rt, ALIVE, &[thread]), V::I(1));
        call(&mut rt, "java/lang/Thread.interrupt.()V", &[thread]);
        call(
            &mut rt,
            "java/lang/Thread.join.(J)V",
            &[thread, V::J(5_000)],
        );
        assert_eq!(call(&mut rt, ALIVE, &[thread]), V::I(0));
        let how = kept(&rt, "t:ended");
        assert_eq!(rt.str_of(how), Some("java/lang/InterruptedException"));
    }

    /// While the mods load, and on a runtime no thread of the game's lends, a started thread
    /// runs where it is started, as it did: done when `start` returns.
    #[test]
    fn with_no_thread_to_lend_a_started_thread_runs_inside_start() {
        let mut rt = runtime();
        rt.threads.lends = 0;
        let latch = latch(&mut rt, 1);
        rt.interned.insert("t:latch".into(), latch);
        rt.interned.insert("t:go".into(), V::I(1));
        let thread = start(&mut rt, "t/Waiter.run", 2);
        assert_eq!(call(&mut rt, COUNT, &[latch]), V::J(0));
        assert_eq!(call(&mut rt, ALIVE, &[thread]), V::I(0));
        assert!(!rt.has_threads());
    }
}
