//! `java.util.regex` on Rust's `regex` engine: `Pattern` (`compile` with its flags, `matcher`,
//! `split`, `quote`, `matches`), `Matcher` (`matches`, `find`, `lookingAt`, groups by number and by
//! name, `start`/`end`, `replaceAll`/`replaceFirst`, `appendReplacement`/`appendTail`, `reset`,
//! `toMatchResult`), and `String`'s regex methods (`split`, `replaceAll`, `replaceFirst`, `matches`,
//! through [`split`], [`replace`] and [`matches`]).
//!
//! A Java pattern is rewritten into Rust's syntax first ([`to_rust`]): `\w`, `\d` and `\s` stay
//! ASCII as Java's are, `\Q…\E` quotes, `\uXXXX` is a code point, a possessive quantifier and an
//! atomic group match greedily, Java's POSIX classes (`\p{Alpha}`) are ASCII. What Rust's engine
//! has no form for (lookaround, backreferences) is Java's `PatternSyntaxException`. Indexes are
//! Java's: UTF-16 units of the input.
use crate::{Data, Rt, V};
use std::cell::RefCell;
use std::collections::HashMap;

const PATTERN: &str = "java/util/regex/Pattern";
const MATCHER: &str = "java/util/regex/Matcher";
const MATCH_RESULT: &str = "java/util/regex/MatchResult";
const SYNTAX_ERROR: &str = "java/util/regex/PatternSyntaxException";
const ILLEGAL_ARGUMENT: &str = "java/lang/IllegalArgumentException";

// Java's flags (`Pattern.CASE_INSENSITIVE`...).
const CASE_INSENSITIVE: i32 = 2;
const COMMENTS: i32 = 4;
const MULTILINE: i32 = 8;
const LITERAL: i32 = 16;
const DOTALL: i32 = 32;

/// How a compiled pattern is anchored: anywhere (`find`), the whole input (`matches`), its start
/// (`lookingAt`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Mode {
    Find,
    Full,
    Prefix,
}

thread_local! {
    /// The patterns compiled on this thread, by source, flags and anchoring: most are constants a
    /// mod compiles once, so a call costs a lookup.
    static COMPILED: RefCell<HashMap<(String, i32, Mode), Result<regex::Regex, String>>> = RefCell::new(HashMap::new());
}

fn compiled(source: &str, flags: i32, mode: Mode) -> Result<regex::Regex, String> {
    let key = (source.to_owned(), flags, mode);
    if let Some(r) = COMPILED.with(|c| c.borrow().get(&key).cloned()) {
        return r;
    }
    let r = to_rust(source, flags).and_then(|p| {
        // In comments mode a `#` comment runs to the line's end: a newline ends it before the
        // anchors.
        let p = if flags & COMMENTS != 0 { format!("{p}\n") } else { p };
        let p = match mode {
            Mode::Find => p,
            Mode::Full => format!(r"\A(?:{p})\z"),
            Mode::Prefix => format!(r"\A(?:{p})"),
        };
        regex::Regex::new(&p).map_err(|e| e.to_string())
    });
    COMPILED.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(key, r.clone());
    });
    r
}

/// Java's pattern `p` with `flags` in Rust's syntax.
pub(crate) fn to_rust(p: &str, flags: i32) -> Result<String, String> {
    let mut out = String::new();
    let inline: String = [(CASE_INSENSITIVE, 'i'), (MULTILINE, 'm'), (DOTALL, 's'), (COMMENTS, 'x')]
        .iter()
        .filter(|(f, _)| flags & f != 0)
        .map(|(_, c)| *c)
        .collect();
    if !inline.is_empty() {
        out.push_str(&format!("(?{inline})"));
    }
    if flags & LITERAL != 0 {
        out.push_str(&regex::escape(p));
        return Ok(out);
    }
    let chars: Vec<char> = p.chars().collect();
    let mut i = 0;
    // Depth in `[...]` (Java's classes nest, as Rust's do).
    let mut class = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let Some(&n) = chars.get(i + 1) else {
                    return Err(format!("Unexpected internal error near index {}", chars.len()));
                };
                i += 2;
                match n {
                    'Q' => {
                        let mut lit = String::new();
                        while i < chars.len() && !(chars[i] == '\\' && chars.get(i + 1) == Some(&'E')) {
                            lit.push(chars[i]);
                            i += 1;
                        }
                        i = (i + 2).min(chars.len().max(i));
                        if class > 0 {
                            for ch in lit.chars() {
                                push_class_literal(&mut out, ch);
                            }
                        } else {
                            out.push_str(&regex::escape(&lit));
                        }
                    }
                    'E' => {}
                    'w' | 'W' | 'd' | 'D' | 's' | 'S' => {
                        let name = match n.to_ascii_lowercase() {
                            'w' => "word",
                            'd' => "digit",
                            _ => "space",
                        };
                        let not = if n.is_ascii_uppercase() { "^" } else { "" };
                        if class > 0 {
                            out.push_str(&format!("[:{not}{name}:]"));
                        } else {
                            out.push_str(&format!("[[:{not}{name}:]]"));
                        }
                    }
                    'h' | 'H' => {
                        let set = r" \t\xA0\x{1680}\x{180E}\x{2000}-\x{200A}\x{202F}\x{205F}\x{3000}";
                        match (class > 0, n == 'H') {
                            (true, false) => out.push_str(set),
                            (true, true) => out.push_str(&format!("[^{set}]")),
                            (false, false) => out.push_str(&format!("[{set}]")),
                            (false, true) => out.push_str(&format!("[^{set}]")),
                        }
                    }
                    'v' => out.push_str(if class > 0 { r"\n\x0B\f\r\x{85}\x{2028}\x{2029}" } else { r"[\n\x0B\f\r\x{85}\x{2028}\x{2029}]" }),
                    'R' if class == 0 => out.push_str(r"(?:\r\n|[\n\x0B\f\r\x{85}\x{2028}\x{2029}])"),
                    'A' | 'z' | 'b' | 'B' if class == 0 => {
                        out.push('\\');
                        out.push(n);
                    }
                    'Z' if class == 0 => out.push_str(r"\z"),
                    't' | 'n' | 'r' | 'f' | 'a' => {
                        out.push('\\');
                        out.push(n);
                    }
                    'e' => out.push_str(r"\x1B"),
                    '0' => {
                        // Octal `\0n`, `\0nn`, `\0mnn`.
                        let mut v = 0u32;
                        let mut k = 0;
                        while k < 3 && chars.get(i).is_some_and(|d| d.is_digit(8)) && v * 8 + chars[i].to_digit(8).unwrap_or(0) <= 0o377 {
                            v = v * 8 + chars[i].to_digit(8).unwrap_or(0);
                            i += 1;
                            k += 1;
                        }
                        if k == 0 {
                            return Err("Illegal octal escape sequence".into());
                        }
                        out.push_str(&format!(r"\x{{{v:X}}}"));
                    }
                    'u' => {
                        let hex: String = chars.get(i..i + 4).map(|h| h.iter().collect()).unwrap_or_default();
                        let v = u32::from_str_radix(&hex, 16).map_err(|_| "Illegal Unicode escape sequence".to_owned())?;
                        i += 4;
                        out.push_str(&format!(r"\x{{{v:X}}}"));
                    }
                    'x' => {
                        out.push_str(r"\x");
                    }
                    'p' | 'P' => {
                        let name: String = if chars.get(i) == Some(&'{') {
                            let end = chars[i..].iter().position(|&ch| ch == '}').ok_or("Unclosed character family")?;
                            let name = chars[i + 1..i + end].iter().collect();
                            i += end + 1;
                            name
                        } else {
                            let one = chars.get(i).copied().ok_or("Illegal character family")?;
                            i += 1;
                            one.to_string()
                        };
                        let posix = match name.trim_start_matches("Is") {
                            "Lower" => Some("lower"),
                            "Upper" => Some("upper"),
                            "ASCII" => Some("ascii"),
                            "Alpha" => Some("alpha"),
                            "Digit" => Some("digit"),
                            "Alnum" => Some("alnum"),
                            "Punct" => Some("punct"),
                            "Graph" => Some("graph"),
                            "Print" => Some("print"),
                            "Blank" => Some("blank"),
                            "Cntrl" => Some("cntrl"),
                            "XDigit" => Some("xdigit"),
                            "Space" => Some("space"),
                            _ => None,
                        };
                        let not = if n == 'P' { "^" } else { "" };
                        match (posix, class > 0) {
                            (Some(c), true) => out.push_str(&format!("[:{not}{c}:]")),
                            (Some(c), false) => out.push_str(&format!("[[:{not}{c}:]]")),
                            (None, _) => out.push_str(&format!(r"\{n}{{{name}}}")),
                        }
                    }
                    '1'..='9' | 'k' => return Err("Back references are not supported".into()),
                    'G' | 'X' | 'N' | 'c' => return Err(format!("Unsupported escape sequence \\{n}")),
                    // An escaped character is itself (Java escapes any non-letter this way).
                    n => {
                        if class > 0 {
                            push_class_literal(&mut out, n);
                        } else {
                            out.push_str(&regex::escape(&n.to_string()));
                        }
                    }
                }
            }
            '[' => {
                class += 1;
                out.push('[');
                i += 1;
                if chars.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            ']' if class > 0 => {
                class -= 1;
                out.push(']');
                i += 1;
            }
            '(' if class == 0 => {
                if chars.get(i + 1) != Some(&'?') {
                    out.push('(');
                    i += 1;
                    continue;
                }
                match (chars.get(i + 2), chars.get(i + 3)) {
                    (Some('>'), _) => {
                        out.push_str("(?:");
                        i += 3;
                    }
                    (Some('=' | '!'), _) | (Some('<'), Some('=' | '!')) => return Err("Lookaround is not supported".into()),
                    (Some('<'), _) => {
                        out.push_str("(?<");
                        i += 3;
                    }
                    _ => {
                        // Inline flags: Java's `d` (Unix lines), `u` (Unicode case) and `U`
                        // (Unicode classes) have no Rust meaning here (Rust's `U` swaps greed).
                        i += 2;
                        let mut f = String::new();
                        while let Some(&ch) = chars.get(i) {
                            if ch == ':' || ch == ')' {
                                break;
                            }
                            if !matches!(ch, 'd' | 'u' | 'U') {
                                f.push(ch);
                            }
                            i += 1;
                        }
                        let closes = chars.get(i) == Some(&')');
                        if f.is_empty() || f == "-" {
                            if closes {
                                i += 1;
                            } else {
                                out.push_str("(?:");
                                i += 1;
                            }
                        } else {
                            out.push_str("(?");
                            out.push_str(&f);
                        }
                    }
                }
            }
            '*' | '+' | '?' | '}' if class == 0 => {
                out.push(c);
                i += 1;
                // A possessive quantifier, greedy here.
                if chars.get(i) == Some(&'+') {
                    i += 1;
                }
            }
            '&' if class > 0 && chars.get(i + 1) == Some(&'&') => {
                out.push_str("&&");
                i += 2;
            }
            c if class > 0 => {
                // Rust reads `[`, `&`, `~` and `-` twice over inside a class as set operations.
                if matches!(c, '~') || (c == '&') {
                    out.push('\\');
                }
                out.push(c);
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn push_class_literal(out: &mut String, ch: char) {
    if matches!(ch, '[' | ']' | '\\' | '^' | '-' | '&' | '~') {
        out.push('\\');
    }
    out.push(ch);
}

fn text(rt: &Rt, v: V) -> String {
    rt.str_of(v).map(str::to_owned).unwrap_or_else(|| rt.to_java_string(v, 'L'))
}

fn new_str(rt: &mut Rt, s: String) -> V {
    rt.alloc("java/lang/String", Data::Str(s))
}

fn throw(rt: &mut Rt, class: &'static str, msg: &str) {
    if class == SYNTAX_ERROR {
        rt.supers.entry(SYNTAX_ERROR).or_insert(ILLEGAL_ARGUMENT);
    }
    crate::jlang::throw_with(rt, class, msg);
}

/// `source` compiled for `mode`, or Java's `PatternSyntaxException` thrown and None.
fn regex_for(rt: &mut Rt, source: &str, flags: i32, mode: Mode) -> Option<regex::Regex> {
    match compiled(source, flags, mode) {
        Ok(r) => Some(r),
        Err(e) => {
            if Rt::tracing() {
                eprintln!("horizon-compat: pattern {source:?} not compiled: {e}");
            }
            throw(rt, SYNTAX_ERROR, &format!("{e}\n{source}"));
            None
        }
    }
}

/// The UTF-16 index of byte offset `b` in `s`.
fn u16_at(s: &str, b: usize) -> i32 {
    s.get(..b).map_or(0, |p| p.encode_utf16().count() as i32)
}

/// The byte offset of UTF-16 index `u` in `s`: None past its end or inside a pair.
fn byte_at(s: &str, u: i32) -> Option<usize> {
    let mut n = 0;
    for (b, ch) in s.char_indices() {
        if n == u {
            return Some(b);
        }
        n += ch.len_utf16() as i32;
        if n > u {
            return None;
        }
    }
    (n == u).then_some(s.len())
}

/// A match's groups: byte ranges, None for a group that took no part.
type Groups = Vec<Option<(usize, usize)>>;

fn groups_of(caps: &regex::Captures<'_>) -> Groups {
    caps.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect()
}

/// The `Pattern` object for `source` and `flags`, compiled now so a bad one throws where Java's does.
fn pattern(rt: &mut Rt, source: V, flags: i32) -> V {
    let s = text(rt, source);
    regex_for(rt, &s, flags, Mode::Find);
    if rt.thrown.is_some() {
        return V::NULL;
    }
    let p = rt.new_object(PATTERN);
    let src = new_str(rt, s);
    rt.put_field(p, "$source", src);
    rt.put_field(p, "$flags", V::I(flags));
    p
}

fn source_of(rt: &Rt, p: V) -> (String, i32) {
    (rt.str_of(rt.get_field(p, "$source")).unwrap_or("").to_owned(), rt.get_field(p, "$flags").i())
}

fn matcher(rt: &mut Rt, p: V, input: V) -> V {
    let m = rt.new_object(MATCHER);
    rt.put_field(m, "$pattern", p);
    let t = text(rt, input);
    let t = new_str(rt, t);
    rt.put_field(m, "$text", t);
    reset(rt, m);
    m
}

fn reset(rt: &mut Rt, m: V) {
    rt.put_field(m, "$first", V::I(-1));
    rt.put_field(m, "$last", V::I(0));
    rt.put_field(m, "$append", V::I(0));
    rt.put_field(m, "$groups", V::NULL);
}

fn matcher_text(rt: &Rt, m: V) -> String {
    rt.str_of(rt.get_field(m, "$text")).unwrap_or("").to_owned()
}

fn set_groups(rt: &mut Rt, m: V, groups: Option<&Groups>) {
    let Some(g) = groups else {
        rt.put_field(m, "$first", V::I(-1));
        rt.put_field(m, "$groups", V::NULL);
        return;
    };
    let flat: Vec<V> = g.iter().flat_map(|r| r.map_or([V::I(-1), V::I(-1)], |(s, e)| [V::I(s as i32), V::I(e as i32)])).collect();
    let (s, e) = g.first().copied().flatten().unwrap_or((0, 0));
    let arr = rt.alloc("[I", Data::Array(flat));
    rt.put_field(m, "$groups", arr);
    rt.put_field(m, "$first", V::I(s as i32));
    rt.put_field(m, "$last", V::I(e as i32));
}

/// The groups of `m`'s last match, or None when it has none.
fn groups(rt: &Rt, m: V) -> Option<Groups> {
    let arr = rt.get_field(m, "$groups");
    if arr.r() == 0 {
        return None;
    }
    let flat = rt.captured(arr);
    Some(flat.chunks(2).map(|c| (c[0].i() >= 0).then(|| (c[0].i() as usize, c[1].i() as usize))).collect())
}

/// Searches `m`'s input from byte `from` (`Matcher.search`): its next match, if any.
fn search(rt: &mut Rt, m: V, from: usize, mode: Mode) -> bool {
    let p = rt.get_field(m, "$pattern");
    let (source, flags) = source_of(rt, p);
    let Some(re) = regex_for(rt, &source, flags, mode) else { return false };
    let t = matcher_text(rt, m);
    let found = if from > t.len() { None } else { re.captures_at(&t, from).map(|c| groups_of(&c)) };
    set_groups(rt, m, found.as_ref());
    found.is_some()
}

/// `Matcher.find()`: from where the last match ended, one character on after an empty one.
fn find(rt: &mut Rt, m: V) -> bool {
    let (first, last) = (rt.get_field(m, "$first").i(), rt.get_field(m, "$last").i());
    let t = matcher_text(rt, m);
    let mut next = last.max(0) as usize;
    if first == last {
        next += t.get(next..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
    }
    if next > t.len() {
        set_groups(rt, m, None);
        return false;
    }
    search(rt, m, next, Mode::Find)
}

/// Java's replacement `repl` for a match of `re` in `t` (`appendReplacement`): `$n` and `${name}`
/// are its groups, `\x` is `x`. Err is the exception Java throws.
fn expand(re: &regex::Regex, t: &str, g: &Groups, repl: &str) -> Result<String, (&'static str, String)> {
    let chars: Vec<char> = repl.chars().collect();
    let count = g.len().saturating_sub(1);
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let c = *chars.get(i + 1).ok_or((ILLEGAL_ARGUMENT, "character to be escaped is missing".to_owned()))?;
                out.push(c);
                i += 2;
            }
            '$' => {
                let c = *chars.get(i + 1).ok_or((ILLEGAL_ARGUMENT, "Illegal group reference: group index is missing".to_owned()))?;
                i += 2;
                let n = if c == '{' {
                    let end = chars[i..].iter().position(|&ch| ch == '}').ok_or((ILLEGAL_ARGUMENT, "named capturing group is missing trailing '}'".to_owned()))?;
                    let name: String = chars[i..i + end].iter().collect();
                    i += end + 1;
                    re.capture_names().position(|n| n == Some(name.as_str())).ok_or((ILLEGAL_ARGUMENT, format!("No group with name {{{name}}}")))?
                } else {
                    let mut n = c.to_digit(10).ok_or((ILLEGAL_ARGUMENT, "Illegal group reference".to_owned()))? as usize;
                    // Java takes the next digits while they name a group.
                    while let Some(d) = chars.get(i).and_then(|ch| ch.to_digit(10)) {
                        if n * 10 + d as usize > count {
                            break;
                        }
                        n = n * 10 + d as usize;
                        i += 1;
                    }
                    if n > count {
                        return Err(("java/lang/IndexOutOfBoundsException", format!("No group {n}")));
                    }
                    n
                };
                if let Some(Some((s, e))) = g.get(n) {
                    out.push_str(&t[*s..*e]);
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Every match of `re` in `t`, as `Matcher.find` walks them (or only the first).
fn all_matches(re: &regex::Regex, t: &str, first_only: bool) -> Vec<Groups> {
    let mut out = Vec::new();
    let (mut from, mut last_empty_at) = (0usize, None);
    while from <= t.len() {
        let Some(c) = re.captures_at(t, from) else { break };
        let g = groups_of(&c);
        let (s, e) = g[0].unwrap_or((from, from));
        // An empty match where the last one ended is skipped, as Java's `find` steps past it.
        if s == e && last_empty_at == Some(s) {
            from = e + t[e..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        out.push(g);
        if first_only {
            break;
        }
        if s == e {
            last_empty_at = Some(e);
            from = e + t[e..].chars().next().map_or(1, char::len_utf8);
        } else {
            last_empty_at = Some(e);
            from = e;
        }
    }
    out
}

/// `Matcher.replaceAll`/`replaceFirst` over `t`.
fn replace_in(rt: &mut Rt, re: &regex::Regex, t: &str, repl: &str, first_only: bool) -> Option<String> {
    let mut out = String::new();
    let mut at = 0;
    for g in all_matches(re, t, first_only) {
        let (s, e) = g[0].unwrap_or((at, at));
        match expand(re, t, &g, repl) {
            Ok(r) => {
                out.push_str(&t[at..s]);
                out.push_str(&r);
            }
            Err((class, msg)) => {
                throw(rt, class, &msg);
                return None;
            }
        }
        at = e;
    }
    out.push_str(&t[at..]);
    Some(out)
}

/// `Pattern.split(input, limit)`, Java's: no leading empty string for an empty match at the start,
/// at most `limit` parts when positive, trailing empty ones dropped when it is 0.
fn split_with(re: &regex::Regex, t: &str, limit: i32) -> Vec<String> {
    let limited = limit > 0;
    let mut parts: Vec<String> = Vec::new();
    let mut index = 0;
    let mut matched = false;
    for g in all_matches(re, t, false) {
        let (s, e) = g[0].unwrap_or((0, 0));
        if !limited || (parts.len() as i32) < limit - 1 {
            if index == 0 && s == 0 && s == e {
                continue;
            }
            parts.push(t[index..s].to_owned());
            index = e;
            matched = true;
        } else {
            break;
        }
    }
    if !matched {
        return vec![t.to_owned()];
    }
    if !limited || (parts.len() as i32) < limit {
        parts.push(t[index..].to_owned());
    }
    if limit == 0 {
        while parts.last().is_some_and(String::is_empty) {
            parts.pop();
        }
    }
    parts
}

fn string_array(rt: &mut Rt, parts: Vec<String>) -> V {
    let items: Vec<V> = parts.into_iter().map(|p| new_str(rt, p)).collect();
    rt.alloc("[Ljava/lang/String;", Data::Array(items))
}

/// `String.split(regex, limit)`.
pub(crate) fn split(rt: &mut Rt, s: &str, regex: &str, limit: i32) -> V {
    let Some(re) = regex_for(rt, regex, 0, Mode::Find) else { return V::NULL };
    let parts = split_with(&re, s, limit);
    string_array(rt, parts)
}

/// `String.replaceAll(regex, replacement)` (`first`: `replaceFirst`).
pub(crate) fn replace(rt: &mut Rt, s: &str, regex: &str, replacement: &str, first: bool) -> V {
    let Some(re) = regex_for(rt, regex, 0, Mode::Find) else { return V::NULL };
    match replace_in(rt, &re, s, replacement, first) {
        Some(r) => new_str(rt, r),
        None => V::NULL,
    }
}

/// `String.matches(regex)`: the whole string.
pub(crate) fn matches(rt: &mut Rt, s: &str, regex: &str) -> V {
    let Some(re) = regex_for(rt, regex, 0, Mode::Full) else { return V::I(0) };
    V::I(re.is_match(s) as i32)
}

/// `Pattern.quote`, Java's: `\Q…\E`, a `\E` inside it quoted on its own.
fn quote(s: &str) -> String {
    if !s.contains(r"\E") {
        return format!(r"\Q{s}\E");
    }
    let mut out = String::from(r"\Q");
    let mut rest = s;
    while let Some(at) = rest.find(r"\E") {
        out.push_str(&rest[..at]);
        out.push_str(r"\E\\E\Q");
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out.push_str(r"\E");
    out
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key
/// only to the modules that claim it ([`crate::api`]'s chain).
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner.starts_with("java/util/regex/")
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        PATTERN => pattern_call(rt, name, desc, args),
        MATCHER | MATCH_RESULT => matcher_call(rt, name, desc, args),
        SYNTAX_ERROR => Some(match name {
            "<init>" => {
                // `new PatternSyntaxException(description, regex, index)`.
                let msg = format!("{}\n{}", text(rt, a(1)), text(rt, a(2)));
                let m = new_str(rt, msg);
                rt.put_field(a(0), "message", m);
                rt.put_field(a(0), "$description", a(1));
                rt.put_field(a(0), "$pattern", a(2));
                rt.put_field(a(0), "$index", a(3));
                rt.supers.entry(SYNTAX_ERROR).or_insert(ILLEGAL_ARGUMENT);
                V::Void
            }
            "getDescription" => rt.get_field(a(0), "$description"),
            "getPattern" => rt.get_field(a(0), "$pattern"),
            "getIndex" => V::I(rt.get_field(a(0), "$index").i()),
            _ => return None,
        }),
        _ => None,
    }
}

fn pattern_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "compile" => {
            let flags = if desc.starts_with("(Ljava/lang/String;I)") { a(1).i() } else { 0 };
            pattern(rt, a(0), flags)
        }
        "matcher" => matcher(rt, a(0), a(1)),
        "pattern" | "toString" => rt.get_field(a(0), "$source"),
        "flags" => V::I(rt.get_field(a(0), "$flags").i()),
        "quote" => {
            let s = text(rt, a(0));
            new_str(rt, quote(&s))
        }
        "matches" => {
            // `Pattern.matches(regex, input)`.
            let (source, t) = (text(rt, a(0)), text(rt, a(1)));
            matches(rt, &t, &source)
        }
        "split" | "splitAsStream" => {
            let (source, flags) = source_of(rt, a(0));
            let t = text(rt, a(1));
            let limit = if desc.contains("Ljava/lang/CharSequence;I)") { a(2).i() } else { 0 };
            let re = regex_for(rt, &source, flags, Mode::Find)?;
            let parts = split_with(&re, &t, limit);
            if name == "split" {
                string_array(rt, parts)
            } else {
                let items: Vec<V> = parts.into_iter().map(|p| new_str(rt, p)).collect();
                rt.alloc("java/util/stream/Stream", Data::Stream(items))
            }
        }
        _ => return None,
    })
}

fn matcher_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let m = a(0);
    // A group's number from the call's argument: an int, or a name (`group("x")`).
    let group_number = |rt: &mut Rt| -> Option<usize> {
        if desc.starts_with("(Ljava/lang/String;)") {
            let n = text(rt, a(1));
            let p = rt.get_field(m, "$pattern");
            let (source, flags) = source_of(rt, p);
            let re = regex_for(rt, &source, flags, Mode::Find)?;
            match re.capture_names().position(|g| g == Some(n.as_str())) {
                Some(i) => Some(i),
                None => {
                    throw(rt, ILLEGAL_ARGUMENT, &format!("No group with name <{n}>"));
                    None
                }
            }
        } else if desc.starts_with("(I)") {
            Some(a(1).i().max(-1) as usize)
        } else {
            Some(0)
        }
    };
    Some(match name {
        "matches" | "lookingAt" => {
            let mode = if name == "matches" { Mode::Full } else { Mode::Prefix };
            V::I(search(rt, m, 0, mode) as i32)
        }
        "find" if desc.starts_with("(I)") => {
            let t = matcher_text(rt, m);
            let Some(from) = byte_at(&t, a(1).i()).filter(|_| a(1).i() >= 0) else {
                throw(rt, "java/lang/IndexOutOfBoundsException", &format!("Illegal start index"));
                return Some(V::I(0));
            };
            reset(rt, m);
            V::I(search(rt, m, from, Mode::Find) as i32)
        }
        "find" => V::I(find(rt, m) as i32),
        "group" | "start" | "end" => {
            let Some(g) = groups(rt, m) else {
                throw(rt, "java/lang/IllegalStateException", "No match found");
                return Some(V::NULL);
            };
            let Some(n) = group_number(rt) else { return Some(V::NULL) };
            if n >= g.len() {
                throw(rt, "java/lang/IndexOutOfBoundsException", &format!("No group {n}"));
                return Some(V::NULL);
            }
            let t = matcher_text(rt, m);
            match (name, g[n]) {
                ("group", Some((s, e))) => new_str(rt, t[s..e].to_owned()),
                ("group", None) => V::NULL,
                ("start", Some((s, _))) => V::I(u16_at(&t, s)),
                ("end", Some((_, e))) => V::I(u16_at(&t, e)),
                _ => V::I(-1),
            }
        }
        "groupCount" => {
            let p = rt.get_field(m, "$pattern");
            let (source, flags) = source_of(rt, p);
            let re = regex_for(rt, &source, flags, Mode::Find)?;
            V::I(re.captures_len() as i32 - 1)
        }
        "hasMatch" => V::I(groups(rt, m).is_some() as i32),
        "replaceAll" | "replaceFirst" if desc.starts_with("(Ljava/lang/String;)") => {
            let p = rt.get_field(m, "$pattern");
            let (source, flags) = source_of(rt, p);
            let re = regex_for(rt, &source, flags, Mode::Find)?;
            let (t, repl) = (matcher_text(rt, m), text(rt, a(1)));
            reset(rt, m);
            match replace_in(rt, &re, &t, &repl, name == "replaceFirst") {
                Some(r) => new_str(rt, r),
                None => V::NULL,
            }
        }
        "appendReplacement" => {
            let Some(g) = groups(rt, m) else {
                throw(rt, "java/lang/IllegalStateException", "No match available");
                return Some(V::NULL);
            };
            let p = rt.get_field(m, "$pattern");
            let (source, flags) = source_of(rt, p);
            let re = regex_for(rt, &source, flags, Mode::Find)?;
            let (t, repl) = (matcher_text(rt, m), text(rt, a(2)));
            let at = rt.get_field(m, "$append").i().max(0) as usize;
            let (s, e) = g[0].unwrap_or((at, at));
            match expand(&re, &t, &g, &repl) {
                Ok(r) => {
                    let piece = format!("{}{r}", t.get(at..s).unwrap_or(""));
                    append(rt, a(1), piece);
                    rt.put_field(m, "$append", V::I(e as i32));
                }
                Err((class, msg)) => throw(rt, class, &msg),
            }
            m
        }
        "appendTail" => {
            let t = matcher_text(rt, m);
            let at = rt.get_field(m, "$append").i().max(0) as usize;
            append(rt, a(1), t.get(at..).unwrap_or("").to_owned());
            a(1)
        }
        "reset" => {
            if desc.starts_with("(Ljava/lang/CharSequence;)") {
                let t = text(rt, a(1));
                let t = new_str(rt, t);
                rt.put_field(m, "$text", t);
            }
            reset(rt, m);
            m
        }
        "pattern" => rt.get_field(m, "$pattern"),
        "toMatchResult" => {
            let r = rt.new_object(MATCH_RESULT);
            for f in ["$pattern", "$text", "$groups", "$first", "$last"] {
                let v = rt.get_field(m, f);
                rt.put_field(r, f, v);
            }
            r
        }
        "results" => {
            let p = rt.get_field(m, "$pattern");
            let (source, flags) = source_of(rt, p);
            let re = regex_for(rt, &source, flags, Mode::Find)?;
            let t = matcher_text(rt, m);
            let mut items = Vec::new();
            for g in all_matches(&re, &t, false) {
                let r = rt.new_object(MATCH_RESULT);
                rt.put_field(r, "$pattern", p);
                let tv = rt.get_field(m, "$text");
                rt.put_field(r, "$text", tv);
                set_groups(rt, r, Some(&g));
                items.push(r);
            }
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        "quoteReplacement" => {
            let s = text(rt, a(0));
            let q: String = s.chars().flat_map(|c| if matches!(c, '\\' | '$') { vec!['\\', c] } else { vec![c] }).collect();
            new_str(rt, q)
        }
        "hitEnd" => {
            let t = matcher_text(rt, m);
            V::I((rt.get_field(m, "$last").i().max(0) as usize >= t.len()) as i32)
        }
        _ => return None,
    })
}

/// Appends `s` to the `StringBuilder` (or `StringBuffer`) `sb`.
fn append(rt: &mut Rt, sb: V, s: String) {
    let v = new_str(rt, s);
    crate::api::call(rt, "java/lang/StringBuilder.append.(Ljava/lang/String;)Ljava/lang/StringBuilder;", &[sb, v]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    fn strings(rt: &Rt, arr: V) -> Vec<String> {
        rt.captured(arr).into_iter().map(|v| rt.str_of(v).unwrap_or("?").to_owned()).collect()
    }

    /// Java's split, word for word (expected values from the JDK): a leading empty string only
    /// for a match that takes something, trailing empty ones dropped at limit 0, at most `limit`
    /// parts, the input itself when nothing matches, and an all-separator input empty.
    #[test]
    fn split_is_javas() {
        crate::install();
        let mut rt = Rt::new();
        let cases: [(&str, &str, i32, &[&str]); 9] = [
            ("a,b,,c,,", ",", 0, &["a", "b", "", "c"]),
            ("a,b,,c,,", ",", -1, &["a", "b", "", "c", "", ""]),
            ("a,b,,c,,", ",", 2, &["a", "b,,c,,"]),
            (",a", ",", 0, &["", "a"]),
            ("abc", "", 0, &["a", "b", "c"]),
            ("  hello   world ", "\\s+", 0, &["", "hello", "world"]),
            (",,,", ",", 0, &[]),
            ("", ",", 0, &[""]),
            ("key=value=more", "=", 2, &["key", "value=more"]),
        ];
        for (input, regex, limit, want) in cases {
            let got = split(&mut rt, input, regex, limit);
            assert_eq!(strings(&rt, got), want, "{input:?}.split({regex:?}, {limit})");
        }
        assert!(rt.thrown.is_none());
    }

    /// A pattern finds its matches with Java's groups and UTF-16 indexes, replaces with `$n` and
    /// `${name}` and `\$`, and its `\w`, `\d` and `\s` are ASCII as Java's.
    #[test]
    fn a_matcher_walks_and_replaces_as_javas() {
        crate::install();
        let mut rt = Rt::new();
        let src = rt.string(r"(?<key>\w+)=(\d+)");
        let p = c(&mut rt, "java/util/regex/Pattern.compile.(Ljava/lang/String;)Ljava/util/regex/Pattern;", &[src]);
        let input = rt.string("é a=1, bb=22; ü=3");
        let m = c(&mut rt, "java/util/regex/Pattern.matcher.(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;", &[p, input]);
        let mut found = Vec::new();
        while c(&mut rt, "java/util/regex/Matcher.find.()Z", &[m]).i() == 1 {
            let key = rt.string("key");
            let k = c(&mut rt, "java/util/regex/Matcher.group.(Ljava/lang/String;)Ljava/lang/String;", &[m, key]);
            let v = c(&mut rt, "java/util/regex/Matcher.group.(I)Ljava/lang/String;", &[m, V::I(2)]);
            let s = c(&mut rt, "java/util/regex/Matcher.start.()I", &[m]).i();
            let e = c(&mut rt, "java/util/regex/Matcher.end.()I", &[m]).i();
            found.push(format!("{}:{}@{s}-{e}", rt.str_of(k).unwrap_or(""), rt.str_of(v).unwrap_or("")));
        }
        // `ü` isn't a word character to Java: `ü=3` doesn't match.
        assert_eq!(found, ["a:1@2-5", "bb:22@7-12"]);
        assert_eq!(c(&mut rt, "java/util/regex/Matcher.groupCount.()I", &[m]).i(), 2);
        let repl = rt.string(r"${key}->$2\$");
        let out = c(&mut rt, "java/util/regex/Matcher.replaceAll.(Ljava/lang/String;)Ljava/lang/String;", &[m, repl]);
        assert_eq!(rt.str_of(out), Some("é a->1$, bb->22$; ü=3"));
        // `matches` is the whole input, `lookingAt` its start.
        let whole = rt.string("x=1");
        let m2 = c(&mut rt, "java/util/regex/Pattern.matcher.(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;", &[p, whole]);
        assert_eq!(c(&mut rt, "java/util/regex/Matcher.matches.()Z", &[m2]).i(), 1);
        let longer = rt.string("x=1 and more");
        c(&mut rt, "java/util/regex/Matcher.reset.(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;", &[m2, longer]);
        assert_eq!(c(&mut rt, "java/util/regex/Matcher.matches.()Z", &[m2]).i(), 0);
        assert_eq!(c(&mut rt, "java/util/regex/Matcher.lookingAt.()Z", &[m2]).i(), 1);
        assert!(rt.thrown.is_none());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Java's syntax read into Rust's: a quoted part (`Pattern.quote`, a `\E` inside it too), the
    /// flags, a possessive quantifier; what Rust has no form for is Java's `PatternSyntaxException`,
    /// which a `catch (IllegalArgumentException e)` catches.
    #[test]
    fn javas_syntax_compiles_or_throws_as_javas() {
        crate::install();
        let mut rt = Rt::new();
        let quoted = quote(r"a.b\Ec");
        assert_eq!(quoted, r"\Qa.b\E\\E\Qc\E");
        let re = compiled(&quoted, 0, Mode::Full).unwrap();
        assert!(re.is_match(r"a.b\Ec") && !re.is_match(r"axb\Ec"));
        assert!(compiled("HELLO", CASE_INSENSITIVE, Mode::Full).unwrap().is_match("hello"));
        assert!(compiled("a++b", 0, Mode::Full).unwrap().is_match("aaab"));
        assert!(compiled(r"[\w-]+\.[a-z&&[^aeiou]]+", 0, Mode::Full).unwrap().is_match("my-mod.txt"));
        assert!(compiled(r"(?u)\p{Alpha}+", 0, Mode::Full).unwrap().is_match("abc"));
        let src = rt.string("(?<=a)b");
        let p = c(&mut rt, "java/util/regex/Pattern.compile.(Ljava/lang/String;)Ljava/util/regex/Pattern;", &[src]);
        assert_eq!(p, V::NULL);
        let e = rt.thrown.take().expect("thrown");
        assert!(rt.instance_of(e, "java/lang/IllegalArgumentException"));
        assert_eq!(rt.class_of(e), SYNTAX_ERROR);
    }
}
