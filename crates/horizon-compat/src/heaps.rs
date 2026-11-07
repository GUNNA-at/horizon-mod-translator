//! Priority queues: `java.util.PriorityQueue` (and `PriorityBlockingQueue`) and fastutil's heap
//! priority queues (`ObjectHeapPriorityQueue`, `IntHeapPriorityQueue`...). Each holds its elements
//! as the binary heap Java's and fastutil's keep (a `Data::List` in the heap array's order, which
//! is also the order a `PriorityQueue` iterates in), ordered by its comparator (null: the
//! elements' natural order), and moves them as their `siftUp`/`siftDown` (fastutil's
//! `upHeap`/`downHeap`) do, comparing in the same order, so equal elements come out as in Java.
//! What the queue shares with a list (`size`, `isEmpty`, `contains`, `clear`, `iterator`,
//! `toArray`, `stream`) is [`crate::jutil`]'s.
use crate::jutil::{compare, elements, equals};
use crate::{Data, Rt, V};

/// The queue's comparator, kept beside its elements.
const COMPARATOR: &str = "$comparator";
/// Set on a mod's class under a priority queue, which the class name doesn't tell.
const HEAP: &str = "$heap";

/// Whether `class` is a priority queue kept as a heap here.
pub(crate) fn heap_class(class: &str) -> bool {
    matches!(
        class,
        "java/util/PriorityQueue" | "java/util/concurrent/PriorityBlockingQueue"
    ) || (class.starts_with("it/unimi/dsi/fastutil/") && class.ends_with("HeapPriorityQueue"))
}

/// Whether `v` is a priority queue: of such a class, or of a mod's class made as one.
pub(crate) fn is_heap(rt: &Rt, v: V) -> bool {
    let class = rt.class_of(v);
    if heap_class(class) {
        return true;
    }
    // The runtime's own collections are told by their class; only a mod's needs the mark.
    !class.starts_with("java/")
        && !class.starts_with("it/unimi/")
        && !class.starts_with("com/google/")
        && matches!(rt.get_field(v, HEAP), V::I(1))
}

/// A constructor's parameter types (`I`, `Ljava/util/Comparator;`, `[Ljava/lang/Object;`).
fn params(desc: &str) -> Vec<&str> {
    let inner = desc
        .strip_prefix('(')
        .and_then(|d| d.split_once(')'))
        .map_or("", |(p, _)| p);
    let b = inner.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i < b.len() && b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        out.push(&inner[start..i.min(b.len())]);
    }
    out
}

/// A priority queue's constructor (`args[0]` the queue): empty, with the comparator given, or
/// holding the elements given. A sorted set's or another priority queue's come with their
/// comparator, in their order (already a heap), as Java's `PriorityQueue(c)` takes them; any
/// other collection's, and fastutil's array (its first `size` elements), are heapified with the
/// comparator given (none: natural order).
pub(crate) fn construct(rt: &mut Rt, owner: &str, desc: &str, args: &[V]) {
    let this = args.first().copied().unwrap_or_default();
    let (mut comparator, mut items, mut ordered, mut size) = (V::NULL, Vec::new(), false, None);
    for (i, ty) in params(desc).into_iter().enumerate() {
        let v = args.get(i + 1).copied().unwrap_or(V::NULL);
        if ty.ends_with("Comparator;") {
            comparator = v;
        } else if ty == "I" {
            // A capacity, or (after an array) how many of its elements are the heap's.
            if !items.is_empty() {
                size = Some(v.i().max(0) as usize);
            }
        } else if ty.starts_with('[') {
            items = elements(rt, v);
        } else if v.r() != 0 {
            items = elements(rt, v);
            if is_heap(rt, v) {
                comparator = rt.get_field(v, COMPARATOR);
                ordered = true;
            } else if let Some(Data::Table(t)) = rt.heap.get(v.r() as usize).map(|o| &o.data)
                && t.sorted
            {
                comparator = t.comparator;
                ordered = true;
            }
        }
    }
    if let Some(n) = size {
        items.truncate(n);
    }
    let comparator = if matches!(comparator, V::Void) {
        V::NULL
    } else {
        comparator
    };
    let java = !owner.starts_with("it/unimi/");
    if !ordered {
        heapify(rt, &mut items, comparator, java);
    }
    let class = rt.class_of(this);
    if let Some(o) = rt.heap.get_mut(this.r() as usize) {
        // A mod's subclass keeps the fields it already set beside the queue it now is.
        if let Data::Fields(f) = std::mem::replace(&mut o.data, Data::List(items))
            && !f.is_empty()
        {
            rt.side_fields.entry(this.r()).or_default().extend(f);
        }
    }
    rt.put_field(this, COMPARATOR, comparator);
    if !heap_class(class) {
        rt.put_field(this, HEAP, V::I(1));
    }
}

/// The queue's elements, taken out while they are moved (the comparator may run the mod's code;
/// nothing else reads the queue meanwhile), and put back with [`put`].
fn take(rt: &mut Rt, q: V) -> Vec<V> {
    match rt.heap.get_mut(q.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(l)) => std::mem::take(l),
        _ => Vec::new(),
    }
}

fn put(rt: &mut Rt, q: V, items: Vec<V>) {
    if let Some(Data::List(l)) = rt.heap.get_mut(q.r() as usize).map(|o| &mut o.data) {
        *l = items;
    }
}

/// `x` moved up from slot `k` to where it belongs: Java's `siftUp` (`compare(x, parent) >= 0`
/// stops it), fastutil's `upHeap` (`compare(parent, x) <= 0`).
fn sift_up(rt: &mut Rt, es: &mut [V], cmp: V, java: bool, mut k: usize, x: V) {
    while k > 0 {
        let parent = (k - 1) >> 1;
        let e = es[parent];
        let stop = if java {
            compare(rt, cmp, x, e) >= 0
        } else {
            compare(rt, cmp, e, x) <= 0
        };
        if stop || rt.thrown.is_some() {
            break;
        }
        es[k] = e;
        k = parent;
    }
    es[k] = x;
}

/// `x` moved down from slot `k` among the first `n`: Java's `siftDown` (the right child when
/// `compare(left, right) > 0`), fastutil's `downHeap` (when `compare(right, left) < 0`); both stop
/// at a child `x` doesn't follow (`compare(x, child) <= 0`).
fn sift_down(rt: &mut Rt, es: &mut [V], cmp: V, java: bool, mut k: usize, x: V, n: usize) {
    loop {
        let mut child = (k << 1) + 1;
        if child >= n {
            break;
        }
        let mut c = es[child];
        let right = child + 1;
        if right < n {
            let take_right = if java {
                compare(rt, cmp, c, es[right]) > 0
            } else {
                compare(rt, cmp, es[right], c) < 0
            };
            if take_right {
                child = right;
                c = es[child];
            }
        }
        if compare(rt, cmp, x, c) <= 0 || rt.thrown.is_some() {
            break;
        }
        es[k] = c;
        k = child;
    }
    es[k] = x;
}

/// Java's `heapify` and fastutil's `makeHeap`: each parent from the last down to the root sifted
/// down.
fn heapify(rt: &mut Rt, es: &mut [V], cmp: V, java: bool) {
    let n = es.len();
    for i in (0..n / 2).rev() {
        let x = es[i];
        sift_down(rt, es, cmp, java, i, x, n);
    }
}

/// Removes slot `i` as Java's `removeAt` does: the last element moved into it, down, else up.
fn remove_at(rt: &mut Rt, es: &mut Vec<V>, cmp: V, java: bool, i: usize) {
    let Some(moved) = es.pop() else { return };
    if i < es.len() {
        let n = es.len();
        sift_down(rt, es, cmp, java, i, moved, n);
        if es[i] == moved {
            sift_up(rt, es, cmp, java, i, moved);
        }
    }
}

/// A call on a priority queue (`args[0]`): what moves its elements. None for what it shares with
/// a list, which [`crate::jutil`] answers.
pub(crate) fn call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let q = a(0);
    let cmp = rt.get_field(q, COMPARATOR);
    let cmp = if matches!(cmp, V::Void) { V::NULL } else { cmp };
    let java = !rt.class_of(q).starts_with("it/unimi/");
    // fastutil's typed queues name the element's type (`dequeueInt`, `firstLong`).
    let base = ["dequeue", "first", "last"]
        .into_iter()
        .find(|b| name.starts_with(b) && name.len() > b.len())
        .unwrap_or(name);
    let empty = |rt: &Rt| {
        rt.heap
            .get(q.r() as usize)
            .is_none_or(|o| matches!(&o.data, Data::List(l) if l.is_empty()))
    };
    Some(match base {
        // `add`/`offer` (true) and fastutil's `enqueue`: the element moves up from the end.
        "add" | "offer" | "enqueue" if crate::jlang::kinds(desc).len() == 1 => {
            let mut es = take(rt, q);
            es.push(a(1));
            let k = es.len() - 1;
            sift_up(rt, &mut es, cmp, java, k, a(1));
            put(rt, q, es);
            if desc.ends_with(")Z") {
                V::I(1)
            } else {
                V::Void
            }
        }
        "addAll" => {
            let items = elements(rt, a(1));
            let any = !items.is_empty();
            let mut es = take(rt, q);
            for x in items {
                es.push(x);
                let k = es.len() - 1;
                sift_up(rt, &mut es, cmp, java, k, x);
            }
            put(rt, q, es);
            V::I(any as i32)
        }
        // The least: `peek` null when empty; `element`, fastutil's `first` throw.
        "peek" | "element" | "first" => {
            if empty(rt) {
                if name == "peek" {
                    return Some(V::NULL);
                }
                rt.throw_new("java/util/NoSuchElementException");
                return Some(V::NULL);
            }
            let first = rt.heap.get(q.r() as usize).and_then(|o| match &o.data {
                Data::List(l) => l.first().copied(),
                _ => None,
            });
            first.unwrap_or(V::NULL)
        }
        // A heap queue keeps no last (fastutil's `AbstractPriorityQueue.last`).
        "last" => {
            rt.throw_new("java/lang/UnsupportedOperationException");
            V::NULL
        }
        // The least taken out: the last element moves to the root, then down. `poll` gives null
        // when empty; `remove()` and fastutil's `dequeue` throw.
        "poll" | "dequeue" | "remove" if !desc.starts_with("(L") => {
            if empty(rt) {
                if name == "poll" {
                    return Some(V::NULL);
                }
                rt.throw_new("java/util/NoSuchElementException");
                return Some(V::NULL);
            }
            let mut es = take(rt, q);
            let result = es[0];
            remove_at(rt, &mut es, cmp, java, 0);
            put(rt, q, es);
            result
        }
        // `remove(o)`: the first element equal to it.
        "remove" => {
            let mut es = take(rt, q);
            let at = es.iter().position(|&e| equals(rt, a(1), e));
            if let Some(i) = at {
                remove_at(rt, &mut es, cmp, java, i);
            }
            put(rt, q, es);
            V::I(at.is_some() as i32)
        }
        // `removeIf`, `removeAll`, `retainAll`: what stays, heapified again (Java's `bulkRemove`).
        "removeIf" | "removeAll" | "retainAll" => {
            let es = take(rt, q);
            let other = if name == "removeIf" {
                Vec::new()
            } else {
                elements(rt, a(1))
            };
            let mut kept = Vec::with_capacity(es.len());
            for &e in &es {
                let hit = match name {
                    "removeIf" => {
                        rt.call_functional(
                            "java/util/function/Predicate.test.(Ljava/lang/Object;)Z",
                            a(1),
                            &[e],
                        )
                        .i() != 0
                    }
                    _ => other.iter().any(|&o| equals(rt, o, e)),
                };
                if hit == (name == "retainAll") {
                    kept.push(e);
                }
            }
            let changed = kept.len() != es.len();
            if changed {
                heapify(rt, &mut kept, cmp, java);
            }
            put(rt, q, if changed { kept } else { es });
            V::I(changed as i32)
        }
        // fastutil's `changed()`: the root, which the caller changed, moves down.
        "changed" => {
            let mut es = take(rt, q);
            if let Some(&x) = es.first() {
                let n = es.len();
                sift_down(rt, &mut es, cmp, java, 0, x, n);
            }
            put(rt, q, es);
            V::Void
        }
        "comparator" => cmp,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    const PQ: &str = "java/util/PriorityQueue";
    const FU: &str = "it/unimi/dsi/fastutil/objects/ObjectHeapPriorityQueue";

    fn ints(rt: &mut Rt, xs: &[i32]) -> Vec<V> {
        xs.iter()
            .map(|&x| crate::mixin::box_value(rt, V::I(x), 'I'))
            .collect()
    }

    fn value(rt: &Rt, v: V) -> i32 {
        rt.get_field(v, "value").i()
    }

    /// Java's `PriorityQueue` gives its least first, whatever the order they went in, and keeps
    /// its array as Java's does: `[5, 1, 4, 2, 3]` added one by one is `[1, 2, 4, 5, 3]`, which is
    /// also the order it iterates in.
    #[test]
    fn a_priority_queue_gives_its_least_first_in_javas_heap_order() {
        let mut rt = Rt::new();
        crate::install();
        let q = rt.new_object(PQ);
        c(&mut rt, "java/util/PriorityQueue.<init>.()V", &[q]);
        for x in ints(&mut rt, &[5, 1, 4, 2, 3]) {
            assert_eq!(
                c(
                    &mut rt,
                    "java/util/PriorityQueue.add.(Ljava/lang/Object;)Z",
                    &[q, x]
                )
                .i(),
                1
            );
        }
        let order: Vec<i32> = elements(&rt, q).iter().map(|&v| value(&rt, v)).collect();
        assert_eq!(order, [1, 2, 4, 5, 3]);
        let peek = c(
            &mut rt,
            "java/util/PriorityQueue.peek.()Ljava/lang/Object;",
            &[q],
        );
        assert_eq!(value(&rt, peek), 1);
        let mut out = Vec::new();
        while c(&mut rt, "java/util/PriorityQueue.isEmpty.()Z", &[q]).i() == 0 {
            let x = c(
                &mut rt,
                "java/util/PriorityQueue.poll.()Ljava/lang/Object;",
                &[q],
            );
            out.push(value(&rt, x));
        }
        assert_eq!(out, [1, 2, 3, 4, 5]);
        assert_eq!(
            c(
                &mut rt,
                "java/util/PriorityQueue.poll.()Ljava/lang/Object;",
                &[q]
            ),
            V::NULL
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A comparator orders the queue (here largest first); `remove(o)` takes one out and keeps the
    /// rest a heap.
    #[test]
    fn a_priority_queue_follows_its_comparator() {
        let mut rt = Rt::new();
        crate::install();
        let natural = c(
            &mut rt,
            "java/util/Comparator.naturalOrder.()Ljava/util/Comparator;",
            &[],
        );
        let reversed = c(
            &mut rt,
            "java/util/Comparator.reversed.()Ljava/util/Comparator;",
            &[natural],
        );
        let q = rt.new_object(PQ);
        c(
            &mut rt,
            "java/util/PriorityQueue.<init>.(Ljava/util/Comparator;)V",
            &[q, reversed],
        );
        let xs = ints(&mut rt, &[3, 9, 1, 7, 5]);
        for &x in &xs {
            c(
                &mut rt,
                "java/util/PriorityQueue.offer.(Ljava/lang/Object;)Z",
                &[q, x],
            );
        }
        let seven = crate::mixin::box_value(&mut rt, V::I(7), 'I');
        assert_eq!(
            c(
                &mut rt,
                "java/util/PriorityQueue.remove.(Ljava/lang/Object;)Z",
                &[q, seven]
            )
            .i(),
            1
        );
        let mut out = Vec::new();
        for _ in 0..4 {
            let x = c(
                &mut rt,
                "java/util/PriorityQueue.poll.()Ljava/lang/Object;",
                &[q],
            );
            out.push(value(&rt, x));
        }
        assert_eq!(out, [9, 5, 3, 1]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// fastutil's heap queue, as Create's hose pulley fills a lake with one: `enqueue`, `first`,
    /// `dequeue` least first, `clear`, and `dequeue` on an empty queue throws.
    #[test]
    fn fastutils_heap_queue_dequeues_its_least() {
        let mut rt = Rt::new();
        crate::install();
        let natural = c(
            &mut rt,
            "java/util/Comparator.naturalOrder.()Ljava/util/Comparator;",
            &[],
        );
        let q = rt.new_object(FU);
        c(
            &mut rt,
            "it/unimi/dsi/fastutil/objects/ObjectHeapPriorityQueue.<init>.(Ljava/util/Comparator;)V",
            &[q, natural],
        );
        for x in ints(&mut rt, &[4, 8, 2, 6]) {
            c(
                &mut rt,
                "it/unimi/dsi/fastutil/PriorityQueue.enqueue.(Ljava/lang/Object;)V",
                &[q, x],
            );
        }
        let first = c(
            &mut rt,
            "it/unimi/dsi/fastutil/PriorityQueue.first.()Ljava/lang/Object;",
            &[q],
        );
        assert_eq!(value(&rt, first), 2);
        let mut out = Vec::new();
        for _ in 0..2 {
            let x = c(
                &mut rt,
                "it/unimi/dsi/fastutil/PriorityQueue.dequeue.()Ljava/lang/Object;",
                &[q],
            );
            out.push(value(&rt, x));
        }
        assert_eq!(out, [2, 4]);
        assert_eq!(
            c(
                &mut rt,
                "it/unimi/dsi/fastutil/PriorityQueue.size.()I",
                &[q]
            )
            .i(),
            2
        );
        c(
            &mut rt,
            "it/unimi/dsi/fastutil/PriorityQueue.clear.()V",
            &[q],
        );
        assert_eq!(
            c(
                &mut rt,
                "it/unimi/dsi/fastutil/PriorityQueue.isEmpty.()Z",
                &[q]
            )
            .i(),
            1
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        c(
            &mut rt,
            "it/unimi/dsi/fastutil/PriorityQueue.dequeue.()Ljava/lang/Object;",
            &[q],
        );
        let thrown = rt.thrown.take().expect("an empty queue's dequeue throws");
        assert_eq!(rt.class_of(thrown), "java/util/NoSuchElementException");
    }

    /// A queue made from a collection heapifies it (`PriorityQueue(c)`): `[5, 4, 3, 2, 1]` becomes
    /// Java's `[1, 2, 3, 5, 4]`.
    #[test]
    fn a_queue_made_from_a_collection_is_heapified() {
        let mut rt = Rt::new();
        crate::install();
        let xs = ints(&mut rt, &[5, 4, 3, 2, 1]);
        let list = crate::jutil::new_list(&mut rt, xs);
        let q = rt.new_object(PQ);
        c(
            &mut rt,
            "java/util/PriorityQueue.<init>.(Ljava/util/Collection;)V",
            &[q, list],
        );
        let order: Vec<i32> = elements(&rt, q).iter().map(|&v| value(&rt, v)).collect();
        assert_eq!(order, [1, 2, 3, 5, 4]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
