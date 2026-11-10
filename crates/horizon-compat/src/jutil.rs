//! `java.util` collections in native Rust: lists, hash maps and sets (insertion ordered, so
//! iteration is stable), sorted maps, iterators, map entries, a working subset of streams, and
//! the small helpers mods lean on (`Objects`, `Collections`, `List.of`, atomics).
//!
//! Keys compare as Java compares them: strings and boxed values by content, Horizon's native
//! values (positions, states) by value, and a mod object through its own translated `equals`
//! and `hashCode` when it overrides them, by identity otherwise.
use crate::{Data, Object, Rt, V};

pub use crate::Table;

/// Whether a table finds its keys by its comparator alone, as a `TreeMap` made with one does
/// (`new TreeMap<>(String.CASE_INSENSITIVE_ORDER)` holds one of `"a"` and `"A"`): such a table
/// keeps no hash index, and its keys are never asked for a hash. A sorted table without a
/// comparator keeps finding its keys by hash and `equals`: the natural order of a key the runtime
/// can't compare is no order to tell two keys apart by.
fn by_comparator(t: &Table) -> bool {
    t.sorted && t.comparator.r() != 0
}

/// Where `k` is in a table sorted by its comparator, or where it goes (`Err`).
fn search(rt: &mut Rt, t: &Table, k: V) -> Result<usize, usize> {
    let (mut low, mut high) = (0, t.entries.len());
    while low < high {
        let mid = (low + high) / 2;
        match compare(rt, t.comparator, k, t.entries[mid].0) {
            0 => return Ok(mid),
            c if c < 0 => high = mid,
            _ => low = mid + 1,
        }
    }
    Err(low)
}

/// The hash index after an entry with hash `h` was put in at `at`: the entries behind it moved
/// up by one. No key is hashed again for it (a table of ten thousand keys with a `hashCode` of
/// their mod's own was hashed whole at every insertion into a sorted map and at every removal).
fn index_inserted(t: &mut Table, at: usize, h: i32) {
    if at + 1 < t.entries.len() {
        for slots in t.index.values_mut() {
            for s in slots.iter_mut().filter(|s| **s >= at) {
                *s += 1;
            }
        }
    }
    t.index.entry(h).or_default().push(at);
}

/// The hash index after the entry at `at`, with hash `h`, was taken out: the entries behind it
/// moved down by one.
fn index_removed(t: &mut Table, at: usize, h: i32) {
    if let Some(slots) = t.index.get_mut(&h) {
        slots.retain(|s| *s != at);
        if slots.is_empty() {
            t.index.remove(&h);
        }
    }
    if at < t.entries.len() {
        for slots in t.index.values_mut() {
            for s in slots.iter_mut().filter(|s| **s > at) {
                *s -= 1;
            }
        }
    }
}

const STRATEGY: &str = "it/unimi/dsi/fastutil/Hash$Strategy";
const STRATEGY_HASH: &str = "it/unimi/dsi/fastutil/Hash$Strategy.hashCode.(Ljava/lang/Object;)I";
const STRATEGY_EQUALS: &str = "it/unimi/dsi/fastutil/Hash$Strategy.equals.(Ljava/lang/Object;Ljava/lang/Object;)Z";

/// A key's hash in a table with `strategy` (fastutil's custom-hash collections): the strategy's,
/// else the key's own `hashCode`.
fn key_hash(rt: &mut Rt, strategy: V, k: V) -> i32 {
    if strategy.r() != 0 { rt.call_functional(STRATEGY_HASH, strategy, &[k]).i() } else { hash(rt, k) }
}

/// Whether two keys are one in a table with `strategy`: the strategy's `equals`, else the key's.
fn key_equals(rt: &mut Rt, strategy: V, a: V, b: V) -> bool {
    if strategy.r() != 0 { rt.call_functional(STRATEGY_EQUALS, strategy, &[a, b]).i() != 0 } else { equals(rt, a, b) }
}

const EQUALS: &str = "equals.(Ljava/lang/Object;)Z";
const HASH_CODE: &str = "hashCode.()I";
const OPTIONAL: &str = "java/util/Optional";
const COMPARE_TO: &str = "compareTo.(Ljava/lang/Object;)I";

fn overrides(rt: &Rt, v: V, key: &'static str) -> bool {
    rt.class_overrides(rt.class_of(v), key)
}

/// `a.equals(b)` as Java answers it: the object's own `equals` when its mod class overrides it.
pub fn equals(rt: &mut Rt, a: V, b: V) -> bool {
    if equals_own(rt, a, b) {
        return true;
    }
    if overrides(rt, a, EQUALS)
        && let Some(vc) = rt.vcall
    {
        return vc(rt, "java/lang/Object.equals.(Ljava/lang/Object;)Z", EQUALS, &[a, b]).i() != 0;
    }
    false
}

/// `equals` without a mod's override: what `Object.equals` (and `super.equals` up to it) answers
/// for the runtime's own values (strings, boxes, positions, lists by content; objects by identity).
pub fn equals_own(rt: &mut Rt, a: V, b: V) -> bool {
    if a == b || rt.java_equals(a, b) {
        return true;
    }
    let (V::R(x), V::R(y)) = (a, b) else { return false };
    if x == 0 || y == 0 {
        return false;
    }
    // Gson's elements are values (`JsonObject.equals` compares members).
    if crate::gson::is_element_class(rt.class_of(a)) || crate::gson::is_element_class(rt.class_of(b)) {
        return crate::gson::same(rt, a, b);
    }
    match (rt.heap.get(x as usize).map(|o| &o.data), rt.heap.get(y as usize).map(|o| &o.data)) {
        (Some(Data::Native(k1, h1)), Some(Data::Native(k2, h2))) => return k1 == k2 && h1 == h2,
        (Some(Data::List(l1)), Some(Data::List(l2))) if l1.len() == l2.len() => {
            let (l1, l2) = (l1.clone(), l2.clone());
            return l1.iter().zip(&l2).all(|(p, q)| equals(rt, *p, *q));
        }
        _ => {}
    }
    for c in ["java/lang/Long", "java/lang/Boolean", "java/lang/Double", "java/lang/Float", "java/lang/Character", "java/lang/Short", "java/lang/Byte"] {
        if rt.class_of(a) == c && rt.class_of(b) == c {
            return rt.get_field(a, "value") == rt.get_field(b, "value");
        }
    }
    // JOML's vectors are values: their public fields.
    if rt.class_of(a).starts_with("org/joml/Vector") && rt.class_of(a) == rt.class_of(b) {
        return ["x", "y", "z", "w"].iter().all(|f| rt.get_field(a, f) == rt.get_field(b, f));
    }
    // Pairs are values (Registrate keys its pending callbacks by `Pair.of(name, registry)`).
    if is_pair(rt.class_of(a)) && is_pair(rt.class_of(b)) {
        let (l1, l2, r1, r2) = (rt.get_field(a, "$left"), rt.get_field(b, "$left"), rt.get_field(a, "$right"), rt.get_field(b, "$right"));
        return (l1 == l2 || equals(rt, l1, l2)) && (r1 == r2 || equals(rt, r1, r2));
    }
    // A stack's custom data is a value: its tag (`CustomData.equals`).
    if let Some(same) = crate::components::same_custom_data(rt, a, b) {
        return same;
    }
    // `Optional`s are values: what they hold (`Optional.equals`).
    if rt.class_of(a) == OPTIONAL && rt.class_of(b) == OPTIONAL {
        return crate::api_common::call(rt, OPTIONAL, "equals", "(Ljava/lang/Object;)Z", &[a, b]).is_some_and(|v| v.i() != 0);
    }
    // Vanilla's records and data classes made here (`PotionContents`): their own `equals`.
    if let Some(same) = crate::records::equals(rt, a, b) {
        return same;
    }
    false
}

/// commons-lang's, DataFixerUpper's and fastutil's pairs (the runtime keeps them all as `$left`,
/// `$right`).
fn is_pair(class: &str) -> bool {
    matches!(
        class,
        "org/apache/commons/lang3/tuple/Pair" | "org/apache/commons/lang3/tuple/ImmutablePair" | "org/apache/commons/lang3/tuple/MutablePair" | "com/mojang/datafixers/util/Pair"
    ) || fast_pair(class)
}

/// Whether `class` is one of fastutil's pairs: `Pair` itself, or a pair of its typed packages
/// (`ints/IntIntPair`, `objects/ObjectIntImmutablePair`...).
pub(crate) fn fast_pair(class: &str) -> bool {
    class == "it/unimi/dsi/fastutil/Pair"
        || (class.starts_with("it/unimi/dsi/fastutil/")
            && class.ends_with("Pair")
            && !class.contains("Sorted"))
}

/// `v.hashCode()` as Java answers it: the object's own `hashCode` when its mod class overrides it.
pub fn hash(rt: &mut Rt, v: V) -> i32 {
    if overrides(rt, v, HASH_CODE)
        && let Some(vc) = rt.vcall
    {
        return vc(rt, "java/lang/Object.hashCode.()I", HASH_CODE, &[v]).i();
    }
    hash_own(rt, v)
}

/// `hashCode` without a mod's override: what `Object.hashCode` (and `super.hashCode` up to it)
/// answers (content for strings, boxes and positions; identity for objects).
pub fn hash_own(rt: &mut Rt, v: V) -> i32 {
    if crate::gson::is_element_class(rt.class_of(v)) {
        return crate::gson::hash_of(rt, v);
    }
    if rt.class_of(v) == OPTIONAL {
        return crate::api_common::call(rt, OPTIONAL, "hashCode", "()I", &[v]).map_or(0, |h| h.i());
    }
    if let Some(h) = crate::components::custom_data_hash(rt, v) {
        return h;
    }
    if let V::R(r) = v
        && r != 0
    {
        match rt.heap.get(r as usize).map(|o| &o.data) {
            Some(Data::Native(k, h)) => {
                let mut x = k.bytes().fold(17i32, |a, b| a.wrapping_mul(31).wrapping_add(b as i32));
                for p in h {
                    x = x.wrapping_mul(31).wrapping_add((*p ^ (*p >> 32)) as i32);
                }
                return x;
            }
            Some(Data::Fields(f)) if f.contains_key("value") && rt.class_of(v).starts_with("java/lang/") => {
                return rt.java_hash(f["value"]);
            }
            // A map entry's: its key's hash and its value's, xor'd.
            Some(Data::List(kv)) if kv.len() == 2 && is_entry(rt.class_of(v)) => {
                let (k, x) = (kv[0], kv[1]);
                let half = |rt: &mut Rt, x: V| if x.is_null() { 0 } else { hash(rt, x) };
                return half(rt, k) ^ half(rt, x);
            }
            // JOML's `Vector2i.hashCode`: `31 * (31 + x) + y`.
            Some(Data::Fields(_)) if rt.class_of(v) == "org/joml/Vector2i" => {
                let (x, y) = (rt.get_field(v, "x").i(), rt.get_field(v, "y").i());
                return 31i32.wrapping_mul(31i32.wrapping_add(x)).wrapping_add(y);
            }
            // commons-lang's `Pair.hashCode`: the halves' hashes, xor'd.
            Some(Data::Fields(_)) if is_pair(rt.class_of(v)) => {
                let (l, r) = (rt.get_field(v, "$left"), rt.get_field(v, "$right"));
                let half = |rt: &mut Rt, x: V| if x.r() == 0 && matches!(x, V::R(_)) { 0 } else { hash(rt, x) };
                return half(rt, l) ^ half(rt, r);
            }
            _ => {}
        }
    }
    if let Some(h) = crate::records::hash(rt, v) {
        return h;
    }
    rt.java_hash(v)
}

/// `a.compareTo(b)` (or `comparator.compare(a, b)`), for sorting.
pub fn compare(rt: &mut Rt, comparator: V, a: V, b: V) -> i32 {
    if comparator.r() != 0 {
        if rt.class_of(comparator) == "java/util/Comparator" && !matches!(rt.get_field(comparator, "$how"), V::Void) {
            return compare_built(rt, comparator, a, b);
        }
        return rt.call_functional("java/util/Comparator.compare.(Ljava/lang/Object;Ljava/lang/Object;)I", comparator, &[a, b]).i();
    }
    if let (Some(x), Some(y)) = (rt.str_of(a), rt.str_of(b)) {
        return crate::jlang::java_compare(x, y);
    }
    let unbox = |rt: &Rt, v: V| match v {
        V::R(_) if rt.class_of(v).starts_with("java/lang/") => rt.get_field(v, "value"),
        v => v,
    };
    match (unbox(rt, a), unbox(rt, b)) {
        (V::I(x), V::I(y)) => x.cmp(&y) as i32,
        (V::J(x), V::J(y)) => x.cmp(&y) as i32,
        (V::F(x), V::F(y)) => x.total_cmp(&y) as i32,
        (V::D(x), V::D(y)) => x.total_cmp(&y) as i32,
        _ if overrides(rt, a, COMPARE_TO) => match rt.vcall {
            Some(vc) => vc(rt, "java/lang/Comparable.compareTo.(Ljava/lang/Object;)I", COMPARE_TO, &[a, b]).i(),
            None => 0,
        },
        // Enums: by ordinal (a direction's or an axis's too).
        _ if !matches!(rt.get_field(a, "$ordinal"), V::Void)
            || matches!(
                rt.class_of(a),
                "net/minecraft/core/Direction" | "net/minecraft/core/Direction$Axis"
            ) =>
        {
            crate::api_common::enum_ordinal(rt, a).cmp(&crate::api_common::enum_ordinal(rt, b))
                as i32
        }
        // One of the runtime's own values that Java compares (an id, a UUID, a path, an instant):
        // by its class's `compareTo`.
        _ if matches!(a, V::R(r) if r != 0) && !rt.is_mod_class(rt.class_of(a)) => {
            let key = rt.static_key(&format!(
                "{}.compareTo.(Ljava/lang/Object;)I",
                rt.class_of(a)
            ));
            match crate::api::call(rt, key, &[a, b]) {
                V::I(c) => c,
                _ => 0,
            }
        }
        _ => 0,
    }
}

/// A `BitSet`'s words, low bits first.
fn bit_words(rt: &Rt, v: V) -> Vec<u64> {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Array(w)) => w.iter().map(|x| x.j() as u64).collect(),
        _ => Vec::new(),
    }
}

fn set_bit_words(rt: &mut Rt, v: V, mut words: Vec<u64>) {
    while words.last() == Some(&0) {
        words.pop();
    }
    // Never `null`'s: the heap's first object is what every null reference reads.
    if v.r() != 0
        && let Some(o) = rt.heap.get_mut(v.r() as usize)
    {
        o.data = Data::Array(words.into_iter().map(|w| V::J(w as i64)).collect());
    }
}

/// Bit `i` of a `BitSet` as it was, after writing it in place: `to` when given, its opposite when
/// `flip`. The words stay as [`set_bit_words`] leaves them, with no zero word at the end.
fn one_bit(rt: &mut Rt, set: V, i: usize, to: Option<bool>, flip: bool) -> bool {
    if set.r() == 0 {
        return false;
    }
    let Some(data) = rt.heap.get_mut(set.r() as usize).map(|o| &mut o.data) else {
        return false;
    };
    if !matches!(data, Data::Array(_)) {
        *data = Data::Array(Vec::new());
    }
    let Data::Array(words) = data else {
        return false;
    };
    let (word, mask) = (i / 64, 1i64 << (i % 64));
    let was = words.get(word).is_some_and(|w| w.j() & mask != 0);
    let now = match to {
        Some(on) => on,
        None if flip => !was,
        None => return was,
    };
    if now && words.len() <= word {
        words.resize(word + 1, V::J(0));
    }
    if let Some(w) = words.get_mut(word) {
        *w = V::J(if now { w.j() | mask } else { w.j() & !mask });
    }
    while !now && words.last().is_some_and(|w| w.j() == 0) {
        words.pop();
    }
    was
}

fn new_bitset(rt: &mut Rt, words: Vec<u64>) -> V {
    let b = rt.new_object("java/util/BitSet");
    set_bit_words(rt, b, words);
    b
}

/// `java.util.BitSet`, as the JDK's: bits by index in 64-bit words (Create keeps which of a
/// contraption's block entities to draw in one).
fn bitset_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // One bit, read or written where the set keeps it: a mod fills sets of millions of bits one
    // by one (Chisels & Bits' sphere shapes: 19 million `set` calls for each).
    if let ("set" | "clear" | "flip" | "get", "(I)V" | "(IZ)V" | "(I)Z", Ok(i)) =
        (name, desc, usize::try_from(a(1).i()))
    {
        let to = match (name, desc) {
            ("set", "(IZ)V") => Some(a(2).i() != 0),
            ("set", _) => Some(true),
            ("clear", _) => Some(false),
            _ => None,
        };
        let was = one_bit(rt, a(0), i, to, name == "flip");
        return Some(if name == "get" {
            V::I(was as i32)
        } else {
            V::Void
        });
    }
    let mut w = bit_words(rt, a(0));
    let get = |w: &[u64], i: usize| w.get(i / 64).is_some_and(|x| x >> (i % 64) & 1 != 0);
    let put = |w: &mut Vec<u64>, i: usize, on: bool| {
        if w.len() <= i / 64 {
            w.resize(i / 64 + 1, 0);
        }
        if on { w[i / 64] |= 1 << (i % 64) } else { w[i / 64] &= !(1 << (i % 64)) }
    };
    let index = |v: V| -> Option<usize> { usize::try_from(v.i()).ok() };
    // A negative index throws, as the JDK's.
    let bad = |rt: &mut Rt| {
        rt.throw_new("java/lang/IndexOutOfBoundsException");
        V::Void
    };
    let len = |w: &[u64]| w.iter().rposition(|&x| x != 0).map_or(0, |i| i * 64 + 64 - w[i].leading_zeros() as usize);
    Some(match name {
        "<init>" => {
            set_bit_words(rt, a(0), Vec::new());
            V::Void
        }
        "valueOf" if desc.starts_with("([J)") => {
            let words = elements(rt, a(0)).into_iter().map(|x| x.j() as u64).collect();
            new_bitset(rt, words)
        }
        "set" | "clear" | "flip" => {
            let all = name == "clear" && desc == "()V";
            let (from, to, on) = match desc {
                "()V" => (0, 0, false),
                "(I)V" => (a(1).i(), a(1).i().saturating_add(1), name == "set"),
                "(IZ)V" => (a(1).i(), a(1).i().saturating_add(1), a(2).i() != 0),
                "(II)V" => (a(1).i(), a(2).i(), name == "set"),
                "(IIZ)V" => (a(1).i(), a(2).i(), a(3).i() != 0),
                _ => return None,
            };
            if all {
                w.clear();
            } else if from < 0 || to < from {
                return Some(bad(rt));
            } else {
                for i in from as usize..to as usize {
                    let now = if name == "flip" { !get(&w, i) } else { on };
                    put(&mut w, i, now);
                }
            }
            set_bit_words(rt, a(0), w);
            V::Void
        }
        "get" if desc == "(I)Z" => match index(a(1)) {
            Some(i) => V::I(get(&w, i) as i32),
            None => bad(rt),
        },
        "nextSetBit" | "nextClearBit" => {
            let Some(from) = index(a(1)) else { return Some(bad(rt)) };
            let want = name == "nextSetBit";
            let end = w.len() * 64;
            let found = (from..end).find(|&i| get(&w, i) == want);
            V::I(match found {
                Some(i) => i as i32,
                None if want => -1,
                None => from.max(end) as i32,
            })
        }
        "previousSetBit" => {
            let from = a(1).i();
            V::I((0..=from.max(-1)).rev().find(|&i| i >= 0 && get(&w, i as usize)).unwrap_or(-1))
        }
        "cardinality" => V::I(w.iter().map(|x| x.count_ones() as i32).sum()),
        "isEmpty" => V::I(w.iter().all(|&x| x == 0) as i32),
        "length" => V::I(len(&w) as i32),
        "size" => V::I((w.len() * 64) as i32),
        "and" | "or" | "xor" | "andNot" => {
            let o = bit_words(rt, a(1));
            let n = w.len().max(o.len());
            w.resize(n, 0);
            for (i, x) in w.iter_mut().enumerate() {
                let y = o.get(i).copied().unwrap_or(0);
                *x = match name {
                    "and" => *x & y,
                    "or" => *x | y,
                    "xor" => *x ^ y,
                    _ => *x & !y,
                };
            }
            set_bit_words(rt, a(0), w);
            V::Void
        }
        "intersects" => {
            let o = bit_words(rt, a(1));
            V::I(w.iter().zip(&o).any(|(x, y)| x & y != 0) as i32)
        }
        "clone" => new_bitset(rt, w),
        "equals" => V::I((rt.class_of(a(1)) == "java/util/BitSet" && {
            let mut o = bit_words(rt, a(1));
            while o.last() == Some(&0) {
                o.pop();
            }
            o == w
        }) as i32),
        // The JDK's: the words folded into a long, then its halves xor'd.
        "hashCode" => {
            let mut h: i64 = 1234;
            for (i, &x) in w.iter().enumerate().rev() {
                h ^= (x as i64).wrapping_mul(i as i64 + 1);
            }
            V::I(((h >> 32) ^ h) as i32)
        }
        "toLongArray" => {
            let items = w.iter().map(|&x| V::J(x as i64)).collect();
            rt.alloc("[J", Data::Array(items))
        }
        "stream" => {
            let items = (0..w.len() * 64).filter(|&i| get(&w, i)).map(|i| V::I(i as i32)).collect();
            rt.alloc("java/util/stream/IntStream", Data::Stream(items))
        }
        _ => return None,
    })
}

/// `Comparator`'s own combinators (`comparing`, `thenComparing`, `reversed`, `nullsFirst`...): each
/// makes a comparator whose `$how` says how it compares, on what (`$x`, `$y`); `compare` reads it
/// back. Their receiver can be any comparator, a lambda's included.
fn comparator_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    fn made(rt: &mut Rt, how: &str, x: V, y: V) -> V {
        let c = rt.new_object("java/util/Comparator");
        let h = rt.string(how);
        rt.put_field(c, "$how", h);
        rt.put_field(c, "$x", x);
        rt.put_field(c, "$y", y);
        c
    }
    // `thenComparing(...)`'s second comparator, from what follows the receiver.
    let key_of = |rt: &mut Rt, at: usize| -> V {
        match desc {
            d if d.starts_with("(Ljava/util/function/Function;Ljava/util/Comparator;)") => made(rt, "key", a(at), a(at + 1)),
            d if d.starts_with("(Ljava/util/function/Function;)") => made(rt, "key", a(at), V::NULL),
            d if d.starts_with("(Ljava/util/function/ToIntFunction;)") => made(rt, "int", a(at), V::NULL),
            d if d.starts_with("(Ljava/util/function/ToLongFunction;)") => made(rt, "long", a(at), V::NULL),
            d if d.starts_with("(Ljava/util/function/ToDoubleFunction;)") => made(rt, "double", a(at), V::NULL),
            _ => a(at),
        }
    };
    Some(match name {
        "comparing" | "comparingInt" | "comparingLong" | "comparingDouble" => key_of(rt, 0),
        "naturalOrder" => made(rt, "natural", V::NULL, V::NULL),
        "reverseOrder" => made(rt, "reversed", V::NULL, V::NULL),
        "nullsFirst" => made(rt, "nulls-first", a(0), V::NULL),
        "nullsLast" => made(rt, "nulls-last", a(0), V::NULL),
        "reversed" => made(rt, "reversed", a(0), V::NULL),
        "thenComparing" | "thenComparingInt" | "thenComparingLong" | "thenComparingDouble" => {
            let second = key_of(rt, 1);
            made(rt, "then", a(0), second)
        }
        "compare" if !matches!(rt.get_field(a(0), "$how"), V::Void) => V::I(compare_built(rt, a(0), a(1), a(2))),
        _ => return None,
    })
}

/// `String.CASE_INSENSITIVE_ORDER`: one comparator, as the JDK's constant is.
pub(crate) fn case_insensitive_order(rt: &mut Rt) -> V {
    rt.intern("compat:case-insensitive-order".into(), |rt| {
        let c = rt.new_object("java/util/Comparator");
        let how = rt.string("ignore-case");
        rt.put_field(c, "$how", how);
        c
    })
}

/// What a comparator made by [`comparator_call`] answers for `(x, y)`.
fn compare_built(rt: &mut Rt, c: V, x: V, y: V) -> i32 {
    let how = rt.str_of(rt.get_field(c, "$how")).unwrap_or("").to_owned();
    let (f, g) = (rt.get_field(c, "$x"), rt.get_field(c, "$y"));
    match how.as_str() {
        "ignore-case" => {
            let (x, y) = (
                rt.str_of(x).unwrap_or("").to_owned(),
                rt.str_of(y).unwrap_or(""),
            );
            crate::jlang::compare_ignore_case(&x, y)
        }
        "key" => {
            let (kx, ky) = (rt.call_functional(APPLY, f, &[x]), rt.call_functional(APPLY, f, &[y]));
            compare(rt, g, kx, ky)
        }
        // `Map.Entry.comparingByKey` and `comparingByValue`: each entry's own half (an entry of
        // a mod's own map class answers for itself).
        "entry" => {
            let half = rt.str_of(f).unwrap_or("").to_owned();
            let half: &'static str = rt.static_key(&half);
            let (kx, ky) = (rt.call_functional(half, x, &[]), rt.call_functional(half, y, &[]));
            compare(rt, g, kx, ky)
        }
        "int" => {
            let k = "java/util/function/ToIntFunction.applyAsInt.(Ljava/lang/Object;)I";
            let (kx, ky) = (rt.call_functional(k, f, &[x]).i(), rt.call_functional(k, f, &[y]).i());
            kx.cmp(&ky) as i32
        }
        "long" => {
            let k = "java/util/function/ToLongFunction.applyAsLong.(Ljava/lang/Object;)J";
            let (kx, ky) = (rt.call_functional(k, f, &[x]).j(), rt.call_functional(k, f, &[y]).j());
            kx.cmp(&ky) as i32
        }
        "double" => {
            let k = "java/util/function/ToDoubleFunction.applyAsDouble.(Ljava/lang/Object;)D";
            let (kx, ky) = (rt.call_functional(k, f, &[x]).d(), rt.call_functional(k, f, &[y]).d());
            kx.total_cmp(&ky) as i32
        }
        "natural" => compare(rt, V::NULL, x, y),
        "reversed" => compare(rt, f, y, x),
        "then" => match compare(rt, f, x, y) {
            0 => compare(rt, g, x, y),
            r => r,
        },
        "nulls-first" | "nulls-last" => {
            let first = if how == "nulls-first" { -1 } else { 1 };
            match (x.r() == 0, y.r() == 0) {
                (true, true) => 0,
                (true, false) => first,
                (false, true) => -first,
                _ if f.r() == 0 => 0,
                _ => compare(rt, f, x, y),
            }
        }
        _ => 0,
    }
}

const NON_NULL_LIST: &str = "net/minecraft/core/NonNullList";

fn list(rt: &Rt, v: V) -> Option<&Vec<V>> {
    match &rt.heap.get(v.r() as usize)?.data {
        Data::List(l) => Some(l),
        _ => None,
    }
}

fn list_mut(rt: &mut Rt, v: V) -> Option<&mut Vec<V>> {
    match &mut rt.heap.get_mut(v.r() as usize)?.data {
        Data::List(l) => Some(l),
        _ => None,
    }
}

fn take_table(rt: &mut Rt, v: V) -> Option<Table> {
    match &mut rt.heap.get_mut(v.r() as usize)?.data {
        Data::Table(t) => Some(std::mem::take(t)),
        _ => None,
    }
}

/// An empty table that orders and compares its keys as `this` does: `clear()` keeps a
/// `TreeMap`'s comparator and a custom-hash set's strategy.
fn emptied(rt: &Rt, this: V) -> Table {
    match rt.heap.get(this.r() as usize).map(|o| &o.data) {
        Some(Data::Table(t)) => Table { sorted: t.sorted, comparator: t.comparator, strategy: t.strategy, ..Table::default() },
        _ => Table::default(),
    }
}

fn put_table(rt: &mut Rt, v: V, t: Table) {
    if let Some(o) = rt.heap.get_mut(v.r() as usize) {
        o.data = Data::Table(t);
    }
}

fn find(rt: &mut Rt, t: &Table, k: V) -> Option<usize> {
    if by_comparator(t) {
        return search(rt, t, k).ok();
    }
    let h = key_hash(rt, t.strategy, k);
    let slots = t.index.get(&h)?;
    slots
        .iter()
        .copied()
        .find(|&i| key_equals(rt, t.strategy, t.entries[i].0, k))
}

/// The other way round of the `BiMap` at `m`, if it is one with an `inverse()` made: Guava's is
/// a view of the same entries, so a `BiMap` here is two tables kept as one, each naming the other
/// in its comparator slot, which a table that isn't sorted has no other use for (a fastutil map
/// keeps a number there, never a table that names it back).
fn inverse_of(rt: &Rt, m: V) -> Option<V> {
    let other = |v: V| match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Table(t)) if !t.sorted && t.comparator.r() != 0 => Some(t.comparator),
        _ => None,
    };
    let inverse = other(m)?;
    (other(inverse)? == m).then_some(inverse)
}

/// Makes the tables at `a` and `b` each other's inverse ([`inverse_of`]).
fn pair(rt: &mut Rt, a: V, b: V) {
    for (this, other) in [(a, b), (b, a)] {
        if let Some(Data::Table(t)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
            t.comparator = other;
        }
    }
}

/// An empty `BiMap` of class `class` with its inverse.
fn new_bimap(rt: &mut Rt, class: &'static str) -> V {
    let m = rt.alloc(class, Data::Table(Table::default()));
    let inverse = rt.alloc(class, Data::Table(Table::default()));
    pair(rt, m, inverse);
    m
}

/// `put` (or `forcePut`, with `force`) of a `BiMap` and its inverse, as Guava's `HashBiMap`: a
/// value has one key, so a value another key holds is refused (`IllegalArgumentException`) or,
/// forced, that key goes; the key's old value is no key of the inverse any more. An entry
/// keeps its place in both when its value changes.
fn put_both(rt: &mut Rt, m: V, inverse: V, k: V, v: V, force: bool) -> V {
    let same =
        |rt: &mut Rt, a: V, b: V| (is_null(a) && is_null(b)) || (!is_null(a) && equals(rt, a, b));
    let had = get_in(rt, m, k);
    if let Some(old) = had
        && same(rt, old, v)
    {
        return old;
    }
    if let Some(other) = get_in(rt, inverse, v) {
        if !force {
            let text = crate::jlang::text(rt, v);
            return crate::jlang::throw_with(
                rt,
                "java/lang/IllegalArgumentException",
                &format!("value already present: {text}"),
            );
        }
        take_out(rt, m, other);
        take_out(rt, inverse, v);
    }
    let moved = match had {
        Some(old) => rekey_in(rt, inverse, old, v),
        None => false,
    };
    if !moved {
        put_in(rt, inverse, v, k);
    }
    put_in(rt, m, k, v);
    had.unwrap_or(V::NULL)
}

/// Gives the entry of key `old` in the table at `m` the key `k`, where it is; whether there
/// was one.
fn rekey_in(rt: &mut Rt, m: V, old: V, k: V) -> bool {
    let Some(mut t) = take_table(rt, m) else {
        return false;
    };
    let at = find(rt, &t, old);
    if let Some(at) = at {
        let (was, now) = (key_hash(rt, t.strategy, old), key_hash(rt, t.strategy, k));
        if let Some(slots) = t.index.get_mut(&was) {
            slots.retain(|s| *s != at);
            if slots.is_empty() {
                t.index.remove(&was);
            }
        }
        t.entries[at].0 = k;
        t.index.entry(now).or_default().push(at);
    }
    put_table(rt, m, t);
    at.is_some()
}

/// Puts `k → v` in the table at `m`; the previous value, or null. In a `BiMap` with its inverse
/// ([`inverse_of`]), in both.
pub fn table_put(rt: &mut Rt, m: V, k: V, v: V) -> V {
    match inverse_of(rt, m) {
        Some(inverse) => put_both(rt, m, inverse, k, v, false),
        None => put_in(rt, m, k, v),
    }
}

/// [`table_put`] in the one table at `m`.
fn put_in(rt: &mut Rt, m: V, k: V, v: V) -> V {
    let Some(mut t) = take_table(rt, m) else { return V::NULL };
    if by_comparator(&t) {
        let old = match search(rt, &t, k) {
            Ok(i) => std::mem::replace(&mut t.entries[i].1, v),
            Err(at) => {
                t.entries.insert(at, (k, v));
                V::NULL
            }
        };
        put_table(rt, m, t);
        return old;
    }
    let old = match find(rt, &t, k) {
        Some(i) => std::mem::replace(&mut t.entries[i].1, v),
        None => {
            let h = key_hash(rt, t.strategy, k);
            if t.sorted {
                // Before the first key it is less than: found by halving, the keys being in
                // order (after the last, for a key that compares to none).
                let (mut at, mut high) = (0, t.entries.len());
                while at < high {
                    let mid = (at + high) / 2;
                    if compare(rt, V::NULL, k, t.entries[mid].0) < 0 {
                        high = mid;
                    } else {
                        at = mid + 1;
                    }
                }
                t.entries.insert(at, (k, v));
                index_inserted(&mut t, at, h);
            } else {
                t.entries.push((k, v));
                t.index.entry(h).or_default().push(t.entries.len() - 1);
            }
            V::NULL
        }
    };
    put_table(rt, m, t);
    old
}

pub fn table_get(rt: &mut Rt, m: V, k: V) -> Option<V> {
    get_in(rt, m, k)
}

fn get_in(rt: &mut Rt, m: V, k: V) -> Option<V> {
    let t = take_table(rt, m)?;
    let r = find(rt, &t, k).map(|i| t.entries[i].1);
    put_table(rt, m, t);
    r
}

/// Takes `k` out of the table at `m`; its value, if it was there. Out of a `BiMap` with its
/// inverse ([`inverse_of`]), out of both.
pub fn table_remove(rt: &mut Rt, m: V, k: V) -> Option<V> {
    let inverse = inverse_of(rt, m);
    let value = take_out(rt, m, k)?;
    if let Some(inverse) = inverse {
        take_out(rt, inverse, value);
    }
    Some(value)
}

/// [`table_remove`] out of the one table at `m`.
fn take_out(rt: &mut Rt, m: V, k: V) -> Option<V> {
    let mut t = take_table(rt, m)?;
    let r = if by_comparator(&t) {
        search(rt, &t, k).ok().map(|i| t.entries.remove(i).1)
    } else {
        let h = key_hash(rt, t.strategy, k);
        let at = t.index.get(&h).and_then(|slots| {
            slots
                .iter()
                .copied()
                .find(|&i| key_equals(rt, t.strategy, t.entries[i].0, k))
        });
        at.map(|i| {
            let value = t.entries.remove(i).1;
            index_removed(&mut t, i, h);
            value
        })
    };
    put_table(rt, m, t);
    r
}

/// The key-value pairs of the map at `m`, in its order (none when it isn't one).
pub fn table_pairs(rt: &Rt, m: V) -> Vec<(V, V)> {
    table_entries(rt, m)
}

/// The keys of the map at `m`, in its order.
pub fn table_keys(rt: &Rt, m: V) -> Vec<V> {
    table_entries(rt, m).into_iter().map(|(k, _)| k).collect()
}

/// `Collection.toArray()` and `toArray(array)`: a new array of the items, or the array given
/// where it holds them all, filled, with a null after the last when it is longer (a mod builds
/// a path as `stack.toArray(new String[stack.size() + 1])` and sets the last itself).
fn to_array(rt: &mut Rt, items: Vec<V>, given: V) -> V {
    if given.r() != 0 && rt.class_of(given).starts_with('[') {
        let len = rt.array_len(given).max(0) as usize;
        if len >= items.len() {
            for (i, v) in items.iter().enumerate() {
                rt.array_set(given, i as i32, *v);
            }
            if len > items.len() {
                rt.array_set(given, items.len() as i32, V::NULL);
            }
            return given;
        }
    }
    rt.alloc("[Ljava/lang/Object;", Data::Array(items))
}

/// How many entries the map or set at `m` has: counted where they are. A set's every call asked
/// its size by copying all its entries first, so filling one with a pack's block states (Puzzles
/// Lib's model locations) took time by the square of their number, and never ended.
fn table_len(rt: &Rt, m: V) -> usize {
    match rt.heap.get(m.r() as usize).map(|o| &o.data) {
        Some(Data::Table(t)) => t.entries.len(),
        _ => 0,
    }
}

fn table_entries(rt: &Rt, m: V) -> Vec<(V, V)> {
    match rt.heap.get(m.r() as usize).map(|o| &o.data) {
        Some(Data::Table(t)) => t.entries.clone(),
        _ => Vec::new(),
    }
}

thread_local! {
    /// Whether a mod's collection is being read ([`own_elements`]): what it calls meanwhile is
    /// answered as it is, not read in turn.
    static READING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What a call on the runtime's collections reads its first argument for and leaves as it is: one
/// of a mod's own class there is read like the arguments after it. Any other call's first argument
/// is its receiver or a collection it changes, which stays the mod's.
const READS_FIRST: [&str; 20] = [
    "stream",
    "parallelStream",
    "forEach",
    "copyOf",
    "unmodifiableCollection",
    "unmodifiableList",
    "unmodifiableSet",
    "unmodifiableMap",
    "newArrayList",
    "newHashSet",
    "newLinkedHashSet",
    "newLinkedList",
    "newArrayDeque",
    "max",
    "min",
    "frequency",
    "enumeration",
    "nCopies",
    "getOrDefault",
    "containsAll",
];

/// `args` with each collection or map of a mod's own class replaced by one of the runtime's
/// holding what it holds now; None when there is none among them (nearly always).
fn own_as_native(rt: &mut Rt, name: &str, args: &[V]) -> Option<Vec<V>> {
    let from = usize::from(!READS_FIRST.contains(&name));
    let own = |rt: &Rt, v: V| {
        let cid = rt.cid(v);
        cid != 0
            && cid & crate::LAMBDA == 0
            && matches!(
                rt.heap.get(v.r() as usize).map(|o| &o.data),
                Some(Data::Fields(_))
            )
    };
    if READING.get() || !args.iter().skip(from).any(|&v| own(rt, v)) {
        return None;
    }
    let mut out = args.to_vec();
    let mut any = false;
    for slot in out.iter_mut().skip(from) {
        if !own(rt, *slot) {
            continue;
        }
        if rt.instance_of_static(*slot, "java/util/Collection") {
            let items = own_elements(rt, *slot);
            *slot = new_list(rt, items);
            any = true;
        } else if rt.instance_of_static(*slot, "java/util/Map") {
            let entries =
                rt.call_functional("java/util/Map.entrySet.()Ljava/util/Set;", *slot, &[]);
            let entries = if own(rt, entries) {
                own_elements(rt, entries)
            } else {
                elements(rt, entries)
            };
            let map = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
            for e in entries {
                let (k, v) = match rt.heap.get(e.r() as usize).map(|o| &o.data) {
                    Some(Data::List(kv)) if kv.len() == 2 => (kv[0], kv[1]),
                    _ => (
                        rt.call_functional(
                            "java/util/Map$Entry.getKey.()Ljava/lang/Object;",
                            e,
                            &[],
                        ),
                        rt.call_functional(
                            "java/util/Map$Entry.getValue.()Ljava/lang/Object;",
                            e,
                            &[],
                        ),
                    ),
                };
                table_put(rt, map, k, v);
            }
            *slot = map;
            any = true;
        }
    }
    any.then_some(out)
}

/// What a collection of a mod's own class holds, in its iterator's order: asked as any code asks
/// (`iterator()`, `hasNext()`, `next()`), up to a number no collection has.
pub(crate) fn own_elements(rt: &mut Rt, collection: V) -> Vec<V> {
    let was = READING.replace(true);
    let mut out = Vec::new();
    let it = rt.call_functional(
        "java/lang/Iterable.iterator.()Ljava/util/Iterator;",
        collection,
        &[],
    );
    while it.r() != 0 && rt.thrown.is_none() && out.len() < 1 << 26 {
        if rt
            .call_functional("java/util/Iterator.hasNext.()Z", it, &[])
            .i()
            == 0
            || rt.thrown.is_some()
        {
            break;
        }
        out.push(rt.call_functional("java/util/Iterator.next.()Ljava/lang/Object;", it, &[]));
    }
    READING.set(was);
    out
}

/// The elements of any collection-like value: a list, a set's members, an array, a stream.
pub fn elements(rt: &Rt, v: V) -> Vec<V> {
    match rt.heap.get(v.r() as usize).map(|o| (&o.data, o.class)) {
        Some((Data::List(l), _)) => l.clone(),
        Some((Data::Table(t), c)) if is_map(c) || is_map_object(rt, v) => {
            t.entries.iter().map(|(_, v)| *v).collect()
        }
        Some((Data::Table(t), _)) => t.entries.iter().map(|(k, _)| *k).collect(),
        Some((Data::Array(a), c)) if c.starts_with('[') => a.clone(),
        Some((Data::Iter(items, at), _)) => items[(*at).min(items.len())..].to_vec(),
        Some((Data::Stream(items), _)) => items.clone(),
        _ => Vec::new(),
    }
}

const SPLITTER: &str = "com/google/common/base/Splitter";
/// What `Stream.generate` gives: its supplier, until `limit` makes it a stream of so many.
const GENERATED_STREAM: &str = "java/util/stream/Stream$Generated";
const STREAM: &str = "java/util/stream/Stream";
const INT_STREAM: &str = "java/util/stream/IntStream";
const LONG_STREAM: &str = "java/util/stream/LongStream";
const DOUBLE_STREAM: &str = "java/util/stream/DoubleStream";
const STREAM_BUILDER: &str = "java/util/stream/Stream$Builder";
const LIST_ITERATOR: &str = "java/util/ListIterator";
const SIMPLE_ENTRY: &str = "java/util/AbstractMap$SimpleEntry";
const SIMPLE_IMMUTABLE_ENTRY: &str = "java/util/AbstractMap$SimpleImmutableEntry";

/// Whether `class` is a map entry's: `Map.entry`'s, a map's own, or `AbstractMap`'s simple ones.
pub(crate) fn is_entry(class: &str) -> bool {
    matches!(
        class,
        "java/util/Map$Entry" | SIMPLE_ENTRY | SIMPLE_IMMUTABLE_ENTRY
    )
}

/// What an iterable or an iterator goes through, from where it is: a collection's elements, an
/// iterator's remaining ones, a mod's own by its `iterator()`, `hasNext()` and `next()`.
fn drain(rt: &mut Rt, v: V) -> Vec<V> {
    if v.r() == 0 {
        return Vec::new();
    }
    if !matches!(
        rt.heap.get(v.r() as usize).map(|o| &o.data),
        Some(Data::Fields(_))
    ) {
        return elements(rt, v);
    }
    let it = if rt.class_overrides(rt.class_of(v), "hasNext.()Z") {
        v
    } else {
        rt.call_functional("java/lang/Iterable.iterator.()Ljava/util/Iterator;", v, &[])
    };
    let mut out = Vec::new();
    while it.r() != 0
        && rt.thrown.is_none()
        && rt
            .call_functional("java/util/Iterator.hasNext.()Z", it, &[])
            .i()
            != 0
        && rt.thrown.is_none()
    {
        out.push(rt.call_functional("java/util/Iterator.next.()Ljava/lang/Object;", it, &[]));
    }
    out
}

/// `Stream.builder()`'s builder: `add` (itself back) and `accept` keep an element, `build` makes
/// the stream of them.
fn stream_builder_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    Some(match name {
        "add" | "accept" => {
            let e = args.get(1).copied().unwrap_or(V::NULL);
            list_mut(rt, this)?.push(e);
            if name == "add" { this } else { V::Void }
        }
        "build" => {
            let items = list(rt, this)?.clone();
            rt.alloc(STREAM, Data::Stream(items))
        }
        _ => return None,
    })
}

/// A `ListIterator` over a list (`$list`), between elements at `$cursor`; `$last` is the index
/// `next` or `previous` gave last (-1 after `add` or `remove`), which `set` and `remove` act on.
fn list_iter_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let src = rt.get_field(this, "$list");
    let (cursor, last) = (
        rt.get_field(this, "$cursor").i(),
        rt.get_field(this, "$last").i(),
    );
    let len = list(rt, src).map_or(0, Vec::len) as i32;
    let at = |i: i32| usize::try_from(i).ok();
    Some(match name {
        "hasNext" => V::I((cursor < len) as i32),
        "hasPrevious" => V::I((cursor > 0) as i32),
        "nextIndex" => V::I(cursor),
        "previousIndex" => V::I(cursor - 1),
        "next" | "previous" => {
            let i = if name == "next" { cursor } else { cursor - 1 };
            let Some(&v) = at(i).and_then(|i| list(rt, src)?.get(i)) else {
                rt.throw_new("java/util/NoSuchElementException");
                return Some(V::NULL);
            };
            rt.put_field(
                this,
                "$cursor",
                V::I(if name == "next" { cursor + 1 } else { i }),
            );
            rt.put_field(this, "$last", V::I(i));
            v
        }
        "remove" | "set" if last < 0 => {
            rt.throw_new("java/lang/IllegalStateException");
            V::Void
        }
        "remove" => {
            if let (Some(l), Some(i)) = (list_mut(rt, src), at(last))
                && i < l.len()
            {
                l.remove(i);
            }
            if last < cursor {
                rt.put_field(this, "$cursor", V::I(cursor - 1));
            }
            rt.put_field(this, "$last", V::I(-1));
            V::Void
        }
        "set" => {
            if let (Some(l), Some(i)) = (list_mut(rt, src), at(last))
                && let Some(slot) = l.get_mut(i)
            {
                *slot = a(1);
            }
            V::Void
        }
        "add" => {
            if let (Some(l), Some(i)) = (list_mut(rt, src), at(cursor)) {
                l.insert(i.min(l.len()), a(1));
            }
            rt.put_field(this, "$cursor", V::I(cursor + 1));
            rt.put_field(this, "$last", V::I(-1));
            V::Void
        }
        "forEachRemaining" => {
            let rest: Vec<V> = list(rt, src)
                .map(|l| l.iter().skip(at(cursor).unwrap_or(0)).copied().collect())
                .unwrap_or_default();
            rt.put_field(this, "$cursor", V::I(len));
            if !rest.is_empty() {
                rt.put_field(this, "$last", V::I(len - 1));
            }
            for e in rest {
                rt.call_functional(ACCEPT, a(1), &[e]);
                if rt.thrown.is_some() {
                    break;
                }
            }
            V::Void
        }
        _ => return None,
    })
}
const JOINER: &str = "com/google/common/base/Joiner";
const OBJECT_ARRAYS: &str = "com/google/common/collect/ObjectArrays";

/// Guava's `Splitter` and `Joiner`, immutable as Guava's are (each setting gives a new one), and
/// `ObjectArrays.concat`.
fn guava_strings(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let with = |rt: &mut Rt, from: V, field: &'static str, v: V| {
        let class = if owner == SPLITTER { SPLITTER } else { JOINER };
        let o = rt.new_object(class);
        for f in ["$sep", "$trim", "$omit", "$skip", "$null"] {
            let old = rt.get_field(from, f);
            rt.put_field(o, f, old);
        }
        rt.put_field(o, field, v);
        o
    };
    Some(match (owner, name) {
        (SPLITTER | JOINER, "on") => {
            let sep = if desc.starts_with("(C)") { char::from_u32(a(0).i() as u32 & 0xffff).map(String::from).unwrap_or_default() } else { rt.to_java_string(a(0), 'L') };
            let sep = rt.string(&sep);
            with(rt, V::NULL, "$sep", sep)
        }
        (SPLITTER, "trimResults") => with(rt, a(0), "$trim", V::I(1)),
        (SPLITTER, "omitEmptyStrings") => with(rt, a(0), "$omit", V::I(1)),
        (SPLITTER, "split" | "splitToList") => {
            let this = a(0);
            let text = rt.to_java_string(a(1), 'L');
            let sep = rt.to_java_string(rt.get_field(this, "$sep"), 'L');
            let (trim, omit) = (rt.get_field(this, "$trim").i() != 0, rt.get_field(this, "$omit").i() != 0);
            let pieces: Vec<String> = if sep.is_empty() { text.chars().map(String::from).collect() } else { text.split(sep.as_str()).map(str::to_owned).collect() };
            let mut out = Vec::new();
            for p in pieces {
                let p = if trim { p.trim().to_owned() } else { p };
                if !(omit && p.is_empty()) {
                    out.push(rt.string(&p));
                }
            }
            new_list(rt, out)
        }
        (JOINER, "skipNulls") => with(rt, a(0), "$skip", V::I(1)),
        (JOINER, "useForNull") => with(rt, a(0), "$null", a(1)),
        (JOINER, "join") => {
            let this = a(0);
            let items = if desc.starts_with("(Ljava/lang/Object;Ljava/lang/Object;[") {
                let mut v = vec![a(1), a(2)];
                v.extend(elements(rt, a(3)));
                v
            } else {
                elements(rt, a(1))
            };
            let sep = rt.to_java_string(rt.get_field(this, "$sep"), 'L');
            let (skip, for_null) = (rt.get_field(this, "$skip").i() != 0, rt.get_field(this, "$null"));
            let mut parts = Vec::new();
            for v in items {
                match v {
                    V::R(0) | V::Void if skip => continue,
                    V::R(0) | V::Void if for_null.r() != 0 => parts.push(rt.to_java_string(for_null, 'L')),
                    V::R(0) | V::Void => {
                        rt.throw_new("java/lang/NullPointerException");
                        return Some(V::NULL);
                    }
                    v => parts.push(rt.to_java_string(v, 'L')),
                }
            }
            rt.string(&parts.join(&sep))
        }
        (OBJECT_ARRAYS, "concat") => {
            let items: Vec<V> = match desc {
                "([Ljava/lang/Object;Ljava/lang/Object;)[Ljava/lang/Object;" => elements(rt, a(0)).into_iter().chain([a(1)]).collect(),
                "(Ljava/lang/Object;[Ljava/lang/Object;)[Ljava/lang/Object;" => std::iter::once(a(0)).chain(elements(rt, a(1))).collect(),
                _ => elements(rt, a(0)).into_iter().chain(elements(rt, a(1))).collect(),
            };
            let arr = rt.new_array(items.len() as i32, V::NULL);
            for (i, v) in items.into_iter().enumerate() {
                rt.array_set(arr, i as i32, v);
            }
            arr
        }
        _ => return None,
    })
}

const GUAVA_OPTIONAL: &str = "com/google/common/base/Optional";

/// Guava's `Optional` (Create's `SeatBlock.getLeashed(level, player).or(player)`): absent or
/// present, never holding null, as Guava's `Absent` and `Present` are. `of`, `or` and `transform`
/// refuse null (`NullPointerException`); `get` on an absent one throws `IllegalStateException`.
fn guava_optional(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    const APPLY: &str =
        "com/google/common/base/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
    const SUPPLY: &str = "com/google/common/base/Supplier.get.()Ljava/lang/Object;";
    const NPE: &str = "java/lang/NullPointerException";
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let null = |v: V| matches!(v, V::R(0) | V::Void);
    let make = |rt: &mut Rt, v: V| -> V {
        if matches!(v, V::R(0) | V::Void) {
            return rt.intern("compat:guava-absent".into(), |rt| rt.new_object(GUAVA_OPTIONAL));
        }
        let o = rt.new_object(GUAVA_OPTIONAL);
        rt.put_field(o, "$value", v);
        o
    };
    // Guava's `checkNotNull` throws with no message; `or(null)` and a null result say why.
    let refuse = |rt: &mut Rt, msg: &str| {
        if msg.is_empty() {
            rt.throw_new(NPE);
            V::NULL
        } else {
            crate::jlang::throw_with(rt, NPE, msg)
        }
    };
    // The factories, and the conversions from and to `java.util.Optional` (null for null).
    match (name, desc) {
        ("absent", _) => return Some(make(rt, V::NULL)),
        ("of", _) if null(a(0)) => return Some(refuse(rt, "")),
        ("of" | "fromNullable", _) => return Some(make(rt, a(0))),
        ("fromJavaUtil", _) if null(a(0)) => return Some(V::NULL),
        ("fromJavaUtil", _) => {
            let v = rt.get_field(a(0), "value");
            return Some(make(rt, v));
        }
        ("toJavaUtil", "(Lcom/google/common/base/Optional;)Ljava/util/Optional;") if null(a(0)) => return Some(V::NULL),
        ("presentInstances", _) => {
            let items: Vec<V> = elements(rt, a(0)).into_iter().map(|o| rt.get_field(o, "$value")).filter(|&v| !null(v)).collect();
            return Some(new_list(rt, items));
        }
        _ => {}
    }
    let this = a(0);
    let v = rt.get_field(this, "$value");
    let present = !null(v);
    Some(match (name, desc) {
        ("isPresent", _) => V::I(i32::from(present)),
        ("get", _) if !present => crate::jlang::throw_with(rt, "java/lang/IllegalStateException", "Optional.get() cannot be called on an absent value"),
        ("get", _) => v,
        ("orNull", _) => if present { v } else { V::NULL },
        ("or", "(Ljava/lang/Object;)Ljava/lang/Object;") if null(a(1)) => refuse(rt, "use Optional.orNull() instead of Optional.or(null)"),
        ("or", "(Ljava/lang/Object;)Ljava/lang/Object;") => if present { v } else { a(1) },
        ("or", "(Lcom/google/common/base/Optional;)Lcom/google/common/base/Optional;") if null(a(1)) => refuse(rt, ""),
        ("or", "(Lcom/google/common/base/Optional;)Lcom/google/common/base/Optional;") => if present { this } else { a(1) },
        ("or", "(Lcom/google/common/base/Supplier;)Ljava/lang/Object;") if null(a(1)) => refuse(rt, ""),
        ("or", "(Lcom/google/common/base/Supplier;)Ljava/lang/Object;") if present => v,
        ("or", "(Lcom/google/common/base/Supplier;)Ljava/lang/Object;") => {
            let r = rt.call_functional(SUPPLY, a(1), &[]);
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            if null(r) {
                refuse(
                    rt,
                    "use Optional.orNull() instead of a Supplier that returns null",
                )
            } else {
                r
            }
        }
        ("transform", _) if null(a(1)) => refuse(rt, ""),
        ("transform", _) if !present => this,
        ("transform", _) => {
            let r = rt.call_functional(APPLY, a(1), &[v]);
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            if null(r) {
                refuse(
                    rt,
                    "the Function passed to Optional.transform() must not return null.",
                )
            } else {
                make(rt, r)
            }
        }
        ("toJavaUtil", _) => crate::api_common::optional(rt, if present { v } else { V::NULL }),
        ("asSet", _) => {
            let items: &[V] = if present {
                std::slice::from_ref(&v)
            } else {
                &[]
            };
            new_set_of(rt, items)
        }
        ("equals", _) => {
            let other = a(1);
            let w = if rt.class_of(other) == GUAVA_OPTIONAL {
                rt.get_field(other, "$value")
            } else {
                V::Void
            };
            let same = rt.class_of(other) == GUAVA_OPTIONAL
                && if present {
                    !null(w) && equals(rt, v, w)
                } else {
                    null(w)
                };
            V::I(i32::from(same))
        }
        ("hashCode", _) => V::I(if present {
            0x598d_f91c_i32.wrapping_add(hash(rt, v))
        } else {
            0x79a3_1aac
        }),
        ("toString", _) => {
            let text = if present {
                format!("Optional.of({})", rt.to_java_string(v, 'L'))
            } else {
                "Optional.absent()".to_owned()
            };
            rt.string(&text)
        }
        _ => return None,
    })
}

fn is_map(class: &str) -> bool {
    class.ends_with("Map") || class.ends_with("Map$Values")
}

/// Whether `v`, an object that holds a table, is a map (its entries are keys with their
/// values) and not a set: one of the JDK's and the libraries' own by its class's name, and a
/// mod's own class by what it extends. `new HashMap<>() {{ put(k, v); }}` makes an object of
/// an anonymous class of the mod's, which is that `HashMap` (taken for a set, by its name, it
/// had no `put`).
pub(crate) fn is_map_object(rt: &Rt, v: V) -> bool {
    is_map(rt.class_of(v)) || (rt.cid(v) != 0 && rt.instance_of_static(v, "java/util/Map"))
}

/// Appends `v` to the list at `l` (`List.add`); nothing for what isn't a list.
pub fn list_add(rt: &mut Rt, l: V, v: V) {
    if let Some(items) = list_mut(rt, l) {
        items.push(v);
    }
}

pub fn new_list(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc("java/util/ArrayList", Data::List(items))
}

/// A `LinkedHashSet` of `items`, in order.
pub fn new_set_of(rt: &mut Rt, items: &[V]) -> V {
    new_set(rt, items)
}

/// A `LinkedHashSet` of distinct objects (a map's entries, FML's scan results): identity keys,
/// no `hashCode` or `equals` callbacks.
pub(crate) fn identity_set(rt: &mut Rt, items: &[V]) -> V {
    let s = rt.alloc("java/util/LinkedHashSet", Data::Table(Table::default()));
    if let Some(Data::Table(t)) = rt.heap.get_mut(s.r() as usize).map(|o| &mut o.data) {
        for (i, e) in items.iter().enumerate() {
            t.entries.push((*e, V::I(1)));
            t.index.entry(e.r() as i32).or_default().push(i);
        }
    }
    s
}

fn new_set(rt: &mut Rt, items: &[V]) -> V {
    let s = rt.alloc("java/util/LinkedHashSet", Data::Table(Table::default()));
    for &i in items {
        table_put(rt, s, i, V::I(1));
    }
    s
}

pub fn new_iter(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc("java/util/Iterator", Data::Iter(items, 0))
}

pub(crate) fn entry(rt: &mut Rt, k: V, v: V) -> V {
    rt.alloc("java/util/Map$Entry", Data::List(vec![k, v]))
}

fn boolean(rt: &mut Rt, b: bool) -> V {
    let _ = rt;
    V::I(b as i32)
}

fn int_box(rt: &mut Rt, i: i32) -> V {
    let o = rt.new_object("java/lang/Integer");
    rt.put_field(o, "value", V::I(i));
    o
}

/// The kind a constructor of `class` makes, or None for classes this module doesn't model.
fn kind_of(class: &str) -> Option<&'static str> {
    let simple = class.rsplit('/').next().unwrap_or(class);
    Some(match simple {
        "ArrayList" | "LinkedList" | "ArrayDeque" | "Vector" | "Stack" | "CopyOnWriteArrayList"
        | "ConcurrentLinkedQueue" | "ConcurrentLinkedDeque" | "LinkedBlockingQueue" | "LinkedBlockingDeque" => "list",
        // fastutil's lists and queues of each element type (`ReferenceArrayList`, `LongArrayList`,
        // `IntArrayFIFOQueue`, `IntImmutableList`...).
        s if class.starts_with("it/unimi/dsi/fastutil/")
            && (s.ends_with("ArrayList")
                || s.ends_with("FIFOQueue")
                || s.ends_with("BigArrayBigList")
                || s.ends_with("ImmutableList")) =>
        {
            "list"
        }
        "TreeMap" | "TreeSet" | "ConcurrentSkipListMap" | "ConcurrentSkipListSet" => "sorted",
        s if s.ends_with("Map") || s.ends_with("Set") => "table",
        // `Hashtable` and its `Properties`: a map as any other here (one thread).
        "Hashtable" | "Properties" => "table",
        _ => return None,
    })
}

fn util_owner(owner: &str) -> bool {
    owner.starts_with("java/util/")
        || owner == NON_NULL_LIST
        || owner == "java/lang/Iterable"
        || owner.starts_with("it/unimi/dsi/fastutil/")
        || owner.starts_with("com/google/common/collect/")
        // Gson's own map, which a mod's config library keeps its values in: a map in the order
        // its keys were put, as any here.
        || owner == "com/google/gson/internal/LinkedTreeMap"
}

/// What `Properties` has past a map's own methods: its texts by key, and its file format (a
/// `key=value` a line, `#` and `!` comments, `\` escapes and line continuations), read and
/// written as `load` and `store` do.
fn properties(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
        "getProperty" => match table_get(rt, this, a(1)) {
            Some(v) if rt.str_of(v).is_some() => v,
            _ if args.len() > 2 => a(2),
            _ => V::NULL,
        },
        "setProperty" => table_put(rt, this, a(1), a(2)),
        "stringPropertyNames" => {
            let keys: Vec<V> = table_pairs(rt, this)
                .into_iter()
                .filter(|(k, v)| rt.str_of(*k).is_some() && rt.str_of(*v).is_some())
                .map(|(k, _)| k)
                .collect();
            new_set_of(rt, &keys)
        }
        "propertyNames" => {
            let keys = table_keys(rt, this);
            new_iter(rt, keys)
        }
        "load" => {
            let text = crate::resources::take_text(rt, a(1)).unwrap_or_default();
            for (k, v) in parse_properties(&text) {
                let (k, v) = (rt.string(&k), rt.string(&v));
                table_put(rt, this, k, v);
            }
            V::Void
        }
        "store" => {
            let mut out = String::new();
            if let Some(comments) = rt.str_of(a(2)) {
                for line in comments.lines() {
                    out.push_str(&format!("#{line}\n"));
                }
            }
            for (k, v) in table_pairs(rt, this) {
                let (k, v) = (rt.to_java_string(k, 'L'), rt.to_java_string(v, 'L'));
                out.push_str(&format!(
                    "{}={}\n",
                    escape_property(&k, true),
                    escape_property(&v, false)
                ));
            }
            if desc.starts_with("(Ljava/io/Writer;") {
                let s = rt.string(&out);
                crate::api::call(rt, "java/io/Writer.write.(Ljava/lang/String;)V", &[a(1), s]);
                crate::api::call(rt, "java/io/Writer.flush.()V", &[a(1)]);
            } else {
                let bytes = crate::files::byte_array(rt, out.as_bytes());
                crate::api::call(rt, "java/io/OutputStream.write.([B)V", &[a(1), bytes]);
                crate::api::call(rt, "java/io/OutputStream.flush.()V", &[a(1)]);
            }
            V::Void
        }
        _ => return None,
    })
}

/// A key or a value as `Properties.store` writes it: the characters that would end it or start a
/// comment escaped, a key's spaces too and a value's leading one, anything past ASCII as `\uXXXX`.
fn escape_property(text: &str, key: bool) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        match c {
            ' ' if key || i == 0 => out.push_str("\\ "),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04X}"));
                }
            }
        }
    }
    out
}

/// The keys and values of a `.properties` text, as `Properties.load` reads it.
fn parse_properties(text: &str) -> Vec<(String, String)> {
    // Logical lines: a line ending in an odd number of backslashes goes on in the next, less its
    // leading blanks.
    let mut lines: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for raw in text.split(['\n', '\r']) {
        let line = raw.trim_start_matches([' ', '\t', '\u{c}']);
        let mut whole = match pending.take() {
            Some(p) => p + line,
            None if line.is_empty() || line.starts_with(['#', '!']) => continue,
            None => line.to_owned(),
        };
        let slashes = whole.len() - whole.trim_end_matches('\\').len();
        if slashes % 2 == 1 {
            whole.pop();
            pending = Some(whole);
        } else {
            lines.push(whole);
        }
    }
    lines.extend(pending);
    let unescape = |s: &str| {
        let (mut out, mut chars) = (String::new(), s.chars());
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('f') => out.push('\u{c}'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    };
    lines
        .iter()
        .map(|line| {
            // The key ends at the first `=`, `:` or blank that no backslash escapes.
            let (mut end, mut escaped) = (line.len(), false);
            for (i, c) in line.char_indices() {
                if !escaped && matches!(c, '=' | ':' | ' ' | '\t' | '\u{c}') {
                    end = i;
                    break;
                }
                escaped = c == '\\' && !escaped;
            }
            let rest = line[end..].trim_start_matches([' ', '\t', '\u{c}']);
            let rest = rest
                .strip_prefix(['=', ':'])
                .unwrap_or(rest)
                .trim_start_matches([' ', '\t', '\u{c}']);
            (unescape(&line[..end]), unescape(rest))
        })
        .collect()
}

/// `StringJoiner`: the pieces added so far, joined, between its prefix and suffix.
fn joiner(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let text = |rt: &Rt, field: &'static str| {
        rt.str_of(rt.get_field(this, field))
            .unwrap_or("")
            .to_owned()
    };
    let put = |rt: &mut Rt, field: &'static str, s: &str| {
        let v = rt.string(s);
        rt.put_field(this, field, v);
    };
    Some(match name {
        "<init>" => {
            let parts: Vec<String> = (1..args.len())
                .map(|i| crate::jlang::text(rt, a(i)))
                .collect();
            put(rt, "$separator", &parts[0]);
            if desc.starts_with(
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;Ljava/lang/CharSequence;)",
            ) {
                put(rt, "$prefix", &parts[1]);
                put(rt, "$suffix", &parts[2]);
            }
            V::Void
        }
        "add" => {
            let piece = crate::jlang::text(rt, a(1));
            let joined = if rt.get_field(this, "$count").i() == 0 {
                piece
            } else {
                text(rt, "$joined") + &text(rt, "$separator") + &piece
            };
            put(rt, "$joined", &joined);
            let count = rt.get_field(this, "$count").i() + 1;
            rt.put_field(this, "$count", V::I(count));
            this
        }
        "setEmptyValue" => {
            rt.put_field(this, "$empty", a(1));
            this
        }
        "toString" | "length" => {
            let empty = rt.get_field(this, "$empty");
            let s = if rt.get_field(this, "$count").i() == 0 && rt.str_of(empty).is_some() {
                rt.str_of(empty).unwrap_or("").to_owned()
            } else {
                text(rt, "$prefix") + &text(rt, "$joined") + &text(rt, "$suffix")
            };
            if name == "length" {
                V::I(s.encode_utf16().count() as i32)
            } else {
                rt.string(&s)
            }
        }
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    util_owner(owner)
        || owner.starts_with("com/google/common/")
        || matches!(owner, "org/apache/commons/lang3/ArrayUtils" | SECURE_RANDOM)
}

/// A `SecureRandom` is a `Random` here: its numbers are no secret a mod of the game keeps.
const SECURE_RANDOM: &str = "java/security/SecureRandom";

/// A `java.util` call (`owner.name.desc`), or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        "java/util/Calendar" | "java/util/GregorianCalendar" | "java/util/Date" => {
            return crate::jsystem::calendar(rt, owner, name, desc, args);
        }
        "java/util/StringJoiner" => return joiner(rt, name, desc, args),
        // An `Enumeration` here is an iterator under its older names.
        "java/util/Enumeration" => {
            let key = match name {
                "hasMoreElements" => "java/util/Iterator.hasNext.()Z",
                "nextElement" => "java/util/Iterator.next.()Ljava/lang/Object;",
                "asIterator" => return Some(a(0)),
                _ => return None,
            };
            return Some(crate::api::call(rt, key, args));
        }
        "java/util/Properties"
            if matches!(
                name,
                "getProperty"
                    | "setProperty"
                    | "load"
                    | "store"
                    | "stringPropertyNames"
                    | "propertyNames"
            ) =>
        {
            return properties(rt, name, desc, args);
        }
        _ => {}
    }
    // Statics first.
    if let Some(v) = statics(rt, owner, name, desc, args) {
        return Some(v);
    }
    match rt.class_of(a(0)) {
        TABLE_CELL => {
            return Some(match name {
                "getRowKey" => rt.get_field(a(0), "$row"),
                "getColumnKey" => rt.get_field(a(0), "$column"),
                "getValue" => rt.get_field(a(0), "$value"),
                _ => return None,
            });
        }
        LIST_ITERATOR => return list_iter_call(rt, name, args),
        STREAM_BUILDER => return stream_builder_call(rt, name, args),
        _ => {}
    }
    if rt.class_of(a(0)) == GENERATED_STREAM {
        if name != "limit" {
            return None;
        }
        let supplier = rt.get_field(a(0), "$supplier");
        let mut items = Vec::new();
        for _ in 0..a(1).j().max(0) {
            items.push(rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", supplier, &[]));
            if rt.thrown.is_some() {
                break;
            }
        }
        return Some(rt.alloc("java/util/stream/Stream", Data::Stream(items)));
    }
    // fastutil's `trim()`: the table's storage, which a runtime table sizes itself.
    if owner.starts_with("it/unimi/dsi/fastutil/") && name == "trim" {
        return Some(V::I(1));
    }
    if owner == "java/util/Comparator"
        && let Some(v) = comparator_call(rt, name, desc, args)
    {
        return Some(v);
    }
    if owner == "java/util/BitSet" {
        return bitset_call(rt, name, desc, args);
    }
    if matches!(owner, "java/util/concurrent/CompletableFuture" | "java/util/concurrent/Future" | "java/util/concurrent/CompletionStage") {
        return future_call(rt, name, args);
    }
    if owner.starts_with("com/google/common/cache/") {
        return cache_call(rt, owner, name, args);
    }
    if matches!(
        owner,
        "java/util/Random" | "java/util/concurrent/ThreadLocalRandom" | SECURE_RANDOM
    ) {
        return random_call(rt, name, desc, args);
    }
    // Guava's immutable collection builders: filled in place, `build` copies.
    if owner.starts_with("com/google/common/collect/Immutable") && owner.ends_with("$Builder") {
        return immutable_builder_call(rt, owner, name, desc, args);
    }
    // Guava's `MapMaker`: weak or concurrent settings make no difference to one thread's map.
    if owner == "com/google/common/collect/MapMaker" {
        return Some(match name {
            "<init>" => V::Void,
            "makeMap" => rt.alloc("java/util/concurrent/ConcurrentHashMap", Data::Table(Table::default())),
            _ => a(0),
        });
    }
    if matches!(owner, SPLITTER | JOINER | OBJECT_ARRAYS) {
        return guava_strings(rt, owner, name, desc, args);
    }
    if owner == GUAVA_OPTIONAL {
        return guava_optional(rt, name, desc, args);
    }
    // Vanilla's strategies for stacks (`ItemStackLinkedSet.TYPE_AND_TAG`); a mod's own runs its code.
    if owner == STRATEGY {
        return crate::api_common::stack_strategy(rt, name, args);
    }
    if !util_owner(owner) {
        return None;
    }
    // A collection or a map of a mod's own class among the arguments is read into one of the
    // runtime's first ([`own_as_native`]): what the runtime's collections take from another
    // (`new ArrayList<>(c)`, `addAll`, `equals`), they then take from that.
    let read = own_as_native(rt, name, args);
    let args: &[V] = read.as_deref().unwrap_or(args);
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if name == "<init>" {
        let this = a(0);
        if crate::heaps::heap_class(owner) {
            crate::heaps::construct(rt, owner, desc, args);
            return Some(V::Void);
        }
        let kind = kind_of(owner)?;
        let class = rt.class_of(this);
        let data = match kind {
            "list" => Data::List(Vec::new()),
            _ => Data::Table(Table { sorted: kind == "sorted", ..Table::default() }),
        };
        if let Some(o) = rt.heap.get_mut(this.r() as usize) {
            // A mod's subclass keeps the fields it already set beside the collection it now is.
            if let Data::Fields(f) = std::mem::replace(&mut o.data, data)
                && !f.is_empty()
            {
                rt.side_fields.entry(this.r()).or_default().extend(f);
            }
            o.class = class;
        }
        // A custom-hash one's strategy, its last argument: set before what it copies goes in.
        if desc.ends_with("Lit/unimi/dsi/fastutil/Hash$Strategy;)V")
            && let Some(&s) = args.last()
            && let Some(Data::Table(t)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data)
        {
            t.strategy = s;
        }
        // From an array, whole or a stretch of it (`new IntImmutableList(values)`,
        // `new IntArrayList(values, offset, length)`).
        if let Some(&arg) = args.get(1)
            && arg.r() != 0
            && desc.starts_with("([")
        {
            let mut items = elements(rt, arg);
            if desc.ends_with("II)V") {
                items = items
                    .into_iter()
                    .skip(a(2).i().max(0) as usize)
                    .take(a(3).i().max(0) as usize)
                    .collect();
            }
            match list_mut(rt, this) {
                Some(l) => l.extend(items),
                None => {
                    for e in items {
                        table_put(rt, this, e, V::I(1));
                    }
                }
            }
        }
        // A copy constructor, or a sorted collection's comparator.
        if let Some(&arg) = args.get(1)
            && arg.r() != 0
            && desc.starts_with("(L")
        {
            let src_is_map = is_map_object(rt, arg)
                && matches!(
                    rt.heap.get(arg.r() as usize).map(|o| &o.data),
                    Some(Data::Table(_))
                );
            if desc.contains("Comparator") {
                if let Some(Data::Table(t)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                    t.comparator = arg;
                }
            } else if kind == "list" {
                let items = elements(rt, arg);
                if let Some(l) = list_mut(rt, this) {
                    l.extend(items);
                }
            } else if src_is_map && is_map(owner) {
                for (k, v) in table_entries(rt, arg) {
                    table_put(rt, this, k, v);
                }
            } else if !desc.starts_with("(Ljava/lang/Class;") {
                for e in elements(rt, arg) {
                    table_put(rt, this, e, V::I(1));
                }
            }
        }
        return Some(V::Void);
    }
    let this = a(0);
    let data_kind = rt.heap.get(this.r() as usize).map(|o| match &o.data {
        Data::List(_) => 'l',
        Data::Table(_) => 't',
        Data::Iter(..) => 'i',
        Data::Stream(_) => 's',
        _ => '?',
    });
    let class = rt.class_of(this);
    if data_kind == Some('t') && class.starts_with("com/google/common/collect/") {
        if class.ends_with("Multimap") {
            return multimap_call(rt, name, args);
        }
        if class.ends_with("Table") {
            return guava_table_call(rt, name, args);
        }
    }
    match data_kind? {
        'l' if is_entry(rt.class_of(this)) => entry_call(rt, name, args),
        // A priority queue moves its elements as a heap; the rest it shares with a list.
        'l' if crate::heaps::is_heap(rt, this) => {
            crate::heaps::call(rt, name, desc, args).or_else(|| list_call(rt, name, desc, args))
        }
        'l' => list_call(rt, name, desc, args),
        // The baked models' registry puts the game's model at a location it lacks when asked
        // for one there, as vanilla's has one at each.
        't' if class == crate::baked_models::REGISTRY
            && matches!(name, "get" | "getOrDefault" | "containsKey") =>
        {
            crate::baked_models::at(rt, this, a(1));
            map_call(rt, name, desc, args)
        }
        't' if is_map_object(rt, this) => map_call(rt, name, desc, args),
        't' => set_call(rt, name, args),
        'i' => iter_call(rt, name, args),
        's' => stream_call(rt, name, args),
        _ => None,
    }
}

/// A done `CompletableFuture` holding `value`: work runs at once here (one thread), so every
/// future is complete when made.
pub fn completed_future(rt: &mut Rt, value: V) -> V {
    let f = rt.new_object("java/util/concurrent/CompletableFuture");
    rt.put_field(f, "$value", value);
    f
}

const CALL: &str = "java/util/concurrent/Callable.call.()Ljava/lang/Object;";
const LOAD: &str = "com/google/common/cache/CacheLoader.load.(Ljava/lang/Object;)Ljava/lang/Object;";
const CACHE: &str = "com/google/common/cache/LocalCache$LocalLoadingCache";

/// Guava's caches (`CacheBuilder`, `Cache`, `LoadingCache`): a cache is an object holding a real map
/// (`$map`, what `asMap` gives) and its loader (`$loader`, null for a plain `Cache`). Nothing expires
/// and nothing is evicted, so removal listeners never run: what a mod caches stays until it
/// invalidates it, which is what its own code expects of a cache that stays under its limits.
fn cache_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let map = |rt: &mut Rt, cache: V| -> V {
        let m = rt.get_field(cache, "$map");
        if m.r() != 0 {
            return m;
        }
        let m = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
        rt.put_field(cache, "$map", m);
        m
    };
    let new_cache = |rt: &mut Rt, loader: V| -> V {
        let c = rt.new_object(CACHE);
        let m = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
        rt.put_field(c, "$map", m);
        rt.put_field(c, "$loader", loader);
        c
    };
    Some(match (owner, name) {
        // The builder: settings are ignored (see above); `build` makes the cache.
        ("com/google/common/cache/CacheBuilder", "newBuilder" | "from") => rt.new_object("com/google/common/cache/CacheBuilder"),
        ("com/google/common/cache/CacheBuilder", "build") => new_cache(rt, if args.len() > 1 { a(1) } else { V::NULL }),
        ("com/google/common/cache/CacheBuilder", "<init>") => V::Void,
        ("com/google/common/cache/CacheBuilder", _) => a(0),
        // `CacheLoader`: a mod's subclass overrides `load`; `from` wraps a function.
        ("com/google/common/cache/CacheLoader", "<init>") => V::Void,
        ("com/google/common/cache/CacheLoader", "from") => {
            let l = rt.new_object("com/google/common/cache/CacheLoader$FunctionToCacheLoader");
            rt.put_field(l, "$function", a(0));
            l
        }
        ("com/google/common/cache/CacheLoader$FunctionToCacheLoader", "load") => {
            let f = rt.get_field(a(0), "$function");
            rt.call_functional(APPLY, f, &[a(1)])
        }
        (_, "getIfPresent") => {
            let m = map(rt, a(0));
            table_get(rt, m, a(1)).unwrap_or(V::NULL)
        }
        // `Cache.get(key, Callable)` and `LoadingCache.get(key)`: the cached value, else the loader's,
        // kept. A null loaded value is Guava's `InvalidCacheLoadException`; here it is just not kept.
        (_, "get" | "getUnchecked" | "apply") => {
            let (cache, key) = (a(0), a(1));
            let m = map(rt, cache);
            if let Some(v) = table_get(rt, m, key) {
                return Some(v);
            }
            let v = if args.len() > 2 {
                rt.call_functional(CALL, a(2), &[])
            } else {
                let loader = rt.get_field(cache, "$loader");
                if loader.r() == 0 {
                    return Some(V::NULL);
                }
                match rt.vcall {
                    Some(vc) => vc(rt, LOAD, "load.(Ljava/lang/Object;)Ljava/lang/Object;", &[loader, key]),
                    None => V::NULL,
                }
            };
            if rt.thrown.is_none() && v.r() != 0 {
                table_put(rt, m, key, v);
            }
            v
        }
        (_, "put") => {
            let m = map(rt, a(0));
            table_put(rt, m, a(1), a(2));
            V::Void
        }
        (_, "putAll") => {
            let m = map(rt, a(0));
            for (k, v) in table_entries(rt, a(1)) {
                table_put(rt, m, k, v);
            }
            V::Void
        }
        (_, "invalidate" | "refresh") => {
            let m = map(rt, a(0));
            table_remove(rt, m, a(1));
            V::Void
        }
        (_, "invalidateAll") if args.len() > 1 => {
            let m = map(rt, a(0));
            let keys = elements(rt, a(1));
            for k in keys {
                table_remove(rt, m, k);
            }
            V::Void
        }
        (_, "invalidateAll") => {
            let m = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
            rt.put_field(a(0), "$map", m);
            V::Void
        }
        (_, "asMap") => map(rt, a(0)),
        (_, "size") => {
            let m = map(rt, a(0));
            V::J(table_len(rt, m) as i64)
        }
        (_, "cleanUp") => V::Void,
        _ => return None,
    })
}

const RUN: &str = "java/lang/Runnable.run.()V";
const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const ACCEPT: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const BI_APPLY: &str = "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const BI_ACCEPT: &str = "java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V";

/// `CompletableFuture` and its stages, run at once (nothing ever fails asynchronously).
fn future_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let value = |rt: &Rt| rt.get_field(a(0), "$value");
    Some(match name {
        "<init>" => V::Void,
        "completedFuture" | "completedStage" => completed_future(rt, a(0)),
        "supplyAsync" => {
            let v = rt.call_functional(SUPPLY, a(0), &[]);
            completed_future(rt, v)
        }
        "runAsync" => {
            rt.call_functional(RUN, a(0), &[]);
            completed_future(rt, V::NULL)
        }
        "allOf" | "anyOf" => completed_future(rt, V::NULL),
        "join" | "get" | "getNow" | "resultNow" => value(rt),
        "isDone" => V::I(1),
        "isCancelled" | "isCompletedExceptionally" | "cancel" => V::I(0),
        "complete" => {
            rt.put_field(a(0), "$value", a(1));
            V::I(1)
        }
        "thenRun" | "thenRunAsync" => {
            rt.call_functional(RUN, a(1), &[]);
            completed_future(rt, V::NULL)
        }
        "thenAccept" | "thenAcceptAsync" => {
            let v = value(rt);
            rt.call_functional(ACCEPT, a(1), &[v]);
            completed_future(rt, V::NULL)
        }
        "thenApply" | "thenApplyAsync" => {
            let v = value(rt);
            let r = rt.call_functional(APPLY, a(1), &[v]);
            completed_future(rt, r)
        }
        "thenCompose" | "thenComposeAsync" => {
            let v = value(rt);
            rt.call_functional(APPLY, a(1), &[v])
        }
        "handle" | "handleAsync" => {
            let v = value(rt);
            let r = rt.call_functional(BI_APPLY, a(1), &[v, V::NULL]);
            completed_future(rt, r)
        }
        "whenComplete" | "whenCompleteAsync" => {
            let v = value(rt);
            rt.call_functional(BI_ACCEPT, a(1), &[v, V::NULL]);
            a(0)
        }
        "exceptionally" | "toCompletableFuture" | "orTimeout" => a(0),
        _ => return None,
    })
}

fn statics(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if crate::collectors::makes(owner, name)
        && let Some(v) = crate::collectors::make(rt, owner, name, desc, args)
    {
        return Some(v);
    }
    if owner == "java/util/Arrays"
        && let Some(v) = arrays_call(rt, name, desc, args)
    {
        return Some(v);
    }
    // The elements a variadic factory was given: `of(E...)` is one array; Guava's
    // `ImmutableList.of(e1, ..., e12, E... others)` is the fixed ones, then the array's.
    let varargs = |rt: &Rt| -> Vec<V> {
        if desc.starts_with("([") {
            return elements(rt, a(0));
        }
        let params = desc.split(')').next().unwrap_or("");
        match args.split_last() {
            Some((&last, fixed)) if params.ends_with("[Ljava/lang/Object;") => {
                let mut v = fixed.to_vec();
                v.extend(elements(rt, last));
                v
            }
            _ => args.to_vec(),
        }
    };
    if owner == "java/util/EnumSet"
        && let Some(v) = enum_set_call(rt, name, desc, args)
    {
        return Some(v);
    }
    Some(match (owner, name) {
        // Minecraft's `NonNullList`: a list (`withSize` holds its default `size` times).
        (NON_NULL_LIST, "create" | "createWithCapacity") => rt.alloc(NON_NULL_LIST, Data::List(Vec::new())),
        (NON_NULL_LIST, "withSize") => rt.alloc(NON_NULL_LIST, Data::List(vec![a(1); a(0).i().max(0) as usize])),
        (NON_NULL_LIST, "of") => {
            let items = elements(rt, a(1));
            rt.alloc(NON_NULL_LIST, Data::List(items))
        }
        (NON_NULL_LIST, "copyOf") => {
            let items = elements(rt, a(0));
            rt.alloc(NON_NULL_LIST, Data::List(items))
        }
        ("java/util/List" | "java/util/Arrays" | "com/google/common/collect/ImmutableList" | "com/google/common/collect/Lists", "of" | "asList" | "copyOf" | "newArrayList")
            if !desc.contains(")V") =>
        {
            let items = if name == "copyOf" || (name == "newArrayList" && desc.starts_with("(Ljava/lang/Iterable")) { elements(rt, a(0)) } else { varargs(rt) };
            new_list(rt, items)
        }
        // fastutil's typed lists and sets of the values given (`IntList.of(1, 2, 3)`,
        // `IntSet.of(ids)`): each value as it is, or the elements of the one array.
        (o, "of")
            if o.starts_with("it/unimi/dsi/fastutil/")
                && !o.contains('2')
                && (o.ends_with("List") || o.ends_with("Set"))
                && !desc.contains(")V") =>
        {
            let items = if desc.starts_with("([") {
                elements(rt, a(0))
            } else {
                args.to_vec()
            };
            if o.ends_with("Set") {
                new_set(rt, &items)
            } else {
                new_list(rt, items)
            }
        }
        ("java/util/Set" | "com/google/common/collect/ImmutableSet" | "com/google/common/collect/Sets", "of" | "copyOf" | "newHashSet") => {
            let items = if name == "copyOf" || desc.starts_with("(Ljava/lang/Iterable") { elements(rt, a(0)) } else { varargs(rt) };
            new_set(rt, &items)
        }
        // Guava's set views (`Sets.union(a, b)`): a copy in the view's order, the first set's
        // elements before the second's.
        ("com/google/common/collect/Sets", "union" | "intersection" | "difference" | "symmetricDifference") => {
            let (x, y) = (elements(rt, a(0)), elements(rt, a(1)));
            // Whether each of `from` is in `of`.
            let within = |rt: &mut Rt, from: &[V], of: &[V]| -> Vec<bool> { from.iter().map(|&e| of.iter().any(|&o| equals(rt, o, e))).collect() };
            let (x_in_y, y_in_x) = (within(rt, &x, &y), within(rt, &y, &x));
            let pick = |from: &[V], is_in: &[bool], keep: bool| -> Vec<V> { from.iter().zip(is_in).filter(|(_, i)| **i == keep).map(|(e, _)| *e).collect() };
            let items = match name {
                // A set keeps the first of equal elements: the first set's, then the second's new ones.
                "union" => [x.clone(), y.clone()].concat(),
                "intersection" => pick(&x, &x_in_y, true),
                "difference" => pick(&x, &x_in_y, false),
                _ => [pick(&x, &x_in_y, false), pick(&y, &y_in_x, false)].concat(),
            };
            new_set(rt, &items)
        }
        // Guava's `Lists.reverse(list)` and `Lists.transform(list, function)`: copies, the elements
        // turned around or each through the function (Guava's are live views of the list; the code
        // asking reads them once: Create's clone command walks its blocks back so).
        ("com/google/common/collect/Lists", "reverse") => {
            let mut items = elements(rt, a(0));
            items.reverse();
            new_list(rt, items)
        }
        ("com/google/common/collect/Lists", "transform") => {
            let mut out = Vec::new();
            for e in elements(rt, a(0)) {
                out.push(rt.call_functional("com/google/common/base/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(1), &[e]));
                if rt.thrown.is_some() {
                    break;
                }
            }
            new_list(rt, out)
        }
        // Guava's other factories (`Sets.newLinkedHashSet()`, `Lists.newArrayListWithCapacity(n)`,
        // `Maps.newHashMap(map)`...): empty for a size hint, else a copy of what they're given (an
        // `Iterable`, an `Iterator`, an array, a map). The runtime's sets and maps keep insertion
        // order, as the linked ones do.
        // `Queues.newConcurrentLinkedQueue()` and its kin: the queue class the factory names.
        ("com/google/common/collect/Queues", n)
            if n.starts_with("new")
                && desc
                    .rsplit_once(")L")
                    .is_some_and(|(_, c)| kind_of(c.trim_end_matches(';')) == Some("list")) =>
        {
            let items = if desc.starts_with("(Ljava/lang/Iterable;)") {
                elements(rt, a(0))
            } else {
                Vec::new()
            };
            let class = rt.static_key(
                desc.rsplit_once(")L")
                    .map_or("java/util/ArrayDeque", |(_, c)| c.trim_end_matches(';')),
            );
            rt.alloc(class, Data::List(items))
        }
        ("com/google/common/collect/Sets" | "com/google/common/collect/Lists" | "com/google/common/collect/Maps", n)
            if n.starts_with("new") =>
        {
            let given = a(0);
            let items = if desc.starts_with("()") || desc.starts_with("(I)") || desc.starts_with("(Ljava/lang/Class;)") || desc.starts_with("(Ljava/util/Comparator;)") {
                Vec::new()
            } else if desc.starts_with("(Ljava/util/Map;)") || desc.starts_with("(Ljava/util/SortedMap;)") {
                table_pairs(rt, given).into_iter().flat_map(|(k, v)| [k, v]).collect()
            } else if desc.starts_with("([") {
                varargs(rt)
            } else {
                elements(rt, given)
            };
            match owner {
                "com/google/common/collect/Lists" => new_list(rt, items),
                "com/google/common/collect/Sets" => new_set(rt, &items),
                _ => {
                    let sorted = n.contains("Tree");
                    let m = rt.alloc(if sorted { "java/util/TreeMap" } else { "java/util/LinkedHashMap" }, Data::Table(Table { sorted, ..Table::default() }));
                    for kv in items.chunks(2) {
                        if let [k, v] = kv {
                            table_put(rt, m, *k, *v);
                        }
                    }
                    m
                }
            }
        }
        // fastutil's `ObjectOpenHashSet.of(a, b, c)` and its kin: a set of them.
        (o, "of") if o.starts_with("it/unimi/dsi/fastutil/") && o.ends_with("Set") => {
            let items = varargs(rt);
            new_set(rt, &items)
        }
        // fastutil's empty maps (`Object2IntSortedMaps.emptyMap()`).
        (o, "emptyMap") if o.starts_with("it/unimi/dsi/fastutil/") && desc.starts_with("()") => rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default())),
        // `Map.ofEntries(entries...)`: their keys and values in order; Guava's
        // `Maps.immutableEnumMap(map)` a copy of the map.
        ("java/util/Map", "ofEntries") | ("com/google/common/collect/Maps", "immutableEnumMap") => {
            let pairs: Vec<(V, V)> = if name == "ofEntries" {
                elements(rt, a(0))
                    .into_iter()
                    .map(|e| {
                        let kv = elements(rt, e);
                        (kv.first().copied().unwrap_or(V::NULL), kv.get(1).copied().unwrap_or(V::NULL))
                    })
                    .collect()
            } else {
                table_pairs(rt, a(0))
            };
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
            for (k, v) in pairs {
                table_put(rt, m, k, v);
            }
            m
        }
        ("java/util/Map" | "com/google/common/collect/ImmutableMap" | "com/google/common/collect/Maps", "of" | "newHashMap" | "newLinkedHashMap" | "newConcurrentMap") => {
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
            for kv in args.chunks(2) {
                if let [k, v] = kv {
                    table_put(rt, m, *k, *v);
                }
            }
            m
        }
        // `ImmutableMap.copyOf(map)` (or of entries) and `Map.copyOf`: a map of its own, in the
        // source's order; a sorted one's keys in their natural order.
        (
            "java/util/Map" | "com/google/common/collect/ImmutableMap" | "com/google/common/collect/ImmutableSortedMap" | "com/google/common/collect/ImmutableBiMap",
            "copyOf",
        ) => {
            let sorted = owner.ends_with("SortedMap");
            let class = if sorted { "java/util/TreeMap" } else { "java/util/LinkedHashMap" };
            let m = rt.alloc(class, Data::Table(Table { sorted, ..Table::default() }));
            let pairs: Vec<(V, V)> = if desc.starts_with("(Ljava/lang/Iterable;") {
                elements(rt, a(0))
                    .into_iter()
                    .map(|e| {
                        let kv = elements(rt, e);
                        (kv.first().copied().unwrap_or_default(), kv.get(1).copied().unwrap_or_default())
                    })
                    .collect()
            } else {
                table_entries(rt, a(0))
            };
            for (k, v) in pairs {
                table_put(rt, m, k, v);
            }
            m
        }
        ("java/util/Map", "entry") => entry(rt, a(0), a(1)),
        // Guava: a row → (column → value) table, and key → collection multimaps.
        (o, "create") if (o.ends_with("BasedTable") || o.ends_with("Multimap")) && o.starts_with("com/google/common/collect/") => {
            let class: &'static str = rt.static_key(o);
            rt.alloc(class, Data::Table(Table::default()))
        }
        // `Multimaps.newMultimap(map, factory)` (`newListMultimap`, `newSetMultimap`): an empty
        // multimap whose keys hold the factory's collections, a set holding each value once.
        (
            "com/google/common/collect/Multimaps",
            "newMultimap" | "newListMultimap" | "newSetMultimap" | "newSortedSetMultimap",
        ) => {
            let sets = match name {
                "newListMultimap" => false,
                "newSetMultimap" | "newSortedSetMultimap" => true,
                _ => {
                    let made = rt.call_functional(
                        "com/google/common/base/Supplier.get.()Ljava/lang/Object;",
                        a(1),
                        &[],
                    );
                    rt.class_of(made).ends_with("Set")
                }
            };
            let class = if sets {
                "com/google/common/collect/HashMultimap"
            } else {
                "com/google/common/collect/ArrayListMultimap"
            };
            rt.alloc(class, Data::Table(Table::default()))
        }
        // `MultimapBuilder.hashKeys().arrayListValues().build()`: the keys' order makes no
        // difference here; the values' kind says whether a key holds a list or a set.
        ("com/google/common/collect/MultimapBuilder", "hashKeys" | "linkedHashKeys" | "treeKeys" | "enumKeys") => {
            rt.new_object("com/google/common/collect/MultimapBuilder$MultimapBuilderWithKeys")
        }
        ("com/google/common/collect/MultimapBuilder$MultimapBuilderWithKeys", n) => {
            if n.ends_with("SetValues") {
                rt.new_object("com/google/common/collect/MultimapBuilder$SetMultimapBuilder")
            } else {
                rt.new_object("com/google/common/collect/MultimapBuilder$ListMultimapBuilder")
            }
        }
        ("com/google/common/collect/MultimapBuilder$SetMultimapBuilder" | "com/google/common/collect/MultimapBuilder$ListMultimapBuilder", "build") => {
            let class = if owner.ends_with("SetMultimapBuilder") { "com/google/common/collect/HashMultimap" } else { "com/google/common/collect/ArrayListMultimap" };
            let m = rt.alloc(class, Data::Table(Table::default()));
            // `build(multimap)` starts with its entries.
            if args.len() > 1 {
                for (k, c) in table_entries(rt, a(1)) {
                    let copy = if class.contains("Hash") { new_set(rt, &elements(rt, c)) } else { new_list(rt, elements(rt, c)) };
                    table_put(rt, m, k, copy);
                }
            }
            m
        }
        (
            "com/google/common/collect/ImmutableList"
            | "com/google/common/collect/ImmutableSet"
            | "com/google/common/collect/ImmutableSortedSet"
            | "com/google/common/collect/ImmutableMap"
            | "com/google/common/collect/ImmutableSortedMap"
            | "com/google/common/collect/ImmutableBiMap",
            "builder" | "builderWithExpectedSize" | "naturalOrder",
        ) => {
            let class: &'static str = rt.static_key(&format!("{owner}$Builder"));
            if owner.ends_with("Map") {
                rt.alloc(class, Data::Table(Table::default()))
            } else {
                rt.alloc(class, Data::List(Vec::new()))
            }
        }
        ("com/google/common/base/Preconditions", "checkNotNull") => a(0),
        ("com/google/common/base/Preconditions", _) => V::Void,
        ("java/util/Collections", "emptyList" | "emptySet" | "emptyMap") => match name {
            "emptyMap" => rt.alloc("java/util/HashMap", Data::Table(Table::default())),
            "emptySet" => new_set(rt, &[]),
            _ => new_list(rt, Vec::new()),
        },
        ("java/util/Collections", "singletonList" | "singleton") => match name {
            "singleton" => new_set(rt, &[a(0)]),
            _ => new_list(rt, vec![a(0)]),
        },
        ("java/util/Collections", "singletonMap") => {
            let m = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
            table_put(rt, m, a(0), a(1));
            m
        }
        ("java/util/Collections", n) if n.starts_with("unmodifiable") || n.starts_with("synchronized") => a(0),
        // A set over a map (`newSetFromMap(new WeakHashMap<>())`): a set; weak keys make no
        // difference to what it holds while it is used.
        ("java/util/Collections", "newSetFromMap") => rt.alloc("java/util/HashSet", Data::Table(Table::default())),
        // Guava's `Tables.newCustomTable(backingMap, factory)`: a table of rows.
        ("com/google/common/collect/Tables", "newCustomTable") => rt.alloc("com/google/common/collect/HashBasedTable", Data::Table(Table::default())),
        // A synchronised or unmodifiable view of one of Guava's collections is the collection:
        // one thread, and nobody here writes through a view it was told not to (Balm keeps its
        // registrations in `Tables.synchronizedTable(HashBasedTable.create())`).
        (o, n)
            if o.starts_with("com/google/common/collect/")
                && (n.starts_with("synchronized") || n.starts_with("unmodifiable"))
                && a(0).r() != 0
                && args.len() <= 2 =>
        {
            a(0)
        }
        ("com/google/common/collect/Tables", "immutableCell") => table_cell(rt, a(0), a(1), a(2)),
        // fastutil's `ObjectArrayList.of(items...)`.
        (o, "of") if o.starts_with("it/unimi/dsi/fastutil/") && o.ends_with("ArrayList") => {
            let items = if desc.starts_with("([") { elements(rt, a(0)) } else { args.to_vec() };
            let class: &'static str = rt.static_key(o);
            rt.alloc(class, Data::List(items))
        }
        // fastutil's wrappers (`Object2ReferenceMaps.synchronize(map)`, `ObjectSets.unmodifiable(set)`):
        // one thread's view of the same collection.
        (o, "synchronize" | "unmodifiable") if o.starts_with("it/unimi/dsi/fastutil/") && args.len() == 1 => a(0),
        // fastutil's `Int2ObjectMaps.fastIterator(map)`, `fastIterable(map)` and `fastForEach(map,
        // action)`: the map's typed entry set, which its own `int2ObjectEntrySet()` gives.
        (o, "fastIterator" | "fastIterable" | "fastForEach")
            if o.starts_with("it/unimi/dsi/fastutil/")
                && o.ends_with("Maps")
                && is_map_object(rt, a(0))
                && matches!(rt.heap.get(a(0).r() as usize).map(|o| &o.data), Some(Data::Table(_))) =>
        {
            let es = map_call(rt, "entrySet", "()Lit/unimi/dsi/fastutil/objects/ObjectSet;", &[a(0)])?;
            match name {
                "fastIterable" => es,
                "fastIterator" => set_call(rt, "iterator", &[es])?,
                _ => {
                    for e in elements(rt, es) {
                        rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[e]);
                    }
                    V::Void
                }
            }
        }
        // `Collections.binarySearch(list, key[, comparator])`: the index, else `-(insertion point) - 1`.
        ("java/util/Collections", "binarySearch") => {
            let items = elements(rt, a(0));
            let cmp = if args.len() > 2 { a(2) } else { V::NULL };
            let (mut lo, mut hi) = (0i32, items.len() as i32 - 1);
            while lo <= hi {
                let mid = (lo + hi) >> 1;
                let c = compare(rt, cmp, items[mid as usize], a(1));
                if c < 0 {
                    lo = mid + 1;
                } else if c > 0 {
                    hi = mid - 1;
                } else {
                    return Some(V::I(mid));
                }
            }
            V::I(-(lo + 1))
        }
        // Commons Lang's array lookups: `contains`/`indexOf` by `equals` (an object array), or by
        // value (a primitive one).
        ("org/apache/commons/lang3/ArrayUtils", "contains" | "indexOf") if args.len() == 2 => {
            let items = elements(rt, a(0));
            let mut at = -1;
            for (i, &e) in items.iter().enumerate() {
                if e == a(1) || (matches!(e, V::R(_)) && equals(rt, e, a(1))) {
                    at = i as i32;
                    break;
                }
            }
            if name == "contains" { V::I((at >= 0) as i32) } else { V::I(at) }
        }
        ("org/apache/commons/lang3/ArrayUtils", "isEmpty") => V::I(elements(rt, a(0)).is_empty() as i32),
        // Guava's `HashBiMap.create()`, with its inverse; `create(map)` holds the map's entries.
        // (An `EnumHashBiMap`'s keys are in the order they were put, not their enum's.)
        (
            "com/google/common/collect/HashBiMap"
            | "com/google/common/collect/EnumHashBiMap"
            | "com/google/common/collect/EnumBiMap",
            "create",
        ) => {
            let class = match owner {
                "com/google/common/collect/EnumHashBiMap" => {
                    "com/google/common/collect/EnumHashBiMap"
                }
                "com/google/common/collect/EnumBiMap" => "com/google/common/collect/EnumBiMap",
                _ => "com/google/common/collect/HashBiMap",
            };
            let m = new_bimap(rt, class);
            for (k, v) in table_entries(rt, a(0)) {
                table_put(rt, m, k, v);
            }
            m
        }
        // Commons Lang's in-place reversal of an array.
        ("org/apache/commons/lang3/ArrayUtils", "reverse") => {
            if let Some(Data::Array(items)) = rt.heap.get_mut(a(0).r() as usize).map(|o| &mut o.data) {
                items.reverse();
            }
            V::Void
        }
        ("java/util/Collections", "sort") | ("java/util/List", "sort") => {
            sort(rt, a(0), args.get(1).copied().unwrap_or(V::NULL));
            V::Void
        }
        // `shuffle(list[, random])`: each place from the end swapped with one at or before it,
        // drawn by the random's `nextInt(i + 1)` (the runtime's own draws without one).
        ("java/util/Collections", "shuffle") if list(rt, a(0)).is_some() => {
            let n = list(rt, a(0)).map_or(0, Vec::len);
            for i in (1..n).rev() {
                let j = if desc.contains("Random") && a(1).r() != 0 {
                    crate::api::call(
                        rt,
                        "java/util/Random.nextInt.(I)I",
                        &[a(1), V::I(i as i32 + 1)],
                    )
                    .i()
                } else {
                    (crate::api_common::next_random(rt) % (i as u64 + 1)) as i32
                };
                if rt.thrown.is_some() {
                    break;
                }
                if let (Ok(j), Some(l)) = (usize::try_from(j), list_mut(rt, a(0)))
                    && j <= i
                {
                    l.swap(i, j);
                }
            }
            V::Void
        }
        ("java/util/Collections", "emptyIterator" | "emptyListIterator") => new_iter(rt, Vec::new()),
        // `requireNonNullElseGet(o, supplier)`: `o`, else what the supplier gives, which must not
        // be null either.
        ("java/util/Objects", "requireNonNullElseGet") => {
            if a(0).r() != 0 {
                a(0)
            } else if a(1).r() == 0 {
                crate::jlang::throw_with(rt, "java/lang/NullPointerException", "supplier")
            } else {
                let v = rt.call_functional(SUPPLY, a(1), &[]);
                if rt.thrown.is_none() && v.is_null() {
                    crate::jlang::throw_with(rt, "java/lang/NullPointerException", "supplier.get()")
                } else {
                    v
                }
            }
        }
        // `new AbstractMap.SimpleEntry(key, value)` (or of another entry): a map entry, as
        // `Map.entry` makes, of its own class.
        (SIMPLE_ENTRY | SIMPLE_IMMUTABLE_ENTRY, "<init>") => {
            let (k, v) = if let Some(kv) =
                list(rt, a(1)).filter(|kv| kv.len() == 2 && is_entry(rt.class_of(a(1))))
            {
                (kv[0], kv[1])
            } else if desc.starts_with("(Ljava/util/Map$Entry;") {
                let k = rt.call_functional(
                    "java/util/Map$Entry.getKey.()Ljava/lang/Object;",
                    a(1),
                    &[],
                );
                (
                    k,
                    rt.call_functional(
                        "java/util/Map$Entry.getValue.()Ljava/lang/Object;",
                        a(1),
                        &[],
                    ),
                )
            } else {
                (a(1), a(2))
            };
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::List(vec![k, v]);
            }
            V::Void
        }
        // Empty streams, and two streams one after the other.
        (STREAM | INT_STREAM | LONG_STREAM | DOUBLE_STREAM, "empty") => {
            let class: &'static str = rt.static_key(owner);
            rt.alloc(class, Data::Stream(Vec::new()))
        }
        (STREAM | INT_STREAM | LONG_STREAM | DOUBLE_STREAM, "concat") => {
            let mut items = elements(rt, a(0));
            items.extend(elements(rt, a(1)));
            let class: &'static str = rt.static_key(owner);
            rt.alloc(class, Data::Stream(items))
        }
        (STREAM, "builder") => rt.alloc(STREAM_BUILDER, Data::List(Vec::new())),
        // A spliterator here holds what it would go through; a stream of one holds the same.
        (_, "spliterator") if desc == "()Ljava/util/Spliterator;" => {
            let items = drain(rt, a(0));
            rt.alloc("java/util/Spliterator", Data::Stream(items))
        }
        ("java/util/Spliterators", "spliteratorUnknownSize" | "spliterator")
            if desc.starts_with("(Ljava/util/Iterator;") =>
        {
            let items = drain(rt, a(0));
            rt.alloc("java/util/Spliterator", Data::Stream(items))
        }
        ("java/util/stream/StreamSupport", "stream")
            if desc.starts_with("(Ljava/util/Spliterator;") =>
        {
            let items = elements(rt, a(0));
            rt.alloc(STREAM, Data::Stream(items))
        }
        ("java/util/Collections", "reverse") => {
            if let Some(l) = list_mut(rt, a(0)) {
                l.reverse();
            }
            V::Void
        }
        // The bounds checks of the JDK's own code: the index back, or the exception.
        ("java/util/Objects", "checkIndex" | "checkFromToIndex" | "checkFromIndexSize") => {
            let n = |i: usize| {
                if desc.starts_with("(J") {
                    a(i).j()
                } else {
                    i64::from(a(i).i())
                }
            };
            let (ok, what) = match name {
                "checkIndex" => (
                    n(0) >= 0 && n(0) < n(1),
                    format!("Index {} out of bounds for length {}", n(0), n(1)),
                ),
                "checkFromToIndex" => (
                    n(0) >= 0 && n(0) <= n(1) && n(1) <= n(2),
                    format!(
                        "Range [{}, {}) out of bounds for length {}",
                        n(0),
                        n(1),
                        n(2)
                    ),
                ),
                _ => (
                    n(0) >= 0 && n(1) >= 0 && n(0) <= n(2) - n(1),
                    format!(
                        "Range [{}, {} + {}) out of bounds for length {}",
                        n(0),
                        n(0),
                        n(1),
                        n(2)
                    ),
                ),
            };
            if !ok {
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/IndexOutOfBoundsException",
                    &what,
                ));
            }
            a(0)
        }
        ("java/util/Objects", "requireNonNull" | "requireNonNullElse") => {
            if a(0).r() == 0 && name == "requireNonNullElse" { a(1) } else { a(0) }
        }
        // Guava's `Objects.equal(a, b)` and `Objects.hashCode(Object...)` are `Objects.equals`
        // and `Objects.hash`.
        ("java/util/Objects", "equals") | ("com/google/common/base/Objects", "equal") => {
            V::I(equals(rt, a(0), a(1)) as i32)
        }
        ("java/util/Objects", "hashCode") => V::I(if a(0).is_null() { 0 } else { hash(rt, a(0)) }),
        ("java/util/Objects", "hash") | ("com/google/common/base/Objects", "hashCode") => {
            let items = elements(rt, a(0));
            V::I(items.into_iter().fold(1i32, |h, v| h.wrapping_mul(31).wrapping_add(if v.is_null() { 0 } else { hash(rt, v) })))
        }
        ("java/util/Objects", "isNull") => V::I(a(0).is_null() as i32),
        // `Objects.toString(o[, nullDefault])`: `String.valueOf(o)`, or the default for null.
        ("java/util/Objects", "toString") => {
            if a(0).is_null() && desc.starts_with("(Ljava/lang/Object;Ljava/lang/String;)") {
                a(1)
            } else {
                let s = rt.to_java_string(a(0), 'L');
                rt.string(&s)
            }
        }
        // `Stream.generate(supplier)` is endless: the supplier is asked once `limit` says how often.
        ("java/util/stream/Stream", "generate") => {
            let g = rt.new_object(GENERATED_STREAM);
            rt.put_field(g, "$supplier", a(0));
            g
        }
        ("java/util/Objects", "nonNull") => V::I(!a(0).is_null() as i32),
        // `IntStream.of(...)`, `LongStream.of(...)`, `DoubleStream.of(...)`: an array's values, or the
        // one given, held unboxed.
        ("java/util/stream/IntStream" | "java/util/stream/LongStream" | "java/util/stream/DoubleStream", "of") => {
            let items = if desc.starts_with("([") { elements(rt, a(0)) } else { vec![a(0)] };
            let class: &'static str = rt.static_key(owner);
            rt.alloc(class, Data::Stream(items))
        }
        // `IntStream.range(lo, hi)` / `rangeClosed(lo, hi)`: its ints, held unboxed.
        ("java/util/stream/IntStream", "range" | "rangeClosed") => {
            let (lo, hi) = (i64::from(a(0).i()), i64::from(a(1).i()) + i64::from(name == "rangeClosed"));
            let items = (lo..hi).map(|i| V::I(i as i32)).collect();
            rt.alloc("java/util/stream/IntStream", Data::Stream(items))
        }
        ("java/util/stream/Stream", "of") => {
            let items = varargs(rt);
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        // `Map.Entry.comparingByKey()` and `comparingByValue()`, in the natural order or by a
        // comparator: a comparator of entries by that half.
        ("java/util/Map$Entry", "comparingByKey" | "comparingByValue") => {
            let half = if name == "comparingByKey" {
                "java/util/Map$Entry.getKey.()Ljava/lang/Object;"
            } else {
                "java/util/Map$Entry.getValue.()Ljava/lang/Object;"
            };
            let (c, how, half) = (rt.new_object("java/util/Comparator"), rt.string("entry"), rt.string(half));
            rt.put_field(c, "$how", how);
            rt.put_field(c, "$x", half);
            rt.put_field(c, "$y", a(0));
            c
        }
        // `Stream.ofNullable(x)`: of `x`, or of nothing for null.
        ("java/util/stream/Stream", "ofNullable") => {
            let items = if is_null(a(0)) {
                Vec::new()
            } else {
                vec![a(0)]
            };
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        ("java/util/Arrays", "stream") => {
            let items = elements(rt, a(0));
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        // `LongAdder`: a sum many threads add to, which here is one number (`value`).
        ("java/util/concurrent/atomic/LongAdder", n) => {
            let this = a(0);
            let sum = match rt.get_field(this, "value") {
                V::J(j) => j,
                _ => 0,
            };
            match n {
                "<init>" | "reset" => {
                    rt.put_field(this, "value", V::J(0));
                    V::Void
                }
                "add" | "increment" | "decrement" => {
                    let by = match n {
                        "add" => a(1).j(),
                        "increment" => 1,
                        _ => -1,
                    };
                    rt.put_field(this, "value", V::J(sum.wrapping_add(by)));
                    V::Void
                }
                "sum" | "longValue" => V::J(sum),
                "intValue" => V::I(sum as i32),
                "floatValue" => V::F(sum as f32),
                "doubleValue" => V::D(sum as f64),
                "sumThenReset" => {
                    rt.put_field(this, "value", V::J(0));
                    V::J(sum)
                }
                "toString" => rt.string(&sum.to_string()),
                _ => return None,
            }
        }
        // Atomics: one `value` field.
        ("java/util/concurrent/atomic/AtomicInteger" | "java/util/concurrent/atomic/AtomicLong" | "java/util/concurrent/atomic/AtomicBoolean" | "java/util/concurrent/atomic/AtomicReference", n) => {
            let this = a(0);
            let zero = match owner {
                o if o.ends_with("Long") => V::J(0),
                o if o.ends_with("Reference") => V::NULL,
                _ => V::I(0),
            };
            let cur = match rt.get_field(this, "value") {
                V::Void => zero,
                v => v,
            };
            match n {
                "<init>" => {
                    rt.put_field(this, "value", args.get(1).copied().unwrap_or(zero));
                    V::Void
                }
                "get" | "intValue" | "longValue" | "getPlain" | "getAcquire" | "getOpaque" => cur,
                "set" | "lazySet" | "setPlain" | "setRelease" | "setOpaque" => {
                    rt.put_field(this, "value", a(1));
                    V::Void
                }
                "getAndSet" => {
                    rt.put_field(this, "value", a(1));
                    cur
                }
                "compareAndSet" | "weakCompareAndSet" | "weakCompareAndSetPlain" | "weakCompareAndSetVolatile" | "weakCompareAndSetAcquire" | "weakCompareAndSetRelease" => {
                    let same = cur == a(1) || (owner.ends_with("Reference") && equals(rt, cur, a(1)));
                    if same {
                        rt.put_field(this, "value", a(2));
                    }
                    V::I(same as i32)
                }
                // The value before, set when it was the one expected (Create makes a contraption's
                // client side lazily: `compareAndExchangeRelease(null, made)`).
                "compareAndExchange" | "compareAndExchangeAcquire" | "compareAndExchangeRelease" => {
                    let same = cur == a(1) || (owner.ends_with("Reference") && equals(rt, cur, a(1)));
                    if same {
                        rt.put_field(this, "value", a(2));
                    }
                    cur
                }
                "incrementAndGet" | "getAndIncrement" | "decrementAndGet" | "getAndDecrement" | "addAndGet" | "getAndAdd" => {
                    let delta = match n {
                        "addAndGet" | "getAndAdd" => a(1),
                        n if n.contains("ecrement") => if owner.ends_with("Long") { V::J(-1) } else { V::I(-1) },
                        _ => if owner.ends_with("Long") { V::J(1) } else { V::I(1) },
                    };
                    let next = match (cur, delta) {
                        (V::J(x), V::J(d)) => V::J(x.wrapping_add(d)),
                        (V::I(x), V::I(d)) => V::I(x.wrapping_add(d)),
                        _ => cur,
                    };
                    rt.put_field(this, "value", next);
                    if n.starts_with("getAnd") { cur } else { next }
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn sort(rt: &mut Rt, l: V, comparator: V) {
    let mut items = list(rt, l).cloned().unwrap_or_default();
    if merge_sort(rt, &mut items, comparator)
        && let Some(dst) = list_mut(rt, l)
    {
        *dst = items;
    }
}

/// A stable merge sort through `compare` (which may call back into the mod). Stable, as Java's
/// object sorts are; a comparator that isn't a total order gives some order (Rust's `sort_by` may
/// panic on one). False, with `items` left unsorted, if the comparator threw.
fn merge_sort(rt: &mut Rt, items: &mut Vec<V>, comparator: V) -> bool {
    let n = items.len();
    let mut buf = items.clone();
    let mut width = 1;
    while width < n {
        let mut lo = 0;
        while lo < n {
            let (mid, hi) = ((lo + width).min(n), (lo + 2 * width).min(n));
            let (mut l, mut r) = (lo, mid);
            for slot in &mut buf[lo..hi] {
                // The right run's element goes first only when strictly smaller: equal ones keep their order.
                let right = l == mid || (r < hi && compare(rt, comparator, items[r], items[l]) < 0);
                if rt.thrown.is_some() {
                    return false;
                }
                *slot = if right { items[r] } else { items[l] };
                if right { r += 1 } else { l += 1 }
            }
            lo = hi;
        }
        std::mem::swap(items, &mut buf);
        width *= 2;
    }
    true
}

/// Java's total order on doubles (`Double.compare`): -0.0 before 0.0, NaN after everything.
fn java_double_cmp(x: f64, y: f64) -> std::cmp::Ordering {
    match (x.is_nan(), y.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => x.total_cmp(&y),
    }
}

/// `java.util.Arrays`' helpers on the runtime's arrays. The element type comes from the descriptor
/// (`sort([I)V`): primitives sort by value, objects through `compare` (natural order or the
/// comparator). `asList` and `stream` are the list arms'.
fn arrays_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let elem = desc.strip_prefix("([").and_then(|d| d.chars().next())?;
    let zero = match elem {
        'J' => V::J(0),
        'F' => V::F(0.0),
        'D' => V::D(0.0),
        'L' | '[' => V::NULL,
        _ => V::I(0),
    };
    let items = |rt: &Rt, v: V| match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Array(items)) => Some(items.clone()),
        _ => None,
    };
    let store = |rt: &mut Rt, v: V, new: Vec<V>| {
        if let Some(Object { data: Data::Array(items), .. }) = rt.heap.get_mut(v.r() as usize) {
            *items = new;
        }
    };
    // A range `[from, to)` of an array of `len`, checked as `Arrays.rangeCheck`.
    let range = |rt: &mut Rt, from: i32, to: i32, len: usize| -> Option<(usize, usize)> {
        if from > to {
            rt.throw_new("java/lang/IllegalArgumentException");
            None
        } else if from < 0 || to as usize > len {
            rt.throw_new("java/lang/ArrayIndexOutOfBoundsException");
            None
        } else {
            Some((from as usize, to as usize))
        }
    };
    let bits = |v: V| -> i64 {
        match elem {
            'F' => {
                let f = v.f();
                i64::from(if f.is_nan() { 0x7fc0_0000 } else { f.to_bits() as i32 })
            }
            'D' => {
                let d = v.d();
                if d.is_nan() { 0x7ff8_0000_0000_0000 } else { d.to_bits() as i64 }
            }
            'J' => v.j(),
            _ => i64::from(v.i()),
        }
    };
    Some(match name {
        "sort" | "parallelSort" => {
            if a(0).r() == 0 {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::Void);
            }
            let mut all = items(rt, a(0))?;
            let (from, to, comparator) = match args.len() {
                1 => (0, all.len() as i32, V::NULL),
                2 => (0, all.len() as i32, a(1)),
                3 => (a(1).i(), a(2).i(), V::NULL),
                _ => (a(1).i(), a(2).i(), a(3)),
            };
            let Some((from, to)) = range(rt, from, to, all.len()) else { return Some(V::Void) };
            let mut part = all[from..to].to_vec();
            match elem {
                'L' | '[' => {
                    if !merge_sort(rt, &mut part, comparator) {
                        return Some(V::Void);
                    }
                }
                'F' => part.sort_by(|x, y| java_double_cmp(f64::from(x.f()), f64::from(y.f()))),
                'D' => part.sort_by(|x, y| java_double_cmp(x.d(), y.d())),
                'J' => part.sort_by_key(|x| x.j()),
                _ => part.sort_by_key(|x| x.i()),
            }
            all.splice(from..to, part);
            store(rt, a(0), all);
            V::Void
        }
        // `setAll(array, generator)`: each element what the generator makes of its index, in
        // order (an element is stored before the next is asked for).
        "setAll" => {
            let len = items(rt, a(0))?.len();
            let apply = match elem {
                'I' => "java/util/function/IntUnaryOperator.applyAsInt.(I)I",
                'J' => "java/util/function/IntToLongFunction.applyAsLong.(I)J",
                'D' => "java/util/function/IntToDoubleFunction.applyAsDouble.(I)D",
                _ => "java/util/function/IntFunction.apply.(I)Ljava/lang/Object;",
            };
            for i in 0..len {
                let v = rt.call_functional(apply, a(1), &[V::I(i as i32)]);
                if rt.thrown.is_some() {
                    break;
                }
                rt.array_set(a(0), i as i32, v);
            }
            V::Void
        }
        "fill" => {
            let mut all = items(rt, a(0))?;
            let (from, to, value) = if args.len() >= 4 { (a(1).i(), a(2).i(), a(3)) } else { (0, all.len() as i32, a(1)) };
            let Some((from, to)) = range(rt, from, to, all.len()) else { return Some(V::Void) };
            all[from..to].fill(value);
            store(rt, a(0), all);
            V::Void
        }
        "copyOf" | "copyOfRange" => {
            let all = items(rt, a(0))?;
            let (from, to) = if name == "copyOf" { (0, a(1).i()) } else { (a(1).i(), a(2).i()) };
            if name == "copyOf" && to < 0 {
                rt.throw_new("java/lang/NegativeArraySizeException");
                return Some(V::NULL);
            }
            if from > to {
                rt.throw_new("java/lang/IllegalArgumentException");
                return Some(V::NULL);
            }
            if from < 0 || from as usize > all.len() {
                rt.throw_new("java/lang/ArrayIndexOutOfBoundsException");
                return Some(V::NULL);
            }
            // The copy is `to - from` long: past the original's end it holds the type's zero.
            let copy: Vec<V> = (from..to).map(|i| all.get(i as usize).copied().unwrap_or(zero)).collect();
            let class = rt.class_of(a(0));
            rt.alloc(class, Data::Array(copy))
        }
        "equals" if args.len() == 2 => {
            let same = match (a(0).r() == 0, a(1).r() == 0) {
                (true, true) => true,
                (true, false) | (false, true) => false,
                _ => {
                    let (x, y) = (items(rt, a(0)).unwrap_or_default(), items(rt, a(1)).unwrap_or_default());
                    x.len() == y.len()
                        && match elem {
                            'L' | '[' => x.iter().zip(&y).all(|(&p, &q)| (p.r() == 0 && q.r() == 0) || (p.r() != 0 && equals(rt, p, q))),
                            _ => x.iter().zip(&y).all(|(&p, &q)| bits(p) == bits(q)),
                        }
                }
            };
            V::I(same as i32)
        }
        "hashCode" => {
            if a(0).r() == 0 {
                return Some(V::I(0));
            }
            let all = items(rt, a(0))?;
            let mut h = 1i32;
            for v in all {
                let e = match elem {
                    'L' | '[' => {
                        if v.r() == 0 { 0 } else { hash(rt, v) }
                    }
                    'Z' => {
                        if v.i() != 0 { 1231 } else { 1237 }
                    }
                    'J' | 'D' => {
                        let b = bits(v);
                        (b ^ ((b as u64) >> 32) as i64) as i32
                    }
                    _ => bits(v) as i32,
                };
                h = h.wrapping_mul(31).wrapping_add(e);
            }
            V::I(h)
        }
        _ => return None,
    })
}

fn list_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let len = list(rt, this)?.len();
    let at = |i: V| i.i().max(0) as usize;
    Some(match name {
        "size" => V::I(len as i32),
        "isEmpty" => V::I((len == 0) as i32),
        "get" | "elementAt" => list(rt, this)?.get(at(a(1))).copied().unwrap_or(V::NULL),
        // fastutil's typed lists (`IntList.getInt(i)`): the element as the type returns it.
        "getInt" | "getLong" | "getFloat" | "getDouble" | "getBoolean" | "getByte" | "getShort" | "getChar" => {
            let v = list(rt, this)?.get(at(a(1))).copied().unwrap_or(V::Void);
            let kind = desc.rsplit(')').next().and_then(|r| r.chars().next()).unwrap_or('I');
            crate::mixin::unbox(rt, v, kind)
        }
        "getFirst" | "peek" | "peekFirst" | "element" | "first" => list(rt, this)?.first().copied().unwrap_or(V::NULL),
        "getLast" | "peekLast" | "last" => list(rt, this)?.last().copied().unwrap_or(V::NULL),
        // One element (`add(E)`, or fastutil's `add(int)`, `add(long)`...); `add(int index, E)` inserts.
        "add" | "addLast" | "offer" | "offerLast" | "push" | "addFirst" | "offerFirst" if crate::jlang::kinds(desc).len() == 1 => {
            let v = a(1);
            let front = matches!(name, "push" | "addFirst" | "offerFirst") && rt.class_of(this) != "java/util/Stack";
            let l = list_mut(rt, this)?;
            if front { l.insert(0, v) } else { l.push(v) }
            if desc.ends_with(")Z") { V::I(1) } else { V::Void }
        }
        "add" | "insertElementAt" => {
            let (i, v) = if name == "add" { (at(a(1)), a(2)) } else { (at(a(2)), a(1)) };
            let l = list_mut(rt, this)?;
            l.insert(i.min(l.len()), v);
            V::Void
        }
        "set" => {
            let (i, v) = (at(a(1)), a(2));
            let l = list_mut(rt, this)?;
            match l.get_mut(i) {
                Some(slot) => std::mem::replace(slot, v),
                None => V::NULL,
            }
        }
        "addAll" => {
            let (i, src) = if desc.starts_with("(I") { (Some(at(a(1))), a(2)) } else { (None, a(1)) };
            let items = elements(rt, src);
            let any = !items.is_empty();
            let l = list_mut(rt, this)?;
            match i {
                Some(i) => {
                    let i = i.min(l.len());
                    l.splice(i..i, items);
                }
                None => l.extend(items),
            }
            V::I(any as i32)
        }
        "remove" | "removeFirst" | "poll" | "pollFirst" | "pop" | "removeLast" | "pollLast" => {
            if name == "remove" && desc == "(I)Ljava/lang/Object;" {
                let i = at(a(1));
                let l = list_mut(rt, this)?;
                return Some(if i < l.len() { l.remove(i) } else { V::NULL });
            }
            if name == "remove" && desc.starts_with("(Ljava/lang/Object;)") {
                let items = list(rt, this)?.clone();
                let pos = items.iter().position(|&e| equals(rt, e, a(1)));
                if let Some(p) = pos {
                    list_mut(rt, this)?.remove(p);
                }
                return Some(V::I(pos.is_some() as i32));
            }
            let from_back = matches!(name, "removeLast" | "pollLast") || (name == "pop" && rt.class_of(this) == "java/util/Stack");
            let l = list_mut(rt, this)?;
            if l.is_empty() {
                V::NULL
            } else if from_back {
                l.pop().unwrap_or(V::NULL)
            } else {
                l.remove(0)
            }
        }
        "clear" | "removeAllElements" => {
            list_mut(rt, this)?.clear();
            V::Void
        }
        "contains" | "indexOf" | "lastIndexOf" => {
            let items = list(rt, this)?.clone();
            let mut found = None;
            for (i, &e) in items.iter().enumerate() {
                if equals(rt, e, a(1)) {
                    found = Some(i);
                    if name != "lastIndexOf" {
                        break;
                    }
                }
            }
            match name {
                "contains" => V::I(found.is_some() as i32),
                _ => V::I(found.map_or(-1, |i| i as i32)),
            }
        }
        // A list iterator walks the list itself, by index: it sets, adds and removes in place.
        "listIterator" => {
            let at = if desc.starts_with("(I)") { a(1).i() } else { 0 };
            if at < 0 || at as usize > len {
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/IndexOutOfBoundsException",
                    &format!("Index: {at}, Size: {len}"),
                ));
            }
            let it = rt.new_object(LIST_ITERATOR);
            rt.put_field(it, "$list", this);
            rt.put_field(it, "$cursor", V::I(at));
            rt.put_field(it, "$last", V::I(-1));
            it
        }
        "iterator" | "descendingIterator" => {
            let mut items = list(rt, this)?.clone();
            if name == "descendingIterator" {
                items.reverse();
            }
            iter_over(rt, this, items)
        }
        "stream" | "parallelStream" => {
            let items = list(rt, this)?.clone();
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        "forEach" => list_for_each(rt, this, a(1))?,
        "removeIf" => {
            let items = list(rt, this)?.clone();
            let mut keep = Vec::new();
            for e in &items {
                if rt.call_functional("java/util/function/Predicate.test.(Ljava/lang/Object;)Z", a(1), &[*e]).i() == 0 {
                    keep.push(*e);
                }
            }
            let removed = keep.len() != items.len();
            *list_mut(rt, this)? = keep;
            V::I(removed as i32)
        }
        // Every element the other collection holds (`equals`), each occurrence; or all the others.
        "removeAll" | "retainAll" => {
            let items = list(rt, this)?.clone();
            let other = elements(rt, a(1));
            let mut keep = Vec::new();
            for e in &items {
                let in_other = other.iter().any(|&o| equals(rt, o, *e));
                if in_other != (name == "removeAll") {
                    keep.push(*e);
                }
            }
            let changed = keep.len() != items.len();
            *list_mut(rt, this)? = keep;
            V::I(changed as i32)
        }
        "sort" => {
            sort(rt, this, a(1));
            V::Void
        }
        "subList" => {
            let items = list(rt, this)?;
            let (lo, hi) = (at(a(1)).min(items.len()), at(a(2)).min(items.len()));
            let part = items[lo..hi.max(lo)].to_vec();
            new_list(rt, part)
        }
        "toArray" => {
            let items = list(rt, this)?.clone();
            to_array(rt, items, a(1))
        }
        n if typed_array(n).is_some() => {
            let items = list(rt, this)?.clone();
            to_typed_array(rt, n, items, a(1))
        }
        "equals" => V::I(equals(rt, this, a(1)) as i32),
        "hashCode" => {
            let items = list(rt, this)?.clone();
            V::I(items.into_iter().fold(1i32, |h, v| h.wrapping_mul(31).wrapping_add(if v.r() == 0 { 0 } else { hash(rt, v) })))
        }
        "reversed" => {
            let mut items = list(rt, this)?.clone();
            items.reverse();
            new_list(rt, items)
        }
        "trimToSize" | "ensureCapacity" => V::Void,
        _ => return None,
    })
}

const ENUM_SET: &str = "java/util/EnumSet";

/// A `java.util.EnumSet` of `class`'s constants: a set kept in their order (by ordinal), which
/// remembers its element type (`$type`) for `complementOf`.
fn new_enum_set(rt: &mut Rt, class: &str, items: Vec<V>) -> V {
    let s = rt.alloc(ENUM_SET, Data::Table(Table { sorted: true, ..Table::default() }));
    if !class.is_empty() {
        let c = rt.string(class);
        rt.put_field(s, "$type", c);
    }
    for e in items {
        table_put(rt, s, e, V::I(1));
    }
    s
}

/// The enum class an `EnumSet` holds: its `$type`, else its first element's class.
fn enum_set_type(rt: &Rt, set: V) -> String {
    if let Some(t) = rt.str_of(rt.get_field(set, "$type")) {
        return t.to_owned();
    }
    elements(rt, set).first().map(|&e| rt.class_of(e).to_owned()).unwrap_or_default()
}

/// Every constant of enum `class`, in order.
fn all_constants(rt: &mut Rt, class: &str) -> Vec<V> {
    crate::jlang::enum_constants(rt, class).map(|v| elements(rt, v)).unwrap_or_default()
}

/// `EnumSet`'s factories (`noneOf`, `allOf`, `of`, `copyOf`, `range`, `complementOf`) and `clone`;
/// None for its other methods, which a set answers.
fn enum_set_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let class_arg = |rt: &Rt, c: V| rt.str_of(c).unwrap_or("").replace('.', "/");
    Some(match name {
        "noneOf" => {
            let class = class_arg(rt, a(0));
            new_enum_set(rt, &class, Vec::new())
        }
        "allOf" => {
            let class = class_arg(rt, a(0));
            let all = all_constants(rt, &class);
            new_enum_set(rt, &class, all)
        }
        // `of(e1, ..., e5)` and `of(E first, E... rest)`.
        "of" => {
            let mut items: Vec<V> = args.to_vec();
            if desc.contains("[L")
                && let Some(rest) = items.pop()
            {
                items.extend(elements(rt, rest));
            }
            let class = items.first().map(|&e| rt.class_of(e).to_owned()).unwrap_or_default();
            new_enum_set(rt, &class, items)
        }
        // A collection that isn't an `EnumSet` must hold something to give the type.
        "copyOf" => {
            let src = a(0);
            let items = elements(rt, src);
            if rt.class_of(src) != ENUM_SET && items.is_empty() {
                rt.throw_new("java/lang/IllegalArgumentException");
                return Some(V::NULL);
            }
            let class = if rt.class_of(src) == ENUM_SET { enum_set_type(rt, src) } else { rt.class_of(items[0]).to_owned() };
            new_enum_set(rt, &class, items)
        }
        "clone" => {
            let class = enum_set_type(rt, a(0));
            let items = elements(rt, a(0));
            new_enum_set(rt, &class, items)
        }
        "range" => {
            let class = rt.class_of(a(0)).to_owned();
            let (lo, hi) = (crate::api_common::enum_ordinal(rt, a(0)), crate::api_common::enum_ordinal(rt, a(1)));
            let all = all_constants(rt, &class);
            let within: Vec<V> = all.into_iter().filter(|&e| (lo..=hi).contains(&crate::api_common::enum_ordinal(rt, e))).collect();
            new_enum_set(rt, &class, within)
        }
        "complementOf" => {
            let class = enum_set_type(rt, a(0));
            let have: Vec<i32> = elements(rt, a(0)).into_iter().map(|e| crate::api_common::enum_ordinal(rt, e)).collect();
            let all = all_constants(rt, &class);
            let rest: Vec<V> = all.into_iter().filter(|&e| !have.contains(&crate::api_common::enum_ordinal(rt, e))).collect();
            new_enum_set(rt, &class, rest)
        }
        _ => return None,
    })
}

/// The array class a fastutil collection's `toIntArray`, `toLongArray`... gives.
fn typed_array(name: &str) -> Option<&'static str> {
    Some(match name {
        "toIntArray" => "[I",
        "toLongArray" => "[J",
        "toDoubleArray" => "[D",
        "toFloatArray" => "[F",
        "toShortArray" => "[S",
        "toByteArray" => "[B",
        "toCharArray" => "[C",
        "toBooleanArray" => "[Z",
        _ => return None,
    })
}

/// fastutil's `toIntArray()` (a new array of the elements) and `toIntArray(int[] a)`, which fills `a`
/// when it is long enough and otherwise makes a new one.
fn to_typed_array(rt: &mut Rt, name: &str, items: Vec<V>, into: V) -> V {
    let class = typed_array(name).unwrap_or("[I");
    if into.r() != 0
        && let Some(Data::Array(dst)) = rt.heap.get_mut(into.r() as usize).map(|o| &mut o.data)
        && dst.len() >= items.len()
    {
        dst[..items.len()].copy_from_slice(&items);
        return into;
    }
    rt.alloc(class, Data::Array(items))
}

/// An iterator over a snapshot of `items`, removing from `source` on `remove()`.
fn iter_over(rt: &mut Rt, source: V, items: Vec<V>) -> V {
    let it = new_iter(rt, items);
    if let Some(o) = rt.heap.get_mut(it.r() as usize) {
        o.class = "java/util/Iterator";
    }
    rt.iter_sources.insert(it.r(), source);
    it
}

fn iter_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    let (len, at) = match &rt.heap.get(this.r() as usize)?.data {
        Data::Iter(items, at) => (items.len(), *at),
        _ => return None,
    };
    Some(match name {
        "hasNext" => V::I((at < len) as i32),
        "next" => match &mut rt.heap.get_mut(this.r() as usize)?.data {
            Data::Iter(items, at) => {
                let v = items.get(*at).copied().unwrap_or(V::NULL);
                *at += 1;
                v
            }
            _ => V::NULL,
        },
        "remove" => {
            let last = match &rt.heap.get(this.r() as usize)?.data {
                Data::Iter(items, at) if *at > 0 => items[*at - 1],
                _ => return Some(V::Void),
            };
            if let Some(&src) = rt.iter_sources.get(&this.r()) {
                remove_from(rt, src, last);
            }
            V::Void
        }
        "forEachRemaining" => {
            let f = args.get(1).copied().unwrap_or_default();
            while let Some(Data::Iter(items, at)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data) {
                let Some(&v) = items.get(*at) else { break };
                *at += 1;
                rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", f, &[v]);
            }
            V::Void
        }
        _ => return None,
    })
}

/// Removes `item` from a collection an iterator walks: a list's element, a set's member, a
/// map's key (key sets) or entry (entry sets).
fn remove_from(rt: &mut Rt, src: V, item: V) {
    match rt.heap.get(src.r() as usize).map(|o| (&o.data, o.class)) {
        Some((Data::List(_), _)) => {
            let items = list(rt, src).cloned().unwrap_or_default();
            if let Some(p) = items.iter().position(|&e| e == item) {
                if let Some(l) = list_mut(rt, src) {
                    l.remove(p);
                }
            }
        }
        Some((Data::Table(_), _)) => {
            let key = if is_entry(rt.class_of(item)) {
                list(rt, item)
                    .and_then(|l| l.first().copied())
                    .unwrap_or(item)
            } else {
                item
            };
            table_remove(rt, src, key);
        }
        _ => {}
    }
}

fn entry_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    let kv = list(rt, this)?.clone();
    Some(match name {
        "getKey" => kv[0],
        "getValue" => kv[1],
        // fastutil's typed accessors (`getIntKey`, `getIntValue`, `getLongKey`...).
        n if n.starts_with("get") && n.ends_with("Key") => kv[0],
        n if n.starts_with("get") && n.ends_with("Value") => kv[1],
        "setValue" if rt.class_of(this) == SIMPLE_IMMUTABLE_ENTRY => {
            rt.throw_new("java/lang/UnsupportedOperationException");
            V::NULL
        }
        "setValue" => {
            let v = args.get(1).copied().unwrap_or_default();
            let old = std::mem::replace(&mut list_mut(rt, this)?[1], v);
            if let Some(&m) = rt.iter_sources.get(&this.r()) {
                table_put(rt, m, kv[0], v);
            }
            old
        }
        _ => return None,
    })
}

fn set_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let len = table_len(rt, this);
    Some(match name {
        "size" => V::I(len as i32),
        "isEmpty" => V::I((len == 0) as i32),
        "add" => {
            let fresh = table_get(rt, this, a(1)).is_none();
            if fresh {
                table_put(rt, this, a(1), V::I(1));
            }
            boolean(rt, fresh)
        }
        "addAll" => {
            let mut any = false;
            for e in elements(rt, a(1)) {
                if table_get(rt, this, e).is_none() {
                    table_put(rt, this, e, V::I(1));
                    any = true;
                }
            }
            V::I(any as i32)
        }
        "contains" => {
            let has = table_get(rt, this, a(1)).is_some();
            V::I(has as i32)
        }
        "remove" => {
            let had = table_remove(rt, this, a(1)).is_some();
            V::I(had as i32)
        }
        "removeAll" | "retainAll" => {
            let other = elements(rt, a(1));
            let mine: Vec<V> = table_entries(rt, this).into_iter().map(|(k, _)| k).collect();
            let mut changed = false;
            for k in mine {
                let in_other = other.iter().any(|&o| equals(rt, o, k));
                if in_other == (name == "removeAll") {
                    table_remove(rt, this, k);
                    changed = true;
                }
            }
            V::I(changed as i32)
        }
        "clear" => {
            let empty = emptied(rt, this);
            put_table(rt, this, empty);
            V::Void
        }
        "iterator" | "fastIterator" => {
            let items = elements(rt, this);
            iter_over(rt, this, items)
        }
        "stream" | "parallelStream" => {
            let items = elements(rt, this);
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        "forEach" => {
            for e in elements(rt, this) {
                rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[e]);
            }
            V::Void
        }
        "removeIf" => {
            let mut any = false;
            for e in elements(rt, this) {
                if rt.call_functional("java/util/function/Predicate.test.(Ljava/lang/Object;)Z", a(1), &[e]).i() != 0 {
                    table_remove(rt, this, e);
                    any = true;
                }
            }
            V::I(any as i32)
        }
        "first" | "getFirst" => elements(rt, this).first().copied().unwrap_or(V::NULL),
        "last" | "getLast" => elements(rt, this).last().copied().unwrap_or(V::NULL),
        // A Guava set view's copies.
        "immutableCopy" => {
            let items = elements(rt, this);
            new_set(rt, &items)
        }
        "copyInto" => {
            for e in elements(rt, this) {
                crate::api::call(rt, "java/util/Set.add.(Ljava/lang/Object;)Z", &[a(1), e]);
            }
            a(1)
        }
        "toArray" => {
            let items = elements(rt, this);
            to_array(rt, items, a(1))
        }
        n if typed_array(n).is_some() => {
            let items = elements(rt, this);
            to_typed_array(rt, n, items, a(1))
        }
        "equals" => {
            let (mine, theirs) = (elements(rt, this), elements(rt, a(1)));
            let same = mine.len() == theirs.len() && mine.iter().all(|&m| theirs.iter().any(|&t| equals(rt, m, t)));
            V::I(same as i32)
        }
        "hashCode" => {
            let items = elements(rt, this);
            V::I(items.into_iter().fold(0i32, |h, v| h.wrapping_add(if v.r() == 0 { 0 } else { hash(rt, v) })))
        }
        _ => return None,
    })
}

/// What a map gives for a missing key: null, or for a fastutil map of primitive values its
/// `defaultReturnValue` (zero unless set, kept in an unsorted table's unused comparator slot).
fn missing(rt: &Rt, m: V, desc: &str) -> V {
    let ret = desc.rsplit(')').next().unwrap_or("");
    if ret.starts_with('L') || ret.starts_with('[') {
        return V::NULL;
    }
    if let Some(Data::Table(t)) = rt.heap.get(m.r() as usize).map(|o| &o.data)
        && !t.sorted
        && matches!(t.comparator, V::I(_) | V::J(_) | V::F(_) | V::D(_))
    {
        return t.comparator;
    }
    match ret {
        "J" => V::J(0),
        "F" => V::F(0.0),
        "D" => V::D(0.0),
        "V" => V::Void,
        _ => V::I(0),
    }
}

const RANDOM_MULTIPLIER: i64 = 0x5_DEEC_E66D;
const RANDOM_MASK: i64 = (1 << 48) - 1;

/// `java.util.Random`: the JDK's documented 48-bit linear congruential generator, so a seeded one
/// gives the numbers it gives in Java (`new Random(0)` in Create's depot renderer). Its state is
/// `$seed`; `$gaussian` holds `nextGaussian`'s second value while there is one.
/// `ThreadLocalRandom.current()` is one such generator, seeded from the runtime's stream.
fn random_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let scramble = |seed: i64| (seed ^ RANDOM_MULTIPLIER) & RANDOM_MASK;
    let this = a(0);
    match name {
        "current" => {
            let seed = crate::api_common::next_random(rt) as i64;
            return Some(rt.intern("compat:thread-local-random".into(), |rt| {
                let r = rt.new_object("java/util/Random");
                rt.put_field(r, "$seed", V::J(scramble(seed)));
                r
            }));
        }
        // `new Random()` takes a seed nobody can predict; `new Random(seed)` that seed.
        "<init>" | "setSeed" => {
            let seed = if desc.starts_with("(J)") { a(1).j() } else { crate::api_common::next_random(rt) as i64 };
            rt.put_field(this, "$seed", V::J(scramble(seed)));
            rt.put_field(this, "$gaussian", V::Void);
            return Some(V::Void);
        }
        _ => {}
    }
    let mut state = match rt.get_field(this, "$seed") {
        V::J(s) => s,
        _ => scramble(crate::api_common::next_random(rt) as i64),
    };
    let mut next = |bits: u32| -> i32 {
        state = state.wrapping_mul(RANDOM_MULTIPLIER).wrapping_add(0xB) & RANDOM_MASK;
        (state >> (48 - bits)) as i32
    };
    let next_double = |next: &mut dyn FnMut(u32) -> i32| ((i64::from(next(26)) << 27) + i64::from(next(27))) as f64 * (1.0 / (1i64 << 53) as f64);
    let v = match (name, desc) {
        ("nextInt", "()I") => V::I(next(32)),
        ("nextInt", "(I)I") => {
            let bound = a(1).i();
            if bound <= 0 {
                rt.throw_new("java/lang/IllegalArgumentException");
                return Some(V::I(0));
            }
            if bound & (bound - 1) == 0 {
                V::I(((i64::from(bound) * i64::from(next(31))) >> 31) as i32)
            } else {
                loop {
                    let bits = next(31);
                    let v = bits % bound;
                    // Java's int overflow is the rejection rule.
                    if bits.wrapping_sub(v).wrapping_add(bound - 1) >= 0 {
                        break V::I(v);
                    }
                }
            }
        }
        // Java 17's `RandomGenerator.nextInt(origin, bound)` (`RandomSupport.boundedNextInt`).
        ("nextInt", "(II)I") => {
            let (origin, bound) = (a(1).i(), a(2).i());
            if origin >= bound {
                rt.throw_new("java/lang/IllegalArgumentException");
                return Some(V::I(0));
            }
            let mut r = next(32);
            let n = bound.wrapping_sub(origin);
            let m = n.wrapping_sub(1);
            if n & m == 0 {
                r = (r & m).wrapping_add(origin);
            } else if n > 0 {
                let mut u = ((r as u32) >> 1) as i32;
                loop {
                    r = u % n;
                    if u.wrapping_add(m).wrapping_sub(r) >= 0 {
                        break;
                    }
                    u = ((next(32) as u32) >> 1) as i32;
                }
                r = r.wrapping_add(origin);
            } else {
                while r < origin || r >= bound {
                    r = next(32);
                }
            }
            V::I(r)
        }
        ("nextLong", "()J") => {
            let high = i64::from(next(32)) << 32;
            V::J(high.wrapping_add(i64::from(next(32))))
        }
        ("nextBoolean", _) => V::I((next(1) != 0) as i32),
        ("nextFloat", "()F") => V::F(next(24) as f32 / (1 << 24) as f32),
        ("nextDouble", "()D") => V::D(next_double(&mut next)),
        ("nextGaussian", "()D") => match rt.get_field(this, "$gaussian") {
            V::D(g) => {
                rt.put_field(this, "$gaussian", V::Void);
                V::D(g)
            }
            _ => {
                // The polar method, as `Random.nextGaussian` (StrictMath's log and sqrt).
                let (v1, v2, s) = loop {
                    let v1 = 2.0 * next_double(&mut next) - 1.0;
                    let v2 = 2.0 * next_double(&mut next) - 1.0;
                    let s = v1 * v1 + v2 * v2;
                    if s < 1.0 && s != 0.0 {
                        break (v1, v2, s);
                    }
                };
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                rt.put_field(this, "$gaussian", V::D(v2 * multiplier));
                V::D(v1 * multiplier)
            }
        },
        ("nextBytes", _) => {
            let len = rt.array_len(a(1));
            let mut i = 0;
            while i < len {
                let mut rnd = next(32);
                for _ in 0..(len - i).min(4) {
                    rt.array_set(a(1), i, V::I(i32::from(rnd as i8)));
                    rnd >>= 8;
                    i += 1;
                }
            }
            V::Void
        }
        _ => return None,
    };
    rt.put_field(this, "$seed", V::J(state));
    Some(v)
}

/// Java's null: a null reference, or nothing at all (a call that returned no value). A primitive
/// (a fastutil map's `int` value) never is.
fn is_null(v: V) -> bool {
    matches!(v, V::R(0) | V::Void)
}

/// A map's `values()`: a list of them, in the map's order.
fn map_values(rt: &mut Rt, this: V) -> V {
    let vals: Vec<V> = table_entries(rt, this).into_iter().map(|(_, v)| v).collect();
    new_list(rt, vals)
}

/// The API's intrinsic for `Map.values()` ([`crate::api`]): one of Java's own maps (a `HashMap`, a
/// `TreeMap`) answers as [`call`] answers it, through [`map_call`]; any other receiver (a mod's
/// own map class, Guava's) is left to the modules.
pub(crate) fn plain_map_values(rt: &mut Rt, args: &[V]) -> Option<V> {
    let this = *args.first()?;
    let class = rt.class_of(this);
    let plain = class.starts_with("java/util/") && is_map(class) && matches!(rt.heap.get(this.r() as usize).map(|o| &o.data), Some(Data::Table(_)));
    plain.then(|| map_values(rt, this))
}

/// A list's `forEach(action)`: `action` takes each element in turn, over the elements it had
/// when it began.
fn list_for_each(rt: &mut Rt, this: V, action: V) -> Option<V> {
    for e in list(rt, this)?.clone() {
        rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", action, &[e]);
    }
    Some(V::Void)
}

/// The API's intrinsic for `Collection.forEach(action)` ([`crate::api`]): an `ArrayList` of Java's
/// own (what `new ArrayList()` and `values()` make) answers as [`call`] answers it, through
/// [`list_call`]; any other receiver is left to the modules.
pub(crate) fn plain_list_for_each(rt: &mut Rt, args: &[V]) -> Option<V> {
    let this = *args.first()?;
    if rt.class_of(this) != "java/util/ArrayList" {
        return None;
    }
    list_for_each(rt, this, args.get(1).copied().unwrap_or_default())
}

fn map_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    // A stored value, or what a missing key gives (`missing`).
    let found = |rt: &Rt, v: Option<V>| v.unwrap_or_else(|| missing(rt, this, desc));
    Some(match name {
        "size" => V::I(table_len(rt, this) as i32),
        "isEmpty" => V::I((table_len(rt, this) == 0) as i32),
        // fastutil: what `get` of a missing key gives.
        "defaultReturnValue" if args.len() > 1 => {
            if let Some(Data::Table(t)) = rt.heap.get_mut(this.r() as usize).map(|o| &mut o.data)
                && !t.sorted
            {
                t.comparator = a(1);
            }
            V::Void
        }
        "defaultReturnValue" => missing(rt, this, desc),
        "get" | "getInt" | "getLong" | "getFloat" | "getDouble" | "getBoolean" | "getByte" | "getShort" | "getChar" => {
            let v = table_get(rt, this, a(1));
            found(rt, v)
        }
        "removeInt" | "removeLong" | "removeFloat" | "removeDouble" | "removeBoolean" => {
            let v = table_remove(rt, this, a(1));
            found(rt, v)
        }
        // `Object2IntOpenHashMap.addTo`: adds to the value (the default when missing); the old one.
        "addTo" => {
            let old = table_get(rt, this, a(1)).unwrap_or_else(|| missing(rt, this, desc));
            let new = match (old, a(2)) {
                (V::J(x), V::J(y)) => V::J(x.wrapping_add(y)),
                (V::F(x), V::F(y)) => V::F(x + y),
                (V::D(x), V::D(y)) => V::D(x + y),
                (x, y) => V::I(x.i().wrapping_add(y.i())),
            };
            table_put(rt, this, a(1), new);
            old
        }
        "getOrDefault" => table_get(rt, this, a(1)).unwrap_or(a(2)),
        // A `BiMap`'s other way round: the table kept with it ([`inverse_of`]), made now for one
        // that had none (an immutable one, which was a map until asked).
        "inverse" if args.len() == 1 => match inverse_of(rt, this) {
            Some(inverse) => inverse,
            None => {
                let class = rt.class_of(this);
                let inverse = rt.alloc(class, Data::Table(Table::default()));
                for (k, v) in table_entries(rt, this) {
                    put_in(rt, inverse, v, k);
                }
                pair(rt, this, inverse);
                inverse
            }
        },
        // `BiMap.forcePut`: the value's other key goes, where `put` refuses it.
        "forcePut" => {
            let had = table_get(rt, this, a(1));
            match inverse_of(rt, this) {
                Some(inverse) => put_both(rt, this, inverse, a(1), a(2), true),
                None => table_put(rt, this, a(1), a(2)),
            };
            found(rt, had)
        }
        "containsKey" => V::I(table_get(rt, this, a(1)).is_some() as i32),
        // A `BiMap`'s values are its inverse's keys.
        "containsValue" => match inverse_of(rt, this) {
            Some(inverse) => V::I(get_in(rt, inverse, a(1)).is_some() as i32),
            None => {
                let vals: Vec<V> = table_entries(rt, this)
                    .into_iter()
                    .map(|(_, v)| v)
                    .collect();
                V::I(vals.into_iter().any(|v| equals(rt, v, a(1))) as i32)
            }
        },
        "put" => {
            let had = table_get(rt, this, a(1));
            table_put(rt, this, a(1), a(2));
            found(rt, had)
        }
        // `replace(key, old, new)`: puts `new` only where the key holds `old` (null too, for a key
        // it holds); `replace(key, value)` only for a key it holds, giving the old value.
        "replace" if desc.ends_with(")Z") => {
            let holds = match table_get(rt, this, a(1)) {
                Some(cur) => equals(rt, cur, a(2)) || (is_null(cur) && is_null(a(2))),
                None => false,
            };
            if holds {
                table_put(rt, this, a(1), a(3));
            }
            V::I(holds as i32)
        }
        "replace" => {
            let had = table_get(rt, this, a(1));
            if had.is_some() {
                table_put(rt, this, a(1), a(2));
            }
            found(rt, had)
        }
        "putIfAbsent" => match table_get(rt, this, a(1)) {
            Some(v) if !is_null(v) => v,
            _ => {
                table_put(rt, this, a(1), a(2));
                missing(rt, this, desc)
            }
        },
        "putAll" => {
            for (k, v) in table_entries(rt, a(1)) {
                table_put(rt, this, k, v);
            }
            V::Void
        }
        // `replaceAll(function)`: each key's value what the function makes of the two.
        "replaceAll" => {
            for (k, v) in table_entries(rt, this) {
                let new = rt.call_functional("java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", a(1), &[k, v]);
                if rt.thrown.is_some() {
                    break;
                }
                table_put(rt, this, k, new);
            }
            V::Void
        }
        "remove" => {
            let v = table_remove(rt, this, a(1));
            found(rt, v)
        }
        "clear" => {
            // A `BiMap`'s inverse with it.
            for m in [Some(this), inverse_of(rt, this)].into_iter().flatten() {
                let empty = emptied(rt, m);
                put_table(rt, m, empty);
            }
            V::Void
        }
        // A key mapped to null counts as absent; a primitive map's value (fastutil's
        // `Object2IntMap.computeIfAbsent`) is never null, so it is kept and found again.
        "computeIfAbsent" => match table_get(rt, this, a(1)) {
            Some(v) if !is_null(v) => v,
            _ => {
                let v = rt.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(2), &[a(1)]);
                if !is_null(v) {
                    table_put(rt, this, a(1), v);
                }
                v
            }
        },
        "computeIfPresent" | "compute" | "merge" => {
            let old = table_get(rt, this, a(1));
            let v = match name {
                "computeIfPresent" => match old {
                    Some(o) if !is_null(o) => rt.call_functional("java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", a(2), &[a(1), o]),
                    _ => return Some(V::NULL),
                },
                "compute" => rt.call_functional("java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", a(2), &[a(1), old.unwrap_or(V::NULL)]),
                _ => match old {
                    Some(o) if !is_null(o) => rt.call_functional("java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", a(3), &[o, a(2)]),
                    _ => a(2),
                },
            };
            if is_null(v) {
                table_remove(rt, this, a(1));
            } else {
                table_put(rt, this, a(1), v);
            }
            v
        }
        "keySet" | "navigableKeySet" => {
            let keys: Vec<V> = table_entries(rt, this).into_iter().map(|(k, _)| k).collect();
            let s = new_set(rt, &keys);
            rt.iter_sources.insert(s.r(), this);
            s
        }
        "values" => map_values(rt, this),
        // fastutil's typed entry sets (`int2ObjectEntrySet`, `object2IntEntrySet`...) are its entry set.
        n if n == "entrySet" || (n.ends_with("EntrySet") && n.contains('2')) => {
            let es: Vec<(V, V)> = table_entries(rt, this);
            let mut items = Vec::with_capacity(es.len());
            for (k, v) in es {
                let e = entry(rt, k, v);
                rt.iter_sources.insert(e.r(), this);
                items.push(e);
            }
            let s = identity_set(rt, &items);
            rt.iter_sources.insert(s.r(), this);
            s
        }
        "forEach" => {
            for (k, v) in table_entries(rt, this) {
                rt.call_functional("java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V", a(1), &[k, v]);
            }
            V::Void
        }
        "firstKey" | "lastKey" | "firstEntry" | "lastEntry" => {
            let es = table_entries(rt, this);
            let e = if name.starts_with("first") { es.first() } else { es.last() };
            match (e, name.ends_with("Key")) {
                (Some((k, _)), true) => *k,
                (Some((k, v)), false) => entry(rt, *k, *v),
                (None, _) => V::NULL,
            }
        }
        "equals" => {
            let (mine, theirs) = (table_entries(rt, this), table_entries(rt, a(1)));
            let mut same = mine.len() == theirs.len();
            for (k, v) in mine {
                if !same {
                    break;
                }
                same = match table_get(rt, a(1), k) {
                    Some(o) => equals(rt, o, v),
                    None => false,
                };
            }
            V::I(same as i32)
        }
        "hashCode" => {
            let es = table_entries(rt, this);
            V::I(es.into_iter().fold(0i32, |h, (k, v)| h.wrapping_add(hash(rt, k) ^ if v.r() == 0 { 0 } else { hash(rt, v) })))
        }
        _ => return None,
    })
}

/// A row's map in a Guava table, made on first use (a live view, as Guava's `row`).
fn row(rt: &mut Rt, t: V, r: V) -> V {
    if let Some(m) = table_get(rt, t, r) {
        return m;
    }
    let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
    table_put(rt, t, r, m);
    m
}

/// A cell of a Guava table: its row's key, its column's and its value.
fn table_cell(rt: &mut Rt, row: V, column: V, value: V) -> V {
    let c = rt.new_object(TABLE_CELL);
    for (field, v) in [("$row", row), ("$column", column), ("$value", value)] {
        rt.put_field(c, field, v);
    }
    c
}

const TABLE_CELL: &str = "com/google/common/collect/Tables$ImmutableCell";

fn guava_table_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
        // Every cell, row by row.
        "cellSet" => {
            let mut cells = Vec::new();
            for (r, m) in table_entries(rt, this) {
                for (c, v) in table_entries(rt, m) {
                    cells.push(table_cell(rt, r, c, v));
                }
            }
            identity_set(rt, &cells)
        }
        "columnKeySet" => {
            let mut keys: Vec<V> = Vec::new();
            for (_, m) in table_entries(rt, this) {
                for (c, _) in table_entries(rt, m) {
                    if !keys.iter().any(|&k| equals(rt, k, c)) {
                        keys.push(c);
                    }
                }
            }
            new_set(rt, &keys)
        }
        "containsColumn" => {
            let rows = table_entries(rt, this);
            V::I(
                rows.into_iter()
                    .any(|(_, m)| table_get(rt, m, a(1)).is_some()) as i32,
            )
        }
        "containsValue" => {
            let mut hit = false;
            for (_, m) in table_entries(rt, this) {
                for (_, v) in table_entries(rt, m) {
                    hit = hit || equals(rt, v, a(1));
                }
            }
            V::I(hit as i32)
        }
        "putAll" => {
            for (r, m) in table_entries(rt, a(1)) {
                let mine = row(rt, this, r);
                for (c, v) in table_entries(rt, m) {
                    table_put(rt, mine, c, v);
                }
            }
            V::Void
        }
        "put" => {
            let m = row(rt, this, a(1));
            table_put(rt, m, a(2), a(3))
        }
        "get" => match table_get(rt, this, a(1)) {
            Some(m) => table_get(rt, m, a(2)).unwrap_or(V::NULL),
            None => V::NULL,
        },
        "contains" => {
            let hit = table_get(rt, this, a(1)).is_some_and(|m| table_get(rt, m, a(2)).is_some());
            V::I(hit as i32)
        }
        "containsRow" => V::I(table_get(rt, this, a(1)).is_some() as i32),
        "row" => row(rt, this, a(1)),
        "column" => {
            let out = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
            for (r, m) in table_entries(rt, this) {
                if let Some(v) = table_get(rt, m, a(1)) {
                    table_put(rt, out, r, v);
                }
            }
            out
        }
        "rowKeySet" => {
            let keys: Vec<V> = table_entries(rt, this).into_iter().map(|(k, _)| k).collect();
            new_set(rt, &keys)
        }
        "rowMap" => this_as_map(rt, this),
        "values" => {
            let mut out = Vec::new();
            for (_, m) in table_entries(rt, this) {
                out.extend(table_entries(rt, m).into_iter().map(|(_, v)| v));
            }
            new_list(rt, out)
        }
        "size" => {
            let rows = table_entries(rt, this);
            V::I(rows.iter().map(|(_, m)| table_entries(rt, *m).len() as i32).sum())
        }
        "isEmpty" => {
            let rows = table_entries(rt, this);
            V::I(rows.iter().all(|(_, m)| table_entries(rt, *m).is_empty()) as i32)
        }
        "clear" => {
            put_table(rt, this, Table::default());
            V::Void
        }
        "remove" => match table_get(rt, this, a(1)) {
            Some(m) => table_remove(rt, m, a(2)).unwrap_or(V::NULL),
            None => V::NULL,
        },
        _ => return None,
    })
}

/// `ImmutableList.Builder` and its kin: `add`/`addAll`/`put`/`putAll` fill it and return it,
/// `build` makes the collection (a set's builder drops repeats; a map's keeps the last value).
fn immutable_builder_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let map = owner.ends_with("Map$Builder");
    Some(match name {
        "<init>" => {
            let data = if map { Data::Table(Table::default()) } else { Data::List(Vec::new()) };
            if let Some(o) = rt.heap.get_mut(this.r() as usize) {
                o.data = data;
            }
            V::Void
        }
        "add" => {
            let items = if desc.starts_with("([") { elements(rt, a(1)) } else { vec![a(1)] };
            list_mut(rt, this)?.extend(items);
            this
        }
        "addAll" => {
            let items = match rt.heap.get(a(1).r() as usize).map(|o| &o.data) {
                Some(Data::Iter(..)) => {
                    let rest = elements(rt, a(1));
                    if let Some(Data::Iter(items, at)) = rt.heap.get_mut(a(1).r() as usize).map(|o| &mut o.data) {
                        *at = items.len();
                    }
                    rest
                }
                _ => elements(rt, a(1)),
            };
            list_mut(rt, this)?.extend(items);
            this
        }
        "put" if map => {
            if desc.starts_with("(Ljava/util/Map$Entry;") {
                let kv = elements(rt, a(1));
                table_put(rt, this, kv.first().copied().unwrap_or_default(), kv.get(1).copied().unwrap_or_default());
            } else {
                table_put(rt, this, a(1), a(2));
            }
            this
        }
        "putAll" if map => {
            for (k, v) in table_entries(rt, a(1)) {
                table_put(rt, this, k, v);
            }
            this
        }
        "orderEntriesByValue" => this,
        "build" | "buildOrThrow" | "buildKeepingLast" => {
            if map {
                let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
                for (k, v) in table_entries(rt, this) {
                    table_put(rt, m, k, v);
                }
                m
            } else {
                let items = elements(rt, this);
                if owner.contains("Set") { new_set(rt, &items) } else { new_list(rt, items) }
            }
        }
        _ => return None,
    })
}

/// A plain map with the same entries (Guava's `asMap`/`rowMap`): values are the live collections.
fn this_as_map(rt: &mut Rt, this: V) -> V {
    let out = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
    for (k, v) in table_entries(rt, this) {
        table_put(rt, out, k, v);
    }
    out
}

fn multimap_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let set_values = rt.class_of(this).contains("HashMultimap") || rt.class_of(this).contains("SetMultimap");
    let values = |rt: &mut Rt, k: V, make: bool| -> Option<V> {
        if let Some(c) = table_get(rt, this, k) {
            return Some(c);
        }
        if !make {
            return None;
        }
        let c = if set_values { new_set(rt, &[]) } else { new_list(rt, Vec::new()) };
        table_put(rt, this, k, c);
        Some(c)
    };
    Some(match name {
        "put" => {
            let c = values(rt, a(1), true)?;
            if set_values {
                let fresh = table_get(rt, c, a(2)).is_none();
                if fresh {
                    table_put(rt, c, a(2), V::I(1));
                }
                V::I(fresh as i32)
            } else {
                list_mut(rt, c)?.push(a(2));
                V::I(1)
            }
        }
        "putAll" => {
            let c = values(rt, a(1), true)?;
            for v in elements(rt, a(2)) {
                if set_values {
                    table_put(rt, c, v, V::I(1));
                } else {
                    list_mut(rt, c)?.push(v);
                }
            }
            V::I(1)
        }
        // Guava's `get` is a live view, empty when absent.
        "get" => values(rt, a(1), true)?,
        "removeAll" => match table_remove(rt, this, a(1)) {
            Some(c) => c,
            None => new_list(rt, Vec::new()),
        },
        "remove" => match values(rt, a(1), false) {
            Some(c) => {
                let items = elements(rt, c);
                let pos = items.iter().position(|&e| equals(rt, e, a(2)));
                if let Some(p) = pos {
                    if set_values {
                        table_remove(rt, c, items[p]);
                    } else {
                        list_mut(rt, c)?.remove(p);
                    }
                }
                V::I(pos.is_some() as i32)
            }
            None => V::I(0),
        },
        "containsKey" => {
            let hit = values(rt, a(1), false).is_some_and(|c| !elements(rt, c).is_empty());
            V::I(hit as i32)
        }
        "containsEntry" => {
            let hit = match values(rt, a(1), false) {
                Some(c) => elements(rt, c).into_iter().any(|e| equals(rt, e, a(2))),
                None => false,
            };
            V::I(hit as i32)
        }
        "size" => {
            let total: usize = table_entries(rt, this).into_iter().map(|(_, c)| elements(rt, c).len()).sum();
            V::I(total as i32)
        }
        "isEmpty" => {
            let empty = table_entries(rt, this).into_iter().all(|(_, c)| elements(rt, c).is_empty());
            V::I(empty as i32)
        }
        "clear" => {
            put_table(rt, this, Table::default());
            V::Void
        }
        "asMap" => this_as_map(rt, this),
        "keySet" => {
            let keys: Vec<V> = table_entries(rt, this).into_iter().filter(|(_, c)| !elements(rt, *c).is_empty()).map(|(k, _)| k).collect();
            new_set(rt, &keys)
        }
        "values" => {
            let mut out = Vec::new();
            for (_, c) in table_entries(rt, this) {
                out.extend(elements(rt, c));
            }
            new_list(rt, out)
        }
        "entries" => {
            let mut out = Vec::new();
            for (k, c) in table_entries(rt, this) {
                for v in elements(rt, c) {
                    out.push(entry(rt, k, v));
                }
            }
            new_list(rt, out)
        }
        "forEach" => {
            for (k, c) in table_entries(rt, this) {
                for v in elements(rt, c) {
                    rt.call_functional("java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V", a(1), &[k, v]);
                }
            }
            V::Void
        }
        _ => return None,
    })
}

/// A whole number a stream holds (an int or a long), as a long.
fn whole(v: V) -> i64 {
    match v {
        V::J(j) => j,
        v => i64::from(v.i()),
    }
}

/// A number a stream holds, as a double.
fn real(v: V) -> f64 {
    match v {
        V::I(x) => f64::from(x),
        V::J(x) => x as f64,
        V::F(x) => f64::from(x),
        V::D(x) => x,
        _ => 0.0,
    }
}

fn stream_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let items = match &rt.heap.get(this.r() as usize)?.data {
        Data::Stream(s) => s.clone(),
        _ => return None,
    };
    let stream = |rt: &mut Rt, items: Vec<V>| rt.alloc("java/util/stream/Stream", Data::Stream(items));
    const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
    const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
    let class = rt.class_of(this);
    Some(match name {
        // A primitive stream's own kind follows (`mapToInt` makes an `IntStream`, its `map`
        // keeps one).
        "map" | "mapToObj" | "mapToInt" | "mapToLong" | "mapToDouble" => {
            let mut out = Vec::with_capacity(items.len());
            for e in items {
                out.push(rt.call_functional(APPLY, a(1), &[e]));
            }
            let kind = match name {
                "mapToInt" => INT_STREAM,
                "mapToLong" => LONG_STREAM,
                "mapToDouble" => DOUBLE_STREAM,
                "map" if matches!(class, INT_STREAM | LONG_STREAM | DOUBLE_STREAM) => class,
                _ => STREAM,
            };
            rt.alloc(kind, Data::Stream(out))
        }
        "asLongStream" => rt.alloc(
            LONG_STREAM,
            Data::Stream(items.iter().map(|v| V::J(whole(*v))).collect()),
        ),
        "asDoubleStream" => rt.alloc(
            DOUBLE_STREAM,
            Data::Stream(items.iter().map(|v| V::D(real(*v))).collect()),
        ),
        // A primitive stream's `max()` and `min()` (an `OptionalInt`... of the first of equal
        // ones; doubles as `Math.max` takes them: NaN wins, 0.0 over -0.0) and `average()` (an
        // `OptionalDouble`; whole numbers summed exactly first).
        "max" | "min" | "average" if args.len() == 1 => {
            let optional = match class {
                LONG_STREAM if name != "average" => "java/util/OptionalLong",
                DOUBLE_STREAM => "java/util/OptionalDouble",
                _ if name == "average" => "java/util/OptionalDouble",
                _ => "java/util/OptionalInt",
            };
            let max = name == "max";
            let best = if name == "average" {
                let n = items.len() as f64;
                (!items.is_empty()).then(|| {
                    V::D(if class == DOUBLE_STREAM {
                        items.iter().map(|&v| real(v)).sum::<f64>() / n
                    } else {
                        items.iter().map(|&v| i128::from(whole(v))).sum::<i128>() as f64 / n
                    })
                })
            } else {
                items.iter().copied().reduce(|b, e| {
                    let better = match (e, b) {
                        (V::D(x), V::D(y)) => {
                            let wins = if max { x > y || (x == 0.0 && y == 0.0 && y.is_sign_negative()) } else { x < y || (x == 0.0 && y == 0.0 && x.is_sign_negative()) };
                            (x.is_nan() && !y.is_nan()) || wins
                        }
                        (x, y) => if max { whole(x) > whole(y) } else { whole(x) < whole(y) },
                    };
                    if better { e } else { b }
                })
            };
            crate::menus::optional_number(rt, optional, best)
        }
        "flatMap" => {
            let mut out = Vec::new();
            for e in items {
                let s = rt.call_functional(APPLY, a(1), &[e]);
                out.extend(match &rt.heap.get(s.r() as usize).map(|o| &o.data) {
                    Some(Data::Stream(v)) => v.clone(),
                    _ => elements(rt, s),
                });
            }
            stream(rt, out)
        }
        "filter" => {
            let mut out = Vec::new();
            for e in items {
                if rt.call_functional(TEST, a(1), &[e]).i() != 0 {
                    out.push(e);
                }
            }
            stream(rt, out)
        }
        "peek" => {
            for &e in &items {
                rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[e]);
            }
            stream(rt, items)
        }
        "distinct" => {
            let mut out: Vec<V> = Vec::new();
            for e in items {
                if !out.iter().any(|&o| equals(rt, o, e)) {
                    out.push(e);
                }
            }
            stream(rt, out)
        }
        "sorted" => {
            let l = new_list(rt, items);
            sort(rt, l, args.get(1).copied().unwrap_or(V::NULL));
            let sorted = list(rt, l)?.clone();
            stream(rt, sorted)
        }
        "limit" => stream(rt, items.into_iter().take(a(1).j().max(0) as usize).collect()),
        "skip" => stream(rt, items.into_iter().skip(a(1).j().max(0) as usize).collect()),
        "boxed" => rt.alloc(STREAM, Data::Stream(items)),
        "sequential" | "parallel" | "unordered" | "onClose" => this,
        // A stream here holds its elements, nothing to release (`Files.list` in try-with-resources).
        "close" => V::Void,
        "forEach" | "forEachOrdered" => {
            for e in items {
                rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[e]);
            }
            V::Void
        }
        "toList" => new_list(rt, items),
        // A primitive stream's array is of its primitive (`LongStream.toArray()` is a `long[]`).
        "toArray" => {
            let class = match rt.class_of(this) {
                "java/util/stream/IntStream" => "[I",
                "java/util/stream/LongStream" => "[J",
                "java/util/stream/DoubleStream" => "[D",
                _ => "[Ljava/lang/Object;",
            };
            rt.alloc(class, Data::Array(items))
        }
        "collect" if args.len() > 2 => {
            let class = rt.class_of(this);
            crate::collectors::collect_into(rt, class, a(1), a(2), items)
        }
        "collect" => crate::collectors::collect(rt, a(1), items),
        "count" => V::J(items.len() as i64),
        "anyMatch" | "allMatch" | "noneMatch" => {
            let mut hit = false;
            for e in items {
                let t = rt.call_functional(TEST, a(1), &[e]).i() != 0;
                if (name == "allMatch") != t {
                    hit = true;
                    break;
                }
            }
            V::I(match name {
                "anyMatch" => hit,
                _ => !hit,
            } as i32)
        }
        "findFirst" | "findAny" => {
            let o = rt.new_object("java/util/Optional");
            rt.put_field(o, "value", items.first().copied().unwrap_or(V::NULL));
            o
        }
        // `reduce(BinaryOperator.maxBy(c))`: of equal ones, the first.
        "max" | "min" if args.len() > 1 => {
            let mut best: Option<V> = None;
            for e in items {
                best = Some(match best {
                    None => e,
                    Some(b) => {
                        let c = compare(rt, a(1), b, e);
                        if rt.thrown.is_some() {
                            return Some(V::NULL);
                        }
                        if (name == "max" && c >= 0) || (name == "min" && c <= 0) { b } else { e }
                    }
                });
            }
            let o = rt.new_object("java/util/Optional");
            rt.put_field(o, "value", best.unwrap_or(V::NULL));
            o
        }
        "iterator" => new_iter(rt, items),
        // `reduce(op)` (an optional of the fold), `reduce(identity, op)` and
        // `reduce(identity, accumulator, combiner)` (the fold from the identity), left to right.
        "reduce" => {
            let class = rt.class_of(this);
            let op_key = match class {
                "java/util/stream/IntStream" => "java/util/function/IntBinaryOperator.applyAsInt.(II)I",
                "java/util/stream/LongStream" => "java/util/function/LongBinaryOperator.applyAsLong.(JJ)J",
                "java/util/stream/DoubleStream" => "java/util/function/DoubleBinaryOperator.applyAsDouble.(DD)D",
                _ => "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            };
            let (mut acc, op, mut rest) = if args.len() > 2 { (Some(a(1)), a(2), items.into_iter()) } else { (None, a(1), items.into_iter()) };
            if acc.is_none() {
                acc = rest.next();
            }
            let mut acc_v = acc;
            for e in rest {
                let prev = acc_v.unwrap_or_default();
                acc_v = Some(rt.call_functional(op_key, op, &[prev, e]));
                if rt.thrown.is_some() {
                    return Some(V::NULL);
                }
            }
            if args.len() > 2 {
                acc_v.unwrap_or_default()
            } else if class == "java/util/stream/IntStream" {
                crate::menus::optional_int(rt, acc_v.map(|v| v.i()))
            } else {
                let o = rt.new_object("java/util/Optional");
                rt.put_field(o, "value", acc_v.unwrap_or(V::NULL));
                o
            }
        }
        "sum" if items.is_empty() => match class {
            LONG_STREAM => V::J(0),
            DOUBLE_STREAM => V::D(0.0),
            _ => V::I(0),
        },
        "sum" => items.iter().fold(V::I(0), |acc, v| match (acc, *v) {
            (V::I(x), V::I(y)) => V::I(x.wrapping_add(y)),
            (V::I(x), V::J(y)) | (V::J(y), V::I(x)) => V::J(y.wrapping_add(x as i64)),
            (V::J(x), V::J(y)) => V::J(x.wrapping_add(y)),
            (V::I(x), V::D(y)) => V::D(x as f64 + y),
            (V::D(x), V::D(y)) => V::D(x + y),
            (acc, _) => acc,
        }),
        _ => {
            let _ = int_box;
            return None;
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn properties_load_and_store_their_file() {
        let mut rt = Rt::new();
        let c = crate::api::call;
        let p = rt.new_object("java/util/Properties");
        c(&mut rt, "java/util/Properties.<init>.()V", &[p]);
        let file = "# a comment\n! another\nspeed = 7\nname:fast\\ one\npath=C\\:\\\\mods\nlong = a,\\\n   b\nempty\n\\u0444=\\u044f\n";
        let reader = rt.alloc(
            "java/io/StringReader",
            crate::Data::Bytes(file.as_bytes().to_vec(), 0),
        );
        c(
            &mut rt,
            "java/util/Properties.load.(Ljava/io/Reader;)V",
            &[p, reader],
        );
        let get = |rt: &mut Rt, k: &str| {
            let k = rt.string(k);
            let v = c(
                rt,
                "java/util/Properties.getProperty.(Ljava/lang/String;)Ljava/lang/String;",
                &[p, k],
            );
            rt.str_of(v).map(str::to_owned)
        };
        assert_eq!(get(&mut rt, "speed").as_deref(), Some("7"));
        assert_eq!(get(&mut rt, "name").as_deref(), Some("fast one"));
        assert_eq!(get(&mut rt, "path").as_deref(), Some("C:\\mods"));
        assert_eq!(get(&mut rt, "long").as_deref(), Some("a,b"));
        assert_eq!(get(&mut rt, "empty").as_deref(), Some(""));
        assert_eq!(get(&mut rt, "\u{444}").as_deref(), Some("\u{44f}"));
        assert_eq!(get(&mut rt, "absent"), None);
        let (absent, fallback) = (rt.string("absent"), rt.string("dflt"));
        assert_eq!(
            c(
                &mut rt,
                "java/util/Properties.getProperty.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[p, absent, fallback]
            ),
            fallback
        );
        // A map's own methods are there too, and `store` writes what `load` reads back.
        assert_eq!(c(&mut rt, "java/util/Properties.size.()I", &[p]), V::I(6));
        let (k, v) = (rt.string("a b"), rt.string(" x=y"));
        c(
            &mut rt,
            "java/util/Properties.setProperty.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;",
            &[p, k, v],
        );
        let (out, title) = (
            rt.new_object("java/io/StringWriter"),
            rt.string("made by a mod"),
        );
        c(&mut rt, "java/io/StringWriter.<init>.()V", &[out]);
        c(
            &mut rt,
            "java/util/Properties.store.(Ljava/io/Writer;Ljava/lang/String;)V",
            &[p, out, title],
        );
        let written = c(
            &mut rt,
            "java/io/StringWriter.toString.()Ljava/lang/String;",
            &[out],
        );
        let written = rt.str_of(written).unwrap().to_owned();
        assert!(
            written.starts_with("#made by a mod\n")
                && written.contains("a\\ b=\\ x\\=y\n")
                && written.contains("\\u0444=\\u044F\n"),
            "{written}"
        );
        let again: std::collections::BTreeMap<String, String> =
            parse_properties(&written).into_iter().collect();
        assert_eq!(again.get("a b").map(String::as_str), Some(" x=y"));
        assert_eq!(again.len(), 7);
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    #[test]
    fn to_array_fills_the_array_given_when_it_is_long_enough() {
        let mut rt = Rt::new();
        let c = crate::api::call;
        let (a, b) = (rt.string("a"), rt.string("b"));
        let list = new_list(&mut rt, vec![a, b]);
        const TO_ARRAY: &str = "java/util/List.toArray.([Ljava/lang/Object;)[Ljava/lang/Object;";
        // Longer: the same array, filled, a null after the last, the rest as it was.
        let given = rt.new_array(4, b);
        assert_eq!(c(&mut rt, TO_ARRAY, &[list, given]), given);
        assert_eq!(elements(&rt, given), [a, b, V::NULL, b]);
        // Too short: a new one of the list's size.
        let short = rt.new_array(0, V::NULL);
        let made = c(&mut rt, TO_ARRAY, &[list, short]);
        assert!(made != short && elements(&rt, made) == [a, b]);
        let set = new_set_of(&mut rt, &[a, b]);
        let given = rt.new_array(2, V::NULL);
        assert_eq!(
            c(
                &mut rt,
                "java/util/Set.toArray.([Ljava/lang/Object;)[Ljava/lang/Object;",
                &[set, given]
            ),
            given
        );
        assert_eq!(elements(&rt, given), [a, b]);
        let plain = c(
            &mut rt,
            "java/util/List.toArray.()[Ljava/lang/Object;",
            &[list],
        );
        assert_eq!(elements(&rt, plain), [a, b]);
    }

    #[test]
    fn a_joiner_joins_between_its_prefix_and_suffix() {
        let mut rt = Rt::new();
        let c = crate::api::call;
        let (j, comma, open, close) = (
            rt.new_object("java/util/StringJoiner"),
            rt.string(", "),
            rt.string("["),
            rt.string("]"),
        );
        c(
            &mut rt,
            "java/util/StringJoiner.<init>.(Ljava/lang/CharSequence;Ljava/lang/CharSequence;Ljava/lang/CharSequence;)V",
            &[j, comma, open, close],
        );
        let text = |rt: &mut Rt, j: V| {
            let s = c(
                rt,
                "java/util/StringJoiner.toString.()Ljava/lang/String;",
                &[j],
            );
            rt.str_of(s).unwrap_or("?").to_owned()
        };
        assert_eq!(text(&mut rt, j), "[]");
        for piece in ["a", "b", "c"] {
            let p = rt.string(piece);
            assert_eq!(
                c(
                    &mut rt,
                    "java/util/StringJoiner.add.(Ljava/lang/CharSequence;)Ljava/util/StringJoiner;",
                    &[j, p]
                ),
                j
            );
        }
        assert_eq!(text(&mut rt, j), "[a, b, c]");
        assert_eq!(
            c(&mut rt, "java/util/StringJoiner.length.()I", &[j]),
            V::I(9)
        );
        let (plain, none) = (rt.new_object("java/util/StringJoiner"), rt.string("none"));
        c(
            &mut rt,
            "java/util/StringJoiner.<init>.(Ljava/lang/CharSequence;)V",
            &[plain, comma],
        );
        c(
            &mut rt,
            "java/util/StringJoiner.setEmptyValue.(Ljava/lang/CharSequence;)Ljava/util/StringJoiner;",
            &[plain, none],
        );
        assert_eq!(text(&mut rt, plain), "none");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Stream.reduce` in its three forms, left to right (`(a - b)` shows the order); an empty
    /// stream's `reduce(op)` is empty. Guava's factories: `Sets.newLinkedHashSet()` empty, then
    /// growing in order (FarmersDelight's), `Maps.newHashMap(map)` a copy, `Lists.newArrayList`
    /// sized empty.
    #[test]
    fn streams_reduce_and_guavas_factories_make_their_collections() {
        fn vcall(rt: &mut Rt, _key: &'static str, _short: &'static str, args: &[V]) -> V {
            // The operator `(a, b) -> a - b`, a mod's own object here.
            let (x, y) = (args[1], args[2]);
            match (x, y) {
                (V::I(x), V::I(y)) => V::I(x - y),
                _ => {
                    let (x, y) = (rt.get_field(x, "value").i(), rt.get_field(y, "value").i());
                    crate::api_common::integer(rt, x - y)
                }
            }
        }
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"));
        let minus = rt.new_object("test/Minus");
        let ints: Vec<V> = [10, 3, 2].iter().map(|&i| crate::api_common::integer(&mut rt, i)).collect();
        let s = rt.alloc("java/util/stream/Stream", Data::Stream(ints.clone()));
        let r = c(&mut rt, "java/util/stream/Stream.reduce.(Ljava/util/function/BinaryOperator;)Ljava/util/Optional;", &[s, minus]);
        assert_eq!(rt.get_field(rt.get_field(r, "value"), "value"), V::I(5));
        let s = rt.alloc("java/util/stream/IntStream", Data::Stream(vec![V::I(10), V::I(3), V::I(2)]));
        assert_eq!(c(&mut rt, "java/util/stream/IntStream.reduce.(ILjava/util/function/IntBinaryOperator;)I", &[s, V::I(100), minus]), V::I(85));
        let empty = rt.alloc("java/util/stream/Stream", Data::Stream(Vec::new()));
        let r = c(&mut rt, "java/util/stream/Stream.reduce.(Ljava/util/function/BinaryOperator;)Ljava/util/Optional;", &[empty, minus]);
        assert_eq!(rt.get_field(r, "value"), V::NULL);

        let set = c(&mut rt, "com/google/common/collect/Sets.newLinkedHashSet.()Ljava/util/LinkedHashSet;", &[]);
        for &i in ints.iter().rev() {
            c(&mut rt, "java/util/LinkedHashSet.add.(Ljava/lang/Object;)Z", &[set, i]);
        }
        let rev: Vec<V> = ints.iter().rev().copied().collect();
        assert_eq!(elements(&rt, set), rev);
        let m = c(&mut rt, "com/google/common/collect/Maps.newHashMap.()Ljava/util/HashMap;", &[]);
        table_put(&mut rt, m, ints[0], ints[1]);
        let copy = c(&mut rt, "com/google/common/collect/Maps.newHashMap.(Ljava/util/Map;)Ljava/util/HashMap;", &[m]);
        assert_ne!(copy, m);
        assert_eq!(table_get(&mut rt, copy, ints[0]), Some(ints[1]));
        // A queue from Guava's factory is the runtime's queue: what is offered is polled, in order.
        let queue = c(
            &mut rt,
            "com/google/common/collect/Queues.newConcurrentLinkedQueue.()Ljava/util/concurrent/ConcurrentLinkedQueue;",
            &[],
        );
        assert_eq!(
            rt.class_of(queue),
            "java/util/concurrent/ConcurrentLinkedQueue"
        );
        assert_eq!(c(&mut rt, "java/util/Queue.isEmpty.()Z", &[queue]), V::I(1));
        let first = rt.string("first");
        c(
            &mut rt,
            "java/util/Queue.offer.(Ljava/lang/Object;)Z",
            &[queue, first],
        );
        assert_eq!(c(&mut rt, "java/util/Queue.isEmpty.()Z", &[queue]), V::I(0));
        assert_eq!(
            c(
                &mut rt,
                "java/util/Queue.poll.()Ljava/lang/Object;",
                &[queue]
            ),
            first
        );
        assert_eq!(c(&mut rt, "java/util/Queue.isEmpty.()Z", &[queue]), V::I(1));
        let sized = c(&mut rt, "com/google/common/collect/Lists.newArrayListWithCapacity.(I)Ljava/util/ArrayList;", &[V::I(8)]);
        assert!(elements(&rt, sized).is_empty());
    }
    /// Guava's strings as SpectreLib's configs use them: a comment array grown by
    /// `ObjectArrays.concat`, a path split and joined again.
    #[test]
    fn guava_splits_joins_and_concatenates_as_guava_does() {
        let mut rt = Rt::new();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"));
        let first = rt.string("a comment");
        let arr = rt.new_array(1, V::NULL);
        rt.array_set(arr, 0, first);
        let more = rt.string("more");
        let grown = c(&mut rt, "com/google/common/collect/ObjectArrays.concat.([Ljava/lang/Object;Ljava/lang/Object;)[Ljava/lang/Object;", &[arr, more]);
        assert_eq!(rt.array_len(grown), 2);
        assert_eq!(rt.str_of(rt.array_get(grown, 1)), Some("more"));
        let dot = rt.string(".");
        let splitter = c(&mut rt, "com/google/common/base/Splitter.on.(Ljava/lang/String;)Lcom/google/common/base/Splitter;", &[dot]);
        let omit = c(&mut rt, "com/google/common/base/Splitter.omitEmptyStrings.()Lcom/google/common/base/Splitter;", &[splitter]);
        let path = rt.string("server..beds. sleep");
        let parts = c(&mut rt, "com/google/common/base/Splitter.split.(Ljava/lang/CharSequence;)Ljava/lang/Iterable;", &[omit, path]);
        let texts: Vec<String> = elements(&rt, parts).iter().map(|&p| rt.str_of(p).unwrap_or("").to_owned()).collect();
        assert_eq!(texts, ["server", "beds", " sleep"]);
        // The first splitter is unchanged by the second's setting.
        let all = c(&mut rt, "com/google/common/base/Splitter.split.(Ljava/lang/CharSequence;)Ljava/lang/Iterable;", &[splitter, path]);
        assert_eq!(elements(&rt, all).len(), 4);
        let joiner = c(&mut rt, "com/google/common/base/Joiner.on.(C)Lcom/google/common/base/Joiner;", &[V::I('/' as i32)]);
        let joined = c(&mut rt, "com/google/common/base/Joiner.join.(Ljava/lang/Iterable;)Ljava/lang/String;", &[joiner, parts]);
        assert_eq!(rt.str_of(joined), Some("server/beds/ sleep"));
    }

    use super::*;

    /// A mod's own subclass of a JDK map is that map (double-brace initialisation, `new
    /// HashMap<>() {{ put(k, v); }}`, makes one): what its constructor puts through its own
    /// class's name, through the map's class or through the interface is in it, whichever of
    /// the JDK's maps it extends.
    #[test]
    fn a_mods_subclass_of_a_map_is_that_map() {
        crate::install();
        for (n, above) in [
            "java/util/HashMap",
            "java/util/LinkedHashMap",
            "java/util/IdentityHashMap",
            "java/util/concurrent/ConcurrentHashMap",
            "java/util/TreeMap",
            "it/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap",
        ]
        .into_iter()
        .enumerate()
        {
            let mut rt = Rt::new();
            rt.supers.insert("m/Names$1", above);
            let made = rt.new_object_id("m/Names$1", 7 + n as u32);
            let init: &'static str = rt.static_key(&format!("{above}.<init>.()V"));
            crate::api::call(&mut rt, init, &[made]);
            let (k, v, k2, v2) = (
                rt.string("a"),
                rt.string("1"),
                rt.string("b"),
                rt.string("2"),
            );
            let own: &'static str = rt.static_key(&format!(
                "{above}.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
            ));
            crate::api::call(&mut rt, own, &[made, k, v]);
            crate::api::call(
                &mut rt,
                "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[made, k2, v2],
            );
            assert!(rt.unmapped.is_empty(), "{above}: {:?}", rt.unmapped);
            let got = crate::api::call(
                &mut rt,
                "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[made, k],
            );
            assert_eq!(rt.str_of(got), Some("1"), "{above}");
            assert_eq!(
                crate::api::call(&mut rt, "java/util/Map.size.()I", &[made]),
                V::I(2),
                "{above}"
            );
            assert_eq!(rt.class_of(made), "m/Names$1");
        }
    }

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"))
    }

    #[test]
    fn streams_join_and_collect_to_maps_as_java_does() {
        let mut rt = Rt::new();
        let words: Vec<V> = ["a", "b", "c"].iter().map(|w| rt.string(w)).collect();
        let stream = rt.alloc("java/util/stream/Stream", Data::Stream(words.clone()));
        let (sep, pre, post) = (rt.string(", "), rt.string("["), rt.string("]"));
        let joining = c(&mut rt, "java/util/stream/Collectors.joining.(Ljava/lang/CharSequence;Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/util/stream/Collector;", &[sep, pre, post]);
        let out = c(&mut rt, "java/util/stream/Stream.collect.(Ljava/util/stream/Collector;)Ljava/lang/Object;", &[stream, joining]);
        assert_eq!(rt.str_of(out), Some("[a, b, c]"));
        // `toMap(identity, identity)`: each word to itself; a key twice without a merge throws. The
        // engine's dispatch applies `identity` (a runtime function), as the mapping answers it.
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let id = crate::api::call(&mut rt, "java/util/function/Function.identity.()Ljava/util/function/Function;", &[]);
        let to_map = c(&mut rt, "java/util/stream/Collectors.toMap.(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;", &[id, id]);
        let stream = rt.alloc("java/util/stream/Stream", Data::Stream(words.clone()));
        let m = c(&mut rt, "java/util/stream/Stream.collect.(Ljava/util/stream/Collector;)Ljava/lang/Object;", &[stream, to_map]);
        let b = rt.string("b");
        assert_eq!(table_get(&mut rt, m, b).map(|v| rt.str_of(v).map(str::to_owned)), Some(Some("b".to_owned())));
        let twice = rt.alloc("java/util/stream/Stream", Data::Stream(vec![words[0], words[0]]));
        c(&mut rt, "java/util/stream/Stream.collect.(Ljava/util/stream/Collector;)Ljava/lang/Object;", &[twice, to_map]);
        assert!(rt.thrown.take().is_some_and(|e| rt.class_of(e) == "java/lang/IllegalStateException"));
        // Vanilla's `Util.toMap()`: a map's entries back into a map, each key to its value.
        let entries = c(&mut rt, "java/util/Map.entrySet.()Ljava/util/Set;", &[m]);
        let stream = c(&mut rt, "java/util/Set.stream.()Ljava/util/stream/Stream;", &[entries]);
        let util_to_map = crate::api::call(&mut rt, "net/minecraft/Util.toMap.()Ljava/util/stream/Collector;", &[]);
        let copy = c(&mut rt, "java/util/stream/Stream.collect.(Ljava/util/stream/Collector;)Ljava/lang/Object;", &[stream, util_to_map]);
        assert_ne!(copy, m);
        for w in ["a", "b", "c"] {
            let k = rt.string(w);
            assert_eq!(table_get(&mut rt, copy, k).map(|v| rt.str_of(v).map(str::to_owned)), Some(Some(w.to_owned())));
        }
        // Guava's concurrent map is a map; fastutil's `trim` answers true.
        let cm = c(&mut rt, "com/google/common/collect/Maps.newConcurrentMap.()Ljava/util/concurrent/ConcurrentMap;", &[]);
        table_put(&mut rt, cm, b, V::I(1));
        assert_eq!(table_get(&mut rt, cm, b), Some(V::I(1)));
        assert_eq!(c(&mut rt, "it/unimi/dsi/fastutil/objects/Object2ReferenceOpenHashMap.trim.()Z", &[cm]), V::I(1));
    }

    #[test]
    fn guava_of_past_twelve_elements_takes_the_rest_from_its_array() {
        let mut rt = Rt::new();
        let xs: Vec<V> = (0..14).map(|i| int_box(&mut rt, i)).collect();
        let rest = rt.alloc("[Ljava/lang/Object;", Data::Array(xs[12..].to_vec()));
        let mut args = xs[..12].to_vec();
        args.push(rest);
        let key = "com/google/common/collect/ImmutableList.of.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;[Ljava/lang/Object;)Lcom/google/common/collect/ImmutableList;";
        let l = c(&mut rt, key, &args);
        assert_eq!(elements(&rt, l), xs);
    }

    #[test]
    fn arrays_sort_fill_and_copy_work_on_the_array_itself() {
        let mut rt = Rt::new();
        let ints = |xs: &[i32]| xs.iter().map(|&x| V::I(x)).collect::<Vec<V>>();
        let a = rt.alloc("[I", Data::Array(ints(&[5, -1, 3, 0, 3])));
        c(&mut rt, "java/util/Arrays.sort.([I)V", &[a]);
        assert_eq!(elements(&rt, a), ints(&[-1, 0, 3, 3, 5]));
        // A range: only [1, 4) moves.
        let b = rt.alloc("[I", Data::Array(ints(&[9, 8, 7, 6, 5])));
        c(&mut rt, "java/util/Arrays.sort.([III)V", &[b, V::I(1), V::I(4)]);
        assert_eq!(elements(&rt, b), ints(&[9, 6, 7, 8, 5]));
        // Doubles in Double.compare's order: -0.0 before 0.0, NaN last.
        let d = rt.alloc("[D", Data::Array(vec![V::D(f64::NAN), V::D(0.0), V::D(-0.0), V::D(-2.5)]));
        c(&mut rt, "java/util/Arrays.sort.([D)V", &[d]);
        let sorted: Vec<f64> = elements(&rt, d).iter().map(|v| v.d()).collect();
        assert_eq!(sorted[..3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(), [(-2.5f64).to_bits(), (-0.0f64).to_bits(), 0.0f64.to_bits()]);
        assert!(sorted[3].is_nan());
        // Strings in their natural order.
        let words: Vec<V> = ["pear", "apple", "fig"].iter().map(|w| rt.string(w)).collect();
        let w = rt.alloc("[Ljava/lang/Object;", Data::Array(words.clone()));
        c(&mut rt, "java/util/Arrays.sort.([Ljava/lang/Object;)V", &[w]);
        assert_eq!(elements(&rt, w), vec![words[1], words[2], words[0]]);
        // A copy is an array of the asked length, padded with the type's zero.
        let longer = c(&mut rt, "java/util/Arrays.copyOf.([II)[I", &[a, V::I(7)]);
        assert_eq!(rt.class_of(longer), "[I");
        assert_eq!(elements(&rt, longer), ints(&[-1, 0, 3, 3, 5, 0, 0]));
        let mid = c(&mut rt, "java/util/Arrays.copyOfRange.([III)[I", &[a, V::I(3), V::I(6)]);
        assert_eq!(elements(&rt, mid), ints(&[3, 5, 0]));
        c(&mut rt, "java/util/Arrays.fill.([II)V", &[a, V::I(4)]);
        assert_eq!(elements(&rt, a), ints(&[4; 5]));
        // Java's values: Arrays.hashCode(new int[]{1, 2, 3}) == 30817; NaN equals NaN in Arrays.equals.
        let h = rt.alloc("[I", Data::Array(ints(&[1, 2, 3])));
        assert_eq!(c(&mut rt, "java/util/Arrays.hashCode.([I)I", &[h]), V::I(30817));
        let n1 = rt.alloc("[D", Data::Array(vec![V::D(f64::NAN)]));
        let n2 = rt.alloc("[D", Data::Array(vec![V::D(f64::NAN)]));
        assert_eq!(c(&mut rt, "java/util/Arrays.equals.([D[D)Z", &[n1, n2]), V::I(1));
        // A bad range throws as the JDK does.
        c(&mut rt, "java/util/Arrays.sort.([III)V", &[b, V::I(3), V::I(1)]);
        assert_eq!(rt.thrown.map(|e| rt.class_of(e)), Some("java/lang/IllegalArgumentException"));
    }

    fn list_of_three(rt: &mut Rt) -> V {
        new_list(rt, vec![V::I(1), V::I(2), V::I(3)])
    }

    /// `Objects.toString` is `String.valueOf` (null as "null") or the default given for null;
    /// `Stream.generate` asks its supplier as often as `limit` says (Create's debug info indents so);
    /// Guava's `Lists.reverse` turns a list around (Create's clone command clears its area so).
    #[test]
    fn objects_print_and_generated_streams_stop_at_their_limit() {
        fn tab(rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
            let n = rt.interned.get("test:asked").map_or(0, |v| v.i()) + 1;
            rt.interned.insert("test:asked".into(), V::I(n));
            Some(rt.string("\t"))
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(tab);
        let to_string = "java/util/Objects.toString.(Ljava/lang/Object;)Ljava/lang/String;";
        let printed = c(&mut rt, to_string, &[V::NULL]);
        assert_eq!(rt.str_of(printed), Some("null"));
        let five = crate::api_common::integer(&mut rt, 5);
        let printed = c(&mut rt, to_string, &[five]);
        assert_eq!(rt.str_of(printed), Some("5"));
        let none = rt.string("none");
        assert_eq!(c(&mut rt, "java/util/Objects.toString.(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;", &[V::NULL, none]), none);
        let supplier = rt.lambda_id("java/util/function/Supplier", 1, vec![]);
        let endless = c(&mut rt, "java/util/stream/Stream.generate.(Ljava/util/function/Supplier;)Ljava/util/stream/Stream;", &[supplier]);
        assert!(!rt.interned.contains_key("test:asked"), "nothing asked before the limit");
        let three = c(&mut rt, "java/util/stream/Stream.limit.(J)Ljava/util/stream/Stream;", &[endless, V::J(3)]);
        let forth = list_of_three(&mut rt);
        let back = c(&mut rt, "com/google/common/collect/Lists.reverse.(Ljava/util/List;)Ljava/util/List;", &[forth]);
        let back: Vec<i32> = elements(&rt, back).iter().map(|v| v.i()).collect();
        assert_eq!(back, [3, 2, 1]);
        let tabs: Vec<&str> = elements(&rt, three).iter().map(|v| rt.str_of(*v).unwrap_or("")).collect();
        assert_eq!(tabs, ["\t"; 3]);
        assert_eq!(rt.interned["test:asked"], V::I(3));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create sorts with `Comparator.comparing(key)`, `thenComparing(next)` and `reversed()`: by the
    /// key first, ties by the next, and the whole order turned around; `nullsFirst` puts nulls ahead.
    #[test]
    fn comparators_compose_as_javas_do() {
        // Lambda 1: a word's length, boxed (natural order); lambda 2: the word itself.
        fn keys(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            let w = rt.str_of(args[1]).unwrap_or("").to_owned();
            Some(if id == 1 { crate::api_common::integer(rt, w.len() as i32) } else { args[1] })
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(keys);
        let (by_len, by_word) = (rt.lambda_id("java/util/function/Function", 1, vec![]), rt.lambda_id("java/util/function/Function", 2, vec![]));
        let cmp = c(&mut rt, "java/util/Comparator.comparing.(Ljava/util/function/Function;)Ljava/util/Comparator;", &[by_len]);
        let cmp = c(&mut rt, "java/util/Comparator.thenComparing.(Ljava/util/function/Function;)Ljava/util/Comparator;", &[cmp, by_word]);
        let words: Vec<V> = ["pear", "fig", "apple", "kiwi"].iter().map(|w| rt.string(w)).collect();
        let list = new_list(&mut rt, words);
        let texts = |rt: &Rt, l: V| -> Vec<String> { elements(rt, l).iter().map(|&v| rt.str_of(v).unwrap_or("null").to_owned()).collect() };
        c(&mut rt, "java/util/List.sort.(Ljava/util/Comparator;)V", &[list, cmp]);
        assert_eq!(texts(&rt, list), ["fig", "kiwi", "pear", "apple"]);
        let back = c(&mut rt, "java/util/Comparator.reversed.()Ljava/util/Comparator;", &[cmp]);
        c(&mut rt, "java/util/List.sort.(Ljava/util/Comparator;)V", &[list, back]);
        assert_eq!(texts(&rt, list), ["apple", "pear", "kiwi", "fig"]);
        let natural = c(&mut rt, "java/util/Comparator.naturalOrder.()Ljava/util/Comparator;", &[]);
        let nulls = c(&mut rt, "java/util/Comparator.nullsFirst.(Ljava/util/Comparator;)Ljava/util/Comparator;", &[natural]);
        let (b, a) = (rt.string("b"), rt.string("a"));
        let mixed = new_list(&mut rt, vec![b, V::NULL, a]);
        c(&mut rt, "java/util/List.sort.(Ljava/util/Comparator;)V", &[mixed, nulls]);
        assert_eq!(texts(&rt, mixed), ["null", "a", "b"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Catnip orders a pole's directions with `Comparator.comparingDouble(Pair::getSecond)`: a
    /// method reference returning `Object` (a boxed `Double`) as a `ToDoubleFunction`, which Java
    /// unboxes. An `int` a `ToDoubleFunction` returns is widened.
    #[test]
    fn a_lambda_returning_a_box_where_a_primitive_is_due_is_unboxed() {
        // Lambda 1: the distance kept with each word, boxed; lambda 2: a word's length as an int.
        fn second(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            let w = rt.str_of(args[1]).unwrap_or("").to_owned();
            Some(match id {
                1 => {
                    let d = if w == "east" { 0.35 } else { 0.72 };
                    crate::mixin::box_value(rt, V::D(d), 'D')
                }
                _ => V::I(w.len() as i32),
            })
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(second);
        let by_distance = rt.lambda_id("java/util/function/ToDoubleFunction", 1, vec![]);
        let cmp = c(
            &mut rt,
            "java/util/Comparator.comparingDouble.(Ljava/util/function/ToDoubleFunction;)Ljava/util/Comparator;",
            &[by_distance],
        );
        let words: Vec<V> = ["west", "east"].iter().map(|w| rt.string(w)).collect();
        let list = new_list(&mut rt, words);
        c(
            &mut rt,
            "java/util/List.sort.(Ljava/util/Comparator;)V",
            &[list, cmp],
        );
        let texts: Vec<String> = elements(&rt, list)
            .iter()
            .map(|&v| rt.str_of(v).unwrap_or("null").to_owned())
            .collect();
        assert_eq!(texts, ["east", "west"]);
        let by_length = rt.lambda_id("java/util/function/ToDoubleFunction", 2, vec![]);
        let word = rt.string("north");
        let r = rt.call_functional(
            "java/util/function/ToDoubleFunction.applyAsDouble.(Ljava/lang/Object;)D",
            by_length,
            &[word],
        );
        assert_eq!(r, V::D(5.0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create keeps which of a contraption's block entities to draw in a `BitSet`: set by range,
    /// walked with `nextSetBit`, trimmed with `andNot`; the hash is the JDK's.
    #[test]
    fn a_bitset_sets_walks_and_combines_as_the_jdks() {
        let mut rt = Rt::new();
        let b = rt.new_object("java/util/BitSet");
        c(&mut rt, "java/util/BitSet.<init>.()V", &[b]);
        c(&mut rt, "java/util/BitSet.set.(II)V", &[b, V::I(2), V::I(5)]);
        c(&mut rt, "java/util/BitSet.set.(I)V", &[b, V::I(70)]);
        let walk = |rt: &mut Rt, b: V| {
            let mut out = Vec::new();
            let mut i = c(rt, "java/util/BitSet.nextSetBit.(I)I", &[b, V::I(0)]).i();
            while i >= 0 {
                out.push(i);
                i = c(rt, "java/util/BitSet.nextSetBit.(I)I", &[b, V::I(i + 1)]).i();
            }
            out
        };
        assert_eq!(walk(&mut rt, b), [2, 3, 4, 70]);
        // The JDK's `hashCode` of {2, 3, 4, 70}, and of {63, 127} (its words' top bits set).
        assert_eq!(c(&mut rt, "java/util/BitSet.hashCode.()I", &[b]), V::I(1102));
        let big = rt.new_object("java/util/BitSet");
        c(&mut rt, "java/util/BitSet.<init>.()V", &[big]);
        c(&mut rt, "java/util/BitSet.set.(I)V", &[big, V::I(63)]);
        c(&mut rt, "java/util/BitSet.set.(I)V", &[big, V::I(127)]);
        assert_eq!(c(&mut rt, "java/util/BitSet.hashCode.()I", &[big]), V::I(-2147482414));
        let other = rt.new_object("java/util/BitSet");
        c(&mut rt, "java/util/BitSet.<init>.()V", &[other]);
        c(&mut rt, "java/util/BitSet.set.(I)V", &[other, V::I(3)]);
        c(&mut rt, "java/util/BitSet.set.(I)V", &[other, V::I(70)]);
        c(&mut rt, "java/util/BitSet.andNot.(Ljava/util/BitSet;)V", &[b, other]);
        assert_eq!(walk(&mut rt, b), [2, 4]);
        assert_eq!(c(&mut rt, "java/util/BitSet.length.()I", &[b]), V::I(5));
        c(&mut rt, "java/util/BitSet.clear.()V", &[b]);
        assert_eq!(c(&mut rt, "java/util/BitSet.isEmpty.()Z", &[b]), V::I(1));
        // A bit written by value, cleared and flipped: a set whose high bits went is the set
        // that never had them (`equals`, `length`), as the JDK trims its words in use.
        let (wide, small) = (
            rt.new_object("java/util/BitSet"),
            rt.new_object("java/util/BitSet"),
        );
        for set in [wide, small] {
            c(&mut rt, "java/util/BitSet.<init>.(I)V", &[set, V::I(4096)]);
            c(
                &mut rt,
                "java/util/BitSet.set.(IZ)V",
                &[set, V::I(3), V::I(1)],
            );
        }
        c(
            &mut rt,
            "java/util/BitSet.set.(I)V",
            &[wide, V::I(2_000_000)],
        );
        assert_eq!(
            c(&mut rt, "java/util/BitSet.length.()I", &[wide]),
            V::I(2_000_001)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/BitSet.get.(I)Z",
                &[wide, V::I(2_000_000)]
            ),
            V::I(1)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/BitSet.get.(I)Z",
                &[wide, V::I(1_999_999)]
            ),
            V::I(0)
        );
        c(
            &mut rt,
            "java/util/BitSet.flip.(I)V",
            &[wide, V::I(2_000_000)],
        );
        c(
            &mut rt,
            "java/util/BitSet.set.(IZ)V",
            &[wide, V::I(900), V::I(0)],
        );
        c(&mut rt, "java/util/BitSet.flip.(I)V", &[wide, V::I(70)]);
        c(&mut rt, "java/util/BitSet.clear.(I)V", &[wide, V::I(70)]);
        assert_eq!(c(&mut rt, "java/util/BitSet.length.()I", &[wide]), V::I(4));
        assert_eq!(
            c(
                &mut rt,
                "java/util/BitSet.equals.(Ljava/lang/Object;)Z",
                &[wide, small]
            ),
            V::I(1)
        );
        assert_eq!(
            c(&mut rt, "java/util/BitSet.hashCode.()I", &[wide]),
            c(&mut rt, "java/util/BitSet.hashCode.()I", &[small])
        );
        assert_eq!(
            c(&mut rt, "java/util/BitSet.cardinality.()I", &[wide]),
            V::I(1)
        );
        // A set of millions, bit by bit, is filled in the time of its bits (it was the time of
        // its bits times its words: a mod's sphere of 16 million took hours).
        let started = std::time::Instant::now();
        for i in 0..2_000_000 {
            c(
                &mut rt,
                "java/util/BitSet.set.(IZ)V",
                &[wide, V::I(i), V::I(i % 3)],
            );
        }
        assert_eq!(
            c(&mut rt, "java/util/BitSet.cardinality.()I", &[wide]),
            V::I(1_333_333)
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "{:?}",
            started.elapsed()
        );
    }

    /// `LongAdder`: what was added, summed; `sumThenReset` gives the sum and starts again.
    #[test]
    fn a_long_adder_sums_what_is_added() {
        let mut rt = Rt::new();
        let adder = rt.new_object("java/util/concurrent/atomic/LongAdder");
        c(
            &mut rt,
            "java/util/concurrent/atomic/LongAdder.<init>.()V",
            &[adder],
        );
        c(
            &mut rt,
            "java/util/concurrent/atomic/LongAdder.add.(J)V",
            &[adder, V::J(40)],
        );
        c(
            &mut rt,
            "java/util/concurrent/atomic/LongAdder.increment.()V",
            &[adder],
        );
        c(
            &mut rt,
            "java/util/concurrent/atomic/LongAdder.increment.()V",
            &[adder],
        );
        c(
            &mut rt,
            "java/util/concurrent/atomic/LongAdder.decrement.()V",
            &[adder],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/atomic/LongAdder.sum.()J",
                &[adder]
            ),
            V::J(41)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/atomic/LongAdder.intValue.()I",
                &[adder]
            ),
            V::I(41)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/atomic/LongAdder.sumThenReset.()J",
                &[adder]
            ),
            V::J(41)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/concurrent/atomic/LongAdder.longValue.()J",
                &[adder]
            ),
            V::J(0)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_primitive_maps_computed_value_is_kept() {
        // A counter, as Create's recipe trie numbers its variants: `computeIfAbsent(v, k -> next++)`.
        fn next_id(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            let counter = rt.captured(args[0])[0];
            let n = rt.get_field(counter, "n").i();
            rt.put_field(counter, "n", V::I(n + 1));
            Some(V::I(n))
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(next_id);
        let counter = rt.new_object("t/Counter");
        rt.put_field(counter, "n", V::I(0));
        let f = rt.lambda_id("it/unimi/dsi/fastutil/objects/Object2IntFunction", 1, vec![counter]);
        let m = rt.new_object("it/unimi/dsi/fastutil/objects/Object2IntOpenHashMap");
        c(&mut rt, "it/unimi/dsi/fastutil/objects/Object2IntOpenHashMap.<init>.()V", &[m]);
        let key = "it/unimi/dsi/fastutil/objects/Object2IntOpenHashMap.computeIfAbsent.(Ljava/lang/Object;Lit/unimi/dsi/fastutil/objects/Object2IntFunction;)I";
        let (andesite, nugget) = (rt.string("andesite"), rt.string("nugget"));
        let ids: Vec<V> = [andesite, nugget, andesite, nugget].iter().map(|&k| c(&mut rt, key, &[m, k, f])).collect();
        assert_eq!(ids, [V::I(0), V::I(1), V::I(0), V::I(1)]);
        assert_eq!(c(&mut rt, "it/unimi/dsi/fastutil/objects/Object2IntMap.size.()I", &[m]), V::I(2));
        // putIfAbsent keeps a stored 0 (an int is never null).
        let put = "it/unimi/dsi/fastutil/objects/Object2IntMap.putIfAbsent.(Ljava/lang/Object;I)I";
        assert_eq!(c(&mut rt, put, &[m, andesite, V::I(9)]), V::I(0));
    }

    #[test]
    fn java_random_gives_the_jdks_numbers() {
        // Expected values printed by the JDK (java.util.Random, same calls in the same order).
        let mut rt = Rt::new();
        let r = rt.new_object("java/util/Random");
        c(&mut rt, "java/util/Random.<init>.(J)V", &[r, V::J(0)]);
        let mut call = |key: &'static str, extra: &[V]| {
            let mut args = vec![r];
            args.extend_from_slice(extra);
            c(&mut rt, key, &args)
        };
        assert_eq!(call("java/util/Random.nextInt.()I", &[]), V::I(-1155484576));
        assert_eq!(call("java/util/Random.nextInt.(I)I", &[V::I(10)]), V::I(8));
        assert_eq!(call("java/util/Random.nextInt.(I)I", &[V::I(7)]), V::I(4));
        assert_eq!(call("java/util/Random.nextInt.(II)I", &[V::I(5), V::I(105)]), V::I(52));
        assert_eq!(call("java/util/Random.nextInt.(II)I", &[V::I(-3), V::I(1 << 30)]), V::I(663681050));
        assert_eq!(call("java/util/Random.nextLong.()J", &[]), V::J(-8292973307042192125));
        assert_eq!(call("java/util/Random.nextBoolean.()Z", &[]), V::I(1));
        assert_eq!(call("java/util/Random.nextFloat.()F", &[]), V::F(0.7815346));
        assert_eq!(call("java/util/Random.nextDouble.()D", &[]), V::D(0.3332183994766498));
        for want in [-0.027290262907887285, 0.11524570286202315, -0.39016704137993785] {
            let got = call("java/util/Random.nextGaussian.()D", &[]).d();
            assert!((got - want).abs() <= 1e-15, "{got} vs {want}");
        }
        let seven = rt.new_object("java/util/Random");
        c(&mut rt, "java/util/Random.<init>.(J)V", &[seven, V::J(7)]);
        let bytes = rt.alloc("[B", Data::Array(vec![V::I(0); 6]));
        c(&mut rt, "java/util/Random.nextBytes.([B)V", &[seven, bytes]);
        assert_eq!(elements(&rt, bytes), [-103, 23, 15, -69, 24, 52].map(V::I));
        let d = rt.alloc("[D", Data::Array(vec![V::D(0.5), V::D(-0.0)]));
        assert_eq!(c(&mut rt, "java/util/Arrays.hashCode.([D)I", &[d]), V::I(1008731073));
        let l = rt.alloc("[J", Data::Array(vec![V::J(1 << 40), V::J(-7)]));
        assert_eq!(c(&mut rt, "java/util/Arrays.hashCode.([J)I", &[l]), V::I(8903));
    }

    #[test]
    fn fastutil_int_maps_and_sets_walk_as_in_java() {
        // Create's recipe trie walks `Int2ObjectMap.int2ObjectEntrySet()` and an `IntSet`'s
        // iterator, unboxing each `Integer` it gives.
        let mut rt = Rt::new();
        let m = rt.new_object("it/unimi/dsi/fastutil/ints/Int2ObjectOpenHashMap");
        c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectOpenHashMap.<init>.()V", &[m]);
        let (x, y) = (rt.string("x"), rt.string("y"));
        for (k, v) in [(4, x), (9, y)] {
            c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectMap.put.(ILjava/lang/Object;)Ljava/lang/Object;", &[m, V::I(k), v]);
        }
        let es = c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectMap.int2ObjectEntrySet.()Lit/unimi/dsi/fastutil/objects/ObjectSet;", &[m]);
        let it = c(&mut rt, "it/unimi/dsi/fastutil/objects/ObjectSet.iterator.()Lit/unimi/dsi/fastutil/objects/ObjectIterator;", &[es]);
        let mut seen = Vec::new();
        while c(&mut rt, "java/util/Iterator.hasNext.()Z", &[it]) == V::I(1) {
            let e = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
            let k = c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectMap$Entry.getIntKey.()I", &[e]);
            let v = c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectMap$Entry.getValue.()Ljava/lang/Object;", &[e]);
            seen.push((k, v));
        }
        assert_eq!(seen, [(V::I(4), x), (V::I(9), y)]);
        // Mekanism's `Int2ObjectMaps.fastIterator(map)` walks the same entries.
        let it = c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectMaps.fastIterator.(Lit/unimi/dsi/fastutil/ints/Int2ObjectMap;)Lit/unimi/dsi/fastutil/objects/ObjectIterator;", &[m]);
        let mut fast = Vec::new();
        while c(&mut rt, "java/util/Iterator.hasNext.()Z", &[it]) == V::I(1) {
            let e = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
            fast.push(c(&mut rt, "it/unimi/dsi/fastutil/ints/Int2ObjectMap$Entry.getIntKey.()I", &[e]));
        }
        assert_eq!(fast, [V::I(4), V::I(9)]);
        let set = rt.new_object("it/unimi/dsi/fastutil/ints/IntOpenHashSet");
        c(&mut rt, "it/unimi/dsi/fastutil/ints/IntOpenHashSet.<init>.()V", &[set]);
        c(&mut rt, "it/unimi/dsi/fastutil/ints/IntSet.add.(I)Z", &[set, V::I(7)]);
        let it = c(&mut rt, "it/unimi/dsi/fastutil/ints/IntSet.iterator.()Lit/unimi/dsi/fastutil/ints/IntIterator;", &[set]);
        let n = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
        crate::install();
        assert_eq!(crate::api::call(&mut rt, "java/lang/Integer.intValue.()I", &[n]), V::I(7));
        assert_eq!(crate::api::call(&mut rt, "java/lang/Number.longValue.()J", &[n]), V::J(7));
    }

    /// fastutil's `toIntArray()` copies the elements into a new `int[]`; `toIntArray(int[])` fills the
    /// given one when it is long enough (Create reads a contraption's block-entity ids this way).
    #[test]
    fn a_fastutil_int_list_gives_its_ints_as_an_array() {
        let mut rt = Rt::new();
        let l = rt.new_object("it/unimi/dsi/fastutil/ints/IntArrayList");
        c(&mut rt, "it/unimi/dsi/fastutil/ints/IntArrayList.<init>.()V", &[l]);
        for i in [5, -2, 9] {
            c(&mut rt, "it/unimi/dsi/fastutil/ints/IntList.add.(I)Z", &[l, V::I(i)]);
        }
        assert_eq!(c(&mut rt, "it/unimi/dsi/fastutil/ints/IntList.getInt.(I)I", &[l, V::I(1)]), V::I(-2));
        let arr = c(&mut rt, "it/unimi/dsi/fastutil/ints/IntList.toIntArray.()[I", &[l]);
        assert_eq!(rt.class_of(arr), "[I");
        let values = |rt: &Rt, v: V| match rt.heap.get(v.r() as usize).map(|o| &o.data) {
            Some(Data::Array(items)) => items.clone(),
            _ => Vec::new(),
        };
        assert_eq!(values(&rt, arr), [V::I(5), V::I(-2), V::I(9)]);
        let big = rt.alloc("[I", Data::Array(vec![V::I(0); 4]));
        let same = c(&mut rt, "it/unimi/dsi/fastutil/ints/IntList.toIntArray.([I)[I", &[l, big]);
        assert_eq!(same, big);
        assert_eq!(values(&rt, big), [V::I(5), V::I(-2), V::I(9), V::I(0)]);
        let small = rt.alloc("[I", Data::Array(vec![V::I(0); 1]));
        let fresh = c(&mut rt, "it/unimi/dsi/fastutil/ints/IntList.toIntArray.([I)[I", &[l, small]);
        assert_ne!(fresh, small);
        assert_eq!(values(&rt, fresh).len(), 3);
    }

    /// Guava's set views iterate as Guava 33's do (measured with the JDK): the first set's order,
    /// then the second's new elements.
    #[test]
    fn guava_set_views_keep_guavas_orders() {
        let mut rt = Rt::new();
        let set = |rt: &mut Rt, items: &[&str]| {
            let v: Vec<V> = items.iter().map(|s| rt.string(s)).collect();
            new_set(rt, &v)
        };
        let x = set(&mut rt, &["5", "1", "3", "2"]);
        let y = set(&mut rt, &["3", "7", "1", "9"]);
        let views: [(&str, &[&str]); 4] = [
            ("union", &["5", "1", "3", "2", "7", "9"]),
            ("intersection", &["1", "3"]),
            ("difference", &["5", "2"]),
            ("symmetricDifference", &["5", "2", "7", "9"]),
        ];
        for (view, want) in views {
            let key = rt.static_key(&format!("com/google/common/collect/Sets.{view}.(Ljava/util/Set;Ljava/util/Set;)Lcom/google/common/collect/Sets$SetView;"));
            let v = c(&mut rt, key, &[x, y]);
            let it = c(&mut rt, "com/google/common/collect/Sets$SetView.iterator.()Lcom/google/common/collect/UnmodifiableIterator;", &[v]);
            let mut got = Vec::new();
            while c(&mut rt, "java/util/Iterator.hasNext.()Z", &[it]).i() != 0 {
                let e = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
                got.push(rt.str_of(e).unwrap_or("?").to_owned());
            }
            assert_eq!(got, want, "{view}");
        }
    }

    /// `Map.replace` as JDK 21's `HashMap` answers it: `replace(key, value)` puts only for a key the
    /// map holds (one held as null too) and gives the old value; `replace(key, old, new)` only where
    /// the key holds `old`.
    #[test]
    fn map_replace_changes_only_what_the_map_holds() {
        const PUT: &str =
            "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        const GET: &str = "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;";
        const HAS: &str = "java/util/Map.containsKey.(Ljava/lang/Object;)Z";
        const REPLACE: &str =
            "java/util/Map.replace.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        const REPLACE_IF: &str =
            "java/util/Map.replace.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z";
        let rt = &mut Rt::new();
        let m = rt.new_object("java/util/HashMap");
        c(rt, "java/util/HashMap.<init>.()V", &[m]);
        let [a, b, cee, n, z] = ["a", "b", "c", "n", "z"].map(|t| rt.string(t));
        let [one, two, three, four, five, seven, nine] = [1, 2, 3, 4, 5, 7, 9].map(|i| crate::api_common::integer(rt, i));
        c(rt, PUT, &[m, a, one]);
        c(rt, PUT, &[m, b, V::NULL]);
        let int = |rt: &mut Rt, v: V| (!v.is_null()).then(|| crate::api::call(rt, "java/lang/Integer.intValue.()I", &[v]).i());
        let old = c(rt, REPLACE, &[m, a, two]);
        let now = c(rt, GET, &[m, a]);
        assert_eq!((int(rt, old), int(rt, now)), (Some(1), Some(2)));
        assert_eq!(c(rt, REPLACE, &[m, cee, three]), V::NULL);
        assert_eq!(c(rt, HAS, &[m, cee]), V::I(0));
        assert_eq!(c(rt, REPLACE, &[m, b, five]), V::NULL);
        let now = c(rt, GET, &[m, b]);
        assert_eq!(int(rt, now), Some(5));
        assert_eq!(c(rt, REPLACE_IF, &[m, a, two, three]), V::I(1));
        assert_eq!(c(rt, REPLACE_IF, &[m, a, nine, four]), V::I(0));
        let now = c(rt, GET, &[m, a]);
        assert_eq!(int(rt, now), Some(3));
        assert_eq!(c(rt, REPLACE_IF, &[m, z, V::NULL, one]), V::I(0));
        assert_eq!(c(rt, HAS, &[m, z]), V::I(0));
        c(rt, PUT, &[m, n, V::NULL]);
        assert_eq!(c(rt, REPLACE_IF, &[m, n, V::NULL, seven]), V::I(1));
        let now = c(rt, GET, &[m, n]);
        assert_eq!(int(rt, now), Some(7));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn hash_map_keys_compare_by_content() {
        let mut rt = Rt::new();
        let m = rt.new_object("java/util/HashMap");
        c(&mut rt, "java/util/HashMap.<init>.()V", &[m]);
        let k1 = rt.alloc("java/lang/String", Data::Str("shaft".into()));
        let k2 = rt.alloc("java/lang/String", Data::Str("shaft".into()));
        let one = rt.string("one");
        let two = rt.string("two");
        assert_eq!(c(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[m, k1, one]), V::NULL);
        assert_eq!(c(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[m, k2, two]), one);
        assert_eq!(c(&mut rt, "java/util/Map.size.()I", &[m]), V::I(1));
        assert_eq!(c(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[m, k1]), two);
        // Native positions are keys by value.
        let p1 = crate::api::block_pos(&mut rt, 1, 2, 3);
        let p2 = crate::api::block_pos(&mut rt, 1, 2, 3);
        c(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[m, p1, one]);
        assert_eq!(c(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[m, p2]), one);
        assert_eq!(c(&mut rt, "java/util/Map.remove.(Ljava/lang/Object;)Ljava/lang/Object;", &[m, k2]), two);
        assert_eq!(c(&mut rt, "java/util/Map.containsKey.(Ljava/lang/Object;)Z", &[m, k1]), V::I(0));
        assert_eq!(c(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[m, p1]), one);
    }

    #[test]
    fn lists_iterate_and_remove_through_iterator() {
        let mut rt = Rt::new();
        let l = rt.new_object("java/util/ArrayList");
        c(&mut rt, "java/util/ArrayList.<init>.()V", &[l]);
        for i in 0..5 {
            c(&mut rt, "java/util/List.add.(Ljava/lang/Object;)Z", &[l, V::I(i)]);
        }
        let it = c(&mut rt, "java/util/List.iterator.()Ljava/util/Iterator;", &[l]);
        let mut seen = Vec::new();
        while c(&mut rt, "java/util/Iterator.hasNext.()Z", &[it]) == V::I(1) {
            let v = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
            seen.push(v.i());
            if v.i() % 2 == 0 {
                c(&mut rt, "java/util/Iterator.remove.()V", &[it]);
            }
        }
        assert_eq!(seen, [0, 1, 2, 3, 4]);
        assert_eq!(elements(&rt, l), [V::I(1), V::I(3)]);
        assert_eq!(c(&mut rt, "java/util/List.getFirst.()Ljava/lang/Object;", &[l]), V::I(1));
    }

    #[test]
    fn a_list_removes_or_keeps_all_another_collection_holds() {
        let mut rt = Rt::new();
        let list_of = |rt: &mut Rt, items: &[&str]| {
            let l = rt.new_object("java/util/ArrayList");
            c(rt, "java/util/ArrayList.<init>.()V", &[l]);
            for i in items {
                let s = rt.string(i);
                c(rt, "java/util/List.add.(Ljava/lang/Object;)Z", &[l, s]);
            }
            l
        };
        let texts = |rt: &Rt, l: V| elements(rt, l).iter().map(|v| rt.str_of(*v).unwrap_or("?").to_owned()).collect::<Vec<_>>();
        // Equal strings, not the same objects; every occurrence goes.
        let l = list_of(&mut rt, &["a", "b", "a", "c"]);
        let gone = list_of(&mut rt, &["a", "x"]);
        assert_eq!(c(&mut rt, "java/util/List.removeAll.(Ljava/util/Collection;)Z", &[l, gone]), V::I(1));
        assert_eq!(texts(&rt, l), ["b", "c"]);
        assert_eq!(c(&mut rt, "java/util/List.removeAll.(Ljava/util/Collection;)Z", &[l, gone]), V::I(0));
        let l = list_of(&mut rt, &["a", "b", "a", "c"]);
        assert_eq!(c(&mut rt, "java/util/List.retainAll.(Ljava/util/Collection;)Z", &[l, gone]), V::I(1));
        assert_eq!(texts(&rt, l), ["a", "a"]);
    }

    #[test]
    fn tree_map_sorts_keys_and_sets_ignore_duplicates() {
        let mut rt = Rt::new();
        let m = rt.new_object("java/util/TreeMap");
        c(&mut rt, "java/util/TreeMap.<init>.()V", &[m]);
        for k in ["pear", "apple", "fig"] {
            let s = rt.string(k);
            c(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[m, s, V::I(1)]);
        }
        let keys = c(&mut rt, "java/util/Map.keySet.()Ljava/util/Set;", &[m]);
        let names: Vec<String> = elements(&rt, keys).iter().map(|&k| rt.str_of(k).unwrap().to_owned()).collect();
        assert_eq!(names, ["apple", "fig", "pear"]);
        let s = rt.new_object("java/util/HashSet");
        c(&mut rt, "java/util/HashSet.<init>.()V", &[s]);
        let x = rt.string("x");
        assert_eq!(c(&mut rt, "java/util/Set.add.(Ljava/lang/Object;)Z", &[s, x]), V::I(1));
        let x2 = rt.alloc("java/lang/String", Data::Str("x".into()));
        assert_eq!(c(&mut rt, "java/util/Set.add.(Ljava/lang/Object;)Z", &[s, x2]), V::I(0));
        assert_eq!(c(&mut rt, "java/util/Set.size.()I", &[s]), V::I(1));
        // A tree set keeps its comparator through clear().
        let rev = c(&mut rt, "java/util/Comparator.reverseOrder.()Ljava/util/Comparator;", &[]);
        let t = rt.new_object("java/util/TreeSet");
        c(&mut rt, "java/util/TreeSet.<init>.(Ljava/util/Comparator;)V", &[t, rev]);
        let add = "java/util/Set.add.(Ljava/lang/Object;)Z";
        for k in ["a", "c"] {
            let k = rt.string(k);
            c(&mut rt, add, &[t, k]);
        }
        c(&mut rt, "java/util/Set.clear.()V", &[t]);
        for k in ["x", "z", "y"] {
            let k = rt.string(k);
            c(&mut rt, add, &[t, k]);
        }
        let names: Vec<String> = elements(&rt, t).iter().map(|&k| rt.str_of(k).unwrap().to_owned()).collect();
        assert_eq!(names, ["z", "y", "x"], "the comparator outlives clear()");
    }

    /// Maps and sets through many insertions and removals, against the same steps on Rust's own:
    /// every key is found (or not) after each batch, in the order Java keeps. A hash map keeps
    /// insertion order, a tree map its keys' order; neither hashes its keys again when one leaves.
    #[test]
    fn tables_find_their_keys_through_insertions_and_removals() {
        let mut rt = Rt::new();
        let put = "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        let get = "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;";
        let remove = "java/util/Map.remove.(Ljava/lang/Object;)Ljava/lang/Object;";
        for class in ["java/util/HashMap", "java/util/TreeMap"] {
            let m = rt.new_object(class);
            let init = rt.static_key(&format!("{class}.<init>.()V"));
            c(&mut rt, init, &[m]);
            // (key, value) in the order the map is expected to hold them.
            let mut model: Vec<(i32, i32)> = Vec::new();
            let mut seed = 0x2545_f491u32;
            for step in 0..4000 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let key = (seed % 300) as i32;
                let k = crate::api_common::integer(&mut rt, key);
                if seed & 0x300 == 0 {
                    let had = model
                        .iter()
                        .position(|e| e.0 == key)
                        .map(|i| model.remove(i).1);
                    let got = c(&mut rt, remove, &[m, k]);
                    assert_eq!(
                        had.map(V::I).unwrap_or(V::NULL),
                        got,
                        "{class}: remove({key}) at step {step}"
                    );
                } else {
                    match model.iter_mut().find(|e| e.0 == key) {
                        Some(e) => e.1 = step,
                        None if class.ends_with("TreeMap") => {
                            let at = model.partition_point(|e| e.0 < key);
                            model.insert(at, (key, step));
                        }
                        None => model.push((key, step)),
                    }
                    c(&mut rt, put, &[m, k, V::I(step)]);
                }
                if step % 500 == 499 {
                    let keys: Vec<i32> = table_keys(&rt, m)
                        .iter()
                        .map(|k| rt.get_field(*k, "value").i())
                        .collect();
                    assert_eq!(
                        keys,
                        model.iter().map(|e| e.0).collect::<Vec<_>>(),
                        "{class}: the keys' order at step {step}"
                    );
                    for key in 0..300 {
                        let k = crate::api_common::integer(&mut rt, key);
                        let want = model
                            .iter()
                            .find(|e| e.0 == key)
                            .map_or(V::NULL, |e| V::I(e.1));
                        assert_eq!(
                            c(&mut rt, get, &[m, k]),
                            want,
                            "{class}: get({key}) at step {step}"
                        );
                    }
                }
            }
        }
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// `Arrays.setAll` fills an array from its indices, `Map.replaceAll` gives each key the value
    /// a function makes of its entry, and `Stream.ofNullable(null)` is empty.
    #[test]
    fn arrays_maps_and_streams_take_a_function_of_each() {
        fn fns(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            Some(match id {
                // An index as its text; an index squared; a key's value and its key's length.
                1 => rt.string(&format!("#{}", args[1].i())),
                2 => V::I(args[1].i() * args[1].i()),
                _ => {
                    let n = rt.str_of(args[1]).map_or(0, str::len) as i32;
                    V::I(args[2].i() + n)
                }
            })
        }
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(fns);
        // The runtime's own functions are called as the linked code calls them.
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let names = rt.new_array(3, V::NULL);
        let name = rt.lambda_id("java/util/function/IntFunction", 1, vec![]);
        c(
            &mut rt,
            "java/util/Arrays.setAll.([Ljava/lang/Object;Ljava/util/function/IntFunction;)V",
            &[names, name],
        );
        let texts: Vec<String> = elements(&rt, names)
            .iter()
            .map(|v| rt.str_of(*v).unwrap_or("?").to_owned())
            .collect();
        assert_eq!(texts, ["#0", "#1", "#2"]);
        let squares = rt.alloc("[I", Data::Array(vec![V::I(0); 4]));
        let square = rt.lambda_id("java/util/function/IntUnaryOperator", 2, vec![]);
        c(
            &mut rt,
            "java/util/Arrays.setAll.([ILjava/util/function/IntUnaryOperator;)V",
            &[squares, square],
        );
        assert_eq!(elements(&rt, squares), [V::I(0), V::I(1), V::I(4), V::I(9)]);

        let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
        for (k, v) in [("iron", 1), ("gold", 20)] {
            let k = rt.string(k);
            table_put(&mut rt, m, k, V::I(v));
        }
        let longer = rt.lambda_id("java/util/function/BiFunction", 3, vec![]);
        c(
            &mut rt,
            "java/util/Map.replaceAll.(Ljava/util/function/BiFunction;)V",
            &[m, longer],
        );
        let values: Vec<V> = table_pairs(&rt, m).into_iter().map(|(_, v)| v).collect();
        assert_eq!(values, [V::I(5), V::I(24)]);

        // The map's entries by their keys, and by their values the other way round.
        let by_key = c(&mut rt, "java/util/Map$Entry.comparingByKey.()Ljava/util/Comparator;", &[]);
        let entries = c(&mut rt, "java/util/Map.entrySet.()Ljava/util/Set;", &[m]);
        let list = new_list(&mut rt, Vec::new());
        c(&mut rt, "java/util/List.addAll.(Ljava/util/Collection;)Z", &[list, entries]);
        c(&mut rt, "java/util/List.sort.(Ljava/util/Comparator;)V", &[list, by_key]);
        let keys = |rt: &mut Rt, list: V| -> Vec<String> {
            elements(rt, list)
                .into_iter()
                .map(|e| {
                    let k = c(rt, "java/util/Map$Entry.getKey.()Ljava/lang/Object;", &[e]);
                    rt.str_of(k).unwrap_or("?").to_owned()
                })
                .collect()
        };
        assert_eq!(keys(&mut rt, list), ["gold", "iron"]);
        let natural = c(&mut rt, "java/util/Comparator.naturalOrder.()Ljava/util/Comparator;", &[]);
        let by_value = c(&mut rt, "java/util/Map$Entry.comparingByValue.(Ljava/util/Comparator;)Ljava/util/Comparator;", &[natural]);
        c(&mut rt, "java/util/List.sort.(Ljava/util/Comparator;)V", &[list, by_value]);
        assert_eq!(keys(&mut rt, list), ["iron", "gold"], "5 before 24");

        let none = c(
            &mut rt,
            "java/util/stream/Stream.ofNullable.(Ljava/lang/Object;)Ljava/util/stream/Stream;",
            &[V::NULL],
        );
        assert_eq!(
            c(&mut rt, "java/util/stream/Stream.count.()J", &[none]),
            V::J(0)
        );
        let one = c(
            &mut rt,
            "java/util/stream/Stream.ofNullable.(Ljava/lang/Object;)Ljava/util/stream/Stream;",
            &[m],
        );
        assert_eq!(
            c(&mut rt, "java/util/stream/Stream.count.()J", &[one]),
            V::J(1)
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Guava's `BiMap.inverse()` is a view: the same object at every call, in which what is put
    /// in the map since is found, and through which the map is written. A value has one key:
    /// `put` refuses a value another key holds, `forcePut` takes it from that key. (The inverse
    /// was a copy made at each call: FramedBlocks asks for it once per block state, and with
    /// 365 mods' states that took memory by the square of their number: 23 GB.)
    #[test]
    fn a_bimap_and_its_inverse_are_one() {
        crate::install();
        let mut rt = Rt::new();
        const PUT: &str = "com/google/common/collect/BiMap.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        const GET: &str =
            "com/google/common/collect/BiMap.get.(Ljava/lang/Object;)Ljava/lang/Object;";
        const INVERSE: &str =
            "com/google/common/collect/BiMap.inverse.()Lcom/google/common/collect/BiMap;";
        let m = c(
            &mut rt,
            "com/google/common/collect/HashBiMap.create.()Lcom/google/common/collect/HashBiMap;",
            &[],
        );
        let [stone, dirt, sand, clay, one, two, three, four, five] =
            ["stone", "dirt", "sand", "clay", "1", "2", "3", "4", "5"].map(|t| rt.string(t));
        let texts = |rt: &Rt, m: V| -> Vec<(String, String)> {
            let text = |v: V| rt.str_of(v).unwrap_or("null").to_owned();
            table_pairs(rt, m)
                .into_iter()
                .map(|(k, v)| (text(k), text(v)))
                .collect()
        };
        let pairs = |all: &[(&str, &str)]| -> Vec<(String, String)> {
            all.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        c(&mut rt, PUT, &[m, stone, one]);
        let inverse = c(&mut rt, INVERSE, &[m]);
        assert_eq!(
            c(&mut rt, INVERSE, &[m]),
            inverse,
            "one view, not a copy a call"
        );
        assert_eq!(c(&mut rt, INVERSE, &[inverse]), m);
        // Put after the view was taken, and through it.
        c(&mut rt, PUT, &[m, dirt, two]);
        c(&mut rt, PUT, &[inverse, three, sand]);
        assert_eq!(c(&mut rt, GET, &[inverse, two]), dirt);
        assert_eq!(c(&mut rt, GET, &[m, sand]), three);
        assert_eq!(
            texts(&rt, m),
            pairs(&[("stone", "1"), ("dirt", "2"), ("sand", "3")])
        );
        // A key's new value: its old one is no key of the inverse, and the entry keeps its place.
        assert_eq!(c(&mut rt, PUT, &[m, dirt, four]), two);
        assert_eq!(
            texts(&rt, inverse),
            pairs(&[("1", "stone"), ("4", "dirt"), ("3", "sand")])
        );
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/BiMap.containsValue.(Ljava/lang/Object;)Z",
                &[m, two]
            ),
            V::I(0)
        );
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/BiMap.containsValue.(Ljava/lang/Object;)Z",
                &[m, four]
            ),
            V::I(1)
        );
        // A value another key holds: refused, and nothing changes; forced, that key goes.
        c(&mut rt, PUT, &[m, clay, one]);
        let thrown = rt.thrown.take().expect("a value has one key");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");
        assert_eq!(
            texts(&rt, m),
            pairs(&[("stone", "1"), ("dirt", "4"), ("sand", "3")])
        );
        c(
            &mut rt,
            "com/google/common/collect/BiMap.forcePut.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[m, clay, one],
        );
        assert_eq!(
            texts(&rt, m),
            pairs(&[("dirt", "4"), ("sand", "3"), ("clay", "1")])
        );
        assert_eq!(
            texts(&rt, inverse),
            pairs(&[("4", "dirt"), ("3", "sand"), ("1", "clay")])
        );
        // The same entry again is no change; a removal through either is one of both.
        assert_eq!(c(&mut rt, PUT, &[m, clay, one]), one);
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/BiMap.remove.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[inverse, four]
            ),
            dirt
        );
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/BiMap.containsKey.(Ljava/lang/Object;)Z",
                &[m, dirt]
            ),
            V::I(0)
        );
        c(&mut rt, PUT, &[m, stone, five]);
        assert_eq!(
            texts(&rt, inverse),
            pairs(&[("3", "sand"), ("1", "clay"), ("5", "stone")])
        );
        c(
            &mut rt,
            "com/google/common/collect/BiMap.clear.()V",
            &[inverse],
        );
        assert!(texts(&rt, m).is_empty() && texts(&rt, inverse).is_empty());
        c(&mut rt, PUT, &[m, sand, two]);
        assert_eq!(
            c(&mut rt, GET, &[inverse, two]),
            sand,
            "still one after a clear"
        );
        // `create(map)` holds the map's entries; an immutable one's inverse is made once.
        let copy = c(
            &mut rt,
            "com/google/common/collect/HashBiMap.create.(Ljava/util/Map;)Lcom/google/common/collect/HashBiMap;",
            &[m],
        );
        let copied = c(&mut rt, INVERSE, &[copy]);
        assert_eq!(texts(&rt, copied), pairs(&[("2", "sand")]));
        c(&mut rt, PUT, &[copy, clay, one]);
        assert_eq!(
            texts(&rt, m),
            pairs(&[("sand", "2")]),
            "a copy is a map of its own"
        );
        let fixed = c(
            &mut rt,
            "com/google/common/collect/ImmutableBiMap.copyOf.(Ljava/util/Map;)Lcom/google/common/collect/ImmutableBiMap;",
            &[copy],
        );
        let back = c(
            &mut rt,
            "com/google/common/collect/ImmutableBiMap.inverse.()Lcom/google/common/collect/ImmutableBiMap;",
            &[fixed],
        );
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/ImmutableBiMap.inverse.()Lcom/google/common/collect/ImmutableBiMap;",
                &[fixed]
            ),
            back
        );
        assert_eq!(texts(&rt, back), pairs(&[("2", "sand"), ("1", "clay")]));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A tree map made with a comparator tells its keys apart by it alone, as Java's does: one
    /// entry for `"Speed"` and `"SPEED"` under `String.CASE_INSENSITIVE_ORDER`, found by either.
    #[test]
    fn a_tree_map_with_a_comparator_finds_keys_by_it() {
        crate::install();
        let mut rt = Rt::new();
        let order = crate::api::get_static(&mut rt, "java/lang/String.CASE_INSENSITIVE_ORDER");
        let m = rt.new_object("java/util/TreeMap");
        c(
            &mut rt,
            "java/util/TreeMap.<init>.(Ljava/util/Comparator;)V",
            &[m, order],
        );
        let put = "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        for (i, k) in ["speed", "Torque", "angle", "SPEED", "torque"]
            .iter()
            .enumerate()
        {
            let k = rt.string(k);
            c(&mut rt, put, &[m, k, V::I(i as i32)]);
        }
        let keys: Vec<String> = table_keys(&rt, m)
            .iter()
            .map(|k| rt.str_of(*k).unwrap().to_owned())
            .collect();
        assert_eq!(
            keys,
            ["angle", "speed", "Torque"],
            "the first spelling of a key stays"
        );
        let k = rt.string("TORQUE");
        assert_eq!(
            c(
                &mut rt,
                "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[m, k]
            ),
            V::I(4)
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/Map.containsKey.(Ljava/lang/Object;)Z",
                &[m, k]
            ),
            V::I(1)
        );
        let k = rt.string("Speed");
        assert_eq!(
            c(
                &mut rt,
                "java/util/Map.remove.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[m, k]
            ),
            V::I(3)
        );
        assert_eq!(c(&mut rt, "java/util/Map.size.()I", &[m]), V::I(2));
        // The comparator's own numbers, as `String.compareToIgnoreCase` gives them.
        let (a, b) = (rt.string("Apple"), rt.string("apricot"));
        assert_eq!(
            crate::api::call(
                &mut rt,
                "java/lang/String.compareToIgnoreCase.(Ljava/lang/String;)I",
                &[a, b]
            ),
            V::I('p' as i32 - 'r' as i32)
        );
        let (a, b) = (rt.string("gear"), rt.string("GEARBOX"));
        assert_eq!(
            c(
                &mut rt,
                "java/util/Comparator.compare.(Ljava/lang/Object;Ljava/lang/Object;)I",
                &[order, a, b]
            ),
            V::I(-3)
        );
        // Ids in a sorted map of their own order: by path, then namespace, as `ResourceLocation` compares.
        let ids = rt.new_object("java/util/TreeMap");
        c(&mut rt, "java/util/TreeMap.<init>.()V", &[ids]);
        for id in [
            "create:shaft",
            "minecraft:stone",
            "create:andesite_alloy",
            "farmersdelight:shaft",
        ] {
            let (ns, path) = id.split_once(':').unwrap();
            let (ns, path) = (rt.string(ns), rt.string(path));
            let k = crate::api::call(
                &mut rt,
                "net/minecraft/resources/ResourceLocation.fromNamespaceAndPath.(Ljava/lang/String;Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;",
                &[ns, path],
            );
            c(&mut rt, put, &[ids, k, V::I(1)]);
        }
        let keys: Vec<String> = table_keys(&rt, ids)
            .iter()
            .map(|k| crate::jlang::text(&rt, *k))
            .collect();
        assert_eq!(
            keys,
            [
                "create:andesite_alloy",
                "create:shaft",
                "farmersdelight:shaft",
                "minecraft:stone"
            ]
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    #[test]
    fn guava_tables_and_multimaps() {
        let mut rt = Rt::new();
        let t = c(&mut rt, "com/google/common/collect/HashBasedTable.create.()Lcom/google/common/collect/HashBasedTable;", &[]);
        let (block, item) = (rt.string("block"), rt.string("item"));
        let (shaft, cog) = (rt.string("shaft"), rt.string("cog"));
        c(&mut rt, "com/google/common/collect/Table.put.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[t, block, shaft, V::I(1)]);
        c(&mut rt, "com/google/common/collect/Table.put.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[t, block, cog, V::I(2)]);
        c(&mut rt, "com/google/common/collect/Table.put.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[t, item, shaft, V::I(3)]);
        let r = c(&mut rt, "com/google/common/collect/Table.row.(Ljava/lang/Object;)Ljava/util/Map;", &[t, block]);
        assert_eq!(c(&mut rt, "java/util/Map.size.()I", &[r]), V::I(2));
        assert_eq!(c(&mut rt, "com/google/common/collect/Table.get.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[t, item, shaft]), V::I(3));
        // A synchronised view is the table, and its cells are its rows' entries.
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/Tables.synchronizedTable.(Lcom/google/common/collect/Table;)Lcom/google/common/collect/Table;",
                &[t]
            ),
            t
        );
        let cells = c(
            &mut rt,
            "com/google/common/collect/Table.cellSet.()Ljava/util/Set;",
            &[t],
        );
        let cells = elements(&rt, cells);
        assert_eq!(cells.len(), 3);
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/Table$Cell.getRowKey.()Ljava/lang/Object;",
                &[cells[2]]
            ),
            item
        );
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/collect/Table$Cell.getValue.()Ljava/lang/Object;",
                &[cells[1]]
            ),
            V::I(2)
        );
        let columns = c(
            &mut rt,
            "com/google/common/collect/Table.columnKeySet.()Ljava/util/Set;",
            &[t],
        );
        assert_eq!(elements(&rt, columns).len(), 2);
        assert_eq!(c(&mut rt, "com/google/common/collect/Table.containsColumn.(Ljava/lang/Object;)Z", &[t, cog]), V::I(1));
        let m = c(&mut rt, "com/google/common/collect/ArrayListMultimap.create.()Lcom/google/common/collect/ArrayListMultimap;", &[]);
        assert_eq!(c(&mut rt, "com/google/common/collect/Multimap.isEmpty.()Z", &[m]), V::I(1));
        c(&mut rt, "com/google/common/collect/Multimap.put.(Ljava/lang/Object;Ljava/lang/Object;)Z", &[m, block, V::I(7)]);
        c(&mut rt, "com/google/common/collect/Multimap.put.(Ljava/lang/Object;Ljava/lang/Object;)Z", &[m, block, V::I(8)]);
        let got = c(&mut rt, "com/google/common/collect/Multimap.get.(Ljava/lang/Object;)Ljava/util/Collection;", &[m, block]);
        assert_eq!(elements(&rt, got), [V::I(7), V::I(8)]);
        assert_eq!(c(&mut rt, "com/google/common/collect/Multimap.size.()I", &[m]), V::I(2));
    }

    /// `Multimaps.newMultimap(new HashMap<>(), HashSet::new)` (Create's station screen): the keys
    /// hold the factory's collections, a set each value once, a list every one in order.
    #[test]
    fn a_multimap_over_a_factory_holds_its_collections() {
        let mut rt = Rt::new();
        crate::install();
        // What generated code does for an object no mod class is: the API call by the method's key.
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        let new = "com/google/common/collect/Multimaps.newMultimap.(Ljava/util/Map;Lcom/google/common/base/Supplier;)Lcom/google/common/collect/Multimap;";
        let put = "com/google/common/collect/Multimap.put.(Ljava/lang/Object;Ljava/lang/Object;)Z";
        let entries = "com/google/common/collect/Multimap.entries.()Ljava/util/Collection;";
        let key = rt.string("station");
        for (factory, held) in [
            ("java/util/HashSet", vec![V::I(4)]),
            ("java/util/ArrayList", vec![V::I(4), V::I(4)]),
        ] {
            let backing = new_set_of(&mut rt, &[]);
            let make = crate::functions::constructor(&mut rt, factory, "()V");
            let m = c(&mut rt, new, &[backing, make]);
            assert_eq!(c(&mut rt, put, &[m, key, V::I(4)]), V::I(1));
            c(&mut rt, put, &[m, key, V::I(4)]);
            let got = c(
                &mut rt,
                "com/google/common/collect/Multimap.get.(Ljava/lang/Object;)Ljava/util/Collection;",
                &[m, key],
            );
            assert_eq!(elements(&rt, got), held, "{factory}");
            let all = c(&mut rt, entries, &[m]);
            assert_eq!(elements(&rt, all).len(), held.len(), "{factory}");
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn registrate_finds_its_pending_callbacks_under_an_equal_pair() {
        // Registrate files a builder's `onRegister` callbacks under `Pair.of(name, registry)` before
        // the entry exists, and takes them back with a new pair of the same halves.
        let mut rt = Rt::new();
        let pair = |rt: &mut Rt, l: &str, r: &str| {
            let (l, r) = (rt.string(l), rt.string(r));
            crate::jlang::call(rt, "org/apache/commons/lang3/tuple/Pair.of.(Ljava/lang/Object;Ljava/lang/Object;)Lorg/apache/commons/lang3/tuple/Pair;", &[l, r]).unwrap()
        };
        let b = c(&mut rt, "com/google/common/collect/MultimapBuilder.hashKeys.()Lcom/google/common/collect/MultimapBuilder$MultimapBuilderWithKeys;", &[]);
        let b = c(&mut rt, "com/google/common/collect/MultimapBuilder$MultimapBuilderWithKeys.arrayListValues.()Lcom/google/common/collect/MultimapBuilder$ListMultimapBuilder;", &[b]);
        let m = c(&mut rt, "com/google/common/collect/MultimapBuilder$ListMultimapBuilder.build.()Lcom/google/common/collect/ListMultimap;", &[b]);
        let put = "com/google/common/collect/Multimap.put.(Ljava/lang/Object;Ljava/lang/Object;)Z";
        let filed = pair(&mut rt, "shaft", "block");
        c(&mut rt, put, &[m, filed, V::I(1)]);
        c(&mut rt, put, &[m, filed, V::I(2)]);
        let other = pair(&mut rt, "cogwheel", "block");
        c(&mut rt, put, &[m, other, V::I(3)]);
        let asked = pair(&mut rt, "shaft", "block");
        let got = c(&mut rt, "com/google/common/collect/Multimap.removeAll.(Ljava/lang/Object;)Ljava/util/Collection;", &[m, asked]);
        assert_eq!(elements(&rt, got), [V::I(1), V::I(2)]);
        assert_eq!(c(&mut rt, "com/google/common/collect/Multimap.size.()I", &[m]), V::I(1));
        // Halves differing in one place are another key.
        let item = pair(&mut rt, "cogwheel", "item");
        assert_eq!(c(&mut rt, "com/google/common/collect/Multimap.containsKey.(Ljava/lang/Object;)Z", &[m, item]), V::I(0));
    }

    #[test]
    fn guava_immutable_builders_collect_what_they_are_given() {
        let mut rt = Rt::new();
        let b = c(&mut rt, "com/google/common/collect/ImmutableList.builder.()Lcom/google/common/collect/ImmutableList$Builder;", &[]);
        let add = "com/google/common/collect/ImmutableList$Builder.add.(Ljava/lang/Object;)Lcom/google/common/collect/ImmutableList$Builder;";
        assert_eq!(c(&mut rt, add, &[b, V::I(1)]), b, "chains");
        c(&mut rt, add, &[b, V::I(2)]);
        let more = new_list(&mut rt, vec![V::I(3), V::I(1)]);
        c(&mut rt, "com/google/common/collect/ImmutableList$Builder.addAll.(Ljava/lang/Iterable;)Lcom/google/common/collect/ImmutableList$Builder;", &[b, more]);
        let list = c(&mut rt, "com/google/common/collect/ImmutableList$Builder.build.()Lcom/google/common/collect/ImmutableList;", &[b]);
        assert_eq!(elements(&rt, list), [V::I(1), V::I(2), V::I(3), V::I(1)]);
        // A set's builder drops repeats; a map's keeps a key's last value.
        let s = c(&mut rt, "com/google/common/collect/ImmutableSet.builder.()Lcom/google/common/collect/ImmutableSet$Builder;", &[]);
        for v in [1, 2, 1] {
            c(&mut rt, "com/google/common/collect/ImmutableSet$Builder.add.(Ljava/lang/Object;)Lcom/google/common/collect/ImmutableSet$Builder;", &[s, V::I(v)]);
        }
        let set = c(&mut rt, "com/google/common/collect/ImmutableSet$Builder.build.()Lcom/google/common/collect/ImmutableSet;", &[s]);
        assert_eq!(elements(&rt, set), [V::I(1), V::I(2)]);
        let m = c(&mut rt, "com/google/common/collect/ImmutableMap.builder.()Lcom/google/common/collect/ImmutableMap$Builder;", &[]);
        let k = rt.string("speed");
        let put = "com/google/common/collect/ImmutableMap$Builder.put.(Ljava/lang/Object;Ljava/lang/Object;)Lcom/google/common/collect/ImmutableMap$Builder;";
        c(&mut rt, put, &[m, k, V::I(8)]);
        c(&mut rt, put, &[m, k, V::I(16)]);
        let map = c(&mut rt, "com/google/common/collect/ImmutableMap$Builder.buildKeepingLast.()Lcom/google/common/collect/ImmutableMap;", &[m]);
        assert_eq!(c(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[map, k]), V::I(16));
        // `ImmutableMap.copyOf(map)` (Create's contraption storage): the same entries, kept when the
        // source changes after.
        let copy = c(&mut rt, "com/google/common/collect/ImmutableMap.copyOf.(Ljava/util/Map;)Lcom/google/common/collect/ImmutableMap;", &[map]);
        c(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[map, k, V::I(32)]);
        assert_eq!(c(&mut rt, "com/google/common/collect/ImmutableMap.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[copy, k]), V::I(16));
        assert_eq!(c(&mut rt, "com/google/common/collect/ImmutableMap.size.()I", &[copy]), V::I(1));
    }

    #[test]
    fn atomics_and_objects() {
        let mut rt = Rt::new();
        let n = rt.new_object("java/util/concurrent/atomic/AtomicInteger");
        c(&mut rt, "java/util/concurrent/atomic/AtomicInteger.<init>.()V", &[n]);
        assert_eq!(c(&mut rt, "java/util/concurrent/atomic/AtomicInteger.incrementAndGet.()I", &[n]), V::I(1));
        assert_eq!(c(&mut rt, "java/util/concurrent/atomic/AtomicInteger.getAndIncrement.()I", &[n]), V::I(1));
        assert_eq!(c(&mut rt, "java/util/concurrent/atomic/AtomicInteger.get.()I", &[n]), V::I(2));
        let s = rt.string("a");
        assert_eq!(c(&mut rt, "java/util/Objects.requireNonNull.(Ljava/lang/Object;)Ljava/lang/Object;", &[s]), s);
        // `compareAndExchange*` answers the value it found, and sets the new one only over the expected.
        let r = rt.new_object("java/util/concurrent/atomic/AtomicReference");
        c(&mut rt, "java/util/concurrent/atomic/AtomicReference.<init>.()V", &[r]);
        let made = rt.new_object("t/Made");
        let cas = "java/util/concurrent/atomic/AtomicReference.compareAndExchangeRelease.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
        assert_eq!(c(&mut rt, cas, &[r, V::NULL, made]), V::NULL);
        let other = rt.new_object("t/Made");
        assert_eq!(c(&mut rt, cas, &[r, V::NULL, other]), made);
        assert_eq!(c(&mut rt, "java/util/concurrent/atomic/AtomicReference.getAcquire.()Ljava/lang/Object;", &[r]), made);
    }

    /// Guava's `Optional` (Create's seats: `getLeashed(level, player).or(player)`): `or` gives the
    /// value, else the default, and refuses a null default; `get` on an absent one throws;
    /// `transform` maps a present value only; to and from `java.util.Optional`; `equals`,
    /// `hashCode` and `toString` as Guava's `Present` and `Absent` answer them.
    #[test]
    fn guavas_optional_gives_its_value_or_the_default() {
        fn vcall(rt: &mut Rt, _key: &'static str, _short: &'static str, args: &[V]) -> V {
            // A mod's own supplier (`() -> "made"`) and function (`s -> s + "!"`).
            if args.len() == 1 {
                return rt.string("made");
            }
            let s = rt.to_java_string(args[1], 'L');
            rt.string(&format!("{s}!"))
        }
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        let thrown = |rt: &mut Rt| rt.thrown.take().map(|e| (rt.class_of(e), rt.to_java_string(rt.get_field(e, "message"), 'L')));
        let of = |rt: &mut Rt, v: V| c(rt, "com/google/common/base/Optional.of.(Ljava/lang/Object;)Lcom/google/common/base/Optional;", &[v]);
        let get = |rt: &mut Rt, o: V| c(rt, "com/google/common/base/Optional.get.()Ljava/lang/Object;", &[o]);
        let text = |rt: &mut Rt, v: V| rt.to_java_string(v, 'L');
        let absent = c(&mut rt, "com/google/common/base/Optional.absent.()Lcom/google/common/base/Optional;", &[]);
        let (player, mob) = (rt.new_object("t/Player"), rt.new_object("t/Mob"));
        let or = "com/google/common/base/Optional.or.(Ljava/lang/Object;)Ljava/lang/Object;";
        assert_eq!(c(&mut rt, or, &[absent, player]), player);
        let leashed = of(&mut rt, mob);
        assert_eq!(c(&mut rt, or, &[leashed, player]), mob);
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/base/Optional.isPresent.()Z",
                &[absent]
            ),
            V::I(0)
        );
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/base/Optional.orNull.()Ljava/lang/Object;",
                &[absent]
            ),
            V::NULL
        );
        // Null is refused where Guava refuses it.
        c(&mut rt, or, &[absent, V::NULL]);
        assert_eq!(
            thrown(&mut rt),
            Some((
                "java/lang/NullPointerException",
                "use Optional.orNull() instead of Optional.or(null)".to_owned()
            ))
        );
        of(&mut rt, V::NULL);
        assert_eq!(
            thrown(&mut rt).map(|(class, _)| class),
            Some("java/lang/NullPointerException")
        );
        assert_eq!(get(&mut rt, absent), V::NULL);
        assert_eq!(
            thrown(&mut rt),
            Some((
                "java/lang/IllegalStateException",
                "Optional.get() cannot be called on an absent value".to_owned()
            ))
        );
        // `or(Optional)` and `or(Supplier)`.
        let or_optional = "com/google/common/base/Optional.or.(Lcom/google/common/base/Optional;)Lcom/google/common/base/Optional;";
        assert_eq!(c(&mut rt, or_optional, &[absent, leashed]), leashed);
        let other = of(&mut rt, player);
        assert_eq!(c(&mut rt, or_optional, &[leashed, other]), leashed);
        let supplier = rt.new_object("t/Supplier");
        let made = c(
            &mut rt,
            "com/google/common/base/Optional.or.(Lcom/google/common/base/Supplier;)Ljava/lang/Object;",
            &[absent, supplier],
        );
        assert_eq!(text(&mut rt, made), "made");
        // `transform` maps a present value; an absent one stays absent.
        let a = rt.string("a");
        let some = of(&mut rt, a);
        let function = rt.new_object("t/Function");
        let transform = "com/google/common/base/Optional.transform.(Lcom/google/common/base/Function;)Lcom/google/common/base/Optional;";
        let mapped = c(&mut rt, transform, &[some, function]);
        let mapped = get(&mut rt, mapped);
        assert_eq!(text(&mut rt, mapped), "a!");
        let still = c(&mut rt, transform, &[absent, function]);
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/base/Optional.isPresent.()Z",
                &[still]
            ),
            V::I(0)
        );
        // To `java.util.Optional` and back.
        let java = c(
            &mut rt,
            "com/google/common/base/Optional.toJavaUtil.()Ljava/util/Optional;",
            &[some],
        );
        assert_eq!(
            (rt.class_of(java), rt.get_field(java, "value")),
            ("java/util/Optional", a)
        );
        let back = c(
            &mut rt,
            "com/google/common/base/Optional.fromJavaUtil.(Ljava/util/Optional;)Lcom/google/common/base/Optional;",
            &[java],
        );
        assert_eq!(get(&mut rt, back), a);
        let empty = c(
            &mut rt,
            "com/google/common/base/Optional.toJavaUtil.()Ljava/util/Optional;",
            &[absent],
        );
        assert_eq!(rt.get_field(empty, "value"), V::NULL);
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/base/Optional.toJavaUtil.(Lcom/google/common/base/Optional;)Ljava/util/Optional;",
                &[V::NULL]
            ),
            V::NULL
        );
        // Equal by value; Guava's hashes and texts.
        let a2 = rt.string("a");
        let same = of(&mut rt, a2);
        let equals = "com/google/common/base/Optional.equals.(Ljava/lang/Object;)Z";
        assert_eq!(c(&mut rt, equals, &[some, same]), V::I(1));
        assert_eq!(c(&mut rt, equals, &[some, absent]), V::I(0));
        assert_eq!(c(&mut rt, equals, &[absent, absent]), V::I(1));
        let hash = "com/google/common/base/Optional.hashCode.()I";
        assert_eq!(c(&mut rt, hash, &[some]), V::I(0x598d_f91c + 97));
        assert_eq!(c(&mut rt, hash, &[absent]), V::I(0x79a3_1aac));
        let to_string = "com/google/common/base/Optional.toString.()Ljava/lang/String;";
        let shown = c(&mut rt, to_string, &[some]);
        assert_eq!(text(&mut rt, shown), "Optional.of(a)");
        let shown = c(&mut rt, to_string, &[absent]);
        assert_eq!(text(&mut rt, shown), "Optional.absent()");
        // `presentInstances` keeps the present values, in order.
        let b = rt.string("b");
        let some_b = of(&mut rt, b);
        let list = new_list(&mut rt, vec![some, absent, some_b]);
        let present = c(
            &mut rt,
            "com/google/common/base/Optional.presentInstances.(Ljava/lang/Iterable;)Ljava/lang/Iterable;",
            &[list],
        );
        assert_eq!(elements(&rt, present), vec![a, b]);
    }

    fn texts(rt: &Rt, v: V) -> Vec<String> {
        elements(rt, v)
            .into_iter()
            .map(|e| rt.to_java_string(e, 'L'))
            .collect()
    }

    /// `Collections.shuffle` with a seeded `Random` swaps as JDK 21's (its order from seed 42); a
    /// `ListIterator` walks the list itself, setting, adding and removing in place as the JDK's
    /// (the same walk, the same log and list); `AbstractMap.SimpleEntry` is an entry by its key and
    /// value, the immutable one refusing `setValue`.
    #[test]
    fn shuffles_list_iterators_and_simple_entries_are_the_jdks() {
        let rt = &mut Rt::new();
        let ints: Vec<V> = (1..=8).map(|i| crate::api_common::integer(rt, i)).collect();
        let list = new_list(rt, ints);
        let random = rt.new_object("java/util/Random");
        c(rt, "java/util/Random.<init>.(J)V", &[random, V::J(42)]);
        c(
            rt,
            "java/util/Collections.shuffle.(Ljava/util/List;Ljava/util/Random;)V",
            &[list, random],
        );
        assert_eq!(texts(rt, list), ["3", "7", "4", "2", "5", "1", "8", "6"]);

        let letters: Vec<V> = ["a", "b", "c", "d"]
            .into_iter()
            .map(|t| rt.string(t))
            .collect();
        let list = new_list(rt, letters);
        let it = c(
            rt,
            "java/util/List.listIterator.()Ljava/util/ListIterator;",
            &[list],
        );
        let mut log = String::new();
        while c(rt, "java/util/ListIterator.hasNext.()Z", &[it]) == V::I(1) {
            let i = c(rt, "java/util/ListIterator.nextIndex.()I", &[it]).i();
            let e = c(
                rt,
                "java/util/ListIterator.next.()Ljava/lang/Object;",
                &[it],
            );
            let t = rt.str_of(e).unwrap_or("?").to_owned();
            if t == "b" {
                c(rt, "java/util/ListIterator.remove.()V", &[it]);
            } else if t == "c" {
                let (big, more) = (rt.string("C"), rt.string("c2"));
                c(
                    rt,
                    "java/util/ListIterator.set.(Ljava/lang/Object;)V",
                    &[it, big],
                );
                c(
                    rt,
                    "java/util/ListIterator.add.(Ljava/lang/Object;)V",
                    &[it, more],
                );
            }
            log.push_str(&format!("{i}{t} "));
        }
        while c(rt, "java/util/ListIterator.hasPrevious.()Z", &[it]) == V::I(1) {
            let e = c(
                rt,
                "java/util/ListIterator.previous.()Ljava/lang/Object;",
                &[it],
            );
            log.push('<');
            log.push_str(rt.str_of(e).unwrap_or("?"));
        }
        assert_eq!(log, "0a 1b 1c 3d <d<c2<C<a");
        assert_eq!(texts(rt, list), ["a", "C", "c2", "d"]);
        let from_two = c(
            rt,
            "java/util/List.listIterator.(I)Ljava/util/ListIterator;",
            &[list, V::I(2)],
        );
        let e = c(
            rt,
            "java/util/ListIterator.next.()Ljava/lang/Object;",
            &[from_two],
        );
        let before = c(rt, "java/util/ListIterator.previousIndex.()I", &[from_two]);
        assert_eq!((rt.str_of(e), before), (Some("c2"), V::I(2)));
        c(
            rt,
            "java/util/List.listIterator.(I)Ljava/util/ListIterator;",
            &[list, V::I(9)],
        );
        let thrown = rt.thrown.take().unwrap();
        assert_eq!(rt.class_of(thrown), "java/lang/IndexOutOfBoundsException");
        let fresh = c(
            rt,
            "java/util/List.listIterator.()Ljava/util/ListIterator;",
            &[list],
        );
        c(rt, "java/util/ListIterator.remove.()V", &[fresh]);
        assert_eq!(
            rt.thrown.take().map(|e| rt.class_of(e)),
            Some("java/lang/IllegalStateException")
        );

        let (k, one, two) = (
            rt.string("k"),
            crate::api_common::integer(rt, 1),
            crate::api_common::integer(rt, 2),
        );
        let entry = rt.new_object(SIMPLE_ENTRY);
        c(
            rt,
            "java/util/AbstractMap$SimpleEntry.<init>.(Ljava/lang/Object;Ljava/lang/Object;)V",
            &[entry, k, one],
        );
        assert_eq!(rt.to_java_string(entry, 'L'), "k=1");
        assert_eq!(hash(rt, entry), 106);
        let same = c(
            rt,
            "java/util/Map.entry.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map$Entry;",
            &[k, one],
        );
        assert!(equals(rt, entry, same));
        assert_eq!(
            c(
                rt,
                "java/util/Map$Entry.setValue.(Ljava/lang/Object;)Ljava/lang/Object;",
                &[entry, two]
            ),
            one
        );
        assert_eq!(
            c(
                rt,
                "java/util/Map$Entry.getValue.()Ljava/lang/Object;",
                &[entry]
            ),
            two
        );
        let fixed = rt.new_object(SIMPLE_IMMUTABLE_ENTRY);
        c(
            rt,
            "java/util/AbstractMap$SimpleImmutableEntry.<init>.(Ljava/util/Map$Entry;)V",
            &[fixed, same],
        );
        assert_eq!(
            c(
                rt,
                "java/util/Map$Entry.getKey.()Ljava/lang/Object;",
                &[fixed]
            ),
            k
        );
        c(
            rt,
            "java/util/Map$Entry.setValue.(Ljava/lang/Object;)Ljava/lang/Object;",
            &[fixed, two],
        );
        assert_eq!(
            rt.thrown.take().map(|e| rt.class_of(e)),
            Some("java/lang/UnsupportedOperationException")
        );
    }

    /// Streams as JDK 21 makes them: empty ones, two after one another, a builder's, a
    /// spliterator's; a primitive stream's `max`, `min` and `average` optionals (the first of
    /// equal ones; doubles as `Math.max`) and an empty one's sum.
    #[test]
    fn streams_concatenate_build_and_reduce_to_optionals() {
        let rt = &mut Rt::new();
        let ints = |rt: &mut Rt, class: &'static str, xs: Vec<V>| rt.alloc(class, Data::Stream(xs));
        let optional = |rt: &mut Rt, o: V| rt.to_java_string(rt.get_field(o, "$value"), 'L') + if rt.get_field(o, "$present").i() == 0 { " (empty)" } else { "" };
        let s = ints(rt, INT_STREAM, vec![V::I(3), V::I(9), V::I(-2)]);
        let max = c(rt, "java/util/stream/IntStream.max.()Ljava/util/OptionalInt;", &[s]);
        assert_eq!((rt.class_of(max), optional(rt, max)), ("java/util/OptionalInt", "9".into()));
        let empty = c(rt, "java/util/stream/IntStream.empty.()Ljava/util/stream/IntStream;", &[]);
        let min = c(rt, "java/util/stream/IntStream.min.()Ljava/util/OptionalInt;", &[empty]);
        assert_eq!(optional(rt, min), "0 (empty)");
        assert_eq!(c(rt, "java/util/stream/IntStream.sum.()I", &[empty]), V::I(0));
        let s = ints(rt, INT_STREAM, vec![V::I(1), V::I(2), V::I(4)]);
        let avg = c(rt, "java/util/stream/IntStream.average.()Ljava/util/OptionalDouble;", &[s]);
        assert_eq!(rt.get_field(avg, "$value"), V::D(2.3333333333333335));
        let s = ints(rt, LONG_STREAM, vec![V::J(5), V::J(1 << 40)]);
        let max = c(rt, "java/util/stream/LongStream.max.()Ljava/util/OptionalLong;", &[s]);
        assert_eq!((rt.class_of(max), rt.get_field(max, "$value")), ("java/util/OptionalLong", V::J(1099511627776)));
        let s = ints(rt, DOUBLE_STREAM, vec![V::D(-0.0), V::D(0.0)]);
        let max = c(rt, "java/util/stream/DoubleStream.max.()Ljava/util/OptionalDouble;", &[s]);
        assert!(rt.get_field(max, "$value").d().is_sign_positive());
        let s = ints(rt, DOUBLE_STREAM, vec![V::D(1.0), V::D(f64::NAN), V::D(2.0)]);
        let max = c(rt, "java/util/stream/DoubleStream.max.()Ljava/util/OptionalDouble;", &[s]);
        assert!(rt.get_field(max, "$value").d().is_nan());

        let (one, two, three) = (crate::api_common::integer(rt, 1), crate::api_common::integer(rt, 2), crate::api_common::integer(rt, 3));
        let (a, b) = (ints(rt, STREAM, vec![one, two]), ints(rt, STREAM, vec![three]));
        let both = c(rt, "java/util/stream/Stream.concat.(Ljava/util/stream/Stream;Ljava/util/stream/Stream;)Ljava/util/stream/Stream;", &[a, b]);
        assert_eq!(elements(rt, both), [one, two, three]);
        let none = c(rt, "java/util/stream/Stream.empty.()Ljava/util/stream/Stream;", &[]);
        assert_eq!(c(rt, "java/util/stream/Stream.count.()J", &[none]), V::J(0));
        let builder = c(rt, "java/util/stream/Stream.builder.()Ljava/util/stream/Stream$Builder;", &[]);
        let (x, y) = (rt.string("x"), rt.string("y"));
        assert_eq!(c(rt, "java/util/stream/Stream$Builder.add.(Ljava/lang/Object;)Ljava/util/stream/Stream$Builder;", &[builder, x]), builder);
        c(rt, "java/util/stream/Stream$Builder.add.(Ljava/lang/Object;)Ljava/util/stream/Stream$Builder;", &[builder, y]);
        let built = c(rt, "java/util/stream/Stream$Builder.build.()Ljava/util/stream/Stream;", &[builder]);
        assert_eq!(elements(rt, built), [x, y]);
        let list = new_list(rt, vec![one, two]);
        let split = c(rt, "java/util/List.spliterator.()Ljava/util/Spliterator;", &[list]);
        let s = c(rt, "java/util/stream/StreamSupport.stream.(Ljava/util/Spliterator;Z)Ljava/util/stream/Stream;", &[split, V::I(0)]);
        assert_eq!(elements(rt, s), [one, two]);
    }
}
