//! Which code of the libraries a program carries it can run: rapid type analysis from the mods'
//! own code. A large library (Kotlin's, ~4 700 classes behind a Kotlin mod) is translated only as
//! far as the mods reach into it.
//!
//! A mod's own classes run whole and count as created. From their code: a static or `super` call
//! runs the method it names, as the JVM resolves it (up the superclasses, then default methods); a
//! virtual or interface call runs the method of its name and descriptor in each library class
//! that is, or is above, a created class (`new`); a field or static method initializes its class
//! (`<clinit>`, its superclasses' too). Method handles (lambdas, method references) run their
//! method. A created library class whose supertypes leave the library (other than for `Object`)
//! may be called from code that isn't translated (the runtime, Java's own classes): all its
//! instance methods run; any runs `toString`, `equals` and `hashCode`. A lambda counts as an
//! instance of its functional interface, so the default methods called on it run. A string naming a library
//! class in a mod's code (reflection) creates it with any constructor; a library's own such
//! strings name its optional parts, left out (`Class.forName` then fails, as when they are
//! absent). A reached service interface creates the implementations its jars list.
use crate::{
    class::{ClassFile, Const, Method},
    insn,
};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A method by name and descriptor.
type Sig = (String, String);

/// What [`reach`] finds: each library class the program uses, with the methods it can run (none
/// when only its type is used: a cast, a field, a superclass).
pub type Reached = HashMap<String, HashSet<Sig>>;

struct Walk<'a> {
    by_name: HashMap<&'a str, &'a ClassFile>,
    library: &'a HashSet<String>,
    provided: &'a dyn Fn(&str) -> bool,
    services: &'a BTreeMap<String, Vec<String>>,
    kept: Reached,
    created: HashSet<String>,
    /// Each created class's supertypes, and the class: a call's method in one of them may run.
    above_created: HashSet<String>,
    called: HashSet<Sig>,
    /// The types each signature was called through (the owner a call names).
    called_through: HashMap<Sig, HashSet<String>>,
    /// The game's own classes, when its code is a library here: what tells which of that
    /// code's methods a call can reach.
    game: Option<&'a crate::derive::Game>,
    /// The library classes with an instance method of each signature (with code).
    declared: HashMap<Sig, Vec<&'a str>>,
    todo: Vec<(&'a ClassFile, &'a Method)>,
}

impl<'a> Walk<'a> {
    fn is_library(&self, class: &str) -> bool {
        self.library.contains(class) && !(self.provided)(class) && self.by_name.contains_key(class)
    }

    /// The class is used: kept, with its library supertypes, and a service interface's
    /// implementations created.
    fn keep(&mut self, class: &str) {
        if !self.is_library(class) || self.kept.contains_key(class) {
            return;
        }
        self.kept.insert(class.to_owned(), HashSet::new());
        let c = self.by_name[class];
        for sup in c.superclass.iter().chain(&c.interfaces) {
            self.keep(sup);
        }
        if let Some(impls) = self.services.get(class) {
            for i in impls.clone() {
                self.create(&i);
                self.run(&i, "<init>", "()V");
            }
        }
        self.make_posted(class);
    }

    /// An event of the game's the runtime makes by its own code and posts
    /// (`horizon_compat::code_event_classes`): no mod's code names its constructor, and the
    /// runtime calls it, so the class is created with each of them, and the methods the runtime
    /// asks of the event afterwards run too. What the mods' listeners ask of it runs as any call
    /// on a created class does.
    fn make_posted(&mut self, class: &str) {
        let Some((_, asked)) = horizon_compat::code_event_classes::CLASSES.iter().find(|c| c.0 == class) else {
            return;
        };
        if !self.is_library(class) || self.created.contains(class) {
            return;
        }
        self.create(class);
        let inits: Vec<String> = self.by_name[class].methods.iter().filter(|m| m.name == "<init>").map(|m| m.desc.clone()).collect();
        for d in inits {
            self.run(class, "<init>", &d);
        }
        for (name, desc) in asked.iter().filter_map(|m| m.split_once('.')) {
            self.run_resolved(class, name, desc);
        }
    }

    /// The method runs, if `class` declares it with code.
    fn run(&mut self, class: &str, name: &str, desc: &str) {
        if !self.is_library(class) {
            return;
        }
        self.keep(class);
        let c = self.by_name[class];
        let Some(m) = c.methods.iter().find(|m| m.name == name && m.desc == desc) else { return };
        if self.kept.get_mut(class).is_some_and(|set| set.insert((name.to_owned(), desc.to_owned()))) {
            self.todo.push((c, m));
        }
    }

    /// The class and its library superclasses are initialized.
    fn init(&mut self, class: &str) {
        let mut at = Some(class.to_owned());
        while let Some(c) = at.filter(|c| self.is_library(c)) {
            self.run(&c, "<clinit>", "()V");
            at = self.by_name[c.as_str()].superclass.clone();
        }
        self.keep(class);
    }

    /// Where the JVM resolves a call to `class.name desc` without dispatch (`invokestatic`,
    /// `invokespecial`, a method handle): the first superclass that declares it, else a default
    /// method of an interface.
    fn run_resolved(&mut self, class: &str, name: &str, desc: &str) {
        let mut at = Some(class.to_owned());
        while let Some(c) = at {
            let Some(cf) = self.by_name.get(c.as_str()).copied() else { break };
            if cf.methods.iter().any(|m| m.name == name && m.desc == desc) {
                self.run(&c, name, desc);
                return;
            }
            at = cf.superclass.clone();
        }
        self.run_default(class, name, desc, &mut HashSet::new());
    }

    /// The default methods of `class`'s interfaces (and theirs) for `name desc`.
    fn run_default(&mut self, class: &str, name: &str, desc: &str, seen: &mut HashSet<String>) {
        let Some(cf) = self.by_name.get(class).copied() else { return };
        let mut ups: Vec<String> = cf.interfaces.clone();
        ups.extend(cf.superclass.clone());
        for i in ups {
            if !seen.insert(i.clone()) {
                continue;
            }
            match self.by_name.get(i.as_str()).copied() {
                Some(icf) if icf.methods.iter().any(|m| m.name == name && m.desc == desc && m.code.is_some()) => self.run(&i, name, desc),
                Some(_) => self.run_default(&i, name, desc, seen),
                None => {}
            }
        }
    }

    /// The class and its supertypes, as far as the program has them.
    fn ancestors(&self, class: &str) -> Vec<&'a str> {
        let mut out: Vec<&'a str> = Vec::new();
        let mut todo: Vec<&str> = vec![class];
        while let Some(c) = todo.pop() {
            let Some(cf) = self.by_name.get(c).copied() else { continue };
            if out.contains(&cf.name.as_str()) {
                continue;
            }
            out.push(cf.name.as_str());
            todo.extend(cf.superclass.iter().chain(&cf.interfaces).map(String::as_str));
        }
        out
    }

    /// Whether `class` has a supertype outside the library other than `Object`: code that isn't
    /// translated may call any of its methods.
    fn outside_supertype(&self, class: &str) -> bool {
        let Some(cf) = self.by_name.get(class).copied() else { return false };
        cf.superclass.iter().chain(&cf.interfaces).any(|s| {
            s != "java/lang/Object" && (!self.library.contains(s) || (self.by_name.contains_key(s.as_str()) && self.outside_supertype(s)))
        })
    }

    /// The class and everything above it, through the program's classes and the game's.
    fn ups(&self, class: &str) -> HashSet<String> {
        let (mut out, mut todo) = (HashSet::new(), vec![class.to_owned()]);
        while let Some(c) = todo.pop() {
            let Some(cf) = self
                .by_name
                .get(c.as_str())
                .copied()
                .or_else(|| self.game.and_then(|g| g.class(&c)))
            else {
                continue;
            };
            if out.insert(c) {
                todo.extend(cf.superclass.iter().chain(&cf.interfaces).cloned());
            }
        }
        out
    }

    fn known(&self, class: &str) -> bool {
        self.by_name.contains_key(class) || self.game.is_some_and(|g| g.class(class).is_some())
    }

    /// Whether a call through `owner` can reach an object of `class` (one of the game's own):
    /// the class is below the owner, or the owner is a type nothing here describes (Java's).
    fn receives(&self, class: &str, owner: &str) -> bool {
        !self.known(owner) || self.ups(class).contains(owner)
    }

    /// The methods of a game class that code outside the translation can call: those a class
    /// above it that isn't translated declares (the runtime calls a block's `getShape`), and
    /// Java's own interfaces' ([`JAVA_CALLS`]).
    fn called_from_outside(&self, class: &str) -> HashSet<Sig> {
        let mut out = HashSet::new();
        for up in self.ups(class) {
            if self.is_library(&up) {
                continue;
            }
            if let Some(cf) = self
                .by_name
                .get(up.as_str())
                .copied()
                .or_else(|| self.game.and_then(|g| g.class(&up)))
            {
                out.extend(
                    cf.methods
                        .iter()
                        .filter(|m| m.access & 0x000a == 0)
                        .map(|m| (m.name.clone(), m.desc.clone())),
                );
            }
        }
        out
    }

    /// An instance of the class is created: the calls made so far may run its methods, and
    /// those it inherits.
    fn create(&mut self, class: &str) {
        if !self.by_name.contains_key(class) || !self.created.insert(class.to_owned()) {
            return;
        }
        self.init(class);
        let outside = self.is_library(class) && self.outside_supertype(class);
        // One the runtime calls by itself: whatever of it isn't private runs.
        let driven = self
            .ancestors(class)
            .iter()
            .any(|a| crate::DRIVEN_CODE.contains(a));
        // The game's own code (and the Java runtime's) is too large to run whole behind each class
        // with a supertype the runtime has: only what can be called there, and what a call through a type above the
        // class reaches.
        let narrow = (self.game.is_some() && crate::install_code(class))
            .then(|| self.called_from_outside(class));
        for a in self.ancestors(class) {
            self.above_created.insert(a.to_owned());
            if !self.is_library(a) {
                continue;
            }
            let cf = self.by_name[a];
            let sigs: Vec<Sig> = cf
                .methods
                .iter()
                .filter(|m| m.access & 0x0008 == 0 && m.code.is_some() && !m.name.starts_with('<'))
                .map(|m| (m.access, (m.name.clone(), m.desc.clone())))
                .filter(|(access, sig)| match &narrow {
                    _ if driven && access & 0x0002 == 0 => true,
                    Some(out) => {
                        (outside && access & 0x0002 == 0 && (out.contains(sig) || JAVA_CALLS.contains(&sig.0.as_str())))
                            || self.called_through.get(sig).is_some_and(|owners| owners.iter().any(|o| self.receives(class, o)))
                            // What any object is asked (a map's key, a logged value).
                            || (matches!(sig.0.as_str(), "toString" | "equals" | "hashCode") && self.called.contains(sig))
                    }
                    None => (outside && access & 0x0002 == 0) || self.called.contains(sig),
                })
                .map(|(_, sig)| sig)
                .collect();
            for (n, d) in sigs {
                self.run(a, &n, &d);
            }
        }
    }

    /// A virtual or interface call to `sig`: the method of that signature in each library class a
    /// created class has above it (or is) may run.
    fn call(&mut self, owner: &str, sig: Sig) {
        let through = self
            .called_through
            .entry(sig.clone())
            .or_default()
            .insert(owner.to_owned());
        let first = self.called.insert(sig.clone());
        if !(first || (through && self.game.is_some())) {
            return;
        }
        let targets: Vec<&'a str> = self.declared.get(&sig).into_iter().flatten().copied().filter(|c| self.above_created.contains(*c)).collect();
        for c in targets {
            // A method of the game's own: only where an object the call can reach has it (the
            // class is below the call's type, or above it and so inherited there).
            if self.game.is_some()
                && crate::install_code(c)
                && self.known(owner)
                && !self.ups(c).contains(owner)
                && !self.ups(owner).contains(c)
            {
                continue;
            }
            self.run(c, &sig.0, &sig.1);
        }
    }

    /// A method handle constant: its method runs (a constructor's class is created).
    fn handle(&mut self, c: &ClassFile, i: u16) {
        if let Some((kind, m)) = c.handle(i) {
            // JVMS 5.4.3.5: 1-4 a field, 5 virtual, 6 static, 7 special, 8 a constructor, 9 interface.
            match kind {
                1..=4 | 6 => self.init(&m.owner),
                5 | 9 => {
                    self.keep(&m.owner);
                    self.call(&m.owner, (m.name.clone(), m.desc.clone()));
                }
                8 => self.create(&m.owner),
                _ => self.keep(&m.owner),
            }
            self.run_resolved(&m.owner, &m.name, &m.desc);
        }
    }

    /// A call to a class outside the program that returns one of the classes the runtime makes
    /// and calls itself ([`crate::DRIVEN_CODE`]: `Gson.newJsonWriter(writer)`): the class is
    /// created there, by any of its constructors.
    fn made_outside(&mut self, owner: &str, desc: &str) {
        if self.by_name.contains_key(owner) {
            return;
        }
        let Some(made) = desc
            .rsplit_once(")L")
            .and_then(|(_, r)| r.strip_suffix(';'))
        else {
            return;
        };
        if !crate::DRIVEN_CODE.contains(&made) || !self.is_library(made) {
            return;
        }
        self.create(made);
        let inits: Vec<String> = self.by_name[made]
            .methods
            .iter()
            .filter(|m| m.name == "<init>")
            .map(|m| m.desc.clone())
            .collect();
        for d in inits {
            self.run(made, "<init>", &d);
        }
    }

    /// A class named by a constant (an array's element class).
    fn keep_named(&mut self, name: &str) {
        let name = name.trim_start_matches('[');
        let name = name.strip_prefix('L').and_then(|n| n.strip_suffix(';')).unwrap_or(name);
        self.keep(name);
    }

    /// What a method's code reaches.
    fn scan(&mut self, c: &'a ClassFile, m: &'a Method) {
        let Some(code) = &m.code else { return };
        for (_, _, _, catch) in &code.handlers {
            self.keep(catch);
        }
        let Ok(insns) = insn::decode(&code.bytes) else { return };
        for i in insns {
            let idx = match i.arg {
                insn::Operand::Int(v) => v as u16,
                insn::Operand::IndexCount(v, _) => v,
                _ => continue,
            };
            match i.op {
                0xb6 | 0xb9 => {
                    if let Some(r) = c.member(idx) {
                        self.keep(&r.owner);
                        self.made_outside(&r.owner, &r.desc);
                        self.call(&r.owner, (r.name, r.desc));
                    }
                }
                0xb7 => {
                    if let Some(r) = c.member(idx) {
                        self.keep(&r.owner);
                        self.run_resolved(&r.owner, &r.name, &r.desc);
                    }
                }
                0xb8 => {
                    if let Some(r) = c.member(idx) {
                        self.init(&r.owner);
                        self.run_resolved(&r.owner, &r.name, &r.desc);
                    }
                }
                0xb2 | 0xb3 => {
                    if let Some(r) = c.member(idx) {
                        self.init(&r.owner);
                    }
                }
                0xb4 | 0xb5 => {
                    if let Some(r) = c.member(idx) {
                        self.keep(&r.owner);
                    }
                }
                0xbb => {
                    if let Ok(n) = c.class_name(idx) {
                        self.create(&n);
                    }
                }
                0xbd | 0xc0 | 0xc1 | 0xc5 => {
                    if let Ok(n) = c.class_name(idx) {
                        self.keep_named(&n);
                    }
                }
                0x12 | 0x13 => match c.pool.get(usize::from(idx)) {
                    Some(Const::Class(_)) => {
                        if let Ok(n) = c.class_name(idx) {
                            self.keep_named(&n);
                        }
                    }
                    // A library's own lookups by name are of its optional parts (Kotlin's
                    // `Reflection` asks for kotlin-reflect): left out, they fall back as when absent.
                    Some(Const::String(s)) if !self.library.contains(&c.name) => {
                        if let Ok(s) = c.utf8(*s) {
                            let n = s.replace('.', "/");
                            if self.is_library(&n) {
                                self.create(&n);
                                let inits: Vec<String> = self.by_name[n.as_str()].methods.iter().filter(|m| m.name == "<init>").map(|m| m.desc.clone()).collect();
                                for d in inits {
                                    self.run(&n, "<init>", &d);
                                }
                            }
                        }
                    }
                    Some(Const::MethodHandle(..)) => self.handle(c, idx),
                    _ => {}
                },
                0xba => {
                    let Some((bsm, _, desc)) = c.indy(idx) else { continue };
                    let Some((h, args)) = c.bootstrap.get(usize::from(bsm)).cloned() else { continue };
                    // A lambda is an instance of its functional interface (what the call site
                    // returns): the interface's default methods run when called on it, as a
                    // class implementing it would have them.
                    if c.handle(h).is_some_and(|(_, m)| m.owner == "java/lang/invoke/LambdaMetafactory")
                        && let Some(iface) = desc.rsplit_once(')').and_then(|(_, r)| r.strip_prefix('L')).and_then(|r| r.strip_suffix(';'))
                    {
                        self.create(iface);
                    }
                    self.handle(c, h);
                    for a in args {
                        match c.pool.get(usize::from(a)) {
                            Some(Const::MethodHandle(..)) => self.handle(c, a),
                            Some(Const::Class(_)) => {
                                if let Ok(n) = c.class_name(a) {
                                    self.keep_named(&n);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The classes a method's descriptor names (`(Lfoo/Bar;I)V`: `foo/Bar`), its return type's too.
fn object_types(desc: &str) -> Vec<&str> {
    let (mut out, mut rest) = (Vec::new(), desc);
    // A type starts where the last ended: `L` there opens a class name, up to its `;` (an `L`
    // inside a name is the name's own).
    while let Some(at) = rest.find(|c: char| !matches!(c, '(' | ')' | '[' | 'B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S' | 'Z' | 'V')) {
        let Some((class, after)) = rest[at..].strip_prefix('L').and_then(|r| r.split_once(';')) else {
            break;
        };
        out.push(class);
        rest = after;
    }
    out
}

/// The library classes (`library`, less what the runtime provides) the program's other classes
/// reach, with the methods of each that can run ([`Reached`]).
pub fn reach(classes: &[ClassFile], library: &HashSet<String>, services: &BTreeMap<String, Vec<String>>, provided: &dyn Fn(&str) -> bool) -> Reached {
    reach_with(classes, library, services, provided, None)
}

/// The methods Java's own interfaces have, by name: what Java's code calls on an object of the
/// game's that implements one (a `Comparable`, an `Iterator`, a `Supplier`), whatever the class.
const JAVA_CALLS: [&str; 30] = [
    "accept",
    "apply",
    "applyAsDouble",
    "applyAsInt",
    "applyAsLong",
    "call",
    "charAt",
    "close",
    "compare",
    "compareTo",
    "equals",
    "get",
    "getAsBoolean",
    "getAsDouble",
    "getAsInt",
    "getAsLong",
    "getKey",
    "getValue",
    "hasNext",
    "hashCode",
    "iterator",
    "length",
    "next",
    "remove",
    "run",
    "size",
    "subSequence",
    "test",
    "toString",
    "isEmpty",
];

/// [`reach`], with the game's own classes (`game`) when its code is among the libraries: a call
/// then reaches a method of that code only in a class its type can be, and a created class of it
/// runs, for the code outside the translation, only what that code can call.
pub fn reach_with(
    classes: &[ClassFile],
    library: &HashSet<String>,
    services: &BTreeMap<String, Vec<String>>,
    provided: &dyn Fn(&str) -> bool,
    game: Option<&crate::derive::Game>,
) -> Reached {
    let mut declared: HashMap<Sig, Vec<&str>> = HashMap::new();
    for c in classes.iter().filter(|c| library.contains(&c.name)) {
        for m in c.methods.iter().filter(|m| m.access & 0x0008 == 0 && m.code.is_some() && !m.name.starts_with('<')) {
            declared.entry((m.name.clone(), m.desc.clone())).or_default().push(c.name.as_str());
        }
    }
    let mut w = Walk {
        by_name: classes.iter().map(|c| (c.name.as_str(), c)).collect(),
        library,
        provided,
        services,
        kept: HashMap::new(),
        created: HashSet::new(),
        above_created: HashSet::new(),
        called: HashSet::new(),
        called_through: HashMap::new(),
        game,
        declared,
        todo: Vec::new(),
    };
    for sig in [("toString", "()Ljava/lang/String;"), ("equals", "(Ljava/lang/Object;)Z"), ("hashCode", "()I")] {
        w.called.insert((sig.0.to_owned(), sig.1.to_owned()));
    }
    let roots: Vec<&ClassFile> = classes.iter().filter(|c| !library.contains(&c.name)).collect();
    for c in &roots {
        w.create(&c.name);
        for sup in c.superclass.iter().chain(&c.interfaces) {
            w.keep(sup);
        }
    }
    // A listener is a method of the mods' that takes its event, whatever it does with it (one
    // that only counts ticks asks nothing of the event): the events the runtime makes by their
    // own code are created for it.
    for c in &roots {
        for m in &c.methods {
            for class in object_types(&m.desc) {
                w.make_posted(class);
            }
        }
    }
    for c in roots {
        for m in &c.methods {
            w.scan(c, m);
        }
    }
    while let Some((c, m)) = w.todo.pop() {
        w.scan(c, m);
    }
    w.kept
}
