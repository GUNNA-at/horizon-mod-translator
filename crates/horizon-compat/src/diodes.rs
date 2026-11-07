//! `DiodeBlock` as NeoForge 1.21.1 has it, for a mod's diodes (blocks whose first vanilla class is
//! `DiodeBlock`): Create's toggle and powered latches and its brass diodes (pulse repeater,
//! extender, timer), Create Connected's sequenced pulse generator, Create Aeronautics' accumulator
//! and inductor. A diode reads its input behind it (where `FACING` points), changes `POWERED` a
//! block tick later (its `getDelay`, at the priority vanilla gives a diode: higher while it is on,
//! highest when it feeds another diode), sends its signal forward only (its strong signal is the
//! same), and tells the block in front, and that block's neighbours, of each change
//! (`updateNeighborsInFront`, from `onPlace` and `onRemove`). It drops when the block below no
//! longer holds it up (a rigid top face).
//!
//! Each method calls the block's own overrides where its class has them, as the JVM dispatches: a
//! latch's own `tick` runs `super.tick` here, which asks the latch's own `getDelay`. The game's
//! hooks reach these for a method the block's class leaves to `DiodeBlock` ([`inherited`]).
use crate::host::{join_state, split_state, NeighborUpdate};
use crate::{api, Rt, V};

pub(crate) const DIODE: &str = "net/minecraft/world/level/block/DiodeBlock";

/// `DiodeBlock`'s method `$short`: (the key a call falls back to, the method).
macro_rules! diode {
    ($short:literal) => {
        (concat!("net/minecraft/world/level/block/DiodeBlock.", $short), $short)
    };
}

const TICK: (&str, &str) = diode!("tick.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)V");
const GET_SIGNAL: (&str, &str) = diode!("getSignal.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)I");
const GET_DIRECT_SIGNAL: (&str, &str) =
    diode!("getDirectSignal.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)I");
const NEIGHBOR_CHANGED: (&str, &str) = diode!(
    "neighborChanged.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;Lnet/minecraft/core/BlockPos;Z)V"
);
const CHECK_TICK: (&str, &str) = diode!("checkTickOnNeighbor.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V");
const IS_LOCKED: (&str, &str) = diode!("isLocked.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z");
const SHOULD_TURN_ON: (&str, &str) = diode!("shouldTurnOn.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z");
const GET_INPUT_SIGNAL: (&str, &str) = diode!("getInputSignal.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)I");
const GET_ALTERNATE_SIGNAL: (&str, &str) =
    diode!("getAlternateSignal.(Lnet/minecraft/world/level/SignalGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)I");
const IS_SIGNAL_SOURCE: (&str, &str) = diode!("isSignalSource.(Lnet/minecraft/world/level/block/state/BlockState;)Z");
const STATE_FOR_PLACEMENT: (&str, &str) = diode!("getStateForPlacement.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;");
const SET_PLACED_BY: (&str, &str) = diode!(
    "setPlacedBy.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;)V"
);
const ON_PLACE: (&str, &str) =
    diode!("onPlace.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V");
const ON_REMOVE: (&str, &str) =
    diode!("onRemove.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V");
const UPDATE_IN_FRONT: (&str, &str) = diode!("updateNeighborsInFront.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V");
const SIDE_DIODES_ONLY: (&str, &str) = diode!("sideInputDiodesOnly.()Z");
const GET_OUTPUT_SIGNAL: (&str, &str) = diode!("getOutputSignal.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)I");
const IS_DIODE: (&str, &str) = diode!("isDiode.(Lnet/minecraft/world/level/block/state/BlockState;)Z");
const SHOULD_PRIORITIZE: (&str, &str) = diode!("shouldPrioritize.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z");
const CAN_SURVIVE: (&str, &str) = diode!("canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z");
const CAN_SURVIVE_ON: (&str, &str) = diode!("canSurviveOn.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z");
/// Abstract in `DiodeBlock`: every diode has its own.
const GET_DELAY: (&str, &str) = diode!("getDelay.(Lnet/minecraft/world/level/block/state/BlockState;)I");

/// The methods answered here.
const METHODS: [(&str, &str); 21] = [
    TICK,
    GET_SIGNAL,
    GET_DIRECT_SIGNAL,
    NEIGHBOR_CHANGED,
    CHECK_TICK,
    IS_LOCKED,
    SHOULD_TURN_ON,
    GET_INPUT_SIGNAL,
    GET_ALTERNATE_SIGNAL,
    IS_SIGNAL_SOURCE,
    STATE_FOR_PLACEMENT,
    SET_PLACED_BY,
    ON_PLACE,
    ON_REMOVE,
    UPDATE_IN_FRONT,
    SIDE_DIODES_ONLY,
    GET_OUTPUT_SIGNAL,
    IS_DIODE,
    SHOULD_PRIORITIZE,
    CAN_SURVIVE,
    CAN_SURVIVE_ON,
];

/// The methods the game's hooks ask a block about, which a diode may leave to `DiodeBlock`.
const HOOKED: [(&str, &str); 10] = [TICK, NEIGHBOR_CHANGED, ON_PLACE, ON_REMOVE, GET_SIGNAL, GET_DIRECT_SIGNAL, IS_SIGNAL_SOURCE, CAN_SURVIVE, STATE_FOR_PLACEMENT, SET_PLACED_BY];

const SET_BLOCK: (&str, &str) = (
    "net/minecraft/world/level/Level.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
    "setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
);
const REMOVE_BLOCK: (&str, &str) = ("net/minecraft/world/level/Level.removeBlock.(Lnet/minecraft/core/BlockPos;Z)Z", "removeBlock.(Lnet/minecraft/core/BlockPos;Z)Z");
const GET_BLOCK_ENTITY: (&str, &str) = (
    "net/minecraft/world/level/Level.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
    "getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
);
const HAS_BLOCK_ENTITY: &str = "net/minecraft/world/level/block/state/BlockState.hasBlockEntity.()Z";
const DROP_RESOURCES: &str = "net/minecraft/world/level/block/Block.dropResources.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;)V";
const DEFAULT_STATE: &str = "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
const HORIZONTAL_DIRECTION: &str = "net/minecraft/world/item/context/BlockPlaceContext.getHorizontalDirection.()Lnet/minecraft/core/Direction;";

/// `TickPriority`'s values.
const EXTREMELY_HIGH: i32 = -3;
const VERY_HIGH: i32 = -2;
const HIGH: i32 = -1;
/// `Direction`'s names and steps, by data value; `SupportType.RIGID`'s ordinal.
const NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const STEPS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
const UP: usize = 1;
const RIGID: usize = 2;

/// Whether the module answers `owner.name desc`.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    owner == DIODE && METHODS.iter().any(|(_, short)| short.split_once('.') == Some((name, desc)))
}

/// The key the game's hook for the method `short` reaches on a mod block of class `class` that has
/// no such method of its own: `DiodeBlock`'s, where `DiodeBlock` is the first vanilla class above
/// it. None otherwise: the block's template answers.
pub(crate) fn inherited(rt: &Rt, class: &'static str, short: &str) -> Option<&'static str> {
    if class.starts_with("net/minecraft/") || crate::rotation::vanilla_base(rt, class) != DIODE {
        return None;
    }
    HOOKED.iter().find(|(_, s)| *s == short).map(|(key, _)| *key)
}

/// `DiodeBlock`'s answer to `key` (the block first in `args`, but for the static `isDiode`).
pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let short = key.strip_prefix(DIODE)?.strip_prefix('.')?;
    if !METHODS.iter().any(|(_, s)| *s == short) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (name, _) = short.split_once('.')?;
    Some(match name {
        "tick" => {
            tick(rt, a(0), a(1), a(2), a(3));
            V::Void
        }
        "getSignal" => V::I(signal(rt, a(0), a(1), a(2), a(3), a(4))),
        // The strong signal is the signal: `state.getSignal(level, pos, side)`, the block's own.
        "getDirectSignal" => dispatch(rt, GET_SIGNAL, &[a(0), a(1), a(2), a(3), a(4)]),
        "neighborChanged" => {
            neighbor_changed(rt, a(0), a(1), a(2), a(3));
            V::Void
        }
        "checkTickOnNeighbor" => {
            check_tick(rt, a(0), a(1), a(2), a(3));
            V::Void
        }
        "isLocked" | "sideInputDiodesOnly" => V::I(0),
        "shouldTurnOn" => V::I((dispatch(rt, GET_INPUT_SIGNAL, &[a(0), a(1), a(2), a(3)]).i() > 0) as i32),
        "getInputSignal" => V::I(input_signal(rt, a(2), a(3))),
        "getAlternateSignal" => V::I(alternate_signal(rt, a(0), a(2), a(3))),
        "isSignalSource" => V::I(1),
        "getStateForPlacement" => state_for_placement(rt, a(0), a(1)),
        "setPlacedBy" => {
            if dispatch(rt, SHOULD_TURN_ON, &[a(0), a(1), a(2), a(3)]).i() != 0 {
                let (p, block) = (api::pos_of(rt, a(2)), api::block_name(rt, a(0)));
                if let Some(h) = rt.host_mut() {
                    h.schedule_block_tick(p, &block, 1);
                }
            }
            V::Void
        }
        "onPlace" => {
            dispatch(rt, UPDATE_IN_FRONT, &[a(0), a(2), a(3), a(1)]);
            V::Void
        }
        "onRemove" => {
            on_remove(rt, a(0), a(1), a(2), a(3), a(4), a(5).i() != 0);
            V::Void
        }
        "updateNeighborsInFront" => {
            update_in_front(rt, a(0), a(2), a(3));
            V::Void
        }
        "getOutputSignal" => V::I(15),
        "isDiode" => {
            let text = crate::api_common::text_of(rt, a(0));
            V::I(is_diode(rt, &text) as i32)
        }
        "shouldPrioritize" => V::I(should_prioritize(rt, a(2), a(3)) as i32),
        "canSurvive" => V::I(can_survive(rt, a(0), a(2), a(3)) as i32),
        // A rigid top face below: a full block's, not a fence's or a bottom slab's.
        "canSurviveOn" => {
            let text = crate::api_common::text_of(rt, a(3));
            V::I(api::face_sturdy(rt, &text, [a(3), a(1), a(2)], UP, RIGID) as i32)
        }
        _ => return None,
    })
}

/// `this.method(args)` as the JVM dispatches it: the diode's own where its class has one, else
/// `DiodeBlock`'s.
fn dispatch(rt: &mut Rt, method: (&'static str, &'static str), args: &[V]) -> V {
    api::virtual_call(rt, method, args)
}

/// A vanilla diode, or a mod block whose class extends `DiodeBlock` (`DiodeBlock.isDiode`).
pub(crate) fn is_diode(rt: &Rt, text: &str) -> bool {
    matches!(split_state(text).0, "minecraft:repeater" | "minecraft:comparator") || crate::glue::mod_block(rt, text).is_some_and(|(_, class)| crate::neo::reaches(rt, class, DIODE))
}

fn prop<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    split_state(text).1.into_iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// The state's `FACING`, a data value (north where it has none).
fn facing(text: &str) -> u8 {
    prop(text, "facing").and_then(|f| NAMES.iter().position(|n| *n == f)).unwrap_or(2) as u8
}

fn powered(text: &str) -> bool {
    prop(text, "powered") == Some("true")
}

/// The state with its property `key` set to `value` (a state without it stays as it is).
fn with(text: &str, key: &str, value: &str) -> String {
    let (block, mut props) = split_state(text);
    if let Some(p) = props.iter_mut().find(|(k, _)| *k == key) {
        p.1 = value;
    }
    join_state(block, props)
}

fn rel(p: [i32; 3], d: u8) -> [i32; 3] {
    let s = STEPS[usize::from(d.min(5))];
    [p[0] + s[0], p[1] + s[1], p[2] + s[2]]
}

/// A horizontal direction turned a quarter about the vertical: clockwise seen from above.
fn clockwise(d: u8) -> u8 {
    match d {
        2 => 5,
        5 => 3,
        3 => 4,
        4 => 2,
        d => d,
    }
}

fn counter_clockwise(d: u8) -> u8 {
    match d {
        2 => 4,
        4 => 3,
        3 => 5,
        5 => 2,
        d => d,
    }
}

fn block_at(rt: &Rt, p: [i32; 3]) -> String {
    rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p))
}

/// `level.getSignal(pos, direction)`: what the block at `pos` sends back along `direction`, as the
/// game reads it (through a conductor, the strong signals into it).
fn level_signal(rt: &mut Rt, p: [i32; 3], d: u8) -> i32 {
    rt.host_call(|h| h.signal(p, d, false)).unwrap_or(0).clamp(0, 15)
}

/// A redstone wire's power, 0 for another block.
fn wire_power(text: &str) -> i32 {
    let (block, props) = split_state(text);
    if block != "minecraft:redstone_wire" {
        return 0;
    }
    props.iter().find(|(k, _)| *k == "power").and_then(|(_, v)| v.parse().ok()).unwrap_or(0)
}

/// `level.setBlock(pos, state, 2)`: the change reaches the players, and the blocks hear of it by the
/// diode's `onPlace`, as vanilla's.
fn set_block(rt: &mut Rt, level: V, pos: V, text: &str) {
    let st = api::state(rt, text);
    dispatch(rt, SET_BLOCK, &[level, pos, st, V::I(2)]);
}

/// `level.scheduleTick(pos, this, delay, priority)`.
fn schedule(rt: &mut Rt, this: V, pos: V, delay: i32, priority: i32) {
    let (p, block) = (api::pos_of(rt, pos), api::block_name(rt, this));
    if let Some(h) = rt.host_mut() {
        h.schedule_block_tick_at_priority(p, &block, delay, priority);
    }
}

/// `DiodeBlock.tick`: unless locked, it turns off when its input is gone, and on otherwise, then
/// off again a delay later when the input was already gone (a pulse shorter than its delay still
/// passes as one as long).
fn tick(rt: &mut Rt, this: V, state: V, level: V, pos: V) {
    let locked = dispatch(rt, IS_LOCKED, &[this, level, pos, state]).i() != 0;
    if locked || rt.thrown.is_some() {
        return;
    }
    let text = crate::api_common::text_of(rt, state);
    let on = powered(&text);
    let should = dispatch(rt, SHOULD_TURN_ON, &[this, level, pos, state]).i() != 0;
    if rt.thrown.is_some() {
        return;
    }
    if on && !should {
        set_block(rt, level, pos, &with(&text, "powered", "false"));
    } else if !on {
        set_block(rt, level, pos, &with(&text, "powered", "true"));
        if !should {
            let delay = dispatch(rt, GET_DELAY, &[this, state]).i();
            schedule(rt, this, pos, delay, VERY_HIGH);
        }
    }
}

/// `DiodeBlock.getSignal(state, level, pos, side)`: its output, forward only, while powered.
fn signal(rt: &mut Rt, this: V, state: V, level: V, pos: V, side: V) -> i32 {
    let text = crate::api_common::text_of(rt, state);
    if !powered(&text) || facing(&text) != crate::api_common::direction_ordinal(rt, side) {
        return 0;
    }
    dispatch(rt, GET_OUTPUT_SIGNAL, &[this, level, pos, state]).i()
}

/// `DiodeBlock.neighborChanged`: where it still stands, whether its input changed
/// (`checkTickOnNeighbor`); else it drops, and the blocks around it hear.
fn neighbor_changed(rt: &mut Rt, this: V, state: V, level: V, pos: V) {
    let survives = dispatch(rt, CAN_SURVIVE, &[this, state, level, pos]).i() != 0;
    if rt.thrown.is_some() {
        return;
    }
    if survives {
        dispatch(rt, CHECK_TICK, &[this, level, pos, state]);
        return;
    }
    let be = if api::call(rt, HAS_BLOCK_ENTITY, &[state]).i() != 0 { dispatch(rt, GET_BLOCK_ENTITY, &[level, pos]) } else { V::NULL };
    api::call(rt, DROP_RESOURCES, &[state, level, pos, be]);
    dispatch(rt, REMOVE_BLOCK, &[level, pos, V::I(0)]);
    let (p, block) = (api::pos_of(rt, pos), api::block_name(rt, this));
    for d in 0..6 {
        let around = NeighborUpdate::Around { pos: rel(p, d), skip: None };
        rt.host_call(|h| h.update_neighbors(around, &block));
    }
}

/// `DiodeBlock.checkTickOnNeighbor`: a tick to come when its input no longer matches `POWERED` and
/// none is about to run, sooner while it is on and soonest when it feeds a diode
/// ([`should_prioritize`]).
fn check_tick(rt: &mut Rt, this: V, level: V, pos: V, state: V) {
    let locked = dispatch(rt, IS_LOCKED, &[this, level, pos, state]).i() != 0;
    if locked || rt.thrown.is_some() {
        return;
    }
    let on = powered(&crate::api_common::text_of(rt, state));
    let should = dispatch(rt, SHOULD_TURN_ON, &[this, level, pos, state]).i() != 0;
    if rt.thrown.is_some() || on == should {
        return;
    }
    let (p, block) = (api::pos_of(rt, pos), api::block_name(rt, this));
    if rt.host_mut().is_some_and(|h| h.block_tick_queued(p, &block, true)) {
        return;
    }
    let priority = if dispatch(rt, SHOULD_PRIORITIZE, &[this, level, pos, state]).i() != 0 {
        EXTREMELY_HIGH
    } else if on {
        VERY_HIGH
    } else {
        HIGH
    };
    let delay = dispatch(rt, GET_DELAY, &[this, state]).i();
    schedule(rt, this, pos, delay, priority);
}

/// `DiodeBlock.getInputSignal`: what the block behind sends it, or a wire's power there.
fn input_signal(rt: &mut Rt, pos: V, state: V) -> i32 {
    let d = facing(&crate::api_common::text_of(rt, state));
    let behind = rel(api::pos_of(rt, pos), d);
    let signal = level_signal(rt, behind, d);
    if signal >= 15 {
        return signal;
    }
    signal.max(wire_power(&block_at(rt, behind)))
}

/// `DiodeBlock.getAlternateSignal`: the stronger of its two sides' control inputs (only from
/// diodes where `sideInputDiodesOnly`).
fn alternate_signal(rt: &mut Rt, this: V, pos: V, state: V) -> i32 {
    let p = api::pos_of(rt, pos);
    let d = facing(&crate::api_common::text_of(rt, state));
    let only = dispatch(rt, SIDE_DIODES_ONLY, &[this]).i() != 0;
    [clockwise(d), counter_clockwise(d)].into_iter().map(|side| crate::level_env::control_input_signal(rt, rel(p, side), side, only)).max().unwrap_or(0)
}

/// `DiodeBlock.getStateForPlacement`: its default state, its output the way the placer looks (its
/// input, where `FACING` points, on the placer's side).
fn state_for_placement(rt: &mut Rt, this: V, ctx: V) -> V {
    let default = api::call(rt, DEFAULT_STATE, &[this]);
    let toward = api::call(rt, HORIZONTAL_DIRECTION, &[ctx]);
    let d = crate::api_common::direction_ordinal(rt, toward) ^ 1;
    let text = with(&crate::api_common::text_of(rt, default), "facing", NAMES[usize::from(d.min(5))]);
    api::state(rt, &text)
}

/// `DiodeBlock.onRemove`: once the block is gone (not moved, not a state of its own), its block
/// entity goes, and the block in front hears.
fn on_remove(rt: &mut Rt, this: V, state: V, level: V, pos: V, new: V, moved: bool) {
    let (old, next) = (crate::api_common::text_of(rt, state), crate::api_common::text_of(rt, new));
    if moved || split_state(&old).0 == split_state(&next).0 {
        return;
    }
    crate::block_breaking::on_remove(rt, state, level, pos, new);
    dispatch(rt, UPDATE_IN_FRONT, &[this, level, pos, state]);
}

/// `DiodeBlock.updateNeighborsInFront`: the block in front hears the diode changed, then the
/// blocks around that one, but the diode.
fn update_in_front(rt: &mut Rt, this: V, pos: V, state: V) {
    let p = api::pos_of(rt, pos);
    let d = facing(&crate::api_common::text_of(rt, state));
    let front = rel(p, d ^ 1);
    let block = api::block_name(rt, this);
    rt.host_call(|h| {
        h.update_neighbors(NeighborUpdate::One { pos: front, from: p }, &block);
        h.update_neighbors(NeighborUpdate::Around { pos: front, skip: Some(d) }, &block);
    });
}

/// `DiodeBlock.shouldPrioritize`: the block in front is a diode that doesn't send its own output
/// back into this one (one in line after it, or turned so this one feeds its side).
fn should_prioritize(rt: &mut Rt, pos: V, state: V) -> bool {
    let d = facing(&crate::api_common::text_of(rt, state)) ^ 1;
    let front = block_at(rt, rel(api::pos_of(rt, pos), d));
    is_diode(rt, &front) && facing(&front) != d
}

/// `DiodeBlock.canSurvive`: the block's own `canSurviveOn` the block below.
fn can_survive(rt: &mut Rt, this: V, level: V, pos: V) -> bool {
    let p = api::pos_of(rt, pos);
    let below = [p[0], p[1] - 1, p[2]];
    let text = block_at(rt, below);
    let st = api::state(rt, &text);
    let at = api::block_pos(rt, below[0].into(), below[1].into(), below[2].into());
    dispatch(rt, CAN_SURVIVE_ON, &[this, level, at, st]).i() != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{BlockFacts, MemoryHost};

    /// A diode with a delay of its own only (a brass diode's part that is `DiodeBlock`'s).
    const PLAIN: &str = "test/PlainDiode";
    /// Create's toggle latch: its own delay (1), `tick` (vanilla's, then `POWERING` flips each time
    /// the input comes on), `getSignal` and `getOutputSignal` (15 while `POWERING`).
    const LATCH: &str = "test/ToggleLatch";
    /// A diode locked while either side gets a signal (its own `isLocked`, by vanilla's
    /// `getAlternateSignal`).
    const LOCKABLE: &str = "test/LockableDiode";
    /// A diode with its own `checkTickOnNeighbor` (Create's powered latch): a tick in 7.
    const SIDED: &str = "test/SidedLatch";
    const SCHEDULE: &str = "net/minecraft/world/level/Level.scheduleTick.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;I)V";

    fn overrides(class: &str, key: &str) -> bool {
        match class {
            PLAIN => key == GET_DELAY.1,
            LATCH => [GET_DELAY.1, TICK.1, GET_SIGNAL.1, GET_OUTPUT_SIGNAL.1].contains(&key),
            LOCKABLE => key == GET_DELAY.1 || key == IS_LOCKED.1,
            SIDED => key == GET_DELAY.1 || key == CHECK_TICK.1,
            _ => false,
        }
    }

    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let class = rt.class_of(args[0]);
        if !overrides(class, key) {
            return api::call(rt, owner_key, args);
        }
        let text = |rt: &Rt, i: usize| crate::api_common::text_of(rt, args[i]);
        match key {
            k if k == GET_DELAY.1 => V::I(if class == LATCH { 1 } else { 2 }),
            k if k == IS_LOCKED.1 => V::I((dispatch(rt, GET_ALTERNATE_SIGNAL, args).i() > 0) as i32),
            k if k == CHECK_TICK.1 => api::call(rt, SCHEDULE, &[args[1], args[2], args[0], V::I(7)]),
            k if k == GET_OUTPUT_SIGNAL.1 => V::I(if prop(&text(rt, 3), "powering") == Some("true") { 15 } else { 0 }),
            k if k == GET_SIGNAL.1 => {
                let side = crate::api_common::direction_ordinal(rt, args[4]);
                if facing(&text(rt, 1)) == side { dispatch(rt, GET_OUTPUT_SIGNAL, &[args[0], args[2], args[3], args[1]]) } else { V::I(0) }
            }
            k if k == TICK.1 => {
                let was = powered(&text(rt, 1));
                api::call(rt, TICK.0, args);
                let p = api::pos_of(rt, args[3]);
                let now = block_at(rt, p);
                if powered(&now) && !was {
                    let flipped = if prop(&now, "powering") == Some("true") { "false" } else { "true" };
                    set_block(rt, args[2], args[3], &with(&now, "powering", flipped));
                }
                V::Void
            }
            _ => unreachable!("{key}"),
        }
    }

    fn runtime() -> Rt {
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        for (class, id, default) in [
            (PLAIN, "test:diode", "facing=north,powered=false"),
            (LATCH, "test:latch", "facing=north,powered=false,powering=false"),
            (LOCKABLE, "test:lockable", "facing=north,powered=false"),
            (SIDED, "test:sided", "facing=north,powered=false"),
        ] {
            rt.supers.insert(class, DIODE);
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), id.into()), b);
            let d = rt.string(default);
            rt.put_field(b, "$default", d);
        }
        rt
    }

    const AT: [i32; 3] = [0, 1, 0];
    /// Behind a diode facing north: its input.
    const BEHIND: [i32; 3] = [0, 1, -1];
    /// In front of it: where its output goes.
    const FRONT: [i32; 3] = [0, 1, 1];

    /// `diode` at (0, 1, 0) on stone (every face of it sturdy), with air (none sturdy) around.
    fn bench(diode: &str) -> MemoryHost {
        let mut host = MemoryHost::default();
        host.facts.insert("minecraft:stone".into(), BlockFacts { hardness: 1.5, push: 0, replaceable: false, sturdy: u32::MAX });
        host.facts.insert("minecraft:air".into(), BlockFacts { hardness: 0.0, push: 1, replaceable: true, sturdy: 0 });
        host.blocks.insert([0, 0, 0], "minecraft:stone".into());
        host.blocks.insert(AT, diode.into());
        host
    }

    /// The input behind the diode at (0, 1, 0) sends `signal` into it (north, the way to it).
    fn input(host: &mut MemoryHost, signal: i32) {
        host.sent_signals.insert((BEHIND, 2), (signal, signal));
    }

    fn neighbor_changed(rt: &mut Rt, host: &mut MemoryHost) -> bool {
        let state = host.blocks[&AT].clone();
        crate::glue::neighbor_changed(rt, host, AT, &state, BEHIND, "minecraft:lever", false)
    }

    /// The scheduled tick the game runs.
    fn tick(rt: &mut Rt, host: &mut MemoryHost) {
        host.scheduled.clear();
        host.tick_priorities.clear();
        let state = host.blocks[&AT].clone();
        assert!(crate::glue::tick_block(rt, host, AT, &state));
    }

    /// The signal the diode at (0, 1, 0) sends along `side` (the way from the block reading it).
    fn sent(rt: &mut Rt, host: &mut MemoryHost, side: u8, direct: bool) -> Option<i32> {
        let state = host.blocks[&AT].clone();
        crate::glue::signal(rt, host, AT, &state, side, direct)
    }

    /// A mod diode that leaves it all to `DiodeBlock` but its delay: its input coming on schedules a
    /// tick in its own delay (high priority), which powers it; it then sends 15 forward only, strong
    /// as weak; its input gone, a tick at very high priority turns it off. Ticked with no input
    /// while off, it still turns on, and off again a delay later (a pulse passes as one as long).
    #[test]
    fn a_mod_diode_turns_on_and_off_a_delay_after_its_input_as_vanillas() {
        let mut rt = runtime();
        let mut host = bench("test:diode[facing=north,powered=false]");
        input(&mut host, 15);
        assert!(neighbor_changed(&mut rt, &mut host));
        assert_eq!(host.scheduled, [(AT, "test:diode".to_owned(), 2)]);
        assert_eq!(host.tick_priorities, [(AT, HIGH)]);
        tick(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "test:diode[facing=north,powered=true]");
        assert!(host.scheduled.is_empty());
        // Forward only: to the block in front, which reads it northward.
        assert_eq!(sent(&mut rt, &mut host, 2, false), Some(15));
        assert_eq!(sent(&mut rt, &mut host, 2, true), Some(15));
        for side in [0, 1, 3, 4, 5] {
            assert_eq!(sent(&mut rt, &mut host, side, false), Some(0), "side {side}");
        }
        // The input gone: off a delay later, sooner than it came on.
        input(&mut host, 0);
        host.scheduled.clear();
        neighbor_changed(&mut rt, &mut host);
        assert_eq!(host.scheduled, [(AT, "test:diode".to_owned(), 2)]);
        assert_eq!(host.tick_priorities, [(AT, VERY_HIGH)]);
        tick(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "test:diode[facing=north,powered=false]");
        assert_eq!(sent(&mut rt, &mut host, 2, false), Some(0));
        // A tick with no input while off: on now, off again in its delay.
        tick(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "test:diode[facing=north,powered=true]");
        assert_eq!(host.scheduled, [(AT, "test:diode".to_owned(), 2)]);
        assert_eq!(host.tick_priorities, [(AT, VERY_HIGH)]);
        // A wire's power behind it counts as its input too.
        let mut wired = bench("test:diode[facing=north,powered=false]");
        wired.blocks.insert(BEHIND, "minecraft:redstone_wire[east=none,north=side,power=4,south=side,west=none]".into());
        neighbor_changed(&mut rt, &mut wired);
        assert_eq!(wired.scheduled.len(), 1);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// No second tick is queued while the one already taken for this game tick is about to run
    /// (`willTickThisTick`), and none when the input agrees with the state.
    #[test]
    fn a_diode_queues_no_tick_when_one_is_about_to_run_or_nothing_changed() {
        let mut rt = runtime();
        let mut host = bench("test:diode[facing=north,powered=false]");
        input(&mut host, 15);
        host.ticking_now.push((AT, "test:diode".into()));
        neighbor_changed(&mut rt, &mut host);
        assert!(host.scheduled.is_empty());
        let mut steady = bench("test:diode[facing=north,powered=true]");
        input(&mut steady, 9);
        neighbor_changed(&mut rt, &mut steady);
        assert!(steady.scheduled.is_empty());
    }

    /// A diode whose output feeds another diode ticks first (`shouldPrioritize`: the diode in front
    /// doesn't send its own output back into it), so chains of them hold together.
    #[test]
    fn a_diode_feeding_a_diode_ticks_at_the_highest_priority() {
        let mut rt = runtime();
        let priority = |rt: &mut Rt, front: &str| {
            let mut host = bench("test:diode[facing=north,powered=false]");
            input(&mut host, 15);
            host.blocks.insert(FRONT, front.into());
            neighbor_changed(rt, &mut host);
            host.tick_priorities[0].1
        };
        assert_eq!(priority(&mut rt, "minecraft:repeater[delay=1,facing=north,locked=false,powered=false]"), EXTREMELY_HIGH);
        assert_eq!(priority(&mut rt, "minecraft:comparator[facing=east,mode=compare,powered=false]"), EXTREMELY_HIGH);
        assert_eq!(priority(&mut rt, "test:latch[facing=west,powered=false,powering=false]"), EXTREMELY_HIGH);
        // Its output back into this one: no hurry.
        assert_eq!(priority(&mut rt, "minecraft:repeater[delay=1,facing=south,locked=false,powered=false]"), HIGH);
        assert_eq!(priority(&mut rt, "minecraft:stone"), HIGH);
    }

    /// Each change of a diode's state, and its removal, tells the block in front and that block's
    /// neighbours but the diode (`updateNeighborsInFront` from `onPlace` and `onRemove`), the
    /// game's change hook reaching `DiodeBlock`'s for a block without its own.
    #[test]
    fn the_blocks_in_front_hear_each_change_of_a_diode() {
        let mut rt = runtime();
        let in_front = [
            (NeighborUpdate::One { pos: FRONT, from: AT }, "test:diode".to_owned()),
            (NeighborUpdate::Around { pos: FRONT, skip: Some(2) }, "test:diode".to_owned()),
        ];
        let (off, on) = ("test:diode[facing=north,powered=false]", "test:diode[facing=north,powered=true]");
        let mut host = bench(on);
        crate::glue::block_replaced(&mut rt, &mut host, AT, off, on, false);
        assert_eq!(host.neighbor_updates, in_front);
        host.neighbor_updates.clear();
        host.blocks.insert(AT, "minecraft:air".into());
        crate::glue::block_replaced(&mut rt, &mut host, AT, on, "minecraft:air", false);
        assert_eq!(host.neighbor_updates, in_front);
        // Moved by a piston, it leaves without a word.
        host.neighbor_updates.clear();
        crate::glue::block_replaced(&mut rt, &mut host, AT, on, "minecraft:air", true);
        assert!(host.neighbor_updates.is_empty());
    }

    /// A diode stands on a rigid top face only: where the block below goes, a neighbour's change
    /// drops it, and the blocks around it hear.
    #[test]
    fn a_diode_without_a_rigid_floor_drops() {
        let mut rt = runtime();
        let mut host = bench("test:diode[facing=north,powered=false]");
        assert_eq!(crate::glue::can_survive(&mut rt, &mut host, AT, "test:diode[facing=north,powered=false]"), Some(true));
        host.blocks.insert([0, 0, 0], "minecraft:air".into());
        assert_eq!(crate::glue::can_survive(&mut rt, &mut host, AT, "test:diode[facing=north,powered=false]"), Some(false));
        neighbor_changed(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "minecraft:air");
        let around: Vec<_> = host.neighbor_updates.iter().filter(|(u, _)| matches!(u, NeighborUpdate::Around { skip: None, .. })).map(|(u, _)| *u).collect();
        let expected: Vec<_> = (0..6).map(|d| NeighborUpdate::Around { pos: rel(AT, d), skip: None }).collect();
        assert_eq!(around, expected);
        assert!(host.scheduled.is_empty());
    }

    /// Create's toggle latch: its own `tick` runs `DiodeBlock`'s (`super.tick`), which asks its own
    /// delay; each time its input comes on its output flips, and stays when the input goes.
    #[test]
    fn a_latchs_own_tick_runs_vanillas_with_its_own_delay() {
        let mut rt = runtime();
        let mut host = bench("test:latch[facing=north,powered=false,powering=false]");
        input(&mut host, 15);
        neighbor_changed(&mut rt, &mut host);
        assert_eq!(host.scheduled, [(AT, "test:latch".to_owned(), 1)]);
        tick(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "test:latch[facing=north,powered=true,powering=true]");
        assert_eq!(sent(&mut rt, &mut host, 2, false), Some(15));
        // The input gone: unpowered, still sending.
        input(&mut host, 0);
        neighbor_changed(&mut rt, &mut host);
        tick(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "test:latch[facing=north,powered=false,powering=true]");
        assert_eq!(sent(&mut rt, &mut host, 2, true), Some(15));
        // On again: off.
        input(&mut host, 15);
        neighbor_changed(&mut rt, &mut host);
        tick(&mut rt, &mut host);
        assert_eq!(host.blocks[&AT], "test:latch[facing=north,powered=true,powering=false]");
        assert_eq!(sent(&mut rt, &mut host, 2, false), Some(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A diode's own `checkTickOnNeighbor` is what a neighbour's change reaches (Create's powered
    /// latch reads its sides there), and its own `isLocked` keeps vanilla's from scheduling: a side
    /// input (`getAlternateSignal`: a redstone block, a wire's power, a source's strong signal).
    #[test]
    fn a_diodes_own_check_and_lock_answer_inside_vanillas() {
        let mut rt = runtime();
        let mut host = bench("test:sided[facing=north,powered=false]");
        neighbor_changed(&mut rt, &mut host);
        assert_eq!(host.scheduled, [(AT, "test:sided".to_owned(), 7)]);
        // The sides of a diode facing north: east (clockwise) and west.
        let (east, west) = ([1, 1, 0], [-1, 1, 0]);
        let locked = |rt: &mut Rt, side: [i32; 3], block: &str, strong: i32| {
            let mut host = bench("test:lockable[facing=north,powered=false]");
            input(&mut host, 15);
            host.blocks.insert(side, block.into());
            let d = if side == east { 5 } else { 4 };
            host.sent_signals.insert((side, d), (strong, strong));
            neighbor_changed(rt, &mut host);
            host.scheduled.is_empty()
        };
        assert!(locked(&mut rt, east, "minecraft:redstone_block", 0));
        assert!(locked(&mut rt, west, "minecraft:redstone_wire[east=side,north=none,power=3,south=none,west=side]", 0));
        assert!(locked(&mut rt, west, "minecraft:redstone_torch", 15));
        assert!(!locked(&mut rt, east, "minecraft:stone", 0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Placed, a diode sends its output the way the placer looks; a diode set down with its input
    /// already on ticks on at once (`setPlacedBy`).
    #[test]
    fn a_placed_diode_faces_the_placer_and_ticks_when_powered() {
        let mut rt = runtime();
        let mut host = bench("minecraft:air");
        let place = |rt: &mut Rt, host: &mut MemoryHost, yaw: f32| {
            let placement = crate::glue::Placement { pos: AT, hit_pos: [0, 0, 0], face: 1, yaw, item: "test:diode".into(), ..Default::default() };
            crate::glue::state_for_placement(rt, host, "test:diode[facing=north,powered=false]", &placement)
        };
        // Looking south: the input on the north side, by the placer.
        assert_eq!(place(&mut rt, &mut host, 0.0), Some(Some("test:diode[facing=north,powered=false]".into())));
        assert_eq!(place(&mut rt, &mut host, 90.0), Some(Some("test:diode[facing=east,powered=false]".into())));
        let mut powered = bench("test:diode[facing=north,powered=false]");
        input(&mut powered, 15);
        rt.with_host(&mut powered, |rt| {
            let b = rt.registered[&("block".to_owned(), "test:diode".to_owned())];
            let (level, pos) = (crate::glue::level(rt), api::block_pos(rt, 0, 1, 0));
            let state = api::state(rt, "test:diode[facing=north,powered=false]");
            api::call(rt, SET_PLACED_BY.0, &[b, level, pos, state, V::NULL, V::NULL]);
        });
        assert_eq!(powered.scheduled, [(AT, "test:diode".to_owned(), 1)]);
        assert!(powered.tick_priorities.is_empty());
    }
}
