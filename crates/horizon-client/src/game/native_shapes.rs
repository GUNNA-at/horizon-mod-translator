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

    /// The face of the occlusion shape of the state `text` names toward `direction`
    /// (`Direction`'s ordinal: down, up, north, south, west, east), as the game hides the faces
    /// beside it (`getFaceOcclusionShape`): boxes in block space, as deep as the block; none for a
    /// face that hides nothing (any face of a state that can't occlude). None for an unknown
    /// state, and in a game that draws nothing (a server). API @API:state_occlusion@.
    #[allow(dead_code)]
    pub(crate) fn native_face_occlusion(&self, text: &str, direction: u8) -> Option<Vec<[f64; 6]>> {
        let state = super::native_hooks::state_from_text(self.pack.assets.registry, text)?;
        let direction = *horizon_core::geometry::Direction::ALL.get(usize::from(direction))?;
        let (_, face) = self.pack.context.tables.visibility.occlusion_face(state, direction)?;
        Some(
            face.boxes()
                .iter()
                .map(|b| {
                    let (lo, hi) = (b.min_position(), b.max_position());
                    [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
                })
                .collect(),
        )
    }
}
