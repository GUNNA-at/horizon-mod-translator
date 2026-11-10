//! What a mod asks of the machine through the JDK, answered for a game that runs on one thread
//! and reaches no network: the threads it starts, the calendar and the clock, HTTP and URLs.
use crate::{Rt, V};

const THREAD: &str = "java/lang/Thread";
const RUN: &str = "java/lang/Runnable.run.()V";
const CALENDAR: &str = "java/util/GregorianCalendar";
const DATE: &str = "java/util/Date";
const URL: &str = "java/net/URL";
const RESPONSE: &str = "java/net/http/HttpResponse";
const IO_EXCEPTION: &str = "java/io/IOException";
const OFFLINE: &str = "a mod has no network here";

thread_local! {
    /// How deep in started threads' bodies the runtime is, and how often the innermost has
    /// waited ([`wait`]).
    static STARTED: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
}

/// A thread's body told to stop this often that waits again is stopped by an error instead: it
/// swallows the interrupt and would never end.
const WAITS: u32 = 3;

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key
/// only to the modules that claim it ([`crate::api`]'s chain). `Thread`'s and the calendar's come
/// through the modules of their packages ([`thread`], [`calendar`]).
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner.starts_with("java/net/http/")
        || owner.starts_with(LOCKS)
        || matches!(
            owner,
            URL | "java/net/URLConnection"
                | "java/net/HttpURLConnection"
                | "javax/net/ssl/HttpsURLConnection"
                | LATCH
                | "java/util/concurrent/Semaphore"
        )
        || socket(owner)
}

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

/// `java.util.concurrent`'s locks, latches and semaphores, on the one thread there is: a lock
/// is always free and held by whoever asks, a read or write lock is the lock itself, and nothing
/// is ever waited for (a latch not counted down yet is passed, as after a timeout; in a thread a
/// mod started, a wait interrupts it: [`wait`]).
fn locks(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
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
            V::Void
        }
        "getCount" => V::J(rt.get_field(this, "$count").j()),
        "availablePermits" => V::I(rt.get_field(this, "$count").j() as i32),
        "await" | "awaitNanos" | "awaitUninterruptibly" | "park" | "parkNanos" => {
            let waited = wait(rt);
            match desc.rsplit(')').next() {
                Some("Z") => V::I((rt.get_field(this, "$count").j() == 0) as i32),
                Some("J") => V::J(0),
                _ => waited,
            }
        }
        "lock"
        | "unlock"
        | "lockInterruptibly"
        | "acquire"
        | "acquireUninterruptibly"
        | "release"
        | "signal"
        | "signalAll"
        | "unpark" => V::Void,
        "tryLock" | "tryAcquire" | "isHeldByCurrentThread" | "isWriteLockedByCurrentThread" => {
            V::I(1)
        }
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

/// A wait (`Thread.sleep`, a lock's) where it is asked. On the game's own thread it is over at
/// once. In the body of a thread a mod started, which runs on the game's thread too
/// ([`thread`]), it can't be waited out: the thread is interrupted, as a mod stops one of its
/// own, and its loop ends the way the mod wrote it to.
pub(crate) fn wait(rt: &mut Rt) -> V {
    let (depth, waits) = STARTED.with(|s| s.get());
    if depth == 0 {
        return V::Void;
    }
    STARTED.with(|s| s.set((depth, waits + 1)));
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

/// `java.lang.Thread`. One runtime runs on one thread, the game's: `currentThread` is that one
/// each time. A thread a mod makes and starts runs its body at once, there, to its end or to its
/// first wait: a task done in the background is done when `start` returns, and a loop that polls
/// and sleeps ends at its first sleep, interrupted ([`wait`]).
pub(crate) fn thread(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let game = |rt: &mut Rt| rt.intern("compat:thread".into(), |rt| rt.new_object(THREAD));
    let run = |rt: &mut Rt, thread: V| {
        // A subclass's own `run`, else the body the thread was made with.
        let body = if rt.class_of(thread) == THREAD { rt.get_field(thread, "$target") } else { thread };
        if body.r() == 0 {
            return;
        }
        let outer = STARTED.with(|s| s.replace((s.get().0 + 1, 0)));
        rt.call_functional(RUN, body, &[]);
        STARTED.with(|s| s.set(outer));
        // What ended it stays in it, as on a JVM: an interrupt silently, anything else named.
        if let Some(t) = rt.thrown.take()
            && rt.class_of(t) != "java/lang/InterruptedException"
        {
            eprintln!("horizon-compat: a thread a mod started ended with {}", rt.class_of(t));
        }
    };
    Some(match name {
        "currentThread" => game(rt),
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
        "start" => {
            if rt.get_field(this, "$started").i() == 0 {
                rt.put_field(this, "$started", V::I(1));
                run(rt, this);
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
            run(rt, t);
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
        // The game's thread runs; one a mod started ran to its end inside `start`.
        "isAlive" => V::I((this == game(rt)) as i32),
        // A started thread's body is always being asked to stop: it can't share the game's thread.
        "isInterrupted" | "interrupted" => V::I(in_started_thread() as i32),
        "sleep" => wait(rt),
        "holdsLock" => V::I(1),
        "getStackTrace" => rt.new_array(0, V::NULL),
        "getThreadGroup" => rt.intern("compat:thread-group".into(), |rt| rt.new_object("java/lang/ThreadGroup")),
        "getContextClassLoader" => rt.intern("compat:class-loader".into(), |rt| rt.new_object("java/lang/ClassLoader")),
        "join" | "interrupt" | "yield" | "onSpinWait" | "setPriority" | "setUncaughtExceptionHandler" | "setDefaultUncaughtExceptionHandler" | "setContextClassLoader" | "checkAccess" => {
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

/// `java.net.http` and `java.net.URL`'s connections, on a machine with no network for a mod: a
/// request is built as the mod builds it and then fails as one does offline (an `IOException`
/// where it is sent and waited for, a 503 where it is sent to be answered later), so an update
/// check or a statistics ping ends in the mod's own handling of that.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if owner.starts_with(LOCKS) || matches!(owner, LATCH | "java/util/concurrent/Semaphore") {
        return locks(rt, owner, name, desc, args);
    }
    if socket(owner) {
        return sockets(rt, name, desc, args);
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let offline = |rt: &mut Rt| crate::jlang::throw_with(rt, IO_EXCEPTION, OFFLINE);
    if owner == URL {
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
            "getPort" => V::I(part(rt, name).parse().unwrap_or(-1)),
            "getDefaultPort" => V::I(if spec(rt).starts_with("https:") { 443 } else { 80 }),
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
    use super::*;

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
