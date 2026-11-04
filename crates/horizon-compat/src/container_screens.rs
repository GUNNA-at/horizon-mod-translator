//! Vanilla's `AbstractContainerScreen` (1.21.1, with NeoForge's additions), natively: the screen of a
//! mod's menu ([`crate::menus`]). It lays the window out (`imageWidth`, `leftPos`), draws the
//! background the mod draws (`renderBg`), the widgets, each slot's stack and the one under the
//! pointer lit, the labels, and the carried stack at the pointer; and it turns the pointer and keys
//! into clicks on the menu (`slotClicked`, then `MultiPlayerGameMode.handleInventoryMouseClick`):
//! picking up and placing, shift to move, a drag over slots to spread a stack (quick craft), a
//! double click to gather, the hotbar and off hand keys to swap, the drop and pick keys. The
//! touchscreen mode and the event bus's `ContainerScreenEvent`s aren't modelled.
use crate::menus;
use crate::{Rt, V};

pub(crate) const CONTAINER_SCREEN: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen";
const MENU_ACCESS: &str = "net/minecraft/client/gui/screens/inventory/MenuAccess";

const K_RENDER_BACKGROUND: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderBackground.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V";
const K_TRANSPARENT: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderTransparentBackground.(Lnet/minecraft/client/gui/GuiGraphics;)V";
const K_RENDER_BG: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderBg.(Lnet/minecraft/client/gui/GuiGraphics;FII)V";
const K_RENDER_SLOT: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderSlot.(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/world/inventory/Slot;)V";
const K_SLOT_CONTENTS: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderSlotContents.(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/inventory/Slot;Ljava/lang/String;)V";
const K_SLOT_HIGHLIGHT: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderSlotHighlight.(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/world/inventory/Slot;IIF)V";
const K_RENDER_LABELS: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.renderLabels.(Lnet/minecraft/client/gui/GuiGraphics;II)V";
const K_SLOT_CLICKED: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.slotClicked.(Lnet/minecraft/world/inventory/Slot;IILnet/minecraft/world/inventory/ClickType;)V";
const K_CONTAINER_TICK: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.containerTick.()V";
const K_CLICKED_OUTSIDE: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.hasClickedOutside.(DDIII)Z";
const K_SLOT_COLOR: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.getSlotColor.(I)I";
const K_HOTBAR_KEY: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.checkHotbarKeyPressed.(II)Z";
const K_ON_CLOSE: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.onClose.()V";

const S_HIGHLIGHTABLE: &str = "net/minecraft/world/inventory/Slot.isHighlightable.()Z";
const S_FAKE: &str = "net/minecraft/world/inventory/Slot.isFake.()Z";
const S_ICON: &str = "net/minecraft/world/inventory/Slot.getNoItemIcon.()Lcom/mojang/datafixers/util/Pair;";
const S_SAME_INVENTORY: &str = "net/minecraft/world/inventory/Slot.isSameInventory.(Lnet/minecraft/world/inventory/Slot;)Z";

const G_POSE: &str = "net/minecraft/client/gui/GuiGraphics.pose.()Lcom/mojang/blaze3d/vertex/PoseStack;";
const P_PUSH: &str = "com/mojang/blaze3d/vertex/PoseStack.pushPose.()V";
const P_POP: &str = "com/mojang/blaze3d/vertex/PoseStack.popPose.()V";
const P_TRANSLATE: &str = "com/mojang/blaze3d/vertex/PoseStack.translate.(FFF)V";
const G_FILL: &str = "net/minecraft/client/gui/GuiGraphics.fill.(IIIII)V";
const G_ITEM: &str = "net/minecraft/client/gui/GuiGraphics.renderItem.(Lnet/minecraft/world/item/ItemStack;II)V";
const G_ITEM_SEEDED: &str = "net/minecraft/client/gui/GuiGraphics.renderItem.(Lnet/minecraft/world/item/ItemStack;III)V";
const G_FAKE_ITEM: &str = "net/minecraft/client/gui/GuiGraphics.renderFakeItem.(Lnet/minecraft/world/item/ItemStack;III)V";
const G_DECORATIONS: &str = "net/minecraft/client/gui/GuiGraphics.renderItemDecorations.(Lnet/minecraft/client/gui/Font;Lnet/minecraft/world/item/ItemStack;IILjava/lang/String;)V";
const G_STRING: &str = "net/minecraft/client/gui/GuiGraphics.drawString.(Lnet/minecraft/client/gui/Font;Lnet/minecraft/network/chat/Component;IIIZ)I";
const G_TOOLTIP: &str = "net/minecraft/client/gui/GuiGraphics.renderTooltip.(Lnet/minecraft/client/gui/Font;Lnet/minecraft/world/item/ItemStack;II)V";
const INVENTORY_NAME: &str = "net/minecraft/world/entity/player/Inventory.getDisplayName.()Lnet/minecraft/network/chat/Component;";
const CLOSE_CONTAINER: &str = "net/minecraft/client/player/LocalPlayer.closeContainer.()V";
const HOVER_NAME: &str = "net/minecraft/world/item/ItemStack.getHoverName.()Lnet/minecraft/network/chat/Component;";

/// The colour a slot under the pointer is lit with, and a slot a drag spreads into.
const SLOT_COLOR: i32 = -2130706433;
/// The labels' colour.
const LABEL_COLOR: i32 = 4210752;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

fn truthy(v: V) -> bool {
    matches!(v, V::I(n) if n != 0)
}

/// `Util.getMillis()`: a monotonic clock in milliseconds, far from 0 as vanilla's (`nanoTime`) is,
/// so the first click is never the second of a double click.
fn millis() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as i64 + 1_000_000
}

fn field_i(rt: &Rt, o: V, f: &'static str) -> i32 {
    rt.get_field(o, f).i()
}

fn menu(rt: &Rt, this: V) -> V {
    rt.get_field(this, "menu")
}

fn font(rt: &mut Rt, this: V) -> V {
    match rt.get_field(this, "font") {
        f if f.r() != 0 => f,
        _ => crate::gui::font(rt),
    }
}

// ---- drawing ----

fn pose(rt: &mut Rt, g: V) -> V {
    crate::api::call(rt, G_POSE, &[g])
}

fn push(rt: &mut Rt, g: V) {
    let p = pose(rt, g);
    crate::api::call(rt, P_PUSH, &[p]);
}

fn pop(rt: &mut Rt, g: V) {
    let p = pose(rt, g);
    crate::api::call(rt, P_POP, &[p]);
}

fn translate(rt: &mut Rt, g: V, x: f32, y: f32, z: f32) {
    let p = pose(rt, g);
    crate::api::call(rt, P_TRANSLATE, &[p, V::F(x), V::F(y), V::F(z)]);
}

fn fill(rt: &mut Rt, g: V, [x0, y0, x1, y1]: [i32; 4], color: i32) {
    crate::api::call(rt, G_FILL, &[g, V::I(x0), V::I(y0), V::I(x1), V::I(y1), V::I(color)]);
}

/// `AbstractContainerScreen.render`: the background, the widgets, then in the window's frame the
/// slots (the one under the pointer lit), the labels, and the carried stack at the pointer.
fn render(rt: &mut Rt, this: V, args: &[V]) {
    let (g, mx, my) = (arg(args, 1), arg(args, 2).i(), arg(args, 3).i());
    let (left, top) = (field_i(rt, this, "leftPos"), field_i(rt, this, "topPos"));
    menus::virt(rt, K_RENDER_BACKGROUND, this, &args[1..]);
    let renderables = rt.get_field(this, "renderables");
    for r in crate::jutil::elements(rt, renderables) {
        rt.call_functional("net/minecraft/client/gui/components/Renderable.render.(Lnet/minecraft/client/gui/GuiGraphics;IIF)V", r, &args[1..]);
        if rt.thrown.is_some() {
            return;
        }
    }
    push(rt, g);
    translate(rt, g, left as f32, top as f32, 0.0);
    rt.put_field(this, "hoveredSlot", V::NULL);
    let m = menu(rt, this);
    for slot in menus::slots(rt, m) {
        let active = slot_active(rt, slot);
        if active {
            menus::virt(rt, K_RENDER_SLOT, this, &[g, slot]);
        }
        if active && hovering_slot(rt, this, slot, f64::from(mx), f64::from(my)) {
            rt.put_field(this, "hoveredSlot", slot);
            menus::virt(rt, K_SLOT_HIGHLIGHT, this, &[g, slot, V::I(mx), V::I(my), arg(args, 4)]);
        }
        if rt.thrown.is_some() {
            pop(rt, g);
            return;
        }
    }
    menus::virt(rt, K_RENDER_LABELS, this, &[g, V::I(mx), V::I(my)]);
    let held = menus::carried(rt, m);
    if menus::count(rt, held) > 0 {
        let mut shown = held;
        let mut text = V::NULL;
        let quick = quick_slots(rt, this);
        if truthy(rt.get_field(this, "isQuickCrafting")) && quick.len() > 1 {
            let left = field_i(rt, this, "$quickCraftingRemainder");
            shown = menus::copy_with_count(rt, held, left);
            if menus::count(rt, shown) <= 0 {
                text = rt.string("§e0");
            }
        }
        floating_item(rt, this, g, shown, mx - left - 8, my - top - 8, text);
    }
    pop(rt, g);
}

/// `renderFloatingItem`: a stack over everything, its count (or `text`) on it.
fn floating_item(rt: &mut Rt, this: V, g: V, stack: V, x: i32, y: i32, text: V) {
    push(rt, g);
    translate(rt, g, 0.0, 0.0, 232.0);
    crate::api::call(rt, G_ITEM, &[g, stack, V::I(x), V::I(y)]);
    let f = font(rt, this);
    crate::api::call(rt, G_DECORATIONS, &[g, f, stack, V::I(x), V::I(y), text]);
    pop(rt, g);
}

/// `renderSlot`: the slot's stack (what a drag would leave there while it goes on), or its empty
/// icon.
fn render_slot(rt: &mut Rt, this: V, g: V, slot: V) {
    let (x, y) = (field_i(rt, slot, "x"), field_i(rt, slot, "y"));
    let m = menu(rt, this);
    let mut stack = menus::slot_item(rt, slot);
    let held = menus::carried(rt, m);
    let mut spread = false;
    let mut text = V::NULL;
    let quick = quick_slots(rt, this);
    if truthy(rt.get_field(this, "isQuickCrafting")) && quick.contains(&slot) && menus::count(rt, held) > 0 {
        if quick.len() == 1 {
            return;
        }
        if menus::can_item_quick_replace(rt, slot, held, true) && menus::can_drag_to(rt, m, slot) {
            spread = true;
            let most = menus::max_size(rt, held).min(menus::slot_max_for(rt, slot, held));
            let here = menus::slot_item(rt, slot);
            let had = menus::count(rt, here);
            let ty = field_i(rt, this, "$quickCraftingType");
            let mut n = menus::quick_craft_place_count(rt, quick.len(), ty, held) + had;
            if n > most {
                n = most;
                text = rt.string(&format!("§e{most}"));
            }
            stack = menus::copy_with_count(rt, held, n);
        } else {
            remove_quick_slot(rt, this, slot);
            recalculate_remainder(rt, this);
        }
    }
    push(rt, g);
    translate(rt, g, 0.0, 0.0, 100.0);
    let mut icon = false;
    if menus::count(rt, stack) <= 0 && slot_active(rt, slot) {
        let pair = menus::virt(rt, S_ICON, slot, &[]);
        if pair.r() != 0 {
            draw_icon(rt, g, pair, x, y);
            icon = true;
        }
    }
    if !icon {
        if spread {
            fill(rt, g, [x, y, x + 16, y + 16], SLOT_COLOR);
        }
        menus::virt(rt, K_SLOT_CONTENTS, this, &[g, stack, slot, text]);
    }
    pop(rt, g);
}

/// An empty slot's icon (`getNoItemIcon`: an atlas and a sprite in it), drawn from the sprite's
/// texture.
fn draw_icon(rt: &mut Rt, g: V, pair: V, x: i32, y: i32) {
    let parts = crate::jutil::elements(rt, pair);
    let Some(sprite) = parts.get(1).and_then(|s| rt.str_of(*s)).map(str::to_owned) else { return };
    let (ns, path) = sprite.split_once(':').unwrap_or(("minecraft", &sprite));
    let texture = format!("{ns}:textures/{path}.png");
    let m = crate::gui::pose(rt, g);
    let (x, y) = (x as f32, y as f32);
    crate::gui::blit_at(rt, &m, &texture, [x, x + 16.0, y, y + 16.0], [0.0, 0.0, 1.0, 1.0]);
}

/// `renderSlotContents` (NeoForge's): the stack and its count.
fn render_slot_contents(rt: &mut Rt, this: V, g: V, stack: V, slot: V, text: V) {
    let (x, y) = (field_i(rt, slot, "x"), field_i(rt, slot, "y"));
    let seed = x + y * field_i(rt, this, "imageWidth");
    let key = if truthy(menus::virt(rt, S_FAKE, slot, &[])) { G_FAKE_ITEM } else { G_ITEM_SEEDED };
    crate::api::call(rt, key, &[g, stack, V::I(x), V::I(y), V::I(seed)]);
    let f = font(rt, this);
    crate::api::call(rt, G_DECORATIONS, &[g, f, stack, V::I(x), V::I(y), text]);
}

// ---- slots under the pointer ----

fn slot_active(rt: &mut Rt, slot: V) -> bool {
    truthy(menus::virt(rt, menus::K_SLOT_ACTIVE, slot, &[]))
}

/// `isHovering(x, y, width, height, mouseX, mouseY)`: the pointer within the rectangle (in the
/// window's frame) or a pixel around it.
fn hovering(rt: &Rt, this: V, [x, y, w, h]: [i32; 4], mx: f64, my: f64) -> bool {
    let mx = mx - f64::from(field_i(rt, this, "leftPos"));
    let my = my - f64::from(field_i(rt, this, "topPos"));
    mx >= f64::from(x - 1) && mx < f64::from(x + w + 1) && my >= f64::from(y - 1) && my < f64::from(y + h + 1)
}

fn hovering_slot(rt: &Rt, this: V, slot: V, mx: f64, my: f64) -> bool {
    hovering(rt, this, [field_i(rt, slot, "x"), field_i(rt, slot, "y"), 16, 16], mx, my)
}

/// `findSlot`: the active slot under the pointer, or null.
fn find_slot(rt: &mut Rt, this: V, mx: f64, my: f64) -> V {
    let m = menu(rt, this);
    for slot in menus::slots(rt, m) {
        if hovering_slot(rt, this, slot, mx, my) && slot_active(rt, slot) {
            return slot;
        }
    }
    V::NULL
}

fn quick_slots(rt: &Rt, this: V) -> Vec<V> {
    crate::jutil::elements(rt, rt.get_field(this, "quickCraftSlots"))
}

fn quick_list(rt: &mut Rt, this: V) -> V {
    match rt.get_field(this, "quickCraftSlots") {
        l if l.r() != 0 => l,
        _ => {
            let l = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(this, "quickCraftSlots", l);
            l
        }
    }
}

fn clear_quick_slots(rt: &mut Rt, this: V) {
    let l = quick_list(rt, this);
    if let Some(crate::Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
        v.clear();
    }
}

fn add_quick_slot(rt: &mut Rt, this: V, slot: V) {
    let l = quick_list(rt, this);
    if let Some(crate::Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data)
        && !v.contains(&slot)
    {
        v.push(slot);
    }
}

fn remove_quick_slot(rt: &mut Rt, this: V, slot: V) {
    let l = quick_list(rt, this);
    if let Some(crate::Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
        v.retain(|s| *s != slot);
    }
}

/// `recalculateQuickCraftRemaining`: what of the carried stack a drag over its slots leaves.
fn recalculate_remainder(rt: &mut Rt, this: V) {
    let m = menu(rt, this);
    let held = menus::carried(rt, m);
    if menus::count(rt, held) <= 0 || !truthy(rt.get_field(this, "isQuickCrafting")) {
        return;
    }
    let ty = field_i(rt, this, "$quickCraftingType");
    let left = if ty == 2 {
        menus::max_size(rt, held)
    } else {
        let quick = quick_slots(rt, this);
        let mut left = menus::count(rt, held);
        for slot in quick.iter().copied() {
            let here = menus::slot_item(rt, slot);
            let had = menus::count(rt, here);
            let most = menus::max_size(rt, held).min(menus::slot_max_for(rt, slot, held));
            let put = (menus::quick_craft_place_count(rt, quick.len(), ty, held) + had).min(most);
            left -= put - had;
        }
        left
    };
    rt.put_field(this, "$quickCraftingRemainder", V::I(left));
}

// ---- clicks ----

fn click_type(rt: &mut Rt, ordinal: i32) -> V {
    let i = ordinal.clamp(0, 6);
    crate::api::api_enum(rt, menus::CLICK_TYPE, menus::CLICK_TYPES[i as usize], i)
}

/// `slotClicked(slot, id, button, type)`, as the screen (or the mod's override) has it.
fn slot_clicked(rt: &mut Rt, this: V, slot: V, id: i32, button: i32, ordinal: i32) {
    let ct = click_type(rt, ordinal);
    menus::virt(rt, K_SLOT_CLICKED, this, &[slot, V::I(id), V::I(button), ct]);
}

/// Whether key mapping `name` (`key.pickItem`) is bound to `key` (`key.mouse.middle`).
fn bound_to(rt: &Rt, name: &str, key: Option<&str>) -> bool {
    key.is_some_and(|k| crate::screens::mapping_binding(rt, name).0 == k)
}

fn shift_down(rt: &Rt) -> bool {
    crate::screens::held_either(rt, "shift")
}

fn control_down(rt: &Rt) -> bool {
    crate::screens::held_either(rt, if cfg!(target_os = "macos") { "win" } else { "control" })
}

/// `mouseClicked`: the widgets first; then the click picks up or places at the slot under the
/// pointer (shift moves the stack across), throws outside the window, or starts a drag with a
/// stack carried.
fn mouse_clicked(rt: &mut Rt, this: V, args: &[V]) -> bool {
    if crate::screens::screen_base(rt, "mouseClicked", "(DDI)Z", args).is_some_and(truthy) {
        return true;
    }
    let (mx, my, button) = (arg(args, 1).d(), arg(args, 2).d(), arg(args, 3).i());
    let mouse = crate::screens::mouse_name(button);
    let pick = bound_to(rt, "key.pickItem", Some(&mouse));
    let slot = find_slot(rt, this, mx, my);
    let now = millis();
    let double = rt.get_field(this, "$lastClickSlot") == slot
        && now - rt.get_field(this, "$lastClickTime").j() < 250
        && field_i(rt, this, "$lastClickButton") == button;
    rt.put_field(this, "$doubleclick", V::I(double as i32));
    rt.put_field(this, "$skipNextRelease", V::I(0));
    if button != 0 && button != 1 && !pick {
        hotbar_mouse_clicked(rt, this, button);
    } else {
        let (left, top) = (field_i(rt, this, "leftPos"), field_i(rt, this, "topPos"));
        let outside = slot.r() == 0 && truthy(menus::virt(rt, K_CLICKED_OUTSIDE, this, &[V::D(mx), V::D(my), V::I(left), V::I(top), V::I(button)]));
        let id = if outside {
            menus::OUTSIDE
        } else if slot.r() != 0 {
            menus::slot_index(rt, slot)
        } else {
            -1
        };
        if id != -1 && !truthy(rt.get_field(this, "isQuickCrafting")) {
            let m = menu(rt, this);
            let held = menus::carried(rt, m);
            if menus::count(rt, held) <= 0 {
                if pick {
                    slot_clicked(rt, this, slot, id, button, menus::CLONE);
                } else {
                    let shift = id != menus::OUTSIDE && shift_down(rt);
                    let ty = if shift {
                        remember_quick_moved(rt, this, slot);
                        menus::QUICK_MOVE
                    } else if id == menus::OUTSIDE {
                        menus::THROW
                    } else {
                        menus::PICKUP
                    };
                    slot_clicked(rt, this, slot, id, button, ty);
                }
                rt.put_field(this, "$skipNextRelease", V::I(1));
            } else {
                rt.put_field(this, "isQuickCrafting", V::I(1));
                rt.put_field(this, "$quickCraftingButton", V::I(button));
                clear_quick_slots(rt, this);
                let ty = match button {
                    0 => Some(0),
                    1 => Some(1),
                    _ if pick => Some(2),
                    _ => None,
                };
                if let Some(ty) = ty {
                    rt.put_field(this, "$quickCraftingType", V::I(ty));
                }
            }
        }
    }
    rt.put_field(this, "$lastClickSlot", slot);
    rt.put_field(this, "$lastClickTime", V::J(now));
    rt.put_field(this, "$lastClickButton", V::I(button));
    true
}

/// What a shift-click moved, for a shift double click to move the rest of its kind.
fn remember_quick_moved(rt: &mut Rt, this: V, slot: V) {
    let moved = if slot.r() != 0 && menus::has_item(rt, slot) {
        let s = menus::slot_item(rt, slot);
        menus::copy(rt, s)
    } else {
        menus::empty(rt)
    };
    rt.put_field(this, "$lastQuickMoved", moved);
}

/// `checkHotbarMouseClicked`: a mouse button bound to a hotbar slot or the off hand swaps the
/// slot under the pointer with it.
fn hotbar_mouse_clicked(rt: &mut Rt, this: V, button: i32) {
    let hovered = rt.get_field(this, "hoveredSlot");
    let m = menu(rt, this);
    let held = menus::carried(rt, m);
    if hovered.r() == 0 || menus::count(rt, held) > 0 {
        return;
    }
    let mouse = crate::screens::mouse_name(button);
    let index = menus::slot_index(rt, hovered);
    if bound_to(rt, "key.swapOffhand", Some(&mouse)) {
        slot_clicked(rt, this, hovered, index, 40, menus::SWAP);
        return;
    }
    for i in 0..9 {
        if bound_to(rt, &format!("key.hotbar.{}", i + 1), Some(&mouse)) {
            slot_clicked(rt, this, hovered, index, i, menus::SWAP);
        }
    }
}

/// `mouseDragged`: a drag with a stack carried takes in each slot it crosses that can take some.
fn mouse_dragged(rt: &mut Rt, this: V, args: &[V]) -> bool {
    let (mx, my) = (arg(args, 1).d(), arg(args, 2).d());
    let slot = find_slot(rt, this, mx, my);
    let m = menu(rt, this);
    let held = menus::carried(rt, m);
    if truthy(rt.get_field(this, "isQuickCrafting")) && slot.r() != 0 && menus::count(rt, held) > 0 {
        let ty = field_i(rt, this, "$quickCraftingType");
        let taken = quick_slots(rt, this).len() as i32;
        if (menus::count(rt, held) > taken || ty == 2)
            && menus::can_item_quick_replace(rt, slot, held, true)
            && menus::may_place(rt, slot, held)
            && menus::can_drag_to(rt, m, slot)
        {
            add_quick_slot(rt, this, slot);
            recalculate_remainder(rt, this);
        }
    }
    true
}

/// `mouseReleased`: a double click gathers the stack's kind (shift: moves every stack like the one
/// last moved), a drag spreads the carried stack over its slots, a click with a stack carried
/// places it.
fn mouse_released(rt: &mut Rt, this: V, args: &[V]) -> bool {
    crate::screens::screen_base(rt, "mouseReleased", "(DDI)Z", args);
    let (mx, my, button) = (arg(args, 1).d(), arg(args, 2).d(), arg(args, 3).i());
    let slot = find_slot(rt, this, mx, my);
    let (left, top) = (field_i(rt, this, "leftPos"), field_i(rt, this, "topPos"));
    let outside = slot.r() == 0 && truthy(menus::virt(rt, K_CLICKED_OUTSIDE, this, &[V::D(mx), V::D(my), V::I(left), V::I(top), V::I(button)]));
    let mouse = crate::screens::mouse_name(button);
    let id = if outside {
        menus::OUTSIDE
    } else if slot.r() != 0 {
        menus::slot_index(rt, slot)
    } else {
        -1
    };
    let m = menu(rt, this);
    let e = menus::empty(rt);
    if truthy(rt.get_field(this, "$doubleclick"))
        && slot.r() != 0
        && button == 0
        && truthy(menus::virt(rt, "net/minecraft/world/inventory/AbstractContainerMenu.canTakeItemForPickAll.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/inventory/Slot;)Z", m, &[e, slot]))
    {
        if shift_down(rt) {
            let moved = rt.get_field(this, "$lastQuickMoved");
            if moved.r() != 0 && menus::count(rt, moved) > 0 {
                let player = crate::network::local_player(rt);
                for other in menus::slots(rt, m) {
                    if menus::may_pickup(rt, other, player)
                        && menus::has_item(rt, other)
                        && truthy(menus::virt(rt, S_SAME_INVENTORY, other, &[slot]))
                        && menus::can_item_quick_replace(rt, other, moved, true)
                    {
                        let index = menus::slot_index(rt, other);
                        slot_clicked(rt, this, other, index, button, menus::QUICK_MOVE);
                    }
                }
            }
        } else {
            slot_clicked(rt, this, slot, id, button, menus::PICKUP_ALL);
        }
        rt.put_field(this, "$doubleclick", V::I(0));
        rt.put_field(this, "$lastClickTime", V::J(0));
    } else {
        if truthy(rt.get_field(this, "isQuickCrafting")) && field_i(rt, this, "$quickCraftingButton") != button {
            rt.put_field(this, "isQuickCrafting", V::I(0));
            clear_quick_slots(rt, this);
            rt.put_field(this, "$skipNextRelease", V::I(1));
            return true;
        }
        if truthy(rt.get_field(this, "$skipNextRelease")) {
            rt.put_field(this, "$skipNextRelease", V::I(0));
            return true;
        }
        let quick = quick_slots(rt, this);
        let held = menus::carried(rt, m);
        if truthy(rt.get_field(this, "isQuickCrafting")) && !quick.is_empty() {
            let ty = field_i(rt, this, "$quickCraftingType");
            let mask = |header: i32| header & 3 | (ty & 3) << 2;
            slot_clicked(rt, this, V::NULL, menus::OUTSIDE, mask(0), menus::QUICK_CRAFT);
            for s in quick {
                let index = menus::slot_index(rt, s);
                slot_clicked(rt, this, s, index, mask(1), menus::QUICK_CRAFT);
            }
            slot_clicked(rt, this, V::NULL, menus::OUTSIDE, mask(2), menus::QUICK_CRAFT);
        } else if menus::count(rt, held) > 0 {
            if bound_to(rt, "key.pickItem", Some(&mouse)) {
                slot_clicked(rt, this, slot, id, button, menus::CLONE);
            } else {
                let shift = id != menus::OUTSIDE && shift_down(rt);
                if shift {
                    remember_quick_moved(rt, this, slot);
                }
                slot_clicked(rt, this, slot, id, button, if shift { menus::QUICK_MOVE } else { menus::PICKUP });
            }
        }
    }
    let held = menus::carried(rt, m);
    if menus::count(rt, held) <= 0 {
        rt.put_field(this, "$lastClickTime", V::J(0));
    }
    rt.put_field(this, "isQuickCrafting", V::I(0));
    true
}

/// `keyPressed`: the screen's own keys (Escape) first; the inventory key closes, the hotbar and off
/// hand keys swap the slot under the pointer, the pick key clones its stack, the drop key throws
/// one (with control: all of it).
fn key_pressed(rt: &mut Rt, this: V, args: &[V]) -> bool {
    if crate::screens::screen_base(rt, "keyPressed", "(III)Z", args).is_some_and(truthy) {
        return true;
    }
    let (code, scan) = (arg(args, 1).i(), arg(args, 2).i());
    let key = crate::screens::key_name(code);
    if bound_to(rt, "key.inventory", key.as_deref()) {
        menus::virt(rt, K_ON_CLOSE, this, &[]);
        return true;
    }
    let mut handled = truthy(menus::virt(rt, K_HOTBAR_KEY, this, &[V::I(code), V::I(scan)]));
    let hovered = rt.get_field(this, "hoveredSlot");
    if hovered.r() != 0 && menus::has_item(rt, hovered) {
        let index = menus::slot_index(rt, hovered);
        if bound_to(rt, "key.pickItem", key.as_deref()) {
            slot_clicked(rt, this, hovered, index, 0, menus::CLONE);
            handled = true;
        } else if bound_to(rt, "key.drop", key.as_deref()) {
            let all = control_down(rt);
            slot_clicked(rt, this, hovered, index, i32::from(all), menus::THROW);
            handled = true;
        }
    } else if bound_to(rt, "key.drop", key.as_deref()) {
        // NeoForge (MC-146650): the drop key over no stack is taken, so nothing in hand drops.
        handled = true;
    }
    handled
}

/// `checkHotbarKeyPressed`: a hotbar key or the off hand's swaps the slot under the pointer.
fn hotbar_key_pressed(rt: &mut Rt, this: V, code: i32) -> bool {
    let hovered = rt.get_field(this, "hoveredSlot");
    let m = menu(rt, this);
    let held = menus::carried(rt, m);
    if hovered.r() == 0 || menus::count(rt, held) > 0 {
        return false;
    }
    let key = crate::screens::key_name(code);
    let index = menus::slot_index(rt, hovered);
    if bound_to(rt, "key.swapOffhand", key.as_deref()) {
        slot_clicked(rt, this, hovered, index, 40, menus::SWAP);
        return true;
    }
    for i in 0..9 {
        if bound_to(rt, &format!("key.hotbar.{}", i + 1), key.as_deref()) {
            slot_clicked(rt, this, hovered, index, i, menus::SWAP);
            return true;
        }
    }
    false
}

// ---- the calls ----

/// `AbstractContainerScreen`'s own methods (what it overrides of `Screen` and what it adds), for a
/// container screen: None for the rest, which are `Screen`'s.
pub(crate) fn own(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match (name, desc) {
        (
            "<init>",
            "(Lnet/minecraft/world/inventory/AbstractContainerMenu;Lnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/chat/Component;)V",
        ) => {
            crate::screens::screen_base(rt, "<init>", "(Lnet/minecraft/network/chat/Component;)V", &[this, a(3)]);
            rt.put_field(this, "menu", a(1));
            let inventory_title = crate::api::call(rt, INVENTORY_NAME, &[a(2)]);
            rt.put_field(this, "playerInventoryTitle", inventory_title);
            for (f, v) in [
                ("imageWidth", 176),
                ("imageHeight", 166),
                ("titleLabelX", 8),
                ("titleLabelY", 6),
                ("inventoryLabelX", 8),
                ("inventoryLabelY", 166 - 94),
                ("slotColor", SLOT_COLOR),
                ("$skipNextRelease", 1),
                ("isQuickCrafting", 0),
            ] {
                rt.put_field(this, f, V::I(v));
            }
            let e = menus::empty(rt);
            rt.put_field(this, "$lastQuickMoved", e);
            rt.put_field(this, "hoveredSlot", V::NULL);
            quick_list(rt, this);
            V::Void
        }
        ("init", "()V") => {
            let (w, h) = (field_i(rt, this, "width"), field_i(rt, this, "height"));
            let (iw, ih) = (field_i(rt, this, "imageWidth"), field_i(rt, this, "imageHeight"));
            rt.put_field(this, "leftPos", V::I((w - iw) / 2));
            rt.put_field(this, "topPos", V::I((h - ih) / 2));
            V::Void
        }
        ("render", "(Lnet/minecraft/client/gui/GuiGraphics;IIF)V") => {
            render(rt, this, args);
            V::Void
        }
        ("renderBackground", "(Lnet/minecraft/client/gui/GuiGraphics;IIF)V") => {
            menus::virt(rt, K_TRANSPARENT, this, &[a(1)]);
            menus::virt(rt, K_RENDER_BG, this, &[a(1), a(4), a(2), a(3)]);
            V::Void
        }
        // Abstract: the mod's screen draws its window.
        ("renderBg", _) => V::Void,
        ("renderSlot", _) => {
            render_slot(rt, this, a(1), a(2));
            V::Void
        }
        ("renderSlotContents", _) => {
            render_slot_contents(rt, this, a(1), a(2), a(3), a(4));
            V::Void
        }
        ("renderSlotHighlight", "(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/world/inventory/Slot;IIF)V") => {
            let slot = a(2);
            if truthy(menus::virt(rt, S_HIGHLIGHTABLE, slot, &[])) {
                let index = menus::slot_index(rt, slot);
                let color = menus::virt(rt, K_SLOT_COLOR, this, &[V::I(index)]).i();
                let (x, y) = (field_i(rt, slot, "x"), field_i(rt, slot, "y"));
                fill(rt, a(1), [x, y, x + 16, y + 16], color);
            }
            V::Void
        }
        // static renderSlotHighlight(graphics, x, y, blitOffset[, colour])
        ("renderSlotHighlight", _) => {
            let color = if desc.ends_with("IIIII)V") { a(4).i() } else { SLOT_COLOR };
            let (x, y) = (a(1).i(), a(2).i());
            fill(rt, a(0), [x, y, x + 16, y + 16], color);
            V::Void
        }
        ("renderLabels", _) => {
            let f = font(rt, this);
            for (text, x, y) in [("title", "titleLabelX", "titleLabelY"), ("playerInventoryTitle", "inventoryLabelX", "inventoryLabelY")] {
                let (t, x, y) = (rt.get_field(this, text), rt.get_field(this, x), rt.get_field(this, y));
                if t.r() != 0 {
                    crate::api::call(rt, G_STRING, &[a(1), f, t, x, y, V::I(LABEL_COLOR), V::I(0)]);
                }
            }
            V::Void
        }
        ("renderTooltip", "(Lnet/minecraft/client/gui/GuiGraphics;II)V") => {
            let m = menu(rt, this);
            let held = menus::carried(rt, m);
            let hovered = rt.get_field(this, "hoveredSlot");
            if menus::count(rt, held) <= 0 && hovered.r() != 0 && menus::has_item(rt, hovered) {
                let s = menus::slot_item(rt, hovered);
                let f = font(rt, this);
                crate::api::call(rt, G_TOOLTIP, &[a(1), f, s, a(2), a(3)]);
            }
            V::Void
        }
        ("getTooltipFromContainerItem", _) => {
            let name = crate::api::call(rt, HOVER_NAME, &[a(1)]);
            crate::jutil::new_list(rt, vec![name])
        }
        ("mouseClicked", "(DDI)Z") => V::I(mouse_clicked(rt, this, args) as i32),
        ("mouseDragged", "(DDIDD)Z") => V::I(mouse_dragged(rt, this, args) as i32),
        ("mouseReleased", "(DDI)Z") => V::I(mouse_released(rt, this, args) as i32),
        ("keyPressed", "(III)Z") => V::I(key_pressed(rt, this, args) as i32),
        ("checkHotbarKeyPressed", _) => V::I(hotbar_key_pressed(rt, this, a(1).i()) as i32),
        ("hasClickedOutside", _) => {
            let (mx, my, left, top) = (a(1).d(), a(2).d(), a(3).i(), a(4).i());
            let (w, h) = (field_i(rt, this, "imageWidth"), field_i(rt, this, "imageHeight"));
            V::I((mx < f64::from(left) || my < f64::from(top) || mx >= f64::from(left + w) || my >= f64::from(top + h)) as i32)
        }
        ("isHovering", "(IIIIDD)Z") => V::I(hovering(rt, this, [a(1).i(), a(2).i(), a(3).i(), a(4).i()], a(5).d(), a(6).d()) as i32),
        ("isHovering", _) => V::I(hovering_slot(rt, this, a(1), a(2).d(), a(3).d()) as i32),
        ("findSlot", _) => find_slot(rt, this, a(1).d(), a(2).d()),
        ("slotClicked", _) => {
            let (slot, button) = (a(1), a(3).i());
            let id = if slot.r() != 0 { menus::slot_index(rt, slot) } else { a(2).i() };
            let m = menu(rt, this);
            let container = field_i(rt, m, "containerId");
            let player = crate::network::local_player(rt);
            menus::client_click(rt, container, id, button, a(4), player);
            V::Void
        }
        ("clearDraggingState", _) => V::Void,
        ("tick", "()V") => {
            menus::virt(rt, K_CONTAINER_TICK, this, &[]);
            V::Void
        }
        ("containerTick", _) => V::Void,
        // The menu is told it closed on this client.
        ("removed", "()V") => {
            let m = menu(rt, this);
            if m.r() != 0 {
                let player = crate::network::local_player(rt);
                menus::virt(rt, menus::K_REMOVED, m, &[player]);
            }
            V::Void
        }
        // The player closes the menu (the server hears), then the screen closes.
        ("onClose", "()V") => {
            let player = crate::network::local_player(rt);
            crate::api::call(rt, CLOSE_CONTAINER, &[player]);
            crate::screens::screen_base(rt, "onClose", "()V", &[this]);
            V::Void
        }
        ("isPauseScreen", "()Z") => V::I(0),
        ("getMenu", _) => menu(rt, this),
        ("getSlotUnderMouse", _) => rt.get_field(this, "hoveredSlot"),
        ("getGuiLeft", _) => rt.get_field(this, "leftPos"),
        ("getGuiTop", _) => rt.get_field(this, "topPos"),
        ("getXSize", _) => rt.get_field(this, "imageWidth"),
        ("getYSize", _) => rt.get_field(this, "imageHeight"),
        ("getSlotColor", _) => rt.get_field(this, "slotColor"),
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, CONTAINER_SCREEN | MENU_ACCESS)
}

/// The calls this module models (`owner.name.desc`), or None: a container screen's own, then
/// `Screen`'s.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    own(rt, name, desc, args).or_else(|| crate::screens::screen_base(rt, name, desc, args))
}
