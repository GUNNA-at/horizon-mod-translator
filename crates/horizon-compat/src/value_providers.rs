//! Vanilla's value providers (`net/minecraft/util/valueproviders`), natively, as 1.21.1 has them:
//! `IntProvider` (constant, uniform, biased to bottom, clamped, clamped normal, weighted list) and
//! `FloatProvider` (constant, uniform, clamped normal, trapezoid). A mod makes them in code
//! (`UniformInt.of(2, 5)`) or reads them with their codecs (`IntProvider.CODEC`: a number is a
//! constant, an object is dispatched by `type`), and samples them from the random it holds (the
//! generator's, in its worldgen) with vanilla's draws in vanilla's order.
use crate::host::Nbt;
use crate::{Rt, V};
use serde_json::Value as Json;
use std::collections::BTreeMap;

pub const INT_PROVIDER: &str = "net/minecraft/util/valueproviders/IntProvider";
pub const FLOAT_PROVIDER: &str = "net/minecraft/util/valueproviders/FloatProvider";
const CONSTANT_INT: &str = "net/minecraft/util/valueproviders/ConstantInt";
const UNIFORM_INT: &str = "net/minecraft/util/valueproviders/UniformInt";
const BIASED_INT: &str = "net/minecraft/util/valueproviders/BiasedToBottomInt";
const CLAMPED_INT: &str = "net/minecraft/util/valueproviders/ClampedInt";
const NORMAL_INT: &str = "net/minecraft/util/valueproviders/ClampedNormalInt";
const WEIGHTED_INT: &str = "net/minecraft/util/valueproviders/WeightedListInt";
const CONSTANT_FLOAT: &str = "net/minecraft/util/valueproviders/ConstantFloat";
const UNIFORM_FLOAT: &str = "net/minecraft/util/valueproviders/UniformFloat";
const NORMAL_FLOAT: &str = "net/minecraft/util/valueproviders/ClampedNormalFloat";
const TRAPEZOID_FLOAT: &str = "net/minecraft/util/valueproviders/TrapezoidFloat";
const WRAPPER: &str = "net/minecraft/util/random/WeightedEntry$Wrapper";

const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const NEXT_FLOAT: &str = "net/minecraft/util/RandomSource.nextFloat.()F";
const NEXT_GAUSSIAN: &str = "net/minecraft/util/RandomSource.nextGaussian.()D";

/// `IntProviderType`'s kinds by registry name, and their classes.
const INT_KINDS: [(&str, &str); 6] = [
    ("constant", CONSTANT_INT),
    ("uniform", UNIFORM_INT),
    ("biased_to_bottom", BIASED_INT),
    ("clamped", CLAMPED_INT),
    ("clamped_normal", NORMAL_INT),
    ("weighted_list", WEIGHTED_INT),
];
/// `FloatProviderType`'s.
const FLOAT_KINDS: [(&str, &str); 4] =
    [("constant", CONSTANT_FLOAT), ("uniform", UNIFORM_FLOAT), ("clamped_normal", NORMAL_FLOAT), ("trapezoid", TRAPEZOID_FLOAT)];

/// Java's `Float.toString`.
fn ft(f: f32) -> String {
    crate::java_double(f64::from(f), true)
}

fn is_int(class: &str) -> bool {
    class == INT_PROVIDER || INT_KINDS.iter().any(|(_, c)| *c == class)
}

fn is_float(class: &str) -> bool {
    class == FLOAT_PROVIDER || FLOAT_KINDS.iter().any(|(_, c)| *c == class)
}

/// The keys [`call`] answers: the providers' factories and methods.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    is_int(owner) || is_float(owner)
}

fn make(rt: &mut Rt, class: &'static str, fields: &[(&'static str, V)]) -> V {
    let p = rt.new_object(class);
    for (f, v) in fields {
        rt.put_field(p, f, *v);
    }
    p
}

/// `ConstantInt.of(value)`: `ZERO` for 0, as vanilla keeps one.
fn constant_int(rt: &mut Rt, value: i32) -> V {
    if value == 0 {
        return rt.intern("compat:constant-int/0".into(), |rt| make(rt, CONSTANT_INT, &[("value", V::I(0))]));
    }
    make(rt, CONSTANT_INT, &[("value", V::I(value))])
}

/// `ConstantFloat.of(value)`: `ZERO` for 0.
fn constant_float(rt: &mut Rt, value: f32) -> V {
    if value == 0.0 {
        return rt.intern("compat:constant-float/0".into(), |rt| make(rt, CONSTANT_FLOAT, &[("value", V::F(0.0))]));
    }
    make(rt, CONSTANT_FLOAT, &[("value", V::F(value))])
}

/// A weighted list of int providers, with the smallest minimum and the largest maximum of its
/// entries, as `WeightedListInt`'s constructor keeps them.
fn weighted_int(rt: &mut Rt, entries: Vec<(V, i32)>) -> V {
    let (mut min, mut max) = (i32::MAX, i32::MIN);
    let mut wrappers = Vec::with_capacity(entries.len());
    for (provider, weight) in entries {
        min = min.min(int_min(rt, provider));
        max = max.max(int_max(rt, provider));
        wrappers.push(make(rt, WRAPPER, &[("data", provider), ("$weight", V::I(weight))]));
    }
    let list = crate::jutil::new_list(rt, wrappers);
    make(rt, WEIGHTED_INT, &[("$entries", list), ("minValue", V::I(min)), ("maxValue", V::I(max))])
}

fn int_field(rt: &Rt, p: V, f: &'static str) -> i32 {
    rt.get_field(p, f).i()
}

fn float_field(rt: &Rt, p: V, f: &'static str) -> f32 {
    rt.get_field(p, f).f()
}

/// `getMinValue()` of an int provider.
fn int_min(rt: &Rt, p: V) -> i32 {
    match rt.class_of(p) {
        CONSTANT_INT => int_field(rt, p, "value"),
        CLAMPED_INT => int_field(rt, p, "minInclusive").max(int_min(rt, rt.get_field(p, "source"))),
        WEIGHTED_INT => int_field(rt, p, "minValue"),
        _ => int_field(rt, p, "minInclusive"),
    }
}

/// `getMaxValue()` of an int provider.
fn int_max(rt: &Rt, p: V) -> i32 {
    match rt.class_of(p) {
        CONSTANT_INT => int_field(rt, p, "value"),
        CLAMPED_INT => int_field(rt, p, "maxInclusive").min(int_max(rt, rt.get_field(p, "source"))),
        WEIGHTED_INT => int_field(rt, p, "maxValue"),
        _ => int_field(rt, p, "maxInclusive"),
    }
}

/// `getMinValue()` of a float provider.
fn float_min(rt: &Rt, p: V) -> f32 {
    match rt.class_of(p) {
        CONSTANT_FLOAT => float_field(rt, p, "value"),
        UNIFORM_FLOAT => float_field(rt, p, "minInclusive"),
        _ => float_field(rt, p, "min"),
    }
}

/// `getMaxValue()` of a float provider.
fn float_max(rt: &Rt, p: V) -> f32 {
    match rt.class_of(p) {
        CONSTANT_FLOAT => float_field(rt, p, "value"),
        UNIFORM_FLOAT => float_field(rt, p, "maxExclusive"),
        _ => float_field(rt, p, "max"),
    }
}

fn draw_int(rt: &mut Rt, random: V, bound: i32) -> i32 {
    crate::api::call(rt, NEXT_INT, &[random, V::I(bound)]).i()
}

fn draw_float(rt: &mut Rt, random: V) -> f32 {
    crate::api::call(rt, NEXT_FLOAT, &[random]).f()
}

/// `Mth.normal(random, mean, deviation)`: `mean + (float) nextGaussian() * deviation`.
fn normal(rt: &mut Rt, random: V, mean: f32, deviation: f32) -> f32 {
    mean + crate::api::call(rt, NEXT_GAUSSIAN, &[random]).d() as f32 * deviation
}

/// Java's `Mth.clamp(float, float, float)`: below the minimum is the minimum, else at most the
/// maximum (`Math.min`, so NaN stays NaN).
fn clamp_f(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo { lo } else if v.is_nan() || hi.is_nan() { f32::NAN } else { v.min(hi) }
}

/// `sample(random)` of an int provider, drawing as vanilla's does.
fn sample_int(rt: &mut Rt, p: V, random: V) -> i32 {
    match rt.class_of(p) {
        CONSTANT_INT => int_field(rt, p, "value"),
        // `Mth.randomBetweenInclusive`: `nextInt(max - min + 1) + min`.
        UNIFORM_INT => {
            let (min, max) = (int_field(rt, p, "minInclusive"), int_field(rt, p, "maxInclusive"));
            draw_int(rt, random, max.wrapping_sub(min).wrapping_add(1)).wrapping_add(min)
        }
        // `min + nextInt(nextInt(max - min + 1) + 1)`.
        BIASED_INT => {
            let (min, max) = (int_field(rt, p, "minInclusive"), int_field(rt, p, "maxInclusive"));
            let inner = draw_int(rt, random, max.wrapping_sub(min).wrapping_add(1));
            min.wrapping_add(draw_int(rt, random, inner.wrapping_add(1)))
        }
        CLAMPED_INT => {
            let source = rt.get_field(p, "source");
            let v = sample_int(rt, source, random);
            v.max(int_field(rt, p, "minInclusive")).min(int_field(rt, p, "maxInclusive"))
        }
        // `(int) Mth.clamp(Mth.normal(random, mean, deviation), min, max)`.
        NORMAL_INT => {
            let (mean, deviation) = (float_field(rt, p, "mean"), float_field(rt, p, "deviation"));
            let (min, max) = (int_field(rt, p, "minInclusive") as f32, int_field(rt, p, "maxInclusive") as f32);
            clamp_f(normal(rt, random, mean, deviation), min, max) as i32
        }
        // `WeightedRandom.getRandomItem(random, entries, total)`, then that entry's sample.
        WEIGHTED_INT => {
            let entries = crate::jutil::elements(rt, rt.get_field(p, "$entries"));
            let weights: Vec<i32> = entries.iter().map(|e| int_field(rt, *e, "$weight")).collect();
            let total = weights.iter().fold(0i32, |t, w| t.wrapping_add(*w));
            if total <= 0 {
                rt.throw_new("java/lang/IllegalStateException");
                return 0;
            }
            let mut index = draw_int(rt, random, total);
            for (e, w) in entries.iter().zip(&weights) {
                index -= w;
                if index < 0 {
                    let data = rt.get_field(*e, "data");
                    return sample_int(rt, data, random);
                }
            }
            rt.throw_new("java/lang/IllegalStateException");
            0
        }
        _ => 0,
    }
}

/// `sample(random)` of a float provider.
fn sample_float(rt: &mut Rt, p: V, random: V) -> f32 {
    match rt.class_of(p) {
        CONSTANT_FLOAT => float_field(rt, p, "value"),
        // `Mth.randomBetween`: `nextFloat() * (max - min) + min`.
        UNIFORM_FLOAT => {
            let (min, max) = (float_field(rt, p, "minInclusive"), float_field(rt, p, "maxExclusive"));
            draw_float(rt, random) * (max - min) + min
        }
        NORMAL_FLOAT => {
            let (mean, deviation) = (float_field(rt, p, "mean"), float_field(rt, p, "deviation"));
            let (min, max) = (float_field(rt, p, "min"), float_field(rt, p, "max"));
            clamp_f(normal(rt, random, mean, deviation), min, max)
        }
        TRAPEZOID_FLOAT => {
            let (min, max, plateau) = (float_field(rt, p, "min"), float_field(rt, p, "max"), float_field(rt, p, "plateau"));
            let span = max - min;
            let ramp = (span - plateau) / 2.0;
            let rest = span - ramp;
            let a = draw_float(rt, random);
            let b = draw_float(rt, random);
            min + a * rest + b * ramp
        }
        _ => 0.0,
    }
}

/// `toString()`, as vanilla's kinds spell themselves; `ClampedInt` and `WeightedListInt` keep
/// `Object`'s.
fn text(rt: &Rt, p: V) -> Option<String> {
    let i = |f: &'static str| int_field(rt, p, f);
    let f = |f: &'static str| ft(float_field(rt, p, f));
    Some(match rt.class_of(p) {
        CONSTANT_INT => i("value").to_string(),
        UNIFORM_INT | BIASED_INT => format!("[{}-{}]", i("minInclusive"), i("maxInclusive")),
        NORMAL_INT => format!("normal({}, {}) in [{}-{}]", f("mean"), f("deviation"), i("minInclusive"), i("maxInclusive")),
        CONSTANT_FLOAT => f("value"),
        UNIFORM_FLOAT => format!("[{}-{}]", f("minInclusive"), f("maxExclusive")),
        NORMAL_FLOAT => format!("normal({}, {}) in [{}-{}]", f("mean"), f("deviation"), f("min"), f("max")),
        TRAPEZOID_FLOAT => format!("trapezoid({}) in [{}-{}]", f("plateau"), f("min"), f("max")),
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    let (i, f) = (|k: usize| a(k).i(), |k: usize| a(k).f());
    Some(match (owner, name) {
        (CONSTANT_INT, "of") => constant_int(rt, i(0)),
        (UNIFORM_INT, "of") => make(rt, UNIFORM_INT, &[("minInclusive", a(0)), ("maxInclusive", a(1))]),
        (BIASED_INT, "of") => make(rt, BIASED_INT, &[("minInclusive", a(0)), ("maxInclusive", a(1))]),
        (CLAMPED_INT, "of") => make(rt, CLAMPED_INT, &[("source", a(0)), ("minInclusive", a(1)), ("maxInclusive", a(2))]),
        (NORMAL_INT, "of") => make(rt, NORMAL_INT, &[("mean", a(0)), ("deviation", a(1)), ("minInclusive", a(2)), ("maxInclusive", a(3))]),
        (CONSTANT_FLOAT, "of") => constant_float(rt, f(0)),
        // `UniformFloat.of` refuses an empty range.
        (UNIFORM_FLOAT, "of") if f(1) <= f(0) => crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", "Max must exceed min"),
        (UNIFORM_FLOAT, "of") => make(rt, UNIFORM_FLOAT, &[("minInclusive", a(0)), ("maxExclusive", a(1))]),
        (NORMAL_FLOAT, "of") => make(rt, NORMAL_FLOAT, &[("mean", a(0)), ("deviation", a(1)), ("min", a(2)), ("max", a(3))]),
        (TRAPEZOID_FLOAT, "of") => make(rt, TRAPEZOID_FLOAT, &[("min", a(0)), ("max", a(1)), ("plateau", a(2))]),
        // `IntProvider.codec(min, max)` and `FloatProvider.codec(min, max)`: `CODEC` that refuses a
        // provider reaching outside.
        (INT_PROVIDER, "codec") if desc == "(II)Lcom/mojang/serialization/Codec;" => {
            crate::codecs::native(rt, &format!("int_provider/{}/{}", i(0), i(1)))
        }
        (FLOAT_PROVIDER, "codec") if desc == "(FF)Lcom/mojang/serialization/Codec;" => {
            let (min, max) = (ft(f(0)), ft(f(1)));
            crate::codecs::native(rt, &format!("float_provider/{min}/{max}"))
        }
        (o, "sample") if is_int(o) && is_int(rt.class_of(a(0))) => V::I(sample_int(rt, a(0), a(1))),
        (o, "sample") if is_float(o) && is_float(rt.class_of(a(0))) => V::F(sample_float(rt, a(0), a(1))),
        (o, "getMinValue") if is_int(o) && is_int(rt.class_of(a(0))) => V::I(int_min(rt, a(0))),
        (o, "getMaxValue") if is_int(o) && is_int(rt.class_of(a(0))) => V::I(int_max(rt, a(0))),
        (o, "getMinValue") if is_float(o) && is_float(rt.class_of(a(0))) => V::F(float_min(rt, a(0))),
        (o, "getMaxValue") if is_float(o) && is_float(rt.class_of(a(0))) => V::F(float_max(rt, a(0))),
        (CONSTANT_INT | CONSTANT_FLOAT, "getValue") => rt.get_field(a(0), "value"),
        (o, "toString") if is_int(o) || is_float(o) => match text(rt, a(0)) {
            Some(t) => rt.string(&t),
            None => crate::api::call(rt, "java/lang/Object.toString.()Ljava/lang/String;", &[a(0)]),
        },
        _ => return None,
    })
}

/// `ConstantInt.ZERO` and `ConstantFloat.ZERO`; the codecs are [`crate::codecs`]'.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (CONSTANT_INT, "ZERO") => Some(constant_int(rt, 0)),
        (CONSTANT_FLOAT, "ZERO") => Some(constant_float(rt, 0.0)),
        _ => None,
    }
}

/// The kind a provider's `type` names (`minecraft:uniform` or `uniform`), among `kinds` of the
/// registry `registry` (`int_provider_type`).
fn kind(kinds: &[(&str, &'static str)], registry: &str, j: &Json) -> Result<&'static str, String> {
    let t = j.get("type").ok_or_else(|| format!("Input does not contain a key [type]: MapLike[{j}]"))?;
    let t = t.as_str().ok_or_else(|| format!("Not a string: {t}"))?;
    let name = if t.contains(':') { t.to_owned() } else { format!("minecraft:{t}") };
    let path = name.strip_prefix("minecraft:");
    kinds
        .iter()
        .find(|(n, _)| path == Some(*n))
        .map(|(_, c)| *c)
        .ok_or_else(|| format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:{registry}]: {name}"))
}

/// What `Codec.either(Codec.INT or FLOAT, the kinds' dispatch)` says when neither reads `j`: the
/// number's refusal, then the kind's (`second`).
fn neither(j: &Json, second: String) -> String {
    format!("Failed to parse either. First: Not a number: {j}; Second: {second}")
}

fn number(j: &Json, key: &str) -> Result<f64, String> {
    let v = j.get(key).ok_or_else(|| format!("No key {key} in MapLike[{j}]"))?;
    v.as_f64().ok_or_else(|| format!("Not a number: {v}"))
}

/// `Codec.INT`'s reading of a JSON number: Java's `Number.intValue()` of it.
fn int_of(n: f64) -> i32 {
    n as i32
}

/// What `IntProvider.CODEC` reads (`int_provider`), or `IntProvider.codec(min, max)`
/// (`int_provider/<min>/<max>`); `FloatProvider`'s alike (`float_provider`...).
pub fn decode(rt: &mut Rt, name: &str, j: &Json) -> Result<V, String> {
    let mut parts = name.split('/');
    let base = parts.next().unwrap_or("");
    let bounds: Option<(f64, f64)> = match (parts.next(), parts.next()) {
        (Some(lo), Some(hi)) => Some((lo.parse().map_err(|_| format!("bad bound {lo}"))?, hi.parse().map_err(|_| format!("bad bound {hi}"))?)),
        _ => None,
    };
    let (p, min, max) = if base == "float_provider" {
        let p = decode_float(rt, j)?;
        (p, f64::from(float_min(rt, p)), f64::from(float_max(rt, p)))
    } else {
        let p = decode_int(rt, j)?;
        (p, f64::from(int_min(rt, p)), f64::from(int_max(rt, p)))
    };
    if let Some((lo, hi)) = bounds {
        let span = |rt: &Rt| text_bounds(rt, p, base == "float_provider");
        if min < lo {
            return Err(format!("Value provider too low: {} [{}]", bound_text(lo, base), span(rt)));
        }
        if max > hi {
            return Err(format!("Value provider too high: {} [{}]", bound_text(hi, base), span(rt)));
        }
    }
    Ok(p)
}

fn bound_text(b: f64, base: &str) -> String {
    if base == "float_provider" { ft(b as f32) } else { (b as i64).to_string() }
}

/// `getMinValue() + "-" + getMaxValue()`, as the validation errors spell them.
fn text_bounds(rt: &Rt, p: V, float: bool) -> String {
    if float {
        format!("{}-{}", ft(float_min(rt, p)), ft(float_max(rt, p)))
    } else {
        format!("{}-{}", int_min(rt, p), int_max(rt, p))
    }
}

fn decode_int(rt: &mut Rt, j: &Json) -> Result<V, String> {
    if let Some(n) = j.as_f64() {
        return Ok(constant_int(rt, int_of(n)));
    }
    if !j.is_object() {
        return Err(neither(j, format!("Not a JSON object: {j}")));
    }
    decode_int_kind(rt, j).map_err(|e| neither(j, e))
}

fn decode_int_kind(rt: &mut Rt, j: &Json) -> Result<V, String> {
    let class = kind(&INT_KINDS, "int_provider_type", j)?;
    let int = |key: &str| number(j, key).map(int_of);
    let range = |class: &str| -> Result<(i32, i32), String> {
        let (min, max) = (int("min_inclusive")?, int("max_inclusive")?);
        if max < min {
            return Err(if class == NORMAL_INT {
                format!("Max must be larger than min: [{min}, {max}]")
            } else {
                format!("Max must be at least min, min_inclusive: {min}, max_inclusive: {max}")
            });
        }
        Ok((min, max))
    };
    Ok(match class {
        CONSTANT_INT => constant_int(rt, int("value")?),
        UNIFORM_INT | BIASED_INT => {
            let (min, max) = range(class)?;
            make(rt, class, &[("minInclusive", V::I(min)), ("maxInclusive", V::I(max))])
        }
        CLAMPED_INT => {
            let source = decode_int(rt, j.get("source").ok_or_else(|| format!("No key source in MapLike[{j}]"))?)?;
            let (min, max) = range(class)?;
            make(rt, class, &[("source", source), ("minInclusive", V::I(min)), ("maxInclusive", V::I(max))])
        }
        NORMAL_INT => {
            let (mean, deviation) = (number(j, "mean")? as f32, number(j, "deviation")? as f32);
            let (min, max) = range(class)?;
            make(rt, class, &[("mean", V::F(mean)), ("deviation", V::F(deviation)), ("minInclusive", V::I(min)), ("maxInclusive", V::I(max))])
        }
        _ => {
            let list = j.get("distribution").ok_or_else(|| format!("No key distribution in MapLike[{j}]"))?;
            let list = list.as_array().ok_or_else(|| format!("Not a list: {list}"))?;
            if list.is_empty() {
                return Err("List must have contents".into());
            }
            let mut entries = Vec::with_capacity(list.len());
            for e in list {
                let data = decode_int(rt, e.get("data").ok_or_else(|| format!("No key data in MapLike[{e}]"))?)?;
                entries.push((data, int_of(number(e, "weight")?)));
            }
            weighted_int(rt, entries)
        }
    })
}

fn decode_float(rt: &mut Rt, j: &Json) -> Result<V, String> {
    if let Some(n) = j.as_f64() {
        return Ok(constant_float(rt, n as f32));
    }
    if !j.is_object() {
        return Err(neither(j, format!("Not a JSON object: {j}")));
    }
    decode_float_kind(rt, j).map_err(|e| neither(j, e))
}

fn decode_float_kind(rt: &mut Rt, j: &Json) -> Result<V, String> {
    let class = kind(&FLOAT_KINDS, "float_provider_type", j)?;
    let float = |key: &str| number(j, key).map(|n| n as f32);
    let larger = |min: f32, max: f32| format!("Max must be larger than min: [{}, {}]", ft(min), ft(max));
    Ok(match class {
        CONSTANT_FLOAT => constant_float(rt, float("value")?),
        UNIFORM_FLOAT => {
            let (min, max) = (float("min_inclusive")?, float("max_exclusive")?);
            if max <= min {
                return Err(format!("Max must be larger than min, min_inclusive: {}, max_exclusive: {}", ft(min), ft(max)));
            }
            make(rt, class, &[("minInclusive", V::F(min)), ("maxExclusive", V::F(max))])
        }
        NORMAL_FLOAT => {
            let (mean, deviation, min, max) = (float("mean")?, float("deviation")?, float("min")?, float("max")?);
            if max < min {
                return Err(larger(min, max));
            }
            make(rt, class, &[("mean", V::F(mean)), ("deviation", V::F(deviation)), ("min", V::F(min)), ("max", V::F(max))])
        }
        _ => {
            let (min, max, plateau) = (float("min")?, float("max")?, float("plateau")?);
            if max < min {
                return Err(larger(min, max));
            }
            if plateau > max - min {
                return Err(format!(
                    "Plateau can at most be the full span: [{}, {}]",
                    ft(min),
                    ft(max)
                ));
            }
            make(rt, class, &[("min", V::F(min)), ("max", V::F(max)), ("plateau", V::F(plateau))])
        }
    })
}

/// What `IntProvider.CODEC` or `FloatProvider.CODEC` writes: a constant as its number, another
/// kind as a map with its `type`.
pub fn write(rt: &Rt, v: V) -> Result<Nbt, String> {
    let class = rt.class_of(v);
    let typed = |kind: &str, fields: Vec<(&str, Nbt)>| {
        let mut m = BTreeMap::from([("type".to_owned(), Nbt::String(format!("minecraft:{kind}")))]);
        m.extend(fields.into_iter().map(|(k, v)| (k.to_owned(), v)));
        Nbt::Compound(m)
    };
    let i = |f: &'static str| Nbt::Int(int_field(rt, v, f));
    let f = |f: &'static str| Nbt::Float(float_field(rt, v, f));
    Ok(match class {
        CONSTANT_INT => i("value"),
        UNIFORM_INT => typed("uniform", vec![("min_inclusive", i("minInclusive")), ("max_inclusive", i("maxInclusive"))]),
        BIASED_INT => typed("biased_to_bottom", vec![("min_inclusive", i("minInclusive")), ("max_inclusive", i("maxInclusive"))]),
        CLAMPED_INT => {
            let source = write(rt, rt.get_field(v, "source"))?;
            typed("clamped", vec![("source", source), ("min_inclusive", i("minInclusive")), ("max_inclusive", i("maxInclusive"))])
        }
        NORMAL_INT => typed(
            "clamped_normal",
            vec![("mean", f("mean")), ("deviation", f("deviation")), ("min_inclusive", i("minInclusive")), ("max_inclusive", i("maxInclusive"))],
        ),
        WEIGHTED_INT => {
            let mut list = Vec::new();
            for e in crate::jutil::elements(rt, rt.get_field(v, "$entries")) {
                let data = write(rt, rt.get_field(e, "data"))?;
                let m = BTreeMap::from([("data".to_owned(), data), ("weight".to_owned(), Nbt::Int(int_field(rt, e, "$weight")))]);
                list.push(Nbt::Compound(m));
            }
            typed("weighted_list", vec![("distribution", Nbt::List(list))])
        }
        CONSTANT_FLOAT => f("value"),
        UNIFORM_FLOAT => typed("uniform", vec![("min_inclusive", f("minInclusive")), ("max_exclusive", f("maxExclusive"))]),
        NORMAL_FLOAT => typed("clamped_normal", vec![("mean", f("mean")), ("deviation", f("deviation")), ("min", f("min")), ("max", f("max"))]),
        TRAPEZOID_FLOAT => typed("trapezoid", vec![("min", f("min")), ("max", f("max")), ("plateau", f("plateau"))]),
        c => return Err(format!("Not a value provider: {c}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, HostRandom};

    /// `LegacyRandomSource`: `java.util.Random`'s stream, with `MarsagliaPolarGaussian`'s cached
    /// second value (an oracle apart from the runtime).
    struct Legacy(u64, Option<f64>);
    impl Legacy {
        fn new(seed: i64) -> Self {
            Self((seed as u64 ^ 0x5_DEEC_E66D) & ((1 << 48) - 1), None)
        }
        fn next(&mut self, bits: u32) -> i32 {
            self.0 = (self.0.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
            (self.0 >> (48 - bits)) as i32
        }
    }
    impl HostRandom for Legacy {
        fn next_int(&mut self) -> i32 {
            self.next(32)
        }
        fn next_int_bound(&mut self, bound: i32) -> i32 {
            if bound & -bound == bound {
                return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
            }
            loop {
                let bits = self.next(31);
                let v = bits % bound;
                if bits - v + (bound - 1) >= 0 {
                    return v;
                }
            }
        }
        fn next_long(&mut self) -> i64 {
            (i64::from(self.next(32)) << 32).wrapping_add(i64::from(self.next(32)))
        }
        fn next_boolean(&mut self) -> bool {
            self.next(1) != 0
        }
        fn next_float(&mut self) -> f32 {
            self.next(24) as f32 / (1 << 24) as f32
        }
        fn next_double(&mut self) -> f64 {
            ((i64::from(self.next(26)) << 27) + i64::from(self.next(27))) as f64 * (1.0 / (1u64 << 53) as f64)
        }
        fn next_gaussian(&mut self) -> f64 {
            if let Some(g) = self.1.take() {
                return g;
            }
            loop {
                let x = 2.0 * self.next_double() - 1.0;
                let y = 2.0 * self.next_double() - 1.0;
                let r = x * x + y * y;
                if r < 1.0 && r != 0.0 {
                    let m = (-2.0 * r.ln() / r).sqrt();
                    self.1 = Some(y * m);
                    return x * m;
                }
            }
        }
    }

    struct Region(Legacy);
    impl Host for Region {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:stone".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn random(&mut self) -> Option<&mut dyn HostRandom> {
            Some(&mut self.0)
        }
    }

    const VP: &str = "net/minecraft/util/valueproviders/";
    const SEEDS: [i64; 3] = [0, 42, -7];

    fn of(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        let v = crate::api::call(rt, key, args);
        assert!(rt.thrown.is_none(), "{key} threw");
        v
    }

    /// Eight samples from a fresh `LegacyRandomSource(seed)`, then the stream's next long.
    fn ints(rt: &mut Rt, p: V, seed: i64) -> (Vec<i32>, i64) {
        let mut region = Region(Legacy::new(seed));
        let samples = rt.with_host(&mut region, |rt| {
            let random = rt.new_object(crate::worldgen::WORLDGEN_RANDOM);
            (0..8).map(|_| crate::api::call(rt, "net/minecraft/util/valueproviders/IntProvider.sample.(Lnet/minecraft/util/RandomSource;)I", &[p, random]).i()).collect()
        });
        (samples, region.0.next_long())
    }

    /// Six samples' bits, then the stream's next long.
    fn floats(rt: &mut Rt, p: V, seed: i64) -> (Vec<u32>, i64) {
        let mut region = Region(Legacy::new(seed));
        let samples = rt.with_host(&mut region, |rt| {
            let random = rt.new_object(crate::worldgen::WORLDGEN_RANDOM);
            (0..6)
                .map(|_| crate::api::call(rt, "net/minecraft/util/valueproviders/FloatProvider.sample.(Lnet/minecraft/util/RandomSource;)F", &[p, random]).f().to_bits())
                .collect()
        });
        (samples, region.0.next_long())
    }

    /// `provider.toString()`, called on the provider's base type as a mod's code calls it.
    fn text_of(rt: &mut Rt, p: V) -> String {
        let key = if is_float(rt.class_of(p)) {
            "net/minecraft/util/valueproviders/FloatProvider.toString.()Ljava/lang/String;"
        } else {
            "net/minecraft/util/valueproviders/IntProvider.toString.()Ljava/lang/String;"
        };
        let s = crate::api::call(rt, key, &[p]);
        rt.str_of(s).unwrap_or_default().to_owned()
    }

    fn bounds(rt: &mut Rt, p: V, float: bool) -> (f64, f64) {
        let owner = if float { "FloatProvider" } else { "IntProvider" };
        let ty = if float { "F" } else { "I" };
        let key = |m: &str| -> &'static str { Box::leak(format!("{VP}{owner}.{m}.(){ty}").into_boxed_str()) };
        let (lo, hi) = (crate::api::call(rt, key("getMinValue"), &[p]), crate::api::call(rt, key("getMaxValue"), &[p]));
        if float { (f64::from(lo.f()), f64::from(hi.f())) } else { (f64::from(lo.i()), f64::from(hi.i())) }
    }

    /// Vanilla 1.21.1's samples (`scratchpad/oracle-vp/ValueProviderOracle.java` on the Mojmapped
    /// 1.21.1 classes): for each seed, the draws and the stream's next long after them.
    #[allow(clippy::type_complexity)]
    const INT_ORACLE: [(&str, [([i32; 8], i64); 3]); 4] = [
        ("[2-5]", [([4, 5, 2, 4, 4, 3, 4, 2], -7423979211207825555), ([4, 2, 4, 2, 3, 5, 3, 4], -6169532649852302182), ([3, 5, 4, 5, 4, 2, 5, 3], 1581709715777812225)]),
        ("[1-8]", [([5, 2, 6, 2, 5, 3, 3, 8], -2228689144322150137), ([4, 3, 2, 3, 6, 4, 1, 3], 8552898714322622292), ([1, 4, 3, 5, 1, 3, 8, 2], -7620213400735635840)]),
        ("clamped", [([0, 6, 0, 0, 6, 6, 0, 0], -7423979211207825555), ([5, 6, 0, 6, 5, 5, 0, 6], -6169532649852302182), ([6, 0, 6, 6, 4, 4, 5, 6], 1581709715777812225)]),
        ("normal(3.0, 2.5) in [0-6]", [([5, 0, 6, 4, 5, 0, 2, 3], -2228689144322150137), ([5, 5, 0, 0, 3, 4, 0, 0], 8552898714322622292), ([0, 5, 3, 5, 0, 2, 6, 6], 6544672467660488722)]),
    ];

    /// A mod's providers made in code sample as vanilla's: the same draws in the same order, so the
    /// generator's stream goes on where vanilla's would.
    #[test]
    fn int_providers_draw_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let uniform = of(&mut rt, "net/minecraft/util/valueproviders/UniformInt.of.(II)Lnet/minecraft/util/valueproviders/UniformInt;", &[V::I(2), V::I(5)]);
        let biased = of(&mut rt, "net/minecraft/util/valueproviders/BiasedToBottomInt.of.(II)Lnet/minecraft/util/valueproviders/BiasedToBottomInt;", &[V::I(1), V::I(8)]);
        let wide = of(&mut rt, "net/minecraft/util/valueproviders/UniformInt.of.(II)Lnet/minecraft/util/valueproviders/UniformInt;", &[V::I(-4), V::I(12)]);
        let clamped = of(
            &mut rt,
            "net/minecraft/util/valueproviders/ClampedInt.of.(Lnet/minecraft/util/valueproviders/IntProvider;II)Lnet/minecraft/util/valueproviders/ClampedInt;",
            &[wide, V::I(0), V::I(6)],
        );
        let normal = of(
            &mut rt,
            "net/minecraft/util/valueproviders/ClampedNormalInt.of.(FFII)Lnet/minecraft/util/valueproviders/ClampedNormalInt;",
            &[V::F(3.0), V::F(2.5), V::I(0), V::I(6)],
        );
        for (p, (name, runs)) in [uniform, biased, clamped, normal].into_iter().zip(INT_ORACLE) {
            for (seed, (samples, next)) in SEEDS.into_iter().zip(runs) {
                assert_eq!(ints(&mut rt, p, seed), (samples.to_vec(), next), "{name}, seed {seed}");
            }
        }
        assert_eq!(bounds(&mut rt, clamped, false), (0.0, 6.0));
        assert_eq!(bounds(&mut rt, uniform, false), (2.0, 5.0));
        assert_eq!(text_of(&mut rt, uniform), "[2-5]");
        assert_eq!(text_of(&mut rt, normal), "normal(3.0, 2.5) in [0-6]");
        assert!(text_of(&mut rt, clamped).starts_with("net.minecraft.util.valueproviders.ClampedInt@"), "ClampedInt keeps Object's toString");
        // A constant draws nothing: the stream's next long is a fresh one's first.
        let seven = of(&mut rt, "net/minecraft/util/valueproviders/ConstantInt.of.(I)Lnet/minecraft/util/valueproviders/ConstantInt;", &[V::I(7)]);
        assert_eq!(ints(&mut rt, seven, 0), (vec![7; 8], -4962768465676381896));
        assert_eq!(text_of(&mut rt, seven), "7");
        let zero = of(&mut rt, "net/minecraft/util/valueproviders/ConstantInt.of.(I)Lnet/minecraft/util/valueproviders/ConstantInt;", &[V::I(0)]);
        assert_eq!(zero, crate::api::get_static(&mut rt, "net/minecraft/util/valueproviders/ConstantInt.ZERO"), "of(0) is ZERO");
    }

    #[allow(clippy::type_complexity)]
    const FLOAT_ORACLE: [(&str, [([u32; 6], i64); 3]); 3] = [
        (
            "[0.25-1.75]",
            [
                ([1068259463, 1069523709, 1058823600, 1066691348, 1067082327, 1060547816], -8292973307042192125),
                ([1068216630, 1051327390, 1067658700, 1050988864, 1060539482, 1070915788], 5111195811822994797),
                ([1059542975, 1070405224, 1067532340, 1070963250, 1065623618, 1055945851], -2732364192705608872),
            ],
        ),
        (
            "normal(1.0, 0.5) in [0.0-2.0]",
            [
                ([1068719284, 1057790500, 1073741824, 1068556703, 1069482821, 1042421704], 7105486291024734541),
                ([1070142714, 1069209492, 1057385157, 1055169608, 1066531722, 1068224732], 6802844026563419272),
                ([1053843606, 1069084618, 1065655502, 1070302942, 1058198464, 1064666501], -1849135647554627033),
            ],
        ),
        (
            "trapezoid(1.0) in [0.0-4.0]",
            [
                ([1078248943, 1069638610, 1073981400, 1069980392, 1076535922, 1067133364], 7105486291024734541),
                ([1072910572, 1071896058, 1074517388, 1071680424, 1072071309, 1077664824], 6802844026563419272),
                ([1073846894, 1078362777, 1069743501, 1076271184, 1057158424, 1073031897], -1849135647554627033),
            ],
        ),
    ];

    #[test]
    fn float_providers_draw_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let uniform = of(&mut rt, "net/minecraft/util/valueproviders/UniformFloat.of.(FF)Lnet/minecraft/util/valueproviders/UniformFloat;", &[V::F(0.25), V::F(1.75)]);
        let normal = of(
            &mut rt,
            "net/minecraft/util/valueproviders/ClampedNormalFloat.of.(FFFF)Lnet/minecraft/util/valueproviders/ClampedNormalFloat;",
            &[V::F(1.0), V::F(0.5), V::F(0.0), V::F(2.0)],
        );
        let trapezoid = of(&mut rt, "net/minecraft/util/valueproviders/TrapezoidFloat.of.(FFF)Lnet/minecraft/util/valueproviders/TrapezoidFloat;", &[V::F(0.0), V::F(4.0), V::F(1.0)]);
        for (p, (name, runs)) in [uniform, normal, trapezoid].into_iter().zip(FLOAT_ORACLE) {
            assert_eq!(text_of(&mut rt, p), name);
            for (seed, (samples, next)) in SEEDS.into_iter().zip(runs) {
                assert_eq!(floats(&mut rt, p, seed), (samples.to_vec(), next), "{name}, seed {seed}");
            }
        }
        assert_eq!(bounds(&mut rt, uniform, true), (0.25, 1.75));
        let half = of(&mut rt, "net/minecraft/util/valueproviders/ConstantFloat.of.(F)Lnet/minecraft/util/valueproviders/ConstantFloat;", &[V::F(0.5)]);
        assert_eq!(floats(&mut rt, half, 0), (vec![0.5f32.to_bits(); 6], -4962768465676381896));
        // `UniformFloat.of` refuses an empty range; vanilla's int kinds take any.
        crate::api::call(&mut rt, "net/minecraft/util/valueproviders/UniformFloat.of.(FF)Lnet/minecraft/util/valueproviders/UniformFloat;", &[V::F(1.0), V::F(1.0)]);
        let thrown = rt.thrown.take().expect("UniformFloat.of(1, 1) throws");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");
        of(&mut rt, "net/minecraft/util/valueproviders/UniformInt.of.(II)Lnet/minecraft/util/valueproviders/UniformInt;", &[V::I(5), V::I(1)]);
    }

    fn codec(rt: &mut Rt, field: &str) -> V {
        let (owner, field) = field.split_once('.').unwrap();
        crate::codecs::get_static(rt, &format!("{VP}{owner}"), field).unwrap()
    }

    /// What `codec.parse` then `encodeStart` give: the provider written back, or the error's last
    /// refusal (the JSON a message quotes is spelled by each side's own printer).
    fn round(rt: &mut Rt, c: V, j: &str) -> Result<serde_json::Value, String> {
        let p = crate::codecs::decode(rt, c, &serde_json::from_str(j).unwrap())?;
        Ok(crate::recipes::nbt_json(&crate::codecs::encode(rt, c, p)?))
    }

    fn last(e: Result<serde_json::Value, String>) -> String {
        let e = e.expect_err("refused");
        e.rsplit("; Second: ").next().unwrap().to_owned()
    }

    /// `IntProvider.CODEC` and its ranged forms read and write as vanilla 1.21.1's, and refuse what
    /// it refuses with its messages (the oracle's).
    #[test]
    fn int_provider_codecs_read_and_write_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let c = codec(&mut rt, "IntProvider.CODEC");
        let json = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        assert_eq!(round(&mut rt, c, "5"), Ok(json("5")));
        assert_eq!(round(&mut rt, c, "3.7"), Ok(json("3")));
        assert_eq!(round(&mut rt, c, r#"{"type":"minecraft:constant","value":3}"#), Ok(json("3")));
        let uniform = r#"{"min_inclusive":2,"max_inclusive":5,"type":"minecraft:uniform"}"#;
        assert_eq!(round(&mut rt, c, r#"{"type":"minecraft:uniform","min_inclusive":2,"max_inclusive":5}"#), Ok(json(uniform)));
        let clamped = r#"{"source":{"min_inclusive":-4,"max_inclusive":12,"type":"minecraft:uniform"},"min_inclusive":0,"max_inclusive":6,"type":"minecraft:clamped"}"#;
        assert_eq!(round(&mut rt, c, clamped), Ok(json(clamped)));
        let normal = r#"{"mean":3.0,"deviation":2.5,"min_inclusive":0,"max_inclusive":6,"type":"minecraft:clamped_normal"}"#;
        assert_eq!(round(&mut rt, c, normal), Ok(json(normal)));
        let weighted = r#"{"distribution":[{"data":1,"weight":3},{"data":{"min_inclusive":10,"max_inclusive":20,"type":"minecraft:uniform"},"weight":1}],"type":"minecraft:weighted_list"}"#;
        assert_eq!(round(&mut rt, c, weighted), Ok(json(weighted)));
        // The weighted list samples as vanilla's: an entry by weight, then its own sample.
        let p = crate::codecs::decode(&mut rt, c, &json(weighted)).unwrap();
        assert_eq!(bounds(&mut rt, p, false), (1.0, 20.0));
        let runs: [([i32; 8], i64); 3] = [
            ([1, 18, 1, 1, 1, 1, 1, 1], -4029974628518736597),
            ([1, 1, 1, 1, 1, 11, 1, 1], 1684641590762760125),
            ([1, 14, 13, 1, 19, 1, 1, 1], -9041719857244631827),
        ];
        for (seed, (samples, next)) in SEEDS.into_iter().zip(runs) {
            assert_eq!(ints(&mut rt, p, seed), (samples.to_vec(), next), "weighted, seed {seed}");
        }
        // Refusals.
        assert_eq!(last(round(&mut rt, c, r#"{"type":"uniform","min_inclusive":6,"max_inclusive":5}"#)), "Max must be at least min, min_inclusive: 6, max_inclusive: 5");
        assert_eq!(
            last(round(&mut rt, c, r#"{"type":"minecraft:clamped_normal","mean":3.0,"deviation":2.5,"min_inclusive":6,"max_inclusive":0}"#)),
            "Max must be larger than min: [6, 0]"
        );
        assert_eq!(last(round(&mut rt, c, r#"{"type":"minecraft:weighted_list","distribution":[]}"#)), "List must have contents");
        assert_eq!(last(round(&mut rt, c, r#"{"type":"nope","value":1}"#)), "Unknown registry key in ResourceKey[minecraft:root / minecraft:int_provider_type]: minecraft:nope");
        assert_eq!(last(round(&mut rt, c, r#"{"type":"other:nope","value":1}"#)), "Unknown registry key in ResourceKey[minecraft:root / minecraft:int_provider_type]: other:nope");
        assert!(last(round(&mut rt, c, r#"{"value":1}"#)).starts_with("Input does not contain a key [type]: MapLike["));
        assert_eq!(last(round(&mut rt, c, r#""abc""#)), r#"Not a JSON object: "abc""#);
        let nested = round(&mut rt, c, r#"{"type":"minecraft:clamped","source":{"type":"minecraft:uniform","min_inclusive":9,"max_inclusive":1},"min_inclusive":0,"max_inclusive":6}"#);
        let nested = nested.expect_err("a bad source is refused");
        assert_eq!(nested.matches("Failed to parse either.").count(), 2, "{nested}");
        assert!(nested.ends_with("Second: Max must be at least min, min_inclusive: 9, max_inclusive: 1"), "{nested}");
        // `IntProvider.codec(0, 10)`, `NON_NEGATIVE_CODEC`, `POSITIVE_CODEC`.
        let ranged = crate::api::call(&mut rt, "net/minecraft/util/valueproviders/IntProvider.codec.(II)Lcom/mojang/serialization/Codec;", &[V::I(0), V::I(10)]);
        assert_eq!(round(&mut rt, ranged, "4"), Ok(json("4")));
        assert_eq!(round(&mut rt, ranged, "11"), Err("Value provider too high: 10 [11-11]".into()));
        assert_eq!(round(&mut rt, ranged, r#"{"type":"minecraft:uniform","min_inclusive":-1,"max_inclusive":5}"#), Err("Value provider too low: 0 [-1-5]".into()));
        assert_eq!(round(&mut rt, ranged, r#"{"type":"minecraft:uniform","min_inclusive":2,"max_inclusive":15}"#), Err("Value provider too high: 10 [2-15]".into()));
        let non_negative = codec(&mut rt, "IntProvider.NON_NEGATIVE_CODEC");
        assert_eq!(round(&mut rt, non_negative, "-3"), Err("Value provider too low: 0 [-3--3]".into()));
        let positive = codec(&mut rt, "IntProvider.POSITIVE_CODEC");
        assert_eq!(round(&mut rt, positive, "0"), Err("Value provider too low: 1 [0-0]".into()));
    }

    #[test]
    fn float_provider_codecs_read_and_write_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let c = codec(&mut rt, "FloatProvider.CODEC");
        let json = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        assert_eq!(round(&mut rt, c, "0.5"), Ok(json("0.5")));
        for kind in [
            r#"{"min_inclusive":0.25,"max_exclusive":1.75,"type":"minecraft:uniform"}"#,
            r#"{"mean":1.0,"deviation":0.5,"min":0.0,"max":2.0,"type":"minecraft:clamped_normal"}"#,
            r#"{"min":0.0,"max":4.0,"plateau":1.0,"type":"minecraft:trapezoid"}"#,
        ] {
            assert_eq!(round(&mut rt, c, kind), Ok(json(kind)));
        }
        assert_eq!(
            last(round(&mut rt, c, r#"{"type":"minecraft:uniform","min_inclusive":1.75,"max_exclusive":0.25}"#)),
            "Max must be larger than min, min_inclusive: 1.75, max_exclusive: 0.25"
        );
        assert_eq!(last(round(&mut rt, c, r#"{"type":"minecraft:trapezoid","min":0.0,"max":4.0,"plateau":5.0}"#)), "Plateau can at most be the full span: [0.0, 4.0]");
        assert_eq!(last(round(&mut rt, c, r#"{"type":"minecraft:trapezoid","min":4.0,"max":0.0,"plateau":1.0}"#)), "Max must be larger than min: [4.0, 0.0]");
        let ranged = crate::api::call(&mut rt, "net/minecraft/util/valueproviders/FloatProvider.codec.(FF)Lcom/mojang/serialization/Codec;", &[V::F(0.0), V::F(1.0)]);
        assert_eq!(round(&mut rt, ranged, "0.5"), Ok(json("0.5")));
        assert_eq!(round(&mut rt, ranged, "1.5"), Err("Value provider too high: 1.0 [1.5-1.5]".into()));
        assert_eq!(round(&mut rt, ranged, r#"{"type":"minecraft:uniform","min_inclusive":0.25,"max_exclusive":1.75}"#), Err("Value provider too high: 1.0 [0.25-1.75]".into()));
    }
}
