//! What a mod asks of the machine through the JDK, answered for a game that runs on one thread
//! and reaches no network: the threads it starts, the calendar and the clock, HTTP and URLs.
use crate::{Rt, V};

const THREAD: &str = "java/lang/Thread";
const RUN: &str = "java/lang/Runnable.run.()V";
const CALENDAR: &str = "java/util/GregorianCalendar";
const DATE: &str = "java/util/Date";
const URL: &str = "java/net/URL";
/// A `URI` is kept as a `URL` is: its text.
const URI: &str = "java/net/URI";
const URL_DECODER: &str = "java/net/URLDecoder";
const URL_ENCODER: &str = "java/net/URLEncoder";

/// `URLDecoder.decode`: `+` is a space, `%xx` a byte of the text's UTF-8; a `%` without two
/// hex digits after it is the error Java makes of it.
fn form_decoded(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => out.push(byte),
                    None => {
                        return Err(match hex {
                            Some(h) => format!(
                                "URLDecoder: Illegal hex characters in escape (%) pattern - Error at index 0 in: \"{h}\""
                            ),
                            None => "URLDecoder: Incomplete trailing escape (%) pattern".to_owned(),
                        });
                    }
                }
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `URLEncoder.encode`: letters, digits and `.-*_` as they are, a space as `+`, every other
/// byte of the text's UTF-8 as `%XX`.
fn form_encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
const RESPONSE: &str = "java/net/http/HttpResponse";
const IO_EXCEPTION: &str = "java/io/IOException";
const OFFLINE: &str = "a mod has no network here";

thread_local! {
    /// How deep in started threads' bodies the runtime is, and how often the innermost has
    /// waited ([`wait`]).
    static STARTED: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
    /// The threads whose bodies run where they were started, innermost last.
    static INLINE: std::cell::RefCell<Vec<V>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What ends the body of a thread that waits where it was started, in a game: an error no
/// `catch (InterruptedException)` takes for its own, as a JVM once stopped a thread
/// ([`wait`]). The thread is started again when the game runs.
const LATER: &str = "java/lang/ThreadDeath";

/// A thread's body told to stop this often that waits again is stopped by an error instead: it
/// swallows the interrupt and would never end.
const WAITS: u32 = 3;

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key
/// only to the modules that claim it ([`crate::api`]'s chain). `Thread`'s and the calendar's come
/// through the modules of their packages ([`thread`], [`calendar`]).
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner == CHARSET
        || owner.starts_with("java/net/http/")
        || owner.starts_with(LOCKS)
        || matches!(
            owner,
            URL | URI
                | URL_DECODER
                | URL_ENCODER
                | "java/net/URLConnection"
                | "java/net/HttpURLConnection"
                | "javax/net/ssl/HttpsURLConnection"
                | LATCH
                | "java/util/concurrent/Semaphore"
                | PRIVILEGED
        )
        || socket(owner)
}

/// `AccessController`, whose `doPrivileged` is the action run: there is no security manager
/// whose checks it would lift.
const PRIVILEGED: &str = "java/security/AccessController";

/// The sockets and channels a mod would reach the network or listen on it with.
fn socket(owner: &str) -> bool {
    matches!(
        owner,
        "java/net/Socket"
            | "java/net/ServerSocket"
            | "java/net/DatagramSocket"
            | "java/net/MulticastSocket"
            | "java/nio/channels/SocketChannel"
            | "java/nio/channels/ServerSocketChannel"
            | "java/nio/channels/DatagramChannel"
            | "java/nio/channels/AsynchronousSocketChannel"
            | "java/nio/channels/AsynchronousServerSocketChannel"
    )
}

/// A socket or a channel of one, with no network for a mod: it can be made unconnected and
/// closed, and whatever would open, bind, connect or wait on it fails as on a machine offline,
/// so a server a mod starts for itself (KubeJS's, a remote console) ends in the mod's own
/// handling and no thread of it spins on the game's.
fn sockets(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    Some(match name {
        "<init>" if args.len() <= 1 => V::Void,
        "close"
        | "setReuseAddress"
        | "setSoTimeout"
        | "setTcpNoDelay"
        | "setKeepAlive"
        | "setReceiveBufferSize"
        | "setSendBufferSize" => V::Void,
        "isClosed" => V::I(1),
        "isOpen" | "isBound" | "isConnected" | "isBlocking" => V::I(0),
        _ if desc.ends_with(")V") || desc.ends_with(';') || name == "<init>" => {
            crate::jlang::throw_with(rt, IO_EXCEPTION, OFFLINE)
        }
        _ => return None,
    })
}

const LOCKS: &str = "java/util/concurrent/locks/";
const LATCH: &str = "java/util/concurrent/CountDownLatch";
const SYNCHRONIZER: &str = "java/util/concurrent/locks/AbstractQueuedSynchronizer";

/// The kind of what a method with descriptor `desc` returns.
fn return_kind(desc: &str) -> char {
    crate::jlang::return_kind(desc)
}

/// `AbstractQueuedSynchronizer`, under a lock or a latch of a mod's own (polylib carries
/// Sentry's reusable count latch): its state, and its acquiring and releasing, which ask the
/// mod's class whether they may (`tryAcquire`, `tryReleaseShared`...) and never queue anyone,
/// there being one thread. An acquire the class refuses is a wait ([`wait`]).
fn synchronizer(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let asks = |rt: &mut Rt, what: &'static str| rt.call_functional(what, this, &[a(1)]);
    Some(match name {
        "getState" => V::I(rt.get_field(this, "$state").i()),
        "setState" => {
            rt.put_field(this, "$state", V::I(a(1).i()));
            V::Void
        }
        "compareAndSetState" => {
            let same = rt.get_field(this, "$state").i() == a(1).i();
            if same {
                rt.put_field(this, "$state", V::I(a(2).i()));
            }
            V::I(i32::from(same))
        }
        // An acquire the class refuses waits for a release, and asks again: with threads of
        // the mods' own another of them may release it meanwhile.
        "acquire"
        | "acquireInterruptibly"
        | "tryAcquireNanos"
        | "acquireShared"
        | "acquireSharedInterruptibly"
        | "tryAcquireSharedNanos" => {
            let shared = name.contains("Shared");
            let what = if shared {
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.tryAcquireShared.(I)I"
            } else {
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.tryAcquire.(I)Z"
            };
            let granted = move |v: V| if shared { v.i() >= 0 } else { v.i() != 0 };
            let mut had = granted(asks(rt, what));
            if !had && rt.thrown.is_none() {
                let until = name.ends_with("Nanos").then(|| {
                    std::time::Instant::now()
                        + std::time::Duration::from_nanos(a(2).j().max(0) as u64)
                });
                let arg = a(1);
                had = crate::mod_threads::wait_until(rt, this, until, |rt| {
                    granted(rt.call_functional(what, this, &[arg])) || rt.thrown.is_some()
                }) && rt.thrown.is_none();
            }
            if name.ends_with("Nanos") {
                V::I(i32::from(had))
            } else {
                V::Void
            }
        }
        "release" | "releaseShared" => {
            let what = if name == "release" {
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.tryRelease.(I)Z"
            } else {
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.tryReleaseShared.(I)Z"
            };
            let released = asks(rt, what).i();
            if released != 0 {
                crate::mod_threads::wake(rt, this, true);
            }
            V::I(released)
        }
        "hasQueuedThreads" | "hasContended" | "hasQueuedPredecessors" | "isQueued" => V::I(0),
        "getQueueLength" => V::I(0),
        "setExclusiveOwnerThread" => {
            rt.put_field(this, "$owner", a(1));
            V::Void
        }
        "getExclusiveOwnerThread" => match rt.get_field(this, "$owner") {
            V::Void => V::NULL,
            thread => thread,
        },
        _ => return None,
    })
}

/// `java.util.concurrent`'s locks, latches and semaphores, on the one thread there is: a lock
/// is always free and held by whoever asks, a read or write lock is the lock itself, and nothing
/// is ever waited for (a latch not counted down yet is passed, as after a timeout; in a thread a
/// mod started, a wait interrupts it: [`wait`]).
fn locks(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    if owner == SYNCHRONIZER
        && let Some(v) = synchronizer(rt, name, args)
    {
        return Some(v);
    }
    // A `StampedLock`'s stamps: every lock is had at once, and every stamp is good.
    if owner == "java/util/concurrent/locks/StampedLock" {
        match (name, return_kind(desc)) {
            ("isWriteLocked" | "isReadLocked", _) => return Some(V::I(0)),
            (_, 'J') => return Some(V::J(1)),
            (_, 'Z') => return Some(V::I(1)),
            ("unlockWrite" | "unlockRead" | "unlock", _) => return Some(V::Void),
            ("asReadWriteLock", _) => return Some(this),
            _ => {}
        }
    }
    Some(match name {
        "<init>" => {
            if owner == LATCH || owner.ends_with("Semaphore") {
                rt.put_field(this, "$count", V::J(i64::from(a(1).i())));
            }
            V::Void
        }
        "countDown" => {
            let left = (rt.get_field(this, "$count").j() - 1).max(0);
            rt.put_field(this, "$count", V::J(left));
            if left == 0 {
                crate::mod_threads::wake(rt, this, true);
            }
            V::Void
        }
        "getCount" => V::J(rt.get_field(this, "$count").j()),
        "availablePermits" => V::I(rt.get_field(this, "$count").j() as i32),
        // A latch is waited for until it is at zero (a thread of the mods' own really waits;
        // the game's lets them run meanwhile, and passes when none can count it down).
        "await" if owner == LATCH => {
            let until = desc.starts_with("(J").then(|| {
                std::time::Instant::now() + crate::api_common::unit_duration(rt, a(1).j(), a(2))
            });
            let zero = |rt: &mut Rt| rt.get_field(this, "$count").j() == 0;
            let had = crate::mod_threads::wait_until(rt, this, until, zero);
            if desc.ends_with(")Z") {
                V::I(had as i32)
            } else {
                V::Void
            }
        }
        // `LockSupport.park`: until unparked, or for the time given.
        "park" | "parkNanos" | "parkUntil" => {
            let time = match (name, args.last().copied()) {
                ("parkNanos", Some(V::J(n))) => {
                    Some(std::time::Duration::from_nanos(n.max(0) as u64))
                }
                ("parkUntil", Some(V::J(at))) => {
                    Some(std::time::Duration::from_millis((at - now()).max(0) as u64))
                }
                _ => None,
            };
            crate::mod_threads::park(rt, time);
            V::Void
        }
        // A condition's: until it is signalled, or for the time given.
        "await" | "awaitNanos" | "awaitUninterruptibly" | "awaitUntil" => {
            let began = std::time::Instant::now();
            let time = match (name, a(1)) {
                ("awaitNanos", V::J(n)) => Some(std::time::Duration::from_nanos(n.max(0) as u64)),
                ("await", V::J(d)) => Some(crate::api_common::unit_duration(rt, d, a(2))),
                _ => None,
            };
            let woken = crate::mod_threads::wait_on(rt, this, time);
            match desc.rsplit(')').next() {
                Some("Z") => V::I(woken as i32),
                // What is left of the time given: none when it ran out.
                Some("J") => V::J(match time {
                    Some(t) if woken => t.saturating_sub(began.elapsed()).as_nanos().max(1) as i64,
                    _ => 0,
                }),
                _ => V::Void,
            }
        }
        // A permit may be the pools' work's to release: that runs first, as beside a wait.
        "acquire" | "acquireUninterruptibly" => {
            crate::tasks::run_pending(rt);
            V::Void
        }
        // One thread runs at a time, so a lock is always free: what it guards is done in one
        // piece all the same, the thread that holds it keeping its turn until it lets go
        // ([`Rt::monitor_enter`]).
        "lock" | "lockInterruptibly" => {
            rt.monitor_enter(this);
            V::Void
        }
        "unlock" => {
            rt.monitor_exit(this);
            V::Void
        }
        "tryLock" => {
            rt.monitor_enter(this);
            V::I(1)
        }
        "signal" | "signalAll" => {
            crate::mod_threads::wake(rt, this, name == "signalAll");
            V::Void
        }
        "unpark" => {
            crate::mod_threads::unpark(rt, this);
            V::Void
        }
        "release" => V::Void,
        "tryAcquire" | "isHeldByCurrentThread" | "isWriteLockedByCurrentThread" => V::I(1),
        "isLocked" | "hasQueuedThreads" | "isWriteLocked" | "isFair" => V::I(0),
        "getHoldCount" | "getQueueLength" | "getReadLockCount" => V::I(0),
        "readLock" | "writeLock" | "asReadLock" | "asWriteLock" if desc.starts_with("()L") => this,
        "newCondition" => rt.new_object("java/util/concurrent/locks/Condition"),
        _ => return None,
    })
}

/// Whether the runtime is inside the body of a thread a mod started.
pub(crate) fn in_started_thread() -> bool {
    STARTED.with(|s| s.get().0 > 0)
}

/// A wait (`Thread.sleep`, a lock's) where no thread of the mods' own runs
/// ([`crate::mod_threads`] has those). On the game's own thread it is over at once, the work
/// the pools hold run meanwhile as their threads would run it beside the one waiting
/// ([`crate::tasks`]): what the code waits for may be that work's doing. In the body of a
/// thread a mod started, which runs where it was started ([`thread`]), it can't be waited out:
/// the body ends there ([`LATER`]), and the thread is started again, from its beginning, on a
/// stack of its own once the game's thread lends its runtime
/// ([`crate::mod_threads::start_later`]): a loop that takes work from a queue, or sleeps between
/// two looks, lives then as it does on a JVM. (Where no game ever runs, a probe or a test, that
/// is the end of it. A wait asked with no such thread known is an interrupt, as before.)
pub(crate) fn wait(rt: &mut Rt) -> V {
    let (depth, waits) = STARTED.with(|s| s.get());
    if depth == 0 {
        crate::tasks::run_pending(rt);
        return V::Void;
    }
    STARTED.with(|s| s.set((depth, waits + 1)));
    if let Some(thread) = INLINE.with(|i| i.borrow().last().copied()) {
        crate::mod_threads::start_later(rt, thread);
        return crate::jlang::throw_with(
            rt,
            LATER,
            "a thread a mod started waits: it goes on when the game runs, on a stack of its own",
        );
    }
    let class = if waits < WAITS { "java/lang/InterruptedException" } else { "java/lang/Error" };
    crate::jlang::throw_with(rt, class, "a thread a mod started runs on the game's thread and can't wait there")
}

/// A descriptor's parameter types (`Ljava/lang/String;`, `I`, `[B`), in order.
fn params(desc: &str) -> Vec<&str> {
    let list = desc.strip_prefix('(').and_then(|d| d.split(')').next()).unwrap_or("");
    let (mut out, mut at) = (Vec::new(), 0);
    while at < list.len() {
        let rest = &list[at..];
        let dims = rest.len() - rest.trim_start_matches('[').len();
        let len = dims + if rest[dims..].starts_with('L') { rest[dims..].find(';').map_or(rest.len() - dims, |i| i + 1) } else { 1 };
        out.push(&rest[..len]);
        at += len;
    }
    out
}

/// The class a descriptor's method returns, if it returns an object.
fn returned(desc: &str) -> Option<&str> {
    desc.rsplit(')').next()?.strip_prefix('L')?.strip_suffix(';')
}

const THREAD_BUILDER: &str = "java/lang/Thread$Builder";

/// A thread builder (`Thread.ofVirtual()`, `ofPlatform()`): the name it gives, and the threads
/// it makes, which are threads as any a mod makes ([`thread`]); what it would set of their
/// scheduling has nothing to act on.
pub(crate) fn thread_builder(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
        "name" => {
            rt.put_field(this, "$name", a(1));
            this
        }
        "daemon"
        | "priority"
        | "group"
        | "inheritInheritableThreadLocals"
        | "uncaughtExceptionHandler"
        | "stackSize" => this,
        "unstarted" | "start" => {
            let made = rt.new_object(THREAD);
            rt.put_field(made, "$target", a(1));
            let named = rt.get_field(this, "$name");
            if rt.str_of(named).is_some() {
                rt.put_field(made, "$name", named);
            }
            if name == "start" {
                thread(rt, "start", "()V", &[made]);
            }
            made
        }
        "factory" => rt.new_object("java/util/concurrent/ThreadFactory"),
        _ => return None,
    })
}

/// The `Thread` the game's own thread is to the mods.
pub(crate) fn game_thread(rt: &mut Rt) -> V {
    rt.intern("compat:thread".into(), |rt| rt.new_object(THREAD))
}

/// `java.lang.Thread`. A runtime runs on one thread of the game's: there `currentThread` is that
/// one each time. A thread a mod makes and starts runs on a stack of its own where the mods have
/// threads ([`crate::mod_threads`]: in a running game), beside the game, and waits where its code
/// waits. Where they have none (while they load) it runs its body at once, there, to its end or
/// to its first wait: a task done in the background is done when `start` returns, and a loop that
/// polls and sleeps ends at its first sleep, interrupted ([`wait`]).
pub(crate) fn thread(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let game = game_thread;
    let run = |rt: &mut Rt, thread: V| {
        // A subclass's own `run`, else the body the thread was made with.
        let body = if rt.class_of(thread) == THREAD { rt.get_field(thread, "$target") } else { thread };
        if body.r() == 0 {
            return;
        }
        let outer = STARTED.with(|s| s.replace((s.get().0 + 1, 0)));
        INLINE.with(|i| i.borrow_mut().push(thread));
        // Its body is a piece of its own for the calls nothing maps: a loop of it that was
        // stopped for them leaves what started the thread its own count.
        let misses = (
            std::mem::take(&mut rt.hook_misses),
            rt.hook_first_miss.take(),
        );
        rt.call_functional(RUN, body, &[]);
        (rt.hook_misses, rt.hook_first_miss) = misses;
        STARTED.with(|s| s.set(outer));
        INLINE.with(|i| i.borrow_mut().pop());
        // What ended it stays in it, as on a JVM: an interrupt silently (and what ends a body
        // that is to go on later), anything else named.
        if let Some(t) = rt.thrown.take()
            && !matches!(rt.class_of(t), "java/lang/InterruptedException" | LATER)
        {
            let message = rt.get_field(t, "message");
            eprintln!(
                "horizon-compat: a thread a mod started ended with {}: {}",
                rt.class_of(t),
                rt.str_of(message).unwrap_or("")
            );
        }
    };
    Some(match name {
        "currentThread" => match crate::mod_threads::current(rt) {
            Some(own) => own,
            None => game(rt),
        },
        // The arguments by their types: a group, the body, a name, a stack size.
        "<init>" => {
            for (i, p) in params(desc).into_iter().enumerate() {
                match p {
                    "Ljava/lang/Runnable;" => rt.put_field(this, "$target", a(i + 1)),
                    "Ljava/lang/String;" => rt.put_field(this, "$name", a(i + 1)),
                    _ => {}
                }
            }
            V::Void
        }
        // On a stack of its own where the mods have threads ([`crate::mod_threads`]); else
        // here and now.
        "start" => {
            if rt.get_field(this, "$started").i() == 0 {
                rt.put_field(this, "$started", V::I(1));
                if !crate::mod_threads::start(rt, this) {
                    run(rt, this);
                }
            }
            V::Void
        }
        // `Thread.run()` itself (a subclass's `super.run()`): the body it was made with.
        "run" => {
            let body = rt.get_field(this, "$target");
            if body.r() != 0 {
                rt.call_functional(RUN, body, &[]);
            }
            V::Void
        }
        "startVirtualThread" => {
            let t = rt.new_object(THREAD);
            rt.put_field(t, "$target", a(0));
            rt.put_field(t, "$started", V::I(1));
            if !crate::mod_threads::start(rt, t) {
                run(rt, t);
            }
            t
        }
        "getName" => match rt.get_field(this, "$name") {
            n if rt.str_of(n).is_some() => n,
            _ if this == game(rt) => rt.string(if crate::neo::is_client(rt) { "Render thread" } else { "Server thread" }),
            _ => rt.string(&format!("Thread-{}", this.r())),
        },
        "setName" => {
            rt.put_field(this, "$name", a(1));
            V::Void
        }
        "setDaemon" => {
            rt.put_field(this, "$daemon", a(1));
            V::Void
        }
        "isDaemon" => V::I(rt.get_field(this, "$daemon").i()),
        "getId" | "threadId" => V::J(if this == game(rt) { 1 } else { i64::from(this.r()) }),
        "getPriority" => V::I(5),
        // The game's thread runs, and a thread on a stack of its own until its body ends; one
        // a mod started with none ran to its end inside `start`.
        "isAlive" => V::I((this == game(rt) || crate::mod_threads::alive(rt, this)) as i32),
        // On a stack of its own a thread is interrupted when a mod interrupts it. One started
        // with none is always being asked to stop: it can't share the game's thread.
        "isInterrupted" if crate::mod_threads::id_of(rt, this) != 0 => {
            V::I(crate::mod_threads::interrupted(rt, this) as i32)
        }
        "interrupted" if rt.thread_id() != 0 => V::I(crate::mod_threads::take_interrupt(rt) as i32),
        "isInterrupted" | "interrupted" => V::I(in_started_thread() as i32),
        // `sleep(millis)`, `sleep(millis, nanos)`, `sleep(Duration)`.
        "sleep" => {
            let time = match a(0) {
                V::J(ms) => std::time::Duration::from_millis(ms.max(0) as u64),
                d => std::time::Duration::from_millis(
                    crate::api_common::duration_millis(rt, d).max(0) as u64,
                ),
            };
            crate::mod_threads::sleep(rt, time)
        }
        "holdsLock" => V::I(1),
        // No handler but a mod's own, which is asked nothing here: what ends a thread's body
        // is told of where it ends.
        "getDefaultUncaughtExceptionHandler" | "getUncaughtExceptionHandler" => V::NULL,
        "ofVirtual" | "ofPlatform" => rt.new_object(THREAD_BUILDER),
        "getStackTrace" => crate::jlang::thread_frames(rt),
        "getThreadGroup" => crate::neo::thread_group(rt),
        "getContextClassLoader" => rt.intern("compat:class-loader".into(), |rt| rt.new_object("java/lang/ClassLoader")),
        // Giving the other threads a turn: the mods' own, and the pools' work
        // ([`crate::tasks`]).
        "yield" | "onSpinWait" => {
            crate::mod_threads::give_way(rt);
            V::Void
        }
        // `join()`, `join(millis)`, `join(millis, nanos)`: until the thread is done.
        "join" => {
            let time = match a(1) {
                V::J(ms) if ms > 0 => Some(std::time::Duration::from_millis(ms as u64)),
                _ => None,
            };
            crate::mod_threads::join(rt, this, time);
            V::Void
        }
        "interrupt" => {
            crate::mod_threads::interrupt(rt, this);
            V::Void
        }
        "setPriority" | "setUncaughtExceptionHandler" | "setDefaultUncaughtExceptionHandler" | "setContextClassLoader" | "checkAccess" => {
            V::Void
        }
        _ => return None,
    })
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The days from 1970-01-01 to a proleptic Gregorian date (`month` from 1).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400));
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// A moment's calendar fields in UTC: year, month (from 0, as `Calendar` counts), day, hour,
/// minute, second, millisecond.
fn fields(millis: i64) -> [i64; 7] {
    let (days, ms) = (millis.div_euclid(86_400_000), millis.rem_euclid(86_400_000));
    let (year, month, day) = crate::api_common::civil_date(days);
    [i64::from(year), i64::from(month) - 1, i64::from(day), ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000]
}

/// The moment of calendar fields ([`fields`]); a month or a time past its range carries over, as
/// a lenient `Calendar` takes them.
fn moment(f: [i64; 7]) -> i64 {
    let (year, month) = (f[0] + f[1].div_euclid(12), f[1].rem_euclid(12) + 1);
    (days_from_civil(year, month, 1) + f[2] - 1) * 86_400_000 + f[3] * 3_600_000 + f[4] * 60_000 + f[5] * 1000 + f[6]
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// From Monday, as `DayOfWeek` counts.
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// The moment `millis` (UTC, the one zone here) written by a pattern of Java's date letters, as
/// `SimpleDateFormat` and `DateTimeFormatter` both read them: a run of a letter is a field, its
/// length the width or the form (`MM` 03, `MMM` Mar, `MMMM` March), text between `'` is itself.
/// A run of `S` is the fraction of the second for `DateTimeFormatter` (`fraction`), the
/// milliseconds for `SimpleDateFormat`.
pub(crate) fn format_moment(pattern: &str, millis: i64, fraction: bool) -> String {
    let f = fields(millis);
    let (year, month, day, hour, minute, second, ms) =
        (f[0], f[1] + 1, f[2], f[3], f[4], f[5], f[6]);
    let days = millis.div_euclid(86_400_000);
    // 1970-01-01 was a Thursday.
    let weekday = (days + 3).rem_euclid(7) as usize;
    let day_of_year = days - days_from_civil(year, 1, 1) + 1;
    let padded = |n: i64, width: usize| format!("{n:0width$}");
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            // `''` is a quote; else text up to the closing one.
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let n = chars[i..].iter().take_while(|x| **x == c).count();
        i += n;
        match c {
            'G' => out.push_str("AD"),
            'y' | 'u' | 'Y' if n == 2 => out.push_str(&padded(year.rem_euclid(100), 2)),
            'y' | 'u' | 'Y' => out.push_str(&padded(year, n)),
            'M' | 'L' if n >= 4 => out.push_str(MONTH_NAMES[(month - 1) as usize]),
            'M' | 'L' if n == 3 => out.push_str(&MONTH_NAMES[(month - 1) as usize][..3]),
            'M' | 'L' => out.push_str(&padded(month, n)),
            'd' => out.push_str(&padded(day, n)),
            'D' => out.push_str(&padded(day_of_year, n)),
            'E' if n >= 4 => out.push_str(WEEKDAYS[weekday]),
            'E' => out.push_str(&WEEKDAYS[weekday][..3]),
            'e' | 'c' => out.push_str(&padded(weekday as i64 + 1, n)),
            'a' => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            'H' => out.push_str(&padded(hour, n)),
            'k' => out.push_str(&padded(if hour == 0 { 24 } else { hour }, n)),
            'K' => out.push_str(&padded(hour % 12, n)),
            'h' => out.push_str(&padded(if hour % 12 == 0 { 12 } else { hour % 12 }, n)),
            'm' => out.push_str(&padded(minute, n)),
            's' => out.push_str(&padded(second, n)),
            'S' if fraction => out.push_str(&format!("{:09}", ms * 1_000_000)[..n.min(9)]),
            'S' => out.push_str(&padded(ms, n)),
            'n' => out.push_str(&padded(ms * 1_000_000, n)),
            'w' => out.push_str(&padded((day_of_year + 6) / 7, n)),
            'z' | 'V' | 'v' => out.push_str("UTC"),
            'Z' if n >= 4 => out.push_str("GMT"),
            'Z' => out.push_str("+0000"),
            'X' => out.push('Z'),
            'x' => out.push_str(if n == 1 { "+00" } else { "+0000" }),
            'O' => out.push_str("GMT"),
            _ => {}
        }
    }
    out
}

/// The moment `text` spells under `pattern`: its numbers by the fields' letters, a month by
/// its name too, the rest matched as written. None where the text does not fit the pattern.
pub(crate) fn parse_moment(pattern: &str, text: &str) -> Option<i64> {
    let (pattern, text): (Vec<char>, Vec<char>) =
        (pattern.chars().collect(), text.chars().collect());
    let mut f = [1970i64, 0, 1, 0, 0, 0, 0];
    let (mut pm, mut twelve) = (false, false);
    let (mut i, mut at) = (0, 0);
    while i < pattern.len() {
        let c = pattern[i];
        if c == '\'' {
            i += 1;
            while i < pattern.len() && pattern[i] != '\'' {
                if text.get(at) != Some(&pattern[i]) {
                    return None;
                }
                (i, at) = (i + 1, at + 1);
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            if text.get(at) != Some(&c) {
                return None;
            }
            (i, at) = (i + 1, at + 1);
            continue;
        }
        let n = pattern[i..].iter().take_while(|x| **x == c).count();
        i += n;
        // A field next to another takes its own width; one before a separator takes the digits there are.
        let adjacent = pattern.get(i).is_some_and(|x| x.is_ascii_alphabetic());
        let number = |at: &mut usize| -> Option<i64> {
            let start = *at;
            while *at < text.len() && text[*at].is_ascii_digit() && (!adjacent || *at - start < n) {
                *at += 1;
            }
            text[start..*at].iter().collect::<String>().parse().ok()
        };
        match c {
            'y' | 'u' | 'Y' => {
                let year = number(&mut at)?;
                f[0] = if n == 2 && year < 100 {
                    2000 + year
                } else {
                    year
                };
            }
            'M' | 'L' if n >= 3 => {
                let rest: String = text[at..].iter().collect();
                let (m, name) = MONTH_NAMES.iter().enumerate().find_map(|(m, name)| {
                    let short = &name[..3];
                    if n >= 4 && rest.starts_with(name) {
                        Some((m, name.len()))
                    } else if rest.starts_with(short) {
                        Some((m, 3))
                    } else {
                        None
                    }
                })?;
                f[1] = m as i64;
                at += name;
            }
            'M' | 'L' => f[1] = number(&mut at)? - 1,
            'd' => f[2] = number(&mut at)?,
            'H' | 'k' => f[3] = number(&mut at)? % 24,
            'h' | 'K' => {
                f[3] = number(&mut at)? % 12;
                twelve = true;
            }
            'm' => f[4] = number(&mut at)?,
            's' => f[5] = number(&mut at)?,
            'S' => f[6] = number(&mut at)?,
            'a' => {
                let rest: String = text[at..].iter().take(2).collect();
                pm = rest.eq_ignore_ascii_case("PM");
                if !pm && !rest.eq_ignore_ascii_case("AM") {
                    return None;
                }
                at += 2;
            }
            // A zone or a weekday's name: read past, the time is taken as UTC.
            _ => {
                while at < text.len()
                    && (text[at].is_ascii_alphanumeric() || matches!(text[at], '+' | '-' | ':'))
                {
                    at += 1;
                }
            }
        }
    }
    if twelve && pm {
        f[3] += 12;
    }
    (at == text.len()).then(|| moment(f))
}

/// The moment a date or time object stands for, in milliseconds since 1970: a `Date`, a
/// `Calendar`, an `Instant`, a `LocalDateTime` or a `ZonedDateTime` (each keeps one), a
/// `LocalDate` (its midnight), a `LocalTime` (that time on the first day).
pub(crate) fn moment_of(rt: &Rt, v: V) -> i64 {
    if let V::J(millis) = rt.get_field(v, "$millis") {
        return millis;
    }
    if let V::J(nanos) = rt.get_field(v, "$nanoOfDay") {
        return nanos / 1_000_000;
    }
    match (
        rt.get_field(v, "year"),
        rt.get_field(v, "month"),
        rt.get_field(v, "day"),
    ) {
        (V::I(year), V::I(month), V::I(day)) => {
            days_from_civil(i64::from(year), i64::from(month), i64::from(day)) * 86_400_000
        }
        _ => now(),
    }
}

const SIMPLE_FORMAT: &str = "java/text/SimpleDateFormat";
const FORMATTER: &str = "java/time/format/DateTimeFormatter";
const INSTANT: &str = "java/time/Instant";
const LOCAL_DATE_TIME: &str = "java/time/LocalDateTime";

/// An object of `class` that keeps a moment.
fn at_moment(rt: &mut Rt, class: &'static str, millis: i64) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "$millis", V::J(millis));
    o
}

/// A `DateTimeFormatter` (or a `SimpleDateFormat`, `class`) of `pattern`.
fn formatter(rt: &mut Rt, class: &'static str, pattern: &str) -> V {
    let f = rt.new_object(class);
    let text = rt.string(pattern);
    rt.put_field(f, "$pattern", text);
    f
}

/// `DateTimeFormatter`'s constants a mod writes a time with.
pub(crate) fn formatter_static(rt: &mut Rt, field: &str) -> Option<V> {
    let pattern = match field {
        "ISO_LOCAL_DATE" | "ISO_DATE" => "yyyy-MM-dd",
        "ISO_LOCAL_TIME" | "ISO_TIME" => "HH:mm:ss",
        "ISO_LOCAL_DATE_TIME" | "ISO_DATE_TIME" => "yyyy-MM-dd'T'HH:mm:ss",
        "ISO_INSTANT" | "ISO_OFFSET_DATE_TIME" | "ISO_ZONED_DATE_TIME" => {
            "yyyy-MM-dd'T'HH:mm:ss'Z'"
        }
        "BASIC_ISO_DATE" => "yyyyMMdd",
        "RFC_1123_DATE_TIME" => "EEE, d MMM yyyy HH:mm:ss 'GMT'",
        _ => return None,
    };
    Some(rt.intern(format!("compat:date-formatter/{field}"), |rt| {
        formatter(rt, FORMATTER, pattern)
    }))
}

/// `SimpleDateFormat` and `DateFormat`: a moment written and read by a pattern, in UTC.
pub(crate) fn date_format(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let pattern = |rt: &Rt, v: V| {
        rt.str_of(rt.get_field(v, "$pattern"))
            .unwrap_or("M/d/yy, h:mm a")
            .to_owned()
    };
    Some(match name {
        "<init>" => {
            let given = if desc.starts_with("(Ljava/lang/String;") {
                crate::jlang::text(rt, a(1))
            } else {
                "M/d/yy, h:mm a".to_owned()
            };
            let text = rt.string(&given);
            rt.put_field(a(0), "$pattern", text);
            V::Void
        }
        "getDateTimeInstance" | "getInstance" => {
            formatter(rt, SIMPLE_FORMAT, "MMM d, yyyy, h:mm:ss a")
        }
        "getDateInstance" => formatter(rt, SIMPLE_FORMAT, "MMM d, yyyy"),
        "getTimeInstance" => formatter(rt, SIMPLE_FORMAT, "h:mm:ss a"),
        "format" if desc.ends_with(")Ljava/lang/String;") => {
            let (p, millis) = (pattern(rt, a(0)), moment_of(rt, a(1)));
            rt.string(&format_moment(&p, millis, false))
        }
        "parse" if desc == "(Ljava/lang/String;)Ljava/util/Date;" => {
            let (p, text) = (pattern(rt, a(0)), crate::jlang::text(rt, a(1)));
            match parse_moment(&p, &text) {
                Some(millis) => at_moment(rt, DATE, millis),
                None => crate::jlang::throw_with(
                    rt,
                    "java/text/ParseException",
                    &format!("Unparseable date: \"{text}\""),
                ),
            }
        }
        "toPattern" | "toLocalizedPattern" => rt.get_field(a(0), "$pattern"),
        "applyPattern" => {
            rt.put_field(a(0), "$pattern", a(1));
            V::Void
        }
        "setTimeZone" | "setLenient" | "setCalendar" => V::Void,
        "getTimeZone" => time_zone(rt, "getDefault", "", &[])?,
        _ => return None,
    })
}

/// `TimeZone`, `ZoneId` and `ZoneOffset`: the zone asked for by its id, every one of them at
/// UTC's time (the runtime keeps one clock).
pub(crate) fn time_zone(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let zone = |rt: &mut Rt, class: &'static str, id: &str| {
        let z = rt.new_object(class);
        let text = rt.string(id);
        rt.put_field(z, "$id", text);
        z
    };
    let id = |rt: &Rt, v: V| {
        rt.str_of(rt.get_field(v, "$id"))
            .unwrap_or("UTC")
            .to_owned()
    };
    Some(match name {
        "getTimeZone" => {
            let asked = if desc.starts_with("(Ljava/lang/String;") {
                crate::jlang::text(rt, a(0))
            } else {
                id(rt, a(0))
            };
            zone(rt, "java/util/TimeZone", &asked)
        }
        "getDefault" => zone(rt, "java/util/TimeZone", "UTC"),
        "systemDefault" => zone(rt, "java/time/ZoneId", "UTC"),
        "of" | "ofOffset" => {
            let asked = crate::jlang::text(rt, a(0));
            zone(rt, "java/time/ZoneId", &asked)
        }
        "toZoneId" | "normalized" => {
            let same = id(rt, a(0));
            zone(rt, "java/time/ZoneId", &same)
        }
        "getID" | "getId" | "getDisplayName" | "toString" => {
            let text = id(rt, a(0));
            rt.string(&text)
        }
        "getRawOffset" | "getOffset" | "getDSTSavings" | "getTotalSeconds" => V::I(0),
        "inDaylightTime" | "useDaylightTime" | "observesDaylightTime" => V::I(0),
        "clone" => a(0),
        "setDefault" | "setID" => V::Void,
        "equals" => V::I((a(1).r() != 0 && id(rt, a(0)) == id(rt, a(1))) as i32),
        "hashCode" => V::I(crate::jlang::string_hash(&id(rt, a(0)))),
        _ => return None,
    })
}

/// `Instant`, `LocalDateTime`, `ZonedDateTime` and `DateTimeFormatter`: a moment kept in
/// milliseconds, read by its fields, and written by a pattern.
pub(crate) fn java_time(
    rt: &mut Rt,
    owner: &'static str,
    name: &str,
    desc: &str,
    args: &[V],
) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == FORMATTER {
        return Some(match name {
            "ofPattern" => {
                let pattern = crate::jlang::text(rt, a(0));
                formatter(rt, FORMATTER, &pattern)
            }
            "format" => {
                let pattern = rt
                    .str_of(rt.get_field(a(0), "$pattern"))
                    .unwrap_or("yyyy-MM-dd'T'HH:mm:ss")
                    .to_owned();
                let millis = moment_of(rt, a(1));
                rt.string(&format_moment(&pattern, millis, true))
            }
            "withZone" | "withLocale" | "withChronology" | "withResolverStyle" => a(0),
            "toString" => rt.get_field(a(0), "$pattern"),
            _ => return None,
        });
    }
    let this = a(0);
    let ints = params(desc).iter().take_while(|p| **p == "I").count();
    Some(match name {
        "now" => at_moment(rt, owner, now()),
        "ofEpochMilli" => at_moment(rt, owner, a(0).j()),
        "ofEpochSecond" => at_moment(rt, owner, a(0).j().saturating_mul(1000)),
        // `LocalDateTime.of(year, month, day, hour, minute[, second])`, or of a date and a time.
        "of" if ints >= 5 => {
            let mut f = [0i64; 7];
            for (i, slot) in f.iter_mut().enumerate().take(ints.min(6)) {
                *slot = i64::from(a(i).i());
            }
            f[1] -= 1;
            at_moment(rt, owner, moment(f))
        }
        "of" | "ofInstant" => {
            let millis = moment_of(rt, a(0)) + if name == "of" { moment_of(rt, a(1)) } else { 0 };
            at_moment(rt, owner, millis)
        }
        "from"
        | "atZone"
        | "atOffset"
        | "toLocalDateTime"
        | "toInstant"
        | "withZoneSameInstant"
        | "toOffsetDateTime"
        | "atZoneSameInstant" => {
            let class: &'static str = match name {
                "toLocalDateTime" => LOCAL_DATE_TIME,
                "toInstant" => INSTANT,
                "atZone" | "atOffset" | "atZoneSameInstant" | "withZoneSameInstant" => {
                    "java/time/ZonedDateTime"
                }
                "toOffsetDateTime" => "java/time/OffsetDateTime",
                _ => owner,
            };
            let millis = moment_of(rt, this);
            at_moment(rt, class, millis)
        }
        "getEpochSecond" | "toEpochSecond" => V::J(moment_of(rt, this).div_euclid(1000)),
        "toEpochMilli" => V::J(moment_of(rt, this)),
        "getNano" => V::I((moment_of(rt, this).rem_euclid(1000) * 1_000_000) as i32),
        "getYear" => V::I(fields(moment_of(rt, this))[0] as i32),
        "getMonthValue" => V::I(fields(moment_of(rt, this))[1] as i32 + 1),
        "getDayOfMonth" => V::I(fields(moment_of(rt, this))[2] as i32),
        "getHour" => V::I(fields(moment_of(rt, this))[3] as i32),
        "getMinute" => V::I(fields(moment_of(rt, this))[4] as i32),
        "getSecond" => V::I(fields(moment_of(rt, this))[5] as i32),
        "getDayOfYear" => {
            let millis = moment_of(rt, this);
            let days = millis.div_euclid(86_400_000);
            V::I((days - days_from_civil(fields(millis)[0], 1, 1) + 1) as i32)
        }
        "plusMillis" | "plusSeconds" | "plusMinutes" | "plusHours" | "plusDays" | "plusWeeks"
        | "minusMillis" | "minusSeconds" | "minusMinutes" | "minusHours" | "minusDays"
        | "minusWeeks" => {
            let unit = match name.trim_start_matches("plus").trim_start_matches("minus") {
                "Millis" => 1,
                "Seconds" => 1000,
                "Minutes" => 60_000,
                "Hours" => 3_600_000,
                "Days" => 86_400_000,
                _ => 604_800_000,
            };
            let by = a(1).j().saturating_mul(unit);
            let millis = moment_of(rt, this);
            at_moment(
                rt,
                rt.class_of(this),
                if name.starts_with("plus") {
                    millis.saturating_add(by)
                } else {
                    millis.saturating_sub(by)
                },
            )
        }
        "isAfter" => V::I((moment_of(rt, this) > moment_of(rt, a(1))) as i32),
        "isBefore" => V::I((moment_of(rt, this) < moment_of(rt, a(1))) as i32),
        "compareTo" => V::I(moment_of(rt, this).cmp(&moment_of(rt, a(1))) as i32),
        "equals" => V::I(
            (a(1).r() != 0
                && rt.class_of(a(1)) == rt.class_of(this)
                && moment_of(rt, this) == moment_of(rt, a(1))) as i32,
        ),
        "hashCode" => V::I(moment_of(rt, this) as i32),
        "format" => {
            let pattern = rt
                .str_of(rt.get_field(a(1), "$pattern"))
                .unwrap_or("yyyy-MM-dd'T'HH:mm:ss")
                .to_owned();
            let millis = moment_of(rt, this);
            rt.string(&format_moment(&pattern, millis, true))
        }
        // ISO-8601, as each prints itself: an `Instant` with its `Z`, the fraction where it
        // is not zero.
        "toString" => {
            let millis = moment_of(rt, this);
            let mut text = format_moment("yyyy-MM-dd'T'HH:mm:ss", millis, true);
            if millis.rem_euclid(1000) != 0 {
                text.push_str(&format!(".{:03}", millis.rem_euclid(1000)));
            }
            if owner != LOCAL_DATE_TIME {
                text.push('Z');
            }
            rt.string(&text)
        }
        _ => return None,
    })
}

/// Where `Calendar`'s field constants are in [`fields`], and for the day's others how many
/// milliseconds one is.
fn field(constant: i32) -> Result<usize, i64> {
    match constant {
        1 => Ok(0),
        2 => Ok(1),
        5 => Ok(2),
        10 | 11 => Ok(3),
        12 => Ok(4),
        13 => Ok(5),
        14 => Ok(6),
        // The week's and the year's days, weeks: a day or seven of them.
        3 | 4 => Err(7 * 86_400_000),
        _ => Err(86_400_000),
    }
}

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `java.util.Calendar` (the Gregorian one, which `getInstance` gives) and `java.util.Date`: a
/// moment in milliseconds, read in UTC (the runtime keeps no time zones).
pub(crate) fn calendar(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let at = |rt: &Rt, v: V| rt.get_field(v, "$millis").j();
    let made = |rt: &mut Rt, class: &'static str, millis: i64| {
        let o = rt.new_object(class);
        rt.put_field(o, "$millis", V::J(millis));
        o
    };
    let set = |rt: &mut Rt, millis: i64| {
        rt.put_field(this, "$millis", V::J(millis));
        V::Void
    };
    let ints = params(desc).iter().take_while(|p| **p == "I").count();
    Some(match (owner == DATE, name) {
        (false, "getInstance") => made(rt, CALENDAR, now()),
        (true, "<init>") if desc == "(J)V" => set(rt, a(1).j()),
        // `new GregorianCalendar(year, month, day[, hour, minute[, second]])`.
        (false, "<init>") if ints >= 3 => {
            let mut f = [0; 7];
            for (i, slot) in f.iter_mut().enumerate().take(ints) {
                *slot = i64::from(a(i + 1).i());
            }
            set(rt, moment(f))
        }
        (_, "<init>") => set(rt, now()),
        (true, "getTime") | (false, "getTimeInMillis") => V::J(at(rt, this)),
        (true, "setTime") | (false, "setTimeInMillis") => set(rt, a(1).j()),
        (false, "getTime") => made(rt, DATE, at(rt, this)),
        (false, "setTime") => set(rt, at(rt, a(1))),
        (_, "after") => V::I((at(rt, this) > at(rt, a(1))) as i32),
        (_, "before") => V::I((at(rt, this) < at(rt, a(1))) as i32),
        (_, "compareTo") => V::I(at(rt, this).cmp(&at(rt, a(1))) as i32),
        (_, "equals") => V::I((rt.class_of(this) == rt.class_of(a(1)) && at(rt, this) == at(rt, a(1))) as i32),
        (_, "hashCode") => V::I((at(rt, this) ^ (at(rt, this) >> 32)) as i32),
        (_, "clone") => made(rt, if owner == DATE { DATE } else { CALENDAR }, at(rt, this)),
        (false, "get") => {
            let (millis, f) = (at(rt, this), fields(at(rt, this)));
            let days = millis.div_euclid(86_400_000);
            let year_day = days - days_from_civil(f[0], 1, 1);
            V::I(match a(1).i() {
                0 => 1,
                // The week of the year and of the month, counted from their first days.
                3 => year_day / 7 + 1,
                4 => (f[2] - 1) / 7 + 1,
                6 => year_day + 1,
                // Sunday is 1; 1970 began on a Thursday.
                7 => (days + 4).rem_euclid(7) + 1,
                8 => (f[2] - 1) / 7 + 1,
                9 => f[3] / 12,
                10 => f[3] % 12,
                15 | 16 => 0,
                c => field(c).map_or(0, |i| f[i]),
            } as i32)
        }
        (false, "set") if ints == 2 => {
            let mut f = fields(at(rt, this));
            match field(a(1).i()) {
                Ok(i) => f[i] = i64::from(a(2).i()),
                Err(_) => return None,
            }
            set(rt, moment(f))
        }
        (false, "set") if ints >= 3 => {
            let mut f = fields(at(rt, this));
            for (i, slot) in f.iter_mut().enumerate().take(ints) {
                *slot = i64::from(a(i + 1).i());
            }
            set(rt, moment(f))
        }
        (false, "add") => {
            let (millis, amount) = (at(rt, this), i64::from(a(2).i()));
            match field(a(1).i()) {
                Ok(i) => {
                    let mut f = fields(millis);
                    f[i] += amount;
                    // A month or a year on, the day stays in the month it lands in.
                    if i < 2 {
                        let (y, m) = (f[0] + f[1].div_euclid(12), f[1].rem_euclid(12));
                        let length = days_from_civil(y + (m + 1) / 12, (m + 1) % 12 + 1, 1) - days_from_civil(y, m + 1, 1);
                        f[2] = f[2].min(length);
                    }
                    set(rt, moment(f))
                }
                Err(unit) => set(rt, millis + amount * unit),
            }
        }
        (false, "getActualMaximum") => {
            let f = fields(at(rt, this));
            V::I(match a(1).i() {
                1 => 292_278_994,
                2 => 11,
                5 => (days_from_civil(f[0] + (f[1] + 1) / 12, (f[1] + 1) % 12 + 1, 1) - days_from_civil(f[0], f[1] + 1, 1)) as i32,
                6 => (days_from_civil(f[0] + 1, 1, 1) - days_from_civil(f[0], 1, 1)) as i32,
                7 => 7,
                10 => 11,
                11 => 23,
                12 | 13 => 59,
                14 => 999,
                _ => return None,
            })
        }
        // `Tue Nov 10 05:34:37 UTC 2026`, as `Date.toString` prints.
        (true, "toString") => {
            let (millis, f) = (at(rt, this), fields(at(rt, this)));
            let day = DAYS[millis.div_euclid(86_400_000).rem_euclid(7) as usize];
            rt.string(&format!("{day} {} {:02} {:02}:{:02}:{:02} UTC {}", MONTHS[f[1] as usize], f[2], f[3], f[4], f[5], f[0]))
        }
        _ => return None,
    })
}

/// A `URI` of `spec`, or the `URL` (`class`).
pub(crate) fn locator(rt: &mut Rt, class: &'static str, spec: &str) -> V {
    let u = rt.new_object(class);
    let text = rt.string(spec);
    rt.put_field(u, "$spec", text);
    u
}

/// The text of a `URI` or `URL`.
pub(crate) fn locator_text(rt: &Rt, u: V) -> Option<String> {
    rt.str_of(rt.get_field(u, "$spec")).map(str::to_owned)
}

/// What a `URI` has that a `URL` has not, or under another name: made by `create` or from its
/// parts, absent parts answered `null`, another reference resolved against it. The parts both
/// have are read as the `URL`'s are (None here).
fn uri_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let spec = |rt: &Rt, v: V| rt.str_of(rt.get_field(v, "$spec")).unwrap_or("").to_owned();
    // The text of an argument; none for `null`.
    let given = |rt: &Rt, v: V| (v.r() != 0).then(|| crate::jlang::text(rt, v));
    let scheme = |s: &str| {
        s.split_once(':')
            .map(|(scheme, _)| scheme)
            .filter(|scheme| {
                !scheme.is_empty()
                    && scheme
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                    && !scheme.contains('/')
            })
            .map(str::to_owned)
    };
    Some(match name {
        "create" => {
            let text = crate::jlang::text(rt, a(0));
            locator(rt, URI, &text)
        }
        "<init>" => {
            let text = match desc {
                "(Ljava/lang/String;)V" => crate::jlang::text(rt, a(1)),
                // `(scheme, userInfo, host, port, path, query, fragment)`.
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V" =>
                {
                    let mut out = String::new();
                    if let Some(scheme) = given(rt, a(1)) {
                        out.push_str(&format!("{scheme}:"));
                    }
                    if let Some(host) = given(rt, a(3)) {
                        out.push_str("//");
                        if let Some(user) = given(rt, a(2)) {
                            out.push_str(&format!("{user}@"));
                        }
                        out.push_str(&host);
                        if a(4).i() >= 0 {
                            out.push_str(&format!(":{}", a(4).i()));
                        }
                    }
                    out.push_str(&given(rt, a(5)).unwrap_or_default());
                    if let Some(query) = given(rt, a(6)) {
                        out.push_str(&format!("?{query}"));
                    }
                    if let Some(fragment) = given(rt, a(7)) {
                        out.push_str(&format!("#{fragment}"));
                    }
                    out
                }
                // `(scheme, host, path, fragment)` and `(scheme, schemeSpecificPart, fragment)`.
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V" => {
                    let mut out = given(rt, a(1)).map_or(String::new(), |s| format!("{s}:"));
                    if let Some(host) = given(rt, a(2)) {
                        out.push_str(&format!("//{host}"));
                    }
                    out.push_str(&given(rt, a(3)).unwrap_or_default());
                    if let Some(fragment) = given(rt, a(4)) {
                        out.push_str(&format!("#{fragment}"));
                    }
                    out
                }
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V" => {
                    let mut out = given(rt, a(1)).map_or(String::new(), |s| format!("{s}:"));
                    out.push_str(&given(rt, a(2)).unwrap_or_default());
                    if let Some(fragment) = given(rt, a(3)) {
                        out.push_str(&format!("#{fragment}"));
                    }
                    out
                }
                _ => return None,
            };
            let text = rt.string(&text);
            rt.put_field(a(0), "$spec", text);
            V::Void
        }
        "toURL" => {
            let s = spec(rt, a(0));
            locator(rt, URL, &s)
        }
        "toASCIIString" => rt.get_field(a(0), "$spec"),
        "getScheme" => match scheme(&spec(rt, a(0))) {
            Some(s) => rt.string(&s),
            None => V::NULL,
        },
        "isAbsolute" => V::I(scheme(&spec(rt, a(0))).is_some() as i32),
        "isOpaque" => {
            let s = spec(rt, a(0));
            V::I(scheme(&s).is_some_and(|scheme| !s[scheme.len() + 1..].starts_with('/')) as i32)
        }
        "getSchemeSpecificPart" | "getRawSchemeSpecificPart" => {
            let s = spec(rt, a(0));
            let rest = match scheme(&s) {
                Some(scheme) => s[scheme.len() + 1..].to_owned(),
                None => s,
            };
            let rest = rest.split('#').next().unwrap_or("").to_owned();
            rt.string(&rest)
        }
        "getFragment" | "getRawFragment" => match spec(rt, a(0)).split_once('#') {
            Some((_, fragment)) => rt.string(fragment),
            None => V::NULL,
        },
        "normalize" | "parseServerAuthority" => a(0),
        // Another reference against this one: one with a scheme stands for itself, one from
        // the root replaces the path, any other the last name of the path.
        "resolve" => {
            let (base, other) = (
                spec(rt, a(0)),
                if rt.class_of(a(1)) == URI {
                    spec(rt, a(1))
                } else {
                    crate::jlang::text(rt, a(1))
                },
            );
            let resolved = if scheme(&other).is_some() || base.is_empty() {
                other
            } else {
                let base = base.split(['?', '#']).next().unwrap_or("");
                let path_at = match base.find("://") {
                    Some(i) => base[i + 3..].find('/').map_or(base.len(), |j| i + 3 + j),
                    None => scheme(base).map_or(0, |s| s.len() + 1),
                };
                if other.starts_with('/') {
                    format!("{}{other}", &base[..path_at])
                } else {
                    let dir = base[path_at..]
                        .rfind('/')
                        .map_or(path_at, |i| path_at + i + 1);
                    format!(
                        "{}{}{other}",
                        &base[..dir],
                        if dir == path_at && path_at == base.len() && base.contains("://") {
                            "/"
                        } else {
                            ""
                        }
                    )
                }
            };
            locator(rt, URI, &resolved)
        }
        "compareTo" => V::I(match spec(rt, a(0)).cmp(&spec(rt, a(1))) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        "equals" => V::I((rt.class_of(a(1)) == URI && spec(rt, a(0)) == spec(rt, a(1))) as i32),
        "getRawPath" | "getRawQuery" | "getAuthority" | "getRawAuthority" | "getUserInfo" => {
            let s = spec(rt, a(0));
            let after = scheme(&s).map_or(s.as_str(), |scheme| &s[scheme.len() + 1..]);
            let (authority, file) = match after.strip_prefix("//") {
                Some(rest) => rest.split_at(rest.find('/').unwrap_or(rest.len())),
                None => ("", after),
            };
            let file = file.split('#').next().unwrap_or("");
            let part = match name {
                "getRawPath" => Some(file.split('?').next().unwrap_or("").to_owned()),
                "getRawQuery" => file.split_once('?').map(|p| p.1.to_owned()),
                "getUserInfo" => authority.rsplit_once('@').map(|p| p.0.to_owned()),
                _ => (!authority.is_empty()).then(|| authority.to_owned()),
            };
            match part {
                Some(text) => rt.string(&text),
                None => V::NULL,
            }
        }
        _ => return None,
    })
}

/// `java.net.http` and `java.net.URL`'s connections, on a machine with no network for a mod: a
/// request is built as the mod builds it and then fails as one does offline (an `IOException`
/// where it is sent and waited for, a 503 where it is sent to be answered later), so an update
/// check or a statistics ping ends in the mod's own handling of that.
const CHARSET: &str = "java/nio/charset/Charset";

/// `Charset`: the default one (UTF-8, as every Java since 18 has it), one by any of its names,
/// and its name. A name no charset here has is `UnsupportedCharsetException`, as the JDK's.
fn charsets(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "defaultCharset" => crate::api_common::charset(rt, "UTF-8"),
        "forName" | "isSupported" => {
            let asked = rt.str_of(a(0)).unwrap_or("").to_owned();
            match (crate::api_common::charset_name(&asked), name) {
                (found, "isSupported") => V::I(found.is_some() as i32),
                (Some(canonical), _) => crate::api_common::charset(rt, canonical),
                (None, _) => {
                    rt.supers.insert(
                        "java/nio/charset/UnsupportedCharsetException",
                        "java/lang/IllegalArgumentException",
                    );
                    crate::jlang::throw_with(
                        rt,
                        "java/nio/charset/UnsupportedCharsetException",
                        &asked,
                    )
                }
            }
        }
        "name" | "displayName" | "toString" => rt.get_field(a(0), "$name"),
        "canEncode" | "isRegistered" => V::I(1),
        "equals" => V::I((a(0) == a(1)) as i32),
        "hashCode" => {
            let n = rt
                .str_of(rt.get_field(a(0), "$name"))
                .unwrap_or("")
                .to_owned();
            V::I(
                n.encode_utf16()
                    .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c))),
            )
        }
        "compareTo" => {
            let n = |rt: &Rt, v: V| {
                rt.str_of(rt.get_field(v, "$name"))
                    .unwrap_or("")
                    .to_ascii_lowercase()
            };
            V::I(n(rt, a(0)).cmp(&n(rt, a(1))) as i32)
        }
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if owner == CHARSET {
        return charsets(rt, name, args);
    }
    if owner.starts_with(LOCKS) || matches!(owner, LATCH | "java/util/concurrent/Semaphore") {
        return locks(rt, owner, name, desc, args);
    }
    if socket(owner) {
        return sockets(rt, name, desc, args);
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // `AccessController.doPrivileged(action[, context])`: what the action's `run()` gives (an
    // exception of a `PrivilegedExceptionAction` is the caller's to catch, as it is thrown).
    if owner == PRIVILEGED {
        if name != "doPrivileged" {
            return None;
        }
        let run = if desc.starts_with("(Ljava/security/PrivilegedExceptionAction;") {
            "java/security/PrivilegedExceptionAction.run.()Ljava/lang/Object;"
        } else {
            "java/security/PrivilegedAction.run.()Ljava/lang/Object;"
        };
        return Some(rt.call_functional(run, a(0), &[]));
    }
    // `URLDecoder.decode(text, charset)` and `URLEncoder.encode(text, charset)`: a form's
    // coding, in UTF-8 whatever charset is named (what mods name).
    if owner == URL_DECODER && name == "decode" {
        let text = crate::jlang::text(rt, a(0));
        return Some(match form_decoded(&text) {
            Ok(decoded) => rt.string(&decoded),
            Err(why) => crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &why),
        });
    }
    if owner == URL_ENCODER && name == "encode" {
        let text = crate::jlang::text(rt, a(0));
        return Some(rt.string(&form_encoded(&text)));
    }
    let offline = |rt: &mut Rt| crate::jlang::throw_with(rt, IO_EXCEPTION, OFFLINE);
    if owner == URI
        && let Some(v) = uri_call(rt, name, desc, args)
    {
        return Some(v);
    }
    if owner == URL || owner == URI {
        let spec = |rt: &Rt| rt.str_of(rt.get_field(a(0), "$spec")).unwrap_or("").to_owned();
        // `scheme://host[:port]/path?query#fragment`, as far as the text has them.
        let part = |rt: &Rt, what: &str| {
            let s = spec(rt);
            let (scheme, rest) = s.split_once(':').unwrap_or(("", s.as_str()));
            let rest = rest.strip_prefix("//").unwrap_or(rest);
            let (authority, file) = if s.contains("://") { rest.split_at(rest.find('/').unwrap_or(rest.len())) } else { ("", rest) };
            let file = file.split('#').next().unwrap_or("");
            match what {
                "getProtocol" => scheme.to_owned(),
                "getHost" => authority.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_owned(),
                "getPort" => authority.rsplit_once(':').map_or("-1", |p| p.1).to_owned(),
                "getPath" => file.split('?').next().unwrap_or("").to_owned(),
                "getQuery" => file.split_once('?').map_or("", |p| p.1).to_owned(),
                _ => file.to_owned(),
            }
        };
        return Some(match name {
            // `new URL(spec)`, `new URL(protocol, host[, port], file)`.
            "<init>" => {
                let texts: Vec<String> = params(desc).iter().enumerate().map(|(i, p)| if *p == "I" { a(i + 1).i().to_string() } else { crate::jlang::text(rt, a(i + 1)) }).collect();
                let spec = match texts.as_slice() {
                    [protocol, host, port, file] => format!("{protocol}://{host}:{port}{file}"),
                    [protocol, host, file] => format!("{protocol}://{host}{file}"),
                    other => other.last().cloned().unwrap_or_default(),
                };
                let spec = rt.string(&spec);
                rt.put_field(a(0), "$spec", spec);
                V::Void
            }
            "toString" | "toExternalForm" => {
                let s = spec(rt);
                rt.string(&s)
            }
            // `url.toURI()`, and `URL.of(uri, handler)`: the same text, as the other kind.
            "toURI" => {
                let s = spec(rt);
                locator(rt, URI, &s)
            }
            "of" if desc.starts_with("(Ljava/net/URI;") => {
                let s = spec(rt);
                locator(rt, URL, &s)
            }
            "getPort" => V::I(part(rt, name).parse().unwrap_or(-1)),
            "getDefaultPort" => V::I(if spec(rt).starts_with("https:") { 443 } else { 80 }),
            // (A `URI` answers `null` for a host it has none of.)
            "getHost" if owner == URI && part(rt, name).is_empty() => V::NULL,
            "getProtocol" | "getHost" | "getPath" | "getFile" => {
                let s = part(rt, name);
                rt.string(&s)
            }
            "getQuery" | "getRef" => {
                let s = if name == "getRef" { spec(rt).split_once('#').map_or(String::new(), |p| p.1.to_owned()) } else { part(rt, name) };
                if s.is_empty() { V::NULL } else { rt.string(&s) }
            }
            "equals" => V::I((rt.class_of(a(1)) == URL && spec(rt) == rt.str_of(rt.get_field(a(1), "$spec")).unwrap_or("")) as i32),
            "hashCode" => V::I(crate::jlang::string_hash(&spec(rt))),
            // A file of a mod's own jar (`Class.getResource`) opens; anything else is the network.
            "openStream" if spec(rt).starts_with("modjar:/") => {
                let path = spec(rt)["modjar:/".len()..].to_owned();
                crate::jlang::jar_resource(rt, &path, false)
            }
            "openConnection" if spec(rt).starts_with("modjar:/") => {
                let c = rt.new_object("java/net/URLConnection");
                let s = rt.get_field(a(0), "$spec");
                rt.put_field(c, "$spec", s);
                c
            }
            "openConnection" | "openStream" | "getContent" => offline(rt),
            _ => return None,
        });
    }
    if !owner.starts_with("java/net/http/") {
        // A connection can be had only to a file of a mod's jar: its bytes, and no more is asked.
        let spec = rt
            .str_of(rt.get_field(a(0), "$spec"))
            .unwrap_or("")
            .to_owned();
        return match (name, spec.strip_prefix("modjar:/")) {
            ("getInputStream", Some(path)) => Some(crate::jlang::jar_resource(rt, path, false)),
            (
                "connect" | "setUseCaches" | "setConnectTimeout" | "setReadTimeout" | "disconnect",
                Some(_),
            ) => Some(V::Void),
            _ => None,
        };
    }
    let simple = owner.rsplit('/').next().unwrap_or(owner);
    Some(match name {
        "<init>" => V::Void,
        "send" => offline(rt),
        // Answered later: by a server that can't be reached.
        "sendAsync" => {
            let response = rt.new_object(RESPONSE);
            crate::jutil::completed_future(rt, response)
        }
        "statusCode" if simple == "HttpResponse" => V::I(503),
        "body" if simple == "HttpResponse" => rt.string(""),
        "close" | "shutdown" | "shutdownNow" => V::Void,
        // A builder's step gives the builder back; what makes something gives one of its kind.
        _ => match returned(desc) {
            Some(class) if class == owner && args.len() == params(desc).len() + 1 => a(0),
            Some(class) if class.starts_with("java/net/http/") => {
                let class = rt.static_key(class);
                rt.new_object(class)
            }
            _ => return None,
        },
    })
}

#[cfg(test)]
mod tests {

    /// `Charset.defaultCharset()` is UTF-8 (every Java since 18), `forName` finds a charset by
    /// any of its names whatever their case, and one no Java runtime need have is
    /// `UnsupportedCharsetException`.
    #[test]
    fn charsets_are_found_by_name() {
        let mut rt = Rt::new();
        let utf8 = crate::api::get_static(&mut rt, "java/nio/charset/StandardCharsets.UTF_8");
        assert_eq!(
            crate::api::call(
                &mut rt,
                "java/nio/charset/Charset.defaultCharset.()Ljava/nio/charset/Charset;",
                &[]
            ),
            utf8
        );
        let for_name =
            "java/nio/charset/Charset.forName.(Ljava/lang/String;)Ljava/nio/charset/Charset;";
        for (asked, canonical) in [
            ("utf8", "UTF-8"),
            ("UTF-8", "UTF-8"),
            ("latin1", "ISO-8859-1"),
            ("ascii", "US-ASCII"),
            ("UTF-16le", "UTF-16LE"),
        ] {
            let n = rt.string(asked);
            let c = crate::api::call(&mut rt, for_name, &[n]);
            let name = crate::api::call(
                &mut rt,
                "java/nio/charset/Charset.name.()Ljava/lang/String;",
                &[c],
            );
            assert_eq!(rt.str_of(name), Some(canonical), "{asked}");
        }
        let n = rt.string("utf8");
        assert_eq!(
            crate::api::call(&mut rt, for_name, &[n]),
            utf8,
            "one object per charset"
        );
        let odd = rt.string("x-klingon");
        assert_eq!(
            crate::api::call(
                &mut rt,
                "java/nio/charset/Charset.isSupported.(Ljava/lang/String;)Z",
                &[odd]
            ),
            V::I(0)
        );
        crate::api::call(&mut rt, for_name, &[odd]);
        let e = rt.thrown.take().expect("no such charset");
        assert!(rt.instance_of(e, "java/lang/IllegalArgumentException"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
    use super::*;

    /// A moment written and read by Java's date patterns, by `SimpleDateFormat` and by
    /// `DateTimeFormatter`; an `Instant`, a `LocalDateTime` and a time zone (UTC's time, under
    /// whatever id).
    #[test]
    fn dates_are_written_and_read_by_their_patterns() {
        crate::install();
        let mut rt = Rt::new();
        let text = |rt: &Rt, v: V| rt.str_of(v).unwrap_or("?").to_owned();
        // 2024-02-29T13:05:09.007Z, a Thursday.
        let millis = moment([2024, 1, 29, 13, 5, 9, 7]);
        let date = rt.new_object(DATE);
        c(&mut rt, "java/util/Date.<init>.(J)V", &[date, V::J(millis)]);
        let simple = |rt: &mut Rt, pattern: &str| {
            let (f, p) = (
                rt.new_object("java/text/SimpleDateFormat"),
                rt.string(pattern),
            );
            c(
                rt,
                "java/text/SimpleDateFormat.<init>.(Ljava/lang/String;)V",
                &[f, p],
            );
            f
        };
        let plain = simple(&mut rt, "yyyy-MM-dd HH:mm:ss.SSS");
        let written = c(
            &mut rt,
            "java/text/SimpleDateFormat.format.(Ljava/util/Date;)Ljava/lang/String;",
            &[plain, date],
        );
        assert_eq!(text(&rt, written), "2024-02-29 13:05:09.007");
        let worded = simple(&mut rt, "EEE, d MMM yy 'at' h:mm a, EEEE MMMM");
        let written = c(
            &mut rt,
            "java/text/DateFormat.format.(Ljava/util/Date;)Ljava/lang/String;",
            &[worded, date],
        );
        assert_eq!(
            text(&rt, written),
            "Thu, 29 Feb 24 at 1:05 PM, Thursday February"
        );
        let spelled = rt.string("2024-02-29 13:05:09.007");
        let read = c(
            &mut rt,
            "java/text/SimpleDateFormat.parse.(Ljava/lang/String;)Ljava/util/Date;",
            &[plain, spelled],
        );
        assert_eq!(
            c(&mut rt, "java/util/Date.getTime.()J", &[read]),
            V::J(millis)
        );
        let nonsense = rt.string("tomorrow");
        c(
            &mut rt,
            "java/text/SimpleDateFormat.parse.(Ljava/lang/String;)Ljava/util/Date;",
            &[plain, nonsense],
        );
        assert_eq!(
            rt.thrown.take().map(|e| rt.class_of(e)),
            Some("java/text/ParseException")
        );

        let pattern = rt.string("dd/MM/yyyy HH:mm");
        let formatter = c(
            &mut rt,
            "java/time/format/DateTimeFormatter.ofPattern.(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
            &[pattern],
        );
        let at = c(
            &mut rt,
            "java/time/LocalDateTime.of.(IIIII)Ljava/time/LocalDateTime;",
            &[V::I(2024), V::I(2), V::I(29), V::I(13), V::I(5)],
        );
        let written = c(
            &mut rt,
            "java/time/LocalDateTime.format.(Ljava/time/format/DateTimeFormatter;)Ljava/lang/String;",
            &[at, formatter],
        );
        assert_eq!(text(&rt, written), "29/02/2024 13:05");
        let written = c(
            &mut rt,
            "java/time/format/DateTimeFormatter.format.(Ljava/time/temporal/TemporalAccessor;)Ljava/lang/String;",
            &[formatter, at],
        );
        assert_eq!(text(&rt, written), "29/02/2024 13:05");
        assert_eq!(
            c(&mut rt, "java/time/LocalDateTime.getDayOfMonth.()I", &[at]),
            V::I(29)
        );
        let iso =
            crate::api::get_static(&mut rt, "java/time/format/DateTimeFormatter.ISO_LOCAL_DATE");
        let today = c(
            &mut rt,
            "java/time/LocalDate.of.(III)Ljava/time/LocalDate;",
            &[V::I(2024), V::I(2), V::I(29)],
        );
        let written = c(
            &mut rt,
            "java/time/LocalDate.format.(Ljava/time/format/DateTimeFormatter;)Ljava/lang/String;",
            &[today, iso],
        );
        assert_eq!(text(&rt, written), "2024-02-29");

        let instant = c(
            &mut rt,
            "java/time/Instant.ofEpochMilli.(J)Ljava/time/Instant;",
            &[V::J(millis)],
        );
        let written = c(
            &mut rt,
            "java/time/Instant.toString.()Ljava/lang/String;",
            &[instant],
        );
        assert_eq!(text(&rt, written), "2024-02-29T13:05:09.007Z");
        assert_eq!(
            c(&mut rt, "java/time/Instant.getEpochSecond.()J", &[instant]),
            V::J(millis / 1000)
        );
        assert_eq!(
            c(&mut rt, "java/time/Instant.getNano.()I", &[instant]),
            V::I(7_000_000)
        );
        let now = c(&mut rt, "java/time/Instant.now.()Ljava/time/Instant;", &[]);
        assert!(c(&mut rt, "java/time/Instant.toEpochMilli.()J", &[now]).j() > 1_700_000_000_000);
        assert_eq!(
            c(
                &mut rt,
                "java/time/Instant.isAfter.(Ljava/time/Instant;)Z",
                &[now, instant]
            ),
            V::I(1)
        );

        let id = rt.string("Europe/Vienna");
        let zone = c(
            &mut rt,
            "java/util/TimeZone.getTimeZone.(Ljava/lang/String;)Ljava/util/TimeZone;",
            &[id],
        );
        let named = c(
            &mut rt,
            "java/util/TimeZone.getID.()Ljava/lang/String;",
            &[zone],
        );
        assert_eq!(text(&rt, named), "Europe/Vienna");
        assert_eq!(
            c(&mut rt, "java/util/TimeZone.getRawOffset.()I", &[zone]),
            V::I(0)
        );
        c(
            &mut rt,
            "java/text/SimpleDateFormat.setTimeZone.(Ljava/util/TimeZone;)V",
            &[plain, zone],
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// A `URI` is its text: its parts (`null` for those it has none of), another reference
    /// resolved against it, the `URL` of it and back; and a path's is the `file:` one a `File`
    /// is made of again (a mod asks its config folder so).
    #[test]
    fn a_uri_is_its_text_and_a_path_s_is_a_file_s() {
        let mut rt = Rt::new();
        let text = |rt: &mut Rt, key: &'static str, on: V| {
            let v = c(rt, key, &[on]);
            rt.str_of(v).map(str::to_owned)
        };
        let spec = rt.string("https://user@example.org:8443/a/b.json?x=1#top");
        let uri = c(
            &mut rt,
            "java/net/URI.create.(Ljava/lang/String;)Ljava/net/URI;",
            &[spec],
        );
        assert_eq!(
            text(&mut rt, "java/net/URI.getScheme.()Ljava/lang/String;", uri).as_deref(),
            Some("https")
        );
        assert_eq!(
            text(&mut rt, "java/net/URI.getHost.()Ljava/lang/String;", uri).as_deref(),
            Some("example.org")
        );
        assert_eq!(c(&mut rt, "java/net/URI.getPort.()I", &[uri]), V::I(8443));
        assert_eq!(
            text(&mut rt, "java/net/URI.getPath.()Ljava/lang/String;", uri).as_deref(),
            Some("/a/b.json")
        );
        assert_eq!(
            text(
                &mut rt,
                "java/net/URI.getRawQuery.()Ljava/lang/String;",
                uri
            )
            .as_deref(),
            Some("x=1")
        );
        assert_eq!(
            text(
                &mut rt,
                "java/net/URI.getFragment.()Ljava/lang/String;",
                uri
            )
            .as_deref(),
            Some("top")
        );
        assert_eq!(
            text(
                &mut rt,
                "java/net/URI.getUserInfo.()Ljava/lang/String;",
                uri
            )
            .as_deref(),
            Some("user")
        );
        assert_eq!(c(&mut rt, "java/net/URI.isAbsolute.()Z", &[uri]), V::I(1));
        let url = c(&mut rt, "java/net/URI.toURL.()Ljava/net/URL;", &[uri]);
        assert_eq!(
            text(&mut rt, "java/net/URL.getHost.()Ljava/lang/String;", url).as_deref(),
            Some("example.org")
        );
        let back = c(&mut rt, "java/net/URL.toURI.()Ljava/net/URI;", &[url]);
        assert_eq!(
            c(
                &mut rt,
                "java/net/URI.equals.(Ljava/lang/Object;)Z",
                &[uri, back]
            ),
            V::I(1)
        );
        for (other, resolved) in [
            ("c.json", "https://user@example.org:8443/a/c.json"),
            ("/d", "https://user@example.org:8443/d"),
            ("mailto:x@y", "mailto:x@y"),
        ] {
            let other = rt.string(other);
            let r = c(
                &mut rt,
                "java/net/URI.resolve.(Ljava/lang/String;)Ljava/net/URI;",
                &[uri, other],
            );
            assert_eq!(
                text(&mut rt, "java/net/URI.toString.()Ljava/lang/String;", r).as_deref(),
                Some(resolved)
            );
        }
        // One with no scheme and no host.
        let plain = rt.new_object("java/net/URI");
        let relative = rt.string("config/mod.toml");
        c(
            &mut rt,
            "java/net/URI.<init>.(Ljava/lang/String;)V",
            &[plain, relative],
        );
        assert_eq!(
            text(
                &mut rt,
                "java/net/URI.getScheme.()Ljava/lang/String;",
                plain
            ),
            None
        );
        assert_eq!(
            text(&mut rt, "java/net/URI.getHost.()Ljava/lang/String;", plain),
            None
        );
        assert_eq!(c(&mut rt, "java/net/URI.isAbsolute.()Z", &[plain]), V::I(0));
        // A path's, and the file made of it.
        let dir = std::env::temp_dir().join(format!("hz-uri-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        let game_dir = rt.string(&dir.to_string_lossy());
        rt.statics.insert(crate::GAME_DIR, game_dir);
        let name = rt.string("config");
        let none = rt.alloc("[Ljava/lang/String;", crate::Data::Array(Vec::new()));
        let path = c(
            &mut rt,
            "java/nio/file/Path.of.(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
            &[name, none],
        );
        let of_path = c(
            &mut rt,
            "java/nio/file/Path.toUri.()Ljava/net/URI;",
            &[path],
        );
        assert_eq!(
            text(
                &mut rt,
                "java/net/URI.getScheme.()Ljava/lang/String;",
                of_path
            )
            .as_deref(),
            Some("file")
        );
        let file = rt.new_object("java/io/File");
        c(
            &mut rt,
            "java/io/File.<init>.(Ljava/net/URI;)V",
            &[file, of_path],
        );
        assert_eq!(c(&mut rt, "java/io/File.isDirectory.()Z", &[file]), V::I(1));
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    #[test]
    fn descriptors_split_into_their_parameters() {
        assert_eq!(params("(Ljava/lang/ThreadGroup;Ljava/lang/Runnable;Ljava/lang/String;J)V"), ["Ljava/lang/ThreadGroup;", "Ljava/lang/Runnable;", "Ljava/lang/String;", "J"]);
        assert_eq!(params("([[I[Ljava/lang/String;Z)I"), ["[[I", "[Ljava/lang/String;", "Z"]);
        assert!(params("()V").is_empty());
        assert_eq!(returned("(I)Ljava/net/http/HttpClient;"), Some("java/net/http/HttpClient"));
        assert_eq!(returned("(I)V"), None);
    }

    #[test]
    fn a_calendar_reads_and_moves_its_moment() {
        let mut rt = Rt::new();
        // 2026-11-10 05:34:37.250 UTC, a Tuesday.
        let millis = days_from_civil(2026, 11, 10) * 86_400_000 + 5 * 3_600_000 + 34 * 60_000 + 37_250;
        assert_eq!(fields(millis), [2026, 10, 10, 5, 34, 37, 250]);
        assert_eq!(moment(fields(millis)), millis);
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        let cal = c(&mut rt, "java/util/Calendar.getInstance.()Ljava/util/Calendar;", &[]);
        assert!(c(&mut rt, "java/util/Calendar.getTimeInMillis.()J", &[cal]).j() > 1_700_000_000_000);
        c(&mut rt, "java/util/Calendar.setTimeInMillis.(J)V", &[cal, V::J(millis)]);
        let get = |rt: &mut Rt, f: i32| c(rt, "java/util/Calendar.get.(I)I", &[cal, V::I(f)]).i();
        let read: Vec<i32> = [1, 2, 5, 6, 7, 9, 10, 11, 12, 13, 14].iter().map(|&f| get(&mut rt, f)).collect();
        assert_eq!(read, [2026, 10, 10, 314, 3, 0, 5, 5, 34, 37, 250]);
        // The end of January, a month on, is the end of February.
        c(&mut rt, "java/util/Calendar.set.(III)V", &[cal, V::I(2024), V::I(0), V::I(31)]);
        c(&mut rt, "java/util/Calendar.add.(II)V", &[cal, V::I(2), V::I(1)]);
        assert_eq!((get(&mut rt, 1), get(&mut rt, 2), get(&mut rt, 5)), (2024, 1, 29));
        assert_eq!(c(&mut rt, "java/util/Calendar.getActualMaximum.(I)I", &[cal, V::I(5)]).i(), 29);
        c(&mut rt, "java/util/Calendar.add.(II)V", &[cal, V::I(5), V::I(2)]);
        assert_eq!((get(&mut rt, 2), get(&mut rt, 5)), (2, 2));
        c(&mut rt, "java/util/Calendar.set.(II)V", &[cal, V::I(11), V::I(23)]);
        assert_eq!(get(&mut rt, 11), 23);
        // Its `Date`, and dates among themselves.
        let date = c(&mut rt, "java/util/Calendar.getTime.()Ljava/util/Date;", &[cal]);
        let later = rt.new_object(DATE);
        let at = c(&mut rt, "java/util/Date.getTime.()J", &[date]).j();
        c(&mut rt, "java/util/Date.<init>.(J)V", &[later, V::J(at + 1)]);
        assert_eq!(c(&mut rt, "java/util/Date.before.(Ljava/util/Date;)Z", &[date, later]), V::I(1));
        assert_eq!(c(&mut rt, "java/util/Date.compareTo.(Ljava/util/Date;)I", &[later, date]), V::I(1));
        let epoch = rt.new_object(DATE);
        c(&mut rt, "java/util/Date.<init>.(J)V", &[epoch, V::J(0)]);
        let text = c(&mut rt, "java/util/Date.toString.()Ljava/lang/String;", &[epoch]);
        assert_eq!(rt.str_of(text), Some("Thu Jan 01 00:00:00 UTC 1970"));
        let made = rt.new_object(CALENDAR);
        c(&mut rt, "java/util/GregorianCalendar.<init>.(III)V", &[made, V::I(1999), V::I(11), V::I(31)]);
        assert_eq!(c(&mut rt, "java/util/GregorianCalendar.get.(I)I", &[made, V::I(6)]).i(), 365);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_request_is_built_and_fails_as_one_does_offline() {
        let mut rt = Rt::new();
        let builder = c(&mut rt, "java/net/http/HttpClient.newBuilder.()Ljava/net/http/HttpClient$Builder;", &[]);
        assert_eq!(rt.class_of(builder), "java/net/http/HttpClient$Builder");
        let same = c(&mut rt, "java/net/http/HttpClient$Builder.connectTimeout.(Ljava/time/Duration;)Ljava/net/http/HttpClient$Builder;", &[builder, V::NULL]);
        assert_eq!(same, builder);
        let client = c(&mut rt, "java/net/http/HttpClient$Builder.build.()Ljava/net/http/HttpClient;", &[builder]);
        assert_eq!(rt.class_of(client), "java/net/http/HttpClient");
        let request = c(&mut rt, "java/net/http/HttpRequest.newBuilder.(Ljava/net/URI;)Ljava/net/http/HttpRequest$Builder;", &[V::NULL]);
        let request = c(&mut rt, "java/net/http/HttpRequest$Builder.GET.()Ljava/net/http/HttpRequest$Builder;", &[request]);
        let request = c(&mut rt, "java/net/http/HttpRequest$Builder.build.()Ljava/net/http/HttpRequest;", &[request]);
        let handler = c(&mut rt, "java/net/http/HttpResponse$BodyHandlers.ofString.()Ljava/net/http/HttpResponse$BodyHandler;", &[]);
        assert!(rt.thrown.is_none() && rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        c(&mut rt, "java/net/http/HttpClient.send.(Ljava/net/http/HttpRequest;Ljava/net/http/HttpResponse$BodyHandler;)Ljava/net/http/HttpResponse;", &[client, request, handler]);
        let thrown = rt.thrown.take().expect("offline");
        assert_eq!(rt.class_of(thrown), IO_EXCEPTION);
        let later = c(
            &mut rt,
            "java/net/http/HttpClient.sendAsync.(Ljava/net/http/HttpRequest;Ljava/net/http/HttpResponse$BodyHandler;)Ljava/util/concurrent/CompletableFuture;",
            &[client, request, handler],
        );
        let response = c(&mut rt, "java/util/concurrent/CompletableFuture.join.()Ljava/lang/Object;", &[later]);
        assert_eq!(c(&mut rt, "java/net/http/HttpResponse.statusCode.()I", &[response]), V::I(503));
        // A server a mod would listen with fails where it is opened, and a socket where it connects.
        c(
            &mut rt,
            "java/nio/channels/ServerSocketChannel.open.()Ljava/nio/channels/ServerSocketChannel;",
            &[],
        );
        assert_eq!(rt.thrown.take().map(|t| rt.class_of(t)), Some(IO_EXCEPTION));
        let (socket, host) = (rt.new_object("java/net/Socket"), rt.string("example.org"));
        c(&mut rt, "java/net/Socket.<init>.()V", &[socket]);
        assert!(rt.thrown.is_none());
        c(
            &mut rt,
            "java/net/Socket.<init>.(Ljava/lang/String;I)V",
            &[socket, host, V::I(80)],
        );
        assert_eq!(rt.thrown.take().map(|t| rt.class_of(t)), Some(IO_EXCEPTION));
        let server = rt.new_object("java/net/ServerSocket");
        c(
            &mut rt,
            "java/net/ServerSocket.accept.()Ljava/net/Socket;",
            &[server],
        );
        assert_eq!(rt.thrown.take().map(|t| rt.class_of(t)), Some(IO_EXCEPTION));
        c(&mut rt, "java/net/ServerSocket.close.()V", &[server]);
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        // A URL is its text; a connection to it can't be had.
        let url = rt.new_object(URL);
        let spec = rt.string("https://example.org:8443/a/b.json?x=1#top");
        c(&mut rt, "java/net/URL.<init>.(Ljava/lang/String;)V", &[url, spec]);
        let text = |rt: &mut Rt, key: &'static str| {
            let v = c(rt, key, &[url]);
            rt.str_of(v).unwrap_or("?").to_owned()
        };
        assert_eq!(text(&mut rt, "java/net/URL.getProtocol.()Ljava/lang/String;"), "https");
        assert_eq!(text(&mut rt, "java/net/URL.getHost.()Ljava/lang/String;"), "example.org");
        assert_eq!(text(&mut rt, "java/net/URL.getPath.()Ljava/lang/String;"), "/a/b.json");
        assert_eq!(text(&mut rt, "java/net/URL.getFile.()Ljava/lang/String;"), "/a/b.json?x=1");
        assert_eq!(text(&mut rt, "java/net/URL.getRef.()Ljava/lang/String;"), "top");
        assert_eq!(c(&mut rt, "java/net/URL.getPort.()I", &[url]), V::I(8443));
        assert!(rt.thrown.is_none() && rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        c(&mut rt, "java/net/URL.openStream.()Ljava/io/InputStream;", &[url]);
        let thrown = rt.thrown.take().expect("offline");
        assert_eq!(rt.class_of(thrown), IO_EXCEPTION);
    }

    #[test]
    fn locks_are_free_and_latches_are_passed_on_the_one_thread() {
        let mut rt = Rt::new();
        let lock = rt.new_object("java/util/concurrent/locks/ReentrantLock");
        c(
            &mut rt,
            "java/util/concurrent/locks/ReentrantLock.<init>.()V",
            &[lock],
        );
        c(
            &mut rt,
            "java/util/concurrent/locks/ReentrantLock.lock.()V",
            &[lock],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/ReentrantLock.tryLock.()Z",
                &[lock]
            ),
            V::I(1)
        );
        c(
            &mut rt,
            "java/util/concurrent/locks/Lock.unlock.()V",
            &[lock],
        );
        let both = rt.new_object("java/util/concurrent/locks/ReentrantReadWriteLock");
        let read = c(
            &mut rt,
            "java/util/concurrent/locks/ReentrantReadWriteLock.readLock.()Ljava/util/concurrent/locks/ReentrantReadWriteLock$ReadLock;",
            &[both],
        );
        c(
            &mut rt,
            "java/util/concurrent/locks/ReentrantReadWriteLock$ReadLock.lock.()V",
            &[read],
        );
        let latch = rt.new_object(LATCH);
        c(
            &mut rt,
            "java/util/concurrent/CountDownLatch.<init>.(I)V",
            &[latch, V::I(2)],
        );
        c(
            &mut rt,
            "java/util/concurrent/CountDownLatch.countDown.()V",
            &[latch],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/CountDownLatch.getCount.()J",
                &[latch]
            ),
            V::J(1)
        );
        // Not counted down yet: passed all the same, as a timed wait that ran out.
        c(
            &mut rt,
            "java/util/concurrent/CountDownLatch.await.()V",
            &[latch],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/CountDownLatch.await.(JLjava/util/concurrent/TimeUnit;)Z",
                &[latch, V::J(5), V::NULL]
            ),
            V::I(0)
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// A synchronizer keeps its state; a stamped lock's stamps are always good; a builder's
    /// thread carries its name and body.
    #[test]
    fn a_synchronizer_keeps_its_state_and_a_stamp_is_always_good() {
        let mut rt = Rt::new();
        let sync = rt.new_object("t/Sync");
        c(
            &mut rt,
            "java/util/concurrent/locks/AbstractQueuedSynchronizer.setState.(I)V",
            &[sync, V::I(3)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.compareAndSetState.(II)Z",
                &[sync, V::I(2), V::I(9)]
            ),
            V::I(0)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.compareAndSetState.(II)Z",
                &[sync, V::I(3), V::I(9)]
            ),
            V::I(1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.getState.()I",
                &[sync]
            ),
            V::I(9)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/AbstractQueuedSynchronizer.hasQueuedThreads.()Z",
                &[sync]
            ),
            V::I(0)
        );
        let lock = rt.new_object("java/util/concurrent/locks/StampedLock");
        c(
            &mut rt,
            "java/util/concurrent/locks/StampedLock.<init>.()V",
            &[lock],
        );
        let stamp = c(
            &mut rt,
            "java/util/concurrent/locks/StampedLock.tryOptimisticRead.()J",
            &[lock],
        );
        assert_ne!(stamp, V::J(0));
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/StampedLock.validate.(J)Z",
                &[lock, stamp]
            ),
            V::I(1)
        );
        let write = c(
            &mut rt,
            "java/util/concurrent/locks/StampedLock.writeLock.()J",
            &[lock],
        );
        c(
            &mut rt,
            "java/util/concurrent/locks/StampedLock.unlockWrite.(J)V",
            &[lock, write],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/locks/StampedLock.isWriteLocked.()Z",
                &[lock]
            ),
            V::I(0)
        );
        // A thread from a builder, and one from a factory of the runtime's own.
        let builder = c(
            &mut rt,
            "java/lang/Thread.ofVirtual.()Ljava/lang/Thread$Builder$OfVirtual;",
            &[],
        );
        let name = rt.string("worker");
        let named = c(
            &mut rt,
            "java/lang/Thread$Builder$OfVirtual.name.(Ljava/lang/String;)Ljava/lang/Thread$Builder$OfVirtual;",
            &[builder, name],
        );
        assert_eq!(named, builder);
        let made = c(
            &mut rt,
            "java/lang/Thread$Builder.unstarted.(Ljava/lang/Runnable;)Ljava/lang/Thread;",
            &[builder, V::NULL],
        );
        let got = c(
            &mut rt,
            "java/lang/Thread.getName.()Ljava/lang/String;",
            &[made],
        );
        assert_eq!(rt.str_of(got), Some("worker"));
        let factory = c(
            &mut rt,
            "java/lang/Thread$Builder.factory.()Ljava/util/concurrent/ThreadFactory;",
            &[builder],
        );
        let from_factory = c(
            &mut rt,
            "java/util/concurrent/ThreadFactory.newThread.(Ljava/lang/Runnable;)Ljava/lang/Thread;",
            &[factory, V::NULL],
        );
        assert_eq!(rt.class_of(from_factory), THREAD);
        assert_eq!(
            c(
                &mut rt,
                "java/lang/Thread.getDefaultUncaughtExceptionHandler.()Ljava/lang/Thread$UncaughtExceptionHandler;",
                &[]
            ),
            V::NULL
        );
        // A pool of the runtime's own has nothing waiting.
        let pool = c(
            &mut rt,
            "java/util/concurrent/Executors.newFixedThreadPool.(I)Ljava/util/concurrent/ExecutorService;",
            &[V::I(4)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/ThreadPoolExecutor.getActiveCount.()I",
                &[pool]
            ),
            V::I(0)
        );
        let queue = c(
            &mut rt,
            "java/util/concurrent/ThreadPoolExecutor.getQueue.()Ljava/util/concurrent/BlockingQueue;",
            &[pool],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/BlockingQueue.size.()I",
                &[queue]
            ),
            V::I(0)
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    #[test]
    fn a_started_thread_runs_to_its_first_wait() {
        let mut rt = Rt::new();
        let game = c(&mut rt, "java/lang/Thread.currentThread.()Ljava/lang/Thread;", &[]);
        assert_eq!(c(&mut rt, "java/lang/Thread.isAlive.()Z", &[game]), V::I(1));
        assert_eq!(c(&mut rt, "java/lang/Thread.getId.()J", &[game]), V::J(1));
        // On the game's thread a sleep is over at once, and nobody is interrupted.
        c(&mut rt, "java/lang/Thread.sleep.(J)V", &[V::J(50)]);
        assert!(rt.thrown.is_none());
        assert_eq!(c(&mut rt, "java/lang/Thread.interrupted.()Z", &[]), V::I(0));
        let worker = rt.new_object(THREAD);
        let name = rt.string("worker");
        c(&mut rt, "java/lang/Thread.<init>.(Ljava/lang/Runnable;Ljava/lang/String;)V", &[worker, V::NULL, name]);
        let got = c(&mut rt, "java/lang/Thread.getName.()Ljava/lang/String;", &[worker]);
        assert_eq!(rt.str_of(got), Some("worker"));
        c(&mut rt, "java/lang/Thread.setDaemon.(Z)V", &[worker, V::I(1)]);
        assert_eq!(c(&mut rt, "java/lang/Thread.isDaemon.()Z", &[worker]), V::I(1));
        c(&mut rt, "java/lang/Thread.start.()V", &[worker]);
        c(&mut rt, "java/lang/Thread.join.()V", &[worker]);
        assert_eq!(c(&mut rt, "java/lang/Thread.isAlive.()Z", &[worker]), V::I(0));
        assert!(rt.thrown.is_none() && rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // A unit of time's sleep and a take from an empty queue are waits too: nothing on the
        // game's own thread, an interrupt in a started thread's body.
        let (seconds, queue) = (
            crate::api::get_static(&mut rt, "java/util/concurrent/TimeUnit.SECONDS"),
            rt.new_object("java/util/concurrent/LinkedBlockingQueue"),
        );
        c(
            &mut rt,
            "java/util/concurrent/LinkedBlockingQueue.<init>.()V",
            &[queue],
        );
        c(
            &mut rt,
            "java/util/concurrent/TimeUnit.sleep.(J)V",
            &[seconds, V::J(1)],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/BlockingQueue.take.()Ljava/lang/Object;",
                &[queue]
            ),
            V::NULL
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        STARTED.with(|s| s.set((1, 0)));
        for key in [
            "java/util/concurrent/TimeUnit.sleep.(J)V",
            "java/util/concurrent/BlockingQueue.take.()Ljava/lang/Object;",
            "java/util/concurrent/BlockingQueue.poll.(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        ] {
            STARTED.with(|s| s.set((1, 0)));
            let args = if key.contains("TimeUnit.sleep") {
                vec![seconds, V::J(1)]
            } else {
                vec![queue, V::J(1), seconds]
            };
            c(&mut rt, key, &args);
            let thrown = rt.thrown.take().unwrap_or_else(|| panic!("{key} waited"));
            assert_eq!(
                rt.class_of(thrown),
                "java/lang/InterruptedException",
                "{key}"
            );
        }
        // The thread's stack begins with the question itself, then the class that asked
        // (told by the translated call), then the unknown.
        rt.caller_class = "kotlin/jvm/internal/Intrinsics";
        let trace = c(
            &mut rt,
            "java/lang/Thread.getStackTrace.()[Ljava/lang/StackTraceElement;",
            &[game],
        );
        let classes: Vec<String> = crate::jutil::elements(&rt, trace)
            .iter()
            .map(|f| {
                let name = c(
                    &mut rt,
                    "java/lang/StackTraceElement.getClassName.()Ljava/lang/String;",
                    &[*f],
                );
                rt.str_of(name).unwrap_or("").to_owned()
            })
            .collect();
        assert_eq!(
            classes[..3],
            [
                "java.lang.Thread",
                "kotlin.jvm.internal.Intrinsics",
                "Unknown"
            ]
        );
        assert_eq!((classes.len(), rt.caller_class), (8, "java/lang/Object"));
        // In a started thread's body a wait interrupts it, and past a few of them stops it.
        STARTED.with(|s| s.set((1, 0)));
        assert_eq!(c(&mut rt, "java/lang/Thread.interrupted.()Z", &[]), V::I(1));
        for want in ["java/lang/InterruptedException", "java/lang/InterruptedException", "java/lang/InterruptedException", "java/lang/Error"] {
            c(&mut rt, "java/lang/Thread.sleep.(J)V", &[V::J(50)]);
            let thrown = rt.thrown.take().expect("interrupted");
            assert_eq!(rt.class_of(thrown), want);
        }
        STARTED.with(|s| s.set((0, 0)));
    }
}
