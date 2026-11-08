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

/// The off hand's slot in vanilla's `Inventory` numbering ([`Host::player_slot`]).
pub const OFF_HAND_SLOT: u8 = 40;

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

impl HandStack {
    /// Its enchantments (`minecraft:enchantments`, as the runtime carries it: `{levels: {id:
    /// level}}`): each one's id and level.
    pub fn enchantments(&self) -> Vec<(String, i32)> {
        let Some((_, Nbt::Compound(c))) = self.components.iter().find(|(n, _)| n == "minecraft:enchantments") else {
            return Vec::new();
        };
        let Some(Nbt::Compound(levels)) = c.get("levels") else {
            return Vec::new();
        };
        levels
            .iter()
            .filter_map(|(id, l)| match l {
                Nbt::Int(l) => Some((id.clone(), *l)),
                _ => None,
            })
            .collect()
    }
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

/// A neighbour update a mod's code asks the level for ([`Host::update_neighbors`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeighborUpdate {
    /// `Level.updateNeighborsAt(pos, block)` (and `blockUpdated`): the six blocks around `pos`, in
    /// vanilla's order, but the one in direction `skip` (`Direction`'s data value) when set
    /// (`updateNeighborsAtExceptFromFacing`).
    Around { pos: [i32; 3], skip: Option<u8> },
    /// `Level.neighborChanged(pos, block, fromPos)`: the block at `pos` hears the change at `from`.
    One { pos: [i32; 3], from: [i32; 3] },
    /// `Level.updateNeighbourForOutputSignal(pos, block)` (and a block entity's `setChanged`):
    /// comparators beside `pos`, or behind a conductor there, read it again.
    Output { pos: [i32; 3] },
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

/// The item handler NeoForge gives a vanilla block's container (`CapabilityHooks.registerVanillaProviders`),
/// by the rules it adds to a plain container's ([`Host::container`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContainerKind {
    /// An `InvWrapper` of the container: a chest's (both halves of a double one), a barrel's, a
    /// dispenser's, a dropper's, a crafter's, a chiseled bookshelf's or a jukebox's.
    #[default]
    Plain,
    /// A hopper's `VanillaHopperItemHandler`: filling the empty hopper sets its cooldown.
    Hopper,
    /// A `SidedInvWrapper` of a furnace, a blast furnace or a smoker (`AbstractFurnaceBlockEntity`):
    /// one bucket at a time into its fuel slot.
    Furnace,
    /// A `SidedInvWrapper` of a brewing stand: one bottle in each of its three bottle slots.
    Brewing,
    /// A `SidedInvWrapper` of a shulker box.
    Shulker,
}

/// A vanilla block's container (`Container`), as the game holds it ([`Host::container`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContainerFacts {
    /// Its slots (`getContainerSize`).
    pub size: u32,
    /// The most a slot holds (`getMaxStackSize()`): 99, 1 in a jukebox or a chiseled bookshelf.
    pub max_stack: u32,
    pub kind: ContainerKind,
}

/// A blow's source as [`Host::hurt_entity_by`] takes it (vanilla's `DamageSource`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HurtBy {
    /// Its damage type (`minecraft:thrown`, a mod's own `create:potato_cannon`).
    pub damage_type: String,
    /// The player it is credited to (`getEntity()`), by UUID.
    pub player: Option<u128>,
    /// Else the mob it is credited to (`getEntity()`), by entity id.
    pub mob: Option<i32>,
    /// Where it came from (`getSourcePosition()`): the knockback's direction.
    pub from: Option<[f64; 3]>,
    /// The horizontal movement of the projectile that dealt it (`getDirectEntity()`'s,
    /// `Projectile.calculateHorizontalHurtKnockbackDirection`): the knockback's direction instead.
    pub motion: Option<[f64; 2]>,
    /// The type of the entity that dealt it (`getDirectEntity()`) when that isn't the one it is
    /// credited to, a projectile (`create:potato_projectile`): the blow isn't direct (`isDirect`).
    pub direct: Option<String>,
}

/// Whom a blow struck ([`Host::post_attack_effects`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Struck {
    /// A player, by UUID.
    Player(u128),
    /// Another entity, by id.
    Entity(i32),
}

/// The weapon whose enchantments act after a blow ([`Host::post_attack_effects`]).
#[derive(Clone, Debug, PartialEq)]
pub enum AttackWeapon {
    /// None: only the victim's worn ones act (`doPostAttackEffectsWithItemSource` with no item).
    None,
    /// What the one the blow is credited to holds in their main hand (`getWeaponItem`).
    Held,
    /// This stack (`doPostAttackEffectsWithItemSource`'s item).
    Stack(HandStack),
}

/// An entity of the server level in place, as translated code reads it (`Level.getEntity(id)`):
/// a mob, a minecart, a boat, an armour stand, an item...
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityView {
    /// The entity's id (`Entity.getId`).
    pub id: i32,
    /// Its type (`minecraft:cow`).
    pub kind: String,
    /// Its feet: the bottom centre of its box.
    pub pos: [f64; 3],
    /// Where it faces, in degrees (`getYRot`): 0 is south.
    pub yaw: f32,
    /// Where it looks up or down, in degrees (`getXRot`): -90 is up.
    pub pitch: f32,
    /// A living mob's head and body facing, in degrees (`getYHeadRot`, `yBodyRot`); the rest's are
    /// their facing.
    pub head_yaw: f32,
    pub body_yaw: f32,
    /// A living mob's health and maximum health; none for the rest.
    pub health: Option<(f32, f32)>,
}

/// How translated code turns an entity: each angle in degrees, the ones left out kept.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turn {
    /// Where it faces (`Entity.setYRot`): 0 is south.
    pub yaw: Option<f32>,
    /// Where it looks up or down (`Entity.setXRot`): -90 is up.
    pub pitch: Option<f32>,
    /// A living mob's head (`LivingEntity.setYHeadRot`).
    pub head_yaw: Option<f32>,
    /// A living mob's body (`LivingEntity.setYBodyRot`).
    pub body_yaw: Option<f32>,
}

/// An attribute of a living entity, as translated code reads it through its `AttributeInstance`
/// (`getValue`, `getBaseValue`, `getModifiers`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeView {
    /// Its value: the base with the modifiers, kept in the attribute's range.
    pub value: f64,
    pub base: f64,
    /// Its modifiers, in the order they came.
    pub modifiers: Vec<AttributeModifierView>,
}

/// One of an attribute's modifiers (vanilla's `AttributeModifier`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeModifierView {
    /// Its id (`mymod:speed_boost`): an attribute has one modifier of each.
    pub id: String,
    pub amount: f64,
    /// `AttributeModifier.Operation`'s ordinal: 0 adds the amount to the base, 1 the base times the
    /// amount, 2 multiplies the total by 1 plus the amount.
    pub operation: u8,
    /// Kept with the entity's saved data (`addPermanentModifier`).
    pub permanent: bool,
}

/// A change translated code makes to an attribute ([`Host::change_attribute`]).
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeChange {
    /// Its base (`setBaseValue`).
    Base(f64),
    /// A modifier added (`addTransientModifier`, `addPermanentModifier`); with `replace`, one of its
    /// id already there is replaced (`addOrUpdateTransientModifier`, `addOrReplacePermanentModifier`),
    /// without, the change fails.
    Add { modifier: AttributeModifierView, replace: bool },
    /// The modifier of this id taken off (`removeModifier`); fails when there is none.
    Remove(String),
    /// All its modifiers taken off (`removeModifiers`).
    RemoveAll,
}

impl AttributeView {
    /// Makes `change` as vanilla's `AttributeInstance` makes it (a modifier of an id already there
    /// replaced only where the change says so), the value computed again before its range:
    /// whether it was made.
    pub fn apply(&mut self, change: &AttributeChange) -> bool {
        let made = match change {
            AttributeChange::Base(v) => {
                self.base = *v;
                true
            }
            AttributeChange::Add { modifier, replace } => match self.modifiers.iter().position(|m| m.id == modifier.id) {
                Some(_) if !replace => false,
                Some(i) => {
                    self.modifiers[i] = modifier.clone();
                    true
                }
                None => {
                    self.modifiers.push(modifier.clone());
                    true
                }
            },
            AttributeChange::Remove(id) => {
                let before = self.modifiers.len();
                self.modifiers.retain(|m| &m.id != id);
                self.modifiers.len() != before
            }
            AttributeChange::RemoveAll => {
                self.modifiers.clear();
                true
            }
        };
        self.value = Self::computed(self.base, &self.modifiers);
        made
    }

    /// Vanilla's `AttributeInstance.calculateValue` of `base` and `modifiers`, before its range:
    /// the additions, then each multiple of the base, then each multiple of the total.
    pub fn computed(base: f64, modifiers: &[AttributeModifierView]) -> f64 {
        let of = |op: u8| modifiers.iter().filter(move |m| m.operation == op).map(|m| m.amount);
        let mut added = base;
        for a in of(0) {
            added += a;
        }
        let mut value = added;
        for a in of(1) {
            value += added * a;
        }
        for a in of(2) {
            value *= 1.0 + a;
        }
        value
    }
}

/// The player in place's health and food, as translated code reads and sets them
/// (`LivingEntity.getHealth`, `getMaxHealth`, `getAbsorptionAmount`; `Player.getFoodData`'s level,
/// saturation and exhaustion).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VitalsView {
    pub health: f32,
    /// The `max_health` attribute's value; setting the vitals leaves it (the attribute sets it).
    pub max_health: f32,
    pub absorption: f32,
    /// The food level (20 is full), its saturation and exhaustion.
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
}

impl Default for VitalsView {
    /// A new player's: full health and food (`FoodData`'s level 20 and saturation 5).
    fn default() -> Self {
        Self { health: 20.0, max_health: 20.0, absorption: 0.0, food: 20, saturation: 5.0, exhaustion: 0.0 }
    }
}

/// One of a living entity's active effects (vanilla's `MobEffectInstance`), as translated code
/// reads it (`getEffect`, `getActiveEffects`) and gives it (`addEffect`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectView {
    /// Its effect: `minecraft:speed`, a mod's `<namespace>:<path>`.
    pub id: String,
    /// The ticks left; -1 for one that doesn't end.
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    /// Whether it shows its particles.
    pub visible: bool,
    pub show_icon: bool,
}

impl EffectView {
    /// Vanilla's `MobEffectInstance.update` of an effect already there (`self`) by one of the same
    /// effect given again (`by`), without the weaker one it keeps hidden: a stronger one takes over,
    /// one as strong lasts as long as the longer, and its flags follow the newer. Whether it changed.
    pub fn update(&mut self, by: &EffectView) -> bool {
        let endless = |e: &EffectView| e.duration == -1;
        let shorter = |a: &EffectView, b: &EffectView| !endless(a) && (a.duration < b.duration || endless(b));
        let mut changed = false;
        if by.amplifier > self.amplifier {
            self.amplifier = by.amplifier;
            self.duration = by.duration;
            changed = true;
        } else if by.amplifier == self.amplifier && shorter(self, by) {
            self.duration = by.duration;
            changed = true;
        }
        if (!by.ambient && self.ambient) || changed {
            self.ambient = by.ambient;
            changed = true;
        }
        if by.visible != self.visible {
            self.visible = by.visible;
            changed = true;
        }
        if by.show_icon != self.show_icon {
            self.show_icon = by.show_icon;
            changed = true;
        }
        changed
    }
}

/// A damage type of the game's data (`minecraft:in_fire`), as translated code reads it through a
/// `DamageSource` (`type()`, `getMsgId`, `is(TagKey)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DamageTypeView {
    /// Its id (`minecraft:in_fire`).
    pub id: String,
    /// Its death message's id (`inFire`: `death.attack.inFire`).
    pub msg_id: String,
    /// The hunger a hit of it costs (`DamageType.exhaustion`).
    pub exhaustion: f32,
    /// The damage type tags it is in (`minecraft:is_fire`, no `#`).
    pub tags: Vec<String>,
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

/// A random stream a host lends translated code for one call: the world generator's feature
/// random, which a mod's placement modifier or feature draws from as vanilla's code would.
pub trait HostRandom {
    fn next_int(&mut self) -> i32;
    /// `nextInt(bound)`; 0 for a bound below 1.
    fn next_int_bound(&mut self, bound: i32) -> i32;
    fn next_long(&mut self) -> i64;
    fn next_boolean(&mut self) -> bool;
    fn next_float(&mut self) -> f32;
    fn next_double(&mut self) -> f64;
    fn next_gaussian(&mut self) -> f64;
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
    /// The world's time of day in ticks (`Level.getDayTime`); it counts past 24000.
    fn day_time(&self) -> i64 {
        self.game_time()
    }
    /// The light at `pos` as `(block, sky)`, each 0 to 15 (`Level.getBrightness` of
    /// `LightLayer.BLOCK` and `SKY`): open daylight where the host keeps none.
    fn light(&self, _pos: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    /// The rain and thunder levels, each 0.0 to 1.0 (`Level.getRainLevel`, `getThunderLevel`).
    fn weather(&self) -> (f32, f32) {
        (0.0, 0.0)
    }
    /// The lowest block y and the height of the world (`getMinBuildHeight`, `getHeight`).
    fn world_height(&self) -> (i32, i32) {
        (-64, 384)
    }
    /// The random stream the host lends the call (the world generator's: the feature's own), which
    /// `RandomSource` objects of the call draw from; None where the host lends none.
    fn random(&mut self) -> Option<&mut dyn HostRandom> {
        None
    }
    /// `getHeight(heightmap, x, z)`: the column's top by heightmap (vanilla's name:
    /// `WORLD_SURFACE_WG`); None where the host keeps no heightmaps.
    fn height(&mut self, _heightmap: &str, _x: i32, _z: i32) -> Option<i32> {
        None
    }
    /// The world's seed (`WorldGenLevel.getSeed()`), where the host has one.
    fn world_seed(&self) -> Option<i64> {
        None
    }
    /// The biome at `pos` (`minecraft:plains`), where the host knows it (the world generator does).
    fn biome(&self, _pos: [i32; 3]) -> Option<String> {
        None
    }
    /// Whether biome `biome` is in biome tag `tag` (`minecraft:is_overworld`), as the game's tags
    /// hold it.
    fn biome_in_tag(&self, _biome: &str, _tag: &str) -> bool {
        false
    }
    /// The ids holder set `set` names (its JSON: an id, `#tag`, a list, NeoForge's `any`, `or`,
    /// `and` and `not` objects) in registry `registry` (`biome`, `placed_feature`), in order, as the
    /// game's registries and tags hold them; None where the host keeps no registries.
    fn holder_set(&self, _registry: &str, _set: &str) -> Option<Vec<String>> {
        None
    }
    /// Biome `id`'s data (its JSON: `temperature`, `has_precipitation`...), where the host has the
    /// biomes' data.
    fn biome_json(&self, _id: &str) -> Option<String> {
        None
    }
    /// `PlacedFeature.place(level, generator, random, pos)` from the mod's code: the placed feature
    /// `json` (an object, or a name as a JSON string) placed at `pos` by the world generator, on
    /// its region and the feature's random stream (the host's [`Host::random`]). Whether it placed
    /// anything; an error where the host can't place features.
    fn place_feature(&mut self, _json: &str, _pos: [i32; 3]) -> Result<bool, String> {
        Err("no world generator to place a feature with".into())
    }
    /// A raw write into the chunk section (`LevelChunkSection.setBlockState`, what features write
    /// through `BulkSectionAccess`): no neighbour updates, no heightmaps. Where the host keeps no
    /// sections apart, a plain `set_block_state`.
    fn set_section_state(&mut self, pos: [i32; 3], state: &str) -> bool {
        self.set_block_state(pos, state)
    }
    /// `WorldGenLevel.ensureCanWrite(pos)`: whether the region being generated may write there.
    fn can_write(&self, _pos: [i32; 3]) -> bool {
        true
    }
    /// Game rule `name` as the game has it (26.3's name: `mob_griefing`), a boolean as 0 or 1;
    /// None for a rule the game doesn't have, or a host that keeps no rules.
    fn game_rule(&self, _name: &str) -> Option<i32> {
        None
    }
    /// The strongest redstone signal reaching the block at `pos` from its six neighbours, 0 to 15
    /// (`SignalGetter.getBestNeighborSignal`, what `hasNeighborSignal` asks): 0 where the world
    /// isn't the authoritative server's.
    fn neighbor_signal(&mut self, _pos: [i32; 3]) -> i32 {
        0
    }
    /// The redstone signal the block at `pos` sends back along `direction` (`Direction`'s data
    /// value: the way from the block asking to `pos`), 0 to 15: `SignalGetter.getSignal`, or with
    /// `direct` only its strong signal (`getDirectSignal`). 0 where the world isn't the
    /// authoritative server's.
    fn signal(&mut self, _pos: [i32; 3], _direction: u8, _direct: bool) -> i32 {
        0
    }
    /// A neighbour update the mod's code asks for, on the authoritative server's level: what
    /// [`NeighborUpdate`] says hears that `block` (a block name) changed. Nothing elsewhere.
    fn update_neighbors(&mut self, _update: NeighborUpdate, _block: &str) {}
    /// Schedules a tick of `block` (a block name) at `pos` in `delay` game ticks
    /// (`Level.scheduleTick`); one already queued there stays. False where nothing ticks blocks
    /// (a client, an unknown block, a position not loaded).
    fn schedule_block_tick(&mut self, _pos: [i32; 3], _block: &str, _delay: i32) -> bool {
        false
    }
    /// [`Host::schedule_block_tick`] at `priority` (`TickPriority`'s value: -3, extremely high, to
    /// 3, extremely low; 0 is normal): of the ticks due in the same game tick, the lower value runs
    /// first (a diode facing another one ticks before it). A host that keeps no priorities
    /// schedules it as a normal one.
    fn schedule_block_tick_at_priority(&mut self, pos: [i32; 3], block: &str, delay: i32, priority: i32) -> bool {
        let _ = priority;
        self.schedule_block_tick(pos, block, delay)
    }
    /// Whether a tick of `block` (a block name) is queued at `pos` (`hasScheduledTick`), or, with
    /// `this_tick`, about to run in the game tick under way (`willTickThisTick`: taken from the
    /// queue, not run yet). False where nothing ticks blocks.
    fn block_tick_queued(&mut self, _pos: [i32; 3], _block: &str, _this_tick: bool) -> bool {
        false
    }
    /// `canSurvive` as a vanilla block class declares it, for the block `state` at `pos` (as
    /// [`Host::block_state`] writes it): the rule of the first of `classes` (simple names, nearest
    /// first, as `LadderBlock`) that the state's block has among its classes, the level only read.
    /// What a mod block's `super.canSurvive` gets. None where nothing runs it (the default).
    fn vanilla_can_survive(&self, _classes: &[&str], _pos: [i32; 3], _state: &str) -> Option<bool> {
        None
    }
    /// `updateShape` as a vanilla block class declares it, for the block `state` at `pos` whose
    /// neighbour toward `direction` (`Direction`'s ordinal) at `neighbour_pos` is `neighbour`
    /// (states as [`Host::block_state`] writes them): the rule of the first of `classes` (simple
    /// names, nearest first, as `StairBlock`) that the state's block has among its classes, with the
    /// level only read; the ticks it schedules are the update's. What a mod block's
    /// `super.updateShape` gets. None where nothing runs it (the default: outside a shape update).
    #[allow(clippy::too_many_arguments)]
    fn vanilla_update_shape(
        &mut self,
        _classes: &[&str],
        _pos: [i32; 3],
        _state: &str,
        _direction: usize,
        _neighbour_pos: [i32; 3],
        _neighbour: &str,
    ) -> Option<String> {
        None
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
    /// The acting player's UUID: whose inventory the slot methods reach. None when no player acts
    /// or the game can't say.
    fn player_id(&self) -> Option<u128> {
        None
    }
    /// The hotbar slot the acting player's main hand holds (`Inventory.selected`), 0 to 8.
    fn player_selected(&self) -> u8 {
        0
    }
    /// Whether the slot methods reach the whole inventory (every slot 0 to 40), not only the hands
    /// and the armour: what `Inventory.add` needs to place a stack where vanilla would.
    fn player_inventory(&self) -> bool {
        false
    }
    /// The stack in slot `slot` of the acting player's inventory (`Inventory.getItem`), by
    /// vanilla's numbering: 0 to 8 the hotbar, 9 to 35 the rest, 36 to 39 the armour from the feet
    /// up, 40 the off hand; None when empty. A host that knows only the hands and the armour
    /// answers those, and None for the rest.
    fn player_slot(&self, slot: u8) -> Option<HandStack> {
        match slot {
            OFF_HAND_SLOT => self.player_hand(1),
            36..=39 => self.player_worn(39 - slot),
            s if s == self.player_selected() => self.player_hand(0),
            _ => None,
        }
    }
    /// Puts `stack` in slot `slot` (`Inventory.setItem`), its `max_damage` aside; None empties it.
    /// False when the game can't (a slot past 40; a host that knows only the hands, for another).
    fn set_player_slot(&mut self, slot: u8, stack: Option<&HandStack>) -> bool {
        let hand = match slot {
            OFF_HAND_SLOT => 1,
            s if s == self.player_selected() => 0,
            _ => return false,
        };
        self.set_player_hand(hand, stack);
        true
    }
    /// Changes slot `slot`'s stack in place, as [`Host::update_player_hand`] does a hand's: its
    /// count (0 empties it), damage, and mod components set or removed; vanilla's other data on it
    /// stays. False when the game refused or can't.
    fn update_player_slot(&mut self, slot: u8, count: u32, damage: u32, components: &[(String, Option<Nbt>)]) -> bool {
        match slot {
            OFF_HAND_SLOT => self.update_player_hand(1, count, damage, components),
            s if s == self.player_selected() => self.update_player_hand(0, count, damage, components),
            _ => false,
        }
    }
    /// The hotbar slot (0 to 8) player `uuid`'s main hand holds, a player other than the acting
    /// one: with nobody acting, a block entity's tick hands a player their tool (Create's toolbox),
    /// and a player's own event reaches them by their object. None when the host can't reach that
    /// player (not here, dead, another player's state in place), and by default: then the other
    /// `_of` methods don't reach them either.
    fn player_selected_of(&mut self, _uuid: u128) -> Option<u8> {
        None
    }
    /// The stack in slot `slot` (numbered as [`Host::player_slot`]) of player `uuid`'s inventory,
    /// a player other than the acting one ([`Host::player_selected_of`]). `Some(None)` for an
    /// empty slot; None when the host can't reach that player, and by default.
    fn player_slot_of(&mut self, _uuid: u128, _slot: u8) -> Option<Option<HandStack>> {
        None
    }
    /// Puts `stack` in slot `slot` of player `uuid`'s inventory (`Inventory.setItem`), its
    /// `max_damage` aside; None empties it. False when the host can't reach that player or the
    /// game refused, and by default.
    fn set_player_slot_of(&mut self, _uuid: u128, _slot: u8, _stack: Option<&HandStack>) -> bool {
        false
    }
    /// Changes the stack in slot `slot` of player `uuid`'s inventory in place, as
    /// [`Host::update_player_slot`] does the acting player's. False when the host can't reach that
    /// player or the game refused, and by default.
    fn update_player_slot_of(&mut self, _uuid: u128, _slot: u8, _count: u32, _damage: u32, _components: &[(String, Option<Nbt>)]) -> bool {
        false
    }
    /// Where the acting player stands (their feet), and their look (yaw, pitch in degrees); None
    /// when the game can't say.
    fn player_place(&self) -> Option<([f64; 3], f32, f32)> {
        None
    }
    /// The operator level (0 to 4) of the player `uuid`, as their command source has it; None for
    /// a player not here.
    fn player_permission(&self, _uuid: u128) -> Option<u8> {
        None
    }
    /// On a client, the local player as a client command's source has them: their name and
    /// operator level (`LocalPlayer.getPermissionLevel`, as the server last said it). None
    /// elsewhere.
    fn client_player(&self) -> Option<(String, u8)> {
        None
    }
    /// On a client, the local player's entity id (`LocalPlayer.getId()`): the one the server gave
    /// them, by which its packets name them. None elsewhere.
    fn client_player_id(&self) -> Option<i32> {
        None
    }
    /// The collision boxes of the block at `pos` as the game has them (`BlockState.getCollisionShape`
    /// with the empty context), each relative to the block's corner; None when the game can't say.
    fn collision_boxes(&self, _pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {
        None
    }
    /// The outline boxes of the block at `pos` as the game has them (`BlockState.getShape` with the
    /// empty context and its random offset: what a player targets), each relative to the block's
    /// corner; None when the game can't say.
    fn outline_boxes(&self, _pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {
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
    /// A message for the server's player `uuid`, whoever acts (a mob's killer told of its death):
    /// as [`Host::player_message`]'s. False when they aren't on, or the host reaches no other
    /// player than the acting one.
    fn tell_player(&mut self, _uuid: u128, _component: &str, _overlay: bool) -> bool {
        false
    }
    /// Attribute `attribute` (the game's id: `minecraft:movement_speed`) of the living entity
    /// `entity` of the level in place, None for the player in place (the acting player; a client's
    /// own): its value, base and modifiers. None when the entity or its attribute isn't there, or
    /// the host can't tell.
    fn attribute(&mut self, _entity: Option<i32>, _attribute: &str) -> Option<AttributeView> {
        None
    }
    /// Makes `change` to an attribute named as [`Host::attribute`] names it: false when it wasn't
    /// made (no such attribute, a modifier's id there already or not there).
    fn change_attribute(&mut self, _entity: Option<i32>, _attribute: &str, _change: &AttributeChange) -> bool {
        false
    }
    /// The active effects of the living entity `entity` (as [`Host::attribute`] names it), in no
    /// particular order. None when it isn't a living entity the host knows, or the host can't tell.
    fn effects(&mut self, _entity: Option<i32>) -> Option<Vec<EffectView>> {
        None
    }
    /// Gives `entity` the effect `effect` (`LivingEntity.addEffect`: one of the same effect there
    /// already is updated as vanilla's; with `force`, `forceAddEffect`, replaced). Whether it was
    /// added or changed what was there: not when the entity can't have it (a spider's poison).
    fn add_effect(&mut self, _entity: Option<i32>, _effect: &EffectView, _force: bool) -> bool {
        false
    }
    /// Takes the effect `effect` (`minecraft:speed`) off `entity` (`removeEffect`): whether it had it.
    fn remove_effect(&mut self, _entity: Option<i32>, _effect: &str) -> bool {
        false
    }
    /// The player in place's health and food: the acting player's on a server, a client's own.
    /// None when the host can't tell.
    fn player_vitals(&mut self) -> Option<VitalsView> {
        None
    }
    /// Sets the player in place's health, absorption and food to `to`'s, as vanilla's setters do
    /// (the health kept between 0 and the maximum, the absorption between 0 and its attribute's
    /// maximum): whether there was a player to change.
    fn set_player_vitals(&mut self, _to: &VitalsView) -> bool {
        false
    }
    /// Level event `event` at `pos`, with `data` (`Level.levelEvent`, vanilla's `LevelEvent` codes:
    /// a dispenser's click 1000, a door's sound 1005..., smoke 2000, a block breaking 2001 with its
    /// state's id in `data`, bone meal 1505...): its sound and particles, for the players near.
    /// Whether the game made it.
    fn level_event(&mut self, _event: i32, _pos: [i32; 3], _data: i32) -> bool {
        false
    }
    /// The game's id of the block state `state` names (`Block.getId`: its place in the game's
    /// table of states, which the network uses): None for a state the game doesn't know.
    fn state_id(&self, _state: &str) -> Option<i32> {
        None
    }
    /// The block state of the game's id `id` (`Block.stateById`), as text: air past the table, as
    /// vanilla answers. None when the host can't tell.
    fn state_of_id(&self, _id: i32) -> Option<String> {
        None
    }
    /// A line of a mod's command's answer (`CommandSourceStack.sendSuccess`, `sendFailure`), for
    /// whoever ran it, as vanilla commands' feedback: `component` is vanilla's JSON text.
    fn command_feedback(&mut self, _component: &str) {}
    /// Runs a command line of the game's own (vanilla's commands, or another bundle's mod's) for
    /// a mod, as `Commands.performPrefixedCommand` runs it with the mod's source: false when it
    /// failed or the host runs none.
    fn run_command(&mut self, _run: &CommandRun) -> bool {
        false
    }
    /// The name of the server's player `uuid` (a selector naming them), when they are on.
    fn player_name(&self, _uuid: u128) -> Option<String> {
        None
    }
    /// The criteria of the server's advancements whose trigger is a mod's (any namespace but
    /// `minecraft`), in the tree's order: what a player's `PlayerAdvancements` listens for on the
    /// mods' triggers (`registerListeners`). None when the host can't tell (a client).
    fn mod_criteria(&mut self) -> Option<Vec<ModCriterion>> {
        None
    }
    /// Whether the server has the advancement `id` (`ServerAdvancementManager.get`).
    fn has_advancement(&mut self, _id: &str) -> bool {
        false
    }
    /// The server's player `player`'s progress in the advancement `id`
    /// (`PlayerAdvancements.getOrStartProgress`): None for an advancement or a player the host
    /// doesn't have.
    fn advancement_progress(
        &mut self,
        _player: u128,
        _id: &str,
    ) -> Option<AdvancementProgressView> {
        None
    }
    /// Grants criterion `criterion` of advancement `advancement` to the server's player `player`
    /// as `PlayerAdvancements.award` does, done advancement or not: one it completes is awarded,
    /// with its rewards, toast and chat line. Whether it was granted: false when they had it, or
    /// for a player, advancement or criterion the host doesn't have.
    fn award_criterion(&mut self, _player: u128, _advancement: &str, _criterion: &str) -> bool {
        false
    }
    /// Whether the server's player `player` passes the `player` predicate of criterion `criterion`
    /// of advancement `advancement` (`SimpleInstance.player()`), tested in their own context as
    /// `SimpleCriterionTrigger.trigger` tests it: true for a criterion with none, false when the
    /// host can't reach the player.
    fn criterion_player_holds(
        &mut self,
        _player: u128,
        _advancement: &str,
        _criterion: &str,
    ) -> bool {
        false
    }
    /// The server's player `player`'s value of the statistic of type `ty` and value `key`
    /// (`ServerStatsCounter.getValue`), both registry ids: `minecraft:custom` and
    /// `minecraft:jump`, `minecraft:mined` and `minecraft:stone`, a mod's custom statistic. None
    /// when the host can't reach the player.
    fn stat_value(&mut self, _player: u128, _ty: &str, _key: &str) -> Option<i32> {
        None
    }
    /// Sets that statistic of the server's player `player` to `value` (`ServerStatsCounter.setValue`):
    /// kept and sent to their client as the game's own statistics are; the scoreboard isn't told.
    /// False when the host can't reach the player.
    fn set_stat(&mut self, _player: u128, _ty: &str, _key: &str, _value: i32) -> bool {
        false
    }
    /// The scoreboard's part of `ServerPlayer.awardStat` and `resetStat`
    /// (`Scoreboard.forAllObjectives(stat, player, ...)`): the player's score in each objective
    /// whose criterion is that statistic goes up by `change`, or is reset when it is None. False
    /// when the host can't reach the player.
    fn score_stat(&mut self, _player: u128, _ty: &str, _key: &str, _change: Option<i32>) -> bool {
        false
    }
    /// The open world's folder (beside its `level.dat`), where a server's mods keep their files
    /// (`MinecraftServer.getWorldPath`, the levels' saved data), from `ServerAboutToStartEvent` on;
    /// None where the host has no world (a client's).
    fn world_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
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
    /// Drawing: whether the game's atlas has sprite `name` (`create:special/blank`,
    /// `minecraft:block/stone`); the game leaves out a face drawn from one it lacks. True for a host
    /// that can't tell.
    fn has_sprite(&self, _name: &str) -> bool {
        true
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
    /// Drawing in the world: adds `text`, a mod's `Font.drawInBatch` in a block entity's, an
    /// entity's or a level stage's renderer. False when the game draws no text there.
    fn world_text(&mut self, _text: &WorldText) -> bool {
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
    /// The damage at which `item` breaks (its `max_damage`: 250 for an iron sword), 0 for one
    /// without durability; None when the game doesn't know it.
    fn item_max_damage(&self, _item: &str) -> Option<u32> {
        None
    }
    /// The equipment slot `item` is worn in (`head`, `chest`, `legs`, `feet`, `body`, or `offhand`
    /// for a shield), as the game equips it; None for an item worn nowhere or one the game doesn't
    /// know.
    fn item_equip_slot(&self, _item: &str) -> Option<String> {
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
    /// Whether entity type `kind` (`minecraft:spider`) is in entity type tag `tag`
    /// (`minecraft:arthropod`, no `#`), as the game's loaded tags hold it (`EntityType.is(TagKey)`).
    /// False for a tag the game doesn't have.
    fn entity_type_in_tag(&self, _kind: &str, _tag: &str) -> bool {
        false
    }
    /// The way the fluid at `pos` flows (`FluidState.getFlow(level, pos)`, vanilla's
    /// `FlowingFluid.getFlow`): x, y, z, zero for still fluid or none.
    fn fluid_flow(&self, _pos: [i32; 3]) -> [f64; 3] {
        [0.0; 3]
    }
    /// The fluid that fluid `fluid` (a name: `minecraft:flowing_water`) takes at `pos` if the
    /// block there were `state`, from its neighbours (`FlowingFluid.getNewLiquid`, as the game's
    /// own fluids work it out): the fluid's name, its amount and whether it falls. None where no
    /// game answers, or for a fluid the game doesn't know.
    fn new_liquid(&mut self, _pos: [i32; 3], _fluid: &str, _state: &str) -> Option<(String, u8, bool)> {
        None
    }
    /// The server's players: UUID and where each is (in the hook's dimension or not). None on a
    /// client.
    fn players(&self) -> Vec<(u128, [f64; 3])> {
        Vec::new()
    }
    /// The server's players by entity id (`Entity.getId`): each one's id, UUID and place. None on
    /// a client.
    fn player_entities(&self) -> Vec<(i32, u128, [f64; 3])> {
        Vec::new()
    }
    /// The damage types of the game's data, with their tags. None where the game gives no data.
    fn damage_types(&self) -> Vec<DamageTypeView> {
        Vec::new()
    }
    /// Sends a mod's payload on the game's mod message channel: `channel` is the payload type's id
    /// (`create:server_speed`), `data` what its `StreamCodec` wrote. False when it can't go (no
    /// such player, a server that carries no mod messages, past the game's caps).
    fn send_payload(&mut self, _to: PayloadTarget, _channel: &str, _data: &[u8]) -> bool {
        false
    }
    /// The server level's entities whose box meets `bounds` (min x, y, z, max x, y, z), players
    /// aside (`player_entities`), as `Level.getEntities` finds the game's own: mobs, item
    /// entities, boats... A mod's entities are the runtime's. None on a client.
    fn entities_in(&self, _bounds: [f64; 6]) -> Vec<EntityView> {
        Vec::new()
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
    /// The container of the vanilla block at `pos` that NeoForge gives an item handler
    /// (`Capabilities.ItemHandler.BLOCK`: a chest, a barrel, a furnace, a hopper...). None for any
    /// other block, and on a client.
    fn container(&self, _pos: [i32; 3]) -> Option<ContainerFacts> {
        None
    }
    /// What slot `slot` of the container at `pos` holds (`Container.getItem`); None when it's empty.
    fn container_item(&self, _pos: [i32; 3], _slot: u32) -> Option<HandStack> {
        None
    }
    /// Puts `stack` (None empties the slot) in slot `slot` of the container at `pos`
    /// (`Container.setItem`: a disc put in a jukebox starts its song), without
    /// [`Host::container_changed`]. False when there is no such slot.
    fn set_container_item(&mut self, _pos: [i32; 3], _slot: u32, _stack: Option<&HandStack>) -> bool {
        false
    }
    /// `Container.setChanged` of the container at `pos`: the level saves it, comparators read it
    /// again.
    fn container_changed(&mut self, _pos: [i32; 3]) {}
    /// Whether `stack` may go in slot `slot` of the container at `pos` (`Container.canPlaceItem`).
    fn container_can_place(&self, _pos: [i32; 3], _slot: u32, _stack: &HandStack) -> bool {
        false
    }
    /// Whether `stack` may go in slot `slot` of the container at `pos` through a face
    /// (`WorldlyContainer.canPlaceItemThroughFace`, which no vanilla container decides by the face:
    /// a shulker box never goes in a shulker box).
    fn container_can_place_through(&self, _pos: [i32; 3], _slot: u32, _stack: &HandStack) -> bool {
        true
    }
    /// The slots face `face` (`Direction`'s data value: 0 down, 1 up, 2 north, 3 south, 4 west,
    /// 5 east) of the container at `pos` reaches, in order (`WorldlyContainer.getSlotsForFace`).
    fn container_face_slots(&self, _pos: [i32; 3], _face: u8) -> Vec<u32> {
        Vec::new()
    }
    /// Whether `stack` may be taken out of slot `slot` of the container at `pos` through face
    /// `face` (`WorldlyContainer.canTakeItemThroughFace`: a furnace's fuel only as a bucket through
    /// its bottom, a brewing stand's ingredient only as a glass bottle).
    fn container_can_take(&self, _pos: [i32; 3], _slot: u32, _stack: &HandStack, _face: u8) -> bool {
        true
    }
    /// The hopper at `pos`, empty before, took items through its item handler: it waits 8 ticks
    /// before it moves any (`setCooldown(8)`), unless it waits longer already
    /// (`isOnCustomCooldown`).
    fn hopper_filled(&mut self, _pos: [i32; 3]) {}
    /// The server level's entity `id` (`Level.getEntity`): none on a client, for a player, or
    /// when the level holds no such entity.
    fn entity(&self, _id: i32) -> Option<EntityView> {
        None
    }
    /// Hurts the living mob `id` by `amount` (`LivingEntity.hurt`): whether it lost health. A blow
    /// that takes the last of it kills the mob, with its loot.
    fn hurt_entity(&mut self, _id: i32, _amount: f32) -> bool {
        false
    }
    /// Hurts the living mob `id` by `amount` as `LivingEntity.hurt(source, amount)` does, the
    /// source told (`by`): its invulnerability frames, armor and effects, the knockback, the hurt
    /// flash and sound, the mob's anger and the kill credited. Whether it was hurt. A host that
    /// can't tell sources hurts it as [`Host::hurt_entity`].
    fn hurt_entity_by(&mut self, id: i32, amount: f32, _by: &HurtBy) -> bool {
        self.hurt_entity(id, amount)
    }
    /// Hurts the server's player `uuid` by `amount` as `LivingEntity.hurt(source, amount)` does on a
    /// `ServerPlayer`, the source told (`by`): nothing in creative, else their invulnerability
    /// frames, a raised shield, armor and effects, the knockback and the hurt sound. Whether they
    /// were hurt; false for a player the host doesn't have.
    fn hurt_player_by(&mut self, _uuid: u128, _amount: f32, _by: &HurtBy) -> bool {
        false
    }
    /// Knocks the living mob `id` back as `LivingEntity.knockback(strength, x, z)` does: away from
    /// the direction (`x`, `z`), less its knockback resistance. False for no such mob.
    fn knockback_entity(&mut self, _id: i32, _strength: f64, _x: f64, _z: f64) -> bool {
        false
    }
    /// Runs the enchantments' effects that follow blow `by` on `victim`, as
    /// `EnchantmentHelper.doPostAttackEffectsWithItemSource` does: the victim's worn ones (thorns
    /// hurting the one the blow is credited to) and, when a living entity is credited with it,
    /// those of `weapon` (fire aspect setting the victim alight; the ones that need a direct blow
    /// don't act for a projectile's). False when the game ran none.
    fn post_attack_effects(&mut self, _victim: Struck, _by: &HurtBy, _weapon: &AttackWeapon) -> bool {
        false
    }
    /// Kills entity `id` as `/kill` does (`Entity.kill`): a mob dies with its loot, a vehicle
    /// breaks. False when the level holds no such entity.
    fn kill_entity(&mut self, _id: i32) -> bool {
        false
    }
    /// Takes the mob `id` out of the level with no death and no loot (`Entity.discard`).
    fn remove_entity(&mut self, _id: i32) -> bool {
        false
    }
    /// Adds an entity of `kind` (`minecraft:skeleton_horse`) at `pos` to the level, made from its
    /// compound `nbt` (`Rotation`, `Motion`, `CustomName`, a TNT's `fuse`...) as `/summon <kind>
    /// <pos> <nbt>` makes one: its id. None when the game didn't add it (a kind it can't make, a
    /// chunk that isn't loaded, a mod that kept it out).
    fn summon(&mut self, _kind: &str, _pos: [f64; 3], _nbt: &std::collections::BTreeMap<String, Nbt>) -> Option<i32> {
        None
    }
    /// Strikes lightning at `pos` (`LightningBolt` added to the level): only its flash and sound
    /// when `visual_only`, and credited to the player `cause` (`setCause`: a channeling trident's
    /// thrower). Its id, or None.
    fn strike_lightning(&mut self, _pos: [f64; 3], _visual_only: bool, _cause: Option<u128>) -> Option<i32> {
        None
    }
    /// The entity `id` as the level saves it (`Entity.saveWithoutId`); None for no such entity.
    fn entity_nbt(&mut self, _id: i32) -> Option<std::collections::BTreeMap<String, Nbt>> {
        None
    }
    /// Sets the living mob `id`'s health (`LivingEntity.setHealth`), above 0.
    fn set_entity_health(&mut self, _id: i32, _health: f32) -> bool {
        false
    }
    /// Adds `velocity` (blocks a tick) to the mob `id`'s motion (`Entity.push`).
    fn push_entity(&mut self, _id: i32, _velocity: [f64; 3]) -> bool {
        false
    }
    /// The mob `id`'s motion in blocks a tick (`Entity.getDeltaMovement`): None for no such mob, or
    /// a host that can't tell.
    fn entity_motion(&self, _id: i32) -> Option<[f64; 3]> {
        None
    }
    /// Sets the mob `id`'s motion (`Entity.setDeltaMovement`): false for no such mob.
    fn set_entity_motion(&mut self, _id: i32, _motion: [f64; 3]) -> bool {
        false
    }
    /// The player `uuid`'s motion in blocks a tick, as this side holds it (a server's copy of a
    /// player's, a client's own player's): None for a player the host doesn't have.
    fn player_motion(&mut self, _uuid: u128) -> Option<[f64; 3]> {
        None
    }
    /// Sets the player `uuid`'s motion as this side holds it (`setDeltaMovement`): a client's own
    /// player moves by it; a server's copy changes, as vanilla's does without `hurtMarked`.
    fn set_player_motion(&mut self, _uuid: u128, _motion: [f64; 3]) -> bool {
        false
    }
    /// Moves the mob `id` to `pos` at once (`Entity.teleportTo`).
    fn teleport_entity(&mut self, _id: i32, _pos: [f64; 3]) -> bool {
        false
    }
    /// Turns the mob `id` by the angles `turn` gives (`Entity.setYRot`, `setXRot`,
    /// `LivingEntity.setYHeadRot`, `setYBodyRot`; a pitch is kept between -90 and 90): false for
    /// no such mob or an angle that isn't finite.
    fn turn_entity(&mut self, _id: i32, _turn: &Turn) -> bool {
        false
    }
    /// Turns a client's own player (`LocalPlayer.setYRot`, `setXRot`): their view turns with it.
    /// False where there is no such player or an angle that isn't finite.
    fn turn_client_player(&mut self, _yaw: Option<f32>, _pitch: Option<f32>) -> bool {
        false
    }
    /// Moves the mob `id` by `movement` (`Entity.move(MoverType.SELF, movement)`), with its
    /// collisions, as its own motion moves it: where that took it, None for no such mob.
    fn move_entity(&mut self, _id: i32, _movement: [f64; 3]) -> Option<[f64; 3]> {
        None
    }
    /// Sends the player `uuid` their motion as the server holds it (`hurtMarked = true` on a
    /// server's player): their client moves by it. False for a player the host doesn't have.
    fn mark_player_motion(&mut self, _uuid: u128) -> bool {
        false
    }
    /// Moves a client's own player by `movement` as their own move does
    /// (`LocalPlayer.move(MoverType.SELF, movement)`: blocks stop them, they step up), as a client
    /// mod's code moves them (Create's belts carry the player on their client). False where there
    /// is no such player.
    fn move_client_player(&mut self, _movement: [f64; 3]) -> bool {
        false
    }
    /// A client's own player's walking input this tick (`LocalPlayer.xxa`, `zza`: sideways and
    /// forward, -1 to 1, slowed by sneaking or by an item in use). None where there is none.
    fn client_player_input(&self) -> Option<[f32; 2]> {
        None
    }
    /// Whether a client's own player stands on something (`LocalPlayer.onGround`). None where
    /// there is no such player.
    fn client_player_on_ground(&self) -> Option<bool> {
        None
    }
    /// On a client, whether its game is paused (`Minecraft.isPaused`): a single player screen that
    /// pauses it is open. False elsewhere.
    fn paused(&self) -> bool {
        false
    }
    /// The text on the clipboard (`KeyboardHandler.getClipboard`), empty when none: a client's.
    fn clipboard(&self) -> String {
        String::new()
    }
    /// Copies `text` to the clipboard (`KeyboardHandler.setClipboard`).
    fn set_clipboard(&mut self, _text: &str) {}
    /// [`Host::spawn_item_entity`] for a whole stack, its damage and mod components with it (a filter
    /// thrown out of a menu keeps its filter). A host that can't carry them spawns the item and count.
    fn spawn_stack_entity(&mut self, stack: &HandStack, pos: [f64; 3], motion: [f64; 3], pickup_delay: u32) -> Option<u32> {
        self.spawn_item_entity(&stack.item, stack.count, pos, motion, pickup_delay)
    }
    /// `BlockItem.place` of `stack`'s block by one other than the acting player (a mod's fake
    /// player: Create's deployer), as the game places a player's or a dispenser's: clicking block
    /// `pos` on its face `face` (a direction ordinal) at `hit`, the click `inside` its shape or not,
    /// looking `look` (yaw, pitch), sneaking or not. The block's own rules choose its state, a mod
    /// block's included; its block entity, sound and level events follow. Where the block went, or
    /// None when vanilla's placement fails (nothing is written). The stack is the caller's to shrink.
    #[allow(clippy::too_many_arguments)]
    fn place_item_block(&mut self, _stack: &HandStack, _pos: [i32; 3], _face: u8, _hit: [f64; 3], _inside: bool, _look: (f32, f32), _sneaking: bool) -> Option<[i32; 3]> {
        None
    }
    /// Whether only the right tool gets block state `state`'s drops
    /// (`BlockState.requiresCorrectToolForDrops`), as the game knows the block. False for a state
    /// it doesn't know.
    fn block_requires_tool(&self, _state: &str) -> bool {
        false
    }
    /// What item `item`'s tool (its `minecraft:tool` component) does to block state `state`, as
    /// the game knows the item. None for an item that is no tool: vanilla's `Item.getDestroySpeed`
    /// is then 1, it is the right tool for no block's drops, and mining doesn't wear it.
    fn item_tool(&self, _item: &str, _state: &str) -> Option<ToolFacts> {
        None
    }
    /// Block state `state`'s sounds (its `SoundType`), as the game knows them. None for a state it
    /// doesn't know.
    fn block_sounds(&self, _state: &str) -> Option<BlockSounds> {
        None
    }
    /// `Block.getDrops(state, level, pos, blockEntity, entity, tool)` of block state `state` broken
    /// at `pos` with `tool` (None: by hand, `ItemStack.EMPTY`): its loot table, rolled as the game
    /// rolls a player's break, with what the game kept of the block entity there. Nothing spawns:
    /// the drops are the caller's.
    fn block_drops(&mut self, _pos: [i32; 3], _state: &str, _tool: Option<&HandStack>) -> Vec<HandStack> {
        Vec::new()
    }
    /// `BlockState.spawnAfterBreak(level, pos, tool, dropExperience)` of vanilla block state
    /// `state` just broken at `pos` with `tool`: an infested block lets its silverfish out (not for
    /// a tool with silk touch, nor without block drops). NeoForge's other vanilla blocks drop their
    /// experience from the game's own break (`getExpDrop`), so nothing else happens here.
    fn spawn_after_break(&mut self, _pos: [i32; 3], _state: &str, _tool: Option<&HandStack>) {}
    /// `Level.destroyBlockProgress(breaker, pos, progress)`: entity `breaker`'s cracks on the block
    /// at `pos`, stage 0 to 9 (any other value takes them away), for the players around to see.
    fn destroy_block_progress(&mut self, _breaker: i32, _pos: [i32; 3], _progress: i32) {}
    /// NeoForge's `getExpDrop` of vanilla block state `state`: the experience breaking it drops, drawn
    /// as the block draws it (an ore's range, a spawner's 15 + 0..14 + 0..14), before enchantments
    /// (silk touch) take it away. 0 for a block that drops none, or a state the game doesn't know.
    fn block_experience(&mut self, _state: &str) -> i32 {
        0
    }
    /// `Block.popExperience(level, pos, amount)`: experience orbs worth `amount` at the centre of
    /// `pos`, when blocks drop (`doTileDrops`).
    fn pop_experience(&mut self, _pos: [i32; 3], _amount: i32) {}
    /// Whether block state `state` is solid as vanilla's movement and heightmaps once asked
    /// (`BlockState.isSolid`, its `legacySolid`: forced by its properties, else from its collision
    /// shape's size). None for a state the game doesn't know.
    fn block_solid(&self, _state: &str) -> Option<bool> {
        None
    }
    /// Whether block state `state` can hide its neighbours' faces (`canOcclude`, from its
    /// properties) and whether it is an opaque full cube to the renderer (`isSolidRender`), as the
    /// game's block facts have them. None for a state the game doesn't know.
    fn block_occlusion(&self, _state: &str) -> Option<(bool, bool)> {
        None
    }
    /// The face of block state `state`'s occlusion shape toward `direction` (a direction ordinal:
    /// down, up, north, south, west, east), as the game hides the faces beside it
    /// (`getFaceOcclusionShape` of a state that can occlude): boxes in block space, as deep as the
    /// block; none for a face that hides nothing. None where the game can't tell: a server, which
    /// draws nothing, or a state it doesn't know.
    fn face_occlusion(&self, _state: &str, _direction: u8) -> Option<Vec<[f64; 6]>> {
        None
    }
    /// A sound a mod plays (`Level.playSound`, `playLocalSound`, `Entity.playSound`). A server's
    /// game sends it to the players near it; a client's plays it for its player.
    fn play_sound(&mut self, _sound: &Sound) {}
    /// Starts the sound a client mod's own instance plays (`SoundManager.play(instance)`), which
    /// the mod may go on changing (a `TickableSoundInstance`). Its id while the game holds it;
    /// None when it doesn't start (a server, an event the game doesn't have).
    fn start_sound(&mut self, _sound: &InstanceSound) -> Option<u64> {
        None
    }
    /// Moves sound `id` to `at` and sets its volume and pitch, as a ticking instance gives them
    /// each tick. False once the game no longer plays it (it ended); the id is then gone.
    fn update_sound(&mut self, _id: u64, _at: [f64; 3], _volume: f32, _pitch: f32) -> bool {
        false
    }
    /// Stops sound `id` (`SoundManager.stop(instance)`, or an instance that stopped itself).
    fn stop_sound(&mut self, _id: u64) {}
    /// Particles a mod makes: a client's game draws them (`Level.addParticle`), a server's sends
    /// them to the players near them (`ServerLevel.sendParticles`).
    fn particles(&mut self, _particles: &Particles) {}
    /// Drawing the mods' own particles: adds `quad`, a face one of them drew this frame
    /// (`Particle.render` into the particle engine's buffer). False when the game draws none.
    fn particle_quad(&mut self, _quad: &ParticleQuad) -> bool {
        false
    }
    /// Whether a client makes a particle a mod's provider would make at `at`, as the game's particle
    /// engine decides for its own (`LevelRenderer.addParticleInternal`): always when `force`d, else
    /// near the camera and as the particle setting allows (`decreased`: `addAlwaysVisibleParticle`,
    /// which the lowest setting keeps one time in ten).
    fn particle_admitted(&mut self, _at: [f64; 3], _force: bool, _decreased: bool) -> bool {
        true
    }

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
    /// The key the player bound a mod's key mapping `name` (`key.create.toolmenu`) to on the
    /// bundle's page in Options > Mods, as `options.txt` names keys (`key.keyboard.h`); `default`,
    /// the key the mod made it with, when they chose none.
    fn mod_key(&self, _name: &str, default: &str) -> String {
        default.to_owned()
    }
    /// Nothing is over the world (`Minecraft.screen == null`): no game screen, no mods' screen.
    /// Presses reach key mappings only then.
    fn playing(&self) -> bool {
        true
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
    /// The bytes of resource file `path` under the game's resources (`assets/create/lang/en_us.json`,
    /// `data/create/recipe/...`): what the resource packs, the mods and vanilla give for it, the
    /// first that has it. None for a file nothing has.
    fn resource(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }
    /// The resource files under folder `prefix` (`assets/create/models`), as paths from the root.
    fn resource_paths(&self, _prefix: &str) -> Vec<String> {
        Vec::new()
    }
    /// The namespaces with resources under `root` (`assets`, `data`).
    fn resource_namespaces(&self, _root: &str) -> Vec<String> {
        Vec::new()
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

/// Text a mod draws in the world (`Font.drawInBatch` in a block entity's, an entity's or a level
/// stage's renderer), as [`Host::world_text`] gets it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldText {
    /// The text's runs, each in its own style.
    pub runs: Vec<TextRun>,
    /// Where it is drawn: a column-major 4x4 matrix, as [`Host::draw_model`]'s pose, from the
    /// text's own units (a font pixel, y down) to the drawn block's.
    pub pose: [f32; 16],
    /// Where the text starts under `pose`: its top left, in font pixels.
    pub at: [f32; 2],
    /// `0xAARRGGBB`, for runs without a colour of their own.
    pub argb: u32,
    /// Vanilla's drop shadow: the text again a pixel right and down, a quarter as bright, under it.
    pub shadow: bool,
    /// An outline around the text, `0xAARRGGBB` (`drawInBatch8xOutline`: the text drawn eight times
    /// a pixel around it in that colour, under it); 0 for none.
    pub outline: u32,
    /// `LightTexture.pack`: block light in bits 4..8, sky light in bits 20..24.
    pub light: u32,
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

/// A face a mod's particle draws ([`Host::particle_quad`]), as vanilla's particle engine buffers it.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleQuad {
    /// Corners relative to the camera, in blocks, in the order drawn.
    pub positions: [[f32; 3]; 4],
    /// The sprite (`create:particle/air_flow_0`, `minecraft:block/stone`; `horizon/white` for an
    /// untextured face) and each corner's place in it, 0 to 1 across.
    pub sprite: String,
    pub uv: [[f32; 2]; 4],
    /// Each corner's colour, 0xAARRGGBB.
    pub colors: [u32; 4],
    /// The first corner's light, `LightTexture.pack`ed (block light in bits 4..8, sky light in
    /// bits 20..24).
    pub light: u32,
    /// Its render type blends and draws after the translucent blocks (`isTranslucent`:
    /// `PARTICLE_SHEET_TRANSLUCENT`, `TERRAIN_SHEET`, a mod's own type).
    pub translucent: bool,
}

/// A criterion of one of the server's advancements whose trigger is a mod's
/// (`create:andesite_alloy_builtin`), as [`Host::mod_criteria`] lists them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModCriterion {
    /// The advancement's id (`create:andesite_alloy`) and the criterion's name in it (`0`).
    pub advancement: String,
    pub criterion: String,
    /// The trigger's id, as the trigger type registry names it.
    pub trigger: String,
    /// The criterion's `conditions` as JSON text, `{}` for none.
    pub conditions: String,
}

/// A server's player's progress in one advancement ([`Host::advancement_progress`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdvancementProgressView {
    /// Its criteria in the advancement's order, and whether each is granted.
    pub criteria: Vec<(String, bool)>,
    /// Whether it is done: a criterion granted in each of its requirements (`isDone`).
    pub done: bool,
}

/// A command line a mod runs ([`Host::run_command`]) and what its source says: where it runs (None
/// for the server's own source, at the world's spawn in the overworld) and faces (yaw, pitch), its
/// operator level, the player it is (by UUID; None for no entity), and whether its output is
/// dropped (`withSuppressedOutput`, `CommandSource.NULL`, the server's own log).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandRun {
    pub line: String,
    pub at: Option<[f64; 3]>,
    pub rot: [f32; 2],
    pub permission: u8,
    pub player: Option<u128>,
    pub silent: bool,
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

/// The sound a client mod's own instance plays ([`Host::start_sound`]): a `SoundInstance` as its
/// getters give it when it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceSound {
    /// Its event, place, source, volume and pitch.
    pub sound: Sound,
    /// `isLooping()`: it starts again when it ends, at once or `delay` ticks later (`getDelay()`).
    pub looping: bool,
    pub delay: i32,
    /// Ticks before it first starts (`SoundManager.playDelayed`).
    pub start_delay: i32,
    /// `getAttenuation()`: LINEAR, fading with distance, or NONE.
    pub linear: bool,
    /// `isRelative()`: placed relative to the listener.
    pub relative: bool,
    /// `canStartSilent()`: it starts even at volume 0.
    pub can_start_silent: bool,
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
    /// Which of its faces are sturdy (`isFaceSturdy(level, pos, direction, support)`, which
    /// doesn't depend on where a vanilla block stands): bit `support * 6 + direction`, with
    /// `SupportType`'s ordinal (FULL 0, CENTER 1, RIGID 2) and `Direction`'s.
    pub sturdy: u32,
}

/// What a tool (an item's `minecraft:tool` component) does to one block state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolFacts {
    /// `Tool.getMiningSpeed(state)`: the speed of its rule for the state, else its default speed.
    pub speed: f32,
    /// `Tool.isCorrectForDrops(state)`.
    pub correct: bool,
    /// `Tool.damagePerBlock()`: what mining a block that isn't broken instantly wears it.
    pub damage_per_block: i32,
}

/// A block state's `SoundType`: its volume and pitch and the ids of the sound events it breaks,
/// steps, is placed, is hit and is fallen on with (`minecraft:block.stone.break`...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockSounds {
    pub volume: f32,
    pub pitch: f32,
    /// Break, step, place, hit, fall.
    pub events: [String; 5],
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
    /// The acting player's UUID ([`Host::player_id`]).
    pub player_id: Option<u128>,
    /// The hotbar slot the main hand holds: `held` is that slot's stack.
    pub selected: u8,
    /// The inventory's other slots (neither a hand nor the armour), by vanilla's numbering.
    pub slots: HashMap<u8, HandStack>,
    /// Whether the slots stand for the whole inventory ([`Host::player_inventory`]).
    pub inventory: bool,
    /// Other players' inventories by UUID ([`Host::player_slot_of`]), by vanilla's numbering: a
    /// player listed here, even with none, is within reach, their main hand on slot 0.
    pub others: HashMap<u128, HashMap<u8, HandStack>>,
    /// Items' durability (`max_damage`); unset ones have none.
    pub max_damage: HashMap<String, u32>,
    /// The slots items are worn in ([`Host::item_equip_slot`]); unset ones are worn nowhere.
    pub equip_slots: HashMap<String, String>,
    /// Where the acting player stands and looks (yaw, pitch).
    pub place: Option<([f64; 3], f32, f32)>,
    /// What the acting player was given that the main hand didn't take, in order.
    pub given: Vec<(String, u32)>,
    /// Positions sent to players again (`block_changed`), in order.
    pub changed: Vec<[i32; 3]>,
    /// Messages the acting player was sent (`player_message`): JSON text, and whether over the hotbar.
    pub messages: Vec<(String, bool)>,
    /// Messages for players by UUID ([`Host::tell_player`]): to whom, JSON, over the hotbar.
    pub told: Vec<(u128, String, bool)>,
    /// The living entities' attributes ([`Host::attribute`]), by entity (None: the acting player)
    /// and the game's attribute id; changes keep their values computed as vanilla's.
    pub attributes: std::collections::BTreeMap<(Option<i32>, String), AttributeView>,
    /// The living entities' effects ([`Host::effects`]), by entity (None: the acting player): one
    /// listed here, even with none, is a living entity.
    pub effects: std::collections::BTreeMap<Option<i32>, Vec<EffectView>>,
    /// The acting player's health and food ([`Host::player_vitals`]), set as given.
    pub vitals: Option<VitalsView>,
    /// Level events made: the event, where, its data ([`Host::level_event`]).
    pub level_events: Vec<(i32, [i32; 3], i32)>,
    /// The game's block states by id ([`Host::state_id`], [`Host::state_of_id`]): empty for a host
    /// that tells none.
    pub state_ids: Vec<String>,
    /// Experience orbs spawned: where, worth, and the motion given.
    pub orbs: Vec<([f64; 3], u32, Option<[f64; 3]>)>,
    /// What the acting player looks at (`player_pov_hit`).
    pub pov: Option<([i32; 3], u8, [f64; 3])>,
    /// Collision boxes by position, where they aren't a full cube's (a flower's: none).
    pub collisions: HashMap<[i32; 3], Vec<[f64; 6]>>,
    /// Outline boxes by position, where they aren't the collision boxes (a flower's small box).
    pub outlines: HashMap<[i32; 3], Vec<[f64; 6]>>,
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
    /// What the fluid takes at a position from its neighbours (`new_liquid`), by position.
    pub new_liquids: HashMap<[i32; 3], (String, u8, bool)>,
    /// The exhaustion the acting player was given (`exhaust_player`), in all.
    pub exhaustion: f32,
    /// Mod payloads sent, in order: where to, channel, bytes.
    pub payloads: Vec<(PayloadTarget, String, Vec<u8>)>,
    /// The items the game knows; when empty, every id is one.
    pub items: Vec<String>,
    /// The sprites the game's atlas has (`block/stone`, `create:block/cogwheel`); when empty, every
    /// one ([`Host::has_sprite`]).
    pub sprites: Vec<String>,
    /// Items drawn: item, display context, pose, light.
    pub items_drawn: Vec<(String, u8, [f32; 16], u32)>,
    /// Items that place blocks.
    pub block_items: Vec<String>,
    /// Stack limits other than 64.
    pub stack_limits: HashMap<String, u32>,
    /// The level's item entities, in order.
    pub item_entities: Vec<ItemEntity>,
    /// The level's other entities ([`Host::entity`]); one killed or removed leaves the list.
    pub entities: Vec<EntityView>,
    /// The blows told with their sources ([`Host::hurt_entity_by`]), in order: entity, amount, source.
    pub blows: Vec<(i32, f32, HurtBy)>,
    /// The blows told to players ([`Host::hurt_player_by`]), in order: player, amount, source.
    pub player_blows: Vec<(u128, f32, HurtBy)>,
    /// The mobs' motions ([`Host::entity_motion`]): a living mob without one stands still.
    pub motions: HashMap<i32, [f64; 3]>,
    /// The players' motions ([`Host::player_motion`]), by UUID.
    pub player_motions: HashMap<u128, [f64; 3]>,
    /// The players whose motion was marked for their client ([`Host::mark_player_motion`]), in
    /// order.
    pub marked_players: Vec<u128>,
    /// The knockbacks given ([`Host::knockback_entity`]), in order: entity, strength, x, z.
    pub knockbacks: Vec<(i32, f64, f64, f64)>,
    /// The blows whose enchantment effects were asked for ([`Host::post_attack_effects`]), in order.
    pub post_attacks: Vec<(Struck, HurtBy, AttackWeapon)>,
    /// The entities added from their compound ([`Host::summon`]), in order: kind, position,
    /// compound. Each also joins [`MemoryHost::entities`] under the next id from 1000.
    pub summoned: Vec<(String, [f64; 3], std::collections::BTreeMap<String, Nbt>)>,
    /// The lightning struck ([`Host::strike_lightning`]), in order: where, visual only, cause.
    pub lightning: Vec<([f64; 3], bool, Option<u128>)>,
    /// The entities' saved compounds ([`Host::entity_nbt`]), by id.
    pub entity_nbts: HashMap<i32, std::collections::BTreeMap<String, Nbt>>,
    /// Entity type tags: tag → entity types.
    pub entity_tags: HashMap<String, Vec<String>>,
    /// The whole stacks of the item entities spawned with their data ([`Host::spawn_stack_entity`]), by id.
    pub item_entity_stacks: HashMap<u32, HandStack>,
    /// The clipboard's text ([`Host::clipboard`]).
    pub clipboard: String,
    /// Block facts by block (`minecraft:oak_planks`, any of its states).
    pub facts: HashMap<String, BlockFacts>,
    /// Blocks whose drops need the right tool ([`Host::block_requires_tool`]).
    pub requires_tool: Vec<String>,
    /// What each item's tool does to a block ([`Host::item_tool`]), by item and block; an item
    /// with any entry is a tool, with speed 1 and wrong for drops on another block.
    pub tools: HashMap<(String, String), ToolFacts>,
    /// Block sounds by block ([`Host::block_sounds`]).
    pub block_sounds: HashMap<String, BlockSounds>,
    /// What each block drops, whatever the tool ([`Host::block_drops`]): item and count.
    pub loot: HashMap<String, Vec<(String, u32)>>,
    /// The blocks' breaks told to the game afterwards ([`Host::spawn_after_break`]): where, the
    /// state and the tool's item.
    pub after_breaks: Vec<([i32; 3], String, Option<String>)>,
    /// Cracks shown ([`Host::destroy_block_progress`]): breaker, position, stage.
    pub cracks: Vec<(i32, [i32; 3], i32)>,
    /// The experience each block drops ([`Host::block_experience`]), by block.
    pub experience: HashMap<String, i32>,
    /// Blocks that are solid ([`Host::block_solid`]); any other block the host has facts for isn't.
    pub solid: Vec<String>,
    /// What the renderer knows of blocks' faces ([`Host::block_occlusion`]): whether each can hide
    /// its neighbours' faces and is an opaque full cube, by block (any of its states).
    pub occlusion: HashMap<String, (bool, bool)>,
    /// Occlusion shapes' faces ([`Host::face_occlusion`]), by state (as written, else its block)
    /// and direction ordinal.
    pub occlusion_faces: HashMap<(String, u8), Vec<[f64; 6]>>,
    /// The server's players and where they are.
    pub players: Vec<(u128, [f64; 3])>,
    /// The text drawn in the world ([`Host::world_text`]), in order.
    pub world_texts: Vec<WorldText>,
    /// The server's players' entity ids ([`Host::player_entities`]), by UUID.
    pub player_entity_ids: HashMap<u128, i32>,
    /// The game's damage types ([`Host::damage_types`]).
    pub damage_types: Vec<DamageTypeView>,
    /// A client's world: its runtime's hooks are a client's (`is_server` false).
    pub client: bool,
    /// On a client, the local player's entity id ([`Host::client_player_id`]).
    pub client_player_id: Option<i32>,
    /// On a client, its own player's walking input ([`Host::client_player_input`]).
    pub client_input: [f32; 2],
    /// On a client, whether its own player stands on something ([`Host::client_player_on_ground`]).
    pub client_on_ground: bool,
    /// Sounds played, in order.
    pub sounds: Vec<Sound>,
    /// Instances' sounds started ([`Host::start_sound`]), in order: their ids are their places
    /// here, from 1. Those still playing, where they are with their volume and pitch.
    pub started_sounds: Vec<InstanceSound>,
    pub playing_sounds: HashMap<u64, ([f64; 3], f32, f32)>,
    /// Particles made, in order.
    pub particles: Vec<Particles>,
    /// Faces the mods' own particles drew, in order.
    pub particle_quads: Vec<ParticleQuad>,
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
    /// The keys the player chose for mods' mappings on the bundle's page ([`Host::mod_key`]).
    pub mod_keys: HashMap<String, String>,
    /// A game screen over the world (the inventory, the pause menu); it or the mods' screen
    /// ([`MemoryHost::screen`]) means not [`Host::playing`].
    pub menu: bool,
    /// The pointer in GUI pixels.
    pub cursor: [f32; 2],
    /// A host lent only to read and draw ([`Host::read_only`]).
    pub read_only: bool,
    /// The time of day ([`Host::day_time`]).
    pub day_time: i64,
    /// Light by position, `(block, sky)`; unset positions are in open daylight.
    pub light: HashMap<[i32; 3], (u8, u8)>,
    /// The rain and thunder levels.
    pub weather: (f32, f32),
    /// The redstone signal reaching each position from its neighbours; unset ones get none.
    pub signals: HashMap<[i32; 3], i32>,
    /// The redstone signal each position sends back along a direction (the way from the block
    /// asking to it, `Direction`'s data value): its signal (`getSignal`) and its strong signal
    /// (`getDirectSignal`). Unset ones send none.
    pub sent_signals: HashMap<([i32; 3], u8), (i32, i32)>,
    /// Block ticks scheduled, in order: where, which block, in how many ticks.
    pub scheduled: Vec<([i32; 3], String, i32)>,
    /// The priorities of the ticks scheduled with one ([`Host::schedule_block_tick_at_priority`]),
    /// in order, by position.
    pub tick_priorities: Vec<([i32; 3], i32)>,
    /// The block ticks taken from the queue to run in the game tick under way
    /// ([`Host::block_tick_queued`] with `this_tick`).
    pub ticking_now: Vec<([i32; 3], String)>,
    /// Neighbour updates asked for, in order, with the block that changed ([`Host::update_neighbors`]).
    pub neighbor_updates: Vec<(NeighborUpdate, String)>,
    /// Resource files by path ([`Host::resource`]).
    pub resources: std::collections::BTreeMap<String, Vec<u8>>,
    /// Vanilla blocks' containers by position ([`Host::container`]).
    pub containers: HashMap<[i32; 3], MemoryContainer>,
    /// The server's advancements: each id with its criteria (name, trigger, conditions as JSON
    /// text), every one required ([`Host::mod_criteria`] lists the mods' triggers' ones).
    pub advancements: Vec<(String, Vec<(String, String, String)>)>,
    /// Criteria granted ([`Host::award_criterion`]), in order: player, advancement, criterion.
    pub granted: Vec<(u128, String, String)>,
    /// The criteria whose `player` predicate a player fails: player, advancement, criterion.
    pub player_fails: Vec<(u128, String, String)>,
    /// The players' statistics ([`Host::stat_value`], [`Host::set_stat`]): player, type, value,
    /// count. Only the players in `players` (or `player_id`'s) are reached.
    pub stats: Vec<(u128, String, String, i32)>,
    /// The scoreboard's changes asked for ([`Host::score_stat`]), in order.
    pub stat_scores: Vec<(u128, String, String, Option<i32>)>,
    /// The world's folder ([`Host::world_dir`]).
    pub world_dir: Option<std::path::PathBuf>,
}

/// A vanilla block's container in a [`MemoryHost`].
#[derive(Clone, Debug, Default)]
pub struct MemoryContainer {
    pub facts: ContainerFacts,
    /// Each slot's stack, `facts.size` of them.
    pub items: Vec<Option<HandStack>>,
    /// The slots each face reaches (by `Direction`'s data value); none: every slot, through any.
    pub faces: Option<[Vec<u32>; 6]>,
    /// Items no slot takes (`canPlaceItem`).
    pub refuses: Vec<String>,
    /// Items no slot takes through a face (`canPlaceItemThroughFace`).
    pub refuses_through: Vec<String>,
    /// The slots and items no face lets out (`canTakeItemThroughFace`).
    pub keeps: Vec<(u32, String)>,
    /// How often it changed ([`Host::container_changed`]).
    pub changed: u32,
    /// How often an item handler filled it while it was empty ([`Host::hopper_filled`]).
    pub filled: u32,
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
    fn day_time(&self) -> i64 {
        self.day_time
    }
    fn light(&self, pos: [i32; 3]) -> (u8, u8) {
        self.light.get(&pos).copied().unwrap_or((0, 15))
    }
    fn weather(&self) -> (f32, f32) {
        self.weather
    }
    fn neighbor_signal(&mut self, pos: [i32; 3]) -> i32 {
        if self.client { 0 } else { self.signals.get(&pos).copied().unwrap_or(0) }
    }
    fn signal(&mut self, pos: [i32; 3], direction: u8, direct: bool) -> i32 {
        let sent = if self.client { None } else { self.sent_signals.get(&(pos, direction)) };
        sent.map_or(0, |&(weak, strong)| if direct { strong } else { weak })
    }
    fn update_neighbors(&mut self, update: NeighborUpdate, block: &str) {
        if !self.client {
            self.neighbor_updates.push((update, block.to_owned()));
        }
    }
    fn schedule_block_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool {
        if self.client {
            return false;
        }
        self.scheduled.push((pos, block.to_owned(), delay));
        true
    }
    fn schedule_block_tick_at_priority(&mut self, pos: [i32; 3], block: &str, delay: i32, priority: i32) -> bool {
        let scheduled = self.schedule_block_tick(pos, block, delay);
        if scheduled {
            self.tick_priorities.push((pos, priority));
        }
        scheduled
    }
    fn block_tick_queued(&mut self, pos: [i32; 3], block: &str, this_tick: bool) -> bool {
        if this_tick {
            self.ticking_now.iter().any(|(p, b)| *p == pos && b == block)
        } else {
            self.scheduled.iter().any(|(p, b, _)| *p == pos && b == block)
        }
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
    /// Those set in `collisions`; else air (and unset positions) and fluids have none, any other
    /// block is a full cube.
    fn collision_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {
        if let Some(boxes) = self.collisions.get(&pos) {
            return Some(boxes.clone());
        }
        let none = self.blocks.get(&pos).is_none_or(|b| {
            let block = b.split('[').next().unwrap_or("");
            matches!(block, "minecraft:air" | "minecraft:water" | "minecraft:lava")
        });
        Some(if none { Vec::new() } else { vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]] })
    }
    /// Those set in `outlines`; else the collision boxes.
    fn outline_boxes(&self, pos: [i32; 3]) -> Option<Vec<[f64; 6]>> {
        self.outlines.get(&pos).cloned().or_else(|| self.collision_boxes(pos))
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
    fn tell_player(&mut self, uuid: u128, component: &str, overlay: bool) -> bool {
        let on = self.players.iter().any(|(id, _)| *id == uuid);
        if on {
            self.told.push((uuid, component.to_owned(), overlay));
        }
        on
    }
    fn attribute(&mut self, entity: Option<i32>, attribute: &str) -> Option<AttributeView> {
        self.attributes.get(&(entity, attribute.to_owned())).cloned()
    }
    fn change_attribute(&mut self, entity: Option<i32>, attribute: &str, change: &AttributeChange) -> bool {
        self.attributes.get_mut(&(entity, attribute.to_owned())).is_some_and(|a| a.apply(change))
    }
    fn effects(&mut self, entity: Option<i32>) -> Option<Vec<EffectView>> {
        self.effects.get(&entity).cloned()
    }
    fn add_effect(&mut self, entity: Option<i32>, effect: &EffectView, force: bool) -> bool {
        let Some(effects) = self.effects.get_mut(&entity) else {
            return false;
        };
        match effects.iter_mut().find(|e| e.id == effect.id) {
            Some(there) if force => {
                *there = effect.clone();
                true
            }
            Some(there) => there.update(effect),
            None => {
                effects.push(effect.clone());
                true
            }
        }
    }
    fn remove_effect(&mut self, entity: Option<i32>, effect: &str) -> bool {
        let Some(effects) = self.effects.get_mut(&entity) else {
            return false;
        };
        let before = effects.len();
        effects.retain(|e| e.id != effect);
        effects.len() != before
    }
    fn player_vitals(&mut self) -> Option<VitalsView> {
        self.vitals
    }
    fn set_player_vitals(&mut self, to: &VitalsView) -> bool {
        self.vitals.as_mut().map(|v| *v = *to).is_some()
    }
    fn level_event(&mut self, event: i32, pos: [i32; 3], data: i32) -> bool {
        self.level_events.push((event, pos, data));
        true
    }
    fn state_id(&self, state: &str) -> Option<i32> {
        self.state_ids.iter().position(|s| s == state).and_then(|i| i32::try_from(i).ok())
    }
    fn state_of_id(&self, id: i32) -> Option<String> {
        if self.state_ids.is_empty() {
            return None;
        }
        let i = usize::try_from(id).ok().filter(|&i| i < self.state_ids.len());
        Some(i.map_or_else(|| "minecraft:air".to_owned(), |i| self.state_ids[i].clone()))
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
    fn player_id(&self) -> Option<u128> {
        self.player_id
    }
    fn player_selected(&self) -> u8 {
        self.selected
    }
    fn player_inventory(&self) -> bool {
        self.inventory
    }
    fn player_slot(&self, slot: u8) -> Option<HandStack> {
        match slot {
            OFF_HAND_SLOT => self.player_hand(1),
            36..=39 => self.player_worn(39 - slot),
            s if s == self.selected => self.player_hand(0),
            s => {
                let mut stack = self.slots.get(&s).cloned()?;
                stack.max_damage = self.max_damage.get(&stack.item).copied().unwrap_or(0);
                Some(stack)
            }
        }
    }
    fn set_player_slot(&mut self, slot: u8, stack: Option<&HandStack>) -> bool {
        let stack = stack.filter(|s| !s.item.is_empty() && s.count > 0);
        match slot {
            OFF_HAND_SLOT => self.set_player_hand(1, stack),
            36..=39 => self.worn[usize::from(39 - slot)] = stack.cloned(),
            s if s == self.selected => self.set_player_hand(0, stack),
            s if s < 36 => match stack {
                Some(st) => {
                    self.slots.insert(s, HandStack { max_damage: 0, ..st.clone() });
                }
                None => {
                    self.slots.remove(&s);
                }
            },
            _ => return false,
        }
        true
    }
    fn update_player_slot(&mut self, slot: u8, count: u32, damage: u32, components: &[(String, Option<Nbt>)]) -> bool {
        let Some(mut s) = self.player_slot(slot) else { return false };
        s.count = count;
        s.damage = damage;
        for (name, value) in components {
            s.components.retain(|(k, _)| k != name);
            if let Some(v) = value {
                s.components.push((name.clone(), v.clone()));
            }
        }
        self.set_player_slot(slot, Some(&s))
    }
    fn player_selected_of(&mut self, uuid: u128) -> Option<u8> {
        self.others.contains_key(&uuid).then_some(0)
    }
    fn player_slot_of(&mut self, uuid: u128, slot: u8) -> Option<Option<HandStack>> {
        let stack = self.others.get(&uuid)?.get(&slot).cloned();
        Some(stack.map(|s| HandStack { max_damage: self.max_damage.get(&s.item).copied().unwrap_or(0), ..s }))
    }
    fn set_player_slot_of(&mut self, uuid: u128, slot: u8, stack: Option<&HandStack>) -> bool {
        let Some(slots) = self.others.get_mut(&uuid).filter(|_| slot <= OFF_HAND_SLOT) else {
            return false;
        };
        match stack.filter(|s| !s.item.is_empty() && s.count > 0) {
            Some(s) => slots.insert(slot, HandStack { max_damage: 0, ..s.clone() }),
            None => slots.remove(&slot),
        };
        true
    }
    fn update_player_slot_of(&mut self, uuid: u128, slot: u8, count: u32, damage: u32, components: &[(String, Option<Nbt>)]) -> bool {
        let Some(Some(mut s)) = self.player_slot_of(uuid, slot) else { return false };
        s.count = count;
        s.damage = damage;
        for (name, value) in components {
            s.components.retain(|(k, _)| k != name);
            if let Some(v) = value {
                s.components.push((name.clone(), v.clone()));
            }
        }
        self.set_player_slot_of(uuid, slot, Some(&s))
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
    fn has_sprite(&self, name: &str) -> bool {
        let name = name.trim_start_matches("minecraft:");
        self.sprites.is_empty() || self.sprites.iter().any(|s| s.trim_start_matches("minecraft:") == name)
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
    fn item_max_damage(&self, item: &str) -> Option<u32> {
        self.max_damage.get(item).copied()
    }
    fn item_equip_slot(&self, item: &str) -> Option<String> {
        self.equip_slots.get(item).cloned()
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
    fn entity_type_in_tag(&self, kind: &str, tag: &str) -> bool {
        self.entity_tags.get(tag).is_some_and(|k| k.iter().any(|x| x == kind))
    }
    fn fluid_flow(&self, pos: [i32; 3]) -> [f64; 3] {
        self.flows.get(&pos).copied().unwrap_or_default()
    }
    fn new_liquid(&mut self, pos: [i32; 3], _fluid: &str, _state: &str) -> Option<(String, u8, bool)> {
        self.new_liquids.get(&pos).cloned()
    }
    fn container(&self, pos: [i32; 3]) -> Option<ContainerFacts> {
        self.containers.get(&pos).map(|c| c.facts)
    }
    fn container_item(&self, pos: [i32; 3], slot: u32) -> Option<HandStack> {
        self.containers.get(&pos)?.items.get(slot as usize)?.clone()
    }
    fn set_container_item(&mut self, pos: [i32; 3], slot: u32, stack: Option<&HandStack>) -> bool {
        let Some(s) = self.containers.get_mut(&pos).and_then(|c| c.items.get_mut(slot as usize)) else { return false };
        *s = stack.filter(|h| h.count > 0).cloned();
        true
    }
    fn container_changed(&mut self, pos: [i32; 3]) {
        if let Some(c) = self.containers.get_mut(&pos) {
            c.changed += 1;
        }
    }
    fn container_can_place(&self, pos: [i32; 3], _slot: u32, stack: &HandStack) -> bool {
        self.containers.get(&pos).is_some_and(|c| !c.refuses.contains(&stack.item))
    }
    fn container_can_place_through(&self, pos: [i32; 3], _slot: u32, stack: &HandStack) -> bool {
        self.containers.get(&pos).is_some_and(|c| !c.refuses_through.contains(&stack.item))
    }
    fn container_face_slots(&self, pos: [i32; 3], face: u8) -> Vec<u32> {
        match self.containers.get(&pos) {
            Some(MemoryContainer { faces: Some(f), .. }) => f.get(face as usize).cloned().unwrap_or_default(),
            Some(c) => (0..c.facts.size).collect(),
            None => Vec::new(),
        }
    }
    fn container_can_take(&self, pos: [i32; 3], slot: u32, stack: &HandStack, _face: u8) -> bool {
        self.containers.get(&pos).is_some_and(|c| !c.keeps.iter().any(|(s, i)| *s == slot && *i == stack.item))
    }
    fn hopper_filled(&mut self, pos: [i32; 3]) {
        if let Some(c) = self.containers.get_mut(&pos) {
            c.filled += 1;
        }
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
    fn player_entities(&self) -> Vec<(i32, u128, [f64; 3])> {
        self.players.iter().filter_map(|&(u, p)| Some((*self.player_entity_ids.get(&u)?, u, p))).collect()
    }
    fn client_player_id(&self) -> Option<i32> {
        self.client_player_id
    }
    fn damage_types(&self) -> Vec<DamageTypeView> {
        self.damage_types.clone()
    }
    fn item_entities(&self, bounds: [f64; 6]) -> Vec<ItemEntity> {
        self.item_entities.iter().filter(|e| e.count > 0 && boxes_meet(&e.bounds(), &bounds)).cloned().collect()
    }
    /// Those of `entities` standing in `bounds`, each taken as its feet.
    fn entities_in(&self, bounds: [f64; 6]) -> Vec<EntityView> {
        self.entities.iter().filter(|e| (0..3).all(|i| bounds[i] <= e.pos[i] && e.pos[i] <= bounds[i + 3])).cloned().collect()
    }
    fn entity(&self, id: i32) -> Option<EntityView> {
        self.entities.iter().find(|e| e.id == id).cloned()
    }
    fn hurt_entity(&mut self, id: i32, amount: f32) -> bool {
        let Some(e) = self.entities.iter_mut().find(|e| e.id == id && e.health.is_some()) else {
            return false;
        };
        let (health, max) = e.health.unwrap_or_default();
        if amount <= 0.0 {
            return false;
        }
        if health - amount > 0.0 {
            e.health = Some((health - amount, max));
        } else {
            self.entities.retain(|e| e.id != id);
        }
        true
    }
    fn hurt_entity_by(&mut self, id: i32, amount: f32, by: &HurtBy) -> bool {
        self.blows.push((id, amount, by.clone()));
        self.hurt_entity(id, amount)
    }
    fn hurt_player_by(&mut self, uuid: u128, amount: f32, by: &HurtBy) -> bool {
        self.player_blows.push((uuid, amount, by.clone()));
        amount > 0.0
    }
    fn knockback_entity(&mut self, id: i32, strength: f64, x: f64, z: f64) -> bool {
        let living = self.entities.iter().any(|e| e.id == id && e.health.is_some());
        if living {
            self.knockbacks.push((id, strength, x, z));
        }
        living
    }
    fn post_attack_effects(&mut self, victim: Struck, by: &HurtBy, weapon: &AttackWeapon) -> bool {
        self.post_attacks.push((victim, by.clone(), weapon.clone()));
        true
    }
    fn summon(&mut self, kind: &str, pos: [f64; 3], nbt: &std::collections::BTreeMap<String, Nbt>) -> Option<i32> {
        let id = 1000 + i32::try_from(self.summoned.len() + self.lightning.len()).ok()?;
        self.summoned.push((kind.to_owned(), pos, nbt.clone()));
        self.entities.push(EntityView { id, kind: kind.to_owned(), pos, ..EntityView::default() });
        Some(id)
    }
    fn strike_lightning(&mut self, pos: [f64; 3], visual_only: bool, cause: Option<u128>) -> Option<i32> {
        let id = 1000 + i32::try_from(self.summoned.len() + self.lightning.len()).ok()?;
        self.lightning.push((pos, visual_only, cause));
        self.entities.push(EntityView { id, kind: "minecraft:lightning_bolt".into(), pos, ..EntityView::default() });
        Some(id)
    }
    fn entity_nbt(&mut self, id: i32) -> Option<std::collections::BTreeMap<String, Nbt>> {
        self.entity_nbts.get(&id).cloned()
    }
    fn kill_entity(&mut self, id: i32) -> bool {
        let had = self.entities.len();
        self.entities.retain(|e| e.id != id);
        self.entities.len() != had
    }
    fn remove_entity(&mut self, id: i32) -> bool {
        self.kill_entity(id)
    }
    fn set_entity_health(&mut self, id: i32, health: f32) -> bool {
        match self.entities.iter_mut().find(|e| e.id == id) {
            Some(EntityView { health: Some((h, max)), .. }) if health > 0.0 => {
                *h = health.min(*max);
                true
            }
            _ => false,
        }
    }
    fn push_entity(&mut self, id: i32, velocity: [f64; 3]) -> bool {
        let Some(m) = self.entity_motion(id) else {
            return false;
        };
        self.motions.insert(
            id,
            [m[0] + velocity[0], m[1] + velocity[1], m[2] + velocity[2]],
        );
        true
    }
    fn entity_motion(&self, id: i32) -> Option<[f64; 3]> {
        let living = self
            .entities
            .iter()
            .any(|e| e.id == id && e.health.is_some());
        living.then(|| self.motions.get(&id).copied().unwrap_or_default())
    }
    fn set_entity_motion(&mut self, id: i32, motion: [f64; 3]) -> bool {
        let living = self.entity_motion(id).is_some();
        if living {
            self.motions.insert(id, motion);
        }
        living
    }
    fn player_motion(&mut self, uuid: u128) -> Option<[f64; 3]> {
        let known = self.player_id == Some(uuid) || self.players.iter().any(|p| p.0 == uuid);
        known.then(|| self.player_motions.get(&uuid).copied().unwrap_or_default())
    }
    fn set_player_motion(&mut self, uuid: u128, motion: [f64; 3]) -> bool {
        let known = self.player_motion(uuid).is_some();
        if known {
            self.player_motions.insert(uuid, motion);
        }
        known
    }
    fn teleport_entity(&mut self, id: i32, pos: [f64; 3]) -> bool {
        match self.entities.iter_mut().find(|e| e.id == id && e.health.is_some()) {
            Some(e) => {
                e.pos = pos;
                true
            }
            None => false,
        }
    }
    fn turn_entity(&mut self, id: i32, turn: &Turn) -> bool {
        let angles = [turn.yaw, turn.pitch, turn.head_yaw, turn.body_yaw];
        let Some(e) = self
            .entities
            .iter_mut()
            .find(|e| e.id == id && e.health.is_some())
        else {
            return false;
        };
        if angles.iter().flatten().any(|a| !a.is_finite()) {
            return false;
        }
        e.yaw = turn.yaw.unwrap_or(e.yaw);
        e.pitch = turn
            .pitch
            .map_or(e.pitch, |p| (p % 360.0).clamp(-90.0, 90.0));
        e.head_yaw = turn.head_yaw.unwrap_or(e.head_yaw);
        e.body_yaw = turn.body_yaw.unwrap_or(e.body_yaw);
        true
    }
    // A client's own player looks as `place` says.
    fn turn_client_player(&mut self, yaw: Option<f32>, pitch: Option<f32>) -> bool {
        let Some((_, y, p)) = self.place.as_mut().filter(|_| self.client) else {
            return false;
        };
        if [yaw, pitch].iter().flatten().any(|a| !a.is_finite()) {
            return false;
        }
        *y = yaw.unwrap_or(*y);
        *p = pitch.map_or(*p, |a| (a % 360.0).clamp(-90.0, 90.0));
        true
    }
    // No blocks stop a move here.
    fn move_entity(&mut self, id: i32, movement: [f64; 3]) -> Option<[f64; 3]> {
        let e = self
            .entities
            .iter_mut()
            .find(|e| e.id == id && e.health.is_some())?;
        e.pos = [
            e.pos[0] + movement[0],
            e.pos[1] + movement[1],
            e.pos[2] + movement[2],
        ];
        Some(e.pos)
    }
    fn mark_player_motion(&mut self, uuid: u128) -> bool {
        let known = self.player_motion(uuid).is_some();
        if known {
            self.marked_players.push(uuid);
        }
        known
    }
    // A client's own player stands at `place`; no blocks stop a move here.
    fn move_client_player(&mut self, movement: [f64; 3]) -> bool {
        if !self.client {
            return false;
        }
        let Some((at, ..)) = self.place.as_mut() else {
            return false;
        };
        for (a, m) in at.iter_mut().zip(movement) {
            *a += m;
        }
        true
    }
    fn client_player_input(&self) -> Option<[f32; 2]> {
        self.client.then_some(self.client_input)
    }
    fn client_player_on_ground(&self) -> Option<bool> {
        self.client.then_some(self.client_on_ground)
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
    fn clipboard(&self) -> String {
        self.clipboard.clone()
    }
    fn set_clipboard(&mut self, text: &str) {
        if !text.is_empty() && !self.read_only {
            self.clipboard = text.to_owned();
        }
    }
    fn spawn_stack_entity(&mut self, stack: &HandStack, pos: [f64; 3], motion: [f64; 3], pickup_delay: u32) -> Option<u32> {
        let id = self.spawn_item_entity(&stack.item, stack.count, pos, motion, pickup_delay)?;
        self.item_entity_stacks.insert(id, stack.clone());
        Some(id)
    }
    /// A block item's block, named as the item, into the clicked block when it is air or else the
    /// one beside the face, when that one is air.
    fn place_item_block(&mut self, stack: &HandStack, pos: [i32; 3], face: u8, _hit: [f64; 3], _inside: bool, _look: (f32, f32), _sneaking: bool) -> Option<[i32; 3]> {
        if self.read_only || !self.item_is_block(&stack.item) {
            return None;
        }
        let air = |h: &Self, p: [i32; 3]| h.blocks.get(&p).is_none_or(|b| b == "minecraft:air");
        const STEPS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
        let s = STEPS[usize::from(face.min(5))];
        let at = if air(self, pos) { pos } else { [pos[0] + s[0], pos[1] + s[1], pos[2] + s[2]] };
        if !air(self, at) {
            return None;
        }
        self.blocks.insert(at, stack.item.clone());
        Some(at)
    }
    fn block_requires_tool(&self, state: &str) -> bool {
        self.requires_tool.iter().any(|b| b == state.split('[').next().unwrap_or(state))
    }
    fn item_tool(&self, item: &str, state: &str) -> Option<ToolFacts> {
        let block = state.split('[').next().unwrap_or(state);
        if let Some(t) = self.tools.get(&(item.to_owned(), block.to_owned())) {
            return Some(*t);
        }
        let wears = self.tools.iter().find(|((i, _), _)| i == item).map(|(_, t)| t.damage_per_block)?;
        Some(ToolFacts { speed: 1.0, correct: false, damage_per_block: wears })
    }
    fn block_sounds(&self, state: &str) -> Option<BlockSounds> {
        self.block_sounds.get(state.split('[').next().unwrap_or(state)).cloned()
    }
    fn block_drops(&mut self, _pos: [i32; 3], state: &str, _tool: Option<&HandStack>) -> Vec<HandStack> {
        let block = state.split('[').next().unwrap_or(state);
        self.loot.get(block).map_or_else(Vec::new, |drops| {
            drops.iter().map(|(item, count)| HandStack { item: item.clone(), count: *count, ..Default::default() }).collect()
        })
    }
    fn spawn_after_break(&mut self, pos: [i32; 3], state: &str, tool: Option<&HandStack>) {
        if !self.read_only {
            self.after_breaks.push((pos, state.to_owned(), tool.map(|t| t.item.clone())));
        }
    }
    fn destroy_block_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {
        if !self.read_only {
            self.cracks.push((breaker, pos, progress));
        }
    }
    fn block_experience(&mut self, state: &str) -> i32 {
        self.experience.get(state.split('[').next().unwrap_or(state)).copied().unwrap_or(0)
    }
    fn pop_experience(&mut self, pos: [i32; 3], amount: i32) {
        if !self.read_only && amount > 0 {
            let at = pos.map(|c| f64::from(c) + 0.5);
            self.orbs.push((at, amount as u32, None));
        }
    }
    fn block_solid(&self, state: &str) -> Option<bool> {
        let block = state.split('[').next().unwrap_or(state);
        let known = self.facts.contains_key(block) || self.solid.iter().any(|b| b == block);
        known.then(|| self.solid.iter().any(|b| b == block))
    }
    fn block_occlusion(&self, state: &str) -> Option<(bool, bool)> {
        self.occlusion.get(state.split('[').next().unwrap_or(state)).copied()
    }
    fn face_occlusion(&self, state: &str, direction: u8) -> Option<Vec<[f64; 6]>> {
        let block = state.split('[').next().unwrap_or(state);
        let faces = &self.occlusion_faces;
        faces.get(&(state.to_owned(), direction)).or_else(|| faces.get(&(block.to_owned(), direction))).cloned()
    }
    fn play_sound(&mut self, sound: &Sound) {
        if !self.read_only {
            self.sounds.push(sound.clone());
        }
    }
    fn start_sound(&mut self, sound: &InstanceSound) -> Option<u64> {
        if self.read_only {
            return None;
        }
        self.started_sounds.push(sound.clone());
        let id = self.started_sounds.len() as u64;
        self.playing_sounds.insert(id, (sound.sound.at, sound.sound.volume, sound.sound.pitch));
        Some(id)
    }
    fn update_sound(&mut self, id: u64, at: [f64; 3], volume: f32, pitch: f32) -> bool {
        match self.playing_sounds.get_mut(&id) {
            Some(s) => {
                *s = (at, volume, pitch);
                true
            }
            None => false,
        }
    }
    fn stop_sound(&mut self, id: u64) {
        self.playing_sounds.remove(&id);
    }
    fn particles(&mut self, particles: &Particles) {
        self.particles.push(particles.clone());
    }
    fn particle_quad(&mut self, quad: &ParticleQuad) -> bool {
        self.particle_quads.push(quad.clone());
        true
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
    fn world_text(&mut self, text: &WorldText) -> bool {
        self.world_texts.push(text.clone());
        true
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
    fn mod_key(&self, name: &str, default: &str) -> String {
        self.mod_keys.get(name).cloned().unwrap_or_else(|| default.to_owned())
    }
    fn playing(&self) -> bool {
        !self.screen && !self.menu
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
    fn resource(&self, path: &str) -> Option<Vec<u8>> {
        self.resources.get(path).cloned()
    }
    fn resource_paths(&self, prefix: &str) -> Vec<String> {
        let folder = format!("{}/", prefix.trim_end_matches('/'));
        self.resources.keys().filter(|p| p.starts_with(&folder)).cloned().collect()
    }
    fn resource_namespaces(&self, root: &str) -> Vec<String> {
        let folder = format!("{root}/");
        let mut out: Vec<String> = self.resources.keys().filter_map(|p| p.strip_prefix(&folder)?.split_once('/').map(|(ns, _)| ns.to_owned())).collect();
        out.dedup();
        out
    }
    fn mod_criteria(&mut self) -> Option<Vec<ModCriterion>> {
        let mods = |trigger: &str| {
            trigger
                .split_once(':')
                .is_some_and(|(ns, _)| ns != "minecraft")
        };
        let list = self.advancements.iter().flat_map(|(id, criteria)| {
            criteria.iter().filter(|(_, trigger, _)| mods(trigger)).map(
                move |(name, trigger, conditions)| ModCriterion {
                    advancement: id.clone(),
                    criterion: name.clone(),
                    trigger: trigger.clone(),
                    conditions: conditions.clone(),
                },
            )
        });
        (!self.client).then(|| list.collect())
    }
    fn has_advancement(&mut self, id: &str) -> bool {
        self.advancements.iter().any(|(a, _)| a == id)
    }
    fn advancement_progress(&mut self, player: u128, id: &str) -> Option<AdvancementProgressView> {
        let (_, criteria) = self.advancements.iter().find(|(a, _)| a == id)?;
        let has = |name: &str| {
            self.granted
                .iter()
                .any(|(p, a, c)| *p == player && a == id && c == name)
        };
        let criteria: Vec<(String, bool)> = criteria
            .iter()
            .map(|(name, _, _)| (name.clone(), has(name)))
            .collect();
        let done = !criteria.is_empty() && criteria.iter().all(|(_, granted)| *granted);
        Some(AdvancementProgressView { criteria, done })
    }
    fn award_criterion(&mut self, player: u128, advancement: &str, criterion: &str) -> bool {
        let known = self.advancements.iter().any(|(a, criteria)| {
            a == advancement && criteria.iter().any(|(n, _, _)| n == criterion)
        });
        let had = self
            .granted
            .iter()
            .any(|(p, a, c)| *p == player && a == advancement && c == criterion);
        if !known || had {
            return false;
        }
        self.granted
            .push((player, advancement.to_owned(), criterion.to_owned()));
        true
    }
    fn criterion_player_holds(&mut self, player: u128, advancement: &str, criterion: &str) -> bool {
        !self
            .player_fails
            .iter()
            .any(|(p, a, c)| *p == player && a == advancement && c == criterion)
    }
    fn stat_value(&mut self, player: u128, ty: &str, key: &str) -> Option<i32> {
        if !self.reaches(player) {
            return None;
        }
        let found = self
            .stats
            .iter()
            .find(|(p, t, k, _)| *p == player && t == ty && k == key);
        Some(found.map_or(0, |s| s.3))
    }
    fn set_stat(&mut self, player: u128, ty: &str, key: &str, value: i32) -> bool {
        if !self.reaches(player) {
            return false;
        }
        match self
            .stats
            .iter_mut()
            .find(|(p, t, k, _)| *p == player && t == ty && k == key)
        {
            Some(s) => s.3 = value,
            None => self
                .stats
                .push((player, ty.to_owned(), key.to_owned(), value)),
        }
        true
    }
    fn score_stat(&mut self, player: u128, ty: &str, key: &str, change: Option<i32>) -> bool {
        if !self.reaches(player) {
            return false;
        }
        self.stat_scores
            .push((player, ty.to_owned(), key.to_owned(), change));
        true
    }
    fn world_dir(&self) -> Option<std::path::PathBuf> {
        self.world_dir.clone()
    }
}

impl MemoryHost {
    /// Whether `player` is one of this host's players: the acting one, or one it lists.
    fn reaches(&self, player: u128) -> bool {
        self.player_id == Some(player) || self.players.iter().any(|(p, _)| *p == player)
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
