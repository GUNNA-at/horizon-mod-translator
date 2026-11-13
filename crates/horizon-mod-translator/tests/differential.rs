//! The translator against the JVM: `tests/fixtures/java/Fixture.java` runs on
//! `java`, then its classes are translated to Rust, built against
//! horizon-compat and run on the same inputs; results must match bit for bit.
//! Needs a JDK (`javac`, `java`); skipped without one.
use std::{fs, path::Path, process::Command};

/// (method, descriptor, inputs). Inputs are ints, or doubles for `(D)`.
const CASES: &[(&str, &str, &[&str])] = &[
    ("sumTo", "(I)I", &["0", "1", "100", "70000"]),
    ("fib", "(I)J", &["0", "1", "50", "90"]),
    ("collatz", "(I)I", &["1", "27", "97"]),
    ("sw", "(I)I", &["0", "1", "2", "3", "100", "-5", "-6"]),
    ("arrays", "(I)I", &["0", "1", "10", "33"]),
    ("shapes", "(I)J", &["0", "7", "100001"]),
    ("statics", "(I)I", &["5"]),
    ("floats", "(I)D", &["0", "10", "1000"]),
    ("bits", "(I)I", &["0", "-1", "123456789", "-2147483648"]),
    ("longBits", "(J)J", &["0", "-1", "123456789012"]),
    ("wideStores", "(J)J", &["0", "7", "-123456789012"]),
    ("charParts", "(I)I", &["0", "3", "30"]),
    ("threads", "(I)I", &["0", "4"]),
    ("monitors", "(I)I", &["0", "3", "-8"]),
    ("jdkStatics", "(I)J", &["0", "9", "20", "36"]),
    ("jdkColor", "(I)J", &["0", "77", "300"]),
    ("jdkMath", "(I)J", &["0", "5", "12345"]),
    ("jdkText", "(I)J", &["0", "9", "25"]),
    ("overflow", "(I)I", &["0", "3000", "-77777"]),
    ("casts", "(D)I", &["0", "1e10", "-3.75", "65601.5", "NaN"]),
    ("nested", "(I)I", &["0", "5", "40"]),
    ("lambdas", "(I)I", &["0", "5", "-9"]),
    ("defaults", "(I)I", &["0", "3", "-7"]),
    ("overloads", "(I)I", &["0", "4", "-6"]),
    ("chained", "(I)I", &["0", "5", "-3"]),
    ("caller", "(I)I", &["0", "1"]),
    ("inherited", "(I)I", &["0", "2"]),
    ("enums", "(I)I", &["0", "2"]),
    ("interfaces", "(I)I", &["0", "1"]),
    ("concat", "(I)I", &["0", "7", "123", "-4"]),
    ("exceptions", "(I)I", &["0", "1", "2", "3", "4"]),
    ("jvmThrows", "(I)I", &["0", "1", "2", "3", "4", "5", "6", "7"]),
    ("records", "(I)I", &["0", "5", "-12"]),
    ("patterns", "(I)I", &["0", "1", "2"]),
    ("grid", "(I)I", &["0", "1", "6"]),
    ("reflection", "(I)I", &["3", "-12"]),
    ("staticFields", "(I)I", &["0", "5"]),
    ("staticRefs", "(I)I", &["3"]),
    ("library", "(I)I", &["0", "3", "-7"]),
    ("rawBoxes", "(I)I", &["0", "2"]),
    ("adapted", "(I)I", &["0", "3", "-5"]),
    (
        "ownCollections",
        "(I)I",
        &[
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
            "17", "18", "0",
        ],
    ),
    (
        "subCollections",
        "(I)I",
        &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "0"],
    ),
    ("staticReflection", "(I)I", &["0", "6", "-4"]),
    (
        "atomics",
        "(I)J",
        &[
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
            "17", "18", "0",
        ],
    ),
    ("reflectionMore", "(I)I", &["0", "3"]),
    ("blockingQueues", "(I)I", &["0", "9"]),
    (
        "arraysMore",
        "(I)I",
        &["0", "1", "2", "3", "4", "5", "6", "7", "8"],
    ),
    ("pools", "(I)I", &["0", "1", "2", "3", "4", "5", "6"]),
    (
        "packedStrings",
        "(I)I",
        &["0", "1", "2", "3", "4", "5", "6"],
    ),
    (
        "hidden",
        "(I)I",
        &["1", "2", "3", "4", "5", "6", "7", "8", "0"],
    ),
    ("keyed", "(I)I", &["1", "2", "3", "4", "5", "6", "0"]),
    ("files", "(I)I", &["1", "2", "3", "4", "5"]),
    ("formats", "(I)I", &["1", "2", "3", "4", "5", "6", "0"]),
];

fn has(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok_and(|o| o.status.success())
}

fn java_value(desc: &str, input: &str) -> String {
    match &desc[1..2] {
        "D" => format!("Double.parseDouble(\"{input}\")"),
        "J" => format!("{input}L"),
        _ => input.to_owned(),
    }
}

fn rust_value(desc: &str, input: &str) -> String {
    match &desc[1..2] {
        "D" => format!("V::D(\"{input}\".parse::<f64>().unwrap())"),
        "J" => format!("V::J({input}i64)"),
        _ => format!("V::I({input})"),
    }
}

#[test]
fn translated_fixture_matches_the_jvm() {
    if !has("javac") || !has("java") {
        eprintln!("no JDK: skipped");
        return;
    }
    // The game's Java runtime, whose own code the fixture's `jdk` cases run.
    let image = horizon_mod_translator::find_java_image();
    if image.is_none() {
        eprintln!(
            "no Java {} runtime: skipped",
            horizon_mod_translator::JAVA_VERSION
        );
        return;
    }
    let root = std::env::temp_dir().join(format!("mod-translator-diff-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let classes = root.join("classes");
    fs::create_dir_all(&classes).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java/Fixture.java");

    // The JVM's answers.
    let mut runner = String::from("public class Runner { public static void main(String[] a) {\n");
    for (name, desc, inputs) in CASES {
        for input in *inputs {
            let call = format!("Fixture.{name}({})", java_value(desc, input));
            let print = if desc.ends_with('D') {
                format!("Double.doubleToRawLongBits({call})")
            } else {
                call
            };
            runner.push_str(&format!("System.out.println(\"{name}({input}) = \" + {print});\n"));
        }
    }
    runner.push_str("}}\n");
    fs::write(root.join("Runner.java"), runner).unwrap();
    let mut javac = Command::new("javac");
    javac.args(["-d", classes.to_str().unwrap()]).arg(&fixture).arg(root.join("Runner.java"));
    for e in fs::read_dir(fixture.with_file_name("lib")).unwrap().flatten() {
        javac.arg(e.path());
    }
    assert!(javac.status().unwrap().success());
    // A service the fixture loads, where `java -cp` finds it and in the mod's jar.
    let service = "# providers\nFixture$Impl\n";
    fs::create_dir_all(classes.join("META-INF/services")).unwrap();
    fs::write(classes.join("META-INF/services/Fixture$Spins"), service).unwrap();
    let java = Command::new("java").args(["-cp", classes.to_str().unwrap(), "Runner"]).output().unwrap();
    assert!(java.status.success(), "{}", String::from_utf8_lossy(&java.stderr));
    let expected = String::from_utf8(java.stdout).unwrap();

    // The mod's jar: its classes, a service it loads, and the library inside (jar-in-jar).
    let packed = root.join("packed");
    fs::create_dir_all(packed.join("META-INF/services")).unwrap();
    fs::create_dir_all(packed.join("META-INF/jarjar")).unwrap();
    for e in fs::read_dir(&classes).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("Fixture") {
            fs::copy(e.path(), packed.join(e.file_name())).unwrap();
        }
    }
    fs::write(packed.join("META-INF/services/Fixture$Spins"), service).unwrap();
    let jar_of = |jar: &Path, dir: &Path, what: &str| {
        assert!(Command::new("jar").arg("cf").arg(jar).arg("-C").arg(dir).arg(what).status().unwrap().success());
    };
    jar_of(&packed.join("META-INF/jarjar/lib.jar"), &classes, "lib");
    let jar = root.join("fixture.jar");
    jar_of(&jar, &packed, ".");

    // Translate what the mod can run: the library as far as it reaches.
    // With the Java runtime's own code (`jdk_code`), as a bundle takes it.
    let image = image.unwrap();
    let mut parsed = horizon_mod_translator::program_with(&[&jar], &[], &[&image])
        .unwrap()
        .0;
    assert!(
        parsed.iter().any(|c| c.name == "java/awt/Color"),
        "the JDK's classes are part of the program"
    );
    let methods = |class: &str| -> Option<Vec<String>> {
        let c = parsed.iter().find(|c| c.name == class)?;
        Some(c.methods.iter().filter(|m| m.code.is_some()).map(|m| m.name.clone()).collect())
    };
    assert_eq!(methods("lib/Unused"), None, "a class only unused code makes is left out");
    assert!(!methods("lib/Lib").unwrap().contains(&"unused".to_owned()));
    assert!(!methods("lib/Base").unwrap().contains(&"unusedHello".to_owned()));
    assert!(methods("lib/Circle").unwrap().iter().all(|m| m == "<init>"), "never made, so none of its methods run");
    assert!(methods("lib/Counter").unwrap().contains(&"peek".to_owned()), "handed out as a JDK interface: all of it stays");
    assert_eq!(methods("lib/Callback").unwrap(), ["accept"], "a lambda is of its interface: the default method called on it runs");
    // And the JDK's static methods the runtime has no answer for, where their own code can stand
    // (`derive`), carried as a bundle carries them.
    let carried = {
        use horizon_mod_translator::derive::{Deriver, Game, Mapped, is_jdk};
        let game = Game::open(&[&image]).unwrap();
        let own: std::collections::HashSet<&str> = parsed.iter().map(|c| c.name.as_str()).collect();
        let refs: Vec<_> = parsed.iter().collect();
        let calls = horizon_mod_translator::code_calls(&refs, &|c| is_jdk(c) && !own.contains(c));
        let asks = |m: &horizon_mod_translator::class::Member, _: bool| {
            let key: &'static str =
                Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
            let mut rt = horizon_compat::Rt::new();
            horizon_compat::api::call(&mut rt, key, &[horizon_compat::V::NULL; 6]);
            rt.unmapped.is_empty()
        };
        let mapped = Mapped {
            method: &asks,
            field: &|_, _, _| false,
            constant: &|_| false,
            translated: &|c| own.contains(c),
        };
        let mut deriver = Deriver::new(&game, mapped);
        let mut stood = Vec::new();
        for (m, (_, is_static)) in &calls {
            if *is_static
                && !asks(m, true)
                && deriver
                    .blockers(&m.owner, &m.name, &m.desc, false)
                    .is_empty()
            {
                stood.push(format!("{}.{}", m.owner, m.name));
            }
        }
        for want in [
            "java/util/Arrays.binarySearch",
            "java/util/Arrays.toString",
            "java/lang/Long.rotateLeft",
        ] {
            assert!(
                stood.iter().any(|s| s == want),
                "{want} does not stand: {stood:?}"
            );
        }
        horizon_mod_translator::codegen::set_accessors(deriver.accessors.clone());
        deriver.classes()
    };
    parsed.extend(carried);
    // The bundle's tables: a file of the jar's and the classes' members, each behind its class.
    for table in [horizon_compat::FILES, horizon_compat::MEMBERS] {
        parsed.push(horizon_mod_translator::codegen::table_class(table));
    }
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    program.services = horizon_mod_translator::services(&jar).unwrap();
    // Small enough that the fixture's longer blocks are cut into functions, and its short ones not.
    program.jar_files = vec![(
        "defaults/fixture.json".to_owned(),
        b"{\"speed\": 7}\n\x00\xff".to_vec(),
    )];
    program.long_block = (600, 200);
    // And a method of any length counts as a big one, cut into functions of a few blocks each.
    program.big_method = (60, 900);
    let module = program.emit();
    assert!(
        module.contains("#[inline(never)]\nfn j_"),
        "no block of the fixture was cut"
    );
    assert!(
        module.contains("__r1(rt: &mut Rt"),
        "no method of the fixture was cut into ranges"
    );

    // The generated crate.
    let krate = root.join("gen");
    fs::create_dir_all(krate.join("src")).unwrap();
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-compat");
    fs::write(
        krate.join("Cargo.toml"),
        format!(
            "[package]\nname = \"gen\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {:?} }}\n",
            compat.canonicalize().unwrap()
        ),
    )
    .unwrap();
    let mut main = String::from("mod fixture;\nuse fixture::*;\nuse horizon_compat::{Rt, V};\nfn main() {\n    horizon_compat::install();\n    let mut rt = Rt::new();\n    init(&mut rt);\n");
    // Static initialiser first, as the JVM runs it.
    if parsed.iter().any(|c| c.name == "Fixture" && c.methods.iter().any(|m| m.name == "<clinit>")) {
        main.push_str(&format!("    {}(&mut rt, &[]);\n", horizon_mod_translator::codegen::fn_name("Fixture", "<clinit>", "()V")));
    }
    for (name, desc, inputs) in CASES {
        let f = horizon_mod_translator::codegen::fn_name("Fixture", name, desc);
        for input in *inputs {
            let fmt = match desc.chars().last().unwrap() {
                'D' => "r.d().to_bits() as i64",
                'J' => "r.j()",
                _ => "r.i()",
            };
            // Each as the runtime calls into translated code: an entry, under which the
            // collector may run (`HORIZON_COMPAT_GC_MIN` makes it, every few objects).
            main.push_str(&format!(
                "    let r = rt.entered(|rt| {f}(rt, &[{}]), |r| *r); println!(\"{name}({input}) = {{}}\", {fmt});\n",
                rust_value(desc, input)
            ));
            // What it left thrown, for whoever reads a mismatch.
            main.push_str(&format!(
                "    if let Some(t) = rt.thrown.take() {{ eprintln!(\"{name}({input}) threw {{}}\", rt.class_of(t)); }}\n"
            ));
        }
    }
    // The tables, read as the runtime reads them.
    main.push_str("    let file = rt.reflect(\"hz/files\", \"defaults/fixture.json\", &[]).map(|a| (rt.array_len(a), rt.array_get(a, 14)));\n    let members = rt.reflect(\"hz/members\", \"Fixture\", &[]).and_then(|m| rt.str_of(m).map(|m| m.lines().any(|l| l == \"9 sumTo (I)I\")));\n    let absent = rt.reflect(\"hz/files\", \"nothing\", &[]).is_none();\n    eprintln!(\"tables {file:?} {members:?} {absent}\");\n");
    // And what the runtime had no answer for.
    main.push_str("    let mut unmapped: Vec<_> = rt.unmapped.keys().collect();\n    unmapped.sort();\n    for k in unmapped { eprintln!(\"unmapped {k}\"); }\n");
    // The collections that ran meanwhile: none, unless the environment asks for them often.
    main.push_str("    eprintln!(\"collections {:?}\", rt.young_runs);\n");
    main.push_str("}\n");
    fs::write(krate.join("src/main.rs"), main).unwrap();
    fs::write(krate.join("src/fixture.rs"), &module).unwrap();
    let out = Command::new(env!("CARGO"))
        .args(["run", "-q", "--release", "--manifest-path"])
        .arg(krate.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(out.status.success(), "generated crate failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let actual = String::from_utf8(out.stdout).unwrap();
    let notes = String::from_utf8_lossy(&out.stderr);
    assert!(
        notes.contains("tables Some((15, I(-1))) Some(true) true"),
        "{notes}"
    );
    // Run with `HORIZON_COMPAT_GC_MIN=<n>` (and `HORIZON_COMPAT_GC_CHECK`), the fixture is the
    // collector's test: it collects every n objects, while calls run and as they end, and the
    // answers below are still the JVM's.
    if std::env::var_os("HORIZON_COMPAT_GC_MIN").is_some() {
        let runs = notes
            .lines()
            .find_map(|l| l.strip_prefix("collections "))
            .unwrap_or("none told of")
            .to_owned();
        let counts: Vec<u64> = runs
            .trim_matches(['(', ')'])
            .split(", ")
            .filter_map(|n| n.parse().ok())
            .collect();
        assert!(
            matches!(counts[..], [in_calls, at_ends, freed, _] if in_calls > 0 && at_ends > 0 && freed > 0),
            "collections: {runs}"
        );
        assert!(!notes.contains("GC CHECK"), "{notes}");
        eprintln!(
            "collections (in calls, at their ends, objects freed, objects looked at): {runs}"
        );
    }
    let wrong: Vec<String> = expected
        .lines()
        .zip(actual.lines())
        .filter(|(e, a)| a != e)
        .map(|(e, a)| format!("{a} (the JVM: {e})"))
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}\n{notes}");
    assert_eq!(expected.lines().count(), CASES.iter().map(|c| c.2.len()).sum::<usize>(), "{expected}");
    assert_eq!(expected.lines().count(), actual.lines().count());
    eprintln!("{actual}");
    assert!(program.unsupported.is_empty(), "{:?}", program.unsupported);
    fs::remove_dir_all(&root).unwrap();
}
