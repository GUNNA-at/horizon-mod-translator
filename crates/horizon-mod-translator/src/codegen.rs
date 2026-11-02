//! Bytecode → Rust. Each Java method becomes `fn(rt: &mut Rt, args: &[V]) -> V`
//! whose body is a `loop { match pc { ... } }` over its instructions, with
//! the operand stack and locals as `V` values. Calls to the mod's own
//! classes are direct Rust calls (virtual ones through a generated
//! dispatch table); calls to Minecraft, NeoForge and the JDK go to
//! `horizon_compat::api`. Everything is native code: no JVM at runtime.
use crate::{
    class::{ClassFile, Const, Member},
    insn::{self, Insn, Operand},
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

/// The translation of a set of classes (one jar).
pub struct Program<'a> {
    classes: BTreeMap<&'a str, &'a ClassFile>,
    /// Static strings, emitted once.
    keys: BTreeSet<String>,
    pub unsupported: BTreeMap<String, usize>,
    /// Lambda implementations seen: key → (handle kind, member).
    lambdas: BTreeMap<String, (u8, Member)>,
    /// The mod's id: picks its `@Mod` class among several (libraries bundled in its jar have theirs).
    pub mod_id: Option<String>,
    /// Methods (`name.desc`) that ask who called them (`StackWalker.getCallerClass`): calls to
    /// them record the calling class first. Everything else keeps no call stack.
    caller_sensitive: BTreeSet<String>,
}

impl<'a> Program<'a> {
    pub fn new(classes: &'a [ClassFile]) -> Program<'a> {
        Program {
            classes: classes.iter().map(|c| (c.name.as_str(), c)).collect(),
            keys: BTreeSet::new(),
            unsupported: BTreeMap::new(),
            lambdas: BTreeMap::new(),
            mod_id: None,
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
        }
    }

    /// The `@Mod` class FML would construct, and the constructor it would call (one taking the
    /// event bus): the class whose annotation names the mod id, or the only candidate.
    fn mod_main(&self) -> Option<(&'a ClassFile, &'a crate::class::Method)> {
        let utf8 = |c: &ClassFile, text: &str| c.pool.iter().any(|k| matches!(k, crate::class::Const::Utf8(s) if s == text));
        let mut found: Vec<(&'a ClassFile, &'a crate::class::Method)> = Vec::new();
        for c in self.classes.values() {
            if !utf8(c, "Lnet/neoforged/fml/common/Mod;") {
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
        if let Some(id) = &self.mod_id
            && let Some(hit) = found.iter().find(|(c, _)| utf8(c, id))
        {
            return Some(*hit);
        }
        (found.len() == 1).then(|| found[0])
    }

    fn own(&self, class: &str) -> bool {
        self.classes.contains_key(class)
    }

    /// The mod class that implements `name desc` for an object of `class`
    /// (walking up the mod's own superclasses).
    /// The class whose code runs for `name.desc` on an object of `class`, as the JVM resolves it:
    /// the class and its superclasses first, then default methods of its superinterfaces.
    fn resolve(&self, class: &str, name: &str, desc: &str) -> Option<String> {
        let start = self.classes.get(class)?;
        let mut chain = Vec::new();
        let mut c = Some(*start);
        while let Some(k) = c {
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
        let mut queue: std::collections::VecDeque<&str> = chain.iter().flat_map(|k| k.interfaces.iter().map(String::as_str)).collect();
        let mut seen = BTreeSet::new();
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            let Some(k) = self.classes.get(i) else { continue };
            if k.methods.iter().any(|m| m.name == name && m.desc == desc && m.code.is_some() && m.access & 0x000a == 0) {
                return Some(k.name.clone());
            }
            queue.extend(k.interfaces.iter().map(String::as_str));
        }
        None
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

    fn tables(&self, classes: &[&ClassFile]) -> String {
        let mut out = String::from("/// Registers the mod's class hierarchy and callbacks with the runtime. Call once, before anything else.\npub fn init(rt: &mut Rt) {\n    rt.vcall = Some(vcall);\n    rt.lambda_call = Some(lambda_call);\n    rt.overrides = Some(overrides);\n");
        for c in classes {
            if let Some(sup) = &c.superclass {
                let _ = writeln!(out, "    rt.supers.insert({:?}, {sup:?});", c.name);
            }
            if !c.interfaces.is_empty() {
                let _ = writeln!(out, "    rt.interfaces.insert({:?}, &{:?});", c.name, c.interfaces);
            }
        }
        out.push_str("}\n\n");
        // Mod loading's registration phase: classes holding registers initialise, then suppliers resolve.
        out.push_str("/// Runs the mod's registrations, as NeoForge's loading does. Call after `init`.\npub fn start(rt: &mut Rt) {\n");
        for c in classes {
            if c.fields.iter().any(|f| f.access & 0x0008 != 0 && f.desc.contains("neoforged/neoforge/registries/DeferredRegister")) {
                let _ = writeln!(out, "    {}", self.ensure(&c.name).replace("if rt.thrown.is_some() { return V::Void; }", &format!("rt.report_thrown({:?});", c.name)));
            }
        }
        // FML constructs the `@Mod` class with the mod bus, then fires the registration events.
        if let Some((main, ctor)) = self.mod_main() {
            let params = crate::class::param_classes(&ctor.desc);
            let _ = writeln!(out, "    {{\n        let this = rt.new_object({:?});", main.name);
            let _ = writeln!(out, "        let mut args = vec![this];");
            for p in &params {
                let _ = writeln!(out, "        let v = horizon_compat::neo::constructor_arg(rt, {p:?}); args.push(v);");
            }
            let _ = writeln!(out, "        {}", self.ensure(&main.name).replace("if rt.thrown.is_some() { return V::Void; }", &format!("rt.report_thrown({:?});", main.name)));
            let _ = writeln!(out, "        {}(rt, &args);\n        rt.report_thrown(\"the mod's constructor\");\n    }}", fn_name(&main.name, "<init>", &ctor.desc));
            out.push_str("    horizon_compat::neo::fire_registrations(rt);\n");
        }
        out.push_str("    rt.resolve_registrations();\n}\n\n");
        out.push_str(&self.dispatch());
        out.push_str(&self.lambda_table());
        out
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
                }
            }
        }
        out.push_str(&self.tables(&classes));
        out
    }

    /// `vcall(rt, key, args)`: virtual and interface calls, by the receiver's class.
    fn dispatch(&self) -> String {
        let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
        for c in self.classes.values() {
            for m in c.methods.iter().filter(|m| m.code.is_some() && m.access & 0x0008 == 0) {
                if m.name != "<init>" {
                    keys.insert((m.name.clone(), m.desc.clone()));
                }
            }
        }
        let mut s = String::from(
            "/// `String.valueOf(v)` for an object: its mod class's own `toString()`, an enum constant's name,\n\
             /// or `v` itself (strings, boxes and API objects, which the runtime prints).\n\
             pub fn java_string(rt: &mut Rt, v: V) -> V {\n\
             \x20   if !matches!(v, V::R(_)) || v.r() == 0 || rt.str_of(v).is_some() { return v; }\n\
             \x20   let class = rt.class_of(v).to_owned();\n\
             \x20   if overrides(&class, \"toString.()Ljava/lang/String;\") {\n\
             \x20       return vcall(rt, \"java/lang/Object.toString.()Ljava/lang/String;\", \"toString.()Ljava/lang/String;\", &[v]);\n\
             \x20   }\n\
             \x20   let name = rt.get_field(v, \"$name\");\n\
             \x20   if rt.str_of(name).is_some() { name } else { v }\n\
             }\n\n\
             /// Virtual dispatch over the mod's classes; API calls when no mod class implements it.\n\
             pub fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n\
             \x20   let class = rt.class_of(args.first().copied().unwrap_or_default());\n\
             \x20   vcall_as(rt, class, owner_key, key, args)\n\
             }\n\n\
             /// `vcall` as if the receiver were of class `class` (a lambda's interface, for its default methods).\n\
             pub fn vcall_as(rt: &mut Rt, class: &str, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n\
             \x20   match (class, key) {\n",
        );
        for class in self.classes.keys() {
            for (name, desc) in &keys {
                if let Some(imp) = self.resolve(class, name, desc) {
                    let _ = writeln!(
                        s,
                        "        ({class:?}, {:?}) => {}(rt, args),",
                        format!("{name}.{desc}"),
                        fn_name(&imp, name, desc)
                    );
                }
            }
        }
        let mut table = String::from("/// Whether a mod class implements `key` (`name.desc`) itself or through a mod superclass.\npub fn overrides(class: &str, key: &str) -> bool {\n    matches!((class, key),\n        (\"\", \"\")");
        for class in self.classes.keys() {
            for (name, desc) in &keys {
                if self.resolve(class, name, desc).is_some() {
                    let _ = write!(table, "\n        | ({class:?}, {:?})", format!("{name}.{desc}"));
                }
            }
        }
        table.push_str(")\n}\n\n");
        s.insert_str(0, &table);
        s.push_str(concat!(
            "        _ => {\n",
            "            let recv = args.first().copied().unwrap_or_default();\n",
            "            match rt.lambda_info.get(&recv.r()).copied() {\n",
            "                // A lambda: its own method runs its implementation; anything else is a default\n",
            "                // method of its interface (the mod's, or the API's).\n",
            "                Some((iface, sam)) if !(key.starts_with(sam) && key.as_bytes().get(sam.len()) == Some(&b'.')) => {\n",
            "                    if class != iface { vcall_as(rt, iface, owner_key, key, args) } else { api::call(rt, owner_key, args) }\n",
            "                }\n",
            "                _ => match lambda_call(rt, rt.class_of(recv), args) {\n",
            "                    Some(r) => r,\n",
            "                    None => api::call(rt, owner_key, args),\n",
            "                },\n",
            "            }\n",
            "        }\n",
            "    }\n}\n"
        ));
        s
    }

    /// Calls on lambda objects: captured values first, then the call's own arguments.
    fn lambda_table(&self) -> String {
        let mut s = String::from(
            "/// A call on a lambda object (its class is the implementation's key).\n\
             pub fn lambda_call(rt: &mut Rt, class: &str, args: &[V]) -> Option<V> {\n\
             \x20   let mut full = rt.captured(args.first().copied().unwrap_or_default());\n\
             \x20   full.extend_from_slice(args.get(1..).unwrap_or_default());\n\
             \x20   Some(match class {\n",
        );
        for (key, (kind, m)) in &self.lambdas {
            let call = match (kind, self.resolve(&m.owner, &m.name, &m.desc)) {
                (8, imp) => {
                    let init = match imp {
                        Some(imp) => format!("{}(rt, &a);", fn_name(&imp, &m.name, &m.desc)),
                        None => format!("api::call(rt, {key:?}, &a);"),
                    };
                    format!("{{ let o = rt.new_object({:?}); let mut a = vec![o]; a.extend(full); {init} o }}", m.owner)
                }
                (5 | 9, _) => format!("vcall(rt, {key:?}, {:?}, &full)", format!("{}.{}", m.name, m.desc)),
                (_, Some(imp)) => format!("{}(rt, &full)", fn_name(&imp, &m.name, &m.desc)),
                (_, None) => format!("api::call(rt, {key:?}, &full)"),
            };
            let _ = writeln!(s, "        {key:?} => {call},");
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
                format!("{collect} let v = rt.lambda({key:?}, a); rt.lambda_info.insert(v.r(), ({iface:?}, {_name:?})); s.push(v); {go}")
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
        let _ = writeln!(s, "/// `{}.{}{}`", c.name, name, desc);
        let _ = writeln!(s, "pub fn {f}(rt: &mut Rt, args: &[V]) -> V {{");
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
            let test = if class.is_empty() { "true".to_owned() } else { format!("rt.instance_of(e, {class:?})") };
            let _ = writeln!(
                s,
                "                if thrown_at >= {start} && thrown_at < {end} && {test} {{ rt.thrown = None; s.clear(); s.push(e); pc = {handler}; continue; }}"
            );
        }
        let _ = writeln!(s, "                return V::Void;");
        let _ = writeln!(s, "            }}");
        let _ = writeln!(s, "            _ => return V::Void,");
        let _ = writeln!(s, "        }}\n    }}\n}}\n");
        s
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
            0xac..=0xb0 => format!("return {pop};"),
            0xb1 => "return V::Void;".into(),
            0xb2..=0xb5 => self.field(c, i.op, idx as u16, &go),
            0xb6..=0xb9 => self.invoke(c, i.op, idx as u16, &go, i.at),
            0xba => self.indy(c, idx as u16, &go),
            0xbb => {
                let class = c.class_name(idx as u16).unwrap_or_default();
                let init = if class != c.name { self.ensure(&class) } else { String::new() };
                format!("{init}let v = rt.new_object({class:?}); s.push(v); {go}")
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
                format!("let a = {pop}; s.push(V::I(rt.instance_of(a, {class:?}) as i32)); {go}")
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
    fn ensure(&self, class: &str) -> String {
        let has = self.classes.get(class).is_some_and(|c| c.methods.iter().any(|m| m.name == "<clinit>"));
        if !has {
            return String::new();
        }
        format!(
            "if rt.initialized.insert({class:?}) {{ {}(rt, &[]); if rt.thrown.is_some() {{ return V::Void; }} }} ",
            fn_name(class, "<clinit>", "()V")
        )
    }

    fn field(&mut self, c: &ClassFile, op: u8, idx: u16, go: &str) -> String {
        let Some(m) = c.member(idx) else { return go.to_owned() };
        let init = if matches!(op, 0xb2 | 0xb3) && m.owner != c.name { self.ensure(&m.owner) } else { String::new() };
        let pop = "s.pop().unwrap_or_default()";
        let skey = format!("{}.{}", m.owner, m.name);
        match op {
            0xb2 if self.own(&m.owner) => format!("{init}let v = rt.statics.get({skey:?}).copied().unwrap_or_default(); s.push(v); {go}"),
            0xb2 => format!("let v = api::get_static(rt, {skey:?}); s.push(v); {go}"),
            0xb3 => format!("{init}let v = {pop}; rt.statics.insert({skey:?}, v); {go}"),
            0xb4 => format!("let o = {pop}; s.push(rt.get_field(o, {:?})); {go}", m.name),
            _ => format!("let v = {pop}; let o = {pop}; rt.put_field(o, {:?}, v); {go}", m.name),
        }
    }

    fn invoke(&mut self, c: &ClassFile, op: u8, idx: u16, go: &str, at: usize) -> String {
        let Some(m) = c.member(idx) else { return go.to_owned() };
        let n = arg_kinds(&m.desc).len() + usize::from(op != 0xb8);
        let key = self.key(&m);
        let init = if op == 0xb8 && m.owner != c.name { self.ensure(&m.owner) } else { String::new() };
        let caller = if self.caller_sensitive.contains(&format!("{}.{}", m.name, m.desc)) {
            format!("rt.caller_class = {:?}; ", c.name)
        } else {
            String::new()
        };
        let collect = format!("{init}{caller}let at = s.len().saturating_sub({n}); let args: Vec<V> = s.split_off(at);");
        let push = if returns_value(&m.desc) { "s.push(r);" } else { "" };
        let call = match op {
            // static, or a direct (super/constructor/private) call
            0xb8 | 0xb7 => match self.resolve(&m.owner, &m.name, &m.desc) {
                Some(imp) => format!("{}(rt, &args)", fn_name(&imp, &m.name, &m.desc)),
                None => format!("api::call(rt, {key:?}, &args)"),
            },
            _ => format!("vcall(rt, {key:?}, {:?}, &args)", format!("{}.{}", m.name, m.desc)),
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
        format!("{collect} let r = {call}; if rt.thrown.is_some() {{ thrown_at = {at}; pc = {THROW}; }} else {{ {push} {go} }}")
    }
}
