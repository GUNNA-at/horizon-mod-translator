//! Minecraft's and NeoForge's API for mods the translator turned into Rust, mapped onto Horizon,
//! natively. The object model (values, heap, dispatch tables) is `horizon-compat-core`, re-exported
//! here: a translated engine needs only that crate, and calls this one through [`install`] when it
//! isn't inlined into the mod (so a mapping change rebuilds this crate, not the engine).
pub use horizon_compat_core::*;
pub use horizon_compat_core::host;

pub mod api;
pub mod api_common;
pub mod blocks;
pub mod glue;
pub mod jlang;
pub mod jutil;
pub mod nbt;
pub mod neo;
pub mod render;
pub mod shapes;
pub mod tags;

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
