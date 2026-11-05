//! `java.text.BreakIterator`: where text may break, found as the Java the game runs (JDK 21) finds
//! it. Lines (`getLineInstance`: Create's tooltips and catnip's screens wrap their text with it),
//! words and sentences follow the JDK's own rule sets (`sun.text.resources.BreakIteratorRules`);
//! characters are the extended grapheme clusters of Unicode 15.0 (`getCharacterInstance` since
//! JDK 20). Also `java.text.StringCharacterIterator`, the text an iterator may be given or hand back.
//!
//! An iterator finds its text's boundaries once, when the text is set, and keeps them with the one
//! it stands at: `next`, `previous`, `following`, `preceding` and `isBoundary` search that list.
use crate::{Data, Rt, V};

/// `BreakIterator.DONE`.
const DONE: i32 = -1;
/// `CharacterIterator.DONE`.
const CHAR_DONE: i32 = 0xffff;
const STRING_ITERATOR: &str = "java/text/StringCharacterIterator";

/// The rules an iterator breaks by (its `$kind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Character = 0,
    Word = 1,
    Line = 2,
    Sentence = 3,
}

/// `BreakIterator`'s methods; None for one it doesn't have.
pub(crate) fn call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let it = a(0);
    Some(match name {
        "getCharacterInstance" => new_iterator(rt, Kind::Character),
        "getWordInstance" => new_iterator(rt, Kind::Word),
        "getLineInstance" => new_iterator(rt, Kind::Line),
        "getSentenceInstance" => new_iterator(rt, Kind::Sentence),
        // A mod's own iterator, built on this class.
        "<init>" => V::Void,
        "setText" => {
            set_text(rt, it, a(1), desc.starts_with("(Ljava/lang/String;"));
            V::Void
        }
        "getText" => text_of(rt, it),
        "first" => {
            let b = first(rt, it);
            rt.put_field(it, "$cur", V::I(b));
            V::I(b)
        }
        "last" => {
            let b = last(rt, it);
            rt.put_field(it, "$cur", V::I(b));
            V::I(b)
        }
        "current" => V::I(rt.get_field(it, "$cur").i()),
        "next" if desc == "()I" => V::I(step(rt, it, 1)),
        "next" => V::I(step(rt, it, a(1).i())),
        "previous" => V::I(step(rt, it, -1)),
        "following" | "preceding" | "isBoundary" => {
            let offset = a(1).i();
            let start = first(rt, it);
            if offset < start || offset > last(rt, it) {
                crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", "offset out of bounds");
                return Some(V::I(DONE));
            }
            match name {
                "following" => V::I(following(rt, it, offset)),
                "preceding" => {
                    rt.put_field(it, "$cur", V::I(offset));
                    V::I(step(rt, it, -1))
                }
                _ => V::I(i32::from(offset == start || following(rt, it, offset - 1) == offset)),
            }
        }
        "clone" => {
            let copy = rt.new_object("java/text/BreakIterator");
            for field in ["$kind", "$b", "$cur", "$text", "$iter"] {
                let v = rt.get_field(it, field);
                rt.put_field(copy, field, v);
            }
            copy
        }
        _ => return None,
    })
}

fn new_iterator(rt: &mut Rt, kind: Kind) -> V {
    let it = rt.new_object("java/text/BreakIterator");
    rt.put_field(it, "$kind", V::I(kind as i32));
    it
}

fn kind_of(rt: &Rt, it: V) -> Kind {
    match rt.get_field(it, "$kind").i() {
        0 => Kind::Character,
        1 => Kind::Word,
        3 => Kind::Sentence,
        _ => Kind::Line,
    }
}

/// `setText(String)` or `setText(CharacterIterator)`: the boundaries found, the iterator at the
/// first. A `CharacterIterator`'s indexes are its own (from its begin index) and so are the
/// boundaries; its characters are read once, here, and it is left at its first, as the JDK leaves it.
fn set_text(rt: &mut Rt, it: V, text: V, string: bool) {
    let (units, begin) = if string {
        (rt.str_of(text).unwrap_or("").encode_utf16().collect(), 0)
    } else {
        read_characters(rt, text)
    };
    let found = boundaries(kind_of(rt, it), &units).into_iter().map(|b| V::I(b + begin)).collect();
    let list = rt.alloc("[I", Data::Array(found));
    rt.put_field(it, "$b", list);
    rt.put_field(it, "$cur", V::I(begin));
    rt.put_field(it, "$iter", if string { V::NULL } else { text });
    let text = if string { text } else { rt.alloc("java/lang/String", Data::Str(String::from_utf16_lossy(&units))) };
    rt.put_field(it, "$text", text);
}

/// A `CharacterIterator`'s characters (ours, or a mod's own class asked through its methods) and its
/// begin index.
fn read_characters(rt: &mut Rt, iter: V) -> (Vec<u16>, i32) {
    if iter.r() == 0 {
        return (Vec::new(), 0);
    }
    if rt.class_of(iter) == STRING_ITERATOR {
        let (units, begin, end, _) = string_iterator(rt, iter);
        rt.put_field(iter, "$pos", V::I(begin));
        return (units[begin as usize..end as usize].to_vec(), begin);
    }
    let begin = rt.call_functional("java/text/CharacterIterator.getBeginIndex.()I", iter, &[]).i();
    let end = rt.call_functional("java/text/CharacterIterator.getEndIndex.()I", iter, &[]).i();
    let mut units = Vec::with_capacity((end - begin).max(0) as usize);
    for i in begin..end {
        let c = rt.call_functional("java/text/CharacterIterator.setIndex.(I)C", iter, &[V::I(i)]);
        if rt.thrown.is_some() {
            break;
        }
        units.push(c.i() as u16);
    }
    if rt.thrown.is_none() {
        rt.call_functional("java/text/CharacterIterator.first.()C", iter, &[]);
    }
    (units, begin)
}

/// `getText()`: the `CharacterIterator` the text was given as, else one over the string, at the
/// boundary the iterator stands at.
fn text_of(rt: &mut Rt, it: V) -> V {
    let iter = rt.get_field(it, "$iter");
    if iter.r() != 0 {
        return iter;
    }
    let text = match rt.get_field(it, "$text") {
        t if t.r() != 0 => t,
        _ => rt.string(""),
    };
    let len = rt.str_of(text).map_or(0, |s| s.encode_utf16().count() as i32);
    let cur = rt.get_field(it, "$cur").i();
    new_string_iterator(rt, text, 0, len, cur)
}

/// The boundaries, ascending (none before a text is set: the empty text's).
fn list(rt: &Rt, it: V) -> &[V] {
    match rt.heap.get(rt.get_field(it, "$b").r() as usize).map(|o| &o.data) {
        Some(Data::Array(b)) if rt.get_field(it, "$b").r() != 0 => b,
        _ => &[],
    }
}

fn first(rt: &Rt, it: V) -> i32 {
    list(rt, it).first().map_or(0, |b| b.i())
}

fn last(rt: &Rt, it: V) -> i32 {
    list(rt, it).last().map_or(0, |b| b.i())
}

/// `next()` `n` times (`previous()` for a negative `n`): the boundary reached, or DONE past either
/// end, where the iterator stays.
fn step(rt: &mut Rt, it: V, n: i32) -> i32 {
    let mut cur = rt.get_field(it, "$cur").i();
    let mut result = cur;
    for _ in 0..n.unsigned_abs() {
        let b = list(rt, it);
        let found = if n > 0 {
            b.get(b.partition_point(|v| v.i() <= cur)).map(|v| v.i())
        } else {
            b.partition_point(|v| v.i() < cur).checked_sub(1).map(|k| b[k].i())
        };
        match found {
            Some(x) => (cur, result) = (x, x),
            None => {
                result = DONE;
                break;
            }
        }
    }
    rt.put_field(it, "$cur", V::I(cur));
    result
}

/// `following(offset)`: the first boundary after `offset`; DONE at the end, where the iterator then
/// stands.
fn following(rt: &mut Rt, it: V, offset: i32) -> i32 {
    let b = list(rt, it);
    let found = b.get(b.partition_point(|v| v.i() <= offset)).map(|v| v.i());
    let end = last(rt, it);
    rt.put_field(it, "$cur", V::I(found.unwrap_or(end)));
    found.unwrap_or(DONE)
}

/// `StringCharacterIterator`, and `CharacterIterator`'s methods on one: None for another class's.
pub(crate) fn string_iterator_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let it = a(0);
    if rt.class_of(it) != STRING_ITERATOR {
        return None;
    }
    let (units, begin, end, pos) = string_iterator(rt, it);
    let at = |rt: &mut Rt, p: i32| {
        rt.put_field(it, "$pos", V::I(p));
        V::I(if p >= begin && p < end { i32::from(units[p as usize]) } else { CHAR_DONE })
    };
    Some(match (name, desc) {
        ("<init>" | "setText", _) => {
            let len = rt.str_of(a(1)).map_or(0, |s| s.encode_utf16().count() as i32);
            let (b, e, p) = match desc {
                "(Ljava/lang/String;I)V" => (0, len, a(2).i()),
                "(Ljava/lang/String;III)V" => (a(2).i(), a(3).i(), a(4).i()),
                _ => (0, len, 0),
            };
            if b < 0 || b > e || e > len || p < b || p > e {
                crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", "Invalid substring range");
                return Some(V::Void);
            }
            for (f, v) in [("$text", a(1)), ("$begin", V::I(b)), ("$end", V::I(e)), ("$pos", V::I(p))] {
                rt.put_field(it, f, v);
            }
            V::Void
        }
        ("first", _) => at(rt, begin),
        ("last", _) => at(rt, if end > begin { end - 1 } else { end }),
        ("current", _) => at(rt, pos),
        ("next", _) if pos < end - 1 => at(rt, pos + 1),
        ("next", _) => at(rt, end),
        ("previous", _) if pos > begin => at(rt, pos - 1),
        ("previous", _) => V::I(CHAR_DONE),
        ("setIndex", _) if a(1).i() < begin || a(1).i() > end => {
            crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", "Invalid index");
            V::I(CHAR_DONE)
        }
        ("setIndex", _) => at(rt, a(1).i()),
        ("getBeginIndex", _) => V::I(begin),
        ("getEndIndex", _) => V::I(end),
        ("getIndex", _) => V::I(pos),
        ("clone", _) => {
            let text = rt.get_field(it, "$text");
            new_string_iterator(rt, text, begin, end, pos)
        }
        _ => return None,
    })
}

fn new_string_iterator(rt: &mut Rt, text: V, begin: i32, end: i32, pos: i32) -> V {
    let it = rt.new_object(STRING_ITERATOR);
    for (f, v) in [("$text", text), ("$begin", V::I(begin)), ("$end", V::I(end)), ("$pos", V::I(pos))] {
        rt.put_field(it, f, v);
    }
    it
}

/// A `StringCharacterIterator`'s text (UTF-16), begin, end and index.
fn string_iterator(rt: &Rt, it: V) -> (Vec<u16>, i32, i32, i32) {
    let units: Vec<u16> = rt.str_of(rt.get_field(it, "$text")).unwrap_or("").encode_utf16().collect();
    let len = units.len() as i32;
    let end = rt.get_field(it, "$end").i().clamp(0, len);
    let begin = rt.get_field(it, "$begin").i().clamp(0, end);
    let pos = rt.get_field(it, "$pos").i().clamp(begin, end);
    (units, begin, end, pos)
}

/// The boundaries of `text` (UTF-16, as Java indexes it) by `kind`'s rules: 0, every break, and
/// the end, ascending.
pub(crate) fn boundaries(kind: Kind, text: &[u16]) -> Vec<i32> {
    let points = code_points(text);
    let classify: fn(u32) -> u32 = match kind {
        Kind::Line => line_class,
        Kind::Word => word_class,
        Kind::Sentence => sentence_class,
        Kind::Character => |_| 0,
    };
    let masks: Vec<u32> = points.iter().map(|&(c, _)| classify(c)).collect();
    let mut out = vec![0];
    let mut at = 0;
    while at < points.len() {
        let taken = match kind {
            Kind::Character => grapheme(&points, at),
            Kind::Sentence => sentence(&masks, at),
            Kind::Line => rules(Kind::Line).longest(&masks, at),
            Kind::Word => rules(Kind::Word).longest(&masks, at),
        };
        // An iterator moves on by a character at least, whatever its rules match.
        at += taken.max(1);
        out.push(points[at - 1].1 as i32);
    }
    out
}

/// The text's code points, each with the UTF-16 index just past it (an unpaired surrogate is a
/// code point of its own, as the JDK's iterators read it).
fn code_points(text: &[u16]) -> Vec<(u32, usize)> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let u = u32::from(text[i]);
        match text.get(i + 1).map(|&l| u32::from(l)) {
            Some(l) if (0xd800..0xdc00).contains(&u) && (0xdc00..0xe000).contains(&l) => {
                out.push((0x10000 + ((u - 0xd800) << 10) + (l - 0xdc00), i + 2));
                i += 2;
            }
            _ => {
                out.push((u, i + 1));
                i += 1;
            }
        }
    }
    out
}

// ---- The rules as patterns over character classes ----

/// A rule's pattern, as the JDK's rule text writes it, over classes of characters (bit masks: a
/// character may be in several).
#[derive(Clone)]
enum Re {
    Class(u32),
    Seq(Vec<Re>),
    Alt(Vec<Re>),
    Star(Box<Re>),
    Opt(Box<Re>),
}

fn c(mask: u32) -> Re {
    Re::Class(mask)
}

fn seq<const N: usize>(parts: [Re; N]) -> Re {
    Re::Seq(parts.into())
}

fn alt<const N: usize>(parts: [Re; N]) -> Re {
    Re::Alt(parts.into())
}

fn star(r: Re) -> Re {
    Re::Star(Box::new(r))
}

fn plus(r: Re) -> Re {
    seq([r.clone(), star(r)])
}

fn opt(r: Re) -> Re {
    Re::Opt(Box::new(r))
}

enum Node {
    Accept,
    Class(u32, usize),
    Split(usize, usize),
}

/// A rule set made a state machine (Thompson's construction); a match is the longest any of its
/// rules makes, as the JDK's tables find it.
struct Nfa {
    nodes: Vec<Node>,
    start: usize,
    /// The class of characters passed over in whatever state the match is in.
    ignore: u32,
}

impl Nfa {
    fn new(rules: &Re, ignore: u32) -> Nfa {
        let mut nodes = vec![Node::Accept];
        let start = Self::compile(rules, 0, &mut nodes);
        Nfa { nodes, start, ignore }
    }

    /// Adds `re`'s states, leading to `next`: its entry.
    fn compile(re: &Re, next: usize, nodes: &mut Vec<Node>) -> usize {
        match re {
            Re::Class(m) => {
                nodes.push(Node::Class(*m, next));
                nodes.len() - 1
            }
            Re::Seq(parts) => parts.iter().rev().fold(next, |n, r| Self::compile(r, n, nodes)),
            Re::Alt(parts) => {
                let mut entries: Vec<usize> = parts.iter().map(|r| Self::compile(r, next, nodes)).collect();
                let mut entry = entries.pop().unwrap_or(next);
                while let Some(e) = entries.pop() {
                    nodes.push(Node::Split(e, entry));
                    entry = nodes.len() - 1;
                }
                entry
            }
            Re::Opt(r) => {
                let e = Self::compile(r, next, nodes);
                nodes.push(Node::Split(e, next));
                nodes.len() - 1
            }
            Re::Star(r) => {
                nodes.push(Node::Split(usize::MAX, next));
                let split = nodes.len() - 1;
                let e = Self::compile(r, split, nodes);
                nodes[split] = Node::Split(e, next);
                split
            }
        }
    }

    /// How many characters from `from` (by their classes) the longest match takes; 0 for none. A
    /// character of the ignored class leaves the state as it is, and joins the match when that
    /// state accepts.
    fn longest(&self, masks: &[u32], from: usize) -> usize {
        let mut seen = vec![usize::MAX; self.nodes.len()];
        let (mut now, mut next) = (Vec::new(), Vec::new());
        let mut accepting = self.closure(self.start, 0, &mut seen, &mut now);
        let mut best = 0;
        for (k, &m) in masks[from..].iter().enumerate() {
            if m == self.ignore {
                if accepting {
                    best = k + 1;
                }
                continue;
            }
            next.clear();
            accepting = false;
            for &s in &now {
                if let Node::Class(cm, to) = self.nodes[s]
                    && cm & m != 0
                {
                    accepting |= self.closure(to, k + 1, &mut seen, &mut next);
                }
            }
            if accepting {
                best = k + 1;
            }
            if next.is_empty() {
                break;
            }
            std::mem::swap(&mut now, &mut next);
        }
        best
    }

    /// The character-reading states `s` reaches without reading one, added to `out` (once per
    /// `generation`); whether it reaches the end.
    fn closure(&self, s: usize, generation: usize, seen: &mut [usize], out: &mut Vec<usize>) -> bool {
        let mut stack = vec![s];
        let mut accepts = false;
        while let Some(s) = stack.pop() {
            if seen[s] == generation {
                continue;
            }
            seen[s] = generation;
            match self.nodes[s] {
                Node::Accept => accepts = true,
                Node::Class(..) => out.push(s),
                Node::Split(a, b) => stack.extend([b, a]),
            }
        }
        accepts
    }
}

fn rules(kind: Kind) -> &'static Nfa {
    static LINE: std::sync::OnceLock<Nfa> = std::sync::OnceLock::new();
    static WORD: std::sync::OnceLock<Nfa> = std::sync::OnceLock::new();
    match kind {
        Kind::Word => WORD.get_or_init(|| Nfa::new(&word_rules(), word::IGNORE)),
        _ => LINE.get_or_init(|| Nfa::new(&line_rules(), line::IGNORE)),
    }
}

/// `LineBreakRules`' classes.
mod line {
    pub const BREAK: u32 = 1;
    pub const CR: u32 = 1 << 1;
    pub const IGNORE: u32 = 1 << 2;
    pub const ENCLOSING: u32 = 1 << 3;
    pub const DANDA: u32 = 1 << 4;
    pub const GLUE: u32 = 1 << 5;
    pub const SPACE: u32 = 1 << 6;
    pub const DASH: u32 = 1 << 7;
    pub const PRE: u32 = 1 << 8;
    pub const POST: u32 = 1 << 9;
    pub const KANJI: u32 = 1 << 10;
    pub const DIGIT: u32 = 1 << 11;
    pub const MID_NUM: u32 = 1 << 12;
    pub const CHAR: u32 = 1 << 13;
    /// `<hack1>`, `<hack2>` and `<hack3>`: `(`, `)`, and `$` or `'`.
    pub const OPEN: u32 = 1 << 14;
    pub const CLOSE: u32 = 1 << 15;
    pub const JOIN: u32 = 1 << 16;
}

/// A line: words (an optional run of prefix marks, then letters and the like, one ideograph, or a
/// number; then dashes or suffix marks), joined by non-breaking spaces, then its spaces and its line
/// break. It may break after it.
fn line_rules() -> Re {
    use line::*;
    let number = seq([star(c(PRE | DASH)), plus(c(DIGIT)), star(seq([c(MID_NUM), plus(c(DIGIT))]))]);
    let core = alt([star(c(CHAR)), c(KANJI), number]);
    let suffix = alt([plus(c(DASH)), star(c(POST))]);
    let word = seq([star(c(PRE)), core, suffix]);
    let join = alt([seq([star(c(SPACE)), plus(c(GLUE)), opt(c(SPACE))]), c(JOIN)]);
    alt([
        seq([
            word.clone(),
            star(seq([join, word])),
            star(c(SPACE)),
            star(c(ENCLOSING)),
            opt(seq([c(OPEN), c(CLOSE), star(c(POST))])),
            star(c(ENCLOSING)),
            opt(c(CR)),
            opt(c(BREAK)),
        ]),
        seq([c(CR), c(BREAK)]),
    ])
}

fn line_class(ch: u32) -> u32 {
    use line::*;
    match ch {
        0x03 | 0x09 | 0x0a | 0x0c | 0x2028 | 0x2029 => return BREAK,
        0x0d => return CR,
        _ => {}
    }
    let glue = matches!(ch, 0xa0 | 0xf0c | 0x2007 | 0x2011 | 0x202f | 0xfeff);
    let mut m = 0;
    if glue {
        m |= GLUE;
    } else {
        // A control character is a space here; a format character, unless a class names it, is
        // passed over.
        if is_space_separator(ch) || is_control(ch) {
            m |= SPACE;
        }
        if within(DASHES, ch) || ch == 0xad {
            m |= DASH;
        }
        if m == 0 && within(FORMATS, ch) {
            return IGNORE;
        }
    }
    if matches!(ch, 0x964 | 0x965) {
        m |= DANDA;
    }
    if within(MARKS, ch) {
        m |= ENCLOSING;
    }
    if (ch != 0xa2 && (within(CURRENCY, ch) || OPENS.binary_search(&ch).is_ok() || INITIALS.binary_search(&ch).is_ok())) || ch == 0x22 || ch == 0x27 {
        m |= PRE;
    }
    if ch == 0x22 || CLOSES.binary_search(&ch).is_ok() || FINALS.binary_search(&ch).is_ok() || matches!(ch, 0x21 | 0x25 | 0x2e | 0x2c | 0x3a | 0x3b | 0x3f) || within(LINE_POST, ch) {
        m |= POST;
    } else if within(LINE_KANJI, ch) {
        m |= KANJI;
    }
    if is_numeric(ch) && !within(LETTER_NUMBERS, ch) {
        m |= DIGIT;
    }
    if matches!(ch, 0x2e | 0x2c) {
        m |= MID_NUM;
    }
    match ch {
        0x28 => m |= OPEN,
        0x29 => m |= CLOSE,
        0x24 | 0x27 => m |= JOIN,
        _ => {}
    }
    if m & (SPACE | DASH | KANJI | GLUE | PRE | POST | MID_NUM | DANDA) == 0 {
        m |= CHAR;
    }
    m
}

/// `WordBreakRules`' classes.
mod word {
    pub const IGNORE: u32 = 1;
    pub const ENCLOSING: u32 = 1 << 1;
    pub const DANDA: u32 = 1 << 2;
    pub const KANJI: u32 = 1 << 3;
    pub const KATA: u32 = 1 << 4;
    pub const HIRA: u32 = 1 << 5;
    pub const CJK_DIACRITIC: u32 = 1 << 6;
    pub const LETTER: u32 = 1 << 7;
    pub const DIGIT: u32 = 1 << 8;
    pub const MID_WORD: u32 = 1 << 9;
    pub const MID_NUM: u32 = 1 << 10;
    pub const PRE_NUM: u32 = 1 << 11;
    pub const POST_NUM: u32 = 1 << 12;
    pub const LINE_SEPARATOR: u32 = 1 << 13;
    pub const WHITESPACE: u32 = 1 << 14;
    pub const CR: u32 = 1 << 15;
    pub const BASE: u32 = 1 << 16;
    pub const ANY: u32 = 1 << 17;
}

/// A word: letters with single inner marks (`don't`, `e.g`, `well-known`), numbers with single
/// inner separators, the two run together (`v1`, `256su`); a run of spaces with its line break; a
/// run of katakana, of hiragana, of ideographs; anything else a character at a time.
fn word_rules() -> Re {
    use word::*;
    let letter = seq([c(LETTER), star(c(ENCLOSING))]);
    let digit = seq([c(DIGIT), star(c(ENCLOSING))]);
    let word = seq([plus(letter.clone()), star(seq([c(MID_WORD), plus(letter)])), opt(c(DANDA))]);
    let number = seq([plus(digit.clone()), star(seq([c(MID_NUM), plus(digit)]))]);
    let tail = seq([star(seq([number.clone(), word.clone()])), opt(seq([number, opt(c(POST_NUM))]))]);
    alt([
        c(ANY),
        seq([opt(word), tail.clone()]),
        seq([c(PRE_NUM), tail]),
        seq([star(seq([c(WHITESPACE), star(c(ENCLOSING))])), opt(c(CR)), opt(c(LINE_SEPARATOR))]),
        star(c(KATA | CJK_DIACRITIC)),
        star(c(HIRA | CJK_DIACRITIC)),
        star(c(KANJI)),
        seq([c(BASE), plus(c(ENCLOSING))]),
    ])
}

fn word_class(ch: u32) -> u32 {
    use word::*;
    let mid_word = within(DASHES, ch) || within(CONNECTORS, ch) || matches!(ch, 0xad | 0x2027 | 0x22 | 0x27 | 0x2e);
    if within(FORMATS, ch) && !mid_word {
        return IGNORE;
    }
    let mut m = ANY;
    let enclosing = within(MARKS, ch);
    if enclosing {
        m |= ENCLOSING;
    }
    if matches!(ch, 0x964 | 0x965) {
        m |= DANDA;
    }
    let cjk = match ch {
        0x3005 | 0x4e00..=0x9fa5 | 0xf900..=0xfa2d => KANJI,
        0x30a1..=0x30fa | 0x30fd | 0x30fe => KATA,
        0x3041..=0x3094 | 0x309d | 0x309e => HIRA,
        0x3099..=0x309c | 0x30fb | 0x30fc => CJK_DIACRITIC,
        _ => 0,
    };
    m |= cjk;
    if cjk == 0 && !enclosing && is_alphabetic(ch) && !is_numeric(ch) {
        m |= LETTER;
    }
    if is_numeric(ch) {
        m |= DIGIT;
    }
    if mid_word {
        m |= MID_WORD;
    }
    if matches!(ch, 0x22 | 0x27 | 0x2c | 0x66b | 0x2e) {
        m |= MID_NUM;
    }
    if ch != 0xa2 && (within(CURRENCY, ch) || matches!(ch, 0x23 | 0x2e)) {
        m |= PRE_NUM;
    }
    if matches!(ch, 0x25 | 0x26 | 0xa2 | 0x66a | 0x2030 | 0x2031) {
        m |= POST_NUM;
    }
    match ch {
        0x0a | 0x0c | 0x2028 | 0x2029 => m |= LINE_SEPARATOR,
        0x0d => m |= CR,
        0x09 => m |= WHITESPACE,
        _ if is_space_separator(ch) => m |= WHITESPACE,
        _ => {}
    }
    if !enclosing && !is_control(ch) && !within(FORMATS, ch) && ch != 0x2028 && ch != 0x2029 {
        m |= BASE;
    }
    m
}

/// `SentenceBreakRules`' classes.
mod sentence {
    pub const IGNORE: u32 = 1;
    pub const LETTER: u32 = 1 << 1;
    pub const LOWER: u32 = 1 << 2;
    pub const SPACE: u32 = 1 << 3;
    pub const START: u32 = 1 << 4;
    pub const END: u32 = 1 << 5;
    pub const DIGIT: u32 = 1 << 6;
    pub const TERM: u32 = 1 << 7;
    pub const PERIOD: u32 = 1 << 8;
    pub const COMMA: u32 = 1 << 9;
    pub const PARAGRAPH: u32 = 1 << 10;
    pub const DANDA: u32 = 1 << 11;
    pub const SENTENCE_START: u32 = 1 << 12;
}

fn sentence_class(ch: u32) -> u32 {
    use sentence::*;
    if within(MARKS, ch) || within(FORMATS, ch) {
        return IGNORE;
    }
    let mut m = 0;
    if is_alphabetic(ch) && !is_numeric(ch) {
        m |= LETTER;
        if char::from_u32(ch).is_some_and(char::is_lowercase) {
            m |= LOWER;
        }
    }
    if matches!(ch, 0x09 | 0x0d | 0x0c | 0x0a | 0x2028) || is_space_separator(ch) {
        m |= SPACE;
    }
    if OPENS.binary_search(&ch).is_ok() || INITIALS.binary_search(&ch).is_ok() || ch == 0x22 || ch == 0x27 {
        m |= START;
    }
    if CLOSES.binary_search(&ch).is_ok() || FINALS.binary_search(&ch).is_ok() || ch == 0x22 || ch == 0x27 {
        m |= END;
    }
    if is_numeric(ch) {
        m |= DIGIT;
    }
    match ch {
        0x21 | 0x3f | 0x3002 | 0xff01 | 0xff1f => m |= TERM,
        0x2e | 0xff0e => m |= PERIOD,
        0x2c => m |= COMMA,
        0x2029 => m |= PARAGRAPH,
        0x964 | 0x965 => m |= DANDA,
        _ => {}
    }
    if m & !DANDA == 0 {
        m |= SENTENCE_START;
    }
    m
}

/// How many characters from `at` a sentence takes: to a paragraph separator; past a danda and
/// its spaces; past `!` or `?` and the marks and spaces after; past a period, the marks and spaces
/// after it, when a sentence plainly starts next ([`after_period`]); else to the end.
fn sentence(masks: &[u32], at: usize) -> usize {
    use sentence::*;
    let past = |mut j: usize, class: u32| {
        while masks.get(j).is_some_and(|&m| m & (class | IGNORE) != 0) {
            j += 1;
        }
        j
    };
    for (i, &m) in masks.iter().enumerate().skip(at) {
        if m & PARAGRAPH != 0 {
            return i + 1 - at;
        }
        if m & DANDA != 0 {
            return past(i + 1, SPACE) - at;
        }
        if m & TERM != 0 {
            let j = past(past(i + 1, TERM | PERIOD | END), SPACE);
            return j + usize::from(masks.get(j).is_some_and(|&m| m & PARAGRAPH != 0)) - at;
        }
        if m & PERIOD != 0
            && let Some(end) = after_period(masks, i)
        {
            return end - at;
        }
    }
    masks.len() - at
}

/// After the period at `i`, the periods and closing marks that follow it and the spaces after
/// them: where the sentence ends when, after at least one space, a letter that isn't lowercase
/// comes, or (spaces or not) a run of opening or other marks and then a letter. The end is where
/// the JDK's iterator last stood before the marks: past a quote that both closes and opens.
fn after_period(masks: &[u32], i: usize) -> Option<usize> {
    use sentence::*;
    // In the period's run of marks, in its spaces, in the run of marks before a letter.
    let (mut marks, mut spaces, mut opening) = (true, false, false);
    let mut end = i + 1;
    for (j, &m) in masks.iter().enumerate().skip(i + 1) {
        if m == IGNORE {
            if marks || spaces {
                end = j + 1;
            }
            continue;
        }
        if m & LETTER != 0 && ((spaces && m & LOWER == 0) || opening) {
            return Some(end);
        }
        let opens = m & (START | SENTENCE_START) != 0;
        (marks, spaces, opening) = (marks && m & (PERIOD | END) != 0, (marks || spaces) && m & SPACE != 0, (marks || spaces || opening) && opens);
        if !(marks || spaces || opening) {
            return None;
        }
        if marks || spaces {
            end = j + 1;
        }
    }
    None
}

/// A character's class for grapheme clusters (Unicode's `Grapheme_Cluster_Break`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Grapheme {
    Cr,
    Lf,
    Control,
    Extend,
    Zwj,
    Regional,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
    Pictographic,
    Other,
}

fn grapheme_class(ch: u32) -> Grapheme {
    use Grapheme::*;
    match ch {
        0x0d => Cr,
        0x0a => Lf,
        0x200c | 0x1f3fb..=0x1f3ff | 0xe0020..=0xe007f | 0xff9e | 0xff9f => Extend,
        0x200d => Zwj,
        0x1f1e6..=0x1f1ff => Regional,
        0x1100..=0x115f | 0xa960..=0xa97c => L,
        0x1160..=0x11a7 | 0xd7b0..=0xd7c6 => V,
        0x11a8..=0x11ff | 0xd7cb..=0xd7fb => T,
        0xac00..=0xd7a3 if (ch - 0xac00) % 28 == 0 => Lv,
        0xac00..=0xd7a3 => Lvt,
        0x0e33 | 0x0eb3 => SpacingMark,
        _ if within(PREPENDED, ch) => Prepend,
        _ if is_control(ch) || ch == 0x2028 || ch == 0x2029 || within(FORMATS, ch) => Control,
        _ if within(MARKS, ch) || within(EXTENDING_SPACING_MARKS, ch) => Extend,
        _ if within(SPACING_MARKS, ch) => SpacingMark,
        _ if within(PICTOGRAPHS, ch) => Pictographic,
        _ => Other,
    }
}

/// How many code points from `at` its extended grapheme cluster takes.
fn grapheme(points: &[(u32, usize)], at: usize) -> usize {
    use Grapheme::*;
    let mut prev = grapheme_class(points[at].0);
    // In a pictograph and its extenders; a joiner after them; how many flags letters in a row.
    let mut pictograph = prev == Pictographic;
    let mut joined_pictograph = false;
    let mut regional = usize::from(prev == Regional);
    let mut i = at + 1;
    while let Some(&(ch, _)) = points.get(i) {
        let next = grapheme_class(ch);
        let stays = match (prev, next) {
            (Cr, Lf) => true,
            (Cr | Lf | Control, _) | (_, Cr | Lf | Control) => false,
            (L, L | V | Lv | Lvt) | (Lv | V, V | T) | (Lvt | T, T) => true,
            (_, Extend | Zwj | SpacingMark) | (Prepend, _) => true,
            (Zwj, Pictographic) => joined_pictograph,
            (Regional, Regional) => regional % 2 == 1,
            _ => false,
        };
        if !stays {
            break;
        }
        joined_pictograph = next == Zwj && pictograph;
        pictograph = next == Pictographic || (next == Extend && pictograph);
        regional = if next == Regional { regional + 1 } else { 0 };
        prev = next;
        i += 1;
    }
    i - at
}

// ---- Unicode's classes, as far as the rules ask ----

fn within(table: &[(u32, u32)], ch: u32) -> bool {
    let k = table.partition_point(|&(_, hi)| hi < ch);
    table.get(k).is_some_and(|&(lo, _)| lo <= ch)
}

fn is_alphabetic(ch: u32) -> bool {
    char::from_u32(ch).is_some_and(char::is_alphabetic) && !within(MARKS, ch)
}

fn is_numeric(ch: u32) -> bool {
    char::from_u32(ch).is_some_and(char::is_numeric)
}

fn is_control(ch: u32) -> bool {
    ch < 0x20 || (0x7f..=0x9f).contains(&ch)
}

fn is_space_separator(ch: u32) -> bool {
    matches!(ch, 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x202f | 0x205f | 0x3000)
}

/// Nonspacing and enclosing marks (`Mn`, `Me`).
const MARKS: &[(u32, u32)] = &[
    (0x0300, 0x036f), (0x0483, 0x0489), (0x0591, 0x05bd), (0x05bf, 0x05bf), (0x05c1, 0x05c2), (0x05c4, 0x05c5), (0x05c7, 0x05c7),
    (0x0610, 0x061a), (0x064b, 0x065f), (0x0670, 0x0670), (0x06d6, 0x06dc), (0x06df, 0x06e4), (0x06e7, 0x06e8), (0x06ea, 0x06ed),
    (0x0711, 0x0711), (0x0730, 0x074a), (0x07a6, 0x07b0), (0x07eb, 0x07f3), (0x07fd, 0x07fd), (0x0816, 0x0819), (0x081b, 0x0823),
    (0x0825, 0x0827), (0x0829, 0x082d), (0x0859, 0x085b), (0x0898, 0x089f), (0x08ca, 0x08e1), (0x08e3, 0x0902), (0x093a, 0x093a),
    (0x093c, 0x093c), (0x0941, 0x0948), (0x094d, 0x094d), (0x0951, 0x0957), (0x0962, 0x0963), (0x0981, 0x0981), (0x09bc, 0x09bc),
    (0x09c1, 0x09c4), (0x09cd, 0x09cd), (0x09e2, 0x09e3), (0x09fe, 0x09fe), (0x0a01, 0x0a02), (0x0a3c, 0x0a3c), (0x0a41, 0x0a42),
    (0x0a47, 0x0a48), (0x0a4b, 0x0a4d), (0x0a51, 0x0a51), (0x0a70, 0x0a71), (0x0a75, 0x0a75), (0x0a81, 0x0a82), (0x0abc, 0x0abc),
    (0x0ac1, 0x0ac5), (0x0ac7, 0x0ac8), (0x0acd, 0x0acd), (0x0ae2, 0x0ae3), (0x0afa, 0x0aff), (0x0b01, 0x0b01), (0x0b3c, 0x0b3c),
    (0x0b3f, 0x0b3f), (0x0b41, 0x0b44), (0x0b4d, 0x0b4d), (0x0b55, 0x0b56), (0x0b62, 0x0b63), (0x0b82, 0x0b82), (0x0bc0, 0x0bc0),
    (0x0bcd, 0x0bcd), (0x0c00, 0x0c00), (0x0c04, 0x0c04), (0x0c3c, 0x0c3c), (0x0c3e, 0x0c40), (0x0c46, 0x0c48), (0x0c4a, 0x0c4d),
    (0x0c55, 0x0c56), (0x0c62, 0x0c63), (0x0c81, 0x0c81), (0x0cbc, 0x0cbc), (0x0cbf, 0x0cbf), (0x0cc6, 0x0cc6), (0x0ccc, 0x0ccd),
    (0x0ce2, 0x0ce3), (0x0d00, 0x0d01), (0x0d3b, 0x0d3c), (0x0d41, 0x0d44), (0x0d4d, 0x0d4d), (0x0d62, 0x0d63), (0x0d81, 0x0d81),
    (0x0dca, 0x0dca), (0x0dd2, 0x0dd4), (0x0dd6, 0x0dd6), (0x0e31, 0x0e31), (0x0e34, 0x0e3a), (0x0e47, 0x0e4e), (0x0eb1, 0x0eb1),
    (0x0eb4, 0x0ebc), (0x0ec8, 0x0ece), (0x0f18, 0x0f19), (0x0f35, 0x0f35), (0x0f37, 0x0f37), (0x0f39, 0x0f39), (0x0f71, 0x0f7e),
    (0x0f80, 0x0f84), (0x0f86, 0x0f87), (0x0f8d, 0x0f97), (0x0f99, 0x0fbc), (0x0fc6, 0x0fc6), (0x102d, 0x1030), (0x1032, 0x1037),
    (0x1039, 0x103a), (0x103d, 0x103e), (0x1058, 0x1059), (0x105e, 0x1060), (0x1071, 0x1074), (0x1082, 0x1082), (0x1085, 0x1086),
    (0x108d, 0x108d), (0x109d, 0x109d), (0x135d, 0x135f), (0x1712, 0x1714), (0x1732, 0x1733), (0x1752, 0x1753), (0x1772, 0x1773),
    (0x17b4, 0x17b5), (0x17b7, 0x17bd), (0x17c6, 0x17c6), (0x17c9, 0x17d3), (0x17dd, 0x17dd), (0x180b, 0x180d), (0x180f, 0x180f),
    (0x1885, 0x1886), (0x18a9, 0x18a9), (0x1920, 0x1922), (0x1927, 0x1928), (0x1932, 0x1932), (0x1939, 0x193b), (0x1a17, 0x1a18),
    (0x1a1b, 0x1a1b), (0x1a56, 0x1a56), (0x1a58, 0x1a5e), (0x1a60, 0x1a60), (0x1a62, 0x1a62), (0x1a65, 0x1a6c), (0x1a73, 0x1a7c),
    (0x1a7f, 0x1a7f), (0x1ab0, 0x1ace), (0x1b00, 0x1b03), (0x1b34, 0x1b34), (0x1b36, 0x1b3a), (0x1b3c, 0x1b3c), (0x1b42, 0x1b42),
    (0x1b6b, 0x1b73), (0x1b80, 0x1b81), (0x1ba2, 0x1ba5), (0x1ba8, 0x1ba9), (0x1bab, 0x1bad), (0x1be6, 0x1be6), (0x1be8, 0x1be9),
    (0x1bed, 0x1bed), (0x1bef, 0x1bf1), (0x1c2c, 0x1c33), (0x1c36, 0x1c37), (0x1cd0, 0x1cd2), (0x1cd4, 0x1ce0), (0x1ce2, 0x1ce8),
    (0x1ced, 0x1ced), (0x1cf4, 0x1cf4), (0x1cf8, 0x1cf9), (0x1dc0, 0x1dff), (0x20d0, 0x20f0), (0x2cef, 0x2cf1), (0x2d7f, 0x2d7f),
    (0x2de0, 0x2dff), (0x302a, 0x302d), (0x3099, 0x309a), (0xa66f, 0xa672), (0xa674, 0xa67d), (0xa69e, 0xa69f), (0xa6f0, 0xa6f1),
    (0xa802, 0xa802), (0xa806, 0xa806), (0xa80b, 0xa80b), (0xa825, 0xa826), (0xa82c, 0xa82c), (0xa8c4, 0xa8c5), (0xa8e0, 0xa8f1),
    (0xa8ff, 0xa8ff), (0xa926, 0xa92d), (0xa947, 0xa951), (0xa980, 0xa982), (0xa9b3, 0xa9b3), (0xa9b6, 0xa9b9), (0xa9bc, 0xa9bd),
    (0xa9e5, 0xa9e5), (0xaa29, 0xaa2e), (0xaa31, 0xaa32), (0xaa35, 0xaa36), (0xaa43, 0xaa43), (0xaa4c, 0xaa4c), (0xaa7c, 0xaa7c),
    (0xaab0, 0xaab0), (0xaab2, 0xaab4), (0xaab7, 0xaab8), (0xaabe, 0xaabf), (0xaac1, 0xaac1), (0xaaec, 0xaaed), (0xaaf6, 0xaaf6),
    (0xabe5, 0xabe5), (0xabe8, 0xabe8), (0xabed, 0xabed), (0xfb1e, 0xfb1e), (0xfe00, 0xfe0f), (0xfe20, 0xfe2f), (0x101fd, 0x101fd),
    (0x102e0, 0x102e0), (0x10376, 0x1037a), (0x10a01, 0x10a03), (0x10a05, 0x10a06), (0x10a0c, 0x10a0f), (0x10a38, 0x10a3a),
    (0x10a3f, 0x10a3f), (0x1d167, 0x1d169), (0x1d17b, 0x1d182), (0x1d185, 0x1d18b), (0x1d1aa, 0x1d1ad), (0x1e000, 0x1e02a),
    (0x1e8d0, 0x1e8d6), (0x1e944, 0x1e94a), (0xe0100, 0xe01ef),
];

/// Format characters (`Cf`).
const FORMATS: &[(u32, u32)] = &[
    (0x00ad, 0x00ad), (0x0600, 0x0605), (0x061c, 0x061c), (0x06dd, 0x06dd), (0x070f, 0x070f), (0x0890, 0x0891), (0x08e2, 0x08e2),
    (0x180e, 0x180e), (0x200b, 0x200f), (0x202a, 0x202e), (0x2060, 0x2064), (0x2066, 0x206f), (0xfeff, 0xfeff), (0xfff9, 0xfffb),
    (0x110bd, 0x110bd), (0x110cd, 0x110cd), (0x13430, 0x1343f), (0x1bca0, 0x1bca3), (0x1d173, 0x1d17a), (0xe0001, 0xe0001),
    (0xe0020, 0xe007f),
];

/// Dash punctuation (`Pd`).
const DASHES: &[(u32, u32)] = &[
    (0x002d, 0x002d), (0x058a, 0x058a), (0x05be, 0x05be), (0x1400, 0x1400), (0x1806, 0x1806), (0x2010, 0x2015), (0x2e17, 0x2e17),
    (0x2e1a, 0x2e1a), (0x2e3a, 0x2e3b), (0x2e40, 0x2e40), (0x2e5d, 0x2e5d), (0x301c, 0x301c), (0x3030, 0x3030), (0x30a0, 0x30a0),
    (0xfe31, 0xfe32), (0xfe58, 0xfe58), (0xfe63, 0xfe63), (0xff0d, 0xff0d), (0x10ead, 0x10ead),
];

/// Connector punctuation (`Pc`).
const CONNECTORS: &[(u32, u32)] = &[(0x005f, 0x005f), (0x203f, 0x2040), (0x2054, 0x2054), (0xfe33, 0xfe34), (0xfe4d, 0xfe4f), (0xff3f, 0xff3f)];

/// Currency symbols (`Sc`).
const CURRENCY: &[(u32, u32)] = &[
    (0x0024, 0x0024), (0x00a2, 0x00a5), (0x058f, 0x058f), (0x060b, 0x060b), (0x07fe, 0x07ff), (0x09f2, 0x09f3), (0x09fb, 0x09fb),
    (0x0af1, 0x0af1), (0x0bf9, 0x0bf9), (0x0e3f, 0x0e3f), (0x17db, 0x17db), (0x20a0, 0x20c0), (0xa838, 0xa838), (0xfdfc, 0xfdfc),
    (0xfe69, 0xfe69), (0xff04, 0xff04), (0xffe0, 0xffe1), (0xffe5, 0xffe6), (0x11fdd, 0x11fe0), (0x1e2ff, 0x1e2ff), (0x1ecb0, 0x1ecb0),
];

/// Opening punctuation (`Ps`).
const OPENS: &[u32] = &[
    0x28, 0x5b, 0x7b, 0xf3a, 0xf3c, 0x169b, 0x201a, 0x201e, 0x2045, 0x207d, 0x208d, 0x2308, 0x230a, 0x2329, 0x2768, 0x276a, 0x276c,
    0x276e, 0x2770, 0x2772, 0x2774, 0x27c5, 0x27e6, 0x27e8, 0x27ea, 0x27ec, 0x27ee, 0x2983, 0x2985, 0x2987, 0x2989, 0x298b, 0x298d,
    0x298f, 0x2991, 0x2993, 0x2995, 0x2997, 0x29d8, 0x29da, 0x29fc, 0x2e22, 0x2e24, 0x2e26, 0x2e28, 0x2e42, 0x2e55, 0x2e57, 0x2e59,
    0x2e5b, 0x3008, 0x300a, 0x300c, 0x300e, 0x3010, 0x3014, 0x3016, 0x3018, 0x301a, 0x301d, 0xfd3f, 0xfe17, 0xfe35, 0xfe37, 0xfe39,
    0xfe3b, 0xfe3d, 0xfe3f, 0xfe41, 0xfe43, 0xfe47, 0xfe59, 0xfe5b, 0xfe5d, 0xff08, 0xff3b, 0xff5b, 0xff5f, 0xff62,
];

/// Closing punctuation (`Pe`).
const CLOSES: &[u32] = &[
    0x29, 0x5d, 0x7d, 0xf3b, 0xf3d, 0x169c, 0x2046, 0x207e, 0x208e, 0x2309, 0x230b, 0x232a, 0x2769, 0x276b, 0x276d, 0x276f, 0x2771,
    0x2773, 0x2775, 0x27c6, 0x27e7, 0x27e9, 0x27eb, 0x27ed, 0x27ef, 0x2984, 0x2986, 0x2988, 0x298a, 0x298c, 0x298e, 0x2990, 0x2992,
    0x2994, 0x2996, 0x2998, 0x29d9, 0x29db, 0x29fd, 0x2e23, 0x2e25, 0x2e27, 0x2e29, 0x2e56, 0x2e58, 0x2e5a, 0x2e5c, 0x3009, 0x300b,
    0x300d, 0x300f, 0x3011, 0x3015, 0x3017, 0x3019, 0x301b, 0x301e, 0x301f, 0xfd3e, 0xfe18, 0xfe36, 0xfe38, 0xfe3a, 0xfe3c, 0xfe3e,
    0xfe40, 0xfe42, 0xfe44, 0xfe48, 0xfe5a, 0xfe5c, 0xfe5e, 0xff09, 0xff3d, 0xff5d, 0xff60, 0xff63,
];

/// Initial quotes (`Pi`).
const INITIALS: &[u32] = &[0xab, 0x2018, 0x201b, 0x201c, 0x201f, 0x2039, 0x2e02, 0x2e04, 0x2e09, 0x2e0c, 0x2e1c, 0x2e20];

/// Final quotes (`Pf`).
const FINALS: &[u32] = &[0xbb, 0x2019, 0x201d, 0x203a, 0x2e03, 0x2e05, 0x2e0a, 0x2e0d, 0x2e1d, 0x2e21];

/// Letter numbers (`Nl`): numbers, but not the digits a line's numbers are made of.
const LETTER_NUMBERS: &[(u32, u32)] = &[
    (0x16ee, 0x16f0), (0x2160, 0x2182), (0x2185, 0x2188), (0x3007, 0x3007), (0x3021, 0x3029), (0x3038, 0x303a), (0xa6e6, 0xa6ef),
    (0x10140, 0x10174), (0x10341, 0x10341), (0x1034a, 0x1034a), (0x103d1, 0x103d5), (0x12400, 0x1246e),
];

/// What `<post-word>` names beyond the closing marks: degrees, per-mille, small kana and the like.
const LINE_POST: &[(u32, u32)] = &[
    (0x00a2, 0x00a2), (0x00b0, 0x00b0), (0x066a, 0x066a), (0x2030, 0x2034), (0x2103, 0x2103), (0x2105, 0x2105), (0x2109, 0x2109),
    (0x3001, 0x3002), (0x3005, 0x3005), (0x3041, 0x3041), (0x3043, 0x3043), (0x3045, 0x3045), (0x3047, 0x3047), (0x3049, 0x3049),
    (0x3063, 0x3063), (0x3083, 0x3083), (0x3085, 0x3085), (0x3087, 0x3087), (0x308e, 0x308e), (0x3099, 0x309e), (0x30a1, 0x30a1),
    (0x30a3, 0x30a3), (0x30a5, 0x30a5), (0x30a7, 0x30a7), (0x30a9, 0x30a9), (0x30c3, 0x30c3), (0x30e3, 0x30e3), (0x30e5, 0x30e5),
    (0x30e7, 0x30e7), (0x30ee, 0x30ee), (0x30f5, 0x30f6), (0x30fc, 0x30fe), (0xff01, 0xff01), (0xff05, 0xff05), (0xff0c, 0xff0c),
    (0xff0e, 0xff0e), (0xff1a, 0xff1b), (0xff1f, 0xff1f),
];

/// `<kanji>` in the line rules: ideographs, kana and Hangul syllables, a line may break around each.
const LINE_KANJI: &[(u32, u32)] = &[(0x3041, 0x3094), (0x30a1, 0x30fa), (0x4e00, 0x9fa5), (0xac00, 0xd7a3), (0xf900, 0xfa2d), (0xfa30, 0xfa6a)];

/// Format characters that go before what they mark (`Prepend`).
const PREPENDED: &[(u32, u32)] = &[
    (0x0600, 0x0605), (0x06dd, 0x06dd), (0x070f, 0x070f), (0x0890, 0x0891), (0x08e2, 0x08e2), (0x0d4e, 0x0d4e), (0x110bd, 0x110bd),
    (0x110cd, 0x110cd), (0x111c2, 0x111c3), (0x1193f, 0x1193f), (0x11941, 0x11941), (0x11a3a, 0x11a3a), (0x11a84, 0x11a89),
    (0x11d46, 0x11d46),
];

/// Spacing marks that extend a cluster as nonspacing ones do (`Grapheme_Extend`).
const EXTENDING_SPACING_MARKS: &[(u32, u32)] = &[
    (0x09be, 0x09be), (0x09d7, 0x09d7), (0x0b3e, 0x0b3e), (0x0b57, 0x0b57), (0x0bbe, 0x0bbe), (0x0bd7, 0x0bd7), (0x0cc2, 0x0cc2),
    (0x0cd5, 0x0cd6), (0x0d3e, 0x0d3e), (0x0d57, 0x0d57), (0x0dcf, 0x0dcf), (0x0ddf, 0x0ddf), (0x1b35, 0x1b35), (0x302e, 0x302f),
    (0x1d165, 0x1d165), (0x1d16e, 0x1d172),
];

/// Spacing marks (`Mc`).
const SPACING_MARKS: &[(u32, u32)] = &[
    (0x0903, 0x0903), (0x093b, 0x093b), (0x093e, 0x0940), (0x0949, 0x094c), (0x094e, 0x094f), (0x0982, 0x0983), (0x09bf, 0x09c0),
    (0x09c7, 0x09c8), (0x09cb, 0x09cc), (0x0a03, 0x0a03), (0x0a3e, 0x0a40), (0x0a83, 0x0a83), (0x0abe, 0x0ac0), (0x0ac9, 0x0ac9),
    (0x0acb, 0x0acc), (0x0b02, 0x0b03), (0x0b40, 0x0b40), (0x0b47, 0x0b48), (0x0b4b, 0x0b4c), (0x0bbf, 0x0bbf), (0x0bc1, 0x0bc2),
    (0x0bc6, 0x0bc8), (0x0bca, 0x0bcc), (0x0c01, 0x0c03), (0x0c41, 0x0c44), (0x0c82, 0x0c83), (0x0cbe, 0x0cbe), (0x0cc0, 0x0cc1),
    (0x0cc3, 0x0cc4), (0x0cc7, 0x0cc8), (0x0cca, 0x0ccb), (0x0d02, 0x0d03), (0x0d3f, 0x0d40), (0x0d46, 0x0d48), (0x0d4a, 0x0d4c),
    (0x0d82, 0x0d83), (0x0dd0, 0x0dd1), (0x0dd8, 0x0dde), (0x0df2, 0x0df3), (0x0f3e, 0x0f3f), (0x0f7f, 0x0f7f), (0x102b, 0x102c),
    (0x1031, 0x1031), (0x1038, 0x1038), (0x103b, 0x103c), (0x1056, 0x1057), (0x1062, 0x1064), (0x1067, 0x106d), (0x1083, 0x1084),
    (0x1087, 0x108c), (0x108f, 0x108f), (0x109a, 0x109c), (0x17b6, 0x17b6), (0x17be, 0x17c5), (0x17c7, 0x17c8), (0x1923, 0x1926),
    (0x1929, 0x192b), (0x1930, 0x1931), (0x1933, 0x1938), (0x1a19, 0x1a1a), (0x1a55, 0x1a55), (0x1a57, 0x1a57), (0x1a6d, 0x1a72),
    (0x1b04, 0x1b04), (0x1b3b, 0x1b3b), (0x1b3d, 0x1b41), (0x1b43, 0x1b44), (0x1b82, 0x1b82), (0x1ba1, 0x1ba1), (0x1ba6, 0x1ba7),
    (0x1baa, 0x1baa), (0x1be7, 0x1be7), (0x1bea, 0x1bec), (0x1bee, 0x1bee), (0x1bf2, 0x1bf3), (0x1c24, 0x1c2b), (0x1c34, 0x1c35),
    (0x1ce1, 0x1ce1), (0x1cf7, 0x1cf7), (0xa823, 0xa824), (0xa827, 0xa827), (0xa880, 0xa881), (0xa8b4, 0xa8c3), (0xa952, 0xa953),
    (0xa983, 0xa983), (0xa9b4, 0xa9b5), (0xa9ba, 0xa9bb), (0xa9be, 0xa9c0), (0xaa2f, 0xaa30), (0xaa33, 0xaa34), (0xaa4d, 0xaa4d),
    (0xaaeb, 0xaaeb), (0xaaee, 0xaaef), (0xaaf5, 0xaaf5), (0xabe3, 0xabe4), (0xabe6, 0xabe7), (0xabe9, 0xabea), (0xabec, 0xabec),
];

/// Pictographs (`Extended_Pictographic`): emoji and the symbols they grew from.
const PICTOGRAPHS: &[(u32, u32)] = &[
    (0x00a9, 0x00a9), (0x00ae, 0x00ae), (0x203c, 0x203c), (0x2049, 0x2049), (0x2122, 0x2122), (0x2139, 0x2139), (0x2194, 0x2199),
    (0x21a9, 0x21aa), (0x231a, 0x231b), (0x2328, 0x2328), (0x2388, 0x2388), (0x23cf, 0x23cf), (0x23e9, 0x23f3), (0x23f8, 0x23fa),
    (0x24c2, 0x24c2), (0x25aa, 0x25ab), (0x25b6, 0x25b6), (0x25c0, 0x25c0), (0x25fb, 0x25fe), (0x2600, 0x2605), (0x2607, 0x2612),
    (0x2614, 0x2685), (0x2690, 0x2705), (0x2708, 0x2712), (0x2714, 0x2714), (0x2716, 0x2716), (0x271d, 0x271d), (0x2721, 0x2721),
    (0x2728, 0x2728), (0x2733, 0x2734), (0x2744, 0x2744), (0x2747, 0x2747), (0x274c, 0x274c), (0x274e, 0x274e), (0x2753, 0x2755),
    (0x2757, 0x2757), (0x2763, 0x2767), (0x2795, 0x2797), (0x27a1, 0x27a1), (0x27b0, 0x27b0), (0x27bf, 0x27bf), (0x2934, 0x2935),
    (0x2b05, 0x2b07), (0x2b1b, 0x2b1c), (0x2b50, 0x2b50), (0x2b55, 0x2b55), (0x3030, 0x3030), (0x303d, 0x303d), (0x3297, 0x3297),
    (0x3299, 0x3299), (0x1f000, 0x1f0ff), (0x1f10d, 0x1f10f), (0x1f12f, 0x1f12f), (0x1f16c, 0x1f171), (0x1f17e, 0x1f17f),
    (0x1f18e, 0x1f18e), (0x1f191, 0x1f19a), (0x1f1ad, 0x1f1e5), (0x1f201, 0x1f20f), (0x1f21a, 0x1f21a), (0x1f22f, 0x1f22f),
    (0x1f232, 0x1f23a), (0x1f23c, 0x1f23f), (0x1f249, 0x1f3fa), (0x1f400, 0x1f53d), (0x1f546, 0x1f64f), (0x1f680, 0x1f6ff),
    (0x1f774, 0x1f77f), (0x1f7d5, 0x1f7ff), (0x1f80c, 0x1f80f), (0x1f848, 0x1f84f), (0x1f85a, 0x1f85f), (0x1f888, 0x1f88f),
    (0x1f8ae, 0x1f8ff), (0x1f90c, 0x1f93a), (0x1f93c, 0x1f945), (0x1f947, 0x1faff), (0x1fc00, 0x1fffd),
];


#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str) -> Kind {
        match name {
            "line" => Kind::Line,
            "word" => Kind::Word,
            "sentence" => Kind::Sentence,
            _ => Kind::Character,
        }
    }

    /// Lines, words, sentences and characters break where JDK 21's iterators break them (the
    /// expected boundaries are its output: `tools/jdk_break_iterator_oracle.py --diff`): a line
    /// after its spaces, after a dash or a closing mark that letters follow, around each ideograph,
    /// never inside a number or before a closing mark; a word at each change between letters,
    /// numbers, spaces and marks; a sentence after its terminator when a sentence plainly starts
    /// next; a character after its marks, joiners and flag pairs.
    #[test]
    fn text_breaks_where_the_jdks_iterators_break_it() {
        let cases: &[(&str, &str, &[i32])] = &[
            ("line", "Hello, world! foo-bar baz", &[0, 7, 14, 18, 22, 25]),
            ("line", "Hold [Shift] for Summary", &[0, 5, 13, 17, 24]),
            ("line", "A  double  spaced   line.", &[0, 3, 11, 20, 25]),
            ("line", "Line one\u{a}Line two\u{d}\u{a}three", &[0, 5, 9, 14, 19, 24]),
            ("line", "Numbers 1,000.50 and 3.14, -5 or 10%.", &[0, 8, 17, 21, 27, 30, 33, 37]),
            ("line", "(parenthesised) \"quoted\" it's e.g. well-known", &[0, 16, 25, 30, 32, 35, 40, 45]),
            ("line", "Mechanical Crafters arrange items\u{2014}like this\u{2014}into recipes.", &[0, 11, 20, 28, 34, 39, 44, 49, 57]),
            ("line", "Ends with spaces   ", &[0, 5, 10, 19]),
            ("line", "tab\u{9}separated\u{9}text", &[0, 4, 14, 18]),
            ("line", "\u{4f60}\u{597d}\u{4e16}\u{754c}, hello \u{3053}\u{3093}\u{306b}\u{3061}\u{306f}", &[0, 1, 2, 3, 6, 12, 13, 14, 15, 16, 17]),
            ("line", "e\u{301}te\u{301} cafe\u{301}", &[0, 6, 11]),
            ("line", "Use /create config to open... Really? Yes! Done.", &[0, 4, 12, 19, 22, 30, 38, 43, 48]),
            ("line", "a", &[0, 1]),
            ("line", "", &[0]),
            ("line", "Rotational force: 256su @ 16rpm (x4)", &[0, 11, 18, 24, 26, 32, 36]),
            ("line", "URL https://example.com/path?q=1 here", &[0, 4, 10, 20, 29, 33, 37]),
            ("line", "Mr. Smith went. Then he left.", &[0, 4, 10, 16, 21, 24, 29]),
            ("line", "end.Next one", &[0, 4, 9, 12]),
            ("line", "Hi there.\u{a}New line", &[0, 3, 10, 14, 18]),
            ("line", "\"Quoted.\" Next", &[0, 10, 14]),
            ("line", "Wait... what? OK", &[0, 8, 14, 16]),
            ("line", "A.B.C. Done", &[0, 2, 4, 7, 11]),
            ("line", "x.  (Paren) y", &[0, 4, 12, 13]),
            ("line", "Is it 3.5? Yes.", &[0, 3, 6, 11, 15]),
            ("line", "Para one\u{2029}Para two", &[0, 5, 9, 14, 17]),
            ("line", "nbsp\u{a0}joined words", &[0, 12, 17]),
            ("line", "a--b x -y $100 off 50% done", &[0, 3, 5, 7, 8, 10, 15, 19, 23, 27]),
            ("line", "word\u{ad}break end) \u{ab}quote\u{bb} text", &[0, 5, 11, 16, 24, 28]),
            ("line", "a/b/c foo_bar \u{9}\u{9}indent", &[0, 6, 15, 16, 22]),
            ("line", "  leading spaces", &[0, 2, 10, 16]),
            ("line", "don't U.S.A. $3.50 #1 item x--y caf\u{e9}", &[0, 6, 8, 10, 13, 19, 22, 27, 30, 32, 36]),
            ("line", "\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30ad}\u{30b9}\u{30c8} \u{30ab}\u{30bf}\u{30ab}\u{30ca}", &[0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12]),
            ("line", "\u{1f44d}\u{1f3fd} ok \u{1f1fa}\u{1f1e6} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}", &[0, 5, 8, 13, 21]),
            ("line", "\u{d55c}\u{ad6d}\u{c5b4} \u{1100}\u{1161}\u{11a8} a\u{200d}b", &[0, 1, 2, 4, 8, 11]),
            ("line", "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}", &[0, 8, 14]),
            ("line", "Shift+Click: 3x faster; (see \u{a7}e/ponder\u{a7}r).", &[0, 13, 16, 24, 29, 42]),
            ("line", "1st 2nd: 10:30 a.m. v1.2.3-beta", &[0, 4, 9, 12, 15, 17, 20, 23, 27, 31]),
            ("line", "...leading dots and trailing!!!", &[0, 3, 11, 16, 20, 31]),
            ("line", "end.(Next one", &[0, 4, 10, 13]),
            ("line", "go./run now", &[0, 3, 8, 11]),
            ("line", "x. 5 apples", &[0, 3, 5, 11]),
            ("line", "a.\u{a0}B c", &[0, 5, 6]),
            ("line", "Hi!Bye", &[0, 3, 6]),
            ("line", "Ok.\u{2029}next", &[0, 4, 8]),
            ("line", "ex.\"Quote\" more", &[0, 4, 11, 15]),
            ("line", "a\u{b}b c", &[0, 2, 4, 5]),
            ("line", "a\u{200b}b c", &[0, 4, 5]),
            ("line", "a\u{feff}b c", &[0, 4, 5]),
            ("line", "ab\u{ad} cd", &[0, 4, 6]),
            ("line", "\u{1f1fa}\u{1f1e6}\u{1f1fa} x", &[0, 7, 8]),
            ("line", "\u{1100}\u{1100}\u{1161}\u{11a8}\u{11a8} \u{ac00}\u{11a8} \u{ac01}\u{11a8}", &[0, 6, 7, 9, 10, 11]),
            ("line", "a\u{d}\u{a}\u{d}\u{a}b\u{85}c", &[0, 3, 5, 7, 8]),
            ("line", "\u{a9}\u{fe0f}\u{200d}\u{1f525} \u{2764}\u{fe0f}", &[0, 6, 8]),
            ("line", "price: $5 (approx.) \u{20ac}10 or 5\u{20ac}", &[0, 7, 10, 20, 24, 27, 28, 29]),
            ("line", "it\u{2019}s \u{201c}fancy\u{201d} \u{2014} isn\u{2019}t it?", &[0, 3, 5, 13, 15, 19, 21, 24]),
            ("word", "Hello, world! foo-bar baz", &[0, 5, 6, 7, 12, 13, 14, 21, 22, 25]),
            ("word", "Hold [Shift] for Summary", &[0, 4, 5, 6, 11, 12, 13, 16, 17, 24]),
            ("word", "A  double  spaced   line.", &[0, 1, 3, 9, 11, 17, 20, 24, 25]),
            ("word", "Line one\u{a}Line two\u{d}\u{a}three", &[0, 4, 5, 8, 9, 13, 14, 17, 19, 24]),
            ("word", "Numbers 1,000.50 and 3.14, -5 or 10%.", &[0, 7, 8, 16, 17, 20, 21, 25, 26, 27, 28, 29, 30, 32, 33, 36, 37]),
            ("word", "(parenthesised) \"quoted\" it's e.g. well-known", &[0, 1, 14, 15, 16, 17, 23, 24, 25, 29, 30, 33, 34, 35, 45]),
            ("word", "Mechanical Crafters arrange items\u{2014}like this\u{2014}into recipes.", &[0, 10, 11, 19, 20, 27, 28, 38, 39, 48, 49, 56, 57]),
            ("word", "Ends with spaces   ", &[0, 4, 5, 9, 10, 16, 19]),
            ("word", "tab\u{9}separated\u{9}text", &[0, 3, 4, 13, 14, 18]),
            ("word", "\u{4f60}\u{597d}\u{4e16}\u{754c}, hello \u{3053}\u{3093}\u{306b}\u{3061}\u{306f}", &[0, 4, 5, 6, 11, 12, 17]),
            ("word", "e\u{301}te\u{301} cafe\u{301}", &[0, 5, 6, 11]),
            ("word", "Use /create config to open... Really? Yes! Done.", &[0, 3, 4, 5, 11, 12, 18, 19, 21, 22, 26, 27, 28, 29, 30, 36, 37, 38, 41, 42, 43, 47, 48]),
            ("word", "a", &[0, 1]),
            ("word", "", &[0]),
            ("word", "Rotational force: 256su @ 16rpm (x4)", &[0, 10, 11, 16, 17, 18, 23, 24, 25, 26, 31, 32, 33, 35, 36]),
            ("word", "URL https://example.com/path?q=1 here", &[0, 3, 4, 9, 10, 11, 12, 23, 24, 28, 29, 30, 31, 32, 33, 37]),
            ("word", "Mr. Smith went. Then he left.", &[0, 2, 3, 4, 9, 10, 14, 15, 16, 20, 21, 23, 24, 28, 29]),
            ("word", "end.Next one", &[0, 8, 9, 12]),
            ("word", "Hi there.\u{a}New line", &[0, 2, 3, 8, 9, 10, 13, 14, 18]),
            ("word", "\"Quoted.\" Next", &[0, 1, 7, 8, 9, 10, 14]),
            ("word", "Wait... what? OK", &[0, 4, 5, 6, 7, 8, 12, 13, 14, 16]),
            ("word", "A.B.C. Done", &[0, 5, 6, 7, 11]),
            ("word", "x.  (Paren) y", &[0, 1, 2, 4, 5, 10, 11, 12, 13]),
            ("word", "Is it 3.5? Yes.", &[0, 2, 3, 5, 6, 9, 10, 11, 14, 15]),
            ("word", "Para one\u{2029}Para two", &[0, 4, 5, 8, 9, 13, 14, 17]),
            ("word", "nbsp\u{a0}joined words", &[0, 4, 5, 11, 12, 17]),
            ("word", "a--b x -y $100 off 50% done", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 14, 15, 18, 19, 22, 23, 27]),
            ("word", "word\u{ad}break end) \u{ab}quote\u{bb} text", &[0, 10, 11, 14, 15, 16, 17, 22, 23, 24, 28]),
            ("word", "a/b/c foo_bar \u{9}\u{9}indent", &[0, 1, 2, 3, 4, 5, 6, 13, 16, 22]),
            ("word", "  leading spaces", &[0, 2, 9, 10, 16]),
            ("word", "don't U.S.A. $3.50 #1 item x--y caf\u{e9}", &[0, 5, 6, 11, 12, 13, 18, 19, 21, 22, 26, 27, 28, 29, 30, 31, 32, 36]),
            ("word", "\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30ad}\u{30b9}\u{30c8} \u{30ab}\u{30bf}\u{30ab}\u{30ca}", &[0, 3, 7, 8, 12]),
            ("word", "\u{1f44d}\u{1f3fd} ok \u{1f1fa}\u{1f1e6} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}", &[0, 2, 4, 5, 7, 8, 10, 12, 13, 16, 19, 21]),
            ("word", "\u{d55c}\u{ad6d}\u{c5b4} \u{1100}\u{1161}\u{11a8} a\u{200d}b", &[0, 3, 4, 7, 8, 11]),
            ("word", "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}", &[0, 7, 8, 14]),
            ("word", "Shift+Click: 3x faster; (see \u{a7}e/ponder\u{a7}r).", &[0, 5, 6, 11, 12, 13, 15, 16, 22, 23, 24, 25, 28, 29, 30, 31, 32, 38, 39, 40, 41, 42]),
            ("word", "1st 2nd: 10:30 a.m. v1.2.3-beta", &[0, 3, 4, 7, 8, 9, 11, 12, 14, 15, 18, 19, 20, 26, 27, 31]),
            ("word", "...leading dots and trailing!!!", &[0, 1, 2, 3, 10, 11, 15, 16, 19, 20, 28, 29, 30, 31]),
            ("word", "end.(Next one", &[0, 3, 4, 5, 9, 10, 13]),
            ("word", "go./run now", &[0, 2, 3, 4, 7, 8, 11]),
            ("word", "x. 5 apples", &[0, 1, 2, 3, 4, 5, 11]),
            ("word", "a.\u{a0}B c", &[0, 1, 2, 3, 4, 5, 6]),
            ("word", "Hi!Bye", &[0, 2, 3, 6]),
            ("word", "Ok.\u{2029}next", &[0, 2, 3, 4, 8]),
            ("word", "ex.\"Quote\" more", &[0, 2, 3, 4, 9, 10, 11, 15]),
            ("word", "a\u{b}b c", &[0, 1, 2, 3, 4, 5]),
            ("word", "a\u{200b}b c", &[0, 3, 4, 5]),
            ("word", "a\u{feff}b c", &[0, 3, 4, 5]),
            ("word", "ab\u{ad} cd", &[0, 2, 3, 4, 6]),
            ("word", "\u{1f1fa}\u{1f1e6}\u{1f1fa} x", &[0, 2, 4, 6, 7, 8]),
            ("word", "\u{1100}\u{1100}\u{1161}\u{11a8}\u{11a8} \u{ac00}\u{11a8} \u{ac01}\u{11a8}", &[0, 5, 6, 8, 9, 11]),
            ("word", "a\u{d}\u{a}\u{d}\u{a}b\u{85}c", &[0, 1, 3, 5, 6, 7, 8]),
            ("word", "\u{a9}\u{fe0f}\u{200d}\u{1f525} \u{2764}\u{fe0f}", &[0, 3, 5, 6, 8]),
            ("word", "price: $5 (approx.) \u{20ac}10 or 5\u{20ac}", &[0, 5, 6, 7, 9, 10, 11, 17, 18, 19, 20, 23, 24, 26, 27, 28, 29]),
            ("word", "it\u{2019}s \u{201c}fancy\u{201d} \u{2014} isn\u{2019}t it?", &[0, 2, 3, 4, 5, 6, 11, 12, 13, 14, 15, 18, 19, 20, 21, 23, 24]),
            ("character", "Hello, world! foo-bar baz", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]),
            ("character", "Line one\u{a}Line two\u{d}\u{a}three", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24]),
            ("character", "Mechanical Crafters arrange items\u{2014}like this\u{2014}into recipes.", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57]),
            ("character", "tab\u{9}separated\u{9}text", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]),
            ("character", "\u{4f60}\u{597d}\u{4e16}\u{754c}, hello \u{3053}\u{3093}\u{306b}\u{3061}\u{306f}", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17]),
            ("character", "e\u{301}te\u{301} cafe\u{301}", &[0, 2, 3, 5, 6, 7, 8, 9, 11]),
            ("character", "Hi there.\u{a}New line", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]),
            ("character", "Para one\u{2029}Para two", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17]),
            ("character", "nbsp\u{a0}joined words", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17]),
            ("character", "word\u{ad}break end) \u{ab}quote\u{bb} text", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28]),
            ("character", "a/b/c foo_bar \u{9}\u{9}indent", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22]),
            ("character", "don't U.S.A. $3.50 #1 item x--y caf\u{e9}", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36]),
            ("character", "\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30ad}\u{30b9}\u{30c8} \u{30ab}\u{30bf}\u{30ab}\u{30ca}", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
            ("character", "\u{1f44d}\u{1f3fd} ok \u{1f1fa}\u{1f1e6} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}", &[0, 4, 5, 6, 7, 8, 12, 13, 21]),
            ("character", "\u{d55c}\u{ad6d}\u{c5b4} \u{1100}\u{1161}\u{11a8} a\u{200d}b", &[0, 1, 2, 3, 4, 7, 8, 10, 11]),
            ("character", "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14]),
            ("character", "Shift+Click: 3x faster; (see \u{a7}e/ponder\u{a7}r).", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42]),
            ("character", "a.\u{a0}B c", &[0, 1, 2, 3, 4, 5, 6]),
            ("character", "Ok.\u{2029}next", &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
            ("character", "a\u{b}b c", &[0, 1, 2, 3, 4, 5]),
            ("character", "a\u{200b}b c", &[0, 1, 2, 3, 4, 5]),
            ("character", "a\u{feff}b c", &[0, 1, 2, 3, 4, 5]),
            ("character", "ab\u{ad} cd", &[0, 1, 2, 3, 4, 5, 6]),
            ("character", "\u{1f1fa}\u{1f1e6}\u{1f1fa} x", &[0, 4, 6, 7, 8]),
            ("character", "\u{1100}\u{1100}\u{1161}\u{11a8}\u{11a8} \u{ac00}\u{11a8} \u{ac01}\u{11a8}", &[0, 5, 6, 8, 9, 11]),
            ("character", "a\u{d}\u{a}\u{d}\u{a}b\u{85}c", &[0, 1, 3, 5, 6, 7, 8]),
            ("character", "\u{a9}\u{fe0f}\u{200d}\u{1f525} \u{2764}\u{fe0f}", &[0, 5, 6, 8]),
            ("character", "price: $5 (approx.) \u{20ac}10 or 5\u{20ac}", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29]),
            ("character", "it\u{2019}s \u{201c}fancy\u{201d} \u{2014} isn\u{2019}t it?", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24]),
            ("sentence", "Hello, world! foo-bar baz", &[0, 14, 25]),
            ("sentence", "Hold [Shift] for Summary", &[0, 24]),
            ("sentence", "A  double  spaced   line.", &[0, 25]),
            ("sentence", "Line one\u{a}Line two\u{d}\u{a}three", &[0, 24]),
            ("sentence", "Numbers 1,000.50 and 3.14, -5 or 10%.", &[0, 37]),
            ("sentence", "(parenthesised) \"quoted\" it's e.g. well-known", &[0, 45]),
            ("sentence", "Mechanical Crafters arrange items\u{2014}like this\u{2014}into recipes.", &[0, 57]),
            ("sentence", "Ends with spaces   ", &[0, 19]),
            ("sentence", "tab\u{9}separated\u{9}text", &[0, 18]),
            ("sentence", "\u{4f60}\u{597d}\u{4e16}\u{754c}, hello \u{3053}\u{3093}\u{306b}\u{3061}\u{306f}", &[0, 17]),
            ("sentence", "e\u{301}te\u{301} cafe\u{301}", &[0, 11]),
            ("sentence", "Use /create config to open... Really? Yes! Done.", &[0, 30, 38, 43, 48]),
            ("sentence", "a", &[0, 1]),
            ("sentence", "", &[0]),
            ("sentence", "Rotational force: 256su @ 16rpm (x4)", &[0, 36]),
            ("sentence", "URL https://example.com/path?q=1 here", &[0, 29, 37]),
            ("sentence", "Mr. Smith went. Then he left.", &[0, 4, 16, 29]),
            ("sentence", "end.Next one", &[0, 12]),
            ("sentence", "Hi there.\u{a}New line", &[0, 10, 18]),
            ("sentence", "\"Quoted.\" Next", &[0, 10, 14]),
            ("sentence", "Wait... what? OK", &[0, 14, 16]),
            ("sentence", "A.B.C. Done", &[0, 7, 11]),
            ("sentence", "x.  (Paren) y", &[0, 4, 13]),
            ("sentence", "Is it 3.5? Yes.", &[0, 11, 15]),
            ("sentence", "Para one\u{2029}Para two", &[0, 9, 17]),
            ("sentence", "nbsp\u{a0}joined words", &[0, 17]),
            ("sentence", "a--b x -y $100 off 50% done", &[0, 27]),
            ("sentence", "word\u{ad}break end) \u{ab}quote\u{bb} text", &[0, 28]),
            ("sentence", "a/b/c foo_bar \u{9}\u{9}indent", &[0, 22]),
            ("sentence", "  leading spaces", &[0, 16]),
            ("sentence", "don't U.S.A. $3.50 #1 item x--y caf\u{e9}", &[0, 36]),
            ("sentence", "\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30ad}\u{30b9}\u{30c8} \u{30ab}\u{30bf}\u{30ab}\u{30ca}", &[0, 12]),
            ("sentence", "\u{1f44d}\u{1f3fd} ok \u{1f1fa}\u{1f1e6} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}", &[0, 21]),
            ("sentence", "\u{d55c}\u{ad6d}\u{c5b4} \u{1100}\u{1161}\u{11a8} a\u{200d}b", &[0, 11]),
            ("sentence", "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}", &[0, 14]),
            ("sentence", "Shift+Click: 3x faster; (see \u{a7}e/ponder\u{a7}r).", &[0, 42]),
            ("sentence", "1st 2nd: 10:30 a.m. v1.2.3-beta", &[0, 31]),
            ("sentence", "...leading dots and trailing!!!", &[0, 31]),
            ("sentence", "end.(Next one", &[0, 4, 13]),
            ("sentence", "go./run now", &[0, 3, 11]),
            ("sentence", "x. 5 apples", &[0, 11]),
            ("sentence", "a.\u{a0}B c", &[0, 3, 6]),
            ("sentence", "Hi!Bye", &[0, 3, 6]),
            ("sentence", "Ok.\u{2029}next", &[0, 4, 8]),
            ("sentence", "ex.\"Quote\" more", &[0, 4, 15]),
            ("sentence", "a\u{b}b c", &[0, 5]),
            ("sentence", "a\u{200b}b c", &[0, 5]),
            ("sentence", "a\u{feff}b c", &[0, 5]),
            ("sentence", "ab\u{ad} cd", &[0, 6]),
            ("sentence", "\u{1f1fa}\u{1f1e6}\u{1f1fa} x", &[0, 8]),
            ("sentence", "\u{1100}\u{1100}\u{1161}\u{11a8}\u{11a8} \u{ac00}\u{11a8} \u{ac01}\u{11a8}", &[0, 11]),
            ("sentence", "a\u{d}\u{a}\u{d}\u{a}b\u{85}c", &[0, 8]),
            ("sentence", "\u{a9}\u{fe0f}\u{200d}\u{1f525} \u{2764}\u{fe0f}", &[0, 8]),
            ("sentence", "price: $5 (approx.) \u{20ac}10 or 5\u{20ac}", &[0, 29]),
            ("sentence", "it\u{2019}s \u{201c}fancy\u{201d} \u{2014} isn\u{2019}t it?", &[0, 24]),
        ];
        let mut wrong = Vec::new();
        for &(k, text, expected) in cases {
            let units: Vec<u16> = text.encode_utf16().collect();
            let found = boundaries(kind(k), &units);
            if found != expected {
                wrong.push(format!("{k} {text:?}: {found:?}, the JDK's {expected:?}"));
            }
        }
        assert!(wrong.is_empty(), "{} of {} differ:\n{}", wrong.len(), cases.len(), wrong.join("\n"));
    }

    fn bi(rt: &mut Rt, name: &'static str, desc: &'static str, args: &[V]) -> V {
        let key = rt.static_key(&format!("java/text/BreakIterator.{name}.{desc}"));
        crate::api::call(rt, key, args)
    }

    /// An iterator walks its boundaries as the JDK's does: `next` and `previous` stop at the ends
    /// with DONE (the loop that wraps a tooltip ends), `next(n)` takes n steps, `following` and
    /// `preceding` find the boundary after or before any offset, `isBoundary` moves to the one at or
    /// after it, an offset past the text throws; a clone walks on its own, and an iterator with no
    /// text has the empty text's one boundary.
    #[test]
    fn an_iterator_walks_its_boundaries_as_the_jdks_does() {
        let mut rt = Rt::new();
        crate::install();
        let it = bi(&mut rt, "getLineInstance", "()Ljava/text/BreakIterator;", &[]);
        let fresh = bi(&mut rt, "getWordInstance", "()Ljava/text/BreakIterator;", &[]);
        let text = rt.string("Hello, world! foo-bar baz");
        bi(&mut rt, "setText", "(Ljava/lang/String;)V", &[it, text]);
        let i = |v: V| v.i();
        let mut walk = Vec::new();
        let mut b = i(bi(&mut rt, "first", "()I", &[it]));
        while b != DONE {
            walk.push(b);
            b = i(bi(&mut rt, "next", "()I", &[it]));
        }
        assert_eq!(walk, [0, 7, 14, 18, 22, 25]);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 25);
        assert_eq!(i(bi(&mut rt, "previous", "()I", &[it])), 22);
        assert_eq!(i(bi(&mut rt, "previous", "()I", &[it])), 18);
        assert_eq!(i(bi(&mut rt, "first", "()I", &[it])), 0);
        assert_eq!(i(bi(&mut rt, "previous", "()I", &[it])), DONE);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 0);
        let steps: Vec<i32> = [2, -1, 0, 9].iter().map(|&n| i(bi(&mut rt, "next", "(I)I", &[it, V::I(n)]))).collect();
        assert_eq!(steps, [14, 7, 7, DONE]);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 25);
        let following: Vec<i32> = [0, 7, 24, 25].iter().map(|&o| i(bi(&mut rt, "following", "(I)I", &[it, V::I(o)]))).collect();
        assert_eq!(following, [7, 14, 25, DONE]);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 25);
        let preceding: Vec<i32> = [25, 7, 1, 0].iter().map(|&o| i(bi(&mut rt, "preceding", "(I)I", &[it, V::I(o)]))).collect();
        assert_eq!(preceding, [22, 0, 0, DONE]);
        assert_eq!(i(bi(&mut rt, "isBoundary", "(I)Z", &[it, V::I(7)])), 1);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 7);
        assert_eq!(i(bi(&mut rt, "isBoundary", "(I)Z", &[it, V::I(9)])), 0);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 14);
        assert_eq!(i(bi(&mut rt, "isBoundary", "(I)Z", &[it, V::I(25)])), 1);
        for (name, desc, offset) in [("following", "(I)I", 26), ("following", "(I)I", -1), ("preceding", "(I)I", 26), ("isBoundary", "(I)Z", 26)] {
            bi(&mut rt, name, desc, &[it, V::I(offset)]);
            let e = rt.thrown.take().unwrap_or_else(|| panic!("{name}({offset}) threw nothing"));
            assert_eq!(rt.class_of(e), "java/lang/IllegalArgumentException");
            assert_eq!(rt.str_of(rt.get_field(e, "message")), Some("offset out of bounds"));
        }
        let copy = bi(&mut rt, "clone", "()Ljava/lang/Object;", &[it]);
        bi(&mut rt, "first", "()I", &[it]);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[copy])), 25);
        assert_eq!(i(bi(&mut rt, "next", "()I", &[copy])), DONE);
        assert_eq!(i(bi(&mut rt, "current", "()I", &[it])), 0);
        let empty: Vec<i32> = ["first", "next", "last", "current"].iter().map(|&n| i(bi(&mut rt, n, "()I", &[fresh]))).collect();
        assert_eq!(empty, [0, DONE, 0, 0]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Text given as a `CharacterIterator` breaks in the iterator's own indexes (from its begin
    /// index), and `getText` hands one back at the iterator's boundary; a `StringCharacterIterator`
    /// walks its range as the JDK's does, DONE (`￿`) past its end.
    #[test]
    fn text_given_as_a_character_iterator_breaks_in_its_indexes() {
        let mut rt = Rt::new();
        crate::install();
        let sci = |rt: &mut Rt, name: &'static str, desc: &'static str, args: &[V]| {
            let key = rt.static_key(&format!("java/text/StringCharacterIterator.{name}.{desc}"));
            crate::api::call(rt, key, args).i()
        };
        let text = rt.string("xx Hello, world!");
        let chars = rt.new_object(STRING_ITERATOR);
        sci(&mut rt, "<init>", "(Ljava/lang/String;III)V", &[chars, text, V::I(3), V::I(15), V::I(9)]);
        let it = bi(&mut rt, "getLineInstance", "()Ljava/text/BreakIterator;", &[]);
        bi(&mut rt, "setText", "(Ljava/text/CharacterIterator;)V", &[it, chars]);
        assert_eq!(bi(&mut rt, "current", "()I", &[it]), V::I(3));
        assert_eq!(sci(&mut rt, "getIndex", "()I", &[chars]), 3);
        let walk: Vec<i32> = ["first", "next", "next", "next"].iter().map(|&n| bi(&mut rt, n, "()I", &[it]).i()).collect();
        assert_eq!(walk, [3, 10, 15, DONE]);
        bi(&mut rt, "following", "(I)I", &[it, V::I(2)]);
        assert!(rt.thrown.take().is_some(), "an offset before the iterator's begin is out of bounds");
        assert_eq!(bi(&mut rt, "preceding", "(I)I", &[it, V::I(15)]), V::I(10));

        let abc = rt.string("abc");
        let s = rt.new_object(STRING_ITERATOR);
        sci(&mut rt, "<init>", "(Ljava/lang/String;)V", &[s, abc]);
        let steps: Vec<i32> = ["first", "next", "next", "next", "getIndex", "previous", "last", "current"].iter().map(|&n| sci(&mut rt, n, "()C", &[s])).collect();
        assert_eq!(steps, [97, 98, 99, CHAR_DONE, 3, 99, 99, 99]);
        sci(&mut rt, "setIndex", "(I)C", &[s, V::I(4)]);
        assert!(rt.thrown.take().is_some(), "an index past the end is invalid");
        assert_eq!(sci(&mut rt, "setIndex", "(I)C", &[s, V::I(3)]), CHAR_DONE);

        let words = bi(&mut rt, "getWordInstance", "()Ljava/text/BreakIterator;", &[]);
        let go = rt.string("go now");
        bi(&mut rt, "setText", "(Ljava/lang/String;)V", &[words, go]);
        let back = bi(&mut rt, "getText", "()Ljava/text/CharacterIterator;", &[words]);
        let range: Vec<i32> = ["getBeginIndex", "getEndIndex", "getIndex"].iter().map(|&n| sci(&mut rt, n, "()I", &[back])).collect();
        assert_eq!(range, [0, 6, 0]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
