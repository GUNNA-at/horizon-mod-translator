//! Vanilla's palettes as a mod uses them itself: Create saves a contraption's blocks with a
//! `HashMapPalette` (each distinct state once, every block by its index in it) and sends them to
//! clients so, reading the states back into the palette's `values` (a
//! `CrudeIncrementalIntIdentityHashBiMap`) before the blocks that index them.
use crate::{Data, Rt, V};

const HASH_MAP_PALETTE: &str = "net/minecraft/world/level/chunk/HashMapPalette";
const BI_MAP: &str = "net/minecraft/util/CrudeIncrementalIntIdentityHashBiMap";

fn new_bi_map(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc(BI_MAP, Data::List(items))
}

fn items(rt: &Rt, map: V) -> Vec<V> {
    match rt.heap.get(map.r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.clone(),
        _ => Vec::new(),
    }
}

fn items_mut(rt: &mut Rt, map: V) -> Option<&mut Vec<V>> {
    match rt.heap.get_mut(map.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(v)) => Some(v),
        _ => None,
    }
}

/// The value's id: by identity, as vanilla's map keys (a state is one object per state there; here
/// two objects can stand for the same state, so they compare as Java's `equals` would).
fn id_of(rt: &Rt, map: V, v: V) -> Option<usize> {
    items(rt, map).iter().position(|&x| x == v || (x.r() != 0 && v.r() != 0 && rt.java_equals(x, v)))
}

fn add(rt: &mut Rt, map: V, v: V) -> i32 {
    match items_mut(rt, map) {
        Some(list) => {
            list.push(v);
            list.len() as i32 - 1
        }
        None => -1,
    }
}

fn by_id(rt: &Rt, map: V, i: i32) -> V {
    usize::try_from(i).ok().and_then(|i| items(rt, map).get(i).copied()).unwrap_or(V::NULL)
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, _desc) = rest.split_once('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // The ids of every block state: the palette's fallback, which nothing here reads.
        ("net/neoforged/neoforge/registries/GameData", "getBlockStateIDMap") => {
            rt.intern("compat:block-state-ids".into(), |rt| rt.new_object("net/minecraft/core/IdMapper"))
        }
        (HASH_MAP_PALETTE, "<init>") => {
            let values = new_bi_map(rt, Vec::new());
            rt.put_field(a(0), "values", values);
            V::Void
        }
        (HASH_MAP_PALETTE, _) => {
            let values = rt.get_field(a(0), "values");
            match name {
                // A new value takes the next id (a full palette would resize; this one grows).
                "idFor" => V::I(match id_of(rt, values, a(1)) {
                    Some(i) => i as i32,
                    None => add(rt, values, a(1)),
                }),
                "valueFor" => {
                    let v = by_id(rt, values, a(1).i());
                    if v.r() == 0 {
                        rt.throw_new("net/minecraft/world/level/chunk/MissingPaletteEntryException");
                    }
                    v
                }
                "getSize" => V::I(items(rt, values).len() as i32),
                "copy" => {
                    let p = rt.new_object(HASH_MAP_PALETTE);
                    let kept = items(rt, values);
                    let copy = new_bi_map(rt, kept);
                    rt.put_field(p, "values", copy);
                    p
                }
                _ => return None,
            }
        }
        (BI_MAP, "create") => new_bi_map(rt, Vec::new()),
        (BI_MAP, "<init>") => V::Void,
        // `add` gives the next id without looking for the value, as vanilla's does.
        (BI_MAP, "add") => V::I(add(rt, a(0), a(1))),
        (BI_MAP, "getId") => V::I(id_of(rt, a(0), a(1)).map_or(-1, |i| i as i32)),
        (BI_MAP, "byId") => by_id(rt, a(0), a(1).i()),
        (BI_MAP, "contains") => V::I(id_of(rt, a(0), a(1)).is_some() as i32),
        (BI_MAP, "size") => V::I(items(rt, a(0)).len() as i32),
        (BI_MAP, "clear") => {
            if let Some(list) = items_mut(rt, a(0)) {
                list.clear();
            }
            V::Void
        }
        (BI_MAP, "copy") => {
            let kept = items(rt, a(0));
            new_bi_map(rt, kept)
        }
        (BI_MAP, "iterator") => {
            let kept = items(rt, a(0));
            crate::jutil::new_iter(rt, kept)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PALETTE_INIT: &str = "net/minecraft/world/level/chunk/HashMapPalette.<init>.(Lnet/minecraft/core/IdMap;ILnet/minecraft/world/level/chunk/PaletteResize;)V";
    const ID_FOR: &str = "net/minecraft/world/level/chunk/HashMapPalette.idFor.(Ljava/lang/Object;)I";
    const VALUE_FOR: &str = "net/minecraft/world/level/chunk/HashMapPalette.valueFor.(I)Ljava/lang/Object;";
    const SIZE: &str = "net/minecraft/world/level/chunk/HashMapPalette.getSize.()I";

    fn palette(rt: &mut Rt) -> V {
        let ids = crate::api::call(rt, "net/neoforged/neoforge/registries/GameData.getBlockStateIDMap.()Lnet/minecraft/core/IdMapper;", &[]);
        let p = rt.new_object(HASH_MAP_PALETTE);
        crate::api::call(rt, PALETTE_INIT, &[p, ids, V::I(16), V::NULL]);
        p
    }

    /// Create's `writeBlocksCompound`: each distinct state once, in the order first met; the
    /// client's `readBlocksCompound` fills a palette's `values` from the list and reads each block's
    /// state by its index.
    #[test]
    fn a_palette_numbers_each_state_once_and_reads_back_by_number() {
        let mut rt = Rt::new();
        let p = palette(&mut rt);
        let plank = crate::api::state(&mut rt, "minecraft:oak_planks");
        let glass = crate::api::state(&mut rt, "minecraft:glass");
        let plank_again = crate::api::state(&mut rt, "minecraft:oak_planks");
        let ids: Vec<V> = [plank, glass, plank_again].iter().map(|&s| crate::api::call(&mut rt, ID_FOR, &[p, s])).collect();
        assert_eq!(ids, [V::I(0), V::I(1), V::I(0)]);
        assert_eq!(crate::api::call(&mut rt, SIZE, &[p]), V::I(2));

        // The client side: `palette.values.clear()`, then `values.add(state)` per palette entry.
        let q = palette(&mut rt);
        let values = rt.get_field(q, "values");
        crate::api::call(&mut rt, "net/minecraft/util/CrudeIncrementalIntIdentityHashBiMap.clear.()V", &[values]);
        for s in [plank, glass] {
            crate::api::call(&mut rt, "net/minecraft/util/CrudeIncrementalIntIdentityHashBiMap.add.(Ljava/lang/Object;)I", &[values, s]);
        }
        let second = crate::api::call(&mut rt, VALUE_FOR, &[q, V::I(1)]);
        assert_eq!(rt.str_of(second), Some("minecraft:glass"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // An index the palette doesn't hold throws, as vanilla's `MissingPaletteEntryException`.
        crate::api::call(&mut rt, VALUE_FOR, &[q, V::I(5)]);
        assert!(rt.thrown.is_some());
    }
}
