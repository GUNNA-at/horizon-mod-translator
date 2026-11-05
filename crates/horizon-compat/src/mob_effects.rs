//! A living entity's mob effects for a mod: `addEffect`, `forceAddEffect`, `hasEffect`,
//! `getEffect`, `removeEffect`, `removeEffectNoUpdate`, `removeAllEffects`, `getActiveEffects` and
//! `getActiveEffectsMap`, on whichever class the mod names them (`LivingEntity`, `Player`,
//! `ServerPlayer`, `LocalPlayer`, a mob's). They are the host's ([`crate::host::Host::effects`],
//! `add_effect`, `remove_effect`): the player in place (the acting player, a client's own) and the
//! level's mobs have the game's own effects, so what a mod gives lasts, ticks and shows as the
//! `/effect` command's does, and a mod sees what potions and beacons gave.
//!
//! An effect is a `MobEffectInstance` (`food.rs`) of an effect's holder: vanilla's `MobEffects`
//! fields and a mod's own `DeferredHolder`s, by their ids (`minecraft:speed`). A read makes new
//! instances of what the host has; one a mod changes afterwards changes nothing. An entity the host
//! doesn't know (a mod's own entity, another player than the one in place, a mob inside a hit's
//! hooks) has no effects and takes none.
use crate::{Rt, V, food, host::EffectView, jutil};

const INSTANCE: &str = "net/minecraft/world/effect/MobEffectInstance";

/// The keys [`call`] answers, by name and descriptor alone: an entity's own effect methods, on any
/// class (the owner names whichever the mod's code did).
pub(crate) fn claims(_owner: &str, name: &str, desc: &str) -> bool {
    entity_method(name, desc)
}

fn entity_method(name: &str, desc: &str) -> bool {
    matches!(
        (name, desc),
        (
            "addEffect",
            "(Lnet/minecraft/world/effect/MobEffectInstance;)Z"
                | "(Lnet/minecraft/world/effect/MobEffectInstance;Lnet/minecraft/world/entity/Entity;)Z"
        ) | ("forceAddEffect", "(Lnet/minecraft/world/effect/MobEffectInstance;Lnet/minecraft/world/entity/Entity;)V")
            | ("hasEffect" | "removeEffect", "(Lnet/minecraft/core/Holder;)Z")
            | ("getEffect" | "removeEffectNoUpdate", "(Lnet/minecraft/core/Holder;)Lnet/minecraft/world/effect/MobEffectInstance;")
            | ("removeAllEffects", "()Z")
            | ("getActiveEffects", "()Ljava/util/Collection;")
            | ("getActiveEffectsMap", "()Ljava/util/Map;")
    )
}

/// An entity's effect methods; None for any other key.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (_, name, desc) = crate::parts(key)?;
    if !entity_method(name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Whose effects: None when the host doesn't know the entity.
    let entity = crate::attributes::entity_of(rt, a(0)).ok();
    Some(match name {
        // `addEffect(instance[, source])`: whether it was added or changed what was there.
        // `forceAddEffect` replaces one of the same effect.
        "addEffect" | "forceAddEffect" => {
            let force = name == "forceAddEffect";
            let added = match (entity, view_of(rt, a(1))) {
                (Some(entity), Some(effect)) => rt.host_mut().is_some_and(|h| h.add_effect(entity, &effect, force)),
                _ => false,
            };
            if force { V::Void } else { V::I(added as i32) }
        }
        "hasEffect" => V::I(find(rt, entity, a(1)).is_some() as i32),
        "getEffect" => match find(rt, entity, a(1)) {
            Some(e) => instance(rt, &e),
            None => V::NULL,
        },
        "removeEffect" => {
            let removed = match (entity, food::effect_id(rt, a(1))) {
                (Some(entity), Some(id)) => rt.host_mut().is_some_and(|h| h.remove_effect(entity, &full(&id))),
                _ => false,
            };
            V::I(removed as i32)
        }
        // The instance taken off, or null.
        "removeEffectNoUpdate" => {
            let Some(had) = find(rt, entity, a(1)) else {
                return Some(V::NULL);
            };
            let removed = entity.is_some_and(|entity| rt.host_mut().is_some_and(|h| h.remove_effect(entity, &had.id)));
            if removed { instance(rt, &had) } else { V::NULL }
        }
        "removeAllEffects" => {
            let mut any = false;
            for e in active(rt, entity) {
                any |= entity.is_some_and(|entity| rt.host_mut().is_some_and(|h| h.remove_effect(entity, &e.id)));
            }
            V::I(any as i32)
        }
        "getActiveEffects" => {
            let effects = active(rt, entity);
            let instances = effects.iter().map(|e| instance(rt, e)).collect();
            jutil::new_list(rt, instances)
        }
        // Each effect's holder to its instance.
        "getActiveEffectsMap" => {
            let effects = active(rt, entity);
            let map = rt.alloc("java/util/HashMap", crate::Data::Table(crate::Table::default()));
            for e in &effects {
                let (holder, instance) = (food::effect_holder(rt, &e.id), instance(rt, e));
                jutil::table_put(rt, map, holder, instance);
            }
            map
        }
        _ => return None,
    })
}

/// An effect's id with its namespace (`speed` is `minecraft:speed`).
fn full(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}

/// The host's effects of `entity`; none for one it doesn't know.
fn active(rt: &mut Rt, entity: Option<Option<i32>>) -> Vec<EffectView> {
    let Some(entity) = entity else {
        return Vec::new();
    };
    rt.host_mut().and_then(|h| h.effects(entity)).unwrap_or_default()
}

/// The effect of `holder` that `entity` has.
fn find(rt: &mut Rt, entity: Option<Option<i32>>, holder: V) -> Option<EffectView> {
    let id = full(&food::effect_id(rt, holder)?);
    active(rt, entity).into_iter().find(|e| full(&e.id) == id)
}

/// What a `MobEffectInstance` gives, for the host; None for one of no effect.
fn view_of(rt: &Rt, instance: V) -> Option<EffectView> {
    if instance.r() == 0 {
        return None;
    }
    let id = full(&food::effect_id(rt, rt.get_field(instance, "$effect"))?);
    let int = |f: &'static str| rt.get_field(instance, f).i();
    Some(EffectView {
        id,
        duration: int("$duration"),
        amplifier: int("$amplifier"),
        ambient: int("$ambient") != 0,
        visible: int("$visible") != 0,
        show_icon: int("$showIcon") != 0,
    })
}

/// A new `MobEffectInstance` of what the host has.
fn instance(rt: &mut Rt, e: &EffectView) -> V {
    let holder = food::effect_holder(rt, &e.id);
    let i = rt.new_object(INSTANCE);
    rt.put_field(i, "$effect", holder);
    rt.put_field(i, "$duration", V::I(e.duration));
    rt.put_field(i, "$amplifier", V::I(e.amplifier));
    rt.put_field(i, "$ambient", V::I(e.ambient as i32));
    rt.put_field(i, "$visible", V::I(e.visible as i32));
    rt.put_field(i, "$showIcon", V::I(e.show_icon as i32));
    i
}

/// `MobEffect`'s methods a mod's own effect may answer, as the game's effect hooks ask them.
const SHOULD_APPLY: &str = "shouldApplyEffectTickThisTick.(II)Z";
const SHOULD_APPLY_KEY: &str = "net/minecraft/world/effect/MobEffect.shouldApplyEffectTickThisTick.(II)Z";
const APPLY: &str = "applyEffectTick.(Lnet/minecraft/world/entity/LivingEntity;I)Z";
const APPLY_KEY: &str = "net/minecraft/world/effect/MobEffect.applyEffectTick.(Lnet/minecraft/world/entity/LivingEntity;I)Z";
const ADDED: &str = "onEffectAdded.(Lnet/minecraft/world/entity/LivingEntity;I)V";
const ADDED_KEY: &str = "net/minecraft/world/effect/MobEffect.onEffectAdded.(Lnet/minecraft/world/entity/LivingEntity;I)V";
const STARTED: &str = "onEffectStarted.(Lnet/minecraft/world/entity/LivingEntity;I)V";
const STARTED_KEY: &str = "net/minecraft/world/effect/MobEffect.onEffectStarted.(Lnet/minecraft/world/entity/LivingEntity;I)V";

/// Every mob effect the mod registered (outside `minecraft`), by id, as `(id, category, colour)`:
/// what `new MobEffect(category, color)` took, the category by its name in lower case
/// (`beneficial`, `harmful`, `neutral`), the colour as RGB. The game knows them as a content mod's
/// effects (`Content::declare_effect`).
pub fn effect_declarations(rt: &mut Rt) -> Vec<(String, String, u32)> {
    let mut effects: Vec<(String, V)> = rt
        .registered
        .iter()
        .filter(|((r, n), _)| r == "mob_effect" && n.split_once(':').is_some_and(|(ns, path)| !ns.is_empty() && ns != "minecraft" && !path.is_empty()))
        .map(|((_, n), v)| (n.clone(), *v))
        .collect();
    effects.sort_by(|a, b| a.0.cmp(&b.0));
    effects
        .into_iter()
        .map(|(id, e)| {
            let category = rt.get_field(e, "$category");
            let category = rt.str_of(rt.get_field(category, "$name")).unwrap_or("NEUTRAL").to_ascii_lowercase();
            let color = (rt.get_field(e, "$color").i() as u32) & 0xFF_FFFF;
            (id, category, color)
        })
        .collect()
}

/// The mod's own effect `id`, when its class answers `nd` itself.
fn own_effect(rt: &Rt, id: &str, nd: &str) -> Option<V> {
    let &e = rt.registered.get(&("mob_effect".to_owned(), id.to_owned()))?;
    let class = rt.class_of(e);
    rt.overrides.is_some_and(|o| o(class, nd)).then_some(e)
}

/// `MobEffect.shouldApplyEffectTickThisTick(duration, amplifier)` of the mod's effect `id`: its own
/// code, which needs no world. None when it isn't the mod's or its class leaves it to vanilla's
/// (which never applies a tick).
pub fn should_apply_tick(rt: &mut Rt, id: &str, duration: i32, amplifier: i32) -> Option<bool> {
    rt.safe_point();
    let e = own_effect(rt, id, SHOULD_APPLY)?;
    let vc = rt.vcall?;
    let r = vc(rt, SHOULD_APPLY_KEY, SHOULD_APPLY, &[e, V::I(duration), V::I(amplifier)]);
    let thrown = rt.thrown.is_some();
    rt.report_thrown("shouldApplyEffectTickThisTick");
    (!thrown).then(|| r.i() != 0)
}

/// The living entity an effect acts on: the player in place (None) or the level's entity `id`,
/// as the host tells of it; None when the host doesn't know it.
fn living(rt: &mut Rt, entity: Option<i32>) -> Option<V> {
    match entity {
        None => Some(crate::api_common::acting_player(rt)),
        Some(id) => {
            let view = rt.host().and_then(|h| h.entity(id))?;
            let v = crate::vanilla_entities::of(rt, id, &view.kind, Some(view.pos));
            (v.r() != 0).then_some(v)
        }
    }
}

/// `MobEffect.applyEffectTick(entity, amplifier)` of the mod's effect `id` on the living entity
/// `entity` (None: the player in place), with `host`'s world: whether the effect stays. None when
/// it isn't the mod's own code, the entity isn't there or the code threw.
pub fn apply_tick(rt: &mut Rt, host: &mut dyn crate::host::Host, id: &str, entity: Option<i32>, amplifier: i32) -> Option<bool> {
    rt.safe_point();
    rt.with_host(host, |rt| {
        let e = own_effect(rt, id, APPLY)?;
        let living = living(rt, entity)?;
        let vc = rt.vcall?;
        let r = vc(rt, APPLY_KEY, APPLY, &[e, living, V::I(amplifier)]);
        let thrown = rt.thrown.is_some();
        rt.report_thrown("applyEffectTick");
        (!thrown).then(|| r.i() != 0)
    })
}

/// The mod's effect `id` was given to the living entity `entity`: its `onEffectAdded(entity,
/// amplifier)` when the entity hadn't had it, or (`started`) its `onEffectStarted`, each time.
pub fn effect_added(rt: &mut Rt, host: &mut dyn crate::host::Host, id: &str, entity: Option<i32>, amplifier: i32, started: bool) {
    rt.safe_point();
    let (nd, key, what) = if started { (STARTED, STARTED_KEY, "onEffectStarted") } else { (ADDED, ADDED_KEY, "onEffectAdded") };
    rt.with_host(host, |rt| {
        crate::glue::trace_some(rt, "effect-added", &format!("{id}/{entity:?}/{what}"), 8, |_| format!("{id} given to {entity:?}: its {what}"));
        let (Some(e), Some(vc)) = (own_effect(rt, id, nd), rt.vcall) else {
            return;
        };
        let Some(living) = living(rt, entity) else {
            crate::glue::trace_once(rt, &format!("effect-entity/{what}"), |_| format!("{id} given to entity {entity:?}, which the game doesn't know: no {what}"));
            return;
        };
        vc(rt, key, nd, &[e, living, V::I(amplifier)]);
        rt.report_thrown(what);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
    const PLAYER: &str = "net/minecraft/world/entity/player/Player";
    const EFFECTS: &str = "net/minecraft/world/effect/MobEffects";
    const HOLDER: &str = "Lnet/minecraft/core/Holder;";

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        format!("{owner}.{name}.{desc}").leak()
    }

    /// `new MobEffectInstance(effect, duration, amplifier)`.
    fn given(rt: &mut Rt, effect: V, duration: i32, amplifier: i32) -> V {
        let i = rt.new_object(INSTANCE);
        crate::api::call(rt, key(INSTANCE, "<init>", "(Lnet/minecraft/core/Holder;II)V"), &[i, effect, V::I(duration), V::I(amplifier)]);
        i
    }

    fn effect(id: &str, duration: i32, amplifier: i32) -> EffectView {
        EffectView { id: id.into(), duration, amplifier, visible: true, show_icon: true, ..EffectView::default() }
    }

    #[test]
    fn a_mod_gives_reads_and_takes_off_the_games_effects() {
        let mut h = MemoryHost { player_id: Some(0xabc), ..MemoryHost::default() };
        h.effects.insert(None, vec![effect("minecraft:speed", 600, 0)]);
        h.effects.insert(Some(7), Vec::new());
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            let player = crate::menus::server_player(rt, 0xabc);
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            let wolf = crate::vanilla_entities::of(rt, 9, "minecraft:wolf", None);
            let speed = crate::api::get_static(rt, format!("{EFFECTS}.MOVEMENT_SPEED").leak());
            // A mod's own effect, by the holder its register gave.
            let comfort = rt.new_object("net/neoforged/neoforge/registries/DeferredHolder");
            let (r, i) = (rt.string("minecraft:mob_effect"), rt.string("farmersdelight:comfort"));
            rt.put_field(comfort, "$registry", r);
            rt.put_field(comfort, "$id", i);

            // What potions gave the player.
            assert_eq!(crate::api::call(rt, key(PLAYER, "hasEffect", &format!("({HOLDER})Z")), &[player, speed]), V::I(1));
            let had = crate::api::call(rt, key(LIVING, "getEffect", &format!("({HOLDER})L{INSTANCE};")), &[player, speed]);
            assert_eq!(crate::api::call(rt, key(INSTANCE, "getAmplifier", "()I"), &[had]), V::I(0));
            assert_eq!(crate::api::call(rt, key(INSTANCE, "getDuration", "()I"), &[had]), V::I(600));
            assert_eq!(crate::api::call(rt, key(INSTANCE, "is", &format!("({HOLDER})Z")), &[had, speed]), V::I(1));
            // A stronger one takes over; a weaker, shorter one changes nothing.
            let add = key(LIVING, "addEffect", &format!("(L{INSTANCE};)Z"));
            let swift = given(rt, speed, 200, 1);
            assert_eq!(crate::api::call(rt, add, &[player, swift]), V::I(1));
            let weak = given(rt, speed, 100, 0);
            assert_eq!(crate::api::call(rt, add, &[player, weak]), V::I(0));

            // The cow takes the mod's effect, which reads back by its holder; then it's taken off.
            let soothed = given(rt, comfort, 400, 0);
            assert_eq!(crate::api::call(rt, key(LIVING, "addEffect", &format!("(L{INSTANCE};Lnet/minecraft/world/entity/Entity;)Z")), &[cow, soothed, player]), V::I(1));
            let all = crate::api::call(rt, key(LIVING, "getActiveEffects", "()Ljava/util/Collection;"), &[cow]);
            let all = jutil::elements(rt, all);
            assert_eq!(all.len(), 1);
            assert_eq!(crate::api::call(rt, key(INSTANCE, "is", &format!("({HOLDER})Z")), &[all[0], comfort]), V::I(1));
            assert_eq!(crate::api::call(rt, key(INSTANCE, "endsWithin", "(I)Z"), &[all[0], V::I(400)]), V::I(1));
            let remove = key(LIVING, "removeEffect", &format!("({HOLDER})Z"));
            assert_eq!(crate::api::call(rt, remove, &[cow, comfort]), V::I(1));
            assert_eq!(crate::api::call(rt, remove, &[cow, comfort]), V::I(0), "gone already");

            // An entity the host doesn't know has none and takes none.
            let bitten = given(rt, speed, 100, 0);
            assert_eq!(crate::api::call(rt, add, &[wolf, bitten]), V::I(0));
            assert_eq!(crate::api::call(rt, key(LIVING, "getEffect", &format!("({HOLDER})L{INSTANCE};")), &[wolf, speed]), V::NULL);

            assert_eq!(crate::api::call(rt, key(LIVING, "removeAllEffects", "()Z"), &[player]), V::I(1));
        });
        assert_eq!(h.effects[&None], Vec::new(), "all taken off at the end");
        assert_eq!(h.effects[&Some(7)], Vec::new());
        assert!(!h.effects.contains_key(&Some(9)));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_mods_own_effect_is_declared_and_its_code_acts_when_the_game_asks() {
        const STATIC: &str = "test/StaticEffect";
        fn overrides(class: &str, short: &str) -> bool {
            class == STATIC && [SHOULD_APPLY, APPLY, ADDED, STARTED].contains(&short)
        }
        // The mod's effect: it acts every 20 ticks and its third act ends it.
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                SHOULD_APPLY => V::I((args[1].i() % 20 == 0) as i32),
                APPLY => {
                    let acts = rt.get_field(args[0], "acts").i() + 1;
                    rt.put_field(args[0], "acts", V::I(acts));
                    rt.put_field(args[0], "actedOn", args[1]);
                    V::I((acts < 3) as i32)
                }
                ADDED => {
                    rt.put_field(args[0], "addedTo", args[1]);
                    rt.put_field(args[0], "addedAt", args[2]);
                    V::Void
                }
                STARTED => {
                    rt.put_field(args[0], "startedOn", args[1]);
                    V::Void
                }
                _ => crate::api::call(rt, key, args),
            }
        }
        let mut h = MemoryHost { player_id: Some(0xabc), ..MemoryHost::default() };
        h.entities.push(crate::host::EntityView { id: 7, kind: "minecraft:cow".into(), health: Some((10.0, 10.0)), ..Default::default() });
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(STATIC, "net/minecraft/world/effect/MobEffect");
        let harmful = crate::api::get_static(&mut rt, "net/minecraft/world/effect/MobEffectCategory.HARMFUL");
        let effect = rt.new_object(STATIC);
        crate::api::call(&mut rt, "net/minecraft/world/effect/MobEffect.<init>.(Lnet/minecraft/world/effect/MobEffectCategory;I)V", &[effect, harmful, V::I(0x3366ff)]);
        rt.registered.insert(("mob_effect".into(), "hzcheck:static".into()), effect);

        assert_eq!(effect_declarations(&mut rt), [("hzcheck:static".to_owned(), "harmful".to_owned(), 0x3366ff)]);
        assert_eq!(should_apply_tick(&mut rt, "hzcheck:static", 40, 0), Some(true));
        assert_eq!(should_apply_tick(&mut rt, "hzcheck:static", 39, 0), Some(false));
        assert_eq!(should_apply_tick(&mut rt, "minecraft:speed", 40, 0), None, "vanilla's, not the mod's");

        // It acts on the cow until its third act; on an entity the game doesn't know, it doesn't act.
        assert_eq!(apply_tick(&mut rt, &mut h, "hzcheck:static", Some(7), 1), Some(true));
        let cow = rt.get_field(effect, "actedOn");
        assert_eq!(rt.get_field(cow, "$vid"), V::I(7));
        assert_eq!(apply_tick(&mut rt, &mut h, "hzcheck:static", Some(9), 1), None);
        assert_eq!(apply_tick(&mut rt, &mut h, "hzcheck:static", Some(7), 1), Some(true));
        assert_eq!(apply_tick(&mut rt, &mut h, "hzcheck:static", Some(7), 1), Some(false), "the third ends it");

        // Given to the player, who hadn't had it; then started again on the cow.
        effect_added(&mut rt, &mut h, "hzcheck:static", None, 1, false);
        let player = rt.with_host(&mut h, |rt| crate::menus::server_player(rt, 0xabc));
        assert_eq!((rt.get_field(effect, "addedTo"), rt.get_field(effect, "addedAt")), (player, V::I(1)));
        assert_eq!(rt.get_field(effect, "startedOn"), V::Void);
        effect_added(&mut rt, &mut h, "hzcheck:static", Some(7), 0, true);
        assert_eq!(rt.get_field(effect, "startedOn"), cow);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
