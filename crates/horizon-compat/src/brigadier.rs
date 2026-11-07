//! Brigadier's reading and its errors, natively, as Brigadier 1.3.10 reads and 1.21.1 words them
//! (`BrigadierExceptions`):
//!
//! - [`Reader`]: `StringReader` over a line as Java's `String` holds it (UTF-16 units). Each read
//!   fails as Brigadier's does, with its error at the cursor Brigadier leaves
//!   (`parsing.int.expected`...). The command parser reads vanilla's argument types with it, and a
//!   `StringReader` object (one a mod makes, or the one its own `ArgumentType.parse` is handed)
//!   runs on it.
//! - `CommandSyntaxException`, as the exception types make it (`SimpleCommandExceptionType`,
//!   `DynamicCommandExceptionType` to `DynamicNCommandExceptionType`), with the reader's place when
//!   given one; its `getMessage` and `getContext` as Brigadier writes them. `LiteralMessage` is a
//!   message of plain text. `CommandSyntaxException.BUILT_IN_EXCEPTIONS` gives 1.21.1's types, the
//!   ones a [`Reader`]'s errors are of.
use crate::{Data, Rt, V};

pub(crate) const READER: &str = "com/mojang/brigadier/StringReader";
const IMMUTABLE_READER: &str = "com/mojang/brigadier/ImmutableStringReader";
pub(crate) const SYNTAX: &str = "com/mojang/brigadier/exceptions/CommandSyntaxException";
const SIMPLE: &str = "com/mojang/brigadier/exceptions/SimpleCommandExceptionType";
const DYNAMIC: &str = "com/mojang/brigadier/exceptions/DynamicCommandExceptionType";
const DYNAMIC_2: &str = "com/mojang/brigadier/exceptions/Dynamic2CommandExceptionType";
const DYNAMIC_3: &str = "com/mojang/brigadier/exceptions/Dynamic3CommandExceptionType";
const DYNAMIC_4: &str = "com/mojang/brigadier/exceptions/Dynamic4CommandExceptionType";
const DYNAMIC_N: &str = "com/mojang/brigadier/exceptions/DynamicNCommandExceptionType";
const PROVIDER: &str = "com/mojang/brigadier/exceptions/BuiltInExceptionProvider";
const VANILLA_PROVIDER: &str = "net/minecraft/commands/BrigadierExceptions";
const LITERAL_MESSAGE: &str = "com/mojang/brigadier/LiteralMessage";
const MESSAGE: &str = "com/mojang/brigadier/Message";
const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const TAG_PARSER: &str = "net/minecraft/nbt/TagParser";

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        READER | IMMUTABLE_READER | SYNTAX | SIMPLE | DYNAMIC | DYNAMIC_2 | DYNAMIC_3 | DYNAMIC_4 | DYNAMIC_N | PROVIDER | VANILLA_PROVIDER | LITERAL_MESSAGE | MESSAGE
    ) || (owner == TAG_PARSER && matches!(name, "<init>" | "parseTag" | "readStruct" | "readValue"))
}

/// `CommandSyntaxException.BUILT_IN_EXCEPTIONS`: 1.21.1's provider (vanilla's bootstrap sets it).
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    (owner == SYNTAX && field == "BUILT_IN_EXCEPTIONS").then(|| rt.intern("compat:brigadier-exceptions".into(), |rt| rt.new_object(VANILLA_PROVIDER)))
}

/// 1.21.1's built-in exceptions (`BrigadierExceptions`): the provider's method, its message's key,
/// and how many arguments its type takes (a two-argument one's message says the bound before what
/// was found).
const BUILT_IN: [(&str, &str, u8); 27] = [
    ("doubleTooLow", "argument.double.low", 2),
    ("doubleTooHigh", "argument.double.big", 2),
    ("floatTooLow", "argument.float.low", 2),
    ("floatTooHigh", "argument.float.big", 2),
    ("integerTooLow", "argument.integer.low", 2),
    ("integerTooHigh", "argument.integer.big", 2),
    ("longTooLow", "argument.long.low", 2),
    ("longTooHigh", "argument.long.big", 2),
    ("literalIncorrect", "argument.literal.incorrect", 1),
    ("readerExpectedStartOfQuote", "parsing.quote.expected.start", 0),
    ("readerExpectedEndOfQuote", "parsing.quote.expected.end", 0),
    ("readerInvalidEscape", "parsing.quote.escape", 1),
    ("readerInvalidBool", "parsing.bool.invalid", 1),
    ("readerInvalidInt", "parsing.int.invalid", 1),
    ("readerExpectedInt", "parsing.int.expected", 0),
    ("readerInvalidLong", "parsing.long.invalid", 1),
    ("readerExpectedLong", "parsing.long.expected", 0),
    ("readerInvalidDouble", "parsing.double.invalid", 1),
    ("readerExpectedDouble", "parsing.double.expected", 0),
    ("readerInvalidFloat", "parsing.float.invalid", 1),
    ("readerExpectedFloat", "parsing.float.expected", 0),
    ("readerExpectedBool", "parsing.bool.expected", 0),
    ("readerExpectedSymbol", "parsing.expected", 1),
    ("dispatcherUnknownCommand", "command.unknown.command", 0),
    ("dispatcherUnknownArgument", "command.unknown.argument", 0),
    ("dispatcherExpectedArgumentSeparator", "command.expected.separator", 0),
    ("dispatcherParseException", "command.exception", 1),
];

// ---- The reader ----

/// Brigadier's `StringReader`: a line as Java's `String` holds it (UTF-16 units) and a cursor into it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reader {
    pub(crate) s: Vec<u16>,
    pub(crate) cursor: i32,
}

/// An argument of an error's message, as `Component.translatableEscape` keeps it (a decimal one as
/// Java prints it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Arg {
    Text(String),
    Int(i32),
    Long(i64),
}

/// A `CommandSyntaxException` made with a reader's context: its message (a translation key of
/// 1.21.1's and its arguments) and where the reader stood.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Syntax {
    pub(crate) key: &'static str,
    pub(crate) args: Vec<Arg>,
    pub(crate) cursor: i32,
}

pub(crate) type Read<T> = Result<T, Syntax>;

pub(crate) const fn unit(c: u8) -> u16 {
    c as u16
}

impl Reader {
    pub(crate) fn new(line: &str) -> Self {
        Reader { s: line.encode_utf16().collect(), cursor: 0 }
    }

    /// `getTotalLength()`.
    pub(crate) fn total(&self) -> i32 {
        self.s.len() as i32
    }

    /// `canRead(n)`.
    pub(crate) fn can_read_n(&self, n: i32) -> bool {
        i64::from(self.cursor) + i64::from(n) <= self.s.len() as i64
    }

    pub(crate) fn can_read(&self) -> bool {
        self.can_read_n(1)
    }

    fn unit_at(&self, i: i64) -> Option<u16> {
        usize::try_from(i).ok().and_then(|i| self.s.get(i).copied())
    }

    /// `peek()`: the unit at the cursor (0 past the line, where Brigadier's callers never look).
    pub(crate) fn peek(&self) -> u16 {
        self.unit_at(i64::from(self.cursor)).unwrap_or(0)
    }

    /// `peek(offset)`: the unit `offset` past the cursor (0 past the line).
    pub(crate) fn peek_at(&self, offset: i32) -> u16 {
        self.unit_at(i64::from(self.cursor) + i64::from(offset)).unwrap_or(0)
    }

    /// Whether the unit at the cursor is `c`.
    pub(crate) fn at(&self, c: u8) -> bool {
        self.can_read() && self.peek() == unit(c)
    }

    pub(crate) fn skip(&mut self) {
        self.cursor += 1;
    }

    /// `getString().substring(from, to)`, kept within the line.
    pub(crate) fn text(&self, from: i32, to: i32) -> String {
        let n = self.total();
        let (a, b) = (from.clamp(0, n) as usize, to.clamp(0, n) as usize);
        String::from_utf16_lossy(&self.s[a..b.max(a)])
    }

    pub(crate) fn line(&self) -> String {
        String::from_utf16_lossy(&self.s)
    }

    /// `getRemaining()`.
    pub(crate) fn remaining(&self) -> String {
        self.text(self.cursor, self.total())
    }

    /// The error `key` (with `args`) where the reader stands: `createWithContext(this, args...)`.
    pub(crate) fn error(&self, key: &'static str, args: Vec<Arg>) -> Syntax {
        Syntax { key, args, cursor: self.cursor }
    }

    pub(crate) fn skip_whitespace(&mut self) {
        while self.can_read() && java_whitespace(self.peek()) {
            self.skip();
        }
    }

    /// `readInt` and the other numbers: the characters of a number there (digits, points and minus
    /// signs), read as Java reads that type; Brigadier's `parsing.<type>.expected` when there are
    /// none, else `parsing.<type>.invalid` back at their start.
    fn read_number<T: std::str::FromStr>(&mut self, expected: &'static str, invalid: &'static str) -> Read<T> {
        let start = self.cursor;
        while self.can_read() && is_allowed_number(self.peek()) {
            self.skip();
        }
        let number = self.text(start, self.cursor);
        if number.is_empty() {
            return Err(self.error(expected, Vec::new()));
        }
        match number.parse() {
            Ok(n) => Ok(n),
            Err(_) => {
                self.cursor = start;
                Err(self.error(invalid, vec![Arg::Text(number)]))
            }
        }
    }

    pub(crate) fn read_int(&mut self) -> Read<i32> {
        self.read_number("parsing.int.expected", "parsing.int.invalid")
    }

    pub(crate) fn read_long(&mut self) -> Read<i64> {
        self.read_number("parsing.long.expected", "parsing.long.invalid")
    }

    pub(crate) fn read_double(&mut self) -> Read<f64> {
        self.read_number("parsing.double.expected", "parsing.double.invalid")
    }

    pub(crate) fn read_float(&mut self) -> Read<f32> {
        self.read_number("parsing.float.expected", "parsing.float.invalid")
    }

    pub(crate) fn read_unquoted_string(&mut self) -> String {
        let start = self.cursor;
        while self.can_read() && is_allowed_in_unquoted_string(self.peek()) {
            self.skip();
        }
        self.text(start, self.cursor)
    }

    /// `readQuotedString()`: nothing at the end of the line, else a quoted string.
    pub(crate) fn read_quoted_string(&mut self) -> Read<String> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if !is_quoted_string_start(next) {
            return Err(self.error("parsing.quote.expected.start", Vec::new()));
        }
        self.skip();
        self.read_string_until(next)
    }

    /// `readStringUntil(terminator)`: up to the terminator, `\` escaping it and itself.
    pub(crate) fn read_string_until(&mut self, terminator: u16) -> Read<String> {
        let mut out: Vec<u16> = Vec::new();
        let mut escaped = false;
        while self.can_read() {
            let c = self.peek();
            self.skip();
            if escaped {
                if c != terminator && c != unit(b'\\') {
                    self.cursor -= 1;
                    return Err(self.error("parsing.quote.escape", vec![Arg::Text(String::from_utf16_lossy(&[c]))]));
                }
                out.push(c);
                escaped = false;
            } else if c == unit(b'\\') {
                escaped = true;
            } else if c == terminator {
                return Ok(String::from_utf16_lossy(&out));
            } else {
                out.push(c);
            }
        }
        Err(self.error("parsing.quote.expected.end", Vec::new()))
    }

    /// `readString()`: a quoted string, or else an unquoted one.
    pub(crate) fn read_string(&mut self) -> Read<String> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if is_quoted_string_start(next) {
            self.skip();
            return self.read_string_until(next);
        }
        Ok(self.read_unquoted_string())
    }

    pub(crate) fn read_boolean(&mut self) -> Read<bool> {
        let start = self.cursor;
        let value = self.read_string()?;
        if value.is_empty() {
            return Err(self.error("parsing.bool.expected", Vec::new()));
        }
        match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => {
                self.cursor = start;
                Err(self.error("parsing.bool.invalid", vec![Arg::Text(value)]))
            }
        }
    }

    pub(crate) fn expect(&mut self, c: u16) -> Read<()> {
        if self.can_read() && self.peek() == c {
            self.skip();
            return Ok(());
        }
        Err(self.error("parsing.expected", vec![Arg::Text(String::from_utf16_lossy(&[c]))]))
    }

    /// The text from the cursor to the next space: the word `CommandNode.getRelevantNodes` looks
    /// up among a node's literals.
    pub(crate) fn word(&self) -> String {
        let mut end = i64::from(self.cursor);
        while end < self.s.len() as i64 && self.unit_at(end) != Some(unit(b' ')) {
            end += 1;
        }
        self.text(self.cursor, end as i32)
    }
}

pub(crate) fn is_allowed_number(c: u16) -> bool {
    matches!(c, 0x30..=0x39 | 0x2E | 0x2D)
}

pub(crate) fn is_quoted_string_start(c: u16) -> bool {
    c == unit(b'"') || c == unit(b'\'')
}

pub(crate) fn is_allowed_in_unquoted_string(c: u16) -> bool {
    matches!(c, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x5F | 0x2D | 0x2E | 0x2B)
}

/// Java's `Character.isWhitespace` for one UTF-16 unit: the space, line and paragraph separators
/// but the non-breaking spaces, and the ASCII controls tab to carriage return and 0x1C to 0x1F.
pub(crate) fn java_whitespace(c: u16) -> bool {
    matches!(c, 0x09..=0x0D | 0x1C..=0x20 | 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200A | 0x2028 | 0x2029 | 0x205F | 0x3000)
}

// ---- Exceptions as Java objects ----

/// The exception type of message `key`: one of [`BUILT_IN`] (taking its arguments), or another
/// vanilla parser's taking `arity` arguments. One object per key, so a mod comparing `getType()`
/// with `BUILT_IN_EXCEPTIONS`' finds it.
pub(crate) fn type_of(rt: &mut Rt, key: &str, arity: usize) -> V {
    let arity = BUILT_IN.iter().find(|b| b.1 == key).map_or(arity, |b| usize::from(b.2));
    let class = match arity {
        0 => SIMPLE,
        1 => DYNAMIC,
        _ => DYNAMIC_2,
    };
    rt.intern(format!("compat:brigadier-type:{key}"), |rt| {
        let t = rt.new_object(class);
        let k = rt.string(key);
        rt.put_field(t, "$builtin", k);
        t
    })
}

/// A Java `CommandSyntaxException` of `e`, read from `line` (its input).
pub(crate) fn exception(rt: &mut Rt, line: &[u16], e: &Syntax) -> V {
    let ty = type_of(rt, e.key, e.args.len());
    let message = message(rt, e);
    let input = rt.string(&String::from_utf16_lossy(line));
    new_exception(rt, ty, message, Some((input, e.cursor)))
}

/// A Java `CommandSyntaxException` of `e` made without a reader (`create`): no place in a line.
pub(crate) fn exception_without_context(rt: &mut Rt, e: &Syntax) -> V {
    let ty = type_of(rt, e.key, e.args.len());
    let message = message(rt, e);
    new_exception(rt, ty, message, None)
}

fn new_exception(rt: &mut Rt, ty: V, message: V, context: Option<(V, i32)>) -> V {
    let ex = rt.new_object(SYNTAX);
    rt.put_field(ex, "$type", ty);
    rt.put_field(ex, "$rawMessage", message);
    match context {
        Some((input, cursor)) => {
            rt.put_field(ex, "$input", input);
            rt.put_field(ex, "$cursor", V::I(cursor));
        }
        None => rt.put_field(ex, "$cursor", V::I(-1)),
    }
    ex
}

/// `Component.translatableEscape(key, args)`: the message of `e`.
pub(crate) fn message(rt: &mut Rt, e: &Syntax) -> V {
    let key = rt.string(e.key);
    let with = if e.args.is_empty() {
        V::NULL
    } else {
        let items: Vec<V> = e.args.iter().map(|a| arg_value(rt, a)).collect();
        rt.alloc("[Ljava/lang/Object;", Data::Array(items))
    };
    crate::api_common::component(rt, key, true, with)
}

fn arg_value(rt: &mut Rt, a: &Arg) -> V {
    match a {
        Arg::Text(t) => rt.string(t),
        Arg::Int(i) => crate::api_common::integer(rt, *i),
        Arg::Long(l) => {
            let o = rt.new_object("java/lang/Long");
            rt.put_field(o, "value", V::J(*l));
            o
        }
    }
}

/// A decimal argument as `String.valueOf` prints it (a float's as `Float.toString`).
pub(crate) fn decimal(d: f64, float: bool) -> Arg {
    Arg::Text(crate::java_double(d, float))
}

/// The line and cursor of a `StringReader` object.
pub(crate) fn reader_of(rt: &Rt, r: V) -> Reader {
    Reader { s: rt.str_of(rt.get_field(r, "$string")).unwrap_or("").encode_utf16().collect(), cursor: rt.get_field(r, "$cursor").i() }
}

/// A `StringReader` object over `r`'s line, at its cursor.
pub(crate) fn new_reader(rt: &mut Rt, r: &Reader) -> V {
    let o = rt.new_object(READER);
    let s = rt.string(&r.line());
    rt.put_field(o, "$string", s);
    rt.put_field(o, "$cursor", V::I(r.cursor));
    o
}

/// What a method returning `desc`'s type gives when it throws.
fn zero(desc: &str) -> V {
    match desc.rsplit(')').next() {
        Some("I" | "C" | "Z" | "S" | "B") => V::I(0),
        Some("J") => V::J(0),
        Some("F") => V::F(0.0),
        Some("D") => V::D(0.0),
        Some("V") => V::Void,
        _ => V::NULL,
    }
}

/// A Brigadier call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        READER | IMMUTABLE_READER => reader_call(rt, name, desc, args),
        SIMPLE | DYNAMIC | DYNAMIC_2 | DYNAMIC_3 | DYNAMIC_4 | DYNAMIC_N => type_call(rt, owner, name, args),
        PROVIDER | VANILLA_PROVIDER => {
            let &(_, key, arity) = BUILT_IN.iter().find(|b| b.0 == name)?;
            Some(type_of(rt, key, usize::from(arity)))
        }
        SYNTAX => syntax_call(rt, name, desc, args),
        TAG_PARSER => tag_parser_call(rt, name, desc, args),
        LITERAL_MESSAGE | MESSAGE => {
            let a0 = args.first().copied().unwrap_or_default();
            Some(match name {
                "<init>" => {
                    rt.put_field(a0, "$str", args.get(1).copied().unwrap_or_default());
                    V::Void
                }
                "getString" | "toString" => {
                    let text = message_text(rt, a0);
                    rt.string(&text)
                }
                _ => return None,
            })
        }
        _ => None,
    }
}

/// `TagParser`: SNBT read into tags as vanilla reads it. `parseTag(text)` reads one compound with
/// nothing after it; `readStruct()` and `readValue()` read from the parser's reader (`new
/// TagParser(reader)`), which moves past what they read. A syntax error throws vanilla's
/// `CommandSyntaxException`.
fn tag_parser_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    if name == "<init>" && desc == "(Lcom/mojang/brigadier/StringReader;)V" {
        rt.put_field(this, "$reader", args.get(1).copied().unwrap_or_default());
        return Some(V::Void);
    }
    let (mut r, reader) = match name {
        "parseTag" if desc == "(Ljava/lang/String;)Lnet/minecraft/nbt/CompoundTag;" => {
            let Some(text) = rt.str_of(this).map(str::to_owned) else {
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/NullPointerException",
                    "Cannot invoke \"String.length()\" because \"this.string\" is null",
                ));
            };
            (Reader::new(&text), None)
        }
        "readStruct" | "readValue" if desc.starts_with("()") => {
            let reader = rt.get_field(this, "$reader");
            if reader.r() == 0 {
                return None;
            }
            (reader_of(rt, reader), Some(reader))
        }
        _ => return None,
    };
    let read = match name {
        "readValue" => crate::command_args::snbt_value(&mut r, 0),
        _ => crate::command_args::snbt_struct(&mut r, 0).map(crate::nbt::Nbt::Compound),
    };
    // `parseTag` reads the whole text: only spaces may follow the compound.
    let read = read.and_then(|tag| {
        if reader.is_none() {
            r.skip_whitespace();
            if r.can_read() {
                return Err(r.error("argument.nbt.trailing", Vec::new()));
            }
        }
        Ok(tag)
    });
    if let Some(reader) = reader {
        rt.put_field(reader, "$cursor", V::I(r.cursor));
    }
    Some(match read {
        Ok(crate::nbt::Nbt::Compound(c)) => crate::nbt::to_runtime(rt, &c),
        Ok(other) => crate::nbt::element_to_runtime(rt, &other).unwrap_or(V::NULL),
        Err(e) => {
            let ex = exception(rt, &r.s, &e);
            rt.thrown = Some(ex);
            V::NULL
        }
    })
}

/// `StringReader`'s methods on a reader object (its line in `$string`, its cursor in `$cursor`).
fn reader_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let char_arg = |i: usize| a(i).i() as u16;
    // The statics, of one character.
    match name {
        "isAllowedNumber" => return Some(V::I(is_allowed_number(char_arg(0)) as i32)),
        "isQuotedStringStart" => return Some(V::I(is_quoted_string_start(char_arg(0)) as i32)),
        "isAllowedInUnquotedString" => return Some(V::I(is_allowed_in_unquoted_string(char_arg(0)) as i32)),
        _ => {}
    }
    let this = a(0);
    match name {
        "<init>" => {
            if desc == "(Ljava/lang/String;)V" {
                rt.put_field(this, "$string", a(1));
                rt.put_field(this, "$cursor", V::I(0));
            } else {
                let (s, c) = (rt.get_field(a(1), "$string"), rt.get_field(a(1), "$cursor").i());
                rt.put_field(this, "$string", s);
                rt.put_field(this, "$cursor", V::I(c));
            }
            return Some(V::Void);
        }
        "getString" => return Some(rt.get_field(this, "$string")),
        "setCursor" => {
            rt.put_field(this, "$cursor", V::I(a(1).i()));
            return Some(V::Void);
        }
        _ => {}
    }
    let mut r = reader_of(rt, this);
    let string = |rt: &mut Rt, s: Read<String>| s.map(|s| rt.string(&s));
    let result: Read<V> = match name {
        "getRemainingLength" => Ok(V::I(r.total() - r.cursor)),
        "getTotalLength" => Ok(V::I(r.total())),
        "getCursor" => Ok(V::I(r.cursor)),
        "getRead" => Ok(rt.string(&r.text(0, r.cursor))),
        "getRemaining" => Ok(rt.string(&r.remaining())),
        "canRead" => Ok(V::I(if desc == "(I)Z" { r.can_read_n(a(1).i()) } else { r.can_read() } as i32)),
        // `peek` and `read` past the line: Java's `charAt` throws.
        "peek" | "read" => {
            let at = i64::from(r.cursor) + if desc == "(I)C" { i64::from(a(1).i()) } else { 0 };
            if name == "read" {
                r.skip();
                rt.put_field(this, "$cursor", V::I(r.cursor));
            }
            return Some(match r.unit_at(at) {
                Some(c) => V::I(i32::from(c)),
                None => {
                    crate::jlang::throw_with(rt, "java/lang/StringIndexOutOfBoundsException", &format!("Index {at} out of bounds for length {}", r.total()));
                    V::I(0)
                }
            });
        }
        "skip" => {
            r.skip();
            Ok(V::Void)
        }
        "skipWhitespace" => {
            r.skip_whitespace();
            Ok(V::Void)
        }
        "readInt" => r.read_int().map(V::I),
        "readLong" => r.read_long().map(V::J),
        "readDouble" => r.read_double().map(V::D),
        "readFloat" => r.read_float().map(V::F),
        "readBoolean" => r.read_boolean().map(|b| V::I(b as i32)),
        "readUnquotedString" => Ok(rt.string(&r.read_unquoted_string())),
        "readQuotedString" => {
            let s = r.read_quoted_string();
            string(rt, s)
        }
        "readStringUntil" => {
            let s = r.read_string_until(char_arg(1));
            string(rt, s)
        }
        "readString" => {
            let s = r.read_string();
            string(rt, s)
        }
        "expect" => r.expect(char_arg(1)).map(|()| V::Void),
        _ => return None,
    };
    rt.put_field(this, "$cursor", V::I(r.cursor));
    Some(match result {
        Ok(v) => v,
        Err(e) => {
            let ex = exception(rt, &r.s, &e);
            rt.thrown = Some(ex);
            zero(desc)
        }
    })
}

/// The exception types: each makes a `CommandSyntaxException` of its message (what a dynamic
/// type's function makes of the arguments), with the reader's place for `createWithContext`.
fn type_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "<init>" if owner == SIMPLE => {
            rt.put_field(a(0), "$message", a(1));
            V::Void
        }
        "<init>" => {
            rt.put_field(a(0), "$function", a(1));
            V::Void
        }
        "create" | "createWithContext" => {
            let with_context = name == "createWithContext";
            let values = &args[usize::from(with_context).saturating_add(1).min(args.len())..];
            let message = type_message(rt, owner, a(0), values);
            let context = with_context.then(|| {
                let r = a(1);
                (rt.get_field(r, "$string"), rt.get_field(r, "$cursor").i())
            });
            new_exception(rt, a(0), message, context)
        }
        "toString" if owner == SIMPLE => {
            let m = rt.get_field(a(0), "$message");
            let text = message_text(rt, m);
            rt.string(&text)
        }
        _ => return None,
    })
}

/// The message type `ty` makes of `values`: a built-in type's (1.21.1's key; a two-argument one
/// says the bound first), a simple type's own, or what a dynamic type's function makes.
fn type_message(rt: &mut Rt, owner: &str, ty: V, values: &[V]) -> V {
    if let Some(key) = rt.str_of(rt.get_field(ty, "$builtin")).map(str::to_owned) {
        let mut items = values.to_vec();
        items.reverse();
        let k = rt.string(&key);
        let with = if items.is_empty() { V::NULL } else { rt.alloc("[Ljava/lang/Object;", Data::Array(items)) };
        return crate::api_common::component(rt, k, true, with);
    }
    let function = rt.get_field(ty, "$function");
    let apply: &'static str = match owner {
        SIMPLE => return rt.get_field(ty, "$message"),
        DYNAMIC => APPLY,
        DYNAMIC_2 => "com/mojang/brigadier/exceptions/Dynamic2CommandExceptionType$Function.apply.(Ljava/lang/Object;Ljava/lang/Object;)Lcom/mojang/brigadier/Message;",
        DYNAMIC_3 => "com/mojang/brigadier/exceptions/Dynamic3CommandExceptionType$Function.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Lcom/mojang/brigadier/Message;",
        DYNAMIC_4 => "com/mojang/brigadier/exceptions/Dynamic4CommandExceptionType$Function.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Lcom/mojang/brigadier/Message;",
        _ => "com/mojang/brigadier/exceptions/DynamicNCommandExceptionType$Function.apply.([Ljava/lang/Object;)Lcom/mojang/brigadier/Message;",
    };
    rt.call_functional(apply, function, values)
}

/// `CommandSyntaxException`'s own methods.
fn syntax_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    Some(match name {
        // `new CommandSyntaxException(type, message[, input, cursor])`.
        "<init>" => {
            rt.put_field(e, "$type", a(1));
            rt.put_field(e, "$rawMessage", a(2));
            if desc.contains("Ljava/lang/String;I)") {
                rt.put_field(e, "$input", a(3));
                rt.put_field(e, "$cursor", a(4));
            } else {
                rt.put_field(e, "$cursor", V::I(-1));
            }
            V::Void
        }
        "getRawMessage" => rt.get_field(e, "$rawMessage"),
        "getType" => rt.get_field(e, "$type"),
        "getInput" => match rt.get_field(e, "$input") {
            V::R(n) => V::R(n),
            _ => V::NULL,
        },
        "getCursor" => V::I(cursor_of(rt, e)),
        // Brigadier's `getMessage()`: the message's text, and where the input went wrong.
        "getMessage" | "getLocalizedMessage" | "getContext" | "toString" => {
            let context = context_of(rt, e);
            if name == "getContext" {
                return Some(match context {
                    Some(c) => rt.string(&c),
                    None => V::NULL,
                });
            }
            let raw = rt.get_field(e, "$rawMessage");
            let mut text = message_text(rt, raw);
            if let Some(c) = context {
                text = format!("{text} at position {}: {c}", cursor_of(rt, e));
            }
            if name == "toString" {
                text = format!("com.mojang.brigadier.exceptions.CommandSyntaxException: {text}");
            }
            rt.string(&text)
        }
        "printStackTrace" => V::Void,
        "fillInStackTrace" => e,
        _ => return None,
    })
}

fn cursor_of(rt: &Rt, e: V) -> i32 {
    match rt.get_field(e, "$cursor") {
        V::I(c) => c,
        _ => -1,
    }
}

/// `CommandSyntaxException.getContext()`: up to ten characters of the input before the cursor
/// (`...` before them when there were more), then `<--[HERE]`; None without an input.
fn context_of(rt: &Rt, e: V) -> Option<String> {
    let input: Vec<u16> = rt.str_of(rt.get_field(e, "$input"))?.encode_utf16().collect();
    let cursor = cursor_of(rt, e);
    if cursor < 0 {
        return None;
    }
    let at = (cursor as usize).min(input.len());
    let from = at.saturating_sub(10);
    Some(format!("{}{}<--[HERE]", if at > 10 { "..." } else { "" }, String::from_utf16_lossy(&input[from..at])))
}

/// A Brigadier `Message`'s text: a component's (translated where the game has the language), a
/// `LiteralMessage`'s, or a string.
pub(crate) fn message_text(rt: &mut Rt, message: V) -> String {
    if let Some(s) = rt.str_of(message) {
        return s.to_owned();
    }
    if rt.class_of(message) == LITERAL_MESSAGE {
        return rt.str_of(rt.get_field(message, "$str")).unwrap_or("").to_owned();
    }
    crate::gui::plain(rt, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, k: &str, args: &[V]) -> V {
        let k: &'static str = rt.static_key(k);
        crate::api::call(rt, k, args)
    }

    /// Brigadier's reads as `StringReader` makes them: numbers up to the first character that
    /// can't be in one, quoted strings with their escapes, booleans by their word, each failing
    /// with Brigadier's error where Brigadier leaves the cursor.
    #[test]
    fn the_reader_reads_and_fails_as_brigadiers() {
        let mut r = Reader::new("123abc");
        assert_eq!((r.read_int(), r.cursor), (Ok(123), 3));
        let mut r = Reader::new("1.5 x");
        assert_eq!(r.read_int(), Err(Syntax { key: "parsing.int.invalid", args: vec![Arg::Text("1.5".into())], cursor: 0 }));
        let mut r = Reader::new("x");
        assert_eq!(r.read_double(), Err(Syntax { key: "parsing.double.expected", args: vec![], cursor: 0 }));
        let mut r = Reader::new("-.5");
        assert_eq!(r.read_float(), Ok(-0.5));
        let mut r = Reader::new("3000000000");
        assert_eq!(r.read_int().map_err(|e| e.key), Err("parsing.int.invalid"));
        assert_eq!(Reader::new("3000000000").read_long(), Ok(3_000_000_000));
        let mut r = Reader::new(r#""a \"b\" \\ c" d"#);
        assert_eq!((r.read_string(), r.cursor), (Ok(r#"a "b" \ c"#.to_owned()), 14));
        let mut r = Reader::new(r#""bad \x""#);
        assert_eq!(r.read_string(), Err(Syntax { key: "parsing.quote.escape", args: vec![Arg::Text("x".into())], cursor: 6 }));
        let mut r = Reader::new(r#"'open"#);
        assert_eq!(r.read_string(), Err(Syntax { key: "parsing.quote.expected.end", args: vec![], cursor: 5 }));
        let mut r = Reader::new("word+1.x:y");
        assert_eq!(r.read_unquoted_string(), "word+1.x");
        let mut r = Reader::new("TRUE");
        assert_eq!(r.read_boolean(), Err(Syntax { key: "parsing.bool.invalid", args: vec![Arg::Text("TRUE".into())], cursor: 0 }));
        let mut r = Reader::new(" ");
        assert_eq!(r.read_boolean().map_err(|e| e.key), Err("parsing.bool.expected"));
        let mut r = Reader::new("ab");
        assert_eq!(r.expect(unit(b'[')), Err(Syntax { key: "parsing.expected", args: vec![Arg::Text("[".into())], cursor: 0 }));
        let mut r = Reader::new("\u{3000}\t x");
        r.skip_whitespace();
        assert_eq!(r.cursor, 3);
        // UTF-16, as Java counts: the emoji is two units.
        let mut r = Reader::new("😀 a");
        r.cursor = 3;
        assert_eq!((r.word(), r.total()), ("a".to_owned(), 4));
    }

    /// A mod's own argument type reads with a `StringReader` object: its reads move the object's
    /// cursor, a failed read throws Brigadier's `CommandSyntaxException` of 1.21.1's type (the one
    /// `BUILT_IN_EXCEPTIONS` gives), with the line and where it failed; reading past the line
    /// throws Java's.
    #[test]
    fn a_reader_object_reads_for_a_mods_parser() {
        let mut rt = Rt::new();
        let r = rt.new_object(READER);
        let line = rt.string("id:7 rest");
        c(&mut rt, &format!("{READER}.<init>.(Ljava/lang/String;)V"), &[r, line]);
        let word = c(&mut rt, &format!("{READER}.readUnquotedString.()Ljava/lang/String;"), &[r]);
        assert_eq!(rt.str_of(word), Some("id"));
        assert_eq!(c(&mut rt, &format!("{READER}.peek.()C"), &[r]), V::I(i32::from(b':')));
        c(&mut rt, &format!("{READER}.expect.(C)V"), &[r, V::I(i32::from(b':'))]);
        assert_eq!(c(&mut rt, &format!("{READER}.readInt.()I"), &[r]), V::I(7));
        assert_eq!(c(&mut rt, &format!("{READER}.getCursor.()I"), &[r]), V::I(4));
        c(&mut rt, &format!("{READER}.skip.()V"), &[r]);
        assert_eq!(c(&mut rt, &format!("{READER}.readInt.()I"), &[r]), V::I(0));
        let e = rt.thrown.take().expect("a syntax error");
        assert_eq!(rt.class_of(e), SYNTAX);
        let provider = get_static(&mut rt, SYNTAX, "BUILT_IN_EXCEPTIONS").unwrap();
        let expected_int = c(&mut rt, &format!("{PROVIDER}.readerExpectedInt.()L{SIMPLE};"), &[provider]);
        assert_eq!(c(&mut rt, &format!("{SYNTAX}.getType.()Lcom/mojang/brigadier/exceptions/CommandExceptionType;"), &[e]), expected_int);
        let said = c(&mut rt, &format!("{SYNTAX}.getMessage.()Ljava/lang/String;"), &[e]);
        assert_eq!(rt.str_of(said), Some("parsing.int.expected at position 5: id:7 <--[HERE]"), "untranslated here: no language loaded");
        // A copy reads on alone.
        let copy = rt.new_object(READER);
        c(&mut rt, &format!("{READER}.<init>.(L{READER};)V"), &[copy, r]);
        c(&mut rt, &format!("{READER}.setCursor.(I)V"), &[copy, V::I(9)]);
        assert_eq!(c(&mut rt, &format!("{READER}.canRead.()Z"), &[copy]), V::I(0));
        assert_eq!(c(&mut rt, &format!("{READER}.getCursor.()I"), &[r]), V::I(5));
        c(&mut rt, &format!("{READER}.peek.()C"), &[copy]);
        let oob = rt.thrown.take().expect("past the line");
        assert_eq!(rt.class_of(oob), "java/lang/StringIndexOutOfBoundsException");
        // A built-in type a mod takes from the provider says the bound before what it found.
        let too_low = c(&mut rt, &format!("{PROVIDER}.integerTooLow.()L{DYNAMIC_2};"), &[provider]);
        let (found, min) = (crate::api_common::integer(&mut rt, 0), crate::api_common::integer(&mut rt, 1));
        let e = c(&mut rt, &format!("{DYNAMIC_2}.createWithContext.(Lcom/mojang/brigadier/ImmutableStringReader;Ljava/lang/Object;Ljava/lang/Object;)L{SYNTAX};"), &[too_low, r, found, min]);
        let raw = rt.get_field(e, "$rawMessage");
        assert_eq!(crate::api_common::component_json(&rt, raw), serde_json::json!({"translate": "argument.integer.low", "with": [1, 0]}));
        assert_eq!(cursor_of(&rt, e), 5);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Brigadier's exceptions as a mod's command throws them: a `Dynamic2CommandExceptionType`'s
    /// message made of both arguments (Create's `/create clone` limit), `LiteralMessage`, and the
    /// exception's `getMessage()`, `getContext()` and `toString()` as Brigadier writes them (up
    /// to ten characters before the cursor, `...` before them, then `<--[HERE]`).
    #[test]
    fn command_exceptions_say_what_brigadiers_do() {
        fn too_big(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            let key = rt.string("commands.clone.toobig");
            let with = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![args[1], args[2]]));
            Some(crate::api_common::component(rt, key, true, with))
        }
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(too_big);
        let function = rt.lambda_id("com/mojang/brigadier/exceptions/Dynamic2CommandExceptionType$Function", 1, vec![]);
        let ty = rt.new_object(DYNAMIC_2);
        c(&mut rt, &format!("{DYNAMIC_2}.<init>.(L{DYNAMIC_2}$Function;)V"), &[ty, function]);
        let (limit, count) = (crate::api_common::integer(&mut rt, 32768), crate::api_common::integer(&mut rt, 68921));
        let e = c(&mut rt, &format!("{DYNAMIC_2}.create.(Ljava/lang/Object;Ljava/lang/Object;)L{SYNTAX};"), &[ty, limit, count]);
        let raw = c(&mut rt, &format!("{SYNTAX}.getRawMessage.()Lcom/mojang/brigadier/Message;"), &[e]);
        let json = crate::api_common::component_json(&rt, raw);
        assert_eq!(json["translate"], "commands.clone.toobig", "{json}");
        assert_eq!(json["with"].as_array().map(Vec::len), Some(2), "{json}");
        assert_eq!(c(&mut rt, &format!("{SYNTAX}.getCursor.()I"), &[e]), V::I(-1));
        // A message of plain text, at a place in the input.
        let message = rt.new_object(LITERAL_MESSAGE);
        let bad = rt.string("Bad");
        c(&mut rt, &format!("{LITERAL_MESSAGE}.<init>.(Ljava/lang/String;)V"), &[message, bad]);
        let text = c(&mut rt, &format!("{LITERAL_MESSAGE}.getString.()Ljava/lang/String;"), &[message]);
        assert_eq!(rt.str_of(text), Some("Bad"));
        let e = rt.new_object(SYNTAX);
        let input = rt.string("create clone 1 2");
        c(&mut rt, &format!("{SYNTAX}.<init>.(Lcom/mojang/brigadier/exceptions/CommandExceptionType;Lcom/mojang/brigadier/Message;Ljava/lang/String;I)V"), &[e, ty, message, input, V::I(13)]);
        let said = |rt: &mut Rt, name: &str| {
            let v = c(rt, &format!("{SYNTAX}.{name}.()Ljava/lang/String;"), &[e]);
            rt.str_of(v).unwrap_or("").to_owned()
        };
        assert_eq!(said(&mut rt, "getContext"), "...ate clone <--[HERE]");
        assert_eq!(said(&mut rt, "getMessage"), "Bad at position 13: ...ate clone <--[HERE]");
        assert_eq!(said(&mut rt, "toString"), "com.mojang.brigadier.exceptions.CommandSyntaxException: Bad at position 13: ...ate clone <--[HERE]");
        assert_eq!(c(&mut rt, &format!("{SYNTAX}.getCursor.()I"), &[e]), V::I(13));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `TagParser` reads SNBT into tags of the type vanilla gives each word (its suffix's, its
    /// number's form, a boolean's byte, else a string, as is a number its type can't hold), lists
    /// of one type and typed arrays; it fails with vanilla's errors where vanilla's cursor stops.
    #[test]
    fn tag_parser_reads_snbt_into_vanillas_tags() {
        use crate::nbt::Nbt;
        use std::collections::BTreeMap;
        const PARSE: &str = "net/minecraft/nbt/TagParser.parseTag.(Ljava/lang/String;)Lnet/minecraft/nbt/CompoundTag;";
        let mut rt = Rt::new();
        crate::install();
        let parse = |rt: &mut Rt, text: &str| {
            let s = rt.string(text);
            let tag = c(rt, PARSE, &[s]);
            (tag, rt.thrown.take())
        };
        let (tag, thrown) = parse(
            &mut rt,
            r#" {a:1b, b:-2S, c:3, d:4L, e:1.5f, f:2.5, g:3d, h:1.e3, i:true, j:False, k:hello, l:'it"s', m:300b, n:1e3, o:[1, 2], p:[B;1b,2B], q:[I; 3], r:[L;4l], s:{t:[]}, "u v":+7} "#,
        );
        assert!(thrown.is_none());
        let text = |s: &str| Nbt::String(s.into());
        let expected: BTreeMap<String, Nbt> = [
            ("a", Nbt::Byte(1)),
            ("b", Nbt::Short(-2)),
            ("c", Nbt::Int(3)),
            ("d", Nbt::Long(4)),
            ("e", Nbt::Float(1.5)),
            ("f", Nbt::Double(2.5)),
            ("g", Nbt::Double(3.0)),
            ("h", Nbt::Double(1000.0)),
            ("i", Nbt::Byte(1)),
            ("j", Nbt::Byte(0)),
            ("k", text("hello")),
            ("l", text("it\"s")),
            ("m", text("300b")),
            ("n", text("1e3")),
            ("o", Nbt::List(vec![Nbt::Int(1), Nbt::Int(2)])),
            ("p", Nbt::ByteArray(vec![1, 2])),
            ("q", Nbt::IntArray(vec![3])),
            ("r", Nbt::LongArray(vec![4])),
            (
                "s",
                Nbt::Compound([("t".to_owned(), Nbt::List(Vec::new()))].into()),
            ),
            ("u v", Nbt::Int(7)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        assert_eq!(crate::nbt::from_runtime(&rt, tag), expected);

        let failure = |rt: &mut Rt, text: &str| {
            let (tag, thrown) = parse(rt, text);
            assert_eq!(tag, V::NULL, "{text}");
            let e = thrown.expect("a syntax error");
            let raw = c(
                rt,
                &format!("{SYNTAX}.getRawMessage.()Lcom/mojang/brigadier/Message;"),
                &[e],
            );
            let json = crate::api_common::component_json(rt, raw);
            (
                json["translate"].as_str().unwrap_or("").to_owned(),
                json["with"].to_string(),
                c(rt, &format!("{SYNTAX}.getCursor.()I"), &[e]),
            )
        };
        let mixed = failure(&mut rt, "{a:[1,2b]}");
        assert_eq!(
            (mixed.0.as_str(), mixed.2),
            ("argument.nbt.list.mixed", V::I(6))
        );
        assert!(
            mixed.1.find("TAG_Byte") < mixed.1.find("TAG_Int"),
            "{}",
            mixed.1
        );
        let in_array = failure(&mut rt, "{a:[I;1,2b]}");
        assert_eq!(
            (in_array.0.as_str(), in_array.2),
            ("argument.nbt.array.mixed", V::I(8))
        );
        assert!(in_array.1.contains("TAG_Int_Array"), "{}", in_array.1);
        assert_eq!(
            failure(&mut rt, "{a:[X;1]}").0,
            "argument.nbt.array.invalid"
        );
        let trailing = failure(&mut rt, "{a:1} x");
        assert_eq!(
            (trailing.0.as_str(), trailing.2),
            ("argument.nbt.trailing", V::I(6))
        );
        // A comma may close a compound (`{a:1,}` reads), but not end the text, and a key is needed.
        let (tag, thrown) = parse(&mut rt, "{a:1,}");
        assert!(thrown.is_none() && tag.r() != 0);
        let open = failure(&mut rt, "{a:1,");
        assert_eq!(
            (open.0.as_str(), open.2),
            ("argument.nbt.expected.key", V::I(5))
        );
        let keyless = failure(&mut rt, "{:1}");
        assert_eq!(
            (keyless.0.as_str(), keyless.2),
            ("argument.nbt.expected.key", V::I(1))
        );

        // A parser over a reader reads one compound from its cursor and leaves the reader after it.
        let reader = rt.new_object(READER);
        let line = rt.string("{x:[I;1,2]} rest");
        c(
            &mut rt,
            &format!("{READER}.<init>.(Ljava/lang/String;)V"),
            &[reader, line],
        );
        let parser = rt.new_object(TAG_PARSER);
        c(
            &mut rt,
            &format!("{TAG_PARSER}.<init>.(L{READER};)V"),
            &[parser, reader],
        );
        let tag = c(
            &mut rt,
            &format!("{TAG_PARSER}.readStruct.()Lnet/minecraft/nbt/CompoundTag;"),
            &[parser],
        );
        assert_eq!(
            crate::nbt::from_runtime(&rt, tag),
            [("x".to_owned(), Nbt::IntArray(vec![1, 2]))].into()
        );
        assert_eq!(
            c(&mut rt, &format!("{READER}.getCursor.()I"), &[reader]),
            V::I(11)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
