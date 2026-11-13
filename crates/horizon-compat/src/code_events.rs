//! The game's events posted by their own code.
//!
//! An event of NeoForge's is a small class: a constructor, the fields it sets and their getters.
//! The mod translator carries that code from the player's own NeoForge for a mod that listens to
//! the event ([`crate::code_event_classes`], `horizon_mod_translator::reach`), so nothing of the
//! class is modelled here: [`post`] makes the event through its own constructor, the listeners
//! read it through its own getters, and what one of them set comes back the same way.
//!
//! Such a class stands on event classes the runtime does name: the entity's event, the living
//! entity's and the player's above it, the level's. Their part is answered here for any event
//! that is its own code, the game's or a mod's: the constructor keeps the entity or the level,
//! the getter hands it back.
//!
//! One class made the same way is no event: who plays (`Minecraft.getUser()`), whose name and
//! UUID nine of the pack's mods ask (FTB Teams finds its own player by it).
//!
//! The hooks that post these are upstream Horizon's (its catalog names the NeoForge event each
//! stands for): the game tells of a player in another dimension, an entity a player begins to be
//! told of, a mob's changed equipment, and the runtime hands that on as the event.
use crate::{Rt, V, host::HandStack};

pub use crate::code_event_classes::CLASSES;

const EVENT: &str = "net/neoforged/bus/api/Event";
const ENTITY_EVENT: &str = "net/neoforged/neoforge/event/entity/EntityEvent";
const LIVING_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent";
const PLAYER_EVENT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent";
const LEVEL_EVENT: &str = "net/neoforged/neoforge/event/level/LevelEvent";
const EQUIPMENT_SLOT: &str = "net/minecraft/world/entity/EquipmentSlot";

/// The named classes one above the other, as their class files have them.
const SUPERS: [(&str, &str); 4] = [(PLAYER_EVENT, LIVING_EVENT), (LIVING_EVENT, ENTITY_EVENT), (ENTITY_EVENT, EVENT), (LEVEL_EVENT, EVENT)];

/// A listener for an event class above ours hears ours: the runtime learns what stands on what.
fn know_classes(rt: &mut Rt) {
    if rt.supers.get(PLAYER_EVENT) != Some(&LIVING_EVENT) || rt.supers.get(LEVEL_EVENT) != Some(&EVENT) {
        for (class, sup) in SUPERS {
            rt.supers.insert(class, sup);
        }
    }
}

/// The class of the list whose name ends `short` (`PlayerEvent$StartTracking`).
fn class(short: &str) -> Option<&'static str> {
    CLASSES.iter().map(|c| c.0).find(|c| c.rsplit('/').next() == Some(short))
}

/// Whether a listener would hear the event `short`.
pub(crate) fn heard(rt: &mut Rt, short: &str) -> bool {
    know_classes(rt);
    class(short).is_some_and(|class| !crate::neo::hearing(rt, class).is_empty())
}

/// An object of the list's class `short`, made by its own constructor (`desc`, with `args`).
/// None when no mod carries the class's code (none names it, or the mods were translated without
/// the game's own code).
fn make(rt: &mut Rt, short: &str, desc: &str, args: &[V]) -> Option<V> {
    let class = class(short)?;
    rt.reflect(class, &["<init>.", desc].concat(), args).filter(|made| made.r() != 0)
}

/// Makes the event `short` by its own constructor (`desc`, with `args`) and posts it. The event,
/// for what its listeners left in it. None when nobody listens, or when no mod carries the
/// class's code.
pub(crate) fn post(rt: &mut Rt, short: &str, desc: &str, args: &[V]) -> Option<V> {
    if !heard(rt, short) {
        return None;
    }
    let ev = make(rt, short, desc, args)?;
    crate::neo::post(rt, class(short)?, ev);
    rt.report_thrown(short);
    Some(ev)
}

/// A method of the event `short`'s own (`key`: its name and descriptor, one the list names for
/// the class) asked of `ev`.
fn own_call(rt: &mut Rt, short: &str, key: &'static str, ev: V) -> V {
    let Some(class) = class(short) else {
        return V::Void;
    };
    let owner_key = rt.static_key(&[class, ".", key].concat());
    match rt.vcall {
        Some(vcall) => vcall(rt, owner_key, key, &[ev]),
        None => crate::api::call(rt, owner_key, &[ev]),
    }
}

/// Whether a listener canceled `ev` (`ICancellableEvent.setCanceled`, kept where the bus reads it).
fn canceled(rt: &Rt, ev: V) -> bool {
    rt.get_field(ev, "$canceled").i() != 0
}

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    (matches!(owner, ENTITY_EVENT | LIVING_EVENT | PLAYER_EVENT) && matches!(name, "<init>" | "getEntity"))
        || (owner == LEVEL_EVENT && matches!(name, "<init>" | "getLevel"))
        || (owner == MINECRAFT && name == "getUser")
}

const MINECRAFT: &str = "net/minecraft/client/Minecraft";
/// Who plays, once made ([`user`]).
const USER: &str = "compat:user";

/// `Minecraft.getUser()`: who plays, as the game's own class holds them, which the mods that ask
/// carry: the name and UUID of the profile the window plays under (the game tells them in a
/// world or not), no token, a Microsoft account's kind. The same object from then on. Where no
/// game is at hand to tell (a mod asks while it loads) it is a player without an account named
/// "Player", made anew at each asking, so that the profile is right once the game tells it.
/// Null when no mod carries the class.
fn user(rt: &mut Rt) -> V {
    if let Some(&user) = rt.interned.get(USER) {
        return user;
    }
    let told = rt.host().and_then(|h| h.client_profile());
    let known = told.is_some();
    let (name, id) = told.unwrap_or_else(|| {
        let (high, low) = crate::tags::name_uuid(b"OfflinePlayer:Player");
        ("Player".to_owned(), (u128::from(high as u64) << 64) | u128::from(low as u64))
    });
    let uuid = crate::tags::uuid(rt, (id >> 64) as i64, id as i64);
    let (name, token) = (rt.string(&name), rt.string(""));
    let none = crate::api::call(rt, "java/util/Optional.empty.()Ljava/util/Optional;", &[]);
    let kind = crate::records::enum_constant(rt, "net/minecraft/client/User$Type.MSA").unwrap_or(V::NULL);
    let desc = "(Ljava/lang/String;Ljava/util/UUID;Ljava/lang/String;Ljava/util/Optional;Ljava/util/Optional;Lnet/minecraft/client/User$Type;)V";
    let Some(user) = make(rt, "User", desc, &[name, uuid, token, none, none, kind]) else {
        return V::NULL;
    };
    if known {
        rt.interned.insert(USER.into(), user);
    }
    user
}

/// The named event classes' part of an event that is its own code: `super(entity)` keeps the
/// entity (the level), `getEntity()` hands it back, null where no constructor kept one. An event
/// another module made is answered there: those modules are asked before this one.
pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    if owner == MINECRAFT {
        return Some(user(rt));
    }
    let ev = *args.first()?;
    let field = if owner == LEVEL_EVENT { "$level" } else { "$entity" };
    match name {
        "<init>" => {
            rt.put_field(ev, field, args.get(1).copied().unwrap_or(V::NULL));
            Some(V::Void)
        }
        _ => Some(rt.get_field(ev, field)).map(|kept| if kept.r() != 0 { kept } else { V::NULL }),
    }
}

const PLAYER: &str = "Lnet/minecraft/world/entity/player/Player;";
const ENTITY: &str = "Lnet/minecraft/world/entity/Entity;";
const STACK: &str = "Lnet/minecraft/world/item/ItemStack;";
const KEY: &str = "Lnet/minecraft/resources/ResourceKey;";

/// `PlayerEvent.PlayerChangedDimensionEvent`: server player `player` came from dimension `from`
/// to `to`.
pub(crate) fn changed_dimension(rt: &mut Rt, player: u128, from: &str, to: &str) {
    let event = "PlayerEvent$PlayerChangedDimensionEvent";
    if !heard(rt, event) {
        return;
    }
    let p = crate::menus::server_player(rt, player);
    let (from, to) = (rt.string(&["dimension#", from].concat()), rt.string(&["dimension#", to].concat()));
    post(rt, event, &["(", PLAYER, KEY, KEY, ")V"].concat(), &[p, from, to]);
}

/// `AdvancementEvent.AdvancementEarnEvent`: server player `player` made the advancement
/// `advancement` (its id).
pub(crate) fn advancement_earned(rt: &mut Rt, player: u128, advancement: &str) {
    let event = "AdvancementEvent$AdvancementEarnEvent";
    if !heard(rt, event) {
        return;
    }
    let p = crate::menus::server_player(rt, player);
    let holder = crate::player_advancements::holder(rt, advancement);
    post(rt, event, &["(", PLAYER, "Lnet/minecraft/advancements/AdvancementHolder;)V"].concat(), &[p, holder]);
}

/// `PlayerEvent.StartTracking` and `StopTracking`: server player `player` begins (`start`) or
/// ends to be told of the game's entity `entity` of type `kind`.
pub(crate) fn tracking(rt: &mut Rt, player: u128, entity: i32, kind: &str, start: bool) {
    let event = if start { "PlayerEvent$StartTracking" } else { "PlayerEvent$StopTracking" };
    if !heard(rt, event) {
        return;
    }
    let p = crate::menus::server_player(rt, player);
    let target = crate::vanilla_entities::of(rt, entity, kind, None);
    if target.r() == 0 {
        return;
    }
    post(rt, event, &["(", PLAYER, ENTITY, ")V"].concat(), &[p, target]);
}

/// `PlayerEvent.ItemCraftedEvent`: server player `player` took `stack` out of a crafting grid.
/// The grid itself is not the mods' to read: an empty one of its size.
pub(crate) fn item_crafted(rt: &mut Rt, player: u128, stack: HandStack) {
    let event = "PlayerEvent$ItemCraftedEvent";
    if !heard(rt, event) {
        return;
    }
    let p = crate::menus::server_player(rt, player);
    let crafted = crate::hand::load(rt, Some(stack));
    let grid = crate::jlang::construct(rt, "net/minecraft/world/SimpleContainer", "(I)V", &[V::I(9)]);
    post(rt, event, &["(", PLAYER, STACK, "Lnet/minecraft/world/Container;)V"].concat(), &[p, crafted, grid]);
}

/// Whether a mod listens to `LivingEquipmentChangeEvent`: the game reads every mob's equipment
/// each tick only then.
pub(crate) fn equipment_heard(rt: &mut Rt) -> bool {
    heard(rt, "LivingEquipmentChangeEvent")
}

/// `LivingEquipmentChangeEvent`: the game's living entity `entity` of type `kind` holds or wears
/// `to` in `slot` (`mainhand`, `head`...) where it had `from`. A slot 1.21.1 has no name for (a
/// saddle's) is not told.
pub(crate) fn equipment_change(rt: &mut Rt, entity: i32, kind: &str, slot: &str, from: Option<HandStack>, to: Option<HandStack>) {
    let event = "LivingEquipmentChangeEvent";
    if !heard(rt, event) {
        return;
    }
    let Some(slot) = crate::records::enum_constant(rt, &[EQUIPMENT_SLOT, ".", &slot.to_ascii_uppercase()].concat()) else {
        return;
    };
    let living = entity_of(rt, entity, kind);
    if living.r() == 0 {
        return;
    }
    let (from, to) = (crate::hand::load(rt, from), crate::hand::load(rt, to));
    let desc = ["(Lnet/minecraft/world/entity/LivingEntity;L", EQUIPMENT_SLOT, ";", STACK, STACK, ")V"].concat();
    post(rt, event, &desc, &[living, slot, from, to]);
}

/// The object of the game's entity `id` of type `kind`: a player by their entity id, any other by
/// its type.
fn entity_of(rt: &mut Rt, id: i32, kind: &str) -> V {
    match kind {
        "minecraft:player" => crate::vanilla_entities::by_id(rt, id).unwrap_or(V::NULL),
        _ => crate::vanilla_entities::of(rt, id, kind, None),
    }
}

/// `EntityMountEvent`: in `dimension`, entity `entity` gets on (`mounting`) or off entity
/// `vehicle`. Whether a listener canceled it.
pub(crate) fn mount(rt: &mut Rt, dimension: &str, entity: i32, kind: &str, vehicle: i32, vehicle_kind: &str, mounting: bool) -> bool {
    let event = "EntityMountEvent";
    if !heard(rt, event) {
        return false;
    }
    let (rider, mount) = (entity_of(rt, entity, kind), entity_of(rt, vehicle, vehicle_kind));
    if rider.r() == 0 || mount.r() == 0 {
        return false;
    }
    let level = crate::glue::server_level(rt, dimension);
    let desc = ["(", ENTITY, ENTITY, "Lnet/minecraft/world/level/Level;Z)V"].concat();
    post(rt, event, &desc, &[rider, mount, level, V::I(i32::from(mounting))]).is_some_and(|ev| canceled(rt, ev))
}

/// `SleepFinishedTimeEvent`: the players of `dimension` slept the night through, and its time,
/// `current`, is about to be `new`. The time a listener set instead (`setTimeAddition`, which
/// takes none before `current`); None when none did.
pub(crate) fn sleep_finished(rt: &mut Rt, dimension: &str, current: i64, new: i64) -> Option<i64> {
    let event = "SleepFinishedTimeEvent";
    if !heard(rt, event) {
        return None;
    }
    let level = crate::glue::server_level(rt, dimension);
    let ev = post(rt, event, "(Lnet/minecraft/server/level/ServerLevel;JJ)V", &[level, V::J(new), V::J(current)])?;
    let set = own_call(rt, event, "getNewTime.()J", ev).j();
    (set != new).then_some(set)
}

/// `ChunkEvent.Load` and `Unload`: the chunk at chunk coordinates `chunk` of `dimension` came into
/// the server's level (`new`: made just now) or left it (`loaded` false).
pub(crate) fn chunk(rt: &mut Rt, dimension: &str, chunk: [i32; 2], loaded: bool, new: bool) {
    let event = if loaded { "ChunkEvent$Load" } else { "ChunkEvent$Unload" };
    if !heard(rt, event) {
        return;
    }
    let level = crate::glue::server_level(rt, dimension);
    let c = crate::api::level_chunk(rt, level, chunk[0], chunk[1]);
    match loaded {
        true => post(rt, event, "(Lnet/minecraft/world/level/chunk/ChunkAccess;Z)V", &[c, V::I(i32::from(new))]),
        false => post(rt, event, "(Lnet/minecraft/world/level/chunk/ChunkAccess;)V", &[c]),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    const TRACKING: &str = "PlayerEvent$StartTracking";
    const PLAYER_INIT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent.<init>.(Lnet/minecraft/world/entity/player/Player;)V";
    const LIVING_GET: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent.getEntity.()Lnet/minecraft/world/entity/LivingEntity;";
    const CANCEL: &str = "net/neoforged/bus/api/ICancellableEvent.setCanceled.(Z)V";

    /// The event's class as the mod translator carries it: its constructor calls the player
    /// event's above it and keeps its own field, as the class file does.
    fn own_code(rt: &mut Rt, class: &str, what: &str, args: &[V]) -> Option<V> {
        if Some(class) != super::class(TRACKING) || !what.starts_with("<init>.") {
            return None;
        }
        // An object of a translated class: it has the class's id.
        let ev = rt.new_object_id(super::class(TRACKING)?, 7);
        api::call(rt, PLAYER_INIT, &[ev, args[0]]);
        rt.put_field(ev, "target", args[1]);
        Some(ev)
    }

    /// Listener 1 notes whose event it heard and of what; listener 2, for any player's event,
    /// cancels what it hears.
    fn listeners(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let ev = *args.last()?;
        match id {
            1 => {
                let who = api::call(rt, LIVING_GET, &[ev]);
                let target = rt.get_field(ev, "target");
                rt.interned.insert("test:who".into(), who);
                rt.interned.insert("test:target".into(), target);
            }
            2 => {
                api::call(rt, CANCEL, &[ev, V::I(1)]);
            }
            _ => return None,
        }
        Some(V::Void)
    }

    /// An event of the list is made by its own constructor and heard by its listeners, and by
    /// those of the named event class above it: the player its constructor handed up is the
    /// event's entity, and what a listener set (canceled) is read back. With no listener nothing
    /// is made; with one but no code of the class (a mod translated without the game's), nothing
    /// is posted.
    #[test]
    fn an_event_is_made_by_its_own_code_and_stands_on_the_named_classes() {
        crate::install();
        let mut rt = Rt::new();
        let class = super::class(TRACKING).unwrap();
        let (player, cow) = (rt.new_object("net/minecraft/server/level/ServerPlayer"), rt.new_object("net/minecraft/world/entity/animal/Cow"));
        let desc = ["(", PLAYER, ENTITY, ")V"].concat();
        assert!(!heard(&mut rt, TRACKING));
        assert_eq!(post(&mut rt, TRACKING, &desc, &[player, cow]), None);

        rt.lambda_call = Some(listeners);
        // The class below the player's event, as the linked program tells it.
        rt.supers.insert(class, PLAYER_EVENT);
        let own = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        rt.listeners.push((class.to_owned(), own));
        assert!(heard(&mut rt, TRACKING));
        assert_eq!(post(&mut rt, TRACKING, &desc, &[player, cow]), None, "no code of the class");
        assert!(!rt.interned.contains_key("test:who"));

        rt.reflect = Some(own_code);
        let above = rt.lambda_id("java/util/function/Consumer", 2, vec![]);
        rt.listeners.push((PLAYER_EVENT.to_owned(), above));
        let ev = post(&mut rt, TRACKING, &desc, &[player, cow]).expect("made and posted");
        assert_eq!(rt.interned.get("test:who"), Some(&player));
        assert_eq!(rt.interned.get("test:target"), Some(&cow));
        assert!(canceled(&rt, ev), "the listener above heard it too");
        // The other events of the list are not heard for this one's listener.
        assert!(!heard(&mut rt, "PlayerEvent$StopTracking"));
        assert!(!heard(&mut rt, "ChunkEvent$Load"));
    }

    /// A game, which tells the profile its window plays under when it has one at hand.
    struct Window(Option<u128>);
    impl crate::host::Host for Window {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn game_time(&self) -> i64 {
            0
        }
        fn client_profile(&self) -> Option<(String, u128)> {
            self.0.map(|id| ("Notch".to_owned(), id))
        }
    }

    /// The class of who plays, as the translator carries it: its constructor keeps what it is
    /// handed.
    fn user_code(rt: &mut Rt, class: &str, what: &str, args: &[V]) -> Option<V> {
        if Some(class) != super::class("User") || !what.starts_with("<init>.(Ljava/lang/String;Ljava/util/UUID;") {
            return None;
        }
        let user = rt.new_object_id(super::class("User")?, 8);
        for (field, v) in ["name", "uuid", "accessToken", "xuid", "clientId", "type"].into_iter().zip(args) {
            rt.put_field(user, field, *v);
        }
        Some(user)
    }

    /// `Minecraft.getUser()` is who plays, made by the class's own constructor with the profile
    /// the game tells: its name and UUID, no token, a Microsoft account's kind, the same object
    /// from then on. Before any game tells (a mod asks while it loads) it is "Player" without an
    /// account (Java's `UUID.nameUUIDFromBytes("OfflinePlayer:Player")`), and not kept: the next
    /// asking has the game's. Null while no mod carries the class.
    #[test]
    fn who_plays_is_made_by_the_games_own_class() {
        crate::install();
        // (The key's return type spelled whole would make the class one the runtime names.)
        let key = |rt: &mut Rt| rt.static_key(&[MINECRAFT, ".getUser.()L", super::class("User").unwrap(), ";"].concat());
        let uuid_text = |rt: &mut Rt, user: V| {
            let id = rt.get_field(user, "uuid");
            let text = api::call(rt, "java/util/UUID.toString.()Ljava/lang/String;", &[id]);
            rt.str_of(text).unwrap_or("").to_owned()
        };
        let mut rt = Rt::new();
        let get = key(&mut rt);
        let mut loading = Window(None);
        assert_eq!(rt.with_host(&mut loading, |rt| api::call(rt, get, &[])), V::NULL, "no code of the class");

        rt.reflect = Some(user_code);
        let early = rt.with_host(&mut loading, |rt| api::call(rt, get, &[]));
        assert_eq!(rt.str_of(rt.get_field(early, "name")), Some("Player"));
        assert_eq!(uuid_text(&mut rt, early), "a01e3843-e521-3998-958a-f459800e4d11");

        let mut game = Window(Some(0x0123_4567_89ab_cdef_0011_2233_4455_6677));
        let user = rt.with_host(&mut game, |rt| api::call(rt, get, &[]));
        assert_ne!(user, early, "the early one was not kept");
        assert_eq!(rt.str_of(rt.get_field(user, "name")), Some("Notch"));
        assert_eq!(uuid_text(&mut rt, user), "01234567-89ab-cdef-0011-223344556677");
        assert_eq!(rt.str_of(rt.get_field(user, "accessToken")), Some(""));
        let kind = rt.get_field(user, "type");
        assert_eq!(rt.str_of(rt.get_field(kind, "$name")), Some("MSA"));
        // The same object from then on, whatever is at hand.
        assert_eq!(rt.with_host(&mut loading, |rt| api::call(rt, get, &[])), user);
    }

    /// The level's event class keeps the level its constructor is handed; an event no constructor
    /// handed an entity has none (null, as the field would be).
    #[test]
    fn the_named_classes_hand_back_what_a_constructor_handed_them() {
        crate::install();
        let mut rt = Rt::new();
        let (ev, level) = (rt.new_object("example/NightEvent"), rt.new_object("net/minecraft/server/level/ServerLevel"));
        let key = rt.static_key(&[LEVEL_EVENT, ".<init>.(Lnet/minecraft/world/level/LevelAccessor;)V"].concat());
        assert_eq!(call(&mut rt, key, &[ev, level]), Some(V::Void));
        let get = rt.static_key(&[LEVEL_EVENT, ".getLevel.()Lnet/minecraft/world/level/LevelAccessor;"].concat());
        assert_eq!(call(&mut rt, get, &[ev]), Some(level));
        let other = rt.new_object("example/OtherEvent");
        assert_eq!(call(&mut rt, LIVING_GET, &[other]), Some(V::NULL));
        assert!(claims(LIVING_EVENT, "getEntity", "") && claims(PLAYER_EVENT, "<init>", "") && !claims(LIVING_EVENT, "getAmount", ""));
    }
}
