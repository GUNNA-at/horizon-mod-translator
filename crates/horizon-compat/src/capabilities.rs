//! NeoForge's capabilities, natively. This covers:
//! - the capability constants (`Capabilities.ItemHandler.BLOCK`, `FluidHandler`, `EnergyStorage`)
//!   and a mod's own (`BlockCapability.create`);
//! - `RegisterCapabilitiesEvent`, fired once the registries are full as NeoForge fires it at the end
//!   of loading, with its block entity, block, item and entity registrations;
//! - the lookups: `Level.getCapability`, `BlockCapabilityCache` and `ItemStack.getCapability`.
//!
//! A block lookup asks the block's own providers first, then its block entity's, in the order
//! NeoForge's `CapabilityHooks` uses. Create's funnels, chutes, arms and basins reach each other's
//! inventories this way.
use crate::{Data, Rt, V, api};

const BLOCK_CAP: &str = "net/neoforged/neoforge/capabilities/BlockCapability";
const ITEM_CAP: &str = "net/neoforged/neoforge/capabilities/ItemCapability";
const ENTITY_CAP: &str = "net/neoforged/neoforge/capabilities/EntityCapability";
const EVENT: &str = "net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent";
const CACHE: &str = "net/neoforged/neoforge/capabilities/BlockCapabilityCache";
const LEVEL_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/ILevelExtension";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const PROVIDER: &str = "net/neoforged/neoforge/capabilities/ICapabilityProvider.getCapability.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const BLOCK_PROVIDER: &str = "net/neoforged/neoforge/capabilities/IBlockCapabilityProvider.getCapability.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/entity/BlockEntity;Ljava/lang/Object;)Ljava/lang/Object;";

/// What a registration is for: (kind, capability, the block entity type / block / item / entity
/// type it names, provider).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    BlockEntity = 0,
    Block = 1,
    Item = 2,
    Entity = 3,
}

/// The capability `name` of a kind (`neoforge:item_handler` names a block, an item and an entity
/// capability): one object each, as NeoForge's registry of capabilities keeps them.
fn capability(rt: &mut Rt, class: &'static str, name: &str) -> V {
    rt.intern(format!("compat:capability/{class}/{name}"), |rt| {
        let c = rt.new_object(class);
        let n = rt.string(name);
        rt.put_field(c, "$name", n);
        c
    })
}

/// `Capabilities.X.Y`, as NeoForge 21.1 names them.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    let group = owner.strip_prefix("net/neoforged/neoforge/capabilities/Capabilities$")?;
    let (class, name) = match (group, field) {
        ("ItemHandler", "BLOCK") => (BLOCK_CAP, "neoforge:item_handler"),
        ("ItemHandler", "ENTITY") => (ENTITY_CAP, "neoforge:item_handler"),
        ("ItemHandler", "ENTITY_AUTOMATION") => (ENTITY_CAP, "neoforge:item_handler_automation"),
        ("ItemHandler", "ITEM") => (ITEM_CAP, "neoforge:item_handler"),
        ("FluidHandler", "BLOCK") => (BLOCK_CAP, "neoforge:fluid_handler"),
        ("FluidHandler", "ENTITY") => (ENTITY_CAP, "neoforge:fluid_handler"),
        ("FluidHandler", "ITEM") => (ITEM_CAP, "neoforge:fluid_handler_item"),
        ("EnergyStorage", "BLOCK") => (BLOCK_CAP, "neoforge:energy"),
        ("EnergyStorage", "ENTITY") => (ENTITY_CAP, "neoforge:energy"),
        ("EnergyStorage", "ITEM") => (ITEM_CAP, "neoforge:energy"),
        _ => return None,
    };
    Some(capability(rt, class, name))
}

/// The registrations so far, four values each (kind, capability, key, provider).
fn registrations(rt: &mut Rt) -> V {
    rt.intern("compat:capability-providers".into(), |rt| rt.alloc("compat:list", Data::List(Vec::new())))
}

fn register(rt: &mut Rt, kind: Kind, cap: V, key: V, provider: V) {
    let list = registrations(rt);
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.extend([V::I(kind as i32), cap, key, provider]);
    }
}

fn entries(rt: &mut Rt) -> Vec<(Kind, V, V, V)> {
    let list = registrations(rt);
    let l = crate::jutil::elements(rt, list);
    l.chunks(4)
        .filter_map(|e| {
            let kind = match e[0].i() {
                0 => Kind::BlockEntity,
                1 => Kind::Block,
                2 => Kind::Item,
                _ => Kind::Entity,
            };
            (e.len() == 4).then_some((kind, e[1], e[2], e[3]))
        })
        .collect()
}

/// Fires `RegisterCapabilitiesEvent` once (the mod's listeners register their providers).
pub fn fire(rt: &mut Rt) {
    if rt.interned.contains_key("compat:capabilities-fired") {
        return;
    }
    rt.interned.insert("compat:capabilities-fired".into(), V::I(1));
    let ev = rt.new_object(EVENT);
    crate::neo::post(rt, EVENT, ev);
}

/// The item a key names (an item, a block, a holder...).
fn item_name(rt: &mut Rt, v: V) -> String {
    let it = crate::creative::as_item(rt, v);
    crate::api_common::item_like_name(rt, it)
}

/// The block capability `cap` at `pos` (`state`, `be` and `side` given, or found): the block's
/// providers first, then its block entity's.
fn find_block(rt: &mut Rt, cap: V, level: V, pos: V, state: V, be: V, side: V) -> V {
    let at = api::pos_of(rt, pos);
    let state = if state.r() != 0 {
        state
    } else {
        let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(at));
        api::state(rt, &text)
    };
    let be = if be.r() != 0 { be } else { crate::glue::block_entity_at(rt, at) };
    let block = crate::host::split_state(rt.str_of(state).unwrap_or("")).0.to_owned();
    for (kind, c, key, provider) in entries(rt) {
        if kind != Kind::Block || c != cap || api::block_name(rt, key) != block {
            continue;
        }
        let r = rt.call_functional(BLOCK_PROVIDER, provider, &[level, pos, state, be, side]);
        rt.report_thrown("getCapability");
        if r.r() != 0 {
            return r;
        }
    }
    if be.r() == 0 {
        return V::NULL;
    }
    let ty = rt.get_field(be, "$type");
    for (kind, c, key, provider) in entries(rt) {
        let same_type = key == ty || rt.registered_name(key, "block_entity_type").is_some_and(|n| rt.registered_name(ty, "block_entity_type") == Some(n));
        if kind != Kind::BlockEntity || c != cap || !same_type {
            continue;
        }
        let r = rt.call_functional(PROVIDER, provider, &[be, side]);
        rt.report_thrown("getCapability");
        if r.r() != 0 {
            return r;
        }
    }
    V::NULL
}

/// The item capability `cap` of a stack: the providers registered for its item.
fn find_item(rt: &mut Rt, cap: V, stack: V, context: V) -> V {
    if crate::api_common::stack_empty(rt, stack) {
        return V::NULL;
    }
    let item = rt.get_field(stack, "$item");
    let name = crate::api_common::item_like_name(rt, item);
    for (kind, c, key, provider) in entries(rt) {
        if kind != Kind::Item || c != cap || rt.str_of(key) != Some(name.as_str()) {
            continue;
        }
        let r = rt.call_functional(PROVIDER, provider, &[stack, context]);
        rt.report_thrown("getCapability");
        if r.r() != 0 {
            return r;
        }
    }
    V::NULL
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, BLOCK_CAP | ITEM_CAP | ENTITY_CAP | EVENT | CACHE) || matches!(name, "getCapability" | "invalidateCapabilities")
}

pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    call(rt, owner, name, desc, args)
}

/// Capability calls; None for anything else.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // A mod's own capability: `create(name, type, context)`, `createSided`, `createVoid`.
        (BLOCK_CAP | ITEM_CAP | ENTITY_CAP, "create" | "createSided" | "createVoid") => {
            let class: &'static str = match owner {
                BLOCK_CAP => BLOCK_CAP,
                ITEM_CAP => ITEM_CAP,
                _ => ENTITY_CAP,
            };
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            capability(rt, class, &n)
        }
        (BLOCK_CAP | ITEM_CAP | ENTITY_CAP, "name") => rt.get_field(a(0), "$name"),
        (EVENT, "registerBlockEntity") => {
            register(rt, Kind::BlockEntity, a(1), a(2), a(3));
            V::Void
        }
        (EVENT, "registerEntity") => {
            register(rt, Kind::Entity, a(1), a(2), a(3));
            V::Void
        }
        // (capability, provider, blocks...) and (capability, provider, items...): each named one.
        (EVENT, "registerBlock") => {
            for b in crate::jutil::elements(rt, a(3)) {
                let n = api::block_name(rt, b);
                let named = api::block(rt, &n);
                register(rt, Kind::Block, a(1), named, a(2));
            }
            V::Void
        }
        // A block capability other blocks may hand on (NeoForge's proxies): queries here always
        // reach the block's own provider, so there is nothing to mark.
        (EVENT, "setProxyable") => V::Void,
        (EVENT, "registerItem") => {
            for i in crate::jutil::elements(rt, a(3)) {
                let n = item_name(rt, i);
                let s = rt.string(&n);
                register(rt, Kind::Item, a(1), s, a(2));
            }
            V::Void
        }
        (EVENT, "isBlockRegistered" | "isItemRegistered" | "isEntityRegistered") => {
            let kind = match name {
                "isBlockRegistered" => Kind::Block,
                "isItemRegistered" => Kind::Item,
                _ => Kind::Entity,
            };
            let wanted = match kind {
                Kind::Block => api::block_name(rt, a(2)),
                Kind::Item => item_name(rt, a(2)),
                _ => String::new(),
            };
            let found = entries(rt).into_iter().any(|(k, c, key, _)| {
                k == kind
                    && c == a(1)
                    && match kind {
                        Kind::Block => api::block_name(rt, key) == wanted,
                        Kind::Item => rt.str_of(key) == Some(wanted.as_str()),
                        _ => key == a(2),
                    }
            });
            V::I(found as i32)
        }
        // `level.getCapability(cap, pos, side)` and `(cap, pos, state, blockEntity, side)`.
        (o, "getCapability") if (api::is_level(o) || o == LEVEL_EXTENSION) && rt.class_of(a(1)) == BLOCK_CAP => {
            if desc.starts_with("(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;") {
                find_block(rt, a(1), a(0), a(2), a(3), a(4), a(5))
            } else {
                find_block(rt, a(1), a(0), a(2), V::NULL, V::NULL, a(3))
            }
        }
        (ITEM_STACK, "getCapability") if rt.class_of(a(1)) == ITEM_CAP => find_item(rt, a(1), a(0), a(2)),
        // `BlockCapabilityCache.create(cap, level, pos, context[, isValid, onInvalidate])`: asked
        // afresh each time (a lookup is a map read and a provider call).
        (CACHE, "create") => {
            let c = rt.new_object(CACHE);
            for (f, v) in [("$cap", a(0)), ("$level", a(1)), ("$pos", a(2)), ("$context", a(3))] {
                rt.put_field(c, f, v);
            }
            c
        }
        (CACHE, "getCapability") => {
            let (cap, level, pos, side) = (rt.get_field(a(0), "$cap"), rt.get_field(a(0), "$level"), rt.get_field(a(0), "$pos"), rt.get_field(a(0), "$context"));
            find_block(rt, cap, level, pos, V::NULL, V::NULL, side)
        }
        (CACHE, "pos") => rt.get_field(a(0), "$pos"),
        (CACHE, "level") => rt.get_field(a(0), "$level"),
        (CACHE, "context") => rt.get_field(a(0), "$context"),
        (o, "invalidateCapabilities") if api::is_level(o) || o == LEVEL_EXTENSION => V::Void,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, MemoryHost};

    /// Every test lambda is a provider answering with what it captured.
    fn providers(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        let c = rt.captured(args[0]);
        Some(c.first().copied().unwrap_or(V::NULL))
    }

    #[test]
    fn a_block_entity_capability_registered_by_the_mod_is_found_at_its_position() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(providers);
        let cap = api::get_static(&mut rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.BLOCK");
        let fluid = api::get_static(&mut rt, "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.BLOCK");
        assert_ne!(cap, fluid);
        // A depot-like block entity whose provider hands out its slots.
        let ty = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType");
        let other_ty = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType");
        let slots = rt.new_object("net/neoforged/neoforge/items/ItemStackHandler");
        let provider = rt.lambda_id("test/Provider", 1, vec![slots]);
        let ev = rt.new_object(EVENT);
        let register = "net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent.registerBlockEntity.(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/neoforged/neoforge/capabilities/ICapabilityProvider;)V";
        api::call(&mut rt, register, &[ev, cap, ty, provider]);
        let be = rt.new_object("test/DepotBlockEntity");
        rt.put_field(be, "$type", ty);
        rt.block_entities.insert([0, 64, 0], be);
        let stranger = rt.new_object("test/OtherBlockEntity");
        rt.put_field(stranger, "$type", other_ty);
        rt.block_entities.insert([1, 64, 0], stranger);

        let mut host = MemoryHost::default();
        host.blocks.insert([0, 64, 0], "test:depot".into());
        host.blocks.insert([1, 64, 0], "test:other".into());
        let get = "net/minecraft/world/level/Level.getCapability.(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Ljava/lang/Object;";
        let up = api::direction(&mut rt, 1);
        let (at, beside, air) = (api::block_pos(&mut rt, 0, 64, 0), api::block_pos(&mut rt, 1, 64, 0), api::block_pos(&mut rt, 2, 64, 0));
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let found = rt.with_host(&mut host, |rt| {
            [
                api::call(rt, get, &[level, cap, at, up]),
                api::call(rt, get, &[level, fluid, at, up]),
                api::call(rt, get, &[level, cap, beside, up]),
                api::call(rt, get, &[level, cap, air, up]),
            ]
        });
        assert_eq!(found, [slots, V::NULL, V::NULL, V::NULL]);
        let _ = host.block_state([0, 64, 0]);

        // The cache asks the same way, each time.
        let create = "net/neoforged/neoforge/capabilities/BlockCapabilityCache.create.(Lnet/neoforged/neoforge/capabilities/BlockCapability;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Ljava/lang/Object;)Lnet/neoforged/neoforge/capabilities/BlockCapabilityCache;";
        let cache = api::call(&mut rt, create, &[cap, level, at, up]);
        let got = rt.with_host(&mut host, |rt| api::call(rt, "net/neoforged/neoforge/capabilities/BlockCapabilityCache.getCapability.()Ljava/lang/Object;", &[cache]));
        assert_eq!(got, slots);
    }
}
