//! A big translated engine as several crates (a mod's bundle crates may depend on each other), so
//! that each compiles in a `rustc` of its own, in parallel, and none holds the whole engine: Create
//! and ten addons as one 177 MB crate needed a ~44 GB footprint.
//!
//! The code goes into crates of about `crate_bytes` each, one mod's classes at a time. The engine
//! crate on top holds the tables, the dispatchers and `init`, and depends on them all. A code crate
//! calls a function of another, or one of the engine's dispatchers, through a shim of its own: the
//! engine's `init` links each crate by name, so a crate's text depends only on its own code.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// A crate of translated code: its id (its folder in the bundle) and its `src/` files.
pub struct CodeCrate {
    pub id: String,
    pub files: Vec<(String, String)>,
}

/// What [`split`] makes: the code crates, and what the engine's `lib.rs` adds to use and link
/// them (its `init` calls `link_crates()` first).
pub struct Split {
    pub crates: Vec<CodeCrate>,
    pub engine: String,
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The identifiers in `text` that may name a function of another crate or of the engine.
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

/// Splits `groups` (each a mod's id and its parts, from
/// [`crate::codegen::Program::emit_groups`]) into crates of about `crate_bytes`, one mod's parts at
/// a time. `engine` is the engine's `lib.rs` as emitted (what it defines), `runtime` the runtime
/// crate's Rust name, and `prefix` the bundle's (a crate `id` is `{prefix}_{id}`).
pub fn split(groups: &[(String, Vec<String>)], engine: &str, runtime: &str, prefix: &str, crate_bytes: usize) -> Split {
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
    let engine_defined: BTreeSet<&str> = defined(engine).collect();
    let mut all_fns: BTreeSet<&str> = BTreeSet::new();
    let mut all_dispatchers: BTreeSet<&str> = BTreeSet::new();
    let mut crates = Vec::new();
    for (k, (id, parts)) in layout.iter().enumerate() {
        let mut fns: BTreeSet<&str> = BTreeSet::new();
        let mut dispatchers: BTreeSet<&str> = BTreeSet::new();
        let (mut miss, mut string) = (false, false);
        for part in parts {
            for name in idents(part) {
                match name {
                    "vcall_miss" => miss = true,
                    "java_string" => string = true,
                    _ if owner.get(name) == Some(&k) => {}
                    _ if name.starts_with("d_") && engine_defined.contains(name) => {
                        dispatchers.insert(name);
                    }
                    _ if name.starts_with("j_") && (owner.contains_key(name) || engine_defined.contains(name)) => {
                        fns.insert(name);
                    }
                    // A word in a string or a comment.
                    _ => {}
                }
            }
        }
        all_fns.extend(&fns);
        all_dispatchers.extend(&dispatchers);
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
        for (n, part) in parts.iter().enumerate() {
            let _ = writeln!(lib, "mod part{n};\npub use part{n}::*;");
            files.push((format!("part{n}.rs"), (*part).clone()));
        }
        files.push(("links.rs".into(), links(&fns, &dispatchers, miss, string)));
        files.push(("lib.rs".into(), lib));
        crates.push(CodeCrate { id: id.clone(), files });
    }
    // The engine: its tables call into every crate, and it links each.
    let mut out = String::new();
    for (id, _) in &layout {
        let _ = writeln!(out, "use {}::*;", crate_name(prefix, id));
    }
    out.push_str("\n/// The functions one of the engine's crates calls in another, by name (sorted).\nstatic LINK_FNS: &[(&str, fn(&mut Rt, &[V]) -> V)] = &[\n");
    for name in &all_fns {
        let _ = writeln!(out, "    ({name:?}, {name}),");
    }
    out.push_str("];\n\n/// The dispatchers the engine's crates call, by name (sorted).\nstatic LINK_DISPATCHERS: &[(&str, horizon_compat::Dispatch)] = &[\n");
    for name in &all_dispatchers {
        let _ = writeln!(out, "    ({name:?}, {name}),");
    }
    out.push_str("];\n\n/// Links each code crate's calls into the others and the engine (once per process).\nfn link_crates() {\n");
    for (id, _) in &layout {
        let _ = writeln!(out, "    {}::link(LINK_FNS, LINK_DISPATCHERS, vcall_miss, java_string);", crate_name(prefix, id));
    }
    out.push_str("}\n");
    Split { crates, engine: out }
}

/// A code crate's `links.rs`: a shim for each function it calls in another crate (`fns`) and each
/// dispatcher (`dispatchers`), and the engine's `vcall_miss` and `java_string` when it calls them.
fn links(fns: &BTreeSet<&str>, dispatchers: &BTreeSet<&str>, miss: bool, string: bool) -> String {
    let mut s = String::from(
        "//! What this crate calls in the engine's other crates, linked by name when the engine starts.\n\
         use crate::horizon_compat::{Dispatch, Rt, V};\n\
         use std::sync::OnceLock;\n\n\
         type Method = fn(&mut Rt, &[V]) -> V;\n\
         type Miss = fn(&mut Rt, u32, &'static str, &'static str, &[V]) -> V;\n\
         type JavaString = fn(&mut Rt, V) -> V;\n\n",
    );
    let names = |set: &BTreeSet<&str>| set.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let _ = writeln!(s, "const FN_NAMES: [&str; {}] = [{}];", fns.len(), names(fns));
    let _ = writeln!(s, "const DISPATCHER_NAMES: [&str; {}] = [{}];", dispatchers.len(), names(dispatchers));
    s.push_str(
        "static FNS: OnceLock<Box<[Method]>> = OnceLock::new();\n\
         static DISPATCHERS: OnceLock<Box<[Dispatch]>> = OnceLock::new();\n\
         static MISS: OnceLock<Miss> = OnceLock::new();\n\
         static JAVA_STRING: OnceLock<JavaString> = OnceLock::new();\n\
         const UNLINKED: &str = \"the engine's init links its crates before any code runs\";\n\n\
         fn find<T: Copy>(table: &[(&str, T)], name: &str) -> T {\n\
         \x20   match table.binary_search_by(|(k, _)| (*k).cmp(name)) {\n\
         \x20       Ok(i) => table[i].1,\n\
         \x20       Err(_) => panic!(\"no crate of the engine defines {name}\"),\n\
         \x20   }\n\
         }\n\n\
         /// Links this crate's calls (the engine's `init` does, once per process): `fns` and\n\
         /// `dispatchers` by name, sorted.\n\
         pub fn link(fns: &[(&str, Method)], dispatchers: &[(&str, Dispatch)], miss: Miss, java_string: JavaString) {\n\
         \x20   let _ = FNS.set(FN_NAMES.iter().map(|n| find(fns, n)).collect());\n\
         \x20   let _ = DISPATCHERS.set(DISPATCHER_NAMES.iter().map(|n| find(dispatchers, n)).collect());\n\
         \x20   let _ = MISS.set(miss);\n\
         \x20   let _ = JAVA_STRING.set(java_string);\n\
         }\n\n",
    );
    for (i, name) in fns.iter().enumerate() {
        let _ = writeln!(s, "pub(crate) fn {name}(rt: &mut Rt, args: &[V]) -> V {{\n    (FNS.get().expect(UNLINKED)[{i}])(rt, args)\n}}");
    }
    for (i, name) in dispatchers.iter().enumerate() {
        let _ = writeln!(
            s,
            "pub(crate) fn {name}(rt: &mut Rt, cid: u32, owner_key: &'static str, args: &[V]) -> V {{\n    (DISPATCHERS.get().expect(UNLINKED)[{i}])(rt, cid, owner_key, args)\n}}"
        );
    }
    if miss {
        s.push_str("pub(crate) fn vcall_miss(rt: &mut Rt, cid: u32, owner_key: &'static str, key: &'static str, args: &[V]) -> V {\n    (MISS.get().expect(UNLINKED))(rt, cid, owner_key, key, args)\n}\n");
    }
    if string {
        s.push_str("pub(crate) fn java_string(rt: &mut Rt, v: V) -> V {\n    (JAVA_STRING.get().expect(UNLINKED))(rt, v)\n}\n");
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
        let engine = "pub fn d_run_1(rt: &mut Rt, cid: u32, owner_key: &'static str, args: &[V]) -> V { V::Void }\npub fn vcall_miss() {}\npub fn java_string() {}\n";
        let groups = vec![
            // One crate per part: each is over the size.
            ("base".to_owned(), vec![part(&["j_a_1"], &["j_b_2"]), part(&["j_b_2"], &["j_a_1", "d_run_1", "vcall_miss"])]),
            ("addon".to_owned(), vec![part(&["j_c_3"], &["j_a_1", "j_c_3"])]),
        ];
        let split = split(&groups, engine, "m_translated_runtime", "m_translated", 10);
        let ids: Vec<&str> = split.crates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["code-base-0", "code-base-1", "code-addon-0"]);
        let links = |k: usize| split.crates[k].files.iter().find(|(n, _)| n == "links.rs").map(|(_, t)| t.clone()).unwrap();
        assert!(links(0).contains("const FN_NAMES: [&str; 1] = [\"j_b_2\"];"), "{}", links(0));
        assert!(links(1).contains("const FN_NAMES: [&str; 1] = [\"j_a_1\"];"));
        assert!(links(1).contains("const DISPATCHER_NAMES: [&str; 1] = [\"d_run_1\"];"));
        assert!(links(1).contains("pub(crate) fn vcall_miss("));
        // The addon calls its own function directly and the base's through a link.
        assert!(links(2).contains("const FN_NAMES: [&str; 1] = [\"j_a_1\"];"));
        assert!(!links(2).contains("fn j_c_3"));
        assert!(split.engine.contains("    (\"j_a_1\", j_a_1),\n    (\"j_b_2\", j_b_2),\n];"), "{}", split.engine);
        assert!(split.engine.contains("    m_translated_code_addon_0::link(LINK_FNS, LINK_DISPATCHERS, vcall_miss, java_string);"));
        // Under the size, a mod's parts share a crate and call each other directly; mods never share.
        let split = super::split(&groups, engine, "m_translated_runtime", "m_translated", 1 << 20);
        let ids: Vec<&str> = split.crates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["code-base-0", "code-addon-0"]);
        let base = split.crates[0].files.iter().find(|(n, _)| n == "links.rs").unwrap();
        assert!(base.1.contains("const FN_NAMES: [&str; 0] = [];"));
    }
}
