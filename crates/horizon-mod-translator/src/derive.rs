//! Which of the game's own methods can stand for ones the runtime doesn't map: a method of a
//! class the runtime models (`Level`, `Entity`, `ItemStack`) whose code, in the player's own
//! copy of the game, asks only what the runtime answers, or what can stand the same way in its
//! turn. `Entity.distanceToSqr(Entity)` is three reads of each position and a sum: with the
//! positions mapped, the game's own code is the answer, and nothing has to be written for it.
//!
//! A method can't stand when its code reads or writes a field the runtime keeps no state for,
//! calls something neither mapped nor able to stand, or when a class below declares it again
//! (the object's own class picks the code, and the runtime's objects don't always say which of
//! the game's classes they are): each such member is a blocker, the thing to map to let it stand.
use crate::{
    class::{ClassFile, Const, Member, Method},
    insn,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io::Read,
    path::Path,
};

/// The classes of the game's own jars, each from the first jar that has it (the NeoForge
/// install's patched classes before the game's unpatched ones).
pub struct Game {
    classes: HashMap<String, ClassFile>,
    /// The classes declaring an instance method of each name and descriptor.
    declared: HashMap<(String, String), Vec<String>>,
}

impl Game {
    pub fn open(jars: &[&Path]) -> Result<Game, String> {
        let mut classes = HashMap::new();
        for jar in jars {
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
                if classes.contains_key(class) || !crate::is_api(class) {
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
        let mut declared: HashMap<(String, String), Vec<String>> = HashMap::new();
        for c in &classes {
            for m in c
                .methods
                .iter()
                .filter(|m| m.access & 0x000a == 0 && !m.name.starts_with('<'))
            {
                declared
                    .entry((m.name.clone(), m.desc.clone()))
                    .or_default()
                    .push(c.name.clone());
            }
        }
        Game {
            classes: classes.into_iter().map(|c| (c.name.clone(), c)).collect(),
            declared,
        }
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

    /// The method a call of `owner.name desc` runs when nothing below `owner` declares it: the
    /// first class up that declares it, then a default method of an interface.
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

    /// Whether another class below `class` or above it has code for the instance method too:
    /// which runs is then the object's class to say, and a `super` call's to pass on.
    pub fn declared_again(&self, class: &str, name: &str, desc: &str) -> bool {
        let has_code = |c: &str| {
            self.classes[c]
                .methods
                .iter()
                .any(|m| m.name == name && m.desc == desc && m.code.is_some())
        };
        let above = self.above(class);
        self.declared
            .get(&(name.to_owned(), desc.to_owned()))
            .is_some_and(|d| {
                d.iter().any(|c| {
                    c != class
                        && has_code(c)
                        && (above.contains(&c.as_str()) || self.above(c).contains(&class))
                })
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

/// Finds what can stand, remembering each method it looked at.
pub struct Deriver<'a> {
    pub game: &'a Game,
    mapped: Mapped<'a>,
    /// A method's blockers, none when it can stand; no entry yet while it is being looked at.
    known: HashMap<Key, BTreeSet<String>>,
    looking: HashSet<Key>,
    asked: HashMap<(Member, bool), bool>,
}

impl<'a> Deriver<'a> {
    pub fn new(game: &'a Game, mapped: Mapped<'a>) -> Deriver<'a> {
        Deriver {
            game,
            mapped,
            known: HashMap::new(),
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

    /// The blockers of `owner.name desc` (none: the game's code can stand for it): the members
    /// to map before it can. `dispatched`: called on an object whose class picks the code
    /// (`invokevirtual`, `invokeinterface`), so a method declared again below can't stand.
    pub fn blockers(
        &mut self,
        owner: &str,
        name: &str,
        desc: &str,
        dispatched: bool,
    ) -> BTreeSet<String> {
        let key = (owner.to_owned(), name.to_owned(), desc.to_owned());
        if let Some(b) = self.known.get(&key) {
            return b.clone();
        }
        // A method that calls itself, or two each other: nothing more to learn on the way back.
        if !self.looking.insert(key.clone()) {
            return BTreeSet::new();
        }
        let b = self.look(owner, name, desc, dispatched);
        self.looking.remove(&key);
        self.known.insert(key, b.clone());
        b
    }

    fn look(&mut self, owner: &str, name: &str, desc: &str, dispatched: bool) -> BTreeSet<String> {
        let game = self.game;
        let own = format!("{owner}.{name}.{desc}");
        let Some((c, m)) = game.method(owner, name, desc) else {
            return BTreeSet::from([own]);
        };
        let Some(code) = &m.code else {
            return BTreeSet::from([own]);
        };
        let _ = dispatched;
        if m.access & 0x000a == 0
            && !name.starts_with('<')
            && game.declared_again(&c.name, name, desc)
        {
            return BTreeSet::from([own]);
        }
        let Ok(insns) = insn::decode(&code.bytes) else {
            return BTreeSet::from([own]);
        };
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
                        out.extend(self.call(&r, i.op == 0xb8, matches!(i.op, 0xb6 | 0xb9)));
                    }
                }
                0xb2..=0xb5 => {
                    let Some(r) = c.member(idx) else { continue };
                    if !crate::is_api(&r.owner) || (self.mapped.translated)(&r.owner) {
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
                    for a in std::iter::once(h).chain(args) {
                        if let Some(Const::MethodHandle(kind, _)) = c.pool.get(usize::from(a))
                            && let Some((_, r)) = c.handle(a)
                        {
                            out.extend(self.call(&r, *kind == 6, matches!(*kind, 5 | 9)));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The methods looked at that stand, by the owner a call names, name and descriptor: the
    /// ones asked for, and what they call that the runtime doesn't answer.
    pub fn standing(&self) -> Vec<Member> {
        let mut out: Vec<Member> = self
            .known
            .iter()
            .filter(|(_, b)| b.is_empty())
            .map(|((o, n, d), _)| Member {
                owner: o.clone(),
                name: n.clone(),
                desc: d.clone(),
            })
            .collect();
        out.sort();
        out
    }

    /// The classes holding the methods that stand ([`horizon_compat::linked::DERIVED`] before the
    /// game class's name): each method in the class that declares it, with its class's constants;
    /// above it the same classes of its supertypes, so that a call naming a class below the
    /// declaring one finds the method as the JVM resolves it.
    pub fn classes(&self) -> Vec<ClassFile> {
        let game = self.game;
        let named = |c: &str| format!("{}{c}", horizon_compat::linked::DERIVED);
        let mut methods: std::collections::BTreeMap<&str, Vec<&Method>> = Default::default();
        let mut wanted: BTreeSet<&str> = BTreeSet::new();
        for m in self.standing() {
            let Some((c, method)) = game.method(&m.owner, &m.name, &m.desc) else {
                continue;
            };
            let list = methods.entry(c.name.as_str()).or_default();
            if !list
                .iter()
                .any(|x| x.name == method.name && x.desc == method.desc)
            {
                list.push(method);
            }
            if let Some(o) = game.class(&m.owner) {
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
                fields: Vec::new(),
                methods: methods
                    .get(c.name.as_str())
                    .into_iter()
                    .flatten()
                    .map(|m| (*m).clone())
                    .collect(),
                bootstrap: c.bootstrap.clone(),
                annotations: Vec::new(),
            })
            .collect()
    }

    /// The blockers of a call of `r` from code that stands.
    fn call(&mut self, r: &Member, is_static: bool, dispatched: bool) -> BTreeSet<String> {
        if !crate::is_api(&r.owner)
            || (self.mapped.translated)(&r.owner)
            || self.answers(r, is_static)
        {
            return BTreeSet::new();
        }
        self.blockers(&r.owner, &r.name, &r.desc, dispatched)
    }
}
