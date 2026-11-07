//! A level read through and never written: what a block's own `updateShape` sees when the level's
//! hook asks for it from inside `before_update_shape` (a mod block's `super.updateShape`), where the
//! level is held as `&self`.
use super::*;

/// `base`, read as it is: block changes, events, block entity loads and the other writes go
/// nowhere, and the random is a stream of its own (seeded from the game time).
pub(crate) struct ReadLevel<'a, L: LevelAccess + ?Sized> {
    base: &'a L,
    random: LegacyRandom,
}

impl<'a, L: LevelAccess + ?Sized> ReadLevel<'a, L> {
    pub(crate) fn new(base: &'a L) -> Self {
        Self {
            random: LegacyRandom::new(base.game_time()),
            base,
        }
    }
}

/// Every read is the base's; the writes (`&mut self`) keep the trait's defaults, which change
/// nothing. A change observed (`observe_*`) never happens here, so the base hears of none.
impl<L: LevelAccess + ?Sized> LevelAccess for ReadLevel<'_, L> {
    fn allow_spread(&self, kind: Spread, from: BlockPos, to: BlockPos) -> bool {
        self.base.allow_spread(kind, from, to)
    }
    fn allow_piston_move(
        &self,
        piston: BlockPos,
        direction: Direction,
        push: &[BlockPos],
        destroy: &[BlockPos],
    ) -> bool {
        self.base
            .allow_piston_move(piston, direction, push, destroy)
    }
    fn is_valid_bonemeal_target(&self, pos: BlockPos, state: BlockStateId) -> Option<bool> {
        self.base.is_valid_bonemeal_target(pos, state)
    }
    #[allow(clippy::too_many_arguments)]
    fn before_update_shape(
        &self,
        ticks: &mut Ticks,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        neighbour_pos: BlockPos,
        neighbour: BlockStateId,
        new: &mut Option<BlockStateId>,
    ) {
        self.base
            .before_update_shape(ticks, pos, state, direction, neighbour_pos, neighbour, new)
    }
    #[allow(clippy::option_option)]
    fn before_state_for_placement(
        &self,
        state: BlockStateId,
        context: &PlacementContext,
        fluid: FluidState,
        answer: &mut Option<Option<BlockStateId>>,
    ) {
        self.base
            .before_state_for_placement(state, context, fluid, answer)
    }
    fn before_can_survive(&self, pos: BlockPos, state: BlockStateId, survives: &mut Option<bool>) {
        self.base.before_can_survive(pos, state, survives)
    }
    fn before_may_place_on(
        &self,
        state: BlockStateId,
        ground: BlockStateId,
        ground_pos: BlockPos,
        may: &mut Option<bool>,
    ) {
        self.base
            .before_may_place_on(state, ground, ground_pos, may)
    }
    fn is_client_side(&self) -> bool {
        self.base.is_client_side()
    }
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.base.block_state(pos)
    }
    fn set_raw(&mut self, _pos: BlockPos, _state: BlockStateId) -> Option<BlockStateId> {
        None
    }
    fn is_loaded(&self, pos: BlockPos) -> bool {
        self.base.is_loaded(pos)
    }
    fn min_y(&self) -> i32 {
        self.base.min_y()
    }
    fn max_y(&self) -> i32 {
        self.base.max_y()
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.base.world_surface(x, z)
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn game_time(&self) -> i64 {
        self.base.game_time()
    }
    fn block_light(&self, pos: BlockPos) -> i32 {
        self.base.block_light(pos)
    }
    fn sky_light(&self, pos: BlockPos) -> i32 {
        self.base.sky_light(pos)
    }
    fn sky_darken(&self) -> i32 {
        self.base.sky_darken()
    }
    fn sun_angle(&self, pos: BlockPos) -> f32 {
        self.base.sun_angle(pos)
    }
    fn is_raining(&self) -> bool {
        self.base.is_raining()
    }
    fn is_thundering(&self) -> bool {
        self.base.is_thundering()
    }
    fn is_raining_at(&self, pos: BlockPos) -> bool {
        self.base.is_raining_at(pos)
    }
    fn dimension(&self) -> Dimension {
        self.base.dimension()
    }
    fn game_rules(&self) -> GameRules {
        self.base.game_rules()
    }
    fn difficulty(&self) -> i32 {
        self.base.difficulty()
    }
    fn can_spread_fire_around(&self, pos: BlockPos) -> bool {
        self.base.can_spread_fire_around(pos)
    }
    fn increased_fire_burnout(&self, pos: BlockPos) -> bool {
        self.base.increased_fire_burnout(pos)
    }
    fn is_unobstructed(&self, pos: BlockPos, state: BlockStateId) -> bool {
        self.base.is_unobstructed(pos, state)
    }
    fn block_entity_analog_signal(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
    ) -> i32 {
        self.base.block_entity_analog_signal(pos, state, direction)
    }
    fn item_frame_analog_output(&self, pos: BlockPos, facing: Direction) -> Option<i32> {
        self.base.item_frame_analog_output(pos, facing)
    }
    fn adjust_signal_source(&self, state: BlockStateId, vanilla: bool) -> bool {
        self.base.adjust_signal_source(state, vanilla)
    }
    fn adjust_has_analog_output(&self, state: BlockStateId, vanilla: bool) -> bool {
        self.base.adjust_has_analog_output(state, vanilla)
    }
    fn adjust_analog_output(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        vanilla: i32,
    ) -> i32 {
        self.base
            .adjust_analog_output(pos, state, direction, vanilla)
    }
    fn adjust_signal(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        direct: bool,
        vanilla: i32,
    ) -> i32 {
        self.base
            .adjust_signal(pos, state, direction, direct, vanilla)
    }
    fn block_entity_signal(&self, pos: BlockPos, state: BlockStateId, direction: Direction) -> i32 {
        self.base.block_entity_signal(pos, state, direction)
    }
    fn precipitation(&self, pos: BlockPos) -> Option<Precipitation> {
        self.base.precipitation(pos)
    }
    fn count_entities(&self, area: Aabb, query: EntityQuery) -> usize {
        self.base.count_entities(area, query)
    }
    fn is_ticking_chunk(&self, chunk: ChunkPos) -> bool {
        self.base.is_ticking_chunk(chunk)
    }
    fn has_mod_block_entity(&self, pos: BlockPos) -> bool {
        self.base.has_mod_block_entity(pos)
    }
    fn biome_name(&self, pos: BlockPos) -> Option<&str> {
        self.base.biome_name(pos)
    }
    fn world_seed(&self) -> i64 {
        self.base.world_seed()
    }
    fn day_time(&self) -> i64 {
        self.base.day_time()
    }
}
