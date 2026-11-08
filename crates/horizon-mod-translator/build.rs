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
