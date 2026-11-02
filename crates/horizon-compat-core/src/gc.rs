//! A mark-and-sweep collector for the runtime's heap. Objects reachable from the runtime's roots
//! stay; the rest are freed, and their slots are reused by later allocations (references are heap
//! indices, so live objects never move).
//!
//! It runs only at a safe point: when a hook enters translated code from the game and no translated
//! frame is running (generated code keeps values in Rust locals the collector cannot see). Anything
//! that holds a value across hook calls keeps it in a root: the runtime's statics, registries,
//! interned values and strings, listeners, block entities, pending suppliers, or [`Rt::pinned`].
use crate::{Data, LAMBDA, Object, Rt, V};

/// Allocations between collections, at least: below it a collection costs more than it frees.
const MIN_INTERVAL: usize = 200_000;

impl Rt {
    /// Collects when enough was allocated since the last collection. Call only at a safe point.
    pub fn maybe_collect(&mut self) {
        let live = self.heap.len() - self.free.len();
        if live >= self.collect_at {
            let survivors = self.collect();
            self.collect_at = (survivors * 2).max(survivors + MIN_INTERVAL);
        }
    }

    /// Frees every object the roots don't reach; returns how many objects stay. Call only at a safe
    /// point (no translated frame running).
    pub fn collect(&mut self) -> usize {
        let n = self.heap.len();
        let mut marked = vec![false; n];
        let mut todo: Vec<u32> = Vec::new();
        let push = |v: V, todo: &mut Vec<u32>| {
            if let V::R(r) = v
                && r != 0
            {
                todo.push(r);
            }
        };
        todo.extend(self.strings.values().copied());
        for &v in self.statics.values().chain(self.registered.values()).chain(self.interned.values()).chain(self.block_entities.values()).chain(self.pinned.iter()) {
            push(v, &mut todo);
        }
        for (_, v) in &self.listeners {
            push(*v, &mut todo);
        }
        for (_, _, v) in &self.suppliers {
            push(*v, &mut todo);
        }
        if let Some(e) = self.thrown {
            push(e, &mut todo);
        }
        self.mark(&mut marked, &mut todo);
        // An iterator's source stays while the iterator does (weak keys, as an ephemeron).
        loop {
            let before = todo.len();
            for (&it, &src) in &self.iter_sources {
                if marked.get(it as usize).copied().unwrap_or(false) {
                    push(src, &mut todo);
                }
            }
            if todo.len() == before {
                break;
            }
            self.mark(&mut marked, &mut todo);
        }
        self.iter_sources.retain(|it, _| marked.get(*it as usize).copied().unwrap_or(false));
        self.lambda_info.retain(|l, _| marked.get(*l as usize).copied().unwrap_or(false));
        let mut survivors = 1;
        for (r, live) in marked.iter().enumerate().skip(1) {
            if *live {
                survivors += 1;
            } else if self.heap[r].class != FREE {
                self.heap[r] = Object { class: FREE, cid: 0, data: Data::Native(FREE, [0; 4]) };
                self.free.push(r as u32);
            }
        }
        survivors
    }

    fn mark(&self, marked: &mut [bool], todo: &mut Vec<u32>) {
        while let Some(r) = todo.pop() {
            let Some(seen) = marked.get_mut(r as usize) else { continue };
            if *seen {
                continue;
            }
            *seen = true;
            let refs = |v: &V, todo: &mut Vec<u32>| {
                if let V::R(x) = *v
                    && x != 0
                {
                    todo.push(x);
                }
            };
            match &self.heap[r as usize].data {
                Data::Fields(f) => f.values().for_each(|v| refs(v, todo)),
                Data::Array(a) | Data::List(a) | Data::Stream(a) | Data::Iter(a, _) => a.iter().for_each(|v| refs(v, todo)),
                Data::Map(m) => m.values().for_each(|v| refs(v, todo)),
                Data::Table(t) => {
                    for (k, v) in &t.entries {
                        refs(k, todo);
                        refs(v, todo);
                    }
                    refs(&t.comparator, todo);
                }
                Data::Str(_) | Data::Native(..) => {}
            }
        }
    }
}

/// The class of a freed slot.
pub const FREE: &str = "compat:free";

/// Whether `cid` is a lambda's (kept here so the collector's users need not know the bit).
pub fn is_lambda(cid: u32) -> bool {
    cid & LAMBDA != 0
}

#[cfg(test)]
mod tests {
    use crate::{Data, Rt, V};

    #[test]
    fn unreachable_objects_are_freed_and_their_slots_reused() {
        let mut rt = Rt::new();
        // Reachable: a static holding an object that holds another, and a registered object.
        let inner = rt.new_object("t/Inner");
        let outer = rt.new_object("t/Outer");
        rt.put_field(outer, "next", inner);
        rt.statics.insert("t/Outer.ROOT", outer);
        let kept = rt.alloc("[I", Data::Array(vec![V::I(7)]));
        rt.registered.insert(("block".into(), "t:kept".into()), kept);
        // Unreachable: a cycle and a list of garbage.
        let a = rt.new_object("t/A");
        let b = rt.new_object("t/B");
        rt.put_field(a, "b", b);
        rt.put_field(b, "a", a);
        let junk = rt.new_object("t/Junk");
        let list = rt.alloc("java/util/ArrayList", Data::List(vec![junk]));
        let before = rt.heap.len();
        let survivors = rt.collect();
        assert_eq!(survivors, before - 4);
        assert_eq!(rt.class_of(rt.get_field(outer, "next")), "t/Inner");
        assert_eq!(rt.class_of(kept), "[I");
        for gone in [a, b, junk, list] {
            assert_eq!(rt.class_of(gone), super::FREE);
        }
        // New objects take the freed slots; the heap doesn't grow.
        let fresh: Vec<V> = (0..4).map(|_| rt.new_object("t/Fresh")).collect();
        assert_eq!(rt.heap.len(), before);
        assert!(fresh.iter().all(|v| [a, b, junk, list].contains(v)));
    }
}
