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
}

/// MixinExtras' `Operation`: what a wrapping handler calls to run what it wraps.
pub const OPERATION: &str = "com/llamalad7/mixinextras/injector/wrapoperation/Operation";

/// Where in the target method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    Head,
    /// Before each return.
    Return,
    /// Before the last return.
    Tail,
    /// A call to `owner.name desc` (the `n`th such call when `ordinal` says), before it or after;
    /// an empty owner is any owner (a target written without one).
    Invoke { owner: String, name: String, desc: String, ordinal: Option<usize>, after: bool },
}

impl At {
    /// Whether a call to `m` is one this `@At` names. Mixin compares the call instruction's
    /// owner as written, not the class that declares the method.
    pub fn names_call(&self, m: &Member) -> bool {
        match self {
            At::Invoke { owner, name, desc, .. } => (owner.is_empty() || *owner == m.owner) && *name == m.name && *desc == m.desc,
            _ => false,
        }
    }
}

/// The methods `m` (of `c`) calls, in the order of its code.
fn calls(c: &ClassFile, m: &Method) -> Vec<Member> {
    let Some(code) = &m.code else { return Vec::new() };
    let Ok(insns) = crate::insn::decode(&code.bytes) else { return Vec::new() };
    insns
        .iter()
        .filter(|i| matches!(i.op, 0xb6..=0xb9))
        .filter_map(|i| match &i.arg {
            crate::insn::Operand::Int(v) => c.member(*v as u16),
            crate::insn::Operand::IndexCount(v, _) => c.member(*v),
            _ => None,
        })
        .collect()
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
}

impl Weave {
    /// The handler's parameter count.
    pub fn handler_params(&self) -> usize {
        crate::codegen::arg_kinds(&self.handler_desc).len()
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
        "RETURN" => At::Return,
        "TAIL" => At::Tail,
        "INVOKE" | "INVOKE_ASSIGN" => {
            let target = at.get("target").ok_or("INVOKE without a target")?;
            let (owner, name, desc) = member_of(target).ok_or_else(|| format!("target {target}"))?;
            let ordinal = at.get("ordinal").and_then(|o| o.parse::<i64>().ok()).filter(|o| *o >= 0).map(|o| o as usize);
            if !matches!(shift, "" | "BEFORE" | "AFTER") {
                return Err(format!("@At shift {shift}"));
            }
            At::Invoke { owner, name, desc, ordinal, after: shift == "AFTER" || value == "INVOKE_ASSIGN" }
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

/// The weaving plan for `classes`: their mixins whose targets are among them.
pub fn plan(classes: &[ClassFile]) -> Plan {
    let by_name: BTreeMap<&str, &ClassFile> = classes.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut plan = Plan::default();
    let count = |plan: &mut Plan, what: String| *plan.unsupported.entry(what).or_default() += 1;
    for mixin in classes {
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
                        let picked = targets(target, a.value("method").unwrap_or(""));
                        if picked.is_empty() {
                            debug(|| format!("not found: {}.{} {:?} in {}", mixin.name, h.name, a.value("method"), target.name));
                            count(&mut plan, "mixin target method not found".into());
                        }
                        // A call the `@At` names, in each method it picks: Mixin injects nothing
                        // into a method without one (a wildcard's), and needs one somewhere.
                        let mut placed = false;
                        let picked_any = !picked.is_empty();
                        for m in picked {
                            if let At::Invoke { ordinal, .. } = &at {
                                let n = calls(target, m).iter().filter(|c| at.names_call(c)).count();
                                if n == 0 || ordinal.is_some_and(|o| o >= n) {
                                    continue;
                                }
                            }
                            placed = true;
                            let w = Weave {
                                mixin: mixin.name.clone(),
                                handler: h.name.clone(),
                                handler_desc: h.desc.clone(),
                                handler_static: h.access & 0x0008 != 0,
                                kind: kind.clone(),
                                at: at.clone(),
                            };
                            match check(&w, m) {
                                Ok(()) => plan.weaves.entry(format!("{}.{}.{}", target.name, m.name, m.desc)).or_default().push(w),
                                Err(e) => {
                                    debug(|| format!("{e}: {}.{}{} -> {}.{}{}", mixin.name, h.name, h.desc, target.name, m.name, m.desc));
                                    count(&mut plan, format!("mixin {e}"))
                                }
                            }
                        }
                        if picked_any && !placed {
                            debug(|| format!("no call: {}.{} {at:?} in {}", mixin.name, h.name, target.name));
                            count(&mut plan, "mixin @At(INVOKE) names no call in its methods".into());
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
    match (&w.kind, &w.at) {
        (Kind::Inject { .. }, _) => {
            // The callback alone, or the target's arguments and the callback; more are locals.
            if n == 1 || n == target_args + 1 {
                Ok(())
            } else {
                Err("@Inject capturing locals".into())
            }
        }
        (Kind::ModifyReturnValue, At::Return | At::Tail) => {
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
        assert_eq!(at, At::Invoke { owner: "com/a/B".into(), name: "go".into(), desc: "(I)V".into(), ordinal: Some(1), after: true });
    }
}
