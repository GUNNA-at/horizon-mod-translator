//! A mod's registered blocks and items as content declarations (the game's `native_content`).
//! A block's constructor records what vanilla's `Block` constructor collects: the
//! `BlockBehaviour.Properties` it was given (strength, sounds, light, map colour, occlusion), the
//! state properties its `createBlockStateDefinition` declares and its default state (each
//! property's first value, then what `registerDefaultState` sets). An item's constructor records its
//! `Item.Properties`.
use crate::host::BlockFacts;
use crate::{api, api_common, jutil, Data, Rt, V};

pub const BLOCK_PROPERTIES: &str = "net/minecraft/world/level/block/state/BlockBehaviour$Properties";
pub const ITEM_PROPERTIES: &str = "net/minecraft/world/item/Item$Properties";
const SOUND_TYPE: &str = "net/minecraft/world/level/block/SoundType";
const MAP_COLOR: &str = "net/minecraft/world/level/material/MapColor";
const DYE_COLOR: &str = "net/minecraft/world/item/DyeColor";
const RARITY: &str = "net/minecraft/world/item/Rarity";
const BUILDER: &str = "net/minecraft/world/level/block/state/StateDefinition$Builder";
const DEFINE: &str = "createBlockStateDefinition.(Lnet/minecraft/world/level/block/state/StateDefinition$Builder;)V";
const DEFINE_KEY: &str = "net/minecraft/world/level/block/Block.createBlockStateDefinition.(Lnet/minecraft/world/level/block/state/StateDefinition$Builder;)V";
const GET_SHAPE: &str = "getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
const GET_SHAPE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;";
/// States a block's shapes are read for, at most; past that its template's shapes stay.
const MAX_SHAPE_STATES: usize = 4096;
/// Boxes in a declared shape, as content allows (`horizon_mods::content::MAX_SHAPE_BOXES`).
const MAX_SHAPE_BOXES: usize = 64;
const PREDICATE: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const TO_INT: &str = "java/util/function/ToIntFunction.applyAsInt.(Ljava/lang/Object;)I";
const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";

/// `MapColor`'s constants by id, as vanilla numbers them.
const MAP_COLORS: [&str; 62] = [
    "NONE", "GRASS", "SAND", "WOOL", "FIRE", "ICE", "METAL", "PLANT", "SNOW", "CLAY", "DIRT", "STONE", "WATER", "WOOD", "QUARTZ",
    "COLOR_ORANGE", "COLOR_MAGENTA", "COLOR_LIGHT_BLUE", "COLOR_YELLOW", "COLOR_LIGHT_GREEN", "COLOR_PINK", "COLOR_GRAY",
    "COLOR_LIGHT_GRAY", "COLOR_CYAN", "COLOR_PURPLE", "COLOR_BLUE", "COLOR_BROWN", "COLOR_GREEN", "COLOR_RED", "COLOR_BLACK", "GOLD",
    "DIAMOND", "LAPIS", "EMERALD", "PODZOL", "NETHER", "TERRACOTTA_WHITE", "TERRACOTTA_ORANGE", "TERRACOTTA_MAGENTA",
    "TERRACOTTA_LIGHT_BLUE", "TERRACOTTA_YELLOW", "TERRACOTTA_LIGHT_GREEN", "TERRACOTTA_PINK", "TERRACOTTA_GRAY",
    "TERRACOTTA_LIGHT_GRAY", "TERRACOTTA_CYAN", "TERRACOTTA_PURPLE", "TERRACOTTA_BLUE", "TERRACOTTA_BROWN", "TERRACOTTA_GREEN",
    "TERRACOTTA_RED", "TERRACOTTA_BLACK", "CRIMSON_NYLIUM", "CRIMSON_STEM", "CRIMSON_HYPHAE", "WARPED_NYLIUM", "WARPED_STEM",
    "WARPED_HYPHAE", "WARPED_WART_BLOCK", "DEEPSLATE", "RAW_IRON", "GLOW_LICHEN",
];

/// `DyeColor`'s constants in order, with the map colour each one's blocks take.
const DYES: [(&str, u8); 16] = [
    ("WHITE", 8), ("ORANGE", 15), ("MAGENTA", 16), ("LIGHT_BLUE", 17), ("YELLOW", 18), ("LIME", 19), ("PINK", 20), ("GRAY", 21),
    ("LIGHT_GRAY", 22), ("CYAN", 23), ("PURPLE", 24), ("BLUE", 25), ("BROWN", 26), ("GREEN", 27), ("RED", 28), ("BLACK", 29),
];

/// `SoundType` constants and a vanilla block that sounds the same, for a declaration's `sounds`.
const SOUNDS: [(&str, &str); 30] = [
    ("WOOD", "minecraft:oak_planks"), ("STONE", "minecraft:stone"), ("METAL", "minecraft:iron_block"), ("GLASS", "minecraft:glass"),
    ("WOOL", "minecraft:white_wool"), ("SAND", "minecraft:sand"), ("GRAVEL", "minecraft:gravel"), ("GRASS", "minecraft:grass_block"),
    ("SNOW", "minecraft:snow_block"), ("LADDER", "minecraft:ladder"), ("ANVIL", "minecraft:anvil"), ("SLIME_BLOCK", "minecraft:slime_block"),
    ("HONEY_BLOCK", "minecraft:honey_block"), ("NETHERITE_BLOCK", "minecraft:netherite_block"), ("COPPER", "minecraft:copper_block"),
    ("LANTERN", "minecraft:lantern"), ("NETHER_BRICKS", "minecraft:nether_bricks"), ("DEEPSLATE", "minecraft:deepslate"),
    ("POLISHED_DEEPSLATE", "minecraft:polished_deepslate"), ("DEEPSLATE_BRICKS", "minecraft:deepslate_bricks"),
    ("DEEPSLATE_TILES", "minecraft:deepslate_tiles"), ("TUFF", "minecraft:tuff"), ("CALCITE", "minecraft:calcite"),
    ("AMETHYST", "minecraft:amethyst_block"), ("BASALT", "minecraft:basalt"), ("BONE_BLOCK", "minecraft:bone_block"),
    ("SCAFFOLDING", "minecraft:scaffolding"), ("NETHERRACK", "minecraft:netherrack"), ("SOUL_SAND", "minecraft:soul_sand"),
    ("MUD_BRICKS", "minecraft:mud_bricks"),
];

/// Vanilla blocks mods copy properties from (`Properties.ofFullCopy`): destroy time, explosion
/// resistance, whether drops need the right tool, map colour.
const COPIED: [(&str, f32, f32, bool, u8); 14] = [
    ("minecraft:stone", 1.5, 6.0, true, 11),
    ("minecraft:andesite", 1.5, 6.0, true, 11),
    ("minecraft:granite", 1.5, 6.0, true, 10),
    ("minecraft:diorite", 1.5, 6.0, true, 14),
    ("minecraft:deepslate", 3.0, 6.0, true, 59),
    ("minecraft:iron_block", 5.0, 6.0, true, 6),
    ("minecraft:gold_block", 3.0, 6.0, true, 30),
    ("minecraft:copper_block", 3.0, 6.0, true, 15),
    ("minecraft:netherite_block", 50.0, 1200.0, true, 29),
    ("minecraft:oak_planks", 2.0, 3.0, false, 13),
    ("minecraft:spruce_planks", 2.0, 3.0, false, 34),
    ("minecraft:stripped_spruce_wood", 2.0, 2.0, false, 34),
    ("minecraft:glass", 0.3, 0.3, false, 0),
    ("minecraft:white_wool", 0.8, 0.8, false, 8),
];

/// The vanilla block a mod block copies when it extends one of these classes: its states, shapes
/// and behaviour (a door opens). The rest copy a property-less block.
const TEMPLATES: [(&str, &str); 7] = [
    ("DoorBlock", "minecraft:oak_door"),
    ("TrapDoorBlock", "minecraft:oak_trapdoor"),
    ("SlabBlock", "minecraft:smooth_stone_slab"),
    ("StairBlock", "minecraft:stone_stairs"),
    ("LadderBlock", "minecraft:ladder"),
    ("RotatedPillarBlock", "minecraft:quartz_pillar"),
    ("IronBarsBlock", "minecraft:iron_bars"),
];

/// The properties a vanilla block class adds in its own `createBlockStateDefinition`
/// (`BlockStateProperties` fields), for a mod class that extends it.
fn vanilla_definition(class: &str) -> &'static [&'static str] {
    match class.rsplit('/').next().unwrap_or(class) {
        "RotatedPillarBlock" => &["AXIS"],
        "SlabBlock" => &["SLAB_TYPE", "WATERLOGGED"],
        "StairBlock" | "WeatheringCopperStairBlock" => &["HORIZONTAL_FACING", "HALF", "STAIRS_SHAPE", "WATERLOGGED"],
        "DoorBlock" => &["HORIZONTAL_FACING", "OPEN", "DOOR_HINGE", "POWERED", "DOUBLE_BLOCK_HALF"],
        "TrapDoorBlock" => &["HORIZONTAL_FACING", "OPEN", "HALF", "POWERED", "WATERLOGGED"],
        "LadderBlock" => &["HORIZONTAL_FACING", "WATERLOGGED"],
        "IronBarsBlock" | "FenceBlock" => &["NORTH", "EAST", "WEST", "SOUTH", "WATERLOGGED"],
        "WallBlock" => &["UP", "NORTH_WALL", "EAST_WALL", "WEST_WALL", "SOUTH_WALL", "WATERLOGGED"],
        "FenceGateBlock" => &["HORIZONTAL_FACING", "OPEN", "POWERED", "IN_WALL"],
        "ButtonBlock" => &["HORIZONTAL_FACING", "POWERED", "ATTACH_FACE"],
        "LeverBlock" => &["ATTACH_FACE", "HORIZONTAL_FACING", "POWERED"],
        "PressurePlateBlock" => &["POWERED"],
        "WeightedPressurePlateBlock" => &["POWER"],
        "ChainBlock" => &["WATERLOGGED", "AXIS"],
        "LanternBlock" => &["HANGING", "WATERLOGGED"],
        "WallTorchBlock" => &["HORIZONTAL_FACING"],
        // Each class's `createBlockStateDefinition` as 1.21.1 writes it (only classes whose
        // constructor defaults are `vanilla_default`'s: a campfire starts lit, an end rod faces up).
        "BellBlock" => &["HORIZONTAL_FACING", "BELL_ATTACHMENT", "POWERED"],
        "LecternBlock" => &["HORIZONTAL_FACING", "POWERED", "HAS_BOOK"],
        "ScaffoldingBlock" => &["STABILITY_DISTANCE", "WATERLOGGED", "BOTTOM"],
        "ChestBlock" | "TrappedChestBlock" => &["HORIZONTAL_FACING", "CHEST_TYPE", "WATERLOGGED"],
        "BarrelBlock" => &["FACING", "OPEN"],
        "HopperBlock" => &["FACING_HOPPER", "ENABLED"],
        "AbstractFurnaceBlock" | "FurnaceBlock" | "SmokerBlock" | "BlastFurnaceBlock" => &["HORIZONTAL_FACING", "LIT"],
        "AnvilBlock" | "StonecutterBlock" | "LoomBlock" | "CarvedPumpkinBlock" => &["HORIZONTAL_FACING"],
        "GrindstoneBlock" => &["HORIZONTAL_FACING", "ATTACH_FACE"],
        "StainedGlassPaneBlock" => &["NORTH", "EAST", "WEST", "SOUTH", "WATERLOGGED"],
        "DispenserBlock" | "DropperBlock" => &["FACING", "TRIGGERED"],
        "RepeaterBlock" => &["HORIZONTAL_FACING", "DELAY", "LOCKED", "POWERED"],
        "ComparatorBlock" => &["HORIZONTAL_FACING", "MODE_COMPARATOR", "POWERED"],
        "SnowLayerBlock" => &["LAYERS"],
        "LeavesBlock" => &["DISTANCE", "PERSISTENT", "WATERLOGGED"],
        "CropBlock" => &["AGE_7"],
        "SaplingBlock" => &["STAGE"],
        "BedBlock" => &["HORIZONTAL_FACING", "BED_PART", "OCCUPIED"],
        "CakeBlock" => &["BITES"],
        "CandleBlock" => &["CANDLES", "LIT", "WATERLOGGED"],
        _ => &[],
    }
}

/// Values of vanilla's `BlockStateProperties` fields, in Java's order (the first is the one
/// `StateDefinition.any()` takes).
fn vanilla_values(field: &str) -> Option<Vec<String>> {
    let list = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect());
    let range = |a: i32, b: i32| Some((a..=b).map(|i| i.to_string()).collect());
    match field {
        "FACING" => list(&["north", "east", "south", "west", "up", "down"]),
        "HORIZONTAL_FACING" => list(&["north", "east", "south", "west"]),
        "FACING_HOPPER" => list(&["down", "north", "south", "west", "east"]),
        "VERTICAL_DIRECTION" => list(&["up", "down"]),
        "AXIS" => list(&["x", "y", "z"]),
        "HORIZONTAL_AXIS" => list(&["x", "z"]),
        "SLAB_TYPE" => list(&["top", "bottom", "double"]),
        "HALF" => list(&["top", "bottom"]),
        "STAIRS_SHAPE" => list(&["straight", "inner_left", "inner_right", "outer_left", "outer_right"]),
        "DOUBLE_BLOCK_HALF" => list(&["upper", "lower"]),
        "DOOR_HINGE" => list(&["left", "right"]),
        "ATTACH_FACE" => list(&["floor", "wall", "ceiling"]),
        "NORTH_WALL" | "EAST_WALL" | "SOUTH_WALL" | "WEST_WALL" => list(&["none", "low", "tall"]),
        "NORTH_REDSTONE" | "EAST_REDSTONE" | "SOUTH_REDSTONE" | "WEST_REDSTONE" => list(&["up", "side", "none"]),
        "BED_PART" => list(&["head", "foot"]),
        "CHEST_TYPE" => list(&["single", "left", "right"]),
        "PISTON_TYPE" => list(&["normal", "sticky"]),
        "MODE_COMPARATOR" => list(&["compare", "subtract"]),
        "BELL_ATTACHMENT" => list(&["floor", "ceiling", "single_wall", "double_wall"]),
        "TILT" => list(&["none", "unstable", "partial", "full"]),
        "RAIL_SHAPE" => list(&RAIL_SHAPES),
        "RAIL_SHAPE_STRAIGHT" => list(&RAIL_SHAPES[..6]),
        "AGE_1" => range(0, 1),
        "AGE_2" => range(0, 2),
        "AGE_3" => range(0, 3),
        "AGE_4" => range(0, 4),
        "AGE_5" => range(0, 5),
        "AGE_7" => range(0, 7),
        "AGE_15" | "POWER" | "LEVEL" | "ROTATION_16" => range(0, 15),
        "AGE_25" => range(0, 25),
        "LEVEL_FLOWING" => range(1, 8),
        "LEVEL_CAULDRON" => range(1, 3),
        "LEVEL_COMPOSTER" => range(0, 8),
        "LEVEL_HONEY" => range(0, 5),
        "DISTANCE" => range(1, 7),
        "STABILITY_DISTANCE" | "MOISTURE" => range(0, 7),
        "STAGE" => range(0, 1),
        "DELAY" => range(1, 4),
        "NOTE" => range(0, 24),
        "LAYERS" => range(1, 8),
        "BITES" => range(0, 6),
        "PICKLES" | "EGGS" | "CANDLES" | "FLOWER_AMOUNT" | "SEGMENT_AMOUNT" => range(1, 4),
        "HATCH" => range(0, 2),
        "RESPAWN_ANCHOR_CHARGES" => range(0, 4),
        "DUSTED" | "DRIED_GHAST_HYDRATION_LEVELS" => range(0, 3),
        _ => match api_common::vanilla_property(field) {
            Some((_, "bool")) => list(&["true", "false"]),
            _ => None,
        },
    }
}

/// `RailShape`'s constants in order; the first six are `RAIL_SHAPE_STRAIGHT`'s.
const RAIL_SHAPES: [&str; 10] = [
    "north_south", "east_west", "ascending_east", "ascending_west", "ascending_north", "ascending_south", "south_east", "south_west",
    "north_west", "north_east",
];

/// The value vanilla's constructor gives a property its class's definition adds (`DoorBlock`
/// starts closed, unpowered, lower half), where `StateDefinition.any()` would take another.
fn vanilla_default(field: &str) -> Option<&'static str> {
    Some(match field {
        "DOUBLE_BLOCK_HALF" => "lower",
        "HALF" | "SLAB_TYPE" => "bottom",
        "STAIRS_SHAPE" => "straight",
        "DOOR_HINGE" => "left",
        "ATTACH_FACE" => "wall",
        "STABILITY_DISTANCE" | "DISTANCE" => "7",
        "ENABLED" => "true",
        "BED_PART" => "foot",
        "AXIS" => "y",
        "HORIZONTAL_FACING" | "FACING" => "north",
        "POWER" => "0",
        f => match api_common::vanilla_property(f) {
            Some((_, "bool")) => "false",
            _ => return None,
        },
    })
}

const PUSH_REACTION: &str = "net/minecraft/world/level/material/PushReaction";
/// `PushReaction`'s constants in order: their ordinals are [`BlockFacts::push`].
const PUSH_REACTIONS: [&str; 5] = ["normal", "destroy", "block", "ignore", "push_only"];
/// NeoForge's `IBlockExtension.getPistonPushReaction(state)`: a block's answer for the state, or
/// null for what its properties say.
const GET_PUSH_REACTION: &str = "getPistonPushReaction.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/material/PushReaction;";
const GET_PUSH_REACTION_KEY: &str =
    "net/minecraft/world/level/block/Block.getPistonPushReaction.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/material/PushReaction;";

/// What vanilla's `BlockStateBase` keeps of a block's properties, for state `text`: a mod block's
/// from what its `Properties` recorded, over the vanilla block they copy (`ofFullCopy`); a vanilla
/// block's from the game. Where neither says, `Properties.of()`'s: breaks at once, pistons push
/// and pull it, placing doesn't replace it.
pub(crate) fn state_facts(rt: &Rt, text: &str) -> BlockFacts {
    let (name, _) = crate::host::split_state(text);
    let java = BlockFacts { hardness: 0.0, push: 0, replaceable: false };
    let from_game = |text: &str| rt.host().and_then(|h| h.block_facts(text));
    // A vanilla block's object is its name; a mod's holds fields.
    let Some(b) = rt.registered.get(&("block".to_owned(), name.to_owned())).copied().filter(|b| rt.str_of(*b).is_none()) else {
        return from_game(text).unwrap_or(java);
    };
    let props = rt.get_field(b, "$properties");
    let mut facts = rt.str_of(rt.get_field(props, "$copy")).and_then(from_game).unwrap_or(java);
    let set = |k: &'static str| props.r() != 0 && !matches!(rt.get_field(props, k), V::NULL | V::Void);
    if set("$destroy") {
        facts.hardness = rt.get_field(props, "$destroy").f();
    }
    if set("$push") {
        facts.push = rt.get_field(rt.get_field(props, "$push"), "$ordinal").i().clamp(0, 4) as u8;
    }
    if rt.get_field(props, "$replaceable").i() != 0 {
        facts.replaceable = true;
    }
    facts
}

/// `PushReaction`'s constant by its serialized name (`normal`).
pub(crate) fn push_reaction_named(rt: &mut Rt, name: &str) -> V {
    vanilla_enum_constant(rt, PUSH_REACTION, name).unwrap_or(V::NULL)
}

/// `BlockState.getPistonPushReaction()`, as NeoForge's: the block's own answer for the state when it
/// gives one, else its properties'.
pub(crate) fn push_reaction(rt: &mut Rt, text: &str) -> V {
    if let Some((b, class)) = crate::glue::mod_block(rt, text)
        && rt.overrides.is_some_and(|o| o(class, GET_PUSH_REACTION))
        && let Some(vc) = rt.vcall
    {
        let st = api::state(rt, text);
        let own = vc(rt, GET_PUSH_REACTION_KEY, GET_PUSH_REACTION, &[b, st]);
        rt.report_thrown("getPistonPushReaction");
        if own.r() != 0 {
            return own;
        }
    }
    let i = state_facts(rt, text).push as usize;
    vanilla_enum_constant(rt, PUSH_REACTION, PUSH_REACTIONS[i.min(4)]).unwrap_or(V::NULL)
}

/// Vanilla enum `class`'s constant whose serialized name is `text` (`floor` is
/// `BellAttachType.FLOOR`): one object per constant, as `values()` and the static fields give them.
pub(crate) fn vanilla_enum_constant(rt: &mut Rt, class: &str, text: &str) -> Option<V> {
    let values = vanilla_enum(class)?;
    let i = values.iter().position(|v| *v == text)?;
    let class: &'static str = rt.static_key(class);
    let name: &'static str = rt.static_key(&values[i].to_ascii_uppercase());
    Some(api::api_enum(rt, class, name, i as i32))
}

/// Vanilla enum `class`'s constants, in order (`values()`); None for one the runtime doesn't know.
fn vanilla_enum_values(rt: &mut Rt, class: &str) -> Option<Vec<V>> {
    let values = vanilla_enum(class)?;
    Some(values.iter().filter_map(|v| vanilla_enum_constant(rt, class, v)).collect())
}

/// `DyeColor`'s colours by id: texture diffuse (opaque ARGB), firework, text; the game's own tables
/// (`block_entity_render::DYES`, `firework::DYE_FIREWORK_COLORS`, `signs::DYE_TEXT`) hold the same.
const DYE_COLORS: [(u32, u32, u32); 16] = [
    (0xF9FFFE, 0xF0F0F0, 0xFFFFFF),
    (0xF9801D, 0xEB8844, 0xFF681F),
    (0xC74EBD, 0xC354CD, 0xFF00FF),
    (0x3AB3DA, 0x6689D3, 0x9AC0CD),
    (0xFED83D, 0xDECF2A, 0xFFFF00),
    (0x80C71F, 0x41CD34, 0xBFFF00),
    (0xF38BAA, 0xD88198, 0xFF69B4),
    (0x474F52, 0x434343, 0x808080),
    (0x9D9D97, 0xABABAB, 0xD3D3D3),
    (0x169C9C, 0x287697, 0x00FFFF),
    (0x8932B8, 0x7B2FBE, 0xA020F0),
    (0x3C44AA, 0x253192, 0x0000FF),
    (0x835432, 0x51301A, 0x8B4513),
    (0x5E7C16, 0x3B511A, 0x00FF00),
    (0xB02E26, 0xB3312C, 0xFF0000),
    (0x1D1D21, 0x1E1B1B, 0x000000),
];

/// `DyeColor`'s methods: its name and id, its colours, and the lookups by id and name.
fn dye_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let dye = |rt: &mut Rt, i: usize| api::api_enum(rt, DYE_COLOR, DYES[i.min(15)].0, i.min(15) as i32);
    let id = |rt: &Rt| rt.get_field(a(0), "$ordinal").i().clamp(0, 15) as usize;
    Some(match name {
        "getId" => V::I(id(rt) as i32),
        "getName" | "getSerializedName" | "toString" => {
            let n = DYES[id(rt)].0.to_ascii_lowercase();
            rt.string(&n)
        }
        "getTextureDiffuseColor" => V::I((0xFF00_0000 | DYE_COLORS[id(rt)].0) as i32),
        "getFireworkColor" => V::I(DYE_COLORS[id(rt)].1 as i32),
        "getTextColor" => V::I(DYE_COLORS[id(rt)].2 as i32),
        "getMapColor" => {
            let c = DYES[id(rt)].1 as usize;
            let o = named(rt, MAP_COLOR, MAP_COLORS[c]);
            rt.put_field(o, "id", V::I(c as i32));
            o
        }
        // `byId`: out of range gives white, as vanilla's `ByIdMap.OutOfBoundsStrategy.ZERO`.
        "byId" => {
            let i = a(0).i();
            dye(rt, if (0..16).contains(&i) { i as usize } else { 0 })
        }
        "byName" | "byFireworkColor" => {
            let found = if name == "byName" {
                let n = rt.str_of(a(0)).unwrap_or("").to_ascii_uppercase();
                DYES.iter().position(|(d, _)| *d == n)
            } else {
                DYE_COLORS.iter().position(|c| c.1 as i32 == a(0).i())
            };
            match found {
                Some(i) => dye(rt, i),
                None if name == "byName" => a(1),
                None => V::NULL,
            }
        }
        _ => return None,
    })
}

/// Whether `class` is a vanilla enum whose constants the runtime makes (`values()`, its statics).
fn enum_class(class: &str) -> bool {
    vanilla_enum(class).is_some()
        && (class.starts_with("net/minecraft/world/level/block/state/properties/")
            || matches!(
                class,
                DYE_COLOR
                    | PUSH_REACTION
                    | crate::rotation::ROTATION
                    | crate::rotation::MIRROR
                    | "net/minecraft/world/level/block/WeatheringCopper$WeatherState"
                    | "net/minecraft/world/item/ArmorItem$Type"
            ))
}

/// Values of vanilla enums a mod's own property may take (`EnumProperty.create(name, Half.class)`).
fn vanilla_enum(class: &str) -> Option<&'static [&'static str]> {
    Some(match class.rsplit('/').next().unwrap_or(class) {
        "SlabType" => &["top", "bottom", "double"],
        "Half" => &["top", "bottom"],
        "StairsShape" => &["straight", "inner_left", "inner_right", "outer_left", "outer_right"],
        "DoubleBlockHalf" => &["upper", "lower"],
        "DoorHingeSide" => &["left", "right"],
        "AttachFace" => &["floor", "wall", "ceiling"],
        "WallSide" => &["none", "low", "tall"],
        "RedstoneSide" => &["up", "side", "none"],
        "BedPart" => &["head", "foot"],
        "ChestType" => &["single", "left", "right"],
        "PistonType" => &["normal", "sticky"],
        "ComparatorMode" => &["compare", "subtract"],
        "BellAttachType" => &["floor", "ceiling", "single_wall", "double_wall"],
        "Tilt" => &["none", "unstable", "partial", "full"],
        "RailShape" => &RAIL_SHAPES,
        "WeatheringCopper$WeatherState" => &["unaffected", "exposed", "weathered", "oxidized"],
        "PushReaction" => &PUSH_REACTIONS,
        "Rotation" if class == crate::rotation::ROTATION => &crate::rotation::ROTATIONS,
        "Mirror" if class == crate::rotation::MIRROR => &crate::rotation::MIRRORS,
        "ArmorItem$Type" => &["helmet", "chestplate", "leggings", "boots", "body"],
        "DyeColor" => &[
            "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown",
            "green", "red", "black",
        ],
        _ => return None,
    })
}

/// Calls this module answers (before the general mapping): property builders record what they're
/// Vanilla's `getStateForPlacement` for block class `class` on block `b`: a pillar's axis is the
/// clicked face's, a face-attached block (lever, button) takes the first looking direction (floor or
/// ceiling facing the player's way, else the wall it looks at), anything else its default state.
fn vanilla_placement(rt: &mut Rt, class: &str, b: V, ctx: V) -> V {
    let default = api::call(rt, "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;", &[b]);
    let Some(text) = rt.str_of(default).map(str::to_owned) else { return default };
    let ordinal = |rt: &mut Rt, key: &'static str| -> i64 {
        let d = api::call(rt, key, &[ctx]);
        match rt.heap.get(d.r() as usize).map(|o| &o.data) {
            Some(Data::Native("Direction", n)) => n[0],
            _ => 2,
        }
    };
    const NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
    let set = |text: &str, key: &str, value: &str| -> String {
        let (block, mut props) = crate::host::split_state(text);
        for p in props.iter_mut() {
            if p.0 == key {
                p.1 = value;
            }
        }
        crate::host::join_state(block, props)
    };
    let out = match class.rsplit('/').next().unwrap_or("") {
        "RotatedPillarBlock" => {
            let face = ordinal(rt, "net/minecraft/world/item/context/BlockPlaceContext.getClickedFace.()Lnet/minecraft/core/Direction;");
            set(&text, "axis", ["y", "y", "z", "z", "x", "x"][face.clamp(0, 5) as usize])
        }
        "FaceAttachedHorizontalDirectionalBlock" => {
            let first = {
                let arr = api::call(rt, "net/minecraft/world/item/context/BlockPlaceContext.getNearestLookingDirections.()[Lnet/minecraft/core/Direction;", &[ctx]);
                let d = rt.array_get(arr, 0);
                match rt.heap.get(d.r() as usize).map(|o| &o.data) {
                    Some(Data::Native("Direction", n)) => n[0],
                    _ => 2,
                }
            };
            if first <= 1 {
                let h = ordinal(rt, "net/minecraft/world/item/context/BlockPlaceContext.getHorizontalDirection.()Lnet/minecraft/core/Direction;");
                set(&set(&text, "face", if first == 1 { "ceiling" } else { "floor" }), "facing", NAMES[h.clamp(2, 5) as usize])
            } else {
                set(&set(&text, "face", "wall"), "facing", NAMES[(first ^ 1) as usize])
            }
        }
        _ => text,
    };
    api::state(rt, &out)
}

/// given, block constructors collect their states.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    match (owner, name) {
        // `BellAttachType.values()`: its constants, in order.
        (o, "values") if desc.starts_with("()[") && enum_class(o) => {
            let all = vanilla_enum_values(rt, o)?;
            let class: &'static str = rt.static_key(&format!("[L{o};"));
            Some(rt.alloc(class, Data::Array(all)))
        }
        (DYE_COLOR, _) => dye_call(rt, name, args),
        // `ArmorItem.Type`: its name and vanilla's durability per material multiplier.
        ("net/minecraft/world/item/ArmorItem$Type", "getName" | "getSerializedName") => {
            let n = rt.str_of(rt.get_field(a(0), "$name")).unwrap_or("").to_ascii_lowercase();
            Some(rt.string(&n))
        }
        ("net/minecraft/world/item/ArmorItem$Type", "getDurability") => {
            const HEALTH: [i32; 5] = [11, 16, 15, 13, 16];
            let i = rt.get_field(a(0), "$ordinal").i().clamp(0, 4) as usize;
            Some(V::I(HEALTH[i] * a(1).i()))
        }
        (BLOCK_PROPERTIES, _) => block_properties(rt, name, desc, args),
        (ITEM_PROPERTIES, _) => item_properties(rt, name, args),
        (WOOD_TYPE, "name") => Some(rt.get_field(a(0), "$name")),
        (BUILDER, "add") => {
            let list = rt.get_field(a(0), "$props");
            for p in jutil::elements(rt, a(1)) {
                add_property(rt, list, p);
            }
            Some(a(0))
        }
        // `super.createBlockStateDefinition(builder)` reaching a vanilla class: what it adds.
        (o, "createBlockStateDefinition") if o.starts_with("net/minecraft/world/level/block/") => {
            add_vanilla_definition(rt, a(1), o);
            Some(V::Void)
        }
        (_, "registerDefaultState") if desc == "(Lnet/minecraft/world/level/block/state/BlockState;)V" => {
            let text = rt.str_of(a(1)).unwrap_or("").to_owned();
            let (_, props) = crate::host::split_state(&text);
            let joined: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
            let s = rt.string(&joined.join(","));
            rt.put_field(a(0), "$default", s);
            Some(V::Void)
        }
        // A mod block's default state: its registered name with the properties its constructor set.
        (_, "defaultBlockState") if desc == "()Lnet/minecraft/world/level/block/state/BlockState;" => {
            let d = rt.get_field(a(0), "$default");
            let props = rt.str_of(d)?.to_owned();
            let block = rt.registered_name(a(0), "block").unwrap_or("").to_owned();
            let text = if props.is_empty() { block } else { format!("{block}[{props}]") };
            Some(api::state(rt, &text))
        }
        // `super.getStateForPlacement(context)` reaching a vanilla class: its own rule where it has one
        // that only reads the click, else `Block`'s (the default state).
        (o, "getStateForPlacement") if o.starts_with("net/minecraft/world/level/block/") && desc.starts_with("(Lnet/minecraft/world/item/context/BlockPlaceContext;)") => {
            Some(vanilla_placement(rt, o, a(0), a(1)))
        }
        (o, "<init>") if o.starts_with("net/minecraft/world/level/block/") && desc.contains("Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;") => {
            let props = args.iter().skip(1).copied().find(|v| v.r() != 0 && rt.class_of(*v) == BLOCK_PROPERTIES).unwrap_or(V::NULL);
            // `LiquidBlock(FlowingFluid, Properties)`: its fluid, the field vanilla names `fluid`.
            if o == "net/minecraft/world/level/block/LiquidBlock" {
                rt.put_field(a(0), "fluid", a(1));
            }
            constructed(rt, a(0), o, props);
            Some(V::Void)
        }
        (o, "<init>") if o.starts_with("net/minecraft/world/item/") && desc.contains("Lnet/minecraft/world/item/Item$Properties;") => {
            if let Some(p) = args.iter().skip(1).copied().find(|v| v.r() != 0 && rt.class_of(*v) == ITEM_PROPERTIES) {
                rt.put_field(a(0), "$properties", p);
            }
            // `BlockItem(Block, Properties)` and its kinds: the block it places (`Block.asItem()` finds it).
            if desc.starts_with("(Lnet/minecraft/world/level/block/Block;") {
                rt.put_field(a(0), "$block", a(1));
            }
            // `BucketItem(Fluid, Properties)`: the fluid it holds, vanilla's field `content`.
            if o == "net/minecraft/world/item/BucketItem" {
                rt.put_field(a(0), "content", a(1));
            }
            Some(V::Void)
        }
        _ => None,
    }
}

/// Static fields this module owns: `SoundType`, `MapColor`, `DyeColor`, `Rarity` constants.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        SOUND_TYPE => {
            let (f, _) = SOUNDS.iter().find(|(f, _)| *f == field)?;
            Some(named(rt, SOUND_TYPE, f))
        }
        MAP_COLOR => {
            let id = MAP_COLORS.iter().position(|c| *c == field)?;
            let o = named(rt, MAP_COLOR, MAP_COLORS[id]);
            rt.put_field(o, "id", V::I(id as i32));
            Some(o)
        }
        DYE_COLOR => {
            let i = DYES.iter().position(|(d, _)| *d == field)?;
            Some(api::api_enum(rt, DYE_COLOR, DYES[i].0, i as i32))
        }
        WOOD_TYPE => {
            let (_, name) = WOOD_TYPES.iter().find(|(f, _)| *f == field)?;
            Some(named(rt, WOOD_TYPE, name))
        }
        RARITY => {
            const R: [&str; 4] = ["COMMON", "UNCOMMON", "RARE", "EPIC"];
            let i = R.iter().position(|r| *r == field)?;
            Some(api::api_enum(rt, RARITY, R[i], i as i32))
        }
        // The block-state enums' constants (`BellAttachType.FLOOR`), the objects a state's value is.
        o if o != DYE_COLOR && enum_class(o) => {
            vanilla_enum_constant(rt, o, &field.to_ascii_lowercase())
        }
        _ => None,
    }
}

const WOOD_TYPE: &str = "net/minecraft/world/level/block/state/properties/WoodType";
/// Vanilla's wood types by field, with their `name()`: mods name blocks after them (Create's
/// `oak_window` is `woodType.name() + "_window"`).
const WOOD_TYPES: [(&str, &str); 12] = [
    ("OAK", "oak"),
    ("SPRUCE", "spruce"),
    ("BIRCH", "birch"),
    ("ACACIA", "acacia"),
    ("CHERRY", "cherry"),
    ("JUNGLE", "jungle"),
    ("DARK_OAK", "dark_oak"),
    ("PALE_OAK", "pale_oak"),
    ("CRIMSON", "crimson"),
    ("WARPED", "warped"),
    ("MANGROVE", "mangrove"),
    ("BAMBOO", "bamboo"),
];

/// One object per named constant of a class that isn't an enum (`SoundType.METAL`).
fn named(rt: &mut Rt, class: &'static str, name: &'static str) -> V {
    rt.intern(format!("const/{class}.{name}"), |rt| {
        let o = rt.new_object(class);
        let n = rt.string(name);
        rt.put_field(o, "$name", n);
        o
    })
}

/// `BlockBehaviour.Properties` (1.21.1's builder, every method): each call is kept on the object as
/// a `$` field. The game reads destroy time, resistance, sound, light, occlusion, collision, tool,
/// friction, random ticks, map colour, push reaction and replaceable; the predicates
/// (`isRedstoneConductor`, `isSuffocating`, `isViewBlocking`, `isValidSpawn`, `hasPostProcess`,
/// `emissiveRendering`) and the other flags are kept for it but not read yet.
fn block_properties(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if name == "of" {
        return Some(rt.new_object(BLOCK_PROPERTIES));
    }
    let this = a(0);
    let set = |rt: &mut Rt, f: &'static str, v: V| {
        rt.put_field(this, f, v);
        Some(this)
    };
    match name {
        "ofFullCopy" | "ofLegacyCopy" | "copy" => {
            let p = rt.new_object(BLOCK_PROPERTIES);
            let from = a(0);
            match rt.str_of(from) {
                // A vanilla block (`Blocks.ANDESITE`): named, its values from the table.
                Some(n) => {
                    let s = rt.string(&n.to_owned());
                    rt.put_field(p, "$copy", s);
                }
                // A mod block: what its own properties recorded.
                None => {
                    let src = rt.get_field(from, "$properties");
                    if let Some(Data::Fields(f)) = rt.heap.get(src.r() as usize).map(|o| o.data.clone()) {
                        for (k, v) in f {
                            rt.put_field(p, k, v);
                        }
                    }
                }
            }
            Some(p)
        }
        "strength" if desc.starts_with("(FF)") => {
            rt.put_field(this, "$destroy", a(1));
            set(rt, "$resist", a(2))
        }
        "strength" => {
            rt.put_field(this, "$destroy", a(1));
            set(rt, "$resist", a(1))
        }
        "destroyTime" => set(rt, "$destroy", a(1)),
        "explosionResistance" => set(rt, "$resist", a(1)),
        "instabreak" => {
            rt.put_field(this, "$destroy", V::F(0.0));
            set(rt, "$resist", V::F(0.0))
        }
        "sound" => set(rt, "$sound", a(1)),
        "lightLevel" => set(rt, "$light", a(1)),
        "noOcclusion" => set(rt, "$noOcclusion", V::I(1)),
        "noCollission" | "noCollision" => {
            rt.put_field(this, "$noOcclusion", V::I(1));
            set(rt, "$noCollision", V::I(1))
        }
        "requiresCorrectToolForDrops" => set(rt, "$requiresTool", V::I(1)),
        "friction" => set(rt, "$friction", a(1)),
        "randomTicks" => set(rt, "$randomTicks", V::I(1)),
        "mapColor" => set(rt, "$mapColor", a(1)),
        "pushReaction" => set(rt, "$push", a(1)),
        "replaceable" => set(rt, "$replaceable", V::I(1)),
        "speedFactor" => set(rt, "$speedFactor", a(1)),
        "jumpFactor" => set(rt, "$jumpFactor", a(1)),
        "dynamicShape" => set(rt, "$dynamicShape", V::I(1)),
        "noLootTable" => set(rt, "$noLootTable", V::I(1)),
        "dropsLike" => set(rt, "$dropsLike", a(1)),
        "lootFrom" => set(rt, "$lootFrom", a(1)),
        "ignitedByLava" => set(rt, "$ignitedByLava", V::I(1)),
        "liquid" => set(rt, "$liquid", V::I(1)),
        "forceSolidOn" => set(rt, "$forceSolidOn", V::I(1)),
        "forceSolidOff" => set(rt, "$forceSolidOff", V::I(1)),
        "air" => set(rt, "$air", V::I(1)),
        "isValidSpawn" => set(rt, "$isValidSpawn", a(1)),
        "isRedstoneConductor" => set(rt, "$isRedstoneConductor", a(1)),
        "isSuffocating" => set(rt, "$isSuffocating", a(1)),
        "isViewBlocking" => set(rt, "$isViewBlocking", a(1)),
        "hasPostProcess" => set(rt, "$hasPostProcess", a(1)),
        "emissiveRendering" => set(rt, "$emissiveRendering", a(1)),
        "offsetType" => set(rt, "$offsetType", a(1)),
        "noTerrainParticles" => set(rt, "$noTerrainParticles", V::I(1)),
        "requiredFeatures" => set(rt, "$requiredFeatures", a(1)),
        "instrument" => set(rt, "$instrument", a(1)),
        _ => None,
    }
}

/// `Item.Properties`: stack size, durability, rarity, food.
fn item_properties(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let field = match name {
        "<init>" => return Some(V::Void),
        "stacksTo" => "$stacksTo",
        "durability" => "$durability",
        "rarity" => "$rarity",
        "food" => "$food",
        "fireResistant" => {
            rt.put_field(this, "$fireResistant", V::I(1));
            return Some(this);
        }
        _ => return None,
    };
    rt.put_field(this, field, a(1));
    Some(this)
}

/// A block's construction reaching vanilla's `Block` constructor (through `vanilla`, the first
/// vanilla class above the mod's): its properties, the states its definition declares, and
/// `StateDefinition.any()` as the default state until `registerDefaultState` sets another.
fn constructed(rt: &mut Rt, b: V, vanilla: &str, props: V) {
    rt.put_field(b, "$properties", props);
    let vanilla_class = rt.string(vanilla);
    rt.put_field(b, "$vanilla", vanilla_class);
    let builder = rt.new_object(BUILDER);
    let list = jutil::new_list(rt, Vec::new());
    rt.put_field(builder, "$props", list);
    rt.put_field(b, "$state-props", list);
    let class = rt.class_of(b);
    match rt.vcall {
        Some(vc) if rt.overrides.is_some_and(|o| o(class, DEFINE)) => {
            vc(rt, DEFINE_KEY, DEFINE, &[b, builder]);
            rt.report_thrown("createBlockStateDefinition");
        }
        _ => add_vanilla_definition(rt, builder, vanilla),
    }
    let mut any = Vec::new();
    for p in jutil::elements(rt, list) {
        let (pname, _) = api_common::prop_parts(rt, p);
        let values = property_values(rt, p);
        let field = rt.str_of(rt.get_field(p, "$field")).map(str::to_owned);
        let vanilla_set = (rt.get_field(p, "$vanilla-default").i() != 0)
            .then(|| field.as_deref().and_then(vanilla_default))
            .flatten()
            .filter(|v| values.iter().any(|x| x == v));
        if let Some(first) = vanilla_set.map(str::to_owned).or_else(|| values.into_iter().next()) {
            any.push(format!("{pname}={first}"));
        }
    }
    any.sort();
    let s = rt.string(&any.join(","));
    rt.put_field(b, "$default", s);
}

fn add_vanilla_definition(rt: &mut Rt, builder: V, class: &str) {
    let list = rt.get_field(builder, "$props");
    for field in vanilla_definition(class) {
        if let Some(p) = api_common::get_static(rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties", field) {
            add_property(rt, list, p);
            // Its vanilla constructor sets this one's default.
            rt.put_field(p, "$vanilla-default", V::I(1));
        }
    }
}

/// Adds a property to a definition's list, once per name (a definition can't hold two).
fn add_property(rt: &mut Rt, list: V, p: V) {
    if list.r() == 0 || p.r() == 0 {
        return;
    }
    let (name, _) = api_common::prop_parts(rt, p);
    let mut items = jutil::elements(rt, list);
    if items.iter().any(|q| api_common::prop_parts(rt, *q).0 == name) {
        return;
    }
    items.push(p);
    if let Some(o) = rt.heap.get_mut(list.r() as usize) {
        o.data = Data::List(items);
    }
}

/// A property made by `BooleanProperty.create` and friends (`owner`, `desc`, `args` of that call):
/// the values it takes beyond its kind's.
pub fn created_property(rt: &mut Rt, p: V, owner: &str, desc: &str, args: &[V]) {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner.ends_with("IntegerProperty") {
        rt.put_field(p, "$min", a(1));
        rt.put_field(p, "$max", a(2));
        return;
    }
    let direction = owner.ends_with("DirectionProperty");
    if !direction && !owner.ends_with("EnumProperty") {
        return;
    }
    // The constants it may take: given, or all its class's, filtered by a predicate.
    let given = if direction { 1 } else { 2 };
    let candidates: Vec<V> = if desc.contains("[L") || desc.contains("Ljava/util/Collection;") || desc.contains("Ljava/util/List;") {
        let listed = jutil::elements(rt, a(given));
        if listed.is_empty() && rt.array_len(a(given)) > 0 {
            (0..rt.array_len(a(given))).map(|i| rt.array_get(a(given), i)).collect()
        } else {
            listed
        }
    } else {
        let all = if direction {
            (0..6).map(|o| api_common::direction(rt, o)).collect()
        } else {
            let (_, kind) = api_common::prop_parts(rt, p);
            enum_constants(rt, &kind)
        };
        if desc.contains("Ljava/util/function/Predicate;") {
            let pred = a(given);
            all.into_iter().filter(|c| rt.call_functional(PREDICATE, pred, &[*c]).i() != 0).collect()
        } else if direction {
            all
        } else {
            return; // every constant: read from the class when asked
        }
    };
    let texts: Vec<V> = candidates
        .iter()
        .map(|c| {
            let t = api_common::text_of(rt, *c);
            rt.string(&t)
        })
        .collect();
    let l = jutil::new_list(rt, texts);
    rt.put_field(p, "$values", l);
}

/// A mod enum's constants, in order (`values()`).
fn enum_constants(rt: &mut Rt, class: &str) -> Vec<V> {
    match rt.reflect(class, "values", &[]) {
        Some(arr) if arr.r() != 0 => (0..rt.array_len(arr)).map(|i| rt.array_get(arr, i)).collect(),
        _ => Vec::new(),
    }
}

/// A property's values in Java's order, as they read in a state.
pub fn property_values(rt: &mut Rt, p: V) -> Vec<String> {
    let listed = rt.get_field(p, "$values");
    if listed.r() != 0 {
        return jutil::elements(rt, listed).iter().map(|v| rt.str_of(*v).unwrap_or("").to_owned()).collect();
    }
    let field = rt.str_of(rt.get_field(p, "$field")).map(str::to_owned);
    if let Some(v) = field.as_deref().and_then(vanilla_values) {
        return v;
    }
    let (_, kind) = api_common::prop_parts(rt, p);
    match kind.as_str() {
        "bool" => vec!["true".into(), "false".into()],
        "int" => {
            let (lo, hi) = (rt.get_field(p, "$min").i(), rt.get_field(p, "$max").i());
            (lo..=hi.max(lo)).map(|i| i.to_string()).collect()
        }
        "axis" => vec!["x".into(), "y".into(), "z".into()],
        "direction" => api_common::DIRECTION_NAMES.iter().map(|s| s.to_string()).collect(),
        class => match vanilla_enum(class) {
            Some(v) => v.iter().map(|s| s.to_string()).collect(),
            None => {
                let constants = enum_constants(rt, class);
                constants.iter().map(|c| api_common::text_of(rt, *c)).collect()
            }
        },
    }
}

/// Every block the mod registered (outside `minecraft`), as `(id, declaration)`: the JSON of a
/// content block file (docs/mods.md).
pub fn block_declarations(rt: &mut Rt) -> Vec<(String, String)> {
    let mut blocks: Vec<(String, V)> = rt
        .registered
        .iter()
        .filter(|((r, n), _)| r == "block" && own_id(n))
        .map(|((_, n), v)| (n.clone(), *v))
        .collect();
    blocks.sort_by(|a, b| a.0.cmp(&b.0));
    let items: std::collections::HashSet<String> = rt.registered.keys().filter(|(r, _)| r == "item").map(|(_, n)| n.clone()).collect();
    blocks.into_iter().map(|(name, b)| {
        let item = items.contains(&name);
        let json = block_declaration(rt, &name, b, item);
        (name, json)
    }).collect()
}

/// A registered name of the mod's own: `<namespace>:<path>` outside `minecraft`.
fn own_id(name: &str) -> bool {
    name.split_once(':').is_some_and(|(ns, path)| !ns.is_empty() && ns != "minecraft" && !path.is_empty())
}

/// A state property's name or value as states spell them: lower case letters, digits, `_`.
fn state_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// One block's declaration.
fn block_declaration(rt: &mut Rt, name: &str, b: V, item: bool) -> String {
    let props = rt.get_field(b, "$properties");
    let vanilla = rt.str_of(rt.get_field(b, "$vanilla")).unwrap_or("").to_owned();
    let list = rt.get_field(b, "$state-props");
    let mut states: Vec<(String, Vec<String>)> = Vec::new();
    for p in jutil::elements(rt, list) {
        let (pname, _) = api_common::prop_parts(rt, p);
        // Values a state can spell (a property whose values aren't known is left out).
        let values: Vec<String> = property_values(rt, p).into_iter().filter(|v| state_name(v)).collect();
        if !values.is_empty() && state_name(&pname) {
            states.push((pname, values));
        }
    }
    let default_text = rt.str_of(rt.get_field(b, "$default")).unwrap_or("").to_owned();
    let mut default: Vec<(String, String)> = Vec::new();
    for (pname, values) in &states {
        let set = default_text.split(',').filter_map(|kv| kv.split_once('=')).find(|(k, _)| k == pname).map(|(_, v)| v.to_owned());
        let v = set.filter(|v| values.contains(v)).unwrap_or_else(|| values[0].clone());
        default.push((pname.clone(), v));
    }
    let default_state = if default.is_empty() { name.to_owned() } else {
        let inner: Vec<String> = default.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{name}[{}]", inner.join(","))
    };

    // The template: a vanilla block of the same kind (a door), else one without states that lets
    // light through when the block isn't a full cube or doesn't occlude, else stone.
    let simple = vanilla.rsplit('/').next().unwrap_or("");
    let class = rt.class_of(b);
    let mut vanilla_template = TEMPLATES.iter().find(|(c, _)| *c == simple).map(|(_, t)| t.to_string());
    let mut template_props: Vec<String> = match vanilla_template {
        Some(_) => vanilla_definition(&vanilla).iter().filter_map(|f| api_common::vanilla_property(f).map(|(n, _)| n)).collect(),
        None => Vec::new(),
    };
    // The game orders a block's states by property name, its template's first: a property of its
    // own that sorts before one of the template's can't be added to it, so such a block (Create's
    // glass pipe: `alt` beside a pillar's `axis`) takes a template without states and owns them all.
    if states.iter().any(|(n, _)| !template_props.contains(n) && template_props.iter().any(|t| n < t)) {
        vanilla_template = None;
        template_props.clear();
    }
    // A shape of its own (`getShape`), read for every state; a vanilla template keeps its own.
    let own_shape = rt.overrides.is_some_and(|o| o(class, GET_SHAPE));
    let shapes = if vanilla_template.is_none() && own_shape { state_shapes(rt, b, name, &states) } else { None };
    let template = match vanilla_template {
        Some(t) => t,
        None => {
            let full = match &shapes {
                Some(s) => s.iter().all(|(_, boxes)| boxes[..] == [FULL_BOX]),
                // Unread: taken as not a full cube.
                None => !own_shape,
            };
            let see_through = rt.get_field(props, "$noOcclusion").i() != 0 || !full;
            if see_through { "minecraft:glass".into() } else { "minecraft:stone".into() }
        }
    };

    let mut p = Vec::new();
    let copy = rt.str_of(rt.get_field(props, "$copy")).map(str::to_owned);
    let copied = copy.as_deref().and_then(|c| COPIED.iter().find(|(n, ..)| *n == c));
    let f = |rt: &Rt, k: &'static str| -> Option<f32> {
        let v = rt.get_field(props, k);
        (props.r() != 0 && v != V::NULL && v != V::Void).then(|| v.f())
    };
    let has = |rt: &Rt, k: &'static str| props.r() != 0 && !matches!(rt.get_field(props, k), V::NULL | V::Void);
    let destroy = if has(rt, "$destroy") { f(rt, "$destroy") } else { copied.map(|c| c.1) };
    let resist = if has(rt, "$resist") { f(rt, "$resist") } else { copied.map(|c| c.2) };
    if let Some(d) = destroy.filter(|d| d.is_finite()) {
        p.push(format!("\"hardness\": {}", num(d)));
    }
    if let Some(r) = resist.filter(|r| r.is_finite() && *r >= 0.0) {
        p.push(format!("\"explosion_resistance\": {}", num(r)));
    }
    let tool = rt.get_field(props, "$requiresTool").i() != 0 || (!has(rt, "$requiresTool") && copied.is_some_and(|c| c.3));
    if tool {
        p.push("\"requires_tool\": true".into());
    }
    if has(rt, "$friction") {
        p.push(format!("\"friction\": {}", num(rt.get_field(props, "$friction").f())));
    }
    if let Some(sound) = sound_block(rt, rt.get_field(props, "$sound")).or_else(|| copy.clone()) {
        p.push(format!("\"sounds\": \"{sound}\""));
    }
    let map = map_color(rt, rt.get_field(props, "$mapColor"), &default_state).or_else(|| copied.map(|c| c.4));
    if let Some(m) = map {
        p.push(format!("\"map_color\": {m}"));
    }
    let light = rt.get_field(props, "$light");
    if props.r() != 0 && light.r() != 0 {
        let st = api::state(rt, &default_state);
        let level = rt.call_functional(TO_INT, light, &[st]).i();
        rt.thrown = None;
        if (1..=15).contains(&level) {
            p.push(format!("\"light\": {level}"));
        }
    }
    if rt.get_field(props, "$randomTicks").i() != 0 {
        p.push("\"random_ticks\": true".into());
    }

    let own: Vec<String> = states
        .iter()
        .filter(|(n, _)| !template_props.contains(n))
        .map(|(n, values)| {
            if values.len() == 2 && values.contains(&"true".to_owned()) && values.contains(&"false".to_owned()) {
                format!("\"{n}\": {{\"type\": \"boolean\"}}")
            } else {
                let vs: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
                format!("\"{n}\": {{\"type\": \"enum\", \"values\": [{}]}}", vs.join(", "))
            }
        })
        .collect();
    let defaults: Vec<String> = default.iter().map(|(k, v)| format!("\"{k}\": \"{v}\"")).collect();
    let shapes = shapes.and_then(|s| shapes_json(&s)).map_or(String::new(), |j| format!(", \"shapes\": {j}"));
    format!(
        "{{\"template\": \"{template}\", \"item\": {item}, \"properties\": {{{}}}, \"state_properties\": {{{}}}, \"default_state\": {{{}}}{shapes}}}",
        p.join(", "),
        own.join(", "),
        defaults.join(", ")
    )
}

const FULL_BOX: crate::shapes::Box3 = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// Each state of the block (its properties' values, by name) with its outline from its own
/// `getShape`, asked as vanilla asks with no entity: an empty collision context at the origin, no
/// level. None when a state's shape can't be read (it throws, reads the level, or isn't a shape the
/// runtime made), when the block has too many states, or a shape isn't finite.
fn state_shapes(rt: &mut Rt, b: V, name: &str, states: &[(String, Vec<String>)]) -> Option<Vec<(Vec<(String, String)>, Vec<crate::shapes::Box3>)>> {
    let vc = rt.vcall?;
    let count = states.iter().try_fold(1usize, |n, (_, vs)| n.checked_mul(vs.len()))?;
    if count > MAX_SHAPE_STATES {
        return None;
    }
    let ctx = crate::shapes::empty_context(rt);
    let origin = crate::shapes::origin(rt);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // The i-th combination, the last property changing fastest.
        let mut rest = i;
        let mut combo = vec![(String::new(), String::new()); states.len()];
        for (k, (pname, values)) in states.iter().enumerate().rev() {
            combo[k] = (pname.clone(), values[rest % values.len()].clone());
            rest /= values.len();
        }
        let text = if combo.is_empty() {
            name.to_owned()
        } else {
            let inner: Vec<String> = combo.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{name}[{}]", inner.join(","))
        };
        let st = api::state(rt, &text);
        rt.interned.remove("compat:missing-property");
        let shape = vc(rt, GET_SHAPE_KEY, GET_SHAPE, &[b, st, V::NULL, origin, ctx]);
        let unknown = rt.thrown.is_some() || rt.interned.remove("compat:missing-property").is_some();
        rt.report_thrown("getShape");
        let boxes = crate::shapes::boxes(rt, shape).filter(|_| !unknown)?;
        if boxes.iter().flatten().any(|x| !x.is_finite()) {
            return None;
        }
        // Float noise from rotating shapes (`0.37499998`) rounds away, so equal shapes compare
        // equal and files read as written.
        let snapped: Vec<crate::shapes::Box3> = boxes.iter().map(|b| b.map(|x| {
            let r = (x * 256.0).round() / 256.0;
            if (x - r).abs() < 1.0e-6 { r } else { x }
        })).collect();
        let mut boxes = crate::shapes::optimized(&snapped);
        if boxes.len() > MAX_SHAPE_BOXES {
            boxes = vec![crate::shapes::bounds(&boxes)];
        }
        combo.sort();
        out.push((combo, boxes));
    }
    Some(out)
}

/// The `shapes` a content declaration holds for these states: one list when every state has the
/// same (None when that is a full block, the template's own), by one property when that decides
/// it, else the states that differ from the most common shape. Keys list properties by name.
fn shapes_json(shapes: &[(Vec<(String, String)>, Vec<crate::shapes::Box3>)]) -> Option<String> {
    let list = |boxes: &[crate::shapes::Box3]| {
        let each: Vec<String> = boxes.iter().map(|b| {
            let n: Vec<String> = b.iter().map(|x| format!("{x}")).collect();
            format!("[{}]", n.join(", "))
        }).collect();
        format!("[{}]", each.join(", "))
    };
    let first = &shapes.first()?.1;
    if shapes.iter().all(|(_, s)| s == first) {
        return (first[..] != [FULL_BOX]).then(|| list(first));
    }
    // One property deciding the shape.
    let props: Vec<&String> = shapes[0].0.iter().map(|(k, _)| k).collect();
    for (pi, pname) in props.iter().enumerate() {
        let mut by: Vec<(&str, &Vec<crate::shapes::Box3>)> = Vec::new();
        let decides = shapes.iter().all(|(combo, s)| {
            let v = combo[pi].1.as_str();
            match by.iter().find(|(x, _)| *x == v) {
                Some((_, t)) => *t == s,
                None => {
                    by.push((v, s));
                    true
                }
            }
        });
        if decides {
            let values: Vec<String> = by.iter().map(|(v, s)| format!("\"{v}\": {}", list(s))).collect();
            return Some(format!("{{\"property\": \"{pname}\", \"values\": {{{}}}}}", values.join(", ")));
        }
    }
    // By state: the most common shape is the default.
    let mut counts: Vec<(&Vec<crate::shapes::Box3>, usize)> = Vec::new();
    for (_, s) in shapes {
        match counts.iter_mut().find(|(t, _)| *t == s) {
            Some(c) => c.1 += 1,
            None => counts.push((s, 1)),
        }
    }
    let default = counts.iter().max_by_key(|(_, n)| *n)?.0;
    let states: Vec<String> = shapes
        .iter()
        .filter(|(_, s)| s != default)
        .map(|(combo, s)| {
            let key: Vec<String> = combo.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("\"{}\": {}", key.join(","), list(s))
        })
        .collect();
    Some(format!("{{\"states\": {{{}}}, \"default\": {}}}", states.join(", "), list(default)))
}

/// A number as JSON writes it.
fn num(x: f32) -> String {
    if x.fract() == 0.0 && x.abs() < 1e9 { format!("{x:.1}") } else { format!("{x}") }
}

/// The vanilla block whose sounds a `SoundType` value names.
fn sound_block(rt: &Rt, sound: V) -> Option<String> {
    let n = rt.str_of(rt.get_field(sound, "$name"))?;
    SOUNDS.iter().find(|(f, _)| *f == n).map(|(_, b)| b.to_string())
}

/// A map colour id from `mapColor(MapColor)`, `mapColor(DyeColor)` or `mapColor(state -> ...)`.
fn map_color(rt: &mut Rt, v: V, default_state: &str) -> Option<u8> {
    if v.r() == 0 {
        return None;
    }
    let v = match rt.class_of(v) {
        MAP_COLOR | DYE_COLOR => v,
        _ => {
            let st = api::state(rt, default_state);
            let r = rt.call_functional(APPLY, v, &[st]);
            rt.thrown = None;
            r
        }
    };
    match rt.class_of(v) {
        MAP_COLOR => Some(rt.get_field(v, "id").i().clamp(0, 61) as u8),
        DYE_COLOR => DYES.get(rt.get_field(v, "$ordinal").i() as usize).map(|d| d.1),
        _ => None,
    }
}

/// Every item the mod registered that isn't a block's own item, as `(id, declaration)`.
pub fn item_declarations(rt: &mut Rt) -> Vec<(String, String)> {
    let blocks: std::collections::HashSet<String> = rt.registered.keys().filter(|(r, _)| r == "block").map(|(_, n)| n.clone()).collect();
    let mut items: Vec<(String, V)> = rt
        .registered
        .iter()
        .filter(|((r, n), _)| r == "item" && own_id(n) && !blocks.contains(n))
        .map(|((_, n), v)| (n.clone(), *v))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
        .into_iter()
        .map(|(name, i)| {
            let props = rt.get_field(i, "$properties");
            let mut fields = vec!["\"template\": \"minecraft:stick\"".to_owned()];
            let durability = rt.get_field(props, "$durability").i();
            let stack = if durability > 0 { 1 } else { rt.get_field(props, "$stacksTo").i() };
            if props.r() != 0 && (1..=99).contains(&stack) && stack != 64 {
                fields.push(format!("\"max_stack_size\": {stack}"));
            }
            let rarity = rt.get_field(props, "$rarity");
            if props.r() != 0 && rarity.r() != 0 {
                let r = rt.str_of(rt.get_field(rarity, "$name")).unwrap_or("COMMON").to_ascii_lowercase();
                if r != "common" {
                    fields.push(format!("\"rarity\": \"{r}\""));
                }
            }
            let mut components = Vec::new();
            if durability > 0 {
                components.push(format!("\"minecraft:max_damage\": {durability}, \"minecraft:damage\": 0"));
            }
            components.extend(crate::food::components(rt, i, props));
            if !components.is_empty() {
                fields.push(format!("\"components\": {{{}}}", components.join(", ")));
            }
            (name, format!("{{{}}}", fields.join(", ")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OF: &str = "net/minecraft/world/level/block/state/BlockBehaviour$Properties.of.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;";

    fn props(rt: &mut Rt) -> V {
        api::call(rt, OF, &[])
    }

    /// Create names its windows `woodType.name() + "_window"`: each wood type keeps its own name,
    /// so `oak_window` and `dark_oak_window` stay two blocks.
    #[test]
    fn wood_types_answer_their_names() {
        let mut rt = Rt::new();
        let names: Vec<String> = ["OAK", "DARK_OAK", "CRIMSON"]
            .iter()
            .map(|f| {
                let w = api::get_static(&mut rt, Box::leak(format!("{WOOD_TYPE}.{f}").into_boxed_str()));
                let n = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/WoodType.name.()Ljava/lang/String;", &[w]);
                rt.str_of(n).unwrap_or("null").to_owned()
            })
            .collect();
        assert_eq!(names, ["oak", "dark_oak", "crimson"]);
    }

    #[test]
    fn a_vanilla_enum_property_reads_as_the_enum_constant_a_switch_compares() {
        let mut rt = Rt::new();
        crate::install();
        const BELL: &str = "net/minecraft/world/level/block/state/properties/BellAttachType";
        let prop = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.BELL_ATTACHMENT");
        let get = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
        let set = "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;";
        let state = api::state(&mut rt, "create:haunted_bell[attachment=ceiling,facing=north,powered=false]");
        let value = api::call(&mut rt, get, &[state, prop]);
        let ceiling = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BellAttachType.CEILING");
        assert_eq!(value, ceiling, "the state's value is the constant itself");
        let ordinal = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/BellAttachType.ordinal.()I", &[value]);
        assert_eq!(ordinal, V::I(1), "FLOOR, CEILING, SINGLE_WALL, DOUBLE_WALL");
        let all = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/BellAttachType.values.()[Lnet/minecraft/world/level/block/state/properties/BellAttachType;", &[]);
        let names: Vec<String> = jutil::elements(&rt, all).iter().map(|v| rt.str_of(rt.get_field(*v, "$name")).unwrap_or("").to_owned()).collect();
        assert_eq!(names, ["FLOOR", "CEILING", "SINGLE_WALL", "DOUBLE_WALL"]);
        let wall = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BellAttachType.DOUBLE_WALL");
        let moved = api::call(&mut rt, set, &[state, prop, wall]);
        assert_eq!(rt.str_of(moved), Some("create:haunted_bell[attachment=double_wall,facing=north,powered=false]"));
        assert_eq!(rt.class_of(wall), BELL);
    }

    #[test]
    fn properties_keep_what_a_declaration_needs() {
        let mut rt = Rt::new();
        let p = props(&mut rt);
        let metal = api::get_static(&mut rt, "net/minecraft/world/level/block/SoundType.METAL");
        let color = api::get_static(&mut rt, "net/minecraft/world/level/material/MapColor.COLOR_ORANGE");
        let k = |s: &str| -> &'static str { Box::leak(format!("{BLOCK_PROPERTIES}.{s}").into_boxed_str()) };
        let q = api::call(&mut rt, k("strength.(FF)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p, V::F(3.0), V::F(6.0)]);
        assert_eq!(q, p);
        api::call(&mut rt, k("sound.(Lnet/minecraft/world/level/block/SoundType;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p, metal]);
        api::call(&mut rt, k("mapColor.(Lnet/minecraft/world/level/material/MapColor;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p, color]);
        api::call(&mut rt, k("noOcclusion.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p]);
        api::call(&mut rt, k("requiresCorrectToolForDrops.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p]);
        // A plain block built with them, registered as `ruby:gear` with an item.
        let b = rt.new_object("net/minecraft/world/level/block/Block");
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[b, p]);
        rt.registered.insert(("block".into(), "ruby:gear".into()), b);
        rt.registered.insert(("item".into(), "ruby:gear".into()), V::NULL);
        let decl = block_declarations(&mut rt);
        assert_eq!(decl.len(), 1);
        let (id, json) = &decl[0];
        assert_eq!(id, "ruby:gear");
        for part in [
            "\"template\": \"minecraft:glass\"",
            "\"item\": true",
            "\"hardness\": 3.0",
            "\"explosion_resistance\": 6.0",
            "\"requires_tool\": true",
            "\"sounds\": \"minecraft:iron_block\"",
            "\"map_color\": 15",
        ] {
            assert!(json.contains(part), "{part} in {json}");
        }
    }

    #[test]
    fn copied_properties_take_the_vanilla_blocks_values() {
        let mut rt = Rt::new();
        let andesite = api::get_static(&mut rt, "net/minecraft/world/level/block/Blocks.ANDESITE");
        let p = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.ofFullCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[andesite]);
        let b = rt.new_object("net/minecraft/world/level/block/Block");
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[b, p]);
        rt.registered.insert(("block".into(), "ruby:casing".into()), b);
        let (_, json) = block_declarations(&mut rt).remove(0);
        for part in ["\"template\": \"minecraft:stone\"", "\"item\": false", "\"hardness\": 1.5", "\"sounds\": \"minecraft:andesite\"", "\"map_color\": 11"] {
            assert!(json.contains(part), "{part} in {json}");
        }
    }

    #[test]
    fn a_pillar_declares_its_axis_and_the_default_its_constructor_sets() {
        // A mod block extending `RotatedPillarBlock` without its own definition: vanilla's adds the
        // axis; the template is a pillar, so the axis is the template's.
        let mut rt = Rt::new();
        let p = props(&mut rt);
        let b = rt.new_object("net/minecraft/world/level/block/RotatedPillarBlock");
        api::call(&mut rt, "net/minecraft/world/level/block/RotatedPillarBlock.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[b, p]);
        // As vanilla's `RotatedPillarBlock` constructor leaves it: along y.
        let d = api::call(&mut rt, "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;", &[b]);
        assert_eq!(rt.str_of(d), Some("[axis=y]"));
        // Its own constructor: `registerDefaultState(defaultBlockState().setValue(AXIS, Z))`.
        let axis = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.AXIS");
        let z = api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.Z");
        let s = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;", &[d, axis, z]);
        api::call(&mut rt, "net/minecraft/world/level/block/Block.registerDefaultState.(Lnet/minecraft/world/level/block/state/BlockState;)V", &[b, s]);
        rt.registered.insert(("block".into(), "ruby:log".into()), b);
        let d = api::call(&mut rt, "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;", &[b]);
        assert_eq!(rt.str_of(d), Some("ruby:log[axis=z]"));
        let (_, json) = block_declarations(&mut rt).remove(0);
        assert!(json.contains("\"template\": \"minecraft:quartz_pillar\""), "{json}");
        assert!(json.contains("\"state_properties\": {}"), "{json}");
        assert!(json.contains("\"default_state\": {\"axis\": \"z\"}"), "{json}");
    }

    #[test]
    fn mod_properties_take_their_own_values() {
        let mut rt = Rt::new();
        let n = rt.string("level");
        let level = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/IntegerProperty.create.(Ljava/lang/String;II)Lnet/minecraft/world/level/block/state/properties/IntegerProperty;", &[n, V::I(1), V::I(4)]);
        let n2 = rt.string("level");
        let other = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/IntegerProperty.create.(Ljava/lang/String;II)Lnet/minecraft/world/level/block/state/properties/IntegerProperty;", &[n2, V::I(0), V::I(2)]);
        assert_ne!(level, other);
        assert_eq!(property_values(&mut rt, level), ["1", "2", "3", "4"]);
        assert_eq!(property_values(&mut rt, other), ["0", "1", "2"]);
        // `FACING` and `HORIZONTAL_FACING` share a name, not their values.
        let facing = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.FACING");
        let horizontal = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.HORIZONTAL_FACING");
        assert_eq!(property_values(&mut rt, facing).len(), 6);
        assert_eq!(property_values(&mut rt, horizontal), ["north", "east", "south", "west"]);
    }

    /// `Integer::compareTo`, as the translator calls a method reference.
    fn integer_compare_to(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        Some(api::call(rt, "java/lang/Integer.compareTo.(Ljava/lang/Integer;)I", &[args[1], args[2]]))
    }

    #[test]
    fn vanilla_class_properties_list_the_values_26_3_gives_them() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(integer_compare_to);
        const POSSIBLE: &str = "net/minecraft/world/level/block/state/properties/IntegerProperty.getPossibleValues.()Ljava/util/Collection;";
        let ints = |rt: &mut Rt, key: &'static str| {
            let p = api::get_static(rt, key);
            let values = api::call(rt, POSSIBLE, &[p]);
            let name = api_common::prop_parts(rt, p).0;
            (name, jutil::elements(rt, values).iter().map(|v| rt.get_field(*v, "value").i()).collect::<Vec<_>>())
        };
        // Values read from 26.3's BlockStateProperties and block classes.
        assert_eq!(ints(&mut rt, "net/minecraft/world/level/block/LayeredCauldronBlock.LEVEL"), ("level".into(), vec![1, 2, 3]));
        assert_eq!(ints(&mut rt, "net/minecraft/world/level/block/ComposterBlock.LEVEL").1, (0..=8).collect::<Vec<_>>());
        assert_eq!(ints(&mut rt, "net/minecraft/world/level/block/FarmBlock.MOISTURE").1, (0..=7).collect::<Vec<_>>());
        assert_eq!(ints(&mut rt, "net/minecraft/world/level/block/BeehiveBlock.HONEY_LEVEL"), ("honey_level".into(), (0..=5).collect()));
        assert_eq!(ints(&mut rt, "net/minecraft/world/level/block/RespawnAnchorBlock.CHARGE"), ("charges".into(), (0..=4).collect()));
        // Create's spout: `LEVEL.getPossibleValues().stream().max(Integer::compareTo).orElseThrow()`.
        let level = api::get_static(&mut rt, "net/minecraft/world/level/block/LayeredCauldronBlock.LEVEL");
        let values = api::call(&mut rt, POSSIBLE, &[level]);
        let stream = api::call(&mut rt, "java/util/Collection.stream.()Ljava/util/stream/Stream;", &[values]);
        let cmp = rt.lambda_id("java/util/Comparator", 1, Vec::new());
        let max = api::call(&mut rt, "java/util/stream/Stream.max.(Ljava/util/Comparator;)Ljava/util/Optional;", &[stream, cmp]);
        let top = api::call(&mut rt, "java/util/Optional.orElseThrow.()Ljava/lang/Object;", &[max]);
        assert!(rt.thrown.is_none());
        assert_eq!(api::call(&mut rt, "java/lang/Integer.intValue.()I", &[top]), V::I(3));
        // The hopper's facing has no `up`; a wall's sides are low or tall, not booleans.
        let hopper = api::get_static(&mut rt, "net/minecraft/world/level/block/HopperBlock.FACING");
        assert_eq!(property_values(&mut rt, hopper), ["down", "north", "south", "west", "east"]);
        let wall = api::get_static(&mut rt, "net/minecraft/world/level/block/WallBlock.NORTH");
        assert_eq!(property_values(&mut rt, wall), ["none", "low", "tall"]);
    }

    #[test]
    fn vanilla_block_classes_hold_the_properties_vanilla_gives_them() {
        let mut rt = Rt::new();
        let field = |rt: &mut Rt, key: &'static str| {
            let p = api::get_static(rt, key);
            (api_common::prop_parts(rt, p).0, property_values(rt, p))
        };
        // A horizontal block's FACING is the four-way one; a directional block's has six.
        let (name, values) = field(&mut rt, "net/minecraft/world/level/block/HorizontalDirectionalBlock.FACING");
        assert_eq!((name.as_str(), values.len()), ("facing", 4));
        assert_eq!(field(&mut rt, "net/minecraft/world/level/block/DirectionalBlock.FACING").1.len(), 6);
        assert_eq!(field(&mut rt, "net/minecraft/world/level/block/SlabBlock.TYPE").0, "type");
        assert_eq!(field(&mut rt, "net/minecraft/world/level/block/DoorBlock.HALF").1, ["upper", "lower"]);
        assert_eq!(field(&mut rt, "net/minecraft/world/level/block/LeverBlock.POWERED").0, "powered");
        // A pipe's sides, by direction.
        let by = api::get_static(&mut rt, "net/minecraft/world/level/block/PipeBlock.PROPERTY_BY_DIRECTION");
        let up = api_common::direction(&mut rt, 1);
        let p = jutil::table_get(&mut rt, by, up).unwrap();
        assert_eq!(api_common::prop_parts(&rt, p).0, "up");
    }

    /// What Create's movement checks ask of a block before a contraption takes it: a vanilla block's
    /// answers are the game's, a mod block's what its properties set (over the vanilla block they
    /// copy), and the push reaction is the constant a `!=` compares.
    #[test]
    fn a_states_destroy_time_push_reaction_and_replaceability_come_from_its_properties() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        host.facts.insert("minecraft:oak_planks".into(), BlockFacts { hardness: 2.0, push: 0, replaceable: false });
        host.facts.insert("minecraft:obsidian".into(), BlockFacts { hardness: 50.0, push: 2, replaceable: false });
        host.facts.insert("minecraft:short_grass".into(), BlockFacts { hardness: 0.0, push: 1, replaceable: true });
        let k = |s: &str| -> &'static str { Box::leak(format!("{BLOCK_PROPERTIES}.{s}").into_boxed_str()) };
        // A mod block pistons can't move, which nothing breaks.
        let p = props(&mut rt);
        let blocked = api::get_static(&mut rt, "net/minecraft/world/level/material/PushReaction.BLOCK");
        api::call(&mut rt, k("pushReaction.(Lnet/minecraft/world/level/material/PushReaction;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p, blocked]);
        api::call(&mut rt, k("strength.(F)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[p, V::F(-1.0)]);
        let b = rt.new_object("net/minecraft/world/level/block/Block");
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[b, p]);
        rt.registered.insert(("block".into(), "ruby:anchor".into()), b);
        // A mod block copying obsidian, made replaceable.
        let obsidian = api::get_static(&mut rt, "net/minecraft/world/level/block/Blocks.OBSIDIAN");
        let q = api::call(&mut rt, k("ofFullCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[obsidian]);
        api::call(&mut rt, k("replaceable.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;"), &[q]);
        let c = rt.new_object("net/minecraft/world/level/block/Block");
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[c, q]);
        rt.registered.insert(("block".into(), "ruby:dark_glass".into()), c);

        const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
        let ask = |rt: &mut Rt, state: &str| {
            let st = api::state(rt, state);
            let push = api::call(rt, Box::leak(format!("{STATE}.getPistonPushReaction.()Lnet/minecraft/world/level/material/PushReaction;").into_boxed_str()), &[st]);
            let speed = api::call(rt, Box::leak(format!("{STATE}.getDestroySpeed.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F").into_boxed_str()), &[st, V::NULL, V::NULL]);
            let replace = api::call(rt, Box::leak(format!("{STATE}.canBeReplaced.()Z").into_boxed_str()), &[st]);
            let name = rt.str_of(rt.get_field(push, "$name")).unwrap_or("null").to_owned();
            (name, speed.f(), replace.i())
        };
        let answers = rt.with_host(&mut host, |rt| {
            ["minecraft:oak_planks", "minecraft:obsidian", "minecraft:short_grass", "ruby:anchor", "ruby:dark_glass"].map(|s| ask(rt, s))
        });
        assert_eq!(
            answers,
            [
                ("NORMAL".to_owned(), 2.0, 0),
                ("BLOCK".to_owned(), 50.0, 0),
                ("DESTROY".to_owned(), 0.0, 1),
                ("BLOCK".to_owned(), -1.0, 0),
                ("BLOCK".to_owned(), 50.0, 1),
            ]
        );
        // The constant a state gives is the one the static field holds (`!= PushReaction.BLOCK`).
        let push = rt.with_host(&mut host, |rt| push_reaction(rt, "ruby:anchor"));
        assert_eq!(push, blocked);
    }
}
