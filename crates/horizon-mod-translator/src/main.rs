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
    // `bundle <jar> <out> [--with <addon jar>]... [--runtime <horizon-compat-core dir>]`: `--with`
    // translates mods built on the first with it, as one engine (see `bundle::write_group`);
    // `--runtime`, a bundle for tools whose engine links the runtime core instead of carrying it.
    if args.first().map(String::as_str) == Some("bundle") && args.len() >= 3 {
        let (mut with, mut runtime, mut rest) = (Vec::new(), None, args[3..].iter());
        while let Some(a) = rest.next() {
            match (a.as_str(), rest.next()) {
                ("--with", Some(j)) => with.push(Path::new(j)),
                ("--runtime", Some(dir)) => runtime = Some(Path::new(dir)),
                _ => {
                    eprintln!("bundle <jar> <out dir> [--with <addon jar>]... [--runtime <horizon-compat-core dir>]");
                    return ExitCode::FAILURE;
                }
            }
        }
        let jars: Vec<&Path> = std::iter::once(Path::new(&args[1])).chain(with).collect();
        let result = horizon_mod_translator::classes_of(&jars)
            .and_then(|c| horizon_mod_translator::bundle::write_group(&jars, &c, Path::new(&args[2]), runtime));
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
    // `mixins <jar>...`: what each jar's mixins do to the game (the injections the translator
    // doesn't apply), one line each, then the totals by kind.
    if args.first().map(String::as_str) == Some("mixins") && args.len() >= 2 {
        for jar in &args[1..] {
            let classes = match horizon_mod_translator::classes(Path::new(jar)) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{jar}: {e}");
                    continue;
                }
            };
            let all = horizon_mod_translator::mixin_injections(&classes);
            let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
            for i in &all {
                *by.entry(i.kind.as_str()).or_default() += 1;
                println!("  {} {} {} @ {} ({})", i.kind, i.target, i.method, i.at, i.mixin);
            }
            println!("{jar}: {} mixin members {by:?}", all.len());
        }
        return ExitCode::SUCCESS;
    }
    // What the translator weaves of the mixins among these jars' classes into each other (a mod
    // and its addons, as `bundle --with` translates them), and what it leaves.
    if args.first().map(String::as_str) == Some("weaves") && args.len() >= 2 {
        let jars: Vec<&Path> = args[1..].iter().map(Path::new).collect();
        let classes = match horizon_mod_translator::classes_of(&jars) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        let plan = horizon_mod_translator::weave::plan(&classes);
        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
        for (target, ws) in &plan.weaves {
            for w in ws {
                let kind = format!("{:?}", w.kind);
                let kind = kind.split([' ', '{']).next().unwrap_or("").to_owned();
                let at = format!("{:?}", w.at);
                let at = at.split([' ', '{']).next().unwrap_or("").to_owned();
                *by.entry(format!("{kind} at {at}")).or_default() += 1;
                println!("  {target} <- {}.{} ({kind} at {at})", w.mixin, w.handler);
            }
        }
        let mixins: usize = plan.mixins.values().map(Vec::len).sum();
        let woven: usize = plan.weaves.values().map(Vec::len).sum();
        let left: usize = plan.unsupported.values().sum();
        println!("{} targets, {mixins} mixins, {woven} handlers woven, {left} left", plan.mixins.len());
        for (k, n) in &by {
            println!("  woven {n:5} {k}");
        }
        for (k, n) in &plan.unsupported {
            println!("  left  {n:5} {k}");
        }
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) != Some("coverage") || args.len() < 2 {
        eprintln!("usage: horizon-mod-translator coverage <jar>... | mixins <jar>... | weaves <mod jar> <addon jar>... | translate <jar> <out dir> | bundle <jar> <out dir> [--with <addon jar>]... [--runtime <horizon-compat-core dir>]");
        return ExitCode::FAILURE;
    }
    // The mapping answers the core's questions (`Rt::is_a` of Minecraft's class families): without
    // it, a handler that checks its receiver's class finds no widget, block or entity.
    horizon_compat::install();
    for jar in &args[1..] {
        let classes = match horizon_mod_translator::classes(Path::new(jar)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        let calls = horizon_mod_translator::api_calls(&classes);
        // Which of them the runtime maps: call each and see whether it counts as unmapped. Null
        // arguments first; then, as a handler may answer only for an object of its class, with an
        // object of the owner's class as the receiver (or first argument of a static call).
        let (mut mapped, mut sites, mut mapped_sites) = (0usize, 0usize, 0usize);
        let mut missing: Vec<(usize, String)> = Vec::new();
        let answers = |key: &'static str, owner: &'static str, n: usize, receiver: bool| {
            let mut rt = horizon_compat::Rt::new();
            let mut args = vec![horizon_compat::V::NULL; n];
            if receiver {
                args[0] = rt.new_object(owner);
            }
            horizon_compat::api::call(&mut rt, key, &args);
            rt.unmapped.is_empty()
        };
        for (m, n) in &calls {
            let key: &'static str = Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
            let arity = horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + 1;
            sites += n;
            let owner: &'static str = Box::leak(m.owner.clone().into_boxed_str());
            if answers(key, owner, arity, false) || answers(key, owner, arity, true) {
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
        // MISSING=1 lists the 60 most called gaps; MISSING=all every one.
        if let Some(m) = std::env::var_os("MISSING") {
            let limit = if m == "all" { usize::MAX } else { 60 };
            for (n, k) in missing.iter().take(limit) {
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
