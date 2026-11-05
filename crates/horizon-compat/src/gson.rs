//! Gson's tree model, as mods read and write JSON: `JsonParser.parseReader`/`parseString`,
//! `JsonObject`, `JsonArray`, `JsonPrimitive` and `JsonNull`, Minecraft's `GsonHelper`, and a
//! `Gson` (from `new Gson()` or a `GsonBuilder`) reading and writing trees (`fromJson(reader,
//! JsonObject.class)`, `toJson(element)`). Of the pack's 367 jars, 122 use `JsonObject` and 103
//! `Gson`; mapping JSON onto a mod's own classes (`fromJson(reader, MyConfig.class)`) and the
//! streaming `JsonReader`/`JsonWriter` are not modelled here.
//!
//! A `JsonObject` is a map ([`Data::Table`]) of names to elements, in insertion order (Gson's
//! `LinkedTreeMap`); a `JsonArray` a list of elements; a `JsonPrimitive` keeps its kind (`$kind`:
//! string, number or boolean) and its value raw (a string, a number as Java holds it, a boolean as
//! an int); `JsonNull` is one object (`JsonNull.INSTANCE`). Objects and arrays are shared by
//! reference, so a member changed through a nested object changes it in its parent too.
use crate::{Data, Rt, Table, V};

pub(crate) const ELEMENT: &str = "com/google/gson/JsonElement";
pub(crate) const OBJECT: &str = "com/google/gson/JsonObject";
pub(crate) const ARRAY: &str = "com/google/gson/JsonArray";
pub(crate) const PRIMITIVE: &str = "com/google/gson/JsonPrimitive";
pub(crate) const NULL: &str = "com/google/gson/JsonNull";
const PARSER: &str = "com/google/gson/JsonParser";
pub(crate) const GSON: &str = "com/google/gson/Gson";
const BUILDER: &str = "com/google/gson/GsonBuilder";
const HELPER: &str = "net/minecraft/util/GsonHelper";
const SYNTAX: &str = "com/google/gson/JsonSyntaxException";
const PARSE_ERROR: &str = "com/google/gson/JsonParseException";
const CONSUMER: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";

/// What a `JsonPrimitive` holds (`$kind`).
const STRING: i32 = 0;
const NUMBER: i32 = 1;
const BOOLEAN: i32 = 2;

pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, ELEMENT | OBJECT | ARRAY | PRIMITIVE | NULL | PARSER | GSON | BUILDER | HELPER)
}

/// The classes of the tree's elements.
pub(crate) fn is_element_class(c: &str) -> bool {
    matches!(c, OBJECT | ARRAY | PRIMITIVE | NULL)
}

/// Gson's classes for `instanceof`: each element is a `JsonElement`, an array `Iterable`.
pub(crate) fn is_a(c: &str, class: &str) -> bool {
    match class {
        ELEMENT => is_element_class(c),
        "java/lang/Iterable" => c == ARRAY,
        _ => false,
    }
}

/// `JsonNull.INSTANCE`.
pub fn null(rt: &mut Rt) -> V {
    rt.intern("compat:json-null".into(), |rt| rt.new_object(NULL))
}

pub fn new_object(rt: &mut Rt) -> V {
    rt.alloc(OBJECT, Data::Table(Table::default()))
}

pub fn new_array(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc(ARRAY, Data::List(items))
}

fn primitive(rt: &mut Rt, kind: i32, value: V) -> V {
    let p = rt.new_object(PRIMITIVE);
    rt.put_field(p, "$kind", V::I(kind));
    rt.put_field(p, "value", value);
    p
}

/// A string primitive (its text not interned: parsed values are many and seldom repeat).
pub fn string(rt: &mut Rt, s: &str) -> V {
    let text = rt.alloc("java/lang/String", Data::Str(s.to_owned()));
    primitive(rt, STRING, text)
}

/// A number primitive of `n`, held as Java holds it (`I`, `J`, `F` or `D`).
pub fn number(rt: &mut Rt, n: V) -> V {
    primitive(rt, NUMBER, n)
}

pub fn boolean(rt: &mut Rt, b: bool) -> V {
    primitive(rt, BOOLEAN, V::I(b as i32))
}

fn kind(rt: &Rt, p: V) -> i32 {
    rt.get_field(p, "$kind").i()
}

fn members(rt: &Rt, o: V) -> Vec<(V, V)> {
    crate::jutil::table_pairs(rt, o)
}

fn items(rt: &Rt, a: V) -> Vec<V> {
    match rt.heap.get(a.r() as usize).map(|o| &o.data) {
        Some(Data::List(l)) => l.clone(),
        _ => Vec::new(),
    }
}

fn items_mut(rt: &mut Rt, a: V) -> Option<&mut Vec<V>> {
    match rt.heap.get_mut(a.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(l)) => Some(l),
        _ => None,
    }
}

/// A member of object `o` by name, or None.
pub fn get(rt: &mut Rt, o: V, name: &str) -> Option<V> {
    let k = rt.string(name);
    crate::jutil::table_get(rt, o, k)
}

/// Puts member `name` in object `o` (null: `JsonNull`).
pub fn put(rt: &mut Rt, o: V, name: V, v: V) {
    let v = if v.r() == 0 { null(rt) } else { v };
    crate::jutil::table_put(rt, o, name, v);
}

// ---- Parsing ----

/// Parses JSON text into the tree as Gson's reader does (objects keep their members' order, a
/// repeated name keeps the last value). `lenient`: Gson's lenient reader, which also takes
/// comments, single quotes, unquoted names and strings, `;` and `=`. `Ok(None)` for an empty
/// document; the flag says whether the value was all the text held.
pub fn parse(rt: &mut Rt, text: &str, lenient: bool) -> Result<Option<(V, bool)>, String> {
    let mut p = Parser { s: text.as_bytes(), at: 0, lenient };
    // A byte order mark is skipped, as Gson's reader skips it.
    if p.s.starts_with(&[0xef, 0xbb, 0xbf]) {
        p.at = 3;
    }
    p.skip_space()?;
    if p.at >= p.s.len() {
        return Ok(None);
    }
    let v = p.value(rt, 0)?;
    p.skip_space()?;
    Ok(Some((v, p.at >= p.s.len())))
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    lenient: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    /// `what`, with where in the text, as Gson's `MalformedJsonException` says it.
    fn error(&self, what: &str) -> String {
        let before = &self.s[..self.at.min(self.s.len())];
        let line = before.iter().filter(|c| **c == b'\n').count() + 1;
        let column = before.len() - before.iter().rposition(|c| *c == b'\n').map_or(0, |i| i + 1) + 1;
        format!("com.google.gson.stream.MalformedJsonException: {what} at line {line} column {column}")
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            self.at += 1;
            if c == b'\n' {
                break;
            }
        }
    }

    fn skip_space(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.at += 1,
                Some(b'/') if self.lenient => match self.s.get(self.at + 1) {
                    Some(b'/') => self.skip_line(),
                    Some(b'*') => match self.s[self.at + 2..].windows(2).position(|w| w == b"*/") {
                        Some(end) => self.at += 2 + end + 2,
                        None => return Err(self.error("Unterminated comment")),
                    },
                    _ => return Ok(()),
                },
                Some(b'#') if self.lenient => self.skip_line(),
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self, rt: &mut Rt, depth: usize) -> Result<V, String> {
        if depth > 512 {
            return Err(self.error("Nesting limit 512 reached"));
        }
        self.skip_space()?;
        match self.peek() {
            None => Err(self.error("End of input")),
            Some(b'{') => {
                self.at += 1;
                self.object(rt, depth)
            }
            Some(b'[') => {
                self.at += 1;
                self.array(rt, depth)
            }
            Some(q @ (b'"' | b'\'')) if q == b'"' || self.lenient => {
                self.at += 1;
                let s = self.quoted(q)?;
                Ok(string(rt, &s))
            }
            Some(_) => self.literal(rt),
        }
    }

    fn object(&mut self, rt: &mut Rt, depth: usize) -> Result<V, String> {
        let o = new_object(rt);
        self.skip_space()?;
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(o);
        }
        loop {
            self.skip_space()?;
            let name = match self.peek() {
                Some(q @ (b'"' | b'\'')) if q == b'"' || self.lenient => {
                    self.at += 1;
                    self.quoted(q)?
                }
                Some(c) if self.lenient && !delimiter(c) => self.unquoted(),
                _ => return Err(self.error("Expected name")),
            };
            self.skip_space()?;
            match self.peek() {
                Some(b':') => self.at += 1,
                Some(b'=') if self.lenient => {
                    self.at += 1;
                    if self.peek() == Some(b'>') {
                        self.at += 1;
                    }
                }
                _ => return Err(self.error("Expected ':'")),
            }
            let v = self.value(rt, depth + 1)?;
            let k = rt.string(&name);
            crate::jutil::table_put(rt, o, k, v);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b';') if self.lenient => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(o);
                }
                _ => return Err(self.error("Unterminated object")),
            }
        }
    }

    fn array(&mut self, rt: &mut Rt, depth: usize) -> Result<V, String> {
        let mut items = Vec::new();
        self.skip_space()?;
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(new_array(rt, items));
        }
        loop {
            items.push(self.value(rt, depth + 1)?);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b';') if self.lenient => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(new_array(rt, items));
                }
                _ => return Err(self.error("Unterminated array")),
            }
            // Lenient: a comma before the end stands for a null.
            self.skip_space()?;
            if self.lenient && self.peek() == Some(b']') {
                self.at += 1;
                items.push(null(rt));
                return Ok(new_array(rt, items));
            }
        }
    }

    /// A quoted string's text, after its opening quote.
    fn quoted(&mut self, quote: u8) -> Result<String, String> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return Err(self.error("Unterminated string")) };
            self.at += 1;
            match c {
                c if c == quote => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let Some(e) = self.peek() else { return Err(self.error("Unterminated escape sequence")) };
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let unit = self.hex4()?;
                            let c = if (0xd800..0xdc00).contains(&unit) && self.s.get(self.at..self.at + 2) == Some(b"\\u") {
                                let at = self.at;
                                self.at += 2;
                                let low = self.hex4()?;
                                if (0xdc00..0xe000).contains(&low) {
                                    char::from_u32(0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00))
                                } else {
                                    self.at = at;
                                    None
                                }
                            } else {
                                char::from_u32(unit)
                            };
                            let mut buf = [0; 4];
                            out.extend_from_slice(c.unwrap_or('\u{fffd}').encode_utf8(&mut buf).as_bytes());
                        }
                        // `\"`, `\\`, `\/`, `\'` and, as Gson takes them, any other character.
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.s.get(self.at..self.at + 4).and_then(|d| std::str::from_utf8(d).ok()).and_then(|d| u32::from_str_radix(d, 16).ok());
        match digits {
            Some(u) => {
                self.at += 4;
                Ok(u)
            }
            None => Err(self.error("Malformed Unicode escape")),
        }
    }

    /// A lenient name or string without quotes: up to the next delimiter.
    fn unquoted(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(|c| !delimiter(c)) {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.at]).into_owned()
    }

    /// `true`, `false`, `null` or a number; lenient, any other word is a string.
    fn literal(&mut self, rt: &mut Rt) -> Result<V, String> {
        let start = self.at;
        let word = self.unquoted();
        if word.is_empty() {
            return Err(self.error("Expected value"));
        }
        match word.as_str() {
            "true" | "TRUE" => return Ok(boolean(rt, true)),
            "false" | "FALSE" => return Ok(boolean(rt, false)),
            "null" | "NULL" => return Ok(null(rt)),
            _ => {}
        }
        if let Some(n) = json_number(&word) {
            return Ok(number(rt, n));
        }
        if self.lenient {
            return Ok(string(rt, &word));
        }
        self.at = start;
        Err(self.error("Use JsonReader.setLenient(true) to accept malformed JSON"))
    }
}

/// What ends a word without quotes, as Gson's reader takes them.
fn delimiter(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' | b'[' | b']' | b'{' | b'}' | b'/' | b'\\' | b';' | b'#' | b'=' | b'"' | b'\'')
}

/// A JSON number (`-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?`): an integer that fits a `long` as one,
/// the rest as a double.
fn json_number(w: &str) -> Option<V> {
    let b = w.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let digits = |i: &mut usize| {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let int_start = i;
    let n = digits(&mut i);
    if n == 0 || (n > 1 && b[int_start] == b'0') {
        return None;
    }
    let mut integral = true;
    if b.get(i) == Some(&b'.') {
        i += 1;
        if digits(&mut i) == 0 {
            return None;
        }
        integral = false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return None;
        }
        integral = false;
    }
    if i != b.len() {
        return None;
    }
    if integral && let Ok(l) = w.parse::<i64>() {
        return Some(V::J(l));
    }
    w.parse::<f64>().ok().map(V::D)
}

// ---- Writing ----

/// How a tree is written: Gson's pretty printing (two spaces, `": "`), its HTML-safe escaping
/// (`<`, `>`, `&`, `=`, `'` as `<`...), and whether an object's null members are written.
#[derive(Clone, Copy)]
pub struct Style {
    pub pretty: bool,
    pub html: bool,
    pub nulls: bool,
    /// Members by name, sorted (`GsonHelper.toStableString`).
    pub sorted: bool,
}

/// `JsonElement.toString()`: compact, nulls written, nothing escaped for HTML.
pub const TO_STRING: Style = Style { pretty: false, html: false, nulls: true, sorted: false };

pub fn write(rt: &Rt, v: V, style: Style) -> String {
    let mut out = String::new();
    write_into(rt, v, style, 0, &mut out);
    out
}

fn newline(out: &mut String, style: Style, depth: usize) {
    if style.pretty {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    }
}

fn write_into(rt: &Rt, v: V, style: Style, depth: usize, out: &mut String) {
    match rt.class_of(v) {
        OBJECT => {
            out.push('{');
            let mut all = members(rt, v);
            if style.sorted {
                all.sort_by(|a, b| rt.str_of(a.0).cmp(&rt.str_of(b.0)));
            }
            let mut first = true;
            for (k, m) in all {
                if !style.nulls && rt.class_of(m) == NULL {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                newline(out, style, depth + 1);
                quote(rt.str_of(k).unwrap_or(""), style.html, out);
                out.push_str(if style.pretty { ": " } else { ":" });
                write_into(rt, m, style, depth + 1, out);
            }
            if !first {
                newline(out, style, depth);
            }
            out.push('}');
        }
        ARRAY => {
            out.push('[');
            let all = items(rt, v);
            for (i, e) in all.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, style, depth + 1);
                write_into(rt, *e, style, depth + 1, out);
            }
            if !all.is_empty() {
                newline(out, style, depth);
            }
            out.push(']');
        }
        PRIMITIVE => match kind(rt, v) {
            STRING => quote(rt.str_of(rt.get_field(v, "value")).unwrap_or(""), style.html, out),
            NUMBER => out.push_str(&number_text(rt.get_field(v, "value"))),
            _ => out.push_str(if rt.get_field(v, "value").i() != 0 { "true" } else { "false" }),
        },
        _ => out.push_str("null"),
    }
}

/// A string as Gson's `JsonWriter` writes it.
fn quote(s: &str, html: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            '<' | '>' | '&' | '=' | '\'' if html => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Java's `toString` of a number as a primitive holds it.
fn number_text(n: V) -> String {
    match n {
        V::I(i) => i.to_string(),
        V::J(j) => j.to_string(),
        V::F(f) => crate::java_double(f64::from(f), true),
        V::D(d) => crate::java_double(d, false),
        _ => "0".into(),
    }
}

/// The tree as serde's JSON (codecs read it so: `codec.parse(JsonOps.INSTANCE, element)`).
pub fn to_serde(rt: &Rt, v: V) -> serde_json::Value {
    use serde_json::Value as J;
    match rt.class_of(v) {
        OBJECT => J::Object(members(rt, v).into_iter().map(|(k, m)| (rt.str_of(k).unwrap_or("").to_owned(), to_serde(rt, m))).collect()),
        ARRAY => J::Array(items(rt, v).into_iter().map(|e| to_serde(rt, e)).collect()),
        PRIMITIVE => match (kind(rt, v), rt.get_field(v, "value")) {
            (STRING, s) => J::String(rt.str_of(s).unwrap_or("").to_owned()),
            (NUMBER, V::I(i)) => J::from(i),
            (NUMBER, V::J(j)) => J::from(j),
            (NUMBER, V::F(f)) => serde_json::Number::from_f64(f64::from(f)).map_or(J::Null, J::Number),
            (NUMBER, V::D(d)) => serde_json::Number::from_f64(d).map_or(J::Null, J::Number),
            (_, b) => J::Bool(b.i() != 0),
        },
        _ => J::Null,
    }
}

/// serde's JSON as the tree.
pub fn from_serde(rt: &mut Rt, j: &serde_json::Value) -> V {
    use serde_json::Value as J;
    match j {
        J::Null => null(rt),
        J::Bool(b) => boolean(rt, *b),
        J::Number(n) => number(rt, n.as_i64().map_or_else(|| V::D(n.as_f64().unwrap_or(0.0)), V::J)),
        J::String(s) => string(rt, s),
        J::Array(a) => {
            let items = a.iter().map(|e| from_serde(rt, e)).collect();
            new_array(rt, items)
        }
        J::Object(m) => {
            let o = new_object(rt);
            for (k, v) in m {
                let (k, v) = (rt.string(k), from_serde(rt, v));
                crate::jutil::table_put(rt, o, k, v);
            }
            o
        }
    }
}

// ---- Equality ----

fn is_integral(n: V) -> bool {
    matches!(n, V::I(_) | V::J(_))
}

fn as_f64(n: V) -> f64 {
    match n {
        V::I(i) => f64::from(i),
        V::J(j) => j as f64,
        V::F(f) => f64::from(f),
        V::D(d) => d,
        _ => 0.0,
    }
}

fn as_i64(n: V) -> i64 {
    match n {
        V::I(i) => i64::from(i),
        V::J(j) => j,
        V::F(f) => f as i64,
        V::D(d) => d as i64,
        _ => 0,
    }
}

/// Gson's `equals` of two elements: objects by their members (in any order), arrays element by
/// element, numbers by value (integers as longs, the rest as doubles), the rest by value.
pub fn same(rt: &Rt, a: V, b: V) -> bool {
    if a == b {
        return true;
    }
    match (rt.class_of(a), rt.class_of(b)) {
        (NULL, NULL) => true,
        (OBJECT, OBJECT) => {
            let (x, y) = (members(rt, a), members(rt, b));
            x.len() == y.len() && x.iter().all(|(k, v)| y.iter().any(|(k2, v2)| rt.str_of(*k) == rt.str_of(*k2) && same(rt, *v, *v2)))
        }
        (ARRAY, ARRAY) => {
            let (x, y) = (items(rt, a), items(rt, b));
            x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| same(rt, *p, *q))
        }
        (PRIMITIVE, PRIMITIVE) => {
            let (va, vb) = (rt.get_field(a, "value"), rt.get_field(b, "value"));
            match (kind(rt, a), kind(rt, b)) {
                (NUMBER, NUMBER) if is_integral(va) && is_integral(vb) => as_i64(va) == as_i64(vb),
                (NUMBER, NUMBER) => {
                    let (x, y) = (as_f64(va), as_f64(vb));
                    x == y || (x.is_nan() && y.is_nan())
                }
                (STRING, STRING) => rt.str_of(va) == rt.str_of(vb),
                (BOOLEAN, BOOLEAN) => va.i() == vb.i(),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Gson's `hashCode` of an element, consistent with [`same`].
pub fn hash_of(rt: &Rt, v: V) -> i32 {
    match rt.class_of(v) {
        OBJECT => members(rt, v).iter().fold(0i32, |h, (k, m)| h.wrapping_add(rt.java_hash(*k) ^ hash_of(rt, *m))),
        ARRAY => items(rt, v).iter().fold(1i32, |h, e| h.wrapping_mul(31).wrapping_add(hash_of(rt, *e))),
        PRIMITIVE => {
            let value = rt.get_field(v, "value");
            match kind(rt, v) {
                NUMBER if is_integral(value) => {
                    let l = as_i64(value);
                    (l ^ (l >> 32)) as i32
                }
                NUMBER => {
                    let l = as_f64(value).to_bits() as i64;
                    (l ^ (l >> 32)) as i32
                }
                BOOLEAN => {
                    if value.i() != 0 {
                        1231
                    } else {
                        1237
                    }
                }
                _ => rt.java_hash(value),
            }
        }
        _ => 0,
    }
}

/// A deep copy (`deepCopy`): primitives and null are immutable, so themselves.
fn deep_copy(rt: &mut Rt, v: V) -> V {
    match rt.class_of(v) {
        OBJECT => {
            let o = new_object(rt);
            for (k, m) in members(rt, v) {
                let m = deep_copy(rt, m);
                crate::jutil::table_put(rt, o, k, m);
            }
            o
        }
        ARRAY => {
            let all: Vec<V> = items(rt, v).into_iter().map(|e| deep_copy(rt, e)).collect();
            new_array(rt, all)
        }
        _ => v,
    }
}

// ---- The calls ----

fn throw(rt: &mut Rt, class: &'static str, msg: &str) -> V {
    crate::jlang::throw_with(rt, class, msg)
}

/// Java's name of an element's class (`com.google.gson.JsonObject`).
fn java_name(rt: &Rt, v: V) -> String {
    rt.class_of(v).replace('/', ".")
}

fn simple_name(rt: &Rt, v: V) -> &'static str {
    rt.class_of(v).rsplit('/').next().unwrap_or("")
}

/// `Integer.parseInt` and the rest: the text's number, or a `NumberFormatException`.
fn parse_text<T: std::str::FromStr>(rt: &mut Rt, s: &str) -> Option<T> {
    let parsed = s.parse::<T>().ok();
    if parsed.is_none() {
        let msg = if s.is_empty() { "For input string: \"\"".to_owned() } else { format!("For input string: \"{s}\"") };
        throw(rt, "java/lang/NumberFormatException", &msg);
    }
    parsed
}

/// `Double.parseDouble`: Java's forms (spaces around, `d`/`f` suffixes, `Infinity`, `NaN`).
fn parse_double(rt: &mut Rt, s: &str) -> f64 {
    let t = s.trim();
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    match t.parse::<f64>() {
        Ok(d) if !t.eq_ignore_ascii_case("inf") && !t.eq_ignore_ascii_case("+inf") && !t.eq_ignore_ascii_case("-inf") => d,
        _ => {
            let msg = if s.trim().is_empty() { "empty String".to_owned() } else { format!("For input string: \"{s}\"") };
            throw(rt, "java/lang/NumberFormatException", &msg);
            0.0
        }
    }
}

/// An element's `getAsX` (`x` the descriptor's return kind; `String` for `getAsString`), as
/// Gson answers it on each kind: a primitive converts, a one-element array answers for its
/// element, the rest throw.
fn get_as(rt: &mut Rt, v: V, name: &str) -> V {
    match rt.class_of(v) {
        PRIMITIVE => primitive_as(rt, v, name),
        ARRAY => {
            let all = items(rt, v);
            if all.len() == 1 {
                return get_as(rt, all[0], name);
            }
            let msg = format!("Array must have size 1, but has size {}", all.len());
            throw(rt, "java/lang/IllegalStateException", &msg)
        }
        _ => {
            let n = simple_name(rt, v);
            throw(rt, "java/lang/UnsupportedOperationException", n)
        }
    }
}

fn primitive_as(rt: &mut Rt, p: V, name: &str) -> V {
    let (k, value) = (kind(rt, p), rt.get_field(p, "value"));
    let as_string = |rt: &Rt| match k {
        STRING => rt.str_of(value).unwrap_or("").to_owned(),
        NUMBER => number_text(value),
        _ => (if value.i() != 0 { "true" } else { "false" }).to_owned(),
    };
    match name {
        "getAsString" => {
            let s = as_string(rt);
            rt.string(&s)
        }
        "getAsBoolean" => match k {
            BOOLEAN => V::I(value.i()),
            _ => V::I(as_string(rt).eq_ignore_ascii_case("true") as i32),
        },
        "getAsInt" | "getAsShort" | "getAsByte" => {
            let i = if k == NUMBER {
                match value {
                    V::F(f) => f as i32,
                    V::D(d) => d as i32,
                    n => as_i64(n) as i32,
                }
            } else {
                let s = as_string(rt);
                parse_text::<i32>(rt, &s).unwrap_or(0)
            };
            V::I(match name {
                "getAsShort" => i32::from(i as i16),
                "getAsByte" => i32::from(i as i8),
                _ => i,
            })
        }
        "getAsLong" => V::J(if k == NUMBER {
            as_i64(value)
        } else {
            let s = as_string(rt);
            parse_text::<i64>(rt, &s).unwrap_or(0)
        }),
        "getAsDouble" => V::D(if k == NUMBER {
            as_f64(value)
        } else {
            let s = as_string(rt);
            parse_double(rt, &s)
        }),
        "getAsFloat" => V::F(if k == NUMBER {
            match value {
                V::F(f) => f,
                n => as_f64(n) as f32,
            }
        } else {
            let s = as_string(rt);
            parse_double(rt, &s) as f32
        }),
        "getAsCharacter" => {
            let s = as_string(rt);
            match s.encode_utf16().next() {
                Some(c) => V::I(i32::from(c)),
                None => throw(rt, "java/lang/StringIndexOutOfBoundsException", "Index 0 out of bounds for length 0"),
            }
        }
        "getAsNumber" => {
            let n = match (k, value) {
                (NUMBER, n) => n,
                _ => {
                    let s = as_string(rt);
                    match json_number(s.trim()) {
                        Some(n) => n,
                        None => V::D(parse_double(rt, &s)),
                    }
                }
            };
            let box_kind = match n {
                V::I(_) => 'I',
                V::J(_) => 'J',
                V::F(_) => 'F',
                _ => 'D',
            };
            crate::mixin::box_value(rt, n, box_kind)
        }
        _ => V::Void,
    }
}

/// `getAsJsonObject()` and its kin: the element as that kind, or Gson's `IllegalStateException`.
fn cast(rt: &mut Rt, v: V, class: &str, what: &str) -> V {
    if rt.class_of(v) == class {
        return v;
    }
    let msg = format!("Not a JSON {what}: {}", write(rt, v, TO_STRING));
    throw(rt, "java/lang/IllegalStateException", &msg)
}

/// An element made from a `java.lang` value: a string, a box (`Number`, `Boolean`,
/// `Character`), or null.
fn from_java(rt: &mut Rt, v: V, desc: &str) -> V {
    if v.r() == 0 && matches!(v, V::R(_)) {
        return null(rt);
    }
    if let Some(s) = rt.str_of(v).map(str::to_owned) {
        return string(rt, &s);
    }
    let raw = match v {
        V::R(_) => rt.get_field(v, "value"),
        raw => raw,
    };
    match (rt.class_of(v), desc) {
        ("java/lang/Boolean", _) | (_, "Z") => boolean(rt, raw.i() != 0),
        ("java/lang/Character", _) | (_, "C") => {
            let s = char::from_u32(raw.i() as u32).map(String::from).unwrap_or_default();
            string(rt, &s)
        }
        ("java/lang/Byte" | "java/lang/Short" | "java/lang/Integer", _) => number(rt, V::I(raw.i())),
        _ => number(rt, raw),
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    match owner {
        HELPER => return helper_call(rt, name, desc, args),
        PARSER => return parser_call(rt, name, desc, args),
        GSON | BUILDER => return gson_call(rt, owner, name, desc, args),
        _ => {}
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    // Constructors: `this` was made by `new` as a plain object.
    if name == "<init>" {
        let data = match owner {
            OBJECT => Data::Table(Table::default()),
            ARRAY => Data::List(Vec::new()),
            PRIMITIVE => {
                let arg = desc.trim_start_matches('(').split(')').next().unwrap_or("");
                let made = from_java(rt, a(1), arg);
                for f in ["$kind", "value"] {
                    let x = rt.get_field(made, f);
                    rt.put_field(this, f, x);
                }
                return Some(V::Void);
            }
            _ => return Some(V::Void),
        };
        if let Some(o) = rt.heap.get_mut(this.r() as usize) {
            o.data = data;
        }
        return Some(V::Void);
    }
    let class = rt.class_of(this);
    Some(match (name, desc) {
        ("isJsonObject", _) => V::I((class == OBJECT) as i32),
        ("isJsonArray", _) => V::I((class == ARRAY) as i32),
        ("isJsonPrimitive", _) => V::I((class == PRIMITIVE) as i32),
        ("isJsonNull", _) => V::I((class == NULL) as i32),
        ("isString", _) => V::I((class == PRIMITIVE && kind(rt, this) == STRING) as i32),
        ("isNumber", _) => V::I((class == PRIMITIVE && kind(rt, this) == NUMBER) as i32),
        ("isBoolean", _) => V::I((class == PRIMITIVE && kind(rt, this) == BOOLEAN) as i32),
        ("getAsJsonObject", "()Lcom/google/gson/JsonObject;") => cast(rt, this, OBJECT, "Object"),
        ("getAsJsonArray", "()Lcom/google/gson/JsonArray;") => cast(rt, this, ARRAY, "Array"),
        ("getAsJsonPrimitive", "()Lcom/google/gson/JsonPrimitive;") => cast(rt, this, PRIMITIVE, "Primitive"),
        ("getAsJsonNull", _) => cast(rt, this, NULL, "Null"),
        (
            "getAsString" | "getAsBoolean" | "getAsInt" | "getAsLong" | "getAsDouble" | "getAsFloat" | "getAsShort" | "getAsByte" | "getAsCharacter" | "getAsNumber",
            _,
        ) if desc.starts_with("()") => get_as(rt, this, name),
        ("deepCopy", _) => deep_copy(rt, this),
        ("toString", _) => {
            let s = write(rt, this, TO_STRING);
            rt.string(&s)
        }
        ("equals", _) => V::I((is_element_class(rt.class_of(a(1))) && same(rt, this, a(1))) as i32),
        ("hashCode", _) => V::I(hash_of(rt, this)),
        _ if class == OBJECT => return object_call(rt, name, desc, args),
        _ if class == ARRAY => return array_call(rt, name, desc, args),
        _ => return None,
    })
}

fn object_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
        "add" => {
            put(rt, this, a(1), a(2));
            V::Void
        }
        "addProperty" => {
            let arg = desc.trim_start_matches("(Ljava/lang/String;").split(')').next().unwrap_or("");
            let v = from_java(rt, a(2), arg);
            put(rt, this, a(1), v);
            V::Void
        }
        "remove" => crate::jutil::table_remove(rt, this, a(1)).unwrap_or(V::NULL),
        "get" => crate::jutil::table_get(rt, this, a(1)).unwrap_or(V::NULL),
        "has" => V::I(crate::jutil::table_get(rt, this, a(1)).is_some() as i32),
        // `getAsJsonObject(name)` and its kin cast the member (null when there is none).
        "getAsJsonObject" | "getAsJsonArray" | "getAsJsonPrimitive" => {
            let m = crate::jutil::table_get(rt, this, a(1)).unwrap_or(V::NULL);
            let want = match name {
                "getAsJsonObject" => OBJECT,
                "getAsJsonArray" => ARRAY,
                _ => PRIMITIVE,
            };
            if m.r() != 0 && rt.class_of(m) != want {
                let msg = format!("class {} cannot be cast to class {}", java_name(rt, m), want.replace('/', "."));
                return Some(throw(rt, "java/lang/ClassCastException", &msg));
            }
            m
        }
        "entrySet" => {
            let entries: Vec<V> = members(rt, this).into_iter().map(|(k, v)| rt.alloc("java/util/Map$Entry", Data::List(vec![k, v]))).collect();
            crate::jutil::identity_set(rt, &entries)
        }
        "keySet" => {
            let keys: Vec<V> = members(rt, this).into_iter().map(|(k, _)| k).collect();
            crate::jutil::new_set_of(rt, &keys)
        }
        "size" => V::I(members(rt, this).len() as i32),
        "isEmpty" => V::I(members(rt, this).is_empty() as i32),
        "asMap" => {
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
            for (k, v) in members(rt, this) {
                crate::jutil::table_put(rt, m, k, v);
            }
            m
        }
        _ => return None,
    })
}

fn array_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let len = items(rt, this).len();
    let index = |rt: &mut Rt, i: i32, len: usize| -> Option<usize> {
        if i < 0 || i as usize >= len {
            throw(rt, "java/lang/IndexOutOfBoundsException", &format!("Index {i} out of bounds for length {len}"));
            None
        } else {
            Some(i as usize)
        }
    };
    Some(match (name, desc) {
        ("add", "(Lcom/google/gson/JsonElement;)V") => {
            let v = if a(1).r() == 0 { null(rt) } else { a(1) };
            if let Some(l) = items_mut(rt, this) {
                l.push(v);
            }
            V::Void
        }
        ("add", _) => {
            let arg = desc.trim_start_matches('(').split(')').next().unwrap_or("");
            let v = from_java(rt, a(1), arg);
            if let Some(l) = items_mut(rt, this) {
                l.push(v);
            }
            V::Void
        }
        ("addAll", _) => {
            let more = items(rt, a(1));
            if let Some(l) = items_mut(rt, this) {
                l.extend(more);
            }
            V::Void
        }
        ("set", _) => {
            let Some(i) = index(rt, a(1).i(), len) else { return Some(V::NULL) };
            let v = if a(2).r() == 0 { null(rt) } else { a(2) };
            items_mut(rt, this).map_or(V::NULL, |l| std::mem::replace(&mut l[i], v))
        }
        ("remove", "(I)Lcom/google/gson/JsonElement;") => {
            let Some(i) = index(rt, a(1).i(), len) else { return Some(V::NULL) };
            items_mut(rt, this).map_or(V::NULL, |l| l.remove(i))
        }
        ("remove", _) => {
            let found = items(rt, this).iter().position(|e| same(rt, *e, a(1)));
            if let (Some(i), Some(l)) = (found, items_mut(rt, this)) {
                l.remove(i);
            }
            V::I(found.is_some() as i32)
        }
        ("contains", _) => V::I(items(rt, this).iter().any(|e| same(rt, *e, a(1))) as i32),
        ("size", _) => V::I(len as i32),
        ("isEmpty", _) => V::I((len == 0) as i32),
        ("get", _) => {
            let Some(i) = index(rt, a(1).i(), len) else { return Some(V::NULL) };
            items(rt, this)[i]
        }
        ("iterator", _) => {
            let all = items(rt, this);
            crate::jutil::new_iter(rt, all)
        }
        ("asList", _) => {
            let all = items(rt, this);
            crate::jutil::new_list(rt, all)
        }
        ("forEach", _) => {
            for e in items(rt, this) {
                rt.call_functional(CONSUMER, a(1), &[e]);
                if rt.thrown.is_some() {
                    break;
                }
            }
            V::Void
        }
        _ => return None,
    })
}

/// A `Gson`'s or builder's settings: pretty printing, HTML-safe escaping (on unless
/// `disableHtmlEscaping`), null members written (`serializeNulls`).
fn style_of(rt: &Rt, gson: V) -> Style {
    Style {
        pretty: rt.get_field(gson, "$pretty") == V::I(1),
        html: rt.get_field(gson, "$html") != V::I(0),
        nulls: rt.get_field(gson, "$nulls") == V::I(1),
        sorted: false,
    }
}

/// The text a source holds: a string, a reader or a stream (read to its end).
fn source_text(rt: &mut Rt, v: V) -> Option<String> {
    crate::resources::take_text(rt, v)
}

/// Reads `text` as Gson's adapter for tree class `class` does (`fromJson(reader,
/// JsonObject.class)`): the element, which must be of that class; None (Java's null) for an empty
/// document.
fn read_as(rt: &mut Rt, text: &str, class: &str, lenient: bool) -> Option<V> {
    match parse(rt, text, lenient) {
        Err(e) => Some(throw(rt, SYNTAX, &e)),
        Ok(None) => None,
        Ok(Some((v, _))) => {
            if class != ELEMENT && rt.class_of(v) != class {
                let msg = format!("Expected a {} but was {}; at path $", class.replace('/', "."), java_name(rt, v));
                return Some(throw(rt, SYNTAX, &msg));
            }
            Some(v)
        }
    }
}

/// Whether `class` is one of the tree's (what Gson reads without a mapping of its own).
fn tree_class(class: &str) -> bool {
    class == ELEMENT || is_element_class(class)
}

fn gson_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    if owner == BUILDER {
        return Some(match name {
            "<init>" => V::Void,
            "setPrettyPrinting" => {
                rt.put_field(this, "$pretty", V::I(1));
                this
            }
            "disableHtmlEscaping" => {
                rt.put_field(this, "$html", V::I(0));
                this
            }
            "serializeNulls" => {
                rt.put_field(this, "$nulls", V::I(1));
                this
            }
            "setLenient" => {
                rt.put_field(this, "$lenient", V::I(1));
                this
            }
            "create" => {
                let g = rt.new_object(GSON);
                for f in ["$pretty", "$html", "$nulls", "$lenient"] {
                    let v = rt.get_field(this, f);
                    rt.put_field(g, f, v);
                }
                g
            }
            // The rest configure what mapping onto classes would use: kept as the builder.
            _ if desc.ends_with(")Lcom/google/gson/GsonBuilder;") => this,
            _ => return None,
        });
    }
    let style = style_of(rt, this);
    Some(match (name, desc) {
        ("<init>", _) => V::Void,
        ("toJson", "(Lcom/google/gson/JsonElement;)Ljava/lang/String;") => {
            let s = write(rt, a(1), style);
            rt.string(&s)
        }
        ("toJson", "(Ljava/lang/Object;)Ljava/lang/String;") if a(1).r() == 0 || is_element_class(rt.class_of(a(1))) => {
            let v = if a(1).r() == 0 { null(rt) } else { a(1) };
            let s = write(rt, v, style);
            rt.string(&s)
        }
        // Into a `StringBuilder`.
        ("toJson", "(Lcom/google/gson/JsonElement;Ljava/lang/Appendable;)V" | "(Ljava/lang/Object;Ljava/lang/Appendable;)V")
            if is_element_class(rt.class_of(a(1))) && matches!(rt.class_of(a(2)), "java/lang/StringBuilder" | "java/lang/StringBuffer") =>
        {
            let s = write(rt, a(1), style);
            let s = rt.string(&s);
            crate::api::call(rt, "java/lang/StringBuilder.append.(Ljava/lang/String;)Ljava/lang/StringBuilder;", &[a(2), s]);
            V::Void
        }
        ("toJsonTree", _) if is_element_class(rt.class_of(a(1))) => deep_copy(rt, a(1)),
        ("fromJson", _) => {
            let class = rt.str_of(a(2)).unwrap_or("").to_owned();
            if !tree_class(&class) {
                return None;
            }
            if is_element_class(rt.class_of(a(1))) {
                let v = a(1);
                if class != ELEMENT && rt.class_of(v) != class {
                    let msg = format!("Expected a {} but was {}; at path $", class.replace('/', "."), java_name(rt, v));
                    return Some(throw(rt, SYNTAX, &msg));
                }
                return Some(v);
            }
            if a(1).r() == 0 {
                return Some(V::NULL);
            }
            let text = source_text(rt, a(1))?;
            let lenient = rt.get_field(this, "$lenient") == V::I(1);
            read_as(rt, &text, &class, lenient).unwrap_or(V::NULL)
        }
        _ => return None,
    })
}

fn parser_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "<init>" => V::Void,
        // `parseString`/`parseReader` (static) and the deprecated instance `parse`: lenient, an
        // empty document is `JsonNull`, and text after the value is an error.
        "parseString" | "parseReader" | "parse" => {
            let source = if name == "parse" { a(1) } else { a(0) };
            if desc.starts_with("(Lcom/google/gson/stream/JsonReader;") {
                return None;
            }
            let text = source_text(rt, source).unwrap_or_default();
            match parse(rt, &text, true) {
                Err(e) => throw(rt, SYNTAX, &e),
                Ok(None) => null(rt),
                Ok(Some((_, false))) => throw(rt, SYNTAX, "Did not consume the entire document."),
                Ok(Some((v, true))) => v,
            }
        }
        _ => return None,
    })
}

/// `StringUtils.abbreviateMiddle(text, "...", 10)`.
fn abbreviate_middle(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= 10 {
        return text.to_owned();
    }
    let (start, end) = (4, units.len() - 3);
    format!("{}...{}", String::from_utf16_lossy(&units[..start]), String::from_utf16_lossy(&units[end..]))
}

/// `GsonHelper.getType`: how its errors name an element.
fn type_name(rt: &Rt, v: V) -> String {
    if v.r() == 0 {
        return "null (missing)".into();
    }
    let s = abbreviate_middle(&write(rt, v, TO_STRING));
    match rt.class_of(v) {
        NULL => "null (json)".into(),
        ARRAY => format!("an array ({s})"),
        OBJECT => format!("an object ({s})"),
        PRIMITIVE if kind(rt, v) == NUMBER => format!("a number ({s})"),
        PRIMITIVE if kind(rt, v) == BOOLEAN => format!("a boolean ({s})"),
        _ => s,
    }
}

/// `GsonHelper`, 1.21.1's: its checks, conversions with their messages, and its reads.
fn helper_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // What the member `a(1)` of object `a(0)` is (None: missing).
    let member = |rt: &mut Rt| if a(0).r() == 0 { None } else { crate::jutil::table_get(rt, a(0), a(1)) };
    let member_name = |rt: &Rt| rt.str_of(a(1)).unwrap_or("").to_owned();
    let on_object = desc.starts_with("(Lcom/google/gson/JsonObject;Ljava/lang/String;");
    let is_prim_kind = |rt: &Rt, v: Option<V>, k: i32| v.is_some_and(|v| rt.class_of(v) == PRIMITIVE && kind(rt, v) == k);
    // The conversions by type: (name suffix, `getAsX` of the element, what errors call it, the
    // element's class it needs: a primitive, or the object or array itself).
    const CONVERSIONS: [(&str, &str, &str); 12] = [
        ("String", "getAsString", "string"),
        ("Boolean", "getAsBoolean", "Boolean"),
        ("Double", "getAsDouble", "Double"),
        ("Float", "getAsFloat", "Float"),
        ("Long", "getAsLong", "Long"),
        ("Int", "getAsInt", "Int"),
        ("Byte", "getAsByte", "Byte"),
        ("Character", "getAsCharacter", "Character"),
        ("Short", "getAsShort", "Short"),
        ("JsonObject", "", "JsonObject"),
        ("JsonArray", "", "JsonArray"),
        ("Object", "", ""),
    ];
    Some(match name {
        "isStringValue" | "isNumberValue" | "isBooleanValue" => {
            let k = match name {
                "isStringValue" => STRING,
                "isNumberValue" => NUMBER,
                _ => BOOLEAN,
            };
            let v = if on_object { member(rt) } else { Some(a(0)).filter(|v| v.r() != 0) };
            V::I(is_prim_kind(rt, v, k) as i32)
        }
        "isArrayNode" => V::I(member(rt).is_some_and(|v| rt.class_of(v) == ARRAY) as i32),
        "isObjectNode" => V::I(member(rt).is_some_and(|v| rt.class_of(v) == OBJECT) as i32),
        "isValidPrimitive" => V::I(member(rt).is_some_and(|v| rt.class_of(v) == PRIMITIVE) as i32),
        "isValidNode" => V::I(member(rt).is_some() as i32),
        "getNonNull" => match member(rt) {
            Some(v) if rt.class_of(v) != NULL => v,
            _ => {
                let msg = format!("Missing field {}", member_name(rt));
                throw(rt, SYNTAX, &msg)
            }
        },
        "getType" => {
            let s = type_name(rt, a(0));
            rt.string(&s)
        }
        "parse" | "parseArray" => {
            let class = if name == "parse" { OBJECT } else { ARRAY };
            let lenient = desc.contains(";Z)") && a(1).i() != 0;
            let text = source_text(rt, a(0)).unwrap_or_default();
            match read_as(rt, &text, class, lenient) {
                Some(v) => v,
                None => throw(rt, PARSE_ERROR, "java.io.EOFException: End of input at line 1 column 1 path $"),
            }
        }
        // `fromJson(gson, reader or text, class[, lenient])` for the tree's classes.
        "fromJson" | "fromNullableJson" => {
            let class = rt.str_of(a(2)).unwrap_or("").to_owned();
            if !tree_class(&class) {
                return None;
            }
            let lenient = desc.ends_with("Z)Ljava/lang/Object;") && a(3).i() != 0;
            let text = source_text(rt, a(1)).unwrap_or_default();
            match read_as(rt, &text, &class, lenient) {
                Some(v) => v,
                None if name == "fromNullableJson" => V::NULL,
                None => throw(rt, PARSE_ERROR, "JSON data was null or empty"),
            }
        }
        "toStableString" => {
            let s = write(rt, a(0), Style { pretty: false, html: false, nulls: true, sorted: true });
            rt.string(&s)
        }
        _ => {
            // `convertToX(element, name)` and `getAsX(object, name[, fallback])`.
            let (get, suffix) = if let Some(s) = name.strip_prefix("convertTo") {
                (false, s)
            } else if let Some(s) = name.strip_prefix("getAs") {
                (true, s)
            } else {
                return None;
            };
            let &(_, getter, what) = CONVERSIONS.iter().find(|(s, _, _)| *s == suffix)?;
            if suffix == "Object" {
                return None;
            }
            let element = if get {
                match member(rt) {
                    Some(v) => v,
                    // A fallback when there is one (the third argument), else an error.
                    None if args.len() > 2 => return Some(a(2)),
                    None => {
                        let msg = format!("Missing {}, expected to find a {what}", member_name(rt));
                        return Some(throw(rt, SYNTAX, &msg));
                    }
                }
            } else {
                a(0)
            };
            let ok = match what {
                "JsonObject" => rt.class_of(element) == OBJECT,
                "JsonArray" => rt.class_of(element) == ARRAY,
                _ => rt.class_of(element) == PRIMITIVE,
            };
            if !ok {
                let msg = format!("Expected {} to be a {what}, was {}", member_name(rt), type_name(rt, element));
                return Some(throw(rt, SYNTAX, &msg));
            }
            if getter.is_empty() { element } else { primitive_as(rt, element, getter) }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let k = rt.static_key(key);
        crate::api::call(rt, k, args)
    }

    fn text(rt: &mut Rt, s: &str) -> V {
        rt.string(s)
    }

    /// A resource file read through the tree as Create's train hat info and GeckoLib's models read
    /// theirs: members by name, numbers as each type, arrays by index, and the file's order kept.
    #[test]
    fn a_parsed_file_reads_as_gsons_tree() {
        crate::install();
        let mut rt = Rt::new();
        let src = text(&mut rt, "{\"entity\": \"minecraft:pig\", \"offset\": [0, 1.5, -2e1], \"scale\": 1.25, \"big\": 5000000000, \"on\": true, \"z\": null, \"a\": {\"x\": \"7\"}}");
        let root = call(&mut rt, "com/google/gson/JsonParser.parseString.(Ljava/lang/String;)Lcom/google/gson/JsonElement;", &[src]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.isJsonObject.()Z", &[root]), V::I(1));
        let o = call(&mut rt, "com/google/gson/JsonElement.getAsJsonObject.()Lcom/google/gson/JsonObject;", &[root]);
        let k = text(&mut rt, "entity");
        let entity = call(&mut rt, "com/google/gson/JsonObject.get.(Ljava/lang/String;)Lcom/google/gson/JsonElement;", &[o, k]);
        let s = call(&mut rt, "com/google/gson/JsonElement.getAsString.()Ljava/lang/String;", &[entity]);
        assert_eq!(rt.str_of(s), Some("minecraft:pig"));
        let k = text(&mut rt, "offset");
        let offset = call(&mut rt, "com/google/gson/JsonObject.getAsJsonArray.(Ljava/lang/String;)Lcom/google/gson/JsonArray;", &[o, k]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonArray.size.()I", &[offset]), V::I(3));
        let y = call(&mut rt, "com/google/gson/JsonArray.get.(I)Lcom/google/gson/JsonElement;", &[offset, V::I(1)]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.getAsFloat.()F", &[y]), V::F(1.5));
        let z = call(&mut rt, "com/google/gson/JsonArray.get.(I)Lcom/google/gson/JsonElement;", &[offset, V::I(2)]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.getAsDouble.()D", &[z]), V::D(-20.0));
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.getAsInt.()I", &[z]), V::I(-20));
        let k = text(&mut rt, "big");
        let big = call(&mut rt, "com/google/gson/JsonObject.get.(Ljava/lang/String;)Lcom/google/gson/JsonElement;", &[o, k]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.getAsLong.()J", &[big]), V::J(5_000_000_000));
        // `(int)` of a long wraps, as `LazilyParsedNumber.intValue` does.
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.getAsInt.()I", &[big]), V::I(5_000_000_000i64 as i32));
        let k = text(&mut rt, "z");
        let z = call(&mut rt, "com/google/gson/JsonObject.get.(Ljava/lang/String;)Lcom/google/gson/JsonElement;", &[o, k]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.isJsonNull.()Z", &[z]), V::I(1));
        let k = text(&mut rt, "missing");
        assert_eq!(call(&mut rt, "com/google/gson/JsonObject.has.(Ljava/lang/String;)Z", &[o, k]), V::I(0));
        // A string member read as a number parses; the nested object is shared with its parent.
        let k = text(&mut rt, "a");
        let inner = call(&mut rt, "com/google/gson/JsonObject.getAsJsonObject.(Ljava/lang/String;)Lcom/google/gson/JsonObject;", &[o, k]);
        let x = text(&mut rt, "x");
        let seven = call(&mut rt, "com/google/gson/JsonObject.get.(Ljava/lang/String;)Lcom/google/gson/JsonElement;", &[inner, x]);
        assert_eq!(call(&mut rt, "com/google/gson/JsonElement.getAsInt.()I", &[seven]), V::I(7));
        let w = text(&mut rt, "w");
        let yes = rt.new_object("java/lang/Boolean");
        rt.put_field(yes, "value", V::I(1));
        call(&mut rt, "com/google/gson/JsonObject.addProperty.(Ljava/lang/String;Ljava/lang/Boolean;)V", &[inner, w, yes]);
        let printed = call(&mut rt, "com/google/gson/JsonElement.toString.()Ljava/lang/String;", &[root]);
        assert_eq!(
            rt.str_of(printed),
            Some(r#"{"entity":"minecraft:pig","offset":[0,1.5,-20.0],"scale":1.25,"big":5000000000,"on":true,"z":null,"a":{"x":"7","w":true}}"#)
        );
        let keys = call(&mut rt, "com/google/gson/JsonObject.keySet.()Ljava/util/Set;", &[o]);
        let keys: Vec<String> = crate::jutil::elements(&rt, keys).iter().map(|k| rt.str_of(*k).unwrap_or("").to_owned()).collect();
        assert_eq!(keys, ["entity", "offset", "scale", "big", "on", "z", "a"]);
        assert!(rt.thrown.is_none() && rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own tree written by a `Gson` as Gson writes it: pretty printing's two spaces and
    /// `": "`, HTML-safe escaping unless disabled, null members left out unless `serializeNulls`.
    #[test]
    fn a_built_tree_is_written_as_gson_writes_it() {
        crate::install();
        let mut rt = Rt::new();
        let o = rt.new_object(OBJECT);
        call(&mut rt, "com/google/gson/JsonObject.<init>.()V", &[o]);
        let (k, v) = (text(&mut rt, "name"), text(&mut rt, "<a & b>"));
        call(&mut rt, "com/google/gson/JsonObject.addProperty.(Ljava/lang/String;Ljava/lang/String;)V", &[o, k, v]);
        let list = rt.new_object(ARRAY);
        call(&mut rt, "com/google/gson/JsonArray.<init>.()V", &[list]);
        let n = crate::mixin::box_value(&mut rt, V::I(3), 'I');
        call(&mut rt, "com/google/gson/JsonArray.add.(Ljava/lang/Number;)V", &[list, n]);
        let f = crate::mixin::box_value(&mut rt, V::F(0.1), 'F');
        call(&mut rt, "com/google/gson/JsonArray.add.(Ljava/lang/Number;)V", &[list, f]);
        let k = text(&mut rt, "list");
        call(&mut rt, "com/google/gson/JsonObject.add.(Ljava/lang/String;Lcom/google/gson/JsonElement;)V", &[o, k, list]);
        let k = text(&mut rt, "gone");
        call(&mut rt, "com/google/gson/JsonObject.add.(Ljava/lang/String;Lcom/google/gson/JsonElement;)V", &[o, k, V::NULL]);
        let empty = new_object(&mut rt);
        let k = text(&mut rt, "empty");
        call(&mut rt, "com/google/gson/JsonObject.add.(Ljava/lang/String;Lcom/google/gson/JsonElement;)V", &[o, k, empty]);

        let builder = rt.new_object(BUILDER);
        call(&mut rt, "com/google/gson/GsonBuilder.<init>.()V", &[builder]);
        call(&mut rt, "com/google/gson/GsonBuilder.setPrettyPrinting.()Lcom/google/gson/GsonBuilder;", &[builder]);
        let gson = call(&mut rt, "com/google/gson/GsonBuilder.create.()Lcom/google/gson/Gson;", &[builder]);
        let out = call(&mut rt, "com/google/gson/Gson.toJson.(Lcom/google/gson/JsonElement;)Ljava/lang/String;", &[gson, o]);
        assert_eq!(rt.str_of(out), Some("{\n  \"name\": \"\\u003ca \\u0026 b\\u003e\",\n  \"list\": [\n    3,\n    0.1\n  ],\n  \"empty\": {}\n}"));

        call(&mut rt, "com/google/gson/GsonBuilder.disableHtmlEscaping.()Lcom/google/gson/GsonBuilder;", &[builder]);
        call(&mut rt, "com/google/gson/GsonBuilder.serializeNulls.()Lcom/google/gson/GsonBuilder;", &[builder]);
        let gson = call(&mut rt, "com/google/gson/GsonBuilder.create.()Lcom/google/gson/Gson;", &[builder]);
        let out = call(&mut rt, "com/google/gson/Gson.toJson.(Lcom/google/gson/JsonElement;)Ljava/lang/String;", &[gson, list]);
        assert_eq!(rt.str_of(out), Some("[\n  3,\n  0.1\n]"));
        let out = call(&mut rt, "com/google/gson/Gson.toJson.(Lcom/google/gson/JsonElement;)Ljava/lang/String;", &[gson, o]);
        assert!(rt.str_of(out).unwrap().contains("\"<a & b>\"") && rt.str_of(out).unwrap().contains("\"gone\": null"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `GsonHelper`'s reads and its errors, word for word as 1.21.1's.
    #[test]
    fn gson_helper_reads_and_complains_as_vanillas() {
        crate::install();
        let mut rt = Rt::new();
        let src = text(&mut rt, "{\"count\": 4, \"name\": \"drill\", \"tags\": [\"a\"], \"flag\": true}");
        let o = call(&mut rt, "net/minecraft/util/GsonHelper.parse.(Ljava/lang/String;)Lcom/google/gson/JsonObject;", &[src]);
        let key = |rt: &mut Rt, s: &str| rt.string(s);
        let (count, name, tags, missing) = (key(&mut rt, "count"), key(&mut rt, "name"), key(&mut rt, "tags"), key(&mut rt, "missing"));
        assert_eq!(call(&mut rt, "net/minecraft/util/GsonHelper.getAsInt.(Lcom/google/gson/JsonObject;Ljava/lang/String;)I", &[o, count]), V::I(4));
        assert_eq!(call(&mut rt, "net/minecraft/util/GsonHelper.getAsInt.(Lcom/google/gson/JsonObject;Ljava/lang/String;I)I", &[o, missing, V::I(9)]), V::I(9));
        let s = call(&mut rt, "net/minecraft/util/GsonHelper.getAsString.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Ljava/lang/String;", &[o, name]);
        assert_eq!(rt.str_of(s), Some("drill"));
        // A number reads as a string, as `getAsString` of a primitive does.
        let s = call(&mut rt, "net/minecraft/util/GsonHelper.getAsString.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Ljava/lang/String;", &[o, count]);
        assert_eq!(rt.str_of(s), Some("4"));
        assert_eq!(call(&mut rt, "net/minecraft/util/GsonHelper.isArrayNode.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Z", &[o, tags]), V::I(1));
        let message = |rt: &mut Rt| {
            let e = rt.thrown.take().expect("thrown");
            let m = rt.get_field(e, "message");
            (rt.class_of(e).to_owned(), rt.str_of(m).unwrap_or("").to_owned())
        };
        call(&mut rt, "net/minecraft/util/GsonHelper.getAsString.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Ljava/lang/String;", &[o, tags]);
        assert_eq!(message(&mut rt), (SYNTAX.to_owned(), "Expected tags to be a string, was an array ([\"a\"])".to_owned()));
        call(&mut rt, "net/minecraft/util/GsonHelper.getAsInt.(Lcom/google/gson/JsonObject;Ljava/lang/String;)I", &[o, missing]);
        assert_eq!(message(&mut rt), (SYNTAX.to_owned(), "Missing missing, expected to find a Int".to_owned()));
        call(&mut rt, "net/minecraft/util/GsonHelper.getAsJsonObject.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Lcom/google/gson/JsonObject;", &[o, missing]);
        assert_eq!(message(&mut rt), (SYNTAX.to_owned(), "Missing missing, expected to find a JsonObject".to_owned()));
        call(&mut rt, "net/minecraft/util/GsonHelper.getAsJsonObject.(Lcom/google/gson/JsonObject;Ljava/lang/String;)Lcom/google/gson/JsonObject;", &[o, count]);
        assert_eq!(message(&mut rt), (SYNTAX.to_owned(), "Expected count to be a JsonObject, was a number (4)".to_owned()));
        let long = text(&mut rt, "{\"k\": \"0123456789abc\"}");
        let lo = call(&mut rt, "net/minecraft/util/GsonHelper.parse.(Ljava/lang/String;)Lcom/google/gson/JsonObject;", &[long]);
        let t = call(&mut rt, "net/minecraft/util/GsonHelper.getType.(Lcom/google/gson/JsonElement;)Ljava/lang/String;", &[lo]);
        assert_eq!(rt.str_of(t), Some("an object ({\"k\"...c\"})"));
        let array = text(&mut rt, "[1]");
        call(&mut rt, "net/minecraft/util/GsonHelper.parse.(Ljava/lang/String;)Lcom/google/gson/JsonObject;", &[array]);
        assert_eq!(message(&mut rt), (SYNTAX.to_owned(), "Expected a com.google.gson.JsonObject but was com.google.gson.JsonArray; at path $".to_owned()));
        let bad = text(&mut rt, "{\"a\": }");
        call(&mut rt, "com/google/gson/JsonParser.parseString.(Ljava/lang/String;)Lcom/google/gson/JsonElement;", &[bad]);
        assert_eq!(message(&mut rt).0, SYNTAX);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Gson's lenient reading (`JsonParser`): comments, single quotes, unquoted names.
    #[test]
    fn the_parser_is_lenient_as_gsons() {
        let mut rt = Rt::new();
        let (v, all) = parse(&mut rt, "// a config\n{name: 'x', /* why */ n: 2; list: [1,]} # end", true).unwrap().unwrap();
        assert!(all);
        assert_eq!(write(&rt, v, TO_STRING), r#"{"name":"x","n":2,"list":[1,null]}"#);
        assert!(parse(&mut rt, "{name: 1}", false).is_err());
        assert!(parse(&mut rt, "   ", true).unwrap().is_none());
        let (v, _) = parse(&mut rt, r#""aé😀\n""#, false).unwrap().unwrap();
        assert_eq!(rt.str_of(rt.get_field(v, "value")), Some("aé😀\n"));
    }
}
