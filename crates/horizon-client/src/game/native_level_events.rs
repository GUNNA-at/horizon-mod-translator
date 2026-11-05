//! Level events for native mods (docs/native-mods.md, "Level events"): a mod's `Level.levelEvent`
//! made as the game's own, its sound and particles for the players near.
use super::Game;

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
}
