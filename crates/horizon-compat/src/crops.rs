//! Crops as NeoForge 1.21.1 grows them, for a mod's crop blocks and for the growth code mods write
//! themselves:
//!
//! - `CropBlock`'s own methods, for a mod crop that inherits them ([`random_tick`]: Farmer's
//!   Delight's cabbages and onions inherit all of it; its rice panicles count their age to 3 with
//!   their own property). On a random tick in enough light, a crop short of its last age grows one
//!   on a draw of 1 in `25 / speed + 1`; the speed is better on wet farmland and worse in rows. Each
//!   method calls the block's own overrides (`getAgeProperty`, `getMaxAge`, `getStateForAge`...)
//!   where its class has them, as the JVM dispatches. So does bone meal's part (`growCrops`).
//! - NeoForge's hooks around a growth (`CommonHooks.canCropGrow`, `fireCropGrowPost`): they post
//!   `CropGrowEvent.Pre`, whose listeners may force or forbid the growth, and `CropGrowEvent.Post`.
//!   With no listener, `canCropGrow` is the draw it was given.
//! - The soil's say in it (`canSustainPlant`, `isFertile`): a mod soil's own answer, else NeoForge's
//!   defaults (no decision; a farmland block is fertile while wet).
use crate::{api, Rt, V};

const CROP: &str = "net/minecraft/world/level/block/CropBlock";
/// The vanilla crops whose growth is `CropBlock`'s as it is (a beetroot skips two ticks in three).
const PLAIN_CROPS: [&str; 3] = [CROP, "net/minecraft/world/level/block/CarrotBlock", "net/minecraft/world/level/block/PotatoBlock"];
const FARM: &str = "net/minecraft/world/level/block/FarmBlock";
const HOOKS: &str = "net/neoforged/neoforge/common/CommonHooks";
const GROW_EVENT: &str = "net/neoforged/neoforge/event/level/block/CropGrowEvent";
const PRE: &str = "net/neoforged/neoforge/event/level/block/CropGrowEvent$Pre";
const POST: &str = "net/neoforged/neoforge/event/level/block/CropGrowEvent$Post";
const PRE_RESULT: &str = "net/neoforged/neoforge/event/level/block/CropGrowEvent$Pre$Result";
const BLOCK_EVENT: &str = "net/neoforged/neoforge/event/level/BlockEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
const EXTENSIONS: &str = "net/neoforged/neoforge/common/extensions/";
const BLOCKS: &str = "net/minecraft/world/level/block/";

// `CropBlock`'s methods a crop's own may replace: (the key `vcall` falls back to, the method).
const AGE_PROPERTY: (&str, &str) = (
    "net/minecraft/world/level/block/CropBlock.getAgeProperty.()Lnet/minecraft/world/level/block/state/properties/IntegerProperty;",
    "getAgeProperty.()Lnet/minecraft/world/level/block/state/properties/IntegerProperty;",
);
const MAX_AGE: (&str, &str) = ("net/minecraft/world/level/block/CropBlock.getMaxAge.()I", "getMaxAge.()I");
const GET_AGE: (&str, &str) = (
    "net/minecraft/world/level/block/CropBlock.getAge.(Lnet/minecraft/world/level/block/state/BlockState;)I",
    "getAge.(Lnet/minecraft/world/level/block/state/BlockState;)I",
);
const STATE_FOR_AGE: (&str, &str) = (
    "net/minecraft/world/level/block/CropBlock.getStateForAge.(I)Lnet/minecraft/world/level/block/state/BlockState;",
    "getStateForAge.(I)Lnet/minecraft/world/level/block/state/BlockState;",
);
const BONEMEAL_INCREASE: (&str, &str) = (
    "net/minecraft/world/level/block/CropBlock.getBonemealAgeIncrease.(Lnet/minecraft/world/level/Level;)I",
    "getBonemealAgeIncrease.(Lnet/minecraft/world/level/Level;)I",
);
// A soil's own say, on its block (`IBlockExtension`'s).
pub(crate) const SUSTAIN: (&str, &str) = (
    "net/neoforged/neoforge/common/extensions/IBlockExtension.canSustainPlant.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/neoforged/neoforge/common/util/TriState;",
    "canSustainPlant.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/neoforged/neoforge/common/util/TriState;",
);
const FERTILE: (&str, &str) = (
    "net/neoforged/neoforge/common/extensions/IBlockExtension.isFertile.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z",
    "isFertile.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z",
);
const GET_VALUE: &str = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
const SET_VALUE: &str = "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;";
const DEFAULT_STATE: &str = "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
const INT_VALUE: &str = "java/lang/Integer.intValue.()I";
const VALUE_OF: &str = "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;";
const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const MTH_NEXT_INT: &str = "net/minecraft/util/Mth.nextInt.(Lnet/minecraft/util/RandomSource;II)I";

/// The events' classes above them, so a listener for `CropGrowEvent` (or `BlockEvent`) hears them.
const SUPERS: [(&str, &str); 4] = [(PRE, GROW_EVENT), (POST, GROW_EVENT), (GROW_EVENT, BLOCK_EVENT), (BLOCK_EVENT, EVENT)];

fn know_classes(rt: &mut Rt) {
    if !rt.supers.contains_key(PRE) {
        for (class, sup) in SUPERS {
            rt.supers.insert(class, sup);
        }
    }
}

/// `this.method(args)` as the JVM dispatches it: the mod class's own where it has one, else the
/// API's (`CropBlock`'s here).
fn dispatch(rt: &mut Rt, (owner_key, key): (&'static str, &'static str), args: &[V]) -> V {
    match rt.vcall {
        Some(vc) => vc(rt, owner_key, key, args),
        None => api::call(rt, owner_key, args),
    }
}

fn int_of(rt: &mut Rt, v: V) -> i32 {
    match v {
        V::R(_) => api::call(rt, INT_VALUE, &[v]).i(),
        raw => raw.i(),
    }
}

/// Whether a mod block of class `class` grows as `CropBlock` does: the first vanilla class above
/// it is one whose `randomTick` is `CropBlock`'s.
pub(crate) fn grows_as_crop(rt: &Rt, class: &'static str) -> bool {
    PLAIN_CROPS.contains(&crate::rotation::vanilla_base(rt, class))
}

/// `CropBlock.randomTick(state, level, pos, random)` (NeoForge's) for the crop `this`.
pub fn random_tick(rt: &mut Rt, this: V, state: V, level: V, pos: V, random: V) {
    let p = api::pos_of(rt, pos);
    if crate::level_env::raw_brightness(rt, p, 0) < 9 {
        return;
    }
    let age = dispatch(rt, GET_AGE, &[this, state]).i();
    let max = dispatch(rt, MAX_AGE, &[this]).i();
    if rt.thrown.is_some() || age >= max {
        return;
    }
    let speed = growth_speed(rt, state, level, pos);
    let bound = (25.0 / speed) as i32 + 1;
    let lucky = api::call(rt, NEXT_INT, &[random, V::I(bound)]).i() == 0;
    if can_crop_grow(rt, level, pos, state, lucky) {
        let next = dispatch(rt, STATE_FOR_AGE, &[this, V::I(age + 1)]);
        if rt.thrown.is_some() {
            return;
        }
        set_block(rt, p, next);
        fire_crop_grow_post(rt, level, pos, state);
    }
}

fn set_block(rt: &mut Rt, p: [i32; 3], state: V) {
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    if let Some(h) = rt.host_mut() {
        h.set_block_state(p, &text);
    }
}

fn block_at(rt: &Rt, p: [i32; 3]) -> String {
    rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p))
}

/// `CropBlock.getGrowthSpeed(state, level, pos)` (NeoForge's): 1, plus for each of the nine blocks
/// under the crop and around that one, 1 where it sustains the crop (the soil's own say, else being
/// farmland) and 3 where it is also fertile, a quarter of that off the middle; halved where the same
/// crop stands beside it on both axes, or on a diagonal.
fn growth_speed(rt: &mut Rt, state: V, level: V, pos: V) -> f32 {
    let p = api::pos_of(rt, pos);
    let text = rt.str_of(state).unwrap_or("").to_owned();
    let (block, _) = crate::host::split_state(&text);
    let block = block.to_owned();
    let mut speed = 1.0f32;
    for i in -1..=1 {
        for j in -1..=1 {
            let soil_pos = [p[0] + i, p[1] - 1, p[2] + j];
            let soil = block_at(rt, soil_pos);
            let sustains = match sustain(rt, &soil, level, soil_pos, state).as_str() {
                "TRUE" => true,
                "FALSE" => false,
                _ => is_farm_block(rt, &soil),
            };
            let mut f = 0.0f32;
            if sustains {
                f = 1.0;
                if fertile(rt, &soil, level, [p[0] + i, p[1], p[2] + j]) {
                    f = 3.0;
                }
            }
            if i != 0 || j != 0 {
                f /= 4.0;
            }
            speed += f;
        }
    }
    let is = |rt: &Rt, dx: i32, dz: i32| crate::host::split_state(&block_at(rt, [p[0] + dx, p[1], p[2] + dz])).0 == block;
    let east_west = is(rt, -1, 0) || is(rt, 1, 0);
    let north_south = is(rt, 0, -1) || is(rt, 0, 1);
    if east_west && north_south {
        speed /= 2.0;
    } else if is(rt, -1, -1) || is(rt, 1, -1) || is(rt, 1, 1) || is(rt, -1, 1) {
        speed /= 2.0;
    }
    speed
}

/// Whether the block in state `text` is a `FarmBlock`: farmland, or a mod block of a class below it.
fn is_farm_block(rt: &Rt, text: &str) -> bool {
    let (block, _) = crate::host::split_state(text);
    block == "minecraft:farmland" || crate::glue::mod_block(rt, text).is_some_and(|(_, class)| crate::neo::reaches(rt, class, FARM))
}

/// `soil.canSustainPlant(level, soilPos, UP, plant)`'s constant (`TRUE`, `DEFAULT`, `FALSE`): a mod
/// soil's own answer, else NeoForge's default (none of vanilla's soils decide).
fn sustain(rt: &mut Rt, soil: &str, level: V, soil_pos: [i32; 3], plant: V) -> String {
    let Some((block, class)) = crate::glue::mod_block(rt, soil) else { return "DEFAULT".into() };
    if !rt.overrides.is_some_and(|o| o(class, SUSTAIN.1)) {
        return "DEFAULT".into();
    }
    let s = api::state(rt, soil);
    let at = api::block_pos(rt, soil_pos[0].into(), soil_pos[1].into(), soil_pos[2].into());
    let up = api::direction(rt, 1);
    let decision = dispatch(rt, SUSTAIN, &[block, s, level, at, up, plant]);
    if decision.r() == 0 {
        return "DEFAULT".into();
    }
    rt.str_of(rt.get_field(decision, "$name")).unwrap_or("DEFAULT").to_owned()
}

/// `soil.isFertile(level, pos)`: a mod soil's own answer, else NeoForge's default (a `FarmBlock`
/// while its moisture is above 0).
fn fertile(rt: &mut Rt, soil: &str, level: V, at: [i32; 3]) -> bool {
    if let Some((block, class)) = crate::glue::mod_block(rt, soil)
        && rt.overrides.is_some_and(|o| o(class, FERTILE.1))
    {
        let s = api::state(rt, soil);
        let p = api::block_pos(rt, at[0].into(), at[1].into(), at[2].into());
        return dispatch(rt, FERTILE, &[block, s, level, p]).i() != 0;
    }
    default_fertile(rt, soil)
}

fn default_fertile(rt: &Rt, soil: &str) -> bool {
    let (_, props) = crate::host::split_state(soil);
    is_farm_block(rt, soil) && props.iter().find(|(k, _)| *k == "moisture").is_some_and(|(_, v)| v.parse::<i32>().unwrap_or(0) > 0)
}

/// `CommonHooks.canCropGrow(level, pos, state, def)`: posts `CropGrowEvent.Pre`; a listener's
/// `GROW` or `DO_NOT_GROW` decides, else `def` (the crop's own draw).
fn can_crop_grow(rt: &mut Rt, level: V, pos: V, state: V, def: bool) -> bool {
    know_classes(rt);
    if crate::neo::hearing(rt, PRE).is_empty() {
        return def;
    }
    let ev = block_event(rt, PRE, level, pos, state);
    let default = api::api_enum(rt, PRE_RESULT, "DEFAULT", 1);
    rt.put_field(ev, "$result", default);
    crate::neo::post(rt, PRE, ev);
    let result = rt.get_field(ev, "$result");
    match rt.str_of(rt.get_field(result, "$name")) {
        Some("GROW") => true,
        Some("DO_NOT_GROW") => false,
        _ => def,
    }
}

/// `CommonHooks.fireCropGrowPost(level, pos, original)`: posts `CropGrowEvent.Post` with the
/// block's state now.
fn fire_crop_grow_post(rt: &mut Rt, level: V, pos: V, original: V) {
    know_classes(rt);
    if crate::neo::hearing(rt, POST).is_empty() {
        return;
    }
    let p = api::pos_of(rt, pos);
    let now = block_at(rt, p);
    let now = api::state(rt, &now);
    let ev = block_event(rt, POST, level, pos, now);
    rt.put_field(ev, "$originalState", original);
    crate::neo::post(rt, POST, ev);
}

/// A new `BlockEvent` of `class` (its level, position and state).
fn block_event(rt: &mut Rt, class: &'static str, level: V, pos: V, state: V) -> V {
    let ev = rt.new_object(class);
    for (field, v) in [("$level", level), ("$pos", pos), ("$state", state), ("$canceled", V::I(0))] {
        rt.put_field(ev, field, v);
    }
    ev
}

/// `CropBlock.growCrops(level, pos, state)` (bone meal's): the age plus the crop's bone meal
/// increase, at most its last.
fn grow_crops(rt: &mut Rt, this: V, level: V, pos: V, state: V) {
    let age = dispatch(rt, GET_AGE, &[this, state]).i() + dispatch(rt, BONEMEAL_INCREASE, &[this, level]).i();
    let age = age.min(dispatch(rt, MAX_AGE, &[this]).i());
    let next = dispatch(rt, STATE_FOR_AGE, &[this, V::I(age)]);
    if rt.thrown.is_none() {
        let p = api::pos_of(rt, pos);
        set_block(rt, p, next);
    }
}

pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    (owner == CROP
        && matches!(
            name,
            "getAgeProperty"
                | "getMaxAge"
                | "getAge"
                | "getStateForAge"
                | "isMaxAge"
                | "isRandomlyTicking"
                | "randomTick"
                | "getGrowthSpeed"
                | "hasSufficientLight"
                | "growCrops"
                | "getBonemealAgeIncrease"
                | "isValidBonemealTarget"
                | "isBonemealSuccess"
                | "performBonemeal"
        ))
        || (owner == HOOKS && matches!(name, "canCropGrow" | "fireCropGrowPost"))
        || owner.starts_with(GROW_EVENT)
        || (owner == BLOCK_EVENT && matches!(name, "getLevel" | "getPos" | "getState"))
        || (matches!(name, "canSustainPlant" | "isFertile") && (owner.starts_with(BLOCKS) || owner.starts_with(EXTENSIONS)))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // `CropBlock`'s own age: `AGE_7`, up to 7 (a mod crop's `super.getAgeProperty()`).
        (CROP, "getAgeProperty") => crate::api_common::vanilla_property_object(rt, "AGE_7")?,
        (CROP, "getMaxAge") => V::I(7),
        // `state.getValue(this.getAgeProperty())`.
        (CROP, "getAge") => {
            let property = dispatch(rt, AGE_PROPERTY, &[a(0)]);
            let v = api::call(rt, GET_VALUE, &[a(1), property]);
            V::I(int_of(rt, v))
        }
        // `this.defaultBlockState().setValue(this.getAgeProperty(), age)`.
        (CROP, "getStateForAge") => {
            let default = api::call(rt, DEFAULT_STATE, &[a(0)]);
            let property = dispatch(rt, AGE_PROPERTY, &[a(0)]);
            let age = api::call(rt, VALUE_OF, &[a(1)]);
            api::call(rt, SET_VALUE, &[default, property, age])
        }
        (CROP, "isMaxAge") => V::I((dispatch(rt, GET_AGE, &[a(0), a(1)]).i() >= dispatch(rt, MAX_AGE, &[a(0)]).i()) as i32),
        (CROP, "isRandomlyTicking") | (CROP, "isValidBonemealTarget") => {
            // `!isMaxAge(state)`; the bone meal target's state is its third argument.
            let state = if name == "isRandomlyTicking" { a(1) } else { a(3) };
            V::I((dispatch(rt, GET_AGE, &[a(0), state]).i() < dispatch(rt, MAX_AGE, &[a(0)]).i()) as i32)
        }
        (CROP, "randomTick") => {
            random_tick(rt, a(0), a(1), a(2), a(3), a(4));
            V::Void
        }
        // Static: NeoForge's takes the crop's state; vanilla's its block (whose default state then
        // stands for it).
        (CROP, "getGrowthSpeed") => {
            let state = if desc.starts_with("(Lnet/minecraft/world/level/block/Block;") { api::call(rt, DEFAULT_STATE, &[a(0)]) } else { a(0) };
            V::F(growth_speed(rt, state, a(1), a(2)))
        }
        (CROP, "hasSufficientLight") => {
            let p = api::pos_of(rt, a(1));
            V::I((crate::level_env::raw_brightness(rt, p, 0) >= 8) as i32)
        }
        (CROP, "growCrops") => {
            grow_crops(rt, a(0), a(1), a(2), a(3));
            V::Void
        }
        // `Mth.nextInt(level.random, 2, 5)`.
        (CROP, "getBonemealAgeIncrease") => api::call(rt, MTH_NEXT_INT, &[V::NULL, V::I(2), V::I(5)]),
        (CROP, "isBonemealSuccess") => V::I(1),
        (CROP, "performBonemeal") => {
            grow_crops(rt, a(0), a(1), a(3), a(4));
            V::Void
        }
        (HOOKS, "canCropGrow") => V::I(can_crop_grow(rt, a(0), a(1), a(2), a(3).i() != 0) as i32),
        (HOOKS, "fireCropGrowPost") => {
            fire_crop_grow_post(rt, a(0), a(1), a(2));
            V::Void
        }
        (o, "getLevel") if o.starts_with(GROW_EVENT) || o == BLOCK_EVENT => rt.get_field(a(0), "$level"),
        (o, "getPos") if o.starts_with(GROW_EVENT) || o == BLOCK_EVENT => rt.get_field(a(0), "$pos"),
        (o, "getState") if o.starts_with(GROW_EVENT) || o == BLOCK_EVENT => rt.get_field(a(0), "$state"),
        (POST, "getOriginalState") => rt.get_field(a(0), "$originalState"),
        (PRE, "getResult") => rt.get_field(a(0), "$result"),
        (PRE, "setResult") => {
            rt.put_field(a(0), "$result", a(1));
            V::Void
        }
        // `state.canSustainPlant(level, soilPos, facing, plant)` on a state, or the block's
        // (`canSustainPlant(state, level, soilPos, facing, plant)`): a mod soil's own answer, else
        // no decision.
        (o, "canSustainPlant") if o.starts_with(BLOCKS) || o.starts_with(EXTENSIONS) => {
            let on_block = desc.starts_with("(Lnet/minecraft/world/level/block/state/BlockState;");
            let (state, level, soil_pos, plant) = if on_block { (a(1), a(2), a(3), a(5)) } else { (a(0), a(1), a(2), a(4)) };
            let soil = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
            let at = api::pos_of(rt, soil_pos);
            // A mod soil's `super.canSustainPlant(...)` is vanilla's: no decision.
            let decision = if on_block && owner.starts_with(BLOCKS) { "DEFAULT".to_owned() } else { sustain(rt, &soil, level, at, plant) };
            let (name, ordinal) = match decision.as_str() {
                "TRUE" => ("TRUE", 0),
                "FALSE" => ("FALSE", 2),
                _ => ("DEFAULT", 1),
            };
            api::api_enum(rt, "net/neoforged/neoforge/common/util/TriState", name, ordinal)
        }
        // `state.isFertile(level, pos)`, or the block's `isFertile(state, level, pos)`.
        (o, "isFertile") if o.starts_with(BLOCKS) || o.starts_with(EXTENSIONS) => {
            let on_block = desc.starts_with("(Lnet/minecraft/world/level/block/state/BlockState;");
            let (state, level, at) = if on_block { (a(1), a(2), a(3)) } else { (a(0), a(1), a(2)) };
            let soil = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
            let p = api::pos_of(rt, at);
            V::I(if on_block && owner.starts_with(BLOCKS) { default_fertile(rt, &soil) } else { fertile(rt, &soil, level, p) } as i32)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const CABBAGE: &str = "test/CabbageBlock";
    const PANICLES: &str = "test/RicePaniclesBlock";
    const SOIL: &str = "test/RichSoilFarmlandBlock";

    /// The mod classes' own methods: the panicles count to 3 with `AGE_3`; the rich soil sustains
    /// every plant and is always fertile.
    fn overrides(class: &str, key: &str) -> bool {
        (class == PANICLES && (key == AGE_PROPERTY.1 || key == MAX_AGE.1)) || (class == SOIL && (key == SUSTAIN.1 || key == FERTILE.1))
    }
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let class = rt.class_of(args[0]);
        match key {
            k if k == AGE_PROPERTY.1 && class == PANICLES => crate::api_common::vanilla_property_object(rt, "AGE_3").unwrap(),
            k if k == MAX_AGE.1 && class == PANICLES => V::I(3),
            k if k == SUSTAIN.1 && class == SOIL => api::api_enum(rt, "net/neoforged/neoforge/common/util/TriState", "TRUE", 0),
            k if k == FERTILE.1 && class == SOIL => V::I(1),
            _ => api::call(rt, owner_key, args),
        }
    }

    fn runtime() -> Rt {
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(CABBAGE, CROP);
        rt.supers.insert(PANICLES, CROP);
        rt.supers.insert(SOIL, FARM);
        for (class, id) in [(CABBAGE, "test:cabbages"), (PANICLES, "test:rice_panicles"), (SOIL, "test:rich_soil_farmland")] {
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), id.into()), b);
            // Its default state's properties, as its constructor registered them.
            let d = rt.string(if class == SOIL { "moisture=0" } else { "age=0" });
            rt.put_field(b, "$default", d);
        }
        rt
    }

    /// A crop at (0, 1, 0) on `soil`, the rest of the nine below it dirt.
    fn field(crop: &str, soil: &str) -> MemoryHost {
        let mut host = MemoryHost::default();
        for x in -1..=1 {
            for z in -1..=1 {
                host.blocks.insert([x, 0, z], "minecraft:dirt".into());
            }
        }
        host.blocks.insert([0, 0, 0], soil.into());
        host.blocks.insert([0, 1, 0], crop.into());
        host
    }

    fn block(rt: &Rt, id: &str) -> V {
        rt.registered[&("block".to_owned(), id.to_owned())]
    }

    /// Random ticks until the crop at (0, 1, 0) changes, at most `ticks`: the ticks it took.
    fn grow(rt: &mut Rt, host: &mut MemoryHost, id: &str, ticks: u32) -> Option<u32> {
        let b = block(rt, id);
        let before = host.blocks[&[0, 1, 0]].clone();
        for n in 1..=ticks {
            let now = host.blocks[&[0, 1, 0]].clone();
            rt.with_host(host, |rt| {
                let state = api::state(rt, &now);
                let level = rt.new_object("net/minecraft/server/level/ServerLevel");
                let pos = api::block_pos(rt, 0, 1, 0);
                let random = rt.new_object("net/minecraft/util/RandomSource");
                random_tick(rt, b, state, level, pos, random);
            });
            if host.blocks[&[0, 1, 0]] != before {
                return Some(n);
            }
        }
        None
    }

    /// A mod crop without a `randomTick` of its own grows as `CropBlock` does: one age at a time,
    /// in enough light only, by its own age property and up to its own last age (on wet farmland
    /// with dirt around, a draw of 1 in `25 / 4 + 1`, 7).
    #[test]
    fn a_mod_crop_without_a_random_tick_grows_as_vanillas() {
        let mut rt = runtime();
        // Wet farmland: one age up, the rest of the state as it was.
        let mut wet = field("test:cabbages[age=0]", "minecraft:farmland[moisture=7]");
        assert!(grow(&mut rt, &mut wet, "test:cabbages", 200).is_some());
        assert_eq!(wet.blocks[&[0, 1, 0]], "test:cabbages[age=1]");
        // At its last age it stays.
        let mut ripe = field("test:cabbages[age=7]", "minecraft:farmland[moisture=7]");
        assert_eq!(grow(&mut rt, &mut ripe, "test:cabbages", 300), None);
        // The rice panicles' own age: 0 to 3 (`AGE_3`), and no further.
        let mut panicles = field("test:rice_panicles[age=2]", "minecraft:farmland[moisture=7]");
        assert!(grow(&mut rt, &mut panicles, "test:rice_panicles", 200).is_some());
        assert_eq!(panicles.blocks[&[0, 1, 0]], "test:rice_panicles[age=3]");
        assert_eq!(grow(&mut rt, &mut panicles, "test:rice_panicles", 300), None);
        // In the dark (sky light 0, block light 8 at the crop), nothing grows.
        let mut dark = field("test:cabbages[age=0]", "minecraft:farmland[moisture=7]");
        dark.light.insert([0, 1, 0], (8, 0));
        assert_eq!(grow(&mut rt, &mut dark, "test:cabbages", 300), None);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The growth speed, as NeoForge's `CropBlock.getGrowthSpeed` adds it up: 1, then 3 for the wet
    /// farmland right below (1 when dry), a quarter of 1 or 3 for each farmland around it; a mod
    /// soil that sustains the crop and is fertile counts 3 though it isn't wet; halved in rows on
    /// both axes or with one on a diagonal.
    #[test]
    fn the_growth_speed_counts_the_soil_and_the_rows() {
        let mut rt = runtime();
        let speed = |rt: &mut Rt, host: &mut MemoryHost| {
            rt.with_host(host, |rt| {
                let state = api::state(rt, "test:cabbages[age=0]");
                let level = rt.new_object("net/minecraft/server/level/ServerLevel");
                let pos = api::block_pos(rt, 0, 1, 0);
                api::call(
                    rt,
                    "net/minecraft/world/level/block/CropBlock.getGrowthSpeed.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F",
                    &[state, level, pos],
                )
                .f()
            })
        };
        assert_eq!(speed(&mut rt, &mut field("test:cabbages", "minecraft:farmland[moisture=7]")), 4.0);
        assert_eq!(speed(&mut rt, &mut field("test:cabbages", "minecraft:farmland[moisture=0]")), 2.0);
        assert_eq!(speed(&mut rt, &mut field("test:cabbages", "minecraft:dirt")), 1.0);
        assert_eq!(speed(&mut rt, &mut field("test:cabbages", "test:rich_soil_farmland[moisture=0]")), 4.0);
        // A whole field of wet farmland: 1 + 3 + 8 * 3 / 4.
        let mut wet_field = field("test:cabbages", "minecraft:farmland[moisture=7]");
        for x in -1..=1 {
            for z in -1..=1 {
                wet_field.blocks.insert([x, 0, z], "minecraft:farmland[moisture=7]".into());
            }
        }
        assert_eq!(speed(&mut rt, &mut wet_field), 10.0);
        // The same crop to the east and to the north: rows on both axes, halved; one to the east
        // alone is a row on one axis, not halved; one on a diagonal is.
        wet_field.blocks.insert([1, 1, 0], "test:cabbages[age=3]".into());
        assert_eq!(speed(&mut rt, &mut wet_field), 10.0);
        wet_field.blocks.insert([0, 1, -1], "test:cabbages[age=0]".into());
        assert_eq!(speed(&mut rt, &mut wet_field), 5.0);
        wet_field.blocks.remove(&[1, 1, 0]);
        wet_field.blocks.remove(&[0, 1, -1]);
        wet_field.blocks.insert([1, 1, 1], "test:cabbages[age=0]".into());
        assert_eq!(speed(&mut rt, &mut wet_field), 5.0);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own growth code (Farmer's Delight's budding tomatoes) asks NeoForge whether the crop
    /// may grow (`CommonHooks.canCropGrow`): with no listener, its own draw decides; a listener of
    /// `CropGrowEvent.Pre` (or of any `BlockEvent`) hears the crop's level, position and state and
    /// may force or forbid the growth. It asks the soil too: vanilla's soils make no decision, and
    /// farmland is fertile while wet.
    #[test]
    fn a_mods_own_growth_asks_neoforges_hooks_and_the_soil() {
        const CAN_GROW: &str = "net/neoforged/neoforge/common/CommonHooks.canCropGrow.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)Z";
        const SUSTAINS: &str = "net/minecraft/world/level/block/state/BlockState.canSustainPlant.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/neoforged/neoforge/common/util/TriState;";
        const IS_FERTILE: &str = "net/minecraft/world/level/block/state/BlockState.isFertile.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
        let mut rt = runtime();
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let pos = api::block_pos(&mut rt, 0, 1, 0);
        let crop = api::state(&mut rt, "test:cabbages[age=0]");
        let grows = |rt: &mut Rt, def: bool| api::call(rt, CAN_GROW, &[level, pos, crop, V::I(def as i32)]).i() != 0;
        assert!(grows(&mut rt, true));
        assert!(!grows(&mut rt, false));
        // A listener that forbids the growth, heard through `BlockEvent`: it keeps the state it heard.
        fn forbid(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            let (seen, ev) = (rt.captured(args[0])[0], args[1]);
            let state = api::call(rt, "net/neoforged/neoforge/event/level/BlockEvent.getState.()Lnet/minecraft/world/level/block/state/BlockState;", &[ev]);
            rt.put_field(seen, "state", state);
            let no = api::api_enum(rt, PRE_RESULT, "DO_NOT_GROW", 2);
            api::call(rt, "net/neoforged/neoforge/event/level/block/CropGrowEvent$Pre.setResult.(Lnet/neoforged/neoforge/event/level/block/CropGrowEvent$Pre$Result;)V", &[ev, no]);
            Some(V::Void)
        }
        rt.lambda_call = Some(forbid);
        let seen = rt.new_object("java/lang/Object");
        let listener = rt.lambda_id("java/util/function/Consumer", 1, vec![seen]);
        rt.listeners.push((BLOCK_EVENT.to_owned(), listener));
        assert!(!grows(&mut rt, true));
        let heard = rt.get_field(seen, "state");
        assert_eq!(rt.str_of(heard), Some("test:cabbages[age=0]"));
        // The soil: vanilla's makes no decision; the rich soil sustains; wet farmland is fertile,
        // dry farmland and dirt aren't, the rich soil always is.
        let mut host = field("test:cabbages[age=0]", "minecraft:farmland[moisture=7]");
        rt.with_host(&mut host, |rt| {
            let soil_pos = api::block_pos(rt, 0, 0, 0);
            let up = api::direction(rt, 1);
            let decision = |rt: &mut Rt, soil: &str| {
                let s = api::state(rt, soil);
                let d = api::call(rt, SUSTAINS, &[s, level, soil_pos, up, crop]);
                rt.str_of(rt.get_field(d, "$name")).unwrap_or("").to_owned()
            };
            assert_eq!(decision(rt, "minecraft:farmland[moisture=7]"), "DEFAULT");
            assert_eq!(decision(rt, "test:rich_soil_farmland[moisture=0]"), "TRUE");
            let fertile = |rt: &mut Rt, soil: &str| {
                let s = api::state(rt, soil);
                api::call(rt, IS_FERTILE, &[s, level, pos]).i() != 0
            };
            assert!(fertile(rt, "minecraft:farmland[moisture=7]"));
            assert!(!fertile(rt, "minecraft:farmland[moisture=0]"));
            assert!(!fertile(rt, "minecraft:dirt"));
            assert!(fertile(rt, "test:rich_soil_farmland[moisture=0]"));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
