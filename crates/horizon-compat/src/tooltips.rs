//! Item tooltips as NeoForge 1.21.1 builds them (`ItemStack.getTooltipLines`): the name, the item's
//! own lines right after it (`Item.appendHoverText`: a mod item's override, or for a block item its
//! block's, `BlockItem.appendHoverText` asking `Block.appendHoverText`), the game's (enchantments,
//! attributes, lore...), and last `ItemTooltipEvent` with all of them, where mods add, change or
//! take out lines (Create's "Hold [Shift] for Summary"). 95 jars of the Create pack override
//! `appendHoverText` (872 classes), 35 listen to the event.
//!
//! The game asks each frame a tooltip shows (`native_tooltip`), with its lines built; the mods see
//! each as a literal of its text, and the game keeps its own (styled) line where a mod left it.

use crate::{Rt, V};

pub(crate) const EVENT: &str = "net/neoforged/neoforge/event/entity/player/ItemTooltipEvent";
const PLAYER_EVENT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent";
pub(crate) const CONTEXT: &str = "net/minecraft/world/item/Item$TooltipContext";
const FLAG: &str = "net/minecraft/world/item/TooltipFlag";
const DEFAULT_FLAG: &str = "net/minecraft/world/item/TooltipFlag$Default";
const CLIENT_FLAG: &str = "net/neoforged/neoforge/client/ClientTooltipFlag";
const ITEM: &str = "net/minecraft/world/item/Item";
const BLOCK_ITEM: &str = "net/minecraft/world/item/BlockItem";
const BLOCK: &str = "net/minecraft/world/level/block/Block";
/// `appendHoverText(ItemStack, Item.TooltipContext, List, TooltipFlag)`: on `Item` and on `Block`.
const APPEND: &str = "appendHoverText.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/Item$TooltipContext;Ljava/util/List;Lnet/minecraft/world/item/TooltipFlag;)V";
const ITEM_APPEND: &str = "net/minecraft/world/item/Item.appendHoverText.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/Item$TooltipContext;Ljava/util/List;Lnet/minecraft/world/item/TooltipFlag;)V";
const BLOCK_APPEND: &str = "net/minecraft/world/level/block/Block.appendHoverText.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/Item$TooltipContext;Ljava/util/List;Lnet/minecraft/world/item/TooltipFlag;)V";

/// A line of the tooltip the mods answer: one of the game's (its index), or a new one (a component,
/// as vanilla's serializer writes it).
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Given(usize),
    New(serde_json::Value),
}

/// What reads as the mods' own: `ItemTooltipEvent`'s calls, the tooltip context's and flags', and
/// `appendHoverText` where a mod's override calls its super.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, EVENT | CONTEXT | FLAG | DEFAULT_FLAG | CLIENT_FLAG)
        || (owner == PLAYER_EVENT && name == "getEntity")
        || (matches!(owner, ITEM | BLOCK_ITEM | BLOCK) && name == "appendHoverText")
}

/// Whose `appendHoverText` adds item `it`'s own lines: its class's, else (a block item) its
/// block's; None when neither is a mod's.
fn appender(rt: &Rt, it: V) -> Option<(V, &'static str)> {
    let overrides = rt.overrides?;
    if overrides(rt.class_of(it), APPEND) {
        return Some((it, ITEM_APPEND));
    }
    block_appender(rt, it)
}

/// A block item's block, when the block is a mod's with its own `appendHoverText`.
fn block_appender(rt: &Rt, it: V) -> Option<(V, &'static str)> {
    let overrides = rt.overrides?;
    let block = rt.get_field(it, "$block");
    (block.r() != 0 && rt.str_of(block).is_none() && overrides(rt.class_of(block), APPEND)).then_some((block, BLOCK_APPEND))
}

/// The flags a tooltip is built with on the client (`ClientTooltipFlag.of`): advanced (F3 + H),
/// creative, and the modifiers held.
fn flag(rt: &mut Rt, advanced: bool) -> V {
    let creative = rt.host().is_some_and(|h| h.player_creative());
    let held = |rt: &mut Rt, which: &'static str| crate::api::call(rt, which, &[]);
    let shift = held(rt, "net/minecraft/client/gui/screens/Screen.hasShiftDown.()Z");
    let control = held(rt, "net/minecraft/client/gui/screens/Screen.hasControlDown.()Z");
    let alt = held(rt, "net/minecraft/client/gui/screens/Screen.hasAltDown.()Z");
    crate::records::make(rt, CLIENT_FLAG, &[V::I(advanced as i32), V::I(creative as i32), shift, control, alt])
}

/// `Item.TooltipContext.of(level)` (null: `EMPTY`).
fn context(rt: &mut Rt, level: V) -> V {
    let c = rt.new_object(CONTEXT);
    rt.put_field(c, "$level", level);
    c
}

/// Stack `held`'s tooltip, the game's lines `given` (their text, the name first) built with
/// `advanced` tooltips or not: the lines as the mods leave them, None when they changed none.
pub fn tooltip(rt: &mut Rt, held: crate::host::HandStack, given: &[String], advanced: bool) -> Option<Vec<Line>> {
    let it = rt.registered.get(&("item".to_owned(), held.item.clone())).copied();
    let own = it.and_then(|it| appender(rt, it));
    if own.is_none() && crate::neo::hearing(rt, EVENT).is_empty() {
        return None;
    }
    let stack = crate::hand::load(rt, Some(held));
    let mut lines = Vec::with_capacity(given.len());
    for text in given {
        let s = rt.string(text);
        lines.push(crate::api_common::component(rt, s, false, V::NULL));
    }
    let flag = flag(rt, advanced);
    let level = crate::glue::level(rt);
    let context = context(rt, level);
    let mut all = lines.clone();
    if let (Some((target, key)), Some(vcall)) = (own, rt.vcall) {
        let mine = crate::jutil::new_list(rt, Vec::new());
        vcall(rt, key, APPEND, &[target, stack, context, mine, flag]);
        rt.report_thrown("appendHoverText");
        // Right after the name, where `getTooltipLines` has them.
        let at = all.len().min(1);
        all.splice(at..at, crate::jutil::elements(rt, mine));
    }
    let list = crate::jutil::new_list(rt, all);
    if !crate::neo::hearing(rt, EVENT).is_empty() {
        let ev = rt.new_object(EVENT);
        let player = if rt.host().is_some_and(|h| h.client_player().is_some()) { crate::network::local_player(rt) } else { V::NULL };
        for (field, v) in [("$itemStack", stack), ("$toolTip", list), ("$flags", flag), ("$context", context), ("$entity", player)] {
            rt.put_field(ev, field, v);
        }
        crate::neo::post(rt, EVENT, ev);
    }
    let after = crate::jutil::elements(rt, list);
    if after == lines {
        return None;
    }
    Some(
        after
            .into_iter()
            .map(|c| match lines.iter().position(|&l| l == c) {
                Some(i) => Line::Given(i),
                None => Line::New(crate::api_common::component_json(rt, c)),
            })
            .collect(),
    )
}

/// Static fields: `TooltipFlag.NORMAL`, `ADVANCED`; `Item.TooltipContext.EMPTY`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (FLAG | DEFAULT_FLAG, "NORMAL" | "ADVANCED") => {
            let key = if field == "NORMAL" { "compat:tooltip-flag/normal" } else { "compat:tooltip-flag/advanced" };
            let advanced = field == "ADVANCED";
            Some(rt.intern(key.into(), |rt| crate::records::make(rt, DEFAULT_FLAG, &[V::I(advanced as i32), V::I(0)])))
        }
        (CONTEXT, "EMPTY") => Some(rt.intern("compat:tooltip-context/empty".into(), |rt| context(rt, V::NULL))),
        _ => None,
    }
}

/// The event's getters, the context's and the flags' calls, and `appendHoverText` called on a
/// vanilla class (a mod's override calling its super: `BlockItem`'s asks its block).
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match (owner, name) {
        (EVENT | PLAYER_EVENT, _) => {
            if owner == PLAYER_EVENT && rt.class_of(a(0)) != EVENT {
                return None;
            }
            let field = match name {
                "getItemStack" => "$itemStack",
                "getToolTip" => "$toolTip",
                "getFlags" => "$flags",
                "getContext" => "$context",
                "getEntity" => "$entity",
                _ => return None,
            };
            Some(rt.get_field(a(0), field))
        }
        (CONTEXT, "of") => Some(match desc {
            "(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/item/Item$TooltipContext;" => context(rt, a(0)),
            // Registries alone: a context without a level answers them.
            _ => {
                let c = context(rt, V::NULL);
                rt.put_field(c, "$registries", a(0));
                c
            }
        }),
        (CONTEXT, "registries") => {
            let level = rt.get_field(a(0), "$level");
            Some(if level.r() != 0 {
                crate::api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[level])
            } else {
                match rt.get_field(a(0), "$registries") {
                    V::Void => V::NULL,
                    v => v,
                }
            })
        }
        (CONTEXT, "tickRate") => Some(V::F(20.0)),
        (CONTEXT, "mapData") => Some(V::NULL),
        (FLAG | DEFAULT_FLAG | CLIENT_FLAG, "isAdvanced" | "isCreative" | "hasShiftDown" | "hasControlDown" | "hasAltDown") => {
            let field = match name {
                "isAdvanced" => "advanced",
                "isCreative" => "creative",
                "hasShiftDown" => "shiftDown",
                "hasControlDown" => "controlDown",
                _ => "altDown",
            };
            Some(match rt.get_field(a(0), field) {
                V::Void => V::I(0),
                v => v,
            })
        }
        (FLAG | DEFAULT_FLAG, "asCreative") => {
            let advanced = rt.get_field(a(0), "advanced");
            Some(crate::records::make(rt, DEFAULT_FLAG, &[advanced, V::I(1)]))
        }
        (CLIENT_FLAG, "of") => {
            let advanced = rt.get_field(a(0), "advanced").i() != 0;
            let f = flag(rt, advanced);
            let creative = rt.get_field(a(0), "creative");
            rt.put_field(f, "creative", creative);
            Some(f)
        }
        // `Item`'s and `Block`'s add nothing; `BlockItem`'s asks its block.
        (ITEM | BLOCK, "appendHoverText") => Some(V::Void),
        (BLOCK_ITEM, "appendHoverText") => {
            if let (Some((block, key)), Some(vcall)) = (block_appender(rt, a(0)), rt.vcall) {
                vcall(rt, key, APPEND, &[block, a(1), a(2), a(3), a(4)]);
            }
            Some(V::Void)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{HandStack, MemoryHost};

    const WRENCH: &str = "test/Wrench";
    const CASING: &str = "test/Casing";

    fn overrides(class: &str, key: &str) -> bool {
        matches!(class, WRENCH | CASING) && key == APPEND
    }

    /// The mods' code: the wrench adds a gray line after its super's; the casing block a line of its
    /// own; the listener (lambda 1) puts a hint after the name for a wrench and drops the game's
    /// line saying "drop me".
    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if short == APPEND && matches!(rt.class_of(args[0]), WRENCH | CASING) {
            if rt.class_of(args[0]) == WRENCH {
                crate::api::call(rt, ITEM_APPEND, args);
            }
            let text = rt.string(if rt.class_of(args[0]) == WRENCH { "Rotates blocks" } else { "A casing" });
            let line = crate::api_common::component(rt, text, false, V::NULL);
            crate::jutil::list_add(rt, args[3], line);
            return V::Void;
        }
        crate::api::call(rt, key, args)
    }

    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        if id != 1 {
            return None;
        }
        let ev = args[1];
        let get = |rt: &mut Rt, name: &str, desc: &str| {
            let key = Box::leak(format!("{EVENT}.{name}.{desc}").into_boxed_str());
            crate::api::call(rt, key, &[ev])
        };
        let stack = get(rt, "getItemStack", "()Lnet/minecraft/world/item/ItemStack;");
        let list = get(rt, "getToolTip", "()Ljava/util/List;");
        let flags = get(rt, "getFlags", "()Lnet/minecraft/world/item/TooltipFlag;");
        let shift = crate::api::call(rt, "net/minecraft/world/item/TooltipFlag.hasShiftDown.()Z", &[flags]);
        let item = crate::api::call(rt, "net/minecraft/world/item/ItemStack.getItem.()Lnet/minecraft/world/item/Item;", &[stack]);
        let lines = crate::jutil::elements(rt, list);
        let mut kept: Vec<V> = lines.into_iter().filter(|&l| crate::api_common::component_json(rt, l)["text"] != "drop me").collect();
        if rt.class_of(item) == WRENCH {
            let text = rt.string(if shift.i() != 0 { "Summary" } else { "Hold [Shift] for Summary" });
            let hint = crate::api_common::component(rt, text, false, V::NULL);
            kept.insert(1, hint);
        }
        let fresh = crate::jutil::new_list(rt, kept);
        crate::api::call(rt, "java/util/List.clear.()V", &[list]);
        crate::api::call(rt, "java/util/List.addAll.(Ljava/util/Collection;)Z", &[list, fresh]);
        Some(V::Void)
    }

    fn texts(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| match l {
                Line::Given(i) => format!("#{i}"),
                Line::New(j) => j["text"].as_str().unwrap_or("?").to_owned(),
            })
            .collect()
    }

    /// A tooltip as `getTooltipLines` builds it for the mods: a mod item's own lines (its override,
    /// calling its super) and a mod block's (through its block item) right after the name, then the
    /// event with every line, where a mod adds a hint after the name (from the flags' Shift) and
    /// takes out a line; the game's lines come back by index. A vanilla item nobody listens for is
    /// left alone.
    #[test]
    fn a_tooltip_gets_the_mods_lines_as_get_tooltip_lines_builds_it() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(lambdas);
        let wrench = rt.new_object(WRENCH);
        rt.registered.insert(("item".into(), "test:wrench".into()), wrench);
        let casing = rt.new_object(CASING);
        let casing_item = rt.new_object(BLOCK_ITEM);
        rt.put_field(casing_item, "$block", casing);
        rt.registered.insert(("item".into(), "test:casing".into()), casing_item);
        let stack = |item: &str| HandStack { item: item.into(), count: 1, ..Default::default() };
        let given = |lines: &[&str]| lines.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut h = MemoryHost { client: true, ..Default::default() };
        rt.with_host(&mut h, |rt| {
            assert_eq!(tooltip(rt, stack("minecraft:stone"), &given(&["Stone"]), false), None, "nobody listens");
            let own = tooltip(rt, stack("test:casing"), &given(&["Casing", "Lore"]), false).unwrap();
            assert_eq!(texts(&own), ["#0", "A casing", "#1"]);
        });
        let l = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        rt.listeners.push((EVENT.to_owned(), l));
        rt.with_host(&mut h, |rt| {
            let lines = tooltip(rt, stack("test:wrench"), &given(&["Wrench", "drop me", "When in main hand:"]), false).unwrap();
            assert_eq!(texts(&lines), ["#0", "Hold [Shift] for Summary", "Rotates blocks", "#2"]);
            assert_eq!(tooltip(rt, stack("minecraft:stone"), &given(&["Stone"]), false), None, "a listener that changes nothing");
        });
        h.keys_held.push("key.keyboard.left.shift".into());
        rt.with_host(&mut h, |rt| {
            let lines = tooltip(rt, stack("test:wrench"), &given(&["Wrench"]), true).unwrap();
            assert_eq!(texts(&lines), ["#0", "Summary", "Rotates blocks"]);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        assert!(rt.thrown.is_none());
    }
}
