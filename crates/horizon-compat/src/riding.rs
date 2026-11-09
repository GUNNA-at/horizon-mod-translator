//! A server's player on an entity a mod made, as Create's seats put them there (`SeatBlock.sitDown`
//! makes a `SeatEntity` where the seat is and puts the player on it with `startRiding`).
//!
//! - The mod's entity keeps its passengers as vanilla's `Entity.passengers` (`$passengers`):
//!   `getPassengers`, `isVehicle`, `hasPassenger`, `addPassenger`, `removePassenger`,
//!   `ejectPassengers`, `positionRider` and `getDismountLocationForPassenger` answer from it, the
//!   mod's own overrides first.
//! - A server's player's `startRiding`, `stopRiding`, `isPassenger` and `getVehicle` reach the game,
//!   which seats them ([`Host::ride_player`](crate::host::Host::ride_player)).
//! - Each tick, after the mod's entity's own tick, it places its riders (`positionRider`) and lets
//!   off one who sneaks, where its `getDismountLocationForPassenger` says (`Player.rideTick`).
//! - A client's copy of the entity seats the riders its client draws, its own player included
//!   ([`client_seat`]).
use crate::host::Ride;
use crate::{Data, Rt, V, api, jutil};

pub(crate) const MOVE_FUNCTION: &str = "net/minecraft/world/entity/Entity$MoveFunction";
/// The `MoveFunction` a seat is read through: the place `accept` was given for the rider.
const SEAT: &str = "compat:ride/seat";
const LOCAL_PLAYER: &str = "net/minecraft/client/player/LocalPlayer";
const ACCEPT: &str = "accept.(Lnet/minecraft/world/entity/Entity;DDD)V";
const ACCEPT_KEY: &str = "net/minecraft/world/entity/Entity$MoveFunction.accept.(Lnet/minecraft/world/entity/Entity;DDD)V";
const POSITION_RIDER: &str = "positionRider.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/Entity$MoveFunction;)V";
const POSITION_RIDER_KEY: &str = "net/minecraft/world/entity/Entity.positionRider.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/Entity$MoveFunction;)V";
const ADD_PASSENGER: &str = "addPassenger.(Lnet/minecraft/world/entity/Entity;)V";
const ADD_PASSENGER_KEY: &str =
    "net/minecraft/world/entity/Entity.addPassenger.(Lnet/minecraft/world/entity/Entity;)V";
const REMOVE_PASSENGER: &str = "removePassenger.(Lnet/minecraft/world/entity/Entity;)V";
const REMOVE_PASSENGER_KEY: &str =
    "net/minecraft/world/entity/Entity.removePassenger.(Lnet/minecraft/world/entity/Entity;)V";
const CAN_ADD_PASSENGER: &str = "canAddPassenger.(Lnet/minecraft/world/entity/Entity;)Z";
const CAN_ADD_PASSENGER_KEY: &str =
    "net/minecraft/world/entity/Entity.canAddPassenger.(Lnet/minecraft/world/entity/Entity;)Z";
const RIDING_POSITION: &str = "getPassengerRidingPosition.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/phys/Vec3;";
const RIDING_POSITION_KEY: &str = "net/minecraft/world/entity/Entity.getPassengerRidingPosition.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/phys/Vec3;";
const ATTACHMENT: &str = "getPassengerAttachmentPoint.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/EntityDimensions;F)Lnet/minecraft/world/phys/Vec3;";
const ATTACHMENT_KEY: &str = "net/minecraft/world/entity/Entity.getPassengerAttachmentPoint.(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/EntityDimensions;F)Lnet/minecraft/world/phys/Vec3;";
const DISMOUNT: &str = "getDismountLocationForPassenger.(Lnet/minecraft/world/entity/LivingEntity;)Lnet/minecraft/world/phys/Vec3;";
const DISMOUNT_KEY: &str = "net/minecraft/world/entity/Entity.getDismountLocationForPassenger.(Lnet/minecraft/world/entity/LivingEntity;)Lnet/minecraft/world/phys/Vec3;";
const VEHICLE_ATTACHMENT: &str = "getVehicleAttachmentPoint.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/phys/Vec3;";
const VEHICLE_ATTACHMENT_KEY: &str = "net/minecraft/world/entity/Entity.getVehicleAttachmentPoint.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/phys/Vec3;";
const GET_POSE: &str = "getPose.()Lnet/minecraft/world/entity/Pose;";
const GET_POSE_KEY: &str =
    "net/minecraft/world/entity/Entity.getPose.()Lnet/minecraft/world/entity/Pose;";
const GET_DIMENSIONS: &str = "getDimensions.(Lnet/minecraft/world/entity/Pose;)Lnet/minecraft/world/entity/EntityDimensions;";
const GET_DIMENSIONS_KEY: &str = "net/minecraft/world/entity/Entity.getDimensions.(Lnet/minecraft/world/entity/Pose;)Lnet/minecraft/world/entity/EntityDimensions;";
const BOUNDING_BOX: &str = "getBoundingBox.()Lnet/minecraft/world/phys/AABB;";
const BOUNDING_BOX_KEY: &str =
    "net/minecraft/world/entity/Entity.getBoundingBox.()Lnet/minecraft/world/phys/AABB;";
const TEST: &str = "test.(Ljava/lang/Object;)Z";
/// `Player.DEFAULT_VEHICLE_ATTACHMENT`: where a standing player meets what they ride.
const PLAYER_VEHICLE_ATTACHMENT: [f64; 3] = [0.0, 0.6, 0.0];
/// `Entity.removePassenger`'s `boardingCooldown`: a rider let off boards nothing on its own for
/// three seconds.
const BOARDING_COOLDOWN: i64 = 60;

/// The mod's entity `e`'s passengers (`Entity.passengers`), in the order they got on.
pub(crate) fn passengers(rt: &Rt, e: V) -> Vec<V> {
    match rt.get_field(e, "$passengers") {
        l if l.r() != 0 => jutil::elements(rt, l),
        _ => Vec::new(),
    }
}

fn set_passengers(rt: &mut Rt, e: V, items: Vec<V>) {
    let l = jutil::new_list(rt, items);
    rt.put_field(e, "$passengers", l);
}

/// Whether `a` and `b` are one entity: one object, or one player's two (a hook's player and the
/// level's).
fn same(rt: &Rt, a: V, b: V) -> bool {
    a == b
        || matches!((crate::network::player_uuid(rt, a), crate::network::player_uuid(rt, b)), (Some(x), Some(y)) if x == y)
}

fn holds(rt: &Rt, e: V, p: V) -> bool {
    passengers(rt, e).into_iter().any(|q| same(rt, q, p))
}

fn client(rt: &Rt) -> bool {
    rt.host().is_some_and(|h| h.client_player_id().is_some())
}

/// The UUID of the server's player `p`: None on a client, for a client's own player and for a
/// mod's fake player.
fn server_player(rt: &Rt, p: V) -> Option<u128> {
    let class = rt.class_of(p);
    if p.r() == 0
        || !crate::api_common::is_player_class(class)
        || class == LOCAL_PLAYER
        || class == crate::fake_players::FAKE_PLAYER
        || client(rt)
    {
        return None;
    }
    crate::network::player_uuid(rt, p).or_else(|| rt.host().and_then(|h| h.player_id()))
}

fn of_vec3(rt: &Rt, v: V) -> [f64; 3] {
    if v.r() == 0 {
        return [0.0; 3];
    }
    [
        rt.get_field(v, "x").d(),
        rt.get_field(v, "y").d(),
        rt.get_field(v, "z").d(),
    ]
}

fn at(rt: &Rt, e: V) -> [f64; 3] {
    [
        rt.get_field(e, "$x").d(),
        rt.get_field(e, "$y").d(),
        rt.get_field(e, "$z").d(),
    ]
}

fn game_time(rt: &Rt) -> i64 {
    rt.host().map_or(0, |h| h.game_time())
}

/// `Entity.getPassengerRidingPosition(passenger)`: the mod's own, else where it stands plus its
/// passenger attachment point (`getPassengerAttachmentPoint`, the mod's own, else vanilla's
/// default for an entity type that names none: `EntityAttachment.PASSENGER` at its height,
/// straight up, which its turn leaves as it is).
fn riding_position(rt: &mut Rt, e: V, p: V) -> [f64; 3] {
    if let Some(v) =
        crate::mod_entities::virtual_call(rt, e, RIDING_POSITION_KEY, RIDING_POSITION, &[p])
    {
        return of_vec3(rt, v);
    }
    default_riding_position(rt, e, p)
}

fn default_riding_position(rt: &mut Rt, e: V, p: V) -> [f64; 3] {
    let base = at(rt, e);
    let pose = api::virtual_call(rt, (GET_POSE_KEY, GET_POSE), &[e]);
    let dimensions = api::virtual_call(rt, (GET_DIMENSIONS_KEY, GET_DIMENSIONS), &[e, pose]);
    let attach = match crate::mod_entities::virtual_call(
        rt,
        e,
        ATTACHMENT_KEY,
        ATTACHMENT,
        &[p, dimensions, V::F(1.0)],
    ) {
        Some(v) => of_vec3(rt, v),
        None => default_attachment(rt, e),
    };
    [
        base[0] + attach[0],
        base[1] + attach[1],
        base[2] + attach[2],
    ]
}

fn default_attachment(rt: &mut Rt, e: V) -> [f64; 3] {
    [0.0, f64::from(crate::mod_entities::own_size(rt, e)[1]), 0.0]
}

/// Where rider `p` meets the vehicle `e` (`Entity.getVehicleAttachmentPoint`): a player's
/// `DEFAULT_VEHICLE_ATTACHMENT`, another entity's own answer.
fn vehicle_attachment(rt: &mut Rt, p: V, e: V) -> [f64; 3] {
    if crate::api_common::is_player_class(rt.class_of(p)) {
        return PLAYER_VEHICLE_ATTACHMENT;
    }
    let v = api::virtual_call(rt, (VEHICLE_ATTACHMENT_KEY, VEHICLE_ATTACHMENT), &[p, e]);
    of_vec3(rt, v)
}

/// Where the mod's entity `e` puts its passenger `p` (`positionRider(p)`, final: only a passenger
/// of it, through `positionRider(p, callback)`, the mod's own, else vanilla's: its riding position
/// less the passenger's vehicle attachment point). None where it put them nowhere.
pub(crate) fn seat_of(rt: &mut Rt, e: V, p: V) -> Option<[f64; 3]> {
    if !holds(rt, e, p) {
        return None;
    }
    let f = rt.new_object(SEAT);
    if crate::mod_entities::virtual_call(rt, e, POSITION_RIDER_KEY, POSITION_RIDER, &[p, f])
        .is_none()
    {
        default_position_rider(rt, e, p, f);
    }
    rt.report_thrown("positionRider");
    (rt.get_field(f, "$set").i() != 0).then(|| {
        [
            rt.get_field(f, "$x").d(),
            rt.get_field(f, "$y").d(),
            rt.get_field(f, "$z").d(),
        ]
    })
}

/// `Entity.positionRider(passenger, callback)`, vanilla's.
fn default_position_rider(rt: &mut Rt, e: V, p: V, f: V) {
    let ride = riding_position(rt, e, p);
    let attach = vehicle_attachment(rt, p, e);
    let to = [
        ride[0] - attach[0],
        ride[1] - attach[1],
        ride[2] - attach[2],
    ];
    api::virtual_call(
        rt,
        (ACCEPT_KEY, ACCEPT),
        &[f, p, V::D(to[0]), V::D(to[1]), V::D(to[2])],
    );
}

/// `Entity.getDismountLocationForPassenger(passenger)`: the mod's own, else vanilla's: where it
/// stands, on top of its box.
fn dismount_location(rt: &mut Rt, e: V, p: V) -> [f64; 3] {
    match crate::mod_entities::virtual_call(rt, e, DISMOUNT_KEY, DISMOUNT, &[p]) {
        Some(v) => of_vec3(rt, v),
        None => default_dismount(rt, e),
    }
}

fn default_dismount(rt: &mut Rt, e: V) -> [f64; 3] {
    let p = at(rt, e);
    let bb = api::virtual_call(rt, (BOUNDING_BOX_KEY, BOUNDING_BOX), &[e]);
    let top = if bb.r() != 0 {
        crate::shapes::aabb_of(rt, bb)[4]
    } else {
        p[1] + f64::from(crate::mod_entities::own_size(rt, e)[1])
    };
    [p[0], top, p[2]]
}

/// `Entity.addPassenger`, vanilla's: last, except that on a server a player goes first when the
/// first isn't one.
fn default_add(rt: &mut Rt, e: V, p: V) {
    let mut items = passengers(rt, e);
    if items.iter().any(|&q| same(rt, q, p)) {
        return;
    }
    let player = crate::api_common::is_player_class(rt.class_of(p));
    let first_player = items
        .first()
        .is_some_and(|&q| crate::api_common::is_player_class(rt.class_of(q)));
    if !client(rt) && player && !first_player {
        items.insert(0, p);
    } else {
        items.push(p);
    }
    set_passengers(rt, e, items);
}

/// `Entity.removePassenger`, vanilla's: off the list, and the rider waits out its boarding cooldown.
fn default_remove(rt: &mut Rt, e: V, p: V) {
    let items: Vec<V> = passengers(rt, e)
        .into_iter()
        .filter(|&q| !same(rt, q, p))
        .collect();
    set_passengers(rt, e, items);
    let until = game_time(rt) + BOARDING_COOLDOWN;
    rt.put_field(p, "$boardingUntil", V::J(until));
}

/// `vehicle.removePassenger(p)`, virtually.
fn remove_passenger(rt: &mut Rt, e: V, p: V) {
    if crate::mod_entities::virtual_call(rt, e, REMOVE_PASSENGER_KEY, REMOVE_PASSENGER, &[p])
        .is_none()
    {
        default_remove(rt, e, p);
    }
    rt.report_thrown("removePassenger");
}

/// `e` and its passengers as vanilla's streams walk them, depth first: each before its own
/// (`Entity.getSelfAndPassengers`), or after them (`getPassengersAndSelf`).
pub(crate) fn passenger_tree(rt: &Rt, e: V, self_first: bool) -> Vec<V> {
    fn walk(rt: &Rt, e: V, self_first: bool, out: &mut Vec<V>) {
        if self_first {
            out.push(e);
        }
        for p in passengers(rt, e).into_iter().filter(|p| p.r() != 0) {
            walk(rt, p, self_first, out);
        }
        if !self_first {
            out.push(e);
        }
    }
    let mut out = Vec::new();
    walk(rt, e, self_first, &mut out);
    out
}

/// The mod's entity `e`'s calls about its passengers (`Entity`'s, for a mod's entity): None for
/// others.
pub(crate) fn vehicle_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    Some(match (name, desc) {
        ("getPassengers", "()Ljava/util/List;") => {
            let items = passengers(rt, e);
            jutil::new_list(rt, items)
        }
        // Itself before its passengers (`getSelfAndPassengers`) or after them
        // (`getPassengersAndSelf`), as streams; `getIndirectPassengers` without it (Create's
        // contraption collisions and seats walk them).
        ("getSelfAndPassengers" | "getPassengersAndSelf", "()Ljava/util/stream/Stream;") => {
            let all = passenger_tree(rt, e, name == "getSelfAndPassengers");
            rt.alloc("java/util/stream/Stream", Data::Stream(all))
        }
        ("getIndirectPassengers", "()Ljava/lang/Iterable;") => {
            let all = passenger_tree(rt, e, true).split_off(1);
            jutil::new_list(rt, all)
        }
        ("isVehicle", "()Z") => V::I(i32::from(!passengers(rt, e).is_empty())),
        ("hasPassenger", "(Lnet/minecraft/world/entity/Entity;)Z") => {
            V::I(i32::from(holds(rt, e, a(1))))
        }
        ("hasPassenger", "(Ljava/util/function/Predicate;)Z") => {
            let mut found = false;
            for p in passengers(rt, e) {
                if rt.call_functional(TEST, a(1), &[p]).i() != 0 {
                    found = true;
                    break;
                }
            }
            V::I(i32::from(found))
        }
        ("getFirstPassenger", "()Lnet/minecraft/world/entity/Entity;") => {
            passengers(rt, e).first().copied().unwrap_or(V::NULL)
        }
        // `Entity.addPassenger`: only through `startRiding`, which sets the rider's vehicle first.
        ("addPassenger", "(Lnet/minecraft/world/entity/Entity;)V") => {
            default_add(rt, e, a(1));
            V::Void
        }
        ("removePassenger", "(Lnet/minecraft/world/entity/Entity;)V") => {
            default_remove(rt, e, a(1));
            V::Void
        }
        ("canAddPassenger", "(Lnet/minecraft/world/entity/Entity;)Z") => {
            V::I(i32::from(passengers(rt, e).is_empty()))
        }
        // The last on gets off first (`Entity.ejectPassengers`).
        ("ejectPassengers", "()V") => {
            for p in passengers(rt, e).into_iter().rev() {
                stop_riding(rt, e, p);
            }
            V::Void
        }
        ("positionRider", "(Lnet/minecraft/world/entity/Entity;)V") => {
            let p = a(1);
            if let (Some(seat), Some(uuid)) = (seat_of(rt, e, p), server_player(rt, p)) {
                rt.host_call(|h| h.ride_player(uuid, Ride::Sit(seat)));
            }
            V::Void
        }
        (
            "positionRider",
            "(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/entity/Entity$MoveFunction;)V",
        ) => {
            default_position_rider(rt, e, a(1), a(2));
            V::Void
        }
        (
            "getPassengerRidingPosition",
            "(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/phys/Vec3;",
        ) => {
            let p = default_riding_position(rt, e, a(1));
            crate::api_common::vec3(rt, p[0], p[1], p[2])
        }
        ("getPassengerAttachmentPoint", _) => {
            let p = default_attachment(rt, e);
            crate::api_common::vec3(rt, p[0], p[1], p[2])
        }
        (
            "getDismountLocationForPassenger",
            "(Lnet/minecraft/world/entity/LivingEntity;)Lnet/minecraft/world/phys/Vec3;",
        ) => {
            let p = default_dismount(rt, e);
            crate::api_common::vec3(rt, p[0], p[1], p[2])
        }
        ("onPassengerTurned", "(Lnet/minecraft/world/entity/Entity;)V") => V::Void,
        _ => return None,
    })
}

/// `MoveFunction.accept(entity, x, y, z)` on the seat [`seat_of`] reads: the place kept.
pub(crate) fn seat_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let f = *args.first()?;
    if name != "accept" || rt.class_of(f) != SEAT || args.len() < 5 {
        return None;
    }
    for (field, v) in ["$x", "$y", "$z"].into_iter().zip(&args[2..5]) {
        rt.put_field(f, field, V::D(v.d()));
    }
    rt.put_field(f, "$set", V::I(1));
    Some(V::Void)
}

/// `LivingEntity.stopRiding` for the server's player `p` on the mod's entity `e`: off it
/// (`removePassenger`, the mod's own first), then where it lets them out (`dismountVehicle`: its
/// `getDismountLocationForPassenger`, or beside a vehicle that is gone, at the higher of the two's
/// feet).
fn stop_riding(rt: &mut Rt, e: V, p: V) {
    remove_passenger(rt, e, p);
    let Some(uuid) = server_player(rt, p) else {
        return;
    };
    let to = if rt.get_field(e, "removalReason").r() != 0 {
        let me = rt
            .host()
            .and_then(|h| h.players().into_iter().find(|q| q.0 == uuid))
            .map(|q| q.1);
        let me = me.unwrap_or_else(|| at(rt, e));
        [me[0], me[1].max(at(rt, e)[1]), me[2]]
    } else {
        dismount_location(rt, e, p)
    };
    rt.host_call(|h| h.ride_player(uuid, Ride::Leave { to }));
}

/// A player's calls about riding (`Entity`'s, `Player`'s, `ServerPlayer`'s, for a player object):
/// None where the answer isn't the runtime's (getting on a vehicle of the game's).
pub(crate) fn player_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let p = a(0);
    let uuid = server_player(rt, p);
    let own = rt.class_of(p) == LOCAL_PLAYER;
    // The client's own player rides what its client was told it rides.
    let vehicle = match uuid {
        Some(u) => rt.host_call(|h| h.player_vehicle(u)).flatten(),
        None if own => rt.host().and_then(|h| h.client_player_vehicle()),
        None => None,
    };
    Some(match (name, desc) {
        ("isPassenger", "()Z") => V::I(i32::from(vehicle.is_some())),
        ("getVehicle", "()Lnet/minecraft/world/entity/Entity;")
        | ("getRootVehicle", "()Lnet/minecraft/world/entity/Entity;") => match vehicle {
            Some(id) => crate::mod_entities::live_by_id(rt, id)
                .or_else(|| crate::vanilla_entities::by_id(rt, id))
                .unwrap_or(V::NULL),
            None if name == "getRootVehicle" => p,
            None => V::NULL,
        },
        (
            "getVehicleAttachmentPoint",
            "(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/phys/Vec3;",
        ) => {
            let [x, y, z] = PLAYER_VEHICLE_ATTACHMENT;
            crate::api_common::vec3(rt, x, y, z)
        }
        ("startRiding", "(Lnet/minecraft/world/entity/Entity;)Z") => {
            return start_riding(rt, p, uuid, vehicle, a(1), false);
        }
        ("startRiding", "(Lnet/minecraft/world/entity/Entity;Z)Z") => {
            return start_riding(rt, p, uuid, vehicle, a(1), a(2).i() != 0);
        }
        ("stopRiding" | "removeVehicle", "()V") => {
            let (Some(uuid), Some(id)) = (uuid, vehicle) else {
                // Riding nothing: nothing to get off (a client's player gets off by its server).
                return Some(V::Void);
            };
            match crate::mod_entities::live_by_id(rt, id) {
                Some(e) if name == "removeVehicle" => remove_passenger(rt, e, p),
                Some(e) => stop_riding(rt, e, p),
                // A vehicle of the game's: the game lets them off where it lets its riders off.
                None => {
                    rt.host_call(|h| h.ride_player(uuid, Ride::Dismount));
                }
            }
            V::Void
        }
        _ => return None,
    })
}

/// `Entity.startRiding(vehicle, force)` for a player, then `ServerPlayer.startRiding`'s seat.
/// Only a server's player gets on, and only a mod's entity (a vehicle of the game's is the game's
/// to board: None). False for what they ride already, while they sneak or wait out their boarding
/// cooldown (`canRide`), and when the vehicle takes no more (`canAddPassenger`), unless forced.
fn start_riding(
    rt: &mut Rt,
    p: V,
    uuid: Option<u128>,
    riding: Option<i32>,
    vehicle: V,
    force: bool,
) -> Option<V> {
    if vehicle.r() == 0 {
        // `vehicle == this.vehicle` for one that rides nothing; nothing to ride otherwise.
        return Some(V::I(0));
    }
    if !crate::mod_entities::is_mod_entity(rt, vehicle) {
        return None;
    }
    let Some(uuid) = uuid else {
        return Some(V::I(0));
    };
    let id = rt.get_field(vehicle, "$id").i();
    if riding == Some(id) || rt.get_field(vehicle, "removalReason").r() != 0 {
        return Some(V::I(0));
    }
    if !force {
        let sneaking = rt.host_call(|h| h.player_wants_off(uuid)).unwrap_or(false);
        let cooling = rt.get_field(p, "$boardingUntil").j() > game_time(rt);
        if sneaking || cooling {
            return Some(V::I(0));
        }
        let can = match crate::mod_entities::virtual_call(
            rt,
            vehicle,
            CAN_ADD_PASSENGER_KEY,
            CAN_ADD_PASSENGER,
            &[p],
        ) {
            Some(v) => v.i() != 0,
            None => passengers(rt, vehicle).is_empty(),
        };
        rt.report_thrown("canAddPassenger");
        if !can {
            return Some(V::I(0));
        }
    }
    // Off what they ride first (`if (this.isPassenger()) this.stopRiding()`).
    if let Some(old) = riding {
        match crate::mod_entities::live_by_id(rt, old) {
            Some(e) => stop_riding(rt, e, p),
            None => {
                rt.host_call(|h| h.ride_player(uuid, Ride::Dismount));
            }
        }
        if rt.host_call(|h| h.player_vehicle(uuid)).flatten().is_some() {
            return Some(V::I(0));
        }
    }
    if crate::mod_entities::virtual_call(rt, vehicle, ADD_PASSENGER_KEY, ADD_PASSENGER, &[p])
        .is_none()
    {
        default_add(rt, vehicle, p);
    }
    rt.report_thrown("addPassenger");
    // The players about it have it before they hear who rides it.
    let dimension = rt
        .str_of(rt.get_field(vehicle, "$dimension"))
        .unwrap_or("")
        .to_owned();
    crate::mod_entities::track(rt, &dimension);
    let seat = seat_of(rt, vehicle, p).unwrap_or_else(|| at(rt, vehicle));
    if rt.host_call(|h| {
        h.ride_player(
            uuid,
            Ride::Board {
                vehicle: id,
                at: seat,
            },
        )
    }) != Some(true)
    {
        let items: Vec<V> = passengers(rt, vehicle)
            .into_iter()
            .filter(|&q| !same(rt, q, p))
            .collect();
        set_passengers(rt, vehicle, items);
        return Some(V::I(0));
    }
    Some(V::I(1))
}

/// `ServerLevel.tickPassenger` for the mod's entity `e`'s riders, after its own tick, on a server:
/// one whose vehicle is no longer `e` (dead, teleported, gone) is off it; one who sneaks gets off
/// (`Player.rideTick`); the others sit where it puts them (`Entity.rideTick`).
pub(crate) fn tick_riders(rt: &mut Rt, e: V) {
    let riders = passengers(rt, e);
    if riders.is_empty() || client(rt) {
        return;
    }
    let id = rt.get_field(e, "$id").i();
    for p in riders {
        let Some(uuid) = server_player(rt, p) else {
            continue;
        };
        if rt.host_call(|h| h.player_vehicle(uuid)).flatten() != Some(id) {
            remove_passenger(rt, e, p);
            continue;
        }
        if rt.host_call(|h| h.player_wants_off(uuid)).unwrap_or(false) {
            stop_riding(rt, e, p);
            continue;
        }
        if let Some(seat) = seat_of(rt, e, p) {
            rt.host_call(|h| h.ride_player(uuid, Ride::Sit(seat)));
        }
    }
}

/// `Entity.setRemoved`: a mod's entity leaving the level lets its riders off.
pub(crate) fn eject_removed(rt: &mut Rt, e: V) {
    if client(rt) {
        return;
    }
    for p in passengers(rt, e).into_iter().rev() {
        stop_riding(rt, e, p);
    }
}

/// `ClientPacketListener.handleSetEntityPassengersPacket` for a mod's entity on a client: its
/// copy's passengers become the entities of `riders` (ids) the client has, the client's own player
/// for its own id. False for a vehicle that isn't a mod's entity the client has.
pub fn client_passengers(rt: &mut Rt, vehicle: i32, riders: &[i32]) -> bool {
    let Some(e) = crate::mod_entities::live_by_id(rt, vehicle) else {
        return false;
    };
    let items: Vec<V> = riders
        .iter()
        .filter_map(|&id| crate::vanilla_entities::by_id(rt, id))
        .collect();
    set_passengers(rt, e, items);
    true
}

/// `Entity.rideTick` on a client: where its copy of the mod's entity `vehicle` seats the rider
/// `rider` (the client's own player for its own id), as the client places those it draws. A rider
/// the client was told rides it and its copy doesn't hold yet (its passengers came before it) is
/// added. None for a vehicle that isn't a mod's entity the client has.
pub fn client_seat(rt: &mut Rt, vehicle: i32, rider: i32) -> Option<[f64; 3]> {
    let e = crate::mod_entities::live_by_id(rt, vehicle)?;
    let p = crate::vanilla_entities::by_id(rt, rider)?;
    if !holds(rt, e, p) {
        let mut items = passengers(rt, e);
        items.push(p);
        set_passengers(rt, e, items);
    }
    seat_of(rt, e, p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// A seat that seats its rider as vanilla's `Entity` does.
    const PLAIN: &str = "test/Seat";
    /// A seat with its own `positionRider` and dismount location, as Create's `SeatEntity` has.
    const OWN: &str = "test/OwnSeat";
    const FACTORY: &str = "test/SeatFactory";
    const FACTORY_CREATE: &str = "create.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";
    const ENTITY_INIT: &str = "net/minecraft/world/entity/Entity.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
    const ADD: &str =
        "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z";
    const START: &str = "net/minecraft/world/entity/player/Player.startRiding.(Lnet/minecraft/world/entity/Entity;)Z";
    const START_FORCED: &str =
        "net/minecraft/world/entity/Entity.startRiding.(Lnet/minecraft/world/entity/Entity;Z)Z";
    const STOP: &str = "net/minecraft/world/entity/Entity.stopRiding.()V";
    const IS_PASSENGER: &str = "net/minecraft/world/entity/Entity.isPassenger.()Z";
    const GET_VEHICLE: &str =
        "net/minecraft/world/entity/Entity.getVehicle.()Lnet/minecraft/world/entity/Entity;";
    const GET_PASSENGERS: &str =
        "net/minecraft/world/entity/Entity.getPassengers.()Ljava/util/List;";
    const DISCARD: &str = "net/minecraft/world/entity/Entity.discard.()V";
    const OVERWORLD: &str = "minecraft:overworld";
    const ME: u128 = 0xabc;
    const YOU: u128 = 0xdef;
    /// The seat's height, Create's `SeatEntity`'s (0.25 wide).
    const HEIGHT: f32 = 0.35;

    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let e = args[0];
        match key {
            // The rider a sixteenth above the seat's feet.
            POSITION_RIDER => {
                let [x, y, z] = at(rt, e);
                api::call(
                    rt,
                    ACCEPT_KEY,
                    &[args[2], args[1], V::D(x), V::D(y + 0.0625), V::D(z)],
                );
                V::Void
            }
            // Half a block above vanilla's (`super.getDismountLocationForPassenger(p).add(0, 0.5, 0)`).
            DISMOUNT => {
                let v = api::call(rt, DISMOUNT_KEY, &[e, args[1]]);
                let [x, y, z] = of_vec3(rt, v);
                crate::api_common::vec3(rt, x, y + 0.5, z)
            }
            FACTORY_CREATE if rt.class_of(e) == FACTORY => {
                let seat = rt.new_object(PLAIN);
                api::call(rt, ENTITY_INIT, &[seat, args[1], args[2]]);
                seat
            }
            _ => api::call(rt, owner_key, args),
        }
    }

    fn overrides(class: &str, key: &str) -> bool {
        class == OWN && matches!(key, POSITION_RIDER | DISMOUNT)
    }

    /// A runtime as the mod's: its seat type, registered as `test:seat`.
    fn runtime() -> (Rt, V) {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        rt.supers.insert(PLAIN, crate::mod_entities::ENTITY);
        rt.supers.insert(OWN, crate::mod_entities::ENTITY);
        let ty = rt.new_object("net/minecraft/world/entity/EntityType");
        rt.put_field(ty, "$width", V::F(0.25));
        rt.put_field(ty, "$height", V::F(HEIGHT));
        rt.put_field(ty, "$range", V::I(4));
        let factory = rt.new_object(FACTORY);
        rt.put_field(ty, "$factory", factory);
        rt.registered
            .insert(("entity_type".into(), "test:seat".into()), ty);
        (rt, ty)
    }

    /// A seat of `class` at `at`, added to the level.
    fn seat(rt: &mut Rt, class: &'static str, ty: V, at: [f64; 3]) -> V {
        let level = crate::glue::level(rt);
        let e = rt.new_object(class);
        api::call(rt, ENTITY_INIT, &[e, ty, level]);
        crate::mod_entities::set_pos(rt, e, at);
        assert_eq!(api::call(rt, ADD, &[level, e]), V::I(1));
        e
    }

    /// A server's player on a mod's entity as vanilla's `startRiding` and `Player.rideTick` have
    /// them: seated where vanilla's `positionRider` puts them (the top of its box, less the
    /// player's vehicle attachment, 0.6), kept there each tick, let off on top of its box once they
    /// sneak (`getDismountLocationForPassenger`), then kept off for the 60 ticks of their boarding
    /// cooldown, and while they sneak, unless forced.
    #[test]
    fn a_player_rides_a_mods_entity_until_they_sneak() {
        let (mut rt, ty) = runtime();
        let mut host = MemoryHost {
            player_id: Some(ME),
            players: vec![(ME, [0.5, 64.0, 2.5])],
            ..MemoryHost::default()
        };
        let top = 64.0 + f64::from(HEIGHT);
        let sit = [0.5, top - 0.6, 0.5];
        let (e, id) = rt.with_host(&mut host, |rt| {
            let e = seat(rt, PLAIN, ty, [0.5, 64.0, 0.5]);
            let me = crate::menus::server_player(rt, ME);
            assert_eq!(api::call(rt, START, &[me, e]), V::I(1));
            let riders = api::call(rt, GET_PASSENGERS, &[e]);
            assert_eq!(jutil::elements(rt, riders), vec![me]);
            assert_eq!(api::call(rt, IS_PASSENGER, &[me]), V::I(1));
            assert_eq!(api::call(rt, GET_VEHICLE, &[me]), e);
            crate::mod_entities::tick_all(rt, OVERWORLD);
            (e, rt.get_field(e, "$id").i())
        });
        assert_eq!(
            host.rides,
            vec![
                (
                    ME,
                    Ride::Board {
                        vehicle: id,
                        at: sit
                    }
                ),
                (ME, Ride::Sit(sit))
            ]
        );
        host.wanting_off.push(ME);
        rt.with_host(&mut host, |rt| crate::mod_entities::tick_all(rt, OVERWORLD));
        assert_eq!(
            host.rides.last(),
            Some(&(
                ME,
                Ride::Leave {
                    to: [0.5, top, 0.5]
                }
            )),
            "off on top of its box"
        );
        assert!(host.vehicles.is_empty());
        host.wanting_off.clear();
        rt.with_host(&mut host, |rt| {
            let me = crate::menus::server_player(rt, ME);
            assert!(passengers(rt, e).is_empty());
            assert_eq!(api::call(rt, IS_PASSENGER, &[me]), V::I(0));
            assert_eq!(api::call(rt, GET_VEHICLE, &[me]), V::NULL);
            assert_eq!(
                api::call(rt, START, &[me, e]),
                V::I(0),
                "the boarding cooldown"
            );
        });
        host.time = 60;
        host.wanting_off.push(ME);
        rt.with_host(&mut host, |rt| {
            let me = crate::menus::server_player(rt, ME);
            assert_eq!(api::call(rt, START, &[me, e]), V::I(0), "sneaking");
            assert_eq!(
                api::call(rt, START_FORCED, &[me, e, V::I(1)]),
                V::I(1),
                "forced"
            );
        });
        assert_eq!(host.vehicles.get(&ME), Some(&id));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's entity's passengers and theirs as vanilla's streams walk them, depth first: each
    /// before its own (`getSelfAndPassengers`), after them (`getPassengersAndSelf`), or without the
    /// one asked (`getIndirectPassengers`); a player carrying nobody is itself alone.
    #[test]
    fn a_mods_entity_walks_its_passengers_and_theirs_as_vanillas_streams() {
        const SELF_FIRST: &str =
            "net/minecraft/world/entity/Entity.getSelfAndPassengers.()Ljava/util/stream/Stream;";
        const SELF_LAST: &str =
            "net/minecraft/world/entity/Entity.getPassengersAndSelf.()Ljava/util/stream/Stream;";
        const INDIRECT: &str =
            "net/minecraft/world/entity/Entity.getIndirectPassengers.()Ljava/lang/Iterable;";
        let (mut rt, ty) = runtime();
        let mut host = MemoryHost {
            player_id: Some(ME),
            ..MemoryHost::default()
        };
        rt.with_host(&mut host, |rt| {
            let [train, carriage, guard] =
                [0.5, 3.5, 6.5].map(|x| seat(rt, PLAIN, ty, [x, 64.0, 0.5]));
            let me = crate::menus::server_player(rt, ME);
            set_passengers(rt, train, vec![carriage, guard]);
            set_passengers(rt, carriage, vec![me]);
            let walk = |rt: &mut Rt, key: &'static str, e: V| {
                let all = api::call(rt, key, &[e]);
                jutil::elements(rt, all)
            };
            assert_eq!(walk(rt, SELF_FIRST, train), [train, carriage, me, guard]);
            assert_eq!(walk(rt, SELF_LAST, train), [me, carriage, guard, train]);
            assert_eq!(walk(rt, INDIRECT, train), [carriage, me, guard]);
            assert_eq!(walk(rt, SELF_FIRST, me), [me]);
            assert_eq!(walk(rt, SELF_LAST, me), [me]);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own `positionRider` and dismount location (Create's seat's) place its rider, and
    /// `stopRiding` lets them off there at once.
    #[test]
    fn a_mods_own_seat_places_its_rider_and_lets_them_off_where_it_says() {
        let (mut rt, ty) = runtime();
        let mut host = MemoryHost {
            player_id: Some(ME),
            ..MemoryHost::default()
        };
        let id = rt.with_host(&mut host, |rt| {
            let e = seat(rt, OWN, ty, [3.5, 70.0, -1.5]);
            let me = crate::menus::server_player(rt, ME);
            assert_eq!(api::call(rt, START_FORCED, &[me, e, V::I(1)]), V::I(1));
            api::call(rt, STOP, &[me]);
            assert!(passengers(rt, e).is_empty());
            rt.get_field(e, "$id").i()
        });
        let top = 70.0 + f64::from(HEIGHT);
        let rides: Vec<Ride> = host.rides.iter().map(|r| r.1).collect();
        assert_eq!(
            rides,
            [
                Ride::Board {
                    vehicle: id,
                    at: [3.5, 70.0625, -1.5]
                },
                Ride::Leave {
                    to: [3.5, top + 0.5, -1.5]
                }
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's entity takes one passenger unless forced (`canAddPassenger`), and one removed lets
    /// its riders off, the last on first, where they are but no lower than its feet
    /// (`dismountVehicle` from a vehicle that is gone).
    #[test]
    fn a_removed_mods_entity_lets_its_riders_off_beside_it() {
        let (mut rt, ty) = runtime();
        let players = vec![(ME, [0.5, 63.0, 0.5]), (YOU, [1.5, 65.0, 0.5])];
        let mut host = MemoryHost {
            player_id: Some(ME),
            players,
            ..MemoryHost::default()
        };
        rt.with_host(&mut host, |rt| {
            let e = seat(rt, PLAIN, ty, [0.5, 64.0, 0.5]);
            let (me, you) = (
                crate::menus::server_player(rt, ME),
                crate::menus::server_player(rt, YOU),
            );
            assert_eq!(api::call(rt, START, &[me, e]), V::I(1));
            assert_eq!(api::call(rt, START, &[you, e]), V::I(0), "full");
            assert_eq!(api::call(rt, START_FORCED, &[you, e, V::I(1)]), V::I(1));
            assert_eq!(passengers(rt, e), vec![me, you]);
            api::call(rt, DISCARD, &[e]);
            assert!(passengers(rt, e).is_empty());
        });
        let offs: Vec<(u128, Ride)> = host
            .rides
            .iter()
            .filter(|r| matches!(r.1, Ride::Leave { .. }))
            .copied()
            .collect();
        assert_eq!(
            offs,
            [
                (
                    YOU,
                    Ride::Leave {
                        to: [1.5, 65.0, 0.5]
                    }
                ),
                (
                    ME,
                    Ride::Leave {
                        to: [0.5, 64.0, 0.5]
                    }
                )
            ]
        );
        assert!(host.vehicles.is_empty());
    }

    /// `ServerLevel.tickPassenger`: a rider the game has on something else now (dead, teleported,
    /// in a boat) is off the mod's entity's passengers at its next tick, with nothing asked of the
    /// game.
    #[test]
    fn a_rider_the_game_took_elsewhere_is_off_the_mods_entity() {
        let (mut rt, ty) = runtime();
        let mut host = MemoryHost {
            player_id: Some(ME),
            ..MemoryHost::default()
        };
        let e = rt.with_host(&mut host, |rt| {
            let e = seat(rt, PLAIN, ty, [0.5, 64.0, 0.5]);
            let me = crate::menus::server_player(rt, ME);
            assert_eq!(api::call(rt, START_FORCED, &[me, e, V::I(1)]), V::I(1));
            e
        });
        host.vehicles.insert(ME, 7);
        let asked = host.rides.len();
        rt.with_host(&mut host, |rt| crate::mod_entities::tick_all(rt, OVERWORLD));
        assert_eq!(host.rides.len(), asked, "nothing asked of the game");
        assert!(passengers(&rt, e).is_empty());
    }

    /// A client's copy of a mod's entity seats its own player where the server's seats them, told
    /// of the rider before its passengers came; the client's own player rides it as their client
    /// was told; the passengers the server sends replace its own.
    #[test]
    fn a_clients_copy_seats_its_own_player_as_the_server_does() {
        let (mut server, ty) = runtime();
        let (mut client, _) = runtime();
        let mut host = MemoryHost {
            players: vec![(ME, [0.5, 64.0, 0.5])],
            ..MemoryHost::default()
        };
        let id = server.with_host(&mut host, |rt| {
            let e = seat(rt, PLAIN, ty, [0.5, 64.0, 0.5]);
            crate::mod_entities::track(rt, OVERWORLD);
            rt.get_field(e, "$id").i()
        });
        let mut screen = MemoryHost {
            client: true,
            client_player_id: Some(5),
            client_vehicle: Some(id),
            ..MemoryHost::default()
        };
        for (_, _, data) in std::mem::take(&mut host.payloads) {
            client.with_host(&mut screen, |rt| crate::mod_entities::receive(rt, &data));
        }
        let sit = [0.5, 64.0 + f64::from(HEIGHT) - 0.6, 0.5];
        client.with_host(&mut screen, |rt| {
            assert!(!client_passengers(rt, id + 1, &[5]), "no such copy");
            assert_eq!(client_seat(rt, id + 1, 5), None);
            assert_eq!(client_seat(rt, id, 5), Some(sit));
            let copy = crate::mod_entities::live_by_id(rt, id).expect("the copy");
            let me = crate::network::local_player(rt);
            assert_eq!(passengers(rt, copy), vec![me]);
            assert_eq!(api::call(rt, IS_PASSENGER, &[me]), V::I(1));
            assert_eq!(api::call(rt, GET_VEHICLE, &[me]), copy);
            assert!(client_passengers(rt, id, &[]));
            assert!(passengers(rt, copy).is_empty(), "off");
        });
        assert!(client.unmapped.is_empty(), "{:?}", client.unmapped);
    }
}
