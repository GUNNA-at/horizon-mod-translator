//! Which of the game's own methods can stand for ones the runtime doesn't map: a method of a
//! class the runtime models (`Level`, `Entity`, `ItemStack`) whose code, in the player's own
//! copy of the game, asks only what the runtime answers, or what can stand the same way in its
//! turn. `Entity.distanceToSqr(Entity)` is three reads of each position and a sum: with the
//! positions mapped, the game's own code is the answer, and nothing has to be written for it.
//!
//! A method can't stand when its code reads or writes a field the runtime keeps no state for, or
//! calls something neither mapped nor able to stand: each such member is a blocker, the thing to
//! map to let it stand. A call the object's class answers (`invokevirtual`, `invokeinterface`)
//! stands when every class at or below the call's type that has code for it does: the runtime
//! then runs the one of the object's class ([`MARK`]).
use crate::{
    class::{ClassFile, Const, Field, Member, Method},
    insn,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io::Read,
    path::Path,
};

/// Before a method's name, as a field of a class of standing methods: a class below this one has
/// code for the method too, so an object known only as this class may be of that one, and the
/// method found here or above isn't the one to run (`Linked::derived` in horizon-compat-core).
pub const MARK: &str = "!";

/// Whether a class is the Java runtime's own (`java.*`, `javax.*`).
pub fn is_jdk(class: &str) -> bool {
    class.starts_with("java/") || class.starts_with("javax/")
}

/// Whether a class is of the plain libraries read for their code beside the game's: the Java
/// runtime's ([`is_jdk`]), Guava's, Apache Commons'. A static method of one that the runtime has
/// no answer for stands by its own code where that asks only what is answered.
pub fn is_library(class: &str) -> bool {
    is_jdk(class)
        || class.starts_with("com/google/common/")
        || class.starts_with("org/apache/commons/")
}

/// The methods each class of `program` takes from a class of the game's above it though an
/// interface it implements has a default method of the same name and descriptor: a class's method
/// wins over an interface's default, as the JVM resolves a call (TFMG's `CopycatCableBlockEntity`
/// implements `IElectric`, whose default `getBlockPos()` asks `getPos()`, which the class answers
/// from `BlockEntity`'s `getBlockPos()`: taken for each other, the two called each other without
/// end). By class: the name and descriptor of each. A class of the program on the way up that
/// declares the method (its own code, or abstract) settles it there, and is left out.
pub fn inherited_over_defaults(
    program: &[ClassFile],
    game: &Game,
) -> BTreeMap<String, Vec<(String, String)>> {
    let own: HashMap<&str, &ClassFile> = program.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut out = BTreeMap::new();
    for c in program
        .iter()
        .filter(|c| c.access & 0x0200 == 0 && !c.interfaces.is_empty())
    {
        // The default methods of the interfaces the class itself names, and of those above them.
        let mut defaults: BTreeSet<(&str, &str)> = BTreeSet::new();
        let mut todo: Vec<&str> = c.interfaces.iter().map(String::as_str).collect();
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(i) = todo.pop() {
            let Some(k) = own.get(i).filter(|_| seen.insert(i)) else {
                continue;
            };
            defaults.extend(
                k.methods
                    .iter()
                    .filter(|m| {
                        m.code.is_some() && m.access & 0x000a == 0 && !m.name.starts_with('<')
                    })
                    .map(|m| (m.name.as_str(), m.desc.as_str())),
            );
            todo.extend(k.interfaces.iter().map(String::as_str));
        }
        if defaults.is_empty() {
            continue;
        }
        // Up the program's classes to the first that is not one: the game's, if it is there.
        let (mut chain, mut at) = (Vec::new(), Some(c));
        let mut above = None;
        while let Some(k) = at.filter(|_| chain.len() < 1024) {
            chain.push(k);
            above = k.superclass.as_deref();
            at = above.and_then(|s| own.get(s).copied());
        }
        let Some(first) = above.filter(|s| !own.contains_key(s) && game.class(s).is_some()) else {
            continue;
        };
        let supers: Vec<&ClassFile> = {
            let mut v = Vec::new();
            let mut at = game.class(first);
            while let Some(k) = at.filter(|_| v.len() < 1024) {
                v.push(k);
                at = k.superclass.as_deref().and_then(|s| game.class(s));
            }
            v
        };
        let taken: Vec<(String, String)> = defaults
            .into_iter()
            .filter(|(name, desc)| {
                !chain
                    .iter()
                    .any(|k| k.methods.iter().any(|m| m.name == *name && m.desc == *desc))
            })
            .filter(|(name, desc)| {
                supers.iter().any(|k| {
                    k.methods.iter().any(|m| {
                        m.name == *name
                            && m.desc == *desc
                            && m.code.is_some()
                            && m.access & 0x000a == 0
                    })
                })
            })
            .map(|(name, desc)| (name.to_owned(), desc.to_owned()))
            .collect();
        if !taken.is_empty() {
            out.insert(c.name.clone(), taken);
        }
    }
    out
}

/// The Java runtime's classes read for their code ([`Game::open`]): those of `java.lang`,
/// `java.util` and `java.util.function` themselves, where the static helpers a mod calls are
/// (`Arrays`, `Collections`, `Math`, `Character`).
fn jdk_core(class: &str) -> bool {
    ["java/lang/", "java/util/", "java/util/function/"]
        .iter()
        .any(|p| {
            class
                .strip_prefix(p)
                .is_some_and(|rest| !rest.contains('/'))
        })
}

/// The bootstrap methods the translator answers by itself (a lambda, a string joined from
/// parts, a record's `toString`): an `invokedynamic` of one of them is no call to look into.
const BOOTSTRAPS: [&str; 3] = [
    "java/lang/invoke/LambdaMetafactory",
    "java/lang/invoke/StringConcatFactory",
    "java/lang/runtime/ObjectMethods",
];

/// More classes with code for one call than this, and it is not looked into: an interface every
/// widget implements is the runtime's to answer.
const MOST_CLASSES: usize = 24;

/// The classes of the game's own jars, each from the first jar that has it (the NeoForge
/// install's patched classes before the game's unpatched ones).
pub struct Game {
    classes: HashMap<String, ClassFile>,
    /// The classes with code for an instance method of each name and descriptor.
    coded: HashMap<(String, String), Vec<String>>,
    /// The methods that only return an instance field (`return this.f`), by the field's class
    /// and name: (the method's class, its name, its descriptor).
    getters: HashMap<(String, String), Vec<Key>>,
    /// The methods that only set one (`this.f = x`), the same way.
    setters: HashMap<(String, String), Vec<Key>>,
}

impl Game {
    pub fn open(jars: &[&Path]) -> Result<Game, String> {
        Game::open_naming(jars, &HashSet::new())
    }

    /// [`Game::open`] for a program that names the classes `named`: those of them that are enums
    /// of the Java runtime outside its core packages are read too (`HttpClient.Version`,
    /// `Normalizer.Form`), so each is carried with its constants as the game's enums are.
    pub fn open_naming(jars: &[&Path], named: &HashSet<String>) -> Result<Game, String> {
        let mut classes = HashMap::new();
        for jar in jars {
            // A Java runtime's image: the classes of its core packages, whose own code can stand
            // for a static method of theirs the runtime doesn't map ([`is_jdk`]).
            if crate::is_image(jar) {
                let mut image = crate::jimage::Image::open(jar)?;
                let names: Vec<String> = image
                    .names()
                    .filter(|n| {
                        (jdk_core(n) || (is_jdk(n) && named.contains(*n)))
                            && !crate::jdk_code(n)
                            && !classes.contains_key(*n)
                    })
                    .map(str::to_owned)
                    .collect();
                for name in names {
                    if let Some(c) = image.class(&name).and_then(|b| ClassFile::parse(&b).ok())
                        && (jdk_core(&name) || c.access & 0x4000 != 0)
                    {
                        classes.insert(name, c);
                    }
                }
                continue;
            }
            let f = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
            let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", jar.display()))?;
            for i in 0..zip.len() {
                let Ok(mut e) = zip.by_index(i) else { continue };
                let Ok(name) = e.name().map(|n| n.into_owned()) else {
                    continue;
                };
                let Some(class) = name.strip_suffix(".class") else {
                    continue;
                };
                if classes.contains_key(class)
                    || !(crate::is_api(class) || (is_library(class) && !crate::library_code(class)))
                {
                    continue;
                }
                let mut b = Vec::new();
                if e.read_to_end(&mut b).is_ok()
                    && let Ok(c) = ClassFile::parse(&b)
                {
                    classes.insert(c.name.clone(), c);
                }
            }
        }
        Ok(Game::of(classes.into_values().collect()))
    }

    pub fn of(classes: Vec<ClassFile>) -> Game {
        let mut coded: HashMap<(String, String), Vec<String>> = HashMap::new();
        for c in &classes {
            for m in c
                .methods
                .iter()
                .filter(|m| m.access & 0x000a == 0 && m.code.is_some() && !m.name.starts_with('<'))
            {
                coded
                    .entry((m.name.clone(), m.desc.clone()))
                    .or_default()
                    .push(c.name.clone());
            }
        }
        let mut game = Game {
            classes: classes.into_iter().map(|c| (c.name.clone(), c)).collect(),
            coded,
            getters: HashMap::new(),
            setters: HashMap::new(),
        };
        // `aload_0, getfield, return` and `aload_0, load_1, putfield, return`: a field's accessor.
        let (mut getters, mut setters): (
            HashMap<(String, String), Vec<Key>>,
            HashMap<(String, String), Vec<Key>>,
        ) = Default::default();
        for c in game.classes.values() {
            for m in c
                .methods
                .iter()
                .filter(|m| m.access & 0x0008 == 0 && !m.name.starts_with('<'))
            {
                let Some(b) = m.code.as_ref().map(|code| code.bytes.as_slice()) else {
                    continue;
                };
                let (field, to) = match *b {
                    [0x2a, 0xb4, hi, lo, 0xac..=0xb0] if m.desc.starts_with("()") => {
                        (u16::from_be_bytes([hi, lo]), &mut getters)
                    }
                    [0x2a, 0x1b | 0x1f | 0x23 | 0x27 | 0x2b, 0xb5, hi, lo, 0xb1]
                        if m.desc.ends_with(")V") =>
                    {
                        (u16::from_be_bytes([hi, lo]), &mut setters)
                    }
                    _ => continue,
                };
                if let Some(f) = c.member(field)
                    && let Some(class) = game.field_class(&f.owner, &f.name)
                {
                    to.entry((class.to_owned(), f.name)).or_default().push((
                        c.name.clone(),
                        m.name.clone(),
                        m.desc.clone(),
                    ));
                }
            }
        }
        game.getters = getters;
        game.setters = setters;
        game
    }

    pub fn class(&self, name: &str) -> Option<&ClassFile> {
        self.classes.get(name)
    }

    /// The class and everything above it that the jars have, the class first, superclasses
    /// before interfaces.
    pub fn above(&self, class: &str) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        let mut interfaces: Vec<&str> = Vec::new();
        let mut at = self.classes.get(class);
        while let Some(c) = at {
            out.push(c.name.as_str());
            interfaces.extend(c.interfaces.iter().map(String::as_str));
            at = c.superclass.as_deref().and_then(|s| self.classes.get(s));
        }
        let mut next = 0;
        while next < interfaces.len() {
            let i = interfaces[next];
            next += 1;
            if let Some(c) = self
                .classes
                .get(i)
                .filter(|c| !out.contains(&c.name.as_str()))
            {
                out.push(c.name.as_str());
                interfaces.extend(c.interfaces.iter().map(String::as_str));
            }
        }
        out
    }

    /// The method a call of `owner.name desc` names, as the JVM resolves it: the first class up
    /// that declares it, then an interface's.
    pub fn method(&self, owner: &str, name: &str, desc: &str) -> Option<(&ClassFile, &Method)> {
        self.above(owner).into_iter().find_map(|c| {
            let c = &self.classes[c];
            c.methods
                .iter()
                .find(|m| m.name == name && m.desc == desc)
                .map(|m| (c, m))
        })
    }

    /// The class declaring field `name` as a reference through `owner` finds it.
    pub fn field_class(&self, owner: &str, name: &str) -> Option<&str> {
        self.above(owner)
            .into_iter()
            .find(|c| self.classes[*c].fields.iter().any(|f| f.name == name))
    }

    /// The classes strictly below `class` with code for the instance method.
    pub fn coded_below(&self, class: &str, name: &str, desc: &str) -> Vec<&str> {
        self.coded
            .get(&(name.to_owned(), desc.to_owned()))
            .into_iter()
            .flatten()
            .filter(|c| c.as_str() != class && self.above(c).contains(&class))
            .map(String::as_str)
            .collect()
    }

    /// Whether a class above `class` has code for the instance method too.
    fn coded_above(&self, class: &str, name: &str, desc: &str) -> bool {
        self.above(class).into_iter().any(|c| {
            c != class
                && self.classes[c]
                    .methods
                    .iter()
                    .any(|m| m.name == name && m.desc == desc && m.code.is_some())
        })
    }
}

/// What the runtime answers, as the translator's caller measures it.
pub struct Mapped<'a> {
    /// A method of the API, by owner, name and descriptor; static or not.
    pub method: &'a dyn Fn(&Member, bool) -> bool,
    /// An instance field of the API, by owner, name and type: a read of it finds something.
    pub field: &'a dyn Fn(&str, &str, &str) -> bool,
    /// A static field of the API (`owner.NAME`).
    pub constant: &'a dyn Fn(&str) -> bool,
    /// A class translated with the mods (theirs, a library's, the game's own code): what calls
    /// into it run is there.
    pub translated: &'a dyn Fn(&str) -> bool,
}

type Key = (String, String, String);

/// Finds what can stand, remembering each call and each method's code it looked at.
pub struct Deriver<'a> {
    pub game: &'a Game,
    mapped: Mapped<'a>,
    /// A call's blockers (none: it stands), by owner, name, descriptor and whether the object's
    /// class answers it.
    calls: HashMap<(Key, bool), BTreeSet<String>>,
    /// The blockers of a method's own code, by its class; no entry while it is looked at.
    bodies: HashMap<Key, BTreeSet<String>>,
    /// The fields of the API's objects that code which stands reads or writes through a mapped
    /// method that does only that (`Entity.position` by `position()`): by the field's class and
    /// name and whether it is a write, the method's key.
    pub accessors: BTreeMap<(String, String, bool), String>,
    /// The game's enums the program names: each is carried with its constants' names, in their
    /// order, whether a method of it stands or not ([`Deriver::classes`]), so the runtime can
    /// answer `values()`, `valueOf` and a `switch` over one it lists nowhere itself.
    pub enums: BTreeSet<String>,
    looking: HashSet<Key>,
    asked: HashMap<(Member, bool), bool>,
}

impl<'a> Deriver<'a> {
    pub fn new(game: &'a Game, mapped: Mapped<'a>) -> Deriver<'a> {
        Deriver {
            game,
            mapped,
            calls: HashMap::new(),
            bodies: HashMap::new(),
            accessors: BTreeMap::new(),
            enums: BTreeSet::new(),
            looking: HashSet::new(),
            asked: HashMap::new(),
        }
    }

    fn answers(&mut self, m: &Member, is_static: bool) -> bool {
        if let Some(&a) = self.asked.get(&(m.clone(), is_static)) {
            return a;
        }
        let a = (self.mapped.method)(m, is_static);
        self.asked.insert((m.clone(), is_static), a);
        a
    }

    fn reads(&mut self, owner: &str, name: &str, desc: &str) -> bool {
        let m = Member {
            owner: owner.to_owned(),
            name: format!("${name}"),
            desc: desc.to_owned(),
        };
        if let Some(&a) = self.asked.get(&(m.clone(), false)) {
            return a;
        }
        let a = (self.mapped.field)(owner, name, desc);
        self.asked.insert((m, false), a);
        a
    }

    /// The blockers of a call of `owner.name desc` (none: the game's code can stand for it): the
    /// members to map before it can. `dispatched`: the object's class answers it
    /// (`invokevirtual`, `invokeinterface`), so each class at or below the owner with code for it
    /// must stand; otherwise the one method the JVM resolves (a static one, a constructor, a
    /// private one, a `super` call).
    pub fn blockers(
        &mut self,
        owner: &str,
        name: &str,
        desc: &str,
        dispatched: bool,
    ) -> BTreeSet<String> {
        let key = (
            (owner.to_owned(), name.to_owned(), desc.to_owned()),
            dispatched,
        );
        if let Some(b) = self.calls.get(&key) {
            return b.clone();
        }
        let b = self.call_blockers(owner, name, desc, dispatched);
        self.calls.insert(key, b.clone());
        b
    }

    fn call_blockers(
        &mut self,
        owner: &str,
        name: &str,
        desc: &str,
        dispatched: bool,
    ) -> BTreeSet<String> {
        let game = self.game;
        let own = format!("{owner}.{name}.{desc}");
        // Why a call can't stand by itself, after its name: the jars don't have the method (a
        // library's class), nothing has code for it, too many classes do, or it is a `super`
        // call of a method the object's class may have its own code for.
        let why = |what: &str| BTreeSet::from([format!("{own} [{what}]")]);
        let named = game.method(owner, name, desc);
        let instance = named.is_none_or(|(_, m)| m.access & 0x000a == 0) && !name.starts_with('<');
        if !(dispatched && instance) {
            let Some((c, m)) = named else {
                return why("absent");
            };
            if m.code.is_none() {
                return why("no code");
            }
            // The runtime finds a method by the object's class: a `super` call must run this
            // one whatever that class has, which it can tell only when no other class has code.
            if instance
                && (!game.coded_below(&c.name, name, desc).is_empty()
                    || game.coded_above(&c.name, name, desc))
            {
                return why("super call");
            }
            return self.body(c, m);
        }
        let mut classes: Vec<&'a str> = game.coded_below(owner, name, desc);
        match named {
            Some((c, m)) if m.code.is_some() => classes.push(c.name.as_str()),
            Some(_) => {}
            None if classes.is_empty() => return why("absent"),
            None => {}
        }
        if classes.is_empty() {
            return why("no code");
        }
        if classes.len() > MOST_CLASSES {
            return why("many classes");
        }
        let mut out = BTreeSet::new();
        for class in classes {
            let c = &game.classes[class];
            if let Some(m) = c.methods.iter().find(|m| m.name == name && m.desc == desc) {
                out.extend(self.body(c, m));
            }
        }
        out
    }

    /// The blockers of one method's own code.
    fn body(&mut self, c: &'a ClassFile, m: &'a Method) -> BTreeSet<String> {
        // Of the libraries' own code only static methods stand: an object of a library's class is
        // the runtime's to answer for, however it keeps it.
        if is_library(&c.name) && m.access & 0x0008 == 0 {
            return BTreeSet::from([format!(
                "{}.{}.{} [a library's object]",
                c.name, m.name, m.desc
            )]);
        }
        let key = (c.name.clone(), m.name.clone(), m.desc.clone());
        if let Some(b) = self.bodies.get(&key) {
            return b.clone();
        }
        // A method that calls itself, or two each other: nothing more to learn on the way back.
        if !self.looking.insert(key.clone()) {
            return BTreeSet::new();
        }
        let b = self.scan(c, m);
        self.looking.remove(&key);
        self.bodies.insert(key, b.clone());
        b
    }

    fn scan(&mut self, c: &'a ClassFile, m: &'a Method) -> BTreeSet<String> {
        let game = self.game;
        let Some(code) = &m.code else {
            return BTreeSet::new();
        };
        let Ok(insns) = insn::decode(&code.bytes) else {
            return BTreeSet::from([format!("{}.{}.{} [unreadable]", c.name, m.name, m.desc)]);
        };
        // The Java runtime's own code is held to everything it asks: the game's may take the
        // libraries for answered (the runtime models them as far as the game uses them), but
        // the JDK's code reaches under itself, for what only a JVM has.
        let strict = is_library(&c.name);
        let mut out = BTreeSet::new();
        for i in insns {
            let idx = match i.arg {
                insn::Operand::Int(v) => v as u16,
                insn::Operand::IndexCount(v, _) => v,
                _ => continue,
            };
            match i.op {
                0xb6..=0xb9 => {
                    if let Some(r) = c.member(idx) {
                        out.extend(self.call(
                            &r,
                            i.op == 0xb8,
                            matches!(i.op, 0xb6 | 0xb9),
                            strict,
                        ));
                    }
                }
                0xb2..=0xb5 => {
                    let Some(r) = c.member(idx) else { continue };
                    if !(crate::is_api(&r.owner) || strict) || (self.mapped.translated)(&r.owner) {
                        continue;
                    }
                    let class = game
                        .field_class(&r.owner, &r.name)
                        .unwrap_or(&r.owner)
                        .to_owned();
                    if (self.mapped.translated)(&class) {
                        continue;
                    }
                    let mapped = match i.op {
                        0xb2 => (self.mapped.constant)(&format!("{class}.{}", r.name)),
                        // The runtime's answer for a field of its objects, as a mod's read asks.
                        0xb4 => {
                            self.reads(&r.owner, &r.name, &r.desc)
                                || (class != r.owner && self.reads(&class, &r.name, &r.desc))
                        }
                        // A write the runtime never hears of changes nothing in the game.
                        _ => false,
                    };
                    // A field the runtime has no answer for, which a mapped method only returns
                    // or only sets: the read or the write is that method's call.
                    if !mapped
                        && matches!(i.op, 0xb4 | 0xb5)
                        && let Some(key) = self.accessor(&class, &r.name, i.op == 0xb5)
                    {
                        // By the class the reference names too, as the translated code has it.
                        self.accessors
                            .insert((r.owner.clone(), r.name.clone(), i.op == 0xb5), key);
                        continue;
                    }
                    if !mapped {
                        let how = if matches!(i.op, 0xb3 | 0xb5) {
                            "write "
                        } else {
                            ""
                        };
                        out.insert(format!("{how}{class}.{}", r.name));
                    }
                }
                0xba => {
                    let Some((bsm, _, _)) = c.indy(idx) else {
                        continue;
                    };
                    let Some((h, args)) = c.bootstrap.get(usize::from(bsm)).cloned() else {
                        continue;
                    };
                    for (at, a) in std::iter::once(h).chain(args).enumerate() {
                        if let Some(Const::MethodHandle(kind, _)) = c.pool.get(usize::from(a))
                            && let Some((_, r)) = c.handle(a)
                        {
                            if at == 0 && BOOTSTRAPS.contains(&r.owner.as_str()) {
                                continue;
                            }
                            out.extend(self.call(&r, *kind == 6, matches!(*kind, 5 | 9), strict));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The mapped method that only returns (or, `write`, only sets) field `name` of `class`, as
    /// a key (`owner.name.desc`), if the game has one.
    pub fn accessor(&mut self, class: &str, name: &str, write: bool) -> Option<String> {
        let id = (class.to_owned(), name.to_owned(), write);
        if let Some(k) = self.accessors.get(&id) {
            return Some(k.clone());
        }
        let game = self.game;
        let list = if write { &game.setters } else { &game.getters };
        let found = list
            .get(&(class.to_owned(), name.to_owned()))?
            .iter()
            .find(|(c, n, d)| {
                self.answers(
                    &Member {
                        owner: c.clone(),
                        name: n.clone(),
                        desc: d.clone(),
                    },
                    false,
                )
            })?;
        let key = format!("{}.{}.{}", found.0, found.1, found.2);
        self.accessors.insert(id, key.clone());
        Some(key)
    }

    /// The blockers of a call of `r` from code that stands. `strict`: from the Java runtime's own
    /// code, where a call of anything (but an array's own `clone`) must be answered or stand;
    /// from the game's, a library's method is taken for answered.
    fn call(
        &mut self,
        r: &Member,
        is_static: bool,
        dispatched: bool,
        strict: bool,
    ) -> BTreeSet<String> {
        if !(crate::is_api(&r.owner) || (strict && !r.owner.starts_with('[')))
            || (self.mapped.translated)(&r.owner)
            || self.answers(r, is_static)
        {
            return BTreeSet::new();
        }
        self.blockers(&r.owner, &r.name, &r.desc, dispatched)
    }

    /// The methods whose code stands, by their own class, name and descriptor: those of the calls
    /// asked for, and what they call that the runtime doesn't answer.
    pub fn standing(&self) -> Vec<Member> {
        let mut out: Vec<Member> = self
            .bodies
            .iter()
            .filter(|(_, b)| b.is_empty())
            .map(|((c, n, d), _)| Member {
                owner: c.clone(),
                name: n.clone(),
                desc: d.clone(),
            })
            .collect();
        out.sort();
        out
    }

    /// The classes holding the methods that stand ([`horizon_compat::linked::DERIVED`] before the
    /// game class's name): each method in the class of its own, with that class's constants.
    /// Above it and at each call's owner the same classes of the game's, so that a call naming
    /// a class below finds the method as the JVM resolves it; and where a class below one of
    /// them has code for a method too, the mark that says so ([`MARK`]).
    pub fn classes(&self) -> Vec<ClassFile> {
        let game = self.game;
        let named = |c: &str| format!("{}{c}", horizon_compat::linked::DERIVED);
        let mut methods: BTreeMap<&str, Vec<&Method>> = BTreeMap::new();
        let mut wanted: BTreeSet<&str> = BTreeSet::new();
        let mut dispatched: BTreeSet<(&str, &str)> = BTreeSet::new();
        for m in self.standing() {
            let Some(c) = game.class(&m.owner) else {
                continue;
            };
            let Some(method) = c
                .methods
                .iter()
                .find(|x| x.name == m.name && x.desc == m.desc)
            else {
                continue;
            };
            methods.entry(c.name.as_str()).or_default().push(method);
            wanted.extend(game.above(&c.name));
            if method.access & 0x000a == 0 && !method.name.starts_with('<') {
                dispatched.insert((method.name.as_str(), method.desc.as_str()));
            }
        }
        for e in &self.enums {
            if let Some(c) = game.class(e).filter(|c| c.access & 0x4000 != 0) {
                wanted.insert(c.name.as_str());
            }
        }
        for ((owner, _, _), _) in self
            .calls
            .iter()
            .filter(|(_, b)| b.is_empty())
            .map(|(k, b)| (&k.0, b))
        {
            if let Some(o) = game.class(owner) {
                wanted.extend(game.above(&o.name));
            }
        }
        wanted
            .into_iter()
            .filter_map(|name| game.class(name))
            .map(|c| ClassFile {
                pool: c.pool.clone(),
                access: c.access,
                name: named(&c.name),
                superclass: c
                    .superclass
                    .as_deref()
                    .filter(|s| game.class(s).is_some())
                    .map(named)
                    .or_else(|| Some("java/lang/Object".to_owned())),
                interfaces: c
                    .interfaces
                    .iter()
                    .filter(|i| game.class(i).is_some())
                    .map(|i| named(i))
                    .collect(),
                fields: dispatched
                    .iter()
                    .filter(|(n, d)| !game.coded_below(&c.name, n, d).is_empty())
                    .map(|(n, d)| Field {
                        access: 0,
                        name: format!("{MARK}{n}"),
                        desc: (*d).to_owned(),
                        annotations: Vec::new(),
                        signature: None,
                    })
                    // An enum's constants, in their order (the fields the compiler marks so).
                    .chain(
                        c.fields
                            .iter()
                            .filter(|f| c.access & 0x4000 != 0 && f.access & 0x4000 != 0)
                            .map(|f| Field {
                                access: f.access,
                                name: f.name.clone(),
                                desc: f.desc.clone(),
                                annotations: Vec::new(),
                                signature: None,
                            }),
                    )
                    .collect(),
                methods: methods
                    .get(c.name.as_str())
                    .into_iter()
                    .flatten()
                    .map(|m| (*m).clone())
                    .collect(),
                bootstrap: c.bootstrap.clone(),
                annotations: Vec::new(),
                signature: None,
            })
            .collect()
    }
}
