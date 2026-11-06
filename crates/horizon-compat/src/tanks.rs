//! NeoForge's fluid handlers, natively: `FluidTank` (a block entity's tank: Create's basins,
//! mixers' inputs, fluid tanks, spouts...) as NeoForge writes it, a mod's subclass of it included
//! (its own `onContentsChanged` or `isFluidValid` answer, through the mod's code), and
//! `IFluidHandler.FluidAction`.
//!
//! The tank's state is NeoForge's own protected fields, `fluid`, `capacity` and `validator`, so a
//! subclass that reads `this.fluid` sees what these methods wrote.
use crate::{Rt, V, api, fluids, nbt};
use std::collections::BTreeMap;

const TANK: &str = "net/neoforged/neoforge/fluids/capability/templates/FluidTank";
const ACTION: &str = "net/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction";
/// Interfaces a tank's methods are called through: answered when the receiver is a tank.
const HANDLERS: [&str; 2] = ["net/neoforged/neoforge/fluids/capability/IFluidHandler", "net/neoforged/neoforge/fluids/IFluidTank"];

// The tank's overridable steps, called virtually: (the API's key, the method's).
const ON_CHANGED: (&str, &str) = ("net/neoforged/neoforge/fluids/capability/templates/FluidTank.onContentsChanged.()V", "onContentsChanged.()V");
const IS_VALID: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/templates/FluidTank.isFluidValid.(Lnet/neoforged/neoforge/fluids/FluidStack;)Z",
    "isFluidValid.(Lnet/neoforged/neoforge/fluids/FluidStack;)Z",
);
const GET_FLUID: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/templates/FluidTank.getFluid.()Lnet/neoforged/neoforge/fluids/FluidStack;",
    "getFluid.()Lnet/neoforged/neoforge/fluids/FluidStack;",
);
const GET_CAPACITY: (&str, &str) = ("net/neoforged/neoforge/fluids/capability/templates/FluidTank.getCapacity.()I", "getCapacity.()I");
const DRAIN: (&str, &str) = (
    "net/neoforged/neoforge/fluids/capability/templates/FluidTank.drain.(ILnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;",
    "drain.(ILnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;",
);
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";

/// A method that may be the mod's own: its override, else the API's.
pub(crate) fn virt(rt: &mut Rt, (owner_key, key): (&'static str, &'static str), args: &[V]) -> V {
    match rt.vcall {
        Some(vc) => vc(rt, owner_key, key, args),
        None => api::call(rt, owner_key, args),
    }
}

/// `FluidAction.EXECUTE` (0) or `SIMULATE` (1), one object each.
pub fn action(rt: &mut Rt, simulate: bool) -> V {
    if simulate { api::api_enum(rt, ACTION, "SIMULATE", 1) } else { api::api_enum(rt, ACTION, "EXECUTE", 0) }
}

/// Whether a `FluidAction` is `SIMULATE`.
pub(crate) fn simulating(rt: &Rt, action: V) -> bool {
    rt.get_field(action, "$ordinal").i() == 1
}

/// The tank's stack (`FluidStack.EMPTY` before anything was put in).
fn stack(rt: &mut Rt, tank: V) -> V {
    match rt.get_field(tank, "fluid") {
        s @ V::R(n) if n != 0 => s,
        _ => fluids::fluid_stack_empty(rt),
    }
}

fn capacity(rt: &Rt, tank: V) -> i32 {
    rt.get_field(tank, "capacity").i()
}

fn same_fluid(rt: &Rt, x: V, y: V) -> bool {
    rt.get_field(x, "$fluid") == rt.get_field(y, "$fluid")
}

/// `FluidTank.fill`: into an empty tank up to its capacity, or onto the same fluid; how much went in.
fn fill(rt: &mut Rt, tank: V, resource: V, simulate: bool) -> i32 {
    if fluids::stack_empty(rt, resource) || virt(rt, IS_VALID, &[tank, resource]).i() == 0 {
        return 0;
    }
    let (have, cap, amount) = (stack(rt, tank), capacity(rt, tank), fluids::stack_amount(rt, resource));
    let empty = fluids::stack_empty(rt, have);
    if !empty && !same_fluid(rt, have, resource) {
        return 0;
    }
    if simulate {
        return if empty { cap.min(amount) } else { (cap - fluids::stack_amount(rt, have)).min(amount) };
    }
    if empty {
        let f = rt.get_field(resource, "$fluid");
        let put = fluids::fluid_stack(rt, f, cap.min(amount));
        rt.put_field(tank, "fluid", put);
        virt(rt, ON_CHANGED, &[tank]);
        return cap.min(amount);
    }
    let room = cap - fluids::stack_amount(rt, have);
    let filled = if amount < room {
        let n = fluids::stack_amount(rt, have) + amount;
        rt.put_field(have, "$amount", V::I(n));
        amount
    } else {
        rt.put_field(have, "$amount", V::I(cap));
        room
    };
    if filled > 0 {
        virt(rt, ON_CHANGED, &[tank]);
    }
    filled
}

/// `FluidTank.drain(int, action)`: up to `max` off the tank; what came out.
fn drain(rt: &mut Rt, tank: V, max: i32, simulate: bool) -> V {
    let have = stack(rt, tank);
    let drained = max.min(fluids::stack_amount(rt, have)).max(0);
    let out = if fluids::stack_empty(rt, have) || drained == 0 {
        fluids::fluid_stack_empty(rt)
    } else {
        let f = rt.get_field(have, "$fluid");
        fluids::fluid_stack(rt, f, drained)
    };
    if !simulate && drained > 0 {
        let left = fluids::stack_amount(rt, have) - drained;
        rt.put_field(have, "$amount", V::I(left));
        virt(rt, ON_CHANGED, &[tank]);
    }
    out
}

/// `FluidStack.save`: `{id, amount}` (components aren't kept on fluid stacks yet).
pub fn save_stack(rt: &Rt, s: V) -> BTreeMap<String, nbt::Nbt> {
    let mut m = BTreeMap::new();
    if !fluids::stack_empty(rt, s) {
        m.insert("id".into(), nbt::Nbt::String(fluids::name_of(rt, rt.get_field(s, "$fluid"))));
        m.insert("amount".into(), nbt::Nbt::Int(fluids::stack_amount(rt, s)));
    }
    m
}

/// `FluidStack.parseOptional`: the saved stack, or `EMPTY` for nothing or a fluid nobody registered.
pub fn parse_stack(rt: &mut Rt, tag: &BTreeMap<String, nbt::Nbt>) -> V {
    let id = match tag.get("id") {
        Some(nbt::Nbt::String(s)) => s.clone(),
        _ => return fluids::fluid_stack_empty(rt),
    };
    let amount = match tag.get("amount") {
        Some(nbt::Nbt::Int(n)) => *n,
        Some(nbt::Nbt::Short(n)) => i32::from(*n),
        Some(nbt::Nbt::Byte(n)) => i32::from(*n),
        Some(nbt::Nbt::Long(n)) => *n as i32,
        _ => 1000,
    };
    let f = fluids::fluid(rt, &id);
    if f.r() == 0 || amount <= 0 {
        return fluids::fluid_stack_empty(rt);
    }
    fluids::fluid_stack(rt, f, amount)
}

fn tank_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let t = a(0);
    Some(match name {
        // (capacity[, validator])
        "<init>" => {
            rt.put_field(t, "capacity", a(1));
            rt.put_field(t, "validator", if desc.starts_with("(ILjava/util/function/Predicate;") { a(2) } else { V::NULL });
            let e = fluids::fluid_stack_empty(rt);
            rt.put_field(t, "fluid", e);
            V::Void
        }
        "setCapacity" | "setValidator" => {
            rt.put_field(t, if name == "setCapacity" { "capacity" } else { "validator" }, a(1));
            t
        }
        "getCapacity" => V::I(capacity(rt, t)),
        "getFluid" => stack(rt, t),
        "setFluid" => {
            rt.put_field(t, "fluid", a(1));
            V::Void
        }
        "getFluidAmount" => {
            let s = stack(rt, t);
            V::I(fluids::stack_amount(rt, s))
        }
        "isEmpty" => {
            let s = stack(rt, t);
            V::I(fluids::stack_empty(rt, s) as i32)
        }
        "getSpace" => {
            let s = stack(rt, t);
            V::I((capacity(rt, t) - fluids::stack_amount(rt, s)).max(0))
        }
        "isFluidValid" if desc.starts_with("(I") => virt(rt, IS_VALID, &[t, a(2)]),
        "isFluidValid" => {
            let v = rt.get_field(t, "validator");
            if v.r() == 0 { V::I(1) } else { V::I((rt.call_functional(TEST, v, &[a(1)]).i() != 0) as i32) }
        }
        "getTanks" => V::I(1),
        "getFluidInTank" => virt(rt, GET_FLUID, &[t]),
        "getTankCapacity" => virt(rt, GET_CAPACITY, &[t]),
        "fill" => {
            let sim = simulating(rt, a(2));
            V::I(fill(rt, t, a(1), sim))
        }
        // (FluidStack, action): only the tank's own fluid comes out.
        "drain" if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidStack;") => {
            let have = stack(rt, t);
            if fluids::stack_empty(rt, a(1)) || !same_fluid(rt, a(1), have) {
                return Some(fluids::fluid_stack_empty(rt));
            }
            let n = fluids::stack_amount(rt, a(1));
            virt(rt, DRAIN, &[t, V::I(n), a(2)])
        }
        "drain" => {
            let sim = simulating(rt, a(2));
            drain(rt, t, a(1).i(), sim)
        }
        "onContentsChanged" => V::Void,
        // (Provider, CompoundTag)
        "readFromNBT" => {
            let tag = nbt::from_runtime(rt, a(2));
            let s = match tag.get("Fluid") {
                Some(nbt::Nbt::Compound(c)) => parse_stack(rt, c),
                _ => fluids::fluid_stack_empty(rt),
            };
            rt.put_field(t, "fluid", s);
            t
        }
        "writeToNBT" => {
            let s = stack(rt, t);
            if !fluids::stack_empty(rt, s) {
                let saved = save_stack(rt, s);
                let saved = nbt::to_runtime(rt, &saved);
                let k = rt.string("Fluid");
                api::call(rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[a(2), k, saved]);
            }
            a(2)
        }
        _ => return None,
    })
}

/// Static fields this module owns: `FluidAction.EXECUTE` and `SIMULATE`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (ACTION, "EXECUTE") => Some(action(rt, false)),
        (ACTION, "SIMULATE") => Some(action(rt, true)),
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner == ACTION || owner == TANK || HANDLERS.contains(&owner)
}

/// [`call`] by the call's key (`owner.name.desc`).
pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Calls on tanks and fluid actions; None for what this module doesn't model.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        ACTION => Some(match name {
            "execute" => V::I(!simulating(rt, a(0)) as i32),
            "simulate" => V::I(simulating(rt, a(0)) as i32),
            "values" => {
                let arr = rt.new_array(2, V::NULL);
                for (i, sim) in [false, true].into_iter().enumerate() {
                    let v = action(rt, sim);
                    rt.array_set(arr, i as i32, v);
                }
                arr
            }
            _ => return None,
        }),
        TANK => tank_call(rt, name, desc, args),
        o if HANDLERS.contains(&o) && !args.is_empty() && rt.instance_of(a(0), TANK) => tank_call(rt, name, desc, args),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> Rt {
        crate::install();
        Rt::new()
    }

    fn water(rt: &mut Rt, amount: i32) -> V {
        let w = fluids::fluid(rt, "minecraft:water");
        fluids::fluid_stack(rt, w, amount)
    }

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        api::call(rt, key, args)
    }

    const FILL: &str = "net/neoforged/neoforge/fluids/capability/IFluidHandler.fill.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)I";
    const DRAIN_INT: &str = "net/neoforged/neoforge/fluids/capability/IFluidHandler.drain.(ILnet/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction;)Lnet/neoforged/neoforge/fluids/FluidStack;";
    const AMOUNT: &str = "net/neoforged/neoforge/fluids/capability/templates/FluidTank.getFluidAmount.()I";

    #[test]
    fn a_tank_fills_to_its_capacity_drains_and_refuses_another_fluid() {
        let mut rt = rt();
        let tank = rt.new_object(TANK);
        c(&mut rt, "net/neoforged/neoforge/fluids/capability/templates/FluidTank.<init>.(I)V", &[tank, V::I(1000)]);
        let (exec, sim) = (api::get_static(&mut rt, "net/neoforged/neoforge/fluids/capability/IFluidHandler$FluidAction.EXECUTE"), action(&mut rt, true));

        // Simulating changes nothing; executing puts in what fits.
        let w = water(&mut rt, 600);
        assert_eq!(c(&mut rt, FILL, &[tank, w, sim]).i(), 600);
        assert_eq!(c(&mut rt, AMOUNT, &[tank]).i(), 0);
        assert_eq!(c(&mut rt, FILL, &[tank, w, exec]).i(), 600);
        let w = water(&mut rt, 600);
        assert_eq!(c(&mut rt, FILL, &[tank, w, exec]).i(), 400, "only the room left goes in");
        assert_eq!(c(&mut rt, AMOUNT, &[tank]).i(), 1000);

        // Another fluid doesn't mix in.
        let lava = fluids::fluid(&mut rt, "minecraft:lava");
        let l = fluids::fluid_stack(&mut rt, lava, 100);
        assert_eq!(c(&mut rt, FILL, &[tank, l, exec]).i(), 0);

        let out = c(&mut rt, DRAIN_INT, &[tank, V::I(250), exec]);
        assert_eq!(fluids::stack_amount(&rt, out), 250);
        assert_eq!(fluids::name_of(&rt, rt.get_field(out, "$fluid")), "minecraft:water");
        assert_eq!(c(&mut rt, AMOUNT, &[tank]).i(), 750);
    }

    #[test]
    fn a_tank_saves_its_fluid_and_loads_it_back() {
        let mut rt = rt();
        let tank = rt.new_object(TANK);
        c(&mut rt, "net/neoforged/neoforge/fluids/capability/templates/FluidTank.<init>.(I)V", &[tank, V::I(4000)]);
        let exec = action(&mut rt, false);
        let w = water(&mut rt, 1500);
        c(&mut rt, FILL, &[tank, w, exec]);
        let tag = crate::tags::new_compound(&mut rt);
        c(&mut rt, "net/neoforged/neoforge/fluids/capability/templates/FluidTank.writeToNBT.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;", &[tank, V::NULL, tag]);
        let saved = nbt::from_runtime(&rt, tag);
        let mut fluid = BTreeMap::new();
        fluid.insert("id".to_owned(), nbt::Nbt::String("minecraft:water".into()));
        fluid.insert("amount".to_owned(), nbt::Nbt::Int(1500));
        assert_eq!(saved.get("Fluid"), Some(&nbt::Nbt::Compound(fluid)));

        let back = rt.new_object(TANK);
        c(&mut rt, "net/neoforged/neoforge/fluids/capability/templates/FluidTank.<init>.(I)V", &[back, V::I(4000)]);
        c(&mut rt, "net/neoforged/neoforge/fluids/capability/templates/FluidTank.readFromNBT.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)Lnet/neoforged/neoforge/fluids/capability/templates/FluidTank;", &[back, V::NULL, tag]);
        assert_eq!(c(&mut rt, AMOUNT, &[back]).i(), 1500);
    }
}
