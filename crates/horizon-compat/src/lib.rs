//! Minecraft's and NeoForge's API for mods the translator turned into Rust, mapped onto Horizon,
//! natively. The object model (values, heap, dispatch tables) is `horizon-compat-core`, re-exported
//! here: a translated engine needs only that crate, and calls this one through [`install`] when it
//! isn't inlined into the mod (so a mapping change rebuilds this crate, not the engine).
pub use horizon_compat_core::*;
pub use horizon_compat_core::host;

pub mod api;
pub mod api_common;
pub mod blocks;
pub mod codecs;
pub mod crafting;
pub mod handlers;
pub mod components;
pub mod container_screens;
pub mod capabilities;
pub mod entities;
pub mod events;
pub mod convention;
pub mod creative;
pub mod fluids;
pub mod food;
pub mod item_use;
pub mod functions;
pub mod glue;
pub mod gui;
pub mod hand;
pub mod jlang;
pub mod jutil;
pub mod menus;
pub mod widgets;
pub mod mod_entities;
pub mod nbt;
pub mod neo;
pub mod neo_tags;
pub mod network;
pub mod quads;
pub mod recipes;
pub mod palette;
pub mod particles;
pub mod records;
pub mod render;
pub mod rotation;
pub mod screens;
pub mod shapes;
pub mod sounds;
pub mod tags;
pub mod tanks;
pub mod vanilla_effects;
pub mod vanilla_records;
pub mod vanilla_shapes;
pub mod vanilla_sounds;
pub mod vanilla_tags;

// BEGIN RUNTIME HELPERS: bundles inline this part of the root into their `compat` module too.

/// An API key's owner, name and descriptor (`owner.name.desc`). The modules [`api::call`] asks in
/// turn each want them for the same key, so the last key's parts are kept: one split per call,
/// not one per module (a third of a client tick went to splitting before).
pub(crate) fn parts(key: &'static str) -> Option<(&'static str, &'static str, &'static str)> {
    type Parts = Option<(&'static str, &'static str, &'static str)>;
    thread_local! {
        static LAST: std::cell::Cell<(usize, usize, Parts)> = const { std::cell::Cell::new((0, 0, None)) };
    }
    LAST.with(|last| {
        let (at, len, parts) = last.get();
        if at == key.as_ptr() as usize && len == key.len() {
            return parts;
        }
        let parts = key.split_once('.').and_then(|(owner, rest)| rest.split_once('.').map(|(name, desc)| (owner, name, desc)));
        last.set((key.as_ptr() as usize, key.len(), parts));
        parts
    })
}

thread_local! {
    static MISSES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static LAST_MISS: std::cell::RefCell<HashMap<&'static str, u64>> = std::cell::RefCell::new(HashMap::default());
}

/// Counts a call or static nothing maps ([`Rt::unmapped`]).
pub(crate) fn miss(rt: &mut Rt, key: &'static str) {
    *rt.unmapped.entry(key).or_insert(0) += 1;
    let n = MISSES.with(|m| {
        m.set(m.get() + 1);
        m.get()
    });
    LAST_MISS.with(|l| l.borrow_mut().insert(key, n));
}

/// How many misses this thread has counted so far.
pub(crate) fn misses() -> u64 {
    MISSES.with(std::cell::Cell::get)
}

/// Whether `key` missed after [`misses`] was `since`. Most calls miss nothing, which [`misses`]
/// alone tells; this is asked only when something did.
pub(crate) fn missed_since(key: &'static str, since: u64) -> bool {
    LAST_MISS.with(|l| l.borrow().get(key).is_some_and(|&n| n > since))
}

// END RUNTIME HELPERS

/// Makes this mapping the one engines that link `horizon-compat-core` alone call.
pub fn install() {
    install_fns(Fns {
        call: api::call,
        get_static: api::get_static,
        constructor_arg: neo::constructor_arg,
        fire_registrations: neo::fire_registrations,
        api_is_a: api_common::api_is_a,
    });
}
