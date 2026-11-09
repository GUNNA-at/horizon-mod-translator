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
}";

const LOWER: &str = "package net.minecraft.core;
public class Lower extends BlockPos {
    public int again() { return 2; }
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
/// mapped getters. `secret` reads a field nothing answers, `both` calls it, and `again` is
/// declared once more below: each is blocked, by the member to map. The method that stands is
/// carried in a class named for its own.
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
        method: &|m, _| matches!(m.name.as_str(), "getX" | "getY"),
        field: &|_, _, _| false,
        constant: &|_| false,
        translated: &|_| false,
    };
    let mut deriver = Deriver::new(&game, mapped);
    let pos = "net/minecraft/core/BlockPos";
    let blockers = |d: &mut Deriver, name: &str| {
        d.blockers(pos, name, "()I", true)
            .into_iter()
            .collect::<Vec<_>>()
    };
    assert_eq!(blockers(&mut deriver, "sum"), Vec::<String>::new());
    assert_eq!(
        blockers(&mut deriver, "secret"),
        ["net/minecraft/core/BlockPos.hidden"]
    );
    assert_eq!(
        blockers(&mut deriver, "both"),
        ["net/minecraft/core/BlockPos.hidden"]
    );
    assert_eq!(
        blockers(&mut deriver, "again"),
        ["net/minecraft/core/BlockPos.again.()I"]
    );
    let classes = deriver.classes();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "hz/derived/net/minecraft/core/BlockPos");
    assert_eq!(
        classes[0]
            .methods
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["sum"]
    );
    let _ = std::fs::remove_dir_all(dir);
}
