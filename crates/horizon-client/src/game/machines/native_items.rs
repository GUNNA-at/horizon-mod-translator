//! Items in a block's container for native server mods (docs/native-mods.md,
//! API 48): read them through a side, take them and move them, by the rules
//! hoppers follow. They act where the world is simulated, never on a client's
//! copy, and only on the level in place, which those that read or take name by
//! its `dimension` (`minecraft:overworld`), as `native_set_block` does.
use super::{Game, Kind, View};
use horizon_client::items::ItemStack;
use horizon_core::geometry::Direction;

/// A slot of a container as a side reaches it (`native_container_slots`).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeContainerSlot {
    /// Its number, for `native_extract_item`.
    pub(crate) slot: usize,
    pub(crate) stack: Option<ItemStack>,
    /// Whether its stack can be taken through this side: not a furnace's fuel
    /// through its bottom, unless it's a bucket, nor a brewing stand's reagent
    /// unless it's a glass bottle, nor a mod's slot outside its `outputs`.
    pub(crate) can_take: bool,
}

/// Whether a mod can take items from this container (`native_item_source`).
fn takes_from(view: &View) -> bool {
    !matches!(view.kind, Kind::Composter(_)) && view.len() > 0
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The level in place (`minecraft:overworld`), for a hook that isn't told
    /// which it runs in. API 48.
    pub(in crate::game) fn native_dimension(&self) -> &'static str {
        self.dim.id()
    }
    /// Whether this game simulates a world, as `native_set_block` asks
    /// (the fluid helpers ask it too).
    pub(super) fn native_simulates(&self) -> bool {
        self.serving.is_some() || self.options.dedicated.is_some()
    }
    /// The container at `pos` in the level in place, `dimension`, if this
    /// level simulates and holds items there. A composter's bone meal is for
    /// hoppers: it empties the block, and a refused move would lose it.
    fn native_item_source(&self, dimension: &str, pos: [i32; 3]) -> Option<View> {
        if !self.native_simulates() || dimension != self.dim.id() {
            return None;
        }
        let view = self.containers().view((pos[0], pos[1], pos[2]), false)?;
        takes_from(&view).then_some(view)
    }
    /// One item into `view` through `face`, trying `slots`, without
    /// `setChanged`: the caller tells the container once it's done. The item,
    /// when nothing took it.
    fn add_one_quietly(
        &mut self,
        view: &View,
        slots: &[usize],
        one: ItemStack,
        face: Direction,
    ) -> Option<ItemStack> {
        if let Kind::Composter(_) = view.kind {
            let (x, y, z) = view.parts[0].0;
            return self.composter_container_insert([x, y, z], one, face == Direction::Up);
        }
        let mut rest = Some(one);
        for &slot in slots {
            let Some(one) = rest.take() else { break };
            rest = self.move_in_item(None, view, one, slot).0;
        }
        rest
    }
    /// `native_insert_item`: `tryMoveInItem` puts all it's given in an empty
    /// slot, and a hopper gives it one item, so a bigger stack goes in one by
    /// one or it would pass the slot's limit. The rest.
    pub(in crate::game) fn insert_stack_through(
        &mut self,
        pos: [i32; 3],
        face: Direction,
        stack: ItemStack,
    ) -> Option<ItemStack> {
        if !self.native_simulates() {
            return Some(stack);
        }
        let Some(view) = self.containers().view((pos[0], pos[1], pos[2]), false) else {
            return Some(stack);
        };
        let slots = view.slots_to_add(Some(face));
        let mut left = stack.count;
        while left > 0 {
            let one = ItemStack {
                count: 1,
                ..stack.clone()
            };
            if self.add_one_quietly(&view, &slots, one, face).is_some() {
                break;
            }
            left -= 1;
            // A composter's `InputContainer` holds one item.
            if matches!(view.kind, Kind::Composter(_)) {
                break;
            }
        }
        if left < stack.count {
            self.view_changed(&view);
        }
        (left > 0).then_some(ItemStack {
            count: left,
            ..stack
        })
    }
    /// The slots of the container at `pos` that its side `face` reaches, with
    /// their stacks. None where no container is: another dimension, a chunk
    /// that isn't loaded, a block without slots, a composter, or the mod block
    /// entity ticking now (it's out of the world, and has its items in hand).
    /// A double chest's slots are both halves'. API 48.
    pub(in crate::game) fn native_container_slots(
        &self,
        dimension: &str,
        pos: [i32; 3],
        face: Direction,
    ) -> Option<Vec<NativeContainerSlot>> {
        let view = self.native_item_source(dimension, pos)?;
        let c = self.containers();
        Some(
            view.slots_for_face(Some(face))
                .into_iter()
                .map(|slot| {
                    let stack = c.get(&view, slot);
                    NativeContainerSlot {
                        slot,
                        stack: stack.cloned(),
                        can_take: stack.is_some_and(|s| c.can_take(&view, slot, s, face)),
                    }
                })
                .collect(),
        )
    }
    /// Takes up to `count` items out of slot `slot` of the container at `pos`,
    /// through its side `face`; they're the mod's to place then, as a hopper's
    /// are: to hand them on without losing any, use `native_move_items`. None
    /// when nothing can be taken: see `native_container_slots`. API 48.
    pub(in crate::game) fn native_extract_item(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        face: Direction,
        slot: usize,
        count: u32,
    ) -> Option<ItemStack> {
        let view = self.native_item_source(dimension, pos)?;
        if count == 0 || !view.slots_for_face(Some(face)).contains(&slot) {
            return None;
        }
        let c = self.containers();
        if !c.can_take(&view, slot, c.get(&view, slot)?, face) {
            return None;
        }
        let target = self.slot_mut_at(&view, slot)?;
        let current = target.as_mut()?;
        let taken = count.min(current.count);
        let out = ItemStack {
            count: taken,
            ..current.clone()
        };
        current.count -= taken;
        if current.count == 0 {
            *target = None;
        }
        self.view_changed(&view);
        Some(out)
    }
    /// Moves up to `max` items from the container at `from`, through its side
    /// `from_face`, into the one at `to`, through its side `to_face`, one by
    /// one as a hopper does: an item the target refuses stays in its slot, so
    /// none is lost. `item` (`minecraft:coal`) takes only that item. How many
    /// moved: 0 where `native_container_slots` is none at `from`, where no
    /// container is at `to`, and within one container. A composter takes one
    /// a call. API 48.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::game) fn native_move_items(
        &mut self,
        dimension: &str,
        from: [i32; 3],
        from_face: Direction,
        to: [i32; 3],
        to_face: Direction,
        item: Option<&str>,
        max: u32,
    ) -> u32 {
        let Some(source) = self.native_item_source(dimension, from) else {
            return 0;
        };
        let Some(target) = self.containers().view((to[0], to[1], to[2]), false) else {
            return 0;
        };
        // A double chest's halves are one container.
        if source.parts.iter().any(|p| target.parts.contains(p)) {
            return 0;
        }
        let max = if matches!(target.kind, Kind::Composter(_)) {
            max.min(1)
        } else {
            max
        };
        let into = target.slots_to_add(Some(to_face));
        let mut moved = 0;
        for slot in source.slots_for_face(Some(from_face)) {
            while moved < max {
                let c = self.containers();
                let Some(stack) = c.get(&source, slot) else {
                    break;
                };
                if item.is_some_and(|i| i != &*stack.item)
                    || !c.can_take(&source, slot, stack, from_face)
                {
                    break;
                }
                let Some(one) = self.take_one(&source, slot) else {
                    break;
                };
                if let Some(rest) = self.add_one_quietly(&target, &into, one, to_face) {
                    self.put_back(&source, slot, rest);
                    break;
                }
                moved += 1;
            }
        }
        if moved > 0 {
            self.view_changed(&source);
            self.view_changed(&target);
        }
        moved
    }
}

/// The item handler NeoForge gives a vanilla block's container
/// (`CapabilityHooks.registerVanillaProviders`, `native_vanilla_container`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeContainerKind {
    /// An `InvWrapper`: a chest's (both halves of a double one), a barrel's,
    /// a dispenser's, a dropper's, a crafter's, a chiseled bookshelf's or a
    /// jukebox's container.
    Plain,
    /// A hopper's `VanillaHopperItemHandler`.
    Hopper,
    /// A `SidedInvWrapper` of a furnace, a blast furnace or a smoker.
    Furnace,
    /// A `SidedInvWrapper` of a brewing stand.
    Brewing,
    /// A `SidedInvWrapper` of a shulker box.
    Shulker,
}

// Translated NeoForge mods use these (their item handlers of vanilla's
// containers); nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The container of the vanilla block at `pos` that NeoForge gives an item
    /// handler, with that handler's kind; None for any other block (a
    /// composter, whose handler is its block state's), and where the world
    /// isn't simulated.
    fn vanilla_handler_view(&self, pos: [i32; 3]) -> Option<(View, NativeContainerKind)> {
        if !self.native_simulates() {
            return None;
        }
        let c = self.containers();
        // `ChestBlock.getContainer(.., true)`: a chest under a block still gives its items.
        let view = c.view((pos[0], pos[1], pos[2]), false)?;
        let kind = match view.kind {
            Kind::Hopper => NativeContainerKind::Hopper,
            Kind::Furnace => NativeContainerKind::Furnace,
            Kind::Brewing => NativeContainerKind::Brewing,
            Kind::Shulker => NativeContainerKind::Shulker,
            Kind::Crafter | Kind::Bookshelf | Kind::Jukebox => NativeContainerKind::Plain,
            Kind::Generic => {
                let name = c.name(self.world.block(pos[0], pos[1], pos[2]));
                let plain = horizon_client::containers::is_chest_block(name)
                    || matches!(
                        name,
                        "minecraft:barrel" | "minecraft:dispenser" | "minecraft:dropper"
                    );
                if !plain {
                    return None;
                }
                NativeContainerKind::Plain
            }
            Kind::Mod | Kind::Composter(_) => return None,
        };
        Some((view, kind))
    }
    /// The vanilla container at `pos` that NeoForge gives an item handler
    /// (`Capabilities.ItemHandler.BLOCK`): its slot count, the most a slot
    /// holds (`getMaxStackSize()`: 1 in a jukebox or a chiseled bookshelf,
    /// else 99) and its handler's kind. None for any other block, a composter,
    /// and where the world isn't simulated. API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container(
        &self,
        pos: [i32; 3],
    ) -> Option<(u32, u32, NativeContainerKind)> {
        let (view, kind) = self.vanilla_handler_view(pos)?;
        let max = if matches!(view.kind, Kind::Jukebox | Kind::Bookshelf) {
            1
        } else {
            99
        };
        Some((view.len() as u32, max, kind))
    }
    /// What slot `slot` of that container holds (`Container.getItem`).
    /// API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container_item(
        &self,
        pos: [i32; 3],
        slot: u32,
    ) -> Option<ItemStack> {
        let (view, _) = self.vanilla_handler_view(pos)?;
        self.containers().get(&view, slot as usize).cloned()
    }
    /// Puts `stack` (None empties the slot) in slot `slot` of that container
    /// (`Container.setItem`: the stack cut to what the slot holds; a disc put in
    /// a jukebox starts its song, one taken out stops it), without
    /// `setChanged` (`native_vanilla_container_changed`). False where there is
    /// no such slot. API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container_set(
        &mut self,
        pos: [i32; 3],
        slot: u32,
        stack: Option<ItemStack>,
    ) -> bool {
        let Some((view, _)) = self.vanilla_handler_view(pos) else {
            return false;
        };
        let c = self.containers();
        let stack = stack.filter(|s| s.count > 0).map(|mut s| {
            // `stack.limitSize(getMaxStackSize(stack))`.
            let most = if matches!(view.kind, Kind::Jukebox | Kind::Bookshelf) {
                1
            } else {
                c.max_stack(&s).min(99)
            };
            s.count = s.count.min(most);
            s
        });
        let had = c.get(&view, slot as usize).is_some();
        let put = stack.is_some();
        let Some(target) = self.slot_mut_at(&view, slot as usize) else {
            return false;
        };
        *target = stack;
        if view.kind == Kind::Jukebox {
            let (x, y, z) = view.parts[0].0;
            if put {
                self.jukebox_inserted([x, y, z]);
            } else if had {
                self.jukebox_taken([x, y, z]);
            }
        }
        true
    }
    /// `Container.setChanged` of that container: the level saves it,
    /// comparators read it again. API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container_changed(&mut self, pos: [i32; 3]) {
        if let Some((view, _)) = self.vanilla_handler_view(pos) {
            self.view_changed(&view);
        }
    }
    /// Whether `stack` may go in slot `slot` of that container
    /// (`Container.canPlaceItem`); `through`: through a face, as a
    /// `SidedInvWrapper` puts it (`canPlaceItemThroughFace` too, which no
    /// vanilla container decides by the face: a shulker box takes no shulker
    /// box). API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container_can_place(
        &self,
        pos: [i32; 3],
        slot: u32,
        stack: &ItemStack,
        through: bool,
    ) -> bool {
        let Some((view, _)) = self.vanilla_handler_view(pos) else {
            return false;
        };
        // A shulker box's own `canPlaceItem` is `Container`'s: anything goes.
        if view.kind == Kind::Shulker && !through {
            return (slot as usize) < view.len();
        }
        (slot as usize) < view.len() && self.containers().can_place(&view, slot as usize, stack)
    }
    /// The slots of that container its side `face` reaches, in order
    /// (`WorldlyContainer.getSlotsForFace`; every slot of one that isn't a
    /// `WorldlyContainer`). API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container_face_slots(
        &self,
        pos: [i32; 3],
        face: Direction,
    ) -> Vec<u32> {
        self.vanilla_handler_view(pos).map_or_else(Vec::new, |(view, _)| {
            view.slots_for_face(Some(face))
                .into_iter()
                .map(|s| s as u32)
                .collect()
        })
    }
    /// Whether `stack` may be taken out of slot `slot` of that container through
    /// its side `face` (`WorldlyContainer.canTakeItemThroughFace`: a furnace's
    /// fuel only as a bucket through its bottom, a brewing stand's ingredient
    /// only as a glass bottle). API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_vanilla_container_can_take(
        &self,
        pos: [i32; 3],
        slot: u32,
        stack: &ItemStack,
        face: Direction,
    ) -> bool {
        self.vanilla_handler_view(pos)
            .is_some_and(|(view, _)| self.containers().can_take(&view, slot as usize, stack, face))
    }
    /// The hopper at `pos`, empty before, took items through its item handler
    /// (`VanillaHopperItemHandler.insertItem`): it waits 8 ticks before it moves
    /// any (`setCooldown(8)`), unless it waits longer already
    /// (`isOnCustomCooldown`). API @API:vanilla_item_handlers@.
    pub(in crate::game) fn native_hopper_filled(&mut self, pos: [i32; 3]) {
        if !matches!(self.vanilla_handler_view(pos), Some((_, NativeContainerKind::Hopper))) {
            return;
        }
        let state = self.hoppers.entry((pos[0], pos[1], pos[2])).or_default();
        if state.cooldown <= 8 {
            state.cooldown = 8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::containers::ModContainer;
    use std::sync::Arc;

    fn view(kind: Kind, slots: usize) -> View {
        View {
            parts: vec![((0, 0, 0), slots)],
            kind,
            container: (kind == Kind::Mod).then(|| {
                Arc::new(ModContainer {
                    slots,
                    ..Default::default()
                })
            }),
        }
    }

    #[test]
    fn a_composter_and_a_block_without_slots_give_nothing() {
        assert!(takes_from(&view(Kind::Generic, 27)));
        assert!(takes_from(&view(Kind::Mod, 3)));
        assert!(!takes_from(&view(Kind::Generic, 0)));
        assert!(!takes_from(&view(Kind::Composter(8), 1)));
    }

    #[test]
    fn a_face_only_narrows_the_slots_of_a_worldly_container() {
        assert_eq!(
            view(Kind::Furnace, 3).slots_to_add(Some(Direction::Up)),
            [0]
        );
        assert_eq!(
            view(Kind::Generic, 2).slots_to_add(Some(Direction::Up)),
            [0, 1]
        );
        assert_eq!(
            view(Kind::Shulker, 2).slots_to_add(Some(Direction::Up)),
            [0, 1]
        );
        assert_eq!(view(Kind::Furnace, 3).slots_to_add(None), [0, 1, 2]);
    }
}
