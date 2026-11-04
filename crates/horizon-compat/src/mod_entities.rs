//! Entities a mod's own classes make (`new ControlledContraptionEntity(type, level)`: Create's
//! contraptions, seats, glue): vanilla `Entity`'s state as fields of the object, under vanilla's
//! names where subclasses read them directly (`xo`, `tickCount`, `entityData`, `removalReason`) and
//! `$` names for the rest; `SynchedEntityData`; the entity types `EntityType.Builder` makes; and
//! the level's list of them (`addFreshEntity`), ticked in the level's tick as `ServerLevel` and
//! `ClientLevel` tick theirs.
use crate::{Data, Rt, V, api, jutil};

pub const ENTITY: &str = "net/minecraft/world/entity/Entity";
const ENTITY_TYPE: &str = "net/minecraft/world/entity/EntityType";
const TYPE_BUILDER: &str = "net/minecraft/world/entity/EntityType$Builder";
const DIMENSIONS: &str = "net/minecraft/world/entity/EntityDimensions";
const SYNCHED: &str = "net/minecraft/network/syncher/SynchedEntityData";
const SYNCHED_BUILDER: &str = "net/minecraft/network/syncher/SynchedEntityData$Builder";
const ACCESSOR: &str = "net/minecraft/network/syncher/EntityDataAccessor";
const REMOVAL: &str = "net/minecraft/world/entity/Entity$RemovalReason";
/// `Entity.RemovalReason`'s constants in order, and whether each destroys the entity.
const REMOVALS: [(&str, bool); 5] =
    [("KILLED", true), ("DISCARDED", true), ("UNLOADED_TO_CHUNK", false), ("UNLOADED_WITH_PLAYER", false), ("CHANGED_DIMENSION", false)];
/// The level's entities of mods, in the order they were added.
const LIVE: &str = "compat:mod-entities";
/// The first id an entity a mod makes gets: far above the game's own entities', so a lookup by id
/// on either side never finds the other's.
const FIRST_ID: i32 = 1 << 30;
/// Vanilla `Entity`'s own synched data (shared flags, air, name, name shown, silent, no gravity,
/// pose, frozen ticks): a subclass's first `defineId` is the one after.
const ENTITY_DATA: i32 = 8;

const DEFINE_SYNCHED: &str = "defineSynchedData.(Lnet/minecraft/network/syncher/SynchedEntityData$Builder;)V";
const DEFINE_SYNCHED_KEY: &str = "net/minecraft/world/entity/Entity.defineSynchedData.(Lnet/minecraft/network/syncher/SynchedEntityData$Builder;)V";
const SET_POS: &str = "setPos.(DDD)V";
const SET_POS_KEY: &str = "net/minecraft/world/entity/Entity.setPos.(DDD)V";
const TICK: &str = "tick.()V";
const TICK_KEY: &str = "net/minecraft/world/entity/Entity.tick.()V";
const ON_SYNCED: &str = "onSyncedDataUpdated.(Lnet/minecraft/network/syncher/EntityDataAccessor;)V";
const ON_SYNCED_KEY: &str = "net/minecraft/world/entity/Entity.onSyncedDataUpdated.(Lnet/minecraft/network/syncher/EntityDataAccessor;)V";
const REMOVE: &str = "remove.(Lnet/minecraft/world/entity/Entity$RemovalReason;)V";
const REMOVE_KEY: &str = "net/minecraft/world/entity/Entity.remove.(Lnet/minecraft/world/entity/Entity$RemovalReason;)V";
const ADDITIONAL_SAVE: &str = "addAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
const ADDITIONAL_SAVE_KEY: &str = "net/minecraft/world/entity/Entity.addAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
const ADDITIONAL_LOAD: &str = "readAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
const ADDITIONAL_LOAD_KEY: &str = "net/minecraft/world/entity/Entity.readAdditionalSaveData.(Lnet/minecraft/nbt/CompoundTag;)V";
const ENTITY_FACTORY: &str = "net/minecraft/world/entity/EntityType$EntityFactory.create.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";

/// Whether `e` is an entity a mod's class made (its `Entity` constructor ran here).
pub fn is_mod_entity(rt: &Rt, e: V) -> bool {
    e.r() != 0 && rt.get_field(e, "$modEntity").i() != 0
}

/// The level's mod entities, in the order they were added.
pub fn live(rt: &Rt) -> Vec<V> {
    rt.interned.get(LIVE).map(|&l| jutil::elements(rt, l)).unwrap_or_default()
}

fn live_list(rt: &mut Rt) -> V {
    rt.intern(LIVE.into(), |rt| jutil::new_list(rt, Vec::new()))
}

fn set_live(rt: &mut Rt, items: Vec<V>) {
    let l = live_list(rt);
    if let Some(o) = rt.heap.get_mut(l.r() as usize) {
        o.data = Data::List(items);
    }
}

fn virtual_call(rt: &mut Rt, e: V, owner_key: &'static str, key: &'static str, args: &[V]) -> Option<V> {
    let class = rt.class_of(e);
    if !rt.overrides.is_some_and(|o| o(class, key)) {
        return None;
    }
    let vc = rt.vcall?;
    let mut all = vec![e];
    all.extend_from_slice(args);
    Some(vc(rt, owner_key, key, &all))
}

fn xyz(rt: &Rt, e: V) -> [f64; 3] {
    [rt.get_field(e, "$x").d(), rt.get_field(e, "$y").d(), rt.get_field(e, "$z").d()]
}

fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
    let v = rt.new_object("net/minecraft/world/phys/Vec3");
    rt.put_field(v, "x", V::D(p[0]));
    rt.put_field(v, "y", V::D(p[1]));
    rt.put_field(v, "z", V::D(p[2]));
    v
}

fn of_vec3(rt: &Rt, v: V) -> [f64; 3] {
    [rt.get_field(v, "x").d(), rt.get_field(v, "y").d(), rt.get_field(v, "z").d()]
}

/// An entity type's width and height (`EntityType.Builder.sized`; vanilla's default 0.6 by 1.8).
fn size(rt: &Rt, ty: V) -> (f64, f64) {
    let w = rt.get_field(ty, "$width");
    let h = rt.get_field(ty, "$height");
    (if matches!(w, V::F(_)) { f64::from(w.f()) } else { 0.6 }, if matches!(h, V::F(_)) { f64::from(h.f()) } else { 1.8 })
}

/// `EntityDimensions.makeBoundingBox(position)`: the type's box, centred on x and z, from y up.
fn make_box(rt: &Rt, e: V, p: [f64; 3]) -> [f64; 6] {
    let (w, h) = size(rt, rt.get_field(e, "$type"));
    [p[0] - w / 2.0, p[1], p[2] - w / 2.0, p[0] + w / 2.0, p[1] + h, p[2] + w / 2.0]
}

fn removal(rt: &mut Rt, i: usize) -> V {
    let (name, _) = REMOVALS[i.min(4)];
    api::api_enum(rt, REMOVAL, name, i as i32)
}

/// `Entity(EntityType, Level)`: an id, a random UUID, at the origin, the synched data vanilla's
/// constructor defines and then the subclass's own (`defineSynchedData`), then `setPos(0, 0, 0)`
/// (both virtual, as in Java: a subclass's run before its own fields are set).
fn construct(rt: &mut Rt, e: V, ty: V, level: V) {
    let id = next_id(rt);
    rt.put_field(e, "$modEntity", V::I(1));
    rt.put_field(e, "$id", V::I(id));
    rt.put_field(e, "$type", ty);
    rt.put_field(e, "$level", level);
    for f in ["$x", "$y", "$z", "xo", "yo", "zo", "xOld", "yOld", "zOld"] {
        rt.put_field(e, f, V::D(0.0));
    }
    for f in ["$yRot", "$xRot", "yRotO", "xRotO"] {
        rt.put_field(e, f, V::F(0.0));
    }
    rt.put_field(e, "tickCount", V::I(0));
    let zero = vec3(rt, [0.0; 3]);
    rt.put_field(e, "$motion", zero);
    let uuid = api::call(rt, "java/util/UUID.randomUUID.()Ljava/util/UUID;", &[]);
    rt.put_field(e, "uuid", uuid);
    let d = rt.host().map(|h| h.dimension()).unwrap_or_else(|| "minecraft:overworld".into());
    let d = rt.string(&d);
    rt.put_field(e, "$dimension", d);
    // Vanilla's eight: unboxed values a mod never reads; only their places count.
    let builder = rt.alloc(SYNCHED_BUILDER, Data::Array(vec![V::NULL; ENTITY_DATA as usize]));
    rt.put_field(builder, "$entity", e);
    let sers = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![V::NULL; ENTITY_DATA as usize]));
    rt.put_field(builder, "$serializers", sers);
    virtual_call(rt, e, DEFINE_SYNCHED_KEY, DEFINE_SYNCHED, &[builder]);
    rt.report_thrown("defineSynchedData");
    let items = match rt.heap.get(builder.r() as usize).map(|o| &o.data) {
        Some(Data::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let data = rt.alloc(SYNCHED, Data::Array(items));
    rt.put_field(data, "$entity", e);
    rt.put_field(data, "$serializers", sers);
    rt.put_field(e, "entityData", data);
    set_pos(rt, e, [0.0; 3]);
}

fn next_id(rt: &mut Rt) -> i32 {
    let n = rt.interned.get("compat:next-entity-id").map_or(FIRST_ID, |v| v.i());
    rt.interned.insert("compat:next-entity-id".into(), V::I(n + 1));
    n
}

/// `entity.setPos(x, y, z)`, virtually: a subclass's own (Create's contraptions move their box with
/// the structure) or vanilla's.
pub fn set_pos(rt: &mut Rt, e: V, p: [f64; 3]) {
    if virtual_call(rt, e, SET_POS_KEY, SET_POS, &[V::D(p[0]), V::D(p[1]), V::D(p[2])]).is_none() {
        base_set_pos(rt, e, p);
    }
    rt.report_thrown("setPos");
}

/// Vanilla's `Entity.setPos`: the position, then the box the type's size makes there.
fn base_set_pos(rt: &mut Rt, e: V, p: [f64; 3]) {
    set_pos_raw(rt, e, p);
    let b = make_box(rt, e, p);
    let bb = crate::shapes::aabb(rt, b);
    rt.put_field(e, "$bb", bb);
}

fn set_pos_raw(rt: &mut Rt, e: V, p: [f64; 3]) {
    rt.put_field(e, "$x", V::D(p[0]));
    rt.put_field(e, "$y", V::D(p[1]));
    rt.put_field(e, "$z", V::D(p[2]));
}

/// Whether two synched values are equal (`ObjectUtils.notEqual` is what `set` checks): the same
/// object, or boxes and optionals holding the same value.
fn same_value(rt: &Rt, a: V, b: V) -> bool {
    if a == b {
        return true;
    }
    if a.r() == 0 || b.r() == 0 || rt.class_of(a) != rt.class_of(b) {
        return false;
    }
    match (rt.get_field(a, "value"), rt.get_field(b, "value")) {
        (V::Void, _) | (_, V::Void) => rt.str_of(a).is_some() && rt.str_of(a) == rt.str_of(b),
        (x, y) => x == y || (x.r() != 0 && same_value(rt, x, y)),
    }
}

fn data_items(rt: &mut Rt, data: V) -> Option<&mut Vec<V>> {
    match rt.heap.get_mut(data.r() as usize).map(|o| &mut o.data) {
        Some(Data::Array(items)) => Some(items),
        _ => None,
    }
}

/// The id `SynchedEntityData.defineId(class, serializer)` gives: one past the last its class
/// defined, or past its superclasses' (vanilla `Entity`'s eight first).
fn define_id(rt: &mut Rt, class_literal: V) -> i32 {
    let class = rt.str_of(class_literal).unwrap_or("").replace('.', "/");
    let key = |c: &str| format!("compat:data-id/{c}");
    let next = match rt.interned.get(&key(&class)) {
        Some(v) => v.i() + 1,
        None => {
            let mut c: Option<&'static str> = rt.supers.get(class.as_str()).copied();
            let mut found = ENTITY_DATA;
            while let Some(k) = c {
                if let Some(v) = rt.interned.get(&key(k)) {
                    found = v.i() + 1;
                    break;
                }
                c = rt.supers.get(k).copied();
            }
            found
        }
    };
    rt.interned.insert(key(&class), V::I(next));
    next
}

/// Static fields this module owns: `Entity.RemovalReason`'s constants.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        REMOVAL => {
            let i = REMOVALS.iter().position(|(n, _)| *n == field)?;
            Some(removal(rt, i))
        }
        // Vanilla's serializers, each with the stream codec it writes with (1.21.1's
        // `EntityDataSerializers`); one the runtime has no codec for keeps its value from players.
        "net/minecraft/network/syncher/EntityDataSerializers" => {
            let codec = |rt: &mut Rt, owner: &str, f: &str| crate::network::get_static(rt, owner, f).unwrap_or(V::NULL);
            const BB: &str = "net/minecraft/network/codec/ByteBufCodecs";
            const OPTIONAL: &str = "net/minecraft/network/codec/ByteBufCodecs.optional.(Lnet/minecraft/network/codec/StreamCodec;)Lnet/minecraft/network/codec/StreamCodec;";
            let f = field.to_owned();
            Some(rt.intern(format!("compat:data-serializer/{f}"), move |rt| {
                let c = match f.as_str() {
                    "BYTE" => codec(rt, BB, "BYTE"),
                    "INT" => codec(rt, BB, "VAR_INT"),
                    "LONG" => codec(rt, BB, "VAR_LONG"),
                    "FLOAT" => codec(rt, BB, "FLOAT"),
                    "STRING" => codec(rt, BB, "STRING_UTF8"),
                    "BOOLEAN" => codec(rt, BB, "BOOL"),
                    "COMPOUND_TAG" => codec(rt, BB, "TRUSTED_COMPOUND_TAG"),
                    "ITEM_STACK" => codec(rt, "net/minecraft/world/item/ItemStack", "OPTIONAL_STREAM_CODEC"),
                    "BLOCK_POS" => codec(rt, "net/minecraft/core/BlockPos", "STREAM_CODEC"),
                    "DIRECTION" => codec(rt, "net/minecraft/core/Direction", "STREAM_CODEC"),
                    "COMPONENT" => codec(rt, "net/minecraft/network/chat/ComponentSerialization", "TRUSTED_STREAM_CODEC"),
                    "OPTIONAL_UUID" | "OPTIONAL_BLOCK_POS" | "OPTIONAL_COMPONENT" => {
                        let inner = match f.as_str() {
                            "OPTIONAL_UUID" => codec(rt, "net/minecraft/core/UUIDUtil", "STREAM_CODEC"),
                            "OPTIONAL_BLOCK_POS" => codec(rt, "net/minecraft/core/BlockPos", "STREAM_CODEC"),
                            _ => codec(rt, "net/minecraft/network/chat/ComponentSerialization", "TRUSTED_STREAM_CODEC"),
                        };
                        api::call(rt, OPTIONAL, &[inner])
                    }
                    _ => V::NULL,
                };
                let s = rt.new_object("net/minecraft/network/syncher/EntityDataSerializer");
                rt.put_field(s, "$codec", c);
                s
            }))
        }
        _ => None,
    }
}

pub fn call_key(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    call(rt, owner, name, desc, args)
}

/// Entity type, synched data and mod entity calls; None for anything else.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // The level's: a mod's entity joins its list (an item entity is `entities`').
    if name == "addFreshEntity" && is_mod_entity(rt, a(1)) {
        let e = a(1);
        let l = live_list(rt);
        if !jutil::elements(rt, l).contains(&e) {
            let mut items = jutil::elements(rt, l);
            items.push(e);
            set_live(rt, items);
        }
        return Some(V::I(1));
    }
    if name == "getEntity" && desc == "(I)Lnet/minecraft/world/entity/Entity;" {
        let id = a(1).i();
        if id >= FIRST_ID {
            return Some(live(rt).into_iter().find(|&e| rt.get_field(e, "$id").i() == id).unwrap_or(V::NULL));
        }
        return None;
    }
    match (owner, name) {
        (TYPE_BUILDER, "of") => {
            let b = rt.new_object(TYPE_BUILDER);
            rt.put_field(b, "$factory", a(0));
            rt.put_field(b, "$category", a(1));
            Some(b)
        }
        (TYPE_BUILDER, "sized") => {
            rt.put_field(a(0), "$width", a(1));
            rt.put_field(a(0), "$height", a(2));
            Some(a(0))
        }
        (TYPE_BUILDER, "clientTrackingRange" | "setTrackingRange") => {
            rt.put_field(a(0), "$range", a(1));
            Some(a(0))
        }
        (TYPE_BUILDER, "updateInterval" | "setUpdateInterval") => {
            rt.put_field(a(0), "$interval", a(1));
            Some(a(0))
        }
        (TYPE_BUILDER, "build") => {
            let t = rt.new_object(ENTITY_TYPE);
            for f in ["$factory", "$category", "$width", "$height", "$range", "$interval"] {
                let v = rt.get_field(a(0), f);
                rt.put_field(t, f, v);
            }
            Some(t)
        }
        (TYPE_BUILDER, _) if desc.ends_with(")Lnet/minecraft/world/entity/EntityType$Builder;") => Some(a(0)),
        (ENTITY_TYPE, "create") if desc.starts_with("(Lnet/minecraft/world/level/Level;") => {
            let factory = rt.get_field(a(0), "$factory");
            if factory.r() == 0 {
                return Some(V::NULL);
            }
            Some(rt.call_functional(ENTITY_FACTORY, factory, &[a(0), a(1)]))
        }
        (ENTITY_TYPE, "getWidth") => Some(V::F(size(rt, a(0)).0 as f32)),
        (ENTITY_TYPE, "getHeight") => Some(V::F(size(rt, a(0)).1 as f32)),
        (ENTITY_TYPE, "clientTrackingRange") => Some(V::I(match rt.get_field(a(0), "$range") {
            V::I(r) => r,
            _ => 5,
        })),
        (ENTITY_TYPE, "updateInterval") => Some(V::I(match rt.get_field(a(0), "$interval") {
            V::I(r) => r,
            _ => 3,
        })),
        (ENTITY_TYPE, "getDimensions") => {
            let (w, h) = size(rt, a(0));
            let d = rt.new_object(DIMENSIONS);
            rt.put_field(d, "width", V::F(w as f32));
            rt.put_field(d, "height", V::F(h as f32));
            Some(d)
        }
        (ENTITY_TYPE, "getDescriptionId") => {
            let id = description_id(rt, a(0));
            Some(rt.string(&id))
        }
        (ENTITY_TYPE, "getDescription") => {
            let id = description_id(rt, a(0));
            let key = rt.string(&id);
            Some(crate::api_common::component(rt, key, true, V::NULL))
        }

        // Synched data
        ("net/minecraft/network/syncher/SynchedEntityData", "defineId") => {
            let id = define_id(rt, a(0));
            let acc = rt.new_object(ACCESSOR);
            rt.put_field(acc, "$id", V::I(id));
            rt.put_field(acc, "$serializer", a(1));
            Some(acc)
        }
        // A mod's own serializer (`EntityDataSerializer.forValueType(codec)`), and any one's codec.
        ("net/minecraft/network/syncher/EntityDataSerializer", "forValueType") => {
            let s = rt.new_object("net/minecraft/network/syncher/EntityDataSerializer");
            rt.put_field(s, "$codec", a(0));
            Some(s)
        }
        ("net/minecraft/network/syncher/EntityDataSerializer", "codec") => Some(rt.get_field(a(0), "$codec")),
        ("net/minecraft/network/syncher/EntityDataSerializer", "createAccessor") => {
            let acc = rt.new_object(ACCESSOR);
            rt.put_field(acc, "$id", a(1));
            rt.put_field(acc, "$serializer", a(0));
            Some(acc)
        }
        (ACCESSOR, "id" | "getId") => Some(V::I(rt.get_field(a(0), "$id").i())),
        (ACCESSOR, "serializer" | "getSerializer") => Some(rt.get_field(a(0), "$serializer")),
        (ACCESSOR, "hashCode") => Some(V::I(rt.get_field(a(0), "$id").i())),
        (ACCESSOR, "equals") => Some(V::I((rt.class_of(a(1)) == ACCESSOR && rt.get_field(a(0), "$id") == rt.get_field(a(1), "$id")) as i32)),
        (SYNCHED_BUILDER, "define") => {
            let id = rt.get_field(a(1), "$id").i().max(0) as usize;
            let value = a(2);
            if let Some(items) = data_items(rt, a(0)) {
                if items.len() <= id {
                    items.resize(id + 1, V::NULL);
                }
                items[id] = value;
            }
            // Each value's serializer, by index, for what players are sent.
            let sers = rt.get_field(a(0), "$serializers");
            let ser = rt.get_field(a(1), "$serializer");
            if let Some(items) = data_items(rt, sers) {
                if items.len() <= id {
                    items.resize(id + 1, V::NULL);
                }
                items[id] = ser;
            }
            Some(a(0))
        }
        (SYNCHED, "get") => {
            let id = rt.get_field(a(1), "$id").i().max(0) as usize;
            Some(data_items(rt, a(0)).and_then(|items| items.get(id).copied()).unwrap_or(V::NULL))
        }
        (SYNCHED, "set") => {
            let (data, acc, value) = (a(0), a(1), a(2));
            let force = desc.ends_with("Z)V") && a(3).i() != 0;
            let id = rt.get_field(acc, "$id").i().max(0) as usize;
            let old = data_items(rt, data).and_then(|items| items.get(id).copied()).unwrap_or(V::NULL);
            if force || !same_value(rt, old, value) {
                if let Some(items) = data_items(rt, data) {
                    if items.len() <= id {
                        items.resize(id + 1, V::NULL);
                    }
                    items[id] = value;
                }
                rt.put_field(data, "$dirty", V::I(1));
                let e = rt.get_field(data, "$entity");
                virtual_call(rt, e, ON_SYNCED_KEY, ON_SYNCED, &[acc]);
            }
            Some(V::Void)
        }
        (SYNCHED, "isDirty") => Some(V::I(rt.get_field(a(0), "$dirty").i())),

        // The entity's own state, for a mod's entity.
        (ENTITY, "<init>") if desc == "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V" => {
            construct(rt, a(0), a(1), a(2));
            Some(V::Void)
        }
        (ENTITY, _) if is_mod_entity(rt, a(0)) => entity_call(rt, name, desc, args),
        _ => None,
    }
}

fn description_id(rt: &Rt, ty: V) -> String {
    let name = rt.registered_name(ty, "entity_type").unwrap_or("minecraft:pig");
    let (ns, path) = name.split_once(':').unwrap_or(("minecraft", name));
    format!("entity.{ns}.{}", path.replace('/', "."))
}

/// What vanilla's `Entity` answers for a mod's entity.
fn entity_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    Some(match name {
        "getX" if desc == "()D" => rt.get_field(e, "$x"),
        "getY" if desc == "()D" => rt.get_field(e, "$y"),
        "getZ" if desc == "()D" => rt.get_field(e, "$z"),
        "position" => {
            let p = xyz(rt, e);
            vec3(rt, p)
        }
        "blockPosition" | "getOnPos" => {
            let p = xyz(rt, e);
            let below = if name == "getOnPos" { 0.2 } else { 0.0 };
            api::block_pos(rt, p[0].floor() as i64, (p[1] - below).floor() as i64, p[2].floor() as i64)
        }
        "setPos" if desc == "(DDD)V" => {
            base_set_pos(rt, e, [a(1).d(), a(2).d(), a(3).d()]);
            V::Void
        }
        "setPos" if desc == "(Lnet/minecraft/world/phys/Vec3;)V" => {
            let p = of_vec3(rt, a(1));
            set_pos(rt, e, p);
            V::Void
        }
        "setPosRaw" => {
            set_pos_raw(rt, e, [a(1).d(), a(2).d(), a(3).d()]);
            V::Void
        }
        "moveTo" | "absMoveTo" if desc.starts_with("(DDD") => {
            let p = [a(1).d(), a(2).d(), a(3).d()];
            for f in ["xo", "xOld"] {
                rt.put_field(e, f, V::D(p[0]));
            }
            for f in ["yo", "yOld"] {
                rt.put_field(e, f, V::D(p[1]));
            }
            for f in ["zo", "zOld"] {
                rt.put_field(e, f, V::D(p[2]));
            }
            if desc.starts_with("(DDDFF") {
                rt.put_field(e, "$yRot", a(4));
                rt.put_field(e, "$xRot", a(5));
            }
            set_pos(rt, e, p);
            V::Void
        }
        "teleportTo" if desc == "(DDD)V" => {
            set_pos(rt, e, [a(1).d(), a(2).d(), a(3).d()]);
            V::Void
        }
        // A client's interpolation toward a server's position: there at once.
        "lerpTo" => {
            set_pos(rt, e, [a(1).d(), a(2).d(), a(3).d()]);
            rt.put_field(e, "$yRot", a(4));
            rt.put_field(e, "$xRot", a(5));
            V::Void
        }
        "move" => {
            let d = of_vec3(rt, a(2));
            let p = xyz(rt, e);
            set_pos(rt, e, [p[0] + d[0], p[1] + d[1], p[2] + d[2]]);
            V::Void
        }
        "getBoundingBox" | "getBoundingBoxForCulling" => match rt.get_field(e, "$bb") {
            bb if bb.r() != 0 => bb,
            _ => {
                let b = make_box(rt, e, xyz(rt, e));
                crate::shapes::aabb(rt, b)
            }
        },
        "setBoundingBox" => {
            rt.put_field(e, "$bb", a(1));
            V::Void
        }
        "getDeltaMovement" => rt.get_field(e, "$motion"),
        "setDeltaMovement" if desc == "(Lnet/minecraft/world/phys/Vec3;)V" => {
            rt.put_field(e, "$motion", a(1));
            V::Void
        }
        "setDeltaMovement" => {
            let m = vec3(rt, [a(1).d(), a(2).d(), a(3).d()]);
            rt.put_field(e, "$motion", m);
            V::Void
        }
        "getYRot" => rt.get_field(e, "$yRot"),
        "getXRot" => rt.get_field(e, "$xRot"),
        "setYRot" => {
            rt.put_field(e, "$yRot", a(1));
            V::Void
        }
        "setXRot" => {
            rt.put_field(e, "$xRot", a(1));
            V::Void
        }
        "setOldPosAndRot" => {
            set_old(rt, e);
            V::Void
        }
        "getId" => rt.get_field(e, "$id"),
        "setId" => {
            rt.put_field(e, "$id", a(1));
            V::Void
        }
        "getUUID" => rt.get_field(e, "uuid"),
        "setUUID" => {
            rt.put_field(e, "uuid", a(1));
            V::Void
        }
        "getStringUUID" => {
            let u = rt.get_field(e, "uuid");
            api::call(rt, "java/util/UUID.toString.()Ljava/lang/String;", &[u])
        }
        "level" | "getCommandSenderWorld" => rt.get_field(e, "$level"),
        "getType" => rt.get_field(e, "$type"),
        "isAlive" => V::I((rt.get_field(e, "removalReason").r() == 0) as i32),
        "isRemoved" => V::I((rt.get_field(e, "removalReason").r() != 0) as i32),
        "getRemovalReason" => rt.get_field(e, "removalReason"),
        "discard" => {
            let r = removal(rt, 1);
            remove(rt, e, r);
            V::Void
        }
        "kill" => {
            let r = removal(rt, 0);
            remove(rt, e, r);
            V::Void
        }
        "remove" | "setRemoved" => {
            if rt.get_field(e, "removalReason").r() == 0 {
                rt.put_field(e, "removalReason", a(1));
            }
            V::Void
        }
        "onRemovedFromLevel" | "onClientRemoval" | "ejectPassengers" | "baseTick" | "tick" | "onAddedToLevel" | "refreshDimensions" => V::Void,
        "getPassengers" => crate::jutil::new_list(rt, Vec::new()),
        "hasPassenger" | "isPassenger" | "isVehicle" | "isSpectator" | "isAlwaysTicking" | "isOnFire" | "isInvisible" | "isSilent" | "isNoGravity" | "noCulling" => V::I(0),
        "getVehicle" | "getControllingPassenger" | "getFirstPassenger" | "getRootVehicle" if desc.ends_with("Lnet/minecraft/world/entity/Entity;") && name != "getRootVehicle" => V::NULL,
        "getRootVehicle" => e,
        "shouldRender" | "shouldRenderAtSqrDistance" | "isPickable" | "isAttackable" => V::I((name != "isPickable" && name != "isAttackable") as i32),
        "getName" | "getDisplayName" => {
            let ty = rt.get_field(e, "$type");
            let id = description_id(rt, ty);
            let key = rt.string(&id);
            crate::api_common::component(rt, key, true, V::NULL)
        }
        "getPistonPushReaction" => crate::blocks::push_reaction_named(rt, "normal"),
        "onBelowWorld" | "outOfWorld" => {
            let r = removal(rt, 1);
            remove(rt, e, r);
            V::Void
        }
        "getEyeY" => V::D(rt.get_field(e, "$y").d() + size(rt, rt.get_field(e, "$type")).1 * 0.85),
        "getEyePosition" if desc == "()Lnet/minecraft/world/phys/Vec3;" => {
            let p = xyz(rt, e);
            let h = size(rt, rt.get_field(e, "$type")).1 * 0.85;
            vec3(rt, [p[0], p[1] + h, p[2]])
        }
        "registryAccess" => {
            let l = rt.get_field(e, "$level");
            api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[l])
        }
        "getPersistentData" => {
            if rt.get_field(e, "$persistent").r() == 0 {
                let t = new_compound(rt);
                rt.put_field(e, "$persistent", t);
            }
            rt.get_field(e, "$persistent")
        }
        "saveWithoutId" => {
            save(rt, e, a(1));
            a(1)
        }
        "load" => {
            load(rt, e, a(1));
            V::Void
        }
        _ => return None,
    })
}

fn new_compound(rt: &mut Rt) -> V {
    let t = rt.new_object("net/minecraft/nbt/CompoundTag");
    api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[t]);
    t
}

fn set_old(rt: &mut Rt, e: V) {
    let p = xyz(rt, e);
    for (fs, v) in [(["xo", "xOld"], p[0]), (["yo", "yOld"], p[1]), (["zo", "zOld"], p[2])] {
        for f in fs {
            rt.put_field(e, f, V::D(v));
        }
    }
    let (y, x) = (rt.get_field(e, "$yRot"), rt.get_field(e, "$xRot"));
    rt.put_field(e, "yRotO", y);
    rt.put_field(e, "xRotO", x);
}

/// `entity.remove(reason)`, virtually (a subclass's own runs first: Create's contraptions stop
/// their actors), then vanilla's: the entity leaves its level at the end of the tick.
fn remove(rt: &mut Rt, e: V, reason: V) {
    if virtual_call(rt, e, REMOVE_KEY, REMOVE, &[reason]).is_none() && rt.get_field(e, "removalReason").r() == 0 {
        rt.put_field(e, "removalReason", reason);
    }
    rt.report_thrown("remove");
}

/// `Entity.saveWithoutId`: where it is and how it moves, its id, then the subclass's own
/// (`addAdditionalSaveData`).
fn save(rt: &mut Rt, e: V, tag: V) {
    let c = |rt: &mut Rt, k: &'static str, args: &[V]| api::call(rt, k, args);
    let p = xyz(rt, e);
    let m = of_vec3(rt, rt.get_field(e, "$motion"));
    for (key, values) in [("Pos", p), ("Motion", m)] {
        let list = rt.new_object("net/minecraft/nbt/ListTag");
        c(rt, "net/minecraft/nbt/ListTag.<init>.()V", &[list]);
        for v in values {
            let d = c(rt, "net/minecraft/nbt/DoubleTag.valueOf.(D)Lnet/minecraft/nbt/DoubleTag;", &[V::D(v)]);
            c(rt, "net/minecraft/nbt/ListTag.add.(Ljava/lang/Object;)Z", &[list, d]);
        }
        let k = rt.string(key);
        c(rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[tag, k, list]);
    }
    let u = rt.get_field(e, "uuid");
    let k = rt.string("UUID");
    c(rt, "net/minecraft/nbt/CompoundTag.putUUID.(Ljava/lang/String;Ljava/util/UUID;)V", &[tag, k, u]);
    virtual_call(rt, e, ADDITIONAL_SAVE_KEY, ADDITIONAL_SAVE, &[tag]);
    rt.report_thrown("addAdditionalSaveData");
}

/// `Entity.load`: [`save`]'s keys back, then the subclass's own (`readAdditionalSaveData`).
fn load(rt: &mut Rt, e: V, tag: V) {
    let c = |rt: &mut Rt, k: &'static str, args: &[V]| api::call(rt, k, args);
    let k = rt.string("Pos");
    let list = c(rt, "net/minecraft/nbt/CompoundTag.getList.(Ljava/lang/String;I)Lnet/minecraft/nbt/ListTag;", &[tag, k, V::I(6)]);
    let p: Vec<f64> = (0..3).map(|i| c(rt, "net/minecraft/nbt/ListTag.getDouble.(I)D", &[list, V::I(i)]).d()).collect();
    if p.len() == 3 {
        set_pos(rt, e, [p[0], p[1], p[2]]);
        set_old(rt, e);
    }
    let k = rt.string("UUID");
    if c(rt, "net/minecraft/nbt/CompoundTag.hasUUID.(Ljava/lang/String;)Z", &[tag, k]).i() != 0 {
        let u = c(rt, "net/minecraft/nbt/CompoundTag.getUUID.(Ljava/lang/String;)Ljava/util/UUID;", &[tag, k]);
        rt.put_field(e, "uuid", u);
    }
    virtual_call(rt, e, ADDITIONAL_LOAD_KEY, ADDITIONAL_LOAD, &[tag]);
    rt.report_thrown("readAdditionalSaveData");
}

/// The level's tick for mod entities in dimension `dimension`, as `ServerLevel.tickNonPassenger`
/// (and the client's) runs each: the last position kept, a tick counted, `tick()`; then those
/// removed leave the list.
pub fn tick_all(rt: &mut Rt, dimension: &str) {
    for e in live(rt) {
        if rt.get_field(e, "removalReason").r() != 0 || rt.str_of(rt.get_field(e, "$dimension")).is_some_and(|d| d != dimension) {
            continue;
        }
        set_old(rt, e);
        let t = rt.get_field(e, "tickCount").i();
        rt.put_field(e, "tickCount", V::I(t + 1));
        virtual_call(rt, e, TICK_KEY, TICK, &[]);
        rt.report_thrown("Entity.tick");
    }
    // A server's: those removed are sent their removal (`track` sends none to a removed entity's
    // players), then leave the list.
    if rt.host().is_some_and(|h| h.is_server()) {
        track(rt, dimension);
    }
    let kept: Vec<V> = live(rt).into_iter().filter(|&e| rt.get_field(e, "removalReason").r() == 0).collect();
    if kept.len() != live(rt).len() {
        set_live(rt, kept);
    }
}

/// The mod entities whose boxes meet `bounds` (min x, y, z, max x, y, z), for the level's queries.
pub fn in_box(rt: &Rt, bounds: [f64; 6]) -> Vec<V> {
    live(rt)
        .into_iter()
        .filter(|&e| rt.get_field(e, "removalReason").r() == 0)
        .filter(|&e| {
            let bb = rt.get_field(e, "$bb");
            let b = if bb.r() != 0 { crate::shapes::aabb_of(rt, bb) } else { make_box(rt, e, xyz(rt, e)) };
            crate::host::boxes_meet(&b, &bounds)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// What players' clients see. The server's runtime keeps which players it sent each entity to (as
// vanilla's `ChunkMap.TrackedEntity` does) and sends them its spawn, moves, synched data and
// removal on the mod message channel; the client's runtime makes, moves and removes copies of its
// own, from the entity's type (`EntityType.create`) and the mod's spawn data
// (`IEntityWithComplexSpawn`).

/// The channel the runtime's own entity messages go on.
pub const CHANNEL: &str = "horizon_compat:entity";
const SPAWN: u8 = 0;
const MOVE: u8 = 1;
const DATA: u8 = 2;
const GONE: u8 = 3;
const PART: u8 = 4;
/// Where a message's synched values end, as vanilla's `SynchedEntityData` packs them.
const DATA_END: u8 = 0xFF;
/// The most bytes one message carries, under the game's cap (`mod_net::MAX_DATA`, 30 000).
const MAX_MESSAGE: usize = 29_000;
const WRITE_SPAWN: &str = "writeSpawnData.(Lnet/minecraft/network/RegistryFriendlyByteBuf;)V";
const WRITE_SPAWN_KEY: &str = "net/neoforged/neoforge/entity/IEntityWithComplexSpawn.writeSpawnData.(Lnet/minecraft/network/RegistryFriendlyByteBuf;)V";
const READ_SPAWN: &str = "readSpawnData.(Lnet/minecraft/network/RegistryFriendlyByteBuf;)V";
const READ_SPAWN_KEY: &str = "net/neoforged/neoforge/entity/IEntityWithComplexSpawn.readSpawnData.(Lnet/minecraft/network/RegistryFriendlyByteBuf;)V";
const LERP: &str = "lerpTo.(DDDFFI)V";
const LERP_KEY: &str = "net/minecraft/world/entity/Entity.lerpTo.(DDDFFI)V";
const CLIENT_REMOVAL: &str = "onClientRemoval.()V";
const CLIENT_REMOVAL_KEY: &str = "net/minecraft/world/entity/Entity.onClientRemoval.()V";

/// The players a server's entity was sent to.
fn sent_to(rt: &Rt, e: V) -> Vec<u128> {
    rt.str_of(rt.get_field(e, "$sentTo"))
        .unwrap_or("")
        .split(',')
        .filter_map(|h| u128::from_str_radix(h, 16).ok())
        .collect()
}

fn set_sent_to(rt: &mut Rt, e: V, players: &[u128]) {
    let text: Vec<String> = players.iter().map(|p| format!("{p:x}")).collect();
    let s = rt.string(&text.join(","));
    rt.put_field(e, "$sentTo", s);
}

fn put_f64(rt: &mut Rt, buf: V, d: f64) {
    crate::network::put(rt, buf, &d.to_be_bytes());
}

fn put_f32(rt: &mut Rt, buf: V, f: f32) {
    crate::network::put(rt, buf, &f.to_be_bytes());
}

fn read_f64(rt: &mut Rt, buf: V) -> f64 {
    crate::network::take_n::<8>(rt, buf).map_or(0.0, f64::from_be_bytes)
}

fn read_f32(rt: &mut Rt, buf: V) -> f32 {
    crate::network::take_n::<4>(rt, buf).map_or(0.0, f32::from_be_bytes)
}

fn read_int(rt: &mut Rt, buf: V) -> i32 {
    crate::network::read_var_int(rt, buf).unwrap_or(0)
}

/// Where an entity is and looks: x, y, z, then `yRot` and `xRot`.
fn place(rt: &Rt, e: V) -> ([f64; 3], [f32; 2]) {
    (xyz(rt, e), [rt.get_field(e, "$yRot").f(), rt.get_field(e, "$xRot").f()])
}

/// The synched values that have a codec, after vanilla `Entity`'s own: index, then the value.
fn put_data(rt: &mut Rt, buf: V, e: V) {
    let data = rt.get_field(e, "entityData");
    let items = data_items(rt, data).cloned().unwrap_or_default();
    let sers = match rt.heap.get(rt.get_field(data, "$serializers").r() as usize).map(|o| &o.data) {
        Some(Data::Array(s)) => s.clone(),
        _ => Vec::new(),
    };
    for (i, value) in items.iter().enumerate().skip(ENTITY_DATA as usize).take(DATA_END as usize - ENTITY_DATA as usize) {
        let codec = sers.get(i).map_or(V::NULL, |&s| rt.get_field(s, "$codec"));
        if codec.r() == 0 || value.r() == 0 && matches!(value, V::R(_)) {
            continue;
        }
        crate::network::put(rt, buf, &[i as u8]);
        crate::network::encode(rt, codec, buf, *value);
        if rt.thrown.is_some() {
            rt.report_thrown("encoding an entity's synched data");
            break;
        }
    }
    crate::network::put(rt, buf, &[DATA_END]);
}

/// [`put_data`]'s values into `e`'s synched data, each heard by the entity
/// (`onSyncedDataUpdated`), as `SynchedEntityData.assignValues` sets them.
fn read_data(rt: &mut Rt, buf: V, e: V) {
    let data = rt.get_field(e, "entityData");
    let sers = match rt.heap.get(rt.get_field(data, "$serializers").r() as usize).map(|o| &o.data) {
        Some(Data::Array(s)) => s.clone(),
        _ => Vec::new(),
    };
    loop {
        let Some([i]) = crate::network::take_n::<1>(rt, buf) else { return };
        if i == DATA_END {
            return;
        }
        let codec = sers.get(i as usize).map_or(V::NULL, |&s| rt.get_field(s, "$codec"));
        if codec.r() == 0 {
            // A value this side can't read: what follows can't be found either.
            return;
        }
        let value = crate::network::decode(rt, codec, buf);
        if rt.thrown.is_some() {
            rt.report_thrown("decoding an entity's synched data");
            return;
        }
        if let Some(items) = data_items(rt, data) {
            if items.len() <= i as usize {
                items.resize(i as usize + 1, V::NULL);
            }
            items[i as usize] = value;
        }
        let acc = rt.new_object(ACCESSOR);
        rt.put_field(acc, "$id", V::I(i32::from(i)));
        virtual_call(rt, e, ON_SYNCED_KEY, ON_SYNCED, &[acc]);
        rt.report_thrown("onSyncedDataUpdated");
    }
}

/// An entity's spawn message: who and where it is, its synched data, then the mod's spawn data.
fn spawn_message(rt: &mut Rt, e: V) -> Vec<u8> {
    let buf = crate::network::new_buffer(rt);
    crate::network::put(rt, buf, &[SPAWN]);
    let id = rt.get_field(e, "$id").i();
    crate::network::put_var(rt, buf, id as u32 as u64);
    let (msb, lsb) = crate::tags::uuid_bits(rt, rt.get_field(e, "uuid"));
    crate::network::put(rt, buf, &msb.to_be_bytes());
    crate::network::put(rt, buf, &lsb.to_be_bytes());
    let ty = rt.get_field(e, "$type");
    let name = rt.registered_name(ty, "entity_type").unwrap_or("").to_owned();
    crate::network::put_utf(rt, buf, &name);
    let (p, r) = place(rt, e);
    for d in p {
        put_f64(rt, buf, d);
    }
    for f in r {
        put_f32(rt, buf, f);
    }
    put_data(rt, buf, e);
    virtual_call(rt, e, WRITE_SPAWN_KEY, WRITE_SPAWN, &[buf]);
    rt.report_thrown("writeSpawnData");
    crate::network::bytes(rt, buf)
}

/// Sends `message` to `player`, in parts when it is past one message's size.
fn send_to(rt: &mut Rt, player: u128, message: &[u8]) {
    let target = crate::host::PayloadTarget::Player(player);
    let Some(host) = rt.host_mut() else { return };
    if message.len() <= MAX_MESSAGE {
        host.send_payload(target, CHANNEL, message);
        return;
    }
    let seq = (message.len() as u64).wrapping_mul(31).wrapping_add(u64::from(message[1])) & 0x0FFF_FFFF;
    let parts: Vec<&[u8]> = message.chunks(MAX_MESSAGE - 16).collect();
    for (i, part) in parts.iter().enumerate() {
        let mut out = vec![PART];
        for n in [seq, i as u64, parts.len() as u64] {
            let mut u = n;
            loop {
                if u < 0x80 {
                    out.push(u as u8);
                    break;
                }
                out.push((u as u8 & 0x7F) | 0x80);
                u >>= 7;
            }
        }
        out.extend_from_slice(part);
        host.send_payload(target, CHANNEL, &out);
    }
}

/// The server's tracker, once a tick after its entities ticked: each player in an entity's range
/// is sent it once, then its moves and changed data; one who left the range, or whose entity was
/// removed, its removal.
pub fn track(rt: &mut Rt, dimension: &str) {
    let players = rt.host().map(|h| h.players()).unwrap_or_default();
    let mut moves: Vec<(u128, Vec<u8>)> = Vec::new();
    for e in live(rt) {
        if rt.str_of(rt.get_field(e, "$dimension")).is_some_and(|d| d != dimension) {
            continue;
        }
        let removed = rt.get_field(e, "removalReason").r() != 0;
        let ty = rt.get_field(e, "$type");
        let range = f64::from(match rt.get_field(ty, "$range") {
            V::I(r) => r,
            _ => 5,
        }) * 16.0;
        let (p, r) = place(rt, e);
        let before = sent_to(rt, e);
        let mut now = Vec::new();
        for &(player, at) in &players {
            let (dx, dz) = (at[0] - p[0], at[2] - p[2]);
            if !removed && dx * dx + dz * dz <= range * range {
                now.push(player);
            }
        }
        let id = rt.get_field(e, "$id").i();
        for &player in before.iter().filter(|p| !now.contains(p)) {
            let mut m = vec![GONE, 1];
            m.extend(var(id as u32 as u64));
            send_to(rt, player, &m);
        }
        let fresh: Vec<u128> = now.iter().copied().filter(|p| !before.contains(p)).collect();
        if !fresh.is_empty() {
            let m = spawn_message(rt, e);
            for &player in &fresh {
                send_to(rt, player, &m);
            }
        }
        let kept: Vec<u128> = now.iter().copied().filter(|p| before.contains(p)).collect();
        // Moved since it was last sent: the players who had it get where it is now.
        let last = (rt.get_field(e, "$sentX"), rt.get_field(e, "$sentY"), rt.get_field(e, "$sentZ"), rt.get_field(e, "$sentYRot"), rt.get_field(e, "$sentXRot"));
        let moved = last != (V::D(p[0]), V::D(p[1]), V::D(p[2]), V::F(r[0]), V::F(r[1]));
        if moved {
            let mut m = var(id as u32 as u64);
            for d in p {
                m.extend(d.to_be_bytes());
            }
            for f in r {
                m.extend(f.to_be_bytes());
            }
            for &player in &kept {
                moves.push((player, m.clone()));
            }
        }
        rt.put_field(e, "$sentX", V::D(p[0]));
        rt.put_field(e, "$sentY", V::D(p[1]));
        rt.put_field(e, "$sentZ", V::D(p[2]));
        rt.put_field(e, "$sentYRot", V::F(r[0]));
        rt.put_field(e, "$sentXRot", V::F(r[1]));
        let data = rt.get_field(e, "entityData");
        if rt.get_field(data, "$dirty").i() != 0 {
            rt.put_field(data, "$dirty", V::I(0));
            if !kept.is_empty() {
                let buf = crate::network::new_buffer(rt);
                crate::network::put(rt, buf, &[DATA]);
                crate::network::put_var(rt, buf, id as u32 as u64);
                put_data(rt, buf, e);
                let m = crate::network::bytes(rt, buf);
                for &player in &kept {
                    send_to(rt, player, &m);
                }
            }
        }
        set_sent_to(rt, e, &now);
    }
    // One message of moves for each player.
    let mut by_player: Vec<(u128, Vec<Vec<u8>>)> = Vec::new();
    for (player, m) in moves {
        match by_player.iter_mut().find(|(p, _)| *p == player) {
            Some((_, ms)) => ms.push(m),
            None => by_player.push((player, vec![m])),
        }
    }
    for (player, ms) in by_player {
        for chunk in ms.chunks(MAX_MESSAGE / 40) {
            let mut out = vec![MOVE];
            out.extend(var(chunk.len() as u64));
            for m in chunk {
                out.extend_from_slice(m);
            }
            send_to(rt, player, &out);
        }
    }
}

fn var(mut u: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        if u < 0x80 {
            out.push(u as u8);
            return out;
        }
        out.push((u as u8 & 0x7F) | 0x80);
        u >>= 7;
    }
}

/// A message of [`CHANNEL`] the server sent the client's runtime.
pub fn receive(rt: &mut Rt, data: &[u8]) {
    let buf = crate::network::buffer_with(rt, data.to_vec());
    let Some([op]) = crate::network::take_n::<1>(rt, buf) else { return };
    match op {
        PART => {
            let (seq, i, n) = (read_int(rt, buf), read_int(rt, buf), read_int(rt, buf));
            let rest = data[data.len() - remaining(rt, buf)..].to_vec();
            let key = format!("compat:entity-parts/{seq}");
            let mut parts: Vec<Option<Vec<u8>>> = match rt.interned.get(&key).copied() {
                Some(v) => match rt.heap.get(v.r() as usize).map(|o| &o.data) {
                    Some(Data::Array(items)) => items.iter().map(|&p| Some(crate::network::bytes(rt, p)).filter(|_| p.r() != 0)).collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            parts.resize(n.max(1) as usize, None);
            if let Some(slot) = parts.get_mut(i.max(0) as usize) {
                *slot = Some(rest);
            }
            if parts.iter().all(Option::is_some) {
                rt.interned.remove(&key);
                let whole: Vec<u8> = parts.into_iter().flatten().flatten().collect();
                receive(rt, &whole);
            } else {
                let items: Vec<V> = parts.into_iter().map(|p| p.map_or(V::NULL, |b| crate::network::buffer_with(rt, b))).collect();
                let held = rt.alloc("[Ljava/lang/Object;", Data::Array(items));
                rt.interned.insert(key, held);
            }
        }
        SPAWN => client_spawn(rt, buf),
        MOVE => {
            for _ in 0..read_int(rt, buf) {
                let id = read_int(rt, buf);
                let p = [read_f64(rt, buf), read_f64(rt, buf), read_f64(rt, buf)];
                let r = [read_f32(rt, buf), read_f32(rt, buf)];
                if let Some(e) = by_id(rt, id) {
                    let args = [V::D(p[0]), V::D(p[1]), V::D(p[2]), V::F(r[0]), V::F(r[1]), V::I(3)];
                    if virtual_call(rt, e, LERP_KEY, LERP, &args).is_none() {
                        set_pos(rt, e, p);
                        rt.put_field(e, "$yRot", V::F(r[0]));
                        rt.put_field(e, "$xRot", V::F(r[1]));
                    }
                    rt.report_thrown("lerpTo");
                }
            }
        }
        DATA => {
            let id = read_int(rt, buf);
            if let Some(e) = by_id(rt, id) {
                read_data(rt, buf, e);
            }
        }
        GONE => {
            for _ in 0..read_int(rt, buf) {
                let id = read_int(rt, buf);
                if let Some(e) = by_id(rt, id) {
                    virtual_call(rt, e, CLIENT_REMOVAL_KEY, CLIENT_REMOVAL, &[]);
                    rt.report_thrown("onClientRemoval");
                    let r = removal(rt, 1);
                    rt.put_field(e, "removalReason", r);
                    let kept: Vec<V> = live(rt).into_iter().filter(|&x| x != e).collect();
                    set_live(rt, kept);
                }
            }
        }
        _ => {}
    }
}

fn remaining(rt: &Rt, buf: V) -> usize {
    match rt.heap.get(buf.r() as usize).map(|o| &o.data) {
        Some(Data::Bytes(v, at)) => v.len().saturating_sub(*at),
        _ => 0,
    }
}

fn by_id(rt: &Rt, id: i32) -> Option<V> {
    live(rt).into_iter().find(|&e| rt.get_field(e, "$id").i() == id)
}

/// A spawn message: the entity made from its type in the client's level, given the server's id,
/// UUID and place, its synched data, then its spawn data (`readSpawnData`).
fn client_spawn(rt: &mut Rt, buf: V) {
    let id = read_int(rt, buf);
    let msb = crate::network::take_n::<8>(rt, buf).map_or(0, i64::from_be_bytes);
    let lsb = crate::network::take_n::<8>(rt, buf).map_or(0, i64::from_be_bytes);
    let name = crate::network::read_utf(rt, buf).unwrap_or_default();
    let p = [read_f64(rt, buf), read_f64(rt, buf), read_f64(rt, buf)];
    let r = [read_f32(rt, buf), read_f32(rt, buf)];
    let Some(&ty) = rt.registered.get(&("entity_type".to_owned(), name.clone())) else {
        crate::glue::trace_once(rt, &format!("entity-type-unknown/{name}"), |_| format!("entity spawned of unknown type {name}"));
        return;
    };
    if let Some(old) = by_id(rt, id) {
        let kept: Vec<V> = live(rt).into_iter().filter(|&x| x != old).collect();
        set_live(rt, kept);
    }
    let level = crate::glue::level(rt);
    let e = api::call(rt, "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;", &[ty, level]);
    rt.report_thrown("EntityType.create");
    if !is_mod_entity(rt, e) {
        return;
    }
    rt.put_field(e, "$id", V::I(id));
    let u = crate::tags::uuid(rt, msb, lsb);
    rt.put_field(e, "uuid", u);
    rt.put_field(e, "$yRot", V::F(r[0]));
    rt.put_field(e, "$xRot", V::F(r[1]));
    set_pos(rt, e, p);
    set_old(rt, e);
    read_data(rt, buf, e);
    virtual_call(rt, e, READ_SPAWN_KEY, READ_SPAWN, &[buf]);
    rt.report_thrown("readSpawnData");
    let mut items = live(rt);
    items.push(e);
    set_live(rt, items);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const CART: &str = "test/Cart";
    const MINECART: &str = "test/FastCart";
    const GET: &str = "net/minecraft/network/syncher/SynchedEntityData.get.(Lnet/minecraft/network/syncher/EntityDataAccessor;)Ljava/lang/Object;";
    const SET: &str = "net/minecraft/network/syncher/SynchedEntityData.set.(Lnet/minecraft/network/syncher/EntityDataAccessor;Ljava/lang/Object;)V";
    const DEFINE: &str = "net/minecraft/network/syncher/SynchedEntityData$Builder.define.(Lnet/minecraft/network/syncher/EntityDataAccessor;Ljava/lang/Object;)Lnet/minecraft/network/syncher/SynchedEntityData$Builder;";
    const DEFINE_ID: &str = "net/minecraft/network/syncher/SynchedEntityData.defineId.(Ljava/lang/Class;Lnet/minecraft/network/syncher/EntityDataSerializer;)Lnet/minecraft/network/syncher/EntityDataAccessor;";
    const FACTORY_CREATE: &str = "create.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";

    /// A cart as a mod writes one: it counts its ticks in synched data and leaves after three.
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let e = args[0];
        let count = rt.statics.get("t.COUNT").copied().unwrap_or_default();
        match key {
            DEFINE_SYNCHED => {
                let zero = crate::api_common::integer(rt, 0);
                api::call(rt, DEFINE, &[args[1], count, zero]);
                V::Void
            }
            TICK => {
                let data = rt.get_field(e, "entityData");
                let n = api::call(rt, GET, &[data, count]);
                let n = rt.get_field(n, "value").i();
                let next = crate::api_common::integer(rt, n + 1);
                api::call(rt, SET, &[data, count, next]);
                if rt.get_field(e, "tickCount").i() == 3 {
                    api::call(rt, "net/minecraft/world/entity/Entity.discard.()V", &[e]);
                }
                V::Void
            }
            ON_SYNCED => {
                let n = rt.statics.get("t.heard").copied().unwrap_or(V::I(0)).i();
                rt.statics.insert("t.heard", V::I(n + 1));
                V::Void
            }
            FACTORY_CREATE if rt.class_of(e) == "test/CartFactory" => {
                let cart = rt.new_object(CART);
                api::call(rt, "net/minecraft/world/entity/Entity.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V", &[cart, args[1], args[2]]);
                cart
            }
            _ => api::call(rt, owner_key, args),
        }
    }

    fn overrides(class: &str, key: &str) -> bool {
        (class == CART || class == MINECART) && matches!(key, DEFINE_SYNCHED | TICK | ON_SYNCED)
    }

    /// A runtime as the mod's: the cart's class, its synched count (an `INT`), its type registered
    /// as `test:cart`, sent within 4 chunks.
    fn runtime() -> (Rt, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        rt.supers.insert(CART, ENTITY);
        let int = api::get_static(&mut rt, "net/minecraft/network/syncher/EntityDataSerializers.INT");
        let class = rt.string(CART);
        let count = api::call(&mut rt, DEFINE_ID, &[class, int]);
        rt.statics.insert("t.COUNT", count);
        let ty = rt.new_object(ENTITY_TYPE);
        let factory = rt.new_object("test/CartFactory");
        rt.put_field(ty, "$factory", factory);
        rt.put_field(ty, "$range", V::I(4));
        rt.registered.insert(("entity_type".into(), "test:cart".into()), ty);
        (rt, ty)
    }

    /// The server's tracker sends a player in range the cart, where it moves and its count, then its
    /// removal; the client's runtime keeps a copy of it that follows. A player out of range gets
    /// nothing.
    #[test]
    fn a_client_is_sent_the_entities_in_its_range_and_follows_them_until_they_go() {
        let (mut server, ty) = runtime();
        let (mut client, _) = runtime();
        let mut host = MemoryHost { players: vec![(1, [0.0, 64.0, 0.0]), (2, [500.0, 64.0, 0.0])], ..MemoryHost::default() };
        let cart = server.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let cart = api::call(rt, "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;", &[ty, level]);
            set_pos(rt, cart, [0.5, 64.0, 0.5]);
            api::call(rt, "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z", &[level, cart]);
            cart
        });
        let mut client_host = MemoryHost::default();
        let mut deliver = |server: &mut Rt, host: &mut MemoryHost, client: &mut Rt, step: &dyn Fn(&mut Rt)| {
            server.with_host(host, |rt| {
                step(rt);
                tick_all(rt, "minecraft:overworld");
            });
            for (to, channel, data) in std::mem::take(&mut host.payloads) {
                assert_eq!(channel, CHANNEL);
                assert_eq!(to, crate::host::PayloadTarget::Player(1), "only the player in range");
                client.with_host(&mut client_host, |rt| receive(rt, &data));
            }
        };
        deliver(&mut server, &mut host, &mut client, &|_| {});
        let copies = live(&client);
        assert_eq!(copies.len(), 1);
        let copy = copies[0];
        assert_eq!(client.get_field(copy, "$id"), server.get_field(cart, "$id"));
        assert_eq!(crate::tags::uuid_bits(&client, client.get_field(copy, "uuid")), crate::tags::uuid_bits(&server, server.get_field(cart, "uuid")));
        deliver(&mut server, &mut host, &mut client, &|rt| {
            let cart = live(rt)[0];
            set_pos(rt, cart, [3.5, 64.0, 0.5]);
        });
        assert_eq!(xyz(&client, copy), [3.5, 64.0, 0.5], "moved where the server's went");
        let count = |rt: &mut Rt, e: V| {
            let data = rt.get_field(e, "entityData");
            let acc = rt.statics.get("t.COUNT").copied().unwrap_or_default();
            let v = api::call(rt, GET, &[data, acc]);
            match v {
                V::R(_) => rt.get_field(v, "value").i(),
                raw => raw.i(),
            }
        };
        assert_eq!(count(&mut client, copy), count(&mut server, cart), "its count as the server's");
        assert_eq!(count(&mut server, cart), 2);
        // Its third tick discards it: the client's copy goes with it.
        deliver(&mut server, &mut host, &mut client, &|_| {});
        assert!(live(&server).is_empty());
        assert!(live(&client).is_empty(), "removed on the client");
    }

    #[test]
    fn a_mod_entity_joins_the_level_ticks_with_it_keeps_synched_data_and_leaves_when_discarded() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        rt.supers.insert(CART, ENTITY);
        rt.supers.insert(MINECART, CART);
        let int = rt.new_object("net/minecraft/network/syncher/EntityDataSerializer");
        let cart_class = rt.string(CART);
        let count = api::call(&mut rt, DEFINE_ID, &[cart_class, int]);
        rt.statics.insert("t.COUNT", count);
        // A subclass's own data comes after its superclass's.
        let fast_class = rt.string(MINECART);
        let speed = api::call(&mut rt, DEFINE_ID, &[fast_class, int]);
        let id = |rt: &Rt, acc: V| rt.get_field(acc, "$id").i();
        assert_eq!((id(&rt, count), id(&rt, speed)), (8, 9));

        let mut host = MemoryHost::default();
        let ty = rt.new_object(ENTITY_TYPE);
        rt.put_field(ty, "$width", V::F(1.0));
        rt.put_field(ty, "$height", V::F(1.0));
        let (e, found, ids) = rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let e = rt.new_object(CART);
            api::call(rt, "net/minecraft/world/entity/Entity.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V", &[e, ty, level]);
            api::call(rt, "net/minecraft/world/entity/Entity.setPos.(DDD)V", &[e, V::D(0.5), V::D(64.0), V::D(0.5)]);
            let added = api::call(rt, "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z", &[level, e]);
            assert_eq!(added, V::I(1));
            let area = crate::shapes::aabb(rt, [0.0, 63.0, 0.0, 2.0, 66.0, 2.0]);
            let class = rt.string(CART);
            let found = api::call(rt, "net/minecraft/world/level/Level.getEntitiesOfClass.(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;)Ljava/util/List;", &[level, class, area]);
            let found = jutil::elements(rt, found);
            // A client's level answers the same (Create's contraption collisions ask it each frame).
            let client_level = rt.new_object("net/minecraft/client/multiplayer/ClientLevel");
            let seen = api::call(rt, "net/minecraft/client/multiplayer/ClientLevel.getEntitiesOfClass.(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;)Ljava/util/List;", &[client_level, class, area]);
            assert_eq!(jutil::elements(rt, seen), found);
            let mut ids = Vec::new();
            for _ in 0..5 {
                tick_all(rt, "minecraft:overworld");
                ids.push(live(rt).len());
            }
            (e, found, ids)
        });
        assert_eq!(found, vec![e], "the level finds it in a box around it");
        assert_eq!(ids, [1, 1, 0, 0, 0], "gone from the level after its third tick");
        let data = rt.get_field(e, "entityData");
        let n = api::call(&mut rt, GET, &[data, count]);
        assert_eq!(rt.get_field(n, "value"), V::I(3));
        assert_eq!(rt.statics.get("t.heard"), Some(&V::I(3)), "each change heard by the entity");
        assert_eq!(api::call(&mut rt, "net/minecraft/world/entity/Entity.isAlive.()Z", &[e]), V::I(0));
        assert!(rt.get_field(e, "$id").i() >= FIRST_ID);
        let bb = api::call(&mut rt, "net/minecraft/world/entity/Entity.getBoundingBox.()Lnet/minecraft/world/phys/AABB;", &[e]);
        assert_eq!(crate::shapes::aabb_of(&rt, bb), [0.0, 64.0, 0.0, 1.0, 65.0, 1.0]);
    }
}
