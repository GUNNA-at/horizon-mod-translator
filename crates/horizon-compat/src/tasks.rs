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
use crate::{Data, Rt, V};

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

/// Gives a pool `work` (of `kind`) to run later, and the future of it.
pub(crate) fn later(rt: &mut Rt, work: V, kind: i32, result: V) -> V {
    let future = waiting(rt, work, kind, result);
    queue(rt, future);
    future
}

/// Puts a future whose work waits ([`waiting`]) in the pools' queue.
pub(crate) fn queue(rt: &mut Rt, future: V) {
    let queue = rt.intern(QUEUE.into(), |rt| crate::jutil::new_list(rt, Vec::new()));
    if let Some(Data::List(l)) = rt.heap.get_mut(queue.r() as usize).map(|o| &mut o.data) {
        l.push(future);
    }
}

/// Whether `future`'s work has not run yet.
pub(crate) fn waits(rt: &Rt, future: V) -> bool {
    future.r() != 0 && rt.get_field(future, TASK).r() != 0
}

/// Runs `future`'s work if it still waits: something is asked of the future, which a pool's
/// thread would have answered by now. What the work throws is the future's ([`FAILED`]), not
/// thrown at whoever asked.
pub(crate) fn force(rt: &mut Rt, future: V) {
    if !waits(rt, future) {
        return;
    }
    run(rt, future);
    if rt.thrown.take().is_some() {
        rt.unwinding.clear();
    }
}

/// `Future.cancel` of work that has not run: it never does. False for work that ran.
pub(crate) fn cancel(rt: &mut Rt, future: V) -> bool {
    if !waits(rt, future) {
        return false;
    }
    rt.put_field(future, TASK, V::Void);
    rt.put_field(future, CANCELLED, V::I(1));
    true
}

/// Set on a future while its work runs: a round of the queue begun inside that work (the work
/// waits for something itself) leaves it where it is, for the call running it to take out.
const RUNNING: &str = "$taskRunning";

/// Runs the work of `future`, which waits no longer whatever comes of it. The work is a
/// piece of the mods' code on a clock of its own, as a listener is: one that never ends is
/// stopped as any other piece is.
fn run(rt: &mut Rt, future: V) {
    rt.put_field(future, RUNNING, V::I(1));
    let outer = crate::api::timed_start();
    run_timed(rt, future);
    crate::api::timed_end(outer);
    rt.put_field(future, RUNNING, V::Void);
}

fn run_timed(rt: &mut Rt, future: V) {
    let work = rt.get_field(future, TASK);
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
    fn list(rt: &Rt, queue: V) -> &[V] {
        match rt.heap.get(queue.r() as usize).map(|o| &o.data) {
            Some(Data::List(l)) => l,
            _ => &[],
        }
    }
    let waiting = list(rt, queue).len();
    let mut ran = false;
    for _ in 0..waiting {
        let next = list(rt, queue)
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
        if let Some(Data::List(l)) = rt.heap.get_mut(queue.r() as usize).map(|o| &mut o.data)
            && let Some(at) = l.iter().position(|f| *f == future)
        {
            l.remove(at);
        }
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
