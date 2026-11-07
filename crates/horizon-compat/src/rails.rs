//! `BaseRailBlock` and `RailState` as NeoForge 1.21.1 has them, for a mod's rails: blocks whose
//! first vanilla class is `BaseRailBlock` (Create's controller rail and cart assembler,
//! FramedBlocks' rail slope), `RailBlock` (FramedBlocks' fancy rail, SecurityCraft's track mine),
//! `PoweredRailBlock` (FramedBlocks' fancy powered and activator rails) or `DetectorRailBlock`.
//! Placed, a rail turns to join the rails beside it, which turn to it in turn (`updateDir`:
//! `RailState.place`), and a straight one then hears of itself; told a neighbour changed, it drops
//! where nothing holds it up (a rigid top face below it, and on the side it climbs to), else runs
//! its own `updateState`: a controller rail takes its power, a plain rail at a junction turns as a
//! signal switches it, a powered rail comes on while a signal reaches it along its line. Removed
//! (or only changed), a straight rail tells the blocks around it and below it, one that climbs the
//! blocks above. A detector rail's own methods (which find the carts on it) aren't answered here.
//!
//! Each method calls the rail's own overrides where its class has them, as the JVM dispatches:
//! Create's controller rail has its own `updateDir`, which runs `super.updateDir`, and its own
//! `updateState`. The game's hooks reach these for a method the rail's class leaves to vanilla
//! ([`inherited`]). The rails beside a mod's may be vanilla's, which answer as theirs.
use crate::host::{join_state, split_state};
use crate::{Rt, V, api};

pub(crate) const BASE: &str = "net/minecraft/world/level/block/BaseRailBlock";
pub(crate) const RAIL: &str = "net/minecraft/world/level/block/RailBlock";
pub(crate) const POWERED: &str = "net/minecraft/world/level/block/PoweredRailBlock";
pub(crate) const DETECTOR: &str = "net/minecraft/world/level/block/DetectorRailBlock";
/// NeoForge's interface on `BaseRailBlock`, which mods also call its methods through.
const EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IBaseRailBlockExtension";
const RAIL_SHAPE: &str = "net/minecraft/world/level/block/state/properties/RailShape";
/// `BlockTags.RAILS`: with a rail's class, what makes a block a rail (`BaseRailBlock.isRail`).
const RAILS: &str = "minecraft:rails";

/// `BaseRailBlock`'s method `$short`: (the key a call falls back to, the method).
macro_rules! base {
    ($short:literal) => {
        (
            concat!("net/minecraft/world/level/block/BaseRailBlock.", $short),
            $short,
        )
    };
}

/// `PoweredRailBlock`'s own method `$short`.
macro_rules! powered {
    ($short:literal) => {
        (
            concat!("net/minecraft/world/level/block/PoweredRailBlock.", $short),
            $short,
        )
    };
}

const ON_PLACE: (&str, &str) = base!(
    "onPlace.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V"
);
const ON_REMOVE: (&str, &str) = base!(
    "onRemove.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V"
);
const NEIGHBOR_CHANGED: (&str, &str) = base!(
    "neighborChanged.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;Lnet/minecraft/core/BlockPos;Z)V"
);
const CAN_SURVIVE: (&str, &str) = base!(
    "canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z"
);
const STATE_FOR_PLACEMENT: (&str, &str) = base!(
    "getStateForPlacement.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;"
);
const UPDATE_SHAPE: (&str, &str) = base!(
    "updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;"
);
/// `updateState(state, level, pos, movedByPiston)`, from `onPlace`: the rail's direction, then a
/// straight one hears of itself.
const UPDATE_STATE_PLACED: (&str, &str) = base!(
    "updateState.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Z)Lnet/minecraft/world/level/block/state/BlockState;"
);
/// `updateState(state, level, pos, neighborBlock)`: what a neighbour's change does to the rail
/// (nothing in `BaseRailBlock`).
const UPDATE_STATE: (&str, &str) = base!(
    "updateState.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V"
);
const UPDATE_DIR: (&str, &str) = base!(
    "updateDir.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)Lnet/minecraft/world/level/block/state/BlockState;"
);
const IS_STRAIGHT: (&str, &str) = base!("isStraight.()Z");
/// Abstract in `BaseRailBlock`: each rail class has its own.
const GET_SHAPE_PROPERTY: (&str, &str) =
    base!("getShapeProperty.()Lnet/minecraft/world/level/block/state/properties/Property;");
const GET_RAIL_DIRECTION: (&str, &str) = base!(
    "getRailDirection.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/vehicle/AbstractMinecart;)Lnet/minecraft/world/level/block/state/properties/RailShape;"
);
const IS_FLEXIBLE: (&str, &str) = base!(
    "isFlexibleRail.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z"
);
const CAN_MAKE_SLOPES: (&str, &str) = base!(
    "canMakeSlopes.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z"
);
const IS_VALID_SHAPE: (&str, &str) =
    base!("isValidRailShape.(Lnet/minecraft/world/level/block/state/properties/RailShape;)Z");
const ON_MINECART_PASS: (&str, &str) = base!(
    "onMinecartPass.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/vehicle/AbstractMinecart;)V"
);
/// The static `isRail(state)` and `isRail(level, pos)`.
const IS_RAIL: &str = "isRail.(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const IS_RAIL_AT: &str = "isRail.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;)Z";
const FIND_SIGNAL: (&str, &str) = powered!(
    "findPoweredRailSignal.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;ZI)Z"
);
const SAME_RAIL_WITH_POWER: (&str, &str) = powered!(
    "isSameRailWithPower.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;ZILnet/minecraft/world/level/block/state/properties/RailShape;)Z"
);
const IS_ACTIVATOR: (&str, &str) = powered!("isActivatorRail.()Z");

/// `BaseRailBlock`'s methods answered here (and `RailBlock`'s, `PoweredRailBlock`'s and
/// `DetectorRailBlock`'s where they inherit them).
const METHODS: [&str; 18] = [
    ON_PLACE.1,
    ON_REMOVE.1,
    NEIGHBOR_CHANGED.1,
    CAN_SURVIVE.1,
    STATE_FOR_PLACEMENT.1,
    UPDATE_SHAPE.1,
    UPDATE_STATE_PLACED.1,
    UPDATE_STATE.1,
    UPDATE_DIR.1,
    IS_STRAIGHT.1,
    GET_SHAPE_PROPERTY.1,
    GET_RAIL_DIRECTION.1,
    IS_FLEXIBLE.1,
    CAN_MAKE_SLOPES.1,
    IS_VALID_SHAPE.1,
    ON_MINECART_PASS.1,
    IS_RAIL,
    IS_RAIL_AT,
];

/// `PoweredRailBlock`'s own, beside those.
const POWERED_METHODS: [&str; 3] = [FIND_SIGNAL.1, SAME_RAIL_WITH_POWER.1, IS_ACTIVATOR.1];

/// The methods the game's hooks ask a block about, which a rail may leave to its vanilla class.
const HOOKED: [(&str, &str); 5] = [
    ON_PLACE,
    ON_REMOVE,
    NEIGHBOR_CHANGED,
    CAN_SURVIVE,
    STATE_FOR_PLACEMENT,
];

const STATE_GET_VALUE: &str = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
const SET_BLOCK: (&str, &str) = (
    "net/minecraft/world/level/Level.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
    "setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
);
const REMOVE_BLOCK: (&str, &str) = (
    "net/minecraft/world/level/Level.removeBlock.(Lnet/minecraft/core/BlockPos;Z)Z",
    "removeBlock.(Lnet/minecraft/core/BlockPos;Z)Z",
);
const UPDATE_NEIGHBORS_AT: (&str, &str) = (
    "net/minecraft/world/level/Level.updateNeighborsAt.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V",
    "updateNeighborsAt.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V",
);
const HAS_NEIGHBOR_SIGNAL: &str =
    "net/minecraft/world/level/Level.hasNeighborSignal.(Lnet/minecraft/core/BlockPos;)Z";
const IS_CLIENT_SIDE: &str = "net/minecraft/world/level/Level.isClientSide.()Z";
const CAN_SUPPORT_RIGID: &str = "net/minecraft/world/level/block/Block.canSupportRigidBlock.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const DROP_RESOURCES: &str = "net/minecraft/world/level/block/Block.dropResources.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;)V";
const DEFAULT_STATE: &str = "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
const IS_SIGNAL_SOURCE: (&str, &str) = (
    "net/minecraft/world/level/block/state/BlockBehaviour.isSignalSource.(Lnet/minecraft/world/level/block/state/BlockState;)Z",
    "isSignalSource.(Lnet/minecraft/world/level/block/state/BlockState;)Z",
);
const CONTEXT_LEVEL: &str =
    "net/minecraft/world/item/context/UseOnContext.getLevel.()Lnet/minecraft/world/level/Level;";
const CLICKED_POS: &str = "net/minecraft/world/item/context/BlockPlaceContext.getClickedPos.()Lnet/minecraft/core/BlockPos;";
const HORIZONTAL_DIRECTION: &str = "net/minecraft/world/item/context/BlockPlaceContext.getHorizontalDirection.()Lnet/minecraft/core/Direction;";
const LEVEL_FLUID_STATE: &str = "net/minecraft/world/level/Level.getFluidState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;";
const FLUID_TYPE: &str = "net/minecraft/world/level/material/FluidState.getType.()Lnet/minecraft/world/level/material/Fluid;";
const WATER: &str = "net/minecraft/world/level/material/Fluids.WATER";

/// The vanilla classes (below `net/minecraft/world/level/block/`) whose `isSignalSource` says yes
/// in 1.21.1 (redstone wire's while it signals, which it does but inside its own update).
const SIGNAL_SOURCES: [&str; 17] = [
    "BasePressurePlateBlock",
    "ButtonBlock",
    "DaylightDetectorBlock",
    "DetectorRailBlock",
    "DiodeBlock",
    "JukeboxBlock",
    "LecternBlock",
    "LeverBlock",
    "LightningRodBlock",
    "ObserverBlock",
    "PoweredBlock",
    "RedStoneWireBlock",
    "RedstoneTorchBlock",
    "SculkSensorBlock",
    "TargetBlock",
    "TrappedChestBlock",
    "TripWireHookBlock",
];

/// `Direction`'s data values of the four horizontal ones, and their steps.
const NORTH: [i32; 3] = [0, 0, -1];
const SOUTH: [i32; 3] = [0, 0, 1];
const WEST: [i32; 3] = [-1, 0, 0];
const EAST: [i32; 3] = [1, 0, 0];

/// Whether the module answers `owner.name desc`.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    let method = |m: &&str| m.split_once('.') == Some((name, desc));
    match owner {
        BASE | RAIL => METHODS.iter().any(method),
        POWERED => METHODS.iter().chain(&POWERED_METHODS).any(method),
        // A detector rail's own `onPlace` also finds the carts on it.
        DETECTOR => name != "onPlace" && METHODS.iter().any(method),
        EXTENSION => [
            IS_FLEXIBLE.1,
            CAN_MAKE_SLOPES.1,
            GET_RAIL_DIRECTION.1,
            IS_VALID_SHAPE.1,
            ON_MINECART_PASS.1,
        ]
        .iter()
        .any(method),
        _ => false,
    }
}

/// The key the game's hook for the method `short` reaches on a mod block of class `class` that has
/// no such method of its own: `BaseRailBlock`'s, where the first vanilla class above it is a rail's
/// (a detector rail's `onPlace` is its own). None otherwise: the block's template answers.
pub(crate) fn inherited(rt: &Rt, class: &'static str, short: &str) -> Option<&'static str> {
    if class.starts_with("net/minecraft/") {
        return None;
    }
    let base = crate::rotation::vanilla_base(rt, class);
    if !is_rail_class(base) || (base == DETECTOR && short == ON_PLACE.1) {
        return None;
    }
    HOOKED
        .iter()
        .find(|(_, s)| *s == short)
        .map(|(key, _)| *key)
}

/// A rail's constructor reached: `BaseRailBlock(isStraight, properties)`, `RailBlock(properties)`
/// (a plain rail turns at corners), `PoweredRailBlock(properties, isPoweredRail)` and NeoForge's
/// `PoweredRailBlock(properties)`, which makes an activator rail, `DetectorRailBlock(properties)`.
/// The fields vanilla gives them: `isStraight`, `isActivator`.
pub(crate) fn constructed(rt: &mut Rt, this: V, owner: &str, desc: &str, args: &[V]) {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        BASE if desc.starts_with("(Z") => {
            rt.put_field(this, "isStraight", V::I((a(1).i() != 0) as i32))
        }
        RAIL => rt.put_field(this, "isStraight", V::I(0)),
        POWERED => {
            rt.put_field(this, "isStraight", V::I(1));
            let powered_rail = desc.ends_with("Z)V") && a(2).i() != 0;
            rt.put_field(this, "isActivator", V::I((!powered_rail) as i32));
        }
        DETECTOR => rt.put_field(this, "isStraight", V::I(1)),
        _ => {}
    }
}

/// A rail's answer to `key` (the block first in `args`, but for the static `isRail`).
pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let short = key.strip_prefix(owner)?.strip_prefix('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match short {
        s if s == IS_RAIL => {
            let text = crate::api_common::text_of(rt, a(0));
            V::I(is_rail(rt, &text) as i32)
        }
        s if s == IS_RAIL_AT => {
            let text = block_at(rt, api::pos_of(rt, a(1)));
            V::I(is_rail(rt, &text) as i32)
        }
        s if s == IS_STRAIGHT.1 => V::I(straight(rt, a(0)) as i32),
        s if s == IS_FLEXIBLE.1 => V::I(!straight(rt, a(0)) as i32),
        // `IBaseRailBlockExtension`'s defaults.
        s if s == CAN_MAKE_SLOPES.1 || s == IS_VALID_SHAPE.1 => V::I(1),
        s if s == ON_MINECART_PASS.1 => V::Void,
        s if s == GET_SHAPE_PROPERTY.1 => shape_property(rt, a(0)),
        // `state.getValue(getShapeProperty())`.
        s if s == GET_RAIL_DIRECTION.1 => {
            let property = dispatch(rt, GET_SHAPE_PROPERTY, &[a(0)]);
            api::call(rt, STATE_GET_VALUE, &[a(1), property])
        }
        s if s == CAN_SURVIVE.1 => {
            let below = step(api::pos_of(rt, a(3)), [0, -1, 0]);
            V::I(rigid(rt, a(2), below) as i32)
        }
        s if s == ON_PLACE.1 => {
            on_place(rt, [a(0), a(1), a(2), a(3)], a(4), a(5));
            V::Void
        }
        s if s == UPDATE_STATE_PLACED.1 => update_state_placed(rt, [a(0), a(1), a(2), a(3)], a(4)),
        s if s == UPDATE_STATE.1 => {
            update_state(rt, [a(0), a(1), a(2), a(3)], a(4));
            V::Void
        }
        s if s == UPDATE_DIR.1 => update_dir(rt, a(0), a(1), a(2), a(3), a(4).i() != 0),
        s if s == NEIGHBOR_CHANGED.1 => {
            neighbor_changed(rt, [a(0), a(1), a(2), a(3)], a(4), a(6));
            V::Void
        }
        s if s == ON_REMOVE.1 => {
            on_remove(rt, [a(0), a(1), a(2), a(3)], a(4), a(5).i() != 0);
            V::Void
        }
        s if s == STATE_FOR_PLACEMENT.1 => state_for_placement(rt, a(0), a(1)),
        // `BlockBehaviour.updateShape`'s: the state stays. (Vanilla's also schedules a waterlogged
        // rail's water tick, which the game's own fluid rules see to.)
        s if s == UPDATE_SHAPE.1 => a(1),
        s if s == FIND_SIGNAL.1 => {
            V::I(find_signal(rt, [a(0), a(1), a(2), a(3)], a(4).i() != 0, a(5).i()) as i32)
        }
        s if s == SAME_RAIL_WITH_POWER.1 => {
            V::I(same_rail_with_power(rt, [a(0), a(1), a(2)], a(3).i() != 0, a(4).i(), a(5)) as i32)
        }
        s if s == IS_ACTIVATOR.1 => V::I(activator(rt, a(0)) as i32),
        _ => return None,
    })
}

/// `this.method(args)` as the JVM dispatches it: the rail's own where its class has one, else
/// its vanilla class's (here).
fn dispatch(rt: &mut Rt, method: (&'static str, &'static str), args: &[V]) -> V {
    api::virtual_call(rt, method, args)
}

fn is_rail_class(class: &str) -> bool {
    matches!(class, BASE | RAIL | POWERED | DETECTOR)
}

/// The vanilla class a block is or extends: a mod block's first vanilla class, a vanilla block's
/// own (`Block` for a block unknown to both).
fn vanilla_class_of(rt: &Rt, block: V) -> &'static str {
    let class = rt.class_of(block);
    if !class.starts_with("net/minecraft/") {
        return crate::rotation::vanilla_base(rt, class);
    }
    let Some(name) = rt.str_of(block) else {
        return class;
    };
    let table = &crate::vanilla_classes::BLOCKS;
    table
        .binary_search_by(|(id, _)| (*id).cmp(name))
        .map_or(class, |i| table[i].1)
}

/// `BaseRailBlock.isRail(state)`: in `#minecraft:rails`, of a rail's class.
fn is_rail(rt: &mut Rt, text: &str) -> bool {
    let (name, _) = split_state(text);
    if !rt.host().is_some_and(|h| h.block_in_tag(name, RAILS)) {
        return false;
    }
    let block = api::block(rt, name);
    is_rail_class(vanilla_class_of(rt, block))
}

/// `this.isStraight` (the field its constructor set): a plain rail's false, any other vanilla
/// rail's true.
fn straight(rt: &Rt, this: V) -> bool {
    match rt.get_field(this, "isStraight") {
        V::I(s) => s != 0,
        _ => vanilla_class_of(rt, this) != RAIL,
    }
}

/// `PoweredRailBlock.isActivatorRail()`.
fn activator(rt: &Rt, this: V) -> bool {
    match rt.get_field(this, "isActivator") {
        V::I(a) => a != 0,
        _ => rt.str_of(this) == Some("minecraft:activator_rail"),
    }
}

/// `getShapeProperty()` of a vanilla rail class: a plain rail's `RAIL_SHAPE` (it curves), the
/// others' `RAIL_SHAPE_STRAIGHT`.
fn shape_property(rt: &mut Rt, this: V) -> V {
    let field = if vanilla_class_of(rt, this) == RAIL {
        "RAIL_SHAPE"
    } else {
        "RAIL_SHAPE_STRAIGHT"
    };
    crate::api_common::vanilla_property_object(rt, field).unwrap_or(V::NULL)
}

/// The rail's shape in `state`, as text (`north_south`): `getRailDirection(state, level, pos, null)`.
fn direction(rt: &mut Rt, [this, state, level, pos]: [V; 4]) -> String {
    let shape = dispatch(rt, GET_RAIL_DIRECTION, &[this, state, level, pos, V::NULL]);
    crate::api_common::text_of(rt, shape)
}

/// The name the rail's `getShapeProperty()` has: what its shape is written as in a state.
fn shape_name(rt: &mut Rt, this: V) -> String {
    let property = dispatch(rt, GET_SHAPE_PROPERTY, &[this]);
    crate::api_common::prop_parts(rt, property).0
}

/// `state.getValue(getShapeProperty())`, as text.
fn shape_of(rt: &mut Rt, this: V, state: V) -> String {
    let property = dispatch(rt, GET_SHAPE_PROPERTY, &[this]);
    let shape = api::call(rt, STATE_GET_VALUE, &[state, property]);
    crate::api_common::text_of(rt, shape)
}

fn prop<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    split_state(text)
        .1
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

/// The state with its property `key` set to `value`, in its place (a state without it stays as it
/// is).
fn with(text: &str, key: &str, value: &str) -> String {
    let (block, mut props) = split_state(text);
    if let Some(p) = props.iter_mut().find(|(k, _)| *k == key) {
        p.1 = value;
    }
    join_state(block, props)
}

/// Two states' texts name the same state (their properties in any order).
fn same_state(a: &str, b: &str) -> bool {
    let (sorted_a, sorted_b) = (split_state(a), split_state(b));
    let (mut pa, mut pb) = (sorted_a.1, sorted_b.1);
    pa.sort_unstable();
    pb.sort_unstable();
    sorted_a.0 == sorted_b.0 && pa == pb
}

fn step(p: [i32; 3], d: [i32; 3]) -> [i32; 3] {
    [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
}

fn above(p: [i32; 3]) -> [i32; 3] {
    step(p, [0, 1, 0])
}

fn below(p: [i32; 3]) -> [i32; 3] {
    step(p, [0, -1, 0])
}

fn pos_value(rt: &mut Rt, p: [i32; 3]) -> V {
    api::block_pos(rt, p[0].into(), p[1].into(), p[2].into())
}

fn block_at(rt: &Rt, p: [i32; 3]) -> String {
    rt.host()
        .map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p))
}

fn client(rt: &mut Rt, level: V) -> bool {
    api::call(rt, IS_CLIENT_SIDE, &[level]).i() != 0
}

/// `Block.canSupportRigidBlock(level, p)`: the top of the block there holds a rail.
fn rigid(rt: &mut Rt, level: V, p: [i32; 3]) -> bool {
    let at = pos_value(rt, p);
    api::call(rt, CAN_SUPPORT_RIGID, &[level, at]).i() != 0
}

fn has_neighbor_signal(rt: &mut Rt, level: V, p: [i32; 3]) -> bool {
    let at = pos_value(rt, p);
    api::call(rt, HAS_NEIGHBOR_SIGNAL, &[level, at]).i() != 0
}

/// `level.setBlock(p, state, 3)`.
fn set_block(rt: &mut Rt, level: V, p: [i32; 3], text: &str) {
    let (at, st) = (pos_value(rt, p), api::state(rt, text));
    dispatch(rt, SET_BLOCK, &[level, at, st, V::I(3)]);
}

/// `level.updateNeighborsAt(p, block)`.
fn update_neighbors_at(rt: &mut Rt, level: V, p: [i32; 3], block: V) {
    let at = pos_value(rt, p);
    dispatch(rt, UPDATE_NEIGHBORS_AT, &[level, at, block]);
}

/// `BaseRailBlock.onPlace`: a rail new there (not a change of its state) takes its direction
/// (`updateState`, the rail's own where it has one).
fn on_place(rt: &mut Rt, [this, state, level, pos]: [V; 4], old: V, moved: V) {
    let (now, before) = (
        crate::api_common::text_of(rt, state),
        crate::api_common::text_of(rt, old),
    );
    if split_state(&now).0 != split_state(&before).0 {
        dispatch(rt, UPDATE_STATE_PLACED, &[this, state, level, pos, moved]);
    }
}

/// `BaseRailBlock.updateState(state, level, pos, movedByPiston)`: the rail's direction
/// (`updateDir`, always placed), then a straight rail hears of itself, as `level.neighborChanged`
/// runs it when no other update is under way.
fn update_state_placed(rt: &mut Rt, [this, state, level, pos]: [V; 4], moved: V) -> V {
    let state = dispatch(rt, UPDATE_DIR, &[this, level, pos, state, V::I(1)]);
    if rt.thrown.is_none() && straight(rt, this) {
        let text = crate::api_common::text_of(rt, state);
        let block = api::block(rt, split_state(&text).0);
        dispatch(
            rt,
            NEIGHBOR_CHANGED,
            &[block, state, level, pos, this, pos, moved],
        );
    }
    state
}

/// `BaseRailBlock.neighborChanged`: on the server, a rail still there drops where nothing holds it
/// up, else runs its own `updateState` for the change.
fn neighbor_changed(rt: &mut Rt, [this, state, level, pos]: [V; 4], block: V, moved: V) {
    if client(rt, level) {
        return;
    }
    let p = api::pos_of(rt, pos);
    if split_state(&block_at(rt, p)).0 != api::block_name(rt, this) {
        return;
    }
    let shape = direction(rt, [this, state, level, pos]);
    if rt.thrown.is_some() {
        return;
    }
    if should_be_removed(rt, level, p, &shape) {
        api::call(rt, DROP_RESOURCES, &[state, level, pos]);
        dispatch(rt, REMOVE_BLOCK, &[level, pos, moved]);
    } else {
        dispatch(rt, UPDATE_STATE, &[this, state, level, pos, block]);
    }
}

/// `BaseRailBlock.shouldBeRemoved`: no rigid top face below it, or, where it climbs, none on the
/// side it climbs to.
fn should_be_removed(rt: &mut Rt, level: V, p: [i32; 3], shape: &str) -> bool {
    if !rigid(rt, level, below(p)) {
        return true;
    }
    let side = match shape {
        "ascending_east" => EAST,
        "ascending_west" => WEST,
        "ascending_north" => NORTH,
        "ascending_south" => SOUTH,
        _ => return false,
    };
    !rigid(rt, level, step(p, side))
}

/// `updateState(state, level, pos, neighborBlock)` of the rail's vanilla class: a plain rail at a
/// junction of three turns again when a signal source changed beside it; a powered rail comes on
/// or goes off with the signal reaching it; `BaseRailBlock`'s does nothing.
fn update_state(rt: &mut Rt, [this, state, level, pos]: [V; 4], block: V) {
    match vanilla_class_of(rt, this) {
        RAIL => {
            if !signal_source(rt, block) {
                return;
            }
            let (p, text) = (api::pos_of(rt, pos), crate::api_common::text_of(rt, state));
            if RailState::new(rt, level, p, &text).count_potential_connections(rt) == 3 {
                dispatch(rt, UPDATE_DIR, &[this, level, pos, state, V::I(0)]);
            }
        }
        POWERED => update_powered(rt, [this, state, level, pos]),
        _ => {}
    }
}

/// `block.defaultBlockState().isSignalSource()`: the block's own answer where its class has one,
/// else its vanilla class's ([`SIGNAL_SOURCES`]).
fn signal_source(rt: &mut Rt, block: V) -> bool {
    let class = rt.class_of(block);
    if !class.starts_with("net/minecraft/")
        && rt.overrides.is_some_and(|o| o(class, IS_SIGNAL_SOURCE.1))
    {
        let st = api::call(rt, DEFAULT_STATE, &[block]);
        return dispatch(rt, IS_SIGNAL_SOURCE, &[block, st]).i() != 0;
    }
    crate::blocks::declaring_class(vanilla_class_of(rt, block), &SIGNAL_SOURCES)
        .is_some_and(|c| c != "BlockBehaviour")
}

/// `PoweredRailBlock.updateState`: on while a signal reaches it, or reaches a rail of its kind in
/// line with it no more than eight rails along either way; a change tells the blocks around the
/// one below it, and around the one above where it climbs.
fn update_powered(rt: &mut Rt, [this, state, level, pos]: [V; 4]) {
    let text = crate::api_common::text_of(rt, state);
    let p = api::pos_of(rt, pos);
    let on = prop(&text, "powered") == Some("true");
    let should = has_neighbor_signal(rt, level, p)
        || dispatch(
            rt,
            FIND_SIGNAL,
            &[this, level, pos, state, V::I(1), V::I(0)],
        )
        .i() != 0
        || dispatch(
            rt,
            FIND_SIGNAL,
            &[this, level, pos, state, V::I(0), V::I(0)],
        )
        .i() != 0;
    if should == on || rt.thrown.is_some() {
        return;
    }
    set_block(
        rt,
        level,
        p,
        &with(&text, "powered", if should { "true" } else { "false" }),
    );
    update_neighbors_at(rt, level, below(p), this);
    if shape_of(rt, this, state).starts_with("ascending_") {
        update_neighbors_at(rt, level, above(p), this);
    }
}

/// `PoweredRailBlock.findPoweredRailSignal`: the next rail along its line (forward: south, or
/// west; up or down a slope), or the one below that where the line doesn't climb, is a rail of its
/// kind with power ([`same_rail_with_power`]); no further than eight rails.
fn find_signal(rt: &mut Rt, [this, level, pos, state]: [V; 4], forward: bool, count: i32) -> bool {
    if count >= 8 {
        return false;
    }
    let [mut i, mut j, mut k] = api::pos_of(rt, pos);
    let mut level_line = true;
    let shape = shape_of(rt, this, state);
    let line = match shape.as_str() {
        "north_south" => {
            k += if forward { 1 } else { -1 };
            "north_south"
        }
        "east_west" => {
            i += if forward { -1 } else { 1 };
            "east_west"
        }
        "ascending_east" => {
            if forward {
                i -= 1;
            } else {
                i += 1;
                j += 1;
                level_line = false;
            }
            "east_west"
        }
        "ascending_west" => {
            if forward {
                i -= 1;
                j += 1;
                level_line = false;
            } else {
                i += 1;
            }
            "east_west"
        }
        "ascending_north" => {
            if forward {
                k += 1;
            } else {
                k -= 1;
                j += 1;
                level_line = false;
            }
            "north_south"
        }
        "ascending_south" => {
            if forward {
                k += 1;
                j += 1;
                level_line = false;
            } else {
                k -= 1;
            }
            "north_south"
        }
        other => other,
    }
    .to_owned();
    let constant = crate::blocks::vanilla_enum_constant(rt, RAIL_SHAPE, &line).unwrap_or(V::NULL);
    let powered_at = |rt: &mut Rt, p: [i32; 3]| {
        let at = pos_value(rt, p);
        dispatch(
            rt,
            SAME_RAIL_WITH_POWER,
            &[this, level, at, V::I(forward as i32), V::I(count), constant],
        )
        .i() != 0
    };
    powered_at(rt, [i, j, k]) || (level_line && powered_at(rt, [i, j - 1, k]))
}

/// `PoweredRailBlock.isSameRailWithPower`: the block at `pos` is a powered rail of the same kind
/// (powered or activator) not across the line `shape`, and a signal reaches it, or one reaches a
/// rail further along ([`find_signal`]).
fn same_rail_with_power(
    rt: &mut Rt,
    [this, level, pos]: [V; 3],
    forward: bool,
    count: i32,
    shape: V,
) -> bool {
    let p = api::pos_of(rt, pos);
    let text = block_at(rt, p);
    let other = api::block(rt, split_state(&text).0);
    if vanilla_class_of(rt, other) != POWERED {
        return false;
    }
    let st = api::state(rt, &text);
    let theirs = direction(rt, [other, st, level, pos]);
    let line = crate::api_common::text_of(rt, shape);
    let across = match line.as_str() {
        "east_west" => matches!(
            theirs.as_str(),
            "north_south" | "ascending_north" | "ascending_south"
        ),
        "north_south" => matches!(
            theirs.as_str(),
            "east_west" | "ascending_east" | "ascending_west"
        ),
        _ => false,
    };
    if across || dispatch(rt, IS_ACTIVATOR, &[this]).i() != dispatch(rt, IS_ACTIVATOR, &[other]).i()
    {
        return false;
    }
    has_neighbor_signal(rt, level, p)
        || dispatch(
            rt,
            FIND_SIGNAL,
            &[other, level, pos, st, V::I(forward as i32), V::I(count + 1)],
        )
        .i() != 0
}

/// `BaseRailBlock.updateDir`: on the server, the rail turns to join the rails beside it
/// (`RailState.place`, with whether a signal reaches it), and they to it.
fn update_dir(rt: &mut Rt, this: V, level: V, pos: V, state: V, always: bool) -> V {
    if client(rt, level) {
        return state;
    }
    let shape = shape_of(rt, this, state);
    let p = api::pos_of(rt, pos);
    let powered = has_neighbor_signal(rt, level, p);
    let text = crate::api_common::text_of(rt, state);
    let placed = RailState::new(rt, level, p, &text).place(rt, level, powered, always, &shape);
    api::state(rt, &placed.state)
}

/// `BaseRailBlock.onRemove`: unless moved, the block entity goes with its block, and the blocks
/// around tell of it: above where it climbed, around and below a straight one.
fn on_remove(rt: &mut Rt, [this, state, level, pos]: [V; 4], new: V, moved: bool) {
    if moved {
        return;
    }
    crate::block_breaking::on_remove(rt, state, level, pos, new);
    let p = api::pos_of(rt, pos);
    if direction(rt, [this, state, level, pos]).starts_with("ascending_") {
        update_neighbors_at(rt, level, above(p), this);
    }
    if straight(rt, this) {
        update_neighbors_at(rt, level, p, this);
        update_neighbors_at(rt, level, below(p), this);
    }
}

/// `BaseRailBlock.getStateForPlacement`: the default state along the way the placer looks (east
/// or west: `east_west`), waterlogged in a water source.
fn state_for_placement(rt: &mut Rt, this: V, ctx: V) -> V {
    let level = api::call(rt, CONTEXT_LEVEL, &[ctx]);
    let at = api::call(rt, CLICKED_POS, &[ctx]);
    let fluid = api::call(rt, LEVEL_FLUID_STATE, &[level, at]);
    let kind = api::call(rt, FLUID_TYPE, &[fluid]);
    let water = kind.r() != 0 && kind == api::get_static(rt, WATER);
    let default = api::call(rt, DEFAULT_STATE, &[this]);
    let toward = api::call(rt, HORIZONTAL_DIRECTION, &[ctx]);
    let along_x = matches!(crate::api_common::direction_ordinal(rt, toward), 4 | 5);
    let name = shape_name(rt, this);
    let text = crate::api_common::text_of(rt, default);
    let text = with(
        &with(
            &text,
            &name,
            if along_x { "east_west" } else { "north_south" },
        ),
        "waterlogged",
        if water { "true" } else { "false" },
    );
    api::state(rt, &text)
}

/// `RailState`: a rail as the rails beside it see it, with the two places its shape joins.
struct RailState {
    pos: [i32; 3],
    /// Its state, as text.
    state: String,
    /// Its block: a mod's object, or a vanilla block's.
    block: V,
    /// What its shape is written as in a state (`getShapeProperty`'s name).
    property: String,
    /// It doesn't curve (`!isFlexibleRail`).
    straight: bool,
    slopes: bool,
    connections: Vec<[i32; 3]>,
}

/// The places a rail of `shape` at `p` joins.
fn connections(p: [i32; 3], shape: &str) -> Vec<[i32; 3]> {
    let (n, s, w, e) = (step(p, NORTH), step(p, SOUTH), step(p, WEST), step(p, EAST));
    match shape {
        "north_south" => vec![n, s],
        "east_west" => vec![w, e],
        "ascending_east" => vec![w, above(e)],
        "ascending_west" => vec![above(w), e],
        "ascending_north" => vec![above(n), s],
        "ascending_south" => vec![n, above(s)],
        "south_east" => vec![e, s],
        "south_west" => vec![w, s],
        "north_west" => vec![w, n],
        "north_east" => vec![e, n],
        _ => Vec::new(),
    }
}

/// `BaseRailBlock.isRail(level, p)`.
fn is_rail_at(rt: &mut Rt, p: [i32; 3]) -> bool {
    let text = block_at(rt, p);
    is_rail(rt, &text)
}

/// `RailState.hasRail`: a rail at `p`, or a step above or below it.
fn has_rail(rt: &mut Rt, p: [i32; 3]) -> bool {
    is_rail_at(rt, p) || is_rail_at(rt, above(p)) || is_rail_at(rt, below(p))
}

/// `RailState.getRail`: the rail at `p`, else a step above, else below.
fn get_rail(rt: &mut Rt, level: V, p: [i32; 3]) -> Option<RailState> {
    [p, above(p), below(p)].into_iter().find_map(|q| {
        let text = block_at(rt, q);
        is_rail(rt, &text).then(|| RailState::new(rt, level, q, &text))
    })
}

impl RailState {
    /// `new RailState(level, pos, state)`: what the rail's block says of its shape, its curving and
    /// its slopes.
    fn new(rt: &mut Rt, level: V, pos: [i32; 3], state: &str) -> Self {
        let block = api::block(rt, split_state(state).0);
        let (st, at) = (api::state(rt, state), pos_value(rt, pos));
        let shape = direction(rt, [block, st, level, at]);
        let straight = dispatch(rt, IS_FLEXIBLE, &[block, st, level, at]).i() == 0;
        let slopes = dispatch(rt, CAN_MAKE_SLOPES, &[block, st, level, at]).i() != 0;
        let property = shape_name(rt, block);
        Self {
            pos,
            state: state.to_owned(),
            block,
            property,
            straight,
            slopes,
            connections: connections(pos, &shape),
        }
    }

    /// `removeSoftConnections`: the places it joins that hold a rail joining it back (that rail's
    /// own place, a step up or down), the others let go.
    fn remove_soft_connections(&mut self, rt: &mut Rt, level: V) {
        let mut i = 0;
        while i < self.connections.len() {
            match get_rail(rt, level, self.connections[i]) {
                Some(r) if r.connects_to(self.pos) => {
                    self.connections[i] = r.pos;
                    i += 1;
                }
                _ => {
                    self.connections.remove(i);
                }
            }
        }
    }

    /// `hasConnection`: it joins the column of `p`.
    fn connects_to(&self, p: [i32; 3]) -> bool {
        self.connections
            .iter()
            .any(|c| c[0] == p[0] && c[2] == p[2])
    }

    /// `countPotentialConnections`: the rails beside it, a step up or down counted.
    fn count_potential_connections(&self, rt: &mut Rt) -> usize {
        [NORTH, EAST, SOUTH, WEST]
            .into_iter()
            .filter(|d| has_rail(rt, step(self.pos, *d)))
            .count()
    }

    /// `canConnectTo`: it joins `other` already, or doesn't join two rails.
    fn can_connect_to(&self, other: &RailState) -> bool {
        self.connects_to(other.pos) || self.connections.len() != 2
    }

    /// The rail's own `isValidRailShape(shape)`.
    fn valid(&self, rt: &mut Rt, shape: &str) -> bool {
        let constant =
            crate::blocks::vanilla_enum_constant(rt, RAIL_SHAPE, shape).unwrap_or(V::NULL);
        dispatch(rt, IS_VALID_SHAPE, &[self.block, constant]).i() != 0
    }

    /// `connectTo`: it joins the rail at `other` too, and turns to the places it joins (curving
    /// where it may, climbing to a rail a step up where it may), unless the rail refuses that
    /// shape.
    fn connect_to(&mut self, rt: &mut Rt, level: V, other: [i32; 3]) {
        self.connections.push(other);
        let (n, s, w, e) = (
            step(self.pos, NORTH),
            step(self.pos, SOUTH),
            step(self.pos, WEST),
            step(self.pos, EAST),
        );
        let (north, south, west, east) = (
            self.connects_to(n),
            self.connects_to(s),
            self.connects_to(w),
            self.connects_to(e),
        );
        let mut shape = None;
        if north || south {
            shape = Some("north_south");
        }
        if west || east {
            shape = Some("east_west");
        }
        if !self.straight {
            if south && east && !north && !west {
                shape = Some("south_east");
            }
            if south && west && !north && !east {
                shape = Some("south_west");
            }
            if north && west && !south && !east {
                shape = Some("north_west");
            }
            if north && east && !south && !west {
                shape = Some("north_east");
            }
        }
        shape = self.sloped(rt, shape);
        let shape = shape.unwrap_or("north_south");
        if !self.valid(rt, shape) {
            if let Some(i) = self.connections.iter().position(|c| *c == other) {
                self.connections.remove(i);
            }
            return;
        }
        self.state = with(&self.state, &self.property, shape);
        set_block(rt, level, self.pos, &self.state);
    }

    /// A straight shape that climbs where the rail may and a rail is a step up at one end: the
    /// south end (or west) wins.
    fn sloped(&self, rt: &mut Rt, shape: Option<&'static str>) -> Option<&'static str> {
        let mut shape = shape;
        if !self.slopes {
            return shape;
        }
        let ends = match shape {
            Some("north_south") => [(NORTH, "ascending_north"), (SOUTH, "ascending_south")],
            Some("east_west") => [(EAST, "ascending_east"), (WEST, "ascending_west")],
            _ => return shape,
        };
        for (d, climbing) in ends {
            if is_rail_at(rt, above(step(self.pos, d))) {
                shape = Some(climbing);
            }
        }
        shape
    }

    /// `hasNeighborRail`: the rail at `p` (or a step up or down) may join this one.
    fn has_neighbor_rail(&self, rt: &mut Rt, level: V, p: [i32; 3]) -> bool {
        match get_rail(rt, level, p) {
            Some(mut r) => {
                r.remove_soft_connections(rt, level);
                r.can_connect_to(self)
            }
            None => false,
        }
    }

    /// `RailState.place`: the shape the rails beside it give it (a curve where it may, at a
    /// junction of three the one a signal picks, a climb to a rail a step up; else `shape`, as it
    /// was), set where `always` or it changed; then each rail it joins that may join it turns to
    /// it.
    fn place(mut self, rt: &mut Rt, level: V, powered: bool, always: bool, shape: &str) -> Self {
        let (n, s, w, e) = (
            step(self.pos, NORTH),
            step(self.pos, SOUTH),
            step(self.pos, WEST),
            step(self.pos, EAST),
        );
        let north = self.has_neighbor_rail(rt, level, n);
        let south = self.has_neighbor_rail(rt, level, s);
        let west = self.has_neighbor_rail(rt, level, w);
        let east = self.has_neighbor_rail(rt, level, e);
        let (along_z, along_x) = (north || south, west || east);
        let mut railshape: Option<&'static str> = None;
        if along_z && !along_x {
            railshape = Some("north_south");
        }
        if along_x && !along_z {
            railshape = Some("east_west");
        }
        let (south_east, south_west, north_east, north_west) =
            (south && east, south && west, north && east, north && west);
        if !self.straight {
            if south_east && !north && !west {
                railshape = Some("south_east");
            }
            if south_west && !north && !east {
                railshape = Some("south_west");
            }
            if north_west && !south && !east {
                railshape = Some("north_west");
            }
            if north_east && !south && !west {
                railshape = Some("north_east");
            }
        }
        let mut chosen = railshape.map(str::to_owned);
        if railshape.is_none() {
            if along_z && along_x {
                chosen = Some(shape.to_owned());
            } else if along_z {
                chosen = Some("north_south".to_owned());
            } else if along_x {
                chosen = Some("east_west".to_owned());
            }
            if !self.straight {
                // At a junction of three the curve the last that joins gives: unpowered toward the
                // south and the east first, powered toward the north and the west.
                let order: [(bool, &str); 4] = if powered {
                    [
                        (south_east, "south_east"),
                        (south_west, "south_west"),
                        (north_east, "north_east"),
                        (north_west, "north_west"),
                    ]
                } else {
                    [
                        (north_west, "north_west"),
                        (north_east, "north_east"),
                        (south_west, "south_west"),
                        (south_east, "south_east"),
                    ]
                };
                for (joins, curve) in order {
                    if joins {
                        chosen = Some(curve.to_owned());
                    }
                }
            }
        }
        let sloped = match chosen.as_deref() {
            Some("north_south") => self.sloped(rt, Some("north_south")),
            Some("east_west") => self.sloped(rt, Some("east_west")),
            _ => None,
        };
        if let Some(s) = sloped {
            chosen = Some(s.to_owned());
        }
        let railshape = match chosen {
            Some(c) if self.valid(rt, &c) => c,
            _ => shape.to_owned(),
        };
        self.connections = connections(self.pos, &railshape);
        self.state = with(&self.state, &self.property, &railshape);
        if always || !same_state(&block_at(rt, self.pos), &self.state) {
            set_block(rt, level, self.pos, &self.state);
            for i in 0..self.connections.len() {
                if let Some(mut r) = get_rail(rt, level, self.connections[i]) {
                    r.remove_soft_connections(rt, level);
                    if r.can_connect_to(&self) {
                        r.connect_to(rt, level, self.pos);
                    }
                }
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{BlockFacts, MemoryHost, NeighborUpdate};

    /// Create's controller rail, cut down: straight, shaped by `RAIL_SHAPE_STRAIGHT`, with its own
    /// `updateDir` (which runs `super.updateDir`) and its own `updateState(state, level, pos,
    /// block)`, which takes the strongest signal beside it as its `power`.
    const CONTROLLER: &str = "test/ControllerRail";
    /// A mod's plain rail (FramedBlocks' fancy rail): `RailBlock`'s all through.
    const FANCY: &str = "test/FancyRail";
    /// A mod's powered rail (`PoweredRailBlock(properties, true)`): its vanilla class's all through.
    const BOOSTER: &str = "test/BoosterRail";
    /// A mod's activator rail (NeoForge's `PoweredRailBlock(properties)`).
    const SHOCK: &str = "test/ShockRail";
    /// A straight rail that never climbs (`canMakeSlopes` false: Create's cart assembler).
    const FLAT: &str = "test/FlatRail";

    fn overrides(class: &str, key: &str) -> bool {
        match class {
            CONTROLLER => [GET_SHAPE_PROPERTY.1, UPDATE_DIR.1, UPDATE_STATE.1].contains(&key),
            FLAT => [GET_SHAPE_PROPERTY.1, CAN_MAKE_SLOPES.1].contains(&key),
            _ => false,
        }
    }

    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let class = rt.class_of(args[0]);
        if !overrides(class, key) {
            return api::call(rt, owner_key, args);
        }
        match key {
            k if k == GET_SHAPE_PROPERTY.1 => {
                crate::api_common::vanilla_property_object(rt, "RAIL_SHAPE_STRAIGHT")
                    .unwrap_or(V::NULL)
            }
            k if k == CAN_MAKE_SLOPES.1 => V::I(0),
            // `super.updateDir(level, pos, state, alwaysPlace)`.
            k if k == UPDATE_DIR.1 => api::call(rt, UPDATE_DIR.0, args),
            k if k == UPDATE_STATE.1 => {
                let (state, level, pos) = (args[1], args[2], args[3]);
                let signal = api::call(rt, "net/minecraft/world/level/Level.getBestNeighborSignal.(Lnet/minecraft/core/BlockPos;)I", &[level, pos]).i().to_string();
                let text = crate::api_common::text_of(rt, state);
                if prop(&text, "power") != Some(signal.as_str()) {
                    let p = api::pos_of(rt, pos);
                    set_block(rt, level, p, &with(&text, "power", &signal));
                }
                V::Void
            }
            _ => unreachable!("{key}"),
        }
    }

    const PROPERTIES_ONLY: &str =
        "(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V";

    /// The test's rails, each made by its vanilla class's constructor as the mods' make theirs,
    /// with the default state given.
    fn runtime() -> Rt {
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        for (class, base, init, flag, id, default) in [
            (
                CONTROLLER,
                BASE,
                "(ZLnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V",
                Some(1),
                "test:controller_rail",
                "backwards=false,power=0,shape=north_south,waterlogged=false",
            ),
            (
                FANCY,
                RAIL,
                PROPERTIES_ONLY,
                None,
                "test:fancy_rail",
                "shape=north_south,waterlogged=false",
            ),
            (
                BOOSTER,
                POWERED,
                "(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;Z)V",
                Some(1),
                "test:booster_rail",
                "powered=false,shape=north_south,waterlogged=false",
            ),
            (
                SHOCK,
                POWERED,
                PROPERTIES_ONLY,
                None,
                "test:shock_rail",
                "powered=false,shape=north_south,waterlogged=false",
            ),
            (
                FLAT,
                BASE,
                "(ZLnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V",
                Some(1),
                "test:flat_rail",
                "shape=north_south,waterlogged=false",
            ),
        ] {
            rt.supers.insert(class, base);
            let b = rt.new_object(class);
            let props = rt.new_object(crate::blocks::BLOCK_PROPERTIES);
            let key = rt.static_key(&format!("{base}.<init>.{init}"));
            let args = match (base, flag) {
                (BASE, Some(f)) => vec![b, V::I(f), props],
                (_, Some(f)) => vec![b, props, V::I(f)],
                _ => vec![b, props],
            };
            api::call(&mut rt, key, &args);
            rt.registered.insert(("block".into(), id.into()), b);
            let d = rt.string(default);
            rt.put_field(b, "$default", d);
        }
        rt
    }

    const AT: [i32; 3] = [0, 1, 0];

    /// A floor of stone (every face sturdy) at y 0, air above; the test's rails and vanilla's in
    /// `#minecraft:rails`.
    fn bench() -> MemoryHost {
        let mut host = MemoryHost::default();
        host.facts.insert(
            "minecraft:stone".into(),
            BlockFacts {
                hardness: 1.5,
                push: 0,
                replaceable: false,
                sturdy: u32::MAX,
            },
        );
        host.facts.insert(
            "minecraft:air".into(),
            BlockFacts {
                hardness: 0.0,
                push: 1,
                replaceable: true,
                sturdy: 0,
            },
        );
        for x in -12..=12 {
            for z in -12..=12 {
                host.blocks.insert([x, 0, z], "minecraft:stone".into());
            }
        }
        let rails = [
            "minecraft:rail",
            "minecraft:powered_rail",
            "minecraft:activator_rail",
            "minecraft:detector_rail",
            "test:controller_rail",
            "test:fancy_rail",
            "test:booster_rail",
            "test:shock_rail",
            "test:flat_rail",
        ];
        host.block_tags
            .insert(RAILS.into(), rails.map(str::to_owned).to_vec());
        host
    }

    fn at(host: &MemoryHost, p: [i32; 3]) -> &str {
        host.blocks.get(&p).map_or("minecraft:air", String::as_str)
    }

    /// The game putting `state` at `p`, as `LevelChunk.setBlockState` tells the blocks.
    fn place(rt: &mut Rt, host: &mut MemoryHost, p: [i32; 3], state: &str) {
        let old = at(host, p).to_owned();
        host.blocks.insert(p, state.into());
        crate::glue::block_replaced(rt, host, p, &old, state, false);
    }

    /// The block at `p` hears that `changed` changed beside it.
    fn neighbor_changed(rt: &mut Rt, host: &mut MemoryHost, p: [i32; 3], changed: &str) -> bool {
        let state = at(host, p).to_owned();
        crate::glue::neighbor_changed(rt, host, p, &state, [p[0] - 1, p[1], p[2]], changed, false)
    }

    fn vanilla_rail(shape: &str) -> String {
        format!("minecraft:rail[shape={shape},waterlogged=false]")
    }

    /// Placed between a rail to its east and one to its south, a mod's plain rail curves to join
    /// both, and they turn to it, as vanilla's `RailState.place` and `connectTo` have it.
    #[test]
    fn a_mod_rail_curves_to_join_the_rails_beside_it_and_they_turn_to_it() {
        let mut rt = runtime();
        let mut host = bench();
        host.blocks.insert([1, 1, 0], vanilla_rail("north_south"));
        host.blocks.insert([0, 1, 1], vanilla_rail("east_west"));
        place(
            &mut rt,
            &mut host,
            AT,
            "test:fancy_rail[shape=north_south,waterlogged=false]",
        );
        assert_eq!(
            at(&host, AT),
            "test:fancy_rail[shape=south_east,waterlogged=false]"
        );
        assert_eq!(at(&host, [1, 1, 0]), vanilla_rail("east_west"));
        assert_eq!(at(&host, [0, 1, 1]), vanilla_rail("north_south"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// At a junction of three a plain rail curves toward the south and the east; once a signal
    /// source beside it changes while a signal reaches it, toward the north (vanilla's `RailBlock.updateState`). A change
    /// of a block that gives no signal leaves it.
    #[test]
    fn a_junction_rail_turns_as_a_signal_switches_it() {
        let mut rt = runtime();
        let mut host = bench();
        for p in [[0, 1, -1], [1, 1, 0], [0, 1, 1]] {
            host.blocks.insert(p, vanilla_rail("north_south"));
        }
        place(
            &mut rt,
            &mut host,
            AT,
            "test:fancy_rail[shape=north_south,waterlogged=false]",
        );
        assert_eq!(
            at(&host, AT),
            "test:fancy_rail[shape=south_east,waterlogged=false]"
        );
        host.signals.insert(AT, 15);
        neighbor_changed(&mut rt, &mut host, AT, "minecraft:stone");
        assert_eq!(
            at(&host, AT),
            "test:fancy_rail[shape=south_east,waterlogged=false]",
            "stone gives no signal"
        );
        neighbor_changed(&mut rt, &mut host, AT, "minecraft:lever");
        assert_eq!(
            at(&host, AT),
            "test:fancy_rail[shape=north_east,waterlogged=false]"
        );
        assert_eq!(at(&host, [0, 1, -1]), vanilla_rail("north_south"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A rail with a rail a step up beside it climbs to it, unless its class says it never climbs.
    #[test]
    fn a_rail_climbs_to_a_rail_a_step_up_unless_it_makes_no_slopes() {
        for (rail, shape) in [
            ("test:fancy_rail", "ascending_east"),
            ("test:flat_rail", "east_west"),
        ] {
            let mut rt = runtime();
            let mut host = bench();
            host.blocks.insert([1, 1, 0], "minecraft:stone".into());
            host.blocks.insert([1, 2, 0], vanilla_rail("north_south"));
            place(
                &mut rt,
                &mut host,
                AT,
                &format!("{rail}[shape=north_south,waterlogged=false]"),
            );
            assert_eq!(
                at(&host, AT),
                format!("{rail}[shape={shape},waterlogged=false]")
            );
            assert_eq!(at(&host, [1, 2, 0]), vanilla_rail("east_west"), "{rail}");
        }
    }

    /// Placed beside a block giving a signal, Create's controller rail takes it as its power at once
    /// (`onPlace` → `updateState` → `updateDir`, then, straight, its own `neighborChanged` →
    /// its own `updateState`), and follows it as it changes.
    #[test]
    fn a_controller_rail_takes_the_signal_beside_it_once_placed_and_as_it_changes() {
        let mut rt = runtime();
        let mut host = bench();
        host.signals.insert(AT, 15);
        place(
            &mut rt,
            &mut host,
            AT,
            "test:controller_rail[backwards=false,power=0,shape=north_south,waterlogged=false]",
        );
        assert_eq!(
            at(&host, AT),
            "test:controller_rail[backwards=false,power=15,shape=north_south,waterlogged=false]"
        );
        host.signals.insert(AT, 6);
        assert!(neighbor_changed(
            &mut rt,
            &mut host,
            AT,
            "minecraft:redstone_wire"
        ));
        assert_eq!(
            at(&host, AT),
            "test:controller_rail[backwards=false,power=6,shape=north_south,waterlogged=false]"
        );
        host.signals.remove(&AT);
        neighbor_changed(&mut rt, &mut host, AT, "minecraft:redstone_block");
        assert_eq!(
            at(&host, AT),
            "test:controller_rail[backwards=false,power=0,shape=north_south,waterlogged=false]"
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A rail drops once the block below no longer holds it up, and a climbing one once the block on
    /// its high side doesn't; the game asks its survival of it ([`crate::glue::can_survive`]).
    #[test]
    fn a_rail_drops_without_a_rigid_block_under_it_or_under_its_climb() {
        let mut rt = runtime();
        let mut host = bench();
        let flat =
            "test:controller_rail[backwards=false,power=0,shape=north_south,waterlogged=false]";
        host.blocks.insert(AT, flat.into());
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut host, AT, flat),
            Some(true)
        );
        assert!(crate::blocks::survival_blocks(&rt).contains(&"test:controller_rail".to_owned()));
        host.blocks.insert([0, 0, 0], "minecraft:air".into());
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut host, AT, flat),
            Some(false)
        );
        neighbor_changed(&mut rt, &mut host, AT, "minecraft:stone");
        assert_eq!(at(&host, AT), "minecraft:air");

        let climbing = "test:fancy_rail[shape=ascending_east,waterlogged=false]";
        let mut host = bench();
        host.blocks.insert([1, 1, 0], "minecraft:stone".into());
        host.blocks.insert(AT, climbing.into());
        neighbor_changed(&mut rt, &mut host, AT, "minecraft:stone");
        assert_eq!(at(&host, AT), climbing, "held up on both sides");
        host.blocks.insert([1, 1, 0], "minecraft:air".into());
        neighbor_changed(&mut rt, &mut host, AT, "minecraft:stone");
        assert_eq!(at(&host, AT), "minecraft:air");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Removed (or only changed), a straight rail tells the blocks around it and around the one
    /// below it, a climbing one first those around the one above; a plain flat rail tells none,
    /// and nor does one a piston moves.
    #[test]
    fn a_removed_rail_tells_the_blocks_around_it() {
        let around = |pos| NeighborUpdate::Around { pos, skip: None };
        for (state, moved, expected) in [
            (
                "test:controller_rail[backwards=false,power=0,shape=north_south,waterlogged=false]",
                false,
                vec![around(AT), around([0, 0, 0])],
            ),
            (
                "test:controller_rail[backwards=false,power=0,shape=ascending_east,waterlogged=false]",
                false,
                vec![around([0, 2, 0]), around(AT), around([0, 0, 0])],
            ),
            (
                "test:fancy_rail[shape=north_south,waterlogged=false]",
                false,
                vec![],
            ),
            (
                "test:controller_rail[backwards=false,power=0,shape=north_south,waterlogged=false]",
                true,
                vec![],
            ),
        ] {
            let mut rt = runtime();
            let mut host = bench();
            host.blocks.insert(AT, "minecraft:air".into());
            crate::glue::block_replaced(&mut rt, &mut host, AT, state, "minecraft:air", moved);
            let block = split_state(state).0.to_owned();
            assert_eq!(
                host.neighbor_updates,
                expected
                    .into_iter()
                    .map(|u| (u, block.clone()))
                    .collect::<Vec<_>>(),
                "{state} moved {moved}"
            );
        }
    }

    /// A mod's powered rail comes on while a signal reaches a powered rail of its kind in line with
    /// it, up to eight rails away, and not through an activator rail; its own activator rail
    /// (NeoForge's one-argument constructor) isn't of its kind.
    #[test]
    fn a_powered_rail_takes_the_signal_along_its_line_of_its_kind() {
        let line = |host: &mut MemoryHost, rail: &str, to: i32| {
            for x in 0..=to {
                host.blocks.insert(
                    [x, 1, 0],
                    format!("{rail}[powered=false,shape=east_west,waterlogged=false]"),
                );
            }
        };
        let powered =
            |host: &MemoryHost, x: i32| prop(at(host, [x, 1, 0]), "powered") == Some("true");
        let mut rt = runtime();
        let mut host = bench();
        line(&mut host, "test:booster_rail", 9);
        host.signals.insert([0, 1, 0], 15);
        for x in [8, 9] {
            neighbor_changed(&mut rt, &mut host, [x, 1, 0], "minecraft:redstone_block");
        }
        assert!(powered(&host, 8), "eight rails along");
        assert!(!powered(&host, 9), "nine is too far");

        let mut host = bench();
        line(&mut host, "test:booster_rail", 8);
        host.blocks.insert(
            [4, 1, 0],
            "minecraft:activator_rail[powered=false,shape=east_west,waterlogged=false]".into(),
        );
        host.signals.insert([0, 1, 0], 15);
        neighbor_changed(&mut rt, &mut host, [8, 1, 0], "minecraft:redstone_block");
        assert!(!powered(&host, 8), "an activator rail breaks the line");

        let mut host = bench();
        line(&mut host, "test:booster_rail", 7);
        host.blocks.insert(
            [8, 1, 0],
            "test:shock_rail[powered=false,shape=east_west,waterlogged=false]".into(),
        );
        host.signals.insert([0, 1, 0], 15);
        neighbor_changed(&mut rt, &mut host, [8, 1, 0], "minecraft:redstone_block");
        assert!(
            !powered(&host, 8),
            "an activator rail takes no power from powered rails"
        );
        neighbor_changed(&mut rt, &mut host, [7, 1, 0], "minecraft:redstone_block");
        assert!(powered(&host, 7));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Placed by a player, a rail lies along the way they look, and is waterlogged in a water
    /// source (`BaseRailBlock.getStateForPlacement`).
    #[test]
    fn a_placed_rail_lies_along_the_look_and_holds_the_water_it_goes_in() {
        let mut rt = runtime();
        let default = "test:fancy_rail[shape=north_south,waterlogged=false]";
        let placed = |rt: &mut Rt, host: &mut MemoryHost, yaw: f32| {
            let placement = crate::glue::Placement {
                pos: AT,
                hit_pos: [0, 0, 0],
                face: 1,
                yaw,
                ..Default::default()
            };
            crate::glue::state_for_placement(rt, host, default, &placement)
        };
        let mut host = bench();
        assert_eq!(
            placed(&mut rt, &mut host, 90.0),
            Some(Some(
                "test:fancy_rail[shape=east_west,waterlogged=false]".into()
            )),
            "looking west"
        );
        assert_eq!(
            placed(&mut rt, &mut host, 0.0),
            Some(Some(default.into())),
            "looking south"
        );
        host.blocks.insert(AT, "minecraft:water[level=0]".into());
        assert_eq!(
            placed(&mut rt, &mut host, 0.0),
            Some(Some(
                "test:fancy_rail[shape=north_south,waterlogged=true]".into()
            ))
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
