//! NeoForge 1.21.1's raw input events (`InputEvent`), where its `ClientHooks` post them: a key after
//! the game handled it (`InputEvent.Key`, at the end of `KeyboardHandler.keyPress`: on any screen,
//! repeats too), a mouse button before the game acts on it (`InputEvent.MouseButton.Pre`, which a
//! mod cancels to keep the press from the game) and after (`.Post`), and the wheel while playing once
//! whole steps have built up (`InputEvent.MouseScrollingEvent`, cancelled to keep the scroll from the
//! hotbar). Create opens its toolbox and schematic tools on keys and scrolls its tools with them.
//!
//! Mods hear none of them before the client's setup (the first tick in a world): their handlers
//! reach for the player and the level.

use crate::{Rt, V};

pub(crate) const KEY: &str = "net/neoforged/neoforge/client/event/InputEvent$Key";
pub(crate) const BUTTON: &str = "net/neoforged/neoforge/client/event/InputEvent$MouseButton";
pub(crate) const BUTTON_PRE: &str = "net/neoforged/neoforge/client/event/InputEvent$MouseButton$Pre";
pub(crate) const BUTTON_POST: &str = "net/neoforged/neoforge/client/event/InputEvent$MouseButton$Post";
pub(crate) const SCROLLING: &str = "net/neoforged/neoforge/client/event/InputEvent$MouseScrollingEvent";

/// GLFW's actions: `GLFW_RELEASE`, `GLFW_PRESS`, `GLFW_REPEAT`.
pub const RELEASE: i32 = 0;
pub const PRESS: i32 = 1;
pub const REPEAT: i32 = 2;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

/// Whether the mods may hear input yet ([`crate::neo::is_client`]: the client's setup ran).
fn heard(rt: &Rt) -> bool {
    crate::neo::is_client(rt)
}

/// `InputEvent.Key`: key `key` (`key.keyboard.g`, as `options.txt` names keys) went down, came up or
/// repeated (`action`, GLFW's), with GLFW's modifier bits. Its scancode is 0: the window gives none
/// that is the same on every system, and a key with a code never needs one
/// (`InputConstants.getKey`, `KeyMapping.matches`).
pub fn key(rt: &mut Rt, key: &str, action: i32, modifiers: i32) {
    let Some(code) = crate::screens::key_code(key) else { return };
    if !heard(rt) {
        return;
    }
    let ev = rt.new_object(KEY);
    rt.put_field(ev, "$key", V::I(code));
    rt.put_field(ev, "$scanCode", V::I(0));
    rt.put_field(ev, "$action", V::I(action));
    rt.put_field(ev, "$modifiers", V::I(modifiers));
    crate::neo::post(rt, KEY, ev);
}

/// `InputEvent.MouseButton.Pre` (`pre`) or `.Post`: mouse button `button` (GLFW's: 0 left, 1 right,
/// 2 middle) went down or up. Whether a mod cancelled the `Pre`: the game then does nothing with it.
pub fn button(rt: &mut Rt, button: i32, pressed: bool, modifiers: i32, pre: bool) -> bool {
    if !heard(rt) {
        return false;
    }
    let class = if pre { BUTTON_PRE } else { BUTTON_POST };
    let ev = rt.new_object(class);
    rt.put_field(ev, "$button", V::I(button));
    rt.put_field(ev, "$action", V::I(if pressed { PRESS } else { RELEASE }));
    rt.put_field(ev, "$modifiers", V::I(modifiers));
    crate::neo::post(rt, class, ev);
    pre && rt.get_field(ev, "$canceled").i() != 0
}

/// `InputEvent.MouseScrollingEvent`: the wheel turned by `delta` (x, y; the scroll as the options
/// scale it, `ClientHooks.onMouseScroll`'s) while playing, with the buttons held (left, middle,
/// right) and the pointer `at`. Whether a mod cancelled it: the hotbar then doesn't scroll.
pub fn scrolling(rt: &mut Rt, delta: [f64; 2], buttons: [bool; 3], at: [f64; 2]) -> bool {
    if !heard(rt) {
        return false;
    }
    let ev = rt.new_object(SCROLLING);
    rt.put_field(ev, "$scrollDeltaX", V::D(delta[0]));
    rt.put_field(ev, "$scrollDeltaY", V::D(delta[1]));
    for (field, down) in ["$leftDown", "$middleDown", "$rightDown"].into_iter().zip(buttons) {
        rt.put_field(ev, field, V::I(down as i32));
    }
    rt.put_field(ev, "$mouseX", V::D(at[0]));
    rt.put_field(ev, "$mouseY", V::D(at[1]));
    crate::neo::post(rt, SCROLLING, ev);
    rt.get_field(ev, "$canceled").i() != 0
}

/// The events' getters and their cancelling (`ICancellableEvent`).
pub(crate) fn call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let ev = arg(args, 0);
    let field: &'static str = match (owner, name) {
        (KEY, "getKey") => "$key",
        (KEY, "getScanCode") => "$scanCode",
        (KEY | BUTTON | BUTTON_PRE | BUTTON_POST, "getAction") => "$action",
        (KEY | BUTTON | BUTTON_PRE | BUTTON_POST, "getModifiers") => "$modifiers",
        (BUTTON | BUTTON_PRE | BUTTON_POST, "getButton") => "$button",
        (SCROLLING, "getScrollDeltaX") => "$scrollDeltaX",
        (SCROLLING, "getScrollDeltaY") => "$scrollDeltaY",
        (SCROLLING, "isLeftDown") => "$leftDown",
        (SCROLLING, "isMiddleDown") => "$middleDown",
        (SCROLLING, "isRightDown") => "$rightDown",
        (SCROLLING, "getMouseX") => "$mouseX",
        (SCROLLING, "getMouseY") => "$mouseY",
        (BUTTON_PRE | SCROLLING, "isCanceled") => "$canceled",
        (BUTTON_PRE | SCROLLING, "setCanceled") => {
            rt.put_field(ev, "$canceled", V::I(arg(args, 1).i()));
            return Some(V::Void);
        }
        _ => return None,
    };
    Some(match rt.get_field(ev, field) {
        V::Void if field.starts_with("$scroll") || field.starts_with("$mouse") => V::D(0.0),
        V::Void => V::I(0),
        v => v,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// A mod listening as Create does: on a key it keeps the key, action and modifiers it was given;
    /// on a right button's `Pre` it cancels; on the wheel it keeps the scroll and cancels while the
    /// middle button is held.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        let get = |rt: &mut Rt, class: &str, name: &str, desc: &str| {
            let key = Box::leak(format!("{class}.{name}.{desc}").into_boxed_str());
            crate::api::call(rt, key, &[ev])
        };
        match id {
            1 => {
                let seen = [get(rt, KEY, "getKey", "()I"), get(rt, KEY, "getAction", "()I"), get(rt, KEY, "getModifiers", "()I")];
                let list = rt.interned["test:keys"];
                for v in seen {
                    crate::jutil::list_add(rt, list, v);
                }
            }
            2 => {
                if get(rt, BUTTON_PRE, "getButton", "()I").i() == 1 {
                    crate::api::call(rt, "net/neoforged/neoforge/client/event/InputEvent$MouseButton$Pre.setCanceled.(Z)V", &[ev, V::I(1)]);
                }
            }
            3 => {
                let y = get(rt, SCROLLING, "getScrollDeltaY", "()D");
                rt.interned.insert("test:scroll".into(), y);
                if get(rt, SCROLLING, "isMiddleDown", "()Z").i() != 0 {
                    crate::api::call(rt, "net/neoforged/neoforge/client/event/InputEvent$MouseScrollingEvent.setCanceled.(Z)V", &[ev, V::I(1)]);
                }
            }
            _ => return None,
        }
        Some(V::Void)
    }

    /// The events reach the mods with what NeoForge gives them (GLFW's key code, action and
    /// modifier bits; the button; the scaled scroll), and a mod's cancelling comes back to the game:
    /// a cancelled right click, a cancelled scroll. Before the client's setup no mod hears input.
    #[test]
    fn input_reaches_the_mods_as_neoforges_input_events() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lambdas);
        for (class, id) in [(KEY, 1), (BUTTON_PRE, 2), (SCROLLING, 3)] {
            let l = rt.lambda_id("java/util/function/Consumer", id, vec![]);
            rt.listeners.push((class.to_owned(), l));
        }
        let list = crate::jutil::new_list(&mut rt, Vec::new());
        rt.interned.insert("test:keys".into(), list);
        let mut h = MemoryHost { client: true, ..Default::default() };
        rt.with_host(&mut h, |rt| {
            key(rt, "key.keyboard.g", PRESS, 0);
            assert!(crate::jutil::elements(rt, list).is_empty(), "before the client's setup");
        });
        rt.interned.insert("compat:client".into(), V::I(1));
        rt.with_host(&mut h, |rt| {
            key(rt, "key.keyboard.g", PRESS, 1);
            key(rt, "key.keyboard.g", REPEAT, 1);
            key(rt, "key.keyboard.left.shift", RELEASE, 0);
            let seen: Vec<i32> = crate::jutil::elements(rt, list).iter().map(|v| v.i()).collect();
            assert_eq!(seen, [71, PRESS, 1, 71, REPEAT, 1, 340, RELEASE, 0]);
            assert!(!button(rt, 0, true, 0, true), "a left click goes on");
            assert!(button(rt, 1, true, 0, true), "the mod cancelled the right click");
            assert!(!button(rt, 1, true, 0, false), "a Post isn't cancelled");
            assert!(!scrolling(rt, [0.0, -2.5], [false; 3], [10.0, 20.0]));
            assert_eq!(rt.interned["test:scroll"], V::D(-2.5));
            assert!(scrolling(rt, [0.0, 1.0], [false, true, false], [10.0, 20.0]), "cancelled with the middle button held");
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
