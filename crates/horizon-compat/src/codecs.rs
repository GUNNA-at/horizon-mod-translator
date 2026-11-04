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
    rt.class_of(c).strip_prefix("compat:codec/").unwrap_or("")
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
        ("com/mojang/serialization/Codec", f) => match f {
            "INT" | "LONG" | "FLOAT" | "DOUBLE" | "BOOL" | "STRING" | "BYTE" | "SHORT" => prim(rt, &f.to_ascii_lowercase()),
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
        ("net/minecraft/nbt/NbtOps", "INSTANCE") => rt.intern("compat:nbt-ops".into(), |rt| rt.new_object("net/minecraft/nbt/NbtOps")),
        ("com/mojang/serialization/JsonOps", "INSTANCE" | "COMPRESSED") => rt.intern("compat:json-ops".into(), |rt| rt.new_object("com/mojang/serialization/JsonOps")),
        ("net/minecraft/resources/ResourceLocation", "CODEC") => native(rt, "resource_location"),
        ("net/minecraft/core/BlockPos", "CODEC") => native(rt, "block_pos"),
        ("net/minecraft/util/Unit", "CODEC") => native(rt, "unit"),
        ("net/minecraft/world/item/ItemStack", "CODEC" | "STRICT_CODEC" | "SINGLE_ITEM_CODEC") => native(rt, "item_stack"),
        ("net/minecraft/world/item/ItemStack", "OPTIONAL_CODEC") => native(rt, "optional_item_stack"),
        ("net/minecraft/world/item/crafting/Ingredient", "CODEC") => native(rt, "ingredient"),
        ("net/minecraft/world/item/crafting/Ingredient", "CODEC_NONEMPTY") => native(rt, "ingredient_nonempty"),
        ("net/minecraft/world/item/crafting/Recipe", "CODEC" | "CONDITIONAL_CODEC") => native(rt, "recipe"),
        ("net/neoforged/neoforge/fluids/FluidStack", "CODEC" | "FIXED_AMOUNT_CODEC") => native(rt, "fluid_stack"),
        ("net/neoforged/neoforge/fluids/FluidStack", "OPTIONAL_CODEC") => native(rt, "optional_fluid_stack"),
        ("net/neoforged/neoforge/fluids/crafting/SizedFluidIngredient", "FLAT_CODEC" | "NESTED_CODEC") => native(rt, "sized_fluid_ingredient"),
        ("net/neoforged/neoforge/fluids/crafting/FluidIngredient", "MAP_CODEC_NONEMPTY") => native(rt, "fluid_ingredient_map"),
        ("net/neoforged/neoforge/fluids/crafting/FluidIngredient", "CODEC" | "CODEC_NON_EMPTY") => native(rt, "fluid_ingredient"),
        ("net/minecraft/core/component/DataComponentPatch" | "net/minecraft/core/component/DataComponentMap", "CODEC") => native(rt, "component_patch"),
        ("net/minecraft/nbt/CompoundTag", "CODEC") => native(rt, "compound_tag"),
        ("net/minecraft/world/item/DyeColor", "CODEC") => native(rt, "dye_color"),
        ("net/minecraft/world/level/block/state/BlockState", "CODEC") => native(rt, "block_state"),
        // `NeoForgeRegistries.FLUID_INGREDIENT_TYPES` is registry `fluid_ingredient_type`.
        ("net/neoforged/neoforge/registries/NeoForgeRegistries", f) => {
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
            map_value(rt, part(1), part(2).i() != 0, v)
        }
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
        "registry" => {
            let Json::String(id) = j else { return Err(format!("Not a string: {j}")) };
            let registry = rt.str_of(part(0)).unwrap_or("").to_owned();
            registry_value(rt, &registry, id)
        }
        "enum" => {
            let Json::String(name) = j else { return Err(format!("Not a string: {j}")) };
            let values = run(rt, SUPPLIER, part(0), &[])?;
            for v in jutil::elements(rt, values) {
                let n = run(rt, SERIALIZED_NAME, v, &[])?;
                if rt.str_of(n) == Some(name.as_str()) {
                    return Ok(v);
                }
            }
            Err(format!("Unknown element name: {name}"))
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
        "of_map" | "field" | "optional" | "record" | "point" | "dispatch" | "native_map" => {
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
        "lazy" => {
            let inner = lazy(rt, c)?;
            decode_map(rt, inner, o)
        }
        "native_map" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            crate::crafting::decode_map(rt, &name, parts.get(1..).unwrap_or_default(), o)
        }
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
        "field" | "optional" | "record" | "point" | "dispatch" | "of_map" | "unit" | "native_map" => true,
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
        "unit" => Ok(Nbt::Compound(BTreeMap::new())),
        "lazy" => {
            let inner = lazy(rt, c)?;
            encode(rt, inner, value)
        }
        "registry" => {
            let registry = rt.str_of(part(0)).unwrap_or("").to_owned();
            registry_key(rt, &registry, value).map(Nbt::String)
        }
        "enum" => serialized_name(rt, value).map(Nbt::String),
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
        "of_map" | "field" | "optional" | "record" | "point" | "dispatch" | "native_map" => {
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
                if kind(rt, g) == "getter" {
                    let field = rt.captured(g);
                    let (codec, getter) = (field.first().copied().unwrap_or_default(), field.get(1).copied().unwrap_or_default());
                    let v = run(rt, FUNCTION, getter, &[value])?;
                    encode_map(rt, codec, v, out)?;
                }
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
        "either" => {
            let left = rt.get_field(value, "$isLeft").i() != 0;
            let v = rt.get_field(value, if left { "$left" } else { "$right" });
            encode_map(rt, if left { part(0) } else { part(1) }, v, out)?;
        }
        "lazy" => {
            let inner = lazy(rt, c)?;
            encode_map(rt, inner, value, out)?;
        }
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
    if parts.get(4).is_some_and(|f| f.i() != 0) { result_of(rt, r) } else { Ok(r) }
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
        "block_pos" => Nbt::IntArray(crate::api::pos_of(rt, v).to_vec()),
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
                let carried: BTreeMap<String, Nbt> = crate::components::saved(rt, v).into_iter().collect();
                if !carried.is_empty() {
                    m.insert("components".to_owned(), Nbt::Compound(carried));
                }
                Nbt::Compound(m)
            }
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
        "component_patch" => {
            let json = rt.get_field(v, "$json");
            match rt.str_of(json).and_then(|t| serde_json::from_str::<Json>(t).ok()) {
                Some(j) => crate::recipes::json_nbt(&j).unwrap_or(Nbt::Compound(BTreeMap::new())),
                None => Nbt::Compound(BTreeMap::new()),
            }
        }
        other => return Err(format!("codec {other} can't write")),
    })
}

/// Calls on codecs, `DataResult`s, `Either`s and `Pair`s; None for what this module doesn't model.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, rest) = key.split_once('.')?;
    let (method, desc) = rest.split_once('.')?;
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
        ("com/mojang/serialization/Codec", "intRange") | ("net/minecraft/util/ExtraCodecs", "intRange") => {
            return Some(range(rt, "int", a(0).i() as f64, a(1).i() as f64));
        }
        ("com/mojang/serialization/Codec", "floatRange") => return Some(range(rt, "float", a(0).f() as f64, a(1).f() as f64)),
        ("com/mojang/serialization/Codec", "doubleRange") => return Some(range(rt, "double", a(0).d(), a(1).d())),
        ("net/minecraft/util/StringRepresentable", "fromEnum" | "fromValues" | "fromEnumWithMapping") => {
            return Some(codec(rt, "compat:codec/enum", vec![a(0)]));
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
        "keys" => jutil::new_list(rt, Vec::new()),
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
}
