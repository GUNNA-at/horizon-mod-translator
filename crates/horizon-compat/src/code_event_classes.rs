//! The classes of the game's the runtime makes by their own code ([`crate::code_events`]): its
//! events, and who plays. Each class, and the methods of it the runtime calls itself (what a
//! listener may have set in an event it heard). The mod translator reads this list: a mod that
//! names one of these (a listener takes its event) carries the class's code from the player's
//! NeoForge, with its constructors and these methods (`horizon_mod_translator::reach`).
//!
//! A class named here is not one the runtime models: the translator's list of the classes the
//! runtime names leaves this file out, as it does the table of every class.
pub const CLASSES: &[(&str, &[&str])] = &[
    ("net/minecraft/client/User", &[]),
    ("net/neoforged/neoforge/event/entity/EntityMountEvent", &[]),
    ("net/neoforged/neoforge/event/entity/living/LivingEquipmentChangeEvent", &[]),
    ("net/neoforged/neoforge/event/entity/player/AdvancementEvent$AdvancementEarnEvent", &[]),
    ("net/neoforged/neoforge/event/entity/player/PlayerEvent$ItemCraftedEvent", &[]),
    ("net/neoforged/neoforge/event/entity/player/PlayerEvent$PlayerChangedDimensionEvent", &[]),
    ("net/neoforged/neoforge/event/entity/player/PlayerEvent$StartTracking", &[]),
    ("net/neoforged/neoforge/event/entity/player/PlayerEvent$StopTracking", &[]),
    ("net/neoforged/neoforge/event/level/ChunkEvent$Load", &[]),
    ("net/neoforged/neoforge/event/level/ChunkEvent$Unload", &[]),
    ("net/neoforged/neoforge/event/level/SleepFinishedTimeEvent", &["getNewTime.()J"]),
];
