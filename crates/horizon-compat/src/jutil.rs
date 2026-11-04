//! `java.util` collections in native Rust: lists, hash maps and sets (insertion ordered, so
//! iteration is stable), sorted maps, iterators, map entries, a working subset of streams, and
//! the small helpers mods lean on (`Objects`, `Collections`, `List.of`, atomics).
//!
//! Keys compare as Java compares them: strings and boxed values by content, Horizon's native
//! values (positions, states) by value, and a mod object through its own translated `equals`
//! and `hashCode` when it overrides them, by identity otherwise.
use crate::{Data, Object, Rt, V};

pub use crate::Table;

/// Rebuilds a table's hash index after its entries moved.
fn reindex(t: &mut Table, rt: &mut Rt) {
    t.index.clear();
    for i in 0..t.entries.len() {
        let h = hash(rt, t.entries[i].0);
        t.index.entry(h).or_default().push(i);
    }
}

const EQUALS: &str = "equals.(Ljava/lang/Object;)Z";
const HASH_CODE: &str = "hashCode.()I";
const COMPARE_TO: &str = "compareTo.(Ljava/lang/Object;)I";

fn overrides(rt: &Rt, v: V, key: &str) -> bool {
    rt.overrides.is_some_and(|o| o(rt.class_of(v), key))
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
    // Pairs are values (Registrate keys its pending callbacks by `Pair.of(name, registry)`).
    if is_pair(rt.class_of(a)) && is_pair(rt.class_of(b)) {
        let (l1, l2, r1, r2) = (rt.get_field(a, "$left"), rt.get_field(b, "$left"), rt.get_field(a, "$right"), rt.get_field(b, "$right"));
        return (l1 == l2 || equals(rt, l1, l2)) && (r1 == r2 || equals(rt, r1, r2));
    }
    false
}

/// commons-lang's and DataFixerUpper's pairs (the runtime keeps both as `$left`, `$right`).
fn is_pair(class: &str) -> bool {
    matches!(
        class,
        "org/apache/commons/lang3/tuple/Pair" | "org/apache/commons/lang3/tuple/ImmutablePair" | "org/apache/commons/lang3/tuple/MutablePair" | "com/mojang/datafixers/util/Pair"
    )
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
            // commons-lang's `Pair.hashCode`: the halves' hashes, xor'd.
            Some(Data::Fields(_)) if is_pair(rt.class_of(v)) => {
                let (l, r) = (rt.get_field(v, "$left"), rt.get_field(v, "$right"));
                let half = |rt: &mut Rt, x: V| if x.r() == 0 && matches!(x, V::R(_)) { 0 } else { hash(rt, x) };
                return half(rt, l) ^ half(rt, r);
            }
            _ => {}
        }
    }
    rt.java_hash(v)
}

/// `a.compareTo(b)` (or `comparator.compare(a, b)`), for sorting.
pub fn compare(rt: &mut Rt, comparator: V, a: V, b: V) -> i32 {
    if comparator.r() != 0 {
        return rt.call_functional("java/util/Comparator.compare.(Ljava/lang/Object;Ljava/lang/Object;)I", comparator, &[a, b]).i();
    }
    match (rt.str_of(a), rt.str_of(b)) {
        (Some(x), Some(y)) => {
            let (x, y): (Vec<u16>, Vec<u16>) = (x.encode_utf16().collect(), y.encode_utf16().collect());
            return x.cmp(&y) as i32;
        }
        _ => {}
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
        // Enums: by ordinal.
        _ => (rt.get_field(a, "$ordinal").i()).cmp(&rt.get_field(b, "$ordinal").i()) as i32,
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

fn put_table(rt: &mut Rt, v: V, t: Table) {
    if let Some(o) = rt.heap.get_mut(v.r() as usize) {
        o.data = Data::Table(t);
    }
}

fn find(rt: &mut Rt, t: &Table, k: V) -> Option<usize> {
    let h = hash(rt, k);
    let slots = t.index.get(&h)?.clone();
    slots.into_iter().find(|&i| equals(rt, t.entries[i].0, k))
}

/// Puts `k → v` in the table at `m`; the previous value, or null.
pub fn table_put(rt: &mut Rt, m: V, k: V, v: V) -> V {
    let Some(mut t) = take_table(rt, m) else { return V::NULL };
    let old = match find(rt, &t, k) {
        Some(i) => std::mem::replace(&mut t.entries[i].1, v),
        None => {
            if t.sorted {
                let cmp = t.comparator;
                let mut at = t.entries.len();
                for (i, (e, _)) in t.entries.iter().enumerate() {
                    let e = *e;
                    if compare(rt, cmp, k, e) < 0 {
                        at = i;
                        break;
                    }
                }
                t.entries.insert(at, (k, v));
                reindex(&mut t, rt);
            } else {
                let h = hash(rt, k);
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
    let t = take_table(rt, m)?;
    let r = find(rt, &t, k).map(|i| t.entries[i].1);
    put_table(rt, m, t);
    r
}

fn table_remove(rt: &mut Rt, m: V, k: V) -> Option<V> {
    let mut t = take_table(rt, m)?;
    let r = find(rt, &t, k).map(|i| t.entries.remove(i).1);
    if r.is_some() {
        reindex(&mut t, rt);
    }
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

fn table_entries(rt: &Rt, m: V) -> Vec<(V, V)> {
    match rt.heap.get(m.r() as usize).map(|o| &o.data) {
        Some(Data::Table(t)) => t.entries.clone(),
        _ => Vec::new(),
    }
}

/// The elements of any collection-like value: a list, a set's members, an array, a stream.
pub fn elements(rt: &Rt, v: V) -> Vec<V> {
    match rt.heap.get(v.r() as usize).map(|o| (&o.data, o.class)) {
        Some((Data::List(l), _)) => l.clone(),
        Some((Data::Table(t), c)) if is_map(c) => t.entries.iter().map(|(_, v)| *v).collect(),
        Some((Data::Table(t), _)) => t.entries.iter().map(|(k, _)| *k).collect(),
        Some((Data::Array(a), c)) if c.starts_with('[') => a.clone(),
        Some((Data::Iter(items, at), _)) => items[(*at).min(items.len())..].to_vec(),
        _ => Vec::new(),
    }
}

fn is_map(class: &str) -> bool {
    class.ends_with("Map") || class.ends_with("Map$Values")
}

pub fn new_list(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc("java/util/ArrayList", Data::List(items))
}

/// A `LinkedHashSet` of `items`, in order.
pub fn new_set_of(rt: &mut Rt, items: &[V]) -> V {
    new_set(rt, items)
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

fn entry(rt: &mut Rt, k: V, v: V) -> V {
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
        "ArrayList" | "LinkedList" | "ArrayDeque" | "Vector" | "Stack" | "CopyOnWriteArrayList" | "PriorityQueue"
        | "ConcurrentLinkedQueue" | "ConcurrentLinkedDeque" | "LinkedBlockingQueue" => "list",
        // fastutil's lists and queues of each element type (`ReferenceArrayList`, `LongArrayList`,
        // `IntArrayFIFOQueue`...).
        s if class.starts_with("it/unimi/dsi/fastutil/") && (s.ends_with("ArrayList") || s.ends_with("FIFOQueue") || s.ends_with("BigArrayBigList")) => "list",
        "TreeMap" | "TreeSet" | "ConcurrentSkipListMap" | "ConcurrentSkipListSet" => "sorted",
        s if s.ends_with("Map") || s.ends_with("Set") => "table",
        _ => return None,
    })
}

fn util_owner(owner: &str) -> bool {
    owner.starts_with("java/util/")
        || owner == NON_NULL_LIST
        || owner == "java/lang/Iterable"
        || owner.starts_with("it/unimi/dsi/fastutil/")
        || owner.starts_with("com/google/common/collect/")
}

/// A `java.util` call (`owner.name.desc`), or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Statics first.
    if let Some(v) = statics(rt, owner, name, desc, args) {
        return Some(v);
    }
    if matches!(owner, "java/util/concurrent/CompletableFuture" | "java/util/concurrent/Future" | "java/util/concurrent/CompletionStage") {
        return future_call(rt, name, args);
    }
    if owner.starts_with("com/google/common/cache/") {
        return cache_call(rt, owner, name, args);
    }
    if matches!(owner, "java/util/Random" | "java/util/concurrent/ThreadLocalRandom") {
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
    if !util_owner(owner) {
        return None;
    }
    if name == "<init>" {
        let this = a(0);
        let kind = kind_of(owner)?;
        let class = rt.class_of(this);
        let data = match kind {
            "list" => Data::List(Vec::new()),
            _ => Data::Table(Table { sorted: kind == "sorted", ..Table::default() }),
        };
        if let Some(o) = rt.heap.get_mut(this.r() as usize) {
            o.data = data;
            o.class = class;
        }
        // A copy constructor, or a sorted collection's comparator.
        if let Some(&arg) = args.get(1)
            && arg.r() != 0
            && desc.starts_with("(L")
        {
            let src_is_map = is_map(rt.class_of(arg)) && matches!(rt.heap.get(arg.r() as usize).map(|o| &o.data), Some(Data::Table(_)));
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
        'l' if rt.class_of(this) == "java/util/Map$Entry" => entry_call(rt, name, args),
        'l' => list_call(rt, name, desc, args),
        't' if is_map(rt.class_of(this)) => map_call(rt, name, desc, args),
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
            V::J(table_entries(rt, m).len() as i64)
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
        ("java/util/Set" | "com/google/common/collect/ImmutableSet" | "com/google/common/collect/Sets", "of" | "copyOf" | "newHashSet") => {
            let items = if name == "copyOf" || desc.starts_with("(Ljava/lang/Iterable") { elements(rt, a(0)) } else { varargs(rt) };
            new_set(rt, &items)
        }
        ("java/util/Map" | "com/google/common/collect/ImmutableMap" | "com/google/common/collect/Maps", "of" | "newHashMap" | "newLinkedHashMap") => {
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()));
            for kv in args.chunks(2) {
                if let [k, v] = kv {
                    table_put(rt, m, *k, *v);
                }
            }
            m
        }
        ("java/util/Map", "entry") => entry(rt, a(0), a(1)),
        // Guava: a row → (column → value) table, and key → collection multimaps.
        (o, "create") if (o.ends_with("BasedTable") || o.ends_with("Multimap")) && o.starts_with("com/google/common/collect/") => {
            let class: &'static str = rt.static_key(o);
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
        ("java/util/Collections", n) if n.starts_with("unmodifiable") || n.starts_with("synchronized") => a(0),
        // fastutil's wrappers (`Object2ReferenceMaps.synchronize(map)`, `ObjectSets.unmodifiable(set)`):
        // one thread's view of the same collection.
        (o, "synchronize" | "unmodifiable") if o.starts_with("it/unimi/dsi/fastutil/") && args.len() == 1 => a(0),
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
        // Guava's `HashBiMap.create()`: a map (its `inverse()` is a copy, read after it's filled).
        ("com/google/common/collect/HashBiMap", "create") => rt.alloc("com/google/common/collect/HashBiMap", Data::Table(Table::default())),
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
        ("java/util/Collections", "reverse") => {
            if let Some(l) = list_mut(rt, a(0)) {
                l.reverse();
            }
            V::Void
        }
        ("java/util/Objects", "requireNonNull" | "requireNonNullElse") => {
            if a(0).r() == 0 && name == "requireNonNullElse" { a(1) } else { a(0) }
        }
        ("java/util/Objects", "equals") => V::I(equals(rt, a(0), a(1)) as i32),
        ("java/util/Objects", "hashCode") => V::I(if a(0).r() == 0 { 0 } else { hash(rt, a(0)) }),
        ("java/util/Objects", "hash") => {
            let items = elements(rt, a(0));
            V::I(items.into_iter().fold(1i32, |h, v| h.wrapping_mul(31).wrapping_add(if v.r() == 0 { 0 } else { hash(rt, v) })))
        }
        ("java/util/Objects", "isNull") => V::I((a(0).r() == 0) as i32),
        ("java/util/Objects", "nonNull") => V::I((a(0).r() != 0) as i32),
        ("java/util/stream/Collectors", "toList" | "toSet" | "toUnmodifiableList" | "toUnmodifiableSet") => {
            rt.alloc(if name.contains("Set") { "compat:collect-set" } else { "compat:collect-list" }, Data::List(Vec::new()))
        }
        ("java/util/stream/Stream", "of") => {
            let items = varargs(rt);
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
        }
        ("java/util/Arrays", "stream") => {
            let items = elements(rt, a(0));
            rt.alloc("java/util/stream/Stream", Data::Stream(items))
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
                "get" | "intValue" | "longValue" | "getPlain" | "getAcquire" => cur,
                "set" | "lazySet" | "setPlain" | "setRelease" => {
                    rt.put_field(this, "value", a(1));
                    V::Void
                }
                "getAndSet" => {
                    rt.put_field(this, "value", a(1));
                    cur
                }
                "compareAndSet" => {
                    let same = cur == a(1) || (owner.ends_with("Reference") && equals(rt, cur, a(1)));
                    if same {
                        rt.put_field(this, "value", a(2));
                    }
                    V::I(same as i32)
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
        "getFirst" | "peek" | "peekFirst" | "element" | "first" => list(rt, this)?.first().copied().unwrap_or(V::NULL),
        "getLast" | "peekLast" | "last" => list(rt, this)?.last().copied().unwrap_or(V::NULL),
        "add" | "addLast" | "offer" | "offerLast" | "push" | "addFirst" | "offerFirst" if desc.starts_with("(Ljava/lang/Object;)") => {
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
        "iterator" | "listIterator" | "descendingIterator" => {
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
        "forEach" => {
            for e in list(rt, this)?.clone() {
                rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[e]);
            }
            V::Void
        }
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
            rt.alloc("[Ljava/lang/Object;", Data::Array(items))
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
            let key = if rt.class_of(item) == "java/util/Map$Entry" { list(rt, item).and_then(|l| l.first().copied()).unwrap_or(item) } else { item };
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
    let len = table_entries(rt, this).len();
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
            put_table(rt, this, Table { sorted: matches!(rt.heap.get(this.r() as usize).map(|o| &o.data), Some(Data::Table(t)) if t.sorted), ..Table::default() });
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
        "toArray" => {
            let items = elements(rt, this);
            rt.alloc("[Ljava/lang/Object;", Data::Array(items))
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

fn map_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    // A stored value, or what a missing key gives (`missing`).
    let found = |rt: &Rt, v: Option<V>| v.unwrap_or_else(|| missing(rt, this, desc));
    Some(match name {
        "size" => V::I(table_entries(rt, this).len() as i32),
        "isEmpty" => V::I(table_entries(rt, this).is_empty() as i32),
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
        // A `BiMap`'s other way round: a copy with keys and values swapped.
        "inverse" if args.len() == 1 => {
            let class = rt.class_of(this);
            let inv = rt.alloc(class, Data::Table(Table::default()));
            for (k, v) in table_entries(rt, this) {
                table_put(rt, inv, v, k);
            }
            inv
        }
        "containsKey" => V::I(table_get(rt, this, a(1)).is_some() as i32),
        "containsValue" => {
            let vals: Vec<V> = table_entries(rt, this).into_iter().map(|(_, v)| v).collect();
            V::I(vals.into_iter().any(|v| equals(rt, v, a(2 - 1))) as i32)
        }
        "put" => {
            let had = table_get(rt, this, a(1));
            table_put(rt, this, a(1), a(2));
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
        "remove" => {
            let v = table_remove(rt, this, a(1));
            found(rt, v)
        }
        "clear" => {
            let (sorted, comparator) = match rt.heap.get(this.r() as usize).map(|o| &o.data) {
                Some(Data::Table(t)) => (t.sorted, t.comparator),
                _ => (false, V::NULL),
            };
            put_table(rt, this, Table { sorted, comparator, ..Table::default() });
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
        "values" => {
            let vals: Vec<V> = table_entries(rt, this).into_iter().map(|(_, v)| v).collect();
            new_list(rt, vals)
        }
        // fastutil's typed entry sets (`int2ObjectEntrySet`, `object2IntEntrySet`...) are its entry set.
        n if n == "entrySet" || (n.ends_with("EntrySet") && n.contains('2')) => {
            let es: Vec<(V, V)> = table_entries(rt, this);
            let mut items = Vec::with_capacity(es.len());
            for (k, v) in es {
                let e = entry(rt, k, v);
                rt.iter_sources.insert(e.r(), this);
                items.push(e);
            }
            let s = rt.alloc("java/util/LinkedHashSet", Data::Table(Table::default()));
            if let Some(Data::Table(t)) = rt.heap.get_mut(s.r() as usize).map(|o| &mut o.data) {
                // Entries are distinct objects: identity keys, no hashing callbacks needed.
                for (i, e) in items.iter().enumerate() {
                    t.entries.push((*e, V::I(1)));
                    t.index.entry(e.r() as i32).or_default().push(i);
                }
            }
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

fn guava_table_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
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
    Some(match name {
        "map" | "mapToObj" | "mapToInt" | "mapToLong" | "mapToDouble" => {
            let mut out = Vec::with_capacity(items.len());
            for e in items {
                out.push(rt.call_functional(APPLY, a(1), &[e]));
            }
            stream(rt, out)
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
        "boxed" | "sequential" | "parallel" | "unordered" | "onClose" => this,
        "forEach" | "forEachOrdered" => {
            for e in items {
                rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[e]);
            }
            V::Void
        }
        "toList" => new_list(rt, items),
        "toArray" => rt.alloc("[Ljava/lang/Object;", Data::Array(items)),
        "collect" => match rt.class_of(a(1)) {
            "compat:collect-set" => new_set(rt, &items),
            _ => new_list(rt, items),
        },
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
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"))
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
        let set = rt.new_object("it/unimi/dsi/fastutil/ints/IntOpenHashSet");
        c(&mut rt, "it/unimi/dsi/fastutil/ints/IntOpenHashSet.<init>.()V", &[set]);
        c(&mut rt, "it/unimi/dsi/fastutil/ints/IntSet.add.(I)Z", &[set, V::I(7)]);
        let it = c(&mut rt, "it/unimi/dsi/fastutil/ints/IntSet.iterator.()Lit/unimi/dsi/fastutil/ints/IntIterator;", &[set]);
        let n = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
        crate::install();
        assert_eq!(crate::api::call(&mut rt, "java/lang/Integer.intValue.()I", &[n]), V::I(7));
        assert_eq!(crate::api::call(&mut rt, "java/lang/Number.longValue.()J", &[n]), V::J(7));
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
        let m = c(&mut rt, "com/google/common/collect/ArrayListMultimap.create.()Lcom/google/common/collect/ArrayListMultimap;", &[]);
        assert_eq!(c(&mut rt, "com/google/common/collect/Multimap.isEmpty.()Z", &[m]), V::I(1));
        c(&mut rt, "com/google/common/collect/Multimap.put.(Ljava/lang/Object;Ljava/lang/Object;)Z", &[m, block, V::I(7)]);
        c(&mut rt, "com/google/common/collect/Multimap.put.(Ljava/lang/Object;Ljava/lang/Object;)Z", &[m, block, V::I(8)]);
        let got = c(&mut rt, "com/google/common/collect/Multimap.get.(Ljava/lang/Object;)Ljava/util/Collection;", &[m, block]);
        assert_eq!(elements(&rt, got), [V::I(7), V::I(8)]);
        assert_eq!(c(&mut rt, "com/google/common/collect/Multimap.size.()I", &[m]), V::I(2));
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
    }
}
