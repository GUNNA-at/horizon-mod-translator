//! Vanilla's records (`StructureTemplate.StructureBlockInfo`, `ClipBlockStateContext`...), the ones no
//! other module models: a mod builds one with its canonical constructor and reads it back through its
//! accessors, compares it (`equals`, `hashCode`) and prints it. The components come from
//! [`crate::vanilla_records`], as each record's class file declares them. This is the last mapping a
//! call reaches: the records the runtime models natively (`TagKey`, `DimensionType`) answer first.
use crate::vanilla_records::RECORDS;
use crate::{Data, Rt, V};

/// The record `class`'s components (`name`, descriptor), in declaration order.
fn components(class: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let i = RECORDS.binary_search_by(|(c, _)| (*c).cmp(class)).ok()?;
    Some(RECORDS[i].1.split(',').filter_map(|c| c.split_once(':')).collect())
}

/// What an unset component reads as: Java's default for its type.
fn zero(desc: &str) -> V {
    match desc {
        "J" => V::J(0),
        "F" => V::F(0.0),
        "D" => V::D(0.0),
        "Z" | "B" | "C" | "S" | "I" => V::I(0),
        _ => V::NULL,
    }
}

/// Whether `v` is a record of `class` made here: its components are fields.
fn made_here(rt: &Rt, v: V, class: &str) -> bool {
    v.r() != 0 && rt.class_of(v) == class && matches!(rt.heap.get(v.r() as usize).map(|o| &o.data), Some(Data::Fields(_)))
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    RECORDS.binary_search_by(|(c, _)| (*c).cmp(owner)).is_ok()
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let comps = components(owner)?;
    let this = args.first().copied().unwrap_or_default();
    match name {
        // The canonical constructor: one argument per component, in order.
        "<init>" => {
            let params: String = comps.iter().map(|(_, d)| *d).collect();
            if desc != format!("({params})V") {
                return None;
            }
            for (i, (n, _)) in comps.iter().enumerate() {
                rt.put_field(this, n, args.get(i + 1).copied().unwrap_or_default());
            }
            Some(V::Void)
        }
        "equals" if desc == "(Ljava/lang/Object;)Z" && made_here(rt, this, owner) => {
            let other = args.get(1).copied().unwrap_or_default();
            let same = made_here(rt, other, owner)
                && comps.iter().all(|(n, _)| {
                    let (x, y) = (rt.get_field(this, n), rt.get_field(other, n));
                    x == y || (matches!(x, V::R(r) if r != 0) && crate::jutil::equals(rt, x, y))
                });
            Some(V::I(same as i32))
        }
        // Java's own rule for a record: `31 * h + hash(component)`, from the first.
        "hashCode" if desc == "()I" && made_here(rt, this, owner) => {
            let mut h = 0i32;
            for (n, _) in &comps {
                let v = rt.get_field(this, n);
                let c = match v {
                    V::R(0) | V::Void => 0,
                    V::R(_) => crate::jutil::hash(rt, v),
                    V::I(i) => i,
                    V::J(j) => (j ^ (j >> 32)) as i32,
                    V::F(f) => f.to_bits() as i32,
                    V::D(d) => {
                        let b = d.to_bits() as i64;
                        (b ^ (b >> 32)) as i32
                    }
                };
                h = h.wrapping_mul(31).wrapping_add(c);
            }
            Some(V::I(h))
        }
        // `StructureBlockInfo[pos=..., state=..., nbt=...]`, as a record prints itself.
        "toString" if desc == "()Ljava/lang/String;" && made_here(rt, this, owner) => {
            let simple = owner.rsplit(['/', '$']).next().unwrap_or(owner);
            let mut parts = Vec::new();
            for (n, _) in &comps {
                let v = rt.get_field(this, n);
                let text = match v {
                    V::R(0) | V::Void => "null".to_owned(),
                    V::R(_) => rt.str_of(v).map(str::to_owned).unwrap_or_else(|| rt.class_of(v).rsplit('/').next().unwrap_or("").to_owned()),
                    V::I(i) => i.to_string(),
                    V::J(j) => j.to_string(),
                    V::F(f) => f.to_string(),
                    V::D(d) => d.to_string(),
                };
                parts.push(format!("{n}={text}"));
            }
            let s = format!("{simple}[{}]", parts.join(", "));
            Some(rt.string(&s))
        }
        // An accessor: the component's value.
        n if made_here(rt, this, owner) => {
            let &(field, d) = comps.iter().find(|(c, _)| *c == n)?;
            if desc != format!("(){d}") {
                return None;
            }
            Some(match rt.get_field(this, field) {
                V::Void => zero(d),
                v => v,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo";

    /// Create's contraptions keep each block as a `StructureBlockInfo(pos, state, nbt)` and place it
    /// back from `info.pos()`, `info.state()` and `info.nbt()`.
    #[test]
    fn a_vanilla_record_reads_back_what_its_constructor_was_given() {
        let mut rt = Rt::new();
        let pos = crate::api::block_pos(&mut rt, 1, 65, -2);
        let state = crate::api::state(&mut rt, "minecraft:oak_planks");
        let info = rt.new_object(INFO);
        let init = "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo.<init>.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/nbt/CompoundTag;)V";
        crate::api::call(&mut rt, init, &[info, pos, state, V::NULL]);
        let get = |rt: &mut Rt, key: &'static str| crate::api::call(rt, key, &[info]);
        assert_eq!(get(&mut rt, "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo.pos.()Lnet/minecraft/core/BlockPos;"), pos);
        let s = get(&mut rt, "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo.state.()Lnet/minecraft/world/level/block/state/BlockState;");
        assert_eq!(rt.str_of(s), Some("minecraft:oak_planks"));
        assert_eq!(get(&mut rt, "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo.nbt.()Lnet/minecraft/nbt/CompoundTag;"), V::NULL);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);

        // Two records with equal components are equal, and hash alike.
        let twin = rt.new_object(INFO);
        let pos2 = crate::api::block_pos(&mut rt, 1, 65, -2);
        crate::api::call(&mut rt, init, &[twin, pos2, state, V::NULL]);
        let equals = "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo.equals.(Ljava/lang/Object;)Z";
        assert_eq!(crate::api::call(&mut rt, equals, &[info, twin]), V::I(1));
        let hash = "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo.hashCode.()I";
        assert_eq!(crate::api::call(&mut rt, hash, &[info]), crate::api::call(&mut rt, hash, &[twin]));
        let other = rt.new_object(INFO);
        let elsewhere = crate::api::block_pos(&mut rt, 0, 0, 0);
        crate::api::call(&mut rt, init, &[other, elsewhere, state, V::NULL]);
        assert_eq!(crate::api::call(&mut rt, equals, &[info, other]), V::I(0));
    }
}
