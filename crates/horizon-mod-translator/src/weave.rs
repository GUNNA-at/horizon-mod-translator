//! Mixins into translated classes, woven by the translator itself. A mixin whose target is one of
//! the classes being translated (an addon's mixin into Create's `KineticBlockEntity`, say) needs
//! no game hook: the translator has both the mixin's code and its target's, so it does what Mixin
//! does at class load, as it writes the target's Rust. Its methods join the target (methods it
//! adds, `@Overwrite`s: `Program::resolve`), its field initialisers run in the target's
//! constructors, and each injector becomes a call to its handler at the place its `@At` names:
//!
//! - `@Inject` at `HEAD`, `RETURN`, `TAIL` or a call (`INVOKE`, before or `shift = AFTER` it), with a
//!   `CallbackInfo` (`CallbackInfoReturnable` for a value) that may end the method early;
//! - `@Redirect` of a call (the handler instead of it), `@ModifyArg` (one argument through the
//!   handler), MixinExtras' `@ModifyExpressionValue` (the call's result through it),
//!   `@WrapWithCondition` (the call only if the handler says so) and `@ModifyReturnValue`.
//!
//! MixinExtras' `@WrapOperation` (a call through the handler, which gets the call as an
//! `Operation` it may make) and `@WrapMethod` (the whole method so) too. What it doesn't weave
//! (other `@At`s, captured locals...) is counted in the plan's `unsupported`, as the codegen counts
//! what it can't translate.
use crate::class::{ClassFile, Member, Method};
use std::collections::BTreeMap;

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
}

impl At {
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

    /// Whether it is at an instruction (a call or a field access) rather than the method's ends.
    pub fn at_instruction(&self) -> bool {
        matches!(self, At::Invoke { .. } | At::Field { .. })
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
fn store_sites(m: &Method, local: &Sugar, ordinal: Option<usize>) -> Vec<(usize, usize)> {
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    let mut out = Vec::new();
    for (k, i) in insns.iter().enumerate() {
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

/// The places in `m` (of `c`) an `@At` goes, as the offsets the codegen weaves at (the call, the
/// return, 0 for the head), each with the offset whose locals the handler sees (after the call
/// for `shift = AFTER`).
fn places(c: &ClassFile, m: &Method, at: &At) -> Vec<(usize, usize)> {
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    match at {
        At::Head => vec![(0, 0)],
        // Found by the local it stores into (`store_sites`).
        At::Store { .. } => Vec::new(),
        At::Return { ordinal } => insns
            .iter()
            .filter(|i| matches!(i.op, 0xac..=0xb1))
            .enumerate()
            .filter(|(n, _)| ordinal.is_none_or(|o| o == *n))
            .map(|(_, i)| (i.at, i.at))
            .collect(),
        At::Tail => insns.iter().rev().find(|i| matches!(i.op, 0xac..=0xb1)).map(|i| vec![(i.at, i.at)]).unwrap_or_default(),
        At::Invoke { ordinal, after, .. } | At::Field { ordinal, after, .. } => {
            let assign = matches!(at, At::Invoke { assign: true, .. });
            let mut n = 0;
            let mut out = Vec::new();
            for (k, i) in insns.iter().enumerate() {
                if !matches!(i.op, 0xb2..=0xb5 | 0xb6..=0xb9) {
                    continue;
                }
                let member = match &i.arg {
                    crate::insn::Operand::Int(v) => c.member(*v as u16),
                    crate::insn::Operand::IndexCount(v, _) => c.member(*v),
                    _ => None,
                };
                if member.is_some_and(|mm| at.names(i.op, &mm)) {
                    if ordinal.is_none_or(|o| o == n) {
                        // The locals the handler sees: before the call, or before what follows it
                        // (and the store of its result, for `INVOKE_ASSIGN`).
                        let next = |k: usize| insns.get(k + 1).map_or(i.at, |x| x.at);
                        let seen = if assign && insns.get(k + 1).is_some_and(|x| is_store(x.op)) {
                            next(k + 1)
                        } else if *after {
                            next(k)
                        } else {
                            i.at
                        };
                        out.push((i.at, seen));
                    }
                    n += 1;
                }
            }
            out
        }
    }
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

/// An `@At` annotation's values, from its rendering (`L...At;(value=INVOKE|target=...|ordinal=1)`).
fn at_values(v: &str) -> BTreeMap<String, String> {
    let inner = v.split_once('(').map_or("", |(_, r)| r.strip_suffix(')').unwrap_or(r));
    inner.split('|').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
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

fn parse_at(v: &str) -> Result<At, String> {
    let at = at_values(v);
    let value = at.get("value").map(String::as_str).unwrap_or("");
    let shift = at.get("shift").map(String::as_str).unwrap_or("");
    Ok(match value {
        "HEAD" => At::Head,
        "STORE" => At::Store { ordinal: at.get("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize) },
        "RETURN" => At::Return { ordinal: at.get("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize) },
        "TAIL" => At::Tail,
        "INVOKE" | "INVOKE_ASSIGN" => {
            let target = at.get("target").ok_or("INVOKE without a target")?;
            let (owner, name, desc) = member_of(target).ok_or_else(|| format!("target {target}"))?;
            let ordinal = at.get("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize);
            if !matches!(shift, "" | "BEFORE" | "AFTER") {
                return Err(format!("@At shift {shift}"));
            }
            At::Invoke { owner, name, desc, ordinal, after: shift == "AFTER" || value == "INVOKE_ASSIGN", assign: value == "INVOKE_ASSIGN" }
        }
        "FIELD" => {
            let target = at.get("target").ok_or("FIELD without a target")?;
            // `Lowner;name:Ldesc;` (or `owner.name:desc`, or a bare `name:desc`).
            let (head, desc) = target.split_once(':').unwrap_or((target.as_str(), ""));
            let (owner, name) = if let Some((o, n)) = head.strip_prefix('L').and_then(|r| r.split_once(';')) {
                (o.to_owned(), n.to_owned())
            } else if let Some((o, n)) = head.rsplit_once('.') {
                (o.replace('.', "/"), n.to_owned())
            } else {
                (String::new(), head.to_owned())
            };
            let ordinal = at.get("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize);
            let opcode = at.get("opcode").and_then(|o| o.parse::<i64>().ok()).filter(|o| (0xb2..=0xb5).contains(o)).map(|o| o as u8);
            if !matches!(shift, "" | "BEFORE" | "AFTER") {
                return Err(format!("@At shift {shift}"));
            }
            At::Field { owner, name, desc: desc.to_owned(), opcode, ordinal, after: shift == "AFTER" }
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
        configs.iter().flat_map(|c| c.classes.iter().filter_map(|m| by_name.get(m.as_str()).map(|k| (*k, c.default_require)))).collect()
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
                    // A slice narrows where the `@At` looks; without it the handler would go to
                    // places Mixin leaves alone.
                    if a.value("slice").is_some_and(|v| !v.is_empty()) {
                        count(&mut plan, "mixin @Slice".into());
                        continue;
                    }
                    let at_value = a.value("at").unwrap_or("");
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
                    // `at = {@At(...), @At(...)}`: each place. A whole method's wrapper has none.
                    let ats: Vec<&str> = if kind == Kind::WrapMethod {
                        vec!["Lorg/spongepowered/asm/mixin/injection/At;(value=HEAD)"]
                    } else {
                        at_value.split(",Lorg/spongepowered/asm/mixin/injection/At;").collect()
                    };
                    for (i, at_text) in ats.iter().enumerate() {
                        let at_text = if i == 0 { at_text.to_string() } else { format!("Lorg/spongepowered/asm/mixin/injection/At;{at_text}") };
                        let at = match parse_at(&at_text) {
                            Ok(at) => at,
                            Err(e) => {
                                count(&mut plan, format!("mixin {e}"));
                                continue;
                            }
                        };
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
                            // A `@ModifyVariable` at `STORE`: the stores into the local it changes.
                            let sites = match (&kind, &at) {
                                (Kind::ModifyVariable { local }, At::Store { ordinal }) => store_sites(m, local, *ordinal),
                                _ => places(target, m, &at),
                            };
                            if sites.is_empty() {
                                continue;
                            }
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
            if n == 1 || n == target_args + 1 { Ok(()) } else { Err("@ModifyReturnValue with extra parameters".into()) }
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
        (Kind::ModifyVariable { .. }, At::Head | At::Return { .. } | At::Tail | At::Invoke { .. } | At::Field { .. } | At::Store { .. }) => {
            if n == 1 || n == 1 + target_args {
                Ok(())
            } else {
                Err("@ModifyVariable with other parameters".into())
            }
        }
        (Kind::ModifyReceiver, At::Invoke { .. } | At::Field { .. }) => Ok(()),
        (kind, at) => Err(format!("{kind:?} at {at:?}")),
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
        let at = parse_at("Lorg/spongepowered/asm/mixin/injection/At;(value=INVOKE|target=Lcom/a/B;go(I)V|ordinal=1|shift=AFTER)").unwrap();
        assert_eq!(at, At::Invoke { owner: "com/a/B".into(), name: "go".into(), desc: "(I)V".into(), ordinal: Some(1), after: true, assign: false });
    }
}
