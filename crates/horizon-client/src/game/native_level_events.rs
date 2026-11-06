//! Level events for native mods (docs/native-mods.md, "Level events"): a mod's `Level.levelEvent`
//! made as the game's own, its sound and particles for the players near.
use super::Game;
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    /// Level event `event` at `pos` with `data` (`Level.levelEvent`, vanilla's `LevelEvent` codes: a
    /// dispenser's click 1000, a door's sound 1005..., smoke 2000, a block breaking 2001 with its
    /// state's id in `data`, bone meal 1505...): made here as the game's own events are, and on a
    /// server sent to its players near. Whether it was made. API @API:native_level_event@.
    #[allow(dead_code)]
    pub(crate) fn native_level_event(&mut self, event: i32, pos: [i32; 3], data: i32) -> bool {
        self.level_event_at(event, pos, data);
        true
    }
    /// The game's id of the block state `text` names (as `native_state_text`
    /// writes it; properties not named keep the block's defaults): what
    /// `Block.getId` gives, and level event 2001 carries. None for an unknown
    /// block, property or value. API @API:native_level_event@.
    #[allow(dead_code)]
    pub(crate) fn native_state_id_of(&self, text: &str) -> Option<u32> {
        super::native_hooks::state_from_text(self.pack.assets.registry, text).map(|s| s.0)
    }
    /// The block state of id `id` as text (`Block.stateById`); air past the
    /// state table. API @API:native_level_event@.
    #[allow(dead_code)]
    pub(crate) fn native_state_of_id(&self, id: u32) -> String {
        super::native_hooks::state_id_text(self.pack.assets.registry, BlockStateId(id))
    }
}
