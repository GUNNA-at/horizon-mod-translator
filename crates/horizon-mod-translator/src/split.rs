//! A big translated engine as several crates (a mod's bundle crates may depend on each other), so
//! that each compiles in a `rustc` of its own, in parallel, and none holds the whole engine: Create
//! and ten addons as one 177 MB crate needed a ~44 GB footprint.
//!
//! The code goes into crates of about `crate_bytes` each, one mod's classes at a time. Each part
//! carries what it registers with the engine's class space (its classes, methods and lambdas:
//! `horizon_compat::linked`), and a virtual call is the runtime's dispatch, so the engine crate on
//! top holds no table of its own: it lists its crates' registries and links them. A code crate
//! calls a function of another through a shim of its own, linked by name when the engine starts
//! (each crate exports the functions the others call), so a crate's text depends only on its own
//! code.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// A crate of translated code: its id (its folder in the bundle) and its `src/` files.
pub struct CodeCrate {
    pub id: String,
    pub files: Vec<(String, String)>,
}

/// What [`split`] makes: the code crates, and what the engine's `lib.rs` adds to use and link
/// them (`all_registries` and `link_crates`, which its `linked()` calls).
pub struct Split {
    pub crates: Vec<CodeCrate>,
    pub engine: String,
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The identifiers in `text` that may name a function of another crate or a dispatcher.
fn idents(text: &str) -> impl Iterator<Item = &str> {
    let b = text.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            if !word(b[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && word(b[i]) {
                i += 1;
            }
            let id = &text[start..i];
            if id.starts_with("j_") || id.starts_with("d_") || id == "vcall_miss" || id == "java_string" {
                return Some(id);
            }
        }
        None
    })
}

/// The functions `text` defines for the rest of its crate: `pub fn NAME(` at a line's start.
fn defined(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("pub fn ").filter_map(move |(at, _)| {
        if at != 0 && text.as_bytes()[at - 1] != b'\n' {
            return None;
        }
        let rest = &text[at + 7..];
        let end = rest.bytes().position(|c| !word(c))?;
        Some(&rest[..end])
    })
}

/// The Rust name of the bundle crate `id` of a bundle whose packages start with `prefix`.
pub fn crate_name(prefix: &str, id: &str) -> String {
    format!("{prefix}_{id}").replace('-', "_")
}

/// What one code crate calls outside itself.
#[derive(Default)]
struct Calls<'a> {
    /// Functions of the other crates.
    fns: BTreeSet<&'a str>,
    /// Dispatchers (`d_...`).
    dispatchers: BTreeSet<&'a str>,
    miss: bool,
    string: bool,
}

/// Splits `groups` (each a mod's id and its parts, from
/// [`crate::codegen::Program::emit_groups`]) into crates of about `crate_bytes`, one mod's parts at
/// a time. `runtime` is the runtime crate's Rust name, `prefix` the bundle's (a crate `id` is
/// `{prefix}_{id}`), and `dispatchers` each dispatcher's method key
/// ([`crate::codegen::Program::dispatchers`]).
pub fn split(groups: &[(String, Vec<String>)], runtime: &str, prefix: &str, crate_bytes: usize, dispatchers: &BTreeMap<String, String>) -> Split {
    // Parts into crates, in order.
    let mut layout: Vec<(String, Vec<&String>)> = Vec::new();
    for (group, parts) in groups {
        let (mut n, mut size) = (0, 0);
        for part in parts {
            // A mod's first part opens a crate; a part that would take one over the size, another.
            if n == 0 || size + part.len() > crate_bytes {
                layout.push((format!("code-{group}-{n}"), Vec::new()));
                n += 1;
                size = 0;
            }
            size += part.len();
            layout.last_mut().expect("pushed above").1.push(part);
        }
    }
    let mut owner: BTreeMap<&str, usize> = BTreeMap::new();
    for (k, (_, parts)) in layout.iter().enumerate() {
        for part in parts {
            for name in defined(part) {
                owner.insert(name, k);
            }
        }
    }
    let calls: Vec<Calls> = layout
        .iter()
        .enumerate()
        .map(|(k, (_, parts))| {
            let mut c = Calls::default();
            for part in parts {
                for name in idents(part) {
                    match name {
                        "vcall_miss" => c.miss = true,
                        "java_string" => c.string = true,
                        _ if owner.get(name) == Some(&k) => {}
                        _ if name.starts_with("d_") && dispatchers.contains_key(name) => {
                            c.dispatchers.insert(name);
                        }
                        _ if name.starts_with("j_") && owner.contains_key(name) => {
                            c.fns.insert(name);
                        }
                        // A word in a string or a comment.
                        _ => {}
                    }
                }
            }
            c
        })
        .collect();
    // What each crate exports: its functions another crate calls.
    let mut exports: Vec<BTreeSet<&str>> = vec![BTreeSet::new(); layout.len()];
    for c in &calls {
        for name in &c.fns {
            exports[owner[name]].insert(name);
        }
    }
    let mut crates = Vec::new();
    for ((id, parts), (calls, exports)) in layout.iter().zip(calls.iter().zip(&exports)) {
        let mut lib = format!(
            "// Generated by horizon-mod-translator. Do not edit.\n\
             #![allow(unused_mut, unused_variables, unused_assignments, unreachable_code, non_snake_case, unused_imports, dead_code, clippy::all)]\n\
             //! Translated code, linked into the engine's other crates by the engine's `init`.\n\
             pub(crate) use {runtime} as horizon_compat;\n\
             use horizon_compat::{{Rt, V}};\n\
             mod links;\n\
             pub use links::link;\n\
             pub(crate) use links::*;\n\n"
        );
        let mut files = Vec::new();
        let mut registries = Vec::new();
        for (n, part) in parts.iter().enumerate() {
            let _ = writeln!(lib, "mod part{n};\npub use part{n}::*;");
            registries.extend(crate::codegen::registries_in(part));
            files.push((format!("part{n}.rs"), (*part).clone()));
        }
        let _ = write!(
            lib,
            "\n/// What this crate's parts register with the engine's class space.\npub static REGISTRIES: &[&horizon_compat::linked::Registry] = &[{}];\n\n/// The functions of this crate the engine's other crates call, by name (sorted).\npub static EXPORTS: &[(&str, horizon_compat::linked::Method)] = &[\n",
            registries.iter().map(|r| format!("&{r}")).collect::<Vec<_>>().join(", ")
        );
        for name in exports {
            let _ = writeln!(lib, "    ({name:?}, {name}),");
        }
        lib.push_str("];\n");
        files.push(("links.rs".into(), links(calls, dispatchers)));
        files.push(("lib.rs".into(), lib));
        crates.push(CodeCrate { id: id.clone(), files });
    }
    // The engine: it calls into every crate (the mods' loading), and links each.
    let mut out = String::new();
    for (id, _) in &layout {
        let _ = writeln!(out, "use {}::*;", crate_name(prefix, id));
    }
    let names: Vec<String> = layout.iter().map(|(id, _)| crate_name(prefix, id)).collect();
    let _ = write!(
        out,
        "\n/// Every part's registry, crate by crate.\nfn all_registries() -> Vec<&'static horizon_compat::linked::Registry> {{\n    [{}].concat()\n}}\n\n\
         /// Links each code crate's calls into the others, by name (once per process).\nfn link_crates() {{\n    let mut fns: Vec<(&'static str, horizon_compat::linked::Method)> = [{}].concat();\n    fns.sort_unstable_by(|a, b| a.0.cmp(b.0));\n",
        names.iter().map(|n| format!("{n}::REGISTRIES")).collect::<Vec<_>>().join(", "),
        names.iter().map(|n| format!("{n}::EXPORTS")).collect::<Vec<_>>().join(", ")
    );
    for n in &names {
        let _ = writeln!(out, "    {n}::link(&fns);");
    }
    out.push_str("}\n");
    Split { crates, engine: out }
}

/// A code crate's `links.rs`: a shim for each function it calls in another crate, each dispatcher
/// it calls (the runtime's dispatch with the key's hash), and the runtime's `vcall_miss` and
/// `java_string` when it calls them.
fn links(calls: &Calls, dispatchers: &BTreeMap<String, String>) -> String {
    let mut s = String::from(
        "//! What this crate calls outside itself: the engine's other crates' functions, linked by name\n\
         //! when the engine starts, and the runtime's dispatch.\n\
         use crate::horizon_compat::{self, Rt, V};\n\
         use crate::horizon_compat::linked::Method;\n\
         use std::sync::OnceLock;\n\n",
    );
    let names = calls.fns.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let _ = writeln!(s, "const FN_NAMES: [&str; {}] = [{names}];", calls.fns.len());
    s.push_str(
        "static FNS: OnceLock<Box<[Method]>> = OnceLock::new();\n\
         const UNLINKED: &str = \"the engine links its crates before any code runs\";\n\n\
         /// Links this crate's calls into the others (the engine does, once per process): `fns` by\n\
         /// name, sorted.\n\
         pub fn link(fns: &[(&str, Method)]) {\n\
         \x20   let _ = FNS.set(\n\
         \x20       FN_NAMES\n\
         \x20           .iter()\n\
         \x20           .map(|n| match fns.binary_search_by(|(k, _)| (*k).cmp(n)) {\n\
         \x20               Ok(i) => fns[i].1,\n\
         \x20               Err(_) => panic!(\"no crate of the engine defines {n}\"),\n\
         \x20           })\n\
         \x20           .collect(),\n\
         \x20   );\n\
         }\n\n",
    );
    for (i, name) in calls.fns.iter().enumerate() {
        let _ = writeln!(s, "pub(crate) fn {name}(rt: &mut Rt, args: &[V]) -> V {{\n    (FNS.get().expect(UNLINKED)[{i}])(rt, args)\n}}");
    }
    for name in &calls.dispatchers {
        s.push_str(&crate::codegen::dispatcher_shim(&dispatchers[*name], "pub(crate) "));
    }
    if calls.miss {
        let _ = write!(s, "pub(crate) {}", crate::codegen::MISS_SHIM);
    }
    if calls.string {
        let _ = write!(s, "pub(crate) {}", crate::codegen::STRING_SHIM);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(defs: &[&str], calls: &[&str]) -> String {
        let mut s = String::from(crate::codegen::PART_HEADER);
        for d in defs {
            let body: Vec<String> = calls.iter().map(|c| format!("{c}(rt, args);")).collect();
            let _ = writeln!(s, "pub fn {d}(rt: &mut Rt, args: &[V]) -> V {{ {} V::Void }}", body.join(" "));
        }
        s
    }

    #[test]
    fn calls_between_crates_go_through_links_and_calls_within_stay_direct() {
        let run = crate::codegen::dispatcher("run.()V");
        let dispatchers: BTreeMap<String, String> = [(run.clone(), "run.()V".to_owned())].into();
        let groups = vec![
            // One crate per part: each is over the size.
            ("base".to_owned(), vec![part(&["j_a_1"], &["j_b_2"]), part(&["j_b_2"], &["j_a_1", &run, "vcall_miss"])]),
            ("addon".to_owned(), vec![part(&["j_c_3"], &["j_a_1", "j_c_3"])]),
        ];
        let split = split(&groups, "m_translated_runtime", "m_translated", 10, &dispatchers);
        let ids: Vec<&str> = split.crates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["code-base-0", "code-base-1", "code-addon-0"]);
        let file = |k: usize, name: &str| split.crates[k].files.iter().find(|(n, _)| n == name).map(|(_, t)| t.clone()).unwrap();
        assert!(file(0, "links.rs").contains("const FN_NAMES: [&str; 1] = [\"j_b_2\"];"), "{}", file(0, "links.rs"));
        assert!(file(1, "links.rs").contains("const FN_NAMES: [&str; 1] = [\"j_a_1\"];"));
        // A dispatcher is the runtime's dispatch with its key's hash; `vcall_miss` the runtime's.
        let hash = format!("{:#x}", horizon_compat::linked::key_hash("run.()V"));
        assert!(file(1, "links.rs").contains(&format!("pub(crate) fn {run}(rt: &mut Rt, cid: u32, owner_key: &'static str, args: &[V]) -> V {{\n    horizon_compat::linked::dispatch(rt, cid, {hash}, \"run.()V\", owner_key, args)\n}}")));
        assert!(file(1, "links.rs").contains("pub(crate) fn vcall_miss("));
        // The addon calls its own function directly and the base's through a link.
        assert!(file(2, "links.rs").contains("const FN_NAMES: [&str; 1] = [\"j_a_1\"];"));
        assert!(!file(2, "links.rs").contains("fn j_c_3"));
        // Each crate exports what the others call of it, and the engine links them all.
        assert!(file(0, "lib.rs").contains("pub static EXPORTS: &[(&str, horizon_compat::linked::Method)] = &[\n    (\"j_a_1\", j_a_1),\n];"), "{}", file(0, "lib.rs"));
        assert!(file(1, "lib.rs").contains("    (\"j_b_2\", j_b_2),\n];"));
        assert!(file(2, "lib.rs").contains("EXPORTS: &[(&str, horizon_compat::linked::Method)] = &[\n];"));
        assert!(split.engine.contains("    m_translated_code_addon_0::link(&fns);"), "{}", split.engine);
        assert!(split.engine.contains("[m_translated_code_base_0::REGISTRIES, m_translated_code_base_1::REGISTRIES, m_translated_code_addon_0::REGISTRIES].concat()"));
        // Under the size, a mod's parts share a crate and call each other directly; mods never share.
        let split = super::split(&groups, "m_translated_runtime", "m_translated", 1 << 20, &dispatchers);
        let ids: Vec<&str> = split.crates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["code-base-0", "code-addon-0"]);
        let base = split.crates[0].files.iter().find(|(n, _)| n == "links.rs").unwrap();
        assert!(base.1.contains("const FN_NAMES: [&str; 0] = [];"));
    }

    /// A part's registry is named in its crate's list of them.
    #[test]
    fn a_crate_lists_its_parts_registries() {
        let mut a = part(&["j_a_1"], &[]);
        a.push_str("pub static REGISTRY_4: Registry = Registry { classes: &[], lambdas: &[], added: &[], game_interfaces: &[], subscriber_options: &[] };\n");
        let mut b = part(&["j_b_2"], &[]);
        b.push_str("pub static REGISTRY_5: Registry = Registry { classes: &[], lambdas: &[], added: &[], game_interfaces: &[], subscriber_options: &[] };\n");
        let split = split(&[("base".to_owned(), vec![a, b])], "m_translated_runtime", "m_translated", 1 << 20, &BTreeMap::new());
        let lib = &split.crates[0].files.iter().find(|(n, _)| n == "lib.rs").unwrap().1;
        assert!(lib.contains("pub static REGISTRIES: &[&horizon_compat::linked::Registry] = &[&REGISTRY_4, &REGISTRY_5];"), "{lib}");
    }
}
