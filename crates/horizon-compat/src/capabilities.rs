//! NeoForge's capabilities, natively. This covers:
//! - the capability constants (`Capabilities.ItemHandler.BLOCK`, `FluidHandler`, `EnergyStorage`)
//!   and a mod's own (`BlockCapability.create`);
//! - `RegisterCapabilitiesEvent`, fired once the registries are full as NeoForge fires it at the end
//!   of loading, with its block entity, block, item and entity registrations;
//! - the lookups: `Level.getCapability`, `BlockCapabilityCache`, `ItemStack.getCapability` and
//!   `Entity.getCapability`.
//!
//! A block lookup asks the block's own providers first, then its block entity's, in the order
//! NeoForge's `CapabilityHooks` uses. Create's funnels, chutes, arms and basins reach each other's
//! inventories this way. An entity lookup asks NeoForge's own handler of a player first (its
//! inventory), then the providers mods registered for the entity's type, then NeoForge's fallback for
//! any other living entity (its hands and armour).
use crate::{Data, Rt, V, api};

const BLOCK_CAP: &str = "net/neoforged/neoforge/capabilities/BlockCapability";
const ITEM_CAP: &str = "net/neoforged/neoforge/capabilities/ItemCapability";
const ENTITY_CAP: &str = "net/neoforged/neoforge/capabilities/EntityCapability";
const EVENT: &str = "net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent";
const CACHE: &str = "net/neoforged/neoforge/capabilities/BlockCapabilityCache";
const LEVEL_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/ILevelExtension";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
const HORSE: &str = "net/minecraft/world/entity/animal/horse/AbstractHorse";
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
    crate::fluid_wrappers::init(rt);
    let ev = rt.new_object(EVENT);
    crate::neo::post(rt, EVENT, ev);
}

const FLUID_HANDLER: &str = "neoforge:fluid_handler";
const ITEM_HANDLER: &str = "neoforge:item_handler";

/// Whether capability `cap` is the one named `name`.
fn named(rt: &Rt, cap: V, name: &str) -> bool {
    rt.str_of(rt.get_field(cap, "$name")) == Some(name)
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
    // NeoForge's own handler of a cauldron, registered ahead of every mod's provider.
    if named(rt, cap, FLUID_HANDLER)
        && let Some(w) = crate::fluid_wrappers::cauldron(rt, level, pos, &block)
    {
        return w;
    }
    // NeoForge's own handlers of vanilla's containers (a chest, a furnace, a hopper...), registered
    // ahead of every mod's provider.
    if block.starts_with("minecraft:")
        && named(rt, cap, ITEM_HANDLER)
        && let Some(h) = crate::item_wrappers::vanilla_handler(rt, at, side)
    {
        return h;
    }
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

/// `level.getCapability(Capabilities.ItemHandler.BLOCK, pos, side)` at `at`, through `side` (a 3D
/// data value): the item handler NeoForge's hoppers, droppers and crafters find there
/// (`VanillaInventoryCodeHooks.getItemHandlerAt`), or null.
pub(crate) fn block_item_handler(rt: &mut Rt, at: [i32; 3], side: u8) -> V {
    let cap = capability(rt, BLOCK_CAP, ITEM_HANDLER);
    let level = crate::glue::level(rt);
    let pos = api::block_pos(rt, i64::from(at[0]), i64::from(at[1]), i64::from(at[2]));
    let side = api::direction(rt, i64::from(side));
    find_block(rt, cap, level, pos, V::NULL, V::NULL, side)
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
    // NeoForge's fallback, after every mod's provider: a bucket's own wrapper.
    if named(rt, cap, "neoforge:fluid_handler_item") && crate::fluid_wrappers::plain_bucket(rt, stack) {
        return crate::fluid_wrappers::bucket(rt, stack);
    }
    V::NULL
}

/// The entity capability `cap` of `entity` (with `context`), in `CapabilityHooks`' order: NeoForge's
/// own item handler of a player (a `PlayerInvWrapper` of their inventory) ahead of every mod's
/// provider; then the providers mods registered for the entity's type; then NeoForge's fallback,
/// registered after them all for every type: a living entity's hands and armour. A horse's own
/// inventory and a chest boat's or a minecart's container aren't the host's to give: none for those.
fn find_entity(rt: &mut Rt, cap: V, entity: V, context: V) -> V {
    if entity.r() == 0 {
        return V::NULL;
    }
    let items = named(rt, cap, ITEM_HANDLER);
    let player = crate::api_common::is_player_class(rt.class_of(entity));
    if items && player {
        return crate::item_wrappers::player_handler(rt, entity);
    }
    let ty = crate::damage::entity_type_name(rt, entity);
    for (kind, c, key, provider) in entries(rt) {
        if kind != Kind::Entity || c != cap || ty.is_empty() || crate::mod_entities::type_id(rt, key) != Some(ty.as_str()) {
            continue;
        }
        let r = rt.call_functional(PROVIDER, provider, &[entity, context]);
        rt.report_thrown("getCapability");
        if r.r() != 0 {
            return r;
        }
    }
    if items && !player && rt.instance_of(entity, LIVING) && !rt.instance_of(entity, HORSE) {
        return crate::item_wrappers::equipment_handler(rt, entity);
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
            // NeoForge registers the cauldrons' own before any mod's listener runs.
            let cauldron = kind == Kind::Block && named(rt, a(1), FLUID_HANDLER) && crate::fluid_wrappers::is_cauldron(rt, &wanted);
            let found = cauldron || entries(rt).into_iter().any(|(k, c, key, _)| {
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
        // `entity.getCapability(cap)` and `(cap, context)`, through any entity class.
        (_, "getCapability") if rt.class_of(a(1)) == ENTITY_CAP => {
            let context = if desc.starts_with("(Lnet/neoforged/neoforge/capabilities/EntityCapability;Ljava/lang/Object;)") { a(2) } else { V::NULL };
            find_entity(rt, a(1), a(0), context)
        }
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

    /// An entity's capability: the providers registered for its type, in order, the first that
    /// answers; then NeoForge's fallback item handler of a living entity, its hands and armour (6
    /// slots, an armour slot holding one), which reads what it holds. An arrow has none.
    #[test]
    fn an_entitys_capability_is_its_types_providers_then_neoforges_fallback() {
        use crate::host::Nbt;
        use std::collections::BTreeMap;
        const GET: &str = "net/minecraft/world/entity/Entity.getCapability.(Lnet/neoforged/neoforge/capabilities/EntityCapability;)Ljava/lang/Object;";
        const REGISTER: &str = "net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent.registerEntity.(Lnet/neoforged/neoforge/capabilities/EntityCapability;Lnet/minecraft/world/entity/EntityType;Lnet/neoforged/neoforge/capabilities/ICapabilityProvider;)V";
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(providers);
        let items = api::get_static(&mut rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.ENTITY");
        let energy = api::get_static(&mut rt, "net/neoforged/neoforge/capabilities/Capabilities$EnergyStorage.ENTITY");
        let ev = rt.new_object(EVENT);
        // A zombie's energy: the first provider answers null, the second a battery.
        let zombie_type = crate::mod_entities::vanilla_type(&mut rt, "minecraft:zombie");
        let nothing = rt.lambda_id("test/Provider", 1, vec![]);
        let battery = rt.new_object("test/Battery");
        let gives_battery = rt.lambda_id("test/Provider", 1, vec![battery]);
        api::call(&mut rt, REGISTER, &[ev, energy, zombie_type, nothing]);
        api::call(&mut rt, REGISTER, &[ev, energy, zombie_type, gives_battery]);
        // A cow's items: the mod's own handler, ahead of NeoForge's fallback.
        let cow_type = crate::mod_entities::vanilla_type(&mut rt, "minecraft:cow");
        let pouch = rt.new_object("test/Pouch");
        let gives_pouch = rt.lambda_id("test/Provider", 1, vec![pouch]);
        api::call(&mut rt, REGISTER, &[ev, items, cow_type, gives_pouch]);

        let mut host = MemoryHost::default();
        for (id, kind, health) in [(7, "minecraft:zombie", Some((20.0, 20.0))), (8, "minecraft:cow", Some((10.0, 10.0))), (9, "minecraft:arrow", None)] {
            host.entities.push(crate::host::EntityView { id, kind: kind.into(), pos: [0.5, 64.0, 0.5], yaw: 0.0, health });
        }
        let sword = BTreeMap::from([("id".to_owned(), Nbt::String("minecraft:iron_sword".into())), ("count".to_owned(), Nbt::Int(1))]);
        host.entity_nbts.insert(7, BTreeMap::from([("equipment".to_owned(), Nbt::Compound(BTreeMap::from([("mainhand".to_owned(), Nbt::Compound(sword))])))]));
        rt.with_host(&mut host, |rt| {
            let [zombie, cow, arrow] = [(7, "minecraft:zombie"), (8, "minecraft:cow"), (9, "minecraft:arrow")].map(|(id, kind)| crate::vanilla_entities::of(rt, id, kind, None));
            assert_eq!(api::call(rt, GET, &[zombie, energy]), battery);
            assert_eq!(api::call(rt, GET, &[cow, energy]), V::NULL);
            assert_eq!(api::call(rt, GET, &[cow, items]), pouch);
            let h = api::call(rt, GET, &[zombie, items]);
            assert_eq!(api::call(rt, "net/neoforged/neoforge/items/IItemHandler.getSlots.()I", &[h]), V::I(6));
            let held = api::call(rt, "net/neoforged/neoforge/items/IItemHandler.getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;", &[h, V::I(0)]);
            assert_eq!(crate::api_common::item_like_name(rt, rt.get_field(held, "$item")), "minecraft:iron_sword");
            let feet = api::call(rt, "net/neoforged/neoforge/items/IItemHandler.getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;", &[h, V::I(2)]);
            assert!(crate::api_common::stack_empty(rt, feet));
            assert_eq!(api::call(rt, "net/neoforged/neoforge/items/IItemHandler.getSlotLimit.(I)I", &[h, V::I(2)]), V::I(1));
            assert_eq!(api::call(rt, "net/neoforged/neoforge/items/IItemHandler.getSlotLimit.(I)I", &[h, V::I(1)]), V::I(99));
            assert_eq!(api::call(rt, GET, &[arrow, items]), V::NULL);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
