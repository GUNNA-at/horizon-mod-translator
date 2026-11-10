//! A player's blows and clicks on the mods' own entities (docs/native-mods.md, "Mod entities hit and
//! clicked"): the one the window's view meets ([`aimed`], `native_client_mod_entity_hit`), the blow
//! `Player.attack` deals one on the server ([`attack`], `native_mod_entity_attack`), and
//! `Player.interactOn` with one, on the server and as the window's client predicts it ([`interact`],
//! `native_mod_entity_interact`, `native_client_mod_entity_interact`).
//!
//! The game keeps the player: it works out the blow (the weapon, the attack strength, a critical
//! hit, the enchantments, the extra knockback) and does the player's side of it (sounds, particles,
//! the sweep, the weapon's wear, the exhaustion). The mod keeps the entity: whether it takes the blow
//! (`isAttackable`, `skipAttackInteraction`, `hurt`), and the knockback that pushes it.

use crate::events::Answer;
use crate::{Rt, V, api};

const IS_PICKABLE: (&str, &str) = ("net/minecraft/world/entity/Entity.isPickable.()Z", "isPickable.()Z");
const PICK_RADIUS: (&str, &str) = ("net/minecraft/world/entity/Entity.getPickRadius.()F", "getPickRadius.()F");
const IS_ATTACKABLE: (&str, &str) = ("net/minecraft/world/entity/Entity.isAttackable.()Z", "isAttackable.()Z");
const SKIP_ATTACK: (&str, &str) = (
    "net/minecraft/world/entity/Entity.skipAttackInteraction.(Lnet/minecraft/world/entity/Entity;)Z",
    "skipAttackInteraction.(Lnet/minecraft/world/entity/Entity;)Z",
);
const PUSH: (&str, &str) = ("net/minecraft/world/entity/Entity.push.(DDD)V", "push.(DDD)V");
const INTERACT: (&str, &str) = (
    "net/minecraft/world/entity/Entity.interact.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResult;",
    "interact.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResult;",
);
const INTERACT_LIVING: (&str, &str) = (
    "net/minecraft/world/item/Item.interactLivingEntity.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResult;",
    "interactLivingEntity.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/InteractionResult;",
);

/// The mods' entity the view meets first.
#[derive(Clone, Debug, PartialEq)]
pub struct Aimed {
    /// Its network id.
    pub id: i32,
    /// How far along the view it is met.
    pub distance: f64,
    /// Where the view meets its box.
    pub location: [f64; 3],
    /// Its feet: a click's place on it is `location` less these (`ServerboundInteractPacket`'s).
    pub feet: [f64; 3],
}

/// A player's blow on one of the mods' entities, as the game worked it out (`Player.attack`).
#[derive(Clone, Debug, PartialEq)]
pub struct Blow {
    /// The damage: the weapon's, scaled by the attack strength, a critical hit's, the enchantments'.
    pub amount: f32,
    /// The damage type of the player's source (`minecraft:player_attack`, `minecraft:mace_smash`).
    pub damage_type: String,
    /// The extra knockback's strength (the weapon's knockback enchantment and a sprint's share,
    /// `causeExtraKnockback`), 0 for none: it pushes the entity along `direction`.
    pub knockback: f32,
    /// `(sin yaw, -cos yaw)` of the player's yaw: `knockback(strength, x, z)` sends it the other way,
    /// the way the player looks.
    pub direction: [f64; 2],
}

/// What a blow did to one of the mods' entities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Struck {
    /// Whether it could be hit at all (`isAttackable`, and not `skipAttackInteraction`): when not,
    /// the blow did nothing, and the player's side of it doesn't happen.
    pub attackable: bool,
    /// Whether it took the blow (`hurt`).
    pub landed: bool,
    /// A living one's health before and after the blow.
    pub health: Option<[f32; 2]>,
    /// Its feet, its box's width and height after the blow.
    pub position: [f64; 3],
    pub size: [f32; 2],
    /// `isAlive` after the blow.
    pub alive: bool,
}

/// Where the ray from `from` along the unit vector `look` first meets the box `b` (min x, y, z, max
/// x, y, z) within `reach`: its distance along the ray (0 from inside it).
fn clip(from: [f64; 3], look: [f64; 3], reach: f64, b: [f64; 6]) -> Option<f64> {
    let (mut near, mut far) = (0.0f64, reach);
    for axis in 0..3 {
        let (lo, hi) = (b[axis], b[axis + 3]);
        if look[axis].abs() < 1.0e-12 {
            if from[axis] < lo || from[axis] > hi {
                return None;
            }
            continue;
        }
        let (a, z) = ((lo - from[axis]) / look[axis], (hi - from[axis]) / look[axis]);
        let (a, z) = if a < z { (a, z) } else { (z, a) };
        near = near.max(a);
        far = far.min(z);
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// The mods' entity the window's view meets first, from the eyes at `eye` along `look` within
/// `reach` blocks (`ProjectileUtil.getEntityHitResult`): one that can be picked (`isPickable`), its
/// box grown by its pick radius (`getPickRadius`). The block in the way is the game's to weigh.
pub fn aimed(rt: &mut Rt, eye: [f64; 3], look: [f64; 3], reach: f64) -> Option<Aimed> {
    let len = (look[0] * look[0] + look[1] * look[1] + look[2] * look[2]).sqrt();
    if len < 1.0e-9 {
        return None;
    }
    let look = look.map(|c| c / len);
    let mut best: Option<Aimed> = None;
    for e in crate::mod_entities::live(rt) {
        if rt.get_field(e, "removalReason").r() != 0 || api::virtual_call(rt, IS_PICKABLE, &[e]).i() == 0 {
            continue;
        }
        let grow = f64::from(api::virtual_call(rt, PICK_RADIUS, &[e]).f());
        let b = crate::mobs::bounds(rt, e);
        let grown = [b[0] - grow, b[1] - grow, b[2] - grow, b[3] + grow, b[4] + grow, b[5] + grow];
        if let Some(t) = clip(eye, look, reach, grown)
            && best.as_ref().is_none_or(|b| t < b.distance)
        {
            let location = [eye[0] + look[0] * t, eye[1] + look[1] * t, eye[2] + look[2] * t];
            let feet = crate::mod_entities::xyz(rt, e);
            best = Some(Aimed { id: rt.get_field(e, "$id").i(), distance: t, location, feet });
        }
    }
    rt.report_thrown("a pick of a mod's entity");
    best
}

/// The level `dimension`'s mod entity of network id `id`, or, with no level named (the window's
/// client, which holds one), the client level's.
fn target(rt: &Rt, dimension: Option<&str>, id: i32) -> Option<V> {
    match dimension {
        Some(d) => crate::entity_commands::by_id(rt, d, id),
        None => crate::mod_entities::live_by_id(rt, id),
    }
}

/// The acting player's blow on the level `dimension`'s mod entity `id` (`Player.attack` on a
/// server, past `AttackEntityEvent`): None when there is none. One that can't be hit
/// (`isAttackable`) or skips the player's blow (`skipAttackInteraction`: Create's contraptions
/// take it themselves) takes nothing. Else `hurt` with the player's source and `blow`'s damage; when
/// it lands, the extra knockback pushes it the way the player looks (`knockback(strength, sin yaw,
/// -cos yaw)` for a living one, `push` for another).
pub fn attack(rt: &mut Rt, dimension: &str, id: i32, blow: &Blow) -> Option<Struck> {
    let e = target(rt, Some(dimension), id)?;
    let struck = crate::hand::with_hand(rt, |rt| {
        let player = crate::api_common::acting_player(rt);
        let living = crate::mobs::is_mod_living(rt, e);
        let mut struck = Struck::default();
        let attackable = api::virtual_call(rt, IS_ATTACKABLE, &[e]).i() != 0
            && api::virtual_call(rt, SKIP_ATTACK, &[e, player]).i() == 0;
        if attackable {
            struck.attackable = true;
            let before = living.then(|| crate::mobs::health(rt, e));
            let source = crate::damage::source(rt, &blow.damage_type, player, player, V::NULL);
            struck.landed = crate::mobs::hurt(rt, e, source, blow.amount);
            if struck.landed && blow.knockback > 0.0 {
                let [x, z] = blow.direction;
                let strength = f64::from(blow.knockback);
                if living {
                    crate::mobs::knock_back(rt, e, strength, x, z);
                } else {
                    api::virtual_call(rt, PUSH, &[e, V::D(-x * strength), V::D(0.1), V::D(-z * strength)]);
                }
            }
            struck.health = before.map(|b| [b, crate::mobs::health(rt, e)]);
        }
        struck.position = crate::mod_entities::xyz(rt, e);
        let size = crate::mod_entities::own_size(rt, e);
        struck.size = [size[0], size[1]];
        struck.alive = crate::mobs::is_alive(rt, e);
        struck
    });
    rt.report_thrown("a player's blow on a mod's entity");
    Some(struck)
}

/// What `r` (an `InteractionResult`) answers a click with.
fn answer(rt: &Rt, r: V) -> Answer {
    match rt.str_of(r).unwrap_or("PASS") {
        "SUCCESS" | "SUCCESS_NO_ITEM_USED" => Answer::Success,
        "CONSUME" | "CONSUME_PARTIAL" => Answer::Consume,
        "FAIL" => Answer::Fail,
        _ => Answer::Pass,
    }
}

/// The acting player's click with `hand` (0 main, 1 off) on the mod entity `id` of the level
/// `dimension` (a server's), or of the client's level with none named (the window's prediction):
/// `Player.interactOn` past `EntityInteract`. The entity's own `interact` first (a mod's mob:
/// `mobInteract`, an animal's food); a creative player keeps what it used. When that passes, the
/// held item's `interactLivingEntity` on a living one, a mod's item's own. None when there is no such
/// entity; else what the click came to.
pub fn interact(rt: &mut Rt, dimension: Option<&str>, id: i32, hand: u8) -> Option<Answer> {
    let e = target(rt, dimension, id)?;
    let answer = crate::hand::with_hand(rt, |rt| {
        let player = crate::api_common::acting_player(rt);
        let constant = crate::hand::constant(rt, hand);
        let stack = crate::hand::stack(rt, hand);
        let count = rt.get_field(stack, "$count").i();
        let r = api::virtual_call(rt, INTERACT, &[e, player, constant]);
        let first = answer(rt, r);
        if first.used() {
            // A creative player's stack stays whole (`hasInfiniteMaterials`).
            let creative = rt.host().is_some_and(|h| h.player_creative());
            if creative && crate::hand::stack(rt, hand) == stack && rt.get_field(stack, "$count").i() < count {
                rt.put_field(stack, "$count", V::I(count));
            }
            return first;
        }
        if crate::api_common::stack_empty(rt, stack) || !crate::mobs::is_mod_living(rt, e) {
            return Answer::Pass;
        }
        // Only a mod's item has an `interactLivingEntity` of its own here; vanilla's (a name tag, a
        // saddle, shears) aren't modelled on a mod's entity.
        let item = rt.get_field(stack, "$item");
        if item.r() == 0 || !rt.overrides.is_some_and(|o| o(rt.class_of(item), INTERACT_LIVING.1)) {
            return Answer::Pass;
        }
        let r = api::virtual_call(rt, INTERACT_LIVING, &[item, stack, player, e, constant]);
        match answer(rt, r) {
            a if a.used() => a,
            _ => Answer::Pass,
        }
    });
    rt.report_thrown("a player's click on a mod's entity");
    Some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const INTERACTION_RESULT: &str = "net/minecraft/world/InteractionResult";
    const PENGUIN: &str = "test/Penguin";
    const ANIMAL: &str = "net/minecraft/world/entity/animal/Animal";
    const INIT: &str = "net/minecraft/world/entity/animal/Animal.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
    const SET_AGE: &str = "net/minecraft/world/entity/AgeableMob.setAge.(I)V";
    const LEVEL: &str = "minecraft:overworld";

    /// The penguin eats cod (`isFood`, its own); nothing else of its class is its own.
    fn overrides(class: &str, key: &str) -> bool {
        class == PENGUIN && key.starts_with("isFood.")
    }

    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if short.starts_with("isFood.") {
            let item = rt.get_field(args[1], "$item");
            return V::I(i32::from(crate::api_common::item_like_name(rt, item) == "minecraft:cod"));
        }
        api::call(rt, key, args)
    }

    /// A runtime with a mod's animal class and its type, 0.6 by 1 block.
    fn runtime() -> (Rt, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(PENGUIN, ANIMAL);
        let ty = rt.new_object("net/minecraft/world/entity/EntityType");
        rt.put_field(ty, "$width", V::F(0.6));
        rt.put_field(ty, "$height", V::F(1.0));
        rt.registered.insert(("entity_type".into(), "test:penguin".into()), ty);
        (rt, ty)
    }

    /// A server's world with a stone floor at y 0, the acting player (survival) at `[0.5, 1, -1]`
    /// looking south (+z), holding `held`.
    fn world(held: Option<(&str, u32)>) -> MemoryHost {
        let mut host = MemoryHost::default();
        for x in -8..=8 {
            for z in -8..=8 {
                host.blocks.insert([x, 0, z], "minecraft:stone".into());
            }
        }
        host.player_id = Some(7);
        host.place = Some(([0.5, 1.0, -1.0], 0.0, 0.0));
        host.held = held.map(|(item, n)| (item.to_owned(), n));
        host
    }

    /// A penguin in the level at `at`: its network id.
    fn penguin(rt: &mut Rt, ty: V, at: [f64; 3]) -> (V, i32) {
        let level = crate::glue::level(rt);
        let e = rt.new_object(PENGUIN);
        api::call(rt, INIT, &[e, ty, level]);
        crate::mod_entities::set_pos(rt, e, at);
        assert!(crate::mod_entities::join(rt, e, false));
        (e, rt.get_field(e, "$id").i())
    }

    #[test]
    fn a_grown_animal_eats_one_of_its_food_from_the_hand_and_falls_in_love_once() {
        let (mut rt, ty) = runtime();
        let mut host = world(Some(("minecraft:cod", 5)));
        rt.with_host(&mut host, |rt| {
            let (e, id) = penguin(rt, ty, [0.5, 1.0, 1.5]);
            assert_eq!(interact(rt, Some(LEVEL), id, 0), Some(Answer::Success));
            assert_eq!(rt.get_field(e, "inLove").i(), 600);
            assert_eq!(interact(rt, Some(LEVEL), id, 0), Some(Answer::Pass), "in love already: the click passes");
            assert_eq!(interact(rt, Some(LEVEL), id + 1000, 0), None, "no such entity");
        });
        assert_eq!(host.held, Some(("minecraft:cod".to_owned(), 4)), "one cod eaten");
    }

    #[test]
    fn a_creative_players_food_stays_whole_and_what_isnt_food_passes() {
        let (mut rt, ty) = runtime();
        let mut host = world(Some(("minecraft:cod", 5)));
        host.creative = true;
        rt.with_host(&mut host, |rt| {
            let (e, id) = penguin(rt, ty, [0.5, 1.0, 1.5]);
            assert_eq!(interact(rt, Some(LEVEL), id, 0), Some(Answer::Success));
            assert_eq!(rt.get_field(e, "inLove").i(), 600);
        });
        assert_eq!(host.held, Some(("minecraft:cod".to_owned(), 5)));
        let mut host = world(Some(("minecraft:stick", 3)));
        rt.with_host(&mut host, |rt| {
            let (e, id) = penguin(rt, ty, [2.5, 1.0, 1.5]);
            assert_eq!(interact(rt, Some(LEVEL), id, 0), Some(Answer::Pass));
            assert_eq!(rt.get_field(e, "inLove").i(), 0);
        });
        assert_eq!(host.held, Some(("minecraft:stick".to_owned(), 3)));
    }

    #[test]
    fn a_fed_baby_grows_a_tenth_of_its_time_left_and_a_clients_grown_one_takes_the_click() {
        let (mut rt, ty) = runtime();
        let mut host = world(Some(("minecraft:cod", 2)));
        rt.with_host(&mut host, |rt| {
            let (e, id) = penguin(rt, ty, [0.5, 1.0, 1.5]);
            api::call(rt, SET_AGE, &[e, V::I(-24000)]);
            // `getSpeedUpSecondsWhenFeeding(24000)`: 120 seconds, 2400 ticks.
            assert_eq!(interact(rt, Some(LEVEL), id, 0), Some(Answer::Consume), "a server's sided success");
            assert_eq!(rt.get_field(e, "$age").i(), -21600);
            assert_eq!(rt.get_field(e, "inLove").i(), 0, "a baby doesn't fall in love");
        });
        assert_eq!(host.held, Some(("minecraft:cod".to_owned(), 1)));
        // The window's client predicts a grown one's: the click is used there, the server decides.
        let (mut client, ty) = runtime();
        client.interned.insert("side:client".into(), V::I(1));
        let mut host = world(Some(("minecraft:cod", 2)));
        client.with_host(&mut host, |rt| {
            let (e, id) = penguin(rt, ty, [0.5, 1.0, 1.5]);
            assert_eq!(interact(rt, None, id, 0), Some(Answer::Consume));
            assert_eq!(rt.get_field(e, "inLove").i(), 0, "love is the server's");
        });
        assert_eq!(host.held, Some(("minecraft:cod".to_owned(), 2)));
    }

    #[test]
    fn a_players_blow_hurts_it_and_its_knockback_pushes_it_away_from_the_player() {
        let (mut rt, ty) = runtime();
        let mut host = world(None);
        rt.with_host(&mut host, |rt| {
            let (e, id) = penguin(rt, ty, [0.5, 1.0, 1.5]);
            // Looking south (yaw 0): `(sin 0, -cos 0)`.
            let blow = Blow { amount: 5.0, damage_type: "minecraft:player_attack".into(), knockback: 0.5, direction: [0.0, -1.0] };
            let struck = attack(rt, LEVEL, id, &blow).expect("one of the mod's");
            assert!(struck.attackable && struck.landed);
            assert_eq!(struck.health, Some([20.0, 15.0]));
            assert!(struck.alive);
            assert_eq!(struck.size, [0.6, 1.0]);
            let m = crate::mobs::motion(rt, e);
            assert!(m[2] > 0.4, "pushed south, away from the player: {m:?}");
            let source = rt.get_field(e, "$lastDamageSource");
            let by = api::call(rt, "net/minecraft/world/damagesource/DamageSource.getEntity.()Lnet/minecraft/world/entity/Entity;", &[source]);
            assert!(crate::api_common::is_player_class(rt.class_of(by)), "the player's blow");
            let again = attack(rt, LEVEL, id, &Blow { knockback: 0.0, ..blow }).expect("still there");
            assert!(!again.landed, "within its hurt frames, no harder");
            assert_eq!(again.health, Some([15.0, 15.0]));
            assert_eq!(attack(rt, "minecraft:the_nether", id + 1000, &Blow { amount: 1.0, damage_type: "minecraft:player_attack".into(), knockback: 0.0, direction: [0.0, -1.0] }), None);
        });
    }

    #[test]
    fn the_view_meets_the_nearest_pickable_one_at_its_box() {
        let (mut rt, ty) = runtime();
        let mut host = world(None);
        rt.with_host(&mut host, |rt| {
            let (_, near) = penguin(rt, ty, [0.5, 1.0, 3.5]);
            let (far_e, _) = penguin(rt, ty, [0.5, 1.0, 5.5]);
            let hit = aimed(rt, [0.5, 1.5, 0.5], [0.0, 0.0, 2.0], 6.0).expect("in view");
            assert_eq!(hit.id, near);
            // Its half width is the float 0.3.
            assert!((hit.distance - 2.7).abs() < 1e-6, "{}", hit.distance);
            assert!((hit.location[2] - 3.2).abs() < 1e-6);
            assert_eq!(aimed(rt, [0.5, 1.5, 0.5], [0.0, 0.0, -1.0], 6.0), None, "behind");
            assert_eq!(aimed(rt, [0.5, 1.5, 0.5], [0.0, 0.0, 1.0], 2.0), None, "out of reach");
            let _ = far_e;
        });
    }

    #[test]
    fn a_ray_meets_a_box_at_its_near_face_and_from_inside_at_once() {
        let b = [1.0, 0.0, -0.5, 2.0, 1.0, 0.5];
        assert_eq!(clip([0.0, 0.5, 0.0], [1.0, 0.0, 0.0], 5.0, b), Some(1.0));
        assert_eq!(clip([0.0, 0.5, 0.0], [-1.0, 0.0, 0.0], 5.0, b), None, "behind");
        assert_eq!(clip([0.0, 0.5, 0.0], [1.0, 0.0, 0.0], 0.5, b), None, "out of reach");
        assert_eq!(clip([0.0, 2.0, 0.0], [1.0, 0.0, 0.0], 5.0, b), None, "above it");
        assert_eq!(clip([1.5, 0.5, 0.0], [0.0, 0.0, 1.0], 5.0, b), Some(0.0), "inside");
    }

    #[test]
    fn interaction_results_answer_by_their_names() {
        let mut rt = Rt::new();
        for (name, want) in [("SUCCESS", Answer::Success), ("CONSUME", Answer::Consume), ("FAIL", Answer::Fail), ("PASS", Answer::Pass)] {
            let r = crate::api_common::result(&mut rt, INTERACTION_RESULT, name);
            assert_eq!(answer(&rt, r), want, "{name}");
        }
    }
}
