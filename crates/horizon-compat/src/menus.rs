//! Container menus, natively, as vanilla 1.21.1 and NeoForge run them: a mod's
//! `AbstractContainerMenu` (its slots, the carried stack, a click as `doClick` handles it,
//! shift-clicks through the mod's `quickMoveStack` and `moveItemStackTo`, a stack dragged over
//! slots, data slots), vanilla's `Slot` and NeoForge's `SlotItemHandler`, the player's `Inventory`
//! (each slot through the host, by [`crate::hand`]), `SimpleContainer`, `MenuType` (NeoForge's
//! `IMenuTypeExtension.create` with the mod's factory), `MenuProvider` and `SimpleMenuProvider`.
//!
//! A server opens a mod's menu for a player (`ServerPlayer.openMenu`, NeoForge's with extra data
//! for the client) and keeps it in step with the player's client as vanilla's synchronizer does,
//! over [`CHANNEL`]: the client makes the menu with the type's factory and its screen with the
//! screen factory the mod registered (`RegisterMenuScreensEvent`), predicts each click
//! (`MultiPlayerGameMode.handleInventoryMouseClick`) and sends it; the server runs it
//! (`handleContainerClick`), sends back what differs from what the client said it has, ticks the
//! menu (`broadcastChanges`, `stillValid`) and closes it. What a client sends runs in its player's
//! tick on the server ([`player_tick`]), where the host acts for that player.
use crate::host::PayloadTarget;
use crate::network;
use crate::{Data, Rt, V};

/// The channel menus keep in step over, between a server's runtime and its players' clients.
pub const CHANNEL: &str = "horizon_compat:menu";

pub(crate) const MENU: &str = "net/minecraft/world/inventory/AbstractContainerMenu";
pub(crate) const INVENTORY_MENU: &str = "net/minecraft/world/inventory/InventoryMenu";
pub(crate) const SLOT: &str = "net/minecraft/world/inventory/Slot";
pub(crate) const HANDLER_SLOT: &str = "net/neoforged/neoforge/items/SlotItemHandler";
const MENU_TYPE: &str = "net/minecraft/world/inventory/MenuType";
const MENU_TYPE_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IMenuTypeExtension";
pub(crate) const INVENTORY: &str = "net/minecraft/world/entity/player/Inventory";
pub(crate) const CONTAINER: &str = "net/minecraft/world/Container";
pub(crate) const SIMPLE_CONTAINER: &str = "net/minecraft/world/SimpleContainer";
const MENU_PROVIDER: &str = "net/minecraft/world/MenuProvider";
const MENU_CONSTRUCTOR: &str = "net/minecraft/world/inventory/MenuConstructor";
const MENU_PROVIDER_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IMenuProviderExtension";
const SIMPLE_MENU_PROVIDER: &str = "net/minecraft/world/SimpleMenuProvider";
const DATA_SLOT: &str = "net/minecraft/world/inventory/DataSlot";
const CONTAINER_DATA: &str = "net/minecraft/world/inventory/ContainerData";
const SIMPLE_CONTAINER_DATA: &str = "net/minecraft/world/inventory/SimpleContainerData";
const LEVEL_ACCESS: &str = "net/minecraft/world/inventory/ContainerLevelAccess";
const OPTIONAL_INT: &str = "java/util/OptionalInt";
const OPTIONAL_LONG: &str = "java/util/OptionalLong";
const OPTIONAL_DOUBLE: &str = "java/util/OptionalDouble";
pub(crate) const CLICK_TYPE: &str = "net/minecraft/world/inventory/ClickType";
const MENU_SCREENS: &str = "net/minecraft/client/gui/screens/MenuScreens";
const REGISTER_SCREENS: &str = "net/neoforged/neoforge/client/event/RegisterMenuScreensEvent";
const GAME_MODE: &str = "net/minecraft/client/multiplayer/MultiPlayerGameMode";
const PLAYER_EXTENSION: &str = "net/neoforged/neoforge/common/extensions/IPlayerExtension";
const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const LOCAL_PLAYER: &str = "net/minecraft/client/player/LocalPlayer";
const NON_NULL_LIST: &str = "net/minecraft/core/NonNullList";

/// `ClickType`'s constants, in order (their ordinals).
pub(crate) const CLICK_TYPES: [&str; 7] = ["PICKUP", "QUICK_MOVE", "SWAP", "CLONE", "THROW", "QUICK_CRAFT", "PICKUP_ALL"];
pub(crate) const PICKUP: i32 = 0;
pub(crate) const QUICK_MOVE: i32 = 1;
pub(crate) const SWAP: i32 = 2;
pub(crate) const CLONE: i32 = 3;
pub(crate) const THROW: i32 = 4;
pub(crate) const QUICK_CRAFT: i32 = 5;
pub(crate) const PICKUP_ALL: i32 = 6;
/// `AbstractContainerMenu.SLOT_CLICKED_OUTSIDE`.
pub(crate) const OUTSIDE: i32 = -999;

// The channel's messages: the server's, then the client's.
/// A menu opens (`AdvancedOpenScreenPayload`): type, id, title, the provider's extra data.
const OPEN: u8 = 0;
/// Slots, the carried stack and data values change (`ClientboundContainerSetContentPacket`,
/// `...SetSlotPacket`, `...SetDataPacket`).
const UPDATE: u8 = 1;
/// The server closed the menu (`ClientboundContainerClosePacket`).
const CLOSE: u8 = 2;
/// A click (`ServerboundContainerClickPacket`).
const CLICK: u8 = 10;
/// A button of the menu (`ServerboundContainerButtonClickPacket`).
const BUTTON: u8 = 11;
/// The client closed the menu (`ServerboundContainerClosePacket`).
const CLOSED: u8 = 12;
/// What one message carries at most before the rest goes in another (the game takes 30000 bytes).
const MESSAGE_BUDGET: usize = 24_000;

const K_QUICK_MOVE: &str = "net/minecraft/world/inventory/AbstractContainerMenu.quickMoveStack.(Lnet/minecraft/world/entity/player/Player;I)Lnet/minecraft/world/item/ItemStack;";
const K_STILL_VALID: &str = "net/minecraft/world/inventory/AbstractContainerMenu.stillValid.(Lnet/minecraft/world/entity/player/Player;)Z";
const K_CLICKED: &str = "net/minecraft/world/inventory/AbstractContainerMenu.clicked.(IILnet/minecraft/world/inventory/ClickType;Lnet/minecraft/world/entity/player/Player;)V";
const K_PICK_ALL: &str = "net/minecraft/world/inventory/AbstractContainerMenu.canTakeItemForPickAll.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/inventory/Slot;)Z";
pub(crate) const K_DRAG_TO: &str = "net/minecraft/world/inventory/AbstractContainerMenu.canDragTo.(Lnet/minecraft/world/inventory/Slot;)Z";
pub(crate) const K_REMOVED: &str = "net/minecraft/world/inventory/AbstractContainerMenu.removed.(Lnet/minecraft/world/entity/player/Player;)V";
const K_MENU_BUTTON: &str = "net/minecraft/world/inventory/AbstractContainerMenu.clickMenuButton.(Lnet/minecraft/world/entity/player/Player;I)Z";
const K_SET_ITEM: &str = "net/minecraft/world/inventory/AbstractContainerMenu.setItem.(IILnet/minecraft/world/item/ItemStack;)V";
const K_SET_DATA: &str = "net/minecraft/world/inventory/AbstractContainerMenu.setData.(II)V";
const K_BROADCAST: &str = "net/minecraft/world/inventory/AbstractContainerMenu.broadcastChanges.()V";

pub(crate) const K_GET_ITEM: &str = "net/minecraft/world/inventory/Slot.getItem.()Lnet/minecraft/world/item/ItemStack;";
const K_HAS_ITEM: &str = "net/minecraft/world/inventory/Slot.hasItem.()Z";
pub(crate) const K_MAY_PLACE: &str = "net/minecraft/world/inventory/Slot.mayPlace.(Lnet/minecraft/world/item/ItemStack;)Z";
pub(crate) const K_MAY_PICKUP: &str = "net/minecraft/world/inventory/Slot.mayPickup.(Lnet/minecraft/world/entity/player/Player;)Z";
const K_SET: &str = "net/minecraft/world/inventory/Slot.set.(Lnet/minecraft/world/item/ItemStack;)V";
const K_SET_BY_PLAYER: &str = "net/minecraft/world/inventory/Slot.setByPlayer.(Lnet/minecraft/world/item/ItemStack;)V";
const K_SET_BY_PLAYER_OLD: &str = "net/minecraft/world/inventory/Slot.setByPlayer.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)V";
const K_SET_CHANGED: &str = "net/minecraft/world/inventory/Slot.setChanged.()V";
const K_SLOT_MAX: &str = "net/minecraft/world/inventory/Slot.getMaxStackSize.()I";
pub(crate) const K_SLOT_MAX_FOR: &str = "net/minecraft/world/inventory/Slot.getMaxStackSize.(Lnet/minecraft/world/item/ItemStack;)I";
const K_REMOVE: &str = "net/minecraft/world/inventory/Slot.remove.(I)Lnet/minecraft/world/item/ItemStack;";
const K_ON_TAKE: &str = "net/minecraft/world/inventory/Slot.onTake.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/item/ItemStack;)V";
const K_ON_SWAP_CRAFT: &str = "net/minecraft/world/inventory/Slot.onSwapCraft.(I)V";
const K_ON_QUICK_CRAFT_N: &str = "net/minecraft/world/inventory/Slot.onQuickCraft.(Lnet/minecraft/world/item/ItemStack;I)V";
const K_ALLOW_MODIFICATION: &str = "net/minecraft/world/inventory/Slot.allowModification.(Lnet/minecraft/world/entity/player/Player;)Z";
const K_TRY_REMOVE: &str = "net/minecraft/world/inventory/Slot.tryRemove.(IILnet/minecraft/world/entity/player/Player;)Ljava/util/Optional;";
const K_SAFE_INSERT: &str = "net/minecraft/world/inventory/Slot.safeInsert.(Lnet/minecraft/world/item/ItemStack;I)Lnet/minecraft/world/item/ItemStack;";
const K_SAFE_TAKE: &str = "net/minecraft/world/inventory/Slot.safeTake.(IILnet/minecraft/world/entity/player/Player;)Lnet/minecraft/world/item/ItemStack;";
pub(crate) const K_SLOT_ACTIVE: &str = "net/minecraft/world/inventory/Slot.isActive.()Z";

const C_GET: &str = "net/minecraft/world/Container.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
const C_SET: &str = "net/minecraft/world/Container.setItem.(ILnet/minecraft/world/item/ItemStack;)V";
const C_REMOVE: &str = "net/minecraft/world/Container.removeItem.(II)Lnet/minecraft/world/item/ItemStack;";
const C_CHANGED: &str = "net/minecraft/world/Container.setChanged.()V";
const C_MAX: &str = "net/minecraft/world/Container.getMaxStackSize.()I";
const C_MAX_FOR: &str = "net/minecraft/world/Container.getMaxStackSize.(Lnet/minecraft/world/item/ItemStack;)I";
const C_SIZE: &str = "net/minecraft/world/Container.getContainerSize.()I";

const H_GET: &str = "net/neoforged/neoforge/items/IItemHandler.getStackInSlot.(I)Lnet/minecraft/world/item/ItemStack;";
const H_SET: &str = "net/neoforged/neoforge/items/IItemHandlerModifiable.setStackInSlot.(ILnet/minecraft/world/item/ItemStack;)V";
const H_VALID: &str = "net/neoforged/neoforge/items/IItemHandler.isItemValid.(ILnet/minecraft/world/item/ItemStack;)Z";
const H_LIMIT: &str = "net/neoforged/neoforge/items/IItemHandler.getSlotLimit.(I)I";
const H_EXTRACT: &str = "net/neoforged/neoforge/items/IItemHandler.extractItem.(IIZ)Lnet/minecraft/world/item/ItemStack;";

const D_GET: &str = "net/minecraft/world/inventory/DataSlot.get.()I";
const D_SET: &str = "net/minecraft/world/inventory/DataSlot.set.(I)V";
const D_CHECK: &str = "net/minecraft/world/inventory/DataSlot.checkAndClearUpdateFlag.()Z";
const CD_GET: &str = "net/minecraft/world/inventory/ContainerData.get.(I)I";
const CD_SET: &str = "net/minecraft/world/inventory/ContainerData.set.(II)V";
const CD_COUNT: &str = "net/minecraft/world/inventory/ContainerData.getCount.()I";

const L_SLOT_CHANGED: &str = "net/minecraft/world/inventory/ContainerListener.slotChanged.(Lnet/minecraft/world/inventory/AbstractContainerMenu;ILnet/minecraft/world/item/ItemStack;)V";
const L_DATA_CHANGED: &str = "net/minecraft/world/inventory/ContainerListener.dataChanged.(Lnet/minecraft/world/inventory/AbstractContainerMenu;II)V";
const CL_CHANGED: &str = "net/minecraft/world/ContainerListener.containerChanged.(Lnet/minecraft/world/Container;)V";

const P_CREATE: &str = "net/minecraft/world/inventory/MenuConstructor.createMenu.(ILnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/world/entity/player/Player;)Lnet/minecraft/world/inventory/AbstractContainerMenu;";
const P_NAME: &str = "net/minecraft/world/MenuProvider.getDisplayName.()Lnet/minecraft/network/chat/Component;";
const P_WRITE: &str = "net/minecraft/world/MenuProvider.writeClientSideData.(Lnet/minecraft/world/inventory/AbstractContainerMenu;Lnet/minecraft/network/RegistryFriendlyByteBuf;)V";
const F_CREATE: &str = "net/neoforged/neoforge/network/IContainerFactory.create.(ILnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/RegistryFriendlyByteBuf;)Lnet/minecraft/world/inventory/AbstractContainerMenu;";
const SUPPLIER_CREATE: &str = "net/minecraft/world/inventory/MenuType$MenuSupplier.create.(ILnet/minecraft/world/entity/player/Inventory;)Lnet/minecraft/world/inventory/AbstractContainerMenu;";
const SCREEN_CREATE: &str = "net/minecraft/client/gui/screens/MenuScreens$ScreenConstructor.create.(Lnet/minecraft/world/inventory/AbstractContainerMenu;Lnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/chat/Component;)Lnet/minecraft/client/gui/screens/Screen;";
const CONSUMER: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
const INT_CONSUMER: &str = "java/util/function/IntConsumer.accept.(I)V";
const BI_FUNCTION: &str = "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const BI_CONSUMER: &str = "java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V";

const S_COPY: &str = "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;";
const S_COPY_WITH: &str = "net/minecraft/world/item/ItemStack.copyWithCount.(I)Lnet/minecraft/world/item/ItemStack;";
const S_SPLIT: &str = "net/minecraft/world/item/ItemStack.split.(I)Lnet/minecraft/world/item/ItemStack;";
const S_MAX: &str = "net/minecraft/world/item/ItemStack.getMaxStackSize.()I";
const S_MATCHES: &str = "net/minecraft/world/item/ItemStack.matches.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";
const S_SAME_COMPONENTS: &str = "net/minecraft/world/item/ItemStack.isSameItemSameComponents.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";
const S_SAME_ITEM: &str = "net/minecraft/world/item/ItemStack.isSameItem.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z";
const S_STACKABLE: &str = "net/minecraft/world/item/ItemStack.isStackable.()Z";

/// The interned flag that a game calls [`player_tick`]: what clients send then waits for it.
const PLAYER_TICKS: &str = "compat:player-ticks";

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

fn truthy(v: V) -> bool {
    matches!(v, V::I(n) if n != 0)
}

// ---- stacks ----

pub(crate) fn empty(rt: &mut Rt) -> V {
    crate::api::get_static(rt, "net/minecraft/world/item/ItemStack.EMPTY")
}

fn is_empty(rt: &Rt, s: V) -> bool {
    crate::api_common::stack_empty(rt, s)
}

pub(crate) fn count(rt: &Rt, s: V) -> i32 {
    if is_empty(rt, s) { 0 } else { rt.get_field(s, "$count").i() }
}

/// A stack as tools show it: its item and count, None when empty.
pub fn stack_view(rt: &Rt, s: V) -> Option<(String, i32)> {
    (!is_empty(rt, s)).then(|| (crate::api_common::item_like_name(rt, rt.get_field(s, "$item")), count(rt, s)))
}

fn set_count(rt: &mut Rt, s: V, n: i32) {
    if s.r() != 0 {
        rt.put_field(s, "$count", V::I(n));
    }
}

fn grow(rt: &mut Rt, s: V, n: i32) {
    let c = rt.get_field(s, "$count").i();
    set_count(rt, s, c + n);
}

pub(crate) fn copy(rt: &mut Rt, s: V) -> V {
    crate::api::call(rt, S_COPY, &[s])
}

pub(crate) fn copy_with_count(rt: &mut Rt, s: V, n: i32) -> V {
    crate::api::call(rt, S_COPY_WITH, &[s, V::I(n)])
}

fn split(rt: &mut Rt, s: V, n: i32) -> V {
    crate::api::call(rt, S_SPLIT, &[s, V::I(n)])
}

pub(crate) fn max_size(rt: &mut Rt, s: V) -> i32 {
    crate::api::call(rt, S_MAX, &[s]).i()
}

pub(crate) fn matches(rt: &mut Rt, a: V, b: V) -> bool {
    truthy(crate::api::call(rt, S_MATCHES, &[a, b]))
}

pub(crate) fn same_components(rt: &mut Rt, a: V, b: V) -> bool {
    truthy(crate::api::call(rt, S_SAME_COMPONENTS, &[a, b]))
}

fn same_item(rt: &mut Rt, a: V, b: V) -> bool {
    truthy(crate::api::call(rt, S_SAME_ITEM, &[a, b]))
}

fn damaged(rt: &Rt, s: V) -> bool {
    crate::components::max_damage(rt, s) > 0 && crate::components::damage(rt, s) > 0
}

// ---- lists ----

fn new_list(rt: &mut Rt) -> V {
    rt.alloc(NON_NULL_LIST, Data::List(Vec::new()))
}

fn list_len(rt: &Rt, l: V) -> usize {
    match rt.heap.get(l.r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.len(),
        _ => 0,
    }
}

fn list_get(rt: &Rt, l: V, i: usize) -> V {
    match rt.heap.get(l.r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.get(i).copied().unwrap_or(V::NULL),
        _ => V::NULL,
    }
}

fn list_set(rt: &mut Rt, l: V, i: usize, x: V) {
    if let Some(Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data)
        && let Some(e) = v.get_mut(i)
    {
        *e = x;
    }
}

fn list_push(rt: &mut Rt, l: V, x: V) {
    if let Some(Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
        v.push(x);
    }
}

fn list_clear(rt: &mut Rt, l: V) {
    if let Some(Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
        v.clear();
    }
}

/// A virtual call: a mod's object answers with its own override, the API's natively here.
pub(crate) fn virt(rt: &mut Rt, key: &'static str, this: V, args: &[V]) -> V {
    if rt.is_mod_class(rt.class_of(this)) {
        return rt.call_functional(key, this, args);
    }
    let mut all = Vec::with_capacity(args.len() + 1);
    all.push(this);
    all.extend_from_slice(args);
    crate::api::call(rt, key, &all)
}

// ---- players ----

/// The player object of the server's player `uuid`: one for as long as the runtime runs, so what a
/// mod keeps on it (`containerMenu`) stays from one hook to the next.
pub fn server_player(rt: &mut Rt, uuid: u128) -> V {
    rt.intern(format!("compat:player/{uuid:x}"), move |rt| {
        let p = rt.new_object(SERVER_PLAYER);
        let u = crate::tags::uuid(rt, (uuid >> 64) as i64, uuid as i64);
        rt.put_field(p, "$uuid", u);
        init_player(rt, p);
        p
    })
}

/// A player object's menus, once: its inventory's (`inventoryMenu`, id 0), open as `containerMenu`.
pub fn init_player(rt: &mut Rt, p: V) {
    if p.r() == 0 || rt.get_field(p, "inventoryMenu").r() != 0 {
        return;
    }
    let m = rt.new_object(INVENTORY_MENU);
    init_menu(rt, m, V::NULL, 0);
    rt.put_field(p, "inventoryMenu", m);
    rt.put_field(p, "containerMenu", m);
}

/// `Player.getInventory()`: one object for each player object, its `selected` the hotbar slot the
/// host says (another player's own, where the host reaches them by UUID).
pub fn inventory(rt: &mut Rt, p: V) -> V {
    let mut inv = rt.get_field(p, "$inventory");
    if crate::fake_players::is_fake(rt, p) {
        return inv;
    }
    if inv.r() == 0 {
        inv = rt.new_object(INVENTORY);
        rt.put_field(inv, "player", p);
        if p.r() != 0 {
            rt.put_field(p, "$inventory", inv);
        }
    }
    let uuid = network::player_uuid(rt, p);
    let selected = crate::hand::for_player(rt, uuid, |rt| selected_slot(rt));
    rt.put_field(inv, "selected", V::I(selected));
    inv
}

/// The player `p` stands for: its own UUID (a server's player object, a payload's sender), else the
/// one the host acts for.
fn uuid_of(rt: &Rt, p: V) -> Option<u128> {
    network::player_uuid(rt, p).or_else(|| rt.host().and_then(|h| h.player_id()))
}

/// Whether `p` has a menu open other than its inventory's (`hasContainerOpen`).
fn menu_open(rt: &Rt, p: V) -> bool {
    let m = rt.get_field(p, "containerMenu");
    m.r() != 0 && m != rt.get_field(p, "inventoryMenu")
}

fn is_client_player(rt: &Rt, p: V) -> bool {
    rt.class_of(p) == LOCAL_PLAYER || crate::neo::is_client(rt)
}

/// `Player.hasInfiniteMaterials()`: a creative player (the acting one: another player's mode
/// isn't known here).
fn infinite(rt: &Rt) -> bool {
    fake_inventory().is_none() && crate::hand::other().is_none() && rt.host().is_some_and(|h| h.player_creative())
}

/// `Player.drop(stack, includeThrowerName)` on a server: an item entity from the player's eyes less
/// 0.3, thrown the way they look, picked up after 40 ticks, the whole stack (damage and components
/// too). A client's drops are the server's to make.
fn drop(rt: &mut Rt, stack: V) {
    if is_empty(rt, stack) || crate::neo::is_client(rt) {
        return;
    }
    if let Some(inv) = fake_inventory() {
        let p = rt.get_field(inv, "player");
        let whole = copy(rt, stack);
        fake_drop(rt, p, whole, false);
        set_count(rt, stack, 0);
        return;
    }
    let Some(whole) = crate::hand::hand_stack_of(rt, stack) else { return };
    let Some((feet, yaw, pitch)) = rt.host().and_then(|h| h.player_place()) else { return };
    let (at, motion) = throw(rt, feet, eye_height(rt), Some((yaw, pitch)), false);
    rt.host_call(|h| h.spawn_stack_entity(&whole, at, motion, 40));
    set_count(rt, stack, 0);
}

/// The acting player's eye height, as a float: standing, or crouching (`Player`'s poses).
fn eye_height(rt: &Rt) -> f32 {
    if rt.host().is_some_and(|h| h.player_sneaking()) { 1.27 } else { 1.62 }
}

/// Where a player's drop starts and how it's thrown (`LivingEntity.createItemStackToDrop`, 26.3): at
/// their eyes less 0.3, the way they look (`look`: yaw and pitch) with a little spread, or gently in
/// any direction (`around`, or no look known). In floats, with `Mth`'s sine table, drawing the
/// runtime's random numbers in vanilla's order.
fn throw(rt: &mut Rt, feet: [f64; 3], eye: f32, look: Option<(f32, f32)>, around: bool) -> ([f64; 3], [f64; 3]) {
    let unit = |rt: &mut Rt| (crate::api_common::next_random(rt) >> 40) as f32 / (1u32 << 24) as f32;
    let sin = |a: f32| crate::api_common::mth_sin(V::D(f64::from(a)));
    let cos = |a: f32| crate::api_common::mth_cos(V::D(f64::from(a)));
    const RADIANS: f32 = 0.017_453_292;
    const TURN: f32 = 6.283_185_5;
    let motion = match look {
        Some((yaw, pitch)) if !around => {
            let (sp, cp, sy, cy) = (sin(pitch * RADIANS), cos(pitch * RADIANS), sin(yaw * RADIANS), cos(yaw * RADIANS));
            let spread = unit(rt) * TURN;
            let strength = 0.02 * unit(rt);
            let up = -sp * 0.3 + 0.1 + (unit(rt) - unit(rt)) * 0.1;
            [
                f64::from(-sy * cp * 0.3) + f64::from(spread).cos() * f64::from(strength),
                f64::from(up),
                f64::from(cy * cp * 0.3) + f64::from(spread).sin() * f64::from(strength),
            ]
        }
        _ => {
            let reach = unit(rt) * 0.5;
            let angle = unit(rt) * TURN;
            [f64::from(-sin(angle) * reach), f64::from(0.2f32), f64::from(cos(angle) * reach)]
        }
    };
    ([feet[0], feet[1] + f64::from(eye) - f64::from(0.3f32), feet[2]], motion)
}

/// `Player.drop(stack, dropAround, ...)`: the stack as a new item entity at the player's eyes less
/// 0.3, picked up after 40 ticks, thrown the way they look with vanilla's spread (`dropAround`: in
/// any direction, gently), and, on a server, added to the level. Null for an empty stack, or no
/// player the game tells of. The stack stays the caller's, as vanilla's does.
///
/// Not modelled: the thrower it names, NeoForge's `ItemTossEvent`, and the drop statistics.
fn player_drop(rt: &mut Rt, p: V, stack: V, around: bool) -> V {
    if is_empty(rt, stack) {
        return V::NULL;
    }
    let uuid = crate::network::player_uuid(rt, p);
    let in_place = crate::attributes::entity_of(rt, p) == Ok(None);
    let Some(h) = rt.host() else { return V::NULL };
    let (feet, look, eye) = match (in_place, h.player_place()) {
        (true, Some((feet, yaw, pitch))) => (feet, Some((yaw, pitch)), eye_height(rt)),
        _ => match uuid.and_then(|u| h.players().into_iter().find(|q| q.0 == u)) {
            Some((_, feet)) => (feet, None, 1.62),
            None => return V::NULL,
        },
    };
    let (at, motion) = throw(rt, feet, eye, look, around);
    let e = crate::entities::unspawned(rt, at, stack, motion, 40);
    // A client's drop is the server's to make; the entity is only returned.
    if !is_client_player(rt, p) {
        let level = crate::glue::level(rt);
        crate::api::call(rt, ADD_FRESH_ENTITY, &[level, e]);
    }
    e
}

const ADD_FRESH_ENTITY: &str = "net/minecraft/world/level/Level.addFreshEntity.(Lnet/minecraft/world/entity/Entity;)Z";

/// [`player_drop`] for a fake player (`fake_players`): from where it stands and looks, into the
/// level it acts in.
pub(crate) fn fake_drop(rt: &mut Rt, p: V, stack: V, around: bool) -> V {
    if is_empty(rt, stack) || crate::neo::is_client(rt) {
        return V::NULL;
    }
    let (feet, yaw, pitch, eye) = crate::fake_players::stand(rt, p);
    let (at, motion) = throw(rt, feet, eye, Some((yaw, pitch)), around);
    let e = crate::entities::unspawned(rt, at, stack, motion, 40);
    let level = crate::glue::level(rt);
    crate::api::call(rt, ADD_FRESH_ENTITY, &[level, e]);
    e
}

// ---- menus ----

/// `AbstractContainerMenu`'s constructor: its type and id, and the lists and state vanilla keeps.
fn init_menu(rt: &mut Rt, m: V, ty: V, id: i32) {
    rt.put_field(m, "$type", ty);
    rt.put_field(m, "containerId", V::I(id));
    for f in ["slots", "$lastSlots", "$remoteSlots", "$dataSlots", "$remoteData", "$quickcraftSlots", "$listeners"] {
        let l = new_list(rt);
        rt.put_field(m, f, l);
    }
    let e = empty(rt);
    rt.put_field(m, "$carried", e);
    rt.put_field(m, "$remoteCarried", e);
    rt.put_field(m, "$stateId", V::I(0));
    rt.put_field(m, "$quickcraftType", V::I(-1));
    rt.put_field(m, "$quickcraftStatus", V::I(0));
}

pub(crate) fn slots(rt: &Rt, m: V) -> Vec<V> {
    crate::jutil::elements(rt, rt.get_field(m, "slots"))
}

pub(crate) fn slot_at(rt: &Rt, m: V, i: i32) -> V {
    if i < 0 {
        return V::NULL;
    }
    list_get(rt, rt.get_field(m, "slots"), i as usize)
}

pub(crate) fn carried(rt: &mut Rt, m: V) -> V {
    match rt.get_field(m, "$carried") {
        v if v.r() != 0 => v,
        _ => empty(rt),
    }
}

fn set_carried(rt: &mut Rt, m: V, s: V) {
    rt.put_field(m, "$carried", s);
}

pub(crate) fn slot_item(rt: &mut Rt, slot: V) -> V {
    let s = virt(rt, K_GET_ITEM, slot, &[]);
    if s.r() == 0 { empty(rt) } else { s }
}

pub(crate) fn has_item(rt: &mut Rt, slot: V) -> bool {
    truthy(virt(rt, K_HAS_ITEM, slot, &[]))
}

pub(crate) fn may_place(rt: &mut Rt, slot: V, s: V) -> bool {
    truthy(virt(rt, K_MAY_PLACE, slot, &[s]))
}

pub(crate) fn may_pickup(rt: &mut Rt, slot: V, player: V) -> bool {
    truthy(virt(rt, K_MAY_PICKUP, slot, &[player]))
}

fn set_by_player(rt: &mut Rt, slot: V, s: V) {
    virt(rt, K_SET_BY_PLAYER, slot, &[s]);
}

fn set_changed(rt: &mut Rt, slot: V) {
    virt(rt, K_SET_CHANGED, slot, &[]);
}

pub(crate) fn slot_max_for(rt: &mut Rt, slot: V, s: V) -> i32 {
    virt(rt, K_SLOT_MAX_FOR, slot, &[s]).i()
}

fn on_take(rt: &mut Rt, slot: V, player: V, s: V) {
    virt(rt, K_ON_TAKE, slot, &[player, s]);
}

fn safe_insert(rt: &mut Rt, slot: V, s: V, n: i32) -> V {
    virt(rt, K_SAFE_INSERT, slot, &[s, V::I(n)])
}

fn safe_take(rt: &mut Rt, slot: V, n: i32, decrement: i32, player: V) -> V {
    virt(rt, K_SAFE_TAKE, slot, &[V::I(n), V::I(decrement), player])
}

/// `slot.tryRemove(count, decrement, player)`: what came out, or None.
fn try_remove(rt: &mut Rt, slot: V, n: i32, decrement: i32, player: V) -> Option<V> {
    let o = virt(rt, K_TRY_REMOVE, slot, &[V::I(n), V::I(decrement), player]);
    let v = rt.get_field(o, "value");
    (v.r() != 0).then_some(v)
}

pub(crate) fn slot_index(rt: &Rt, slot: V) -> i32 {
    rt.get_field(slot, "index").i()
}

pub(crate) fn can_drag_to(rt: &mut Rt, m: V, slot: V) -> bool {
    truthy(virt(rt, K_DRAG_TO, m, &[slot]))
}

/// `AbstractContainerMenu.canItemQuickReplace(slot, stack, stackSizeMatters)`.
pub(crate) fn can_item_quick_replace(rt: &mut Rt, slot: V, s: V, size_matters: bool) -> bool {
    let free = slot.r() == 0 || !has_item(rt, slot);
    if free {
        return true;
    }
    let held = slot_item(rt, slot);
    if !same_components(rt, s, held) {
        return false;
    }
    let extra = if size_matters { 0 } else { count(rt, s) };
    count(rt, held) + extra <= max_size(rt, s)
}

/// `AbstractContainerMenu.getQuickCraftPlaceCount(slots, type, stack)`.
pub(crate) fn quick_craft_place_count(rt: &mut Rt, slots: usize, ty: i32, s: V) -> i32 {
    match ty {
        0 => (count(rt, s) as f32 / slots.max(1) as f32).floor() as i32,
        1 => 1,
        2 => max_size(rt, s),
        _ => count(rt, s),
    }
}

fn reset_quick_craft(rt: &mut Rt, m: V) {
    rt.put_field(m, "$quickcraftStatus", V::I(0));
    let l = rt.get_field(m, "$quickcraftSlots");
    list_clear(rt, l);
}

/// `AbstractContainerMenu.doClick`, as 1.21.1 runs it (the item overrides of a click, bundles', and
/// NeoForge's `ItemStackedOnOtherEvent` aside).
fn do_click(rt: &mut Rt, m: V, slot_id: i32, button: i32, click: i32, player: V) {
    let inv = inventory(rt, player);
    if click == QUICK_CRAFT {
        let before = rt.get_field(m, "$quickcraftStatus").i();
        let status = button & 3;
        rt.put_field(m, "$quickcraftStatus", V::I(status));
        let held = carried(rt, m);
        let quick = rt.get_field(m, "$quickcraftSlots");
        if (before != 1 || status != 2) && before != status {
            reset_quick_craft(rt, m);
        } else if is_empty(rt, held) {
            reset_quick_craft(rt, m);
        } else if status == 0 {
            let ty = button >> 2 & 3;
            rt.put_field(m, "$quickcraftType", V::I(ty));
            if ty == 0 || ty == 1 || (ty == 2 && infinite(rt)) {
                rt.put_field(m, "$quickcraftStatus", V::I(1));
                list_clear(rt, quick);
            } else {
                reset_quick_craft(rt, m);
            }
        } else if status == 1 {
            let slot = slot_at(rt, m, slot_id);
            let ty = rt.get_field(m, "$quickcraftType").i();
            let listed = crate::jutil::elements(rt, quick);
            if slot.r() != 0
                && can_item_quick_replace(rt, slot, held, true)
                && may_place(rt, slot, held)
                && (ty == 2 || count(rt, held) > listed.len() as i32)
                && can_drag_to(rt, m, slot)
                && !listed.contains(&slot)
            {
                list_push(rt, quick, slot);
            }
        } else if status == 2 {
            let listed = crate::jutil::elements(rt, quick);
            let ty = rt.get_field(m, "$quickcraftType").i();
            if !listed.is_empty() {
                if listed.len() == 1 {
                    let only = slot_index(rt, listed[0]);
                    reset_quick_craft(rt, m);
                    do_click(rt, m, only, ty, PICKUP, player);
                    return;
                }
                let spread = copy(rt, held);
                if is_empty(rt, spread) {
                    reset_quick_craft(rt, m);
                    return;
                }
                let mut left = count(rt, held);
                for slot in listed.iter().copied() {
                    let now = carried(rt, m);
                    if can_item_quick_replace(rt, slot, now, true)
                        && may_place(rt, slot, now)
                        && (ty == 2 || count(rt, now) >= listed.len() as i32)
                        && can_drag_to(rt, m, slot)
                    {
                        let had = if has_item(rt, slot) {
                            let s = slot_item(rt, slot);
                            count(rt, s)
                        } else {
                            0
                        };
                        let most = max_size(rt, spread).min(slot_max_for(rt, slot, spread));
                        let put = (quick_craft_place_count(rt, listed.len(), ty, spread) + had).min(most);
                        left -= put - had;
                        let placed = copy_with_count(rt, spread, put);
                        set_by_player(rt, slot, placed);
                    }
                }
                set_count(rt, spread, left);
                set_carried(rt, m, spread);
            }
            reset_quick_craft(rt, m);
        } else {
            reset_quick_craft(rt, m);
        }
    } else if rt.get_field(m, "$quickcraftStatus").i() != 0 {
        reset_quick_craft(rt, m);
    } else if (click == PICKUP || click == QUICK_MOVE) && (button == 0 || button == 1) {
        let primary = button == 0;
        if slot_id == OUTSIDE {
            let held = carried(rt, m);
            if !is_empty(rt, held) {
                if primary {
                    drop(rt, held);
                    let e = empty(rt);
                    set_carried(rt, m, e);
                } else {
                    let one = split(rt, held, 1);
                    drop(rt, one);
                }
            }
        } else if click == QUICK_MOVE {
            if slot_id < 0 {
                return;
            }
            let slot = slot_at(rt, m, slot_id);
            if slot.r() == 0 || !may_pickup(rt, slot, player) {
                return;
            }
            let mut moved = virt(rt, K_QUICK_MOVE, m, &[player, V::I(slot_id)]);
            while rt.thrown.is_none() && !is_empty(rt, moved) {
                let now = slot_item(rt, slot);
                if !same_item(rt, now, moved) {
                    break;
                }
                moved = virt(rt, K_QUICK_MOVE, m, &[player, V::I(slot_id)]);
            }
        } else {
            if slot_id < 0 {
                return;
            }
            let slot = slot_at(rt, m, slot_id);
            if slot.r() == 0 {
                return;
            }
            let there = slot_item(rt, slot);
            let held = carried(rt, m);
            if is_empty(rt, there) {
                if !is_empty(rt, held) {
                    let n = if primary { count(rt, held) } else { 1 };
                    let rest = safe_insert(rt, slot, held, n);
                    set_carried(rt, m, rest);
                }
            } else if may_pickup(rt, slot, player) {
                if is_empty(rt, held) {
                    let n = if primary { count(rt, there) } else { (count(rt, there) + 1) / 2 };
                    if let Some(taken) = try_remove(rt, slot, n, i32::MAX, player) {
                        set_carried(rt, m, taken);
                        on_take(rt, slot, player, taken);
                    }
                } else if may_place(rt, slot, held) {
                    if same_components(rt, there, held) {
                        let n = if primary { count(rt, held) } else { 1 };
                        let rest = safe_insert(rt, slot, held, n);
                        set_carried(rt, m, rest);
                    } else if count(rt, held) <= slot_max_for(rt, slot, held) {
                        set_carried(rt, m, there);
                        set_by_player(rt, slot, held);
                    }
                } else if same_components(rt, there, held) {
                    let room = max_size(rt, held) - count(rt, held);
                    let n = count(rt, there);
                    if let Some(taken) = try_remove(rt, slot, n, room, player) {
                        let got = count(rt, taken);
                        grow(rt, held, got);
                        on_take(rt, slot, player, taken);
                    }
                }
            }
            set_changed(rt, slot);
        }
    } else if click == SWAP && ((0..9).contains(&button) || button == 40) {
        let slot = slot_at(rt, m, slot_id);
        if slot.r() == 0 {
            return;
        }
        let hot = inventory_get(rt, button);
        let there = slot_item(rt, slot);
        if is_empty(rt, hot) && is_empty(rt, there) {
            return;
        }
        if is_empty(rt, hot) {
            if may_pickup(rt, slot, player) {
                inventory_set(rt, button, there);
                let n = count(rt, there);
                virt(rt, K_ON_SWAP_CRAFT, slot, &[V::I(n)]);
                let e = empty(rt);
                set_by_player(rt, slot, e);
                on_take(rt, slot, player, there);
            }
        } else if is_empty(rt, there) {
            if may_place(rt, slot, hot) {
                let most = slot_max_for(rt, slot, hot);
                if count(rt, hot) > most {
                    let part = split(rt, hot, most);
                    set_by_player(rt, slot, part);
                } else {
                    let e = empty(rt);
                    inventory_set(rt, button, e);
                    set_by_player(rt, slot, hot);
                }
            }
        } else if may_pickup(rt, slot, player) && may_place(rt, slot, hot) {
            let most = slot_max_for(rt, slot, hot);
            if count(rt, hot) > most {
                let part = split(rt, hot, most);
                set_by_player(rt, slot, part);
                on_take(rt, slot, player, there);
                if !inventory_add(rt, inv, -1, there) {
                    drop(rt, there);
                }
            } else {
                inventory_set(rt, button, there);
                set_by_player(rt, slot, hot);
                on_take(rt, slot, player, there);
            }
        }
    } else if click == CLONE && slot_id >= 0 {
        let held = carried(rt, m);
        if infinite(rt) && is_empty(rt, held) {
            let slot = slot_at(rt, m, slot_id);
            if slot.r() != 0 && has_item(rt, slot) {
                let s = slot_item(rt, slot);
                let n = max_size(rt, s);
                let c = copy_with_count(rt, s, n);
                set_carried(rt, m, c);
            }
        }
    } else if click == THROW && slot_id >= 0 {
        let held = carried(rt, m);
        let slot = slot_at(rt, m, slot_id);
        if is_empty(rt, held) && slot.r() != 0 {
            let n = if button == 0 {
                1
            } else {
                let s = slot_item(rt, slot);
                count(rt, s)
            };
            let thrown = safe_take(rt, slot, n, i32::MAX, player);
            drop(rt, thrown);
        }
    } else if click == PICKUP_ALL && slot_id >= 0 {
        let slot = slot_at(rt, m, slot_id);
        let held = carried(rt, m);
        if slot.r() != 0 && !is_empty(rt, held) && (!has_item(rt, slot) || !may_pickup(rt, slot, player)) {
            let all = slots(rt, m);
            let n = all.len() as i32;
            let (start, step) = if button == 0 { (0, 1) } else { (n - 1, -1) };
            for pass in 0..2 {
                let mut i = start;
                while i >= 0 && i < n && count(rt, held) < max_size(rt, held) {
                    let other = all[i as usize];
                    if has_item(rt, other)
                        && can_item_quick_replace(rt, other, held, true)
                        && may_pickup(rt, other, player)
                        && truthy(virt(rt, K_PICK_ALL, m, &[held, other]))
                    {
                        let s = slot_item(rt, other);
                        if pass != 0 || count(rt, s) != max_size(rt, s) {
                            let room = max_size(rt, held) - count(rt, held);
                            let n = count(rt, s);
                            let took = safe_take(rt, other, n, room, player);
                            let got = count(rt, took);
                            grow(rt, held, got);
                        }
                    }
                    i += step;
                }
            }
        }
    }
}

/// `AbstractContainerMenu.moveItemStackTo(stack, start, end, reverse)`: into slots that hold the
/// same item first, then the first empty one that takes it.
fn move_item_stack_to(rt: &mut Rt, m: V, s: V, start: i32, end: i32, reverse: bool) -> bool {
    let mut moved = false;
    let mut i = if reverse { end - 1 } else { start };
    if truthy(crate::api::call(rt, S_STACKABLE, &[s])) {
        while !is_empty(rt, s) && (if reverse { i >= start } else { i < end }) {
            let slot = slot_at(rt, m, i);
            let there = slot_item(rt, slot);
            if !is_empty(rt, there) && same_components(rt, s, there) {
                let total = count(rt, there) + count(rt, s);
                let most = slot_max_for(rt, slot, there);
                if total <= most {
                    set_count(rt, s, 0);
                    set_count(rt, there, total);
                    set_changed(rt, slot);
                    moved = true;
                } else if count(rt, there) < most {
                    let room = most - count(rt, there);
                    grow(rt, s, -room);
                    set_count(rt, there, most);
                    set_changed(rt, slot);
                    moved = true;
                }
            }
            i += if reverse { -1 } else { 1 };
        }
    }
    if !is_empty(rt, s) {
        i = if reverse { end - 1 } else { start };
        while if reverse { i >= start } else { i < end } {
            let slot = slot_at(rt, m, i);
            let there = slot_item(rt, slot);
            if is_empty(rt, there) && may_place(rt, slot, s) {
                let most = slot_max_for(rt, slot, s);
                let n = count(rt, s).min(most);
                let part = split(rt, s, n);
                set_by_player(rt, slot, part);
                set_changed(rt, slot);
                moved = true;
                break;
            }
            i += if reverse { -1 } else { 1 };
        }
    }
    moved
}

/// The player a menu keeps in step with (vanilla's synchronizer), once it was opened for them.
fn synced(rt: &Rt, m: V) -> Option<u128> {
    let p = rt.get_field(m, "$sync");
    if p.r() == 0 {
        return None;
    }
    network::player_uuid(rt, p)
}

fn next_state(rt: &mut Rt, m: V) -> i32 {
    let s = (rt.get_field(m, "$stateId").i() + 1) & 32767;
    rt.put_field(m, "$stateId", V::I(s));
    s
}

/// `broadcastChanges`: listeners hear of slots and data that changed since they last did, and the
/// client of what differs from what it has (one message for all of it).
fn broadcast_changes(rt: &mut Rt, m: V) {
    let all = slots(rt, m);
    let (last, remote) = (rt.get_field(m, "$lastSlots"), rt.get_field(m, "$remoteSlots"));
    let listeners = crate::jutil::elements(rt, rt.get_field(m, "$listeners"));
    let suppressed = truthy(rt.get_field(m, "$suppress"));
    let sync = synced(rt, m).is_some();
    let mut out: Vec<(i32, V)> = Vec::new();
    for (i, slot) in all.into_iter().enumerate() {
        let now = slot_item(rt, slot);
        let before = list_get(rt, last, i);
        if !matches(rt, before, now) {
            let c = copy(rt, now);
            list_set(rt, last, i, c);
            for l in &listeners {
                virt(rt, L_SLOT_CHANGED, *l, &[m, V::I(i as i32), c]);
            }
        }
        if !suppressed {
            let theirs = list_get(rt, remote, i);
            if !matches(rt, theirs, now) {
                let c = copy(rt, now);
                list_set(rt, remote, i, c);
                if sync {
                    next_state(rt, m);
                    out.push((i as i32, c));
                }
            }
        }
    }
    let mut carried_out = None;
    if !suppressed {
        let held = carried(rt, m);
        let theirs = rt.get_field(m, "$remoteCarried");
        if !matches(rt, held, theirs) {
            let c = copy(rt, held);
            rt.put_field(m, "$remoteCarried", c);
            if sync {
                next_state(rt, m);
                carried_out = Some(c);
            }
        }
    }
    let data = crate::jutil::elements(rt, rt.get_field(m, "$dataSlots"));
    let remote_data = rt.get_field(m, "$remoteData");
    let mut data_out = Vec::new();
    for (j, d) in data.into_iter().enumerate() {
        let v = virt(rt, D_GET, d, &[]).i();
        if truthy(virt(rt, D_CHECK, d, &[])) {
            for l in &listeners {
                virt(rt, L_DATA_CHANGED, *l, &[m, V::I(j as i32), V::I(v)]);
            }
        }
        if !suppressed && list_get(rt, remote_data, j).i() != v {
            list_set(rt, remote_data, j, V::I(v));
            data_out.push((j as i32, v));
        }
    }
    send_update(rt, m, &out, carried_out, &data_out);
}

/// `sendAllDataToRemote`: what the client has is taken to be everything, as it is sent now.
fn send_all_data(rt: &mut Rt, m: V) {
    let all = slots(rt, m);
    let remote = rt.get_field(m, "$remoteSlots");
    let mut out = Vec::with_capacity(all.len());
    for (i, slot) in all.into_iter().enumerate() {
        let now = slot_item(rt, slot);
        let c = copy(rt, now);
        list_set(rt, remote, i, c);
        out.push((i as i32, c));
    }
    let held = carried(rt, m);
    let c = copy(rt, held);
    rt.put_field(m, "$remoteCarried", c);
    let data = crate::jutil::elements(rt, rt.get_field(m, "$dataSlots"));
    let remote_data = rt.get_field(m, "$remoteData");
    let mut data_out = Vec::new();
    for (j, d) in data.into_iter().enumerate() {
        let v = virt(rt, D_GET, d, &[]).i();
        list_set(rt, remote_data, j, V::I(v));
        data_out.push((j as i32, v));
    }
    if synced(rt, m).is_some() {
        next_state(rt, m);
        send_update(rt, m, &out, Some(c), &data_out);
    }
}

/// An encoded slot entry: its index, then its stack.
fn slot_entry(rt: &mut Rt, i: i32, s: V) -> Vec<u8> {
    let b = network::new_buffer(rt);
    network::put_var(rt, b, u64::from(i as u32));
    network::put_stack(rt, b, s);
    network::bytes(rt, b)
}

/// Sends the menu's client what changed: as many messages as the slots need, the carried stack and
/// the data values in the last.
fn send_update(rt: &mut Rt, m: V, changed: &[(i32, V)], carried: Option<V>, data: &[(i32, i32)]) {
    let Some(uuid) = synced(rt, m) else { return };
    if changed.is_empty() && carried.is_none() && data.is_empty() {
        return;
    }
    let id = rt.get_field(m, "containerId").i();
    let state = rt.get_field(m, "$stateId").i();
    let entries: Vec<Vec<u8>> = changed.iter().map(|&(i, s)| slot_entry(rt, i, s)).collect();
    let mut start = 0;
    loop {
        let mut size = 0;
        let mut end = start;
        while end < entries.len() && (end == start || size + entries[end].len() <= MESSAGE_BUDGET) {
            size += entries[end].len();
            end += 1;
        }
        let last = end >= entries.len();
        let b = network::new_buffer(rt);
        network::put(rt, b, &[UPDATE]);
        network::put_var(rt, b, u64::from(id as u32));
        network::put_var(rt, b, u64::from(state as u32));
        let with_carried = last && carried.is_some();
        network::put(rt, b, &[u8::from(with_carried)]);
        network::put_var(rt, b, (end - start) as u64);
        for e in &entries[start..end] {
            network::put(rt, b, e);
        }
        if let (true, Some(c)) = (with_carried, carried) {
            network::put_stack(rt, b, c);
        }
        let data = if last { data } else { &[] };
        network::put_var(rt, b, data.len() as u64);
        for &(j, v) in data {
            // As vanilla's packet: both shorts.
            network::put(rt, b, &(j as i16).to_be_bytes());
            network::put(rt, b, &(v as i16).to_be_bytes());
        }
        let bytes = network::bytes(rt, b);
        network::send_raw(rt, PayloadTarget::Player(uuid), CHANNEL, &bytes);
        start = end;
        if last {
            break;
        }
    }
}

/// The calls of `AbstractContainerMenu` itself.
fn menu_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let m = a(0);
    Some(match (name, desc) {
        ("<init>", _) => {
            init_menu(rt, m, a(1), a(2).i());
            V::Void
        }
        ("getType", _) => rt.get_field(m, "$type"),
        ("isValidSlotIndex", _) => {
            let i = a(1).i();
            V::I((i == -1 || i == OUTSIDE || (i >= 0 && (i as usize) < list_len(rt, rt.get_field(m, "slots")))) as i32)
        }
        ("addSlot", _) => {
            let slots = rt.get_field(m, "slots");
            let n = list_len(rt, slots) as i32;
            rt.put_field(a(1), "index", V::I(n));
            list_push(rt, slots, a(1));
            let e = empty(rt);
            for f in ["$lastSlots", "$remoteSlots"] {
                let l = rt.get_field(m, f);
                list_push(rt, l, e);
            }
            a(1)
        }
        ("addDataSlot", _) => {
            let l = rt.get_field(m, "$dataSlots");
            list_push(rt, l, a(1));
            let r = rt.get_field(m, "$remoteData");
            list_push(rt, r, V::I(0));
            a(1)
        }
        ("addDataSlots", _) => {
            let n = virt(rt, CD_COUNT, a(1), &[]).i();
            for i in 0..n {
                let d = data_slot(rt, 1, a(1), i);
                let l = rt.get_field(m, "$dataSlots");
                list_push(rt, l, d);
                let r = rt.get_field(m, "$remoteData");
                list_push(rt, r, V::I(0));
            }
            V::Void
        }
        ("addSlotListener", _) => {
            let l = rt.get_field(m, "$listeners");
            if !crate::jutil::elements(rt, l).contains(&a(1)) {
                list_push(rt, l, a(1));
                virt(rt, K_BROADCAST, m, &[]);
            }
            V::Void
        }
        ("removeSlotListener", _) => {
            let l = rt.get_field(m, "$listeners");
            if let Some(Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
                v.retain(|x| *x != a(1));
            }
            V::Void
        }
        ("setSynchronizer", _) => {
            send_all_data(rt, m);
            V::Void
        }
        ("sendAllDataToRemote", _) => {
            send_all_data(rt, m);
            V::Void
        }
        ("getItems", _) => {
            let mut items = Vec::new();
            for slot in slots(rt, m) {
                items.push(slot_item(rt, slot));
            }
            rt.alloc(NON_NULL_LIST, Data::List(items))
        }
        ("broadcastChanges", _) => {
            broadcast_changes(rt, m);
            V::Void
        }
        ("broadcastFullState", _) => {
            let last = rt.get_field(m, "$lastSlots");
            let listeners = crate::jutil::elements(rt, rt.get_field(m, "$listeners"));
            for (i, slot) in slots(rt, m).into_iter().enumerate() {
                let now = slot_item(rt, slot);
                let before = list_get(rt, last, i);
                if !matches(rt, before, now) {
                    let c = copy(rt, now);
                    list_set(rt, last, i, c);
                    for l in &listeners {
                        virt(rt, L_SLOT_CHANGED, *l, &[m, V::I(i as i32), c]);
                    }
                }
            }
            send_all_data(rt, m);
            V::Void
        }
        ("setRemoteSlot", _) => {
            let c = copy(rt, a(2));
            let l = rt.get_field(m, "$remoteSlots");
            list_set(rt, l, a(1).i().max(0) as usize, c);
            V::Void
        }
        ("setRemoteSlotNoCopy", _) => {
            let l = rt.get_field(m, "$remoteSlots");
            if a(1).i() >= 0 {
                list_set(rt, l, a(1).i() as usize, a(2));
            }
            V::Void
        }
        ("setRemoteCarried", _) => {
            let c = copy(rt, a(1));
            rt.put_field(m, "$remoteCarried", c);
            V::Void
        }
        ("clickMenuButton", _) => V::I(0),
        ("getSlot", _) => {
            let s = slot_at(rt, m, a(1).i());
            if s.r() == 0 {
                rt.throw_new("java/lang/IndexOutOfBoundsException");
            }
            s
        }
        // Abstract in vanilla: a menu without its own moves nothing.
        ("quickMoveStack", _) => empty(rt),
        ("clicked", _) => {
            let click = rt.get_field(a(3), "$ordinal").i();
            crate::hand::with_hand(rt, |rt| do_click(rt, m, a(1).i(), a(2).i(), click, a(4)));
            V::Void
        }
        ("canTakeItemForPickAll" | "canDragTo", _) => V::I(1),
        // On a server, the carried stack goes back into the inventory.
        ("removed", _) => {
            let held = carried(rt, m);
            if !is_client_player(rt, a(1)) && !is_empty(rt, held) {
                let inv = inventory(rt, a(1));
                crate::hand::with_hand(rt, |rt| place_back(rt, inv, held));
                let e = empty(rt);
                set_carried(rt, m, e);
            }
            V::Void
        }
        ("clearContainer", _) => {
            let (player, c) = (a(1), a(2));
            if !is_client_player(rt, player) {
                let n = virt(rt, C_SIZE, c, &[]).i();
                let inv = inventory(rt, player);
                for i in 0..n {
                    let s = virt(rt, "net/minecraft/world/Container.removeItemNoUpdate.(I)Lnet/minecraft/world/item/ItemStack;", c, &[V::I(i)]);
                    crate::hand::with_hand(rt, |rt| place_back(rt, inv, s));
                }
            }
            V::Void
        }
        ("slotsChanged", _) => {
            virt(rt, K_BROADCAST, m, &[]);
            V::Void
        }
        ("setItem", _) => {
            let slot = slot_at(rt, m, a(1).i());
            if slot.r() != 0 {
                virt(rt, K_SET, slot, &[a(3)]);
            }
            rt.put_field(m, "$stateId", a(2));
            V::Void
        }
        ("initializeContents", _) => {
            for (i, s) in crate::jutil::elements(rt, a(2)).into_iter().enumerate() {
                let slot = slot_at(rt, m, i as i32);
                if slot.r() != 0 {
                    virt(rt, K_SET, slot, &[s]);
                }
            }
            rt.put_field(m, "$carried", a(3));
            rt.put_field(m, "$stateId", a(1));
            V::Void
        }
        ("setData", _) => {
            let d = list_get(rt, rt.get_field(m, "$dataSlots"), a(1).i().max(0) as usize);
            if d.r() != 0 {
                virt(rt, D_SET, d, &[a(2)]);
            }
            V::Void
        }
        // Abstract in vanilla.
        ("stillValid", "(Lnet/minecraft/world/entity/player/Player;)Z") => V::I(1),
        // static stillValid(access, player, block): the block is still there and within reach.
        ("stillValid", _) => V::I(still_valid_block(rt, a(0), a(1), a(2)) as i32),
        ("moveItemStackTo", _) => V::I(move_item_stack_to(rt, m, a(1), a(2).i(), a(3).i(), truthy(a(4))) as i32),
        ("getQuickcraftType", _) => V::I(a(0).i() >> 2 & 3),
        ("getQuickcraftHeader", _) => V::I(a(0).i() & 3),
        ("getQuickcraftMask", _) => V::I(a(0).i() & 3 | (a(1).i() & 3) << 2),
        ("isValidQuickcraftType", _) => V::I(matches!(a(0).i(), 0 | 1) as i32 | (a(0).i() == 2 && infinite(rt)) as i32),
        ("resetQuickCraft", _) => {
            reset_quick_craft(rt, m);
            V::Void
        }
        ("canItemQuickReplace", _) => V::I(can_item_quick_replace(rt, a(0), a(1), truthy(a(2))) as i32),
        ("getQuickCraftPlaceCount", _) => {
            let n = crate::jutil::elements(rt, a(0)).len();
            V::I(quick_craft_place_count(rt, n, a(1).i(), a(2)))
        }
        ("getRedstoneSignalFromBlockEntity" | "getRedstoneSignalFromContainer", _) => {
            let c = a(0);
            if c.r() == 0 || !rt.instance_of(c, CONTAINER) {
                return Some(V::I(0));
            }
            let n = virt(rt, C_SIZE, c, &[]).i();
            let mut f = 0.0f32;
            for i in 0..n {
                let s = virt(rt, C_GET, c, &[V::I(i)]);
                if !is_empty(rt, s) {
                    let most = virt(rt, C_MAX_FOR, c, &[s]).i().max(1);
                    f += count(rt, s) as f32 / most as f32;
                }
            }
            f /= n.max(1) as f32;
            // `Mth.lerpDiscrete(f, 0, 15)`.
            V::I((f * 14.0).floor() as i32 + i32::from(f > 0.0))
        }
        ("setCarried", _) => {
            set_carried(rt, m, a(1));
            V::Void
        }
        ("getCarried", _) => carried(rt, m),
        ("suppressRemoteUpdates", _) => {
            rt.put_field(m, "$suppress", V::I(1));
            V::Void
        }
        ("resumeRemoteUpdates", _) => {
            rt.put_field(m, "$suppress", V::I(0));
            V::Void
        }
        ("transferState", _) => V::Void,
        ("findSlot", _) => {
            for (i, slot) in slots(rt, m).into_iter().enumerate() {
                if rt.get_field(slot, "container") == a(1) && rt.get_field(slot, "$slot").i() == a(2).i() {
                    return Some(optional_int(rt, Some(i as i32)));
                }
            }
            optional_int(rt, None)
        }
        ("getStateId", _) => rt.get_field(m, "$stateId"),
        ("incrementStateId", _) => V::I(next_state(rt, m)),
        ("checkContainerSize", _) => {
            if virt(rt, C_SIZE, a(0), &[]).i() < a(1).i() {
                rt.throw_new("java/lang/IllegalArgumentException");
            }
            V::Void
        }
        ("checkContainerDataCount", _) => {
            if virt(rt, CD_COUNT, a(0), &[]).i() < a(1).i() {
                rt.throw_new("java/lang/IllegalArgumentException");
            }
            V::Void
        }
        _ => return None,
    })
}

/// `AbstractContainerMenu.stillValid(access, player, block)`: the block is still at the access's
/// position and the player within 4 blocks' reach of it (an access with no place: true).
fn still_valid_block(rt: &mut Rt, access: V, player: V, block: V) -> bool {
    let pos = rt.get_field(access, "$pos");
    if rt.get_field(access, "$level").r() == 0 || pos.r() == 0 {
        return true;
    }
    let p = crate::api::pos_of(rt, pos);
    let state = rt.host().map(|h| h.block_state(p)).unwrap_or_default();
    let (there, _) = crate::host::split_state(&state);
    if there != crate::api_common::item_like_name(rt, block) {
        return false;
    }
    truthy(crate::api::call(rt, "net/minecraft/world/entity/player/Player.canInteractWithBlock.(Lnet/minecraft/core/BlockPos;D)Z", &[player, pos, V::D(4.0)]))
}

// ---- slots ----

/// An empty container standing for a `SlotItemHandler`'s (NeoForge's `emptyInventory`).
fn empty_container(rt: &mut Rt) -> V {
    rt.intern("compat:empty-container".into(), |rt| {
        let c = rt.new_object(SIMPLE_CONTAINER);
        let items = new_list(rt);
        rt.put_field(c, "$items", items);
        c
    })
}

/// The calls of NeoForge's `SlotItemHandler` that differ from vanilla's `Slot`: its stack is the
/// item handler's (`$handler`, slot `$slot`).
fn handler_slot_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    let h = rt.get_field(this, "$handler");
    let i = rt.get_field(this, "$slot");
    Some(match (name, desc) {
        ("<init>", "(Lnet/neoforged/neoforge/items/IItemHandler;III)V") => {
            let c = empty_container(rt);
            rt.put_field(this, "container", c);
            rt.put_field(this, "$handler", a(1));
            rt.put_field(this, "$slot", a(2));
            rt.put_field(this, "x", a(3));
            rt.put_field(this, "y", a(4));
            V::Void
        }
        ("mayPlace", _) => {
            if is_empty(rt, a(1)) {
                V::I(0)
            } else {
                V::I(truthy(virt(rt, H_VALID, h, &[i, a(1)])) as i32)
            }
        }
        ("getItem", _) => virt(rt, H_GET, h, &[i]),
        ("set" | "initialize", _) => {
            virt(rt, H_SET, h, &[i, a(1)]);
            virt(rt, K_SET_CHANGED, this, &[]);
            V::Void
        }
        ("onQuickCraft", "(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)V") => V::Void,
        ("getMaxStackSize", "()I") => virt(rt, H_LIMIT, h, &[i]),
        ("getMaxStackSize", _) => {
            let limit = virt(rt, H_LIMIT, h, &[i]).i();
            V::I(max_size(rt, a(1)).min(limit))
        }
        ("mayPickup", _) => {
            let out = virt(rt, H_EXTRACT, h, &[i, V::I(1), V::I(1)]);
            V::I(!is_empty(rt, out) as i32)
        }
        ("remove", _) => virt(rt, H_EXTRACT, h, &[i, a(1), V::I(0)]),
        ("getItemHandler", _) => h,
        ("isSameInventory", _) => V::I((rt.instance_of(a(1), HANDLER_SLOT) && rt.get_field(a(1), "$handler") == h) as i32),
        _ => return None,
    })
}

/// The calls of vanilla's `Slot` (and a `SlotItemHandler`'s, past what it overrides).
fn slot_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    if (owner == HANDLER_SLOT || rt.instance_of(this, HANDLER_SLOT))
        && let Some(v) = handler_slot_call(rt, name, desc, args)
    {
        return Some(v);
    }
    let c = rt.get_field(this, "container");
    let i = rt.get_field(this, "$slot");
    Some(match (name, desc) {
        ("<init>", "(Lnet/minecraft/world/Container;III)V") => {
            rt.put_field(this, "container", a(1));
            rt.put_field(this, "$slot", a(2));
            rt.put_field(this, "x", a(3));
            rt.put_field(this, "y", a(4));
            V::Void
        }
        ("onQuickCraft", "(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)V") => {
            let n = count(rt, a(2)) - count(rt, a(1));
            if n > 0 {
                virt(rt, K_ON_QUICK_CRAFT_N, this, &[a(2), V::I(n)]);
            }
            V::Void
        }
        ("onQuickCraft" | "onSwapCraft" | "checkTakeAchievements", _) => V::Void,
        ("onTake", _) => {
            virt(rt, K_SET_CHANGED, this, &[]);
            V::Void
        }
        ("mayPlace" | "mayPickup" | "isActive" | "isHighlightable", _) => V::I(1),
        ("isFake", _) => V::I(0),
        ("getItem", _) => {
            let s = virt(rt, C_GET, c, &[i]);
            if s.r() == 0 { empty(rt) } else { s }
        }
        ("hasItem", _) => {
            let s = slot_item(rt, this);
            V::I(!is_empty(rt, s) as i32)
        }
        ("setByPlayer", "(Lnet/minecraft/world/item/ItemStack;)V") => {
            let old = slot_item(rt, this);
            virt(rt, K_SET_BY_PLAYER_OLD, this, &[a(1), old]);
            V::Void
        }
        ("setByPlayer", _) => {
            virt(rt, K_SET, this, &[a(1)]);
            V::Void
        }
        ("set", _) => {
            virt(rt, C_SET, c, &[i, a(1)]);
            virt(rt, K_SET_CHANGED, this, &[]);
            V::Void
        }
        ("setChanged", _) => {
            if c.r() != 0 {
                virt(rt, C_CHANGED, c, &[]);
            }
            V::Void
        }
        ("getMaxStackSize", "()I") => virt(rt, C_MAX, c, &[]),
        ("getMaxStackSize", _) => {
            let own = virt(rt, K_SLOT_MAX, this, &[]).i();
            V::I(own.min(max_size(rt, a(1))))
        }
        ("getNoItemIcon", _) => {
            let (atlas, sprite) = (rt.get_field(this, "$iconAtlas"), rt.get_field(this, "$icon"));
            if sprite.r() == 0 {
                V::NULL
            } else {
                crate::api::call(rt, "com/mojang/datafixers/util/Pair.of.(Ljava/lang/Object;Ljava/lang/Object;)Lcom/mojang/datafixers/util/Pair;", &[atlas, sprite])
            }
        }
        ("setBackground", _) => {
            rt.put_field(this, "$iconAtlas", a(1));
            rt.put_field(this, "$icon", a(2));
            this
        }
        ("remove", _) => virt(rt, C_REMOVE, c, &[i, a(1)]),
        ("getSlotIndex" | "getContainerSlot", _) => V::I(i.i()),
        ("isSameInventory", _) => V::I((c == rt.get_field(a(1), "container")) as i32),
        ("tryRemove", _) => {
            let out = slot_try_remove(rt, this, a(1).i(), a(2).i(), a(3));
            crate::api_common::optional(rt, out.unwrap_or(V::NULL))
        }
        ("safeTake", _) => {
            let out = slot_try_remove(rt, this, a(1).i(), a(2).i(), a(3));
            match out {
                Some(s) => {
                    on_take(rt, this, a(3), s);
                    s
                }
                None => empty(rt),
            }
        }
        ("safeInsert", "(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/ItemStack;") => {
            let n = count(rt, a(1));
            virt(rt, K_SAFE_INSERT, this, &[a(1), V::I(n)])
        }
        ("safeInsert", _) => {
            let s = a(1);
            if !is_empty(rt, s) && may_place(rt, this, s) {
                let there = slot_item(rt, this);
                let n = a(2).i().min(count(rt, s)).min(slot_max_for(rt, this, s) - count(rt, there));
                if is_empty(rt, there) {
                    let part = split(rt, s, n);
                    set_by_player(rt, this, part);
                } else if same_components(rt, there, s) {
                    grow(rt, s, -n);
                    grow(rt, there, n);
                    set_by_player(rt, this, there);
                }
            }
            s
        }
        ("allowModification", _) => {
            let s = slot_item(rt, this);
            V::I((may_pickup(rt, this, a(1)) && may_place(rt, this, s)) as i32)
        }
        _ => return None,
    })
}

/// `Slot.tryRemove(count, decrement, player)`.
fn slot_try_remove(rt: &mut Rt, slot: V, n: i32, decrement: i32, player: V) -> Option<V> {
    if !may_pickup(rt, slot, player) {
        return None;
    }
    let there = slot_item(rt, slot);
    if !truthy(virt(rt, K_ALLOW_MODIFICATION, slot, &[player])) && decrement < count(rt, there) {
        return None;
    }
    let out = virt(rt, K_REMOVE, slot, &[V::I(n.min(decrement))]);
    if is_empty(rt, out) {
        return None;
    }
    let now = slot_item(rt, slot);
    if is_empty(rt, now) {
        let e = empty(rt);
        virt(rt, K_SET_BY_PLAYER_OLD, slot, &[e, out]);
    }
    Some(out)
}

// ---- the player's inventory ----

/// `Inventory.getItem(slot)`: the acting player's (or the player's [`crate::hand::for_player`]
/// reaches), by vanilla's numbering.
pub(crate) fn inventory_get(rt: &mut Rt, slot: i32) -> V {
    if let Some(inv) = fake_inventory() {
        return crate::fake_players::slot(rt, inv, slot);
    }
    match u8::try_from(slot) {
        Ok(s) if s <= 40 => crate::hand::slot_stack(rt, s),
        _ => empty(rt),
    }
}

pub(crate) fn inventory_set(rt: &mut Rt, slot: i32, s: V) {
    if let Some(inv) = fake_inventory() {
        return crate::fake_players::set_slot(rt, inv, slot, s);
    }
    if let Ok(i) = u8::try_from(slot)
        && i <= 40
    {
        crate::hand::replace_slot(rt, i, s);
    }
}

/// Whether the host reaches the whole inventory, so stacks go where vanilla's `Inventory` puts them:
/// another player's by UUID, every slot.
fn whole_inventory(rt: &Rt) -> bool {
    fake_inventory().is_some() || crate::hand::other().is_some() || rt.host().is_some_and(|h| h.player_inventory())
}

thread_local! {
    /// The fake player's inventory (`fake_players`) an `Inventory` call is on, while it runs: its
    /// slots are its own, never the acting player's.
    static FAKE_INVENTORY: std::cell::Cell<V> = const { std::cell::Cell::new(V::NULL) };
}

fn fake_inventory() -> Option<V> {
    Some(FAKE_INVENTORY.get()).filter(|v| v.r() != 0)
}

/// The hotbar slot in hand: a fake player's own, another player's the host reaches by UUID, else
/// the acting player's.
fn selected_slot(rt: &Rt) -> i32 {
    match (fake_inventory(), crate::hand::other()) {
        (Some(inv), _) => crate::fake_players::selected(rt, inv),
        (None, Some(other)) => i32::from(other.selected),
        (None, None) => rt.host().map_or(0, |h| i32::from(h.player_selected().min(8))),
    }
}

/// `Inventory.getMaxStackSize(stack)`: the inventory's 99 or the stack's own, the lesser.
fn inventory_max_for(rt: &mut Rt, s: V) -> i32 {
    max_size(rt, s).min(99)
}

fn has_room_for(rt: &mut Rt, there: V, s: V) -> bool {
    !is_empty(rt, there)
        && same_components(rt, there, s)
        && truthy(crate::api::call(rt, S_STACKABLE, &[there]))
        && count(rt, there) < inventory_max_for(rt, there)
}

/// `Inventory.getSlotWithRemainingSpace(stack)`: the selected slot, the off hand, then the first
/// slot that holds the same and has room.
fn slot_with_room(rt: &mut Rt, s: V) -> i32 {
    let selected = selected_slot(rt);
    for i in [selected, 40].into_iter().chain(0..36) {
        let there = inventory_get(rt, i);
        if has_room_for(rt, there, s) {
            return i;
        }
    }
    -1
}

/// `Inventory.getFreeSlot()`: the first empty slot of the 36.
fn free_slot(rt: &mut Rt) -> i32 {
    for i in 0..36 {
        let there = inventory_get(rt, i);
        if is_empty(rt, there) {
            return i;
        }
    }
    -1
}

/// `Inventory.addResource(slot, stack)`: what of the stack's count doesn't fit in `slot`.
fn add_resource_at(rt: &mut Rt, slot: i32, s: V) -> i32 {
    let n = count(rt, s);
    let mut there = inventory_get(rt, slot);
    if is_empty(rt, there) {
        there = copy_with_count(rt, s, 0);
        inventory_set(rt, slot, there);
    }
    let room = inventory_max_for(rt, there) - count(rt, there);
    let k = n.min(room);
    if k <= 0 {
        return n;
    }
    grow(rt, there, k);
    n - k
}

fn add_resource(rt: &mut Rt, s: V) -> i32 {
    let mut i = slot_with_room(rt, s);
    if i == -1 {
        i = free_slot(rt);
    }
    if i == -1 { count(rt, s) } else { add_resource_at(rt, i, s) }
}

/// `Inventory.add(slot, stack)` (-1 for anywhere): the stack shrinks by what went in; whether some
/// did. A host that knows only the hands has the game place it.
fn inventory_add(rt: &mut Rt, _inv: V, slot: i32, s: V) -> bool {
    if is_empty(rt, s) {
        return false;
    }
    if !whole_inventory(rt) {
        let before = count(rt, s);
        crate::api_common::give_player(rt, s);
        return count(rt, s) < before;
    }
    crate::hand::with_hand(rt, |rt| {
        if damaged(rt, s) {
            let at = if slot == -1 { free_slot(rt) } else { slot };
            if at >= 0 {
                let moved = copy(rt, s);
                set_count(rt, s, 0);
                inventory_set(rt, at, moved);
                return true;
            }
            if infinite(rt) {
                set_count(rt, s, 0);
                return true;
            }
            return false;
        }
        let mut before;
        loop {
            before = count(rt, s);
            let left = if slot == -1 { add_resource(rt, s) } else { add_resource_at(rt, slot, s) };
            set_count(rt, s, left);
            if is_empty(rt, s) || count(rt, s) >= before {
                break;
            }
        }
        if count(rt, s) == before && infinite(rt) {
            set_count(rt, s, 0);
            return true;
        }
        count(rt, s) < before
    })
}

/// `Inventory.placeItemBackInInventory(stack)`: where it fits, the rest dropped at the player's feet.
fn place_back(rt: &mut Rt, inv: V, s: V) {
    if !whole_inventory(rt) {
        crate::api_common::give_player(rt, s);
        drop(rt, s);
        return;
    }
    while !is_empty(rt, s) {
        let mut i = slot_with_room(rt, s);
        if i == -1 {
            i = free_slot(rt);
        }
        if i == -1 {
            drop(rt, s);
            break;
        }
        let there = inventory_get(rt, i);
        let n = max_size(rt, s) - count(rt, there);
        let part = split(rt, s, n);
        inventory_add(rt, inv, i, part);
    }
}

/// The calls of a player's `Inventory`: a fake player's on its own slots, another player's than the
/// acting one's on theirs where the host reaches them by UUID (none acts in a block entity's tick,
/// where Create's toolbox hands a player their tool), else the acting player's.
fn inventory_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let inv = arg(args, 0);
    let fake = if crate::fake_players::is_fake_inventory(rt, inv) { inv } else { V::NULL };
    let outer = FAKE_INVENTORY.replace(fake);
    let uuid = if fake.r() == 0 { network::player_uuid(rt, rt.get_field(inv, "player")) } else { None };
    let out = crate::hand::for_player(rt, uuid, |rt| player_inventory_call(rt, name, desc, args));
    FAKE_INVENTORY.set(outer);
    out
}

const PUT_BYTE: &str = "net/minecraft/nbt/CompoundTag.putByte.(Ljava/lang/String;B)V";
const GET_BYTE: &str = "net/minecraft/nbt/CompoundTag.getByte.(Ljava/lang/String;)B";
const LIST_ADD: &str = "net/minecraft/nbt/ListTag.add.(Ljava/lang/Object;)Z";
const LIST_SIZE: &str = "net/minecraft/nbt/ListTag.size.()I";
const LIST_COMPOUND: &str = "net/minecraft/nbt/ListTag.getCompound.(I)Lnet/minecraft/nbt/CompoundTag;";
const STACK_SAVE: &str = "net/minecraft/world/item/ItemStack.save.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;";
const STACK_PARSE: &str = "net/minecraft/world/item/ItemStack.parseOptional.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/world/item/ItemStack;";

/// Vanilla's number in `Inventory.save`'s `Slot` for slot `i` of ours: the 36 as they are, the
/// armor's from 100, the off hand 150.
fn saved_slot(i: i32) -> i32 {
    match i {
        36..=39 => i - 36 + 100,
        40 => 150,
        _ => i,
    }
}

/// `Inventory.save(list)`: each stack that isn't empty, with its `Slot`, in the list.
fn inventory_save(rt: &mut Rt, list: V) -> V {
    for i in 0..=40 {
        let s = inventory_get(rt, i);
        if is_empty(rt, s) {
            continue;
        }
        let tag = crate::tags::new_compound(rt);
        let key = rt.string("Slot");
        crate::api::call(rt, PUT_BYTE, &[tag, key, V::I(i32::from(saved_slot(i) as u8 as i8))]);
        let saved = crate::api::call(rt, STACK_SAVE, &[s, V::NULL, tag]);
        crate::api::call(rt, LIST_ADD, &[list, saved]);
    }
    list
}

/// `Inventory.load(list)`: every slot empty, then each stack the list names in its slot.
fn inventory_load(rt: &mut Rt, list: V) {
    for i in 0..=40 {
        let e = empty(rt);
        inventory_set(rt, i, e);
    }
    let n = crate::api::call(rt, LIST_SIZE, &[list]).i();
    for k in 0..n {
        let tag = crate::api::call(rt, LIST_COMPOUND, &[list, V::I(k)]);
        let key = rt.string("Slot");
        let slot = crate::api::call(rt, GET_BYTE, &[tag, key]).i() & 255;
        let at = match slot {
            0..=35 => slot,
            100..=103 => slot - 100 + 36,
            150 => 40,
            _ => continue,
        };
        let s = crate::api::call(rt, STACK_PARSE, &[V::NULL, tag]);
        inventory_set(rt, at, s);
    }
}

/// The calls of a player's `Inventory`, on the slots [`inventory_get`] reaches.
fn player_inventory_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let inv = a(0);
    Some(match (name, desc) {
        ("<init>", _) => {
            rt.put_field(inv, "player", a(1));
            rt.put_field(inv, "selected", V::I(0));
            V::Void
        }
        ("player", _) => rt.get_field(inv, "player"),
        ("getItem", _) => inventory_get(rt, a(1).i()),
        ("setItem", _) => {
            inventory_set(rt, a(1).i(), a(2));
            V::Void
        }
        ("getSelected", _) => {
            let selected = selected_slot(rt);
            inventory_get(rt, selected)
        }
        ("getArmor", _) => inventory_get(rt, 36 + a(1).i()),
        ("removeItem", "(II)Lnet/minecraft/world/item/ItemStack;") => {
            let there = inventory_get(rt, a(1).i());
            if is_empty(rt, there) || a(2).i() <= 0 {
                empty(rt)
            } else {
                crate::hand::with_hand(rt, |rt| {
                    // The slot is reached in this hook, so what's left reaches the game.
                    let there = inventory_get(rt, a(1).i());
                    split(rt, there, a(2).i())
                })
            }
        }
        // `removeItem(stack)`: the slot holding that very stack empties.
        ("removeItem", _) => {
            for i in 0..=40 {
                if inventory_get(rt, i) == a(1) {
                    let e = empty(rt);
                    inventory_set(rt, i, e);
                    break;
                }
            }
            V::Void
        }
        ("removeItemNoUpdate", _) => {
            let there = inventory_get(rt, a(1).i());
            if is_empty(rt, there) {
                empty(rt)
            } else {
                let e = empty(rt);
                inventory_set(rt, a(1).i(), e);
                there
            }
        }
        ("getContainerSize", _) => V::I(41),
        ("getMaxStackSize", "()I") => V::I(99),
        ("getMaxStackSize", _) => V::I(inventory_max_for(rt, a(1))),
        ("isEmpty", _) => {
            for i in 0..=40 {
                let s = inventory_get(rt, i);
                if !is_empty(rt, s) {
                    return Some(V::I(0));
                }
            }
            V::I(1)
        }
        ("add", "(Lnet/minecraft/world/item/ItemStack;)Z") => V::I(inventory_add(rt, inv, -1, a(1)) as i32),
        ("add", _) => V::I(inventory_add(rt, inv, a(1).i(), a(2)) as i32),
        ("placeItemBackInInventory", _) => {
            crate::hand::with_hand(rt, |rt| place_back(rt, inv, a(1)));
            V::Void
        }
        ("getFreeSlot", _) => V::I(free_slot(rt)),
        ("getSlotWithRemainingSpace", _) => V::I(slot_with_room(rt, a(1))),
        ("findSlotMatchingItem", _) => {
            for i in 0..36 {
                let there = inventory_get(rt, i);
                if !is_empty(rt, there) && same_components(rt, a(1), there) {
                    return Some(V::I(i));
                }
            }
            V::I(-1)
        }
        ("contains", "(Lnet/minecraft/world/item/ItemStack;)Z") => {
            for i in 0..=40 {
                let there = inventory_get(rt, i);
                if !is_empty(rt, there) && same_components(rt, there, a(1)) {
                    return Some(V::I(1));
                }
            }
            V::I(0)
        }
        ("countItem", _) => {
            let want = crate::api_common::item_like_name(rt, a(1));
            let mut n = 0;
            for i in 0..=40 {
                let there = inventory_get(rt, i);
                if !is_empty(rt, there) && crate::api_common::item_like_name(rt, rt.get_field(there, "$item")) == want {
                    n += count(rt, there);
                }
            }
            V::I(n)
        }
        ("clearContent", _) => {
            for i in 0..=40 {
                let e = empty(rt);
                inventory_set(rt, i, e);
            }
            V::Void
        }
        ("setChanged" | "startOpen" | "stopOpen", _) => V::Void,
        ("stillValid", _) => V::I(1),
        ("getTimesChanged", _) => V::I(0),
        ("getName" | "getDisplayName", _) => {
            let key = rt.string("container.inventory");
            crate::api_common::component(rt, key, true, V::NULL)
        }
        ("getSelectionSize", _) => V::I(9),
        ("save", _) => inventory_save(rt, a(1)),
        ("load", _) => {
            inventory_load(rt, a(1));
            V::Void
        }
        // Each stack thrown around its player, its slot emptied.
        ("dropAll", _) => {
            let p = rt.get_field(inv, "player");
            for i in 0..=40 {
                let s = inventory_get(rt, i);
                if is_empty(rt, s) {
                    continue;
                }
                if fake_inventory().is_some() {
                    fake_drop(rt, p, s, true);
                } else {
                    player_drop(rt, p, s, true);
                }
                let e = empty(rt);
                inventory_set(rt, i, e);
            }
            V::Void
        }
        ("isHotbarSlot", _) => V::I((0..9).contains(&a(0).i()) as i32),
        _ => return None,
    })
}

// ---- SimpleContainer, and Container's defaults ----

fn simple_container_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let c = a(0);
    let items = rt.get_field(c, "$items");
    let size = list_len(rt, items);
    let valid = |i: i32| i >= 0 && (i as usize) < size;
    Some(match (name, desc) {
        ("<init>", "(I)V") => {
            let e = empty(rt);
            let l = rt.alloc(NON_NULL_LIST, Data::List(vec![e; a(1).i().max(0) as usize]));
            rt.put_field(c, "$items", l);
            V::Void
        }
        ("<init>", _) => {
            let stacks = crate::jutil::elements(rt, a(1));
            let l = rt.alloc(NON_NULL_LIST, Data::List(stacks));
            rt.put_field(c, "$items", l);
            V::Void
        }
        ("getContainerSize", _) => V::I(size as i32),
        ("getItems", _) => items,
        ("getItem", _) => {
            if valid(a(1).i()) {
                let s = list_get(rt, items, a(1).i() as usize);
                if s.r() == 0 { empty(rt) } else { s }
            } else {
                empty(rt)
            }
        }
        ("setItem", _) => {
            if valid(a(1).i()) {
                list_set(rt, items, a(1).i() as usize, a(2));
                // `stack.limitSize(getMaxStackSize(stack))`
                let most = virt(rt, C_MAX_FOR, c, &[a(2)]).i();
                if count(rt, a(2)) > most {
                    set_count(rt, a(2), most);
                }
                virt(rt, C_CHANGED, c, &[]);
            }
            V::Void
        }
        ("removeItem", "(II)Lnet/minecraft/world/item/ItemStack;") => {
            let there = if valid(a(1).i()) { list_get(rt, items, a(1).i() as usize) } else { V::NULL };
            if there.r() == 0 || is_empty(rt, there) || a(2).i() <= 0 {
                empty(rt)
            } else {
                let out = split(rt, there, a(2).i());
                virt(rt, C_CHANGED, c, &[]);
                out
            }
        }
        ("removeItemNoUpdate", _) => {
            let there = if valid(a(1).i()) { list_get(rt, items, a(1).i() as usize) } else { V::NULL };
            if there.r() == 0 || is_empty(rt, there) {
                empty(rt)
            } else {
                let e = empty(rt);
                list_set(rt, items, a(1).i() as usize, e);
                there
            }
        }
        ("isEmpty", _) => {
            for s in crate::jutil::elements(rt, items) {
                if !is_empty(rt, s) {
                    return Some(V::I(0));
                }
            }
            V::I(1)
        }
        ("clearContent", _) => {
            let e = empty(rt);
            for i in 0..size {
                list_set(rt, items, i, e);
            }
            virt(rt, C_CHANGED, c, &[]);
            V::Void
        }
        ("setChanged", _) => {
            for l in crate::jutil::elements(rt, rt.get_field(c, "$listeners")) {
                virt(rt, CL_CHANGED, l, &[c]);
            }
            V::Void
        }
        ("addListener", _) => {
            let mut l = rt.get_field(c, "$listeners");
            if l.r() == 0 {
                l = new_list(rt);
                rt.put_field(c, "$listeners", l);
            }
            list_push(rt, l, a(1));
            V::Void
        }
        ("removeListener", _) => {
            let l = rt.get_field(c, "$listeners");
            if let Some(Data::List(v)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
                v.retain(|x| *x != a(1));
            }
            V::Void
        }
        // `addItem(stack)`: onto stacks of the same first, then into empty slots; what's left.
        ("addItem", _) => {
            let s = copy(rt, a(1));
            for i in 0..size {
                if is_empty(rt, s) {
                    break;
                }
                let there = list_get(rt, items, i);
                if !is_empty(rt, there) && same_components(rt, there, s) {
                    let most = virt(rt, C_MAX_FOR, c, &[there]).i();
                    let n = count(rt, s).min(most - count(rt, there)).max(0);
                    grow(rt, there, n);
                    grow(rt, s, -n);
                }
            }
            for i in 0..size {
                if is_empty(rt, s) {
                    break;
                }
                let there = list_get(rt, items, i);
                if is_empty(rt, there) {
                    let moved = copy(rt, s);
                    set_count(rt, s, 0);
                    list_set(rt, items, i, moved);
                }
            }
            virt(rt, C_CHANGED, c, &[]);
            if is_empty(rt, s) { empty(rt) } else { s }
        }
        ("stillValid" | "canPlaceItem" | "canTakeItem", _) => V::I(1),
        ("startOpen" | "stopOpen", _) => V::Void,
        _ => return container_default(rt, name, desc, args),
    })
}

/// `Container`'s default methods, for a mod's own container.
fn container_default(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let c = a(0);
    Some(match (name, desc) {
        ("getMaxStackSize", "()I") => V::I(99),
        ("getMaxStackSize", _) => {
            let own = virt(rt, C_MAX, c, &[]).i();
            V::I(own.min(max_size(rt, a(1))))
        }
        ("startOpen" | "stopOpen", _) => V::Void,
        ("canPlaceItem" | "canTakeItem", _) => V::I(1),
        ("countItem", _) => {
            let want = crate::api_common::item_like_name(rt, a(1));
            let n = virt(rt, C_SIZE, c, &[]).i();
            let mut total = 0;
            for i in 0..n {
                let s = virt(rt, C_GET, c, &[V::I(i)]);
                if !is_empty(rt, s) && crate::api_common::item_like_name(rt, rt.get_field(s, "$item")) == want {
                    total += count(rt, s);
                }
            }
            V::I(total)
        }
        ("stillValidBlockEntity", _) => V::I(1),
        _ => return None,
    })
}

fn container_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let this = arg(args, 0);
    // An `Inventory` keeps `Container`'s defaults it doesn't override (`canPlaceItem`, which
    // NeoForge's `InvWrapper` asks before it fills a player's slot).
    if owner == INVENTORY || rt.class_of(this) == INVENTORY {
        return inventory_call(rt, name, desc, args).or_else(|| container_default(rt, name, desc, args));
    }
    if owner == SIMPLE_CONTAINER || rt.instance_of(this, SIMPLE_CONTAINER) {
        return simple_container_call(rt, name, desc, args);
    }
    container_default(rt, name, desc, args)
}

// ---- data slots ----

/// A `DataSlot` of `kind` 0 standalone, 1 a `ContainerData`'s value `index`, 2 an `int[]`'s.
fn data_slot(rt: &mut Rt, kind: i32, of: V, index: i32) -> V {
    let d = rt.new_object(DATA_SLOT);
    rt.put_field(d, "$kind", V::I(kind));
    rt.put_field(d, "$of", of);
    rt.put_field(d, "$index", V::I(index));
    d
}

fn data_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let d = a(0);
    Some(match (owner, name) {
        (DATA_SLOT, "<init>") => V::Void,
        (DATA_SLOT, "standalone") => data_slot(rt, 0, V::NULL, 0),
        (DATA_SLOT, "forContainer") => data_slot(rt, 1, a(0), a(1).i()),
        (DATA_SLOT, "shared") => data_slot(rt, 2, a(0), a(1).i()),
        (DATA_SLOT, "get") => {
            let (of, i) = (rt.get_field(d, "$of"), rt.get_field(d, "$index").i());
            match rt.get_field(d, "$kind").i() {
                1 => virt(rt, CD_GET, of, &[V::I(i)]),
                2 => V::I(rt.array_get(of, i).i()),
                _ => V::I(rt.get_field(d, "$value").i()),
            }
        }
        (DATA_SLOT, "set") => {
            let (of, i) = (rt.get_field(d, "$of"), rt.get_field(d, "$index").i());
            match rt.get_field(d, "$kind").i() {
                1 => {
                    virt(rt, CD_SET, of, &[V::I(i), a(1)]);
                }
                2 => rt.array_set(of, i, V::I(a(1).i())),
                _ => rt.put_field(d, "$value", V::I(a(1).i())),
            }
            V::Void
        }
        (DATA_SLOT, "checkAndClearUpdateFlag") => {
            let v = virt(rt, D_GET, d, &[]).i();
            let changed = v != rt.get_field(d, "$prev").i();
            rt.put_field(d, "$prev", V::I(v));
            V::I(changed as i32)
        }
        (SIMPLE_CONTAINER_DATA | CONTAINER_DATA, "<init>") => {
            let ints = rt.alloc("[I", Data::Array(vec![V::I(0); a(1).i().max(0) as usize]));
            rt.put_field(d, "$ints", ints);
            V::Void
        }
        (_, "get") if rt.class_of(d) == SIMPLE_CONTAINER_DATA => V::I(rt.array_get(rt.get_field(d, "$ints"), a(1).i()).i()),
        (_, "set") if rt.class_of(d) == SIMPLE_CONTAINER_DATA => {
            let ints = rt.get_field(d, "$ints");
            rt.array_set(ints, a(1).i(), V::I(a(2).i()));
            V::Void
        }
        (_, "getCount") if rt.class_of(d) == SIMPLE_CONTAINER_DATA => V::I(rt.array_len(rt.get_field(d, "$ints"))),
        _ => return None,
    })
}

// ---- OptionalInt, OptionalLong, OptionalDouble ----

pub(crate) fn optional_int(rt: &mut Rt, v: Option<i32>) -> V {
    optional_number(rt, OPTIONAL_INT, v.map(V::I))
}

/// An `OptionalInt`, `OptionalLong` or `OptionalDouble` (`class`) of `v`, of the class's kind.
fn optional_number(rt: &mut Rt, class: &'static str, v: Option<V>) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "$present", V::I(v.is_some() as i32));
    let zero = match class {
        OPTIONAL_LONG => V::J(0),
        OPTIONAL_DOUBLE => V::D(0.0),
        _ => V::I(0),
    };
    rt.put_field(o, "$value", v.unwrap_or(zero));
    o
}

fn optional_number_call(rt: &mut Rt, class: &'static str, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let o = a(0);
    let present = truthy(rt.get_field(o, "$present"));
    // The consumer `ifPresent` hands the value to, and the supplier `orElseGet` asks.
    let (consumer, supplier) = match class {
        OPTIONAL_LONG => ("java/util/function/LongConsumer.accept.(J)V", "java/util/function/LongSupplier.getAsLong.()J"),
        OPTIONAL_DOUBLE => ("java/util/function/DoubleConsumer.accept.(D)V", "java/util/function/DoubleSupplier.getAsDouble.()D"),
        _ => (INT_CONSUMER, "java/util/function/IntSupplier.getAsInt.()I"),
    };
    Some(match name {
        "of" => optional_number(rt, class, Some(a(0))),
        "empty" => optional_number(rt, class, None),
        "isPresent" => V::I(present as i32),
        "isEmpty" => V::I(!present as i32),
        "getAsInt" | "getAsLong" | "getAsDouble" | "orElseThrow" => {
            if !present {
                rt.throw_new("java/util/NoSuchElementException");
            }
            rt.get_field(o, "$value")
        }
        "orElse" => {
            if present {
                rt.get_field(o, "$value")
            } else {
                a(1)
            }
        }
        "orElseGet" => {
            if present {
                rt.get_field(o, "$value")
            } else {
                rt.call_functional(supplier, a(1), &[])
            }
        }
        "ifPresent" => {
            if present {
                let v = rt.get_field(o, "$value");
                rt.call_functional(consumer, a(1), &[v]);
            }
            V::Void
        }
        "ifPresentOrElse" => {
            if present {
                let v = rt.get_field(o, "$value");
                rt.call_functional(consumer, a(1), &[v]);
            } else {
                rt.call_functional("java/lang/Runnable.run.()V", a(2), &[]);
            }
            V::Void
        }
        _ => return None,
    })
}

// ---- menu types, providers, screens ----

/// `MenuType.create(id, inventory[, extra data])`: the mod's factory makes the menu.
fn create_menu(rt: &mut Rt, ty: V, id: i32, inv: V, buf: V) -> V {
    let factory = rt.get_field(ty, "$factory");
    if factory.r() != 0 {
        return rt.call_functional(F_CREATE, factory, &[V::I(id), inv, buf]);
    }
    let supplier = rt.get_field(ty, "$supplier");
    if supplier.r() == 0 {
        return V::NULL;
    }
    rt.call_functional(SUPPLIER_CREATE, supplier, &[V::I(id), inv])
}

fn type_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match (owner, name) {
        (MENU_TYPE_EXTENSION, "create") if desc.starts_with("(Lnet/neoforged/neoforge/network/IContainerFactory;") => {
            let t = rt.new_object(MENU_TYPE);
            rt.put_field(t, "$factory", a(0));
            t
        }
        (MENU_TYPE, "<init>") => {
            rt.put_field(a(0), "$supplier", a(1));
            V::Void
        }
        (_, "create") if desc.starts_with("(ILnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/RegistryFriendlyByteBuf;)") => create_menu(rt, a(0), a(1).i(), a(2), a(3)),
        (_, "create") if desc.starts_with("(ILnet/minecraft/world/entity/player/Inventory;)") => create_menu(rt, a(0), a(1).i(), a(2), V::NULL),
        (_, "requiredFeatures") => rt.intern("compat:feature-flags".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagSet")),
        (_, "isEnabled") => V::I(1),
        _ => return None,
    })
}

fn provider_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let p = a(0);
    let simple = rt.class_of(p) == SIMPLE_MENU_PROVIDER;
    Some(match (owner, name) {
        (SIMPLE_MENU_PROVIDER, "<init>") => {
            rt.put_field(p, "$constructor", a(1));
            rt.put_field(p, "$title", a(2));
            V::Void
        }
        (_, "createMenu") if simple => {
            let c = rt.get_field(p, "$constructor");
            rt.call_functional(P_CREATE, c, &[a(1), a(2), a(3)])
        }
        (_, "getDisplayName") if simple => rt.get_field(p, "$title"),
        (_, "writeClientSideData") => V::Void,
        (_, "shouldTriggerClientSideContainerClosingOnOpen") => V::I(1),
        _ => return None,
    })
}

fn level_access_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "create" => {
            let o = rt.new_object(LEVEL_ACCESS);
            rt.put_field(o, "$level", a(0));
            rt.put_field(o, "$pos", a(1));
            o
        }
        "evaluate" => {
            let (level, pos) = (rt.get_field(a(0), "$level"), rt.get_field(a(0), "$pos"));
            let with_default = desc.contains(";Ljava/lang/Object;)");
            if level.r() == 0 {
                return Some(if with_default { a(2) } else { crate::api_common::optional(rt, V::NULL) });
            }
            let v = rt.call_functional(BI_FUNCTION, a(1), &[level, pos]);
            if with_default { v } else { crate::api_common::optional(rt, v) }
        }
        "execute" => {
            let (level, pos) = (rt.get_field(a(0), "$level"), rt.get_field(a(0), "$pos"));
            if level.r() != 0 {
                rt.call_functional(BI_CONSUMER, a(1), &[level, pos]);
            }
            V::Void
        }
        _ => return None,
    })
}

/// Static fields this module owns.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    Some(match (owner, field) {
        (LEVEL_ACCESS, "NULL") => rt.intern("compat:level-access-null".into(), |rt| rt.new_object(LEVEL_ACCESS)),
        _ => return None,
    })
}

// ---- opening and closing ----

/// `ServerPlayer.openMenu(provider, extra data)`: the menu the provider makes opens for the player,
/// on their client too; its id, or empty when none opened. `write` puts the extra data the
/// client's factory reads (NeoForge's).
fn open_menu(rt: &mut Rt, player: V, provider: V, write: impl FnOnce(&mut Rt, V)) -> V {
    if provider.r() == 0 || is_client_player(rt, player) {
        return optional_int(rt, None);
    }
    init_player(rt, player);
    if menu_open(rt, player) {
        server_close(rt, player);
    }
    let counter = rt.get_field(player, "$containerCounter").i() % 100 + 1;
    rt.put_field(player, "$containerCounter", V::I(counter));
    let inv = inventory(rt, player);
    let menu = virt(rt, P_CREATE, provider, &[V::I(counter), inv, player]);
    if rt.thrown.is_some() || menu.r() == 0 {
        rt.report_thrown("creating a menu");
        return optional_int(rt, None);
    }
    let Some(uuid) = uuid_of(rt, player) else { return optional_int(rt, None) };
    let ty = rt.get_field(menu, "$type");
    let Some(type_name) = rt.registered_name(ty, "menu").map(str::to_owned) else {
        crate::glue::trace_once(rt, "menu-type", |rt| format!("server: menu {} has no type of a mod's (vanilla's own menus don't open yet)", rt.class_of(menu)));
        return optional_int(rt, None);
    };
    let buf = network::new_buffer(rt);
    virt(rt, P_WRITE, provider, &[menu, buf]);
    write(rt, buf);
    let extra = network::bytes(rt, buf);
    let title = virt(rt, P_NAME, provider, &[]);
    let json = crate::api_common::component_json(rt, title).to_string();
    let out = network::new_buffer(rt);
    network::put(rt, out, &[OPEN]);
    network::put_utf(rt, out, &type_name);
    network::put_var(rt, out, u64::from(counter as u32));
    network::put_utf(rt, out, &json);
    network::put_var(rt, out, extra.len() as u64);
    network::put(rt, out, &extra);
    let bytes = network::bytes(rt, out);
    network::send_raw(rt, PayloadTarget::Player(uuid), CHANNEL, &bytes);
    // `initMenu`: listeners hear of the contents, then the client gets them all.
    virt(rt, K_BROADCAST, menu, &[]);
    rt.put_field(menu, "$sync", player);
    send_all_data(rt, menu);
    rt.put_field(player, "containerMenu", menu);
    crate::glue::trace_some(rt, "menu-open", &type_name, 20, |_| format!("server: menu {type_name} #{counter} opened"));
    optional_int(rt, Some(counter))
}

/// `ServerPlayer.closeContainer()`: the client is told, then the menu closes here.
fn server_close(rt: &mut Rt, player: V) {
    let m = rt.get_field(player, "containerMenu");
    if let Some(uuid) = uuid_of(rt, player) {
        let out = network::new_buffer(rt);
        network::put(rt, out, &[CLOSE]);
        network::put_var(rt, out, u64::from(rt.get_field(m, "containerId").i() as u32));
        let bytes = network::bytes(rt, out);
        network::send_raw(rt, PayloadTarget::Player(uuid), CHANNEL, &bytes);
    }
    do_close(rt, player);
}

/// `ServerPlayer.doCloseContainer()`: the menu is `removed` (a mod saves what it edited) and the
/// inventory's is open again.
fn do_close(rt: &mut Rt, player: V) {
    let m = rt.get_field(player, "containerMenu");
    if menu_open(rt, player) {
        crate::hand::with_hand(rt, |rt| virt(rt, K_REMOVED, m, &[player]));
        rt.report_thrown("a menu's removed()");
        rt.put_field(m, "$sync", V::NULL);
    }
    let inv = rt.get_field(player, "inventoryMenu");
    rt.put_field(player, "containerMenu", inv);
}

/// `LocalPlayer.clientSideCloseContainer()`: the inventory's menu is open again and the screen
/// closes (its `removed` tells the menu).
fn client_side_close(rt: &mut Rt, player: V) {
    init_player(rt, player);
    let inv = rt.get_field(player, "inventoryMenu");
    rt.put_field(player, "containerMenu", inv);
    crate::screens::set_screen(rt, V::NULL);
}

/// Sends the server a message about the client's menu.
fn send_server(rt: &mut Rt, out: V) {
    let bytes = network::bytes(rt, out);
    network::send_raw(rt, PayloadTarget::Server, CHANNEL, &bytes);
}

/// `LocalPlayer.closeContainer()`: the server is told, then the menu closes here.
fn client_close(rt: &mut Rt, player: V) {
    let m = rt.get_field(player, "containerMenu");
    if menu_open(rt, player) {
        let out = network::new_buffer(rt);
        network::put(rt, out, &[CLOSED]);
        network::put_var(rt, out, u64::from(rt.get_field(m, "containerId").i() as u32));
        send_server(rt, out);
    }
    client_side_close(rt, player);
}

/// The game closed the mods' screen itself while a menu was open on the client: the server hears
/// the menu closed, as it would from the screen.
pub fn screen_gone(rt: &mut Rt) {
    let player = network::local_player(rt);
    if !menu_open(rt, player) {
        return;
    }
    let m = rt.get_field(player, "containerMenu");
    let out = network::new_buffer(rt);
    network::put(rt, out, &[CLOSED]);
    network::put_var(rt, out, u64::from(rt.get_field(m, "containerId").i() as u32));
    send_server(rt, out);
    let inv = rt.get_field(player, "inventoryMenu");
    rt.put_field(player, "containerMenu", inv);
}

fn player_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let p = a(0);
    Some(match (name, desc) {
        ("getInventory", _) => {
            init_player(rt, p);
            inventory(rt, p)
        }
        // `Player.drop(stack, includeThrowerName)` (NeoForge's `CommonHooks.onPlayerTossEvent`) and
        // `drop(stack, dropAround, includeThrowerName)`.
        ("drop", "(Lnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/entity/item/ItemEntity;") => player_drop(rt, p, a(1), false),
        ("drop", "(Lnet/minecraft/world/item/ItemStack;ZZ)Lnet/minecraft/world/entity/item/ItemEntity;") => player_drop(rt, p, a(1), a(2).i() != 0),
        ("openMenu", "(Lnet/minecraft/world/MenuProvider;)Ljava/util/OptionalInt;") => open_menu(rt, p, a(1), |_, _| {}),
        ("openMenu", "(Lnet/minecraft/world/MenuProvider;Lnet/minecraft/core/BlockPos;)Ljava/util/OptionalInt;") => {
            let pos = a(2);
            open_menu(rt, p, a(1), move |rt, buf| {
                crate::api::call(rt, "net/minecraft/network/FriendlyByteBuf.writeBlockPos.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/network/FriendlyByteBuf;", &[buf, pos]);
            })
        }
        ("openMenu", _) => {
            let writer = a(2);
            open_menu(rt, p, a(1), move |rt, buf| {
                if writer.r() != 0 {
                    rt.call_functional(CONSUMER, writer, &[buf]);
                }
            })
        }
        ("closeContainer", _) => {
            if is_client_player(rt, p) {
                client_close(rt, p);
            } else if menu_open(rt, p) {
                server_close(rt, p);
            }
            V::Void
        }
        ("doCloseContainer", _) => {
            do_close(rt, p);
            V::Void
        }
        ("clientSideCloseContainer", _) => {
            client_side_close(rt, p);
            V::Void
        }
        ("hasContainerOpen", _) => V::I(menu_open(rt, p) as i32),
        ("initMenu", _) => {
            rt.put_field(a(1), "$sync", p);
            send_all_data(rt, a(1));
            V::Void
        }
        ("nextContainerCounter", _) => {
            let counter = rt.get_field(p, "$containerCounter").i() % 100 + 1;
            rt.put_field(p, "$containerCounter", V::I(counter));
            V::Void
        }
        _ => return None,
    })
}

// ---- the client's side ----

/// `MultiPlayerGameMode.handleInventoryMouseClick`: the click runs on the client's menu as a
/// prediction, and goes to the server with the slots it changed and the carried stack.
pub(crate) fn client_click(rt: &mut Rt, id: i32, slot_id: i32, button: i32, click: V, player: V) {
    let m = rt.get_field(player, "containerMenu");
    if m.r() == 0 || rt.get_field(m, "containerId").i() != id {
        return;
    }
    let all = slots(rt, m);
    let mut before = Vec::with_capacity(all.len());
    for &slot in &all {
        let s = slot_item(rt, slot);
        before.push(copy(rt, s));
    }
    virt(rt, K_CLICKED, m, &[V::I(slot_id), V::I(button), click, player]);
    rt.report_thrown("a click in a menu");
    let mut changed = Vec::new();
    for (j, &slot) in all.iter().enumerate() {
        let now = slot_item(rt, slot);
        if !matches(rt, before[j], now) {
            let c = copy(rt, now);
            changed.push((j as i32, c));
        }
    }
    let out = network::new_buffer(rt);
    network::put(rt, out, &[CLICK]);
    network::put_var(rt, out, u64::from(id as u32));
    network::put_var(rt, out, u64::from(rt.get_field(m, "$stateId").i() as u32));
    network::put(rt, out, &slot_id.to_be_bytes());
    network::put_var(rt, out, u64::from(button as u32));
    network::put(rt, out, &[rt.get_field(click, "$ordinal").i() as u8]);
    let held = carried(rt, m);
    let held = copy(rt, held);
    network::put_stack(rt, out, held);
    network::put_var(rt, out, changed.len() as u64);
    for (j, s) in changed {
        network::put_var(rt, out, u64::from(j as u32));
        network::put_stack(rt, out, s);
    }
    send_server(rt, out);
}

fn game_mode_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "handleInventoryMouseClick" => {
            client_click(rt, a(1).i(), a(2).i(), a(3).i(), a(4), a(5));
            V::Void
        }
        "handleInventoryButtonClick" => {
            let out = network::new_buffer(rt);
            network::put(rt, out, &[BUTTON]);
            network::put_var(rt, out, u64::from(a(1).i() as u32));
            network::put_var(rt, out, u64::from(a(2).i() as u32));
            send_server(rt, out);
            V::Void
        }
        _ => return None,
    })
}

/// A message about menus from the server, on its player's client.
pub fn receive_client(rt: &mut Rt, data: &[u8]) {
    let buf = network::buffer_with(rt, data.to_vec());
    let Some([kind]) = network::take_n::<1>(rt, buf) else { return };
    let player = network::local_player(rt);
    init_player(rt, player);
    match kind {
        OPEN => open_client(rt, buf, player),
        UPDATE => update_client(rt, buf, player),
        CLOSE => client_side_close(rt, player),
        _ => {}
    }
    rt.report_thrown("a menu message from the server");
}

/// The server opened a menu: the client makes it with the type's factory and opens the screen the
/// mod registered for the type (NeoForge's `ClientPayloadHandler` for `AdvancedOpenScreenPayload`).
fn open_client(rt: &mut Rt, buf: V, player: V) {
    let (Some(type_name), Some(id), Some(json), Some(n)) =
        (network::read_utf(rt, buf), network::read_var_int(rt, buf), network::read_utf(rt, buf), network::read_var_int(rt, buf))
    else {
        return;
    };
    let Some(extra) = network::take(rt, buf, n.max(0) as usize) else { return };
    let Some(&ty) = rt.registered.get(&("menu".to_owned(), type_name.clone())) else {
        crate::glue::trace_once(rt, "menu-unknown", |_| format!("client: no menu type {type_name}"));
        return;
    };
    let ctor = rt.get_field(ty, "$screen");
    if ctor.r() == 0 {
        crate::glue::trace_once(rt, "menu-screenless", |_| format!("client: no screen registered for menu type {type_name}"));
        return;
    }
    let inv = inventory(rt, player);
    let extra = network::buffer_with(rt, extra);
    let menu = create_menu(rt, ty, id, inv, extra);
    if rt.thrown.is_some() || menu.r() == 0 {
        rt.report_thrown("making a menu on the client");
        return;
    }
    let title = match serde_json::from_str::<serde_json::Value>(&json) {
        Ok(j) => crate::api_common::component_from_json(rt, &j),
        Err(_) => {
            let s = rt.string(&json);
            crate::api_common::component(rt, s, false, V::NULL)
        }
    };
    let screen = rt.call_functional(SCREEN_CREATE, ctor, &[menu, inv, title]);
    if rt.thrown.is_some() || screen.r() == 0 {
        rt.report_thrown("making a menu's screen");
        return;
    }
    rt.put_field(player, "containerMenu", menu);
    crate::screens::set_screen(rt, screen);
    crate::glue::trace_some(rt, "menu-open", &type_name, 20, |rt| format!("client: menu {type_name} #{id} opened with screen {}", rt.class_of(screen)));
}

/// Slots, the carried stack and data values the server sent, set on the open menu if it is still
/// the one they are for.
fn update_client(rt: &mut Rt, buf: V, player: V) {
    let (Some(id), Some(state), Some([flags]), Some(n)) =
        (network::read_var_int(rt, buf), network::read_var_int(rt, buf), network::take_n::<1>(rt, buf), network::read_var_int(rt, buf))
    else {
        return;
    };
    let m = rt.get_field(player, "containerMenu");
    let current = m.r() != 0 && rt.get_field(m, "containerId").i() == id;
    crate::hand::with_hand(rt, |rt| {
        for _ in 0..n.max(0) {
            let (Some(i), Some(s)) = (network::read_var_int(rt, buf), network::read_stack(rt, buf)) else { return };
            if current {
                virt(rt, K_SET_ITEM, m, &[V::I(i), V::I(state), s]);
            }
        }
        if flags & 1 != 0 {
            let Some(s) = network::read_stack(rt, buf) else { return };
            if current {
                set_carried(rt, m, s);
            }
        }
        let Some(data) = network::read_var_int(rt, buf) else { return };
        for _ in 0..data.max(0) {
            let (Some(j), Some(v)) = (network::take_n::<2>(rt, buf), network::take_n::<2>(rt, buf)) else { return };
            if current {
                virt(rt, K_SET_DATA, m, &[V::I(i32::from(i16::from_be_bytes(j))), V::I(i32::from(i16::from_be_bytes(v)))]);
            }
        }
    });
    if current {
        rt.put_field(m, "$stateId", V::I(state));
    }
}

// ---- the server's side ----

fn inbox_key(uuid: u128) -> String {
    format!("compat:inbox/{uuid:x}")
}

/// Whether the game calls [`player_tick`]: what a client sends then waits for its player's tick.
pub fn player_ticks_seen(rt: &Rt) -> bool {
    rt.interned.contains_key(PLAYER_TICKS)
}

/// Marks that the game calls [`player_tick`].
pub fn seen_player_tick(rt: &mut Rt) {
    rt.interned.insert(PLAYER_TICKS.into(), V::I(1));
}

/// Keeps what `player`'s client sent for their next tick, in order.
pub fn queue(rt: &mut Rt, player: u128, channel: &str, data: &[u8]) {
    let list = rt.intern(inbox_key(player), |rt| crate::jutil::new_list(rt, Vec::new()));
    let e = rt.new_object("compat:received");
    let c = rt.string(channel);
    rt.put_field(e, "$channel", c);
    let b = rt.alloc("compat:bytes", Data::Bytes(data.to_vec(), 0));
    rt.put_field(e, "$data", b);
    list_push(rt, list, e);
}

/// Whether `player`'s tick has anything to do here: a message waiting, or a menu open.
pub fn has_work(rt: &Rt, player: u128) -> bool {
    rt.interned.contains_key(&inbox_key(player)) || rt.interned.get(&format!("compat:player/{player:x}")).is_some_and(|&p| menu_open(rt, p))
}

/// What `player`'s client sent, run with the host acting for them: a menu's own messages here, a
/// mod's payloads by their handlers.
pub fn handle_from_client(rt: &mut Rt, player: u128, channel: &str, data: &[u8]) -> bool {
    if channel == CHANNEL {
        receive_server(rt, player, data);
        return true;
    }
    network::receive(rt, channel, data, Some(player))
}

/// The player's tick on the server (`ServerPlayer.tick`), with the host acting for them: what their
/// client sent runs, then the open menu sends what changed and closes when it no longer may stay
/// open. While a menu is open every slot of the inventory is reached, so what the menu does to a
/// stack in it (a filter saving into the one in hand) reaches the game at the end of the tick.
pub fn player_tick(rt: &mut Rt, uuid: u128) {
    let p = server_player(rt, uuid);
    if menu_open(rt, p) {
        for s in 0..=40u8 {
            crate::hand::slot_stack(rt, s);
        }
    }
    if let Some(list) = rt.interned.remove(&inbox_key(uuid)) {
        for e in crate::jutil::elements(rt, list) {
            let channel = rt.str_of(rt.get_field(e, "$channel")).unwrap_or("").to_owned();
            let data = match rt.heap.get(rt.get_field(e, "$data").r() as usize).map(|o| &o.data) {
                Some(Data::Bytes(b, _)) => b.clone(),
                _ => Vec::new(),
            };
            handle_from_client(rt, uuid, &channel, &data);
            rt.report_thrown("a message from a client");
        }
    }
    if menu_open(rt, p) {
        inventory(rt, p);
        let m = rt.get_field(p, "containerMenu");
        virt(rt, K_BROADCAST, m, &[]);
        rt.report_thrown("a menu's broadcastChanges()");
        let valid = truthy(virt(rt, K_STILL_VALID, m, &[p]));
        rt.report_thrown("a menu's stillValid()");
        if !valid && rt.get_field(p, "containerMenu") == m {
            server_close(rt, p);
        }
    }
}

/// A message about menus from `uuid`'s client (`handleContainerClick`, `...ButtonClick`,
/// `...Close`).
fn receive_server(rt: &mut Rt, uuid: u128, data: &[u8]) {
    let buf = network::buffer_with(rt, data.to_vec());
    let Some([kind]) = network::take_n::<1>(rt, buf) else { return };
    let player = server_player(rt, uuid);
    let m = rt.get_field(player, "containerMenu");
    let id = network::read_var_int(rt, buf).unwrap_or(-1);
    let current = m.r() != 0 && rt.get_field(m, "containerId").i() == id;
    match kind {
        CLICK => {
            let (Some(state), Some(slot), Some(button), Some([click])) =
                (network::read_var_int(rt, buf), network::take_n::<4>(rt, buf).map(i32::from_be_bytes), network::read_var_int(rt, buf), network::take_n::<1>(rt, buf))
            else {
                return;
            };
            let Some(held) = network::read_stack(rt, buf) else { return };
            let n = network::read_var_int(rt, buf).unwrap_or(0);
            let mut changed = Vec::new();
            for _ in 0..n.max(0) {
                let (Some(i), Some(s)) = (network::read_var_int(rt, buf), network::read_stack(rt, buf)) else { return };
                changed.push((i, s));
            }
            if !current || !truthy(virt(rt, K_STILL_VALID, m, &[player])) {
                return;
            }
            let size = list_len(rt, rt.get_field(m, "slots")) as i32;
            if !(slot == -1 || slot == OUTSIDE || (0..size).contains(&slot)) {
                return;
            }
            let stale = state != rt.get_field(m, "$stateId").i();
            rt.put_field(m, "$suppress", V::I(1));
            let ct = crate::api::api_enum(rt, CLICK_TYPE, CLICK_TYPES[usize::from(click.min(6))], i32::from(click.min(6)));
            virt(rt, K_CLICKED, m, &[V::I(slot), V::I(button), ct, player]);
            rt.report_thrown("a click in a menu");
            let remote = rt.get_field(m, "$remoteSlots");
            for (i, s) in changed {
                if i >= 0 {
                    list_set(rt, remote, i as usize, s);
                }
            }
            rt.put_field(m, "$remoteCarried", held);
            rt.put_field(m, "$suppress", V::I(0));
            if stale {
                menu_call(rt, "broadcastFullState", "()V", &[m]);
            } else {
                virt(rt, K_BROADCAST, m, &[]);
            }
        }
        BUTTON => {
            let Some(button) = network::read_var_int(rt, buf) else { return };
            if current && truthy(virt(rt, K_STILL_VALID, m, &[player])) && truthy(virt(rt, K_MENU_BUTTON, m, &[player, V::I(button)])) {
                virt(rt, K_BROADCAST, m, &[]);
            }
        }
        CLOSED => do_close(rt, player),
        _ => {}
    }
}

// ---- dispatch ----

/// Whether `owner` is a class whose `getInventory`, `openMenu`... this module answers.
fn player_owner(owner: &str) -> bool {
    owner == PLAYER_EXTENSION || crate::api_common::is_player(owner)
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        MENU | INVENTORY_MENU
            | SLOT
            | HANDLER_SLOT
            | MENU_TYPE
            | MENU_TYPE_EXTENSION
            | INVENTORY
            | CONTAINER
            | SIMPLE_CONTAINER
            | MENU_PROVIDER
            | MENU_CONSTRUCTOR
            | MENU_PROVIDER_EXTENSION
            | SIMPLE_MENU_PROVIDER
            | DATA_SLOT
            | CONTAINER_DATA
            | SIMPLE_CONTAINER_DATA
            | LEVEL_ACCESS
            | OPTIONAL_INT
            | OPTIONAL_LONG
            | OPTIONAL_DOUBLE
            | REGISTER_SCREENS
            | MENU_SCREENS
    ) || (owner == GAME_MODE && matches!(name, "handleInventoryMouseClick" | "handleInventoryButtonClick"))
        || (player_owner(owner)
            && matches!(name, "getInventory" | "openMenu" | "closeContainer" | "doCloseContainer" | "clientSideCloseContainer" | "hasContainerOpen" | "initMenu" | "nextContainerCounter" | "drop"))
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        MENU | INVENTORY_MENU => menu_call(rt, name, desc, args),
        SLOT | HANDLER_SLOT => slot_call(rt, owner, name, desc, args),
        INVENTORY | CONTAINER | SIMPLE_CONTAINER => container_call(rt, owner, name, desc, args),
        MENU_TYPE | MENU_TYPE_EXTENSION => type_call(rt, owner, name, desc, args),
        MENU_PROVIDER | MENU_CONSTRUCTOR | MENU_PROVIDER_EXTENSION | SIMPLE_MENU_PROVIDER => provider_call(rt, owner, name, args),
        DATA_SLOT | CONTAINER_DATA | SIMPLE_CONTAINER_DATA => data_call(rt, owner, name, args),
        LEVEL_ACCESS => level_access_call(rt, name, desc, args),
        OPTIONAL_INT | OPTIONAL_LONG | OPTIONAL_DOUBLE => optional_number_call(rt, owner, name, args),
        REGISTER_SCREENS if name == "register" => {
            rt.put_field(arg(args, 1), "$screen", arg(args, 2));
            Some(V::Void)
        }
        MENU_SCREENS if name == "register" => {
            rt.put_field(arg(args, 0), "$screen", arg(args, 1));
            Some(V::Void)
        }
        GAME_MODE => game_mode_call(rt, name, args),
        o if player_owner(o) => player_call(rt, name, desc, args),
        _ => None,
    }
}

/// The client's screens learn of the mods' menu screens (`RegisterMenuScreensEvent`), once, after
/// the client's setup.
pub fn register_screens(rt: &mut Rt) {
    let ev = rt.new_object(REGISTER_SCREENS);
    crate::neo::post(rt, REGISTER_SCREENS, ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{GuiDraw, HandStack, Host, MemoryHost};
    use crate::screens::Input;

    const FILTER_MENU: &str = "test/FilterMenu";
    const FILTER_SCREEN: &str = "test/FilterScreen";
    const FACTORY: &str = "test/FilterFactory";
    const PROVIDER: &str = "test/FilterProvider";
    const WRITER: &str = "test/HeldWriter";
    const SCREEN_CTOR: &str = "test/FilterScreenConstructor";
    const CHEST_MENU: &str = "test/ChestMenu";
    const PLAYER: u128 = 0x1234_5678_9abc_def0_1122_3344_5566_7788;

    const MENU_INIT: &str = "net/minecraft/world/inventory/AbstractContainerMenu.<init>.(Lnet/minecraft/world/inventory/MenuType;I)V";
    const ADD_SLOT: &str = "net/minecraft/world/inventory/AbstractContainerMenu.addSlot.(Lnet/minecraft/world/inventory/Slot;)Lnet/minecraft/world/inventory/Slot;";
    const MOVE_TO: &str = "net/minecraft/world/inventory/AbstractContainerMenu.moveItemStackTo.(Lnet/minecraft/world/item/ItemStack;IIZ)Z";
    const SLOT_INIT: &str = "net/minecraft/world/inventory/Slot.<init>.(Lnet/minecraft/world/Container;III)V";
    const HANDLER_SLOT_INIT: &str = "net/neoforged/neoforge/items/SlotItemHandler.<init>.(Lnet/neoforged/neoforge/items/IItemHandler;III)V";
    const STACK_HANDLER: &str = "net/neoforged/neoforge/items/ItemStackHandler";
    const STACK_HANDLER_INIT: &str = "net/neoforged/neoforge/items/ItemStackHandler.<init>.(I)V";
    const CONTAINER_INIT: &str = "net/minecraft/world/SimpleContainer.<init>.(I)V";
    const GET_SELECTED: &str = "net/minecraft/world/entity/player/Inventory.getSelected.()Lnet/minecraft/world/item/ItemStack;";
    const MAIN_HAND: &str = "net/minecraft/world/entity/player/Player.getMainHandItem.()Lnet/minecraft/world/item/ItemStack;";
    const GET_INVENTORY: &str = "net/minecraft/world/entity/player/Player.getInventory.()Lnet/minecraft/world/entity/player/Inventory;";
    const OPEN_MENU: &str = "net/minecraft/world/entity/player/Player.openMenu.(Lnet/minecraft/world/MenuProvider;Ljava/util/function/Consumer;)Ljava/util/OptionalInt;";
    const WRITE_UTF: &str = "net/minecraft/network/FriendlyByteBuf.writeUtf.(Ljava/lang/String;)Lnet/minecraft/network/FriendlyByteBuf;";
    const READ_UTF: &str = "net/minecraft/network/FriendlyByteBuf.readUtf.()Ljava/lang/String;";
    const SET_DAMAGE: &str = "net/minecraft/world/item/ItemStack.setDamageValue.(I)V";
    const TYPE_CREATE: &str = "net/neoforged/neoforge/common/extensions/IMenuTypeExtension.create.(Lnet/neoforged/neoforge/network/IContainerFactory;)Lnet/minecraft/world/inventory/MenuType;";
    const REGISTER_SCREEN: &str = "net/neoforged/neoforge/client/event/RegisterMenuScreensEvent.register.(Lnet/minecraft/world/inventory/MenuType;Lnet/minecraft/client/gui/screens/MenuScreens$ScreenConstructor;)V";
    const SCREEN_INIT: &str = "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen.<init>.(Lnet/minecraft/world/inventory/AbstractContainerMenu;Lnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/chat/Component;)V";
    const GUI_FILL: &str = "net/minecraft/client/gui/GuiGraphics.fill.(IIIII)V";
    const OPTIONAL_INT_GET: &str = "java/util/OptionalInt.getAsInt.()I";
    const BACKGROUND: u32 = 0xFFC6_C6C6;

    fn slot(rt: &mut Rt, m: V, c: V, index: i32, x: i32, y: i32) {
        let s = rt.new_object(SLOT);
        crate::api::call(rt, SLOT_INIT, &[s, c, V::I(index), V::I(x), V::I(y)]);
        crate::api::call(rt, ADD_SLOT, &[m, s]);
    }

    /// Create's filter menu, reduced: the player's inventory (the main rows as slots 0 to 26, the
    /// hotbar 27 to 35, as vanilla lays them out) and two ghost slots (36, 37) over an item
    /// handler; `$holder` the stack it edits.
    fn filter_menu(rt: &mut Rt, ty: V, id: i32, inv: V, holder: V) -> V {
        let m = rt.new_object(FILTER_MENU);
        crate::api::call(rt, MENU_INIT, &[m, ty, V::I(id)]);
        rt.put_field(m, "$holder", holder);
        rt.put_field(m, "$inv", inv);
        for row in 0..3 {
            for col in 0..9 {
                slot(rt, m, inv, col + row * 9 + 9, 8 + col * 18, 84 + row * 18);
            }
        }
        for col in 0..9 {
            slot(rt, m, inv, col, 8 + col * 18, 142);
        }
        let ghosts = rt.new_object(STACK_HANDLER);
        crate::api::call(rt, STACK_HANDLER_INIT, &[ghosts, V::I(2)]);
        rt.put_field(m, "$ghosts", ghosts);
        for i in 0..2 {
            let s = rt.new_object(HANDLER_SLOT);
            crate::api::call(rt, HANDLER_SLOT_INIT, &[s, ghosts, V::I(i), V::I(23 + i * 18), V::I(22)]);
            crate::api::call(rt, ADD_SLOT, &[m, s]);
        }
        m
    }

    /// The mods' classes of these tests, as translated code answers: Create's filter (its menu,
    /// screen, factories and provider, reduced) and vanilla's chest menu over a 3-slot container.
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let this = args[0];
        match (rt.class_of(this), key) {
            (FILTER_MENU, "clicked.(IILnet/minecraft/world/inventory/ClickType;Lnet/minecraft/world/entity/player/Player;)V") => {
                if args[1].i() < 36 {
                    return crate::api::call(rt, owner_key, args);
                }
                // A ghost slot takes one of the carried stack, or empties.
                let held = carried(rt, this);
                let put = if is_empty(rt, held) { empty(rt) } else { copy_with_count(rt, held, 1) };
                let ghosts = rt.get_field(this, "$ghosts");
                crate::api::call(rt, H_SET, &[ghosts, V::I(args[1].i() - 36), put]);
                V::Void
            }
            (FILTER_MENU, "quickMoveStack.(Lnet/minecraft/world/entity/player/Player;I)Lnet/minecraft/world/item/ItemStack;") => {
                let ghosts = rt.get_field(this, "$ghosts");
                let i = args[2].i();
                if i < 36 {
                    let s = slot_item(rt, slot_at(rt, this, i));
                    for g in 0..2 {
                        let there = crate::api::call(rt, H_GET, &[ghosts, V::I(g)]);
                        if is_empty(rt, there) {
                            let one = copy_with_count(rt, s, 1);
                            crate::api::call(rt, H_SET, &[ghosts, V::I(g), one]);
                            break;
                        }
                    }
                } else {
                    let e = empty(rt);
                    crate::api::call(rt, H_SET, &[ghosts, V::I(i - 36), e]);
                }
                empty(rt)
            }
            // `HeldItemGhostItemMenu.stillValid`: the very stack it opened for is still in hand.
            (FILTER_MENU, "stillValid.(Lnet/minecraft/world/entity/player/Player;)Z") => {
                let inv = rt.get_field(this, "$inv");
                let selected = crate::api::call(rt, GET_SELECTED, &[inv]);
                V::I((selected == rt.get_field(this, "$holder")) as i32)
            }
            // `MenuBase.removed`: then `saveData` writes the ghosts into the stack (here: how many
            // as its damage).
            (FILTER_MENU, "removed.(Lnet/minecraft/world/entity/player/Player;)V") => {
                crate::api::call(rt, owner_key, args);
                let ghosts = rt.get_field(this, "$ghosts");
                let mut n = 0;
                for g in 0..2 {
                    let there = crate::api::call(rt, H_GET, &[ghosts, V::I(g)]);
                    n += i32::from(!is_empty(rt, there));
                }
                let holder = rt.get_field(this, "$holder");
                crate::api::call(rt, SET_DAMAGE, &[holder, V::I(n)]);
                V::Void
            }
            (FILTER_SCREEN, "renderBg.(Lnet/minecraft/client/gui/GuiGraphics;FII)V") => {
                let (l, t) = (rt.get_field(this, "leftPos").i(), rt.get_field(this, "topPos").i());
                let (w, h) = (rt.get_field(this, "imageWidth").i(), rt.get_field(this, "imageHeight").i());
                crate::api::call(rt, GUI_FILL, &[args[1], V::I(l), V::I(t), V::I(l + w), V::I(t + h), V::I(BACKGROUND as i32)]);
                V::Void
            }
            (FACTORY, "create.(ILnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/RegistryFriendlyByteBuf;)Lnet/minecraft/world/inventory/AbstractContainerMenu;") => {
                let item = crate::api::call(rt, READ_UTF, &[args[3]]);
                let name = rt.str_of(item).unwrap_or("").to_owned();
                let holder = crate::api_common::stack(rt, &name, 1);
                let ty = rt.get_field(this, "$type");
                filter_menu(rt, ty, args[1].i(), args[2], holder)
            }
            (PROVIDER, "createMenu.(ILnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/world/entity/player/Player;)Lnet/minecraft/world/inventory/AbstractContainerMenu;") => {
                let holder = crate::api::call(rt, MAIN_HAND, &[args[3]]);
                let ty = rt.get_field(this, "$type");
                filter_menu(rt, ty, args[1].i(), args[2], holder)
            }
            (PROVIDER, "getDisplayName.()Lnet/minecraft/network/chat/Component;") => {
                let s = rt.string("Filter");
                crate::api_common::component(rt, s, false, V::NULL)
            }
            (WRITER, "accept.(Ljava/lang/Object;)V") => {
                let item = rt.get_field(this, "$item");
                crate::api::call(rt, WRITE_UTF, &[args[1], item]);
                V::Void
            }
            (SCREEN_CTOR, "create.(Lnet/minecraft/world/inventory/AbstractContainerMenu;Lnet/minecraft/world/entity/player/Inventory;Lnet/minecraft/network/chat/Component;)Lnet/minecraft/client/gui/screens/Screen;") => {
                let s = rt.new_object(FILTER_SCREEN);
                crate::api::call(rt, SCREEN_INIT, &[s, args[1], args[2], args[3]]);
                s
            }
            // Vanilla's `ChestMenu.quickMoveStack`, its container 3 slots.
            (CHEST_MENU, "quickMoveStack.(Lnet/minecraft/world/entity/player/Player;I)Lnet/minecraft/world/item/ItemStack;") => {
                let i = args[2].i();
                let slot = slot_at(rt, this, i);
                if slot.r() == 0 || !has_item(rt, slot) {
                    return empty(rt);
                }
                let s = slot_item(rt, slot);
                let before = copy(rt, s);
                let n = slots(rt, this).len() as i32;
                let (from, to, reverse) = if i < 3 { (3, n, true) } else { (0, 3, false) };
                if !truthy(crate::api::call(rt, MOVE_TO, &[this, s, V::I(from), V::I(to), V::I(reverse as i32)])) {
                    return empty(rt);
                }
                if is_empty(rt, s) {
                    let e = empty(rt);
                    set_by_player(rt, slot, e);
                } else {
                    set_changed(rt, slot);
                }
                before
            }
            _ => crate::api::call(rt, owner_key, args),
        }
    }

    fn runtime() -> Rt {
        crate::install();
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.supers.insert(FILTER_MENU, MENU);
        rt.supers.insert(CHEST_MENU, MENU);
        rt.supers.insert(FILTER_SCREEN, crate::container_screens::CONTAINER_SCREEN);
        for c in [FACTORY, PROVIDER, WRITER, SCREEN_CTOR] {
            rt.supers.insert(c, "java/lang/Object");
        }
        rt
    }

    /// The menu type `test:filter` of the mod, made as Registrate makes Create's.
    fn register_type(rt: &mut Rt) -> V {
        let factory = rt.new_object(FACTORY);
        let ty = crate::api::call(rt, TYPE_CREATE, &[factory]);
        rt.put_field(factory, "$type", ty);
        rt.registered.insert(("menu".into(), "test:filter".into()), ty);
        ty
    }

    fn stack_of(item: &str, count: u32) -> HandStack {
        HandStack { item: item.into(), count, damage: 0, max_damage: 0, components: Vec::new() }
    }

    /// A player holding a filter (paper), stone in inventory slot 9 and dirt in 10, as the server
    /// or as their client knows them.
    fn player_host(client: bool) -> MemoryHost {
        let mut h = MemoryHost {
            client,
            gui: [427, 240],
            player_id: Some(PLAYER),
            inventory: true,
            held: Some(("minecraft:paper".into(), 1)),
            place: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
            ..Default::default()
        };
        h.slots.insert(9, stack_of("minecraft:stone", 10));
        h.slots.insert(10, stack_of("minecraft:dirt", 5));
        for (name, key) in [("key.inventory", "key.keyboard.e"), ("key.pickItem", "key.mouse.middle"), ("key.drop", "key.keyboard.q"), ("key.swapOffhand", "key.keyboard.f")] {
            h.mappings.insert(name.into(), (key.into(), false));
        }
        h
    }

    /// What the game holds in `slot`: item and count.
    fn game_slot(h: &MemoryHost, slot: u8) -> Option<(String, u32)> {
        h.player_slot(slot).map(|s| (s.item, s.count))
    }

    fn to_client(server: &mut MemoryHost, rt: &mut Rt, client: &mut MemoryHost) -> usize {
        let out = std::mem::take(&mut server.payloads);
        for (to, channel, data) in &out {
            assert_eq!(*to, PayloadTarget::Player(PLAYER));
            assert!(crate::glue::payload_from_server(rt, client, channel, data));
        }
        out.len()
    }

    /// What the client sent reaches the server, and the player's tick runs it.
    fn to_server(client: &mut MemoryHost, rt: &mut Rt, server: &mut MemoryHost) -> usize {
        let out = std::mem::take(&mut client.payloads);
        for (to, channel, data) in &out {
            assert_eq!(*to, PayloadTarget::Server);
            assert!(crate::glue::payload_from_client(rt, server, PLAYER, channel, data));
        }
        crate::glue::player_tick(rt, server);
        out.len()
    }

    fn client_menu(rt: &mut Rt) -> V {
        let p = network::local_player(rt);
        rt.get_field(p, "containerMenu")
    }

    /// Opens the filter for the server's player, as `FilterItem.use` does (the held stack's item for
    /// the client's factory).
    fn open_filter(rt: &mut Rt, host: &mut MemoryHost, ty: V) -> V {
        let provider = rt.new_object(PROVIDER);
        rt.put_field(provider, "$type", ty);
        let writer = rt.new_object(WRITER);
        let item = rt.string("minecraft:paper");
        rt.put_field(writer, "$item", item);
        rt.with_host(host, |rt| {
            crate::hand::with_hand(rt, |rt| {
                let p = crate::api_common::acting_player(rt);
                crate::api::call(rt, OPEN_MENU, &[p, provider, writer])
            })
        })
    }

    #[test]
    fn a_menu_opened_on_the_server_opens_its_screen_on_the_client_and_clicks_stay_in_step() {
        let (mut srt, mut crt) = (runtime(), runtime());
        let (mut server, mut client) = (player_host(false), player_host(true));
        let ty = register_type(&mut srt);
        // The client's setup ran (`RegisterMenuScreensEvent`: the screen for the type).
        crate::neo::fire_client_setup(&mut crt);
        let client_ty = register_type(&mut crt);
        let ctor = crt.new_object(SCREEN_CTOR);
        let ev = crt.new_object(REGISTER_SCREENS);
        crate::api::call(&mut crt, REGISTER_SCREEN, &[ev, client_ty, ctor]);
        crate::glue::player_tick(&mut srt, &mut server);

        let id = open_filter(&mut srt, &mut server, ty);
        assert_eq!(crate::api::call(&mut srt, OPTIONAL_INT_GET, &[id]), V::I(1));
        assert_eq!(to_client(&mut server, &mut crt, &mut client), 2, "the menu opens, then its contents");
        let screen = crate::screens::current(&crt);
        assert_eq!(crt.class_of(screen), FILTER_SCREEN);
        assert!(client.screen, "the game opened its screen for the mod's");
        let m = client_menu(&mut crt);
        assert_eq!((crt.class_of(m), crt.get_field(m, "containerId")), (FILTER_MENU, V::I(1)));
        assert_eq!(slots(&crt, m).len(), 38);
        let holder = crt.get_field(m, "$holder");
        assert_eq!(stack_view(&crt, holder), Some(("minecraft:paper".into(), 1)), "the factory read the provider's extra data");
        // The window is centred: (427 - 176) / 2, (240 - 166) / 2.
        assert_eq!((crt.get_field(screen, "leftPos"), crt.get_field(screen, "topPos")), (V::I(125), V::I(37)));

        // Drawn: the window, the slots' stacks, the slot under the pointer lit.
        let over_stone = [125.0 + 8.0 + 4.0, 37.0 + 84.0 + 4.0];
        crate::glue::screen_render(&mut crt, &mut client, over_stone, 0.0);
        assert!(client.drawn_gui.iter().any(|d| matches!(d, GuiDraw::Fill(_, c) if *c == BACKGROUND)));
        let items: Vec<&str> = client.drawn_gui.iter().filter_map(|d| if let GuiDraw::Item(i, ..) = d { Some(i.as_str()) } else { None }).collect();
        assert!(items.contains(&"minecraft:stone") && items.contains(&"minecraft:dirt"), "{items:?}");
        assert_eq!(crt.get_field(screen, "hoveredSlot"), slot_at(&crt, m, 0));

        // A click on the stone picks it up: predicted on the client, run on the server, nothing to
        // correct.
        assert!(crate::glue::screen_input(&mut crt, &mut client, over_stone, Input::Press(0), 0));
        assert!(crate::glue::screen_input(&mut crt, &mut client, over_stone, Input::Release(0), 0));
        let carried_now = carried(&mut crt, m);
        assert_eq!(stack_view(&crt, carried_now), Some(("minecraft:stone".into(), 10)));
        assert_eq!(to_server(&mut client, &mut srt, &mut server), 1);
        let sm = server_player(&mut srt, PLAYER);
        let sm = srt.get_field(sm, "containerMenu");
        let server_carried = carried(&mut srt, sm);
        assert_eq!(stack_view(&srt, server_carried), Some(("minecraft:stone".into(), 10)));
        assert_eq!(game_slot(&server, 9), None, "the stone left the server's inventory");
        assert!(server.payloads.is_empty(), "the client predicted it all: {:?}", server.payloads);

        // A ghost slot takes one of the carried stack without taking it; then the stone goes down in
        // inventory slot 14.
        let ghost = [125.0 + 23.0 + 4.0, 37.0 + 22.0 + 4.0];
        crate::glue::screen_input(&mut crt, &mut client, ghost, Input::Press(0), 0);
        crate::glue::screen_input(&mut crt, &mut client, ghost, Input::Release(0), 0);
        let empty_slot = [125.0 + 8.0 + 5.0 * 18.0 + 4.0, 37.0 + 84.0 + 4.0];
        crate::glue::screen_input(&mut crt, &mut client, empty_slot, Input::Press(0), 0);
        crate::glue::screen_input(&mut crt, &mut client, empty_slot, Input::Release(0), 0);
        assert_eq!(to_server(&mut client, &mut srt, &mut server), 2);
        assert_eq!(game_slot(&server, 14), Some(("minecraft:stone".into(), 10)));
        assert_eq!(game_slot(&client, 14), Some(("minecraft:stone".into(), 10)));
        let ghosts = srt.get_field(sm, "$ghosts");
        let first = crate::api::call(&mut srt, H_GET, &[ghosts, V::I(0)]);
        assert_eq!(stack_view(&srt, first), Some(("minecraft:stone".into(), 1)));
        assert!(server.payloads.is_empty());

        // The game gives the player gold while the menu is open: the server's tick sends it.
        server.slots.insert(11, stack_of("minecraft:gold_ingot", 3));
        crate::glue::player_tick(&mut srt, &mut server);
        assert_eq!(to_client(&mut server, &mut crt, &mut client), 1);
        let gold_slot = slot_at(&crt, m, 2);
        let gold = crt.with_host(&mut client, |rt| {
            let s = slot_item(rt, gold_slot);
            stack_view(rt, s)
        });
        assert_eq!(gold, Some(("minecraft:gold_ingot".into(), 3)));
        assert_eq!(game_slot(&client, 11), Some(("minecraft:gold_ingot".into(), 3)));

        // Escape closes the screen; the server closes the menu and the filter saves into the stack
        // in hand, which reaches the game.
        assert!(crate::glue::screen_input(&mut crt, &mut client, [0.0, 0.0], Input::Key("key.keyboard.escape", true), 0));
        assert_eq!(crate::screens::current(&crt), V::NULL);
        assert!(!client.screen);
        assert_eq!(to_server(&mut client, &mut srt, &mut server), 1);
        let sp = server_player(&mut srt, PLAYER);
        assert!(!menu_open(&srt, sp));
        assert_eq!(server.held, Some(("minecraft:paper".into(), 1)));
        assert_eq!(server.held_damage, 1, "saveData wrote into the held stack");
    }

    #[test]
    fn a_menu_closes_for_both_when_its_stack_leaves_the_hand() {
        let (mut srt, mut crt) = (runtime(), runtime());
        let (mut server, mut client) = (player_host(false), player_host(true));
        let ty = register_type(&mut srt);
        crate::neo::fire_client_setup(&mut crt);
        let client_ty = register_type(&mut crt);
        let ctor = crt.new_object(SCREEN_CTOR);
        let ev = crt.new_object(REGISTER_SCREENS);
        crate::api::call(&mut crt, REGISTER_SCREEN, &[ev, client_ty, ctor]);
        open_filter(&mut srt, &mut server, ty);
        to_client(&mut server, &mut crt, &mut client);
        assert!(client.screen);
        // Still the same stack in hand: the menu stays open tick after tick.
        crate::glue::player_tick(&mut srt, &mut server);
        crate::glue::player_tick(&mut srt, &mut server);
        assert!(server.payloads.is_empty());
        server.held = Some(("minecraft:stick".into(), 1));
        crate::glue::player_tick(&mut srt, &mut server);
        // As `ServerPlayer.tick`: the menu sends what changed (the hotbar slot), then closes.
        let kinds: Vec<u8> = server.payloads.iter().map(|(_, _, d)| d[0]).collect();
        assert_eq!(kinds, vec![UPDATE, CLOSE]);
        to_client(&mut server, &mut crt, &mut client);
        assert_eq!(crate::screens::current(&crt), V::NULL);
        assert!(!client.screen);
        let sp = server_player(&mut srt, PLAYER);
        assert!(!menu_open(&srt, sp));
    }

    /// Vanilla's clicks on a chest-like menu (3 container slots, then the hotbar), with the counts
    /// vanilla's `doClick` gives.
    #[test]
    fn clicks_move_stacks_as_vanilla_does() {
        let mut rt = runtime();
        let mut host = player_host(false);
        host.held = None;
        host.slots.clear();
        rt.with_host(&mut host, |rt| {
            crate::hand::with_hand(rt, |rt| {
                let player = crate::api_common::acting_player(rt);
                let inv = crate::api::call(rt, GET_INVENTORY, &[player]);
                let chest = rt.new_object(SIMPLE_CONTAINER);
                crate::api::call(rt, CONTAINER_INIT, &[chest, V::I(3)]);
                let m = rt.new_object(CHEST_MENU);
                crate::api::call(rt, MENU_INIT, &[m, V::NULL, V::I(1)]);
                for i in 0..3 {
                    slot(rt, m, chest, i, 8 + i * 18, 18);
                }
                for i in 0..9 {
                    slot(rt, m, inv, i, 8 + i * 18, 50);
                }
                let stone = |rt: &mut Rt, n| crate::api_common::stack(rt, "minecraft:stone", n);
                let s = stone(rt, 10);
                virt(rt, C_SET, chest, &[V::I(0), s]);
                let click = |rt: &mut Rt, slot: i32, button: i32, ty: i32| {
                    let ct = crate::api::api_enum(rt, CLICK_TYPE, CLICK_TYPES[ty as usize], ty);
                    virt(rt, K_CLICKED, m, &[V::I(slot), V::I(button), ct, player]);
                };
                let at = |rt: &mut Rt, i: i32| {
                    let s = slot_item(rt, slot_at(rt, m, i));
                    stack_view(rt, s).map_or(0, |(_, n)| n)
                };
                let held = |rt: &mut Rt| {
                    let c = carried(rt, m);
                    stack_view(rt, c).map_or(0, |(_, n)| n)
                };
                // A right click takes the larger half: (10 + 1) / 2.
                click(rt, 0, 1, PICKUP);
                assert_eq!((held(rt), at(rt, 0)), (5, 5));
                // A left drag over three slots leaves floor(5 / 3) in each.
                click(rt, OUTSIDE, 0, QUICK_CRAFT);
                for s in [1, 2, 3] {
                    click(rt, s, 1, QUICK_CRAFT);
                }
                click(rt, OUTSIDE, 2, QUICK_CRAFT);
                assert_eq!((held(rt), at(rt, 1), at(rt, 2), at(rt, 3)), (2, 1, 1, 1));
                // A double click gathers the rest of the kind, slot by slot in order.
                click(rt, 4, 0, PICKUP_ALL);
                assert_eq!((held(rt), at(rt, 0), at(rt, 1), at(rt, 2), at(rt, 3)), (10, 0, 0, 0, 0));
                // Into hotbar slot 1 (menu 4); shift moves it back, onto the 60 there first.
                click(rt, 4, 0, PICKUP);
                let s = stone(rt, 60);
                virt(rt, C_SET, chest, &[V::I(1), s]);
                click(rt, 4, 0, QUICK_MOVE);
                assert_eq!((at(rt, 0), at(rt, 1), at(rt, 4)), (6, 64, 0));
                // The drop key with control throws the whole stack, from the player's eyes.
                click(rt, 1, 1, THROW);
                assert_eq!(at(rt, 1), 0);
                // A hotbar key swaps the slot with that hotbar slot.
                click(rt, 0, 1, SWAP);
                assert_eq!(at(rt, 0), 0);
            })
        });
        assert_eq!(game_slot(&host, 1), Some(("minecraft:stone".into(), 6)));
        let thrown: Vec<(&str, u32, u32)> = host.item_entities.iter().map(|e| (e.item.as_str(), e.count, e.pickup_delay)).collect();
        assert_eq!(thrown, vec![("minecraft:stone", 64, 40)]);
        let e = &host.item_entities[0];
        // Vanilla's `getEyeY() - 0.3F`: the eye height and the 0.3 are floats.
        assert_eq!(e.pos[1], 64.0 + f64::from(1.62f32) - f64::from(0.3f32), "{:?}", e.pos);
        // Thrown as a whole stack (its data with it), not as an item and a count.
        let whole = &host.item_entity_stacks[&e.id];
        assert_eq!((whole.item.as_str(), whole.count), ("minecraft:stone", 64));
    }

    /// `OptionalDouble` and `OptionalLong` as `OptionalInt`: a value of their kind, or none, which
    /// `orElse` replaces and `getAsDouble` refuses with Java's exception.
    #[test]
    fn optional_doubles_and_longs_hold_their_kind() {
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args);
        let some = c(&mut rt, "java/util/OptionalDouble.of.(D)Ljava/util/OptionalDouble;", &[V::D(1.5)]);
        assert_eq!(c(&mut rt, "java/util/OptionalDouble.getAsDouble.()D", &[some]), V::D(1.5));
        let none = c(&mut rt, "java/util/OptionalDouble.empty.()Ljava/util/OptionalDouble;", &[]);
        assert_eq!(c(&mut rt, "java/util/OptionalDouble.isPresent.()Z", &[none]), V::I(0));
        assert_eq!(c(&mut rt, "java/util/OptionalDouble.orElse.(D)D", &[none, V::D(2.5)]), V::D(2.5));
        c(&mut rt, "java/util/OptionalDouble.getAsDouble.()D", &[none]);
        assert!(rt.thrown.take().is_some_and(|e| rt.class_of(e) == "java/util/NoSuchElementException"));
        let long = c(&mut rt, "java/util/OptionalLong.of.(J)Ljava/util/OptionalLong;", &[V::J(7)]);
        assert_eq!(c(&mut rt, "java/util/OptionalLong.orElse.(J)J", &[long, V::J(0)]), V::J(7));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Player.drop(stack, includeThrowerName)` on a server, as a mod's "throw what it holds" does: a
    /// new item entity of the stack, picked up after 40 ticks, joins the level at the player's eyes
    /// less 0.3 (1.62 standing), thrown the way they look (south, level): 0.3 forward and 0.1 up,
    /// with a spread of up to 0.02 and a lift of up to 0.1 either way. `drop(stack, true, false)`
    /// throws it around them: 0.2 up, at most 0.5 out. An empty stack drops nothing; a client's drop
    /// is the server's to make, so it is only returned.
    #[test]
    fn a_player_throws_a_stack_from_their_eyes_the_way_they_look() {
        const DROP: &str = "net/minecraft/world/entity/player/Player.drop.(Lnet/minecraft/world/item/ItemStack;Z)Lnet/minecraft/world/entity/item/ItemEntity;";
        const DROP_AROUND: &str = "net/minecraft/world/entity/player/Player.drop.(Lnet/minecraft/world/item/ItemStack;ZZ)Lnet/minecraft/world/entity/item/ItemEntity;";
        let mut rt = runtime();
        let mut h = MemoryHost { player_id: Some(PLAYER), place: Some(([0.5, 64.0, 0.5], 0.0, 0.0)), ..MemoryHost::default() };
        rt.with_host(&mut h, |rt| {
            let p = server_player(rt, PLAYER);
            let diamonds = crate::hand::load(rt, Some(stack_of("minecraft:diamond", 3)));
            assert_ne!(crate::api::call(rt, DROP, &[p, diamonds, V::I(0)]), V::NULL);
            let stone = crate::hand::load(rt, Some(stack_of("minecraft:stone", 1)));
            assert_ne!(crate::api::call(rt, DROP_AROUND, &[p, stone, V::I(1), V::I(0)]), V::NULL);
            let empty = crate::hand::load(rt, None);
            assert_eq!(crate::api::call(rt, DROP, &[p, empty, V::I(0)]), V::NULL);
        });
        let eyes = 64.0 + f64::from(1.62f32) - f64::from(0.3f32);
        let [thrown, around] = &h.item_entities[..] else { panic!("{:?}", h.item_entities.len()) };
        assert_eq!((thrown.item.as_str(), thrown.count, thrown.pickup_delay, thrown.pos), ("minecraft:diamond", 3, 40, [0.5, eyes, 0.5]));
        let [x, y, z] = thrown.motion;
        assert!(x.hypot(z - 0.3) <= 0.02 + 1e-9 && (0.0..0.2).contains(&y), "{:?}", thrown.motion);
        assert_eq!((around.item.as_str(), around.pos), ("minecraft:stone", [0.5, eyes, 0.5]));
        let [x, y, z] = around.motion;
        assert!(x.hypot(z) < 0.5 && y == f64::from(0.2f32), "{:?}", around.motion);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);

        let mut rt = runtime();
        rt.interned.insert("compat:client".into(), V::I(1));
        let mut client = MemoryHost { client: true, player_id: Some(PLAYER), place: Some(([0.5, 64.0, 0.5], 0.0, 0.0)), ..MemoryHost::default() };
        rt.with_host(&mut client, |rt| {
            let p = server_player(rt, PLAYER);
            let diamonds = crate::hand::load(rt, Some(stack_of("minecraft:diamond", 3)));
            assert_ne!(crate::api::call(rt, DROP, &[p, diamonds, V::I(0)]), V::NULL);
        });
        assert!(client.item_entities.is_empty());
    }

    /// With nobody acting (a block entity's tick), another player's inventory is theirs by UUID, as
    /// Create's toolbox hands a player their tool: what the tick reads and puts there is that
    /// player's (a stack put whole, one changed in place in a hook), and `add` fills their slots as
    /// vanilla's; the hands the host acts with stay as they were. The acting player's own object
    /// still reaches the acting player's inventory.
    #[test]
    fn another_players_inventory_is_reached_by_their_uuid() {
        const INV_GET: &str = "net/minecraft/world/entity/player/Inventory.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
        const INV_SET: &str = "net/minecraft/world/entity/player/Inventory.setItem.(ILnet/minecraft/world/item/ItemStack;)V";
        const INV_ADD: &str = "net/minecraft/world/entity/player/Inventory.add.(Lnet/minecraft/world/item/ItemStack;)Z";
        const SHRINK: &str = "net/minecraft/world/item/ItemStack.shrink.(I)V";
        const OTHER: u128 = 0xBEEF;
        let mut rt = runtime();
        let mut h = MemoryHost {
            held: Some(("minecraft:stick".into(), 1)),
            others: std::collections::HashMap::from([(OTHER, std::collections::HashMap::from([(9, stack_of("minecraft:dirt", 64))]))]),
            ..MemoryHost::default()
        };
        rt.with_host(&mut h, |rt| {
            let p = server_player(rt, OTHER);
            let inv = crate::api::call(rt, GET_INVENTORY, &[p]);
            let first = crate::api::call(rt, INV_GET, &[inv, V::I(0)]);
            assert!(is_empty(rt, first), "their slot 0, not the stick in the host's hand");
            let dirt = crate::api::call(rt, INV_GET, &[inv, V::I(9)]);
            assert_eq!(count(rt, dirt), 64);
            let pick = crate::hand::load(rt, Some(stack_of("minecraft:iron_pickaxe", 1)));
            crate::api::call(rt, SET_DAMAGE, &[pick, V::I(125)]);
            crate::api::call(rt, INV_SET, &[inv, V::I(0), pick]);
            crate::hand::with_hand(rt, |rt| {
                let dirt = crate::api::call(rt, INV_GET, &[inv, V::I(9)]);
                crate::api::call(rt, SHRINK, &[dirt, V::I(1)]);
            });
            // Onto their dirt first, then their first empty slot.
            let more = crate::hand::load(rt, Some(stack_of("minecraft:dirt", 3)));
            assert_eq!(crate::api::call(rt, INV_ADD, &[inv, more]).i(), 1);
            let gold = crate::hand::load(rt, Some(stack_of("minecraft:gold_ingot", 2)));
            assert_eq!(crate::api::call(rt, INV_ADD, &[inv, gold]).i(), 1);
        });
        let theirs = |slot: u8| h.others[&OTHER].get(&slot).map(|s| (s.item.clone(), s.count, s.damage));
        assert_eq!(theirs(0), Some(("minecraft:iron_pickaxe".into(), 1, 125)));
        assert_eq!(theirs(9), Some(("minecraft:dirt".into(), 64, 0)));
        assert_eq!(theirs(1), Some(("minecraft:dirt".into(), 2, 0)));
        assert_eq!(theirs(2), Some(("minecraft:gold_ingot".into(), 2, 0)));
        assert_eq!(h.held, Some(("minecraft:stick".into(), 1)), "the host's hand isn't theirs");

        h.player_id = Some(PLAYER);
        rt.with_host(&mut h, |rt| {
            let p = server_player(rt, PLAYER);
            let inv = crate::api::call(rt, GET_INVENTORY, &[p]);
            let held = crate::api::call(rt, INV_GET, &[inv, V::I(0)]);
            assert_eq!(crate::api_common::item_like_name(rt, rt.get_field(held, "$item")), "minecraft:stick");
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
