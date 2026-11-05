//! Mojang's DataFixerUpper codecs (`com.mojang.serialization`), natively. A mod builds its codecs from
//! DFU's and Minecraft's pieces (`RecordCodecBuilder`, `fieldOf`, `xmap`, `Codec.either`,
//! `Ingredient.CODEC`...). Each piece here is an object recording what it was made of (class
//! `compat:codec/<kind>`, its parts in order), and [`decode`] walks one over a JSON value, calling the
//! mod's own functions where DFU would (a record's constructor, an `xmap`'s mapping, a dispatch's
//! codec for a type). Create's recipe serializers read its recipe files this way. Minecraft's own
//! codecs are read natively ([`crate::recipes::decode_native`]). Also here: DFU's `DataResult`,
//! `Either` and `Pair`, which codecs and the functions they call hand around.
//!
//! [`encode`] walks a codec the other way, into NBT (`encodeStart(NbtOps.INSTANCE, value)`): what a
//! block entity saves with a codec (Create's display links, chain conveyors, packagers), calling the
//! mod's getters and inverse mappings where DFU would.
use crate::nbt::Nbt;
use crate::{Data, Rt, Table, V, jutil};
use serde_json::{Map, Value as Json};
use std::collections::BTreeMap;

const DATA_RESULT: &str = "com/mojang/serialization/DataResult";
const DATA_ERROR: &str = "com/mojang/serialization/DataResult$Error";
const EITHER: &str = "com/mojang/datafixers/util/Either";
const PAIR: &str = "com/mojang/datafixers/util/Pair";
const OPTIONAL: &str = "java/util/Optional";
const FUNCTION: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const BI_FUNCTION: &str = "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const SUPPLIER: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const CONSUMER: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const SERIALIZED_NAME: &str = "net/minecraft/util/StringRepresentable.getSerializedName.()Ljava/lang/String;";

fn codec(rt: &mut Rt, kind: &'static str, parts: Vec<V>) -> V {
    rt.alloc(kind, Data::Array(parts))
}

/// Whether `v` is one of these codec objects.
pub fn is_codec(rt: &Rt, v: V) -> bool {
    v.r() != 0 && rt.class_of(v).starts_with("compat:codec/")
}

fn kind(rt: &Rt, c: V) -> &'static str {
    match rt.class_of(c) {
        // A mod's `new StringRepresentable.EnumCodec(values, resolver)`.
        ENUM_CODEC => "enum_codec",
        class => class.strip_prefix("compat:codec/").unwrap_or(""),
    }
}

const ENUM_CODEC: &str = "net/minecraft/util/StringRepresentable$EnumCodec";

/// The constant enum codec `c` names `name` (a string): through its resolver (a mod's
/// `EnumCodec`), or by serialized name (`StringRepresentable.fromEnum`'s); null for none.
fn enum_by_name(rt: &mut Rt, c: V, name: V) -> Result<V, String> {
    if kind(rt, c) == "enum_codec" {
        let resolver = rt.get_field(c, "$resolver");
        return run(rt, FUNCTION, resolver, &[name]);
    }
    let Some(wanted) = rt.str_of(name).map(str::to_owned) else { return Ok(V::NULL) };
    let values = run(rt, SUPPLIER, rt.captured(c).first().copied().unwrap_or_default(), &[])?;
    for v in jutil::elements(rt, values) {
        let n = run(rt, SERIALIZED_NAME, v, &[])?;
        if rt.str_of(n) == Some(wanted.as_str()) {
            return Ok(v);
        }
    }
    Ok(V::NULL)
}

/// A codec of Minecraft's own, read natively (`Ingredient.CODEC` is `native` + `ingredient`).
pub fn native(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("compat:codec/native/{name}"), move |rt| {
        let s = rt.string(&n);
        codec(rt, "compat:codec/native", vec![s])
    })
}

/// A map codec of Minecraft's own over a whole object (`ShapedRecipePattern.MAP_CODEC`, vanilla's
/// shaped recipe), read by [`crate::crafting::decode_map`]; `extra` is what it was made with.
pub fn native_map(rt: &mut Rt, name: &str, extra: &[V]) -> V {
    let n = name.to_owned();
    if extra.is_empty() {
        return rt.intern(format!("compat:codec/native_map/{name}"), move |rt| {
            let s = rt.string(&n);
            codec(rt, "compat:codec/native_map", vec![s])
        });
    }
    let s = rt.string(&n);
    let mut parts = vec![s];
    parts.extend_from_slice(extra);
    codec(rt, "compat:codec/native_map", parts)
}

fn prim(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("compat:codec/prim/{name}"), move |rt| {
        let s = rt.string(&n);
        codec(rt, "compat:codec/prim", vec![s])
    })
}

fn range(rt: &mut Rt, prim_name: &str, min: f64, max: f64) -> V {
    let p = prim(rt, prim_name);
    codec(rt, "compat:codec/range", vec![p, V::D(min), V::D(max)])
}

/// The codecs in static fields: DFU's primitives, `ExtraCodecs`', Minecraft's and NeoForge's own,
/// and NeoForge's registries (named as this runtime names registries).
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    Some(match (owner, field) {
        // `PrimitiveCodec.INT` is `Codec.INT`, the interface's constant reached through its own.
        ("com/mojang/serialization/Codec" | "com/mojang/serialization/codecs/PrimitiveCodec", f) => match f {
            "INT" | "LONG" | "FLOAT" | "DOUBLE" | "BOOL" | "STRING" | "BYTE" | "SHORT" | "INT_STREAM" | "LONG_STREAM" => prim(rt, &f.to_ascii_lowercase()),
            "PASSTHROUGH" => native(rt, "passthrough"),
            _ => return None,
        },
        ("net/minecraft/util/ExtraCodecs", f) => match f {
            "NON_NEGATIVE_INT" => range(rt, "int", 0.0, i32::MAX as f64),
            "POSITIVE_INT" => range(rt, "int", 1.0, i32::MAX as f64),
            "POSITIVE_FLOAT" => range(rt, "float", f64::from(f32::MIN_POSITIVE), f32::MAX as f64),
            "NON_NEGATIVE_FLOAT" => range(rt, "float", 0.0, f32::MAX as f64),
            "NON_EMPTY_STRING" => prim(rt, "string"),
            _ => return None,
        },
        ("net/minecraft/nbt/NbtOps", "INSTANCE") => crate::ops::nbt_ops(rt),
        ("com/mojang/serialization/JsonOps", "INSTANCE" | "COMPRESSED") => rt.intern("compat:json-ops".into(), |rt| rt.new_object("com/mojang/serialization/JsonOps")),
        ("net/minecraft/resources/ResourceLocation", "CODEC") => native(rt, "resource_location"),
        ("net/minecraft/core/BlockPos", "CODEC") => native(rt, "block_pos"),
        // `{dimension, pos}`, as vanilla's record codec reads and writes it.
        ("net/minecraft/core/GlobalPos", "CODEC") => rt.intern("compat:codec/global_pos".into(), |rt| {
            let ctor = crate::functions::record_constructor(rt, "net/minecraft/core/GlobalPos");
            let dimension = native(rt, "resource_key/dimension");
            let pos = native(rt, "block_pos");
            let mut parts = vec![ctor];
            for (name, c) in [("dimension", dimension), ("pos", pos)] {
                let n = rt.string(name);
                let field = codec(rt, "compat:codec/field", vec![n, c]);
                let getter = crate::functions::component(rt, name);
                parts.push(codec(rt, "compat:codec/getter", vec![field, getter]));
            }
            let record = codec(rt, "compat:codec/record", parts);
            codec(rt, "compat:codec/of_map", vec![record])
        }),
        // A block item's state properties: a map of names to values, the record around it.
        ("net/minecraft/world/item/component/BlockItemStateProperties", "CODEC") => rt.intern("compat:codec/block_item_state_properties".into(), |rt| {
            let string = prim(rt, "string");
            let map = codec(rt, "compat:codec/map", vec![string, string]);
            let to = crate::functions::record_constructor(rt, "net/minecraft/world/item/component/BlockItemStateProperties");
            let from = crate::functions::component(rt, "properties");
            codec(rt, "compat:codec/xmap", vec![map, to, V::I(0), from, V::I(0)])
        }),
        ("net/minecraft/core/Vec3i", "CODEC") => native(rt, "vec3i"),
        ("net/minecraft/world/phys/Vec3", "CODEC") => native(rt, "vec3"),
        ("net/minecraft/core/UUIDUtil", "CODEC") => native(rt, "uuid"),
        ("net/minecraft/core/UUIDUtil", "STRING_CODEC" | "LENIENT_CODEC") => native(rt, "uuid_string"),
        ("net/minecraft/core/Direction", "CODEC") => native(rt, "direction"),
        ("net/minecraft/core/Direction$Axis", "CODEC") => native(rt, "axis"),
        (crate::rotation::MIRROR, "CODEC") => native(rt, "mirror"),
        (crate::rotation::ROTATION, "CODEC") => native(rt, "rotation"),
        ("net/minecraft/util/Unit", "CODEC") => native(rt, "unit"),
        ("net/minecraft/world/item/ItemStack", "CODEC" | "STRICT_CODEC" | "SINGLE_ITEM_CODEC") => native(rt, "item_stack"),
        ("net/minecraft/world/item/ItemStack", "OPTIONAL_CODEC") => native(rt, "optional_item_stack"),
        ("net/minecraft/world/item/component/ItemContainerContents", "CODEC") => native(rt, "container_contents"),
        ("net/minecraft/world/item/crafting/Ingredient", "CODEC") => native(rt, "ingredient"),
        ("net/minecraft/world/item/crafting/Ingredient", "CODEC_NONEMPTY") => native(rt, "ingredient_nonempty"),
        // NeoForge's `MAP_CODEC_NONEMPTY.codec().listOf()`, and the same refusing an empty list.
        ("net/minecraft/world/item/crafting/Ingredient", "LIST_CODEC" | "LIST_CODEC_NONEMPTY") => {
            let element = native(rt, "ingredient_nonempty");
            let mut parts = vec![element];
            if field == "LIST_CODEC_NONEMPTY" {
                parts.push(rt.string("Item array cannot be empty, at least one item must be defined"));
            }
            codec(rt, "compat:codec/list", parts)
        }
        ("net/minecraft/world/item/crafting/Recipe", "CODEC" | "CONDITIONAL_CODEC") => native(rt, "recipe"),
        ("net/neoforged/neoforge/fluids/FluidStack", "CODEC" | "FIXED_AMOUNT_CODEC") => native(rt, "fluid_stack"),
        ("net/neoforged/neoforge/fluids/FluidStack", "OPTIONAL_CODEC") | ("net/neoforged/neoforge/fluids/SimpleFluidContent", "CODEC") => native(rt, "optional_fluid_stack"),
        ("net/neoforged/neoforge/fluids/crafting/SizedFluidIngredient", "FLAT_CODEC" | "NESTED_CODEC") => native(rt, "sized_fluid_ingredient"),
        ("net/neoforged/neoforge/common/crafting/SizedIngredient", "FLAT_CODEC" | "NESTED_CODEC") => native(rt, "sized_ingredient"),
        ("net/neoforged/neoforge/fluids/crafting/FluidIngredient", "MAP_CODEC_NONEMPTY") => native(rt, "fluid_ingredient_map"),
        ("net/neoforged/neoforge/fluids/crafting/FluidIngredient", "CODEC" | "CODEC_NON_EMPTY") => native(rt, "fluid_ingredient"),
        ("net/minecraft/core/component/DataComponentPatch" | "net/minecraft/core/component/DataComponentMap", "CODEC") => native(rt, "component_patch"),
        ("net/minecraft/nbt/CompoundTag", "CODEC") => native(rt, "compound_tag"),
        ("net/minecraft/world/item/DyeColor", "CODEC") => native(rt, "dye_color"),
        ("net/minecraft/world/level/block/state/BlockState", "CODEC") => native(rt, "block_state"),
        // Ore targets and the rule tests they're made of (`OreConfiguration.TargetBlockState.CODEC`,
        // `RuleTest.CODEC`: by `predicate_type`).
        ("net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration$TargetBlockState", "CODEC") => native(rt, "target_block_state"),
        ("net/minecraft/world/level/levelgen/structure/templatesystem/RuleTest", "CODEC") => native(rt, "rule_test"),
        ("net/minecraft/world/level/levelgen/feature/configurations/OreConfiguration", "CODEC") => native(rt, "ore_configuration"),
        ("net/minecraft/network/chat/ComponentSerialization", "CODEC" | "FLAT_CODEC") => native(rt, "component"),
        // A fluid by name (`Holder<Fluid>`; registries are their names here, holders their entries).
        ("net/neoforged/neoforge/fluids/FluidStack", "FLUID_NON_EMPTY_CODEC") => rt.intern("compat:codec/fluid-non-empty".into(), |rt| {
            let r = rt.string("fluid");
            codec(rt, "compat:codec/registry", vec![r])
        }),
        // A block-state enum's codec (`WeatheringCopper.WeatherState.CODEC`): by serialized name.
        (o, "CODEC") if crate::blocks::is_vanilla_enum(o) => native(rt, &format!("vanilla_enum/{o}")),
        // Any other vanilla enum's (`EquipmentSlot.CODEC`, `AttributeModifier.Operation.CODEC`,
        // `GenerationStep.Decoration.CODEC`): `StringRepresentable.fromEnum` by the serialized
        // name, which for these is the constant's name in lower case.
        (o, "CODEC") if crate::api_common::enum_names(o).is_some() => native(rt, &format!("lower_enum/{o}")),
        // `NeoForgeRegistries.FLUID_INGREDIENT_TYPES` is registry `fluid_ingredient_type`, and so is
        // its key (`NeoForgeRegistries.Keys`): registries are their names here.
        ("net/neoforged/neoforge/registries/NeoForgeRegistries" | "net/neoforged/neoforge/registries/NeoForgeRegistries$Keys", f) => {
            let n = f.to_ascii_lowercase();
            let n = n.strip_suffix("_types").map_or(n.clone(), |t| format!("{t}_type"));
            rt.string(&n)
        }
        _ => return None,
    })
}

/// The functional interface key of a record constructor taking `n` fields.
fn constructor_key(rt: &mut Rt, n: usize) -> &'static str {
    match n {
        1 => FUNCTION,
        2 => BI_FUNCTION,
        n => {
            let objects = "Ljava/lang/Object;".repeat(n);
            rt.static_key(&format!("com/mojang/datafixers/util/Function{n}.apply.({objects})Ljava/lang/Object;"))
        }
    }
}

fn optional(rt: &mut Rt, v: Option<V>) -> V {
    let o = rt.new_object(OPTIONAL);
    rt.put_field(o, "value", v.unwrap_or(V::NULL));
    o
}

/// `DataResult.success(v)`.
pub fn success(rt: &mut Rt, v: V) -> V {
    let r = rt.new_object(DATA_RESULT);
    rt.put_field(r, "$value", v);
    r
}

/// `DataResult.error(() -> message)`.
pub fn error(rt: &mut Rt, message: &str) -> V {
    let r = rt.new_object(DATA_RESULT);
    let m = rt.string(message);
    rt.put_field(r, "$error", m);
    r
}

/// A result's value, or its error message (asked of the supplier it holds, once).
pub fn result_of(rt: &mut Rt, r: V) -> Result<V, String> {
    if rt.class_of(r) != DATA_RESULT {
        return Err(format!("not a DataResult: {}", rt.class_of(r)));
    }
    let e = rt.get_field(r, "$error");
    if e.r() != 0 {
        return Err(rt.str_of(e).unwrap_or("error").to_owned());
    }
    let f = rt.get_field(r, "$errorFn");
    if f.r() != 0 {
        let m = rt.call_functional(SUPPLIER, f, &[]);
        rt.thrown = None;
        let text = rt.str_of(m).unwrap_or("error").to_owned();
        let s = rt.string(&text);
        rt.put_field(r, "$error", s);
        return Err(text);
    }
    Ok(rt.get_field(r, "$value"))
}

fn either(rt: &mut Rt, left: bool, v: V) -> V {
    let e = rt.new_object(EITHER);
    rt.put_field(e, if left { "$left" } else { "$right" }, v);
    rt.put_field(e, "$isLeft", V::I(left as i32));
    e
}

fn pair(rt: &mut Rt, a: V, b: V) -> V {
    let p = rt.new_object(PAIR);
    rt.put_field(p, "$first", a);
    rt.put_field(p, "$second", b);
    p
}

/// Runs a mod function; an exception it throws is a reading error.
fn run(rt: &mut Rt, key: &'static str, f: V, args: &[V]) -> Result<V, String> {
    let r = rt.call_functional(key, f, args);
    match rt.thrown.take() {
        Some(e) => {
            let m = rt.get_field(e, "message");
            Err(format!("{} thrown: {}", rt.class_of(e), rt.str_of(m).unwrap_or("")))
        }
        None => Ok(r),
    }
}

fn boxed(rt: &mut Rt, class: &'static str, v: V) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// Reads `j` with codec `c`, as `c.parse(JsonOps.INSTANCE, j)` would: the value, or why not.
pub fn decode(rt: &mut Rt, c: V, j: &Json) -> Result<V, String> {
    let parts = rt.captured(c);
    let part = |i: usize| parts.get(i).copied().unwrap_or_default();
    match kind(rt, c) {
        "prim" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            primitive(rt, &name, j)
        }
        "range" => {
            let v = decode(rt, part(0), j)?;
            let n = match rt.get_field(v, "value") {
                V::I(i) => i as f64,
                V::F(f) => f as f64,
                V::D(d) => d,
                V::J(l) => l as f64,
                _ => return Err(format!("Not a number: {j}")),
            };
            if n < part(1).d() || n > part(2).d() {
                return Err(format!("Value {n} outside of range [{}:{}]", part(1).d(), part(2).d()));
            }
            Ok(v)
        }
        "list" => {
            let Json::Array(items) = j else { return Err(format!("Not a list: {j}")) };
            // A list that refuses to be empty, with the message it gives.
            if let Some(message) = rt.str_of(part(1)).filter(|_| items.is_empty()) {
                return Err(message.to_owned());
            }
            let mut out = Vec::with_capacity(items.len());
            for x in items {
                out.push(decode(rt, part(0), x)?);
            }
            Ok(jutil::new_list(rt, out))
        }
        "map" => {
            let Json::Object(o) = j else { return Err(format!("Not a map: {j}")) };
            let m = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
            for (k, v) in o {
                let key = decode(rt, part(0), &Json::String(k.clone()))?;
                let value = decode(rt, part(1), v)?;
                jutil::table_put(rt, m, key, value);
            }
            Ok(m)
        }
        "xmap" => {
            let v = decode(rt, part(0), j)?;
            let r = map_value(rt, part(1), part(2).i() != 0, v)?;
            // `stringResolver`'s: a name its function doesn't know.
            if part(5).i() != 0 && r.r() == 0 {
                return Err(format!("Unknown element name:{}", j.as_str().unwrap_or_default()));
            }
            Ok(r)
        }
        // `ExtraCodecs.optionalEmptyMap(codec)`: `{}` is an empty Optional, anything else the codec's.
        "optional_empty_map" => match j {
            Json::Object(o) if o.is_empty() => Ok(optional(rt, None)),
            _ => {
                let v = decode(rt, part(0), j)?;
                Ok(optional(rt, Some(v)))
            }
        },
        "validate" => {
            let v = decode(rt, part(0), j)?;
            let r = run(rt, FUNCTION, part(1), &[v])?;
            result_of(rt, r)
        }
        "either" => match decode(rt, part(0), j) {
            Ok(v) => Ok(either(rt, true, v)),
            Err(left) => match decode(rt, part(1), j) {
                Ok(v) => Ok(either(rt, false, v)),
                Err(right) => Err(format!("Failed to parse either. First: {left}; Second: {right}")),
            },
        },
        "alt" => match decode(rt, part(0), j) {
            Ok(v) => Ok(v),
            Err(first) => {
                let v = decode(rt, part(1), j).map_err(|second| format!("{first}; {second}"))?;
                alternative(rt, part(2), v)
            }
        },
        "unit" => Ok(part(0)),
        "lazy" => {
            let inner = lazy(rt, c)?;
            decode(rt, inner, j)
        }
        "java" => java_decode(rt, part(0), j),
        "registry" => {
            let Json::String(id) = j else { return Err(format!("Not a string: {j}")) };
            let registry = rt.str_of(part(0)).unwrap_or("").to_owned();
            registry_value(rt, &registry, id)
        }
        "enum" | "enum_codec" => {
            let Json::String(name) = j else { return Err(format!("Not a string: {j}")) };
            let n = rt.string(name);
            match enum_by_name(rt, c, n)? {
                v if v.r() != 0 => Ok(v),
                _ => Err(format!("Unknown element name: {name}")),
            }
        }
        "native" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            crate::recipes::decode_native(rt, &name, j)
        }
        "pair" => {
            let a = decode(rt, part(0), j)?;
            let b = decode(rt, part(1), j)?;
            Ok(pair(rt, a, b))
        }
        // A map codec read as a codec (`MapCodec.codec()`, a record, a dispatch): the value is an
        // object of its fields.
        "of_map" | "field" | "optional" | "record" | "point" | "dispatch" | "native_map" | "xor" | "dispatch_or_else" | "java_map" => {
            let Json::Object(o) = j else { return Err(format!("Not a map: {j}")) };
            decode_map(rt, c, o)
        }
        other => Err(format!("codec {other} can't read")),
    }
}

/// Reads the fields of object `o` with map codec `c` (`MapCodec.decode`).
pub fn decode_map(rt: &mut Rt, c: V, o: &Map<String, Json>) -> Result<V, String> {
    let parts = rt.captured(c);
    let part = |i: usize| parts.get(i).copied().unwrap_or_default();
    match kind(rt, c) {
        "field" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            match o.get(&name) {
                Some(j) => decode(rt, part(1), j).map_err(|e| format!("{name}: {e}")),
                None => Err(format!("No key {name} in MapLike")),
            }
        }
        // `optionalFieldOf`: absent is the default (or an empty Optional); present but unreadable is
        // an error, unless lenient.
        "optional" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            let (has_default, lenient) = (part(3).i() != 0, part(4).i() != 0);
            let found = match o.get(&name) {
                None | Some(Json::Null) => None,
                Some(j) => match decode(rt, part(1), j) {
                    Ok(v) => Some(v),
                    Err(_) if lenient => None,
                    Err(e) => return Err(format!("{name}: {e}")),
                },
            };
            Ok(match (found, has_default) {
                (Some(v), true) => v,
                (None, true) => part(2),
                (v, false) => optional(rt, v),
            })
        }
        "record" => {
            let ctor = part(0);
            let mut values = Vec::with_capacity(parts.len().saturating_sub(1));
            for &g in parts.iter().skip(1) {
                values.push(app_value(rt, g, o)?);
            }
            let key = constructor_key(rt, values.len());
            run(rt, key, ctor, &values)
        }
        "point" | "unit" => Ok(part(0)),
        "of_map" => decode_map(rt, part(0), o),
        "xmap" => {
            let v = decode_map(rt, part(0), o)?;
            map_value(rt, part(1), part(2).i() != 0, v)
        }
        "validate" => {
            let v = decode_map(rt, part(0), o)?;
            let r = run(rt, FUNCTION, part(1), &[v])?;
            result_of(rt, r)
        }
        "dispatch" => dispatch(rt, &parts, o),
        "alt" => match decode_map(rt, part(0), o) {
            Ok(v) => Ok(v),
            Err(first) => {
                let v = decode_map(rt, part(1), o).map_err(|second| format!("{first}; {second}"))?;
                alternative(rt, part(2), v)
            }
        },
        "either" => match decode_map(rt, part(0), o) {
            Ok(v) => Ok(either(rt, true, v)),
            Err(_) => decode_map(rt, part(1), o).map(|v| either(rt, false, v)),
        },
        // NeoForge's `dispatchMapOrElse(typeCodec, type, codec, fallback)`: by `type` when the map
        // has one (left), else the fallback's (right).
        "dispatch_or_else" => {
            if o.contains_key("type") {
                decode_map(rt, part(0), o).map(|v| either(rt, true, v))
            } else {
                decode_map(rt, part(1), o).map(|v| either(rt, false, v))
            }
        }
        // NeoForge's `xor(first, second)`: exactly one of them reads.
        "xor" => match (decode_map(rt, part(0), o), decode_map(rt, part(1), o)) {
            (Ok(_), Ok(_)) => Err("Both alternatives read successfully, can not pick the correct one".into()),
            (Ok(v), Err(_)) => Ok(either(rt, true, v)),
            (Err(_), Ok(v)) => Ok(either(rt, false, v)),
            (Err(first), Err(second)) => Err(format!("Neither alternative read successfully! first: {first}; second: {second}")),
        },
        "lazy" => {
            let inner = lazy(rt, c)?;
            decode_map(rt, inner, o)
        }
        "native_map" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            crate::crafting::decode_map(rt, &name, parts.get(1..).unwrap_or_default(), o)
        }
        "java_map" => java_decode_map(rt, part(0), o),
        // A codec over the whole object.
        _ => decode(rt, c, &Json::Object(o.clone())),
    }
}

/// One of a record's fields (`fieldOf(...).forGetter(...)`), or a value it always has
/// (`instance.point(value)`).
fn app_value(rt: &mut Rt, app: V, o: &Map<String, Json>) -> Result<V, String> {
    match kind(rt, app) {
        "getter" => {
            let field = rt.captured(app).first().copied().unwrap_or_default();
            decode_map(rt, field, o)
        }
        "point" => Ok(rt.captured(app).first().copied().unwrap_or_default()),
        // `builder.dependent(getter, encoder, decoderOf)`: the builder's own field read first, then
        // the decoder its value picks (a map codec, or a mod's `MapDecoder`) reads the same object.
        "dependent" => {
            let parts = rt.captured(app);
            let part = |i: usize| parts.get(i).copied().unwrap_or_default();
            let first = app_value(rt, part(0), o)?;
            let decoder = run(rt, FUNCTION, part(3), &[first])?;
            let decoder = as_codec(rt, decoder);
            decode_map(rt, decoder, o)
        }
        other => Err(format!("a record field of kind {other:?}")),
    }
}

/// `withAlternative(primary, alternative, converter)`'s converter on what the alternative read.
fn alternative(rt: &mut Rt, converter: V, v: V) -> Result<V, String> {
    if converter.r() == 0 { Ok(v) } else { run(rt, FUNCTION, converter, &[v]) }
}

/// `xmap`'s and `comapFlatMap`'s mapping of a value read: `to` returns the value, or a `DataResult`.
fn map_value(rt: &mut Rt, to: V, flat: bool, v: V) -> Result<V, String> {
    let r = run(rt, FUNCTION, to, &[v])?;
    if flat { result_of(rt, r) } else { Ok(r) }
}

/// `Codec.lazyInitialized(supplier)`'s codec, asked of the supplier once.
fn lazy(rt: &mut Rt, c: V) -> Result<V, String> {
    let parts = rt.captured(c);
    if let Some(&made) = parts.get(1).filter(|v| v.r() != 0) {
        return Ok(made);
    }
    let supplier = parts.first().copied().unwrap_or_default();
    let made = run(rt, SUPPLIER, supplier, &[])?;
    if let Some(o) = rt.heap.get_mut(c.r() as usize) {
        o.data = Data::Array(vec![supplier, made]);
    }
    Ok(made)
}

/// Whether codec `c` reads its fields from the object it is given (a map codec, or a codec made of
/// one): what a dispatch hands the rest of its object to, as DFU's `KeyDispatchCodec` does.
fn reads_object(rt: &mut Rt, c: V) -> bool {
    match kind(rt, c) {
        "field" | "optional" | "record" | "point" | "dispatch" | "of_map" | "unit" | "native_map" | "xor" | "dispatch_or_else" | "java_map" => true,
        "xmap" | "validate" | "alt" => {
            let inner = rt.captured(c).first().copied().unwrap_or_default();
            reads_object(rt, inner)
        }
        "lazy" => lazy(rt, c).is_ok_and(|inner| reads_object(rt, inner)),
        _ => false,
    }
}

/// `Codec.dispatch(typeKey, typeOf, codecOf)`: the type in `typeKey`, then the object with the
/// type's codec: a map codec reads the same object (its own key included); another codec reads
/// `value`. Parts: [typeKey, type codec, typeOf, codecOf].
fn dispatch(rt: &mut Rt, parts: &[V], o: &Map<String, Json>) -> Result<V, String> {
    let key = rt.str_of(parts[0]).unwrap_or("type").to_owned();
    let Some(t) = o.get(&key) else { return Err(format!("No key {key} in MapLike")) };
    let ty = decode(rt, parts[1], t)?;
    let mut c = run(rt, FUNCTION, parts[3], &[ty])?;
    // `partialDispatch`'s function answers a `DataResult`.
    if rt.class_of(c) == DATA_RESULT {
        c = result_of(rt, c)?;
    }
    if !is_codec(rt, c) {
        return Err(format!("no codec this runtime reads for type {t}"));
    }
    if reads_object(rt, c) {
        decode_map(rt, c, o)
    } else {
        match o.get("value") {
            Some(v) => decode(rt, c, v),
            None => decode(rt, c, &Json::Object(o.clone())),
        }
    }
}

/// A primitive codec's value: Java's box of it (a string as itself).
fn primitive(rt: &mut Rt, name: &str, j: &Json) -> Result<V, String> {
    let num = || j.as_f64().ok_or_else(|| format!("Not a number: {j}"));
    Ok(match name {
        "int" => {
            let v = V::I(num()? as i32);
            boxed(rt, "java/lang/Integer", v)
        }
        "short" => {
            let v = V::I(num()? as i16 as i32);
            boxed(rt, "java/lang/Short", v)
        }
        "byte" => {
            let v = match j {
                Json::Bool(b) => V::I(*b as i32),
                _ => V::I(num()? as i8 as i32),
            };
            boxed(rt, "java/lang/Byte", v)
        }
        "long" => {
            let v = V::J(match j.as_i64() {
                Some(l) => l,
                None => num()? as i64,
            });
            boxed(rt, "java/lang/Long", v)
        }
        "float" => {
            let v = V::F(num()? as f32);
            boxed(rt, "java/lang/Float", v)
        }
        "double" => {
            let v = V::D(num()?);
            boxed(rt, "java/lang/Double", v)
        }
        "bool" => {
            let b = match j {
                Json::Bool(b) => *b,
                Json::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
                _ => return Err(format!("Not a boolean: {j}")),
            };
            crate::api_common::boolean(rt, b)
        }
        "string" => match j {
            Json::String(s) => rt.string(s),
            _ => return Err(format!("Not a string: {j}")),
        },
        // `Codec.INT_STREAM` and `LONG_STREAM`: a list of numbers, read as an `IntStream` or a
        // `LongStream` (Mekanism's QIO drives keep their items' counts in one).
        "int_stream" | "long_stream" => match j {
            Json::Array(items) => {
                let long = name == "long_stream";
                let ns: Option<Vec<V>> = items.iter().map(|x| x.as_i64().map(|i| if long { V::J(i) } else { V::I(i as i32) })).collect();
                let ns = ns.ok_or_else(|| format!("Not a list of numbers: {j}"))?;
                rt.alloc(if long { "java/util/stream/LongStream" } else { "java/util/stream/IntStream" }, Data::Stream(ns))
            }
            _ => return Err(format!("Not a list: {j}")),
        },
        _ => return Err(format!("no primitive {name}")),
    })
}

/// An entry of a registry by id (`byNameCodec`): what the mod registered, vanilla's and the game's
/// items, blocks and fluids; another registry's entry the mod didn't make is its id.
fn registry_value(rt: &mut Rt, registry: &str, id: &str) -> Result<V, String> {
    let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
    if let Some(&v) = rt.registered.get(&(registry.to_owned(), id.clone())) {
        return Ok(v);
    }
    let unknown = || Err(format!("Unknown registry key in {registry}: {id}"));
    match registry {
        "item" => {
            if id == "minecraft:air" || !crate::recipes::item_known(rt, &id) {
                return unknown();
            }
            Ok(crate::api_common::item(rt, &id))
        }
        "fluid" => match crate::fluids::fluid(rt, &id) {
            f if f.r() != 0 => Ok(f),
            _ => unknown(),
        },
        "block" if id.starts_with("minecraft:") => Ok(crate::api::block(rt, &id)),
        // The mod's own registries, and the vanilla ones it fills, hold what it registered.
        r if rt.custom_registries.iter().any(|c| c == r) || crate::neo::REGISTRIES.contains(&r) => unknown(),
        _ => Ok(rt.string(&id)),
    }
}

// ---- Writing ----

/// Writes `value` with codec `c`, as `c.encodeStart(NbtOps.INSTANCE, value)` would: the tag, or why
/// not.
pub fn encode(rt: &mut Rt, c: V, value: V) -> Result<Nbt, String> {
    let parts = rt.captured(c);
    let part = |i: usize| parts.get(i).copied().unwrap_or_default();
    match kind(rt, c) {
        "prim" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            write_primitive(rt, &name, value)
        }
        "range" => encode(rt, part(0), value),
        "list" => {
            let mut out = Vec::new();
            for v in jutil::elements(rt, value) {
                out.push(encode(rt, part(0), v)?);
            }
            Ok(Nbt::List(out))
        }
        "map" => {
            let mut out = BTreeMap::new();
            for (k, v) in jutil::table_pairs(rt, value) {
                let Nbt::String(key) = encode(rt, part(0), k)? else { return Err("Not a string key".into()) };
                let v = encode(rt, part(1), v)?;
                out.insert(key, v);
            }
            Ok(Nbt::Compound(out))
        }
        "xmap" => {
            let v = map_back(rt, &parts, value)?;
            encode(rt, part(0), v)
        }
        "validate" => {
            let r = run(rt, FUNCTION, part(1), &[value])?;
            let v = result_of(rt, r)?;
            encode(rt, part(0), v)
        }
        "either" => {
            let left = rt.get_field(value, "$isLeft").i() != 0;
            let v = rt.get_field(value, if left { "$left" } else { "$right" });
            encode(rt, if left { part(0) } else { part(1) }, v)
        }
        // `withAlternative`: written with the first.
        "alt" => encode(rt, part(0), value),
        "optional_empty_map" => {
            let held = rt.get_field(value, "value");
            if held.r() == 0 { Ok(Nbt::Compound(BTreeMap::new())) } else { encode(rt, part(0), held) }
        }
        "unit" => Ok(Nbt::Compound(BTreeMap::new())),
        "lazy" => {
            let inner = lazy(rt, c)?;
            encode(rt, inner, value)
        }
        "java" => java_encode(rt, part(0), value),
        "registry" => {
            let registry = rt.str_of(part(0)).unwrap_or("").to_owned();
            registry_key(rt, &registry, value).map(Nbt::String)
        }
        "enum" | "enum_codec" => serialized_name(rt, value).map(Nbt::String),
        "native" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            write_native(rt, &name, value)
        }
        // `Codec.pair`: both halves into one map.
        "pair" => {
            let mut out = BTreeMap::new();
            for (half, field) in [(part(0), "$first"), (part(1), "$second")] {
                let v = rt.get_field(value, field);
                match encode(rt, half, v)? {
                    Nbt::Compound(m) => out.extend(m),
                    _ => return Err("a pair's half that is not a map".into()),
                }
            }
            Ok(Nbt::Compound(out))
        }
        "of_map" | "field" | "optional" | "record" | "point" | "dispatch" | "native_map" | "xor" | "dispatch_or_else" | "java_map" => {
            let mut out = BTreeMap::new();
            encode_map(rt, c, value, &mut out)?;
            Ok(Nbt::Compound(out))
        }
        other => Err(format!("codec {other} can't write")),
    }
}

/// Writes `value`'s fields into `out` with map codec `c` (`MapCodec.encode`).
fn encode_map(rt: &mut Rt, c: V, value: V, out: &mut BTreeMap<String, Nbt>) -> Result<(), String> {
    let parts = rt.captured(c);
    let part = |i: usize| parts.get(i).copied().unwrap_or_default();
    match kind(rt, c) {
        "field" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            let v = encode(rt, part(1), value).map_err(|e| format!("{name}: {e}"))?;
            out.insert(name, v);
        }
        // `optionalFieldOf(name[, default])`: an empty Optional, or the default, is left out
        // (NeoForge's `optionalFieldAlwaysWrite` writes the default too).
        "optional" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            let (has_default, always) = (part(3).i() != 0, part(5).i() != 0);
            let present = if has_default {
                (always || !jutil::equals(rt, value, part(2))).then_some(value)
            } else {
                let held = rt.get_field(value, "value");
                (held.r() != 0).then_some(held)
            };
            if let Some(v) = present {
                let v = encode(rt, part(1), v).map_err(|e| format!("{name}: {e}"))?;
                out.insert(name, v);
            }
        }
        // Each field from the mod's getter (`forGetter`).
        "record" => {
            for &g in parts.iter().skip(1) {
                let field = rt.captured(g);
                let at = |i: usize| field.get(i).copied().unwrap_or_default();
                // A field's map codec and getter; a dependent one's encoder (its builder's own field
                // is written by that builder).
                let (codec, getter) = match kind(rt, g) {
                    "getter" => (at(0), at(1)),
                    "dependent" => (as_codec(rt, at(2)), at(1)),
                    _ => continue,
                };
                let v = run(rt, FUNCTION, getter, &[value])?;
                encode_map(rt, codec, v, out)?;
            }
        }
        "point" | "unit" => {}
        "of_map" => encode_map(rt, part(0), value, out)?,
        "xmap" => {
            let v = map_back(rt, &parts, value)?;
            encode_map(rt, part(0), v, out)?;
        }
        "validate" => {
            let r = run(rt, FUNCTION, part(1), &[value])?;
            let v = result_of(rt, r)?;
            encode_map(rt, part(0), v, out)?;
        }
        // The type under its key, then the type's codec: a map codec beside it, another under `value`.
        "dispatch" => {
            let key = rt.str_of(part(0)).unwrap_or("type").to_owned();
            let ty = run(rt, FUNCTION, part(2), &[value])?;
            let mut typed = run(rt, FUNCTION, part(3), &[ty])?;
            if rt.class_of(typed) == DATA_RESULT {
                typed = result_of(rt, typed)?;
            }
            if !is_codec(rt, typed) {
                return Err("no codec this runtime writes for its type".into());
            }
            let t = encode(rt, part(1), ty)?;
            out.insert(key, t);
            if reads_object(rt, typed) {
                encode_map(rt, typed, value, out)?;
            } else {
                let v = encode(rt, typed, value)?;
                out.insert("value".into(), v);
            }
        }
        "alt" => encode_map(rt, part(0), value, out)?,
        "either" | "xor" | "dispatch_or_else" => {
            let left = rt.get_field(value, "$isLeft").i() != 0;
            let v = rt.get_field(value, if left { "$left" } else { "$right" });
            encode_map(rt, if left { part(0) } else { part(1) }, v, out)?;
        }
        "lazy" => {
            let inner = lazy(rt, c)?;
            encode_map(rt, inner, value, out)?;
        }
        "java_map" => java_encode_map(rt, part(0), value, out)?,
        // A codec over the whole object.
        _ => match encode(rt, c, value)? {
            Nbt::Compound(m) => out.extend(m),
            _ => return Err("not a map".into()),
        },
    }
    Ok(())
}

/// `xmap`'s way back (`from`), answering a value or a `DataResult`.
fn map_back(rt: &mut Rt, parts: &[V], value: V) -> Result<V, String> {
    let from = parts.get(3).copied().unwrap_or_default();
    if from.r() == 0 {
        return Err("a mapped codec with no way back".into());
    }
    let r = run(rt, FUNCTION, from, &[value])?;
    let r = if parts.get(4).is_some_and(|f| f.i() != 0) { result_of(rt, r)? } else { r };
    // `stringResolver`'s: a value its function names nothing.
    if parts.get(5).is_some_and(|f| f.i() != 0) && r.r() == 0 {
        return Err("Element with unknown name".into());
    }
    Ok(r)
}

/// A registry entry's id (`byNameCodec`): what the mod registered, a vanilla entry's own name.
fn registry_key(rt: &mut Rt, registry: &str, v: V) -> Result<String, String> {
    if let Some(n) = rt.registered_name(v, registry) {
        return Ok(n.to_owned());
    }
    if registry == "item" {
        let n = crate::api_common::item_like_name(rt, v);
        if !n.is_empty() {
            return Ok(n);
        }
    }
    rt.str_of(v).map(str::to_owned).ok_or_else(|| format!("Unknown registry element in {registry}"))
}

fn serialized_name(rt: &mut Rt, v: V) -> Result<String, String> {
    let n = run(rt, SERIALIZED_NAME, v, &[])?;
    rt.str_of(n).map(str::to_owned).ok_or_else(|| "an enum value with no name".to_owned())
}

/// A primitive codec's tag of a value (Java's box of it, or the raw value a primitive collection
/// holds).
fn write_primitive(rt: &Rt, name: &str, v: V) -> Result<Nbt, String> {
    if name == "string" {
        return rt.str_of(v).map(|s| Nbt::String(s.to_owned())).ok_or_else(|| "Not a string".to_owned());
    }
    if name == "int_stream" || name == "long_stream" {
        return match rt.heap.get(v.r() as usize).map(|o| &o.data) {
            Some(Data::Stream(items)) if name == "long_stream" => Ok(Nbt::LongArray(items.iter().map(|x| x.j()).collect())),
            Some(Data::Stream(items)) => Ok(Nbt::IntArray(items.iter().map(|x| x.i()).collect())),
            _ => Err(format!("Not a stream: {name}")),
        };
    }
    let raw = match v {
        V::R(_) => rt.get_field(v, "value"),
        raw => raw,
    };
    let (i, f) = match raw {
        V::I(i) => (i64::from(i), f64::from(i)),
        V::J(j) => (j, j as f64),
        V::F(x) => (x as i64, f64::from(x)),
        V::D(x) => (x as i64, x),
        _ => return Err(format!("Not a number: {name}")),
    };
    Ok(match name {
        "int" => Nbt::Int(i as i32),
        "short" => Nbt::Short(i as i16),
        "byte" => Nbt::Byte(i as i8),
        "long" => Nbt::Long(i),
        "float" => Nbt::Float(f as f32),
        "double" => Nbt::Double(f),
        "bool" => Nbt::Byte((i != 0) as i8),
        _ => return Err(format!("no primitive {name}")),
    })
}

/// Minecraft's and NeoForge's own codecs, as vanilla writes them.
fn write_native(rt: &mut Rt, name: &str, v: V) -> Result<Nbt, String> {
    Ok(match name {
        "resource_location" => Nbt::String(rt.str_of(v).ok_or("Not a resource location")?.to_owned()),
        "block_pos" | "vec3i" => Nbt::IntArray(crate::api::pos_of(rt, v).to_vec()),
        // `[x, y, z]` as doubles (`Vec3.CODEC` is `DOUBLE.listOf()` of three).
        "vec3" => {
            let xyz = ["x", "y", "z"].map(|f| Nbt::Double(rt.get_field(v, f).d()));
            Nbt::List(xyz.to_vec())
        }
        // Four ints, most significant first (`UUIDUtil.CODEC`), or the dashed text.
        "uuid" => {
            let (msb, lsb) = crate::tags::uuid_bits(rt, v);
            Nbt::IntArray(vec![(msb >> 32) as i32, msb as i32, (lsb >> 32) as i32, lsb as i32])
        }
        "uuid_string" => {
            let (msb, lsb) = crate::tags::uuid_bits(rt, v);
            Nbt::String(crate::tags::uuid_text(msb, lsb))
        }
        "direction" | "axis" => Nbt::String(crate::api_common::text_of(rt, v)),
        // By serialized name, from the constant's ordinal (`Rotation.CLOCKWISE_180` is `180`).
        "mirror" | "rotation" => {
            let names: &[&str] = if name == "mirror" { &crate::rotation::MIRRORS } else { &crate::rotation::ROTATION_NAMES };
            let i = rt.get_field(v, "$ordinal").i();
            Nbt::String(names.get(i as usize).ok_or("an enum value with no name")?.to_string())
        }
        "unit" => Nbt::Compound(BTreeMap::new()),
        // `{id, count, components}` (the mod's components); nothing for an empty optional one.
        "item_stack" | "optional_item_stack" => {
            if crate::api_common::stack_empty(rt, v) {
                if name == "item_stack" {
                    return Err("Item must not be minecraft:air".into());
                }
                Nbt::Compound(BTreeMap::new())
            } else {
                let id = crate::api_common::item_like_name(rt, rt.get_field(v, "$item"));
                let count = rt.get_field(v, "$count").i();
                let mut m = BTreeMap::from([("id".to_owned(), Nbt::String(id)), ("count".to_owned(), Nbt::Int(count))]);
                let carried = crate::components::patch(rt, v);
                if !carried.is_empty() {
                    m.insert("components".to_owned(), Nbt::Compound(carried));
                }
                Nbt::Compound(m)
            }
        }
        // `[{slot, item}]`, the slots that aren't empty (`ItemContainerContents.CODEC`).
        "container_contents" => {
            let mut out = Vec::new();
            for (i, s) in crate::handlers::contents_items(rt, v).into_iter().enumerate() {
                if !crate::api_common::stack_empty(rt, s) {
                    let item = write_native(rt, "item_stack", s)?;
                    out.push(Nbt::Compound(BTreeMap::from([("slot".to_owned(), Nbt::Int(i as i32)), ("item".to_owned(), item)])));
                }
            }
            Nbt::List(out)
        }
        "fluid_stack" | "optional_fluid_stack" => Nbt::Compound(crate::tanks::save_stack(rt, v)),
        "compound_tag" => Nbt::Compound(crate::nbt::from_runtime(rt, v)),
        // `{Name, Properties}`, as `BlockState.CODEC` writes a state.
        "block_state" => {
            let text = rt.str_of(v).unwrap_or("minecraft:air");
            let (block, props) = match text.split_once('[') {
                Some((b, rest)) => (b, rest.trim_end_matches(']')),
                None => (text, ""),
            };
            let mut m = BTreeMap::from([("Name".to_owned(), Nbt::String(block.to_owned()))]);
            let properties: BTreeMap<String, Nbt> = props.split(',').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_owned(), Nbt::String(v.to_owned()))).collect();
            if !properties.is_empty() {
                m.insert("Properties".to_owned(), Nbt::Compound(properties));
            }
            Nbt::Compound(m)
        }
        "dye_color" => Nbt::String(serialized_name(rt, v)?),
        "item_enchantments" => crate::enchantments::encode(rt, v)?,
        // Ingredients, sized ones and fluid ones as their codecs write them (a stream codec built
        // on one sends this).
        "ingredient" | "ingredient_nonempty" => json_to_nbt(&crate::recipes::write_ingredient(rt, v))?,
        "sized_ingredient" => json_to_nbt(&crate::recipes::write_sized_item(rt, v))?,
        "fluid_ingredient" | "fluid_ingredient_map" => json_to_nbt(&crate::recipes::write_fluid_ingredient(rt, v, name == "fluid_ingredient_map"))?,
        "sized_fluid_ingredient" => json_to_nbt(&crate::recipes::write_sized_fluid(rt, v))?,
        "component_patch" => {
            let json = rt.get_field(v, "$json");
            match rt.str_of(json).and_then(|t| serde_json::from_str::<Json>(t).ok()) {
                Some(j) => crate::recipes::json_nbt(&j).unwrap_or(Nbt::Compound(BTreeMap::new())),
                None => Nbt::Compound(BTreeMap::new()),
            }
        }
        "component" => crate::recipes::json_nbt(&crate::api_common::component_json(rt, v)).ok_or("Not a component")?,
        n if n.starts_with("tag_key/") || n.starts_with("hashed_tag_key/") => {
            let location = rt.str_of(rt.get_field(v, "$location")).ok_or("Not a tag key")?.to_owned();
            Nbt::String(if n.starts_with("hashed") { format!("#{location}") } else { location })
        }
        n if n.starts_with("resource_key/") => {
            let key = rt.str_of(v).ok_or("Not a resource key")?;
            Nbt::String(key.rsplit_once('#').map_or(key, |(_, l)| l).to_owned())
        }
        n if n.starts_with("vanilla_enum/") => {
            let class = &n["vanilla_enum/".len()..];
            Nbt::String(crate::blocks::enum_serialized(rt, class, v).ok_or("Not an enum constant")?)
        }
        n if n.starts_with("lower_enum/") => {
            let name = rt.str_of(rt.get_field(v, "$name")).ok_or("Not an enum constant")?;
            Nbt::String(name.to_ascii_lowercase())
        }
        other => return Err(format!("codec {other} can't write")),
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner.starts_with("com/mojang/serialization/")
        || owner.starts_with("com/mojang/datafixers/")
        || matches!(
            owner,
            "net/minecraft/util/ExtraCodecs"
                | "net/minecraft/util/StringRepresentable"
                | ENUM_CODEC
                | "net/neoforged/neoforge/common/util/NeoForgeExtraCodecs"
                | "net/minecraft/resources/RegistryOps"
                | "net/minecraft/resources/RegistryFileCodec"
                | "net/minecraft/core/RegistryCodecs"
        )
        || owner == crate::ops::NBT_OPS
        || matches!(name, "byNameCodec" | "holderByNameCodec" | "createSerializationContext")
        || (matches!(name, "simpleCodec" | "propertiesCodec") && owner.starts_with("net/minecraft/world/level/block/"))
        || (matches!(owner, "net/minecraft/tags/TagKey" | "net/minecraft/resources/ResourceKey") && matches!(name, "codec" | "hashedCodec"))
        || (name == "commonFields" && owner.starts_with("net/minecraft/world/level/storage/loot/functions/"))
        || (name == "codecStart" && owner.starts_with("net/neoforged/neoforge/common/loot/"))
        || RECEIVER_METHODS.contains(&name)
}

/// What a codec, `DataResult`, `Either` or `Pair` answers whichever class names the call.
const RECEIVER_METHODS: [&str; 66] = [
    "apply", "codec", "comapFlatMap", "decode", "deprecated", "dispatch", "dispatchMap", "dispatchStable", "encode", "encodeStart", "fieldOf", "flatComapMap",
    "flatXmap", "forGetter", "group", "keys", "lenientOptionalFieldOf", "listOf", "mapResult", "optionalFieldOf", "orElse", "orElseGet", "parse",
    "partialDispatch", "point", "promotePartial", "sizeLimitedListOf", "stable", "toString", "validate", "withAlternative", "withLifecycle", "xmap",
    "addLifecycle", "error", "flatMap", "getOrThrow", "getPartialOrThrow", "ifError", "ifSuccess", "isError", "isSuccess", "lifecycle", "map",
    "mapError", "mapOrElse", "orThrow", "partialOrThrow", "result", "resultOrPartial", "setLifecycle", "setPartial", "getFirst", "getLeft",
    "getRight", "getSecond", "ifLeft", "ifRight", "left", "mapBoth", "mapFirst", "mapLeft", "mapRight", "mapSecond", "right", "swap",
];

/// Calls on codecs, `DataResult`s, `Either`s and `Pair`s; None for what this module doesn't model.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, method, desc) = crate::parts(key)?;
    // Static factories.
    match (owner, method) {
        ("com/mojang/serialization/codecs/RecordCodecBuilder", "create" | "mapCodec") => {
            let instance = rt.intern("compat:codec/instance".into(), |rt| codec(rt, "compat:codec/instance", vec![]));
            let built = rt.call_functional(FUNCTION, a(0), &[instance]);
            if rt.thrown.is_some() || !is_codec(rt, built) {
                return Some(V::NULL);
            }
            return Some(if method == "create" { codec(rt, "compat:codec/of_map", vec![built]) } else { built });
        }
        ("com/mojang/serialization/codecs/RecordCodecBuilder", "point") => return Some(codec(rt, "compat:codec/point", vec![a(0)])),
        // `RegistryFileCodec.create(registry, codec[, inline])`: an entry of the registry by its id
        // (registries are their names here).
        ("net/minecraft/resources/RegistryFileCodec", "create") if rt.str_of(a(0)).is_some() => return Some(codec(rt, "compat:codec/registry", vec![a(0)])),
        // `RegistryCodecs.homogeneousList(registry, ...)`: a holder set (`#tag`, an id, a list of
        // them), which this runtime doesn't read yet; its codec stands, reading with it fails.
        ("net/minecraft/core/RegistryCodecs", "homogeneousList") => {
            let registry = rt.str_of(a(0)).unwrap_or("").to_owned();
            return Some(native(rt, &format!("unmodelled/RegistryCodecs.homogeneousList/{registry}")));
        }
        // `TagKey.codec(registry)` reads `ns:path`, `hashedCodec` `#ns:path`; `ResourceKey.codec`
        // an entry's key.
        ("net/minecraft/tags/TagKey", "codec" | "hashedCodec") if rt.str_of(a(0)).is_some() => {
            let registry = rt.str_of(a(0)).unwrap_or("").to_owned();
            let kind = if method == "codec" { "tag_key" } else { "hashed_tag_key" };
            return Some(native(rt, &format!("{kind}/{registry}")));
        }
        ("net/minecraft/resources/ResourceKey", "codec") if rt.str_of(a(0)).is_some() => {
            let registry = rt.str_of(a(0)).unwrap_or("").to_owned();
            return Some(native(rt, &format!("resource_key/{registry}")));
        }
        // `BlockBehaviour.propertiesCodec()`: the field `properties`, which reads as a new
        // `Properties.of()` (vanilla's `Properties.CODEC` is `Codec.unit(Properties::of)`).
        (o, "propertiesCodec") if o.starts_with("net/minecraft/world/level/block/") => {
            let props = crate::api::call(rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.of.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[]);
            let unit = codec(rt, "compat:codec/unit", vec![props]);
            let name = rt.string("properties");
            let field = codec(rt, "compat:codec/field", vec![name, unit]);
            return Some(codec(rt, "compat:codec/getter", vec![field, V::NULL]));
        }
        // `BlockBehaviour.simpleCodec(constructor)`: a block's codec, its properties alone, which
        // only a block-type registry reads.
        (o, "simpleCodec") if o.starts_with("net/minecraft/world/level/block/") && desc == "(Ljava/util/function/Function;)Lcom/mojang/serialization/MapCodec;" => {
            return Some(codec(rt, "compat:codec/block", vec![a(0)]));
        }
        // A loot function's shared fields (`LootItemConditionalFunction.commonFields(instance)`): the
        // group of `conditions`, an optional list of loot conditions (none by default), read from
        // the function's `predicates`. NeoForge's loot modifiers start with a required `conditions`
        // (`LootModifier.codecStart`). Loot conditions themselves aren't modelled: a list of them
        // fails to read, an absent or empty one reads.
        (o, "commonFields") if o.starts_with("net/minecraft/world/level/storage/loot/functions/") && desc.starts_with("(Lcom/mojang/serialization/codecs/RecordCodecBuilder$Instance;)") => {
            let condition = native(rt, "unmodelled/net/minecraft/world/level/storage/loot/predicates/LootItemCondition.DIRECT_CODEC");
            let list = codec(rt, "compat:codec/list", vec![condition]);
            let (name, none) = (rt.string("conditions"), jutil::new_list(rt, Vec::new()));
            let field = codec(rt, "compat:codec/optional", vec![name, list, none, V::I(1), V::I(0)]);
            let getter = crate::functions::component(rt, "predicates");
            let app = codec(rt, "compat:codec/getter", vec![field, getter]);
            return Some(codec(rt, "compat:codec/products", vec![app]));
        }
        (o, "codecStart") if o.starts_with("net/neoforged/neoforge/common/loot/") && desc.starts_with("(Lcom/mojang/serialization/codecs/RecordCodecBuilder$Instance;)") => {
            let condition = native(rt, "unmodelled/net/minecraft/world/level/storage/loot/predicates/LootItemCondition.DIRECT_CODEC");
            let list = codec(rt, "compat:codec/list", vec![condition]);
            let name = rt.string("conditions");
            let field = codec(rt, "compat:codec/field", vec![name, list]);
            let getter = crate::functions::component(rt, "conditions");
            let app = codec(rt, "compat:codec/getter", vec![field, getter]);
            return Some(codec(rt, "compat:codec/products", vec![app]));
        }
        // `RecordCodecBuilder.of(getter, mapCodec)`: a field.
        ("com/mojang/serialization/codecs/RecordCodecBuilder", "of") => return Some(codec(rt, "compat:codec/getter", vec![a(1), a(0)])),
        ("com/mojang/serialization/Codec", "either") if desc.starts_with("(Lcom/mojang/serialization/Codec;Lcom/mojang/serialization/Codec;)") => {
            return Some(codec(rt, "compat:codec/either", vec![a(0), a(1)]));
        }
        // The static `withAlternative(primary, alternative[, converter])`.
        ("com/mojang/serialization/Codec" | "com/mojang/serialization/MapCodec", "withAlternative") if desc.starts_with("(Lcom/mojang/serialization/") && desc.matches(';').count() >= 2 => {
            let converter = if desc.contains("Ljava/util/function/Function;") { a(2) } else { V::NULL };
            return Some(codec(rt, "compat:codec/alt", vec![a(0), a(1), converter]));
        }
        ("com/mojang/serialization/Codec", "list") if desc.starts_with("(Lcom/mojang/serialization/Codec;") => {
            return Some(codec(rt, "compat:codec/list", vec![a(0)]));
        }
        ("com/mojang/serialization/Codec", "unboundedMap") => return Some(codec(rt, "compat:codec/map", vec![a(0), a(1)])),
        ("com/mojang/serialization/Codec", "pair") => return Some(codec(rt, "compat:codec/pair", vec![a(0), a(1)])),
        ("com/mojang/serialization/Codec" | "com/mojang/serialization/MapCodec", "unit") => {
            // `unit(value)` or `unit(supplier)`.
            let v = if desc.starts_with("(Ljava/util/function/Supplier;)") { rt.call_functional(SUPPLIER, a(0), &[]) } else { a(0) };
            return Some(codec(rt, "compat:codec/unit", vec![v]));
        }
        ("com/mojang/serialization/Codec", "lazyInitialized") => return Some(codec(rt, "compat:codec/lazy", vec![a(0), V::NULL])),
        // `Codec.stringResolver(toString, fromString)`: a string the mod's functions turn into the
        // value and back; one they answer null for is an error. Parts as `xmap`'s, and that check.
        ("com/mojang/serialization/Codec", "stringResolver") => {
            let s = prim(rt, "string");
            return Some(codec(rt, "compat:codec/xmap", vec![s, a(1), V::I(0), a(0), V::I(0), V::I(1)]));
        }
        ("net/minecraft/util/ExtraCodecs", "optionalEmptyMap") => return Some(codec(rt, "compat:codec/optional_empty_map", vec![a(0)])),
        // `Codec.checkRange(min, max)`: a function answering a `DataResult` (for `validate`,
        // `flatXmap`).
        ("com/mojang/serialization/Codec", "checkRange") => return Some(crate::functions::check_range(rt, a(0), a(1))),
        // The static `Codec.optionalField(name, codec, lenient)`: `optionalFieldOf`'s.
        ("com/mojang/serialization/Codec", "optionalField") => {
            return Some(codec(rt, "compat:codec/optional", vec![a(0), a(1), V::NULL, V::I(0), V::I((a(2).i() != 0) as i32)]));
        }
        // NeoForge's `aliasedFieldOf(codec, names...)`: the field under the first name present,
        // written under the first (fields with alternatives, as NeoForge builds it).
        ("net/neoforged/neoforge/common/util/NeoForgeExtraCodecs", "aliasedFieldOf") => {
            let names = jutil::elements(rt, a(1));
            let mut fields = names.into_iter().map(|n| codec(rt, "compat:codec/field", vec![n, a(0)])).collect::<Vec<_>>().into_iter();
            let first = fields.next()?;
            return Some(fields.fold(first, |c, alt| codec(rt, "compat:codec/alt", vec![c, alt, V::NULL])));
        }
        ("net/neoforged/neoforge/common/util/NeoForgeExtraCodecs", "xor") if desc.starts_with("(Lcom/mojang/serialization/MapCodec;") => {
            return Some(codec(rt, "compat:codec/xor", vec![a(0), a(1)]));
        }
        // `dispatchMapOrElse(typeCodec, type, codec, fallback)`: `typeCodec.dispatchMap(type, codec)`,
        // or the fallback for a map without `type` (Mekanism's chemical ingredients).
        ("net/neoforged/neoforge/common/util/NeoForgeExtraCodecs", "dispatchMapOrElse") => {
            let key = rt.string("type");
            let dispatched = codec(rt, "compat:codec/dispatch", vec![key, a(0), a(1), a(2)]);
            return Some(codec(rt, "compat:codec/dispatch_or_else", vec![dispatched, a(3)]));
        }
        ("com/mojang/serialization/Codec", "intRange") | ("net/minecraft/util/ExtraCodecs", "intRange") => {
            return Some(range(rt, "int", a(0).i() as f64, a(1).i() as f64));
        }
        ("com/mojang/serialization/Codec", "floatRange") => return Some(range(rt, "float", a(0).f() as f64, a(1).f() as f64)),
        ("com/mojang/serialization/Codec", "doubleRange") => return Some(range(rt, "double", a(0).d(), a(1).d())),
        ("net/minecraft/util/StringRepresentable", "fromEnum" | "fromValues" | "fromEnumWithMapping") => {
            return Some(codec(rt, "compat:codec/enum", vec![a(0)]));
        }
        ("net/minecraft/util/StringRepresentable", "createNameLookup") => return Some(crate::functions::name_lookup(rt, a(0), a(1))),
        // A mod's own `new StringRepresentable.EnumCodec(values, resolver)`: by name through it.
        (ENUM_CODEC, "<init>") => {
            rt.put_field(a(0), "$values", a(1));
            rt.put_field(a(0), "$resolver", a(2));
            return Some(V::Void);
        }
        // `byName(name)` and `byName(name, default)`, on that or on `fromEnum`'s codec.
        (ENUM_CODEC, "byName") => {
            let found = enum_by_name(rt, a(0), a(1)).unwrap_or(V::NULL);
            return Some(if found.r() == 0 && desc.contains(";Ljava/lang/Enum;)") { a(2) } else { found });
        }
        ("net/neoforged/neoforge/common/util/NeoForgeExtraCodecs", "optionalFieldAlwaysWrite") => {
            return Some(codec(rt, "compat:codec/optional", vec![a(1), a(0), a(2), V::I(1), V::I(0), V::I(1)]));
        }
        ("net/minecraft/util/ExtraCodecs", "nonEmptyList") => return Some(a(0)),
        // Registries are their names here: `BuiltInRegistries.ITEM.byNameCodec()`.
        (_, "byNameCodec" | "holderByNameCodec") if rt.str_of(a(0)).is_some() => {
            return Some(codec(rt, "compat:codec/registry", vec![a(0)]));
        }
        (DATA_RESULT, "success") => {
            let v = a(0);
            return Some(success(rt, v));
        }
        (DATA_RESULT, "error") => {
            let r = rt.new_object(DATA_RESULT);
            rt.put_field(r, "$errorFn", a(0));
            if desc.starts_with("(Ljava/util/function/Supplier;Ljava/lang/Object;") {
                rt.put_field(r, "$partial", a(1));
            }
            return Some(r);
        }
        (DATA_ERROR, "message") => return Some(rt.get_field(a(0), "$message")),
        (EITHER, "left") if desc.starts_with("(Ljava/lang/Object;)") => return Some(either(rt, true, a(0))),
        (EITHER, "right") if desc.starts_with("(Ljava/lang/Object;)") => return Some(either(rt, false, a(0))),
        (EITHER, "unwrap") => {
            let side = if rt.get_field(a(0), "$isLeft").i() != 0 { "$left" } else { "$right" };
            return Some(rt.get_field(a(0), side));
        }
        (PAIR, "of") => return Some(pair(rt, a(0), a(1))),
        // Registry-aware ops write as the ops they wrap (registries are names here).
        ("net/minecraft/resources/RegistryOps", "create") => return Some(a(0)),
        (_, "createSerializationContext") if desc.starts_with("(Lcom/mojang/serialization/DynamicOps;)") => return Some(a(1)),
        _ => {}
    }
    let receiver = a(0);
    if is_codec(rt, receiver) {
        return codec_method(rt, method, desc, args);
    }
    if crate::ops::takes(rt, owner, receiver) {
        return crate::ops::call(rt, method, desc, args);
    }
    // A codec class of the mod's own (SophisticatedCore's `SetCodec`, Mekanism's
    // `SequencedCollectionCodec`): a combinator or a default method of DFU's interfaces wraps it,
    // and reading or writing runs its own `decode`/`encode`.
    if MOD_CODEC_METHODS.contains(&method)
        && let Some(kind) = mod_codec_kind(rt, receiver)
    {
        let wrapped = codec(rt, kind, vec![receiver]);
        let mut with = args.to_vec();
        with[0] = wrapped;
        return codec_method(rt, method, desc, &with);
    }
    match rt.class_of(receiver) {
        DATA_RESULT => data_result_method(rt, method, desc, args),
        EITHER => {
            let left = rt.get_field(receiver, "$isLeft").i() != 0;
            let value = rt.get_field(receiver, if left { "$left" } else { "$right" });
            Some(match method {
                "left" => optional(rt, left.then_some(value)),
                "right" => optional(rt, (!left).then_some(value)),
                "ifLeft" | "ifRight" => {
                    if left == (method == "ifLeft") {
                        rt.call_functional(CONSUMER, a(1), &[value]);
                    }
                    receiver
                }
                "map" => rt.call_functional(FUNCTION, if left { a(1) } else { a(2) }, &[value]),
                "mapLeft" | "mapRight" => {
                    if left == (method == "mapLeft") {
                        let v = rt.call_functional(FUNCTION, a(1), &[value]);
                        either(rt, left, v)
                    } else {
                        receiver
                    }
                }
                "mapBoth" => {
                    let v = rt.call_functional(FUNCTION, if left { a(1) } else { a(2) }, &[value]);
                    either(rt, left, v)
                }
                "orThrow" | "getLeft" | "getRight" => value,
                "swap" => either(rt, !left, value),
                _ => return None,
            })
        }
        PAIR => Some(match method {
            "getFirst" => rt.get_field(receiver, "$first"),
            "getSecond" => rt.get_field(receiver, "$second"),
            "mapFirst" | "mapSecond" => {
                let (f, s) = (rt.get_field(receiver, "$first"), rt.get_field(receiver, "$second"));
                if method == "mapFirst" {
                    let v = rt.call_functional(FUNCTION, a(1), &[f]);
                    pair(rt, v, s)
                } else {
                    let v = rt.call_functional(FUNCTION, a(1), &[s]);
                    pair(rt, f, v)
                }
            }
            "swap" => {
                let (f, s) = (rt.get_field(receiver, "$first"), rt.get_field(receiver, "$second"));
                pair(rt, s, f)
            }
            _ => return None,
        }),
        _ => None,
    }
}

/// What a mod's own codec class gets from DFU's interfaces (their default methods) and wraps for.
const MOD_CODEC_METHODS: [&str; 26] = [
    "fieldOf", "optionalFieldOf", "lenientOptionalFieldOf", "listOf", "sizeLimitedListOf", "xmap", "comapFlatMap", "flatXmap", "flatComapMap",
    "validate", "codec", "forGetter", "dispatch", "partialDispatch", "dispatchStable", "dispatchMap", "orElse", "orElseGet", "withAlternative",
    "promotePartial", "withLifecycle", "stable", "deprecated", "mapResult", "parse", "encodeStart",
];

/// The kind wrapping `v`, an object of a mod class that is a codec: `java_map` for a map codec
/// (`MapCodec`, `MapDecoder`, `MapEncoder`), `java` for another (`Codec`, `Decoder`, `Encoder`).
fn mod_codec_kind(rt: &Rt, v: V) -> Option<&'static str> {
    let class = rt.class_of(v);
    if v.r() == 0 || !rt.is_mod_class(class) {
        return None;
    }
    let is = |c: &str| rt.is_a(class, c);
    if is("com/mojang/serialization/MapCodec") || is("com/mojang/serialization/MapDecoder") || is("com/mojang/serialization/MapEncoder") {
        Some("compat:codec/java_map")
    } else if is("com/mojang/serialization/Codec") || is("com/mojang/serialization/Decoder") || is("com/mojang/serialization/Encoder") {
        Some("compat:codec/java")
    } else {
        None
    }
}

const DECODE: &str = "com/mojang/serialization/Decoder.decode.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;";
pub(crate) const PRIMITIVE_CODEC: &str = "com/mojang/serialization/codecs/PrimitiveCodec";
/// A `PrimitiveCodec`'s own two methods; its `decode` and `encode` are DFU's defaults over them.
const PRIMITIVE_READ: &str = "com/mojang/serialization/codecs/PrimitiveCodec.read.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;";
const PRIMITIVE_WRITE: &str = "com/mojang/serialization/codecs/PrimitiveCodec.write.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Ljava/lang/Object;";

/// Whether the mod codec `c` is a `PrimitiveCodec` (`read`/`write`, DFU's `decode`/`encode`).
fn primitive_mod_codec(rt: &Rt, c: V) -> bool {
    rt.is_a(rt.class_of(c), PRIMITIVE_CODEC)
}
const ENCODE: &str = "com/mojang/serialization/Encoder.encode.(Ljava/lang/Object;Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;";
const MAP_DECODE: &str = "com/mojang/serialization/MapDecoder.decode.(Lcom/mojang/serialization/DynamicOps;Lcom/mojang/serialization/MapLike;)Lcom/mojang/serialization/DataResult;";
const MAP_ENCODE: &str =
    "com/mojang/serialization/MapEncoder.encode.(Ljava/lang/Object;Lcom/mojang/serialization/DynamicOps;Lcom/mojang/serialization/RecordBuilder;)Lcom/mojang/serialization/RecordBuilder;";

/// JSON as the NBT a codec writes (an empty list stays a list).
fn json_to_nbt(j: &Json) -> Result<Nbt, String> {
    crate::recipes::json_nbt(j).ok_or_else(|| format!("Not writable: {j}"))
}

/// `c` as a codec this module reads and writes with: a mod's own codec class wrapped
/// ([`mod_codec_kind`]), anything else as it is.
fn as_codec(rt: &mut Rt, c: V) -> V {
    match mod_codec_kind(rt, c) {
        Some(k) => codec(rt, k, vec![c]),
        None => c,
    }
}

/// A mod codec's own `decode(NbtOps, tag)` of `j`: the value of the pair it answers.
fn java_decode(rt: &mut Rt, c: V, j: &Json) -> Result<V, String> {
    let (ops, input) = (crate::ops::nbt_ops(rt), crate::ops::json_tag(rt, j));
    if primitive_mod_codec(rt, c) {
        // `read(ops, input).map(r -> Pair.of(r, ops.empty()))`: the value read.
        let r = run(rt, PRIMITIVE_READ, c, &[ops, input])?;
        return result_of(rt, r);
    }
    let r = run(rt, DECODE, c, &[ops, input])?;
    let p = result_of(rt, r)?;
    Ok(rt.get_field(p, "$first"))
}

/// A mod map codec's own `decode(NbtOps, MapLike)` of object `o`.
fn java_decode_map(rt: &mut Rt, c: V, o: &Map<String, Json>) -> Result<V, String> {
    let ops = crate::ops::nbt_ops(rt);
    let tag = crate::ops::json_tag(rt, &Json::Object(o.clone()));
    let m = crate::ops::map_like(rt, tag);
    let r = run(rt, MAP_DECODE, c, &[ops, m])?;
    result_of(rt, r)
}

/// A mod codec's own `encode(value, NbtOps, EndTag)`.
fn java_encode(rt: &mut Rt, c: V, value: V) -> Result<Nbt, String> {
    let (ops, empty) = (crate::ops::nbt_ops(rt), crate::ops::end(rt));
    let tag = if primitive_mod_codec(rt, c) {
        // `ops.mergeToPrimitive(prefix, write(ops, input))` onto an empty prefix: what `write` gave.
        run(rt, PRIMITIVE_WRITE, c, &[ops, value])?
    } else {
        let r = run(rt, ENCODE, c, &[value, ops, empty])?;
        result_of(rt, r)?
    };
    crate::nbt::element_from_runtime(rt, tag).ok_or_else(|| format!("a mod codec wrote {}", rt.class_of(tag)))
}

/// A mod map codec's own `encode(value, NbtOps, builder)`, its fields into `out`.
fn java_encode_map(rt: &mut Rt, c: V, value: V, out: &mut BTreeMap<String, Nbt>) -> Result<(), String> {
    let ops = crate::ops::nbt_ops(rt);
    let builder = crate::ops::map_builder(rt);
    let b = run(rt, MAP_ENCODE, c, &[value, ops, builder])?;
    out.extend(crate::ops::built(rt, b)?);
    Ok(())
}

/// A method on a codec object: `RecordCodecBuilder.Instance`'s `group`, a group's `apply`, the
/// combinators (`fieldOf`, `listOf`, `xmap`...), `forGetter`, and reading (`parse`, `decode`).
fn codec_method(rt: &mut Rt, method: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    let k = kind(rt, c);
    if k == "instance" {
        return Some(match method {
            // `instance.group(fields...)`, then the group's `apply(instance, constructor)`.
            "group" => codec(rt, "compat:codec/products", args[1..].to_vec()),
            "point" | "stable" => codec(rt, "compat:codec/point", vec![a(1)]),
            _ => return None,
        });
    }
    if k == "products" {
        return Some(match method {
            // `group(a, b).and(c)`, or `.and(otherGroup)`: the fields in order.
            "and" => {
                let mut parts = rt.captured(c);
                let other = a(1);
                if is_codec(rt, other) && kind(rt, other) == "products" {
                    parts.extend(rt.captured(other));
                } else {
                    parts.push(other);
                }
                codec(rt, "compat:codec/products", parts)
            }
            "apply" => {
                let mut parts = vec![a(2)];
                parts.extend(rt.captured(c));
                codec(rt, "compat:codec/record", parts)
            }
            _ => return None,
        });
    }
    Some(match method {
        "fieldOf" => codec(rt, "compat:codec/field", vec![a(1), c]),
        "optionalFieldOf" | "lenientOptionalFieldOf" => {
            let lenient = V::I((method == "lenientOptionalFieldOf") as i32);
            if desc.starts_with("(Ljava/lang/String;Ljava/lang/Object;") {
                codec(rt, "compat:codec/optional", vec![a(1), c, a(2), V::I(1), lenient])
            } else {
                codec(rt, "compat:codec/optional", vec![a(1), c, V::NULL, V::I(0), lenient])
            }
        }
        "listOf" | "sizeLimitedListOf" => codec(rt, "compat:codec/list", vec![c]),
        // Parts: [codec, to, to answers a DataResult, from, from answers one].
        "xmap" => codec(rt, "compat:codec/xmap", vec![c, a(1), V::I(0), a(2), V::I(0)]),
        // `flatComapMap(to, from)`'s `to` maps plainly; the others' `to` answers a DataResult.
        // `comapFlatMap`'s `from` maps plainly.
        "comapFlatMap" | "flatXmap" | "flatComapMap" => {
            codec(rt, "compat:codec/xmap", vec![c, a(1), V::I((method != "flatComapMap") as i32), a(2), V::I((method != "comapFlatMap") as i32)])
        }
        "validate" => codec(rt, "compat:codec/validate", vec![c, a(1)]),
        "codec" => codec(rt, "compat:codec/of_map", vec![c]),
        "forGetter" => codec(rt, "compat:codec/getter", vec![c, a(1)]),
        // On a record field (`fieldOf(...).forGetter(...)`): [builder, getter, encoder, decoderOf].
        "dependent" => codec(rt, "compat:codec/dependent", vec![c, a(1), a(2), a(3)]),
        "dispatch" | "partialDispatch" | "dispatchStable" | "dispatchMap" => {
            // `dispatch([typeKey,] typeOf, codecOf)`.
            let (key, type_of, codec_of) = if desc.starts_with("(Ljava/lang/String;") { (a(1), a(2), a(3)) } else { (rt.string("type"), a(1), a(2)) };
            codec(rt, "compat:codec/dispatch", vec![key, c, type_of, codec_of])
        }
        "orElse" | "orElseGet" => {
            let v = if method == "orElseGet" && desc.starts_with("(Ljava/util/function/Supplier;") { rt.call_functional(SUPPLIER, a(1), &[]) } else { a(1) };
            let unit = codec(rt, "compat:codec/unit", vec![v]);
            codec(rt, "compat:codec/alt", vec![c, unit, V::NULL])
        }
        "withAlternative" => codec(rt, "compat:codec/alt", vec![c, a(1), V::NULL]),
        "promotePartial" | "withLifecycle" | "stable" | "deprecated" | "mapResult" => c,
        "parse" => {
            // `parse(ops, input)`: the input is a value the mod holds (a tag, a JSON value).
            let input = crate::recipes::to_json(rt, a(2));
            match decode(rt, c, &input) {
                Ok(v) => success(rt, v),
                Err(e) => error(rt, &e),
            }
        }
        // `MapCodec.decode(ops, mapLike)`: the fields of the map, a `DataResult` of the value.
        "decode" if crate::ops::map_like_tag(rt, a(2)).is_some() => {
            let tag = crate::ops::map_like_tag(rt, a(2)).unwrap_or_default();
            let o = match crate::recipes::to_json(rt, tag) {
                Json::Object(o) => o,
                _ => Map::new(),
            };
            match decode_map(rt, c, &o) {
                Ok(v) => success(rt, v),
                Err(e) => error(rt, &e),
            }
        }
        "decode" => {
            let input = crate::recipes::to_json(rt, a(2));
            match decode(rt, c, &input) {
                Ok(v) => {
                    let rest = a(2);
                    let p = pair(rt, v, rest);
                    success(rt, p)
                }
                Err(e) => error(rt, &e),
            }
        }
        // `MapCodec.encode(value, ops, builder)`: the fields into the builder.
        "encode" if desc.ends_with("Lcom/mojang/serialization/RecordBuilder;)Lcom/mojang/serialization/RecordBuilder;") => {
            let mut out = BTreeMap::new();
            match encode_map(rt, c, a(1), &mut out) {
                Ok(()) => crate::ops::builder_add_all(rt, a(3), &out),
                Err(e) => crate::ops::fail(rt, a(3), &e),
            }
            a(3)
        }
        // `encodeStart(ops, value)`, `encode(value, ops, prefix)`: NBT (`NbtOps`, or `RegistryOps`
        // over it, which is the same here).
        "encodeStart" | "encode" => {
            let (ops, value, prefix) = if method == "encodeStart" { (a(1), a(2), V::NULL) } else { (a(2), a(1), a(3)) };
            if rt.class_of(ops).contains("JsonOps") {
                return Some(error(rt, "writing JSON is not modelled"));
            }
            match encode(rt, c, value) {
                Ok(n) => {
                    let n = match (n, rt.class_of(prefix)) {
                        (Nbt::Compound(m), "net/minecraft/nbt/CompoundTag") => {
                            let mut merged = crate::nbt::from_runtime(rt, prefix);
                            merged.extend(m);
                            Nbt::Compound(merged)
                        }
                        (n, _) => n,
                    };
                    let tag = crate::nbt::element_to_runtime(rt, &n).unwrap_or(V::NULL);
                    success(rt, tag)
                }
                Err(e) => error(rt, &e),
            }
        }
        "keys" => rt.alloc("java/util/stream/Stream", Data::Stream(Vec::new())),
        "toString" => {
            let text = format!("Codec[{k}]");
            rt.string(&text)
        }
        _ => return None,
    })
}

fn data_result_method(rt: &mut Rt, method: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let r = a(0);
    let result = result_of(rt, r);
    let partial = rt.get_field(r, "$partial");
    let error_object = |rt: &mut Rt, e: &str| {
        let err = rt.new_object(DATA_ERROR);
        let m = rt.string(e);
        rt.put_field(err, "$message", m);
        err
    };
    Some(match method {
        "result" => optional(rt, result.ok()),
        "error" => match result {
            Ok(_) => optional(rt, None),
            Err(e) => {
                let err = error_object(rt, &e);
                optional(rt, Some(err))
            }
        },
        "isSuccess" => V::I(result.is_ok() as i32),
        "isError" => V::I(result.is_err() as i32),
        "resultOrPartial" => match result {
            Ok(v) => optional(rt, Some(v)),
            Err(e) => {
                if desc.starts_with("(Ljava/util/function/Consumer;)") {
                    let m = rt.string(&e);
                    rt.call_functional(CONSUMER, a(1), &[m]);
                }
                optional(rt, (partial.r() != 0).then_some(partial))
            }
        },
        "getOrThrow" | "getPartialOrThrow" | "orThrow" | "partialOrThrow" => match result {
            Ok(v) => v,
            Err(_) if partial.r() != 0 && method.contains("artial") => partial,
            Err(e) => {
                let ex = rt.new_object("java/lang/IllegalStateException");
                let m = rt.string(&e);
                rt.put_field(ex, "message", m);
                rt.thrown = Some(ex);
                V::NULL
            }
        },
        "map" => match result {
            Ok(v) => {
                let m = rt.call_functional(FUNCTION, a(1), &[v]);
                success(rt, m)
            }
            Err(_) => r,
        },
        "flatMap" => match result {
            Ok(v) => rt.call_functional(FUNCTION, a(1), &[v]),
            Err(_) => r,
        },
        "mapOrElse" => match result {
            Ok(v) => rt.call_functional(FUNCTION, a(1), &[v]),
            Err(e) => {
                let err = error_object(rt, &e);
                rt.call_functional(FUNCTION, a(2), &[err])
            }
        },
        "ifSuccess" => {
            if let Ok(v) = result {
                rt.call_functional(CONSUMER, a(1), &[v]);
            }
            r
        }
        "ifError" => {
            if let Err(e) = result {
                let err = error_object(rt, &e);
                rt.call_functional(CONSUMER, a(1), &[err]);
            }
            r
        }
        "mapError" | "setLifecycle" | "addLifecycle" | "promotePartial" | "setPartial" => r,
        "lifecycle" => V::NULL,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    fn c(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let k = rt.static_key(key);
        api::call(rt, k, args)
    }

    fn st(rt: &mut Rt, key: &str) -> V {
        let k = rt.static_key(key);
        api::get_static(rt, k)
    }

    /// The codecs mods build theirs on, reached as javac names them: `PrimitiveCodec.INT` is
    /// `Codec.INT`; NeoForge's `Ingredient.LIST_CODEC_NONEMPTY` reads a list of ingredients and
    /// refuses an empty one with vanilla's message; a codec of the game's not modelled yet
    /// (`Biome.LIST_CODEC`) is still a codec, so a field built on it is one too.
    #[test]
    fn codecs_mods_build_on_are_codecs() {
        let mut rt = Rt::new();
        crate::install();
        assert_eq!(st(&mut rt, "com/mojang/serialization/codecs/PrimitiveCodec.INT"), st(&mut rt, "com/mojang/serialization/Codec.INT"));
        let list = st(&mut rt, "net/minecraft/world/item/crafting/Ingredient.LIST_CODEC_NONEMPTY");
        let empty: Json = serde_json::from_str("[]").unwrap();
        assert_eq!(decode(&mut rt, list, &empty).err().as_deref(), Some("Item array cannot be empty, at least one item must be defined"));
        let two: Json = serde_json::from_str(r#"[{"item": "minecraft:stick"}, {"tag": "c:ingots"}]"#).unwrap();
        let read = decode(&mut rt, list, &two).unwrap();
        assert_eq!(crate::jutil::elements(&rt, read).len(), 2);
        let biomes = st(&mut rt, "net/minecraft/world/level/biome/Biome.LIST_CODEC");
        assert!(is_codec(&rt, biomes));
        let name = rt.string("biomes");
        let field = c(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[biomes, name]);
        assert!(is_codec(&rt, field));
    }

    /// A vanilla enum's codec (`EquipmentSlot.CODEC`, `AttributeModifier.Operation.CODEC`) reads
    /// and writes a constant by its serialized name, and refuses a name that isn't one.
    #[test]
    fn a_vanilla_enums_codec_goes_by_its_serialized_name() {
        let mut rt = Rt::new();
        crate::install();
        let slots = st(&mut rt, "net/minecraft/world/entity/EquipmentSlot.CODEC");
        let chest = decode(&mut rt, slots, &Json::String("chest".into())).unwrap();
        assert_eq!(chest, st(&mut rt, "net/minecraft/world/entity/EquipmentSlot.CHEST"));
        assert_eq!(encode(&mut rt, slots, chest), Ok(Nbt::String("chest".into())));
        assert!(decode(&mut rt, slots, &Json::String("CHEST".into())).is_err());
        let ops = st(&mut rt, "net/minecraft/world/entity/ai/attributes/AttributeModifier$Operation.CODEC");
        let base = decode(&mut rt, ops, &Json::String("add_multiplied_base".into())).unwrap();
        assert_eq!(rt.get_field(base, "$ordinal"), V::I(1));
        let steps = st(&mut rt, "net/minecraft/world/level/levelgen/GenerationStep$Decoration.CODEC");
        let ores = decode(&mut rt, steps, &Json::String("underground_ores".into())).unwrap();
        assert_eq!(encode(&mut rt, steps, ores), Ok(Nbt::String("underground_ores".into())));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's `dispatchMapOrElse(typeCodec, type, codec, fallback)`, as Mekanism reads its
    /// chemical ingredients: a map with `type` goes by its type's codec (left), one without by the
    /// fallback (right); each writes back the way it was read.
    #[test]
    fn dispatch_or_else_reads_by_type_and_falls_back_without_one() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let string = st(&mut rt, "com/mojang/serialization/Codec.STRING");
        let int = st(&mut rt, "com/mojang/serialization/Codec.INT");
        let field_of = "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;";
        let count = rt.string("count");
        let counted = c(&mut rt, field_of, &[int, count]);
        let chemical = rt.string("chemical");
        let fallback = c(&mut rt, field_of, &[string, chemical]);
        let kind = rt.string("test:compound");
        let type_of = crate::functions::constant(&mut rt, kind);
        let codec_of = crate::functions::constant(&mut rt, counted);
        let c4 = c(
            &mut rt,
            "net/neoforged/neoforge/common/util/NeoForgeExtraCodecs.dispatchMapOrElse.(Lcom/mojang/serialization/Codec;Ljava/util/function/Function;Ljava/util/function/Function;Lcom/mojang/serialization/MapCodec;)Lcom/mojang/serialization/MapCodec;",
            &[string, type_of, codec_of, fallback],
        );
        let typed = decode(&mut rt, c4, &serde_json::json!({"type": "test:compound", "count": 3})).unwrap();
        assert_eq!(rt.get_field(typed, "$isLeft"), V::I(1));
        assert_eq!(rt.get_field(rt.get_field(typed, "$left"), "value"), V::I(3));
        let plain = decode(&mut rt, c4, &serde_json::json!({"chemical": "mekanism:hydrogen"})).unwrap();
        assert_eq!(rt.get_field(plain, "$isLeft"), V::I(0));
        assert_eq!(rt.str_of(rt.get_field(plain, "$right")), Some("mekanism:hydrogen"));
        let written = encode(&mut rt, c4, typed).unwrap();
        assert_eq!(crate::recipes::nbt_json(&written), serde_json::json!({"type": "test:compound", "count": 3}));
        let written = encode(&mut rt, c4, plain).unwrap();
        assert_eq!(crate::recipes::nbt_json(&written), serde_json::json!({"chemical": "mekanism:hydrogen"}));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own `new StringRepresentable.EnumCodec(values, createNameLookup(values, identity()))`
    /// (Mekanism's) reads and writes a constant by its serialized name, and `byName` finds one, or
    /// gives the default; `fromEnum`'s codec answers `byName` the same.
    #[test]
    fn an_enum_codec_reads_and_finds_constants_by_serialized_name() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let values = crate::api::call(&mut rt, "net/minecraft/world/item/DyeColor.values.()[Lnet/minecraft/world/item/DyeColor;", &[]);
        let identity = crate::api::call(&mut rt, "java/util/function/Function.identity.()Ljava/util/function/Function;", &[]);
        let lookup = crate::api::call(
            &mut rt,
            "net/minecraft/util/StringRepresentable.createNameLookup.([Lnet/minecraft/util/StringRepresentable;Ljava/util/function/Function;)Ljava/util/function/Function;",
            &[values, identity],
        );
        let c = rt.new_object("net/minecraft/util/StringRepresentable$EnumCodec");
        crate::api::call(&mut rt, "net/minecraft/util/StringRepresentable$EnumCodec.<init>.([Ljava/lang/Enum;Ljava/util/function/Function;)V", &[c, values, lookup]);
        let lime = decode(&mut rt, c, &Json::String("lime".into())).unwrap();
        assert_eq!(lime, st(&mut rt, "net/minecraft/world/item/DyeColor.LIME"));
        assert!(decode(&mut rt, c, &Json::String("chartreuse".into())).is_err());
        assert_eq!(encode(&mut rt, c, lime), Ok(Nbt::String("lime".into())));
        const BY_NAME: &str = "net/minecraft/util/StringRepresentable$EnumCodec.byName.(Ljava/lang/String;)Ljava/lang/Enum;";
        let red = st(&mut rt, "net/minecraft/world/item/DyeColor.RED");
        let n = rt.string("red");
        assert_eq!(crate::api::call(&mut rt, BY_NAME, &[c, n]), red);
        let (none, white) = (rt.string("none"), st(&mut rt, "net/minecraft/world/item/DyeColor.WHITE"));
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/util/StringRepresentable$EnumCodec.byName.(Ljava/lang/String;Ljava/lang/Enum;)Ljava/lang/Enum;", &[c, none, white]), white);
        assert_eq!(crate::api::call(&mut rt, BY_NAME, &[c, none]), V::NULL);
        let supplier = crate::functions::constant(&mut rt, values);
        let from = crate::api::call(&mut rt, "net/minecraft/util/StringRepresentable.fromEnum.(Ljava/util/function/Supplier;)Lnet/minecraft/util/StringRepresentable$EnumCodec;", &[supplier]);
        assert_eq!(crate::api::call(&mut rt, BY_NAME, &[from, n]), red);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Vanilla's value codecs write what 1.21.1 writes (`Vec3` three doubles, a UUID four ints, a
    /// rotation its serialized name) and read it back.
    /// A mod's loot function codec: `RecordCodecBuilder.mapCodec(i -> commonFields(i).and(INT.fieldOf("count")
    /// ...).apply(i, ctor))`. Its `conditions` are optional: a function written without them reads,
    /// with an empty list for the constructor, as vanilla's does.
    #[test]
    fn a_loot_functions_shared_fields_read_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let instance = rt.intern("compat:codec/instance".into(), |rt| codec(rt, "compat:codec/instance", vec![]));
        let common = crate::api::call(
            &mut rt,
            "net/minecraft/world/level/storage/loot/functions/LootItemConditionalFunction.commonFields.(Lcom/mojang/serialization/codecs/RecordCodecBuilder$Instance;)Lcom/mojang/datafixers/Products$P1;",
            &[instance],
        );
        assert!(is_codec(&rt, common) && kind(&rt, common) == "products");
        let int = crate::api::get_static(&mut rt, "com/mojang/serialization/Codec.INT");
        let count = rt.string("count");
        let count = crate::api::call(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[int, count]);
        let count = crate::api::call(&mut rt, "com/mojang/serialization/MapCodec.forGetter.(Ljava/util/function/Function;)Lcom/mojang/serialization/codecs/RecordCodecBuilder;", &[count, V::NULL]);
        let both = crate::api::call(&mut rt, "com/mojang/datafixers/Products$P1.and.(Lcom/mojang/datafixers/kinds/App;)Lcom/mojang/datafixers/Products$P2;", &[common, count]);
        assert_eq!(rt.captured(both).len(), 2, "conditions, then count");
        let fields = rt.captured(both);
        let j: Json = serde_json::from_str(r#"{"count": 3}"#).unwrap();
        let Json::Object(o) = &j else { unreachable!() };
        let conditions = app_value(&mut rt, fields[0], o).unwrap();
        assert_eq!(jutil::elements(&rt, conditions).len(), 0, "no conditions: the empty list");
        let n = app_value(&mut rt, fields[1], o).unwrap();
        assert_eq!(rt.get_field(n, "value"), V::I(3));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `GlobalPos.CODEC` reads and writes `{dimension, pos}`, the position as three ints, as vanilla's
    /// record codec does.
    #[test]
    fn a_global_position_reads_and_writes_as_vanillas_codec() {
        let mut rt = Rt::new();
        crate::install();
        // What generated code does for an object no mod class is: the API call by the method's key.
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        let codec = crate::api::get_static(&mut rt, "net/minecraft/core/GlobalPos.CODEC");
        let j: Json = serde_json::from_str(r#"{"dimension": "minecraft:the_end", "pos": [4, 64, -7]}"#).unwrap();
        let at = decode(&mut rt, codec, &j).unwrap();
        let dimension = crate::api::call(&mut rt, "net/minecraft/core/GlobalPos.dimension.()Lnet/minecraft/resources/ResourceKey;", &[at]);
        assert_eq!(rt.str_of(dimension), Some("dimension#minecraft:the_end"));
        let pos = crate::api::call(&mut rt, "net/minecraft/core/GlobalPos.pos.()Lnet/minecraft/core/BlockPos;", &[at]);
        assert_eq!(crate::api::pos_of(&rt, pos), [4, 64, -7]);
        let written = encode(&mut rt, codec, at).unwrap();
        let mut want = BTreeMap::new();
        want.insert("dimension".to_owned(), Nbt::String("minecraft:the_end".into()));
        want.insert("pos".to_owned(), Nbt::IntArray(vec![4, 64, -7]));
        assert_eq!(written, Nbt::Compound(want));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn vanilla_value_codecs_write_and_read_as_vanillas() {
        let mut rt = Rt::new();
        let vec3 = st(&mut rt, "net/minecraft/world/phys/Vec3.CODEC");
        let v = crate::api_common::vec3(&mut rt, 1.5, -2.0, 0.25);
        assert_eq!(encode(&mut rt, vec3, v), Ok(Nbt::List(vec![Nbt::Double(1.5), Nbt::Double(-2.0), Nbt::Double(0.25)])));
        let back = decode(&mut rt, vec3, &serde_json::json!([1.5, -2.0, 0.25])).unwrap();
        assert_eq!(rt.get_field(back, "y"), V::D(-2.0));
        let uuid = st(&mut rt, "net/minecraft/core/UUIDUtil.CODEC");
        let u = crate::tags::uuid(&mut rt, 0x0123_4567_89ab_cdef, -2);
        assert_eq!(encode(&mut rt, uuid, u), Ok(Nbt::IntArray(vec![0x0123_4567, 0x89ab_cdef_u32 as i32, -1, -2])));
        let back = decode(&mut rt, uuid, &serde_json::json!([0x0123_4567, 0x89ab_cdef_u32 as i32, -1, -2])).unwrap();
        assert_eq!(crate::tags::uuid_bits(&rt, back), (0x0123_4567_89ab_cdef, -2));
        let dir = st(&mut rt, "net/minecraft/core/Direction.CODEC");
        let north = st(&mut rt, "net/minecraft/core/Direction.NORTH");
        assert_eq!(encode(&mut rt, dir, north), Ok(Nbt::String("north".into())));
        assert_eq!(decode(&mut rt, dir, &serde_json::json!("north")), Ok(north));
        let rot = st(&mut rt, "net/minecraft/world/level/block/Rotation.CODEC");
        let half = st(&mut rt, "net/minecraft/world/level/block/Rotation.CLOCKWISE_180");
        assert_eq!(encode(&mut rt, rot, half), Ok(Nbt::String("180".into())));
        assert_eq!(decode(&mut rt, rot, &serde_json::json!("180")), Ok(half));
        let ints = st(&mut rt, "com/mojang/serialization/Codec.INT_STREAM");
        let read = decode(&mut rt, ints, &serde_json::json!([4, -5, 6])).unwrap();
        assert_eq!(encode(&mut rt, ints, read), Ok(Nbt::IntArray(vec![4, -5, 6])));
        // `LONG_STREAM`, as Mekanism's QIO drive keeps its counts: `LongStream.of(long[])` written as
        // a long array, read back to a `long[]` by `toArray()`.
        let longs = st(&mut rt, "com/mojang/serialization/Codec.LONG_STREAM");
        let counts = rt.alloc("[J", Data::Array(vec![V::J(1 << 40), V::J(-3)]));
        let stream = c(&mut rt, "java/util/stream/LongStream.of.([J)Ljava/util/stream/LongStream;", &[counts]);
        let written = encode(&mut rt, longs, stream).unwrap();
        assert_eq!(written, Nbt::LongArray(vec![1 << 40, -3]));
        let back = decode(&mut rt, longs, &crate::recipes::nbt_json(&written)).unwrap();
        let array = c(&mut rt, "java/util/stream/LongStream.toArray.()[J", &[back]);
        assert_eq!((rt.class_of(array), jutil::elements(&rt, array)), ("[J", vec![V::J(1 << 40), V::J(-3)]));
    }

    #[test]
    fn a_field_codec_reads_its_key_and_an_optional_one_its_default() {
        let mut rt = Rt::new();
        crate::install();
        let int = st(&mut rt, "com/mojang/serialization/Codec.INT");
        let name = rt.string("processing_time");
        let field = c(&mut rt, "com/mojang/serialization/codecs/PrimitiveCodec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[int, name]);
        let as_codec = c(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[field]);
        let v = decode(&mut rt, as_codec, &serde_json::json!({"processing_time": 150})).unwrap();
        assert_eq!(rt.get_field(v, "value"), V::I(150));
        assert!(decode(&mut rt, as_codec, &serde_json::json!({"other": 1})).is_err());

        let zero = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(0)]);
        let opt = c(
            &mut rt,
            "com/mojang/serialization/codecs/PrimitiveCodec.optionalFieldOf.(Ljava/lang/String;Ljava/lang/Object;)Lcom/mojang/serialization/MapCodec;",
            &[int, name, zero],
        );
        let as_codec = c(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[opt]);
        assert_eq!(decode(&mut rt, as_codec, &serde_json::json!({})).unwrap(), zero);
        // Present but not an int: an error, not the default (DFU's optional fields are strict).
        assert!(decode(&mut rt, as_codec, &serde_json::json!({"processing_time": "slow"})).is_err());
    }

    #[test]
    fn either_tries_its_left_codec_then_its_right_one() {
        let mut rt = Rt::new();
        crate::install();
        let int = st(&mut rt, "com/mojang/serialization/Codec.INT");
        let string = st(&mut rt, "com/mojang/serialization/Codec.STRING");
        let e = c(
            &mut rt,
            "com/mojang/serialization/Codec.either.(Lcom/mojang/serialization/Codec;Lcom/mojang/serialization/Codec;)Lcom/mojang/serialization/Codec;",
            &[int, string],
        );
        let list = c(&mut rt, "com/mojang/serialization/Codec.listOf.()Lcom/mojang/serialization/Codec;", &[e]);
        let v = decode(&mut rt, list, &serde_json::json!([3, "three"])).unwrap();
        let items = jutil::elements(&rt, v);
        assert_eq!(rt.get_field(items[0], "$isLeft"), V::I(1));
        assert_eq!(rt.get_field(items[1], "$isLeft"), V::I(0));
        assert_eq!(rt.str_of(rt.get_field(items[1], "$right")), Some("three"));
        assert!(decode(&mut rt, list, &serde_json::json!([true])).is_err());
    }

    /// The mod's side of a record codec as Create's `FactoryPanelPosition.CODEC` is built: the
    /// function `RecordCodecBuilder.create` calls, the fields' getters, the constructor; and an
    /// `xmap`'s two ways.
    fn panel(rt: &mut Rt, _key: &'static str, _: &'static str, args: &[V]) -> V {
        let field = |rt: &mut Rt, codec: V, name: &str, getter: &'static str| {
            let n = rt.string(name);
            let f = c(rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[codec, n]);
            let g = rt.new_object(getter);
            c(rt, "com/mojang/serialization/MapCodec.forGetter.(Ljava/util/function/Function;)Lcom/mojang/serialization/codecs/RecordCodecBuilder;", &[f, g])
        };
        match rt.class_of(args[0]) {
            "test/Build" => {
                let (pos, slot) = (st(rt, "net/minecraft/core/BlockPos.CODEC"), st(rt, "com/mojang/serialization/Codec.STRING"));
                let (p, s) = (field(rt, pos, "pos", "test/PosOf"), field(rt, slot, "slot", "test/SlotOf"));
                let group = c(rt, "com/mojang/serialization/codecs/RecordCodecBuilder$Instance.group.(Lcom/mojang/datafixers/kinds/App;Lcom/mojang/datafixers/kinds/App;)Lcom/mojang/datafixers/Products$P2;", &[args[1], p, s]);
                let new = rt.new_object("test/New");
                c(rt, "com/mojang/datafixers/Products$P2.apply.(Lcom/mojang/datafixers/kinds/Applicative;Ljava/util/function/BiFunction;)Lcom/mojang/datafixers/kinds/App;", &[group, args[1], new])
            }
            // `group(pos).and(slot).and(count)`: a group grown one field at a time, then a `Function3`.
            "test/Build3" => {
                let (pos, slot, int) = (st(rt, "net/minecraft/core/BlockPos.CODEC"), st(rt, "com/mojang/serialization/Codec.STRING"), st(rt, "com/mojang/serialization/Codec.INT"));
                let (p, s, n) = (field(rt, pos, "pos", "test/PosOf"), field(rt, slot, "slot", "test/SlotOf"), field(rt, int, "count", "test/CountOf"));
                let one = c(rt, "com/mojang/serialization/codecs/RecordCodecBuilder$Instance.group.(Lcom/mojang/datafixers/kinds/App;)Lcom/mojang/datafixers/Products$P1;", &[args[1], p]);
                let two = c(rt, "com/mojang/datafixers/Products$P1.and.(Lcom/mojang/datafixers/kinds/App;)Lcom/mojang/datafixers/Products$P2;", &[one, s]);
                let three = c(rt, "com/mojang/datafixers/Products$P2.and.(Lcom/mojang/datafixers/kinds/App;)Lcom/mojang/datafixers/Products$P3;", &[two, n]);
                let new = rt.new_object("test/New3");
                c(rt, "com/mojang/datafixers/Products$P3.apply.(Lcom/mojang/datafixers/kinds/Applicative;Lcom/mojang/datafixers/util/Function3;)Lcom/mojang/datafixers/kinds/App;", &[three, args[1], new])
            }
            "test/New3" => {
                let o = rt.new_object("test/Panel");
                rt.put_field(o, "pos", args[1]);
                rt.put_field(o, "slot", args[2]);
                rt.put_field(o, "count", args[3]);
                o
            }
            "test/CountOf" => rt.get_field(args[1], "count"),
            "test/PosOf" => rt.get_field(args[1], "pos"),
            "test/SlotOf" => rt.get_field(args[1], "slot"),
            "test/New" => {
                let o = rt.new_object("test/Panel");
                rt.put_field(o, "pos", args[1]);
                rt.put_field(o, "slot", args[2]);
                o
            }
            "test/Wrap" => {
                let o = rt.new_object("test/Ticks");
                let n = rt.get_field(args[1], "value");
                rt.put_field(o, "n", n);
                o
            }
            "test/Unwrap" => {
                let n = rt.get_field(args[1], "n");
                c(rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[n])
            }
            _ => V::Void,
        }
    }

    #[test]
    fn a_record_writes_each_field_its_getter_gives_and_reads_back_the_same() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(panel);
        let build = rt.new_object("test/Build");
        let record = c(&mut rt, "com/mojang/serialization/codecs/RecordCodecBuilder.create.(Ljava/util/function/Function;)Lcom/mojang/serialization/Codec;", &[build]);
        let list = c(&mut rt, "com/mojang/serialization/Codec.list.(Lcom/mojang/serialization/Codec;)Lcom/mojang/serialization/Codec;", &[record]);
        // No linked panels: an empty list (Create's `FactoryPanelSupportBehaviour.write`).
        let none = jutil::new_list(&mut rt, Vec::new());
        assert_eq!(encode(&mut rt, list, none), Ok(Nbt::List(Vec::new())));
        let new = rt.new_object("test/New");
        let pos = crate::api::block_pos(&mut rt, 1, -2, 3);
        let slot = rt.string("TOP_LEFT");
        let one = rt.call_functional(BI_FUNCTION, new, &[pos, slot]);
        let panels = jutil::new_list(&mut rt, vec![one]);
        let written = Nbt::List(vec![Nbt::Compound(BTreeMap::from([
            ("pos".to_owned(), Nbt::IntArray(vec![1, -2, 3])),
            ("slot".to_owned(), Nbt::String("TOP_LEFT".to_owned())),
        ]))]);
        assert_eq!(encode(&mut rt, list, panels), Ok(written));
        // `encodeStart(NbtOps.INSTANCE, ...)` hands the tag over; parsing it reads the panel again.
        let ops = st(&mut rt, "net/minecraft/nbt/NbtOps.INSTANCE");
        let r = c(&mut rt, "com/mojang/serialization/Codec.encodeStart.(Lcom/mojang/serialization/DynamicOps;Ljava/lang/Object;)Lcom/mojang/serialization/DataResult;", &[list, ops, panels]);
        let tag = result_of(&mut rt, r).unwrap();
        assert_eq!(rt.class_of(tag), "net/minecraft/nbt/ListTag");
        let json = crate::recipes::to_json(&mut rt, tag);
        let read = decode(&mut rt, list, &json).unwrap();
        let back = jutil::elements(&rt, read);
        assert_eq!(back.len(), 1);
        assert_eq!(crate::api::pos_of(&rt, rt.get_field(back[0], "pos")), [1, -2, 3]);
        assert_eq!(rt.str_of(rt.get_field(back[0], "slot")), Some("TOP_LEFT"));
    }

    /// Mekanism's `SerializerHelper.oneRequired(main, secondary, getter)` over a record of two
    /// optional strings: `main.dependent(getter, secondary, m -> m.isEmpty() ? required : secondary)`,
    /// `required` a `MapDecoder` of the mod's own that fails where the secondary is absent.
    fn one_required(rt: &mut Rt, _key: &'static str, _: &'static str, args: &[V]) -> V {
        let optional_string = |rt: &mut Rt, name: &str| {
            let s = st(rt, "com/mojang/serialization/Codec.STRING");
            let n = rt.string(name);
            c(rt, "com/mojang/serialization/Codec.optionalFieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[s, n])
        };
        match rt.class_of(args[0]) {
            "test/Build" => {
                let main = optional_string(rt, "main");
                let main_of = rt.new_object("test/MainOf");
                let main = c(rt, "com/mojang/serialization/MapCodec.forGetter.(Ljava/util/function/Function;)Lcom/mojang/serialization/codecs/RecordCodecBuilder;", &[main, main_of]);
                let secondary = optional_string(rt, "secondary");
                let required = rt.new_object("test/Required");
                rt.put_field(required, "codec", secondary);
                let pick = rt.new_object("test/Pick");
                rt.put_field(pick, "codec", secondary);
                rt.put_field(pick, "required", required);
                let secondary_of = rt.new_object("test/SecondaryOf");
                let dependent = c(
                    rt,
                    "com/mojang/serialization/codecs/RecordCodecBuilder.dependent.(Ljava/util/function/Function;Lcom/mojang/serialization/MapEncoder;Ljava/util/function/Function;)Lcom/mojang/serialization/codecs/RecordCodecBuilder;",
                    &[main, secondary_of, secondary, pick],
                );
                let group = c(rt, "com/mojang/serialization/codecs/RecordCodecBuilder$Instance.group.(Lcom/mojang/datafixers/kinds/App;Lcom/mojang/datafixers/kinds/App;)Lcom/mojang/datafixers/Products$P2;", &[args[1], main, dependent]);
                let new = rt.new_object("test/New");
                c(rt, "com/mojang/datafixers/Products$P2.apply.(Lcom/mojang/datafixers/kinds/Applicative;Ljava/util/function/BiFunction;)Lcom/mojang/datafixers/kinds/App;", &[group, args[1], new])
            }
            "test/MainOf" => rt.get_field(args[1], "main"),
            "test/SecondaryOf" => rt.get_field(args[1], "secondary"),
            "test/New" => {
                let o = rt.new_object("test/Outputs");
                rt.put_field(o, "main", args[1]);
                rt.put_field(o, "secondary", args[2]);
                o
            }
            "test/Pick" => {
                let empty = c(rt, "java/util/Optional.isEmpty.()Z", &[args[1]]);
                rt.get_field(args[0], if empty.i() != 0 { "required" } else { "codec" })
            }
            "test/Required" => {
                let codec = rt.get_field(args[0], "codec");
                let r = c(rt, "com/mojang/serialization/MapCodec.decode.(Lcom/mojang/serialization/DynamicOps;Lcom/mojang/serialization/MapLike;)Lcom/mojang/serialization/DataResult;", &[codec, args[1], args[2]]);
                let value = result_of(rt, r).unwrap_or(V::NULL);
                if c(rt, "java/util/Optional.isPresent.()Z", &[value]).i() != 0 { r } else { error(rt, "Missing value") }
            }
            _ => V::Void,
        }
    }

    #[test]
    fn a_dependent_field_reads_with_the_decoder_the_field_before_picks() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(one_required);
        rt.supers.insert("test/Required", "com/mojang/serialization/MapDecoder$Implementation");
        rt.interfaces.insert("test/Required", &["com/mojang/serialization/MapDecoder"]);
        let build = rt.new_object("test/Build");
        let record = c(&mut rt, "com/mojang/serialization/codecs/RecordCodecBuilder.create.(Ljava/util/function/Function;)Lcom/mojang/serialization/Codec;", &[build]);
        let text = |rt: &mut Rt, o: V, field: &'static str| {
            let opt = rt.get_field(o, field);
            let v = rt.get_field(opt, "value");
            rt.str_of(v).map(str::to_owned)
        };
        // The main output alone; the secondary alone (the mod's decoder reads it).
        let read = decode(&mut rt, record, &serde_json::json!({"main": "ingot"})).unwrap();
        assert_eq!((text(&mut rt, read, "main"), text(&mut rt, read, "secondary")), (Some("ingot".into()), None));
        let read = decode(&mut rt, record, &serde_json::json!({"secondary": "dust"})).unwrap();
        assert_eq!((text(&mut rt, read, "main"), text(&mut rt, read, "secondary")), (None, Some("dust".into())));
        // Neither: the mod's decoder fails, and so does the record.
        let err = decode(&mut rt, record, &serde_json::json!({})).unwrap_err();
        assert!(err.contains("Missing value"), "{err}");
        // Written: each field from its getter, the dependent one by its encoder.
        let both = decode(&mut rt, record, &serde_json::json!({"main": "ingot", "secondary": "dust"})).unwrap();
        let written = Nbt::Compound(BTreeMap::from([("main".to_owned(), Nbt::String("ingot".into())), ("secondary".to_owned(), Nbt::String("dust".into()))]));
        assert_eq!(encode(&mut rt, record, both), Ok(written));
    }

    /// A block item's state properties (`minecraft:block_state` on a stack): a map of strings in and
    /// out, the record around it.
    #[test]
    fn block_item_state_properties_read_and_write_as_a_map_of_names() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let c = st(&mut rt, "net/minecraft/world/item/component/BlockItemStateProperties.CODEC");
        let read = decode(&mut rt, c, &serde_json::json!({"facing": "north", "lit": "true"})).unwrap();
        assert_eq!(rt.class_of(read), "net/minecraft/world/item/component/BlockItemStateProperties");
        let written = Nbt::Compound(BTreeMap::from([("facing".to_owned(), Nbt::String("north".into())), ("lit".to_owned(), Nbt::String("true".into()))]));
        assert_eq!(encode(&mut rt, c, read), Ok(written));
        assert!(decode(&mut rt, c, &serde_json::json!({"facing": 3})).is_err());
    }

    #[test]
    fn a_group_grown_with_and_reads_and_writes_every_field() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(panel);
        let build = rt.new_object("test/Build3");
        let record = c(&mut rt, "com/mojang/serialization/codecs/RecordCodecBuilder.create.(Ljava/util/function/Function;)Lcom/mojang/serialization/Codec;", &[build]);
        let read = decode(&mut rt, record, &serde_json::json!({"pos": [4, 5, 6], "slot": "BOTTOM", "count": 7})).unwrap();
        assert_eq!(crate::api::pos_of(&rt, rt.get_field(read, "pos")), [4, 5, 6]);
        assert_eq!(rt.str_of(rt.get_field(read, "slot")), Some("BOTTOM"));
        let written = Nbt::Compound(BTreeMap::from([
            ("count".to_owned(), Nbt::Int(7)),
            ("pos".to_owned(), Nbt::IntArray(vec![4, 5, 6])),
            ("slot".to_owned(), Nbt::String("BOTTOM".to_owned())),
        ]));
        assert_eq!(encode(&mut rt, record, read), Ok(written));
    }

    #[test]
    fn keys_enums_and_text_read_and_write_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let fluid = rt.string("fluid");
        let tags = c(&mut rt, "net/minecraft/tags/TagKey.codec.(Lnet/minecraft/resources/ResourceKey;)Lcom/mojang/serialization/Codec;", &[fluid]);
        let water = decode(&mut rt, tags, &serde_json::json!("c:water")).unwrap();
        assert_eq!(water, crate::fluids::tag_key(&mut rt, "fluid", "c:water"), "the same key `TagKey.create` gives");
        assert_eq!(encode(&mut rt, tags, water), Ok(Nbt::String("c:water".to_owned())));
        let hashed = c(&mut rt, "net/minecraft/tags/TagKey.hashedCodec.(Lnet/minecraft/resources/ResourceKey;)Lcom/mojang/serialization/Codec;", &[fluid]);
        assert!(decode(&mut rt, hashed, &serde_json::json!("c:water")).is_err());
        assert_eq!(decode(&mut rt, hashed, &serde_json::json!("#c:water")), Ok(water));
        assert_eq!(encode(&mut rt, hashed, water), Ok(Nbt::String("#c:water".to_owned())));
        let dimension = rt.string("dimension");
        let keys = c(&mut rt, "net/minecraft/resources/ResourceKey.codec.(Lnet/minecraft/resources/ResourceKey;)Lcom/mojang/serialization/Codec;", &[dimension]);
        let nether = decode(&mut rt, keys, &serde_json::json!("the_nether")).unwrap();
        let id = rt.string("minecraft:the_nether");
        let made = c(&mut rt, "net/minecraft/resources/ResourceKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[dimension, id]);
        assert_eq!(rt.str_of(nether), rt.str_of(made));
        assert_eq!(encode(&mut rt, keys, nether), Ok(Nbt::String("minecraft:the_nether".to_owned())));
        // A block-state enum's codec, by serialized name; `PistonType.DEFAULT` is `normal`.
        let weather = st(&mut rt, "net/minecraft/world/level/block/WeatheringCopper$WeatherState.CODEC");
        let exposed = st(&mut rt, "net/minecraft/world/level/block/WeatheringCopper$WeatherState.EXPOSED");
        assert_eq!(decode(&mut rt, weather, &serde_json::json!("exposed")), Ok(exposed));
        assert_eq!(encode(&mut rt, weather, exposed), Ok(Nbt::String("exposed".to_owned())));
        let piston = st(&mut rt, "net/minecraft/world/level/block/state/properties/PistonType.DEFAULT");
        assert_eq!(rt.str_of(rt.get_field(piston, "$name")), Some("DEFAULT"));
        let shown = c(&mut rt, "net/minecraft/world/level/block/state/properties/PistonType.getSerializedName.()Ljava/lang/String;", &[piston]);
        assert_eq!(rt.str_of(shown), Some("normal"));
        let text = st(&mut rt, "net/minecraft/network/chat/ComponentSerialization.CODEC");
        let hi = decode(&mut rt, text, &serde_json::json!({"text": "Hi"})).unwrap();
        assert_eq!(rt.str_of(rt.get_field(hi, "$text")), Some("Hi"));
        // A codec the runtime doesn't model yet is still a codec (a record built on it stays
        // whole); reading with it fails.
        let food = st(&mut rt, "net/minecraft/world/food/FoodProperties.DIRECT_CODEC");
        assert!(is_codec(&rt, food));
        assert!(decode(&mut rt, food, &serde_json::json!({"nutrition": 4})).is_err());
    }

    #[test]
    fn a_mapped_codec_writes_through_its_way_back_and_an_optional_field_leaves_out_its_default() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(panel);
        let int = st(&mut rt, "com/mojang/serialization/Codec.INT");
        let (wrap, unwrap) = (rt.new_object("test/Wrap"), rt.new_object("test/Unwrap"));
        let ticks = c(&mut rt, "com/mojang/serialization/Codec.xmap.(Ljava/util/function/Function;Ljava/util/function/Function;)Lcom/mojang/serialization/Codec;", &[int, wrap, unwrap]);
        let five = rt.new_object("test/Ticks");
        rt.put_field(five, "n", V::I(5));
        assert_eq!(encode(&mut rt, ticks, five), Ok(Nbt::Int(5)));
        let name = rt.string("count");
        let zero = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(0)]);
        let opt = c(&mut rt, "com/mojang/serialization/codecs/PrimitiveCodec.optionalFieldOf.(Ljava/lang/String;Ljava/lang/Object;)Lcom/mojang/serialization/MapCodec;", &[int, name, zero]);
        let as_codec = c(&mut rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[opt]);
        let again = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(0)]);
        assert_eq!(encode(&mut rt, as_codec, again), Ok(Nbt::Compound(BTreeMap::new())));
        let three = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(3)]);
        assert_eq!(encode(&mut rt, as_codec, three), Ok(Nbt::Compound(BTreeMap::from([("count".to_owned(), Nbt::Int(3))]))));
    }

    #[test]
    fn a_range_refuses_what_lies_outside() {
        let mut rt = Rt::new();
        crate::install();
        let positive = st(&mut rt, "net/minecraft/util/ExtraCodecs.POSITIVE_INT");
        assert!(decode(&mut rt, positive, &serde_json::json!(4)).is_ok());
        assert!(decode(&mut rt, positive, &serde_json::json!(0)).is_err());
    }

    /// A mod's `fromString` (the frequency types Mekanism names, StorageDrawers' key types): known
    /// names only, `toString` back. What `Codec.stringResolver` is made of.
    fn names(rt: &mut Rt, key: &'static str, _: &'static str, args: &[V]) -> V {
        let arg = rt.str_of(args[1]).map(str::to_owned);
        match rt.class_of(args[0]) {
            "test/FromName" => match arg.as_deref() {
                Some("public") => rt.string("PUBLIC"),
                Some("private") => rt.string("PRIVATE"),
                _ => V::NULL,
            },
            "test/ToName" => match arg.as_deref() {
                Some("PUBLIC") => rt.string("public"),
                _ => V::NULL,
            },
            _ => crate::api::call(rt, key, args),
        }
    }

    /// DFU's `stringResolver`: a name its function knows reads, one it doesn't is an error both
    /// ways, as DFU's `Optional.ofNullable(...).orElseGet(error)`.
    #[test]
    fn a_string_resolver_reads_only_the_names_its_functions_know() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(names);
        let (to, from) = (rt.new_object("test/ToName"), rt.new_object("test/FromName"));
        let resolver = c(&mut rt, "com/mojang/serialization/Codec.stringResolver.(Ljava/util/function/Function;Ljava/util/function/Function;)Lcom/mojang/serialization/Codec;", &[to, from]);
        let v = decode(&mut rt, resolver, &serde_json::json!("public")).unwrap();
        assert_eq!(rt.str_of(v), Some("PUBLIC"));
        assert_eq!(decode(&mut rt, resolver, &serde_json::json!("trusted")), Err("Unknown element name:trusted".to_owned()));
        assert_eq!(encode(&mut rt, resolver, v), Ok(Nbt::String("public".into())));
        let private = rt.string("PRIVATE");
        assert!(encode(&mut rt, resolver, private).is_err(), "a value toString names nothing");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ExtraCodecs.optionalEmptyMap`: `{}` is an empty Optional, written back as `{}`; anything
    /// else is the codec's. `Codec.checkRange` refuses what lies outside, as `validate` asks it.
    #[test]
    fn an_empty_map_is_an_empty_optional_and_a_checked_range_refuses_outside() {
        let mut rt = Rt::new();
        crate::install();
        // What generated code does for an object no mod class is: the API call by the method's key.
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        let int = st(&mut rt, "com/mojang/serialization/Codec.INT");
        let opt = c(&mut rt, "net/minecraft/util/ExtraCodecs.optionalEmptyMap.(Lcom/mojang/serialization/Codec;)Lcom/mojang/serialization/Codec;", &[int]);
        let none = decode(&mut rt, opt, &serde_json::json!({})).unwrap();
        assert_eq!(rt.get_field(none, "value"), V::NULL);
        let some = decode(&mut rt, opt, &serde_json::json!(7)).unwrap();
        assert_eq!(rt.get_field(rt.get_field(some, "value"), "value"), V::I(7));
        assert_eq!(encode(&mut rt, opt, none), Ok(Nbt::Compound(BTreeMap::new())));
        assert_eq!(encode(&mut rt, opt, some), Ok(Nbt::Int(7)));
        assert!(decode(&mut rt, opt, &serde_json::json!("seven")).is_err());

        let long = st(&mut rt, "com/mojang/serialization/Codec.LONG");
        let (lo, hi) = (boxed(&mut rt, "java/lang/Long", V::J(0)), boxed(&mut rt, "java/lang/Long", V::J(i64::MAX)));
        let check = c(&mut rt, "com/mojang/serialization/Codec.checkRange.(Ljava/lang/Number;Ljava/lang/Number;)Ljava/util/function/Function;", &[lo, hi]);
        let checked = c(&mut rt, "com/mojang/serialization/Codec.validate.(Ljava/util/function/Function;)Lcom/mojang/serialization/Codec;", &[long, check]);
        let big = decode(&mut rt, checked, &serde_json::json!(9_000_000_000_i64)).unwrap();
        assert_eq!(rt.get_field(big, "value"), V::J(9_000_000_000));
        assert_eq!(decode(&mut rt, checked, &serde_json::json!(-1)), Err("Value -1 outside of range [0:9223372036854775807]".to_owned()));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The static `Codec.optionalField(name, codec, lenient)` reads as `optionalFieldOf`; NeoForge's
    /// `aliasedFieldOf` reads the first of its names present and writes the first; its `xor` reads
    /// exactly one of two.
    #[test]
    fn optional_aliased_and_exclusive_fields_read_as_neoforge_builds_them() {
        let mut rt = Rt::new();
        crate::install();
        let (int, string) = (st(&mut rt, "com/mojang/serialization/Codec.INT"), st(&mut rt, "com/mojang/serialization/Codec.STRING"));
        let obj = |j: serde_json::Value| match j {
            Json::Object(o) => o,
            _ => unreachable!(),
        };
        let n = rt.string("n");
        let optional = c(&mut rt, "com/mojang/serialization/Codec.optionalField.(Ljava/lang/String;Lcom/mojang/serialization/Codec;Z)Lcom/mojang/serialization/MapCodec;", &[n, int, V::I(0)]);
        let absent = decode_map(&mut rt, optional, &obj(serde_json::json!({}))).unwrap();
        assert_eq!(rt.get_field(absent, "value"), V::NULL);
        let two = decode_map(&mut rt, optional, &obj(serde_json::json!({"n": 2}))).unwrap();
        assert_eq!(rt.get_field(rt.get_field(two, "value"), "value"), V::I(2));
        assert!(decode_map(&mut rt, optional, &obj(serde_json::json!({"n": "two"}))).is_err(), "not lenient");

        let (count, amount) = (rt.string("count"), rt.string("amount"));
        let aliases = rt.alloc("[Ljava/lang/String;", Data::Array(vec![count, amount]));
        let aliased = c(&mut rt, "net/neoforged/neoforge/common/util/NeoForgeExtraCodecs.aliasedFieldOf.(Lcom/mojang/serialization/Codec;[Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[int, aliases]);
        let three = decode_map(&mut rt, aliased, &obj(serde_json::json!({"amount": 3}))).unwrap();
        assert_eq!(rt.get_field(three, "value"), V::I(3));
        let four = decode_map(&mut rt, aliased, &obj(serde_json::json!({"count": 4, "amount": 3}))).unwrap();
        assert_eq!(rt.get_field(four, "value"), V::I(4));
        assert!(decode_map(&mut rt, aliased, &obj(serde_json::json!({}))).is_err());
        let mut out = BTreeMap::new();
        encode_map(&mut rt, aliased, three, &mut out).unwrap();
        assert_eq!(out, BTreeMap::from([("count".to_owned(), Nbt::Int(3))]));

        let (a, b) = (rt.string("a"), rt.string("b"));
        let fa = c(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[int, a]);
        let fb = c(&mut rt, "com/mojang/serialization/Codec.fieldOf.(Ljava/lang/String;)Lcom/mojang/serialization/MapCodec;", &[string, b]);
        let xor = c(&mut rt, "net/neoforged/neoforge/common/util/NeoForgeExtraCodecs.xor.(Lcom/mojang/serialization/MapCodec;Lcom/mojang/serialization/MapCodec;)Lcom/mojang/serialization/MapCodec;", &[fa, fb]);
        let left = decode_map(&mut rt, xor, &obj(serde_json::json!({"a": 1}))).unwrap();
        assert_eq!(rt.get_field(left, "$isLeft"), V::I(1));
        let right = decode_map(&mut rt, xor, &obj(serde_json::json!({"b": "x"}))).unwrap();
        assert_eq!(rt.str_of(rt.get_field(right, "$right")), Some("x"));
        assert!(decode_map(&mut rt, xor, &obj(serde_json::json!({"a": 1, "b": "x"}))).unwrap_err().starts_with("Both alternatives"));
        assert!(decode_map(&mut rt, xor, &obj(serde_json::json!({}))).unwrap_err().starts_with("Neither alternative"));
        let mut out = BTreeMap::new();
        encode_map(&mut rt, xor, right, &mut out).unwrap();
        assert_eq!(out, BTreeMap::from([("b".to_owned(), Nbt::String("x".into()))]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
