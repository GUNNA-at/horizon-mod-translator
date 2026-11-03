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
fn reaches(rt: &Rt, event: &str, wanted: &str) -> bool {
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

/// Posts `event` (an object of class `class`) to the mod bus's listeners.
pub fn post(rt: &mut Rt, class: &str, event: V) {
    let listeners: Vec<(String, V)> = rt.listeners.clone();
    for (wanted, l) in listeners {
        if reaches(rt, class, &wanted) {
            rt.call_functional(ACCEPT, l, &[event]);
            rt.report_thrown(class);
        }
    }
}

/// Fires `RegisterEvent` once per registry, as NeoForge does after mod construction.
pub fn fire_registrations(rt: &mut Rt) {
    let mut names: Vec<String> = REGISTRIES.iter().map(|s| s.to_string()).collect();
    for extra in rt.custom_registries.clone() {
        if !names.contains(&extra) {
            names.push(extra);
        }
    }
    for registry in names {
        let ev = rt.new_object(REGISTER_EVENT);
        let key = rt.string(&registry);
        rt.put_field(ev, "$registry", key);
        post(rt, REGISTER_EVENT, ev);
    }
    // The registries are full: then common setup, as NeoForge's loading goes on.
    rt.resolve_registrations();
    fire_common_setup(rt);
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
fn registry_get(rt: &mut Rt, registry: &str, id: &str) -> V {
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
fn registry_holder(rt: &mut Rt, registry: &str, id: &str, value: V) -> V {
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

fn location(rt: &Rt, v: V) -> String {
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

/// A NeoForge loading call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
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
        // Level events: the level they're about.
        (o, "getLevel") if o.starts_with("net/neoforged/neoforge/event/level/LevelEvent") => rt.get_field(a(0), "$level"),
        ("net/neoforged/bus/api/IEventBus", "post") => {
            let class = rt.class_of(a(1));
            post(rt, class, a(1));
            a(1)
        }
        (REGISTER_EVENT, "getRegistryKey") => rt.get_field(a(0), "$registry"),
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
                "keySet" | "entrySet" | "stream" | "iterator" | "size" | "holders" | "listElements" => {
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
                        _ => {
                            let vs: Vec<V> = entries.iter().map(|(_, v)| *v).collect();
                            crate::jutil::new_iter(rt, vs)
                        }
                    }
                }
                "key" => rt.string(&reg),
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
        (o, "registryOrThrow" | "lookupOrThrow" | "registry" | "lookup") if o.starts_with("net/minecraft/core/") && (o.contains("RegistryAccess") || o.contains("HolderLookup") || o.contains("HolderGetter")) => {
            let key = a(1);
            if matches!(name, "registry" | "lookup") { optional(rt, key) } else { key }
        }
        ("net/neoforged/bus/api/Event", "<init>") => V::Void,
        // A registry entry's `Holder` made above: its value, key and name.
        ("net/minecraft/core/Holder" | "net/minecraft/core/Holder$Reference", n) if rt.get_field(a(0), "$registry").r() != 0 => {
            let (reg, id) = (rt.str_of(rt.get_field(a(0), "$registry")).unwrap_or("").to_owned(), rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned());
            match n {
                "value" | "get" => {
                    let v = rt.get_field(a(0), "$value");
                    if v.r() != 0 { v } else { registry_get(rt, &reg, &id) }
                }
                "isBound" => V::I(1),
                "key" => rt.string(&format!("{reg}#{id}")),
                "unwrapKey" => {
                    let k = rt.string(&format!("{reg}#{id}"));
                    optional(rt, k)
                }
                "getRegisteredName" => rt.string(&id),
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
            a(2)
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
        ("net/neoforged/fml/ModContainer", "registerConfig" | "registerExtensionPoint") => V::Void,
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

/// Loading objects a mod's constructor takes: the event bus, its container, the side.
pub fn constructor_arg(rt: &mut Rt, class: &str) -> V {
    match class {
        "net/neoforged/bus/api/IEventBus" => rt.new_object("net/neoforged/bus/api/IEventBus"),
        "net/neoforged/fml/ModContainer" => rt.new_object("net/neoforged/fml/ModContainer"),
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
