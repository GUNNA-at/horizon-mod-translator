//! Vanilla's weighted choices, natively, as 1.21.1 has them: a
//! `Weight`, an entry that has one (`WeightedEntry`: a mod's own class, one built on
//! `IntrusiveBase`, or a `Wrapper` around any value), `WeightedRandom`'s draws over a list of
//! entries, and the lists that keep their total (`WeightedRandomList`, `SimpleWeightedRandomList`
//! and its builder). A mod picks a mob's variant, a spawn, a block of a feature with them; the
//! draw is one `nextInt(total)` and a walk down the list, in the list's order.
use crate::{Data, Rt, V, jutil};

const WEIGHT: &str = "net/minecraft/util/random/Weight";
const ENTRY: &str = "net/minecraft/util/random/WeightedEntry";
const INTRUSIVE: &str = "net/minecraft/util/random/WeightedEntry$IntrusiveBase";
const WRAPPER: &str = "net/minecraft/util/random/WeightedEntry$Wrapper";
const RANDOM: &str = "net/minecraft/util/random/WeightedRandom";
const LIST: &str = "net/minecraft/util/random/WeightedRandomList";
const SIMPLE: &str = "net/minecraft/util/random/SimpleWeightedRandomList";
const BUILDER: &str = "net/minecraft/util/random/SimpleWeightedRandomList$Builder";

const GET_WEIGHT: &str = "getWeight.()Lnet/minecraft/util/random/Weight;";
const GET_WEIGHT_KEY: &str =
    "net/minecraft/util/random/WeightedEntry.getWeight.()Lnet/minecraft/util/random/Weight;";
const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";

/// The keys [`call`] answers.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        WEIGHT | ENTRY | INTRUSIVE | WRAPPER | RANDOM | LIST | SIMPLE | BUILDER
    )
}

/// `Weight.of(weight)`: one object for 1, as vanilla keeps one; a negative weight is refused.
pub(crate) fn weight(rt: &mut Rt, value: i32) -> V {
    if value < 0 {
        return crate::jlang::throw_with(
            rt,
            "java/lang/IllegalArgumentException",
            "Weight should be >= 0",
        );
    }
    let made = |rt: &mut Rt| {
        let w = rt.new_object(WEIGHT);
        rt.put_field(w, "value", V::I(value));
        w
    };
    if value == 1 {
        rt.intern("compat:weight/1".into(), made)
    } else {
        made(rt)
    }
}

/// `entry.getWeight().asInt()`: a wrapper's and an `IntrusiveBase`'s from the `weight` they keep
/// (vanilla's fields: a spawner's data is an `IntrusiveBase`), any other entry's from its own
/// `getWeight()`.
fn weight_of(rt: &mut Rt, entry: V) -> i32 {
    let class = rt.class_of(entry);
    if rt.overrides.is_some_and(|o| o(class, GET_WEIGHT))
        && let Some(vc) = rt.vcall
    {
        let w = vc(rt, GET_WEIGHT_KEY, GET_WEIGHT, &[entry]);
        return rt.get_field(w, "value").i();
    }
    match (
        rt.get_field(entry, "weight"),
        rt.get_field(entry, "$weight"),
    ) {
        (w, _) if w.r() != 0 => rt.get_field(w, "value").i(),
        // (A wrapper the value providers made keeps the number alone.)
        (_, V::I(n)) => n,
        _ => 0,
    }
}

/// `WeightedEntry.wrap(data, weight)`.
fn wrap(rt: &mut Rt, data: V, value: i32) -> V {
    let w = rt.new_object(WRAPPER);
    let kept = weight(rt, value);
    rt.put_field(w, "data", data);
    // (Its record's component as well as the number: what the record's own `equals`,
    // `hashCode` and `toString` read.)
    rt.put_field(w, "weight", kept);
    rt.put_field(w, "$weight", V::I(value));
    w
}

/// `WeightedRandom.getTotalWeight(entries)`: more than an int holds is refused. None then.
fn total(rt: &mut Rt, entries: &[V]) -> Option<i32> {
    let mut sum = 0i64;
    for e in entries {
        sum += i64::from(weight_of(rt, *e));
    }
    match i32::try_from(sum) {
        Ok(sum) => Some(sum),
        Err(_) => {
            crate::jlang::throw_with(
                rt,
                "java/lang/IllegalArgumentException",
                "Sum of weights must be <= 2147483647",
            );
            None
        }
    }
}

/// `WeightedRandom.getWeightedItem(entries, index)`: the entry `index` falls in, walking the
/// weights down the list.
fn weighted_item(rt: &mut Rt, entries: &[V], mut index: i32) -> V {
    for e in entries {
        index = index.wrapping_sub(weight_of(rt, *e));
        if index < 0 {
            return *e;
        }
    }
    V::NULL
}

/// `WeightedRandom.getRandomItem(random, entries, total)`: none for a total of 0, else the entry
/// `random.nextInt(total)` falls in.
fn random_item(rt: &mut Rt, random: V, entries: &[V], total: i32) -> V {
    if total < 0 {
        return crate::jlang::throw_with(
            rt,
            "java/lang/IllegalArgumentException",
            "Negative total weight in getRandomItem",
        );
    }
    if total == 0 {
        return V::NULL;
    }
    let index =
        crate::api::virtual_call(rt, (NEXT_INT, "nextInt.(I)I"), &[random, V::I(total)]).i();
    weighted_item(rt, entries, index)
}

fn optional(rt: &mut Rt, v: V) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", v);
    o
}

/// A list of entries with its total (`WeightedRandomList`'s constructor), of class `class`.
fn list(rt: &mut Rt, class: &'static str, items: Vec<V>) -> V {
    let Some(sum) = total(rt, &items) else {
        return V::NULL;
    };
    let l = rt.new_object(class);
    let kept = rt.alloc("com/google/common/collect/ImmutableList", Data::List(items));
    rt.put_field(l, "$items", kept);
    rt.put_field(l, "$total", V::I(sum));
    l
}

fn items(rt: &Rt, l: V) -> Vec<V> {
    jutil::elements(rt, rt.get_field(l, "$items"))
}

/// A weighted choice's call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (WEIGHT, "of") => weight(rt, a(0).i()),
        (WEIGHT, "asInt") => V::I(rt.get_field(a(0), "value").i()),
        (WEIGHT, "toString") => {
            let text = rt.get_field(a(0), "value").i().to_string();
            rt.string(&text)
        }
        (WEIGHT, "hashCode") => V::I(rt.get_field(a(0), "value").i()),
        (WEIGHT, "equals") => {
            let same = a(0) == a(1)
                || (rt.class_of(a(1)) == WEIGHT
                    && rt.get_field(a(0), "value") == rt.get_field(a(1), "value"));
            V::I(i32::from(same))
        }
        (ENTRY, "wrap") => {
            // (The weight is checked as `Weight.of` checks it.)
            let checked = weight(rt, a(1).i());
            if rt.thrown.is_some() || checked.r() == 0 {
                return Some(V::NULL);
            }
            wrap(rt, a(0), a(1).i())
        }
        // An entry of a mod's own class built on `IntrusiveBase`: its weight is kept with it.
        (INTRUSIVE, "<init>") => {
            let kept = if desc.starts_with("(I)") {
                weight(rt, a(1).i())
            } else {
                a(1)
            };
            rt.put_field(a(0), "weight", kept);
            V::Void
        }
        (WRAPPER, "<init>") => {
            let value = rt.get_field(a(2), "value").i();
            rt.put_field(a(0), "data", a(1));
            rt.put_field(a(0), "weight", a(2));
            rt.put_field(a(0), "$weight", V::I(value));
            V::Void
        }
        (WRAPPER, "data") => rt.get_field(a(0), "data"),
        (ENTRY | INTRUSIVE | WRAPPER, "getWeight" | "weight") => match rt.get_field(a(0), "weight")
        {
            w if w.r() != 0 => w,
            _ => {
                let value = weight_of(rt, a(0));
                weight(rt, value)
            }
        },
        (RANDOM, "getTotalWeight") => {
            let entries = jutil::elements(rt, a(0));
            V::I(total(rt, &entries)?)
        }
        (RANDOM, "getRandomItem") => {
            let entries = jutil::elements(rt, a(1));
            let sum = if args.len() > 2 {
                a(2).i()
            } else {
                total(rt, &entries)?
            };
            let picked = random_item(rt, a(0), &entries, sum);
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            optional(rt, picked)
        }
        (RANDOM, "getWeightedItem") => {
            let entries = jutil::elements(rt, a(0));
            let picked = weighted_item(rt, &entries, a(1).i());
            optional(rt, picked)
        }
        // `create()`, `create(entries...)`, `create(List)`.
        (LIST, "create") => {
            let given = if args.is_empty() {
                Vec::new()
            } else {
                jutil::elements(rt, a(0))
            };
            list(rt, LIST, given)
        }
        (SIMPLE, "empty") => list(rt, SIMPLE, Vec::new()),
        (SIMPLE, "single") => {
            let only = wrap(rt, a(0), 1);
            list(rt, SIMPLE, vec![only])
        }
        (SIMPLE, "builder") => {
            let b = rt.new_object(BUILDER);
            let held = jutil::new_list(rt, Vec::new());
            rt.put_field(b, "$items", held);
            b
        }
        (BUILDER, "<init>") => {
            let held = jutil::new_list(rt, Vec::new());
            rt.put_field(a(0), "$items", held);
            V::Void
        }
        // `add(data)` (a weight of 1) and `add(data, weight)`.
        (BUILDER, "add") => {
            let value = if args.len() > 2 { a(2).i() } else { 1 };
            let checked = weight(rt, value);
            if rt.thrown.is_some() || checked.r() == 0 {
                return Some(V::NULL);
            }
            let entry = wrap(rt, a(1), value);
            let held = rt.get_field(a(0), "$items");
            jutil::list_add(rt, held, entry);
            a(0)
        }
        (BUILDER, "build") => {
            let given = items(rt, a(0));
            list(rt, SIMPLE, given)
        }
        (LIST | SIMPLE, "isEmpty") => V::I(i32::from(items(rt, a(0)).is_empty())),
        (LIST | SIMPLE, "unwrap") => rt.get_field(a(0), "$items"),
        (LIST | SIMPLE, "getRandom" | "getRandomValue") => {
            let (entries, sum) = (items(rt, a(0)), rt.get_field(a(0), "$total").i());
            let mut picked = random_item(rt, a(1), &entries, sum);
            // `getRandomValue`: what the wrapper picked holds.
            if name == "getRandomValue" && picked.r() != 0 {
                picked = rt.get_field(picked, "data");
            }
            optional(rt, picked)
        }
        (LIST | SIMPLE, "equals") => {
            let same = a(0) == a(1)
                || (rt.get_field(a(1), "$items").r() != 0
                    && rt.get_field(a(0), "$total") == rt.get_field(a(1), "$total")
                    && items(rt, a(0)) == items(rt, a(1)));
            V::I(i32::from(same))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    /// A random source that answers `nextInt(bound)` with what its `$next` holds.
    fn random(rt: &mut Rt, next: i32) -> V {
        let r = rt.new_object("test/Fixed");
        rt.put_field(r, "$next", V::I(next));
        r
    }

    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match short {
            "nextInt.(I)I" => rt.get_field(args[0], "$next"),
            // A mod's own entry, whose weight is its `heavy` field.
            GET_WEIGHT => {
                let w = rt.get_field(args[0], "heavy").i();
                weight(rt, w)
            }
            _ => c(rt, key, args),
        }
    }

    fn held(rt: &Rt, optional: V) -> V {
        rt.get_field(optional, "value")
    }

    /// The draw is `nextInt(total)` walked down the list: with weights 3, 0 and 5 the indexes 0
    /// to 2 are the first entry's and 3 to 7 the third's; no weight at all picks nothing (Living
    /// Things draws an owl's variant so and takes what comes without looking). Entries are
    /// wrappers, objects built on `IntrusiveBase`, or a mod's own with a `getWeight()`.
    #[test]
    fn a_weighted_draw_walks_the_weights_in_the_lists_order() {
        const WRAP: &str = "net/minecraft/util/random/WeightedEntry.wrap.(Ljava/lang/Object;I)Lnet/minecraft/util/random/WeightedEntry$Wrapper;";
        const PICK: &str = "net/minecraft/util/random/WeightedRandom.getRandomItem.(Lnet/minecraft/util/RandomSource;Ljava/util/List;)Ljava/util/Optional;";
        const PICK_OF: &str = "net/minecraft/util/random/WeightedRandom.getRandomItem.(Lnet/minecraft/util/RandomSource;Ljava/util/List;I)Ljava/util/Optional;";
        crate::install();
        let mut rt = Rt::new();
        rt.overrides = Some(|class, method| {
            (class == "test/Fixed" && method == "nextInt.(I)I")
                || (class == "test/Own" && method == GET_WEIGHT)
        });
        rt.vcall = Some(vcall);
        rt.supers.insert("test/Based", INTRUSIVE);
        let (red, none, blue) = (rt.string("red"), rt.string("none"), rt.string("blue"));
        let first = c(&mut rt, WRAP, &[red, V::I(3)]);
        let second = c(&mut rt, WRAP, &[none, V::I(0)]);
        let third = c(&mut rt, WRAP, &[blue, V::I(5)]);
        let entries = jutil::new_list(&mut rt, vec![first, second, third]);
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/random/WeightedRandom.getTotalWeight.(Ljava/util/List;)I",
                &[entries]
            ),
            V::I(8)
        );
        for (drawn, picked) in [(0, first), (2, first), (3, third), (7, third)] {
            let r = random(&mut rt, drawn);
            let got = c(&mut rt, PICK, &[r, entries]);
            assert_eq!(held(&rt, got), picked, "{drawn}");
        }
        let r = random(&mut rt, 0);
        let nothing = jutil::new_list(&mut rt, vec![second]);
        let got = c(&mut rt, PICK, &[r, nothing]);
        assert_eq!(held(&rt, got), V::NULL);
        // A total given is the total used; a negative one is an error.
        let got = c(&mut rt, PICK_OF, &[r, entries, V::I(3)]);
        assert_eq!(held(&rt, got), first);
        c(&mut rt, PICK_OF, &[r, entries, V::I(-1)]);
        let thrown = rt.thrown.take().expect("a negative total");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");
        // An entry built on `IntrusiveBase`, and one of a mod's own class.
        let based = rt.new_object("test/Based");
        c(
            &mut rt,
            "net/minecraft/util/random/WeightedEntry$IntrusiveBase.<init>.(I)V",
            &[based, V::I(2)],
        );
        let own = rt.new_object("test/Own");
        rt.put_field(own, "heavy", V::I(4));
        let mixed = jutil::new_list(&mut rt, vec![based, own]);
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/random/WeightedRandom.getTotalWeight.(Ljava/util/List;)I",
                &[mixed]
            ),
            V::I(6)
        );
        let r = random(&mut rt, 2);
        let got = c(&mut rt, PICK, &[r, mixed]);
        assert_eq!(held(&rt, got), own);
        let w = c(
            &mut rt,
            "net/minecraft/util/random/WeightedEntry$IntrusiveBase.getWeight.()Lnet/minecraft/util/random/Weight;",
            &[based],
        );
        assert_eq!(
            c(&mut rt, "net/minecraft/util/random/Weight.asInt.()I", &[w]),
            V::I(2)
        );
        assert!(
            rt.unmapped.is_empty() && rt.thrown.is_none(),
            "{:?}",
            rt.unmapped
        );
    }

    /// A list keeps its total and gives what a draw falls on; a simple list's builder wraps
    /// each value, with a weight of 1 unless told, and gives the value itself.
    #[test]
    fn a_weighted_list_draws_from_what_it_was_built_of() {
        const ADD: &str = "net/minecraft/util/random/SimpleWeightedRandomList$Builder.add.(Ljava/lang/Object;I)Lnet/minecraft/util/random/SimpleWeightedRandomList$Builder;";
        const ADD_ONE: &str = "net/minecraft/util/random/SimpleWeightedRandomList$Builder.add.(Ljava/lang/Object;)Lnet/minecraft/util/random/SimpleWeightedRandomList$Builder;";
        crate::install();
        let mut rt = Rt::new();
        rt.overrides = Some(|class, method| class == "test/Fixed" && method == "nextInt.(I)I");
        rt.vcall = Some(vcall);
        let (oak, birch) = (rt.string("oak"), rt.string("birch"));
        let builder = c(
            &mut rt,
            "net/minecraft/util/random/SimpleWeightedRandomList.builder.()Lnet/minecraft/util/random/SimpleWeightedRandomList$Builder;",
            &[],
        );
        c(&mut rt, ADD, &[builder, oak, V::I(9)]);
        c(&mut rt, ADD_ONE, &[builder, birch]);
        let built = c(
            &mut rt,
            "net/minecraft/util/random/SimpleWeightedRandomList$Builder.build.()Lnet/minecraft/util/random/SimpleWeightedRandomList;",
            &[builder],
        );
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/random/WeightedRandomList.isEmpty.()Z",
                &[built]
            ),
            V::I(0)
        );
        let wrappers = c(
            &mut rt,
            "net/minecraft/util/random/WeightedRandomList.unwrap.()Ljava/util/List;",
            &[built],
        );
        let wrappers = jutil::elements(&rt, wrappers);
        assert_eq!(wrappers.len(), 2);
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/random/WeightedEntry$Wrapper.data.()Ljava/lang/Object;",
                &[wrappers[1]]
            ),
            birch
        );
        for (drawn, value) in [(0, oak), (8, oak), (9, birch)] {
            let r = random(&mut rt, drawn);
            let got = c(
                &mut rt,
                "net/minecraft/util/random/SimpleWeightedRandomList.getRandomValue.(Lnet/minecraft/util/RandomSource;)Ljava/util/Optional;",
                &[built, r],
            );
            assert_eq!(held(&rt, got), value, "{drawn}");
            let got = c(
                &mut rt,
                "net/minecraft/util/random/WeightedRandomList.getRandom.(Lnet/minecraft/util/RandomSource;)Ljava/util/Optional;",
                &[built, r],
            );
            assert_eq!(rt.get_field(held(&rt, got), "data"), value, "{drawn}");
        }
        // Of a list of entries; an empty one gives nothing and draws nothing.
        let listed = jutil::new_list(&mut rt, wrappers.clone());
        let of_list = c(
            &mut rt,
            "net/minecraft/util/random/WeightedRandomList.create.(Ljava/util/List;)Lnet/minecraft/util/random/WeightedRandomList;",
            &[listed],
        );
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/random/WeightedRandomList.equals.(Ljava/lang/Object;)Z",
                &[of_list, built]
            ),
            V::I(1)
        );
        let empty = c(
            &mut rt,
            "net/minecraft/util/random/WeightedRandomList.create.()Lnet/minecraft/util/random/WeightedRandomList;",
            &[],
        );
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/random/WeightedRandomList.isEmpty.()Z",
                &[empty]
            ),
            V::I(1)
        );
        let r = random(&mut rt, 0);
        let got = c(
            &mut rt,
            "net/minecraft/util/random/WeightedRandomList.getRandom.(Lnet/minecraft/util/RandomSource;)Ljava/util/Optional;",
            &[empty, r],
        );
        assert_eq!(held(&rt, got), V::NULL);
        assert!(
            rt.unmapped.is_empty() && rt.thrown.is_none(),
            "{:?}",
            rt.unmapped
        );
    }
}
