//! NeoForge's mod loading, natively: the mod event bus, `RegisterEvent` for each registry,
//! `DeferredHolder`, `ResourceKey` and `ResourceLocation`. A translated mod is constructed as
//! FML constructs it (its `@Mod` class gets the event bus), then registration events fire, so
//! registration libraries (Registrate and the like) run their own translated code.
//!
//! Registry names are their path (`block`, `item`, `block_entity_type`), as
//! `Registries.BLOCK` reads in [`crate::api::get_static`]; an element's `ResourceKey` is
//! `registry#namespace:path`; a `ResourceLocation` is its `namespace:path` string.
use crate::{Data, Rt, V};

/// The registries `RegisterEvent` fires for, in NeoForge's order (blocks before items, items
/// before what refers to them).
pub const REGISTRIES: [&str; 22] = [
    "attribute",
    "sound_event",
    "fluid",
    "mob_effect",
    "block",
    "entity_type",
    "item",
    "potion",
    "particle_type",
    "block_entity_type",
    "custom_stat",
    "menu",
    "recipe_type",
    "recipe_serializer",
    "creative_mode_tab",
    "data_component_type",
    "enchantment_effect_component_type",
    "armor_material",
    "worldgen/feature",
    "loot_condition_type",
    "loot_function_type",
    "attachment_types",
];

const REGISTER_EVENT: &str = "net/neoforged/neoforge/registries/RegisterEvent";
const ACCEPT: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const REGISTRY_BUILDER: &str = "net/neoforged/neoforge/registries/RegistryBuilder";
const NEW_REGISTRY_EVENT: &str = "net/neoforged/neoforge/registries/NewRegistryEvent";
const BAKE: &str = "net/neoforged/neoforge/registries/callback/BakeCallback.onBake.(Lnet/minecraft/core/Registry;)V";

/// The last parameter type of a method descriptor (`(La;Lnet/x/Event;)V` → `net/x/Event`).
fn last_param(desc: &str) -> Option<&str> {
    let params = desc.strip_prefix('(')?.split(')').next()?;
    let mut last = None;
    let mut rest = params;
    while !rest.is_empty() {
        let start = rest;
        let mut dims = 0;
        while rest.starts_with('[') {
            rest = &rest[1..];
            dims += 1;
        }
        if let Some(r) = rest.strip_prefix('L') {
            let end = r.find(';')?;
            last = Some(if dims == 0 { &r[..end] } else { &start[..dims + end + 2] });
            rest = &r[end + 1..];
        } else {
            last = Some(&start[..dims + 1]);
            rest = &rest[1..];
        }
    }
    last
}

/// The event class a listener object takes: a lambda's implementation names it as its last parameter.
fn listener_event(rt: &Rt, listener: V) -> String {
    let class = rt.class_of(listener);
    class.rsplit_once('.').and_then(|(_, desc)| last_param(desc)).unwrap_or("").to_owned()
}

/// Whether an event of class `event` reaches a listener for `wanted` (its own class or a superclass).
pub(crate) fn reaches(rt: &Rt, event: &str, wanted: &str) -> bool {
    let mut c = event;
    loop {
        if c == wanted {
            return true;
        }
        match rt.supers.get(c) {
            Some(&s) => c = s,
            None => return false,
        }
    }
}

/// Posts `event` (an object of class `class`) to the mod bus's listeners. Once one cancels it
/// (`setCanceled(true)` on a cancellable event), the listeners after it don't hear it.
pub fn post(rt: &mut Rt, class: &str, event: V) {
    let profile = profiling();
    for l in hearing(rt, class) {
        let started = profile.then(std::time::Instant::now);
        rt.call_functional(ACCEPT, l, &[event]);
        if let Some(t) = started {
            record(rt, class, l, t.elapsed());
        }
        rt.report_thrown(class);
        if rt.get_field(event, "$canceled").i() != 0 {
            break;
        }
    }
}

/// `IEventBus.unregister(x)`: the listeners `x` added stop listening, as NeoForge's bus keys them by
/// what was registered: a listener `addListener` took, the `@SubscribeEvent` methods of an object
/// `register` took (bound to it), or a class's static ones.
fn unregister(rt: &mut Rt, x: V) {
    if x.r() == 0 {
        return;
    }
    let static_of = (rt.class_of(x) == "java/lang/Class").then(|| rt.str_of(x).unwrap_or("").to_owned());
    let mut keys: Vec<(&'static str, bool)> = Vec::new();
    let mut class = Some(match &static_of {
        Some(c) => rt.static_key(c),
        None => rt.class_of(x),
    });
    while let Some(c) = class {
        for (_, key, is_static, _) in rt.subscribers.get(c).cloned().unwrap_or_default() {
            keys.push((key, is_static));
        }
        // A class's static subscribers are its own; an object's are its class's and its supers'.
        class = if static_of.is_some() { None } else { rt.supers.get(c).copied() };
    }
    let before = rt.listeners.len();
    let listeners = std::mem::take(&mut rt.listeners);
    rt.listeners = listeners
        .into_iter()
        .filter(|(_, l)| {
            if *l == x {
                return false;
            }
            let class = rt.class_of(*l);
            let bound = rt.captured(*l);
            !keys.iter().any(|&(key, is_static)| {
                key == class && if is_static { static_of.is_some() && bound.is_empty() } else { static_of.is_none() && bound == [x] }
            })
        })
        .collect();
    if rt.listeners.len() != before {
        rt.listeners_by_event.clear();
        rt.listeners_indexed = (usize::MAX, usize::MAX);
    }
}

/// The listeners an event of class `class` reaches, in the order they listen (as they were when
/// it was posted: one a listener adds hears the next post). Kept per class
/// ([`Rt::listeners_by_event`]): a tick's events don't walk the mod's hundreds of listeners.
pub(crate) fn hearing(rt: &mut Rt, class: &str) -> Vec<V> {
    let now = (rt.listeners.len(), rt.supers.len());
    if rt.listeners_indexed != now {
        rt.listeners_by_event.clear();
        rt.listeners_indexed = now;
    }
    if let Some(l) = rt.listeners_by_event.get(class) {
        return l.clone();
    }
    let l: Vec<V> = rt.listeners.iter().filter(|(wanted, _)| reaches(rt, class, wanted)).map(|(_, l)| *l).collect();
    rt.listeners_by_event.insert(class.to_owned(), l.clone());
    l
}

/// With `HORIZON_COMPAT_PROFILE` set, what each event's listeners cost is kept ([`profile`]): which
/// of a mod's handlers make a hook slow, measured.
fn profiling() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("HORIZON_COMPAT_PROFILE").is_some())
}

thread_local! {
    static COSTS: std::cell::RefCell<std::collections::HashMap<(String, String), (u32, std::time::Duration)>> = Default::default();
}

fn record(rt: &Rt, class: &str, listener: V, took: std::time::Duration) {
    // A listener is a lambda over the mod's handler method: its class names that method.
    let name = rt.class_of(listener).to_owned();
    COSTS.with(|c| {
        let mut c = c.borrow_mut();
        let e = c.entry((class.rsplit('/').next().unwrap_or(class).to_owned(), name)).or_default();
        e.0 += 1;
        e.1 += took;
    });
}

/// The listeners' costs measured on this thread so far (with `HORIZON_COMPAT_PROFILE`), the
/// costliest first: event, listener, calls, total time.
pub fn profile() -> Vec<(String, String, u32, std::time::Duration)> {
    let mut out: Vec<_> = COSTS.with(|c| c.borrow().iter().map(|((e, l), (n, t))| (e.clone(), l.clone(), *n, *t)).collect());
    out.sort_by(|a, b| b.3.cmp(&a.3));
    out
}

/// Fires what NeoForge's loading fires once the mods are constructed: `FMLConstructModEvent`
/// (Dragons Plus makes its content in it), then `RegisterEvent` once per registry, then setup.
pub fn fire_registrations(rt: &mut Rt) {
    let ev = rt.new_object(CONSTRUCT);
    post(rt, CONSTRUCT, ev);
    let mut names: Vec<String> = REGISTRIES.iter().map(|s| s.to_string()).collect();
    for extra in rt.custom_registries.clone() {
        if !names.contains(&extra) {
            names.push(extra);
        }
    }
    // NeoForge asks for new registries first (`event.create(builder)`, `event.register(registry)`).
    let ev = rt.new_object(NEW_REGISTRY_EVENT);
    post(rt, NEW_REGISTRY_EVENT, ev);
    for registry in names {
        let ev = rt.new_object(REGISTER_EVENT);
        let key = rt.string(&registry);
        rt.put_field(ev, "$registry", key);
        post(rt, REGISTER_EVENT, ev);
    }
    resolve(rt);
    // The registries are frozen: each mod registry's bake callbacks run, once, on the full registry
    // (Create sorts its fan processing types and arm interaction points here).
    let all = bake_callbacks(rt);
    let bakes = std::mem::take(list_mut(rt, all));
    for pair in bakes {
        let (registry, callback) = match rt.heap.get(pair.r() as usize).map(|o| &o.data) {
            Some(Data::List(v)) if v.len() == 2 => (v[0], v[1]),
            _ => continue,
        };
        rt.call_functional(BAKE, callback, &[registry]);
        rt.report_thrown("BakeCallback.onBake");
    }
    // NeoForge's "Config loading" step, then common setup and "Registration events"
    // (capabilities), as its loading goes on. The client's own setup fires on the runtime that
    // draws, at its first hook ([`fire_client_setup`]).
    fire_config_loading(rt);
    fire_common_setup(rt);
    crate::capabilities::fire(rt);
    // Then NeoForge's finish: the mods send each other messages ("Enqueue IMC"), read the ones
    // sent to them ("Process IMC"), complete their loading, and the network registry locks with
    // the payload types they registered.
    for event in [ENQUEUE_IMC, PROCESS_IMC, LOAD_COMPLETE] {
        let ev = rt.new_object(event);
        post(rt, event, ev);
    }
    crate::network::fire_registration(rt);
}

fn is_registry(owner: &str) -> bool {
    matches!(
        owner,
        "net/minecraft/core/Registry"
            | "net/minecraft/core/DefaultedRegistry"
            | "net/minecraft/core/WritableRegistry"
            | "net/minecraft/core/MappedRegistry"
            | "net/minecraft/core/DefaultedMappedRegistry"
            | "net/minecraft/core/IdMap"
            | "net/minecraft/core/HolderLookup"
            | "net/minecraft/core/HolderLookup$RegistryLookup"
            | "net/minecraft/core/HolderGetter"
    )
}

/// The name an object is registered under in `registry` (`create:shaft`).
fn registry_name_of(rt: &Rt, registry: &str, obj: V) -> Option<String> {
    if obj.r() == 0 {
        return None;
    }
    if registry == "entity_type" {
        return crate::mod_entities::type_id(rt, obj).map(str::to_owned);
    }
    // Vanilla's potions and effects know their own ids, and its blocks and items are their names
    // (`api::block`, `api_common::item`); a mod's are registrations.
    rt.registered_name(obj, registry)
        .or_else(|| matches!(registry, "potion" | "mob_effect").then(|| rt.str_of(rt.get_field(obj, "$id"))).flatten())
        .or_else(|| {
            let named = matches!(registry, "block" | "item") && rt.class_of(obj) != "java/lang/String";
            named.then(|| rt.str_of(obj)).flatten().filter(|n| n.contains(':') && !n.starts_with(crate::api::UNREGISTERED))
        })
        .map(str::to_owned)
}

/// A registry lookup's id: a `ResourceLocation` (`ns:path`) or a `ResourceKey` (`registry#ns:path`).
fn name_in(rt: &Rt, v: V) -> String {
    let s = rt.str_of(v).unwrap_or("");
    let s = s.split_once('#').map_or(s, |(_, l)| l);
    if s.contains(':') { s.to_owned() } else { format!("minecraft:{s}") }
}

/// What `registry` holds under `id`: the mod's registrations, vanilla's blocks and items.
pub(crate) fn registry_get(rt: &mut Rt, registry: &str, id: &str) -> V {
    if let Some(v) = rt.registered.get(&(registry.to_owned(), id.to_owned())) {
        return *v;
    }
    // Through its aliases (`addAlias`), a few at most: a cycle names nothing.
    let mut name = id.to_owned();
    for _ in 0..8 {
        let Some(to) = rt.interned.get(&format!("alias/{registry}/{name}")).and_then(|t| rt.str_of(*t)).map(str::to_owned) else { break };
        if let Some(v) = rt.registered.get(&(registry.to_owned(), to.clone())) {
            return *v;
        }
        name = to;
    }
    let id = name.as_str();
    match registry {
        "block" if id.starts_with("minecraft:") => crate::api::block(rt, id),
        "item" if id.starts_with("minecraft:") => crate::api_common::item(rt, id),
        "fluid" if id.starts_with("minecraft:") => crate::fluids::fluid(rt, id),
        "entity_type" if id.starts_with("minecraft:") => crate::mod_entities::vanilla_type(rt, id),
        "potion" if id.starts_with("minecraft:") => {
            let h = crate::food::potion_holder(rt, id);
            rt.get_field(h, "$value")
        }
        // Enchantments are data the runtime doesn't list: a name is taken to be one, as the
        // game's data has it (`enchantments.rs`).
        "enchantment" if crate::recipes::valid_location(id) => crate::enchantments::value(rt, id),
        _ => V::NULL,
    }
}

/// A registry entry's `Holder`: its key and value. One per named entry, as vanilla's
/// `Holder.Reference`s are (they compare by identity); an intrusive holder, made before its value
/// has a name, is its own.
pub(crate) fn registry_holder(rt: &mut Rt, registry: &str, id: &str, value: V) -> V {
    let made = |rt: &mut Rt, value: V| {
        let h = rt.new_object("net/minecraft/core/Holder$Reference");
        let (r, i) = (rt.string(registry), rt.string(id));
        rt.put_field(h, "$registry", r);
        rt.put_field(h, "$id", i);
        rt.put_field(h, "$value", value);
        h
    };
    if id.is_empty() {
        return made(rt, value);
    }
    let h = rt.intern(format!("compat:holder/{registry}/{id}"), |rt| made(rt, V::NULL));
    // A holder asked for before its entry was registered learns the value when one comes.
    if value.r() != 0 && rt.get_field(h, "$value").r() == 0 {
        rt.put_field(h, "$value", value);
    }
    h
}

fn optional(rt: &mut Rt, v: V) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", v);
    o
}

/// A `ResourceLocation`'s `namespace:path` (`minecraft` when it names none).
pub(crate) fn location(rt: &Rt, v: V) -> String {
    let s = rt.str_of(v).unwrap_or("");
    if s.contains(':') { s.to_owned() } else { format!("minecraft:{s}") }
}

/// `ResourceLocation.bySeparator(s, separator)`'s id, or the message of the
/// `ResourceLocationException` it throws: the namespace before the first `separator` (`minecraft`
/// for none, or an empty one) of `[a-z0-9_.-]`, the path after it of `[a-z0-9/._-]`, the namespace
/// checked first.
pub(crate) fn checked_location(s: &str, separator: char) -> Result<String, String> {
    let (ns, path) = match s.find(separator) {
        Some(0) => ("minecraft", &s[separator.len_utf8()..]),
        Some(i) => (&s[..i], &s[i + separator.len_utf8()..]),
        None => ("minecraft", s),
    };
    if !ns.chars().all(namespace_char) {
        return Err(format!("Non [a-z0-9_.-] character in namespace of location: {ns}:{path}"));
    }
    if !path.chars().all(path_char) {
        return Err(format!("Non [a-z0-9/._-] character in path of location: {ns}:{path}"));
    }
    Ok(format!("{ns}:{path}"))
}

/// `ResourceLocation.validPathChar`.
fn path_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '/')
}

/// `ResourceLocation.validNamespaceChar`.
fn namespace_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.')
}

/// A `ResourceKey`'s registry and element location (`block#create:shaft` → (`block`, `create:shaft`)).
pub fn split_key(rt: &Rt, key: V) -> (String, String) {
    let s = rt.str_of(key).unwrap_or("");
    match s.split_once('#') {
        Some((r, l)) => (r.to_owned(), l.to_owned()),
        None => (String::new(), s.to_owned()),
    }
}

/// The bake callbacks of the mod registries made so far, as `[registry, callback]` pairs.
fn bake_callbacks(rt: &mut Rt) -> V {
    rt.intern("compat:bake-callbacks".into(), |rt| rt.alloc("java/util/ArrayList", Data::List(Vec::new())))
}

fn list_mut(rt: &mut Rt, list: V) -> &mut Vec<V> {
    match rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(v)) => v,
        _ => unreachable!("compat's own lists are lists"),
    }
}

/// `RegistryBuilder.create()`: a mod's registry is its name, as vanilla's are here. What the mod
/// registers to it lands in `rt.registered` under that name; its bake callbacks wait for the freeze.
fn make_registry(rt: &mut Rt, builder: V) -> V {
    let registry = rt.get_field(builder, "$key");
    if let Some(name) = rt.str_of(registry).map(str::to_owned)
        && !name.is_empty()
        && !rt.custom_registries.contains(&name)
        && !REGISTRIES.contains(&name.as_str())
    {
        rt.custom_registries.push(name);
    }
    let callbacks = match rt.heap.get(rt.get_field(builder, "$bake").r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.clone(),
        _ => Vec::new(),
    };
    let all = bake_callbacks(rt);
    for callback in callbacks {
        let pair = rt.alloc("java/util/List", Data::List(vec![registry, callback]));
        list_mut(rt, all).push(pair);
    }
    registry
}

fn register(rt: &mut Rt, registry: &str, name: &str, obj: V) {
    if obj.r() != 0 {
        rt.registered.insert((registry.to_owned(), name.to_owned()), obj);
        if registry == "block" {
            crate::api::adopt_names(rt);
        }
    }
}

/// Runs the registered suppliers (`DeferredRegister`'s), then names the states their blocks made.
pub(crate) fn resolve(rt: &mut Rt) {
    rt.resolve_registrations();
    crate::api::adopt_names(rt);
}

fn name_of(rt: &Rt, v: V) -> String {
    let s = rt.str_of(v).unwrap_or("");
    let s = s.split_once('#').map_or(s, |(_, l)| l);
    if s.contains(':') { s.to_owned() } else { format!("{}:{s}", rt.namespace) }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner.starts_with("net/neoforged/")
        || crate::scan::claims(owner)
        || owner.starts_with("thedarkcolour/kotlinforforge/")
        || owner == REGISTRY_SET_BUILDER
        || matches!(owner, ARGUMENT_TYPE_INFOS | SINGLETON_ARGUMENT_INFO)
        || owner.starts_with("net/minecraft/resources/")
        || is_registry(owner)
        || owner.starts_with("org/apache/maven/artifact/versioning/")
        || owner.starts_with("cpw/mods/modlauncher/")
        || matches!(owner, "net/minecraft/core/Holder" | "net/minecraft/core/Holder$Reference" | "net/minecraft/core/Holder$Direct")
        || (owner.starts_with("net/minecraft/") && matches!(name, "registryAccess" | "holderLookup" | "registryOrThrow" | "lookupOrThrow" | "registry" | "lookup"))
        || (name == "canPerformAction" && owner.starts_with("net/minecraft/world/item/"))
}

const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const ITEM: &str = "net/minecraft/world/item/Item";
/// `IItemExtension.canPerformAction(stack, ability)`, as a call on an item names it.
const CAN_PERFORM: &str = "canPerformAction.(Lnet/minecraft/world/item/ItemStack;Lnet/neoforged/neoforge/common/ItemAbility;)Z";
const ITEM_CAN_PERFORM: &str = "net/minecraft/world/item/Item.canPerformAction.(Lnet/minecraft/world/item/ItemStack;Lnet/neoforged/neoforge/common/ItemAbility;)Z";

/// The abilities NeoForge 1.21.1's patched item classes give their items (`canPerformAction` over
/// `ItemAbilities.DEFAULT_*_ACTIONS`, as their class files say): none for the other classes.
fn default_abilities(class: &str) -> &'static [&'static str] {
    match class {
        "AxeItem" => &["axe_dig", "axe_strip", "axe_scrape", "axe_wax_off"],
        "HoeItem" => &["hoe_dig", "till"],
        "ShovelItem" => &["shovel_dig", "shovel_flatten", "shovel_douse"],
        "PickaxeItem" => &["pickaxe_dig"],
        "SwordItem" => &["sword_dig", "sword_sweep"],
        "ShearsItem" => &["shears_dig", "shears_harvest", "shears_remove_armor", "shears_carve", "shears_disarm", "shears_trim"],
        "ShieldItem" => &["shield_block"],
        "FishingRodItem" => &["fishing_rod_cast"],
        "TridentItem" => &["trident_throw"],
        "BrushItem" => &["brush_brush"],
        "FlintAndSteelItem" | "FireChargeItem" => &["firestarter_light"],
        "SpyglassItem" => &["spyglass_scope"],
        _ => &[],
    }
}

/// The vanilla class an item is of, by its simple name: a mod item's first vanilla class, a vanilla
/// item's by its name (`minecraft:iron_axe` is an `AxeItem`) among the classes with abilities.
fn item_class(rt: &Rt, item: V) -> &'static str {
    let mut class = rt.class_of(item);
    for _ in 0..64 {
        if class.starts_with("net/minecraft/world/item/") || class.is_empty() {
            break;
        }
        match rt.supers.get(class).copied() {
            Some(up) => class = up,
            None => break,
        }
    }
    if class != ITEM {
        return class.rsplit('/').next().unwrap_or(class);
    }
    let name = rt.str_of(item).unwrap_or("");
    let Some(path) = name.strip_prefix("minecraft:") else { return "Item" };
    match path {
        "shears" => "ShearsItem",
        "shield" => "ShieldItem",
        "fishing_rod" => "FishingRodItem",
        "trident" => "TridentItem",
        "brush" => "BrushItem",
        "flint_and_steel" => "FlintAndSteelItem",
        "fire_charge" => "FireChargeItem",
        "spyglass" => "SpyglassItem",
        p if p.ends_with("_pickaxe") => "PickaxeItem",
        p if p.ends_with("_axe") => "AxeItem",
        p if p.ends_with("_hoe") => "HoeItem",
        p if p.ends_with("_shovel") => "ShovelItem",
        p if p.ends_with("_sword") => "SwordItem",
        _ => "Item",
    }
}

/// NeoForge's `ItemAbility` named `name` (`axe_strip`): one object per name.
pub(crate) fn item_ability(rt: &mut Rt, name: &str) -> V {
    rt.intern(format!("item-ability/{name}"), |rt| {
        let o = rt.new_object("net/neoforged/neoforge/common/ItemAbility");
        let n = rt.string(name);
        rt.put_field(o, "name", n);
        o
    })
}

/// A NeoForge loading call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    // A call through the class a mod checked for, or through the interface it holds, is the
    // container's or the metadata's own.
    let owner = match owner {
        FML_CONTAINER => MOD_CONTAINER,
        "net/neoforged/neoforgespi/language/IModInfo" => MOD_INFO,
        "org/apache/maven/artifact/versioning/ArtifactVersion" => VERSION,
        "net/neoforged/neoforgespi/language/IModFileInfo" => MOD_FILE_INFO,
        "thedarkcolour/kotlinforforge/KotlinModLoadingContext$Companion" => KOTLIN_CONTEXT_COMPANION,
        "thedarkcolour/kotlinforforge/KotlinModLoadingContext" => KOTLIN_CONTEXT,
        o => o,
    };
    if crate::scan::claims(owner) {
        return crate::scan::call(rt, owner, name, desc, args);
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // What a crash report would list: there are no Java crash reports here.
        ("net/neoforged/fml/CrashReportCallables", "registerCrashCallable" | "registerHeader") => V::Void,
        // `ItemAbility.get(name)`: one object per name, as NeoForge interns them.
        ("net/neoforged/neoforge/common/ItemAbility", "get") => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            item_ability(rt, &n)
        }
        ("net/neoforged/neoforge/common/ItemAbility", "name") => rt.get_field(a(0), "name"),
        ("net/neoforged/bus/api/IEventBus", "addListener") => {
            let listener = *args.last()?;
            // A class argument names the event; otherwise the lambda's parameter does.
            let wanted = args
                .iter()
                .find(|&&v| rt.class_of(v) == "java/lang/Class")
                .and_then(|&v| rt.str_of(v).map(str::to_owned))
                .unwrap_or_else(|| listener_event(rt, listener));
            rt.listeners.push((wanted, listener));
            V::Void
        }
        // `bus.unregister(x)`: what `register(x)` or `addListener(x)` added stops listening.
        ("net/neoforged/bus/api/IEventBus", "unregister") => {
            unregister(rt, a(1));
            V::Void
        }
        // `bus.register(Foo.class)` listens with its static `@SubscribeEvent` methods, `bus.register(obj)`
        // with the object's.
        ("net/neoforged/bus/api/IEventBus", "register") => {
            if rt.class_of(a(1)) == "java/lang/Class" {
                let class = rt.str_of(a(1)).unwrap_or("").to_owned();
                rt.subscribe_class(&class);
            } else {
                rt.subscribe_object(a(1));
            }
            V::Void
        }
        // Level events and a level's tick: the level they're about, with time to spare (`hasTime`).
        (o, "getLevel") if o.starts_with("net/neoforged/neoforge/event/level/LevelEvent") || o.starts_with("net/neoforged/neoforge/event/tick/LevelTickEvent") => {
            rt.get_field(a(0), "$level")
        }
        (o, "hasTime") if o.starts_with("net/neoforged/neoforge/event/tick/LevelTickEvent") => V::I(1),
        // The server's tick: the one server, with time to spare (`hasTime`).
        (o, "getServer") if o.starts_with("net/neoforged/neoforge/event/tick/ServerTickEvent") => {
            rt.intern("compat:server".into(), |rt| rt.new_object("net/minecraft/server/MinecraftServer"))
        }
        (o, "hasTime") if o.starts_with("net/neoforged/neoforge/event/tick/ServerTickEvent") => V::I(1),
        // `ServerLifecycleHooks.getCurrentServer()`: the server from its `ServerAboutToStartEvent`
        // (or once its world's level is made, as `LevelEvent.Load` hears it) until it stopped, in
        // the server's runtime; null before and after, and in the client's (a client's world can
        // be another machine's).
        ("net/neoforged/neoforge/server/ServerLifecycleHooks", "getCurrentServer") => {
            if is_client(rt) || !(rt.interned.contains_key(SERVER_RUNNING) || rt.interned.contains_key("compat:level")) {
                V::NULL
            } else {
                server(rt)
            }
        }
        // A server lifecycle event's server: the one it was posted about.
        (o, "getServer") if o.starts_with(SERVER_EVENTS) => match rt.get_field(a(0), "$server") {
            V::R(n) if n != 0 => V::R(n),
            _ => server(rt),
        },
        // `ModLoader.postEvent(event)` (or in mod order, `postEventWrapContainerInModOrder`): a mod
        // posting its own event to every mod's bus (FramedBlocks', Curios' registration events).
        ("net/neoforged/fml/ModLoader", "postEvent" | "postEventWrapContainerInModOrder" | "postEventWithReturn") => {
            let class = rt.class_of(a(0));
            post(rt, class, a(0));
            if desc.ends_with(")V") { V::Void } else { a(0) }
        }
        ("net/neoforged/bus/api/IEventBus", "post") => {
            let class = rt.class_of(a(1));
            post(rt, class, a(1));
            a(1)
        }
        // The registry an event fires for is its name, so `event.getRegistry() ==
        // BuiltInRegistries.TRIGGER_TYPES` holds for the same interned string.
        (REGISTER_EVENT, "getRegistryKey" | "getRegistry") => rt.get_field(a(0), "$registry"),
        // A mod's own registry: `new RegistryBuilder<>(KEY).sync(true).onBake(cb).create()`, or made
        // through `NewRegistryEvent.create(builder)` or `DeferredRegister.makeRegistry(builder -> ..)`.
        (REGISTRY_BUILDER, "<init>") => {
            rt.put_field(a(0), "$key", a(1));
            V::Void
        }
        (REGISTRY_BUILDER, "onBake") => {
            let mut list = rt.get_field(a(0), "$bake");
            if list.r() == 0 {
                list = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
                rt.put_field(a(0), "$bake", list);
            }
            list_mut(rt, list).push(a(1));
            a(0)
        }
        (REGISTRY_BUILDER, "create") => make_registry(rt, a(0)),
        // Its other settings (`sync`, `withIntrusiveHolders`, `maxId`, `defaultKey`...): nothing here
        // syncs registries or caps their ids.
        (REGISTRY_BUILDER, _) if desc.ends_with(")Lnet/neoforged/neoforge/registries/RegistryBuilder;") => a(0),
        (NEW_REGISTRY_EVENT, "create") => make_registry(rt, a(1)),
        (NEW_REGISTRY_EVENT, "register") => V::Void,
        (o, "makeRegistry") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let builder = rt.new_object(REGISTRY_BUILDER);
            let key = rt.get_field(a(0), "$registry");
            rt.put_field(builder, "$key", key);
            rt.call_functional(ACCEPT, a(1), &[builder]);
            make_registry(rt, builder)
        }
        // `WritableRegistry.register(key, value, info)`: Create adds its registries to the registry
        // of registries so; a value registered by key, handed back as its holder.
        ("net/minecraft/core/WritableRegistry" | "net/minecraft/core/MappedRegistry", "register") if rt.str_of(a(0)).is_some() => {
            let reg = rt.str_of(a(0)).unwrap_or("").to_owned();
            let (_, loc) = split_key(rt, a(1));
            let n = if loc.contains(':') { loc } else { format!("minecraft:{loc}") };
            register(rt, &reg, &n, a(2));
            registry_holder(rt, &reg, &n, a(2))
        }
        (REGISTER_EVENT, "register") => {
            let this = a(0);
            let registry = rt.get_field(this, "$registry");
            if !crate::jutil::equals(rt, registry, a(1)) {
                return Some(V::Void);
            }
            let reg = rt.str_of(registry).unwrap_or("").to_owned();
            if desc.contains("Ljava/util/function/Consumer;") {
                let helper = rt.new_object("net/neoforged/neoforge/registries/RegisterEvent$RegisterHelper");
                rt.put_field(helper, "$registry", registry);
                rt.call_functional(ACCEPT, a(2), &[helper]);
            } else {
                let obj = rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", a(3), &[]);
                let n = name_of(rt, a(2));
                register(rt, &reg, &n, obj);
            }
            V::Void
        }
        ("net/neoforged/neoforge/registries/RegisterEvent$RegisterHelper", "register") => {
            let registry = rt.get_field(a(0), "$registry");
            let reg = rt.str_of(registry).unwrap_or("").to_owned();
            let n = name_of(rt, a(1));
            register(rt, &reg, &n, a(2));
            V::Void
        }
        // ResourceLocation: its `namespace:path` string.
        ("net/minecraft/resources/ResourceLocation", "fromNamespaceAndPath") => {
            let s = format!("{}:{}", rt.str_of(a(0)).unwrap_or("minecraft"), rt.str_of(a(1)).unwrap_or(""));
            rt.string(&s)
        }
        ("net/minecraft/resources/ResourceLocation", "withDefaultNamespace") => {
            let s = format!("minecraft:{}", rt.str_of(a(0)).unwrap_or(""));
            rt.string(&s)
        }
        // `ObfuscationReflectionHelper.setPrivateValue(class, instance, value, name)` and
        // `getPrivateValue(class, instance, name)`: a field by its name (NeoForge runs on Mojang's
        // names): a translated class's own field; a game class's as a mixin accessor reaches it (the
        // `$` field the runtime keeps its state in, unless a mod wrote the plain one); a static
        // without an instance. Registrate clears
        // `Properties.drops` (what the block it copies drops) so a block drops its own loot: the
        // runtime's record of `noLootTable` and `dropsLike`.
        ("net/neoforged/fml/util/ObfuscationReflectionHelper", "setPrivateValue" | "getPrivateValue") => {
            let set = name == "setPrivateValue";
            let class = rt.str_of(a(0)).unwrap_or("").to_owned();
            let field = rt.str_of(if set { a(3) } else { a(2) }).unwrap_or("").to_owned();
            let (instance, value) = (a(1), a(2));
            if instance.is_null() {
                let key = rt.static_key(&format!("{class}.{field}"));
                if !set {
                    return Some(rt.statics.get(key).copied().unwrap_or(V::NULL));
                }
                rt.statics.insert(key, value);
            } else if set && field == "drops" && rt.class_of(instance) == crate::blocks::BLOCK_PROPERTIES {
                rt.put_field(instance, "$noLootTable", V::I(0));
                rt.put_field(instance, "$dropsLike", V::NULL);
                rt.put_field(instance, "$drops", value);
            } else if rt.is_mod_class(&class) {
                let plain: &'static str = rt.static_key(&field);
                if !set {
                    return Some(rt.get_field(instance, plain));
                }
                rt.put_field(instance, plain, value);
            } else {
                let own = format!("${field}");
                let own: &'static str = rt.static_key(&own);
                let plain: &'static str = rt.static_key(&field);
                if !set {
                    return Some(match rt.get_field(instance, own) {
                        V::Void => rt.get_field(instance, plain),
                        v => v,
                    });
                }
                let to_own = !matches!(rt.get_field(instance, own), V::Void) || matches!(rt.get_field(instance, plain), V::Void);
                rt.put_field(instance, if to_own { own } else { plain }, value);
            }
            V::Void
        }
        // `tryBuild(namespace, path)`: null unless both are valid (lowercase letters, digits, `_-.`,
        // and `/` in the path), as vanilla checks them.
        ("net/minecraft/resources/ResourceLocation", "tryBuild") => {
            let (ns, path) = (rt.str_of(a(0)).unwrap_or("").to_owned(), rt.str_of(a(1)).unwrap_or("").to_owned());
            let ok = |s: &str, slash: bool| s.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.') || (slash && c == '/'));
            if a(0).is_null() || a(1).is_null() || !ok(&ns, false) || !ok(&path, true) {
                return Some(V::NULL);
            }
            rt.string(&format!("{ns}:{path}"))
        }
        // An id from a string, split at its first `:` (or the separator given): `parse` and
        // `bySeparator` throw `ResourceLocationException` for a character vanilla doesn't allow,
        // `tryParse` and `tryBySeparator` give null, `read(String)` an error result.
        ("net/minecraft/resources/ResourceLocation", "parse" | "tryParse" | "bySeparator" | "tryBySeparator" | "read")
            if desc.starts_with("(Ljava/lang/String;") =>
        {
            if a(0).is_null() {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::NULL);
            }
            let s = rt.str_of(a(0)).unwrap_or("").to_owned();
            let separator = if name.ends_with("ySeparator") { char::from_u32(a(1).i() as u32).unwrap_or(':') } else { ':' };
            match checked_location(&s, separator) {
                Ok(id) => {
                    let v = rt.string(&id);
                    if name == "read" { crate::codecs::success(rt, v) } else { v }
                }
                Err(_) if name.starts_with("try") => V::NULL,
                Err(e) if name == "read" => crate::codecs::error(rt, &format!("Not a valid resource location: {s} {e}")),
                Err(e) => {
                    let ex = rt.new_object("net/minecraft/ResourceLocationException");
                    let m = rt.string(&e);
                    rt.put_field(ex, "message", m);
                    rt.thrown = Some(ex);
                    V::NULL
                }
            }
        }
        // An id read from a command line (`read(StringReader)`, `readNonEmpty`), as
        // `ResourceLocationArgument` and mods' own argument types read one: the characters an id
        // may have, then `argument.id.invalid` where it began (`readNonEmpty`: also for none).
        ("net/minecraft/resources/ResourceLocation", "read" | "readNonEmpty") => {
            let reader = a(0);
            let mut r = crate::brigadier::reader_of(rt, reader);
            let start = r.cursor;
            let read = match crate::command_args::read_location(&mut r) {
                Ok(_) if name == "readNonEmpty" && r.cursor == start => Err(r.error("argument.id.invalid", Vec::new())),
                read => read,
            };
            rt.put_field(reader, "$cursor", V::I(r.cursor));
            match read {
                Ok(id) => rt.string(&id),
                Err(e) => {
                    let ex = crate::brigadier::exception(rt, &r.s, &e);
                    rt.thrown = Some(ex);
                    V::NULL
                }
            }
        }
        // The characters an id may have: in a path, in a namespace, and in either or as their `:`
        // (what a command line's id is read over).
        ("net/minecraft/resources/ResourceLocation", "validPathChar" | "validNamespaceChar" | "isAllowedInResourceLocation") => {
            let c = char::from_u32(a(0).i() as u32).unwrap_or('\0');
            V::I(match name {
                "validPathChar" => path_char(c),
                "validNamespaceChar" => namespace_char(c),
                _ => path_char(c) || c == ':',
            } as i32)
        }
        ("net/minecraft/resources/ResourceLocation", "isValidPath" | "isValidNamespace") => {
            if a(0).is_null() {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::I(0));
            }
            let s = rt.str_of(a(0)).unwrap_or("");
            V::I(s.chars().all(if name == "isValidPath" { path_char } else { namespace_char }) as i32)
        }
        // Ids sort by path, then namespace; `compareNamespaced` by namespace first. Each part as
        // `String.compareTo` compares.
        ("net/minecraft/resources/ResourceLocation", "compareTo" | "compareNamespaced") => {
            let (x, y) = (location(rt, a(0)), location(rt, a(1)));
            let (xn, xp) = x.split_once(':').unwrap_or(("minecraft", &x));
            let (yn, yp) = y.split_once(':').unwrap_or(("minecraft", &y));
            let (first, then) = if name == "compareTo" { ((xp, yp), (xn, yn)) } else { ((xn, yn), (xp, yp)) };
            let c = crate::jlang::java_compare(first.0, first.1);
            V::I(if c != 0 { c } else { crate::jlang::java_compare(then.0, then.1) })
        }
        ("net/minecraft/resources/ResourceLocation", "toDebugFileName") => {
            let s = location(rt, a(0)).replace(['/', ':'], "_");
            rt.string(&s)
        }
        ("net/minecraft/resources/ResourceLocation", "getNamespace" | "getPath") => {
            let s = location(rt, a(0));
            let (ns, path) = s.split_once(':').unwrap_or(("minecraft", &s));
            let part = if name == "getNamespace" { ns } else { path }.to_owned();
            rt.string(&part)
        }
        // `namespace.path`, as a translation key names it: `toShortLanguageKey` without the
        // namespace when it is `minecraft`, `toLanguageKey(type)` and `toLanguageKey(type, key)`
        // with the type before and the key after.
        ("net/minecraft/resources/ResourceLocation", "toString" | "toLanguageKey" | "toShortLanguageKey") => {
            let s = location(rt, a(0));
            let (ns, path) = s.split_once(':').unwrap_or(("minecraft", &s));
            let key = format!("{ns}.{path}");
            let arg = |rt: &Rt, i: usize| rt.str_of(a(i)).unwrap_or("null").to_owned();
            let s = match name {
                "toString" => s.clone(),
                "toShortLanguageKey" if ns == "minecraft" => path.to_owned(),
                "toShortLanguageKey" => key,
                _ => match desc_params(desc).len() {
                    0 => key,
                    1 => format!("{}.{key}", arg(rt, 1)),
                    _ => format!("{}.{key}.{}", arg(rt, 1), arg(rt, 2)),
                },
            };
            rt.string(&s)
        }
        ("net/minecraft/resources/ResourceLocation", "withPath" | "withPrefix" | "withSuffix") => {
            let s = location(rt, a(0));
            let (ns, path) = s.split_once(':').unwrap_or(("minecraft", &s));
            let arg = rt.str_of(a(1)).unwrap_or("").to_owned();
            let path = match name {
                "withPath" => arg,
                "withPrefix" => format!("{arg}{path}"),
                _ => format!("{path}{arg}"),
            };
            let s = format!("{ns}:{path}");
            rt.string(&s)
        }
        // ResourceKey: `registry#location`; a registry's own key is its name.
        ("net/minecraft/resources/ResourceKey", "create") => {
            let reg = rt.str_of(a(0)).unwrap_or("").to_owned();
            let loc = location(rt, a(1));
            rt.string(&format!("{reg}#{loc}"))
        }
        ("net/minecraft/resources/ResourceKey", "createRegistryKey") => {
            let loc = location(rt, a(0));
            let path = loc.split_once(':').map_or(loc.as_str(), |(_, p)| p).to_owned();
            if !rt.custom_registries.contains(&path) && !REGISTRIES.contains(&path.as_str()) {
                rt.custom_registries.push(path.clone());
            }
            rt.string(&path)
        }
        ("net/minecraft/resources/ResourceKey", "location") => {
            let (reg, loc) = split_key(rt, a(0));
            let s = if reg.is_empty() { format!("minecraft:{loc}") } else { loc };
            rt.string(&s)
        }
        ("net/minecraft/resources/ResourceKey", "registry" | "registryKey") => {
            let (reg, _) = split_key(rt, a(0));
            let s = if name == "registry" { format!("minecraft:{reg}") } else { reg };
            rt.string(&s)
        }
        ("net/minecraft/resources/ResourceKey", "isFor") => {
            let (reg, _) = split_key(rt, a(0));
            V::I((Some(reg.as_str()) == rt.str_of(a(1))) as i32)
        }
        // Vanilla's Gson adapter for ids (`new ResourceLocation.Serializer()`, Jade's): an id from a
        // JSON string, and back.
        ("net/minecraft/resources/ResourceLocation$Serializer", "<init>") => V::Void,
        ("net/minecraft/resources/ResourceLocation$Serializer", "deserialize") => {
            let text = crate::api::call(rt, "com/google/gson/JsonElement.getAsString.()Ljava/lang/String;", &[a(1)]);
            let id = location(rt, text);
            rt.string(&id)
        }
        ("net/minecraft/resources/ResourceLocation$Serializer", "serialize") => {
            let id = location(rt, a(1));
            crate::gson::string(rt, &id)
        }
        // `DeferredItem.createItem(id)`, `DeferredBlock.createBlock(id)`: a holder of the item or
        // block under `id` (a `ResourceLocation`, or a `ResourceKey`).
        ("net/neoforged/neoforge/registries/DeferredItem", "createItem") | ("net/neoforged/neoforge/registries/DeferredBlock", "createBlock") => {
            let registry = if name == "createItem" { "item" } else { "block" };
            let id = if desc.starts_with("(Lnet/minecraft/resources/ResourceKey;") { split_key(rt, a(0)).1 } else { location(rt, a(0)) };
            let class: &'static str = rt.static_key(owner);
            let h = rt.new_object(class);
            let (r, i) = (rt.string(registry), rt.string(&id));
            rt.put_field(h, "$registry", r);
            rt.put_field(h, "$id", i);
            h
        }
        // DeferredHolder and its subclasses: a registry and a name, resolved when asked.
        (o, "create") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && !o.ends_with("DeferredRegister") => {
            let (registry, id) = if desc.starts_with("(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;") {
                let id = location(rt, a(1));
                (rt.str_of(a(0)).unwrap_or("").to_owned(), id)
            } else {
                split_key(rt, a(0))
            };
            let class: &'static str = if o.ends_with("DeferredBlock") || o.ends_with("DeferredItem") { rt.static_key(o) } else { "net/neoforged/neoforge/registries/DeferredHolder" };
            let h = rt.new_object(class);
            let (r, i) = (rt.string(&registry), rt.string(&id));
            rt.put_field(h, "$registry", r);
            rt.put_field(h, "$id", i);
            h
        }
        (o, "get" | "value") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && rt.get_field(a(0), "$id").r() != 0 => {
            let reg = rt.get_field(a(0), "$registry");
            let id = rt.get_field(a(0), "$id");
            let k = (rt.str_of(reg).unwrap_or("").to_owned(), rt.str_of(id).unwrap_or("").to_owned());
            if !rt.registered.contains_key(&k) && rt.suppliers.iter().any(|(r, n, _)| *r == k.0 && *n == k.1) {
                resolve(rt);
            }
            rt.registered.get(&k).copied().unwrap_or(V::NULL)
        }
        (o, "getId") if o.starts_with("net/neoforged/neoforge/registries/Deferred") => rt.get_field(a(0), "$id"),
        // `DeferredHolder.equals`: a holder of the same key (registry and name), its own or the
        // registry's; `hashCode` is the key's.
        (o, "equals") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && desc == "(Ljava/lang/Object;)Z" => {
            let key = |rt: &Rt, h: V| {
                let (r, i) = (rt.str_of(rt.get_field(h, "$registry"))?, rt.str_of(rt.get_field(h, "$id"))?);
                Some((r.strip_prefix("minecraft:").unwrap_or(r).to_owned(), i.to_owned()))
            };
            let same = a(0) == a(1) || (!a(1).is_null() && key(rt, a(0)).is_some() && key(rt, a(0)) == key(rt, a(1)));
            V::I(same as i32)
        }
        (o, "hashCode") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && desc == "()I" => {
            let id = rt.get_field(a(0), "$id");
            V::I(rt.java_hash(id))
        }
        // `DeferredItem.toStack([count])`, and Registrate's `asStack` reaching the holder: a stack of
        // the item (a block's holder: its item, which has the block's name).
        (o, "toStack" | "asStack") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && rt.get_field(a(0), "$id").r() != 0 => {
            let id = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned();
            let count = if desc.starts_with("(I)") { a(1).i() } else { 1 };
            crate::api_common::stack(rt, &id, count)
        }
        (o, "getKey" | "unwrapKey") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && rt.get_field(a(0), "$id").r() != 0 => {
            let reg = rt.get_field(a(0), "$registry");
            let id = rt.get_field(a(0), "$id");
            let s = format!("{}#{}", rt.str_of(reg).unwrap_or(""), rt.str_of(id).unwrap_or(""));
            let k = rt.string(&s);
            if name == "unwrapKey" {
                let o = rt.new_object("java/util/Optional");
                rt.put_field(o, "value", k);
                o
            } else {
                k
            }
        }
        // A mod's own register (Mekanism's `MekanismDeferredRegister`) made through NeoForge's
        // constructors: what `DeferredRegister.create` keeps. `Blocks`, `Items` and
        // `DataComponents` name their registry themselves.
        (o, "<init>") if o.starts_with("net/neoforged/neoforge/registries/DeferredRegister") => {
            let (registry, ns) = match (o, desc) {
                ("net/neoforged/neoforge/registries/DeferredRegister$Blocks", "(Ljava/lang/String;)V") => ("block".to_owned(), a(1)),
                ("net/neoforged/neoforge/registries/DeferredRegister$Items", "(Ljava/lang/String;)V") => ("item".to_owned(), a(1)),
                ("net/neoforged/neoforge/registries/DeferredRegister$DataComponents", "(Ljava/lang/String;)V") => ("data_component_type".to_owned(), a(1)),
                _ => (rt.str_of(a(1)).unwrap_or("unknown").to_owned(), a(2)),
            };
            let ns = rt.str_of(ns).unwrap_or("").to_owned();
            if rt.namespace.is_empty() {
                rt.namespace = ns.clone();
            }
            let (r, n) = (rt.string(&registry), rt.string(&ns));
            rt.put_field(a(0), "$registry", r);
            rt.put_field(a(0), "$ns", n);
            V::Void
        }
        // `super(key)` from a mod's own holder class (Registrate's `RegistryEntry`).
        (o, "<init>") if o.starts_with("net/neoforged/neoforge/registries/Deferred") && desc.starts_with("(Lnet/minecraft/resources/ResourceKey;") => {
            let (registry, id) = split_key(rt, a(1));
            let (r, i) = (rt.string(&registry), rt.string(&id));
            rt.put_field(a(0), "$registry", r);
            rt.put_field(a(0), "$id", i);
            V::Void
        }
        // A registry read back (registries are their path, `block`): what was registered under a
        // name, and vanilla's blocks and items by name.
        (o, n) if is_registry(o) && rt.str_of(a(0)).is_some() && !matches!(n, "register" | "registerForHolder") => {
            let reg = rt.str_of(a(0)).unwrap_or("").to_owned();
            match n {
                "getKey" => match registry_name_of(rt, &reg, a(1)) {
                    Some(k) => rt.string(&k),
                    None => V::NULL,
                },
                "getResourceKey" => {
                    let v = match registry_name_of(rt, &reg, a(1)) {
                        Some(k) => rt.string(&format!("{reg}#{k}")),
                        None => V::NULL,
                    };
                    optional(rt, v)
                }
                "get" | "getValue" | "getOptional" | "getOrThrow" | "containsKey" | "getHolder" | "getHolderOrThrow" | "wrapAsHolder" => {
                    let id = if n == "wrapAsHolder" { registry_name_of(rt, &reg, a(1)).unwrap_or_default() } else { name_in(rt, a(1)) };
                    let v = registry_get(rt, &reg, &id);
                    // A `HolderGetter`'s `getOrThrow(key)` and `get(key)` give the entry's holder
                    // (in an optional), as `Registry.getHolderOrThrow` and `getHolder` do;
                    // `Registry.get` and `getOrThrow` give its value.
                    let holder = desc.ends_with(")Lnet/minecraft/core/Holder$Reference;");
                    let holder_in_optional = n == "getHolder" || (n == "get" && desc.ends_with(")Ljava/util/Optional;"));
                    match n {
                        "containsKey" => V::I((v.r() != 0) as i32),
                        _ if holder || n == "wrapAsHolder" => registry_holder(rt, &reg, &id, v),
                        _ if holder_in_optional => {
                            let h = if v.r() != 0 { registry_holder(rt, &reg, &id, v) } else { V::NULL };
                            optional(rt, h)
                        }
                        "getOptional" => optional(rt, v),
                        _ => v,
                    }
                }
                "keySet" | "entrySet" | "stream" | "iterator" | "size" | "holders" | "listElements" | "forEach" => {
                    let mut entries: Vec<(String, V)> = rt.registered.iter().filter(|((r, _), _)| *r == reg).map(|((_, k), v)| (k.clone(), *v)).collect();
                    entries.sort_by(|x, y| x.0.cmp(&y.0));
                    match n {
                        "size" => V::I(entries.len() as i32),
                        "keySet" => {
                            let keys: Vec<V> = entries.iter().map(|(k, _)| rt.string(k)).collect();
                            crate::jutil::new_set_of(rt, &keys)
                        }
                        "holders" | "listElements" => {
                            let hs: Vec<V> = entries.iter().map(|(k, v)| registry_holder(rt, &reg, k, *v)).collect();
                            rt.alloc("java/util/stream/Stream", Data::Stream(hs))
                        }
                        "entrySet" => {
                            let mut es = Vec::new();
                            for (k, v) in &entries {
                                let key = rt.string(&format!("{reg}#{k}"));
                                es.push(rt.alloc("java/util/Map$Entry", Data::List(vec![key, *v])));
                            }
                            crate::jutil::new_set_of(rt, &es)
                        }
                        "stream" => {
                            let vs: Vec<V> = entries.iter().map(|(_, v)| *v).collect();
                            rt.alloc("java/util/stream/Stream", Data::Stream(vs))
                        }
                        "forEach" => {
                            for (_, v) in entries {
                                rt.call_functional(ACCEPT, a(1), &[v]);
                            }
                            V::Void
                        }
                        _ => {
                            let vs: Vec<V> = entries.iter().map(|(_, v)| *v).collect();
                            crate::jutil::new_iter(rt, vs)
                        }
                    }
                }
                "key" => rt.string(&reg),
                // The registry owns its holders (`Holder.Reference.createStandAlone(owner, key)`).
                // As its lookup (`asLookup()`): registries are their names here.
                "holderOwner" | "asLookup" => a(0),
                // `addAlias(from, to)`: an old name that reads as the new one (a renamed block).
                "addAlias" => {
                    let (from, to) = (name_in(rt, a(1)), name_in(rt, a(2)));
                    let t = rt.string(&to);
                    rt.interned.insert(format!("alias/{reg}/{from}"), t);
                    V::Void
                }
                // An intrusive holder (Create's `ContraptionType.holder`): made with its value before
                // the value is registered; its key is the name the value is registered under then.
                "createIntrusiveHolder" => registry_holder(rt, &reg, "", a(1)),
                // A tag's members: tags from data files aren't read yet, so every tag of a registry
                // is empty here (fluid tags hold vanilla's water and lava: `fluids.rs`).
                "getTagOrEmpty" => crate::jutil::new_list(rt, Vec::new()),
                "getTag" => optional(rt, V::NULL),
                _ => return None,
            }
        }
        // The level's registries: each is its registry key (its path).
        (o, "registryAccess") if o.starts_with("net/minecraft/") && (o.ends_with("Level") || o.ends_with("LevelAccessor") || o.ends_with("LevelReader") || o.ends_with("MinecraftServer")) => {
            rt.intern("compat:registry-access".into(), |rt| rt.new_object("net/minecraft/core/RegistryAccess"))
        }
        // `level.holderLookup(Registries.BLOCK)`: the registry, as the level's registries give it.
        (o, "holderLookup") if o.starts_with("net/minecraft/") && (o.ends_with("Level") || o.ends_with("LevelAccessor") || o.ends_with("LevelReader")) => a(1),
        (o, "registryOrThrow" | "lookupOrThrow" | "registry" | "lookup") if o.starts_with("net/minecraft/core/") && (o.contains("RegistryAccess") || o.contains("HolderLookup") || o.contains("HolderGetter")) => {
            let key = a(1);
            if matches!(name, "registry" | "lookup") { optional(rt, key) } else { key }
        }
        ("net/neoforged/bus/api/Event", "<init>") => V::Void,
        // A registry as the owner of its holders, and a holder made for a key apart from
        // registering (`Holder.Reference.createStandAlone`): the registry's own holder for the key,
        // whose value is the entry once it is registered.
        (o, "holderOwner") if is_registry(o) => a(0),
        ("net/minecraft/core/Holder$Reference", "createStandAlone") => {
            let (registry, id) = split_key(rt, a(1));
            registry_holder(rt, &registry, &id, V::NULL)
        }
        // A registry entry's `Holder` made above: its value, key and name.
        ("net/minecraft/core/Holder" | "net/minecraft/core/Holder$Reference", n) if rt.get_field(a(0), "$registry").r() != 0 => {
            let (reg, mut id) = (rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("").to_owned(), rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned());
            // An intrusive holder learns its name when its value is registered.
            if id.is_empty()
                && let Some(named) = registry_name_of(rt, &reg, rt.get_field(a(0), "$value"))
            {
                let s = rt.string(&named);
                rt.put_field(a(0), "$id", s);
                id = named;
            }
            match n {
                "value" | "get" => {
                    let v = rt.get_field(a(0), "$value");
                    if v.r() != 0 { v } else { registry_get(rt, &reg, &id) }
                }
                "isBound" => V::I(!id.is_empty() as i32),
                "key" => rt.string(&format!("{reg}#{id}")),
                "unwrapKey" => {
                    let k = rt.string(&format!("{reg}#{id}"));
                    optional(rt, k)
                }
                "getRegisteredName" => rt.string(&id),
                // In a tag of its registry, as the game's loaded tags hold them.
                "is" if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
                    let tag_registry = rt.str_of(rt.get_field(a(1), "$registry")).unwrap_or("").trim_start_matches("minecraft:").to_owned();
                    let tag = rt.str_of(rt.get_field(a(1), "$location")).unwrap_or("").to_owned();
                    let inside = tag_registry == reg.trim_start_matches("minecraft:")
                        && match tag_registry.as_str() {
                            "block" => rt.host().is_some_and(|h| h.block_in_tag(&id, &tag)),
                            "fluid" => rt.host().is_some_and(|h| h.fluid_in_tag(&id, &tag)),
                            "item" => crate::recipes::item_in_tag(rt, &id, &tag),
                            "biome" => rt.host().is_some_and(|h| h.biome_in_tag(&id, &tag)),
                            _ => false,
                        };
                    V::I(inside as i32)
                }
                "is" => V::I((name_in(rt, a(1)) == id) as i32),
                _ => return None,
            }
        }
        // `Holder.direct(value)`: a holder of a value outside any registry.
        ("net/minecraft/core/Holder", "direct") => {
            let h = rt.new_object("net/minecraft/core/Holder$Direct");
            rt.put_field(h, "value", a(0));
            h
        }
        ("net/minecraft/core/Holder" | "net/minecraft/core/Holder$Direct", "value" | "get") if rt.class_of(a(0)) == "net/minecraft/core/Holder$Direct" => rt.get_field(a(0), "value"),
        ("net/minecraft/core/Holder" | "net/minecraft/core/Holder$Direct", "isBound") if rt.class_of(a(0)) == "net/minecraft/core/Holder$Direct" => V::I(1),
        // Vanilla's static registration (`Registry.register(BuiltInRegistries.X, id, value)`).
        ("net/minecraft/core/Registry", "register" | "registerForHolder") => {
            let reg = rt.str_of(a(0)).unwrap_or("").to_owned();
            let n = if desc.starts_with("(Lnet/minecraft/core/Registry;Lnet/minecraft/resources/ResourceKey;") {
                split_key(rt, a(1)).1
            } else {
                location(rt, a(1))
            };
            register(rt, &reg, &n, a(2));
            if name == "registerForHolder" { registry_holder(rt, &reg, &n, a(2)) } else { a(2) }
        }
        // NeoForge's `Lazy`: a supplier run once.
        ("net/neoforged/neoforge/common/util/Lazy", "of" | "concurrentOf") => {
            let l = rt.new_object("net/neoforged/neoforge/common/util/Lazy");
            rt.put_field(l, "$supplier", a(0));
            l
        }
        ("net/neoforged/neoforge/common/util/Lazy", "get") => {
            let this = a(0);
            if rt.get_field(this, "$done").i() == 0 {
                let f = rt.get_field(this, "$supplier");
                let v = rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", f, &[]);
                rt.put_field(this, "$value", v);
                rt.put_field(this, "$done", V::I(1));
            }
            rt.get_field(this, "$value")
        }
        ("net/neoforged/neoforge/common/util/Lazy", "invalidate") => {
            rt.put_field(a(0), "$done", V::I(0));
            V::Void
        }
        // ModLauncher's environment (`Launcher.INSTANCE.environment()`): a player's game, which
        // tells a mod none of its properties (Aquaculture's dev check finds no version).
        ("cpw/mods/modlauncher/Launcher", "environment") => rt.intern("compat:modlauncher-environment".into(), |rt| rt.new_object("cpw/mods/modlauncher/Environment")),
        ("cpw/mods/modlauncher/Environment" | "cpw/mods/modlauncher/api/IEnvironment", "getProperty" | "findLaunchPlugin" | "findLaunchHandler" | "findNameMapping") => {
            crate::api_common::optional(rt, V::NULL)
        }
        // The side: Horizon runs mods in the client.
        ("net/neoforged/fml/loading/FMLLoader", "isProduction") => V::I(1),
        ("net/neoforged/fml/loading/FMLLoader" | "net/neoforged/fml/loading/FMLEnvironment", "getDist") => {
            crate::api::get_static(rt, "net/neoforged/api/distmarker/Dist.CLIENT")
        }
        // The logical side the code runs on: the client's runtime or the server thread's.
        ("net/neoforged/fml/util/thread/EffectiveSide", "get") => {
            let side = if is_client(rt) { "net/neoforged/fml/LogicalSide.CLIENT" } else { "net/neoforged/fml/LogicalSide.SERVER" };
            crate::api::get_static(rt, side)
        }
        ("net/neoforged/fml/LogicalSide", "isClient" | "isServer") => {
            let side = rt.get_field(a(0), "$name");
            let want = if name == "isClient" { "CLIENT" } else { "SERVER" };
            V::I((rt.str_of(side) == Some(want)) as i32)
        }
        // NeoForge's milk fluid, asked for: vanilla's milk bucket stays as it is.
        ("net/neoforged/neoforge/common/NeoForgeMod", "enableMilkFluid") => V::Void,
        ("net/neoforged/api/distmarker/Dist", "isClient" | "isDedicatedServer") => {
            let side = rt.get_field(a(0), "$name");
            let want = if name == "isClient" { "CLIENT" } else { "DEDICATED_SERVER" };
            V::I((rt.str_of(side) == Some(want)) as i32)
        }
        (o, "isBound") if o.starts_with("net/neoforged/neoforge/registries/Deferred") => {
            let reg = rt.get_field(a(0), "$registry");
            let id = rt.get_field(a(0), "$id");
            let k = (rt.str_of(reg).unwrap_or("").to_owned(), rt.str_of(id).unwrap_or("").to_owned());
            V::I(rt.registered.contains_key(&k) as i32)
        }
        // NeoForge configs: a value is its default (Horizon has no config files for translated mods
        // yet), boxed as Java boxes it.
        (CONFIG_BUILDER, "<init>") => V::Void,
        (CONFIG_BUILDER, "configure") => {
            let made = rt.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(1), &[a(0)]);
            let spec = rt.new_object("net/neoforged/neoforge/common/ModConfigSpec");
            let pair = rt.new_object("org/apache/commons/lang3/tuple/Pair");
            rt.put_field(pair, "$left", made);
            rt.put_field(pair, "$right", spec);
            pair
        }
        (CONFIG_BUILDER, "build") => rt.new_object("net/neoforged/neoforge/common/ModConfigSpec"),
        (CONFIG_BUILDER, n) if n.starts_with("define") => {
            let params = desc_params(desc);
            let default = match params.get(1).map(String::as_str) {
                Some("Z") => boxed(rt, "java/lang/Boolean", a(2)),
                Some("I") => boxed(rt, "java/lang/Integer", a(2)),
                Some("J") => boxed(rt, "java/lang/Long", a(2)),
                Some("D") => boxed(rt, "java/lang/Double", a(2)),
                Some("Ljava/util/function/Supplier;") => rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", a(2), &[]),
                _ => a(2),
            };
            let class: &'static str = rt.static_key(desc.rsplit(')').next().unwrap_or("").trim_start_matches('L').trim_end_matches(';'));
            let v = rt.new_object(class);
            rt.put_field(v, "$value", default);
            v
        }
        (CONFIG_BUILDER, _) => a(0), // comment, push, pop, translation, worldRestart...
        (o, n) if o.starts_with("net/neoforged/neoforge/common/ModConfigSpec$") && o.ends_with("Value") => {
            let v = rt.get_field(a(0), "$value");
            match n {
                "get" | "getDefault" | "getRaw" => v,
                "getAsInt" | "getAsLong" | "getAsDouble" | "getAsBoolean" => rt.get_field(v, "value"),
                "set" => {
                    rt.put_field(a(0), "$value", a(1));
                    V::Void
                }
                "save" | "clearCache" => V::Void,
                _ => return None,
            }
        }
        (MOD_CONTAINER, "registerConfig") => {
            let file = if desc_params(desc).len() > 2 { a(3) } else { V::NULL };
            register_config(rt, a(0), a(1), a(2), file);
            V::Void
        }
        (MOD_CONTAINER, "registerExtensionPoint") => V::Void,
        // A mod's config as FML keeps it, and the event its loading posts.
        (MOD_CONFIG, "getType") => rt.get_field(a(0), "$type"),
        (MOD_CONFIG, "getSpec") => rt.get_field(a(0), "$spec"),
        (MOD_CONFIG, "getModId") => rt.get_field(a(0), "$modId"),
        (MOD_CONFIG, "getFileName") => rt.get_field(a(0), "$fileName"),
        (o, "getConfig") if o.starts_with(CONFIG_EVENT) => rt.get_field(a(0), "$config"),
        // Its values are there from the start (the defaults): loaded.
        ("net/neoforged/neoforge/common/ModConfigSpec", "isLoaded") => V::I(1),
        (MOD_CONTAINER, "getCustomExtension") => crate::api_common::optional(rt, V::NULL),
        (MOD_CONTAINER, "getModInfo") => {
            let id = rt.get_field(a(0), "$modId");
            let id = rt.str_of(id).unwrap_or("").to_owned();
            mod_info(rt, &id)
        }
        // A mod's metadata, from its `neoforge.mods.toml` (the translator records each mod's
        // version and name); FML's defaults where the file has none: version `1`, the id as name.
        (MOD_INFO, "getModId" | "getNamespace") => rt.get_field(a(0), "$modId"),
        (MOD_INFO, "getDisplayName") => rt.get_field(a(0), "$displayName"),
        (MOD_INFO, "getVersion") => rt.get_field(a(0), "$version"),
        // What a mod's `mods.toml` adds past its id, name and version, which the engine doesn't
        // carry: no properties, no description, no update URL.
        (MOD_INFO, "getModProperties") => rt.alloc("java/util/HashMap", crate::Data::Table(Default::default())),
        (MOD_INFO, "getDescription") => rt.string(""),
        (MOD_INFO, "getUpdateURL" | "getLogoFile" | "getModURL") => crate::api_common::optional(rt, V::NULL),
        (MOD_INFO, "getDependencies") => crate::jutil::new_list(rt, Vec::new()),
        (VERSION, "toString") => rt.get_field(a(0), "$version"),
        (VERSION, "getMajorVersion" | "getMinorVersion" | "getIncrementalVersion") => {
            let text = rt.get_field(a(0), "$version");
            let i = ["getMajorVersion", "getMinorVersion", "getIncrementalVersion"].iter().position(|n| *n == name).unwrap_or(0);
            V::I(version_part(rt.str_of(text).unwrap_or(""), i))
        }
        // The loading context: during a mod's construction (and after it, the last one built) its
        // container is the active one.
        (LOADING_CONTEXT, "get") => rt.intern("compat:loading-context".into(), |rt| rt.new_object(LOADING_CONTEXT)),
        (LOADING_CONTEXT, "getActiveContainer") => active_container(rt),
        (LOADING_CONTEXT, "getActiveNamespace") => {
            let c = active_container(rt);
            rt.get_field(c, "$modId")
        }
        (LOADING_CONTEXT, "registerConfig") => {
            let file = if desc_params(desc).len() > 2 { a(3) } else { V::NULL };
            let container = active_container(rt);
            register_config(rt, container, a(1), a(2), file);
            V::Void
        }
        (LOADING_CONTEXT, "registerExtensionPoint") => V::Void,
        // Kotlin for Forge's loading context: a Kotlin mod's constructor asks it for the mod bus
        // (`MOD_BUS`), as a Java mod's constructor takes the bus.
        (KOTLIN_CONTEXT_COMPANION, "get") => rt.intern("compat:kotlin-loading-context".into(), |rt| rt.new_object(KOTLIN_CONTEXT)),
        (KOTLIN_CONTEXT, "getKEventBus") => rt.new_object("net/neoforged/bus/api/IEventBus"),
        // The mods this engine carries, as FML lists them: the jar's own and those bundled in it.
        ("net/neoforged/fml/ModList", "get") => rt.intern("compat:mod-list".into(), |rt| rt.new_object("net/neoforged/fml/ModList")),
        ("net/neoforged/fml/ModList", "isLoaded") => {
            let id = rt.str_of(a(1)).unwrap_or("").to_owned();
            V::I(mod_loaded(rt, &id) as i32)
        }
        ("net/neoforged/fml/ModList", "getModContainerById") => {
            let id = rt.str_of(a(1)).unwrap_or("").to_owned();
            let c = if mod_loaded(rt, &id) { mod_container(rt, &id) } else { V::NULL };
            crate::api_common::optional(rt, c)
        }
        ("net/neoforged/fml/ModList", "size") => V::I(loaded_mods(rt).len() as i32),
        // Each loaded mod's id and container, in FML's order: `forEachModContainer(BiConsumer)`,
        // `forEachModInOrder(Consumer)` (containers), `applyForEachModContainer(Function)` (a stream
        // of the answers), `getSortedMods()` (the containers).
        ("net/neoforged/fml/ModList", "forEachModContainer" | "forEachModInOrder" | "applyForEachModContainer" | "getSortedMods") => {
            let mut out = Vec::new();
            for id in loaded_mods(rt) {
                let c = mod_container(rt, &id);
                match name {
                    "forEachModContainer" => {
                        let s = rt.string(&id);
                        rt.call_functional("java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V", a(1), &[s, c]);
                    }
                    "forEachModInOrder" => {
                        rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[c]);
                    }
                    "applyForEachModContainer" => out.push(rt.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(1), &[c])),
                    _ => out.push(c),
                }
                if rt.thrown.is_some() {
                    break;
                }
            }
            match name {
                "applyForEachModContainer" => rt.alloc("java/util/stream/Stream", Data::Stream(out)),
                "getSortedMods" => crate::jutil::new_list(rt, out),
                _ => V::Void,
            }
        }
        // What FML's scan of each jar found: its annotations ([`crate::scan`]).
        ("net/neoforged/fml/ModList", "getAllScanData") => crate::scan::all(rt),
        // `getMods()`: each loaded mod's metadata, in FML's order.
        ("net/neoforged/fml/ModList", "getMods") => {
            let infos: Vec<V> = loaded_mods(rt).iter().map(|id| mod_info(rt, id)).collect();
            crate::jutil::new_list(rt, infos)
        }
        // The game runs, it isn't generating data: data generators stay unregistered.
        ("net/neoforged/neoforge/data/loading/DatagenModLoader", "isRunningDataGen") => V::I(0),
        // A data generator's registry set (datapack entries a mod's datagen writes): built up, never run.
        (REGISTRY_SET_BUILDER, "<init>") => V::Void,
        // Data map types (`DataMapType.builder(id, registry, codec).synced(...).build()`): an id, its
        // registry and its codec, as NeoForge's own (`NeoForgeDataMaps.FURNACE_FUELS`) are.
        (DATA_MAP_TYPE | ADVANCED_DATA_MAP_TYPE, "builder") => {
            let b = rt.new_object(DATA_MAP_BUILDER);
            rt.put_field(b, "$id", a(0));
            rt.put_field(b, "$registry", a(1));
            rt.put_field(b, "$codec", a(2));
            b
        }
        (DATA_MAP_BUILDER | ADVANCED_DATA_MAP_BUILDER, "synced") => {
            rt.put_field(a(0), "$networkCodec", a(1));
            rt.put_field(a(0), "$mandatory", a(2));
            a(0)
        }
        (DATA_MAP_BUILDER | ADVANCED_DATA_MAP_BUILDER, "build") => {
            let t = rt.new_object(DATA_MAP_TYPE);
            for f in ["$id", "$registry", "$codec", "$networkCodec", "$mandatory"] {
                let v = rt.get_field(a(0), f);
                rt.put_field(t, f, v);
            }
            t
        }
        (DATA_MAP_BUILDER | ADVANCED_DATA_MAP_BUILDER, _) if desc.ends_with(")Lnet/neoforged/neoforge/registries/datamaps/DataMapType$Builder;") || desc.ends_with(")Lnet/neoforged/neoforge/registries/datamaps/AdvancedDataMapType$Builder;") => a(0),
        (DATA_MAP_TYPE | ADVANCED_DATA_MAP_TYPE, "id") => rt.get_field(a(0), "$id"),
        (DATA_MAP_TYPE | ADVANCED_DATA_MAP_TYPE, "registryKey") => rt.get_field(a(0), "$registry"),
        (DATA_MAP_TYPE | ADVANCED_DATA_MAP_TYPE, "codec") => rt.get_field(a(0), "$codec"),
        (DATA_MAP_TYPE | ADVANCED_DATA_MAP_TYPE, "mandatorySync") => V::I((rt.get_field(a(0), "$mandatory").i() != 0) as i32),
        // Command argument types a mod adds: registered by their class, answered by the info.
        (ARGUMENT_TYPE_INFOS, "registerByClass") => {
            let all = rt.intern("compat:argument-type-infos".into(), |rt| rt.alloc("java/util/HashMap", crate::Data::Table(Default::default())));
            crate::jutil::table_put(rt, all, a(0), a(1));
            a(1)
        }
        (SINGLETON_ARGUMENT_INFO, "contextFree" | "contextAware") => {
            let info = rt.new_object(SINGLETON_ARGUMENT_INFO);
            rt.put_field(info, "$constructor", a(0));
            rt.put_field(info, "$contextAware", V::I((name == "contextAware") as i32));
            info
        }
        (REGISTRY_SET_BUILDER, "add") => a(0),
        // What FML found before loading: a mod's file, or null for one that isn't there (how mods
        // check for an optional one early).
        (LOADING_MOD_LIST, "get") => rt.intern("compat:loading-mod-list".into(), |rt| rt.new_object(LOADING_MOD_LIST)),
        (LOADING_MOD_LIST, "getModFileById") => {
            let id = rt.str_of(a(1)).unwrap_or("").to_owned();
            if mod_loaded(rt, &id) {
                rt.intern(format!("compat:mod-file/{id}"), |rt| {
                    let f = rt.new_object(MOD_FILE_INFO);
                    let s = rt.string(&id);
                    rt.put_field(f, "$modId", s);
                    f
                })
            } else {
                V::NULL
            }
        }
        // A jar's mods: the one asked for by id, or all a scanned jar declares (`$modIds`).
        (MOD_FILE_INFO, "getMods") => {
            let ids = match rt.str_of(rt.get_field(a(0), "$modIds")) {
                Some(all) => all.to_owned(),
                None => rt.str_of(rt.get_field(a(0), "$modId")).unwrap_or("").to_owned(),
            };
            let infos: Vec<V> = ids.split(',').filter(|i| !i.is_empty()).map(|id| mod_info(rt, id)).collect();
            crate::jutil::new_list(rt, infos)
        }
        (MOD_CONTAINER, "getModId" | "getNamespace") => rt.get_field(a(0), "$modId"),
        // Events route by class, so every handle on the mod bus is the same bus.
        (MOD_CONTAINER, "getEventBus") => rt.new_object("net/neoforged/bus/api/IEventBus"),
        // A setup event's deferred work runs at once (one thread here), and its future is done.
        (o, "enqueueWork") if o.starts_with("net/neoforged/fml/event/lifecycle/") => {
            let value = if desc.starts_with("(Ljava/lang/Runnable;") {
                rt.call_functional("java/lang/Runnable.run.()V", a(1), &[]);
                V::NULL
            } else {
                rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", a(1), &[])
            };
            crate::jutil::completed_future(rt, value)
        }
        // `ItemStack.canPerformAction(ability)`: its item's answer (`IItemExtension`), the mod
        // item's own where its class has one.
        (ITEM_STACK, "canPerformAction") => {
            let item = crate::api::call(rt, "net/minecraft/world/item/ItemStack.getItem.()Lnet/minecraft/world/item/Item;", &[a(0)]);
            let args = [item, a(0), a(1)];
            match rt.vcall {
                Some(vc) => vc(rt, ITEM_CAN_PERFORM, CAN_PERFORM, &args),
                None => crate::api::call(rt, ITEM_CAN_PERFORM, &args),
            }
        }
        // `Item.canPerformAction(stack, ability)` (a call through `super` names a tool's class):
        // the abilities of the item's vanilla class.
        (o, "canPerformAction") if o.starts_with("net/minecraft/world/item/") && desc == &CAN_PERFORM["canPerformAction.".len()..] => {
            let class = if o == ITEM { item_class(rt, a(0)) } else { o.rsplit('/').next().unwrap_or(o) };
            let ability = rt.str_of(rt.get_field(a(2), "name")).unwrap_or("").to_owned();
            V::I(default_abilities(class).contains(&ability.as_str()) as i32)
        }
        // A lifecycle event's mod: FML dispatches one to each container, here the runtime's own.
        (o, "getContainer") if o.starts_with("net/neoforged/fml/event/lifecycle/") => active_container(rt),
        // `InterModComms.sendTo([sender,] modId, method, supplier)`: queued for a loaded mod only.
        (IMC, "sendTo") => {
            let four = desc.starts_with("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;");
            let (sender, rest) = if four {
                (a(0), &args[1..])
            } else {
                let c = active_container(rt);
                (rt.get_field(c, "$modId"), args)
            };
            let arg = |i: usize| rest.get(i).copied().unwrap_or_default();
            V::I(send_imc(rt, sender, arg(0), arg(1), arg(2)) as i32)
        }
        // `getMessages(modId[, methodFilter])`, and a lifecycle event's `getIMCStream([filter])`
        // for its mod: the messages sent to it, taken off its queue as the stream reaches them.
        (IMC, "getMessages") => {
            let id = rt.str_of(a(0)).unwrap_or("").to_owned();
            take_imc(rt, &id, args.get(1).copied())
        }
        (o, "getIMCStream") if o.starts_with("net/neoforged/fml/event/lifecycle/") => {
            let c = active_container(rt);
            let id = rt.str_of(rt.get_field(c, "$modId")).unwrap_or("").to_owned();
            take_imc(rt, &id, args.get(1).copied())
        }
        (IMC_MESSAGE, n) => match n {
            "senderModId" | "getSenderModId" => rt.get_field(a(0), "senderModId"),
            "modId" | "getModId" => rt.get_field(a(0), "modId"),
            "method" | "getMethod" => rt.get_field(a(0), "method"),
            "messageSupplier" | "getMessageSupplier" => rt.get_field(a(0), "messageSupplier"),
            _ => return None,
        },
        _ => return None,
    })
}

/// Set while the server runs: from `ServerAboutToStartEvent` until `ServerStoppedEvent`.
const SERVER_RUNNING: &str = "compat:server-running";
const SERVER_EVENTS: &str = "net/neoforged/neoforge/event/server/";
const LEVEL_UNLOAD: &str = "net/neoforged/neoforge/event/level/LevelEvent$Unload";

/// The server: one object while it runs.
pub(crate) fn server(rt: &mut Rt) -> V {
    rt.intern("compat:server".into(), |rt| rt.new_object("net/minecraft/server/MinecraftServer"))
}

/// Posts the server lifecycle event `name` (`ServerStartingEvent`) about the server.
fn server_event(rt: &mut Rt, name: &str) {
    let class: &'static str = rt.static_key(&format!("{SERVER_EVENTS}{name}"));
    let ev = rt.new_object(class);
    let s = server(rt);
    rt.put_field(ev, "$server", s);
    if Rt::tracing() {
        eprintln!("horizon-compat: {name} to {} listeners", hearing(rt, class).len());
    }
    post(rt, class, ev);
}

/// NeoForge's `ServerAboutToStartEvent`: the server is there before it loads anything.
pub(crate) fn server_about_to_start(rt: &mut Rt) {
    rt.interned.insert(SERVER_RUNNING.into(), V::I(1));
    server_event(rt, "ServerAboutToStartEvent");
}

/// The world open: `ServerStartingEvent`, then `ServerStartedEvent`, as `MinecraftServer.runServer`
/// posts them before its first tick.
pub(crate) fn server_started(rt: &mut Rt) {
    rt.interned.insert(SERVER_RUNNING.into(), V::I(1));
    server_event(rt, "ServerStartingEvent");
    server_event(rt, "ServerStartedEvent");
}

/// NeoForge's `ServerStoppingEvent`, with the worlds still loaded.
pub(crate) fn server_stopping(rt: &mut Rt) {
    server_event(rt, "ServerStoppingEvent");
}

/// The server gone: its level unloads (`LevelEvent.Unload`), then `ServerStoppedEvent`; the next
/// world gets a new server and level (a new `LevelEvent.Load`), and `getCurrentServer()` is null
/// until then.
pub(crate) fn server_stopped(rt: &mut Rt) {
    if !rt.interned.contains_key(SERVER_RUNNING) && !rt.interned.contains_key("compat:server") {
        return;
    }
    if let Some(&level) = rt.interned.get("compat:level") {
        let ev = rt.new_object(LEVEL_UNLOAD);
        rt.put_field(ev, "$level", level);
        if Rt::tracing() {
            eprintln!("horizon-compat: LevelEvent$Unload to {} listeners", hearing(rt, LEVEL_UNLOAD).len());
        }
        post(rt, LEVEL_UNLOAD, ev);
    }
    server_event(rt, "ServerStoppedEvent");
    // The server's commands went with it (`crate::commands`).
    for key in [SERVER_RUNNING, "compat:server", "compat:level", "compat:commands"] {
        rt.interned.remove(key);
    }
}

/// The messages sent to mod `id` and not yet read, in the order they were sent.
fn imc_queue(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:imc/{id}"), |rt| rt.alloc("java/util/concurrent/ConcurrentLinkedQueue", Data::List(Vec::new())))
}

/// `InterModComms.sendTo`: false, and nothing queued, when mod `to` isn't loaded.
fn send_imc(rt: &mut Rt, sender: V, to: V, method: V, thing: V) -> bool {
    let target = rt.str_of(to).unwrap_or("").to_owned();
    if !mod_loaded(rt, &target) {
        return false;
    }
    let m = rt.new_object(IMC_MESSAGE);
    for (field, v) in [("senderModId", sender), ("modId", to), ("method", method), ("messageSupplier", thing)] {
        rt.put_field(m, field, v);
    }
    let q = imc_queue(rt, &target);
    list_mut(rt, q).push(m);
    true
}

/// The messages to mod `id` whose method `filter` accepts (all without one), off its queue.
fn take_imc(rt: &mut Rt, id: &str, filter: Option<V>) -> V {
    let q = imc_queue(rt, id);
    let queued = std::mem::take(list_mut(rt, q));
    let (mut taken, mut kept) = (Vec::new(), Vec::new());
    for m in queued {
        let wanted = match filter {
            Some(f) if f.r() != 0 => {
                let method = rt.get_field(m, "method");
                rt.call_functional(PREDICATE_TEST, f, &[method]).i() != 0
            }
            _ => true,
        };
        if wanted { taken.push(m) } else { kept.push(m) }
    }
    *list_mut(rt, q) = kept;
    rt.alloc("java/util/stream/Stream", Data::Stream(taken))
}

const COMMON_SETUP: &str = "net/neoforged/fml/event/lifecycle/FMLCommonSetupEvent";
const ENQUEUE_IMC: &str = "net/neoforged/fml/event/lifecycle/InterModEnqueueEvent";
const PROCESS_IMC: &str = "net/neoforged/fml/event/lifecycle/InterModProcessEvent";
const LOAD_COMPLETE: &str = "net/neoforged/fml/event/lifecycle/FMLLoadCompleteEvent";
const IMC: &str = "net/neoforged/fml/InterModComms";
const IMC_MESSAGE: &str = "net/neoforged/fml/InterModComms$IMCMessage";
const PREDICATE_TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const CONSTRUCT: &str = "net/neoforged/fml/event/lifecycle/FMLConstructModEvent";
const CLIENT_SETUP: &str = "net/neoforged/fml/event/lifecycle/FMLClientSetupEvent";
const REGISTER_RENDERERS: &str = "net/neoforged/neoforge/client/event/EntityRenderersEvent$RegisterRenderers";

/// `FMLCommonSetupEvent`, once the registries are filled, as NeoForge's loading fires it on both
/// sides.
pub fn fire_common_setup(rt: &mut Rt) {
    let ev = rt.new_object(COMMON_SETUP);
    post(rt, COMMON_SETUP, ev);
}

/// The client's own setup, once, on the runtime that draws: `EntityRenderersEvent.RegisterRenderers`
/// and `FMLClientSetupEvent` (Registrate registers block entity renderers there).
pub fn fire_client_setup(rt: &mut Rt) {
    if rt.interned.contains_key("compat:client") {
        return;
    }
    rt.interned.insert("compat:client".into(), V::I(1));
    // The mods' reload listeners, as the client's resource manager is made.
    let ev = crate::resources::client_listeners_event(rt);
    post(rt, crate::resources::CLIENT_LISTENERS_EVENT, ev);
    let ev = rt.new_object(REGISTER_RENDERERS);
    post(rt, REGISTER_RENDERERS, ev);
    // The mods' key mappings (`ClientHooks.initClientHooks` posts it after the renderers).
    crate::keys::post_register(rt);
    let ev = rt.new_object(CLIENT_SETUP);
    post(rt, CLIENT_SETUP, ev);
    // The mods' menu types get their screens (`MenuScreens.register` through the event).
    crate::menus::register_screens(rt);
    // The HUD's layers: vanilla's, and where each mod puts its own among them.
    let ev = rt.new_object(crate::gui::REGISTER_LAYERS);
    post(rt, crate::gui::REGISTER_LAYERS, ev);
    // The block atlas is stitched (catnip's `StitchedSprite`s take their sprites from it: Create's
    // scrolling belts).
    let atlas = crate::render::block_atlas(rt);
    let ev = rt.new_object(crate::render::ATLAS_STITCHED);
    rt.put_field(ev, "$atlas", atlas);
    post(rt, crate::render::ATLAS_STITCHED, ev);
}

/// Whether this runtime is the client's (its setup has fired): its level is a client level.
pub fn is_client(rt: &Rt) -> bool {
    rt.interned.contains_key("compat:client")
}

const CONFIG_BUILDER: &str = "net/neoforged/neoforge/common/ModConfigSpec$Builder";
const MOD_CONFIG: &str = "net/neoforged/fml/config/ModConfig";
const CONFIG_EVENT: &str = "net/neoforged/fml/event/config/ModConfigEvent";
const CONFIG_LOADING: &str = "net/neoforged/fml/event/config/ModConfigEvent$Loading";
/// The configs registered and not loaded yet ([`fire_config_loading`]).
const CONFIGS: &str = "compat:configs";

/// `ModContainer.registerConfig`: the mod's `ModConfig` (its type, spec and file, FML's
/// `<mod id>-<type>.toml` unless named), loaded with the others once the registries are
/// ([`fire_config_loading`]); a `STARTUP` config loads at once, as FML opens it there.
fn register_config(rt: &mut Rt, container: V, ty: V, spec: V, file: V) {
    let id = rt.get_field(container, "$modId");
    let kind = rt.str_of(rt.get_field(ty, "$name")).unwrap_or("COMMON").to_ascii_lowercase();
    let file = if file.r() != 0 {
        file
    } else {
        let name = format!("{}-{kind}.toml", rt.str_of(id).unwrap_or(""));
        rt.string(&name)
    };
    let config = rt.new_object(MOD_CONFIG);
    for (field, v) in [("$type", ty), ("$spec", spec), ("$modId", id), ("$fileName", file), ("$container", container)] {
        rt.put_field(config, field, v);
    }
    if kind == "startup" {
        load_config(rt, config);
        return;
    }
    let pending = rt.intern(CONFIGS.into(), |rt| crate::jutil::new_list(rt, Vec::new()));
    crate::jutil::list_add(rt, pending, config);
}

/// A config's loading: `ModConfigEvent.Loading` to the mod bus, heard by a listener for it or for
/// `ModConfigEvent` (the MDK's `Config.onLoad` copies the values into its fields there).
fn load_config(rt: &mut Rt, config: V) {
    for (class, sup) in [
        (CONFIG_LOADING, CONFIG_EVENT),
        ("net/neoforged/fml/event/config/ModConfigEvent$Reloading", CONFIG_EVENT),
        ("net/neoforged/fml/event/config/ModConfigEvent$Unloading", CONFIG_EVENT),
        (CONFIG_EVENT, "net/neoforged/bus/api/Event"),
    ] {
        rt.supers.insert(class, sup);
    }
    let ev = rt.new_object(CONFIG_LOADING);
    rt.put_field(ev, "$config", config);
    post(rt, CONFIG_LOADING, ev);
}

/// NeoForge's "Config loading" step, once the registries are frozen and before common setup: each
/// registered config loads, in the order registered. Horizon keeps no config files for translated
/// mods, so the values are the defaults; a server's configs load with the rest (NeoForge loads
/// them as its server starts).
fn fire_config_loading(rt: &mut Rt) {
    let Some(&pending) = rt.interned.get(CONFIGS) else {
        return;
    };
    let configs = std::mem::take(list_mut(rt, pending));
    for config in configs {
        load_config(rt, config);
    }
}

/// A `java.lang` box of `v`.
fn boxed(rt: &mut Rt, class: &'static str, v: V) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// A method descriptor's parameter types, as written (`I`, `Ljava/lang/String;`, `[I`...).
pub(crate) fn desc_params(desc: &str) -> Vec<String> {
    let inner = desc.trim_start_matches('(').split(')').next().unwrap_or("");
    let (mut out, mut cur) = (Vec::new(), String::new());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        cur.push(c);
        match c {
            '[' => continue,
            'L' => {
                for d in chars.by_ref() {
                    cur.push(d);
                    if d == ';' {
                        break;
                    }
                }
            }
            _ => {}
        }
        out.push(std::mem::take(&mut cur));
    }
    out
}

/// The mod ids FML would list for this engine: its `@Mod`s (or, from an engine translated before
/// they were listed, the jar's own namespace).
pub(crate) fn loaded_mods(rt: &Rt) -> Vec<String> {
    if rt.mods.is_empty() && !rt.namespace.is_empty() { vec![rt.namespace.clone()] } else { rt.mods.clone() }
}

fn mod_loaded(rt: &Rt, id: &str) -> bool {
    !id.is_empty() && loaded_mods(rt).iter().any(|m| m == id)
}

/// NeoForge's container of a Java mod: what a mod's constructor, `ModList` and `ModLoadingContext`
/// hand out (mods check `instanceof FMLModContainer` before asking it for their bus).
pub const FML_CONTAINER: &str = "net/neoforged/fml/javafmlmod/FMLModContainer";
const MOD_CONTAINER: &str = "net/neoforged/fml/ModContainer";
const MOD_INFO: &str = "net/neoforged/fml/loading/moddiscovery/ModInfo";
const VERSION: &str = "org/apache/maven/artifact/versioning/DefaultArtifactVersion";
const LOADING_CONTEXT: &str = "net/neoforged/fml/ModLoadingContext";
const LOADING_MOD_LIST: &str = "net/neoforged/fml/loading/LoadingModList";
const MOD_FILE_INFO: &str = "net/neoforged/fml/loading/moddiscovery/ModFileInfo";
/// The interned string naming the mod whose constructor runs (the generated `start` sets it).
pub const ACTIVE_MOD: &str = "compat:active-mod";
const REGISTRY_SET_BUILDER: &str = "net/minecraft/core/RegistrySetBuilder";
const DATA_MAP_TYPE: &str = "net/neoforged/neoforge/registries/datamaps/DataMapType";
const DATA_MAP_BUILDER: &str = "net/neoforged/neoforge/registries/datamaps/DataMapType$Builder";
const ADVANCED_DATA_MAP_TYPE: &str = "net/neoforged/neoforge/registries/datamaps/AdvancedDataMapType";
const ADVANCED_DATA_MAP_BUILDER: &str = "net/neoforged/neoforge/registries/datamaps/AdvancedDataMapType$Builder";
const ARGUMENT_TYPE_INFOS: &str = "net/minecraft/commands/synchronization/ArgumentTypeInfos";
const SINGLETON_ARGUMENT_INFO: &str = "net/minecraft/commands/synchronization/SingletonArgumentInfo";
/// NeoForge's own data maps (`NeoForgeDataMaps`), by field: their ids and registries.
const NEOFORGE_DATA_MAPS: [(&str, &str); 8] = [
    ("COMPOSTABLES", "item"),
    ("FURNACE_FUELS", "item"),
    ("MONSTER_ROOM_MOBS", "entity_type"),
    ("OXIDIZABLES", "block"),
    ("PARROT_IMITATIONS", "entity_type"),
    ("RAID_HERO_GIFTS", "villager_profession"),
    ("VIBRATION_FREQUENCIES", "game_event"),
    ("WAXABLES", "block"),
];
const KOTLIN_CONTEXT: &str = "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext";
const KOTLIN_CONTEXT_COMPANION: &str = "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext$Companion";

/// The loader's static fields: Kotlin for Forge's loading context's companion (`KotlinModLoadingContext.get()`
/// in Kotlin is a call on it).
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if let Some(v) = crate::scan::get_static(rt, owner, field) {
        return Some(v);
    }
    match (owner, field) {
        ("cpw/mods/modlauncher/Launcher", "INSTANCE") => Some(rt.intern("compat:modlauncher".into(), |rt| rt.new_object("cpw/mods/modlauncher/Launcher"))),
        // `IEnvironment.Keys.VERSION`: a supplier of the property's key.
        ("cpw/mods/modlauncher/api/IEnvironment$Keys", f) => {
            let name = f.to_ascii_lowercase();
            Some(rt.intern(format!("compat:modlauncher-key/{name}"), move |rt| {
                let key = rt.new_object("cpw/mods/modlauncher/api/TypesafeMap$Key");
                let n = rt.string(&name);
                rt.put_field(key, "$name", n);
                crate::functions::constant(rt, key)
            }))
        }
        // NeoForge's abilities: each constant's name lower-cased, but `HOE_TILL`'s is `till`.
        ("net/neoforged/neoforge/common/ItemAbilities", f) if !f.starts_with("DEFAULT_") => {
            let name = if f == "HOE_TILL" { "till".to_owned() } else { f.to_ascii_lowercase() };
            Some(item_ability(rt, &name))
        }
        (KOTLIN_CONTEXT | "thedarkcolour/kotlinforforge/KotlinModLoadingContext", "Companion") => {
            Some(rt.intern("compat:kotlin-loading-context-companion".into(), |rt| rt.new_object(KOTLIN_CONTEXT_COMPANION)))
        }
        ("net/neoforged/neoforge/registries/datamaps/builtin/NeoForgeDataMaps", f) => {
            let &(_, registry) = NEOFORGE_DATA_MAPS.iter().find(|(n, _)| *n == f)?;
            let id = format!("neoforge:{}", f.to_ascii_lowercase());
            Some(rt.intern(format!("compat:data-map/{id}"), |rt| {
                let t = rt.new_object(DATA_MAP_TYPE);
                let i = rt.string(&id);
                rt.put_field(t, "$id", i);
                let r = rt.string(registry);
                rt.put_field(t, "$registry", r);
                t
            }))
        }
        // `RegistrationInfo.BUILT_IN`: a built-in entry's (no known pack, stable).
        ("net/minecraft/core/RegistrationInfo", "BUILT_IN") => Some(rt.intern("compat:registration-info/built-in".into(), |rt| {
            let info = rt.new_object("net/minecraft/core/RegistrationInfo");
            let none = crate::api_common::optional(rt, V::NULL);
            rt.put_field(info, "knownPackInfo", none);
            let stable = rt.new_object("com/mojang/serialization/Lifecycle");
            rt.put_field(info, "lifecycle", stable);
            info
        })),
        _ => None,
    }
}

/// The mod's container, one per id.
fn mod_container(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:mod-container/{id}"), |rt| {
        let c = rt.new_object(FML_CONTAINER);
        let s = rt.string(id);
        rt.put_field(c, "$modId", s);
        c
    })
}

/// The container of the mod being constructed: the one `start` named, else the engine's first.
fn active_container(rt: &mut Rt) -> V {
    let named = rt.interned.get(ACTIVE_MOD).copied().and_then(|v| rt.str_of(v).map(str::to_owned));
    let id = named.or_else(|| loaded_mods(rt).into_iter().next()).unwrap_or_default();
    mod_container(rt, &id)
}

/// A mod's `IModInfo`, one per id, with the version and name the translator found for it.
fn mod_info(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:mod-info/{id}"), |rt| {
        let info = rt.new_object(MOD_INFO);
        let s = rt.string(id);
        rt.put_field(info, "$modId", s);
        let name = rt.interned.get(&format!("compat:mod-name/{id}")).copied().unwrap_or(s);
        rt.put_field(info, "$displayName", name);
        let text = match rt.interned.get(&format!("compat:mod-version/{id}")).copied() {
            Some(v) => v,
            None => rt.string("1"),
        };
        let version = rt.new_object(VERSION);
        rt.put_field(version, "$version", text);
        rt.put_field(info, "$version", version);
        info
    })
}

/// Part `i` (major, minor, incremental) of a version as Maven reads it: the leading digits of each
/// dot-separated piece, 0 where there are none.
fn version_part(text: &str, i: usize) -> i32 {
    let piece = text.split(['-', '+']).next().unwrap_or("").split('.').nth(i).unwrap_or("");
    piece.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}

/// Loading objects a mod's constructor takes: the event bus, its container, the side.
pub fn constructor_arg(rt: &mut Rt, class: &str) -> V {
    match class {
        "net/neoforged/bus/api/IEventBus" => rt.new_object("net/neoforged/bus/api/IEventBus"),
        MOD_CONTAINER | FML_CONTAINER => active_container(rt),
        "net/neoforged/api/distmarker/Dist" => crate::api::get_static(rt, "net/neoforged/api/distmarker/Dist.DEDICATED_SERVER"),
        _ => V::NULL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Data;

    fn location_call(rt: &mut Rt, method: &str, args: &[V]) -> V {
        let k = rt.static_key(&format!("net/minecraft/resources/ResourceLocation.{method}"));
        crate::api::call(rt, k, args)
    }

    /// Ids are checked as 1.21.1 checks them: `parse` throws for a character an id can't have,
    /// `tryParse` gives null and `read` an error result, each with vanilla's message; a command
    /// line's id is read over the characters `isAllowedInResourceLocation` allows (catnip's config
    /// path argument reads its own over them); ids compare by path first and make their keys.
    #[test]
    fn ids_are_checked_read_compared_and_keyed_as_vanilla_does() {
        let mut rt = Rt::new();
        let allowed = "isAllowedInResourceLocation.(C)Z";
        let chars: Vec<i32> = ['c', '!', ':', 'A', '/']
            .iter()
            .map(|&c| location_call(&mut rt, allowed, &[V::I(c as i32)]).i())
            .collect();
        assert_eq!(chars, [1, 0, 1, 0, 1]);
        assert_eq!(location_call(&mut rt, "validPathChar.(C)Z", &[V::I(':' as i32)]).i(), 0);
        assert_eq!(location_call(&mut rt, "validNamespaceChar.(C)Z", &[V::I('/' as i32)]).i(), 0);
        let s = rt.string("a/b.c");
        assert_eq!(location_call(&mut rt, "isValidPath.(Ljava/lang/String;)Z", &[s]).i(), 1);
        assert_eq!(location_call(&mut rt, "isValidNamespace.(Ljava/lang/String;)Z", &[s]).i(), 0);

        let parse = "parse.(Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;";
        let try_parse = "tryParse.(Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;";
        let bad = rt.string("Bad:x");
        assert_eq!(location_call(&mut rt, parse, &[bad]), V::NULL);
        let e = rt.thrown.take().expect("parse throws");
        assert_eq!(rt.class_of(e), "net/minecraft/ResourceLocationException");
        let m = rt.get_field(e, "message");
        assert_eq!(rt.str_of(m), Some("Non [a-z0-9_.-] character in namespace of location: Bad:x"));
        assert_eq!(location_call(&mut rt, try_parse, &[bad]), V::NULL);
        assert!(rt.thrown.is_none());
        let colon_first = rt.string(":stone");
        let v = location_call(&mut rt, try_parse, &[colon_first]);
        assert_eq!(rt.str_of(v), Some("minecraft:stone"));
        let two = rt.string("a:b:c");
        let r = location_call(&mut rt, "read.(Ljava/lang/String;)Lcom/mojang/serialization/DataResult;", &[two]);
        assert_eq!(
            crate::codecs::result_of(&mut rt, r),
            Err("Not a valid resource location: a:b:c Non [a-z0-9/._-] character in path of location: a:b:c".to_owned())
        );
        let dotted = rt.string("create.shaft");
        let v = location_call(&mut rt, "bySeparator.(Ljava/lang/String;C)Lnet/minecraft/resources/ResourceLocation;", &[dotted, V::I('.' as i32)]);
        assert_eq!(rt.str_of(v), Some("create:shaft"));

        // From a command line: the id and the cursor after it, or the error where it began.
        let read = "read.(Lcom/mojang/brigadier/StringReader;)Lnet/minecraft/resources/ResourceLocation;";
        let non_empty = "readNonEmpty.(Lcom/mojang/brigadier/StringReader;)Lnet/minecraft/resources/ResourceLocation;";
        let message = "com/mojang/brigadier/exceptions/CommandSyntaxException.getMessage.()Ljava/lang/String;";
        let reader = crate::brigadier::new_reader(&mut rt, &crate::brigadier::Reader::new("create:shaft rest"));
        let v = location_call(&mut rt, read, &[reader]);
        assert_eq!((rt.str_of(v), rt.get_field(reader, "$cursor")), (Some("create:shaft"), V::I(12)));
        for (line, method) in [(" x", non_empty), ("a:b:c d", read)] {
            let reader = crate::brigadier::new_reader(&mut rt, &crate::brigadier::Reader::new(line));
            assert_eq!(location_call(&mut rt, method, &[reader]), V::NULL);
            let e = rt.thrown.take().expect("the read throws");
            let m = crate::api::call(&mut rt, message, &[e]);
            assert_eq!(rt.str_of(m), Some("argument.id.invalid at position 0: <--[HERE]"), "{line}");
            assert_eq!(rt.get_field(reader, "$cursor"), V::I(0));
        }

        // Path first, then namespace; `compareNamespaced` the other way round.
        let (az, ba, aa) = (rt.string("a:z"), rt.string("b:a"), rt.string("a:a"));
        let compare = "compareTo.(Lnet/minecraft/resources/ResourceLocation;)I";
        let namespaced = "compareNamespaced.(Lnet/minecraft/resources/ResourceLocation;)I";
        assert_eq!(location_call(&mut rt, compare, &[az, ba]).i(), 25);
        assert_eq!(location_call(&mut rt, namespaced, &[az, ba]).i(), -1);
        assert_eq!(location_call(&mut rt, compare, &[ba, aa]).i(), 1);

        let shaft = rt.string("create:shaft");
        let stone = rt.string("minecraft:stone");
        let (block, tooltip) = (rt.string("block"), rt.string("tooltip"));
        let mut keys = Vec::new();
        for (method, args) in [
            ("toLanguageKey.(Ljava/lang/String;)Ljava/lang/String;", vec![shaft, block]),
            ("toLanguageKey.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", vec![shaft, block, tooltip]),
            ("toShortLanguageKey.()Ljava/lang/String;", vec![stone]),
            ("toShortLanguageKey.()Ljava/lang/String;", vec![shaft]),
            ("toDebugFileName.()Ljava/lang/String;", vec![rt.string("create:a/b")]),
        ] {
            let v = location_call(&mut rt, method, &args);
            keys.push(rt.str_of(v).unwrap_or("").to_owned());
        }
        assert_eq!(keys, ["block.create.shaft", "block.create.shaft.tooltip", "stone", "create.shaft", "create_a_b"]);
    }

    /// `ServerLifecycleHooks.getCurrentServer()` is null until the world's level is open (Mekanism's
    /// saved data asks for it from `LevelEvent.Load`), then the one server, whose overworld is that
    /// level; a client's runtime has none.
    #[test]
    fn the_current_server_is_there_once_the_world_is() {
        const CURRENT: &str = "net/neoforged/neoforge/server/ServerLifecycleHooks.getCurrentServer.()Lnet/minecraft/server/MinecraftServer;";
        let mut rt = Rt::new();
        assert_eq!(crate::api::call(&mut rt, CURRENT, &[]), V::NULL);
        let level = crate::glue::level(&mut rt);
        let server = crate::api::call(&mut rt, CURRENT, &[]);
        assert_eq!(rt.class_of(server), "net/minecraft/server/MinecraftServer");
        let overworld = crate::api::call(&mut rt, "net/minecraft/server/MinecraftServer.overworld.()Lnet/minecraft/server/level/ServerLevel;", &[server]);
        assert_eq!(overworld, level);
        let mut client = Rt::new();
        client.interned.insert("compat:client".into(), V::I(1));
        crate::glue::level(&mut client);
        assert_eq!(crate::api::call(&mut client, CURRENT, &[]), V::NULL);
    }

    /// The logical side code runs on is its runtime's: the server thread's until a client's setup
    /// has fired in it. A holder made for a key apart from registering (`createStandAlone`, its
    /// registry as owner) gives the entry once it is registered.
    #[test]
    fn effective_side_and_stand_alone_holders() {
        crate::install();
        let mut rt = Rt::new();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args);
        const GET: &str = "net/neoforged/fml/util/thread/EffectiveSide.get.()Lnet/neoforged/fml/LogicalSide;";
        const IS_SERVER: &str = "net/neoforged/fml/LogicalSide.isServer.()Z";
        let side = c(&mut rt, GET, &[]);
        assert_eq!(c(&mut rt, IS_SERVER, &[side]).i(), 1);
        rt.interned.insert("compat:client".into(), V::I(1));
        let side = c(&mut rt, GET, &[]);
        assert_eq!(c(&mut rt, IS_SERVER, &[side]).i(), 0);
        assert_eq!(c(&mut rt, "net/neoforged/fml/LogicalSide.isClient.()Z", &[side]).i(), 1);

        // The registry as the game's statics give it: its path, a string.
        let blocks = crate::api::get_static(&mut rt, "net/minecraft/core/registries/BuiltInRegistries.BLOCK");
        let registry = c(&mut rt, "net/minecraft/core/Registry.holderOwner.()Lnet/minecraft/core/HolderOwner;", &[blocks]);
        let key = rt.string("block#t:machine");
        let holder = c(&mut rt, "net/minecraft/core/Holder$Reference.createStandAlone.(Lnet/minecraft/core/HolderOwner;Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/Holder$Reference;", &[registry, key]);
        let machine = rt.new_object("t/Machine");
        rt.registered.insert(("block".into(), "t:machine".into()), machine);
        assert_eq!(c(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]), machine);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn the_side_is_one_constant_however_it_is_asked() {
        // catnip: `FMLLoader.getDist() == Dist.CLIENT ? Env.CLIENT : Env.SERVER`, by identity.
        let mut rt = Rt::new();
        let asked = call(&mut rt, "net/neoforged/fml/loading/FMLLoader.getDist.()Lnet/neoforged/api/distmarker/Dist;", &[]).unwrap();
        let client = crate::api::get_static(&mut rt, "net/neoforged/api/distmarker/Dist.CLIENT");
        assert_eq!(asked, client);
        assert_eq!(crate::api::get_static(&mut rt, "net/neoforged/fml/loading/FMLEnvironment.dist"), client);
        let server = crate::api::get_static(&mut rt, "net/neoforged/api/distmarker/Dist.DEDICATED_SERVER");
        assert_ne!(server, client);
        let is_client = "net/neoforged/api/distmarker/Dist.isClient.()Z";
        assert_eq!(call(&mut rt, is_client, &[client]), Some(V::I(1)));
        assert_eq!(call(&mut rt, is_client, &[server]), Some(V::I(0)));
    }

    /// Through the dispatcher, as a translated mod calls: a Kotlin mod asks Kotlin for Forge's
    /// loading context for its bus and listens on it; `ModList.getMods()` lists every mod; the game
    /// isn't generating data; NeoForge's data maps are its own ids.
    #[test]
    fn kotlin_mods_reach_their_bus_and_the_loading_calls_answer() {
        let mut rt = Rt::new();
        rt.mods = vec!["lib".into(), "kmod".into()];
        let companion = crate::api::get_static(&mut rt, "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext.Companion");
        let ctx = crate::api::call(&mut rt, "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext$Companion.get.()Lthedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext;", &[companion]);
        assert_ne!(ctx, V::NULL);
        let bus = crate::api::call(&mut rt, "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext.getKEventBus.()Lnet/neoforged/bus/api/IEventBus;", &[ctx]);
        assert!(rt.instance_of(bus, "net/neoforged/bus/api/IEventBus"));
        let event = rt.class_literal("net/neoforged/fml/event/lifecycle/FMLCommonSetupEvent");
        let listener = rt.new_object("test/Listener");
        let before = rt.listeners.len();
        crate::api::call(&mut rt, "net/neoforged/bus/api/IEventBus.addListener.(Ljava/lang/Class;Ljava/util/function/Consumer;)V", &[bus, event, listener]);
        assert_eq!(rt.listeners.len(), before + 1);
        let list = crate::api::call(&mut rt, "net/neoforged/fml/ModList.get.()Lnet/neoforged/fml/ModList;", &[]);
        let mods = crate::api::call(&mut rt, "net/neoforged/fml/ModList.getMods.()Ljava/util/List;", &[list]);
        let ids: Vec<String> = crate::jutil::elements(&rt, mods)
            .into_iter()
            .map(|info| {
                let id = crate::api::call(&mut rt, "net/neoforged/neoforgespi/language/IModInfo.getModId.()Ljava/lang/String;", &[info]);
                rt.str_of(id).unwrap_or("").to_owned()
            })
            .collect();
        assert_eq!(ids, ["lib", "kmod"]);
        let first = crate::jutil::elements(&rt, mods)[0];
        let props = crate::api::call(&mut rt, "net/neoforged/neoforgespi/language/IModInfo.getModProperties.()Ljava/util/Map;", &[first]);
        assert!(rt.instance_of(props, "java/util/Map"));
        assert!(crate::jutil::table_pairs(&rt, props).is_empty());
        // `Holder.direct(value)` holds the value itself.
        let v = rt.string("anything");
        let h = crate::api::call(&mut rt, "net/minecraft/core/Holder.direct.(Ljava/lang/Object;)Lnet/minecraft/core/Holder;", &[v]);
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[h]), v);
        assert_eq!(crate::api::call(&mut rt, "net/neoforged/neoforge/data/loading/DatagenModLoader.isRunningDataGen.()Z", &[]), V::I(0));
        let fuels = crate::api::get_static(&mut rt, "net/neoforged/neoforge/registries/datamaps/builtin/NeoForgeDataMaps.FURNACE_FUELS");
        let id = crate::api::call(&mut rt, "net/neoforged/neoforge/registries/datamaps/DataMapType.id.()Lnet/minecraft/resources/ResourceLocation;", &[fuels]);
        assert_eq!(rt.str_of(id), Some("neoforge:furnace_fuels"));
        assert_eq!(crate::api::get_static(&mut rt, "net/neoforged/neoforge/registries/datamaps/builtin/NeoForgeDataMaps.FURNACE_FUELS"), fuels);
        // A mod's own: the builder's id survives `synced(...)` and `build()`.
        let mine = rt.string("kmod:heat");
        let b = crate::api::call(&mut rt, "net/neoforged/neoforge/registries/datamaps/DataMapType.builder.(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceKey;Lcom/mojang/serialization/Codec;)Lnet/neoforged/neoforge/registries/datamaps/DataMapType$Builder;", &[mine, V::NULL, V::NULL]);
        let b = crate::api::call(&mut rt, "net/neoforged/neoforge/registries/datamaps/DataMapType$Builder.synced.(Lcom/mojang/serialization/Codec;Z)Lnet/neoforged/neoforge/registries/datamaps/DataMapType$Builder;", &[b, V::NULL, V::I(1)]);
        let t = crate::api::call(&mut rt, "net/neoforged/neoforge/registries/datamaps/DataMapType$Builder.build.()Lnet/neoforged/neoforge/registries/datamaps/DataMapType;", &[b]);
        let id = crate::api::call(&mut rt, "net/neoforged/neoforge/registries/datamaps/DataMapType.id.()Lnet/minecraft/resources/ResourceLocation;", &[t]);
        assert_eq!(rt.str_of(id), Some("kmod:heat"));
    }

    /// FML hands a mod its own container while constructing it: a Java mod's (Comforts checks
    /// `instanceof FMLModContainer` before using its bus), the one `ModList` and the loading
    /// context give for that id, whose metadata has the version the mod's file declares, or FML's
    /// `1` where it declares none.
    #[test]
    fn a_mod_gets_its_own_fml_container_with_its_version() {
        crate::install();
        let mut rt = Rt::new();
        rt.mods = vec!["lib".into(), "comforts".into()];
        let v = rt.string("9.0.5+1.21.1");
        rt.interned.insert("compat:mod-version/comforts".into(), v);
        let id = rt.string("comforts");
        rt.interned.insert(ACTIVE_MOD.into(), id);
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"));
        let container = constructor_arg(&mut rt, "net/neoforged/fml/ModContainer");
        assert!(rt.instance_of(container, "net/neoforged/fml/javafmlmod/FMLModContainer"));
        assert!(rt.instance_of(container, "net/neoforged/fml/ModContainer"));
        let ctx = c(&mut rt, "net/neoforged/fml/ModLoadingContext.get.()Lnet/neoforged/fml/ModLoadingContext;", &[]);
        assert_eq!(c(&mut rt, "net/neoforged/fml/ModLoadingContext.getActiveContainer.()Lnet/neoforged/fml/ModContainer;", &[ctx]), container);
        let list = c(&mut rt, "net/neoforged/fml/ModList.get.()Lnet/neoforged/fml/ModList;", &[]);
        let by_id = |rt: &mut Rt, id: &str| {
            let id = rt.string(id);
            let found = c(rt, "net/neoforged/fml/ModList.getModContainerById.(Ljava/lang/String;)Ljava/util/Optional;", &[list, id]);
            crate::api::call(rt, "java/util/Optional.get.()Ljava/lang/Object;", &[found])
        };
        assert_eq!(by_id(&mut rt, "comforts"), container);
        let info = c(&mut rt, "net/neoforged/fml/javafmlmod/FMLModContainer.getModInfo.()Lnet/neoforged/neoforgespi/language/IModInfo;", &[container]);
        let version = c(&mut rt, "net/neoforged/neoforgespi/language/IModInfo.getVersion.()Lorg/apache/maven/artifact/versioning/ArtifactVersion;", &[info]);
        let text = c(&mut rt, "org/apache/maven/artifact/versioning/ArtifactVersion.toString.()Ljava/lang/String;", &[version]);
        assert_eq!(rt.str_of(text), Some("9.0.5+1.21.1"));
        assert_eq!(c(&mut rt, "org/apache/maven/artifact/versioning/ArtifactVersion.getMinorVersion.()I", &[version]), V::I(0));
        assert_eq!(c(&mut rt, "org/apache/maven/artifact/versioning/ArtifactVersion.getIncrementalVersion.()I", &[version]), V::I(5));
        let lib = by_id(&mut rt, "lib");
        assert_ne!(lib, container);
        let info = c(&mut rt, "net/neoforged/fml/ModContainer.getModInfo.()Lnet/neoforged/neoforgespi/language/IModInfo;", &[lib]);
        let version = c(&mut rt, "net/neoforged/neoforgespi/language/IModInfo.getVersion.()Lorg/apache/maven/artifact/versioning/ArtifactVersion;", &[info]);
        let text = c(&mut rt, "org/apache/maven/artifact/versioning/ArtifactVersion.toString.()Ljava/lang/String;", &[version]);
        assert_eq!(rt.str_of(text), Some("1"));
    }

    /// `LoadingModList.get().getModFileById(id) != null` is how mods look for another early: a
    /// mod the engine carries has a file, with its metadata; one it doesn't is null.
    #[test]
    fn the_loading_mod_list_finds_the_mods_there_and_no_others() {
        let mut rt = Rt::new();
        rt.mods = vec!["create".into(), "copycats".into()];
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"));
        let list = c(&mut rt, "net/neoforged/fml/loading/LoadingModList.get.()Lnet/neoforged/fml/loading/LoadingModList;", &[]);
        let by_id = |rt: &mut Rt, id: &str| {
            let id = rt.string(id);
            c(rt, "net/neoforged/fml/loading/LoadingModList.getModFileById.(Ljava/lang/String;)Lnet/neoforged/fml/loading/moddiscovery/ModFileInfo;", &[list, id])
        };
        assert_eq!(by_id(&mut rt, "sodium"), V::NULL);
        let file = by_id(&mut rt, "create");
        assert_ne!(file, V::NULL);
        let mods = c(&mut rt, "net/neoforged/neoforgespi/language/IModFileInfo.getMods.()Ljava/util/List;", &[file]);
        let info = crate::jutil::elements(&rt, mods)[0];
        let id = c(&mut rt, "net/neoforged/neoforgespi/language/IModInfo.getModId.()Ljava/lang/String;", &[info]);
        assert_eq!(rt.str_of(id), Some("create"));
    }

    #[test]
    fn config_values_are_their_defaults_boxed_as_java_boxes_them() {
        let mut rt = Rt::new();
        let b = rt.new_object(CONFIG_BUILDER);
        let name = rt.string("maxRotationSpeed");
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| call(rt, key, args).unwrap();
        // Comments and sections hand the builder back, so chains keep going.
        let note = rt.string("RPM");
        assert_eq!(c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$Builder.comment.(Ljava/lang/String;)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;", &[b, note]), b);
        let iv = c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$Builder.defineInRange.(Ljava/lang/String;III)Lnet/neoforged/neoforge/common/ModConfigSpec$IntValue;", &[b, name, V::I(256), V::I(64), V::I(65535)]);
        assert_eq!(c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$IntValue.getAsInt.()I", &[iv]), V::I(256));
        let boxed = c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$ConfigValue.get.()Ljava/lang/Object;", &[iv]);
        assert_eq!((rt.class_of(boxed), rt.get_field(boxed, "value")), ("java/lang/Integer", V::I(256)));
        let dv = c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$Builder.defineInRange.(Ljava/lang/String;DDD)Lnet/neoforged/neoforge/common/ModConfigSpec$DoubleValue;", &[b, name, V::D(0.5), V::D(0.0), V::D(1.0)]);
        assert_eq!(c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$DoubleValue.getAsDouble.()D", &[dv]), V::D(0.5));
        let bv = c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$Builder.define.(Ljava/lang/String;Z)Lnet/neoforged/neoforge/common/ModConfigSpec$BooleanValue;", &[b, name, V::I(1)]);
        assert_eq!(c(&mut rt, "net/neoforged/neoforge/common/ModConfigSpec$BooleanValue.getAsBoolean.()Z", &[bv]), V::I(1));
    }

    #[test]
    fn last_param_of_descriptors() {
        assert_eq!(last_param("(Lcom/x/A;Lnet/neoforged/neoforge/registries/RegisterEvent;)V"), Some("net/neoforged/neoforge/registries/RegisterEvent"));
        assert_eq!(last_param("(IJ[Ljava/lang/String;)V"), Some("[Ljava/lang/String;"));
        assert_eq!(last_param("()V"), None);
    }

    /// Create's `FanProcessingTypeRegistry.init` as a bake callback: it reads the registry with
    /// `forEach` into a list (id 1), whose `Consumer` adds each value (id 2).
    fn bake_into_list(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let seen = rt.captured(args[0])[0];
        match id {
            1 => {
                let add = rt.lambda_id("java/util/function/Consumer", 2, vec![seen]);
                call(rt, "net/minecraft/core/Registry.forEach.(Ljava/util/function/Consumer;)V", &[args[1], add]);
            }
            _ => list_mut(rt, seen).push(args[1]),
        }
        Some(V::Void)
    }

    /// The server's lifecycle as NeoForge posts it: `ServerAboutToStartEvent` before the world
    /// loads (its level's `LevelEvent.Load` comes after), `ServerStartingEvent` and
    /// `ServerStartedEvent` once it opened, `ServerStoppingEvent`, then the level's
    /// `LevelEvent.Unload` and `ServerStoppedEvent`. `getCurrentServer()` is the events' server
    /// from the first to the last, null before and after; the next world has a server and level of
    /// its own.
    #[test]
    fn the_server_starts_and_stops_as_neoforge_posts_it() {
        const CURRENT: &str = "net/neoforged/neoforge/server/ServerLifecycleHooks.getCurrentServer.()Lnet/minecraft/server/MinecraftServer;";
        const GET_SERVER: &str = "net/neoforged/neoforge/event/server/ServerLifecycleEvent.getServer.()Lnet/minecraft/server/MinecraftServer;";
        let mut rt = Rt::new();
        rt.lambda_call = Some(bake_into_list);
        let seen = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        let events = [
            "net/neoforged/neoforge/event/server/ServerAboutToStartEvent",
            "net/neoforged/neoforge/event/level/LevelEvent$Load",
            "net/neoforged/neoforge/event/server/ServerStartingEvent",
            "net/neoforged/neoforge/event/server/ServerStartedEvent",
            "net/neoforged/neoforge/event/server/ServerStoppingEvent",
            LEVEL_UNLOAD,
            "net/neoforged/neoforge/event/server/ServerStoppedEvent",
        ];
        for event in events {
            let listener = rt.lambda_id("java/util/function/Consumer", 2, vec![seen]);
            rt.listeners.push((event.to_owned(), listener));
        }
        assert_eq!(call(&mut rt, CURRENT, &[]), Some(V::NULL), "no server before it starts");
        server_about_to_start(&mut rt);
        let current = call(&mut rt, CURRENT, &[]).unwrap();
        assert_ne!(current, V::NULL);
        let level = crate::glue::level(&mut rt);
        server_started(&mut rt);
        server_stopping(&mut rt);
        server_stopped(&mut rt);
        let heard = list_mut(&mut rt, seen).clone();
        let classes: Vec<&str> = heard.iter().map(|e| rt.class_of(*e)).collect();
        assert_eq!(classes, events);
        let server_events: Vec<V> = heard.iter().copied().filter(|e| rt.class_of(*e).starts_with(SERVER_EVENTS)).collect();
        assert_eq!(server_events.len(), 5);
        for e in server_events {
            assert_eq!(call(&mut rt, GET_SERVER, &[e]), Some(current));
        }
        let unload = heard[5];
        assert_eq!(rt.get_field(unload, "$level"), level);
        assert_eq!(call(&mut rt, CURRENT, &[]), Some(V::NULL), "no server once it stopped");
        // The next world: a server and a level of its own, its level loaded again.
        server_about_to_start(&mut rt);
        assert_ne!(call(&mut rt, CURRENT, &[]), Some(current));
        assert_ne!(crate::glue::level(&mut rt), level);
        assert_eq!(list_mut(&mut rt, seen).len(), events.len() + 2);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ModList`'s walks over the loaded mods, in FML's order: ids with containers
    /// (`forEachModContainer`, Create's debug info lists the mods so), and the containers alone.
    #[test]
    fn the_mod_list_walks_its_mods_in_order() {
        fn seen(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            let log = rt.interned["test:log"];
            list_mut(rt, log).push(args[1]);
            Some(V::Void)
        }
        let mut rt = Rt::new();
        rt.mods = vec!["create".into(), "flywheel".into()];
        rt.lambda_call = Some(seen);
        let log = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        rt.interned.insert("test:log".into(), log);
        let list = call(&mut rt, "net/neoforged/fml/ModList.get.()Lnet/neoforged/fml/ModList;", &[]).unwrap();
        let each = rt.lambda_id("java/util/function/BiConsumer", 1, vec![]);
        call(&mut rt, "net/neoforged/fml/ModList.forEachModContainer.(Ljava/util/function/BiConsumer;)V", &[list, each]);
        let heard = list_mut(&mut rt, log).clone();
        let ids: Vec<&str> = heard.iter().map(|v| rt.str_of(*v).unwrap_or("")).collect();
        assert_eq!(ids, ["create", "flywheel"]);
        let sorted = call(&mut rt, "net/neoforged/fml/ModList.getSortedMods.()Ljava/util/List;", &[list]).unwrap();
        let mut owners = Vec::new();
        for c in crate::jutil::elements(&rt, sorted) {
            let id = call(&mut rt, "net/neoforged/fml/ModContainer.getModId.()Ljava/lang/String;", &[c]).unwrap();
            owners.push(rt.str_of(id).unwrap_or("").to_owned());
        }
        assert_eq!(owners, ["create", "flywheel"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `IEventBus.unregister`, as Create's `OneTimeEventReceiver` does once loading completes: an
    /// object `register` took stops hearing through its `@SubscribeEvent` methods, a class through
    /// its static ones, a listener `addListener` took itself; another object of the same class and
    /// the other listeners still hear.
    #[test]
    fn unregistered_listeners_stop_hearing() {
        const STARTED: &str = "net/neoforged/neoforge/event/server/ServerStartedEvent";
        const HANDLER: &str = "test/Handler";
        const ON: &str = "test/Handler.onStarted.(Lnet/neoforged/neoforge/event/server/ServerStartedEvent;)V";
        const ON_STATIC: &str = "test/Handler.onStartedStatic.(Lnet/neoforged/neoforge/event/server/ServerStartedEvent;)V";
        const REGISTER: &str = "net/neoforged/bus/api/IEventBus.register.(Ljava/lang/Object;)V";
        const ADD: &str = "net/neoforged/bus/api/IEventBus.addListener.(Ljava/lang/Class;Ljava/util/function/Consumer;)V";
        const UNREGISTER: &str = "net/neoforged/bus/api/IEventBus.unregister.(Ljava/lang/Object;)V";
        /// 7: the handler's method, bound to its object (heard as that object); 8: a plain
        /// listener; 9: the handler's static method.
        fn heard(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            let who = if id == 7 { rt.captured(args[0])[0] } else { V::I(id as i32) };
            let log = rt.interned["test:log"];
            list_mut(rt, log).push(who);
            Some(V::Void)
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(heard);
        let log = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        rt.interned.insert("test:log".into(), log);
        rt.subscribers.insert(HANDLER, vec![(STARTED, ON, false, 7), (STARTED, ON_STATIC, true, 9)]);
        let (one, two) = (rt.new_object(HANDLER), rt.new_object(HANDLER));
        let bus = rt.new_object("net/neoforged/bus/api/IEventBus");
        let handler_class = crate::jlang::class_object(&mut rt, HANDLER);
        let started_class = crate::jlang::class_object(&mut rt, STARTED);
        let plain = rt.lambda_id("java/util/function/Consumer", 8, vec![]);
        for (key, args) in [(REGISTER, [bus, one, V::NULL]), (REGISTER, [bus, two, V::NULL]), (REGISTER, [bus, handler_class, V::NULL]), (ADD, [bus, started_class, plain])] {
            call(&mut rt, key, &args[..if key == ADD { 3 } else { 2 }]);
        }
        let post_started = |rt: &mut Rt| {
            let ev = rt.new_object(STARTED);
            post(rt, STARTED, ev);
        };
        post_started(&mut rt);
        assert_eq!(list_mut(&mut rt, log).clone(), [one, two, V::I(9), V::I(8)]);
        for gone in [one, handler_class, plain] {
            call(&mut rt, UNREGISTER, &[bus, gone]);
        }
        post_started(&mut rt);
        assert_eq!(list_mut(&mut rt, log)[4..], [two]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ItemStack.canPerformAction(ability)` (Farmer's Delight asks it of the held stack to tell a
    /// knife): a mod item's own answer, else its vanilla class's abilities as NeoForge 1.21.1's
    /// patched classes give them: an iron axe strips, a hoe tills, shears harvest, a stick does
    /// nothing; a mod's axe without an answer of its own strips as vanilla's.
    #[test]
    fn a_stacks_abilities_are_its_items() {
        const KNIFE: &str = "test/KnifeItem";
        const BIG_AXE: &str = "test/BigAxeItem";
        fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
            if key == CAN_PERFORM && rt.class_of(args[0]) == KNIFE {
                let ability = rt.str_of(rt.get_field(args[2], "name")).unwrap_or("").to_owned();
                return V::I((ability == "knife_dig") as i32);
            }
            crate::api::call(rt, owner_key, args)
        }
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.supers.insert(KNIFE, "net/minecraft/world/item/DiggerItem");
        rt.supers.insert(BIG_AXE, "net/minecraft/world/item/AxeItem");
        for (class, id) in [(KNIFE, "test:knife"), (BIG_AXE, "test:big_axe")] {
            let item = rt.new_object(class);
            rt.registered.insert(("item".into(), id.into()), item);
        }
        let can = |rt: &mut Rt, item: &str, ability: &str| {
            let stack = crate::api_common::stack(rt, item, 1);
            let a = item_ability(rt, ability);
            crate::api::call(rt, "net/minecraft/world/item/ItemStack.canPerformAction.(Lnet/neoforged/neoforge/common/ItemAbility;)Z", &[stack, a]) == V::I(1)
        };
        let cases = [
            ("minecraft:iron_axe", "axe_strip", true),
            ("minecraft:iron_axe", "till", false),
            ("minecraft:diamond_pickaxe", "pickaxe_dig", true),
            ("minecraft:diamond_pickaxe", "axe_dig", false),
            ("minecraft:wooden_hoe", "till", true),
            ("minecraft:shears", "shears_harvest", true),
            ("minecraft:flint_and_steel", "firestarter_light", true),
            ("minecraft:stick", "sword_sweep", false),
            ("test:knife", "knife_dig", true),
            ("test:knife", "axe_dig", false),
            ("test:big_axe", "axe_strip", true),
            ("test:big_axe", "sword_sweep", false),
        ];
        for (item, ability, expected) in cases {
            assert_eq!(can(&mut rt, item, ability), expected, "{item} {ability}");
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge 21.1's finish, after common setup and the capabilities: the mods send each other
    /// messages (`InterModEnqueueEvent`), read the ones sent to them (`InterModProcessEvent`),
    /// complete their loading (`FMLLoadCompleteEvent`), then register their payloads. A message
    /// reaches a loaded mod only; each is read once, and a method filter leaves the rest queued.
    #[test]
    fn the_mods_message_each_other_then_complete_loading() {
        const SEND: &str = "net/neoforged/fml/InterModComms.sendTo.(Ljava/lang/String;Ljava/lang/String;Ljava/util/function/Supplier;)Z";
        const SEND_AS: &str = "net/neoforged/fml/InterModComms.sendTo.(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/util/function/Supplier;)Z";
        const STREAM: &str = "net/neoforged/fml/event/lifecycle/InterModProcessEvent.getIMCStream.(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;";
        const MESSAGES: &str = "net/neoforged/fml/InterModComms.getMessages.(Ljava/lang/String;)Ljava/util/stream/Stream;";
        fn items(rt: &Rt, stream: V) -> Vec<V> {
            match rt.heap.get(stream.r() as usize).map(|o| &o.data) {
                Some(Data::Stream(v)) => v.clone(),
                _ => panic!("not a stream"),
            }
        }
        // What a message says: its method, its sender and what its supplier gives.
        fn said(rt: &mut Rt, m: V) -> (String, String, String) {
            let text = |rt: &Rt, v: V| rt.str_of(v).unwrap_or("").to_owned();
            let supplier = crate::api::call(rt, "net/neoforged/fml/InterModComms$IMCMessage.messageSupplier.()Ljava/util/function/Supplier;", &[m]);
            let value = rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", supplier, &[]);
            let method = crate::api::call(rt, "net/neoforged/fml/InterModComms$IMCMessage.method.()Ljava/lang/String;", &[m]);
            let sender = crate::api::call(rt, "net/neoforged/fml/InterModComms$IMCMessage.senderModId.()Ljava/lang/String;", &[m]);
            (text(rt, method), text(rt, sender), text(rt, value))
        }
        fn mods(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            let captured = rt.captured(args[0]);
            Some(match id {
                // A message's supplier, and a filter that takes `register` only.
                7 => captured[0],
                8 => V::I((rt.str_of(args[1]) == Some("register")) as i32),
                _ => {
                    let (seen, answers) = (captured[0], captured[1]);
                    list_mut(rt, seen).push(args[1]);
                    if id == 5 {
                        let s = |rt: &mut Rt, t: &str| rt.string(t);
                        let (ruby, absent, register, other, lib, gem) =
                            (s(rt, "ruby"), s(rt, "absent"), s(rt, "register"), s(rt, "other"), s(rt, "lib"), s(rt, "gem"));
                        let supplier = rt.lambda_id("java/util/function/Supplier", 7, vec![gem]);
                        for sent in [
                            crate::api::call(rt, SEND, &[ruby, register, supplier]),
                            crate::api::call(rt, SEND, &[absent, register, supplier]),
                            crate::api::call(rt, SEND_AS, &[lib, ruby, other, supplier]),
                        ] {
                            list_mut(rt, answers).push(sent);
                        }
                    } else if id == 6 {
                        let filter = rt.lambda_id("java/util/function/Predicate", 8, vec![]);
                        let stream = crate::api::call(rt, STREAM, &[args[1], filter]);
                        let read = items(rt, stream);
                        list_mut(rt, answers).extend(read);
                    }
                    V::Void
                }
            })
        }
        let mut rt = Rt::new();
        rt.mods = vec!["ruby".into()];
        rt.lambda_call = Some(mods);
        let (seen, answers) = (rt.alloc("java/util/ArrayList", Data::List(Vec::new())), rt.alloc("java/util/ArrayList", Data::List(Vec::new())));
        let events = [
            (crate::network::REGISTER_PAYLOADS, 2),
            (LOAD_COMPLETE, 2),
            (PROCESS_IMC, 6),
            (ENQUEUE_IMC, 5),
            ("net/neoforged/neoforge/capabilities/RegisterCapabilitiesEvent", 2),
            (COMMON_SETUP, 2),
        ];
        for (event, id) in events {
            let listener = rt.lambda_id("java/util/function/Consumer", id, vec![seen, answers]);
            rt.listeners.push((event.to_owned(), listener));
        }
        fire_registrations(&mut rt);
        let heard: Vec<String> = list_mut(&mut rt, seen).clone().into_iter().map(|e| rt.class_of(e).to_owned()).collect();
        let order: Vec<&str> = events.iter().rev().map(|(e, _)| *e).collect();
        assert_eq!(heard, order);
        let got = list_mut(&mut rt, answers).clone();
        assert_eq!(got[..3], [V::I(1), V::I(0), V::I(1)], "sent to the loaded mod only");
        let read: Vec<_> = got[3..].iter().map(|m| said(&mut rt, *m)).collect();
        assert_eq!(read, [("register".to_owned(), "ruby".to_owned(), "gem".to_owned())]);
        // The message the filter left waits for the next reader, once.
        let ruby = rt.string("ruby");
        let rest = call(&mut rt, MESSAGES, &[ruby]).unwrap();
        let rest: Vec<_> = items(&rt, rest).into_iter().map(|m| said(&mut rt, m)).collect();
        assert_eq!(rest, [("other".to_owned(), "lib".to_owned(), "gem".to_owned())]);
        let again = call(&mut rt, MESSAGES, &[ruby]).unwrap();
        assert!(items(&rt, again).is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's loading order on the mod bus: construction (where Dragons Plus makes its blocks,
    /// before its registrate hears `RegisterEvent`), the registries, then common setup.
    #[test]
    fn the_mod_bus_hears_construction_then_registration_then_setup() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(bake_into_list);
        let seen = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        for event in [COMMON_SETUP, REGISTER_EVENT, CONSTRUCT] {
            let listener = rt.lambda_id("java/util/function/Consumer", 2, vec![seen]);
            rt.listeners.push((event.to_owned(), listener));
        }
        fire_registrations(&mut rt);
        let heard: Vec<String> = list_mut(&mut rt, seen).clone().into_iter().map(|e| rt.class_of(e).to_owned()).collect();
        assert_eq!(heard.first().map(String::as_str), Some(CONSTRUCT));
        assert_eq!(heard.last().map(String::as_str), Some(COMMON_SETUP));
        assert!(heard[1..heard.len() - 1].iter().all(|e| e == REGISTER_EVENT) && heard.len() > 2, "{heard:?}");
    }

    /// NeoForge's config loading: a mod's configs load once the registries are frozen, before
    /// common setup, each heard as `ModConfigEvent.Loading` by a listener for `ModConfigEvent` (the
    /// MDK's `Config.onLoad`), with the config the mod registered; a `STARTUP` config loads as it is
    /// registered.
    #[test]
    fn a_mods_configs_load_after_the_registries() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(bake_into_list);
        let seen = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        for event in [COMMON_SETUP, CONFIG_EVENT] {
            let listener = rt.lambda_id("java/util/function/Consumer", 2, vec![seen]);
            rt.listeners.push((event.to_owned(), listener));
        }
        let container = mod_container(&mut rt, "ruby");
        let ty = "net/neoforged/fml/config/ModConfig$Type";
        let common = crate::api::api_enum(&mut rt, ty, "COMMON", 0);
        let startup = crate::api::api_enum(&mut rt, ty, "STARTUP", 3);
        let (spec, early) = (rt.new_object("net/neoforged/neoforge/common/ModConfigSpec"), rt.new_object("net/neoforged/neoforge/common/ModConfigSpec"));
        let register = "net/neoforged/fml/ModContainer.registerConfig.(Lnet/neoforged/fml/config/ModConfig$Type;Lnet/neoforged/fml/config/IConfigSpec;)V";
        call(&mut rt, register, &[container, common, spec]);
        call(&mut rt, register, &[container, startup, early]);
        assert_eq!(list_mut(&mut rt, seen).len(), 1, "the startup config, at once");
        fire_registrations(&mut rt);
        let events = list_mut(&mut rt, seen).clone();
        let classes: Vec<&str> = events.iter().map(|e| rt.class_of(*e)).collect();
        assert_eq!(classes, [CONFIG_LOADING, CONFIG_LOADING, COMMON_SETUP]);
        let get = "net/neoforged/fml/event/config/ModConfigEvent.getConfig.()Lnet/neoforged/fml/config/ModConfig;";
        let (first, config) = (call(&mut rt, get, &[events[0]]).unwrap(), call(&mut rt, get, &[events[1]]).unwrap());
        let spec_of = "net/neoforged/fml/config/ModConfig.getSpec.()Lnet/neoforged/fml/config/IConfigSpec;";
        assert_eq!(call(&mut rt, spec_of, &[first]), Some(early));
        assert_eq!(call(&mut rt, spec_of, &[config]), Some(spec));
        assert_eq!(call(&mut rt, "net/neoforged/fml/config/ModConfig.getType.()Lnet/neoforged/fml/config/ModConfig$Type;", &[config]), Some(common));
        let text = |rt: &mut Rt, key: &'static str| {
            let v = call(rt, key, &[config]).unwrap();
            rt.str_of(v).map(str::to_owned)
        };
        assert_eq!(text(&mut rt, "net/neoforged/fml/config/ModConfig.getFileName.()Ljava/lang/String;").as_deref(), Some("ruby-common.toml"));
        assert_eq!(text(&mut rt, "net/neoforged/fml/config/ModConfig.getModId.()Ljava/lang/String;").as_deref(), Some("ruby"));
    }

    /// Create's `ContraptionType` makes its holder from the registry before it is registered
    /// (`createIntrusiveHolder`), and a contraption is saved and sent under that holder's key.
    #[test]
    fn an_intrusive_holder_takes_the_name_its_value_is_registered_under() {
        let mut rt = Rt::new();
        let loc = rt.string("t:widget_type");
        let key = call(&mut rt, "net/minecraft/resources/ResourceKey.createRegistryKey.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[loc]).unwrap();
        let builder = rt.new_object(REGISTRY_BUILDER);
        call(&mut rt, "net/neoforged/neoforge/registries/RegistryBuilder.<init>.(Lnet/minecraft/resources/ResourceKey;)V", &[builder, key]);
        let b = crate::api::call(&mut rt, "net/neoforged/neoforge/registries/RegistryBuilder.withIntrusiveHolders.()Lnet/neoforged/neoforge/registries/RegistryBuilder;", &[builder]);
        let registry = call(&mut rt, "net/neoforged/neoforge/registries/RegistryBuilder.create.()Lnet/minecraft/core/Registry;", &[b]).unwrap();
        let widget = rt.new_object("t/Widget");
        let own = call(&mut rt, "net/minecraft/core/Registry.createIntrusiveHolder.(Ljava/lang/Object;)Lnet/minecraft/core/Holder$Reference;", &[registry, widget]).unwrap();
        assert_eq!(call(&mut rt, "net/minecraft/core/Holder$Reference.isBound.()Z", &[own]), Some(V::I(0)));
        let id = rt.string("t:spinner");
        let held = call(&mut rt, "net/minecraft/core/Registry.registerForHolder.(Lnet/minecraft/core/Registry;Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)Lnet/minecraft/core/Holder$Reference;", &[registry, id, widget]).unwrap();
        assert_eq!(call(&mut rt, "net/minecraft/core/Holder$Reference.value.()Ljava/lang/Object;", &[held]), Some(widget));
        let k = call(&mut rt, "net/minecraft/core/Holder$Reference.key.()Lnet/minecraft/resources/ResourceKey;", &[own]).unwrap();
        let at = crate::api::call(&mut rt, "net/minecraft/resources/ResourceKey.location.()Lnet/minecraft/resources/ResourceLocation;", &[k]);
        assert_eq!(rt.str_of(at), Some("t:spinner"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `CreateBuiltInRegistries`: a registry of the mod's own, made by `RegistryBuilder` with a bake
    /// callback, filled by `Registry.register` while registries fire, and read whole by the callback
    /// once they are frozen.
    /// Vanilla's blocks and items have their keys in the built-in registries, as a mod reading one
    /// back gets (catnip's `RegisteredObjectsHelper.getKeyOrThrow(item)`, from every tooltip).
    #[test]
    fn vanilla_blocks_and_items_have_their_keys() {
        let mut rt = Rt::new();
        let key = |rt: &mut Rt, registry: &str, v: V| {
            let field = rt.static_key(&format!("net/minecraft/core/registries/BuiltInRegistries.{registry}"));
            let r = crate::api::get_static(rt, field);
            let k = crate::api::call(rt, "net/minecraft/core/Registry.getKey.(Ljava/lang/Object;)Lnet/minecraft/resources/ResourceLocation;", &[r, v]);
            rt.str_of(k).map(str::to_owned)
        };
        let door = crate::api_common::item(&mut rt, "minecraft:spruce_door");
        assert_eq!(key(&mut rt, "ITEM", door).as_deref(), Some("minecraft:spruce_door"));
        let stone = crate::api::block(&mut rt, "minecraft:stone");
        assert_eq!(key(&mut rt, "BLOCK", stone).as_deref(), Some("minecraft:stone"));
        // Not an item: no key.
        let other = rt.string("minecraft:stone");
        assert_eq!(key(&mut rt, "ITEM", other), None);
    }

    #[test]
    fn a_mods_own_registry_holds_its_entries_and_bakes_once_they_are_in() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(bake_into_list);
        let loc = rt.string("t:widget_type");
        let key = call(&mut rt, "net/minecraft/resources/ResourceKey.createRegistryKey.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[loc]).unwrap();
        let builder = rt.new_object(REGISTRY_BUILDER);
        call(&mut rt, "net/neoforged/neoforge/registries/RegistryBuilder.<init>.(Lnet/minecraft/resources/ResourceKey;)V", &[builder, key]);
        let seen = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        let callback = rt.lambda_id("net/neoforged/neoforge/registries/callback/BakeCallback", 1, vec![seen]);
        call(&mut rt, "net/neoforged/neoforge/registries/RegistryBuilder.onBake.(Lnet/neoforged/neoforge/registries/callback/BakeCallback;)Lnet/neoforged/neoforge/registries/RegistryBuilder;", &[builder, callback]);
        let registry = call(&mut rt, "net/neoforged/neoforge/registries/RegistryBuilder.create.()Lnet/minecraft/core/Registry;", &[builder]).unwrap();
        let (splashing, blasting) = (rt.new_object("t/Splashing"), rt.new_object("t/Blasting"));
        for (name, v) in [("t:splashing", splashing), ("t:blasting", blasting)] {
            let id = rt.string(name);
            call(&mut rt, "net/minecraft/core/Registry.register.(Lnet/minecraft/core/Registry;Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)Ljava/lang/Object;", &[registry, id, v]);
        }
        let id = rt.string("t:blasting");
        assert_eq!(call(&mut rt, "net/minecraft/core/Registry.get.(Lnet/minecraft/resources/ResourceLocation;)Ljava/lang/Object;", &[registry, id]), Some(blasting));
        assert!(list_mut(&mut rt, seen).is_empty(), "baked before the registries froze");
        fire_registrations(&mut rt);
        let mut baked = list_mut(&mut rt, seen).clone();
        baked.sort_by_key(|v| v.r());
        assert_eq!(baked, [splashing, blasting]);
        // An event for that registry is about the registry the mod holds.
        let ev = rt.new_object(REGISTER_EVENT);
        rt.put_field(ev, "$registry", key);
        assert_eq!(call(&mut rt, "net/neoforged/neoforge/registries/RegisterEvent.getRegistry.()Lnet/minecraft/core/Registry;", &[ev]), Some(registry));
    }

    #[test]
    fn register_event_reaches_listeners_and_holders_resolve() {
        let mut rt = Rt::new();
        // A listener as a lambda over `RegisterEvent`, registering through a helper.
        let seen = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        let listener = rt.lambda("t/Mod.onRegister.(Lnet/neoforged/neoforge/registries/RegisterEvent;)V", vec![seen]);
        call(&mut rt, "net/neoforged/bus/api/IEventBus.addListener.(Ljava/util/function/Consumer;)V", &[V::NULL, listener]);
        assert_eq!(rt.listeners, [("net/neoforged/neoforge/registries/RegisterEvent".to_owned(), listener)]);
        // Holders: a block one, resolved once something registers `t:thing`.
        let block = crate::api::get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK");
        let id = rt.string("t:thing");
        let h = call(&mut rt, "net/neoforged/neoforge/registries/DeferredHolder.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/neoforged/neoforge/registries/DeferredHolder;", &[block, id]).unwrap();
        assert_eq!(call(&mut rt, "net/neoforged/neoforge/registries/DeferredHolder.get.()Ljava/lang/Object;", &[h]), Some(V::NULL));
        let ev = rt.new_object(REGISTER_EVENT);
        rt.put_field(ev, "$registry", block);
        let helper = rt.new_object("net/neoforged/neoforge/registries/RegisterEvent$RegisterHelper");
        rt.put_field(helper, "$registry", block);
        let thing = rt.new_object("t/ThingBlock");
        call(&mut rt, "net/neoforged/neoforge/registries/RegisterEvent$RegisterHelper.register.(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)V", &[helper, id, thing]);
        assert_eq!(call(&mut rt, "net/neoforged/neoforge/registries/DeferredHolder.get.()Ljava/lang/Object;", &[h]), Some(thing));
        assert_eq!(rt.registered.get(&("block".to_owned(), "t:thing".to_owned())), Some(&thing));
        // Keys: registry and location.
        let k = call(&mut rt, "net/minecraft/resources/ResourceKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[block, id]).unwrap();
        let loc = call(&mut rt, "net/minecraft/resources/ResourceKey.location.()Lnet/minecraft/resources/ResourceLocation;", &[k]).unwrap();
        assert_eq!(rt.str_of(loc), Some("t:thing"));
    }

    #[test]
    fn holders_aliases_locations_and_private_fields_answer_as_neoforge_does() {
        let mut rt = Rt::new();
        let block = crate::api::get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK");
        let (id, other_id) = (rt.string("t:thing"), rt.string("t:other"));
        let create = "net/neoforged/neoforge/registries/DeferredHolder.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/neoforged/neoforge/registries/DeferredHolder;";
        let (h, same, other) = (call(&mut rt, create, &[block, id]).unwrap(), call(&mut rt, create, &[block, id]).unwrap(), call(&mut rt, create, &[block, other_id]).unwrap());
        // Two holders of one key are equal (and hash alike); another key's, null, are not.
        let equals = "net/neoforged/neoforge/registries/DeferredHolder.equals.(Ljava/lang/Object;)Z";
        let hash = "net/neoforged/neoforge/registries/DeferredHolder.hashCode.()I";
        assert_ne!(h, same);
        assert_eq!(call(&mut rt, equals, &[h, same]), Some(V::I(1)));
        assert_eq!(call(&mut rt, hash, &[h]), call(&mut rt, hash, &[same]));
        assert_eq!(call(&mut rt, equals, &[h, other]), Some(V::I(0)));
        assert_eq!(call(&mut rt, equals, &[h, V::NULL]), Some(V::I(0)));
        // An alias reads as the name it points to.
        let thing = rt.new_object("t/ThingBlock");
        rt.registered.insert(("block".into(), "t:thing".into()), thing);
        let old = rt.string("t:old_thing");
        call(&mut rt, "net/minecraft/core/Registry.addAlias.(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceLocation;)V", &[block, old, id]);
        assert_eq!(call(&mut rt, "net/minecraft/core/Registry.get.(Lnet/minecraft/resources/ResourceLocation;)Ljava/lang/Object;", &[block, old]), Some(thing));
        // `tryBuild`: null for a name vanilla refuses.
        let try_build = "net/minecraft/resources/ResourceLocation.tryBuild.(Ljava/lang/String;Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;";
        let (ns, path, bad) = (rt.string("t"), rt.string("a/b_c.d"), rt.string("Bad Path"));
        let ok = call(&mut rt, try_build, &[ns, path]).unwrap();
        assert_eq!(rt.str_of(ok), Some("t:a/b_c.d"));
        assert_eq!(call(&mut rt, try_build, &[ns, bad]), Some(V::NULL));
        assert_eq!(call(&mut rt, try_build, &[path, ns]), Some(V::NULL));
        // Private fields by name: a translated object's own; a static without an instance.
        rt.supers.insert("t/Thing", "java/lang/Object");
        let class = crate::jlang::class_object(&mut rt, "t/Thing");
        let (field, value) = (rt.string("count"), V::I(7));
        let set = "net/neoforged/fml/util/ObfuscationReflectionHelper.setPrivateValue.(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/String;)V";
        let get = "net/neoforged/fml/util/ObfuscationReflectionHelper.getPrivateValue.(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;";
        call(&mut rt, set, &[class, thing, value, field]);
        assert_eq!(rt.get_field(thing, "count"), V::I(7));
        assert_eq!(call(&mut rt, get, &[class, thing, field]), Some(V::I(7)));
        call(&mut rt, set, &[class, V::NULL, V::I(3), field]);
        assert_eq!(rt.statics.get("t/Thing.count"), Some(&V::I(3)));
        // Registrate's: a copied block's drops cleared.
        let andesite = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/Blocks.ANDESITE");
        let p = crate::api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.ofFullCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[andesite]);
        crate::api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.noLootTable.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[p]);
        let (pc, drops) = (crate::jlang::class_object(&mut rt, crate::blocks::BLOCK_PROPERTIES), rt.string("drops"));
        call(&mut rt, set, &[pc, p, V::NULL, drops]);
        assert_eq!(rt.get_field(p, "$noLootTable"), V::I(0));
    }

    /// Aquaculture's dev check, `Launcher.INSTANCE.environment().getProperty(IEnvironment.Keys
    /// .VERSION.get())`: a player's game tells no property.
    #[test]
    fn modlaunchers_environment_tells_no_properties() {
        let mut rt = Rt::new();
        crate::install();
        let launcher = crate::api::get_static(&mut rt, "cpw/mods/modlauncher/Launcher.INSTANCE");
        let env = call(&mut rt, "cpw/mods/modlauncher/Launcher.environment.()Lcpw/mods/modlauncher/Environment;", &[launcher]).unwrap();
        let supplier = crate::api::get_static(&mut rt, "cpw/mods/modlauncher/api/IEnvironment$Keys.VERSION");
        let key = crate::api::call(&mut rt, "java/util/function/Supplier.get.()Ljava/lang/Object;", &[supplier]);
        assert_eq!(rt.class_of(key), "cpw/mods/modlauncher/api/TypesafeMap$Key");
        let found = call(&mut rt, "cpw/mods/modlauncher/Environment.getProperty.(Lcpw/mods/modlauncher/api/TypesafeMap$Key;)Ljava/util/Optional;", &[env, key]).unwrap();
        assert_eq!(crate::api::call(&mut rt, "java/util/Optional.isPresent.()Z", &[found]), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
