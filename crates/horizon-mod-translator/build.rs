//! The Horizon commit this translator is built from, which its bundles pin
//! (`[compatibility] horizon_revision`): their generated code calls the game's hook points and the
//! compat runtime of exactly that commit. From `HORIZON_TRANSLATOR_REVISION`, else `GITHUB_SHA`
//! (a GitHub Actions build), else the checkout's `HEAD`; none outside a checkout.
use std::{path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=HORIZON_TRANSLATOR_REVISION");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let given = ["HORIZON_TRANSLATOR_REVISION", "GITHUB_SHA"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|r| is_revision(r)));
    if let Some(revision) = given.or_else(head) {
        println!("cargo:rustc-env=HORIZON_TRANSLATOR_REVISION={revision}");
    }
    named_classes();
}

/// The game and loader classes the runtime's source (and this translator's) names, sorted, as
/// `NAMED_CLASSES` in `$OUT_DIR/named_classes.rs`, and the packages it names (`.../`, a family it
/// answers for by the class's short name) as `NAMED_PACKAGES`: a class one of them names is
/// modelled there, or its objects may come from there. Any other class of the game is code like
/// a library's (`horizon_mod_translator::runtime_names`).
fn named_classes() {
    /// The runtime's generated table of every class of the game's with what is above it: a
    /// name there is no model of the class (read for names, it made each of them the
    /// runtime's, and none of the game's own code was carried).
    const EVERY_CLASS: &str = "game_classes.rs";
    /// The runtime's generated table of vanilla's records, data classes and enums: a row is
    /// about the class it starts with, a whole text there. The types of a record's
    /// components, of a constructor's arguments and of what a getter gives stand inside the
    /// row's other text and say nothing of who models them (`com/mojang/math/Transformation`
    /// was the runtime's for being a model state's component, with nothing to answer for it).
    const ROWS: &str = "vanilla_records.rs";
    let Ok(here) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let mut names = std::collections::BTreeSet::new();
    for dir in ["../horizon-compat/src", "../horizon-compat-core/src", "src"] {
        let dir = Path::new(&here).join(dir);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut todo = vec![dir];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    todo.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with(EVERY_CLASS) {
                    let text = std::fs::read_to_string(&p).unwrap_or_default();
                    if p.ends_with(ROWS) {
                        rows_in(&text, &mut names);
                    } else {
                        names_in(&text, &mut names);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    for (list, package) in [("NAMED_CLASSES", false), ("NAMED_PACKAGES", true)] {
        out.push_str(&format!("pub const {list}: &[&str] = &[\n"));
        for n in names.iter().filter(|n| n.ends_with('/') == package) {
            out.push_str(&format!("    {n:?},\n"));
        }
        out.push_str("];\n");
    }
    let to = Path::new(&std::env::var("OUT_DIR").unwrap_or_default()).join("named_classes.rs");
    std::fs::write(to, out).expect("OUT_DIR is writable");
}

/// Each `net/minecraft/...`, `net/neoforged/...` or `com/mojang/...` class or package (`.../`)
/// name in `text`, up to what can't be part of one (a member's `.`, a quote).
fn names_in(text: &str, names: &mut std::collections::BTreeSet<String>) {
    for root in ["net/minecraft/", "net/neoforged/", "com/mojang/"] {
        let mut from = 0;
        while let Some(at) = text[from..].find(root) {
            let start = from + at;
            let len = text[start..]
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'$'))
                .count();
            let name = &text[start..start + len];
            // A parameter's type in a method's descriptor (`(L...;L...;)`) names what a mapped
            // method is given, which says nothing of who models it; its return type does (the
            // runtime makes that object).
            let parameter = text[..start]
                .strip_suffix('L')
                .map(|b| b.trim_end_matches('['))
                .is_some_and(|b| b.ends_with('(') || b.ends_with(';'));
            if !(parameter && text[start + len..].starts_with(';')) {
                names.insert(name.to_owned());
            }
            from = start + len.max(1);
        }
    }
}

/// Each class name that is a whole text in `text` (`"net/minecraft/core/GlobalPos"`): what a
/// table's rows are about, the names inside their other texts left out.
fn rows_in(text: &str, names: &mut std::collections::BTreeSet<String>) {
    for root in ["\"net/minecraft/", "\"net/neoforged/", "\"com/mojang/"] {
        for (at, _) in text.match_indices(root) {
            let start = at + 1;
            let len = text[start..]
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'$'))
                .count();
            if text[start + len..].starts_with('"') {
                names.insert(text[start..start + len].to_owned());
            }
        }
    }
}

fn is_revision(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn git(args: &[&str]) -> Option<String> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The checkout's `HEAD`, rebuilt when it moves: the build script reruns when `HEAD`, the branch it
/// names or the packed refs change (only the ones that exist: a missing one would rerun it always).
fn head() -> Option<String> {
    let revision = git(&["rev-parse", "HEAD"]).filter(|r| is_revision(r))?;
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let dir = Path::new(&dir);
        let mut watch = vec![dir.join("HEAD"), dir.join("packed-refs")];
        if let Ok(text) = std::fs::read_to_string(dir.join("HEAD"))
            && let Some(name) = text.trim().strip_prefix("ref: ")
        {
            watch.push(dir.join(name));
        }
        for path in watch.iter().filter(|p| p.is_file()) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Some(revision)
}
