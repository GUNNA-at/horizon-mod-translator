//! A translated jar as a Horizon native mod bundle (docs/native-mods.md):
//! `horizon-mixin.toml`, one helper per hook point that turns Horizon's
//! `GameLevel` into a [`horizon_compat::host::Host`] and calls the mod
//! through `horizon_compat::glue`, and the crates holding the translated code:
//! `runtime/` (horizon-compat's sources), `code-<mod>-<n>/` (the mods' code, see
//! [`crate::split`]) and `engine/` (the tables and the entry points the hooks call).
use crate::{class::ClassFile, codegen::Program};
use std::{fs, io::Read, path::Path};

/// The mod id in a jar's `META-INF/neoforge.mods.toml` (or `mods.toml`).
pub fn mod_id(jar: &Path) -> Option<String> {
    let mut zip = zip::ZipArchive::new(fs::File::open(jar).ok()?).ok()?;
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        let Ok(mut e) = zip.by_name(name) else { continue };
        let mut text = String::new();
        e.read_to_string(&mut text).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("modId").map(str::trim).and_then(|v| v.strip_prefix('=')) {
                return Some(toml_string(v).to_owned());
            }
        }
    }
    None
}

/// A TOML value as written on its line: a quoted string's content (a basic string's escapes
/// read: `displayName="The \"Mod\""`), or a bare word, without what follows it
/// (`modId="torchmaster" #mandatory` is `torchmaster`).
/// A TOML table header line as its table, without its comment and the spaces TOML allows in it
/// (`[[mods]] #mandatory` and `[[ mods ]]` are `[[mods]]`); None for a line that isn't one.
pub(crate) fn toml_header(line: &str) -> Option<String> {
    let line = line.trim_start();
    if !line.starts_with('[') {
        return None;
    }
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '#' => break,
            None if c.is_whitespace() => {}
            None => {
                if matches!(c, '"' | '\'') {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    Some(out)
}

pub(crate) fn toml_string(v: &str) -> String {
    let v = v.trim();
    match v.chars().next() {
        Some('\'') => v[1..].split('\'').next().unwrap_or("").to_owned(),
        Some('"') => {
            let mut out = String::new();
            let mut chars = v[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('r') => out.push('\r'),
                        Some('b') => out.push('\u{8}'),
                        Some('f') => out.push('\u{c}'),
                        Some(u @ ('u' | 'U')) => {
                            let hex: String = chars.by_ref().take(if u == 'u' { 4 } else { 8 }).collect();
                            out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
                        }
                        // `\"` and `\\`.
                        Some(c) => out.push(c),
                        None => break,
                    },
                    c => out.push(c),
                }
            }
            out
        }
        _ => v.split(|c: char| c == '#' || c.is_whitespace()).next().unwrap_or("").to_owned(),
    }
}

/// The runtime's sources, carried in each bundle as a crate of its own (`runtime/`), which the
/// engine and its code crates depend on: bundle crates may depend only on Horizon's Cargo.lock
/// registry crates and on their own mod's crates (horizon-mixins' rule; the runtime names
/// [`ENGINE_DEPENDENCIES`] for them). The core's root becomes the crate's `lib.rs`; horizon-compat's
/// root is left out (its modules are declared there instead).
const CORE: &str = include_str!("../../horizon-compat-core/src/lib.rs");
/// The mapping's root: its runtime helpers (between the `RUNTIME HELPERS` markers) go into the
/// inlined `compat` module beside the core's.
const COMPAT_ROOT: &str = include_str!("../../horizon-compat/src/lib.rs");
const COMPAT: [(&str, &str); 95] = [
    ("host.rs", include_str!("../../horizon-compat-core/src/host.rs")),
    ("gc.rs", include_str!("../../horizon-compat-core/src/gc.rs")),
    ("api.rs", include_str!("../../horizon-compat/src/api.rs")),
    ("api_common.rs", include_str!("../../horizon-compat/src/api_common.rs")),
    ("blocks.rs", include_str!("../../horizon-compat/src/blocks.rs")),
    ("block_breaking.rs", include_str!("../../horizon-compat/src/block_breaking.rs")),
    ("loot_params.rs", include_str!("../../horizon-compat/src/loot_params.rs")),
    ("breaks.rs", include_str!("../../horizon-compat/src/breaks.rs")),
    ("brigadier.rs", include_str!("../../horizon-compat/src/brigadier.rs")),
    ("codecs.rs", include_str!("../../horizon-compat/src/codecs.rs")),
    ("command_args.rs", include_str!("../../horizon-compat/src/command_args.rs")),
    ("commands.rs", include_str!("../../horizon-compat/src/commands.rs")),
    ("recipes.rs", include_str!("../../horizon-compat/src/recipes.rs")),
    ("crafting.rs", include_str!("../../horizon-compat/src/crafting.rs")),
    ("handlers.rs", include_str!("../../horizon-compat/src/handlers.rs")),
    ("components.rs", include_str!("../../horizon-compat/src/components.rs")),
    ("container_screens.rs", include_str!("../../horizon-compat/src/container_screens.rs")),
    ("capabilities.rs", include_str!("../../horizon-compat/src/capabilities.rs")),
    ("entities.rs", include_str!("../../horizon-compat/src/entities.rs")),
    ("tanks.rs", include_str!("../../horizon-compat/src/tanks.rs")),
    ("fluid_wrappers.rs", include_str!("../../horizon-compat/src/fluid_wrappers.rs")),
    ("item_wrappers.rs", include_str!("../../horizon-compat/src/item_wrappers.rs")),
    ("creative.rs", include_str!("../../horizon-compat/src/creative.rs")),
    ("crops.rs", include_str!("../../horizon-compat/src/crops.rs")),
    ("fluids.rs", include_str!("../../horizon-compat/src/fluids.rs")),
    ("neo_tags.rs", include_str!("../../horizon-compat/src/neo_tags.rs")),
    ("nightconfig.rs", include_str!("../../horizon-compat/src/nightconfig.rs")),
    ("annotations.rs", include_str!("../../horizon-compat/src/annotations.rs")),
    ("invoke.rs", include_str!("../../horizon-compat/src/invoke.rs")),
    ("worldgen.rs", include_str!("../../horizon-compat/src/worldgen.rs")),
    ("mod_entities.rs", include_str!("../../horizon-compat/src/mod_entities.rs")),
    ("projectiles.rs", include_str!("../../horizon-compat/src/projectiles.rs")),
    ("palette.rs", include_str!("../../horizon-compat/src/palette.rs")),
    ("particles.rs", include_str!("../../horizon-compat/src/particles.rs")),
    ("records.rs", include_str!("../../horizon-compat/src/records.rs")),
    ("rotation.rs", include_str!("../../horizon-compat/src/rotation.rs")),
    ("vanilla_records.rs", include_str!("../../horizon-compat/src/vanilla_records.rs")),
    ("vanilla_shapes.rs", include_str!("../../horizon-compat/src/vanilla_shapes.rs")),
    ("functions.rs", include_str!("../../horizon-compat/src/functions.rs")),
    ("gamerules.rs", include_str!("../../horizon-compat/src/gamerules.rs")),
    ("shapes.rs", include_str!("../../horizon-compat/src/shapes.rs")),
    ("sounds.rs", include_str!("../../horizon-compat/src/sounds.rs")),
    ("vanilla_sounds.rs", include_str!("../../horizon-compat/src/vanilla_sounds.rs")),
    ("vanilla_effects.rs", include_str!("../../horizon-compat/src/vanilla_effects.rs")),
    ("food.rs", include_str!("../../horizon-compat/src/food.rs")),
    ("item_use.rs", include_str!("../../horizon-compat/src/item_use.rs")),
    ("nbt.rs", include_str!("../../horizon-compat/src/nbt.rs")),
    ("ops.rs", include_str!("../../horizon-compat/src/ops.rs")),
    ("scan.rs", include_str!("../../horizon-compat/src/scan.rs")),
    ("glue.rs", include_str!("../../horizon-compat/src/glue.rs")),
    ("gui.rs", include_str!("../../horizon-compat/src/gui.rs")),
    ("screens.rs", include_str!("../../horizon-compat/src/screens.rs")),
    ("hand.rs", include_str!("../../horizon-compat/src/hand.rs")),
    ("events.rs", include_str!("../../horizon-compat/src/events.rs")),
    ("fake_players.rs", include_str!("../../horizon-compat/src/fake_players.rs")),
    ("jutil.rs", include_str!("../../horizon-compat/src/jutil.rs")),
    ("keys.rs", include_str!("../../horizon-compat/src/keys.rs")),
    ("input.rs", include_str!("../../horizon-compat/src/input.rs")),
    ("tooltips.rs", include_str!("../../horizon-compat/src/tooltips.rs")),
    ("damage.rs", include_str!("../../horizon-compat/src/damage.rs")),
    ("world_events.rs", include_str!("../../horizon-compat/src/world_events.rs")),
    ("datapack.rs", include_str!("../../horizon-compat/src/datapack.rs")),
    ("vanilla_entities.rs", include_str!("../../horizon-compat/src/vanilla_entities.rs")),
    ("made_entities.rs", include_str!("../../horizon-compat/src/made_entities.rs")),
    ("menus.rs", include_str!("../../horizon-compat/src/menus.rs")),
    ("widgets.rs", include_str!("../../horizon-compat/src/widgets.rs")),
    ("edit_box.rs", include_str!("../../horizon-compat/src/edit_box.rs")),
    ("level_env.rs", include_str!("../../horizon-compat/src/level_env.rs")),
    ("mixin.rs", include_str!("../../horizon-compat/src/mixin.rs")),
    ("neo.rs", include_str!("../../horizon-compat/src/neo.rs")),
    ("attachments.rs", include_str!("../../horizon-compat/src/attachments.rs")),
    ("attributes.rs", include_str!("../../horizon-compat/src/attributes.rs")),
    ("mob_effects.rs", include_str!("../../horizon-compat/src/mob_effects.rs")),
    ("vitals.rs", include_str!("../../horizon-compat/src/vitals.rs")),
    ("level_events.rs", include_str!("../../horizon-compat/src/level_events.rs")),
    ("enchantments.rs", include_str!("../../horizon-compat/src/enchantments.rs")),
    ("files.rs", include_str!("../../horizon-compat/src/files.rs")),
    ("resources.rs", include_str!("../../horizon-compat/src/resources.rs")),
    ("gson.rs", include_str!("../../horizon-compat/src/gson.rs")),
    ("network.rs", include_str!("../../horizon-compat/src/network.rs")),
    ("jlang.rs", include_str!("../../horizon-compat/src/jlang.rs")),
    ("jregex.rs", include_str!("../../horizon-compat/src/jregex.rs")),
    ("tags.rs", include_str!("../../horizon-compat/src/tags.rs")),
    ("render.rs", include_str!("../../horizon-compat/src/render.rs")),
    ("models.rs", include_str!("../../horizon-compat/src/models.rs")),
    ("entity_render.rs", include_str!("../../horizon-compat/src/entity_render.rs")),
    ("camera.rs", include_str!("../../horizon-compat/src/camera.rs")),
    ("vertices.rs", include_str!("../../horizon-compat/src/vertices.rs")),
    ("client_extensions.rs", include_str!("../../horizon-compat/src/client_extensions.rs")),
    ("options.rs", include_str!("../../horizon-compat/src/options.rs")),
    ("quads.rs", include_str!("../../horizon-compat/src/quads.rs")),
    ("vanilla_tags.rs", include_str!("../../horizon-compat/src/vanilla_tags.rs")),
    ("vanilla_classes.rs", include_str!("../../horizon-compat/src/vanilla_classes.rs")),
    ("value_providers.rs", include_str!("../../horizon-compat/src/value_providers.rs")),
    ("vanilla_tag_keys.rs", include_str!("../../horizon-compat/src/vanilla_tag_keys.rs")),
];

/// The registry crates the carried runtime uses, as the engine's `[dependencies]`: each must be
/// in Horizon's Cargo.lock, at a version that lock holds.
const ENGINE_DEPENDENCIES: &str = "serde_json = \"1.0.151\"\npng = \"0.18\"\nregex = { version = \"1\", default-features = false, features = [\"std\", \"unicode-perl\", \"unicode-case\"] }\n";

/// horizon-compat's modules for tools only (the port pack's), not carried: nothing carried uses them.
#[cfg(test)]
const TOOL_ONLY: [&str; 1] = ["convention.rs"];

/// The core's root inlined: without the installable entry points (inlined, `api` and `neo` are
/// the mapping's own modules), and declaring the mapping's modules.
fn inlined_core() -> String {
    let (head, rest) = CORE.split_once("// BEGIN ENGINE ENTRY POINTS").expect("core entry point markers");
    let (_, tail) = rest.split_once("// END ENGINE ENTRY POINTS").expect("core entry point markers");
    let mods: String = COMPAT
        .iter()
        .filter(|(n, _)| !matches!(*n, "host.rs" | "gc.rs"))
        .map(|(n, _)| format!("pub mod {};\n", n.trim_end_matches(".rs")))
        .collect();
    let (_, helpers) = COMPAT_ROOT.split_once("// BEGIN RUNTIME HELPERS").expect("compat runtime helper markers");
    let (helpers, _) = helpers.split_once("// END RUNTIME HELPERS").expect("compat runtime helper markers");
    let helpers = helpers.split_once('\n').map_or("", |(_, rest)| rest);
    // `Rt::is_a` asks the mapping's table of Minecraft's class families, `Rt::type_of` its
    // vanilla blocks' and items' classes.
    format!("{head}{tail}\n{mods}pub use api_common::{{api_is_a, vanilla_class}};\n{helpers}")
}

/// Writes `contents` unless the file already holds exactly that, so an unchanged engine keeps its
/// mtimes and Cargo does not rebuild it. Makes the file's folder when it is missing.
fn put(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    let (path, contents) = (path.as_ref(), contents.as_ref());
    if fs::read(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}

/// Writes `files` into `dir` ([`put`]: an unchanged file keeps its mtime) and removes whatever else
/// `dir` holds: a part or module an earlier translation wrote and this one doesn't.
fn sync_dir(dir: &Path, files: &[(String, String)]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (name, text) in files {
        put(dir.join(name), text).map_err(|e| e.to_string())?;
    }
    for e in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        if files.iter().any(|(name, _)| *name == *e.file_name().to_string_lossy()) {
            continue;
        }
        let gone = match e.file_type() {
            Ok(t) if t.is_dir() => fs::remove_dir_all(e.path()),
            _ => fs::remove_file(e.path()),
        };
        gone.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The size a split engine's code crates aim at ([`crate::split`]): `HORIZON_TRANSLATOR_CRATE_MB`
/// megabytes of Rust, or 8. rustc's memory grows with a crate's size, and Cargo builds as many of
/// them at once as it has jobs.
fn crate_bytes() -> usize {
    std::env::var("HORIZON_TRANSLATOR_CRATE_MB").ok().and_then(|v| v.parse::<usize>().ok()).filter(|&mb| mb > 0).unwrap_or(8) << 20
}

/// A split engine's runtime crate: horizon-compat-core's root, with the API mapping's modules
/// declared in it ([`inlined_core`]), and their sources.
fn write_runtime(dir: &Path, package: &str) -> Result<(), String> {
    put(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{ENGINE_DEPENDENCIES}"),
    )
    .map_err(|e| e.to_string())?;
    let core = inlined_core();
    let files: Vec<(String, String)> = std::iter::once(("lib.rs", core.as_str())).chain(COMPAT).map(|(name, text)| (name.to_owned(), text.replace("#![", "// #!["))).collect();
    sync_dir(&dir.join("src"), &files)
}

/// Block hooks the bundle wires: (hook name, glue function).
const BLOCK_HOOKS: [(&str, &str); 2] = [("tick_block", "tick_block"), ("random_tick", "random_tick")];

/// A block hook's host: the hook's level, which it may change, and its scheduled ticks (`level`
/// and `ticks` in scope).
fn level_host(krate: &str) -> String {
    format!(
        r#"    struct Host<'a, 'b, 'c> {{
        level: &'a mut crate::game::GameLevel<'b>,
        ticks: &'c mut horizon_world::level::Ticks,
    }}
    impl {krate}::compat::host::Host for Host<'_, '_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.level.native_state_text(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.level.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{krate}::compat::host::BlockFacts> {{
            self.level.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {krate}::compat::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.level.native_dimension_name().to_owned()
        }}
        fn light(&self, pos: [i32; 3]) -> (u8, u8) {{
            self.level.native_light_here(pos)
        }}
        fn neighbor_signal(&mut self, pos: [i32; 3]) -> i32 {{
            self.level.native_neighbor_signal_here(self.ticks, pos)
        }}
        fn schedule_block_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool {{
            self.level.native_schedule_block_tick(self.ticks, pos, block, delay)
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.level.native_fluid_name_in_tag(fluid, tag)
        }}
        fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {{
            self.level.native_set_state_text(self.ticks, pos, state, 3)
        }}
        fn play_sound(&mut self, s: &{krate}::compat::host::Sound) {{
            self.level.native_play_sound(&s.event, s.at, s.source, s.volume, s.pitch);
        }}
    }}
"#
    )
}

/// A read-only question's host: the level is only read (`level` in scope, a `&GameLevel`).
fn level_read_host(c: &str) -> String {
    format!(
        r#"    struct Host<'a, 'b> {{
        level: &'a crate::game::GameLevel<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.level.native_state_text(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.level.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.level.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.level.native_dimension_name().to_owned()
        }}
        fn light(&self, pos: [i32; 3]) -> (u8, u8) {{
            self.level.native_light_here(pos)
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.level.native_fluid_name_in_tag(fluid, tag)
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false // a question changes nothing
        }}
    }}
"#
    )
}

fn helper(id: &str, krate: &str, hook: &str, glue: &str) -> String {
    let host = level_host(krate);
    format!(
        r#"// Generated by horizon-mod-translator: Horizon's `{hook}` hook → the translated mod.
fn {id}_{hook}(
    level: &mut crate::game::GameLevel<'_>,
    ticks: &mut horizon_world::level::Ticks,
    pos: horizon_core::position::BlockPos,
    handled: &mut bool,
) {{
{host}    if *handled {{
        return;
    }}
    let p = [pos.x, pos.y, pos.z];
    let state = level.native_state_text(p);
    if !{krate}::ours(&state) {{
        return; // only the mod's own blocks
    }}
    let mut host = Host {{ level, ticks }};
    {krate}::hook_runtime("{glue}", |rt| {{
        if {krate}::compat::glue::{glue}(rt, &mut host, p, &state) {{
            *handled = true;
        }}
    }});
}}
"#
    )
}

/// Horizon's NBT (`horizon_world::nbt::Tag`) to the runtime's (`{c}::nbt::Nbt`) and back, as
/// nested functions of a helper.
fn nbt_conversions(c: &str) -> String {
    format!(
        r#"    use horizon_world::nbt::{{Compound, JavaString, Tag}};
    use {c}::nbt::Nbt;
    #[allow(dead_code)]
    fn to_nbt(tag: &Tag) -> Option<Nbt> {{
        Some(match tag {{
            Tag::Byte(b) => Nbt::Byte(*b),
            Tag::Short(s) => Nbt::Short(*s),
            Tag::Int(i) => Nbt::Int(*i),
            Tag::Long(l) => Nbt::Long(*l),
            Tag::Float(f) => Nbt::Float(f32::from_bits(*f)),
            Tag::Double(d) => Nbt::Double(f64::from_bits(*d)),
            Tag::String(s) => Nbt::String(s.to_string_lossy()),
            Tag::Compound(c) => Nbt::Compound(c.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect()),
            Tag::List(l) => Nbt::List(l.iter().filter_map(to_nbt).collect()),
            Tag::ByteArray(a) => Nbt::ByteArray(a.iter().map(|b| *b as i8).collect()),
            Tag::IntArray(a) => Nbt::IntArray(a.clone()),
            Tag::LongArray(a) => Nbt::LongArray(a.clone()),
            Tag::End => return None,
        }})
    }}
    #[allow(dead_code)]
    fn to_tag(n: &Nbt) -> Option<Tag> {{
        Some(match n {{
            Nbt::Byte(b) => Tag::Byte(*b),
            Nbt::Short(s) => Tag::Short(*s),
            Nbt::Int(i) => Tag::Int(*i),
            Nbt::Long(l) => Tag::Long(*l),
            Nbt::Float(f) => Tag::Float(f.to_bits()),
            Nbt::Double(d) => Tag::Double(d.to_bits()),
            Nbt::String(s) => Tag::String(JavaString::from(s.as_str())),
            Nbt::Compound(c) => Tag::Compound(c.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), to_tag(v)?))).collect::<Compound>()),
            Nbt::List(l) => Tag::List(l.iter().filter_map(to_tag).collect()),
            Nbt::ByteArray(a) => Tag::ByteArray(a.iter().map(|b| *b as u8).collect()),
            Nbt::IntArray(a) => Tag::IntArray(a.clone()),
            Nbt::LongArray(a) => Tag::LongArray(a.clone()),
        }})
    }}
"#
    )
}

/// A server-side host's `send_payload`: a mod payload to the players `to` names, each through the
/// game's mod message channel.
/// A server host's sounds and particles: for the players near them in the level the game holds.
fn server_effects(c: &str) -> String {
    format!(r#"        fn run_command(&mut self, run: &{c}::host::CommandRun) -> bool {{
            self.game.native_run_command_as(&run.line, run.player, run.permission, run.at, run.rot, run.silent)
        }}
        fn player_permission(&self, uuid: u128) -> Option<u8> {{
            let acting = self.game.native_acting_player().filter(|p| p.uuid == uuid);
            acting.or_else(|| self.game.native_server_players().into_iter().find(|p| p.uuid == uuid)).map(|p| p.permission)
        }}
        fn play_sound(&mut self, s: &{c}::host::Sound) {{
            self.game.native_level_sound(&s.event, s.at, s.source, s.volume, s.pitch);
        }}
        fn particles(&mut self, p: &{c}::host::Particles) {{
            let Some(count) = p.count else {{ return }};
            if let Some((options, state)) = self.game.native_particle_options(&p.kind, p.block.as_deref(), p.item.as_deref(), p.dust, p.at) {{
                self.game.native_level_particles(&options, state, p.at, p.delta, p.speed, count);
            }}
        }}
        // Any player on the server by their UUID, whoever acts (a mob's killer told of its death).
        fn tell_player(&mut self, uuid: u128, component: &str, overlay: bool) -> bool {{
            let Ok(json) = serde_json::from_str::<serde_json::Value>(component) else {{
                return false;
            }};
            let plain = json.get("text").or_else(|| json.get("translate")).and_then(|t| t.as_str()).unwrap_or("").to_owned();
            let line = horizon_client::text::Text::literal(plain).with_component(json);
            if overlay {{
                self.game.native_action_bar(uuid, line)
            }} else {{
                self.game.native_tell(uuid, line)
            }}
        }}
"#)
}

fn server_send(c: &str) -> String {
    format!(r#"        fn send_payload(&mut self, to: {c}::host::PayloadTarget, channel: &str, data: &[u8]) -> bool {{
            // A player by id goes straight out: inside a player's hook (`as_guest`) the server's
            // list of players is out of reach, and the message leaves at the tick's end anyway.
            if let {c}::host::PayloadTarget::Player(uuid) = to {{
                return self.game.native_net_send_to_player(uuid, channel, data).is_ok();
            }}
            let mut all: Vec<(u128, [f64; 3])> = self.game.native_server_players().into_iter().map(|p| (p.uuid, p.position)).collect();
            // Inside a player's hook only the acting player is in reach.
            if let Some(p) = self.game.native_acting_player()
                && !all.iter().any(|q| q.0 == p.uuid)
            {{
                all.push((p.uuid, p.position));
            }}
            let players: Vec<u128> = all
                .into_iter()
                .filter(|&(_, at)| match to {{
                    {c}::host::PayloadTarget::Server | {c}::host::PayloadTarget::Player(_) => false,
                    {c}::host::PayloadTarget::All => true,
                    {c}::host::PayloadTarget::Near {{ x, y, z, radius }} => {{
                        let d = [at[0] - x, at[1] - y, at[2] - z];
                        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= radius * radius
                    }}
                }})
                .map(|(uuid, _)| uuid)
                .collect();
            let mut sent = false;
            for p in players {{
                sent |= self.game.native_net_send_to_player(p, channel, data).is_ok();
            }}
            sent
        }}
"#)
}

/// The host every hook of the window's client gets: the client's world and its player (hands, what
/// it wears, the block it looks at, where it stands), the language, payloads to the server, sounds
/// and particles, the mod's own screen (opening and closing it, the keys and the pointer), and,
/// for a hook that draws, the frame's GUI batch. A hook that may change the game (the tick, a
/// payload, the use key) holds it mutably; a screen's drawing and input only read it, and what
/// they would change waits for the next tick ([`Host::read_only`]). Needs the helper's NBT
/// conversions (`to_nbt`).
fn client_host(c: &str, id: &str) -> String {
    let reads = world_reads("self.game.get()", &c);
    // The client's own player's attributes, effects, health and food (`native_attribute` of None),
    // where the game is lent.
    let attributes = format!(
        "{}{}{}{}",
        attribute_methods(c, "self.game.get_mut()"),
        effect_methods(c, "self.game.get_mut()"),
        vital_methods(c, "self.game.get_mut()"),
        level_event_methods("self.game.get_mut()")
    );
    format!(r#"    enum GameRef<'a, 'b> {{
        Mut(&'a mut crate::game::Game<'b>),
        Shared(&'a crate::game::Game<'b>),
    }}
    impl<'b> GameRef<'_, 'b> {{
        fn get(&self) -> &crate::game::Game<'b> {{
            match self {{
                GameRef::Mut(g) => &**g,
                GameRef::Shared(g) => *g,
            }}
        }}
        fn get_mut(&mut self) -> Option<&mut crate::game::Game<'b>> {{
            match self {{
                GameRef::Mut(g) => Some(&mut **g),
                GameRef::Shared(_) => None,
            }}
        }}
    }}
    struct Host<'a, 'b, 'c> {{
        game: GameRef<'a, 'b>,
        batch: Option<&'c mut horizon_client::hud::HudBatch>,
    }}
    impl {c}::host::Host for Host<'_, '_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.get().native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.get().native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.game.get().native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.game.get().native_dimension().to_owned()
        }}
{reads}{attributes}        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.game.get().native_fluid_name_in_tag(fluid, tag)
        }}
        fn fluid_flow(&self, pos: [i32; 3]) -> [f64; 3] {{
            self.game.get().native_fluid_flow_at(pos)
        }}
        fn item_is_block(&self, item: &str) -> bool {{
            self.game.get().native_item_facts(item).is_some_and(|(block, _)| block)
        }}
        fn item_stack_limit(&self, item: &str) -> Option<u32> {{
            self.game.get().native_item_facts(item).map(|(_, limit)| limit)
        }}
        fn item_max_damage(&self, item: &str) -> Option<u32> {{
            Some(self.game.get().native_item_max_damage(item))
        }}
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.get().mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
        }}
        fn outline_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            Some(self.game.get().native_outline_boxes(pos))
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn is_server(&self) -> bool {{
            false
        }}
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.get().native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
        fn send_payload(&mut self, to: {c}::host::PayloadTarget, channel: &str, data: &[u8]) -> bool {{
            to == {c}::host::PayloadTarget::Server && self.game.get_mut().is_some_and(|g| g.native_net_send_to_server(channel, data).is_ok())
        }}
        fn play_sound(&mut self, s: &{c}::host::Sound) {{
            if let Some(g) = self.game.get_mut() {{
                g.native_play_local_sound(&s.event, s.at, s.volume, s.pitch, 0);
            }}
        }}
        // A mod's own sound instance, which it goes on changing (`TickableSoundInstance`).
        fn start_sound(&mut self, s: &{c}::host::InstanceSound) -> Option<u64> {{
            let sound = crate::game::native_hooks::NativeSound {{
                event: s.sound.event.clone(),
                at: s.sound.at,
                source: s.sound.source,
                volume: s.sound.volume,
                pitch: s.sound.pitch,
                looping: s.looping,
                delay: s.delay,
                start_delay: s.start_delay,
                linear: s.linear,
                relative: s.relative,
                can_start_silent: s.can_start_silent,
            }};
            self.game.get_mut()?.native_start_sound(&sound)
        }}
        // A host that only reads (a screen's drawing) changes nothing: the sound goes on as it was.
        fn update_sound(&mut self, id: u64, at: [f64; 3], volume: f32, pitch: f32) -> bool {{
            match self.game.get_mut() {{
                Some(g) => g.native_update_sound(id, at, volume, pitch),
                None => true,
            }}
        }}
        fn stop_sound(&mut self, id: u64) {{
            if let Some(g) = self.game.get_mut() {{
                g.native_stop_sound(id);
            }}
        }}
        fn particles(&mut self, p: &{c}::host::Particles) {{
            if p.count.is_some() {{
                return;
            }}
            let Some(g) = self.game.get_mut() else {{ return }};
            if let Some((options, _)) = g.native_particle_options(&p.kind, p.block.as_deref(), p.item.as_deref(), p.dust, p.at) {{
                g.native_add_particle(&options, p.at, p.delta, p.force, p.always);
            }}
        }}
        fn player_sneaking(&self) -> bool {{
            self.game.get().has_shift_down()
        }}
        fn player_creative(&self) -> bool {{
            self.game.get().native_local_player().is_some_and(|p| p.3)
        }}
        fn player_place(&self) -> Option<([f64; 3], f32, f32)> {{
            self.game.get().native_local_player().map(|(at, yaw, pitch, _)| (at, yaw, pitch))
        }}
        fn client_player(&self) -> Option<(String, u8)> {{
            self.game.get().native_client_player()
        }}
        // A client command's answer: a line in the window's chat, the whole component as the
        // window draws one (translations, siblings, styles).
        fn command_feedback(&mut self, component: &str) {{
            let Ok(json) = serde_json::from_str::<serde_json::Value>(component) else {{
                return;
            }};
            let text = horizon_client::component::to_text(&json, &mut |s| horizon_client::text::Text::literal(s));
            if let Some(game) = self.game.get_mut() {{
                game.native_feedback(text);
            }}
        }}
        // A message for the window's player (`LocalPlayer.displayClientMessage`): in the chat, or
        // over the hotbar.
        fn player_message(&mut self, component: &str, overlay: bool) {{
            let Ok(json) = serde_json::from_str::<serde_json::Value>(component) else {{
                return;
            }};
            let text = horizon_client::component::to_text(&json, &mut |s| horizon_client::text::Text::literal(s));
            if let Some(game) = self.game.get_mut() {{
                game.native_player_message(text, overlay);
            }}
        }}
        fn player_pov_hit(&self, fluids: u8) -> Option<([i32; 3], u8, [f64; 3])> {{
            // A face's step as a direction ordinal: down, up, north, south, west, east.
            let face = |f: [i32; 3]| match f {{
                [0, -1, 0] => 0,
                [0, 1, 0] => 1,
                [0, 0, -1] => 2,
                [0, 0, 1] => 3,
                [-1, 0, 0] => 4,
                _ => 5,
            }};
            self.game.get().native_player_pov_hit(fluids).map(|(pos, f, at)| (pos, face(f), at))
        }}
        fn player_hand(&self, hand: u8) -> Option<{c}::host::HandStack> {{
            let hand = if hand == 1 {{ horizon_client::first_person::Hand::Off }} else {{ horizon_client::first_person::Hand::Main }};
            Some(self.held(self.game.get().native_hand_stack(hand)?))
        }}
        fn player_worn(&self, slot: u8) -> Option<{c}::host::HandStack> {{
            Some(self.held(self.game.get().native_worn(usize::from(slot))?))
        }}
        // Every slot reads (a mod's menu shows them); none changes here: the server's sync does.
        fn player_selected(&self) -> u8 {{
            self.game.get().native_selected_slot() as u8
        }}
        fn player_inventory(&self) -> bool {{
            true
        }}
        fn player_slot(&self, slot: u8) -> Option<{c}::host::HandStack> {{
            Some(self.held(self.game.get().native_slot_stack(usize::from(slot))?))
        }}
        fn set_player_slot(&mut self, _slot: u8, _stack: Option<&{c}::host::HandStack>) -> bool {{
            false
        }}
        fn clipboard(&self) -> String {{
            self.game.get().native_clipboard()
        }}
        fn set_clipboard(&mut self, text: &str) {{
            if let Some(g) = self.game.get_mut() {{
                g.native_set_clipboard(text);
            }}
        }}
        fn update_player_slot(&mut self, _slot: u8, _count: u32, _damage: u32, _components: &[(String, Option<{c}::nbt::Nbt>)]) -> bool {{
            false
        }}
        fn translate(&self, key: &str) -> Option<String> {{
            horizon_client::lang::has(key).then(|| horizon_client::lang::get(key))
        }}
        // The mod's own screen: one at a time, over the world (`native_mod_screen_open`).
        fn open_screen(&mut self) -> bool {{
            self.game.get_mut().is_some_and(|g| g.native_mod_screen_open("{id}"))
        }}
        fn close_screen(&mut self) {{
            if let Some(g) = self.game.get_mut() {{
                g.native_mod_screen_close("{id}");
            }}
        }}
        fn screen_open(&self) -> bool {{
            self.game.get().native_mod_screen() == Some("{id}")
        }}
        fn mapping(&self, name: &str) -> Option<(String, bool)> {{
            Some(self.game.get().native_mapping_binding(name))
        }}
        fn key_held(&self, key: &str) -> bool {{
            self.game.get().native_key_held(key)
        }}
        // A mod mapping's key as the player chose it on the bundle's page in Options > Mods.
        fn mod_key(&self, name: &str, default: &str) -> String {{
            self.game.get().native_mod_key("{id}", name, default)
        }}
        fn playing(&self) -> bool {{
            self.game.get().native_playing()
        }}
        fn cursor(&self) -> [f32; 2] {{
            self.game.get().native_cursor()
        }}
        fn set_cursor(&mut self, at: [f32; 2]) {{
            if let Some(g) = self.game.get_mut() {{
                g.native_set_cursor(at);
            }}
        }}
        fn read_only(&self) -> bool {{
            matches!(self.game, GameRef::Shared(_))
        }}
        fn gui_size(&self) -> [i32; 2] {{
            match &self.batch {{
                Some(b) => [b.width as i32, b.height as i32],
                None => {{
                    let ([w, h], _) = self.game.get().native_gui_size();
                    [w as i32, h as i32]
                }}
            }}
        }}
        fn gui_scale(&self) -> f64 {{
            match &self.batch {{
                Some(b) => f64::from(b.scale),
                None => f64::from(self.game.get().native_gui_size().1),
            }}
        }}
        fn gui_fill(&mut self, quad: [[f32; 2]; 4], argb: u32) {{
            let game = self.game.get();
            if let Some(batch) = self.batch.as_deref_mut() {{
                game.native_quad(batch, quad, argb);
            }}
        }}
        fn gui_blit(&mut self, texture: &str, uv: [f32; 4], rect: [f32; 4], tint: u32) -> bool {{
            // `ns:textures/<path>.png`, or an atlas sprite `sprite:ns:<path>`, as the GUI atlas
            // names it: `<path>` for vanilla's, `ns:<path>` for a mod's.
            let name = match texture.strip_prefix("sprite:") {{
                Some(sprite) => sprite.to_owned(),
                None => {{
                    let (ns, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
                    let path = path.strip_prefix("textures/").unwrap_or(path);
                    format!("{{ns}}:{{}}", path.strip_suffix(".png").unwrap_or(path))
                }}
            }};
            let name = name.strip_prefix("minecraft:").unwrap_or(&name).to_owned();
            let game = self.game.get();
            let Some([w, h]) = game.native_sprite_size(&name) else {{
                return false;
            }};
            let Some(batch) = self.batch.as_deref_mut() else {{
                return false;
            }};
            let start = batch.vertices.len();
            let src = [uv[0] * w, uv[1] * h, (uv[2] - uv[0]) * w, (uv[3] - uv[1]) * h];
            let dst = [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]];
            let drew = game.native_sprite(batch, &name, src, dst);
            if drew && tint != u32::MAX {{
                self.tint_since(start, tint);
            }}
            drew
        }}
        fn gui_text(&mut self, runs: &[{c}::host::TextRun], at: [f32; 2], scale: f32, argb: u32, shadow: bool) {{
            let game = self.game.get();
            let Some(batch) = self.batch.as_deref_mut() else {{
                return;
            }};
            let start = batch.vertices.len();
            let mut x = at[0];
            for run in runs {{
                let rgb = run.color.unwrap_or(argb & 0xFF_FFFF);
                let width = if run.bold {{
                    // Vanilla's bold: each glyph drawn again a pixel to the right, a pixel wider.
                    let mut w = 0.0;
                    for ch in run.text.chars() {{
                        let one = ch.to_string();
                        game.native_text(batch, &one, [x + w, at[1]], rgb, shadow);
                        game.native_text(batch, &one, [x + w + 1.0, at[1]], rgb, shadow);
                        w += game.native_text_width(&one) + 1.0;
                    }}
                    w
                }} else {{
                    game.native_text(batch, &run.text, [x, at[1]], rgb, shadow);
                    game.native_text_width(&run.text)
                }};
                // Vanilla's line effects: a pixel high, under the glyphs or through them.
                if run.underlined {{
                    game.native_fill(batch, [x - 1.0, at[1] + 8.0, width + 1.0, 1.0], 0xFF00_0000 | rgb);
                }}
                if run.strikethrough {{
                    game.native_fill(batch, [x - 1.0, at[1] + 3.5, width + 1.0, 1.0], 0xFF00_0000 | rgb);
                }}
                x += width;
            }}
            // `Font.adjustColor`: an alpha below 4 is opaque.
            let alpha = argb >> 24;
            if (4..0xFF).contains(&alpha) {{
                self.tint_since(start, alpha << 24 | 0xFF_FFFF);
            }}
            if scale != 1.0
                && let Some(batch) = self.batch.as_deref_mut()
            {{
                batch.scale_since(start, at[0], at[1], scale, scale);
            }}
        }}
        fn gui_char_width(&self, c: char, bold: bool) -> f32 {{
            self.game.get().native_text_width(&c.to_string()) + if bold {{ 1.0 }} else {{ 0.0 }}
        }}
        fn gui_item(&mut self, item: &str, at: [f32; 2], size: f32) -> bool {{
            let game = self.game.get();
            match self.batch.as_deref_mut() {{
                Some(batch) => game.native_item(batch, item, at, size),
                None => false,
            }}
        }}
    }}
    impl Host<'_, '_, '_> {{
        /// A stack of the player's with its mod data and enchantments, as the runtime holds a hand's.
        fn held(&self, stack: &horizon_client::items::ItemStack) -> {c}::host::HandStack {{
            let game = self.game.get();
            let mut components: Vec<_> = game
                .native_registered_components()
                .into_iter()
                .filter_map(|name| {{
                    let tag = game.native_component(stack, name).ok()??;
                    Some((name.to_owned(), to_nbt(tag)?))
                }})
                .collect();
            for (name, tag) in game.native_vanilla_components(stack) {{
                components.extend(to_nbt(&tag).map(|n| (name.to_owned(), n)));
            }}
            {c}::host::HandStack {{
                item: stack.item.to_string(),
                count: stack.count,
                damage: stack.damage,
                max_damage: game.native_item_max_damage(&stack.item),
                components,
            }}
        }}
        /// The colours drawn since vertex `start` times `tint` (`0xAARRGGBB`); a vertex's colour is
        /// `rgba`'s, alpha in the top byte, then blue, green and red.
        fn tint_since(&mut self, start: usize, tint: u32) {{
            let [ta, tr, tg, tb] = tint.to_be_bytes();
            let times = |c: u32, shift: u32, t: u8| ((c >> shift & 0xFF) * u32::from(t) / 255) << shift;
            let Some(batch) = self.batch.as_deref_mut() else {{ return }};
            for v in &mut batch.vertices[start..] {{
                v.color = times(v.color, 24, ta) | times(v.color, 16, tb) | times(v.color, 8, tg) | times(v.color, 0, tr);
            }}
        }}
    }}
"#)
}

/// The client's hooks: drawing the mod's block entities with its renderers, the update tags the
/// server sends for them, the client tick (animation clocks) and the payloads the server sends.
fn client_helpers(id: &str, krate: &str) -> Vec<(&'static str, String)> {
    let c = format!("{krate}::compat");
    let conversions = nbt_conversions(&c);
    let client_host = client_host(&c, id);
    // The host a client's draws get: the game, and the frame's pose stack and geometry.
    let reads = world_reads("self.game", &c);
    let draw_host = format!(r#"    struct Host<'a, 'b, 'c> {{
        game: &'a crate::game::Game<'b>,
        draw: &'c mut crate::game::native_block_entity_draw::BlockEntityDraw,
    }}
    impl {c}::host::Host for Host<'_, '_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.game.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.game.native_dimension().to_owned()
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.game.native_fluid_name_in_tag(fluid, tag)
        }}
        fn fluid_flow(&self, pos: [i32; 3]) -> [f64; 3] {{
            self.game.native_fluid_flow_at(pos)
        }}
        fn item_is_block(&self, item: &str) -> bool {{
            self.game.native_item_facts(item).is_some_and(|(block, _)| block)
        }}
        fn item_stack_limit(&self, item: &str) -> Option<u32> {{
            self.game.native_item_facts(item).map(|(_, limit)| limit)
        }}
        fn item_max_damage(&self, item: &str) -> Option<u32> {{
            Some(self.game.native_item_max_damage(item))
        }}
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
        }}
        fn outline_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            Some(self.game.native_outline_boxes(pos))
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn is_server(&self) -> bool {{
            false
        }}
        // Which items exist (an update tag's stacks are read through it: an item the client
        // didn't know would read as an empty stack) and what a tag holds.
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
        fn game_time(&self) -> i64 {{
            self.draw.game_time
        }}
        // The window's own player, as a renderer reads it (`Minecraft.player`'s position, look
        // and sneaking).
        fn player_place(&self) -> Option<([f64; 3], f32, f32)> {{
            self.game.native_local_player().map(|(at, yaw, pitch, _)| (at, yaw, pitch))
        }}
        fn player_sneaking(&self) -> bool {{
            self.game.has_shift_down()
        }}
{reads}        fn partial_tick(&self) -> f32 {{
            self.draw.partial
        }}
        fn draw_item(&mut self, item: &str, display: u8, pose: &[f32; 16], light: u32, seed: i32) -> bool {{
            use horizon_assets::item_model::DisplayContext;
            let saved = self.draw.light;
            if light != 0 {{
                let (block, sky) = (light & 0xFFFF, light >> 16 & 0xFFFF);
                self.draw.light = sky.min(0xF0) | block.min(0xF0) << 8;
            }}
            self.draw.push();
            self.draw.transform(*pose);
            let stack = horizon_client::items::ItemStack::new(item, 1);
            let display = DisplayContext::ALL.get(usize::from(display)).copied().unwrap_or(DisplayContext::Fixed);
            let drew = self.game.native_draw_item(self.draw, &stack, display, None, seed);
            self.draw.pop();
            self.draw.light = saved;
            drew
        }}
        fn draw_model(&mut self, model: &{c}::host::Model, pose: &[f32; 16], color: u32, light: u32) -> bool {{
            self.model(model, pose, color, light, None)
        }}
        // The sprites `native_draw_faces` finds (it leaves out a face whose sprite isn't there).
        fn has_sprite(&self, name: &str) -> bool {{
            self.game.gui.atlas.sprites.contains_key(name.trim_start_matches("minecraft:"))
        }}
        // A model's faces as the game bakes them, for the mod to read, change and build its own
        // models from (`BakedModel.getQuads`).
        fn model_quads(&self, model: &{c}::host::Model) -> Option<std::sync::Arc<[{c}::host::Quad]>> {{
            let faces = match model {{
                {c}::host::Model::State(text) => {{
                    let (block, props) = {c}::host::split_state(text);
                    self.game.native_block_model_faces(block, &props)?
                }}
                {c}::host::Model::Named(name) => self.game.native_named_model_faces(name)?,
                {c}::host::Model::Quads(quads) => return Some(quads.clone()),
            }};
            use horizon_assets::model::Direction as D;
            let side = |d: D| match d {{ D::Down => 0u8, D::Up => 1, D::North => 2, D::South => 3, D::West => 4, D::East => 5 }};
            Some(faces.into_iter().map(|(q, sprite)| {c}::host::Quad {{
                positions: q.positions,
                uv: q.uv,
                sprite,
                facing: side(q.facing),
                cull: q.cull.map(side),
                tint_index: q.tint_index,
                shade: true,
                light_emission: q.light_emission,
                colors: [u32::MAX; 4],
            }}).collect())
        }}
        fn model_particle(&self, model: &{c}::host::Model) -> Option<String> {{
            match model {{
                {c}::host::Model::Named(name) => self.game.native_named_model_particle(name),
                _ => None,
            }}
        }}
        fn draw_model_shifted(&mut self, model: &{c}::host::Model, pose: &[f32; 16], color: u32, light: u32, shift: &{c}::host::UvShift) -> bool {{
            use crate::game::native_block_entity_draw::SpriteShift;
            let how = match shift.kind {{
                {c}::host::UvShiftKind::Retarget => SpriteShift::Retarget,
                {c}::host::UvShiftKind::Scroll {{ u, v }} => SpriteShift::Scroll {{ u, v }},
                {c}::host::UvShiftKind::Sheet {{ u, v, size }} => SpriteShift::Sheet {{ u, v, size }},
            }};
            self.model(model, pose, color, light, Some((&shift.from, &shift.to, how)))
        }}
    }}
    impl Host<'_, '_, '_> {{
        /// `model` under `pose`, lit by `light` (`LightTexture.pack`; 0 for the block's own), its
        /// faces on one sprite textured from another as `shift` says.
        fn model(&mut self, model: &{c}::host::Model, pose: &[f32; 16], color: u32, light: u32, shift: Option<(&str, &str, crate::game::native_block_entity_draw::SpriteShift)>) -> bool {{
            let saved = self.draw.light;
            if light != 0 {{
                // LightTexture.pack (block << 4 | sky << 20) as Horizon's (sky * 16 | block * 16 << 8).
                let (block, sky) = (light & 0xFFFF, light >> 16 & 0xFFFF);
                self.draw.light = sky.min(0xF0) | block.min(0xF0) << 8;
            }}
            self.draw.push();
            self.draw.transform(*pose);
            let drew = match model {{
                {c}::host::Model::State(text) => {{
                    let (block, props) = {c}::host::split_state(text);
                    self.game.native_draw_block(self.draw, block, &props)
                }}
                {c}::host::Model::Named(name) => self.game.native_draw_model_shifted(self.draw, name, color & 0xFF_FFFF, shift),
                {c}::host::Model::Quads(quads) => {{
                    use horizon_assets::model::Direction as D;
                    const SIDES: [D; 6] = [D::Down, D::Up, D::North, D::South, D::West, D::East];
                    let faces = quads.iter().map(|q| crate::game::native_block_entity_draw::NativeFace {{
                        positions: q.positions,
                        uv: q.uv,
                        sprite: &q.sprite,
                        facing: SIDES[usize::from(q.facing.min(5))],
                        light_emission: q.light_emission,
                        colors: q.colors,
                    }});
                    self.game.native_draw_faces(self.draw, faces, color & 0xFF_FFFF, shift)
                }}
            }};
            self.draw.pop();
            self.draw.light = saved;
            drew
        }}
    }}
"#);
    vec![
        ("block_entity_submit", format!(r#"// Generated by horizon-mod-translator: the translated mod's block entity renderer, once a frame.
fn {id}_block_entity_submit(
    game: &crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &horizon_client::containers::ModBlockEntity,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
) {{
{conversions}    use std::collections::BTreeMap;
    if !{krate}::ours(&entity.id) {{
        return;
    }}
{draw_host}    let state = game.native_state_text_at(pos);
    let nbt: BTreeMap<String, Nbt> = entity.nbt.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    // Horizon's light (sky * 16 | block * 16 << 8) as LightTexture.pack.
    let light = ((draw.light >> 8 & 0xFF) | (draw.light & 0xFF) << 16) as i32;
    let mut host = Host {{ game, draw }};
    {krate}::hook_runtime("block_entity_submit", |rt| {c}::glue::render_block_entity(rt, &mut host, pos, &state, &nbt, light));
}}
"#)),
        ("level_submit", format!(r#"// Generated by horizon-mod-translator: the translated mod's own entities, each drawn by the
// renderer its mod registered, once a frame.
fn {id}_level_submit(
    game: &crate::game::Game<'_>,
    camera: [f64; 3],
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
) {{
{draw_host}    let mut host = Host {{ game, draw }};
    {krate}::hook_runtime("level_submit", |rt| {c}::glue::render_mod_entities(rt, &mut host, camera));
}}
"#)),
        ("block_entity_update_tag", format!(r#"// Generated by horizon-mod-translator: what players are sent of the translated mod's block
// entity (`getUpdateTag`); its saved data when its object isn't live on this thread.
fn {id}_block_entity_update_tag(
    pos: [i32; 3],
    entity: &horizon_client::containers::ModBlockEntity,
    tag: &mut Option<horizon_world::nbt::Compound>,
) {{
{conversions}    if tag.is_some() || !{krate}::ours(&entity.id) {{
        return;
    }}
    let live = {krate}::hook_runtime("block_entity_update_tag", |rt| {c}::glue::block_entity_update_tag(rt, pos));
    *tag = Some(match live {{
        Some(nbt) => nbt.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), to_tag(v)?))).collect::<Compound>(),
        None => entity.nbt.clone(),
    }});
}}
"#)),
        ("component_types", format!(r#"// Generated by horizon-mod-translator: the translated mod's item data that persists (its component
// types built with a codec), registered as native components so the game's stacks carry it.
fn {id}_component_types(types: &mut Vec<String>) {{
    for name in {krate}::with_probe_runtime(|rt| {c}::components::persistent_types(rt)) {{
        if !types.contains(&name) {{
            types.push(name);
        }}
    }}
}}
"#)),
        ("content", format!(r#"// Generated by horizon-mod-translator: every block and item the translated mod's registration
// makes, declared as content files would (states, default state, strength, sounds, light, map
// colour), replacing what the bundle could read from bytecode alone.
fn {id}_content(content: &mut horizon_mods::Content) {{
    // The mod's own worldgen types (its placement modifiers, features and biome modifiers), which
    // the world generator asks for by id when a placed or configured feature names one, and the
    // content root when a biome modifier file does: read with the mod's codec and run by its code,
    // on the generator's region and random stream, or on the root's biomes.
    {{
        use horizon_world::worldgen::foreign::{{BiomeModifier, Biomes, Feature, Handler, Placement, Random, StepFeatures, World}};
        struct Draws<'a>(&'a mut dyn Random);
        impl {krate}::compat::host::HostRandom for Draws<'_> {{
            fn next_int(&mut self) -> i32 {{ self.0.next_int() }}
            fn next_int_bound(&mut self, bound: i32) -> i32 {{ self.0.next_int_bound(bound) }}
            fn next_long(&mut self) -> i64 {{ self.0.next_long() }}
            fn next_boolean(&mut self) -> bool {{ self.0.next_boolean() }}
            fn next_float(&mut self) -> f32 {{ self.0.next_float() }}
            fn next_double(&mut self) -> f64 {{ self.0.next_double() }}
            fn next_gaussian(&mut self) -> f64 {{ self.0.next_gaussian() }}
        }}
        // The generator's region as the host the mod's code reads.
        struct Region<'a> {{
            world: std::cell::RefCell<&'a mut dyn World>,
            random: Draws<'a>,
        }}
        impl {krate}::compat::host::Host for Region<'_> {{
            fn block_state(&self, pos: [i32; 3]) -> String {{
                self.world.borrow_mut().block(pos).unwrap_or_else(|| "minecraft:air".to_owned())
            }}
            fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {{
                self.world.get_mut().set_block(pos, state, 3)
            }}
            fn set_section_state(&mut self, pos: [i32; 3], state: &str) -> bool {{
                self.world.get_mut().set_section_block(pos, state)
            }}
            fn can_write(&self, pos: [i32; 3]) -> bool {{
                self.world.borrow_mut().can_write(pos)
            }}
            fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
                self.world.borrow_mut().named_block_in_tag(block, tag)
            }}
            fn world_height(&self) -> (i32, i32) {{
                self.world.borrow_mut().y_range().map_or((-64, 384), |(low, high)| (low, high - low + 1))
            }}
            fn random(&mut self) -> Option<&mut dyn {krate}::compat::host::HostRandom> {{
                Some(&mut self.random)
            }}
            fn height(&mut self, heightmap: &str, x: i32, z: i32) -> Option<i32> {{
                self.world.get_mut().height(heightmap, x, z)
            }}
            fn world_seed(&self) -> Option<i64> {{
                self.world.borrow_mut().seed()
            }}
            fn biome(&self, pos: [i32; 3]) -> Option<String> {{
                self.world.borrow_mut().biome(pos)
            }}
            fn biome_in_tag(&self, biome: &str, tag: &str) -> bool {{
                self.world.borrow_mut().biome_in_tag(biome, tag)
            }}
            fn place_feature(&mut self, json: &str, pos: [i32; 3]) -> Result<bool, String> {{
                self.world.get_mut().place_feature(json, pos)
            }}
        }}
        // The content root's biomes, as a biome modifier's host: holder sets' members, biome tags,
        // a biome's data.
        struct Root<'a>(&'a dyn Biomes);
        impl {krate}::compat::host::Host for Root<'_> {{
            fn block_state(&self, _pos: [i32; 3]) -> String {{
                "minecraft:air".to_owned()
            }}
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
                false
            }}
            fn holder_set(&self, registry: &str, set: &str) -> Option<Vec<String>> {{
                self.0.resolve(registry, set).ok()
            }}
            fn biome_json(&self, id: &str) -> Option<String> {{
                self.0.biome(id)
            }}
            fn biome_in_tag(&self, biome: &str, tag: &str) -> bool {{
                // The tag as a holder set's JSON: a string of `#` and the tag's name.
                let set = format!("{{q}}#{{tag}}{{q}}", q = '"');
                self.0.resolve("biome", &set).is_ok_and(|members| members.iter().any(|m| m == biome))
            }}
        }}
        // A call's outermost entry, where garbage may go: what the generator holds of the mod's
        // (the modifiers and configured features it read) is the runtime's own, kept by its text.
        struct ModPlacement({krate}::compat::V);
        impl Placement for ModPlacement {{
            fn positions(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<Vec<[i32; 3]>, String> {{
                let mut region = Region {{ world: std::cell::RefCell::new(world), random: Draws(random) }};
                {krate}::with_worldgen_runtime(|rt| {{
                    rt.safe_point();
                    rt.with_host(&mut region, |rt| {c}::worldgen::positions(rt, self.0, origin))
                }})
                .unwrap_or_else(|| Err("the mod's worldgen code is already running on this thread".to_owned()))
            }}
        }}
        struct ModBiomeModifier({krate}::compat::V);
        impl BiomeModifier for ModBiomeModifier {{
            fn modify(&self, biomes: &dyn Biomes, biome: &str, phase: usize, features: &mut StepFeatures) -> Result<(), String> {{
                let mut root = Root(biomes);
                {krate}::with_worldgen_runtime(|rt| {{
                    rt.safe_point();
                    rt.with_host(&mut root, |rt| {c}::worldgen::modify_biome(rt, self.0, biome, phase, features))
                }})
                .unwrap_or_else(|| Err("the mod's worldgen code is already running on this thread".to_owned()))
            }}
        }}
        struct ModFeature({krate}::compat::V);
        impl Feature for ModFeature {{
            fn place(&self, world: &mut dyn World, random: &mut dyn Random, origin: [i32; 3]) -> Result<bool, String> {{
                let mut region = Region {{ world: std::cell::RefCell::new(world), random: Draws(random) }};
                {krate}::with_worldgen_runtime(|rt| {{
                    rt.safe_point();
                    rt.with_host(&mut region, |rt| {c}::worldgen::place(rt, self.0, origin))
                }})
                .unwrap_or_else(|| Err("the mod's worldgen code is already running on this thread".to_owned()))
            }}
        }}
        struct Worldgen;
        impl Handler for Worldgen {{
            fn placement(&self, kind: &str, json: &str) -> Option<Result<std::sync::Arc<dyn Placement>, String>> {{
                // Only the engine's own namespaces: a type of another mod's isn't this one's to read.
                if !{krate}::ours(kind) {{
                    return None;
                }}
                let read = {krate}::with_worldgen_runtime(|rt| {c}::worldgen::placement(rt, kind, json)).flatten()?;
                Some(read.map(|v| std::sync::Arc::new(ModPlacement(v)) as std::sync::Arc<dyn Placement>))
            }}
            fn feature(&self, kind: &str, json: &str) -> Option<Result<std::sync::Arc<dyn Feature>, String>> {{
                if !{krate}::ours(kind) {{
                    return None;
                }}
                let read = {krate}::with_worldgen_runtime(|rt| {c}::worldgen::feature(rt, kind, json)).flatten()?;
                Some(read.map(|v| std::sync::Arc::new(ModFeature(v)) as std::sync::Arc<dyn Feature>))
            }}
            fn biome_modifier(&self, kind: &str, json: &str) -> Option<Result<std::sync::Arc<dyn BiomeModifier>, String>> {{
                if !{krate}::ours(kind) {{
                    return None;
                }}
                let read = {krate}::with_worldgen_runtime(|rt| {c}::worldgen::biome_modifier(rt, kind, json)).flatten()?;
                Some(read.map(|v| std::sync::Arc::new(ModBiomeModifier(v)) as std::sync::Arc<dyn BiomeModifier>))
            }}
            fn survival_blocks(&self) -> Vec<String> {{
                {krate}::with_probe_runtime(|rt| {c}::blocks::survival_blocks(rt))
            }}
            fn survives(&self, world: &mut dyn World, state: &str, position: [i32; 3]) -> Option<Result<bool, String>> {{
                // A survival question draws nothing.
                struct Still;
                impl Random for Still {{
                    fn next_int(&mut self) -> i32 {{ 0 }}
                    fn next_int_bound(&mut self, _bound: i32) -> i32 {{ 0 }}
                    fn next_long(&mut self) -> i64 {{ 0 }}
                    fn next_boolean(&mut self) -> bool {{ false }}
                    fn next_float(&mut self) -> f32 {{ 0.0 }}
                    fn next_double(&mut self) -> f64 {{ 0.0 }}
                    fn next_gaussian(&mut self) -> f64 {{ 0.0 }}
                }}
                if !{krate}::ours(state) {{
                    return None;
                }}
                let mut still = Still;
                let mut region = Region {{ world: std::cell::RefCell::new(world), random: Draws(&mut still) }};
                {krate}::with_worldgen_query(|rt| rt.with_host(&mut region, |rt| {c}::worldgen::survives(rt, state, position))).flatten().map(Ok)
            }}
        }}
        horizon_world::worldgen::foreign::register({id:?}, std::sync::Arc::new(Worldgen));
    }}
    // The content mod of one of the engine's mods (a mod and the addons translated with it): what
    // that mod registers, its namespace's.
    let Some(ns) = content.namespaces.first().cloned().filter(|n| {krate}::NAMESPACES.contains(&n.as_str())) else {{
        return;
    }};
    let mine = |id: &str| id.split_once(':').is_some_and(|(n, _)| n == ns);
    let (blocks, items, tabs, effects) = {krate}::with_probe_runtime(|rt| ({c}::blocks::block_declarations(rt), {c}::blocks::item_declarations(rt), {c}::creative::tabs(rt), {c}::mob_effects::effect_declarations(rt)));
    let (mut declared, mut refused) = (0usize, 0usize);
    for (id, json) in blocks.iter().filter(|(id, _)| mine(id)) {{
        match content.declare_block(id, json) {{
            Ok(()) => declared += 1,
            Err(e) => {{
                refused += 1;
                log::warn!("{{ns}} (translated): block not declared: {{e}}");
            }}
        }}
    }}
    for (id, json) in items.iter().filter(|(id, _)| mine(id)) {{
        match content.declare_item(id, json) {{
            Ok(()) => declared += 1,
            Err(e) => {{
                refused += 1;
                log::warn!("{{ns}} (translated): item not declared: {{e}}");
            }}
        }}
    }}
    // Its mob effects: the game runs their code through the effect hooks.
    let mut effect_count = 0usize;
    for (id, category, color) in effects.iter().filter(|(id, ..)| mine(id)) {{
        let category = match category.as_str() {{
            "harmful" => horizon_mods::content::EffectCategory::Harmful,
            "neutral" => horizon_mods::content::EffectCategory::Neutral,
            _ => horizon_mods::content::EffectCategory::Beneficial,
        }};
        match content.declare_effect(id, category, *color) {{
            Ok(()) => effect_count += 1,
            Err(e) => log::warn!("{{ns}} (translated): mob effect not declared: {{e}}"),
        }}
    }}
    // Its creative tabs, as its own `displayItems` code fills them.
    let tabs: Vec<_> = tabs.into_iter().filter(|t| mine(&t.id)).collect();
    let tab_count = tabs.len();
    for tab in tabs {{
        let tab = horizon_mods::content::CreativeTab {{ id: tab.id, title: tab.title, icon: tab.icon, items: tab.items }};
        match content.creative_tabs.iter_mut().find(|t| t.id == tab.id) {{
            Some(old) => *old = tab,
            None => content.creative_tabs.push(tab),
        }}
    }}
    log::info!("{{ns}} (translated): {{declared}} blocks and items declared from its registration, {{refused}} refused, {{effect_count}} mob effects, {{tab_count}} creative tabs");
}}
"#)),
        ("content_block", format!(r#"// Generated by horizon-mod-translator: what draws the translated mod's blocks, by its own code
// (`getRenderShape`, the models its client swaps in), read once when its content is. States are
// asked one property at a time from the default: one property may decide (a sliding door is drawn
// by its renderer only while open).
fn {id}_content_block(block: &mut horizon_mods::content::Block) {{
    use horizon_mods::content::RenderShapes;
    if !{krate}::ours(&block.id) || block.properties.render_shape.is_some() {{
        return;
    }}
    let own: Vec<(String, Vec<String>)> =
        block.state_properties.iter().map(|(k, p)| (k.clone(), p.values().iter().map(|v| v.to_string()).collect())).collect();
    let base: Vec<String> = own
        .iter()
        .map(|(k, values)| block.default_state.get(k).cloned().unwrap_or_else(|| values.first().cloned().unwrap_or_default()))
        .collect();
    let text = |values: &[String]| -> String {{
        if own.is_empty() {{
            return block.id.clone();
        }}
        let props: Vec<String> = own.iter().zip(values).map(|((k, _), v)| format!("{{k}}={{v}}")).collect();
        format!("{{}}[{{}}]", block.id, props.join(","))
    }};
    let (default, varying) = {krate}::with_probe_runtime(|rt| {{
        let default = {c}::glue::render_shape(rt, &text(&base)).unwrap_or("model");
        let mut varying: Vec<(usize, Vec<(String, &'static str)>)> = Vec::new();
        for (i, (_, values)) in own.iter().enumerate() {{
            let mut by_value = Vec::new();
            for v in values {{
                let mut state = base.clone();
                state[i] = v.clone();
                by_value.push((v.clone(), {c}::glue::render_shape(rt, &text(&state)).unwrap_or("model")));
            }}
            if by_value.iter().any(|(_, s)| *s != default) {{
                varying.push((i, by_value));
            }}
        }}
        (default, varying)
    }});
    let all = (default != "model").then(|| RenderShapes::All(default.to_owned()));
    let declared = match varying.as_slice() {{
        [(i, by_value)] => {{
            let off: Vec<&(String, &str)> = by_value.iter().filter(|(_, s)| *s != "model").collect();
            match off.first() {{
                Some((_, shape)) if off.iter().all(|(_, s)| s == shape) => Some(RenderShapes::States {{
                    shape: shape.to_string(),
                    property: own[*i].0.clone(),
                    values: off.iter().map(|(v, _)| v.clone()).collect(),
                }}),
                _ => all,
            }}
        }}
        _ => all,
    }};
    if declared.is_some() {{
        block.properties.render_shape = declared;
    }}
}}
"#)),
        ("tick", format!(r#"// Generated by horizon-mod-translator: the translated mod's client tick (`ClientTickEvent`), in
// the window's game only.
fn {id}_tick(game: &mut crate::game::Game<'_>) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("tick", |rt| {c}::glue::client_tick(rt, &mut host));
}}
"#)),
        ("block_entity_data", format!(r#"// Generated by horizon-mod-translator: an update tag the server sent for the translated mod's
// block entity, loaded as it arrives (`handleUpdateTag`): what the block entity does once with it
// (particles, sounds) is done once per packet.
fn {id}_block_entity_data(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &horizon_client::containers::ModBlockEntity,
    tag: &horizon_world::nbt::Compound,
) {{
{conversions}    use std::collections::BTreeMap;
    if !{krate}::ours(&entity.id) {{
        return;
    }}
{client_host}    let state = game.native_state_text_at(pos);
    let packet: BTreeMap<String, Nbt> = tag.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    let merged: BTreeMap<String, Nbt> = entity.nbt.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("block_entity_data", |rt| {c}::glue::block_entity_data(rt, &mut host, pos, &state, &packet, &merged));
}}
"#)),
        ("use_animation", format!(r#"// Generated by horizon-mod-translator: the first person pose of one of the translated mod's items in
// use (`Item.getUseAnimation`: a package drawn back as a bow, a drink).
fn {id}_use_animation(stack: &horizon_client::items::ItemStack, animation: &mut horizon_world::consumable::UseAnimation) {{
    use horizon_world::consumable::UseAnimation;
    if !{krate}::ours(&stack.item) {{
        return;
    }}
    if let Some(pose) = {krate}::with_registrations(|rt| {c}::item_use::use_animation(rt, &stack.item)) {{
        *animation = match pose.as_str() {{
            "eat" => UseAnimation::Eat,
            "drink" => UseAnimation::Drink,
            "block" => UseAnimation::Block,
            "bow" => UseAnimation::Bow,
            "trident" => UseAnimation::Trident,
            "crossbow" => UseAnimation::Crossbow,
            "spyglass" => UseAnimation::Spyglass,
            "toot_horn" => UseAnimation::TootHorn,
            "brush" => UseAnimation::Brush,
            _ => UseAnimation::None,
        }};
    }}
}}
"#)),
        ("gui_layer_pre", format!(r#"// Generated by horizon-mod-translator: the translated mod's HUD layers drawn below vanilla's layer
// `layer` (where `RegisterGuiLayersEvent` put them), in the window's game; a vanilla layer one of
// them replaced is hidden.
fn {id}_gui_layer_pre(game: &mut crate::game::Game<'_>, layer: &str, batch: &mut horizon_client::hud::HudBatch, canceled: &mut bool) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: Some(batch) }};
    if {krate}::hook_runtime("gui_layer_pre", |rt| {c}::glue::gui_layer(rt, &mut host, layer, true)) {{
        *canceled = true;
    }}
}}
"#)),
        ("gui_layer_post", format!(r#"// Generated by horizon-mod-translator: the translated mod's HUD layers drawn above vanilla's layer
// `layer` (where `RegisterGuiLayersEvent` put them), in the window's game.
fn {id}_gui_layer_post(game: &mut crate::game::Game<'_>, layer: &str, batch: &mut horizon_client::hud::HudBatch) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: Some(batch) }};
    {krate}::hook_runtime("gui_layer_post", |rt| {c}::glue::gui_layer(rt, &mut host, layer, false));
}}
"#)),
        ("commands", format!(r#"// Generated by horizon-mod-translator: the translated mods' client commands
// (`RegisterClientCommandsEvent`), asked in the window's game when a connection starts and when the
// server sends its tree: the names of their roots, so the chat box takes them.
fn {id}_commands(game: &mut crate::game::Game<'_>, commands: &mut Vec<&'static str>) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    // Each name once for the process: the hook asks again on every connection.
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut kept = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    for name in {krate}::hook_runtime("commands", |rt| {c}::glue::client_commands(rt, &mut host)) {{
        let name = match kept.iter().find(|k| **k == name) {{
            Some(k) => *k,
            None => {{
                let k: &'static str = Box::leak(name.into_boxed_str());
                kept.push(k);
                k
            }}
        }};
        if !commands.contains(&name) {{
            commands.push(name);
        }}
    }}
}}
"#)),
        ("command", format!(r#"// Generated by horizon-mod-translator: a command typed or clicked in chat, run by the translated
// mods' client tree as NeoForge's `ClientCommandHandler.runCommand` runs it; an unknown command or
// argument goes on to the server.
fn {id}_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {{
    if *ran || !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    *ran = {krate}::hook_runtime("command", |rt| {c}::glue::client_command(rt, &mut host, command));
}}
"#)),
        ("key", format!(r#"// Generated by horizon-mod-translator: a key or mouse button the player pressed or let go
// (`native_key`, repeats left out). With nothing over the world a press clicks the translated mods'
// key mappings bound to it (`KeyMapping.click`); they take the clicks with `consumeClick`.
fn {id}_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("key", |rt| {c}::glue::key_input(rt, &mut host, key, pressed));
}}
"#)),
        ("tooltip", format!(r#"// Generated by horizon-mod-translator: an item's tooltip as 1.21.1's `ItemStack.getTooltipLines`
// builds it for the translated mods (`native_tooltip`, each frame it shows): their item's or block's
// own lines after the name (`appendHoverText`), then `ItemTooltipEvent` with every line. The game's
// lines a mod left keep their look.
fn {id}_tooltip(game: &crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, lines: &mut Vec<(horizon_client::text::Text, u32)>) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    let held = host.held(stack);
    let plain: Vec<String> = lines.iter().map(|(t, _)| t.plain()).collect();
    let advanced = game.native_advanced_tooltips();
    let Some(answer) = {krate}::hook_runtime("tooltip", |rt| {c}::glue::tooltip(rt, &mut host, held, &plain, advanced)) else {{
        return;
    }};
    let given = std::mem::take(lines);
    for line in answer {{
        match line {{
            {c}::tooltips::Line::Given(i) => lines.extend(given.get(i).cloned()),
            {c}::tooltips::Line::New(json) => {{
                let text = horizon_client::component::to_text(&json, &mut |s| horizon_client::text::Text::literal(s));
                lines.push((text, u32::MAX));
            }}
        }}
    }}
}}
"#)),
        ("input_key", format!(r#"// Generated by horizon-mod-translator: a key the window handled (`native_input_key`: on any screen,
// repeats too), to the translated mods as NeoForge's `InputEvent.Key`.
fn {id}_input_key(game: &mut crate::game::Game<'_>, event: &winit::event::KeyEvent, modifiers: u32) {{
    if !game.native_window() {{
        return;
    }}
    let winit::keyboard::PhysicalKey::Code(code) = event.physical_key else {{
        return;
    }};
    let Some(key) = horizon_client::keybinds::key_name(code) else {{
        return;
    }};
    // GLFW's actions: release, press, repeat.
    let action = match (event.state, event.repeat) {{
        (winit::event::ElementState::Released, _) => 0,
        (_, false) => 1,
        (_, true) => 2,
    }};
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("input_key", |rt| {c}::glue::input_key(rt, &mut host, key, action, modifiers as i32));
}}
"#)),
        ("mouse_button_pre", format!(r#"// Generated by horizon-mod-translator: a mouse button before the game acts on it
// (`native_mouse_button_pre`), to the translated mods as NeoForge's `InputEvent.MouseButton.Pre`;
// one they cancel goes no further.
fn {id}_mouse_button_pre(game: &mut crate::game::Game<'_>, button: i32, pressed: bool, modifiers: u32, canceled: &mut bool) {{
    if *canceled || !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    if {krate}::hook_runtime("mouse_button_pre", |rt| {c}::glue::mouse_button(rt, &mut host, button, pressed, modifiers as i32, true)) {{
        *canceled = true;
    }}
}}
"#)),
        ("mouse_button_post", format!(r#"// Generated by horizon-mod-translator: a mouse button after the game acted on it
// (`native_mouse_button_post`), to the translated mods as NeoForge's `InputEvent.MouseButton.Post`.
fn {id}_mouse_button_post(game: &mut crate::game::Game<'_>, button: i32, pressed: bool, modifiers: u32) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("mouse_button_post", |rt| {c}::glue::mouse_button(rt, &mut host, button, pressed, modifiers as i32, false));
}}
"#)),
        ("mouse_scrolling", format!(r#"// Generated by horizon-mod-translator: the wheel while playing, once whole steps built up and before
// the hotbar scrolls (`native_mouse_scrolling`), to the translated mods as NeoForge's
// `InputEvent.MouseScrollingEvent`; one they cancel leaves the hotbar.
fn {id}_mouse_scrolling(
    game: &mut crate::game::Game<'_>,
    scroll: [f64; 2],
    accumulated: [i32; 2],
    buttons: [bool; 3],
    cursor: [f32; 2],
    modifiers: u32,
    canceled: &mut bool,
) {{
    let _ = (accumulated, modifiers);
    if *canceled || !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    let at = [f64::from(cursor[0]), f64::from(cursor[1])];
    if {krate}::hook_runtime("mouse_scrolling", |rt| {c}::glue::mouse_scrolling(rt, &mut host, scroll, buttons, at)) {{
        *canceled = true;
    }}
}}
"#)),
        ("mods", format!(r#"// Generated by horizon-mod-translator: the translated mods' row in Options > Mods (`native_mods`):
// their names and versions, and the key mappings they registered (`RegisterKeyMappingsEvent`), whose
// keys the player chooses there.
fn {id}_mods(game: &mut crate::game::Game<'_>, mods: &mut Vec<crate::game::native_hooks::NativeMod>) {{
    if !game.native_window() {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    let (title, description, keys) = {krate}::hook_runtime("mods", |rt| {c}::glue::mod_page(rt, &mut host));
    if !title.is_empty() {{
        mods.push(crate::game::native_hooks::NativeMod {{ id: "{id}".into(), title, description, keys }});
    }}
}}
"#)),
        ("net_from_server", format!(r#"// Generated by horizon-mod-translator: a mod payload the server sent (`native_net_from_server`), to
// the translated mod's handler for its type.
fn {id}_net_from_server(game: &mut crate::game::Game<'_>, channel: &str, data: &[u8]) {{
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("net_from_server", |rt| {c}::glue::payload_from_server(rt, &mut host, channel, data));
}}
"#)),
        ("client_level_load", format!(r#"// Generated by horizon-mod-translator: the client made a level (`native_client_level_load`). Joining
// a server, the mods hear NeoForge's `ClientPlayerNetworkEvent.LoggingIn`; changing dimension, `.Clone`.
fn {id}_client_level_load(game: &mut crate::game::Game<'_>, dimension: &str) {{
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("client_level_load", |rt| {c}::glue::client_level_load(rt, &mut host, dimension));
}}
"#)),
        ("client_level_unload", format!(r#"// Generated by horizon-mod-translator: the client dropped a level (`native_client_level_unload`).
// Leaving the server, the mods hear NeoForge's `ClientPlayerNetworkEvent.LoggingOut` and the level's
// `LevelEvent.Unload`.
fn {id}_client_level_unload(game: &mut crate::game::Game<'_>, dimension: &str) {{
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("client_level_unload", |rt| {c}::glue::client_level_unload(rt, &mut host, dimension));
}}
"#)),
        ("screen_render_post", format!(r#"// Generated by horizon-mod-translator: the translated mod's own screen (`Minecraft.setScreen` with
// one of its screens: Create's value settings), drawn over the game's while it is open.
fn {id}_screen_render_post(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: Some(batch) }};
    {krate}::hook_runtime("screen_render_post", |rt| {c}::glue::screen_render(rt, &mut host, screen.mouse, screen.partial_tick));
}}
"#)),
        ("compute_fov", format!(r#"// Generated by horizon-mod-translator: the world's field of view as the translated mods'
// `ViewportEvent.ComputeFov` listeners leave it.
fn {id}_compute_fov(game: &crate::game::Game<'_>, partial: f32, camera: [f64; 3], degrees: &mut f32) {{
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    let asked = *degrees;
    if let Some(fov) = {krate}::hook_runtime("compute_fov", |rt| {c}::glue::compute_fov(rt, &mut host, partial, camera, asked)) {{
        *degrees = fov;
    }}
}}
"#)),
        ("compute_fog_color", format!(r#"// Generated by horizon-mod-translator: the fog's colour as the translated mods'
// `ViewportEvent.ComputeFogColor` listeners leave it.
fn {id}_compute_fog_color(game: &crate::game::Game<'_>, partial: f32, camera: [f64; 3], fog_type: horizon_client::fog::FogType, rgb: &mut [f32; 3]) {{
    let _ = fog_type;
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    let asked = *rgb;
    if let Some(color) = {krate}::hook_runtime("compute_fog_color", |rt| {c}::glue::compute_fog_color(rt, &mut host, partial, camera, asked)) {{
        *rgb = color;
    }}
}}
"#)),
        ("render_fog", format!(r#"// Generated by horizon-mod-translator: the fog's distances as the translated mods'
// `ViewportEvent.RenderFog` listeners set them (a listener that cancels it): the sky's, and the
// terrain's, which is the environment's (a fluid's, the Nether's) where that is nearer than the
// render distance's.
fn {id}_render_fog(game: &crate::game::Game<'_>, partial: f32, camera: [f64; 3], fog_type: horizon_client::fog::FogType, fog: &mut horizon_client::fog::FogData) {{
    use horizon_client::fog::FogType;
    // `FogType`'s ordinal in 1.21.1: lava, water, powder snow, none.
    let vanilla = match fog_type {{
        FogType::Lava => 0,
        FogType::Water => 1,
        FogType::PowderSnow => 2,
        FogType::Atmospheric => 3,
    }};
    let environmental = fog.environmental_end < fog.render_distance_end;
    let (near, far) = if environmental {{ (fog.environmental_start, fog.environmental_end) }} else {{ (fog.render_distance_start, fog.render_distance_end) }};
    let sky_end = fog.sky_end;
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    let (sky, terrain) = {krate}::hook_runtime("render_fog", |rt| {c}::glue::render_fog(rt, &mut host, partial, camera, vanilla, sky_end, near, far));
    if let Some(end) = sky {{
        fog.sky_end = end;
    }}
    if let Some((start, end)) = terrain {{
        if environmental {{
            (fog.environmental_start, fog.environmental_end) = (start, end);
        }} else {{
            (fog.render_distance_start, fog.render_distance_end) = (start, end);
        }}
    }}
}}
"#)),
        ("camera_shake", format!(r#"// Generated by horizon-mod-translator: the turn the translated mods'
// `ViewportEvent.ComputeCameraAngles` listeners give the camera, added to the frame's shake.
fn {id}_camera_shake(game: &crate::game::Game<'_>, partial: f32, shake: &mut crate::game::native_hooks::NativeShake) {{
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if let Some([pitch, yaw, roll]) = {krate}::hook_runtime("camera_shake", |rt| {c}::glue::camera_angles(rt, &mut host, partial)) {{
        shake.pitch += pitch;
        shake.yaw += yaw;
        shake.roll += roll;
    }}
}}
"#)),
        ("item_property", format!(r#"// Generated by horizon-mod-translator: an item property the translated mods registered
// (`ItemProperties.register`, `registerGeneric`), asked by a 1.21.1 model's `overrides` as
// `ItemOverrides.resolve` asks it. The local player holding the stack is the entity the property
// gets; the stack they use is their hand's own (a bow's `getUseItem() == stack`).
fn {id}_item_property(
    game: &crate::game::Game<'_>,
    stack: &horizon_assets::item_model::StackView,
    property: &str,
    owner: Option<&horizon_assets::item_model::OwnerState>,
    seed: i32,
    value: &mut Option<f32>,
) {{
    if value.is_some() {{
        return;
    }}
    let holder = owner.filter(|o| o.is_local_player).map(|o| {{
        // The stack in use is in the main hand when that holds its item, else in the off hand.
        let main = game.native_hand_stack(horizon_client::first_person::Hand::Main).is_some_and(|s| &*s.item == stack.item.as_str());
        {c}::glue::Holder {{
            at: o.position,
            yaw: o.visual_rotation_y_degrees,
            hand: o.using_this_item.then_some(if main {{ 0 }} else {{ 1 }}),
            remaining: o.use_item_remaining_ticks,
            duration: o.use_duration,
        }}
    }});
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    *value = {krate}::hook_runtime("item_property", |rt| {c}::glue::item_property(rt, &mut host, &stack.item, stack.count, &stack.patch, property, holder.as_ref(), seed));
}}
"#)),
        ("interaction_key", format!(r#"// Generated by horizon-mod-translator: an interaction key about to act while playing
// (`InputEvent.InteractionKeyMappingTriggered`): attack, use (once for each hand tried) or pick
// block. A mod that cancels it takes the action from the game; a cancelled use ends the click.
fn {id}_interaction_key(
    game: &mut crate::game::Game<'_>,
    button: i32,
    key_mapping: &str,
    hand: horizon_client::first_person::Hand,
    swing_hand: &mut bool,
    canceled: &mut bool,
) {{
    if *canceled || !game.native_window() {{
        return;
    }}
    let hand = if matches!(hand, horizon_client::first_person::Hand::Off) {{ 1 }} else {{ 0 }};
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    if let Some(swing) = {krate}::hook_runtime("interaction_key", |rt| {c}::glue::interaction_key(rt, &mut host, button, key_mapping, hand)) {{
        *canceled = true;
        *swing_hand = swing;
    }}
}}
"#)),
        ("client_use_item_on", format!(r#"// Generated by horizon-mod-translator: the window's player using a hand on the block under the
// crosshair (`MultiPlayerGameMode.performUseItemOn`): the block hears `RightClickBlock` on the
// client (Create's value settings start holding there, its filters take the click). A listener's
// cancel answers for the block and the item here; the use still goes to the server.
fn {id}_client_use_item_on(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    face: [i32; 3],
    point: [f64; 3],
    hand: horizon_client::first_person::Hand,
    result: &mut crate::game::native_hooks::NativeClientUse,
) {{
    use crate::game::native_hooks::NativeClientUse;
    if *result != NativeClientUse::Goes || !game.native_window() {{
        return;
    }}
    let hand = if matches!(hand, horizon_client::first_person::Hand::Off) {{ 1 }} else {{ 0 }};
    // Direction's 3D data value: down, up, north, south, west, east.
    let face = match face {{
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        _ => 5,
    }};
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    let answer = {krate}::hook_runtime("client_use_item_on", |rt| {c}::glue::client_use_item_on(rt, &mut host, pos, face, point, hand));
    *result = match answer {{
        None => return,
        Some({c}::events::Answer::Pass) => NativeClientUse::Pass,
        Some({c}::events::Answer::Fail) => NativeClientUse::Fail,
        Some({c}::events::Answer::Consume) => NativeClientUse::Consume,
        Some({c}::events::Answer::Success) => NativeClientUse::Success,
    }};
}}
"#)),
        ("screen_mouse_button_pressed_pre", format!(r#"// Generated by horizon-mod-translator: a mouse button going down over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_mouse_button_pressed_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_mouse_button_pressed_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Press(button), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
        ("screen_mouse_button_released_pre", format!(r#"// Generated by horizon-mod-translator: a mouse button coming up (Create's value settings confirm on the use button's) over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_mouse_button_released_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_mouse_button_released_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Release(button), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
        ("screen_mouse_dragged_pre", format!(r#"// Generated by horizon-mod-translator: the pointer dragged with a button held over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_mouse_dragged_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    button: i32,
    delta: [f32; 2],
    modifiers: u32,
    canceled: &mut bool,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_mouse_dragged_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Drag(button, delta), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
        ("screen_mouse_scrolled_pre", format!(r#"// Generated by horizon-mod-translator: the wheel over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_mouse_scrolled_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    scroll: [f64; 2],
    modifiers: u32,
    canceled: &mut bool,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_mouse_scrolled_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Scroll(scroll), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
        ("screen_key_pressed_pre", format!(r#"// Generated by horizon-mod-translator: a key going down (and its repeats) over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_key_pressed_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    key: &str,
    repeat: bool,
    modifiers: u32,
    canceled: &mut bool,
) {{
    let _ = repeat;
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_key_pressed_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Key(key, true), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
        ("screen_key_released_pre", format!(r#"// Generated by horizon-mod-translator: a key coming up over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_key_released_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    key: &str,
    modifiers: u32,
    canceled: &mut bool,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_key_released_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Key(key, false), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
        ("screen_char_typed_pre", format!(r#"// Generated by horizon-mod-translator: a character typed over the translated mod's own screen, to the
// screen (`Screen`'s input handlers); taken, it goes no further.
fn {id}_screen_char_typed_pre(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    c: char,
    modifiers: u32,
    canceled: &mut bool,
) {{
    if game.native_mod_screen() != Some("{id}") {{
        return;
    }}
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Shared(game), batch: None }};
    if {krate}::hook_runtime("screen_char_typed_pre", |rt| {c}::glue::screen_input(rt, &mut host, screen.mouse, {c}::screens::Input::Char(c), modifiers)) {{
        *canceled = true;
    }}
}}
"#)),
    ]
}

/// The host translated code gets on the server's game while a hook runs there with a level held
/// (a block entity's tick, a level's): its blocks, items, recipes and item entities, and payloads
/// to players.
fn game_host(c: &str, server_send: &str) -> String {
    let server_effects = server_effects(c);
    let (reads, writes) = (world_reads("self.game", c), world_writes(c));
    let (containers, stacks) = (container_methods(c), stack_helpers(c));
    format!(r#"    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.game.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.game.native_dimension().to_owned()
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.game.native_fluid_name_in_tag(fluid, tag)
        }}
        fn fluid_flow(&self, pos: [i32; 3]) -> [f64; 3] {{
            self.game.native_fluid_flow_at(pos)
        }}
        fn new_liquid(&mut self, pos: [i32; 3], fluid: &str, state: &str) -> Option<(String, u8, bool)> {{
            self.game.native_new_liquid(pos, fluid, state).map(|(name, amount, falls)| (name.to_owned(), amount, falls))
        }}
        fn item_is_block(&self, item: &str) -> bool {{
            self.game.native_item_facts(item).is_some_and(|(block, _)| block)
        }}
        fn item_stack_limit(&self, item: &str) -> Option<u32> {{
            self.game.native_item_facts(item).map(|(_, limit)| limit)
        }}
        fn item_max_damage(&self, item: &str) -> Option<u32> {{
            Some(self.game.native_item_max_damage(item))
        }}
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
        }}
        fn outline_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            Some(self.game.native_outline_boxes(pos))
        }}
        fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {{
            self.game.native_set_state_text_at(pos, state, 3)
        }}
        fn block_changed(&mut self, pos: [i32; 3]) {{
            self.game.native_block_entity_sync(pos);
        }}
        fn spawn_experience_orb(&mut self, at: [f64; 3], value: u32, motion: Option<[f64; 3]>) -> bool {{
            self.game.native_spawn_experience_orb(at, value, motion)
        }}
        fn players(&self) -> Vec<(u128, [f64; 3])> {{
            let dim = self.game.native_dimension();
            self.game.native_server_players().into_iter().filter(|p| p.dimension == dim).map(|p| (p.uuid, p.position)).collect()
        }}
        fn recipes_of_type(&self, kind: &str) -> Vec<(String, String)> {{
            self.game.native_recipes_of_type(kind).iter().map(|r| (r.id.clone(), r.json.to_string())).collect()
        }}
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
        fn game_time(&self) -> i64 {{
            self.game.native_game_time()
        }}
{reads}{writes}        fn item_entities(&self, b: [f64; 6]) -> Vec<{c}::host::ItemEntity> {{
            let dim = self.game.native_dimension();
            self.game.native_item_entities(dim, [b[0], b[1], b[2]], [b[3], b[4], b[5]]).into_iter().map(|e| {c}::host::ItemEntity {{
                id: e.id,
                pos: e.position,
                motion: e.motion,
                on_ground: e.on_ground,
                item: e.stack.item.to_string(),
                count: e.stack.count,
                pickup_delay: e.pickup_delay,
                age: e.age,
            }}).collect()
        }}
        fn set_item_entity(&mut self, id: u32, item: &str, count: u32) -> bool {{
            let dim = self.game.native_dimension();
            let stack = horizon_client::items::ItemStack::new(item, count);
            self.game.native_set_item_entity(dim, id, stack)
        }}
        fn move_item_entity(&mut self, id: u32, pos: Option<[f64; 3]>, motion: Option<[f64; 3]>) -> bool {{
            let dim = self.game.native_dimension();
            self.game.native_move_item_entity(dim, id, pos, motion)
        }}
        fn spawn_item_entity(&mut self, item: &str, count: u32, pos: [f64; 3], motion: [f64; 3], pickup_delay: u32) -> Option<u32> {{
            let dim = self.game.native_dimension();
            let stack = horizon_client::items::ItemStack::new(item, count);
            self.game.native_spawn_item_entity(dim, stack, pos, motion, pickup_delay)
        }}
{containers}{server_effects}{server_send}    }}
{stacks}"#)
}

/// A server level's tick (`LevelTickEvent.Pre` before it, `.Post` after), on the server's game
/// holding that level.
fn level_tick(id: &str, krate: &str, c: &str, host: &str, post: bool) -> String {
    let (phase, event, when) = if post { ("post", "Post", "after") } else { ("pre", "Pre", "before") };
    format!(r#"// Generated by horizon-mod-translator: a server level's tick (`LevelTickEvent.{event}`), {when} the
// level ticks, on the server's game holding that level.
fn {id}_level_tick_{phase}(game: &mut crate::game::Game<'_>, dimension: &str) {{
    let _ = dimension;
{host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("level_tick", |rt| {c}::glue::level_tick(rt, &mut host, {post}));
}}
"#)
}

/// The server's hooks (on `Game`): the block entities (creating, whether they tick, each tick), the
/// server's tick, and the payloads clients send.
/// The host a server hook for one player's click gets (`native_right_click_block`,
/// `native_use_item_on`, `native_use_item`): the game with that player's state swapped in (hand,
/// sneaking, food, mode, place), its blocks, items and recipes, and the effects it makes.
/// A host's reads of the world around a block (light, time of day, weather, height), for a host
/// whose game `get` gives (`self.game`, `self.game.get()`).
fn world_reads(get: &str, c: &str) -> String {
    format!(
        r#"        fn day_time(&self) -> i64 {{
            {get}.native_day_time()
        }}
        fn light(&self, pos: [i32; 3]) -> (u8, u8) {{
            {get}.native_light(pos)
        }}
        fn weather(&self) -> (f32, f32) {{
            {get}.native_weather()
        }}
        fn world_height(&self) -> (i32, i32) {{
            {get}.native_world_height()
        }}
        fn game_rule(&self, name: &str) -> Option<i32> {{
            {get}.native_game_rule(name)
        }}
        fn resource(&self, path: &str) -> Option<Vec<u8>> {{
            {get}.native_resource(path)
        }}
        fn resource_paths(&self, prefix: &str) -> Vec<String> {{
            {get}.native_resource_paths(prefix)
        }}
        fn resource_namespaces(&self, root: &str) -> Vec<String> {{
            {get}.native_resource_namespaces(root)
        }}
        fn state_id(&self, state: &str) -> Option<i32> {{
            {get}.native_state_id_of(state).and_then(|s| i32::try_from(s).ok())
        }}
        fn state_of_id(&self, id: i32) -> Option<String> {{
            Some({get}.native_state_of_id(u32::try_from(id).unwrap_or(u32::MAX)))
        }}
        fn block_requires_tool(&self, state: &str) -> bool {{
            {get}.native_state_requires_tool(state).unwrap_or(false)
        }}
        fn item_tool(&self, item: &str, state: &str) -> Option<{c}::host::ToolFacts> {{
            {get}.native_item_tool(item, state).map(|(speed, correct, per_block)| {c}::host::ToolFacts {{ speed, correct, damage_per_block: i32::try_from(per_block).unwrap_or(i32::MAX) }})
        }}
        fn block_sounds(&self, state: &str) -> Option<{c}::host::BlockSounds> {{
            {get}.native_state_sounds(state).map(|(volume, pitch, events)| {c}::host::BlockSounds {{ volume, pitch, events }})
        }}
        fn block_solid(&self, state: &str) -> Option<bool> {{
            {get}.native_state_solid(state)
        }}
"#
    )
}

/// A server host's redstone and block ticks, and the level's entities (`self.game` a `&mut Game`):
/// any entity by its id (its type and place as the commands read them, a mob's facing and health),
/// and a mob hurt, killed, removed, healed, pushed or moved through the native mob helpers. Its
/// text for a translation key is the game's language, as a vanilla server's `Language` (an
/// entity's name, `Component.getString`).
fn world_writes(c: &str) -> String {
    format!(
        r#"        fn place_item_block(&mut self, held: &{c}::host::HandStack, pos: [i32; 3], face: u8, hit: [f64; 3], inside: bool, look: (f32, f32), sneaking: bool) -> Option<[i32; 3]> {{
            let stack = horizon_client::items::ItemStack {{ item: held.item.as_str().into(), count: held.count, damage: held.damage, extra: None }};
            self.game.native_place_item_block(&stack, pos, face, hit, inside, look, sneaking)
        }}
        // A break by a mod's actor: the tool crosses with its enchantments, the drops with their
        // mod data (a toolbox's contents) as the runtime's NBT.
        fn block_drops(&mut self, pos: [i32; 3], state: &str, tool: Option<&{c}::host::HandStack>) -> Vec<{c}::host::HandStack> {{
            fn nbt(tag: &horizon_world::nbt::Tag) -> Option<{c}::nbt::Nbt> {{
                use horizon_world::nbt::Tag;
                use {c}::nbt::Nbt;
                Some(match tag {{
                    Tag::Byte(b) => Nbt::Byte(*b),
                    Tag::Short(s) => Nbt::Short(*s),
                    Tag::Int(i) => Nbt::Int(*i),
                    Tag::Long(l) => Nbt::Long(*l),
                    Tag::Float(f) => Nbt::Float(f32::from_bits(*f)),
                    Tag::Double(d) => Nbt::Double(f64::from_bits(*d)),
                    Tag::String(s) => Nbt::String(s.to_string_lossy()),
                    Tag::Compound(c) => Nbt::Compound(c.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), nbt(v)?))).collect()),
                    Tag::List(l) => Nbt::List(l.iter().filter_map(nbt).collect()),
                    Tag::ByteArray(a) => Nbt::ByteArray(a.iter().map(|b| *b as i8).collect()),
                    Tag::IntArray(a) => Nbt::IntArray(a.clone()),
                    Tag::LongArray(a) => Nbt::LongArray(a.clone()),
                    Tag::End => return None,
                }})
            }}
            let tool = tool.map(|t| {{
                let mut stack = horizon_client::items::ItemStack {{ item: t.item.as_str().into(), count: t.count, damage: t.damage, extra: None }};
                for (id, level) in t.enchantments() {{
                    stack.enchant(&id, u32::try_from(level).unwrap_or(0));
                }}
                stack
            }});
            let drops = self.game.native_block_loot(pos, state, tool.as_ref());
            let types = self.game.native_registered_components();
            drops
                .iter()
                .map(|d| {{
                    let components = types.iter().filter_map(|name| Some((name.to_string(), nbt(self.game.native_component(d, name).ok()??)?))).collect();
                    {c}::host::HandStack {{ item: d.item.to_string(), count: d.count, damage: d.damage, max_damage: self.game.native_item_max_damage(&d.item), components }}
                }})
                .collect()
        }}
        fn spawn_after_break(&mut self, pos: [i32; 3], state: &str, tool: Option<&{c}::host::HandStack>) {{
            let tool = tool.map(|t| {{
                let mut stack = horizon_client::items::ItemStack::new(&t.item, t.count);
                for (id, level) in t.enchantments() {{
                    stack.enchant(&id, u32::try_from(level).unwrap_or(0));
                }}
                stack
            }});
            self.game.native_spawn_after_break(pos, state, tool.as_ref());
        }}
        fn destroy_block_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {{
            self.game.native_destroy_block_progress(breaker, pos, progress);
        }}
        fn block_experience(&mut self, state: &str) -> i32 {{
            i32::try_from(self.game.native_block_experience(state)).unwrap_or(i32::MAX)
        }}
        fn pop_experience(&mut self, pos: [i32; 3], amount: i32) {{
            self.game.native_pop_experience(pos, u32::try_from(amount).unwrap_or(0));
        }}
        fn damage_types(&self) -> Vec<{c}::host::DamageTypeView> {{
            let catalog = &self.game.motion.health.damage;
            let tags: Vec<&str> = catalog.tags().collect();
            let named = |t: &str| {{
                let t = t.trim_start_matches('#');
                if t.contains(':') {{ t.to_owned() }} else {{ format!("minecraft:{{t}}") }}
            }};
            catalog
                .entries()
                .map(|(id, d)| {c}::host::DamageTypeView {{
                    id: d.name.clone(),
                    msg_id: d.message_id.clone(),
                    exhaustion: d.exhaustion,
                    tags: tags.iter().filter(|t| catalog.in_tag(id, t)).map(|t| named(t)).collect(),
                }})
                .collect()
        }}
        fn player_entities(&self) -> Vec<(i32, u128, [f64; 3])> {{
            self.game.lan.as_ref().map_or_else(Vec::new, |host| host.hub.guests.iter().filter(|g| g.placed).map(|g| (g.entity_id, g.uuid, g.position)).collect())
        }}
        fn translate(&self, key: &str) -> Option<String> {{
            horizon_client::lang::has(key).then(|| horizon_client::lang::get(key))
        }}
        fn entity_type_in_tag(&self, kind: &str, tag: &str) -> bool {{
            self.game.res.mob_data().entity_has(tag, kind)
        }}
        fn neighbor_signal(&mut self, pos: [i32; 3]) -> i32 {{
            let dimension = self.game.native_dimension().to_owned();
            self.game.native_neighbor_signal(&dimension, pos)
        }}
        fn schedule_block_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool {{
            self.game.native_schedule_block_tick_at(pos, block, delay)
        }}
        fn entities_in(&self, b: [f64; 6]) -> Vec<{c}::host::EntityView> {{
            let dim = self.game.native_dimension();
            self.game.native_entities_in(dim, [b[0], b[1], b[2]], [b[3], b[4], b[5]]).into_iter().filter_map(|id| {c}::host::Host::entity(self, id)).collect()
        }}
        fn entity(&self, id: i32) -> Option<{c}::host::EntityView> {{
            let t = self.game.level_entity(id)?;
            let pos = self.game.target_position(t)?;
            let kind = self.game.target_type(t);
            let mob = u32::try_from(id).ok().and_then(|i| self.game.native_mob(self.game.native_dimension(), i));
            Some({c}::host::EntityView {{ id, kind, pos, yaw: mob.as_ref().map_or(0.0, |m| m.yaw), health: mob.map(|m| (m.health, m.max_health)) }})
        }}
        fn hurt_entity(&mut self, id: i32, amount: f32) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            let Some(mob) = u32::try_from(id).ok().and_then(|i| self.game.native_mob(&dimension, i)) else {{
                return false;
            }};
            if !(amount > 0.0) {{
                return false;
            }}
            if mob.health - amount > 0.0 {{
                self.game.native_set_mob_health(&dimension, mob.id, mob.health - amount)
            }} else {{
                self.game.native_kill_mob(&dimension, mob.id)
            }}
        }}
        fn hurt_entity_by(&mut self, id: i32, amount: f32, by: &{c}::host::HurtBy) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            let hurt = crate::game::native_mobs::NativeHurt {{
                damage_type: by.damage_type.clone(),
                player: by.player,
                mob: by.mob.and_then(|m| u32::try_from(m).ok()),
                from: by.from,
                motion: by.motion,
                direct: by.direct.clone(),
            }};
            u32::try_from(id).is_ok_and(|i| self.game.native_hurt_mob(&dimension, i, amount, &hurt))
        }}
        fn post_attack_effects(&mut self, victim: {c}::host::Struck, by: &{c}::host::HurtBy, weapon: &{c}::host::AttackWeapon) -> bool {{
            use crate::game::native_mobs::{{NativeStruck, NativeWeapon}};
            let dimension = self.game.native_dimension().to_owned();
            let hurt = crate::game::native_mobs::NativeHurt {{
                damage_type: by.damage_type.clone(),
                player: by.player,
                mob: by.mob.and_then(|m| u32::try_from(m).ok()),
                from: by.from,
                motion: by.motion,
                direct: by.direct.clone(),
            }};
            let victim = match victim {{
                {c}::host::Struck::Player(uuid) => NativeStruck::Player(uuid),
                {c}::host::Struck::Entity(id) => match u32::try_from(id) {{
                    Ok(id) => NativeStruck::Mob(id),
                    Err(_) => return false,
                }},
            }};
            let weapon = match weapon {{
                {c}::host::AttackWeapon::None => NativeWeapon::None,
                {c}::host::AttackWeapon::Held => NativeWeapon::Held,
                {c}::host::AttackWeapon::Stack(t) => {{
                    let mut stack = horizon_client::items::ItemStack {{ item: t.item.as_str().into(), count: t.count, damage: t.damage, extra: None }};
                    for (id, level) in t.enchantments() {{
                        stack.enchant(&id, u32::try_from(level).unwrap_or(0));
                    }}
                    NativeWeapon::Stack(stack)
                }}
            }};
            self.game.native_post_attack(&dimension, victim, &hurt, &weapon)
        }}
        fn knockback_entity(&mut self, id: i32, strength: f64, x: f64, z: f64) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            u32::try_from(id).is_ok_and(|i| self.game.native_knockback_mob(&dimension, i, strength, x, z))
        }}
        fn kill_entity(&mut self, id: i32) -> bool {{
            let Some(t) = self.game.level_entity(id) else {{
                return false;
            }};
            self.game.kill_target(t);
            true
        }}
        fn remove_entity(&mut self, id: i32) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            u32::try_from(id).is_ok_and(|i| self.game.native_remove_mob(&dimension, i))
        }}
        // A vanilla entity a mod made and added (`made_entities`): the game makes it from its
        // compound as `/summon` does, or strikes its lightning; and an entity's compound, read back.
        fn summon(&mut self, kind: &str, pos: [f64; 3], nbt: &std::collections::BTreeMap<String, {c}::nbt::Nbt>) -> Option<i32> {{
            fn tag(n: &{c}::nbt::Nbt) -> horizon_world::nbt::Tag {{
                use horizon_world::nbt::{{Compound, JavaString, Tag}};
                use {c}::nbt::Nbt;
                match n {{
                    Nbt::Byte(b) => Tag::Byte(*b),
                    Nbt::Short(s) => Tag::Short(*s),
                    Nbt::Int(i) => Tag::Int(*i),
                    Nbt::Long(l) => Tag::Long(*l),
                    Nbt::Float(f) => Tag::Float(f.to_bits()),
                    Nbt::Double(d) => Tag::Double(d.to_bits()),
                    Nbt::String(s) => Tag::String(JavaString::from(s.as_str())),
                    Nbt::Compound(m) => Tag::Compound(m.iter().map(|(k, v)| (JavaString::from(k.as_str()), tag(v))).collect::<Compound>()),
                    Nbt::List(l) => Tag::List(l.iter().map(tag).collect()),
                    Nbt::ByteArray(a) => Tag::ByteArray(a.iter().map(|b| *b as u8).collect()),
                    Nbt::IntArray(a) => Tag::IntArray(a.clone()),
                    Nbt::LongArray(a) => Tag::LongArray(a.clone()),
                }}
            }}
            let dimension = self.game.native_dimension().to_owned();
            let compound = nbt.iter().map(|(k, v)| (horizon_world::nbt::JavaString::from(k.as_str()), tag(v))).collect::<horizon_world::nbt::Compound>();
            self.game.native_summon(&dimension, kind, pos, compound).and_then(|id| i32::try_from(id).ok())
        }}
        fn strike_lightning(&mut self, pos: [f64; 3], visual_only: bool, cause: Option<u128>) -> Option<i32> {{
            let dimension = self.game.native_dimension().to_owned();
            self.game.native_strike_lightning(&dimension, pos, visual_only, cause).and_then(|id| i32::try_from(id).ok())
        }}
        fn entity_nbt(&mut self, id: i32) -> Option<std::collections::BTreeMap<String, {c}::nbt::Nbt>> {{
            fn nbt(tag: &horizon_world::nbt::Tag) -> Option<{c}::nbt::Nbt> {{
                use horizon_world::nbt::Tag;
                use {c}::nbt::Nbt;
                Some(match tag {{
                    Tag::Byte(b) => Nbt::Byte(*b),
                    Tag::Short(s) => Nbt::Short(*s),
                    Tag::Int(i) => Nbt::Int(*i),
                    Tag::Long(l) => Nbt::Long(*l),
                    Tag::Float(f) => Nbt::Float(f32::from_bits(*f)),
                    Tag::Double(d) => Nbt::Double(f64::from_bits(*d)),
                    Tag::String(s) => Nbt::String(s.to_string_lossy()),
                    Tag::Compound(m) => Nbt::Compound(m.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), nbt(v)?))).collect()),
                    Tag::List(l) => Nbt::List(l.iter().filter_map(nbt).collect()),
                    Tag::ByteArray(a) => Nbt::ByteArray(a.iter().map(|b| *b as i8).collect()),
                    Tag::IntArray(a) => Nbt::IntArray(a.clone()),
                    Tag::LongArray(a) => Nbt::LongArray(a.clone()),
                    Tag::End => return None,
                }})
            }}
            let dimension = self.game.native_dimension().to_owned();
            let saved = self.game.native_entity_nbt(&dimension, u32::try_from(id).ok()?)?;
            Some(saved.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), nbt(v)?))).collect())
        }}
        fn set_entity_health(&mut self, id: i32, health: f32) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            u32::try_from(id).is_ok_and(|i| self.game.native_set_mob_health(&dimension, i, health))
        }}
        fn push_entity(&mut self, id: i32, velocity: [f64; 3]) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            u32::try_from(id).is_ok_and(|i| self.game.native_push_mob(&dimension, i, velocity))
        }}
        fn teleport_entity(&mut self, id: i32, pos: [f64; 3]) -> bool {{
            let dimension = self.game.native_dimension().to_owned();
            u32::try_from(id).is_ok_and(|i| self.game.native_teleport_mob(&dimension, i, pos))
        }}
{attributes}{effects}{vitals}{level_events}"#,
        attributes = attribute_methods(c, "Some(&mut *self.game)"),
        effects = effect_methods(c, "Some(&mut *self.game)"),
        vitals = vital_methods(c, "Some(&mut *self.game)"),
        level_events = level_event_methods("Some(&mut *self.game)"),
    )
}

/// A host's living entities' attributes, through the game `game` gives (`Option<&mut Game>`):
/// the player in place's whole map, a mob's plain values and the rest of its attributes
/// (`native_attribute`, `native_change_attribute`).
fn attribute_methods(c: &str, game: &str) -> String {
    format!(
        r#"        fn attribute(&mut self, entity: Option<i32>, attribute: &str) -> Option<{c}::host::AttributeView> {{
            let a = {game}?.native_attribute(entity, attribute)?;
            let modifiers = a.modifiers.into_iter().map(|m| {c}::host::AttributeModifierView {{ id: m.id, amount: m.amount, operation: m.operation, permanent: m.permanent }});
            Some({c}::host::AttributeView {{ value: a.value, base: a.base, modifiers: modifiers.collect() }})
        }}
        fn change_attribute(&mut self, entity: Option<i32>, attribute: &str, change: &{c}::host::AttributeChange) -> bool {{
            use crate::game::native_attributes::{{NativeAttributeChange as Change, NativeAttributeModifier as Modifier}};
            let change = match change {{
                {c}::host::AttributeChange::Base(v) => Change::Base(*v),
                {c}::host::AttributeChange::Add {{ modifier: m, replace }} => Change::Add {{
                    modifier: Modifier {{ id: m.id.clone(), amount: m.amount, operation: m.operation, permanent: m.permanent }},
                    replace: *replace,
                }},
                {c}::host::AttributeChange::Remove(id) => Change::Remove(id.clone()),
                {c}::host::AttributeChange::RemoveAll => Change::RemoveAll,
            }};
            {game}.is_some_and(|game| game.native_change_attribute(entity, attribute, &change))
        }}
"#
    )
}

/// A host's living entities' effects, through the game `game` gives (`Option<&mut Game>`): the
/// player in place's and a mob's (`native_effects`, `native_add_effect`, `native_remove_effect`).
fn effect_methods(c: &str, game: &str) -> String {
    format!(
        r#"        fn effects(&mut self, entity: Option<i32>) -> Option<Vec<{c}::host::EffectView>> {{
            let effects = {game}?.native_effects(entity)?;
            Some(effects.into_iter().map(|e| {c}::host::EffectView {{ id: e.id, duration: e.duration, amplifier: e.amplifier, ambient: e.ambient, visible: e.visible, show_icon: e.show_icon }}).collect())
        }}
        fn add_effect(&mut self, entity: Option<i32>, effect: &{c}::host::EffectView, force: bool) -> bool {{
            let effect = crate::game::native_effects::NativeEffect {{
                id: effect.id.clone(),
                duration: effect.duration,
                amplifier: effect.amplifier,
                ambient: effect.ambient,
                visible: effect.visible,
                show_icon: effect.show_icon,
            }};
            {game}.is_some_and(|game| game.native_add_effect(entity, &effect, force))
        }}
        fn remove_effect(&mut self, entity: Option<i32>, effect: &str) -> bool {{
            {game}.is_some_and(|game| game.native_remove_effect(entity, effect))
        }}
"#
    )
}

/// A host's player in place's health and food, through the game `game` gives (`Option<&mut
/// Game>`): `native_player_vitals`, `native_set_player_vitals`.
fn vital_methods(c: &str, game: &str) -> String {
    format!(
        r#"        fn player_vitals(&mut self) -> Option<{c}::host::VitalsView> {{
            let v = {game}?.native_player_vitals()?;
            Some({c}::host::VitalsView {{ health: v.health, max_health: v.max_health, absorption: v.absorption, food: v.food, saturation: v.saturation, exhaustion: v.exhaustion }})
        }}
        fn set_player_vitals(&mut self, to: &{c}::host::VitalsView) -> bool {{
            let to = crate::game::native_vitals::NativeVitals {{ health: to.health, max_health: to.max_health, absorption: to.absorption, food: to.food, saturation: to.saturation, exhaustion: to.exhaustion }};
            {game}.is_some_and(|game| game.native_set_player_vitals(&to))
        }}
"#
    )
}

/// The level events of a host whose game `game` gives (an `Option<&mut Game>`): a mod's
/// `levelEvent`, the game's own (`native_level_event`).
fn level_event_methods(game: &str) -> String {
    format!(
        r#"        fn level_event(&mut self, event: i32, pos: [i32; 3], data: i32) -> bool {{
            {game}.is_some_and(|game| game.native_level_event(event, pos, data))
        }}
"#
    )
}

/// A stack's component as the runtime's NBT (`Self::nbt_of`, inside an `impl`): the game's tag,
/// none for an end tag.
fn nbt_of_fn(c: &str) -> String {
    format!(
        r#"        fn nbt_of(tag: &horizon_world::nbt::Tag) -> Option<{c}::nbt::Nbt> {{
            use horizon_world::nbt::Tag;
            use {c}::nbt::Nbt;
            Some(match tag {{
                Tag::Byte(b) => Nbt::Byte(*b),
                Tag::Short(s) => Nbt::Short(*s),
                Tag::Int(i) => Nbt::Int(*i),
                Tag::Long(l) => Nbt::Long(*l),
                Tag::Float(f) => Nbt::Float(f32::from_bits(*f)),
                Tag::Double(d) => Nbt::Double(f64::from_bits(*d)),
                Tag::String(s) => Nbt::String(s.to_string_lossy()),
                Tag::Compound(c) => Nbt::Compound(c.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), Self::nbt_of(v)?))).collect()),
                Tag::List(l) => Nbt::List(l.iter().filter_map(Self::nbt_of).collect()),
                Tag::ByteArray(a) => Nbt::ByteArray(a.iter().map(|b| *b as i8).collect()),
                Tag::IntArray(a) => Nbt::IntArray(a.clone()),
                Tag::LongArray(a) => Nbt::LongArray(a.clone()),
                Tag::End => return None,
            }})
        }}
"#
    )
}

/// The host of a hit's hooks (`native_living_incoming_damage`, `_knockback`, `_damage_pre`,
/// `_damage_post`), which run inside the game's damage code with no game at hand: it answers what
/// the game told of the hit (the player whose blow it is, as the blow started:
/// `native_hit_player`), and what a listener changes of the world waits for the hit to end
/// (`native_after_hit`), made then by a host of the game (`Later`).
fn hit_host(c: &str, server_send: &str) -> String {
    let writes = world_writes(c);
    let nbt_of = nbt_of_fn(c);
    let later = |call: &str| format!("crate::game::native_hits::native_after_hit(move |game| {{\n                {call};\n            }});");
    let (hurt, kill, remove) = (later("Later { game }.hurt_entity(id, amount)"), later("Later { game }.kill_entity(id)"), later("Later { game }.remove_entity(id)"));
    let (hurt_by, knockback) = (later("Later { game }.hurt_entity_by(id, amount, &by)"), later("Later { game }.knockback_entity(id, strength, x, z)"));
    let post_attack = later("Later { game }.post_attack_effects(victim, &by, &weapon)");
    let (health, push, teleport) = (later("Later { game }.set_entity_health(id, health)"), later("Later { game }.push_entity(id, velocity)"), later("Later { game }.teleport_entity(id, pos)"));
    let (orb, payload) = (later("game.native_spawn_experience_orb(at, value, motion)"), later("Later { game }.send_payload(to, &channel, &data)"));
    let attribute = later("Later { game }.change_attribute(entity, &attribute, &change)");
    let (give, take) = (later("Later { game }.add_effect(entity, &effect, force)"), later("Later { game }.remove_effect(entity, &effect)"));
    let (summon, strike) = (later("Later { game }.summon(&kind, pos, &nbt)"), later("Later { game }.strike_lightning(pos, visual_only, cause)"));
    format!(r#"    // The trait's methods, called on the hosts below by name.
    use {c}::host::Host as _;
    struct Host {{
        player: Option<crate::game::native_hits::NativeHitPlayer>,
    }}
    impl Host {{
{nbt_of}    }}
    impl {c}::host::Host for Host {{
        // No level is at hand inside a hit: no blocks to read or set.
        fn block_state(&self, _pos: [i32; 3]) -> String {{
            "minecraft:air".into()
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn dimension(&self) -> String {{
            self.player.as_ref().map_or_else(|| "minecraft:overworld".to_owned(), |p| p.player.dimension.clone())
        }}
        fn player_id(&self) -> Option<u128> {{
            self.player.as_ref().map(|p| p.player.uuid)
        }}
        fn player_creative(&self) -> bool {{
            self.player.as_ref().is_some_and(|p| p.player.game_mode == 1)
        }}
        fn player_sneaking(&self) -> bool {{
            self.player.as_ref().is_some_and(|p| p.sneaking)
        }}
        fn player_place(&self) -> Option<([f64; 3], f32, f32)> {{
            self.player.as_ref().map(|p| (p.player.position, p.player.yaw, p.player.pitch))
        }}
        fn player_held(&self) -> Option<(String, u32)> {{
            self.player_hand(0).map(|h| (h.item, h.count))
        }}
        // What the player held as the blow started, with its data.
        fn player_hand(&self, hand: u8) -> Option<{c}::host::HandStack> {{
            let held = self.player.as_ref()?.hands[usize::from(hand.min(1))].as_ref()?;
            Some({c}::host::HandStack {{
                item: held.stack.item.to_string(),
                count: held.stack.count,
                damage: held.stack.damage,
                max_damage: held.max_damage,
                components: held.components.iter().filter_map(|(name, tag)| Some(((*name).to_owned(), Self::nbt_of(tag)?))).collect(),
            }})
        }}
        fn entity(&self, id: i32) -> Option<{c}::host::EntityView> {{
            let p = self.player.as_ref().filter(|p| p.entity == id)?;
            Some({c}::host::EntityView {{ id, kind: "minecraft:player".into(), pos: p.player.position, yaw: p.player.yaw, health: Some((p.player.health, 20.0)) }})
        }}
        fn player_entities(&self) -> Vec<(i32, u128, [f64; 3])> {{
            self.player.iter().map(|p| (p.entity, p.player.uuid, p.player.position)).collect()
        }}
        // What a listener changes waits for the hit to end.
        fn play_sound(&mut self, s: &{c}::host::Sound) {{
            let s = s.clone();
            crate::game::native_hits::native_after_hit(move |game| {{
                game.native_level_sound(&s.event, s.at, s.source, s.volume, s.pitch);
            }});
        }}
        fn particles(&mut self, p: &{c}::host::Particles) {{
            let Some(count) = p.count else {{ return }};
            let p = p.clone();
            crate::game::native_hits::native_after_hit(move |game| {{
                if let Some((options, state)) = game.native_particle_options(&p.kind, p.block.as_deref(), p.item.as_deref(), p.dust, p.at) {{
                    game.native_level_particles(&options, state, p.at, p.delta, p.speed, count);
                }}
            }});
        }}
        fn player_message(&mut self, component: &str, overlay: bool) {{
            if let Some(uuid) = self.player_id() {{
                self.tell_player(uuid, component, overlay);
            }}
        }}
        fn tell_player(&mut self, uuid: u128, component: &str, overlay: bool) -> bool {{
            let Ok(json) = serde_json::from_str::<serde_json::Value>(component) else {{
                return false;
            }};
            crate::game::native_hits::native_after_hit(move |game| {{
                let plain = json.get("text").or_else(|| json.get("translate")).and_then(|t| t.as_str()).unwrap_or("").to_owned();
                let line = horizon_client::text::Text::literal(plain).with_component(json);
                if overlay {{
                    game.native_action_bar(uuid, line);
                }} else {{
                    game.native_tell(uuid, line);
                }}
            }});
            true
        }}
        fn hurt_entity(&mut self, id: i32, amount: f32) -> bool {{
            {hurt}
            true
        }}
        fn hurt_entity_by(&mut self, id: i32, amount: f32, by: &{c}::host::HurtBy) -> bool {{
            let by = by.clone();
            {hurt_by}
            true
        }}
        fn knockback_entity(&mut self, id: i32, strength: f64, x: f64, z: f64) -> bool {{
            {knockback}
            true
        }}
        fn post_attack_effects(&mut self, victim: {c}::host::Struck, by: &{c}::host::HurtBy, weapon: &{c}::host::AttackWeapon) -> bool {{
            let (by, weapon) = (by.clone(), weapon.clone());
            {post_attack}
            true
        }}
        fn level_event(&mut self, event: i32, pos: [i32; 3], data: i32) -> bool {{
            crate::game::native_hits::native_after_hit(move |game| {{
                game.native_level_event(event, pos, data);
            }});
            true
        }}
        // An entity a listener makes comes once the hit is over, with no id for it yet.
        fn summon(&mut self, kind: &str, pos: [f64; 3], nbt: &std::collections::BTreeMap<String, {c}::nbt::Nbt>) -> Option<i32> {{
            let (kind, nbt) = (kind.to_owned(), nbt.clone());
            {summon}
            None
        }}
        fn strike_lightning(&mut self, pos: [f64; 3], visual_only: bool, cause: Option<u128>) -> Option<i32> {{
            {strike}
            None
        }}
        fn kill_entity(&mut self, id: i32) -> bool {{
            {kill}
            true
        }}
        fn remove_entity(&mut self, id: i32) -> bool {{
            {remove}
            true
        }}
        fn set_entity_health(&mut self, id: i32, health: f32) -> bool {{
            {health}
            true
        }}
        fn push_entity(&mut self, id: i32, velocity: [f64; 3]) -> bool {{
            {push}
            true
        }}
        fn teleport_entity(&mut self, id: i32, pos: [f64; 3]) -> bool {{
            {teleport}
            true
        }}
        fn spawn_experience_orb(&mut self, at: [f64; 3], value: u32, motion: Option<[f64; 3]>) -> bool {{
            {orb}
            true
        }}
        fn send_payload(&mut self, to: {c}::host::PayloadTarget, channel: &str, data: &[u8]) -> bool {{
            let (channel, data) = (channel.to_owned(), data.to_vec());
            {payload}
            true
        }}
        fn change_attribute(&mut self, entity: Option<i32>, attribute: &str, change: &{c}::host::AttributeChange) -> bool {{
            let (attribute, change) = (attribute.to_owned(), change.clone());
            {attribute}
            true
        }}
        // The striking player's effects as the blow started; a mob's aren't at hand.
        fn effects(&mut self, entity: Option<i32>) -> Option<Vec<{c}::host::EffectView>> {{
            let p = self.player.as_ref().filter(|p| entity.is_none_or(|id| id == p.entity))?;
            Some(p.effects.iter().map(|e| {c}::host::EffectView {{ id: e.id.clone(), duration: e.duration, amplifier: e.amplifier, ambient: e.ambient, visible: e.visible, show_icon: e.show_icon }}).collect())
        }}
        fn add_effect(&mut self, entity: Option<i32>, effect: &{c}::host::EffectView, force: bool) -> bool {{
            let effect = effect.clone();
            {give}
            true
        }}
        fn remove_effect(&mut self, entity: Option<i32>, effect: &str) -> bool {{
            let effect = effect.to_owned();
            {take}
            true
        }}
    }}
    // The game once the hit is over, for what its hooks changed.
    struct Later<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Later<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
{writes}{server_send}    }}
    let mut host = Host {{ player: crate::game::native_hits::native_hit_player() }};
"#)
}

fn player_host(c: &str, server_effects: &str) -> String {
    let (reads, writes) = (world_reads("self.game", c), world_writes(c));
    let (containers, stacks) = (container_methods(c), stack_helpers(c));
    format!(r#"    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.game.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.game.native_dimension().to_owned()
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.game.native_fluid_name_in_tag(fluid, tag)
        }}
        fn fluid_flow(&self, pos: [i32; 3]) -> [f64; 3] {{
            self.game.native_fluid_flow_at(pos)
        }}
        fn item_is_block(&self, item: &str) -> bool {{
            self.game.native_item_facts(item).is_some_and(|(block, _)| block)
        }}
        fn item_stack_limit(&self, item: &str) -> Option<u32> {{
            self.game.native_item_facts(item).map(|(_, limit)| limit)
        }}
        fn item_max_damage(&self, item: &str) -> Option<u32> {{
            Some(self.game.native_item_max_damage(item))
        }}
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
        }}
        fn outline_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            Some(self.game.native_outline_boxes(pos))
        }}
        fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {{
            self.game.native_set_state_text_at(pos, state, 3)
        }}
        fn player_sneaking(&self) -> bool {{
            self.game.has_shift_down()
        }}
        fn player_held(&self) -> Option<(String, u32)> {{
            self.game.native_held().map(|(item, count, _)| (item, count))
        }}
        fn player_creative(&self) -> bool {{
            self.game.native_acting_player().is_some_and(|p| p.game_mode == 1)
        }}
        fn player_place(&self) -> Option<([f64; 3], f32, f32)> {{
            self.game.native_acting_player().map(|p| (p.position, p.yaw, p.pitch))
        }}
        fn take_player_items(&mut self, item: &str, count: u32) -> u32 {{
            self.game.native_take_items(item, count)
        }}
        fn give_player_items(&mut self, item: &str, count: u32) -> u32 {{
            self.game.native_give_items(item, count)
        }}
        fn player_food(&self) -> i32 {{
            self.game.native_food_level()
        }}
        fn exhaust_player(&mut self, amount: f32) {{
            self.game.native_exhaust(amount)
        }}
        fn block_changed(&mut self, pos: [i32; 3]) {{
            self.game.native_block_entity_sync(pos);
        }}
        fn players(&self) -> Vec<(u128, [f64; 3])> {{
            let dim = self.game.native_dimension();
            self.game.native_server_players().into_iter().filter(|p| p.dimension == dim).map(|p| (p.uuid, p.position)).collect()
        }}
        fn recipes_of_type(&self, kind: &str) -> Vec<(String, String)> {{
            self.game.native_recipes_of_type(kind).iter().map(|r| (r.id.clone(), r.json.to_string())).collect()
        }}
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
        fn player_message(&mut self, component: &str, overlay: bool) {{
            let (Some(uuid), Ok(json)) = (self.game.native_acting_player().map(|p| p.uuid), serde_json::from_str::<serde_json::Value>(component)) else {{
                return;
            }};
            let plain = json.get("text").or_else(|| json.get("translate")).and_then(|t| t.as_str()).unwrap_or("").to_owned();
            let line = horizon_client::text::Text::literal(plain).with_component(json);
            if overlay {{
                self.game.native_action_bar(uuid, line);
            }} else {{
                self.game.native_tell(uuid, line);
            }}
        }}
        fn spawn_experience_orb(&mut self, at: [f64; 3], value: u32, motion: Option<[f64; 3]>) -> bool {{
            self.game.native_spawn_experience_orb(at, value, motion)
        }}
        fn player_pov_hit(&self, fluids: u8) -> Option<([i32; 3], u8, [f64; 3])> {{
            // A face's step as a direction ordinal: down, up, north, south, west, east.
            let face = |f: [i32; 3]| match f {{
                [0, -1, 0] => 0,
                [0, 1, 0] => 1,
                [0, 0, -1] => 2,
                [0, 0, 1] => 3,
                [-1, 0, 0] => 4,
                _ => 5,
            }};
            self.game.native_player_pov_hit(fluids).map(|(pos, f, at)| (pos, face(f), at))
        }}
        fn player_hand(&self, hand: u8) -> Option<{c}::host::HandStack> {{
            Some(self.held_of(self.game.native_hand_stack(Self::hand(hand))?))
        }}
        fn player_id(&self) -> Option<u128> {{
            self.game.native_acting_player().map(|p| p.uuid)
        }}
        fn player_selected(&self) -> u8 {{
            self.game.native_selected_slot() as u8
        }}
        fn player_inventory(&self) -> bool {{
            true
        }}
        fn player_slot(&self, slot: u8) -> Option<{c}::host::HandStack> {{
            Some(self.held_of(self.game.native_slot_stack(usize::from(slot))?))
        }}
        fn set_player_slot(&mut self, slot: u8, held: Option<&{c}::host::HandStack>) -> bool {{
            let stack = held.filter(|h| !h.item.is_empty() && h.count > 0).map(|h| self.stack_of(h));
            self.game.native_set_slot_stack(usize::from(slot), stack)
        }}
        fn update_player_slot(&mut self, slot: u8, count: u32, damage: u32, components: &[(String, Option<{c}::nbt::Nbt>)]) -> bool {{
            let Some(mut stack) = self.game.native_slot_stack(usize::from(slot)).cloned() else {{
                return false;
            }};
            let mut all = true;
            for (name, value) in components {{
                all &= self.set_component(&mut stack, name, value.as_ref());
            }}
            stack.count = count;
            stack.damage = damage;
            self.game.native_set_slot_stack(usize::from(slot), Some(stack)) && all
        }}
        // What the player drops (a menu's throw, a stack carried out of it): the whole stack.
        fn spawn_item_entity(&mut self, item: &str, count: u32, pos: [f64; 3], motion: [f64; 3], pickup_delay: u32) -> Option<u32> {{
            let dim = self.game.native_dimension();
            self.game.native_spawn_item_entity(dim, horizon_client::items::ItemStack::new(item, count), pos, motion, pickup_delay)
        }}
        fn spawn_stack_entity(&mut self, held: &{c}::host::HandStack, pos: [f64; 3], motion: [f64; 3], pickup_delay: u32) -> Option<u32> {{
            let dim = self.game.native_dimension();
            let stack = self.stack_of(held);
            self.game.native_spawn_item_entity(dim, stack, pos, motion, pickup_delay)
        }}
        fn update_player_hand(&mut self, hand: u8, count: u32, damage: u32, components: &[(String, Option<{c}::nbt::Nbt>)]) -> bool {{
            let hand = Self::hand(hand);
            let Some(mut stack) = self.game.native_hand_stack(hand).cloned() else {{
                return false;
            }};
            let mut all = true;
            for (name, value) in components {{
                all &= self.set_component(&mut stack, name, value.as_ref());
            }}
            stack.count = count;
            stack.damage = damage;
            self.game.native_set_hand_stack(hand, Some(stack));
            all
        }}
        fn set_player_hand(&mut self, hand: u8, held: Option<&{c}::host::HandStack>) {{
            let hand = Self::hand(hand);
            let stack = held.filter(|h| !h.item.is_empty() && h.count > 0).map(|h| self.stack_of(h));
            self.game.native_set_hand_stack(hand, stack);
        }}
        fn hurt_player_hand(&mut self, hand: u8, amount: u32) -> bool {{
            self.game.native_hurt_hand_stack(Self::hand(hand), amount);
            true
        }}
{reads}{writes}{containers}{server_effects}    }}
    // A hand as `InteractionHand`'s ordinal.
    impl Host<'_, '_> {{
        fn hand(ordinal: u8) -> horizon_client::first_person::Hand {{
            if ordinal == 1 {{ horizon_client::first_person::Hand::Off }} else {{ horizon_client::first_person::Hand::Main }}
        }}
    }}
{stacks}"#)
}

/// The inherent helpers of a server host whose game is `self.game` (a `&mut Game`): a stack crosses
/// between the game and the runtime with its mod data as the runtime's NBT.
fn stack_helpers(c: &str) -> String {
    let nbt_of = nbt_of_fn(c);
    format!(r#"    // A stack's mod data crosses as the runtime's NBT.
    impl Host<'_, '_> {{
        /// The game's stack as the runtime's: its mod data by the registered component types, and
        /// the vanilla components the runtime models (its enchantments).
        fn held_of(&self, stack: &horizon_client::items::ItemStack) -> {c}::host::HandStack {{
            let mut components: Vec<_> = self
                .game
                .native_registered_components()
                .into_iter()
                .filter_map(|name| {{
                    let tag = self.game.native_component(stack, name).ok()??;
                    Some((name.to_owned(), Self::nbt_of(tag)?))
                }})
                .collect();
            for (name, tag) in self.game.native_vanilla_components(stack) {{
                components.extend(Self::nbt_of(&tag).map(|n| (name.to_owned(), n)));
            }}
            {c}::host::HandStack {{
                item: stack.item.to_string(),
                count: stack.count,
                damage: stack.damage,
                max_damage: self.game.native_item_max_damage(&stack.item),
                components,
            }}
        }}
        /// The runtime's stack as the game's, its mod data set by type.
        fn stack_of(&self, held: &{c}::host::HandStack) -> horizon_client::items::ItemStack {{
            let mut stack = horizon_client::items::ItemStack {{ item: held.item.as_str().into(), count: held.count, damage: held.damage, extra: None }};
            for (name, value) in &held.components {{
                self.set_component(&mut stack, name, Some(value));
            }}
            stack
        }}
        /// Sets component `name` on `stack` (None removes it): a registered type's, or a vanilla one
        /// the runtime carries (`minecraft:enchantments`); false when the game refuses it.
        fn set_component(&self, stack: &mut horizon_client::items::ItemStack, name: &str, value: Option<&{c}::nbt::Nbt>) -> bool {{
            let tag = match value.map(Self::tag_of) {{
                Some(None) => return false,
                tag => tag.flatten(),
            }};
            if name.starts_with("minecraft:") {{
                return self.game.native_set_vanilla_component(stack, name, tag.as_ref()).is_ok();
            }}
            match tag {{
                Some(tag) => self.game.native_set_component(stack, name, tag).is_ok(),
                None => self.game.native_remove_component(stack, name).is_ok(),
            }}
        }}
{nbt_of}        fn tag_of(n: &{c}::nbt::Nbt) -> Option<horizon_world::nbt::Tag> {{
            use horizon_world::nbt::{{Compound, JavaString, Tag}};
            use {c}::nbt::Nbt;
            Some(match n {{
                Nbt::Byte(b) => Tag::Byte(*b),
                Nbt::Short(s) => Tag::Short(*s),
                Nbt::Int(i) => Tag::Int(*i),
                Nbt::Long(l) => Tag::Long(*l),
                Nbt::Float(f) => Tag::Float(f.to_bits()),
                Nbt::Double(d) => Tag::Double(d.to_bits()),
                Nbt::String(s) => Tag::String(JavaString::from(s.as_str())),
                Nbt::Compound(c) => Tag::Compound(c.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), Self::tag_of(v)?))).collect::<Compound>()),
                Nbt::List(l) => Tag::List(l.iter().filter_map(Self::tag_of).collect()),
                Nbt::ByteArray(a) => Tag::ByteArray(a.iter().map(|b| *b as u8).collect()),
                Nbt::IntArray(a) => Tag::IntArray(a.clone()),
                Nbt::LongArray(a) => Tag::LongArray(a.clone()),
            }})
        }}
    }}
"#)
}

/// A server host's vanilla containers (`self.game` a `&mut Game`), as NeoForge's item handlers of
/// them reach them; [`stack_helpers`] carry their stacks.
fn container_methods(c: &str) -> String {
    format!(
        r#"        fn container(&self, pos: [i32; 3]) -> Option<{c}::host::ContainerFacts> {{
            use crate::game::machines::native_items::NativeContainerKind as Kind;
            use {c}::host::ContainerKind;
            self.game.native_vanilla_container(pos).map(|(size, max_stack, kind)| {c}::host::ContainerFacts {{
                size,
                max_stack,
                kind: match kind {{
                    Kind::Plain => ContainerKind::Plain,
                    Kind::Hopper => ContainerKind::Hopper,
                    Kind::Furnace => ContainerKind::Furnace,
                    Kind::Brewing => ContainerKind::Brewing,
                    Kind::Shulker => ContainerKind::Shulker,
                }},
            }})
        }}
        fn container_item(&self, pos: [i32; 3], slot: u32) -> Option<{c}::host::HandStack> {{
            self.game.native_vanilla_container_item(pos, slot).map(|s| self.held_of(&s))
        }}
        fn set_container_item(&mut self, pos: [i32; 3], slot: u32, stack: Option<&{c}::host::HandStack>) -> bool {{
            let stack = stack.filter(|h| !h.item.is_empty() && h.count > 0).map(|h| self.stack_of(h));
            self.game.native_vanilla_container_set(pos, slot, stack)
        }}
        fn container_changed(&mut self, pos: [i32; 3]) {{
            self.game.native_vanilla_container_changed(pos);
        }}
        fn container_can_place(&self, pos: [i32; 3], slot: u32, stack: &{c}::host::HandStack) -> bool {{
            self.game.native_vanilla_container_can_place(pos, slot, &self.stack_of(stack), false)
        }}
        fn container_can_place_through(&self, pos: [i32; 3], slot: u32, stack: &{c}::host::HandStack) -> bool {{
            self.game.native_vanilla_container_can_place(pos, slot, &self.stack_of(stack), true)
        }}
        fn container_face_slots(&self, pos: [i32; 3], face: u8) -> Vec<u32> {{
            let face = horizon_core::geometry::Direction::ALL[usize::from(face.min(5))];
            self.game.native_vanilla_container_face_slots(pos, face)
        }}
        fn container_can_take(&self, pos: [i32; 3], slot: u32, stack: &{c}::host::HandStack, face: u8) -> bool {{
            let face = horizon_core::geometry::Direction::ALL[usize::from(face.min(5))];
            self.game.native_vanilla_container_can_take(pos, slot, &self.stack_of(stack), face)
        }}
        fn hopper_filled(&mut self, pos: [i32; 3]) {{
            self.game.native_hopper_filled(pos);
        }}
"#
    )
}

fn block_entity_helpers(id: &str, krate: &str) -> Vec<(&'static str, String)> {
    let c = format!("{krate}::compat");
    // The block a hopper, dropper or crafter faces: `facing`'s 3D data value (down, up, north,
    // south, west, east) and the position next to `pos` that way.
    let front = "    let d = facing as u8;\n    let front = match d {\n        0 => [pos[0], pos[1] - 1, pos[2]],\n        1 => [pos[0], pos[1] + 1, pos[2]],\n        2 => [pos[0], pos[1], pos[2] - 1],\n        3 => [pos[0], pos[1], pos[2] + 1],\n        4 => [pos[0] - 1, pos[1], pos[2]],\n        _ => [pos[0] + 1, pos[1], pos[2]],\n    };\n";
    let conversions = nbt_conversions(&c);
    let (reads, writes) = (world_reads("self.game", &c), world_writes(&c));
    let server_send = server_send(&c);
    let server_effects = server_effects(&c);
    // A player's hooks reach their client too: a menu the mod opens, a payload it sends.
    let player_host = player_host(&c, &format!("{server_effects}{server_send}"));
    let host = game_host(&c, &server_send);
    let (read_host, level_host) = (level_read_host(&c), level_host(krate));
    // A hit's hooks have no game at hand: the hurt mob as the game holds it then, and a host of
    // what the game told of the hit.
    let hit_host = hit_host(&c, &server_send);
    let seen = format!(
        "crate::game::native_hooks::native_hit_entity().filter(|h| h.entity == entity).map(|h| {c}::host::EntityView {{ id: entity, kind: kind.to_owned(), pos: h.position, yaw: h.yaw, health: Some((h.health, h.max_health)) }})"
    );
    // A server player's own events, on the server's game with no player's state swapped in: the
    // mod reaches the player by their object (a payload to them, their inventory).
    let player_hook = |hook: &str, what: &str| {
        format!(r#"// Generated by horizon-mod-translator: {what} (`native_{hook}`), on the server's game.
fn {id}_{hook}(game: &mut crate::game::Game<'_>, player: u128, name: &str) {{
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        // No level is swapped in for a player's event: no blocks here.
        fn block_state(&self, _pos: [i32; 3]) -> String {{
            "minecraft:air".into()
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn game_time(&self) -> i64 {{
            self.game.native_game_time()
        }}
        // Every player on the server, whichever level they are in.
        fn players(&self) -> Vec<(u128, [f64; 3])> {{
            self.game.native_server_players().into_iter().map(|p| (p.uuid, p.position)).collect()
        }}
{reads}{writes}{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::hook_runtime("{hook}", |rt| {c}::glue::{hook}(rt, &mut host, player, name));
}}
"#)
    };
    // The server's lifecycle, on the server's game between levels.
    let server_hook = |hook: &str, params: &str, what: &str, call: &str| {
        format!(r#"// Generated by horizon-mod-translator: {what} (`native_{hook}`), on the server's game.
fn {id}_{hook}(game: &mut crate::game::Game<'_>, {params}) {{
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        // Between levels: no blocks here.
        fn block_state(&self, _pos: [i32; 3]) -> String {{
            "minecraft:air".into()
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn game_time(&self) -> i64 {{
            self.game.native_game_time()
        }}
        // Every player on the server, whichever level they are in.
        fn players(&self) -> Vec<(u128, [f64; 3])> {{
            self.game.native_server_players().into_iter().map(|p| (p.uuid, p.position)).collect()
        }}
{reads}{writes}{server_send}    }}
    let mut host = Host {{ game }};
    {call}
}}
"#)
    };
    let about = format!("let _ = dedicated;\n    {krate}::hook_runtime(\"server_about_to_start\", |rt| {c}::glue::server_about_to_start(rt, &mut host));");
    let stopping = format!("let _ = dedicated;\n    {krate}::hook_runtime(\"server_stopping\", |rt| {c}::glue::server_stopping(rt, &mut host));");
    let phases = "let _ = (context, error_text);\n    use crate::game::native_lifecycle::WorldPhase;\n    let phase = match phase {\n        WorldPhase::WorldOpenReady => \"open_ready\",\n        WorldPhase::WorldOpenFailed => \"open_failed\",\n        WorldPhase::WorldCloseFinal => \"close_final\",\n        WorldPhase::WorldCloseFailed => \"close_failed\",\n    };\n";
    let lifecycle = format!("{phases}    {krate}::hook_runtime(\"world_lifecycle\", |rt| {c}::glue::world_lifecycle(rt, &mut host, phase));");
    // The mods' commands: registered when a world opens to players, run for a player (their state
    // swapped in) or the console; what a command says is its feedback, to whoever ran it.
    let register = format!(
        "for (usage, level) in {krate}::hook_runtime(\"register_commands\", |rt| {c}::glue::register_commands(rt, &mut host)) {{\n        commands.push(crate::game::native_server::NativeCommand::op(&usage, level));\n    }}"
    );
    let feedback = r#"        fn command_feedback(&mut self, component: &str) {
            let Ok(json) = serde_json::from_str::<serde_json::Value>(component) else {
                return;
            };
            let plain = json.get("text").or_else(|| json.get("translate")).and_then(|t| t.as_str()).unwrap_or("").to_owned();
            self.game.native_feedback(horizon_client::text::Text::literal(plain).with_component(json));
        }
        fn player_name(&self, uuid: u128) -> Option<String> {
            self.game.native_server_players().into_iter().find(|p| p.uuid == uuid).map(|p| p.name)
        }
"#;
    let command_host = self::player_host(&c, &format!("{server_effects}{server_send}{feedback}"));
    let console_host = game_host(&c, &format!("{server_send}{feedback}"));
    let perform = format!(r#"// Generated by horizon-mod-translator: a player's command for the mods' commands
// (`native_perform_command`), with the player's state swapped in.
fn {id}_perform_command(game: &mut crate::game::Game<'_>, player: &crate::game::native_server::NativeServerPlayer, command: &str, ran: &mut bool) {{
    if *ran {{
        return;
    }}
{command_host}    let mut host = Host {{ game }};
    let (who, permission, at, rot) = (Some((player.uuid, player.name.as_str())), player.permission, player.position, [player.yaw, player.pitch]);
    *ran = {krate}::hook_runtime("perform_command", |rt| {c}::glue::perform_command(rt, &mut host, who, permission, at, rot, command));
}}
"#);
    let console = format!(r#"// Generated by horizon-mod-translator: the console's command for the mods' commands
// (`native_perform_console_command`): no player, level 4.
fn {id}_perform_console_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {{
    if *ran {{
        return;
    }}
{console_host}    let mut host = Host {{ game }};
    *ran = {krate}::hook_runtime("perform_console_command", |rt| {c}::glue::perform_command(rt, &mut host, None, 4, [0.0; 3], [0.0; 2], command));
}}
"#);
    // An effect given: its code runs once the action or the tick that gave it is over, as the
    // effect may be given from inside the mod's own call (`addEffect`), where the runtime is busy.
    let effect_added = |hook: &str, started: bool, what: &str| {
        format!(r#"// Generated by horizon-mod-translator: {what}.
// Its code runs once the player's action or the level's tick that gave it is over: a mod's own
// `addEffect` gives it from inside the mod's call.
fn {id}_{hook}(game: &mut crate::game::Game<'_>, effect: &str, entity: Option<i32>, amplifier: i32) {{
    let _ = game;
    if !{krate}::ours(effect) {{
        return;
    }}
    let effect = effect.to_owned();
    crate::game::native_hits::native_after_hit(move |game| {{
{player_host}        let mut host = Host {{ game }};
        {krate}::hook_runtime("{hook}", |rt| {c}::mob_effects::effect_added(rt, &mut host, &effect, entity, amplifier, {started}));
    }});
}}
"#)
    };
    vec![
        ("register_commands", server_hook("register_commands", "commands: &mut Vec<crate::game::native_server::NativeCommand>", "the server's commands (`RegisterCommandsEvent`)", &register)),
        ("perform_command", perform),
        ("perform_console_command", console),
        ("server_about_to_start", server_hook("server_about_to_start", "dedicated: bool", "the server is about to load its worlds (`ServerAboutToStartEvent`)", &about)),
        ("server_stopping", server_hook("server_stopping", "dedicated: bool", "the server begins to stop (`ServerStoppingEvent`)", &stopping)),
        ("world_lifecycle", server_hook(
            "world_lifecycle",
            "context: &crate::game::native_lifecycle::NativeLifecycleContext, phase: crate::game::native_lifecycle::WorldPhase, error_text: &str",
            "the world opened (`ServerStartingEvent`, `ServerStartedEvent`) or closed (`LevelEvent.Unload`, `ServerStoppedEvent`)",
            &lifecycle,
        )),
        ("level_tick_pre", level_tick(id, krate, &c, &host, false)),
        ("level_tick_post", level_tick(id, krate, &c, &host, true)),
        ("server_tick", format!(r#"// Generated by horizon-mod-translator: the server's tick (`ServerTickEvent.Pre` and `.Post`), on
// the server's game.
fn {id}_server_tick(game: &mut crate::game::Game<'_>, phase: crate::game::native_lifecycle::ServerTickPhase) {{
    let post = matches!(phase, crate::game::native_lifecycle::ServerTickPhase::ServerTickEnd {{ .. }});
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        // Which level the server's game holds between levels is not defined: no blocks here.
        fn block_state(&self, _pos: [i32; 3]) -> String {{
            "minecraft:air".into()
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn game_time(&self) -> i64 {{
            self.game.native_game_time()
        }}
{reads}{writes}{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::hook_runtime("server_tick", |rt| {c}::glue::server_tick(rt, &mut host, post));
}}
"#)),
        ("server_data_loaded", format!(r#"// Generated by horizon-mod-translator: the server's data loaded (`native_server_data_loaded`: a
// world's first open, each `/reload`), on the server's game: the mods' data reload listeners.
fn {id}_server_data_loaded(game: &mut crate::game::Game<'_>, reload: bool) {{
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        // Between levels: no blocks here.
        fn block_state(&self, _pos: [i32; 3]) -> String {{
            "minecraft:air".into()
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn game_time(&self) -> i64 {{
            self.game.native_game_time()
        }}
        // Every player on the server (`/reload`'s data goes to all of them).
        fn players(&self) -> Vec<(u128, [f64; 3])> {{
            self.game.native_server_players().into_iter().map(|p| (p.uuid, p.position)).collect()
        }}
{reads}{writes}{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::hook_runtime("server_data_loaded", |rt| {c}::glue::server_data_loaded(rt, &mut host, reload));
}}
"#)),
        ("player_logged_in", player_hook("player_logged_in", "a player joined")),
        ("player_logged_out", player_hook("player_logged_out", "a player left")),
        ("player_respawned", player_hook("player_respawned", "a player respawned after dying")),
        ("player_tick", format!(r#"// Generated by horizon-mod-translator: the end of a player's server tick (`ServerPlayer.tick`),
// their state swapped in: what their client sent the mod's menus runs, and the menu they have open
// keeps their client in step and closes when it may no longer stay open. Then the mods hear the
// tick (NeoForge's `PlayerTickEvent`).
fn {id}_player_tick(game: &mut crate::game::Game<'_>) {{
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("player_tick", |rt| {c}::glue::player_tick(rt, &mut host));
}}
"#)),
        ("destroy_block", format!(r#"// Generated by horizon-mod-translator: a player breaks a block (`native_destroy_block`: where
// `ServerPlayerGameMode.destroyBlock` starts, once the game allowed it), their state swapped in. The
// mods hear it as NeoForge's `BlockEvent.BreakEvent`; one that cancels it keeps the block.
fn {id}_destroy_block(game: &mut crate::game::Game<'_>, pos: [i32; 3], state: horizon_world::registry::BlockStateId, canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
    // The block still there, as the mods read states.
    let _ = state;
    let state = game.native_state_text_at(pos);
{player_host}    let mut host = Host {{ game }};
    if {krate}::hook_runtime("destroy_block", |rt| {c}::glue::destroy_block(rt, &mut host, pos, &state)) {{
        *canceled = true;
    }}
}}
"#)),
        ("block_replaced", format!(r#"// Generated by horizon-mod-translator: a block changed on the server, the old or the new one the
// translated mod's (`native_block_replaced`: 1.21.1's `BlockBehaviour.onRemove` and `onPlace`
// point, once the level operation is done): the old block's own `onRemove` runs (a machine drops
// what it held, a tank splits), then the new one's `onPlace`. A change the mod's own code made
// reaches it while that code still runs: the runtime runs it once the change is back.
fn {id}_block_replaced(game: &mut crate::game::Game<'_>, pos: [i32; 3], old: horizon_world::registry::BlockStateId, new: horizon_world::registry::BlockStateId, moved: bool) {{
    let old = game.native_state_id_text_of(old);
    let new = game.native_state_id_text_of(new);
    if !{krate}::ours(&old) && !{krate}::ours(&new) {{
        return;
    }}
    if {krate}::runtime_busy() {{
        {c}::glue::defer_replaced(pos, &old, &new, moved);
        return;
    }}
{host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("block_replaced", |rt| {c}::glue::block_replaced(rt, &mut host, pos, &old, &new, moved));
}}
"#)),
        ("loot_drops", format!(r#"// Generated by horizon-mod-translator: a broken block's drops, rolled from its loot table
// (`native_loot_drops`: where `Block.getDrops` rolls it for a player's break or an explosion). A
// translated mod's block with its own `getDrops(state, LootParams.Builder)` answers for them, its
// `super.` call getting the game's stacks: Create's belt adds the shaft its pulley held, its backtank
// keeps its air. A stack it left as it was stays the game's own.
fn {id}_loot_drops(game: &mut crate::game::Game<'_>, context: &crate::game::native_hooks::NativeLootDrops<'_>, drops: &mut Vec<horizon_client::items::ItemStack>) {{
    let Some((pos, state)) = context.block.filter(|_| !context.used) else {{
        return;
    }};
    let state = game.native_state_id_text_of(state);
    if !{krate}::ours(&state) {{
        return;
    }}
{player_host}    let mut host = Host {{ game }};
    let rolled: Vec<_> = drops.iter().map(|d| host.held_of(d)).collect();
    let tool = context.tool.map(|t| host.held_of(t));
    let (player, explosion) = (context.player, context.explosion);
    if let Some(answer) = {krate}::hook_runtime("loot_drops", |rt| {c}::glue::loot_drops(rt, &mut host, pos, &state, &rolled, tool, player, explosion)) {{
        let kept: Vec<_> = answer
            .iter()
            .map(|(origin, held)| origin.and_then(|i| drops.get(i)).map_or_else(|| host.stack_of(held), Clone::clone))
            .collect();
        *drops = kept;
    }}
}}
"#)),
        ("place_block", format!(r#"// Generated by horizon-mod-translator: a player placed a block with an item (`native_place_block`:
// set, before the stack shrinks), their state swapped in. A mod's block entity reads the stack's
// components and its block hears `setPlacedBy`, as `BlockItem.place` goes on; then the mods hear
// it as NeoForge's `BlockEvent.EntityPlaceEvent`.
fn {id}_place_block(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, pos: [i32; 3], state: horizon_world::registry::BlockStateId) {{
    // The block now there, as the mods read states.
    let _ = state;
    let state = game.native_state_text_at(pos);
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("place_block", |rt| {c}::glue::place_block(rt, &mut host, pos, &state, &stack.item));
}}
"#)),
        ("attack_entity", format!(r#"// Generated by horizon-mod-translator: a player attacks an entity (`native_attack_entity`: where
// `Player.attack` starts), their state swapped in. The mods hear it as NeoForge's
// `AttackEntityEvent`; one that cancels it stops the blow.
fn {id}_attack_entity(game: &mut crate::game::Game<'_>, entity: i32, kind: &str, pos: [f64; 3], canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
{player_host}    let mut host = Host {{ game }};
    if {krate}::hook_runtime("attack_entity", |rt| {c}::events::attack_entity(rt, &mut host, entity, kind, pos)) {{
        *canceled = true;
    }}
}}
"#)),
        ("living_incoming_damage", format!(r#"// Generated by horizon-mod-translator: a hit is about to hurt an entity (`native_living_incoming_damage`:
// `LivingEntity.hurtServer` past its guards), inside the game's damage code. The mods hear it as
// NeoForge's `LivingIncomingDamageEvent`; one may change the amount or cancel the hit.
fn {id}_living_incoming_damage(entity: i32, kind: &str, damage_type: &str, causing: i32, direct: i32, amount: &mut f32, canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
    let seen = {seen};
    let source = {c}::damage::Source {{ damage_type, causing, direct }};
{hit_host}    if {krate}::hook_runtime("living_incoming_damage", |rt| {c}::damage::incoming(rt, &mut host, entity, kind, seen.as_ref(), source, &mut *amount)) {{
        *canceled = true;
    }}
}}
"#)),
        ("living_knockback", format!(r#"// Generated by horizon-mod-translator: an entity is about to be knocked back
// (`native_living_knockback`: `LivingEntity.knockback`). The mods hear it as NeoForge's
// `LivingKnockBackEvent`; one may change the push or cancel it.
fn {id}_living_knockback(entity: i32, kind: &str, damage_type: &str, causing: i32, direct: i32, strength: &mut f64, x: &mut f64, z: &mut f64, canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
    // NeoForge's event names no source; its attacker has hurt the entity by then (`getKillCredit`).
    let _ = (damage_type, direct);
    let seen = {seen};
{hit_host}    if {krate}::hook_runtime("living_knockback", |rt| {c}::damage::knockback(rt, &mut host, entity, kind, seen.as_ref(), causing, &mut *strength, &mut *x, &mut *z)) {{
        *canceled = true;
    }}
}}
"#)),
        ("living_damage_pre", format!(r#"// Generated by horizon-mod-translator: what is left of a hit once armour took its share
// (`native_living_damage_pre`, in `LivingEntity.actuallyHurt`). The mods hear it as NeoForge's
// `LivingDamageEvent.Pre`, and may change it.
fn {id}_living_damage_pre(entity: i32, kind: &str, damage_type: &str, causing: i32, direct: i32, amount: &mut f32) {{
    let seen = {seen};
    let source = {c}::damage::Source {{ damage_type, causing, direct }};
{hit_host}    {krate}::hook_runtime("living_damage_pre", |rt| {c}::damage::damage_pre(rt, &mut host, entity, kind, seen.as_ref(), source, &mut *amount));
}}
"#)),
        ("living_damage_post", format!(r#"// Generated by horizon-mod-translator: what of a hit reached the health
// (`native_living_damage_post`). The mods hear it as NeoForge's `LivingDamageEvent.Post`.
fn {id}_living_damage_post(entity: i32, kind: &str, damage_type: &str, causing: i32, direct: i32, amount: f32) {{
    let seen = {seen};
    let source = {c}::damage::Source {{ damage_type, causing, direct }};
{hit_host}    {krate}::hook_runtime("living_damage_post", |rt| {c}::damage::damage_post(rt, &mut host, entity, kind, seen.as_ref(), source, amount));
}}
"#)),
        ("living_death", format!(r#"// Generated by horizon-mod-translator: a mob dies (`native_living_death`: where
// `LivingEntity.die` starts). The mods hear it as NeoForge's `LivingDeathEvent`; one that cancels it
// (and gives health back) keeps the mob alive.
fn {id}_living_death(game: &mut crate::game::Game<'_>, entity: i32, kind: &str, damage_type: &str, causing: i32, direct: i32, canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
{host}    let mut host = Host {{ game }};
    let source = {c}::damage::Source {{ damage_type, causing, direct }};
    if {krate}::hook_runtime("living_death", |rt| {c}::damage::death(rt, &mut host, entity, kind, source)) {{
        *canceled = true;
    }}
}}
"#)),
        ("living_drops", format!(r#"// Generated by horizon-mod-translator: a dead mob's drops before they join the level
// (`native_living_drops`: in `LivingEntity.dropAllDeathLoot`). The mods hear them as NeoForge's
// `LivingDropsEvent`, item entities they may change, add to or take from; one that cancels it keeps
// them all out. A drop whose stack they left keeps the game's own stack.
fn {id}_living_drops(game: &mut crate::game::Game<'_>, entity: i32, kind: &str, damage_type: &str, causing: i32, direct: i32, recently_hit: bool, drops: &mut Vec<crate::game::native_hooks::NativeDrop>, canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
{player_host}    let mut host = Host {{ game }};
    let source = {c}::damage::Source {{ damage_type, causing, direct }};
    let mut items: Vec<{c}::damage::Drop> = drops
        .iter()
        .map(|d| {c}::damage::Drop {{ stack: host.held_of(&d.stack), position: d.position, motion: d.motion, pickup_delay: d.pickup_delay, origin: None }})
        .collect();
    match {krate}::hook_runtime("living_drops", |rt| {c}::damage::living_drops(rt, &mut host, entity, kind, source, recently_hit, &mut items)) {{
        Some(true) => *canceled = true,
        Some(false) => {{
            let kept: Vec<_> = items
                .iter()
                .map(|d| crate::game::native_hooks::NativeDrop {{
                    stack: d.origin.and_then(|i| drops.get(i)).map_or_else(|| host.stack_of(&d.stack), |o| o.stack.clone()),
                    position: d.position,
                    motion: d.motion,
                    pickup_delay: d.pickup_delay,
                }})
                .collect();
            *drops = kept;
        }}
        None => {{}}
    }}
}}
"#)),
        ("should_apply_effect_tick_this_tick", format!(r#"// Generated by horizon-mod-translator: whether a translated mod's effect acts this tick
// (`native_should_apply_effect_tick_this_tick`: `MobEffect.shouldApplyEffectTickThisTick`), its own
// code, which needs no world.
fn {id}_should_apply_effect_tick_this_tick(game: &crate::game::Game<'_>, effect: &str, tick_count: i32, amplifier: i32, apply: &mut bool) {{
    let _ = game;
    if !{krate}::ours(effect) {{
        return;
    }}
    if let Some(answer) = {krate}::hook_runtime("should_apply_effect_tick_this_tick", |rt| {c}::mob_effects::should_apply_tick(rt, effect, tick_count, amplifier)) {{
        *apply = answer;
    }}
}}
"#)),
        ("apply_effect_tick", format!(r#"// Generated by horizon-mod-translator: a translated mod's effect acts on a player or a mob
// (`native_apply_effect_tick`: `MobEffect.applyEffectTick`). Inside the entity's tick the game is
// read-only, so the effect's own code runs once the player's action or the level's tick is over,
// with the game at hand; an effect whose code says it ends is taken off then.
fn {id}_apply_effect_tick(game: &crate::game::Game<'_>, effect: &str, dimension: &str, entity: Option<i32>, amplifier: i32, keep: &mut bool) {{
    let _ = (game, dimension, keep);
    if !{krate}::ours(effect) {{
        return;
    }}
    let effect = effect.to_owned();
    crate::game::native_hits::native_after_hit(move |game| {{
{player_host}        let mut host = Host {{ game }};
        let stays = {krate}::hook_runtime("apply_effect_tick", |rt| {c}::mob_effects::apply_tick(rt, &mut host, &effect, entity, amplifier));
        if stays == Some(false) {{
            host.game.native_remove_effect(entity, &effect);
        }}
    }});
}}
"#)),
        ("on_effect_added", effect_added("on_effect_added", false, "a translated mod's effect was given to an entity that hadn't had it (`native_on_effect_added`:\n// `MobEffect.onEffectAdded`)")),
        ("mob_effect_added", effect_added("mob_effect_added", true, "a translated mod's effect was given to an entity, new or not (`native_mob_effect_added`:\n// `MobEffect.onEffectStarted`)")),
        ("player_died", format!(r#"// Generated by horizon-mod-translator: a player dies (`native_player_died`: where
// `ServerPlayer.die` starts). The mods hear it as NeoForge's `LivingDeathEvent`, of the damage type
// the death message names.
fn {id}_player_died(game: &mut crate::game::Game<'_>, player: u128, cause: Option<&str>) {{
{host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("player_died", |rt| {c}::damage::player_died(rt, &mut host, player, cause));
}}
"#)),
        ("interact_entity", format!(r#"// Generated by horizon-mod-translator: a player right-clicks an entity (`native_interact_entity`:
// where `Player.interactOn` starts, once per hand), their state swapped in. The mods hear it as
// NeoForge's `PlayerInteractEvent.EntityInteractSpecific`, then `.EntityInteract`; one that cancels
// it takes the click.
fn {id}_interact_entity(game: &mut crate::game::Game<'_>, entity: i32, kind: &str, stack: Option<&horizon_client::items::ItemStack>, claimed: &mut bool) {{
    if *claimed {{
        return;
    }}
    let _ = stack;
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
{player_host}    let mut host = Host {{ game }};
    if {krate}::hook_runtime("interact_entity", |rt| {c}::events::interact_entity(rt, &mut host, entity, kind, hand)).is_some() {{
        *claimed = true;
    }}
}}
"#)),
        ("net_from_client", format!(r#"// Generated by horizon-mod-translator: a mod payload a player's client sent
// (`native_net_from_client`), to the translated mod's handler for its type.
fn {id}_net_from_client(game: &mut crate::game::Game<'_>, player: u128, channel: &str, data: &[u8]) {{
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false
        }}
        fn game_time(&self) -> i64 {{
            self.game.native_game_time()
        }}
{reads}{writes}{server_effects}{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::hook_runtime("net_from_client", |rt| {c}::glue::payload_from_client(rt, &mut host, player, channel, data));
}}
"#)),
        ("block_entity", format!(r#"// Generated by horizon-mod-translator: `EntityBlock.newBlockEntity` of the translated mod.
fn {id}_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {{
    if entity.is_some() || !{krate}::ours(&block) {{
        return;
    }}
    if let Some(be) = {krate}::with_registrations(|rt| {c}::glue::block_entity_id(rt, block)) {{
        *entity = Some((be, horizon_world::nbt::Compound::new()));
    }}
}}
"#)),
        ("block_entity_ticks", format!(r#"// Generated by horizon-mod-translator: `EntityBlock.getTicker` of the translated mod.
fn {id}_block_entity_ticks(block: &str, ticks: &mut bool) {{
    if {krate}::ours(&block) && {krate}::with_registrations(|rt| {c}::glue::block_entity_ticks(rt, block)) {{
        *ticks = true;
    }}
}}
"#)),
        ("block_entity_tick", format!(r#"// Generated by horizon-mod-translator: a server tick of the translated mod's block entity.
fn {id}_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {{
    use horizon_world::nbt::{{Compound, JavaString, Tag}};
    use {c}::nbt::Nbt;
    use std::collections::BTreeMap;
    if !{krate}::ours(&entity.id) {{
        return;
    }}
    fn to_nbt(tag: &Tag) -> Option<Nbt> {{
        Some(match tag {{
            Tag::Byte(b) => Nbt::Byte(*b),
            Tag::Short(s) => Nbt::Short(*s),
            Tag::Int(i) => Nbt::Int(*i),
            Tag::Long(l) => Nbt::Long(*l),
            Tag::Float(f) => Nbt::Float(f32::from_bits(*f)),
            Tag::Double(d) => Nbt::Double(f64::from_bits(*d)),
            Tag::String(s) => Nbt::String(s.to_string_lossy()),
            Tag::Compound(c) => Nbt::Compound(c.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect()),
            Tag::List(l) => Nbt::List(l.iter().filter_map(to_nbt).collect()),
            Tag::ByteArray(a) => Nbt::ByteArray(a.iter().map(|b| *b as i8).collect()),
            Tag::IntArray(a) => Nbt::IntArray(a.clone()),
            Tag::LongArray(a) => Nbt::LongArray(a.clone()),
            Tag::End => return None,
        }})
    }}
    fn to_tag(n: &Nbt) -> Option<Tag> {{
        Some(match n {{
            Nbt::Byte(b) => Tag::Byte(*b),
            Nbt::Short(s) => Tag::Short(*s),
            Nbt::Int(i) => Tag::Int(*i),
            Nbt::Long(l) => Tag::Long(*l),
            Nbt::Float(f) => Tag::Float(f.to_bits()),
            Nbt::Double(d) => Tag::Double(d.to_bits()),
            Nbt::String(s) => Tag::String(JavaString::from(s.as_str())),
            Nbt::Compound(c) => Tag::Compound(c.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), to_tag(v)?))).collect::<Compound>()),
            Nbt::List(l) => Tag::List(l.iter().filter_map(to_tag).collect()),
            Nbt::ByteArray(a) => Tag::ByteArray(a.iter().map(|b| *b as u8).collect()),
            Nbt::IntArray(a) => Tag::IntArray(a.clone()),
            Nbt::LongArray(a) => Tag::LongArray(a.clone()),
        }})
    }}
{host}    let state = game.native_state_text_at(pos);
    let mut nbt: BTreeMap<String, Nbt> = entity.nbt.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    let mut host = Host {{ game }};
    if {krate}::hook_runtime("block_entity_tick", |rt| {c}::glue::block_entity_tick(rt, &mut host, pos, &state, &mut nbt)) {{
        // What the block entity saved, all of it: keys it dropped are gone.
        entity.nbt = nbt.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), to_tag(v)?))).collect::<Compound>();
    }}
}}
"#)),
        ("block_entity_components", format!(r#"// Generated by horizon-mod-translator: what the translated mod's block entity gives the item its
// block drops (`BlockEntity.collectComponents`: what it kept of the stack it was placed from, then
// its own `collectImplicitComponents`), each as its codec writes it, as a player or an explosion
// breaks the block; the game keeps those the block's loot table copies.
fn {id}_block_entity_components(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &horizon_client::containers::ModBlockEntity,
    components: &mut Vec<(String, horizon_world::nbt::Tag)>,
) {{
{conversions}    use std::collections::BTreeMap;
    if !{krate}::ours(&entity.id) {{
        return;
    }}
{host}    let state = game.native_state_text_at(pos);
    let nbt: BTreeMap<String, Nbt> = entity.nbt.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    let mut host = Host {{ game }};
    let named = {krate}::hook_runtime("block_entity_components", |rt| {c}::glue::block_entity_components(rt, &mut host, pos, &state, &nbt));
    components.extend(named.iter().filter_map(|(name, value)| Some((name.clone(), to_tag(value)?))));
}}
"#)),
        ("right_click_block", format!(r#"// Generated by horizon-mod-translator: a right click on a block. First the mod's listeners
// hear it (NeoForge's `PlayerInteractEvent.RightClickBlock`, on any block: Create's manual recipes,
// filters, value settings, links); one that cancels it stops the click. Then, on the translated
// mod's own block, its `useItemOn` with the acting hand's stack and, for the main hand,
// `useWithoutItem`. A click either takes is canceled for the game, which would otherwise use the
// held item on the block.
fn {id}_right_click_block(game: &mut crate::game::Game<'_>, pos: [i32; 3], face: [i32; 3], canceled: &mut bool) {{
    if *canceled {{
        return;
    }}
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
    // Where the click landed: as the player's client aimed it (a value slot or a filter is a small
    // part of the face), else their look traced on the server when it meets this block, else the
    // face's centre.
    let centre = |i: usize| pos[i] as f64 + 0.5 + face[i] as f64 * 0.5;
    let hit = game
        .native_click_hit()
        .filter(|h| h.0 == pos)
        .or_else(|| game.native_player_pov_hit(0).filter(|h| h.0 == pos))
        .map_or([centre(0), centre(1), centre(2)], |h| h.2);
    // Direction's 3D data value: down, up, north, south, west, east.
    let face = match face {{
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        _ => 5,
    }};
{player_host}    let mut host = Host {{ game }};
    if {krate}::hook_runtime("right_click_block", |rt| {c}::events::right_click_block(rt, &mut host, pos, face, hit, hand)).is_some() {{
        *canceled = true;
        return;
    }}
    let state = host.game.native_state_text_at(pos);
    if !{krate}::ours(&state) {{
        return;
    }}
    if {krate}::hook_runtime("right_click_block", |rt| {c}::glue::use_block(rt, &mut host, pos, &state, face, hit, hand)) {{
        *canceled = true;
    }}
}}
"#)),
        ("hopper_insert", format!(r#"// Generated by horizon-mod-translator: a hopper pushing toward one of the translated mod's
// blocks, which has no container of the game's own: NeoForge's `VanillaInventoryCodeHooks.insertHook`
// puts one of its items into the block's item handler (`Capabilities.ItemHandler.BLOCK`).
fn {id}_hopper_insert(game: &mut crate::game::Game<'_>, pos: [i32; 3], facing: horizon_core::geometry::Direction, moved: &mut bool) {{
    if *moved {{
        return;
    }}
{front}    if !{krate}::ours(&game.native_block(front)) {{
        return;
    }}
{host}    let mut host = Host {{ game }};
    *moved = {krate}::hook_runtime("hopper_insert", |rt| {c}::glue::hopper_insert(rt, &mut host, pos, d)) == Some(true);
}}
"#)),
        ("hopper_extract", format!(r#"// Generated by horizon-mod-translator: a hopper under one of the translated mod's blocks, which
// has no container of the game's own: NeoForge's `VanillaInventoryCodeHooks.extractHook` takes one
// item from the block's item handler; a block with one leaves the items lying on it alone.
fn {id}_hopper_extract(game: &mut crate::game::Game<'_>, pos: [i32; 3], took: &mut Option<bool>) {{
    if took.is_some() || !{krate}::ours(&game.native_block([pos[0], pos[1] + 1, pos[2]])) {{
        return;
    }}
{host}    let mut host = Host {{ game }};
    *took = {krate}::hook_runtime("hopper_extract", |rt| {c}::glue::hopper_extract(rt, &mut host, pos));
}}
"#)),
        ("dropper_insert", format!(r#"// Generated by horizon-mod-translator: a dropper facing one of the translated mod's blocks, which
// has no container of the game's own: NeoForge's `VanillaInventoryCodeHooks.dropperInsertHook` puts
// one item of the slot into the block's item handler, or keeps it there; nothing drops.
fn {id}_dropper_insert(game: &mut crate::game::Game<'_>, pos: [i32; 3], facing: horizon_core::geometry::Direction, slot: u32, handled: &mut bool) {{
    if *handled {{
        return;
    }}
{front}    if !{krate}::ours(&game.native_block(front)) {{
        return;
    }}
{host}    let mut host = Host {{ game }};
    *handled = {krate}::hook_runtime("dropper_insert", |rt| {c}::glue::dropper_insert(rt, &mut host, pos, d, slot));
}}
"#)),
        ("crafter_output", format!(r#"// Generated by horizon-mod-translator: a crafter's result put out toward one of the translated
// mod's blocks, which has no container of the game's own: NeoForge's
// `VanillaInventoryCodeHooks.insertCrafterOutput` offers it to the block's item handler, and what
// it leaves flies out.
fn {id}_crafter_output(game: &mut crate::game::Game<'_>, pos: [i32; 3], facing: horizon_core::geometry::Direction, stack: &mut horizon_client::items::ItemStack) {{
    if stack.count == 0 {{
        return;
    }}
{front}    if !{krate}::ours(&game.native_block(front)) {{
        return;
    }}
{host}    let mut host = Host {{ game }};
    let held = host.held_of(stack);
    match {krate}::hook_runtime("crafter_output", |rt| {c}::glue::crafter_output(rt, &mut host, pos, d, held)) {{
        // The handler took all of it.
        Some(None) => stack.count = 0,
        Some(Some(rest)) => *stack = host.stack_of(&rest),
        None => {{}}
    }}
}}
"#)),
        ("use_item_on", format!(r#"// Generated by horizon-mod-translator: a right click on a block holding one of the translated
// mod's items, once the block let it through: the item's own `useOn` (Create's wrench turns
// blocks). An item that takes the click claims it, so the game's own use of the item doesn't run.
fn {id}_use_item_on(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, pos: [i32; 3], face: [i32; 3], claimed: &mut bool) {{
    if *claimed {{
        return;
    }}
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
    let item = stack.item.to_string();
    if !{krate}::ours(&item) {{
        return;
    }}
    // Direction's 3D data value: down, up, north, south, west, east.
    let face_id = match face {{
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        _ => 5,
    }};
{player_host}    // Where the click landed: as the player's client aimed it, else the clicked face's centre.
    let at = |i: usize| pos[i] as f64 + 0.5 + face[i] as f64 * 0.5;
    let hit = game.native_click_hit().filter(|h| h.0 == pos).map_or([at(0), at(1), at(2)], |h| h.2);
    let click = {c}::glue::Placement {{
        pos,
        hit_pos: pos,
        face: face_id,
        hit,
        item,
        hand,
        replace_clicked: true,
        ..Default::default()
    }};
    let mut host = Host {{ game }};
    if {krate}::hook_runtime("use_item_on", |rt| {c}::glue::use_item_on(rt, &mut host, &click)) {{
        *claimed = true;
    }}
}}
"#)),
        ("use_item", format!(r#"// Generated by horizon-mod-translator: a right click holding one of the translated mod's items,
// in the air or on a block that let it through: the item's own `use` (Create's goggles, filters,
// experience nuggets). An item that takes the click claims it, so the game's own use doesn't run.
fn {id}_use_item(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, claimed: &mut bool) {{
    if *claimed {{
        return;
    }}
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
    let item = stack.item.to_string();
{player_host}    let mut host = Host {{ game }};
    // The mod's listeners hear it first, whatever the item (NeoForge's
    // `PlayerInteractEvent.RightClickItem`); one that cancels it takes the click.
    if {krate}::hook_runtime("use_item", |rt| {c}::events::right_click_item(rt, &mut host, hand)).is_some() {{
        *claimed = true;
        return;
    }}
    if !{krate}::ours(&item) {{
        return;
    }}
    if {krate}::hook_runtime("use_item", |rt| {c}::glue::use_item(rt, &mut host, &item, hand)) {{
        *claimed = true;
    }}
}}
"#)),
        ("use_duration", format!(r#"// Generated by horizon-mod-translator: how long one of the translated mod's items is used
// (`Item.getUseDuration`), on both sides: an item its `use` starts using counts that down to its
// `finishUsingItem`, unless let go before (`releaseUsing`).
fn {id}_use_duration(stack: &horizon_client::items::ItemStack, duration: &mut i32) {{
    if !{krate}::ours(&stack.item) {{
        return;
    }}
    if let Some(ticks) = {krate}::with_registrations(|rt| {c}::item_use::use_duration(rt, &stack.item)) {{
        *duration = ticks;
    }}
}}
"#)),
        ("living_entity_use_item_start", format!(r#"// Generated by horizon-mod-translator: a player starts using one of the translated mod's items on
// the server. The use goes on only when the click's own `use` started it (`startUsingItem`), as in
// NeoForge; a food's eating is the game's.
fn {id}_living_entity_use_item_start(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, entity: Option<i32>, canceled: &mut bool) {{
    if entity.is_some() || game.native_window() || !{krate}::ours(&stack.item) {{
        return;
    }}
    if {krate}::hook_runtime("living_entity_use_item_start", |rt| {c}::item_use::use_may_start(rt, &stack.item)) == Some(false) {{
        *canceled = true;
    }}
}}
"#)),
        ("on_use_tick", format!(r#"// Generated by horizon-mod-translator: each tick a player uses one of the translated mod's items,
// on the server: its own `onUseTick`.
fn {id}_on_use_tick(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, entity: Option<i32>, remaining: i32) {{
    if entity.is_some() || game.native_window() || !{krate}::ours(&stack.item) {{
        return;
    }}
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("on_use_tick", |rt| {c}::item_use::use_tick(rt, &mut host, &stack.item, remaining, hand));
}}
"#)),
        ("living_entity_use_item_finish", format!(r#"// Generated by horizon-mod-translator: a player's use of one of the translated mod's items ran its
// time, on the server (`completeUsingItem`): its own `finishUsingItem` (Create's builder's tea
// leaves a glass bottle), after the game's own finish (the food eaten). What it does to the hand
// that used it (the stack it answers goes there) reaches that hand itself, so `result` stays as it
// was.
fn {id}_living_entity_use_item_finish(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, entity: Option<i32>, result: &mut Option<horizon_client::items::ItemStack>) {{
    if entity.is_some() || game.native_window() || !{krate}::ours(&stack.item) {{
        return;
    }}
    let _ = result;
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("living_entity_use_item_finish", |rt| {c}::item_use::finish_using_item(rt, &mut host, &stack.item, hand));
}}
"#)),
        ("release_using", format!(r#"// Generated by horizon-mod-translator: a player lets go of one of the translated mod's items before
// its use ran its time, on the server: its own `releaseUsing` (Create's packages are thrown).
fn {id}_release_using(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, entity: Option<i32>, time_left: i32) {{
    if entity.is_some() || game.native_window() || !{krate}::ours(&stack.item) {{
        return;
    }}
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("release_using", |rt| {c}::item_use::release_using(rt, &mut host, &stack.item, time_left, hand));
}}
"#)),
        ("is_valid_bonemeal_target", format!(r#"// Generated by horizon-mod-translator: whether bone meal works on one of the translated mod's
// blocks (`BonemealableBlock.isValidBonemealTarget`), by its own code or its vanilla crop base's.
// Asked on both sides, as vanilla asks it; the level is only read.
fn {id}_is_valid_bonemeal_target(
    level: &crate::game::GameLevel<'_>,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
    valid: &mut Option<bool>,
) {{
    if valid.is_some() || !{krate}::ours(level.native_state_block_name(state)) {{
        return;
    }}
    let text = level.native_state_id_text(state);
{read_host}    let mut host = Host {{ level }};
    if let Some(answer) = {krate}::with_registrations(|rt| {c}::glue::is_valid_bonemeal_target(rt, &mut host, [pos.x, pos.y, pos.z], &text)) {{
        *valid = Some(answer);
    }}
}}
"#)),
        ("is_bonemeal_success", format!(r#"// Generated by horizon-mod-translator: whether this use of bone meal grows one of the translated
// mod's blocks (`BonemealableBlock.isBonemealSuccess`), on the server, after it said it takes it.
fn {id}_is_bonemeal_success(
    level: &mut crate::game::GameLevel<'_>,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
    success: &mut bool,
) {{
    if *success || !{krate}::ours(level.native_state_block_name(state)) {{
        return;
    }}
    let text = level.native_state_id_text(state);
    let level = &*level;
{read_host}    let mut host = Host {{ level }};
    *success = {krate}::hook_runtime("is_bonemeal_success", |rt| {c}::glue::is_bonemeal_success(rt, &mut host, [pos.x, pos.y, pos.z], &text));
}}
"#)),
        ("perform_bonemeal", format!(r#"// Generated by horizon-mod-translator: bone meal grows one of the translated mod's blocks
// (`BonemealableBlock.performBonemeal`), on the server.
fn {id}_perform_bonemeal(
    level: &mut crate::game::GameLevel<'_>,
    ticks: &mut horizon_world::level::Ticks,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
) {{
    if !{krate}::ours(level.native_state_block_name(state)) {{
        return;
    }}
    let text = level.native_state_id_text(state);
{level_host}    let mut host = Host {{ level, ticks }};
    {krate}::hook_runtime("perform_bonemeal", |rt| {c}::glue::perform_bonemeal(rt, &mut host, [pos.x, pos.y, pos.z], &text));
}}
"#)),
        ("can_survive", format!(r#"// Generated by horizon-mod-translator: whether one of the translated mod's blocks survives where it
// is or goes (`BlockState.canSurvive`: placing it, a neighbour's change, a feature's
// `would_survive`), by its own code: its `canSurvive`, else its vanilla plant base's with its own
// `mayPlaceOn`. Asked about every block: other blocks are passed over by name.
fn {id}_can_survive(
    level: &crate::game::GameLevel<'_>,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
    survives: &mut Option<bool>,
) {{
    if survives.is_some() || !{krate}::ours(level.native_state_block_name(state)) {{
        return;
    }}
    let text = level.native_state_id_text(state);
{read_host}    let mut host = Host {{ level }};
    if let Some(answer) = {krate}::with_registrations(|rt| {c}::glue::can_survive(rt, &mut host, [pos.x, pos.y, pos.z], &text)) {{
        *survives = Some(answer);
    }}
}}
"#)),
        ("may_place_on", format!(r#"// Generated by horizon-mod-translator: the blocks one of the translated mod's plants stands on
// (`mayPlaceOn`), which its template's survival asks: its own code's answer.
fn {id}_may_place_on(
    level: &crate::game::GameLevel<'_>,
    state: horizon_world::registry::BlockStateId,
    ground: horizon_world::registry::BlockStateId,
    ground_pos: horizon_core::position::BlockPos,
    may: &mut Option<bool>,
) {{
    if may.is_some() || !{krate}::ours(level.native_state_block_name(state)) {{
        return;
    }}
    let (text, ground_text) = (level.native_state_id_text(state), level.native_state_id_text(ground));
{read_host}    let mut host = Host {{ level }};
    let at = [ground_pos.x, ground_pos.y, ground_pos.z];
    if let Some(answer) = {krate}::with_registrations(|rt| {c}::glue::may_place_on(rt, &mut host, &text, &ground_text, at)) {{
        *may = Some(answer);
    }}
}}
"#)),
        ("state_for_placement", format!(r#"// Generated by horizon-mod-translator: placing one of the translated mod's blocks asks its own
// `getStateForPlacement` (a player's `BlockItem.place`, a dispenser's).
fn {id}_state_for_placement(
    level: &crate::game::GameLevel<'_>,
    state: horizon_world::registry::BlockStateId,
    context: &horizon_world::level::PlacementContext,
    answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
) {{
    if answer.is_some() {{
        return;
    }}
    let text = level.native_state_id_text(state);
    if !{krate}::ours(&text) {{
        return;
    }}
    struct Host<'a, 'b> {{
        level: &'a crate::game::GameLevel<'b>,
        sneaking: bool,
        item: String,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.level.native_state_text(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.level.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.level.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.level.native_dimension_name().to_owned()
        }}
        fn light(&self, pos: [i32; 3]) -> (u8, u8) {{
            self.level.native_light_here(pos)
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.level.native_fluid_name_in_tag(fluid, tag)
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false // choosing a state changes nothing
        }}
        fn player_sneaking(&self) -> bool {{
            self.sneaking
        }}
        fn player_held(&self) -> Option<(String, u32)> {{
            (!self.item.is_empty()).then(|| (self.item.clone(), 1))
        }}
    }}
    let placement = {c}::glue::Placement {{
        pos: [context.pos.x, context.pos.y, context.pos.z],
        hit_pos: [context.hit_pos.x, context.hit_pos.y, context.hit_pos.z],
        face: context.face as u8 as i64,
        hit: [context.hit.x, context.hit.y, context.hit.z],
        inside: context.inside,
        yaw: context.yaw,
        pitch: context.pitch,
        item: context.item.clone(),
        replace_clicked: context.replace_clicked,
        // The level's placement context names the placed item, not the hand holding it.
        hand: 0,
    }};
    let mut host = Host {{ level, sneaking: context.sneaking, item: context.item.clone() }};
    match {krate}::with_registrations(|rt| {c}::glue::state_for_placement(rt, &mut host, &text, &placement)) {{
        Some(Some(chosen)) => {{
            if let Some(id) = level.native_state_from_text(&chosen) {{
                *answer = Some(Some(id));
            }}
        }}
        Some(None) => *answer = Some(None),
        None => {{}}
    }}
}}
"#)),
        ("update_shape", format!(r#"// Generated by horizon-mod-translator: the state one of the translated mod's blocks takes when a
// neighbour changes (`BlockState.updateShape`), by its own `updateShape`: a pipe, a cable or a pane
// joining what is next to it. The level is only read; the ticks the block schedules go to the
// level's. It runs inside the mod's own calls too (its `setBlock` updates the neighbours), on the
// same runtime, so the block sees its block entity as the mod left it.
fn {id}_update_shape(
    level: &crate::game::GameLevel<'_>,
    ticks: &mut horizon_world::level::Ticks,
    pos: horizon_core::position::BlockPos,
    state: horizon_world::registry::BlockStateId,
    direction: horizon_core::geometry::Direction,
    neighbour_pos: horizon_core::position::BlockPos,
    neighbour: horizon_world::registry::BlockStateId,
    new: &mut Option<horizon_world::registry::BlockStateId>,
) {{
    if new.is_some() || !{krate}::ours(level.native_state_block_name(state)) {{
        return;
    }}
    let (text, neighbour_text) = (level.native_state_id_text(state), level.native_state_id_text(neighbour));
    struct Host<'a, 'b, 'c> {{
        level: &'a crate::game::GameLevel<'b>,
        ticks: &'c mut horizon_world::level::Ticks,
    }}
    impl {c}::host::Host for Host<'_, '_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.level.native_state_text(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.level.native_block_name_in_tag(block, tag)
        }}
        fn block_facts(&self, state: &str) -> Option<{c}::host::BlockFacts> {{
            self.level.native_state_facts(state).map(|(hardness, push, replaceable, sturdy)| {c}::host::BlockFacts {{ hardness, push, replaceable, sturdy }})
        }}
        fn dimension(&self) -> String {{
            self.level.native_dimension_name().to_owned()
        }}
        fn light(&self, pos: [i32; 3]) -> (u8, u8) {{
            self.level.native_light_here(pos)
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.level.native_fluid_name_in_tag(fluid, tag)
        }}
        fn schedule_block_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool {{
            self.level.native_schedule_block_tick(self.ticks, pos, block, delay)
        }}
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {{
            false // the level is only read
        }}
    }}
    let mut host = Host {{ level, ticks }};
    let (at, next) = ([pos.x, pos.y, pos.z], [neighbour_pos.x, neighbour_pos.y, neighbour_pos.z]);
    let chosen = {krate}::hook_runtime("update_shape", |rt| {c}::glue::update_shape(rt, &mut host, at, &text, direction as usize, next, &neighbour_text));
    if let Some(id) = chosen.and_then(|c| level.native_state_from_text(&c)) {{
        *new = Some(id);
    }}
}}
"#)),
    ]
}

const CLIENT_BINDINGS: [(&str, &str); 38] = [
    ("tooltip", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"lines\", ty = \"&mut Vec<(horizon_client::text::Text, u32)>\", value = \"&mut *lines\" },\n"),
    ("input_key", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"event\", ty = \"&winit::event::KeyEvent\", value = \"event\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n"),
    ("mouse_button_pre", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"pressed\", ty = \"bool\", value = \"pressed\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("mouse_button_post", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"pressed\", ty = \"bool\", value = \"pressed\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n"),
    ("mouse_scrolling", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"scroll\", ty = \"[f64; 2]\", value = \"scroll\" },\n    { name = \"accumulated\", ty = \"[i32; 2]\", value = \"accumulated\" },\n    { name = \"buttons\", ty = \"[bool; 3]\", value = \"buttons\" },\n    { name = \"cursor\", ty = \"[f32; 2]\", value = \"cursor\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("key", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"key\", ty = \"&str\", value = \"key\" },\n    { name = \"pressed\", ty = \"bool\", value = \"pressed\" },\n"),
    ("mods", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"mods\", ty = \"&mut Vec<crate::game::native_hooks::NativeMod>\", value = \"&mut *mods\" },\n"),
    ("commands", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"commands\", ty = \"&mut Vec<&'static str>\", value = \"&mut *commands\" },\n"),
    ("command", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"command\", ty = \"&str\", value = \"command\" },\n    { name = \"ran\", ty = \"&mut bool\", value = \"&mut *ran\" },\n"),
    ("component_types", "    { name = \"types\", ty = \"&mut Vec<String>\", value = \"&mut *types\" },\n"),
    ("level_submit", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"camera\", ty = \"[f64; 3]\", value = \"camera\" },\n    { name = \"draw\", ty = \"&mut crate::game::native_block_entity_draw::BlockEntityDraw\", value = \"&mut *draw\" },\n"),
    ("content", "    { name = \"content\", ty = \"&mut horizon_mods::Content\", value = \"&mut *content\" },\n"),
    ("content_block", "    { name = \"block\", ty = \"&mut horizon_mods::content::Block\", value = \"&mut *block\" },\n"),
    ("block_entity_submit", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"draw\", ty = \"&mut crate::game::native_block_entity_draw::BlockEntityDraw\", value = \"&mut *draw\" },\n"),
    ("block_entity_update_tag", "    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"tag\", ty = \"&mut Option<horizon_world::nbt::Compound>\", value = \"&mut *tag\" },\n"),
    ("tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n"),
    ("block_entity_data", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"tag\", ty = \"&horizon_world::nbt::Compound\", value = \"tag\" },\n"),
    ("net_from_server", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"channel\", ty = \"&str\", value = \"channel\" },\n    { name = \"data\", ty = \"&[u8]\", value = \"data\" },\n"),
    ("client_level_load", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n"),
    ("client_level_unload", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n"),
    ("use_animation", "    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"animation\", ty = \"&mut horizon_world::consumable::UseAnimation\", value = \"&mut *animation\" },\n"),
    ("gui_layer_pre", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"layer\", ty = \"&str\", value = \"layer\" },\n    { name = \"batch\", ty = \"&mut horizon_client::hud::HudBatch\", value = \"&mut *batch\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("gui_layer_post", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"layer\", ty = \"&str\", value = \"layer\" },\n    { name = \"batch\", ty = \"&mut horizon_client::hud::HudBatch\", value = \"&mut *batch\" },\n"),    ("screen_render_post", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"batch\", ty = \"&mut horizon_client::hud::HudBatch\", value = \"&mut *batch\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n"),
    ("screen_mouse_button_pressed_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("screen_mouse_button_released_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("screen_mouse_dragged_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"delta\", ty = \"[f32; 2]\", value = \"delta\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("screen_mouse_scrolled_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"scroll\", ty = \"[f64; 2]\", value = \"scroll\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("screen_key_pressed_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"key\", ty = \"&str\", value = \"key\" },\n    { name = \"repeat\", ty = \"bool\", value = \"repeat\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("screen_key_released_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"key\", ty = \"&str\", value = \"key\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("screen_char_typed_pre", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"screen\", ty = \"&crate::game::native_hooks::NativeScreenView<'_>\", value = \"screen\" },\n    { name = \"c\", ty = \"char\", value = \"c\" },\n    { name = \"modifiers\", ty = \"u32\", value = \"modifiers\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("item_property", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"stack\", ty = \"&horizon_assets::item_model::StackView\", value = \"stack\" },\n    { name = \"property\", ty = \"&str\", value = \"property\" },\n    { name = \"owner\", ty = \"Option<&horizon_assets::item_model::OwnerState>\", value = \"owner\" },\n    { name = \"seed\", ty = \"i32\", value = \"seed\" },\n    { name = \"value\", ty = \"&mut Option<f32>\", value = \"&mut *value\" },\n"),
    ("interaction_key", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"key_mapping\", ty = \"&str\", value = \"key_mapping\" },\n    { name = \"hand\", ty = \"horizon_client::first_person::Hand\", value = \"hand\" },\n    { name = \"swing_hand\", ty = \"&mut bool\", value = \"&mut *swing_hand\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("client_use_item_on", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"point\", ty = \"[f64; 3]\", value = \"point\" },\n    { name = \"hand\", ty = \"horizon_client::first_person::Hand\", value = \"hand\" },\n    { name = \"result\", ty = \"&mut crate::game::native_hooks::NativeClientUse\", value = \"&mut *result\" },\n"),
    ("compute_fov", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"partial\", ty = \"f32\", value = \"partial\" },\n    { name = \"camera\", ty = \"[f64; 3]\", value = \"camera\" },\n    { name = \"degrees\", ty = \"&mut f32\", value = \"&mut *degrees\" },\n"),
    ("compute_fog_color", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"partial\", ty = \"f32\", value = \"partial\" },\n    { name = \"camera\", ty = \"[f64; 3]\", value = \"camera\" },\n    { name = \"fog_type\", ty = \"horizon_client::fog::FogType\", value = \"fog_type\" },\n    { name = \"rgb\", ty = \"&mut [f32; 3]\", value = \"&mut *rgb\" },\n"),
    ("render_fog", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"partial\", ty = \"f32\", value = \"partial\" },\n    { name = \"camera\", ty = \"[f64; 3]\", value = \"camera\" },\n    { name = \"fog_type\", ty = \"horizon_client::fog::FogType\", value = \"fog_type\" },\n    { name = \"fog\", ty = \"&mut horizon_client::fog::FogData\", value = \"&mut *fog\" },\n"),
    ("camera_shake", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"partial\", ty = \"f32\", value = \"partial\" },\n    { name = \"shake\", ty = \"&mut crate::game::native_hooks::NativeShake\", value = \"&mut *shake\" },\n"),
];

const BLOCK_ENTITY_BINDINGS: [(&str, &str); 55] = [
    ("register_commands", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"commands\", ty = \"&mut Vec<crate::game::native_server::NativeCommand>\", value = \"&mut *commands\" },\n"),
    ("perform_command", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"&crate::game::native_server::NativeServerPlayer\", value = \"player\" },\n    { name = \"command\", ty = \"&str\", value = \"command\" },\n    { name = \"ran\", ty = \"&mut bool\", value = \"&mut *ran\" },\n"),
    ("perform_console_command", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"command\", ty = \"&str\", value = \"command\" },\n    { name = \"ran\", ty = \"&mut bool\", value = \"&mut *ran\" },\n"),
    ("server_about_to_start", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dedicated\", ty = \"bool\", value = \"dedicated\" },\n"),
    ("server_stopping", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dedicated\", ty = \"bool\", value = \"dedicated\" },\n"),
    ("world_lifecycle", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"context\", ty = \"&crate::game::native_lifecycle::NativeLifecycleContext\", value = \"context\" },\n    { name = \"phase\", ty = \"crate::game::native_lifecycle::WorldPhase\", value = \"phase\" },\n    { name = \"error_text\", ty = \"&str\", value = \"error_text\" },\n"),
    ("player_logged_in", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"name\", ty = \"&str\", value = \"name\" },\n"),
    ("player_logged_out", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"name\", ty = \"&str\", value = \"name\" },\n"),
    ("player_respawned", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"name\", ty = \"&str\", value = \"name\" },\n"),
    ("server_data_loaded", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"reload\", ty = \"bool\", value = \"reload\" },\n"),
    ("level_tick_pre", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n"),
    ("level_tick_post", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n"),
    ("server_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"phase\", ty = \"crate::game::native_lifecycle::ServerTickPhase\", value = \"phase\" },\n"),
    ("player_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n"),
    ("destroy_block", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("block_replaced", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"old\", ty = \"horizon_world::registry::BlockStateId\", value = \"old\" },\n    { name = \"new\", ty = \"horizon_world::registry::BlockStateId\", value = \"new\" },\n    { name = \"moved\", ty = \"bool\", value = \"moved\" },\n"),
    ("loot_drops", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"context\", ty = \"&crate::game::native_hooks::NativeLootDrops<'_>\", value = \"context\" },\n    { name = \"drops\", ty = \"&mut Vec<horizon_client::items::ItemStack>\", value = \"&mut *drops\" },\n"),
    ("place_block", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n"),
    ("attack_entity", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"pos\", ty = \"[f64; 3]\", value = \"pos\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("living_incoming_damage", "    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"damage_type\", ty = \"&str\", value = \"damage_type\" },\n    { name = \"causing\", ty = \"i32\", value = \"causing\" },\n    { name = \"direct\", ty = \"i32\", value = \"direct\" },\n    { name = \"amount\", ty = \"&mut f32\", value = \"&mut *amount\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("living_knockback", "    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"damage_type\", ty = \"&str\", value = \"damage_type\" },\n    { name = \"causing\", ty = \"i32\", value = \"causing\" },\n    { name = \"direct\", ty = \"i32\", value = \"direct\" },\n    { name = \"strength\", ty = \"&mut f64\", value = \"&mut *strength\" },\n    { name = \"x\", ty = \"&mut f64\", value = \"&mut *x\" },\n    { name = \"z\", ty = \"&mut f64\", value = \"&mut *z\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("living_damage_pre", "    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"damage_type\", ty = \"&str\", value = \"damage_type\" },\n    { name = \"causing\", ty = \"i32\", value = \"causing\" },\n    { name = \"direct\", ty = \"i32\", value = \"direct\" },\n    { name = \"amount\", ty = \"&mut f32\", value = \"&mut *amount\" },\n"),
    ("living_damage_post", "    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"damage_type\", ty = \"&str\", value = \"damage_type\" },\n    { name = \"causing\", ty = \"i32\", value = \"causing\" },\n    { name = \"direct\", ty = \"i32\", value = \"direct\" },\n    { name = \"amount\", ty = \"f32\", value = \"amount\" },\n"),
    ("living_death", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"damage_type\", ty = \"&str\", value = \"damage_type\" },\n    { name = \"causing\", ty = \"i32\", value = \"causing\" },\n    { name = \"direct\", ty = \"i32\", value = \"direct\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("living_drops", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"damage_type\", ty = \"&str\", value = \"damage_type\" },\n    { name = \"causing\", ty = \"i32\", value = \"causing\" },\n    { name = \"direct\", ty = \"i32\", value = \"direct\" },\n    { name = \"recently_hit\", ty = \"bool\", value = \"recently_hit\" },\n    { name = \"drops\", ty = \"&mut Vec<crate::game::native_hooks::NativeDrop>\", value = \"&mut *drops\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("player_died", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"cause\", ty = \"Option<&str>\", value = \"cause\" },\n"),
    ("interact_entity", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"entity\", ty = \"i32\", value = \"entity\" },\n    { name = \"kind\", ty = \"&str\", value = \"kind\" },\n    { name = \"stack\", ty = \"Option<&horizon_client::items::ItemStack>\", value = \"stack\" },\n    { name = \"claimed\", ty = \"&mut bool\", value = \"&mut *claimed\" },\n"),
    ("net_from_client", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"channel\", ty = \"&str\", value = \"channel\" },\n    { name = \"data\", ty = \"&[u8]\", value = \"data\" },\n"),
    ("is_valid_bonemeal_target", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"pos\", ty = \"horizon_core::position::BlockPos\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"valid\", ty = \"&mut Option<bool>\", value = \"&mut *valid\" },\n"),
    ("is_bonemeal_success", "    { name = \"level\", ty = \"&mut crate::game::GameLevel<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"horizon_core::position::BlockPos\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"success\", ty = \"&mut bool\", value = \"&mut *success\" },\n"),
    ("perform_bonemeal", "    { name = \"level\", ty = \"&mut crate::game::GameLevel<'_>\", value = \"&mut *self\" },\n    { name = \"ticks\", ty = \"&mut horizon_world::level::Ticks\", value = \"&mut *ticks\" },\n    { name = \"pos\", ty = \"horizon_core::position::BlockPos\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n"),
    ("can_survive", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"pos\", ty = \"horizon_core::position::BlockPos\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"survives\", ty = \"&mut Option<bool>\", value = \"&mut *survives\" },\n"),
    ("may_place_on", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"ground\", ty = \"horizon_world::registry::BlockStateId\", value = \"ground\" },\n    { name = \"ground_pos\", ty = \"horizon_core::position::BlockPos\", value = \"ground_pos\" },\n    { name = \"may\", ty = \"&mut Option<bool>\", value = \"&mut *may\" },\n"),
    ("state_for_placement", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"context\", ty = \"&horizon_world::level::PlacementContext\", value = \"context\" },\n    { name = \"answer\", ty = \"&mut Option<Option<horizon_world::registry::BlockStateId>>\", value = \"&mut *answer\" },\n"),
    ("update_shape", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"ticks\", ty = \"&mut horizon_world::level::Ticks\", value = \"&mut *ticks\" },\n    { name = \"pos\", ty = \"horizon_core::position::BlockPos\", value = \"pos\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"direction\", ty = \"horizon_core::geometry::Direction\", value = \"direction\" },\n    { name = \"neighbour_pos\", ty = \"horizon_core::position::BlockPos\", value = \"neighbour_pos\" },\n    { name = \"neighbour\", ty = \"horizon_world::registry::BlockStateId\", value = \"neighbour\" },\n    { name = \"new\", ty = \"&mut Option<horizon_world::registry::BlockStateId>\", value = \"&mut *new\" },\n"),
    ("block_entity", "    { name = \"block\", ty = \"&str\", value = \"block\" },\n    { name = \"entity\", ty = \"&mut Option<(String, horizon_world::nbt::Compound)>\", value = \"&mut *entity\" },\n"),
    ("block_entity_ticks", "    { name = \"block\", ty = \"&str\", value = \"block\" },\n    { name = \"ticks\", ty = \"&mut bool\", value = \"&mut *ticks\" },\n"),
    ("block_entity_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&mut horizon_client::containers::ModBlockEntity\", value = \"&mut *entity\" },\n"),
    ("block_entity_components", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"components\", ty = \"&mut Vec<(String, horizon_world::nbt::Tag)>\", value = \"&mut *components\" },\n"),
    ("right_click_block", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("use_item_on", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"claimed\", ty = \"&mut bool\", value = \"&mut *claimed\" },\n"),
    ("use_item", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"claimed\", ty = \"&mut bool\", value = \"&mut *claimed\" },\n"),
    ("use_duration", "    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"duration\", ty = \"&mut i32\", value = \"&mut *duration\" },\n"),
    ("living_entity_use_item_start", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("on_use_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"remaining\", ty = \"i32\", value = \"remaining\" },\n"),
    ("living_entity_use_item_finish", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"result\", ty = \"&mut Option<horizon_client::items::ItemStack>\", value = \"&mut *result\" },\n"),
    ("release_using", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"time_left\", ty = \"i32\", value = \"time_left\" },\n"),
    ("should_apply_effect_tick_this_tick", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"effect\", ty = \"&str\", value = \"effect\" },\n    { name = \"tick_count\", ty = \"i32\", value = \"tick_count\" },\n    { name = \"amplifier\", ty = \"i32\", value = \"amplifier\" },\n    { name = \"apply\", ty = \"&mut bool\", value = \"&mut *apply\" },\n"),
    ("apply_effect_tick", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"effect\", ty = \"&str\", value = \"effect\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"amplifier\", ty = \"i32\", value = \"amplifier\" },\n    { name = \"keep\", ty = \"&mut bool\", value = \"&mut *keep\" },\n"),
    ("on_effect_added", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"effect\", ty = \"&str\", value = \"effect\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"amplifier\", ty = \"i32\", value = \"amplifier\" },\n"),
    ("mob_effect_added", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"effect\", ty = \"&str\", value = \"effect\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"amplifier\", ty = \"i32\", value = \"amplifier\" },\n"),
    ("hopper_insert", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"facing\", ty = \"horizon_core::geometry::Direction\", value = \"facing\" },\n    { name = \"moved\", ty = \"&mut bool\", value = \"&mut *moved\" },\n"),
    ("hopper_extract", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"took\", ty = \"&mut Option<bool>\", value = \"&mut *took\" },\n"),
    ("dropper_insert", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"facing\", ty = \"horizon_core::geometry::Direction\", value = \"facing\" },\n    { name = \"slot\", ty = \"u32\", value = \"slot\" },\n    { name = \"handled\", ty = \"&mut bool\", value = \"&mut *handled\" },\n"),
    ("crafter_output", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"facing\", ty = \"horizon_core::geometry::Direction\", value = \"facing\" },\n    { name = \"stack\", ty = \"&mut horizon_client::items::ItemStack\", value = \"&mut *stack\" },\n"),
];

fn mixin_toml(id: &str, version: &str, crates: &[String]) -> String {
    let mut s = format!(
        "# Generated by horizon-mod-translator from the mod's jar.\n[mod]\nid = \"{id}\"\nversion = \"{version}\"\n\n"
    );
    for (hook, _) in BLOCK_HOOKS {
        s.push_str(&format!(
            "[[injection]]\nid = \"{hook}\"\nhook = \"{hook}\"\nhook_version = 1\nkind = \"HEAD\"\nhelper = \"{hook}.rs\"\n\
             bindings = [\n    {{ name = \"level\", ty = \"&mut crate::game::GameLevel<'_>\", value = \"&mut *self\" }},\n    \
             {{ name = \"ticks\", ty = \"&mut horizon_world::level::Ticks\", value = \"&mut *ticks\" }},\n    \
             {{ name = \"pos\", ty = \"horizon_core::position::BlockPos\", value = \"pos\" }},\n    \
             {{ name = \"handled\", ty = \"&mut bool\", value = \"&mut *handled\" }},\n]\n\n"
        ));
    }
    for (hook, bindings) in BLOCK_ENTITY_BINDINGS.iter().chain(CLIENT_BINDINGS.iter()) {
        s.push_str(&format!(
            "[[injection]]\nid = \"{hook}\"\nhook = \"{hook}\"\nhook_version = 1\nkind = \"HEAD\"\nhelper = \"{hook}.rs\"\nbindings = [\n{bindings}]\n\n"
        ));
    }
    for c in crates {
        s.push_str(&format!("[[crate]]\nid = \"{c}\"\npath = \"{c}\"\n\n"));
    }
    s
}

/// Writes the bundle for `jar` into `out`. Returns the mod id and what the translation left out.
pub fn write(jar: &Path, classes: &[ClassFile], out: &Path) -> Result<(String, Vec<String>), String> {
    write_with(jar, classes, out, None)
}

/// [`write`], with the engine linking the runtime core crate at `runtime` (a path to
/// `horizon-compat-core`) instead of carrying the runtime inlined. The engine then calls the API
/// mapping through `horizon_compat::install()`, so tools that link `horizon-compat` themselves
/// rebuild only that crate when the mapping changes. Such a bundle is for tools (harnesses), not
/// for horizon-mixins, which allows no path dependencies.
pub fn write_with(jar: &Path, classes: &[ClassFile], out: &Path, runtime: Option<&Path>) -> Result<(String, Vec<String>), String> {
    write_group(&[jar], classes, out, runtime)
}

/// [`write_with`] for a mod and the mods built on it (its addons), translated as one engine:
/// `jars[0]` is the mod the others need, `classes` all of theirs ([`crate::classes_of`]). One
/// runtime holds them all, so an addon's classes extend the mod's and call into it natively, as on
/// the JVM. The engine's hooks answer for every one of their namespaces (`NAMESPACES` in the
/// engine), and each mod keeps a content mod of its own: the first's in `content/`, each other's
/// assets and manifest in `content-<id>/` (the engine declares their blocks and items).
pub fn write_group(jars: &[&Path], classes: &[ClassFile], out: &Path, runtime: Option<&Path>) -> Result<(String, Vec<String>), String> {
    let jar = *jars.first().ok_or("no jar to translate")?;
    let id = mod_id(jar).ok_or_else(|| format!("{}: no NeoForge mod id", jar.display()))?;
    let mut members: Vec<(&Path, String)> = vec![(jar, id.clone())];
    for j in &jars[1..] {
        let n = mod_id(j).ok_or_else(|| format!("{}: no NeoForge mod id", j.display()))?;
        if members.iter().all(|(_, m)| *m != n) {
            members.push((j, n));
        }
    }
    let namespaces: Vec<&str> = members.iter().map(|(_, n)| n.as_str()).collect();
    let bundle_id = format!("{id}_translated");
    let prefix = format!("{}_translated", id.replace('-', "_"));
    let krate = format!("{prefix}_engine");
    let mut program = Program::new(classes);
    program.mod_id = Some(id.clone());
    program.addons = members[1..].iter().map(|(_, n)| n.clone()).collect();
    program.services = crate::services_of(jars)?;
    // The mods FML lists: the jars', and those they bundle for jar-in-jar (Flywheel, Ponder).
    program.metadata = jars.iter().flat_map(|j| crate::mod_metadata(j).into_iter().chain(crate::nested_mod_metadata(j))).collect();
    program.scan_files = jars.iter().map(|j| (crate::mod_metadata(j).into_iter().map(|m| m.0).collect(), crate::class_names(j))).collect();
    program.use_mixin_configs(&crate::mixin_configs_of(jars));
    let src = out.join("engine/src");
    let package = |name: &str| name.replace('_', "-");
    // The bundle's crates, the engine last (horizon-mixin.toml lists them).
    let mut crate_ids: Vec<String> = Vec::new();
    let (files, deps, root) = match runtime {
        Some(core) => (
            program.emit_files(40),
            format!("horizon-compat-core = {{ path = {:?} }}\n", core.display().to_string()),
            "/// The runtime core; the API mapping comes through `horizon_compat::install()`.\npub use horizon_compat_core as horizon_compat;\nuse horizon_compat::".to_owned(),
        ),
        None => {
            // The runtime, each mod's code and the engine as crates of their own (`crate::split`):
            // a mod's crates may depend on each other, and each compiles in a rustc of its own.
            let runtime_crate = format!("{prefix}_runtime");
            write_runtime(&out.join("runtime"), &package(&runtime_crate))?;
            crate_ids.push("runtime".into());
            let owners: Vec<std::collections::HashSet<String>> = members.iter().map(|(j, _)| crate::class_names(j)).collect();
            let group = |c: &str| owners.iter().position(|o| o.contains(c)).unwrap_or(owners.len());
            let (parts, lib) = program.emit_groups(40, owners.len() + 1, &group);
            // A class no member jar holds is a library's they bundle.
            let groups: Vec<(String, Vec<String>)> = members.iter().map(|(_, ns)| ns.clone()).chain(["lib".to_owned()]).zip(parts).collect();
            let split = crate::split::split(&groups, &lib, &runtime_crate, &prefix, crate_bytes());
            let mut deps = format!("{ENGINE_DEPENDENCIES}{} = {{ path = \"../runtime\" }}\n", package(&runtime_crate));
            for c in &split.crates {
                let name = package(&crate::split::crate_name(&prefix, &c.id));
                put(
                    out.join(&c.id).join("Cargo.toml"),
                    format!(
                        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{} = {{ path = \"../runtime\" }}\n",
                        package(&runtime_crate)
                    ),
                )
                .map_err(|e| e.to_string())?;
                sync_dir(&out.join(&c.id).join("src"), &c.files)?;
                deps.push_str(&format!("{name} = {{ path = \"../{}\" }}\n", c.id));
                crate_ids.push(c.id.clone());
            }
            // Code crates an earlier translation made and this one doesn't.
            for e in fs::read_dir(out).map_err(|e| e.to_string())?.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("code-") && !crate_ids.contains(&n) {
                    fs::remove_dir_all(e.path()).map_err(|e| e.to_string())?;
                }
            }
            let lib = lib.replacen("pub fn init(rt: &mut Rt) {\n", "pub fn init(rt: &mut Rt) {\n    link_crates();\n", 1);
            (
                vec![("lib.rs".to_owned(), format!("{lib}\n{}", split.engine))],
                deps,
                format!("/// The runtime (also for the hook helpers compiled into the game).\npub use {runtime_crate} as compat;\npub use compat as horizon_compat;\nuse horizon_compat::"),
            )
        }
    };
    crate_ids.push("engine".into());
    put(
        out.join("engine/Cargo.toml"),
        format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{deps}", package(&krate)),
    )
    .map_err(|e| e.to_string())?;
    let mut engine_files = Vec::new();
    for (name, mut text) in files {
        if name == "lib.rs" {
            text = text.replacen("use horizon_compat::", &root, 1);
            text.push_str(&format!(
                "\nthread_local! {{\n    static RUNTIME: std::cell::RefCell<Option<Rt>> = const {{ std::cell::RefCell::new(None) }};\n    /// Where the runtime is ([`Rt::address`]), which a hook run from inside one of its calls finds it by.\n    static RUNTIME_AT: std::cell::Cell<usize> = const {{ std::cell::Cell::new(0) }};\n}}\n\n\
                 /// The mod's runtime on this thread, made and registered on first use. Not from inside one of its\n\
                 /// own calls (it is busy then): the game's hooks go through [`hook_runtime`].\n\
                 pub fn with_runtime<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    RUNTIME.with(|cell| {{\n        let mut slot = cell.borrow_mut();\n        let rt = slot.get_or_insert_with(|| {{\n            let mut rt = Rt::new();\n            rt.namespace = {id:?}.into();\n            init(&mut rt);\n            start(&mut rt);\n            rt\n        }});\n        RUNTIME_AT.with(|at| at.set(rt.address()));\n        f(rt)\n    }})\n}}\n\n\
                 /// Whether the mod's runtime is in a call on this thread: a hook the game runs from inside one (the\n\
                 /// mod's code places a block, and the game asks for the block's block entity) finds it so.\n\
                 pub fn runtime_busy() -> bool {{\n    RUNTIME.with(|cell| cell.try_borrow_mut().is_err())\n}}\n\n\
                 /// [`with_runtime`] for a hook the game runs. When the runtime is busy on this thread (the game ran\n\
                 /// the hook from inside one of the mod's calls), the runtime runs it where it waits for the game\n\
                 /// ([`Rt::host_call`]), as the JVM runs an event's listeners inside the call that posted it. Where it\n\
                 /// doesn't wait (a call that can't take a hook), the hook does nothing (`R::default()`, which\n\
                 /// leaves it to the game), and says so once.\n\
                 pub fn hook_runtime<R: Default>(hook: &'static str, f: impl FnOnce(&mut Rt) -> R) -> R {{\n    if runtime_busy() {{\n        thread_local! {{\n            static TOLD: std::cell::RefCell<Vec<(&'static str, bool)>> = const {{ std::cell::RefCell::new(Vec::new()) }};\n        }}\n        let mut f = Some(f);\n        let at = RUNTIME_AT.with(|at| at.get());\n        let ran = horizon_compat::reenter(at, |rt| f.take().map(|f| f(rt))).flatten();\n        let first = TOLD.with(|t| !t.borrow().contains(&(hook, ran.is_some())) && {{ t.borrow_mut().push((hook, ran.is_some())); true }});\n        if let Some(r) = ran {{\n            if first && Rt::tracing() {{\n                eprintln!(\"{id}: the game ran the {{hook}} hook from inside one of the mod's own calls; run there\");\n            }}\n            return r;\n        }}\n        if first {{\n            eprintln!(\"{id}: the game ran the {{hook}} hook from inside one of the mod's own calls; skipped\");\n        }}\n        return R::default();\n    }}\n    with_runtime(f)\n}}\n\n\
                 /// A question about the mod's classes and registrations alone (a block's block entity type, an\n\
                 /// item's use duration, a block's state for a placement): the runtime answers it, or the probe\n\
                 /// runtime while the runtime is busy (a block the mod's own code places asks from inside its call).\n\
                 pub fn with_registrations<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    if runtime_busy() {{ with_probe_runtime(f) }} else {{ with_runtime(f) }}\n}}\n\n\
                 thread_local! {{\n    static PROBE: std::cell::RefCell<Option<Rt>> = const {{ std::cell::RefCell::new(None) }};\n}}\n\n\
                 /// A runtime of its own on this thread, for questions asked before a game runs (`native_content_block`):\n\
                 /// it may fire the client's setup without changing the side [`with_runtime`]'s plays.\n\
                 pub fn with_probe_runtime<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    PROBE.with(|cell| {{\n        let mut slot = cell.borrow_mut();\n        let rt = slot.get_or_insert_with(|| {{\n            let mut rt = Rt::new();\n            rt.namespace = {id:?}.into();\n            init(&mut rt);\n            start(&mut rt);\n            rt\n        }});\n        f(rt)\n    }})\n}}\n\n\
                 /// The runtime the world generator runs the mod's worldgen code on (its placement modifiers):\n\
                 /// one for every generating thread, taken in turn. None on a thread already inside it.\n\
                 pub fn with_worldgen_runtime<R>(f: impl FnOnce(&mut Rt) -> R) -> Option<R> {{\n    struct Worldgen(Rt);\n    // SAFETY: the runtime is reached only through the mutex below, and holds no host between calls\n    // (`Rt::with_host` clears it); the runtime's thread-local caches are keyed by static keys.\n    unsafe impl Send for Worldgen {{}}\n    static WORLDGEN: std::sync::Mutex<Option<Worldgen>> = std::sync::Mutex::new(None);\n    thread_local! {{\n        static INSIDE: std::cell::Cell<bool> = const {{ std::cell::Cell::new(false) }};\n    }}\n    struct Leave;\n    impl Drop for Leave {{\n        fn drop(&mut self) {{\n            INSIDE.with(|i| i.set(false));\n        }}\n    }}\n    if INSIDE.with(|i| i.replace(true)) {{\n        return None;\n    }}\n    let _leave = Leave;\n    let mut slot = WORLDGEN.lock().unwrap_or_else(|e| e.into_inner());\n    let w = slot.get_or_insert_with(|| {{\n        let mut rt = Rt::new();\n        rt.namespace = {id:?}.into();\n        init(&mut rt);\n        start(&mut rt);\n        Worldgen(rt)\n    }});\n    Some(f(&mut w.0))\n}}\n\n\
                 /// A question the world generator asks about the mod's blocks (whether one survives where a\n\
                 /// feature would place it): the worldgen runtime, or, while that one is in a call on this thread (the\n\
                 /// mod's feature places a vanilla one, which asks), this thread's probe runtime. None while both are.\n\
                 pub fn with_worldgen_query<R>(f: impl FnOnce(&mut Rt) -> R) -> Option<R> {{\n    let mut f = Some(f);\n    if let Some(r) = with_worldgen_runtime(|rt| f.take().map(|f| f(rt))).flatten() {{\n        return Some(r);\n    }}\n    let f = f.take()?;\n    PROBE.with(|cell| {{\n        let mut slot = cell.try_borrow_mut().ok()?;\n        let rt = slot.get_or_insert_with(|| {{\n            let mut rt = Rt::new();\n            rt.namespace = {id:?}.into();\n            init(&mut rt);\n            start(&mut rt);\n            rt\n        }});\n        Some(f(rt))\n    }})\n}}\n"
            ));
        }
        if name == "lib.rs" {
            text.push_str(&format!(
                "\n/// The mods this engine's code is: a mod and the mods built on it, translated together.\n\
                 pub const NAMESPACES: &[&str] = &{namespaces:?};\n\n\
                 /// Whether `id` (`create:shaft`, a block state's text) is one of this engine's mods'.\n\
                 pub fn ours(id: &str) -> bool {{\n    id.split_once(':').is_some_and(|(ns, _)| NAMESPACES.contains(&ns))\n}}\n"
            ));
        }
        engine_files.push((name, text));
    }
    sync_dir(&src, &engine_files)?;
    put(out.join("horizon-mixin.toml"), mixin_toml(&bundle_id, "0.1.0", &crate_ids)).map_err(|e| e.to_string())?;
    for (hook, text) in block_entity_helpers(&bundle_id, &krate).into_iter().chain(client_helpers(&bundle_id, &krate)) {
        put(out.join(format!("{hook}.rs")), text).map_err(|e| e.to_string())?;
    }
    for (hook, glue) in BLOCK_HOOKS {
        put(out.join(format!("{hook}.rs")), helper(&bundle_id, &krate, hook, glue)).map_err(|e| e.to_string())?;
    }
    // The blocks the code registers, as a content mod per mod (a folder mod for `horizon-mods/`):
    // the game knows them, and their random ticks reach the translated `randomTick`. A block is the
    // mod's whose classes register it (the first mod's, for its bundled libraries'); the content
    // hook replaces these first declarations with what the registration makes.
    let registered = crate::registrations(classes);
    let props = crate::props::block_properties(classes);
    let owners: Vec<std::collections::HashSet<String>> = members[1..].iter().map(|(j, _)| crate::class_names(j)).collect();
    for (n, (j, ns)) in members.iter().enumerate() {
        let blocks: Vec<String> = registered
            .iter()
            .filter(|r| r.registry == "block")
            .filter(|r| match n {
                0 => owners.iter().all(|o| !o.contains(&r.class)),
                _ => owners[n - 1].contains(&r.class),
            })
            .map(|r| r.name.rsplit(':').next().unwrap_or(&r.name).to_owned())
            .collect();
        let content = if n == 0 { out.join("content") } else { out.join(format!("content-{ns}")) };
        let dir = content.join("data").join(ns).join("horizon/block");
        if !blocks.is_empty() {
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        for b in &blocks {
            // Stone has no state properties and no behaviour of its own; `random_ticks` keeps the
            // mod's `randomTick` reachable.
            let mut json = String::from("{ \"template\": \"minecraft:stone\", \"properties\": { \"random_ticks\": true }, \"item\": true");
            if let Some(ps) = props.get(b.as_str()) {
                let defs: Vec<String> = ps
                    .iter()
                    .map(|p| {
                        if p.values == ["false", "true"] {
                            format!("\"{}\": {{ \"type\": \"boolean\" }}", p.name)
                        } else {
                            let vs: Vec<String> = p.values.iter().map(|v| format!("\"{v}\"")).collect();
                            format!("\"{}\": {{ \"type\": \"enum\", \"values\": [{}] }}", p.name, vs.join(", "))
                        }
                    })
                    .collect();
                if !defs.is_empty() {
                    json.push_str(&format!(", \"state_properties\": {{ {} }}", defs.join(", ")));
                }
            }
            json.push_str(" }\n");
            put(dir.join(format!("{b}.json")), json).map_err(|e| e.to_string())?;
        }
        // A mod without blocks still has its look (lang, screens' textures) for its client code.
        copy_assets(j, ns, &content)?;
        if content.is_dir() {
            put(content.join("horizon-mod.toml"), manifest(ns, &program.metadata)).map_err(|e| e.to_string())?;
        }
    }
    // The mod jars the members bundle for jar-in-jar (Create's Flywheel and Ponder): their code is
    // in the engine, their look (Flywheel's shaders, Ponder's lang and textures) a content mod of
    // their own, `content-nested-<id>`, as JarJar loads each as a mod of its own; a member of the
    // group already is one.
    for (j, _) in &members {
        for (nid, bytes) in crate::nested_mods(j) {
            let content = out.join(format!("content-nested-{nid}"));
            if members.iter().any(|(_, m)| *m == nid) || content.exists() {
                continue;
            }
            if let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
                let item_models = copy_namespace_assets(&mut zip, &nid, &content);
                write_item_definitions(&nid, &item_models, &content)?;
            }
            if content.is_dir() {
                put(content.join("horizon-mod.toml"), manifest(&nid, &program.metadata)).map_err(|e| e.to_string())?;
            }
        }
    }
    let left: Vec<String> = program.unsupported.iter().map(|(k, n)| format!("{k} ({n})")).collect();
    Ok((id, left))
}

/// A translated mod's content manifest: it declares registries (the engine's blocks and items).
/// Its name and version are the mod's own (`mods.toml`'s `displayName` and `version`, in
/// `metadata` as [`crate::mod_metadata`] reads them), as the Mods page shows them.
fn manifest(id: &str, metadata: &[(String, String, String)]) -> String {
    let (version, name) = metadata
        .iter()
        .find(|m| m.0 == id)
        .map(|m| (m.1.as_str(), if m.2.is_empty() { id } else { m.2.as_str() }))
        .filter(|(v, _)| !v.is_empty())
        .unwrap_or(("0.1.0", id));
    // A JSON string is a TOML basic string.
    let quote = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    let description = format!("{name}'s content (blocks, items, models, textures, sounds, text), from its NeoForge jar");
    format!(
        "id = \"{id}\"\nname = {}\nversion = {}\ndescription = {}\napi = \"0.2\"\ncapabilities = [\"registries\"]\n",
        quote(name),
        quote(version),
        quote(&description)
    )
}

/// The files of a jar's `assets/` a content mod carries, by extension: what vanilla's resource
/// loading reads (models, textures, sounds, lang, `.mcmeta`, OBJ models), OptiFine's `.properties`
/// (`horizon-optifine`), and what mods read through the resource manager themselves: Ponder's
/// scenes (`.nbt`), structure text (`.snbt`), shader sources (Flywheel's `.glsl`, `.vert`, `.frag`,
/// core shaders' `.vsh`, `.fsh`) and text (`.txt`).
const ASSET_TYPES: [&str; 15] = ["json", "png", "mcmeta", "ogg", "obj", "mtl", "properties", "nbt", "snbt", "glsl", "vert", "frag", "vsh", "fsh", "txt"];

/// Whether a content mod carries jar entry `path` (`assets/create/ponder/belt.nbt`): one of
/// [`ASSET_TYPES`], each part of its path of vanilla's resource-location characters
/// (`[a-z0-9_.-]`, as `horizon_mods::content::check_path` asks: one other file and the game
/// refuses the whole content mod).
fn carried_asset(path: &str) -> bool {
    let plain = |part: &str| !part.is_empty() && part != "." && part != ".." && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
    path.rsplit_once('.').is_some_and(|(_, ext)| ASSET_TYPES.contains(&ext)) && path.split('/').all(plain)
}

/// Copies the files under `assets/<ns>/` of `zip` a content mod carries ([`carried_asset`]) into
/// `content`; the item models among them (`m` of `<ns>:item/<m>`) come back.
fn copy_namespace_assets<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, ns: &str, content: &Path) -> Vec<String> {
    let prefix = format!("assets/{ns}/");
    let mut item_models = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut e) = zip.by_index(i) else { continue };
        let Some(name) = e.enclosed_name() else { continue };
        let path = name.to_string_lossy().into_owned();
        if !e.is_file() || !path.starts_with(&prefix) || !carried_asset(&path) {
            continue;
        }
        if let Some(model) = path.strip_prefix(&format!("{prefix}models/item/")).and_then(|m| m.strip_suffix(".json")) {
            item_models.push(model.to_owned());
        }
        let target = content.join(&name);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut bytes = Vec::new();
        if e.read_to_end(&mut bytes).is_ok() {
            let _ = put(target, bytes);
        }
    }
    item_models
}

/// A jar for 1.21.1 has no client item definitions (1.21.4 put item models behind
/// `items/<name>.json`): without one an item draws no icon. Each item model of `id` in `content`
/// gets the plain definition, as the port-pack's carry-over writes, or its overrides; the jar's own
/// win.
fn write_item_definitions(id: &str, item_models: &[String], content: &Path) -> Result<(), String> {
    for m in item_models {
        let def = content.join(format!("assets/{id}/items/{m}.json"));
        if !def.exists() {
            if let Some(parent) = def.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let model = fs::read(content.join(format!("assets/{id}/models/item/{m}.json")))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            put(def, item_definition(id, m, model.as_ref())).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Mod `id`'s own look into content mod `content`: its assets (blockstates, models, textures, lang,
/// Ponder's scenes, shader sources) from `jar`, with a plain item definition for each item model a
/// 1.21.1 jar leaves without one.
fn copy_assets(jar: &Path, id: &str, content: &Path) -> Result<(), String> {
    let Ok(f) = fs::File::open(jar) else { return Ok(()) };
    let Ok(mut zip) = zip::ZipArchive::new(f) else { return Ok(()) };
    let item_models = copy_namespace_assets(&mut zip, id, content);
    write_item_definitions(id, &item_models, content)
}

/// The client item definition (`items/<m>.json`) of the mod's item model `<id>:item/<m>`: the
/// model itself, or, when the model has `overrides` of its own (1.21.1's, which 1.21.4 dropped),
/// Horizon's `horizon:overrides` over it: the last override whose predicates all hold draws its
/// model instead (`ItemOverrides.resolve`).
fn item_definition(id: &str, m: &str, model: Option<&serde_json::Value>) -> String {
    let overrides: Vec<serde_json::Value> = model
        .and_then(|v| v.get("overrides"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|o| {
            // `ResourceLocation.parse`: a bare path is vanilla's.
            let location = o.get("model")?.as_str()?;
            let location = if location.contains(':') { location.to_owned() } else { format!("minecraft:{location}") };
            let predicate = o.get("predicate").cloned().unwrap_or_else(|| serde_json::json!({}));
            Some(serde_json::json!({"predicate": predicate, "model": {"type": "minecraft:model", "model": location}}))
        })
        .collect();
    if overrides.is_empty() {
        return format!("{{\"model\": {{\"type\": \"minecraft:model\", \"model\": \"{id}:item/{m}\"}}}}\n");
    }
    let base = serde_json::json!({"type": "minecraft:model", "model": format!("{id}:item/{m}")});
    format!("{}\n", serde_json::json!({"model": {"type": "horizon:overrides", "base": base, "overrides": overrides}}))
}

#[cfg(test)]
mod tests {
    /// A mod's look as content mods: its own assets of the kinds the game and mods read (Ponder's
    /// scenes, shader sources, lang), none whose path vanilla couldn't name, also for a mod without
    /// blocks; each mod it bundles for jar-in-jar (Flywheel) a content mod of its own, a library
    /// that declares no mod (Registrate) none.
    #[test]
    fn a_mods_look_and_the_mods_it_bundles_become_content_mods() {
        let dir = std::env::temp_dir().join(format!("hz-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_of = |files: &[(&str, &[u8])]| -> Vec<u8> {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, body) in files {
                zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                std::io::Write::write_all(&mut zip, body).unwrap();
            }
            zip.finish().unwrap().into_inner()
        };
        let toml = |id: &str| format!("modLoader=\"javafml\"\n[[mods]]\nmodId=\"{id}\"\n");
        let flywheel_toml = "modLoader=\"javafml\"\n[[mods]]\nmodId=\"flywheel\"\nversion=\"1.0.6\"\ndisplayName=\"Flywheel \\\"engine\\\"\"\n";
        let flywheel = zip_of(&[
            ("META-INF/neoforge.mods.toml", flywheel_toml.as_bytes()),
            ("assets/flywheel/flywheel/internal/components_header.frag", b"// frag"),
            ("assets/flywheel/lang/en_us.json", b"{}"),
        ]);
        let registrate = zip_of(&[("com/tterrag/registrate/Registrate.class", b"\xca\xfe")]);
        let jar = dir.join("gears.jar");
        let main = zip_of(&[
            ("META-INF/neoforge.mods.toml", toml("gears").as_bytes()),
            ("META-INF/jarjar/flywheel.jar", &flywheel),
            ("META-INF/jarjar/registrate.jar", &registrate),
            ("assets/gears/ponder/belt.nbt", b"nbt"),
            ("assets/gears/lang/en_us.json", b"{}"),
            ("assets/gears/models/item/gear.json", br#"{"parent": "item/generated"}"#),
            // Not a resource location (the game would refuse the whole content mod), not a
            // resource, and another namespace's.
            ("assets/gears/textures/Gear.png", b"png"),
            ("assets/gears/notes.md", b"# notes"),
            ("assets/other/lang/en_us.json", b"{}"),
        ]);
        std::fs::write(&jar, main).unwrap();
        let out = dir.join("out");
        super::write_group(&[jar.as_path()], &[], &out, None).unwrap();
        let has = |rel: &str| out.join(rel).is_file();
        assert!(has("content/horizon-mod.toml"));
        assert!(has("content/assets/gears/ponder/belt.nbt"));
        assert!(has("content/assets/gears/lang/en_us.json"));
        assert!(has("content/assets/gears/items/gear.json"), "an item definition for its item model");
        assert!(!has("content/assets/gears/textures/Gear.png"));
        assert!(!has("content/assets/gears/notes.md"));
        assert!(!has("content/assets/other/lang/en_us.json"));
        assert!(has("content-nested-flywheel/assets/flywheel/flywheel/internal/components_header.frag"));
        assert!(has("content-nested-flywheel/assets/flywheel/lang/en_us.json"));
        let manifest = std::fs::read_to_string(out.join("content-nested-flywheel/horizon-mod.toml")).unwrap();
        // Named and versioned as the mod is on the Mods page, and read back by the game.
        let parsed = horizon_mods::Manifest::parse(&manifest).unwrap();
        assert_eq!((parsed.id.as_str(), parsed.name.as_str(), parsed.version.as_str()), ("flywheel", "Flywheel \"engine\"", "1.0.6"));
        let own = horizon_mods::Manifest::parse(&std::fs::read_to_string(out.join("content/horizon-mod.toml")).unwrap()).unwrap();
        assert_eq!((own.name.as_str(), own.version.as_str()), ("gears", "0.1.0"), "a mod without a name or version");
        assert!(!out.join("content-nested-registrate").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// mods.toml values as jars write them: quoted, single-quoted, bare, and with a comment after
    /// (Torchmaster's, JEI's, Guard Villagers' `modId="..." #mandatory`).
    #[test]
    fn a_toml_value_ends_where_its_string_does() {
        assert_eq!(super::toml_string(r#""torchmaster" #mandatory"#), "torchmaster");
        assert_eq!(super::toml_string(" 'jei' "), "jei");
        assert_eq!(super::toml_string("create # the id"), "create");
        assert_eq!(super::toml_string(r#""a#b""#), "a#b");
        assert_eq!(super::toml_string(r#""The \"Mod\" \\ \u00e9" # its name"#), "The \"Mod\" \\ \u{e9}");
    }

    /// A 1.21.1 item model's `overrides` (Create's clipboard) become Horizon's `horizon:overrides`
    /// over the model, in their order (the last that holds wins); a model without any keeps the
    /// plain definition.
    #[test]
    fn item_models_with_overrides_get_an_overrides_definition() {
        let clipboard = serde_json::json!({
            "parent": "create:item/clipboard_0",
            "overrides": [
                {"predicate": {"create:clipboard_type": 0.5}, "model": "create:item/clipboard_1"},
                {"predicate": {"create:clipboard_type": 1.0}, "model": "item/paper"}
            ]
        });
        let def: serde_json::Value = serde_json::from_str(&super::item_definition("create", "clipboard", Some(&clipboard))).unwrap();
        let leaf = |m: &str| serde_json::json!({"type": "minecraft:model", "model": m});
        assert_eq!(
            def,
            serde_json::json!({"model": {
                "type": "horizon:overrides",
                "base": leaf("create:item/clipboard"),
                "overrides": [
                    {"predicate": {"create:clipboard_type": 0.5}, "model": leaf("create:item/clipboard_1")},
                    {"predicate": {"create:clipboard_type": 1.0}, "model": leaf("minecraft:item/paper")}
                ]
            }})
        );
        let plain = serde_json::json!({"parent": "item/generated"});
        for model in [Some(&plain), None] {
            let def: serde_json::Value = serde_json::from_str(&super::item_definition("create", "wrench", model)).unwrap();
            assert_eq!(def, serde_json::json!({"model": leaf("create:item/wrench")}));
        }
    }

    /// Every helper a bundle writes is injected (the manifest names it, with its hook), and every
    /// hook it names is one the game has: in the catalog or a fragment still waiting for its number.
    /// A helper the manifest leaves out is never called.
    #[test]
    fn every_helper_is_injected_into_a_hook_the_game_has() {
        let mut written: Vec<String> = super::block_entity_helpers("m", "m_engine")
            .into_iter()
            .chain(super::client_helpers("m", "m_engine"))
            .map(|(hook, _)| format!("{hook}.rs"))
            .chain(super::BLOCK_HOOKS.iter().map(|(hook, _)| format!("{hook}.rs")))
            .collect();
        written.sort();
        let manifest = super::mixin_toml("m", "0.1.0", &[]);
        let field = |key: &str| -> Vec<String> {
            let prefix = format!("{key} = \"");
            manifest.lines().filter_map(|l| l.strip_prefix(prefix.as_str())?.strip_suffix('"')).map(str::to_owned).collect()
        };
        let mut injected = field("helper");
        injected.sort();
        assert_eq!(injected, written, "helpers written and injected");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let names = |text: &str| -> Vec<String> {
            let json: serde_json::Value = serde_json::from_str(text).unwrap();
            json["hooks"].as_array().into_iter().flatten().filter_map(|h| h["name"].as_str()).map(str::to_owned).collect()
        };
        let mut known = names(&std::fs::read_to_string(root.join("native-hooks.json")).unwrap());
        for fragment in std::fs::read_dir(root.join("tools/native-hook-fragments")).unwrap().flatten() {
            if fragment.path().extension().is_some_and(|e| e == "json") {
                known.extend(names(&std::fs::read_to_string(fragment.path()).unwrap()));
            }
        }
        for hook in field("hook") {
            assert!(known.contains(&hook), "the game has no hook `{hook}`");
        }
    }

    /// The mixin tool takes a helper file only as one private function: no attributes (an `#[allow]`
    /// included), no `pub`, `async`, `const` or `unsafe`, no generic parameters, and nothing else at
    /// the top of the file. A helper it refuses stops the game's build.
    #[test]
    fn every_helper_is_one_plain_private_function() {
        let helpers = super::block_entity_helpers("m", "m_engine")
            .into_iter()
            .chain(super::client_helpers("m", "m_engine"))
            .chain(super::BLOCK_HOOKS.iter().map(|(hook, glue)| (*hook, super::helper("m", "m_engine", hook, glue))));
        for (hook, text) in helpers {
            let top: Vec<&str> = text.lines().filter(|l| !l.is_empty() && !l.starts_with(char::is_whitespace) && !l.starts_with("//")).collect();
            let functions: Vec<&str> = top.iter().copied().filter(|l| l.starts_with("fn ")).collect();
            assert_eq!(functions.len(), 1, "`{hook}`: one function at the top, not {top:?}");
            let name = functions[0].trim_start_matches("fn ").split('(').next().unwrap_or("");
            assert!(!name.contains('<'), "`{hook}`: no generic parameters on `{name}`");
            for line in top.iter().filter(|l| !l.starts_with("fn ")) {
                assert!(line.starts_with(')') || line.starts_with('}'), "`{hook}`: only the function at the top, not `{line}`");
            }
        }
    }

    #[test]
    fn bundles_carry_every_runtime_source_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut on_disk: Vec<String> = ["horizon-compat-core/src", "horizon-compat/src"]
            .iter()
            .flat_map(|d| std::fs::read_dir(root.join(d)).unwrap().flatten())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".rs") && n != "lib.rs" && !super::TOOL_ONLY.contains(&n.as_str()))
            .collect();
        on_disk.sort();
        let mut carried: Vec<String> = super::COMPAT.iter().map(|(n, _)| n.to_string()).collect();
        carried.sort();
        assert_eq!(carried, on_disk);
        // The inlined runtime is one crate with the core's entry points left out: carried code names
        // the core as `crate`, never as the `horizon_compat_core` crate (tests aside).
        for (n, text) in super::COMPAT {
            let code = text.split("#[cfg(test)]").next().unwrap_or(text);
            let named = code.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| l.contains("horizon_compat_core::"));
            assert!(!named, "{n} calls the core crate, which an inlined runtime doesn't have");
        }
        // The inlined core declares every carried mapping module and drops the installable entry points.
        let core = super::inlined_core();
        assert!(!core.contains("pub fn install_fns"));
        for n in carried.iter().filter(|n| *n != "host.rs") {
            assert!(core.contains(&format!("pub mod {};", n.trim_end_matches(".rs"))), "{n}");
        }
    }
}
