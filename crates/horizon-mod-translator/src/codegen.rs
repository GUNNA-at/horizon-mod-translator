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
    collections::{BTreeMap, BTreeSet, HashSet},
    fmt::Write,
};

/// The pseudo-instruction offset of a method's unwind block.
const THROW: usize = usize::MAX - 1;
/// What a piece of a long block leaves in `pc` when it returned from the method, and when it ran to
/// its end ([`cut`]).
const LEFT: usize = usize::MAX - 2;
const FELL: usize = usize::MAX - 3;
/// A basic block of more text than this is cut into functions of its own: one initialiser of
/// keyframes was a block of 3 MB, and the compiler took 7 GB for a crate of those.
const LONG_BLOCK: usize = 64 << 10;
/// About how much of a long block each of its functions holds.
const BLOCK_PIECE: usize = 24 << 10;
/// A method of this much bytecode or more is cut into functions of several blocks each
/// ([`ranges`]), whatever its blocks' lengths: `Ignis_Model.animate` came out as one function of
/// 1.8 MB over 92 blocks, none of them long, and the compiler was longer over its crate alone
/// than over the pack's other 560 together.
const BIG_METHOD: usize = 4 << 10;
/// About how much text of a big method each of its functions holds.
const RANGE_TEXT: usize = 32 << 10;
/// A part's classes hold about this much bytecode at most, so that a part fits a crate
/// ([`crate::split`]): dense code translates to some fifty times its size, and forty classes of
/// keyframes made a part of 26 MB.
const PART_CODE: usize = 160 << 10;

/// `classes` in runs of at most `count` classes and about [`PART_CODE`] of bytecode: a part each.
fn parts_of<'a>(classes: &[&'a ClassFile], count: usize) -> Vec<Vec<&'a ClassFile>> {
    let mut parts: Vec<Vec<&ClassFile>> = Vec::new();
    let mut size = 0;
    for c in classes {
        let code: usize = c
            .methods
            .iter()
            .filter_map(|m| m.code.as_ref())
            .map(|code| code.bytes.len())
            .sum();
        match parts.last_mut() {
            Some(last) if last.len() < count.max(1) && size + code <= PART_CODE => last.push(c),
            _ => {
                parts.push(vec![c]);
                size = 0;
            }
        }
        size += code;
    }
    parts
}

/// A long basic block (`bodies`, its instructions' code) of the method `f`, as calls of functions
/// that each hold a piece of it, written to `out`. A piece runs once: where its code went on to
/// another block (`continue`) it leaves that block in `at`, where it returned, [`LEFT`].
fn cut(
    f: &str,
    block: usize,
    bodies: &[String],
    piece: usize,
    (locals, stack): (usize, usize),
    held: bool,
    out: &mut String,
) -> String {
    // Called from the method itself, which owns its locals and stack, or from a function of a
    // big method's ([`ranges`]), which already holds them by reference.
    let frame = if held { "l, s" } else { "&mut l, &mut s" };
    let mut arm = String::from(" let mut at = (0usize, thrown_at);");
    let mut pieces: Vec<String> = vec![String::new()];
    for b in bodies {
        if pieces.last().is_some_and(|p| p.len() >= piece) {
            pieces.push(String::new());
        }
        let last = pieces.last_mut().expect("starts with one");
        last.push(' ');
        last.push_str(b);
    }
    for (k, piece) in pieces.iter().enumerate() {
        let name = format!("{f}__b{block}_{k}");
        let _ = writeln!(
            out,
            "#[inline(never)]\nfn {name}(rt: &mut Rt, args: &[V], l: &mut [V; {locals}], s: &mut horizon_compat::Stack<{stack}>, at: &mut (usize, usize)) -> V {{\n    let (mut pc, mut thrown_at): (usize, usize) = ({FELL}, at.1);\n    let mut once = true;\n    loop {{\n        if !once {{ break; }}\n        once = false;\n       {piece}\n        break;\n    }}\n    *at = (pc, thrown_at);\n    V::Void\n}}\n"
        );
        let _ = write!(
            arm,
            " at.0 = {LEFT}; let r = {name}(rt, args, {frame}, &mut at); if at.0 == {LEFT} {{ return r; }} thrown_at = at.1; if at.0 != {FELL} {{ pc = at.0; continue; }}"
        );
    }
    arm
}

/// The arms of a big method `f` (`arms`: each block's start and its arm of the method's `match`),
/// as arms that each call a function holding several of the blocks, written to `out`. A function
/// runs the blocks of its range from the one `at` names, going round among them, until the code
/// goes to a block outside the range, which it leaves in `at`, or returns ([`LEFT`] stays there).
fn ranges(
    f: &str,
    arms: &[(usize, String)],
    text: usize,
    (locals, stack): (usize, usize),
    out: &mut String,
) -> String {
    let (mut main, mut from, mut k) = (String::new(), 0, 0);
    while from < arms.len() {
        let (mut to, mut size) = (from, 0);
        while to < arms.len() && (to == from || size + arms[to].1.len() <= text) {
            size += arms[to].1.len();
            to += 1;
        }
        let name = format!("{f}__r{k}");
        let _ = writeln!(
            out,
            "#[inline(never)]\nfn {name}(rt: &mut Rt, args: &[V], l: &mut [V; {locals}], s: &mut horizon_compat::Stack<{stack}>, at: &mut (usize, usize)) -> V {{\n    let (mut pc, mut thrown_at): (usize, usize) = *at;\n    at.0 = {LEFT};\n    loop {{\n        match pc {{"
        );
        for (_, arm) in &arms[from..to] {
            out.push_str(arm);
        }
        let _ = writeln!(
            out,
            "            _ => break,\n        }}\n    }}\n    *at = (pc, thrown_at);\n    V::Void\n}}\n"
        );
        let _ = writeln!(
            main,
            "            {}..={} => {{ let mut at = (pc, thrown_at); let r = {name}(rt, args, &mut l, &mut s, &mut at); if at.0 == {LEFT} {{ return r; }} pc = at.0; thrown_at = at.1; }}",
            arms[from].0,
            arms[to - 1].0
        );
        from = to;
        k += 1;
    }
    main
}

/// An empty class of the name of one of the bundle's tables ([`horizon_compat::FILES`],
/// [`horizon_compat::MEMBERS`]): given with the program's classes, it gets an id and a
/// definition like any, and its `reflect` function answers from the table.
pub fn table_class(name: &str) -> ClassFile {
    ClassFile {
        pool: Vec::new(),
        access: 0x0011,
        name: name.to_owned(),
        superclass: Some("java/lang/Object".to_owned()),
        interfaces: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
        bootstrap: Vec::new(),
        annotations: Vec::new(),
        signature: None,
    }
}

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

/// The interfaces a mixin adds to its targets: an interface mixin (an accessor, an invoker) is one
/// itself, and a mixin class adds the interfaces it implements.
fn mixin_adds(mixin: &ClassFile) -> Vec<&str> {
    let own = (mixin.access & 0x0200 != 0).then_some(mixin.name.as_str());
    own.into_iter().chain(mixin.interfaces.iter().map(String::as_str)).collect()
}

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

/// The dispatcher of instance method `nd` (`name.desc`): `fn(rt, cid, owner_key, args) -> V`, the
/// runtime's dispatch for that key ([`dispatcher_shim`]).
pub(crate) fn dispatcher(nd: &str) -> String {
    format!("d_{}_{:08x}", ident(nd.split('.').next().unwrap_or("")), hash(nd))
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
pub(crate) const PART_HEADER: &str = "#![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, unused_imports, clippy::all)]\nuse crate::*;\nuse horizon_compat::{api, Rt, V};\nuse horizon_compat::linked::{Added, ClassDef, Registry};\n\n";
/// The game fields read or written through a mapped method that does only that
/// (`derive::Deriver::accessors`): by the class a reference names, the field and whether it is a
/// write, the method's key. Set before a bundle is written with the game's own code.
static ACCESSORS: std::sync::RwLock<BTreeMap<(String, String, bool), String>> =
    std::sync::RwLock::new(BTreeMap::new());

/// The methods a class takes from the game's class above it over an interface's default of the
/// same name (`derive::inherited_over_defaults`): by class, each one's name and descriptor. Set
/// before a bundle is written with the game's own code.
static INHERITED: std::sync::RwLock<BTreeMap<String, Vec<(String, String)>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Sets the methods classes take from the game over a default ([`INHERITED`]).
pub fn set_inherited(inherited: BTreeMap<String, Vec<(String, String)>>) {
    if let Ok(mut i) = INHERITED.write() {
        *i = inherited;
    }
}

fn inherited(class: &str) -> Vec<(String, String)> {
    INHERITED
        .read()
        .ok()
        .and_then(|i| i.get(class).cloned())
        .unwrap_or_default()
}

/// The instance fields the game's classes hold above the classes being translated
/// (`derive::fields_above`): by game class, the names it and the classes above it declare. Set
/// before a bundle is written with the game's own code; empty without it, when only a field of
/// a translated class counts as hidden ([`Program::hiding_class`]).
static GAME_FIELDS: std::sync::RwLock<BTreeMap<String, BTreeSet<String>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Sets the game's classes' instance fields ([`GAME_FIELDS`]).
pub fn set_game_fields(fields: BTreeMap<String, BTreeSet<String>>) {
    if let Ok(mut f) = GAME_FIELDS.write() {
        *f = fields;
    }
}

/// Whether game class `class`, or one above it, declares instance field `name`.
fn game_declares(class: &str, name: &str) -> bool {
    GAME_FIELDS
        .read()
        .is_ok_and(|f| f.get(class).is_some_and(|names| names.contains(name)))
}

/// The function that stands in `class` for a method it takes from the game's class above it.
fn inherited_fn(class: &str, name: &str, desc: &str) -> String {
    format!("{}__above", fn_name(class, name, desc))
}

/// Sets the game fields' accessors the code written from here on uses ([`ACCESSORS`]).
pub fn set_accessors(accessors: BTreeMap<(String, String, bool), String>) {
    if let Ok(mut a) = ACCESSORS.write() {
        *a = accessors;
    }
}

fn accessor(owner: &str, field: &str, write: bool) -> Option<String> {
    ACCESSORS
        .read()
        .ok()?
        .get(&(owner.to_owned(), field.to_owned(), write))
        .cloned()
}

const MOD: &str = "Lnet/neoforged/fml/common/Mod;";
const SUBSCRIBE_EVENT: &str = "Lnet/neoforged/bus/api/SubscribeEvent;";
/// `@EventBusSubscriber` (NeoForge 21) and its older home on `@Mod`.
/// Classes whose objects the runtime keeps as an array of their instance fields, in the order the
/// class declares them (then a subclass's): the game's matrices, which the runtime's own code
/// reads and writes as sixteen and nine numbers. Their own code, taken from the player's install,
/// reads a field as its slot.
pub const ARRAY_CLASSES: [&str; 2] = ["org/joml/Matrix4f", "org/joml/Matrix3f"];

/// A step of the mods' loading told to the runtime (`hz/loading`: which mod's constructor runs
/// next, that the last one ran, a class subscribed for its mod), as a call any build of the
/// generated code can make: the texts `args` are its arguments.
fn loading(step: &str, args: &[&str]) -> String {
    let texts: Vec<String> = args.iter().map(|a| format!("rt.string({a:?})")).collect();
    let desc = "Ljava/lang/String;".repeat(args.len());
    format!(
        "{{ let a = [{}]; horizon_compat::api::call(rt, \"hz/loading.{step}.({desc})V\", &a); }}",
        texts.join(", ")
    )
}

/// A field's value before anything is written to it, by its descriptor: None for a reference.
fn zero_of(desc: &str) -> Option<&'static str> {
    match desc {
        "I" | "Z" | "B" | "S" | "C" => Some("V::I(0)"),
        "J" => Some("V::J(0)"),
        "F" => Some("V::F(0.0)"),
        "D" => Some("V::D(0.0)"),
        _ => None,
    }
}

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
    /// Each mod class's id (1.., in name order, or as an earlier translation numbered them:
    /// [`Program::keep_ids`]): objects carry it, and virtual calls jump on it.
    class_ids: BTreeMap<String, u32>,
    /// The ids an earlier translation gave classes this one doesn't have.
    kept_classes: BTreeMap<String, u32>,
    /// Instance methods (`name.desc`) some mod class implements: each gets a dispatcher.
    vkeys: BTreeSet<String>,
    /// Each lambda implementation's id in `lambda_call` (as first seen).
    lambda_ids: BTreeMap<String, u32>,
    /// Those keys as first seen: a part registers the lambdas it saw first ([`Program::part`]).
    lambda_order: Vec<String>,
    /// The ids an earlier translation gave lambdas, and the next free one.
    kept_lambdas: BTreeMap<String, u32>,
    next_lambda: u32,
    /// The registries of the parts emitted so far (`REGISTRY_<n>`), by number.
    registries: BTreeSet<u32>,
    /// The mod's id: picks its `@Mod` class among several (libraries bundled in its jar have theirs).
    pub mod_id: Option<String>,
    /// The mods built on it translated with it (its addons), in loading order: FML constructs a
    /// mod after the mods it needs, so theirs come after its own.
    pub addons: Vec<String>,
    /// The mods' ids in the order FML loads them ([`crate::load_order`]): the order their classes
    /// are constructed in. Empty: the mod, then its addons as given.
    pub load_order: Vec<String>,
    /// Each mod's id, version and display name from its jar ([`crate::mod_metadata`]): what its
    /// `IModInfo` answers.
    pub metadata: Vec<(String, String, String)>,
    /// Each jar translated: the ids of the mods it declares and the classes it has
    /// ([`crate::class_names`]), for FML's scan of it. Empty: one jar of every class.
    pub scan_files: Vec<(Vec<String>, std::collections::HashSet<String>)>,
    /// A basic block of more text than the first is cut into functions of about the second
    /// ([`cut`]): [`LONG_BLOCK`] and [`BLOCK_PIECE`] unless a test asks for less.
    pub long_block: (usize, usize),
    /// The mods' jars' own files, by path, sorted ([`crate::jar_resources`]): what
    /// `Class.getResourceAsStream` finds, carried behind [`horizon_compat::FILES`].
    pub jar_files: Vec<(String, Vec<u8>)>,
    /// How much bytecode makes a method a big one, cut into functions of several blocks, and
    /// about how much text each holds ([`ranges`]): [`BIG_METHOD`] and [`RANGE_TEXT`] unless a
    /// test asks for less.
    pub big_method: (usize, usize),
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
    /// The static methods some bytecode names (`class.name.desc`, by the class that declares
    /// each), and every text the classes hold as a string constant or as a call site's name:
    /// what reflection can find a static method by ([`reflection_names`]).
    named_statics: HashSet<String>,
    texts: HashSet<&'a str>,
}

/// What reflection can find a static method by, among `classes`: the static methods some
/// bytecode names (a call, a method reference), each by the class that declares it, up the
/// superclasses from the one named (JVMS 5.4.3.3); and every text a class holds as a string
/// constant (`getMethod("register")`) or as the name of a call site (a bootstrap method looks
/// its target up by it). A static method outside both, and without an annotation a scan could
/// find it by, is one no code of the mods can name: its code is left for the linker to drop,
/// as it drops every method nothing calls.
fn reflection_names<'a>(
    classes: &BTreeMap<&'a str, &'a ClassFile>,
) -> (HashSet<String>, HashSet<&'a str>) {
    let (mut named, mut texts) = (HashSet::new(), HashSet::new());
    for c in classes.values() {
        let utf = |i: u16| match c.pool.get(i as usize) {
            Some(Const::Utf8(s)) => Some(s.as_str()),
            _ => None,
        };
        for k in &c.pool {
            match k {
                Const::String(i) => texts.extend(utf(*i)),
                Const::InvokeDynamic(_, nt) | Const::Dynamic(_, nt) => {
                    if let Some(Const::NameAndType(n, _)) = c.pool.get(*nt as usize) {
                        texts.extend(utf(*n));
                    }
                }
                Const::MethodRef(owner, nt) | Const::InterfaceMethodRef(owner, nt) => {
                    let (Some(Const::Class(owner)), Some(Const::NameAndType(n, d))) =
                        (c.pool.get(*owner as usize), c.pool.get(*nt as usize))
                    else {
                        continue;
                    };
                    let (Some(owner), Some(name), Some(desc)) = (utf(*owner), utf(*n), utf(*d))
                    else {
                        continue;
                    };
                    let mut at = classes.get(owner);
                    while let Some(k) = at {
                        if let Some(m) = k.methods.iter().find(|m| m.name == name && m.desc == desc)
                        {
                            if m.access & 0x0008 != 0 {
                                named.insert(format!("{}.{name}.{desc}", k.name));
                            }
                            break;
                        }
                        at = k.superclass.as_deref().and_then(|s| classes.get(s));
                    }
                }
                _ => {}
            }
        }
    }
    (named, texts)
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

/// Whether a mixin's method only stands for its target's: a `@Shadow`, or an `@Accessor` or
/// `@Invoker`, whose static form has a body only because `javac` wants one (`throw new
/// AssertionError()`) and Mixin replaces it. Taken for the target's own, a static invoker's
/// placeholder ran in place of the method it names: Create's `GaugeShaper.make()` threw, by
/// another mod's `@Invoker("make")`, and its gauges were never registered.
fn is_shadow(m: &crate::class::Method) -> bool {
    m.annotations.iter().any(|a| {
        matches!(
            a.ty.as_str(),
            "Lorg/spongepowered/asm/mixin/Shadow;"
                | "Lorg/spongepowered/asm/mixin/gen/Accessor;"
                | "Lorg/spongepowered/asm/mixin/gen/Invoker;"
        )
    })
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

/// The call the runtime answers after a write to game field `m` it hears of, which is kept as any
/// other first: `Entity.hurtMarked` (a server sends a marked player their motion, as
/// `ServerEntity.sendChanges` does). None for the rest.
fn heard_write(m: &Member) -> Option<&'static str> {
    match (m.name.as_str(), m.desc.as_str()) {
        ("hurtMarked", "Z") => Some("net/minecraft/world/entity/Entity.$hurtMarked.(Z)V"),
        // A living entity's head and body, which the game turns (Create's seats align the mob
        // they carry).
        ("yHeadRot", "F") => Some("net/minecraft/world/entity/LivingEntity.$yHeadRot.(F)V"),
        ("yBodyRot", "F") => Some("net/minecraft/world/entity/LivingEntity.$yBodyRot.(F)V"),
        _ => None,
    }
}

/// A method a mixin adds to the game's classes (or overwrites there), as the dispatcher of its
/// `name.desc` finds it for a game object: through the mixin, its target or an interface it adds.
struct Added {
    targets: Vec<String>,
    /// Owner keys whose calls are this method's whatever the receiver.
    owners: Vec<String>,
    /// Owner keys whose calls are its on an object of one of `targets` (the game's own interfaces
    /// the mixin adds; the target itself, for an `@Overwrite`).
    checked: Vec<String>,
}

/// Class `c`'s `@SubscribeEvent` methods: (key, handle kind as a lambda's, the method), so
/// listeners made from them run as lambdas. With them the handlers a mixin injects into a method
/// the runtime runs itself (`native`: [`weave::Plan::native`]), which listen for the places they
/// name as a subscriber does for its event.
fn subscribers_of(
    c: &ClassFile,
    native: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, u8, Member)> {
    c.methods
        .iter()
        .filter(|m| m.code.is_some())
        .map(|m| (format!("{}.{}.{}", c.name, m.name, m.desc), m))
        .filter(|(key, m)| {
            m.annotations.iter().any(|a| a.ty == SUBSCRIBE_EVENT) || native.contains_key(key)
        })
        .map(|(key, m)| {
            let kind = if m.access & 0x0008 != 0 { 6 } else { 5 };
            let member = Member { owner: c.name.clone(), name: m.name.clone(), desc: m.desc.clone() };
            (key, kind, member)
        })
        .collect()
}

/// Those of class `c`'s `@SubscribeEvent` methods whose options aren't the default, by key:
/// `EventPriority`'s ordinal (0 `HIGHEST` to 4 `LOWEST`, 2 `NORMAL`) and `receiveCanceled`.
fn subscriber_options_of(c: &ClassFile) -> Vec<(String, (u8, bool))> {
    const PRIORITIES: [&str; 5] = ["HIGHEST", "HIGH", "NORMAL", "LOW", "LOWEST"];
    let mut out = Vec::new();
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
            out.push((format!("{}.{}.{}", c.name, m.name, m.desc), (priority, canceled)));
        }
    }
    out
}

/// The registries a part defines (`pub static REGISTRY_<n>`), by name.
pub(crate) fn registries_in(part: &str) -> Vec<String> {
    part.match_indices("pub static REGISTRY_")
        .filter_map(|(at, _)| {
            let rest = &part[at + "pub static ".len()..];
            let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
            Some(rest[..end].to_owned())
        })
        .collect()
}

/// The dispatcher of `nd` (`d_...`, which generated code calls) as a function of the crate that
/// calls it: the runtime's dispatch, with the key's hash.
pub(crate) fn dispatcher_shim(nd: &str, vis: &str) -> String {
    format!(
        "{vis}fn {}(rt: &mut Rt, cid: u32, owner_key: &'static str, args: &[V]) -> V {{\n    horizon_compat::linked::dispatch(rt, cid, {:#x}, {nd:?}, owner_key, args)\n}}\n",
        dispatcher(nd),
        horizon_compat::linked::key_hash(nd)
    )
}

/// The runtime's `vcall_miss` under the name generated code calls it by.
pub(crate) const MISS_SHIM: &str = "fn vcall_miss(rt: &mut Rt, cid: u32, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n    horizon_compat::linked::vcall_miss(rt, cid, owner_key, key, args)\n}\n";
/// The runtime's `java_string` under the name generated code calls it by.
pub(crate) const STRING_SHIM: &str = "fn java_string(rt: &mut Rt, v: V) -> V {\n    horizon_compat::linked::java_string(rt, v)\n}\n";

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
        let by_name: BTreeMap<&'a str, &'a ClassFile> =
            classes.iter().map(|c| (c.name.as_str(), c)).collect();
        let (named_statics, texts) = reflection_names(&by_name);
        Program {
            classes: by_name,
            named_statics,
            texts,
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
            lambda_order: Vec::new(),
            kept_classes: BTreeMap::new(),
            kept_lambdas: BTreeMap::new(),
            next_lambda: 0,
            registries: BTreeSet::new(),
            mod_id: None,
            addons: Vec::new(),
            load_order: Vec::new(),
            metadata: Vec::new(),
            scan_files: Vec::new(),
            long_block: (LONG_BLOCK, BLOCK_PIECE),
            big_method: (BIG_METHOD, RANGE_TEXT),
            jar_files: Vec::new(),
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

    /// What mixin `mixin` adds to the game's classes `targets` ([`weave::Plan::vanilla`]), by
    /// `name.desc`: its instance methods with code, less `@Shadow`s and injectors' handlers.
    fn vanilla_added(&self, mixin: &str, targets: &[String]) -> Vec<(String, Added)> {
        let mut out = Vec::new();
        let Some(k) = self.classes.get(mixin) else { return out };
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
            let mut owners: Vec<String> = std::iter::once(mixin).chain(mods.iter().map(String::as_str)).map(key).collect();
            let mut checked: Vec<String> = game.iter().map(|c| key(c)).collect();
            for t in targets {
                if overwrite { checked.push(key(t)) } else { owners.push(key(t)) }
            }
            out.push((nd, Added { targets: targets.to_vec(), owners, checked }));
        }
        out
    }

    /// The field initialisers of a mixin into the game's class (its constructor, whose call of
    /// its superclass's is left out), as a function that runs them once on each game object the
    /// mixin's methods run on: Mixin runs them in the target's constructors, the game's own. None
    /// for a mixin without fields of its own.
    fn mixin_fields(&self, mixin: &str) -> Option<(String, String)> {
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

    /// Whether handler `key` (`mixin.name.desc`) is one a mixin injects at the end of the
    /// constructors of the game's class, and nowhere else (`@Inject(method = "<init>", at =
    /// @At("RETURN"))`, or `TAIL`): it runs once on each object, where the runtime tells of the
    /// constructor's end ([`weave::Plan::native`]).
    fn at_constructors_end(&self, key: &str) -> bool {
        self.weave.native.get(key).is_some_and(|places| {
            !places.is_empty()
                && places
                    .iter()
                    .all(|p| p.ends_with(".<init>@RETURN") || p.ends_with(".<init>@TAIL"))
        })
    }

    /// What Mixin puts into the constructors of a mixin's target, where the target is the
    /// game's (its constructors are not run here), as a function that runs it once on each game
    /// object before the first of the mixin's methods does: the mixin's field initialisers
    /// ([`Self::mixin_fields`]), then its handlers at the constructors' end, should the runtime
    /// not have run those where the object was made: asked of the runtime as an API call
    /// (`hz/mixin-made:<mixin>.late`: the code's crates link the runtime's core alone). The
    /// name of the function and its code, the field initialisers' with it. None for a mixin
    /// with neither.
    fn mixin_init(&self, mixin: &str) -> Option<(String, String)> {
        let k = self.classes.get(mixin)?;
        let fields = self.mixin_fields(mixin);
        let ends = k.methods.iter().any(|m| {
            m.access & 0x0008 == 0
                && self.at_constructors_end(&format!("{mixin}.{}.{}", m.name, m.desc))
        });
        if !ends {
            return fields;
        }
        let name = fn_name(mixin, "<mixin-made>", "");
        let flag = format!("$mixed {mixin}");
        let late = format!("hz/mixin-made:{mixin}.late.(Ljava/lang/Object;)V");
        let (run_fields, fields) = match fields {
            Some((f, code)) => (format!("    {f}(rt, this);\n"), code),
            None => (String::new(), String::new()),
        };
        let code = format!(
            "{fields}/// What `{mixin}` adds to its target's constructors, run once on each game object before\n\
             /// the first of its methods: its handlers at the constructors' end, where nothing ran them.\n\
             #[allow(dead_code)]\n\
             fn {name}(rt: &mut Rt, this: V) {{\n\
             \x20   if matches!(rt.get_field(this, {flag:?}), V::I(1)) {{ return; }}\n\
             \x20   rt.put_field(this, {flag:?}, V::I(1));\n\
             {run_fields}\
             \x20   api::call(rt, {late:?}, &[this]);\n\
             }}\n\n"
        );
        Some((name, code))
    }

    /// A class's interfaces, with those its mixins add.
    fn interfaces_of(&self, k: &ClassFile) -> Vec<String> {
        let mut out = k.interfaces.clone();
        for m in self.mixins_of(&k.name) {
            for i in self.classes.get(m.as_str()).copied().map(mixin_adds).unwrap_or_default() {
                if !out.iter().any(|o| o == i) {
                    out.push(i.to_owned());
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
        // The sides a `@Mod` class is for (`dist`): both unless it names them.
        let sides = |c: &ClassFile| {
            c.annotation_of(MOD)
                .and_then(|a| a.value("dist"))
                .filter(|d| !d.is_empty())
                .map(|d| d.split(',').map(str::to_owned).collect::<Vec<_>>())
        };
        let mut found: Vec<(&'a ClassFile, &'a crate::class::Method)> = Vec::new();
        for c in self.classes.values() {
            if c.annotation_of(MOD).is_none() {
                continue;
            }
            // FML constructs a class only on a side it is for: the game here is a client, so a
            // dedicated server's entry class (AE2's `AppEngServer`) is not constructed.
            if sides(c).is_some_and(|s| !s.iter().any(|d| d == "CLIENT")) {
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
            if !self.load_order.is_empty() {
                // FML's order of the mods, those it does not list first.
                self.load_order
                    .iter()
                    .position(|m| Some(m.as_str()) == id)
                    .map_or(0, |i| i + 1)
            } else if id.is_some() && id == self.mod_id.as_deref() {
                1
            } else {
                self.addons.iter().position(|a| Some(a.as_str()) == id).map_or(0, |i| i + 2)
            }
        };
        // Of one mod's classes, the one for both sides before the one for this side only, as FML
        // sorts a mod's entry classes.
        found.sort_by_key(|(c, _)| (rank(c), sides(c).is_some(), c.name.clone()));
        found
    }

    fn own(&self, class: &str) -> bool {
        self.classes.contains_key(class)
    }

    /// A class's id; 0 for the API's.
    fn cid(&self, class: &str) -> u32 {
        self.class_ids.get(class).copied().unwrap_or(0)
    }

    fn lambda_id(&mut self, key: &str) -> u32 {
        if let Some(&id) = self.lambda_ids.get(key) {
            return id;
        }
        // The id an earlier translation gave it ([`Program::keep_ids`]), else the next free one.
        let id = match self.kept_lambdas.get(key) {
            Some(&id) => id,
            None => {
                self.next_lambda += 1;
                self.next_lambda - 1
            }
        };
        self.lambda_ids.insert(key.to_owned(), id);
        self.lambda_order.push(key.to_owned());
        id
    }

    /// Gives each class and lambda the id an earlier translation gave it (`ids`: what
    /// [`Program::ids`] wrote then), and the rest ids past those. A class's id and a lambda's are
    /// part of the text of every crate that makes one, so numbering them afresh in order would
    /// change, and build again, every crate after a class added or dropped; kept, a translation
    /// of the same mods with one more changes only the crates that mod's code is in.
    pub fn keep_ids(&mut self, ids: &str) {
        let (mut classes, mut lambdas) = (BTreeMap::new(), BTreeMap::new());
        for line in ids.lines() {
            let mut parts = line.splitn(3, ' ');
            let (Some(kind), Some(Ok(id)), Some(key)) = (
                parts.next(),
                parts.next().map(str::parse::<u32>),
                parts.next(),
            ) else {
                continue;
            };
            match kind {
                "c" if id > 0 => classes.insert(key.to_owned(), id),
                "l" => lambdas.insert(key.to_owned(), id),
                _ => None,
            };
        }
        let mut next = classes.values().copied().max().unwrap_or(0) + 1;
        for (name, id) in &mut self.class_ids {
            *id = match classes.get(name) {
                Some(&kept) => kept,
                None => {
                    next += 1;
                    next - 1
                }
            };
        }
        self.next_lambda = lambdas.values().copied().max().map_or(0, |m| m + 1);
        // What this translation has no use for keeps its id, should it come back.
        classes.retain(|name, _| !self.class_ids.contains_key(name));
        self.kept_classes = classes;
        self.kept_lambdas = lambdas;
    }

    /// The ids of the classes and the lambdas, a line each (`c <id> <class>`, `l <id> <lambda>`),
    /// for the next translation to keep ([`Program::keep_ids`]): after the program is emitted.
    pub fn ids(&self) -> String {
        let mut out = String::new();
        for (name, id) in self.class_ids.iter().chain(&self.kept_classes) {
            let _ = writeln!(out, "c {id} {name}");
        }
        let kept = self
            .kept_lambdas
            .iter()
            .filter(|(key, _)| !self.lambda_ids.contains_key(*key));
        for (key, id) in self.lambda_ids.iter().chain(kept) {
            let _ = writeln!(out, "l {id} {key}");
        }
        out
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

    /// The module split into files of about `per_file` classes each, for big mods: `lib.rs` holds
    /// the engine ([`Program::tables`]), the dispatchers and the list of the parts' registries, and
    /// re-exports every part.
    pub fn emit_files(&mut self, per_file: usize) -> Vec<(String, String)> {
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let mut files = Vec::new();
        let mut lib = format!("{LIB_HEADER}use horizon_compat::{{api, Rt, V}};\n");
        let mut registries = Vec::new();
        for (n, chunk) in parts_of(&classes, per_file).iter().enumerate() {
            let _ = writeln!(lib, "mod part{n};\npub use part{n}::*;");
            let part = self.part(chunk);
            registries.extend(registries_in(&part));
            files.push((format!("part{n}.rs"), part));
        }
        lib.push_str(&self.tables(&classes));
        lib.push_str(&self.own_links(&registries));
        files.push(("lib.rs".into(), lib));
        files
    }

    /// The module for a split engine ([`crate::split`]): the parts of each group of classes
    /// (`group(class)` below `groups`; others go to the last), each group's classes in files of
    /// about `per_file`, and the engine's `lib.rs`, which holds no part: [`crate::split`] adds
    /// what links its crates.
    pub fn emit_groups(&mut self, per_file: usize, groups: usize, group: &dyn Fn(&str) -> usize) -> (Vec<Vec<String>>, String) {
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let groups = groups.max(1);
        let mut parts = vec![Vec::new(); groups];
        for (g, list) in parts.iter_mut().enumerate() {
            let mine: Vec<&ClassFile> = classes.iter().copied().filter(|c| group(&c.name).min(groups - 1) == g).collect();
            for chunk in parts_of(&mine, per_file) {
                list.push(self.part(&chunk));
            }
        }
        let lib = format!("{LIB_HEADER}use horizon_compat::{{api, Rt, V}};\n{}", self.tables(&classes));
        (parts, lib)
    }

    /// Each dispatcher's name (`d_...`) with the method key it dispatches: what a code crate's
    /// links make of the dispatchers it calls ([`crate::split`]).
    pub fn dispatchers(&self) -> BTreeMap<String, String> {
        self.vkeys.iter().map(|nd| (dispatcher(nd), nd.clone())).collect()
    }

    /// What the engine of a module of one crate adds (its parts are its own modules): every
    /// dispatcher, the runtime's `vcall_miss` and `java_string`, and the list of the parts'
    /// `registries`.
    fn own_links(&self, registries: &[String]) -> String {
        let mut s = String::new();
        for nd in &self.vkeys {
            s.push_str(&dispatcher_shim(nd, "pub "));
        }
        let _ = write!(
            s,
            "pub {MISS_SHIM}pub {STRING_SHIM}\n/// Every part's registry.\nfn all_registries() -> Vec<&'static horizon_compat::linked::Registry> {{\n    vec![{}]\n}}\n\n/// The parts are this crate's own: nothing to link.\nfn link_crates() {{}}\n",
            registries.iter().map(|r| format!("&{r}")).collect::<Vec<_>>().join(", ")
        );
        s
    }

    /// One file of methods: every method of `classes` with code, the functions they need, and
    /// what the file registers with the engine's class space ([`Program::registry`]).
    fn part(&mut self, classes: &[&ClassFile]) -> String {
        let mut part = String::from(PART_HEADER);
        let first_lambda = self.lambda_order.len();
        // Listeners made from `@SubscribeEvent` methods run as lambdas.
        for c in classes {
            for (key, _, _) in subscribers_of(c, &self.weave.native) {
                self.lambda_id(&key);
            }
        }
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
        part.push_str(&self.registry(classes, first_lambda));
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

    /// The engine: its class space linked from every part's registry (`all_registries` and
    /// `link_crates`, which [`Program::own_links`] or [`crate::split`] add), `init`, which joins a
    /// runtime to it, FML's scan of each jar, the services, and `start`, the mods' loading.
    fn tables(&mut self, classes: &[&ClassFile]) -> String {
        let mut out = String::from(
            "/// The engine's class space: every part's registry, linked once.\n\
             static LINKED: std::sync::OnceLock<horizon_compat::linked::Linked> = std::sync::OnceLock::new();\n\n\
             fn linked() -> &'static horizon_compat::linked::Linked {\n    LINKED.get_or_init(|| {\n        link_crates();\n        horizon_compat::linked::Linked::new(all_registries(), SERVICES)\n    })\n}\n\n\
             /// Whether a mod class implements `key` (`name.desc`) itself or through a mod superclass.\n\
             pub fn overrides(class: &str, key: &str) -> bool {\n    linked().overrides(class, key)\n}\n\n\
             /// Registers the mod's class hierarchy and callbacks with the runtime. Call once, before anything else.\n\
             pub fn init(rt: &mut Rt) {\n    rt.link(linked());\n    rt.overrides = Some(overrides);\n",
        );
        // What FML's scan of each jar finds (`ModList.getAllScanData`).
        let scans = self.scan_texts(classes);
        for (i, (mods, _)) in scans.iter().enumerate() {
            let _ = writeln!(out, "    rt.scan_data.push(({mods:?}, SCAN_{i}));");
        }
        out.push_str("}\n\n");
        for (i, (_, text)) in scans.iter().enumerate() {
            let _ = writeln!(out, "const SCAN_{i}: &str = {text:?};\n");
        }
        out.push_str("/// `META-INF/services`: the implementations each interface names, for `ServiceLoader`.\nstatic SERVICES: &[(&str, &[&str])] = &[\n");
        for (iface, impls) in &self.services {
            let _ = writeln!(out, "    ({iface:?}, &{impls:?}),");
        }
        out.push_str("];\n\n");
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
                let _ = writeln!(out, "        {}", loading("constructing", &[id]));
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
        if !mains.is_empty() {
            let _ = writeln!(out, "    {}", loading("constructed", &[]));
        }
        for c in classes {
            let Some(a) = EVENT_BUS_SUBSCRIBER.iter().find_map(|t| c.annotation_of(t)) else {
                continue;
            };
            // FML subscribes a class on the sides it is for (`value`, both unless it names them):
            // a dedicated server's is left out of this client.
            if a.value("value")
                .is_some_and(|sides| !sides.is_empty() && !sides.split(',').any(|d| d == "CLIENT"))
            {
                continue;
            }
            // The mod whose bus FML subscribes it to: the one it names, the one its class is the
            // main class of, else its jar's.
            let named = a
                .value("modid")
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            let own = || {
                c.annotation_of(MOD)
                    .and_then(|a| a.value("value"))
                    .map(str::to_owned)
            };
            let jar = || match self
                .scan_files
                .iter()
                .find(|(_, names)| names.contains(&c.name))
            {
                Some((jar_ids, _)) => jar_ids.first().cloned(),
                None => ids.first().cloned(),
            };
            match named.or_else(own).or_else(jar) {
                Some(id) => {
                    let _ = writeln!(out, "    {}", loading("subscribe", &[&c.name, &id]));
                }
                None => {
                    let _ = writeln!(out, "    rt.subscribe_class({:?});", c.name);
                }
            }
        }
        if !mains.is_empty() {
            out.push_str("    horizon_compat::neo::fire_registrations(rt);\n");
        }
        let _ = writeln!(out, "    {}\n}}\n", loading("resolved", &[]));
        out
    }

    /// What a part registers with the engine's class space (`REGISTRY_<n>`): each of its classes
    /// (its supertypes, its instance methods and what its mixins add, its reflection, fields,
    /// annotations and listeners), the lambdas the part made first (`first_lambda` on), the
    /// methods and interfaces its mixins add to the game's classes, and its listeners' options.
    /// The functions those name come first.
    fn registry(&mut self, classes: &[&ClassFile], first_lambda: usize) -> String {
        let mut code = String::new();
        let mut defs = String::new();
        let mut implementations: BTreeMap<String, (u8, Member)> = BTreeMap::new();
        for c in classes {
            let reflect = self.reflect_fn(c).map(|(name, f)| {
                code.push_str(&f);
                name
            });
            // What the class takes from the game's class above it, over an interface's default: its
            // own method here, which asks the class above as `super.method(...)` would.
            for (name, desc) in inherited(&c.name) {
                let above = c.superclass.as_deref().unwrap_or("java/lang/Object");
                let _ = write!(
                    code,
                    "/// `{}` takes `{name}{desc}` from the game's class above it, not from an interface's default.
fn {}(rt: &mut Rt, args: &[V]) -> V {{
    api::call(rt, {:?}, args)
}}

",
                    c.name,
                    inherited_fn(&c.name, &name, &desc),
                    format!("{above}.{name}.{desc}")
                );
            }
            defs.push_str(&self.class_def(c, reflect));
            for (key, kind, m) in subscribers_of(c, &self.weave.native) {
                implementations.insert(key, (kind, m));
            }
        }
        let mut lambdas = String::new();
        for key in &self.lambda_order[first_lambda..] {
            let lid = self.lambda_ids[key];
            let body = match (self.lambdas.get(key).or(implementations.get(key)), self.operations.get(key)) {
                (Some((kind, m)), _) => self.lambda_call(key, *kind, m),
                (None, Some(f)) => format!("{f}(rt, &full)"),
                (None, None) => continue,
            };
            let _ = write!(code, "// {key}\nfn l_{lid}(rt: &mut Rt, mut full: Vec<V>) -> V {{\n    {body}\n}}\n\n");
            let _ = writeln!(lambdas, "        ({lid}, l_{lid}),");
        }
        let (mut added, mut game, mut options) = (String::new(), String::new(), String::new());
        for c in classes {
            if let Some(targets) = self.weave.vanilla.get(&c.name) {
                let targets = targets.clone();
                self.added_by(c, &targets, &mut code, &mut added, &mut game);
            }
            for (key, (priority, canceled)) in subscriber_options_of(c) {
                let _ = writeln!(options, "        ({key:?}, ({priority}, {canceled})),");
            }
        }
        // Named by the part's first class, so that a part added to another crate renames none of
        // this one's (a number in order would, and with it change the text of every later part).
        let mut n = hash(classes.first().map_or("", |c| c.name.as_str()));
        while !self.registries.insert(n) {
            n = n.wrapping_add(1);
        }
        format!(
            "{code}/// What this part registers with the engine's class space.\n\
             pub static REGISTRY_{n}: Registry = Registry {{\n    classes: &[\n{defs}    ],\n    lambdas: &[\n{lambdas}    ],\n    added: &[\n{added}    ],\n    game_interfaces: &[\n{game}    ],\n    subscriber_options: &[\n{options}    ],\n}};\n"
        )
    }

    /// A class as its part registers it ([`horizon_compat::linked::ClassDef`]); `reflect` names
    /// its reflection function, when it has one.
    fn class_def(&self, c: &ClassFile, reflect: Option<String>) -> String {
        let methods: Vec<String> = c
            .methods
            .iter()
            // A class of the game's methods standing for unmapped ones is asked by key for any
            // of them, a static one or a constructor too.
            .filter(|m| m.code.is_some() && (c.name.starts_with(horizon_compat::linked::DERIVED) || (m.access & 0x0008 == 0 && !m.name.starts_with('<'))))
            .map(|m| format!("({:?}, {}, {})", format!("{}.{}", m.name, m.desc), fn_name(&c.name, &m.name, &m.desc), m.access & 0x0002 != 0))
            .chain(inherited(&c.name).iter().map(|(name, desc)| format!("({:?}, {}, false)", format!("{name}.{desc}"), inherited_fn(&c.name, name, desc))))
            .collect();
        // What a mixin adds to the class, or overwrites in it, is the class's own, before its own.
        let mut mixed = Vec::new();
        for mixin in self.mixins_of(&c.name) {
            let Some(k) = self.classes.get(mixin.as_str()) else { continue };
            for x in k.methods.iter().filter(|x| x.code.is_some() && !is_shadow(x) && x.access & 0x0008 == 0 && !x.name.starts_with('<')) {
                mixed.push(format!("({:?}, {})", format!("{}.{}", x.name, x.desc), fn_name(mixin, &x.name, &x.desc)));
            }
        }
        // A class's declared fields (`getDeclaredFields`, `Field.getType`): a line of `access name
        // descriptor` per field, with its generic signature after where it has one (what a list
        // holds, for whoever fills one by the field's type); and before them the class's own
        // (`0 <class> signature`: its supertypes' type arguments).
        let class_signature = c.signature.iter().map(|s| format!("0 <class> {s}"));
        // And the instance fields among them that hide one above (`0 <hides> name...`): where
        // reflection finds each, as the code does ([`Self::instance_key`]).
        let hides: Vec<&str> = c
            .fields
            .iter()
            .filter(|f| {
                f.access & 0x0008 == 0
                    && self.hiding_class(&c.name, &f.name) == Some(c.name.as_str())
            })
            .map(|f| f.name.as_str())
            .collect();
        let hides =
            (!hides.is_empty()).then(|| format!("0 {} {}", horizon_compat::HIDES, hides.join(" ")));
        let fields = class_signature
            .chain(hides)
            .chain(c.fields.iter().map(|f| match &f.signature {
                Some(s) => format!("{} {} {} {s}", f.access, f.name, f.desc),
                None => format!("{} {} {}", f.access, f.name, f.desc),
            }))
            .collect::<Vec<_>>()
            .join("\n");
        let annotations = annotation_lines(c).unwrap_or_default();
        // `@SubscribeEvent` methods: what `bus.register(...)` and `@EventBusSubscriber` listen with.
        // And a mixin's injectors into a method the runtime runs itself, which listen for the
        // places they name.
        let subscribers: Vec<String> = subscribers_of(c, &self.weave.native)
            .iter()
            .flat_map(|(key, kind, m)| {
                let events = match self.weave.native.get(key) {
                    Some(places) => places.clone(),
                    None => crate::class::param_classes(&m.desc)
                        .last()
                        .cloned()
                        .into_iter()
                        .collect(),
                };
                let lid = self.lambda_ids.get(key).copied();
                events.into_iter().filter_map(move |event| {
                    Some(format!("({event:?}, {key:?}, {}, {})", *kind == 6, lid?))
                })
            })
            .collect();
        format!(
            "        ClassDef {{ name: {:?}, id: {}, superclass: {}, interfaces: &{:?}, methods: &[{}], mixed: &[{}], reflect: {}, fields: {fields:?}, annotations: {annotations:?}, subscribers: &[{}] }},\n",
            c.name,
            self.cid(&c.name),
            c.superclass.as_ref().map_or_else(|| "None".to_owned(), |s| format!("Some({s:?})")),
            self.interfaces_of(c),
            methods.join(", "),
            mixed.join(", "),
            reflect.map_or_else(|| "None".to_owned(), |r| format!("Some({r})")),
            subscribers.join(", ")
        )
    }

    /// Reflection on class `c` (`r_<id>`, see `Rt::reflect`): what `Class.forName`, `newInstance`,
    /// `getEnumConstants` and `ServiceLoader` reach. Every constructor of a class that can be
    /// made (`getConstructor(Builder.class).newInstance(builder)`, as Max Henkel's configs are
    /// made): the runtime hands it the arguments, primitives unboxed. None when it has nothing.
    fn reflect_fn(&self, c: &ClassFile) -> Option<(String, String)> {
        // The bundle's tables, each behind a class of its own ([`table_class`]).
        if c.name == horizon_compat::FILES {
            return Some(self.table_fn(c, &self.jar_files, false));
        }
        if c.name == horizon_compat::MEMBERS {
            return Some(self.table_fn(c, &self.members(), true));
        }
        let ensure = self.ensure(&c.name).replace("return V::Void;", "return Some(V::Void);");
        let mut arms = String::new();
        if !ensure.is_empty() {
            let _ = writeln!(arms, "        \"<clinit>\" => {{ {ensure}V::Void }}");
        }
        let concrete = c.access & 0x0600 == 0 && c.mixin_targets().is_empty();
        for m in c.methods.iter().filter(|m| concrete && m.name == "<init>" && m.code.is_some()) {
            let _ = writeln!(
                arms,
                "        {:?} => {{ {ensure}let o = {}; let mut a = vec![o]; a.extend_from_slice(args); {}(rt, &a); o }}",
                format!("<init>.{}", m.desc),
                self.new_object(&c.name),
                fn_name(&c.name, "<init>", &m.desc)
            );
        }
        if c.access & 0x4000 != 0
            && let Some(m) = c.methods.iter().find(|m| m.name == "values" && m.access & 0x0008 != 0 && m.desc.starts_with("()[") && m.code.is_some())
        {
            let _ = writeln!(arms, "        \"values\" => {{ {ensure}{}(rt, &[]) }}", fn_name(&c.name, "values", &m.desc));
        }
        // Its static methods, which reflection runs by name (`method.invoke(null, ...)`, a
        // handle `findStatic` gave): each under `s:` and its name and descriptor, the class
        // initialised first as a call of one does. Those reflection can find
        // ([`reflection_names`]): one some bytecode names, one whose name some class holds as
        // text, one with an annotation reflection sees (AE2 runs each part class's
        // `@PartModels` method). Not those the compiler made for itself (a lambda's body, an
        // accessor), which no code names.
        if c.mixin_targets().is_empty() {
            let mut statics = String::new();
            for m in c.methods.iter().filter(|m| {
                m.access & 0x0008 != 0
                    && m.access & 0x1000 == 0
                    && m.name != "<clinit>"
                    && m.code.is_some()
            }) {
                if self
                    .named_statics
                    .contains(&format!("{}.{}.{}", c.name, m.name, m.desc))
                    || self.texts.contains(m.name.as_str())
                    || m.annotations.iter().any(|a| a.visible)
                {
                    let _ = writeln!(
                        statics,
                        "                {:?} => {},",
                        format!("{}.{}", m.name, m.desc),
                        fn_name(&c.name, &m.name, &m.desc)
                    );
                }
            }
            if !statics.is_empty() {
                let _ = writeln!(
                    arms,
                    "        s if s.starts_with(\"s:\") => {{\n            let f: fn(&mut Rt, &[V]) -> V = match &s[2..] {{\n{statics}                _ => return None,\n            }};\n            {ensure}f(rt, args)\n        }}"
                );
            }
        }
        if arms.is_empty() {
            return None;
        }
        let name = format!("r_{}", self.cid(&c.name));
        Some((name.clone(), format!("fn {name}(rt: &mut Rt, what: &str, args: &[V]) -> Option<V> {{\n    Some(match what {{\n{arms}        _ => return None,\n    }})\n}}\n\n")))
    }

    /// Each class's constructors and methods as reflection lists them, by the class's name: a
    /// line each, `access name descriptor` (a constructor's name is `<init>`), in the class
    /// file's order. One table for the whole program, so that `getDeclaredMethods` costs the
    /// crates of the classes nothing.
    fn members(&self) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = self
            .classes
            .values()
            .filter(|c| !c.name.starts_with("hz/"))
            .map(|c| {
                let lines: Vec<String> = c
                    .methods
                    .iter()
                    .filter(|m| m.name != "<clinit>")
                    .map(|m| format!("{} {} {}", m.access, m.name, m.desc))
                    .collect();
                (c.name.clone(), lines.join("\n").into_bytes())
            })
            .filter(|(_, lines)| !lines.is_empty())
            .collect();
        out.sort();
        out
    }

    /// The `reflect` function of a table's class: what the table holds under the name asked for
    /// (`rows`, sorted by name), as a string (`text`) or as a `byte[]`.
    fn table_fn(&self, c: &ClassFile, rows: &[(String, Vec<u8>)], text: bool) -> (String, String) {
        let name = format!("r_{}", self.cid(&c.name));
        let mut table = String::new();
        for (key, value) in rows {
            let literal: String = value
                .iter()
                .flat_map(|b| std::ascii::escape_default(*b))
                .map(char::from)
                .collect();
            let _ = writeln!(table, "        ({key:?}, b\"{literal}\"),");
        }
        let made = if text {
            "rt.string(&String::from_utf8_lossy(TABLE[i].1))".to_owned()
        } else {
            "{ let a = rt.new_array(TABLE[i].1.len() as i32, V::I(0)); for (at, b) in TABLE[i].1.iter().enumerate() { rt.array_set(a, at as i32, V::I(i32::from(*b as i8))); } a }".to_owned()
        };
        let body = format!(
            "fn {name}(rt: &mut Rt, what: &str, _args: &[V]) -> Option<V> {{\n    static TABLE: &[(&str, &[u8])] = &[\n{table}    ];\n    let i = TABLE.binary_search_by(|row| row.0.cmp(what)).ok()?;\n    Some({made})\n}}\n\n"
        );
        (name, body)
    }

    /// The methods mixin `c` adds to the game's classes `targets` (or overwrites there): for each,
    /// a function that initialises the mixin's class and runs its field initialisers on the object
    /// first (into `code`), and its entry (into `added`); and the interfaces it adds to them (into
    /// `game`): `instanceof` a duck interface or an accessor on a game object.
    fn added_by(&mut self, c: &ClassFile, targets: &[String], code: &mut String, added: &mut String, game: &mut String) {
        let mut adds: Vec<&str> = Vec::new();
        let mut queue: Vec<&str> = mixin_adds(c);
        while let Some(i) = queue.pop() {
            if !adds.contains(&i) {
                adds.push(i);
                queue.extend(self.classes.get(i).map(|ik| ik.interfaces.iter().map(String::as_str).collect::<Vec<_>>()).unwrap_or_default());
            }
        }
        if !adds.is_empty() {
            for t in targets {
                let _ = writeln!(game, "        ({t:?}, &{adds:?}),");
            }
        }
        let list = self.vanilla_added(&c.name, targets);
        // What it adds to the constructors: for the methods it adds, and for its handlers the
        // runtime calls at places of the game's code ([`Self::lambda_call`]).
        let heard = c.methods.iter().any(|m| {
            m.access & 0x0008 == 0
                && self
                    .weave
                    .native
                    .contains_key(&format!("{}.{}.{}", c.name, m.name, m.desc))
        });
        let init = match self.mixin_init(&c.name) {
            Some((name, f)) if heard || !list.is_empty() => {
                code.push_str(&f);
                format!("{name}(rt, args[0]); if rt.thrown.is_some() {{ return V::Void; }} ")
            }
            _ => String::new(),
        };
        let id = self.cid(&c.name);
        // The default methods of the mods' interfaces it adds: a class it is mixed into has
        // them as any class has its interfaces' (Architectury's `arch$registryName()` of an
        // item, of a block, of an entity type). Not one the mixin has for itself, nor one a
        // target's own classes declare, since a class's method comes before an interface's
        // default; of one that two of the interfaces have, the one of the interface below.
        // A call reaches it through one of those interfaces on an object of a target, or
        // written on the target.
        let below = |this: &Self, a: &str, b: &str| {
            let mut queue = vec![a];
            let mut seen: Vec<&str> = Vec::new();
            while let Some(i) = queue.pop() {
                if seen.contains(&i) {
                    continue;
                }
                seen.push(i);
                if let Some(k) = this.classes.get(i) {
                    queue.extend(k.interfaces.iter().map(String::as_str));
                }
            }
            a != b && seen.contains(&b)
        };
        let mut defaults: Vec<(String, &str)> = Vec::new();
        for &i in &adds {
            let Some(ik) = self.classes.get(i) else {
                continue;
            };
            for m in ik
                .methods
                .iter()
                .filter(|m| m.code.is_some() && m.access & 0x000a == 0 && !m.name.starts_with('<'))
            {
                let nd = format!("{}.{}", m.name, m.desc);
                if c.methods
                    .iter()
                    .any(|x| x.name == m.name && x.desc == m.desc)
                    || targets
                        .iter()
                        .any(|t| horizon_compat::api::game_class_declares(t, &nd))
                {
                    continue;
                }
                match defaults.iter_mut().find(|(k, _)| *k == nd) {
                    Some((_, of)) if below(self, i, of) => *of = i,
                    Some(_) => {}
                    None => defaults.push((nd, i)),
                }
            }
        }
        let through: Vec<&str> = adds
            .iter()
            .copied()
            .filter(|a| self.classes.contains_key(a))
            .collect();
        for (j, (nd, of)) in defaults.iter().enumerate() {
            let (name, desc) = nd.split_once('.').unwrap_or((nd.as_str(), ""));
            let _ = write!(
                code,
                "fn ai_{id}_{j}(rt: &mut Rt, args: &[V]) -> V {{\n    {}{}(rt, args)\n}}\n\n",
                self.ensure(of),
                fn_name(of, name, desc)
            );
            let owners: Vec<String> = targets.iter().map(|t| format!("{t}.{nd}")).collect();
            let checked: Vec<String> = through.iter().map(|a| format!("{a}.{nd}")).collect();
            let _ = writeln!(
                added,
                "        Added {{ key: {nd:?}, run: ai_{id}_{j}, owners: &{owners:?}, checked: &{checked:?}, targets: &{targets:?} }},"
            );
        }
        if list.is_empty() {
            return;
        }
        let ensure = self.ensure(&c.name);
        for (i, (nd, a)) in list.iter().enumerate() {
            let (name, desc) = nd.split_once('.').unwrap_or((nd.as_str(), ""));
            let _ = write!(code, "fn a_{id}_{i}(rt: &mut Rt, args: &[V]) -> V {{\n    {ensure}{init}{}(rt, args)\n}}\n\n", fn_name(&c.name, name, desc));
            let _ = writeln!(
                added,
                "        Added {{ key: {nd:?}, run: a_{id}_{i}, owners: &{:?}, checked: &{:?}, targets: &{:?} }},",
                a.owners, a.checked, a.targets
            );
        }
    }

    /// A lambda's call (`l_<lid>`'s body): its implementation `m` (handle kind `kind`) run on
    /// `full`, the values it runs with, as `LambdaMetafactory` adapts them.
    fn lambda_call(&self, key: &str, kind: u8, m: &Member) -> String {
        // A handler a mixin injects into a method of the game's, which the runtime calls on the
        // game's object where its own code for the method passes the place
        // (horizon-compat's `places`): the mixin's own code for it, on the object, the method's
        // arguments and the `CallbackInfo`. Its class is initialised first, and what the mixin
        // adds to the object's constructors has run, as for a method it adds. One at the end
        // of the constructors is itself part of that: after the field initialisers, once on
        // each object, whoever calls it.
        if kind == 5 && self.weave.native.contains_key(key) {
            let ensure = self.ensure(&m.owner);
            let handler = format!("{}(rt, &full)", fn_name(&m.owner, &m.name, &m.desc));
            let this = "full.first().copied().unwrap_or_default()";
            if self.at_constructors_end(key) {
                let fields = match self.mixin_fields(&m.owner) {
                    Some((name, _)) => format!("{name}(rt, this); "),
                    None => String::new(),
                };
                let ran = format!("$mixin {key}");
                return format!(
                    "{{ {ensure}let this = {this}; {fields}if rt.thrown.is_some() || matches!(rt.get_field(this, {ran:?}), V::I(1)) {{ V::Void }} else {{ rt.put_field(this, {ran:?}, V::I(1)); {handler} }} }}"
                );
            }
            let init = match self.mixin_init(&m.owner) {
                Some((name, _)) => {
                    format!("{name}(rt, {this}); if rt.thrown.is_some() {{ return V::Void; }} ")
                }
                None => String::new(),
            };
            return format!("{{ {ensure}{init}{handler} }}");
        }
        let nd = format!("{}.{}", m.name, m.desc);
        let call = match (kind, self.resolve(&m.owner, &m.name, &m.desc)) {
            (8, imp) => {
                let init = match imp {
                    Some(imp) => format!("{}(rt, &a);", fn_name(&imp, &m.name, &m.desc)),
                    None => format!("api::call(rt, {key:?}, &a);"),
                };
                // A constructor reference (`BeltBlock::new`) initialises the class as `new` does.
                format!(
                    "{{ {}let o = {}; let mut a = vec![o]; a.extend(full); {init} o }}",
                    self.ensure(&m.owner),
                    self.new_object(&m.owner)
                )
            }
            (5 | 9, _) if self.private_method(&m.owner, &m.name, &m.desc) => format!("{}(rt, &full)", fn_name(&m.owner, &m.name, &m.desc)),
            (5 | 9, _) => {
                // The API's key when no mod class implements it (as at call sites).
                let owner_key = match self.api_owner(&m.owner) {
                    Some(api) => format!("{api}.{nd}"),
                    None => key.to_owned(),
                };
                if self.vkeys.contains(&nd) {
                    format!("{{ let cid = rt.recv_cid(&full); {}(rt, cid, {owner_key:?}, &full) }}", dispatcher(&nd))
                } else {
                    format!("{{ let cid = rt.recv_cid(&full); vcall_miss(rt, cid, {owner_key:?}, {nd:?}, &full) }}")
                }
            }
            // A mixin's own static initialiser, heard where the runtime first answers for its
            // target ([`weave::native_place`]): the class is initialised, once, as anything else
            // that touches it would.
            (6, _) if m.name == "<clinit>" => format!("{{ {}V::Void }}", self.ensure(&m.owner)),
            // A static method reference to another class (`FanProcessingTypeRegistry::init`)
            // initialises the class that declares it, as `invokestatic` does. A lambda's own body
            // (`lambda$...`) lives in the class that made it, which is initialised already.
            (6, Some(imp)) if !m.name.starts_with("lambda$") => format!("{{ {}{}(rt, &full) }}", self.ensure(&imp), fn_name(&imp, &m.name, &m.desc)),
            (_, Some(imp)) => format!("{}(rt, &full)", fn_name(&imp, &m.name, &m.desc)),
            (_, None) => format!("api::call(rt, {key:?}, &full)"),
        };
        // `LambdaMetafactory`'s adaptation: a primitive parameter of the implementation takes the
        // value out of the box the interface method hands it, widened. The receiver of an instance
        // method comes first.
        let kinds = format!("{}{}", if matches!(kind, 5 | 7 | 9) { "L" } else { "" }, raw_kinds(&m.desc));
        if kinds.chars().any(|k| k != 'L') { format!("{{ rt.adapt_args(&mut full, {kinds:?}); {call} }}") } else { call }
    }

    pub fn emit(&mut self) -> String {
        let classes: Vec<&ClassFile> = self.classes.values().copied().collect();
        let part = self.part(&classes);
        let body = part.strip_prefix(PART_HEADER).unwrap_or(&part);
        let mut out = format!("{LIB_HEADER}use horizon_compat::{{api, Rt, V}};\nuse horizon_compat::linked::{{Added, ClassDef, Registry}};\n\n{body}");
        out.push_str(&self.tables(&classes));
        out.push_str(&self.own_links(&registries_in(&part)));
        out
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
        // A long block is cut into functions ([`cut`]) where nothing is woven: woven code names
        // what this function holds.
        let cuts = cur.weaves.is_empty();
        // And a big method into functions of several blocks each ([`ranges`]).
        let split = cuts && code.bytes.len() >= self.big_method.0;
        let sizes = (
            (code.max_locals as usize).max(1),
            usize::from(code.max_stack.max(1)),
        );
        let (mut block, mut block_at, mut pieces) = (Vec::new(), 0, String::new());
        let mut arms: Vec<(usize, String)> = Vec::new();
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
                block_at = i.at;
            }
            block.push(body);
            if !joined.contains(&next) {
                let mut arm = format!("            {block_at} => {{");
                if cuts && block.iter().map(String::len).sum::<usize>() > self.long_block.0 {
                    arm.push_str(&cut(
                        &f,
                        block_at,
                        &block,
                        self.long_block.1,
                        sizes,
                        split,
                        &mut pieces,
                    ));
                } else {
                    for body in &block {
                        let _ = write!(arm, " {body}");
                    }
                }
                arm.push_str(" }\n");
                arms.push((block_at, arm));
                block.clear();
            }
        }
        if split {
            s.push_str(&ranges(&f, &arms, self.big_method.1, sizes, &mut pieces));
        } else {
            for (_, arm) in &arms {
                s.push_str(arm);
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
        s.push_str(&pieces);
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
                "pub fn {op_fn}(rt: &mut Rt, all: &[V]) -> V {{\n    let arr = all.last().copied().unwrap_or_default();\n    let mut a: Vec<V> = all.get(..{recv}).unwrap_or_default().to_vec();\n    a.extend::<[V; {}]>([{}]);\n    let r = {body_name}(rt, &a);\n    {}\n}}\n",
                arg_kinds(desc).len(),
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
            // array loads: the element, or the JVM's exception for a null array or an index
            // outside it (`Rt::array_load` throws it and answers none).
            0x2e..=0x35 => format!(
                "let ix = {pop}.i(); let a = {pop}; match rt.array_load(a, ix) {{ Some(v) => s.push(v), None => {{ thrown_at = {}; pc = {THROW}; continue; }} }} {go}",
                i.at
            ),
            // stores
            0x36..=0x3a => format!("l[{idx}] = {pop}; {go}"),
            0x3b..=0x4e => format!("l[{}] = {pop}; {go}", (i.op - 0x3b) % 4),
            0x4f..=0x56 => format!(
                "let v = {pop}; let ix = {pop}.i(); let a = {pop}; if !rt.array_store(a, ix, v) {{ thrown_at = {}; pc = {THROW}; continue; }} {go}",
                i.at
            ),
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
            // dup2_x2, by what the top values are (JVMS 6.5): a long or double is one value here.
            0x5e => format!(
                "let a = {pop}; let b = {pop}; if matches!(a, V::J(_) | V::D(_)) {{ if matches!(b, V::J(_) | V::D(_)) {{ s.push(a); s.push(b); s.push(a); }} else {{ let c = {pop}; s.push(a); s.push(c); s.push(b); s.push(a); }} }} else {{ let c = {pop}; if matches!(c, V::J(_) | V::D(_)) {{ s.push(b); s.push(a); s.push(c); s.push(b); s.push(a); }} else {{ let d = {pop}; s.push(b); s.push(a); s.push(d); s.push(c); s.push(b); s.push(a); }} }} {go}"
            ),
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
                // An object of the game's own translated code, or of a class above which there
                // is some: marked as built here, its fields its own (`game_built_read`).
                if self.array_fields(&class).is_some() {
                    return format!("{init}let v = {}; s.push(v); {go}", self.new_object(&class));
                }
                let built = if self.game_built(&class) {
                    " rt.put_field(v, \"$vt\", V::I(1));"
                } else {
                    ""
                };
                format!(
                    "{init}let v = rt.new_object_id({class:?}, {});{built} s.push(v); {go}",
                    self.cid(&class)
                )
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

    /// The class declaring instance field `name` as a reference through `owner` finds it, when
    /// that field hides another: a class above the one declaring it (translated, or the game's:
    /// [`GAME_FIELDS`]) declares an instance field of the same name. Java keeps both, told apart
    /// by the declaring class: GeckoLib's `GeoArmorRenderer` has `head`, `body` and the limbs as
    /// bones, and `HumanoidModel` above it has them as model parts, which another mod reads
    /// through the model (`baseModel.head`); an anonymous class made of an inner class has its
    /// own `this$0` over the inner class's. None for a field that hides nothing, and for a
    /// mixin's, which is its target's (a `@Shadow`) or added to it.
    fn hiding_class(&self, owner: &str, name: &str) -> Option<&'a str> {
        let declares = |k: &ClassFile| {
            k.fields
                .iter()
                .any(|f| f.name == name && f.access & 0x0008 == 0)
        };
        // (The classes that carry the game's own methods have marks for fields, not fields.)
        if owner.starts_with(horizon_compat::linked::DERIVED) {
            return None;
        }
        let mut c = owner;
        let declaring: &'a ClassFile = loop {
            let k: &'a ClassFile = self.classes.get(c).copied()?;
            if let Some(target) = self.mixin_of.get(c) {
                if k.fields
                    .iter()
                    .any(|f| f.name == name && !is_shadow_field(f))
                {
                    return None;
                }
                c = target;
                continue;
            }
            if declares(k) {
                break k;
            }
            c = k.superclass.as_deref()?;
        };
        let mut above = declaring.superclass.as_deref();
        while let Some(class) = above {
            match self.classes.get(class) {
                Some(k) if declares(k) => return Some(declaring.name.as_str()),
                Some(k) => above = k.superclass.as_deref(),
                None => return game_declares(class, name).then_some(declaring.name.as_str()),
            }
        }
        None
    }

    /// The name an object keeps instance field `name` under, read or written through class
    /// `owner`: the field's name; for one that hides a field above it ([`Self::hiding_class`]),
    /// the name with the class declaring it (`head@software/bernie/.../GeoArmorRenderer`), so
    /// that the one above keeps the plain name the runtime and the game's own code know it by.
    fn instance_key(&self, owner: &str, name: &str) -> String {
        match self.hiding_class(owner, name) {
            Some(class) => horizon_compat::hidden_key(name, class),
            None => name.to_owned(),
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
        // A field of an object kept as an array of its fields ([`ARRAY_CLASSES`]) is its slot. One
        // the runtime made shorter (a matrix without the flags JOML keeps beside it) reads as zero
        // there: nothing known of it.
        if matches!(op, 0xb4 | 0xb5)
            && let Some(slot) = self.array_slot(&m.owner, &m.name)
        {
            let zero = zero_of(&m.desc).unwrap_or("V::NULL");
            return if op == 0xb4 {
                format!(
                    "let o = {pop}; s.push(match rt.array_get(o, {slot}) {{ V::Void => {zero}, v => v }}); {go}"
                )
            } else {
                format!("let v = {pop}; let o = {pop}; rt.array_set(o, {slot}, v); {go}")
            };
        }
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
        let game_read = if op == 0xb4 { self.game_field_read(&m, "o").or_else(|| self.game_built_read(&m, "o")) } else { None };
        let heard = if op == 0xb5 && !self.own(&self.field_owner(&m.owner, &m.name)) { heard_write(&m) } else { None };
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
            // (A field that hides one above it is kept under a name of its own.)
            0xb4 => match zero {
                Some(z) => format!(
                    "let o = {pop}; s.push(match rt.get_field(o, {:?}) {{ V::Void => {z}, v => v }}); {go}",
                    self.instance_key(&m.owner, &m.name)
                ),
                None => format!(
                    "let o = {pop}; s.push(rt.get_field(o, {:?})); {go}",
                    self.instance_key(&m.owner, &m.name)
                ),
            },
            // A game field a mapped method only sets: the write is that method's call (kept in
            // the object, it would hide what the game makes of the field afterwards).
            0xb5 if !self.own(&self.field_owner(&m.owner, &m.name))
                && accessor(&m.owner, &m.name, true).is_some() =>
            {
                format!(
                    "let v = {pop}; let o = {pop}; api::call(rt, {:?}, &[o, v]); {go}",
                    accessor(&m.owner, &m.name, true).unwrap_or_default()
                )
            }
            // A game field whose writes the runtime hears of: kept as any other, then told.
            _ if heard.is_some() => format!("let v = {pop}; let o = {pop}; rt.put_field(o, {:?}, v); api::call(rt, {:?}, &[o, v]); {go}", m.name, heard.unwrap_or_default()),
            _ => format!("let v = {pop}; let o = {pop}; rt.put_field(o, {:?}, v); {go}", self.instance_key(&m.owner, &m.name)),
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
        // The runtime has no state for the field, and a mapped method only returns it: that
        // method's answer, past the object's own state.
        if let Some(getter) = accessor(&owner, name, false) {
            return Some(format!(
                "match rt.get_field({o}, {name:?}) {{ V::Void => match rt.get_field({o}, {own:?}) {{ V::Void if {o}.r() != 0 => match api::call(rt, {getter:?}, &[{o}]) {{ V::Void => {zero}, v => v }}, V::Void => {zero}, v => v }}, v => v }}"
            ));
        }
        Some(format!("match api::game_field(rt, {o}, {name:?}, {own:?}, {key:?}) {{ V::Void => {zero}, v => v }}"))
    }

    /// Whether `class`, or a translated class above it, is of the game's own code
    /// ([`crate::game_code`]).
    /// The instance fields of `class`'s objects when the runtime keeps them as an array
    /// ([`ARRAY_CLASSES`]): each field's declaring class, name and descriptor, in slot order (the
    /// array class's own first, in the order it declares them, then each class's below it down to
    /// `class`). None for any other class.
    fn array_fields(&self, class: &str) -> Option<Vec<(&str, &str, &str)>> {
        let mut chain: Vec<&ClassFile> = Vec::new();
        let mut at = self.classes.get(class);
        loop {
            let c = at.filter(|_| chain.len() < 1024)?;
            chain.push(c);
            if ARRAY_CLASSES.contains(&c.name.as_str()) {
                break;
            }
            at = c.superclass.as_deref().and_then(|s| self.classes.get(s));
        }
        Some(
            chain
                .iter()
                .rev()
                .flat_map(|c| {
                    c.fields
                        .iter()
                        .filter(|f| f.access & 0x0008 == 0)
                        .map(|f| (c.name.as_str(), f.name.as_str(), f.desc.as_str()))
                })
                .collect(),
        )
    }

    /// The expression that makes a new object of `class` before its constructor runs: one kept
    /// as an array ([`Self::array_fields`]) with each field's slot at its zero.
    fn new_object(&self, class: &str) -> String {
        match self.array_fields(class) {
            Some(fields) => {
                let slots: Vec<&str> = fields
                    .iter()
                    .map(|(_, _, desc)| zero_of(desc).unwrap_or("V::NULL"))
                    .collect();
                format!(
                    "rt.alloc_cid({class:?}, {}, horizon_compat::Data::Array(vec![{}]))",
                    self.cid(class),
                    slots.join(", ")
                )
            }
            None => format!("rt.new_object_id({class:?}, {})", self.cid(class)),
        }
    }

    /// The slot of instance field `name`, read through class `class`, in an object kept as an
    /// array ([`Self::array_fields`]); None for a field of any other object.
    fn array_slot(&self, class: &str, name: &str) -> Option<usize> {
        let owner = self.field_owner(class, name);
        self.array_fields(&owner)?
            .iter()
            .position(|(c, n, _)| *c == owner && *n == name)
    }

    fn game_built(&self, class: &str) -> bool {
        let mut at = self.classes.get(class);
        let mut steps = 0;
        while let Some(c) = at.filter(|_| steps < 1024) {
            if crate::game_code(&c.name) {
                return true;
            }
            steps += 1;
            at = c.superclass.as_deref().and_then(|s| self.classes.get(s));
        }
        false
    }

    /// Reading instance field `m` of object `o` when a class of the game's own translated code
    /// declares it: the object's own, when translated code built it (`$vt`, set where it is
    /// made); an object the runtime made of that class has no such state, and the read asks the
    /// runtime as a read of any game field does ([`Self::game_field_read`]), so nothing reads as
    /// a silent zero. None for any other field.
    fn game_built_read(&self, m: &Member, o: &str) -> Option<String> {
        let owner = self.field_owner(&m.owner, &m.name);
        if !self.own(&owner) || !crate::game_code(&owner) {
            return None;
        }
        let zero = match m.desc.as_str() {
            "I" | "Z" | "B" | "S" | "C" => "V::I(0)",
            "J" => "V::J(0)",
            "F" => "V::F(0.0)",
            "D" => "V::D(0.0)",
            _ => "V::Void",
        };
        let (name, own, key) = (
            &m.name,
            format!("${}", m.name),
            format!("{owner}.${}.(){}", m.name, m.desc),
        );
        // (Its own state is where the write keeps it: under a name of its own for a field that
        // hides one above, as an enum's `name` hides `Enum.name`.)
        let stored = self.instance_key(&m.owner, &m.name);
        Some(format!(
            "match rt.get_field({o}, {stored:?}) {{ V::Void if matches!(rt.get_field({o}, \"$vt\"), V::Void) => match api::game_field(rt, {o}, {name:?}, {own:?}, {key:?}) {{ V::Void => {zero}, v => v }}, V::Void => {zero}, v => v }}"
        ))
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
            0xb4 => format!(
                "match rt.get_field(args[0], {:?}) {{ V::Void => {zero}, v => v }}",
                self.instance_key(&m.owner, &m.name)
            ),
            _ => format!(
                "{{ rt.put_field(args[0], {:?}, args[1]); V::Void }}",
                self.instance_key(&m.owner, &m.name)
            ),
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
                    "match rt.get_field(args[0], {own:?}) {{ V::Void => match rt.get_field(args[0], {kept:?}) {{ V::Void => api::call(rt, {key:?}, &[args[0]]), v => v }}, v => v }}",
                    own = format!("${field}"),
                    kept = self.instance_key(&target, &field),
                    key = format!("{target}.${field}.{}", m.desc)
                ),
                (false, true) => format!(
                    "{{ let own = !matches!(rt.get_field(args[0], {own:?}), V::Void) || matches!(rt.get_field(args[0], {kept:?}), V::Void); rt.put_field(args[0], if own {{ {own:?} }} else {{ {kept:?} }}, args[1]); V::Void }}",
                    own = format!("${field}"),
                    kept = self.instance_key(&target, &field)
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
                    "{{ {}let o = {}; let mut a = vec![o]; a.extend_from_slice(&args); {}(rt, &a); o }}",
                    self.ensure(&target),
                    self.new_object(&target),
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
        // (So does `Thread.getStackTrace()`, whose answer begins with the class that asked.)
        let asks_stack = m.owner == "java/lang/Thread" && m.name == "getStackTrace";
        let caller = if asks_stack
            || self
                .caller_sensitive
                .contains(&format!("{}.{}", m.name, m.desc))
        {
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
mod invoker_tests {
    use crate::class::ClassFile;
    use std::process::Command;

    /// A static `@Invoker` in an accessor mixin has a body only for `javac`. The method it names
    /// stays its class's own, for that class's callers and for the invoker's alike.
    #[test]
    fn an_invokers_placeholder_does_not_stand_for_the_method_it_names() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("horizon-invoker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            (
                "org/spongepowered/asm/mixin/Mixin.java",
                "package org.spongepowered.asm.mixin; import java.lang.annotation.*; @Retention(RetentionPolicy.CLASS) public @interface Mixin { Class<?>[] value() default {}; }",
            ),
            (
                "org/spongepowered/asm/mixin/gen/Invoker.java",
                "package org.spongepowered.asm.mixin.gen; import java.lang.annotation.*; @Retention(RetentionPolicy.RUNTIME) public @interface Invoker { String value() default \"\"; }",
            ),
            (
                "t/Shaper.java",
                "package t; public class Shaper { static Shaper make() { return new Shaper(); } }",
            ),
            (
                "t/ShaperAccessor.java",
                "package t; @org.spongepowered.asm.mixin.Mixin(Shaper.class) public interface ShaperAccessor { @org.spongepowered.asm.mixin.gen.Invoker(\"make\") static Shaper make() { throw new AssertionError(); } }",
            ),
            (
                "t/User.java",
                "package t; public class User { static Object own() { return Shaper.make(); } static Object through() { return ShaperAccessor.make(); } }",
            ),
        ];
        for (path, src) in files {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        let classes: Vec<ClassFile> = ["t/Shaper.class", "t/ShaperAccessor.class", "t/User.class"]
            .iter()
            .map(|c| ClassFile::parse(&std::fs::read(root.join(c)).unwrap()).unwrap())
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        let mut p = super::Program::new(&classes);
        assert_eq!(
            p.resolve("t/Shaper", "make", "()Lt/Shaper;").as_deref(),
            Some("t/Shaper")
        );
        let module = p.emit();
        // Both callers reach `Shaper.make`'s own code, and nothing calls the placeholder.
        let own = super::fn_name("t/Shaper", "make", "()Lt/Shaper;");
        let placeholder = super::fn_name("t/ShaperAccessor", "make", "()Lt/Shaper;");
        let body = |f: &str| {
            let at = module
                .find(&format!("pub fn {f}("))
                .unwrap_or_else(|| panic!("{f} is not written"));
            &module[at..at + module[at..].find("\n}\n").unwrap()]
        };
        for user in ["own", "through"] {
            let text = body(&super::fn_name("t/User", user, "()Ljava/lang/Object;"));
            assert!(
                text.contains(&format!("{own}(rt")),
                "{user} does not call Shaper.make:\n{text}"
            );
            assert!(
                !text.contains(&format!("{placeholder}(rt")),
                "{user} calls the placeholder:\n{text}"
            );
        }
        assert!(p.unsupported.is_empty(), "{:?}", p.unsupported);
    }
}

#[cfg(test)]
mod loading_tests {
    use crate::class::ClassFile;
    use std::process::Command;

    /// FML's loading, as `start` does it: the mods' classes are constructed in the order the mods
    /// load in, a mod's class for both sides before its client's, and a dedicated server's entry
    /// class not at all; each `@EventBusSubscriber` class is subscribed for the mod it names (else
    /// its jar's), unless it is for a dedicated server only.
    #[test]
    fn mods_are_constructed_in_loading_order_on_the_clients_side() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("horizon-loading-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            (
                "net/neoforged/api/distmarker/Dist.java",
                "package net.neoforged.api.distmarker; public enum Dist { CLIENT, DEDICATED_SERVER }",
            ),
            (
                "net/neoforged/fml/common/Mod.java",
                "package net.neoforged.fml.common; import java.lang.annotation.*; import net.neoforged.api.distmarker.Dist; @Retention(RetentionPolicy.RUNTIME) public @interface Mod { String value(); Dist[] dist() default {Dist.CLIENT, Dist.DEDICATED_SERVER}; }",
            ),
            (
                "net/neoforged/fml/common/EventBusSubscriber.java",
                "package net.neoforged.fml.common; import java.lang.annotation.*; import net.neoforged.api.distmarker.Dist; @Retention(RetentionPolicy.RUNTIME) public @interface EventBusSubscriber { Dist[] value() default {Dist.CLIENT, Dist.DEDICATED_SERVER}; String modid() default \"\"; }",
            ),
            (
                "t/Addon.java",
                "package t; @net.neoforged.fml.common.Mod(\"addon\") public class Addon { public Addon() {} }",
            ),
            (
                "t/Base.java",
                "package t; @net.neoforged.fml.common.Mod(\"base\") public class Base { public Base() {} }",
            ),
            (
                "t/AClient.java",
                "package t; @net.neoforged.fml.common.Mod(value = \"base\", dist = net.neoforged.api.distmarker.Dist.CLIENT) public class AClient { public AClient() {} }",
            ),
            (
                "t/BaseServer.java",
                "package t; @net.neoforged.fml.common.Mod(value = \"base\", dist = net.neoforged.api.distmarker.Dist.DEDICATED_SERVER) public class BaseServer { public BaseServer() {} }",
            ),
            (
                "t/Events.java",
                "package t; @net.neoforged.fml.common.EventBusSubscriber(modid = \"addon\") public class Events {}",
            ),
            (
                "t/BaseEvents.java",
                "package t; @net.neoforged.fml.common.EventBusSubscriber public class BaseEvents {}",
            ),
            (
                "t/ServerEvents.java",
                "package t; @net.neoforged.fml.common.EventBusSubscriber(net.neoforged.api.distmarker.Dist.DEDICATED_SERVER) public class ServerEvents {}",
            ),
        ];
        for (path, src) in files {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        let names = [
            "t/Addon",
            "t/Base",
            "t/AClient",
            "t/BaseServer",
            "t/Events",
            "t/BaseEvents",
            "t/ServerEvents",
        ];
        let classes: Vec<ClassFile> = names
            .iter()
            .map(|c| {
                ClassFile::parse(&std::fs::read(root.join(format!("{c}.class"))).unwrap()).unwrap()
            })
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        let mut p = super::Program::new(&classes);
        p.mod_id = Some("addon".to_owned());
        p.load_order = vec!["base".to_owned(), "addon".to_owned()];
        p.scan_files = vec![(
            vec!["base".to_owned()],
            names.iter().map(|n| (*n).to_owned()).collect(),
        )];
        let module = p.emit();
        let start = &module[module
            .find("pub fn start(rt: &mut Rt)")
            .expect("start is written")..];
        let constructed: Vec<&str> = start
            .match_indices("\"the constructor of ")
            .map(|(at, m)| start[at + m.len()..].split('"').next().unwrap())
            .collect();
        assert_eq!(constructed, ["t/Base", "t/AClient", "t/Addon"]);
        let subscribed: Vec<&str> = start
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix("{ let a = [rt.string(")
                    .filter(|l| l.contains("hz/loading.subscribe."))
            })
            .map(|l| l.split("]; ").next().unwrap_or(""))
            .collect();
        assert_eq!(
            subscribed,
            [
                "\"t/BaseEvents\"), rt.string(\"base\")",
                "\"t/Events\"), rt.string(\"addon\")"
            ]
        );
        assert!(
            !start.contains("ServerEvents"),
            "a dedicated server's subscriber is subscribed"
        );
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
mod reflect_tests {
    use crate::class::ClassFile;
    use std::process::Command;

    /// Reflection's entries for a class's static methods: one some bytecode calls (through
    /// the class below it too), one whose name a class holds as text, one with an annotation
    /// reflection sees. Not one nothing of the mods can name, nor what the compiler made.
    #[test]
    fn reflection_runs_the_static_methods_code_can_name() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("mod-translator-statics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("m")).unwrap();
        let files = [
            (
                "m/Marked.java",
                "package m; import java.lang.annotation.*; @Retention(RetentionPolicy.RUNTIME) public @interface Marked {}",
            ),
            (
                "m/Tools.java",
                "package m; public class Tools { public static int called() { return 1; } public static int byName() { return 2; } @Marked static int marked() { return 3; } public static int unused() { return 4; } static Runnable made() { return () -> {}; } public int own() { return 5; } }",
            ),
            ("m/Sub.java", "package m; public class Sub extends Tools {}"),
            (
                "m/User.java",
                "package m; public class User { int use() throws Exception { return Sub.called() + Tools.class.getMethod(\"byName\").getModifiers(); } }",
            ),
        ];
        for (path, src) in files {
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        let classes: Vec<ClassFile> = [
            "m/Marked.class",
            "m/Tools.class",
            "m/Sub.class",
            "m/User.class",
        ]
        .iter()
        .map(|c| ClassFile::parse(&std::fs::read(root.join(c)).unwrap()).unwrap())
        .collect();
        let _ = std::fs::remove_dir_all(&root);
        let p = super::Program::new(&classes);
        let (_, code) = p.reflect_fn(&classes[1]).expect("its reflection");
        let listed: Vec<&str> = code
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix('"')?
                    .split_once("\" => ")
                    .map(|(key, _)| key)
            })
            .collect();
        assert_eq!(
            listed,
            ["<init>.()V", "called.()I", "byName.()I", "marked.()I"],
            "{code}"
        );
        // The class below has no static method of its own: nothing to list.
        assert!(
            p.reflect_fn(&classes[2])
                .is_none_or(|(_, code)| !code.contains("\"s:\""))
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

    /// A field that hides one of the same name above it is another field, kept under a name of
    /// its own: over a translated class's, and over the game's (told by the game's own jars).
    /// The one above keeps the plain name; a static of the same name hides nothing; a class
    /// below the one declaring it finds it through that class. Reflection is told which
    /// (`0 <hides> ...` among the class's fields).
    #[test]
    fn a_field_that_hides_one_above_is_kept_apart() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("mod-translator-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("gapi")).unwrap();
        std::fs::create_dir_all(root.join("m")).unwrap();
        let files = [
            (
                "gapi/Model.java",
                "package gapi; public class Model { public Object head; public int size; public int plain; }",
            ),
            (
                "m/Armor.java",
                "package m; public class Armor extends gapi.Model { String head = \"bone\"; int count; static int size; \
                 String own() { return head; } Object above() { return ((gapi.Model) this).head; } int inherited() { return plain; } }",
            ),
            (
                "m/Plate.java",
                "package m; public class Plate extends Armor { int count; int both() { return count + ((Armor) this).count; } String through() { return head; } }",
            ),
        ];
        for (path, src) in files {
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        // The game's class isn't the mod's: its fields are known from the game's jars alone.
        let classes: Vec<ClassFile> = ["m/Armor.class", "m/Plate.class"]
            .iter()
            .map(|c| ClassFile::parse(&std::fs::read(root.join(c)).unwrap()).unwrap())
            .collect();
        let game = crate::derive::Game::of(vec![
            ClassFile::parse(&std::fs::read(root.join("gapi/Model.class")).unwrap()).unwrap(),
        ]);
        let _ = std::fs::remove_dir_all(&root);
        let above = crate::derive::fields_above(&classes, &game);
        assert_eq!(above.keys().collect::<Vec<_>>(), ["gapi/Model"]);
        assert_eq!(
            above["gapi/Model"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["head", "plain", "size"]
        );
        super::set_game_fields(above);
        let mut p = super::Program::new(&classes);
        assert_eq!(p.hiding_class("m/Armor", "head"), Some("m/Armor"));
        assert_eq!(p.hiding_class("m/Plate", "head"), Some("m/Armor"));
        assert_eq!(p.hiding_class("m/Plate", "count"), Some("m/Plate"));
        assert_eq!(p.hiding_class("m/Armor", "count"), None);
        // `Armor.size` is a static: the game's instance field of that name is found past it.
        assert_eq!(p.hiding_class("m/Armor", "size"), None);
        assert_eq!(p.hiding_class("m/Armor", "plain"), None);
        assert_eq!(p.instance_key("m/Plate", "head"), "head@m/Armor");
        assert_eq!(p.instance_key("m/Plate", "count"), "count@m/Plate");
        assert_eq!(p.instance_key("m/Armor", "count"), "count");
        let code = p.emit();
        // The mod's own `head`, through either class; the game's stays the plain one.
        assert!(
            code.contains(r#"rt.get_field(o, "head@m/Armor")"#),
            "{code}"
        );
        assert!(code.contains(r#"rt.put_field(o, "head@m/Armor", v)"#));
        assert!(code.contains(r#"api::game_field(rt, o, "head", "$head""#));
        assert!(
            !code.contains(r#"rt.get_field(o, "head")"#)
                && !code.contains(r#"rt.put_field(o, "head", v)"#)
        );
        assert!(
            code.contains(r#"rt.get_field(o, "count@m/Plate")"#)
                && code.contains(r#"rt.get_field(o, "count")"#)
        );
        assert!(code.contains("0 <hides> head\\n"), "{code}");
        assert!(code.contains("0 <hides> count\\n"));
    }

    /// A class of the game's own translated code reads a field of its own that hides one above
    /// where its constructor wrote it (vanilla's `OctahedralGroup.name` over `Enum.name`: read
    /// under the plain name, `getSerializedName()` answered null). An object the runtime made
    /// of the class is still asked by the field's plain name.
    #[test]
    fn a_game_classs_hiding_field_is_read_where_it_is_written() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("mod-translator-game-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // (A class of the game's by its package, named here in parts: one this source named
        // whole would be the runtime's to model, not code to translate.)
        let folder = ["net", "minecraft", "hztest"].join("/");
        let package = root.join(&folder);
        std::fs::create_dir_all(&package).unwrap();
        let files = [
            (
                "Base.java",
                "package net.minecraft.hztest; public class Base { protected String name = \"base\"; String above() { return name; } }",
            ),
            (
                "Turn.java",
                "package net.minecraft.hztest; public class Turn extends Base { private final String name; Turn(String n) { name = n; } String own() { return name; } }",
            ),
        ];
        for (path, src) in files {
            std::fs::write(package.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| package.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        let classes: Vec<ClassFile> = ["Base.class", "Turn.class"]
            .iter()
            .map(|c| ClassFile::parse(&std::fs::read(package.join(c)).unwrap()).unwrap())
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        let turn = format!("{folder}/Turn");
        assert!(crate::game_code(&turn));
        let mut p = super::Program::new(&classes);
        let code = p.emit();
        let stored = format!("name@{turn}");
        assert!(
            code.contains(&format!("rt.put_field(o, {stored:?}, v)")),
            "{code}"
        );
        let asked = r#"{ V::Void if matches!(rt.get_field(o, "$vt"), V::Void) => match api::game_field(rt, o, "name", "$name""#;
        assert!(
            code.contains(&format!("match rt.get_field(o, {stored:?}) {asked}")),
            "{code}"
        );
        // The class above reads its own under the plain name.
        assert!(code.contains(&format!("match rt.get_field(o, \"name\") {asked}")));
    }

    /// A class added between two others takes an id past theirs when the earlier translation's
    /// ids are kept, so what the others' crates say of themselves stays the same text.
    #[test]
    fn kept_ids_number_only_what_is_new() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("horizon-kept-ids-{}", std::process::id()));
        std::fs::create_dir_all(root.join("m")).unwrap();
        let files = [
            (
                "m/A.java",
                "package m; public class A { Runnable r() { return () -> new C(); } }",
            ),
            (
                "m/B.java",
                "package m; public class B { Runnable r() { return () -> new A(); } }",
            ),
            (
                "m/C.java",
                "package m; public class C { Runnable r() { return () -> new C(); } }",
            ),
        ];
        for (path, src) in files {
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        let class = |c: &str| ClassFile::parse(&std::fs::read(root.join(c)).unwrap()).unwrap();
        let (two, three) = (
            vec![class("m/A.class"), class("m/C.class")],
            vec![class("m/A.class"), class("m/B.class"), class("m/C.class")],
        );
        let _ = std::fs::remove_dir_all(&root);
        let mut first = super::Program::new(&two);
        let before = first.emit();
        let ids = first.ids();
        assert!(
            ids.contains("c 1 m/A\n") && ids.contains("c 2 m/C\n"),
            "{ids}"
        );
        // Numbered afresh, the class in the middle takes the last one's id.
        let mut afresh = super::Program::new(&three);
        afresh.emit();
        assert!(afresh.ids().contains("c 2 m/B\n") && afresh.ids().contains("c 3 m/C\n"));
        let mut kept = super::Program::new(&three);
        kept.keep_ids(&ids);
        let after = kept.emit();
        let now = kept.ids();
        assert!(
            now.contains("c 1 m/A\n") && now.contains("c 2 m/C\n") && now.contains("c 3 m/B\n"),
            "{now}"
        );
        for line in ids.lines() {
            assert!(
                now.lines().any(|l| l == line),
                "{line} is not kept in\n{now}"
            );
        }
        assert_eq!(now.lines().filter(|l| l.starts_with("l ")).count(), 3);
        // What the first program said of `C` it says again: its objects' id, its lambda's.
        let mentions = |text: &str| {
            text.lines()
                .filter(|l| l.contains("\"m/C\"") && l.contains("new_object_id"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert!(!mentions(&before).is_empty());
        assert!(mentions(&before).iter().all(|l| after.contains(l.as_str())));
        // A class the next translation lacks keeps its id for when it is back.
        let mut without = super::Program::new(&two);
        without.keep_ids(&now);
        without.emit();
        assert!(without.ids().contains("c 3 m/B\n"));
        // And a part's registry is named by its first class, whatever was emitted before it.
        assert_eq!(super::registries_in(&before), super::registries_in(&after));
    }

    /// A write to `Entity.hurtMarked` is kept, then told to the runtime (a server sends the marked
    /// player their motion), through whichever class the mod names it by (`ServerPlayer`, its own
    /// entity's); a field of the mod's own of the same name is only kept.
    #[test]
    fn a_write_to_hurt_marked_is_told_to_the_runtime() {
        if Command::new("javac").arg("-version").output().is_err() {
            eprintln!("no JDK: skipped");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("mod-translator-marked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "net/minecraft/world/entity",
            "net/minecraft/server/level",
            "m",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let files = [
            (
                "net/minecraft/world/entity/Entity.java",
                "package net.minecraft.world.entity; public class Entity { public boolean hurtMarked; }",
            ),
            (
                "net/minecraft/server/level/ServerPlayer.java",
                "package net.minecraft.server.level; public class ServerPlayer extends net.minecraft.world.entity.Entity {}",
            ),
            (
                "m/Belt.java",
                "package m; public class Belt { void carry(net.minecraft.server.level.ServerPlayer p) { p.hurtMarked = true; } }",
            ),
            (
                "m/Own.java",
                "package m; public class Own { boolean hurtMarked; void mark() { this.hurtMarked = true; } }",
            ),
        ];
        for (path, src) in files {
            std::fs::write(root.join(path), src).unwrap();
        }
        let paths: Vec<_> = files.iter().map(|(p, _)| root.join(p)).collect();
        assert!(
            Command::new("javac")
                .args(["-d", root.to_str().unwrap()])
                .args(&paths)
                .status()
                .unwrap()
                .success()
        );
        let parse = |c: &str| ClassFile::parse(&std::fs::read(root.join(c)).unwrap()).unwrap();
        let (belt, own) = ([parse("m/Belt.class")], [parse("m/Own.class")]);
        let _ = std::fs::remove_dir_all(&root);
        let told =
            r#"api::call(rt, "net/minecraft/world/entity/Entity.$hurtMarked.(Z)V", &[o, v])"#;
        let code = super::Program::new(&belt).emit();
        assert!(
            code.contains(r#"rt.put_field(o, "hurtMarked", v); "#) && code.contains(told),
            "{code}"
        );
        let code = super::Program::new(&own).emit();
        assert!(
            code.contains(r#"rt.put_field(o, "hurtMarked", v); "#) && !code.contains(told),
            "{code}"
        );
    }
}
