//! What translated code needs from the game it runs in. Horizon's native
//! mod glue implements [`Host`] over `GameLevel`'s helpers; tests use
//! [`MemoryHost`]. Block states cross as `namespace:block[prop=value,...]`
//! strings' interned ids, so the runtime never depends on Horizon's tables.
use std::collections::HashMap;

/// A model a block entity draws (`BakedModel`): a block state's, or one named by its location (a
/// mod's part model, `create:block/shaft_half`, never placed as a block).
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    State(String),
    Named(String),
}

pub trait Host {
    /// The block state at a position, as `minecraft:stone` or `create:shaft[axis=y]`.
    fn block_state(&self, pos: [i32; 3]) -> String;
    /// Sets a block state; false when the game refused it (unknown block, unloaded).
    fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool;
    /// Whether the world is the authoritative server (`Level.isClientSide()` is the negation).
    fn is_server(&self) -> bool {
        true
    }
    /// The game time in ticks.
    fn game_time(&self) -> i64 {
        0
    }
    /// The acting player (the one whose click or action runs the mod): sneaking.
    fn player_sneaking(&self) -> bool {
        false
    }
    /// The acting player is in creative mode.
    fn player_creative(&self) -> bool {
        false
    }
    /// The acting player's main-hand stack: item id and count.
    fn player_held(&self) -> Option<(String, u32)> {
        None
    }
    /// The collision boxes of the block at `pos` as the game has them (`BlockState.getCollisionShape`
    /// with the empty context), each relative to the block's corner; None when the game can't say.
    fn collision_boxes(&self, _pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {
        None
    }
    /// Takes up to `count` of `item` from the acting player, the main hand first; what it took
    /// (`Player.setItemInHand` replacing what the hand holds).
    fn take_player_items(&mut self, _item: &str, _count: u32) -> u32 {
        0
    }
    /// Gives the acting player up to `count` of `item` where they fit, as `Inventory.add` places
    /// them (`placeItemBackInInventory`); what it gave.
    fn give_player_items(&mut self, _item: &str, _count: u32) -> u32 {
        0
    }
    /// Players are sent the block at `pos` and its block entity's update tag again
    /// (`ServerChunkCache.blockChanged`, `Level.sendBlockUpdated`).
    fn block_changed(&mut self, _pos: [i32; 3]) {}
    /// Drawing a block entity: the frame's partial tick, 0 to 1.
    fn partial_tick(&self) -> f32 {
        0.0
    }
    /// Drawing a block entity: adds `model` under `pose` (a column-major 4x4 matrix, in blocks from
    /// the drawn block's lower north-west corner), times `color` (0xAARRGGBB), lit by `light`
    /// (`LightTexture.pack`: block light in bits 4..8, sky light in bits 20..24). False when the game
    /// has no such model.
    fn draw_model(&mut self, _model: &Model, _pose: &[f32; 16], _color: u32, _light: u32) -> bool {
        false
    }
    /// Drawing a block entity: adds item `item` under `pose` (as [`Host::draw_model`]) as the game
    /// draws an item in display context `display` (`ItemDisplayContext`'s ordinal: 0 none, 6 gui,
    /// 7 ground, 8 fixed...), lit by `light` (`LightTexture.pack`, 0 for the block's own); `seed`
    /// picks among random models. False when the game has no such item.
    fn draw_item(&mut self, _item: &str, _display: u8, _pose: &[f32; 16], _light: u32, _seed: i32) -> bool {
        false
    }
    /// Whether `item` places a block, so its model is a block's (`BakedModel.isGui3d`).
    fn item_is_block(&self, _item: &str) -> bool {
        false
    }
    /// How many of `item` one stack holds (its `max_stack_size`); None when the game doesn't know it.
    fn item_stack_limit(&self, _item: &str) -> Option<u32> {
        None
    }
    /// The recipes of a type outside `minecraft` (`create:pressing`): each one's id and its file as
    /// JSON text, the world's data packs merged and load conditions held. Empty outside a world.
    fn recipes_of_type(&self, _kind: &str) -> Vec<(String, String)> {
        Vec::new()
    }
    /// The items an ingredient entry names, in order: an item id (none when the game doesn't know
    /// it) or `#tag` (the tag's items, none for a tag the game doesn't have).
    fn ingredient_items(&self, _entry: &str) -> Vec<String> {
        Vec::new()
    }
}

/// A world in a map, for tests and tools. Unset positions are air.
#[derive(Default)]
pub struct MemoryHost {
    pub blocks: HashMap<[i32; 3], String>,
    pub time: i64,
    pub sneaking: bool,
    pub creative: bool,
    pub held: Option<(String, u32)>,
    /// What the acting player was given that the main hand didn't take, in order.
    pub given: Vec<(String, u32)>,
    /// Positions sent to players again (`block_changed`), in order.
    pub changed: Vec<[i32; 3]>,
    /// The partial tick drawing reads.
    pub partial: f32,
    /// What was drawn: model, pose, colour, light.
    pub drawn: Vec<(Model, [f32; 16], u32, u32)>,
    /// Recipe files by type: id and JSON text.
    pub recipes: HashMap<String, Vec<(String, String)>>,
    /// Item tags (no `#`) and their items.
    pub item_tags: HashMap<String, Vec<String>>,
    /// The items the game knows; when empty, every id is one.
    pub items: Vec<String>,
    /// Items drawn: item, display context, pose, light.
    pub items_drawn: Vec<(String, u8, [f32; 16], u32)>,
    /// Items that place blocks.
    pub block_items: Vec<String>,
    /// Stack limits other than 64.
    pub stack_limits: HashMap<String, u32>,
}

impl Host for MemoryHost {
    fn block_state(&self, pos: [i32; 3]) -> String {
        self.blocks.get(&pos).cloned().unwrap_or_else(|| "minecraft:air".into())
    }
    fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {
        self.blocks.insert(pos, state.to_owned());
        true
    }
    fn game_time(&self) -> i64 {
        self.time
    }
    fn player_sneaking(&self) -> bool {
        self.sneaking
    }
    fn player_creative(&self) -> bool {
        self.creative
    }
    fn player_held(&self) -> Option<(String, u32)> {
        self.held.clone()
    }
    /// Air (and unset positions) has none; any other block is a full cube.
    fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {
        let air = self.blocks.get(&pos).is_none_or(|b| b == "minecraft:air");
        Some(if air { Vec::new() } else { vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]] })
    }
    fn take_player_items(&mut self, item: &str, count: u32) -> u32 {
        let Some((held, n)) = &mut self.held else { return 0 };
        if held != item {
            return 0;
        }
        let took = count.min(*n);
        *n -= took;
        if *n == 0 {
            self.held = None;
        }
        took
    }
    fn give_player_items(&mut self, item: &str, count: u32) -> u32 {
        match &mut self.held {
            None => self.held = Some((item.to_owned(), count)),
            Some((held, n)) if held == item => *n += count,
            _ => self.given.push((item.to_owned(), count)),
        }
        count
    }
    fn block_changed(&mut self, pos: [i32; 3]) {
        self.changed.push(pos);
    }
    fn partial_tick(&self) -> f32 {
        self.partial
    }
    fn draw_model(&mut self, model: &Model, pose: &[f32; 16], color: u32, light: u32) -> bool {
        self.drawn.push((model.clone(), *pose, color, light));
        true
    }
    fn draw_item(&mut self, item: &str, display: u8, pose: &[f32; 16], light: u32, _seed: i32) -> bool {
        self.items_drawn.push((item.to_owned(), display, *pose, light));
        true
    }
    fn item_is_block(&self, item: &str) -> bool {
        self.block_items.iter().any(|b| b == item)
    }
    fn item_stack_limit(&self, item: &str) -> Option<u32> {
        Some(self.stack_limits.get(item).copied().unwrap_or(64))
    }
    fn recipes_of_type(&self, kind: &str) -> Vec<(String, String)> {
        self.recipes.get(kind).cloned().unwrap_or_default()
    }
    fn ingredient_items(&self, entry: &str) -> Vec<String> {
        match entry.strip_prefix('#') {
            Some(tag) => self.item_tags.get(tag).cloned().unwrap_or_default(),
            None if self.items.is_empty() || self.items.iter().any(|i| i == entry) => vec![entry.to_owned()],
            None => Vec::new(),
        }
    }
}

/// A block state string split into its block and properties.
pub fn split_state(state: &str) -> (&str, Vec<(&str, &str)>) {
    match state.split_once('[') {
        Some((block, rest)) => (
            block,
            rest.trim_end_matches(']')
                .split(',')
                .filter_map(|p| p.split_once('='))
                .collect(),
        ),
        None => (state, Vec::new()),
    }
}

/// Joins a block and properties back into a state string (properties sorted, as the game prints them).
pub fn join_state(block: &str, mut props: Vec<(&str, &str)>) -> String {
    if props.is_empty() {
        return block.to_owned();
    }
    props.sort();
    let inner: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{block}[{}]", inner.join(","))
}
