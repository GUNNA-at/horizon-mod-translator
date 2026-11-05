//! The player's health and food for native server mods (docs/native-mods.md, "Vitals"): what a mod
//! reads of the player in place, and sets as vanilla's `LivingEntity.setHealth`,
//! `setAbsorptionAmount` and `FoodData`'s setters do. Their client hears of it as of any other
//! change to their health and food.
use super::{Game, native_attributes::Living};
use horizon_world::attribute_map::ValueKind;

/// The player in place's health and food: vanilla's `LivingEntity` health and absorption, and the
/// player's `FoodData`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeVitals {
    pub(crate) health: f32,
    /// The `max_health` attribute's value. Setting the vitals doesn't change it: the attribute does
    /// (`native_change_attribute`).
    pub(crate) max_health: f32,
    pub(crate) absorption: f32,
    /// The food level (20 is full), its saturation and exhaustion.
    pub(crate) food: i32,
    pub(crate) saturation: f32,
    pub(crate) exhaustion: f32,
}

impl Game<'_> {
    /// The health and food of the player in place: the acting player on a server, the client's
    /// own on a client. None with no player in place. API @API:vitals@.
    #[allow(dead_code)]
    pub(crate) fn native_player_vitals(&mut self) -> Option<NativeVitals> {
        let Living::Player = self.living(None)? else {
            return None;
        };
        let p = self.player.as_mut()?;
        let key = p.attributes.key("minecraft:max_health")?;
        let max_health = p.attributes.value(Some(key), ValueKind::Computed).ok()? as f32;
        let food = &p.health.food;
        Some(NativeVitals {
            health: p.health.health(),
            max_health,
            absorption: p.health.absorption(),
            food: food.food,
            saturation: food.saturation,
            exhaustion: food.exhaustion,
        })
    }

    /// Sets the player in place's health, absorption and food to `to`'s, as vanilla's setters do:
    /// the health between 0 and the maximum, the absorption between 0 and the `max_absorption`
    /// attribute, the food as given (`FoodData`'s setters keep no range). What is the same stays
    /// untouched. Whether there was a player to change: never on a client, whose player's health
    /// and food are the server's. API @API:vitals@.
    #[allow(dead_code)]
    pub(crate) fn native_set_player_vitals(&mut self, to: &NativeVitals) -> bool {
        if self.remote.is_some() || self.living(None).is_none() {
            return false;
        }
        let Some(p) = self.player.as_mut() else {
            return false;
        };
        if to.health != p.health.health() {
            let _ = p.set_health(to.health, false);
        }
        if to.absorption != p.health.absorption() {
            let _ = p.set_health(to.absorption, true);
        }
        let food = &mut p.health.food;
        food.food = to.food;
        food.saturation = to.saturation;
        food.exhaustion = to.exhaustion;
        true
    }
}

