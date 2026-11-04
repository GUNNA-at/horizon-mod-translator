//! The acting player's hands while a hook runs. Vanilla's player holds one stack object in each
//! hand: what a mod changes in it (`shrink`, a component `set`, wear), it finds there again, through
//! the player (`getItemInHand`, `getMainHandItem`, `getOffhandItem`) or a use's context
//! (`UseOnContext.getItemInHand`). Here each hand is one stack object for a whole hook
//! ([`with_hand`]), loaded from the game when the mod first reaches it (its damage, and its mod
//! components read with their types' codecs), and what the mod did to it reaches the game's stack
//! when the hook ends: its count, damage and components in place (vanilla's own other data on it
//! stays), or the stack the mod put in that hand instead (`setItemInHand`, a use's answer). Wear
//! (`hurtAndBreak`) on a stack in hand is the game's own, as it happens.
use crate::nbt::Nbt;
use crate::{api_common, components, Rt, V};
use std::collections::BTreeMap;

/// The main hand and the off hand, as `InteractionHand`'s ordinals.
pub const MAIN: u8 = 0;
pub const OFF: u8 = 1;

/// `hand` as an `InteractionHand` constant.
pub fn constant(rt: &mut Rt, hand: u8) -> V {
    let field = if hand == OFF { "OFF_HAND" } else { "MAIN_HAND" };
    api_common::get_static(rt, "net/minecraft/world/InteractionHand", field).unwrap_or(V::NULL)
}

fn record(rt: &mut Rt, hand: u8) -> V {
    let key = if hand == OFF { "compat:hand/off" } else { "compat:hand/main" };
    rt.intern(key.into(), |rt| rt.new_object("compat:Hand"))
}

/// How deep in hooks acting for a player this thread is.
fn depth_record(rt: &mut Rt) -> V {
    rt.intern("compat:hand/depth".into(), |rt| rt.new_object("compat:HandDepth"))
}

fn in_hook(rt: &mut Rt) -> bool {
    let d = depth_record(rt);
    rt.get_field(d, "$depth").i() > 0
}

/// Runs `f`, a hook acting for a player, with one stack per hand for all of it; what the mod did to
/// them reaches the game when the outermost such hook ends. Call inside `with_host`.
pub fn with_hand<R>(rt: &mut Rt, f: impl FnOnce(&mut Rt) -> R) -> R {
    let d = depth_record(rt);
    let depth = rt.get_field(d, "$depth").i();
    rt.put_field(d, "$depth", V::I(depth + 1));
    let out = f(rt);
    let d = depth_record(rt);
    rt.put_field(d, "$depth", V::I(depth));
    if depth == 0 {
        for hand in [MAIN, OFF] {
            flush(rt, hand);
            let r = record(rt, hand);
            rt.put_field(r, "$stack", V::NULL);
            rt.put_field(r, "$saved", V::NULL);
        }
    }
    out
}

fn empty(rt: &mut Rt) -> V {
    api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
}

/// The stack in `hand`: within a hook the same object each time (`ItemStack.EMPTY` for an empty
/// hand); outside one, a new one each time.
pub fn stack(rt: &mut Rt, hand: u8) -> V {
    let r = record(rt, hand);
    if let V::R(s) = rt.get_field(r, "$stack")
        && s != 0
    {
        return V::R(s);
    }
    let s = load(rt, hand);
    if in_hook(rt) {
        let r = record(rt, hand);
        rt.put_field(r, "$stack", s);
        rt.put_field(r, "$replaced", V::I(0));
        snapshot(rt, hand, s);
    }
    s
}

/// The hand `stack` is in during this hook, if it is one of them.
pub(crate) fn hand_of(rt: &mut Rt, stack: V) -> Option<u8> {
    if stack.r() == 0 || !in_hook(rt) {
        return None;
    }
    [MAIN, OFF].into_iter().find(|&h| {
        let r = record(rt, h);
        rt.get_field(r, "$stack") == stack
    })
}

/// The game's stack in `hand` as a stack here: its count, damage, and mod components.
fn load(rt: &mut Rt, hand: u8) -> V {
    let Some(held) = rt.host().and_then(|h| h.player_hand(hand)) else { return empty(rt) };
    let s = api_common::stack(rt, &held.item, held.count as i32);
    rt.put_field(s, "$damage", V::I(held.damage as i32));
    if held.max_damage > 0 {
        rt.put_field(s, "$maxDamage", V::I(held.max_damage as i32));
    }
    components::load(rt, s, &held.components);
    s
}

/// What `s` holds (item, count, damage; None when empty) and its components as the game keeps
/// them.
fn state(rt: &mut Rt, s: V) -> (Option<(String, u32, u32)>, BTreeMap<String, Nbt>) {
    if api_common::stack_empty(rt, s) {
        return (None, BTreeMap::new());
    }
    let id = api_common::item_like_name(rt, rt.get_field(s, "$item"));
    let count = rt.get_field(s, "$count").i().max(0) as u32;
    let damage = components::damage(rt, s).max(0) as u32;
    (Some((id, count, damage)), components::saved(rt, s).into_iter().collect())
}

/// Records what `hand` holds now as what the game holds: a codec that doesn't round-trip exactly
/// doesn't count as a change.
fn snapshot(rt: &mut Rt, hand: u8, s: V) {
    let (held, carried) = state(rt, s);
    let id = match &held {
        Some((id, ..)) => rt.string(id),
        None => V::NULL,
    };
    let saved = crate::nbt::to_runtime(rt, &carried);
    let r = record(rt, hand);
    rt.put_field(r, "$id", id);
    rt.put_field(r, "$count", V::I(held.as_ref().map_or(0, |h| h.1 as i32)));
    rt.put_field(r, "$damage", V::I(held.as_ref().map_or(0, |h| h.2 as i32)));
    rt.put_field(r, "$saved", saved);
}

/// The mod put `stack` in `hand` (`Player.setItemInHand`, a use answering another stack): within
/// a hook, the hand is that object from now on and the game gets it when the hook ends; outside
/// one, the game gets it now.
pub fn replace(rt: &mut Rt, hand: u8, stack: V) {
    if !in_hook(rt) {
        put(rt, hand, stack);
        return;
    }
    // What the hand held first stays the baseline.
    self::stack(rt, hand);
    let r = record(rt, hand);
    rt.put_field(r, "$stack", stack);
    rt.put_field(r, "$replaced", V::I(1));
}

/// The game's `hand` holds what `stack` holds now.
fn put(rt: &mut Rt, hand: u8, stack: V) {
    let (held, carried) = state(rt, stack);
    let held = held.map(|(item, count, damage)| crate::host::HandStack {
        item,
        count,
        damage,
        max_damage: 0,
        components: carried.into_iter().collect(),
    });
    if let Some(h) = rt.host_mut() {
        h.set_player_hand(hand, held.as_ref());
    }
}

/// What the mod did to `hand` so far reaches the game.
fn flush(rt: &mut Rt, hand: u8) {
    let r = record(rt, hand);
    let s = rt.get_field(r, "$stack");
    if s.r() == 0 {
        return; // never reached
    }
    if rt.get_field(r, "$replaced").i() != 0 {
        put(rt, hand, s);
        return;
    }
    let before_id = rt.str_of(rt.get_field(r, "$id")).map(str::to_owned);
    let before_count = rt.get_field(r, "$count").i().max(0) as u32;
    let before_damage = rt.get_field(r, "$damage").i().max(0) as u32;
    let saved = rt.get_field(r, "$saved");
    let before = if saved.r() == 0 { BTreeMap::new() } else { crate::nbt::from_runtime(rt, saved) };
    let (held, now) = state(rt, s);
    match (before_id, held) {
        (None, None) => {}
        // Used up: the hand is empty.
        (Some(_), None) => {
            if let Some(h) = rt.host_mut() {
                h.update_player_hand(hand, 0, 0, &[]);
            }
        }
        (Some(was), Some((id, n, damage))) if was == id => {
            let mut changes: Vec<(String, Option<Nbt>)> =
                before.keys().filter(|k| !now.contains_key(*k)).map(|k| (k.clone(), None)).collect();
            changes.extend(now.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), Some(v.clone()))));
            if (n != before_count || damage != before_damage || !changes.is_empty())
                && let Some(h) = rt.host_mut()
            {
                h.update_player_hand(hand, n, damage, &changes);
            }
        }
        // The hand's stack holds something else now.
        (_, Some(_)) => put(rt, hand, s),
    }
}

/// `stack.hurtAndBreak(amount, ...)`: a stack in hand is worn by the game's own rules (creative,
/// Unbreaking, breaking with its sound) and then holds what the game left; any other stack (or
/// with a host that can't) wears here: none for a creative player, and one worn out shrinks by one.
pub(crate) fn hurt(rt: &mut Rt, stack: V, amount: i32) {
    if amount <= 0 || api_common::stack_empty(rt, stack) {
        return;
    }
    if let Some(hand) = hand_of(rt, stack) {
        // The game wears what the mod left so far.
        flush(rt, hand);
        let r = record(rt, hand);
        rt.put_field(r, "$replaced", V::I(0));
        snapshot(rt, hand, stack);
        if rt.host_mut().is_some_and(|h| h.hurt_player_hand(hand, amount as u32)) {
            let now = rt.host().and_then(|h| h.player_hand(hand));
            let (count, damage) = now.map_or((0, 0), |s| (s.count as i32, s.damage as i32));
            rt.put_field(stack, "$count", V::I(count));
            rt.put_field(stack, "$damage", V::I(damage));
            snapshot(rt, hand, stack);
            return;
        }
    }
    if rt.host().is_some_and(|h| h.player_creative()) {
        return;
    }
    let max = components::max_damage(rt, stack);
    if max <= 0 {
        return;
    }
    let worn = components::damage(rt, stack) + amount;
    rt.put_field(stack, "$damage", V::I(worn));
    if worn >= max {
        let n = rt.get_field(stack, "$count").i();
        rt.put_field(stack, "$count", V::I((n - 1).max(0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::host::{HandStack, MemoryHost};

    const IN_HAND: &str = "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;";
    const OFF_HAND_ITEM: &str = "net/minecraft/world/entity/player/Player.getOffhandItem.()Lnet/minecraft/world/item/ItemStack;";
    const SET_IN_HAND: &str = "net/minecraft/world/entity/player/Player.setItemInHand.(Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;)V";
    const SET: &str = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
    const GET: &str = "net/minecraft/core/component/DataComponentHolder.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    const REMOVE: &str = "net/minecraft/world/item/ItemStack.remove.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    const SHRINK: &str = "net/minecraft/world/item/ItemStack.shrink.(I)V";
    const SPLIT: &str = "net/minecraft/world/item/ItemStack.split.(I)Lnet/minecraft/world/item/ItemStack;";
    const COPY: &str = "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;";
    const COUNT: &str = "net/minecraft/world/item/ItemStack.getCount.()I";
    const IS_EMPTY: &str = "net/minecraft/world/item/ItemStack.isEmpty.()Z";
    const HURT: &str = "net/minecraft/world/item/ItemStack.hurtAndBreak.(ILnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/entity/EquipmentSlot;)V";
    const DAMAGE: &str = "net/minecraft/world/item/ItemStack.getDamageValue.()I";
    const MAX_DAMAGE: &str = "net/minecraft/world/item/ItemStack.getMaxDamage.()I";
    const DAMAGED: &str = "net/minecraft/world/item/ItemStack.isDamaged.()Z";

    /// A mod's persistent component type of a block position, registered as Create registers its
    /// belt connector's first shaft.
    fn position_type(rt: &mut Rt, name: &str) -> V {
        let b = api::call(rt, "net/minecraft/core/component/DataComponentType.builder.()Lnet/minecraft/core/component/DataComponentType$Builder;", &[]);
        let codec = api::get_static(rt, "net/minecraft/core/BlockPos.CODEC");
        let b = api::call(rt, "net/minecraft/core/component/DataComponentType$Builder.persistent.(Lcom/mojang/serialization/Codec;)Lnet/minecraft/core/component/DataComponentType$Builder;", &[b, codec]);
        let ty = api::call(rt, "net/minecraft/core/component/DataComponentType$Builder.build.()Lnet/minecraft/core/component/DataComponentType;", &[b]);
        rt.registered.insert(("data_component_type".into(), name.into()), ty);
        ty
    }

    fn hand_constant(rt: &mut Rt, hand: u8) -> V {
        api::get_static(rt, if hand == OFF { "net/minecraft/world/InteractionHand.OFF_HAND" } else { "net/minecraft/world/InteractionHand.MAIN_HAND" })
    }

    fn mainhand_slot(rt: &mut Rt) -> V {
        api::get_static(rt, "net/minecraft/world/entity/EquipmentSlot.MAINHAND")
    }

    /// A component the mod sets on the stack in hand, and a shrink, reach the game's stack when the
    /// hook ends; the next hook reads the component back from the game, and a removal removes it.
    #[test]
    fn what_a_mod_does_to_the_stack_in_hand_reaches_the_game() {
        let mut rt = Rt::new();
        crate::install();
        let ty = position_type(&mut rt, "test:first_shaft");
        assert_eq!(components::persistent_types(&mut rt), vec!["test:first_shaft".to_owned()]);
        let mut host = MemoryHost::default();
        host.held = Some(("minecraft:stick".into(), 3));
        rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let player = api_common::acting_player(rt);
                let main = hand_constant(rt, MAIN);
                let s = api::call(rt, IN_HAND, &[player, main]);
                let other_player = api_common::acting_player(rt);
                assert_eq!(api::call(rt, IN_HAND, &[other_player, main]), s, "one stack object in hand through the hook");
                let pos = api::block_pos(rt, 4, 64, -2);
                api::call(rt, SET, &[s, ty, pos]);
                api::call(rt, SHRINK, &[s, V::I(1)]);
            })
        });
        assert_eq!(host.held, Some(("minecraft:stick".into(), 2)));
        assert_eq!(host.held_components, BTreeMap::from([("test:first_shaft".to_owned(), Nbt::IntArray(vec![4, 64, -2]))]));

        let read = rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let s = stack(rt, MAIN);
                let v = api::call(rt, GET, &[s, ty]);
                api::pos_of(rt, v)
            })
        });
        assert_eq!(read, [4, 64, -2]);
        assert_eq!(host.held, Some(("minecraft:stick".into(), 2)), "a hook that only reads changes nothing");

        rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let s = stack(rt, MAIN);
                api::call(rt, REMOVE, &[s, ty]);
            })
        });
        assert!(host.held_components.is_empty());
        assert_eq!(host.held, Some(("minecraft:stick".into(), 2)));
    }

    /// A stack the mod puts in the hand replaces what it held, its components with it; the old
    /// stack, changed after, changes nothing.
    #[test]
    fn a_stack_put_in_the_hand_replaces_it() {
        let mut rt = Rt::new();
        crate::install();
        let ty = position_type(&mut rt, "test:target");
        let mut host = MemoryHost::default();
        host.held = Some(("minecraft:stick".into(), 3));
        host.held_components.insert("test:target".into(), Nbt::IntArray(vec![1, 2, 3]));
        rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let player = api_common::acting_player(rt);
                let main = hand_constant(rt, MAIN);
                let old = api::call(rt, IN_HAND, &[player, main]);
                let bottle = api_common::stack(rt, "minecraft:glass_bottle", 1);
                let pos = api::block_pos(rt, 0, 70, 0);
                api::call(rt, SET, &[bottle, ty, pos]);
                api::call(rt, SET_IN_HAND, &[player, main, bottle]);
                api::call(rt, SHRINK, &[old, V::I(3)]);
                assert_eq!(api::call(rt, IN_HAND, &[player, main]), bottle);
            })
        });
        assert_eq!(host.held, Some(("minecraft:glass_bottle".into(), 1)));
        assert_eq!(host.held_components, BTreeMap::from([("test:target".to_owned(), Nbt::IntArray(vec![0, 70, 0]))]));
    }

    /// The off hand is a stack of its own, as Create's sandpaper takes one item to polish from it:
    /// `getOffhandItem` and `getItemInHand(OFF_HAND)` answer the same object, and the rest the mod
    /// puts back there reaches the game's off hand; the main hand keeps what it held.
    #[test]
    fn the_off_hand_is_a_stack_of_its_own() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.held = Some(("minecraft:stick".into(), 1));
        host.offhand = Some(HandStack { item: "minecraft:diamond".into(), count: 5, ..Default::default() });
        let taken = rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let player = api_common::acting_player(rt);
                let off = hand_constant(rt, OFF);
                let s = api::call(rt, IN_HAND, &[player, off]);
                assert_eq!(api::call(rt, OFF_HAND_ITEM, &[player]), s);
                assert_ne!(stack(rt, MAIN), s);
                let rest = api::call(rt, COPY, &[s]);
                let one = api::call(rt, SPLIT, &[rest, V::I(1)]);
                api::call(rt, SET_IN_HAND, &[player, off, rest]);
                api::call(rt, COUNT, &[one]).i()
            })
        });
        assert_eq!(taken, 1);
        assert_eq!(host.offhand.as_ref().map(|s| (s.item.as_str(), s.count)), Some(("minecraft:diamond", 4)));
        assert_eq!(host.held, Some(("minecraft:stick".into(), 1)));
    }

    /// `hurtAndBreak` on a stack in hand wears the game's stack (the game's own rules), and the
    /// mod's stack holds what the game left: worn, then broken and empty.
    #[test]
    fn wear_on_a_stack_in_hand_is_the_games() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.held = Some(("minecraft:shears".into(), 1));
        host.held_damage = 1;
        host.max_damage.insert("minecraft:shears".into(), 3);
        let seen = rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let player = api_common::acting_player(rt);
                let main = hand_constant(rt, MAIN);
                let s = api::call(rt, IN_HAND, &[player, main]);
                let before = (api::call(rt, DAMAGE, &[s]).i(), api::call(rt, MAX_DAMAGE, &[s]).i(), api::call(rt, DAMAGED, &[s]).i());
                let slot = mainhand_slot(rt);
                api::call(rt, HURT, &[s, V::I(1), player, slot]);
                (before, api::call(rt, DAMAGE, &[s]).i())
            })
        });
        assert_eq!(seen, ((1, 3, 1), 2));
        assert_eq!((host.held.clone(), host.held_damage), (Some(("minecraft:shears".into(), 1)), 2));

        let empty = rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let s = stack(rt, MAIN);
                let player = api_common::acting_player(rt);
                let slot = mainhand_slot(rt);
                api::call(rt, HURT, &[s, V::I(1), player, slot]);
                api::call(rt, IS_EMPTY, &[s]).i()
            })
        });
        assert_eq!(empty, 1, "worn out, the stack is gone");
        assert_eq!(host.held, None);

        host.creative = true;
        host.held = Some(("minecraft:shears".into(), 1));
        host.held_damage = 0;
        rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let s = stack(rt, MAIN);
                let player = api_common::acting_player(rt);
                let slot = mainhand_slot(rt);
                api::call(rt, HURT, &[s, V::I(5), player, slot]);
            })
        });
        assert_eq!((host.held.clone(), host.held_damage), (Some(("minecraft:shears".into(), 1)), 0), "none in creative");
    }

    /// A stack in no hand wears where it is: its damage grows, a copy keeps it, and one worn out
    /// shrinks by one.
    #[test]
    fn a_stack_in_no_hand_wears_itself() {
        let mut rt = Rt::new();
        crate::install();
        let s = api_common::stack(&mut rt, "minecraft:shears", 1);
        let max = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.MAX_DAMAGE");
        let two = api_common::integer(&mut rt, 2);
        api::call(&mut rt, SET, &[s, max, two]);
        let slot = mainhand_slot(&mut rt);
        api::call(&mut rt, HURT, &[s, V::I(1), V::NULL, slot]);
        let copy = api::call(&mut rt, COPY, &[s]);
        assert_eq!((api::call(&mut rt, DAMAGE, &[s]).i(), api::call(&mut rt, DAMAGE, &[copy]).i()), (1, 1));
        api::call(&mut rt, HURT, &[s, V::I(1), V::NULL, slot]);
        assert_eq!(api::call(&mut rt, IS_EMPTY, &[s]).i(), 1);
        assert_eq!(api::call(&mut rt, IS_EMPTY, &[copy]).i(), 0);
    }
}
