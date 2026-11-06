//! Minecraft's and NeoForge's API for mods the translator turned into Rust, mapped onto Horizon,
//! natively. The object model (values, heap, dispatch tables) is `horizon-compat-core`, re-exported
//! here: a translated engine needs only that crate, and calls this one through [`install`] when it
//! isn't inlined into the mod (so a mapping change rebuilds this crate, not the engine).
pub use horizon_compat_core::*;
pub use horizon_compat_core::host;

pub mod annotations;
pub mod api;
pub mod api_common;
pub mod attachments;
pub mod attributes;
pub mod blocks;
pub mod block_breaking;
pub mod loot_params;
pub mod breaks;
pub mod brigadier;
pub mod codecs;
pub mod command_args;
pub mod commands;
pub mod crafting;
pub mod handlers;
pub mod components;
pub mod container_screens;
pub mod capabilities;
pub mod enchantments;
pub mod entities;
pub mod events;
pub mod faces;
pub mod fake_players;
pub mod files;
pub mod convention;
pub mod creative;
pub mod crops;
pub mod fluid_wrappers;
pub mod item_wrappers;
pub mod fluids;
pub mod food;
pub mod item_use;
pub mod invoke;
pub mod input;
pub mod tooltips;
pub mod damage;
pub mod world_events;
pub mod datapack;
pub mod vanilla_entities;
pub mod functions;
pub mod gamerules;
pub mod glue;
pub mod gui;
pub mod gson;
pub mod hand;
pub mod jlang;
pub mod jregex;
pub mod jutil;
pub mod keys;
pub mod menus;
pub mod widgets;
pub mod edit_box;
pub mod level_env;
pub mod level_events;
pub mod made_entities;
pub mod mixin;
pub mod mob_effects;
pub mod mod_entities;
pub mod projectiles;
pub mod models;
pub mod entity_render;
pub mod camera;
pub mod client_extensions;
pub mod options;
pub mod vertices;
pub mod nbt;
pub mod ops;
pub mod neo;
pub mod neo_tags;
pub mod nightconfig;
pub mod network;
pub mod quads;
pub mod recipes;
pub mod palette;
pub mod particles;
pub mod records;
pub mod render;
pub mod resources;
pub mod rotation;
pub mod scan;
pub mod screens;
pub mod shapes;
pub mod sounds;
pub mod tags;
pub mod tanks;
pub mod vanilla_classes;
pub mod vanilla_effects;
pub mod vanilla_records;
pub mod vanilla_shapes;
pub mod vanilla_sounds;
pub mod vanilla_tag_keys;
pub mod vanilla_tags;
pub mod value_providers;
pub mod vitals;
pub mod worldgen;

// BEGIN RUNTIME HELPERS: bundles inline this part of the root into their `compat` module too.

/// An API key's owner, name and descriptor (`owner.name.desc`). The modules [`api::call`] asks in
/// turn each want them for the same key, and a call's own calls come back to it, so recent keys'
/// parts are kept, by the key's address (a third of a client tick went to splitting before; a
/// single kept key missed on every nested call).
pub(crate) fn parts(key: &'static str) -> Option<(&'static str, &'static str, &'static str)> {
    type Parts = Option<(&'static str, &'static str, &'static str)>;
    const SLOTS: usize = 256;
    thread_local! {
        static KEPT: std::cell::RefCell<[(usize, usize, Parts); SLOTS]> = const { std::cell::RefCell::new([(0, 0, None); SLOTS]) };
    }
    let (at, len) = (key.as_ptr() as usize, key.len());
    let slot = (at >> 3 ^ len) % SLOTS;
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        let (a, l, parts) = kept[slot];
        if a == at && l == len {
            return parts;
        }
        let parts = key.split_once('.').and_then(|(owner, rest)| rest.split_once('.').map(|(name, desc)| (owner, name, desc)));
        kept[slot] = (at, len, parts);
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
        vanilla_class: api_common::vanilla_class,
    });
}
