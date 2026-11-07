//! What a level answers about a block's surroundings, from the host's world: light, the time of
//! day and the sky, the weather, redstone reaching a block, and its scheduled ticks
//! (`Level.getBrightness`, `getSkyDarken`, `isDay`, `getRainLevel`, `hasNeighborSignal`,
//! `getSignal`, `scheduleTick`...). A solar panel, a lamp, a clutch or a redstone-driven machine
//! reads these. The sky follows 1.21.1's formulas (`DimensionType.timeOfDay`,
//! `Level.updateSkyBrightness`).
use crate::{Rt, V};

pub(crate) const LIGHT_LAYER: &str = "net/minecraft/world/level/LightLayer";
const LEVEL_TICKS: &str = "net/minecraft/world/ticks/LevelTicks";
/// A block's neighbour each way, by `Direction`'s data value (down, up, north, south, west, east),
/// the order `SignalGetter` asks them in.
const STEPS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

/// `Mth.cos`: 1.21.1 reads a table of 65536 sines.
fn mth_cos(f: f32) -> f32 {
    let i = ((f * 10430.378_f32 + 16384.0_f32) as i32) & 65535;
    (f64::from(i) * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

/// The dimension's fixed time (`DimensionType.fixedTime`): the Nether's 18000, the End's 6000.
fn fixed_time(rt: &Rt) -> Option<i64> {
    match rt.host().map(|h| h.dimension()).as_deref() {
        Some("minecraft:the_nether") => Some(18000),
        Some("minecraft:the_end") => Some(6000),
        _ => None,
    }
}

fn day_time(rt: &Rt) -> i64 {
    rt.host().map_or(0, |h| h.day_time())
}

/// `DimensionType.timeOfDay(dayTime)`: 0.0 at noon's start of the cycle, 0.5 at midnight.
pub(crate) fn time_of_day(rt: &Rt) -> f32 {
    let t = fixed_time(rt).unwrap_or_else(|| day_time(rt)) as f64 / 24000.0 - 0.25;
    let d0 = t - t.floor();
    let d1 = 0.5 - (d0 * std::f64::consts::PI).cos() / 2.0;
    (d0 * 2.0 + d1) as f32 / 3.0
}

fn weather(rt: &Rt) -> (f32, f32) {
    rt.host().map_or((0.0, 0.0), |h| h.weather())
}

/// `Level.skyDarken` (`updateSkyBrightness`): 0 in full daylight, up to 11 at night, more darkened
/// by rain and thunder.
pub(crate) fn sky_darken(rt: &Rt) -> i32 {
    let (rain, thunder) = weather(rt);
    let d0 = 1.0 - f64::from(rain * 5.0) / 16.0;
    let d1 = 1.0 - f64::from(thunder * 5.0) / 16.0;
    let d2 = 0.5 + 2.0 * f64::from(mth_cos(time_of_day(rt) * std::f32::consts::TAU)).clamp(-0.25, 0.25);
    ((1.0 - d2 * d0 * d1) * 11.0) as i32
}

fn light(rt: &Rt, pos: [i32; 3]) -> (i32, i32) {
    let (block, sky) = rt.host().map_or((0, 15), |h| h.light(pos));
    (i32::from(block), i32::from(sky))
}

/// `getRawBrightness(pos, darkening)`: the brighter of the block light and the sky's less `darkening`.
pub(crate) fn raw_brightness(rt: &Rt, pos: [i32; 3], darkening: i32) -> i32 {
    let (block, sky) = light(rt, pos);
    block.max(sky - darkening)
}

/// The level's answer to `name` (`desc`, `args` with the level first), or None for another call.
pub(crate) fn call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let pos = |rt: &Rt, i: usize| crate::api::pos_of(rt, a(i));
    Some(match (name, desc) {
        // `getBrightness(LightLayer, pos)`: SKY is the layer's first constant.
        ("getBrightness", _) => {
            let (block, sky) = light(rt, pos(rt, 2));
            let layer = rt.get_field(a(1), "$name");
            V::I(if rt.str_of(layer) == Some("BLOCK") { block } else { sky })
        }
        ("getRawBrightness", _) => V::I(raw_brightness(rt, pos(rt, 1), a(2).i())),
        ("getMaxLocalRawBrightness", "(Lnet/minecraft/core/BlockPos;)I") => {
            let darken = sky_darken(rt);
            V::I(raw_brightness(rt, pos(rt, 1), darken))
        }
        ("getMaxLocalRawBrightness", _) => V::I(raw_brightness(rt, pos(rt, 1), a(2).i())),
        // `BlockAndTintGetter.canSeeSky`: full sky light there.
        ("canSeeSky", _) => V::I((light(rt, pos(rt, 1)).1 >= 15) as i32),
        ("getSkyDarken", _) => V::I(sky_darken(rt)),
        ("getDayTime", "()J") => V::J(day_time(rt)),
        ("getTimeOfDay", _) => V::F(time_of_day(rt)),
        ("getSunAngle", _) => V::F(time_of_day(rt) * std::f32::consts::TAU),
        ("getMoonPhase", _) => V::I(((fixed_time(rt).unwrap_or_else(|| day_time(rt)) / 24000 % 8 + 8) % 8) as i32),
        ("isDay", _) => V::I((fixed_time(rt).is_none() && sky_darken(rt) < 4) as i32),
        ("isNight", _) => V::I((fixed_time(rt).is_none() && sky_darken(rt) >= 4) as i32),
        ("getRainLevel", _) => V::F(weather(rt).0),
        ("getThunderLevel", _) => V::F(weather(rt).1),
        ("isRaining", "()Z") => V::I((f64::from(weather(rt).0) > 0.2) as i32),
        ("isThundering", "()Z") => V::I((f64::from(weather(rt).1) > 0.9) as i32),
        // Raining on the block: under open sky. (Vanilla also asks the biome: no rain in a desert.)
        ("isRainingAt", _) => V::I((f64::from(weather(rt).0) > 0.2 && light(rt, pos(rt, 1)).1 >= 15) as i32),
        // `SignalGetter`: the strongest signal any neighbour sends it. Each read is the game's call,
        // inside which a neighbour of the mod's answers by its own code (an analog lever's level).
        ("hasNeighborSignal", _) => {
            let p = pos(rt, 1);
            V::I((rt.host_call(|h| h.neighbor_signal(p)).unwrap_or(0) > 0) as i32)
        }
        ("getBestNeighborSignal", _) => {
            let p = pos(rt, 1);
            V::I(rt.host_call(|h| h.neighbor_signal(p)).unwrap_or(0).clamp(0, 15))
        }
        // `getSignal(pos, direction)`: what the block at `pos` sends back along `direction` (the
        // way from the block asking to it), a nixie tube's or a lamp's input; `hasSignal`, whether
        // it sends any; `getDirectSignal`, its strong signal alone.
        ("getSignal" | "hasSignal" | "getDirectSignal", "(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)I" | "(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Z") => {
            let (p, d) = (pos(rt, 1), crate::api_common::direction_ordinal(rt, a(2)));
            let direct = name == "getDirectSignal";
            let signal = rt.host_call(|h| h.signal(p, d, direct)).unwrap_or(0).clamp(0, 15);
            V::I(if name == "hasSignal" { (signal > 0) as i32 } else { signal })
        }
        // `getControlInputSignal(pos, direction, diodesOnly)`: what a diode's side reads there.
        ("getControlInputSignal", "(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Z)I") => {
            let (p, d) = (pos(rt, 1), crate::api_common::direction_ordinal(rt, a(2)));
            V::I(control_input_signal(rt, p, d, a(3).i() != 0))
        }
        // `getDirectSignalTo(pos)`: the strongest strong signal its neighbours send into it.
        ("getDirectSignalTo", "(Lnet/minecraft/core/BlockPos;)I") => {
            let p = pos(rt, 1);
            let mut best = 0;
            for (d, step) in STEPS.iter().enumerate() {
                let n = [p[0] + step[0], p[1] + step[1], p[2] + step[2]];
                best = best.max(rt.host_call(|h| h.signal(n, d as u8, true)).unwrap_or(0).clamp(0, 15));
                if best >= 15 {
                    break;
                }
            }
            V::I(best)
        }
        // `scheduleTick(pos, block, delay[, priority])`, a priority by `TickPriority`'s value (its
        // constants run from `EXTREMELY_HIGH`, -3); a fluid's ticks are the game's own.
        ("scheduleTick", d) if d.starts_with("(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;I") => {
            let (p, block) = (pos(rt, 1), crate::api::block_name(rt, a(2)));
            let priority = d.contains("TickPriority").then(|| crate::api_common::enum_ordinal(rt, a(4)) - 3);
            if !block.is_empty()
                && let Some(h) = rt.host_mut()
            {
                match priority {
                    Some(priority) => h.schedule_block_tick_at_priority(p, &block, a(3).i(), priority),
                    None => h.schedule_block_tick(p, &block, a(3).i()),
                };
            }
            V::Void
        }
        ("scheduleTick", _) => V::Void,
        // The block ticks: the game keeps them, a tick queued again is kept once.
        ("getBlockTicks" | "getFluidTicks", _) => rt.intern("compat:level-ticks".into(), |rt| rt.new_object(LEVEL_TICKS)),
        // The level's random, by its getter or its public field (`level.random`).
        ("getRandom" | "$random", "()Lnet/minecraft/util/RandomSource;") => rt.intern("compat:level-random".into(), |rt| rt.new_object("net/minecraft/util/RandomSource")),
        // The players in the level: a server's in this dimension, a client's own.
        ("players", "()Ljava/util/List;") => {
            let players = if crate::neo::is_client(rt) { vec![crate::network::local_player(rt)] } else { crate::events::server_players(rt) };
            crate::jutil::new_list(rt, players)
        }
        ("getMinBuildHeight" | "getMinY", "()I") => V::I(rt.host().map_or(-64, |h| h.world_height().0)),
        ("getMaxBuildHeight", "()I") => V::I(rt.host().map_or(320, |h| h.world_height().0 + h.world_height().1)),
        ("getMaxY", "()I") => V::I(rt.host().map_or(319, |h| h.world_height().0 + h.world_height().1 - 1)),
        ("getHeight", "()I") => V::I(rt.host().map_or(384, |h| h.world_height().1)),
        // A column's top by heightmap, and the world's seed: what worldgen code reads of its region.
        ("getHeight", "(Lnet/minecraft/world/level/levelgen/Heightmap$Types;II)I") => crate::worldgen::height(rt, a(1), a(2).i(), a(3).i()),
        // `getHeightmapPos(type, pos)`: that column's top as a position (`getHeight` at its x, z).
        ("getHeightmapPos", "(Lnet/minecraft/world/level/levelgen/Heightmap$Types;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/BlockPos;") => {
            let [x, _, z] = pos(rt, 2);
            let y = crate::worldgen::height(rt, a(1), x, z).i();
            crate::api::block_pos(rt, x.into(), y.into(), z.into())
        }
        ("getSeed", "()J") => V::J(rt.host().and_then(|h| h.world_seed()).unwrap_or(0)),
        // Whether the region being generated may write at a position (a feature asks before it does).
        ("ensureCanWrite", "(Lnet/minecraft/core/BlockPos;)Z") => {
            let p = pos(rt, 1);
            V::I(rt.host().is_none_or(|h| h.can_write(p)) as i32)
        }
        // The biome at a position: its registry holder, whose tags the host answers (plains where
        // the host keeps no biomes, as vanilla's own fallback biome).
        ("getBiome", "(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/Holder;") => {
            let p = pos(rt, 1);
            let name = rt.host().and_then(|h| h.biome(p)).unwrap_or_else(|| "minecraft:plains".to_owned());
            crate::neo::registry_holder(rt, "biome", &name, V::NULL)
        }
        _ => return None,
    })
}

/// `SignalGetter.getControlInputSignal(pos, direction, diodesOnly)`: the side input a diode (a
/// repeater's lock, a comparator's subtraction) reads from the block at `pos`, `direction` being the
/// way to it: a diode's strong signal where only diodes count; else a redstone block's 15, a wire's
/// power, or the block's strong signal (none from a block that is no signal source).
pub(crate) fn control_input_signal(rt: &mut Rt, p: [i32; 3], direction: u8, diodes_only: bool) -> i32 {
    let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
    let (block, props) = crate::host::split_state(&text);
    let direct = |rt: &mut Rt| rt.host_call(|h| h.signal(p, direction, true)).unwrap_or(0).clamp(0, 15);
    if diodes_only {
        return if crate::diodes::is_diode(rt, &text) { direct(rt) } else { 0 };
    }
    match block {
        "minecraft:redstone_block" => 15,
        "minecraft:redstone_wire" => props.iter().find(|(k, _)| *k == "power").and_then(|(_, v)| v.parse().ok()).unwrap_or(0),
        _ => direct(rt),
    }
}

/// `LevelTicks`/`LevelTickAccess` (what `getBlockTicks` gives, and `getFluidTicks`): whether a
/// block's tick is queued at a position (`hasScheduledTick`) or about to run in the game tick under
/// way (`willTickThisTick`, which a diode or a nixie tube asks before it schedules one), from the
/// host; a fluid's never reads as queued, so a mod schedules again, which keeps the tick already
/// there. `copyArea` (a clone command's) copies none, as the game's own `/clone` doesn't.
pub(crate) fn ticks_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match name {
        "hasScheduledTick" | "willTickThisTick" => {
            let p = crate::api::pos_of(rt, a(1));
            let block = crate::api::block_name(rt, a(2));
            let fluid = rt.registered_name(a(2), "fluid").is_some() || crate::fluids::is_fluid_class(rt.class_of(a(2)));
            let known = !fluid && !block.is_empty() && !block.starts_with(crate::api::UNREGISTERED);
            let this_tick = name == "willTickThisTick";
            Some(V::I((known && rt.host_mut().is_some_and(|h| h.block_tick_queued(p, &block, this_tick))) as i32))
        }
        "schedule" | "clearArea" | "copyArea" | "copyAreaFrom" => Some(V::Void),
        "count" => Some(V::I(0)),
        _ => None,
    }
}

pub(crate) const BLOCK_IN_WORLD: &str = "net/minecraft/world/level/block/state/pattern/BlockInWorld";
const HAS_CHUNK_AT: (&str, &str) =
    ("net/minecraft/world/level/LevelReader.hasChunkAt.(Lnet/minecraft/core/BlockPos;)Z", "hasChunkAt.(Lnet/minecraft/core/BlockPos;)Z");
const GET_BLOCK_STATE: (&str, &str) = (
    "net/minecraft/world/level/BlockGetter.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;",
    "getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;",
);
const GET_BLOCK_ENTITY: (&str, &str) = (
    "net/minecraft/world/level/BlockGetter.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
    "getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
);

/// `BlockInWorld(level, pos, loadChunks)`: a level's block whose state and block entity are read
/// from the level (a mod's own level too) once, when first asked, as 1.21.1's. Without
/// `loadChunks`, a state where the level has no chunk is null.
pub(crate) fn block_in_world(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let at = |rt: &Rt| [rt.get_field(this, "level"), rt.get_field(this, "pos")];
    Some(match name {
        "<init>" => {
            let p = crate::api::pos_of(rt, a(2));
            let pos = crate::api::block_pos(rt, p[0].into(), p[1].into(), p[2].into());
            rt.put_field(this, "level", a(1));
            rt.put_field(this, "pos", pos);
            rt.put_field(this, "loadChunks", a(3));
            V::Void
        }
        "getState" => match rt.get_field(this, "state") {
            V::R(n) if n != 0 => V::R(n),
            _ => {
                let level_pos = at(rt);
                let loaded = rt.get_field(this, "loadChunks").i() != 0 || crate::api::virtual_call(rt, HAS_CHUNK_AT, &level_pos).i() != 0;
                if !loaded {
                    return Some(V::NULL);
                }
                let state = crate::api::virtual_call(rt, GET_BLOCK_STATE, &level_pos);
                rt.put_field(this, "state", state);
                state
            }
        },
        "getEntity" => {
            if rt.get_field(this, "cachedEntity").i() == 0 {
                let level_pos = at(rt);
                let entity = crate::api::virtual_call(rt, GET_BLOCK_ENTITY, &level_pos);
                rt.put_field(this, "entity", entity);
                rt.put_field(this, "cachedEntity", V::I(1));
            }
            match rt.get_field(this, "entity") {
                V::R(n) => V::R(n),
                _ => V::NULL,
            }
        }
        "getLevel" => rt.get_field(this, "level"),
        "getPos" => rt.get_field(this, "pos"),
        _ => return None,
    })
}

pub(crate) fn is_ticks(owner: &str) -> bool {
    matches!(
        owner,
        LEVEL_TICKS | "net/minecraft/world/ticks/LevelTickAccess" | "net/minecraft/world/ticks/TickAccess" | "net/minecraft/world/ticks/BlackholeTickAccess"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn with_host<R>(host: &mut MemoryHost, f: impl FnOnce(&mut Rt) -> R) -> R {
        crate::install();
        let mut rt = Rt::new();
        rt.with_host(host, f)
    }

    const LEVEL: &str = "net/minecraft/world/level/Level";

    fn level_call(rt: &mut Rt, name_desc: &str, args: &[V]) -> V {
        let key: &'static str = Box::leak(format!("{LEVEL}.{name_desc}").into_boxed_str());
        crate::api::call(rt, key, args)
    }

    #[test]
    fn the_sky_darkens_as_vanilla_and_beds_work_when_vanilla_lets_them() {
        let sky = |time: i64, weather: (f32, f32)| {
            let mut host = MemoryHost { day_time: time, weather, ..MemoryHost::default() };
            with_host(&mut host, |rt| {
                let level = rt.new_object(LEVEL);
                (level_call(rt, "getSkyDarken.()I", &[level]).i(), level_call(rt, "isDay.()Z", &[level]).i() != 0)
            })
        };
        // Full daylight at noon, the darkest sky (11) at midnight.
        assert_eq!(sky(6000, (0.0, 0.0)).0, 0);
        assert_eq!(sky(18000, (0.0, 0.0)).0, 11);
        // The wiki's bed times (a bed works while `isDay` is false): from 12542 to 23459 in clear
        // weather, from 12010 to 23991 in rain, and all day in a thunderstorm.
        for (time, weather, day) in [
            (12541, (0.0, 0.0), true),
            (12542, (0.0, 0.0), false),
            (23459, (0.0, 0.0), false),
            (23460, (0.0, 0.0), true),
            (12009, (1.0, 0.0), true),
            (12010, (1.0, 0.0), false),
            (23991, (1.0, 0.0), false),
            (23992, (1.0, 0.0), true),
            (6000, (1.0, 1.0), false),
        ] {
            assert_eq!(sky(time, weather).1, day, "time {time}, weather {weather:?}");
        }
    }

    #[test]
    fn light_signal_weather_and_ticks_come_from_the_world() {
        let mut host = MemoryHost { day_time: 6000, weather: (1.0, 0.0), ..MemoryHost::default() };
        host.light.insert([1, 64, 1], (12, 3));
        host.signals.insert([2, 64, 2], 9);
        host.blocks.insert([3, 64, 3], "minecraft:stone".into());
        with_host(&mut host, |rt| {
            let level = rt.new_object(LEVEL);
            let at = |rt: &mut Rt, p: [i32; 3]| {
                let v = rt.new_object("net/minecraft/core/BlockPos");
                crate::api::call(rt, "net/minecraft/core/BlockPos.<init>.(III)V", &[v, V::I(p[0]), V::I(p[1]), V::I(p[2])]);
                v
            };
            let (dim, open, lit) = (at(rt, [1, 64, 1]), at(rt, [5, 64, 5]), at(rt, [2, 64, 2]));
            let sky = crate::api::get_static(rt, "net/minecraft/world/level/LightLayer.SKY");
            let block = crate::api::get_static(rt, "net/minecraft/world/level/LightLayer.BLOCK");
            let brightness = "getBrightness.(Lnet/minecraft/world/level/LightLayer;Lnet/minecraft/core/BlockPos;)I";
            assert_eq!(level_call(rt, brightness, &[level, sky, dim]).i(), 3);
            assert_eq!(level_call(rt, brightness, &[level, block, dim]).i(), 12);
            // Raining: the noon sky darkened by rain (1.0 → 3), the block light brighter.
            assert_eq!(level_call(rt, "getMaxLocalRawBrightness.(Lnet/minecraft/core/BlockPos;)I", &[level, dim]).i(), 12);
            assert_eq!(level_call(rt, "getMaxLocalRawBrightness.(Lnet/minecraft/core/BlockPos;)I", &[level, open]).i(), 12);
            assert_eq!(level_call(rt, "canSeeSky.(Lnet/minecraft/core/BlockPos;)Z", &[level, dim]).i(), 0);
            assert_eq!(level_call(rt, "canSeeSky.(Lnet/minecraft/core/BlockPos;)Z", &[level, open]).i(), 1);
            assert_eq!(level_call(rt, "isRaining.()Z", &[level]).i(), 1);
            assert_eq!(level_call(rt, "isRainingAt.(Lnet/minecraft/core/BlockPos;)Z", &[level, open]).i(), 1);
            assert_eq!(level_call(rt, "isRainingAt.(Lnet/minecraft/core/BlockPos;)Z", &[level, dim]).i(), 0);
            assert_eq!(level_call(rt, "isThundering.()Z", &[level]).i(), 0);
            assert_eq!(level_call(rt, "hasNeighborSignal.(Lnet/minecraft/core/BlockPos;)Z", &[level, lit]).i(), 1);
            assert_eq!(level_call(rt, "getBestNeighborSignal.(Lnet/minecraft/core/BlockPos;)I", &[level, lit]).i(), 9);
            assert_eq!(level_call(rt, "hasNeighborSignal.(Lnet/minecraft/core/BlockPos;)Z", &[level, dim]).i(), 0);
            let stone = crate::api::get_static(rt, "net/minecraft/world/level/block/Blocks.STONE");
            level_call(rt, "scheduleTick.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;I)V", &[level, lit, stone, V::I(4)]);
            let ticks = level_call(rt, "getBlockTicks.()Lnet/minecraft/world/ticks/LevelTickAccess;", &[level]);
            const QUEUED: &str = "net/minecraft/world/ticks/LevelTickAccess.hasScheduledTick.(Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Z";
            const THIS_TICK: &str = "net/minecraft/world/ticks/LevelTickAccess.willTickThisTick.(Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Z";
            // Queued there for stone, not for dirt; neither about to run in this game tick.
            let dirt = crate::api::get_static(rt, "net/minecraft/world/level/block/Blocks.DIRT");
            assert_eq!(crate::api::call(rt, QUEUED, &[ticks, lit, stone]).i(), 1);
            assert_eq!(crate::api::call(rt, QUEUED, &[ticks, lit, dirt]).i(), 0);
            assert_eq!(crate::api::call(rt, QUEUED, &[ticks, dim, stone]).i(), 0);
            assert_eq!(crate::api::call(rt, THIS_TICK, &[ticks, lit, stone]).i(), 0);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(host.scheduled, [([2, 64, 2], "minecraft:stone".to_owned(), 4)]);
    }

    /// Create's nixie tube asks each neighbour what it sends it (`getSignal(pos.relative(d), d)`):
    /// a redstone block east of it sends 15 that way only, and none of it strong; a lever on top of
    /// another block sends that block a strong 15, which `getDirectSignalTo` finds. A client's
    /// world sends nothing.
    #[test]
    fn what_a_block_sends_one_way_comes_from_the_world() {
        const SIGNAL: &str = "getSignal.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)I";
        const HAS: &str = "hasSignal.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Z";
        const DIRECT: &str = "getDirectSignal.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)I";
        const DIRECT_TO: &str = "getDirectSignalTo.(Lnet/minecraft/core/BlockPos;)I";
        let (tube, redstone, block, lever) = ([0, 64, 0], [1, 64, 0], [0, 64, 5], [0, 65, 5]);
        let mut host = MemoryHost::default();
        host.sent_signals.insert((redstone, 5), (15, 0));
        host.sent_signals.insert((lever, 1), (15, 15));
        let mut client = MemoryHost { client: true, sent_signals: host.sent_signals.clone(), ..MemoryHost::default() };
        let ask = |host: &mut MemoryHost| {
            with_host(host, |rt| {
                let level = rt.new_object(LEVEL);
                let at = |rt: &mut Rt, p: [i32; 3]| crate::api::block_pos(rt, p[0].into(), p[1].into(), p[2].into());
                let (tube, redstone, block, lever) = (at(rt, tube), at(rt, redstone), at(rt, block), at(rt, lever));
                let (up, west, east) = (crate::api::direction(rt, 1), crate::api::direction(rt, 4), crate::api::direction(rt, 5));
                let answers = [
                    level_call(rt, SIGNAL, &[level, redstone, east]).i(),
                    level_call(rt, SIGNAL, &[level, redstone, west]).i(),
                    level_call(rt, HAS, &[level, redstone, east]).i(),
                    level_call(rt, HAS, &[level, redstone, west]).i(),
                    level_call(rt, DIRECT, &[level, redstone, east]).i(),
                    level_call(rt, DIRECT, &[level, lever, up]).i(),
                    level_call(rt, DIRECT_TO, &[level, tube]).i(),
                    level_call(rt, DIRECT_TO, &[level, block]).i(),
                ];
                assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
                answers
            })
        };
        assert_eq!(ask(&mut host), [15, 0, 1, 0, 0, 15, 0, 15]);
        assert_eq!(ask(&mut client), [0; 8]);
    }

    /// `BlockInWorld` reads its level's block once, when first asked, as 1.21.1's caches it (a
    /// clone command reads each block of its area so), and its position is its own copy;
    /// `Clearable.tryClear` empties a container and leaves anything else.
    #[test]
    fn a_block_in_world_reads_its_level_once() {
        const BIW: &str = "net/minecraft/world/level/block/state/pattern/BlockInWorld";
        let mut host = MemoryHost::default();
        host.blocks.insert([1, 70, 2], "minecraft:glowstone".to_owned());
        with_host(&mut host, |rt| {
            let level = rt.new_object(LEVEL);
            let pos = rt.alloc("net/minecraft/core/BlockPos$MutableBlockPos", crate::Data::Native("BlockPos", [1, 70, 2, 0]));
            let biw = rt.new_object(BIW);
            crate::api::call(rt, "net/minecraft/world/level/block/state/pattern/BlockInWorld.<init>.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Z)V", &[biw, level, pos, V::I(0)]);
            // The position given moves on; the block's stays.
            crate::api::call(rt, "net/minecraft/core/BlockPos$MutableBlockPos.set.(III)Lnet/minecraft/core/BlockPos$MutableBlockPos;", &[pos, V::I(9), V::I(9), V::I(9)]);
            let get_state = "net/minecraft/world/level/block/state/pattern/BlockInWorld.getState.()Lnet/minecraft/world/level/block/state/BlockState;";
            let state = crate::api::call(rt, get_state, &[biw]);
            assert_eq!(rt.str_of(state), Some("minecraft:glowstone"));
            rt.host_mut().unwrap().set_block_state([1, 70, 2], "minecraft:stone");
            assert_eq!(crate::api::call(rt, get_state, &[biw]), state, "read once");
            let own = crate::api::call(rt, "net/minecraft/world/level/block/state/pattern/BlockInWorld.getPos.()Lnet/minecraft/core/BlockPos;", &[biw]);
            assert_eq!(crate::api::pos_of(rt, own), [1, 70, 2]);
            let entity = crate::api::call(rt, "net/minecraft/world/level/block/state/pattern/BlockInWorld.getEntity.()Lnet/minecraft/world/level/block/entity/BlockEntity;", &[biw]);
            assert_eq!(entity, V::NULL);
            // A container empties; a string is no container.
            let chest = rt.new_object("net/minecraft/world/SimpleContainer");
            crate::api::call(rt, "net/minecraft/world/SimpleContainer.<init>.(I)V", &[chest, V::I(3)]);
            let item = crate::api::get_static(rt, "net/minecraft/world/item/Items.DIAMOND");
            let stack = rt.new_object("net/minecraft/world/item/ItemStack");
            crate::api::call(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;I)V", &[stack, item, V::I(5)]);
            crate::api::call(rt, "net/minecraft/world/SimpleContainer.setItem.(ILnet/minecraft/world/item/ItemStack;)V", &[chest, V::I(1), stack]);
            let is_empty = "net/minecraft/world/SimpleContainer.isEmpty.()Z";
            assert_eq!(crate::api::call(rt, is_empty, &[chest]).i(), 0);
            let try_clear = "net/minecraft/world/Clearable.tryClear.(Ljava/lang/Object;)V";
            crate::api::call(rt, try_clear, &[chest]);
            assert_eq!(crate::api::call(rt, is_empty, &[chest]).i(), 1);
            let text = rt.string("not a container");
            crate::api::call(rt, try_clear, &[text]);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }
}
