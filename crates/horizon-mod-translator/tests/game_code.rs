//! The game's own code as a library of the mods' (`program_with`), and its methods standing for
//! ones the runtime doesn't map (`derive`): from a made-up game jar, compiled here.
//! Needs a JDK (`javac`); skipped without one.
use horizon_mod_translator::{
    derive::{Deriver, Game, Mapped},
    program_with,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

fn has(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok()
}

/// A class of a package the runtime names nothing in: the game's own code to it.
const TALLY: &str = "package net.minecraft.hztest;
public class Tally {
    private int n;
    public Tally(int start) { n = start; }
    public int add(int by) { n += by; return n; }
    public int unused() { return -1; }
    public static int twice(int x) { return x * 2; }
}";

/// A class the runtime models, as this game has it: two methods the test's runtime maps, and
/// more it doesn't.
const BLOCK_POS: &str = "package net.minecraft.core;
public class BlockPos {
    private int hidden;
    public int getX() { return 1; }
    public int getY() { return 2; }
    public int sum() { return getX() + getY(); }
    public int secret() { return hidden; }
    public int both() { return sum() + secret(); }
    public int again() { return 1; }
    private int kept;
    public int getKept() { return kept; }
    public int keptTwice() { return kept * 2; }
    public int open;
    public int getOpen() { return open; }
}";

const LOWER: &str = "package net.minecraft.core;
public class Lower extends BlockPos {
    public int again() { return 2; }
    public int viaSuper() { return super.again() + 1; }
}";

const USES: &str = "package example;
import net.minecraft.core.BlockPos;
import net.minecraft.hztest.Tally;
public class Uses {
    public static int run(BlockPos p) {
        Tally t = new Tally(3);
        return t.add(4) + Tally.twice(5) + p.sum() + p.secret() + p.both() + p.again();
    }
}";

/// `sources` (path, text) compiled against `classpath` and put in `dir/<name>.jar`.
fn jar(
    dir: &Path,
    name: &str,
    sources: &[(&str, &str)],
    classpath: Option<&Path>,
) -> (PathBuf, PathBuf) {
    let (src, out) = (
        dir.join(format!("{name}-src")),
        dir.join(format!("{name}-classes")),
    );
    let mut javac = Command::new("javac");
    javac.arg("-d").arg(&out);
    if let Some(cp) = classpath {
        javac.arg("-cp").arg(cp);
    }
    for (path, text) in sources {
        let file = src.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        javac.arg(file);
    }
    assert!(javac.status().unwrap().success());
    let jar = dir.join(format!("{name}.jar"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
    let mut todo = vec![out.clone()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                let name = p
                    .strip_prefix(&out)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&std::fs::read(&p).unwrap()).unwrap();
            }
        }
    }
    zip.finish().unwrap();
    (jar, out)
}

fn jars(dir: &Path) -> (PathBuf, PathBuf) {
    let (game, classes) = jar(
        dir,
        "game",
        &[
            ("net/minecraft/hztest/Tally.java", TALLY),
            ("net/minecraft/core/BlockPos.java", BLOCK_POS),
            ("net/minecraft/core/Lower.java", LOWER),
        ],
        None,
    );
    let (mods, _) = jar(dir, "mod", &[("example/Uses.java", USES)], Some(&classes));
    (game, mods)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hz-game-code-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A class of the game the runtime doesn't name is translated with the mod, as far as the mod
/// reaches into it; one the runtime names stays the runtime's; and without the game's jars
/// nothing of the game is there.
#[test]
fn the_games_own_code_joins_the_program_as_far_as_it_is_reached() {
    if !has("javac") {
        eprintln!("skipped: no javac");
        return;
    }
    let dir = scratch("program");
    let (game, mods) = jars(&dir);
    let (program, _, _) = program_with(&[&mods], &[], &[&game]).unwrap();
    let tally = program
        .iter()
        .find(|c| c.name == "net/minecraft/hztest/Tally")
        .expect("the game's class is in the program");
    let mut methods: Vec<&str> = tally
        .methods
        .iter()
        .filter(|m| m.code.is_some())
        .map(|m| m.name.as_str())
        .collect();
    methods.sort_unstable();
    assert_eq!(methods, ["<init>", "add", "twice"]);
    assert!(horizon_mod_translator::runtime_names(
        "net/minecraft/core/BlockPos"
    ));
    assert!(
        !program
            .iter()
            .any(|c| c.name.starts_with("net/minecraft/core/"))
    );
    let (alone, _, _) = program_with(&[&mods], &[], &[]).unwrap();
    assert_eq!(
        alone.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["example/Uses"]
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A method of a modelled class stands where its code asks only what is mapped: `sum` calls two
/// mapped getters. `secret` reads a field nothing answers and `both` calls it: each is blocked,
/// by the member to map. `again` has code in the class below too: the call stands with both, each
/// carried in a class named for its own, and the upper class is marked (an object known only as
/// it may be the lower one's). A `super` call of such a method can't be told from the object's
/// own at run time, and blocks. `keptTwice` reads a field a mapped method only returns: it stands,
/// reading through that method.
#[test]
fn a_method_stands_where_its_code_asks_only_what_is_mapped() {
    if !has("javac") {
        eprintln!("skipped: no javac");
        return;
    }
    let dir = scratch("derive");
    let (game, _) = jars(&dir);
    let game = Game::open(&[&game]).unwrap();
    let mapped = Mapped {
        method: &|m, _| matches!(m.name.as_str(), "getX" | "getY" | "getKept"),
        field: &|_, _, _| false,
        constant: &|_| false,
        translated: &|_| false,
    };
    let mut deriver = Deriver::new(&game, mapped);
    let (pos, lower) = ("net/minecraft/core/BlockPos", "net/minecraft/core/Lower");
    let blockers = |d: &mut Deriver, class: &str, name: &str| {
        d.blockers(class, name, "()I", true)
            .into_iter()
            .collect::<Vec<_>>()
    };
    assert_eq!(blockers(&mut deriver, pos, "sum"), Vec::<String>::new());
    assert_eq!(
        blockers(&mut deriver, pos, "secret"),
        ["net/minecraft/core/BlockPos.hidden"]
    );
    assert_eq!(
        blockers(&mut deriver, pos, "both"),
        ["net/minecraft/core/BlockPos.hidden"]
    );
    assert_eq!(blockers(&mut deriver, pos, "again"), Vec::<String>::new());
    assert_eq!(
        blockers(&mut deriver, lower, "viaSuper"),
        ["net/minecraft/core/BlockPos.again.()I [super call]"]
    );
    // A field nothing answers that a mapped method only returns is read through that method.
    assert_eq!(blockers(&mut deriver, pos, "keptTwice"), Vec::<String>::new());
    assert_eq!(
        deriver.accessors.get(&(pos.to_owned(), "kept".to_owned(), false)).map(String::as_str),
        Some("net/minecraft/core/BlockPos.getKept.()I")
    );
    let classes = deriver.classes();
    let shape: Vec<(&str, Vec<&str>, Vec<&str>)> = classes
        .iter()
        .map(|c| {
            (
                c.name.as_str(),
                c.methods.iter().map(|m| m.name.as_str()).collect(),
                c.fields.iter().map(|f| f.name.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            (
                "hz/derived/net/minecraft/core/BlockPos",
                vec!["again", "keptTwice", "sum"],
                vec!["!again"]
            ),
            ("hz/derived/net/minecraft/core/Lower", vec!["again"], vec![]),
        ]
    );
    assert_eq!(
        classes[1].superclass.as_deref(),
        Some("hz/derived/net/minecraft/core/BlockPos")
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A field of the game's that a mod's own code reads, which nothing answers and a mapped method
/// only returns, is read through that method, by the class the mod's reference names:
/// `lower.open`, a field `BlockPos` declares, read through `Lower`. A field the runtime answers
/// for itself is left to it.
#[test]
fn a_mods_read_of_a_games_field_goes_through_its_getter() {
    if !has("javac") {
        eprintln!("skipped: no javac");
        return;
    }
    const READS: &str = "package example;
import net.minecraft.core.Lower;
public class Reads {
    public static int run(Lower l) { return l.open + 1; }
}";
    let dir = scratch("field-accessors");
    let (game_jar, classes) = jar(
        &dir,
        "game",
        &[
            ("net/minecraft/core/BlockPos.java", BLOCK_POS),
            ("net/minecraft/core/Lower.java", LOWER),
        ],
        None,
    );
    let (_, own) = jar(
        &dir,
        "mod",
        &[("example/Reads.java", READS)],
        Some(&classes),
    );
    let reads = horizon_mod_translator::class::ClassFile::parse(
        &std::fs::read(own.join("example/Reads.class")).unwrap(),
    )
    .unwrap();
    let game = Game::open(&[&game_jar]).unwrap();
    let through = |answered: bool| {
        let field = move |_: &str, name: &str, _: &str| answered && name == "open";
        let mapped = Mapped {
            method: &|m, _| m.name == "getOpen",
            field: &field,
            constant: &|_| false,
            translated: &|_| false,
        };
        let mut deriver = Deriver::new(&game, mapped);
        deriver.field_accessors(&reads);
        deriver
            .accessors
            .get(&(
                "net/minecraft/core/Lower".to_owned(),
                "open".to_owned(),
                false,
            ))
            .cloned()
    };
    assert_eq!(
        through(false).as_deref(),
        Some("net/minecraft/core/BlockPos.getOpen.()I")
    );
    assert_eq!(through(true), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// An enum of the game's the program names is carried with its constants, in their order, though
/// no method of it stands: the runtime answers its `values()` and a `switch` over it by them.
#[test]
fn a_game_enum_is_carried_with_its_constants() {
    if !has("javac") {
        eprintln!("skipped: no javac");
        return;
    }
    let dir = scratch("enums");
    let sources = [
        (
            "net/minecraft/core/Mode.java",
            "package net.minecraft.core; public enum Mode { OFF, ON, AUTO; public static final int COUNT = 3; }",
        ),
        (
            "net/minecraft/core/Plain.java",
            "package net.minecraft.core; public class Plain { public static final Plain ONE = new Plain(); }",
        ),
    ];
    let (game, _) = jar(&dir, "game", &sources, None);
    let game = Game::open(&[&game]).unwrap();
    let mapped = Mapped {
        method: &|_, _| false,
        field: &|_, _, _| false,
        constant: &|_| false,
        translated: &|_| false,
    };
    let mut deriver = Deriver::new(&game, mapped);
    deriver.enums.insert("net/minecraft/core/Mode".to_owned());
    deriver.enums.insert("net/minecraft/core/Plain".to_owned());
    let classes = deriver.classes();
    assert_eq!(
        classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["hz/derived/net/minecraft/core/Mode"]
    );
    let constants: Vec<(&str, bool)> = classes[0]
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.access & 0x4000 != 0))
        .collect();
    assert_eq!(constants, [("OFF", true), ("ON", true), ("AUTO", true)]);
    assert!(classes[0].methods.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// An enum of the Java runtime outside its core packages that the program names
/// (`HttpClient.Version`) is read from the runtime's image and carried with its constants as the
/// game's enums are; a class there that is no enum, and an enum the program does not name, stay
/// unread.
#[test]
fn a_java_runtimes_enum_the_program_names_is_carried() {
    let Some(image) = horizon_mod_translator::find_java_image() else {
        eprintln!(
            "skipped: no Java {} runtime",
            horizon_mod_translator::JAVA_VERSION
        );
        return;
    };
    let named: std::collections::HashSet<String> = [
        "java/net/http/HttpClient$Version",
        "java/net/http/HttpClient",
    ]
    .iter()
    .map(|n| (*n).to_owned())
    .collect();
    let game = Game::open_naming(&[&image], &named).unwrap();
    assert!(game.class("java/net/http/HttpClient$Version").is_some());
    assert!(
        game.class("java/net/http/HttpClient").is_none(),
        "no enum: its code is not the translator's to take"
    );
    assert!(
        game.class("java/net/http/HttpClient$Redirect").is_none(),
        "not named"
    );
    let mapped = Mapped {
        method: &|_, _| false,
        field: &|_, _, _| false,
        constant: &|_| false,
        translated: &|_| false,
    };
    let mut deriver = Deriver::new(&game, mapped);
    deriver
        .enums
        .insert("java/net/http/HttpClient$Version".to_owned());
    let classes = deriver.classes();
    assert_eq!(
        classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["hz/derived/java/net/http/HttpClient$Version"]
    );
    let constants: Vec<&str> = classes[0]
        .fields
        .iter()
        .filter(|f| f.access & 0x4000 != 0)
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(constants, ["HTTP_1_1", "HTTP_2"]);
}

/// A class's method wins over an interface's default, as the JVM resolves a call: a mod's class
/// that extends a class of the game's and implements an interface with a default method of the same
/// name takes the game's (TFMG's cable block entity, `IElectric.getBlockPos()` and
/// `BlockEntity.getBlockPos()`). Its code then holds that method as its own, asking the class
/// above; a class with no such class above keeps the default, and one that declares the method
/// itself needs nothing.
#[test]
fn a_games_method_wins_over_an_interfaces_default() {
    if !has("javac") {
        eprintln!("skipped: no javac");
        return;
    }
    let dir = scratch("inherited");
    let game_sources = [(
        "net/minecraft/hztest/Thing.java",
        "package net.minecraft.hztest; public class Thing { public int place() { return 7; } public int other() { return 1; } }",
    )];
    let (game_jar, game_classes) = jar(&dir, "game", &game_sources, None);
    let mod_sources = [
        (
            "t/Placed.java",
            "package t; public interface Placed { int code(); default int place() { return code() * 100; } }",
        ),
        (
            "t/Machine.java",
            "package t; public class Machine extends net.minecraft.hztest.Thing implements Placed { public int code() { return place(); } }",
        ),
        (
            "t/Deeper.java",
            "package t; public class Deeper extends Machine { }",
        ),
        (
            "t/Own.java",
            "package t; public class Own extends net.minecraft.hztest.Thing implements Placed { public int code() { return 1; } public int place() { return 2; } }",
        ),
        (
            "t/Plain.java",
            "package t; public class Plain implements Placed { public int code() { return 3; } }",
        ),
    ];
    let (mod_jar, _) = jar(&dir, "mod", &mod_sources, Some(&game_classes));
    // The game's class is the runtime's to answer here, not code to translate: the program is the mod's.
    let program = horizon_mod_translator::classes(&mod_jar).unwrap();
    let game = Game::open(&[&game_jar]).unwrap();
    let taken = horizon_mod_translator::derive::inherited_over_defaults(&program, &game);
    assert_eq!(
        taken.into_iter().collect::<Vec<_>>(),
        [(
            "t/Machine".to_owned(),
            vec![("place".to_owned(), "()I".to_owned())]
        )]
    );
    // And as the code is written: the class's own entry for the method, asking the class above.
    horizon_mod_translator::codegen::set_inherited(
        horizon_mod_translator::derive::inherited_over_defaults(&program, &game),
    );
    let module = horizon_mod_translator::codegen::Program::new(&program).emit();
    horizon_mod_translator::codegen::set_inherited(Default::default());
    let stands = format!(
        "{}__above",
        horizon_mod_translator::codegen::fn_name("t/Machine", "place", "()I")
    );
    assert!(
        module.contains(&format!("(\"place.()I\", {stands}, false)")),
        "the class's table has no entry for it"
    );
    assert!(
        module.contains("api::call(rt, \"net/minecraft/hztest/Thing.place.()I\", args)"),
        "nothing asks the class above"
    );
    let _ = std::fs::remove_dir_all(dir);
}
