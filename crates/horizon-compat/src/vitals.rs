//! The player's health and food, as a mod reads and changes them: `LivingEntity`'s `getHealth`,
//! `getMaxHealth`, `getAbsorptionAmount`, `isHurt`, `isDeadOrDying`, `setHealth`, `heal` and
//! `setAbsorptionAmount` on the player in place, and the `FoodData` `Player.getFoodData` gives
//! (the level, saturation and exhaustion, and `eat`). They are the game's own player's
//! (`Host::player_vitals`): Farmer's Delight's Comfort heals them, its Nourishment keeps their food
//! from draining. A mob's health is its stand-in's (`vanilla_entities`).
use crate::host::VitalsView;
use crate::{Rt, V};

const FOOD_DATA: &str = "net/minecraft/world/food/FoodData";

/// The keys [`call`] may answer, by owner, name and descriptor alone: the living methods on any
/// class (the owner names whichever the mod's code did), `FoodData`'s on its own.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    living_method(name, desc) || (owner == FOOD_DATA && food_method(name, desc))
}

fn living_method(name: &str, desc: &str) -> bool {
    matches!(
        (name, desc),
        ("getHealth" | "getMaxHealth" | "getAbsorptionAmount", "()F") | ("isHurt" | "isDeadOrDying", "()Z") | ("setHealth" | "heal" | "setAbsorptionAmount", "(F)V")
    )
}

fn food_method(name: &str, desc: &str) -> bool {
    matches!(
        (name, desc),
        ("getFoodLevel", "()I")
            | ("needsFood", "()Z")
            | ("getSaturationLevel" | "getExhaustionLevel", "()F")
            | ("setFoodLevel", "(I)V")
            | ("setSaturation" | "setExhaustion" | "addExhaustion", "(F)V")
            | ("eat", "(IF)V" | "(Lnet/minecraft/world/food/FoodProperties;)V")
    )
}

/// The player's health and food methods; None for any other key, and for a living entity that
/// isn't the player in place (a mob's stand-in answers its own).
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == FOOD_DATA && food_method(name, desc) {
        return Some(food(rt, name, desc, a(1), a(2)));
    }
    if !living_method(name, desc) || crate::attributes::entity_of(rt, a(0)) != Ok(None) {
        return None;
    }
    let mut v = vitals(rt);
    match name {
        "getHealth" => return Some(V::F(v.health)),
        "getMaxHealth" => return Some(V::F(v.max_health)),
        "getAbsorptionAmount" => return Some(V::F(v.absorption)),
        "isHurt" => return Some(V::I(i32::from(v.health > 0.0 && v.health < v.max_health))),
        "isDeadOrDying" => return Some(V::I(i32::from(v.health <= 0.0))),
        // The game keeps both in their range, as vanilla's setters clamp them.
        "setHealth" => v.health = a(1).f(),
        "setAbsorptionAmount" => v.absorption = a(1).f(),
        // NeoForge's `heal`: not by a negative amount, nor a dead player.
        _ if a(1).f() > 0.0 && v.health > 0.0 => v.health += a(1).f(),
        _ => return Some(V::Void),
    }
    set(rt, &v);
    Some(V::Void)
}

/// `FoodData`'s method `name` on the player in place's food.
fn food(rt: &mut Rt, name: &str, desc: &str, a1: V, a2: V) -> V {
    // The game's exhaustion, as `Player.causeFoodExhaustion`'s: none in creative.
    if name == "addExhaustion" {
        if let Some(h) = rt.host_mut() {
            h.exhaust_player(a1.f());
        }
        return V::Void;
    }
    let mut v = vitals(rt);
    match name {
        "getFoodLevel" => return V::I(v.food),
        "needsFood" => return V::I(i32::from(v.food < 20)),
        "getSaturationLevel" => return V::F(v.saturation),
        "getExhaustionLevel" => return V::F(v.exhaustion),
        "setFoodLevel" => v.food = a1.i(),
        "setSaturation" => v.saturation = a1.f(),
        "setExhaustion" => v.exhaustion = a1.f(),
        // `eat(nutrition, saturationModifier)` or `eat(FoodProperties)`: `FoodData.add`.
        _ => {
            let (nutrition, saturation) = if desc == "(IF)V" {
                (a1.i(), a1.i() as f32 * a2.f() * 2.0)
            } else {
                (rt.get_field(a1, "nutrition").i(), rt.get_field(a1, "saturation").f())
            };
            v.food = (v.food + nutrition).clamp(0, 20);
            v.saturation = (v.saturation + saturation).clamp(0.0, v.food as f32);
        }
    }
    set(rt, &v);
    V::Void
}

/// The player in place's health and food, as the host tells them; else a new player's (full
/// health, the host's food level).
fn vitals(rt: &mut Rt) -> VitalsView {
    let Some(h) = rt.host_mut() else {
        return VitalsView::default();
    };
    h.player_vitals().unwrap_or_else(|| VitalsView { food: h.player_food(), ..VitalsView::default() })
}

fn set(rt: &mut Rt, v: &VitalsView) {
    if let Some(h) = rt.host_mut() {
        h.set_player_vitals(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
    const PLAYER: &str = "net/minecraft/world/entity/player/Player";

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        format!("{owner}.{name}.{desc}").leak()
    }

    #[test]
    fn a_mod_heals_and_feeds_the_games_player() {
        let hurt = VitalsView { health: 12.0, max_health: 20.0, absorption: 0.0, food: 15, saturation: 0.0, exhaustion: 2.5 };
        let mut h = MemoryHost { player_id: Some(0xabc), vitals: Some(hurt), ..MemoryHost::default() };
        h.entities.push(crate::host::EntityView { id: 7, kind: "minecraft:cow".into(), health: Some((10.0, 10.0)), ..Default::default() });
        let mut rt = Rt::new();
        crate::install();
        rt.with_host(&mut h, |rt| {
            let player = crate::menus::server_player(rt, 0xabc);
            let cow = crate::vanilla_entities::of(rt, 7, "minecraft:cow", None);
            // Farmer's Delight's Comfort: a hurt player with no saturation heals by one.
            assert_eq!(crate::api::call(rt, key(LIVING, "getHealth", "()F"), &[player]), V::F(12.0));
            assert_eq!(crate::api::call(rt, key(LIVING, "getMaxHealth", "()F"), &[player]), V::F(20.0));
            assert_eq!(crate::api::call(rt, key(PLAYER, "isHurt", "()Z"), &[player]), V::I(1));
            crate::api::call(rt, key(LIVING, "heal", "(F)V"), &[player, V::F(1.0)]);
            assert_eq!(crate::api::call(rt, key(LIVING, "getHealth", "()F"), &[player]), V::F(13.0));
            crate::api::call(rt, key(LIVING, "heal", "(F)V"), &[player, V::F(-5.0)]);
            assert_eq!(crate::api::call(rt, key(LIVING, "getHealth", "()F"), &[player]), V::F(13.0), "no healing by a negative amount");

            // Its Nourishment: the food stops draining.
            let food = crate::api::call(rt, key(PLAYER, "getFoodData", "()Lnet/minecraft/world/food/FoodData;"), &[player]);
            assert_eq!(crate::api::call(rt, key(FOOD_DATA, "getSaturationLevel", "()F"), &[food]), V::F(0.0));
            crate::api::call(rt, key(FOOD_DATA, "setExhaustion", "(F)V"), &[food, V::F(0.0)]);
            assert_eq!(crate::api::call(rt, key(FOOD_DATA, "getExhaustionLevel", "()F"), &[food]), V::F(0.0));
            // A bite of 2 at a modifier of 5 gives 20 saturation, which the food level, 17, caps;
            // then the food level stops at 20.
            crate::api::call(rt, key(FOOD_DATA, "eat", "(IF)V"), &[food, V::I(2), V::F(5.0)]);
            assert_eq!(crate::api::call(rt, key(FOOD_DATA, "getFoodLevel", "()I"), &[food]), V::I(17));
            assert_eq!(crate::api::call(rt, key(FOOD_DATA, "getSaturationLevel", "()F"), &[food]), V::F(17.0));
            crate::api::call(rt, key(FOOD_DATA, "eat", "(IF)V"), &[food, V::I(6), V::F(0.25)]);
            assert_eq!(crate::api::call(rt, key(FOOD_DATA, "getFoodLevel", "()I"), &[food]), V::I(20));
            assert_eq!(crate::api::call(rt, key(FOOD_DATA, "needsFood", "()Z"), &[food]), V::I(0));

            // The cow's health is its own.
            assert_eq!(crate::api::call(rt, key(LIVING, "getHealth", "()F"), &[cow]), V::F(10.0));
            crate::api::call(rt, key(LIVING, "setHealth", "(F)V"), &[player, V::F(0.0)]);
            assert_eq!(crate::api::call(rt, key(LIVING, "isDeadOrDying", "()Z"), &[player]), V::I(1));
        });
        let v = h.vitals.unwrap();
        assert_eq!((v.health, v.food, v.saturation, v.exhaustion), (0.0, 20, 20.0, 0.0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn with_no_player_to_read_a_player_is_a_new_ones() {
        let mut rt = Rt::new();
        crate::install();
        let player = rt.new_object("net/minecraft/server/level/ServerPlayer");
        assert_eq!(crate::api::call(&mut rt, key(LIVING, "getHealth", "()F"), &[player]), V::F(20.0));
        assert_eq!(crate::api::call(&mut rt, key(PLAYER, "isHurt", "()Z"), &[player]), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
