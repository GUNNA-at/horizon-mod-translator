//! Vanilla's `Rotation` and `Mirror`: how they turn a direction or a 16-step rotation, and how they
//! turn a block state (`BlockState.rotate`, `BlockState.mirror`). A mod block's own `rotate`/`mirror`
//! answers for its states; a block without one does what its vanilla base class does: `Block`
//! leaves the state as it is, the others turn the properties vanilla's blocks share (`facing`,
//! `axis`, `rotation`, the four sides' connections, a rail's `shape`). Create turns each block of a
//! contraption this way when it puts the contraption back down at an angle.
use crate::{Data, Rt, V};

pub const ROTATION: &str = "net/minecraft/world/level/block/Rotation";
pub const MIRROR: &str = "net/minecraft/world/level/block/Mirror";
/// Field names in order (ordinal): quarter turns clockwise.
pub const ROTATIONS: [&str; 4] = ["none", "clockwise_90", "clockwise_180", "counterclockwise_90"];
pub const MIRRORS: [&str; 3] = ["none", "left_right", "front_back"];
/// `Rotation.getSerializedName()`.
pub(crate) const ROTATION_NAMES: [&str; 4] = ["none", "clockwise_90", "180", "counterclockwise_90"];

const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
/// North, east, south, west: a quarter turn clockwise is one step along.
const RING: [usize; 4] = [2, 5, 3, 4];
const RAIL_SHAPES: [&str; 10] =
    ["north_south", "east_west", "ascending_east", "ascending_west", "ascending_north", "ascending_south", "south_east", "south_west", "north_west", "north_east"];

const BLOCK_ROTATE: &str = "rotate.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/world/level/block/state/BlockState;";
const BLOCK_ROTATE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.rotate.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/world/level/block/state/BlockState;";
const BLOCK_MIRROR: &str = "mirror.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/Mirror;)Lnet/minecraft/world/level/block/state/BlockState;";
const BLOCK_MIRROR_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.mirror.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/Mirror;)Lnet/minecraft/world/level/block/state/BlockState;";

fn ordinal(rt: &Rt, v: V) -> usize {
    rt.get_field(v, "$ordinal").i().max(0) as usize
}

fn direction_ordinal(rt: &Rt, v: V) -> usize {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => h[0].clamp(0, 5) as usize,
        _ => 2,
    }
}

/// Direction ordinal `d` turned `quarters` times clockwise (seen from above); up and down stay.
fn rotate_dir(d: usize, quarters: usize) -> usize {
    match RING.iter().position(|&r| r == d) {
        Some(i) => RING[(i + quarters) % 4],
        None => d,
    }
}

/// Direction ordinal `d` in mirror `m`: left-right swaps north and south, front-back west and east.
fn mirror_dir(d: usize, m: usize) -> usize {
    match (m, d) {
        (1, 2) => 3,
        (1, 3) => 2,
        (2, 4) => 5,
        (2, 5) => 4,
        _ => d,
    }
}

/// `Rotation.rotate(rotation, count)`: a step of a `count`-step turn (a sign's 16).
fn rotate_steps(q: usize, rotation: i32, count: i32) -> i32 {
    if count <= 0 {
        return rotation;
    }
    match q {
        1 => (rotation + count / 4) % count,
        2 => (rotation + count / 2) % count,
        3 => (rotation + count * 3 / 4) % count,
        _ => rotation,
    }
}

/// `Mirror.mirror(rotation, count)`.
fn mirror_steps(m: usize, rotation: i32, count: i32) -> i32 {
    if count <= 0 {
        return rotation;
    }
    let half = count / 2;
    let r = if rotation > half { rotation - count } else { rotation };
    match m {
        1 => (half - r + count) % count,
        2 => (count - r) % count,
        _ => rotation,
    }
}

/// Each direction named in `value` (`north_east`, `up_west`) changed by `f`; a rail shape is then
/// named as vanilla's `RailShape` names it.
fn turn_names(value: &str, f: impl Fn(usize) -> usize) -> String {
    let turned: Vec<String> = value
        .split('_')
        .map(|part| match DIRECTIONS.iter().position(|d| *d == part) {
            Some(d) => DIRECTIONS[f(d)].to_owned(),
            None => part.to_owned(),
        })
        .collect();
    let joined = turned.join("_");
    if RAIL_SHAPES.contains(&value) && !RAIL_SHAPES.contains(&joined.as_str()) && turned.len() == 2 {
        return format!("{}_{}", turned[1], turned[0]);
    }
    joined
}

/// The state `text` turned the way vanilla's blocks turn their shared properties: `quarters`
/// clockwise, then mirror `m`.
pub(crate) fn transform_text(text: &str, quarters: usize, m: usize) -> String {
    let (block, props) = crate::host::split_state(text);
    let turn = |d: usize| if m == 0 { rotate_dir(d, quarters) } else { mirror_dir(d, m) };
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in &props {
        let (k, v) = (*k, *v);
        let new = match k {
            "facing" | "orientation" | "vertical_direction" => (k.to_owned(), turn_names(v, turn)),
            "axis" if m == 0 && quarters % 2 == 1 => (k.to_owned(), match v {
                "x" => "z".to_owned(),
                "z" => "x".to_owned(),
                other => other.to_owned(),
            }),
            "rotation" => match v.parse::<i32>() {
                Ok(r) => (k.to_owned(), if m == 0 { rotate_steps(quarters, r, 16) } else { mirror_steps(m, r, 16) }.to_string()),
                Err(_) => (k.to_owned(), v.to_owned()),
            },
            "north" | "east" | "south" | "west" => {
                let d = DIRECTIONS.iter().position(|n| *n == k).unwrap_or(2);
                (DIRECTIONS[turn(d)].to_owned(), v.to_owned())
            }
            "shape" if RAIL_SHAPES.contains(&v) => (k.to_owned(), turn_names(v, turn)),
            _ => (k.to_owned(), v.to_owned()),
        };
        out.push(new);
    }
    crate::host::join_state(block, out.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect())
}

/// Whether the vanilla class `class` turns its states (`Block` and the bases that add nothing to
/// it leave them as they are).
fn vanilla_turns(class: &str) -> bool {
    !matches!(
        class,
        "net/minecraft/world/level/block/Block" | "net/minecraft/world/level/block/BaseEntityBlock" | "net/minecraft/world/level/block/state/BlockBehaviour"
    )
}

/// The first vanilla class above the mod's own `class`.
pub(crate) fn vanilla_base(rt: &Rt, mut class: &'static str) -> &'static str {
    for _ in 0..64 {
        match rt.supers.get(class).copied() {
            Some(up) if !up.starts_with("net/minecraft/") => class = up,
            Some(up) => return up,
            None => break,
        }
    }
    "net/minecraft/world/level/block/Block"
}

/// `state.rotate(rotation)` (`mirror` when `mirror` is set): the mod block's own answer where it has
/// one, else its vanilla base class's.
pub(crate) fn turn_state(rt: &mut Rt, state: V, how: V, mirror: bool) -> V {
    let Some(text) = rt.str_of(state).map(str::to_owned) else { return state };
    let n = ordinal(rt, how);
    if n == 0 {
        return state;
    }
    let (q, m) = if mirror { (0, n) } else { (n, 0) };
    if let Some((b, class)) = crate::glue::mod_block(rt, &text) {
        let (short, key) = if mirror { (BLOCK_MIRROR, BLOCK_MIRROR_KEY) } else { (BLOCK_ROTATE, BLOCK_ROTATE_KEY) };
        if rt.overrides.is_some_and(|o| o(class, short))
            && let Some(vc) = rt.vcall
        {
            let r = vc(rt, key, short, &[b, state, how]);
            return if r.r() == 0 { state } else { r };
        }
        if !vanilla_turns(vanilla_base(rt, class)) {
            return state;
        }
    }
    let t = transform_text(&text, q, m);
    crate::api::state(rt, &t)
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, ROTATION | MIRROR) || (matches!(name, "rotate" | "mirror") && owner.starts_with("net/minecraft/world/level/block/"))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    Some(match (owner, name) {
        (ROTATION, "rotate") if desc.starts_with("(Lnet/minecraft/core/Direction;)") => {
            let d = rotate_dir(direction_ordinal(rt, a(1)), ordinal(rt, a(0)));
            crate::api::direction(rt, d as i64)
        }
        (ROTATION, "rotate") if desc == "(II)I" => V::I(rotate_steps(ordinal(rt, a(0)), a(1).i(), a(2).i())),
        (ROTATION, "getRotated") => {
            let q = (ordinal(rt, a(0)) + ordinal(rt, a(1))) % 4;
            crate::blocks::vanilla_enum_constant(rt, ROTATION, ROTATIONS[q])?
        }
        (ROTATION, "getSerializedName") => {
            let n = ROTATION_NAMES[ordinal(rt, a(0)) % 4];
            rt.string(n)
        }
        (MIRROR, "mirror") if desc.starts_with("(Lnet/minecraft/core/Direction;)") => {
            let d = mirror_dir(direction_ordinal(rt, a(1)), ordinal(rt, a(0)));
            crate::api::direction(rt, d as i64)
        }
        (MIRROR, "mirror") if desc == "(II)I" => V::I(mirror_steps(ordinal(rt, a(0)), a(1).i(), a(2).i())),
        // A mirror across the direction's own axis turns it round; any other leaves it.
        (MIRROR, "getRotation") => {
            let (m, d) = (ordinal(rt, a(0)), direction_ordinal(rt, a(1)));
            let flips = (m == 1 && matches!(d, 2 | 3)) || (m == 2 && matches!(d, 4 | 5));
            crate::blocks::vanilla_enum_constant(rt, ROTATION, ROTATIONS[if flips { 2 } else { 0 }])?
        }
        (MIRROR, "getSerializedName") => {
            let n = MIRRORS[ordinal(rt, a(0)) % 3];
            rt.string(n)
        }
        // `super.rotate(state, rotation)` from a mod block reaching its vanilla base.
        (o, "rotate" | "mirror") if o.starts_with("net/minecraft/world/level/block/") && desc.starts_with("(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/") => {
            let (state, how) = (a(1), a(2));
            if !vanilla_turns(o) || ordinal(rt, how) == 0 {
                return Some(state);
            }
            let text = rt.str_of(state)?.to_owned();
            let n = ordinal(rt, how);
            let t = if name == "mirror" { transform_text(&text, 0, n) } else { transform_text(&text, n, 0) };
            crate::api::state(rt, &t)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against vanilla's blocks: a furnace facing north turned clockwise faces east, a log along x
    /// lies along z, a sign at 0 (south) is at 4 (west), a fence's north arm becomes its east one,
    /// a rail's curve turns with it; a left-right mirror swaps north and south.
    #[test]
    fn a_state_turns_the_properties_vanillas_blocks_turn() {
        assert_eq!(transform_text("minecraft:furnace[facing=north,lit=false]", 1, 0), "minecraft:furnace[facing=east,lit=false]");
        assert_eq!(transform_text("minecraft:oak_log[axis=x]", 1, 0), "minecraft:oak_log[axis=z]");
        assert_eq!(transform_text("minecraft:oak_log[axis=x]", 2, 0), "minecraft:oak_log[axis=x]");
        assert_eq!(transform_text("minecraft:oak_sign[rotation=0,waterlogged=false]", 1, 0), "minecraft:oak_sign[rotation=4,waterlogged=false]");
        assert_eq!(
            transform_text("minecraft:oak_fence[east=false,north=true,south=false,waterlogged=false,west=false]", 1, 0),
            "minecraft:oak_fence[east=true,north=false,south=false,waterlogged=false,west=false]"
        );
        assert_eq!(transform_text("minecraft:rail[shape=south_east,waterlogged=false]", 1, 0), "minecraft:rail[shape=south_west,waterlogged=false]");
        assert_eq!(transform_text("minecraft:rail[shape=north_south,waterlogged=false]", 3, 0), "minecraft:rail[shape=east_west,waterlogged=false]");
        assert_eq!(transform_text("minecraft:furnace[facing=north,lit=false]", 0, 1), "minecraft:furnace[facing=south,lit=false]");
        assert_eq!(transform_text("minecraft:furnace[facing=north,lit=false]", 0, 2), "minecraft:furnace[facing=north,lit=false]");
        // `Mirror.mirror(rotation, 16)`: left-right maps 0 (south) to 8 (north), front-back 4 (west) to 12 (east).
        assert_eq!(mirror_steps(1, 0, 16), 8);
        assert_eq!(mirror_steps(2, 4, 16), 12);
    }

    #[test]
    fn rotations_turn_directions_and_add_up() {
        let mut rt = Rt::new();
        let cw = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/Rotation.CLOCKWISE_90");
        let ccw = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/Rotation.COUNTERCLOCKWISE_90");
        let north = crate::api::get_static(&mut rt, "net/minecraft/core/Direction.NORTH");
        let key = "net/minecraft/world/level/block/Rotation.rotate.(Lnet/minecraft/core/Direction;)Lnet/minecraft/core/Direction;";
        let east = crate::api::call(&mut rt, key, &[cw, north]);
        assert_eq!(direction_ordinal(&rt, east), 5);
        let west = crate::api::call(&mut rt, key, &[ccw, north]);
        assert_eq!(direction_ordinal(&rt, west), 4);
        let sum = crate::api::call(&mut rt, "net/minecraft/world/level/block/Rotation.getRotated.(Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/world/level/block/Rotation;", &[cw, ccw]);
        let none = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/Rotation.NONE");
        assert_eq!(sum, none);
        let state = crate::api::state(&mut rt, "minecraft:furnace[facing=north,lit=false]");
        let turned = crate::api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.rotate.(Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/world/level/block/state/BlockState;", &[state, cw]);
        assert_eq!(rt.str_of(turned), Some("minecraft:furnace[facing=east,lit=false]"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
