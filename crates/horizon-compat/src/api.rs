//! The Java API surface translated mods call, keyed by
//! `owner.name.descriptor` exactly as the class file names it. Each entry
//! is native Rust. Anything not here is counted in [`Rt::unmapped`] and
//! returns a zero value, so a mod runs with that one feature missing
//! instead of not running at all; `horizon-mod-translator coverage` lists
//! what to map next.
use crate::{Data, Rt, V};

/// `net/minecraft/core/Direction` ordinals, as vanilla orders them.
pub const DIRECTIONS: [[i64; 3]; 6] = [
    [0, -1, 0], // DOWN
    [0, 1, 0],  // UP
    [0, 0, -1], // NORTH
    [0, 0, 1],  // SOUTH
    [-1, 0, 0], // WEST
    [1, 0, 0],  // EAST
];
const OPPOSITE: [i64; 6] = [1, 0, 3, 2, 5, 4];
/// Direction ordinal → Axis ordinal (X 0, Y 1, Z 2).
const AXIS: [i32; 6] = [1, 1, 2, 2, 0, 0];

const BLOCK_POS: &str = "net/minecraft/core/BlockPos";
const DIRECTION: &str = "net/minecraft/core/Direction";

pub fn block_pos(rt: &mut Rt, x: i64, y: i64, z: i64) -> V {
    rt.alloc(BLOCK_POS, Data::Native("BlockPos", [x, y, z, 0]))
}

pub fn direction(rt: &mut Rt, ordinal: i64) -> V {
    rt.alloc(DIRECTION, Data::Native("Direction", [ordinal, 0, 0, 0]))
}

fn native(rt: &Rt, v: V) -> [i64; 4] {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => *h,
        _ => [0; 4],
    }
}

/// Calls the API method `key` (`owner.name.desc`). For instance methods
/// `args[0]` is the receiver.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match key {
        // java.lang
        "java/lang/Object.<init>.()V" => V::Void,
        "java/lang/Math.abs.(I)I" => V::I(a(0).i().wrapping_abs()),
        "java/lang/Math.max.(II)I" => V::I(a(0).i().max(a(1).i())),
        "java/lang/Math.min.(II)I" => V::I(a(0).i().min(a(1).i())),
        "java/lang/Math.abs.(F)F" => V::F(a(0).f().abs()),
        "java/lang/Math.abs.(D)D" => V::D(a(0).d().abs()),
        "java/lang/Math.max.(FF)F" => V::F(a(0).f().max(a(1).f())),
        "java/lang/Math.min.(FF)F" => V::F(a(0).f().min(a(1).f())),
        "java/lang/Math.max.(DD)D" => V::D(a(0).d().max(a(1).d())),
        "java/lang/Math.min.(DD)D" => V::D(a(0).d().min(a(1).d())),
        "java/lang/Math.sqrt.(D)D" => V::D(a(0).d().sqrt()),
        "java/lang/Math.floor.(D)D" => V::D(a(0).d().floor()),
        "java/lang/Math.ceil.(D)D" => V::D(a(0).d().ceil()),
        "java/lang/String.equals.(Ljava/lang/Object;)Z" => {
            V::I((rt.str_of(a(0)).is_some() && rt.str_of(a(0)) == rt.str_of(a(1))) as i32)
        }
        "java/lang/String.length.()I" => V::I(rt.str_of(a(0)).map_or(0, |s| s.encode_utf16().count() as i32)),
        "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;" => {
            let o = rt.new_object("java/lang/Integer");
            rt.put_field(o, "value", a(0));
            o
        }
        "java/lang/Integer.intValue.()I" => rt.get_field(a(0), "value"),
        "java/lang/Enum.ordinal.()I" => V::I(rt.get_field(a(0), "$ordinal").i()),

        // net.minecraft.core.BlockPos
        "net/minecraft/core/BlockPos.<init>.(III)V" => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Native("BlockPos", [a(1).i() as i64, a(2).i() as i64, a(3).i() as i64, 0]);
            }
            V::Void
        }
        "net/minecraft/core/BlockPos.getX.()I" | "net/minecraft/core/Vec3i.getX.()I" => V::I(native(rt, a(0))[0] as i32),
        "net/minecraft/core/BlockPos.getY.()I" | "net/minecraft/core/Vec3i.getY.()I" => V::I(native(rt, a(0))[1] as i32),
        "net/minecraft/core/BlockPos.getZ.()I" | "net/minecraft/core/Vec3i.getZ.()I" => V::I(native(rt, a(0))[2] as i32),
        "net/minecraft/core/BlockPos.above.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 1, 0], 1),
        "net/minecraft/core/BlockPos.below.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, -1, 0], 1),
        "net/minecraft/core/BlockPos.north.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 0, -1], 1),
        "net/minecraft/core/BlockPos.south.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [0, 0, 1], 1),
        "net/minecraft/core/BlockPos.west.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [-1, 0, 0], 1),
        "net/minecraft/core/BlockPos.east.()Lnet/minecraft/core/BlockPos;" => offset(rt, a(0), [1, 0, 0], 1),
        "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction;)Lnet/minecraft/core/BlockPos;" => {
            let d = DIRECTIONS[native(rt, a(1))[0].clamp(0, 5) as usize];
            offset(rt, a(0), d, 1)
        }
        "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction;I)Lnet/minecraft/core/BlockPos;" => {
            let d = DIRECTIONS[native(rt, a(1))[0].clamp(0, 5) as usize];
            offset(rt, a(0), d, a(2).i() as i64)
        }
        "net/minecraft/core/BlockPos.offset.(III)Lnet/minecraft/core/BlockPos;" => {
            offset(rt, a(0), [a(1).i() as i64, a(2).i() as i64, a(3).i() as i64], 1)
        }
        "net/minecraft/core/BlockPos.equals.(Ljava/lang/Object;)Z" => {
            V::I((native(rt, a(0))[..3] == native(rt, a(1))[..3]) as i32)
        }

        // net.minecraft.core.Direction
        "net/minecraft/core/Direction.getOpposite.()Lnet/minecraft/core/Direction;" => {
            let o = OPPOSITE[native(rt, a(0))[0].clamp(0, 5) as usize];
            direction(rt, o)
        }
        "net/minecraft/core/Direction.ordinal.()I" | "net/minecraft/core/Direction.get3DDataValue.()I" => {
            V::I(native(rt, a(0))[0] as i32)
        }
        "net/minecraft/core/Direction.getStepX.()I" => V::I(DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize][0] as i32),
        "net/minecraft/core/Direction.getStepY.()I" => V::I(DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize][1] as i32),
        "net/minecraft/core/Direction.getStepZ.()I" => V::I(DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize][2] as i32),
        "net/minecraft/core/Direction.getAxis.()Lnet/minecraft/core/Direction$Axis;" => {
            let ax = AXIS[native(rt, a(0))[0].clamp(0, 5) as usize];
            rt.alloc("net/minecraft/core/Direction$Axis", Data::Native("Axis", [ax as i64, 0, 0, 0]))
        }
        _ => {
            *rt.unmapped.entry(key).or_insert(0) += 1;
            V::Void
        }
    }
}

fn offset(rt: &mut Rt, p: V, d: [i64; 3], n: i64) -> V {
    let h = native(rt, p);
    block_pos(rt, h[0] + d[0] * n, h[1] + d[1] * n, h[2] + d[2] * n)
}

/// Static fields of API classes (`Direction.UP`...).
pub fn get_static(rt: &mut Rt, key: &'static str) -> V {
    let dir = |rt: &mut Rt, o| direction(rt, o);
    match key {
        "net/minecraft/core/Direction.DOWN" => dir(rt, 0),
        "net/minecraft/core/Direction.UP" => dir(rt, 1),
        "net/minecraft/core/Direction.NORTH" => dir(rt, 2),
        "net/minecraft/core/Direction.SOUTH" => dir(rt, 3),
        "net/minecraft/core/Direction.WEST" => dir(rt, 4),
        "net/minecraft/core/Direction.EAST" => dir(rt, 5),
        "net/minecraft/core/BlockPos.ZERO" => block_pos(rt, 0, 0, 0),
        _ => {
            *rt.unmapped.entry(key).or_insert(0) += 1;
            V::NULL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_pos_relative_and_opposite() {
        let mut rt = Rt::new();
        let p = block_pos(&mut rt, 10, 64, -3);
        let north = get_static(&mut rt, "net/minecraft/core/Direction.NORTH");
        let q = call(&mut rt, "net/minecraft/core/BlockPos.relative.(Lnet/minecraft/core/Direction;I)Lnet/minecraft/core/BlockPos;", &[p, north, V::I(4)]);
        assert_eq!(call(&mut rt, "net/minecraft/core/BlockPos.getZ.()I", &[q]), V::I(-7));
        let south = call(&mut rt, "net/minecraft/core/Direction.getOpposite.()Lnet/minecraft/core/Direction;", &[north]);
        assert_eq!(call(&mut rt, "net/minecraft/core/Direction.getStepZ.()I", &[south]), V::I(1));
    }

    #[test]
    fn unmapped_calls_are_counted() {
        let mut rt = Rt::new();
        call(&mut rt, "net/minecraft/Nope.x.()V", &[]);
        call(&mut rt, "net/minecraft/Nope.x.()V", &[]);
        assert_eq!(rt.unmapped["net/minecraft/Nope.x.()V"], 2);
    }
}
