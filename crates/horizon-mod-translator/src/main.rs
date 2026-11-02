//! `horizon-mod-translator coverage <jar>...`: the game and loader API each
//! jar calls, most used first, and how much of it Horizon's compat layer maps.
use std::{path::Path, process::ExitCode};

/// `translate <jar> <out dir>`: the jar's code as a Rust crate in `<out dir>`,
/// built against horizon-compat.
fn translate(jar: &Path, out: &Path) -> Result<(), String> {
    let classes = horizon_mod_translator::classes(jar)?;
    let mut program = horizon_mod_translator::codegen::Program::new(&classes);
    program.services = horizon_mod_translator::services(jar)?;
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
    // `bundle <jar> <out> [--runtime <horizon-compat-core dir>]`: with `--runtime`, a bundle for
    // tools whose engine links the runtime core instead of carrying it (see `bundle::write_with`).
    let runtime = match (args.get(3).map(String::as_str), args.get(4)) {
        (Some("--runtime"), Some(dir)) => Some(Path::new(dir).to_path_buf()),
        _ => None,
    };
    if args.first().map(String::as_str) == Some("bundle") && (args.len() == 3 || (args.len() == 5 && runtime.is_some())) {
        let jar = Path::new(&args[1]);
        let result = horizon_mod_translator::classes(jar)
            .and_then(|c| horizon_mod_translator::bundle::write_with(jar, &c, Path::new(&args[2]), runtime.as_deref()));
        return match result {
            Ok((id, left)) => {
                println!("{id}: native mod bundle in {}", args[2]);
                for l in left {
                    println!("  not translated: {l}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
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
        eprintln!("usage: horizon-mod-translator coverage <jar>... | translate <jar> <out dir> | bundle <jar> <out dir> [--runtime <horizon-compat-core dir>]");
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
        // Which of them the runtime maps: call each once and see whether it counts as unmapped.
        let (mut mapped, mut sites, mut mapped_sites) = (0usize, 0usize, 0usize);
        let mut missing: Vec<(usize, String)> = Vec::new();
        for (m, n) in &calls {
            let key: &'static str = Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
            let mut rt = horizon_compat::Rt::new();
            let args = vec![horizon_compat::V::NULL; horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + 1];
            horizon_compat::api::call(&mut rt, key, &args);
            sites += n;
            if rt.unmapped.is_empty() {
                mapped += 1;
                mapped_sites += n;
            } else {
                missing.push((*n, key.to_owned()));
            }
        }
        missing.sort_by(|a, b| b.0.cmp(&a.0));
        println!(
            "  mapped: {mapped} of {} API methods ({:.1}%), {mapped_sites} of {sites} class uses ({:.1}%)",
            calls.len(),
            100.0 * mapped as f64 / calls.len().max(1) as f64,
            100.0 * mapped_sites as f64 / sites.max(1) as f64
        );
        if std::env::var_os("MISSING").is_some() {
            for (n, k) in missing.iter().take(60) {
                println!("  missing {n:5} {k}");
            }
        }
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
