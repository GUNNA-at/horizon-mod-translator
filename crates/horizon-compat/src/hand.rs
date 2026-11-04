//! The acting player's main hand while a hook runs. Vanilla's player holds one stack object in each
//! hand: what a mod changes in it (`shrink`, a component `set`), it finds there again, through the
//! player (`getItemInHand`) or a use's context (`UseOnContext.getItemInHand`). Here the hand is one
//! stack object for a whole hook ([`with_hand`]), loaded from the game when the mod first reaches it
//! (its mod components read with their types' codecs), and what the mod did to it reaches the
//! game's stack when the hook ends: its count and components in place (vanilla's own data on it
//! stays), or the stack the mod put in the hand instead (`setItemInHand`, a use's answer).
use crate::nbt::Nbt;
use crate::{api_common, components, Rt, V};
use std::collections::BTreeMap;

const HAND: &str = "compat:hand";

fn record(rt: &mut Rt) -> V {
    rt.intern(HAND.into(), |rt| rt.new_object(HAND))
}

/// Runs `f`, a hook acting for a player, with one main hand for all of it; what the mod did to the
/// hand reaches the game when the outermost such hook ends. Call inside `with_host`.
pub fn with_hand<R>(rt: &mut Rt, f: impl FnOnce(&mut Rt) -> R) -> R {
    let r = record(rt);
    let depth = rt.get_field(r, "$depth").i();
    rt.put_field(r, "$depth", V::I(depth + 1));
    let out = f(rt);
    let r = record(rt);
    rt.put_field(r, "$depth", V::I(depth));
    if depth == 0 {
        flush(rt);
        let r = record(rt);
        rt.put_field(r, "$stack", V::NULL);
        rt.put_field(r, "$saved", V::NULL);
    }
    out
}

fn in_hook(rt: &mut Rt) -> bool {
    let r = record(rt);
    rt.get_field(r, "$depth").i() > 0
}

fn empty(rt: &mut Rt) -> V {
    api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
}

/// The main hand's stack: within a hook the same object each time (`ItemStack.EMPTY` for an empty
/// hand); outside one, a new one each time.
pub fn stack(rt: &mut Rt) -> V {
    let r = record(rt);
    if let V::R(s) = rt.get_field(r, "$stack")
        && s != 0
    {
        return V::R(s);
    }
    let s = load(rt);
    if in_hook(rt) {
        let r = record(rt);
        rt.put_field(r, "$stack", s);
        rt.put_field(r, "$replaced", V::I(0));
        // What the game holds, as the stack writes it back: a codec that doesn't round-trip
        // exactly doesn't count as a change.
        let (held, carried) = state(rt, s);
        let id = match &held {
            Some((id, _)) => rt.string(id),
            None => V::NULL,
        };
        let saved = crate::nbt::to_runtime(rt, &carried);
        let r = record(rt);
        rt.put_field(r, "$id", id);
        rt.put_field(r, "$count", V::I(held.map_or(0, |(_, n)| n as i32)));
        rt.put_field(r, "$saved", saved);
    }
    s
}

/// The game's main-hand stack as a stack here, its mod components on it.
fn load(rt: &mut Rt) -> V {
    let Some((id, count)) = rt.host().and_then(|h| h.player_held()) else { return empty(rt) };
    let s = api_common::stack(rt, &id, count as i32);
    let carried = rt.host().map(|h| h.player_held_components()).unwrap_or_default();
    components::load(rt, s, &carried);
    s
}

/// What `s` holds (its item and count, None when empty) and its components as the game keeps them.
fn state(rt: &mut Rt, s: V) -> (Option<(String, u32)>, BTreeMap<String, Nbt>) {
    if api_common::stack_empty(rt, s) {
        return (None, BTreeMap::new());
    }
    let id = api_common::item_like_name(rt, rt.get_field(s, "$item"));
    let count = rt.get_field(s, "$count").i().max(0) as u32;
    (Some((id, count)), components::saved(rt, s).into_iter().collect())
}

/// The mod put `stack` in the main hand (`Player.setItemInHand`, a use answering another stack):
/// within a hook, the hand is that object from now on and the game gets it when the hook ends;
/// outside one, the game gets it now.
pub fn replace(rt: &mut Rt, stack: V) {
    if !in_hook(rt) {
        put(rt, stack);
        return;
    }
    // What the hand held first stays the baseline.
    self::stack(rt);
    let r = record(rt);
    rt.put_field(r, "$stack", stack);
    rt.put_field(r, "$replaced", V::I(1));
}

/// The game's main hand holds what `stack` holds now.
fn put(rt: &mut Rt, stack: V) {
    let (held, carried) = state(rt, stack);
    let carried: Vec<(String, Nbt)> = carried.into_iter().collect();
    if let Some(h) = rt.host_mut() {
        match held {
            Some((id, n)) => h.set_player_held(&id, n, &carried),
            None => h.set_player_held("", 0, &[]),
        }
    }
}

/// The hook ends: what the mod did to the hand reaches the game.
fn flush(rt: &mut Rt) {
    let r = record(rt);
    let s = rt.get_field(r, "$stack");
    if s.r() == 0 {
        return; // never reached
    }
    if rt.get_field(r, "$replaced").i() != 0 {
        put(rt, s);
        return;
    }
    let before_id = rt.str_of(rt.get_field(r, "$id")).map(str::to_owned);
    let before_count = rt.get_field(r, "$count").i().max(0) as u32;
    let saved = rt.get_field(r, "$saved");
    let before = if saved.r() == 0 { BTreeMap::new() } else { crate::nbt::from_runtime(rt, saved) };
    let (held, now) = state(rt, s);
    let Some(h) = rt.host_mut() else { return };
    match (before_id, held) {
        (None, None) => {}
        // Used up: the hand is empty.
        (Some(_), None) => {
            h.update_player_held(0, &[]);
        }
        (Some(was), Some((id, n))) if was == id => {
            let mut changes: Vec<(String, Option<Nbt>)> =
                before.keys().filter(|k| !now.contains_key(*k)).map(|k| (k.clone(), None)).collect();
            changes.extend(now.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), Some(v.clone()))));
            if n != before_count || !changes.is_empty() {
                h.update_player_held(n, &changes);
            }
        }
        // The hand's stack holds something else now.
        (_, Some((id, n))) => {
            let carried: Vec<(String, Nbt)> = now.into_iter().collect();
            h.set_player_held(&id, n, &carried);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::host::MemoryHost;

    const IN_HAND: &str = "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;";
    const SET_IN_HAND: &str = "net/minecraft/world/entity/player/Player.setItemInHand.(Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;)V";
    const SET: &str = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
    const GET: &str = "net/minecraft/core/component/DataComponentHolder.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    const REMOVE: &str = "net/minecraft/world/item/ItemStack.remove.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    const SHRINK: &str = "net/minecraft/world/item/ItemStack.shrink.(I)V";

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

    fn main_hand(rt: &mut Rt) -> V {
        api::get_static(rt, "net/minecraft/world/InteractionHand.MAIN_HAND")
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
                let main = main_hand(rt);
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
                let s = stack(rt);
                let v = api::call(rt, GET, &[s, ty]);
                api::pos_of(rt, v)
            })
        });
        assert_eq!(read, [4, 64, -2]);
        assert_eq!(host.held, Some(("minecraft:stick".into(), 2)), "a hook that only reads changes nothing");

        rt.with_host(&mut host, |rt| {
            with_hand(rt, |rt| {
                let s = stack(rt);
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
                let main = main_hand(rt);
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
}
