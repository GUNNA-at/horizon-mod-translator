//! Mixins woven by the translator: `tests/fixtures/weave` has a class (`Machine`), an addon's mixin
//! into it (`MachineMixin`: `@Inject` at the head, at a return and after a call, `@ModifyArg`,
//! `@WrapWithCondition`, `@ModifyExpressionValue`, `@ModifyReturnValue`, `@Redirect`,
//! `@WrapOperation`, `@WrapMethod`, an `@Overwrite`, `@Shadow`s, field initialisers and an
//! interface it adds) and a class calling the
//! target as other code would (`Probe`). They are compiled against stubs of Mixin's annotations,
//! translated together and run; each answer is the one Mixin's weaving gives, worked out by hand
//! from what the handlers do. Needs `javac`.
use std::{fs, path::Path, process::Command};

/// (Probe method, descriptor, argument, what Mixin's weaving makes it return).
const CASES: &[(&str, &str, i32, &str)] = &[
    // `tick` cancels at the head from the fifth call on.
    ("ticks", "(I)I", 3, "3"),
    ("ticks", "(I)I", 6, "4"),
    // Each tick adds 1 × the mixin's `boost` (3, its field initialiser).
    ("total", "(I)I", 2, "6"),
    ("total", "(I)I", 6, "12"),
    // `log` runs on even ticks only.
    ("logged", "(I)I", 1, "0"),
    ("logged", "(I)I", 6, "2"),
    // After each `add`.
    ("afterAdds", "(I)I", 6, "4"),
    ("boost", "()I", 0, "3"),
    // The mixin's static initialiser ran with its target's.
    ("seen", "()I", 0, "7"),
    // A load of 0 returns the handler's -1; others their own value.
    ("speed", "(I)I", 0, "-1"),
    ("speed", "(I)I", 3, "7"),
    // `base()` + 5, doubled.
    ("power", "(I)I", 0, "10"),
    ("power", "(I)I", 2, "22"),
    // The overwrite: 50 + `base()` (its `@Shadow`).
    ("weight", "(I)I", 2, "56"),
    // A static handler in a static method.
    // The wrapped call made with 2 + 1, then 1000 added.
    ("scale", "(I)I", 2, "1012"),
    // The wrapped method: (5 + 1) × 3, doubled; a negative never runs it.
    ("heavy", "(I)I", 5, "36"),
    ("heavy", "(I)I", -1, "0"),
    ("twice", "(I)I", -3, "0"),
    ("twice", "(I)I", 4, "8"),
    ("name", "()Ljava/lang/String;", 0, "m1!"),
    // The redirected `name()` reads the target's static through its `@Shadow`.
    ("describe", "(I)Ljava/lang/String;", 2, "mixed1:6"),
];

#[test]
fn a_mixin_into_a_translated_class_is_woven_as_mixin_weaves_it() {
    if !Command::new("javac").arg("-version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("no JDK: skipped");
        return;
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = std::env::temp_dir().join(format!("mod-translator-weave-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let classes = root.join("classes");
    fs::create_dir_all(&classes).unwrap();
    let mut javac = Command::new("javac");
    javac.args(["-d", classes.to_str().unwrap()]);
    let mut stack = vec![here.join("tests/fixtures/weave")];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(d).unwrap().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                javac.arg(e.path());
            }
        }
    }
    assert!(javac.status().unwrap().success());
    // The stubs stand for Mixin; only the mods' classes are translated.
    let parsed: Vec<_> = fs::read_dir(classes.join("weave"))
        .unwrap()
        .flatten()
        .map(|e| horizon_mod_translator::class::ClassFile::parse(&fs::read(e.path()).unwrap()).unwrap())
        .collect();
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    let module = program.emit();
    assert!(program.unsupported.is_empty(), "{:?}", program.unsupported);

    let krate = root.join("gen");
    fs::create_dir_all(krate.join("src")).unwrap();
    fs::write(
        krate.join("Cargo.toml"),
        format!(
            "[package]\nname = \"gen\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {:?} }}\n",
            here.join("../horizon-compat").canonicalize().unwrap()
        ),
    )
    .unwrap();
    let mut main = String::from("mod woven;\nuse woven::*;\nuse horizon_compat::{Rt, V};\nfn main() {\n    horizon_compat::install();\n    let mut rt = Rt::new();\n    init(&mut rt);\n");
    for (name, desc, arg, _) in CASES {
        let f = horizon_mod_translator::codegen::fn_name("weave/Probe", name, desc);
        let args = if desc.starts_with("()") { String::new() } else { format!("V::I({arg})") };
        main.push_str(&format!(
            "    let r = {f}(&mut rt, &[{args}]);\n    if let Some(e) = rt.thrown.take() {{ println!(\"{name}({arg}) threw {{}}\", rt.class_of(e)); }} else {{ println!(\"{name}({arg}) = {{}}\", rt.str_of(r).map(str::to_owned).unwrap_or_else(|| r.i().to_string())); }}\n"
        ));
    }
    main.push_str("}\n");
    fs::write(krate.join("src/main.rs"), main).unwrap();
    fs::write(krate.join("src/woven.rs"), &module).unwrap();
    let out = Command::new(env!("CARGO"))
        .args(["run", "-q", "--manifest-path"])
        .arg(krate.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}\n{stdout}", String::from_utf8_lossy(&out.stderr));
    eprintln!("{stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), CASES.len());
    for ((name, _, arg, want), line) in CASES.iter().zip(&lines) {
        assert_eq!(*line, format!("{name}({arg}) = {want}"));
    }
    fs::remove_dir_all(&root).unwrap();
}
