//! A mod's item used over time, as 1.21.1's `LivingEntity` runs it. The item's `use` calls
//! `startUsingItem(hand)` (a charge to throw, a polish, a drink of its own); the use then counts the
//! item's `getUseDuration` down, calling its `onUseTick` each tick, and ends in `finishUsingItem` when
//! the time runs out or `releaseUsing` when the button is let go (`updatingUsingItem`,
//! `completeUsingItem`, `releaseUsingItem`).
//!
//! The game counts every use down itself (`game/item_use.rs`); its hooks ask the mod's parts here:
//! [`use_duration`] and [`use_animation`] (the hand's pose), [`use_may_start`] (whether the click's
//! `use` started the use), [`use_tick`], [`finish_using_item`] and [`release_using`]. Vanilla's own
//! part of `Item.finishUsingItem`, eating a food, is the game's: a mod's override that calls it gets
//! the stack the game left.
use crate::host::Host;
use crate::{api, api_common, glue, hand, Rt, V};

const ITEM: &str = "net/minecraft/world/item/Item";
pub(crate) const USE_DURATION: &str = "getUseDuration.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/LivingEntity;)I";
pub(crate) const USE_DURATION_KEY: &str = "net/minecraft/world/item/Item.getUseDuration.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/LivingEntity;)I";
pub(crate) const USE_ANIMATION: &str = "getUseAnimation.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/UseAnim;";
pub(crate) const USE_ANIMATION_KEY: &str = "net/minecraft/world/item/Item.getUseAnimation.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/UseAnim;";
const FINISH: &str = "finishUsingItem.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;)Lnet/minecraft/world/item/ItemStack;";
const FINISH_KEY: &str = "net/minecraft/world/item/Item.finishUsingItem.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;)Lnet/minecraft/world/item/ItemStack;";
const RELEASE: &str = "releaseUsing.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;I)V";
const RELEASE_KEY: &str = "net/minecraft/world/item/Item.releaseUsing.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;I)V";
const USE_TICK: &str = "onUseTick.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;I)V";
const USE_TICK_KEY: &str = "net/minecraft/world/item/Item.onUseTick.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;I)V";
const USE_ANIM: &str = "net/minecraft/world/item/UseAnim";
const EQUIPMENT_SLOT: &str = "net/minecraft/world/entity/EquipmentSlot";
const GET_ITEM_IN_HAND: &str = "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;";
const START_USING: &str = "net/minecraft/world/entity/player/Player.startUsingItem.(Lnet/minecraft/world/InteractionHand;)V";

const USE_ON: &str = "useOn.(Lnet/minecraft/world/item/context/UseOnContext;)Lnet/minecraft/world/InteractionResult;";
const USE_ON_KEY: &str = "net/minecraft/world/item/Item.useOn.(Lnet/minecraft/world/item/context/UseOnContext;)Lnet/minecraft/world/InteractionResult;";
const USE_FIRST: &str = "onItemUseFirst.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/context/UseOnContext;)Lnet/minecraft/world/InteractionResult;";
const USE_FIRST_KEY: &str = "net/minecraft/world/item/Item.onItemUseFirst.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/context/UseOnContext;)Lnet/minecraft/world/InteractionResult;";
const SNEAK_BYPASS: &str = "doesSneakBypassUse.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const SNEAK_BYPASS_KEY: &str = "net/minecraft/world/item/Item.doesSneakBypassUse.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const SNEAKING: &str = "net/minecraft/world/entity/player/Player.isSecondaryUseActive.()Z";
const RESULT: &str = "net/minecraft/world/InteractionResult";

/// The mod's own override of `nd` on the class of item object `item`, if it has one.
fn own_override(rt: &Rt, item: V, nd: &str) -> bool {
    let class = rt.class_of(item);
    rt.is_mod_class(class) && rt.overrides.is_some_and(|o| o(class, nd))
}

/// `ItemStack.useOn(context)` (a click with the stack on a block): the item's `useOn`, a mod
/// item's own where its class has one. A block item's is `BlockItem.place`, which the game does
/// for whoever the context names (a fake player: Create's deployer) through
/// `Host::place_item_block`: placed, the stack loses one (none in a creative player's hand) and the
/// use consumes the action (`CONSUME` on a server); refused, it fails. Other vanilla items' uses on a
/// block (bone meal, a hoe, a bucket) aren't modelled here yet: they pass.
pub(crate) fn stack_use_on(rt: &mut Rt, stack: V, ctx: V) -> V {
    let item = rt.get_field(stack, "$item");
    if own_override(rt, item, USE_ON) {
        return rt.call_functional(USE_ON_KEY, item, &[ctx]);
    }
    let name = api_common::item_like_name(rt, item);
    if !rt.host().is_some_and(|h| h.item_is_block(&name)) {
        return api_common::result(rt, RESULT, "PASS");
    }
    block_item_use_on(rt, stack, ctx)
}

/// `BlockItem.useOn(context)`: `place`, as [`stack_use_on`] says.
pub(crate) fn block_item_use_on(rt: &mut Rt, stack: V, ctx: V) -> V {
    if api_common::stack_empty(rt, stack) {
        return api_common::result(rt, RESULT, "FAIL");
    }
    let player = rt.get_field(ctx, "$player");
    let h = rt.get_field(ctx, "$hitResult");
    let pos = api::pos_of(rt, rt.get_field(h, "$pos"));
    let face = api_common::direction_ordinal(rt, rt.get_field(h, "$dir"));
    let hit = api_common::xyz(rt, rt.get_field(h, "$loc"));
    let inside = rt.get_field(h, "$inside").i() != 0;
    let look = (rt.get_field(player, "$yaw").f(), rt.get_field(player, "$pitch").f());
    let sneaking = player.r() != 0 && api::call(rt, SNEAKING, &[player]).i() != 0;
    let Some(held) = hand::hand_stack_of(rt, stack) else {
        return api_common::result(rt, RESULT, "PASS");
    };
    let placed = rt.host_mut().and_then(|h| h.place_item_block(&held, pos, face, hit, inside, look, sneaking));
    if placed.is_none() {
        return api_common::result(rt, RESULT, "FAIL");
    }
    let creative = !crate::fake_players::is_fake(rt, player) && player.r() != 0 && rt.host().is_some_and(|h| h.player_creative());
    if !creative {
        let n = rt.get_field(stack, "$count").i();
        rt.put_field(stack, "$count", V::I(n - 1));
    }
    let client = crate::neo::is_client(rt);
    api_common::result(rt, RESULT, if client { "SUCCESS" } else { "CONSUME" })
}

/// `ItemStack.onItemUseFirst(context)`, NeoForge's: the item's own (a mod's override), else
/// `IItemExtension`'s, which passes.
pub(crate) fn stack_use_first(rt: &mut Rt, stack: V, ctx: V) -> V {
    let item = rt.get_field(stack, "$item");
    if own_override(rt, item, USE_FIRST) {
        return rt.call_functional(USE_FIRST_KEY, item, &[stack, ctx]);
    }
    api_common::result(rt, RESULT, "PASS")
}

/// `ItemStack.doesSneakBypassUse(level, pos, player)`, NeoForge's: whether a sneaking player's click
/// still uses the block: the item's own answer, else `IItemExtension`'s, no.
pub(crate) fn sneak_bypasses(rt: &mut Rt, stack: V, level: V, pos: V, player: V) -> V {
    let item = rt.get_field(stack, "$item");
    if own_override(rt, item, SNEAK_BYPASS) {
        return rt.call_functional(SNEAK_BYPASS_KEY, item, &[stack, level, pos, player]);
    }
    V::I(0)
}

/// The mod's registered item `item` and its class, when the class has its own `nd`.
fn own(rt: &Rt, item: &str, nd: &str) -> Option<V> {
    let &it = rt.registered.get(&("item".to_owned(), item.to_owned()))?;
    let class = rt.class_of(it);
    rt.overrides.is_some_and(|o| o(class, nd)).then_some(it)
}

/// What the last click on this thread left: the item whose `use` ran, and whether it called
/// `startUsingItem`.
fn marker(rt: &mut Rt) -> V {
    rt.intern("compat:use-started".into(), |rt| rt.new_object("compat:UseStarted"))
}

/// A click's `use` of `item` is about to run: no use started yet.
pub(crate) fn use_begins(rt: &mut Rt, item: &str) {
    let m = marker(rt);
    let s = rt.string(item);
    rt.put_field(m, "$item", s);
    rt.put_field(m, "$started", V::I(0));
}

/// Whether the click's `use` called `startUsingItem`.
pub(crate) fn use_started(rt: &mut Rt) -> bool {
    let m = marker(rt);
    rt.get_field(m, "$started").i() != 0
}

/// Whether the mod's item `it` is food (`Item.Properties.food`).
fn has_food(rt: &Rt, it: V) -> bool {
    let props = rt.get_field(it, "$properties");
    props.r() != 0 && rt.get_field(props, "$food").r() != 0
}

/// `Item.getUseDuration(stack, entity)` of the mod's `item`, in ticks, when its class answers it
/// (asked with a stack of one: an item's answer seldom depends on the stack). None: the game's own
/// stands (a food's eating time, 0).
pub fn use_duration(rt: &mut Rt, item: &str) -> Option<i32> {
    rt.safe_point();
    let it = own(rt, item, USE_DURATION)?;
    let vc = rt.vcall?;
    let stack = api_common::stack(rt, item, 1);
    let player = api_common::acting_player(rt);
    let r = vc(rt, USE_DURATION_KEY, USE_DURATION, &[it, stack, player]);
    let thrown = rt.thrown.is_some();
    rt.report_thrown("getUseDuration");
    (!thrown).then(|| r.i())
}

/// `Item.getUseAnimation(stack)` of the mod's `item`, by 26.3's names (`bow`, `drink`; 1.21.1's
/// `spear` is `trident`, NeoForge's `custom` none), when its class answers it.
pub fn use_animation(rt: &mut Rt, item: &str) -> Option<String> {
    rt.safe_point();
    let it = own(rt, item, USE_ANIMATION)?;
    let vc = rt.vcall?;
    let stack = api_common::stack(rt, item, 1);
    let r = vc(rt, USE_ANIMATION_KEY, USE_ANIMATION, &[it, stack]);
    rt.report_thrown("getUseAnimation");
    let name = rt.str_of(rt.get_field(r, "$name"))?.to_ascii_lowercase();
    Some(match name.as_str() {
        "spear" => "trident".into(),
        "custom" => "none".into(),
        _ => name,
    })
}

/// Whether the use over time the game starts now with the mod's `item` goes on: true when the
/// click's `use` called `startUsingItem`; false when it didn't, or the item has no `use` of its own
/// (vanilla's `Item.use` starts only eating). None when its class has no `getUseDuration` of its own,
/// or it is a food: the game decides whether it is eaten.
pub fn use_may_start(rt: &mut Rt, item: &str) -> Option<bool> {
    let it = own(rt, item, USE_DURATION)?;
    let m = marker(rt);
    let started = rt.str_of(rt.get_field(m, "$item")) == Some(item) && rt.get_field(m, "$started").i() != 0;
    rt.put_field(m, "$item", V::NULL);
    if started {
        return Some(true);
    }
    (!has_food(rt, it)).then_some(false)
}

/// The player a use's hook acts for, using `hand` with `left` ticks left.
fn user(rt: &mut Rt, left: i32, hand: u8) -> V {
    let p = api_common::acting_player(rt);
    let hand = hand::constant(rt, hand);
    rt.put_field(p, "$usingHand", hand);
    rt.put_field(p, "$useLeft", V::I(left));
    p
}

/// What `stack` holds: its item and count, None when empty.
fn contents(rt: &Rt, stack: V) -> Option<(String, u32)> {
    (!api_common::stack_empty(rt, stack)).then(|| {
        let id = api_common::item_like_name(rt, rt.get_field(stack, "$item"));
        (id, rt.get_field(stack, "$count").i().max(0) as u32)
    })
}

/// `Item.finishUsingItem(stack, level, entity)` of the mod's `item` once its use ran its time, after
/// the game's own finish (a food eaten, its remainder given). The mod gets the stack in hand when
/// the hand still holds the item, else an emptied stack of it (the game ate the last one). What it
/// did to the hand reaches the game's; a stack it answered goes in the hand, as `completeUsingItem`
/// puts it there (an empty answer leaves what the game left). `hand` is the hand that used it. False
/// when its class has none.
pub fn finish_using_item(rt: &mut Rt, host: &mut dyn Host, item: &str, hand: u8) -> bool {
    rt.safe_point();
    let Some(it) = own(rt, item, FINISH) else { return false };
    rt.with_host(host, |rt| {
        hand::with_hand(rt, |rt| {
            let Some(vc) = rt.vcall else { return false };
            let held = hand::stack(rt, hand);
            let in_hand = contents(rt, held).is_some_and(|(h, _)| h == item);
            let stack = if in_hand { held } else { api_common::stack(rt, item, 0) };
            let level = glue::level(rt);
            let player = user(rt, 0, hand);
            let r = vc(rt, FINISH_KEY, FINISH, &[it, stack, level, player]);
            let thrown = rt.thrown.is_some();
            rt.report_thrown("finishUsingItem");
            if thrown {
                return true;
            }
            if r.r() != 0 && r != held && (in_hand || !api_common::stack_empty(rt, r)) {
                hand::replace(rt, hand, r);
            }
            true
        })
    })
}

/// `Item.releaseUsing(stack, level, entity, timeLeft)` of the mod's `item`: the button let go with
/// `time_left` ticks of its use left. What it does to the stack in hand (a thrown package leaves it)
/// reaches the hand. `hand` is the hand that used it. False when its class has none, or that hand
/// doesn't hold the item.
pub fn release_using(rt: &mut Rt, host: &mut dyn Host, item: &str, time_left: i32, hand: u8) -> bool {
    rt.safe_point();
    let Some(it) = own(rt, item, RELEASE) else { return false };
    rt.with_host(host, |rt| {
        hand::with_hand(rt, |rt| {
            let Some(vc) = rt.vcall else { return false };
            let player = user(rt, time_left, hand);
            let Some(stack) = held_stack(rt, item, hand) else { return false };
            let level = glue::level(rt);
            vc(rt, RELEASE_KEY, RELEASE, &[it, stack, level, player, V::I(time_left)]);
            rt.report_thrown("releaseUsing");
            true
        })
    })
}

/// `Item.onUseTick(level, entity, stack, remaining)` of the mod's `item`, each tick it is used with
/// `remaining` ticks left. As [`release_using`].
pub fn use_tick(rt: &mut Rt, host: &mut dyn Host, item: &str, remaining: i32, hand: u8) -> bool {
    rt.safe_point();
    let Some(it) = own(rt, item, USE_TICK) else { return false };
    rt.with_host(host, |rt| {
        hand::with_hand(rt, |rt| {
            let Some(vc) = rt.vcall else { return false };
            let player = user(rt, remaining, hand);
            let Some(stack) = held_stack(rt, item, hand) else { return false };
            let level = glue::level(rt);
            vc(rt, USE_TICK_KEY, USE_TICK, &[it, level, player, stack, V::I(remaining)]);
            rt.report_thrown("onUseTick");
            true
        })
    })
}

/// The stack in `hand`, when it is `item`: the one object the hook's player answers.
fn held_stack(rt: &mut Rt, item: &str, hand: u8) -> Option<V> {
    let stack = hand::stack(rt, hand);
    contents(rt, stack).filter(|(h, _)| h == item).map(|_| stack)
}

/// The hand a player object uses, when it was told one (`startUsingItem`, a use's hook).
fn using_hand(rt: &Rt, player: V) -> Option<V> {
    match rt.get_field(player, "$usingHand") {
        V::R(r) if r != 0 => Some(V::R(r)),
        _ => None,
    }
}

/// The food an item stack's item gives (its `Item.Properties.food`), when it is a mod's food.
fn stack_food(rt: &Rt, stack: V) -> Option<V> {
    let item = rt.get_field(stack, "$item");
    let props = (item.r() != 0).then(|| rt.get_field(item, "$properties"))?;
    let food = (props.r() != 0).then(|| rt.get_field(props, "$food"))?;
    (food.r() != 0).then_some(food)
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    let using = matches!(
        name,
        "startUsingItem" | "isUsingItem" | "getUsedItemHand" | "getUseItem" | "getUseItemRemainingTicks" | "getTicksUsingItem" | "stopUsingItem" | "releaseUsingItem" | "getSlotForHand"
    );
    (using && api_common::is_player(owner)) || matches!(owner, "net/minecraft/world/item/ItemUtils" | ITEM)
}

/// Using items over time: a player's use (`startUsingItem` and what tells of it), `ItemUtils`'
/// instant start, and vanilla `Item`'s own parts a mod's override calls (`super`).
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, _desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let player = api_common::is_player(owner);
    Some(match (owner, name) {
        // The use starts where the game counts it: the hook that runs the click's `use` sees it.
        (_, "startUsingItem") if player => {
            rt.put_field(a(0), "$usingHand", a(1));
            let m = marker(rt);
            rt.put_field(m, "$started", V::I(1));
            V::Void
        }
        (_, "isUsingItem") if player => V::I(i32::from(using_hand(rt, a(0)).is_some())),
        (_, "getUsedItemHand") if player => match using_hand(rt, a(0)) {
            Some(h) => h,
            None => api::get_static(rt, "net/minecraft/world/InteractionHand.MAIN_HAND"),
        },
        (_, "getUseItem") if player => match using_hand(rt, a(0)) {
            Some(h) => api::call(rt, GET_ITEM_IN_HAND, &[a(0), h]),
            None => api::get_static(rt, "net/minecraft/world/item/ItemStack.EMPTY"),
        },
        (_, "getUseItemRemainingTicks") if player => V::I(if using_hand(rt, a(0)).is_some() { rt.get_field(a(0), "$useLeft").i() } else { 0 }),
        (_, "getTicksUsingItem") if player => V::I(rt.get_field(a(0), "$useTicks").i()),
        (_, "stopUsingItem" | "releaseUsingItem") if player => {
            rt.put_field(a(0), "$usingHand", V::NULL);
            V::Void
        }
        ("net/minecraft/world/entity/LivingEntity" | "net/minecraft/world/entity/player/Player", "getSlotForHand") => {
            let off = rt.get_field(a(0), "$ordinal").i() == 1;
            api::api_enum(rt, EQUIPMENT_SLOT, if off { "OFFHAND" } else { "MAINHAND" }, i32::from(off))
        }
        // `ItemUtils.startUsingInstantly(level, player, hand)`: the use starts, the stack answered.
        ("net/minecraft/world/item/ItemUtils", "startUsingInstantly") => {
            api::call(rt, START_USING, &[a(1), a(2)]);
            let stack = api::call(rt, GET_ITEM_IN_HAND, &[a(1), a(2)]);
            let consume = api::call(rt, "net/minecraft/world/InteractionResultHolder.consume.(Ljava/lang/Object;)Lnet/minecraft/world/InteractionResultHolder;", &[stack]);
            consume
        }
        // Vanilla's `finishUsingItem` eats a food: the game did, before the mod's override ran.
        (ITEM, "finishUsingItem") => a(1),
        (ITEM, "releaseUsing" | "onUseTick" | "onStopUsing") => V::Void,
        // `FoodProperties.eatDurationTicks()` of a food, 0 for the rest; a food eats, the rest none.
        (ITEM, "getUseDuration") => V::I(stack_food(rt, a(1)).map_or(0, |f| (rt.get_field(f, "eatSeconds").f() * 20.0) as i32)),
        (ITEM, "getUseAnimation") => {
            let eat = stack_food(rt, a(1)).is_some();
            api::api_enum(rt, USE_ANIM, if eat { "EAT" } else { "NONE" }, i32::from(eat))
        }
        (ITEM, "getDefaultInstance") => {
            let id = api_common::item_like_name(rt, a(0));
            api_common::stack(rt, &id, 1)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const IS_USING: &str = "net/minecraft/world/entity/player/Player.isUsingItem.()Z";
    const USED_HAND: &str = "net/minecraft/world/entity/LivingEntity.getUsedItemHand.()Lnet/minecraft/world/InteractionHand;";
    const NEW_ORB: &str = "net/minecraft/world/entity/ExperienceOrb.<init>.(Lnet/minecraft/world/level/Level;DDDI)V";
    const ORB_MOTION: &str = "net/minecraft/world/entity/ExperienceOrb.setDeltaMovement.(Lnet/minecraft/world/phys/Vec3;)V";
    const ADD: &str = "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z";
    const AWARD: &str = "net/minecraft/world/entity/ExperienceOrb.award.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/phys/Vec3;I)V";
    const NEW_VEC3: &str = "net/minecraft/world/phys/Vec3.<init>.(DDD)V";

    fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
        let v = rt.new_object("net/minecraft/world/phys/Vec3");
        api::call(rt, NEW_VEC3, &[v, V::D(p[0]), V::D(p[1]), V::D(p[2])]);
        v
    }

    /// A player's `startUsingItem(OFF_HAND)` is the click's start of a use, and the player then
    /// says it uses that hand, as vanilla's `LivingEntity` does.
    #[test]
    fn starting_a_use_marks_the_click_and_the_player() {
        let mut rt = Rt::new();
        crate::install();
        use_begins(&mut rt, "create:sand_paper");
        let player = api_common::acting_player(&mut rt);
        assert_eq!(api::call(&mut rt, IS_USING, &[player]).i(), 0);
        let off = api::get_static(&mut rt, "net/minecraft/world/InteractionHand.OFF_HAND");
        api::call(&mut rt, START_USING, &[player, off]);
        assert!(use_started(&mut rt));
        assert_eq!(api::call(&mut rt, IS_USING, &[player]).i(), 1);
        assert_eq!(api::call(&mut rt, USED_HAND, &[player]).r(), off.r());
    }

    /// An orb a mod makes and adds is spawned where it was made, worth its points, moving as the mod
    /// set it; `award` splits points into vanilla's orb sizes (20 is 17 and 3).
    #[test]
    fn experience_orbs_a_mod_adds_spawn_through_the_host() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let level = glue::level(rt);
            let orb = rt.new_object("net/minecraft/world/entity/ExperienceOrb");
            api::call(rt, NEW_ORB, &[orb, level, V::D(1.5), V::D(64.0), V::D(-2.5), V::I(7)]);
            let m = vec3(rt, [0.1, 0.2, 0.0]);
            api::call(rt, ORB_MOTION, &[orb, m]);
            assert_eq!(api::call(rt, ADD, &[level, orb]).i(), 1);
            let at = vec3(rt, [0.0, 70.0, 0.0]);
            api::call(rt, AWARD, &[level, at, V::I(20)]);
        });
        assert_eq!(host.orbs, vec![([1.5, 64.0, -2.5], 7, Some([0.1, 0.2, 0.0])), ([0.0, 70.0, 0.0], 17, None), ([0.0, 70.0, 0.0], 3, None)]);
    }

    /// `displayClientMessage(literal("Shaper").withStyle(RED).append(translatable(key)), true)`
    /// reaches the player over the hotbar as vanilla's serializer writes the component.
    #[test]
    fn a_styled_message_reaches_the_player_as_vanilla_json() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let text = rt.string("Shaper");
            let c = api::call(rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[text]);
            let red = api::get_static(rt, "net/minecraft/ChatFormatting.RED");
            api::call(rt, "net/minecraft/network/chat/MutableComponent.withStyle.(Lnet/minecraft/ChatFormatting;)Lnet/minecraft/network/chat/MutableComponent;", &[c, red]);
            let key = rt.string("create.terrainzapper.shiftRightClickToSet");
            let t = api::call(rt, "net/minecraft/network/chat/Component.translatable.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[key]);
            api::call(rt, "net/minecraft/network/chat/MutableComponent.append.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/network/chat/MutableComponent;", &[c, t]);
            let player = api_common::acting_player(rt);
            api::call(rt, "net/minecraft/world/entity/player/Player.displayClientMessage.(Lnet/minecraft/network/chat/Component;Z)V", &[player, c, V::I(1)]);
        });
        let [(json, overlay)] = host.messages.as_slice() else { panic!("{:?}", host.messages) };
        assert!(overlay);
        let got: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(got, serde_json::json!({"text": "Shaper", "color": "red", "extra": [{"translate": "create.terrainzapper.shiftRightClickToSet"}]}));
    }
}
