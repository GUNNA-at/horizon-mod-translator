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
}
