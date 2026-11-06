//! Item durability for native mods (docs/native-mods.md, "Item durability"): the damage at which
//! a vanilla item breaks, for the stacks a mod makes of it.
use super::Game;

impl Game<'_> {
    /// The damage at which `item` breaks (its `max_damage` component: 250 for an iron sword, 1561
    /// for a diamond pickaxe), 0 for an item without durability; None for an item the game doesn't
    /// have. API @API:native_item_max_damage@.
    #[allow(dead_code)]
    pub(crate) fn native_item_max_damage(&self, item: &str) -> Option<u32> {
        self.res.items().get(item).map(|i| i.max_damage)
    }
}
