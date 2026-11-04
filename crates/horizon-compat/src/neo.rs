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
    rt.resolve_registrations();
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
    // Then common setup and capabilities, as NeoForge's loading goes on.
    fire_common_setup(rt);
    // NeoForge's network registry: the mods register their payload types.
    crate::network::fire_registration(rt);
    crate::capabilities::fire(rt);
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
            | "net/minecraft/core/HolderLookup$RegistryLookup"
            | "net/minecraft/core/HolderGetter"
    )
}

/// The name an object is registered under in `registry` (`create:shaft`).
fn registry_name_of(rt: &Rt, registry: &str, obj: V) -> Option<String> {
    if obj.r() == 0 {
        return None;
    }
    rt.registered_name(obj, registry).map(str::to_owned)
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
    match registry {
        "block" if id.starts_with("minecraft:") => crate::api::block(rt, id),
        "item" if id.starts_with("minecraft:") => crate::api_common::item(rt, id),
        "fluid" if id.starts_with("minecraft:") => crate::fluids::fluid(rt, id),
        _ => V::NULL,
    }
}

/// A registry entry's `Holder`: its key and value.
pub(crate) fn registry_holder(rt: &mut Rt, registry: &str, id: &str, value: V) -> V {
    let h = rt.new_object("net/minecraft/core/Holder$Reference");
    let (r, i) = (rt.string(registry), rt.string(id));
    rt.put_field(h, "$registry", r);
    rt.put_field(h, "$id", i);
    rt.put_field(h, "$value", value);
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
    }
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
        || owner.starts_with("net/minecraft/resources/")
        || is_registry(owner)
        || owner.starts_with("org/apache/maven/artifact/versioning/")
        || matches!(owner, "net/minecraft/core/Holder" | "net/minecraft/core/Holder$Reference")
        || (owner.starts_with("net/minecraft/") && matches!(name, "registryAccess" | "holderLookup" | "registryOrThrow" | "lookupOrThrow" | "registry" | "lookup"))
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
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
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
        ("net/minecraft/resources/ResourceLocation", "parse" | "tryParse" | "bySeparator") => {
            let s = location(rt, a(0));
            rt.string(&s)
        }
        ("net/minecraft/resources/ResourceLocation", "getNamespace" | "getPath") => {
            let s = location(rt, a(0));
            let (ns, path) = s.split_once(':').unwrap_or(("minecraft", &s));
            let part = if name == "getNamespace" { ns } else { path }.to_owned();
            rt.string(&part)
        }
        ("net/minecraft/resources/ResourceLocation", "toString" | "toLanguageKey" | "toShortLanguageKey") => {
            let s = location(rt, a(0));
            let s = if name == "toString" { s } else { s.replace(':', ".") };
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
                rt.resolve_registrations();
            }
            rt.registered.get(&k).copied().unwrap_or(V::NULL)
        }
        (o, "getId") if o.starts_with("net/neoforged/neoforge/registries/Deferred") => rt.get_field(a(0), "$id"),
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
                    match n {
                        "containsKey" => V::I((v.r() != 0) as i32),
                        "getOptional" => optional(rt, v),
                        "getHolder" => {
                            let h = if v.r() != 0 { registry_holder(rt, &reg, &id, v) } else { V::NULL };
                            optional(rt, h)
                        }
                        "getHolderOrThrow" | "wrapAsHolder" => registry_holder(rt, &reg, &id, v),
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
                            _ => false,
                        };
                    V::I(inside as i32)
                }
                "is" => V::I((name_in(rt, a(1)) == id) as i32),
                _ => return None,
            }
        }
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
        // The side: Horizon runs mods in the client.
        ("net/neoforged/fml/loading/FMLLoader", "isProduction") => V::I(1),
        ("net/neoforged/fml/loading/FMLLoader" | "net/neoforged/fml/loading/FMLEnvironment", "getDist") => {
            crate::api::get_static(rt, "net/neoforged/api/distmarker/Dist.CLIENT")
        }
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
        (MOD_CONTAINER, "registerConfig" | "registerExtensionPoint") => V::Void,
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
        (LOADING_CONTEXT, "registerExtensionPoint" | "registerConfig") => V::Void,
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
        (MOD_FILE_INFO, "getMods") => {
            let id = rt.get_field(a(0), "$modId");
            let id = rt.str_of(id).unwrap_or("").to_owned();
            let info = mod_info(rt, &id);
            crate::jutil::new_list(rt, vec![info])
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
        _ => return None,
    })
}

const COMMON_SETUP: &str = "net/neoforged/fml/event/lifecycle/FMLCommonSetupEvent";
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
    let ev = rt.new_object(REGISTER_RENDERERS);
    post(rt, REGISTER_RENDERERS, ev);
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

/// A `java.lang` box of `v`.
fn boxed(rt: &mut Rt, class: &'static str, v: V) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// A method descriptor's parameter types, as written (`I`, `Ljava/lang/String;`, `[I`...).
fn desc_params(desc: &str) -> Vec<String> {
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
fn loaded_mods(rt: &Rt) -> Vec<String> {
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
const KOTLIN_CONTEXT: &str = "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext";
const KOTLIN_CONTEXT_COMPANION: &str = "thedarkcolour/kotlinforforge/neoforge/KotlinModLoadingContext$Companion";

/// The loader's static fields: Kotlin for Forge's loading context's companion (`KotlinModLoadingContext.get()`
/// in Kotlin is a call on it).
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (KOTLIN_CONTEXT | "thedarkcolour/kotlinforforge/KotlinModLoadingContext", "Companion") => {
            Some(rt.intern("compat:kotlin-loading-context-companion".into(), |rt| rt.new_object(KOTLIN_CONTEXT_COMPANION)))
        }
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
}
