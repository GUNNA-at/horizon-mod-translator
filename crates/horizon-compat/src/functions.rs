//! `java.util.function`'s default and static combinators (`Predicate.or`, `negate`, `not`,
//! `Function.andThen`, `compose`, `identity`, `Consumer.andThen`) and Guava's (`Predicates`,
//! `Suppliers.memoize`). Each is an object holding the functions it combines (class
//! `compat:fn/<kind>`); calling its functional method runs them through `call_functional`, so
//! lambdas, the mod's own classes and other combinators mix as in Java.
use crate::{Data, Rt, V};

const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";

fn combinator(rt: &mut Rt, kind: &'static str, parts: Vec<V>) -> V {
    rt.alloc(kind, Data::Array(parts))
}

/// A function of any arguments answering `v` (`state -> 0`, Guava's `Suppliers.ofInstance`).
pub(crate) fn constant(rt: &mut Rt, v: V) -> V {
    combinator(rt, "compat:fn/constant", vec![v])
}

/// What DFU's `getList`/`getMapEntries` answer: a consumer of a consumer, which hands it each of
/// `items` (a list): each element, or each pair's two halves to a `BiConsumer`.
pub(crate) fn feed(rt: &mut Rt, items: V, pairs: bool) -> V {
    combinator(rt, "compat:fn/feed", vec![items, V::I(pairs as i32)])
}

/// DFU's `Codec.checkRange(min, max)`: a function answering a `DataResult` of its (boxed) number,
/// an error with the number as its partial value outside `[min, max]`.
pub(crate) fn check_range(rt: &mut Rt, min: V, max: V) -> V {
    combinator(rt, "compat:fn/check_range", vec![min, max])
}

/// A boxed number's value (`Long.valueOf(5)`), or the raw one a primitive collection holds.
fn number(rt: &Rt, v: V) -> V {
    match v {
        V::R(_) => rt.get_field(v, "value"),
        raw => raw,
    }
}

/// Java's `toString` of a number.
fn number_text(v: V) -> String {
    match v {
        V::I(i) => i.to_string(),
        V::J(j) => j.to_string(),
        V::F(f) => crate::java_double(f64::from(f), true),
        V::D(d) => crate::java_double(d, false),
        _ => "null".into(),
    }
}

/// Vanilla record `class`'s constructor as a function of its components.
pub(crate) fn record_constructor(rt: &mut Rt, class: &str) -> V {
    let c = rt.string(class);
    combinator(rt, "compat:fn/record", vec![c])
}

/// A constructor of the game's `class` (`Item::new`) as a function of its arguments: `desc` is
/// the constructor's descriptor.
pub(crate) fn constructor(rt: &mut Rt, class: &str, desc: &str) -> V {
    let (c, d) = (rt.string(class), rt.string(desc));
    combinator(rt, "compat:fn/new", vec![c, d])
}

/// `new BlockItem(block.get(), properties)` as a supplier: `block` a supplier of the block (a
/// holder), asked only when the item is made, as NeoForge's `registerSimpleBlockItem` asks it.
pub(crate) fn block_item(rt: &mut Rt, block: V, properties: V) -> V {
    combinator(rt, "compat:fn/block_item", vec![block, properties])
}

/// One object's method as a function (`atlas::getSprite`): `key` called on `receiver` with the
/// functional method's arguments.
pub(crate) fn bound(rt: &mut Rt, key: &str, receiver: V) -> V {
    let k = rt.string(key);
    combinator(rt, "compat:fn/bound", vec![k, receiver])
}

/// A record component's accessor (`name`) as a function of the record.
pub(crate) fn component(rt: &mut Rt, name: &str) -> V {
    let n = rt.string(name);
    combinator(rt, "compat:fn/component", vec![n])
}

/// `StringRepresentable.createNameLookup(values, keyFunction)`: a function from a name to the
/// constant of `values` whose serialized name, through `key_fn`, is that name (null for none).
pub(crate) fn name_lookup(rt: &mut Rt, values: V, key_fn: V) -> V {
    combinator(rt, "compat:fn/name_lookup", vec![values, key_fn])
}

/// A method of the game's or Java's as a function of its receiver and arguments
/// (`Map.Entry::getKey`): `key` is the method's.
pub(crate) fn method(rt: &mut Rt, key: &str) -> V {
    let k = rt.string(key);
    combinator(rt, "compat:fn/method", vec![k])
}

/// A lambda `LambdaMetafactory` made when called by hand: `parts` the implementation's handle,
/// then what the lambda captured; its functional method invokes the handle with those and its
/// arguments (see [`crate::invoke`]).
pub(crate) fn handle_function(rt: &mut Rt, parts: Vec<V>) -> V {
    combinator(rt, "compat:fn/handle", parts)
}

/// A new object of the game's `class`, made by its constructor `desc` with `args`.
fn make(rt: &mut Rt, class: &str, desc: &str, args: &[V]) -> V {
    let class: &'static str = rt.static_key(class);
    let o = rt.new_object(class);
    let mut with_this = vec![o];
    with_this.extend_from_slice(args);
    let init = rt.static_key(&format!("{class}.<init>.{desc}"));
    crate::api::call(rt, init, &with_this);
    o
}

/// A call nothing else answered on one of these functions through an interface of the game's
/// that holds it (a vanilla constant made one: `CauldronInteraction.DYED_ITEM.interact(...)`):
/// its one method, unless the name is `Object`'s or a functional interface's default method's,
/// which a function doesn't answer this way. `args` start with the receiver.
pub(crate) fn invoke_through(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    let kind = rt.class_of(this);
    let (_, name, _) = crate::parts(key)?;
    let defaults = matches!(name, "equals" | "hashCode" | "toString" | "getClass" | "<init>" | "andThen" | "compose" | "negate" | "and" | "or");
    (kind.starts_with("compat:fn/") && !defaults).then(|| run(rt, kind, this, key, args.get(1..).unwrap_or_default()))
}

/// A combinator's functional method (`test`, `apply`, `accept`, `get`), invoked by `key` with
/// `args` after the receiver.
fn run(rt: &mut Rt, kind: &str, this: V, key: &'static str, args: &[V]) -> V {
    let parts = rt.captured(this);
    let part = |i: usize| parts.get(i).copied().unwrap_or_default();
    let truth = |b: bool| V::I(b as i32);
    match kind {
        "compat:fn/and" => truth(parts.iter().all(|&p| rt.call_functional(key, p, args).i() != 0)),
        "compat:fn/or" => truth(parts.iter().any(|&p| rt.call_functional(key, p, args).i() != 0)),
        "compat:fn/not" => truth(rt.call_functional(key, part(0), args).i() == 0),
        "compat:fn/equal" => truth(rt.java_equals(part(0), args.first().copied().unwrap_or_default())),
        "compat:fn/null" => truth(args.first().is_none_or(|v| v.r() == 0)),
        "compat:fn/true" => truth(true),
        "compat:fn/false" => truth(false),
        // `first.andThen(second)`: the first takes the arguments, the second its result.
        "compat:fn/then" => {
            let r = rt.call_functional(key, part(0), args);
            rt.call_functional(APPLY, part(1), &[r])
        }
        "compat:fn/both" => {
            rt.call_functional(key, part(0), args);
            rt.call_functional(key, part(1), args);
            V::Void
        }
        "compat:fn/identity" => args.first().copied().unwrap_or_default(),
        // A vanilla record's constructor as a function (`GlobalPos::of`): [class].
        "compat:fn/record" => {
            let class = rt.str_of(part(0)).unwrap_or("").to_owned();
            crate::records::make(rt, &class, args)
        }
        // A component's accessor as a function (`GlobalPos::pos`): [name].
        "compat:fn/component" => {
            let name = rt.str_of(part(0)).unwrap_or("").to_owned();
            let name: &'static str = rt.static_key(&name);
            rt.get_field(args.first().copied().unwrap_or_default(), name)
        }
        "compat:fn/constant" => part(0),
        // [values, keyFunction]: `apply(name)`.
        "compat:fn/name_lookup" => {
            let Some(wanted) = args.first().and_then(|&n| rt.str_of(n)).map(str::to_owned) else { return V::NULL };
            for v in crate::jutil::elements(rt, part(0)) {
                let n = rt.call_functional("net/minecraft/util/StringRepresentable.getSerializedName.()Ljava/lang/String;", v, &[]);
                let k = rt.call_functional(APPLY, part(1), &[n]);
                if rt.str_of(k) == Some(wanted.as_str()) {
                    return v;
                }
            }
            V::NULL
        }
        // [handle, captured...]: the handle invoked with what was captured, then the arguments,
        // its result as the functional method returns it.
        "compat:fn/handle" => {
            let mut all = parts.get(1..).unwrap_or_default().to_vec();
            all.extend_from_slice(args);
            let ret = crate::parts(key).map_or('L', |(_, _, d)| crate::jlang::return_kind(d));
            match crate::invoke::invoke_handle(rt, part(0), &all, ret) {
                Some(v) => v,
                None => {
                    crate::miss(rt, key);
                    V::NULL
                }
            }
        }
        // One object's method as a function (`atlas::getSprite`): [key, receiver], on the receiver
        // with the arguments.
        "compat:fn/bound" => {
            let k = rt.str_of(part(0)).unwrap_or("").to_owned();
            let k: &'static str = rt.static_key(&k);
            let mut all = vec![part(1)];
            all.extend_from_slice(args);
            crate::api::call(rt, k, &all)
        }
        // A method as a function (`Map.Entry::getKey`): [key], on the first argument with the rest.
        "compat:fn/method" => {
            let k = rt.str_of(part(0)).unwrap_or("").to_owned();
            let k: &'static str = rt.static_key(&k);
            crate::api::call(rt, k, args)
        }
        // [items, pairs]: `accept(consumer)`.
        "compat:fn/feed" => {
            let consumer = args.first().copied().unwrap_or_default();
            for item in crate::jutil::elements(rt, part(0)) {
                if part(1).i() != 0 {
                    let (k, v) = (rt.get_field(item, "$first"), rt.get_field(item, "$second"));
                    rt.call_functional("java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V", consumer, &[k, v]);
                } else {
                    rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", consumer, &[item]);
                }
                if rt.thrown.is_some() {
                    break;
                }
            }
            V::Void
        }
        // [min, max]; `compareTo` as the boxes compare: whole numbers exactly, others as doubles.
        "compat:fn/check_range" => {
            let value = args.first().copied().unwrap_or_default();
            let (v, lo, hi) = (number(rt, value), number(rt, part(0)), number(rt, part(1)));
            let whole = |x: V| match x {
                V::I(i) => Some(i64::from(i)),
                V::J(j) => Some(j),
                _ => None,
            };
            let real = |x: V| match x {
                V::I(i) => f64::from(i),
                V::J(j) => j as f64,
                V::F(f) => f64::from(f),
                V::D(d) => d,
                _ => f64::NAN,
            };
            let inside = match (whole(v), whole(lo), whole(hi)) {
                (Some(v), Some(lo), Some(hi)) => lo <= v && v <= hi,
                _ => real(lo) <= real(v) && real(v) <= real(hi),
            };
            if inside {
                crate::codecs::success(rt, value)
            } else {
                let message = format!("Value {} outside of range [{}:{}]", number_text(v), number_text(lo), number_text(hi));
                let r = crate::codecs::error(rt, &message);
                rt.put_field(r, "$partial", value);
                r
            }
        }
        // A constructor as a function (`Item::new`): [class, descriptor].
        "compat:fn/new" => {
            let (class, desc) = (rt.str_of(part(0)).unwrap_or("").to_owned(), rt.str_of(part(1)).unwrap_or("").to_owned());
            make(rt, &class, &desc, args)
        }
        // [block supplier, properties].
        "compat:fn/block_item" => {
            let block = rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", part(0), &[]);
            make(rt, "net/minecraft/world/item/BlockItem", "(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/world/item/Item$Properties;)V", &[block, part(1)])
        }
        // `Suppliers.memoize`: the supplier runs once; [supplier, value, done].
        "compat:fn/memo" => {
            if part(2).i() != 0 {
                return part(1);
            }
            let v = rt.call_functional(key, part(0), &[]);
            if let Some(o) = rt.heap.get_mut(this.r() as usize) {
                o.data = Data::Array(vec![part(0), v, V::I(1)]);
            }
            v
        }
        _ => V::Void,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(name, "test" | "apply" | "accept" | "get")
        || owner.starts_with("java/util/function/")
        || matches!(owner, "com/google/common/base/Predicates" | "com/google/common/base/Suppliers")
}

/// A combinator call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    let receiver = rt.class_of(a(0));
    if receiver.starts_with("compat:fn/") && matches!(name, "test" | "apply" | "accept" | "get" | "applyAsInt" | "applyAsLong" | "applyAsDouble" | "getAsInt" | "getAsLong" | "getAsDouble" | "getAsBoolean") {
        return Some(run(rt, receiver, a(0), key, args.get(1..).unwrap_or_default()));
    }
    const PREDICATES: &str = "com/google/common/base/Predicates";
    let both = |rt: &mut Rt, kind| combinator(rt, kind, vec![a(0), a(1)]);
    Some(match (owner, name) {
        ("java/util/function/Predicate" | "java/util/function/BiPredicate" | "java/util/function/IntPredicate", "and") => both(rt, "compat:fn/and"),
        ("java/util/function/Predicate" | "java/util/function/BiPredicate" | "java/util/function/IntPredicate", "or") => both(rt, "compat:fn/or"),
        ("java/util/function/Predicate" | "java/util/function/BiPredicate" | "java/util/function/IntPredicate", "negate") => combinator(rt, "compat:fn/not", vec![a(0)]),
        // Statics: `Predicate.not(p)`, `Predicate.isEqual(x)`.
        ("java/util/function/Predicate", "not") => combinator(rt, "compat:fn/not", vec![a(0)]),
        ("java/util/function/Predicate", "isEqual") => combinator(rt, "compat:fn/equal", vec![a(0)]),
        ("java/util/function/Function" | "java/util/function/BiFunction", "andThen") => both(rt, "compat:fn/then"),
        ("java/util/function/Function", "compose") => combinator(rt, "compat:fn/then", vec![a(1), a(0)]),
        ("java/util/function/Function" | "java/util/function/UnaryOperator", "identity") => combinator(rt, "compat:fn/identity", Vec::new()),
        ("java/util/function/Consumer" | "java/util/function/BiConsumer", "andThen") => both(rt, "compat:fn/both"),
        (PREDICATES, "not") => combinator(rt, "compat:fn/not", vec![a(0)]),
        (PREDICATES, "alwaysTrue") => combinator(rt, "compat:fn/true", Vec::new()),
        (PREDICATES, "alwaysFalse") => combinator(rt, "compat:fn/false", Vec::new()),
        (PREDICATES, "isNull") => combinator(rt, "compat:fn/null", Vec::new()),
        (PREDICATES, "notNull") => {
            let null = combinator(rt, "compat:fn/null", Vec::new());
            combinator(rt, "compat:fn/not", vec![null])
        }
        (PREDICATES, "equalTo") => combinator(rt, "compat:fn/equal", vec![a(0)]),
        (PREDICATES, n @ ("and" | "or")) => {
            // Two predicates, or an array or iterable of them.
            let parts = if desc.starts_with("(Lcom/google/common/base/Predicate;Lcom/google/common/base/Predicate;)") {
                vec![a(0), a(1)]
            } else {
                crate::jutil::elements(rt, a(0))
            };
            combinator(rt, if n == "and" { "compat:fn/and" } else { "compat:fn/or" }, parts)
        }
        ("com/google/common/base/Suppliers", "memoize") => combinator(rt, "compat:fn/memo", vec![a(0), V::NULL, V::I(0)]),
        ("com/google/common/base/Suppliers", "ofInstance") => constant(rt, a(0)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";

    /// What generated code does for an object no mod class is: the API call by the method's key.
    fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// Lambda 1: `x -> x > 2`; lambda 2: `x -> x * 10`; lambda 3: counts its runs, gives 7.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        Some(match id {
            1 => V::I((args[1].i() > 2) as i32),
            2 => V::I(args[1].i() * 10),
            3 => {
                let n = rt.statics.get("t.runs").copied().unwrap_or(V::I(0)).i();
                rt.statics.insert("t.runs", V::I(n + 1));
                V::I(7)
            }
            _ => return None,
        })
    }

    fn rt() -> Rt {
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(lambdas);
        rt
    }

    #[test]
    fn predicates_combine_as_in_java() {
        let mut rt = rt();
        let big = rt.lambda_id("t.big", 1, Vec::new());
        let small = c(&mut rt, "java/util/function/Predicate.negate.()Ljava/util/function/Predicate;", &[big]);
        let three = c(&mut rt, "java/util/function/Predicate.isEqual.(Ljava/lang/Object;)Ljava/util/function/Predicate;", &[V::I(3)]);
        let small_or_three = c(&mut rt, "java/util/function/Predicate.or.(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;", &[small, three]);
        let neither = c(&mut rt, "java/util/function/Predicate.not.(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;", &[small_or_three]);
        let answers: Vec<(i32, i32, i32)> = [1, 3, 5]
            .iter()
            .map(|&x| {
                let t = |rt: &mut Rt, p| rt.call_functional(TEST, p, &[V::I(x)]).i();
                (t(&mut rt, small), t(&mut rt, small_or_three), t(&mut rt, neither))
            })
            .collect();
        assert_eq!(answers, vec![(1, 1, 0), (0, 1, 0), (0, 0, 1)]);
        let guava = c(&mut rt, "com/google/common/base/Predicates.and.(Lcom/google/common/base/Predicate;Lcom/google/common/base/Predicate;)Lcom/google/common/base/Predicate;", &[big, neither]);
        assert_eq!(rt.call_functional("com/google/common/base/Predicate.apply.(Ljava/lang/Object;)Z", guava, &[V::I(5)]).i(), 1);
    }

    #[test]
    fn functions_chain_and_suppliers_memoize() {
        let mut rt = rt();
        let times_ten = rt.lambda_id("t.ten", 2, Vec::new());
        let twice = c(&mut rt, "java/util/function/Function.andThen.(Ljava/util/function/Function;)Ljava/util/function/Function;", &[times_ten, times_ten]);
        assert_eq!(rt.call_functional(APPLY, twice, &[V::I(2)]), V::I(200));
        let same = c(&mut rt, "java/util/function/Function.identity.()Ljava/util/function/Function;", &[]);
        let composed = c(&mut rt, "java/util/function/Function.compose.(Ljava/util/function/Function;)Ljava/util/function/Function;", &[times_ten, same]);
        assert_eq!(rt.call_functional(APPLY, composed, &[V::I(4)]), V::I(40));
        let seven = rt.lambda_id("t.seven", 3, Vec::new());
        let memo = c(&mut rt, "com/google/common/base/Suppliers.memoize.(Lcom/google/common/base/Supplier;)Lcom/google/common/base/Supplier;", &[seven]);
        let get = "com/google/common/base/Supplier.get.()Ljava/lang/Object;";
        assert_eq!((rt.call_functional(get, memo, &[]), rt.call_functional(get, memo, &[])), (V::I(7), V::I(7)));
        assert_eq!(rt.statics.get("t.runs"), Some(&V::I(1)));
    }
}
