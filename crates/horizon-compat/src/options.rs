//! The client's options (`Minecraft.options`): each of `Options`' getters (`fov()`, `graphicsMode()`,
//! `renderDistance()`...) hands out one `OptionInstance` per option, holding the value vanilla 1.21.1
//! starts with until a mod sets it (`OptionInstance.set`), and the getters computed from them
//! (`getEffectiveRenderDistance`, `getCloudsType`, `getBackgroundOpacity`). A mod's own
//! `OptionInstance` (its constructor, `createBoolean`) holds its initial value and tells its
//! listener when it is set.
//!
//! The values are vanilla's defaults, not the game's settings: the game doesn't hand its options
//! over (the camera's field of view and render distance are vanilla's too, see `camera.rs`).
use crate::{api, Rt, V};

const OPTIONS: &str = "net/minecraft/client/Options";
const INSTANCE: &str = "net/minecraft/client/OptionInstance";
const TOOLTIP: &str = "net/minecraft/client/OptionInstance$TooltipSupplier";
const CONSUMER_ACCEPT: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";

/// What an option starts with.
#[derive(Clone, Copy)]
enum Initial {
    Bool(bool),
    Int(i32),
    Double(f64),
    /// An enum constant, by its static field (`net/minecraft/client/GraphicsStatus.FANCY`).
    Enum(&'static str),
    Str(&'static str),
}

use Initial::{Bool, Double, Enum, Int, Str};

/// Vanilla 1.21.1's options (`Options`' fields): the getter's name, the caption's translation key and
/// the initial value.
const DEFAULTS: [(&str, &str, Initial); 71] = [
    ("darkMojangStudiosBackground", "options.darkMojangStudiosBackgroundColor", Bool(false)),
    ("hideLightningFlash", "options.hideLightningFlashes", Bool(false)),
    ("hideSplashTexts", "options.hideSplashTexts", Bool(false)),
    ("sensitivity", "options.sensitivity", Double(0.5)),
    ("entityDistanceScaling", "options.entityDistanceScaling", Double(1.0)),
    ("framerateLimit", "options.framerateLimit", Int(120)),
    ("cloudStatus", "options.renderClouds", Enum("net/minecraft/client/CloudStatus.FANCY")),
    ("graphicsMode", "options.graphics", Enum("net/minecraft/client/GraphicsStatus.FANCY")),
    ("ambientOcclusion", "options.ao", Bool(true)),
    ("prioritizeChunkUpdates", "options.prioritizeChunkUpdates", Enum("net/minecraft/client/PrioritizeChunkUpdates.NONE")),
    ("chatVisibility", "options.chat.visibility", Enum("net/minecraft/world/entity/player/ChatVisiblity.FULL")),
    ("chatOpacity", "options.chat.opacity", Double(1.0)),
    ("chatLineSpacing", "options.chat.line_spacing", Double(0.0)),
    ("menuBackgroundBlurriness", "options.accessibility.menu_background_blurriness", Int(5)),
    ("textBackgroundOpacity", "options.accessibility.text_background_opacity", Double(0.5)),
    ("panoramaSpeed", "options.accessibility.panorama_speed", Double(1.0)),
    ("highContrast", "options.accessibility.high_contrast", Bool(false)),
    ("narratorHotkey", "options.accessibility.narrator_hotkey", Bool(true)),
    ("mainHand", "options.mainHand", Enum("net/minecraft/world/entity/HumanoidArm.RIGHT")),
    ("chatScale", "options.chat.scale", Double(1.0)),
    ("chatWidth", "options.chat.width", Double(1.0)),
    // `ChatComponent.defaultUnfocusedPct()`: 70 of the focused chat's 160 lines' pixels.
    ("chatHeightUnfocused", "options.chat.height.unfocused", Double(0.4375)),
    ("chatHeightFocused", "options.chat.height.focused", Double(1.0)),
    ("chatDelay", "options.chat.delay_instant", Double(0.0)),
    ("notificationDisplayTime", "options.notifications.display_time", Double(1.0)),
    ("mipmapLevels", "options.mipmapLevels", Int(4)),
    ("attackIndicator", "options.attackIndicator", Enum("net/minecraft/client/AttackIndicatorStatus.CROSSHAIR")),
    ("biomeBlendRadius", "options.biomeBlendRadius", Int(2)),
    // `logMouse(0)`: 10 to the power 0.
    ("mouseWheelSensitivity", "options.mouseWheelSensitivity", Double(1.0)),
    ("rawMouseInput", "options.rawMouseInput", Bool(true)),
    ("autoJump", "options.autoJump", Bool(false)),
    ("operatorItemsTab", "options.operatorItemsTab", Bool(false)),
    ("autoSuggestions", "options.autoSuggestCommands", Bool(true)),
    ("chatColors", "options.chat.color", Bool(true)),
    ("chatLinks", "options.chat.links", Bool(true)),
    ("chatLinksPrompt", "options.chat.links.prompt", Bool(true)),
    ("enableVsync", "options.vsync", Bool(true)),
    ("entityShadows", "options.entityShadows", Bool(true)),
    ("forceUnicodeFont", "options.forceUnicodeFont", Bool(false)),
    ("japaneseGlyphVariants", "options.japaneseGlyphVariants", Bool(false)),
    ("invertYMouse", "options.invertMouse", Bool(false)),
    ("discreteMouseScroll", "options.discrete_mouse_scroll", Bool(false)),
    ("realmsNotifications", "options.realmsNotifications", Bool(true)),
    ("allowServerListing", "options.allowServerListing", Bool(true)),
    ("reducedDebugInfo", "options.reducedDebugInfo", Bool(false)),
    ("showSubtitles", "options.showSubtitles", Bool(false)),
    ("directionalAudio", "options.directionalAudio", Bool(false)),
    ("backgroundForChatOnly", "options.accessibility.text_background", Bool(true)),
    ("touchscreen", "options.touchscreen", Bool(false)),
    ("fullscreen", "options.fullscreen", Bool(false)),
    ("bobView", "options.viewBobbing", Bool(true)),
    ("toggleCrouch", "key.sneak", Bool(false)),
    ("toggleSprint", "key.sprint", Bool(false)),
    ("hideMatchedNames", "options.hideMatchedNames", Bool(true)),
    ("showAutosaveIndicator", "options.autosaveIndicator", Bool(true)),
    ("onlyShowSecureChat", "options.onlyShowSecureChat", Bool(false)),
    ("fov", "options.fov", Int(70)),
    ("telemetryOptInExtra", "options.telemetry.button", Bool(false)),
    ("screenEffectScale", "options.screenEffectScale", Double(1.0)),
    ("fovEffectScale", "options.fovEffectScale", Double(1.0)),
    ("darknessEffectScale", "options.darknessEffectScale", Double(1.0)),
    ("glintSpeed", "options.glintSpeed", Double(0.5)),
    ("glintStrength", "options.glintStrength", Double(0.75)),
    ("damageTiltStrength", "options.damageTiltStrength", Double(1.0)),
    ("gamma", "options.gamma", Double(0.5)),
    ("guiScale", "options.guiScale", Int(0)),
    ("particles", "options.particles", Enum("net/minecraft/client/ParticleStatus.ALL")),
    ("narrator", "options.narrator", Enum("net/minecraft/client/NarratorStatus.OFF")),
    ("soundDevice", "options.audioDevice", Str("")),
    ("renderDistance", "options.renderDistance", Int(12)),
    ("simulationDistance", "options.simulationDistance", Int(12)),
];

/// `Options`' methods computed from the options (beyond their getters).
const DERIVED: [&str; 11] = [
    "getEffectiveRenderDistance",
    "getSoundSourceVolume",
    "getSoundSourceOptionInstance",
    "getBackgroundOpacity",
    "getBackgroundColor",
    "getCloudsType",
    "getMenuBackgroundBlurriness",
    "isModelPartEnabled",
    "useNativeTransport",
    "save",
    "genericValueLabel",
];

fn boxed(rt: &mut Rt, initial: Initial) -> V {
    match initial {
        Bool(b) => crate::mixin::box_value(rt, V::I(i32::from(b)), 'Z'),
        Int(i) => crate::mixin::box_value(rt, V::I(i), 'I'),
        Double(d) => crate::mixin::box_value(rt, V::D(d), 'D'),
        Enum(field) => api::get_static(rt, field),
        Str(s) => rt.string(s),
    }
}

/// Option `name`'s one `OptionInstance` (vanilla's, by its getter's name), made with its initial
/// value the first time it is asked for.
fn option(rt: &mut Rt, name: &str) -> Option<V> {
    let &(_, caption, initial) = DEFAULTS.iter().find(|(n, _, _)| *n == name)?;
    Some(rt.intern(format!("compat:option/{name}"), |rt| {
        let o = rt.new_object(INSTANCE);
        let c = rt.string(caption);
        rt.put_field(o, "$caption", c);
        let v = boxed(rt, initial);
        rt.put_field(o, "$value", v);
        o
    }))
}

/// Option `name`'s value as it is now (unboxed: an int, a double, a boolean as 0 or 1).
fn value(rt: &mut Rt, name: &str) -> V {
    let Some(o) = option(rt, name) else { return V::NULL };
    let v = rt.get_field(o, "$value");
    match rt.class_of(v) {
        "java/lang/Boolean" | "java/lang/Integer" | "java/lang/Double" => rt.get_field(v, "value"),
        _ => v,
    }
}

/// `Options.getSoundSourceOptionInstance(source)`: one option a sound source, at full volume.
fn sound_option(rt: &mut Rt, source: V) -> V {
    let name = rt.str_of(rt.get_field(source, "$name")).unwrap_or("MASTER").to_ascii_lowercase();
    rt.intern(format!("compat:option/sound/{name}"), |rt| {
        let o = rt.new_object(INSTANCE);
        let c = rt.string(&format!("soundCategory.{name}"));
        rt.put_field(o, "$caption", c);
        let v = crate::mixin::box_value(rt, V::D(1.0), 'D');
        rt.put_field(o, "$value", v);
        o
    })
}

fn options_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if desc == "()Lnet/minecraft/client/OptionInstance;" {
        return option(rt, name);
    }
    Some(match name {
        // No server limits the client's render distance here.
        "getEffectiveRenderDistance" => value(rt, "renderDistance"),
        "getSoundSourceOptionInstance" => sound_option(rt, a(1)),
        "getSoundSourceVolume" => {
            let o = sound_option(rt, a(1));
            let v = rt.get_field(o, "$value");
            V::F(rt.get_field(v, "value").d() as f32)
        }
        // A text's background: the given opacity, or the option's where it is drawn everywhere.
        "getBackgroundOpacity" => background_opacity(rt, a(1).f()),
        "getBackgroundColor" if desc.starts_with("(F)") => {
            let opacity = background_opacity(rt, a(1).f()).f();
            V::I(((opacity * 255.0) as i32) << 24 & 0xFF00_0000_u32 as i32)
        }
        "getBackgroundColor" => {
            if value(rt, "backgroundForChatOnly").i() != 0 {
                a(1)
            } else {
                V::I(((value(rt, "textBackgroundOpacity").d() * 255.0) as i32) << 24 & 0xFF00_0000_u32 as i32)
            }
        }
        "getCloudsType" => {
            if value(rt, "renderDistance").i() >= 4 {
                value(rt, "cloudStatus")
            } else {
                api::get_static(rt, "net/minecraft/client/CloudStatus.OFF")
            }
        }
        "getMenuBackgroundBlurriness" => value(rt, "menuBackgroundBlurriness"),
        "isModelPartEnabled" | "useNativeTransport" => V::I(1),
        // The options file is the game's.
        "save" => V::Void,
        // `options.generic_value`: "<caption>: <value>".
        "genericValueLabel" => {
            let key = rt.string("options.generic_value");
            let parts = rt.alloc("[Ljava/lang/Object;", crate::Data::Array(vec![a(0), a(1)]));
            api::call(rt, "net/minecraft/network/chat/Component.translatable.(Ljava/lang/String;[Ljava/lang/Object;)Lnet/minecraft/network/chat/MutableComponent;", &[key, parts])
        }
        _ => return None,
    })
}

fn background_opacity(rt: &mut Rt, opacity: f32) -> V {
    if value(rt, "backgroundForChatOnly").i() != 0 {
        V::F(opacity)
    } else {
        V::F(value(rt, "textBackgroundOpacity").d() as f32)
    }
}

/// The argument of a boolean option's factory (`createBoolean(caption, [tooltip,] [toString,]
/// initial, [listener])`) that is its initial value: the descriptor's one `Z`.
fn boolean_initial(desc: &str, args: &[V]) -> V {
    let params = desc.trim_start_matches('(').split(')').next().unwrap_or("");
    let mut i = 0;
    let mut chars = params.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'Z' => return args.get(i).copied().unwrap_or(V::I(0)),
            'L' => {
                for d in chars.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
            }
            '[' => continue,
            _ => {}
        }
        i += 1;
    }
    V::I(0)
}

fn instance_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
        // A mod's option: `(caption, tooltip, toString, values, [codec,] initialValue, onValueUpdate)`.
        "<init>" => {
            let n = args.len();
            rt.put_field(this, "$caption", a(1));
            if n >= 3 {
                rt.put_field(this, "$value", a(n - 2));
                rt.put_field(this, "$onValueUpdate", a(n - 1));
            }
            V::Void
        }
        "createBoolean" => {
            let o = rt.new_object(INSTANCE);
            rt.put_field(o, "$caption", a(0));
            let initial = boolean_initial(desc, args);
            let v = crate::mixin::box_value(rt, V::I(i32::from(initial.i() != 0)), 'Z');
            rt.put_field(o, "$value", v);
            if desc.contains("Ljava/util/function/Consumer;)") {
                rt.put_field(o, "$onValueUpdate", a(args.len() - 1));
            }
            o
        }
        "get" => rt.get_field(this, "$value"),
        // The new value, then its listener (a mod's own option's).
        "set" => {
            rt.put_field(this, "$value", a(1));
            let listener = rt.get_field(this, "$onValueUpdate");
            if listener.r() != 0 {
                rt.call_functional(CONSUMER_ACCEPT, listener, &[a(1)]);
            }
            V::Void
        }
        "noTooltip" => rt.intern("compat:option-tooltip/none".into(), |rt| rt.new_object(TOOLTIP)),
        "cachedConstantTooltip" => {
            let t = rt.new_object(TOOLTIP);
            rt.put_field(t, "$tooltip", a(0));
            t
        }
        "toString" => rt.get_field(this, "$caption"),
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        OPTIONS => (desc == "()Lnet/minecraft/client/OptionInstance;" && DEFAULTS.iter().any(|(n, _, _)| *n == name)) || DERIVED.contains(&name),
        INSTANCE => matches!(name, "<init>" | "createBoolean" | "get" | "set" | "noTooltip" | "cachedConstantTooltip" | "toString"),
        _ => false,
    }
}

/// An options call, or None when this module doesn't model it.
pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        OPTIONS => options_call(rt, name, desc, args),
        INSTANCE => instance_call(rt, name, desc, args),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    fn option_of(rt: &mut Rt, getter: &str) -> V {
        let options = rt.new_object(OPTIONS);
        let key: &'static str = Box::leak(format!("{OPTIONS}.{getter}.()Lnet/minecraft/client/OptionInstance;").into_boxed_str());
        c(rt, key, &[options])
    }

    fn get(rt: &mut Rt, o: V) -> V {
        c(rt, "net/minecraft/client/OptionInstance.get.()Ljava/lang/Object;", &[o])
    }

    fn set(rt: &mut Rt, o: V, v: V) {
        c(rt, "net/minecraft/client/OptionInstance.set.(Ljava/lang/Object;)V", &[o, v]);
    }

    /// Vanilla's options start at 1.21.1's defaults, an enum option at the very constant its class
    /// holds (mods compare them by identity), and each getter hands out one option.
    #[test]
    fn options_start_at_vanillas_defaults() {
        let mut rt = Rt::new();
        let fov = option_of(&mut rt, "fov");
        let v = get(&mut rt, fov);
        assert_eq!(rt.class_of(v), "java/lang/Integer");
        assert_eq!(rt.get_field(v, "value"), V::I(70));
        assert_eq!(option_of(&mut rt, "fov"), fov);
        let graphics = option_of(&mut rt, "graphicsMode");
        let fancy = api::get_static(&mut rt, "net/minecraft/client/GraphicsStatus.FANCY");
        assert_eq!(get(&mut rt, graphics), fancy);
        let hand = option_of(&mut rt, "mainHand");
        let right = api::get_static(&mut rt, "net/minecraft/world/entity/HumanoidArm.RIGHT");
        assert_eq!(get(&mut rt, hand), right);
        let gamma = option_of(&mut rt, "gamma");
        let g = get(&mut rt, gamma);
        assert_eq!(rt.get_field(g, "value"), V::D(0.5));
    }

    /// Setting an option changes what it and the getters computed from it answer.
    #[test]
    fn a_set_option_changes_the_getters_computed_from_it() {
        let mut rt = Rt::new();
        let options = rt.new_object(OPTIONS);
        let distance = c(&mut rt, "net/minecraft/client/Options.getEffectiveRenderDistance.()I", &[options]);
        assert_eq!(distance, V::I(12));
        let clouds = "net/minecraft/client/Options.getCloudsType.()Lnet/minecraft/client/CloudStatus;";
        let fancy = api::get_static(&mut rt, "net/minecraft/client/CloudStatus.FANCY");
        assert_eq!(c(&mut rt, clouds, &[options]), fancy);
        let render = option_of(&mut rt, "renderDistance");
        let three = crate::mixin::box_value(&mut rt, V::I(3), 'I');
        set(&mut rt, render, three);
        assert_eq!(c(&mut rt, "net/minecraft/client/Options.getEffectiveRenderDistance.()I", &[options]), V::I(3));
        let off = api::get_static(&mut rt, "net/minecraft/client/CloudStatus.OFF");
        assert_eq!(c(&mut rt, clouds, &[options]), off);
        // A chat-only background keeps the opacity asked for; everywhere, the option's.
        let opacity = "net/minecraft/client/Options.getBackgroundOpacity.(F)F";
        assert_eq!(c(&mut rt, opacity, &[options, V::F(0.25)]), V::F(0.25));
        let chat_only = option_of(&mut rt, "backgroundForChatOnly");
        let no = crate::mixin::box_value(&mut rt, V::I(0), 'Z');
        set(&mut rt, chat_only, no);
        assert_eq!(c(&mut rt, opacity, &[options, V::F(0.25)]), V::F(0.5));
        assert_eq!(c(&mut rt, "net/minecraft/client/Options.getBackgroundColor.(F)I", &[options, V::F(0.25)]), V::I(0x7F00_0000));
    }

    /// A mod's own boolean option holds its initial value, whichever factory made it.
    #[test]
    fn a_mods_boolean_option_holds_its_initial_value() {
        let mut rt = Rt::new();
        let caption = rt.string("options.mymod.sparkles");
        let tooltip = c(&mut rt, "net/minecraft/client/OptionInstance.noTooltip.()Lnet/minecraft/client/OptionInstance$TooltipSupplier;", &[]);
        let o = c(
            &mut rt,
            "net/minecraft/client/OptionInstance.createBoolean.(Ljava/lang/String;Lnet/minecraft/client/OptionInstance$TooltipSupplier;Z)Lnet/minecraft/client/OptionInstance;",
            &[caption, tooltip, V::I(1)],
        );
        let v = get(&mut rt, o);
        assert_eq!((rt.class_of(v), rt.get_field(v, "value")), ("java/lang/Boolean", V::I(1)));
        let o = c(&mut rt, "net/minecraft/client/OptionInstance.createBoolean.(Ljava/lang/String;Z)Lnet/minecraft/client/OptionInstance;", &[caption, V::I(0)]);
        let v = get(&mut rt, o);
        assert_eq!(rt.get_field(v, "value"), V::I(0));
    }
}
