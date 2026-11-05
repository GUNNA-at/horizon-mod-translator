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
    ("records", "(I)I", &["0", "5", "-12"]),
    ("patterns", "(I)I", &["0", "1", "2"]),
    ("grid", "(I)I", &["0", "1", "6"]),
    ("reflection", "(I)I", &["3", "-12"]),
    ("staticFields", "(I)I", &["0", "5"]),
    ("staticRefs", "(I)I", &["3"]),
    ("library", "(I)I", &["0", "3", "-7"]),
    ("rawBoxes", "(I)I", &["0", "2"]),
    ("adapted", "(I)I", &["0", "3", "-5"]),
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
    let parsed = horizon_mod_translator::classes(&jar).unwrap();
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
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    program.services = horizon_mod_translator::services(&jar).unwrap();
    let module = program.emit();

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
            main.push_str(&format!(
                "    let r = {f}(&mut rt, &[{}]); println!(\"{name}({input}) = {{}}\", {fmt});\n",
                rust_value(desc, input)
            ));
        }
    }
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
    for (e, a) in expected.lines().zip(actual.lines()) {
        assert_eq!(a, e);
    }
    assert_eq!(expected.lines().count(), CASES.iter().map(|c| c.2.len()).sum::<usize>(), "{expected}");
    assert_eq!(expected.lines().count(), actual.lines().count());
    eprintln!("{actual}");
    assert!(program.unsupported.is_empty(), "{:?}", program.unsupported);
    fs::remove_dir_all(&root).unwrap();
}
