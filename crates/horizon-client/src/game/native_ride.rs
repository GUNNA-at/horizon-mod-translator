//! A server's players on the mods' own entities (docs/native-mods.md, "Players"): a mod's seat
//! (Create's) takes a player as its passenger (`Entity.startRiding`), places them each tick
//! (`positionRider`) and lets them off where it says (`getDismountLocationForPassenger`). The
//! player's state changes as that player (`native_as_player`); the players in the level are told
//! who rides the mod's entity (`ClientboundSetPassengersPacket`), which the game's tracker, which
//! doesn't hold the mod's entities, can't tell them. A vehicle of the game's (a boat, a minecart, a
//! horse) the player leaves by the game's own code.
use super::{Game, lan::LanHost, lan_vehicles::input_keys};
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::play::{self, Clientbound as C};

/// What a native mod does with a server's player and a vehicle.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NativeRide {
    /// On the mod's entity `vehicle` (`Entity.startRiding`), in its seat at `at`: the player's
    /// feet, where its `positionRider` puts them.
    Board { vehicle: i32, at: [f64; 3] },
    /// In the seat of the mod's entity they ride, now at `at` (`positionRider`, each tick).
    Sit([f64; 3]),
    /// Off the mod's entity they ride, at `to` (`ServerPlayer.dismountTo`): their client hears
    /// they are off, then is teleported there.
    Leave { to: [f64; 3] },
    /// Off a vehicle of the game's, where the game lets its riders off (`LivingEntity.stopRiding`).
    Dismount,
}

/// A mod's entity the server's players ride: its level, and who its level was last told rides it.
#[derive(Debug, Default)]
pub(super) struct ModVehicle {
    dimension: String,
    sent: Vec<i32>,
}

impl Game<'_> {
    /// Whether `uuid` is the acting player, whose state is in place.
    fn native_ride_acting(&mut self, uuid: u128) -> bool {
        (self.lan_acting.is_some() || self.player.is_some()) && uuid == self.native_acting_id()
    }
    /// Puts the server's player `uuid` at `at` in their seat, as `guest_vehicle_tick` does for a
    /// vehicle of the game's: false for no such player here, dead, or riding nothing.
    fn native_seat_guest(&mut self, uuid: u128, at: [f64; 3]) -> bool {
        let Some(g) = self.lan.as_mut().and_then(|h| {
            h.hub
                .guests
                .iter_mut()
                .find(|g| g.placed && !g.dead && g.uuid == uuid)
        }) else {
            return false;
        };
        if g.vehicle.is_none() {
            return false;
        }
        g.previous_position = g.position;
        g.position = at;
        if let Some(m) = g.motion.as_mut() {
            m.position = Vec3::new(at[0], at[1], at[2]);
            m.velocity = Vec3::ZERO;
            m.fall_distance = 0.0;
        }
        true
    }

    /// `ServerEntity.sendChanges`' passengers, for the mods' entities in the level in place: when
    /// the players riding one change, the players in the level hear who rides it now. One nobody
    /// rides any more is let go once they have heard.
    pub(super) fn lan_mod_passengers(&mut self, host: &mut LanHost) {
        if self.mod_vehicles.is_empty() {
            return;
        }
        let dimension = self.native_dimension();
        let hub = &host.hub;
        self.mod_vehicles.retain(|&id, v| {
            if v.dimension != dimension {
                return true;
            }
            let riders: Vec<i32> = hub
                .guests
                .iter()
                .filter(|g| hub.here(g) && g.vehicle == Some(id))
                .map(|g| g.entity_id)
                .collect();
            if riders != v.sent {
                hub.broadcast_level(&C::SetPassengers(play::SetPassengers {
                    vehicle: id as i32,
                    passengers: riders.clone(),
                }));
                v.sent = riders;
            }
            !v.sent.is_empty()
        });
    }
    /// Whether `vehicle` is a mod's entity a server's player rides: it seats them itself.
    pub(super) fn mod_vehicle(&self, vehicle: u32) -> bool {
        self.mod_vehicles.contains_key(&vehicle)
    }
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// What the player `uuid` rides as the server holds them (`Entity.getVehicle`), a vehicle of
    /// the game's or a mod's entity, also inside a player's hook for the acting player: None for
    /// nothing, or no such player here. API @API:mod_riding@.
    pub(super) fn native_player_vehicle(&mut self, uuid: u128) -> Option<i32> {
        if self.native_ride_acting(uuid) {
            return self.riding.map(|v| v as i32);
        }
        let host = self.lan.as_ref()?;
        let g = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && g.uuid == uuid)?;
        g.vehicle.map(|v| v as i32)
    }
    /// Whether the player `uuid` sneaks, which gets a rider off (`Player.wantsToStopRiding`: the
    /// shift key their client last sent). False for no such player here. API @API:mod_riding@.
    pub(super) fn native_player_wants_off(&mut self, uuid: u128) -> bool {
        if self.native_ride_acting(uuid) {
            return self.player.as_ref().is_some_and(|p| p.shift_key_down);
        }
        self.lan
            .as_ref()
            .and_then(|h| h.hub.guests.iter().find(|g| g.placed && g.uuid == uuid))
            .is_some_and(|g| input_keys(g.input).shift)
    }
    /// Puts the player `uuid` on, in or off a vehicle as `ride` says, as the server holds them.
    /// `Board` seats them on a mod's entity: the players in the level are told at the end of the
    /// tick, and the rider's client seats them itself from then on. `Sit` keeps a rider in the
    /// seat; `Leave` lets them off a mod's entity where it says, their client teleported there;
    /// `Dismount` lets them off a vehicle of the game's. False for no such player here, or dead,
    /// and for `Sit` and `Dismount` one riding nothing of the kind. API @API:mod_riding@.
    pub(super) fn native_ride_player(&mut self, uuid: u128, ride: NativeRide) -> bool {
        match ride {
            NativeRide::Board { vehicle, at } => {
                let Ok(id) = u32::try_from(vehicle) else {
                    return false;
                };
                let dimension = self.native_dimension().to_owned();
                let boarded = self.native_as_player(uuid, |game| {
                    game.riding = Some(id);
                    if let Some(p) = game.player.as_mut() {
                        p.position = Vec3::new(at[0], at[1], at[2]);
                        p.velocity = Vec3::ZERO;
                        p.fall_distance = 0.0;
                        p.is_passenger = true;
                    }
                });
                if boarded {
                    self.native_seat_guest(uuid, at);
                    self.mod_vehicles.entry(id).or_insert_with(|| ModVehicle {
                        dimension,
                        sent: Vec::new(),
                    });
                }
                boarded
            }
            NativeRide::Sit(at) => {
                if !self.native_ride_acting(uuid) {
                    return self.native_seat_guest(uuid, at);
                }
                if self.riding.is_none() {
                    return false;
                }
                let Some(p) = self.player.as_mut() else {
                    return false;
                };
                p.position = Vec3::new(at[0], at[1], at[2]);
                p.velocity = Vec3::ZERO;
                p.fall_distance = 0.0;
                true
            }
            // `as_guest` tells the client it is off its vehicle, then teleports it.
            NativeRide::Leave { to } => self.native_as_player(uuid, |game| {
                game.riding = None;
                if let Some(p) = game.player.as_mut() {
                    p.position = Vec3::new(to[0], to[1], to[2]);
                    p.velocity = Vec3::ZERO;
                    p.is_passenger = false;
                }
            }),
            NativeRide::Dismount => {
                let mut off = false;
                self.native_as_player(uuid, |game| {
                    if game.riding.is_some_and(|v| !game.mod_vehicle(v)) {
                        game.stop_riding();
                        off = true;
                    }
                });
                off
            }
        }
    }
    /// What the window's own player rides as their client was told (`LocalPlayer.getVehicle`): a
    /// vehicle of the game's or a mod's entity; None for nothing, or no player. API
    /// @API:mod_riding@.
    pub(super) fn native_client_player_vehicle(&self) -> Option<i32> {
        self.player.as_ref()?;
        self.riding.map(|v| v as i32)
    }
}
