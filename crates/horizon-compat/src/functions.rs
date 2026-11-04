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
        "compat:fn/constant" => part(0),
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
    if receiver.starts_with("compat:fn/") && matches!(name, "test" | "apply" | "accept" | "get") {
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
        ("com/google/common/base/Suppliers", "ofInstance") => combinator(rt, "compat:fn/constant", vec![a(0)]),
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
