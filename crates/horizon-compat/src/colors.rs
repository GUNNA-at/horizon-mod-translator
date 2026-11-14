//! Block and item colours: the handlers a mod's client registers to tint its models
//! (`RegisterColorHandlersEvent`, `BlockColors`/`ItemColors.register`), what they answer
//! (`BlockColors.getColor`, `ItemColors.getColor`; vanilla's blocks as `BlockColors.createDefault`
//! registers them), and the colour tables they read: a biome's grass, foliage and water colours
//! (`BiomeColors`, `getBlockTint` with a `ColorResolver`), `GrassColor`, `FoliageColor` and
//! `RedStoneWireBlock.getColorForPower`.
//!
//! A chunk's mesh is built without the mod's runtime, so the game draws a mod block by what
//! [`block_tints`] finds of its handler: each tint index as the biome's grass, foliage or water
//! colour (which the game blends across biomes as it does vanilla's) or as a constant.
//!
//! Elsewhere (a block entity's renderer asks) a biome colour is the biome's at the position: its
//! data's own colour, else the colour map's at its climate, grass through its modifier. Vanilla's
//! blend across neighbouring biomes is not made, swamp grass is its lighter shade, and where the
//! host doesn't know the biome it is plains'.
use crate::{Rt, V, api, host::Host};

const BLOCK_EVENT: &str = "net/neoforged/neoforge/client/event/RegisterColorHandlersEvent$Block";
const ITEM_EVENT: &str = "net/neoforged/neoforge/client/event/RegisterColorHandlersEvent$Item";
const BLOCK_COLORS: &str = "net/minecraft/client/color/block/BlockColors";
const ITEM_COLORS: &str = "net/minecraft/client/color/item/ItemColors";
const BIOME_COLORS: &str = "net/minecraft/client/renderer/BiomeColors";
const GRASS_COLOR: &str = "net/minecraft/world/level/GrassColor";
const FOLIAGE_COLOR: &str = "net/minecraft/world/level/FoliageColor";
const RESOLVER: &str = "net/minecraft/world/level/ColorResolver";
const REDSTONE_WIRE: &str = "net/minecraft/world/level/block/RedStoneWireBlock";
const MINECRAFT: &str = "net/minecraft/client/Minecraft";
const BIOME: &str = "net/minecraft/world/level/biome/Biome";
const CLIMATE: &str = "net/minecraft/world/level/biome/Biome$ClimateSettings";
const GRASS_MODIFIER: &str =
    "net/minecraft/world/level/biome/BiomeSpecialEffects$GrassColorModifier";
/// 1.21.1's own fog and water fog where a biome's file names none (the overworld's).
const FOG: i32 = 0xC0_D8FF;
const WATER_FOG: i32 = 0x05_0533;
const BLOCK_COLOR: &str = "net/minecraft/client/color/block/BlockColor.getColor.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;I)I";
const ITEM_COLOR: &str =
    "net/minecraft/client/color/item/ItemColor.getColor.(Lnet/minecraft/world/item/ItemStack;I)I";
const RESOLVER_COLOR: &str =
    "net/minecraft/world/level/ColorResolver.getColor.(Lnet/minecraft/world/level/biome/Biome;DD)I";
/// `getBlockTint(pos, resolver)`, on whichever getter type the mod's code names.
const BLOCK_TINT: &str =
    "(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/ColorResolver;)I";

/// `FoliageColor`'s constants (1.21.1): opaque.
const FOLIAGE_EVERGREEN: i32 = -10_380_959;
const FOLIAGE_BIRCH: i32 = -8_345_771;
const FOLIAGE_DEFAULT: i32 = -12_012_264;
const FOLIAGE_MANGROVE: i32 = -7_158_200;
/// What `GrassColor.get` answers past its map.
const GRASS_OFF_MAP: i32 = -65_281;

/// While [`block_tints`] runs a handler: which run (1 and 2 answer the markers, 3 plains' colours).
const PROBE: &str = "compat:tint-probe";
/// The biome colours (grass, foliage, water) the probe's first two runs answer: a handler that
/// hands one on is that biome colour; two sets tell it apart from a constant that equals one.
const MARKERS: [[i32; 3]; 2] = [
    [0x01_0203, 0x04_0506, 0x07_0809],
    [0x0A_0B0C, 0x0D_0E0F, 0x10_1112],
];
/// Plains' colours (temperature 0.8, downfall 0.4): grass, foliage, water. The colour of a biome
/// the host doesn't know, and of the probe's last run.
const PLAINS: [i32; 3] = [0x91_BD59, 0x77_AB2F, 0x3F_76E4];

/// A biome colour, as `BiomeColors`' three resolvers read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BiomeColor {
    Grass,
    Foliage,
    Water,
}

const BIOME_COLORS_ALL: [BiomeColor; 3] =
    [BiomeColor::Grass, BiomeColor::Foliage, BiomeColor::Water];

impl BiomeColor {
    fn index(self) -> usize {
        self as usize
    }
    /// `BiomeColors`' static resolver of it.
    fn resolver_field(self) -> &'static str {
        match self {
            BiomeColor::Grass => "GRASS_COLOR_RESOLVER",
            BiomeColor::Foliage => "FOLIAGE_COLOR_RESOLVER",
            BiomeColor::Water => "WATER_COLOR_RESOLVER",
        }
    }
    fn of_average(name: &str) -> Option<BiomeColor> {
        Some(match name {
            "getAverageGrassColor" => BiomeColor::Grass,
            "getAverageFoliageColor" => BiomeColor::Foliage,
            "getAverageWaterColor" => BiomeColor::Water,
            _ => return None,
        })
    }
}

/// What the game tints one tint index of a mod block's model with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// Nothing (white).
    None,
    /// The biome's grass colour at the block.
    Grass,
    /// The biome's foliage colour.
    Foliage,
    /// The biome's water colour.
    Water,
    /// One colour, `0xRRGGBB`.
    Constant(u32),
}

/// `BlockColors`, the client's one (`Minecraft.getBlockColors()`, the event's).
pub(crate) fn block_colors(rt: &mut Rt) -> V {
    rt.intern("compat:colors/block-colors".into(), |rt| {
        rt.new_object(BLOCK_COLORS)
    })
}

fn item_colors(rt: &mut Rt) -> V {
    rt.intern("compat:colors/item-colors".into(), |rt| {
        rt.new_object(ITEM_COLORS)
    })
}

/// `BiomeColors`' resolver of `color`: a resolver object that names it.
fn resolver(rt: &mut Rt, color: BiomeColor) -> V {
    rt.intern(format!("compat:colors/resolver/{}", color.index()), |rt| {
        let r = rt.new_object(RESOLVER);
        rt.put_field(r, "$biomeColor", V::I(color.index() as i32 + 1));
        r
    })
}

fn resolver_color(rt: &Rt, r: V) -> Option<BiomeColor> {
    match rt.get_field(r, "$biomeColor") {
        V::I(i @ 1..=3) => Some(BIOME_COLORS_ALL[i as usize - 1]),
        _ => None,
    }
}

/// The handler registered for `id` (a block's or an item's name) in `registry`.
fn handler(rt: &Rt, registry: &str, id: &str) -> Option<V> {
    rt.interned
        .get(&format!("compat:colors/{registry}/{id}"))
        .copied()
        .filter(|h| h.r() != 0)
}

/// `register(handler, targets...)`: `targets` are blocks or item-likes; the last registration of
/// one keeps it, as vanilla's map does.
fn register(rt: &mut Rt, registry: &'static str, handler: V, targets: V) {
    if handler.r() == 0 {
        return;
    }
    for target in crate::jutil::elements(rt, targets) {
        let target = if registry == "item" {
            crate::creative::as_item(rt, target)
        } else {
            target
        };
        match rt.registered_name(target, registry).map(str::to_owned) {
            Some(id) => {
                rt.interned
                    .insert(format!("compat:colors/{registry}/{id}"), handler);
            }
            None => crate::glue::trace_some(rt, "colors-unregistered", registry, 4, |_| {
                format!("a colour handler for an unregistered {registry}")
            }),
        }
    }
}

/// The mods' colour handlers, registered as the client is made (`BlockColors.createDefault` and
/// `ItemColors.createDefault` post the events).
pub(crate) fn init(rt: &mut Rt) {
    let colors = block_colors(rt);
    let ev = rt.new_object(BLOCK_EVENT);
    rt.put_field(ev, "$blockColors", colors);
    crate::neo::post(rt, BLOCK_EVENT, ev);
    let items = item_colors(rt);
    let ev = rt.new_object(ITEM_EVENT);
    rt.put_field(ev, "$itemColors", items);
    rt.put_field(ev, "$blockColors", colors);
    crate::neo::post(rt, ITEM_EVENT, ev);
}

/// Runs a block colour handler; a handler that throws tints nothing.
fn run_block_handler(rt: &mut Rt, handler: V, state: V, getter: V, pos: V, index: i32) -> i32 {
    let c = rt.call_functional(BLOCK_COLOR, handler, &[state, getter, pos, V::I(index)]);
    if rt.thrown.is_some() {
        rt.report_thrown("a block colour handler");
        return -1;
    }
    c.i()
}

/// `BlockColors.getColor(state, level, pos, tintIndex)`: the handler registered for the state's
/// block, else vanilla's for its own blocks, else none (-1).
fn block_color(rt: &mut Rt, state: V, getter: V, pos: V, index: i32) -> i32 {
    let text = rt.str_of(state).unwrap_or("").to_owned();
    let id = text.split('[').next().unwrap_or("");
    if let Some(h) = handler(rt, "block", id) {
        return run_block_handler(rt, h, state, getter, pos, index);
    }
    match id.strip_prefix("minecraft:") {
        Some(name) => vanilla_block_color(rt, name, &text, getter, pos, index).unwrap_or(-1),
        None => -1,
    }
}

/// A property's value in a state's text (`half` of `minecraft:tall_grass[half=upper]`).
fn property<'a>(state: &'a str, name: &str) -> Option<&'a str> {
    let props = state.split_once('[')?.1.trim_end_matches(']');
    props.split(',').find_map(|kv| {
        kv.split_once('=')
            .filter(|(k, _)| *k == name)
            .map(|(_, v)| v)
    })
}

/// What `BlockColors.createDefault` (1.21.1) registers for vanilla block `name`, answered for
/// `state` at `pos` of `getter` (null when the caller has no level) and tint index `index`.
fn vanilla_block_color(
    rt: &mut Rt,
    name: &str,
    state: &str,
    getter: V,
    pos: V,
    index: i32,
) -> Option<i32> {
    let world = getter.r() != 0 && pos.r() != 0;
    Some(match name {
        "large_fern" | "tall_grass" if world => {
            let p = api::pos_of(rt, pos);
            let upper = property(state, "half") == Some("upper");
            let at = if upper {
                api::block_pos(rt, p[0] as i64, p[1] as i64 - 1, p[2] as i64)
            } else {
                pos
            };
            average(rt, getter, at, BiomeColor::Grass)
        }
        "grass_block" | "fern" | "short_grass" | "potted_fern" if world => {
            average(rt, getter, pos, BiomeColor::Grass)
        }
        "large_fern" | "tall_grass" | "grass_block" | "fern" | "short_grass" | "potted_fern" => {
            grass_default(rt)
        }
        "pink_petals" if index == 0 => -1,
        "pink_petals" if world => average(rt, getter, pos, BiomeColor::Grass),
        "pink_petals" => grass_default(rt),
        "spruce_leaves" => FOLIAGE_EVERGREEN,
        "birch_leaves" => FOLIAGE_BIRCH,
        "oak_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves" | "vine"
        | "mangrove_leaves"
            if world =>
        {
            average(rt, getter, pos, BiomeColor::Foliage)
        }
        "oak_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves" | "vine"
        | "mangrove_leaves" => FOLIAGE_DEFAULT,
        "water" | "bubble_column" | "water_cauldron" if world => {
            average(rt, getter, pos, BiomeColor::Water)
        }
        "water" | "bubble_column" | "water_cauldron" => -1,
        "redstone_wire" => redstone_color(
            property(state, "power")
                .and_then(|p| p.parse().ok())
                .unwrap_or(0),
        ),
        "sugar_cane" if world => average(rt, getter, pos, BiomeColor::Grass),
        "sugar_cane" => -1,
        "attached_melon_stem" | "attached_pumpkin_stem" => -2_046_180,
        "melon_stem" | "pumpkin_stem" => {
            let age: i32 = property(state, "age")
                .and_then(|a| a.parse().ok())
                .unwrap_or(0);
            // `FastColor.ARGB32.color(r, g, b)`: opaque.
            (0xFF00_0000_u32 as i32) | (age * 32) << 16 | (255 - age * 8) << 8 | age * 4
        }
        "lily_pad" if world => -14_647_248,
        "lily_pad" => -9_321_636,
        _ => return None,
    })
}

/// `RedStoneWireBlock.getColorForPower(power)`: `Mth.color` of `COLORS[power]`, in floats as vanilla
/// computes them; no alpha.
pub fn redstone_color(power: i32) -> i32 {
    let f = power.clamp(0, 15) as f32 / 15.0;
    let r = f * 0.6 + if f > 0.0 { 0.4 } else { 0.3 };
    let g = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
    let b = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
    let byte = |v: f32| (v * 255.0).floor() as i32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// `BiomeColors.getAverage*Color(getter, pos)`, `getter.getBlockTint(pos, resolver)` with
/// `BiomeColors`' resolver: the biome's colour at `pos` (markers while probing).
fn average(rt: &mut Rt, _getter: V, pos: V, color: BiomeColor) -> i32 {
    match rt.interned.get(PROBE).map(|v| v.i()) {
        Some(run @ 1..=2) => return MARKERS[run as usize - 1][color.index()] | OPAQUE,
        Some(_) => return plains(color),
        None => {}
    }
    let p = api::pos_of(rt, pos);
    let biome = rt.host().and_then(|h| h.biome(p));
    match biome {
        Some(id) => biome_color(rt, &id, color),
        None => plains(color),
    }
}

const OPAQUE: i32 = 0xFF00_0000_u32 as i32;

/// Plains' colour of `color`, as its biome answers it: grass and foliage from the colour map
/// (opaque), water from its data.
fn plains(color: BiomeColor) -> i32 {
    match color {
        BiomeColor::Water => PLAINS[2],
        c => PLAINS[c.index()] | OPAQUE,
    }
}

/// A colour in a biome's data: a number (1.21.1's files) or `#rrggbb` (26.3's).
fn data_color(v: &serde_json::Value) -> Option<i32> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().map(|n| n as i32 & 0xFF_FFFF),
        serde_json::Value::String(s) => i32::from_str_radix(s.trim_start_matches('#'), 16).ok(),
        _ => None,
    }
}

/// Biome `id`'s colour of `color` (`Biome.getGrassColor(x, z)`, `getFoliageColor()`,
/// `getWaterColor()`), from its data; plains' where the host has none.
fn biome_color(rt: &mut Rt, id: &str, color: BiomeColor) -> i32 {
    let json: serde_json::Value = match rt
        .host()
        .and_then(|h| h.biome_json(id))
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(j) => j,
        None => return plains(color),
    };
    let effects = &json["effects"];
    let climate = |key: &str| json[key].as_f64().unwrap_or(0.5).clamp(0.0, 1.0);
    let (temperature, downfall) = (climate("temperature"), climate("downfall"));
    match color {
        BiomeColor::Water => data_color(&effects["water_color"]).unwrap_or(PLAINS[2]),
        BiomeColor::Foliage => data_color(&effects["foliage_color"])
            .unwrap_or_else(|| foliage_at(rt, temperature, downfall)),
        BiomeColor::Grass => {
            let c = data_color(&effects["grass_color"])
                .unwrap_or_else(|| grass_at(rt, temperature, downfall));
            match effects["grass_color_modifier"].as_str() {
                Some("dark_forest") => ((c & 0xFE_FEFE) + 0x28_340A) >> 1,
                // Its lighter shade: vanilla's noise picks it or a darker one by position.
                Some("swamp") => 0x6A_7039,
                _ => c,
            }
        }
    }
}

thread_local! {
    /// The colour maps read (`grass`, `foliage`): opaque ARGB pixels.
    static MAPS: std::cell::RefCell<crate::HashMap<&'static str, std::rc::Rc<[i32]>>> = std::cell::RefCell::new(crate::HashMap::default());
}

/// Colour map `name`'s pixels (`textures/colormap/grass.png`), as `LegacyStuffWrapper.getPixels`
/// reads them; None where the resources lack it.
fn colormap(rt: &Rt, name: &'static str) -> Option<std::rc::Rc<[i32]>> {
    if let Some(m) = MAPS.with(|m| m.borrow().get(name).cloned()) {
        return Some(m);
    }
    let bytes = crate::resources::file(
        rt,
        &format!("assets/minecraft/textures/colormap/{name}.png"),
    )?;
    let (_, rgba) = crate::resources::decode_png(&bytes)?;
    let pixels: std::rc::Rc<[i32]> = rgba
        .chunks_exact(4)
        .map(|p| (p[3] as i32) << 24 | (p[0] as i32) << 16 | (p[1] as i32) << 8 | p[2] as i32)
        .collect();
    MAPS.with(|m| m.borrow_mut().insert(name, pixels.clone()));
    Some(pixels)
}

/// `GrassColor.get` and `FoliageColor.get`'s place in the map, for a climate.
fn map_index(temperature: f64, downfall: f64) -> i64 {
    let downfall = downfall * temperature;
    let i = ((1.0 - temperature) * 255.0) as i32;
    let j = ((1.0 - downfall) * 255.0) as i32;
    (j << 8 | i) as i64
}

fn map_at(rt: &Rt, name: &'static str, temperature: f64, downfall: f64) -> Option<Option<i32>> {
    let map = colormap(rt, name)?;
    let k = map_index(temperature, downfall);
    Some(usize::try_from(k).ok().and_then(|k| map.get(k).copied()))
}

/// `GrassColor.get(temperature, downfall)`; plains' grass where the map can't be read.
fn grass_at(rt: &Rt, temperature: f64, downfall: f64) -> i32 {
    match map_at(rt, "grass", temperature, downfall) {
        Some(c) => c.unwrap_or(GRASS_OFF_MAP),
        None => plains(BiomeColor::Grass),
    }
}

/// `FoliageColor.get(temperature, downfall)`; plains' foliage where the map can't be read.
fn foliage_at(rt: &Rt, temperature: f64, downfall: f64) -> i32 {
    match map_at(rt, "foliage", temperature, downfall) {
        Some(c) => c.unwrap_or(FOLIAGE_DEFAULT),
        None => plains(BiomeColor::Foliage),
    }
}

/// `GrassColor.getDefaultColor()`: the map at 0.5, 1.0.
fn grass_default(rt: &Rt) -> i32 {
    grass_at(rt, 0.5, 1.0)
}

/// `getBlockTint(pos, resolver)`: `BiomeColors`' resolvers read the biome's colour; a mod's own
/// resolver runs on the biome at `pos`.
fn block_tint(rt: &mut Rt, getter: V, pos: V, r: V) -> i32 {
    if let Some(color) = resolver_color(rt, r) {
        return average(rt, getter, pos, color);
    }
    if r.r() == 0 {
        return -1;
    }
    let p = api::pos_of(rt, pos);
    let id = rt
        .host()
        .and_then(|h| h.biome(p))
        .unwrap_or_else(|| "minecraft:plains".to_owned());
    let biome = crate::worldgen::biome_value(rt, &id);
    let c = rt.call_functional(
        RESOLVER_COLOR,
        r,
        &[biome, V::D(p[0] as f64), V::D(p[2] as f64)],
    );
    if rt.thrown.is_some() {
        rt.report_thrown("a colour resolver");
        return -1;
    }
    c.i()
}

/// A biome object's colours (`Biome.getGrassColor(x, z)`...), as the resolvers read them: its own,
/// by its id; markers while probing.
fn biome_object_color(rt: &mut Rt, biome: V, color: BiomeColor) -> i32 {
    match rt.interned.get(PROBE).map(|v| v.i()) {
        Some(run @ 1..=2) => return MARKERS[run as usize - 1][color.index()] | OPAQUE,
        Some(_) => return plains(color),
        None => {}
    }
    match rt.str_of(rt.get_field(biome, "$id")).map(str::to_owned) {
        Some(id) => biome_color(rt, &id, color),
        None => plains(color),
    }
}

/// `BiomeSpecialEffects`' constructor, as the records' table makes one ([`crate::records`]): fog,
/// water, water fog, sky; what it sets its foliage and its grass to; its grass's modifier; and
/// five things of its particles, sounds and music, which are the game's own here.
const EFFECTS_INIT: &str = "(IIIILjava/util/Optional;Ljava/util/Optional;Lnet/minecraft/world/level/biome/BiomeSpecialEffects$GrassColorModifier;Ljava/util/Optional;Ljava/util/Optional;Ljava/util/Optional;Ljava/util/Optional;Ljava/util/Optional;)V";

/// `Biome.getSpecialEffects()`, as 1.21.1 had it, for the biome object `biome`: made once from
/// the biome's file as the game has it now. Its water, and the foliage and grass it sets, are
/// still among the file's `effects`; its sky, fog and water fog are attributes of how the
/// biome looks (`minecraft:visual/sky_color`...), and where the file names none they are
/// 1.21.1's own defaults: the overworld's fog and water fog, and the sky its temperature gives
/// (`OverworldBiomes.calculateSkyColor`).
fn special_effects(rt: &mut Rt, biome: V) -> V {
    let made = rt.get_field(biome, "$effects");
    if made.r() != 0 {
        return made;
    }
    let json: serde_json::Value = rt
        .str_of(rt.get_field(biome, "$id"))
        .map(str::to_owned)
        .and_then(|id| rt.host().and_then(|h| h.biome_json(&id)))
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    let looks = |name: &str| {
        data_color(&json["effects"][name])
            .or_else(|| data_color(&json["attributes"][format!("minecraft:visual/{name}")]))
    };
    let temperature = json["temperature"].as_f64().unwrap_or(0.8) as f32;
    let set = |rt: &mut Rt, name: &str| {
        let held = match data_color(&json["effects"][name]) {
            Some(c) => crate::api_common::integer(rt, c),
            None => V::NULL,
        };
        crate::api_common::optional(rt, held)
    };
    let (foliage, grass) = (set(rt, "foliage_color"), set(rt, "grass_color"));
    let which = match json["effects"]["grass_color_modifier"].as_str() {
        Some("dark_forest") => "DARK_FOREST",
        Some("swamp") => "SWAMP",
        _ => "NONE",
    };
    let key = rt.static_key(&format!("{GRASS_MODIFIER}.{which}"));
    let modifier = crate::api::get_static(rt, key);
    let mut args = vec![
        V::I(looks("fog_color").unwrap_or(FOG)),
        V::I(looks("water_color").unwrap_or(PLAINS[2])),
        V::I(looks("water_fog_color").unwrap_or(WATER_FOG)),
        V::I(looks("sky_color").unwrap_or_else(|| sky_of(temperature))),
        foliage,
        grass,
        modifier,
    ];
    for _ in 0..5 {
        args.push(crate::api_common::optional(rt, V::NULL));
    }
    // (A data class of the records' table: the table answers its getters. On one line, with
    // the call that makes it: the records' test tells a class built so from one modelled here.)
    #[rustfmt::skip]
    let effects = crate::records::make_data(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects", EFFECTS_INIT, &args);
    rt.put_field(biome, "$effects", effects);
    effects
}

/// 1.21.1's sky for a biome of `temperature` (`OverworldBiomes.calculateSkyColor`): a hue and
/// a saturation that go with the temperature, at full brightness.
fn sky_of(temperature: f32) -> i32 {
    let f = (temperature / 3.0).clamp(-1.0, 1.0);
    let (h, s, v) = (0.622_222_24 - f * 0.05, 0.5 + f * 0.1, 1.0f32);
    // `Mth.hsvToRgb`.
    let i = (h * 6.0) as i32 % 6;
    let fr = h * 6.0 - (h * 6.0).floor();
    let (p, q, t) = (
        v * (1.0 - s),
        v * (1.0 - fr * s),
        v * (1.0 - (1.0 - fr) * s),
    );
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |c: f32| (c * 255.0) as i32 & 0xFF;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

/// `ItemColors.getColor(stack, tintIndex)`: the handler registered for the stack's item, else none.
fn item_color(rt: &mut Rt, stack: V, index: i32) -> i32 {
    let item = rt.get_field(stack, "$item");
    let Some(id) = rt.registered_name(item, "item").map(str::to_owned) else {
        return -1;
    };
    let Some(h) = handler(rt, "item", &id) else {
        return -1;
    };
    let c = rt.call_functional(ITEM_COLOR, h, &[stack, V::I(index)]);
    if rt.thrown.is_some() {
        rt.report_thrown("an item colour handler");
        return -1;
    }
    c.i()
}

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        BLOCK_EVENT => matches!(name, "register" | "getBlockColors"),
        ITEM_EVENT => matches!(name, "register" | "getItemColors" | "getBlockColors"),
        BLOCK_COLORS => matches!(name, "register" | "getColor"),
        ITEM_COLORS => matches!(name, "register" | "getColor"),
        MINECRAFT => matches!(
            name,
            "getBlockColors" | "getItemColors" | "$blockColors" | "$itemColors"
        ),
        BIOME_COLORS => BiomeColor::of_average(name).is_some(),
        GRASS_COLOR => matches!(name, "get" | "getDefaultColor"),
        FOLIAGE_COLOR => matches!(
            name,
            "get" | "getDefaultColor" | "getEvergreenColor" | "getBirchColor" | "getMangroveColor"
        ),
        REDSTONE_WIRE => name == "getColorForPower",
        RESOLVER => name == "getColor",
        BIOME => matches!(
            name,
            "getGrassColor"
                | "getFoliageColor"
                | "getWaterColor"
                | "getFogColor"
                | "getSkyColor"
                | "getWaterFogColor"
                | "getSpecialEffects"
                | "getModifiedSpecialEffects"
                | "$climateSettings"
                | "getGrassColorFromTexture"
                | "getFoliageColorFromTexture"
        ),
        CLIMATE => matches!(name, "temperature" | "downfall" | "hasPrecipitation"),
        GRASS_MODIFIER => name == "modifyColor",
        _ => name == "getBlockTint" && desc == BLOCK_TINT,
    }
}

/// A colour call, or None when this module doesn't model it.
pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (BLOCK_EVENT | BLOCK_COLORS, "register") => {
            register(rt, "block", a(1), a(2));
            V::Void
        }
        (ITEM_EVENT | ITEM_COLORS, "register") => {
            register(rt, "item", a(1), a(2));
            V::Void
        }
        // (And `Minecraft`'s two fields, as a mixin's accessor reads them.)
        (BLOCK_EVENT | ITEM_EVENT | MINECRAFT, "getBlockColors") | (MINECRAFT, "$blockColors") => {
            block_colors(rt)
        }
        (ITEM_EVENT | MINECRAFT, "getItemColors") | (MINECRAFT, "$itemColors") => item_colors(rt),
        // `getColor(state, level, pos)`: the handler with no level, else (vanilla: the state's map
        // colour, which isn't kept) none.
        (BLOCK_COLORS, "getColor") if desc.contains("Lnet/minecraft/world/level/Level;") => {
            let text = rt.str_of(a(1)).unwrap_or("").to_owned();
            let id = text.split('[').next().unwrap_or("");
            match handler(rt, "block", id) {
                Some(h) => V::I(run_block_handler(rt, h, a(1), V::NULL, V::NULL, 0)),
                None => V::I(-1),
            }
        }
        (BLOCK_COLORS, "getColor") => V::I(block_color(rt, a(1), a(2), a(3), a(4).i())),
        (ITEM_COLORS, "getColor") => V::I(item_color(rt, a(1), a(2).i())),
        (BIOME_COLORS, n) => V::I(average(rt, a(0), a(1), BiomeColor::of_average(n)?)),
        (GRASS_COLOR, "get") => V::I(grass_at(rt, a(0).d(), a(1).d())),
        (GRASS_COLOR, "getDefaultColor") => V::I(grass_default(rt)),
        (FOLIAGE_COLOR, "get") => V::I(foliage_at(rt, a(0).d(), a(1).d())),
        (FOLIAGE_COLOR, "getDefaultColor") => V::I(FOLIAGE_DEFAULT),
        (FOLIAGE_COLOR, "getEvergreenColor") => V::I(FOLIAGE_EVERGREEN),
        (FOLIAGE_COLOR, "getBirchColor") => V::I(FOLIAGE_BIRCH),
        (FOLIAGE_COLOR, "getMangroveColor") => V::I(FOLIAGE_MANGROVE),
        (REDSTONE_WIRE, "getColorForPower") => V::I(redstone_color(a(0).i())),
        // `BiomeColors`' resolvers: `Biome::getGrassColor`, `getFoliageColor()`, `getWaterColor()`.
        (RESOLVER, "getColor") => {
            let color = resolver_color(rt, a(0))?;
            V::I(biome_object_color(rt, a(1), color))
        }
        (BIOME, "getGrassColor") => V::I(biome_object_color(rt, a(0), BiomeColor::Grass)),
        (BIOME, "getFoliageColor") => V::I(biome_object_color(rt, a(0), BiomeColor::Foliage)),
        (BIOME, "getWaterColor") => V::I(biome_object_color(rt, a(0), BiomeColor::Water)),
        // 1.21.1's special effects of a biome, and its colours that live there.
        (BIOME, "getSpecialEffects" | "getModifiedSpecialEffects") => special_effects(rt, a(0)),
        (BIOME, "getFogColor" | "getSkyColor" | "getWaterFogColor") => {
            let effects = special_effects(rt, a(0));
            rt.get_field(
                effects,
                match name {
                    "getFogColor" => "fogColor",
                    "getSkyColor" => "skyColor",
                    _ => "waterFogColor",
                },
            )
        }
        // `modifyColor(x, z, color)`: a dark forest's darker green; a swamp's, the lighter of
        // its two (vanilla's noise picks one by position).
        (GRASS_MODIFIER, "modifyColor") => {
            let c = a(3).i();
            let which = crate::api_common::enum_name(rt, a(0), false);
            V::I(match rt.str_of(which) {
                Some("DARK_FOREST") => ((c & 0xFE_FEFE) + 0x28_340A) >> 1,
                Some("SWAMP") => 0x6A_7039,
                _ => c,
            })
        }
        // Its climate (`climateSettings`, a field mods read through an accessor).
        (BIOME, "$climateSettings") => {
            let biome = a(0);
            match rt.get_field(biome, "$climate") {
                made if made.r() != 0 => made,
                _ => {
                    let climate = rt.new_object(CLIMATE);
                    rt.put_field(climate, "$biome", biome);
                    rt.put_field(biome, "$climate", climate);
                    climate
                }
            }
        }
        (CLIMATE, "temperature" | "downfall" | "hasPrecipitation") => {
            let biome = rt.get_field(a(0), "$biome");
            rt.get_field(
                biome,
                match name {
                    "temperature" => "$temperature",
                    "downfall" => "$downfall",
                    _ => "$precipitation",
                },
            )
        }
        // The colour maps' own answer for its climate, whatever its file sets instead.
        (BIOME, "getGrassColorFromTexture" | "getFoliageColorFromTexture") => {
            let climate = |rt: &Rt, field: &'static str| {
                f64::from(rt.get_field(a(0), field).f()).clamp(0.0, 1.0)
            };
            let (temperature, downfall) = (climate(rt, "$temperature"), climate(rt, "$downfall"));
            V::I(if name == "getGrassColorFromTexture" {
                grass_at(rt, temperature, downfall)
            } else {
                foliage_at(rt, temperature, downfall)
            })
        }
        (_, "getBlockTint") if desc == BLOCK_TINT => V::I(block_tint(rt, a(0), a(1), a(2))),
        _ => return None,
    })
}

/// `BiomeColors.GRASS_COLOR_RESOLVER`...
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != BIOME_COLORS {
        return None;
    }
    let color = BIOME_COLORS_ALL
        .into_iter()
        .find(|c| c.resolver_field() == field)?;
    Some(resolver(rt, color))
}

/// The level [`block_tints`] runs a handler in: one block, air around it, nothing kept.
struct TintProbe {
    state: String,
}

impl TintProbe {
    const AT: [i32; 3] = [0, -4096, 0];
}

impl Host for TintProbe {
    fn block_state(&self, pos: [i32; 3]) -> String {
        if pos == Self::AT {
            self.state.clone()
        } else {
            "minecraft:air".into()
        }
    }
    fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
        false
    }
    fn is_server(&self) -> bool {
        false
    }
}

/// How state `state` (`create:controller_rail[power=3,...]`) tints its model, by tint index (the
/// last for any higher index): its mod's colour handler, run for indices 0 to 2 in a level of that
/// block, three times. The first two answer the biome colours with two sets of markers: a colour
/// equal to the marker both times is that biome colour, one equal across the runs a constant
/// (white: none). A colour that came out different without being a marker (a darkened foliage
/// colour) is the last run's, with plains' colours. None when the block has no handler.
pub fn block_tints(rt: &mut Rt, state: &str) -> Option<Vec<Tint>> {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let mut probe = TintProbe {
        state: state.to_owned(),
    };
    // The handlers are registered as the client is made.
    crate::glue::probe_setup(rt, &mut probe);
    let id = state.split('[').next().unwrap_or(state);
    let h = handler(rt, "block", id)?;
    let runs = rt.with_host(&mut probe, |rt| {
        let st = api::state(rt, state);
        let level = crate::glue::level(rt);
        let [x, y, z] = TintProbe::AT;
        let pos = api::block_pos(rt, x as i64, y as i64, z as i64);
        let mut runs = [[0; 3]; 3];
        for (run, colors) in runs.iter_mut().enumerate() {
            rt.interned.insert(PROBE.into(), V::I(run as i32 + 1));
            for (index, c) in colors.iter_mut().enumerate() {
                *c = run_block_handler(rt, h, st, level, pos, index as i32);
            }
        }
        rt.interned.remove(PROBE);
        runs
    });
    let rgb = |c: i32| c as u32 & 0xFF_FFFF;
    let sources: Vec<Tint> = (0..3)
        .map(|i| {
            let (a, b) = (runs[0][i], runs[1][i]);
            if let Some(color) = BIOME_COLORS_ALL.into_iter().find(|c| {
                rgb(a) == MARKERS[0][c.index()] as u32 && rgb(b) == MARKERS[1][c.index()] as u32
            }) {
                return match color {
                    BiomeColor::Grass => Tint::Grass,
                    BiomeColor::Foliage => Tint::Foliage,
                    BiomeColor::Water => Tint::Water,
                };
            }
            let c = if a == b { a } else { runs[2][i] };
            if rgb(c) == 0xFF_FFFF {
                Tint::None
            } else {
                Tint::Constant(rgb(c))
            }
        })
        .collect();
    let mut sources = sources;
    // The last source holds for any higher index: drop the repeats.
    while sources.len() > 1 && sources[sources.len() - 1] == sources[sources.len() - 2] {
        sources.pop();
    }
    Some(sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Data;
    use crate::api::call as c;

    const EVENT_REGISTER: &str = "net/neoforged/neoforge/client/event/RegisterColorHandlersEvent$Block.register.(Lnet/minecraft/client/color/block/BlockColor;[Lnet/minecraft/world/level/block/Block;)V";
    const GET_COLOR: &str = "net/minecraft/client/color/block/BlockColors.getColor.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;I)I";
    const GET_BLOCK_COLORS: &str = "net/minecraft/client/Minecraft.getBlockColors.()Lnet/minecraft/client/color/block/BlockColors;";
    const FOLIAGE_AVERAGE: &str = "net/minecraft/client/renderer/BiomeColors.getAverageFoliageColor.(Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;)I";
    const POWER_COLOR: &str =
        "net/minecraft/world/level/block/RedStoneWireBlock.getColorForPower.(I)I";

    /// Mod code: colour handlers as mods write them. `test/LeafColor` hands on the biome's
    /// foliage colour; `test/RailColor` the redstone colour of the state's power where it has a
    /// level (Create's controller rail); `test/FixedColor` one colour for index 0 and none above.
    fn handler_code(rt: &mut Rt, owner_key: &'static str, _key: &'static str, args: &[V]) -> V {
        let recv = args.first().copied().unwrap_or_default();
        let class = rt.class_of(recv);
        if !class.starts_with("test/") || args.len() < 5 {
            return c(rt, owner_key, args);
        }
        let (state, level, pos, index) = (args[1], args[2], args[3], args[4].i());
        match class {
            "test/LeafColor" => c(rt, FOLIAGE_AVERAGE, &[level, pos]),
            "test/RailColor" => {
                let power = rt
                    .str_of(state)
                    .and_then(|s| property(s, "power"))
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(0);
                let power = if level.r() != 0 && pos.r() != 0 {
                    power
                } else {
                    0
                };
                c(rt, POWER_COLOR, &[V::I(power)])
            }
            "test/FixedColor" if index == 0 => V::I(0x12_3456),
            _ => V::I(-1),
        }
    }

    /// Registers `handler` (an object of `class`) for a block `id` of the test's mod.
    fn register_block(rt: &mut Rt, id: &str, class: &'static str) {
        let block = rt.new_object("net/minecraft/world/level/block/Block");
        rt.registered.insert(("block".into(), id.into()), block);
        let handler = rt.new_object(class);
        let blocks = rt.alloc(
            "[Lnet/minecraft/world/level/block/Block;",
            Data::Array(vec![block]),
        );
        let ev = rt.new_object(BLOCK_EVENT);
        c(rt, EVENT_REGISTER, &[ev, handler, blocks]);
    }

    fn runtime() -> Rt {
        let mut rt = Rt::new();
        rt.vcall = Some(handler_code);
        // The client is set up: probing doesn't fire its setup again.
        rt.interned.insert("side:client".into(), V::I(1));
        rt
    }

    /// A handler registered through the event answers `BlockColors.getColor` for its block, on
    /// the client's one `BlockColors`; a block with none answers -1.
    #[test]
    fn a_registered_handler_answers_for_its_block() {
        let mut rt = runtime();
        register_block(&mut rt, "test:panel", "test/FixedColor");
        let colors = c(&mut rt, GET_BLOCK_COLORS, &[V::NULL]);
        assert_eq!(colors, block_colors(&mut rt));
        let panel = api::state(&mut rt, "test:panel");
        assert_eq!(
            c(
                &mut rt,
                GET_COLOR,
                &[colors, panel, V::NULL, V::NULL, V::I(0)]
            ),
            V::I(0x12_3456)
        );
        assert_eq!(
            c(
                &mut rt,
                GET_COLOR,
                &[colors, panel, V::NULL, V::NULL, V::I(1)]
            ),
            V::I(-1)
        );
        let other = api::state(&mut rt, "test:other");
        assert_eq!(
            c(
                &mut rt,
                GET_COLOR,
                &[colors, other, V::NULL, V::NULL, V::I(0)]
            ),
            V::I(-1)
        );
    }

    /// Vanilla's blocks answer as `BlockColors.createDefault` registers them: spruce leaves their
    /// constant, oak leaves the foliage default without a level and the biome's with one, the
    /// lily pad its two constants.
    #[test]
    fn vanilla_blocks_answer_their_own_colours() {
        let mut rt = runtime();
        let colors = block_colors(&mut rt);
        let ask = |rt: &mut Rt, state: &str, world: bool| {
            let st = api::state(rt, state);
            let (level, pos) = if world {
                (crate::glue::level(rt), api::block_pos(rt, 0, 64, 0))
            } else {
                (V::NULL, V::NULL)
            };
            c(rt, GET_COLOR, &[colors, st, level, pos, V::I(0)]).i()
        };
        assert_eq!(
            ask(
                &mut rt,
                "minecraft:spruce_leaves[distance=7,persistent=false,waterlogged=false]",
                true
            ),
            0xFF61_9961_u32 as i32
        );
        assert_eq!(
            ask(
                &mut rt,
                "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]",
                false
            ),
            0xFF48_B518_u32 as i32
        );
        // No host biome: plains' foliage.
        assert_eq!(
            ask(
                &mut rt,
                "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]",
                true
            ) & 0xFF_FFFF,
            0x77_AB2F
        );
        assert_eq!(
            ask(&mut rt, "minecraft:lily_pad", true),
            0xFF20_8030_u32 as i32
        );
        assert_eq!(
            ask(&mut rt, "minecraft:lily_pad", false),
            0xFF71_C35C_u32 as i32
        );
        assert_eq!(ask(&mut rt, "minecraft:stone", true), -1);
    }

    /// `RedStoneWireBlock.getColorForPower`: vanilla's floats, worked by hand. Power 0 is
    /// 0.3 red (76); power 15 is full red, green 0.7 - 0.5 (just under 0.2: 50); power 10 is
    /// red 0.4 + 0.4 (204), green 0.444 * 0.7 - 0.5 (below 0: none).
    #[test]
    fn redstone_colours_follow_vanillas_floats() {
        assert_eq!(redstone_color(0), 0x4C_0000);
        assert_eq!(redstone_color(15), 0xFF_3200);
        assert_eq!(redstone_color(10), 0xCC_0000);
    }

    /// The probe finds what a handler tints by: a foliage handler is the biome's foliage at every
    /// index, a fixed handler its constant then none, and the rail's colour comes from its state
    /// (each power its own constant). A block with no handler has nothing to say.
    #[test]
    fn the_probe_tells_biome_colours_from_constants() {
        let mut rt = runtime();
        register_block(&mut rt, "test:leaves", "test/LeafColor");
        register_block(&mut rt, "test:panel", "test/FixedColor");
        register_block(&mut rt, "test:rail", "test/RailColor");
        assert_eq!(
            block_tints(&mut rt, "test:leaves"),
            Some(vec![Tint::Foliage])
        );
        assert_eq!(
            block_tints(&mut rt, "test:panel"),
            Some(vec![Tint::Constant(0x12_3456), Tint::None])
        );
        assert_eq!(
            block_tints(&mut rt, "test:rail[power=0,shape=north_south]"),
            Some(vec![Tint::Constant(0x4C_0000)])
        );
        assert_eq!(
            block_tints(&mut rt, "test:rail[power=15,shape=north_south]"),
            Some(vec![Tint::Constant(0xFF_3200)])
        );
        assert_eq!(block_tints(&mut rt, "test:stone"), None);
        // The probe leaves no marker behind: a later ask has plains' foliage.
        let level = crate::glue::level(&mut rt);
        let pos = api::block_pos(&mut rt, 0, 64, 0);
        assert_eq!(
            c(&mut rt, FOLIAGE_AVERAGE, &[level, pos]).i() & 0xFF_FFFF,
            0x77_AB2F
        );
    }

    /// `BiomeColors`' resolvers are statics that `getBlockTint` reads as the averages do.
    #[test]
    fn the_resolvers_answer_through_get_block_tint() {
        let mut rt = runtime();
        let water = api::get_static(
            &mut rt,
            "net/minecraft/client/renderer/BiomeColors.WATER_COLOR_RESOLVER",
        );
        let level = crate::glue::level(&mut rt);
        let pos = api::block_pos(&mut rt, 0, 64, 0);
        let tint = "net/minecraft/world/level/BlockAndTintGetter.getBlockTint.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/ColorResolver;)I";
        assert_eq!(c(&mut rt, tint, &[level, pos, water]), V::I(0x3F_76E4));
    }

    /// The biome a level hands out at a position is there to ask (`getBiome(pos).value()`): it
    /// snows where its temperature is under 0.15, which height brings a temperate biome to far
    /// above the clouds, and nothing falls in one without precipitation.
    /// 1.21.1's special effects of a biome, from the biome's file as the game has it now: its
    /// sky and fog are attributes of how it looks there, its water and what it sets its foliage
    /// to are still its effects. Where the file names no sky it is the one 1.21.1 made of the
    /// temperature (plains at 0.8: 0x78A7FF, vanilla's own), and no fog is the overworld's.
    #[test]
    fn a_biomes_special_effects_are_read_from_its_file() {
        struct Biomes;
        impl Host for Biomes {
            fn block_state(&self, _pos: [i32; 3]) -> String {
                "minecraft:air".into()
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
            fn biome(&self, pos: [i32; 3]) -> Option<String> {
                Some(
                    if pos[0] < 0 {
                        "minecraft:swamp"
                    } else {
                        "minecraft:plains"
                    }
                    .into(),
                )
            }
            fn biome_json(&self, id: &str) -> Option<String> {
                Some(match id {
                    "minecraft:swamp" => r##"{"temperature":0.8,"downfall":0.9,"has_precipitation":true,
                        "attributes":{"minecraft:visual/sky_color":"#78a7ff","minecraft:visual/fog_color":"#ccd8e2","minecraft:visual/water_fog_color":"#232317"},
                        "effects":{"water_color":"#617b64","foliage_color":"#6a7039","grass_color_modifier":"swamp"}}"##.into(),
                    _ => r##"{"temperature":0.8,"downfall":0.4,"has_precipitation":true,"effects":{"water_color":"#3f76e4"}}"##.into(),
                })
            }
        }
        let mut rt = runtime();
        let mut host = Biomes;
        rt.with_host(&mut host, |rt| {
            use crate::api::call as ask;
            let level = crate::glue::level(rt);
            let biome_at = |rt: &mut Rt, x: i64| {
                let pos = crate::api::block_pos(rt, x, 64, 0);
                let holder = ask(rt, "net/minecraft/world/level/Level.getBiome.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/Holder;", &[level, pos]);
                ask(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder])
            };
            let plains = biome_at(rt, 5);
            // The biomes' registry holds that very object under its key.
            assert_eq!(crate::neo::registry_get(rt, "biome", "minecraft:plains"), plains);
            assert_eq!(ask(rt, "net/minecraft/world/level/biome/Biome.getSkyColor.()I", &[plains]), V::I(0x78_A7FF));
            assert_eq!(ask(rt, "net/minecraft/world/level/biome/Biome.getFogColor.()I", &[plains]), V::I(0xC0_D8FF));
            let swamp = biome_at(rt, -5);
            const EFFECTS_OF: &str = "net/minecraft/world/level/biome/Biome.getSpecialEffects.()Lnet/minecraft/world/level/biome/BiomeSpecialEffects;";
            let effects = ask(rt, EFFECTS_OF, &[swamp]);
            assert_eq!(ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects.getFogColor.()I", &[effects]), V::I(0xCC_D8E2));
            assert_eq!(ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects.getWaterColor.()I", &[effects]), V::I(0x61_7B64));
            assert_eq!(ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects.getWaterFogColor.()I", &[effects]), V::I(0x23_2317));
            let foliage = ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects.getFoliageColorOverride.()Ljava/util/Optional;", &[effects]);
            let held = ask(rt, "java/util/Optional.get.()Ljava/lang/Object;", &[foliage]);
            assert_eq!(crate::mixin::unbox(rt, held, 'I'), V::I(0x6A_7039));
            let grass = ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects.getGrassColorOverride.()Ljava/util/Optional;", &[effects]);
            assert_eq!(ask(rt, "java/util/Optional.isPresent.()Z", &[grass]), V::I(0));
            let modifier = ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects.getGrassColorModifier.()Lnet/minecraft/world/level/biome/BiomeSpecialEffects$GrassColorModifier;", &[effects]);
            let name = crate::api_common::enum_name(rt, modifier, false);
            assert_eq!(rt.str_of(name), Some("SWAMP"));
            // A dark forest's grass is darkened so (vanilla's own arithmetic on plains' green).
            let dark = crate::api::get_static(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects$GrassColorModifier.DARK_FOREST");
            assert_eq!(
                ask(rt, "net/minecraft/world/level/biome/BiomeSpecialEffects$GrassColorModifier.modifyColor.(DDI)I", &[dark, V::D(0.0), V::D(0.0), V::I(0x91_BD59)]),
                V::I(0x5C_7831)
            );
            // Its climate, as the accessor reads the field.
            let climate = ask(rt, "net/minecraft/world/level/biome/Biome.$climateSettings.()Lnet/minecraft/world/level/biome/Biome$ClimateSettings;", &[swamp]);
            assert_eq!(ask(rt, "net/minecraft/world/level/biome/Biome$ClimateSettings.downfall.()F", &[climate]), V::F(0.9));
            // The same object each time.
            assert_eq!(ask(rt, EFFECTS_OF, &[swamp]), effects);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_levels_biome_says_whether_it_rains_or_snows() {
        struct Climates;
        impl Host for Climates {
            fn block_state(&self, _pos: [i32; 3]) -> String {
                "minecraft:air".into()
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
            fn biome(&self, pos: [i32; 3]) -> Option<String> {
                Some(
                    match pos[0] {
                        x if x < 0 => "minecraft:snowy_plains",
                        0..=9 => "minecraft:plains",
                        _ => "minecraft:desert",
                    }
                    .into(),
                )
            }
            fn biome_json(&self, id: &str) -> Option<String> {
                Some(match id {
                    "minecraft:snowy_plains" => {
                        r#"{"temperature":0.0,"downfall":0.5,"has_precipitation":true}"#.into()
                    }
                    "minecraft:plains" => {
                        r#"{"temperature":0.8,"downfall":0.4,"has_precipitation":true}"#.into()
                    }
                    _ => r#"{"temperature":2.0,"downfall":0.0,"has_precipitation":false}"#.into(),
                })
            }
        }
        let mut rt = runtime();
        let mut host = Climates;
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let falls = |rt: &mut Rt, at: [i64; 3]| {
                let pos = api::block_pos(rt, at[0], at[1], at[2]);
                let holder = c(rt, "net/minecraft/world/level/Level.getBiome.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/Holder;", &[level, pos]);
                let biome = c(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]);
                assert!(biome.r() != 0, "no biome at {at:?}");
                let cold = c(rt, "net/minecraft/world/level/biome/Biome.coldEnoughToSnow.(Lnet/minecraft/core/BlockPos;)Z", &[biome, pos]).i() != 0;
                let kind = c(rt, "net/minecraft/world/level/biome/Biome.getPrecipitationAt.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/biome/Biome$Precipitation;", &[biome, pos]);
                let kind = c(rt, "java/lang/Enum.name.()Ljava/lang/String;", &[kind]);
                (cold, rt.str_of(kind).unwrap_or("?").to_owned())
            };
            assert_eq!(falls(rt, [-5, 64, 0]), (true, "SNOW".to_owned()));
            assert_eq!(falls(rt, [5, 64, 0]), (false, "RAIN".to_owned()));
            assert_eq!(falls(rt, [5, 100, 0]), (false, "RAIN".to_owned()));
            assert_eq!(falls(rt, [5, 700, 0]), (true, "SNOW".to_owned()));
            assert_eq!(falls(rt, [15, 64, 0]), (false, "NONE".to_owned()));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A biome's colours from its data: its own water and foliage colours, dark forest's grass
    /// modifier on the map's colour; plains where nothing is said.
    #[test]
    fn biome_data_colours_and_modifiers() {
        struct Biomes;
        impl Host for Biomes {
            fn block_state(&self, _pos: [i32; 3]) -> String {
                "minecraft:air".into()
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
            fn biome(&self, pos: [i32; 3]) -> Option<String> {
                Some(
                    if pos[0] < 0 {
                        "minecraft:swamp"
                    } else {
                        "minecraft:dark_forest"
                    }
                    .into(),
                )
            }
            fn biome_json(&self, id: &str) -> Option<String> {
                Some(match id {
                    "minecraft:swamp" => r##"{"temperature":0.8,"downfall":0.9,"effects":{"water_color":"#617b64","foliage_color":"#6a7039","grass_color_modifier":"swamp"}}"##.into(),
                    _ => r##"{"temperature":0.7,"downfall":0.8,"effects":{"water_color":4159204,"grass_color":"#406020","grass_color_modifier":"dark_forest"}}"##.into(),
                })
            }
        }
        let mut rt = runtime();
        let mut host = Biomes;
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let swamp = api::block_pos(rt, -5, 64, 0);
            let forest = api::block_pos(rt, 5, 64, 0);
            let ask = |rt: &mut Rt, name: &str, pos: V| {
                let key = rt.static_key(&format!("{BIOME_COLORS}.{name}.(Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;)I"));
                c(rt, key, &[level, pos]).i()
            };
            assert_eq!(ask(rt, "getAverageWaterColor", swamp), 0x61_7B64);
            assert_eq!(ask(rt, "getAverageFoliageColor", swamp), 0x6A_7039);
            assert_eq!(ask(rt, "getAverageGrassColor", swamp), 0x6A_7039);
            assert_eq!(ask(rt, "getAverageWaterColor", forest), 0x3F_76E4);
            // Dark forest: its own grass colour, 0x406020 (every byte even), plus 0x28340A is
            // 0x68942A, halved.
            assert_eq!(ask(rt, "getAverageGrassColor", forest), 0x34_4A15);
        });
    }
}
