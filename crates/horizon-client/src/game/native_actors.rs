//! What a mod's own actor does in the world through the game: a fake player (NeoForge's
//! `FakePlayer`, Create's deployer) places a block item's block as the game places a player's or a
//! dispenser's: `BlockItem.place` through the vanilla rules (a mod block's own state for the
//! placement included), then the block entity, the item's data on it, the level events (the place
//! sound) and a mod block's placement hook.
//!
//! One that breaks blocks (the deployer punching, Create's drills and saws) asks what the game
//! knows of a break: whether the block needs the right tool for its drops, what a tool does to it
//! (its speed, whether it is right for the drops, its wear per block), the block's sounds, its loot
//! rolled with the tool, its experience and the orbs it pops, an infested block's silverfish
//! afterwards, whether a block is solid, and the cracks shown on it.
use super::Game;
use super::native_hooks::state_from_text;
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

// Only translated mods' generated code calls these, so a build without one never does.
#[allow(dead_code)]
impl Game<'_> {
    /// `BlockItem.place` of `stack`'s block by a mod's fake player: clicking block `pos` on face
    /// `face` (a direction ordinal) at `hit`, `inside` its shape or not, looking `yaw`/`pitch`,
    /// sneaking or not. Where the block went; None when `stack` is no block item or vanilla's
    /// placement fails (nothing is written then). The stack is the caller's to shrink. Only a
    /// translated mod's code calls it. API @API:actors@.
    #[allow(clippy::too_many_arguments)]
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

    /// Whether only the right tool gets the drops of the block state `text` names
    /// (`BlockState.requiresCorrectToolForDrops`); None for a state the game doesn't know.
    /// API @API:actors@.
    pub(super) fn native_state_requires_tool(&self, text: &str) -> Option<bool> {
        let state = state_from_text(self.pack.assets.registry, text)?;
        Some(self.res.facts().get(state).requires_tool)
    }

    /// What item `item`'s tool does to the block state `text` names: its mining speed
    /// (`Tool.getMiningSpeed`), whether it is right for the block's drops (`isCorrectForDrops`)
    /// and what each block mined wears it (`damagePerBlock`). None for an item without a tool
    /// component, or a state the game doesn't know. API @API:actors@.
    pub(super) fn native_item_tool(&self, item: &str, text: &str) -> Option<(f32, bool, u32)> {
        let per_block = self.res.items().get(item)?.tool_damage_per_block?;
        let state = state_from_text(self.pack.assets.registry, text)?;
        let name = self.res.facts().get(state).name.clone();
        let held = ItemStack::new(item, 1);
        let items = self.res.items();
        Some((items.mining_speed(Some(&held), &name), items.correct_for_drops(Some(&held), &name), per_block))
    }

    /// The `SoundType` of the block state `text` names: volume, pitch and its break, step, place,
    /// hit and fall sounds' ids. None for a state the game doesn't know. API @API:actors@.
    pub(super) fn native_state_sounds(&self, text: &str) -> Option<(f32, f32, [String; 5])> {
        let state = state_from_text(self.pack.assets.registry, text)?;
        let s = &self.res.facts().get(state).sounds;
        let events = [&s.break_sound, &s.step, &s.place, &s.hit, &s.fall].map(|e| e.to_string());
        Some((s.volume, s.pitch, events))
    }

    /// `Block.getDrops(state, level, pos, blockEntity, entity, tool)` for the block state `text`
    /// names broken at `pos` with `tool` (None: by hand): its loot table rolled as a player's break
    /// rolls it, with what the game kept of the block entity there for the break (its name, its
    /// contents). Nothing spawns and no mod hears of them: the drops are the caller's (a mod's
    /// actor's, which puts them in its own inventory). API @API:actors@.
    pub(super) fn native_block_loot(&mut self, pos: [i32; 3], text: &str, tool: Option<&ItemStack>) -> Vec<ItemStack> {
        let Some(state) = state_from_text(self.pack.assets.registry, text) else {
            return Vec::new();
        };
        let mut drops = self.block_loot_rolled(pos, state, tool);
        drops.retain(|d| &*d.item != "minecraft:air" && d.count > 0);
        for d in &mut drops {
            self.pending.attach(pos, d);
        }
        drops
    }

    /// `BlockState.spawnAfterBreak(level, pos, tool, dropExperience)` of the vanilla block state
    /// `text` names, just broken at `pos` with `tool` by a mod's actor: an infested block lets its
    /// silverfish out (`InfestedBlock.spawnInfestation`), unless the tool has silk touch
    /// (`#prevents_infested_spawns`) or blocks don't drop. NeoForge's other blocks pay their
    /// experience from a player's own break (`getExpDrop`), not here. API @API:actors@.
    pub(super) fn native_spawn_after_break(&mut self, pos: [i32; 3], text: &str, tool: Option<&ItemStack>) {
        let Some(state) = state_from_text(self.pack.assets.registry, text) else {
            return;
        };
        let infested = self.res.facts().get(state).name.starts_with("minecraft:infested_");
        let silk = tool.is_some_and(|t| t.enchantment_level("minecraft:silk_touch") > 0);
        if infested && !silk && self.ui.rules.bool("block_drops") {
            let at = [f64::from(pos[0]) + 0.5, f64::from(pos[1]), f64::from(pos[2]) + 0.5];
            if let Some(id) = self.spawn_mob("minecraft:silverfish", at, false) {
                self.mob_poof(id);
            }
        }
    }

    /// NeoForge's `getExpDrop` of the block state `text` names: the experience its break drops,
    /// drawn from the level's random as the block draws it (an ore's range, a spawner's 15 + 0..14
    /// + 0..14), before enchantments (silk touch) take it away. 0 for a block that drops none or a
    /// state the game doesn't know. API @API:actors@.
    pub(super) fn native_block_experience(&mut self, text: &str) -> u32 {
        let Some(state) = state_from_text(self.pack.assets.registry, text) else {
            return 0;
        };
        let name = self.res.facts().get(state).name.clone();
        self.block_experience(&name)
    }

    /// `Block.popExperience(level, pos, amount)`: experience orbs worth `amount` at the centre of
    /// `pos`, when blocks drop (`doTileDrops`). API @API:actors@.
    pub(super) fn native_pop_experience(&mut self, pos: [i32; 3], amount: u32) {
        if amount > 0 && self.ui.rules.bool("block_drops") {
            self.spawn_orbs(pos.map(|c| f64::from(c) + 0.5), amount);
        }
    }

    /// Whether the block state `text` names is solid (`BlockState.isSolid`, vanilla's
    /// `legacySolid`); None for a state the game doesn't know. API @API:actors@.
    pub(super) fn native_state_solid(&self, text: &str) -> Option<bool> {
        let state = state_from_text(self.pack.assets.registry, text)?;
        Some(self.res.facts().get(state).legacy_solid)
    }

    /// `ServerLevel.destroyBlockProgress(breaker, pos, progress)`: entity `breaker`'s cracks on
    /// the block at `pos`, stage 0 to 9 (any other value takes them away), shown here and sent to
    /// the players within 32 blocks. API @API:actors@.
    pub(super) fn native_destroy_block_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {
        self.destroy_block_progress(breaker, pos, progress);
        if let Some(host) = self.lan.as_ref() {
            let at = pos.map(f64::from);
            let packet = horizon_protocol::packets::play::Clientbound::BlockDestruction(horizon_protocol::packets::play::BlockDestruction {
                id: breaker,
                pos,
                progress: if (0..10).contains(&progress) { progress as u8 } else { 255 },
            });
            host.hub.broadcast_near(at, 32.0, &packet, None);
        }
    }
}
