//! The translated code's class space, linked when its engine starts. Each part of the code
//! registers its own classes, methods and lambdas ([`Registry`]); a virtual call then finds its
//! method from the receiver's class as the JVM resolves it (the class and its superclasses, then
//! the default methods of their superinterfaces), once per class and method on each thread.
//!
//! The tables live beside the code they name, so an engine is as big as the list of its crates,
//! however many mods it holds: a whole-group dispatcher per method grew with every class of
//! every mod translated together, and Create's engine crate was 13 MB of them.
use crate::{Data, HashMap, LAMBDA, Rt, V};
use std::collections::{BTreeSet, HashSet, VecDeque};

/// A translated method: `(rt, args)`, the receiver first for an instance method.
pub type Method = fn(&mut Rt, &[V]) -> V;
/// A lambda's implementation, called with every value it runs with: the captured ones, then the
/// call's own arguments.
pub type LambdaFn = fn(&mut Rt, Vec<V>) -> V;
/// Reflection on one class: `(rt, what, args)` with `what` as in [`Rt::reflect`] (`<clinit>`, a
/// constructor's `<init>.desc`, an enum's `values`). None when the class has no such member.
pub type ReflectFn = fn(&mut Rt, &str, &[V]) -> Option<V>;

/// The classes that hold the game's own methods standing for ones the runtime doesn't map: a
/// class of this prefix before a game class's name has that class's methods (static ones and
/// constructors too) whose code, in the player's own copy of the game, asks only what the runtime
/// answers. A call nothing maps runs the one of its key ([`Linked::derived`]).
pub const DERIVED: &str = "hz/derived/";

/// FNV-1a over 64 bits: a method key's (`name.desc`) hash. Generated code passes it with each
/// virtual call, so finding the method hashes no string; the translator computes the same.
pub const fn key_hash(key: &str) -> u64 {
    let b = key.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// A translated class, as the part of the code that holds it registers it.
pub struct ClassDef {
    pub name: &'static str,
    /// Its id: what its objects carry ([`crate::Object::cid`]), 1 and up.
    pub id: u32,
    pub superclass: Option<&'static str>,
    /// Its interfaces, with those its mixins add.
    pub interfaces: &'static [&'static str],
    /// Its instance methods with code (`name.desc`), each with whether it is private: a private
    /// one is never an interface's default method.
    pub methods: &'static [(&'static str, Method, bool)],
    /// What its mixins add to it or overwrite there, in their order: a call finds these before the
    /// class's own.
    pub mixed: &'static [(&'static str, Method)],
    pub reflect: Option<ReflectFn>,
    /// Its declared fields as reflection sees them: an `access name descriptor` line each; empty
    /// for none.
    pub fields: &'static str,
    /// The annotations reflection sees on it, a line each; empty for none.
    pub annotations: &'static str,
    /// Its `@SubscribeEvent` methods: (event class, method key, static, lambda id).
    pub subscribers: &'static [(&'static str, &'static str, bool, u32)],
}

/// A method a mixin adds to the game's classes (or overwrites there): a call reaches it on a game
/// object, or on a mod object whose classes don't have the method, by the call's owner key.
pub struct Added {
    pub key: &'static str,
    /// The method, after its mixin's class is initialised and its field initialisers ran on the
    /// object.
    pub run: Method,
    /// Owner keys whose calls are this method's whatever the receiver.
    pub owners: &'static [&'static str],
    /// Owner keys whose calls are its on an object of one of `targets`.
    pub checked: &'static [&'static str],
    pub targets: &'static [&'static str],
}

/// What one part of the translated code registers.
pub struct Registry {
    pub classes: &'static [ClassDef],
    /// Lambda implementations by id: the lambdas this part makes first.
    pub lambdas: &'static [(u32, LambdaFn)],
    pub added: &'static [Added],
    /// The interfaces its mixins add to the game's classes, by target ([`Rt::add_game_interfaces`]).
    pub game_interfaces: &'static [(&'static str, &'static [&'static str])],
    /// The `@SubscribeEvent(priority, receiveCanceled)` of its methods whose options aren't the
    /// default, by method key ([`Rt::subscriber_options`]).
    pub subscriber_options: &'static [(&'static str, (u8, bool))],
}

/// One engine's class space: its parts' registries, linked.
pub struct Linked {
    /// By class id.
    classes: Vec<Option<&'static ClassDef>>,
    ids: HashMap<&'static str, u32>,
    /// Each class's methods by key hash, sorted: (hash, method, private). By class id.
    methods: Vec<Box<[(u64, Method, bool)]>>,
    /// Each class's mixins' methods, in their order. By class id.
    mixed: Vec<Box<[(u64, Method)]>>,
    lambdas: Vec<Option<LambdaFn>>,
    added: HashMap<u64, Vec<&'static Added>>,
    /// `META-INF/services`: an interface's implementations.
    services: HashMap<&'static str, &'static [&'static str]>,
    registries: Vec<&'static Registry>,
}

impl Linked {
    /// Links `registries` (every part's) and the services the engine's jars list. Two keys of one
    /// class with the same hash would make one call reach the other's method: the link says so
    /// and keeps the first.
    pub fn new(
        registries: impl IntoIterator<Item = &'static Registry>,
        services: &'static [(&'static str, &'static [&'static str])],
    ) -> Linked {
        let registries: Vec<&'static Registry> = registries.into_iter().collect();
        let defs = || registries.iter().flat_map(|r| r.classes.iter());
        let top = defs().map(|d| d.id as usize).max().unwrap_or(0);
        let mut l = Linked {
            classes: vec![None; top + 1],
            ids: HashMap::default(),
            methods: vec![Box::default(); top + 1],
            mixed: vec![Box::default(); top + 1],
            lambdas: Vec::new(),
            added: HashMap::default(),
            services: services.iter().copied().collect(),
            registries: Vec::new(),
        };
        for d in defs() {
            let id = d.id as usize;
            l.classes[id] = Some(d);
            l.ids.insert(d.name, d.id);
            let mut methods: Vec<(u64, Method, bool, &str)> = d
                .methods
                .iter()
                .map(|&(k, m, p)| (key_hash(k), m, p, k))
                .collect();
            methods.sort_by_key(|e| e.0);
            methods.dedup_by(|b, a| {
                let same = a.0 == b.0;
                if same && a.3 != b.3 {
                    eprintln!(
                        "horizon-compat: {} has two methods whose keys hash alike: {} and {}",
                        d.name, a.3, b.3
                    );
                }
                same
            });
            l.methods[id] = methods.into_iter().map(|(h, m, p, _)| (h, m, p)).collect();
            l.mixed[id] = d.mixed.iter().map(|&(k, m)| (key_hash(k), m)).collect();
        }
        for r in &registries {
            for &(lid, f) in r.lambdas {
                let i = lid as usize;
                if l.lambdas.len() <= i {
                    l.lambdas.resize(i + 1, None);
                }
                l.lambdas[i] = Some(f);
            }
            for a in r.added {
                l.added.entry(key_hash(a.key)).or_default().push(a);
            }
        }
        l.registries = registries;
        l
    }

    /// The class with id `cid`; None for the game's objects (0) and lambdas.
    fn class(&self, cid: u32) -> Option<&'static ClassDef> {
        self.classes.get(cid as usize).copied().flatten()
    }

    /// A class's id by name; None for a class the code doesn't have.
    pub fn id(&self, class: &str) -> Option<u32> {
        self.ids.get(class).copied()
    }

    fn own(&self, id: u32, kh: u64) -> Option<(Method, bool)> {
        let list = self.methods.get(id as usize)?;
        list.binary_search_by_key(&kh, |e| e.0)
            .ok()
            .map(|i| (list[i].1, list[i].2))
    }

    /// The method a call of key hash `kh` runs on an object of class `cid`, as the JVM resolves it:
    /// the class and its superclasses (what their mixins add first), then the default methods of
    /// their superinterfaces, breadth first. None when no translated class has it (the game's, a
    /// lambda's).
    fn resolve_uncached(&self, cid: u32, kh: u64) -> Option<Method> {
        let mut chain: Vec<&'static ClassDef> = Vec::new();
        let mut c = self.class(cid);
        while let Some(d) = c {
            if let Some(&(_, m)) = self.mixed[d.id as usize].iter().find(|e| e.0 == kh) {
                return Some(m);
            }
            if let Some((m, _)) = self.own(d.id, kh) {
                return Some(m);
            }
            chain.push(d);
            // A superclass cycle is no class file javac writes; stop rather than spin.
            if chain.len() > 1024 {
                return None;
            }
            c = d
                .superclass
                .and_then(|s| self.id(s))
                .and_then(|id| self.class(id));
        }
        let mut queue: VecDeque<&'static str> = chain
            .iter()
            .flat_map(|d| d.interfaces.iter().copied())
            .collect();
        let mut seen: HashSet<&'static str> = HashSet::new();
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            let Some(d) = self.id(i).and_then(|id| self.class(id)) else {
                continue;
            };
            if let Some((m, false)) = self.own(d.id, kh) {
                return Some(m);
            }
            queue.extend(d.interfaces.iter().copied());
        }
        None
    }

    /// [`Linked::resolve_uncached`], kept for this thread by (class, key).
    pub fn resolve(&self, cid: u32, kh: u64) -> Option<Method> {
        type Id = (usize, u32, u64);
        thread_local! {
            static RESOLVED: std::cell::RefCell<HashMap<Id, Option<Method>>> = std::cell::RefCell::new(HashMap::default());
        }
        if cid == 0 || cid & LAMBDA != 0 {
            return None;
        }
        let id = (std::ptr::from_ref(self) as usize, cid, kh);
        if let Some(m) = RESOLVED.with(|r| r.borrow().get(&id).copied()) {
            return m;
        }
        let m = self.resolve_uncached(cid, kh);
        RESOLVED.with(|r| r.borrow_mut().insert(id, m));
        m
    }

    /// The game's own code for a call of `key` (`name.desc`) naming the game class `owner`, where
    /// the translator carried it with the mods ([`DERIVED`]): the method of the receiver's class
    /// (`receiver`, when it is one carried and at or below the owner) or of the owner, or the
    /// first above it, as the JVM resolves the call. None where a class below that one has code
    /// for the method too (the mark the translator leaves, a field `!name` of the descriptor):
    /// the object may be of that class, and its code isn't the one found. A constructor is its
    /// own class's only.
    pub fn derived(&self, receiver: Option<&str>, owner: &str, key: &str) -> Option<Method> {
        let own = self.id(&format!("{DERIVED}{owner}"));
        let (name, desc) = key.split_once('.')?;
        if name == "<init>" {
            return self.own(own?, key_hash(key)).map(|m| m.0);
        }
        let start = receiver
            .and_then(|r| self.id(&format!("{DERIVED}{r}")))
            .filter(|&r| own.is_none_or(|o| self.at_or_below(r, o)))
            .or(own)?;
        let marked =
            self.class(start)?.fields.lines().any(|l| {
                l.strip_prefix("0 !").and_then(|l| l.split_once(' ')) == Some((name, desc))
            });
        if marked {
            None
        } else {
            self.resolve(start, key_hash(key))
        }
    }

    /// Whether class `class` is `above` or has it among its supertypes.
    fn at_or_below(&self, class: u32, above: u32) -> bool {
        let (mut todo, mut seen) = (vec![class], HashSet::new());
        while let Some(id) = todo.pop() {
            if id == above {
                return true;
            }
            if let Some(d) = self.class(id).filter(|_| seen.insert(id)) {
                todo.extend(
                    d.superclass
                        .iter()
                        .chain(d.interfaces)
                        .filter_map(|s| self.id(s)),
                );
            }
        }
        false
    }

    /// Whether the code has a class of the game's standing methods for `class` ([`DERIVED`]).
    pub fn has_derived(&self, class: &str) -> bool {
        self.id(&format!("{DERIVED}{class}")).is_some()
    }

    /// Whether class `class` has `key` (`name.desc`) itself or through a translated superclass or
    /// superinterface (the generated `overrides`).
    pub fn overrides(&self, class: &str, key: &str) -> bool {
        self.id(class)
            .is_some_and(|id| self.resolve(id, key_hash(key)).is_some())
    }

    /// Every class registered.
    pub fn classes(&self) -> impl Iterator<Item = &'static ClassDef> + '_ {
        self.classes.iter().flatten().copied()
    }
}

impl Rt {
    /// Joins this runtime to an engine's class space: its classes' ids, superclasses, interfaces
    /// and listeners, and the calls the API makes back into the code (`vcall`, `lambda_call`,
    /// `reflect`). The engine sets `overrides` itself, as a function of its own.
    pub fn link(&mut self, linked: &'static Linked) {
        self.linked = Some(linked);
        self.vcall = Some(vcall);
        self.lambda_call = Some(lambda_call);
        self.reflect = Some(reflect);
        for d in linked.classes() {
            self.class_ids.insert(d.name, d.id);
            if let Some(s) = d.superclass {
                self.supers.insert(d.name, s);
            }
            if !d.interfaces.is_empty() {
                self.interfaces.insert(d.name, d.interfaces);
            }
            if !d.subscribers.is_empty() {
                self.subscribers.insert(d.name, d.subscribers.to_vec());
            }
            self.vkeys.extend(d.methods.iter().map(|m| m.0));
        }
        for r in &linked.registries {
            for &(target, interfaces) in r.game_interfaces {
                self.add_game_interfaces(target, interfaces);
            }
            for &(key, options) in r.subscriber_options {
                self.subscriber_options.insert(key, options);
            }
        }
    }
}

/// A virtual call: the method the receiver's class (`cid`) has for `key` (hash `kh`); else one a
/// mixin adds to the game's classes, by the call's owner key; else the API's (or a lambda's own).
pub fn dispatch(
    rt: &mut Rt,
    cid: u32,
    kh: u64,
    key: &'static str,
    owner_key: &'static str,
    args: &[V],
) -> V {
    if on_null(args) {
        return null_receiver(rt, owner_key);
    }
    dispatch_any(rt, cid, kh, key, owner_key, args)
}

/// [`dispatch`] for a receiver the caller answers for: the runtime's own calls into the code
/// ([`vcall`]), which may call what is null and take no answer for one.
fn dispatch_any(
    rt: &mut Rt,
    cid: u32,
    kh: u64,
    key: &'static str,
    owner_key: &'static str,
    args: &[V],
) -> V {
    if diagnosing() {
        return observed(rt, "virtual", owner_key, args, |rt| {
            dispatch_unobserved(rt, cid, kh, key, owner_key, args)
        });
    }
    dispatch_unobserved(rt, cid, kh, key, owner_key, args)
}

/// Whether a call the translated code makes on an object is one on Java's `null`: the JVM
/// throws `NullPointerException` there, and code counts on it (a mod asks every block
/// `canSurvive(null, null)` and takes the exception for "no": answered with stand-ins instead,
/// one block's loop never ended and the question was asked again every tick). What an unmapped
/// call answered (`V::Void`) is not null: the code goes on with it, as before.
/// `HORIZON_COMPAT_NULL_CALLS=stand-in` gives a call on null a stand-in answer again.
fn on_null(args: &[V]) -> bool {
    static THROWS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    args.first() == Some(&V::NULL)
        && *THROWS.get_or_init(|| {
            std::env::var("HORIZON_COMPAT_NULL_CALLS").map_or(true, |v| v != "stand-in")
        })
}

/// Whether reading a field of Java's `null` throws, as a call on it does ([`on_null`]): for the
/// API mapping's reads of the game's fields.
pub fn null_throws(object: V) -> bool {
    on_null(&[object])
}

/// Throws the `NullPointerException` of a call of `owner_key` (`owner.name.desc`) on `null`, with
/// the JVM's message for it.
pub fn null_receiver(rt: &mut Rt, owner_key: &str) -> V {
    let method = owner_key.split_once(".(").map_or(owner_key, |(m, _)| m);
    let e = rt.new_object("java/lang/NullPointerException");
    let message = rt.string(&format!(
        "Cannot invoke \"{}()\" because the object is null",
        method.replace('/', ".")
    ));
    rt.put_field(e, "message", message);
    rt.thrown = Some(e);
    V::Void
}

fn dispatch_unobserved(
    rt: &mut Rt,
    cid: u32,
    kh: u64,
    key: &'static str,
    owner_key: &'static str,
    args: &[V],
) -> V {
    let Some(l) = rt.linked else {
        return miss(rt, cid, owner_key, key, args);
    };
    if let Some(f) = l.resolve(cid, kh) {
        return f(rt, args);
    }
    if cid & LAMBDA == 0
        && let Some(V::R(r)) = args.first()
        && *r != 0
        && let Some(list) = l.added.get(&kh)
    {
        for a in list.iter().filter(|a| a.key == key) {
            if a.owners.contains(&owner_key) {
                return (a.run)(rt, args);
            }
            if a.checked.contains(&owner_key)
                && a.targets.iter().any(|t| rt.instance_of(args[0], t))
            {
                return (a.run)(rt, args);
            }
        }
    }
    miss(rt, cid, owner_key, key, args)
}

/// No translated class of the receiver has `key`: a lambda's own method (or a default method of
/// its interface), or the API's.
pub fn vcall_miss(
    rt: &mut Rt,
    cid: u32,
    owner_key: &'static str,
    key: &'static str,
    args: &[V],
) -> V {
    if on_null(args) {
        return null_receiver(rt, owner_key);
    }
    if diagnosing() {
        return observed(rt, "virtual", owner_key, args, |rt| {
            miss(rt, cid, owner_key, key, args)
        });
    }
    miss(rt, cid, owner_key, key, args)
}

fn miss(rt: &mut Rt, cid: u32, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
    if cid & LAMBDA == 0 {
        return crate::api::call(rt, owner_key, args);
    }
    let recv = args.first().copied().unwrap_or_default();
    if let Some((iface, sam)) = rt.lambda_info.get(&recv.r()).copied() {
        // A method its interface has code for runs as declared (a default method, though it may
        // share the functional method's name: an overload); of the rest, the functional one is the
        // lambda's own (by name: a bridge's descriptor differs).
        let icid = rt.class_ids.get(iface).copied().unwrap_or(0);
        if icid != 0
            && let Some(f) = rt.linked.and_then(|l| l.resolve(icid, kept_hash(key)))
        {
            return f(rt, args);
        }
        if !(key.starts_with(sam) && key.as_bytes().get(sam.len()) == Some(&b'.')) {
            return crate::api::call(rt, owner_key, args);
        }
    }
    // (From the mods' own code, or from the runtime's through [`vcall`], which is the entry.)
    let run = lambda_of(rt, cid & !LAMBDA, args).map(|(f, full)| f(rt, full));
    match run {
        Some(r) => rt.adapt_return(r, key),
        None => crate::api::call(rt, owner_key, args),
    }
}

/// The implementation of the lambda `lid` and what it runs with on `args` (`args[0]` the
/// lambda object): the values it captured, then the call's own.
fn lambda_of(rt: &Rt, lid: u32, args: &[V]) -> Option<(LambdaFn, Vec<V>)> {
    let f = rt.linked?.lambdas.get(lid as usize).copied().flatten()?;
    let mut full = rt.captured(args.first().copied().unwrap_or_default());
    full.extend_from_slice(args.get(1..).unwrap_or_default());
    Some((f, full))
}

/// A call on a lambda object (`lid`: its implementation's id), `args[0]` the lambda, from the
/// runtime's own code: an entry into the mods' ([`Rt::entered`]).
pub fn lambda_call(rt: &mut Rt, lid: u32, args: &[V]) -> Option<V> {
    let (f, full) = lambda_of(rt, lid, args)?;
    Some(rt.entered(move |rt| f(rt, full), |r| *r))
}

/// The diagnostics of calls the environment asks for, read once: bit 1 counts them
/// (`HORIZON_COMPAT_COUNT`, [`count`]), bit 2 traces some ([`observed`]).
fn diagnostics() -> u8 {
    const UNREAD: u8 = 0x80;
    static ON: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(UNREAD);
    match ON.load(std::sync::atomic::Ordering::Relaxed) {
        UNREAD => {
            let on = u8::from(std::env::var_os("HORIZON_COMPAT_COUNT").is_some())
                | (u8::from(!traced_parts().is_empty()) << 1);
            ON.store(on, std::sync::atomic::Ordering::Relaxed);
            on
        }
        on => on,
    }
}

/// Whether calls are counted (`HORIZON_COMPAT_COUNT` set).
pub fn counting() -> bool {
    diagnostics() & 1 != 0
}

/// Whether calls are counted or traced: the one check a call makes when neither is asked for.
pub fn diagnosing() -> bool {
    diagnostics() != 0
}

/// The parts of keys whose calls are traced: `HORIZON_COMPAT_CALLS=<part>[,<part>...]`.
fn traced_parts() -> &'static [String] {
    static PARTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    PARTS.get_or_init(|| {
        std::env::var("HORIZON_COMPAT_CALLS")
            .unwrap_or_default()
            .split(',')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

/// A call of a kind (`virtual`, `API`) by `key`, made by `call`, counted and traced as the
/// environment asks ([`diagnosing`]). With `HORIZON_COMPAT_CALLS=<part of a key>[,<another>...]`
/// each call whose key holds one of the parts is written to the log (stderr) as it starts, with
/// what it is called on and with, and as it returns, with what it gives or throws: the first 64
/// calls of each such key on each thread (or `HORIZON_COMPAT_CALLS_MAX`). Its depth is how many
/// calls of either kind it runs in. Where a mod's code leaves the way it takes in the JVM (a
/// screen it doesn't open, an item it doesn't use), the calls it made on the way say where.
pub fn observed(
    rt: &mut Rt,
    kind: &'static str,
    key: &'static str,
    args: &[V],
    call: impl FnOnce(&mut Rt) -> V,
) -> V {
    thread_local! {
        static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static SHOWN: std::cell::RefCell<HashMap<(usize, usize), u32>> = std::cell::RefCell::new(HashMap::default());
    }
    struct Out(u32);
    impl Drop for Out {
        fn drop(&mut self) {
            DEPTH.set(self.0);
        }
    }
    let on = diagnostics();
    if on & 1 != 0 {
        count(kind, key);
    }
    if on & 2 == 0 {
        return call(rt);
    }
    static MAX: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let max = *MAX.get_or_init(|| {
        std::env::var("HORIZON_COMPAT_CALLS_MAX")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(64)
    });
    let depth = DEPTH.get();
    let shown = traced_parts().iter().any(|p| key.contains(p.as_str()))
        && SHOWN.with_borrow_mut(|s| {
            let n = s.entry((key.as_ptr() as usize, key.len())).or_default();
            *n += 1;
            *n <= max
        });
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("?");
    if shown {
        let with: Vec<String> = args.iter().map(|&v| described(rt, v)).collect();
        eprintln!(
            "horizon-compat: {thread} {depth:>3} > {kind} {key} ({})",
            with.join(", ")
        );
    }
    let out = Out(depth);
    DEPTH.set(depth + 1);
    let r = call(rt);
    drop(out);
    if shown {
        let threw = match rt.thrown {
            Some(e) => format!(", threw {}", described(rt, e)),
            None => String::new(),
        };
        eprintln!(
            "horizon-compat: {thread} {depth:>3} < {key} = {}{threw}",
            described(rt, r)
        );
    }
    r
}

/// A value as a trace shows it: a number, `null`, a string's or a class's text, an enum
/// constant's name, else the object's class and heap index.
fn described(rt: &Rt, v: V) -> String {
    match v {
        V::Void => "void".into(),
        V::I(n) => n.to_string(),
        V::J(n) => format!("{n}L"),
        V::F(x) => format!("{x}f"),
        V::D(x) => format!("{x}d"),
        V::R(0) => "null".into(),
        V::R(r) => {
            let class = rt.class_of(v);
            if let Some(s) = rt.str_of(v) {
                let s: String = s.chars().take(80).collect();
                return format!("{class} {s:?}");
            }
            match rt.str_of(rt.get_field(v, "$name")) {
                Some(name) => format!("{class}.{name}"),
                None => format!("{class}#{r}"),
            }
        }
    }
}

/// With `HORIZON_COMPAT_COUNT` set, every call of a kind (`virtual`: the code's own virtual
/// calls by the key they name, `owner.name.desc`; `API`: the calls into the API) is counted by
/// its key on its thread, and every 2^24 of them the most frequent are written to the log
/// (stderr) and counted anew: where a mod's time goes.
pub fn count(kind: &'static str, key: &'static str) {
    type Counts = (u64, HashMap<(usize, usize), (&'static str, u64)>);
    thread_local! {
        static COUNTS: std::cell::RefCell<HashMap<&'static str, Counts>> = std::cell::RefCell::new(HashMap::default());
    }
    COUNTS.with(|c| {
        let mut all = c.borrow_mut();
        let c = all.entry(kind).or_default();
        c.0 += 1;
        c.1.entry((key.as_ptr() as usize, key.len()))
            .or_insert((key, 0))
            .1 += 1;
        if c.0 < 1 << 24 {
            return;
        }
        // The same key in two crates is two literals: counted as one.
        let mut merged: HashMap<&'static str, u64> = HashMap::default();
        for (k, n) in c.1.values() {
            *merged.entry(k).or_default() += n;
        }
        let mut top: Vec<(&'static str, u64)> = merged.into_iter().collect();
        top.sort_by_key(|e| std::cmp::Reverse(e.1));
        let thread = std::thread::current().name().unwrap_or("?").to_owned();
        let lines: Vec<String> = top
            .iter()
            .take(40)
            .map(|(k, n)| format!("  {n:>10}  {k}"))
            .collect();
        eprintln!(
            "horizon-compat: the most frequent of {} {kind} calls on {thread}:\n{}",
            c.0,
            lines.join("\n")
        );
        *c = Counts::default();
    });
}

/// [`key_hash`] of a key the runtime names (its own constants), kept by the key's address and
/// length: hashing it a byte at a time at every call cost more than the call (a block entity
/// ticker's `tick` key is 172 bytes, hashed at every tick of every block entity).
fn kept_hash(key: &'static str) -> u64 {
    const SLOTS: usize = 64;
    thread_local! {
        static KEPT: std::cell::RefCell<[(usize, usize, u64); SLOTS]> = const { std::cell::RefCell::new([(0, 0, 0); SLOTS]) };
    }
    let (at, len) = (key.as_ptr() as usize, key.len());
    let slot = ((at >> 3) ^ len) % SLOTS;
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        let (a, l, h) = kept[slot];
        if a == at && l == len {
            return h;
        }
        let h = key_hash(key);
        kept[slot] = (at, len, h);
        h
    })
}

/// Virtual dispatch by key (`name.desc`): the API's calls back into the code.
pub fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
    let cid = rt.recv_cid(args);
    rt.entered(
        |rt| dispatch_any(rt, cid, kept_hash(key), key, owner_key, args),
        |r| *r,
    )
}

/// Reflection over the translated classes ([`Rt::reflect`]).
pub fn reflect(rt: &mut Rt, class: &str, what: &str, args: &[V]) -> Option<V> {
    let l = rt.linked?;
    if what == "services"
        && let Some(names) = l.services.get(class).copied()
    {
        let a = rt.new_array(names.len() as i32, V::NULL);
        for (i, n) in names.iter().enumerate() {
            let v = rt.string(n);
            rt.array_set(a, i as i32, v);
        }
        return Some(a);
    }
    let d = l.id(class).and_then(|id| l.class(id))?;
    if let Some(r) = d.reflect
        && let Some(v) = rt.entered(|rt| r(rt, what, args), |made| made.unwrap_or_default())
    {
        return Some(v);
    }
    match what {
        // An object of the class no constructor has run on (what a reader of saved objects makes
        // when the class has no constructor without arguments).
        "allocate" => Some(rt.new_object_id(d.name, d.id)),
        "fields" if !d.fields.is_empty() => Some(rt.string(d.fields)),
        "annotations" if !d.annotations.is_empty() => Some(rt.string(d.annotations)),
        _ => None,
    }
}

const TO_STRING: u64 = key_hash("toString.()Ljava/lang/String;");

/// `String.valueOf(v)` for an object: its translated class's own `toString()`; else the API's
/// for its class, where that is more than `Object`'s (a formatting code is its control sequence,
/// a JSON element its text, a string builder what it holds); else an enum constant's name, or
/// `v` itself (strings, boxes and collections, which the runtime prints).
pub fn java_string(rt: &mut Rt, v: V) -> V {
    string_at(rt, v, 0)
}

/// [`java_string`] of an object `depth` collections down.
fn string_at(rt: &mut Rt, v: V, depth: u32) -> V {
    if !matches!(v, V::R(_)) || v.r() == 0 || rt.str_of(v).is_some() {
        return v;
    }
    let cid = rt.cid(v);
    if let Some(f) = rt.linked.and_then(|l| l.resolve(cid, TO_STRING)) {
        return rt.entered(|rt| f(rt, &[v]), |r| *r);
    }
    if depth < 16 {
        // It holds the collection's elements while it asks each for its text: the runtime's
        // own code, as far as a collection goes.
        rt.native += 1;
        let text = collection_string(rt, v, depth);
        rt.native = rt.native.saturating_sub(1);
        if let Some(text) = text {
            return rt.alloc("java/lang/String", Data::Str(text));
        }
    }
    // (A lambda has no class the API knows: it prints as an object of its own.)
    if cid & LAMBDA == 0 && !rt.prints(v) && !rt.class_of(v).contains('.') {
        let class = rt.class_of(v);
        let key = rt.static_key(&format!("{class}.toString.()Ljava/lang/String;"));
        let thrown = rt.thrown.take();
        let text = crate::api::call(rt, key, &[v]);
        // An exception of the question is not the code's.
        rt.thrown = thrown;
        let plain = format!("{}@{:x}", class.replace('/', "."), v.r());
        if rt.str_of(text).is_some_and(|t| t != plain) {
            return text;
        }
    }
    let name = rt.get_field(v, "$name");
    if rt.str_of(name).is_some() { name } else { v }
}

/// A collection, a map or a map's entry of Java's own as `AbstractCollection.toString` and
/// `AbstractMap.toString` write them: each element as [`java_string`] writes it, so one of the
/// code's own classes or of the API's prints by its `toString()` there too (one that holds
/// itself says so). None for another object.
fn collection_string(rt: &mut Rt, v: V, depth: u32) -> Option<String> {
    let class = rt.class_of(v);
    let entry = matches!(
        class,
        "java/util/Map$Entry"
            | "java/util/AbstractMap$SimpleEntry"
            | "java/util/AbstractMap$SimpleImmutableEntry"
    );
    let (pairs, items): (Option<Vec<(V, V)>>, Vec<V>) =
        match rt.heap.get(v.r() as usize).map(|o| &o.data)? {
            Data::List(kv) if entry && kv.len() == 2 => (Some(vec![(kv[0], kv[1])]), Vec::new()),
            Data::List(items) if rt.is_a(class, "java/util/Collection") => (None, items.clone()),
            Data::Table(t) if rt.is_a(class, "java/util/Map") => {
                (Some(t.entries.clone()), Vec::new())
            }
            Data::Table(t) if rt.is_a(class, "java/util/Collection") => {
                (None, t.entries.iter().map(|&(k, _)| k).collect())
            }
            _ => return None,
        };
    let shown = |rt: &mut Rt, e: V, this: &str| {
        if e == v {
            return this.to_owned();
        }
        let text = string_at(rt, e, depth + 1);
        rt.to_java_string(text, 'L')
    };
    Some(match pairs {
        Some(pairs) => {
            let mut parts = Vec::with_capacity(pairs.len());
            for (k, e) in pairs {
                let (k, e) = (shown(rt, k, "(this Map)"), shown(rt, e, "(this Map)"));
                parts.push(format!("{k}={e}"));
            }
            if entry {
                parts.concat()
            } else {
                format!("{{{}}}", parts.join(", "))
            }
        }
        None => {
            let mut parts = Vec::with_capacity(items.len());
            for e in items {
                parts.push(shown(rt, e, "(this Collection)"));
            }
            format!("[{}]", parts.join(", "))
        }
    })
}

/// Every key (`name.desc`) of an instance method some translated class has, sorted: what
/// reflection looks a method up in by its name and parameter types.
pub type Keys = BTreeSet<&'static str>;

#[cfg(test)]
mod tests {
    use super::*;

    fn api_miss(_: &mut Rt, _: &'static str, _: &[V]) -> V {
        V::I(-1)
    }

    fn install() {
        crate::install_fns(crate::Fns {
            call: api_miss,
            get_static: |_, _| V::Void,
            constructor_arg: |_, _| V::Void,
            fire_registrations: |_| {},
            api_is_a: |class, of| class == "java/util/ArrayList" && of == "java/util/Collection",
            vanilla_class: |_, _, _| None,
        });
    }

    fn base_tick(_: &mut Rt, _: &[V]) -> V {
        V::I(1)
    }
    fn child_tick(_: &mut Rt, _: &[V]) -> V {
        V::I(2)
    }
    fn mixin_tick(_: &mut Rt, _: &[V]) -> V {
        V::I(3)
    }
    fn power_default(_: &mut Rt, _: &[V]) -> V {
        V::I(4)
    }
    fn private_helper(_: &mut Rt, _: &[V]) -> V {
        V::I(5)
    }
    fn deeper_helper(_: &mut Rt, _: &[V]) -> V {
        V::I(6)
    }
    fn mixin_name(_: &mut Rt, _: &[V]) -> V {
        V::I(7)
    }
    fn to_string(rt: &mut Rt, _: &[V]) -> V {
        rt.string("machine")
    }
    fn add(_: &mut Rt, full: Vec<V>) -> V {
        V::I(full.iter().map(|v| v.i()).sum())
    }
    fn base_reflect(rt: &mut Rt, what: &str, _: &[V]) -> Option<V> {
        (what == "<clinit>").then(|| {
            rt.statics.insert("m/Base.READY", V::I(1));
            V::Void
        })
    }

    const POWER: &str = "power.()I";
    const HELPER: &str = "helper.()I";
    const NAME: &str = "name.()I";
    const ENTITY: &str = "net/minecraft/world/entity/Entity";

    static CLASSES: [ClassDef; 5] = [
        ClassDef {
            name: "m/Base",
            id: 1,
            superclass: Some("net/minecraft/world/level/block/entity/BlockEntity"),
            interfaces: &["m/Powered"],
            methods: &[
                ("tick.()V", base_tick, false),
                ("toString.()Ljava/lang/String;", to_string, false),
            ],
            mixed: &[],
            reflect: Some(base_reflect),
            fields: "2 energy J",
            annotations: "",
            subscribers: &[],
        },
        ClassDef {
            name: "m/Child",
            id: 2,
            superclass: Some("m/Base"),
            interfaces: &[],
            methods: &[("tick.()V", child_tick, false)],
            mixed: &[],
            reflect: None,
            fields: "",
            annotations: "",
            subscribers: &[(
                "net/neoforged/neoforge/event/tick/ServerTickEvent$Post",
                "m/Child.onTick.(Lnet/neoforged/neoforge/event/tick/ServerTickEvent$Post;)V",
                false,
                0,
            )],
        },
        ClassDef {
            name: "m/Mixed",
            id: 3,
            superclass: Some("m/Base"),
            interfaces: &[],
            methods: &[("tick.()V", child_tick, false)],
            mixed: &[("tick.()V", mixin_tick)],
            reflect: None,
            fields: "",
            annotations: "",
            subscribers: &[],
        },
        ClassDef {
            name: "m/Powered",
            id: 4,
            superclass: Some("java/lang/Object"),
            interfaces: &["m/Deeper"],
            methods: &[
                (POWER, power_default, false),
                (HELPER, private_helper, true),
            ],
            mixed: &[],
            reflect: None,
            fields: "",
            annotations: "",
            subscribers: &[],
        },
        ClassDef {
            name: "m/Deeper",
            id: 5,
            superclass: Some("java/lang/Object"),
            interfaces: &[],
            methods: &[(HELPER, deeper_helper, false)],
            mixed: &[],
            reflect: None,
            fields: "",
            annotations: "",
            subscribers: &[],
        },
    ];

    static ADDED: [Added; 1] = [Added {
        key: NAME,
        run: mixin_name,
        owners: &[
            "m/EntityMixin.name.()I",
            "net/minecraft/world/entity/Entity.name.()I",
        ],
        checked: &["net/minecraft/world/Nameable.name.()I"],
        targets: &[ENTITY],
    }];

    static REGISTRY: Registry = Registry {
        classes: &CLASSES,
        lambdas: &[(0, add)],
        added: &ADDED,
        game_interfaces: &[(ENTITY, &["m/EntityAccess"])],
        subscriber_options: &[],
    };

    static SERVICES: [(&str, &[&str]); 1] = [("m/Powered", &["m/Child"])];

    fn runtime() -> Rt {
        static LINKED: std::sync::OnceLock<Linked> = std::sync::OnceLock::new();
        install();
        let mut rt = Rt::new();
        rt.link(LINKED.get_or_init(|| Linked::new([&REGISTRY], &SERVICES)));
        rt
    }

    fn call(rt: &mut Rt, receiver: V, owner_key: &'static str, key: &'static str) -> i32 {
        let cid = rt.cid(receiver);
        dispatch(rt, cid, key_hash(key), key, owner_key, &[receiver]).i()
    }

    #[test]
    fn the_key_hash_is_fnv_1a() {
        assert_eq!(key_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(key_hash("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(key_hash("foobar"), 0x8594_4171_f739_67e8);
    }

    /// A kept hash is the key's own, however many keys share its slot: two hundred keys, more
    /// than there are slots, asked twice over.
    #[test]
    fn a_kept_hash_is_the_keys_hash() {
        let keys: Vec<&'static str> = (0..200)
            .map(|i| &*String::leak(format!("m{i}.()V")))
            .collect();
        for _ in 0..2 {
            for k in &keys {
                assert_eq!(kept_hash(k), key_hash(k), "{k}");
            }
        }
    }

    /// A call finds the method as the JVM resolves it: the receiver's class first (what a mixin
    /// adds to it before its own), then its superclasses, then the default methods of their
    /// superinterfaces breadth first, where a private method is no default; past them, the API's.
    #[test]
    fn a_call_finds_its_method_as_the_jvm_resolves_it() {
        let mut rt = runtime();
        let (base, child, mixed) = (
            rt.new_object("m/Base"),
            rt.new_object("m/Child"),
            rt.new_object("m/Mixed"),
        );
        let owner = "net/minecraft/world/level/block/entity/BlockEntity.tick.()V";
        assert_eq!(call(&mut rt, base, owner, "tick.()V"), 1);
        assert_eq!(call(&mut rt, child, owner, "tick.()V"), 2);
        assert_eq!(call(&mut rt, mixed, owner, "tick.()V"), 3);
        assert_eq!(call(&mut rt, child, "m/Powered.power.()I", POWER), 4);
        assert_eq!(call(&mut rt, child, "m/Powered.helper.()I", HELPER), 6);
        assert_eq!(
            call(
                &mut rt,
                child,
                "net/minecraft/world/level/block/entity/BlockEntity.isRemoved.()Z",
                "isRemoved.()Z"
            ),
            -1
        );
        // Asked again (kept now), the same.
        assert_eq!(call(&mut rt, child, owner, "tick.()V"), 2);
        let linked = rt.linked.unwrap();
        assert!(linked.overrides("m/Child", POWER));
        assert!(!linked.overrides("m/Child", "isRemoved.()Z"));
        assert!(!linked.overrides("m/Unknown", POWER));
        // The API's calls back in by key.
        let vcall = rt.vcall.unwrap();
        assert_eq!(vcall(&mut rt, owner, "tick.()V", &[child]).i(), 2);
    }

    /// A method a mixin adds to a game class answers a call whose owner key is its own (the mixin's,
    /// its target's), on any object; a call through an interface it adds only on an object of its
    /// target. A lambda never takes it.
    #[test]
    fn a_method_a_mixin_adds_to_the_game_answers_by_the_calls_owner() {
        let mut rt = runtime();
        let entity = rt.new_object(ENTITY);
        let stack = rt.new_object("net/minecraft/world/item/ItemStack");
        assert_eq!(
            call(
                &mut rt,
                entity,
                "net/minecraft/world/entity/Entity.name.()I",
                NAME
            ),
            7
        );
        assert_eq!(call(&mut rt, stack, "m/EntityMixin.name.()I", NAME), 7);
        assert_eq!(
            call(
                &mut rt,
                entity,
                "net/minecraft/world/Nameable.name.()I",
                NAME
            ),
            7
        );
        assert_eq!(
            call(
                &mut rt,
                stack,
                "net/minecraft/world/Nameable.name.()I",
                NAME
            ),
            -1
        );
        assert_eq!(
            call(
                &mut rt,
                entity,
                "net/minecraft/world/entity/Mob.name.()I",
                NAME
            ),
            -1
        );
        assert!(rt.instance_of(entity, "m/EntityAccess"));
    }

    /// A lambda runs its implementation with its captured values first; its functional method by
    /// name, and others are the API's.
    #[test]
    fn a_lambda_runs_with_its_captured_values_first() {
        let mut rt = runtime();
        let lambda = rt.lambda_id("m/Base.lambda$0.(II)I", 0, vec![V::I(10)]);
        assert_eq!(lambda_call(&mut rt, 0, &[lambda, V::I(5)]), Some(V::I(15)));
        assert_eq!(lambda_call(&mut rt, 9, &[lambda, V::I(5)]), None);
        rt.lambda_info.insert(
            lambda.r(),
            ("java/util/function/IntUnaryOperator", "applyAsInt"),
        );
        let vcall = rt.vcall.unwrap();
        assert_eq!(
            vcall(
                &mut rt,
                "java/util/function/IntUnaryOperator.applyAsInt.(I)I",
                "applyAsInt.(I)I",
                &[lambda, V::I(5)]
            )
            .i(),
            15
        );
        assert_eq!(vcall(&mut rt, "java/util/function/IntUnaryOperator.andThen.(Ljava/util/function/IntUnaryOperator;)Ljava/util/function/IntUnaryOperator;", "andThen.(Ljava/util/function/IntUnaryOperator;)Ljava/util/function/IntUnaryOperator;", &[lambda, V::NULL]).i(), -1);
    }

    /// Reflection reaches a class's own function, then its fields and annotations; the services an
    /// interface lists; nothing of a class the code doesn't have. A runtime joined to the space
    /// knows the classes' ids, superclasses, listeners and the interfaces mixins add.
    #[test]
    fn reflection_and_the_runtime_see_the_registered_classes() {
        let mut rt = runtime();
        assert_eq!(rt.reflect("m/Base", "<clinit>", &[]), Some(V::Void));
        assert_eq!(rt.statics.get("m/Base.READY"), Some(&V::I(1)));
        let fields = rt.reflect("m/Base", "fields", &[]).unwrap();
        assert_eq!(rt.str_of(fields), Some("2 energy J"));
        assert_eq!(rt.reflect("m/Base", "annotations", &[]), None);
        assert_eq!(rt.reflect("m/Child", "fields", &[]), None);
        let services = rt.reflect("m/Powered", "services", &[]).unwrap();
        let first = rt.array_get(services, 0);
        assert_eq!(rt.str_of(first), Some("m/Child"));
        assert_eq!(rt.reflect("m/Unknown", "<clinit>", &[]), None);
        assert_eq!(rt.class_ids.get("m/Child"), Some(&2));
        assert_eq!(rt.supers.get("m/Child"), Some(&"m/Base"));
        assert!(rt.is_a("m/Child", "m/Deeper"));
        assert_eq!(rt.subscribers.get("m/Child").map(Vec::len), Some(1));
        assert!(rt.vkeys.contains(POWER));
        // `String.valueOf`: the class's own `toString()`, else an enum constant's name.
        let child = rt.new_object("m/Child");
        let text = java_string(&mut rt, child);
        assert_eq!(rt.str_of(text), Some("machine"));
        let constant = rt.new_object("m/Deeper");
        let name = rt.string("NORTH");
        rt.put_field(constant, "$name", name);
        assert_eq!(java_string(&mut rt, constant), name);
        // In a collection each element is written the same way: the class's own `toString()`.
        let list = rt.alloc(
            "java/util/ArrayList",
            Data::List(vec![child, V::I(5), constant]),
        );
        let text = java_string(&mut rt, list);
        assert_eq!(rt.str_of(text), Some("[machine, 5, NORTH]"));
        // A call the code makes on `null` throws, as the JVM does; the runtime's own call of
        // what may be null (`vcall`) takes the API's answer as before.
        assert_eq!(
            vcall(&mut rt, "m/Base.power.()I", POWER, &[V::NULL]),
            V::I(-1)
        );
        assert!(rt.thrown.is_none());
        dispatch(
            &mut rt,
            0,
            kept_hash(POWER),
            POWER,
            "m/Base.power.()I",
            &[V::NULL],
        );
        let thrown = rt.thrown.take().expect("a call on null throws");
        assert_eq!(rt.class_of(thrown), "java/lang/NullPointerException");
        assert_eq!(
            rt.str_of(rt.get_field(thrown, "message")),
            Some("Cannot invoke \"m.Base.power()\" because the object is null")
        );
        // A lambda is not asked of the API: it has no class there.
        let key = "m/Base.lambda$tick$0.()Ljava/util/function/Supplier;";
        for lambda in [rt.lambda_id(key, 3, Vec::new()), rt.lambda(key, Vec::new())] {
            let asked = rt.unmapped.len();
            assert_eq!(java_string(&mut rt, lambda), lambda);
            assert_eq!(rt.unmapped.len(), asked);
        }
        // What an unmapped call answered is not null.
        assert_eq!(
            dispatch(
                &mut rt,
                0,
                kept_hash(POWER),
                POWER,
                "m/Base.power.()I",
                &[V::Void]
            ),
            V::I(-1)
        );
        assert!(rt.thrown.is_none());
    }
}
