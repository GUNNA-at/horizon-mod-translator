//! Key mappings as NeoForge 1.21.1 has them: `KeyMapping` with NeoForge's conflict contexts and
//! modifiers (`KeyConflictContext`, `KeyModifier`, `IKeyMappingExtension`),
//! `RegisterKeyMappingsEvent`, and the clicks a press gives the mappings bound to its key
//! (`KeyMapping.click` through `KeyMappingLookup.getAll`), which a mod takes with `consumeClick`.
//!
//! A mod's mapping (one its code made) is bound to the key the player chose for it on the bundle's
//! page in Options > Mods ([`Host::mod_key`](crate::host::Host::mod_key)), else to the key it was
//! made with; a mod setting one (`setKey`, `setKeyModifierAndCode`) wins for the session. Vanilla's
//! (`options.keyUse`) are the game's own, bound and held as the game says.

use crate::{Rt, V};

pub(crate) const KEY_MAPPING: &str = "net/minecraft/client/KeyMapping";
pub(crate) const REGISTER_EVENT: &str = "net/neoforged/neoforge/client/event/RegisterKeyMappingsEvent";
pub(crate) const MODIFIER: &str = "net/neoforged/neoforge/client/settings/KeyModifier";
pub(crate) const CONTEXT: &str = "net/neoforged/neoforge/client/settings/KeyConflictContext";
pub(crate) const ICONTEXT: &str = "net/neoforged/neoforge/client/settings/IKeyConflictContext";
const MODIFIER_ARRAY: &str = "[Lnet/neoforged/neoforge/client/settings/KeyModifier;";
const UNKNOWN: &str = "key.keyboard.unknown";
/// Every mapping the mods made, in the order made (`KeyMapping.ALL`): presses reach them.
const MADE: &str = "compat:key-mappings";
/// The mods heard `RegisterKeyMappingsEvent`.
const POSTED: &str = "compat:key-mappings-posted";
const CONTEXT_IS_ACTIVE: &str = "net/neoforged/neoforge/client/settings/IKeyConflictContext.isActive.()Z";
const CONTEXT_CONFLICTS: &str =
    "net/neoforged/neoforge/client/settings/IKeyConflictContext.conflicts.(Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;)Z";
const SUPPLIER_GET: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";

/// `KeyModifier`'s constants by ordinal.
const CONTROL: i32 = 0;
const SHIFT: i32 = 1;
const ALT: i32 = 2;
const NONE: i32 = 3;
const MODIFIERS: [&str; 4] = ["CONTROL", "SHIFT", "ALT", "NONE"];
/// `KeyModifier.MODIFIER_VALUES`: the order `getActiveModifier` asks them in.
const MODIFIER_VALUES: [i32; 3] = [SHIFT, CONTROL, ALT];
/// `KeyConflictContext`'s constants by ordinal.
const UNIVERSAL: i32 = 0;
const GUI: i32 = 1;
const IN_GAME: i32 = 2;
const CONTEXTS: [&str; 3] = ["UNIVERSAL", "GUI", "IN_GAME"];
/// Vanilla's categories in `KeyMapping.compareTo`'s order (`CATEGORY_SORT_ORDER`); others after.
const CATEGORY_ORDER: [&str; 7] = [
    "key.categories.movement",
    "key.categories.gameplay",
    "key.categories.inventory",
    "key.categories.creative",
    "key.categories.multiplayer",
    "key.categories.ui",
    "key.categories.misc",
];

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

fn int_field(rt: &Rt, o: V, field: &'static str, default: i32) -> i32 {
    match rt.get_field(o, field) {
        V::I(v) => v,
        _ => default,
    }
}

fn modifier(rt: &mut Rt, m: i32) -> V {
    let i = m.clamp(CONTROL, NONE);
    crate::api::api_enum(rt, MODIFIER, MODIFIERS[i as usize], i)
}

fn context(rt: &mut Rt, c: i32) -> V {
    let i = c.clamp(UNIVERSAL, IN_GAME);
    crate::api::api_enum(rt, CONTEXT, CONTEXTS[i as usize], i)
}

/// The `KeyModifier` constant `v` is, by ordinal (NONE for anything else).
fn modifier_of(rt: &Rt, v: V) -> i32 {
    if v.r() != 0 && rt.class_of(v) == MODIFIER { rt.get_field(v, "$ordinal").i() } else { NONE }
}

/// One of NeoForge's three contexts by ordinal (a mapping made without one is universal), or None
/// for a mod's own `IKeyConflictContext`.
fn builtin_context(rt: &Rt, ctx: V) -> Option<i32> {
    if ctx.r() == 0 {
        return Some(UNIVERSAL);
    }
    (rt.class_of(ctx) == CONTEXT).then(|| rt.get_field(ctx, "$ordinal").i())
}

/// Nothing over the world (`Minecraft.screen == null`): IN_GAME mappings act, GUI ones don't.
fn playing(rt: &Rt) -> bool {
    rt.host().is_none_or(|h| h.playing())
}

/// `IKeyConflictContext.isActive`.
fn context_active(rt: &mut Rt, ctx: V) -> bool {
    match builtin_context(rt, ctx) {
        Some(UNIVERSAL) => true,
        Some(GUI) => !playing(rt),
        Some(_) => playing(rt),
        None => rt.call_functional(CONTEXT_IS_ACTIVE, ctx, &[]).i() != 0,
    }
}

/// `a.conflicts(b)`: UNIVERSAL with any, the others with themselves.
fn conflicts(rt: &mut Rt, a: V, b: V) -> bool {
    match builtin_context(rt, a) {
        Some(UNIVERSAL) => true,
        Some(c) => builtin_context(rt, b) == Some(c),
        None => rt.call_functional(CONTEXT_CONFLICTS, a, &[b]).i() != 0,
    }
}

/// The key `KeyModifier` `m` holds down: on macOS NeoForge's CONTROL is Command (`Screen.hasControlDown`).
fn modifier_key(m: i32) -> Option<&'static str> {
    match m {
        CONTROL if cfg!(target_os = "macos") => Some("win"),
        CONTROL => Some("control"),
        SHIFT => Some("shift"),
        ALT => Some("alt"),
        _ => None,
    }
}

/// `KeyModifier.matches(key)`: `key` is one of the modifier's own keys.
fn modifier_matches(m: i32, key: &str) -> bool {
    modifier_key(m).is_some_and(|which| key.strip_prefix("key.keyboard.").is_some_and(|k| k == format!("left.{which}") || k == format!("right.{which}")))
}

/// `KeyModifier.isActive(context)`: its key held; NONE always, but for a context that doesn't
/// conflict with IN_GAME (GUI), only while no modifier is held.
fn modifier_active(rt: &mut Rt, m: i32, ctx: Option<V>) -> bool {
    if let Some(which) = modifier_key(m) {
        return crate::screens::held_either(rt, which);
    }
    let Some(ctx) = ctx else { return true };
    let in_game = context(rt, IN_GAME);
    if conflicts(rt, ctx, in_game) {
        return true;
    }
    !MODIFIER_VALUES.into_iter().any(|m| modifier_active(rt, m, None))
}

/// `KeyModifier.getActiveModifier()`: the first held of SHIFT, CONTROL, ALT, else NONE.
fn active_modifier(rt: &mut Rt) -> i32 {
    MODIFIER_VALUES.into_iter().find(|&m| modifier_active(rt, m, None)).unwrap_or(NONE)
}

/// A mapping's modifier now (`getKeyModifier`), NONE unless made or set with one.
fn key_modifier(rt: &Rt, k: V) -> i32 {
    int_field(rt, k, "$modifier", NONE)
}

/// A mapping's conflict context (`getKeyConflictContext`), null for UNIVERSAL.
fn key_context(rt: &Rt, k: V) -> V {
    match rt.get_field(k, "$context") {
        V::R(r) => V::R(r),
        _ => V::NULL,
    }
}

/// Whether `k` is a mod's own mapping (its constructor ran), not one of vanilla's the game binds.
fn own(rt: &Rt, k: V) -> bool {
    rt.str_of(rt.get_field(k, "$default")).is_some()
}

/// The key bound to mapping `k` (`key.keyboard.g`, `key.mouse.4`) and whether it is held now.
pub(crate) fn binding(rt: &Rt, k: V) -> (String, bool) {
    let name = rt.str_of(rt.get_field(k, "$name")).unwrap_or("");
    let key = match rt.str_of(rt.get_field(k, "$default")) {
        Some(default) => match rt.str_of(rt.get_field(k, "$key")) {
            Some(set) => set.to_owned(),
            None => rt.host().map_or_else(|| default.to_owned(), |h| h.mod_key(name, default)),
        },
        None => return rt.host().and_then(|h| h.mapping(name)).unwrap_or_else(|| (UNKNOWN.into(), false)),
    };
    let held = key != UNKNOWN && rt.host().is_some_and(|h| h.key_held(&key));
    (key, held)
}

/// `isConflictContextAndModifierActive()`.
fn context_and_modifier_active(rt: &mut Rt, k: V) -> bool {
    let ctx = key_context(rt, k);
    context_active(rt, ctx) && modifier_active(rt, key_modifier(rt, k), Some(ctx))
}

/// `isActiveAndMatches(key)`: bound to `key`, its context and modifier active.
fn active_and_matches(rt: &mut Rt, k: V, key: &str) -> bool {
    key != UNKNOWN && binding(rt, k).0 == key && context_and_modifier_active(rt, k)
}

fn made(rt: &Rt) -> Vec<V> {
    rt.interned.get(MADE).map_or_else(Vec::new, |&list| crate::jutil::elements(rt, list))
}

/// The mappings a press of `key` reaches (`KeyMappingLookup.getAll`): with a modifier held that
/// isn't `key` itself, the ones made with that modifier, or, when none is, those with none.
fn reached(rt: &mut Rt, key: &str) -> Vec<V> {
    let all = made(rt);
    let find = |rt: &mut Rt, m: i32| -> Vec<V> {
        let mut found = Vec::new();
        for &k in &all {
            if key_modifier(rt, k) == m && active_and_matches(rt, k, key) {
                found.push(k);
            }
        }
        found
    };
    let active = active_modifier(rt);
    if active != NONE && !modifier_matches(active, key) {
        let with = find(rt, active);
        if !with.is_empty() {
            return with;
        }
    }
    find(rt, NONE)
}

/// `RegisterKeyMappingsEvent`, once, as NeoForge posts it while the client's options are made (before
/// the client's setup).
pub fn post_register(rt: &mut Rt) {
    if rt.interned.contains_key(POSTED) {
        return;
    }
    rt.interned.insert(POSTED.into(), V::I(1));
    let ev = rt.new_object(REGISTER_EVENT);
    crate::neo::post(rt, REGISTER_EVENT, ev);
}

/// A key or mouse button went down or up (`key.keyboard.g`, `key.mouse.left`), as the window has
/// it (repeats left out). With nothing over the world a press clicks the mappings it reaches
/// (`KeyMapping.click`); vanilla clicks none while a screen is open.
pub fn key_input(rt: &mut Rt, key: &str, pressed: bool) {
    if !pressed || !playing(rt) {
        return;
    }
    post_register(rt);
    for k in reached(rt, key) {
        let n = int_field(rt, k, "$clicks", 0);
        rt.put_field(k, "$clicks", V::I(n.saturating_add(1)));
    }
}

/// The client's tick: a screen over the world lets go of every click (`KeyMapping.releaseAll`, as
/// opening a screen does).
pub fn tick(rt: &mut Rt) {
    if !playing(rt) {
        for k in made(rt) {
            rt.put_field(k, "$clicks", V::I(0));
        }
    }
}

/// The mods' registered mappings for the bundle's page in Options > Mods, in `KeyMapping.compareTo`'s
/// order: each one's name (`key.create.toolmenu`), label (its name translated) and default key.
pub fn page_keys(rt: &mut Rt) -> Vec<[String; 3]> {
    post_register(rt);
    let mut registered: Vec<V> = made(rt).into_iter().filter(|&k| int_field(rt, k, "$registered", 0) != 0).collect();
    registered.sort_by(|&a, &b| compare(rt, a, b));
    registered
        .into_iter()
        .map(|k| {
            let name = rt.str_of(rt.get_field(k, "$name")).unwrap_or("").to_owned();
            let label = translated(rt, &name);
            let default = rt.str_of(rt.get_field(k, "$default")).unwrap_or(UNKNOWN).to_owned();
            [name, label, default]
        })
        .collect()
}

/// `I18n.get(key)`: the key itself when the game has no translation.
fn translated(rt: &Rt, key: &str) -> String {
    rt.host().and_then(|h| h.translate(key)).unwrap_or_else(|| key.to_owned())
}

/// `KeyMapping.compareTo`: by category (vanilla's in their order, then the others by their
/// translations), then by translated name.
fn compare(rt: &Rt, a: V, b: V) -> std::cmp::Ordering {
    let category = |k: V| rt.str_of(rt.get_field(k, "$category")).unwrap_or("").to_owned();
    let (ca, cb) = (category(a), category(b));
    if ca == cb {
        let name = |k: V| translated(rt, rt.str_of(rt.get_field(k, "$name")).unwrap_or(""));
        return name(a).cmp(&name(b));
    }
    let order = |c: &str| CATEGORY_ORDER.iter().position(|o| *o == c);
    match (order(&ca), order(&cb)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => translated(rt, &ca).cmp(&translated(rt, &cb)),
    }
}

/// The key a mapping is shown as with its modifier (`KeyModifier.getCombinedName`): `SHIFT + Z`.
fn combined_text(rt: &Rt, m: i32, shown: &str) -> String {
    let (key, english) = match m {
        CONTROL if cfg!(target_os = "macos") => ("neoforge.controlsgui.control.mac", "CMD + %s"),
        CONTROL => ("neoforge.controlsgui.control", "CTRL + %s"),
        SHIFT => ("neoforge.controlsgui.shift", "SHIFT + %s"),
        ALT => ("neoforge.controlsgui.alt", "ALT + %s"),
        _ => return shown.to_owned(),
    };
    let format = rt.host().and_then(|h| h.translate(key)).unwrap_or_else(|| english.to_owned());
    format.replacen("%1$s", shown, 1).replacen("%s", shown, 1)
}

fn literal(rt: &mut Rt, text: &str) -> V {
    let s = rt.string(text);
    crate::api_common::component(rt, s, false, V::NULL)
}

/// What mapping `k` shows as (`getTranslatedKeyMessage`, `Component.keybind`): its key as vanilla's
/// Controls screen names it, with its modifier (`SHIFT + Z`).
pub(crate) fn display(rt: &Rt, k: V) -> String {
    let shown = crate::screens::key_display(rt, &binding(rt, k).0);
    combined_text(rt, key_modifier(rt, k), &shown)
}

/// `KeyMapping.<init>` in each of its forms: vanilla's `(name, [type,] code, category)`, NeoForge's
/// with a conflict context, a modifier or an `InputConstants.Key`. It keeps its name, the key it
/// starts on, its category, context and modifier (none when its key is the modifier's own), and
/// joins the mappings presses reach.
fn construct(rt: &mut Rt, desc: &str, args: &[V]) {
    let this = arg(args, 0);
    let params = crate::neo::desc_params(desc);
    let (mut key, mut mouse) = (UNKNOWN.to_owned(), false);
    let (mut ctx, mut m) = (V::NULL, NONE);
    for (i, p) in params.iter().enumerate() {
        let v = arg(args, i + 1);
        match p.as_str() {
            "Lcom/mojang/blaze3d/platform/InputConstants$Type;" => mouse = rt.get_field(v, "$ordinal").i() == 2,
            "I" => {
                key = if mouse {
                    crate::screens::mouse_name(v.i())
                } else {
                    crate::screens::key_name(v.i()).unwrap_or_else(|| UNKNOWN.into())
                }
            }
            "Lcom/mojang/blaze3d/platform/InputConstants$Key;" => key = rt.str_of(rt.get_field(v, "$name")).unwrap_or(UNKNOWN).to_owned(),
            "Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;" => ctx = v,
            "Lnet/neoforged/neoforge/client/settings/KeyModifier;" => m = modifier_of(rt, v),
            _ => {}
        }
    }
    let category = match params.iter().rposition(|p| p == "Ljava/lang/String;") {
        Some(i) if i > 0 => arg(args, i + 1),
        _ => V::NULL,
    };
    let k = rt.string(&key);
    rt.put_field(this, "$name", arg(args, 1));
    rt.put_field(this, "$default", k);
    rt.put_field(this, "$category", category);
    rt.put_field(this, "$context", ctx);
    rt.put_field(this, "$defaultModifier", V::I(m));
    rt.put_field(this, "$modifier", V::I(if modifier_matches(m, &key) { NONE } else { m }));
    let list = rt.intern(MADE.into(), |rt| crate::jutil::new_list(rt, Vec::new()));
    crate::jutil::list_add(rt, list, this);
}

/// `setKeyModifierAndCode(modifier, key)`: no modifier when the key is its own.
fn set_modifier_and_key(rt: &mut Rt, k: V, m: i32, key: &str) {
    let s = rt.string(key);
    rt.put_field(k, "$key", s);
    rt.put_field(k, "$modifier", V::I(if modifier_matches(m, key) { NONE } else { m }));
}

fn key_name_of(rt: &Rt, input_key: V) -> String {
    rt.str_of(rt.get_field(input_key, "$name")).unwrap_or(UNKNOWN).to_owned()
}

/// A call on `KeyMapping` (an instance's, or its statics), or None.
fn mapping_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "<init>" => {
            construct(rt, desc, args);
            V::Void
        }
        "getName" => rt.get_field(a(0), "$name"),
        "getCategory" => match rt.get_field(a(0), "$category") {
            c if c.r() != 0 => c,
            _ => rt.string("key.categories.gameplay"),
        },
        // Vanilla's as the game holds it; a mod's while nothing is over the world, with its context
        // and modifier active (`isDown && isConflictContextAndModifierActive()`).
        "isDown" => {
            let k = a(0);
            let (_, held) = binding(rt, k);
            let down = if own(rt, k) { held && playing(rt) && context_and_modifier_active(rt, k) } else { held };
            V::I(down as i32)
        }
        // A mod takes a click; a screen over the world let go of them (`releaseAll`).
        "consumeClick" => {
            let k = a(0);
            let n = int_field(rt, k, "$clicks", 0);
            if !playing(rt) {
                rt.put_field(k, "$clicks", V::I(0));
                V::I(0)
            } else if n > 0 {
                rt.put_field(k, "$clicks", V::I(n - 1));
                V::I(1)
            } else {
                V::I(0)
            }
        }
        "matches" => {
            let bound = binding(rt, a(0)).0;
            let keysym = crate::screens::key_name(a(1).i());
            V::I((a(1).i() != -1 && keysym.is_some_and(|n| n == bound)) as i32)
        }
        "matchesMouse" => V::I((binding(rt, a(0)).0 == crate::screens::mouse_name(a(1).i())) as i32),
        "isActiveAndMatches" => {
            let key = key_name_of(rt, a(1));
            V::I(active_and_matches(rt, a(0), &key) as i32)
        }
        "isConflictContextAndModifierActive" => V::I(context_and_modifier_active(rt, a(0)) as i32),
        "isUnbound" => V::I((binding(rt, a(0)).0 == UNKNOWN) as i32),
        "getKey" => {
            let bound = binding(rt, a(0)).0;
            crate::screens::input_key(rt, &bound)
        }
        "getDefaultKey" => {
            let default = match rt.str_of(rt.get_field(a(0), "$default")) {
                Some(d) => d.to_owned(),
                None => binding(rt, a(0)).0,
            };
            crate::screens::input_key(rt, &default)
        }
        "isDefault" => {
            let k = a(0);
            let default = rt.str_of(rt.get_field(k, "$default")).map(str::to_owned);
            let same_key = default.is_none_or(|d| binding(rt, k).0 == d);
            V::I((same_key && key_modifier(rt, k) == int_field(rt, k, "$defaultModifier", NONE)) as i32)
        }
        "saveString" => {
            let bound = binding(rt, a(0)).0;
            rt.string(&bound)
        }
        "setKey" => {
            let key = key_name_of(rt, a(1));
            let s = rt.string(&key);
            rt.put_field(a(0), "$key", s);
            V::Void
        }
        "setKeyModifierAndCode" => {
            let (m, key) = (modifier_of(rt, a(1)), key_name_of(rt, a(2)));
            set_modifier_and_key(rt, a(0), m, &key);
            V::Void
        }
        "setToDefault" => {
            let k = a(0);
            let m = int_field(rt, k, "$defaultModifier", NONE);
            let key = rt.str_of(rt.get_field(k, "$default")).unwrap_or(UNKNOWN).to_owned();
            set_modifier_and_key(rt, k, m, &key);
            V::Void
        }
        "getKeyModifier" => {
            let m = key_modifier(rt, a(0));
            modifier(rt, m)
        }
        "getDefaultKeyModifier" => {
            let m = int_field(rt, a(0), "$defaultModifier", NONE);
            modifier(rt, m)
        }
        "getKeyConflictContext" => match key_context(rt, a(0)) {
            c if c.r() != 0 => c,
            _ => context(rt, UNIVERSAL),
        },
        "setKeyConflictContext" => {
            rt.put_field(a(0), "$context", a(1));
            V::Void
        }
        // `same`: NeoForge's conflict rule (contexts that conflict, then modifiers and keys).
        "same" => V::I(same(rt, a(0), a(1)) as i32),
        "hasKeyModifierConflict" => V::I(modifier_conflict(rt, a(0), a(1)) as i32),
        "getTranslatedKeyMessage" => {
            let text = display(rt, a(0));
            literal(rt, &text)
        }
        "getDisplayName" => {
            let name = rt.get_field(a(0), "$name");
            crate::api_common::component(rt, name, true, V::NULL)
        }
        "compareTo" => V::I(compare(rt, a(0), a(1)) as i32),
        // Statics: `click` as a press does; holding is the game's; `releaseAll` drops the clicks.
        "click" => {
            let key = key_name_of(rt, a(0));
            for k in reached(rt, &key) {
                let n = int_field(rt, k, "$clicks", 0);
                rt.put_field(k, "$clicks", V::I(n.saturating_add(1)));
            }
            V::Void
        }
        "releaseAll" => {
            for k in made(rt) {
                rt.put_field(k, "$clicks", V::I(0));
            }
            V::Void
        }
        "set" | "setAll" | "resetMapping" | "resetToggleKeys" | "setDown" | "release" => V::Void,
        _ => return None,
    })
}

/// NeoForge's `KeyMapping.same(other)`.
fn same(rt: &mut Rt, a: V, b: V) -> bool {
    let (ca, cb) = (key_context(rt, a), key_context(rt, b));
    if conflicts(rt, ca, cb) || conflicts(rt, cb, ca) {
        let (ma, mb) = (key_modifier(rt, a), key_modifier(rt, b));
        let (ka, kb) = (binding(rt, a).0, binding(rt, b).0);
        if modifier_matches(ma, &kb) || modifier_matches(mb, &ka) {
            return true;
        }
        if ka == kb {
            let in_game = context(rt, IN_GAME);
            return ma == mb || (conflicts(rt, ca, in_game) && (ma == NONE || mb == NONE));
        }
    }
    binding(rt, a).0 == binding(rt, b).0
}

/// `IKeyMappingExtension.hasKeyModifierConflict(other)`.
fn modifier_conflict(rt: &mut Rt, a: V, b: V) -> bool {
    let (ca, cb) = (key_context(rt, a), key_context(rt, b));
    if conflicts(rt, ca, cb) || conflicts(rt, cb, ca) {
        let (ma, mb) = (key_modifier(rt, a), key_modifier(rt, b));
        return modifier_matches(ma, &binding(rt, b).0) || modifier_matches(mb, &binding(rt, a).0);
    }
    false
}

/// A call on `KeyModifier` (a constant's, or its statics), or None.
fn modifier_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "matches" => {
            let m = modifier_of(rt, a(0));
            let key = key_name_of(rt, a(1));
            V::I(modifier_matches(m, &key) as i32)
        }
        "isActive" => {
            let m = modifier_of(rt, a(0));
            let ctx = (a(1).r() != 0).then(|| a(1));
            V::I(modifier_active(rt, m, ctx) as i32)
        }
        "getCombinedName" => {
            let m = modifier_of(rt, a(0));
            let shown = rt.call_functional(SUPPLIER_GET, a(2), &[]);
            if m == NONE {
                return Some(shown);
            }
            let text: String = crate::gui::runs(rt, shown).iter().map(|r| r.text.as_str()).collect();
            let combined = combined_text(rt, m, &text);
            literal(rt, &combined)
        }
        "getActiveModifier" => {
            let m = active_modifier(rt);
            modifier(rt, m)
        }
        "getKeyModifier" => {
            let key = key_name_of(rt, a(0));
            let m = MODIFIER_VALUES.into_iter().find(|&m| modifier_matches(m, &key)).unwrap_or(NONE);
            modifier(rt, m)
        }
        "isKeyCodeModifier" => {
            let key = key_name_of(rt, a(0));
            V::I(MODIFIER_VALUES.into_iter().any(|m| modifier_matches(m, &key)) as i32)
        }
        "valueFromString" => {
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            let m = MODIFIERS.iter().position(|n| *n == text).map_or(NONE, |i| i as i32);
            modifier(rt, m)
        }
        _ => return None,
    })
}

/// The calls of this module: `KeyMapping`, `KeyModifier`, NeoForge's conflict contexts,
/// `RegisterKeyMappingsEvent`.
pub(crate) fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    match owner {
        KEY_MAPPING => mapping_call(rt, name, desc, args),
        MODIFIER => modifier_call(rt, name, args),
        CONTEXT | ICONTEXT => Some(match name {
            "isActive" => V::I(context_active(rt, arg(args, 0)) as i32),
            "conflicts" => V::I(conflicts(rt, arg(args, 0), arg(args, 1)) as i32),
            _ => return None,
        }),
        // `RegisterKeyMappingsEvent.register`: the mapping is listed with the game's options, so its
        // key can be chosen on the bundle's page.
        REGISTER_EVENT => Some(match name {
            "register" => {
                let k = arg(args, 1);
                rt.put_field(k, "$registered", V::I(1));
                V::Void
            }
            _ => return None,
        }),
        _ => None,
    }
}

/// `KeyModifier.MODIFIER_VALUES`.
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (MODIFIER, "MODIFIER_VALUES") => {
            let values: Vec<V> = MODIFIER_VALUES.into_iter().map(|m| modifier(rt, m)).collect();
            Some(rt.alloc(MODIFIER_ARRAY, crate::Data::Array(values)))
        }
        _ => None,
    }
}

/// The mod mapping named `name` (`Component.keybind`'s), if a mod made one.
pub(crate) fn mod_mapping(rt: &Rt, name: &str) -> Option<V> {
    made(rt).into_iter().find(|&k| rt.str_of(rt.get_field(k, "$name")) == Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const KEY_INIT: &str = "net/minecraft/client/KeyMapping.<init>.";

    fn made_with(rt: &mut Rt, desc: &str, args: &[V]) -> V {
        let k = rt.new_object(KEY_MAPPING);
        let key = Box::leak(format!("{KEY_INIT}{desc}").into_boxed_str());
        crate::api::call(rt, key, &[&[k], args].concat());
        k
    }

    fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    fn clicks(rt: &mut Rt, k: V) -> usize {
        (0..10).take_while(|_| call(rt, "net/minecraft/client/KeyMapping.consumeClick.()Z", &[k]).i() != 0).count()
    }

    /// Three mappings as mods make them: G in game (NeoForge's context form), SHIFT + Z anywhere
    /// (a modifier), M on screens (GUI); registered in the event, which the bundle's page lists in
    /// `compareTo`'s order with their translated names.
    fn three(rt: &mut Rt) -> [V; 3] {
        let category = rt.string("key.categories.glass");
        let in_game = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyConflictContext.IN_GAME");
        let gui = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyConflictContext.GUI");
        let shift = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyModifier.SHIFT");
        let universal = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyConflictContext.UNIVERSAL");
        let keysym = crate::api::get_static(rt, "com/mojang/blaze3d/platform/InputConstants$Type.KEYSYM");
        let ctx_form = "(Ljava/lang/String;Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;Lcom/mojang/blaze3d/platform/InputConstants$Type;ILjava/lang/String;)V";
        let name = rt.string("key.glass.open");
        let open = made_with(rt, ctx_form, &[name, in_game, keysym, V::I(71), category]);
        let name = rt.string("key.glass.zoom");
        let modifier_form = "(Ljava/lang/String;Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;Lnet/neoforged/neoforge/client/settings/KeyModifier;Lcom/mojang/blaze3d/platform/InputConstants$Type;ILjava/lang/String;)V";
        let zoom = made_with(rt, modifier_form, &[name, universal, shift, keysym, V::I(90), category]);
        let name = rt.string("key.glass.menu");
        let menu = made_with(rt, ctx_form, &[name, gui, keysym, V::I(77), category]);
        let listener = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        rt.listeners.push((REGISTER_EVENT.to_owned(), listener));
        let list = crate::jutil::new_list(rt, vec![open, zoom, menu]);
        rt.interned.insert("test:mappings".into(), list);
        [open, zoom, menu]
    }

    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        (id == 1).then(|| {
            let list = rt.interned["test:mappings"];
            for k in crate::jutil::elements(rt, list) {
                crate::api::call(rt, "net/neoforged/neoforge/client/event/RegisterKeyMappingsEvent.register.(Lnet/minecraft/client/KeyMapping;)V", &[args[1], k]);
            }
            V::Void
        })
    }

    fn host() -> MemoryHost {
        let mut h = MemoryHost::default();
        h.lang.insert("key.glass.open".into(), "Open the glass".into());
        h.lang.insert("key.glass.zoom".into(), "Zoom".into());
        h.lang.insert("key.glass.menu".into(), "Glass menu".into());
        h
    }

    /// Presses click the mappings bound to their key as NeoForge's `KeyMappingLookup` finds them, and
    /// a mod takes each click once: only with nothing over the world; a held modifier picks the
    /// mappings made with it (SHIFT + Z), else those without; the key the player chose on the
    /// bundle's page wins over the one the mapping was made with.
    #[test]
    fn presses_click_the_mappings_bound_to_their_key_as_neoforge_finds_them() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lambdas);
        let [open, zoom, menu] = three(&mut rt);
        let mut h = host();
        let keys = rt.with_host(&mut h, page_keys);
        assert_eq!(
            keys,
            [
                ["key.glass.menu", "Glass menu", "key.keyboard.m"],
                ["key.glass.open", "Open the glass", "key.keyboard.g"],
                ["key.glass.zoom", "Zoom", "key.keyboard.z"],
            ]
            .map(|r| r.map(str::to_owned))
        );
        rt.with_host(&mut h, |rt| {
            key_input(rt, "key.keyboard.g", true);
            key_input(rt, "key.keyboard.g", false);
            key_input(rt, "key.keyboard.g", true);
            assert_eq!(clicks(rt, open), 2, "each press, once");
            // Z without SHIFT reaches no mapping without a modifier: none is bound to it.
            key_input(rt, "key.keyboard.z", true);
            assert_eq!(clicks(rt, zoom), 0);
        });
        h.keys_held.push("key.keyboard.left.shift".into());
        rt.with_host(&mut h, |rt| {
            key_input(rt, "key.keyboard.z", true);
            assert_eq!(clicks(rt, zoom), 1, "SHIFT + Z");
            // With SHIFT held and no SHIFT + G mapping, G still reaches the one without.
            key_input(rt, "key.keyboard.g", true);
            assert_eq!(clicks(rt, open), 1);
            // The GUI mapping clicks never from a press (vanilla's are while playing only).
            key_input(rt, "key.keyboard.m", true);
            assert_eq!(clicks(rt, menu), 0);
        });
        h.keys_held.clear();
        // A screen over the world: no click, and isDown false.
        h.menu = true;
        h.keys_held.push("key.keyboard.g".into());
        rt.with_host(&mut h, |rt| {
            key_input(rt, "key.keyboard.g", true);
            assert_eq!(clicks(rt, open), 0);
            assert_eq!(call(rt, "net/minecraft/client/KeyMapping.isDown.()Z", &[open]).i(), 0);
            // On a screen the GUI mapping matches its key; the IN_GAME one doesn't.
            let m = crate::screens::input_key(rt, "key.keyboard.m");
            let g = crate::screens::input_key(rt, "key.keyboard.g");
            let matches = "net/minecraft/client/KeyMapping.isActiveAndMatches.(Lcom/mojang/blaze3d/platform/InputConstants$Key;)Z";
            assert_eq!(call(rt, matches, &[menu, m]).i(), 1);
            assert_eq!(call(rt, matches, &[open, g]).i(), 0);
        });
        h.menu = false;
        rt.with_host(&mut h, |rt| {
            assert_eq!(call(rt, "net/minecraft/client/KeyMapping.isDown.()Z", &[open]).i(), 1);
        });
        // The player chose H for it on the bundle's page.
        h.keys_held.clear();
        h.mod_keys.insert("key.glass.open".into(), "key.keyboard.h".into());
        rt.with_host(&mut h, |rt| {
            key_input(rt, "key.keyboard.g", true);
            assert_eq!(clicks(rt, open), 0);
            key_input(rt, "key.keyboard.h", true);
            assert_eq!(clicks(rt, open), 1);
            let key = call(rt, "net/minecraft/client/KeyMapping.getKey.()Lcom/mojang/blaze3d/platform/InputConstants$Key;", &[open]);
            assert_eq!(rt.str_of(rt.get_field(key, "$name")), Some("key.keyboard.h"));
            let default = call(rt, "net/minecraft/client/KeyMapping.getDefaultKey.()Lcom/mojang/blaze3d/platform/InputConstants$Key;", &[open]);
            assert_eq!(rt.str_of(rt.get_field(default, "$name")), Some("key.keyboard.g"));
            assert_eq!(call(rt, "net/minecraft/client/KeyMapping.isDefault.()Z", &[open]).i(), 0);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a mapping says of its key and modifier: `SHIFT + Z` (NeoForge's `getCombinedName`), its
    /// modifier and context back as NeoForge's constants, `same` by NeoForge's rule (a key shared by
    /// IN_GAME mappings conflicts when one has no modifier), and a mod setting its key.
    #[test]
    fn a_mapping_tells_its_key_modifier_and_conflicts_as_neoforge_does() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lambdas);
        let [open, zoom, menu] = three(&mut rt);
        let mut h = host();
        rt.with_host(&mut h, |rt| {
            let message = call(rt, "net/minecraft/client/KeyMapping.getTranslatedKeyMessage.()Lnet/minecraft/network/chat/Component;", &[zoom]);
            let text: String = crate::gui::runs(rt, message).iter().map(|r| r.text.as_str()).collect();
            assert_eq!(text, "SHIFT + Z");
            let m = call(rt, "net/minecraft/client/KeyMapping.getKeyModifier.()Lnet/neoforged/neoforge/client/settings/KeyModifier;", &[zoom]);
            assert_eq!(m, crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyModifier.SHIFT"));
            let c = call(rt, "net/minecraft/client/KeyMapping.getKeyConflictContext.()Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;", &[menu]);
            assert_eq!(c, crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyConflictContext.GUI"));
            let same = "net/minecraft/client/KeyMapping.same.(Lnet/minecraft/client/KeyMapping;)Z";
            let set_key = "net/minecraft/client/KeyMapping.setKey.(Lcom/mojang/blaze3d/platform/InputConstants$Key;)V";
            // G (IN_GAME) and the GUI mapping moved onto G: contexts that don't conflict still compare
            // their keys (NeoForge's last line, `this.key.equals(binding.key)`).
            let g = crate::screens::input_key(rt, "key.keyboard.g");
            call(rt, set_key, &[menu, g]);
            assert_eq!(call(rt, same, &[open, menu]).i(), 1);
            // SHIFT + Z (universal) and the GUI mapping on ALT + Z: one key under two modifiers.
            let z = crate::screens::input_key(rt, "key.keyboard.z");
            let alt = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyModifier.ALT");
            let set_both = "net/minecraft/client/KeyMapping.setKeyModifierAndCode.(Lnet/neoforged/neoforge/client/settings/KeyModifier;Lcom/mojang/blaze3d/platform/InputConstants$Key;)V";
            call(rt, set_both, &[menu, alt, z]);
            assert_eq!(call(rt, same, &[zoom, menu]).i(), 0);
            // SHIFT + Z and G moved onto Z with no modifier: in game, a modifier-less key conflicts.
            call(rt, set_key, &[open, z]);
            assert_eq!(call(rt, same, &[open, zoom]).i(), 1);
            // Back to its default: G, no modifier.
            call(rt, "net/minecraft/client/KeyMapping.setToDefault.()V", &[open]);
            assert_eq!(call(rt, "net/minecraft/client/KeyMapping.isDefault.()Z", &[open]).i(), 1);
            // A mapping made with SHIFT on Left Shift itself keeps no modifier (NeoForge's constructor).
            let shift = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyModifier.SHIFT");
            let universal = crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyConflictContext.UNIVERSAL");
            let keysym = crate::api::get_static(rt, "com/mojang/blaze3d/platform/InputConstants$Type.KEYSYM");
            let (name, category) = (rt.string("key.glass.sprint"), rt.string("key.categories.glass"));
            let form = "(Ljava/lang/String;Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;Lnet/neoforged/neoforge/client/settings/KeyModifier;Lcom/mojang/blaze3d/platform/InputConstants$Type;ILjava/lang/String;)V";
            let sprint = made_with(rt, form, &[name, universal, shift, keysym, V::I(340), category]);
            let m = call(rt, "net/minecraft/client/KeyMapping.getKeyModifier.()Lnet/neoforged/neoforge/client/settings/KeyModifier;", &[sprint]);
            assert_eq!(m, crate::api::get_static(rt, "net/neoforged/neoforge/client/settings/KeyModifier.NONE"));
            let d = call(rt, "net/minecraft/client/KeyMapping.getDefaultKeyModifier.()Lnet/neoforged/neoforge/client/settings/KeyModifier;", &[sprint]);
            assert_eq!(d, shift);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
