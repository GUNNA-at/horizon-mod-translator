//! Mods' screens on the client, natively. `Minecraft.setScreen` with a mod's `Screen` opens the
//! host's screen over the world ([`set_screen`]); the game's screen hooks then draw it each frame
//! ([`render`]) and give it the pointer, the wheel, the keys and typing ([`input`]), and the client
//! tick ticks it ([`tick`]). Vanilla's `Screen` is modelled here (its widget lists, `init` for the
//! GUI's size, backgrounds, Escape), with `ContainerEventHandler`'s defaults (focus, dragging, the
//! child under the pointer), the key mappings mods read (`Options.keyUse`: the key bound to it,
//! whether it is held, `matchesMouse`), `InputConstants`, and GUI geometry built with `Tesselator`
//! (a `BufferBuilder`'s quads drawn as the host's fills and blits, the texture the last
//! `RenderSystem.setShaderTexture` named).
//!
//! A screen's drawing and input come with a read-only host (the game lends them `&self`): what
//! would change the game then (a payload sent, a screen opened or closed, the pointer moved) waits
//! in an outbox for the client's next tick ([`flush`]).
use crate::host::{PayloadTarget, Sound};
use crate::render;
use crate::{Data, Rt, V};

const SCREEN: &str = "net/minecraft/client/gui/screens/Screen";
const CONTAINER: &str = "net/minecraft/client/gui/components/events/ContainerEventHandler";
const ABSTRACT_CONTAINER: &str = "net/minecraft/client/gui/components/events/AbstractContainerEventHandler";
const LISTENER: &str = "net/minecraft/client/gui/components/events/GuiEventListener";
const MINECRAFT: &str = "net/minecraft/client/Minecraft";
const KEY_MAPPING: &str = "net/minecraft/client/KeyMapping";
const INPUT: &str = "com/mojang/blaze3d/platform/InputConstants";
const INPUT_KEY: &str = "com/mojang/blaze3d/platform/InputConstants$Key";
const INPUT_TYPE: &str = "com/mojang/blaze3d/platform/InputConstants$Type";
const TESSELATOR: &str = "com/mojang/blaze3d/vertex/Tesselator";
const BUFFER_BUILDER: &str = "com/mojang/blaze3d/vertex/BufferBuilder";
const MESH: &str = "com/mojang/blaze3d/vertex/MeshData";
const UPLOADER: &str = "com/mojang/blaze3d/vertex/BufferUploader";
const MODE: &str = "com/mojang/blaze3d/vertex/VertexFormat$Mode";
const GLFW: &str = "org/lwjgl/glfw/GLFW";

// What is called virtually: a mod's override answers first, else the natives below.
const K_INIT: &str = "net/minecraft/client/gui/screens/Screen.init.()V";
const K_ADDED: &str = "net/minecraft/client/gui/screens/Screen.added.()V";
const K_REMOVED: &str = "net/minecraft/client/gui/screens/Screen.removed.()V";
const K_TICK: &str = "net/minecraft/client/gui/screens/Screen.tick.()V";
const K_REPOSITION: &str = "net/minecraft/client/gui/screens/Screen.repositionElements.()V";
const K_RENDER: &str = "net/minecraft/client/gui/screens/Screen.render.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V";
const K_RENDER_BACKGROUND: &str = "net/minecraft/client/gui/screens/Screen.renderBackground.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V";
const K_MENU_BACKGROUND: &str = "net/minecraft/client/gui/screens/Screen.renderMenuBackground.(Lnet/minecraft/client/gui/GuiGraphics;)V";
const K_MENU_BACKGROUND_AT: &str = "net/minecraft/client/gui/screens/Screen.renderMenuBackground.(Lnet/minecraft/client/gui/GuiGraphics;IIII)V";
const K_CLOSE_ON_ESC: &str = "net/minecraft/client/gui/screens/Screen.shouldCloseOnEsc.()Z";
const K_ON_CLOSE: &str = "net/minecraft/client/gui/screens/Screen.onClose.()V";
const K_CHILDREN: &str = "net/minecraft/client/gui/components/events/ContainerEventHandler.children.()Ljava/util/List;";
const K_GET_FOCUSED: &str = "net/minecraft/client/gui/components/events/ContainerEventHandler.getFocused.()Lnet/minecraft/client/gui/components/events/GuiEventListener;";
const K_SET_FOCUSED: &str = "net/minecraft/client/gui/components/events/ContainerEventHandler.setFocused.(Lnet/minecraft/client/gui/components/events/GuiEventListener;)V";
const K_IS_DRAGGING: &str = "net/minecraft/client/gui/components/events/ContainerEventHandler.isDragging.()Z";
const K_SET_DRAGGING: &str = "net/minecraft/client/gui/components/events/ContainerEventHandler.setDragging.(Z)V";
const L_CLICKED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.mouseClicked.(DDI)Z";
const L_RELEASED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.mouseReleased.(DDI)Z";
const L_DRAGGED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.mouseDragged.(DDIDD)Z";
const L_SCROLLED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.mouseScrolled.(DDDD)Z";
const L_KEY_PRESSED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.keyPressed.(III)Z";
const L_KEY_RELEASED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.keyReleased.(III)Z";
const L_CHAR_TYPED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.charTyped.(CI)Z";
const L_OVER: &str = "net/minecraft/client/gui/components/events/GuiEventListener.isMouseOver.(DD)Z";
const L_SET_FOCUSED: &str = "net/minecraft/client/gui/components/events/GuiEventListener.setFocused.(Z)V";
const RENDERABLE_RENDER: &str = "net/minecraft/client/gui/components/Renderable.render.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V";
const RUN: &str = "java/lang/Runnable.run.()V";
const TOOLTIP_SEQUENCES: &str = "net/minecraft/client/gui/GuiGraphics.renderTooltip.(Lnet/minecraft/client/gui/Font;Ljava/util/List;II)V";

/// The screen open on this client (also `Minecraft.screen`).
const OPEN: &str = "compat:screen";
/// Set while the screen waits for the game to open it (it was opened with a read-only host).
const PENDING: &str = "compat:screen-pending";
/// What waits for the client's next tick: `compat:deferred` objects, in order.
const OUTBOX: &str = "compat:outbox";
/// The texture `RenderSystem.setShaderTexture(0, ...)` named last: what textured quads sample.
pub(crate) const SHADER_TEXTURE: &str = "compat:gui-texture";

/// Vanilla's in-world menu background, tiled 32 x 32 (`Screen.renderMenuBackground`).
const MENU_BACKGROUND: &str = "minecraft:textures/gui/inworld_menu_background.png";

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

fn list_push(rt: &mut Rt, list: V, v: V) {
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.push(v);
    }
}

fn list_remove(rt: &mut Rt, list: V, v: V) {
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data)
        && let Some(i) = l.iter().position(|x| *x == v)
    {
        l.remove(i);
    }
}

fn list_clear(rt: &mut Rt, list: V) {
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.clear();
    }
}

fn truthy(v: V) -> bool {
    matches!(v, V::I(n) if n != 0)
}

// ---- the open screen ----

/// The mods' screen open on this client, or null.
pub fn current(rt: &Rt) -> V {
    rt.interned.get(OPEN).copied().unwrap_or(V::NULL)
}

fn minecraft(rt: &mut Rt) -> V {
    crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[])
}

fn read_only(rt: &Rt) -> bool {
    rt.host().is_none_or(|h| h.read_only())
}

/// The lists a screen keeps (`Screen`'s constructor): its widgets that draw, those that take input,
/// and those the narrator reads.
fn ensure_lists(rt: &mut Rt, s: V) {
    for field in ["renderables", "children", "narratables"] {
        if rt.get_field(s, field).r() == 0 {
            let l = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(s, field, l);
        }
    }
}

/// `Minecraft.setScreen`: the screen open before is told it is `removed`, the new one is `added`
/// and laid out for the GUI's size (`init`), and the game opens its screen over the world for it
/// (or closes it, for null).
pub fn set_screen(rt: &mut Rt, screen: V) {
    let old = current(rt);
    if old.r() != 0 && old != screen {
        rt.call_functional(K_REMOVED, old, &[]);
        rt.report_thrown("a screen's removed()");
    }
    let mc = minecraft(rt);
    if screen.r() == 0 {
        rt.interned.remove(OPEN);
        rt.interned.remove(PENDING);
        rt.put_field(mc, "screen", V::NULL);
        if old.r() != 0 {
            close_on_host(rt);
        }
        return;
    }
    rt.interned.insert(OPEN.into(), screen);
    rt.put_field(mc, "screen", screen);
    rt.call_functional(K_ADDED, screen, &[]);
    rt.report_thrown("a screen's added()");
    // Vanilla frees the mouse before `init` (`Minecraft.setScreen`: `releaseMouse`, then
    // `screen.init`), so a screen that moves the pointer there (Create's value settings put it
    // on the current value) moves a free one.
    if old.r() == 0 && current(rt) == screen {
        open_on_host(rt);
        if current(rt) != screen {
            return;
        }
    }
    crate::gui::fill_minecraft(rt, mc);
    let [w, h] = rt.host().map_or([0, 0], |h| h.gui_size());
    init_screen(rt, screen, mc, w, h);
}

/// `Screen.init(Minecraft, width, height)`: the fields a screen reads set, then its own `init`
/// the first time (its widgets), `repositionElements` after.
fn init_screen(rt: &mut Rt, s: V, mc: V, w: i32, h: i32) {
    ensure_lists(rt, s);
    rt.put_field(s, "minecraft", mc);
    let font = crate::gui::font(rt);
    rt.put_field(s, "font", font);
    rt.put_field(s, "width", V::I(w));
    rt.put_field(s, "height", V::I(h));
    if rt.get_field(s, "$initialized").i() == 0 {
        rt.call_functional(K_INIT, s, &[]);
        rt.report_thrown("a screen's init()");
    } else {
        rt.call_functional(K_REPOSITION, s, &[]);
        rt.report_thrown("a screen's repositionElements()");
    }
    rt.put_field(s, "$initialized", V::I(1));
}

fn open_on_host(rt: &mut Rt) {
    if read_only(rt) && rt.host().is_some() {
        rt.interned.insert(PENDING.into(), V::I(1));
        defer(rt, 1, |_, _| {});
        return;
    }
    if !rt.host_mut().is_some_and(|h| h.open_screen()) {
        refused(rt);
    }
}

/// The game couldn't open its screen (another of its own is open, the world loads): the mod's is
/// let go, as if closed.
fn refused(rt: &mut Rt) {
    let s = current(rt);
    crate::glue::trace_once(rt, "screen-refused", |rt| format!("client: the game didn't open screen {}", rt.class_of(s)));
    if s.r() != 0 {
        set_screen(rt, V::NULL);
    }
}

fn close_on_host(rt: &mut Rt) {
    if read_only(rt) {
        defer(rt, 2, |_, _| {});
        return;
    }
    if let Some(h) = rt.host_mut() {
        h.close_screen();
    }
}

/// The game closed its screen itself (Escape over a mod's screen, another screen of the game's
/// replacing it): the mod's screen is let go as `setScreen(null)` would.
fn sync(rt: &mut Rt) {
    if current(rt).r() == 0 || rt.interned.contains_key(PENDING) {
        return;
    }
    if rt.host().is_some_and(|h| !h.screen_open()) {
        // A menu open behind the screen closes with it; the server hears (Escape, the game's own
        // screen over the mod's).
        crate::menus::screen_gone(rt);
        set_screen(rt, V::NULL);
    }
}

// ---- the outbox ----

/// Queues what a read-only host can't do now: `kind` 0 a payload to the server, 1 open the
/// screen, 2 close it, 3 move the pointer; `fill` sets the entry's fields.
fn defer(rt: &mut Rt, kind: i32, fill: impl FnOnce(&mut Rt, V)) {
    let list = rt.intern(OUTBOX.into(), |rt| crate::jutil::new_list(rt, Vec::new()));
    let e = rt.new_object("compat:deferred");
    rt.put_field(e, "$kind", V::I(kind));
    fill(rt, e);
    list_push(rt, list, e);
}

/// Whether a payload to `to` must wait for the client's next tick, queued if so (a screen's
/// input sent it, with a host that only reads).
pub fn defer_payload(rt: &mut Rt, to: PayloadTarget, channel: &str, data: &[u8]) -> bool {
    if to != PayloadTarget::Server || !rt.host().is_some_and(|h| h.read_only()) {
        return false;
    }
    let (channel, data) = (channel.to_owned(), data.to_vec());
    defer(rt, 0, move |rt, e| {
        let c = rt.string(&channel);
        rt.put_field(e, "$channel", c);
        let b = rt.alloc("compat:bytes", Data::Bytes(data, 0));
        rt.put_field(e, "$data", b);
    });
    true
}

/// Whether `sound` must wait for the client's next tick, queued if so (a screen's drawing or input
/// played it, with a host that only reads: Create's value settings click as the value changes).
pub fn defer_sound(rt: &mut Rt, sound: &Sound) -> bool {
    if !rt.host().is_some_and(|h| h.read_only()) {
        return false;
    }
    let sound = sound.clone();
    defer(rt, 4, move |rt, e| {
        let event = rt.string(&sound.event);
        rt.put_field(e, "$event", event);
        for (f, v) in ["$x", "$y", "$z"].into_iter().zip(sound.at) {
            rt.put_field(e, f, V::D(v));
        }
        rt.put_field(e, "$source", V::I(sound.source as i32));
        rt.put_field(e, "$volume", V::F(sound.volume));
        rt.put_field(e, "$pitch", V::F(sound.pitch));
    });
    true
}

/// Does what screens' read-only hosts left for the client's tick, in order (the host now can):
/// payloads go to the server, the screen opens or closes, the pointer moves, sounds play.
pub fn flush(rt: &mut Rt) {
    let Some(list) = rt.interned.remove(OUTBOX) else { return };
    for e in crate::jutil::elements(rt, list) {
        match rt.get_field(e, "$kind").i() {
            0 => {
                let channel = rt.str_of(rt.get_field(e, "$channel")).unwrap_or("").to_owned();
                let data = match rt.heap.get(rt.get_field(e, "$data").r() as usize).map(|o| &o.data) {
                    Some(Data::Bytes(b, _)) => b.clone(),
                    _ => Vec::new(),
                };
                let sent = rt.host_mut().is_some_and(|h| h.send_payload(PayloadTarget::Server, &channel, &data));
                crate::glue::trace_some(rt, "payload-sent", &channel, 40, |_| {
                    format!("payload {channel} ({} bytes) to Server after the screen's input: {}", data.len(), if sent { "sent" } else { "not sent" })
                });
            }
            1 => {
                rt.interned.remove(PENDING);
                if current(rt).r() != 0 && !rt.host().is_some_and(|h| h.screen_open()) && !rt.host_mut().is_some_and(|h| h.open_screen()) {
                    refused(rt);
                }
            }
            2 => {
                if current(rt).r() == 0
                    && let Some(h) = rt.host_mut()
                {
                    h.close_screen();
                }
            }
            3 => {
                let at = [rt.get_field(e, "$x").f(), rt.get_field(e, "$y").f()];
                if let Some(h) = rt.host_mut() {
                    h.set_cursor(at);
                }
            }
            5 => {
                let text = rt.str_of(rt.get_field(e, "$text")).unwrap_or("").to_owned();
                if let Some(h) = rt.host_mut() {
                    h.set_clipboard(&text);
                }
            }
            _ => {
                let sound = Sound {
                    event: rt.str_of(rt.get_field(e, "$event")).unwrap_or("").to_owned(),
                    at: [rt.get_field(e, "$x").d(), rt.get_field(e, "$y").d(), rt.get_field(e, "$z").d()],
                    source: rt.get_field(e, "$source").i().max(0) as usize,
                    volume: rt.get_field(e, "$volume").f(),
                    pitch: rt.get_field(e, "$pitch").f(),
                };
                if let Some(h) = rt.host_mut() {
                    h.play_sound(&sound);
                }
            }
        }
    }
}

/// `KeyboardHandler.getClipboard()`: the clipboard's text, empty when none.
pub(crate) fn clipboard(rt: &Rt) -> String {
    rt.host().map(|h| h.clipboard()).unwrap_or_default()
}

/// `KeyboardHandler.setClipboard(text)`: now, or at the next tick when the host only reads (a
/// field's copy as a key goes down).
pub(crate) fn set_clipboard(rt: &mut Rt, text: &str) {
    if read_only(rt) {
        let text = text.to_owned();
        defer(rt, 5, move |rt, e| {
            let s = rt.string(&text);
            rt.put_field(e, "$text", s);
        });
    } else if let Some(h) = rt.host_mut() {
        h.set_clipboard(text);
    }
}

/// Moves the pointer (`GLFW.glfwSetCursorPos`), now or at the next tick.
fn set_cursor(rt: &mut Rt, at: [f32; 2]) {
    if read_only(rt) {
        defer(rt, 3, |rt, e| {
            rt.put_field(e, "$x", V::F(at[0]));
            rt.put_field(e, "$y", V::F(at[1]));
        });
    } else if let Some(h) = rt.host_mut() {
        h.set_cursor(at);
    }
}

// ---- the game's hooks ----

/// The client's tick for screens (`Minecraft.tick`): what screens left for it goes out, then the
/// open screen ticks.
pub fn tick(rt: &mut Rt) {
    flush(rt);
    sync(rt);
    let s = current(rt);
    if s.r() != 0 {
        rt.call_functional(K_TICK, s, &[]);
        rt.report_thrown("ticking a screen");
    }
}

/// Draws the open screen (`ClientHooks.drawScreen` -> `Screen.renderWithTooltip`), the pointer at
/// `mouse` in GUI pixels.
pub fn render(rt: &mut Rt, mouse: [f32; 2], partial: f32) {
    sync(rt);
    let s = current(rt);
    if s.r() == 0 {
        return;
    }
    crate::gui::begin_drawing(rt);
    let g = crate::gui::graphics(rt);
    let (x, y) = (V::I(mouse[0] as i32), V::I(mouse[1] as i32));
    rt.call_functional(K_RENDER, s, &[g, x, y, V::F(partial)]);
    rt.report_thrown("drawing a screen");
    deferred_tooltip(rt, s, g, x, y);
    crate::gui::end_drawing(rt);
}

/// What reaches the open screen from the pointer and the keyboard.
#[derive(Clone, Copy, Debug)]
pub enum Input<'a> {
    Press(i32),
    Release(i32),
    Drag(i32, [f32; 2]),
    Scroll([f64; 2]),
    Key(&'a str, bool),
    Char(char),
}

/// Hands `input` to the open screen (`MouseHandler.onPress`, `onScroll`, `KeyboardHandler.keyPress`,
/// `charTyped`), the pointer at `mouse` in GUI pixels and `modifiers` GLFW's bits. True when the
/// screen took it. Escape closes the screen through its own `keyPressed`, as vanilla's does.
pub fn input(rt: &mut Rt, mouse: [f32; 2], input: Input<'_>, modifiers: u32) -> bool {
    sync(rt);
    let s = current(rt);
    if s.r() == 0 {
        return false;
    }
    let (x, y) = (V::D(f64::from(mouse[0])), V::D(f64::from(mouse[1])));
    let m = V::I(modifiers as i32);
    let r = match input {
        Input::Press(b) => rt.call_functional(scr("mouseClicked.(DDI)Z"), s, &[x, y, V::I(b)]),
        Input::Release(b) => rt.call_functional(scr("mouseReleased.(DDI)Z"), s, &[x, y, V::I(b)]),
        Input::Drag(b, [dx, dy]) => rt.call_functional(scr("mouseDragged.(DDIDD)Z"), s, &[x, y, V::I(b), V::D(f64::from(dx)), V::D(f64::from(dy))]),
        Input::Scroll([sx, sy]) => rt.call_functional(scr("mouseScrolled.(DDDD)Z"), s, &[x, y, V::D(sx), V::D(sy)]),
        Input::Key(name, pressed) => {
            let code = key_code(name).unwrap_or(-1);
            let key = if pressed { scr("keyPressed.(III)Z") } else { scr("keyReleased.(III)Z") };
            rt.call_functional(key, s, &[V::I(code), V::I(0), m])
        }
        Input::Char(c) => rt.call_functional(scr("charTyped.(CI)Z"), s, &[V::I(c as i32), m]),
    };
    rt.report_thrown("a screen's input");
    truthy(r)
}

/// A key of `Screen`'s, by `name.desc`.
fn scr(rest: &'static str) -> &'static str {
    match rest {
        "mouseClicked.(DDI)Z" => "net/minecraft/client/gui/screens/Screen.mouseClicked.(DDI)Z",
        "mouseReleased.(DDI)Z" => "net/minecraft/client/gui/screens/Screen.mouseReleased.(DDI)Z",
        "mouseDragged.(DDIDD)Z" => "net/minecraft/client/gui/screens/Screen.mouseDragged.(DDIDD)Z",
        "mouseScrolled.(DDDD)Z" => "net/minecraft/client/gui/screens/Screen.mouseScrolled.(DDDD)Z",
        "keyPressed.(III)Z" => "net/minecraft/client/gui/screens/Screen.keyPressed.(III)Z",
        "keyReleased.(III)Z" => "net/minecraft/client/gui/screens/Screen.keyReleased.(III)Z",
        _ => "net/minecraft/client/gui/screens/Screen.charTyped.(CI)Z",
    }
}

/// `Screen.renderWithTooltip`'s second half: the tooltip a widget set for this frame.
fn deferred_tooltip(rt: &mut Rt, s: V, g: V, x: V, y: V) {
    let lines = rt.get_field(s, "$tooltip");
    if lines.r() == 0 {
        return;
    }
    rt.put_field(s, "$tooltip", V::NULL);
    let font = crate::gui::font(rt);
    crate::api::call(rt, TOOLTIP_SEQUENCES, &[g, font, lines, x, y]);
}

// ---- Screen ----

fn children(rt: &mut Rt, this: V) -> Vec<V> {
    let l = rt.call_functional(K_CHILDREN, this, &[]);
    crate::jutil::elements(rt, l)
}

/// A screen's method: a container screen's own first ([`crate::container_screens`]), then
/// `Screen`'s.
fn screen_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    if rt.instance_of(arg(args, 0), crate::container_screens::CONTAINER_SCREEN)
        && let Some(v) = crate::container_screens::own(rt, name, desc, args)
    {
        return Some(v);
    }
    screen_base(rt, name, desc, args)
}

/// Vanilla `Screen`'s own methods (and what it inherits from `AbstractContainerEventHandler`): what
/// a subclass's `super` reaches.
pub(crate) fn screen_base(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match (name, desc) {
        ("<init>", "(Lnet/minecraft/network/chat/Component;)V") => {
            rt.put_field(this, "title", a(1));
            ensure_lists(rt, this);
            V::Void
        }
        ("init", "()V") | ("tick", "()V") | ("removed", "()V") | ("added", "()V") | ("setInitialFocus", _) | ("clearFocus", "()V") => V::Void,
        ("renderBlurredBackground", _) | ("renderPanorama", _) | ("triggerImmediateNarration", _) | ("updateNarratorStatus", _) => V::Void,
        ("afterMouseMove" | "afterMouseAction" | "afterKeyboardAction" | "handleDelayedNarration" | "onFilesDrop", _) => V::Void,
        ("init", "(Lnet/minecraft/client/Minecraft;II)V") => {
            init_screen(rt, this, a(1), a(2).i(), a(3).i());
            V::Void
        }
        ("resize", "(Lnet/minecraft/client/Minecraft;II)V") => {
            rt.put_field(this, "width", a(2));
            rt.put_field(this, "height", a(3));
            rt.call_functional(K_REPOSITION, this, &[]);
            V::Void
        }
        ("repositionElements" | "rebuildWidgets", "()V") => {
            clear_widgets(rt, this);
            rt.call_functional(K_SET_FOCUSED, this, &[V::NULL]);
            rt.call_functional(K_INIT, this, &[]);
            V::Void
        }
        ("render", "(Lnet/minecraft/client/gui/GuiGraphics;IIF)V") => {
            let rest = &args[1..];
            rt.call_functional(K_RENDER_BACKGROUND, this, rest);
            let renderables = rt.get_field(this, "renderables");
            for r in crate::jutil::elements(rt, renderables) {
                rt.call_functional(RENDERABLE_RENDER, r, rest);
                if rt.thrown.is_some() {
                    return Some(V::Void);
                }
            }
            V::Void
        }
        ("renderWithTooltip", _) => {
            rt.call_functional(K_RENDER, this, &args[1..]);
            V::Void
        }
        // In a world: the world blurred (not here) under the in-world menu background.
        ("renderBackground", _) => {
            rt.call_functional(K_MENU_BACKGROUND, this, &[a(1)]);
            V::Void
        }
        ("renderMenuBackground", "(Lnet/minecraft/client/gui/GuiGraphics;)V") => {
            let (w, h) = (rt.get_field(this, "width"), rt.get_field(this, "height"));
            rt.call_functional(K_MENU_BACKGROUND_AT, this, &[a(1), V::I(0), V::I(0), w, h]);
            V::Void
        }
        ("renderMenuBackground", "(Lnet/minecraft/client/gui/GuiGraphics;IIII)V") => {
            let g = a(1);
            let texture = rt.string(MENU_BACKGROUND);
            menu_background(rt, g, texture, [a(2).i(), a(3).i()], [0.0, 0.0], [a(4).i(), a(5).i()]);
            V::Void
        }
        // static (graphics, texture, x, y, u, v, width, height)
        ("renderMenuBackgroundTexture", _) => {
            menu_background(rt, a(0), a(1), [a(2).i(), a(3).i()], [a(4).f(), a(5).f()], [a(6).i(), a(7).i()]);
            V::Void
        }
        ("renderTransparentBackground", _) => {
            let m = crate::gui::pose(rt, a(1));
            let (w, h) = (rt.get_field(this, "width").i(), rt.get_field(this, "height").i());
            crate::gui::gradient(rt, &m, [0, 0, w, h], -1072689136, -804253680);
            V::Void
        }
        ("children", "()Ljava/util/List;") => rt.get_field(this, "children"),
        ("getTitle" | "getNarrationMessage", _) => rt.get_field(this, "title"),
        ("getMinecraft", _) => rt.get_field(this, "minecraft"),
        ("isPauseScreen" | "shouldCloseOnEsc", "()Z") => V::I(1),
        ("isMouseOver", "(DD)Z") => V::I(1),
        // NeoForge's `popGuiLayer`: with no layers over it, the screen closes.
        ("onClose", "()V") => {
            set_screen(rt, V::NULL);
            V::Void
        }
        ("addRenderableWidget", _) => {
            for field in ["renderables", "children", "narratables"] {
                let l = rt.get_field(this, field);
                list_push(rt, l, a(1));
            }
            a(1)
        }
        ("addRenderableOnly", _) => {
            let l = rt.get_field(this, "renderables");
            list_push(rt, l, a(1));
            a(1)
        }
        ("addWidget", _) => {
            for field in ["children", "narratables"] {
                let l = rt.get_field(this, field);
                list_push(rt, l, a(1));
            }
            a(1)
        }
        ("removeWidget", _) => {
            for field in ["renderables", "children", "narratables"] {
                let l = rt.get_field(this, field);
                list_remove(rt, l, a(1));
            }
            V::Void
        }
        ("clearWidgets", "()V") => {
            clear_widgets(rt, this);
            V::Void
        }
        ("keyPressed", "(III)Z") => {
            if a(1).i() == 256 && truthy(rt.call_functional(K_CLOSE_ON_ESC, this, &[])) {
                rt.call_functional(K_ON_CLOSE, this, &[]);
                return Some(V::I(1));
            }
            return container_call(rt, name, desc, args);
        }
        ("hasShiftDown", "()Z") => V::I(held_either(rt, "shift") as i32),
        ("hasControlDown", "()Z") => V::I(held_either(rt, if cfg!(target_os = "macos") { "win" } else { "control" }) as i32),
        ("hasAltDown", "()Z") => V::I(held_either(rt, "alt") as i32),
        ("isCut" | "isPaste" | "isCopy" | "isSelectAll", "(I)Z") => {
            let letter = match name {
                "isCut" => 88,
                "isPaste" => 86,
                "isCopy" => 67,
                _ => 65,
            };
            let control = held_either(rt, if cfg!(target_os = "macos") { "win" } else { "control" });
            V::I((a(0).i() == letter && control && !held_either(rt, "shift") && !held_either(rt, "alt")) as i32)
        }
        // A tooltip drawn after the screen (`setTooltipForNextRenderPass`): lines of text.
        ("setTooltipForNextRenderPass", "(Ljava/util/List;)V") | ("setTooltipForNextRenderPass", "(Ljava/util/List;Lnet/minecraft/client/gui/screens/inventory/tooltip/ClientTooltipPositioner;Z)V") => {
            if rt.get_field(this, "$tooltip").r() == 0 || truthy(a(3)) {
                rt.put_field(this, "$tooltip", a(1));
            }
            V::Void
        }
        ("setTooltipForNextRenderPass", "(Lnet/minecraft/network/chat/Component;)V") => {
            let font = crate::gui::font(rt);
            let width = (rt.get_field(this, "width").i() / 2 - 12).max(170);
            let lines = crate::api::call(rt, "net/minecraft/client/gui/Font.split.(Lnet/minecraft/network/chat/FormattedText;I)Ljava/util/List;", &[font, a(1), V::I(width)]);
            rt.put_field(this, "$tooltip", lines);
            V::Void
        }
        ("clearTooltipForNextRenderPass", _) => {
            rt.put_field(this, "$tooltip", V::NULL);
            V::Void
        }
        _ => return container_call(rt, name, desc, args),
    })
}

fn clear_widgets(rt: &mut Rt, this: V) {
    for field in ["renderables", "children", "narratables"] {
        let l = rt.get_field(this, field);
        list_clear(rt, l);
    }
}

/// `renderMenuBackgroundTexture`: `texture` tiled 32 x 32 over `size` at `at`, from `uv`.
fn menu_background(rt: &mut Rt, g: V, texture: V, at: [i32; 2], uv: [f32; 2], size: [i32; 2]) {
    let m = crate::gui::pose(rt, g);
    let tex = rt.str_of(texture).unwrap_or("").to_owned();
    let [x, y] = at.map(|v| v as f32);
    let [w, h] = size.map(|v| v as f32);
    crate::gui::blit_at(rt, &m, &tex, [x, x + w, y, y + h], [uv[0] / 32.0, uv[1] / 32.0, (uv[0] + w) / 32.0, (uv[1] + h) / 32.0]);
}

/// `AbstractContainerEventHandler` and `ContainerEventHandler`'s defaults: input goes to the
/// focused child or the one under the pointer.
fn container_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match (name, desc) {
        ("getChildAt", "(DD)Ljava/util/Optional;") => {
            let hit = child_at(rt, this, a(1), a(2));
            crate::api_common::optional(rt, hit)
        }
        ("mouseClicked", "(DDI)Z") => {
            for c in children(rt, this) {
                if truthy(rt.call_functional(L_CLICKED, c, &args[1..])) {
                    rt.call_functional(K_SET_FOCUSED, this, &[c]);
                    if a(3).i() == 0 {
                        rt.call_functional(K_SET_DRAGGING, this, &[V::I(1)]);
                    }
                    return Some(V::I(1));
                }
            }
            V::I(0)
        }
        ("mouseReleased", "(DDI)Z") => {
            if a(3).i() == 0 && truthy(rt.call_functional(K_IS_DRAGGING, this, &[])) {
                rt.call_functional(K_SET_DRAGGING, this, &[V::I(0)]);
                let f = rt.call_functional(K_GET_FOCUSED, this, &[]);
                if f.r() != 0 {
                    return Some(rt.call_functional(L_RELEASED, f, &args[1..]));
                }
            }
            let hit = child_at(rt, this, a(1), a(2));
            V::I((hit.r() != 0 && truthy(rt.call_functional(L_RELEASED, hit, &args[1..]))) as i32)
        }
        ("mouseDragged", "(DDIDD)Z") => {
            let f = rt.call_functional(K_GET_FOCUSED, this, &[]);
            let dragging = truthy(rt.call_functional(K_IS_DRAGGING, this, &[]));
            V::I((f.r() != 0 && dragging && a(3).i() == 0 && truthy(rt.call_functional(L_DRAGGED, f, &args[1..]))) as i32)
        }
        ("mouseScrolled", "(DDDD)Z") => {
            let hit = child_at(rt, this, a(1), a(2));
            V::I((hit.r() != 0 && truthy(rt.call_functional(L_SCROLLED, hit, &args[1..]))) as i32)
        }
        ("keyPressed" | "keyReleased", "(III)Z") | ("charTyped", "(CI)Z") => {
            let f = rt.call_functional(K_GET_FOCUSED, this, &[]);
            let key = match name {
                "keyPressed" => L_KEY_PRESSED,
                "keyReleased" => L_KEY_RELEASED,
                _ => L_CHAR_TYPED,
            };
            V::I((f.r() != 0 && truthy(rt.call_functional(key, f, &args[1..]))) as i32)
        }
        ("mouseMoved", "(DD)V") => V::Void,
        ("isDragging", "()Z") => V::I(truthy(rt.get_field(this, "$dragging")) as i32),
        ("setDragging", "(Z)V") => {
            rt.put_field(this, "$dragging", a(1));
            V::Void
        }
        ("getFocused", _) => match rt.get_field(this, "$focused") {
            V::Void => V::NULL,
            v => v,
        },
        ("setFocused", "(Lnet/minecraft/client/gui/components/events/GuiEventListener;)V") => {
            let old = rt.get_field(this, "$focused");
            if old.r() != 0 {
                rt.call_functional(L_SET_FOCUSED, old, &[V::I(0)]);
            }
            if a(1).r() != 0 {
                rt.call_functional(L_SET_FOCUSED, a(1), &[V::I(1)]);
            }
            rt.put_field(this, "$focused", a(1));
            V::Void
        }
        ("setFocused", "(Z)V") => V::Void,
        ("isFocused", "()Z") => V::I((rt.call_functional(K_GET_FOCUSED, this, &[]).r() != 0) as i32),
        ("getCurrentFocusPath" | "nextFocusPath", _) => V::NULL,
        _ => return None,
    })
}

/// `getChildAt`: the first child under the pointer, or null.
fn child_at(rt: &mut Rt, this: V, x: V, y: V) -> V {
    for c in children(rt, this) {
        if truthy(rt.call_functional(L_OVER, c, &[x, y])) {
            return c;
        }
    }
    V::NULL
}

/// `GuiEventListener`'s defaults, for a listener that is neither a screen nor a widget.
fn listener_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = arg(args, 0);
    if let Some(v) = crate::widgets::own(rt, name, desc, args) {
        return Some(v);
    }
    if rt.instance_of(this, SCREEN) {
        return screen_call(rt, name, desc, args);
    }
    if rt.instance_of(this, ABSTRACT_CONTAINER) || rt.instance_of(this, CONTAINER) {
        return container_call(rt, name, desc, args);
    }
    Some(match (name, desc) {
        ("mouseMoved", "(DD)V") | ("setFocused", "(Z)V") => V::Void,
        ("mouseClicked" | "mouseReleased", "(DDI)Z")
        | ("mouseDragged", "(DDIDD)Z")
        | ("mouseScrolled", "(DDDD)Z")
        | ("keyPressed" | "keyReleased", "(III)Z")
        | ("charTyped", "(CI)Z")
        | ("isMouseOver", "(DD)Z")
        | ("isFocused", "()Z") => V::I(0),
        ("getCurrentFocusPath" | "nextFocusPath", _) => V::NULL,
        ("getTabOrderGroup", "()I") => V::I(0),
        _ => return None,
    })
}

// ---- keys ----

/// Vanilla's names for GLFW's key codes (`InputConstants.Type.KEYSYM`), past the letters, digits,
/// F keys and keypad digits [`key_name`] spells out.
const KEY_NAMES: [(i32, &str); 52] = [
    (32, "space"),
    (39, "apostrophe"),
    (44, "comma"),
    (45, "minus"),
    (46, "period"),
    (47, "slash"),
    (59, "semicolon"),
    (61, "equal"),
    (91, "left.bracket"),
    (92, "backslash"),
    (93, "right.bracket"),
    (96, "grave.accent"),
    (161, "world.1"),
    (162, "world.2"),
    (256, "escape"),
    (257, "enter"),
    (258, "tab"),
    (259, "backspace"),
    (260, "insert"),
    (261, "delete"),
    (262, "right"),
    (263, "left"),
    (264, "down"),
    (265, "up"),
    (266, "page.up"),
    (267, "page.down"),
    (268, "home"),
    (269, "end"),
    (280, "caps.lock"),
    (281, "scroll.lock"),
    (282, "num.lock"),
    (283, "print.screen"),
    (284, "pause"),
    (330, "keypad.decimal"),
    (331, "keypad.divide"),
    (332, "keypad.multiply"),
    (333, "keypad.subtract"),
    (334, "keypad.add"),
    (335, "keypad.enter"),
    (336, "keypad.equal"),
    (340, "left.shift"),
    (341, "left.control"),
    (342, "left.alt"),
    (343, "left.win"),
    (344, "right.shift"),
    (345, "right.control"),
    (346, "right.alt"),
    (347, "right.win"),
    (348, "menu"),
    (-1, "unknown"),
    (0x7FFF_FFFF, "unused"),
    (0x7FFF_FFFE, "unused2"),
];

/// Vanilla's name for GLFW key `code` (`key.keyboard.e`).
pub fn key_name(code: i32) -> Option<String> {
    let short = match code {
        48..=57 => ((b'0' + (code - 48) as u8) as char).to_string(),
        65..=90 => ((b'a' + (code - 65) as u8) as char).to_string(),
        290..=314 => format!("f{}", code - 289),
        320..=329 => format!("keypad.{}", code - 320),
        _ => KEY_NAMES.iter().take(49).find(|(c, _)| *c == code)?.1.to_owned(),
    };
    Some(format!("key.keyboard.{short}"))
}

/// GLFW's code for key `name` (`key.keyboard.e` is 69), the inverse of [`key_name`].
pub fn key_code(name: &str) -> Option<i32> {
    let short = name.strip_prefix("key.keyboard.")?;
    let b = short.as_bytes();
    if b.len() == 1 && b[0].is_ascii_lowercase() {
        return Some(i32::from(b[0] - b'a') + 65);
    }
    if b.len() == 1 && b[0].is_ascii_digit() {
        return Some(i32::from(b[0] - b'0') + 48);
    }
    if let Some(n) = short.strip_prefix('f').and_then(|n| n.parse::<i32>().ok()) {
        return (1..=25).contains(&n).then_some(n + 289);
    }
    if let Some(n) = short.strip_prefix("keypad.").and_then(|n| n.parse::<i32>().ok()) {
        return (0..=9).contains(&n).then_some(n + 320);
    }
    KEY_NAMES.iter().take(49).find(|(_, n)| *n == short).map(|(c, _)| *c)
}

/// The mouse button key `name` is (`key.mouse.right` is 1), GLFW's numbering.
fn mouse_button(name: &str) -> Option<i32> {
    match name.strip_prefix("key.mouse.")? {
        "left" => Some(0),
        "right" => Some(1),
        "middle" => Some(2),
        n => n.parse::<i32>().ok().filter(|n| *n >= 1).map(|n| n - 1),
    }
}

pub(crate) fn mouse_name(button: i32) -> String {
    match button {
        0 => "key.mouse.left".into(),
        1 => "key.mouse.right".into(),
        2 => "key.mouse.middle".into(),
        n => format!("key.mouse.{}", n + 1),
    }
}

/// Whether the left or right `which` key (`shift`, `control`, `alt`, `win`) is held.
pub(crate) fn held_either(rt: &Rt, which: &str) -> bool {
    rt.host().is_some_and(|h| h.key_held(&format!("key.keyboard.left.{which}")) || h.key_held(&format!("key.keyboard.right.{which}")))
}

/// `Options`' key mappings a mod reads (`options.keyUse`), by field and vanilla's name.
const OPTION_KEYS: [(&str, &str); 26] = [
    ("keyUp", "key.forward"),
    ("keyLeft", "key.left"),
    ("keyDown", "key.back"),
    ("keyRight", "key.right"),
    ("keyJump", "key.jump"),
    ("keyShift", "key.sneak"),
    ("keySprint", "key.sprint"),
    ("keyInventory", "key.inventory"),
    ("keySwapOffhand", "key.swapOffhand"),
    ("keyDrop", "key.drop"),
    ("keyUse", "key.use"),
    ("keyAttack", "key.attack"),
    ("keyPickItem", "key.pickItem"),
    ("keyChat", "key.chat"),
    ("keyPlayerList", "key.playerlist"),
    ("keyCommand", "key.command"),
    ("keySocialInteractions", "key.socialInteractions"),
    ("keyScreenshot", "key.screenshot"),
    ("keyTogglePerspective", "key.togglePerspective"),
    ("keySmoothCamera", "key.smoothCamera"),
    ("keyFullscreen", "key.fullscreen"),
    ("keySpectatorOutlines", "key.spectatorOutlines"),
    ("keyAdvancements", "key.advancements"),
    ("keySaveHotbarActivator", "key.saveToolbarActivator"),
    ("keyLoadHotbarActivator", "key.loadToolbarActivator"),
    ("keyHotbarSlots", ""),
];

/// The key mapping named `name` (`key.use`): one object each.
pub fn key_mapping(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("compat:key-mapping/{name}"), move |rt| {
        let k = rt.new_object(KEY_MAPPING);
        let s = rt.string(&n);
        rt.put_field(k, "$name", s);
        k
    })
}

/// `Options`' key mapping fields (`keyUse`, `keyHotbarSlots`).
pub fn fill_options(rt: &mut Rt, options: V) {
    for (field, name) in OPTION_KEYS {
        let v = if name.is_empty() {
            let slots: Vec<V> = (1..=9).map(|i| key_mapping(rt, &format!("key.hotbar.{i}"))).collect();
            rt.alloc("[Lnet/minecraft/client/KeyMapping;", Data::Array(slots))
        } else {
            key_mapping(rt, name)
        };
        rt.put_field(options, field, v);
    }
}

/// The key bound to mapping `name` (`key.use`) and whether it is held, as the host says.
pub(crate) fn mapping_binding(rt: &Rt, name: &str) -> (String, bool) {
    rt.host().and_then(|h| h.mapping(name)).unwrap_or_else(|| ("key.keyboard.unknown".into(), false))
}

/// What key `key` (`key.mouse.right`) is called on screen (`InputConstants.Key.getDisplayName`):
/// a character key its character, upper-cased (GLFW's key name), others their translation
/// (`Right Button`, `Left Shift`); a mouse button the game has no name for `Button <n>`.
pub fn key_display(rt: &Rt, key: &str) -> String {
    let translate = |k: &str| rt.host().and_then(|h| h.translate(k));
    if let Some(c) = key.strip_prefix("key.keyboard.").filter(|c| c.chars().count() == 1) {
        return c.to_uppercase();
    }
    if let Some(t) = translate(key) {
        return t;
    }
    match key.strip_prefix("key.mouse.").and_then(|n| n.parse::<u32>().ok()) {
        Some(n) => translate("key.mouse").map_or_else(|| format!("Button {n}"), |t| t.replace("%s", &n.to_string()).replace("%1$s", &n.to_string())),
        None => key.to_owned(),
    }
}

/// What mapping `name` (`key.use`, a mod's `key.create.toolmenu`) shows as (`Component.keybind`,
/// `KeyMapping.getTranslatedKeyMessage`): the key bound to it, by [`key_display`], with a mod
/// mapping's modifier.
pub fn mapping_display(rt: &Rt, name: &str) -> String {
    match crate::keys::mod_mapping(rt, name) {
        Some(k) => crate::keys::display(rt, k),
        None => key_display(rt, &mapping_binding(rt, name).0),
    }
}

/// An `InputConstants.Key` for key `name` (`key.mouse.right`).
pub(crate) fn input_key(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("compat:input-key/{name}"), move |rt| {
        let k = rt.new_object(INPUT_KEY);
        let s = rt.string(&n);
        rt.put_field(k, "$name", s);
        let (ty, value) = match mouse_button(&n) {
            Some(b) => (2, b),
            None => (0, key_code(&n).unwrap_or(-1)),
        };
        rt.put_field(k, "$type", V::I(ty));
        rt.put_field(k, "$value", V::I(value));
        k
    })
}

fn input_type(rt: &mut Rt, ordinal: i32) -> V {
    const TYPES: [&str; 3] = ["KEYSYM", "SCANCODE", "MOUSE"];
    let i = ordinal.clamp(0, 2) as usize;
    crate::api::api_enum(rt, INPUT_TYPE, TYPES[i], i as i32)
}

fn key_calls(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        (INPUT, "isKeyDown") if desc == "(JI)Z" => {
            let code = a(1).i();
            let held = key_name(code).is_some_and(|n| rt.host().is_some_and(|h| h.key_held(&n)));
            V::I(held as i32)
        }
        (INPUT, "getKey") if desc == "(II)Lcom/mojang/blaze3d/platform/InputConstants$Key;" => {
            let n = key_name(a(0).i()).unwrap_or_else(|| "key.keyboard.unknown".into());
            input_key(rt, &n)
        }
        (INPUT, "getKey") => {
            let n = rt.str_of(a(0)).unwrap_or("key.keyboard.unknown").to_owned();
            input_key(rt, &n)
        }
        (INPUT_TYPE, "getOrCreate") => {
            let n = match rt.get_field(a(0), "$ordinal").i() {
                2 => mouse_name(a(1).i()),
                _ => key_name(a(1).i()).unwrap_or_else(|| "key.keyboard.unknown".into()),
            };
            input_key(rt, &n)
        }
        (INPUT_KEY, "getValue") => rt.get_field(a(0), "$value"),
        (INPUT_KEY, "getName" | "toString") => rt.get_field(a(0), "$name"),
        (INPUT_KEY, "getType") => {
            let t = rt.get_field(a(0), "$type").i();
            input_type(rt, t)
        }
        (INPUT_KEY, "getDisplayName") => {
            let n = rt.str_of(rt.get_field(a(0), "$name")).unwrap_or("").to_owned();
            let shown = key_display(rt, &n);
            let s = rt.string(&shown);
            crate::api_common::component(rt, s, false, V::NULL)
        }
        (GLFW, "glfwSetCursorPos") => {
            let scale = rt.host().map_or(1.0, |h| h.gui_scale()).max(f64::MIN_POSITIVE);
            set_cursor(rt, [(a(1).d() / scale) as f32, (a(2).d() / scale) as f32]);
            V::Void
        }
        (GLFW, "glfwGetKey") => {
            let held = key_name(a(1).i()).is_some_and(|n| rt.host().is_some_and(|h| h.key_held(&n)));
            V::I(held as i32)
        }
        (GLFW, "glfwGetMouseButton") => V::I(rt.host().is_some_and(|h| h.key_held(&mouse_name(a(1).i()))) as i32),
        _ => return None,
    })
}

// ---- Minecraft ----

fn client_calls(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "setScreen" => {
            set_screen(rt, a(1));
            V::Void
        }
        // NeoForge's screen layers: a layer over the screen is the screen here.
        "pushGuiLayer" => {
            set_screen(rt, a(1));
            V::Void
        }
        "popGuiLayer" => {
            set_screen(rt, V::NULL);
            V::Void
        }
        // The client thread's queue (`BlockableEventLoop.tell`, `execute`): run now, on it.
        "tell" | "execute" | "submit" => {
            rt.call_functional(RUN, a(1), &[]);
            rt.report_thrown("a task for the client thread");
            if name == "submit" { crate::jutil::completed_future(rt, V::NULL) } else { V::Void }
        }
        "isSameThread" => V::I(1),
        _ => return None,
    })
}

// ---- GUI geometry: Tesselator, BufferBuilder, BufferUploader ----

/// `Tesselator.begin(mode, format)`: a builder collecting vertices (position, texture
/// coordinates, colour) for `mode`'s primitives; textured when its format has UVs.
fn begin(rt: &mut Rt, mode: V, format: V) -> V {
    let b = rt.new_object(BUFFER_BUILDER);
    let m = rt.get_field(mode, "$ordinal");
    rt.put_field(b, "$mode", m);
    let textured = rt.str_of(rt.get_field(format, "$name")).is_some_and(|n| n.contains("TEX"));
    rt.put_field(b, "$textured", V::I(textured as i32));
    let verts = crate::jutil::new_list(rt, Vec::new());
    rt.put_field(b, "$verts", verts);
    b
}

/// Ints a vertex keeps in its builder's list: x, y, u, v, colour.
const VERTEX: usize = 5;

fn push_vertex(rt: &mut Rt, b: V, x: f32, y: f32) {
    let list = rt.get_field(b, "$verts");
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        l.extend([V::F(x), V::F(y), V::F(0.0), V::F(0.0), V::I(-1)]);
    }
}

/// Sets field `at` (2 the texture's u, 4 the colour) of the builder's last vertex.
fn set_last(rt: &mut Rt, b: V, at: usize, values: &[V]) {
    let list = rt.get_field(b, "$verts");
    if let Some(Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data)
        && l.len() >= VERTEX
    {
        let start = l.len() - VERTEX + at;
        for (i, v) in values.iter().enumerate() {
            if let Some(slot) = l.get_mut(start + i) {
                *slot = *v;
            }
        }
    }
}

fn argb_of(r: i32, g: i32, b: i32, a: i32) -> i32 {
    let c = |v: i32| v.clamp(0, 255) as u32;
    (c(a) << 24 | c(r) << 16 | c(g) << 8 | c(b)) as i32
}

/// What a vertex consumer that is a builder of ours does (`addVertex(...).setColor(...).setUv(...)`).
pub fn vertex_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let b = a(0);
    if rt.class_of(b) != BUFFER_BUILDER {
        return None;
    }
    match (name, desc) {
        ("addVertex", "(Lorg/joml/Matrix4f;FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;") => {
            let m = render::mat4(rt, a(1));
            let (x, y) = (a(2).f(), a(3).f());
            push_vertex(rt, b, m[0] * x + m[4] * y + m[12], m[1] * x + m[5] * y + m[13]);
        }
        ("addVertex", "(FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;") => push_vertex(rt, b, a(1).f(), a(2).f()),
        ("addVertex", d) if d.starts_with("(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;FFF)") => {
            let m = render::mat4(rt, rt.get_field(a(1), "pose"));
            let (x, y) = (a(2).f(), a(3).f());
            push_vertex(rt, b, m[0] * x + m[4] * y + m[12], m[1] * x + m[5] * y + m[13]);
        }
        ("setColor", "(IIII)Lcom/mojang/blaze3d/vertex/VertexConsumer;") => {
            let c = argb_of(a(1).i(), a(2).i(), a(3).i(), a(4).i());
            set_last(rt, b, 4, &[V::I(c)]);
        }
        ("setColor", "(FFFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;") => {
            let f = |v: V| (v.f() * 255.0).round() as i32;
            let c = argb_of(f(a(1)), f(a(2)), f(a(3)), f(a(4)));
            set_last(rt, b, 4, &[V::I(c)]);
        }
        ("setColor", "(I)Lcom/mojang/blaze3d/vertex/VertexConsumer;") => set_last(rt, b, 4, &[a(1)]),
        ("setUv", "(FF)Lcom/mojang/blaze3d/vertex/VertexConsumer;") => set_last(rt, b, 2, &[a(1), a(2)]),
        // Light, overlay and normals mean nothing to a GUI quad.
        _ if desc.ends_with(")Lcom/mojang/blaze3d/vertex/VertexConsumer;") => {}
        ("buildOrThrow" | "build", _) => {
            let mesh = rt.new_object(MESH);
            rt.put_field(mesh, "$builder", b);
            return Some(mesh);
        }
        _ => return None,
    }
    Some(b)
}

/// `BufferUploader.drawWithShader(mesh)`: the builder's quads (or triangles, strips, fans) drawn
/// on the GUI, textured ones from the shader's texture.
fn draw_mesh(rt: &mut Rt, mesh: V) {
    let b = rt.get_field(mesh, "$builder");
    if b.r() == 0 || !crate::gui::drawing(rt) {
        return;
    }
    let mode = rt.get_field(b, "$mode").i();
    let textured = truthy(rt.get_field(b, "$textured"));
    let list = rt.get_field(b, "$verts");
    let flat = crate::jutil::elements(rt, list);
    let verts: Vec<([f32; 2], [f32; 2], u32)> = flat
        .chunks_exact(VERTEX)
        .map(|v| ([v[0].f(), v[1].f()], [v[2].f(), v[3].f()], v[4].i() as u32))
        .collect();
    // Quads as GL draws them: QUADS 4 at a time; TRIANGLES 3; strips and fans as triangles.
    let mut shapes: Vec<Vec<usize>> = Vec::new();
    match mode {
        7 => shapes.extend((0..verts.len() / 4).map(|q| (q * 4..q * 4 + 4).collect())),
        4 => shapes.extend((0..verts.len() / 3).map(|t| vec![t * 3, t * 3 + 1, t * 3 + 2])),
        5 => shapes.extend((2..verts.len()).map(|i| vec![i - 2, i - 1, i])),
        6 => shapes.extend((2..verts.len()).map(|i| vec![0, i - 1, i])),
        _ => {}
    }
    let texture = rt.interned.get(SHADER_TEXTURE).and_then(|v| rt.str_of(*v)).unwrap_or("").to_owned();
    for s in shapes {
        let corners: Vec<([f32; 2], [f32; 2], u32)> = s.iter().map(|&i| verts[i]).collect();
        if textured && !texture.is_empty() {
            textured_shape(rt, &texture, &corners);
        } else {
            coloured_shape(rt, &corners);
        }
    }
}

/// A textured quad or triangle: blitted over its bounds with its texture coordinates' range
/// (the axis-aligned rectangles GUI code draws), tinted by its first vertex's colour.
fn textured_shape(rt: &mut Rt, texture: &str, corners: &[([f32; 2], [f32; 2], u32)]) {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for (p, _, _) in corners {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    // The texture corner at each rectangle corner: the vertex nearest it.
    let uv_at = |x: f32, y: f32| {
        corners
            .iter()
            .min_by(|a, b| {
                let d = |p: [f32; 2]| (p[0] - x).powi(2) + (p[1] - y).powi(2);
                d(a.0).total_cmp(&d(b.0))
            })
            .map_or([0.0, 0.0], |c| c.1)
    };
    let (top_left, bottom_right) = (uv_at(lo[0], lo[1]), uv_at(hi[0], hi[1]));
    let tint = corners[0].2;
    crate::gui::blit_tinted(rt, texture, [lo[0], lo[1], hi[0], hi[1]], [top_left[0], top_left[1], bottom_right[0], bottom_right[1]], tint);
}

/// A coloured quad or triangle (a triangle as a quad with its last corner twice); vertices of
/// different colours give their average.
fn coloured_shape(rt: &mut Rt, corners: &[([f32; 2], [f32; 2], u32)]) {
    let mut q = [[0.0f32; 2]; 4];
    for (i, slot) in q.iter_mut().enumerate() {
        *slot = corners[i.min(corners.len() - 1)].0;
    }
    let n = corners.len() as u32;
    let channel = |shift: u32| corners.iter().map(|c| (c.2 >> shift) & 0xFF).sum::<u32>() / n << shift;
    let argb = channel(24) | channel(16) | channel(8) | channel(0);
    if let Some(h) = rt.host_mut() {
        h.gui_fill(q, argb);
    }
}

fn geometry_calls(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        (TESSELATOR, "getInstance") | ("com/mojang/blaze3d/systems/RenderSystem", "renderThreadTesselator") => {
            rt.intern("compat:tesselator".into(), |rt| rt.new_object(TESSELATOR))
        }
        (TESSELATOR, "begin") => begin(rt, a(1), a(2)),
        (TESSELATOR, "clear") => V::Void,
        (BUFFER_BUILDER, _) => return vertex_call(rt, name, desc, args),
        (MESH, "close") => V::Void,
        (UPLOADER, "drawWithShader" | "draw") => {
            draw_mesh(rt, a(0));
            V::Void
        }
        (UPLOADER, "reset" | "invalidate") => V::Void,
        ("com/mojang/blaze3d/systems/RenderSystem", "setShaderTexture") if desc == "(ILnet/minecraft/resources/ResourceLocation;)V" => {
            if a(0).i() == 0 {
                rt.interned.insert(SHADER_TEXTURE.into(), a(1));
            }
            V::Void
        }
        _ => return None,
    })
}

// ---- the API ----

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        SCREEN
            | CONTAINER
            | ABSTRACT_CONTAINER
            | LISTENER
            | KEY_MAPPING
            | crate::keys::REGISTER_EVENT
            | crate::keys::MODIFIER
            | crate::keys::CONTEXT
            | crate::keys::ICONTEXT
            | crate::input::KEY
            | crate::input::BUTTON
            | crate::input::BUTTON_PRE
            | crate::input::BUTTON_POST
            | crate::input::SCROLLING
            | crate::input::INTERACTION
            | INPUT
            | INPUT_KEY
            | INPUT_TYPE
            | GLFW
            | TESSELATOR
            | BUFFER_BUILDER
            | MESH
            | UPLOADER
            | "com/mojang/blaze3d/vertex/VertexConsumer"
            | "com/mojang/blaze3d/systems/RenderSystem"
    ) || (owner == MINECRAFT && matches!(name, "setScreen" | "pushGuiLayer" | "popGuiLayer" | "tell" | "execute" | "submit" | "isSameThread"))
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        SCREEN => screen_call(rt, name, desc, args),
        CONTAINER | ABSTRACT_CONTAINER => {
            if rt.instance_of(arg(args, 0), SCREEN) {
                screen_call(rt, name, desc, args)
            } else {
                container_call(rt, name, desc, args)
            }
        }
        LISTENER => listener_call(rt, name, desc, args),
        MINECRAFT => client_calls(rt, name, args),
        "com/mojang/blaze3d/vertex/VertexConsumer" => vertex_call(rt, name, desc, args),
        KEY_MAPPING | crate::keys::REGISTER_EVENT | crate::keys::MODIFIER | crate::keys::CONTEXT | crate::keys::ICONTEXT => {
            crate::keys::call(rt, owner, name, desc, args)
        }
        crate::input::KEY
        | crate::input::BUTTON
        | crate::input::BUTTON_PRE
        | crate::input::BUTTON_POST
        | crate::input::SCROLLING
        | crate::input::INTERACTION => crate::input::call(rt, owner, name, args),
        INPUT | INPUT_KEY | INPUT_TYPE | GLFW => key_calls(rt, owner, name, desc, args),
        _ => geometry_calls(rt, owner, name, desc, args),
    }
}

/// Static fields this module owns.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if let Some(v) = crate::keys::get_static(rt, owner, field) {
        return Some(v);
    }
    Some(match (owner, field) {
        (INPUT, "UNKNOWN") => input_key(rt, "key.keyboard.unknown"),
        (INPUT_TYPE, f) => {
            let i = ["KEYSYM", "SCANCODE", "MOUSE"].iter().position(|n| *n == f)?;
            input_type(rt, i as i32)
        }
        (MODE, f) => {
            const MODES: [&str; 8] = ["LINES", "LINE_STRIP", "DEBUG_LINES", "DEBUG_LINE_STRIP", "TRIANGLES", "TRIANGLE_STRIP", "TRIANGLE_FAN", "QUADS"];
            let i = MODES.iter().position(|n| *n == f)?;
            crate::api::api_enum(rt, MODE, MODES[i], i as i32)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{GuiDraw, MemoryHost};

    const TEST_SCREEN: &str = "test/ValueScreen";

    /// The test screen (`test/ValueScreen`): `init` adds nothing and counts; `mouseReleased`
    /// with the use key's button sends a payload and closes, as Create's value settings do;
    /// `render` draws the background, then a fill where the pointer is.
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let this = args[0];
        if rt.class_of(this) == TEST_SCREEN {
            match key {
                "init.()V" => {
                    let n = rt.get_field(this, "inits").i();
                    rt.put_field(this, "inits", V::I(n + 1));
                    return V::Void;
                }
                "tick.()V" => {
                    let n = rt.get_field(this, "ticks").i();
                    rt.put_field(this, "ticks", V::I(n + 1));
                    return V::Void;
                }
                "removed.()V" => {
                    rt.put_field(this, "gone", V::I(1));
                    return V::Void;
                }
                "mouseReleased.(DDI)Z" => {
                    let mc = minecraft(rt);
                    let options = rt.get_field(mc, "options");
                    let use_key = rt.get_field(options, "keyUse");
                    let matches = crate::api::call(rt, "net/minecraft/client/KeyMapping.matchesMouse.(I)Z", &[use_key, args[3]]);
                    if truthy(matches) {
                        rt.put_field(this, "released_at", args[1]);
                        crate::network::send_raw(rt, PayloadTarget::Server, "test:value", &[args[1].d() as u8]);
                        // A click as it confirms (`SimpleSoundInstance.forUI`, the sound manager).
                        let click = crate::api::get_static(rt, "net/minecraft/sounds/SoundEvents.UI_BUTTON_CLICK");
                        let sound = crate::api::call(rt, "net/minecraft/client/resources/sounds/SimpleSoundInstance.forUI.(Lnet/minecraft/core/Holder;F)Lnet/minecraft/client/resources/sounds/SimpleSoundInstance;", &[click, V::F(1.5)]);
                        let manager = crate::api::call(rt, "net/minecraft/client/Minecraft.getSoundManager.()Lnet/minecraft/client/sounds/SoundManager;", &[mc]);
                        crate::api::call(rt, "net/minecraft/client/sounds/SoundManager.play.(Lnet/minecraft/client/resources/sounds/SoundInstance;)V", &[manager, sound]);
                        rt.call_functional(K_ON_CLOSE, this, &[]);
                        return V::I(1);
                    }
                    return crate::api::call(rt, owner_key, args);
                }
                _ => {}
            }
        }
        crate::api::call(rt, owner_key, args)
    }

    fn rt_with_screen() -> (Rt, V) {
        crate::install();
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.supers.insert(TEST_SCREEN, SCREEN);
        let s = rt.new_object(TEST_SCREEN);
        let title = rt.string("Speed");
        crate::api::call(&mut rt, "net/minecraft/client/gui/screens/Screen.<init>.(Lnet/minecraft/network/chat/Component;)V", &[s, title]);
        (rt, s)
    }

    fn host() -> MemoryHost {
        let mut h = MemoryHost { client: true, gui: [427, 240], ..Default::default() };
        h.mappings.insert("key.use".into(), ("key.mouse.right".into(), true));
        h
    }

    #[test]
    fn a_screen_set_from_the_tick_opens_on_the_game_initialised_for_the_gui_and_ticks() {
        let (mut rt, s) = rt_with_screen();
        let mut h = host();
        rt.with_host(&mut h, |rt| {
            let mc = minecraft(rt);
            crate::api::call(rt, "net/minecraft/client/Minecraft.setScreen.(Lnet/minecraft/client/gui/screens/Screen;)V", &[mc, s]);
        });
        assert!(h.screen);
        assert_eq!(rt.get_field(s, "inits"), V::I(1));
        assert_eq!((rt.get_field(s, "width"), rt.get_field(s, "height")), (V::I(427), V::I(240)));
        let mc = minecraft(&mut rt);
        assert_eq!(rt.get_field(mc, "screen"), s);
        rt.with_host(&mut h, |rt| tick(rt));
        rt.with_host(&mut h, |rt| tick(rt));
        assert_eq!(rt.get_field(s, "ticks"), V::I(2));
    }

    #[test]
    fn releasing_the_use_button_over_a_screen_sends_its_payload_and_closes_it_at_the_next_tick() {
        let (mut rt, s) = rt_with_screen();
        let mut h = host();
        h.place = Some(([0.5, 64.0, 2.5], 0.0, 0.0));
        rt.with_host(&mut h, |rt| set_screen(rt, s));
        // The screen's input comes with a read-only host: nothing leaves yet.
        h.read_only = true;
        let took = rt.with_host(&mut h, |rt| input(rt, [100.0, 50.0], Input::Release(1), 0));
        assert!(took);
        assert_eq!(rt.get_field(s, "released_at"), V::D(100.0));
        assert_eq!(rt.get_field(s, "gone"), V::I(1));
        assert!(h.payloads.is_empty());
        assert!(h.sounds.is_empty());
        assert!(h.screen, "still open on the game until the tick");
        h.read_only = false;
        rt.with_host(&mut h, |rt| tick(rt));
        assert_eq!(h.payloads, vec![(PayloadTarget::Server, "test:value".to_owned(), vec![100])]);
        // The click plays then, at the listener (the player's eyes), at a UI sound's volume.
        let click = Sound { event: "minecraft:ui.button.click".into(), at: [0.5, 65.62, 2.5], source: 0, volume: 0.25, pitch: 1.5 };
        assert_eq!(h.sounds, vec![click]);
        assert!(!h.screen);
        assert_eq!(current(&rt), V::NULL);
    }

    /// `Minecraft.keyboardHandler`'s clipboard: read at once, written at once from the tick, and
    /// at the next tick when a screen's input writes it (the game then only lends itself to read).
    #[test]
    fn the_keyboard_handler_reads_the_clipboard_and_writes_it_when_the_game_can() {
        const GET: &str = "net/minecraft/client/KeyboardHandler.getClipboard.()Ljava/lang/String;";
        const SET: &str = "net/minecraft/client/KeyboardHandler.setClipboard.(Ljava/lang/String;)V";
        let (mut rt, _) = rt_with_screen();
        let mut h = host();
        h.clipboard = "from the system".into();
        let set = |rt: &mut Rt, text: &str| {
            let mc = minecraft(rt);
            let keyboard = rt.get_field(mc, "keyboardHandler");
            let s = rt.string(text);
            crate::api::call(rt, SET, &[keyboard, s]);
        };
        let read = rt.with_host(&mut h, |rt| {
            let mc = minecraft(rt);
            let keyboard = rt.get_field(mc, "keyboardHandler");
            let v = crate::api::call(rt, GET, &[keyboard]);
            rt.str_of(v).unwrap_or("").to_owned()
        });
        assert_eq!(read, "from the system");
        h.read_only = true;
        rt.with_host(&mut h, |rt| set(rt, "copied in a screen"));
        assert_eq!(h.clipboard, "from the system", "not while the game only reads");
        h.read_only = false;
        rt.with_host(&mut h, |rt| tick(rt));
        assert_eq!(h.clipboard, "copied in a screen");
        rt.with_host(&mut h, |rt| set(rt, "copied in a tick"));
        assert_eq!(h.clipboard, "copied in a tick");
    }

    /// Keys show as vanilla's Controls screen names them: a mapping by the key bound to it, a
    /// character key by its character, a mouse button by its translation or its number.
    #[test]
    fn keys_and_mappings_show_by_the_names_the_controls_screen_gives() {
        let (mut rt, _) = rt_with_screen();
        let mut h = host();
        h.lang.insert("key.mouse.right".into(), "Right Button".into());
        h.lang.insert("key.mouse".into(), "Button %1$s".into());
        h.mappings.insert("key.inventory".into(), ("key.keyboard.e".into(), false));
        rt.with_host(&mut h, |rt| {
            assert_eq!(mapping_display(rt, "key.use"), "Right Button");
            assert_eq!(mapping_display(rt, "key.inventory"), "E");
            assert_eq!(key_display(rt, "key.mouse.4"), "Button 4");
            // `Component.keybind` in a translation's argument, as Create's "Release %1$s to Confirm".
            let key = rt.string("key.use");
            let keybind = crate::api::call(rt, "net/minecraft/network/chat/Component.keybind.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[key]);
            let runs = crate::gui::runs(rt, keybind);
            assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "Right Button");
        });
    }

    /// A mod's own mapping, which the game's controls don't list, answers on the key it was made
    /// with: Create's tool menu on Left Alt (NeoForge's constructor with a conflict context), a
    /// mouse button's by vanilla's, an unbound one on none.
    #[test]
    fn a_mods_own_key_mapping_answers_on_the_key_it_was_made_with() {
        let (mut rt, _) = rt_with_screen();
        let mut h = host();
        h.keys_held.push("key.keyboard.left.alt".into());
        rt.with_host(&mut h, |rt| {
            let made = |rt: &mut Rt, desc: &str, args: &[V]| {
                let k = rt.new_object(KEY_MAPPING);
                let key = Box::leak(format!("net/minecraft/client/KeyMapping.<init>.{desc}").into_boxed_str());
                crate::api::call(rt, key, &[&[k], args].concat());
                k
            };
            let call = |rt: &mut Rt, k: V, key: &'static str| crate::api::call(rt, key, &[k]);
            let (name, category) = (rt.string("key.create.toolmenu"), rt.string("Create"));
            let keysym = input_type(rt, 0);
            let tool_menu = made(
                rt,
                "(Ljava/lang/String;Lnet/neoforged/neoforge/client/settings/IKeyConflictContext;Lcom/mojang/blaze3d/platform/InputConstants$Type;ILjava/lang/String;)V",
                &[name, V::NULL, keysym, V::I(342), category],
            );
            assert_eq!(call(rt, tool_menu, "net/minecraft/client/KeyMapping.isDown.()Z"), V::I(1));
            let shown = call(rt, tool_menu, "net/minecraft/client/KeyMapping.getCategory.()Ljava/lang/String;");
            assert_eq!(rt.str_of(shown), Some("Create"));
            let key = call(rt, tool_menu, "net/minecraft/client/KeyMapping.getKey.()Lcom/mojang/blaze3d/platform/InputConstants$Key;");
            assert_eq!(rt.str_of(rt.get_field(key, "$name")), Some("key.keyboard.left.alt"));

            let (name, mouse) = (rt.string("key.mod.zoom"), input_type(rt, 2));
            let zoom = made(rt, "(Ljava/lang/String;Lcom/mojang/blaze3d/platform/InputConstants$Type;ILjava/lang/String;)V", &[name, mouse, V::I(4), category]);
            let key = call(rt, zoom, "net/minecraft/client/KeyMapping.getKey.()Lcom/mojang/blaze3d/platform/InputConstants$Key;");
            assert_eq!(rt.str_of(rt.get_field(key, "$name")), Some("key.mouse.5"));
            assert_eq!(call(rt, zoom, "net/minecraft/client/KeyMapping.isDown.()Z"), V::I(0));

            let name = rt.string("key.mod.unbound");
            let unbound = made(rt, "(Ljava/lang/String;ILjava/lang/String;)V", &[name, V::I(-1), category]);
            assert_eq!(call(rt, unbound, "net/minecraft/client/KeyMapping.isUnbound.()Z"), V::I(1));
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    #[test]
    fn escape_closes_a_screen_through_its_own_key_handler_and_the_game_closing_it_lets_it_go() {
        let (mut rt, s) = rt_with_screen();
        let mut h = host();
        rt.with_host(&mut h, |rt| set_screen(rt, s));
        let took = rt.with_host(&mut h, |rt| input(rt, [0.0, 0.0], Input::Key("key.keyboard.escape", true), 0));
        assert!(took);
        assert_eq!(rt.get_field(s, "gone"), V::I(1));
        assert!(!h.screen);
        // Opened again, then closed by the game itself: the next hook lets it go.
        rt.put_field(s, "gone", V::I(0));
        rt.with_host(&mut h, |rt| set_screen(rt, s));
        h.screen = false;
        rt.with_host(&mut h, |rt| tick(rt));
        assert_eq!(rt.get_field(s, "gone"), V::I(1));
        assert_eq!(current(&rt), V::NULL);
    }

    #[test]
    fn a_screen_draws_the_in_world_menu_background_tiled_over_the_gui() {
        let (mut rt, s) = rt_with_screen();
        let mut h = host();
        rt.with_host(&mut h, |rt| set_screen(rt, s));
        h.drawn_gui.clear();
        rt.with_host(&mut h, |rt| render(rt, [10.0, 10.0], 0.0));
        let tiles: Vec<&GuiDraw> = h.drawn_gui.iter().filter(|d| matches!(d, GuiDraw::Blit(t, ..) if t == MENU_BACKGROUND)).collect();
        // 427 x 240 in 32-pixel tiles: 14 columns by 8 rows.
        assert_eq!(tiles.len(), 14 * 8);
        let GuiDraw::Blit(_, uv, rect, _) = tiles.last().unwrap() else { unreachable!() };
        assert_eq!(*rect, [416.0, 224.0, 427.0, 240.0]);
        assert_eq!(*uv, [0.0, 0.0, 11.0 / 32.0, 0.5]);
    }

    /// A textured quad built with the `Tesselator` under matrix `m`, white, its corners
    /// `(x, y, u, v)` in the order given, drawn with the shader texture `texture`.
    fn tesselated_quad(rt: &mut Rt, texture: &str, m: V, corners: [(f32, f32, f32, f32); 4]) {
        let tex = rt.string(texture);
        crate::api::call(
            rt,
            "com/mojang/blaze3d/systems/RenderSystem.setShaderTexture.(ILnet/minecraft/resources/ResourceLocation;)V",
            &[V::I(0), tex],
        );
        let t = crate::api::call(
            rt,
            "com/mojang/blaze3d/vertex/Tesselator.getInstance.()Lcom/mojang/blaze3d/vertex/Tesselator;",
            &[],
        );
        let quads = crate::api::get_static(rt, "com/mojang/blaze3d/vertex/VertexFormat$Mode.QUADS");
        let format = crate::api::get_static(
            rt,
            "com/mojang/blaze3d/vertex/DefaultVertexFormat.POSITION_TEX_COLOR",
        );
        let b = crate::api::call(
            rt,
            "com/mojang/blaze3d/vertex/Tesselator.begin.(Lcom/mojang/blaze3d/vertex/VertexFormat$Mode;Lcom/mojang/blaze3d/vertex/VertexFormat;)Lcom/mojang/blaze3d/vertex/BufferBuilder;",
            &[t, quads, format],
        );
        for (x, y, u, v) in corners {
            let c = crate::api::call(
                rt,
                "com/mojang/blaze3d/vertex/BufferBuilder.addVertex.(Lorg/joml/Matrix4f;FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;",
                &[b, m, V::F(x), V::F(y), V::F(0.0)],
            );
            let c = crate::api::call(
                rt,
                "com/mojang/blaze3d/vertex/VertexConsumer.setColor.(IIII)Lcom/mojang/blaze3d/vertex/VertexConsumer;",
                &[c, V::I(255), V::I(255), V::I(255), V::I(255)],
            );
            crate::api::call(
                rt,
                "com/mojang/blaze3d/vertex/VertexConsumer.setUv.(FF)Lcom/mojang/blaze3d/vertex/VertexConsumer;",
                &[c, V::F(u), V::F(v)],
            );
        }
        let mesh = crate::api::call(
            rt,
            "com/mojang/blaze3d/vertex/BufferBuilder.buildOrThrow.()Lcom/mojang/blaze3d/vertex/MeshData;",
            &[b],
        );
        crate::api::call(
            rt,
            "com/mojang/blaze3d/vertex/BufferUploader.drawWithShader.(Lcom/mojang/blaze3d/vertex/MeshData;)V",
            &[mesh],
        );
    }

    fn matrix(rt: &mut Rt) -> V {
        let m = rt.new_object("org/joml/Matrix4f");
        crate::api::call(rt, "org/joml/Matrix4f.<init>.()V", &[m]);
        m
    }

    #[test]
    fn a_quad_built_with_the_tesselator_draws_as_a_blit_of_the_shader_texture() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = host();
        rt.with_host(&mut h, |rt| {
            crate::gui::begin_drawing(rt);
            let m = matrix(rt);
            crate::api::call(rt, "org/joml/Matrix4f.translate.(FFF)Lorg/joml/Matrix4f;", &[m, V::F(10.0), V::F(20.0), V::F(0.0)]);
            // drawTexturedQuad's order: bottom left, bottom right, top right, top left.
            let corners = [(0.0, 8.0, 0.0, 0.25), (16.0, 8.0, 0.5, 0.25), (16.0, 0.0, 0.5, 0.0), (0.0, 0.0, 0.0, 0.0)];
            tesselated_quad(rt, "create:textures/gui/value_settings.png", m, corners);
            crate::gui::end_drawing(rt);
        });
        assert_eq!(h.drawn_gui, vec![GuiDraw::Blit("create:textures/gui/value_settings.png".into(), [0.0, 0.0, 0.5, 0.25], [10.0, 20.0, 26.0, 28.0], u32::MAX)]);
    }

    #[test]
    fn a_sheets_frame_picked_by_a_negative_index_draws_as_gl_repeats_the_texture() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = host();
        rt.with_host(&mut h, |rt| {
            crate::gui::begin_drawing(rt);
            // Ponder's placement indicator: a 2 x 2 quad scaled by 0.75 and 12 at the crosshair,
            // the frame of a 16-frame sheet its angle picks, -90 degrees picking frame -4.
            let m = matrix(rt);
            crate::api::call(
                rt,
                "org/joml/Matrix4f.translate.(FFF)Lorg/joml/Matrix4f;",
                &[m, V::F(213.5), V::F(120.0), V::F(50.0)],
            );
            crate::api::call(
                rt,
                "org/joml/Matrix4f.scale.(FFF)Lorg/joml/Matrix4f;",
                &[m, V::F(9.0), V::F(9.0), V::F(1.0)],
            );
            let (top, bottom) = (-4.0 * 0.0625, -3.0 * 0.0625);
            let corners = [
                (-1.0, -1.0, 0.0, top),
                (-1.0, 1.0, 0.0, bottom),
                (1.0, 1.0, 1.0, bottom),
                (1.0, -1.0, 1.0, top),
            ];
            tesselated_quad(
                rt,
                "ponder:textures/gui/placement_indicator.png",
                m,
                corners,
            );
            crate::gui::end_drawing(rt);
        });
        // GL's repeat wrap reads v -0.25 as 0.75: the sheet's frame 12.
        assert_eq!(
            h.drawn_gui,
            vec![GuiDraw::Blit(
                "ponder:textures/gui/placement_indicator.png".into(),
                [0.0, 0.75, 1.0, 0.8125],
                [204.5, 111.0, 222.5, 129.0],
                u32::MAX
            )]
        );
    }

    #[test]
    fn key_names_and_codes_are_glfws_and_vanillas() {
        assert_eq!(key_name(69).as_deref(), Some("key.keyboard.e"));
        assert_eq!(key_name(256).as_deref(), Some("key.keyboard.escape"));
        assert_eq!(key_name(295).as_deref(), Some("key.keyboard.f6"));
        assert_eq!(key_name(340).as_deref(), Some("key.keyboard.left.shift"));
        assert_eq!(key_code("key.keyboard.keypad.7"), Some(327));
        assert_eq!(key_code("key.keyboard.0"), Some(48));
        assert_eq!(key_code("key.mouse.right"), None);
        assert_eq!(mouse_button("key.mouse.right"), Some(1));
        assert_eq!(mouse_button("key.mouse.4"), Some(3));
    }
}
