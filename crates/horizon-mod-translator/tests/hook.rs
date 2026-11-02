//! A NeoForge-style test mod (tests/fixtures/mod, compiled against our own
//! API stubs in tests/fixtures/stubs) translated to Rust and driven through
//! the hook glue, as Horizon's `tick_block` hook would: its registration
//! runs, and its block's `tick` changes the world. Needs `javac`.
use std::{fs, path::Path, process::Command};

#[test]
fn a_translated_mod_block_ticks_through_the_hook() {
    if !Command::new("javac").arg("-version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("no JDK: skipped");
        return;
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = std::env::temp_dir().join(format!("mod-translator-hook-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let classes = root.join("classes");
    fs::create_dir_all(&classes).unwrap();
    let mut javac = Command::new("javac");
    javac.args(["-d", classes.to_str().unwrap()]);
    for dir in ["tests/fixtures/stubs", "tests/fixtures/mod"] {
        let mut stack = vec![here.join(dir)];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(d).unwrap().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    javac.arg(e.path());
                }
            }
        }
    }
    assert!(javac.status().unwrap().success());
    // Only the mod's classes are translated: the stubs stand for the game.
    let parsed: Vec<_> = fs::read_dir(classes.join("testmod"))
        .unwrap()
        .flatten()
        .map(|e| horizon_mod_translator::class::ClassFile::parse(&fs::read(e.path()).unwrap()).unwrap())
        .collect();
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    let module = program.emit();
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
    fs::write(krate.join("src/testmod.rs"), module).unwrap();
    fs::write(
        krate.join("src/main.rs"),
        r#"mod testmod;
use horizon_compat::{glue, host::{Host, MemoryHost}, Rt};
fn main() {
    horizon_compat::install();
    let mut rt = Rt::new();
    testmod::init(&mut rt);
    testmod::start(&mut rt);
    let mut names: Vec<_> = rt.registered.keys().filter(|(_, n)| n.starts_with("testmod:")).map(|(r, n)| format!("{r} {n}")).collect();
    names.sort();
    println!("registered: {names:?}");
    let mut host = MemoryHost::default();
    host.set_block_state([4, 64, 4], "testmod:grow");
    for step in 0..3 {
        let ran = glue::tick_block(&mut rt, &mut host, [4, 64, 4], "testmod:grow");
        println!("tick {step}: ran={ran} above={}", host.block_state([4, 65, 4]));
    }
    let ran = glue::tick_block(&mut rt, &mut host, [0, 0, 0], "minecraft:stone");
    println!("vanilla block: ran={ran}");
    // A machine: its block entity ticks, saves, and survives losing its live object.
    println!("counter: entity={:?} ticks={} grow: entity={:?}", glue::block_entity_id(&rt, "testmod:counter"), glue::block_entity_ticks(&rt, "testmod:counter"), glue::block_entity_id(&rt, "testmod:grow"));
    let mut nbt = std::collections::BTreeMap::new();
    for step in 1..=4 {
        if step == 3 {
            glue::block_entity_removed(&mut rt, [8, 64, 8]); // as a chunk unloading and loading again
        }
        glue::block_entity_tick(&mut rt, &mut host, [8, 64, 8], "testmod:counter", &mut nbt);
        println!("be tick {step}: nbt={nbt:?} above={}", host.block_state([8, 65, 8]));
    }
    // The level loaded (once) for those ticks: the `@EventBusSubscriber` and the registered object heard it.
    println!("level loads: static={:?} object={:?}", rt.statics.get("testmod/ModEvents.loads"), rt.statics.get("testmod/ModEvents.objectLoads"));
    // Right clicks: toggles, and passes while sneaking.
    host.set_block_state([12, 64, 12], "testmod:switch[powered=false]");
    let took = glue::use_block(&mut rt, &mut host, [12, 64, 12], "testmod:switch[powered=false]", 1);
    println!("click: took={took} now={} above={}", host.block_state([12, 64, 12]), host.block_state([12, 65, 12]));
    host.sneaking = true;
    let took = glue::use_block(&mut rt, &mut host, [12, 64, 12], "testmod:switch[powered=true]", 1);
    println!("sneak click: took={took} now={}", host.block_state([12, 64, 12]));
    let mut un: Vec<_> = rt.unmapped.keys().collect();
    un.sort();
    println!("unmapped: {un:?}");
}
"#,
    )
    .unwrap();
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
    assert_eq!(lines[0], r#"registered: ["block testmod:counter", "block testmod:grow", "block testmod:switch", "block_entity_type testmod:counter"]"#);
    assert_eq!(lines[1], "tick 0: ran=true above=minecraft:stone");
    assert_eq!(lines[2], "tick 1: ran=true above=minecraft:dirt");
    assert_eq!(lines[3], "tick 2: ran=true above=minecraft:dirt");
    assert_eq!(lines[4], "vanilla block: ran=false");
    assert_eq!(lines[5], r#"counter: entity=Some("testmod:counter") ticks=true grow: entity=None"#);
    assert_eq!(lines[6], r#"be tick 1: nbt={"count": Int(1)} above=minecraft:air"#);
    assert_eq!(lines[7], r#"be tick 2: nbt={"count": Int(2)} above=minecraft:air"#);
    assert_eq!(lines[8], r#"be tick 3: nbt={"count": Int(3)} above=minecraft:stone"#);
    assert_eq!(lines[9], r#"be tick 4: nbt={"count": Int(4)} above=minecraft:stone"#);
    assert_eq!(lines[10], "level loads: static=Some(I(11)) object=Some(I(1))");
    assert_eq!(lines[11], "click: took=true now=testmod:switch[powered=true] above=minecraft:stone");
    assert_eq!(lines[12], "sneak click: took=false now=testmod:switch[powered=true]");
    assert_eq!(lines[13], "unmapped: []");
    fs::remove_dir_all(&root).unwrap();
}
