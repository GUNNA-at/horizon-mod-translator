//! Bytecode → Rust. Each Java method becomes `fn(rt: &mut Rt, args: &[V]) -> V`
//! whose body is a `loop { match pc { ... } }` over its instructions, with
//! the operand stack and locals as `V` values. Calls to the mod's own
//! classes are direct Rust calls (virtual ones through a generated
//! dispatch table); calls to Minecraft, NeoForge and the JDK go to
//! `horizon_compat::api`. Everything is native code: no JVM at runtime.
use crate::{
    class::{ClassFile, Const, Member},
    insn::{self, Insn, Operand},
    weave::{self, At, Kind, Weave},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

/// The pseudo-instruction offset of a method's unwind block.
const THROW: usize = usize::MAX - 1;

/// FNV-1a, for short stable suffixes on generated names.
fn hash(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// A Rust identifier for a Java method.
pub fn fn_name(class: &str, name: &str, desc: &str) -> String {
    let simple = class.rsplit('/').next().unwrap_or(class);
    let clean: String = format!("{simple}_{name}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    format!("j_{clean}_{:08x}", hash(&format!("{class}.{name}{desc}")))
}

fn ident(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

/// The dispatcher of instance method `nd` (`name.desc`): `fn(rt, cid, owner_key, args) -> V`.
/// The member a mixin accessor or invoker names when its annotation has no value: the method's name
/// without its mod prefix (`create$`) and verb (`get`, `call`...), its first letter lowered.
fn accessor_name(method: &str, verbs: &[&str]) -> String {
    let bare = method.rsplit('$').next().unwrap_or(method);
    let rest = verbs.iter().find_map(|v| bare.strip_prefix(v).filter(|r| r.starts_with(|c: char| c.is_ascii_uppercase()))).unwrap_or(bare);
    let mut c = rest.chars();
    match c.next() {
        // All capitals is a constant's name, kept as it is (`getOPERATING_SYSTEM`).
        Some(f) if rest.chars().any(|c| c.is_ascii_lowercase()) => f.to_ascii_lowercase().to_string() + c.as_str(),
        _ => rest.to_owned(),
    }
}

fn dispatcher(nd: &str) -> String {
    format!("d_{}_{:08x}", ident(nd.split('.').next().unwrap_or("")), hash(nd))
}

/// Whether a class id implements `nd`: `fn(cid) -> bool`.
fn overrider(nd: &str) -> String {
    format!("o_{}_{:08x}", ident(nd.split('.').next().unwrap_or("")), hash(nd))
}

/// A match pattern for a set of ids: runs of consecutive ids as ranges.
fn ids_pattern(ids: &[u32]) -> String {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[j] + 1 {
            j += 1;
        }
        parts.push(if j > i { format!("{}..={}", ids[i], ids[j]) } else { ids[i].to_string() });
        i = j + 1;
    }
    if parts.is_empty() { "0xFFFF_FFFF".to_owned() } else { parts.join(" | ") }
}

/// Argument kinds from a method descriptor: 'I' (int-like), 'J', 'F', 'D', 'L' (reference).
pub fn arg_kinds(desc: &str) -> Vec<char> {
    let inner = &desc[1..desc.find(')').unwrap_or(1)];
    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'L' => {
                for d in chars.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
                out.push('L');
            }
            '[' => {
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.next() == Some('L') {
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                out.push('L');
            }
            'J' | 'F' | 'D' => out.push(c),
            _ => out.push('I'),
        }
    }
    out
}

/// Argument kinds keeping booleans and chars apart ('Z', 'C'), for string conversion.
pub fn raw_kinds(desc: &str) -> String {
    let inner = &desc[1..desc.find(')').unwrap_or(1)];
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'L' => {
                for d in chars.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
                out.push('L');
            }
            '[' => {
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.next() == Some('L') {
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                out.push('L');
            }
            'B' | 'S' => out.push('I'),
            c => out.push(c),
        }
    }
    out
}

fn returns_value(desc: &str) -> bool {
    !desc.ends_with(")V")
}

const MOD: &str = "Lnet/neoforged/fml/common/Mod;";
const SUBSCRIBE_EVENT: &str = "Lnet/neoforged/bus/api/SubscribeEvent;";
/// `@EventBusSubscriber` (NeoForge 21) and its older home on `@Mod`.
const EVENT_BUS_SUBSCRIBER: [&str; 2] = ["Lnet/neoforged/fml/common/EventBusSubscriber;", "Lnet/neoforged/fml/common/Mod$EventBusSubscriber;"];

/// The translation of a set of classes (one jar).
pub struct Program<'a> {
    classes: BTreeMap<&'a str, &'a ClassFile>,
    /// Static strings, emitted once.
    keys: BTreeSet<String>,
    pub unsupported: BTreeMap<String, usize>,
    /// Lambda implementations seen: key → (handle kind, member).
    lambdas: BTreeMap<String, (u8, Member)>,
    /// What wrapping handlers' `Operation`s run (`@WrapOperation`, `@WrapMethod`): functions the
    /// codegen writes beside the method, by lambda key, with their names.
    operations: BTreeMap<String, String>,
    /// Those functions' code, written out with the method's part.
    synthetic: Vec<String>,
    /// `META-INF/services`: interface → implementations (internal names), for `ServiceLoader`.
    pub services: BTreeMap<String, Vec<String>>,
    /// Each mod class's id (1.., in name order): objects carry it, and virtual calls jump on it.
    class_ids: BTreeMap<String, u32>,
    /// Instance methods (`name.desc`) some mod class implements: each gets a dispatcher.
    vkeys: BTreeSet<String>,
    /// Each lambda implementation's id in `lambda_call` (as first seen).
    lambda_ids: BTreeMap<String, u32>,
    /// The mod's id: picks its `@Mod` class among several (libraries bundled in its jar have theirs).
    pub mod_id: Option<String>,
    /// The mods built on it translated with it (its addons), in loading order: FML constructs a
    /// mod after the mods it needs, so theirs come after its own.
    pub addons: Vec<String>,
    /// Each mod's id, version and display name from its jar ([`crate::mod_metadata`]): what its
    /// `IModInfo` answers.
    pub metadata: Vec<(String, String, String)>,
    /// Methods (`name.desc`) that ask who called them (`StackWalker.getCallerClass`): calls to
    /// them record the calling class first. Everything else keeps no call stack.
    caller_sensitive: BTreeSet<String>,
    /// Mixins whose targets are among these classes, woven into them ([`crate::weave`]).
    weave: weave::Plan,
    /// Each such mixin's target (its first, for a mixin of several).
    mixin_of: BTreeMap<String, String>,
    /// The method being written, for weaving.
    cur: Option<Cur>,
}

/// The method [`Program::method`] is writing, as weaving needs it.
#[derive(Default)]
struct Cur {
    class: String,
    fn_name: String,
    name: String,
    desc: String,
    is_static: bool,
    /// The mixins' handlers woven into it.
    weaves: Vec<Weave>,
    /// Weaves (indices into `weaves`) at each call instruction, by its offset.
    at_calls: BTreeMap<usize, Vec<usize>>,
    /// The offset of its last return instruction (`@At("TAIL")`).
    tail: usize,
    /// Whether a constructor's call of its superclass's has been written.
    super_done: bool,
}

/// A call with the weaves at it ([`Program::call_weaves`]).
#[derive(Default)]
struct Woven {
    before: String,
    call: String,
    modify: String,
    after: String,
    /// Whether `before` changes the call's arguments.
    mutable: bool,
}

/// An `Operation`'s arguments (an `Object[]`, `arr`) as the code it runs takes them: each of kind
/// `kinds[i]`, a primitive out of its box.
fn unboxed_items(kinds: &[char]) -> String {
    kinds
        .iter()
        .enumerate()
        .map(|(i, k)| match k {
            'L' => format!("rt.array_get(arr, {i})"),
            _ => format!("{{ let v = rt.array_get(arr, {i}); match v {{ V::R(r) if r != 0 => rt.get_field(v, \"value\"), v => v }} }}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// What an `Operation` hands back for a value `r` of kind `k`: a primitive boxed, nothing for void.
fn boxed(k: char) -> String {
    let class = match k {
        'V' => return "V::NULL".into(),
        'Z' => "java/lang/Boolean",
        'B' => "java/lang/Byte",
        'C' => "java/lang/Character",
        'S' => "java/lang/Short",
        'I' => "java/lang/Integer",
        'J' => "java/lang/Long",
        'F' => "java/lang/Float",
        'D' => "java/lang/Double",
        _ => return "r".into(),
    };
    format!("{{ let b = rt.new_object({class:?}); rt.put_field(b, \"value\", r); b }}")
}

/// The member a call or field instruction names.
fn insn_member(c: &ClassFile, i: &Insn) -> Option<Member> {
    let idx = match &i.arg {
        Operand::Int(v) => *v,
        Operand::IndexCount(v, _) => *v as i32,
        _ => return None,
    };
    c.member(idx as u16)
}

/// Whether a mixin's method only stands for its target's (`@Shadow`).
fn is_shadow(m: &crate::class::Method) -> bool {
    m.annotations.iter().any(|a| a.ty == "Lorg/spongepowered/asm/mixin/Shadow;")
}

/// The kind letter of what a descriptor returns (`V`, `I`..., `L` for any reference).
fn return_kind(desc: &str) -> char {
    match desc.rsplit(')').next().and_then(|r| r.chars().next()) {
        Some('[') | Some('L') | None => 'L',
        Some(k) => k,
    }
}

impl<'a> Program<'a> {
    pub fn new(classes: &'a [ClassFile]) -> Program<'a> {
        let weave = weave::plan(classes);
        let mut mixin_of = BTreeMap::new();
        for (target, mixins) in &weave.mixins {
            for m in mixins {
                mixin_of.entry(m.clone()).or_insert_with(|| target.clone());
            }
        }
        Program {
            classes: classes.iter().map(|c| (c.name.as_str(), c)).collect(),
            keys: BTreeSet::new(),
            unsupported: weave.unsupported.clone(),
            lambdas: BTreeMap::new(),
            operations: BTreeMap::new(),
            synthetic: Vec::new(),
            services: BTreeMap::new(),
            class_ids: {
                let mut names: Vec<&str> = classes.iter().map(|c| c.name.as_str()).collect();
                names.sort_unstable();
                names.dedup();
                names.iter().enumerate().map(|(i, n)| (n.to_string(), i as u32 + 1)).collect()
            },
            vkeys: classes
                .iter()
                .flat_map(|c| c.methods.iter())
                .filter(|m| m.code.is_some() && m.access & 0x0008 == 0 && !m.name.starts_with('<'))
                .map(|m| format!("{}.{}", m.name, m.desc))
                .collect(),
            lambda_ids: BTreeMap::new(),
            mod_id: None,
            addons: Vec::new(),
            metadata: Vec::new(),
            caller_sensitive: classes
                .iter()
                .flat_map(|c| c.methods.iter().map(move |m| (c, m)))
                .filter(|(c, m)| {
                    m.code.is_some()
                        && c.called_by(m).iter().any(|k| {
                            (k.owner == "java/lang/StackWalker" && k.name == "getCallerClass")
                                || (k.owner.ends_with("/Reflection") && k.name == "getCallerClass")
                        })
                })
                .map(|(_, m)| format!("{}.{}", m.name, m.desc))
                .collect(),
            weave,
            mixin_of,
            cur: None,
        }
    }

    /// The mixins woven into `class`.
    fn mixins_of(&self, class: &str) -> &[String] {
        self.weave.mixins.get(class).map_or(&[], Vec::as_slice)
    }

    /// The mixin whose method `name desc` joins `target` (one it adds, or an `@Overwrite`): a method
    /// with code that doesn't only stand for the target's own (`@Shadow`).
    fn mixin_method(&self, target: &str, name: &str, desc: &str) -> Option<String> {
        if name.starts_with('<') {
            return None;
        }
        self.mixins_of(target)
            .iter()
            .find(|m| {
                self.classes
                    .get(m.as_str())
                    .is_some_and(|k| k.methods.iter().any(|x| x.name == name && x.desc == desc && x.code.is_some() && !is_shadow(x)))
            })
            .cloned()
    }

    /// A class's interfaces, with those its mixins add.
    fn interfaces_of(&self, k: &ClassFile) -> Vec<String> {
        let mut out = k.interfaces.clone();
        for m in self.mixins_of(&k.name) {
            for i in self.classes.get(m.as_str()).map(|mk| mk.interfaces.as_slice()).unwrap_or_default() {
                if !out.contains(i) {
                    out.push(i.clone());
                }
            }
        }
        out
    }

    /// A call through a mixin class reaches its target, as Mixin merges the class into it: its
    /// `@Shadow`s and what the target inherits. The mixin's own methods keep their owner.
    fn through_mixin(&self, m: Member) -> Member {
        let Some(target) = self.mixin_of.get(&m.owner) else { return m };
        let own = self
            .classes
            .get(m.owner.as_str())
            .is_some_and(|k| k.methods.iter().any(|x| x.name == m.name && x.desc == m.desc && x.code.is_some() && !is_shadow(x)));
        if own { m } else { Member { owner: target.clone(), name: m.name, desc: m.desc } }
    }

    /// The `@Mod` classes FML constructs, each with the constructor it would pick (the one taking
    /// the mod bus), in FML's order (a mod after the ones it needs): mods bundled in the jar first,
    /// then the jar's own mod (`mod_id`), then its addons translated with it.
    fn mod_mains(&self) -> Vec<(&'a ClassFile, &'a crate::class::Method)> {
        let mut found: Vec<(&'a ClassFile, &'a crate::class::Method)> = Vec::new();
        for c in self.classes.values() {
            if c.annotation_of(MOD).is_none() {
                continue;
            }
            let ctor = c
                .methods
                .iter()
                .filter(|m| m.name == "<init>" && m.code.is_some())
                .max_by_key(|m| (m.desc.contains("IEventBus"), m.desc.len()));
            if let Some(m) = ctor {
                found.push((c, m));
            }
        }
        let rank = |c: &ClassFile| {
            let id = c.annotation_of(MOD).and_then(|a| a.value("value"));
            if id.is_some() && id == self.mod_id.as_deref() {
                1
            } else {
                self.addons.iter().position(|a| Some(a.as_str()) == id).map_or(0, |i| i + 2)
            }
        };
        found.sort_by_key(|(c, _)| (rank(c), c.name.clone()));
        found
    }

    /// `@SubscribeEvent` methods: (key, handle kind as a lambda's, the method), so listeners made
    /// from them dispatch through `lambda_call`.
    fn subscriber_methods(&self) -> Vec<(String, u8, Member)> {
        let mut out = Vec::new();
        for c in self.classes.values() {
            for m in c.methods.iter().filter(|m| m.code.is_some() && m.annotations.iter().any(|a| a.ty == SUBSCRIBE_EVENT)) {
                let kind = if m.access & 0x0008 != 0 { 6 } else { 5 };
                let member = Member { owner: c.name.clone(), name: m.name.clone(), desc: m.desc.clone() };
                out.push((format!("{}.{}.{}", c.name, m.name, m.desc), kind, member));
            }
        }
        out
    }

    fn own(&self, class: &str) -> bool {
        self.classes.contains_key(class)
    }

    /// A class's id; 0 for the API's.
    fn cid(&self, class: &str) -> u32 {
        self.class_ids.get(class).copied().unwrap_or(0)
    }

    fn lambda_id(&mut self, key: &str) -> u32 {
        let next = self.lambda_ids.len() as u32;
        *self.lambda_ids.entry(key.to_owned()).or_insert(next)
    }

    /// The mod class that implements `name desc` for an object of `class`
    /// (walking up the mod's own superclasses).
    /// The class whose code runs for `name.desc` on an object of `class`, as the JVM resolves it:
    /// the class and its superclasses first, then default methods of its superinterfaces.
    /// Whether `class` declares `name desc` as a private method with code: called directly however
    /// it is invoked (Java 11 reaches private methods with `invokevirtual`/`invokeinterface`, and a
    /// lambda's body, `lambda$...`, is one), since it is never inherited or overridden.
    fn private_method(&self, class: &str, name: &str, desc: &str) -> bool {
        self.classes
            .get(class)
            .is_some_and(|k| k.methods.iter().any(|m| m.name == name && m.desc == desc && m.code.is_some() && m.access & 0x0002 != 0 && m.access & 0x0008 == 0))
    }

    fn resolve(&self, class: &str, name: &str, desc: &str) -> Option<String> {
        let start = self.classes.get(class)?;
        let mut chain = Vec::new();
        let mut c = Some(*start);
        while let Some(k) = c {
            // What a mixin adds to the class, or overwrites in it, is the class's own.
            if let Some(m) = self.mixin_method(&k.name, name, desc) {
                return Some(m);
            }
            if k.methods.iter().any(|m| m.name == name && m.desc == desc && m.code.is_some()) {
                return Some(k.name.clone());
            }
            chain.push(k);
            c = k.superclass.as_deref().and_then(|s| self.classes.get(s).copied());
        }
        if name.starts_with('<') {
            return None;
        }
        // Superinterfaces, breadth first: an inherited default method (never a static or private one).
        let mut queue: std::collections::VecDeque<String> = chain.iter().flat_map(|k| self.interfaces_of(k)).collect();
        let mut seen = BTreeSet::new();
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i.clone()) {
                continue;
            }
            let Some(k) = self.classes.get(i.as_str()) else { continue };
            if k.methods.iter().any(|m| m.name == name && m.desc == desc && m.code.is_some() && m.access & 0x000a == 0) {
                return Some(k.name.clone());
            }
            queue.extend(self.interfaces_of(k));
        }
        None
    }

    /// The API type a call on the mod's `owner` reaches when no mod class implements the method:
    /// a class's first superclass outside the mod, or a mod interface's only API superinterface.
    /// None when `owner` is not the mod's (the key is already the API's) or it is ambiguous.
    fn api_owner(&self, owner: &str) -> Option<String> {
        if let Some(target) = self.mixin_of.get(owner) {
            return self.api_owner(target);
        }
        let start = self.classes.get(owner)?;
        if start.access & 0x0200 == 0 {
            let mut k = *start;
            loop {
                let sup = k.superclass.as_deref()?;
                match self.classes.get(sup) {
                    Some(next) => k = next,
                    None => return Some(sup.to_owned()),
                }
            }
        }
        let mut api = BTreeSet::new();
        let mut queue: std::collections::VecDeque<&str> = start.interfaces.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            match self.classes.get(i) {
                Some(k) => queue.extend(k.interfaces.iter().map(String::as_str)),
                None => {
                    api.insert(i);
                }
            }
        }
        let mut it = api.into_iter();
        match (it.next(), it.next()) {
            (Some(only), None) => Some(only.to_owned()),
            _ => None,
        }
    }

    fn unsupported(&mut self, what: &str) {
        *self.unsupported.entry(what.to_owned()).or_insert(0) += 1;
    }

    /// The whole module: every method, then the virtual dispatch table.
    /// The module split into files of about `per_file` classes each, for
    /// big mods: `lib.rs` holds the tables and re-exports every part.
    pub fn emit_files(&mut self, per_file: usize) -> Vec<(String, String)> {
        let header = "// Generated by horizon-mod-translator. Do not edit.\n\
             #![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, clippy::all)]\n";
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let mut files = Vec::new();
        let mut lib = format!("{header}use horizon_compat::{{api, Rt, V}};\n");
        for (n, chunk) in classes.chunks(per_file.max(1)).enumerate() {
            let mut part = String::from("#![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, unused_imports, clippy::all)]\nuse crate::*;\nuse horizon_compat::{api, Rt, V};\n\n");
            for c in chunk {
                for m in &c.methods {
                    if let Some(code) = &m.code {
                        part.push_str(&self.method(c, &m.name, &m.desc, m.access & 0x0008 != 0, code));
                        for f in std::mem::take(&mut self.synthetic) {
                            part.push_str(&f);
                        }
                    }
                }
            }
            let _ = writeln!(lib, "mod part{n};\npub use part{n}::*;");
            files.push((format!("part{n}.rs"), part));
        }
        lib.push_str(&self.tables(&classes));
        files.push(("lib.rs".into(), lib));
        files
    }

    fn tables(&mut self, classes: &[&ClassFile]) -> String {
        // Listeners made from `@SubscribeEvent` methods dispatch through `lambda_call` too.
        for (key, _, _) in self.subscriber_methods() {
            self.lambda_id(&key);
        }
        let mut out = String::from(
            "/// Registers the mod's class hierarchy and callbacks with the runtime. Call once, before anything else.\n\
             pub fn init(rt: &mut Rt) {\n    rt.vcall = Some(vcall);\n    rt.lambda_call = Some(lambda_call);\n    rt.overrides = Some(overrides);\n    rt.reflect = Some(reflect);\n\
             \x20   rt.class_ids.extend(CLASS_IDS.iter().copied());\n\
             \x20   rt.vkeys.extend(VKEYS.iter().map(|&(k, d, o)| (k, (d, o))));\n",
        );
        for c in classes {
            if let Some(sup) = &c.superclass {
                let _ = writeln!(out, "    rt.supers.insert({:?}, {sup:?});", c.name);
            }
            let interfaces = self.interfaces_of(c);
            if !interfaces.is_empty() {
                let _ = writeln!(out, "    rt.interfaces.insert({:?}, &{:?});", c.name, interfaces);
            }
        }
        // `@SubscribeEvent` methods by class: what `bus.register(...)` and `@EventBusSubscriber` listen with.
        let mut subs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (key, kind, m) in self.subscriber_methods() {
            let Some(event) = crate::class::param_classes(&m.desc).last().cloned() else { continue };
            let lid = self.lambda_ids[&key];
            subs.entry(m.owner.clone()).or_default().push(format!("({event:?}, {key:?}, {}, {lid})", kind == 6));
        }
        for (class, list) in subs {
            let _ = writeln!(out, "    rt.subscribers.insert({class:?}, vec![{}]);", list.join(", "));
        }
        out.push_str("}\n\n");
        // Mod loading's registration phase: classes holding registers initialise, then suppliers resolve.
        out.push_str("/// Runs the mod's registrations, as NeoForge's loading does. Call after `init`.\npub fn start(rt: &mut Rt) {\n");
        for c in classes {
            if c.fields.iter().any(|f| f.access & 0x0008 != 0 && f.desc.contains("neoforged/neoforge/registries/DeferredRegister")) {
                let _ = writeln!(out, "    {}", self.ensure(&c.name).replace("if rt.thrown.is_some() { return V::Void; }", &format!("rt.report_thrown({:?});", c.name)));
            }
        }
        // FML constructs each `@Mod` class with the mod bus and registers its `@EventBusSubscriber`
        // classes, then fires the registration events.
        let mains = self.mod_mains();
        let mut ids: Vec<String> = Vec::new();
        for id in mains.iter().filter_map(|(c, _)| c.annotation_of(MOD).and_then(|a| a.value("value"))) {
            if !ids.iter().any(|i| i == id) {
                ids.push(id.to_owned());
            }
        }
        if !ids.is_empty() {
            let _ = writeln!(out, "    rt.mods = vec![{}];", ids.iter().map(|i| format!("{i:?}.into()")).collect::<Vec<_>>().join(", "));
        }
        for (id, version, name) in &self.metadata {
            for (what, value) in [("version", version), ("name", name)].into_iter().filter(|(_, v)| !v.is_empty()) {
                let _ = writeln!(out, "    {{ let v = rt.string({value:?}); rt.interned.insert({:?}.into(), v); }}", format!("compat:mod-{what}/{id}"));
            }
        }
        for (main, ctor) in &mains {
            let params = crate::class::param_classes(&ctor.desc);
            let _ = writeln!(out, "    {{\n        let this = rt.new_object_id({:?}, {});", main.name, self.cid(&main.name));
            // FML makes the mod's container the active one while its constructor runs.
            if let Some(id) = main.annotation_of(MOD).and_then(|a| a.value("value")) {
                let _ = writeln!(out, "        {{ let id = rt.string({id:?}); rt.interned.insert({:?}.into(), id); }}", "compat:active-mod");
            }
            let _ = writeln!(out, "        let mut args = vec![this];");
            for p in &params {
                let _ = writeln!(out, "        let v = horizon_compat::neo::constructor_arg(rt, {p:?}); args.push(v);");
            }
            let _ = writeln!(out, "        {}", self.ensure(&main.name).replace("if rt.thrown.is_some() { return V::Void; }", &format!("rt.report_thrown({:?});", main.name)));
            let _ = writeln!(out, "        {}(rt, &args);\n        rt.report_thrown({:?});\n    }}", fn_name(&main.name, "<init>", &ctor.desc), format!("the constructor of {}", main.name));
        }
        for c in classes {
            if EVENT_BUS_SUBSCRIBER.iter().any(|t| c.annotation_of(t).is_some()) {
                let _ = writeln!(out, "    rt.subscribe_class({:?});", c.name);
            }
        }
        if !mains.is_empty() {
            out.push_str("    horizon_compat::neo::fire_registrations(rt);\n");
        }
        out.push_str("    rt.resolve_registrations();\n}\n\n");
        // Class ids by name, and each dispatched method's dispatcher, both sorted (binary search).
        out.push_str("static CLASS_IDS: &[(&str, u32)] = &[\n");
        for (name, id) in &self.class_ids {
            let _ = writeln!(out, "    ({name:?}, {id}),");
        }
        out.push_str("];\n\nstatic VKEYS: &[(&str, horizon_compat::Dispatch, fn(u32) -> bool)] = &[\n");
        for nd in &self.vkeys {
            let _ = writeln!(out, "    ({nd:?}, {}, {}),", dispatcher(nd), overrider(nd));
        }
        out.push_str("];\n\n");
        out.push_str(&self.dispatch());
        out.push_str(&self.lambda_table());
        out.push_str(&self.reflect_table());
        out
    }

    /// `reflect(rt, class, what, args)` (see `Rt::reflect`): what `Class.forName`,
    /// `newInstance`, `getEnumConstants` and `ServiceLoader` reach. Constructors by name are the
    /// no-argument ones of concrete classes (what those APIs call).
    fn reflect_table(&self) -> String {
        let mut s = String::from(
            "/// Reflection over the mod's classes (see `Rt::reflect`).\n\
             pub fn reflect(rt: &mut Rt, class: &str, what: &str, args: &[V]) -> Option<V> {\n\
             \x20   Some(match (class, what) {\n",
        );
        for c in self.classes.values() {
            let ensure = self.ensure(&c.name).replace("return V::Void;", "return Some(V::Void);");
            if !ensure.is_empty() {
                let _ = writeln!(s, "        ({:?}, \"<clinit>\") => {{ {ensure}V::Void }}", c.name);
            }
            // Every constructor of a class that can be made (`getConstructor(Builder.class)
            // .newInstance(builder)` as Max Henkel's configs are made): the runtime hands it the
            // arguments, primitives unboxed.
            let concrete = c.access & 0x0600 == 0 && c.mixin_targets().is_empty();
            for m in c.methods.iter().filter(|m| concrete && m.name == "<init>" && m.code.is_some()) {
                let _ = writeln!(
                    s,
                    "        ({:?}, {:?}) => {{ {ensure}let o = rt.new_object_id({:?}, {}); let mut a = vec![o]; a.extend_from_slice(args); {}(rt, &a); o }}",
                    c.name,
                    format!("<init>.{}", m.desc),
                    c.name,
                    self.cid(&c.name),
                    fn_name(&c.name, "<init>", &m.desc)
                );
            }
            if c.access & 0x4000 != 0
                && let Some(m) = c.methods.iter().find(|m| m.name == "values" && m.access & 0x0008 != 0 && m.desc.starts_with("()[") && m.code.is_some())
            {
                let _ = writeln!(s, "        ({:?}, \"values\") => {{ {ensure}{}(rt, &[]) }}", c.name, fn_name(&c.name, "values", &m.desc));
            }
        }
        for (iface, impls) in &self.services {
            let _ = writeln!(
                s,
                "        ({iface:?}, \"services\") => {{ let names: &[&str] = &{impls:?}; let a = rt.new_array(names.len() as i32, V::NULL); for (i, n) in names.iter().enumerate() {{ let v = rt.string(n); rt.array_set(a, i as i32, v); }} a }}"
            );
        }
        s.push_str("        _ => return None,\n    })\n}\n");
        s
    }

    pub fn emit(&mut self) -> String {
        let mut out = String::from(
            "// Generated by horizon-mod-translator. Do not edit.\n\
             #![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, clippy::all)]\n\
             use horizon_compat::{api, Rt, V};\n\n",
        );
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        for c in &classes {
            for m in &c.methods {
                if let Some(code) = &m.code {
                    let body = self.method(c, &m.name, &m.desc, m.access & 0x0008 != 0, code);
                    out.push_str(&body);
                    for f in std::mem::take(&mut self.synthetic) {
                        out.push_str(&f);
                    }
                }
            }
        }
        out.push_str(&self.tables(&classes));
        out
    }

    /// Virtual dispatch: a dispatcher per instance method (`name.desc`) that some mod class
    /// implements, jumping on the receiver's class id; and the runtime's entry points by key.
    fn dispatch(&self) -> String {
        let ts = "toString.()Ljava/lang/String;";
        let to_string = if self.vkeys.contains(ts) {
            format!(
                "    if {}(cid) {{ return {}(rt, cid, \"java/lang/Object.toString.()Ljava/lang/String;\", &[v]); }}\n",
                overrider(ts),
                dispatcher(ts)
            )
        } else {
            String::new()
        };
        let mut s = format!(
            "/// `String.valueOf(v)` for an object: its mod class's own `toString()`, an enum constant's name,\n\
             /// or `v` itself (strings, boxes and API objects, which the runtime prints).\n\
             pub fn java_string(rt: &mut Rt, v: V) -> V {{\n\
             \x20   if !matches!(v, V::R(_)) || v.r() == 0 || rt.str_of(v).is_some() {{ return v; }}\n\
             \x20   let cid = rt.cid(v);\n\
             {to_string}\
             \x20   let name = rt.get_field(v, \"$name\");\n\
             \x20   if rt.str_of(name).is_some() {{ name }} else {{ v }}\n\
             }}\n\n"
        );
        s.push_str(concat!(
            "/// Virtual dispatch by key (`name.desc`): the runtime's calls back into the mod.\n",
            "pub fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n",
            "    let cid = rt.recv_cid(args);\n",
            "    vcall_cid(rt, cid, owner_key, key, args)\n",
            "}\n\n",
            "fn vcall_cid(rt: &mut Rt, cid: u32, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n",
            "    match rt.vkeys.get(key).copied() {\n",
            "        Some((d, _)) => d(rt, cid, owner_key, args),\n",
            "        None => vcall_miss(rt, cid, owner_key, key, args),\n",
            "    }\n",
            "}\n\n",
            "/// `vcall` as if the receiver were of class `class` (a lambda's interface, for its default methods).\n",
            "pub fn vcall_as(rt: &mut Rt, class: &str, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n",
            "    let cid = rt.class_ids.get(class).copied().unwrap_or(0);\n",
            "    vcall_cid(rt, cid, owner_key, key, args)\n",
            "}\n\n",
            "/// No mod class of the receiver implements `key`: a lambda's own method (or a default method of\n",
            "/// its interface), or the API's.\n",
            "pub fn vcall_miss(rt: &mut Rt, cid: u32, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n",
            "    if cid & horizon_compat::LAMBDA == 0 {\n",
            "        return api::call(rt, owner_key, args);\n",
            "    }\n",
            "    let recv = args.first().copied().unwrap_or_default();\n",
            "    match rt.lambda_info.get(&recv.r()).copied() {\n",
            "        Some((iface, sam)) if !(key.starts_with(sam) && key.as_bytes().get(sam.len()) == Some(&b'.')) => {\n",
            "            let icid = rt.class_ids.get(iface).copied().unwrap_or(0);\n",
            "            match rt.vkeys.get(key).copied() {\n",
            "                Some((d, o)) if icid != 0 && o(icid) => d(rt, icid, owner_key, args),\n",
            "                _ => api::call(rt, owner_key, args),\n",
            "            }\n",
            "        }\n",
            "        _ => match lambda_call(rt, cid & !horizon_compat::LAMBDA, args) {\n",
            "            Some(r) => r,\n",
            "            None => api::call(rt, owner_key, args),\n",
            "        },\n",
            "    }\n",
            "}\n\n",
            "/// Whether a mod class implements `key` (`name.desc`) itself or through a mod superclass.\n",
            "pub fn overrides(class: &str, key: &str) -> bool {\n",
            "    let Ok(c) = CLASS_IDS.binary_search_by(|(n, _)| (*n).cmp(class)) else { return false };\n",
            "    let Ok(k) = VKEYS.binary_search_by(|(n, _, _)| (*n).cmp(key)) else { return false };\n",
            "    (VKEYS[k].2)(CLASS_IDS[c].1)\n",
            "}\n\n",
        ));
        for nd in &self.vkeys {
            let Some((name, desc)) = nd.split_once('.') else { continue };
            let mut by_imp: BTreeMap<String, Vec<u32>> = BTreeMap::new();
            for (class, &id) in &self.class_ids {
                if let Some(imp) = self.resolve(class, name, desc) {
                    by_imp.entry(imp).or_default().push(id);
                }
            }
            let _ = writeln!(s, "pub fn {}(rt: &mut Rt, cid: u32, owner_key: &'static str, args: &[V]) -> V {{\n    match cid {{", dispatcher(nd));
            let mut all = Vec::new();
            for (imp, ids) in &by_imp {
                let _ = writeln!(s, "        {} => {}(rt, args),", ids_pattern(ids), fn_name(imp, name, desc));
                all.extend_from_slice(ids);
            }
            let _ = writeln!(s, "        _ => vcall_miss(rt, cid, owner_key, {nd:?}, args),\n    }}\n}}");
            let _ = writeln!(s, "pub fn {}(cid: u32) -> bool {{\n    matches!(cid, {})\n}}\n", overrider(nd), ids_pattern(&all));
        }
        s
    }

    /// Calls on lambda objects, by lambda id: captured values first, then the call's own arguments.
    fn lambda_table(&self) -> String {
        let mut s = String::from(
            "/// A call on a lambda object (`lid`: its implementation's id).\n\
             pub fn lambda_call(rt: &mut Rt, lid: u32, args: &[V]) -> Option<V> {\n\
             \x20   let mut full = rt.captured(args.first().copied().unwrap_or_default());\n\
             \x20   full.extend_from_slice(args.get(1..).unwrap_or_default());\n\
             \x20   Some(match lid {\n",
        );
        let mut all = self.lambdas.clone();
        for (key, kind, m) in self.subscriber_methods() {
            all.entry(key).or_insert((kind, m));
        }
        let mut rows: Vec<(u32, String)> = Vec::new();
        for (key, (kind, m)) in &all {
            let Some(&lid) = self.lambda_ids.get(key) else { continue };
            let nd = format!("{}.{}", m.name, m.desc);
            let call = match (kind, self.resolve(&m.owner, &m.name, &m.desc)) {
                (8, imp) => {
                    let init = match imp {
                        Some(imp) => format!("{}(rt, &a);", fn_name(&imp, &m.name, &m.desc)),
                        None => format!("api::call(rt, {key:?}, &a);"),
                    };
                    // A constructor reference (`BeltBlock::new`) initialises the class as `new` does.
                    let ensure = self.ensure(&m.owner).replace("return V::Void;", "return Some(V::Void);");
                    format!("{{ {ensure}let o = rt.new_object_id({:?}, {}); let mut a = vec![o]; a.extend(full); {init} o }}", m.owner, self.cid(&m.owner))
                }
                (5 | 9, _) if self.private_method(&m.owner, &m.name, &m.desc) => format!("{}(rt, &full)", fn_name(&m.owner, &m.name, &m.desc)),
                (5 | 9, _) => {
                    // The API's key when no mod class implements it (as at call sites).
                    let owner_key = match self.api_owner(&m.owner) {
                        Some(api) => format!("{api}.{nd}"),
                        None => key.clone(),
                    };
                    if self.vkeys.contains(&nd) {
                        format!("{{ let cid = rt.recv_cid(&full); {}(rt, cid, {owner_key:?}, &full) }}", dispatcher(&nd))
                    } else {
                        format!("{{ let cid = rt.recv_cid(&full); vcall_miss(rt, cid, {owner_key:?}, {nd:?}, &full) }}")
                    }
                }
                // A static method reference to another class (`FanProcessingTypeRegistry::init`)
                // initialises the class that declares it, as `invokestatic` does. A lambda's own
                // body (`lambda$...`) lives in the class that made it, which is initialised already.
                (6, Some(imp)) if !m.name.starts_with("lambda$") => {
                    let ensure = self.ensure(&imp).replace("return V::Void;", "return Some(V::Void);");
                    format!("{{ {ensure}{}(rt, &full) }}", fn_name(&imp, &m.name, &m.desc))
                }
                (_, Some(imp)) => format!("{}(rt, &full)", fn_name(&imp, &m.name, &m.desc)),
                (_, None) => format!("api::call(rt, {key:?}, &full)"),
            };
            rows.push((lid, format!("        {lid} => {call}, // {key}")));
        }
        for (key, f) in &self.operations {
            if let Some(&lid) = self.lambda_ids.get(key) {
                rows.push((lid, format!("        {lid} => {f}(rt, &full), // {key}")));
            }
        }
        rows.sort();
        for (_, row) in rows {
            let _ = writeln!(s, "{row}");
        }
        s.push_str("        _ => return None,\n    })\n}\n");
        s
    }

    fn indy(&mut self, c: &ClassFile, idx: u16, go: &str) -> String {
        let Some((bsm, _name, desc)) = c.indy(idx) else { return go.to_owned() };
        let Some((handle, args)) = c.bootstrap.get(bsm as usize).cloned() else { return go.to_owned() };
        let Some((_, factory)) = c.handle(handle) else { return go.to_owned() };
        let n = arg_kinds(&desc).len();
        let collect = format!("let at = s.len().saturating_sub({n}); let a: Vec<V> = s.split_off(at);");
        let string = |i: u16| match c.pool.get(i as usize) {
            Some(Const::String(u)) => c.utf8(*u).ok(),
            _ => None,
        };
        match (factory.owner.as_str(), factory.name.as_str()) {
            ("java/lang/invoke/StringConcatFactory", "makeConcatWithConstants") => {
                let recipe = args.first().and_then(|&i| string(i)).unwrap_or_default();
                let consts: Vec<String> = args.iter().skip(1).filter_map(|&i| string(i)).collect();
                // Objects print through their own `toString()`, as `String.valueOf` does.
                let kinds = raw_kinds(&desc);
                let fix: String = kinds
                    .chars()
                    .enumerate()
                    .filter(|&(_, k)| k == 'L')
                    .map(|(i, _)| format!(" a[{i}] = java_string(rt, a[{i}]);"))
                    .collect();
                let fix = if fix.is_empty() { fix } else { format!(" let mut a = a;{fix}") };
                format!("{collect}{fix} let v = rt.concat({recipe:?}, {kinds:?}, &a, &{consts:?}); s.push(v); {go}")
            }
            ("java/lang/invoke/LambdaMetafactory", "metafactory" | "altMetafactory") => {
                let Some((kind, m)) = args.get(1).and_then(|&i| c.handle(i)) else {
                    self.unsupported("lambda without an implementation handle");
                    return format!("s.push(V::NULL); {go}");
                };
                let key = format!("{}.{}.{}", m.owner, m.name, m.desc);
                self.lambdas.insert(key.clone(), (kind, m));
                // The functional interface and its method: other calls on the lambda are the
                // interface's default methods.
                let iface = desc.rsplit(')').next().unwrap_or("").trim_start_matches('L').trim_end_matches(';').to_owned();
                let lid = self.lambda_id(&key);
                format!("{collect} let v = rt.lambda_id({key:?}, {lid}, a); rt.lambda_info.insert(v.r(), ({iface:?}, {_name:?})); s.push(v); {go}")
            }
            ("java/lang/runtime/ObjectMethods", "bootstrap") => {
                let names = args.get(1).and_then(|&i| string(i)).unwrap_or_default();
                let kinds: Vec<char> = args
                    .iter()
                    .skip(2)
                    .filter_map(|&i| c.handle(i))
                    .map(|(_, m)| raw_kinds(&format!("({})V", m.desc)).chars().next().unwrap_or('L'))
                    .collect();
                let fields: Vec<String> = names
                    .split(';')
                    .filter(|n| !n.is_empty())
                    .zip(kinds.iter().chain(std::iter::repeat(&'L')))
                    .map(|(n, k)| format!("({n:?}, {k:?})"))
                    .collect();
                let simple = c.name.rsplit(['/', '$']).next().unwrap_or(&c.name).to_owned();
                format!(
                    "{collect} let v = rt.record_method({_name:?}, {simple:?}, &[{}], &a); s.push(v); {go}",
                    fields.join(", ")
                )
            }
            (o, n) => {
                self.unsupported(&format!("invokedynamic {o}.{n}"));
                format!("{collect} s.push(V::NULL); {go}")
            }
        }
    }

    fn method(&mut self, c: &ClassFile, name: &str, desc: &str, is_static: bool, code: &crate::class::Code) -> String {
        let mut s = String::new();
        let f = fn_name(&c.name, name, desc);
        let key = format!("{}.{}.{}", c.name, name, desc);
        // A `@WrapMethod` handler takes the method's name; its body becomes the `Operation` the
        // handler may call.
        let wrap = self.weave.weaves.get(&key).and_then(|ws| ws.iter().find(|w| w.kind == Kind::WrapMethod).cloned());
        let body_name = if wrap.is_some() { format!("{f}__body") } else { f.clone() };
        let _ = writeln!(s, "/// `{}.{}{}`", c.name, name, desc);
        let _ = writeln!(s, "pub fn {body_name}(rt: &mut Rt, args: &[V]) -> V {{");
        // An intrinsic: the runtime does this method's work natively (a whole buffer drawn in one
        // call, say, instead of its translated per-vertex loop).
        if horizon_compat::render::INTRINSICS.contains(&key.as_str()) {
            let _ = writeln!(s, "    api::call(rt, {key:?}, args)\n}}\n");
            return s;
        }
        let _ = writeln!(s, "    let mut l = [V::Void; {}];", (code.max_locals as usize).max(1));
        // Arguments to local slots: wide values take two.
        let mut slot = 0usize;
        let mut ai = 0usize;
        if !is_static {
            let _ = writeln!(s, "    l[0] = args.first().copied().unwrap_or_default();");
            slot = 1;
            ai = 1;
        }
        for k in arg_kinds(desc) {
            let _ = writeln!(s, "    l[{slot}] = args.get({ai}).copied().unwrap_or_default();");
            slot += if k == 'J' || k == 'D' { 2 } else { 1 };
            ai += 1;
        }
        let _ = writeln!(s, "    let mut s: Vec<V> = Vec::with_capacity({});", code.max_stack.max(1));
        let insns = match insn::decode(&code.bytes) {
            Ok(i) => i,
            Err(e) => {
                self.unsupported(&format!("decode: {e}"));
                let _ = writeln!(s, "    V::Void\n}}\n");
                return s;
            }
        };
        let _ = writeln!(s, "    let mut pc: usize = 0;");
        let _ = writeln!(s, "    let mut thrown_at: usize = 0;");
        // Weaving: the mixins' handlers for this method, and the calls they go at (Mixin counts a
        // call's `ordinal` among the method's calls of the same target, in order).
        let mut cur = Cur { class: c.name.clone(), fn_name: f.clone(), name: name.to_owned(), desc: desc.to_owned(), is_static, ..Cur::default() };
        cur.weaves = self.weave.weaves.get(&key).cloned().unwrap_or_default();
        let mut matched = vec![0usize; cur.weaves.len()];
        for i in &insns {
            if matches!(i.op, 0xac..=0xb1) {
                cur.tail = i.at;
            }
            let Some(m) = (!cur.weaves.is_empty() && matches!(i.op, 0xb6..=0xb9)).then(|| insn_member(c, i)).flatten() else { continue };
            for (wi, w) in cur.weaves.iter().enumerate() {
                if let At::Invoke { ordinal, .. } = &w.at
                    && w.at.names_call(&m)
                {
                    if ordinal.is_none_or(|o| o == matched[wi]) {
                        cur.at_calls.entry(i.at).or_default().push(wi);
                    }
                    matched[wi] += 1;
                }
            }
        }
        let missing = cur.weaves.iter().zip(&matched).filter(|(w, n)| matches!(w.at, At::Invoke { .. }) && **n == 0).count();
        for _ in 0..missing {
            self.unsupported("mixin @At(INVOKE) call not in the method");
        }
        let head: Vec<Weave> = cur.weaves.iter().filter(|w| w.at == At::Head).cloned().collect();
        self.cur = Some(cur);
        for w in &head {
            let code = self.inject_code(w, None, 0, false);
            let _ = writeln!(s, "    if pc != {THROW} {{ {code} }}");
        }
        let _ = writeln!(s, "    loop {{");
        let _ = writeln!(s, "        match pc {{");
        for (n, i) in insns.iter().enumerate() {
            let next = insns.get(n + 1).map_or(usize::MAX, |x| x.at);
            let body = self.insn(c, i, next);
            let _ = writeln!(s, "            {} => {{ {body} }}", i.at);
        }
        // Unwinding: the first handler covering the throwing instruction whose
        // class matches catches; otherwise the exception leaves the method.
        let _ = writeln!(s, "            {THROW} => {{");
        let _ = writeln!(s, "                let e = rt.thrown.unwrap_or_default();");
        for (start, end, handler, class) in &code.handlers {
            let test = if class.is_empty() { "true".to_owned() } else { format!("rt.instance_of_static(e, {class:?})") };
            let _ = writeln!(
                s,
                "                if thrown_at >= {start} && thrown_at < {end} && {test} {{ rt.thrown = None; rt.unwinding.clear(); s.clear(); s.push(e); pc = {handler}; continue; }}"
            );
        }
        // Not caught here: the exception leaves the method (recorded for `HORIZON_COMPAT_TRACE`).
        let _ = writeln!(s, "                rt.unwound({:?});", format!("{}.{}", c.name, name));
        let _ = writeln!(s, "                return V::Void;");
        let _ = writeln!(s, "            }}");
        let _ = writeln!(s, "            _ => return V::Void,");
        let _ = writeln!(s, "        }}\n    }}\n}}\n");
        self.cur = None;
        if let Some(w) = wrap {
            let lkey = format!("{key}#wrap");
            let lid = self.lambda_id(&lkey);
            let recv = usize::from(!is_static);
            let this = if w.handler_static { "" } else { "args[0], " };
            let _ = writeln!(
                s,
                "pub fn {f}(rt: &mut Rt, args: &[V]) -> V {{\n    let op = rt.lambda_id({lkey:?}, {lid}, args.get(..{recv}).unwrap_or_default().to_vec());\n    rt.lambda_info.insert(op.r(), ({:?}, \"call\"));\n    let mut h: Vec<V> = vec![{this}];\n    h.extend_from_slice(args.get({recv}..).unwrap_or_default());\n    h.push(op);\n    {}(rt, &h)\n}}\n",
                weave::OPERATION,
                fn_name(&w.mixin, &w.handler, &w.handler_desc)
            );
            let op_fn = format!("{f}__op");
            let _ = writeln!(
                s,
                "fn {op_fn}(rt: &mut Rt, all: &[V]) -> V {{\n    let arr = all.last().copied().unwrap_or_default();\n    let mut a: Vec<V> = all.get(..{recv}).unwrap_or_default().to_vec();\n    a.extend([{}]);\n    let r = {body_name}(rt, &a);\n    {}\n}}\n",
                unboxed_items(&arg_kinds(desc)),
                boxed(return_kind(desc))
            );
            self.operations.insert(lkey, op_fn);
        }
        s
    }

    /// The target method's arguments where they are now: its local slots.
    fn target_args(&self) -> Vec<String> {
        let Some(cur) = self.cur.as_ref() else { return Vec::new() };
        let mut slot = usize::from(!cur.is_static);
        arg_kinds(&cur.desc)
            .into_iter()
            .map(|k| {
                let e = format!("l[{slot}]");
                slot += if matches!(k, 'J' | 'D') { 2 } else { 1 };
                e
            })
            .collect()
    }

    /// An `@Inject` handler's call as Mixin weaves it: a callback (a `CallbackInfoReturnable` for a
    /// method with a value, holding `value` at a return), the handler with the target object, the
    /// target's arguments when it takes them, and the callback; then the method returns early
    /// when the handler cancelled. A throw unwinds from offset `at` (from none at the head).
    fn inject_code(&self, w: &Weave, value: Option<&str>, at: usize, in_loop: bool) -> String {
        let Some(cur) = self.cur.as_ref() else { return String::new() };
        let ret = return_kind(&cur.desc);
        let cancellable = matches!(w.kind, Kind::Inject { cancellable: true });
        let class = if ret == 'V' { weave::CALLBACK_INFO } else { weave::RETURNABLE };
        let held = match (ret, value) {
            ('V', _) | (_, None) => String::new(),
            ('L', Some(_)) => "Ljava/lang/Object;".to_owned(),
            (k, Some(_)) => k.to_string(),
        };
        let init = format!("{class}.<init>.(Ljava/lang/String;Z{held})V");
        let value_arg = match value {
            Some(v) if !held.is_empty() => format!(", {v}"),
            _ => String::new(),
        };
        let targs = self.target_args();
        let mut list: Vec<String> = Vec::new();
        if !w.handler_static {
            list.push("l[0]".into());
        }
        if !targs.is_empty() && w.handler_params() == targs.len() + 1 {
            list.extend(targs);
        }
        list.push("ci".into());
        let back = match ret {
            'V' => "V::Void".to_owned(),
            'L' => format!("api::call(rt, {:?}, &[ci])", format!("{}.getReturnValue.()Ljava/lang/Object;", weave::RETURNABLE)),
            k => format!("api::call(rt, {:?}, &[ci])", format!("{}.getReturnValue{k}.(){k}", weave::RETURNABLE)),
        };
        let throw = if in_loop { format!("thrown_at = {at}; pc = {THROW}; continue;") } else { format!("thrown_at = usize::MAX; pc = {THROW};") };
        let cancel = if cancellable {
            format!(" else if api::call(rt, {:?}, &[ci]).i() != 0 {{ return {back}; }}", format!("{class}.isCancelled.()Z"))
        } else {
            String::new()
        };
        format!(
            "{{ let ci = rt.new_object({class:?}); let id = rt.string({:?}); api::call(rt, {init:?}, &[ci, id, V::I({}){value_arg}]); {}(rt, &[{}]); if rt.thrown.is_some() {{ {throw} }}{cancel} }}",
            cur.name,
            cancellable as i32,
            fn_name(&w.mixin, &w.handler, &w.handler_desc),
            list.join(", ")
        )
    }

    /// What the weaves at a return instruction (offset `at`) do before it returns `ret`: a
    /// `@ModifyReturnValue` replaces it, an `@Inject` sees it (and may return another); `TAIL`
    /// is the method's last return only.
    fn return_weaves(&mut self, at: usize) -> Option<String> {
        let cur = self.cur.as_ref()?;
        let here: Vec<Weave> = cur.weaves.iter().filter(|w| w.at == At::Return || (w.at == At::Tail && at == cur.tail)).cloned().collect();
        if here.is_empty() {
            return None;
        }
        let targs = self.target_args();
        let mut code = String::new();
        for w in &here {
            match w.kind {
                Kind::ModifyReturnValue => {
                    let this = if w.handler_static { "" } else { "l[0], " };
                    let extra = if w.handler_params() == 1 || targs.is_empty() { String::new() } else { format!(", {}", targs.join(", ")) };
                    let _ = write!(
                        code,
                        "ret = {}(rt, &[{this}ret{extra}]); if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }} ",
                        fn_name(&w.mixin, &w.handler, &w.handler_desc)
                    );
                }
                Kind::Inject { .. } => {
                    code.push_str(&self.inject_code(w, Some("ret"), at, true));
                    code.push(' ');
                }
                _ => {}
            }
        }
        Some(code)
    }

    fn key(&mut self, m: &Member) -> String {
        let k = format!("{}.{}.{}", m.owner, m.name, m.desc);
        self.keys.insert(k.clone());
        k
    }

    /// One instruction's Rust; `next` is the following instruction's offset.
    fn insn(&mut self, c: &ClassFile, i: &Insn, next: usize) -> String {
        let go = format!("pc = {next};");
        let idx = match &i.arg {
            Operand::Int(v) => *v,
            Operand::IndexCount(v, _) => *v as i32,
            _ => 0,
        };
        let pop = "s.pop().unwrap_or_default()";
        let bin = |ty: &str, wrap: &str, expr: &str| {
            format!("let b = {pop}.{ty}(); let a = {pop}.{ty}(); s.push(V::{wrap}({expr})); {go}")
        };
        let cmp_jump = |cond: &str| match &i.arg {
            Operand::Jump(t) => format!("if {cond} {{ pc = {t}; }} else {{ {go} }}"),
            _ => go.clone(),
        };
        match i.op {
            0x00 => go,
            0x01 => format!("s.push(V::NULL); {go}"),
            0x02..=0x08 => format!("s.push(V::I({})); {go}", i.op as i32 - 3),
            0x09 | 0x0a => format!("s.push(V::J({})); {go}", i.op - 0x09),
            0x0b..=0x0d => format!("s.push(V::F({}.0)); {go}", i.op - 0x0b),
            0x0e | 0x0f => format!("s.push(V::D({}.0)); {go}", i.op - 0x0e),
            0x10 | 0x11 => format!("s.push(V::I({idx})); {go}"),
            0x12..=0x14 => match c.pool.get(idx as usize) {
                Some(Const::Int(v)) => format!("s.push(V::I({v})); {go}"),
                Some(Const::Float(v)) => format!("s.push(V::F(f32::from_bits({}))); {go}", v.to_bits()),
                Some(Const::Long(v)) => format!("s.push(V::J({v}i64)); {go}"),
                Some(Const::Double(v)) => format!("s.push(V::D(f64::from_bits({}))); {go}", v.to_bits()),
                Some(Const::String(u)) => {
                    let text = c.utf8(*u).unwrap_or_default();
                    format!("let v = rt.string({text:?}); s.push(v); {go}")
                }
                Some(Const::Class(n)) => {
                    let name = c.utf8(*n).unwrap_or_default();
                    format!("let v = rt.alloc(\"java/lang/Class\", horizon_compat::Data::Str({name:?}.into())); s.push(v); {go}")
                }
                _ => {
                    self.unsupported("ldc of a dynamic constant");
                    format!("s.push(V::NULL); {go}")
                }
            },
            // loads
            0x15..=0x19 => format!("s.push(l[{idx}]); {go}"),
            0x1a..=0x2d => format!("s.push(l[{}]); {go}", (i.op - 0x1a) % 4),
            // array loads
            0x2e..=0x35 => format!("let ix = {pop}.i(); let a = {pop}; s.push(rt.array_get(a, ix)); {go}"),
            // stores
            0x36..=0x3a => format!("l[{idx}] = {pop}; {go}"),
            0x3b..=0x4e => format!("l[{}] = {pop}; {go}", (i.op - 0x3b) % 4),
            0x4f..=0x56 => format!("let v = {pop}; let ix = {pop}.i(); let a = {pop}; rt.array_set(a, ix, v); {go}"),
            0x57 => format!("s.pop(); {go}"),
            0x58 => format!("if matches!(s.pop(), Some(V::J(_) | V::D(_))) {{}} else {{ s.pop(); }} {go}"),
            0x59 => format!("let v = *s.last().unwrap(); s.push(v); {go}"),
            0x5a => format!("let a = {pop}; let b = {pop}; s.push(a); s.push(b); s.push(a); {go}"),
            0x5b => format!(
                "let a = {pop}; let b = {pop}; if matches!(b, V::J(_) | V::D(_)) {{ s.push(a); s.push(b); s.push(a); }} else {{ let c = {pop}; s.push(a); s.push(c); s.push(b); s.push(a); }} {go}"
            ),
            0x5c => format!(
                "let a = {pop}; if matches!(a, V::J(_) | V::D(_)) {{ s.push(a); s.push(a); }} else {{ let b = {pop}; s.push(b); s.push(a); s.push(b); s.push(a); }} {go}"
            ),
            0x5d => format!(
                "let a = {pop}; if matches!(a, V::J(_) | V::D(_)) {{ let b = {pop}; s.push(a); s.push(b); s.push(a); }} else {{ let b = {pop}; let c = {pop}; s.push(b); s.push(a); s.push(c); s.push(b); s.push(a); }} {go}"
            ),
            0x5e => {
                self.unsupported("dup2_x2");
                go
            }
            0x5f => format!("let a = {pop}; let b = {pop}; s.push(a); s.push(b); {go}"),
            0x60 => bin("i", "I", "a.wrapping_add(b)"),
            0x61 => bin("j", "J", "a.wrapping_add(b)"),
            0x62 => bin("f", "F", "a + b"),
            0x63 => bin("d", "D", "a + b"),
            0x64 => bin("i", "I", "a.wrapping_sub(b)"),
            0x65 => bin("j", "J", "a.wrapping_sub(b)"),
            0x66 => bin("f", "F", "a - b"),
            0x67 => bin("d", "D", "a - b"),
            0x68 => bin("i", "I", "a.wrapping_mul(b)"),
            0x69 => bin("j", "J", "a.wrapping_mul(b)"),
            0x6a => bin("f", "F", "a * b"),
            0x6b => bin("d", "D", "a * b"),
            0x6c => format!("let b = {pop}.i(); let a = {pop}.i(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; }} else {{ s.push(V::I(horizon_compat::idiv(a, b))); {go} }}", at = i.at),
            0x6d => format!("let b = {pop}.j(); let a = {pop}.j(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; }} else {{ s.push(V::J(horizon_compat::ldiv(a, b))); {go} }}", at = i.at),
            0x6e => bin("f", "F", "a / b"),
            0x6f => bin("d", "D", "a / b"),
            0x70 => format!("let b = {pop}.i(); let a = {pop}.i(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; }} else {{ s.push(V::I(horizon_compat::irem(a, b))); {go} }}", at = i.at),
            0x71 => format!("let b = {pop}.j(); let a = {pop}.j(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; }} else {{ s.push(V::J(horizon_compat::lrem(a, b))); {go} }}", at = i.at),
            0x72 => bin("f", "F", "a % b"),
            0x73 => bin("d", "D", "a % b"),
            0x74 => format!("let a = {pop}.i(); s.push(V::I(a.wrapping_neg())); {go}"),
            0x75 => format!("let a = {pop}.j(); s.push(V::J(a.wrapping_neg())); {go}"),
            0x76 => format!("let a = {pop}.f(); s.push(V::F(-a)); {go}"),
            0x77 => format!("let a = {pop}.d(); s.push(V::D(-a)); {go}"),
            0x78 => format!("let b = {pop}.i(); let a = {pop}.i(); s.push(V::I(a.wrapping_shl(b as u32 & 31))); {go}"),
            0x79 => format!("let b = {pop}.i(); let a = {pop}.j(); s.push(V::J(a.wrapping_shl(b as u32 & 63))); {go}"),
            0x7a => format!("let b = {pop}.i(); let a = {pop}.i(); s.push(V::I(a >> (b & 31))); {go}"),
            0x7b => format!("let b = {pop}.i(); let a = {pop}.j(); s.push(V::J(a >> (b & 63))); {go}"),
            0x7c => format!("let b = {pop}.i(); let a = {pop}.i(); s.push(V::I(((a as u32) >> (b & 31)) as i32)); {go}"),
            0x7d => format!("let b = {pop}.i(); let a = {pop}.j(); s.push(V::J(((a as u64) >> (b & 63)) as i64)); {go}"),
            0x7e => bin("i", "I", "a & b"),
            0x7f => bin("j", "J", "a & b"),
            0x80 => bin("i", "I", "a | b"),
            0x81 => bin("j", "J", "a | b"),
            0x82 => bin("i", "I", "a ^ b"),
            0x83 => bin("j", "J", "a ^ b"),
            0x84 => match &i.arg {
                Operand::Iinc(slot, d) => format!("l[{slot}] = V::I(l[{slot}].i().wrapping_add({d})); {go}"),
                _ => go,
            },
            // conversions
            0x85 => format!("let a = {pop}.i(); s.push(V::J(a as i64)); {go}"),
            0x86 => format!("let a = {pop}.i(); s.push(V::F(a as f32)); {go}"),
            0x87 => format!("let a = {pop}.i(); s.push(V::D(a as f64)); {go}"),
            0x88 => format!("let a = {pop}.j(); s.push(V::I(a as i32)); {go}"),
            0x89 => format!("let a = {pop}.j(); s.push(V::F(a as f32)); {go}"),
            0x8a => format!("let a = {pop}.j(); s.push(V::D(a as f64)); {go}"),
            0x8b => format!("let a = {pop}.f(); s.push(V::I(a as i32)); {go}"),
            0x8c => format!("let a = {pop}.f(); s.push(V::J(a as i64)); {go}"),
            0x8d => format!("let a = {pop}.f(); s.push(V::D(a as f64)); {go}"),
            0x8e => format!("let a = {pop}.d(); s.push(V::I(a as i32)); {go}"),
            0x8f => format!("let a = {pop}.d(); s.push(V::J(a as i64)); {go}"),
            0x90 => format!("let a = {pop}.d(); s.push(V::F(a as f32)); {go}"),
            0x91 => format!("let a = {pop}.i(); s.push(V::I(a as i8 as i32)); {go}"),
            0x92 => format!("let a = {pop}.i(); s.push(V::I(a as u16 as i32)); {go}"),
            0x93 => format!("let a = {pop}.i(); s.push(V::I(a as i16 as i32)); {go}"),
            // comparisons
            0x94 => format!("let b = {pop}.j(); let a = {pop}.j(); s.push(V::I((a > b) as i32 - (a < b) as i32)); {go}"),
            0x95 => format!("let b = {pop}.f() as f64; let a = {pop}.f() as f64; s.push(V::I(horizon_compat::fcmp(a, b, -1))); {go}"),
            0x96 => format!("let b = {pop}.f() as f64; let a = {pop}.f() as f64; s.push(V::I(horizon_compat::fcmp(a, b, 1))); {go}"),
            0x97 => format!("let b = {pop}.d(); let a = {pop}.d(); s.push(V::I(horizon_compat::fcmp(a, b, -1))); {go}"),
            0x98 => format!("let b = {pop}.d(); let a = {pop}.d(); s.push(V::I(horizon_compat::fcmp(a, b, 1))); {go}"),
            0x99..=0x9e => {
                let op = ["==", "!=", "<", ">=", ">", "<="][(i.op - 0x99) as usize];
                format!("let a = {pop}.i(); {}", cmp_jump(&format!("a {op} 0")))
            }
            0x9f..=0xa4 => {
                let op = ["==", "!=", "<", ">=", ">", "<="][(i.op - 0x9f) as usize];
                format!("let b = {pop}.i(); let a = {pop}.i(); {}", cmp_jump(&format!("a {op} b")))
            }
            0xa5 => format!("let b = {pop}.r(); let a = {pop}.r(); {}", cmp_jump("a == b")),
            0xa6 => format!("let b = {pop}.r(); let a = {pop}.r(); {}", cmp_jump("a != b")),
            0xa7 | 0xc8 => match &i.arg {
                Operand::Jump(t) => format!("pc = {t};"),
                _ => go,
            },
            0xaa | 0xab => match &i.arg {
                Operand::Switch(default, pairs) => {
                    let mut m = format!("let k = {pop}.i(); pc = match k {{ ");
                    for (k, t) in pairs {
                        let _ = write!(m, "{k} => {t}, ");
                    }
                    let _ = write!(m, "_ => {default} }};");
                    m
                }
                _ => go,
            },
            0xac..=0xb0 => match self.return_weaves(i.at) {
                Some(code) => format!("let mut ret = {pop}; {code}return ret;"),
                None => format!("return {pop};"),
            },
            0xb1 => match self.return_weaves(i.at) {
                Some(code) => format!("let mut ret = V::Void; {code}return V::Void;"),
                None => "return V::Void;".into(),
            },
            0xb2..=0xb5 => self.field(c, i.op, idx as u16, &go),
            0xb6..=0xb9 => self.invoke(c, i.op, idx as u16, &go, i.at),
            0xba => self.indy(c, idx as u16, &go),
            0xbb => {
                let class = c.class_name(idx as u16).unwrap_or_default();
                let init = if class != c.name { self.ensure(&class) } else { String::new() };
                format!("{init}let v = rt.new_object_id({class:?}, {}); s.push(v); {go}", self.cid(&class))
            }
            0xbc => {
                let zero = match idx {
                    6 => "V::F(0.0)",
                    7 => "V::D(0.0)",
                    11 => "V::J(0)",
                    _ => "V::I(0)",
                };
                format!("let n = {pop}.i(); let v = rt.new_array(n, {zero}); s.push(v); {go}")
            }
            0xbd => format!("let n = {pop}.i(); let v = rt.new_array(n, V::NULL); s.push(v); {go}"),
            0xbe => format!("let a = {pop}; s.push(V::I(rt.array_len(a))); {go}"),
            0xbf => format!("let e = {pop}; rt.thrown = Some(e); thrown_at = {}; pc = {THROW};", i.at),
            0xc0 => go, // checkcast: translated code trusts the mod's types
            0xc1 => {
                let class = c.class_name(idx as u16).unwrap_or_default();
                format!("let a = {pop}; s.push(V::I(rt.instance_of_static(a, {class:?}) as i32)); {go}")
            }
            0xc2 | 0xc3 => format!("s.pop(); {go}"),
            0xc5 => match &i.arg {
                Operand::IndexCount(t, dims) => {
                    let ty = c.class_name(*t).unwrap_or_default();
                    let zero = match ty.trim_start_matches('[').chars().next() {
                        Some('J') => "V::J(0)",
                        Some('F') => "V::F(0.0)",
                        Some('D') => "V::D(0.0)",
                        Some('L') => "V::NULL",
                        _ => "V::I(0)",
                    };
                    let zero = if ty.len() - ty.trim_start_matches('[').len() > *dims as usize { "V::NULL" } else { zero };
                    format!("let at = s.len().saturating_sub({dims}); let d: Vec<i32> = s.split_off(at).iter().map(|v| v.i()).collect(); let v = rt.new_multi(&d, {zero}); s.push(v); {go}")
                }
                _ => go,
            },
            0xc6 => format!("let a = {pop}.r(); {}", cmp_jump("a == 0")),
            0xc7 => format!("let a = {pop}.r(); {}", cmp_jump("a != 0")),
            op => {
                self.unsupported(&format!("opcode 0x{op:02x}"));
                go
            }
        }
    }

    /// Runs `class`'s static initialiser first if it has one and hasn't run (JVMS 5.5).
    /// Initialises `class` before its use, as the JVM does (JVMS 5.5): its mod superclasses first,
    /// from the top, then the class; each static initialiser runs once.
    fn ensure(&self, class: &str) -> String {
        let has_init = |k: &ClassFile| k.methods.iter().any(|m| m.name == "<clinit>");
        let mut chain: Vec<Vec<String>> = Vec::new();
        let mut c = Some(class);
        while let Some(name) = c {
            let Some(k) = self.classes.get(name) else { break };
            // The class's initialiser, then its mixins' (Mixin merges theirs into it).
            let mut level = Vec::new();
            if has_init(k) {
                level.push(k.name.clone());
            }
            for m in self.mixins_of(&k.name) {
                if self.classes.get(m.as_str()).is_some_and(|mk| has_init(mk)) {
                    level.push(m.clone());
                }
            }
            chain.push(level);
            c = k.superclass.as_deref();
        }
        chain
            .iter()
            .rev()
            .flatten()
            .map(|k| {
                format!(
                    "if rt.initialized.insert({k:?}) {{ {}(rt, &[]); if rt.thrown.is_some() {{ return V::Void; }} }} ",
                    fn_name(k, "<clinit>", "()V")
                )
            })
            .collect()
    }

    /// The class declaring static field `name` as the JVM resolves a reference through `owner`
    /// (JVMS 5.4.3.2): `owner`, its superinterfaces, then its superclass, recursively. Past the
    /// mod's classes, the API class the chain reaches (`MyBlock.FACING` → vanilla's own class).
    fn field_owner(&self, owner: &str, name: &str) -> String {
        // A mixin's static is its target's (a `@Shadow`, or one the target inherits), unless only
        // the mixin declares it.
        if let Some(target) = self.mixin_of.get(owner) {
            let found = self.field_owner(target, name);
            let target_has = self.classes.get(found.as_str()).is_some_and(|k| k.fields.iter().any(|f| f.name == name));
            let mixin_has = self.classes.get(owner).is_some_and(|k| k.fields.iter().any(|f| f.name == name));
            return if mixin_has && !target_has { owner.to_owned() } else { found };
        }
        let mut c = owner;
        loop {
            let Some(k) = self.classes.get(c) else { return c.to_owned() };
            if k.fields.iter().any(|f| f.name == name) {
                return k.name.clone();
            }
            if let Some(i) = self.interface_field(k, name) {
                return i;
            }
            match k.superclass.as_deref() {
                Some(sup) => c = sup,
                None => return owner.to_owned(),
            }
        }
    }

    fn interface_field(&self, k: &ClassFile, name: &str) -> Option<String> {
        for i in &k.interfaces {
            let Some(ik) = self.classes.get(i.as_str()) else { continue };
            if ik.fields.iter().any(|f| f.name == name) {
                return Some(ik.name.clone());
            }
            if let Some(found) = self.interface_field(ik, name) {
                return Some(found);
            }
        }
        None
    }

    fn field(&mut self, c: &ClassFile, op: u8, idx: u16, go: &str) -> String {
        let Some(m) = c.member(idx) else { return go.to_owned() };
        let owner = if matches!(op, 0xb2 | 0xb3) { self.field_owner(&m.owner, &m.name) } else { m.owner.clone() };
        // The declaring class initialises, as the JVM does (not the subclass the reference names).
        let init = if matches!(op, 0xb2 | 0xb3) && owner != c.name { self.ensure(&owner) } else { String::new() };
        let pop = "s.pop().unwrap_or_default()";
        let skey = format!("{owner}.{}", m.name);
        // A primitive field nothing wrote reads as its type's zero, as the JVM's defaults (a `long`
        // counter, an `int` angle saved to NBT); a reference one stays unset (null to the code).
        let zero = match m.desc.as_str() {
            "I" | "Z" | "B" | "S" | "C" => Some("V::I(0)"),
            "J" => Some("V::J(0)"),
            "F" => Some("V::F(0.0)"),
            "D" => Some("V::D(0.0)"),
            _ => None,
        };
        match op {
            0xb2 if self.own(&owner) => match zero {
                Some(z) => format!("{init}let v = match rt.statics.get({skey:?}).copied() {{ None | Some(V::Void) => {z}, Some(v) => v }}; s.push(v); {go}"),
                None => format!("{init}let v = rt.statics.get({skey:?}).copied().unwrap_or_default(); s.push(v); {go}"),
            },
            0xb2 => format!("let v = api::get_static(rt, {skey:?}); s.push(v); {go}"),
            0xb3 => format!("{init}let v = {pop}; rt.statics.insert({skey:?}, v); {go}"),
            0xb4 => match zero {
                Some(z) => format!("let o = {pop}; s.push(match rt.get_field(o, {:?}) {{ V::Void => {z}, v => v }}); {go}", m.name),
                None => format!("let o = {pop}; s.push(rt.get_field(o, {:?})); {go}", m.name),
            },
            _ => format!("let v = {pop}; let o = {pop}; rt.put_field(o, {:?}, v); {go}", m.name),
        }
    }

    /// A call to a mixin's accessor or invoker: `@Accessor("pattern")` or `@Invoker` in an interface
    /// that is `@Mixin(ShapedRecipe.class)`. The mixin would add the method to the game's class; here
    /// the call reads or writes the game object's field, or calls the game's method, directly.
    fn mixin_call(&mut self, m: &Member, op: u8) -> Option<String> {
        const ACCESSOR: &str = "Lorg/spongepowered/asm/mixin/gen/Accessor;";
        const INVOKER: &str = "Lorg/spongepowered/asm/mixin/gen/Invoker;";
        let class: &ClassFile = self.classes.get(m.owner.as_str())?;
        let target = class.mixin_target()?;
        let method = class.methods.iter().find(|x| x.name == m.name && x.desc == m.desc && x.code.is_none())?;
        let is_static = op == 0xb8;
        // Without a value, the name follows the method's: `create$getPattern` is `pattern`.
        let named = |a: &crate::class::Annotation, verbs: &[&str]| match a.value("value") {
            Some(v) if !v.is_empty() => v.to_owned(),
            _ => accessor_name(&m.name, verbs),
        };
        if let Some(a) = method.annotations.iter().find(|a| a.ty == ACCESSOR) {
            let field = named(a, &["get", "is", "set"]);
            let setter = m.desc.ends_with(")V");
            return Some(match (is_static, setter) {
                // The runtime keeps a game object's state in `$`-named fields, and a field a mod
                // subclass wrote itself under its plain name.
                (false, false) => format!("match rt.get_field(args[0], {:?}) {{ V::Void => rt.get_field(args[0], {field:?}), v => v }}", format!("${field}")),
                (false, true) => format!(
                    "{{ let own = !matches!(rt.get_field(args[0], {own:?}), V::Void) || matches!(rt.get_field(args[0], {field:?}), V::Void); rt.put_field(args[0], if own {{ {own:?} }} else {{ {field:?} }}, args[1]); V::Void }}",
                    own = format!("${field}")
                ),
                // A static of a translated class is the runtime's own, under its declaring class.
                (true, false) if self.own(&target) => {
                    let owner = self.field_owner(&target, &field);
                    format!("{{ {}rt.statics.get({:?}).copied().unwrap_or_default() }}", self.ensure(&owner), format!("{owner}.{field}"))
                }
                (true, true) if self.own(&target) => {
                    let owner = self.field_owner(&target, &field);
                    format!("{{ {}rt.statics.insert({:?}, args[0]); V::Void }}", self.ensure(&owner), format!("{owner}.{field}"))
                }
                (true, false) => format!("api::get_static(rt, {:?})", format!("{target}.{field}")),
                (true, true) => format!("{{ rt.statics.insert({:?}, args[0]); V::Void }}", format!("{target}.{field}")),
            });
        }
        let a = method.annotations.iter().find(|a| a.ty == INVOKER)?;
        let name = named(a, &["call", "invoke", "new", "create"]);
        if name == "<init>" {
            // A constructor's invoker: static, returning the new object.
            let init = format!("{target}.<init>.{}V", &m.desc[..=m.desc.find(')')?]);
            return Some(format!(
                "{{ let o = rt.new_object({target:?}); let mut a = vec![o]; a.extend_from_slice(&args); api::call(rt, {init:?}, &a); o }}"
            ));
        }
        // A translated class's method is called as its own code.
        if is_static && let Some(imp) = self.resolve(&target, &name, &m.desc) {
            return Some(format!("{{ {}{}(rt, &args) }}", self.ensure(&imp), fn_name(&imp, &name, &m.desc)));
        }
        let owner = if self.own(&target) { self.api_owner(&target).unwrap_or(target) } else { target };
        let key = self.key(&Member { owner, name: name.clone(), desc: m.desc.clone() });
        if is_static {
            return Some(format!("api::call(rt, {key:?}, &args)"));
        }
        // Virtual: a mod subclass's override of the game's method answers first.
        let nd = format!("{name}.{}", m.desc);
        Some(if self.vkeys.contains(&nd) {
            format!("{{ let cid = rt.recv_cid(&args); {}(rt, cid, {key:?}, &args) }}", dispatcher(&nd))
        } else {
            format!("{{ let cid = rt.recv_cid(&args); vcall_miss(rt, cid, {key:?}, {nd:?}, &args) }}")
        })
    }

    fn invoke(&mut self, c: &ClassFile, op: u8, idx: u16, go: &str, at: usize) -> String {
        let Some(m) = c.member(idx) else { return go.to_owned() };
        // A mixin's accessors and invokers first; any other call through a mixin reaches its target.
        let mixin = self.mixin_call(&m, op);
        let m = if mixin.is_some() { m } else { self.through_mixin(m) };
        let n = arg_kinds(&m.desc).len() + usize::from(op != 0xb8);
        // A constructor's call of its superclass's: skipped in a mixin's (the target's constructor
        // made it), followed in a target's by its mixins' field initialisers, as Mixin merges them.
        let super_call = op == 0xb7
            && m.name == "<init>"
            && c.superclass.as_deref() == Some(m.owner.as_str())
            && self.cur.as_ref().is_some_and(|cur| cur.name == "<init>" && !cur.super_done);
        if super_call && let Some(cur) = self.cur.as_mut() {
            cur.super_done = true;
        }
        if super_call && self.mixin_of.contains_key(&c.name) {
            return format!("let at = s.len().saturating_sub({n}); s.truncate(at); {go}");
        }
        let mut initialisers = String::new();
        if super_call {
            for mx in self.mixins_of(&c.name).to_vec() {
                if let Some(ctor) = self.classes.get(mx.as_str()).and_then(|k| k.methods.iter().find(|x| x.name == "<init>" && x.code.is_some())) {
                    let _ = write!(
                        initialisers,
                        "{}(rt, &[l[0]]); if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }} ",
                        fn_name(&mx, "<init>", &ctor.desc)
                    );
                }
            }
        }
        let key = self.key(&m);
        let init = if op == 0xb8 && m.owner != c.name { self.ensure(&m.owner) } else { String::new() };
        let caller = if self.caller_sensitive.contains(&format!("{}.{}", m.name, m.desc)) {
            format!("rt.caller_class = {:?}; ", c.name)
        } else {
            String::new()
        };
        let collect = format!("{init}{caller}let at = s.len().saturating_sub({n}); let args: Vec<V> = s.split_off(at);");
        let push = if returns_value(&m.desc) { "s.push(r);" } else { "" };
        // A method no mod class implements, called through a mod class (`this.getLevel()` in a
        // mod block entity): the API class it is inherited from, found now rather than per call.
        let api_key = |this: &mut Self| match this.api_owner(&m.owner) {
            Some(api) => this.key(&Member { owner: api, name: m.name.clone(), desc: m.desc.clone() }),
            None => key.clone(),
        };
        let call = match op {
            _ if mixin.is_some() => mixin.unwrap_or_default(),
            // static, or a direct (super/constructor/private) call
            0xb8 | 0xb7 => match self.resolve(&m.owner, &m.name, &m.desc) {
                Some(imp) => format!("{}(rt, &args)", fn_name(&imp, &m.name, &m.desc)),
                None => format!("api::call(rt, {:?}, &args)", api_key(self)),
            },
            _ if self.private_method(&m.owner, &m.name, &m.desc) => format!("{}(rt, &args)", fn_name(&m.owner, &m.name, &m.desc)),
            // Virtual: the method's dispatcher jumps on the receiver's class id.
            _ => {
                let nd = format!("{}.{}", m.name, m.desc);
                let owner_key = api_key(self);
                if self.vkeys.contains(&nd) {
                    format!("{{ let cid = rt.recv_cid(&args); {}(rt, cid, {owner_key:?}, &args) }}", dispatcher(&nd))
                } else {
                    format!("{{ let cid = rt.recv_cid(&args); vcall_miss(rt, cid, {owner_key:?}, {nd:?}, &args) }}")
                }
            }
        };
        // `String.valueOf(Object)` and `append(Object)` print through the object's own `toString()`.
        let to_string = matches!(
            key.as_str(),
            "java/lang/String.valueOf.(Ljava/lang/Object;)Ljava/lang/String;"
                | "java/lang/StringBuilder.append.(Ljava/lang/Object;)Ljava/lang/StringBuilder;"
                | "java/lang/StringBuffer.append.(Ljava/lang/Object;)Ljava/lang/StringBuffer;"
        );
        let collect = if to_string {
            format!("{collect} let mut args = args; if let Some(l) = args.len().checked_sub(1) {{ args[l] = java_string(rt, args[l]); }}")
        } else {
            collect
        };
        let woven = self.call_weaves(&m, op, at, call);
        let collect = if woven.mutable { format!("{collect} let mut args = args;") } else { collect };
        format!(
            "{collect} {}let r = {}; if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; }} else {{ {}{push} {initialisers}{}{go} }}",
            woven.before, woven.call, woven.modify, woven.after
        )
    }

    /// The weaves at the call at offset `at` (`m`, made with opcode `op`) around its Rust `call`:
    /// what runs before it (`@Inject`, `@ModifyArg`, `@WrapWithCondition`'s condition), the call
    /// itself or a `@Redirect`'s handler instead, what its result goes through
    /// (`@ModifyExpressionValue`), and what runs after it (`@Inject` with `shift = AFTER`).
    fn call_weaves(&mut self, m: &Member, op: u8, at: usize, call: String) -> Woven {
        let mut out = Woven { call, ..Woven::default() };
        let site: Vec<Weave> = self
            .cur
            .as_ref()
            .and_then(|cur| cur.at_calls.get(&at).map(|ix| ix.iter().map(|&i| cur.weaves[i].clone()).collect()))
            .unwrap_or_default();
        if site.is_empty() {
            return out;
        }
        let recv = usize::from(op != 0xb8);
        let params = crate::class::param_classes(&m.desc);
        let returns = returns_value(&m.desc);
        let targs = self.target_args();
        let throw = format!("if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }}");
        let mut wrapped = false;
        for w in &site {
            let h = fn_name(&w.mixin, &w.handler, &w.handler_desc);
            let this = if w.handler_static { "" } else { "l[0], " };
            let np = w.handler_params();
            match (&w.kind, &w.at) {
                (Kind::Inject { .. }, At::Invoke { after, .. }) => {
                    let code = self.inject_code(w, None, at, true);
                    let to = if *after { &mut out.after } else { &mut out.before };
                    to.push_str(&code);
                    to.push(' ');
                }
                // The handler instead of the call: the receiver and arguments, then the target
                // method's when it takes them.
                (Kind::Redirect, _) => {
                    let extra = if np == recv + params.len() {
                        String::new()
                    } else if np == recv + params.len() + targs.len() {
                        targs.join(", ")
                    } else {
                        self.unsupported("mixin @Redirect taking other parameters");
                        continue;
                    };
                    out.call = format!("{{ let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); h.extend_from_slice(&[{extra}]); {h}(rt, &h) }}");
                }
                // One argument through the handler: the one it is given (or, without an index,
                // the only one of the type it returns), or all of them for it to pick.
                (Kind::ModifyArg { index }, _) => {
                    let ret = w.handler_desc.rsplit(')').next().unwrap_or("");
                    let ret = ret.strip_prefix('L').and_then(|r| r.strip_suffix(';')).unwrap_or(ret);
                    let index = match *index {
                        usize::MAX => {
                            let same: Vec<usize> = params.iter().enumerate().filter(|(_, p)| p.as_str() == ret).map(|(i, _)| i).collect();
                            match same.as_slice() {
                                [only] => *only,
                                _ => {
                                    self.unsupported("mixin @ModifyArg without an index");
                                    continue;
                                }
                            }
                        }
                        i => i,
                    };
                    if index >= params.len() {
                        self.unsupported("mixin @ModifyArg index past the call's arguments");
                        continue;
                    }
                    let i = recv + index;
                    out.mutable = true;
                    if np == 1 {
                        let _ = write!(out.before, "{{ let v = {h}(rt, &[{this}args[{i}]]); {throw} args[{i}] = v; }} ");
                    } else if np == params.len() {
                        let _ = write!(out.before, "{{ let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args[{recv}..]); let v = {h}(rt, &h); {throw} args[{i}] = v; }} ");
                    } else {
                        self.unsupported("mixin @ModifyArg taking other parameters");
                    }
                }
                // The handler instead of the call, given the call as an `Operation` that makes it
                // (with the locals as they are, for what other weaves at the call read).
                (Kind::WrapOperation, At::Invoke { .. }) => {
                    let base = recv + params.len() + 1;
                    let extra = if np == base {
                        String::new()
                    } else if np == base + targs.len() {
                        targs.join(", ")
                    } else {
                        self.unsupported("mixin @WrapOperation taking other parameters");
                        continue;
                    };
                    let Some(cur) = self.cur.as_ref() else { continue };
                    let n = self.operations.len();
                    let lkey = format!("{}.{}.{}@{at}#{n}", cur.class, cur.name, cur.desc);
                    let op_fn = format!("{}__op{at}_{n}", cur.fn_name);
                    let lid = self.lambda_id(&lkey);
                    let mut kinds: Vec<char> = Vec::new();
                    if recv == 1 {
                        kinds.push('L');
                    }
                    kinds.extend(arg_kinds(&m.desc));
                    self.synthetic.push(format!(
                        "fn {op_fn}(rt: &mut Rt, all: &[V]) -> V {{\n    let (l, rest) = all.split_at(all.len().saturating_sub(1));\n    let arr = rest.first().copied().unwrap_or_default();\n    let args: Vec<V> = vec![{}];\n    let r = {};\n    {}\n}}\n",
                        unboxed_items(&kinds),
                        out.call,
                        boxed(return_kind(&m.desc))
                    ));
                    self.operations.insert(lkey.clone(), op_fn);
                    out.call = format!(
                        "{{ let op = rt.lambda_id({lkey:?}, {lid}, l.to_vec()); rt.lambda_info.insert(op.r(), ({:?}, \"call\")); let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); h.push(op); h.extend_from_slice(&[{extra}]); {h}(rt, &h) }}",
                        weave::OPERATION
                    );
                }
                (Kind::ModifyExpressionValue, _) if returns && np == 1 => {
                    let _ = write!(out.modify, "let r = {h}(rt, &[{this}r]); {throw} ");
                }
                // The call only when the handler, given its receiver and arguments, says so.
                (Kind::WrapWithCondition, _) if !returns && np == recv + params.len() => {
                    if !wrapped {
                        out.before.push_str("let mut wrap_ok = true; ");
                        wrapped = true;
                    }
                    let _ = write!(out.before, "if wrap_ok {{ let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); wrap_ok = {h}(rt, &h).i() != 0; {throw} }} ");
                }
                (kind, _) => {
                    let what = format!("mixin {kind:?} at this call");
                    self.unsupported(&what);
                }
            }
        }
        if wrapped {
            out.call = format!("if wrap_ok {{ {} }} else {{ V::Void }}", out.call);
        }
        out
    }
}

#[cfg(test)]
mod mixin_tests {
    #[test]
    fn an_accessor_without_a_value_names_the_member_after_its_method() {
        let accessor = ["get", "is", "set"];
        assert_eq!(super::accessor_name("create$getPattern", &accessor), "pattern");
        assert_eq!(super::accessor_name("isPowered", &accessor), "powered");
        // A constant's name stays as it is; a verb must start a word.
        assert_eq!(super::accessor_name("getOPERATING_SYSTEM", &accessor), "OPERATING_SYSTEM");
        assert_eq!(super::accessor_name("getter", &accessor), "getter");
        assert_eq!(super::accessor_name("mod$callIsValid", &["call", "invoke"]), "isValid");
    }
}
