//! Vanilla's records (`StructureTemplate.StructureBlockInfo`, `ClipBlockStateContext`...), the ones no
//! other module models: a mod builds one with its canonical constructor and reads it back through its
//! accessors, compares it (`equals`, `hashCode`) and prints it. The components come from
//! [`crate::vanilla_records`], as each record's class file declares them. This is the last mapping a
//! call reaches: the records the runtime models natively (`TagKey`, `DimensionType`) answer first.
//!
//! The same for vanilla's plain data classes (`Tuple`, `LevelResource`, `ModelLayerLocation`): the
//! constructors that only store their arguments, the getters and setters that only read and write a
//! field, and `equals`/`hashCode` as the class has them (`Object`'s, or field by field), all read
//! from their bytecode. And an enum constant's `equals` and `hashCode`, `Enum`'s: identity.
use crate::vanilla_records::{DATA_CLASSES, ENUMS, ENUM_CONSTANTS, FACTORIES, RECORDS};
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
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    RECORDS.binary_search_by(|(c, _)| (*c).cmp(owner)).is_ok()
        || members(owner).is_some()
        || (matches!(name, "equals" | "hashCode" | "ordinal" | "name" | "compareTo") && ENUMS.binary_search(&owner).is_ok())
        || (matches!(name, "values" | "valueOf") && constants(owner).is_some())
        || FACTORIES.binary_search_by(|(c, _)| (*c).cmp(owner)).is_ok()
}

/// The constructor a static factory of `class` (a record's or data class's: `GlobalPos.of`) only
/// calls with its own arguments, by the factory's name and descriptor.
fn factory(class: &str, name: &str, desc: &str) -> Option<(&'static str, &'static str)> {
    let i = FACTORIES.binary_search_by(|(c, _)| (*c).cmp(class)).ok()?;
    let (c, list) = FACTORIES[i];
    list.split('|').find_map(|f| f.strip_prefix(name)?.strip_prefix(desc)?.strip_prefix('=')).map(|init| (c, init))
}

/// A record of `class` made from its components, in order (`GlobalPos::of` as a function).
pub(crate) fn make(rt: &mut Rt, class: &str, args: &[V]) -> V {
    let Ok(i) = RECORDS.binary_search_by(|(c, _)| (*c).cmp(class)) else { return V::NULL };
    let class = RECORDS[i].0;
    let o = rt.new_object(class);
    for (i, (n, _)) in components(class).unwrap_or_default().iter().enumerate() {
        rt.put_field(o, n, args.get(i).copied().unwrap_or_default());
    }
    o
}

/// A data class of the table built by its constructor `desc` from `args`, as a mod's `new` builds
/// one: a vanilla value the runtime hands out without modelling it (`new DataVersion(3955, "main")`).
pub(crate) fn make_data(rt: &mut Rt, class: &str, desc: &str, args: &[V]) -> V {
    let class: &'static str = rt.static_key(class);
    let o = rt.new_object(class);
    let key: &'static str = rt.static_key(&format!("{class}.<init>.{desc}"));
    let mut all = vec![o];
    all.extend_from_slice(args);
    call(rt, key, &all);
    o
}

/// The constants of a vanilla enum the runtime doesn't model by hand, in ordinal order.
fn constants(class: &str) -> Option<(&'static str, &'static str)> {
    let i = ENUM_CONSTANTS.binary_search_by(|(c, _)| (*c).cmp(class)).ok()?;
    Some(ENUM_CONSTANTS[i])
}

/// The constants of a vanilla enum the table lists, in ordinal order.
pub(crate) fn enum_names(class: &str) -> Option<(&'static str, Vec<&'static str>)> {
    let (c, names) = constants(class)?;
    Some((c, names.split(',').collect()))
}

/// `Enum.FIELD` of a vanilla enum the runtime doesn't model by hand (`NoteBlockInstrument.BASS`):
/// one object per constant, with its name and ordinal ([`crate::api::api_enum`]).
pub(crate) fn enum_constant(rt: &mut Rt, key: &str) -> Option<V> {
    let (owner, field) = key.rsplit_once('.')?;
    let (class, names) = constants(owner)?;
    let (ordinal, name) = names.split(',').enumerate().find(|(_, n)| *n == field)?;
    Some(crate::api::api_enum(rt, class, name, ordinal as i32))
}

/// A plain data class's members, as [`DATA_CLASSES`] lists them.
fn members(class: &str) -> Option<&'static str> {
    let i = DATA_CLASSES.binary_search_by(|(c, _)| (*c).cmp(class)).ok()?;
    Some(DATA_CLASSES[i].1)
}

/// The fields of the member of `kind` whose signature is `sig` (a constructor's descriptor, a
/// getter's or setter's name and descriptor).
fn member(members: &'static str, kind: char, sig: &str) -> Option<&'static str> {
    members.split('|').find_map(|m| m.strip_prefix(kind)?.strip_prefix(sig)?.strip_prefix('='))
}

/// Whether `v` is an object of the data class `class` (or a mod class extending it) built here.
fn data_object(rt: &Rt, v: V, class: &str) -> bool {
    v.r() != 0 && matches!(rt.heap.get(v.r() as usize).map(|o| &o.data), Some(Data::Fields(_))) && rt.is_a(rt.class_of(v), class)
}

/// A call on a plain data class: its pure constructors, getters and setters, its equality.
fn data_call(rt: &mut Rt, owner: &str, members: &'static str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    if !data_object(rt, this, owner) {
        return None;
    }
    let other = args.get(1).copied().unwrap_or_default();
    let fieldwise = members.split('|').find_map(|m| m.strip_prefix("e="));
    let identity = members.split('|').any(|m| m == "o");
    match (name, desc) {
        ("<init>", _) => {
            let fields = member(members, 'i', desc)?;
            for (i, f) in fields.split(',').filter(|f| !f.is_empty()).enumerate() {
                rt.put_field(this, f, args.get(i + 1).copied().unwrap_or_default());
            }
            Some(V::Void)
        }
        ("equals", "(Ljava/lang/Object;)Z") if identity => Some(V::I((this == other) as i32)),
        ("hashCode", "()I") if identity => Some(V::I(this.r() as i32)),
        ("equals", "(Ljava/lang/Object;)Z") => {
            let fields = fieldwise?;
            let same = this == other
                || (data_object(rt, other, owner)
                    && fields.split(',').all(|n| {
                        let (x, y) = (rt.get_field(this, n), rt.get_field(other, n));
                        x == y || (matches!(x, V::R(r) if r != 0) && crate::jutil::equals(rt, x, y))
                    }));
            Some(V::I(same as i32))
        }
        ("hashCode", "()I") => {
            let mut h = 0i32;
            for n in fieldwise?.split(',') {
                let v = rt.get_field(this, n);
                h = h.wrapping_mul(31).wrapping_add(hash_of(rt, v));
            }
            Some(V::I(h))
        }
        _ => {
            let sig = format!("{name}{desc}");
            if let Some(field) = member(members, 'g', &sig) {
                let ret = desc.strip_prefix("()")?;
                return Some(match rt.get_field(this, field) {
                    V::Void => zero(ret),
                    v => v,
                });
            }
            let field = member(members, 's', &sig)?;
            rt.put_field(this, field, other);
            Some(V::Void)
        }
    }
}

/// A component's or field's share of a hash, as `Objects.hashCode` and the boxes' `hashCode` give it.
fn hash_of(rt: &mut Rt, v: V) -> i32 {
    match v {
        V::R(0) | V::Void => 0,
        V::R(_) => crate::jutil::hash(rt, v),
        V::I(i) => i,
        V::J(j) => (j ^ (j >> 32)) as i32,
        V::F(f) => f.to_bits() as i32,
        V::D(d) => {
            let b = d.to_bits() as i64;
            (b ^ (b >> 32)) as i32
        }
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    // A static factory: the object its constructor makes of the same arguments.
    if let Some((class, init)) = factory(owner, name, desc) {
        let o = rt.new_object(class);
        let mut with_this = Vec::with_capacity(args.len() + 1);
        with_this.push(o);
        with_this.extend_from_slice(args);
        let init_key = rt.static_key(&format!("{class}.<init>.{init}"));
        crate::api::call(rt, init_key, &with_this);
        return Some(o);
    }
    if let Some(m) = members(owner) {
        return data_call(rt, owner, m, name, desc, args);
    }
    if ENUMS.binary_search(&owner).is_ok() {
        let (this, other) = (args.first().copied().unwrap_or_default(), args.get(1).copied().unwrap_or_default());
        return match (name, desc) {
            ("equals", "(Ljava/lang/Object;)Z") => Some(V::I((this == other) as i32)),
            ("hashCode", "()I") => Some(V::I(this.r() as i32)),
            // `Enum`'s final methods, named through the enum's own class (`switch` on a vanilla
            // enum reads `WallSide.ordinal()`).
            ("ordinal", "()I") => Some(V::I(crate::api_common::enum_ordinal(rt, this))),
            ("name", "()Ljava/lang/String;") => Some(crate::api_common::enum_name(rt, this, false)),
            ("compareTo", _) if desc.ends_with(")I") => {
                Some(V::I(crate::api_common::enum_ordinal(rt, this) - crate::api_common::enum_ordinal(rt, other)))
            }
            // `values()`: a new array of the constants each time, as Java clones it.
            ("values", _) if desc.starts_with("()[") => {
                let (class, names) = constants(owner)?;
                let all: Vec<V> = names.split(',').enumerate().map(|(i, n)| crate::api::api_enum(rt, class, n, i as i32)).collect();
                let array: &'static str = rt.static_key(&format!("[L{class};"));
                Some(rt.alloc(array, Data::Array(all)))
            }
            // `valueOf(name)`: the constant, or `IllegalArgumentException` as `Enum.valueOf` throws.
            ("valueOf", _) if desc.starts_with("(Ljava/lang/String;)") => {
                let (class, names) = constants(owner)?;
                let wanted = rt.str_of(this).map(str::to_owned);
                match names.split(',').enumerate().find(|(_, n)| Some(*n) == wanted.as_deref()) {
                    Some((i, n)) => Some(crate::api::api_enum(rt, class, n, i as i32)),
                    None => {
                        let text = format!("No enum constant {}.{}", class.replace(['/', '$'], "."), wanted.unwrap_or_else(|| "null".into()));
                        Some(crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &text))
                    }
                }
            }
            _ => None,
        };
    }
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
                h = h.wrapping_mul(31).wrapping_add(hash_of(rt, v));
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

    const TUPLE: &str = "net/minecraft/util/Tuple";

    /// FramedBlocks keeps pairs in vanilla's `Tuple` (its state cache) and Mekanism and others name
    /// world folders with `new LevelResource(id)`: built, read back, set, and compared as their
    /// classes do (`Tuple` keeps `Object`'s `equals`: two pairs of the same things differ).
    #[test]
    fn a_plain_vanilla_data_class_keeps_what_it_was_given() {
        let mut rt = Rt::new();
        let (a, b, c) = (rt.string("a"), rt.string("b"), rt.string("c"));
        let t = rt.new_object(TUPLE);
        crate::api::call(&mut rt, "net/minecraft/util/Tuple.<init>.(Ljava/lang/Object;Ljava/lang/Object;)V", &[t, a, b]);
        let get_a = "net/minecraft/util/Tuple.getA.()Ljava/lang/Object;";
        assert_eq!(crate::api::call(&mut rt, get_a, &[t]), a);
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/util/Tuple.getB.()Ljava/lang/Object;", &[t]), b);
        crate::api::call(&mut rt, "net/minecraft/util/Tuple.setA.(Ljava/lang/Object;)V", &[t, c]);
        assert_eq!(crate::api::call(&mut rt, get_a, &[t]), c);

        let twin = rt.new_object(TUPLE);
        crate::api::call(&mut rt, "net/minecraft/util/Tuple.<init>.(Ljava/lang/Object;Ljava/lang/Object;)V", &[twin, c, b]);
        let equals = "net/minecraft/util/Tuple.equals.(Ljava/lang/Object;)Z";
        assert_eq!(crate::api::call(&mut rt, equals, &[t, twin]), V::I(0));
        assert_eq!(crate::api::call(&mut rt, equals, &[t, t]), V::I(1));

        let folder = rt.new_object("net/minecraft/world/level/storage/LevelResource");
        let id = rt.string("serverconfig");
        crate::api::call(&mut rt, "net/minecraft/world/level/storage/LevelResource.<init>.(Ljava/lang/String;)V", &[folder, id]);
        let got = crate::api::call(&mut rt, "net/minecraft/world/level/storage/LevelResource.getId.()Ljava/lang/String;", &[folder]);
        assert_eq!(rt.str_of(got), Some("serverconfig"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ModelLayerLocation` overrides `equals` and `hashCode` over both its fields: a renderer's layer
    /// looked up with a location built again is the same layer.
    #[test]
    fn a_data_class_that_compares_its_fields_is_equal_to_its_twin() {
        let mut rt = Rt::new();
        let init = "net/minecraft/client/model/geom/ModelLayerLocation.<init>.(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/String;)V";
        let layer = |rt: &mut Rt, path: &str, part: &str| {
            let l = rt.new_object("net/minecraft/client/model/geom/ModelLayerLocation");
            let rl = rt.string(&format!("farmersdelight:{path}"));
            let part = rt.string(part);
            crate::api::call(rt, init, &[l, rl, part]);
            l
        };
        let (x, twin, other) = (layer(&mut rt, "canvas_sign", "main"), layer(&mut rt, "canvas_sign", "main"), layer(&mut rt, "canvas_sign", "outer"));
        let equals = "net/minecraft/client/model/geom/ModelLayerLocation.equals.(Ljava/lang/Object;)Z";
        let hash = "net/minecraft/client/model/geom/ModelLayerLocation.hashCode.()I";
        assert_eq!(crate::api::call(&mut rt, equals, &[x, twin]), V::I(1));
        assert_eq!(crate::api::call(&mut rt, hash, &[x]), crate::api::call(&mut rt, hash, &[twin]));
        assert_eq!(crate::api::call(&mut rt, equals, &[x, other]), V::I(0));
        let model = crate::api::call(&mut rt, "net/minecraft/client/model/geom/ModelLayerLocation.getModel.()Lnet/minecraft/resources/ResourceLocation;", &[x]);
        assert_eq!(rt.str_of(model), Some("farmersdelight:canvas_sign"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// An enum's `equals` is `Enum`'s (final): the same constant or not. FarmersDelight compares
    /// axes so.
    #[test]
    fn enum_constants_are_equal_only_to_themselves() {
        let mut rt = Rt::new();
        let x = crate::api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.X");
        let z = crate::api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.Z");
        let equals = "net/minecraft/core/Direction$Axis.equals.(Ljava/lang/Object;)Z";
        let x_again = crate::api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.X");
        assert_eq!(crate::api::call(&mut rt, equals, &[x, x_again]), V::I(1));
        assert_eq!(crate::api::call(&mut rt, equals, &[x, z]), V::I(0));
        let hash = "net/minecraft/core/Direction$Axis.hashCode.()I";
        assert_eq!(crate::api::call(&mut rt, hash, &[x]), crate::api::call(&mut rt, hash, &[x_again]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const INSTRUMENT: &str = "net/minecraft/world/level/block/state/properties/NoteBlockInstrument";

    /// A vanilla enum the runtime doesn't model by hand still has its constants, named and ordered
    /// as 1.21.1's class declares them (`NoteBlockInstrument.BASS` is the fifth), one object each:
    /// what `values()` and `valueOf` give is the same constant.
    #[test]
    fn a_vanilla_enums_constants_come_from_its_class() {
        let mut rt = Rt::new();
        let bass = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.BASS");
        assert_eq!(crate::api::call(&mut rt, "java/lang/Enum.ordinal.()I", &[bass]), V::I(4));
        let all = crate::api::call(&mut rt, "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.values.()[Lnet/minecraft/world/level/block/state/properties/NoteBlockInstrument;", &[]);
        assert_eq!(rt.array_len(all), 23);
        assert_eq!(rt.array_get(all, 4), bass);
        let name = rt.string("BASS");
        let value_of = "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.valueOf.(Ljava/lang/String;)Lnet/minecraft/world/level/block/state/properties/NoteBlockInstrument;";
        assert_eq!(crate::api::call(&mut rt, value_of, &[name]), bass);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        assert!(rt.thrown.is_none());
        let wrong = rt.string("TUBA");
        crate::api::call(&mut rt, value_of, &[wrong]);
        let e = rt.thrown.take().expect("valueOf of no constant throws");
        assert_eq!(rt.class_of(e), "java/lang/IllegalArgumentException");
        let m = rt.get_field(e, "message");
        assert_eq!(rt.str_of(m), Some(&*format!("No enum constant {}.TUBA", INSTRUMENT.replace('/', "."))));
    }

    /// A `switch` on a vanilla enum reads `ordinal()` through the enum's own class (javac's
    /// `$SwitchMap$` arrays): FramedBlocks' on a wall's `WallSide`. `name()` and `compareTo` are
    /// `Enum`'s final methods too.
    #[test]
    fn a_vanilla_enums_own_class_answers_enums_final_methods() {
        let mut rt = Rt::new();
        let wall_side = "net/minecraft/world/level/block/state/properties/WallSide";
        let call = |rt: &mut Rt, key: String, args: &[V]| {
            let key = rt.static_key(&key);
            crate::api::call(rt, key, args)
        };
        let tall = crate::blocks::vanilla_enum_constant(&mut rt, wall_side, "tall").unwrap();
        assert_eq!(call(&mut rt, format!("{wall_side}.ordinal.()I"), &[tall]), V::I(2));
        let harp = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.HARP");
        let bass = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.BASS");
        assert_eq!(call(&mut rt, format!("{INSTRUMENT}.ordinal.()I"), &[bass]), V::I(4));
        let name = call(&mut rt, format!("{INSTRUMENT}.name.()Ljava/lang/String;"), &[bass]);
        assert_eq!(rt.str_of(name), Some("BASS"));
        assert_eq!(call(&mut rt, format!("{INSTRUMENT}.compareTo.(Ljava/lang/Enum;)I"), &[harp, bass]), V::I(-4));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The data-class table answers last, from fields named as vanilla names them; a class the
    /// runtime models by hand keeps its own representation, so the table must leave it out
    /// (`tools/vanilla_records.py` does; rerun it after modelling one). One the runtime only builds
    /// through the table ([`make_data`]) is the table's.
    #[test]
    fn the_data_class_table_leaves_out_what_the_runtime_models_by_hand() {
        let mut text = String::new();
        for dir in [concat!(env!("CARGO_MANIFEST_DIR"), "/src"), concat!(env!("CARGO_MANIFEST_DIR"), "/../horizon-compat-core/src")] {
            for e in std::fs::read_dir(dir).unwrap() {
                let path = e.unwrap().path();
                if path.extension().is_some_and(|x| x == "rs") && !path.ends_with("vanilla_records.rs") && !path.ends_with("records.rs") {
                    // The module's code, not its tests.
                    let source = std::fs::read_to_string(&path).unwrap();
                    for line in source.split("#[cfg(test)]").next().unwrap_or("").lines().filter(|l| !l.contains("records::make_data(")) {
                        text += line;
                        text.push('\n');
                    }
                }
            }
        }
        let shadowed: Vec<&str> = DATA_CLASSES
            .iter()
            .map(|(c, _)| *c)
            .filter(|c| text.contains(&format!("\"{c}\"")) || text.contains(&format!("\"{c}.")) || text.contains(&format!("{c}\"")))
            .collect();
        assert!(shadowed.is_empty(), "modelled by hand and in DATA_CLASSES: {shadowed:?}");
    }
}