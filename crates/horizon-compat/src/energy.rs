//! NeoForge's energy (FE), natively: `EnergyStorage` (a machine's or a battery's buffer) as NeoForge
//! writes it, a mod's subclass of it included (its own `canReceive`, `canExtract` or
//! `getEnergyStored` answer, through the mod's code), and `EmptyEnergyStorage`.
//!
//! The buffer's state is NeoForge's own protected fields, `energy`, `capacity`, `maxReceive` and
//! `maxExtract`, so a subclass that reads or sets `this.energy` sees what these methods wrote. Mods
//! hand their buffers to each other through capabilities (`Capabilities.EnergyStorage.BLOCK`), so
//! another mod's cable fills this mod's machine through `IEnergyStorage`.
use crate::{Rt, V, api, tanks};

const STORAGE: &str = "net/neoforged/neoforge/energy/EnergyStorage";
const EMPTY: &str = "net/neoforged/neoforge/energy/EmptyEnergyStorage";
const ENERGY: &str = "net/neoforged/neoforge/energy/IEnergyStorage";
const SERIALIZABLE: &str = "net/neoforged/neoforge/common/util/INBTSerializable";

// The buffer's overridable steps, called virtually: (the API's key, the method's).
const CAN_RECEIVE: (&str, &str) = (
    "net/neoforged/neoforge/energy/EnergyStorage.canReceive.()Z",
    "canReceive.()Z",
);
const CAN_EXTRACT: (&str, &str) = (
    "net/neoforged/neoforge/energy/EnergyStorage.canExtract.()Z",
    "canExtract.()Z",
);
const GET_STORED: (&str, &str) = (
    "net/neoforged/neoforge/energy/EnergyStorage.getEnergyStored.()I",
    "getEnergyStored.()I",
);
const INT_TAG: &str = "net/minecraft/nbt/IntTag";
const INT_VALUE_OF: &str = "net/minecraft/nbt/IntTag.valueOf.(I)Lnet/minecraft/nbt/IntTag;";
const INT_GET: &str = "net/minecraft/nbt/IntTag.getAsInt.()I";

/// Whether `c` is `class` among the energy classes (`instanceof IEnergyStorage`).
pub(crate) fn is_a(c: &str, class: &str) -> bool {
    match class {
        ENERGY => matches!(c, STORAGE | EMPTY),
        SERIALIZABLE => c == STORAGE,
        _ => false,
    }
}

fn field(rt: &Rt, s: V, name: &'static str) -> i32 {
    rt.get_field(s, name).i()
}

/// `EnergyStorage`'s methods on buffer `a(0)`.
fn storage_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    Some(match name {
        // (capacity[, maxTransfer | maxReceive, maxExtract[, energy]]): the energy within
        // `0..=capacity`.
        "<init>" => {
            let capacity = a(1).i();
            let (receive, extract, energy) = match desc {
                "(II)V" => (a(2).i(), a(2).i(), 0),
                "(III)V" => (a(2).i(), a(3).i(), 0),
                "(IIII)V" => (a(2).i(), a(3).i(), a(4).i()),
                _ => (capacity, capacity, 0),
            };
            rt.put_field(s, "capacity", V::I(capacity));
            rt.put_field(s, "maxReceive", V::I(receive));
            rt.put_field(s, "maxExtract", V::I(extract));
            rt.put_field(s, "energy", V::I(energy.min(capacity).max(0)));
            V::Void
        }
        // What the buffer takes of `n`: as much as it has room for, up to `maxReceive`; nothing
        // when it doesn't receive. A simulation leaves the buffer as it was.
        "receiveEnergy" => {
            let (n, simulate) = (a(1).i(), a(2).i() != 0);
            if tanks::virt(rt, CAN_RECEIVE, &[s]).i() == 0 || n <= 0 {
                return Some(V::I(0));
            }
            let (energy, capacity, max) = (
                field(rt, s, "energy"),
                field(rt, s, "capacity"),
                field(rt, s, "maxReceive"),
            );
            // `Mth.clamp(capacity - energy, 0, min(maxReceive, n))`.
            let took = capacity.wrapping_sub(energy).max(0).min(max.min(n));
            if !simulate {
                rt.put_field(s, "energy", V::I(energy.wrapping_add(took)));
            }
            V::I(took)
        }
        // What the buffer gives of `n`: as much as it holds, up to `maxExtract`.
        "extractEnergy" => {
            let (n, simulate) = (a(1).i(), a(2).i() != 0);
            if tanks::virt(rt, CAN_EXTRACT, &[s]).i() == 0 || n <= 0 {
                return Some(V::I(0));
            }
            let (energy, max) = (field(rt, s, "energy"), field(rt, s, "maxExtract"));
            let gave = energy.min(max.min(n));
            if !simulate {
                rt.put_field(s, "energy", V::I(energy.wrapping_sub(gave)));
            }
            V::I(gave)
        }
        "getEnergyStored" => V::I(field(rt, s, "energy")),
        "getMaxEnergyStored" => V::I(field(rt, s, "capacity")),
        "canExtract" => V::I(i32::from(field(rt, s, "maxExtract") > 0)),
        "canReceive" => V::I(i32::from(field(rt, s, "maxReceive") > 0)),
        // Saved as an `IntTag` of what it holds, and loaded from one; anything else throws, as
        // NeoForge's does.
        "serializeNBT" => {
            let n = tanks::virt(rt, GET_STORED, &[s]);
            api::call(rt, INT_VALUE_OF, &[n])
        }
        "deserializeNBT" => {
            let tag = a(2);
            if !rt.instance_of(tag, INT_TAG) {
                let message =
                    "Can not deserialize to an instance that isn't the default implementation";
                return Some(crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalArgumentException",
                    message,
                ));
            }
            let n = api::call(rt, INT_GET, &[tag]);
            rt.put_field(s, "energy", n);
            V::Void
        }
        _ => return None,
    })
}

/// `EmptyEnergyStorage`: nothing in, nothing out, nothing held.
fn empty_call(name: &str) -> Option<V> {
    Some(match name {
        "<init>" => V::Void,
        "receiveEnergy" | "extractEnergy" | "getEnergyStored" | "getMaxEnergyStored"
        | "canExtract" | "canReceive" => V::I(0),
        _ => return None,
    })
}

/// Static fields this module owns: `EmptyEnergyStorage.INSTANCE`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    (owner == EMPTY && field == "INSTANCE")
        .then(|| rt.intern("compat:empty-energy".into(), |rt| rt.new_object(EMPTY)))
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, STORAGE | EMPTY | ENERGY | SERIALIZABLE)
}

/// [`call`] by the call's key (`owner.name.desc`).
pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on energy buffers, directly or through the interfaces they implement; None for anything
/// else (a mod's own `IEnergyStorage` answers through its own code).
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    match owner {
        STORAGE => storage_call(rt, name, desc, args),
        EMPTY => empty_call(name),
        ENERGY | SERIALIZABLE if rt.instance_of(this, STORAGE) => {
            storage_call(rt, name, desc, args)
        }
        ENERGY if rt.instance_of(this, EMPTY) => empty_call(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INIT: &str = "net/neoforged/neoforge/energy/EnergyStorage.<init>.(III)V";
    const RECEIVE: &str = "net/neoforged/neoforge/energy/IEnergyStorage.receiveEnergy.(IZ)I";
    const EXTRACT: &str = "net/neoforged/neoforge/energy/IEnergyStorage.extractEnergy.(IZ)I";
    const STORED: &str = "net/neoforged/neoforge/energy/IEnergyStorage.getEnergyStored.()I";

    fn rt() -> Rt {
        crate::install();
        Rt::new()
    }

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    fn cell(rt: &mut Rt, class: &'static str, [capacity, receive, extract]: [i32; 3]) -> V {
        let s = rt.new_object(class);
        c(rt, INIT, &[s, V::I(capacity), V::I(receive), V::I(extract)]);
        s
    }

    /// A buffer of 1000 taking at most 300 and giving at most 200 at a time, through the interface
    /// other mods use: it fills to its capacity, a simulation changes nothing, and it gives what it
    /// holds.
    #[test]
    fn a_buffer_takes_and_gives_within_its_limits() {
        let mut rt = rt();
        let s = cell(&mut rt, STORAGE, [1000, 300, 200]);
        let take = |rt: &mut Rt, n: i32, simulate: bool| {
            c(rt, RECEIVE, &[s, V::I(n), V::I(i32::from(simulate))]).i()
        };
        assert_eq!(take(&mut rt, 500, true), 300);
        assert_eq!(
            c(&mut rt, STORED, &[s]),
            V::I(0),
            "a simulation takes nothing"
        );
        assert_eq!(
            [500, 500, 500, 500].map(|n| take(&mut rt, n, false)),
            [300, 300, 300, 100]
        );
        assert_eq!(take(&mut rt, 50, false), 0, "full");
        assert_eq!(c(&mut rt, STORED, &[s]), V::I(1000));
        let give = |rt: &mut Rt, n: i32, simulate: bool| {
            c(rt, EXTRACT, &[s, V::I(n), V::I(i32::from(simulate))]).i()
        };
        assert_eq!(
            (
                give(&mut rt, 500, true),
                give(&mut rt, 500, false),
                give(&mut rt, 150, false)
            ),
            (200, 200, 150)
        );
        assert_eq!((give(&mut rt, 0, false), give(&mut rt, -5, false)), (0, 0));
        assert_eq!(c(&mut rt, STORED, &[s]), V::I(650));
        // NeoForge's own field, as a subclass reads it.
        assert_eq!(rt.get_field(s, "energy"), V::I(650));

        // A buffer that only gives (`maxReceive` 0) takes nothing; one made full past its capacity
        // holds its capacity.
        let source = cell(&mut rt, STORAGE, [100, 0, 50]);
        let can = |rt: &mut Rt, s: V, what: &'static str| c(rt, what, &[s]);
        assert_eq!(c(&mut rt, RECEIVE, &[source, V::I(10), V::I(0)]), V::I(0));
        assert_eq!(
            can(
                &mut rt,
                source,
                "net/neoforged/neoforge/energy/IEnergyStorage.canReceive.()Z"
            ),
            V::I(0)
        );
        assert_eq!(
            can(
                &mut rt,
                source,
                "net/neoforged/neoforge/energy/IEnergyStorage.canExtract.()Z"
            ),
            V::I(1)
        );
        let full = rt.new_object(STORAGE);
        c(
            &mut rt,
            "net/neoforged/neoforge/energy/EnergyStorage.<init>.(IIII)V",
            &[full, V::I(100), V::I(10), V::I(10), V::I(250)],
        );
        assert_eq!(c(&mut rt, STORED, &[full]), V::I(100));
        // `EnergyStorage(capacity)` takes and gives its whole capacity at once.
        let open = rt.new_object(STORAGE);
        c(
            &mut rt,
            "net/neoforged/neoforge/energy/EnergyStorage.<init>.(I)V",
            &[open, V::I(64)],
        );
        assert_eq!(c(&mut rt, RECEIVE, &[open, V::I(100), V::I(0)]), V::I(64));
        assert!(rt.instance_of(s, ENERGY) && rt.instance_of(s, SERIALIZABLE));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's buffer that never gives (its own `canExtract`) keeps what it has; saved, it is an
    /// `IntTag` of what it holds, and a buffer loads it back; the empty buffer has nothing.
    #[test]
    fn a_subclass_rules_and_a_buffer_saves_what_it_holds() {
        let mut rt = rt();
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            if rt.class_of(args[0]) == "test/OneWayCell" && short == "canExtract.()Z" {
                return V::I(0);
            }
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        rt.supers.insert("test/OneWayCell", STORAGE);
        let s = cell(&mut rt, "test/OneWayCell", [1000, 100, 100]);
        assert_eq!(c(&mut rt, RECEIVE, &[s, V::I(80), V::I(0)]), V::I(80));
        assert_eq!(c(&mut rt, EXTRACT, &[s, V::I(50), V::I(0)]), V::I(0));

        let saved = c(
            &mut rt,
            "net/neoforged/neoforge/common/util/INBTSerializable.serializeNBT.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/Tag;",
            &[s, V::NULL],
        );
        assert_eq!(
            (rt.class_of(saved), c(&mut rt, INT_GET, &[saved])),
            (INT_TAG, V::I(80))
        );
        let load = "net/neoforged/neoforge/common/util/INBTSerializable.deserializeNBT.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/Tag;)V";
        let other = cell(&mut rt, STORAGE, [1000, 100, 100]);
        c(&mut rt, load, &[other, V::NULL, saved]);
        assert_eq!(c(&mut rt, STORED, &[other]), V::I(80));
        let wrong = crate::tags::new_compound(&mut rt);
        c(&mut rt, load, &[other, V::NULL, wrong]);
        let thrown = rt.thrown.take().expect("a compound isn't an energy amount");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");

        let empty = crate::api::get_static(
            &mut rt,
            "net/neoforged/neoforge/energy/EmptyEnergyStorage.INSTANCE",
        );
        assert_eq!(
            empty,
            crate::api::get_static(
                &mut rt,
                "net/neoforged/neoforge/energy/EmptyEnergyStorage.INSTANCE"
            )
        );
        assert_eq!(
            (
                c(&mut rt, RECEIVE, &[empty, V::I(10), V::I(0)]),
                c(&mut rt, STORED, &[empty])
            ),
            (V::I(0), V::I(0))
        );
        assert!(rt.instance_of(empty, ENERGY));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
