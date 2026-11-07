//! NeoForge 1.21.1's data maps: values data files attach to a registry's entries, as
//! `DataMapLoader` builds them, and what NeoForge asks of them:
//!
//! - An item's burn time as fuel (`ItemStack.getBurnTime(recipeType)`, 22 of the pack's mods): 0
//!   for an empty stack; else the item's `getBurnTime(stack, recipeType)` (a mod's override, called
//!   virtually), by default its value in `neoforge:furnace_fuels`; a negative one is an
//!   `IllegalStateException`. Then `FurnaceFuelBurnTimeEvent` is posted, and a listener may set
//!   another (`setBurnTime`, which cancels it). `AbstractFurnaceBlockEntity.isFuel(stack)` is a
//!   burn time above 0; `getFuel()` is vanilla's own table, which NeoForge keeps for it.
//! - A value by holder, by registry and key, or a registry's whole map (`Holder.getData(type)`,
//!   `Registry.getData(type, key)`, `getDataMap(type)`): null for a type of another registry.
//! - The types mods add (`RegisterDataMapTypesEvent.register`), posted once the registries are
//!   full, as NeoForge's `RegistrationEvents` posts it; a type registered twice is an
//!   `IllegalArgumentException`.
//!
//! A type's file is `data/<type namespace>/data_maps/<registry folder>/<type path>.json` (the
//! folder is the registry's path, after its namespace when that isn't `minecraft`), read once, the
//! first time a value is asked with the game's resources at hand. Each file in turn: `replace`
//! drops what the ones before gave; each of its `values` (an id, or a `#tag` of the registry: its
//! entries) sets its entries' value, the later over the earlier (NeoForge's default merger: an
//! `AdvancedDataMapType`'s own isn't called), and is skipped where its `neoforge:conditions` don't
//! hold (the value is then its `neoforge:value`, or the object without the conditions); then its
//! `remove` list drops entries (a removal with a remover of its own is left alone). A value is
//! plain or `{"value": ..., "replace": ...}`, decoded with the type's codec (NeoForge's own types'
//! natively); a file with a value that doesn't decode is left out whole, as NeoForge's loading
//! leaves it.
//!
//! NeoForge's own furnace fuels (its jar's file: vanilla's fuels) come first, as the pack doesn't
//! carry NeoForge's data; its own values of its other types (vanilla's compostables, waxables...)
//! aren't carried. The game gives the first pack's copy of a file, not every pack's: of two mods
//! that each add fuels in a copy of the file, one is read.
use crate::{Data, Rt, V, api, api_common, jutil};
use serde_json::Value as Json;

const STACK: &str = "net/minecraft/world/item/ItemStack";
const STACK_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IItemStackExtension";
const ITEM_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IItemExtension";
const FUEL_EVENT: &str = "net/neoforged/neoforge/event/furnace/FurnaceFuelBurnTimeEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
const EVENT_HOOKS: &str = "net/neoforged/neoforge/event/EventHooks";
const REGISTER_TYPES: &str = "net/neoforged/neoforge/registries/datamaps/RegisterDataMapTypesEvent";
const REGISTRY_MANAGER: &str = "net/neoforged/neoforge/registries/RegistryManager";
const BUILT_IN: &str = "net/neoforged/neoforge/registries/datamaps/builtin/";
const FURNACE_FUEL: &str = "net/neoforged/neoforge/registries/datamaps/builtin/FurnaceFuel";
const COMPOSTABLE: &str = "net/neoforged/neoforge/registries/datamaps/builtin/Compostable";
const VIBRATION_FREQUENCY: &str =
    "net/neoforged/neoforge/registries/datamaps/builtin/VibrationFrequency";
const WAXABLE: &str = "net/neoforged/neoforge/registries/datamaps/builtin/Waxable";
const OXIDIZABLE: &str = "net/neoforged/neoforge/registries/datamaps/builtin/Oxidizable";
const PARROT_IMITATION: &str = "net/neoforged/neoforge/registries/datamaps/builtin/ParrotImitation";
const RAID_HERO_GIFT: &str = "net/neoforged/neoforge/registries/datamaps/builtin/RaidHeroGift";
/// A data map type as the first parameter: `getData(type[, key])`, `getDataMap(type)`.
const TYPE_PARAM: &str = "(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;";
const BURN_TIME_DESC: &str =
    "(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/crafting/RecipeType;)I";
/// `Item.getBurnTime(stack, recipeType)`, called virtually: (the API's key, the method's).
const ITEM_BURN_TIME: (&str, &str) = (
    "net/minecraft/world/item/Item.getBurnTime.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/crafting/RecipeType;)I",
    "getBurnTime.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/crafting/RecipeType;)I",
);
const FURNACES: [&str; 4] = [
    "net/minecraft/world/level/block/entity/AbstractFurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/FurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/BlastFurnaceBlockEntity",
    "net/minecraft/world/level/block/entity/SmokerBlockEntity",
];
const FUELS: &str = "neoforge:furnace_fuels";
const FUELS_FIELD: &str =
    "net/neoforged/neoforge/registries/datamaps/builtin/NeoForgeDataMaps.FURNACE_FUELS";
/// The items vanilla never burns, whatever a table says (`isNeverAFurnaceFuel`).
const NEVER_FUEL: &str = "minecraft:non_flammable_wood";
/// Each type's files, as read ([`layers`]), by type id.
const LAYERS: &str = "compat:data-map-files/";
/// An entry's value once found ([`value`]): `<type id>/<entry id>`.
const VALUES: &str = "compat:data-map-value/";
/// The types mods registered (`RegisterDataMapTypesEvent.register`), a list.
const TYPES: &str = "compat:data-map-types";
const LAYER: &str = "compat:data-map-file";
const CONDITIONS: &str = "neoforge:conditions";
const CONDITIONAL_VALUE: &str = "neoforge:value";

/// NeoForge 21.1's own `data/neoforge/data_maps/item/furnace_fuels.json`: vanilla's fuels, in burn
/// ticks; then the items of `#minecraft:non_flammable_wood` are removed.
const NEOFORGE_FUELS: [(&str, i32); 59] = [
    ("#minecraft:bamboo_blocks", 300),
    ("#minecraft:banners", 300),
    ("#minecraft:boats", 1200),
    ("#minecraft:fence_gates", 300),
    ("#minecraft:hanging_signs", 800),
    ("#minecraft:logs", 300),
    ("#minecraft:planks", 300),
    ("#minecraft:saplings", 100),
    ("#minecraft:signs", 200),
    ("#minecraft:wooden_buttons", 100),
    ("#minecraft:wooden_doors", 200),
    ("#minecraft:wooden_fences", 300),
    ("#minecraft:wooden_pressure_plates", 300),
    ("#minecraft:wooden_slabs", 150),
    ("#minecraft:wooden_stairs", 300),
    ("#minecraft:wooden_trapdoors", 300),
    ("#minecraft:wool", 100),
    ("#minecraft:wool_carpets", 67),
    ("minecraft:azalea", 100),
    ("minecraft:bamboo", 50),
    ("minecraft:bamboo_mosaic", 300),
    ("minecraft:bamboo_mosaic_slab", 150),
    ("minecraft:bamboo_mosaic_stairs", 300),
    ("minecraft:barrel", 300),
    ("minecraft:blaze_rod", 2400),
    ("minecraft:bookshelf", 300),
    ("minecraft:bow", 300),
    ("minecraft:bowl", 100),
    ("minecraft:cartography_table", 300),
    ("minecraft:charcoal", 1600),
    ("minecraft:chest", 300),
    ("minecraft:chiseled_bookshelf", 300),
    ("minecraft:coal", 1600),
    ("minecraft:coal_block", 16000),
    ("minecraft:composter", 300),
    ("minecraft:crafting_table", 300),
    ("minecraft:crossbow", 300),
    ("minecraft:daylight_detector", 300),
    ("minecraft:dead_bush", 100),
    ("minecraft:dried_kelp_block", 4001),
    ("minecraft:fishing_rod", 300),
    ("minecraft:fletching_table", 300),
    ("minecraft:flowering_azalea", 100),
    ("minecraft:jukebox", 300),
    ("minecraft:ladder", 300),
    ("minecraft:lava_bucket", 20000),
    ("minecraft:lectern", 300),
    ("minecraft:loom", 300),
    ("minecraft:mangrove_roots", 300),
    ("minecraft:note_block", 300),
    ("minecraft:scaffolding", 50),
    ("minecraft:smithing_table", 300),
    ("minecraft:stick", 100),
    ("minecraft:trapped_chest", 300),
    ("minecraft:wooden_axe", 200),
    ("minecraft:wooden_hoe", 200),
    ("minecraft:wooden_pickaxe", 200),
    ("minecraft:wooden_shovel", 200),
    ("minecraft:wooden_sword", 200),
];

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match name {
        _ if owner == FUEL_EVENT => true,
        "getBurnTime" => {
            matches!(owner, STACK | STACK_EXTENSION | ITEM_EXTENSION)
                || (owner.starts_with("net/minecraft/world/item/") && desc == BURN_TIME_DESC)
        }
        "isFuel" | "getFuel" => FURNACES.contains(&owner),
        "getData" => desc.starts_with(TYPE_PARAM),
        "getDataMap" => desc.starts_with(TYPE_PARAM) || owner == REGISTRY_MANAGER,
        "getItemBurnTime" => owner == EVENT_HOOKS,
        "register" => owner == REGISTER_TYPES,
        "<init>" => owner == COMPOSTABLE && desc == "(F)V",
        _ => false,
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == FUEL_EVENT {
        return fuel_event(rt, name, args);
    }
    Some(match name {
        "getBurnTime" if matches!(owner, STACK | STACK_EXTENSION) => V::I(burn_time(rt, a(0), a(1))),
        // `Item.getBurnTime(stack, recipeType)`, the item's own unless it overrides it.
        "getBurnTime" => V::I(item_burn_time(rt, a(0))),
        "getItemBurnTime" => V::I(post_burn_time(rt, a(0), a(1).i(), a(2))),
        "isFuel" => V::I(i32::from(burn_time(rt, a(0), V::NULL) > 0)),
        "getFuel" => vanilla_fuels(rt),
        // `Registry.getData(type, key)`.
        "getData" if desc.starts_with("(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;Lnet/minecraft/resources/ResourceKey;)") => {
            let (registry, id) = crate::neo::split_key(rt, a(2));
            let of = rt.str_of(a(0)).unwrap_or("").to_owned();
            if same_registry(&registry, &of) || registry.is_empty() { registry_value(rt, a(1), &of, &id) } else { V::NULL }
        }
        // `Holder.getData(type)`: a registry's holder's (a `DeferredHolder`'s too); `Holder.direct`'s
        // has none.
        "getData" => holder_data(rt, a(0), a(1)),
        "getDataMap" if owner == REGISTRY_MANAGER => registered_type(rt, a(0), a(1)),
        "getDataMap" => {
            let of = rt.str_of(a(0)).unwrap_or("").to_owned();
            whole_map(rt, a(1), &of)
        }
        "register" => register(rt, a(1)),
        // `Compostable(chance)`: one villagers don't compost.
        "<init>" => {
            rt.put_field(a(0), "chance", a(1));
            rt.put_field(a(0), "canVillagerCompost", V::I(0));
            V::Void
        }
        _ => return None,
    })
}

/// `ItemStack.getBurnTime(recipeType)`: see the module's doc.
fn burn_time(rt: &mut Rt, stack: V, recipe_type: V) -> i32 {
    if api_common::stack_empty(rt, stack) {
        return 0;
    }
    let item = rt.get_field(stack, "$item");
    let ticks = api::virtual_call(rt, ITEM_BURN_TIME, &[item, stack, recipe_type]).i();
    if rt.thrown.is_some() {
        return 0;
    }
    if ticks < 0 {
        let id = api_common::item_like_name(rt, item);
        crate::jlang::throw_with(
            rt,
            "java/lang/IllegalStateException",
            &format!("Stack of item {id} has a negative burn time"),
        );
        return 0;
    }
    post_burn_time(rt, stack, ticks, recipe_type)
}

/// The burn time of `item` in `neoforge:furnace_fuels`, 0 for none.
fn item_burn_time(rt: &mut Rt, item: V) -> i32 {
    let id = api_common::item_like_name(rt, item);
    let fuels = api::get_static(rt, FUELS_FIELD);
    let fuel = value(rt, fuels, &id);
    if fuel.r() == 0 {
        0
    } else {
        rt.get_field(fuel, "burnTime").i()
    }
}

/// `EventHooks.getItemBurnTime`: `FurnaceFuelBurnTimeEvent` posted with `ticks`, and what its
/// listeners leave.
fn post_burn_time(rt: &mut Rt, stack: V, ticks: i32, recipe_type: V) -> i32 {
    rt.supers.entry(FUEL_EVENT).or_insert(EVENT);
    let ev = rt.new_object(FUEL_EVENT);
    rt.put_field(ev, "$stack", stack);
    rt.put_field(ev, "$burnTime", V::I(ticks));
    rt.put_field(ev, "$recipeType", recipe_type);
    crate::neo::post(rt, FUEL_EVENT, ev);
    rt.get_field(ev, "$burnTime").i()
}

/// `FurnaceFuelBurnTimeEvent`'s methods on event `a(0)`.
fn fuel_event(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    let reference = |v: V| if v == V::Void { V::NULL } else { v };
    Some(match name {
        "<init>" => {
            rt.put_field(ev, "$stack", a(1));
            rt.put_field(ev, "$burnTime", V::I(a(2).i()));
            rt.put_field(ev, "$recipeType", a(3));
            V::Void
        }
        "getItemStack" => reference(rt.get_field(ev, "$stack")),
        "getRecipeType" => reference(rt.get_field(ev, "$recipeType")),
        "getBurnTime" => V::I(rt.get_field(ev, "$burnTime").i()),
        // A time of 0 or more is taken, and ends the event for the listeners after.
        "setBurnTime" => {
            if a(1).i() >= 0 {
                rt.put_field(ev, "$burnTime", V::I(a(1).i()));
                rt.put_field(ev, "$canceled", V::I(1));
            }
            V::Void
        }
        "isCanceled" => V::I(rt.get_field(ev, "$canceled").i()),
        "setCanceled" => {
            rt.put_field(ev, "$canceled", V::I(a(1).i()));
            V::Void
        }
        _ => return None,
    })
}

/// `AbstractFurnaceBlockEntity.getFuel()`: vanilla's fuels (NeoForge's own table, read apart from
/// the data map), each item its burn time, but those vanilla never burns.
fn vanilla_fuels(rt: &mut Rt) -> V {
    if let Some(&m) = rt.interned.get("compat:vanilla-fuels") {
        return m;
    }
    let map = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
    for (entry, ticks) in NEOFORGE_FUELS {
        let items = match entry.strip_prefix('#') {
            Some(tag) => crate::recipes::tag_items(rt, &format!("#{tag}")),
            None => vec![entry.to_owned()],
        };
        for id in items {
            if crate::recipes::item_in_tag(rt, &id, NEVER_FUEL) {
                continue;
            }
            let (item, time) = (api_common::item(rt, &id), api_common::integer(rt, ticks));
            jutil::table_put(rt, map, item, time);
        }
    }
    if rt.host().is_some() {
        rt.interned.insert("compat:vanilla-fuels".into(), map);
    }
    map
}

/// A registry's name without vanilla's namespace (`item`, `create:potato_projectile/type`).
fn bare(registry: &str) -> &str {
    registry.strip_prefix("minecraft:").unwrap_or(registry)
}

fn same_registry(a: &str, b: &str) -> bool {
    bare(a) == bare(b)
}

/// A type's id (`neoforge:furnace_fuels`) and its registry's name (`item`).
fn type_of(rt: &Rt, ty: V) -> Option<(String, String)> {
    if ty.r() == 0 {
        return None;
    }
    let id = rt.get_field(ty, "$id");
    let registry = rt.str_of(rt.get_field(ty, "$registry"))?.to_owned();
    rt.str_of(id)
        .map(|_| (crate::neo::location(rt, id), bare(&registry).to_owned()))
}

/// The folder of `registry`'s data maps below `data_maps/` (`DataMapLoader.getFolderLocation`):
/// the registry's path, after its namespace when that isn't `minecraft`.
fn folder(rt: &Rt, registry: &str) -> String {
    match registry.split_once(':') {
        Some((ns, path)) => format!("{ns}/{path}"),
        // Registries are named by their field here (`Registries.BIOME` is `biome`).
        None if registry == "biome" => "worldgen/biome".to_owned(),
        None => crate::datapack::folder(rt, registry),
    }
}

/// `holder.getData(type)`: the value of the holder's entry when it is of the type's registry.
fn holder_data(rt: &mut Rt, holder: V, ty: V) -> V {
    if holder.r() == 0 {
        return V::NULL;
    }
    let Some(registry) = rt
        .str_of(rt.get_field(holder, "$registry"))
        .map(str::to_owned)
    else {
        return V::NULL;
    };
    let mut id = rt
        .str_of(rt.get_field(holder, "$id"))
        .unwrap_or("")
        .to_owned();
    // An intrusive holder is named once its value is registered.
    if id.is_empty() {
        let value = rt.get_field(holder, "$value");
        id = rt
            .registered_name(value, &registry)
            .unwrap_or("")
            .to_owned();
    }
    if id.is_empty() {
        return V::NULL;
    }
    registry_value(rt, ty, &registry, &id)
}

/// The value type `ty` gives entry `id` of registry `registry`: null for a type of another.
fn registry_value(rt: &mut Rt, ty: V, registry: &str, id: &str) -> V {
    match type_of(rt, ty) {
        Some((_, of)) if same_registry(&of, registry) => value(rt, ty, id),
        _ => V::NULL,
    }
}

/// The value data map type `ty` gives entry `id` of its registry, or null: see the module's doc.
pub(crate) fn value(rt: &mut Rt, ty: V, id: &str) -> V {
    let Some((type_id, registry)) = type_of(rt, ty) else {
        return V::NULL;
    };
    let cache = format!("{VALUES}{type_id}/{id}");
    if let Some(&v) = rt.interned.get(&cache) {
        return v;
    }
    let layers = layers(rt, ty, &type_id, &registry);
    let mut found = V::NULL;
    for layer in jutil::elements(rt, layers) {
        if rt.get_field(layer, "$replace").i() != 0 {
            found = V::NULL;
        }
        let (keys, entries) = (
            rt.get_field(layer, "$keys"),
            rt.get_field(layer, "$entries"),
        );
        let (keys, entries) = (jutil::elements(rt, keys), jutil::elements(rt, entries));
        for (k, entry) in keys.into_iter().zip(entries) {
            let key = rt.str_of(k).unwrap_or("").to_owned();
            // An entry whose conditions don't hold is none.
            if entry.r() != 0 && names(rt, &registry, &key, id) {
                found = entry;
            }
        }
        let removed = rt.get_field(layer, "$remove");
        for k in jutil::elements(rt, removed) {
            let key = rt.str_of(k).unwrap_or("").to_owned();
            if names(rt, &registry, &key, id) {
                found = V::NULL;
            }
        }
    }
    // Tags are the game's: without it, asked again.
    if rt.host().is_some() {
        rt.interned.insert(cache, found);
    }
    found
}

/// Whether a file's key (`minecraft:coal`, `#minecraft:logs`) names entry `id` of `registry`: the
/// entry itself, or a tag of the registry holding it, as the game's tags hold it.
fn names(rt: &mut Rt, registry: &str, key: &str, id: &str) -> bool {
    let Some(tag) = key.strip_prefix('#') else {
        return key == id;
    };
    match registry {
        "item" => crate::recipes::item_in_tag(rt, id, tag),
        "block" => rt.host().is_some_and(|h| h.block_in_tag(id, tag)),
        "fluid" => rt.host().is_some_and(|h| h.fluid_in_tag(id, tag)),
        "entity_type" => rt.host().is_some_and(|h| h.entity_type_in_tag(id, tag)),
        "biome" => rt.host().is_some_and(|h| h.biome_in_tag(id, tag)),
        _ => false,
    }
}

/// `registry.getDataMap(type)`: each entry the type gives a value, by `ResourceKey`. The entries
/// are those its files name, and the items of the item tags they name (the game lists no other
/// registry's tags).
fn whole_map(rt: &mut Rt, ty: V, registry: &str) -> V {
    let map = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
    let Some((type_id, of)) = type_of(rt, ty) else {
        return map;
    };
    if !same_registry(&of, registry) {
        return map;
    }
    let mut ids: Vec<String> = Vec::new();
    let layers = layers(rt, ty, &type_id, &of);
    for layer in jutil::elements(rt, layers) {
        let keys = rt.get_field(layer, "$keys");
        for k in jutil::elements(rt, keys) {
            let key = rt.str_of(k).unwrap_or("").to_owned();
            let more = match key.strip_prefix('#') {
                Some(tag) if of == "item" => crate::recipes::tag_items(rt, &format!("#{tag}")),
                Some(_) => Vec::new(),
                None => vec![key],
            };
            for id in more {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    for id in ids {
        let v = value(rt, ty, &id);
        if v.r() != 0 {
            let k = rt.string(&format!("{of}#{id}"));
            jutil::table_put(rt, map, k, v);
        }
    }
    map
}

/// Type `ty`'s files, in order, as layer objects: NeoForge's own fuels first for its fuels, then
/// the game's file. Read once with the game's resources at hand.
fn layers(rt: &mut Rt, ty: V, type_id: &str, registry: &str) -> V {
    let key = format!("{LAYERS}{type_id}");
    if let Some(&l) = rt.interned.get(&key) {
        return l;
    }
    let mut all = Vec::new();
    if type_id == FUELS {
        all.push(neoforge_fuels(rt));
    }
    let (ns, path) = type_id.split_once(':').unwrap_or(("minecraft", type_id));
    let file = format!("data/{ns}/data_maps/{}/{path}.json", folder(rt, registry));
    if let Some(bytes) = crate::resources::file(rt, &file) {
        match read(rt, ty, type_id, &bytes) {
            Ok(layer) => all.push(layer),
            Err(e) => {
                if Rt::tracing() {
                    eprintln!("horizon-compat: data map {type_id}: {file} left out: {e}");
                }
            }
        }
    }
    let list = jutil::new_list(rt, all);
    if rt.host().is_some() {
        rt.interned.insert(key, list);
    }
    list
}

fn layer(
    rt: &mut Rt,
    replace: bool,
    keys: Vec<String>,
    entries: Vec<V>,
    removed: Vec<String>,
) -> V {
    let l = rt.new_object(LAYER);
    rt.put_field(l, "$replace", V::I(i32::from(replace)));
    let keys: Vec<V> = keys.iter().map(|k| rt.string(k)).collect();
    let keys = jutil::new_list(rt, keys);
    rt.put_field(l, "$keys", keys);
    let entries = jutil::new_list(rt, entries);
    rt.put_field(l, "$entries", entries);
    let removed: Vec<V> = removed.iter().map(|k| rt.string(k)).collect();
    let removed = jutil::new_list(rt, removed);
    rt.put_field(l, "$remove", removed);
    l
}

/// NeoForge's own furnace fuels file.
fn neoforge_fuels(rt: &mut Rt) -> V {
    let keys = NEOFORGE_FUELS
        .iter()
        .map(|(k, _)| (*k).to_owned())
        .collect();
    let entries = NEOFORGE_FUELS
        .iter()
        .map(|&(_, t)| crate::records::make(rt, FURNACE_FUEL, &[V::I(t)]))
        .collect();
    layer(rt, false, keys, entries, vec![format!("#{NEVER_FUEL}")])
}

/// A file's key: `#namespace:path` for a tag, `namespace:path` for an entry (`minecraft` when it
/// names none).
fn tag_or_id(s: &str) -> Result<String, String> {
    match s.strip_prefix('#') {
        Some(tag) => crate::neo::checked_location(tag, ':').map(|t| format!("#{t}")),
        None => crate::neo::checked_location(s, ':'),
    }
}

/// One data map file (`DataMapFile`'s codec): see the module's doc.
fn read(rt: &mut Rt, ty: V, type_id: &str, bytes: &[u8]) -> Result<V, String> {
    let json: Json = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let o = json.as_object().ok_or("not an object")?;
    let replace = match o.get("replace") {
        None => false,
        Some(b) => b.as_bool().ok_or("replace isn't a boolean")?,
    };
    let values = o
        .get("values")
        .and_then(Json::as_object)
        .ok_or("no values")?;
    let namespaces = rt
        .host()
        .map(|h| h.resource_namespaces("data"))
        .unwrap_or_default();
    let (mut keys, mut entries) = (Vec::new(), Vec::new());
    for (k, v) in values {
        keys.push(tag_or_id(k)?);
        entries.push(match conditioned(rt, v, &namespaces)? {
            Some(v) => entry(rt, ty, type_id, &v)?,
            None => V::NULL,
        });
    }
    let mut removed = Vec::new();
    match o.get("remove") {
        None => {}
        Some(Json::Array(list)) => {
            for r in list {
                match r {
                    Json::String(s) => removed.push(tag_or_id(s)?),
                    // An advanced type's `{"key": ..., "remover": ...}`: without a remover, the
                    // entry goes.
                    Json::Object(m) => {
                        let key = m
                            .get("key")
                            .and_then(Json::as_str)
                            .ok_or("a removal names no key")?;
                        let key = tag_or_id(key)?;
                        if !m.contains_key("remover") {
                            removed.push(key);
                        }
                    }
                    _ => return Err("a removal isn't a key".into()),
                }
            }
        }
        // An advanced type's map of keys to removers.
        Some(Json::Object(_)) => {}
        Some(_) => return Err("remove isn't a list".into()),
    }
    Ok(layer(rt, replace, keys, entries, removed))
}

/// A value under `neoforge:conditions` (`ConditionalOps`): None where they don't hold, else what
/// to decode (its `neoforge:value`, or the object without the conditions).
fn conditioned(rt: &Rt, v: &Json, namespaces: &[String]) -> Result<Option<Json>, String> {
    let Some(o) = v.as_object() else {
        return Ok(Some(v.clone()));
    };
    let Some(c) = o.get(CONDITIONS) else {
        return Ok(Some(v.clone()));
    };
    let all = c.as_array().ok_or("conditions aren't a list")?;
    if !all
        .iter()
        .all(|c| crate::datapack::holds(rt, c, namespaces))
    {
        return Ok(None);
    }
    Ok(Some(match o.get(CONDITIONAL_VALUE) {
        Some(inner) => inner.clone(),
        None => {
            let mut rest = o.clone();
            rest.remove(CONDITIONS);
            Json::Object(rest)
        }
    }))
}

/// A value of the file (`DataMapEntry`'s codec): `{"value": ..., "replace": ...}` where that
/// decodes, else the value itself.
fn entry(rt: &mut Rt, ty: V, type_id: &str, v: &Json) -> Result<V, String> {
    if let Some(inner) = v.as_object().and_then(|o| o.get("value"))
        && v.get("replace").is_none_or(Json::is_boolean)
        && let Ok(value) = decode(rt, ty, type_id, inner)
    {
        return Ok(value);
    }
    decode(rt, ty, type_id, v)
}

/// An int as DFU's `Codec.INT` reads one from JSON (`Number.intValue`).
fn int(j: &Json) -> Option<i32> {
    j.as_i64()
        .map(|n| n as i32)
        .or_else(|| j.as_f64().map(|f| f as i32))
}

/// A value of data map type `ty`: NeoForge's own types natively (each `withAlternative` of an
/// object of one field and the field's value alone), a mod's type with its codec.
fn decode(rt: &mut Rt, ty: V, type_id: &str, j: &Json) -> Result<V, String> {
    let field = |name: &str| j.as_object().and_then(|o| o.get(name)).unwrap_or(j);
    let entry_of = |rt: &mut Rt, registry: &str, id: Option<&str>| -> Result<V, String> {
        let id = crate::neo::checked_location(id.ok_or("not an id")?, ':')?;
        let v = crate::neo::registry_get(rt, registry, &id);
        if v.r() == 0 {
            Err(format!("Unknown registry key in {registry}: {id}"))
        } else {
            Ok(v)
        }
    };
    let record = |rt: &mut Rt, class: &str, v: V| crate::records::make(rt, class, &[v]);
    Ok(match type_id {
        FUELS => {
            let n = int(field("burn_time"))
                .filter(|n| *n > 0)
                .ok_or("a burn time isn't a positive int")?;
            record(rt, FURNACE_FUEL, V::I(n))
        }
        "neoforge:compostables" => {
            let (chance, villagers) = match j.as_object() {
                Some(o) if o.contains_key("chance") => (
                    o["chance"].as_f64(),
                    o.get("can_villager_compost")
                        .map_or(Some(false), Json::as_bool),
                ),
                _ => (j.as_f64(), Some(false)),
            };
            let chance = chance
                .filter(|c| (0.0..=1.0).contains(c))
                .ok_or("a chance isn't within 0 and 1")?;
            let villagers = villagers.ok_or("can_villager_compost isn't a boolean")?;
            crate::records::make(
                rt,
                COMPOSTABLE,
                &[V::F(chance as f32), V::I(i32::from(villagers))],
            )
        }
        "neoforge:vibration_frequencies" => {
            let n = int(field("frequency"))
                .filter(|n| (1..=15).contains(n))
                .ok_or("a frequency isn't within 1 and 15")?;
            record(rt, VIBRATION_FREQUENCY, V::I(n))
        }
        "neoforge:waxables" => {
            let block = entry_of(rt, "block", field("waxed").as_str())?;
            record(rt, WAXABLE, block)
        }
        "neoforge:oxidizables" => {
            let block = entry_of(rt, "block", field("next_oxidation_stage").as_str())?;
            record(rt, OXIDIZABLE, block)
        }
        "neoforge:parrot_imitations" => {
            let id =
                crate::neo::checked_location(field("sound").as_str().ok_or("not an id")?, ':')?;
            let sound = crate::sounds::vanilla_event(rt, &id)
                .unwrap_or_else(|| crate::neo::registry_get(rt, "sound_event", &id));
            if sound.r() == 0 {
                return Err(format!("Unknown registry key in sound_event: {id}"));
            }
            record(rt, PARROT_IMITATION, sound)
        }
        "neoforge:raid_hero_gifts" => {
            let id = crate::neo::checked_location(
                field("loot_table").as_str().ok_or("not an id")?,
                ':',
            )?;
            let key = rt.string(&format!("loot_table#{id}"));
            record(rt, RAID_HERO_GIFT, key)
        }
        _ => {
            let codec = rt.get_field(ty, "$codec");
            if !crate::codecs::is_codec(rt, codec) {
                return Err(format!("{type_id} has no codec to read with"));
            }
            crate::codecs::decode(rt, codec, j)?
        }
    })
}

/// Posts `RegisterDataMapTypesEvent` to the mods (`RegistryManager.initDataMaps`), once.
pub(crate) fn fire(rt: &mut Rt) {
    if rt.interned.contains_key(TYPES) {
        return;
    }
    let types = jutil::new_list(rt, Vec::new());
    rt.interned.insert(TYPES.into(), types);
    rt.supers.entry(REGISTER_TYPES).or_insert(EVENT);
    let ev = rt.new_object(REGISTER_TYPES);
    crate::neo::post(rt, REGISTER_TYPES, ev);
}

/// The types mods registered.
fn registered(rt: &mut Rt) -> Vec<V> {
    let types = rt.intern(TYPES.into(), |rt| jutil::new_list(rt, Vec::new()));
    jutil::elements(rt, types)
}

/// `RegisterDataMapTypesEvent.register(type)`: a type of an id its registry has already is an
/// `IllegalArgumentException`, NeoForge's own types among them.
fn register(rt: &mut Rt, ty: V) -> V {
    let Some((id, registry)) = type_of(rt, ty) else {
        return V::Void;
    };
    let built_in = id.strip_prefix("neoforge:").is_some_and(|f| {
        crate::neo::neoforge_data_map(&f.to_ascii_uppercase()) == Some(registry.as_str())
    });
    let again = built_in
        || registered(rt)
            .into_iter()
            .any(|t| type_of(rt, t).is_some_and(|(i, r)| i == id && same_registry(&r, &registry)));
    if again {
        let message = format!(
            "Tried to register data map type with ID {id} to registry minecraft:{registry} twice"
        );
        return crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &message);
    }
    let types = rt.intern(TYPES.into(), |rt| jutil::new_list(rt, Vec::new()));
    jutil::list_add(rt, types, ty);
    V::Void
}

/// `RegistryManager.getDataMap(registryKey, id)`: the type registered as `id` to the registry,
/// NeoForge's own or a mod's; null for none.
fn registered_type(rt: &mut Rt, registry: V, id: V) -> V {
    let registry = bare(rt.str_of(registry).unwrap_or("")).to_owned();
    let id = crate::neo::location(rt, id);
    if let Some(field) = id.strip_prefix("neoforge:").map(str::to_ascii_uppercase)
        && crate::neo::neoforge_data_map(&field) == Some(registry.as_str())
    {
        let key = rt_key(rt, &field);
        return api::get_static(rt, key);
    }
    registered(rt)
        .into_iter()
        .find(|&t| type_of(rt, t).is_some_and(|(i, r)| i == id && same_registry(&r, &registry)))
        .unwrap_or(V::NULL)
}

/// `NeoForgeDataMaps.<field>`'s key.
fn rt_key(rt: &mut Rt, field: &str) -> &'static str {
    rt.static_key(&format!("{BUILT_IN}NeoForgeDataMaps.{field}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const GET_BURN_TIME: &str = "net/minecraft/world/item/ItemStack.getBurnTime.(Lnet/minecraft/world/item/crafting/RecipeType;)I";
    const IS_FUEL: &str = "net/minecraft/world/level/block/entity/AbstractFurnaceBlockEntity.isFuel.(Lnet/minecraft/world/item/ItemStack;)Z";
    const SET_BURN_TIME: &str =
        "net/neoforged/neoforge/event/furnace/FurnaceFuelBurnTimeEvent.setBurnTime.(I)V";
    const GET_STACK: &str = "net/neoforged/neoforge/event/furnace/FurnaceFuelBurnTimeEvent.getItemStack.()Lnet/minecraft/world/item/ItemStack;";
    const FUELS_FILE: &str = "data/neoforge/data_maps/item/furnace_fuels.json";

    fn host() -> MemoryHost {
        let mut host = MemoryHost::default();
        for (tag, items) in [
            (
                "minecraft:logs",
                vec!["minecraft:oak_log", "minecraft:crimson_stem"],
            ),
            (
                "minecraft:non_flammable_wood",
                vec!["minecraft:crimson_stem"],
            ),
            ("minecraft:planks", vec!["minecraft:oak_planks"]),
        ] {
            host.item_tags
                .insert(tag.into(), items.into_iter().map(str::to_owned).collect());
        }
        host
    }

    fn burn(rt: &mut Rt, item: &str) -> i32 {
        let stack = api_common::stack(rt, item, 1);
        api::call(rt, GET_BURN_TIME, &[stack, V::NULL]).i()
    }

    /// NeoForge's own fuels by item and by tag, without the wood that never burns, and a mod's file
    /// of the same map over them: a value of its own, a vanilla fuel's time changed, one removed
    /// (`remove`), one under conditions that don't hold (another mod not loaded) and one under
    /// conditions that do, with its value apart. An empty stack and a stone don't burn; a furnace
    /// takes as fuel what burns.
    #[test]
    fn an_items_burn_time_is_its_value_in_neoforges_fuel_map() {
        crate::install();
        let mut rt = Rt::new();
        let mut host = host();
        host.resources.insert(
            FUELS_FILE.into(),
            br##"{"values": {
                "testmod:peat": {"burn_time": 3200},
                "minecraft:coal": {"value": 800, "replace": true},
                "minecraft:dirt": {"neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "absent"}], "burn_time": 50},
                "minecraft:gravel": {"neoforge:conditions": [{"type": "neoforge:true"}], "neoforge:value": 70}
            }, "remove": ["minecraft:bowl"]}"##
                .to_vec(),
        );
        rt.with_host(&mut host, |rt| {
            let times: Vec<i32> = [
                "minecraft:charcoal",
                "minecraft:oak_log",
                "minecraft:oak_planks",
                "minecraft:crimson_stem",
                "testmod:peat",
                "minecraft:coal",
                "minecraft:bowl",
                "minecraft:dirt",
                "minecraft:gravel",
                "minecraft:stone",
            ]
            .iter()
            .map(|i| burn(rt, i))
            .collect();
            assert_eq!(times, [1600, 300, 300, 0, 3200, 800, 0, 0, 70, 0]);
            let empty = api_common::stack(rt, "minecraft:coal", 0);
            assert_eq!(api::call(rt, GET_BURN_TIME, &[empty, V::NULL]), V::I(0));
            let (log, stone) = (
                api_common::stack(rt, "minecraft:oak_log", 1),
                api_common::stack(rt, "minecraft:stone", 1),
            );
            assert_eq!(
                [
                    api::call(rt, IS_FUEL, &[log]),
                    api::call(rt, IS_FUEL, &[stone])
                ],
                [V::I(1), V::I(0)]
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A file that doesn't decode (a burn time of 0) is left out whole; one with `replace` drops
    /// NeoForge's own fuels.
    #[test]
    fn a_file_that_doesnt_decode_is_left_out_and_replace_drops_the_ones_before() {
        crate::install();
        for (file, coal, peat) in [
            (
                r#"{"values": {"testmod:peat": 3200, "minecraft:stick": 0}}"#,
                1600,
                0,
            ),
            (
                r#"{"replace": true, "values": {"testmod:peat": 3200}}"#,
                0,
                3200,
            ),
        ] {
            let mut rt = Rt::new();
            let mut host = host();
            host.resources
                .insert(FUELS_FILE.into(), file.as_bytes().to_vec());
            rt.with_host(&mut host, |rt| {
                assert_eq!(
                    [burn(rt, "minecraft:coal"), burn(rt, "testmod:peat")],
                    [coal, peat],
                    "{file}"
                );
            });
        }
    }

    /// A mod's item that overrides `getBurnTime` burns for its own time, a listener of
    /// `FurnaceFuelBurnTimeEvent` sees the stack and sets paper's, and a negative time is an
    /// `IllegalStateException`.
    #[test]
    fn a_mods_item_and_listeners_decide_a_burn_time() {
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match rt.class_of(args[0]) {
                "test/Battery" if short == ITEM_BURN_TIME.1 => V::I(5000),
                "test/Broken" if short == ITEM_BURN_TIME.1 => V::I(-1),
                _ => crate::api::call(rt, key, args),
            }
        }
        fn listener(rt: &mut Rt, _: u32, args: &[V]) -> Option<V> {
            let ev = args[1];
            let stack = api::call(rt, GET_STACK, &[ev]);
            let seen = rt.captured(args[0])[0];
            jutil::list_add(rt, seen, stack);
            if api_common::item_like_name(rt, rt.get_field(stack, "$item")) == "minecraft:paper" {
                api::call(rt, SET_BURN_TIME, &[ev, V::I(40)]);
            }
            Some(V::Void)
        }
        crate::install();
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(listener);
        for (class, id) in [
            ("test/Battery", "testmod:battery"),
            ("test/Broken", "testmod:broken"),
        ] {
            rt.supers.insert(class, "net/minecraft/world/item/Item");
            let item = rt.new_object(class);
            rt.registered.insert(("item".into(), id.into()), item);
        }
        let seen = jutil::new_list(&mut rt, Vec::new());
        let l = rt.lambda_id("java/util/function/Consumer", 1, vec![seen]);
        rt.listeners.push((FUEL_EVENT.to_owned(), l));
        let mut host = host();
        rt.with_host(&mut host, |rt| {
            assert_eq!(
                [
                    burn(rt, "testmod:battery"),
                    burn(rt, "minecraft:paper"),
                    burn(rt, "minecraft:coal")
                ],
                [5000, 40, 1600]
            );
            assert_eq!(
                jutil::elements(rt, seen).len(),
                3,
                "the listener heard each stack"
            );
            assert_eq!(burn(rt, "testmod:broken"), 0);
            let thrown = rt.thrown.take().expect("a negative burn time throws");
            assert_eq!(rt.class_of(thrown), "java/lang/IllegalStateException");
            assert_eq!(
                rt.str_of(rt.get_field(thrown, "message")),
                Some("Stack of item testmod:broken has a negative burn time")
            );
        });
    }

    /// A mod's own data map type of blocks, registered through the event and read with its codec
    /// (an int): a holder's value, the registry's by key and its whole map; a type of another
    /// registry gives nothing, and the type registered again is refused.
    #[test]
    fn a_mods_data_map_type_reads_its_values_with_its_codec() {
        crate::install();
        let mut rt = Rt::new();
        let id = rt.string("testmod:hardness");
        let blocks = api::get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK");
        let int = api::get_static(&mut rt, "com/mojang/serialization/Codec.INT");
        let builder = api::call(
            &mut rt,
            "net/neoforged/neoforge/registries/datamaps/DataMapType.builder.(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceKey;Lcom/mojang/serialization/Codec;)Lnet/neoforged/neoforge/registries/datamaps/DataMapType$Builder;",
            &[id, blocks, int],
        );
        let ty = api::call(
            &mut rt,
            "net/neoforged/neoforge/registries/datamaps/DataMapType$Builder.build.()Lnet/neoforged/neoforge/registries/datamaps/DataMapType;",
            &[builder],
        );
        let event = rt.new_object(REGISTER_TYPES);
        let register = "net/neoforged/neoforge/registries/datamaps/RegisterDataMapTypesEvent.register.(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;)V";
        api::call(&mut rt, register, &[event, ty]);
        assert!(rt.thrown.is_none());
        api::call(&mut rt, register, &[event, ty]);
        let thrown = rt.thrown.take().expect("registered twice");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");

        let mut host = MemoryHost::default();
        host.block_tags
            .insert("minecraft:logs".into(), vec!["minecraft:oak_log".into()]);
        host.resources.insert(
            "data/testmod/data_maps/block/hardness.json".into(),
            br##"{"values": {"minecraft:stone": 3, "#minecraft:logs": {"value": 2}}}"##.to_vec(),
        );
        rt.with_host(&mut host, |rt| {
            let stone = crate::neo::registry_holder(rt, "block", "minecraft:stone", V::NULL);
            let data = api::call(rt, "net/minecraft/core/Holder.getData.(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;)Ljava/lang/Object;", &[stone, ty]);
            assert_eq!(rt.get_field(data, "value"), V::I(3));
            let key = rt.string("block#minecraft:oak_log");
            let log = api::call(rt, "net/minecraft/core/Registry.getData.(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;Lnet/minecraft/resources/ResourceKey;)Ljava/lang/Object;", &[blocks, ty, key]);
            assert_eq!(rt.get_field(log, "value"), V::I(2));
            let items = api::get_static(rt, "net/minecraft/core/registries/Registries.ITEM");
            let none = api::call(rt, "net/minecraft/core/Registry.getData.(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;Lnet/minecraft/resources/ResourceKey;)Ljava/lang/Object;", &[items, ty, key]);
            assert_eq!(none, V::NULL, "a type of another registry");
            let map = api::call(rt, "net/minecraft/core/Registry.getDataMap.(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;)Ljava/util/Map;", &[blocks, ty]);
            assert_eq!(api::call(rt, "java/util/Map.size.()I", &[map]), V::I(1), "the logs tag lists no blocks here: stone");
            let fuels = api::get_static(rt, FUELS_FIELD);
            let coal = crate::neo::registry_holder(rt, "item", "minecraft:coal", V::NULL);
            let fuel = api::call(rt, "net/minecraft/core/Holder.getData.(Lnet/neoforged/neoforge/registries/datamaps/DataMapType;)Ljava/lang/Object;", &[coal, fuels]);
            assert_eq!(api::call(rt, "net/neoforged/neoforge/registries/datamaps/builtin/FurnaceFuel.burnTime.()I", &[fuel]), V::I(1600));
        });
    }
}
