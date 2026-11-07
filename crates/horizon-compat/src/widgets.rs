//! Vanilla's widgets, natively: `AbstractWidget` (its place and size, message, the states a screen
//! reads, a tooltip, the pointer's clicks as vanilla routes them to `onClick`, `onRelease`,
//! `onDrag`), `AbstractButton` (drawn as vanilla's button sprites, pressed by a click or Enter)
//! and `Button` with its builder, and `Tooltip`. A mod's widget extending them (Create's
//! `AbstractSimiWidget`) keeps its own overrides; what it inherits answers here.
use crate::{Data, Rt, V};

pub(crate) const WIDGET: &str = "net/minecraft/client/gui/components/AbstractWidget";
const ABSTRACT_BUTTON: &str = "net/minecraft/client/gui/components/AbstractButton";
const BUTTON: &str = "net/minecraft/client/gui/components/Button";
const BUILDER: &str = "net/minecraft/client/gui/components/Button$Builder";
const TOOLTIP: &str = "net/minecraft/client/gui/components/Tooltip";
const RENDERABLE: &str = "net/minecraft/client/gui/components/Renderable";
const LAYOUT_ELEMENT: &str = "net/minecraft/client/gui/layouts/LayoutElement";
const NARRATABLE: &str = "net/minecraft/client/gui/narration/NarratableEntry";
const NARRATION_OUTPUT: &str = "net/minecraft/client/gui/narration/NarrationElementOutput";
const COMMON_COMPONENTS: &str = "net/minecraft/network/chat/CommonComponents";
/// One thing a narration output was given: its kind, depth and lines ([`narration`]).
const NARRATION_ENTRY: &str = "compat:narration-entry";
const ADD_COMPONENT: &str = "(Lnet/minecraft/client/gui/narration/NarratedElementType;Lnet/minecraft/network/chat/Component;)V";
const ADD_STRING: &str =
    "(Lnet/minecraft/client/gui/narration/NarratedElementType;Ljava/lang/String;)V";
const ADD_COMPONENTS: &str = "(Lnet/minecraft/client/gui/narration/NarratedElementType;[Lnet/minecraft/network/chat/Component;)V";
const GET_STRING: &str = "net/minecraft/network/chat/Component.getString.()Ljava/lang/String;";
const EMPTY_COMPONENT: &str =
    "net/minecraft/network/chat/Component.empty.()Lnet/minecraft/network/chat/MutableComponent;";
const LITERAL: &str = "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;";
const APPEND: &str = "net/minecraft/network/chat/MutableComponent.append.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/network/chat/MutableComponent;";

const K_RENDER_WIDGET: &str = "net/minecraft/client/gui/components/AbstractWidget.renderWidget.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V";
const K_CLICKED: &str = "net/minecraft/client/gui/components/AbstractWidget.clicked.(DD)Z";
const K_VALID_BUTTON: &str = "net/minecraft/client/gui/components/AbstractWidget.isValidClickButton.(I)Z";
const K_ON_CLICK_BUTTON: &str = "net/minecraft/client/gui/components/AbstractWidget.onClick.(DDI)V";
const K_ON_CLICK: &str = "net/minecraft/client/gui/components/AbstractWidget.onClick.(DD)V";
const K_ON_RELEASE: &str = "net/minecraft/client/gui/components/AbstractWidget.onRelease.(DD)V";
const K_ON_DRAG: &str = "net/minecraft/client/gui/components/AbstractWidget.onDrag.(DDDD)V";
const K_PLAY_DOWN: &str = "net/minecraft/client/gui/components/AbstractWidget.playDownSound.(Lnet/minecraft/client/sounds/SoundManager;)V";
const K_HOVERED_OR_FOCUSED: &str = "net/minecraft/client/gui/components/AbstractWidget.isHoveredOrFocused.()Z";
const K_FG_COLOR: &str = "net/minecraft/client/gui/components/AbstractWidget.getFGColor.()I";
const K_RENDER_STRING: &str = "net/minecraft/client/gui/components/AbstractButton.renderString.(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/client/gui/Font;I)V";
const K_ON_PRESS: &str = "net/minecraft/client/gui/components/AbstractButton.onPress.()V";
const ON_PRESS: &str = "net/minecraft/client/gui/components/Button$OnPress.onPress.(Lnet/minecraft/client/gui/components/Button;)V";
const CONSUMER: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";

const G_BLIT_SPRITE: &str = "net/minecraft/client/gui/GuiGraphics.blitSprite.(Lnet/minecraft/resources/ResourceLocation;IIII)V";
const G_CENTERED: &str = "net/minecraft/client/gui/GuiGraphics.drawCenteredString.(Lnet/minecraft/client/gui/Font;Lnet/minecraft/network/chat/Component;III)V";
const G_SET_COLOR: &str = "net/minecraft/client/gui/GuiGraphics.setColor.(FFFF)V";
const FONT_WIDTH: &str = "net/minecraft/client/gui/Font.width.(Lnet/minecraft/network/chat/FormattedText;)I";
const TOOLTIP_PASS: &str = "net/minecraft/client/gui/screens/Screen.setTooltipForNextRenderPass.(Lnet/minecraft/network/chat/Component;)V";
const GET_INSTANCE: &str = "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;";
const SOUND_MANAGER: &str = "net/minecraft/client/Minecraft.getSoundManager.()Lnet/minecraft/client/sounds/SoundManager;";
const FOR_UI: &str = "net/minecraft/client/resources/sounds/SimpleSoundInstance.forUI.(Lnet/minecraft/core/Holder;F)Lnet/minecraft/client/resources/sounds/SimpleSoundInstance;";
const PLAY: &str = "net/minecraft/client/sounds/SoundManager.play.(Lnet/minecraft/client/resources/sounds/SoundInstance;)V";

/// NeoForge's `AbstractWidget.UNSET_FG_COLOR`: the text colour follows the widget's state.
const UNSET_FG_COLOR: i32 = -1;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

fn truthy(v: V) -> bool {
    matches!(v, V::I(n) if n != 0)
}

fn int(rt: &Rt, o: V, f: &'static str) -> i32 {
    rt.get_field(o, f).i()
}

/// `AbstractWidget`'s constructor: place, size and message; visible, active, opaque.
pub(crate) fn init_widget(rt: &mut Rt, w: V, [x, y, width, height]: [i32; 4], message: V) {
    for (f, v) in [("x", x), ("y", y), ("width", width), ("height", height), ("visible", 1), ("active", 1), ("packedFGColor", UNSET_FG_COLOR)] {
        rt.put_field(w, f, V::I(v));
    }
    rt.put_field(w, "message", message);
    rt.put_field(w, "alpha", V::F(1.0));
}

/// The pointer within the widget's rectangle.
fn within(rt: &Rt, w: V, x: f64, y: f64) -> bool {
    let (wx, wy) = (int(rt, w, "x"), int(rt, w, "y"));
    x >= f64::from(wx) && y >= f64::from(wy) && x < f64::from(wx + int(rt, w, "width")) && y < f64::from(wy + int(rt, w, "height"))
}

fn usable(rt: &Rt, w: V) -> bool {
    truthy(rt.get_field(w, "active")) && truthy(rt.get_field(w, "visible"))
}

/// `playDownSound`: the button click, as the GUI plays it.
fn play_click(rt: &mut Rt, manager: V) {
    let click = crate::api::get_static(rt, "net/minecraft/sounds/SoundEvents.UI_BUTTON_CLICK");
    let sound = crate::api::call(rt, FOR_UI, &[click, V::F(1.0)]);
    crate::api::call(rt, PLAY, &[manager, sound]);
}

fn sound_manager(rt: &mut Rt) -> V {
    let mc = crate::api::call(rt, GET_INSTANCE, &[]);
    crate::api::call(rt, SOUND_MANAGER, &[mc])
}

/// `AbstractWidget.render`: a visible widget learns whether the pointer is over it, draws itself,
/// and shows its tooltip while hovered or focused.
fn render(rt: &mut Rt, this: V, args: &[V]) {
    if !truthy(rt.get_field(this, "visible")) {
        return;
    }
    let (mx, my) = (arg(args, 2).i(), arg(args, 3).i());
    let hovered = within(rt, this, f64::from(mx), f64::from(my));
    rt.put_field(this, "isHovered", V::I(hovered as i32));
    crate::menus::virt(rt, K_RENDER_WIDGET, this, &args[1..]);
    let tip = rt.get_field(this, "$tooltip");
    if tip.r() != 0 && (hovered || truthy(rt.get_field(this, "focused"))) {
        let screen = crate::screens::current(rt);
        let message = rt.get_field(tip, "$message");
        if screen.r() != 0 && message.r() != 0 {
            crate::api::call(rt, TOOLTIP_PASS, &[screen, message]);
        }
    }
}

/// `AbstractWidget.renderScrollingString`: `text` centred in the box (a text too wide for it is
/// drawn centred as well, not scrolled).
fn scrolling_string(rt: &mut Rt, g: V, font: V, text: V, [min_x, min_y, max_x, max_y]: [i32; 4], color: i32) {
    if text.r() == 0 {
        return;
    }
    let width = crate::api::call(rt, FONT_WIDTH, &[font, text]).i();
    let y = (min_y + max_y - 9) / 2 + 1;
    let center = (min_x + max_x) / 2;
    let x = if width > max_x - min_x { center } else { center.clamp(min_x + width / 2, max_x - width / 2) };
    crate::api::call(rt, G_CENTERED, &[g, font, text, V::I(x), V::I(y), V::I(color)]);
}

/// `AbstractButton.renderWidget`: vanilla's button sprite for its state, its message on it.
fn render_button(rt: &mut Rt, this: V, g: V) {
    let lit = truthy(crate::menus::virt(rt, K_HOVERED_OR_FOCUSED, this, &[]));
    let sprite = match (truthy(rt.get_field(this, "active")), lit) {
        (false, _) => "minecraft:widget/button_disabled",
        (true, true) => "minecraft:widget/button_highlighted",
        (true, false) => "minecraft:widget/button",
    };
    let alpha = rt.get_field(this, "alpha").f();
    let s = rt.string(sprite);
    crate::api::call(rt, G_SET_COLOR, &[g, V::F(1.0), V::F(1.0), V::F(1.0), V::F(alpha)]);
    let (x, y, w, h) = (int(rt, this, "x"), int(rt, this, "y"), int(rt, this, "width"), int(rt, this, "height"));
    crate::api::call(rt, G_BLIT_SPRITE, &[g, s, V::I(x), V::I(y), V::I(w), V::I(h)]);
    crate::api::call(rt, G_SET_COLOR, &[g, V::F(1.0), V::F(1.0), V::F(1.0), V::F(1.0)]);
    let color = crate::menus::virt(rt, K_FG_COLOR, this, &[]).i() | ((alpha * 255.0).ceil() as i32) << 24;
    let font = crate::gui::font(rt);
    crate::menus::virt(rt, K_RENDER_STRING, this, &[g, font, V::I(color)]);
}

/// `AbstractButton`'s and `Button`'s own methods.
fn button_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match (name, desc) {
        (
            "<init>",
            "(IIIILnet/minecraft/network/chat/Component;Lnet/minecraft/client/gui/components/Button$OnPress;Lnet/minecraft/client/gui/components/Button$CreateNarration;)V",
        ) => {
            init_widget(rt, this, [a(1).i(), a(2).i(), a(3).i(), a(4).i()], a(5));
            rt.put_field(this, "onPress", a(6));
            V::Void
        }
        ("<init>", "(Lnet/minecraft/client/gui/components/Button$Builder;)V") => {
            let b = a(1);
            let at = [int(rt, b, "x"), int(rt, b, "y"), int(rt, b, "width"), int(rt, b, "height")];
            let message = rt.get_field(b, "message");
            init_widget(rt, this, at, message);
            let press = rt.get_field(b, "onPress");
            rt.put_field(this, "onPress", press);
            let tip = rt.get_field(b, "tooltip");
            rt.put_field(this, "$tooltip", tip);
            V::Void
        }
        ("onPress", "()V") => {
            let press = rt.get_field(this, "onPress");
            if press.r() != 0 {
                rt.call_functional(ON_PRESS, press, &[this]);
            }
            V::Void
        }
        ("onClick", "(DD)V") => {
            crate::menus::virt(rt, K_ON_PRESS, this, &[]);
            V::Void
        }
        ("renderWidget", _) => {
            render_button(rt, this, a(1));
            V::Void
        }
        ("renderString", _) => {
            let message = rt.get_field(this, "message");
            let (x, y, w, h) = (int(rt, this, "x"), int(rt, this, "y"), int(rt, this, "width"), int(rt, this, "height"));
            scrolling_string(rt, a(1), a(2), message, [x + 2, y, x + w - 2, y + h], a(3).i());
            V::Void
        }
        // Enter, Space or the keypad's Enter press a usable button.
        ("keyPressed", "(III)Z") => {
            if !usable(rt, this) || !matches!(a(1).i(), 257 | 32 | 335) {
                return Some(V::I(0));
            }
            let manager = sound_manager(rt);
            crate::menus::virt(rt, K_PLAY_DOWN, this, &[manager]);
            crate::menus::virt(rt, K_ON_PRESS, this, &[]);
            V::I(1)
        }
        _ => return None,
    })
}

/// `AbstractWidget`'s methods (and what `GuiEventListener`, `Renderable`, `LayoutElement` and
/// `NarratableEntry` give it).
fn widget_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match (name, desc) {
        ("<init>", "(IIIILnet/minecraft/network/chat/Component;)V") => {
            init_widget(rt, this, [a(1).i(), a(2).i(), a(3).i(), a(4).i()], a(5));
            V::Void
        }
        ("render", "(Lnet/minecraft/client/gui/GuiGraphics;IIF)V") => {
            render(rt, this, args);
            V::Void
        }
        // Abstract.
        ("renderWidget", _) | ("onClick" | "onRelease", "(DD)V") | ("onDrag", "(DDDD)V") => V::Void,
        ("onClick", "(DDI)V") => {
            crate::menus::virt(rt, K_ON_CLICK, this, &[a(1), a(2)]);
            V::Void
        }
        ("mouseClicked", "(DDI)Z") => {
            if !usable(rt, this) || !truthy(crate::menus::virt(rt, K_VALID_BUTTON, this, &[a(3)])) {
                return Some(V::I(0));
            }
            if !truthy(crate::menus::virt(rt, K_CLICKED, this, &[a(1), a(2)])) {
                return Some(V::I(0));
            }
            let manager = sound_manager(rt);
            crate::menus::virt(rt, K_PLAY_DOWN, this, &[manager]);
            crate::menus::virt(rt, K_ON_CLICK_BUTTON, this, &[a(1), a(2), a(3)]);
            V::I(1)
        }
        ("mouseReleased", "(DDI)Z") => {
            if !truthy(crate::menus::virt(rt, K_VALID_BUTTON, this, &[a(3)])) {
                return Some(V::I(0));
            }
            crate::menus::virt(rt, K_ON_RELEASE, this, &[a(1), a(2)]);
            V::I(1)
        }
        ("mouseDragged", "(DDIDD)Z") => {
            if !truthy(crate::menus::virt(rt, K_VALID_BUTTON, this, &[a(3)])) {
                return Some(V::I(0));
            }
            crate::menus::virt(rt, K_ON_DRAG, this, &[a(1), a(2), a(4), a(5)]);
            V::I(1)
        }
        ("isValidClickButton", _) => V::I((a(1).i() == 0) as i32),
        ("clicked" | "isMouseOver", "(DD)Z") => V::I((usable(rt, this) && within(rt, this, a(1).d(), a(2).d())) as i32),
        ("playDownSound", _) => {
            play_click(rt, a(1));
            V::Void
        }
        ("mouseMoved", _) => V::Void,
        ("setFocused", "(Z)V") => {
            rt.put_field(this, "focused", V::I(truthy(a(1)) as i32));
            V::Void
        }
        ("isFocused", "()Z") => V::I(truthy(rt.get_field(this, "focused")) as i32),
        ("isHovered", "()Z") => V::I(truthy(rt.get_field(this, "isHovered")) as i32),
        ("isHoveredOrFocused", _) => V::I((truthy(rt.get_field(this, "isHovered")) || truthy(rt.get_field(this, "focused"))) as i32),
        ("isActive", "()Z") => V::I(usable(rt, this) as i32),
        ("mouseScrolled", _) | ("keyPressed" | "keyReleased", "(III)Z") | ("charTyped", "(CI)Z") => V::I(0),
        ("getX", "()I") => rt.get_field(this, "x"),
        ("getY", "()I") => rt.get_field(this, "y"),
        ("getWidth", "()I") => rt.get_field(this, "width"),
        ("getHeight", "()I") => rt.get_field(this, "height"),
        ("getRight", "()I") => V::I(int(rt, this, "x") + int(rt, this, "width")),
        ("getBottom", "()I") => V::I(int(rt, this, "y") + int(rt, this, "height")),
        ("setX", "(I)V") => {
            rt.put_field(this, "x", a(1));
            V::Void
        }
        ("setY", "(I)V") => {
            rt.put_field(this, "y", a(1));
            V::Void
        }
        ("setPosition", "(II)V") => {
            rt.put_field(this, "x", a(1));
            rt.put_field(this, "y", a(2));
            V::Void
        }
        ("setWidth", "(I)V") => {
            rt.put_field(this, "width", a(1));
            V::Void
        }
        ("setHeight", "(I)V") => {
            rt.put_field(this, "height", a(1));
            V::Void
        }
        ("setSize", "(II)V") => {
            rt.put_field(this, "width", a(1));
            rt.put_field(this, "height", a(2));
            V::Void
        }
        ("setRectangle", "(IIII)V") => {
            for (f, i) in [("width", 1), ("height", 2), ("x", 3), ("y", 4)] {
                rt.put_field(this, f, a(i));
            }
            V::Void
        }
        ("getMessage", _) => rt.get_field(this, "message"),
        ("setMessage", _) => {
            rt.put_field(this, "message", a(1));
            V::Void
        }
        ("setAlpha", _) => {
            rt.put_field(this, "alpha", V::F(a(1).f()));
            V::Void
        }
        ("getAlpha", _) => rt.get_field(this, "alpha"),
        ("setTooltip", _) => {
            rt.put_field(this, "$tooltip", a(1));
            V::Void
        }
        ("getTooltip", _) => rt.get_field(this, "$tooltip"),
        ("setTooltipDelay", _) => V::Void,
        ("getFGColor", _) => {
            let packed = int(rt, this, "packedFGColor");
            V::I(if packed != UNSET_FG_COLOR {
                packed
            } else if truthy(rt.get_field(this, "active")) {
                16777215
            } else {
                10526880
            })
        }
        ("setFGColor", _) => {
            rt.put_field(this, "packedFGColor", a(1));
            V::Void
        }
        ("clearFGColor", _) => {
            rt.put_field(this, "packedFGColor", V::I(UNSET_FG_COLOR));
            V::Void
        }
        ("visitWidgets", _) => {
            rt.call_functional(CONSUMER, a(1), &[this]);
            V::Void
        }
        // protected renderScrollingString(graphics, font, margin, colour)
        ("renderScrollingString", "(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/client/gui/Font;II)V") => {
            let message = rt.get_field(this, "message");
            let (x, y, w, h) = (int(rt, this, "x"), int(rt, this, "y"), int(rt, this, "width"), int(rt, this, "height"));
            let margin = a(3).i();
            scrolling_string(rt, a(1), a(2), message, [x + margin, y, x + w - margin, y + h], a(4).i());
            V::Void
        }
        // static (graphics, font, text, minX, minY, maxX, maxY, colour), and with a centre first.
        ("renderScrollingString", _) => {
            let o = usize::from(desc.ends_with("IIIIII)V"));
            scrolling_string(rt, a(0), a(1), a(2), [a(3 + o).i(), a(4 + o).i(), a(5 + o).i(), a(6 + o).i()], a(7 + o).i());
            V::Void
        }
        ("updateNarration" | "updateWidgetNarration" | "defaultButtonNarrationText", _) => V::Void,
        ("createNarrationMessage", _) => rt.get_field(this, "message"),
        ("narrationPriority", _) => V::NULL,
        ("isMouseOverForNarration", _) => V::I(0),
        ("getTabOrderGroup", _) => V::I(0),
        ("getCurrentFocusPath" | "nextFocusPath" | "getRectangle", _) => V::NULL,
        _ => return None,
    })
}

/// The methods a widget answers natively, `EditBox`'s or `AbstractButton`'s first: None for a
/// receiver that is no widget or a method that isn't modelled.
pub(crate) fn own(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = arg(args, 0);
    if !rt.instance_of(this, WIDGET) {
        return None;
    }
    if rt.instance_of(this, crate::edit_box::EDIT_BOX)
        && let Some(v) = crate::edit_box::own(rt, name, desc, args)
    {
        return Some(v);
    }
    if rt.instance_of(this, ABSTRACT_BUTTON)
        && let Some(v) = button_call(rt, name, desc, args)
    {
        return Some(v);
    }
    widget_call(rt, name, desc, args)
}

fn builder_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let b = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(b, "message", a(1));
            rt.put_field(b, "onPress", a(2));
            rt.put_field(b, "width", V::I(150));
            rt.put_field(b, "height", V::I(20));
            V::Void
        }
        "pos" => {
            rt.put_field(b, "x", a(1));
            rt.put_field(b, "y", a(2));
            b
        }
        "width" => {
            rt.put_field(b, "width", a(1));
            b
        }
        "size" => {
            rt.put_field(b, "width", a(1));
            rt.put_field(b, "height", a(2));
            b
        }
        "bounds" => {
            for (f, i) in [("x", 1), ("y", 2), ("width", 3), ("height", 4)] {
                rt.put_field(b, f, a(i));
            }
            b
        }
        "tooltip" => {
            rt.put_field(b, "tooltip", a(1));
            b
        }
        "createNarration" => b,
        "build" => {
            let button = rt.new_object(BUTTON);
            button_call(rt, "<init>", "(Lnet/minecraft/client/gui/components/Button$Builder;)V", &[button, b]);
            button
        }
        _ => return None,
    })
}

/// A screen's narration output (`ScreenNarrationCollector`'s), which its widgets'
/// `updateNarration` fill: [`narration`] reads it, for the game's narrator.
pub fn narration_output(rt: &mut Rt) -> V {
    let o = rt.new_object(NARRATION_OUTPUT);
    let entries = crate::jutil::new_list(rt, Vec::new());
    rt.put_field(o, "$entries", entries);
    rt.put_field(o, "$depth", V::I(0));
    o
}

/// What narration output `output` holds, as vanilla's collector reads it out: one entry for each
/// kind (`NarratedElementType`'s ordinal: title, position, hint, usage) and depth (how many
/// `nest()`s down), the last given, in that order.
pub fn narration(rt: &Rt, output: V) -> Vec<(i32, i32, Vec<String>)> {
    let entries = rt.get_field(output, "$entries");
    let mut all: Vec<(i32, i32, Vec<String>)> = crate::jutil::elements(rt, entries)
        .into_iter()
        .map(|e| {
            let lines = crate::jutil::elements(rt, rt.get_field(e, "$lines"))
                .into_iter()
                .map(|l| rt.str_of(l).unwrap_or("").to_owned())
                .collect();
            (
                rt.get_field(e, "$kind").i(),
                rt.get_field(e, "$depth").i(),
                lines,
            )
        })
        .collect();
    all.sort_by_key(|(kind, depth, _)| (*kind, *depth));
    all
}

/// `NarrationElementOutput`'s `add` and `nest`, on an output the runtime made (its class is the
/// interface's: [`narration_output`]'s): None for a mod's own output, which adds through its own
/// `add`, and for contents given as a `NarrationThunk`.
fn narration_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let out = arg(args, 0);
    let mut entries = rt.get_field(out, "$entries");
    if entries.r() == 0 {
        if out.r() == 0 || rt.class_of(out) != NARRATION_OUTPUT {
            return None;
        }
        entries = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(out, "$entries", entries);
    }
    let depth = rt.get_field(out, "$depth").i();
    match name {
        "nest" => {
            let nested = rt.new_object(NARRATION_OUTPUT);
            rt.put_field(nested, "$entries", entries);
            rt.put_field(nested, "$depth", V::I(depth + 1));
            Some(nested)
        }
        "add" => {
            let contents = arg(args, 2);
            let lines = match desc {
                ADD_STRING => vec![contents],
                ADD_COMPONENT => vec![crate::api::call(rt, GET_STRING, &[contents])],
                ADD_COMPONENTS => crate::jutil::elements(rt, contents)
                    .into_iter()
                    .map(|c| crate::api::call(rt, GET_STRING, &[c]))
                    .collect(),
                _ => return None,
            };
            let kind = rt.get_field(arg(args, 1), "$ordinal").i();
            let lines = crate::jutil::new_list(rt, lines);
            // The collector keeps one entry for each kind and depth: the last given.
            let same = crate::jutil::elements(rt, entries).into_iter().find(|e| {
                rt.get_field(*e, "$kind").i() == kind && rt.get_field(*e, "$depth").i() == depth
            });
            let entry = match same {
                Some(e) => e,
                None => {
                    let e = rt.new_object(NARRATION_ENTRY);
                    rt.put_field(e, "$kind", V::I(kind));
                    rt.put_field(e, "$depth", V::I(depth));
                    crate::jutil::list_add(rt, entries, e);
                    e
                }
            };
            rt.put_field(entry, "$lines", lines);
            Some(V::Void)
        }
        _ => None,
    }
}

/// `CommonComponents.joinForNarration(components)`: the components, `. ` between each two.
fn join_for_narration(rt: &mut Rt, components: V) -> V {
    let parts = crate::jutil::elements(rt, components);
    let joined = crate::api::call(rt, EMPTY_COMPONENT, &[]);
    let separator = rt.string(". ");
    let separator = crate::api::call(rt, LITERAL, &[separator]);
    for (i, c) in parts.iter().enumerate() {
        crate::api::call(rt, APPEND, &[joined, *c]);
        if i + 1 != parts.len() {
            crate::api::call(rt, APPEND, &[joined, separator]);
        }
    }
    joined
}

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(owner, WIDGET | ABSTRACT_BUTTON | BUTTON | BUILDER | TOOLTIP | RENDERABLE | LAYOUT_ELEMENT | NARRATABLE | crate::edit_box::EDIT_BOX)
        || (owner == NARRATION_OUTPUT && matches!(name, "add" | "nest"))
        || (owner == COMMON_COMPONENTS && name == "joinForNarration")
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    match (owner, name) {
        (NARRATION_OUTPUT, _) => narration_call(rt, name, desc, args),
        (COMMON_COMPONENTS, _) => Some(join_for_narration(rt, arg(args, 0))),
        // `wrapDefaultNarrationMessage(message)`: "Button: message".
        (WIDGET, "wrapDefaultNarrationMessage") => {
            let key = rt.string("gui.narrate.button");
            let with = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![arg(args, 0)]));
            Some(crate::api_common::component(rt, key, true, with))
        }
        (BUILDER, _) => builder_call(rt, name, args),
        (BUTTON, "builder") => {
            let b = rt.new_object(BUILDER);
            builder_call(rt, "<init>", &[b, arg(args, 0), arg(args, 1)]);
            Some(b)
        }
        (TOOLTIP, "create") => {
            let t = rt.new_object(TOOLTIP);
            rt.put_field(t, "$message", arg(args, 0));
            Some(t)
        }
        (TOOLTIP, _) => None,
        _ => own(rt, name, desc, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADD: &str = "net/minecraft/client/gui/narration/NarrationElementOutput.add.(Lnet/minecraft/client/gui/narration/NarratedElementType;Lnet/minecraft/network/chat/Component;)V";
    const ADD_ALL: &str = "net/minecraft/client/gui/narration/NarrationElementOutput.add.(Lnet/minecraft/client/gui/narration/NarratedElementType;[Lnet/minecraft/network/chat/Component;)V";
    const NEST: &str = "net/minecraft/client/gui/narration/NarrationElementOutput.nest.()Lnet/minecraft/client/gui/narration/NarrationElementOutput;";
    const JOIN: &str = "net/minecraft/network/chat/CommonComponents.joinForNarration.([Lnet/minecraft/network/chat/Component;)Lnet/minecraft/network/chat/MutableComponent;";

    fn literal(rt: &mut Rt, text: &str) -> V {
        let t = rt.string(text);
        crate::api::call(rt, LITERAL, &[t])
    }

    /// A screen's widgets narrate into one output, as vanilla's collector keeps it: an entry for
    /// each kind and depth, the last one given, read out by kind and then depth.
    #[test]
    fn widgets_narrate_into_their_screens_output() {
        crate::install();
        let mut rt = Rt::new();
        let out = narration_output(&mut rt);
        let title = crate::api::get_static(
            &mut rt,
            "net/minecraft/client/gui/narration/NarratedElementType.TITLE",
        );
        let usage = crate::api::get_static(
            &mut rt,
            "net/minecraft/client/gui/narration/NarratedElementType.USAGE",
        );
        let (screen, hint) = (literal(&mut rt, "Stockpile"), literal(&mut rt, "Use Tab"));
        crate::api::call(&mut rt, ADD, &[out, usage, hint]);
        crate::api::call(&mut rt, ADD, &[out, title, screen]);
        let widget = crate::api::call(&mut rt, NEST, &[out]);
        let first = literal(&mut rt, "Button: Sort");
        crate::api::call(&mut rt, ADD, &[widget, title, first]);
        let (button, state) = (literal(&mut rt, "Button: Filter"), literal(&mut rt, "Off"));
        let both = rt.alloc(
            "[Lnet/minecraft/network/chat/Component;",
            Data::Array(vec![button, state]),
        );
        crate::api::call(&mut rt, ADD_ALL, &[widget, title, both]);
        let lines = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            narration(&rt, out),
            vec![
                (0, 0, lines(&["Stockpile"])),
                (0, 1, lines(&["Button: Filter", "Off"])),
                (3, 0, lines(&["Use Tab"]))
            ]
        );
        let (a, b) = (literal(&mut rt, "Sort"), literal(&mut rt, "Descending"));
        let parts = rt.alloc(
            "[Lnet/minecraft/network/chat/Component;",
            Data::Array(vec![a, b]),
        );
        let joined = crate::api::call(&mut rt, JOIN, &[parts]);
        let text = crate::api::call(&mut rt, GET_STRING, &[joined]);
        assert_eq!(rt.str_of(text), Some("Sort. Descending"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
