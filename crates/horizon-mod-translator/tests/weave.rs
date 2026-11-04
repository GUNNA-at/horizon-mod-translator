//! Mixins woven by the translator: `tests/fixtures/weave` has a class (`Machine`), an addon's mixin
//! into it (`MachineMixin`: `@Inject` at the head, at a return and after a call, `@ModifyArg`,
//! `@WrapWithCondition`, `@ModifyExpressionValue`, `@ModifyReturnValue`, `@Redirect`,
//! `@WrapOperation`, `@WrapMethod`, an `@Overwrite`, `@Shadow`s, field initialisers and an
//! interface it adds), handlers taking the target's locals (`GaugeMixin`: MixinExtras' `@Local`
//! by type, ordinal and name, its refs, `@Share`; Mixin's captured locals at one return;
//! `INVOKE_ASSIGN`), weaves at field reads and writes (`@At("FIELD")`), `@ModifyVariable` (at the
//! head, after a store, before a call), `@ModifyReceiver`, an enum constant added through a constructor invoker (`ModeMixin`),
//! handlers at constants (`@ModifyConstant`, `@At("CONSTANT")`), before `new`s, round an
//! `instanceof`, and in `@Slice`s of their methods (`DialMixin`), a mixin into the game's `Vec3`
//! adding an interface, methods and fields to its objects (`Vec3Mixin`), and a class calling the
//! targets as other code would (`Probe`). They are compiled against stubs of Mixin's annotations,
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
    // Once per call: the wrapper is not also an injection into the method's head.
    ("heavyWraps", "(I)I", 5, "1"),
    ("twice", "(I)I", -3, "0"),
    ("twice", "(I)I", 4, "8"),
    ("name", "()Ljava/lang/String;", 0, "m1!"),
    // The redirected `name()` reads the target's static through its `@Shadow`.
    ("describe", "(I)Ljava/lang/String;", 2, "mixed1:6"),
    // `reading(2, 3)`: sum 5, scaled 50 (then 57 through its ref), tag "g50"; returns 57 + 1.
    ("reading", "(I)I", 2, "58"),
    // sum × 100 + the second argument (`arg1`).
    ("readingSeen", "(I)I", 2, "503"),
    ("readingNote", "(I)Ljava/lang/String;", 2, "g50"),
    // The third return only: 5 doubled + 1000; the first two untouched.
    ("pick", "(I)I", 5, "1010"),
    ("pick", "(I)I", -1, "-1"),
    ("pick", "(I)I", 0, "0"),
    // `v = twice(3)` is 6, ten times that after the store, + 1.
    ("stored", "(I)I", 3, "61"),
    // 4 × 3 + 1 on each call: the shared count starts again at 0.
    ("sharedTwice", "(I)I", 4, "13"),
    // The local changed after `record` was given "n4".
    ("label", "(I)Ljava/lang/String;", 4, "n4?"),
    ("labelLast", "(I)I", 4, "2"),
    // The enum's constant made by the mixin's static initialiser, with its constructor.
    ("modes", "()I", 0, "3"),
    ("turbo", "()I", 0, "9"),
    ("turboName", "()Ljava/lang/String;", 0, "TURBO"),
    // Field accesses: (4 + 1) × 2; 2 stored as 2 × 3; 100 + 5; after `level` became 5.
    ("doubledLevel", "()I", 0, "10"),
    ("setLevel", "(I)I", 2, "6"),
    ("limit", "()I", 0, "105"),
    ("bumpSeen", "()I", 0, "50"),
    // `@ModifyVariable`: (3 + 100) × 2; (2 × 3) × 10 + 1, the second store untouched; (2 + 1) × 4.
    ("offset", "(I)I", 3, "206"),
    ("accumulate", "(I)I", 2, "61"),
    ("spread", "(I)I", 2, "12"),
    // `@ModifyReceiver`: the length of "abc!!".
    ("nameLength", "()I", 0, "5"),
    // Slices: the second `step` doubled (4 × 100 + 8), an injection before the first only.
    ("sliced", "(I)I", 3, "408"),
    ("slicedSeen", "(I)I", 3, "1"),
    // Constants: 3 × 8; 2 × (2.5 + 2), × 10; "dial!"; 10 + (10 + 5).
    ("sevens", "(I)I", 3, "24"),
    ("half", "(I)I", 2, "90"),
    ("dialName", "()Ljava/lang/String;", 0, "dial!"),
    ("tens", "()I", 0, "25"),
    // Before a `new`: 1 + 40; `count` tripled.
    ("made", "(I)I", 5, "41"),
    ("counted", "(I)I", 4, "12"),
    // An `instanceof` wrapped: a `Dial` counts as text, a `String` still does, a `Gauge` not.
    ("isText", "(I)I", 1, "1"),
    ("isText", "(I)I", 0, "1"),
    ("isText", "(I)I", -1, "0"),
    // The method's first argument after the value: (2 + 1) + 2000, + 5.
    ("first", "(I)I", 2, "2008"),
    // `shift = BY 2`: after the store (5 + 50 + 3), after a cast and store (3 + 30 + 1), and on a
    // jump's label: run as the code falls through (3 + 100), not as the `if` jumps there.
    ("shiftedTwo", "(I)I", 4, "58"),
    ("assigned", "(I)I", 3, "34"),
    ("branched", "(I)I", 2, "103"),
    ("branched", "(I)I", -1, "0"),
    // A mixin into the game's `Vec3`: a field initialised once per object (10, then counted up);
    // `lengthSqr()` (its `@Shadow`, the game's) + `x` (a `@Shadow` field) + 0.5, doubled; each
    // object's own label, with the mixin's static (40); the interface it adds.
    ("vecReads", "(I)I", 3, "13"),
    ("vecSize", "(I)I", 3, "25"),
    ("vecLabels", "(I)Ljava/lang/String;", 4, "a440/b40"),
    ("vecMeasured", "(I)I", 1, "1"),
    ("vecMeasured", "(I)I", 0, "0"),
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
    // With the local variable tables, as Gradle compiles mods (`-g`): handlers find locals by them.
    let mut javac = Command::new("javac");
    javac.args(["-g", "-d", classes.to_str().unwrap()]);
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
    // Mixin applies the mixins a configuration lists, and only those.
    let all: Vec<&horizon_mod_translator::class::ClassFile> = parsed.iter().collect();
    let config = |classes: &[&str]| horizon_mod_translator::MixinConfig {
        file: "weave.mixins.json".into(),
        classes: classes.iter().map(|c| c.to_string()).collect(),
        default_require: 1,
        ..Default::default()
    };
    let every = horizon_mod_translator::weave::plan_with(&all, &[config(&["weave/MachineMixin", "weave/GaugeMixin", "weave/ModeMixin", "weave/DialMixin"])]);
    assert_eq!(every.weaves, horizon_mod_translator::weave::plan(&parsed).weaves);
    let one = horizon_mod_translator::weave::plan_with(&all, &[config(&["weave/MachineMixin"])]);
    assert!(!one.weaves.is_empty() && one.weaves.keys().all(|k| k.starts_with("weave/Machine.")));
    assert!(horizon_mod_translator::weave::plan_with(&all, &[config(&["weave/OtherMixin"])]).weaves.is_empty());

    // A config plugin of the addons' kind turns mixins off by the mods there are: an enum constant
    // names its mod by the string it is made with, else by its name.
    let load = |name: &str| fs::read(classes.join(format!("{name}.class"))).ok().and_then(|b| horizon_mod_translator::class::ClassFile::parse(&b).ok());
    let mods: std::collections::BTreeSet<String> = ["create", "extra_mod"].map(String::from).into();
    let off = |m: &str| horizon_mod_translator::plugin_turns_off(&load(m).unwrap(), &load, &mods);
    assert!(!off("weave/OnlyWithCreateMixin"));
    assert!(off("weave/OnlyWithGoneMixin"));
    assert!(!off("weave/WithoutGoneMixin"));
    assert!(!off("weave/OnlyWithExtraMixin"));
    assert!(!off("weave/GaugeMixin"));

    // One class a file, as a bundle splits a mod (`emit_files`): the woven code calls across them.
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    let files = program.emit_files(1);
    assert!(program.unsupported.is_empty(), "{:?}", program.unsupported);

    let krate = root.join("woven");
    fs::create_dir_all(krate.join("src")).unwrap();
    fs::write(
        krate.join("Cargo.toml"),
        format!(
            "[package]\nname = \"woven\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {:?} }}\n",
            here.join("../horizon-compat").canonicalize().unwrap()
        ),
    )
    .unwrap();
    for (name, text) in &files {
        fs::write(krate.join("src").join(name), text).unwrap();
    }
    let mut main = String::from("use woven::*;\nuse horizon_compat::{Rt, V};\nfn main() {\n    horizon_compat::install();\n    let mut rt = Rt::new();\n    init(&mut rt);\n");
    for (name, desc, arg, _) in CASES {
        let f = horizon_mod_translator::codegen::fn_name("weave/Probe", name, desc);
        let args = if desc.starts_with("()") { String::new() } else { format!("V::I({arg})") };
        main.push_str(&format!(
            "    let r = {f}(&mut rt, &[{args}]);\n    if let Some(e) = rt.thrown.take() {{ println!(\"{name}({arg}) threw {{}}\", rt.class_of(e)); }} else {{ println!(\"{name}({arg}) = {{}}\", rt.str_of(r).map(str::to_owned).unwrap_or_else(|| r.i().to_string())); }}\n"
        ));
    }
    main.push_str("}\n");
    fs::write(krate.join("src/main.rs"), main).unwrap();
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
