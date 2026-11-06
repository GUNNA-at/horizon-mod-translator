//! A block's shapes for native mods (docs/native-mods.md, "Block shapes"): its outline, what a
//! player targets of it.
use super::Game;

impl Game<'_> {
    /// The outline of the block at `pos` (`BlockState.getShape` with its random offset: what a
    /// player targets, and the game's own ray stops at), as boxes relative to the block's corner;
    /// none for air. API @API:native_outline_boxes@.
    #[allow(dead_code)]
    pub(crate) fn native_outline_boxes(&self, pos: [i32; 3]) -> Vec<[f64; 6]> {
        let o = pos.map(f64::from);
        self.outline_boxes(pos)
            .into_iter()
            .map(|b| std::array::from_fn(|i| b[i] - o[i % 3]))
            .collect()
    }
}
