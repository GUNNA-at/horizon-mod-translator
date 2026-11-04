//! What translated code needs from the game it runs in. Horizon's native
//! mod glue implements [`Host`] over `GameLevel`'s helpers; tests use
//! [`MemoryHost`]. Block states cross as `namespace:block[prop=value,...]`
//! strings' interned ids, so the runtime never depends on Horizon's tables.
use std::collections::HashMap;

/// NBT as the glue hands it over: the game converts it to and from its own tags, the runtime to and
/// from its `CompoundTag`s (`horizon_compat::nbt`).
#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    List(Vec<Nbt>),
    Compound(std::collections::BTreeMap<String, Nbt>),
    ByteArray(Vec<i8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

/// A stack in one of the acting player's hands, as the game holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandStack {
    pub item: String,
    pub count: u32,
    /// Damage taken (`minecraft:damage`).
    pub damage: u32,
    /// The damage at which it breaks (`minecraft:max_damage`), 0 for an item without durability.
    pub max_damage: u32,
    /// Its mod components: each type's name (`create:belt_first_shaft`) with its value as NBT, as
    /// the type's codec writes it.
    pub components: Vec<(String, Nbt)>,
}

/// A model a block entity draws (`BakedModel`): a block state's, or one named by its location (a
/// mod's part model, `create:block/shaft_half`, never placed as a block), or faces a mod put
/// together itself (a `SimpleBakedModel` of `BakedQuad`s: Create's water wheel, its template's
/// faces retextured with the wheel's wood).
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    State(String),
    Named(String),
    Quads(std::sync::Arc<[Quad]>),
}

/// One face of a model (`BakedQuad`): as the game bakes it, or as a mod made or changed it.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    /// Corners, in blocks from the block's lower north-west corner.
    pub positions: [[f32; 3]; 4],
    /// Each corner's place in the sprite, 0 to 1 across it.
    pub uv: [[f32; 2]; 4],
    /// The sprite (`minecraft:block/oak_planks`).
    pub sprite: String,
    /// The side it faces, as `Direction`'s data value: 0 down, 1 up, 2 north, 3 south, 4 west, 5 east.
    pub facing: u8,
    /// The side whose neighbour hides it (the side `getQuads` lists it under), as `facing`.
    pub cull: Option<u8>,
    /// `getTintIndex`: -1 for an untinted face.
    pub tint_index: i32,
    /// `isShade`: the side's shade darkens it.
    pub shade: bool,
    /// Light the face gives itself, 0 to 15 (an emissive face's).
    pub light_emission: u8,
    /// Each corner's colour, 0xAARRGGBB: white unless the mod tinted it.
    pub colors: [u32; 4],
}

/// How [`Host::draw_model_shifted`] moves the faces of a model on one sprite to another sprite, as
/// catnip's `SuperByteBuffer.shiftUV*` do each vertex (Create's belts, its animated sheets).
#[derive(Clone, Debug, PartialEq)]
pub struct UvShift {
    /// The sprite whose faces move (`create:block/belt`).
    pub from: String,
    /// The sprite they take their texture from (`create:block/belt_scroll`).
    pub to: String,
    pub kind: UvShiftKind,
}

/// Where a moved face lands in the target sprite; `u` and `v` are fractions of the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UvShiftKind {
    /// `shiftUV`: the same place, each a fraction of its own sprite.
    Retarget,
    /// `shiftUVScrolling`: the same texel offset from the target's corner as from the source's,
    /// then `(u, v)` further.
    Scroll { u: f32, v: f32 },
    /// `shiftUVtoSheet`: the source shrunk to `1 / size` of the target, at `(u, v)`.
    Sheet { u: f32, v: f32, size: f32 },
}

/// Where [`Host::send_payload`] sends a mod's payload (NeoForge's `PacketDistributor`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PayloadTarget {
    /// From a client to its server (`sendToServer`).
    Server,
    /// To one player, by UUID (`sendToPlayer`).
    Player(u128),
    /// To every player on the server (`sendToAllPlayers`).
    All,
    /// To the players near a point of a dimension (`sendToPlayersNear`, and those tracking a chunk or
    /// an entity, which this treats as everyone within the radius): x, y, z, radius in blocks.
    Near { x: f64, y: f64, z: f64, radius: f64 },
}

/// An item entity of the server level (`ItemEntity`), as translated code reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemEntity {
    /// The entity's id, for [`Host::set_item_entity`].
    pub id: u32,
    /// Its feet: the bottom centre of its box (0.25 wide and tall).
    pub pos: [f64; 3],
    /// Blocks a tick (`getDeltaMovement`).
    pub motion: [f64; 3],
    pub on_ground: bool,
    pub item: String,
    pub count: u32,
    /// Ticks before a player may pick it up.
    pub pickup_delay: u32,
    /// Ticks lived (`ItemEntity.age`).
    pub age: u32,
}

impl ItemEntity {
    /// Its box (`getBoundingBox`): min x, y, z, then max.
    pub fn bounds(&self) -> [f64; 6] {
        let [x, y, z] = self.pos;
        [x - 0.125, y, z - 0.125, x + 0.125, y + 0.25, z + 0.125]
    }
}

/// `AABB.intersects`: boxes (min x, y, z, max x, y, z) overlap, touching faces excluded.
pub fn boxes_meet(a: &[f64; 6], b: &[f64; 6]) -> bool {
    (0..3).all(|i| a[i] < b[i + 3] && b[i] < a[i + 3])
}

pub trait Host {
    /// The block state at a position, as `minecraft:stone` or `create:shaft[axis=y]`.
    fn block_state(&self, pos: [i32; 3]) -> String;
    /// Sets a block state; false when the game refused it (unknown block, unloaded).
    fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool;
    /// The dimension the hook's level is (`minecraft:overworld`): a mod entity ticks in its own.
    fn dimension(&self) -> String {
        "minecraft:overworld".into()
    }
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
    /// The stack in the acting player's `hand` (0 main, 1 off: `InteractionHand`'s ordinal), None
    /// when empty. A host that knows only the main hand's item answers that.
    fn player_hand(&self, hand: u8) -> Option<HandStack> {
        if hand != 0 {
            return None;
        }
        self.player_held().map(|(item, count)| HandStack { item, count, ..Default::default() })
    }
    /// Changes the stack in `hand` in place: `count` of it (0 empties the hand), `damage`, and mod
    /// components set (Some) or removed (None) by type name; vanilla's other data on it stays. False
    /// when the game refused a change (an unregistered type, a value too big to send).
    fn update_player_hand(&mut self, _hand: u8, _count: u32, _damage: u32, _components: &[(String, Option<Nbt>)]) -> bool {
        false
    }
    /// Puts `stack` in `hand` (`Player.setItemInHand`), its `max_damage` aside; None empties it.
    fn set_player_hand(&mut self, _hand: u8, _stack: Option<&HandStack>) {}
    /// Wears the stack in `hand` by `amount` by the game's rules (`ItemStack.hurtAndBreak`: none in
    /// creative, Unbreaking, one worn out breaks with its sound). False when the host can't: the
    /// runtime wears it itself.
    fn hurt_player_hand(&mut self, _hand: u8, _amount: u32) -> bool {
        false
    }
    /// What the acting player wears in armor slot `slot` (0 head, 1 chest, 2 legs, 3 feet:
    /// `Player.getItemBySlot`), None when nothing.
    fn player_worn(&self, _slot: u8) -> Option<HandStack> {
        None
    }
    /// Where the acting player stands (their feet), and their look (yaw, pitch in degrees); None
    /// when the game can't say.
    fn player_place(&self) -> Option<([f64; 3], f32, f32)> {
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
    /// The acting player's food level, 0 to 20 (`FoodData.getFoodLevel`).
    fn player_food(&self) -> i32 {
        20
    }
    /// Tires the acting player by `amount` (`Player.causeFoodExhaustion`); the game keeps it off in
    /// creative.
    fn exhaust_player(&mut self, _amount: f32) {}
    /// Gives the acting player up to `count` of `item` where they fit, as `Inventory.add` places
    /// them (`placeItemBackInInventory`); what it gave.
    fn give_player_items(&mut self, _item: &str, _count: u32) -> u32 {
        0
    }
    /// Players are sent the block at `pos` and its block entity's update tag again
    /// (`ServerChunkCache.blockChanged`, `Level.sendBlockUpdated`).
    fn block_changed(&mut self, _pos: [i32; 3]) {}
    /// A message for the acting player (`Player.displayClientMessage`, `sendSystemMessage`):
    /// `component` is vanilla's JSON text, shown over the hotbar when `overlay`.
    fn player_message(&mut self, _component: &str, _overlay: bool) {}
    /// Spawns an experience orb worth `value` points at `at` (`new ExperienceOrb` added with
    /// `Level.addFreshEntity`), moving by `motion` when given, else with vanilla's random motion.
    /// False when none spawned.
    fn spawn_experience_orb(&mut self, _at: [f64; 3], _value: u32, _motion: Option<[f64; 3]>) -> bool {
        false
    }
    /// The block the acting player looks at within reach, as `Item.getPlayerPOVHitResult` finds it
    /// from their eyes: its position, the face hit (a direction ordinal: down, up, north, south,
    /// west, east) and the point hit. `fluids`: 0 none, 1 source fluids, 2 any fluid.
    fn player_pov_hit(&self, _fluids: u8) -> Option<([i32; 3], u8, [f64; 3])> {
        None
    }
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
    /// The faces of `model` as the game bakes it, every side's (`BakedModel.getQuads`): a state's
    /// model picked with seed 42, as vanilla draws a block in an entity's place. None when the game
    /// has no such model.
    fn model_quads(&self, model: &Model) -> Option<std::sync::Arc<[Quad]>> {
        match model {
            Model::Quads(quads) => Some(quads.clone()),
            _ => None,
        }
    }
    /// The sprite `model` breaks into (`BakedModel.getParticleIcon`): its `particle` texture.
    fn model_particle(&self, _model: &Model) -> Option<String> {
        None
    }
    /// Drawing a block entity: as [`Host::draw_model`], with the faces on sprite `shift.from`
    /// textured from `shift.to`. A host that can't move textures draws the model as it is.
    fn draw_model_shifted(&mut self, model: &Model, pose: &[f32; 16], color: u32, light: u32, _shift: &UvShift) -> bool {
        self.draw_model(model, pose, color, light)
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
    /// What the game knows of block state `state` (`minecraft:oak_planks`, as
    /// [`Host::block_state`] gives them). None for a state the game doesn't know.
    fn block_facts(&self, _state: &str) -> Option<BlockFacts> {
        None
    }
    /// Whether block `block` (`minecraft:oak_leaves`) is in block tag `tag` (`minecraft:leaves`, no
    /// `#`), as the game's loaded tags hold it (`BlockState.is(TagKey)`). False for a tag the game
    /// doesn't have.
    fn block_in_tag(&self, _block: &str, _tag: &str) -> bool {
        false
    }
    /// Whether fluid `fluid` (`minecraft:flowing_water`) is in fluid tag `tag`
    /// (`create:fan_processing_catalysts/splashing`, no `#`), as the game's loaded tags hold it
    /// (`FluidState.is(TagKey)`). False for a tag the game doesn't have.
    fn fluid_in_tag(&self, _fluid: &str, _tag: &str) -> bool {
        false
    }
    /// The way the fluid at `pos` flows (`FluidState.getFlow(level, pos)`, vanilla's
    /// `FlowingFluid.getFlow`): x, y, z, zero for still fluid or none.
    fn fluid_flow(&self, _pos: [i32; 3]) -> [f64; 3] {
        [0.0; 3]
    }
    /// The server's players: UUID and where each is (in the hook's dimension or not). None on a
    /// client.
    fn players(&self) -> Vec<(u128, [f64; 3])> {
        Vec::new()
    }
    /// Sends a mod's payload on the game's mod message channel: `channel` is the payload type's id
    /// (`create:server_speed`), `data` what its `StreamCodec` wrote. False when it can't go (no
    /// such player, a server that carries no mod messages, past the game's caps).
    fn send_payload(&mut self, _to: PayloadTarget, _channel: &str, _data: &[u8]) -> bool {
        false
    }
    /// The server level's item entities whose box meets `bounds` (min x, y, z, max x, y, z), in
    /// the level's order (`Level.getEntitiesOfClass(ItemEntity.class, box)`). None on a client.
    fn item_entities(&self, _bounds: [f64; 6]) -> Vec<ItemEntity> {
        Vec::new()
    }
    /// Sets item entity `id`'s stack (`ItemEntity.setItem`); a count of 0 removes the entity
    /// (`discard`). False when the level has no such item entity.
    fn set_item_entity(&mut self, _id: u32, _item: &str, _count: u32) -> bool {
        false
    }
    /// Moves item entity `id` (`setPos`) and sets its motion (`setDeltaMovement`); `None` keeps
    /// either. False when the level has no such item entity.
    fn move_item_entity(&mut self, _id: u32, _pos: Option<[f64; 3]>, _motion: Option<[f64; 3]>) -> bool {
        false
    }
    /// Adds an item entity to the server level (`addFreshEntity(new ItemEntity(...))`): its id,
    /// or none when the level takes none (a client).
    fn spawn_item_entity(&mut self, _item: &str, _count: u32, _pos: [f64; 3], _motion: [f64; 3], _pickup_delay: u32) -> Option<u32> {
        None
    }
    /// A sound a mod plays (`Level.playSound`, `playLocalSound`, `Entity.playSound`). A server's
    /// game sends it to the players near it; a client's plays it for its player.
    fn play_sound(&mut self, _sound: &Sound) {}
    /// Particles a mod makes: a client's game draws them (`Level.addParticle`), a server's sends
    /// them to the players near them (`ServerLevel.sendParticles`).
    fn particles(&mut self, _particles: &Particles) {}

    // ---- A client's GUI: what a mod's `GuiGraphics` draws in a frame (a HUD layer, a screen) ----

    /// The GUI's size in GUI pixels (`GuiGraphics.guiWidth`/`guiHeight`, the window's scaled
    /// size); zero where nothing is drawn.
    fn gui_size(&self) -> [i32; 2] {
        [0, 0]
    }
    /// GUI pixels per window pixel (`Window.getGuiScale`).
    fn gui_scale(&self) -> f64 {
        1.0
    }
    /// A quad of GUI pixels (its corners in drawing order) filled with `argb`.
    fn gui_fill(&mut self, _quad: [[f32; 2]; 4], _argb: u32) {}
    /// Part `uv` (`[u0, v0, u1, v1]`, 0 to 1 across it) of texture `texture`
    /// (`create:textures/gui/value_settings.png`) drawn into `rect` (`[x0, y0, x1, y1]` in GUI
    /// pixels), its colours times `tint` (`0xAARRGGBB`, white for none: `RenderSystem.setShaderColor`).
    /// False when the game has no such texture.
    fn gui_blit(&mut self, _texture: &str, _uv: [f32; 4], _rect: [f32; 4], _tint: u32) -> bool {
        false
    }
    /// A line of text at `at` (its top left), `scale` times its size: `runs` are its pieces in
    /// order, each with its own colour (`0xRRGGBB`, None for `argb`'s) and boldness, as a text
    /// component's are once translated. With vanilla's drop shadow or not.
    fn gui_text(&mut self, _runs: &[TextRun], _at: [f32; 2], _scale: f32, _argb: u32, _shadow: bool) {}
    /// How far character `c` advances in GUI pixels (the font's glyph advance; a bold one is one
    /// more).
    fn gui_char_width(&self, _c: char, _bold: bool) -> f32 {
        6.0
    }
    /// The icon of item `item` at `at` (its top left), `size` GUI pixels wide (16 is vanilla's).
    /// False when the game has no such item.
    fn gui_item(&mut self, _item: &str, _at: [f32; 2], _size: f32) -> bool {
        false
    }
    /// The game's text for translation key `key` in the player's language (`I18n.get`), None
    /// when it has none.
    fn translate(&self, _key: &str) -> Option<String> {
        None
    }

    // ---- A client's screen and input: a mod's own screen over the world, the keys it reads ----

    /// Opens the mods' screen over the world (`Minecraft.setScreen` with a mod's screen): the game
    /// frees the pointer and hands the screen's drawing and input to the runtime. False when it
    /// can't now (another screen is open, the world is loading, a server's game).
    fn open_screen(&mut self) -> bool {
        false
    }
    /// Closes the mods' screen, if it is open.
    fn close_screen(&mut self) {}
    /// Whether the mods' screen is open: the player closes it with Escape without the mod.
    fn screen_open(&self) -> bool {
        false
    }
    /// Vanilla key mapping `name` (`key.use`, `key.inventory`): the key bound to it as
    /// `options.txt` names keys (`key.mouse.right`, `key.keyboard.e`), and whether it is held.
    /// None for a mapping the game doesn't have.
    fn mapping(&self, _name: &str) -> Option<(String, bool)> {
        None
    }
    /// Whether key `key` (`key.keyboard.left.shift`, `key.mouse.left`) is held now
    /// (`InputConstants.isKeyDown`).
    fn key_held(&self, _key: &str) -> bool {
        false
    }
    /// The pointer, in GUI pixels from the window's top left.
    fn cursor(&self) -> [f32; 2] {
        [0.0, 0.0]
    }
    /// Moves the pointer to `at` in GUI pixels (`GLFW.glfwSetCursorPos`).
    fn set_cursor(&mut self, _at: [f32; 2]) {}
    /// True for a host lent only to read and draw (a screen's drawing and input, a HUD layer):
    /// what would change the game (a payload, a screen opening or closing, the pointer moving)
    /// waits for the client's next tick.
    fn read_only(&self) -> bool {
        false
    }
}

/// A piece of a line of text with one style ([`Host::gui_text`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextRun {
    pub text: String,
    /// `0xRRGGBB`; None takes the colour the text is drawn with.
    pub color: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub strikethrough: bool,
}

/// What a mod's `GuiGraphics` drew on a [`MemoryHost`], in order.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiDraw {
    Fill([[f32; 2]; 4], u32),
    Blit(String, [f32; 4], [f32; 4], u32),
    Text(Vec<TextRun>, [f32; 2], f32, u32, bool),
    Item(String, [f32; 2], f32),
}

/// Particles a mod makes ([`Host::particles`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Particles {
    /// The type's id: `minecraft:smoke`, `minecraft:block`, a mod's own `create:air_flow`.
    pub kind: String,
    /// `BlockParticleOption`'s block state, as text (`minecraft:stone`).
    pub block: Option<String>,
    /// `ItemParticleOption`'s item id.
    pub item: Option<String>,
    /// `DustParticleOptions`' colour and scale.
    pub dust: Option<([f32; 3], f32)>,
    pub at: [f64; 3],
    /// One particle's velocity (`addParticle`), or how far `count` of them spread
    /// (`sendParticles`; with a count of 0, one moves along it at `speed`).
    pub delta: [f64; 3],
    pub speed: f64,
    /// `sendParticles`' count; None for `addParticle`.
    pub count: Option<i32>,
    /// Drawn however far away (`addParticle(options, true, ...)`, `sendParticles(player, options, true, ...)`).
    pub force: bool,
    /// `addAlwaysVisibleParticle`: drawn at the lowest particle setting too.
    pub always: bool,
}

/// A sound a mod plays ([`Host::play_sound`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    /// The sound event's id: `create:contraption_assemble`, `minecraft:block.wooden_trapdoor.open`.
    pub event: String,
    pub at: [f64; 3],
    /// Its `SoundSource`'s ordinal, the volume slider it plays under: master 0, music, record,
    /// weather, block 4, hostile, neutral 6, player 7, ambient, voice 9 (1.21.1's order, which
    /// 26.3's `SoundSource` keeps).
    pub source: usize,
    pub volume: f32,
    pub pitch: f32,
}

/// What vanilla's block state accessors read from a block's properties: what the game knows of a
/// state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockFacts {
    /// Destroy time (`getDestroySpeed`), -1 for a block nothing breaks.
    pub hardness: f32,
    /// How pistons move it (`getPistonPushReaction`), as `PushReaction`'s ordinal: 0 normal,
    /// 1 destroy, 2 block, 3 ignore, 4 push only.
    pub push: u8,
    /// Whether placing a block where it is replaces it (`canBeReplaced()`).
    pub replaceable: bool,
}

/// A world in a map, for tests and tools. Unset positions are air.
#[derive(Default)]
pub struct MemoryHost {
    pub blocks: HashMap<[i32; 3], String>,
    pub time: i64,
    pub sneaking: bool,
    pub creative: bool,
    pub held: Option<(String, u32)>,
    /// The mod components of the main-hand stack, by type name.
    pub held_components: std::collections::BTreeMap<String, Nbt>,
    /// The main-hand stack's damage.
    pub held_damage: u32,
    /// The off hand's stack.
    pub offhand: Option<HandStack>,
    /// What the acting player wears: head, chest, legs, feet.
    pub worn: [Option<HandStack>; 4],
    /// Items' durability (`max_damage`); unset ones have none.
    pub max_damage: HashMap<String, u32>,
    /// Where the acting player stands and looks (yaw, pitch).
    pub place: Option<([f64; 3], f32, f32)>,
    /// What the acting player was given that the main hand didn't take, in order.
    pub given: Vec<(String, u32)>,
    /// Positions sent to players again (`block_changed`), in order.
    pub changed: Vec<[i32; 3]>,
    /// Messages the acting player was sent (`player_message`): JSON text, and whether over the hotbar.
    pub messages: Vec<(String, bool)>,
    /// Experience orbs spawned: where, worth, and the motion given.
    pub orbs: Vec<([f64; 3], u32, Option<[f64; 3]>)>,
    /// What the acting player looks at (`player_pov_hit`).
    pub pov: Option<([i32; 3], u8, [f64; 3])>,
    /// The partial tick drawing reads.
    pub partial: f32,
    /// What was drawn: model, pose, colour, light.
    pub drawn: Vec<(Model, [f32; 16], u32, u32)>,
    /// The texture moves drawn models were given: the index in `drawn`, and the move.
    pub shifts: Vec<(usize, UvShift)>,
    /// Models' faces ([`Host::model_quads`]) by state text or location, and their particle sprites.
    pub models: HashMap<String, (Vec<Quad>, String)>,
    /// Recipe files by type: id and JSON text.
    pub recipes: HashMap<String, Vec<(String, String)>>,
    /// Item tags (no `#`) and their items.
    pub item_tags: HashMap<String, Vec<String>>,
    /// Block tags (no `#`) and their blocks.
    pub block_tags: HashMap<String, Vec<String>>,
    /// Fluid tags (no `#`) and their fluids.
    pub fluid_tags: HashMap<String, Vec<String>>,
    /// Fluid currents by position (`fluid_flow`); unset positions have none.
    pub flows: HashMap<[i32; 3], [f64; 3]>,
    /// The exhaustion the acting player was given (`exhaust_player`), in all.
    pub exhaustion: f32,
    /// Mod payloads sent, in order: where to, channel, bytes.
    pub payloads: Vec<(PayloadTarget, String, Vec<u8>)>,
    /// The items the game knows; when empty, every id is one.
    pub items: Vec<String>,
    /// Items drawn: item, display context, pose, light.
    pub items_drawn: Vec<(String, u8, [f32; 16], u32)>,
    /// Items that place blocks.
    pub block_items: Vec<String>,
    /// Stack limits other than 64.
    pub stack_limits: HashMap<String, u32>,
    /// The level's item entities, in order.
    pub item_entities: Vec<ItemEntity>,
    /// Block facts by block (`minecraft:oak_planks`, any of its states).
    pub facts: HashMap<String, BlockFacts>,
    /// The server's players and where they are.
    pub players: Vec<(u128, [f64; 3])>,
    /// A client's world: its runtime's hooks are a client's (`is_server` false).
    pub client: bool,
    /// Sounds played, in order.
    pub sounds: Vec<Sound>,
    /// Particles made, in order.
    pub particles: Vec<Particles>,
    /// The GUI's size while a mod draws on it ([`Host::gui_size`]), and what it drew, in order.
    pub gui: [i32; 2],
    pub drawn_gui: Vec<GuiDraw>,
    /// The player's language: translation key to text.
    pub lang: HashMap<String, String>,
    /// Whether the mods' screen is open, and how often it was opened.
    pub screen: bool,
    pub screens_opened: u32,
    /// Key mappings: the key bound and whether it is held.
    pub mappings: HashMap<String, (String, bool)>,
    /// Keys held, by name.
    pub keys_held: Vec<String>,
    /// The pointer in GUI pixels.
    pub cursor: [f32; 2],
    /// A host lent only to read and draw ([`Host::read_only`]).
    pub read_only: bool,
}

impl Host for MemoryHost {
    fn block_state(&self, pos: [i32; 3]) -> String {
        self.blocks.get(&pos).cloned().unwrap_or_else(|| "minecraft:air".into())
    }
    fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {
        self.blocks.insert(pos, state.to_owned());
        true
    }
    fn is_server(&self) -> bool {
        !self.client
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
    fn player_hand(&self, hand: u8) -> Option<HandStack> {
        if hand == 1 {
            return self.offhand.clone();
        }
        let (item, count) = self.held.clone()?;
        let max_damage = self.max_damage.get(&item).copied().unwrap_or(0);
        let components = self.held_components.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        Some(HandStack { item, count, damage: self.held_damage, max_damage, components })
    }
    fn update_player_hand(&mut self, hand: u8, count: u32, damage: u32, components: &[(String, Option<Nbt>)]) -> bool {
        let Some(mut s) = self.player_hand(hand) else { return false };
        s.count = count;
        s.damage = damage;
        for (name, value) in components {
            s.components.retain(|(k, _)| k != name);
            if let Some(v) = value {
                s.components.push((name.clone(), v.clone()));
            }
        }
        self.set_player_hand(hand, Some(&s));
        true
    }
    /// As the game wears a stack in hand: none in creative or for an item without durability; one
    /// worn out breaks.
    fn hurt_player_hand(&mut self, hand: u8, amount: u32) -> bool {
        let Some(mut s) = self.player_hand(hand) else { return true };
        if self.creative || s.max_damage == 0 {
            return true;
        }
        s.damage += amount;
        let worn = s.damage >= s.max_damage;
        self.set_player_hand(hand, (!worn).then_some(&s));
        true
    }
    fn set_player_hand(&mut self, hand: u8, stack: Option<&HandStack>) {
        let stack = stack.filter(|s| !s.item.is_empty() && s.count > 0);
        if hand == 1 {
            self.offhand = stack.cloned();
            return;
        }
        self.held = stack.map(|s| (s.item.clone(), s.count));
        self.held_damage = stack.map_or(0, |s| s.damage);
        self.held_components = stack.map(|s| s.components.iter().cloned().collect()).unwrap_or_default();
    }
    fn player_place(&self) -> Option<([f64; 3], f32, f32)> {
        self.place
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
            self.held_components.clear();
            self.held_damage = 0;
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
    fn player_message(&mut self, component: &str, overlay: bool) {
        self.messages.push((component.to_owned(), overlay));
    }
    fn spawn_experience_orb(&mut self, at: [f64; 3], value: u32, motion: Option<[f64; 3]>) -> bool {
        self.orbs.push((at, value, motion));
        true
    }
    fn player_pov_hit(&self, _fluids: u8) -> Option<([i32; 3], u8, [f64; 3])> {
        self.pov
    }
    fn player_worn(&self, slot: u8) -> Option<HandStack> {
        self.worn.get(usize::from(slot)).cloned().flatten()
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
    fn model_quads(&self, model: &Model) -> Option<std::sync::Arc<[Quad]>> {
        match model {
            Model::State(key) | Model::Named(key) => self.models.get(key).map(|(q, _)| q.as_slice().into()),
            Model::Quads(quads) => Some(quads.clone()),
        }
    }
    fn model_particle(&self, model: &Model) -> Option<String> {
        match model {
            Model::State(key) | Model::Named(key) => self.models.get(key).map(|(_, p)| p.clone()),
            Model::Quads(_) => None,
        }
    }
    fn draw_model_shifted(&mut self, model: &Model, pose: &[f32; 16], color: u32, light: u32, shift: &UvShift) -> bool {
        self.shifts.push((self.drawn.len(), shift.clone()));
        self.draw_model(model, pose, color, light)
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
    fn block_facts(&self, state: &str) -> Option<BlockFacts> {
        self.facts.get(state.split('[').next().unwrap_or(state)).copied()
    }
    fn block_in_tag(&self, block: &str, tag: &str) -> bool {
        self.block_tags.get(tag).is_some_and(|b| b.iter().any(|x| x == block))
    }
    fn fluid_in_tag(&self, fluid: &str, tag: &str) -> bool {
        self.fluid_tags.get(tag).is_some_and(|f| f.iter().any(|x| x == fluid))
    }
    fn fluid_flow(&self, pos: [i32; 3]) -> [f64; 3] {
        self.flows.get(&pos).copied().unwrap_or_default()
    }
    fn exhaust_player(&mut self, amount: f32) {
        self.exhaustion += amount;
    }
    fn send_payload(&mut self, to: PayloadTarget, channel: &str, data: &[u8]) -> bool {
        if self.read_only {
            return false;
        }
        self.payloads.push((to, channel.to_owned(), data.to_vec()));
        true
    }
    fn players(&self) -> Vec<(u128, [f64; 3])> {
        self.players.clone()
    }
    fn item_entities(&self, bounds: [f64; 6]) -> Vec<ItemEntity> {
        self.item_entities.iter().filter(|e| e.count > 0 && boxes_meet(&e.bounds(), &bounds)).cloned().collect()
    }
    fn set_item_entity(&mut self, id: u32, item: &str, count: u32) -> bool {
        let Some(i) = self.item_entities.iter().position(|e| e.id == id) else { return false };
        if count == 0 {
            self.item_entities.remove(i);
        } else {
            let e = &mut self.item_entities[i];
            e.item = item.to_owned();
            e.count = count;
        }
        true
    }
    fn move_item_entity(&mut self, id: u32, pos: Option<[f64; 3]>, motion: Option<[f64; 3]>) -> bool {
        let Some(e) = self.item_entities.iter_mut().find(|e| e.id == id) else { return false };
        if let Some(p) = pos {
            e.pos = p;
        }
        if let Some(m) = motion {
            e.motion = m;
        }
        true
    }
    fn spawn_item_entity(&mut self, item: &str, count: u32, pos: [f64; 3], motion: [f64; 3], pickup_delay: u32) -> Option<u32> {
        let id = self.item_entities.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        self.item_entities.push(ItemEntity { id, pos, motion, on_ground: false, item: item.to_owned(), count, pickup_delay, age: 0 });
        Some(id)
    }
    fn play_sound(&mut self, sound: &Sound) {
        if !self.read_only {
            self.sounds.push(sound.clone());
        }
    }
    fn particles(&mut self, particles: &Particles) {
        self.particles.push(particles.clone());
    }
    fn gui_size(&self) -> [i32; 2] {
        self.gui
    }
    fn gui_fill(&mut self, quad: [[f32; 2]; 4], argb: u32) {
        self.drawn_gui.push(GuiDraw::Fill(quad, argb));
    }
    fn gui_blit(&mut self, texture: &str, uv: [f32; 4], rect: [f32; 4], tint: u32) -> bool {
        self.drawn_gui.push(GuiDraw::Blit(texture.to_owned(), uv, rect, tint));
        true
    }
    fn gui_text(&mut self, runs: &[TextRun], at: [f32; 2], scale: f32, argb: u32, shadow: bool) {
        self.drawn_gui.push(GuiDraw::Text(runs.to_vec(), at, scale, argb, shadow));
    }
    fn gui_item(&mut self, item: &str, at: [f32; 2], size: f32) -> bool {
        self.drawn_gui.push(GuiDraw::Item(item.to_owned(), at, size));
        true
    }
    fn translate(&self, key: &str) -> Option<String> {
        self.lang.get(key).cloned()
    }
    fn open_screen(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        self.screen = true;
        self.screens_opened += 1;
        true
    }
    fn close_screen(&mut self) {
        if !self.read_only {
            self.screen = false;
        }
    }
    fn screen_open(&self) -> bool {
        self.screen
    }
    fn mapping(&self, name: &str) -> Option<(String, bool)> {
        self.mappings.get(name).cloned()
    }
    fn key_held(&self, key: &str) -> bool {
        self.keys_held.iter().any(|k| k == key)
    }
    fn cursor(&self) -> [f32; 2] {
        self.cursor
    }
    fn set_cursor(&mut self, at: [f32; 2]) {
        if !self.read_only {
            self.cursor = at;
        }
    }
    fn read_only(&self) -> bool {
        self.read_only
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
