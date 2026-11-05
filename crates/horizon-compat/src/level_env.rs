//! What a level answers about a block's surroundings, from the host's world: light, the time of
//! day and the sky, the weather, redstone reaching a block, and its scheduled ticks
//! (`Level.getBrightness`, `getSkyDarken`, `isDay`, `getRainLevel`, `hasNeighborSignal`,
//! `scheduleTick`...). A solar panel, a lamp, a clutch or a redstone-driven machine reads these.
//! The sky follows 1.21.1's formulas (`DimensionType.timeOfDay`, `Level.updateSkyBrightness`).
use crate::{Rt, V};

pub(crate) const LIGHT_LAYER: &str = "net/minecraft/world/level/LightLayer";
const LEVEL_TICKS: &str = "net/minecraft/world/ticks/LevelTicks";

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
fn raw_brightness(rt: &Rt, pos: [i32; 3], darkening: i32) -> i32 {
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
        // `SignalGetter`: the strongest signal any neighbour sends it.
        ("hasNeighborSignal", _) => {
            let p = pos(rt, 1);
            V::I((rt.host_mut().map_or(0, |h| h.neighbor_signal(p)) > 0) as i32)
        }
        ("getBestNeighborSignal", _) => {
            let p = pos(rt, 1);
            V::I(rt.host_mut().map_or(0, |h| h.neighbor_signal(p)).clamp(0, 15))
        }
        // `scheduleTick(pos, block, delay[, priority])`; a fluid's ticks are the game's own.
        ("scheduleTick", d) if d.starts_with("(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;I") => {
            let (p, block) = (pos(rt, 1), crate::api::block_name(rt, a(2)));
            if !block.is_empty()
                && let Some(h) = rt.host_mut()
            {
                h.schedule_block_tick(p, &block, a(3).i());
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

/// `LevelTicks`/`LevelTickAccess` (what `getBlockTicks` gives): nothing reads as queued, so a mod
/// schedules again, which keeps the tick already there.
pub(crate) fn ticks_call(rt: &mut Rt, name: &str) -> Option<V> {
    let _ = rt;
    match name {
        "hasScheduledTick" | "willTickThisTick" => Some(V::I(0)),
        "schedule" | "clearArea" => Some(V::Void),
        "count" => Some(V::I(0)),
        _ => None,
    }
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
            let queued = crate::api::call(rt, "net/minecraft/world/ticks/LevelTickAccess.hasScheduledTick.(Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Z", &[ticks, lit, stone]);
            assert_eq!(queued.i(), 0);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(host.scheduled, [([2, 64, 2], "minecraft:stone".to_owned(), 4)]);
    }
}
