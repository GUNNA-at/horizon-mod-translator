//! A translated jar as a Horizon native mod bundle (docs/native-mods.md):
//! `horizon-mixin.toml`, one helper per hook point that turns Horizon's
//! `GameLevel` into a [`horizon_compat::host::Host`] and calls the mod
//! through `horizon_compat::glue`, and `engine/`, the crate holding the
//! translated code (with `horizon-compat` from Horizon's workspace).
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
                return Some(v.trim().trim_matches('"').to_owned());
            }
        }
    }
    None
}

/// The runtime's sources, carried in each bundle as the engine's own `compat` module: bundle
/// crates may depend only on Horizon's Cargo.lock registry crates (horizon-mixins' rule; the
/// engine names [`ENGINE_DEPENDENCIES`] for them). The core's root becomes `compat/mod.rs`;
/// horizon-compat's root is left out (its modules are declared there instead).
const CORE: &str = include_str!("../../horizon-compat-core/src/lib.rs");
/// The mapping's root: its runtime helpers (between the `RUNTIME HELPERS` markers) go into the
/// inlined `compat` module beside the core's.
const COMPAT_ROOT: &str = include_str!("../../horizon-compat/src/lib.rs");
const COMPAT: [(&str, &str); 47] = [
    ("host.rs", include_str!("../../horizon-compat-core/src/host.rs")),
    ("gc.rs", include_str!("../../horizon-compat-core/src/gc.rs")),
    ("api.rs", include_str!("../../horizon-compat/src/api.rs")),
    ("api_common.rs", include_str!("../../horizon-compat/src/api_common.rs")),
    ("blocks.rs", include_str!("../../horizon-compat/src/blocks.rs")),
    ("codecs.rs", include_str!("../../horizon-compat/src/codecs.rs")),
    ("recipes.rs", include_str!("../../horizon-compat/src/recipes.rs")),
    ("crafting.rs", include_str!("../../horizon-compat/src/crafting.rs")),
    ("handlers.rs", include_str!("../../horizon-compat/src/handlers.rs")),
    ("components.rs", include_str!("../../horizon-compat/src/components.rs")),
    ("container_screens.rs", include_str!("../../horizon-compat/src/container_screens.rs")),
    ("capabilities.rs", include_str!("../../horizon-compat/src/capabilities.rs")),
    ("entities.rs", include_str!("../../horizon-compat/src/entities.rs")),
    ("tanks.rs", include_str!("../../horizon-compat/src/tanks.rs")),
    ("creative.rs", include_str!("../../horizon-compat/src/creative.rs")),
    ("fluids.rs", include_str!("../../horizon-compat/src/fluids.rs")),
    ("neo_tags.rs", include_str!("../../horizon-compat/src/neo_tags.rs")),
    ("mod_entities.rs", include_str!("../../horizon-compat/src/mod_entities.rs")),
    ("palette.rs", include_str!("../../horizon-compat/src/palette.rs")),
    ("particles.rs", include_str!("../../horizon-compat/src/particles.rs")),
    ("records.rs", include_str!("../../horizon-compat/src/records.rs")),
    ("rotation.rs", include_str!("../../horizon-compat/src/rotation.rs")),
    ("vanilla_records.rs", include_str!("../../horizon-compat/src/vanilla_records.rs")),
    ("vanilla_shapes.rs", include_str!("../../horizon-compat/src/vanilla_shapes.rs")),
    ("functions.rs", include_str!("../../horizon-compat/src/functions.rs")),
    ("shapes.rs", include_str!("../../horizon-compat/src/shapes.rs")),
    ("sounds.rs", include_str!("../../horizon-compat/src/sounds.rs")),
    ("vanilla_sounds.rs", include_str!("../../horizon-compat/src/vanilla_sounds.rs")),
    ("vanilla_effects.rs", include_str!("../../horizon-compat/src/vanilla_effects.rs")),
    ("food.rs", include_str!("../../horizon-compat/src/food.rs")),
    ("item_use.rs", include_str!("../../horizon-compat/src/item_use.rs")),
    ("nbt.rs", include_str!("../../horizon-compat/src/nbt.rs")),
    ("glue.rs", include_str!("../../horizon-compat/src/glue.rs")),
    ("gui.rs", include_str!("../../horizon-compat/src/gui.rs")),
    ("screens.rs", include_str!("../../horizon-compat/src/screens.rs")),
    ("hand.rs", include_str!("../../horizon-compat/src/hand.rs")),
    ("events.rs", include_str!("../../horizon-compat/src/events.rs")),
    ("jutil.rs", include_str!("../../horizon-compat/src/jutil.rs")),
    ("menus.rs", include_str!("../../horizon-compat/src/menus.rs")),
    ("widgets.rs", include_str!("../../horizon-compat/src/widgets.rs")),
    ("neo.rs", include_str!("../../horizon-compat/src/neo.rs")),
    ("network.rs", include_str!("../../horizon-compat/src/network.rs")),
    ("jlang.rs", include_str!("../../horizon-compat/src/jlang.rs")),
    ("tags.rs", include_str!("../../horizon-compat/src/tags.rs")),
    ("render.rs", include_str!("../../horizon-compat/src/render.rs")),
    ("quads.rs", include_str!("../../horizon-compat/src/quads.rs")),
    ("vanilla_tags.rs", include_str!("../../horizon-compat/src/vanilla_tags.rs")),
];

/// The registry crates the carried runtime uses, as the engine's `[dependencies]`: each must be
/// in Horizon's Cargo.lock, at a version that lock holds.
const ENGINE_DEPENDENCIES: &str = "serde_json = \"1.0.151\"\n";

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
    // `Rt::is_a` asks the mapping's table of Minecraft's class families.
    format!("{head}{tail}\n{mods}pub use api_common::api_is_a;\n{helpers}")
}

/// Writes `contents` unless the file already holds exactly that, so an unchanged engine keeps its
/// mtimes and Cargo does not rebuild it.
fn put(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    let (path, contents) = (path.as_ref(), contents.as_ref());
    if fs::read(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    fs::write(path, contents)
}

/// Block hooks the bundle wires: (hook name, glue function).
const BLOCK_HOOKS: [(&str, &str); 2] = [("tick_block", "tick_block"), ("random_tick", "random_tick")];

fn helper(id: &str, krate: &str, hook: &str, glue: &str) -> String {
    format!(
        r#"// Generated by horizon-mod-translator: Horizon's `{hook}` hook → the translated mod.
fn {id}_{hook}(
    level: &mut crate::game::GameLevel<'_>,
    ticks: &mut horizon_world::level::Ticks,
    pos: horizon_core::position::BlockPos,
    handled: &mut bool,
) {{
    struct Host<'a, 'b, 'c> {{
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
            self.level.native_state_facts(state).map(|(hardness, push, replaceable)| {krate}::compat::host::BlockFacts {{ hardness, push, replaceable }})
        }}
        fn dimension(&self) -> String {{
            self.level.native_dimension_name().to_owned()
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
    if *handled {{
        return;
    }}
    let p = [pos.x, pos.y, pos.z];
    let state = level.native_state_text(p);
    if !state.starts_with("{id}:") {{
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
    format!(r#"        fn play_sound(&mut self, s: &{c}::host::Sound) {{
            self.game.native_level_sound(&s.event, s.at, s.source, s.volume, s.pitch);
        }}
        fn particles(&mut self, p: &{c}::host::Particles) {{
            let Some(count) = p.count else {{ return }};
            if let Some((options, state)) = self.game.native_particle_options(&p.kind, p.block.as_deref(), p.item.as_deref(), p.dust, p.at) {{
                self.game.native_level_particles(&options, state, p.at, p.delta, p.speed, count);
            }}
        }}
"#)
}

fn server_send(c: &str) -> String {
    format!(r#"        fn send_payload(&mut self, to: {c}::host::PayloadTarget, channel: &str, data: &[u8]) -> bool {{
            let players: Vec<u128> = self
                .game
                .native_server_players()
                .into_iter()
                .filter(|p| match to {{
                    {c}::host::PayloadTarget::Server => false,
                    {c}::host::PayloadTarget::Player(uuid) => p.uuid == uuid,
                    {c}::host::PayloadTarget::All => true,
                    {c}::host::PayloadTarget::Near {{ x, y, z, radius }} => {{
                        let d = [p.position[0] - x, p.position[1] - y, p.position[2] - z];
                        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= radius * radius
                    }}
                }})
                .map(|p| p.uuid)
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
            self.game.get().native_state_facts(state).map(|(hardness, push, replaceable)| {c}::host::BlockFacts {{ hardness, push, replaceable }})
        }}
        fn dimension(&self) -> String {{
            self.game.get().native_dimension().to_owned()
        }}
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
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
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.get().mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
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
        /// A stack of the player's with its mod data, as the runtime holds a hand's.
        fn held(&self, stack: &horizon_client::items::ItemStack) -> {c}::host::HandStack {{
            let game = self.game.get();
            let components = game
                .native_registered_components()
                .into_iter()
                .filter_map(|name| {{
                    let tag = game.native_component(stack, name).ok()??;
                    Some((name.to_owned(), to_nbt(tag)?))
                }})
                .collect();
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
fn client_helpers(id: &str, ns: &str, krate: &str) -> Vec<(&'static str, String)> {
    let c = format!("{krate}::compat");
    let conversions = nbt_conversions(&c);
    let client_host = client_host(&c, id);
    // The host a client's draws get: the game, and the frame's pose stack and geometry.
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
            self.game.native_state_facts(state).map(|(hardness, push, replaceable)| {c}::host::BlockFacts {{ hardness, push, replaceable }})
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
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
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
        fn partial_tick(&self) -> f32 {{
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
    if !entity.id.starts_with("{ns}:") {{
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
{conversions}    if tag.is_some() || !entity.id.starts_with("{ns}:") {{
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
    if content.namespaces.first().map(String::as_str) != Some("{ns}") {{
        return;
    }}
    let (blocks, items, tabs) = {krate}::with_probe_runtime(|rt| ({c}::blocks::block_declarations(rt), {c}::blocks::item_declarations(rt), {c}::creative::tabs(rt)));
    let (mut declared, mut refused) = (0usize, 0usize);
    for (id, json) in blocks.iter() {{
        match content.declare_block(id, json) {{
            Ok(()) => declared += 1,
            Err(e) => {{
                refused += 1;
                log::warn!("{ns} (translated): block not declared: {{e}}");
            }}
        }}
    }}
    for (id, json) in items.iter() {{
        match content.declare_item(id, json) {{
            Ok(()) => declared += 1,
            Err(e) => {{
                refused += 1;
                log::warn!("{ns} (translated): item not declared: {{e}}");
            }}
        }}
    }}
    // Its creative tabs, as its own `displayItems` code fills them.
    let tab_count = tabs.len();
    for tab in tabs {{
        let tab = horizon_mods::content::CreativeTab {{ id: tab.id, title: tab.title, icon: tab.icon, items: tab.items }};
        match content.creative_tabs.iter_mut().find(|t| t.id == tab.id) {{
            Some(old) => *old = tab,
            None => content.creative_tabs.push(tab),
        }}
    }}
    log::info!("{ns} (translated): {{declared}} blocks and items declared from its registration, {{refused}} refused, {{tab_count}} creative tabs");
}}
"#)),
        ("content_block", format!(r#"// Generated by horizon-mod-translator: what draws the translated mod's blocks, by its own code
// (`getRenderShape`, the models its client swaps in), read once when its content is. States are
// asked one property at a time from the default: one property may decide (a sliding door is drawn
// by its renderer only while open).
fn {id}_content_block(block: &mut horizon_mods::content::Block) {{
    use horizon_mods::content::RenderShapes;
    if !block.id.starts_with("{ns}:") || block.properties.render_shape.is_some() {{
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
    if !entity.id.starts_with("{ns}:") {{
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
    if !stack.item.starts_with("{ns}:") {{
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
        ("net_from_server", format!(r#"// Generated by horizon-mod-translator: a mod payload the server sent (`native_net_from_server`), to
// the translated mod's handler for its type.
fn {id}_net_from_server(game: &mut crate::game::Game<'_>, channel: &str, data: &[u8]) {{
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("net_from_server", |rt| {c}::glue::payload_from_server(rt, &mut host, channel, data));
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
        ("interaction_key", format!(r#"// Generated by horizon-mod-translator: the window's use key acting with a hand
// (`InputEvent.InteractionKeyMappingTriggered`): the block under the crosshair hears
// `RightClickBlock` on the client, as `MultiPlayerGameMode.useItemOn` posts it (Create's value
// settings start holding there). The use goes on to the server either way.
fn {id}_interaction_key(
    game: &mut crate::game::Game<'_>,
    button: i32,
    key_mapping: &str,
    hand: horizon_client::first_person::Hand,
    swing_hand: &mut bool,
    canceled: &mut bool,
) {{
    let _ = (key_mapping, swing_hand, canceled);
    if button != 1 || !game.native_window() {{
        return;
    }}
    let hand = if matches!(hand, horizon_client::first_person::Hand::Off) {{ 1 }} else {{ 0 }};
{conversions}{client_host}    let mut host = Host {{ game: GameRef::Mut(game), batch: None }};
    {krate}::hook_runtime("interaction_key", |rt| {c}::glue::client_use(rt, &mut host, hand));
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
            self.game.native_state_facts(state).map(|(hardness, push, replaceable)| {c}::host::BlockFacts {{ hardness, push, replaceable }})
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
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
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
        fn item_entities(&self, b: [f64; 6]) -> Vec<{c}::host::ItemEntity> {{
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
{server_effects}{server_send}    }}
"#)
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
fn player_host(c: &str, server_effects: &str) -> String {
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
            self.game.native_state_facts(state).map(|(hardness, push, replaceable)| {c}::host::BlockFacts {{ hardness, push, replaceable }})
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
        fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {{
            let o = [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])];
            let boxes = self.game.mob_world_base(&[]).block_collision_boxes(pos[0], pos[1], pos[2]);
            Some(boxes.iter().map(|b| {{
                let (lo, hi) = (b.min_position(), b.max_position());
                [lo.x - o[0], lo.y - o[1], lo.z - o[2], hi.x - o[0], hi.y - o[1], hi.z - o[2]]
            }}).collect())
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
                all &= match value.as_ref().map(Self::tag_of) {{
                    Some(Some(tag)) => self.game.native_set_component(&mut stack, name, tag).is_ok(),
                    Some(None) => false,
                    None => self.game.native_remove_component(&mut stack, name).is_ok(),
                }};
            }}
            stack.count = count;
            stack.damage = damage;
            self.game.native_set_slot_stack(usize::from(slot), Some(stack)) && all
        }}
        fn update_player_hand(&mut self, hand: u8, count: u32, damage: u32, components: &[(String, Option<{c}::nbt::Nbt>)]) -> bool {{
            let hand = Self::hand(hand);
            let Some(mut stack) = self.game.native_hand_stack(hand).cloned() else {{
                return false;
            }};
            let mut all = true;
            for (name, value) in components {{
                all &= match value.as_ref().map(Self::tag_of) {{
                    Some(Some(tag)) => self.game.native_set_component(&mut stack, name, tag).is_ok(),
                    Some(None) => false,
                    None => self.game.native_remove_component(&mut stack, name).is_ok(),
                }};
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
{server_effects}    }}
    // A stack's mod data crosses as the runtime's NBT; a hand as `InteractionHand`'s ordinal.
    impl Host<'_, '_> {{
        fn hand(ordinal: u8) -> horizon_client::first_person::Hand {{
            if ordinal == 1 {{ horizon_client::first_person::Hand::Off }} else {{ horizon_client::first_person::Hand::Main }}
        }}
        /// The game's stack as the runtime's: its mod data by the registered component types.
        fn held_of(&self, stack: &horizon_client::items::ItemStack) -> {c}::host::HandStack {{
            let components = self
                .game
                .native_registered_components()
                .into_iter()
                .filter_map(|name| {{
                    let tag = self.game.native_component(stack, name).ok()??;
                    Some((name.to_owned(), Self::nbt_of(tag)?))
                }})
                .collect();
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
                if let Some(tag) = Self::tag_of(value) {{
                    let _ = self.game.native_set_component(&mut stack, name, tag);
                }}
            }}
            stack
        }}
        fn nbt_of(tag: &horizon_world::nbt::Tag) -> Option<{c}::nbt::Nbt> {{
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
        fn tag_of(n: &{c}::nbt::Nbt) -> Option<horizon_world::nbt::Tag> {{
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

fn block_entity_helpers(id: &str, ns: &str, krate: &str) -> Vec<(&'static str, String)> {
    let c = format!("{krate}::compat");
    let server_send = server_send(&c);
    let server_effects = server_effects(&c);
    // A player's hooks reach their client too: a menu the mod opens, a payload it sends.
    let player_host = player_host(&c, &format!("{server_effects}{server_send}"));
    let host = game_host(&c, &server_send);
    vec![
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
{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::hook_runtime("server_tick", |rt| {c}::glue::server_tick(rt, &mut host, post));
}}
"#)),
        ("player_tick", format!(r#"// Generated by horizon-mod-translator: the end of a player's server tick (`ServerPlayer.tick`),
// their state swapped in: what their client sent the mod's menus runs, and the menu they have open
// keeps their client in step and closes when it may no longer stay open.
fn {id}_player_tick(game: &mut crate::game::Game<'_>) {{
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("player_tick", |rt| {c}::glue::player_tick(rt, &mut host));
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
{server_effects}{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::hook_runtime("net_from_client", |rt| {c}::glue::payload_from_client(rt, &mut host, player, channel, data));
}}
"#)),
        ("block_entity", format!(r#"// Generated by horizon-mod-translator: `EntityBlock.newBlockEntity` of the translated mod.
fn {id}_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {{
    if entity.is_some() || !block.starts_with("{ns}:") {{
        return;
    }}
    if let Some(be) = {krate}::with_registrations(|rt| {c}::glue::block_entity_id(rt, block)) {{
        *entity = Some((be, horizon_world::nbt::Compound::new()));
    }}
}}
"#)),
        ("block_entity_ticks", format!(r#"// Generated by horizon-mod-translator: `EntityBlock.getTicker` of the translated mod.
fn {id}_block_entity_ticks(block: &str, ticks: &mut bool) {{
    if block.starts_with("{ns}:") && {krate}::with_registrations(|rt| {c}::glue::block_entity_ticks(rt, block)) {{
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
    if !entity.id.starts_with("{ns}:") {{
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
    // Where the click landed: the player's own look when it meets this block, else the face's
    // centre.
    let centre = |i: usize| pos[i] as f64 + 0.5 + face[i] as f64 * 0.5;
    let hit = game.native_player_pov_hit(0).filter(|h| h.0 == pos).map_or([centre(0), centre(1), centre(2)], |h| h.2);
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
    if !state.starts_with("{ns}:") {{
        return;
    }}
    if {krate}::hook_runtime("right_click_block", |rt| {c}::glue::use_block(rt, &mut host, pos, &state, face, hand)) {{
        *canceled = true;
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
    if !item.starts_with("{ns}:") {{
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
{player_host}    // The click lands on the clicked face's centre.
    let at = |i: usize| pos[i] as f64 + 0.5 + face[i] as f64 * 0.5;
    let click = {c}::glue::Placement {{
        pos,
        hit_pos: pos,
        face: face_id,
        hit: [at(0), at(1), at(2)],
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
    if !item.starts_with("{ns}:") {{
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
    if !stack.item.starts_with("{ns}:") {{
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
    if entity.is_some() || game.native_window() || !stack.item.starts_with("{ns}:") {{
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
    if entity.is_some() || game.native_window() || !stack.item.starts_with("{ns}:") {{
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
    if entity.is_some() || game.native_window() || !stack.item.starts_with("{ns}:") {{
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
    if entity.is_some() || game.native_window() || !stack.item.starts_with("{ns}:") {{
        return;
    }}
    let hand = u8::from(game.hand == horizon_client::first_person::Hand::Off);
{player_host}    let mut host = Host {{ game }};
    {krate}::hook_runtime("release_using", |rt| {c}::item_use::release_using(rt, &mut host, &stack.item, time_left, hand));
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
    if !text.starts_with("{ns}:") {{
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
            self.level.native_state_facts(state).map(|(hardness, push, replaceable)| {c}::host::BlockFacts {{ hardness, push, replaceable }})
        }}
        fn dimension(&self) -> String {{
            self.level.native_dimension_name().to_owned()
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
    ]
}

const CLIENT_BINDINGS: [(&str, &str); 21] = [
    ("component_types", "    { name = \"types\", ty = \"&mut Vec<String>\", value = \"&mut *types\" },\n"),
    ("level_submit", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"camera\", ty = \"[f64; 3]\", value = \"camera\" },\n    { name = \"draw\", ty = \"&mut crate::game::native_block_entity_draw::BlockEntityDraw\", value = \"&mut *draw\" },\n"),
    ("content", "    { name = \"content\", ty = \"&mut horizon_mods::Content\", value = \"&mut *content\" },\n"),
    ("content_block", "    { name = \"block\", ty = \"&mut horizon_mods::content::Block\", value = \"&mut *block\" },\n"),
    ("block_entity_submit", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"draw\", ty = \"&mut crate::game::native_block_entity_draw::BlockEntityDraw\", value = \"&mut *draw\" },\n"),
    ("block_entity_update_tag", "    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"tag\", ty = \"&mut Option<horizon_world::nbt::Compound>\", value = \"&mut *tag\" },\n"),
    ("tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n"),
    ("block_entity_data", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"tag\", ty = \"&horizon_world::nbt::Compound\", value = \"tag\" },\n"),
    ("net_from_server", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"channel\", ty = \"&str\", value = \"channel\" },\n    { name = \"data\", ty = \"&[u8]\", value = \"data\" },\n"),
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
    ("interaction_key", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"button\", ty = \"i32\", value = \"button\" },\n    { name = \"key_mapping\", ty = \"&str\", value = \"key_mapping\" },\n    { name = \"hand\", ty = \"horizon_client::first_person::Hand\", value = \"hand\" },\n    { name = \"swing_hand\", ty = \"&mut bool\", value = \"&mut *swing_hand\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
];

const BLOCK_ENTITY_BINDINGS: [(&str, &str); 17] = [
    ("level_tick_pre", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n"),
    ("level_tick_post", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"dimension\", ty = \"&str\", value = \"dimension\" },\n"),
    ("server_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"phase\", ty = \"crate::game::native_lifecycle::ServerTickPhase\", value = \"phase\" },\n"),
    ("player_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n"),
    ("net_from_client", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"channel\", ty = \"&str\", value = \"channel\" },\n    { name = \"data\", ty = \"&[u8]\", value = \"data\" },\n"),
    ("state_for_placement", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"context\", ty = \"&horizon_world::level::PlacementContext\", value = \"context\" },\n    { name = \"answer\", ty = \"&mut Option<Option<horizon_world::registry::BlockStateId>>\", value = \"&mut *answer\" },\n"),
    ("block_entity", "    { name = \"block\", ty = \"&str\", value = \"block\" },\n    { name = \"entity\", ty = \"&mut Option<(String, horizon_world::nbt::Compound)>\", value = \"&mut *entity\" },\n"),
    ("block_entity_ticks", "    { name = \"block\", ty = \"&str\", value = \"block\" },\n    { name = \"ticks\", ty = \"&mut bool\", value = \"&mut *ticks\" },\n"),
    ("block_entity_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&mut horizon_client::containers::ModBlockEntity\", value = \"&mut *entity\" },\n"),
    ("right_click_block", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("use_item_on", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"claimed\", ty = \"&mut bool\", value = \"&mut *claimed\" },\n"),
    ("use_item", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"claimed\", ty = \"&mut bool\", value = \"&mut *claimed\" },\n"),
    ("use_duration", "    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"duration\", ty = \"&mut i32\", value = \"&mut *duration\" },\n"),
    ("living_entity_use_item_start", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("on_use_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"remaining\", ty = \"i32\", value = \"remaining\" },\n"),
    ("living_entity_use_item_finish", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"result\", ty = \"&mut Option<horizon_client::items::ItemStack>\", value = \"&mut *result\" },\n"),
    ("release_using", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"entity\", ty = \"Option<i32>\", value = \"entity\" },\n    { name = \"time_left\", ty = \"i32\", value = \"time_left\" },\n"),
];

fn mixin_toml(id: &str, version: &str) -> String {
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
    s.push_str("[[crate]]\nid = \"engine\"\npath = \"engine\"\n");
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
    let id = mod_id(jar).ok_or_else(|| format!("{}: no NeoForge mod id", jar.display()))?;
    let bundle_id = format!("{id}_translated");
    let krate = format!("{}_translated_engine", id.replace('-', "_"));
    let mut program = Program::new(classes);
    program.mod_id = Some(id.clone());
    program.services = crate::services(jar)?;
    let files = program.emit_files(40);
    let src = out.join("engine/src");
    fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    let deps = match runtime {
        Some(core) => format!("\n[dependencies]\nhorizon-compat-core = {{ path = {:?} }}\n", core.display().to_string()),
        None => format!("\n[dependencies]\n{ENGINE_DEPENDENCIES}"),
    };
    put(
        out.join("engine/Cargo.toml"),
        format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n{deps}",
            krate.replace('_', "-")
        ),
    )
    .map_err(|e| e.to_string())?;
    let root = if runtime.is_some() {
        "/// The runtime core; the API mapping comes through `horizon_compat::install()`.\npub use horizon_compat_core as horizon_compat;\nuse horizon_compat::"
    } else {
        // The runtime as the engine's own `compat` module (bundle crates take no path dependencies).
        let compat_dir = src.join("compat");
        fs::create_dir_all(&compat_dir).map_err(|e| e.to_string())?;
        let core = inlined_core();
        for (name, text) in std::iter::once(("mod.rs", core.as_str())).chain(COMPAT) {
            let text = text.replace("crate::", "crate::compat::").replace("#![", "// #![");
            put(compat_dir.join(name), text).map_err(|e| e.to_string())?;
        }
        "pub mod compat;\n/// The runtime (also for the hook helpers compiled into the game).\npub use compat as horizon_compat;\nuse horizon_compat::"
    };
    for (name, mut text) in files {
        if name == "lib.rs" {
            text = text.replacen("use horizon_compat::", root, 1);
            text.push_str(&format!(
                "\nthread_local! {{\n    static RUNTIME: std::cell::RefCell<Option<Rt>> = const {{ std::cell::RefCell::new(None) }};\n}}\n\n\
                 /// The mod's runtime on this thread, made and registered on first use. Not from inside one of its\n\
                 /// own calls (it is busy then): the game's hooks go through [`hook_runtime`].\n\
                 pub fn with_runtime<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    RUNTIME.with(|cell| {{\n        let mut slot = cell.borrow_mut();\n        let rt = slot.get_or_insert_with(|| {{\n            let mut rt = Rt::new();\n            rt.namespace = {id:?}.into();\n            init(&mut rt);\n            start(&mut rt);\n            rt\n        }});\n        f(rt)\n    }})\n}}\n\n\
                 /// Whether the mod's runtime is in a call on this thread: a hook the game runs from inside one (the\n\
                 /// mod's code places a block, and the game asks for the block's block entity) finds it so.\n\
                 pub fn runtime_busy() -> bool {{\n    RUNTIME.with(|cell| cell.try_borrow_mut().is_err())\n}}\n\n\
                 /// [`with_runtime`] for a hook the game runs. When the runtime is busy on this thread (the game ran\n\
                 /// the hook from inside one of the mod's calls), the hook does nothing (`R::default()`, which\n\
                 /// leaves it to the game) instead of entering the runtime again, and says so once.\n\
                 pub fn hook_runtime<R: Default>(hook: &'static str, f: impl FnOnce(&mut Rt) -> R) -> R {{\n    if runtime_busy() {{\n        thread_local! {{\n            static TOLD: std::cell::RefCell<Vec<&'static str>> = const {{ std::cell::RefCell::new(Vec::new()) }};\n        }}\n        if TOLD.with(|t| !t.borrow().contains(&hook) && {{ t.borrow_mut().push(hook); true }}) {{\n            eprintln!(\"{id}: the game ran the {{hook}} hook from inside one of the mod's own calls; skipped\");\n        }}\n        return R::default();\n    }}\n    with_runtime(f)\n}}\n\n\
                 /// A question about the mod's classes and registrations alone (a block's block entity type, an\n\
                 /// item's use duration, a block's state for a placement): the runtime answers it, or the probe\n\
                 /// runtime while the runtime is busy (a block the mod's own code places asks from inside its call).\n\
                 pub fn with_registrations<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    if runtime_busy() {{ with_probe_runtime(f) }} else {{ with_runtime(f) }}\n}}\n\n\
                 thread_local! {{\n    static PROBE: std::cell::RefCell<Option<Rt>> = const {{ std::cell::RefCell::new(None) }};\n}}\n\n\
                 /// A runtime of its own on this thread, for questions asked before a game runs (`native_content_block`):\n\
                 /// it may fire the client's setup without changing the side [`with_runtime`]'s plays.\n\
                 pub fn with_probe_runtime<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    PROBE.with(|cell| {{\n        let mut slot = cell.borrow_mut();\n        let rt = slot.get_or_insert_with(|| {{\n            let mut rt = Rt::new();\n            rt.namespace = {id:?}.into();\n            init(&mut rt);\n            start(&mut rt);\n            rt\n        }});\n        f(rt)\n    }})\n}}\n"
            ));
        }
        put(src.join(&name), text).map_err(|e| e.to_string())?;
    }
    put(out.join("horizon-mixin.toml"), mixin_toml(&bundle_id, "0.1.0")).map_err(|e| e.to_string())?;
    for (hook, text) in block_entity_helpers(&bundle_id, &id, &krate).into_iter().chain(client_helpers(&bundle_id, &id, &krate)) {
        put(out.join(format!("{hook}.rs")), text).map_err(|e| e.to_string())?;
    }
    for (hook, glue) in BLOCK_HOOKS {
        put(out.join(format!("{hook}.rs")), helper(&bundle_id, &krate, hook, glue).replace(&format!("\"{bundle_id}:\""), &format!("\"{id}:\""))).map_err(|e| e.to_string())?;
    }
    // The blocks the code registers, as a content mod (a folder mod for `horizon-mods/`): the game
    // knows them, and their random ticks reach the translated `randomTick`.
    let content = out.join("content");
    let blocks: Vec<String> = crate::registrations(classes)
        .into_iter()
        .filter(|r| r.registry == "block")
        .map(|r| r.name.rsplit(':').next().unwrap_or(&r.name).to_owned())
        .collect();
    if !blocks.is_empty() {
        let dir = content.join("data").join(&id).join("horizon/block");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let props = crate::props::block_properties(classes);
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
        // The mod's own look: its assets (blockstates, models, textures, lang) from the jar.
        if let Ok(f) = fs::File::open(jar)
            && let Ok(mut zip) = zip::ZipArchive::new(f)
        {
            let prefix = format!("assets/{id}/");
            let mut item_models = Vec::new();
            for i in 0..zip.len() {
                let Ok(mut e) = zip.by_index(i) else { continue };
                let Some(name) = e.enclosed_name() else { continue };
                let path = name.to_string_lossy().into_owned();
                let resource = [".json", ".png", ".mcmeta", ".ogg", ".obj", ".mtl"].iter().any(|x| path.ends_with(x));
                if !e.is_file() || !path.starts_with(&prefix) || !resource {
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
            // A jar for 1.21.1 has no client item definitions (1.21.4 put item models behind
            // `items/<name>.json`): without one an item draws no icon. Each item model gets the
            // plain definition, as the port-pack's carry-over writes; the jar's own win.
            for m in item_models {
                let def = content.join(format!("assets/{id}/items/{m}.json"));
                if !def.exists() {
                    if let Some(parent) = def.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    let json = format!("{{\"model\": {{\"type\": \"minecraft:model\", \"model\": \"{id}:item/{m}\"}}}}\n");
                    put(def, json).map_err(|e| e.to_string())?;
                }
            }
        }
        put(
            content.join("horizon-mod.toml"),
            format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\napi = \"0.2\"\ncapabilities = [\"registries\"]\n"),
        )
        .map_err(|e| e.to_string())?;
    }
    let left: Vec<String> = program.unsupported.iter().map(|(k, n)| format!("{k} ({n})")).collect();
    Ok((id, left))
}

#[cfg(test)]
mod tests {
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
        // The inlined core declares every carried mapping module and drops the installable entry points.
        let core = super::inlined_core();
        assert!(!core.contains("pub fn install_fns"));
        for n in carried.iter().filter(|n| *n != "host.rs") {
            assert!(core.contains(&format!("pub mod {};", n.trim_end_matches(".rs"))), "{n}");
        }
    }
}
