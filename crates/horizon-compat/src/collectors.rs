//! Collectors: `java.util.stream.Collectors`'s, `Collector.of`'s, Guava's `toImmutableList`,
//! `toImmutableSet` and `toImmutableMap`, and a mod's own `Collector` class. Each of the
//! runtime's is an object of class `compat:collect-<kind>` holding what it was made with (its
//! functions, its downstream collector); `Stream.collect` runs it over the stream's elements
//! ([`collect`]), a downstream one over each group's. The functions run element by element in
//! the stream's order, a downstream's after its group is complete.
use crate::jutil::{elements, new_list, table_get, table_put};
use crate::{Data, Rt, Table, V};

const COLLECTORS: &str = "java/util/stream/Collectors";
const LIST: &str = "compat:collect-list";
const SET: &str = "compat:collect-set";
const JOIN: &str = "compat:collect-join";
/// `toMap` and its kin: key function, value function, merge (null: a key twice throws), map
/// factory (null: a `HashMap`), and whether a key twice is Guava's error (1) or Java's (0).
const MAP: &str = "compat:collect-map";
const TO: &str = "compat:collect-to";
const GROUP: &str = "compat:collect-group";
const PARTITION: &str = "compat:collect-partition";
const THEN: &str = "compat:collect-then";
const MAPPING: &str = "compat:collect-mapping";
const FILTERING: &str = "compat:collect-filtering";
const FLAT_MAPPING: &str = "compat:collect-flat-mapping";
const COUNTING: &str = "compat:collect-counting";
const SUMMING: &str = "compat:collect-summing";
const AVERAGING: &str = "compat:collect-averaging";
const MIN_MAX: &str = "compat:collect-min-max";
const REDUCING: &str = "compat:collect-reducing";
const OF: &str = "compat:collect-of";

const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const BI_APPLY: &str =
    "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const GET: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const ACCEPT: &str = "java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V";
const ADD: (&str, &str) = (
    "java/util/Collection.add.(Ljava/lang/Object;)Z",
    "add.(Ljava/lang/Object;)Z",
);
const PUT: (&str, &str) = (
    "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
);
const MERGE: (&str, &str) = (
    "java/util/Map.merge.(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
    "merge.(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
);
const SUPPLIER: (&str, &str) = (
    "java/util/stream/Collector.supplier.()Ljava/util/function/Supplier;",
    "supplier.()Ljava/util/function/Supplier;",
);
const ACCUMULATOR: (&str, &str) = (
    "java/util/stream/Collector.accumulator.()Ljava/util/function/BiConsumer;",
    "accumulator.()Ljava/util/function/BiConsumer;",
);
const FINISHER: (&str, &str) = (
    "java/util/stream/Collector.finisher.()Ljava/util/function/Function;",
    "finisher.()Ljava/util/function/Function;",
);

fn made(rt: &mut Rt, kind: &'static str, parts: Vec<V>) -> V {
    rt.alloc(kind, Data::List(parts))
}

/// Whether [`make`] may answer a static call on `owner` named `name`.
pub(crate) fn makes(owner: &str, name: &str) -> bool {
    owner == COLLECTORS
        || (owner == "java/util/stream/Collector" && name == "of")
        || (owner.starts_with("com/google/common/collect/Immutable")
            && name.starts_with("toImmutable"))
}

/// A collector a static factory makes (`Collectors.groupingBy(f)`, `Collector.of(...)`,
/// `ImmutableList.toImmutableList()`), or None for one this module doesn't make.
pub(crate) fn make(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let n = args.len();
    let kind = |c: char| V::I(c as i32);
    Some(match (owner, name) {
        (COLLECTORS, "toList" | "toUnmodifiableList") | (_, "toImmutableList") => {
            made(rt, LIST, vec![])
        }
        (COLLECTORS, "toSet" | "toUnmodifiableSet") | (_, "toImmutableSet") => {
            made(rt, SET, vec![])
        }
        // `joining([separator[, prefix, suffix]])`.
        (COLLECTORS, "joining") => made(rt, JOIN, args.to_vec()),
        (COLLECTORS, "toMap" | "toUnmodifiableMap" | "toConcurrentMap") => {
            let factory = if n == 4 { a(3) } else { V::NULL };
            made(
                rt,
                MAP,
                vec![
                    a(0),
                    a(1),
                    if n >= 3 { a(2) } else { V::NULL },
                    factory,
                    V::I(0),
                ],
            )
        }
        (_, "toImmutableMap" | "toImmutableBiMap") if n == 2 => {
            made(rt, MAP, vec![a(0), a(1), V::NULL, V::NULL, V::I(1)])
        }
        (COLLECTORS, "toCollection") => made(rt, TO, vec![a(0)]),
        // `groupingBy(classifier[, [mapFactory,] downstream])`.
        (COLLECTORS, "groupingBy" | "groupingByConcurrent") => match n {
            1 => made(rt, GROUP, vec![a(0), V::NULL, V::NULL]),
            2 => made(rt, GROUP, vec![a(0), V::NULL, a(1)]),
            _ => made(rt, GROUP, vec![a(0), a(1), a(2)]),
        },
        (COLLECTORS, "partitioningBy") => made(
            rt,
            PARTITION,
            vec![a(0), if n == 2 { a(1) } else { V::NULL }],
        ),
        (COLLECTORS, "collectingAndThen") => made(rt, THEN, vec![a(0), a(1)]),
        (COLLECTORS, "mapping") => made(rt, MAPPING, vec![a(0), a(1)]),
        (COLLECTORS, "filtering") => made(rt, FILTERING, vec![a(0), a(1)]),
        (COLLECTORS, "flatMapping") => made(rt, FLAT_MAPPING, vec![a(0), a(1)]),
        (COLLECTORS, "counting") => made(rt, COUNTING, vec![]),
        (COLLECTORS, "summingInt") => made(rt, SUMMING, vec![a(0), kind('I')]),
        (COLLECTORS, "summingLong") => made(rt, SUMMING, vec![a(0), kind('J')]),
        (COLLECTORS, "summingDouble") => made(rt, SUMMING, vec![a(0), kind('D')]),
        (COLLECTORS, "averagingInt") => made(rt, AVERAGING, vec![a(0), kind('I')]),
        (COLLECTORS, "averagingLong") => made(rt, AVERAGING, vec![a(0), kind('J')]),
        (COLLECTORS, "averagingDouble") => made(rt, AVERAGING, vec![a(0), kind('D')]),
        (COLLECTORS, "minBy") => made(rt, MIN_MAX, vec![a(0), V::I(0)]),
        (COLLECTORS, "maxBy") => made(rt, MIN_MAX, vec![a(0), V::I(1)]),
        // `reducing(op)` (an optional), `reducing(identity, op)`, `reducing(identity, mapper, op)`.
        (COLLECTORS, "reducing") => match n {
            1 => made(rt, REDUCING, vec![V::Void, V::NULL, a(0)]),
            2 => made(rt, REDUCING, vec![a(0), V::NULL, a(1)]),
            _ => made(rt, REDUCING, vec![a(0), a(1), a(2)]),
        },
        // `Collector.of(supplier, accumulator, combiner[, finisher], characteristics...)`.
        ("java/util/stream/Collector", "of") => {
            let finisher = if desc.contains("Ljava/util/function/Function;") {
                a(3)
            } else {
                V::NULL
            };
            made(rt, OF, vec![a(0), a(1), finisher])
        }
        _ => return None,
    })
}

/// `Optional.ofNullable(v)`; `None` is empty.
fn optional(rt: &mut Rt, v: Option<V>) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", v.unwrap_or(V::NULL));
    o
}

fn new_map(rt: &mut Rt, class: &'static str) -> V {
    rt.alloc(class, Data::Table(Table::default()))
}

fn is_table(rt: &Rt, v: V) -> bool {
    matches!(
        rt.heap.get(v.r() as usize).map(|o| &o.data),
        Some(Data::Table(_))
    )
}

/// `map.put(k, v)`: the runtime's map, or a mod's own through its `put`.
fn map_put(rt: &mut Rt, m: V, k: V, v: V) {
    if is_table(rt, m) {
        table_put(rt, m, k, v);
    } else {
        crate::api::virtual_call(rt, PUT, &[m, k, v]);
    }
}

/// `stream.collect(collector)`: what `collector` makes of `items`. An unknown one (an unmapped
/// factory's stand-in) gives a list, as `toList()` would.
pub(crate) fn collect(rt: &mut Rt, collector: V, items: Vec<V>) -> V {
    let parts = match rt.heap.get(collector.r() as usize).map(|o| &o.data) {
        Some(Data::List(p)) => p.clone(),
        _ => Vec::new(),
    };
    let p = |i: usize| parts.get(i).copied().unwrap_or(V::NULL);
    let class = rt.class_of(collector);
    match class {
        LIST => new_list(rt, items),
        SET => crate::jutil::new_set_of(rt, &items),
        JOIN => join(rt, &parts, &items),
        MAP => to_map(rt, [p(0), p(1), p(2), p(3)], p(4).i() == 1, items),
        TO => {
            let into = rt.call_functional(GET, p(0), &[]);
            for e in items {
                if rt.thrown.is_some() {
                    break;
                }
                crate::api::virtual_call(rt, ADD, &[into, e]);
            }
            into
        }
        GROUP => group(rt, p(0), p(1), p(2), items),
        PARTITION => {
            let (mut no, mut yes) = (Vec::new(), Vec::new());
            for e in items {
                let t = rt.call_functional(TEST, p(0), &[e]).i() != 0;
                if rt.thrown.is_some() {
                    return V::NULL;
                }
                if t { yes.push(e) } else { no.push(e) }
            }
            // Java's partition map holds `false` then `true`, each a list unless a downstream
            // collector is given.
            let m = new_map(rt, "java/util/LinkedHashMap");
            for (b, group) in [(0, no), (1, yes)] {
                let k = crate::mixin::box_value(rt, V::I(b), 'Z');
                let v = downstream(rt, p(1), group);
                table_put(rt, m, k, v);
            }
            m
        }
        THEN => {
            let r = collect(rt, p(0), items);
            if rt.thrown.is_some() {
                return V::NULL;
            }
            rt.call_functional(APPLY, p(1), &[r])
        }
        MAPPING | FILTERING | FLAT_MAPPING => {
            let mut out = Vec::with_capacity(items.len());
            for e in items {
                match class {
                    MAPPING => out.push(rt.call_functional(APPLY, p(0), &[e])),
                    FILTERING => {
                        if rt.call_functional(TEST, p(0), &[e]).i() != 0 {
                            out.push(e);
                        }
                    }
                    _ => {
                        let s = rt.call_functional(APPLY, p(0), &[e]);
                        out.extend(elements(rt, s));
                    }
                }
                if rt.thrown.is_some() {
                    return V::NULL;
                }
            }
            collect(rt, p(1), out)
        }
        COUNTING => crate::mixin::box_value(rt, V::J(items.len() as i64), 'J'),
        SUMMING | AVERAGING => numbers(
            rt,
            class == AVERAGING,
            p(0),
            char::from_u32(p(1).i() as u32).unwrap_or('I'),
            items,
        ),
        MIN_MAX => {
            let max = p(1).i() == 1;
            let mut best: Option<V> = None;
            for e in items {
                best = Some(match best {
                    None => e,
                    Some(b) => {
                        // `BinaryOperator.minBy`/`maxBy`: of equal ones, the first.
                        let c = crate::jutil::compare(rt, p(0), b, e);
                        if rt.thrown.is_some() {
                            return V::NULL;
                        }
                        if (max && c >= 0) || (!max && c <= 0) {
                            b
                        } else {
                            e
                        }
                    }
                });
            }
            optional(rt, best)
        }
        REDUCING => {
            let (identity, mapper, op) = (p(0), p(1), p(2));
            let mut acc = (!matches!(identity, V::Void)).then_some(identity);
            for e in items {
                let e = if mapper.r() != 0 {
                    rt.call_functional(APPLY, mapper, &[e])
                } else {
                    e
                };
                acc = Some(match acc {
                    None => e,
                    Some(x) => rt.call_functional(BI_APPLY, op, &[x, e]),
                });
                if rt.thrown.is_some() {
                    return V::NULL;
                }
            }
            if matches!(identity, V::Void) {
                optional(rt, acc)
            } else {
                acc.unwrap_or(V::NULL)
            }
        }
        OF => run(rt, p(0), p(1), p(2), items),
        _ if collector.r() == 0 => new_list(rt, items),
        // A mod's own `Collector` class: its supplier, accumulator and finisher.
        _ => {
            let supplier = crate::api::virtual_call(rt, SUPPLIER, &[collector]);
            let accumulator = crate::api::virtual_call(rt, ACCUMULATOR, &[collector]);
            let finisher = crate::api::virtual_call(rt, FINISHER, &[collector]);
            if rt.thrown.is_some() {
                return V::NULL;
            }
            run(rt, supplier, accumulator, finisher, items)
        }
    }
}

/// A collector's parts at work: a container from `supplier`, each element given to it by
/// `accumulator`, then `finisher` (none: the container is the result).
fn run(rt: &mut Rt, supplier: V, accumulator: V, finisher: V, items: Vec<V>) -> V {
    let container = rt.call_functional(GET, supplier, &[]);
    for e in items {
        if rt.thrown.is_some() {
            return V::NULL;
        }
        rt.call_functional(ACCEPT, accumulator, &[container, e]);
    }
    if finisher.r() == 0 || rt.thrown.is_some() {
        return container;
    }
    match rt.call_functional(APPLY, finisher, &[container]) {
        V::Void => container,
        r => r,
    }
}

/// A group's or a partition's downstream result: its collector's, else the group as a list.
fn downstream(rt: &mut Rt, collector: V, group: Vec<V>) -> V {
    if collector.r() == 0 {
        new_list(rt, group)
    } else {
        collect(rt, collector, group)
    }
}

fn join(rt: &mut Rt, parts: &[V], items: &[V]) -> V {
    let text = |rt: &Rt, v: Option<&V>| v.and_then(|v| rt.str_of(*v)).unwrap_or("").to_owned();
    let joined: Vec<String> = items
        .iter()
        .map(|e| rt.str_of(*e).unwrap_or("null").to_owned())
        .collect();
    let s = format!(
        "{}{}{}",
        text(rt, parts.get(1)),
        joined.join(&text(rt, parts.first())),
        text(rt, parts.get(2))
    );
    rt.alloc("java/lang/String", Data::Str(s))
}

/// `toMap(key, value[, merge[, mapFactory]])`: each element's key and value put in the map; a key
/// met again merges the values (a null merge result removes it), or without a merge throws
/// Java's `IllegalStateException` (Guava's builder: `IllegalArgumentException`).
fn to_map(rt: &mut Rt, [key, value, merge, factory]: [V; 4], guava: bool, items: Vec<V>) -> V {
    let m = if factory.r() != 0 {
        rt.call_functional(GET, factory, &[])
    } else {
        new_map(rt, "java/util/HashMap")
    };
    let table = is_table(rt, m);
    for e in items {
        let k = rt.call_functional(APPLY, key, &[e]);
        let v = rt.call_functional(APPLY, value, &[e]);
        if rt.thrown.is_some() {
            return V::NULL;
        }
        if !table {
            if merge.r() != 0 {
                crate::api::virtual_call(rt, MERGE, &[m, k, v, merge]);
            } else {
                crate::api::virtual_call(rt, PUT, &[m, k, v]);
            }
            continue;
        }
        match table_get(rt, m, k) {
            Some(old) if merge.r() != 0 => {
                let merged = rt.call_functional(BI_APPLY, merge, &[old, v]);
                if rt.thrown.is_some() {
                    return V::NULL;
                }
                if merged.is_null() {
                    crate::jutil::table_remove(rt, m, k);
                } else {
                    table_put(rt, m, k, merged);
                }
            }
            Some(old) => {
                let shown = |rt: &mut Rt, v: V| rt.to_java_string(v, 'L');
                let (ks, olds, vs) = (shown(rt, k), shown(rt, old), shown(rt, v));
                let (class, msg) = if guava {
                    (
                        "java/lang/IllegalArgumentException",
                        format!("Multiple entries with same key: {ks}={vs} and {ks}={olds}"),
                    )
                } else {
                    (
                        "java/lang/IllegalStateException",
                        format!("Duplicate key {ks} (attempted merging values {olds} and {vs})"),
                    )
                };
                crate::jlang::throw_with(rt, class, &msg);
                return V::NULL;
            }
            None => {
                table_put(rt, m, k, v);
            }
        }
    }
    m
}

/// `groupingBy(classifier, mapFactory, downstream)`: the elements by their key, in the order each
/// key was first met, each group collected by `downstream` (none: a list). A null key throws, as
/// Java's does.
fn group(rt: &mut Rt, classifier: V, factory: V, downstream_collector: V, items: Vec<V>) -> V {
    // Where each key's group is, by the key's own `hashCode` and `equals`.
    let index = new_map(rt, "java/util/HashMap");
    let mut groups: Vec<(V, Vec<V>)> = Vec::new();
    for e in items {
        let k = rt.call_functional(APPLY, classifier, &[e]);
        if rt.thrown.is_some() {
            return V::NULL;
        }
        if k.is_null() {
            crate::jlang::throw_with(
                rt,
                "java/lang/NullPointerException",
                "element cannot be mapped to a null key",
            );
            return V::NULL;
        }
        match table_get(rt, index, k) {
            Some(i) => groups[i.i() as usize].1.push(e),
            None => {
                table_put(rt, index, k, V::I(groups.len() as i32));
                groups.push((k, vec![e]));
            }
        }
    }
    let m = if factory.r() != 0 {
        rt.call_functional(GET, factory, &[])
    } else {
        new_map(rt, "java/util/HashMap")
    };
    for (k, g) in groups {
        let v = downstream(rt, downstream_collector, g);
        if rt.thrown.is_some() {
            return V::NULL;
        }
        map_put(rt, m, k, v);
    }
    m
}

/// `summingInt`/`Long`/`Double` and `averagingInt`/`Long`/`Double`: the boxed sum (an int's wraps,
/// a double's is Java's compensated sum) or the average as a `Double` (0 for none).
fn numbers(rt: &mut Rt, average: bool, f: V, kind: char, items: Vec<V>) -> V {
    let n = items.len();
    let key = match kind {
        'I' => "java/util/function/ToIntFunction.applyAsInt.(Ljava/lang/Object;)I",
        'J' => "java/util/function/ToLongFunction.applyAsLong.(Ljava/lang/Object;)J",
        _ => "java/util/function/ToDoubleFunction.applyAsDouble.(Ljava/lang/Object;)D",
    };
    let (mut int, mut long) = (0i32, 0i64);
    // Java's `Collectors.sumWithCompensation`: the sum, its compensation, the plain sum.
    let (mut sum, mut comp, mut simple) = (0f64, 0f64, 0f64);
    for e in items {
        let v = rt.call_functional(key, f, &[e]);
        if rt.thrown.is_some() {
            return V::NULL;
        }
        match kind {
            'I' => {
                int = int.wrapping_add(v.i());
                long = long.wrapping_add(i64::from(v.i()));
            }
            'J' => long = long.wrapping_add(v.j()),
            _ => {
                let x = v.d();
                let tmp = x - comp;
                let velvel = sum + tmp;
                comp = (velvel - sum) - tmp;
                sum = velvel;
                simple += x;
            }
        }
    }
    // `Collectors.computeFinalSum`.
    let double_sum = || {
        let tmp = sum - comp;
        if tmp.is_nan() && simple.is_infinite() {
            simple
        } else {
            tmp
        }
    };
    if average {
        let avg = if n == 0 {
            0.0
        } else {
            match kind {
                'I' | 'J' => long as f64 / n as f64,
                _ => double_sum() / n as f64,
            }
        };
        return crate::mixin::box_value(rt, V::D(avg), 'D');
    }
    match kind {
        'I' => crate::mixin::box_value(rt, V::I(int), 'I'),
        'J' => crate::mixin::box_value(rt, V::J(long), 'J'),
        _ => crate::mixin::box_value(rt, V::D(double_sum()), 'D'),
    }
}

/// `stream.collect(supplier, accumulator, combiner)`: a container from `supplier`, each element
/// given to it (a primitive stream's to an `ObjIntConsumer`...). `class` is the stream's.
pub(crate) fn collect_into(
    rt: &mut Rt,
    class: &str,
    supplier: V,
    accumulator: V,
    items: Vec<V>,
) -> V {
    let key = match class {
        "java/util/stream/IntStream" => {
            "java/util/function/ObjIntConsumer.accept.(Ljava/lang/Object;I)V"
        }
        "java/util/stream/LongStream" => {
            "java/util/function/ObjLongConsumer.accept.(Ljava/lang/Object;J)V"
        }
        "java/util/stream/DoubleStream" => {
            "java/util/function/ObjDoubleConsumer.accept.(Ljava/lang/Object;D)V"
        }
        _ => ACCEPT,
    };
    let container = rt.call_functional(GET, supplier, &[]);
    for e in items {
        if rt.thrown.is_some() {
            break;
        }
        rt.call_functional(key, accumulator, &[container, e]);
    }
    container
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    fn strings(rt: &mut Rt, xs: &[&str]) -> Vec<V> {
        xs.iter().map(|x| rt.string(x)).collect()
    }

    fn stream(rt: &mut Rt, items: Vec<V>) -> V {
        rt.alloc("java/util/stream/Stream", Data::Stream(items))
    }

    const COLLECT: &str =
        "java/util/stream/Stream.collect.(Ljava/util/stream/Collector;)Ljava/lang/Object;";
    const LENGTH: &str = "java/lang/String.length.()I";

    fn text(rt: &mut Rt, v: V) -> String {
        rt.to_java_string(v, 'L')
    }

    /// A runtime whose functions (`String::length`) answer through the API, as the generated
    /// dispatch makes them.
    fn runtime() -> Rt {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        rt
    }

    /// `groupingBy(String::length)` groups words by length (each group a list in the stream's
    /// order); with `counting()` downstream each group is a `Long` count; `summingInt` sums the
    /// lengths into an `Integer`.
    #[test]
    fn grouping_by_collects_each_group_downstream() {
        let mut rt = runtime();
        let length = crate::functions::method(&mut rt, LENGTH);
        let words = strings(&mut rt, &["ab", "c", "de", "fgh", "i"]);
        let s = stream(&mut rt, words.clone());
        let by = c(
            &mut rt,
            "java/util/stream/Collectors.groupingBy.(Ljava/util/function/Function;)Ljava/util/stream/Collector;",
            &[length],
        );
        let m = c(&mut rt, COLLECT, &[s, by]);
        assert_eq!(crate::jutil::table_pairs(&rt, m).len(), 3);
        for (k, want) in [(2, "[ab, de]"), (1, "[c, i]"), (3, "[fgh]")] {
            let group = table_get(&mut rt, m, V::I(k)).expect("a group per length");
            assert_eq!(text(&mut rt, group), want);
        }
        let counting = c(
            &mut rt,
            "java/util/stream/Collectors.counting.()Ljava/util/stream/Collector;",
            &[],
        );
        let by = c(
            &mut rt,
            "java/util/stream/Collectors.groupingBy.(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
            &[length, counting],
        );
        let s = stream(&mut rt, words.clone());
        let m = c(&mut rt, COLLECT, &[s, by]);
        let two = table_get(&mut rt, m, V::I(2)).expect("two words of 2");
        assert_eq!(
            (rt.class_of(two), rt.get_field(two, "value")),
            ("java/lang/Long", V::J(2))
        );
        let summing = c(
            &mut rt,
            "java/util/stream/Collectors.summingInt.(Ljava/util/function/ToIntFunction;)Ljava/util/stream/Collector;",
            &[length],
        );
        let s = stream(&mut rt, words);
        let total = c(&mut rt, COLLECT, &[s, summing]);
        assert_eq!(
            (rt.class_of(total), rt.get_field(total, "value")),
            ("java/lang/Integer", V::I(9))
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `toCollection(TreeSet::new)` collects into the collection its supplier makes (sorted here);
    /// `partitioningBy` splits into `false` and `true`; `collectingAndThen` finishes the result.
    #[test]
    fn to_collection_partitioning_and_finishing() {
        let mut rt = runtime();
        let words = strings(&mut rt, &["pear", "fig", "apple", "kiwi"]);
        let tree = crate::functions::constructor(&mut rt, "java/util/TreeSet", "()V");
        let to = c(
            &mut rt,
            "java/util/stream/Collectors.toCollection.(Ljava/util/function/Supplier;)Ljava/util/stream/Collector;",
            &[tree],
        );
        let s = stream(&mut rt, words.clone());
        let set = c(&mut rt, COLLECT, &[s, to]);
        assert_eq!(rt.class_of(set), "java/util/TreeSet");
        assert_eq!(text(&mut rt, set), "[apple, fig, kiwi, pear]");
        let is_empty = crate::functions::method(&mut rt, "java/lang/String.isEmpty.()Z");
        let parts = c(
            &mut rt,
            "java/util/stream/Collectors.partitioningBy.(Ljava/util/function/Predicate;)Ljava/util/stream/Collector;",
            &[is_empty],
        );
        let s = stream(&mut rt, words.clone());
        let m = c(&mut rt, COLLECT, &[s, parts]);
        assert_eq!(
            text(&mut rt, m),
            "{false=[pear, fig, apple, kiwi], true=[]}"
        );
        let list = c(
            &mut rt,
            "java/util/stream/Collectors.toList.()Ljava/util/stream/Collector;",
            &[],
        );
        let size = crate::functions::method(&mut rt, "java/util/List.size.()I");
        let then = c(
            &mut rt,
            "java/util/stream/Collectors.collectingAndThen.(Ljava/util/stream/Collector;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
            &[list, size],
        );
        let s = stream(&mut rt, words);
        let n = c(&mut rt, COLLECT, &[s, then]);
        assert_eq!(n, V::I(4));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `toMap(key, value)` refuses a key met twice with Java's message; with a merge function it
    /// merges.
    #[test]
    fn to_map_refuses_a_key_twice_without_a_merge() {
        let mut rt = runtime();
        let length = crate::functions::method(&mut rt, LENGTH);
        let identity = c(
            &mut rt,
            "java/util/function/Function.identity.()Ljava/util/function/Function;",
            &[],
        );
        let words = strings(&mut rt, &["ab", "cd"]);
        let to_map = c(
            &mut rt,
            "java/util/stream/Collectors.toMap.(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
            &[length, identity],
        );
        let s = stream(&mut rt, words.clone());
        c(&mut rt, COLLECT, &[s, to_map]);
        let thrown = rt.thrown.take().expect("a key twice throws");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalStateException");
        let msg = c(
            &mut rt,
            "java/lang/Throwable.getMessage.()Ljava/lang/String;",
            &[thrown],
        );
        assert_eq!(
            rt.str_of(msg),
            Some("Duplicate key 2 (attempted merging values ab and cd)")
        );
        let concat = crate::functions::method(
            &mut rt,
            "java/lang/String.concat.(Ljava/lang/String;)Ljava/lang/String;",
        );
        let merging = c(
            &mut rt,
            "java/util/stream/Collectors.toMap.(Ljava/util/function/Function;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;",
            &[length, identity, concat],
        );
        let s = stream(&mut rt, words);
        let m = c(&mut rt, COLLECT, &[s, merging]);
        assert_eq!(text(&mut rt, m), "{2=abcd}");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
