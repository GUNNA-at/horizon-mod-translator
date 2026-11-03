//! Mojang's DataFixerUpper codecs (`com.mojang.serialization`), natively. A mod builds its codecs from
//! DFU's and Minecraft's pieces (`RecordCodecBuilder`, `fieldOf`, `xmap`, `Codec.either`,
//! `Ingredient.CODEC`...). Each piece here is an object recording what it was made of (class
//! `compat:codec/<kind>`, its parts in order), and [`decode`] walks one over a JSON value, calling the
//! mod's own functions where DFU would (a record's constructor, an `xmap`'s mapping, a dispatch's
//! codec for a type). Create's recipe serializers read its recipe files this way. Minecraft's own
//! codecs are read natively ([`crate::recipes::decode_native`]). Also here: DFU's `DataResult`,
//! `Either` and `Pair`, which codecs and the functions they call hand around.
//!
//! Encoding is not modelled: a codec only reads.
use crate::{Data, Rt, Table, V, jutil};
use serde_json::{Map, Value as Json};

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
            return Some(codec(rt, "compat:codec/optional", vec![a(1), a(0), a(2), V::I(1), V::I(0)]));
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
        "xmap" => codec(rt, "compat:codec/xmap", vec![c, a(1), V::I(0)]),
        // `flatComapMap(to, from)`'s `to` maps plainly; the others' `to` answers a DataResult.
        "comapFlatMap" | "flatXmap" | "flatComapMap" => codec(rt, "compat:codec/xmap", vec![c, a(1), V::I((method != "flatComapMap") as i32)]),
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
        "encodeStart" | "encode" => error(rt, "encoding is not modelled"),
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

    #[test]
    fn a_range_refuses_what_lies_outside() {
        let mut rt = Rt::new();
        crate::install();
        let positive = st(&mut rt, "net/minecraft/util/ExtraCodecs.POSITIVE_INT");
        assert!(decode(&mut rt, positive, &serde_json::json!(4)).is_ok());
        assert!(decode(&mut rt, positive, &serde_json::json!(0)).is_err());
    }
}
