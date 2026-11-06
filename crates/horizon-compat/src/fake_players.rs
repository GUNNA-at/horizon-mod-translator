//! NeoForge's `FakePlayer`: a server player a mod makes to act in the world by itself. Create's
//! deployer holds and uses its item through one, and mob farms and miners make others
//! (`FakePlayerFactory`). One has its own place, look, hands and inventory, never the acting
//! player's. This module is offered every player call first ([`claims`]) and answers those made on
//! a fake player from its own state, as vanilla's player object would. Calls it leaves go on to the
//! others, which answer from the object's own fields or the level.
use crate::{Rt, V};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub(crate) const FAKE_PLAYER: &str = "net/neoforged/neoforge/common/util/FakePlayer";
const FACTORY: &str = "net/neoforged/neoforge/common/util/FakePlayerFactory";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const GAME_PROFILE: &str = "com/mojang/authlib/GameProfile";
const PROFILE_ID: &str = "com/mojang/authlib/GameProfile.getId.()Ljava/util/UUID;";
const PROFILE_NAME: &str = "com/mojang/authlib/GameProfile.getName.()Ljava/lang/String;";
const LITERAL: &str = "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;";
const ADD: &str = "net/minecraft/world/entity/player/Inventory.add.(Lnet/minecraft/world/item/ItemStack;)Z";
/// The inventory's slots: 36 of its own, then the armor's 4 (feet to head) and the off hand.
const SLOTS: usize = 41;
const OFF_HAND: i32 = 40;
const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";

thread_local! {
    /// Whether this thread's runtime has made a fake player: until one is, no call is looked at.
    static ANY: Cell<bool> = const { Cell::new(false) };
    /// `FakePlayer` calls' keys as `ServerPlayer`'s, for those it inherits.
    static INHERITED: RefCell<HashMap<&'static str, &'static str>> = RefCell::new(HashMap::new());
}

/// Whether `v` is a fake player.
pub(crate) fn is_fake(rt: &Rt, v: V) -> bool {
    ANY.get() && matches!(v, V::R(r) if r != 0) && rt.get_field(v, "$fake").i() == 1
}

/// Whether `inv` is a fake player's inventory.
pub(crate) fn is_fake_inventory(rt: &Rt, inv: V) -> bool {
    ANY.get() && matches!(inv, V::R(r) if r != 0) && rt.get_field(inv, "$fake").i() == 1
}

/// The stack in slot `i` of fake inventory `inv`, `ItemStack.EMPTY` outside its slots.
pub(crate) fn slot(rt: &mut Rt, inv: V, i: i32) -> V {
    let slots = rt.get_field(inv, "$slots");
    let s = if (0..SLOTS as i32).contains(&i) { rt.array_get(slots, i) } else { V::NULL };
    if s.r() == 0 { empty(rt) } else { s }
}

/// Slot `i` of fake inventory `inv` holds `s` now.
pub(crate) fn set_slot(rt: &mut Rt, inv: V, i: i32, s: V) {
    if (0..SLOTS as i32).contains(&i) {
        let slots = rt.get_field(inv, "$slots");
        let s = if s.r() == 0 { empty(rt) } else { s };
        rt.array_set(slots, i, s);
    }
}

/// Its hotbar slot in hand (`Inventory.selected`).
pub(crate) fn selected(rt: &Rt, inv: V) -> i32 {
    rt.get_field(inv, "selected").i().clamp(0, 8)
}

fn empty(rt: &mut Rt) -> V {
    crate::api_common::get_static(rt, ITEM_STACK, "EMPTY").unwrap_or(V::NULL)
}

/// The calls this module answers may be any player's, its factory's and its own class's.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner == FAKE_PLAYER || owner == FACTORY || crate::api_common::is_player(owner)
}

/// A fake player's calls, its constructor's and its factory's; None for anything else.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
        (FAKE_PLAYER, "<init>") => {
            init(rt, a(0), a(1), a(2));
            return Some(V::Void);
        }
        (FACTORY, _) => return factory(rt, name, args),
        _ if !claims(owner, name, desc) => return None,
        _ => {}
    }
    if let Some(v) = is_fake(rt, a(0)).then(|| player_call(rt, name, desc, args)).flatten() {
        return Some(v);
    }
    // One `FakePlayer` inherits (a mod's subclass calls them through it): `ServerPlayer`'s.
    (owner == FAKE_PLAYER).then(|| crate::api::call(rt, inherited(key), args))
}

/// `key`, a `FakePlayer` method's, as `ServerPlayer`'s.
fn inherited(key: &'static str) -> &'static str {
    INHERITED.with(|m| {
        *m.borrow_mut().entry(key).or_insert_with(|| {
            let rest = key.strip_prefix(FAKE_PLAYER).unwrap_or(key);
            Box::leak(format!("{SERVER_PLAYER}{rest}").into_boxed_str())
        })
    })
}

/// `new FakePlayer(level, profile)`: in `level`, at its origin, looking south, with an empty
/// inventory of its own and an entity id no entity of the game's has.
fn init(rt: &mut Rt, p: V, level: V, profile: V) {
    ANY.set(true);
    rt.put_field(p, "$fake", V::I(1));
    rt.put_field(p, "$level", level);
    rt.put_field(p, "$profile", profile);
    for f in ["$x", "$y", "$z"] {
        rt.put_field(p, f, V::D(0.0));
    }
    rt.put_field(p, "$yaw", V::F(0.0));
    rt.put_field(p, "$pitch", V::F(0.0));
    let n = rt.interned.get("compat:fake-players").map_or(0, |v| v.i()) + 1;
    rt.interned.insert("compat:fake-players".into(), V::I(n));
    // Below every id the game hands out.
    rt.put_field(p, "$entityId", V::I(-n));
    let inv = rt.new_object(crate::menus::INVENTORY);
    rt.put_field(inv, "player", p);
    rt.put_field(inv, "selected", V::I(0));
    rt.put_field(inv, "$fake", V::I(1));
    let e = empty(rt);
    let slots = rt.alloc("[Lnet/minecraft/world/item/ItemStack;", crate::Data::Array(vec![e; SLOTS]));
    rt.put_field(inv, "$slots", slots);
    rt.put_field(p, "$inventory", inv);
    crate::menus::init_player(rt, p);
}

/// `FakePlayerFactory`: one fake player for each profile (`get`), the default one named
/// `[Minecraft]` (`getMinecraft`), and letting go of a level's (`unloadLevel`).
fn factory(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "get" => made(rt, a(0), a(1)),
        "getMinecraft" => {
            let profile = rt.new_object(GAME_PROFILE);
            // `FakePlayerFactory.MINECRAFT`: UUID 41C82C87-7AfB-4024-BA57-13D2C99CAE77.
            let uuid = crate::tags::uuid(rt, 0x41C8_2C87_7AFB_4024, 0xBA57_13D2_C99C_AE77u64 as i64);
            let n = rt.string("[Minecraft]");
            rt.put_field(profile, "$id", uuid);
            rt.put_field(profile, "$name", n);
            made(rt, a(0), profile)
        }
        "unloadLevel" => {
            let keys: Vec<String> = rt.interned.keys().filter(|k| k.starts_with("compat:fake-player/")).cloned().collect();
            for k in keys {
                if let Some(&p) = rt.interned.get(&k)
                    && rt.get_field(p, "$level") == a(0)
                {
                    rt.interned.remove(&k);
                }
            }
            V::Void
        }
        _ => return None,
    })
}

/// The factory's fake player for `profile` in `level`: made the first time, the same one after.
fn made(rt: &mut Rt, level: V, profile: V) -> V {
    let id = crate::menus::virt(rt, PROFILE_ID, profile, &[]);
    let (msb, lsb) = crate::tags::uuid_bits(rt, id);
    let key = format!("compat:fake-player/{}", crate::tags::uuid_text(msb, lsb));
    if let Some(&p) = rt.interned.get(&key)
        && rt.get_field(p, "$level") == level
    {
        return p;
    }
    let p = rt.new_object(FAKE_PLAYER);
    init(rt, p, level, profile);
    rt.interned.insert(key, p);
    p
}

/// The equipment slot named by `slot` (an `EquipmentSlot`) as an inventory slot: the hand's, the
/// off hand's, or the armor's.
fn equipment_slot(rt: &Rt, inv: V, slot: V) -> i32 {
    let name = rt.str_of(rt.get_field(slot, "$name")).unwrap_or("").to_owned();
    match name.as_str() {
        "OFFHAND" => OFF_HAND,
        "FEET" => 36,
        "LEGS" => 37,
        "CHEST" => 38,
        "HEAD" => 39,
        "BODY" => -1,
        _ => selected(rt, inv),
    }
}

/// The inventory slot of hand `hand` (an `InteractionHand`).
fn hand_slot(rt: &Rt, inv: V, hand: V) -> i32 {
    if rt.get_field(hand, "$ordinal").i() == 1 { OFF_HAND } else { selected(rt, inv) }
}

fn place(rt: &Rt, p: V) -> [f64; 3] {
    [rt.get_field(p, "$x").d(), rt.get_field(p, "$y").d(), rt.get_field(p, "$z").d()]
}

fn set_place(rt: &mut Rt, p: V, [x, y, z]: [f64; 3]) {
    rt.put_field(p, "$x", V::D(x));
    rt.put_field(p, "$y", V::D(y));
    rt.put_field(p, "$z", V::D(z));
}

/// Its eye height: standing, or crouching (`Player`'s poses).
fn eye_height(rt: &Rt, p: V) -> f32 {
    if rt.get_field(p, "$sneaking").i() != 0 { 1.27 } else { 1.62 }
}

fn player_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let p = a(0);
    let inv = rt.get_field(p, "$inventory");
    let (yaw, pitch) = (rt.get_field(p, "$yaw").f(), rt.get_field(p, "$pitch").f());
    let at = place(rt, p);
    Some(match name {
        // Who it is: its profile's, which a mod's own profile class answers.
        "getGameProfile" => rt.get_field(p, "$profile"),
        "getUUID" => {
            let profile = rt.get_field(p, "$profile");
            crate::menus::virt(rt, PROFILE_ID, profile, &[])
        }
        "getStringUUID" => {
            let profile = rt.get_field(p, "$profile");
            let id = crate::menus::virt(rt, PROFILE_ID, profile, &[]);
            let (msb, lsb) = crate::tags::uuid_bits(rt, id);
            rt.string(&crate::tags::uuid_text(msb, lsb))
        }
        "getScoreboardName" => {
            let profile = rt.get_field(p, "$profile");
            crate::menus::virt(rt, PROFILE_NAME, profile, &[])
        }
        "getName" | "getDisplayName" if desc == "()Lnet/minecraft/network/chat/Component;" => {
            let profile = rt.get_field(p, "$profile");
            let n = crate::menus::virt(rt, PROFILE_NAME, profile, &[]);
            crate::api::call(rt, LITERAL, &[n])
        }
        "getId" if desc == "()I" => rt.get_field(p, "$entityId"),
        "getType" => crate::mod_entities::vanilla_type(rt, "minecraft:player"),
        // Where it is and looks.
        "level" | "serverLevel" | "getCommandSenderWorld" => match rt.get_field(p, "$level") {
            l if l.r() != 0 => l,
            _ => crate::glue::level(rt),
        },
        "setPos" | "setPosRaw" | "moveTo" | "absMoveTo" | "teleportTo" if desc.starts_with("(DDD") => {
            set_place(rt, p, [a(1).d(), a(2).d(), a(3).d()]);
            if desc.starts_with("(DDDFF)") {
                rt.put_field(p, "$yaw", a(4));
                rt.put_field(p, "$pitch", a(5));
            }
            V::Void
        }
        "setPos" | "moveTo" if desc.starts_with("(Lnet/minecraft/world/phys/Vec3;") => {
            let v = a(1);
            let xyz = [
                crate::api::call(rt, "net/minecraft/world/phys/Vec3.x.()D", &[v]).d(),
                crate::api::call(rt, "net/minecraft/world/phys/Vec3.y.()D", &[v]).d(),
                crate::api::call(rt, "net/minecraft/world/phys/Vec3.z.()D", &[v]).d(),
            ];
            set_place(rt, p, xyz);
            V::Void
        }
        "getX" => V::D(at[0]),
        "getY" => V::D(at[1]),
        "getZ" => V::D(at[2]),
        "position" => crate::api_common::vec3(rt, at[0], at[1], at[2]),
        "blockPosition" | "getOnPos" => crate::api::block_pos(rt, at[0].floor() as i64, at[1].floor() as i64, at[2].floor() as i64),
        "getEyeY" => V::D(at[1] + f64::from(eye_height(rt, p))),
        "getEyeHeight" => V::F(eye_height(rt, p)),
        "getEyePosition" => crate::api_common::vec3(rt, at[0], at[1] + f64::from(eye_height(rt, p)), at[2]),
        "getBbWidth" => V::F(0.6),
        "getBbHeight" => V::F(if rt.get_field(p, "$sneaking").i() != 0 { 1.5 } else { 1.8 }),
        "getBoundingBox" => {
            let h = if rt.get_field(p, "$sneaking").i() != 0 { 1.5 } else { 1.8 };
            crate::shapes::aabb(rt, [at[0] - 0.3, at[1], at[2] - 0.3, at[0] + 0.3, at[1] + h, at[2] + 0.3])
        }
        "setYRot" | "setYHeadRot" | "setYBodyRot" => {
            rt.put_field(p, "$yaw", V::F(a(1).f()));
            V::Void
        }
        "setXRot" => {
            rt.put_field(p, "$pitch", V::F(a(1).f()));
            V::Void
        }
        "getYRot" | "getYHeadRot" | "getViewYRot" | "getVisualRotationYInDegrees" => V::F(yaw),
        "getXRot" | "getViewXRot" => V::F(pitch),
        "getLookAngle" | "getViewVector" | "getForward" => crate::api_common::view_vector(rt, yaw, pitch),
        "getDirection" | "getMotionDirection" => crate::api_common::direction(rt, crate::api_common::horizontal_facing(yaw)),
        // What it holds.
        "getInventory" => inv,
        "getMainHandItem" | "getWeaponItem" => {
            let i = selected(rt, inv);
            slot(rt, inv, i)
        }
        "getOffhandItem" => slot(rt, inv, OFF_HAND),
        "getItemInHand" => {
            let i = hand_slot(rt, inv, a(1));
            slot(rt, inv, i)
        }
        "setItemInHand" => {
            let i = hand_slot(rt, inv, a(1));
            set_slot(rt, inv, i, a(2));
            V::Void
        }
        "getItemBySlot" => {
            let i = equipment_slot(rt, inv, a(1));
            slot(rt, inv, i)
        }
        "setItemSlot" => {
            let i = equipment_slot(rt, inv, a(1));
            set_slot(rt, inv, i, a(2));
            V::Void
        }
        "addItem" => crate::api::call(rt, ADD, &[inv, a(1)]),
        // What it is doing: nothing a player's client would show.
        "isShiftKeyDown" | "isCrouching" | "isSteppingCarefully" | "isSecondaryUseActive" => V::I(rt.get_field(p, "$sneaking").i()),
        "setShiftKeyDown" => {
            rt.put_field(p, "$sneaking", V::I(a(1).i()));
            V::Void
        }
        "isCreative" | "isSpectator" | "isLocalPlayer" | "isSleeping" | "isPassenger" | "isFallFlying" | "isSwimming" | "isUsingItem" | "isInWater" | "isOnFire" => V::I(0),
        "mayBuild" => V::I(1),
        "isAlive" => V::I(i32::from(rt.get_field(p, "$removed").i() == 0)),
        "isRemoved" => V::I(rt.get_field(p, "$removed").i()),
        "discard" | "remove" | "kill" => {
            rt.put_field(p, "$removed", V::I(1));
            V::Void
        }
        "getHealth" | "getMaxHealth" => V::F(20.0),
        "getAttackStrengthScale" => V::F(1.0),
        "getAbilities" => {
            let abilities = rt.new_object("net/minecraft/world/entity/player/Abilities");
            for f in ["instabuild", "mayfly", "invulnerable", "flying"] {
                rt.put_field(abilities, f, V::I(0));
            }
            rt.put_field(abilities, "mayBuild", V::I(1));
            rt.put_field(abilities, "flyingSpeed", V::F(0.05));
            rt.put_field(abilities, "walkingSpeed", V::F(0.1));
            abilities
        }
        "getCooldowns" => {
            let mut c = rt.get_field(p, "$cooldowns");
            if c.r() == 0 {
                c = rt.new_object("net/minecraft/world/item/ItemCooldowns");
                rt.put_field(p, "$cooldowns", c);
            }
            c
        }
        // `FakePlayer`'s own: no messages, stats, sounds or menus reach anyone; nothing hurts it, it
        // hurts no player and rides nothing.
        "displayClientMessage" | "sendSystemMessage" | "awardStat" | "resetStat" | "playNotifySound" | "swing" | "resetAttackStrengthTicker" | "causeFoodExhaustion" | "closeContainer" | "doCloseContainer" | "tick" | "doTick" | "die" | "updateOptions" | "openHorseInventory" => V::Void,
        "isFakePlayer" | "isInvulnerableTo" => V::I(1),
        // The server it acts in (`ServerLifecycleHooks.getCurrentServer()`).
        "getServer" => rt.intern("compat:server".into(), |rt| rt.new_object("net/minecraft/server/MinecraftServer")),
        "canHarmPlayer" | "startRiding" => V::I(0),
        "openMenu" => {
            let none = rt.new_object("java/util/OptionalInt");
            rt.put_field(none, "$present", V::I(0));
            none
        }
        "drop" => crate::menus::fake_drop(rt, p, a(1), desc.starts_with("(Lnet/minecraft/world/item/ItemStack;ZZ)") && a(2).i() != 0),
        _ => return None,
    })
}

/// Where fake player `p` stands and looks, for what it throws.
pub(crate) fn stand(rt: &Rt, p: V) -> ([f64; 3], f32, f32, f32) {
    (place(rt, p), rt.get_field(p, "$yaw").f(), rt.get_field(p, "$pitch").f(), eye_height(rt, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// A mod's own fake player (Create's `DeployerFakePlayer`), calling through `FakePlayer` what
    /// it inherits.
    const MOD_PLAYER: &str = "test/DeployerPlayer";
    const INIT: &str = "net/neoforged/neoforge/common/util/FakePlayer.<init>.(Lnet/minecraft/server/level/ServerLevel;Lcom/mojang/authlib/GameProfile;)V";
    const PROFILE_INIT: &str = "com/mojang/authlib/GameProfile.<init>.(Ljava/util/UUID;Ljava/lang/String;)V";
    const SET_IN_HAND: &str = "net/neoforged/neoforge/common/util/FakePlayer.setItemInHand.(Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;)V";
    const MAIN_HAND: &str = "net/neoforged/neoforge/common/util/FakePlayer.getMainHandItem.()Lnet/minecraft/world/item/ItemStack;";
    const OFF_HAND_ITEM: &str = "net/minecraft/world/entity/LivingEntity.getOffhandItem.()Lnet/minecraft/world/item/ItemStack;";
    const PLAYER_MAIN_HAND: &str = "net/minecraft/world/entity/player/Player.getMainHandItem.()Lnet/minecraft/world/item/ItemStack;";
    const GET_INVENTORY: &str = "net/neoforged/neoforge/common/util/FakePlayer.getInventory.()Lnet/minecraft/world/entity/player/Inventory;";
    const INV_GET: &str = "net/minecraft/world/entity/player/Inventory.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
    const INV_SAVE: &str = "net/minecraft/world/entity/player/Inventory.save.(Lnet/minecraft/nbt/ListTag;)Lnet/minecraft/nbt/ListTag;";
    const INV_LOAD: &str = "net/minecraft/world/entity/player/Inventory.load.(Lnet/minecraft/nbt/ListTag;)V";
    const SET_POS: &str = "net/neoforged/neoforge/common/util/FakePlayer.setPos.(DDD)V";
    const SET_Y_ROT: &str = "net/neoforged/neoforge/common/util/FakePlayer.setYRot.(F)V";
    const GET_X: &str = "net/minecraft/world/entity/Entity.getX.()D";
    const BLOCK_POSITION: &str = "net/minecraft/world/entity/Entity.blockPosition.()Lnet/minecraft/core/BlockPos;";
    const DIRECTION: &str = "net/minecraft/world/entity/Entity.getDirection.()Lnet/minecraft/core/Direction;";
    const GET_UUID: &str = "net/neoforged/neoforge/common/util/FakePlayer.getUUID.()Ljava/util/UUID;";
    const GET_ID: &str = "net/neoforged/neoforge/common/util/FakePlayer.getId.()I";
    const IS_FAKE: &str = "net/neoforged/neoforge/common/util/FakePlayer.isFakePlayer.()Z";

    fn runtime() -> crate::Rt {
        crate::install();
        let mut rt = crate::Rt::new();
        rt.supers.insert(MOD_PLAYER, FAKE_PLAYER);
        rt
    }

    /// A deployer's fake player, as its constructor makes it: `super(level, profile)`.
    fn made(rt: &mut Rt, msb: i64) -> V {
        let p = rt.new_object(MOD_PLAYER);
        let profile = rt.new_object(GAME_PROFILE);
        let uuid = crate::tags::uuid(rt, msb, 7);
        let name = rt.string("Deployer");
        crate::api::call(rt, PROFILE_INIT, &[profile, uuid, name]);
        crate::api::call(rt, INIT, &[p, V::NULL, profile]);
        p
    }

    fn item(rt: &Rt, s: V) -> (String, i32) {
        (crate::api_common::item_like_name(rt, rt.get_field(s, "$item")), rt.get_field(s, "$count").i())
    }

    fn hand(rt: &mut Rt, which: &str) -> V {
        crate::api_common::get_static(rt, "net/minecraft/world/InteractionHand", which).unwrap()
    }

    /// What a fake player holds is its own: the acting player's hands and inventory stay as they
    /// were, and the acting player's calls still reach them.
    #[test]
    fn a_fake_players_hands_and_inventory_are_its_own() {
        let mut rt = runtime();
        let mut h = MemoryHost { inventory: true, held: Some(("minecraft:paper".into(), 1)), ..Default::default() };
        rt.with_host(&mut h, |rt| {
            let p = made(rt, 1);
            for class in ["net/minecraft/server/level/ServerPlayer", "net/minecraft/world/entity/player/Player", "net/minecraft/world/entity/LivingEntity", "net/minecraft/world/entity/Entity"] {
                assert!(rt.instance_of(p, class), "a fake player is a {class}");
            }
            assert_eq!(crate::api::call(rt, IS_FAKE, &[p]).i(), 1);
            let main = hand(rt, "MAIN_HAND");
            let off = hand(rt, "OFF_HAND");
            let iron = crate::api_common::stack(rt, "minecraft:iron_ingot", 3);
            crate::api::call(rt, SET_IN_HAND, &[p, main, iron]);
            let shield = crate::api_common::stack(rt, "minecraft:shield", 1);
            crate::api::call(rt, SET_IN_HAND, &[p, off, shield]);
            let held = crate::api::call(rt, MAIN_HAND, &[p]);
            assert_eq!(item(rt, held), ("minecraft:iron_ingot".into(), 3));
            let offhand = crate::api::call(rt, OFF_HAND_ITEM, &[p]);
            assert_eq!(item(rt, offhand), ("minecraft:shield".into(), 1));
            // Its inventory: the main hand is hotbar slot 0, the off hand 40.
            let inv = crate::api::call(rt, GET_INVENTORY, &[p]);
            let first = crate::api::call(rt, INV_GET, &[inv, V::I(0)]);
            assert_eq!(item(rt, first), ("minecraft:iron_ingot".into(), 3));
            // More iron joins the stack in hand; dirt takes the next free slot.
            let more = crate::api_common::stack(rt, "minecraft:iron_ingot", 2);
            assert_eq!(crate::api::call(rt, ADD, &[inv, more]).i(), 1);
            let dirt = crate::api_common::stack(rt, "minecraft:dirt", 4);
            assert_eq!(crate::api::call(rt, ADD, &[inv, dirt]).i(), 1);
            let first = crate::api::call(rt, INV_GET, &[inv, V::I(0)]);
            assert_eq!(item(rt, first), ("minecraft:iron_ingot".into(), 5));
            let second = crate::api::call(rt, INV_GET, &[inv, V::I(1)]);
            assert_eq!(item(rt, second), ("minecraft:dirt".into(), 4));
            // The acting player's hand is untouched.
            let player = crate::menus::server_player(rt, 9);
            let theirs = crate::api::call(rt, PLAYER_MAIN_HAND, &[player]);
            assert_eq!(item(rt, theirs), ("minecraft:paper".into(), 1));
        });
        assert_eq!(h.held, Some(("minecraft:paper".into(), 1)));
        assert!(h.slots.is_empty() && h.offhand.is_none() && h.given.is_empty(), "nothing reached the acting player");
    }

    /// A fake player stands and looks where it is put; its identity is its profile's; the id it
    /// has is no entity's of the game.
    #[test]
    fn a_fake_player_stands_where_it_is_put() {
        let mut rt = runtime();
        let mut h = MemoryHost { place: Some(([100.5, 70.0, 100.5], 0.0, 0.0)), ..Default::default() };
        rt.with_host(&mut h, |rt| {
            let p = made(rt, 0x55);
            crate::api::call(rt, SET_POS, &[p, V::D(1.5), V::D(64.0), V::D(-2.5)]);
            crate::api::call(rt, SET_Y_ROT, &[p, V::F(90.0)]);
            assert_eq!(crate::api::call(rt, GET_X, &[p]).d(), 1.5);
            let at = crate::api::call(rt, BLOCK_POSITION, &[p]);
            assert_eq!(crate::api::pos_of(rt, at), [1, 64, -3]);
            // Yaw 90 looks west (`Direction.fromYRot`).
            let facing = crate::api::call(rt, DIRECTION, &[p]);
            assert_eq!(crate::api_common::direction_ordinal(rt, facing), 4);
            let uuid = crate::api::call(rt, GET_UUID, &[p]);
            assert_eq!(crate::tags::uuid_bits(rt, uuid), (0x55, 7));
            assert!(crate::api::call(rt, GET_ID, &[p]).i() < 0);
            let other = made(rt, 0x56);
            assert_ne!(crate::api::call(rt, GET_ID, &[other]).i(), crate::api::call(rt, GET_ID, &[p]).i());
        });
    }

    /// What a block entity keeps of its fake player's inventory (`Inventory.save`) comes back into
    /// another's (`load`), each stack in its slot: the hand's, the armor's and the off hand's.
    #[test]
    fn a_fake_players_inventory_is_saved_and_loaded() {
        let mut rt = runtime();
        let mut h = MemoryHost::default();
        rt.with_host(&mut h, |rt| {
            let p = made(rt, 1);
            let inv = crate::api::call(rt, GET_INVENTORY, &[p]);
            for (slot, name, n) in [(0, "minecraft:iron_ingot", 3), (36, "minecraft:iron_boots", 1), (40, "minecraft:shield", 1)] {
                let s = crate::api_common::stack(rt, name, n);
                crate::api::call(rt, "net/minecraft/world/entity/player/Inventory.setItem.(ILnet/minecraft/world/item/ItemStack;)V", &[inv, V::I(slot), s]);
            }
            let list = crate::tags::new_list(rt, Vec::new());
            let saved = crate::api::call(rt, INV_SAVE, &[inv, list]);
            let slots: Vec<i32> = crate::jutil::elements(rt, saved)
                .into_iter()
                .map(|tag| {
                    let key = rt.string("Slot");
                    crate::api::call(rt, "net/minecraft/nbt/CompoundTag.getByte.(Ljava/lang/String;)B", &[tag, key]).i() & 255
                })
                .collect();
            assert_eq!(slots, [0, 100, 150], "vanilla's slot numbers");
            let q = made(rt, 2);
            let other = crate::api::call(rt, GET_INVENTORY, &[q]);
            crate::api::call(rt, INV_LOAD, &[other, saved]);
            for (slot, name, n) in [(0, "minecraft:iron_ingot", 3), (36, "minecraft:iron_boots", 1), (40, "minecraft:shield", 1)] {
                let s = crate::api::call(rt, INV_GET, &[other, V::I(slot)]);
                assert_eq!(item(rt, s), (name.into(), n), "slot {slot}");
            }
            let held = crate::api::call(rt, MAIN_HAND, &[q]);
            assert_eq!(item(rt, held), ("minecraft:iron_ingot".into(), 3));
        });
    }
}
