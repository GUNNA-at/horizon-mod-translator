//! `horizon-mod-translator coverage <jar>...`: the game and loader API each
//! jar calls, most used first, and how much of it Horizon's compat layer maps.
use std::{path::Path, process::ExitCode};

/// `translate <jar> <out dir>`: the jar's code as a Rust crate in `<out dir>`,
/// built against horizon-compat.
fn translate(jar: &Path, out: &Path) -> Result<(), String> {
    let classes = horizon_mod_translator::classes(jar)?;
    let mut program = horizon_mod_translator::codegen::Program::new(&classes);
    let files = program.emit_files(40);
    let lines: usize = files.iter().map(|(_, f)| f.lines().count()).sum();
    let src = out.join("src");
    std::fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-compat");
    let compat = compat.canonicalize().map_err(|e| e.to_string())?;
    let name = jar.file_stem().unwrap_or_default().to_string_lossy().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    std::fs::write(
        out.join("Cargo.toml"),
        format!("[package]\nname = \"translated_{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {compat:?} }}\n"),
    )
    .map_err(|e| e.to_string())?;
    for (name, text) in &files {
        std::fs::write(src.join(name), text).map_err(|e| e.to_string())?;
    }
    println!("{}: {} classes, {lines} lines of Rust in {} files", jar.display(), classes.len(), files.len());
    for (what, n) in &program.unsupported {
        println!("  unsupported: {what} ({n})");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("translate") && args.len() == 3 {
        return match translate(Path::new(&args[1]), Path::new(&args[2])) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.first().map(String::as_str) != Some("coverage") || args.len() < 2 {
        eprintln!("usage: horizon-mod-translator coverage <jar>... | translate <jar> <out dir>");
        return ExitCode::FAILURE;
    }
    for jar in &args[1..] {
        let classes = match horizon_mod_translator::classes(Path::new(jar)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        let calls = horizon_mod_translator::api_calls(&classes);
        let methods: usize = classes.iter().map(|c| c.methods.len()).sum();
        println!("{jar}: {} classes, {methods} methods, {} distinct API methods called", classes.len(), calls.len());
        let mut top: Vec<_> = calls.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        for (m, n) in top.iter().take(15) {
            println!("  {n:5}  {}.{}{}", m.owner, m.name, m.desc);
        }
        let regs = horizon_mod_translator::registrations(&classes);
        let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
        for r in &regs {
            *by.entry(r.registry.as_str()).or_default() += 1;
        }
        println!("  registrations in code: {} {by:?}", regs.len());
        for r in regs.iter().take(5) {
            println!("    {} {}", r.registry, r.name);
        }
    }
    ExitCode::SUCCESS
}
