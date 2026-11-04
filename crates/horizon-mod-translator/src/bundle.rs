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
const COMPAT: [(&str, &str); 26] = [
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
    ("capabilities.rs", include_str!("../../horizon-compat/src/capabilities.rs")),
    ("entities.rs", include_str!("../../horizon-compat/src/entities.rs")),
    ("tanks.rs", include_str!("../../horizon-compat/src/tanks.rs")),
    ("creative.rs", include_str!("../../horizon-compat/src/creative.rs")),
    ("fluids.rs", include_str!("../../horizon-compat/src/fluids.rs")),
    ("functions.rs", include_str!("../../horizon-compat/src/functions.rs")),
    ("shapes.rs", include_str!("../../horizon-compat/src/shapes.rs")),
    ("nbt.rs", include_str!("../../horizon-compat/src/nbt.rs")),
    ("glue.rs", include_str!("../../horizon-compat/src/glue.rs")),
    ("jutil.rs", include_str!("../../horizon-compat/src/jutil.rs")),
    ("neo.rs", include_str!("../../horizon-compat/src/neo.rs")),
    ("network.rs", include_str!("../../horizon-compat/src/network.rs")),
    ("jlang.rs", include_str!("../../horizon-compat/src/jlang.rs")),
    ("tags.rs", include_str!("../../horizon-compat/src/tags.rs")),
    ("render.rs", include_str!("../../horizon-compat/src/render.rs")),
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
    // `Rt::is_a` asks the mapping's table of Minecraft's class families.
    format!("{head}{tail}\n{mods}pub use api_common::api_is_a;\n")
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
        fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {{
            self.level.native_fluid_name_in_tag(fluid, tag)
        }}
        fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {{
            self.level.native_set_state_text(self.ticks, pos, state, 3)
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
    {krate}::with_runtime(|rt| {{
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

/// The window's host (the client tick, payloads from the server): the client's world, and payloads
/// to the server.
fn client_host(c: &str) -> String {
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
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
        fn send_payload(&mut self, to: {c}::host::PayloadTarget, channel: &str, data: &[u8]) -> bool {{
            to == {c}::host::PayloadTarget::Server && self.game.native_net_send_to_server(channel, data).is_ok()
        }}
    }}
"#)
}

/// The client's hooks: drawing the mod's block entities with its renderers, the update tags the
/// server sends for them, the client tick (animation clocks) and the payloads the server sends.
fn client_helpers(id: &str, ns: &str, krate: &str) -> Vec<(&'static str, String)> {
    let c = format!("{krate}::compat");
    let conversions = nbt_conversions(&c);
    let client_host = client_host(&c);
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
    struct Host<'a, 'b, 'c> {{
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
            }};
            self.draw.pop();
            self.draw.light = saved;
            drew
        }}
    }}
    let state = game.native_state_text_at(pos);
    let nbt: BTreeMap<String, Nbt> = entity.nbt.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    // Horizon's light (sky * 16 | block * 16 << 8) as LightTexture.pack.
    let light = ((draw.light >> 8 & 0xFF) | (draw.light & 0xFF) << 16) as i32;
    let mut host = Host {{ game, draw }};
    {krate}::with_runtime(|rt| {c}::glue::render_block_entity(rt, &mut host, pos, &state, &nbt, light));
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
    let live = {krate}::with_runtime(|rt| {c}::glue::block_entity_update_tag(rt, pos));
    *tag = Some(match live {{
        Some(nbt) => nbt.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), to_tag(v)?))).collect::<Compound>(),
        None => entity.nbt.clone(),
    }});
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
{client_host}    let mut host = Host {{ game }};
    {krate}::with_runtime(|rt| {c}::glue::client_tick(rt, &mut host));
}}
"#)),
        ("net_from_server", format!(r#"// Generated by horizon-mod-translator: a mod payload the server sent (`native_net_from_server`), to
// the translated mod's handler for its type.
fn {id}_net_from_server(game: &mut crate::game::Game<'_>, channel: &str, data: &[u8]) {{
{client_host}
    let mut host = Host {{ game }};
    {krate}::with_runtime(|rt| {c}::glue::payload_from_server(rt, &mut host, channel, data));
}}
"#)),
    ]
}

/// The server's hooks (on `Game`): the block entities (creating, whether they tick, each tick), the
/// server's tick, and the payloads clients send.
fn block_entity_helpers(id: &str, ns: &str, krate: &str) -> Vec<(&'static str, String)> {
    let c = format!("{krate}::compat");
    let server_send = server_send(&c);
    vec![
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
    {krate}::with_runtime(|rt| {c}::glue::server_tick(rt, &mut host, post));
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
{server_send}    }}
    let mut host = Host {{ game }};
    {krate}::with_runtime(|rt| {c}::glue::payload_from_client(rt, &mut host, player, channel, data));
}}
"#)),
        ("block_entity", format!(r#"// Generated by horizon-mod-translator: `EntityBlock.newBlockEntity` of the translated mod.
fn {id}_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {{
    if entity.is_some() || !block.starts_with("{ns}:") {{
        return;
    }}
    if let Some(be) = {krate}::with_runtime(|rt| {c}::glue::block_entity_id(rt, block)) {{
        *entity = Some((be, horizon_world::nbt::Compound::new()));
    }}
}}
"#)),
        ("block_entity_ticks", format!(r#"// Generated by horizon-mod-translator: `EntityBlock.getTicker` of the translated mod.
fn {id}_block_entity_ticks(block: &str, ticks: &mut bool) {{
    if block.starts_with("{ns}:") && {krate}::with_runtime(|rt| {c}::glue::block_entity_ticks(rt, block)) {{
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
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.native_block_name_in_tag(block, tag)
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
{server_send}    }}
    let state = game.native_state_text_at(pos);
    let mut nbt: BTreeMap<String, Nbt> = entity.nbt.iter().filter_map(|(k, v)| Some((k.to_string_lossy(), to_nbt(v)?))).collect();
    let mut host = Host {{ game }};
    if {krate}::with_runtime(|rt| {c}::glue::block_entity_tick(rt, &mut host, pos, &state, &mut nbt)) {{
        // What the block entity saved, all of it: keys it dropped are gone.
        entity.nbt = nbt.iter().filter_map(|(k, v)| Some((JavaString::from(k.as_str()), to_tag(v)?))).collect::<Compound>();
    }}
}}
"#)),
        ("right_click_block", format!(r#"// Generated by horizon-mod-translator: a right click on the translated mod's block
// (`useItemOn`, then `useWithoutItem`), with the main hand. A click the block takes is canceled
// for the game, which would otherwise use the held item on it.
fn {id}_right_click_block(game: &mut crate::game::Game<'_>, pos: [i32; 3], face: [i32; 3], canceled: &mut bool) {{
    if *canceled || game.hand != horizon_client::first_person::Hand::Main {{
        return;
    }}
    let state = game.native_state_text_at(pos);
    if !state.starts_with("{ns}:") {{
        return;
    }}
    // Direction's 3D data value: down, up, north, south, west, east.
    let face = match face {{
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        _ => 5,
    }};
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.native_block_name_in_tag(block, tag)
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
        fn recipes_of_type(&self, kind: &str) -> Vec<(String, String)> {{
            self.game.native_recipes_of_type(kind).iter().map(|r| (r.id.clone(), r.json.to_string())).collect()
        }}
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
    }}
    let mut host = Host {{ game }};
    if {krate}::with_runtime(|rt| {c}::glue::use_block(rt, &mut host, pos, &state, face)) {{
        *canceled = true;
    }}
}}
"#)),
        ("use_item_on", format!(r#"// Generated by horizon-mod-translator: a right click on a block holding one of the translated
// mod's items, once the block let it through: the item's own `useOn` (Create's wrench turns
// blocks). An item that takes the click claims it, so the game's own use of the item doesn't run.
fn {id}_use_item_on(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, pos: [i32; 3], face: [i32; 3], claimed: &mut bool) {{
    if *claimed || game.hand != horizon_client::first_person::Hand::Main {{
        return;
    }}
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
    struct Host<'a, 'b> {{
        game: &'a mut crate::game::Game<'b>,
    }}
    impl {c}::host::Host for Host<'_, '_> {{
        fn block_state(&self, pos: [i32; 3]) -> String {{
            self.game.native_state_text_at(pos)
        }}
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {{
            self.game.native_block_name_in_tag(block, tag)
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
        fn recipes_of_type(&self, kind: &str) -> Vec<(String, String)> {{
            self.game.native_recipes_of_type(kind).iter().map(|r| (r.id.clone(), r.json.to_string())).collect()
        }}
        fn ingredient_items(&self, entry: &str) -> Vec<String> {{
            self.game.native_recipe_ingredient(&serde_json::Value::String(entry.to_owned())).map(|i| i.1).unwrap_or_default()
        }}
    }}
    // The click lands on the clicked face's centre.
    let at = |i: usize| pos[i] as f64 + 0.5 + face[i] as f64 * 0.5;
    let click = {c}::glue::Placement {{
        pos,
        hit_pos: pos,
        face: face_id,
        hit: [at(0), at(1), at(2)],
        item,
        replace_clicked: true,
        ..Default::default()
    }};
    let mut host = Host {{ game }};
    if {krate}::with_runtime(|rt| {c}::glue::use_item_on(rt, &mut host, &click)) {{
        *claimed = true;
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
    }};
    let mut host = Host {{ level, sneaking: context.sneaking, item: context.item.clone() }};
    match {krate}::with_runtime(|rt| {c}::glue::state_for_placement(rt, &mut host, &text, &placement)) {{
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

const CLIENT_BINDINGS: [(&str, &str); 6] = [
    ("content", "    { name = \"content\", ty = \"&mut horizon_mods::Content\", value = \"&mut *content\" },\n"),
    ("content_block", "    { name = \"block\", ty = \"&mut horizon_mods::content::Block\", value = \"&mut *block\" },\n"),
    ("block_entity_submit", "    { name = \"game\", ty = \"&crate::game::Game<'_>\", value = \"self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"draw\", ty = \"&mut crate::game::native_block_entity_draw::BlockEntityDraw\", value = \"&mut *draw\" },\n"),
    ("block_entity_update_tag", "    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&horizon_client::containers::ModBlockEntity\", value = \"entity\" },\n    { name = \"tag\", ty = \"&mut Option<horizon_world::nbt::Compound>\", value = \"&mut *tag\" },\n"),
    ("tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n"),
    ("net_from_server", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"channel\", ty = \"&str\", value = \"channel\" },\n    { name = \"data\", ty = \"&[u8]\", value = \"data\" },\n"),
];

const BLOCK_ENTITY_BINDINGS: [(&str, &str); 8] = [
    ("server_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"phase\", ty = \"crate::game::native_lifecycle::ServerTickPhase\", value = \"phase\" },\n"),
    ("net_from_client", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"player\", ty = \"u128\", value = \"player\" },\n    { name = \"channel\", ty = \"&str\", value = \"channel\" },\n    { name = \"data\", ty = \"&[u8]\", value = \"data\" },\n"),
    ("state_for_placement", "    { name = \"level\", ty = \"&crate::game::GameLevel<'_>\", value = \"self\" },\n    { name = \"state\", ty = \"horizon_world::registry::BlockStateId\", value = \"state\" },\n    { name = \"context\", ty = \"&horizon_world::level::PlacementContext\", value = \"context\" },\n    { name = \"answer\", ty = \"&mut Option<Option<horizon_world::registry::BlockStateId>>\", value = \"&mut *answer\" },\n"),
    ("block_entity", "    { name = \"block\", ty = \"&str\", value = \"block\" },\n    { name = \"entity\", ty = \"&mut Option<(String, horizon_world::nbt::Compound)>\", value = \"&mut *entity\" },\n"),
    ("block_entity_ticks", "    { name = \"block\", ty = \"&str\", value = \"block\" },\n    { name = \"ticks\", ty = \"&mut bool\", value = \"&mut *ticks\" },\n"),
    ("block_entity_tick", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"entity\", ty = \"&mut horizon_client::containers::ModBlockEntity\", value = \"&mut *entity\" },\n"),
    ("right_click_block", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"canceled\", ty = \"&mut bool\", value = \"&mut *canceled\" },\n"),
    ("use_item_on", "    { name = \"game\", ty = \"&mut crate::game::Game<'_>\", value = \"&mut *self\" },\n    { name = \"stack\", ty = \"&horizon_client::items::ItemStack\", value = \"stack\" },\n    { name = \"pos\", ty = \"[i32; 3]\", value = \"pos\" },\n    { name = \"face\", ty = \"[i32; 3]\", value = \"face\" },\n    { name = \"claimed\", ty = \"&mut bool\", value = \"&mut *claimed\" },\n"),
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
                 /// The mod's runtime on this thread, made and registered on first use.\n\
                 pub fn with_runtime<R>(f: impl FnOnce(&mut Rt) -> R) -> R {{\n    RUNTIME.with(|cell| {{\n        let mut slot = cell.borrow_mut();\n        let rt = slot.get_or_insert_with(|| {{\n            let mut rt = Rt::new();\n            rt.namespace = {id:?}.into();\n            init(&mut rt);\n            start(&mut rt);\n            rt\n        }});\n        f(rt)\n    }})\n}}\n\n\
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
                let resource = [".json", ".png", ".mcmeta", ".ogg"].iter().any(|x| path.ends_with(x));
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
