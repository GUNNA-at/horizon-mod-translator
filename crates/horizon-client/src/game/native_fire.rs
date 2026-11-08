//! Fire on mobs and players for native mods (docs/native-mods.md, "Mobs"): how long one burns, and
//! setting it alight or putting it out as vanilla's `Entity` does. A mob's act where the world is
//! simulated, on the level in place, as `native_mobs`' do; a player's as that player
//! (`native_as_player`). The players who see one are sent whether it burns, as its shared flags.
use super::Game;

/// A mob's or a player's fire as a native mod reads it.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NativeFire {
    /// Its remaining fire ticks (`Entity.getRemainingFireTicks`): it burns while they are above 0.
    pub(crate) ticks: i32,
    /// Whether its kind never burns (`Entity.fireImmune`): a blaze, a strider.
    pub(crate) immune: bool,
}

/// What a native mod does to a mob's or a player's fire.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeFireChange {
    /// `Entity.setRemainingFireTicks(ticks)`: a player who can't be hurt keeps at most 1.
    Set(i32),
    /// `Entity.igniteForTicks(ticks)`: alight at least that long, the ticks scaled by its burning
    /// time (`LivingEntity.igniteForTicks`: the attribute, which fire protection lowers), and no
    /// longer freezing.
    Ignite(i32),
    /// `Entity.extinguishFire()`: one that burns plays the extinguish sound, then its fire is out
    /// (`clearFire`, which keeps a fire-immune countdown).
    Extinguish,
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Mob `id`'s fire in the level in place, `dimension`: None for no such mob, another dimension
    /// or a client's copy. API @API:entity_fire@.
    pub(super) fn native_mob_fire(&self, dimension: &str, id: u32) -> Option<NativeFire> {
        if !self.native_mob_level(dimension) {
            return None;
        }
        let m = self.mobs.iter().find(|m| m.id == id && !m.removed)?;
        Some(NativeFire {
            ticks: m.fire_ticks,
            immune: m.kind.flags.fire_immune,
        })
    }
    /// Changes mob `id`'s fire as `change` says, in the level in place, `dimension`: the players
    /// who see it are sent whether it burns, and an extinguished one's sound plays at the end of
    /// the mobs' tick. False for no such mob, another dimension or a client's copy. API
    /// @API:entity_fire@.
    pub(super) fn native_set_mob_fire(
        &mut self,
        dimension: &str,
        id: u32,
        change: NativeFireChange,
    ) -> bool {
        if !self.native_mob_level(dimension) {
            return false;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) else {
            return false;
        };
        match change {
            NativeFireChange::Set(ticks) => m.fire_ticks = ticks,
            NativeFireChange::Ignite(ticks) => m.ignite_for_ticks(ticks),
            NativeFireChange::Extinguish => m.extinguish_fire(),
        }
        true
    }
    /// The player `uuid`'s fire as the server holds it, also inside a player's hook for the acting
    /// player: None for no such player here, or dead. A player never is fire-immune. API
    /// @API:entity_fire@.
    pub(super) fn native_player_fire(&mut self, uuid: u128) -> Option<NativeFire> {
        let mut fire = None;
        self.native_as_player(uuid, |game| {
            fire = game.player.as_ref().map(|p| NativeFire {
                ticks: p.fire_ticks,
                immune: false,
            });
        });
        fire
    }
    /// Changes the player `uuid`'s fire as `change` says, as the server holds it: their client
    /// shows the flames, and the players who see them are sent whether they burn; the extinguish
    /// sound plays for everyone near. False for no such player here, or dead. API
    /// @API:entity_fire@.
    pub(super) fn native_set_player_fire(&mut self, uuid: u128, change: NativeFireChange) -> bool {
        let mut changed = false;
        self.native_as_player(uuid, |game| {
            let Some(p) = game.player.as_mut() else {
                return;
            };
            match change {
                NativeFireChange::Set(ticks) => p.set_remaining_fire_ticks(ticks),
                NativeFireChange::Ignite(ticks) => {
                    if let Err(e) = p.ignite_for_ticks(ticks) {
                        log::warn!("Horizon: a mod setting a player alight: {e}");
                        return;
                    }
                }
                NativeFireChange::Extinguish => {
                    let burned = p.fire_ticks > 0;
                    let feet = [p.position.x, p.position.y, p.position.z];
                    let out = p.fire_ticks.min(0);
                    p.set_remaining_fire_ticks(out);
                    if burned {
                        game.extinguish_sound(feet);
                    }
                }
            }
            changed = true;
        });
        changed
    }
    /// The window's own player's fire as their client shows it (`LocalPlayer.isOnFire`): on a
    /// server's client 1 tick while the server says they burn, as a client's own fire ticks are
    /// cleared each tick. None with no player. API @API:entity_fire@.
    pub(super) fn native_client_player_fire(&self) -> Option<NativeFire> {
        let p = self.player.as_ref()?;
        let ticks = match &self.remote {
            Some(_) => i32::from(self.own_on_fire()),
            None => p.fire_ticks,
        };
        Some(NativeFire {
            ticks,
            immune: false,
        })
    }
}
