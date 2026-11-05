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

/// The receiver a coverage probe calls an API method with.
#[derive(Clone, Copy)]
enum Receiver {
    /// Null: a static method's, or one answered whatever the receiver.
    Null,
    /// A bare object of the owner's class.
    Bare,
    /// The probe's server player, the player in place ([`probe_host`]).
    Player,
    /// The probe's cow, a living entity of the level.
    Mob,
    /// The runtime's level (`glue::level`), the one a mod's code is given.
    Level,
    /// An API class's static field (`owner.NAME`) the mod reads.
    Static(&'static str),
    /// A new object of the class, its constructor (the key, with its argument count) run.
    New(&'static str, usize),
    /// What an API method (the key, with its argument count) returns.
    Returned(&'static str, usize),
}

/// The receivers the mod makes of each API class it calls: a constructor of the class it calls,
/// and the API methods it calls that return the class, up to [`STATICS_PER_CLASS`] of each class.
fn made_receivers(calls: &std::collections::BTreeMap<horizon_mod_translator::class::Member, usize>) -> std::collections::BTreeMap<String, Vec<Receiver>> {
    let mut by: std::collections::BTreeMap<String, Vec<Receiver>> = Default::default();
    for m in calls.keys() {
        let key: &'static str = Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
        let n = horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + 1;
        let (class, r) = if m.name == "<init>" {
            (m.owner.as_str(), Receiver::New(key, n))
        } else {
            let Some(ret) = m.desc.rsplit(')').next().and_then(|r| r.strip_prefix('L')).and_then(|r| r.strip_suffix(';')) else { continue };
            if !horizon_mod_translator::is_api(ret) {
                continue;
            }
            (ret, Receiver::Returned(key, n))
        };
        let list = by.entry(class.to_owned()).or_default();
        if list.len() < STATICS_PER_CLASS {
            list.push(r);
        }
    }
    by
}

/// The static fields of API classes the mod's classes refer to, by their declared class: up to
/// [`STATICS_PER_CLASS`] of each, the first met.
fn static_receivers(classes: &[horizon_mod_translator::class::ClassFile]) -> std::collections::BTreeMap<String, Vec<&'static str>> {
    let mut by: std::collections::BTreeMap<String, Vec<&'static str>> = Default::default();
    for c in classes {
        for i in 0..c.pool.len() as u16 {
            if !matches!(c.pool[i as usize], horizon_mod_translator::class::Const::FieldRef(..)) {
                continue;
            }
            let Some(m) = c.member(i) else { continue };
            let Some(ty) = m.desc.strip_prefix('L').and_then(|d| d.strip_suffix(';')) else { continue };
            if !horizon_mod_translator::is_api(&m.owner) || !horizon_mod_translator::is_api(ty) {
                continue;
            }
            let list = by.entry(ty.to_owned()).or_default();
            let key = format!("{}.{}", m.owner, m.name);
            if list.len() < STATICS_PER_CLASS && !list.iter().any(|k| *k == key) {
                list.push(Box::leak(key.into_boxed_str()));
            }
        }
    }
    by
}

/// How many static fields of one class a method is tried on.
const STATICS_PER_CLASS: usize = 3;

/// Whether `owner` is one of the level's classes or interfaces, whose methods a mod calls on the
/// level it is given.
fn level_class(owner: &str) -> bool {
    matches!(
        owner.strip_prefix("net/minecraft/world/level/"),
        Some(
            "Level" | "LevelAccessor" | "LevelReader" | "BlockGetter" | "LevelWriter" | "CommonLevelAccessor" | "ServerLevelAccessor"
                | "LevelSimulatedReader" | "LevelSimulatedRW" | "EntityGetter" | "CollisionGetter" | "SignalGetter" | "LevelHeightAccessor"
                | "LevelTimeAccess" | "BlockAndTintGetter"
        )
    ) || matches!(
        owner,
        "net/minecraft/server/level/ServerLevel"
            | "net/neoforged/neoforge/common/extensions/ILevelExtension"
            | "net/neoforged/neoforge/common/extensions/ILevelReaderExtension"
            | "net/neoforged/neoforge/common/extensions/IBlockGetterExtension"
    )
}

/// The probe's player's UUID and its cow's entity id.
const PROBE_PLAYER: u128 = 0x5052_4f42_4500_0000_0000_0000_0000_0001;
const PROBE_MOB: i32 = 7;

/// What a hosted probe's calls ask: a player in place at full health, standing at the level's
/// origin, and a cow beside them.
fn probe_host() -> horizon_compat::host::MemoryHost {
    use horizon_compat::host::{EntityView, MemoryHost, VitalsView};
    let place = [0.5, 64.0, 0.5];
    let mut h = MemoryHost {
        player_id: Some(PROBE_PLAYER),
        place: Some((place, 0.0, 0.0)),
        vitals: Some(VitalsView::default()),
        players: vec![(PROBE_PLAYER, place)],
        ..MemoryHost::default()
    };
    h.player_entity_ids.insert(PROBE_PLAYER, 1);
    h.entities.push(EntityView { id: PROBE_MOB, kind: "minecraft:cow".into(), pos: [2.5, 64.0, 0.5], health: Some((10.0, 10.0)), ..EntityView::default() });
    h.effects.insert(None, Vec::new());
    h.effects.insert(Some(PROBE_MOB), Vec::new());
    h
}

/// Whether `owner` is one of vanilla's entity classes (or an interface of theirs), whose methods a
/// mod calls on the game's players and mobs.
fn entity_class(owner: &str) -> bool {
    owner.starts_with("net/minecraft/world/entity/")
        || matches!(owner, "net/minecraft/server/level/ServerPlayer" | "net/minecraft/client/player/LocalPlayer" | "net/minecraft/client/player/AbstractClientPlayer")
        || owner.starts_with("net/neoforged/neoforge/common/extensions/IEntityExtension")
        || owner.starts_with("net/neoforged/neoforge/common/extensions/ILivingEntityExtension")
        || owner.starts_with("net/neoforged/neoforge/common/extensions/IPlayerExtension")
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `bundle <jar> <out> [--with <addon jar>]... [--library <jar>]... [--runtime <horizon-compat-core
    // dir>]`: `--with` translates mods built on the first with it, as one engine (see
    // `bundle::write_group`); `--library`, a jar whose code they may call (a language loader's,
    // Kotlin for Forge's), of which what they reach; `--runtime`, a bundle for tools whose engine
    // links the runtime core instead of carrying it.
    if args.first().map(String::as_str) == Some("bundle") && args.len() >= 3 {
        let (mut with, mut libraries, mut runtime, mut rest) = (Vec::new(), Vec::new(), None, args[3..].iter());
        while let Some(a) = rest.next() {
            match (a.as_str(), rest.next()) {
                ("--with", Some(j)) => with.push(Path::new(j)),
                ("--library", Some(j)) => libraries.push(Path::new(j)),
                ("--runtime", Some(dir)) => runtime = Some(Path::new(dir)),
                _ => {
                    eprintln!("bundle <jar> <out dir> [--with <addon jar>]... [--library <jar>]... [--runtime <horizon-compat-core dir>]");
                    return ExitCode::FAILURE;
                }
            }
        }
        let jars: Vec<&Path> = std::iter::once(Path::new(&args[1])).chain(with).collect();
        let result = horizon_mod_translator::program_of(&jars, &libraries).and_then(|(c, library, kept)| {
            println!("{} classes, {kept} of them of the {library} in the libraries the jars bundle", c.len());
            horizon_mod_translator::bundle::write_group(&jars, &c, Path::new(&args[2]), runtime)
        });
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
        let all: Vec<&horizon_mod_translator::class::ClassFile> = classes.iter().collect();
        let configs = horizon_mod_translator::mixin_configs_of(&jars);
        let plan = horizon_mod_translator::weave::plan_with(&all, &configs);
        for c in &configs {
            for m in &c.off {
                println!("  off {m} (its plugin's, for the mods here)");
            }
        }
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
        for (k, n) in &plan.skipped {
            println!("  skipped {n:5} {k} (may find none: Mixin skips them too)");
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
        // object of the owner's class as the receiver (or first argument of a static call). With
        // PROBE=hosted, each call also has a game to ask (`probe_host`: a player in place, a cow
        // beside them), and an entity class's method is tried on that player and that cow too, as
        // a mod's calls find them in a game; a level class's on the runtime's level; and any
        // method on the static fields of API classes the mod reads whose declared type is the
        // method's class (`BuiltInRegistries.ITEM` for `DefaultedRegistry.getKey`, `MobEffects.SPEED`
        // for `Holder.value`, `Codec.STRING` for `PrimitiveCodec.fieldOf`), as its calls get them.
        let hosted = std::env::var_os("PROBE").is_some_and(|p| p == "hosted");
        let statics = if hosted { static_receivers(&classes) } else { Default::default() };
        // And on what the mod makes of the method's class: an object its constructor builds, or
        // what an API method returning that class gives (`NonNullList.withSize`), null arguments.
        let made = if hosted { made_receivers(&calls) } else { Default::default() };
        let (mut mapped, mut sites, mut mapped_sites) = (0usize, 0usize, 0usize);
        let mut missing: Vec<(usize, String)> = Vec::new();
        let answers = |key: &'static str, owner: &'static str, n: usize, receiver: Receiver| {
            let mut rt = horizon_compat::Rt::new();
            let call = |rt: &mut horizon_compat::Rt| {
                let mut args = vec![horizon_compat::V::NULL; n];
                match receiver {
                    Receiver::Null => {}
                    Receiver::Bare => args[0] = rt.new_object(owner),
                    Receiver::Player => args[0] = horizon_compat::menus::server_player(rt, PROBE_PLAYER),
                    Receiver::Mob => args[0] = horizon_compat::vanilla_entities::of(rt, PROBE_MOB, "minecraft:cow", None),
                    Receiver::Level => args[0] = horizon_compat::glue::level(rt),
                    Receiver::New(init, n) => {
                        let class = init.split_once('.').map_or(owner, |(c, _)| c);
                        let o = rt.new_object(class);
                        let mut a = vec![horizon_compat::V::NULL; n];
                        a[0] = o;
                        horizon_compat::api::call(rt, init, &a);
                        if !rt.unmapped.is_empty() || rt.thrown.is_some() {
                            return false;
                        }
                        args[0] = o;
                    }
                    Receiver::Returned(maker, n) => {
                        let v = horizon_compat::api::call(rt, maker, &vec![horizon_compat::V::NULL; n]);
                        if v == horizon_compat::V::NULL || v == horizon_compat::V::Void || !rt.unmapped.is_empty() || rt.thrown.is_some() {
                            return false;
                        }
                        args[0] = v;
                    }
                    Receiver::Static(field) => {
                        let v = horizon_compat::api::get_static(rt, field);
                        if v == horizon_compat::V::NULL || v == horizon_compat::V::Void || !rt.unmapped.is_empty() {
                            return false;
                        }
                        args[0] = v;
                    }
                }
                horizon_compat::api::call(rt, key, &args);
                rt.unmapped.is_empty()
            };
            if hosted {
                let mut host = probe_host();
                rt.with_host(&mut host, call)
            } else {
                call(&mut rt)
            }
        };
        for (m, n) in &calls {
            let key: &'static str = Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
            let arity = horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + 1;
            sites += n;
            let owner: &'static str = Box::leak(m.owner.clone().into_boxed_str());
            let entity = hosted && entity_class(owner);
            let level = hosted && level_class(owner);
            let fields = statics.get(owner).map_or(&[][..], Vec::as_slice);
            if answers(key, owner, arity, Receiver::Null)
                || answers(key, owner, arity, Receiver::Bare)
                || (entity && (answers(key, owner, arity, Receiver::Player) || answers(key, owner, arity, Receiver::Mob)))
                || (level && answers(key, owner, arity, Receiver::Level))
                || fields.iter().any(|&f| answers(key, owner, arity, Receiver::Static(f)))
                || made.get(owner).map_or(&[][..], Vec::as_slice).iter().any(|&r| answers(key, owner, arity, r))
            {
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
