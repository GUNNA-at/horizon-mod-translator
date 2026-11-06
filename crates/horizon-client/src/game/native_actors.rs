//! What a mod's own actor does in the world through the game: a fake player (NeoForge's
//! `FakePlayer`, Create's deployer) places a block item's block as the game places a player's or a
//! dispenser's: `BlockItem.place` through the vanilla rules (a mod block's own state for the
//! placement included), then the block entity, the item's data on it, the level events (the place
//! sound) and a mod block's placement hook.
use super::Game;
use horizon_client::items::ItemStack;
use horizon_core::{
    geometry::{Direction, Vec3},
    position::BlockPos,
};
use horizon_world::level::{PlaceContext, PlaceOutcome};

/// Direction ordinals as vanilla numbers them: down, up, north, south, west, east.
#[allow(dead_code)]
const FACES: [Direction; 6] = [
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

impl Game<'_> {
    /// `BlockItem.place` of `stack`'s block by a mod's fake player: clicking block `pos` on face
    /// `face` (a direction ordinal) at `hit`, `inside` its shape or not, looking `yaw`/`pitch`,
    /// sneaking or not. Where the block went; None when `stack` is no block item or vanilla's
    /// placement fails (nothing is written then). The stack is the caller's to shrink. Only a
    /// translated mod's code calls it.
    #[allow(dead_code, clippy::too_many_arguments)]
    pub(super) fn native_place_item_block(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: u8,
        hit: [f64; 3],
        inside: bool,
        (yaw, pitch): (f32, f32),
        sneaking: bool,
    ) -> Option<[i32; 3]> {
        self.items().get(&stack.item)?.block.as_ref()?;
        let ctx = PlaceContext {
            clicked_pos: BlockPos::new(pos[0], pos[1], pos[2]),
            face: FACES[usize::from(face.min(5))],
            hit: Vec3::new(hit[0], hit[1], hit[2]),
            inside,
            yaw,
            pitch,
            sneaking,
            item: stack.item.to_string(),
            can_use_game_master_blocks: false,
        };
        let outcome = self.with_level(|b, level, ticks| b.place_block(level, ticks, &ctx));
        let PlaceOutcome::Placed { pos: placed, state } = outcome else {
            let _ = self.process_level_events();
            return None;
        };
        let target = [placed.x, placed.y, placed.z];
        let state = self.apply_item_block_state(target, state, stack).ok()?;
        let name = self.res.facts().get(state).name.clone();
        if let Some(entity) = self.new_block_entity(&name) {
            self.world.block_entities.insert((target[0], target[1], target[2]), entity);
            self.apply_placed_item(target, stack, false);
        }
        let _ = self.process_level_events();
        self.native_place_block(stack, target, state);
        let _ = self.check_wither_spawn(target);
        let _ = self.check_golem_spawn(target);
        Some(target)
    }
}
