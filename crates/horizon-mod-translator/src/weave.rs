//! Mixins into translated classes, woven by the translator itself. A mixin whose target is one of
//! the classes being translated (an addon's mixin into Create's `KineticBlockEntity`, say) needs
//! no game hook: the translator has both the mixin's code and its target's, so it does what Mixin
//! does at class load, as it writes the target's Rust. Its methods join the target (methods it
//! adds, `@Overwrite`s: `Program::resolve`), its field initialisers run in the target's
//! constructors, and each injector becomes a call to its handler at the place its `@At` names:
//!
//! - `@Inject` at `HEAD`, `RETURN`, `TAIL` or a call (`INVOKE`, before or `shift = AFTER` it, or
//!   `shift = BY` some of ASM's nodes on), with a `CallbackInfo` (`CallbackInfoReturnable` for a
//!   value) that may end the method early;
//! - `@Redirect` of a call (the handler instead of it), `@ModifyArg` (one argument through the
//!   handler), MixinExtras' `@ModifyExpressionValue` (the call's result through it),
//!   `@WrapWithCondition` (the call only if the handler says so) and `@ModifyReturnValue`.
//!
//! MixinExtras' `@WrapOperation` (a call through the handler, which gets the call as an
//! `Operation` it may make) and `@WrapMethod` (the whole method so) too. What it doesn't weave
//! (other `@At`s, captured locals...) is counted in the plan's `unsupported`, as the codegen counts
//! what it can't translate.
use crate::class::{Annotation, ClassFile, Member, Method};
use std::collections::{BTreeMap, BTreeSet};

const MIXIN: &str = "Lorg/spongepowered/asm/mixin/";
const EXTRAS: &str = "Lcom/llamalad7/mixinextras/";
pub const CALLBACK_INFO: &str = "org/spongepowered/asm/mixin/injection/callback/CallbackInfo";
pub const RETURNABLE: &str = "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable";

/// What a handler does at its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// `@Inject`: called with the target method's arguments (or none) and a callback.
    Inject { cancellable: bool },
    /// `@Redirect`: called instead of the call, with its receiver and arguments.
    Redirect,
    /// `@ModifyArg`: the call's argument `index` (its receiver not counted) goes through it.
    ModifyArg { index: usize },
    /// `@ModifyExpressionValue`: the call's result goes through it.
    ModifyExpressionValue,
    /// `@WrapWithCondition`: the call happens only if it returns true.
    WrapWithCondition,
    /// `@ModifyReturnValue`: the value the method returns goes through it.
    ModifyReturnValue,
    /// `@WrapOperation`: called instead of the call, with its receiver, arguments and the call as
    /// an `Operation`.
    WrapOperation,
    /// `@WrapMethod`: called instead of the method, with its arguments and the method as an
    /// `Operation`.
    WrapMethod,
    /// `@ModifyVariable`: a local (picked as `@Local` picks one, by the handler's type) goes
    /// through the handler at the place, and back into its slot.
    ModifyVariable { local: Sugar },
    /// MixinExtras' `@ModifyReceiver`: a call's receiver goes through the handler, which also
    /// gets the call's arguments.
    ModifyReceiver,
    /// `@ModifyConstant`: a constant goes through the handler (with the method's arguments when
    /// it takes them).
    ModifyConstant,
}

/// A constant an `@At("CONSTANT")` (or a `@ModifyConstant`'s `@Constant`) names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constant {
    Null,
    Int(i32),
    /// Its bits, as Java's `Float.equals` compares.
    Float(u32),
    Long(i64),
    Double(u64),
    String(String),
    /// A class literal, by internal name.
    Class(String),
    /// Any constant of this type (a descriptor): a `@ModifyConstant` without a value takes the
    /// constants of its handler's type.
    OfType(String),
}

impl Constant {
    /// The type of the constant, as Mixin's `Bytecode.getConstantType` gives it.
    fn type_desc(&self) -> &str {
        match self {
            Constant::Null => "Ljava/lang/Object;",
            Constant::Int(_) => "I",
            Constant::Float(_) => "F",
            Constant::Long(_) => "J",
            Constant::Double(_) => "D",
            Constant::String(_) => "Ljava/lang/String;",
            Constant::Class(_) => "Ljava/lang/Class;",
            Constant::OfType(t) => t,
        }
    }

    /// Whether the constant an instruction pushes (`found`) is one this names.
    pub fn matches(&self, found: &Constant) -> bool {
        match self {
            Constant::OfType(t) => found.type_desc() == t,
            _ => self == found,
        }
    }
}

/// The constant instruction `i` (of `c`) pushes: `aconst_null`, `iconst_*`... `bipush`, `sipush`,
/// `ldc` of a number, string or class; and the class an `instanceof` or `checkcast` names, which
/// Mixin's `classValue` matches too.
pub fn constant_of(c: &ClassFile, i: &crate::insn::Insn) -> Option<Constant> {
    use crate::class::Const;
    let idx = match &i.arg {
        crate::insn::Operand::Int(v) => *v,
        _ => 0,
    };
    Some(match i.op {
        0x01 => Constant::Null,
        0x02..=0x08 => Constant::Int(i.op as i32 - 3),
        0x09 | 0x0a => Constant::Long(i64::from(i.op - 0x09)),
        0x0b..=0x0d => Constant::Float(f32::from(i.op - 0x0b).to_bits()),
        0x0e | 0x0f => Constant::Double(f64::from(i.op - 0x0e).to_bits()),
        0x10 | 0x11 => Constant::Int(idx),
        0x12..=0x14 => match c.pool.get(idx as usize)? {
            Const::Int(v) => Constant::Int(*v),
            Const::Float(v) => Constant::Float(v.to_bits()),
            Const::Long(v) => Constant::Long(*v),
            Const::Double(v) => Constant::Double(v.to_bits()),
            Const::String(u) => Constant::String(c.utf8(*u).ok()?),
            Const::Class(n) => Constant::Class(c.utf8(*n).ok()?),
            _ => return None,
        },
        0xc0 | 0xc1 => Constant::Class(c.class_name(idx as u16).ok()?),
        _ => return None,
    })
}

impl Kind {
    /// Its annotation's name.
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Inject { .. } => "@Inject",
            Kind::Redirect => "@Redirect",
            Kind::ModifyArg { .. } => "@ModifyArg",
            Kind::ModifyExpressionValue => "@ModifyExpressionValue",
            Kind::WrapWithCondition => "@WrapWithCondition",
            Kind::ModifyReturnValue => "@ModifyReturnValue",
            Kind::WrapOperation => "@WrapOperation",
            Kind::WrapMethod => "@WrapMethod",
            Kind::ModifyVariable { .. } => "@ModifyVariable",
            Kind::ModifyReceiver => "@ModifyReceiver",
            Kind::ModifyConstant => "@ModifyConstant",
        }
    }
}

/// MixinExtras' `Operation`: what a wrapping handler calls to run what it wraps.
pub const OPERATION: &str = "com/llamalad7/mixinextras/injector/wrapoperation/Operation";

/// Where in the target method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    Head,
    /// Before each return (the `n`th when `ordinal` says).
    Return { ordinal: Option<usize> },
    /// Before the last return.
    Tail,
    /// A call to `owner.name desc` (the `n`th such call when `ordinal` says), before it or after;
    /// an empty owner is any owner (a target written without one). `assign` (`INVOKE_ASSIGN`):
    /// after it and after the store of its result into a local that follows it, if one does.
    Invoke { owner: String, name: String, desc: String, ordinal: Option<usize>, after: bool, assign: bool },
    /// A read or write of field `owner.name` of type `desc` (`opcode` picks `getfield`...; the
    /// `n`th such access when `ordinal` says), before it or after.
    Field { owner: String, name: String, desc: String, opcode: Option<u8>, ordinal: Option<usize>, after: bool },
    /// After a store into the local a `@ModifyVariable` modifies (the `n`th such store when
    /// `ordinal` says).
    Store { ordinal: Option<usize> },
    /// A constant pushed (`ldc`, `iconst_1`, `bipush`...), the `n`th such when `ordinal` says.
    Constant { value: Constant, ordinal: Option<usize> },
    /// An object made: a `new` of `class` (whose constructor, called after it, is `desc` when
    /// given), the `n`th `new` of the class when `ordinal` says.
    New { class: String, desc: Option<String>, ordinal: Option<usize> },
    /// `by` nodes on from where `at` is (`shift = BY`), counting ASM's nodes as Mixin does: the
    /// instructions, and the labels, line numbers and frames between them ([`nodes`]).
    Shifted { at: Box<At>, by: i32 },
}

impl At {
    /// Its type, as `@At` names it.
    pub fn name(&self) -> &'static str {
        match self {
            At::Head => "HEAD",
            At::Return { .. } => "RETURN",
            At::Tail => "TAIL",
            At::Invoke { assign: true, .. } => "INVOKE_ASSIGN",
            At::Invoke { .. } => "INVOKE",
            At::Field { .. } => "FIELD",
            At::Store { .. } => "STORE",
            At::Constant { .. } => "CONSTANT",
            At::New { .. } => "NEW",
            At::Shifted { at, .. } => at.name(),
        }
    }

    /// Whether instruction `op` naming `m` (a call, or a field access) is one this `@At` names.
    /// Mixin compares the owner as the instruction writes it, not the class that declares it.
    pub fn names(&self, op: u8, m: &Member) -> bool {
        match self {
            At::Invoke { owner, name, desc, .. } => matches!(op, 0xb6..=0xb9) && (owner.is_empty() || *owner == m.owner) && *name == m.name && *desc == m.desc,
            At::Field { owner, name, desc, opcode, .. } => {
                matches!(op, 0xb2..=0xb5) && opcode.is_none_or(|o| o == op) && (owner.is_empty() || *owner == m.owner) && *name == m.name && (desc.is_empty() || *desc == m.desc)
            }
            _ => false,
        }
    }

    /// Whether it is at an instruction (a call, a field access, a constant, a `new`) rather than
    /// the method's ends.
    pub fn at_instruction(&self) -> bool {
        matches!(self, At::Invoke { .. } | At::Field { .. } | At::Constant { .. } | At::New { .. } | At::Shifted { .. })
    }
}

/// One handler woven into one target method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weave {
    pub mixin: String,
    pub handler: String,
    pub handler_desc: String,
    pub handler_static: bool,
    pub kind: Kind,
    pub at: At,
    /// Its last parameters that are locals of the target method: an `@Inject`'s captured ones
    /// (`locals = LocalCapture...`), then MixinExtras' `@Local`s and `@Share`s.
    pub sugar: Vec<Sugar>,
    /// What those parameters get at each place it goes (the offset of the call, the return, or 0
    /// for the head), in their order.
    pub locals: BTreeMap<usize, Vec<LocalArg>>,
    /// A `@ModifyVariable`'s local at each place: the slot it changes.
    pub var: BTreeMap<usize, u16>,
    /// The places of a `shift = BY` it goes after the instruction at rather than before: those
    /// that land on the label after it (a jump there skips the handler).
    pub after: BTreeSet<usize>,
}

impl Weave {
    /// The handler's parameters Mixin itself passes: all but [`Weave::sugar`]'s.
    pub fn handler_params(&self) -> usize {
        crate::codegen::arg_kinds(&self.handler_desc).len() - self.sugar.len()
    }
}

/// A handler parameter that is a local of the target method where the handler goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sugar {
    /// `@Inject(locals = ...)`: the next local after the method's arguments (and those before it
    /// taken), of type `ty`.
    Captured { ty: String },
    /// MixinExtras' `@Local`: the local of type `ty` that is the `ordinal`th of its type, or in
    /// `index`, or named one of `names`, or else the only one of its type (arguments only with
    /// `args_only`); a `Local*Ref` parameter (`by_ref`, its class) lets the handler change it.
    Local { ty: String, by_ref: Option<String>, ordinal: Option<usize>, index: Option<u16>, names: Vec<String>, args_only: bool },
    /// MixinExtras' `@Share`: a `Local*Ref` (`class`) the mixin's handlers in the method share,
    /// new on each run of it.
    Share { name: String, class: String },
}

/// What a [`Sugar`] parameter gets at one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalArg {
    /// The local in this slot.
    Value(u16),
    /// A `class` ref (`LocalIntRef`...) holding the local in `slot`, which goes back to the slot
    /// after the handler.
    Ref { slot: u16, class: String },
    /// The mixin's shared ref `name` for this run of the method.
    Share { name: String, class: String },
}

const SUGAR: &str = "Lcom/llamalad7/mixinextras/sugar/";
const REFS: &str = "com/llamalad7/mixinextras/sugar/ref/";

/// The descriptors of a method descriptor's parameters (`(ILjava/lang/String;[J)V` → `I`,
/// `Ljava/lang/String;`, `[J`).
pub fn param_descs(desc: &str) -> Vec<String> {
    let inner = desc.strip_prefix('(').and_then(|d| d.split_once(')')).map_or("", |(p, _)| p);
    let b = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while b[i] == b'[' {
            i += 1;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        out.push(inner[start..i.min(inner.len())].to_owned());
    }
    out
}

/// The parameters of a generic method signature, each as written (`Ljava/util/List<TT;>;`).
fn signature_params(sig: &str) -> Vec<String> {
    let sig = if sig.starts_with('<') {
        // Type parameters first: `<T:Ljava/lang/Object;>(...)V`.
        let mut depth = 0;
        let mut end = 0;
        for (i, c) in sig.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        &sig[end..]
    } else {
        sig
    };
    let inner = sig.strip_prefix('(').map_or("", |r| r.rsplit_once(')').map_or("", |(p, _)| p));
    let b = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while b[i] == b'[' {
            i += 1;
        }
        match b[i] {
            b'L' | b'T' => {
                let mut depth = 0;
                while i < b.len() {
                    match b[i] {
                        b'<' => depth += 1,
                        b'>' => depth -= 1,
                        b';' if depth == 0 => break,
                        _ => {}
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
        out.push(inner[start..i.min(inner.len())].to_owned());
    }
    out
}

/// The type a `LocalRef<T>` parameter holds, as a descriptor (`T`'s erasure), from its signature.
fn ref_type(sig_param: &str) -> Option<String> {
    let open = sig_param.find('<')?;
    let arg = &sig_param[open + 1..sig_param.rfind('>')?];
    if !arg.starts_with('L') && !arg.starts_with('[') {
        return None;
    }
    // Erase the argument's own type arguments: `Ljava/util/List<...>;` → `Ljava/util/List;`.
    let mut out = String::new();
    let mut depth = 0;
    for c in arg.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    Some(out)
}

/// The primitive a `Local<Kind>Ref` holds (`LocalIntRef` → `I`), or `L` for `LocalRef`.
fn ref_kind(class: &str) -> Option<char> {
    Some(match class.strip_prefix(REFS)? {
        "LocalRef" => 'L',
        "LocalBooleanRef" => 'Z',
        "LocalByteRef" => 'B',
        "LocalCharRef" => 'C',
        "LocalShortRef" => 'S',
        "LocalIntRef" => 'I',
        "LocalLongRef" => 'J',
        "LocalFloatRef" => 'F',
        "LocalDoubleRef" => 'D',
        _ => return None,
    })
}

/// A handler's MixinExtras sugar parameters (they come last), and `captured` locals before them.
fn handler_sugar(h: &Method, captured: usize) -> Result<Vec<Sugar>, String> {
    let descs = param_descs(&h.desc);
    let sig = h.signature.as_deref().map(signature_params).unwrap_or_default();
    let mut sugar: Vec<(usize, Sugar)> = Vec::new();
    for (i, anns) in h.param_annotations.iter().enumerate() {
        for a in anns.iter().filter(|a| a.ty.starts_with(SUGAR)) {
            let desc = descs.get(i).cloned().unwrap_or_default();
            let class = desc.strip_prefix('L').and_then(|d| d.strip_suffix(';')).unwrap_or("");
            let kind = ref_kind(class);
            let held = match kind {
                Some('L') => Some(sig.get(i).and_then(|p| ref_type(p)).ok_or("@Local of a LocalRef with no type")?),
                Some(k) => Some(k.to_string()),
                None => None,
            };
            let simple = a.ty.trim_end_matches(';').rsplit('/').next().unwrap_or("");
            let one = match simple {
                "Local" => Sugar::Local {
                    ty: held.clone().unwrap_or(desc.clone()),
                    by_ref: held.is_some().then(|| class.to_owned()),
                    ordinal: a.value("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize),
                    index: a.value("index").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as u16),
                    names: a.value("name").map(|n| n.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect()).unwrap_or_default(),
                    args_only: a.value("argsOnly") == Some("1"),
                },
                "Share" if kind.is_some() => {
                    let ns = a.value("namespace").unwrap_or("");
                    Sugar::Share { name: format!("{ns}:{}", a.value("value").unwrap_or("")), class: class.to_owned() }
                }
                other => return Err(format!("@{other}")),
            };
            sugar.push((i, one));
        }
    }
    // Sugar only at the end, as MixinExtras requires.
    let first = descs.len() - sugar.len();
    if sugar.iter().enumerate().any(|(n, (i, _))| *i != first + n) {
        return Err("sugar parameters before others".into());
    }
    if captured > first {
        return Err("@Inject capturing more locals than parameters".into());
    }
    let mut out: Vec<Sugar> = descs[first - captured..first].iter().map(|d| Sugar::Captured { ty: d.clone() }).collect();
    out.extend(sugar.into_iter().map(|(_, s)| s));
    Ok(out)
}

/// The slots `sugar` names at offset `pc` of `m` (its locals in scope there, by the method's local
/// variable table, as Mixin finds them), for a target method that `is_static`.
fn resolve_locals(sugar: &[Sugar], m: &Method, pc: usize) -> Result<Vec<LocalArg>, String> {
    if sugar.is_empty() {
        return Ok(Vec::new());
    }
    let code = m.code.as_ref().ok_or("no code")?;
    if code.locals.is_empty() {
        return Err("locals in a method without a local variable table".into());
    }
    let is_static = m.access & 0x0008 != 0;
    let base = u16::from(!is_static);
    // The first slot after the arguments (`long`s and `double`s take two).
    let first_local = base + crate::codegen::arg_kinds(&m.desc).iter().map(|k| if matches!(k, 'J' | 'D') { 2 } else { 1 }).sum::<u16>();
    let mut here: Vec<&crate::class::LocalVar> = code.locals.iter().filter(|v| v.in_scope(pc) && v.slot >= base).collect();
    here.sort_by_key(|v| v.slot);
    let mut captured = here.iter().filter(|v| v.slot >= first_local);
    let mut out = Vec::new();
    for s in sugar {
        out.push(match s {
            Sugar::Captured { ty } => {
                let v = captured.next().ok_or("@Inject capturing a local not there")?;
                if v.desc != *ty {
                    return Err("@Inject capturing locals of other types".into());
                }
                LocalArg::Value(v.slot)
            }
            Sugar::Local { ty, by_ref, ordinal, index, names, args_only } => {
                let candidates: Vec<&&crate::class::LocalVar> = here.iter().filter(|v| !*args_only || v.slot < first_local).collect();
                let slot = if let Some(i) = index {
                    *i
                } else if !names.is_empty() {
                    // Mixin names the arguments `arg0`, `arg1`... (`this` not counted), the
                    // other locals as the table does.
                    let arg_name = |slot: u16| {
                        let mut at = base;
                        for (n, k) in crate::codegen::arg_kinds(&m.desc).iter().enumerate() {
                            if at == slot {
                                return Some(format!("arg{n}"));
                            }
                            at += if matches!(k, 'J' | 'D') { 2 } else { 1 };
                        }
                        None
                    };
                    candidates
                        .iter()
                        .find(|v| names.contains(&v.name) || arg_name(v.slot).is_some_and(|a| names.contains(&a)))
                        .ok_or("@Local by a name not there")?
                        .slot
                } else {
                    let typed: Vec<u16> = candidates.iter().filter(|v| v.desc == *ty).map(|v| v.slot).collect();
                    match (ordinal, typed.as_slice()) {
                        (Some(o), _) => *typed.get(*o).ok_or("@Local ordinal past the locals of its type")?,
                        (None, [only]) => *only,
                        (None, []) => return Err("@Local of a type with no local".into()),
                        (None, _) => return Err("@Local of a type with several locals".into()),
                    }
                };
                if slot >= code.max_locals {
                    return Err("@Local index past the method's locals".into());
                }
                match by_ref {
                    Some(class) => LocalArg::Ref { slot, class: class.clone() },
                    None => LocalArg::Value(slot),
                }
            }
            Sugar::Share { name, class } => LocalArg::Share { name: name.clone(), class: class.clone() },
        });
    }
    Ok(out)
}

/// The stores in `m` into the local `local` picks (as it picks one just after each store), the
/// `ordinal`th only when given: (the store's offset, the next instruction's).
fn store_sites(m: &Method, local: &Sugar, ordinal: Option<usize>, range: Option<(usize, usize)>) -> Vec<(usize, usize)> {
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    let mut out = Vec::new();
    for (k, i) in insns.iter().enumerate() {
        if range.is_some_and(|(lo, hi)| !(lo..=hi).contains(&i.at)) {
            continue;
        }
        let slot = match (i.op, &i.arg) {
            (0x36..=0x3a, crate::insn::Operand::Int(v)) => *v as u16,
            (0x3b..=0x4e, _) => ((i.op - 0x3b) % 4) as u16,
            _ => continue,
        };
        let Some(next) = insns.get(k + 1) else { continue };
        if let Ok(found) = resolve_locals(std::slice::from_ref(local), m, next.at)
            && found == [LocalArg::Value(slot)]
        {
            out.push((i.at, next.at));
        }
    }
    match ordinal {
        Some(o) => out.into_iter().nth(o).into_iter().collect(),
        None => out,
    }
}

/// Whether opcode `op` stores into a local (`istore`... `astore_3`).
pub fn is_store(op: u8) -> bool {
    matches!(op, 0x36..=0x3a | 0x3b..=0x4e)
}

/// One of the nodes of a method's instruction list as Mixin sees it (ASM's `ClassReader`, with
/// debug information and frames): at an instruction's offset, a label when anything points
/// there, its line numbers, its frame, then the instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node {
    Label(usize),
    Line(usize),
    Frame(usize),
    Insn(usize),
}

/// `m`'s nodes in order, the label at the end last when something points there. ASM labels
/// the offsets jumps and switches go to, the exception table's, those where lines and local
/// variables start (and variables end), the frames', and the `new`s its frame scan finds.
fn nodes(m: &Method) -> Vec<Node> {
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    let mut labels: BTreeSet<usize> = BTreeSet::new();
    for i in &insns {
        match &i.arg {
            crate::insn::Operand::Jump(to) => {
                labels.insert(*to);
            }
            crate::insn::Operand::Switch(default, cases) => {
                labels.insert(*default);
                labels.extend(cases.iter().map(|(_, to)| *to));
            }
            _ => {}
        }
    }
    for (start, end, handler, _) in &code.handlers {
        labels.extend([*start, *end, *handler].map(usize::from));
    }
    labels.extend(code.lines.iter().map(|(at, _)| usize::from(*at)));
    for l in &code.locals {
        labels.extend([usize::from(l.start), usize::from(l.start) + usize::from(l.length)]);
    }
    labels.extend(code.frames.iter().chain(&code.frame_news).map(|&at| usize::from(at)));
    let mut out = Vec::new();
    for i in &insns {
        if labels.contains(&i.at) {
            out.push(Node::Label(i.at));
        }
        out.extend(code.lines.iter().filter(|(at, _)| usize::from(*at) == i.at).map(|_| Node::Line(i.at)));
        if code.frames.iter().any(|&f| usize::from(f) == i.at) {
            out.push(Node::Frame(i.at));
        }
        out.push(Node::Insn(i.at));
    }
    if labels.contains(&code.bytes.len()) {
        out.push(Node::Label(code.bytes.len()));
    }
    out
}

/// The places of a `shift = BY` (`at` shifted `by` nodes): the node each of `at`'s places is
/// (for `INVOKE_ASSIGN`, the one after the call and the cast and store of its result that
/// follow it, as Mixin's `AfterInvoke` finds), `by` nodes on. One that lands on an instruction,
/// or on the line number or frame before one, goes before that instruction: (it, it). One that
/// lands on a label goes after the instruction before it (only what falls through from that
/// runs the handler): (that instruction, the label's), and `true`.
fn shifted_places(c: &ClassFile, m: &Method, at: &At, by: i32, range: Option<(usize, usize)>) -> Vec<(usize, usize, bool)> {
    let list = nodes(m);
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    let op_at = |at: usize| insns.iter().find(|i| i.at == at).map(|i| i.op);
    let mut out = Vec::new();
    for (site, _) in places(c, m, at, range) {
        // The head is the list's first node (Mixin's `MethodHead`), a label when one is there.
        let first = if *at == At::Head { Some(0) } else { None };
        let Some(mut k) = first.or_else(|| list.iter().position(|n| *n == Node::Insn(site))) else { continue };
        if matches!(at, At::Invoke { assign: true, .. }) {
            // The next node; past a store of the result, or a cast and the store after it.
            let next = |k: usize| if k + 1 < list.len() { k + 1 } else { k };
            k = next(k);
            match list[k] {
                Node::Insn(a) if op_at(a).is_some_and(is_store) => k = next(k),
                Node::Insn(a) if op_at(a) == Some(0xc0) => {
                    k = next(k);
                    if let Node::Insn(b) = list[k]
                        && op_at(b).is_some_and(is_store)
                    {
                        k = next(k);
                    }
                }
                _ => {}
            }
        }
        let Some(node) = usize::try_from(k as i64 + i64::from(by)).ok().and_then(|k| list.get(k).map(|n| (k, *n))) else { continue };
        match node {
            (_, Node::Insn(a) | Node::Line(a) | Node::Frame(a)) => out.push((a, a, false)),
            (k, Node::Label(a)) => {
                // The instruction before the label (none before the method's first: nothing
                // falls through to it).
                if let Some(&Node::Insn(p)) = list[..k].iter().rev().find(|n| matches!(n, Node::Insn(_))) {
                    out.push((p, a, true));
                }
            }
        }
    }
    out
}

/// The places in `m` (of `c`) an `@At` goes, as the offsets the codegen weaves at (the call, the
/// return, 0 for the head), each with the offset whose locals the handler sees (after the call
/// for `shift = AFTER`); within `range` (offsets, both in) only, for a `@Slice`'s, where an
/// `ordinal` counts.
fn places(c: &ClassFile, m: &Method, at: &At, range: Option<(usize, usize)>) -> Vec<(usize, usize)> {
    if let At::Shifted { at, by } = at {
        return shifted_places(c, m, at, *by, range).into_iter().map(|(site, seen, _)| (site, seen)).collect();
    }
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    let inside = |i: &crate::insn::Insn| range.is_none_or(|(lo, hi)| (lo..=hi).contains(&i.at));
    let next = |k: usize| insns.get(k + 1).map_or(insns[k].at, |x| x.at);
    let nth = |hits: Vec<usize>, ordinal: &Option<usize>| -> Vec<usize> {
        match ordinal {
            Some(o) => hits.get(*o).copied().into_iter().collect(),
            None => hits,
        }
    };
    let at_k = |ks: Vec<usize>| ks.into_iter().map(|k| (insns[k].at, insns[k].at)).collect::<Vec<_>>();
    match at {
        At::Head => vec![(0, 0)],
        // Found by the local it stores into (`store_sites`); a shifted one, above.
        At::Store { .. } | At::Shifted { .. } => Vec::new(),
        At::Constant { value, ordinal } => {
            let hits = (0..insns.len()).filter(|&k| inside(&insns[k]) && constant_of(c, &insns[k]).is_some_and(|x| value.matches(&x))).collect();
            at_k(nth(hits, ordinal))
        }
        At::New { class, desc, ordinal } => {
            let hits = (0..insns.len())
                .filter(|&k| inside(&insns[k]) && insns[k].op == 0xbb && matches!(&insns[k].arg, crate::insn::Operand::Int(v) if c.class_name(*v as u16).is_ok_and(|n| n == *class)))
                .collect();
            // With a constructor named, a `new` some call of it follows (Mixin's `findCtor`).
            let made = |k: &usize| {
                desc.as_ref().is_none_or(|d| {
                    insns[*k..].iter().any(|x| {
                        x.op == 0xb7 && matches!(&x.arg, crate::insn::Operand::Int(v) if c.member(*v as u16).is_some_and(|mm| mm.name == "<init>" && mm.owner == *class && mm.desc == *d))
                    })
                })
            };
            at_k(nth(hits, ordinal).into_iter().filter(made).collect())
        }
        At::Return { ordinal } => at_k(nth((0..insns.len()).filter(|&k| inside(&insns[k]) && matches!(insns[k].op, 0xac..=0xb1)).collect(), ordinal)),
        At::Tail => at_k((0..insns.len()).rev().find(|&k| inside(&insns[k]) && matches!(insns[k].op, 0xac..=0xb1)).into_iter().collect()),
        At::Invoke { ordinal, after, .. } | At::Field { ordinal, after, .. } => {
            let assign = matches!(at, At::Invoke { assign: true, .. });
            let hits = (0..insns.len())
                .filter(|&k| {
                    let i = &insns[k];
                    let member = match &i.arg {
                        crate::insn::Operand::Int(v) if matches!(i.op, 0xb2..=0xb8) => c.member(*v as u16),
                        crate::insn::Operand::IndexCount(v, _) if i.op == 0xb9 => c.member(*v),
                        _ => None,
                    };
                    inside(i) && member.is_some_and(|mm| at.names(i.op, &mm))
                })
                .collect();
            nth(hits, ordinal)
                .into_iter()
                .map(|k| {
                    // The locals the handler sees: before the call, or before what follows it (and
                    // the store of its result, for `INVOKE_ASSIGN`).
                    let seen = if assign && insns.get(k + 1).is_some_and(|x| is_store(x.op)) {
                        next(k + 1)
                    } else if *after {
                        next(k)
                    } else {
                        insns[k].at
                    };
                    (insns[k].at, seen)
                })
                .collect()
        }
    }
}

/// A `@Slice`: the part of a method its `@At`s look in, from the place `from` names to the one
/// `to` names (both in; the first such place, or the last for a `:LAST` selector), either end the
/// method's when not given.
#[derive(Debug, Clone)]
struct Slice {
    id: String,
    from: Option<(At, bool)>,
    to: Option<(At, bool)>,
}

fn parse_slice(a: &Annotation) -> Result<Slice, String> {
    let bound = |k: &str| -> Result<Option<(At, bool)>, String> {
        let Some(at) = a.nested(k).first() else { return Ok(None) };
        let selector = at.value("value").and_then(|v| v.split_once(':')).map_or("FIRST", |(_, s)| s);
        if selector == "ONE" {
            return Err("@Slice bound with selector ONE".into());
        }
        Ok(Some((parse_at(at)?, selector == "LAST")))
    };
    Ok(Slice { id: a.value("id").unwrap_or("").to_owned(), from: bound("from")?, to: bound("to")? })
}

/// The offsets (first, last: both in) of `s` in `m`, as Mixin's `MethodSlice` finds them: a
/// `from` that finds nothing starts at the method's start, a `to` that finds nothing ends where
/// the slice starts.
fn slice_range(c: &ClassFile, m: &Method, s: &Slice) -> Result<(usize, usize), String> {
    let len = m.code.as_ref().map_or(0, |code| code.bytes.len());
    let find = |b: &Option<(At, bool)>| -> Option<Option<usize>> {
        let (at, last) = b.as_ref()?;
        if *at == At::Head {
            return Some(Some(0));
        }
        let found = places(c, m, at, None);
        // A shifted place's node is the instruction after (Mixin's `nextNode`).
        let node = |(site, seen): (usize, usize)| if matches!(at, At::Invoke { after: true, .. } | At::Field { after: true, .. } | At::Shifted { .. }) { seen } else { site };
        Some(if *last { found.last() } else { found.first() }.map(|p| node(*p)))
    };
    let start = find(&s.from).flatten().unwrap_or(0);
    let end = match find(&s.to) {
        None => len,
        Some(found) => found.unwrap_or(start),
    };
    if start > end {
        return Err("@Slice ending before it starts".into());
    }
    Ok((start, end))
}

#[derive(Debug, Default)]
pub struct Plan {
    /// Mixin classes by the translated class they target, in name order.
    pub mixins: BTreeMap<String, Vec<String>>,
    /// The weaves into each target method, by `class.name.desc`, in the mixins' order.
    pub weaves: BTreeMap<String, Vec<Weave>>,
    /// What could not be woven, counted by what it is.
    pub unsupported: BTreeMap<String, usize>,
    /// Injectors that find no place but may (`require = 0`, or their config's `defaultRequire`):
    /// Mixin applies them nowhere either, counted by why.
    pub skipped: BTreeMap<String, usize>,
}

/// An `@At`'s `args` (`["doubleValue=0.25d", "log=true"]`, joined by `,`) as pairs.
fn at_args(v: &str) -> BTreeMap<String, String> {
    let mut out: Vec<(String, String)> = Vec::new();
    for part in v.split(',') {
        match part.split_once('=') {
            Some((k, val)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => out.push((k.to_owned(), val.to_owned())),
            // A `,` in a value (a string's): it goes on.
            _ => {
                if let Some(last) = out.last_mut() {
                    last.1.push(',');
                    last.1.push_str(part);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// The constant `get` names (`intValue`, `doubleValue`... as an `@At("CONSTANT")`'s `args` or a
/// `@Constant`'s values), or, naming none, any of type `by_type` when given (`@ModifyConstant`'s).
fn constant_named(get: &dyn Fn(&str) -> Option<String>, by_type: Option<&str>) -> Result<Constant, String> {
    // Java's own float syntax takes a type suffix: `64f`, `0.25d`.
    let number = |v: &str| v.trim().trim_end_matches(['f', 'F', 'd', 'D']).to_owned();
    let mut found = Vec::new();
    if get("nullValue").is_some_and(|v| v == "true" || v == "1") {
        found.push(Constant::Null);
    }
    if let Some(v) = get("intValue") {
        found.push(Constant::Int(v.trim().parse().map_err(|_| format!("intValue {v}"))?));
    }
    if let Some(v) = get("floatValue") {
        found.push(Constant::Float(number(&v).parse::<f32>().map_err(|_| format!("floatValue {v}"))?.to_bits()));
    }
    if let Some(v) = get("longValue") {
        found.push(Constant::Long(v.trim().trim_end_matches(['l', 'L']).parse().map_err(|_| format!("longValue {v}"))?));
    }
    if let Some(v) = get("doubleValue") {
        found.push(Constant::Double(number(&v).parse::<f64>().map_err(|_| format!("doubleValue {v}"))?.to_bits()));
    }
    if let Some(v) = get("stringValue") {
        found.push(Constant::String(v));
    }
    if let Some(v) = get("classValue") {
        // `com.a.B` in an `@At`'s args, `Lcom/a/B;` as a `@Constant`'s class.
        let v = v.strip_prefix('L').and_then(|r| r.strip_suffix(';')).map(str::to_owned).unwrap_or(v);
        found.push(Constant::Class(v.replace('.', "/")));
    }
    if get("expandZeroConditions").is_some_and(|v| !v.is_empty()) {
        return Err("@At CONSTANT expandZeroConditions".into());
    }
    match (found.len(), by_type) {
        (1, _) => Ok(found.remove(0)),
        (0, Some(t)) => Ok(Constant::OfType(t.to_owned())),
        (0, None) => Err("@At CONSTANT without a value".into()),
        _ => Err("@At CONSTANT with several values".into()),
    }
}

/// `Lnet/minecraft/world/level/Level;setBlock(Lnet/...;I)Z` (or `net/.../Level.setBlock(...)Z`,
/// or `setBlock(...)Z` for any owner) as (owner, name, desc).
pub fn member_of(target: &str) -> Option<(String, String, String)> {
    let paren = target.find('(')?;
    let (head, desc) = target.split_at(paren);
    let (owner, name) = if let Some((o, n)) = head.strip_prefix('L').and_then(|r| r.split_once(';')) {
        (o.to_owned(), n.to_owned())
    } else if let Some((o, n)) = head.rsplit_once('.') {
        (o.replace('.', "/"), n.to_owned())
    } else {
        (String::new(), head.to_owned())
    };
    Some((owner, name, desc.to_owned()))
}

fn parse_at(a: &Annotation) -> Result<At, String> {
    let shift = a.value("shift").unwrap_or("");
    if shift != "BY" {
        return parse_unshifted(a, shift);
    }
    // `by` nodes on from the place itself; Mixin refuses more than 5
    // (`InjectionPoint.MAX_ALLOWED_SHIFT_BY`).
    let by = match a.value("by") {
        None => 0,
        Some(b) => b.trim().parse::<i32>().map_err(|_| format!("@At by {b}"))?,
    };
    if by > 5 {
        return Err("@At shift BY over 5".into());
    }
    Ok(At::Shifted { at: Box::new(parse_unshifted(a, "")?), by })
}

/// An `@At`, shifted `BEFORE` or `AFTER` as `shift` says.
fn parse_unshifted(a: &Annotation, shift: &str) -> Result<At, String> {
    let raw = a.value("value").unwrap_or("");
    // A selector after it (`INVOKE:LAST`) matters to a slice's bounds only.
    let value = raw.split_once(':').map_or(raw, |(v, _)| v);
    let ordinal = a.value("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize);
    let args = at_args(a.value("args").unwrap_or(""));
    let plain_shift = || if matches!(shift, "" | "BEFORE" | "AFTER") { Ok(()) } else { Err(format!("@At shift {shift}")) };
    Ok(match value {
        "HEAD" => At::Head,
        "STORE" => At::Store { ordinal },
        "RETURN" => At::Return { ordinal },
        "TAIL" => At::Tail,
        "INVOKE" | "INVOKE_ASSIGN" => {
            plain_shift()?;
            let target = a.value("target").ok_or("INVOKE without a target")?;
            let (owner, name, desc) = member_of(target).ok_or_else(|| format!("target {target}"))?;
            At::Invoke { owner, name, desc, ordinal, after: shift == "AFTER" || value == "INVOKE_ASSIGN", assign: value == "INVOKE_ASSIGN" }
        }
        "FIELD" => {
            plain_shift()?;
            let target = a.value("target").ok_or("FIELD without a target")?;
            // `Lowner;name:Ldesc;` (or `owner.name:desc`, or a bare `name:desc`).
            let (head, desc) = target.split_once(':').unwrap_or((target, ""));
            let (owner, name) = if let Some((o, n)) = head.strip_prefix('L').and_then(|r| r.split_once(';')) {
                (o.to_owned(), n.to_owned())
            } else if let Some((o, n)) = head.rsplit_once('.') {
                (o.replace('.', "/"), n.to_owned())
            } else {
                (String::new(), head.to_owned())
            };
            let opcode = a.value("opcode").and_then(|o| o.parse::<i64>().ok()).filter(|o| (0xb2..=0xb5).contains(o)).map(|o| o as u8);
            At::Field { owner, name, desc: desc.to_owned(), opcode, ordinal, after: shift == "AFTER" }
        }
        "CONSTANT" => {
            if !shift.is_empty() && shift != "BEFORE" {
                return Err(format!("@At CONSTANT shift {shift}"));
            }
            At::Constant { value: constant_named(&|k| args.get(k).cloned(), None)?, ordinal }
        }
        "NEW" => {
            if !shift.is_empty() && shift != "BEFORE" {
                return Err(format!("@At NEW shift {shift}"));
            }
            // `com/a/B`, `Lcom/a/B;`, or a constructor's descriptor returning it: `(D)Lcom/a/B;`.
            let target = a.value("target").filter(|t| !t.is_empty()).map(str::to_owned).or_else(|| args.get("class").cloned()).ok_or("NEW without a class")?;
            let (class, desc) = match target.strip_prefix('(').and_then(|r| r.split_once(')')) {
                Some((params, ret)) => (ret.to_owned(), Some(format!("({params})V"))),
                None => (target.clone(), None),
            };
            let class = class.strip_prefix('L').and_then(|r| r.strip_suffix(';')).map(str::to_owned).unwrap_or(class).replace('.', "/");
            At::New { class, desc, ordinal }
        }
        other => return Err(format!("@At {other}")),
    })
}

/// Whether selector `sel` (`tick`, `tick()V`, `Lpkg/Owner;tick()V`, `tick*`, `*`) names method
/// `name desc`; `all` is false for a bare name, which Mixin takes as the first overload only.
fn selects(sel: &str, name: &str, desc: &str) -> (bool, bool) {
    let sel = sel.trim();
    // An owner in front: `Lpkg/Owner;name(...)`.
    let sel = match sel.strip_prefix('L').and_then(|r| r.split_once(';')) {
        Some((_, rest)) if !rest.is_empty() => rest,
        _ => sel,
    };
    if sel == "*" {
        return (!name.starts_with('<'), true);
    }
    if let Some(paren) = sel.find('(') {
        let (n, d) = sel.split_at(paren);
        return (n == name && d == desc, true);
    }
    if let Some(prefix) = sel.strip_suffix('*') {
        return (name == prefix, true);
    }
    (sel == name, false)
}

/// The methods of `target` a selector list (`method = {...}`) picks.
fn targets<'a>(target: &'a ClassFile, selectors: &str) -> Vec<&'a Method> {
    let mut out: Vec<&Method> = Vec::new();
    for sel in selectors.split(',').filter(|s| !s.is_empty()) {
        for m in &target.methods {
            let (hit, all) = selects(sel, &m.name, &m.desc);
            if hit && m.code.is_some() {
                if !out.iter().any(|o| std::ptr::eq(*o, m)) {
                    out.push(m);
                }
                if !all {
                    break;
                }
            }
        }
    }
    out
}

/// The weaving plan for `classes`: their mixins whose targets are among them, every mixin
/// applied.
pub fn plan(classes: &[ClassFile]) -> Plan {
    let all: Vec<&ClassFile> = classes.iter().collect();
    plan_with(&all, &[])
}

/// [`plan`] for the mixins `configs` list, as Mixin applies only those (a mod's jar may hold
/// others its configurations leave out), in their order (Mixin's, for mixins of one priority);
/// no configuration at all applies every mixin, in `classes`' order.
pub fn plan_with(classes: &[&ClassFile], configs: &[crate::MixinConfig]) -> Plan {
    let by_name: BTreeMap<&str, &ClassFile> = classes.iter().map(|c| (c.name.as_str(), *c)).collect();
    let mut order: Vec<(&ClassFile, i64)> = if configs.is_empty() {
        classes.iter().map(|c| (*c, 0)).collect()
    } else {
        // Less those its plugin turns off.
        configs
            .iter()
            .flat_map(|c| c.classes.iter().filter(|m| !c.off.contains(m)).filter_map(|m| by_name.get(m.as_str()).map(|k| (*k, c.default_require))))
            .collect()
    };
    // Lower priorities first (`@Mixin(priority = ...)`, 1000 if not given).
    order.sort_by_key(|(m, _)| m.annotation_of("Lorg/spongepowered/asm/mixin/Mixin;").and_then(|a| a.value("priority")).and_then(|p| p.parse::<i64>().ok()).unwrap_or(1000));
    let mut plan = Plan::default();
    let count = |plan: &mut Plan, what: String| *plan.unsupported.entry(what).or_default() += 1;
    for (mixin, default_require) in order {
        for target_name in mixin.mixin_targets() {
            let Some(target) = by_name.get(target_name.as_str()) else { continue };
            plan.mixins.entry(target_name.clone()).or_default().push(mixin.name.clone());
            for h in &mixin.methods {
                for a in &h.annotations {
                    let simple = a.ty.trim_end_matches(';').rsplit('/').next().unwrap_or("");
                    let injector = (a.ty.starts_with(MIXIN) && a.ty.contains("/injection/")) || (a.ty.starts_with(EXTRAS) && a.ty.contains("/injector/"));
                    if !injector || matches!(simple, "At" | "Slice" | "Coerce" | "Desc" | "Group") {
                        continue;
                    }
                    let kind = match simple {
                        "Inject" => Kind::Inject { cancellable: a.value("cancellable") == Some("1") },
                        "Redirect" => Kind::Redirect,
                        "ModifyArg" => Kind::ModifyArg { index: a.value("index").and_then(|i| i.parse().ok()).unwrap_or(usize::MAX) },
                        "ModifyExpressionValue" => Kind::ModifyExpressionValue,
                        "WrapWithCondition" => Kind::WrapWithCondition,
                        "ModifyReturnValue" => Kind::ModifyReturnValue,
                        "WrapOperation" => Kind::WrapOperation,
                        "WrapMethod" => Kind::WrapMethod,
                        "ModifyReceiver" => Kind::ModifyReceiver,
                        "ModifyConstant" => Kind::ModifyConstant,
                        // The local of the handler's type, picked as `@Local` picks one.
                        "ModifyVariable" => Kind::ModifyVariable {
                            local: Sugar::Local {
                                ty: h.desc.rsplit(')').next().unwrap_or("").to_owned(),
                                by_ref: None,
                                ordinal: a.value("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize),
                                index: a.value("index").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as u16),
                                names: a.value("name").map(|n| n.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect()).unwrap_or_default(),
                                args_only: a.value("argsOnly") == Some("1"),
                            },
                        },
                        other => {
                            count(&mut plan, format!("mixin @{other}"));
                            continue;
                        }
                    };
                    // Slices narrow where an `@At` looks: the one its `slice` id names.
                    let slices: Vec<Slice> = match a.nested("slice").iter().map(parse_slice).collect::<Result<_, _>>() {
                        Ok(v) => v,
                        Err(e) => {
                            debug(|| format!("{e}: {}.{}{}", mixin.name, h.name, h.desc));
                            count(&mut plan, format!("mixin {e}"));
                            continue;
                        }
                    };
                    // `at = {@At(...), @At(...)}`: each place, with the id of its slice. A
                    // `@ModifyConstant`'s are its `@Constant`s (any constant of its type without
                    // one); a whole method's wrapper has none.
                    let ats: Vec<(Result<At, String>, String)> = match &kind {
                        Kind::WrapMethod => vec![(Ok(At::Head), String::new())],
                        Kind::ModifyConstant => {
                            let ty = h.desc.rsplit(')').next().unwrap_or("");
                            let constants = a.nested("constant");
                            if constants.is_empty() {
                                vec![(Ok(At::Constant { value: Constant::OfType(ty.to_owned()), ordinal: None }), String::new())]
                            } else {
                                constants
                                    .iter()
                                    .map(|k| {
                                        let ordinal = k.value("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize);
                                        let value = constant_named(&|v| k.value(v).map(str::to_owned), Some(ty));
                                        (value.map(|value| At::Constant { value, ordinal }), k.value("slice").unwrap_or("").to_owned())
                                    })
                                    .collect()
                            }
                        }
                        // MixinExtras' take `@Constant`s too (`@WrapOperation` of an `instanceof`).
                        _ => a
                            .nested("at")
                            .iter()
                            .map(|x| (parse_at(x), x.value("slice").unwrap_or("").to_owned()))
                            .chain(a.nested("constant").iter().map(|k| {
                                let ordinal = k.value("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize);
                                let value = constant_named(&|v| k.value(v).map(str::to_owned), None);
                                (value.map(|value| At::Constant { value, ordinal }), k.value("slice").unwrap_or("").to_owned())
                            }))
                            .collect(),
                    };
                    if ats.is_empty() {
                        debug(|| format!("no @At: {}.{}{} {:?}", mixin.name, h.name, h.desc, a.values));
                        count(&mut plan, format!("mixin @{simple} without an @At"));
                    }
                    for (at, slice_id) in ats {
                        let at = match at {
                            Ok(at) => at,
                            Err(e) => {
                                debug(|| format!("{e}: {}.{}{}", mixin.name, h.name, h.desc));
                                count(&mut plan, format!("mixin {e}"));
                                continue;
                            }
                        };
                        let slice = slices.iter().find(|s| s.id == slice_id);
                        // How many places it needs: its own `require`, else its config's.
                        let require = a.value("require").and_then(|r| r.parse::<i64>().ok()).filter(|r| *r >= 0).unwrap_or(default_require);
                        let missing = |plan: &mut Plan, what: &str| {
                            if require > 0 {
                                count(plan, format!("mixin {what}"));
                            } else {
                                *plan.skipped.entry(what.to_owned()).or_default() += 1;
                            }
                        };
                        let picked = targets(target, a.value("method").unwrap_or(""));
                        if picked.is_empty() {
                            debug(|| format!("not found (require {require}): {}.{} {:?} in {}", mixin.name, h.name, a.value("method"), target.name));
                            missing(&mut plan, "target method not found");
                        }
                        // A call the `@At` names, in each method it picks: Mixin injects nothing
                        // into a method without one (a wildcard's), and needs one somewhere.
                        let mut placed = false;
                        let picked_any = !picked.is_empty();
                        for m in picked {
                            let range = match slice.map(|s| slice_range(target, m, s)).transpose() {
                                Ok(r) => r,
                                Err(e) => {
                                    debug(|| format!("{e}: {}.{}{} -> {}.{}{}", mixin.name, h.name, h.desc, target.name, m.name, m.desc));
                                    count(&mut plan, format!("mixin {e}"));
                                    continue;
                                }
                            };
                            // The head of a slice that starts later is its first instruction.
                            if at == At::Head && range.is_some_and(|(lo, _)| lo > 0) {
                                count(&mut plan, "mixin @At HEAD of a slice".into());
                                continue;
                            }
                            // A `@ModifyVariable` at `STORE`: the stores into the local it changes.
                            let sites = match (&kind, &at) {
                                (Kind::ModifyVariable { local }, At::Store { ordinal }) => store_sites(m, local, *ordinal, range),
                                _ => places(target, m, &at, range),
                            };
                            if sites.is_empty() {
                                continue;
                            }
                            // A `shift = BY` that lands on a label goes after the instruction before it.
                            let after: BTreeSet<usize> = match &at {
                                At::Shifted { at: base, by } => shifted_places(target, m, base, *by, range).into_iter().filter(|p| p.2).map(|p| p.0).collect(),
                                _ => BTreeSet::new(),
                            };
                            placed = true;
                            let mut w = Weave {
                                mixin: mixin.name.clone(),
                                handler: h.name.clone(),
                                handler_desc: h.desc.clone(),
                                handler_static: h.access & 0x0008 != 0,
                                kind: kind.clone(),
                                at: at.clone(),
                                sugar: Vec::new(),
                                locals: BTreeMap::new(),
                                var: BTreeMap::new(),
                                after,
                            };
                            // An `@Inject` capturing locals takes them after the callback.
                            let n = crate::codegen::arg_kinds(&h.desc).len();
                            let sugar_n = h.param_annotations.iter().filter(|p| p.iter().any(|a| a.ty.starts_with(SUGAR))).count();
                            let target_args = crate::codegen::arg_kinds(&m.desc).len();
                            let captured = match &kind {
                                Kind::Inject { .. } if a.value("locals").is_some_and(|l| l != "NO_CAPTURE") => (n - sugar_n).saturating_sub(target_args + 1),
                                _ => 0,
                            };
                            let resolved = handler_sugar(h, captured).and_then(|sugar| {
                                w.sugar = sugar;
                                for &(site, seen) in &sites {
                                    let args = resolve_locals(&w.sugar, m, seen)?;
                                    w.locals.insert(site, args);
                                    if let Kind::ModifyVariable { local } = &w.kind
                                        && let [LocalArg::Value(slot)] = resolve_locals(std::slice::from_ref(local), m, seen)?.as_slice()
                                    {
                                        w.var.insert(site, *slot);
                                    }
                                }
                                Ok(())
                            });
                            if let Err(e) = resolved {
                                debug(|| format!("{e}: {}.{}{} -> {}.{}{}", mixin.name, h.name, h.desc, target.name, m.name, m.desc));
                                count(&mut plan, format!("mixin {e}"));
                                continue;
                            }
                            match check(&w, m) {
                                Ok(()) => plan.weaves.entry(format!("{}.{}.{}", target.name, m.name, m.desc)).or_default().push(w),
                                Err(e) => {
                                    debug(|| format!("{e}: {}.{}{} -> {}.{}{}", mixin.name, h.name, h.desc, target.name, m.name, m.desc));
                                    count(&mut plan, format!("mixin {e}"))
                                }
                            }
                        }
                        if picked_any && !placed {
                            debug(|| format!("no call (require {require}): {}.{} {at:?} in {}", mixin.name, h.name, target.name));
                            missing(&mut plan, "@At names nothing in its methods");
                        }
                    }
                }
            }
        }
    }
    plan
}

/// `HORIZON_WEAVE_DEBUG=1`: what is not woven, handler by handler, on stderr.
fn debug(line: impl FnOnce() -> String) {
    if std::env::var_os("HORIZON_WEAVE_DEBUG").is_some() {
        eprintln!("{}", line());
    }
}

/// Whether the codegen can weave `w` into `m`: the place fits the kind, and the handler takes
/// what it will be given.
fn check(w: &Weave, m: &Method) -> Result<(), String> {
    if m.access & 0x0008 != 0 && !w.handler_static {
        return Err("instance handler for a static method".into());
    }
    let target_args = crate::codegen::arg_kinds(&m.desc).len();
    let n = w.handler_params();
    // After its own `base` parameters a handler may take the method's first arguments, as many as
    // it has room for (Mixin's `captureTargetArgs`).
    let takes = |base: usize| n >= base && n - base <= target_args;
    let call_args = |at: &At| match at {
        At::Invoke { desc, .. } => Some(crate::codegen::arg_kinds(desc).len()),
        _ => None,
    };
    // `n` leaves the locals out: one capturing them takes the method's arguments before them.
    if w.sugar.iter().any(|s| matches!(s, Sugar::Captured { .. })) && n != target_args + 1 {
        return Err("@Inject capturing locals without the method's arguments".into());
    }
    match (&w.kind, &w.at) {
        (Kind::Inject { .. }, _) => {
            // The callback alone, or the target's arguments and the callback.
            if n == 1 || n == target_args + 1 {
                Ok(())
            } else {
                Err("@Inject with other parameters".into())
            }
        }
        // A whole method's wrapper shares nothing with what goes inside it yet.
        (Kind::WrapMethod, _) if !w.sugar.is_empty() => Err("@WrapMethod with sugar parameters".into()),
        (Kind::ModifyReturnValue, At::Return { .. } | At::Tail) => {
            if takes(1) { Ok(()) } else { Err("@ModifyReturnValue with extra parameters".into()) }
        }
        // The method's arguments and the `Operation`.
        (Kind::WrapMethod, _) => {
            if n == target_args + 1 {
                Ok(())
            } else {
                Err("@WrapMethod with other parameters".into())
            }
        }
        (Kind::Redirect | Kind::WrapWithCondition | Kind::ModifyArg { .. } | Kind::ModifyExpressionValue | Kind::WrapOperation, At::Invoke { .. }) => {
            let _ = call_args(&w.at);
            Ok(())
        }
        (Kind::Redirect | Kind::WrapWithCondition | Kind::ModifyExpressionValue | Kind::WrapOperation, At::Field { .. }) => Ok(()),
        // The value, then the method's arguments when it takes them.
        (Kind::ModifyVariable { .. }, _) => {
            if takes(1) {
                Ok(())
            } else {
                Err("@ModifyVariable with other parameters".into())
            }
        }
        (Kind::ModifyReceiver, At::Invoke { .. } | At::Field { .. }) => Ok(()),
        // The constant, and the method's arguments when it takes them.
        (Kind::ModifyConstant, At::Constant { .. }) => {
            if takes(1) {
                Ok(())
            } else {
                Err("@ModifyConstant with other parameters".into())
            }
        }
        (Kind::ModifyExpressionValue, At::Constant { .. }) => {
            if takes(1) {
                Ok(())
            } else {
                Err("@ModifyExpressionValue of a constant with other parameters".into())
            }
        }
        // An `instanceof`: the object and the check as an `Operation`.
        (Kind::WrapOperation, At::Constant { value: Constant::Class(_), .. }) => {
            if takes(2) {
                Ok(())
            } else {
                Err("@WrapOperation of an instanceof with other parameters".into())
            }
        }
        (kind, At::Shifted { at, .. }) => Err(format!("{} at {} shift BY", kind.name(), at.name())),
        (kind, at) => Err(format!("{} at {}", kind.name(), at.name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_pick_methods_as_mixin_does() {
        assert_eq!(selects("tick", "tick", "()V"), (true, false));
        assert_eq!(selects("tick()V", "tick", "()V"), (true, true));
        assert_eq!(selects("tick()V", "tick", "(I)V"), (false, true));
        assert_eq!(selects("Lcom/a/B;tick(I)V", "tick", "(I)V"), (true, true));
        assert_eq!(selects("tick*", "tick", "(I)V"), (true, true));
        assert_eq!(selects("<init>", "<init>", "()V"), (true, false));
        assert_eq!(selects("*", "<init>", "()V"), (false, true));
        assert_eq!(
            member_of("Lnet/minecraft/world/level/Level;setBlock(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z"),
            Some((
                "net/minecraft/world/level/Level".into(),
                "setBlock".into(),
                "(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z".into()
            ))
        );
        let at = Annotation {
            values: vec![("value".into(), "INVOKE".into()), ("target".into(), "Lcom/a/B;go(I)V".into()), ("ordinal".into(), "1".into()), ("shift".into(), "AFTER".into())],
            ..Annotation::default()
        };
        let at = parse_at(&at).unwrap();
        assert_eq!(at, At::Invoke { owner: "com/a/B".into(), name: "go".into(), desc: "(I)V".into(), ordinal: Some(1), after: true, assign: false });
    }
}
