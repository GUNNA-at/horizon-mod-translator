//! Bytecode → Rust. Each Java method becomes `fn(rt: &mut Rt, args: &[V]) -> V`
//! whose body is a `loop { match pc { ... } }` with an arm per basic block, the
//! operand stack a fixed array on the Rust stack (`Stack`, its `max_stack` values:
//! no allocation for it or a call's arguments) and the locals `V` values. Calls to the mod's own
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

/// What reflection sees of class `c`'s annotations, a line each (see the generated `ANNOTATIONS`):
/// the `RUNTIME` ones on the class, its fields and methods, and an annotation type's element
/// defaults. None when it has none. Kotlin's metadata is left out: only Kotlin's reflection reads it.
fn annotation_lines(c: &ClassFile) -> Option<String> {
    let mut out = String::new();
    let mut line = |target: &str, member: &str, a: &crate::class::Annotation| {
        if a.visible && !a.ty.starts_with("Lkotlin/") {
            let _ = writeln!(out, "{target}\t{member}\t{}\t{}", a.ty, serde_json::Value::Object(a.data.clone()));
        }
    };
    for a in &c.annotations {
        line("TYPE", "", a);
    }
    for f in &c.fields {
        for a in &f.annotations {
            line("FIELD", &f.name, a);
        }
    }
    for m in &c.methods {
        for a in &m.annotations {
            line("METHOD", &format!("{}{}", m.name, m.desc), a);
        }
    }
    for m in &c.methods {
        if let Some(d) = &m.default {
            let _ = writeln!(out, "DEFAULT\t{}\t\t{d}", m.name);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn dispatcher(nd: &str) -> String {
    format!("d_{}_{:08x}", ident(nd.split('.').next().unwrap_or("")), hash(nd))
}

/// Whether a class id implements `nd`: `fn(cid) -> bool`.
fn overrider(nd: &str) -> String {
    format!("o_{}_{:08x}", ident(nd.split('.').next().unwrap_or("")), hash(nd))
}

/// The instructions of `code` that run in the arm of the one before them ([`Cur::joined`]): all
/// but the first, those something jumps to (a branch, a switch, an exception handler) and those
/// after an instruction that doesn't fall through to them (a branch, a switch, a return, a
/// throw).
fn joined(insns: &[Insn], code: &crate::class::Code) -> BTreeSet<usize> {
    let mut starts: BTreeSet<usize> = code.handlers.iter().map(|h| usize::from(h.2)).collect();
    for (n, i) in insns.iter().enumerate() {
        match &i.arg {
            Operand::Jump(t) => {
                starts.insert(*t);
            }
            Operand::Switch(default, cases) => {
                starts.insert(*default);
                starts.extend(cases.iter().map(|(_, t)| *t));
            }
            _ => {}
        }
        // Branches, `goto`, `jsr`, `ret`, switches, returns and `athrow`.
        if matches!(i.op, 0x99..=0xb1 | 0xbf | 0xc6..=0xc9)
            && let Some(next) = insns.get(n + 1)
        {
            starts.insert(next.at);
        }
    }
    insns.iter().skip(1).map(|i| i.at).filter(|at| !starts.contains(at)).collect()
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

/// The top of a generated module's `lib.rs`.
const LIB_HEADER: &str = "// Generated by horizon-mod-translator. Do not edit.\n\
     #![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, clippy::all)]\n";
/// The top of each file of methods: it names what the crate's root has (the other parts, the
/// runtime as `horizon_compat`).
pub(crate) const PART_HEADER: &str = "#![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, unused_imports, clippy::all)]\nuse crate::*;\nuse horizon_compat::{api, Rt, V};\n\n";
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
    /// Each jar translated: the ids of the mods it declares and the classes it has
    /// ([`crate::class_names`]), for FML's scan of it. Empty: one jar of every class.
    pub scan_files: Vec<(Vec<String>, std::collections::HashSet<String>)>,
    /// Methods (`name.desc`) that ask who called them (`StackWalker.getCallerClass`): calls to
    /// them record the calling class first. Everything else keeps no call stack.
    caller_sensitive: BTreeSet<String>,
    /// Mixins whose targets are among these classes, woven into them ([`crate::weave`]).
    weave: weave::Plan,
    /// Each mixin's target (its first translated one, for a mixin of several), or the game's class
    /// a mixin goes into ([`weave::Plan::vanilla`]).
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
    /// Weaves after an instruction, by its offset: (their place, the weave). `INVOKE_ASSIGN`
    /// `@Inject`s after the store of the call's result, `@ModifyVariable`s after the store
    /// they're at, a `shift = BY` that lands on the label after the instruction.
    after_store: BTreeMap<usize, Vec<(usize, usize)>>,
    /// A `shift = BY`'s weaves before the instruction it lands on, by its offset, as
    /// `after_store`.
    before_insn: BTreeMap<usize, Vec<(usize, usize)>>,
    /// The `@Share`d refs the method makes: (mixin, name, ref class), `share0`...
    shares: Vec<(String, String, String)>,
    /// Instructions that only the one before reaches: each runs in that one's arm, after it (a
    /// basic block is one arm). The others (where jumps, switches and handlers go, and those after
    /// an instruction that doesn't fall through) start arms.
    joined: BTreeSet<usize>,
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

/// Each mixin's target: its first translated one, else the first of the game's it goes into.
fn mixin_targets_of(plan: &weave::Plan) -> BTreeMap<String, String> {
    let mut mixin_of = BTreeMap::new();
    for (target, mixins) in &plan.mixins {
        for m in mixins {
            mixin_of.entry(m.clone()).or_insert_with(|| target.clone());
        }
    }
    for (m, targets) in &plan.vanilla {
        if let Some(t) = targets.first() {
            mixin_of.entry(m.clone()).or_insert_with(|| t.clone());
        }
    }
    mixin_of
}

/// Whether a mixin's method is an injector's handler (Mixin's `@Inject`..., MixinExtras'), which
/// runs where it is woven rather than as a method of the target.
fn is_injector(m: &crate::class::Method) -> bool {
    m.annotations.iter().any(|a| {
        (a.ty.starts_with("Lorg/spongepowered/asm/mixin/") && (a.ty.contains("/injection/") || a.ty.contains("/gen/")))
            || a.ty.starts_with("Lcom/llamalad7/mixinextras/injector/")
    })
}

/// Whether a mixin's field only stands for its target's (`@Shadow`).
fn is_shadow_field(f: &crate::class::Field) -> bool {
    f.annotations.iter().any(|a| a.ty == "Lorg/spongepowered/asm/mixin/Shadow;")
}

/// A method a mixin adds to the game's classes (or overwrites there), as the dispatcher of its
/// `name.desc` finds it for a game object: through the mixin, its target or an interface it adds.
struct Added {
    mixin: String,
    targets: Vec<String>,
    /// Owner keys whose calls are this method's whatever the receiver.
    owners: Vec<String>,
    /// Owner keys whose calls are its on an object of one of `targets` (the game's own interfaces
    /// the mixin adds; the target itself, for an `@Overwrite`).
    checked: Vec<String>,
}

impl<'a> Program<'a> {
    /// Weave only the mixins `configs` list (the jars' mixin configurations), as Mixin does; call
    /// before emitting.
    pub fn use_mixin_configs(&mut self, configs: &[crate::MixinConfig]) {
        if configs.is_empty() {
            return;
        }
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let plan = weave::plan_with(&classes, configs);
        for (k, n) in &self.weave.unsupported {
            if let Some(v) = self.unsupported.get_mut(k) {
                *v = v.saturating_sub(*n);
            }
        }
        self.unsupported.retain(|_, v| *v > 0);
        for (k, n) in &plan.unsupported {
            *self.unsupported.entry(k.clone()).or_default() += n;
        }
        self.mixin_of = mixin_targets_of(&plan);
        self.weave = plan;
    }

    pub fn new(classes: &'a [ClassFile]) -> Program<'a> {
        let weave = weave::plan(classes);
        let mixin_of = mixin_targets_of(&weave);
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
            scan_files: Vec::new(),
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

    /// What mixins add to the game's classes ([`weave::Plan::vanilla`]), by `name.desc`: their
    /// instance methods with code, less `@Shadow`s and injectors' handlers.
    fn vanilla_methods(&self) -> BTreeMap<String, Vec<Added>> {
        let mut out: BTreeMap<String, Vec<Added>> = BTreeMap::new();
        for (mixin, targets) in &self.weave.vanilla {
            let Some(k) = self.classes.get(mixin.as_str()) else { continue };
            // The interfaces the mixin adds, with theirs: the mod's (its duck interfaces) and the game's.
            let (mut mods, mut game): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
            let mut queue = k.interfaces.clone();
            while let Some(i) = queue.pop() {
                if mods.contains(&i) || game.contains(&i) {
                    continue;
                }
                match self.classes.get(i.as_str()) {
                    Some(ik) => {
                        queue.extend(ik.interfaces.iter().cloned());
                        mods.push(i);
                    }
                    None => game.push(i),
                }
            }
            for m in &k.methods {
                if m.code.is_none() || m.access & 0x0008 != 0 || m.name.starts_with('<') || is_shadow(m) || is_injector(m) {
                    continue;
                }
                let nd = format!("{}.{}", m.name, m.desc);
                let key = |c: &str| format!("{c}.{nd}");
                let overwrite = m.annotations.iter().any(|a| a.ty == "Lorg/spongepowered/asm/mixin/Overwrite;");
                let mut owners: Vec<String> = std::iter::once(mixin).chain(&mods).map(|c| key(c)).collect();
                let mut checked: Vec<String> = game.iter().map(|c| key(c)).collect();
                for t in targets {
                    if overwrite { checked.push(key(t)) } else { owners.push(key(t)) }
                }
                out.entry(nd).or_default().push(Added { mixin: mixin.clone(), targets: targets.clone(), owners, checked });
            }
        }
        out
    }

    /// The field initialisers of a mixin into the game's class (its constructor, whose call of
    /// its superclass's is left out), as a function that runs them once on each game object the
    /// mixin's methods run on: Mixin runs them in the target's constructors, the game's own. None
    /// for a mixin without fields of its own.
    fn mixin_init(&self, mixin: &str) -> Option<(String, String)> {
        let k = self.classes.get(mixin)?;
        if !k.fields.iter().any(|f| f.access & 0x0008 == 0 && !is_shadow_field(f)) {
            return None;
        }
        let ctor = k.methods.iter().find(|m| m.name == "<init>" && m.code.is_some())?;
        let name = fn_name(mixin, "<mixin-init>", "");
        let flag = format!("$mixin {mixin}");
        let code = format!(
            "/// `{mixin}`'s field initialisers, run once on each game object its methods run on.\n\
             fn {name}(rt: &mut Rt, this: V) {{\n\
             \x20   if matches!(rt.get_field(this, {flag:?}), V::I(1)) {{ return; }}\n\
             \x20   rt.put_field(this, {flag:?}, V::I(1));\n\
             \x20   {}(rt, &[this]);\n\
             }}\n\n",
            fn_name(mixin, "<init>", &ctor.desc)
        );
        Some((name, code))
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

    /// The `@SubscribeEvent` methods whose options aren't the default, by key: `EventPriority`'s
    /// ordinal (0 `HIGHEST` to 4 `LOWEST`, 2 `NORMAL`) and `receiveCanceled`.
    fn subscriber_options(&self) -> BTreeMap<String, (u8, bool)> {
        const PRIORITIES: [&str; 5] = ["HIGHEST", "HIGH", "NORMAL", "LOW", "LOWEST"];
        let mut out = BTreeMap::new();
        for c in self.classes.values() {
            for m in c.methods.iter().filter(|m| m.code.is_some()) {
                let Some(a) = m.annotations.iter().find(|a| a.ty == SUBSCRIBE_EVENT) else { continue };
                let priority = a
                    .data
                    .get("priority")
                    .and_then(|p| p.get("v"))
                    .and_then(|v| v.as_str())
                    .and_then(|n| PRIORITIES.iter().position(|&p| p == n))
                    .map_or(2, |i| i as u8);
                let canceled = a.data.get("receiveCanceled").and_then(|v| v.get("Z")).and_then(serde_json::Value::as_i64).is_some_and(|z| z != 0);
                if (priority, canceled) != (2, false) {
                    out.insert(format!("{}.{}.{}", c.name, m.name, m.desc), (priority, canceled));
                }
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
            return if self.classes.contains_key(target.as_str()) { self.api_owner(target) } else { Some(target.clone()) };
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
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let mut files = Vec::new();
        let mut lib = format!("{LIB_HEADER}use horizon_compat::{{api, Rt, V}};\n");
        for (n, chunk) in classes.chunks(per_file.max(1)).enumerate() {
            let _ = writeln!(lib, "mod part{n};\npub use part{n}::*;");
            files.push((format!("part{n}.rs"), self.part(chunk)));
        }
        lib.push_str(&self.tables(&classes));
        files.push(("lib.rs".into(), lib));
        files
    }

    /// The module for a split engine ([`crate::split`]): the parts of each group of classes
    /// (`group(class)` below `groups`; others go to the last), each group's classes in files of
    /// about `per_file`, and the engine's `lib.rs`, which holds the tables and no part.
    pub fn emit_groups(&mut self, per_file: usize, groups: usize, group: &dyn Fn(&str) -> usize) -> (Vec<Vec<String>>, String) {
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let groups = groups.max(1);
        let mut parts = vec![Vec::new(); groups];
        for (g, list) in parts.iter_mut().enumerate() {
            let mine: Vec<&ClassFile> = classes.iter().copied().filter(|c| group(&c.name).min(groups - 1) == g).collect();
            for chunk in mine.chunks(per_file.max(1)) {
                list.push(self.part(chunk));
            }
        }
        let lib = format!("{LIB_HEADER}use horizon_compat::{{api, Rt, V}};\n{}", self.tables(&classes));
        (parts, lib)
    }

    /// One file of methods: every method of `classes` with code, and the functions they need.
    fn part(&mut self, classes: &[&ClassFile]) -> String {
        let mut part = String::from(PART_HEADER);
        for c in classes {
            for m in &c.methods {
                if let Some(code) = &m.code {
                    part.push_str(&self.method(c, &m.name, &m.desc, m.access & 0x0008 != 0, code));
                    for f in std::mem::take(&mut self.synthetic) {
                        part.push_str(&f);
                    }
                }
            }
        }
        part
    }

    /// FML's scan of each jar ([`Program::scan_files`]): its mods' ids (`,` between them), and a
    /// line per annotation on a class, field or method, its fields split by tabs: the
    /// annotation's descriptor, `TYPE`, `FIELD` or `METHOD`, the class, the member as FML names
    /// it (the class's dotted name, the field's name, the method's name and descriptor), and its
    /// values ([`crate::class::Annotation::data`]). Annotations only the compiler and tools read
    /// (nullability, Kotlin's metadata, mixins) are left out: no mod looks them up at run time.
    fn scan_texts(&self, classes: &[&ClassFile]) -> Vec<(String, String)> {
        const TOOLS_ONLY: [&str; 11] = [
            "Lorg/jetbrains/annotations/",
            "Ljavax/annotation/",
            "Lorg/jspecify/",
            "Lorg/checkerframework/",
            "Ledu/umd/cs/findbugs/",
            "Lcom/google/errorprone/",
            "Lkotlin/Metadata;",
            "Lkotlin/jvm/",
            "Lorg/spongepowered/asm/",
            "Lcom/llamalad7/mixinextras/",
            "Lnet/minecraft/MethodsReturnNonnullByDefault;",
        ];
        let kept = |a: &crate::class::Annotation| !TOOLS_ONLY.iter().any(|t| a.ty.starts_with(t)) && a.ty != "Lnet/minecraft/FieldsAreNonnullByDefault;";
        let files: Vec<(String, Option<&std::collections::HashSet<String>>)> = if self.scan_files.is_empty() {
            let mut ids: Vec<String> = self.metadata.iter().map(|m| m.0.clone()).collect();
            if ids.is_empty() {
                ids.extend(self.mod_id.clone());
            }
            vec![(ids.join(","), None)]
        } else {
            self.scan_files.iter().map(|(ids, names)| (ids.join(","), Some(names))).collect()
        };
        let mut out = Vec::new();
        for (ids, names) in files {
            let mut text = String::new();
            for c in classes.iter().filter(|c| names.is_none_or(|n| n.contains(&c.name))) {
                let mut line = |a: &crate::class::Annotation, target: &str, member: &str| {
                    let data = serde_json::Value::Object(a.data.clone());
                    let _ = writeln!(text, "{}\t{target}\t{}\t{member}\t{data}", a.ty, c.name);
                };
                for a in c.annotations.iter().filter(|a| kept(a)) {
                    line(a, "TYPE", &c.name.replace('/', "."));
                }
                for f in &c.fields {
                    for a in f.annotations.iter().filter(|a| kept(a)) {
                        line(a, "FIELD", &f.name);
                    }
                }
                for m in &c.methods {
                    for a in m.annotations.iter().filter(|a| kept(a)) {
                        line(a, "METHOD", &format!("{}{}", m.name, m.desc));
                    }
                }
            }
            out.push((ids, text));
        }
        out
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
        // The game's classes with the interfaces their mixins add (`instanceof` a duck interface).
        let mut joined: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (mixin, targets) in &self.weave.vanilla {
            let Some(k) = self.classes.get(mixin.as_str()) else { continue };
            for t in targets {
                let list = joined.entry(t.as_str()).or_default();
                for i in &k.interfaces {
                    if !list.contains(&i.as_str()) {
                        list.push(i);
                    }
                }
            }
        }
        for (target, interfaces) in joined.iter().filter(|(_, l)| !l.is_empty()) {
            let _ = writeln!(out, "    rt.interfaces.insert({target:?}, &{interfaces:?});");
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
        // Their `@SubscribeEvent(priority = ..., receiveCanceled = ...)` where it isn't the default.
        for (key, options) in self.subscriber_options() {
            let _ = writeln!(out, "    rt.subscriber_options.insert({key:?}, {options:?});");
        }
        // What FML's scan of each jar finds (`ModList.getAllScanData`).
        let scans = self.scan_texts(classes);
        for (i, (mods, _)) in scans.iter().enumerate() {
            let _ = writeln!(out, "    rt.scan_data.push(({mods:?}, SCAN_{i}));");
        }
        out.push_str("}\n\n");
        for (i, (_, text)) in scans.iter().enumerate() {
            let _ = writeln!(out, "const SCAN_{i}: &str = {text:?};\n");
        }
        // Mod loading: FML constructs each `@Mod` class with the mod bus (its classes initialise as
        // it reaches them, in Java's order), registers the `@EventBusSubscriber` classes, then fires
        // the registration events; suppliers resolve last.
        out.push_str("/// Runs the mod's registrations, as NeoForge's loading does. Call after `init`.\npub fn start(rt: &mut Rt) {\n");
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
        // A class holding registers that no constructor reached initialises now, so its entries
        // register still (the runtime registers every `DeferredRegister`'s entries).
        for c in classes {
            if c.fields.iter().any(|f| f.access & 0x0008 != 0 && f.desc.contains("neoforged/neoforge/registries/DeferredRegister")) {
                let _ = writeln!(out, "    {}", self.ensure(&c.name).replace("if rt.thrown.is_some() { return V::Void; }", &format!("rt.report_thrown({:?});", c.name)));
            }
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
        // A class's declared fields (`getDeclaredFields`, `Field.getType`): one line of
        // `access name descriptor` per field, from the table below.
        s.push_str("        (c, \"fields\") => match FIELDS.binary_search_by(|(n, _)| (*n).cmp(c)) { Ok(i) => rt.string(FIELDS[i].1), Err(_) => return None },\n");
        // What reflection sees of its annotations (`isAnnotationPresent`, `getAnnotation`).
        s.push_str("        (c, \"annotations\") => match ANNOTATIONS.binary_search_by(|(n, _)| (*n).cmp(c)) { Ok(i) => rt.string(ANNOTATIONS[i].1), Err(_) => return None },\n");
        s.push_str("        _ => return None,\n    })\n}\n\n");
        let mut fields: Vec<(&str, String)> = self
            .classes
            .values()
            .filter(|c| !c.fields.is_empty())
            .map(|c| (c.name.as_str(), c.fields.iter().map(|f| format!("{} {} {}", f.access, f.name, f.desc)).collect::<Vec<_>>().join("\n")))
            .collect();
        fields.sort();
        let _ = writeln!(s, "/// Each class's declared fields, by class name: `access name descriptor` lines.\nstatic FIELDS: [(&str, &str); {}] = [", fields.len());
        for (c, f) in &fields {
            let _ = writeln!(s, "    ({c:?}, {f:?}),");
        }
        s.push_str("];\n");
        let mut annotations: Vec<(&str, String)> = self.classes.values().filter_map(|c| Some((c.name.as_str(), annotation_lines(c)?))).collect();
        annotations.sort();
        let _ = writeln!(
            s,
            "/// Each class's annotations reflection sees, by class name: `TYPE`, `FIELD name`, `METHOD name+descriptor`\n\
             /// or an annotation type's `DEFAULT element` lines (tab-separated: target, member, type, typed values).\n\
             static ANNOTATIONS: [(&str, &str); {}] = [",
            annotations.len()
        );
        for (c, a) in &annotations {
            let _ = writeln!(s, "    ({c:?}, {a:?}),");
        }
        s.push_str("];\n");
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
            "    if let Some((iface, sam)) = rt.lambda_info.get(&recv.r()).copied() {\n",
            "        // A method its interface has code for runs as declared (a default method, though it may\n",
            "        // share the functional method's name: an overload); of the rest, the functional one is\n",
            "        // the lambda's own (by name: a bridge's descriptor differs).\n",
            "        let icid = rt.class_ids.get(iface).copied().unwrap_or(0);\n",
            "        if icid != 0 && let Some((d, o)) = rt.vkeys.get(key).copied() && o(icid) {\n",
            "            return d(rt, icid, owner_key, args);\n",
            "        }\n",
            "        if !(key.starts_with(sam) && key.as_bytes().get(sam.len()) == Some(&b'.')) {\n",
            "            return api::call(rt, owner_key, args);\n",
            "        }\n",
            "    }\n",
            "    match lambda_call(rt, cid & !horizon_compat::LAMBDA, args) {\n",
            "        Some(r) => r,\n",
            "        None => api::call(rt, owner_key, args),\n",
            "    }\n",
            "}\n\n",
            "/// Whether a mod class implements `key` (`name.desc`) itself or through a mod superclass.\n",
            "pub fn overrides(class: &str, key: &str) -> bool {\n",
            "    let Ok(c) = CLASS_IDS.binary_search_by(|(n, _)| (*n).cmp(class)) else { return false };\n",
            "    let Ok(k) = VKEYS.binary_search_by(|(n, _, _)| (*n).cmp(key)) else { return false };\n",
            "    (VKEYS[k].2)(CLASS_IDS[c].1)\n",
            "}\n\n",
        ));
        // What mixins add to the game's classes: their field initialisers, then each method's
        // place in its dispatcher.
        let added = self.vanilla_methods();
        let mut inits: BTreeMap<&str, String> = BTreeMap::new();
        for a in added.values().flatten() {
            if !inits.contains_key(a.mixin.as_str())
                && let Some((name, code)) = self.mixin_init(&a.mixin)
            {
                s.push_str(&code);
                inits.insert(&a.mixin, name);
            }
        }
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
            match added.get(nd) {
                None => {
                    let _ = writeln!(s, "        _ => vcall_miss(rt, cid, owner_key, {nd:?}, args),\n    }}\n}}");
                }
                // A game object (not a lambda, not null) takes a mixin's method through the
                // mixin, an interface it adds or its target; through the game's own interfaces (or
                // the target, for an `@Overwrite`) only an object of its target does.
                Some(list) => {
                    s.push_str("        _ => {\n            if cid & horizon_compat::LAMBDA == 0 && matches!(args.first(), Some(V::R(r)) if *r != 0) {\n");
                    for a in list {
                        let init = inits.get(a.mixin.as_str()).map(|f| format!("{f}(rt, args[0]); if rt.thrown.is_some() {{ return V::Void; }} ")).unwrap_or_default();
                        let run = format!("{}{init}return {}(rt, args);", self.ensure(&a.mixin), fn_name(&a.mixin, name, desc));
                        let pattern = |keys: &[String]| keys.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(" | ");
                        if !a.owners.is_empty() {
                            let _ = writeln!(s, "                if matches!(owner_key, {}) {{ {run} }}", pattern(&a.owners));
                        }
                        if !a.checked.is_empty() {
                            let of_target = a.targets.iter().map(|t| format!("rt.instance_of(args[0], {t:?})")).collect::<Vec<_>>().join(" || ");
                            let _ = writeln!(s, "                if matches!(owner_key, {}) && ({of_target}) {{ {run} }}", pattern(&a.checked));
                        }
                    }
                    let _ = writeln!(s, "            }}\n            vcall_miss(rt, cid, owner_key, {nd:?}, args)\n        }}\n    }}\n}}");
                }
            }
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
            // `LambdaMetafactory`'s adaptation: a primitive parameter of the implementation takes
            // the value out of the box the interface method hands it, widened. The receiver of an
            // instance method comes first.
            let kinds = format!("{}{}", if matches!(kind, 5 | 7 | 9) { "L" } else { "" }, raw_kinds(&m.desc));
            let call = if kinds.chars().any(|k| k != 'L') { format!("{{ rt.adapt_args(&mut full, {kinds:?}); {call} }}") } else { call };
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
        let collect = format!("let a = s.take::<{n}>().to_vec();");
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
            // A pattern `switch` (Java 21): the case its object matches, from the restart index on.
            ("java/lang/runtime/SwitchBootstraps", "typeSwitch" | "enumSwitch") => {
                let mut labels = Vec::new();
                for &i in &args {
                    labels.push(match c.pool.get(i as usize) {
                        Some(Const::Class(n)) => format!("horizon_compat::SwitchLabel::Class({:?})", c.utf8(*n).unwrap_or_default()),
                        Some(Const::String(u)) => format!("horizon_compat::SwitchLabel::Str({:?})", c.utf8(*u).unwrap_or_default()),
                        Some(Const::Int(v)) => format!("horizon_compat::SwitchLabel::Int({v})"),
                        _ => {
                            // An enum constant's description (a dynamic constant): matched by nothing.
                            self.unsupported("pattern switch label of a dynamic constant");
                            "horizon_compat::SwitchLabel::Class(\"\")".to_owned()
                        }
                    });
                }
                format!("let r = s.p().i(); let o = s.p(); let v = rt.type_switch(o, r, &[{}]); s.push(V::I(v)); {go}", labels.join(", "))
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
        let _ = writeln!(s, "    let mut s: horizon_compat::Stack<{}> = Default::default();", code.max_stack.max(1));
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
        // The plan found each weave's places (`Weave::locals`' offsets); an `@Inject` at
        // `INVOKE_ASSIGN` goes after the store of the call's result when one follows.
        for (wi, w) in cur.weaves.iter().enumerate() {
            // A `@ModifyVariable` at `STORE` goes after the store itself.
            if let At::Store { .. } = w.at {
                for &site in w.locals.keys() {
                    cur.after_store.entry(site).or_default().push((site, wi));
                }
                continue;
            }
            if let At::Shifted { .. } = w.at {
                for &site in w.locals.keys() {
                    let to = if w.after.contains(&site) { &mut cur.after_store } else { &mut cur.before_insn };
                    to.entry(site).or_default().push((site, wi));
                }
                continue;
            }
            if !w.at.at_instruction() {
                continue;
            }
            let assign = matches!(w.at, At::Invoke { assign: true, .. });
            for &site in w.locals.keys() {
                let k = insns.iter().position(|i| i.at == site);
                let store = k.and_then(|k| insns.get(k + 1)).filter(|x| weave::is_store(x.op));
                match store {
                    Some(st) if assign && matches!(w.kind, Kind::Inject { .. } | Kind::ModifyVariable { .. }) => cur.after_store.entry(st.at).or_default().push((site, wi)),
                    _ => cur.at_calls.entry(site).or_default().push(wi),
                }
            }
            if w.locals.is_empty() {
                self.unsupported("mixin @At(INVOKE) call not in the method");
            }
        }
        // `@Share`d refs: new for each run of the method, before anything is woven.
        for w in &cur.weaves {
            for a in w.locals.values().flatten() {
                if let weave::LocalArg::Share { name, class } = a
                    && !cur.shares.iter().any(|(m, n, _)| *m == w.mixin && n == name)
                {
                    cur.shares.push((w.mixin.clone(), name.clone(), class.clone()));
                }
            }
        }
        for (n, (_, _, class)) in cur.shares.iter().enumerate() {
            let zero = match class.rsplit('/').next().unwrap_or("") {
                "LocalLongRef" => "V::J(0)",
                "LocalFloatRef" => "V::F(0.0)",
                "LocalDoubleRef" => "V::D(0.0)",
                "LocalRef" => "V::NULL",
                _ => "V::I(0)",
            };
            let _ = writeln!(s, "    let share{n} = rt.new_object({class:?}); rt.put_field(share{n}, \"$value\", {zero});");
        }
        // The head's weaves; a whole method's wrapper goes round this body instead.
        let head: Vec<Weave> = cur.weaves.iter().filter(|w| w.at == At::Head && w.kind != Kind::WrapMethod).cloned().collect();
        self.cur = Some(cur);
        for w in &head {
            let code = match w.kind {
                Kind::ModifyVariable { .. } => self.modify_var_code(w, 0, false),
                _ => self.inject_code(w, None, 0, false),
            };
            let _ = writeln!(s, "    if pc != {THROW} {{ {code} }}");
        }
        let _ = writeln!(s, "    loop {{");
        let _ = writeln!(s, "        match pc {{");
        let joined = joined(&insns, code);
        if let Some(cur) = self.cur.as_mut() {
            cur.joined = joined.clone();
        }
        for (n, i) in insns.iter().enumerate() {
            let next = insns.get(n + 1).map_or(usize::MAX, |x| x.at);
            // A constant or a `new` some mixin weaves at (calls and field accesses weave in
            // `invoke` and `field`).
            let plain = (i.op == 0xbb || weave::constant_of(c, i).is_some()) && self.cur.as_ref().is_some_and(|cur| cur.at_calls.contains_key(&i.at));
            let mut body = if plain { self.woven_plain(c, i, next) } else { self.insn(c, i, next) };
            let weaves_at = |map: fn(&Cur) -> &BTreeMap<usize, Vec<(usize, usize)>>, cur: Option<&Cur>| -> Vec<(usize, Weave)> {
                cur.and_then(|cur| map(cur).get(&i.at).map(|v| v.iter().map(|&(site, wi)| (site, cur.weaves[wi].clone())).collect())).unwrap_or_default()
            };
            // Those before it, then those after it (the code above set `pc` to the next, or left).
            let before = weaves_at(|cur| &cur.before_insn, self.cur.as_ref());
            let after = weaves_at(|cur| &cur.after_store, self.cur.as_ref());
            let woven = |this: &Self, (site, w): &(usize, Weave)| match w.kind {
                Kind::ModifyVariable { .. } => this.modify_var_code(w, *site, true),
                _ => this.inject_code(w, None, *site, true),
            };
            if !before.is_empty() {
                let code: Vec<String> = before.iter().map(|b| woven(self, b)).collect();
                body = format!("{} {body}", code.join(" "));
            }
            for a in &after {
                body.push(' ');
                body.push_str(&woven(self, a));
            }
            // An arm per basic block: an instruction joined to the one before goes on in its arm.
            if !joined.contains(&i.at) {
                let _ = write!(s, "            {} => {{", i.at);
            }
            let _ = write!(s, " {body}");
            if !joined.contains(&next) {
                let _ = writeln!(s, " }}");
            }
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
                "pub fn {op_fn}(rt: &mut Rt, all: &[V]) -> V {{\n    let arr = all.last().copied().unwrap_or_default();\n    let mut a: Vec<V> = all.get(..{recv}).unwrap_or_default().to_vec();\n    a.extend([{}]);\n    let r = {body_name}(rt, &a);\n    {}\n}}\n",
                unboxed_items(&arg_kinds(desc)),
                boxed(return_kind(desc))
            );
            self.operations.insert(lkey, op_fn);
        }
        s
    }

    /// An instruction some mixin weaves at that is neither a call nor a field access: a constant
    /// (which an `@ModifyExpressionValue` or a `@ModifyConstant` changes as it is pushed) or a
    /// `new`; `@Inject`s and `@ModifyVariable`s go before it.
    fn woven_plain(&mut self, c: &ClassFile, i: &Insn, next: usize) -> String {
        let at = i.at;
        let site: Vec<Weave> = self
            .cur
            .as_ref()
            .and_then(|cur| cur.at_calls.get(&at).map(|ix| ix.iter().map(|&k| cur.weaves[k].clone()).collect()))
            .unwrap_or_default();
        let throw = format!("if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }}");
        let (mut before, mut modify) = (String::new(), String::new());
        let mut wrapped: Option<String> = None;
        for w in &site {
            let h = fn_name(&w.mixin, &w.handler, &w.handler_desc);
            let this = if w.handler_static { "" } else { "l[0], " };
            let (pre, locals, post) = self.local_args(w, at);
            let more: String = locals.iter().map(|a| format!(", {a}")).collect();
            match &w.kind {
                Kind::Inject { .. } => before.push_str(&self.inject_code(w, None, at, true)),
                Kind::ModifyVariable { .. } => before.push_str(&self.modify_var_code(w, at, true)),
                Kind::ModifyExpressionValue | Kind::ModifyConstant if i.op <= 0x14 => {
                    let Some(extra) = self.targs_after(w.handler_params(), 1) else {
                        self.unsupported("mixin handler of a constant taking other parameters");
                        continue;
                    };
                    let _ = write!(modify, "{pre}let v = {h}(rt, &[{this}v{extra}{more}]); {post}{throw} ");
                    continue;
                }
                // An `instanceof` (MixinExtras' `@WrapOperation` of a `@Constant(classValue)`): the
                // handler gets the object and the check as an `Operation`.
                Kind::WrapOperation if i.op == 0xc1 && wrapped.is_none() => {
                    let Some(extra) = self.targs_after(w.handler_params(), 2) else {
                        self.unsupported("mixin @WrapOperation of an instanceof taking other parameters");
                        continue;
                    };
                    let Some(cur) = self.cur.as_ref() else { continue };
                    let class = match &i.arg {
                        Operand::Int(v) => c.class_name(*v as u16).unwrap_or_default(),
                        _ => String::new(),
                    };
                    let n = self.operations.len();
                    let lkey = format!("{}.{}.{}@{at}#{n}", cur.class, cur.name, cur.desc);
                    let op_fn = format!("{}__op{at}_{n}", cur.fn_name);
                    let lid = self.lambda_id(&lkey);
                    self.synthetic.push(format!(
                        "pub fn {op_fn}(rt: &mut Rt, all: &[V]) -> V {{\n    let arr = all.last().copied().unwrap_or_default();\n    let o = rt.array_get(arr, 0);\n    let r = V::I(rt.instance_of_static(o, {class:?}) as i32);\n    {}\n}}\n",
                        boxed('Z')
                    ));
                    self.operations.insert(lkey.clone(), op_fn);
                    wrapped = Some(format!(
                        "let a = s.p(); {pre}let op = rt.lambda_id({lkey:?}, {lid}, l.to_vec()); rt.lambda_info.insert(op.r(), ({:?}, \"call\")); let r = {h}(rt, &[{this}a, op{extra}{more}]); {post}{throw} s.push(r); pc = {next};",
                        weave::OPERATION
                    ));
                    continue;
                }
                k => {
                    let what = format!("mixin {} at this instruction", k.name());
                    self.unsupported(&what);
                    continue;
                }
            }
            before.push(' ');
        }
        if let Some(w) = wrapped {
            return format!("{before}{w}");
        }
        let body = self.insn(c, i, next);
        if modify.is_empty() {
            return format!("{before}{body}");
        }
        // Pushed as ever, then through the handlers.
        format!("{before}{body} let v = s.p(); {modify}s.push(v);")
    }

    /// What a handler's local parameters get at place `site` (`Weave::locals`): code that makes
    /// the refs before its call, the arguments, and code that puts what the refs hold back into
    /// the locals after it.
    fn local_args(&self, w: &Weave, site: usize) -> (String, Vec<String>, String) {
        let (mut pre, mut args, mut post) = (String::new(), Vec::new(), String::new());
        let shares = self.cur.as_ref().map(|c| c.shares.as_slice()).unwrap_or_default();
        for (k, a) in w.locals.get(&site).into_iter().flatten().enumerate() {
            match a {
                weave::LocalArg::Value(slot) => args.push(format!("l[{slot}]")),
                weave::LocalArg::Ref { slot, class } => {
                    let _ = write!(pre, "let lr{k} = rt.new_object({class:?}); rt.put_field(lr{k}, \"$value\", l[{slot}]); ");
                    args.push(format!("lr{k}"));
                    let _ = write!(post, "l[{slot}] = rt.get_field(lr{k}, \"$value\"); ");
                }
                weave::LocalArg::Share { name, .. } => {
                    let n = shares.iter().position(|(m, s, _)| *m == w.mixin && s == name).unwrap_or(0);
                    args.push(format!("share{n}"));
                }
            }
        }
        (pre, args, post)
    }

    /// A `@ModifyVariable` at place `site`: its local through the handler (with the method's
    /// arguments when it takes them, and its own locals) and back into the slot.
    fn modify_var_code(&self, w: &Weave, site: usize, in_loop: bool) -> String {
        let Some(slot) = w.var.get(&site) else { return String::new() };
        let this = if w.handler_static { "" } else { "l[0], " };
        let targs: String = self.targs_after(w.handler_params(), 1).unwrap_or_default();
        let (pre, locals, post) = self.local_args(w, site);
        let locals: String = locals.iter().map(|a| format!(", {a}")).collect();
        let throw = if in_loop { format!("thrown_at = {site}; pc = {THROW}; continue;") } else { format!("thrown_at = usize::MAX; pc = {THROW};") };
        format!(
            "{{ {pre}let v = {}(rt, &[{this}l[{slot}]{targs}{locals}]); {post}if rt.thrown.is_some() {{ {throw} }} l[{slot}] = v; }}",
            fn_name(&w.mixin, &w.handler, &w.handler_desc)
        )
    }

    /// The target method's first arguments a handler with `np` parameters takes after its `base`
    /// own (Mixin's `captureTargetArgs`: as many as it has room for), joined each after a `, `; `None`
    /// when it takes more than the method has.
    fn targs_after(&self, np: usize, base: usize) -> Option<String> {
        let targs = self.target_args();
        let n = np.checked_sub(base)?;
        (n <= targs.len()).then(|| targs[..n].iter().map(|a| format!(", {a}")).collect())
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
        let (pre, locals, post) = self.local_args(w, at);
        list.extend(locals);
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
            "{{ let ci = rt.new_object({class:?}); let id = rt.string({:?}); api::call(rt, {init:?}, &[ci, id, V::I({}){value_arg}]); {pre}{}(rt, &[{}]); {post}if rt.thrown.is_some() {{ {throw} }}{cancel} }}",
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
        let here: Vec<Weave> = cur.weaves.iter().filter(|w| matches!(w.at, At::Return { .. } | At::Tail) && w.locals.contains_key(&at)).cloned().collect();
        if here.is_empty() {
            return None;
        }
        let mut code = String::new();
        for w in &here {
            match w.kind {
                Kind::ModifyReturnValue => {
                    let this = if w.handler_static { "" } else { "l[0], " };
                    let extra = self.targs_after(w.handler_params(), 1).unwrap_or_default();
                    let (pre, locals, post) = self.local_args(w, at);
                    let locals: String = locals.iter().map(|a| format!(", {a}")).collect();
                    let _ = write!(
                        code,
                        "{pre}ret = {}(rt, &[{this}ret{extra}{locals}]); {post}if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }} ",
                        fn_name(&w.mixin, &w.handler, &w.handler_desc)
                    );
                }
                Kind::Inject { .. } => {
                    code.push_str(&self.inject_code(w, Some("ret"), at, true));
                    code.push(' ');
                }
                Kind::ModifyVariable { .. } => {
                    code.push_str(&self.modify_var_code(w, at, true));
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
        // Into the next instruction: it follows in the same arm unless something jumps there.
        let go = if self.cur.as_ref().is_some_and(|cur| cur.joined.contains(&next)) { String::new() } else { format!("pc = {next};") };
        let idx = match &i.arg {
            Operand::Int(v) => *v,
            Operand::IndexCount(v, _) => *v as i32,
            _ => 0,
        };
        let pop = "s.p()";
        let bin = |ty: &str, wrap: &str, expr: &str| {
            format!("let b = {pop}.{ty}(); let a = {pop}.{ty}(); s.push(V::{wrap}({expr})); {go}")
        };
        let cmp_jump = |cond: &str| match &i.arg {
            Operand::Jump(t) => format!("if {cond} {{ pc = {t}; continue; }} else {{ {go} }}"),
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
                    format!("let v = rt.class_literal({name:?}); s.push(v); {go}")
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
            0x57 => format!("s.p(); {go}"),
            0x58 => format!("if matches!(s.p(), V::J(_) | V::D(_)) {{}} else {{ s.p(); }} {go}"),
            0x59 => format!("let v = s.top(); s.push(v); {go}"),
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
            0x6c => format!("let b = {pop}.i(); let a = {pop}.i(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; continue; }} else {{ s.push(V::I(horizon_compat::idiv(a, b))); {go} }}", at = i.at),
            0x6d => format!("let b = {pop}.j(); let a = {pop}.j(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; continue; }} else {{ s.push(V::J(horizon_compat::ldiv(a, b))); {go} }}", at = i.at),
            0x6e => bin("f", "F", "a / b"),
            0x6f => bin("d", "D", "a / b"),
            0x70 => format!("let b = {pop}.i(); let a = {pop}.i(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; continue; }} else {{ s.push(V::I(horizon_compat::irem(a, b))); {go} }}", at = i.at),
            0x71 => format!("let b = {pop}.j(); let a = {pop}.j(); if b == 0 {{ rt.throw_new(\"java/lang/ArithmeticException\"); thrown_at = {at}; pc = {THROW}; continue; }} else {{ s.push(V::J(horizon_compat::lrem(a, b))); {go} }}", at = i.at),
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
                Operand::Jump(t) => format!("pc = {t}; continue;"),
                _ => go,
            },
            0xaa | 0xab => match &i.arg {
                Operand::Switch(default, pairs) => {
                    let mut m = format!("let k = {pop}.i(); pc = match k {{ ");
                    for (k, t) in pairs {
                        let _ = write!(m, "{k} => {t}, ");
                    }
                    let _ = write!(m, "_ => {default} }}; continue;");
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
            0xb2..=0xb5 => self.field(c, i.op, idx as u16, &go, i.at),
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
            0xbf => format!("let e = {pop}; rt.thrown = Some(e); thrown_at = {}; pc = {THROW}; continue;", i.at),
            0xc0 => go, // checkcast: translated code trusts the mod's types
            0xc1 => {
                let class = c.class_name(idx as u16).unwrap_or_default();
                format!("let a = {pop}; s.push(V::I(rt.instance_of_static(a, {class:?}) as i32)); {go}")
            }
            0xc2 | 0xc3 => format!("s.p(); {go}"),
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
                    format!("let d: Vec<i32> = s.take::<{dims}>().iter().map(|v| v.i()).collect(); let v = rt.new_multi(&d, {zero}); s.push(v); {go}")
                }
                _ => go,
            },
            0xc6 => format!("let a = {pop}; {}", cmp_jump("a.is_null()")),
            0xc7 => format!("let a = {pop}; {}", cmp_jump("!a.is_null()")),
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
            let mixin_has = self.classes.get(owner).is_some_and(|k| k.fields.iter().any(|f| f.name == name && !is_shadow_field(f)));
            return if mixin_has && !target_has { owner.to_owned() } else { found };
        }
        let mut c = owner;
        // The first of the API's interfaces the chain implements: where the JVM finds a constant the
        // mod's classes don't declare (`RecipeSerializer.SHAPED_RECIPE` through a mod's serializer)
        // when the chain then reaches Java's own classes, which declare none.
        let mut api_interface: Option<String> = None;
        loop {
            let Some(k) = self.classes.get(c) else {
                return match api_interface {
                    Some(i) if c.starts_with("java/") => i,
                    _ => c.to_owned(),
                };
            };
            if k.fields.iter().any(|f| f.name == name) {
                return k.name.clone();
            }
            if let Some(i) = self.interface_field(k, name) {
                return i;
            }
            if api_interface.is_none() {
                api_interface = self.api_interface(k);
            }
            match k.superclass.as_deref() {
                Some(sup) => c = sup,
                None => return owner.to_owned(),
            }
        }
    }

    /// The first interface outside the mod's classes that `k` implements, itself or through one of
    /// the mod's interfaces, in the order the JVM searches them.
    fn api_interface(&self, k: &ClassFile) -> Option<String> {
        k.interfaces.iter().find_map(|i| match self.classes.get(i.as_str()) {
            None => Some(i.clone()),
            Some(ik) => self.api_interface(ik),
        })
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

    fn field(&mut self, c: &ClassFile, op: u8, idx: u16, go: &str, at: usize) -> String {
        let Some(m) = c.member(idx) else { return go.to_owned() };
        if self.cur.as_ref().is_some_and(|cur| cur.at_calls.contains_key(&at)) {
            return self.woven_field(c, op, &m, go, at);
        }
        let owner = if matches!(op, 0xb2 | 0xb3) { self.field_owner(&m.owner, &m.name) } else { m.owner.clone() };
        // The declaring class initialises, as the JVM does (not the subclass the reference names).
        let init = if matches!(op, 0xb2 | 0xb3) && owner != c.name { self.ensure(&owner) } else { String::new() };
        let pop = "s.p()";
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
        let game_read = if op == 0xb4 { self.game_field_read(&m, "o") } else { None };
        match op {
            0xb2 if self.own(&owner) => match zero {
                Some(z) => format!("{init}let v = match rt.statics.get({skey:?}).copied() {{ None | Some(V::Void) => {z}, Some(v) => v }}; s.push(v); {go}"),
                None => format!("{init}let v = rt.statics.get({skey:?}).copied().unwrap_or_default(); s.push(v); {go}"),
            },
            0xb2 => format!("let v = api::get_static(rt, {skey:?}); s.push(v); {go}"),
            0xb3 => format!("{init}let v = {pop}; rt.statics.insert({skey:?}, v); {go}"),
            // A field a game class declares (an access transformer opens it): the runtime keeps
            // its state, or answers for it.
            0xb4 if game_read.is_some() => format!("let o = {pop}; s.push({}); {go}", game_read.unwrap_or_default()),
            0xb4 => match zero {
                Some(z) => format!("let o = {pop}; s.push(match rt.get_field(o, {:?}) {{ V::Void => {z}, v => v }}); {go}", m.name),
                None => format!("let o = {pop}; s.push(rt.get_field(o, {:?})); {go}", m.name),
            },
            _ => format!("let v = {pop}; let o = {pop}; rt.put_field(o, {:?}, v); {go}", m.name),
        }
    }

    /// Reading instance field `m` of object `o` when a game class declares it (no translated class
    /// on the way up does): `api::game_field`, the runtime's state or answer for it, then the
    /// type's zero for a primitive nothing answers. None for a field of the mod's own.
    fn game_field_read(&self, m: &Member, o: &str) -> Option<String> {
        let owner = self.field_owner(&m.owner, &m.name);
        if self.own(&owner) {
            return None;
        }
        let zero = match m.desc.as_str() {
            "I" | "Z" | "B" | "S" | "C" => "V::I(0)",
            "J" => "V::J(0)",
            "F" => "V::F(0.0)",
            "D" => "V::D(0.0)",
            _ => "V::Void",
        };
        let (name, own, key) = (&m.name, format!("${}", m.name), format!("{owner}.${}.(){}", m.name, m.desc));
        Some(format!("match api::game_field(rt, {o}, {name:?}, {own:?}, {key:?}) {{ V::Void => {zero}, v => v }}"))
    }

    /// A field access some mixin weaves at (`@At("FIELD")`): made as a call would be, of a method
    /// `()T` for a read and `(T)V` for a write (static or on the popped object), so that a
    /// `@Redirect`, `@ModifyExpressionValue`, `@WrapOperation` or `@Inject` goes round it as
    /// round a call.
    fn woven_field(&mut self, c: &ClassFile, op: u8, m: &Member, go: &str, at: usize) -> String {
        let is_static = matches!(op, 0xb2 | 0xb3);
        let owner = if is_static { self.field_owner(&m.owner, &m.name) } else { m.owner.clone() };
        let init = if is_static && owner != c.name { self.ensure(&owner) } else { String::new() };
        let skey = format!("{owner}.{}", m.name);
        let zero = match m.desc.as_str() {
            "I" | "Z" | "B" | "S" | "C" => "V::I(0)",
            "J" => "V::J(0)",
            "F" => "V::F(0.0)",
            "D" => "V::D(0.0)",
            _ => "V::NULL",
        };
        let read = matches!(op, 0xb2 | 0xb4);
        let n = usize::from(!is_static) + usize::from(!read);
        let call = match op {
            0xb2 if self.own(&owner) => format!("match rt.statics.get({skey:?}).copied() {{ None | Some(V::Void) => {zero}, Some(v) => v }}"),
            0xb2 => format!("api::get_static(rt, {skey:?})"),
            0xb3 => format!("{{ rt.statics.insert({skey:?}, args[0]); V::Void }}"),
            0xb4 => format!("match rt.get_field(args[0], {:?}) {{ V::Void => {zero}, v => v }}", m.name),
            _ => format!("{{ rt.put_field(args[0], {:?}, args[1]); V::Void }}", m.name),
        };
        let as_call = Member { owner: m.owner.clone(), name: m.name.clone(), desc: if read { format!("(){}", m.desc) } else { format!("({})V", m.desc) } };
        let woven = self.call_weaves(&as_call, if is_static { 0xb8 } else { 0xb6 }, at, call);
        let mutable = if woven.mutable { " let mut args = args;" } else { "" };
        let push = if read { "s.push(r);" } else { "" };
        format!(
            "{init}let args = s.take::<{n}>();{mutable} {}let r = {}; if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }} else {{ {}{push} {}{go} }}",
            woven.before, woven.call, woven.modify, woven.after
        )
    }

    /// A call to a mixin's accessor or invoker: `@Accessor("pattern")` or `@Invoker` in an interface
    /// that is `@Mixin(ShapedRecipe.class)`. The mixin would add the method to the game's class; here
    /// the call reads or writes the game object's field, or calls the game's method, directly.
    fn mixin_call(&mut self, m: &Member, op: u8) -> Option<String> {
        const ACCESSOR: &str = "Lorg/spongepowered/asm/mixin/gen/Accessor;";
        const INVOKER: &str = "Lorg/spongepowered/asm/mixin/gen/Invoker;";
        let class: &ClassFile = self.classes.get(m.owner.as_str())?;
        let target = class.mixin_target()?;
        // A static accessor or invoker has a body (javac needs one: `throw new AssertionError()`),
        // which Mixin replaces as it does an interface's abstract one.
        let method = class.methods.iter().find(|x| x.name == m.name && x.desc == m.desc && x.annotations.iter().any(|a| a.ty == ACCESSOR || a.ty == INVOKER))?;
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
                // Neither (the runtime models the object otherwise: a `VoxelShape`'s boxes): the
                // runtime answers it as an API call named after the field, `VoxelShape.$shape`.
                (false, false) => format!(
                    "match rt.get_field(args[0], {own:?}) {{ V::Void => match rt.get_field(args[0], {field:?}) {{ V::Void => api::call(rt, {key:?}, &[args[0]]), v => v }}, v => v }}",
                    own = format!("${field}"),
                    key = format!("{target}.${field}.{}", m.desc)
                ),
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
            // A constructor's invoker: static, returning the new object (a translated class's
            // made by its own constructor, as `new` makes it).
            let desc = format!("{}V", &m.desc[..=m.desc.find(')')?]);
            if self.own(&target)
                && let Some(imp) = self.resolve(&target, "<init>", &desc)
            {
                return Some(format!(
                    "{{ {}let o = rt.new_object_id({target:?}, {}); let mut a = vec![o]; a.extend_from_slice(&args); {}(rt, &a); o }}",
                    self.ensure(&target),
                    self.cid(&target),
                    fn_name(&imp, "<init>", &desc)
                ));
            }
            let init = format!("{target}.<init>.{desc}");
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
            return format!("s.drop_n({n}); {go}");
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
        let collect = format!("{init}{caller}let args = s.take::<{n}>();");
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
            "{collect} {}let r = {}; if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }} else {{ {}{push} {initialisers}{}{go} }}",
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
        let throw = format!("if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; continue; }}");
        let mut wrapped = false;
        for w in &site {
            let h = fn_name(&w.mixin, &w.handler, &w.handler_desc);
            let this = if w.handler_static { "" } else { "l[0], " };
            let np = w.handler_params();
            // The handler's `@Local`/`@Share` arguments, last, around each call of it.
            let (pre, locals, post) = self.local_args(w, at);
            let more: String = locals.iter().map(|a| format!(", {a}")).collect();
            let more_vec = if locals.is_empty() { String::new() } else { format!(" h.extend_from_slice(&[{}]);", locals.join(", ")) };
            match (&w.kind, &w.at) {
                (Kind::ModifyVariable { .. }, At::Invoke { after, .. } | At::Field { after, .. }) => {
                    let code = self.modify_var_code(w, at, true);
                    let to = if *after { &mut out.after } else { &mut out.before };
                    to.push_str(&code);
                    to.push(' ');
                }
                // The receiver through the handler, which also sees the arguments.
                (Kind::ModifyReceiver, _) if recv == 1 && self.targs_after(np, 1 + params.len()).is_some() => {
                    let extra = self.targs_after(np, 1 + params.len()).unwrap_or_default();
                    out.mutable = true;
                    let _ = write!(out.before, "{{ {pre}let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); h.extend_from_slice(&[{}]);{more_vec} let v = {h}(rt, &h); {post}{throw} args[0] = v; }} ", extra.trim_start_matches(", "));
                }
                (Kind::Inject { .. }, At::Invoke { after, .. } | At::Field { after, .. }) => {
                    let code = self.inject_code(w, None, at, true);
                    let to = if *after { &mut out.after } else { &mut out.before };
                    to.push_str(&code);
                    to.push(' ');
                }
                // The handler instead of the call: the receiver and arguments, then the target
                // method's when it takes them.
                (Kind::Redirect, _) => {
                    let Some(extra) = self.targs_after(np, recv + params.len()) else {
                        self.unsupported("mixin @Redirect taking other parameters");
                        continue;
                    };
                    let extra = extra.trim_start_matches(", ");
                    out.call = format!("{{ {pre}let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); h.extend_from_slice(&[{extra}]);{more_vec} let r = {h}(rt, &h); {post}r }}");
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
                        let _ = write!(out.before, "{{ {pre}let v = {h}(rt, &[{this}args[{i}]{more}]); {post}{throw} args[{i}] = v; }} ");
                    } else if np == params.len() {
                        let _ = write!(out.before, "{{ {pre}let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args[{recv}..]);{more_vec} let v = {h}(rt, &h); {post}{throw} args[{i}] = v; }} ");
                    } else {
                        self.unsupported("mixin @ModifyArg taking other parameters");
                    }
                }
                // The handler instead of the call, given the call as an `Operation` that makes it
                // (with the locals as they are, for what other weaves at the call read).
                (Kind::WrapOperation, At::Invoke { .. } | At::Field { .. }) => {
                    let Some(extra) = self.targs_after(np, recv + params.len() + 1) else {
                        self.unsupported("mixin @WrapOperation taking other parameters");
                        continue;
                    };
                    let extra = extra.trim_start_matches(", ").to_owned();
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
                        "pub fn {op_fn}(rt: &mut Rt, all: &[V]) -> V {{\n    let (l, rest) = all.split_at(all.len().saturating_sub(1));\n    let arr = rest.first().copied().unwrap_or_default();\n    let args: Vec<V> = vec![{}];\n    let r = {};\n    {}\n}}\n",
                        unboxed_items(&kinds),
                        out.call,
                        boxed(return_kind(&m.desc))
                    ));
                    self.operations.insert(lkey.clone(), op_fn);
                    out.call = format!(
                        "{{ {pre}let op = rt.lambda_id({lkey:?}, {lid}, l.to_vec()); rt.lambda_info.insert(op.r(), ({:?}, \"call\")); let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); h.push(op); h.extend_from_slice(&[{extra}]);{more_vec} let r = {h}(rt, &h); {post}r }}",
                        weave::OPERATION
                    );
                }
                (Kind::ModifyExpressionValue, _) if returns && self.targs_after(np, 1).is_some() => {
                    let extra = self.targs_after(np, 1).unwrap_or_default();
                    let _ = write!(out.modify, "{pre}let r = {h}(rt, &[{this}r{extra}{more}]); {post}{throw} ");
                }
                // The call only when the handler, given its receiver and arguments, says so.
                (Kind::WrapWithCondition, _) if !returns && self.targs_after(np, recv + params.len()).is_some() => {
                    if !wrapped {
                        out.before.push_str("let mut wrap_ok = true; ");
                        wrapped = true;
                    }
                    let extra = self.targs_after(np, recv + params.len()).unwrap_or_default();
                    let _ = write!(out.before, "if wrap_ok {{ {pre}let mut h: Vec<V> = vec![{this}]; h.extend_from_slice(&args); h.extend_from_slice(&[{}]);{more_vec} wrap_ok = {h}(rt, &h).i() != 0; {post}{throw} }} ", extra.trim_start_matches(", "));
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

#[cfg(test)]
mod scan_tests {
    use crate::class::ClassFile;
    use std::process::Command;

    /// FML's scan as the translator writes it: a line per annotation javac kept (visible and
    /// class-file-only), each value with its type.
    #[test]
    fn the_scan_keeps_what_fml_would_find() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("mod-translator-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let src = r#"
package scan;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME) @interface Plugin { String value(); }
@Retention(RetentionPolicy.CLASS) @interface Sync { String tag() default ""; ElementType kind(); Class<?> type(); int size(); String[] names(); }
@Plugin("mymod")
public class Scanned {
    @Sync(tag = "e", kind = ElementType.FIELD, type = Long.class, size = 3, names = {"a", "b"}) long energy;
    @Deprecated public void old(int x) {}
}
"#;
        std::fs::write(root.join("Scanned.java"), src).unwrap();
        assert!(Command::new("javac").args(["-d", root.to_str().unwrap()]).arg(root.join("Scanned.java")).status().unwrap().success());
        let class = ClassFile::parse(&std::fs::read(root.join("scan/Scanned.class")).unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let classes = vec![class];
        let mut p = super::Program::new(&classes);
        p.mod_id = Some("mymod".into());
        let refs: Vec<&ClassFile> = classes.iter().collect();
        let scans = p.scan_texts(&refs);
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].0, "mymod");
        let lines: Vec<&str> = scans[0].1.lines().collect();
        assert_eq!(
            lines,
            [
                "Lscan/Plugin;\tTYPE\tscan/Scanned\tscan.Scanned\t{\"value\":\"mymod\"}",
                "Lscan/Sync;\tFIELD\tscan/Scanned\tenergy\t{\"kind\":{\"e\":\"Ljava/lang/annotation/ElementType;\",\"v\":\"FIELD\"},\"names\":[\"a\",\"b\"],\"size\":{\"I\":3},\"tag\":\"e\",\"type\":{\"c\":\"Ljava/lang/Long;\"}}",
                "Ljava/lang/Deprecated;\tMETHOD\tscan/Scanned\told(I)V\t{}",
            ]
        );
    }
}

#[cfg(test)]
mod annotation_tests {
    use crate::class::ClassFile;
    use std::process::Command;

    /// What reflection sees, as the bundle's table lists it: the `RUNTIME` annotations only (javac
    /// keeps `CLASS` ones in the class file too, where reflection can't see them), and an
    /// annotation type's element defaults.
    #[test]
    fn reflection_sees_runtime_annotations_and_element_defaults() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("mod-translator-annotations-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let src = r#"
package refl;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME) @interface Group { String value() default "misc"; int order() default 3; ElementType[] kinds() default {ElementType.FIELD}; }
@Retention(RetentionPolicy.CLASS) @interface Hidden {}
public class Content {
    @Group("blocks") @Hidden public static int STONE;
    @Hidden public void make() {}
}
"#;
        std::fs::write(root.join("Content.java"), src).unwrap();
        assert!(Command::new("javac").args(["-d", root.to_str().unwrap()]).arg(root.join("Content.java")).status().unwrap().success());
        let read = |name: &str| ClassFile::parse(&std::fs::read(root.join(format!("refl/{name}.class"))).unwrap()).unwrap();
        let (content, group) = (read("Content"), read("Group"));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(super::annotation_lines(&content).as_deref(), Some("FIELD\tSTONE\tLrefl/Group;\t{\"value\":\"blocks\"}\n"));
        let lines = super::annotation_lines(&group).unwrap();
        let lines: Vec<&str> = lines.lines().collect();
        assert_eq!(
            lines,
            [
                "TYPE\t\tLjava/lang/annotation/Retention;\t{\"value\":{\"e\":\"Ljava/lang/annotation/RetentionPolicy;\",\"v\":\"RUNTIME\"}}",
                "DEFAULT\tvalue\t\t\"misc\"",
                "DEFAULT\torder\t\t{\"I\":3}",
                "DEFAULT\tkinds\t\t[{\"e\":\"Ljava/lang/annotation/ElementType;\",\"v\":\"FIELD\"}]",
            ]
        );
    }
}

#[cfg(test)]
mod field_tests {
    use crate::class::ClassFile;
    use std::process::Command;

    /// A static field the mod's classes reach without declaring it resolves as the JVM resolves it
    /// (JVMS 5.4.3.2): a constant of an API interface the chain implements (javac writes
    /// `getstatic Child.SHAPED` for `RecipeSerializer.SHAPED_RECIPE` through a mod's serializer)
    /// is the interface's, since Java's own classes the chain then reaches declare none; past an
    /// API superclass, that class's.
    #[test]
    fn an_inherited_constant_is_the_api_interfaces_the_chain_implements() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("mod-translator-fields-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("api")).unwrap();
        std::fs::create_dir_all(root.join("m")).unwrap();
        let files = [
            ("api/Serializer.java", "package api; public interface Serializer { Object SHAPED = new Object(); }"),
            ("api/Item.java", "package api; public class Item { public static final Object ID = new Object(); }"),
            ("m/Base.java", "package m; public class Base implements api.Serializer {}"),
            ("m/Child.java", "package m; public class Child extends Base { Object get() { return SHAPED; } }"),
            ("m/MyItem.java", "package m; public class MyItem extends api.Item implements api.Serializer { Object get() { return ID; } }"),
        ];
        for (path, src) in files {
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(Command::new("javac").args(["-d", root.to_str().unwrap()]).args(&paths).status().unwrap().success());
        // The API's classes aren't the mod's: only `m`'s are given.
        let classes: Vec<ClassFile> = ["m/Base.class", "m/Child.class", "m/MyItem.class"]
            .iter()
            .map(|c| ClassFile::parse(&std::fs::read(root.join(c)).unwrap()).unwrap())
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        let p = super::Program::new(&classes);
        assert_eq!(p.field_owner("m/Child", "SHAPED"), "api/Serializer");
        assert_eq!(p.field_owner("m/MyItem", "ID"), "api/Item");
    }
}
