//! Damage as translated mods meet it, natively: the game's damage types (`DamageTypes.FALL`, a
//! mod's own `create:crush` from its data) with their tags, the `DamageSource`s a mod makes
//! (`level.damageSources().inFire()`, `new DamageSource(holder, entity)` of Create's
//! `registryOrThrow(DAMAGE_TYPE).getHolderOrThrow(key)`) and reads, and NeoForge 1.21.1's events of
//! a hit, posted from the game's damage hooks where NeoForge posts them:
//!
//! - [`incoming`]: `LivingIncomingDamageEvent`, where `LivingEntity.hurt` is past its guards. A
//!   listener changes the amount (`setAmount`) or cancels the hit.
//! - [`damage_pre`] and [`damage_post`]: `LivingDamageEvent.Pre` in `actuallyHurt`, once the
//!   armour, the enchantments and Resistance took their share (a listener sets what is left), and
//!   `.Post` once it reached the health.
//! - [`knockback`]: `LivingKnockBackEvent`, which a listener changes or cancels.
//! - [`death`]: a mob's `LivingDeathEvent`, where `LivingEntity.die` starts. A listener that cancels
//!   it, and gives health back, keeps the mob alive. [`player_died`] tells a player's, heard only.
//! - [`living_drops`]: a dead mob's `LivingDropsEvent`, its drops as item entities not yet in the
//!   level: a listener changes, adds or takes them, or cancels them all.
//!
//! One `DamageContainer` follows a hit from its incoming event to its `Post`, as NeoForge's stack of
//! them on the entity does, so `getOriginalDamage` is the hit's amount before armour.
//!
//! The hit's hooks run inside the game's damage code, in the middle of the hit: their host answers
//! what the game told of it (the player whose blow it is, with what they held as it started), and
//! makes what a listener changes of the world (a sound, a message, another entity hurt) once the hit
//! is over. The hurt entity is brought up to date from what the game told of it
//! ([`crate::vanilla_entities::living`]), a player is the server's player by entity id
//! ([`server_tick`] keeps the map), and an attacker the runtime hasn't met stands as an entity of
//! its id alone.
//!
//! Not modelled: a container's reduction functions (`addModifier`: the game computes its own
//! reductions), the invulnerable time a listener sets (the game keeps its own), a player's death a
//! listener cancels (the game only tells it), and the events of a hit a mod's own call deals
//! (`entity.hurt` from the mod's code reaches the game while the runtime is busy).

use crate::host::{EntityView, Host};
use crate::{Rt, V};

const SOURCE: &str = "net/minecraft/world/damagesource/DamageSource";
const SOURCES: &str = "net/minecraft/world/damagesource/DamageSources";
const DAMAGE_TYPES: &str = "net/minecraft/world/damagesource/DamageTypes";
const DAMAGE_TYPE: &str = "net/minecraft/world/damagesource/DamageType";
const CONTAINER: &str = "net/neoforged/neoforge/common/damagesource/DamageContainer";
pub(crate) const INCOMING: &str = "net/neoforged/neoforge/event/entity/living/LivingIncomingDamageEvent";
const DAMAGE_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingDamageEvent";
pub(crate) const PRE: &str = "net/neoforged/neoforge/event/entity/living/LivingDamageEvent$Pre";
pub(crate) const POST: &str = "net/neoforged/neoforge/event/entity/living/LivingDamageEvent$Post";
pub(crate) const KNOCKBACK: &str = "net/neoforged/neoforge/event/entity/living/LivingKnockBackEvent";
pub(crate) const DEATH: &str = "net/neoforged/neoforge/event/entity/living/LivingDeathEvent";
pub(crate) const DROPS: &str = "net/neoforged/neoforge/event/entity/living/LivingDropsEvent";
const LIVING_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent";
const ENTITY_EVENT: &str = "net/neoforged/neoforge/event/entity/EntityEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
const CANCELLABLE: &str = "net/neoforged/bus/api/ICancellableEvent";
const PLAYER: &str = "net/minecraft/world/entity/player/Player";
const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
const HOLDER_VALUE: &str = "net/minecraft/core/Holder.value.()Ljava/lang/Object;";
const IS_CREATIVE: &str = "net/minecraft/world/entity/player/Player.isCreative.()Z";
const MAIN_HAND: &str = "net/minecraft/world/entity/LivingEntity.getMainHandItem.()Lnet/minecraft/world/item/ItemStack;";
const EXPLOSION_DIRECT: &str = "net/minecraft/world/level/Explosion.getDirectSourceEntity.()Lnet/minecraft/world/entity/Entity;";
const EXPLOSION_CAUSING: &str = "net/minecraft/world/level/Explosion.getIndirectSourceEntity.()Lnet/minecraft/world/entity/LivingEntity;";
/// The ids of the damage types the runtime has from the game's data, comma-joined, once read.
const LOADED: &str = "compat:damage-types";
/// The server's player of an entity id (`compat:player-entity/12`), and the ids kept so.
const PLAYER_ENTITY: &str = "compat:player-entity/";
const PLAYER_IDS: &str = "compat:player-entity-ids";
/// A hit's container while the game deals it, by the hurt entity's id, and those ids.
const HIT: &str = "compat:hit/";
const HIT_IDS: &str = "compat:hit-ids";
/// A `Post` event's or a container's reductions, by `DamageContainer.Reduction`'s ordinal.
const REDUCTIONS: [&str; 6] = ["$reduction0", "$reduction1", "$reduction2", "$reduction3", "$reduction4", "$reduction5"];

/// The events' classes above them, so a listener for `LivingEvent` (or any event) hears them.
const SUPERS: [(&str, &str); 9] = [
    (INCOMING, LIVING_EVENT),
    (DROPS, LIVING_EVENT),
    (PRE, DAMAGE_EVENT),
    (POST, DAMAGE_EVENT),
    (DAMAGE_EVENT, LIVING_EVENT),
    (KNOCKBACK, LIVING_EVENT),
    (DEATH, LIVING_EVENT),
    (LIVING_EVENT, ENTITY_EVENT),
    (ENTITY_EVENT, EVENT),
];

/// 1.21.1's `DamageTypes` fields: each is its type's key, the field's name in lower case
/// (`IN_FIRE` is `minecraft:in_fire`) but for `FELL_OUT_OF_WORLD`, `minecraft:out_of_world`.
const TYPE_FIELDS: [&str; 47] = [
    "IN_FIRE",
    "CAMPFIRE",
    "LIGHTNING_BOLT",
    "ON_FIRE",
    "LAVA",
    "HOT_FLOOR",
    "IN_WALL",
    "CRAMMING",
    "DROWN",
    "STARVE",
    "CACTUS",
    "FALL",
    "FLY_INTO_WALL",
    "FELL_OUT_OF_WORLD",
    "GENERIC",
    "MAGIC",
    "WITHER",
    "DRAGON_BREATH",
    "DRY_OUT",
    "SWEET_BERRY_BUSH",
    "FREEZE",
    "STALAGMITE",
    "FALLING_BLOCK",
    "FALLING_ANVIL",
    "FALLING_STALACTITE",
    "STING",
    "MOB_ATTACK",
    "MOB_ATTACK_NO_AGGRO",
    "PLAYER_ATTACK",
    "ARROW",
    "TRIDENT",
    "MOB_PROJECTILE",
    "SPIT",
    "WIND_CHARGE",
    "FIREWORKS",
    "FIREBALL",
    "UNATTRIBUTED_FIREBALL",
    "WITHER_SKULL",
    "THROWN",
    "INDIRECT_MAGIC",
    "THORNS",
    "EXPLOSION",
    "PLAYER_EXPLOSION",
    "SONIC_BOOM",
    "BAD_RESPAWN_POINT",
    "OUTSIDE_BORDER",
    "GENERIC_KILL",
];

/// `DamageSources`' sources of no entity, by method: the type each is of.
const PLAIN: [(&str, &str); 24] = [
    ("inFire", "in_fire"),
    ("campfire", "campfire"),
    ("lightningBolt", "lightning_bolt"),
    ("onFire", "on_fire"),
    ("lava", "lava"),
    ("hotFloor", "hot_floor"),
    ("inWall", "in_wall"),
    ("cramming", "cramming"),
    ("drown", "drown"),
    ("starve", "starve"),
    ("cactus", "cactus"),
    ("fall", "fall"),
    ("flyIntoWall", "fly_into_wall"),
    ("fellOutOfWorld", "out_of_world"),
    ("generic", "generic"),
    ("magic", "magic"),
    ("wither", "wither"),
    ("dragonBreath", "dragon_breath"),
    ("dryOut", "dry_out"),
    ("sweetBerryBush", "sweet_berry_bush"),
    ("freeze", "freeze"),
    ("stalagmite", "stalagmite"),
    ("outOfBorder", "outside_border"),
    ("genericKill", "generic_kill"),
];

/// `DamageSources`' sources of one entity, both the direct and the causing one (`mobAttack(mob)`).
const OF_ONE: [(&str, &str); 9] = [
    ("fallingBlock", "falling_block"),
    ("anvil", "falling_anvil"),
    ("fallingStalactite", "falling_stalactite"),
    ("sting", "sting"),
    ("mobAttack", "mob_attack"),
    ("noAggroMobAttack", "mob_attack_no_aggro"),
    ("playerAttack", "player_attack"),
    ("thorns", "thorns"),
    ("sonicBoom", "sonic_boom"),
];

/// `DamageSources`' sources of a direct entity and a causing one (`arrow(arrow, shooter)`).
const OF_TWO: [(&str, &str); 9] = [
    ("arrow", "arrow"),
    ("trident", "trident"),
    ("mobProjectile", "mob_projectile"),
    ("spit", "spit"),
    ("windCharge", "wind_charge"),
    ("fireworks", "fireworks"),
    ("witherSkull", "wither_skull"),
    ("thrown", "thrown"),
    ("indirectMagic", "indirect_magic"),
];

/// A hit's `DamageSource` as the game's hooks name it: its type (`minecraft:player_attack`) and the
/// ids of its causing entity (`getEntity`: an arrow's shooter) and direct one (`getDirectEntity`:
/// the arrow), 0 for none or not known.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub damage_type: &'a str,
    pub causing: i32,
    pub direct: i32,
}

fn know_classes(rt: &mut Rt) {
    if rt.supers.get(PRE) != Some(&DAMAGE_EVENT) {
        for (class, sup) in SUPERS {
            rt.supers.insert(class, sup);
        }
    }
}

/// Whether any listener would hear an event of `class`.
fn listened(rt: &mut Rt, class: &str) -> bool {
    know_classes(rt);
    !crate::neo::hearing(rt, class).is_empty()
}

/// Whether any mod listens to a hit, a push, a death or a death's drops.
pub fn damage_heard(rt: &mut Rt) -> bool {
    [INCOMING, PRE, POST, KNOCKBACK, DEATH, DROPS].into_iter().any(|class| listened(rt, class))
}

fn put_all(rt: &mut Rt, o: V, fields: &[(&'static str, V)]) {
    for &(field, v) in fields {
        rt.put_field(o, field, v);
    }
}

/// `minecraft:` when `id` names no namespace.
fn full_id(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}

// ---- damage types ----

/// `DamageTypes.X`: the type's `ResourceKey` (`damage_type#minecraft:in_fire`).
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != DAMAGE_TYPES || !TYPE_FIELDS.contains(&field) {
        return None;
    }
    let path = if field == "FELL_OUT_OF_WORLD" { "out_of_world".to_owned() } else { field.to_ascii_lowercase() };
    Some(rt.string(&format!("damage_type#minecraft:{path}")))
}

/// The damage type `id`'s record (`DamageType`: its `msgId` and `exhaustion` read as the record's
/// own), one for each type. Of the game's data once the runtime has read it ([`load_types`]); its
/// path as its message and no exhaustion till then.
fn type_object(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:damage-type/{id}"), |rt| {
        let t = rt.new_object(DAMAGE_TYPE);
        let path = id.split_once(':').map_or(id, |(_, p)| p);
        let msg = rt.string(path);
        put_all(rt, t, &[("msgId", msg), ("exhaustion", V::F(0.0))]);
        t
    })
}

/// The registry `damage_type`'s entry `id` (`Holder.value()`, `registryOrThrow(DAMAGE_TYPE).get`).
pub(crate) fn damage_type(rt: &mut Rt, id: &str) -> V {
    load_types(rt);
    type_object(rt, id)
}

fn type_holder(rt: &mut Rt, id: &str) -> V {
    let t = damage_type(rt, id);
    crate::neo::registry_holder(rt, "damage_type", id, t)
}

/// Whether damage type `id` is in the damage type tag `tag` (`minecraft:is_fire`), as the game's
/// data and the mods' tag files put it.
pub(crate) fn in_tag(rt: &mut Rt, id: &str, tag: &str) -> bool {
    let t = damage_type(rt, id);
    let tags = rt.get_field(t, "$tags");
    let tag = full_id(tag);
    rt.str_of(tags).is_some_and(|all| all.split(',').any(|x| x == tag))
}

/// A damage type as the runtime keeps it: id, message, exhaustion, tags.
type TypeData = (String, String, f32, Vec<String>);

/// Reads the damage types of the game's data once there is a host that gives them: the game's own
/// with their tags, then the mods' types and tag files (`data/create/damage_type/crush.json`,
/// `data/minecraft/tags/damage_type/bypasses_armor.json`).
fn load_types(rt: &mut Rt) {
    if rt.interned.contains_key(LOADED) {
        return;
    }
    let Some(game) = rt.host().map(|h| h.damage_types()) else { return };
    if game.is_empty() {
        return;
    }
    let mut types: Vec<TypeData> = game.into_iter().map(|t| (t.id, t.msg_id, t.exhaustion, t.tags)).collect();
    let (mod_types, files) = rt.host().map(mods_data).unwrap_or_default();
    for (id, msg, exhaustion) in mod_types {
        if !types.iter().any(|t| t.0 == id) {
            types.push((id, msg, exhaustion, Vec::new()));
        }
    }
    // What the mods' tag files put in each tag: their members, and their `#tag`s' (the game's tags
    // among them), a few levels deep at most (a cycle ends there).
    let mut added: Vec<(String, String)> = Vec::new();
    for (tag, _, _) in &files {
        let mut todo = vec![(tag.as_str(), 0)];
        while let Some((t, depth)) = todo.pop() {
            added.extend(types.iter().filter(|ty| ty.3.iter().any(|x| x == t)).map(|ty| (ty.0.clone(), tag.clone())));
            for (_, members, refs) in files.iter().filter(|(name, _, _)| name == t) {
                added.extend(members.iter().map(|m| (m.clone(), tag.clone())));
                if depth < 8 {
                    todo.extend(refs.iter().map(|r| (r.as_str(), depth + 1)));
                }
            }
        }
    }
    for (id, tag) in added {
        let at = match types.iter().position(|t| t.0 == id) {
            Some(i) => i,
            None => {
                let path = id.split_once(':').map_or(id.as_str(), |(_, p)| p).to_owned();
                types.push((id.clone(), path, 0.0, Vec::new()));
                types.len() - 1
            }
        };
        if !types[at].3.contains(&tag) {
            types[at].3.push(tag);
        }
    }
    types.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, msg, exhaustion, tags) in &types {
        let t = type_object(rt, id);
        let m = rt.string(msg);
        let all = rt.string(&tags.join(","));
        put_all(rt, t, &[("msgId", m), ("exhaustion", V::F(*exhaustion)), ("$tags", all)]);
    }
    let ids = rt.string(&types.iter().map(|t| t.0.as_str()).collect::<Vec<_>>().join(","));
    rt.interned.insert(LOADED.into(), ids);
    if Rt::tracing() {
        eprintln!("horizon-compat: {} damage types read", types.len());
    }
}

/// A damage type tag file: the tag, its members and the tags it takes in (`#minecraft:is_fire`).
type TagFile = (String, Vec<String>, Vec<String>);

/// The mods' damage types (id, message, exhaustion) and damage type tag files.
fn mods_data(h: &dyn Host) -> (Vec<(String, String, f32)>, Vec<TagFile>) {
    let json = |path: &str| h.resource(path).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let mut types = Vec::new();
    let mut tags: Vec<TagFile> = Vec::new();
    for ns in h.resource_namespaces("data") {
        let folder = format!("data/{ns}/damage_type/");
        for path in h.resource_paths(&folder[..folder.len() - 1]) {
            let Some(name) = path.strip_prefix(&folder).and_then(|p| p.strip_suffix(".json")) else { continue };
            let Some(j) = json(&path) else { continue };
            let msg = j.get("message_id").and_then(|m| m.as_str()).unwrap_or(name).to_owned();
            let exhaustion = j.get("exhaustion").and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32;
            types.push((format!("{ns}:{name}"), msg, exhaustion));
        }
        let folder = format!("data/{ns}/tags/damage_type/");
        for path in h.resource_paths(&folder[..folder.len() - 1]) {
            let Some(name) = path.strip_prefix(&folder).and_then(|p| p.strip_suffix(".json")) else { continue };
            let Some(j) = json(&path) else { continue };
            let (mut members, mut refs) = (Vec::new(), Vec::new());
            for v in j.get("values").and_then(|v| v.as_array()).into_iter().flatten() {
                let Some(entry) = v.as_str().or_else(|| v.get("id").and_then(|i| i.as_str())) else { continue };
                match entry.strip_prefix('#') {
                    Some(tag) => refs.push(full_id(tag)),
                    None => members.push(full_id(entry)),
                }
            }
            tags.push((format!("{ns}:{name}"), members, refs));
        }
    }
    (types, tags)
}

/// The damage type a death message's key (`death.attack.inFire.player`, `death.fell.accident.ladder`)
/// tells of: the type of that message, the one named like it first (`inFire` is `in_fire`, not
/// `campfire`).
fn type_of_message(rt: &mut Rt, key: &str) -> Option<String> {
    if key.starts_with("death.fell.") {
        return Some("minecraft:fall".into());
    }
    let msg = key.strip_prefix("death.attack.")?;
    let ids: Vec<String> = rt.interned.get(LOADED).and_then(|v| rt.str_of(*v)).unwrap_or("").split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect();
    let find = |rt: &mut Rt, m: &str| {
        let snake: String = m.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] }).collect();
        let of_msg: Vec<&String> = ids
            .iter()
            .filter(|id| {
                let t = type_object(rt, id);
                rt.str_of(rt.get_field(t, "msgId")) == Some(m)
            })
            .collect();
        of_msg.iter().find(|id| id.split_once(':').is_some_and(|(_, p)| p == snake)).or(of_msg.first()).map(|id| (*id).clone())
    };
    if let Some(id) = find(rt, msg) {
        return Some(id);
    }
    let shorter = msg.strip_suffix(".player").or_else(|| msg.strip_suffix(".item"))?;
    find(rt, shorter)
}

// ---- sources ----

/// A `DamageSource` of type `id`, from `direct` and `causing` (null for none) at `pos` (a `Vec3`,
/// null for its direct entity's place).
pub(crate) fn source(rt: &mut Rt, id: &str, direct: V, causing: V, pos: V) -> V {
    let s = rt.new_object(SOURCE);
    let holder = type_holder(rt, id);
    init_source(rt, s, holder, direct, causing, pos);
    s
}

fn init_source(rt: &mut Rt, s: V, holder: V, direct: V, causing: V, pos: V) {
    let id = rt.get_field(holder, "$id");
    put_all(rt, s, &[("$holder", holder), ("$type", id), ("$direct", direct), ("$causing", causing), ("$pos", pos)]);
}

/// The source of no entity of type `id`, one for each type (as `DamageSources` keeps them).
fn shared(rt: &mut Rt, id: &str) -> V {
    rt.intern(format!("compat:damage-source/{id}"), |rt| source(rt, id, V::NULL, V::NULL, V::NULL))
}

fn type_id(rt: &Rt, s: V) -> String {
    rt.str_of(rt.get_field(s, "$type")).unwrap_or("").to_owned()
}

/// The source's `DamageType` (`type()`): its registry entry, or a direct holder's value.
fn type_value(rt: &mut Rt, s: V) -> V {
    let id = type_id(rt, s);
    if !id.is_empty() {
        return damage_type(rt, &id);
    }
    let holder = rt.get_field(s, "$holder");
    if holder.r() == 0 { V::NULL } else { crate::api::call(rt, HOLDER_VALUE, &[holder]) }
}

/// `explosion(direct, causing)`: a player's explosion when both are known (vanilla's rule).
fn explosion(rt: &mut Rt, direct: V, causing: V) -> V {
    let id = if direct.r() != 0 && causing.r() != 0 { "minecraft:player_explosion" } else { "minecraft:explosion" };
    source(rt, id, direct, causing, V::NULL)
}

/// The level's `DamageSources` (`level.damageSources()`, `entity.damageSources()`): `damageTypes` is
/// the registry, as registries are their names here.
fn sources(rt: &mut Rt) -> V {
    rt.intern("compat:damage-sources".into(), |rt| {
        let s = rt.new_object(SOURCES);
        let r = rt.string("damage_type");
        rt.put_field(s, "damageTypes", r);
        s
    })
}

fn sources_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let entities = desc.split(')').next().unwrap_or("").matches("Lnet/minecraft/world/").count();
    Some(match name {
        "<init>" => {
            let r = rt.string("damage_type");
            rt.put_field(a(0), "damageTypes", r);
            V::Void
        }
        "source" => {
            let (_, id) = crate::neo::split_key(rt, a(1));
            match entities {
                0 => source(rt, &id, V::NULL, V::NULL, V::NULL),
                1 => source(rt, &id, a(2), a(2), V::NULL),
                _ => source(rt, &id, a(2), a(3), V::NULL),
            }
        }
        // An attributed fireball is its shooter's; one of nobody burns as fire does.
        "fireball" if a(2).r() == 0 => source(rt, "minecraft:unattributed_fireball", a(1), a(1), V::NULL),
        "fireball" => source(rt, "minecraft:fireball", a(1), a(2), V::NULL),
        "explosion" if desc.starts_with("(Lnet/minecraft/world/level/Explosion;)") => {
            let e = a(1);
            if e.r() == 0 {
                explosion(rt, V::NULL, V::NULL)
            } else {
                let direct = crate::api::call(rt, EXPLOSION_DIRECT, &[e]);
                let causing = crate::api::call(rt, EXPLOSION_CAUSING, &[e]);
                explosion(rt, direct, causing)
            }
        }
        "explosion" => explosion(rt, a(1), a(2)),
        "badRespawnPointExplosion" => source(rt, "minecraft:bad_respawn_point", V::NULL, V::NULL, a(1)),
        _ => {
            if let Some(&(_, id)) = PLAIN.iter().find(|(m, _)| *m == name).filter(|_| entities == 0) {
                shared(rt, &format!("minecraft:{id}"))
            } else if let Some(&(_, id)) = OF_ONE.iter().find(|(m, _)| *m == name).filter(|_| entities == 1) {
                source(rt, &format!("minecraft:{id}"), a(1), a(1), V::NULL)
            } else if let Some(&(_, id)) = OF_TWO.iter().find(|(m, _)| *m == name).filter(|_| entities == 2) {
                source(rt, &format!("minecraft:{id}"), a(1), a(2), V::NULL)
            } else {
                return None;
            }
        }
    })
}

fn source_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    Some(match name {
        // (holder[, direct[, causing]][, pos]) or (holder, pos): one entity is both.
        "<init>" => {
            let params = desc.split(')').next().unwrap_or("");
            let entities = params.matches("Lnet/minecraft/world/entity/Entity;").count();
            let pos = if params.ends_with("Lnet/minecraft/world/phys/Vec3;") { a(args.len() - 1) } else { V::NULL };
            let (direct, causing) = match entities {
                0 => (V::NULL, V::NULL),
                1 => (a(2), a(2)),
                _ => (a(2), a(3)),
            };
            init_source(rt, s, a(1), direct, causing, pos);
            V::Void
        }
        "getEntity" => rt.get_field(s, "$causing"),
        "getDirectEntity" => rt.get_field(s, "$direct"),
        "isDirect" => V::I(i32::from(rt.get_field(s, "$causing") == rt.get_field(s, "$direct"))),
        "typeHolder" => rt.get_field(s, "$holder"),
        "type" => type_value(rt, s),
        "getMsgId" => {
            let t = type_value(rt, s);
            if t.r() == 0 { V::NULL } else { rt.get_field(t, "msgId") }
        }
        "getFoodExhaustion" => {
            let t = type_value(rt, s);
            V::F(if t.r() == 0 { 0.0 } else { rt.get_field(t, "exhaustion").f() })
        }
        "is" if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
            let id = type_id(rt, s);
            let tag = rt.str_of(rt.get_field(a(1), "$location")).unwrap_or("").to_owned();
            V::I(i32::from(!id.is_empty() && in_tag(rt, &id, &tag)))
        }
        "is" => {
            let (_, id) = crate::neo::split_key(rt, a(1));
            V::I(i32::from(full_id(&id) == type_id(rt, s)))
        }
        "sourcePositionRaw" => rt.get_field(s, "$pos"),
        // Where it was dealt from: its own place, else its direct entity's.
        "getSourcePosition" => {
            let pos = rt.get_field(s, "$pos");
            let direct = rt.get_field(s, "$direct");
            if pos.r() != 0 || direct.r() == 0 {
                pos
            } else {
                match crate::api_common::entity_place(rt, direct) {
                    Some([x, y, z]) => crate::api_common::vec3(rt, x, y, z),
                    None => V::NULL,
                }
            }
        }
        // The direct entity's weapon (a living one's main hand), none for the rest.
        "getWeaponItem" => {
            let direct = rt.get_field(s, "$direct");
            if direct.r() != 0 && rt.instance_of(direct, LIVING) { crate::api::call(rt, MAIN_HAND, &[direct]) } else { V::NULL }
        }
        "isCreativePlayer" => {
            let causing = rt.get_field(s, "$causing");
            V::I(i32::from(causing.r() != 0 && rt.instance_of(causing, PLAYER) && crate::api::call(rt, IS_CREATIVE, &[causing]).i() != 0))
        }
        "toString" => {
            let t = type_value(rt, s);
            let msg = if t.r() == 0 { "null".to_owned() } else { rt.str_of(rt.get_field(t, "msgId")).unwrap_or("null").to_owned() };
            rt.string(&format!("DamageSource ({msg})"))
        }
        "equals" => V::I(i32::from(s == a(1))),
        "hashCode" => V::I(s.r() as i32),
        _ => return None,
    })
}

// ---- containers ----

/// A hit's `DamageContainer` of `source` and `amount`: what is left of it (`getNewDamage`) starts as
/// all of it, and the hurt entity's invulnerable time after it is vanilla's 20 ticks.
fn container(rt: &mut Rt, source: V, amount: f32) -> V {
    let c = rt.new_object(CONTAINER);
    init_container(rt, c, source, amount);
    c
}

fn init_container(rt: &mut Rt, c: V, source: V, amount: f32) {
    put_all(
        rt,
        c,
        &[("$source", source), ("$original", V::F(amount)), ("$new", V::F(amount)), ("$invulnerability", V::I(20)), ("$blocked", V::F(0.0)), ("$shield", V::F(0.0))],
    );
}

fn reduction_field(rt: &Rt, reduction: V) -> &'static str {
    REDUCTIONS[crate::api_common::enum_ordinal(rt, reduction).clamp(0, 5) as usize]
}

fn container_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    Some(match name {
        "<init>" => {
            init_container(rt, c, a(1), a(2).f());
            V::Void
        }
        "getOriginalDamage" => V::F(rt.get_field(c, "$original").f()),
        "getSource" => rt.get_field(c, "$source"),
        "getNewDamage" => V::F(rt.get_field(c, "$new").f()),
        "setNewDamage" => {
            rt.put_field(c, "$new", V::F(a(1).f()));
            V::Void
        }
        "getBlockedDamage" => V::F(rt.get_field(c, "$blocked").f()),
        "getShieldDamage" => V::F(rt.get_field(c, "$shield").f()),
        "getPostAttackInvulnerabilityTicks" => V::I(rt.get_field(c, "$invulnerability").i()),
        "setPostAttackInvulnerabilityTicks" => {
            rt.put_field(c, "$invulnerability", V::I(a(1).i()));
            V::Void
        }
        "getReduction" => {
            let field = reduction_field(rt, a(1));
            V::F(rt.get_field(c, field).f())
        }
        // NeoForge's own reductions as a mod sets one: what is left goes down by it.
        "setReduction" => {
            let field = reduction_field(rt, a(1));
            let r = a(2).f();
            rt.put_field(c, field, V::F(r));
            let left = rt.get_field(c, "$new").f() - r.max(0.0);
            rt.put_field(c, "$new", V::F(left));
            V::Void
        }
        // Reduction functions run on NeoForge's own reductions; the game computes its own here.
        "addModifier" | "setBlockedDamage" => V::Void,
        _ => return None,
    })
}

// ---- the entities of a hit ----

/// The hurt entity: the server's player of that entity id, or the object of the game's mob, brought
/// up to date from `seen`. Null for a player the runtime hasn't been told of.
fn victim(rt: &mut Rt, entity: i32, kind: &str, seen: Option<&EntityView>) -> V {
    if kind == "minecraft:player" {
        return rt.interned.get(&format!("{PLAYER_ENTITY}{entity}")).copied().unwrap_or(V::NULL);
    }
    crate::vanilla_entities::living(rt, entity, kind, seen)
}

/// The entity `id` of a hit's source: a player, an entity the runtime holds, one the game tells of,
/// else one standing for the id alone (a `LivingEntity` when it `causes` the hit). Null for 0.
fn actor(rt: &mut Rt, id: i32, causes: bool) -> V {
    if id == 0 {
        return V::NULL;
    }
    if let Some(&p) = rt.interned.get(&format!("{PLAYER_ENTITY}{id}")) {
        return p;
    }
    if let Some(e) = crate::vanilla_entities::known(rt, id) {
        return e;
    }
    if let Some(kind) = rt.host().and_then(|h| h.entity(id)).map(|v| v.kind) {
        let e = crate::vanilla_entities::of(rt, id, &kind, None);
        if e.r() != 0 {
            return e;
        }
    }
    crate::vanilla_entities::unseen(rt, id, causes)
}

/// Records `by`, a hit's causing entity, as what last hurt `victim` (`LivingEntity.setLastHurtByMob`),
/// and a player also as the last player that did (`lastHurtByPlayer`): a living one only, as
/// vanilla's. What `getLastHurtByMob` and `getKillCredit` (the player first) answer.
pub(crate) fn hurt_by(rt: &mut Rt, victim: V, by: V) {
    if victim.r() != 0 && by.r() != 0 && rt.instance_of(by, LIVING) {
        rt.put_field(victim, "$lastHurtBy", by);
        if rt.instance_of(by, PLAYER) {
            rt.put_field(victim, "$lastHurtByPlayer", by);
        }
    }
}

/// A mod's `DamageSource` as the game takes a blow's (`Host::hurt_entity_by`): its type, the
/// player (by UUID) or the game's mob (by id) it is credited to (`getEntity()`), where it came from
/// (`getSourcePosition()`), and, dealt by a mod's projectile, that one's horizontal movement
/// (`Projectile.calculateHorizontalHurtKnockbackDirection`, which the knockback goes along).
pub(crate) fn blow_of(rt: &mut Rt, s: V) -> crate::host::HurtBy {
    if s.r() == 0 {
        return crate::host::HurtBy { damage_type: "minecraft:generic".into(), ..Default::default() };
    }
    let causing = rt.get_field(s, "$causing");
    let player = (causing.r() != 0 && crate::api_common::is_player_class(rt.class_of(causing)))
        .then(|| crate::network::player_uuid(rt, causing).or_else(|| rt.host().and_then(|h| h.player_id())))
        .flatten();
    let mob = (player.is_none() && crate::vanilla_entities::is_proxy(rt, causing)).then(|| crate::vanilla_entities::id(rt, causing));
    let at = call_source(rt, "getSourcePosition", "()Lnet/minecraft/world/phys/Vec3;", s);
    let from = (at.r() != 0).then(|| ["x", "y", "z"].map(|f| rt.get_field(at, f).d()));
    let direct = rt.get_field(s, "$direct");
    let motion = rt.get_field(direct, "$motion");
    let projectile = direct.r() != 0 && motion.r() != 0 && crate::projectiles::is_projectile_class(crate::mod_entities::vanilla_base(rt, direct));
    let motion = projectile.then(|| [rt.get_field(motion, "x").d(), rt.get_field(motion, "z").d()]);
    let id = type_id(rt, s);
    crate::host::HurtBy { damage_type: if id.is_empty() { "minecraft:generic".into() } else { id }, player, mob, from, motion }
}

fn call_source(rt: &mut Rt, name: &str, desc: &str, s: V) -> V {
    let k = rt.static_key(&format!("{SOURCE}.{name}.{desc}"));
    crate::api::call(rt, k, &[s])
}

/// The `DamageSource` of a hit as the hooks name it.
fn hit_source(rt: &mut Rt, s: &Source<'_>) -> V {
    let causing = actor(rt, s.causing, true);
    let direct = if s.direct == s.causing { causing } else { actor(rt, s.direct, false) };
    let id = if s.damage_type.is_empty() { "minecraft:generic".to_owned() } else { full_id(s.damage_type) };
    source(rt, &id, direct, causing, V::NULL)
}

/// Keeps `c` as entity `entity`'s hit till its `Post` (or the next tick).
fn keep_hit(rt: &mut Rt, entity: i32, c: V) {
    rt.interned.insert(format!("{HIT}{entity}"), c);
    let ids = rt.interned.get(HIT_IDS).and_then(|v| rt.str_of(*v)).unwrap_or("").to_owned();
    if !ids.split(',').any(|i| i == entity.to_string()) {
        let more = if ids.is_empty() { entity.to_string() } else { format!("{ids},{entity}") };
        let s = rt.string(&more);
        rt.interned.insert(HIT_IDS.into(), s);
    }
}

fn drop_hit(rt: &mut Rt, entity: i32) {
    rt.interned.remove(&format!("{HIT}{entity}"));
}

/// Lets go of the hits a tick left: one canceled, or stopped by the game, has no `Post`.
fn clear_hits(rt: &mut Rt) {
    let Some(ids) = rt.interned.remove(HIT_IDS) else { return };
    let ids = rt.str_of(ids).unwrap_or("").to_owned();
    for id in ids.split(',').filter(|i| !i.is_empty()) {
        rt.interned.remove(&format!("{HIT}{id}"));
    }
}

/// Entity `entity`'s hit of `source`: the container its incoming event made, or a new one of
/// `amount` (a hit the incoming event wasn't told of).
fn hit_container(rt: &mut Rt, entity: i32, source: &Source<'_>, amount: f32) -> V {
    if let Some(&c) = rt.interned.get(&format!("{HIT}{entity}")) {
        let s = rt.get_field(c, "$source");
        if type_id(rt, s) == full_id(source.damage_type) {
            return c;
        }
    }
    let s = hit_source(rt, source);
    let c = container(rt, s, amount);
    keep_hit(rt, entity, c);
    c
}

// ---- the events ----

/// `LivingIncomingDamageEvent`: entity `entity` of type `kind` (`minecraft:player` for a player),
/// as the game saw it (`seen`), is about to take `amount` of `source`. A listener's amount comes
/// back in `amount`; whether one canceled the hit.
pub fn incoming(rt: &mut Rt, host: &mut dyn Host, entity: i32, kind: &str, seen: Option<&EntityView>, source: Source<'_>, amount: &mut f32) -> bool {
    let heard = listened(rt, INCOMING);
    if !heard && !listened(rt, PRE) && !listened(rt, POST) {
        return false;
    }
    rt.with_host(host, |rt| {
        let victim = victim(rt, entity, kind, seen);
        if victim.r() == 0 {
            return false;
        }
        let s = hit_source(rt, &source);
        let c = container(rt, s, *amount);
        keep_hit(rt, entity, c);
        if !heard {
            return false;
        }
        let ev = rt.new_object(INCOMING);
        put_all(rt, ev, &[("$entity", victim), ("$container", c), ("$canceled", V::I(0))]);
        crate::neo::post(rt, INCOMING, ev);
        let canceled = rt.get_field(ev, "$canceled").i() != 0;
        let new = rt.get_field(c, "$new").f();
        if new.is_finite() {
            *amount = new;
        }
        if canceled {
            drop_hit(rt, entity);
        } else {
            let by = rt.get_field(s, "$causing");
            hurt_by(rt, victim, by);
        }
        canceled
    })
}

/// `LivingDamageEvent.Pre`: what is left of the hit on entity `entity` once armour, enchantments and
/// Resistance took their share, `amount`, which a listener sets (a value not finite is left as it
/// was, one below 0 is 0).
pub fn damage_pre(rt: &mut Rt, host: &mut dyn Host, entity: i32, kind: &str, seen: Option<&EntityView>, source: Source<'_>, amount: &mut f32) {
    if !listened(rt, PRE) && !listened(rt, POST) {
        return;
    }
    rt.with_host(host, |rt| {
        let victim = victim(rt, entity, kind, seen);
        if victim.r() == 0 {
            return;
        }
        let c = hit_container(rt, entity, &source, *amount);
        rt.put_field(c, "$new", V::F(*amount));
        if !listened(rt, PRE) {
            return;
        }
        let ev = rt.new_object(PRE);
        put_all(rt, ev, &[("$entity", victim), ("$container", c)]);
        crate::neo::post(rt, PRE, ev);
        let new = rt.get_field(c, "$new").f();
        if new.is_finite() {
            *amount = new.max(0.0);
        }
    })
}

/// `LivingDamageEvent.Post`: `amount` of the hit reached entity `entity`'s health (0 when
/// absorption took it all). Heard only.
pub fn damage_post(rt: &mut Rt, host: &mut dyn Host, entity: i32, kind: &str, seen: Option<&EntityView>, source: Source<'_>, amount: f32) {
    if !listened(rt, POST) {
        drop_hit(rt, entity);
        return;
    }
    rt.with_host(host, |rt| {
        let victim = victim(rt, entity, kind, seen);
        if victim.r() == 0 {
            drop_hit(rt, entity);
            return;
        }
        let c = hit_container(rt, entity, &source, amount);
        drop_hit(rt, entity);
        let ev = rt.new_object(POST);
        let fields: Vec<(&'static str, V)> = ["$source", "$original", "$blocked", "$shield", "$invulnerability"]
            .into_iter()
            .chain(REDUCTIONS)
            .map(|f| (f, rt.get_field(c, f)))
            .collect();
        put_all(rt, ev, &fields);
        put_all(rt, ev, &[("$entity", victim), ("$new", V::F(amount))]);
        crate::neo::post(rt, POST, ev);
    })
}

/// `LivingKnockBackEvent`: entity `entity` is about to be pushed `strength` away from (`x`, `z`) by
/// a blow of entity `causing` (0 for none), which has hurt it by then (`getKillCredit`). A
/// listener's values come back (the strength through a float, as NeoForge's event holds it);
/// whether one canceled the push.
pub fn knockback(rt: &mut Rt, host: &mut dyn Host, entity: i32, kind: &str, seen: Option<&EntityView>, causing: i32, strength: &mut f64, x: &mut f64, z: &mut f64) -> bool {
    if !listened(rt, KNOCKBACK) {
        return false;
    }
    rt.with_host(host, |rt| {
        let victim = victim(rt, entity, kind, seen);
        if victim.r() == 0 {
            return false;
        }
        // The blow's attacker has hurt it by now (`getKillCredit`, as Farmer's Delight's knife asks).
        let by = actor(rt, causing, true);
        hurt_by(rt, victim, by);
        let ev = rt.new_object(KNOCKBACK);
        let s = *strength as f32;
        put_all(
            rt,
            ev,
            &[
                ("$entity", victim),
                ("$strength", V::F(s)),
                ("$ratioX", V::D(*x)),
                ("$ratioZ", V::D(*z)),
                ("$originalStrength", V::F(s)),
                ("$originalRatioX", V::D(*x)),
                ("$originalRatioZ", V::D(*z)),
                ("$canceled", V::I(0)),
            ],
        );
        crate::neo::post(rt, KNOCKBACK, ev);
        if rt.get_field(ev, "$canceled").i() != 0 {
            return true;
        }
        // A strength no listener changed stays the game's own double.
        let to = rt.get_field(ev, "$strength").f();
        if to != s && to.is_finite() {
            *strength = f64::from(to);
        }
        for (field, v) in [("$ratioX", x), ("$ratioZ", z)] {
            let to = rt.get_field(ev, field).d();
            if to.is_finite() {
                *v = to;
            }
        }
        false
    })
}

/// `LivingDeathEvent`: the game's mob `entity` of type `kind` dies of `source`. Whether a listener
/// canceled the death.
pub fn death(rt: &mut Rt, host: &mut dyn Host, entity: i32, kind: &str, source: Source<'_>) -> bool {
    rt.safe_point();
    drop_hit(rt, entity);
    if !listened(rt, DEATH) {
        return false;
    }
    rt.with_host(host, |rt| {
        load_types(rt);
        let victim = crate::vanilla_entities::of(rt, entity, kind, None);
        if victim.r() == 0 {
            return false;
        }
        let s = hit_source(rt, &source);
        let by = rt.get_field(s, "$causing");
        hurt_by(rt, victim, by);
        let ev = rt.new_object(DEATH);
        put_all(rt, ev, &[("$entity", victim), ("$source", s), ("$canceled", V::I(0))]);
        crate::neo::post(rt, DEATH, ev);
        rt.get_field(ev, "$canceled").i() != 0
    })
}

/// `LivingDeathEvent` for the server's player `player`, dying of what the death message's key
/// `cause` tells (`death.attack.lava`; `minecraft:generic` for none). Heard only: the game goes on
/// with the death whatever a listener does.
pub fn player_died(rt: &mut Rt, host: &mut dyn Host, player: u128, cause: Option<&str>) {
    rt.safe_point();
    if !listened(rt, DEATH) {
        return;
    }
    rt.with_host(host, |rt| {
        load_types(rt);
        let p = crate::menus::server_player(rt, player);
        if let Some((_, at)) = rt.host().and_then(|h| h.players().into_iter().find(|(id, _)| *id == player)) {
            for (field, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
                rt.put_field(p, field, V::D(v));
            }
        }
        let id = cause.and_then(|c| type_of_message(rt, c)).unwrap_or_else(|| "minecraft:generic".into());
        let s = source(rt, &id, V::NULL, V::NULL, V::NULL);
        let ev = rt.new_object(DEATH);
        put_all(rt, ev, &[("$entity", p), ("$source", s), ("$canceled", V::I(0))]);
        crate::neo::post(rt, DEATH, ev);
    });
}

/// One of a dead mob's drops, as [`living_drops`] hands them to the mods and back: its stack, where
/// its item entity appears, its motion and pickup delay. `origin` is the drop given that it still is,
/// its stack unchanged (the game keeps its own stack for it); None for one a mod added or changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Drop {
    pub stack: crate::host::HandStack,
    pub position: [f64; 3],
    pub motion: [f64; 3],
    pub pickup_delay: u32,
    pub origin: Option<usize>,
}

/// `LivingDropsEvent` for the dead mob `entity` of type `kind`, killed by `source`, with its drops
/// `drops` not yet in the level, `recently_hit` whether a player hurt it lately (where NeoForge's
/// `LivingEntity.dropAllDeathLoot` posts it). The mods see the drops as item entities (`getDrops`)
/// they may change, add to or take from, and `drops` is left as they leave them. Whether a listener
/// canceled it (no drop joins the level); None when no mod listens, `drops` untouched.
pub fn living_drops(rt: &mut Rt, host: &mut dyn Host, entity: i32, kind: &str, source: Source<'_>, recently_hit: bool, drops: &mut Vec<Drop>) -> Option<bool> {
    rt.safe_point();
    if !listened(rt, DROPS) {
        return None;
    }
    rt.with_host(host, |rt| {
        load_types(rt);
        let victim = crate::vanilla_entities::of(rt, entity, kind, None);
        if victim.r() == 0 {
            return None;
        }
        let s = hit_source(rt, &source);
        // Each drop as an item entity, and its stack as the runtime reads it back unchanged.
        let mut given = Vec::with_capacity(drops.len());
        for d in drops.iter() {
            let stack = crate::hand::load(rt, Some(d.stack.clone()));
            let unchanged = crate::hand::hand_stack_of(rt, stack);
            given.push((crate::entities::unspawned(rt, d.position, stack, d.motion, d.pickup_delay), unchanged));
        }
        let list = crate::jutil::new_list(rt, given.iter().map(|g| g.0).collect());
        let ev = rt.new_object(DROPS);
        put_all(rt, ev, &[("$entity", victim), ("$source", s), ("$drops", list), ("$recentlyHit", V::I(recently_hit as i32)), ("$canceled", V::I(0))]);
        crate::neo::post(rt, DROPS, ev);
        if rt.get_field(ev, "$canceled").i() != 0 {
            return Some(true);
        }
        let left = rt.get_field(ev, "$drops");
        let mut out = Vec::new();
        for e in crate::jutil::elements(rt, left) {
            let Some((stack, position, motion, pickup_delay)) = crate::entities::unspawned_drop(rt, e) else { continue };
            let origin = given.iter().position(|g| g.0 == e).filter(|&i| given[i].1.as_ref() == Some(&stack));
            out.push(Drop { stack, position, motion, pickup_delay, origin });
        }
        *drops = out;
        Some(false)
    })
}

/// A server tick's start (inside the host): the hits the last tick left go, and while a mod listens
/// to hits the game's damage types are read and the server's players are kept by entity id, for the
/// hit's hooks that have no world to ask.
pub fn server_tick(rt: &mut Rt) {
    clear_hits(rt);
    if !damage_heard(rt) {
        return;
    }
    load_types(rt);
    let players = rt.host().map(|h| h.player_entities()).unwrap_or_default();
    if let Some(old) = rt.interned.remove(PLAYER_IDS) {
        let old = rt.str_of(old).unwrap_or("").to_owned();
        for id in old.split(',').filter(|i| !i.is_empty()) {
            rt.interned.remove(&format!("{PLAYER_ENTITY}{id}"));
        }
    }
    let mut ids = Vec::new();
    for (id, uuid, at) in players {
        let p = crate::vanilla_entities::player(rt, id, uuid, at);
        rt.interned.insert(format!("{PLAYER_ENTITY}{id}"), p);
        ids.push(id.to_string());
    }
    let s = rt.string(&ids.join(","));
    rt.interned.insert(PLAYER_IDS.into(), s);
}

// ---- the calls ----

/// Which of the damage events `ev` is, if any.
fn event_class(rt: &Rt, ev: V) -> Option<&'static str> {
    if ev.r() == 0 {
        return None;
    }
    let mut c = rt.class_of(ev);
    for _ in 0..8 {
        if matches!(c, INCOMING | PRE | POST | KNOCKBACK | DEATH | DROPS) {
            return Some(c);
        }
        c = rt.supers.get(c).copied()?;
    }
    None
}

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(owner, SOURCE | SOURCES | CONTAINER | INCOMING | DAMAGE_EVENT | PRE | POST | KNOCKBACK | DEATH | DROPS)
        || (matches!(owner, EVENT | CANCELLABLE) && matches!(name, "isCanceled" | "setCanceled"))
        || (matches!(owner, LIVING_EVENT | ENTITY_EVENT) && name == "getEntity")
        || (name == "damageSources" && desc == "()Lnet/minecraft/world/damagesource/DamageSources;")
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        SOURCE => return source_call(rt, name, desc, args),
        SOURCES => return sources_call(rt, name, desc, args),
        CONTAINER => return container_call(rt, name, args),
        _ => {}
    }
    if name == "damageSources" && desc == "()Lnet/minecraft/world/damagesource/DamageSources;" {
        return Some(sources(rt));
    }
    let ev = a(0);
    if name == "<init>" {
        return construct(rt, owner, args);
    }
    let class = event_class(rt, ev)?;
    let container = rt.get_field(ev, "$container");
    let of_container = |rt: &Rt, field: &'static str| rt.get_field(container, field);
    Some(match (class, name) {
        (_, "getEntity") => rt.get_field(ev, "$entity"),
        (_, "isCanceled") => V::I(rt.get_field(ev, "$canceled").i()),
        (INCOMING | KNOCKBACK | DEATH | DROPS, "setCanceled") => {
            rt.put_field(ev, "$canceled", V::I(a(1).i()));
            V::Void
        }
        (INCOMING | PRE, "getContainer") => container,
        (INCOMING | PRE, "getSource") => of_container(rt, "$source"),
        (INCOMING, "getAmount") | (PRE, "getNewDamage") => V::F(of_container(rt, "$new").f()),
        (INCOMING, "getOriginalAmount") | (PRE, "getOriginalDamage") => V::F(of_container(rt, "$original").f()),
        (INCOMING, "setAmount") | (PRE, "setNewDamage") => {
            rt.put_field(container, "$new", V::F(a(1).f()));
            V::Void
        }
        (INCOMING, "setInvulnerabilityTicks") => {
            rt.put_field(container, "$invulnerability", V::I(a(1).i()));
            V::Void
        }
        (INCOMING, "addReductionModifier") => V::Void,
        (POST | DEATH | DROPS, "getSource") => rt.get_field(ev, "$source"),
        (DROPS, "getDrops") => rt.get_field(ev, "$drops"),
        (DROPS, "isRecentlyHit") => V::I(rt.get_field(ev, "$recentlyHit").i()),
        (POST, "getOriginalDamage") => V::F(rt.get_field(ev, "$original").f()),
        (POST, "getNewDamage") => V::F(rt.get_field(ev, "$new").f()),
        (POST, "getBlockedDamage") => V::F(rt.get_field(ev, "$blocked").f()),
        (POST, "getShieldDamage") => V::F(rt.get_field(ev, "$shield").f()),
        (POST, "getPostAttackInvulnerabilityTicks") => V::I(rt.get_field(ev, "$invulnerability").i()),
        (POST, "getReduction") => {
            let field = reduction_field(rt, a(1));
            V::F(rt.get_field(ev, field).f())
        }
        (KNOCKBACK, "getStrength") => V::F(rt.get_field(ev, "$strength").f()),
        (KNOCKBACK, "getRatioX") => V::D(rt.get_field(ev, "$ratioX").d()),
        (KNOCKBACK, "getRatioZ") => V::D(rt.get_field(ev, "$ratioZ").d()),
        (KNOCKBACK, "getOriginalStrength") => V::F(rt.get_field(ev, "$originalStrength").f()),
        (KNOCKBACK, "getOriginalRatioX") => V::D(rt.get_field(ev, "$originalRatioX").d()),
        (KNOCKBACK, "getOriginalRatioZ") => V::D(rt.get_field(ev, "$originalRatioZ").d()),
        (KNOCKBACK, "setStrength") => {
            rt.put_field(ev, "$strength", V::F(a(1).f()));
            V::Void
        }
        (KNOCKBACK, "setRatioX" | "setRatioZ") => {
            let field = if name == "setRatioX" { "$ratioX" } else { "$ratioZ" };
            rt.put_field(ev, field, V::D(a(1).d()));
            V::Void
        }
        _ => return None,
    })
}

/// A damage event a mod makes (`new LivingDeathEvent(entity, source)`, to post on the bus itself).
fn construct(rt: &mut Rt, owner: &str, args: &[V]) -> Option<V> {
    know_classes(rt);
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    match owner {
        INCOMING | PRE => put_all(rt, ev, &[("$entity", a(1)), ("$container", a(2)), ("$canceled", V::I(0))]),
        POST => {
            let c = a(2);
            let fields: Vec<(&'static str, V)> = ["$source", "$original", "$new", "$blocked", "$shield", "$invulnerability"]
                .into_iter()
                .chain(REDUCTIONS)
                .map(|f| (f, rt.get_field(c, f)))
                .collect();
            put_all(rt, ev, &fields);
            rt.put_field(ev, "$entity", a(1));
        }
        KNOCKBACK => put_all(
            rt,
            ev,
            &[
                ("$entity", a(1)),
                ("$strength", V::F(a(2).f())),
                ("$ratioX", V::D(a(3).d())),
                ("$ratioZ", V::D(a(4).d())),
                ("$originalStrength", V::F(a(2).f())),
                ("$originalRatioX", V::D(a(3).d())),
                ("$originalRatioZ", V::D(a(4).d())),
                ("$canceled", V::I(0)),
            ],
        ),
        DEATH => put_all(rt, ev, &[("$entity", a(1)), ("$source", a(2)), ("$canceled", V::I(0))]),
        DROPS => put_all(rt, ev, &[("$entity", a(1)), ("$source", a(2)), ("$drops", a(3)), ("$recentlyHit", V::I(a(4).i())), ("$canceled", V::I(0))]),
        _ => return None,
    }
    Some(V::Void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{DamageTypeView, MemoryHost};

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        Box::leak(format!("{owner}.{name}.{desc}").into_boxed_str())
    }

    fn view(id: &str, msg: &str, tags: &[&str]) -> DamageTypeView {
        DamageTypeView { id: id.into(), msg_id: msg.into(), exhaustion: 0.1, tags: tags.iter().map(|t| (*t).to_owned()).collect() }
    }

    /// A server's world: four of the game's damage types, a mod's type and tag file, a cow and a
    /// player whose entity id is 42.
    fn world() -> MemoryHost {
        let mut h = MemoryHost::default();
        h.damage_types = vec![
            view("minecraft:in_fire", "inFire", &["minecraft:is_fire", "minecraft:bypasses_armor"]),
            view("minecraft:campfire", "inFire", &["minecraft:is_fire"]),
            view("minecraft:arrow", "arrow", &["minecraft:is_projectile"]),
            view("minecraft:player_attack", "player", &[]),
            view("minecraft:lava", "lava", &["minecraft:is_fire"]),
        ];
        h.resources.insert("data/create/damage_type/crush.json".into(), br#"{"exhaustion": 0.0, "message_id": "create.crush", "scaling": "always"}"#.to_vec());
        h.resources.insert("data/minecraft/tags/damage_type/bypasses_armor.json".into(), br##"{"values": ["create:crush", {"id": "create:saw", "required": false}, "#minecraft:is_fire"]}"##.to_vec());
        h.entities.push(EntityView { id: 7, kind: "minecraft:cow".into(), pos: [1.5, 64.0, -2.5], yaw: 0.0, health: Some((10.0, 10.0)) });
        h.players.push((0xabc, [4.5, 64.0, 4.5]));
        h.player_entity_ids.insert(0xabc, 42);
        h
    }

    fn heard(rt: &mut Rt) -> Vec<String> {
        let list = rt.interned["test:heard"];
        crate::jutil::elements(rt, list).into_iter().map(|v| rt.str_of(v).unwrap_or("").to_owned()).collect()
    }

    fn note(rt: &mut Rt, line: String) {
        let list = rt.interned["test:heard"];
        let s = rt.string(&line);
        crate::jutil::list_add(rt, list, s);
    }

    fn text(rt: &Rt, v: V) -> String {
        rt.str_of(v).unwrap_or("null").to_owned()
    }

    fn id_of(rt: &mut Rt, e: V) -> String {
        if e.r() == 0 {
            return "none".into();
        }
        if rt.instance_of(e, PLAYER) {
            return "player".into();
        }
        crate::api::call(rt, key("net/minecraft/world/entity/Entity", "getId", "()I"), &[e]).i().to_string()
    }

    /// Listeners as mods write them: one halves fire damage and keeps players from arrows, one
    /// raises what is left of a hit by 1 and one reads it once dealt, one doubles a push and
    /// cancels it for a player, one keeps a cow from dying and heals it.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        let source_of = |rt: &mut Rt, owner: &str| crate::api::call(rt, key(owner, "getSource", "()Lnet/minecraft/world/damagesource/DamageSource;"), &[ev]);
        match id {
            1 => {
                let s = source_of(rt, INCOMING);
                let fire = crate::api::get_static(rt, "net/minecraft/tags/DamageTypeTags.IS_FIRE");
                let is_fire = crate::api::call(rt, key(SOURCE, "is", "(Lnet/minecraft/tags/TagKey;)Z"), &[s, fire]).i() != 0;
                let arrow = crate::api::get_static(rt, "net/minecraft/world/damagesource/DamageTypes.ARROW");
                let is_arrow = crate::api::call(rt, key(SOURCE, "is", "(Lnet/minecraft/resources/ResourceKey;)Z"), &[s, arrow]).i() != 0;
                let victim = crate::api::call(rt, key(LIVING_EVENT, "getEntity", "()Lnet/minecraft/world/entity/LivingEntity;"), &[ev]);
                let amount = crate::api::call(rt, key(INCOMING, "getAmount", "()F"), &[ev]).f();
                let msg = crate::api::call(rt, key(SOURCE, "getMsgId", "()Ljava/lang/String;"), &[s]);
                let (by, direct) = {
                    let by = crate::api::call(rt, key(SOURCE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[s]);
                    let direct = crate::api::call(rt, key(SOURCE, "getDirectEntity", "()Lnet/minecraft/world/entity/Entity;"), &[s]);
                    (id_of(rt, by), id_of(rt, direct))
                };
                let who = id_of(rt, victim);
                let msg = text(rt, msg);
                note(rt, format!("incoming {amount} {msg} on {who} by {by} through {direct}"));
                if is_fire {
                    crate::api::call(rt, key(INCOMING, "setAmount", "(F)V"), &[ev, V::F(amount / 2.0)]);
                }
                if is_arrow && rt.instance_of(victim, PLAYER) {
                    crate::api::call(rt, key(CANCELLABLE, "setCanceled", "(Z)V"), &[ev, V::I(1)]);
                }
            }
            2 => {
                let new = crate::api::call(rt, key(PRE, "getNewDamage", "()F"), &[ev]).f();
                let original = crate::api::call(rt, key(PRE, "getOriginalDamage", "()F"), &[ev]).f();
                note(rt, format!("pre {new} of {original}"));
                crate::api::call(rt, key(PRE, "setNewDamage", "(F)V"), &[ev, V::F(new + 1.0)]);
            }
            3 => {
                let new = crate::api::call(rt, key(POST, "getNewDamage", "()F"), &[ev]).f();
                let original = crate::api::call(rt, key(POST, "getOriginalDamage", "()F"), &[ev]).f();
                let s = source_of(rt, POST);
                let t = crate::api::call(rt, key(SOURCE, "type", "()Lnet/minecraft/world/damagesource/DamageType;"), &[s]);
                let msg = crate::api::call(rt, key(DAMAGE_TYPE, "msgId", "()Ljava/lang/String;"), &[t]);
                let msg = text(rt, msg);
                note(rt, format!("post {new} of {original} {msg}"));
            }
            4 => {
                let victim = crate::api::call(rt, key(LIVING_EVENT, "getEntity", "()Lnet/minecraft/world/entity/LivingEntity;"), &[ev]);
                let strength = crate::api::call(rt, key(KNOCKBACK, "getStrength", "()F"), &[ev]).f();
                note(rt, format!("knockback {strength}"));
                if rt.instance_of(victim, PLAYER) {
                    crate::api::call(rt, key(KNOCKBACK, "setCanceled", "(Z)V"), &[ev, V::I(1)]);
                } else {
                    crate::api::call(rt, key(KNOCKBACK, "setStrength", "(F)V"), &[ev, V::F(strength * 2.0)]);
                    crate::api::call(rt, key(KNOCKBACK, "setRatioX", "(D)V"), &[ev, V::D(1.0)]);
                }
            }
            5 => {
                let victim = crate::api::call(rt, key(LIVING_EVENT, "getEntity", "()Lnet/minecraft/world/entity/LivingEntity;"), &[ev]);
                let s = source_of(rt, DEATH);
                let msg = crate::api::call(rt, key(SOURCE, "getMsgId", "()Ljava/lang/String;"), &[s]);
                let by = crate::api::call(rt, key(SOURCE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[s]);
                let (who, by, msg) = (id_of(rt, victim), id_of(rt, by), text(rt, msg));
                note(rt, format!("death of {who} by {by} {msg}"));
                if who == "7" {
                    crate::api::call(rt, key(DEATH, "setCanceled", "(Z)V"), &[ev, V::I(1)]);
                    crate::api::call(rt, key(LIVING, "setHealth", "(F)V"), &[victim, V::F(4.0)]);
                }
            }
            // As Create's crushing wheels still a kill's drops, and other mods add and take some.
            6 => {
                let s = source_of(rt, DROPS);
                let msg = crate::api::call(rt, key(SOURCE, "getMsgId", "()Ljava/lang/String;"), &[s]);
                let msg = text(rt, msg);
                let recently = crate::api::call(rt, key(DROPS, "isRecentlyHit", "()Z"), &[ev]).i() != 0;
                let drops = crate::api::call(rt, key(DROPS, "getDrops", "()Ljava/util/Collection;"), &[ev]);
                let n = crate::api::call(rt, key("java/util/Collection", "size", "()I"), &[drops]).i();
                note(rt, format!("drops {n} by {msg}, recently hit {recently}"));
                if msg == "inFire" {
                    crate::api::call(rt, key(DROPS, "setCanceled", "(Z)V"), &[ev, V::I(1)]);
                    return Some(V::Void);
                }
                let entity = "net/minecraft/world/entity/Entity";
                for e in crate::jutil::elements(rt, drops) {
                    crate::api::call(rt, key(entity, "setDeltaMovement", "(DDD)V"), &[e, V::D(0.0), V::D(0.0), V::D(0.0)]);
                    let stack = crate::api::call(rt, key(crate::entities::ITEM_ENTITY, "getItem", "()Lnet/minecraft/world/item/ItemStack;"), &[e]);
                    let item = crate::api_common::item_like_name(rt, rt.get_field(stack, "$item"));
                    if item == "minecraft:leather" {
                        crate::api::call(rt, key("java/util/Collection", "remove", "(Ljava/lang/Object;)Z"), &[drops, e]);
                    }
                }
                let level = crate::glue::level(rt);
                let diamond = crate::api_common::stack(rt, "minecraft:diamond", 1);
                let e = rt.new_object(crate::entities::ITEM_ENTITY);
                let init = key(crate::entities::ITEM_ENTITY, "<init>", "(Lnet/minecraft/world/level/Level;DDDLnet/minecraft/world/item/ItemStack;DDD)V");
                crate::api::call(rt, init, &[e, level, V::D(0.5), V::D(70.0), V::D(0.5), diamond, V::D(0.0), V::D(0.3), V::D(0.0)]);
                crate::api::call(rt, key("java/util/Collection", "add", "(Ljava/lang/Object;)Z"), &[drops, e]);
            }
            // As Farmer's Delight's skillet: what the striking player holds, a clang at the victim and
            // a word over the player's hotbar.
            7 => {
                let s = source_of(rt, PRE);
                let by = crate::api::call(rt, key(SOURCE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[s]);
                if !rt.instance_of(by, PLAYER) {
                    note(rt, "pre by no player".into());
                    return Some(V::Void);
                }
                let main = crate::hand::constant(rt, crate::hand::MAIN);
                let held = crate::api::call(rt, key(PLAYER, "getItemInHand", "(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;"), &[by, main]);
                let held = crate::api_common::item_like_name(rt, rt.get_field(held, "$item"));
                let weapon = crate::api::call(rt, key(SOURCE, "getWeaponItem", "()Lnet/minecraft/world/item/ItemStack;"), &[s]);
                let weapon = crate::api_common::item_like_name(rt, rt.get_field(weapon, "$item"));
                note(rt, format!("pre by a player holding {held}, weapon {weapon}"));
                let victim = crate::api::call(rt, key(LIVING_EVENT, "getEntity", "()Lnet/minecraft/world/entity/LivingEntity;"), &[ev]);
                let entity = "net/minecraft/world/entity/Entity";
                let at: Vec<V> = ["getX", "getY", "getZ"].into_iter().map(|m| crate::api::call(rt, key(entity, m, "()D"), &[victim])).collect();
                let level = crate::glue::level(rt);
                let clang = crate::api::get_static(rt, "net/minecraft/sounds/SoundEvents.ANVIL_LAND");
                let players = crate::api::get_static(rt, "net/minecraft/sounds/SoundSource.PLAYERS");
                let play = "net/minecraft/world/level/Level.playSound.(Lnet/minecraft/world/entity/player/Player;DDDLnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundSource;FF)V";
                crate::api::call(rt, play, &[level, V::NULL, at[0], at[1], at[2], clang, players, V::F(1.0), V::F(0.5)]);
                let text = rt.string("bonk");
                let line = crate::api::call(rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[text]);
                crate::api::call(rt, key(PLAYER, "displayClientMessage", "(Lnet/minecraft/network/chat/Component;Z)V"), &[by, line, V::I(1)]);
            }
            // A mob's killer told of its death, in the mobs' tick with nobody acting.
            8 => {
                let s = source_of(rt, DEATH);
                let by = crate::api::call(rt, key(SOURCE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[s]);
                if rt.instance_of(by, PLAYER) {
                    let text = rt.string("got it");
                    let line = crate::api::call(rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[text]);
                    crate::api::call(rt, key(PLAYER, "sendSystemMessage", "(Lnet/minecraft/network/chat/Component;)V"), &[by, line]);
                }
            }
            // As Farmer's Delight's knife: whoever gets the credit for the victim (who just hit it),
            // if they hold a knife, pushes it less.
            9 => {
                let victim = crate::api::call(rt, key(LIVING_EVENT, "getEntity", "()Lnet/minecraft/world/entity/LivingEntity;"), &[ev]);
                let by = crate::api::call(rt, key(LIVING, "getKillCredit", "()Lnet/minecraft/world/entity/LivingEntity;"), &[victim]);
                if by.r() == 0 {
                    note(rt, "knockback with no credit".into());
                    return Some(V::Void);
                }
                let main = crate::hand::constant(rt, crate::hand::MAIN);
                let held = crate::api::call(rt, key(LIVING, "getItemInHand", "(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;"), &[by, main]);
                let held = crate::api_common::item_like_name(rt, rt.get_field(held, "$item"));
                let who = id_of(rt, by);
                note(rt, format!("knockback credited to {who} holding {held}"));
                if held.ends_with("_knife") {
                    let original = crate::api::call(rt, key(KNOCKBACK, "getOriginalStrength", "()F"), &[ev]).f();
                    crate::api::call(rt, key(KNOCKBACK, "setStrength", "(F)V"), &[ev, V::F(original - 0.1)]);
                }
            }
            // As Farmer's Delight's backstabbing: a blow from behind the victim, as it looks, hurts
            // double, on the server's side.
            10 => {
                let s = source_of(rt, INCOMING);
                let by = crate::api::call(rt, key(SOURCE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[s]);
                let victim = crate::api::call(rt, key(LIVING_EVENT, "getEntity", "()Lnet/minecraft/world/entity/LivingEntity;"), &[ev]);
                let (entity, vec3) = ("net/minecraft/world/entity/Entity", "net/minecraft/world/phys/Vec3");
                let looks = crate::api::call(rt, key(LIVING, "getViewVector", "(F)Lnet/minecraft/world/phys/Vec3;"), &[victim, V::F(1.0)]);
                let at = crate::api::call(rt, key(SOURCE, "getSourcePosition", "()Lnet/minecraft/world/phys/Vec3;"), &[s]);
                let here = crate::api::call(rt, key(entity, "position", "()Lnet/minecraft/world/phys/Vec3;"), &[victim]);
                let to = crate::api::call(rt, key(vec3, "subtract", "(Lnet/minecraft/world/phys/Vec3;)Lnet/minecraft/world/phys/Vec3;"), &[at, here]);
                let to = crate::api::call(rt, key(vec3, "normalize", "()Lnet/minecraft/world/phys/Vec3;"), &[to]);
                let flat = rt.new_object(vec3);
                let (x, z) = (rt.get_field(to, "x"), rt.get_field(to, "z"));
                crate::api::call(rt, key(vec3, "<init>", "(DDD)V"), &[flat, x, V::D(0.0), z]);
                let dot = crate::api::call(rt, key(vec3, "dot", "(Lnet/minecraft/world/phys/Vec3;)D"), &[flat, looks]).d();
                let level = crate::api::call(rt, key(entity, "level", "()Lnet/minecraft/world/level/Level;"), &[by]);
                let client = crate::api::call(rt, "net/minecraft/world/level/Level.$isClientSide.()Z", &[level]).i() != 0;
                let behind = dot < -0.5;
                note(rt, format!("incoming from behind {behind}, on the client {client}"));
                if behind && !client {
                    let amount = crate::api::call(rt, key(INCOMING, "getAmount", "()F"), &[ev]).f();
                    crate::api::call(rt, key(INCOMING, "setAmount", "(F)V"), &[ev, V::F(amount * 2.0)]);
                }
            }
            _ => return None,
        }
        Some(V::Void)
    }

    fn listening(events: &[(&str, u32)]) -> Rt {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lambdas);
        for &(class, id) in events {
            let l = rt.lambda_id("java/util/function/Consumer", id, vec![]);
            rt.listeners.push((class.to_owned(), l));
        }
        let list = crate::jutil::new_list(&mut rt, Vec::new());
        rt.interned.insert("test:heard".into(), list);
        rt
    }

    fn cow(health: f32) -> EntityView {
        EntityView { id: 7, kind: "minecraft:cow".into(), pos: [1.5, 64.0, -2.5], yaw: 0.0, health: Some((health, 10.0)) }
    }

    /// The sources a mod makes read as vanilla's: their types (a mod's own from its data, with the
    /// tags its tag files give), entities, messages and tags.
    #[test]
    fn a_mods_damage_sources_read_as_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let mut h = world();
        rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            let sources = crate::api::call(rt, key("net/minecraft/world/level/Level", "damageSources", "()Lnet/minecraft/world/damagesource/DamageSources;"), &[level]);
            // `damageSources().inFire()`: one source, of the game's `in_fire`.
            let in_fire = crate::api::call(rt, key(SOURCES, "inFire", "()Lnet/minecraft/world/damagesource/DamageSource;"), &[sources]);
            assert_eq!(crate::api::call(rt, key(SOURCES, "inFire", "()Lnet/minecraft/world/damagesource/DamageSource;"), &[sources]), in_fire);
            let msg = crate::api::call(rt, key(SOURCE, "getMsgId", "()Ljava/lang/String;"), &[in_fire]);
            assert_eq!(rt.str_of(msg), Some("inFire"));
            let exhaustion = crate::api::call(rt, key(SOURCE, "getFoodExhaustion", "()F"), &[in_fire]);
            assert_eq!(exhaustion.f(), 0.1);
            let fire = crate::api::get_static(rt, "net/minecraft/tags/DamageTypeTags.IS_FIRE");
            let is = key(SOURCE, "is", "(Lnet/minecraft/tags/TagKey;)Z");
            assert_eq!(crate::api::call(rt, is, &[in_fire, fire]).i(), 1);
            let in_fire_key = crate::api::get_static(rt, "net/minecraft/world/damagesource/DamageTypes.IN_FIRE");
            assert_eq!(crate::api::call(rt, key(SOURCE, "is", "(Lnet/minecraft/resources/ResourceKey;)Z"), &[in_fire, in_fire_key]).i(), 1);
            let shown = crate::api::call(rt, key(SOURCE, "toString", "()Ljava/lang/String;"), &[in_fire]);
            assert_eq!(rt.str_of(shown), Some("DamageSource (inFire)"));

            // Create's way: the registry's holder of its own key, then `new DamageSource(holder, e)`.
            let registry = crate::api::get_static(rt, "net/minecraft/core/registries/Registries.DAMAGE_TYPE");
            let access = crate::api::call(rt, key("net/minecraft/world/level/Level", "registryAccess", "()Lnet/minecraft/core/RegistryAccess;"), &[level]);
            let types = crate::api::call(rt, key("net/minecraft/core/RegistryAccess", "registryOrThrow", "(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/Registry;"), &[access, registry]);
            let crush_key = rt.string("damage_type#create:crush");
            let holder = crate::api::call(rt, key("net/minecraft/core/Registry", "getHolderOrThrow", "(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/Holder$Reference;"), &[types, crush_key]);
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            let crush = rt.new_object(SOURCE);
            crate::api::call(rt, key(SOURCE, "<init>", "(Lnet/minecraft/core/Holder;Lnet/minecraft/world/entity/Entity;)V"), &[crush, holder, cow]);
            let msg = crate::api::call(rt, key(SOURCE, "getMsgId", "()Ljava/lang/String;"), &[crush]);
            assert_eq!(rt.str_of(msg), Some("create.crush"));
            let armor = crate::api::get_static(rt, "net/minecraft/tags/DamageTypeTags.BYPASSES_ARMOR");
            assert_eq!(crate::api::call(rt, is, &[crush, armor]).i(), 1, "Create's tag file puts crush there");
            assert_eq!(crate::api::call(rt, is, &[crush, fire]).i(), 0);
            // The tag's `#minecraft:is_fire` brings lava in too.
            let lava = crate::api::call(rt, key(SOURCES, "lava", "()Lnet/minecraft/world/damagesource/DamageSource;"), &[sources]);
            assert_eq!(crate::api::call(rt, is, &[lava, armor]).i(), 1);
            assert_eq!(crate::api::call(rt, key(SOURCE, "getEntity", "()Lnet/minecraft/world/entity/Entity;"), &[crush]), cow);
            assert_eq!(crate::api::call(rt, key(SOURCE, "isDirect", "()Z"), &[crush]).i(), 1);
            let at = crate::api::call(rt, key(SOURCE, "getSourcePosition", "()Lnet/minecraft/world/phys/Vec3;"), &[crush]);
            assert_eq!(rt.get_field(at, "x").d(), 1.5);

            // An arrow's: shot by the cow, its direct entity the arrow (an entity the runtime hasn't met).
            let arrow = crate::vanilla_entities::unseen(rt, 99, false);
            let shot = crate::api::call(rt, key(SOURCES, "arrow", "(Lnet/minecraft/world/entity/projectile/AbstractArrow;Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/damagesource/DamageSource;"), &[sources, arrow, cow]);
            assert_eq!(crate::api::call(rt, key(SOURCE, "getDirectEntity", "()Lnet/minecraft/world/entity/Entity;"), &[shot]), arrow);
            assert_eq!(crate::api::call(rt, key(SOURCE, "isDirect", "()Z"), &[shot]).i(), 0);
            let projectile = crate::api::get_static(rt, "net/minecraft/tags/DamageTypeTags.IS_PROJECTILE");
            assert_eq!(crate::api::call(rt, is, &[shot, projectile]).i(), 1);
            // `explosion(direct, causing)` of both is a player's explosion, of neither a plain one.
            let both = crate::api::call(rt, key(SOURCES, "explosion", "(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/damagesource/DamageSource;"), &[sources, arrow, cow]);
            let none = crate::api::call(rt, key(SOURCES, "explosion", "(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/damagesource/DamageSource;"), &[sources, V::NULL, V::NULL]);
            let player_explosion = crate::api::get_static(rt, "net/minecraft/world/damagesource/DamageTypes.PLAYER_EXPLOSION");
            let explosion_key = crate::api::get_static(rt, "net/minecraft/world/damagesource/DamageTypes.EXPLOSION");
            let is_key = key(SOURCE, "is", "(Lnet/minecraft/resources/ResourceKey;)Z");
            assert_eq!(crate::api::call(rt, is_key, &[both, player_explosion]).i(), 1);
            assert_eq!(crate::api::call(rt, is_key, &[none, explosion_key]).i(), 1);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A hit on the game's cow: the incoming event tells its type, amount and attacker, a listener
    /// halves fire; `Pre` tells what armour left with the hit's first amount, and raises it; `Post`
    /// what reached the health. A player's arrow the listener cancels; a push it doubles.
    #[test]
    fn a_hit_reaches_the_mods_and_their_changes_come_back() {
        let mut rt = listening(&[(INCOMING, 1), (PRE, 2), (POST, 3), (KNOCKBACK, 4)]);
        let mut h = world();
        // The server's tick: the game's damage types and the players by entity id.
        rt.with_host(&mut h, server_tick);
        let fire = Source { damage_type: "minecraft:in_fire", causing: 0, direct: 0 };
        let mut amount = 6.0;
        assert!(!incoming(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(10.0)), fire, &mut amount));
        assert_eq!(amount, 3.0, "halved");
        // Armour took 1 of the 3.
        let mut left = 2.0;
        damage_pre(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(10.0)), fire, &mut left);
        assert_eq!(left, 3.0, "raised by 1");
        damage_post(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(7.0)), fire, 3.0);

        // The player shoots the cow: its source names the player as the shooter.
        let shot = Source { damage_type: "minecraft:arrow", causing: 42, direct: 99 };
        let mut amount = 4.0;
        assert!(!incoming(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(7.0)), shot, &mut amount));
        // The cow's arrow on the player: canceled.
        let back = Source { damage_type: "minecraft:arrow", causing: 7, direct: 98 };
        let mut amount = 4.0;
        assert!(incoming(&mut rt, &mut h, 42, "minecraft:player", None, back, &mut amount));

        let (mut strength, mut x, mut z) = (0.4, 0.5, -0.5);
        assert!(!knockback(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(4.0)), 42, &mut strength, &mut x, &mut z));
        assert_eq!((strength as f32, x, z), (0.8, 1.0, -0.5));
        let (mut strength, mut x, mut z) = (0.4, 0.5, -0.5);
        assert!(knockback(&mut rt, &mut h, 42, "minecraft:player", None, 7, &mut strength, &mut x, &mut z), "the player isn't pushed");
        // The player behind the arrow is of `EntityType.PLAYER`, as a listener asks its type's name.
        let player = rt.interned[&format!("{PLAYER_ENTITY}42")];
        let ty = crate::api::call(&mut rt, "net/minecraft/world/entity/Entity.getType.()Lnet/minecraft/world/entity/EntityType;", &[player]);
        assert_eq!(crate::mod_entities::type_id(&rt, ty), Some("minecraft:player"));
        assert_eq!(
            heard(&mut rt),
            [
                "incoming 6 inFire on 7 by none through none",
                "pre 2 of 6",
                "post 3 of 6 inFire",
                "incoming 4 arrow on 7 by player through 99",
                "incoming 4 arrow on player by 7 through 98",
                "knockback 0.4",
                "knockback 0.4",
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A blow's hooks read the striking player as the blow started and what they held (Farmer's
    /// Delight's skillet asks its holder's main hand, its backstabbing the source's weapon), and what
    /// a listener does there, a sound and a word to the player, reaches the hit's host, which makes it
    /// once the hit is over. A mob's death tells its killer by their UUID, with nobody acting.
    #[test]
    fn a_hits_listener_reads_the_weapon_and_its_sound_and_message_reach_the_game() {
        use crate::host::{MemoryHost, Sound};
        let mut rt = listening(&[(PRE, 7), (DEATH, 8)]);
        let mut h = world();
        rt.with_host(&mut h, server_tick);
        // The hit's host: the player 0xabc (entity 42) strikes with a skillet.
        let mut hit = MemoryHost { player_id: Some(0xabc), held: Some(("farmersdelight:skillet".into(), 1)), players: h.players.clone(), ..MemoryHost::default() };
        let blow = Source { damage_type: "minecraft:player_attack", causing: 42, direct: 42 };
        let mut left = 4.0;
        damage_pre(&mut rt, &mut hit, 7, "minecraft:cow", Some(&cow(10.0)), blow, &mut left);
        let clang = Sound { event: "minecraft:block.anvil.land".into(), at: [1.5, 64.0, -2.5], source: 7, volume: 1.0, pitch: 0.5 };
        assert_eq!(hit.sounds, [clang]);
        assert_eq!(hit.messages, [(r#"{"text":"bonk"}"#.to_owned(), true)]);
        // A fall's: no player strikes.
        let fall = Source { damage_type: "minecraft:fall", causing: 0, direct: 0 };
        damage_pre(&mut rt, &mut MemoryHost::default(), 7, "minecraft:cow", Some(&cow(10.0)), fall, &mut left);
        // The cow dies of the blow in the mobs' tick: its killer is told by their UUID.
        assert!(!death(&mut rt, &mut h, 7, "minecraft:cow", blow));
        assert_eq!(h.told, [(0xabc, r#"{"text":"got it"}"#.to_owned(), false)]);
        assert!(h.messages.is_empty(), "nobody acts");
        assert_eq!(heard(&mut rt), ["pre by a player holding farmersdelight:skillet, weapon farmersdelight:skillet", "pre by no player"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What Farmer's Delight's knife and backstabbing ask of a hit's victim: who gets the credit for
    /// it (the player who just hit it, whose knife then pushes it less), and where it looks (a blow
    /// from behind it, on the server's side, hurts double).
    #[test]
    fn a_hits_listener_finds_its_attacker_and_where_the_victim_looks() {
        let mut rt = listening(&[(INCOMING, 10), (KNOCKBACK, 9)]);
        let mut h = world();
        rt.with_host(&mut h, server_tick);
        h.player_id = Some(0xabc);
        h.held = Some(("farmersdelight:flint_knife".into(), 1));
        let blow = Source { damage_type: "minecraft:player_attack", causing: 42, direct: 42 };
        // The cow looks south (yaw 0), towards the player at 4.5 64 4.5: a blow from in front.
        let mut amount = 2.0;
        assert!(!incoming(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(10.0)), blow, &mut amount));
        assert_eq!(amount, 2.0);
        let (mut strength, mut x, mut z) = (0.4, 0.5, -0.5);
        assert!(!knockback(&mut rt, &mut h, 7, "minecraft:cow", Some(&cow(8.0)), 42, &mut strength, &mut x, &mut z));
        assert_eq!(strength as f32, 0.3, "the knife's push");
        // Turned north (yaw 180), its back is to the player.
        let away = EntityView { yaw: 180.0, ..cow(8.0) };
        let mut amount = 2.0;
        assert!(!incoming(&mut rt, &mut h, 7, "minecraft:cow", Some(&away), blow, &mut amount));
        assert_eq!(amount, 4.0, "doubled");
        // A pig nobody hurt has nobody to credit.
        let pig = EntityView { id: 8, kind: "minecraft:pig".into(), pos: [0.5, 64.0, 0.5], yaw: 0.0, health: Some((10.0, 10.0)) };
        let (mut strength, mut x, mut z) = (0.4, 0.5, -0.5);
        assert!(!knockback(&mut rt, &mut h, 8, "minecraft:pig", Some(&pig), 0, &mut strength, &mut x, &mut z));
        assert_eq!(strength, 0.4);
        assert_eq!(
            heard(&mut rt),
            [
                "incoming from behind false, on the client false",
                "knockback credited to player holding farmersdelight:flint_knife",
                "incoming from behind true, on the client false",
                "knockback with no credit",
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A dead mob's drops reach the mods as item entities: a listener stills them, takes the leather
    /// and adds a diamond, and the game gets them back so, the beef it gave still its own (its stack
    /// unchanged). One that cancels them leaves none; with no listener nothing is asked.
    #[test]
    fn a_mods_drops_listener_changes_the_drops() {
        use crate::host::HandStack;
        let stack = |item: &str, count| HandStack { item: item.into(), count, ..HandStack::default() };
        let drop = |item: &str, count, motion| Drop { stack: stack(item, count), position: [1.5, 64.5, -2.5], motion, pickup_delay: 10, origin: None };
        let given = || vec![drop("minecraft:beef", 2, [0.05, 0.2, -0.03]), drop("minecraft:leather", 1, [-0.02, 0.2, 0.01])];
        let blow = Source { damage_type: "minecraft:player_attack", causing: 42, direct: 42 };

        let mut rt = listening(&[]);
        let mut h = world();
        let mut drops = given();
        assert_eq!(living_drops(&mut rt, &mut h, 7, "minecraft:cow", blow, true, &mut drops), None);
        assert_eq!(drops, given());

        let mut rt = listening(&[(DROPS, 6)]);
        rt.with_host(&mut h, server_tick);
        let mut drops = given();
        assert_eq!(living_drops(&mut rt, &mut h, 7, "minecraft:cow", blow, true, &mut drops), Some(false));
        let diamond = Drop { stack: stack("minecraft:diamond", 1), position: [0.5, 70.0, 0.5], motion: [0.0, 0.3, 0.0], pickup_delay: 0, origin: None };
        assert_eq!(drops, vec![Drop { origin: Some(0), ..drop("minecraft:beef", 2, [0.0; 3]) }, diamond]);
        let fire = Source { damage_type: "minecraft:in_fire", causing: 0, direct: 0 };
        let mut drops = given();
        assert_eq!(living_drops(&mut rt, &mut h, 7, "minecraft:cow", fire, false, &mut drops), Some(true));
        assert_eq!(heard(&mut rt), ["drops 2 by player, recently hit true", "drops 2 by inFire, recently hit false"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mob's death reaches the mods with what killed it; one that cancels it and heals the cow
    /// keeps it alive in the game. A player's death is told with the type its message names.
    #[test]
    fn a_death_the_mods_cancel_keeps_the_mob_alive() {
        let mut rt = listening(&[(DEATH, 5)]);
        let mut h = world();
        rt.with_host(&mut h, server_tick);
        h.entities[0].health = Some((0.0, 10.0));
        let blow = Source { damage_type: "minecraft:player_attack", causing: 42, direct: 42 };
        assert!(death(&mut rt, &mut h, 7, "minecraft:cow", blow));
        assert_eq!(h.entities[0].health, Some((4.0, 10.0)), "healed");
        player_died(&mut rt, &mut h, 0xabc, Some("death.attack.inFire.player"));
        player_died(&mut rt, &mut h, 0xabc, Some("death.fell.accident.ladder"));
        assert_eq!(heard(&mut rt), ["death of 7 by player player", "death of player by none inFire", "death of player by none fall"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own events: `new LivingDeathEvent(entity, source)` posted on the bus reaches the
    /// listeners for `LivingEvent`, and a `DamageContainer` it makes keeps what it sets.
    #[test]
    fn a_mods_own_damage_events_and_containers() {
        let mut rt = listening(&[(LIVING_EVENT, 5)]);
        let mut h = world();
        rt.with_host(&mut h, |rt| {
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            let sources = sources(rt);
            let s = crate::api::call(rt, key(SOURCES, "magic", "()Lnet/minecraft/world/damagesource/DamageSource;"), &[sources]);
            let c = rt.new_object(CONTAINER);
            crate::api::call(rt, key(CONTAINER, "<init>", "(Lnet/minecraft/world/damagesource/DamageSource;F)V"), &[c, s, V::F(5.0)]);
            crate::api::call(rt, key(CONTAINER, "setNewDamage", "(F)V"), &[c, V::F(2.0)]);
            assert_eq!(crate::api::call(rt, key(CONTAINER, "getNewDamage", "()F"), &[c]).f(), 2.0);
            assert_eq!(crate::api::call(rt, key(CONTAINER, "getOriginalDamage", "()F"), &[c]).f(), 5.0);
            assert_eq!(crate::api::call(rt, key(CONTAINER, "getPostAttackInvulnerabilityTicks", "()I"), &[c]).i(), 20);
            let ev = rt.new_object(DEATH);
            crate::api::call(rt, key(DEATH, "<init>", "(Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/damagesource/DamageSource;)V"), &[ev, cow, s]);
            crate::neo::post(rt, DEATH, ev);
            assert_eq!(crate::api::call(rt, key(DEATH, "isCanceled", "()Z"), &[ev]).i(), 1);
        });
        assert_eq!(heard(&mut rt), ["death of 7 by none magic"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
