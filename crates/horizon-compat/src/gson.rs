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
const STREAMS: &str = "com/google/gson/internal/Streams";
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
    matches!(
        owner,
        ELEMENT
            | OBJECT
            | ARRAY
            | PRIMITIVE
            | NULL
            | PARSER
            | STREAMS
            | GSON
            | BUILDER
            | HELPER
            | TOKEN
            | FIELD_ATTRIBUTES
            | TYPE_ADAPTER
            | OWN_ADAPTER
            | SERIALIZATION_CONTEXT
            | DESERIALIZATION_CONTEXT
    )
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

/// A number read from JSON text: its value, and the text itself where Java's `toString` of the
/// value would spell it otherwise (`1.50`, `1e3`, `-0`, an integer longer than a `long`). Gson
/// keeps what it parsed as the text (`LazilyParsedNumber`), so writing the tree back and
/// `getAsString` give the file's own spelling.
fn parsed_number(rt: &mut Rt, n: V, text: &str) -> V {
    let p = number(rt, n);
    if number_text(n) != text {
        let t = rt.alloc("java/lang/String", Data::Str(text.to_owned()));
        rt.put_field(p, "$text", t);
    }
    p
}

/// A number primitive's text: as it was parsed, else Java's `toString` of the value.
fn number_string(rt: &Rt, p: V) -> String {
    match rt.str_of(rt.get_field(p, "$text")) {
        Some(text) => text.to_owned(),
        None => number_text(rt.get_field(p, "value")),
    }
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
            return Ok(parsed_number(rt, n, &word));
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
            NUMBER => out.push_str(&number_string(rt, v)),
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
        NUMBER => number_string(rt, p),
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
        // Gson's own helper, which a config library calls for itself: a tree written to a
        // writer, and one read from a reader.
        STREAMS => {
            let a = |i: usize| args.get(i).copied().unwrap_or_default();
            return Some(match name {
                "write" => {
                    let tree = if a(0).r() == 0 { null(rt) } else { a(0) };
                    write_to(rt, a(1), tree, None);
                    V::Void
                }
                "parse" => parse_reader(rt, a(0)),
                _ => return None,
            });
        }
        GSON | BUILDER => return gson_call(rt, owner, name, desc, args),
        FIELD_ATTRIBUTES => return field_attributes_call(rt, name, args),
        TOKEN => return token_call(rt, name, args),
        // One of the runtime's own adapters, asked as a `TypeAdapter` (or, where that class is
        // Gson's own code among the mod's, by the adapter's own class: what a call that found
        // no code for it is asked by next).
        TYPE_ADAPTER | OWN_ADAPTER => return adapter_call(rt, name, desc, args),
        SERIALIZATION_CONTEXT | DESERIALIZATION_CONTEXT => return context_call(rt, name, args),
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

// ---- Types and adapters ----
//
// What a mod registers with a `GsonBuilder` to write and read a type its own way: a
// `JsonSerializer`, a `JsonDeserializer`, a `TypeAdapter`, an `InstanceCreator`, a
// `TypeAdapterFactory`. The `Gson` asks them before it maps a value itself, in Gson's order.

const TOKEN: &str = "com/google/gson/reflect/TypeToken";
const TYPE_ADAPTER: &str = "com/google/gson/TypeAdapter";
const SERIALIZER: &str = "com/google/gson/JsonSerializer";
const DESERIALIZER: &str = "com/google/gson/JsonDeserializer";
const SERIALIZATION_CONTEXT: &str = "com/google/gson/JsonSerializationContext";
const DESERIALIZATION_CONTEXT: &str = "com/google/gson/JsonDeserializationContext";
/// The runtime's own `TypeAdapter` (`Gson.getAdapter`, `getDelegateAdapter`): a `Gson`, a type,
/// and which of the Gson's adapters it starts at.
const OWN_ADAPTER: &str = "compat:gson-adapter";
/// The context a serializer or a deserializer is handed: its `Gson`.
const CONTEXT: &str = "compat:gson-context";

/// How a registered adapter is matched to a type: the very type (a `TypeAdapter`), the type or,
/// registered for a plain class, any type of that class (a serializer or a deserializer), a
/// class and those under it (`registerTypeHierarchyAdapter`), or whatever its factory makes one for.
const EXACT: i32 = 0;
const OF_CLASS: i32 = 1;
const BELOW: i32 = 2;
const MADE_BY: i32 = 3;

/// One adapter a `Gson` has: the type it was registered for, the object, how it matches.
struct Entry {
    sig: String,
    adapter: V,
    mode: i32,
}

/// A builder's settings and what was registered with it, onto the `Gson` it makes (or back, for
/// `Gson.newBuilder()`): each has its own lists from then on.
fn copy_settings(rt: &mut Rt, from: V, to: V) {
    for f in [
        "$pretty",
        "$html",
        "$nulls",
        "$lenient",
        "$naming",
        "$namer",
        "$modifiers",
    ] {
        let v = rt.get_field(from, f);
        rt.put_field(to, f, v);
    }
    for f in ["$adapters", "$creators", "$skipWriting", "$skipReading"] {
        let list = rt.get_field(from, f);
        if list.r() != 0 {
            let entries = crate::jutil::elements(rt, list);
            let copy = crate::jutil::new_list(rt, entries);
            rt.put_field(to, f, copy);
        }
    }
}

/// Adds an entry to one of a builder's lists (`$adapters`, `$creators`).
fn add_entry(rt: &mut Rt, builder: V, field: &'static str, entry: Vec<V>) {
    let entry = crate::jutil::new_list(rt, entry);
    let list = rt.get_field(builder, field);
    if list.r() == 0 {
        let list = crate::jutil::new_list(rt, vec![entry]);
        rt.put_field(builder, field, list);
    } else {
        crate::api::call(
            rt,
            "java/util/List.add.(Ljava/lang/Object;)Z",
            &[list, entry],
        );
    }
}

/// An adapter of a builder's: `list` 0 for the ones Gson asks first (last registered first), 1
/// for the hierarchy's serializers and deserializers, asked after those.
fn add_adapter(rt: &mut Rt, builder: V, sig: &str, adapter: V, mode: i32, list: i32) {
    let sig = rt.string(sig);
    add_entry(
        rt,
        builder,
        "$adapters",
        vec![sig, adapter, V::I(mode), V::I(list)],
    );
}

/// `registerTypeAdapter(type, object)` and `registerTypeHierarchyAdapter(class, object)`: the
/// object is whichever of Gson's four it implements, and may be several.
fn register(rt: &mut Rt, builder: V, sig: &str, adapter: V, hierarchy: bool) {
    if !hierarchy && rt.instance_of(adapter, "com/google/gson/InstanceCreator") {
        let s = rt.string(sig);
        add_entry(rt, builder, "$creators", vec![s, adapter]);
    }
    if rt.instance_of(adapter, SERIALIZER) || rt.instance_of(adapter, DESERIALIZER) {
        let (mode, list) = if hierarchy { (BELOW, 1) } else { (OF_CLASS, 0) };
        add_adapter(rt, builder, sig, adapter, mode, list);
    }
    if rt.instance_of(adapter, TYPE_ADAPTER) || rt.class_of(adapter) == OWN_ADAPTER {
        add_adapter(
            rt,
            builder,
            sig,
            adapter,
            if hierarchy { BELOW } else { EXACT },
            0,
        );
    }
}

fn has_adapters(rt: &Rt, gson: V) -> bool {
    gson.r() != 0 && rt.get_field(gson, "$adapters").r() != 0
}

/// A `Gson`'s adapters in the order it asks them: the type adapters, the serializers and the
/// factories, the last registered first; then the hierarchy's serializers and deserializers, the
/// last registered first.
fn chain(rt: &Rt, gson: V) -> Vec<Entry> {
    if !has_adapters(rt, gson) {
        return Vec::new();
    }
    let all: Vec<(Entry, i32)> = crate::jutil::elements(rt, rt.get_field(gson, "$adapters"))
        .into_iter()
        .map(|e| {
            let p = crate::jutil::elements(rt, e);
            let at = |i: usize| p.get(i).copied().unwrap_or_default();
            (
                Entry {
                    sig: rt.str_of(at(0)).unwrap_or("").to_owned(),
                    adapter: at(1),
                    mode: at(2).i(),
                },
                at(3).i(),
            )
        })
        .collect();
    let (first, hierarchy): (Vec<_>, Vec<_>) = all.into_iter().partition(|(_, list)| *list == 0);
    first
        .into_iter()
        .rev()
        .chain(hierarchy.into_iter().rev())
        .map(|(e, _)| e)
        .collect()
}

/// The first of a `Gson`'s adapters, from the one at `from` on, that answers for type `asked`
/// (`writing` it, or reading it): a serializer alone reads nothing and a deserializer writes
/// nothing, so the next is asked; a factory answers with what it makes for the type.
fn adapter_for(rt: &mut Rt, gson: V, asked: &str, from: usize, writing: bool) -> Option<V> {
    let asked_class = crate::jtypes::parts(asked).0;
    for (i, e) in chain(rt, gson).into_iter().enumerate().skip(from) {
        let hit = match e.mode {
            MADE_BY => match factory_adapter(rt, gson, i, e.adapter, asked) {
                Some(made) => return Some(made),
                None => false,
            },
            EXACT => e.sig == asked,
            OF_CLASS => {
                e.sig == asked
                    || (!e.sig.contains('<') && crate::jtypes::parts(&e.sig).0 == asked_class)
            }
            _ => rt.is_a(&asked_class, &crate::jtypes::parts(&e.sig).0),
        };
        let tree_adapter =
            !rt.instance_of(e.adapter, TYPE_ADAPTER) && rt.class_of(e.adapter) != OWN_ADAPTER;
        let other_way = tree_adapter
            && !rt.instance_of(e.adapter, if writing { SERIALIZER } else { DESERIALIZER });
        if hit && !other_way {
            return Some(e.adapter);
        }
    }
    None
}

/// What the factory at `i` of a `Gson`'s adapters makes for a type (`create(gson, typeToken)`),
/// asked once for each type as Gson asks it: an adapter, or nothing for a type it leaves alone.
fn factory_adapter(rt: &mut Rt, gson: V, i: usize, factory: V, sig: &str) -> Option<V> {
    let mut made = rt.get_field(gson, "$made");
    if made.r() == 0 {
        made = rt.alloc("compat:gson-made", Data::Map(Default::default()));
        rt.put_field(gson, "$made", made);
    }
    let key = format!("{i}:{sig}");
    if let Some(Data::Map(m)) = rt.heap.get(made.r() as usize).map(|o| &o.data)
        && let Some(v) = m.get(&key)
    {
        return Some(*v).filter(|v| v.r() != 0);
    }
    let token = token_of(rt, sig);
    let adapter = rt.call_functional(
        "com/google/gson/TypeAdapterFactory.create.(Lcom/google/gson/Gson;Lcom/google/gson/reflect/TypeToken;)Lcom/google/gson/TypeAdapter;",
        factory,
        &[gson, token],
    );
    let adapter = if adapter.r() == 0 { V::NULL } else { adapter };
    if let Some(Data::Map(m)) = rt.heap.get_mut(made.r() as usize).map(|o| &mut o.data) {
        m.insert(key, adapter);
    }
    Some(adapter).filter(|v| v.r() != 0)
}

/// The tree an adapter writes for `v` of type `sig`: a `TypeAdapter` through a writer into a
/// tree (its own `toJsonTree`), a serializer by its `serialize`, which is not asked for a null.
fn write_with(rt: &mut Rt, gson: V, adapter: V, v: V, sig: &str) -> V {
    let tree = if rt.class_of(adapter) == OWN_ADAPTER {
        own_to_tree(rt, adapter, v)
    } else if rt.instance_of(adapter, TYPE_ADAPTER) {
        rt.call_functional("com/google/gson/TypeAdapter.toJsonTree.(Ljava/lang/Object;)Lcom/google/gson/JsonElement;", adapter, &[v])
    } else if v.r() == 0 {
        V::NULL
    } else {
        let (ty, context) = (crate::jtypes::of(rt, sig), context_of(rt, gson));
        rt.call_functional(
            "com/google/gson/JsonSerializer.serialize.(Ljava/lang/Object;Ljava/lang/reflect/Type;Lcom/google/gson/JsonSerializationContext;)Lcom/google/gson/JsonElement;",
            adapter,
            &[v, ty, context],
        )
    };
    if tree.r() == 0 { null(rt) } else { tree }
}

/// What an adapter reads from a tree: the reverse of [`write_with`].
fn read_with(rt: &mut Rt, gson: V, adapter: V, json: V, sig: &str) -> V {
    let absent = json.r() == 0 || rt.class_of(json) == NULL;
    if rt.class_of(adapter) == OWN_ADAPTER {
        return own_from_tree(rt, adapter, json);
    }
    if rt.instance_of(adapter, TYPE_ADAPTER) {
        let json = if json.r() == 0 { null(rt) } else { json };
        return rt.call_functional("com/google/gson/TypeAdapter.fromJsonTree.(Lcom/google/gson/JsonElement;)Ljava/lang/Object;", adapter, &[json]);
    }
    if absent {
        return V::NULL;
    }
    let (ty, context) = (crate::jtypes::of(rt, sig), context_of(rt, gson));
    rt.call_functional(
        "com/google/gson/JsonDeserializer.deserialize.(Lcom/google/gson/JsonElement;Ljava/lang/reflect/Type;Lcom/google/gson/JsonDeserializationContext;)Ljava/lang/Object;",
        adapter,
        &[json, ty, context],
    )
}

/// The object an `InstanceCreator` registered for the type makes (for the type itself, else for
/// its class), where there is one.
fn created(rt: &mut Rt, gson: V, ty: &str) -> Option<V> {
    if gson.r() == 0 || rt.get_field(gson, "$creators").r() == 0 {
        return None;
    }
    let all: Vec<(String, V)> = crate::jutil::elements(rt, rt.get_field(gson, "$creators"))
        .into_iter()
        .map(|e| {
            let p = crate::jutil::elements(rt, e);
            (
                rt.str_of(p.first().copied().unwrap_or_default())
                    .unwrap_or("")
                    .to_owned(),
                p.get(1).copied().unwrap_or_default(),
            )
        })
        .collect();
    let raw = generic(ty).0;
    let (sig, creator) = all
        .iter()
        .rev()
        .find(|(sig, _)| sig == ty)
        .or_else(|| all.iter().rev().find(|(sig, _)| *sig == raw))?;
    let ty = crate::jtypes::of(rt, sig);
    let made = rt.call_functional("com/google/gson/InstanceCreator.createInstance.(Ljava/lang/reflect/Type;)Ljava/lang/Object;", *creator, &[ty]);
    Some(made).filter(|v| v.r() != 0)
}

/// `Gson.toJsonTree(value[, type])`: `JsonNull` for nothing, else the value's tree by the type
/// given, or by its own class.
fn tree_of(rt: &mut Rt, gson: V, v: V, declared: Option<String>, nulls: bool) -> V {
    if v.r() == 0 && matches!(v, V::R(_) | V::Void) && declared.is_none() {
        return null(rt);
    }
    let ty = match declared {
        Some(ty) => ty,
        None if matches!(v, V::R(_)) => format!("L{};", rt.class_of(v)),
        None => ANY.to_owned(),
    };
    to_tree_in(rt, gson, v, &ty, nulls, 0, 0)
}

/// One of the runtime's own adapters: what `gson` does with type `sig`, by its adapters from the
/// one at `from` on.
fn own_adapter(rt: &mut Rt, gson: V, sig: &str, from: usize) -> V {
    let a = rt.new_object(OWN_ADAPTER);
    let s = rt.string(sig);
    rt.put_field(a, "$gson", gson);
    rt.put_field(a, "$type", s);
    rt.put_field(a, "$from", V::I(from as i32));
    a
}

fn own_parts(rt: &Rt, adapter: V) -> (V, String, usize) {
    (
        rt.get_field(adapter, "$gson"),
        rt.str_of(rt.get_field(adapter, "$type"))
            .unwrap_or(ANY)
            .to_owned(),
        rt.get_field(adapter, "$from").i().max(0) as usize,
    )
}

/// A `TypeAdapter` writes every null it is given (its writer, on its own, writes them).
fn own_to_tree(rt: &mut Rt, adapter: V, v: V) -> V {
    let (gson, sig, from) = own_parts(rt, adapter);
    to_tree_in(rt, gson, v, &sig, true, 0, from)
}

fn own_from_tree(rt: &mut Rt, adapter: V, json: V) -> V {
    let (gson, sig, from) = own_parts(rt, adapter);
    if tree_class(&crate::jtypes::parts(&sig).0) {
        return if json.r() == 0 { null(rt) } else { json };
    }
    from_tree_in(rt, gson, json, &sig, 0, from)
}

/// `TypeAdapter`'s methods on one of the runtime's own ([`OWN_ADAPTER`]); a mod's adapter runs
/// its own code and Gson's.
fn adapter_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    if rt.class_of(this) != OWN_ADAPTER {
        return None;
    }
    // As a `JsonWriter` made for the purpose writes: compact, nulls and all, nothing escaped.
    let plain = Style {
        pretty: false,
        html: false,
        nulls: true,
        sorted: false,
    };
    Some(match (name, desc) {
        ("write", _) => {
            let tree = own_to_tree(rt, this, a(2));
            if rt.thrown.is_none() {
                write_to(rt, a(1), tree, None);
            }
            V::Void
        }
        ("read", _) => match read_from(rt, a(1)) {
            Some(tree) => own_from_tree(rt, this, tree),
            None => V::NULL,
        },
        ("toJsonTree", _) => own_to_tree(rt, this, a(1)),
        ("fromJsonTree", _) => own_from_tree(rt, this, a(1)),
        ("toJson", "(Ljava/lang/Object;)Ljava/lang/String;") => {
            let tree = own_to_tree(rt, this, a(1));
            let s = write(rt, tree, plain);
            rt.string(&s)
        }
        ("toJson", _) => {
            let tree = own_to_tree(rt, this, a(2));
            let s = write(rt, tree, plain);
            let s = rt.string(&s);
            crate::api::call(rt, "java/io/Writer.write.(Ljava/lang/String;)V", &[a(1), s]);
            V::Void
        }
        // Its reader is strict, as one made by `new JsonReader(reader)` is.
        ("fromJson", _) => {
            let text = source_text(rt, a(1))?;
            match parse(rt, &text, false) {
                Err(e) => throw(rt, "com/google/gson/stream/MalformedJsonException", &e),
                Ok(None) => throw(
                    rt,
                    "java/io/EOFException",
                    "End of input at line 1 column 1 path $",
                ),
                Ok(Some((tree, _))) => own_from_tree(rt, this, tree),
            }
        }
        ("nullSafe", _) => this,
        _ => return None,
    })
}

/// The context a `Gson` hands its serializers and deserializers: one object for the `Gson`.
fn context_of(rt: &mut Rt, gson: V) -> V {
    let made = rt.get_field(gson, "$context");
    if made.r() != 0 {
        return made;
    }
    let c = rt.new_object(CONTEXT);
    rt.put_field(c, "$gson", gson);
    rt.put_field(gson, "$context", c);
    c
}

/// `context.serialize(value[, type])` and `context.deserialize(json, type)`: the `Gson`'s own
/// `toJsonTree` and `fromJson`.
fn context_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if rt.class_of(a(0)) != CONTEXT {
        return None;
    }
    let gson = rt.get_field(a(0), "$gson");
    Some(match name {
        "serialize" => {
            let declared = (args.len() > 2).then(|| crate::jtypes::signature(rt, a(2)));
            let nulls = style_of(rt, gson).nulls;
            tree_of(rt, gson, a(1), declared, nulls)
        }
        "deserialize" if a(1).r() == 0 => V::NULL,
        "deserialize" => {
            let ty = crate::jtypes::signature(rt, a(2));
            if tree_class(&crate::jtypes::parts(&ty).0) {
                return Some(a(1));
            }
            from_tree_in(rt, gson, a(1), &ty, 0, 0)
        }
        _ => return None,
    })
}

/// A `TypeToken` for a signature: Gson's name for a type with its arguments.
fn token_of(rt: &mut Rt, sig: &str) -> V {
    let t = rt.new_object(TOKEN);
    let s = rt.string(sig);
    rt.put_field(t, "$signature", s);
    t
}

/// `TypeToken`: made by a class of the mod's that names its type argument (`new
/// TypeToken<List<Entry>>() {}`), or for a type given.
fn token_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let own = |rt: &Rt| {
        rt.str_of(rt.get_field(this, "$signature"))
            .unwrap_or(ANY)
            .to_owned()
    };
    Some(match name {
        // The type argument the object's class gives `TypeToken`, found up its classes.
        "<init>" => {
            let mut class = rt.class_of(this).to_owned();
            let mut sig = ANY.to_owned();
            loop {
                if let Some((_, up, _)) = crate::jtypes::class_signature(rt, &class)
                    && up.starts_with("Lcom/google/gson/reflect/TypeToken<")
                {
                    sig = crate::jtypes::parts(&up).1.first().cloned().unwrap_or(sig);
                    break;
                }
                match rt.supers.get(class.as_str()).copied() {
                    Some(up) if up != TOKEN => class = up.to_owned(),
                    _ => break,
                }
            }
            // A wildcard or a type variable there stands for its bound.
            let sig = match sig.as_bytes().first() {
                Some(b'+' | b'-') => sig[1..].to_owned(),
                Some(b'*' | b'T') => ANY.to_owned(),
                _ => sig,
            };
            let s = rt.string(&sig);
            rt.put_field(this, "$signature", s);
            V::Void
        }
        "get" => {
            let sig = crate::jtypes::signature(rt, a(0));
            token_of(rt, &sig)
        }
        "getParameterized" => {
            let raw = crate::jtypes::parts(&crate::jtypes::signature(rt, a(0))).0;
            let of: Vec<String> = crate::jutil::elements(rt, a(1))
                .into_iter()
                .map(|t| crate::jtypes::signature(rt, t))
                .collect();
            let sig = if of.is_empty() {
                format!("L{raw};")
            } else {
                format!("L{raw}<{}>;", of.concat())
            };
            token_of(rt, &sig)
        }
        "getArray" => {
            let sig = format!("[{}", crate::jtypes::signature(rt, a(0)));
            token_of(rt, &sig)
        }
        "getType" => {
            let sig = own(rt);
            crate::jtypes::of(rt, &sig)
        }
        "getRawType" => {
            let sig = own(rt);
            let raw = if sig.starts_with('[') {
                sig.split('<').next().unwrap_or(&sig).to_owned()
            } else {
                crate::jtypes::parts(&sig).0
            };
            rt.class_literal(&raw)
        }
        "equals" => V::I(
            (a(1).r() != 0
                && rt.class_of(a(1)) != "java/lang/Class"
                && rt.instance_of(a(1), TOKEN)
                && crate::jtypes::signature(rt, a(1)) == own(rt)) as i32,
        ),
        "hashCode" => V::I(crate::jlang::string_hash(&own(rt))),
        "toString" => {
            let text = crate::jtypes::type_name(&own(rt));
            rt.string(&text)
        }
        _ => return None,
    })
}

// ---- Streams ----
//
// `JsonWriter` and `JsonReader` are Gson's own code: the translator takes them from the player's
// install as it takes a library's (a mod's writer over a file, a reader of its own class). What
// Gson itself does with one, walking a tree into a writer or building a tree from a reader, is
// done here through the stream's own methods, in Gson's order.

const READER: &str = "com/google/gson/stream/JsonReader";
const IO_ERROR: &str = "com/google/gson/JsonIOException";
const W_BEGIN_ARRAY: &str =
    "com/google/gson/stream/JsonWriter.beginArray.()Lcom/google/gson/stream/JsonWriter;";
const W_END_ARRAY: &str =
    "com/google/gson/stream/JsonWriter.endArray.()Lcom/google/gson/stream/JsonWriter;";
const W_BEGIN_OBJECT: &str =
    "com/google/gson/stream/JsonWriter.beginObject.()Lcom/google/gson/stream/JsonWriter;";
const W_END_OBJECT: &str =
    "com/google/gson/stream/JsonWriter.endObject.()Lcom/google/gson/stream/JsonWriter;";
const W_NAME: &str = "com/google/gson/stream/JsonWriter.name.(Ljava/lang/String;)Lcom/google/gson/stream/JsonWriter;";
const W_STRING: &str = "com/google/gson/stream/JsonWriter.value.(Ljava/lang/String;)Lcom/google/gson/stream/JsonWriter;";
const W_NUMBER: &str = "com/google/gson/stream/JsonWriter.value.(Ljava/lang/Number;)Lcom/google/gson/stream/JsonWriter;";
const W_BOOLEAN: &str =
    "com/google/gson/stream/JsonWriter.value.(Z)Lcom/google/gson/stream/JsonWriter;";
const W_NULL: &str =
    "com/google/gson/stream/JsonWriter.nullValue.()Lcom/google/gson/stream/JsonWriter;";
/// A writer's three settings `Gson` sets for its own writing: each one's getter and setter.
const W_SETTINGS: [(&str, &str); 3] = [
    (
        "com/google/gson/stream/JsonWriter.isLenient.()Z",
        "com/google/gson/stream/JsonWriter.setLenient.(Z)V",
    ),
    (
        "com/google/gson/stream/JsonWriter.isHtmlSafe.()Z",
        "com/google/gson/stream/JsonWriter.setHtmlSafe.(Z)V",
    ),
    (
        "com/google/gson/stream/JsonWriter.getSerializeNulls.()Z",
        "com/google/gson/stream/JsonWriter.setSerializeNulls.(Z)V",
    ),
];
const R_PEEK: &str = "com/google/gson/stream/JsonReader.peek.()Lcom/google/gson/stream/JsonToken;";
const R_HAS_NEXT: &str = "com/google/gson/stream/JsonReader.hasNext.()Z";
const R_IS_LENIENT: &str = "com/google/gson/stream/JsonReader.isLenient.()Z";
const R_SET_LENIENT: &str = "com/google/gson/stream/JsonReader.setLenient.(Z)V";

/// `TypeAdapters.JSON_ELEMENT.write`: the tree at `v` into writer `w`, which is told each value as
/// Gson tells it. `GsonHelper.writeValue` is the same with an object's names in `sorter`'s order.
/// Stops where the writer throws.
fn write_to(rt: &mut Rt, w: V, v: V, sorter: Option<V>) {
    let class = if v.r() == 0 { NULL } else { rt.class_of(v) };
    match class {
        PRIMITIVE => {
            let value = rt.get_field(v, "value");
            match kind(rt, v) {
                NUMBER => {
                    let n = crate::jlang::box_raw(rt, value);
                    rt.call_functional(W_NUMBER, w, &[n]);
                }
                BOOLEAN => {
                    rt.call_functional(W_BOOLEAN, w, &[V::I(value.i())]);
                }
                _ => {
                    rt.call_functional(W_STRING, w, &[value]);
                }
            }
        }
        ARRAY => {
            rt.call_functional(W_BEGIN_ARRAY, w, &[]);
            for e in items(rt, v) {
                if rt.thrown.is_some() {
                    return;
                }
                write_to(rt, w, e, sorter);
            }
            if rt.thrown.is_none() {
                rt.call_functional(W_END_ARRAY, w, &[]);
            }
        }
        OBJECT => {
            rt.call_functional(W_BEGIN_OBJECT, w, &[]);
            let mut all = members(rt, v);
            if let Some(order) = sorter {
                all.sort_by(|x, y| crate::jutil::compare(rt, order, x.0, y.0).cmp(&0));
            }
            for (name, e) in all {
                if rt.thrown.is_some() {
                    return;
                }
                rt.call_functional(W_NAME, w, &[name]);
                if rt.thrown.is_some() {
                    return;
                }
                write_to(rt, w, e, sorter);
            }
            if rt.thrown.is_none() {
                rt.call_functional(W_END_OBJECT, w, &[]);
            }
        }
        _ => {
            rt.call_functional(W_NULL, w, &[]);
        }
    }
}

/// `Gson.toJson(JsonElement, JsonWriter)`: the tree into the writer, which is lenient, escapes for
/// HTML and writes nulls as this `Gson` does for as long as that takes, then is as it was. What
/// the writer throws of `IOException` leaves as a `JsonIOException` of it.
fn to_writer(rt: &mut Rt, gson: V, w: V, tree: V) {
    let style = style_of(rt, gson);
    let wanted = [true, style.html, style.nulls];
    let mut before = [V::I(0); 3];
    for (i, (get, set)) in W_SETTINGS.iter().enumerate() {
        before[i] = rt.call_functional(get, w, &[]);
        rt.call_functional(set, w, &[V::I(wanted[i] as i32)]);
    }
    if rt.thrown.is_none() {
        write_to(rt, w, tree, None);
    }
    let thrown = rt.thrown.take();
    for (i, (_, set)) in W_SETTINGS.iter().enumerate() {
        rt.call_functional(set, w, &[before[i]]);
    }
    if let Some(e) = thrown {
        rt.thrown = Some(e);
        if rt.instance_of(e, "java/io/IOException") {
            wrap_thrown(rt, IO_ERROR);
        }
    }
}

/// `new X(cause)` thrown in place of what is being thrown, which becomes its cause: the message
/// is the cause's `toString()`, as `Throwable(Throwable)` makes it.
fn wrap_thrown(rt: &mut Rt, class: &'static str) {
    let Some(cause) = rt.thrown.take() else {
        return;
    };
    let text = crate::api::call(
        rt,
        "java/lang/Throwable.toString.()Ljava/lang/String;",
        &[cause],
    );
    let e = rt.new_object(class);
    rt.put_field(e, "message", text);
    rt.put_field(e, "cause", cause);
    rt.thrown = Some(e);
}

/// The token a reader is at (`peek()`), by its constant's name; None when the reader threw.
fn peek(rt: &mut Rt, r: V) -> Option<String> {
    let token = rt.call_functional(R_PEEK, r, &[]);
    if rt.thrown.is_some() {
        return None;
    }
    Some(
        rt.str_of(rt.get_field(token, "$name"))
            .unwrap_or("")
            .to_owned(),
    )
}

/// A call of the reader's that returns nothing a tree is made of; whether it came back.
fn step(rt: &mut Rt, r: V, key: &'static str) -> Option<V> {
    let v = rt.call_functional(key, r, &[]);
    rt.thrown.is_none().then_some(v)
}

/// `TypeAdapters.JSON_ELEMENT.read`: the tree of the value reader `r` is at, read through the
/// reader's own methods. A number is what its text says, as the parser reads one. None when the
/// reader threw.
fn read_from(rt: &mut Rt, r: V) -> Option<V> {
    match peek(rt, r)?.as_str() {
        "BEGIN_ARRAY" => {
            step(rt, r, "com/google/gson/stream/JsonReader.beginArray.()V")?;
            let mut all = Vec::new();
            while step(rt, r, R_HAS_NEXT)?.i() != 0 {
                all.push(read_from(rt, r)?);
            }
            step(rt, r, "com/google/gson/stream/JsonReader.endArray.()V")?;
            Some(new_array(rt, all))
        }
        "BEGIN_OBJECT" => {
            step(rt, r, "com/google/gson/stream/JsonReader.beginObject.()V")?;
            let o = new_object(rt);
            while step(rt, r, R_HAS_NEXT)?.i() != 0 {
                let name = step(
                    rt,
                    r,
                    "com/google/gson/stream/JsonReader.nextName.()Ljava/lang/String;",
                )?;
                let v = read_from(rt, r)?;
                put(rt, o, name, v);
            }
            step(rt, r, "com/google/gson/stream/JsonReader.endObject.()V")?;
            Some(o)
        }
        "STRING" => {
            let text = step(
                rt,
                r,
                "com/google/gson/stream/JsonReader.nextString.()Ljava/lang/String;",
            )?;
            let text = rt.str_of(text).unwrap_or("").to_owned();
            Some(string(rt, &text))
        }
        "NUMBER" => {
            let text = step(
                rt,
                r,
                "com/google/gson/stream/JsonReader.nextString.()Ljava/lang/String;",
            )?;
            let text = rt.str_of(text).unwrap_or("").to_owned();
            let n = json_number(&text)
                .or_else(|| text.parse().ok().map(V::D))
                .unwrap_or(V::D(0.0));
            Some(parsed_number(rt, n, &text))
        }
        "BOOLEAN" => {
            let b = step(rt, r, "com/google/gson/stream/JsonReader.nextBoolean.()Z")?;
            Some(boolean(rt, b.i() != 0))
        }
        "NULL" => {
            step(rt, r, "com/google/gson/stream/JsonReader.nextNull.()V")?;
            Some(null(rt))
        }
        other => {
            let msg = format!("Unexpected token: {other}");
            throw(rt, "java/lang/IllegalStateException", &msg);
            None
        }
    }
}

/// What a reader holds, read with the reader lenient for as long as that takes (`Gson.fromJson`
/// and `JsonParser.parseReader` both set it so, and put it back): the tree, nothing for a document
/// with nothing in it, or the exception the reader threw (left in `rt.thrown`, with whether the
/// document had begun).
fn read_all(rt: &mut Rt, r: V) -> Result<Option<V>, bool> {
    let lenient = rt.call_functional(R_IS_LENIENT, r, &[]);
    rt.call_functional(R_SET_LENIENT, r, &[V::I(1)]);
    let mut begun = false;
    let tree = match peek(rt, r) {
        Some(_) => {
            begun = true;
            read_from(rt, r)
        }
        None => None,
    };
    let thrown = rt.thrown.take();
    rt.call_functional(R_SET_LENIENT, r, &[lenient]);
    match thrown {
        None => Ok(tree),
        Some(e) if !begun && rt.instance_of(e, "java/io/EOFException") => Ok(None),
        Some(e) => {
            rt.thrown = Some(e);
            Err(begun)
        }
    }
}

/// `Gson.fromJson(JsonReader, Type)`'s tree: none for an empty document; an `IOException` or an
/// `IllegalStateException` of the reader's leaves as a `JsonSyntaxException` of it (the outer
/// None).
fn tree_from_reader(rt: &mut Rt, r: V) -> Option<Option<V>> {
    match read_all(rt, r) {
        Ok(tree) => Some(tree),
        Err(_) => {
            if rt.thrown.is_some_and(|e| {
                rt.instance_of(e, "java/io/IOException")
                    || rt.instance_of(e, "java/lang/IllegalStateException")
            }) {
                wrap_thrown(rt, SYNTAX);
            }
            None
        }
    }
}

/// `JsonParser.parseReader(JsonReader)` (`Streams.parse`): `JsonNull` for an empty document; a
/// malformed one, one that ends early or a number that is none is a `JsonSyntaxException`, any
/// other `IOException` a `JsonIOException`.
fn parse_reader(rt: &mut Rt, r: V) -> V {
    match read_all(rt, r) {
        Ok(Some(tree)) => tree,
        Ok(None) => null(rt),
        Err(_) => {
            let Some(e) = rt.thrown else { return V::NULL };
            let is = |rt: &Rt, class: &str| rt.instance_of(e, class);
            if is(rt, "java/io/EOFException")
                || is(rt, "com/google/gson/stream/MalformedJsonException")
                || is(rt, "java/lang/NumberFormatException")
            {
                wrap_thrown(rt, SYNTAX);
            } else if is(rt, "java/io/IOException") {
                wrap_thrown(rt, IO_ERROR);
            }
            V::NULL
        }
    }
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
            // How a field is named in JSON: one of Gson's policies, or a strategy of the mod's.
            "setFieldNamingPolicy" | "setFieldNamingStrategy" => {
                let policy = rt.class_of(a(1)) == NAMING_POLICY;
                let named = if policy {
                    crate::api_common::enum_name(rt, a(1), false)
                } else {
                    V::NULL
                };
                rt.put_field(this, "$naming", named);
                rt.put_field(this, "$namer", if policy { V::NULL } else { a(1) });
                this
            }
            // The modifiers that keep a field out, in place of `transient` and `static`.
            "excludeFieldsWithModifiers" => {
                let mask = crate::jutil::elements(rt, a(1))
                    .iter()
                    .fold(0, |m, v| m | v.i());
                rt.put_field(this, "$modifiers", V::I(mask));
                this
            }
            "setExclusionStrategies" => {
                for strategy in crate::jutil::elements(rt, a(1)) {
                    for list in ["$skipWriting", "$skipReading"] {
                        add_entry(rt, this, list, vec![strategy]);
                    }
                }
                this
            }
            "addSerializationExclusionStrategy" | "addDeserializationExclusionStrategy" => {
                let list = if name == "addSerializationExclusionStrategy" {
                    "$skipWriting"
                } else {
                    "$skipReading"
                };
                add_entry(rt, this, list, vec![a(1)]);
                this
            }
            "create" => {
                let g = rt.new_object(GSON);
                copy_settings(rt, this, g);
                g
            }
            "registerTypeAdapter" | "registerTypeHierarchyAdapter" => {
                let sig = crate::jtypes::signature(rt, a(1));
                register(rt, this, &sig, a(2), name == "registerTypeHierarchyAdapter");
                this
            }
            "registerTypeAdapterFactory" => {
                add_adapter(rt, this, "", a(1), MADE_BY, 0);
                this
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
        // A tree into a `StringBuilder`, or into a writer (a file's, a `StringWriter`).
        ("toJson", "(Lcom/google/gson/JsonElement;Ljava/lang/Appendable;)V" | "(Ljava/lang/Object;Ljava/lang/Appendable;)V")
            if a(1).r() == 0 || is_element_class(rt.class_of(a(1))) =>
        {
            let tree = if a(1).r() == 0 { null(rt) } else { a(1) };
            let s = write(rt, tree, style);
            let s = rt.string(&s);
            let append = if matches!(
                rt.class_of(a(2)),
                "java/lang/StringBuilder" | "java/lang/StringBuffer"
            ) {
                "java/lang/StringBuilder.append.(Ljava/lang/String;)Ljava/lang/StringBuilder;"
            } else {
                "java/io/Writer.write.(Ljava/lang/String;)V"
            };
            crate::api::call(rt, append, &[a(2), s]);
            V::Void
        }
        // Into a `JsonWriter` of the mod's: through the writer's own methods.
        ("toJson", "(Lcom/google/gson/JsonElement;Lcom/google/gson/stream/JsonWriter;)V") => {
            let tree = if a(1).r() == 0 { null(rt) } else { a(1) };
            to_writer(rt, this, a(2), tree);
            V::Void
        }
        (
            "toJson",
            "(Ljava/lang/Object;Ljava/lang/reflect/Type;Lcom/google/gson/stream/JsonWriter;)V",
        ) => {
            let declared = Some(crate::jtypes::signature(rt, a(2)));
            let tree = tree_of(rt, this, a(1), declared, true);
            to_writer(rt, this, a(3), tree);
            V::Void
        }
        // A writer or a reader set as this `Gson` writes and reads: Gson's own class, made by its
        // constructor.
        ("newJsonWriter", _) => {
            let w = rt.reflect(
                "com/google/gson/stream/JsonWriter",
                "<init>.(Ljava/io/Writer;)V",
                &[a(1)],
            )?;
            if style.pretty {
                let indent = rt.string("  ");
                rt.call_functional(
                    "com/google/gson/stream/JsonWriter.setIndent.(Ljava/lang/String;)V",
                    w,
                    &[indent],
                );
            }
            let lenient = rt.get_field(this, "$lenient") == V::I(1);
            for ((_, set), on) in W_SETTINGS.iter().zip([lenient, style.html, style.nulls]) {
                rt.call_functional(set, w, &[V::I(on as i32)]);
            }
            w
        }
        ("newJsonReader", _) => {
            let r = rt.reflect(READER, "<init>.(Ljava/io/Reader;)V", &[a(1)])?;
            let lenient = rt.get_field(this, "$lenient") == V::I(1);
            rt.call_functional(R_SET_LENIENT, r, &[V::I(lenient as i32)]);
            r
        }
        ("toJsonTree", _) if is_element_class(rt.class_of(a(1))) => deep_copy(rt, a(1)),
        // An object of the mod's own, or a collection of them: field by field ([`to_tree_in`]),
        // or as an adapter registered with this `Gson` writes it.
        ("toJsonTree", _) => {
            let declared = (args.len() > 2).then(|| crate::jtypes::signature(rt, a(2)));
            tree_of(rt, this, a(1), declared, style.nulls)
        }
        // The adapter for a type: the mod's own where it registered one for exactly that type,
        // else one of the runtime's that writes and reads as this `Gson` does.
        ("getAdapter", _) => {
            let sig = crate::jtypes::signature(rt, a(1));
            let registered = chain(rt, this).into_iter().find(|e| {
                e.mode == EXACT && e.sig == sig && rt.instance_of(e.adapter, TYPE_ADAPTER)
            });
            match registered {
                Some(e) => e.adapter,
                None => own_adapter(rt, this, &sig, 0),
            }
        }
        // The adapter that would answer were `skipPast` not registered: the ones after it. One
        // that isn't registered leaves only Gson's own.
        ("getDelegateAdapter", _) => {
            let sig = crate::jtypes::signature(rt, a(2));
            let all = chain(rt, this);
            let from = all
                .iter()
                .position(|e| e.adapter == a(1))
                .map_or(all.len(), |i| i + 1);
            own_adapter(rt, this, &sig, from)
        }
        ("newBuilder", _) => {
            let b = rt.new_object(BUILDER);
            copy_settings(rt, this, b);
            b
        }
        (
            "toJson",
            "(Ljava/lang/Object;)Ljava/lang/String;"
            | "(Ljava/lang/Object;Ljava/lang/reflect/Type;)Ljava/lang/String;",
        ) => {
            let declared = (args.len() > 2).then(|| crate::jtypes::signature(rt, a(2)));
            let tree = tree_of(rt, this, a(1), declared, style.nulls);
            let s = write(rt, tree, style);
            rt.string(&s)
        }
        (
            "toJson",
            "(Ljava/lang/Object;Ljava/lang/Appendable;)V"
            | "(Ljava/lang/Object;Ljava/lang/reflect/Type;Ljava/lang/Appendable;)V",
        ) => {
            let declared = (args.len() > 3).then(|| crate::jtypes::signature(rt, a(2)));
            let tree = tree_of(rt, this, a(1), declared, style.nulls);
            let s = write(rt, tree, style);
            let (s, out) = (rt.string(&s), a(args.len() - 1));
            let append = if matches!(
                rt.class_of(out),
                "java/lang/StringBuilder" | "java/lang/StringBuffer"
            ) {
                "java/lang/StringBuilder.append.(Ljava/lang/String;)Ljava/lang/StringBuilder;"
            } else {
                "java/io/Writer.write.(Ljava/lang/String;)V"
            };
            crate::api::call(rt, append, &[out, s]);
            V::Void
        }
        ("fromJson", _) => {
            // The type asked for: a class, a type with its arguments (a `TypeToken`'s), nothing.
            let ty = crate::jtypes::signature(rt, a(2));
            let class = crate::jtypes::parts(&ty).0;
            if !tree_class(&class) {
                // Into a class of the mod's own (or a map, a list): the tree, read by the class
                // ([`from_tree_in`]). A `Type` that is no class is read as its tree's own shape.
                let tree = if is_element_class(rt.class_of(a(1))) {
                    a(1)
                } else if a(1).r() == 0 {
                    return Some(V::NULL);
                } else if rt.instance_of(a(1), READER) {
                    match tree_from_reader(rt, a(1)) {
                        Some(Some(tree)) => tree,
                        _ => return Some(V::NULL),
                    }
                } else {
                    let text = source_text(rt, a(1))?;
                    // `Gson.fromJson` reads leniently whatever the builder said (Gson 2.10's
                    // `fromJson(JsonReader, Type)` sets its reader lenient): a mod's config file
                    // with comments in it is read.
                    match parse(rt, &text, true) {
                        Err(e) => return Some(throw(rt, SYNTAX, &e)),
                        Ok(None) => return Some(V::NULL),
                        Ok(Some((v, _))) => v,
                    }
                };
                let read = from_tree_in(rt, this, tree, &ty, 0, 0);
                // What is read as a primitive's type comes back in its box, an object as any
                // (`(Boolean) gson.fromJson(reader, boolean.class)`).
                return Some(match ty.as_bytes() {
                    &[kind] => crate::mixin::box_value(rt, read, char::from(kind)),
                    _ => read,
                });
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
            if rt.instance_of(a(1), READER) {
                return Some(match tree_from_reader(rt, a(1)) {
                    Some(Some(v)) if class != ELEMENT && rt.class_of(v) != class => {
                        let msg = format!(
                            "Expected a {} but was {}; at path $",
                            class.replace('/', "."),
                            java_name(rt, v)
                        );
                        throw(rt, SYNTAX, &msg)
                    }
                    Some(Some(v)) => v,
                    _ => V::NULL,
                });
            }
            let text = source_text(rt, a(1))?;
            read_as(rt, &text, &class, true).unwrap_or(V::NULL)
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
                return Some(parse_reader(rt, source));
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
        // `writeValue(writer, element, sorter)`: into a `JsonWriter` of the mod's.
        "writeValue" => {
            write_to(rt, a(0), a(1), Some(a(2)).filter(|c| c.r() != 0));
            V::Void
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

/// How deep an object's fields are followed before it is taken for one that holds itself.
const DEPTH: usize = 32;
const ANY: &str = "Ljava/lang/Object;";
const SERIALIZED_NAME: &str = "com/google/gson/annotations/SerializedName";

/// The constants of enum `class` (or of the enum a constant's own class extends) that carry
/// `@SerializedName`: (the constant, its name in JSON, the alternates it is read by). Gson's
/// adapter reads the annotation off each constant's field; found once per class.
fn enum_names(rt: &mut Rt, class: &str) -> std::sync::Arc<Vec<(String, String, Vec<String>)>> {
    type Names = std::sync::Arc<Vec<(String, String, Vec<String>)>>;
    static FOUND: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Names>>> =
        std::sync::OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    if let Some(names) = found.lock().ok().and_then(|f| f.get(class).cloned()) {
        return names;
    }
    let constants = crate::jlang::enum_constants(rt, class)
        .map(|c| crate::jutil::elements(rt, c))
        .unwrap_or_default();
    let mut names = Vec::new();
    for c in constants {
        let name = crate::api::call(rt, "java/lang/Enum.name.()Ljava/lang/String;", &[c]);
        let name = rt.str_of(name).unwrap_or("").to_owned();
        let Some(named) = crate::annotations::on_field(rt, class, &name)
            .into_iter()
            .find(|&a| rt.class_of(a) == SERIALIZED_NAME)
        else {
            continue;
        };
        let value = crate::annotations::element(
            rt,
            "com/google/gson/annotations/SerializedName.value.()Ljava/lang/String;",
            &[named],
        )
        .and_then(|v| rt.str_of(v).map(str::to_owned));
        let alternates = crate::annotations::element(
            rt,
            "com/google/gson/annotations/SerializedName.alternate.()[Ljava/lang/String;",
            &[named],
        )
        .map(|all| {
            crate::jutil::elements(rt, all)
                .into_iter()
                .filter_map(|a| rt.str_of(a).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
        names.push((name.clone(), value.unwrap_or(name), alternates));
    }
    let names: Names = std::sync::Arc::new(names);
    if let Ok(mut f) = found.lock() {
        f.insert(class.to_owned(), names.clone());
    }
    names
}

/// A type as a field's descriptor or generic signature has it, apart: the class's own descriptor
/// (`Ljava/util/List;`) and its type arguments (`Lcom/example/Entry;`), a wildcard or a type
/// variable as `Object`.
fn generic(ty: &str) -> (String, Vec<String>) {
    let Some(open) = ty.find('<') else {
        return (
            if ty.starts_with('T') {
                ANY.to_owned()
            } else {
                ty.to_owned()
            },
            Vec::new(),
        );
    };
    let (mut args, mut depth, mut at) = (Vec::new(), 0usize, open + 1);
    let b = ty.as_bytes();
    let mut i = open + 1;
    while i < b.len() {
        match b[i] {
            b'<' => depth += 1,
            b'>' if depth == 0 => break,
            b'>' => depth -= 1,
            b'*' if depth == 0 && at == i => {
                args.push(ANY.to_owned());
                at = i + 1;
            }
            b';' if depth == 0 => {
                let arg = ty[at..=i].trim_start_matches(['+', '-']);
                args.push(if arg.starts_with('T') {
                    ANY.to_owned()
                } else {
                    arg.to_owned()
                });
                at = i + 1;
            }
            // A primitive array's element has no `;` to end it.
            b'I' | b'J' | b'Z' | b'B' | b'C' | b'S' | b'F' | b'D'
                if depth == 0 && ty[at..i].bytes().all(|c| c == b'[') && at < i =>
            {
                args.push(ty[at..=i].to_owned());
                at = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    (format!("{};", &ty[..open]), args)
}

const NAMING_POLICY: &str = "com/google/gson/FieldNamingPolicy";
const FIELD_ATTRIBUTES: &str = "com/google/gson/FieldAttributes";

/// A field's name in JSON under one of Gson's naming policies (`FieldNamingPolicy`): the name's
/// words, which begin at its capitals, set apart or its first letter raised.
fn policy_name(policy: &str, name: &str) -> String {
    let apart = |separator: char| {
        let mut out = String::new();
        for c in name.chars() {
            if c.is_uppercase() && !out.is_empty() {
                out.push(separator);
            }
            out.push(c);
        }
        out
    };
    let raised = |s: String| match s.char_indices().find(|(_, c)| c.is_alphabetic()) {
        Some((i, c)) if !c.is_uppercase() => {
            format!("{}{}{}", &s[..i], c.to_uppercase(), &s[i + c.len_utf8()..])
        }
        _ => s,
    };
    match policy {
        "UPPER_CAMEL_CASE" => raised(name.to_owned()),
        "UPPER_CAMEL_CASE_WITH_SPACES" => raised(apart(' ')),
        "UPPER_CASE_WITH_UNDERSCORES" => apart('_').to_uppercase(),
        "LOWER_CASE_WITH_UNDERSCORES" => apart('_').to_lowercase(),
        "LOWER_CASE_WITH_DASHES" => apart('-').to_lowercase(),
        "LOWER_CASE_WITH_DOTS" => apart('.').to_lowercase(),
        _ => name.to_owned(),
    }
}

/// What an `ExclusionStrategy` is shown of a field: its class, its name, its type, its modifiers.
fn field_attributes(rt: &mut Rt, class: &str, name: &str, ty: &str, access: u16) -> V {
    let f = rt.new_object(FIELD_ATTRIBUTES);
    for (field, text) in [("$class", class), ("$name", name), ("$type", ty)] {
        let v = rt.string(text);
        rt.put_field(f, field, v);
    }
    rt.put_field(f, "$access", V::I(i32::from(access)));
    f
}

fn field_attributes_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let held = |rt: &Rt, field: &'static str| {
        rt.str_of(rt.get_field(this, field))
            .unwrap_or("")
            .to_owned()
    };
    Some(match name {
        "getName" => rt.get_field(this, "$name"),
        "getDeclaringClass" => {
            let class = held(rt, "$class");
            rt.class_literal(&class)
        }
        "getDeclaredType" => {
            let ty = held(rt, "$type");
            crate::jtypes::of(rt, &ty)
        }
        "getDeclaredClass" => {
            let raw = generic(&held(rt, "$type")).0;
            rt.class_literal(crate::jlang::descriptor_class(&raw))
        }
        "hasModifier" => V::I((rt.get_field(this, "$access").i() & a(1).i() != 0) as i32),
        "getAnnotation" => {
            let (class, field, wanted) = (
                held(rt, "$class"),
                held(rt, "$name"),
                rt.str_of(a(1)).unwrap_or("").to_owned(),
            );
            crate::annotations::on_field(rt, &class, &field)
                .into_iter()
                .find(|&n| rt.class_of(n) == wanted)
                .unwrap_or(V::NULL)
        }
        "getAnnotations" => {
            let (class, field) = (held(rt, "$class"), held(rt, "$name"));
            let all = crate::annotations::on_field(rt, &class, &field);
            crate::jutil::new_list(rt, all)
        }
        _ => return None,
    })
}

/// The fields `gson` reads (or, `writing`, writes) of an object of mod class `class`: its own and
/// its superclasses', less the compiler's, those with a modifier that keeps a field out
/// (`transient` and `static`, unless the builder named others) and those an `ExclusionStrategy`
/// of the Gson's skips, by the field or by its class. Each by its name, its name in JSON
/// (`@SerializedName`, else as the Gson's naming policy or strategy has it, else the same) and
/// its type ([`generic`]'s input).
fn bound_fields(
    rt: &mut Rt,
    gson: V,
    class: &str,
    writing: bool,
) -> Vec<(&'static str, String, String)> {
    let mut out: Vec<(&'static str, String, String)> = Vec::new();
    let mut c = class.to_owned();
    let excluded = match rt.get_field(gson, "$modifiers") {
        V::I(mask) if gson.r() != 0 => mask as u16,
        _ => 0x88,
    };
    let policy = rt.str_of(rt.get_field(gson, "$naming")).map(str::to_owned);
    let namer = rt.get_field(gson, "$namer");
    let skips = match rt.get_field(
        gson,
        if writing {
            "$skipWriting"
        } else {
            "$skipReading"
        },
    ) {
        list if gson.r() != 0 && list.r() != 0 => crate::jutil::elements(rt, list)
            .into_iter()
            .flat_map(|e| crate::jutil::elements(rt, e))
            .collect(),
        _ => Vec::new(),
    };
    // The fields' own names, a class's before those above it: of two of one name, the lower.
    let mut seen: Vec<String> = Vec::new();
    while rt.is_mod_class(&c) {
        for (access, name, desc, signature) in
            crate::jlang::typed_fields(rt, &c).unwrap_or_default()
        {
            if access & 0x1000 != 0 || access & excluded != 0 || seen.contains(&name) {
                continue;
            }
            let ty = signature.unwrap_or(desc);
            if !skips.is_empty() {
                let shown = field_attributes(rt, &c, &name, &ty, access);
                let of = rt.class_literal(crate::jlang::descriptor_class(&generic(&ty).0));
                let skipped = skips.iter().any(|&strategy| {
                    rt.call_functional("com/google/gson/ExclusionStrategy.shouldSkipClass.(Ljava/lang/Class;)Z", strategy, &[of]).i() != 0
                        || rt.call_functional("com/google/gson/ExclusionStrategy.shouldSkipField.(Lcom/google/gson/FieldAttributes;)Z", strategy, &[shown]).i() != 0
                });
                if skipped {
                    continue;
                }
            }
            let named = crate::annotations::on_field(rt, &c, &name)
                .into_iter()
                .find(|&a| rt.class_of(a) == SERIALIZED_NAME);
            let json = named
                .and_then(|a| {
                    crate::annotations::element(
                        rt,
                        "com/google/gson/annotations/SerializedName.value.()Ljava/lang/String;",
                        &[a],
                    )
                })
                .and_then(|v| rt.str_of(v).map(str::to_owned));
            let json = match json {
                Some(json) => json,
                None if namer.r() != 0 && gson.r() != 0 => {
                    let (holder, field_name) = (rt.class_literal(&c), rt.string(&name));
                    let field = crate::api::call(
                        rt,
                        "java/lang/Class.getDeclaredField.(Ljava/lang/String;)Ljava/lang/reflect/Field;",
                        &[holder, field_name],
                    );
                    let translated = rt.call_functional("com/google/gson/FieldNamingStrategy.translateName.(Ljava/lang/reflect/Field;)Ljava/lang/String;", namer, &[field]);
                    rt.str_of(translated)
                        .map_or_else(|| name.clone(), str::to_owned)
                }
                None => policy
                    .as_deref()
                    .map_or_else(|| name.clone(), |p| policy_name(p, &name)),
            };
            // (Where the object keeps it: a field that hides one above it has a name of its own.)
            let plain = rt.static_key(&name);
            out.push((crate::jlang::stored_name(rt, &c, plain), json, ty));
            seen.push(name);
        }
        match rt.supers.get(c.as_str()) {
            Some(&sup) => c = sup.to_owned(),
            None => break,
        }
    }
    out
}

/// A Java value as the tree Gson writes for it, by its type `ty` (a descriptor or a generic
/// signature): a primitive or its box, a string, an enum's constant by name, an array or a
/// collection element by element, a map by its keys' text, a tree as itself, and an object of a
/// mod's own class field by field ([`bound_fields`]; one left null is left out unless `nulls`).
///
/// As `gson` writes it: one of its adapters, from the one at `from` on, answers first
/// ([`adapter_for`]), for the value's own class where the type says no more than a class, then
/// for the type. What a value holds is written by all of them.
fn to_tree_in(rt: &mut Rt, gson: V, v: V, ty: &str, nulls: bool, depth: usize, from: usize) -> V {
    // A primitive's type: the value raw, or in its box (`toJson(field.get(o), boolean.class)`).
    if let &[kind @ (b'Z' | b'C' | b'I' | b'S' | b'B' | b'J' | b'F' | b'D')] = ty.as_bytes() {
        let raw = crate::mixin::unbox(rt, v, char::from(kind));
        return match kind {
            b'Z' => boolean(rt, raw.i() != 0),
            b'C' => {
                let c = char::from_u32(raw.i() as u32)
                    .map(String::from)
                    .unwrap_or_default();
                string(rt, &c)
            }
            _ => number(rt, raw),
        };
    }
    // A number a collection holds raw.
    if matches!(v, V::I(_) | V::J(_) | V::F(_) | V::D(_)) {
        return number(rt, v);
    }
    if depth > DEPTH {
        return null(rt);
    }
    if v.r() != 0 && is_element_class(rt.class_of(v)) {
        return v;
    }
    if has_adapters(rt, gson) && ty.starts_with(['L', '[']) {
        let mut asked: Vec<String> = Vec::new();
        if v.r() != 0 && !ty.contains('<') && from == 0 {
            asked.push(format!("L{};", rt.class_of(v)));
        }
        if ty != ANY && !asked.iter().any(|a| a == ty) {
            asked.push(ty.to_owned());
        }
        for sig in asked {
            if let Some(adapter) = adapter_for(rt, gson, &sig, from, true) {
                return write_with(rt, gson, adapter, v, &sig);
            }
        }
    }
    if v.r() == 0 {
        return null(rt);
    }
    let class = rt.class_of(v);
    if let Some(text) = rt.str_of(v).map(str::to_owned) {
        return string(rt, &text);
    }
    match class {
        "java/lang/Integer" | "java/lang/Long" | "java/lang/Short" | "java/lang/Byte"
        | "java/lang/Float" | "java/lang/Double" => {
            let n = rt.get_field(v, "value");
            return number(rt, n);
        }
        "java/lang/Boolean" => return boolean(rt, rt.get_field(v, "value").i() != 0),
        "java/lang/Character" => {
            let c = char::from_u32(rt.get_field(v, "value").i() as u32)
                .map(String::from)
                .unwrap_or_default();
            return string(rt, &c);
        }
        _ => {}
    }
    if rt.is_a(class, "java/lang/Enum") {
        let name = crate::api::call(rt, "java/lang/Enum.name.()Ljava/lang/String;", &[v]);
        let name = rt.str_of(name).unwrap_or("").to_owned();
        let names = enum_names(rt, class);
        let written = names
            .iter()
            .find(|n| n.0 == name)
            .map_or(name.as_str(), |n| n.1.as_str());
        return string(rt, written);
    }
    let (_, args) = generic(ty);
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| ANY.to_owned());
    let listed = match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::List(_) | Data::Array(_)) => Some(true),
        Some(Data::Table(_)) => Some(class.ends_with("Set")),
        _ => None,
    };
    match listed {
        Some(true) => {
            let of = ty.strip_prefix('[').map_or_else(|| arg(0), str::to_owned);
            let mut items = Vec::new();
            for e in crate::jutil::elements(rt, v) {
                items.push(to_tree_in(rt, gson, e, &of, nulls, depth + 1, 0));
            }
            new_array(rt, items)
        }
        Some(false) => {
            let (o, of) = (new_object(rt), arg(1));
            for (k, e) in crate::jutil::table_pairs(rt, v) {
                // A key is its text: a string, a number, an enum's constant by name.
                let key = to_tree_in(rt, gson, k, ANY, false, depth + 1, 0);
                let key = if rt.class_of(key) == PRIMITIVE {
                    primitive_as(rt, key, "getAsString")
                } else {
                    rt.string(&rt.to_java_string(k, 'L'))
                };
                let e = to_tree_in(rt, gson, e, &of, nulls, depth + 1, 0);
                put(rt, o, key, e);
            }
            o
        }
        None if rt.is_mod_class(class) => {
            let o = new_object(rt);
            for (field, json, of) in bound_fields(rt, gson, class, true) {
                let value = rt.get_field(v, field);
                if value.r() == 0 && !nulls && of.len() > 1 {
                    continue;
                }
                let (key, e) = (
                    rt.string(&json),
                    to_tree_in(rt, gson, value, &of, nulls, depth + 1, 0),
                );
                put(rt, o, key, e);
            }
            o
        }
        // Anything else by its text, as a value with no fields to show.
        None => {
            let text = rt.to_java_string(v, 'L');
            string(rt, &text)
        }
    }
}

/// What Gson reads a tree as when the type says only `Object`: a string, a boolean, a number as
/// a `Double` (Gson's default for a number whose type nothing names: `1` is read back `1.0`), a
/// list, a map.
fn natural(rt: &mut Rt, gson: V, json: V, depth: usize) -> V {
    match rt.class_of(json) {
        PRIMITIVE => match kind(rt, json) {
            STRING => primitive_as(rt, json, "getAsString"),
            BOOLEAN => from_tree_in(rt, gson, json, "Ljava/lang/Boolean;", depth, 0),
            _ => from_tree_in(rt, gson, json, "Ljava/lang/Double;", depth, 0),
        },
        ARRAY => from_tree_in(rt, gson, json, "Ljava/util/List;", depth, 0),
        OBJECT => from_tree_in(rt, gson, json, "Ljava/util/Map;", depth, 0),
        _ => V::NULL,
    }
}

/// A tree as the Java value of type `ty` (a descriptor or a generic signature), as Gson reads it:
/// the reverse of [`to_tree_in`]. An object of a mod's own class is made by its constructor without
/// arguments, or with none run where it has none, and then each field the tree has a member for
/// is set; a field the tree doesn't name keeps what the constructor gave it.
///
/// As `gson` reads it: one of its adapters for the type, from the one at `from` on, reads first
/// ([`adapter_for`]); an object is made by the `InstanceCreator` registered for its type where
/// there is one.
fn from_tree_in(rt: &mut Rt, gson: V, json: V, ty: &str, depth: usize, from: usize) -> V {
    let (raw, args) = generic(ty);
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| ANY.to_owned());
    let absent = json.r() == 0 || rt.class_of(json) == NULL;
    let scalar = !absent && rt.class_of(json) == PRIMITIVE;
    let primitive = |rt: &mut Rt, kind: char| match kind {
        _ if !scalar => match kind {
            'J' => V::J(0),
            'F' => V::F(0.0),
            'D' => V::D(0.0),
            _ => V::I(0),
        },
        'J' => primitive_as(rt, json, "getAsLong"),
        'F' => primitive_as(rt, json, "getAsFloat"),
        'D' => primitive_as(rt, json, "getAsDouble"),
        'Z' => primitive_as(rt, json, "getAsBoolean"),
        'C' => {
            let s = primitive_as(rt, json, "getAsString");
            V::I(
                rt.str_of(s)
                    .and_then(|s| s.encode_utf16().next())
                    .map_or(0, i32::from),
            )
        }
        _ => primitive_as(rt, json, "getAsInt"),
    };
    if raw.len() == 1 {
        return primitive(rt, raw.chars().next().unwrap_or('I'));
    }
    if depth > DEPTH {
        return V::NULL;
    }
    // What the type says only `Object` of, and a tree, are Gson's own to read before any
    // adapter of the mod's.
    if has_adapters(rt, gson)
        && raw != ANY
        && !tree_class(raw.trim_start_matches('L').trim_end_matches(';'))
        && let Some(adapter) = adapter_for(rt, gson, ty, from, false)
    {
        return read_with(rt, gson, adapter, json, ty);
    }
    if absent {
        return V::NULL;
    }
    let boxed = match raw.as_str() {
        "Ljava/lang/Integer;" => Some('I'),
        "Ljava/lang/Long;" => Some('J'),
        "Ljava/lang/Short;" => Some('S'),
        "Ljava/lang/Byte;" => Some('B'),
        "Ljava/lang/Float;" => Some('F'),
        "Ljava/lang/Double;" | "Ljava/lang/Number;" => Some('D'),
        "Ljava/lang/Boolean;" => Some('Z'),
        "Ljava/lang/Character;" => Some('C'),
        _ => None,
    };
    if let Some(kind) = boxed {
        let v = primitive(rt, kind);
        return crate::mixin::box_value(rt, v, kind);
    }
    if let Some(of) = raw.strip_prefix('[') {
        let mut out = Vec::new();
        for e in items(rt, json) {
            out.push(from_tree_in(rt, gson, e, of, depth + 1, 0));
        }
        return rt.alloc("[", Data::Array(out));
    }
    let class = raw.trim_start_matches('L').trim_end_matches(';').to_owned();
    if raw == ANY {
        return natural(rt, gson, json, depth);
    }
    if class == "java/lang/String" || class == "java/lang/CharSequence" {
        return if scalar {
            primitive_as(rt, json, "getAsString")
        } else {
            V::NULL
        };
    }
    if tree_class(&class) {
        return json;
    }
    if rt.is_a(&class, "java/lang/Enum") {
        let name = if scalar {
            primitive_as(rt, json, "getAsString")
        } else {
            V::NULL
        };
        let name = rt.str_of(name).unwrap_or("").to_owned();
        let constants = crate::jlang::enum_constants(rt, &class)
            .map(|c| crate::jutil::elements(rt, c))
            .unwrap_or_default();
        // By the name Gson gives the constant (`@SerializedName`'s, or one of its alternates;
        // its own without one), else by what its `toString` says.
        let names = enum_names(rt, &class);
        let wanted = names
            .iter()
            .find(|n| n.1 == name || n.2.contains(&name))
            .map_or(name.as_str(), |n| n.0.as_str());
        for &c in &constants {
            let n = crate::api::call(rt, "java/lang/Enum.name.()Ljava/lang/String;", &[c]);
            if rt.str_of(n) == Some(wanted) {
                return c;
            }
        }
        for c in constants {
            let shown =
                rt.call_functional("java/lang/Object.toString.()Ljava/lang/String;", c, &[]);
            if rt.thrown.take().is_none() && rt.str_of(shown) == Some(name.as_str()) {
                return c;
            }
        }
        return V::NULL;
    }
    let simple = class.rsplit('/').next().unwrap_or("");
    if rt.class_of(json) == ARRAY && !rt.is_mod_class(&class) {
        let of = arg(0);
        let mut out = Vec::new();
        for e in items(rt, json) {
            out.push(from_tree_in(rt, gson, e, &of, depth + 1, 0));
        }
        return if simple.ends_with("Set") {
            crate::jutil::new_set_of(rt, &out)
        } else {
            crate::jutil::new_list(rt, out)
        };
    }
    if rt.class_of(json) != OBJECT {
        return V::NULL;
    }
    if !rt.is_mod_class(&class) {
        // A map: its keys read from the members' names by the key's type, in the tree's order.
        let (key_ty, of) = (arg(0), arg(1));
        let map = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
        for (k, e) in members(rt, json) {
            let key = if key_ty == ANY || key_ty == "Ljava/lang/String;" {
                k
            } else {
                let text = rt.str_of(k).unwrap_or("").to_owned();
                let named = string(rt, &text);
                let read = if key_ty.ends_with("Integer;")
                    || key_ty.ends_with("Long;")
                    || key_ty.ends_with("Double;")
                    || key_ty.ends_with("Float;")
                {
                    json_number(&text).map_or(named, |n| number(rt, n))
                } else {
                    named
                };
                from_tree_in(rt, gson, read, &key_ty, depth + 1, 0)
            };
            let e = from_tree_in(rt, gson, e, &of, depth + 1, 0);
            crate::jutil::table_put(rt, map, key, e);
        }
        return map;
    }
    let made = created(rt, gson, ty).or_else(|| rt.reflect(&class, "<init>.()V", &[])).or_else(|| {
        if rt.thrown.is_some() {
            None
        } else {
            rt.reflect(&class, "allocate", &[])
        }
    });
    let Some(object) = made else {
        return V::NULL;
    };
    for (field, name, of) in bound_fields(rt, gson, &class, false) {
        let key = rt.string(&name);
        let Some(member) = crate::jutil::table_get(rt, json, key) else {
            continue;
        };
        // A null leaves a primitive as it is and clears anything else.
        if of.len() == 1 && rt.class_of(member) != PRIMITIVE {
            continue;
        }
        let value = from_tree_in(rt, gson, member, &of, depth + 1, 0);
        rt.put_field(object, field, value);
    }
    object
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

    /// A config class of a mod's own, read from JSON and written back as Gson does both: each
    /// field by its type (a list's elements and a map's values by the field's generic signature),
    /// a static and a transient field left alone, a field the file lacks as the constructor made
    /// it, a nested class with no constructor without arguments made bare.
    #[test]
    fn gson_reads_and_writes_a_mods_own_objects() {
        fn reflect(rt: &mut Rt, class: &str, what: &str, _: &[V]) -> Option<V> {
            match (class, what) {
                ("t/Config", "fields") => Some(rt.string(
                    "1 speed I\n1 name Ljava/lang/String;\n1 ratio D\n1 on Z\n1 kept I\n1 tags Ljava/util/List; Ljava/util/List<Ljava/lang/String;>;\n\
                     1 entries Ljava/util/List; Ljava/util/List<Lt/Entry;>;\n1 limits Ljava/util/Map; Ljava/util/Map<Ljava/lang/String;Ljava/lang/Long;>;\n\
                     129 cache Ljava/lang/Object;\n9 SHARED I\n1 inner Lt/Entry;",
                )),
                ("t/Entry", "fields") => Some(rt.string("1 id Ljava/lang/String;\n1 weight I")),
                ("t/Config", "<init>.()V") => {
                    let o = rt.new_object("t/Config");
                    rt.put_field(o, "speed", V::I(8));
                    rt.put_field(o, "kept", V::I(11));
                    Some(o)
                }
                ("t/Entry", "allocate") => Some(rt.new_object("t/Entry")),
                _ => None,
            }
        }
        let mut rt = Rt::new();
        rt.supers.insert("t/Config", "java/lang/Object");
        rt.supers.insert("t/Entry", "java/lang/Object");
        rt.reflect = Some(reflect);
        let gson = rt.new_object(GSON);
        let file = text(
            &mut rt,
            r#"{"speed": 3, "name": "fast", "ratio": 0.5, "on": true, "tags": ["a", "b"], "entries": [{"id": "x", "weight": 2}, {"id": "y"}],
               "limits": {"a": 1, "b": 20}, "cache": 5, "SHARED": 9, "inner": null, "unknown": [1]}"#,
        );
        let class = rt.class_literal("t/Config");
        let config = call(
            &mut rt,
            "com/google/gson/Gson.fromJson.(Ljava/lang/String;Ljava/lang/Class;)Ljava/lang/Object;",
            &[gson, file, class],
        );
        assert_eq!(rt.class_of(config), "t/Config");
        assert_eq!(
            (
                rt.get_field(config, "speed"),
                rt.get_field(config, "kept"),
                rt.get_field(config, "ratio"),
                rt.get_field(config, "on")
            ),
            (V::I(3), V::I(11), V::D(0.5), V::I(1))
        );
        assert_eq!(rt.str_of(rt.get_field(config, "name")), Some("fast"));
        assert_eq!(
            rt.get_field(config, "cache").r(),
            0,
            "a transient field is not read"
        );
        let tags = crate::jutil::elements(&rt, rt.get_field(config, "tags"));
        assert_eq!(
            tags.iter()
                .map(|&t| rt.str_of(t).unwrap_or("?"))
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        let entries = crate::jutil::elements(&rt, rt.get_field(config, "entries"));
        assert_eq!(
            entries.iter().map(|&e| rt.class_of(e)).collect::<Vec<_>>(),
            ["t/Entry", "t/Entry"]
        );
        assert_eq!(
            (
                rt.str_of(rt.get_field(entries[0], "id")),
                rt.get_field(entries[0], "weight")
            ),
            (Some("x"), V::I(2))
        );
        let (limits, b) = (rt.get_field(config, "limits"), text(&mut rt, "b"));
        let twenty = crate::jutil::table_get(&mut rt, limits, b).unwrap();
        assert_eq!(
            (rt.class_of(twenty), rt.get_field(twenty, "value")),
            ("java/lang/Long", V::J(20))
        );
        // And back: the fields in their order, a null one and the ones Gson skips left out.
        let written = call(
            &mut rt,
            "com/google/gson/Gson.toJson.(Ljava/lang/Object;)Ljava/lang/String;",
            &[gson, config],
        );
        assert_eq!(
            rt.str_of(written),
            Some(
                r#"{"speed":3,"name":"fast","ratio":0.5,"on":true,"kept":11,"tags":["a","b"],"entries":[{"id":"x","weight":2},{"id":"y","weight":0}],"limits":{"a":1,"b":20}}"#
            )
        );
        // A box written as its primitive's type, as a config library writes a field it read
        // reflectively (`gson.toJson(field.get(o), field.getGenericType())`): a `Boolean` as
        // `boolean` was `false` whatever it held.
        for (class, kind, value, text) in [
            ("boolean", 'Z', V::I(1), "true"),
            ("int", 'I', V::I(7), "7"),
            ("double", 'D', V::D(0.25), "0.25"),
        ] {
            let boxed = crate::mixin::box_value(&mut rt, value, kind);
            let ty = rt.class_literal(class);
            let written = call(
                &mut rt,
                "com/google/gson/Gson.toJson.(Ljava/lang/Object;Ljava/lang/reflect/Type;)Ljava/lang/String;",
                &[gson, boxed, ty],
            );
            assert_eq!(rt.str_of(written), Some(text), "{class}");
            // And read as it: in its box again.
            let read = call(
                &mut rt,
                "com/google/gson/Gson.fromJson.(Ljava/lang/String;Ljava/lang/reflect/Type;)Ljava/lang/Object;",
                &[gson, written, ty],
            );
            assert_eq!(rt.class_of(read), rt.class_of(boxed), "{class}");
            assert_eq!(rt.get_field(read, "value"), value, "{class}");
        }
        // With no class to read by, the tree's own shape: maps, lists, every number a `Double`.
        let object = rt.class_literal("java/lang/Object");
        let plain = call(
            &mut rt,
            "com/google/gson/Gson.fromJson.(Ljava/lang/String;Ljava/lang/Class;)Ljava/lang/Object;",
            &[gson, file, object],
        );
        let speed = text(&mut rt, "speed");
        let three = crate::jutil::table_get(&mut rt, plain, speed).unwrap();
        assert_eq!(
            (rt.class_of(three), rt.get_field(three, "value")),
            ("java/lang/Double", V::D(3.0))
        );
        let listed = crate::jutil::new_list(&mut rt, vec![config]);
        let tree = call(
            &mut rt,
            "com/google/gson/Gson.toJsonTree.(Ljava/lang/Object;)Lcom/google/gson/JsonElement;",
            &[gson, listed],
        );
        assert_eq!(rt.class_of(tree), ARRAY);
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        assert_eq!(
            generic("Ljava/util/Map<Ljava/lang/String;Ljava/util/List<+Lt/Entry;>;>;"),
            (
                "Ljava/util/Map;".to_owned(),
                vec![
                    "Ljava/lang/String;".to_owned(),
                    "Ljava/util/List<+Lt/Entry;>;".to_owned()
                ]
            )
        );
        assert_eq!(generic("Ljava/util/List<*>;").1, [ANY]);
        assert_eq!(generic("Ljava/util/List<TT;>;").1, [ANY]);
        assert_eq!(generic("Ljava/util/List<[I>;").1, ["[I"]);
        assert_eq!(generic("I"), ("I".to_owned(), Vec::new()));
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
            Some(
                r#"{"entity":"minecraft:pig","offset":[0,1.5,-2e1],"scale":1.25,"big":5000000000,"on":true,"z":null,"a":{"x":"7","w":true}}"#
            )
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

    /// A number keeps the spelling it was read with, as Gson's does: written back and asked for
    /// as a string it is the file's own text, and its value is what the text says.
    #[test]
    fn a_parsed_number_keeps_its_spelling() {
        let mut rt = Rt::new();
        let text = r#"{"a":1.50,"b":1e3,"c":-0,"d":12345678901234567890,"e":0.1,"f":100,"g":1E-2}"#;
        let (o, _) = parse(&mut rt, text, false).unwrap().unwrap();
        assert_eq!(write(&rt, o, TO_STRING), text);
        let a = get(&mut rt, o, "a").unwrap();
        let s = primitive_as(&mut rt, a, "getAsString");
        assert_eq!(rt.str_of(s), Some("1.50"));
        assert_eq!(primitive_as(&mut rt, a, "getAsDouble"), V::D(1.5));
        let b = get(&mut rt, o, "b").unwrap();
        assert_eq!(primitive_as(&mut rt, b, "getAsInt"), V::I(1000));
        // One made by code is Java's `toString` of its value.
        let made = number(&mut rt, V::D(1.5));
        assert_eq!(write(&rt, made, TO_STRING), "1.5");
        assert!(same(&rt, a, made), "equal by value, whatever the spelling");
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
        // `Gson.fromJson` reads so whatever its builder was told (a mod's JSON5 config file).
        let gson = rt.new_object(GSON);
        let file = text(&mut rt, "{\n  // how far apart\n  \"spread\": 2\n}");
        let class = rt.class_literal(OBJECT);
        let o = call(
            &mut rt,
            "com/google/gson/Gson.fromJson.(Ljava/lang/String;Ljava/lang/Class;)Ljava/lang/Object;",
            &[gson, file, class],
        );
        assert!(rt.thrown.is_none());
        assert_eq!(write(&rt, o, TO_STRING), r#"{"spread":2}"#);
    }
}
