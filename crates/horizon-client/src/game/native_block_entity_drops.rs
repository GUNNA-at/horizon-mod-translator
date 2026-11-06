//! What a mod's block entity gives the item its block drops: the block's loot table copies
//! components from its block entity (`copy_components`, source `block_entity`), as vanilla's
//! `CopyComponentsFunction` reads `BlockEntity.collectComponents()`, and the mod names them
//! (`native_block_entity_components`). A cooking pot keeps its meal, a toolbox its contents.
use super::Game;
use horizon_client::containers::BlockEntity;
use horizon_world::nbt::{Compound, JavaString};
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    /// As block `state` at `pos` breaks with its block entity `entity`, both still there: when
    /// that is a mod's and the block's loot table copies components from it, the ones the mod names
    /// that the table copies wait for the item the table drops (`Pending::attach`).
    pub(super) fn record_block_entity_copies(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        entity: &BlockEntity,
    ) {
        let BlockEntity::Mod(m) = entity else {
            return;
        };
        let registry = self.pack.assets.registry;
        let copies = world_items!(self).block_entity_copies(registry, state);
        if copies.is_empty() {
            return;
        }
        let mut components = Vec::new();
        self.native_block_entity_components(pos, m, &mut components);
        let patch: Compound = components
            .into_iter()
            .filter(|(name, _)| copies.iter().any(|c| c.copies(name)))
            .map(|(name, value)| (JavaString::from(name.as_str()), value))
            .collect();
        if patch.is_empty() {
            return;
        }
        // The entry the function sits on, else the block's own item.
        let item = copies
            .iter()
            .find_map(|c| c.item.clone())
            .unwrap_or_else(|| self.res.facts().get(state).name.to_string());
        self.pending.copied.insert(pos, (item, patch));
    }
}
