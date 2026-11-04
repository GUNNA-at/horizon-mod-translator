//! Vanilla's text field, `EditBox`, natively, as the mods' 1.21.1 API has it: the value with its
//! cursor and selection (positions in UTF-16 units, as Java's), the filter, the responder (told of
//! every change and cursor move, as 1.21.1's `moveCursorTo` tells it), the formatter, typing,
//! deleting by character and by word, the edit keys, the clipboard, a click placing the cursor, and
//! drawing: the sprite, the text scrolled to the cursor, the blinking cursor, the suggestion, the
//! hint and the selection. Whatever else a field does is `AbstractWidget`'s ([`crate::widgets`]).
use crate::{Rt, V};

pub(crate) const EDIT_BOX: &str = "net/minecraft/client/gui/components/EditBox";

const FONT_SUBSTR: &str = "net/minecraft/client/gui/Font.plainSubstrByWidth.(Ljava/lang/String;I)Ljava/lang/String;";
const FONT_SUBSTR_TAIL: &str = "net/minecraft/client/gui/Font.plainSubstrByWidth.(Ljava/lang/String;IZ)Ljava/lang/String;";
const FONT_WIDTH: &str = "net/minecraft/client/gui/Font.width.(Ljava/lang/String;)I";
const FORWARD: &str = "net/minecraft/util/FormattedCharSequence.forward.(Ljava/lang/String;Lnet/minecraft/network/chat/Style;)Lnet/minecraft/util/FormattedCharSequence;";
const STYLE_EMPTY: &str = "net/minecraft/network/chat/Style.EMPTY";
const FORMAT: &str = "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const ACCEPT: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const BOX_INT: &str = "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;";
const MILLIS: &str = "net/minecraft/Util.getMillis.()J";
const G_BLIT_SPRITE: &str = "net/minecraft/client/gui/GuiGraphics.blitSprite.(Lnet/minecraft/resources/ResourceLocation;IIII)V";
const G_SEQ: &str = "net/minecraft/client/gui/GuiGraphics.drawString.(Lnet/minecraft/client/gui/Font;Lnet/minecraft/util/FormattedCharSequence;III)I";
const G_STRING: &str = "net/minecraft/client/gui/GuiGraphics.drawString.(Lnet/minecraft/client/gui/Font;Ljava/lang/String;III)I";
const G_TEXT: &str = "net/minecraft/client/gui/GuiGraphics.drawString.(Lnet/minecraft/client/gui/Font;Lnet/minecraft/network/chat/Component;III)I";
const G_FILL: &str = "net/minecraft/client/gui/GuiGraphics.fill.(IIIII)V";
const IS_SELECT_ALL: &str = "net/minecraft/client/gui/screens/Screen.isSelectAll.(I)Z";
const IS_COPY: &str = "net/minecraft/client/gui/screens/Screen.isCopy.(I)Z";
const IS_PASTE: &str = "net/minecraft/client/gui/screens/Screen.isPaste.(I)Z";
const IS_CUT: &str = "net/minecraft/client/gui/screens/Screen.isCut.(I)Z";

/// `EditBox`'s defaults: text colour 14737632, greyed 7368816, the suggestion's -8355712, the
/// insert cursor's -3092272, the selection's -16776961 (drawn half see-through here: vanilla
/// inverts what is under it).
const TEXT_COLOR: i32 = 14_737_632;
const TEXT_COLOR_UNEDITABLE: i32 = 7_368_816;
const SUGGESTION_COLOR: i32 = -8_355_712;
const CURSOR_COLOR: i32 = -3_092_272;
const HIGHLIGHT_COLOR: i32 = 0x8000_00FF_u32 as i32;
const LINE_HEIGHT: i32 = 9;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

fn truthy(v: V) -> bool {
    matches!(v, V::I(n) if n != 0)
}

fn int(rt: &Rt, o: V, f: &'static str) -> i32 {
    rt.get_field(o, f).i()
}

fn flag(rt: &Rt, o: V, f: &'static str) -> bool {
    truthy(rt.get_field(o, f))
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn text(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn value(rt: &Rt, this: V) -> Vec<u16> {
    units(rt.str_of(rt.get_field(this, "$value")).unwrap_or(""))
}

fn put_value(rt: &mut Rt, this: V, v: &[u16]) {
    let s = rt.string(&text(v));
    rt.put_field(this, "$value", s);
}

fn length(rt: &Rt, this: V) -> i32 {
    value(rt, this).len() as i32
}

/// `EditBox`'s constructor: the widget, its font and the defaults; a box it replaces gives its value.
fn init(rt: &mut Rt, this: V, font: V, rect: [i32; 4], old: V, message: V) {
    crate::widgets::init_widget(rt, this, rect, message);
    rt.put_field(this, "$font", font);
    let empty = rt.string("");
    rt.put_field(this, "$value", empty);
    for (f, v) in [
        ("$maxLength", 32),
        ("$bordered", 1),
        ("$canLoseFocus", 1),
        ("$isEditable", 1),
        ("$displayPos", 0),
        ("$cursorPos", 0),
        ("$highlightPos", 0),
        ("$textColor", TEXT_COLOR),
        ("$textColorUneditable", TEXT_COLOR_UNEDITABLE),
    ] {
        rt.put_field(this, f, V::I(v));
    }
    let now = crate::api::call(rt, MILLIS, &[]);
    rt.put_field(this, "$focusedTime", now);
    if old.r() != 0 && rt.instance_of(old, EDIT_BOX) {
        let v = value(rt, old);
        set_value(rt, this, &v);
    }
}

/// The filter lets `v` through (none set: everything does).
fn allowed(rt: &mut Rt, this: V, v: &[u16]) -> bool {
    let filter = rt.get_field(this, "$filter");
    if filter.r() == 0 {
        return true;
    }
    let s = rt.string(&text(v));
    truthy(rt.call_functional(TEST, filter, &[s]))
}

/// `onValueChange`: the responder hears the value.
fn changed(rt: &mut Rt, this: V, v: &[u16]) {
    let responder = rt.get_field(this, "$responder");
    if responder.r() != 0 {
        let s = rt.string(&text(v));
        rt.call_functional(ACCEPT, responder, &[s]);
    }
}

/// `setValue`: cut to the maximum length, the cursor at its end with nothing selected; the
/// responder hears the value as given.
fn set_value(rt: &mut Rt, this: V, v: &[u16]) {
    if !allowed(rt, this, v) {
        return;
    }
    let max = int(rt, this, "$maxLength").max(0) as usize;
    put_value(rt, this, &v[..v.len().min(max)]);
    move_cursor_to_end(rt, this, false);
    let c = int(rt, this, "$cursorPos");
    set_highlight_pos(rt, this, c);
    changed(rt, this, v);
}

/// `StringUtil.filterText`: no section signs, control characters or line breaks.
fn filter_text(input: &[u16]) -> Vec<u16> {
    input.iter().copied().filter(|&c| c != 167 && c >= 32 && c != 127).collect()
}

fn is_high_surrogate(c: u16) -> bool {
    (0xD800..0xDC00).contains(&c)
}

fn is_low_surrogate(c: u16) -> bool {
    (0xDC00..0xE000).contains(&c)
}

/// `insertText`: replaces the selection with `input`, as much of it as the maximum length leaves
/// room for (a surrogate pair isn't split), when the filter lets the result through.
fn insert_text(rt: &mut Rt, this: V, input: &[u16]) {
    let (c, h) = (int(rt, this, "$cursorPos"), int(rt, this, "$highlightPos"));
    let (start, end) = (c.min(h) as usize, c.max(h) as usize);
    let v = value(rt, this);
    let (start, end) = (start.min(v.len()), end.min(v.len()));
    let mut room = int(rt, this, "$maxLength") - v.len() as i32 - (start as i32 - end as i32);
    if room <= 0 {
        return;
    }
    let mut add = filter_text(input);
    if (room as usize) < add.len() {
        if is_high_surrogate(add[room as usize - 1]) {
            room -= 1;
        }
        add.truncate(room as usize);
    }
    let mut next = v[..start].to_vec();
    next.extend_from_slice(&add);
    next.extend_from_slice(&v[end..]);
    if !allowed(rt, this, &next) {
        return;
    }
    put_value(rt, this, &next);
    set_cursor_position(rt, this, (start + add.len()) as i32);
    let c = int(rt, this, "$cursorPos");
    set_highlight_pos(rt, this, c);
    changed(rt, this, &next);
}

/// `deleteText(count)`: by words with the edit modifier held, else by characters.
fn delete_text(rt: &mut Rt, this: V, count: i32) {
    if control_down(rt) {
        delete_words(rt, this, count);
    } else {
        delete_chars(rt, this, count);
    }
}

fn delete_words(rt: &mut Rt, this: V, count: i32) {
    if length(rt, this) == 0 {
        return;
    }
    if int(rt, this, "$highlightPos") != int(rt, this, "$cursorPos") {
        insert_text(rt, this, &[]);
    } else {
        let to = word_position(rt, this, count, int(rt, this, "$cursorPos"), true);
        delete_chars_to_pos(rt, this, to);
    }
}

fn delete_chars(rt: &mut Rt, this: V, count: i32) {
    let to = cursor_pos_by(rt, this, count);
    delete_chars_to_pos(rt, this, to);
}

/// `deleteCharsToPos`: the selection goes, or what lies between the cursor and `pos`.
fn delete_chars_to_pos(rt: &mut Rt, this: V, pos: i32) {
    let v = value(rt, this);
    if v.is_empty() {
        return;
    }
    if int(rt, this, "$highlightPos") != int(rt, this, "$cursorPos") {
        insert_text(rt, this, &[]);
        return;
    }
    let c = int(rt, this, "$cursorPos");
    let (start, end) = (pos.min(c).clamp(0, v.len() as i32) as usize, pos.max(c).clamp(0, v.len() as i32) as usize);
    if start == end {
        return;
    }
    let mut next = v[..start].to_vec();
    next.extend_from_slice(&v[end..]);
    if allowed(rt, this, &next) {
        put_value(rt, this, &next);
        move_cursor_to(rt, this, start as i32, false);
    }
}

/// `getWordPosition(dir, from, stripSpaces)`: `dir` words on (back when negative), skipping the
/// spaces next to them.
fn word_position(rt: &Rt, this: V, dir: i32, from: i32, strip: bool) -> i32 {
    let v = value(rt, this);
    let n = v.len() as i32;
    let mut at = from.clamp(0, n);
    for _ in 0..dir.unsigned_abs() {
        if dir >= 0 {
            match v[at as usize..].iter().position(|&c| c == 32) {
                None => at = n,
                Some(i) => {
                    at += i as i32;
                    while strip && at < n && v[at as usize] == 32 {
                        at += 1;
                    }
                }
            }
        } else {
            while strip && at > 0 && v[at as usize - 1] == 32 {
                at -= 1;
            }
            while at > 0 && v[at as usize - 1] != 32 {
                at -= 1;
            }
        }
    }
    at
}

/// `Util.offsetByCodepoints(value, cursorPos, dir)`: `dir` code points on from the cursor.
fn cursor_pos_by(rt: &Rt, this: V, dir: i32) -> i32 {
    let v = value(rt, this);
    let n = v.len();
    let mut at = (int(rt, this, "$cursorPos").max(0) as usize).min(n);
    if dir >= 0 {
        for _ in 0..dir {
            if at >= n {
                break;
            }
            at += 1;
            if is_high_surrogate(v[at - 1]) && at < n && is_low_surrogate(v[at]) {
                at += 1;
            }
        }
    } else {
        for _ in 0..dir.unsigned_abs() {
            if at == 0 {
                break;
            }
            at -= 1;
            if is_low_surrogate(v[at]) && at > 0 && is_high_surrogate(v[at - 1]) {
                at -= 1;
            }
        }
    }
    at as i32
}

/// `moveCursorTo(pos, extend)`: the cursor there, the selection kept only when extending; the
/// responder hears the value (1.21.1 tells it of every move).
fn move_cursor_to(rt: &mut Rt, this: V, pos: i32, extend: bool) {
    set_cursor_position(rt, this, pos);
    if !extend {
        let c = int(rt, this, "$cursorPos");
        set_highlight_pos(rt, this, c);
    }
    let v = value(rt, this);
    changed(rt, this, &v);
}

fn move_cursor_to_end(rt: &mut Rt, this: V, extend: bool) {
    let n = length(rt, this);
    move_cursor_to(rt, this, n, extend);
}

fn set_cursor_position(rt: &mut Rt, this: V, pos: i32) {
    let c = pos.clamp(0, length(rt, this));
    rt.put_field(this, "$cursorPos", V::I(c));
    scroll_to(rt, this, c);
}

fn set_highlight_pos(rt: &mut Rt, this: V, pos: i32) {
    let h = pos.clamp(0, length(rt, this));
    rt.put_field(this, "$highlightPos", V::I(h));
    scroll_to(rt, this, h);
}

fn inner_width(rt: &Rt, this: V) -> i32 {
    let w = int(rt, this, "width");
    if flag(rt, this, "$bordered") { w - 8 } else { w }
}

fn font(rt: &Rt, this: V) -> V {
    rt.get_field(this, "$font")
}

/// `font.plainSubstrByWidth(s, width[, tail])`, in UTF-16 units.
fn fitting(rt: &mut Rt, font: V, s: &[u16], width: i32, tail: bool) -> Vec<u16> {
    let js = rt.string(&text(s));
    let out = if tail {
        crate::api::call(rt, FONT_SUBSTR_TAIL, &[font, js, V::I(width), V::I(1)])
    } else {
        crate::api::call(rt, FONT_SUBSTR, &[font, js, V::I(width)])
    };
    units(rt.str_of(out).unwrap_or(""))
}

fn width(rt: &mut Rt, font: V, s: &[u16]) -> i32 {
    let js = rt.string(&text(s));
    crate::api::call(rt, FONT_WIDTH, &[font, js]).i()
}

/// `scrollTo(pos)`: the first shown position moves so that `pos` is in view.
fn scroll_to(rt: &mut Rt, this: V, pos: i32) {
    let f = font(rt, this);
    if f.r() == 0 {
        return;
    }
    let v = value(rt, this);
    let n = v.len() as i32;
    let mut display = int(rt, this, "$displayPos").min(n).max(0);
    let inner = inner_width(rt, this);
    let shown = fitting(rt, f, &v[display as usize..], inner, false);
    let last = shown.len() as i32 + display;
    if pos == display {
        display -= fitting(rt, f, &v, inner, true).len() as i32;
    }
    if pos > last {
        display += pos - last;
    } else if pos <= display {
        display -= display - pos;
    }
    rt.put_field(this, "$displayPos", V::I(display.clamp(0, n)));
}

fn control_down(rt: &Rt) -> bool {
    crate::screens::held_either(rt, if cfg!(target_os = "macos") { "win" } else { "control" })
}

fn shift_down(rt: &Rt) -> bool {
    crate::screens::held_either(rt, "shift")
}

fn active(rt: &Rt, this: V) -> bool {
    flag(rt, this, "active") && flag(rt, this, "visible")
}

fn can_consume_input(rt: &Rt, this: V) -> bool {
    flag(rt, this, "visible") && flag(rt, this, "focused") && flag(rt, this, "$isEditable")
}

fn selected(rt: &Rt, this: V) -> Vec<u16> {
    let v = value(rt, this);
    let (c, h) = (int(rt, this, "$cursorPos"), int(rt, this, "$highlightPos"));
    let (start, end) = ((c.min(h).max(0) as usize).min(v.len()), (c.max(h).max(0) as usize).min(v.len()));
    v[start..end].to_vec()
}

/// `keyPressed(key, scan, modifiers)`: the edit keys of a focused, active field.
fn key_pressed(rt: &mut Rt, this: V, key: i32) -> bool {
    if !(active(rt, this) && flag(rt, this, "focused")) {
        return false;
    }
    let editable = flag(rt, this, "$isEditable");
    let shift = shift_down(rt);
    match key {
        // Backspace and delete.
        259 | 261 => {
            if editable {
                delete_text(rt, this, if key == 259 { -1 } else { 1 });
            }
            true
        }
        // Right and left: a word with the edit modifier, else a character.
        262 | 263 => {
            let dir = if key == 262 { 1 } else { -1 };
            let to = if control_down(rt) {
                word_position(rt, this, dir, int(rt, this, "$cursorPos"), true)
            } else {
                cursor_pos_by(rt, this, dir)
            };
            move_cursor_to(rt, this, to, shift);
            true
        }
        // Home and end.
        268 => {
            move_cursor_to(rt, this, 0, shift);
            true
        }
        269 => {
            move_cursor_to_end(rt, this, shift);
            true
        }
        _ => {
            let is = |rt: &mut Rt, k: &'static str| truthy(crate::api::call(rt, k, &[V::I(key)]));
            if is(rt, IS_SELECT_ALL) {
                move_cursor_to_end(rt, this, false);
                set_highlight_pos(rt, this, 0);
            } else if is(rt, IS_COPY) {
                let s = text(&selected(rt, this));
                crate::screens::set_clipboard(rt, &s);
            } else if is(rt, IS_PASTE) {
                if editable {
                    let pasted = crate::screens::clipboard(rt);
                    insert_text(rt, this, &units(&pasted));
                }
            } else if is(rt, IS_CUT) {
                let s = text(&selected(rt, this));
                crate::screens::set_clipboard(rt, &s);
                if editable {
                    insert_text(rt, this, &[]);
                }
            } else {
                return false;
            }
            true
        }
    }
}

/// `charTyped(c, modifiers)`: a character allowed in chat goes in.
fn char_typed(rt: &mut Rt, this: V, c: i32) -> bool {
    if !can_consume_input(rt, this) {
        return false;
    }
    let Ok(c) = u16::try_from(c) else { return false };
    if c == 167 || c < 32 || c == 127 {
        return false;
    }
    insert_text(rt, this, &[c]);
    true
}

/// `onClick(x, y)`: the cursor where the pointer is in the shown text (shift extends the
/// selection).
fn on_click(rt: &mut Rt, this: V, x: f64) {
    let mut at = x.floor() as i32 - int(rt, this, "x");
    if flag(rt, this, "$bordered") {
        at -= 4;
    }
    let f = font(rt, this);
    let v = value(rt, this);
    let display = (int(rt, this, "$displayPos").max(0) as usize).min(v.len());
    let inner = inner_width(rt, this);
    let shown = fitting(rt, f, &v[display..], inner, false);
    let to = fitting(rt, f, &shown, at, false).len() + display;
    let shift = shift_down(rt);
    move_cursor_to(rt, this, to as i32, shift);
}

/// The formatter's sequence for `s` starting at `offset` (vanilla's: plain, unstyled).
fn formatted(rt: &mut Rt, this: V, s: &[u16], offset: i32) -> V {
    let js = rt.string(&text(s));
    let formatter = rt.get_field(this, "$formatter");
    if formatter.r() != 0 {
        let at = crate::api::call(rt, BOX_INT, &[V::I(offset)]);
        let out = rt.call_functional(FORMAT, formatter, &[js, at]);
        if out.r() != 0 {
            return out;
        }
    }
    let style = crate::api::get_static(rt, STYLE_EMPTY);
    crate::api::call(rt, FORWARD, &[js, style])
}

/// `renderWidget`: 1.21.1's, line by line.
fn render_widget(rt: &mut Rt, this: V, g: V) {
    if !flag(rt, this, "visible") {
        return;
    }
    let (x, y, w, h) = (int(rt, this, "x"), int(rt, this, "y"), int(rt, this, "width"), int(rt, this, "height"));
    let bordered = flag(rt, this, "$bordered");
    let focused = flag(rt, this, "focused");
    if bordered {
        let sprite = if active(rt, this) && focused { "minecraft:widget/text_field_highlighted" } else { "minecraft:widget/text_field" };
        let s = rt.string(sprite);
        crate::api::call(rt, G_BLIT_SPRITE, &[g, s, V::I(x), V::I(y), V::I(w), V::I(h)]);
    }
    let color = if flag(rt, this, "$isEditable") { int(rt, this, "$textColor") } else { int(rt, this, "$textColorUneditable") };
    let f = font(rt, this);
    let v = value(rt, this);
    let display = (int(rt, this, "$displayPos").max(0) as usize).min(v.len());
    let cursor = int(rt, this, "$cursorPos");
    let rel_cursor = cursor - display as i32;
    let inner = inner_width(rt, this);
    let shown = fitting(rt, f, &v[display..], inner, false);
    let on_screen = rel_cursor >= 0 && rel_cursor <= shown.len() as i32;
    let now = crate::api::call(rt, MILLIS, &[]).j();
    let since = now - rt.get_field(this, "$focusedTime").j();
    let blink_on = focused && since / 300 % 2 == 0 && on_screen;
    let text_x = if bordered { x + 4 } else { x };
    let text_y = if bordered { y + (h - 8) / 2 } else { y };
    let mut draw_x = text_x;
    let rel_highlight = (int(rt, this, "$highlightPos") - display as i32).clamp(0, shown.len() as i32);
    if !shown.is_empty() {
        let half = if on_screen { &shown[..rel_cursor as usize] } else { &shown[..] };
        let seq = formatted(rt, this, half, display as i32);
        draw_x = crate::api::call(rt, G_SEQ, &[g, f, seq, V::I(text_x), V::I(text_y), V::I(color)]).i();
    }
    let max = int(rt, this, "$maxLength");
    let insert = cursor < v.len() as i32 || v.len() as i32 >= max;
    let mut cursor_x = draw_x;
    if !on_screen {
        cursor_x = if rel_cursor > 0 { text_x + w } else { text_x };
    } else if insert {
        cursor_x = draw_x - 1;
        draw_x -= 1;
    }
    if !shown.is_empty() && on_screen && (rel_cursor as usize) < shown.len() {
        let seq = formatted(rt, this, &shown[rel_cursor as usize..], cursor);
        crate::api::call(rt, G_SEQ, &[g, f, seq, V::I(draw_x), V::I(text_y), V::I(color)]);
    }
    let hint = rt.get_field(this, "$hint");
    if hint.r() != 0 && shown.is_empty() && !focused {
        crate::api::call(rt, G_TEXT, &[g, f, hint, V::I(draw_x), V::I(text_y), V::I(color)]);
    }
    let suggestion = rt.get_field(this, "$suggestion");
    if !insert && suggestion.r() != 0 {
        crate::api::call(rt, G_STRING, &[g, f, suggestion, V::I(cursor_x - 1), V::I(text_y), V::I(SUGGESTION_COLOR)]);
    }
    if blink_on {
        if insert {
            crate::api::call(rt, G_FILL, &[g, V::I(cursor_x), V::I(text_y - 1), V::I(cursor_x + 1), V::I(text_y + 1 + LINE_HEIGHT), V::I(CURSOR_COLOR)]);
        } else {
            let under = rt.string("_");
            crate::api::call(rt, G_STRING, &[g, f, under, V::I(cursor_x), V::I(text_y), V::I(color)]);
        }
    }
    if rel_highlight != rel_cursor {
        let highlight_x = text_x + width(rt, f, &shown[..rel_highlight as usize]);
        // `renderHighlight`: ordered corners, clipped to the field's right edge.
        let (mut x0, mut x1) = (cursor_x.min(highlight_x - 1), cursor_x.max(highlight_x - 1));
        x0 = x0.min(x + w);
        x1 = x1.min(x + w);
        crate::api::call(rt, G_FILL, &[g, V::I(x0), V::I(text_y - 1), V::I(x1), V::I(text_y + 1 + LINE_HEIGHT), V::I(HIGHLIGHT_COLOR)]);
    }
}

/// `EditBox`'s own methods on a field (a mod's subclass too); None for what is `AbstractWidget`'s.
pub(crate) fn own(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match (name, desc) {
        ("<init>", "(Lnet/minecraft/client/gui/Font;IIIILnet/minecraft/network/chat/Component;)V") => {
            init(rt, this, a(1), [a(2).i(), a(3).i(), a(4).i(), a(5).i()], V::NULL, a(6));
            V::Void
        }
        ("<init>", "(Lnet/minecraft/client/gui/Font;IIIILnet/minecraft/client/gui/components/EditBox;Lnet/minecraft/network/chat/Component;)V") => {
            init(rt, this, a(1), [a(2).i(), a(3).i(), a(4).i(), a(5).i()], a(6), a(7));
            V::Void
        }
        ("<init>", "(Lnet/minecraft/client/gui/Font;IILnet/minecraft/network/chat/Component;)V") => {
            init(rt, this, a(1), [0, 0, a(2).i(), a(3).i()], V::NULL, a(4));
            V::Void
        }
        ("setResponder", _) => {
            rt.put_field(this, "$responder", a(1));
            V::Void
        }
        ("setFormatter", _) => {
            rt.put_field(this, "$formatter", a(1));
            V::Void
        }
        ("setFilter", _) => {
            rt.put_field(this, "$filter", a(1));
            V::Void
        }
        ("setValue", _) => {
            let v = units(rt.str_of(a(1)).unwrap_or(""));
            set_value(rt, this, &v);
            V::Void
        }
        ("getValue", _) => {
            let v = value(rt, this);
            rt.string(&text(&v))
        }
        ("getHighlighted", _) => {
            let s = selected(rt, this);
            rt.string(&text(&s))
        }
        ("insertText", _) => {
            let v = units(rt.str_of(a(1)).unwrap_or(""));
            insert_text(rt, this, &v);
            V::Void
        }
        ("deleteWords", _) => {
            delete_words(rt, this, a(1).i());
            V::Void
        }
        ("deleteChars", _) => {
            delete_chars(rt, this, a(1).i());
            V::Void
        }
        ("deleteCharsToPos", _) => {
            delete_chars_to_pos(rt, this, a(1).i());
            V::Void
        }
        ("getWordPosition", "(I)I") => V::I(word_position(rt, this, a(1).i(), int(rt, this, "$cursorPos"), true)),
        ("getWordPosition", "(II)I") => V::I(word_position(rt, this, a(1).i(), a(2).i(), true)),
        ("getWordPosition", "(IIZ)I") => V::I(word_position(rt, this, a(1).i(), a(2).i(), truthy(a(3)))),
        ("moveCursor", _) => {
            let to = cursor_pos_by(rt, this, a(1).i());
            move_cursor_to(rt, this, to, truthy(a(2)));
            V::Void
        }
        ("moveCursorTo", _) => {
            move_cursor_to(rt, this, a(1).i(), truthy(a(2)));
            V::Void
        }
        ("setCursorPosition", _) => {
            set_cursor_position(rt, this, a(1).i());
            V::Void
        }
        ("moveCursorToStart", _) => {
            move_cursor_to(rt, this, 0, truthy(a(1)));
            V::Void
        }
        ("moveCursorToEnd", _) => {
            move_cursor_to_end(rt, this, truthy(a(1)));
            V::Void
        }
        ("keyPressed", "(III)Z") => V::I(key_pressed(rt, this, a(1).i()) as i32),
        ("canConsumeInput", _) => V::I(can_consume_input(rt, this) as i32),
        ("charTyped", "(CI)Z") => V::I(char_typed(rt, this, a(1).i()) as i32),
        ("onClick", "(DD)V") => {
            on_click(rt, this, a(1).d());
            V::Void
        }
        // A field makes no click sound.
        ("playDownSound", _) => V::Void,
        ("renderWidget", "(Lnet/minecraft/client/gui/GuiGraphics;IIF)V") => {
            render_widget(rt, this, a(1));
            V::Void
        }
        ("setMaxLength", _) => {
            let max = a(1).i();
            rt.put_field(this, "$maxLength", V::I(max));
            let v = value(rt, this);
            if v.len() as i32 > max {
                let cut = v[..max.max(0) as usize].to_vec();
                put_value(rt, this, &cut);
                changed(rt, this, &cut);
            }
            V::Void
        }
        ("getMaxLength", _) => rt.get_field(this, "$maxLength"),
        ("getCursorPosition", _) => rt.get_field(this, "$cursorPos"),
        ("isBordered", _) => V::I(flag(rt, this, "$bordered") as i32),
        ("setBordered", _) => {
            rt.put_field(this, "$bordered", V::I(truthy(a(1)) as i32));
            V::Void
        }
        ("setTextColor", _) => {
            rt.put_field(this, "$textColor", a(1));
            V::Void
        }
        ("setTextColorUneditable", _) => {
            rt.put_field(this, "$textColorUneditable", a(1));
            V::Void
        }
        ("setFocused", "(Z)V") => {
            if flag(rt, this, "$canLoseFocus") || truthy(a(1)) {
                rt.put_field(this, "focused", V::I(truthy(a(1)) as i32));
                if truthy(a(1)) {
                    let now = crate::api::call(rt, MILLIS, &[]);
                    rt.put_field(this, "$focusedTime", now);
                }
            }
            V::Void
        }
        ("isEditable", _) => V::I(flag(rt, this, "$isEditable") as i32),
        ("setEditable", _) => {
            rt.put_field(this, "$isEditable", V::I(truthy(a(1)) as i32));
            V::Void
        }
        ("getInnerWidth", _) => V::I(inner_width(rt, this)),
        ("setHighlightPos", _) => {
            set_highlight_pos(rt, this, a(1).i());
            V::Void
        }
        ("setCanLoseFocus", _) => {
            rt.put_field(this, "$canLoseFocus", V::I(truthy(a(1)) as i32));
            V::Void
        }
        ("isVisible", _) => V::I(flag(rt, this, "visible") as i32),
        ("setVisible", _) => {
            rt.put_field(this, "visible", V::I(truthy(a(1)) as i32));
            V::Void
        }
        ("setSuggestion", _) => {
            rt.put_field(this, "$suggestion", a(1));
            V::Void
        }
        ("getScreenX", _) => {
            let v = value(rt, this);
            let i = a(1).i();
            let x = int(rt, this, "x");
            if i > v.len() as i32 {
                V::I(x)
            } else {
                let f = font(rt, this);
                V::I(x + width(rt, f, &v[..i.max(0) as usize]))
            }
        }
        ("setHint", _) => {
            rt.put_field(this, "$hint", a(1));
            V::Void
        }
        ("updateWidgetNarration", _) => V::Void,
        _ => return None,
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{GuiDraw, MemoryHost};
    use std::cell::RefCell;

    const FONT: &str = "net/minecraft/client/gui/Font";
    const LISTENER: &str = "net/minecraft/client/gui/components/events/GuiEventListener";
    const RENDER: &str = "net/minecraft/client/gui/components/Renderable.render.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V";

    fn key(rest: &str) -> &'static str {
        Box::leak(format!("{EDIT_BOX}.{rest}").into_boxed_str())
    }

    /// A screen's call on its child (`GuiEventListener`'s key).
    fn listener(rest: &str) -> &'static str {
        Box::leak(format!("{LISTENER}.{rest}").into_boxed_str())
    }

    /// `new EditBox(font, 10, 20, width, 20, message)`.
    fn field(rt: &mut Rt, width: i32) -> V {
        let font = rt.new_object(FONT);
        let b = rt.new_object(EDIT_BOX);
        let message = rt.string("");
        crate::api::call(rt, key("<init>.(Lnet/minecraft/client/gui/Font;IIIILnet/minecraft/network/chat/Component;)V"), &[b, font, V::I(10), V::I(20), V::I(width), V::I(20), message]);
        b
    }

    fn focus(rt: &mut Rt, b: V) {
        crate::api::call(rt, listener("setFocused.(Z)V"), &[b, V::I(1)]);
    }

    fn get(rt: &mut Rt, b: V) -> String {
        let v = crate::api::call(rt, key("getValue.()Ljava/lang/String;"), &[b]);
        rt.str_of(v).unwrap_or("").to_owned()
    }

    fn highlighted(rt: &mut Rt, b: V) -> String {
        let v = crate::api::call(rt, key("getHighlighted.()Ljava/lang/String;"), &[b]);
        rt.str_of(v).unwrap_or("").to_owned()
    }

    fn cursor(rt: &mut Rt, b: V) -> i32 {
        crate::api::call(rt, key("getCursorPosition.()I"), &[b]).i()
    }

    fn set(rt: &mut Rt, b: V, s: &str) {
        let js = rt.string(s);
        crate::api::call(rt, key("setValue.(Ljava/lang/String;)V"), &[b, js]);
    }

    /// Each character as the screen passes it on; whether the field took it.
    fn typed(rt: &mut Rt, b: V, s: &str) -> Vec<bool> {
        s.encode_utf16().map(|c| truthy(crate::api::call(rt, listener("charTyped.(CI)Z"), &[b, V::I(i32::from(c)), V::I(0)]))).collect()
    }

    fn press(rt: &mut Rt, b: V, k: i32) -> bool {
        truthy(crate::api::call(rt, listener("keyPressed.(III)Z"), &[b, V::I(k), V::I(0), V::I(0)]))
    }

    fn click(rt: &mut Rt, b: V, x: f64, y: f64) -> bool {
        truthy(crate::api::call(rt, listener("mouseClicked.(DDI)Z"), &[b, V::D(x), V::D(y), V::I(0)]))
    }

    const BACKSPACE: i32 = 259;
    const DELETE: i32 = 261;
    const RIGHT: i32 = 262;
    const LEFT: i32 = 263;
    const HOME: i32 = 268;
    const END: i32 = 269;

    /// Typing and the edit keys as vanilla's field takes them: unfocused it takes nothing; a
    /// character goes in at the cursor (not `§` or a control character) up to the maximum length;
    /// backspace and delete take one, the arrows move, home and end jump, shift extends a
    /// selection, what is typed replaces the selection, a field that isn't editable takes no
    /// character, and `setValue` cuts to the maximum length with the cursor at the end.
    #[test]
    fn a_field_takes_typing_and_the_edit_keys_as_vanilla_does() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost { client: true, ..Default::default() };
        let b = rt.with_host(&mut host, |rt| {
            let b = field(rt, 100);
            assert_eq!(typed(rt, b, "ab"), [false, false], "an unfocused field takes nothing");
            assert_eq!(get(rt, b), "");
            focus(rt, b);
            assert_eq!(typed(rt, b, "he\u{a7}llo\u{7}"), [true, true, false, true, true, true, false]);
            assert_eq!((get(rt, b), cursor(rt, b)), ("hello".into(), 5));
            assert!(press(rt, b, LEFT) && press(rt, b, LEFT));
            typed(rt, b, "X");
            assert_eq!((get(rt, b), cursor(rt, b)), ("helXlo".into(), 4));
            assert!(press(rt, b, BACKSPACE));
            assert!(press(rt, b, DELETE));
            assert_eq!((get(rt, b), cursor(rt, b)), ("helo".into(), 3));
            press(rt, b, HOME);
            assert_eq!(cursor(rt, b), 0);
            press(rt, b, END);
            assert_eq!(cursor(rt, b), 4);
            crate::api::call(rt, key("setMaxLength.(I)V"), &[b, V::I(6)]);
            typed(rt, b, "wxyz");
            assert_eq!(get(rt, b), "helowx", "two more fit");
            crate::api::call(rt, key("setCursorPosition.(I)V"), &[b, V::I(4)]);
            crate::api::call(rt, key("setHighlightPos.(I)V"), &[b, V::I(1)]);
            assert_eq!(highlighted(rt, b), "elo");
            typed(rt, b, "Q");
            assert_eq!((get(rt, b), cursor(rt, b), highlighted(rt, b)), ("hQwx".into(), 2, String::new()));
            assert!(!press(rt, b, 290), "F1 is no edit key");
            b
        });
        // Shift held: the arrows extend the selection from where it started.
        host.keys_held = vec!["key.keyboard.left.shift".into()];
        rt.with_host(&mut host, |rt| {
            press(rt, b, RIGHT);
            press(rt, b, RIGHT);
            assert_eq!((cursor(rt, b), highlighted(rt, b)), (4, "wx".into()));
            typed(rt, b, "-");
            assert_eq!(get(rt, b), "hQ-");
        });
        host.keys_held.clear();
        rt.with_host(&mut host, |rt| {
            crate::api::call(rt, key("setEditable.(Z)V"), &[b, V::I(0)]);
            assert_eq!(typed(rt, b, "z"), [false]);
            assert!(press(rt, b, BACKSPACE), "the key is still the field's");
            assert_eq!(get(rt, b), "hQ-");
            crate::api::call(rt, key("setEditable.(Z)V"), &[b, V::I(1)]);
            set(rt, b, "abcdefghij");
            assert_eq!((get(rt, b), cursor(rt, b)), ("abcdef".into(), 6));
        });
    }

    /// With the edit modifier held (control; command on a Mac) the arrows and backspace go by
    /// words, and select-all, copy, paste and cut use the game's clipboard.
    #[test]
    fn the_edit_modifier_moves_by_words_and_uses_the_clipboard() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost { client: true, clipboard: "pasted".into(), ..Default::default() };
        host.keys_held = vec!["key.keyboard.left.control".into(), "key.keyboard.left.win".into()];
        rt.with_host(&mut host, |rt| {
            let b = field(rt, 200);
            focus(rt, b);
            set(rt, b, "one two  three");
            press(rt, b, LEFT);
            assert_eq!(cursor(rt, b), 9, "the start of \"three\"");
            press(rt, b, LEFT);
            assert_eq!(cursor(rt, b), 4, "the start of \"two\", past the spaces");
            press(rt, b, RIGHT);
            assert_eq!(cursor(rt, b), 9, "past \"two\" and the spaces after it");
            press(rt, b, BACKSPACE);
            assert_eq!((get(rt, b), cursor(rt, b)), ("one three".into(), 4));
            assert!(press(rt, b, 86), "paste");
            assert_eq!((get(rt, b), cursor(rt, b)), ("one pastedthree".into(), 10));
            assert!(press(rt, b, 65), "select all");
            assert_eq!(highlighted(rt, b), "one pastedthree");
            assert!(press(rt, b, 67), "copy");
            assert_eq!(crate::screens::clipboard(rt), "one pastedthree");
            crate::api::call(rt, key("moveCursorTo.(IZ)V"), &[b, V::I(3), V::I(0)]);
            crate::api::call(rt, key("setHighlightPos.(I)V"), &[b, V::I(0)]);
            assert!(press(rt, b, 88), "cut");
            assert_eq!(get(rt, b), " pastedthree");
        });
        assert_eq!(host.clipboard, "one");
    }

    thread_local! {
        static HEARD: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// The test's lambdas: 1 lets digits through (`Predicate`), 2 notes what it hears (`Consumer`).
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let s = rt.str_of(args[1]).unwrap_or("").to_owned();
        match id {
            1 => Some(V::I(s.chars().all(|c| c.is_ascii_digit()) as i32)),
            2 => {
                HEARD.with(|h| h.borrow_mut().push(s));
                Some(V::Void)
            }
            _ => None,
        }
    }

    /// The filter refuses a value it doesn't let through (typed or set), and the responder hears
    /// each change and each cursor move, as 1.21.1's field tells it (twice for `setValue`).
    #[test]
    fn the_filter_refuses_and_the_responder_hears() {
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(lambdas);
        HEARD.with(|h| h.borrow_mut().clear());
        let mut host = MemoryHost { client: true, ..Default::default() };
        rt.with_host(&mut host, |rt| {
            let b = field(rt, 100);
            focus(rt, b);
            let digits = rt.lambda_id("test:digits", 1, Vec::new());
            let heard = rt.lambda_id("test:heard", 2, Vec::new());
            crate::api::call(rt, key("setFilter.(Ljava/util/function/Predicate;)V"), &[b, digits]);
            crate::api::call(rt, key("setResponder.(Ljava/util/function/Consumer;)V"), &[b, heard]);
            typed(rt, b, "1a2");
            assert_eq!(get(rt, b), "12");
            set(rt, b, "abc");
            assert_eq!(get(rt, b), "12");
            set(rt, b, "345");
            press(rt, b, LEFT);
            assert_eq!(get(rt, b), "345");
        });
        assert_eq!(HEARD.with(|h| h.borrow().clone()), ["1", "12", "345", "345", "345"]);
    }

    fn texts(host: &MemoryHost) -> Vec<(String, [f32; 2])> {
        host.drawn_gui
            .iter()
            .filter_map(|d| match d {
                GuiDraw::Text(runs, at, ..) => Some((runs.iter().map(|r| r.text.as_str()).collect::<String>(), *at)),
                _ => None,
            })
            .filter(|(t, _)| !t.is_empty())
            .collect()
    }

    /// Fills as (x0, y0, x1, y1, argb).
    fn fills(host: &MemoryHost) -> Vec<(f32, f32, f32, f32, u32)> {
        host.drawn_gui
            .iter()
            .filter_map(|d| match d {
                GuiDraw::Fill(q, argb) => {
                    let xs = q.map(|p| p[0]);
                    let ys = q.map(|p| p[1]);
                    let min = |v: [f32; 4]| v.into_iter().fold(f32::MAX, f32::min);
                    let max = |v: [f32; 4]| v.into_iter().fold(f32::MIN, f32::max);
                    Some((min(xs), min(ys), max(xs), max(ys), *argb))
                }
                _ => None,
            })
            .collect()
    }

    /// Drawing and clicking as vanilla's field does, with 6 pixel characters: a 40 wide field shows
    /// 32 pixels of text from 4 in, scrolled to keep the cursor in view; at the end the cursor is an
    /// `_` after the text, before a character a bar; a click puts the cursor at the character under
    /// the pointer (shift: extending the selection) and makes no sound.
    #[test]
    fn the_field_scrolls_draws_its_cursor_and_takes_clicks() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost { client: true, gui: [427, 240], ..Default::default() };
        let render = |rt: &mut Rt, b: V| {
            let g = crate::gui::graphics(rt);
            crate::gui::begin_drawing(rt);
            crate::api::call(rt, RENDER, &[b, g, V::I(0), V::I(0), V::F(0.0)]);
            crate::gui::end_drawing(rt);
        };
        let b = rt.with_host(&mut host, |rt| {
            let b = field(rt, 40);
            focus(rt, b);
            typed(rt, b, "abcdefghij");
            render(rt, b);
            b
        });
        // The last five characters, at (14, 26); the cursor after them (the shadowed text's end).
        assert_eq!(texts(&host), [("fghij".into(), [14.0, 26.0]), ("_".into(), [45.0, 26.0])]);
        assert!(host.drawn_gui.iter().any(|d| matches!(d, GuiDraw::Blit(t, ..) if t.contains("text_field_highlighted"))), "{:?}", host.drawn_gui);
        host.drawn_gui.clear();
        rt.with_host(&mut host, |rt| {
            press(rt, b, HOME);
            render(rt, b);
        });
        assert_eq!(texts(&host), [("abcde".into(), [14.0, 26.0])]);
        assert!(fills(&host).contains(&(14.0, 25.0, 15.0, 36.0, 0xFFD0_D0D0)), "the insert cursor: {:?}", fills(&host));
        rt.with_host(&mut host, |rt| {
            // 15 pixels in: two characters fit.
            assert!(click(rt, b, 29.0, 30.0));
            assert_eq!(cursor(rt, b), 2);
            assert!(!click(rt, b, 29.0, 45.0), "below the field");
        });
        host.keys_held = vec!["key.keyboard.right.shift".into()];
        rt.with_host(&mut host, |rt| {
            // 27 pixels in: four.
            click(rt, b, 41.0, 30.0);
            assert_eq!((cursor(rt, b), highlighted(rt, b)), (4, "cd".into()));
        });
        assert!(host.sounds.is_empty(), "a field's click makes no sound");
    }
}
