//! Runtime for mods the translator (`horizon-mod-translator`) turned into
//! Rust. Generated code works on [`V`] values and a [`Rt`] that owns the
//! mod's objects and static fields; calls into Minecraft's and NeoForge's
//! API go through [`api`], which maps them onto Horizon.
use std::collections::HashMap;

pub mod api;

/// A JVM value. References index [`Rt::heap`]; 0 is null.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum V {
    #[default]
    Void,
    I(i32),
    J(i64),
    F(f32),
    D(f64),
    R(u32),
}

impl V {
    pub fn i(self) -> i32 {
        match self {
            V::I(v) => v,
            _ => 0,
        }
    }
    pub fn j(self) -> i64 {
        match self {
            V::J(v) => v,
            _ => 0,
        }
    }
    pub fn f(self) -> f32 {
        match self {
            V::F(v) => v,
            _ => 0.0,
        }
    }
    pub fn d(self) -> f64 {
        match self {
            V::D(v) => v,
            _ => 0.0,
        }
    }
    pub fn r(self) -> u32 {
        match self {
            V::R(v) => v,
            _ => 0,
        }
    }
    pub const NULL: V = V::R(0);
}

/// What an object holds.
#[derive(Debug, Clone)]
pub enum Data {
    /// An instance of a mod class (or an API class the runtime models as fields).
    Fields(HashMap<&'static str, V>),
    Str(String),
    Array(Vec<V>),
    /// An API object Horizon owns, by kind and handle (a block state id, a position...).
    Native(&'static str, [i64; 4]),
}

#[derive(Debug, Clone)]
pub struct Object {
    pub class: &'static str,
    pub data: Data,
}

/// The translated mod's runtime state.
#[derive(Default)]
pub struct Rt {
    pub heap: Vec<Object>,
    pub statics: HashMap<&'static str, V>,
    /// Interned strings by value, so `ldc` gives the same reference twice.
    strings: HashMap<String, u32>,
    /// API methods the mod called that Horizon doesn't map yet, with counts.
    pub unmapped: HashMap<&'static str, u64>,
}

impl Rt {
    pub fn new() -> Rt {
        let mut rt = Rt::default();
        rt.heap.push(Object { class: "null", data: Data::Array(Vec::new()) });
        rt
    }

    pub fn alloc(&mut self, class: &'static str, data: Data) -> V {
        self.heap.push(Object { class, data });
        V::R(self.heap.len() as u32 - 1)
    }

    pub fn new_object(&mut self, class: &'static str) -> V {
        self.alloc(class, Data::Fields(HashMap::new()))
    }

    pub fn string(&mut self, s: &str) -> V {
        if let Some(&r) = self.strings.get(s) {
            return V::R(r);
        }
        let v = self.alloc("java/lang/String", Data::Str(s.to_owned()));
        self.strings.insert(s.to_owned(), v.r());
        v
    }

    pub fn str_of(&self, v: V) -> Option<&str> {
        match &self.heap.get(v.r() as usize)?.data {
            Data::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn class_of(&self, v: V) -> &'static str {
        self.heap.get(v.r() as usize).map_or("null", |o| o.class)
    }

    pub fn get_field(&self, obj: V, name: &'static str) -> V {
        match self.heap.get(obj.r() as usize).map(|o| &o.data) {
            Some(Data::Fields(f)) => f.get(name).copied().unwrap_or_default(),
            _ => V::Void,
        }
    }

    pub fn put_field(&mut self, obj: V, name: &'static str, v: V) {
        if let Some(Object { data: Data::Fields(f), .. }) = self.heap.get_mut(obj.r() as usize) {
            f.insert(name, v);
        }
    }

    pub fn new_array(&mut self, len: i32, zero: V) -> V {
        self.alloc("[", Data::Array(vec![zero; len.max(0) as usize]))
    }

    pub fn array_len(&self, a: V) -> i32 {
        match self.heap.get(a.r() as usize).map(|o| &o.data) {
            Some(Data::Array(v)) => v.len() as i32,
            _ => 0,
        }
    }

    pub fn array_get(&self, a: V, i: i32) -> V {
        match self.heap.get(a.r() as usize).map(|o| &o.data) {
            Some(Data::Array(v)) => v.get(i as usize).copied().unwrap_or_default(),
            _ => V::Void,
        }
    }

    pub fn array_set(&mut self, a: V, i: i32, x: V) {
        if let Some(Object { data: Data::Array(v), .. }) = self.heap.get_mut(a.r() as usize)
            && let Some(slot) = v.get_mut(i as usize)
        {
            *slot = x;
        }
    }
}

impl Rt {
    /// A lambda object: its class is the implementing method's key, its data the captured values.
    pub fn lambda(&mut self, implementation: &'static str, captured: Vec<V>) -> V {
        self.alloc(implementation, Data::Array(captured))
    }

    pub fn captured(&self, v: V) -> Vec<V> {
        match self.heap.get(v.r() as usize).map(|o| &o.data) {
            Some(Data::Array(c)) => c.clone(),
            _ => Vec::new(),
        }
    }

    /// `String.valueOf` of a value whose static kind is `kind` ('I', 'Z', 'C', 'J', 'F', 'D', 'L').
    pub fn to_java_string(&self, v: V, kind: char) -> String {
        match kind {
            'Z' => (v.i() != 0).to_string(),
            'C' => char::from_u32(v.i() as u32 & 0xffff).map_or_else(String::new, String::from),
            'J' => v.j().to_string(),
            'F' => java_double(v.f() as f64, true),
            'D' => java_double(v.d(), false),
            'L' => match self.str_of(v) {
                Some(s) => s.to_owned(),
                None if v.r() == 0 => "null".into(),
                None => match self.heap.get(v.r() as usize).map(|o| &o.data) {
                    Some(Data::Fields(f)) if f.contains_key("value") => f["value"].i().to_string(),
                    _ => format!("{}@{:x}", self.class_of(v).replace('/', "."), v.r()),
                },
            },
            _ => v.i().to_string(),
        }
    }

    /// `StringConcatFactory.makeConcatWithConstants`: `\u{1}` takes the next
    /// argument, `\u{2}` the next constant.
    pub fn concat(&mut self, recipe: &str, kinds: &str, args: &[V], constants: &[&str]) -> V {
        let mut out = String::new();
        let (mut a, mut c) = (0, 0);
        let kinds: Vec<char> = kinds.chars().collect();
        for ch in recipe.chars() {
            match ch {
                '\u{1}' => {
                    out.push_str(&self.to_java_string(args.get(a).copied().unwrap_or_default(), kinds.get(a).copied().unwrap_or('L')));
                    a += 1;
                }
                '\u{2}' => {
                    out.push_str(constants.get(c).copied().unwrap_or(""));
                    c += 1;
                }
                ch => out.push(ch),
            }
        }
        self.alloc("java/lang/String", Data::Str(out))
    }
}

/// `Double.toString` / `Float.toString`: shortest round-trip digits, as the
/// JDK prints them since 19, with Java's `1.0`, `1.0E10` and `NaN` forms.
pub fn java_double(d: f64, float: bool) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        let s = if float { format!("{}", d as f32) } else { format!("{d}") };
        return if s.contains('.') { s } else { format!("{s}.0") };
    }
    let s = if float { format!("{:e}", d as f32) } else { format!("{d:e}") };
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let m = if m.contains('.') { m.to_owned() } else { format!("{m}.0") };
    format!("{m}E{e}")
}

/// Java's integer division and remainder: no panic on zero (the JVM would
/// throw; translated code returns 0) and `MIN / -1` wraps.
pub fn idiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}
pub fn irem(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_rem(b) }
}
pub fn ldiv(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}
pub fn lrem(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a.wrapping_rem(b) }
}

/// `fcmpl`/`dcmpl` (`nan_is` -1) and `fcmpg`/`dcmpg` (+1).
pub fn fcmp(a: f64, b: f64, nan_is: i32) -> i32 {
    if a.is_nan() || b.is_nan() {
        nan_is
    } else if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

/// Java's float-to-int conversion: NaN is 0, out of range saturates (Rust's `as` does both).
pub fn d2i(d: f64) -> i32 {
    d as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_arithmetic_edges() {
        assert_eq!(idiv(i32::MIN, -1), i32::MIN);
        assert_eq!(irem(-7, 3), -1);
        assert_eq!(fcmp(f64::NAN, 1.0, -1), -1);
        assert_eq!(fcmp(f64::NAN, 1.0, 1), 1);
        assert_eq!(d2i(f64::NAN), 0);
        assert_eq!(d2i(1e20), i32::MAX);
    }

    #[test]
    fn strings_are_interned_and_fields_round_trip() {
        let mut rt = Rt::new();
        let a = rt.string("create");
        assert_eq!(a, rt.string("create"));
        let o = rt.new_object("com/example/Thing");
        rt.put_field(o, "speed", V::F(2.5));
        assert_eq!(rt.get_field(o, "speed"), V::F(2.5));
        assert_eq!(rt.get_field(V::NULL, "speed"), V::Void);
    }
}
