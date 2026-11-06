//! The client's GUI for mods, natively. What a mod draws with `GuiGraphics` (fills and gradients,
//! textures and sprites, text, items, tooltips) goes to the host's `gui_*` calls in GUI pixels,
//! through the graphics' pose. Text is drawn as styled runs ([`runs`]): a `String` with its `§`
//! codes, or a component translated into the player's language with its arguments and siblings in
//! place; `Font` measures and splits them as vanilla's `StringSplitter` does. NeoForge's HUD layers
//! (`RegisterGuiLayersEvent`) are kept in vanilla's order and drawn next to the game's own layers
//! ([`draw_layers`]).
use crate::host::TextRun;
use crate::render::{self, M4};
use crate::{Data, Rt, V};

const GRAPHICS: &str = "net/minecraft/client/gui/GuiGraphics";
const FONT: &str = "net/minecraft/client/gui/Font";
const SPLITTER: &str = "net/minecraft/client/StringSplitter";
/// Text in visual order (`FormattedCharSequence`), and text pieces (`FormattedText`) this module
/// makes: `$of` the text they stand for, `$parts` (a list) when made of several, `$style` the style
/// over it.
const SEQUENCE: &str = "net/minecraft/util/FormattedCharSequence";
const FORMATTED: &str = "net/minecraft/network/chat/FormattedText";
const STYLE: &str = "net/minecraft/network/chat/Style";
const TEXT_COLOR: &str = "net/minecraft/network/chat/TextColor";
const COMPONENT: &str = "net/minecraft/network/chat/Component";
const MUTABLE: &str = "net/minecraft/network/chat/MutableComponent";
const FORMATTING: &str = "net/minecraft/ChatFormatting";
const LAYERS_EVENT: &str = "net/neoforged/neoforge/client/event/RegisterGuiLayersEvent";
const LAYER_RENDER: &str = "net/minecraft/client/gui/LayeredDraw$Layer.render.(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/client/DeltaTracker;)V";
const TOOLTIP: &str = "net/minecraft/client/gui/screens/inventory/tooltip/ClientTooltipComponent";
const TEXT_TOOLTIP: &str = "net/minecraft/client/gui/screens/inventory/tooltip/ClientTextTooltip";
const TOOLTIP_WIDTH: &str = "net/minecraft/client/gui/screens/inventory/tooltip/ClientTooltipComponent.getWidth.(Lnet/minecraft/client/gui/Font;)I";
const TOOLTIP_HEIGHT: &str = "net/minecraft/client/gui/screens/inventory/tooltip/ClientTooltipComponent.getHeight.()I";
const TOOLTIP_TEXT: &str = "net/minecraft/client/gui/screens/inventory/tooltip/ClientTooltipComponent.renderText.(Lnet/minecraft/client/gui/Font;IILorg/joml/Matrix4f;Lnet/minecraft/client/renderer/MultiBufferSource$BufferSource;)V";
const TOOLTIP_IMAGE: &str = "net/minecraft/client/gui/screens/inventory/tooltip/ClientTooltipComponent.renderImage.(Lnet/minecraft/client/gui/Font;IILnet/minecraft/client/gui/GuiGraphics;)V";
const POSITIONER: &str = "net/minecraft/client/gui/screens/inventory/tooltip/DefaultTooltipPositioner";
const TOOLTIP_EVENT: &str = "net/neoforged/neoforge/client/event/RenderTooltipEvent";
const TOOLTIP_PRE: &str = "net/neoforged/neoforge/client/event/RenderTooltipEvent$Pre";
const TOOLTIP_COLOR: &str = "net/neoforged/neoforge/client/event/RenderTooltipEvent$Color";
const TOOLTIP_GATHER: &str = "net/neoforged/neoforge/client/event/RenderTooltipEvent$GatherComponents";
const WINDOW: &str = "com/mojang/blaze3d/platform/Window";
const MINECRAFT: &str = "net/minecraft/client/Minecraft";
const LANGUAGE_MANAGER: &str = "net/minecraft/client/resources/language/LanguageManager";
const PARTICLE_ENGINE: &str = "net/minecraft/client/particle/ParticleEngine";
const GAME_MODE: &str = "net/minecraft/client/multiplayer/MultiPlayerGameMode";
const GAME_TYPE: &str = "net/minecraft/world/level/GameType";
const GAME_TYPES: [&str; 4] = ["SURVIVAL", "CREATIVE", "ADVENTURE", "SPECTATOR"];
/// The drawing flag: set while a mod draws on the GUI, so text and items drawn through a pose go
/// to the GUI, not the world.
const DRAWING: &str = "compat:gui-drawing";
/// The tint blits take (`RenderSystem.setShaderColor`), `0xAARRGGBB`.
const TINT: &str = "compat:gui-tint";

/// The default tooltip's colours (`TooltipRenderUtil`): background, border top and bottom.
const TOOLTIP_BACKGROUND: i32 = 0xF010_0010_u32 as i32;
const TOOLTIP_BORDER_TOP: i32 = 0x5050_00FF;
const TOOLTIP_BORDER_BOTTOM: i32 = 0x5028_007F;

/// Vanilla's line height (`Font.lineHeight`).
pub const LINE_HEIGHT: i32 = 9;

/// `ChatFormatting`: each constant's name, `§` code and colour (`0xRRGGBB`; None for a format).
const FORMATS: [(&str, char, Option<u32>); 22] = [
    ("BLACK", '0', Some(0x000000)),
    ("DARK_BLUE", '1', Some(0x0000AA)),
    ("DARK_GREEN", '2', Some(0x00AA00)),
    ("DARK_AQUA", '3', Some(0x00AAAA)),
    ("DARK_RED", '4', Some(0xAA0000)),
    ("DARK_PURPLE", '5', Some(0xAA00AA)),
    ("GOLD", '6', Some(0xFFAA00)),
    ("GRAY", '7', Some(0xAAAAAA)),
    ("DARK_GRAY", '8', Some(0x555555)),
    ("BLUE", '9', Some(0x5555FF)),
    ("GREEN", 'a', Some(0x55FF55)),
    ("AQUA", 'b', Some(0x55FFFF)),
    ("RED", 'c', Some(0xFF5555)),
    ("LIGHT_PURPLE", 'd', Some(0xFF55FF)),
    ("YELLOW", 'e', Some(0xFFFF55)),
    ("WHITE", 'f', Some(0xFFFFFF)),
    ("OBFUSCATED", 'k', None),
    ("BOLD", 'l', None),
    ("STRIKETHROUGH", 'm', None),
    ("UNDERLINE", 'n', None),
    ("ITALIC", 'o', None),
    ("RESET", 'r', None),
];

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

// ---- styles ----
//
// A component's style is its `$style` field: tokens joined by commas, applied in order:
// `ChatFormatting` names (`RED`, `BOLD`), `#RRGGBB` for a colour, `#` for none, `!BOLD` to turn a
// format off. A `Style` object carries the same tokens.

/// A style as text is drawn with: a colour (None takes the colour the text is drawn in) and
/// formats.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct St {
    color: Option<u32>,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

impl St {
    fn apply(mut self, tokens: &str) -> St {
        for t in tokens.split(',').filter(|t| !t.is_empty()) {
            let (on, name) = t.strip_prefix('!').map_or((true, t), |n| (false, n));
            match name {
                "BOLD" => self.bold = on,
                "ITALIC" => self.italic = on,
                "UNDERLINE" => self.underlined = on,
                "STRIKETHROUGH" => self.strikethrough = on,
                "OBFUSCATED" => self.obfuscated = on,
                "RESET" => self = St::default(),
                "#" => self.color = None,
                n => {
                    if let Some(c) = color_named(n) {
                        self.color = Some(c);
                    }
                }
            }
        }
        self
    }

    /// A `§` code's change (`StringDecomposer.iterateFormatted`): `r` goes back to `reset`, a colour
    /// clears the formats (`Style.applyLegacyFormat`), a format turns on.
    fn legacy(self, code: char, reset: St) -> St {
        let code = code.to_ascii_lowercase();
        let Some(&(name, _, color)) = FORMATS.iter().find(|f| f.1 == code) else { return self };
        match (name, color) {
            ("RESET", _) => reset,
            (_, Some(c)) => St { color: Some(c), ..St::default() },
            (n, None) => self.apply(n),
        }
    }

    fn run(&self, text: String) -> TextRun {
        TextRun {
            text,
            color: self.color,
            bold: self.bold,
            italic: self.italic,
            underlined: self.underlined,
            strikethrough: self.strikethrough,
        }
    }
}

/// The style of a drawn run, as tokens.
fn tokens_of(r: &TextRun) -> String {
    let mut t = String::new();
    if let Some(c) = r.color {
        t.push_str(&format!("#{c:06X},"));
    }
    for (on, name) in [(r.bold, "BOLD"), (r.italic, "ITALIC"), (r.underlined, "UNDERLINE"), (r.strikethrough, "STRIKETHROUGH")] {
        if on {
            t.push_str(name);
            t.push(',');
        }
    }
    t
}

/// A colour token's colour: a `ChatFormatting` colour's name, or `#RRGGBB`.
fn color_named(name: &str) -> Option<u32> {
    if let Some(hex) = name.strip_prefix('#') {
        return u32::from_str_radix(hex, 16).ok();
    }
    FORMATS.iter().find(|f| f.0 == name).and_then(|f| f.2)
}

fn formatting_name(rt: &Rt, v: V) -> String {
    if v.r() == 0 {
        return String::new();
    }
    rt.str_of(rt.get_field(v, "$name")).unwrap_or("").to_owned()
}

fn style_tokens(rt: &Rt, v: V) -> String {
    rt.str_of(rt.get_field(v, "$style")).unwrap_or("").to_owned()
}

/// A `Style` with `tokens`.
fn style_object(rt: &mut Rt, tokens: &str) -> V {
    let s = rt.new_object(STYLE);
    let t = rt.string(tokens);
    rt.put_field(s, "$style", t);
    s
}

/// `Style.EMPTY`: one object.
fn empty_style(rt: &mut Rt) -> V {
    rt.intern("compat:style-empty".into(), |rt| style_object(rt, ""))
}

/// A `TextColor` of `rgb`.
fn text_color(rt: &mut Rt, rgb: u32) -> V {
    let c = rt.new_object(TEXT_COLOR);
    rt.put_field(c, "$rgb", V::I(rgb as i32));
    c
}

/// Tokens with a Boolean format turned on, off, or left as it was (null: none of `Style`'s own).
fn with_format(tokens: &str, name: &str, on: Option<bool>) -> String {
    match on {
        Some(true) => format!("{tokens}{name},"),
        Some(false) | None => format!("{tokens}!{name},"),
    }
}

fn boolean_arg(rt: &Rt, v: V) -> Option<bool> {
    match v {
        V::I(i) => Some(i != 0),
        V::R(0) => None,
        V::R(_) => Some(rt.get_field(v, "value").i() != 0),
        _ => None,
    }
}

fn style_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let tokens = style_tokens(rt, a(0));
    let st = St::default().apply(&tokens);
    let derived = |rt: &mut Rt, t: String| style_object(rt, &t);
    Some(match name {
        "withColor" => match desc {
            "(I)Lnet/minecraft/network/chat/Style;" => derived(rt, format!("{tokens}#{:06X},", a(1).i() as u32 & 0xFF_FFFF)),
            "(Lnet/minecraft/ChatFormatting;)Lnet/minecraft/network/chat/Style;" => {
                let n = formatting_name(rt, a(1));
                derived(rt, if a(1).r() == 0 { format!("{tokens}#,") } else { format!("{tokens}{n},") })
            }
            _ => {
                let t = if a(1).r() == 0 { format!("{tokens}#,") } else { format!("{tokens}#{:06X},", rt.get_field(a(1), "$rgb").i() as u32 & 0xFF_FFFF) };
                derived(rt, t)
            }
        },
        "withBold" | "withItalic" | "withUnderlined" | "withStrikethrough" | "withObfuscated" => {
            let format = match name {
                "withBold" => "BOLD",
                "withItalic" => "ITALIC",
                "withUnderlined" => "UNDERLINE",
                "withStrikethrough" => "STRIKETHROUGH",
                _ => "OBFUSCATED",
            };
            let on = boolean_arg(rt, a(1));
            derived(rt, with_format(&tokens, format, on))
        }
        "applyFormat" | "applyLegacyFormat" => {
            let n = formatting_name(rt, a(1));
            let t = match (name, color_named(&n)) {
                ("applyLegacyFormat", Some(_)) => format!("{tokens}!BOLD,!ITALIC,!UNDERLINE,!STRIKETHROUGH,!OBFUSCATED,{n},"),
                _ => format!("{tokens}{n},"),
            };
            derived(rt, t)
        }
        "applyFormats" => {
            let mut t = tokens.clone();
            for f in crate::jutil::elements(rt, a(1)) {
                t.push_str(&formatting_name(rt, f));
                t.push(',');
            }
            derived(rt, t)
        }
        // `this.applyTo(that)`: this style's own values over that one's.
        "applyTo" => {
            let under = style_tokens(rt, a(1));
            derived(rt, format!("{under}{tokens}"))
        }
        // Events and fonts: kept, not drawn.
        "withHoverEvent" | "withClickEvent" | "withInsertion" | "withFont" => {
            let s = derived(rt, tokens);
            let field = match name {
                "withHoverEvent" => "$hover",
                "withClickEvent" => "$click",
                "withInsertion" => "$insertion",
                _ => "$font",
            };
            for f in ["$hover", "$click", "$insertion", "$font"] {
                let v = rt.get_field(a(0), f);
                rt.put_field(s, f, v);
            }
            rt.put_field(s, field, a(1));
            s
        }
        "getHoverEvent" => rt.get_field(a(0), "$hover"),
        "getClickEvent" => rt.get_field(a(0), "$click"),
        "getInsertion" => rt.get_field(a(0), "$insertion"),
        "getColor" => match st.color {
            Some(c) => text_color(rt, c),
            None => V::NULL,
        },
        "isBold" => V::I(st.bold as i32),
        "isItalic" => V::I(st.italic as i32),
        "isUnderlined" => V::I(st.underlined as i32),
        "isStrikethrough" => V::I(st.strikethrough as i32),
        "isObfuscated" => V::I(st.obfuscated as i32),
        "isEmpty" => V::I(tokens.is_empty() as i32),
        "equals" => V::I((rt.class_of(a(1)) == STYLE && style_tokens(rt, a(1)) == tokens) as i32),
        "hashCode" => V::I(tokens.bytes().fold(0i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))),
        _ => return None,
    })
}

// ---- text: runs, translation ----

/// Whether `v` is a text component (`Component.literal`, `translatable`...).
fn is_component(rt: &Rt, v: V) -> bool {
    v.r() != 0 && rt.class_of(v) == MUTABLE
}

/// The styled pieces text `v` draws as, in order, adjacent pieces of one style merged: a `String`
/// (its `§` codes applied), a component (translated, with its arguments and siblings), or a
/// `FormattedCharSequence` / `FormattedText` this module made.
pub fn runs(rt: &mut Rt, v: V) -> Vec<TextRun> {
    let mut out = Vec::new();
    push(rt, v, St::default(), &mut out, 0);
    out
}

/// The text `v` reads as, without its styles (`getString`).
pub fn plain(rt: &mut Rt, v: V) -> String {
    runs(rt, v).into_iter().map(|r| r.text).collect()
}

fn push_run(out: &mut Vec<TextRun>, st: St, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.color == st.color
        && (last.bold, last.italic, last.underlined, last.strikethrough) == (st.bold, st.italic, st.underlined, st.strikethrough)
    {
        last.text.push_str(text);
        return;
    }
    out.push(st.run(text.to_owned()));
}

/// `text` from style `base`, its `§` codes changing the style as it goes.
fn legacy(text: &str, base: St, out: &mut Vec<TextRun>) {
    let mut st = base;
    let mut cur = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            cur.push(c);
            continue;
        }
        let Some(code) = chars.next() else { break };
        push_run(out, st, &std::mem::take(&mut cur));
        st = st.legacy(code, base);
    }
    push_run(out, st, &cur);
}

fn push(rt: &mut Rt, v: V, st: St, out: &mut Vec<TextRun>, depth: u32) {
    if v.r() == 0 || depth > 64 {
        return;
    }
    if let Some(s) = rt.str_of(v) {
        let s = s.to_owned();
        legacy(&s, st, out);
        return;
    }
    let class = rt.class_of(v);
    if class == SEQUENCE || class == FORMATTED {
        let st = st.apply(&style_tokens(rt, v));
        let of = rt.get_field(v, "$of");
        push(rt, of, st, out, depth + 1);
        for part in crate::jutil::elements(rt, rt.get_field(v, "$parts")) {
            push(rt, part, st, out, depth + 1);
        }
        return;
    }
    if !is_component(rt, v) {
        return;
    }
    let st = st.apply(&style_tokens(rt, v));
    let text = rt.str_of(rt.get_field(v, "$text")).unwrap_or("").to_owned();
    if rt.get_field(v, "$translate").i() != 0 {
        let fallback = rt.str_of(rt.get_field(v, "$fallback")).map(str::to_owned);
        let format = translate(rt, &text).or(fallback).unwrap_or(text);
        let args = crate::jutil::elements(rt, rt.get_field(v, "$with"));
        match decompose(&format, args.len()) {
            Some(pieces) => {
                for p in pieces {
                    match p {
                        Piece::Text(t) => legacy(&t, st, out),
                        Piece::Arg(i) if is_component(rt, args[i]) => push(rt, args[i], st, out, depth + 1),
                        Piece::Arg(i) => {
                            let t = java_text(rt, args[i]);
                            legacy(&t, st, out);
                        }
                    }
                }
            }
            None => legacy(&format, st, out),
        }
    } else if let Some(key) = rt.str_of(rt.get_field(v, "$keybind")).map(str::to_owned) {
        // `Component.keybind("key.use")`: the key bound to the mapping, as the Controls screen
        // names it ("Right Button"); a mapping the game doesn't know shows its own name translated.
        let t = match rt.host().and_then(|h| h.mapping(&key)) {
            Some(_) => crate::screens::mapping_display(rt, &key),
            None => translate(rt, &key).unwrap_or(key),
        };
        legacy(&t, st, out);
    } else {
        legacy(&text, st, out);
    }
    for sibling in crate::jutil::elements(rt, rt.get_field(v, "$extra")) {
        push(rt, sibling, st, out, depth + 1);
    }
}

/// `String.valueOf(v)` of an argument: a string, a box, anything else by its class.
fn java_text(rt: &Rt, v: V) -> String {
    match v {
        V::R(0) => "null".into(),
        V::R(_) => match rt.class_of(v) {
            "java/lang/Float" => crate::java_double(f64::from(rt.get_field(v, "value").f()), true),
            "java/lang/Double" => crate::java_double(rt.get_field(v, "value").d(), false),
            "java/lang/Long" => rt.get_field(v, "value").j().to_string(),
            "java/lang/Boolean" => (rt.get_field(v, "value").i() != 0).to_string(),
            "java/lang/Character" => char::from_u32(rt.get_field(v, "value").i() as u32).map(String::from).unwrap_or_default(),
            _ => rt.to_java_string(v, 'L'),
        },
        V::F(f) => crate::java_double(f64::from(f), true),
        V::D(d) => crate::java_double(d, false),
        V::J(j) => j.to_string(),
        other => other.i().to_string(),
    }
}

/// The player's text for `key`, if the game has one.
fn translate(rt: &Rt, key: &str) -> Option<String> {
    rt.host().and_then(|h| h.translate(key))
}

#[derive(Debug, PartialEq)]
enum Piece {
    Text(String),
    Arg(usize),
}

/// A translation's text split into its literal pieces and the arguments it takes
/// (`TranslatableContents.decomposeTemplate`: `%s`, `%2$s`, `%%`), None where vanilla gives up and
/// shows the text as it is (another conversion, a stray `%`, an argument it doesn't have).
fn decompose(format: &str, args: usize) -> Option<Vec<Piece>> {
    let mut out = Vec::new();
    let (mut next, mut last) = (0usize, 0usize);
    let bytes = format.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        // `%(?:(\d+)\$)?([A-Za-z%]|$)` at i.
        let digits = bytes[i + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        let indexed = digits > 0 && bytes.get(i + 1 + digits) == Some(&b'$');
        let (index, conv_at) = if indexed { (format[i + 1..i + 1 + digits].parse::<usize>().ok(), i + 2 + digits) } else { (None, i + 1) };
        let conv = bytes.get(conv_at).copied();
        let end = match conv {
            Some(c) if c.is_ascii_alphabetic() || c == b'%' => conv_at + 1,
            None => conv_at,
            Some(_) => {
                // No match here: this `%` stays in a literal piece, which vanilla refuses.
                return None;
            }
        };
        if i > last {
            out.push(Piece::Text(format[last..i].to_owned()));
        }
        match conv {
            Some(b'%') if !indexed => out.push(Piece::Text("%".into())),
            Some(b's') => {
                let k = match index {
                    Some(n) => n.checked_sub(1)?,
                    None if indexed => return None,
                    None => {
                        next += 1;
                        next - 1
                    }
                };
                if k >= args {
                    return None;
                }
                out.push(Piece::Arg(k));
            }
            _ => return None,
        }
        last = end;
        i = end;
    }
    if last < format.len() {
        out.push(Piece::Text(format[last..].to_owned()));
    }
    Some(out)
}

/// Java's `String.format` for the client's `I18n.get`: `%s`, `%d` and `%n$s`, `%%`.
fn java_format(rt: &Rt, format: &str, args: &[V]) -> String {
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    let mut next = 0;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() || d == '$' || d == '.' {
                spec.push(d);
                chars.next();
            } else {
                break;
            }
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('n') => out.push('\n'),
            Some(conv) => {
                let k = match spec.split_once('$') {
                    Some((n, _)) => n.parse::<usize>().unwrap_or(1).saturating_sub(1),
                    None => {
                        next += 1;
                        next - 1
                    }
                };
                let v = args.get(k).copied().unwrap_or(V::NULL);
                let precision = spec.rsplit_once('.').and_then(|(_, p)| p.parse::<usize>().ok());
                match (conv, precision) {
                    ('f', p) => {
                        let x = if rt.class_of(v) == "java/lang/Float" { f64::from(rt.get_field(v, "value").f()) } else { rt.get_field(v, "value").d() };
                        out.push_str(&format!("{x:.*}", p.unwrap_or(6)));
                    }
                    _ => out.push_str(&java_text(rt, v)),
                }
            }
            None => out.push('%'),
        }
    }
    out
}

// ---- widths and lines ----

fn char_width(rt: &Rt, c: char, bold: bool) -> f32 {
    rt.host().map_or(6.0 + bold as i32 as f32, |h| h.gui_char_width(c, bold))
}

/// How wide `runs` are in GUI pixels (`StringSplitter.stringWidth`).
fn width_of(rt: &Rt, runs: &[TextRun]) -> f32 {
    runs.iter().map(|r| r.text.chars().map(|c| char_width(rt, c, r.bold)).sum::<f32>()).sum()
}

/// `Font.width`: the width rounded up.
pub fn width(rt: &mut Rt, v: V) -> i32 {
    let r = runs(rt, v);
    width_of(rt, &r).ceil() as i32
}

/// `runs` split into lines at most `max` wide, as `StringSplitter.splitLines` breaks them: after the
/// last space that fits (the space dropped), inside a word too long for a line, and at each `\n`.
fn split(rt: &Rt, runs: &[TextRun], max: f32) -> Vec<Vec<TextRun>> {
    let max = max.max(1.0);
    let chars: Vec<(char, usize)> = runs.iter().enumerate().flat_map(|(i, r)| r.text.chars().map(move |c| (c, i))).collect();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut after_newline = false;
    loop {
        let (mut width, mut wide, mut last_space, mut brk) = (0.0f32, false, None, None);
        for (i, &(c, run)) in chars.iter().enumerate().skip(start) {
            if c == '\n' {
                brk = Some(i);
                break;
            }
            if c == ' ' {
                last_space = Some(i);
            }
            let w = char_width(rt, c, runs[run].bold);
            width += w;
            if wide && width > max {
                brk = Some(last_space.unwrap_or(i));
                break;
            }
            wide |= w != 0.0;
        }
        match brk {
            None => {
                if start < chars.len() {
                    lines.push(line(runs, &chars[start..]));
                } else if after_newline {
                    lines.push(Vec::new());
                }
                break;
            }
            Some(k) => {
                lines.push(line(runs, &chars[start..k]));
                after_newline = chars[k].0 == '\n';
                start = if matches!(chars[k].0, '\n' | ' ') { k + 1 } else { k };
            }
        }
    }
    lines
}

/// The runs of a line of characters (each with its run's index).
fn line(runs: &[TextRun], chars: &[(char, usize)]) -> Vec<TextRun> {
    let mut out: Vec<TextRun> = Vec::new();
    for &(c, i) in chars {
        match out.last_mut() {
            Some(last) if last.color == runs[i].color && last.bold == runs[i].bold && last.italic == runs[i].italic && last.underlined == runs[i].underlined && last.strikethrough == runs[i].strikethrough => last.text.push(c),
            _ => out.push(TextRun { text: c.to_string(), ..runs[i].clone() }),
        }
    }
    out
}

/// The longest start of `runs` at most `max` wide (`StringSplitter.headByWidth`).
fn head(rt: &Rt, runs: &[TextRun], max: f32) -> Vec<TextRun> {
    let mut out: Vec<TextRun> = Vec::new();
    let mut width = 0.0;
    for r in runs {
        let mut piece = String::new();
        for c in r.text.chars() {
            let w = char_width(rt, c, r.bold);
            if width + w > max {
                if !piece.is_empty() {
                    out.push(TextRun { text: piece, ..r.clone() });
                }
                return out;
            }
            width += w;
            piece.push(c);
        }
        if !piece.is_empty() {
            out.push(TextRun { text: piece, ..r.clone() });
        }
    }
    out
}

/// A component drawing as `runs` (a line of a split text).
fn component_of(rt: &mut Rt, runs: &[TextRun]) -> V {
    let literal = |rt: &mut Rt, r: &TextRun| {
        let t = rt.string(&r.text);
        let c = crate::api_common::component(rt, t, false, V::NULL);
        let s = rt.string(&tokens_of(r));
        rt.put_field(c, "$style", s);
        c
    };
    match runs {
        [one] => literal(rt, one),
        _ => {
            let empty = rt.string("");
            let c = crate::api_common::component(rt, empty, false, V::NULL);
            let parts: Vec<V> = runs.iter().map(|r| literal(rt, r)).collect();
            let list = crate::jutil::new_list(rt, parts);
            rt.put_field(c, "$extra", list);
            c
        }
    }
}

/// A `FormattedCharSequence` of `of` (a component, a string), under `style`'s tokens.
fn sequence(rt: &mut Rt, of: V, style: &str) -> V {
    let s = rt.new_object(SEQUENCE);
    rt.put_field(s, "$of", of);
    if !style.is_empty() {
        let t = rt.string(style);
        rt.put_field(s, "$style", t);
    }
    s
}

/// A text of `parts` in order (`FormattedText.composite`, `FormattedCharSequence.composite`).
fn composite(rt: &mut Rt, class: &'static str, parts: Vec<V>) -> V {
    let s = rt.new_object(class);
    let list = crate::jutil::new_list(rt, parts);
    rt.put_field(s, "$parts", list);
    s
}

/// `text` split into lines at most `max` wide, each a component.
fn split_components(rt: &mut Rt, text: V, max: i32) -> Vec<V> {
    let r = runs(rt, text);
    let lines = split(rt, &r, max as f32);
    lines.iter().map(|l| component_of(rt, l)).collect()
}

fn text_calls(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        (FONT, "width") => V::I(width(rt, a(1))),
        (FONT, "split") => {
            let lines = split_components(rt, a(1), a(2).i());
            let seqs: Vec<V> = lines.into_iter().map(|l| sequence(rt, l, "")).collect();
            crate::jutil::new_list(rt, seqs)
        }
        (FONT, "getSplitter") => rt.intern("compat:string-splitter".into(), |rt| rt.new_object(SPLITTER)),
        (FONT, "wordWrapHeight") => {
            let r = runs(rt, a(1));
            V::I(split(rt, &r, a(2).i() as f32).len() as i32 * LINE_HEIGHT)
        }
        (FONT, "isBidirectional") => V::I(0),
        (FONT, "bidirectionalShaping") => a(1),
        (FONT, "plainSubstrByWidth") => {
            let text = rt.str_of(a(1)).unwrap_or("").to_owned();
            let r = St::default().run(text.clone());
            let tail = desc.contains("IZ)") && a(3).i() != 0;
            let out = if tail {
                let rev = TextRun { text: text.chars().rev().collect(), ..r };
                let h = head(rt, &[rev], a(2).i() as f32);
                h.first().map(|t| t.text.chars().rev().collect::<String>()).unwrap_or_default()
            } else {
                head(rt, &[r], a(2).i() as f32).first().map(|t| t.text.clone()).unwrap_or_default()
            };
            rt.string(&out)
        }
        (FONT, "substrByWidth") => {
            let r = runs(rt, a(1));
            let h = head(rt, &r, a(2).i() as f32);
            component_of(rt, &h)
        }
        (FONT, "ellipsize") => {
            let r = runs(rt, a(1));
            let max = a(2).i() as f32;
            if width_of(rt, &r) <= max {
                return Some(a(1));
            }
            let dots = St::default().run("...".into());
            let room = max - width_of(rt, std::slice::from_ref(&dots));
            let mut h = head(rt, &r, room);
            h.push(dots);
            component_of(rt, &h)
        }
        (FONT, "drawInBatch") if desc.contains("Lorg/joml/Matrix4f;") => {
            // (text, x, y, colour, shadow, matrix, buffers, mode, background, light[, bidi])
            let m = render::mat4(rt, a(6));
            let r = runs(rt, a(1));
            V::I(draw_runs(rt, &r, a(2).f(), a(3).f(), a(4).i(), a(5).i() != 0, &m))
        }
        (SPLITTER, "stringWidth") => {
            let r = runs(rt, a(1));
            V::F(width_of(rt, &r))
        }
        (SPLITTER, "splitLines") if desc.ends_with(")Ljava/util/List;") => {
            let base = style_tokens(rt, a(3));
            let mut r = Vec::new();
            push(rt, a(1), St::default().apply(&base), &mut r, 0);
            let lines = split(rt, &r, a(2).i() as f32);
            let parts: Vec<V> = lines.iter().map(|l| component_of(rt, l)).collect();
            crate::jutil::new_list(rt, parts)
        }
        (SPLITTER, "plainHeadByWidth") => {
            let text = rt.str_of(a(1)).unwrap_or("").to_owned();
            let h = head(rt, &[St::default().run(text)], a(2).i() as f32);
            let s = h.first().map(|t| t.text.clone()).unwrap_or_default();
            rt.string(&s)
        }
        (SPLITTER, "headByWidth") => {
            let r = runs(rt, a(1));
            let h = head(rt, &r, a(2).i() as f32);
            component_of(rt, &h)
        }
        // Text in visual order: the text itself, drawn left to right.
        ("net/minecraft/locale/Language", "getInstance") => rt.intern("compat:language".into(), |rt| rt.new_object("net/minecraft/locale/Language")),
        ("net/minecraft/locale/Language", "getVisualOrder") if desc.starts_with("(Ljava/util/List;)") => {
            let items = crate::jutil::elements(rt, a(1));
            let seqs: Vec<V> = items.into_iter().map(|t| sequence(rt, t, "")).collect();
            crate::jutil::new_list(rt, seqs)
        }
        ("net/minecraft/locale/Language", "getVisualOrder") => sequence(rt, a(1), ""),
        ("net/minecraft/locale/Language", "getOrDefault") => {
            let key = rt.str_of(a(1)).unwrap_or("").to_owned();
            match translate(rt, &key) {
                Some(t) => rt.string(&t),
                None if desc.starts_with("(Ljava/lang/String;Ljava/lang/String;)") => a(2),
                None => a(1),
            }
        }
        ("net/minecraft/locale/Language", "has") => {
            let key = rt.str_of(a(1)).unwrap_or("").to_owned();
            V::I(translate(rt, &key).is_some() as i32)
        }
        ("net/minecraft/locale/Language", "isDefaultRightToLeft") => V::I(0),
        ("net/minecraft/client/resources/language/I18n", "get") => {
            let key = rt.str_of(a(0)).unwrap_or("").to_owned();
            let format = translate(rt, &key).unwrap_or(key);
            let with = crate::jutil::elements(rt, a(1));
            let t = java_format(rt, &format, &with);
            rt.string(&t)
        }
        ("net/minecraft/client/resources/language/I18n", "exists") => {
            let key = rt.str_of(a(0)).unwrap_or("").to_owned();
            V::I(translate(rt, &key).is_some() as i32)
        }
        (SEQUENCE, "forward") => {
            let tokens = style_tokens(rt, a(1));
            sequence(rt, a(0), &tokens)
        }
        (SEQUENCE, "codepoint") => {
            let c = char::from_u32(a(0).i() as u32).map(String::from).unwrap_or_default();
            let s = rt.string(&c);
            let tokens = style_tokens(rt, a(1));
            sequence(rt, s, &tokens)
        }
        (SEQUENCE, "composite") => {
            let parts = if desc.starts_with("(Ljava/util/List;)") || desc.starts_with("([") { crate::jutil::elements(rt, a(0)) } else { args.to_vec() };
            composite(rt, SEQUENCE, parts)
        }
        (FORMATTED, "of") => {
            let tokens = if desc.contains("Style;") { style_tokens(rt, a(1)) } else { String::new() };
            let f = rt.new_object(FORMATTED);
            rt.put_field(f, "$of", a(0));
            if !tokens.is_empty() {
                let t = rt.string(&tokens);
                rt.put_field(f, "$style", t);
            }
            f
        }
        (FORMATTED, "composite") => {
            let parts = if desc.starts_with("(Ljava/util/List;)") || desc.starts_with("([") { crate::jutil::elements(rt, a(0)) } else { args.to_vec() };
            composite(rt, FORMATTED, parts)
        }
        (FORMATTED | COMPONENT | MUTABLE, "getString") if desc == "()Ljava/lang/String;" => {
            let t = plain(rt, a(0));
            rt.string(&t)
        }
        (COMPONENT | MUTABLE, "getString") => {
            let t: String = plain(rt, a(0)).chars().take(a(1).i().max(0) as usize).collect();
            rt.string(&t)
        }
        (COMPONENT | MUTABLE, "getVisualOrderText") => sequence(rt, a(0), ""),
        (COMPONENT | MUTABLE, "getStyle") => {
            let t = style_tokens(rt, a(0));
            style_object(rt, &t)
        }
        (COMPONENT | MUTABLE, "getSiblings") => {
            let extra = crate::jutil::elements(rt, rt.get_field(a(0), "$extra"));
            crate::jutil::new_list(rt, extra)
        }
        (MUTABLE, "setStyle") => {
            let t = style_tokens(rt, a(1));
            let s = rt.string(&t);
            rt.put_field(a(0), "$style", s);
            a(0)
        }
        // `withStyle(Style)`: the style's values over the component's; `withStyle(UnaryOperator)`:
        // the operator's style of the component's.
        (COMPONENT | MUTABLE, "withStyle") if desc == "(Lnet/minecraft/network/chat/Style;)Lnet/minecraft/network/chat/MutableComponent;" => {
            let t = format!("{}{}", style_tokens(rt, a(0)), style_tokens(rt, a(1)));
            let s = rt.string(&t);
            rt.put_field(a(0), "$style", s);
            a(0)
        }
        (COMPONENT | MUTABLE, "withStyle") if desc.starts_with("(Ljava/util/function/UnaryOperator;)") => {
            let t = style_tokens(rt, a(0));
            let old = style_object(rt, &t);
            let new = rt.call_functional("java/util/function/UnaryOperator.apply.(Ljava/lang/Object;)Ljava/lang/Object;", a(1), &[old]);
            let t = if rt.class_of(new) == STYLE { style_tokens(rt, new) } else { t };
            let s = rt.string(&t);
            rt.put_field(a(0), "$style", s);
            a(0)
        }
        (COMPONENT | MUTABLE, "withColor") => {
            let t = format!("{}#{:06X},", style_tokens(rt, a(0)), a(1).i() as u32 & 0xFF_FFFF);
            let s = rt.string(&t);
            rt.put_field(a(0), "$style", s);
            a(0)
        }
        (COMPONENT, "keybind") => {
            let empty = rt.string("");
            let c = crate::api_common::component(rt, empty, false, V::NULL);
            rt.put_field(c, "$keybind", a(0));
            c
        }
        (COMPONENT, "translatableWithFallback") => {
            let with = if desc.contains("[Ljava/lang/Object;") { a(2) } else { V::NULL };
            let c = crate::api_common::component(rt, a(0), true, with);
            rt.put_field(c, "$fallback", a(1));
            c
        }
        (COMPONENT, "nullToEmpty") => {
            let text = if a(0).r() == 0 { rt.string("") } else { a(0) };
            crate::api_common::component(rt, text, false, V::NULL)
        }
        ("net/minecraft/network/chat/ComponentUtils", "wrapInSquareBrackets") => {
            let key = rt.string("chat.square_brackets");
            let with = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![a(0)]));
            crate::api_common::component(rt, key, true, with)
        }
        ("net/minecraft/network/chat/CommonComponents", "space") => {
            let t = rt.string(" ");
            crate::api_common::component(rt, t, false, V::NULL)
        }
        ("net/minecraft/network/chat/CommonComponents", "optionStatus") if desc == "(Z)Lnet/minecraft/network/chat/Component;" => {
            let key = rt.string(if a(0).i() != 0 { "options.on" } else { "options.off" });
            crate::api_common::component(rt, key, true, V::NULL)
        }
        (STYLE, _) => return style_call(rt, name, desc, args),
        (TEXT_COLOR, "getValue") => rt.get_field(a(0), "$rgb"),
        (TEXT_COLOR, "fromRgb") => text_color(rt, a(0).i() as u32 & 0xFF_FFFF),
        (TEXT_COLOR, "fromLegacyFormat") => {
            let n = formatting_name(rt, a(0));
            match color_named(&n) {
                Some(c) => text_color(rt, c),
                None => V::NULL,
            }
        }
        (FORMATTING, "getColor") => {
            let n = formatting_name(rt, a(0));
            match color_named(&n) {
                Some(c) => crate::api_common::integer(rt, c as i32),
                None => V::NULL,
            }
        }
        (FORMATTING, "isColor") => {
            let n = formatting_name(rt, a(0));
            V::I(color_named(&n).is_some() as i32)
        }
        (FORMATTING, "isFormat") => {
            let n = formatting_name(rt, a(0));
            V::I((color_named(&n).is_none() && n != "RESET") as i32)
        }
        (FORMATTING, "getChar") => {
            let n = formatting_name(rt, a(0));
            V::I(FORMATS.iter().find(|f| f.0 == n).map_or(0, |f| f.1 as i32))
        }
        (FORMATTING, "stripFormatting") => {
            let mut out = Vec::new();
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            legacy(&text, St::default(), &mut out);
            let t: String = out.into_iter().map(|r| r.text).collect();
            rt.string(&t)
        }
        _ => return None,
    })
}

// ---- drawing ----

/// Whether a mod is drawing on the GUI now.
pub fn drawing(rt: &Rt) -> bool {
    rt.interned.contains_key(DRAWING)
}

/// `m` applied to GUI point `(x, y)`.
fn at(m: &M4, x: f32, y: f32) -> [f32; 2] {
    [m[0] * x + m[4] * y + m[12], m[1] * x + m[5] * y + m[13]]
}

/// How many GUI pixels a unit along x is under `m`.
fn scale_of(m: &M4) -> f32 {
    (m[0] * m[0] + m[1] * m[1]).sqrt()
}

fn pose_of(rt: &Rt, graphics: V) -> M4 {
    render::last_pose(rt, rt.get_field(graphics, "$pose"))
}

/// A rectangle's corners under `m`, in drawing order.
fn quad(m: &M4, x0: f32, y0: f32, x1: f32, y1: f32) -> [[f32; 2]; 4] {
    [at(m, x0, y0), at(m, x0, y1), at(m, x1, y1), at(m, x1, y0)]
}

fn fill(rt: &mut Rt, m: &M4, x0: i32, y0: i32, x1: i32, y1: i32, argb: i32) {
    let q = quad(m, x0 as f32, y0 as f32, x1 as f32, y1 as f32);
    if let Some(h) = rt.host_mut() {
        h.gui_fill(q, argb as u32);
    }
}

/// A gradient from `from` at the top to `to` at the bottom (`fillGradient`), in one-pixel bands
/// (at most 64).
fn fill_gradient(rt: &mut Rt, m: &M4, x0: i32, y0: i32, x1: i32, y1: i32, from: i32, to: i32) {
    if from == to || y0 == y1 {
        fill(rt, m, x0, y0, x1, y1, from);
        return;
    }
    let bands = (y1 - y0).unsigned_abs().clamp(1, 64);
    let lerp = |t: f32| -> u32 {
        let [a, b] = [from as u32, to as u32];
        (0..4).fold(0u32, |c, i| {
            let shift = i * 8;
            let (x, y) = ((a >> shift) & 0xFF, (b >> shift) & 0xFF);
            let v = (x as f32 + (y as f32 - x as f32) * t).round() as u32;
            c | (v.min(255) << shift)
        })
    };
    for k in 0..bands {
        let t0 = k as f32 / bands as f32;
        let t1 = (k + 1) as f32 / bands as f32;
        let ya = y0 as f32 + (y1 - y0) as f32 * t0;
        let yb = y0 as f32 + (y1 - y0) as f32 * t1;
        let q = quad(m, x0 as f32, ya, x1 as f32, yb);
        let c = lerp((t0 + t1) / 2.0);
        if let Some(h) = rt.host_mut() {
            h.gui_fill(q, c);
        }
    }
}

/// `Font.adjustColor`: a colour with (almost) no alpha is opaque.
fn adjust_color(color: i32) -> u32 {
    let c = color as u32;
    if c & 0xFC00_0000 == 0 { c | 0xFF00_0000 } else { c }
}

/// Draws `runs` at `(x, y)` under `m` (`Font.drawInBatch`): what vanilla returns, the end of the
/// text (plus the shadow's pixel). Only on the GUI: text a block entity's renderer draws in the
/// world isn't drawn here.
fn draw_runs(rt: &mut Rt, runs: &[TextRun], x: f32, y: f32, color: i32, shadow: bool, m: &M4) -> i32 {
    let w = width_of(rt, runs);
    if drawing(rt) && !runs.is_empty() {
        let p = at(m, x, y);
        let s = scale_of(m);
        if let Some(h) = rt.host_mut() {
            h.gui_text(runs, p, s, adjust_color(color), shadow);
        }
    }
    (x + w) as i32 + shadow as i32
}

/// The texture a GUI sprite is (`blitSprite(create:widget/button)`): its file under
/// `textures/gui/sprites`.
fn sprite_texture(sprite: &str) -> String {
    let (ns, path) = sprite.split_once(':').unwrap_or(("minecraft", sprite));
    format!("{ns}:textures/gui/sprites/{path}.png")
}

fn tint(rt: &Rt) -> u32 {
    rt.interned.get(TINT).map_or(u32::MAX, |v| v.i() as u32)
}

/// `innerBlit`: part `uv` of `texture` into the rectangle `x0..x1, y0..y1` under `m`. Texture
/// coordinates past 0..1 repeat the texture (GL's wrap, as vanilla's 32 x 32 menu background
/// tiles over a screen): such a blit is drawn a tile at a time, up to 4096 tiles.
fn blit(rt: &mut Rt, m: &M4, texture: &str, [x0, x1, y0, y1]: [f32; 4], uv: [f32; 4]) {
    let [u0, v0, u1, v1] = uv;
    let span = |a: f32, b: f32| (b.ceil() - a.floor()).max(1.0);
    let repeats = u1 > u0 && v1 > v0 && (u0 < 0.0 || v0 < 0.0 || u1 > 1.0 || v1 > 1.0);
    if repeats && uv.iter().all(|c| c.is_finite()) && span(u0, u1) * span(v0, v1) <= 4096.0 {
        // The pieces of `a..b` between whole numbers.
        let cuts = |a: f32, b: f32| {
            let mut out = Vec::new();
            let mut s = a;
            while s < b {
                let e = (s.floor() + 1.0).min(b);
                out.push((s, e));
                s = e;
            }
            out
        };
        let x_at = |u: f32| x0 + (x1 - x0) * (u - u0) / (u1 - u0);
        let y_at = |v: f32| y0 + (y1 - y0) * (v - v0) / (v1 - v0);
        for (ua, ub) in cuts(u0, u1) {
            let bu = ua.floor();
            for (va, vb) in cuts(v0, v1) {
                let bv = va.floor();
                blit_one(rt, m, texture, [x_at(ua), x_at(ub), y_at(va), y_at(vb)], [ua - bu, va - bv, ub - bu, vb - bv]);
            }
        }
        return;
    }
    blit_one(rt, m, texture, [x0, x1, y0, y1], uv);
}

fn blit_one(rt: &mut Rt, m: &M4, texture: &str, [x0, x1, y0, y1]: [f32; 4], uv: [f32; 4]) {
    let (a, b) = (at(m, x0, y0), at(m, x1, y1));
    let rect = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
    let tint = tint(rt);
    blit_rect(rt, texture, rect, uv, tint);
}

fn blit_rect(rt: &mut Rt, texture: &str, rect: [f32; 4], uv: [f32; 4], tint: u32) {
    let drew = rt.host_mut().is_some_and(|h| h.gui_blit(texture, uv, rect, tint));
    if !drew {
        crate::glue::trace_once(rt, &format!("gui-blit/{texture}"), |_| format!("client: no GUI texture {texture}"));
    }
}

// ---- for screens (`screens.rs`) ----

/// A mod starts drawing on the GUI (a screen): text and items drawn through a pose go to the GUI.
pub(crate) fn begin_drawing(rt: &mut Rt) {
    rt.interned.insert(DRAWING.into(), V::I(1));
}

/// The drawing is over: the flag and the tint go.
pub(crate) fn end_drawing(rt: &mut Rt) {
    rt.interned.remove(DRAWING);
    rt.interned.remove(TINT);
}

/// The pose a `GuiGraphics` draws under now.
pub(crate) fn pose(rt: &Rt, graphics: V) -> M4 {
    pose_of(rt, graphics)
}

/// `fillGradient(x0, y0, x1, y1, from, to)` under `m`.
pub(crate) fn gradient(rt: &mut Rt, m: &M4, [x0, y0, x1, y1]: [i32; 4], from: i32, to: i32) {
    fill_gradient(rt, m, x0, y0, x1, y1, from, to);
}

/// A blit under `m` (`x0..x1, y0..y1`), its texture repeating past 0..1 ([`blit`]).
pub(crate) fn blit_at(rt: &mut Rt, m: &M4, texture: &str, rect: [f32; 4], uv: [f32; 4]) {
    blit(rt, m, texture, rect, uv);
}

/// A blit into GUI rectangle `rect` (`[x0, y0, x1, y1]`, already through the pose), its colour
/// `argb` (a vertex's) times the shader colour.
pub(crate) fn blit_tinted(rt: &mut Rt, texture: &str, rect: [f32; 4], uv: [f32; 4], argb: u32) {
    let shader = tint(rt);
    let times = |shift: u32| ((argb >> shift & 0xFF) * (shader >> shift & 0xFF) / 255) << shift;
    let tint = times(24) | times(16) | times(8) | times(0);
    blit_rect(rt, texture, rect, uv, tint);
}

fn texture_name(rt: &Rt, v: V) -> String {
    rt.str_of(v).unwrap_or("").to_owned()
}

/// `GuiGraphics.blit` and `blitSprite`, by descriptor.
fn blit_call(rt: &mut Rt, m: &M4, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let i = |k: usize| a(k).i() as f32;
    let f = |k: usize| a(k).f();
    let tex = |rt: &Rt| texture_name(rt, a(1));
    match desc {
        // (texture, x, y, u, v, width, height): a 256 x 256 texture.
        "(Lnet/minecraft/resources/ResourceLocation;IIIIII)V" => {
            let (x, y, u, v, w, h) = (i(2), i(3), i(4), i(5), i(6), i(7));
            blit(rt, m, &tex(rt), [x, x + w, y, y + h], [u / 256.0, v / 256.0, (u + w) / 256.0, (v + h) / 256.0]);
        }
        // (texture, x, y, z, u, v, width, height, textureWidth, textureHeight)
        "(Lnet/minecraft/resources/ResourceLocation;IIIFFIIII)V" => {
            let (x, y, u, v, w, h, tw, th) = (i(2), i(3), f(5), f(6), i(7), i(8), i(9), i(10));
            blit(rt, m, &tex(rt), [x, x + w, y, y + h], [u / tw, v / th, (u + w) / tw, (v + h) / th]);
        }
        // (texture, x, y, width, height, u, v, uWidth, vHeight, textureWidth, textureHeight)
        "(Lnet/minecraft/resources/ResourceLocation;IIIIFFIIII)V" => {
            let (x, y, w, h, u, v, uw, vh, tw, th) = (i(2), i(3), i(4), i(5), f(6), f(7), i(8), i(9), i(10), i(11));
            blit(rt, m, &tex(rt), [x, x + w, y, y + h], [u / tw, v / th, (u + uw) / tw, (v + vh) / th]);
        }
        // (texture, x, y, u, v, width, height, textureWidth, textureHeight)
        "(Lnet/minecraft/resources/ResourceLocation;IIFFIIII)V" => {
            let (x, y, u, v, w, h, tw, th) = (i(2), i(3), f(4), f(5), i(6), i(7), i(8), i(9));
            blit(rt, m, &tex(rt), [x, x + w, y, y + h], [u / tw, v / th, (u + w) / tw, (v + h) / th]);
        }
        // (texture, x1, x2, y1, y2, z, uWidth, vHeight, u, v, textureWidth, textureHeight)
        "(Lnet/minecraft/resources/ResourceLocation;IIIIIIIFFII)V" => {
            let (x1, x2, y1, y2, uw, vh, u, v, tw, th) = (i(2), i(3), i(4), i(5), i(7), i(8), f(9), f(10), i(11), i(12));
            blit(rt, m, &tex(rt), [x1, x2, y1, y2], [u / tw, v / th, (u + uw) / tw, (v + vh) / th]);
        }
        // (x, y, z, width, height, sprite[, r, g, b, a]): a sprite of an atlas.
        d if d.starts_with("(IIIIILnet/minecraft/client/renderer/texture/TextureAtlasSprite;") => {
            let (x, y, w, h) = (i(1), i(2), i(4), i(5));
            let name = rt.str_of(rt.get_field(a(6), "$name")).unwrap_or("minecraft:missingno").to_owned();
            blit(rt, m, &format!("sprite:{name}"), [x, x + w, y, y + h], [0.0, 0.0, 1.0, 1.0]);
        }
        // blitSprite(sprite, x, y, width, height) and with z.
        "(Lnet/minecraft/resources/ResourceLocation;IIII)V" | "(Lnet/minecraft/resources/ResourceLocation;IIIII)V" => {
            let (x, y) = (i(2), i(3));
            let (w, h) = if desc.ends_with("IIIII)V") { (i(5), i(6)) } else { (i(4), i(5)) };
            if w != 0.0 && h != 0.0 {
                blit(rt, m, &sprite_texture(&tex(rt)), [x, x + w, y, y + h], [0.0, 0.0, 1.0, 1.0]);
            }
        }
        // blitSprite(sprite, textureWidth, textureHeight, u, v, x, y[, z], uWidth, vHeight)
        "(Lnet/minecraft/resources/ResourceLocation;IIIIIIII)V" | "(Lnet/minecraft/resources/ResourceLocation;IIIIIIIII)V" => {
            let (tw, th, u, v, x, y) = (i(2), i(3), i(4), i(5), i(6), i(7));
            let (uw, vh) = if desc.ends_with("IIIIIIIII)V") { (i(9), i(10)) } else { (i(8), i(9)) };
            if uw != 0.0 && vh != 0.0 {
                blit(rt, m, &sprite_texture(&tex(rt)), [x, x + uw, y, y + vh], [u / tw, v / th, (u + uw) / tw, (v + vh) / th]);
            }
        }
        _ => return None,
    }
    Some(V::Void)
}

/// An item's icon at GUI point `(x, y)` (its top left) under `m`, 16 pixels at scale 1.
fn item_icon(rt: &mut Rt, m: &M4, stack: V, x: f32, y: f32) {
    if crate::api_common::stack_empty(rt, stack) {
        return;
    }
    let item = crate::api_common::item_like_name(rt, rt.get_field(stack, "$item"));
    let p = at(m, x, y);
    let size = 16.0 * scale_of(m);
    let drew = rt.host_mut().is_some_and(|h| h.gui_item(&item, p, size));
    if !drew {
        crate::glue::trace_once(rt, &format!("gui-item/{item}"), |_| format!("client: no GUI icon for {item}"));
    }
}

/// An item a renderer draws in the GUI display context while a mod draws on the GUI (catnip's
/// `GuiGameElement`): the item model's unit cube under `m`, centred on its origin.
pub fn gui_item(rt: &mut Rt, item: &str, m: &M4) {
    let size = scale_of(m);
    let c = at(m, 0.0, 0.0);
    let p = [c[0] - size / 2.0, c[1] - size / 2.0];
    let drew = rt.host_mut().is_some_and(|h| h.gui_item(item, p, size));
    if !drew {
        crate::glue::trace_once(rt, &format!("gui-item/{item}"), |_| format!("client: no GUI icon for {item}"));
    }
}

fn graphics_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let g = a(0);
    if name == "<init>" {
        let pose = render::new_pose_stack(rt, render::IDENTITY);
        rt.put_field(g, "$pose", pose);
        return Some(V::Void);
    }
    let m = pose_of(rt, g);
    // Overloads that take a render type first draw as the plain ones.
    let typed = desc.starts_with("(Lnet/minecraft/client/renderer/RenderType;");
    let o = typed as usize;
    let i = |k: usize| a(k + o).i();
    Some(match name {
        "pose" => rt.get_field(g, "$pose"),
        "guiWidth" => V::I(rt.host().map_or(0, |h| h.gui_size()[0])),
        "guiHeight" => V::I(rt.host().map_or(0, |h| h.gui_size()[1])),
        "bufferSource" => rt.intern("compat:gui-buffers".into(), |rt| rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource")),
        "flush" | "enableScissor" | "disableScissor" => V::Void,
        "containsPointInScissor" => V::I(1),
        "drawManaged" => {
            rt.call_functional("java/lang/Runnable.run.()V", a(1), &[]);
            V::Void
        }
        "setColor" => {
            let c = |k: usize| ((a(k).f().clamp(0.0, 1.0) * 255.0).round() as u32) << (match k {
                4 => 24,
                1 => 16,
                2 => 8,
                _ => 0,
            });
            let tint = c(1) | c(2) | c(3) | c(4);
            rt.interned.insert(TINT.into(), V::I(tint as i32));
            V::Void
        }
        "hLine" => {
            let (mut x0, mut x1, y, color) = (i(1), i(2), i(3), i(4));
            if x1 < x0 {
                std::mem::swap(&mut x0, &mut x1);
            }
            fill(rt, &m, x0, y, x1 + 1, y + 1, color);
            V::Void
        }
        "vLine" => {
            let (x, mut y0, mut y1, color) = (i(1), i(2), i(3), i(4));
            if y1 < y0 {
                std::mem::swap(&mut y0, &mut y1);
            }
            fill(rt, &m, x, y0 + 1, x + 1, y1, color);
            V::Void
        }
        "fill" => {
            // (x0, y0, x1, y1[, z], colour)
            let color = if desc.ends_with("IIIIII)V") { i(6) } else { i(5) };
            fill(rt, &m, i(1), i(2), i(3), i(4), color);
            V::Void
        }
        "fillGradient" => {
            // (x0, y0, x1, y1, from, to), with z before the colours, or (type, ..., from, to, z).
            let (from, to) = match (typed, desc.ends_with("IIIIIII)V")) {
                (true, _) => (i(5), i(6)),
                (false, true) => (i(6), i(7)),
                (false, false) => (i(5), i(6)),
            };
            fill_gradient(rt, &m, i(1), i(2), i(3), i(4), from, to);
            V::Void
        }
        "fillRenderType" => V::Void,
        "renderOutline" => {
            let (x, y, w, h, c) = (i(1), i(2), i(3), i(4), i(5));
            fill(rt, &m, x, y, x + w, y + 1, c);
            fill(rt, &m, x, y + h - 1, x + w, y + h, c);
            fill(rt, &m, x, y + 1, x + 1, y + h - 1, c);
            fill(rt, &m, x + w - 1, y + 1, x + w, y + h - 1, c);
            V::Void
        }
        "drawString" => {
            // (font, text, x, y, colour[, shadow]), x and y ints or floats.
            let floats = desc.contains("FFI");
            let (x, y) = if floats { (a(3).f(), a(4).f()) } else { (a(3).i() as f32, a(4).i() as f32) };
            let shadow = !desc.ends_with("Z)I") || a(6).i() != 0;
            if a(2).r() == 0 {
                return Some(V::I(0));
            }
            let r = runs(rt, a(2));
            V::I(draw_runs(rt, &r, x, y, a(5).i(), shadow, &m))
        }
        "drawCenteredString" => {
            let r = runs(rt, a(2));
            let w = width_of(rt, &r).ceil() as i32;
            draw_runs(rt, &r, (a(3).i() - w / 2) as f32, a(4).i() as f32, a(5).i(), true, &m);
            V::Void
        }
        "drawWordWrap" => {
            // (font, text, x, y, lineWidth, colour)
            let r = runs(rt, a(2));
            let mut y = a(4).i();
            for l in split(rt, &r, a(5).i() as f32) {
                draw_runs(rt, &l, a(3).i() as f32, y as f32, a(6).i(), false, &m);
                y += LINE_HEIGHT;
            }
            V::Void
        }
        "drawStringWithBackdrop" => {
            let r = runs(rt, a(2));
            V::I(draw_runs(rt, &r, a(3).i() as f32, a(4).i() as f32, a(6).i(), true, &m))
        }
        "blit" | "blitSprite" => return blit_call(rt, &m, desc, args),
        "renderItem" | "renderFakeItem" => {
            // (stack, x, y, ...) or (entity, stack, x, y, seed)
            let s = if desc.starts_with("(Lnet/minecraft/world/entity/LivingEntity;") { 1 } else { 0 };
            item_icon(rt, &m, a(1 + s), a(2 + s).i() as f32, a(3 + s).i() as f32);
            V::Void
        }
        "renderItemDecorations" => {
            item_decorations(rt, &m, a(2), a(3).i(), a(4).i(), if desc.contains("Ljava/lang/String;") { a(5) } else { V::NULL });
            V::Void
        }
        "renderTooltip" | "renderComponentTooltip" | "renderComponentTooltipFromElements" => {
            tooltip_call(rt, g, desc, name, args);
            V::Void
        }
        _ => return None,
    })
}

/// `renderItemDecorations`: the count (or `text`) at the slot's bottom right, the durability bar.
fn item_decorations(rt: &mut Rt, m: &M4, stack: V, x: i32, y: i32, text: V) {
    if crate::api_common::stack_empty(rt, stack) {
        return;
    }
    let count = rt.get_field(stack, "$count").i();
    if count != 1 || text.r() != 0 {
        let s = match rt.str_of(text) {
            Some(t) => t.to_owned(),
            None => count.to_string(),
        };
        let r = vec![St::default().run(s)];
        let w = width_of(rt, &r).ceil() as i32;
        draw_runs(rt, &r, (x + 19 - 2 - w) as f32, (y + 6 + 3) as f32, 0xFF_FFFF, true, m);
    }
    const VISIBLE: &str = "net/minecraft/world/item/ItemStack.isBarVisible.()Z";
    const WIDTH: &str = "net/minecraft/world/item/ItemStack.getBarWidth.()I";
    const COLOR: &str = "net/minecraft/world/item/ItemStack.getBarColor.()I";
    if crate::api::call(rt, VISIBLE, &[stack]).i() != 0 {
        let w = crate::api::call(rt, WIDTH, &[stack]).i();
        let c = crate::api::call(rt, COLOR, &[stack]).i();
        let (bx, by) = (x + 2, y + 13);
        fill(rt, m, bx, by, bx + 13, by + 2, 0xFF00_0000_u32 as i32);
        fill(rt, m, bx, by, bx + w, by + 1, c | 0xFF00_0000_u32 as i32);
    }
}

// ---- tooltips ----

/// A tooltip's line of text (`ClientTooltipComponent.create(FormattedCharSequence)`).
fn text_tooltip(rt: &mut Rt, text: V) -> V {
    let t = rt.new_object(TEXT_TOOLTIP);
    rt.put_field(t, "$text", text);
    t
}

fn tooltip_width(rt: &mut Rt, c: V) -> i32 {
    if rt.class_of(c) == TEXT_TOOLTIP {
        let t = rt.get_field(c, "$text");
        return width(rt, t);
    }
    let font = font(rt);
    rt.call_functional(TOOLTIP_WIDTH, c, &[font]).i()
}

fn tooltip_height(rt: &mut Rt, c: V) -> i32 {
    if rt.class_of(c) == TEXT_TOOLTIP {
        return 10;
    }
    rt.call_functional(TOOLTIP_HEIGHT, c, &[]).i()
}

/// `ClientHooks.gatherTooltipComponentsFromElements` for text lines: wrapped where the tooltip
/// would not fit beside the mouse.
fn gather(rt: &mut Rt, stack: V, texts: Vec<V>, mouse_x: i32, screen_w: i32, screen_h: i32) -> Vec<V> {
    // Mods may add to or change the lines (`RenderTooltipEvent.GatherComponents`).
    let ev = rt.new_object(TOOLTIP_GATHER);
    let elements: Vec<V> = texts
        .into_iter()
        .map(|t| {
            let e = rt.new_object("com/mojang/datafixers/util/Either");
            rt.put_field(e, "$left", t);
            e
        })
        .collect();
    let list = crate::jutil::new_list(rt, elements);
    for (k, v) in [("$stack", stack), ("$screenWidth", V::I(screen_w)), ("$screenHeight", V::I(screen_h)), ("$elements", list), ("$maxWidth", V::I(-1))] {
        rt.put_field(ev, k, v);
    }
    crate::neo::post(rt, TOOLTIP_GATHER, ev);
    if rt.get_field(ev, "$canceled").i() != 0 {
        return Vec::new();
    }
    let texts: Vec<V> = crate::jutil::elements(rt, rt.get_field(ev, "$elements")).into_iter().map(|e| rt.get_field(e, "$left")).filter(|t| t.r() != 0).collect();
    let mut text_width = texts.iter().map(|&t| width(rt, t)).max().unwrap_or(0);
    let mut wrap = false;
    let x = mouse_x + 12;
    if x + text_width + 4 > screen_w && mouse_x - 16 - text_width < 4 {
        text_width = if mouse_x > screen_w / 2 { mouse_x - 12 - 8 } else { screen_w - 16 - mouse_x };
        wrap = true;
    }
    let max = rt.get_field(ev, "$maxWidth").i();
    if max > 0 && text_width > max {
        text_width = max;
        wrap = true;
    }
    let mut out = Vec::new();
    for t in texts {
        if wrap && !(is_component(rt, t) && plain(rt, t).is_empty()) {
            for l in split_components(rt, t, text_width) {
                let s = sequence(rt, l, "");
                out.push(text_tooltip(rt, s));
            }
        } else {
            let s = sequence(rt, t, "");
            out.push(text_tooltip(rt, s));
        }
    }
    out
}

/// `GuiGraphics.renderTooltipInternal`: the tooltip box beside the mouse, its lines on it.
fn render_tooltip(rt: &mut Rt, g: V, stack: V, components: Vec<V>, mouse_x: i32, mouse_y: i32) {
    if components.is_empty() {
        return;
    }
    let [sw, sh] = rt.host().map_or([0, 0], |h| h.gui_size());
    let font = font(rt);
    let list = crate::jutil::new_list(rt, components.clone());
    let pre = rt.new_object(TOOLTIP_PRE);
    for (k, v) in [("$stack", stack), ("$graphics", g), ("$x", V::I(mouse_x)), ("$y", V::I(mouse_y)), ("$screenWidth", V::I(sw)), ("$screenHeight", V::I(sh)), ("$font", font), ("$components", list)] {
        rt.put_field(pre, k, v);
    }
    crate::neo::post(rt, TOOLTIP_PRE, pre);
    if rt.get_field(pre, "$canceled").i() != 0 {
        return;
    }
    let (mx, my) = (rt.get_field(pre, "$x").i(), rt.get_field(pre, "$y").i());
    let mut w = 0;
    let mut h = if components.len() == 1 { -2 } else { 0 };
    for &c in &components {
        w = w.max(tooltip_width(rt, c));
        h += tooltip_height(rt, c);
    }
    // `DefaultTooltipPositioner`: beside the mouse, kept on the screen.
    let (mut x, mut y) = (mx + 12, my - 12);
    if x + w > sw {
        x = (x - 24 - w).max(4);
    }
    if y + h + 3 > sh {
        y = sh - h - 3;
    }
    let color = rt.new_object(TOOLTIP_COLOR);
    for (k, v) in [
        ("$stack", stack),
        ("$graphics", g),
        ("$x", V::I(x)),
        ("$y", V::I(y)),
        ("$font", font),
        ("$backgroundStart", V::I(TOOLTIP_BACKGROUND)),
        ("$backgroundEnd", V::I(TOOLTIP_BACKGROUND)),
        ("$borderStart", V::I(TOOLTIP_BORDER_TOP)),
        ("$borderEnd", V::I(TOOLTIP_BORDER_BOTTOM)),
        ("$components", list),
    ] {
        rt.put_field(color, k, v);
    }
    crate::neo::post(rt, TOOLTIP_COLOR, color);
    let c = |k| rt.get_field(color, k).i();
    let colors = [c("$backgroundStart"), c("$backgroundEnd"), c("$borderStart"), c("$borderEnd")];
    let m = pose_of(rt, g);
    tooltip_background(rt, &m, x, y, w, h, colors);
    let mut ty = y;
    for (k, &comp) in components.iter().enumerate() {
        if rt.class_of(comp) == TEXT_TOOLTIP {
            let t = rt.get_field(comp, "$text");
            let r = runs(rt, t);
            draw_runs(rt, &r, x as f32, ty as f32, -1, true, &m);
        } else {
            let matrix = render::new_mat4(rt, m);
            let buffers = rt.intern("compat:gui-buffers".into(), |rt| rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource"));
            rt.call_functional(TOOLTIP_TEXT, comp, &[font, V::I(x), V::I(ty), matrix, buffers]);
            rt.call_functional(TOOLTIP_IMAGE, comp, &[font, V::I(x), V::I(ty), g]);
        }
        ty += tooltip_height(rt, comp) + if k == 0 { 2 } else { 0 };
    }
}

/// `TooltipRenderUtil.renderTooltipBackground`: the box round text `width` x `height` at
/// `(x, y)`, with NeoForge's gradients (`[background top, bottom, border top, bottom]`).
fn tooltip_background(rt: &mut Rt, m: &M4, x: i32, y: i32, width: i32, height: i32, [bg_top, bg_bottom, border_top, border_bottom]: [i32; 4]) {
    let (i, j, k, l) = (x - 3, y - 3, width + 6, height + 6);
    fill(rt, m, i, j - 1, i + k, j, bg_top);
    fill(rt, m, i, j + l, i + k, j + l + 1, bg_bottom);
    fill_gradient(rt, m, i, j, i + k, j + l, bg_top, bg_bottom);
    fill_gradient(rt, m, i - 1, j, i, j + l, bg_top, bg_bottom);
    fill_gradient(rt, m, i + k, j, i + k + 1, j + l, bg_top, bg_bottom);
    // The frame, one pixel in.
    let (fy, fl) = (j + 1, l);
    fill_gradient(rt, m, i, fy, i + 1, fy + fl - 2, border_top, border_bottom);
    fill_gradient(rt, m, i + k - 1, fy, i + k, fy + fl - 2, border_top, border_bottom);
    fill(rt, m, i, fy - 1, i + k, fy, border_top);
    fill(rt, m, i, fy - 1 + fl - 1, i + k, fy + fl - 1, border_bottom);
}

/// The tooltip calls of `GuiGraphics`: lines from components (gathered as NeoForge does), from
/// sequences, from a stack's name.
fn tooltip_call(rt: &mut Rt, g: V, desc: &str, name: &str, args: &[V]) {
    let a = |i| arg(args, i);
    let [sw, sh] = rt.host().map_or([0, 0], |h| h.gui_size());
    let stack_last = desc.ends_with("Lnet/minecraft/world/item/ItemStack;)V");
    let (lines, x, y, stack, sequences) = match (name, desc) {
        // (font, stack, x, y): the stack's name.
        (_, "(Lnet/minecraft/client/gui/Font;Lnet/minecraft/world/item/ItemStack;II)V") => {
            let n = crate::api::call(rt, "net/minecraft/world/item/ItemStack.getHoverName.()Lnet/minecraft/network/chat/Component;", &[a(2)]);
            (vec![n], a(3).i(), a(4).i(), a(2), false)
        }
        // (font, component, x, y)
        (_, "(Lnet/minecraft/client/gui/Font;Lnet/minecraft/network/chat/Component;II)V") => (vec![a(2)], a(3).i(), a(4).i(), V::NULL, false),
        // (font, lines, Optional, [stack,] x, y)
        (_, d) if d.contains("Ljava/util/Optional;") => {
            let lines = crate::jutil::elements(rt, a(2));
            let (x, y, stack) = if d.contains("Lnet/minecraft/world/item/ItemStack;II") { (a(5).i(), a(6).i(), a(4)) } else { (a(4).i(), a(5).i(), V::NULL) };
            (lines, x, y, stack, false)
        }
        // (font, sequences, positioner, x, y) and (font, sequences, x, y)
        ("renderTooltip", d) if d.contains("ClientTooltipPositioner;") => (crate::jutil::elements(rt, a(2)), a(4).i(), a(5).i(), V::NULL, true),
        ("renderTooltip", _) => (crate::jutil::elements(rt, a(2)), a(3).i(), a(4).i(), V::NULL, true),
        // renderComponentTooltip(font, lines, x, y[, stack]), FromElements(font, eithers, x, y, stack)
        (_, _) => {
            let mut lines = crate::jutil::elements(rt, a(2));
            if name == "renderComponentTooltipFromElements" {
                lines = lines.into_iter().map(|e| rt.get_field(e, "$left")).filter(|t| t.r() != 0).collect();
            }
            (lines, a(3).i(), a(4).i(), if stack_last { a(5) } else { V::NULL }, false)
        }
    };
    let components = if sequences {
        lines.into_iter().map(|s| text_tooltip(rt, s)).collect()
    } else {
        gather(rt, stack, lines, x, sw, sh)
    };
    render_tooltip(rt, g, stack, components, x, y);
}

fn tooltip_calls(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        ("net/neoforged/neoforge/client/ClientHooks", "gatherTooltipComponents") => {
            // (stack, texts, [Optional,] mouseX, screenWidth, screenHeight, font)
            let o = desc.contains("Ljava/util/Optional;") as usize;
            let texts = crate::jutil::elements(rt, a(1));
            let out = gather(rt, a(0), texts, a(2 + o).i(), a(3 + o).i(), a(4 + o).i());
            crate::jutil::new_list(rt, out)
        }
        ("net/neoforged/neoforge/client/ClientHooks", "gatherTooltipComponentsFromElements") => {
            let texts = crate::jutil::elements(rt, a(1)).into_iter().map(|e| rt.get_field(e, "$left")).filter(|t| t.r() != 0).collect();
            let out = gather(rt, a(0), texts, a(2).i(), a(3).i(), a(4).i());
            crate::jutil::new_list(rt, out)
        }
        (TOOLTIP | TEXT_TOOLTIP, "create") if desc.starts_with("(Lnet/minecraft/util/FormattedCharSequence;)") => text_tooltip(rt, a(0)),
        (TOOLTIP | TEXT_TOOLTIP, "getWidth") if rt.class_of(a(0)) == TEXT_TOOLTIP => V::I(tooltip_width(rt, a(0))),
        (TOOLTIP | TEXT_TOOLTIP, "getHeight") if rt.class_of(a(0)) == TEXT_TOOLTIP => V::I(10),
        (TOOLTIP | TEXT_TOOLTIP, "renderText") if rt.class_of(a(0)) == TEXT_TOOLTIP => {
            // (font, x, y, matrix, buffers): the line in white, with its shadow.
            let m = render::mat4(rt, a(4));
            let t = rt.get_field(a(0), "$text");
            let r = runs(rt, t);
            draw_runs(rt, &r, a(2).i() as f32, a(3).i() as f32, -1, true, &m);
            V::Void
        }
        (TOOLTIP | TEXT_TOOLTIP, "renderImage") if rt.class_of(a(0)) == TEXT_TOOLTIP => V::Void,
        ("net/minecraft/client/gui/screens/inventory/tooltip/TooltipRenderUtil", "renderTooltipBackground") => {
            // (graphics, x, y, width, height, z[, bgTop, bgBottom, borderTop, borderBottom])
            let m = pose_of(rt, a(0));
            let colors = if desc.ends_with("IIIIIIIIII)V") {
                [a(6).i(), a(7).i(), a(8).i(), a(9).i()]
            } else {
                [TOOLTIP_BACKGROUND, TOOLTIP_BACKGROUND, TOOLTIP_BORDER_TOP, TOOLTIP_BORDER_BOTTOM]
            };
            tooltip_background(rt, &m, a(1).i(), a(2).i(), a(3).i(), a(4).i(), colors);
            V::Void
        }
        // The tooltip events a mod makes and posts itself (Create's `RemovedGuiUtils`), or reads.
        (TOOLTIP_PRE, "<init>") => {
            for (k, i) in [("$stack", 1), ("$graphics", 2), ("$x", 3), ("$y", 4), ("$screenWidth", 5), ("$screenHeight", 6), ("$font", 7), ("$components", 8)] {
                rt.put_field(a(0), k, a(i));
            }
            V::Void
        }
        (TOOLTIP_COLOR, "<init>") => {
            // (stack, graphics, x, y, font, background, borderStart, borderEnd, components)
            for (k, i) in [("$stack", 1), ("$graphics", 2), ("$x", 3), ("$y", 4), ("$font", 5), ("$backgroundStart", 6), ("$backgroundEnd", 6), ("$borderStart", 7), ("$borderEnd", 8), ("$components", 9)] {
                rt.put_field(a(0), k, a(i));
            }
            V::Void
        }
        (o, n) if o.starts_with(TOOLTIP_EVENT) && rt.class_of(a(0)).starts_with(TOOLTIP_EVENT) => {
            let field = match n {
                "getItemStack" => "$stack",
                "getGraphics" => "$graphics",
                "getX" => "$x",
                "getY" => "$y",
                "getScreenWidth" => "$screenWidth",
                "getScreenHeight" => "$screenHeight",
                "getFont" => "$font",
                "getComponents" => "$components",
                "getTooltipElements" => "$elements",
                "getMaxWidth" => "$maxWidth",
                "getBackgroundStart" | "getOriginalBackgroundStart" => "$backgroundStart",
                "getBackgroundEnd" | "getOriginalBackgroundEnd" => "$backgroundEnd",
                "getBorderStart" | "getOriginalBorderStart" => "$borderStart",
                "getBorderEnd" | "getOriginalBorderEnd" => "$borderEnd",
                "isCanceled" => "$canceled",
                "setX" | "setY" | "setFont" | "setMaxWidth" | "setBackgroundStart" | "setBackgroundEnd" | "setBorderStart" | "setBorderEnd" | "setCanceled" => {
                    let f = match n {
                        "setX" => "$x",
                        "setY" => "$y",
                        "setFont" => "$font",
                        "setMaxWidth" => "$maxWidth",
                        "setBackgroundStart" => "$backgroundStart",
                        "setBackgroundEnd" => "$backgroundEnd",
                        "setBorderStart" => "$borderStart",
                        "setBorderEnd" => "$borderEnd",
                        _ => "$canceled",
                    };
                    rt.put_field(a(0), f, a(1));
                    return Some(V::Void);
                }
                "setBackground" => {
                    rt.put_field(a(0), "$backgroundStart", a(1));
                    rt.put_field(a(0), "$backgroundEnd", a(1));
                    return Some(V::Void);
                }
                _ => return None,
            };
            let v = rt.get_field(a(0), field);
            if field == "$canceled" { V::I(v.i()) } else { v }
        }
        ("net/neoforged/bus/api/ICancellableEvent", "isCanceled") if rt.class_of(a(0)).starts_with(TOOLTIP_EVENT) => V::I(rt.get_field(a(0), "$canceled").i()),
        ("net/neoforged/bus/api/ICancellableEvent", "setCanceled") if rt.class_of(a(0)).starts_with(TOOLTIP_EVENT) => {
            rt.put_field(a(0), "$canceled", V::I(a(1).i()));
            V::Void
        }
        _ => return None,
    })
}

// ---- the client: Minecraft's GUI fields, its window, the font ----

/// The client's font (`Minecraft.font`): one object.
pub fn font(rt: &mut Rt) -> V {
    rt.intern("compat:font".into(), |rt| {
        let f = rt.new_object(FONT);
        rt.put_field(f, "lineHeight", V::I(LINE_HEIGHT));
        f
    })
}

fn single(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("client/{class}"), |rt| rt.new_object(class))
}

/// `CameraType`'s constants, in its order.
const CAMERA_TYPES: [&str; 3] = ["FIRST_PERSON", "THIRD_PERSON_BACK", "THIRD_PERSON_FRONT"];

/// `Minecraft`'s GUI fields, set once: its font, options (the HUD shown), game mode, mouse and
/// window.
pub fn fill_minecraft(rt: &mut Rt, mc: V) {
    if rt.get_field(mc, "font").r() != 0 {
        return;
    }
    let f = font(rt);
    rt.put_field(mc, "font", f);
    let options = single(rt, "net/minecraft/client/Options");
    rt.put_field(options, "hideGui", V::I(0));
    rt.put_field(options, "advancedItemTooltips", V::I(0));
    crate::screens::fill_options(rt, options);
    rt.put_field(mc, "options", options);
    let mode = single(rt, GAME_MODE);
    rt.put_field(mc, "gameMode", mode);
    let mouse = single(rt, "net/minecraft/client/MouseHandler");
    rt.put_field(mc, "mouseHandler", mouse);
    let keyboard = single(rt, "net/minecraft/client/KeyboardHandler");
    rt.put_field(mc, "keyboardHandler", keyboard);
    let renderer = single(rt, "net/minecraft/client/renderer/GameRenderer");
    rt.put_field(mc, "gameRenderer", renderer);
}

/// The game type the client's player plays in.
fn game_type(rt: &mut Rt) -> V {
    let i = rt.host().is_some_and(|h| h.player_creative()) as usize;
    crate::api::api_enum(rt, GAME_TYPE, GAME_TYPES[i], i as i32)
}

fn client_calls(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let size = |rt: &Rt| rt.host().map_or([0, 0], |h| h.gui_size());
    let scale = |rt: &Rt| rt.host().map_or(1.0, |h| h.gui_scale());
    Some(match (owner, name) {
        (MINECRAFT, "getWindow") => single(rt, WINDOW),
        (MINECRAFT, "renderBuffers") => single(rt, "net/minecraft/client/renderer/RenderBuffers"),
        // The models' set, as a renderer's context hands it (`new BlockEntityWithoutLevelRenderer(
        // dispatcher, Minecraft.getInstance().getEntityModels())` for a mod's item renderer).
        (MINECRAFT, "getEntityModels") => single(rt, "net/minecraft/client/model/geom/EntityModelSet"),
        (MINECRAFT, "getBlockEntityRenderDispatcher") => single(rt, "net/minecraft/client/renderer/blockentity/BlockEntityRenderDispatcher"),
        // The client's language: the game's (English until it says otherwise), `Locale`'s one object.
        (MINECRAFT, "getLanguageManager") => single(rt, LANGUAGE_MANAGER),
        (LANGUAGE_MANAGER, "getSelected") => rt.string("en_us"),
        (LANGUAGE_MANAGER, "getJavaLocale") | (MINECRAFT, "getLocale") => rt.intern("compat:locale".into(), |rt| rt.new_object("java/util/Locale")),
        // The client's runtime holds no server object, even in single player (the game's server
        // thread runs its own): a mod syncs over the network, as to a remote server.
        (MINECRAFT, "hasSingleplayerServer") => V::I(0),
        // Mixin accessors onto the client: the screen open (none until one opens) and the particle
        // engine, whose providers map a mod may fill (the game draws particles from its own).
        (MINECRAFT, "$screen") => V::NULL,
        (MINECRAFT, "$particleEngine") => single(rt, PARTICLE_ENGINE),
        (PARTICLE_ENGINE, "$providers") => {
            let engine = a(0);
            match rt.get_field(engine, "$providers") {
                m if m.r() != 0 => m,
                _ => {
                    let m = rt.alloc("java/util/HashMap", crate::Data::Table(Default::default()));
                    rt.put_field(engine, "$providers", m);
                    m
                }
            }
        }
        ("net/minecraft/client/renderer/RenderBuffers", "bufferSource" | "crumblingBufferSource") => {
            rt.intern("compat:gui-buffers".into(), |rt| rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource"))
        }
        // The batch ends: the vertices written into it are drawn (`endBatch(type)`: that type's).
        ("net/minecraft/client/renderer/MultiBufferSource$BufferSource", "endBatch" | "endLastBatch") => {
            let only = a(1);
            crate::vertices::flush(rt, (only.r() != 0).then_some(only));
            V::Void
        }
        (WINDOW, "getGuiScaledWidth") => V::I(size(rt)[0]),
        (WINDOW, "getGuiScaledHeight") => V::I(size(rt)[1]),
        (WINDOW, "getGuiScale") => V::D(scale(rt)),
        (WINDOW, "getWidth" | "getScreenWidth") => V::I((f64::from(size(rt)[0]) * scale(rt)) as i32),
        (WINDOW, "getHeight" | "getScreenHeight") => V::I((f64::from(size(rt)[1]) * scale(rt)) as i32),
        (WINDOW, "getWindow") => V::J(0),
        (WINDOW, "getX" | "getY") => V::I(0),
        (WINDOW, "isFullscreen") => V::I(0),
        (GAME_MODE, "getPlayerMode") => game_type(rt),
        (GAME_MODE, "getPreviousPlayerMode") => V::NULL,
        (GAME_MODE, "canHurtPlayer" | "hasExperience" | "hasMissTime") => V::I(!rt.host().is_some_and(|h| h.player_creative()) as i32),
        (GAME_MODE, "hasInfiniteItems" | "isAlwaysFlying") => V::I(rt.host().is_some_and(|h| h.player_creative()) as i32),
        (GAME_MODE, "isServerControlledInventory" | "hasFarPickRange" | "isDestroying") => V::I(0),
        (GAME_TYPE, "isCreative") => V::I((rt.get_field(a(0), "$ordinal").i() == 1) as i32),
        (GAME_TYPE, "isSurvival") => V::I(matches!(rt.get_field(a(0), "$ordinal").i(), 0 | 2) as i32),
        (GAME_TYPE, "isBlockPlacingRestricted") => V::I(matches!(rt.get_field(a(0), "$ordinal").i(), 2 | 3) as i32),
        (GAME_TYPE, "getName") => {
            let n = formatting_name(rt, a(0)).to_ascii_lowercase();
            rt.string(&n)
        }
        // Textures by location: one object each, whose GL settings the host keeps itself.
        ("net/minecraft/client/renderer/texture/TextureManager", "getTexture") => {
            let at = rt.str_of(a(1)).unwrap_or("").to_owned();
            rt.intern(format!("compat:texture/{at}"), |rt| rt.new_object("net/minecraft/client/renderer/texture/AbstractTexture"))
        }
        ("net/minecraft/client/renderer/texture/AbstractTexture", "setFilter" | "bind" | "close" | "releaseId") => V::Void,
        ("net/minecraft/client/renderer/texture/AbstractTexture", "getId") => V::I(0),
        // The pointer in the window's pixels.
        ("net/minecraft/client/MouseHandler", "xpos" | "ypos") => {
            let c = rt.host().map_or([0.0, 0.0], |h| h.cursor());
            V::D(f64::from(c[usize::from(name == "ypos")]) * scale(rt))
        }
        ("net/minecraft/client/MouseHandler", "isMouseGrabbed") => V::I(1),
        ("net/minecraft/client/KeyboardHandler", "getClipboard") => {
            let text = crate::screens::clipboard(rt);
            rt.string(&text)
        }
        ("net/minecraft/client/KeyboardHandler", "setClipboard") => {
            let text = rt.str_of(a(1)).unwrap_or("").to_owned();
            crate::screens::set_clipboard(rt, &text);
            V::Void
        }
        ("net/minecraft/client/Options", "getCameraType") => crate::api::api_enum(rt, "net/minecraft/client/CameraType", "FIRST_PERSON", 0),
        ("net/minecraft/client/CameraType", "isFirstPerson") => V::I((rt.get_field(a(0), "$ordinal").i() == 0) as i32),
        // `THIRD_PERSON_FRONT` looks back at the player.
        ("net/minecraft/client/CameraType", "isMirrored") => V::I((rt.get_field(a(0), "$ordinal").i() == 2) as i32),
        ("net/minecraft/client/CameraType", "cycle") => {
            let next = (rt.get_field(a(0), "$ordinal").i() + 1).rem_euclid(3);
            crate::api::api_enum(rt, "net/minecraft/client/CameraType", CAMERA_TYPES[next as usize], next)
        }
        // The game keeps its own view (F5 is the player's).
        ("net/minecraft/client/Options", "setCameraType") => {
            crate::glue::trace_once(rt, "camera-type", |_| "client: a mod set the camera type; the game keeps its own view".into());
            V::Void
        }
        // GL state for what it draws next: the host keeps its own; only the tint reaches blits.
        ("com/mojang/blaze3d/systems/RenderSystem", "setShaderColor") => {
            let c = |k: usize, shift: u32| ((a(k).f().clamp(0.0, 1.0) * 255.0).round() as u32) << shift;
            let tint = c(0, 16) | c(1, 8) | c(2, 0) | c(3, 24);
            rt.interned.insert(TINT.into(), V::I(tint as i32));
            V::Void
        }
        ("com/mojang/blaze3d/systems/RenderSystem", "getShaderColor") => {
            let t = tint(rt);
            let arr = rt.alloc("[F", Data::Array([16, 8, 0, 24].iter().map(|s| V::F(((t >> s) & 0xFF) as f32 / 255.0)).collect()));
            arr
        }
        ("com/mojang/blaze3d/systems/RenderSystem", n) if !n.starts_with("get") => V::Void,
        ("com/mojang/blaze3d/platform/Lighting", _) => V::Void,
        _ => return None,
    })
}

// ---- HUD layers ----

/// Vanilla's HUD layers in drawing order (`Gui`'s layer manager, NeoForge's `VanillaGuiLayers`).
const VANILLA_LAYERS: [&str; 25] = [
    "minecraft:camera_overlays",
    "minecraft:crosshair",
    "minecraft:hotbar",
    "minecraft:jump_meter",
    "minecraft:experience_bar",
    "minecraft:player_health",
    "minecraft:armor_level",
    "minecraft:food_level",
    "minecraft:vehicle_health",
    "minecraft:air_level",
    "minecraft:selected_item_name",
    "minecraft:spectator_tooltip",
    "minecraft:experience_level",
    "minecraft:effects",
    "minecraft:boss_overlay",
    "minecraft:sleep_overlay",
    "minecraft:demo_overlay",
    "minecraft:debug_overlay",
    "minecraft:scoreboard_sidebar",
    "minecraft:overlay_message",
    "minecraft:title",
    "minecraft:chat",
    "minecraft:tab_list",
    "minecraft:subtitle_overlay",
    "minecraft:saving_indicator",
];

/// Where a mod's layer drawn right after vanilla's layer `id` goes in the game's HUD: after
/// (`false`) or before (`true`) the game's layer that draws it. Some of the game's layers draw
/// several of vanilla's (`minecraft:player_health` the health, armor, food and air rows); what
/// comes before the crosshair, or after it, goes before the hotbar (the crosshair isn't drawn in
/// third person).
fn hook_after(id: &str) -> (&'static str, bool) {
    match id.trim_start_matches("minecraft:") {
        "camera_overlays" | "crosshair" => ("minecraft:hotbar", true),
        "hotbar" => ("minecraft:hotbar", false),
        "jump_meter" | "experience_bar" | "experience_level" => ("minecraft:contextual_info_bar", false),
        "player_health" | "armor_level" | "food_level" | "vehicle_health" | "air_level" => ("minecraft:player_health", false),
        "selected_item_name" | "spectator_tooltip" => ("minecraft:selected_item_name", false),
        "effects" => ("minecraft:effects", false),
        "boss_overlay" => ("minecraft:boss_overlay", false),
        "sleep_overlay" | "demo_overlay" | "debug_overlay" => ("minecraft:sleep_overlay", false),
        "scoreboard_sidebar" => ("minecraft:scoreboard_sidebar", false),
        "overlay_message" => ("minecraft:overlay_message", false),
        "title" => ("minecraft:title", false),
        "chat" => ("minecraft:chat", false),
        "subtitle_overlay" => ("minecraft:subtitle_overlay", false),
        _ => ("minecraft:tab_list", false),
    }
}

/// The game's layers vanilla's layer `id` alone stands for: one a mod can replace.
fn replaceable(id: &str) -> Option<&'static str> {
    Some(match id.trim_start_matches("minecraft:") {
        "crosshair" => "minecraft:crosshair",
        "hotbar" => "minecraft:hotbar",
        "effects" => "minecraft:effects",
        "boss_overlay" => "minecraft:boss_overlay",
        "selected_item_name" => "minecraft:selected_item_name",
        "sleep_overlay" => "minecraft:sleep_overlay",
        "scoreboard_sidebar" => "minecraft:scoreboard_sidebar",
        "overlay_message" => "minecraft:overlay_message",
        "title" => "minecraft:title",
        "chat" => "minecraft:chat",
        "tab_list" => "minecraft:tab_list",
        "subtitle_overlay" => "minecraft:subtitle_overlay",
        "vehicle_health" => "minecraft:vehicle_health",
        _ => return None,
    })
}

/// The HUD's layers in order, vanilla's and the mods': objects with `$id` and, for a mod's, the
/// `$layer` it draws; `$replaces` on a mod's layer standing for vanilla's.
fn layer_list(rt: &mut Rt) -> V {
    rt.intern("compat:gui-layers".into(), |rt| {
        let items: Vec<V> = VANILLA_LAYERS
            .iter()
            .map(|id| {
                let e = rt.new_object("compat:gui-layer");
                let s = rt.string(id);
                rt.put_field(e, "$id", s);
                e
            })
            .collect();
        crate::jutil::new_list(rt, items)
    })
}

fn layer_entries(rt: &mut Rt) -> Vec<V> {
    let list = layer_list(rt);
    crate::jutil::elements(rt, list)
}

/// `RegisterGuiLayersEvent.register`: a mod's layer `layer` named `id`, before or after layer
/// `other` (None: all of them).
fn register_layer(rt: &mut Rt, before: bool, other: Option<String>, id: V, layer: V) {
    let entries = layer_entries(rt);
    let index = match &other {
        None => Some(if before { 0 } else { entries.len() }),
        Some(o) => entries.iter().position(|&e| rt.str_of(rt.get_field(e, "$id")) == Some(o.as_str())).map(|i| if before { i } else { i + 1 }),
    };
    let Some(index) = index else {
        let o = other.unwrap_or_default();
        crate::glue::trace_once(rt, &format!("gui-layer/{o}"), |_| format!("client: a GUI layer ordered against unknown layer {o}"));
        return;
    };
    let e = rt.new_object("compat:gui-layer");
    let name = rt.str_of(id).unwrap_or("").to_owned();
    let s = rt.string(&name);
    rt.put_field(e, "$id", s);
    rt.put_field(e, "$layer", layer);
    let list = layer_list(rt);
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.insert(index.min(l.len()), e);
    }
}

fn layers_event(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let text = |rt: &Rt, v: V| rt.str_of(v).map(str::to_owned);
    match name {
        "registerBelowAll" => register_layer(rt, true, None, a(1), a(2)),
        "registerAboveAll" => register_layer(rt, false, None, a(1), a(2)),
        "registerBelow" => {
            let o = text(rt, a(1));
            register_layer(rt, true, o, a(2), a(3));
        }
        "registerAbove" => {
            let o = text(rt, a(1));
            register_layer(rt, false, o, a(2), a(3));
        }
        // A replacement draws in the layer's place; a wrapper of vanilla's own drawing can't.
        "replaceLayer" | "wrapLayer" => {
            let id = text(rt, a(1)).unwrap_or_default();
            let layer = if name == "replaceLayer" {
                a(2)
            } else {
                crate::glue::trace_once(rt, &format!("gui-wrap/{id}"), |_| format!("client: GUI layer {id} wrapped, drawn unwrapped"));
                return Some(V::Void);
            };
            for e in layer_entries(rt) {
                if rt.str_of(rt.get_field(e, "$id")) == Some(id.as_str()) {
                    rt.put_field(e, "$layer", layer);
                    rt.put_field(e, "$replaces", V::I(1));
                }
            }
        }
        _ => return None,
    }
    Some(V::Void)
}

/// The mods' layers the game draws at its layer `layer` (`minecraft:hotbar`), before it (`pre`)
/// or after it, in order: each with whether it stands in for vanilla's layer there.
fn layers_at(rt: &mut Rt, layer: &str, pre: bool) -> Vec<(V, bool)> {
    let mut out = Vec::new();
    let mut anchor: Option<String> = None;
    for e in layer_entries(rt) {
        let id = rt.str_of(rt.get_field(e, "$id")).unwrap_or("").to_owned();
        let drawn = rt.get_field(e, "$layer");
        let vanilla = VANILLA_LAYERS.contains(&id.as_str());
        if vanilla {
            if drawn.r() != 0 && rt.get_field(e, "$replaces").i() != 0 && replaceable(&id) == Some(layer) && pre {
                out.push((drawn, true));
            }
            anchor = Some(id);
            continue;
        }
        let (hook, before) = match &anchor {
            Some(a) => hook_after(a),
            None => ("minecraft:hotbar", true),
        };
        if hook == layer && before == pre {
            out.push((drawn, false));
        }
    }
    out
}

/// A fresh `GuiGraphics` for a frame's drawing: its pose the identity.
pub fn graphics(rt: &mut Rt) -> V {
    let g = rt.new_object(GRAPHICS);
    let pose = render::new_pose_stack(rt, render::IDENTITY);
    rt.put_field(g, "$pose", pose);
    g
}

/// What the mods' GUI code reads of the client this frame: the block under the crosshair
/// (`Minecraft.hitResult`) and the fields [`fill_minecraft`] sets.
pub(crate) fn frame(rt: &mut Rt) {
    let mc = crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[]);
    fill_minecraft(rt, mc);
    let hit = crate::api_common::pov_hit_result(rt, 0);
    rt.put_field(mc, "hitResult", hit);
}

/// Draws the mods' HUD layers at the game's layer `layer`, before it (`pre`) or after it, through
/// the host's GUI. True when one of them stands in for the layer (`replaceLayer`): the game's own
/// drawing of it is skipped.
pub fn draw_layers(rt: &mut Rt, layer: &str, pre: bool) -> bool {
    let picked = layers_at(rt, layer, pre);
    if picked.is_empty() {
        return false;
    }
    frame(rt);
    let delta = rt.intern("client/net/minecraft/client/DeltaTracker".into(), |rt| rt.new_object("net/minecraft/client/DeltaTracker"));
    rt.interned.insert(DRAWING.into(), V::I(1));
    let mut replaced = false;
    for (l, replaces) in picked {
        let g = graphics(rt);
        rt.call_functional(LAYER_RENDER, l, &[g, delta]);
        rt.report_thrown("gui layer");
        replaced |= replaces;
    }
    rt.interned.remove(DRAWING);
    rt.interned.remove(TINT);
    replaced
}

/// `RegisterGuiLayersEvent`, posted with the client's setup.
pub const REGISTER_LAYERS: &str = LAYERS_EVENT;

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        GRAPHICS
            | LAYERS_EVENT
            | FONT
            | SPLITTER
            | SEQUENCE
            | FORMATTED
            | COMPONENT
            | MUTABLE
            | STYLE
            | TEXT_COLOR
            | FORMATTING
            | TOOLTIP
            | TEXT_TOOLTIP
            | TOOLTIP_PRE
            | TOOLTIP_COLOR
            | WINDOW
            | GAME_MODE
            | GAME_TYPE
            | "net/minecraft/locale/Language"
            | "net/minecraft/client/resources/language/I18n"
            | "net/minecraft/network/chat/ComponentUtils"
            | "net/minecraft/network/chat/CommonComponents"
            | "net/neoforged/neoforge/client/ClientHooks"
            | "net/minecraft/client/gui/screens/inventory/tooltip/TooltipRenderUtil"
            | "net/neoforged/bus/api/ICancellableEvent"
            | "net/minecraft/client/renderer/RenderBuffers"
            | "net/minecraft/client/renderer/MultiBufferSource$BufferSource"
            | "net/minecraft/client/renderer/texture/TextureManager"
            | "net/minecraft/client/renderer/texture/AbstractTexture"
            | "net/minecraft/client/MouseHandler"
            | "net/minecraft/client/KeyboardHandler"
            | "net/minecraft/client/Options"
            | "net/minecraft/client/CameraType"
            | "com/mojang/blaze3d/systems/RenderSystem"
            | "com/mojang/blaze3d/platform/Lighting"
    ) || owner.starts_with(TOOLTIP_EVENT)
        || (owner == MINECRAFT
            && matches!(
                name,
                "getWindow" | "renderBuffers" | "getLanguageManager" | "getLocale" | "hasSingleplayerServer" | "$screen" | "$particleEngine" | "getEntityModels" | "getBlockEntityRenderDispatcher"
            ))
        || owner == LANGUAGE_MANAGER
        || (owner == PARTICLE_ENGINE && name == "$providers")
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        GRAPHICS => graphics_call(rt, name, desc, args),
        LAYERS_EVENT => layers_event(rt, name, args),
        _ => text_calls(rt, owner, name, desc, args)
            .or_else(|| tooltip_calls(rt, owner, name, desc, args))
            .or_else(|| client_calls(rt, owner, name, args)),
    }
}

/// Static fields this module owns.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    let literal = |rt: &mut Rt, key: &str, text: &str, translatable: bool| {
        let t = text.to_owned();
        rt.intern(format!("compat:common/{key}"), move |rt| {
            let s = rt.string(&t);
            crate::api_common::component(rt, s, translatable, V::NULL)
        })
    };
    Some(match (owner, field) {
        ("net/minecraft/network/chat/CommonComponents", f) => match f {
            "EMPTY" => literal(rt, f, "", false),
            "SPACE" => literal(rt, f, " ", false),
            "NEW_LINE" => literal(rt, f, "\n", false),
            "ELLIPSIS" => literal(rt, f, "...", false),
            "GUI_DONE" => literal(rt, f, "gui.done", true),
            "GUI_CANCEL" => literal(rt, f, "gui.cancel", true),
            "GUI_YES" => literal(rt, f, "gui.yes", true),
            "GUI_NO" => literal(rt, f, "gui.no", true),
            "GUI_OK" => literal(rt, f, "gui.ok", true),
            "GUI_PROCEED" => literal(rt, f, "gui.proceed", true),
            "GUI_CONTINUE" => literal(rt, f, "gui.continue", true),
            "GUI_BACK" => literal(rt, f, "gui.back", true),
            "GUI_TO_TITLE" => literal(rt, f, "gui.toTitle", true),
            "OPTION_ON" => literal(rt, f, "options.on", true),
            "OPTION_OFF" => literal(rt, f, "options.off", true),
            _ => return None,
        },
        (STYLE, "EMPTY") => empty_style(rt),
        (STYLE, "DEFAULT_FONT") => rt.string("minecraft:default"),
        (FORMATTED, "EMPTY") => rt.intern("compat:formatted-empty".into(), |rt| {
            let e = rt.string("");
            let f = rt.new_object(FORMATTED);
            rt.put_field(f, "$of", e);
            f
        }),
        (SEQUENCE, "EMPTY") => rt.intern("compat:sequence-empty".into(), |rt| {
            let e = rt.string("");
            sequence(rt, e, "")
        }),
        (GAME_TYPE, f) => {
            let i = GAME_TYPES.iter().position(|n| *n == f)?;
            crate::api::api_enum(rt, GAME_TYPE, GAME_TYPES[i], i as i32)
        }
        ("net/minecraft/client/gui/Font$DisplayMode", f) => {
            let i = ["NORMAL", "SEE_THROUGH", "POLYGON_OFFSET"].iter().position(|n| *n == f)?;
            let name = ["NORMAL", "SEE_THROUGH", "POLYGON_OFFSET"][i];
            crate::api::api_enum(rt, "net/minecraft/client/gui/Font$DisplayMode", name, i as i32)
        }
        (POSITIONER, "INSTANCE") => single(rt, POSITIONER),
        // Blend factors (`RenderSystem.blendFunc`'s arguments, which draw nothing different here),
        // in declaration order with their GL values.
        ("com/mojang/blaze3d/platform/GlStateManager$SourceFactor" | "com/mojang/blaze3d/platform/GlStateManager$DestFactor", f) => {
            const FACTORS: [(&str, i32); 15] = [
                ("CONSTANT_ALPHA", 32771),
                ("CONSTANT_COLOR", 32769),
                ("DST_ALPHA", 772),
                ("DST_COLOR", 774),
                ("ONE", 1),
                ("ONE_MINUS_CONSTANT_ALPHA", 32772),
                ("ONE_MINUS_CONSTANT_COLOR", 32770),
                ("ONE_MINUS_DST_ALPHA", 773),
                ("ONE_MINUS_DST_COLOR", 775),
                ("ONE_MINUS_SRC_ALPHA", 771),
                ("ONE_MINUS_SRC_COLOR", 769),
                ("SRC_ALPHA", 770),
                ("SRC_ALPHA_SATURATE", 776),
                ("SRC_COLOR", 768),
                ("ZERO", 0),
            ];
            let source = owner.ends_with("SourceFactor");
            let listed: Vec<(&str, i32)> = FACTORS.iter().copied().filter(|(n, _)| source || *n != "SRC_ALPHA_SATURATE").collect();
            let i = listed.iter().position(|(n, _)| *n == f)?;
            let class = if source { "com/mojang/blaze3d/platform/GlStateManager$SourceFactor" } else { "com/mojang/blaze3d/platform/GlStateManager$DestFactor" };
            let e = crate::api::api_enum(rt, class, listed[i].0, i as i32);
            rt.put_field(e, "value", V::I(listed[i].1));
            e
        }
        // Vanilla's layers' ids (`HOTBAR` is `minecraft:hotbar`), which mods order theirs against.
        ("net/neoforged/neoforge/client/gui/VanillaGuiLayers", f) => {
            let id = format!("minecraft:{}", f.to_ascii_lowercase());
            if !VANILLA_LAYERS.contains(&id.as_str()) {
                return None;
            }
            rt.string(&id)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{GuiDraw, MemoryHost};

    fn text(rt: &mut Rt, s: &str) -> V {
        rt.string(s)
    }

    fn literal(rt: &mut Rt, s: &str, style: &str) -> V {
        let t = rt.string(s);
        let c = crate::api_common::component(rt, t, false, V::NULL);
        let st = rt.string(style);
        rt.put_field(c, "$style", st);
        c
    }

    fn run(text: &str, color: Option<u32>, bold: bool) -> TextRun {
        TextRun { text: text.into(), color, bold, ..TextRun::default() }
    }

    #[test]
    fn section_codes_change_the_style_as_vanilla_reads_them() {
        let mut rt = Rt::new();
        // §c red, §l bold on red, §a green (a colour clears bold), §r back to plain, a lone § ends it.
        let s = text(&mut rt, "a§cb§lc§ad§re§");
        assert_eq!(
            runs(&mut rt, s),
            vec![run("a", None, false), run("b", Some(0xFF5555), false), run("c", Some(0xFF5555), true), run("d", Some(0x55FF55), false), run("e", None, false)]
        );
    }

    #[test]
    fn a_translation_takes_its_arguments_in_place_with_their_styles() {
        let mut rt = Rt::new();
        let mut host = MemoryHost::default();
        host.lang.insert("create.gui.stress".into(), "Stress: %s of %2$s%%".into());
        let key = text(&mut rt, "create.gui.stress");
        let first = literal(&mut rt, "128", "GOLD,");
        let second = text(&mut rt, "256");
        let with = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![first, second]));
        let c = crate::api_common::component(&mut rt, key, true, with);
        let gray = rt.string("GRAY,");
        rt.put_field(c, "$style", gray);
        let got = rt.with_host(&mut host, |rt| runs(rt, c));
        assert_eq!(
            got,
            vec![run("Stress: ", Some(0xAAAAAA), false), run("128", Some(0xFFAA00), false), run(" of 256%", Some(0xAAAAAA), false)]
        );
        // A key the game doesn't know shows as itself; a format vanilla refuses, as written.
        assert_eq!(decompose("%d left", 1), None);
        assert_eq!(decompose("100% sure", 0), None);
        assert_eq!(decompose("%s and %s", 1), None);
        assert_eq!(decompose("%2$s %1$s", 2), Some(vec![Piece::Arg(1), Piece::Text(" ".into()), Piece::Arg(0)]));
    }

    #[test]
    fn lines_break_after_the_last_space_that_fits() {
        let rt = Rt::new();
        // Every character 6 wide without a host: 5 characters fit in 30.
        let r = vec![run("ab cd efgh", None, false)];
        let lines: Vec<String> = split(&rt, &r, 30.0).into_iter().map(|l| l.into_iter().map(|r| r.text).collect()).collect();
        assert_eq!(lines, vec!["ab cd", "efgh"]);
        // A word longer than a line breaks inside it; a newline always breaks, an ending one
        // leaves an empty line.
        let r = vec![run("abcdefgh\nx\n", None, false)];
        let lines: Vec<String> = split(&rt, &r, 30.0).into_iter().map(|l| l.into_iter().map(|r| r.text).collect()).collect();
        assert_eq!(lines, vec!["abcde", "fgh", "x", ""]);
        // Styles carry over the break.
        let r = vec![run("ab ", Some(1), false), run("cdefg", Some(2), true)];
        let lines = split(&rt, &r, 21.0);
        assert_eq!(lines, vec![vec![run("ab", Some(1), false)], vec![run("cde", Some(2), true)], vec![run("fg", Some(2), true)]]);
    }

    #[test]
    fn a_layer_registered_above_the_hotbar_draws_after_it_and_a_replacement_instead() {
        let mut rt = Rt::new();
        let (a, b, c) = (rt.new_object("Layer"), rt.new_object("Layer"), rt.new_object("Layer"));
        // The anchor as mods name it: NeoForge's `VanillaGuiLayers.HOTBAR`.
        let hotbar = get_static(&mut rt, "net/neoforged/neoforge/client/gui/VanillaGuiLayers", "HOTBAR").unwrap();
        let id_a = rt.string("create:goggle_info");
        let id_b = rt.string("create:value_settings");
        // Registered after the hotbar in turn: the later one goes right after it, before the earlier.
        layers_event(&mut rt, "registerAbove", &[V::NULL, hotbar, id_a, a]);
        layers_event(&mut rt, "registerAbove", &[V::NULL, hotbar, id_b, b]);
        let air = rt.string("minecraft:air_level");
        let id_c = rt.string("create:remaining_air");
        layers_event(&mut rt, "registerAbove", &[V::NULL, air, id_c, c]);
        assert_eq!(layers_at(&mut rt, "minecraft:hotbar", false), vec![(b, false), (a, false)]);
        assert_eq!(layers_at(&mut rt, "minecraft:player_health", false), vec![(c, false)]);
        assert!(layers_at(&mut rt, "minecraft:hotbar", true).is_empty());
        let chat = rt.string("minecraft:chat");
        let d = rt.new_object("Layer");
        layers_event(&mut rt, "replaceLayer", &[V::NULL, chat, d]);
        assert_eq!(layers_at(&mut rt, "minecraft:chat", true), vec![(d, true)]);
    }

    #[test]
    fn graphics_draw_through_their_pose() {
        let mut rt = Rt::new();
        let mut host = MemoryHost { gui: [320, 240], ..MemoryHost::default() };
        let g = graphics(&mut rt);
        rt.interned.insert(DRAWING.into(), V::I(1));
        rt.with_host(&mut host, |rt| {
            let pose = rt.get_field(g, "$pose");
            crate::api::call(rt, "com/mojang/blaze3d/vertex/PoseStack.translate.(FFF)V", &[pose, V::F(10.0), V::F(20.0), V::F(0.0)]);
            call(rt, "net/minecraft/client/gui/GuiGraphics.fill.(IIIII)V", &[g, V::I(0), V::I(0), V::I(4), V::I(2), V::I(0x8000_00FF_u32 as i32)]);
            let font = font(rt);
            let s = rt.string("Hi");
            let end = call(rt, "net/minecraft/client/gui/GuiGraphics.drawString.(Lnet/minecraft/client/gui/Font;Ljava/lang/String;III)I", &[g, font, s, V::I(1), V::I(2), V::I(0x00FF_FFFF)]);
            // Two characters 6 wide from x 1, and the shadow's pixel.
            assert_eq!(end, Some(V::I(14)));
            assert_eq!(call(rt, "net/minecraft/client/gui/GuiGraphics.guiWidth.()I", &[g]), Some(V::I(320)));
        });
        assert_eq!(
            host.drawn_gui,
            vec![
                GuiDraw::Fill([[10.0, 20.0], [10.0, 22.0], [14.0, 22.0], [14.0, 20.0]], 0x8000_00FF),
                GuiDraw::Text(vec![run("Hi", None, false)], [11.0, 22.0], 1.0, 0xFFFF_FFFF, true),
            ]
        );
    }

    #[test]
    fn a_tooltip_is_a_box_beside_the_mouse_with_its_lines() {
        let mut rt = Rt::new();
        let mut host = MemoryHost { gui: [320, 240], ..MemoryHost::default() };
        let g = graphics(&mut rt);
        rt.interned.insert(DRAWING.into(), V::I(1));
        rt.with_host(&mut host, |rt| {
            let font = font(rt);
            let first = literal(rt, "Kinetic Stats", "");
            let second = literal(rt, "Speed", "GRAY,");
            let lines = crate::jutil::new_list(rt, vec![first, second]);
            call(rt, "net/minecraft/client/gui/GuiGraphics.renderComponentTooltip.(Lnet/minecraft/client/gui/Font;Ljava/util/List;II)V", &[g, font, lines, V::I(100), V::I(50)]);
        });
        let texts: Vec<_> = host.drawn_gui.iter().filter_map(|d| if let GuiDraw::Text(r, at, ..) = d { Some((r[0].text.clone(), *at)) } else { None }).collect();
        // The box at the mouse plus (12, -12); the first line, a 2-pixel gap, then 10 per line.
        assert_eq!(texts, vec![("Kinetic Stats".to_owned(), [112.0, 38.0]), ("Speed".to_owned(), [112.0, 50.0])]);
        // The background's top line spans the text (78 wide) and 3 pixels each side.
        assert!(host.drawn_gui.iter().any(|d| matches!(d, GuiDraw::Fill(q, c) if *c == TOOLTIP_BACKGROUND as u32 && q[0] == [109.0, 34.0] && q[2] == [193.0, 35.0])));
    }
}
