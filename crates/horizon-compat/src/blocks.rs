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
/// `MapColor.col` by id: the colour each constant was built with.
const MAP_COLOR_VALUES: [i32; 62] = [
    0, 8368696, 16247203, 13092807, 16711680, 10526975, 10987431, 31744, 16777215, 10791096, 9923917, 7368816, 4210943, 9402184,
    16776437, 14188339, 11685080, 6724056, 15066419, 8375321, 15892389, 5000268, 10066329, 5013401, 8339378, 3361970, 6704179,
    6717235, 10040115, 1644825, 16445005, 6085589, 4882687, 55610, 8476209, 7340544, 13742497, 10441252, 9787244, 7367818,
    12223780, 6780213, 10505550, 3746083, 8874850, 5725276, 8014168, 4996700, 4993571, 5001770, 9321518, 2430480, 12398641,
    9715553, 6035741, 1474182, 3837580, 5647422, 1356933, 6579300, 14200723, 8365974,
];

/// The `MapColor` constant with id `id`, its fields `id` and `col` set.
fn map_color_constant(rt: &mut Rt, id: usize) -> V {
    let id = id.min(MAP_COLORS.len() - 1);
    let o = named(rt, MAP_COLOR, MAP_COLORS[id]);
    rt.put_field(o, "id", V::I(id as i32));
    rt.put_field(o, "col", V::I(MAP_COLOR_VALUES[id]));
    o
}

/// `DyeColor`'s constants in order, with the map colour each one's blocks take.
pub(crate) const DYES: [(&str, u8); 16] = [
    ("WHITE", 8), ("ORANGE", 15), ("MAGENTA", 16), ("LIGHT_BLUE", 17), ("YELLOW", 18), ("LIME", 19), ("PINK", 20), ("GRAY", 21),
    ("LIGHT_GRAY", 22), ("CYAN", 23), ("PURPLE", 24), ("BLUE", 25), ("BROWN", 26), ("GREEN", 27), ("RED", 28), ("BLACK", 29),
];

/// `SoundType` constants and a vanilla block that sounds the same, for a declaration's `sounds`.
const SOUNDS: [(&str, &str); 33] = [
    ("WOOD", "minecraft:oak_planks"), ("STONE", "minecraft:stone"), ("METAL", "minecraft:iron_block"), ("GLASS", "minecraft:glass"),
    ("WOOL", "minecraft:white_wool"), ("SAND", "minecraft:sand"), ("GRAVEL", "minecraft:gravel"), ("GRASS", "minecraft:grass_block"),
    ("SNOW", "minecraft:snow_block"), ("LADDER", "minecraft:ladder"), ("ANVIL", "minecraft:anvil"), ("SLIME_BLOCK", "minecraft:slime_block"),
    ("HONEY_BLOCK", "minecraft:honey_block"), ("NETHERITE_BLOCK", "minecraft:netherite_block"), ("COPPER", "minecraft:copper_block"),
    ("LANTERN", "minecraft:lantern"), ("NETHER_BRICKS", "minecraft:nether_bricks"), ("DEEPSLATE", "minecraft:deepslate"),
    ("POLISHED_DEEPSLATE", "minecraft:polished_deepslate"), ("DEEPSLATE_BRICKS", "minecraft:deepslate_bricks"),
    ("DEEPSLATE_TILES", "minecraft:deepslate_tiles"), ("TUFF", "minecraft:tuff"), ("CALCITE", "minecraft:calcite"),
    ("AMETHYST", "minecraft:amethyst_block"), ("BASALT", "minecraft:basalt"), ("BONE_BLOCK", "minecraft:bone_block"),
    ("SCAFFOLDING", "minecraft:scaffolding"), ("NETHERRACK", "minecraft:netherrack"), ("SOUL_SAND", "minecraft:soul_sand"),
    ("MUD_BRICKS", "minecraft:mud_bricks"), ("CHERRY_WOOD", "minecraft:cherry_planks"), ("NETHER_WOOD", "minecraft:crimson_planks"),
    ("BAMBOO_WOOD", "minecraft:bamboo_planks"),
];

/// Vanilla 1.21.1's `SoundType` constants (from its class file): volume, pitch, and the
/// `SoundEvents` it breaks, steps, is placed, hit and fallen on with.
const VANILLA_SOUND_TYPES: [(&str, f32, f32, [&str; 5]); 113] = [
    ("EMPTY", 1.0, 1.0, ["EMPTY", "EMPTY", "EMPTY", "EMPTY", "EMPTY"]),
    ("WOOD", 1.0, 1.0, ["WOOD_BREAK", "WOOD_STEP", "WOOD_PLACE", "WOOD_HIT", "WOOD_FALL"]),
    ("GRAVEL", 1.0, 1.0, ["GRAVEL_BREAK", "GRAVEL_STEP", "GRAVEL_PLACE", "GRAVEL_HIT", "GRAVEL_FALL"]),
    ("GRASS", 1.0, 1.0, ["GRASS_BREAK", "GRASS_STEP", "GRASS_PLACE", "GRASS_HIT", "GRASS_FALL"]),
    ("LILY_PAD", 1.0, 1.0, ["BIG_DRIPLEAF_BREAK", "BIG_DRIPLEAF_STEP", "LILY_PAD_PLACE", "BIG_DRIPLEAF_HIT", "BIG_DRIPLEAF_FALL"]),
    ("STONE", 1.0, 1.0, ["STONE_BREAK", "STONE_STEP", "STONE_PLACE", "STONE_HIT", "STONE_FALL"]),
    ("METAL", 1.0, 1.5, ["METAL_BREAK", "METAL_STEP", "METAL_PLACE", "METAL_HIT", "METAL_FALL"]),
    ("GLASS", 1.0, 1.0, ["GLASS_BREAK", "GLASS_STEP", "GLASS_PLACE", "GLASS_HIT", "GLASS_FALL"]),
    ("WOOL", 1.0, 1.0, ["WOOL_BREAK", "WOOL_STEP", "WOOL_PLACE", "WOOL_HIT", "WOOL_FALL"]),
    ("SAND", 1.0, 1.0, ["SAND_BREAK", "SAND_STEP", "SAND_PLACE", "SAND_HIT", "SAND_FALL"]),
    ("SNOW", 1.0, 1.0, ["SNOW_BREAK", "SNOW_STEP", "SNOW_PLACE", "SNOW_HIT", "SNOW_FALL"]),
    ("POWDER_SNOW", 1.0, 1.0, ["POWDER_SNOW_BREAK", "POWDER_SNOW_STEP", "POWDER_SNOW_PLACE", "POWDER_SNOW_HIT", "POWDER_SNOW_FALL"]),
    ("LADDER", 1.0, 1.0, ["LADDER_BREAK", "LADDER_STEP", "LADDER_PLACE", "LADDER_HIT", "LADDER_FALL"]),
    ("ANVIL", 0.3, 1.0, ["ANVIL_BREAK", "ANVIL_STEP", "ANVIL_PLACE", "ANVIL_HIT", "ANVIL_FALL"]),
    ("SLIME_BLOCK", 1.0, 1.0, ["SLIME_BLOCK_BREAK", "SLIME_BLOCK_STEP", "SLIME_BLOCK_PLACE", "SLIME_BLOCK_HIT", "SLIME_BLOCK_FALL"]),
    ("HONEY_BLOCK", 1.0, 1.0, ["HONEY_BLOCK_BREAK", "HONEY_BLOCK_STEP", "HONEY_BLOCK_PLACE", "HONEY_BLOCK_HIT", "HONEY_BLOCK_FALL"]),
    ("WET_GRASS", 1.0, 1.0, ["WET_GRASS_BREAK", "WET_GRASS_STEP", "WET_GRASS_PLACE", "WET_GRASS_HIT", "WET_GRASS_FALL"]),
    ("CORAL_BLOCK", 1.0, 1.0, ["CORAL_BLOCK_BREAK", "CORAL_BLOCK_STEP", "CORAL_BLOCK_PLACE", "CORAL_BLOCK_HIT", "CORAL_BLOCK_FALL"]),
    ("BAMBOO", 1.0, 1.0, ["BAMBOO_BREAK", "BAMBOO_STEP", "BAMBOO_PLACE", "BAMBOO_HIT", "BAMBOO_FALL"]),
    ("BAMBOO_SAPLING", 1.0, 1.0, ["BAMBOO_SAPLING_BREAK", "BAMBOO_STEP", "BAMBOO_SAPLING_PLACE", "BAMBOO_SAPLING_HIT", "BAMBOO_FALL"]),
    ("SCAFFOLDING", 1.0, 1.0, ["SCAFFOLDING_BREAK", "SCAFFOLDING_STEP", "SCAFFOLDING_PLACE", "SCAFFOLDING_HIT", "SCAFFOLDING_FALL"]),
    ("SWEET_BERRY_BUSH", 1.0, 1.0, ["SWEET_BERRY_BUSH_BREAK", "GRASS_STEP", "SWEET_BERRY_BUSH_PLACE", "GRASS_HIT", "GRASS_FALL"]),
    ("CROP", 1.0, 1.0, ["CROP_BREAK", "GRASS_STEP", "CROP_PLANTED", "GRASS_HIT", "GRASS_FALL"]),
    ("HARD_CROP", 1.0, 1.0, ["WOOD_BREAK", "WOOD_STEP", "CROP_PLANTED", "WOOD_HIT", "WOOD_FALL"]),
    ("VINE", 1.0, 1.0, ["VINE_BREAK", "VINE_STEP", "VINE_PLACE", "VINE_HIT", "VINE_FALL"]),
    ("NETHER_WART", 1.0, 1.0, ["NETHER_WART_BREAK", "STONE_STEP", "NETHER_WART_PLANTED", "STONE_HIT", "STONE_FALL"]),
    ("LANTERN", 1.0, 1.0, ["LANTERN_BREAK", "LANTERN_STEP", "LANTERN_PLACE", "LANTERN_HIT", "LANTERN_FALL"]),
    ("STEM", 1.0, 1.0, ["STEM_BREAK", "STEM_STEP", "STEM_PLACE", "STEM_HIT", "STEM_FALL"]),
    ("NYLIUM", 1.0, 1.0, ["NYLIUM_BREAK", "NYLIUM_STEP", "NYLIUM_PLACE", "NYLIUM_HIT", "NYLIUM_FALL"]),
    ("FUNGUS", 1.0, 1.0, ["FUNGUS_BREAK", "FUNGUS_STEP", "FUNGUS_PLACE", "FUNGUS_HIT", "FUNGUS_FALL"]),
    ("ROOTS", 1.0, 1.0, ["ROOTS_BREAK", "ROOTS_STEP", "ROOTS_PLACE", "ROOTS_HIT", "ROOTS_FALL"]),
    ("SHROOMLIGHT", 1.0, 1.0, ["SHROOMLIGHT_BREAK", "SHROOMLIGHT_STEP", "SHROOMLIGHT_PLACE", "SHROOMLIGHT_HIT", "SHROOMLIGHT_FALL"]),
    ("WEEPING_VINES", 1.0, 1.0, ["WEEPING_VINES_BREAK", "WEEPING_VINES_STEP", "WEEPING_VINES_PLACE", "WEEPING_VINES_HIT", "WEEPING_VINES_FALL"]),
    ("TWISTING_VINES", 1.0, 0.5, ["WEEPING_VINES_BREAK", "WEEPING_VINES_STEP", "WEEPING_VINES_PLACE", "WEEPING_VINES_HIT", "WEEPING_VINES_FALL"]),
    ("SOUL_SAND", 1.0, 1.0, ["SOUL_SAND_BREAK", "SOUL_SAND_STEP", "SOUL_SAND_PLACE", "SOUL_SAND_HIT", "SOUL_SAND_FALL"]),
    ("SOUL_SOIL", 1.0, 1.0, ["SOUL_SOIL_BREAK", "SOUL_SOIL_STEP", "SOUL_SOIL_PLACE", "SOUL_SOIL_HIT", "SOUL_SOIL_FALL"]),
    ("BASALT", 1.0, 1.0, ["BASALT_BREAK", "BASALT_STEP", "BASALT_PLACE", "BASALT_HIT", "BASALT_FALL"]),
    ("WART_BLOCK", 1.0, 1.0, ["WART_BLOCK_BREAK", "WART_BLOCK_STEP", "WART_BLOCK_PLACE", "WART_BLOCK_HIT", "WART_BLOCK_FALL"]),
    ("NETHERRACK", 1.0, 1.0, ["NETHERRACK_BREAK", "NETHERRACK_STEP", "NETHERRACK_PLACE", "NETHERRACK_HIT", "NETHERRACK_FALL"]),
    ("NETHER_BRICKS", 1.0, 1.0, ["NETHER_BRICKS_BREAK", "NETHER_BRICKS_STEP", "NETHER_BRICKS_PLACE", "NETHER_BRICKS_HIT", "NETHER_BRICKS_FALL"]),
    ("NETHER_SPROUTS", 1.0, 1.0, ["NETHER_SPROUTS_BREAK", "NETHER_SPROUTS_STEP", "NETHER_SPROUTS_PLACE", "NETHER_SPROUTS_HIT", "NETHER_SPROUTS_FALL"]),
    ("NETHER_ORE", 1.0, 1.0, ["NETHER_ORE_BREAK", "NETHER_ORE_STEP", "NETHER_ORE_PLACE", "NETHER_ORE_HIT", "NETHER_ORE_FALL"]),
    ("BONE_BLOCK", 1.0, 1.0, ["BONE_BLOCK_BREAK", "BONE_BLOCK_STEP", "BONE_BLOCK_PLACE", "BONE_BLOCK_HIT", "BONE_BLOCK_FALL"]),
    ("NETHERITE_BLOCK", 1.0, 1.0, ["NETHERITE_BLOCK_BREAK", "NETHERITE_BLOCK_STEP", "NETHERITE_BLOCK_PLACE", "NETHERITE_BLOCK_HIT", "NETHERITE_BLOCK_FALL"]),
    ("ANCIENT_DEBRIS", 1.0, 1.0, ["ANCIENT_DEBRIS_BREAK", "ANCIENT_DEBRIS_STEP", "ANCIENT_DEBRIS_PLACE", "ANCIENT_DEBRIS_HIT", "ANCIENT_DEBRIS_FALL"]),
    ("LODESTONE", 1.0, 1.0, ["LODESTONE_BREAK", "LODESTONE_STEP", "LODESTONE_PLACE", "LODESTONE_HIT", "LODESTONE_FALL"]),
    ("CHAIN", 1.0, 1.0, ["CHAIN_BREAK", "CHAIN_STEP", "CHAIN_PLACE", "CHAIN_HIT", "CHAIN_FALL"]),
    ("NETHER_GOLD_ORE", 1.0, 1.0, ["NETHER_GOLD_ORE_BREAK", "NETHER_GOLD_ORE_STEP", "NETHER_GOLD_ORE_PLACE", "NETHER_GOLD_ORE_HIT", "NETHER_GOLD_ORE_FALL"]),
    ("GILDED_BLACKSTONE", 1.0, 1.0, ["GILDED_BLACKSTONE_BREAK", "GILDED_BLACKSTONE_STEP", "GILDED_BLACKSTONE_PLACE", "GILDED_BLACKSTONE_HIT", "GILDED_BLACKSTONE_FALL"]),
    ("CANDLE", 1.0, 1.0, ["CANDLE_BREAK", "CANDLE_STEP", "CANDLE_PLACE", "CANDLE_HIT", "CANDLE_FALL"]),
    ("AMETHYST", 1.0, 1.0, ["AMETHYST_BLOCK_BREAK", "AMETHYST_BLOCK_STEP", "AMETHYST_BLOCK_PLACE", "AMETHYST_BLOCK_HIT", "AMETHYST_BLOCK_FALL"]),
    ("AMETHYST_CLUSTER", 1.0, 1.0, ["AMETHYST_CLUSTER_BREAK", "AMETHYST_CLUSTER_STEP", "AMETHYST_CLUSTER_PLACE", "AMETHYST_CLUSTER_HIT", "AMETHYST_CLUSTER_FALL"]),
    ("SMALL_AMETHYST_BUD", 1.0, 1.0, ["SMALL_AMETHYST_BUD_BREAK", "AMETHYST_CLUSTER_STEP", "SMALL_AMETHYST_BUD_PLACE", "AMETHYST_CLUSTER_HIT", "AMETHYST_CLUSTER_FALL"]),
    ("MEDIUM_AMETHYST_BUD", 1.0, 1.0, ["MEDIUM_AMETHYST_BUD_BREAK", "AMETHYST_CLUSTER_STEP", "MEDIUM_AMETHYST_BUD_PLACE", "AMETHYST_CLUSTER_HIT", "AMETHYST_CLUSTER_FALL"]),
    ("LARGE_AMETHYST_BUD", 1.0, 1.0, ["LARGE_AMETHYST_BUD_BREAK", "AMETHYST_CLUSTER_STEP", "LARGE_AMETHYST_BUD_PLACE", "AMETHYST_CLUSTER_HIT", "AMETHYST_CLUSTER_FALL"]),
    ("TUFF", 1.0, 1.0, ["TUFF_BREAK", "TUFF_STEP", "TUFF_PLACE", "TUFF_HIT", "TUFF_FALL"]),
    ("TUFF_BRICKS", 1.0, 1.0, ["TUFF_BRICKS_BREAK", "TUFF_BRICKS_STEP", "TUFF_BRICKS_PLACE", "TUFF_BRICKS_HIT", "TUFF_BRICKS_FALL"]),
    ("POLISHED_TUFF", 1.0, 1.0, ["POLISHED_TUFF_BREAK", "POLISHED_TUFF_STEP", "POLISHED_TUFF_PLACE", "POLISHED_TUFF_HIT", "POLISHED_TUFF_FALL"]),
    ("CALCITE", 1.0, 1.0, ["CALCITE_BREAK", "CALCITE_STEP", "CALCITE_PLACE", "CALCITE_HIT", "CALCITE_FALL"]),
    ("DRIPSTONE_BLOCK", 1.0, 1.0, ["DRIPSTONE_BLOCK_BREAK", "DRIPSTONE_BLOCK_STEP", "DRIPSTONE_BLOCK_PLACE", "DRIPSTONE_BLOCK_HIT", "DRIPSTONE_BLOCK_FALL"]),
    ("POINTED_DRIPSTONE", 1.0, 1.0, ["POINTED_DRIPSTONE_BREAK", "POINTED_DRIPSTONE_STEP", "POINTED_DRIPSTONE_PLACE", "POINTED_DRIPSTONE_HIT", "POINTED_DRIPSTONE_FALL"]),
    ("COPPER", 1.0, 1.0, ["COPPER_BREAK", "COPPER_STEP", "COPPER_PLACE", "COPPER_HIT", "COPPER_FALL"]),
    ("COPPER_BULB", 1.0, 1.0, ["COPPER_BULB_BREAK", "COPPER_BULB_STEP", "COPPER_BULB_PLACE", "COPPER_BULB_HIT", "COPPER_BULB_FALL"]),
    ("COPPER_GRATE", 1.0, 1.0, ["COPPER_GRATE_BREAK", "COPPER_GRATE_STEP", "COPPER_GRATE_PLACE", "COPPER_GRATE_HIT", "COPPER_GRATE_FALL"]),
    ("CAVE_VINES", 1.0, 1.0, ["CAVE_VINES_BREAK", "CAVE_VINES_STEP", "CAVE_VINES_PLACE", "CAVE_VINES_HIT", "CAVE_VINES_FALL"]),
    ("SPORE_BLOSSOM", 1.0, 1.0, ["SPORE_BLOSSOM_BREAK", "SPORE_BLOSSOM_STEP", "SPORE_BLOSSOM_PLACE", "SPORE_BLOSSOM_HIT", "SPORE_BLOSSOM_FALL"]),
    ("AZALEA", 1.0, 1.0, ["AZALEA_BREAK", "AZALEA_STEP", "AZALEA_PLACE", "AZALEA_HIT", "AZALEA_FALL"]),
    ("FLOWERING_AZALEA", 1.0, 1.0, ["FLOWERING_AZALEA_BREAK", "FLOWERING_AZALEA_STEP", "FLOWERING_AZALEA_PLACE", "FLOWERING_AZALEA_HIT", "FLOWERING_AZALEA_FALL"]),
    ("MOSS_CARPET", 1.0, 1.0, ["MOSS_CARPET_BREAK", "MOSS_CARPET_STEP", "MOSS_CARPET_PLACE", "MOSS_CARPET_HIT", "MOSS_CARPET_FALL"]),
    ("PINK_PETALS", 1.0, 1.0, ["PINK_PETALS_BREAK", "PINK_PETALS_STEP", "PINK_PETALS_PLACE", "PINK_PETALS_HIT", "PINK_PETALS_FALL"]),
    ("MOSS", 1.0, 1.0, ["MOSS_BREAK", "MOSS_STEP", "MOSS_PLACE", "MOSS_HIT", "MOSS_FALL"]),
    ("BIG_DRIPLEAF", 1.0, 1.0, ["BIG_DRIPLEAF_BREAK", "BIG_DRIPLEAF_STEP", "BIG_DRIPLEAF_PLACE", "BIG_DRIPLEAF_HIT", "BIG_DRIPLEAF_FALL"]),
    ("SMALL_DRIPLEAF", 1.0, 1.0, ["SMALL_DRIPLEAF_BREAK", "SMALL_DRIPLEAF_STEP", "SMALL_DRIPLEAF_PLACE", "SMALL_DRIPLEAF_HIT", "SMALL_DRIPLEAF_FALL"]),
    ("ROOTED_DIRT", 1.0, 1.0, ["ROOTED_DIRT_BREAK", "ROOTED_DIRT_STEP", "ROOTED_DIRT_PLACE", "ROOTED_DIRT_HIT", "ROOTED_DIRT_FALL"]),
    ("HANGING_ROOTS", 1.0, 1.0, ["HANGING_ROOTS_BREAK", "HANGING_ROOTS_STEP", "HANGING_ROOTS_PLACE", "HANGING_ROOTS_HIT", "HANGING_ROOTS_FALL"]),
    ("AZALEA_LEAVES", 1.0, 1.0, ["AZALEA_LEAVES_BREAK", "AZALEA_LEAVES_STEP", "AZALEA_LEAVES_PLACE", "AZALEA_LEAVES_HIT", "AZALEA_LEAVES_FALL"]),
    ("SCULK_SENSOR", 1.0, 1.0, ["SCULK_SENSOR_BREAK", "SCULK_SENSOR_STEP", "SCULK_SENSOR_PLACE", "SCULK_SENSOR_HIT", "SCULK_SENSOR_FALL"]),
    ("SCULK_CATALYST", 1.0, 1.0, ["SCULK_CATALYST_BREAK", "SCULK_CATALYST_STEP", "SCULK_CATALYST_PLACE", "SCULK_CATALYST_HIT", "SCULK_CATALYST_FALL"]),
    ("SCULK", 1.0, 1.0, ["SCULK_BLOCK_BREAK", "SCULK_BLOCK_STEP", "SCULK_BLOCK_PLACE", "SCULK_BLOCK_HIT", "SCULK_BLOCK_FALL"]),
    ("SCULK_VEIN", 1.0, 1.0, ["SCULK_VEIN_BREAK", "SCULK_VEIN_STEP", "SCULK_VEIN_PLACE", "SCULK_VEIN_HIT", "SCULK_VEIN_FALL"]),
    ("SCULK_SHRIEKER", 1.0, 1.0, ["SCULK_SHRIEKER_BREAK", "SCULK_SHRIEKER_STEP", "SCULK_SHRIEKER_PLACE", "SCULK_SHRIEKER_HIT", "SCULK_SHRIEKER_FALL"]),
    ("GLOW_LICHEN", 1.0, 1.0, ["GRASS_BREAK", "VINE_STEP", "GRASS_PLACE", "GRASS_HIT", "GRASS_FALL"]),
    ("DEEPSLATE", 1.0, 1.0, ["DEEPSLATE_BREAK", "DEEPSLATE_STEP", "DEEPSLATE_PLACE", "DEEPSLATE_HIT", "DEEPSLATE_FALL"]),
    ("DEEPSLATE_BRICKS", 1.0, 1.0, ["DEEPSLATE_BRICKS_BREAK", "DEEPSLATE_BRICKS_STEP", "DEEPSLATE_BRICKS_PLACE", "DEEPSLATE_BRICKS_HIT", "DEEPSLATE_BRICKS_FALL"]),
    ("DEEPSLATE_TILES", 1.0, 1.0, ["DEEPSLATE_TILES_BREAK", "DEEPSLATE_TILES_STEP", "DEEPSLATE_TILES_PLACE", "DEEPSLATE_TILES_HIT", "DEEPSLATE_TILES_FALL"]),
    ("POLISHED_DEEPSLATE", 1.0, 1.0, ["POLISHED_DEEPSLATE_BREAK", "POLISHED_DEEPSLATE_STEP", "POLISHED_DEEPSLATE_PLACE", "POLISHED_DEEPSLATE_HIT", "POLISHED_DEEPSLATE_FALL"]),
    ("FROGLIGHT", 1.0, 1.0, ["FROGLIGHT_BREAK", "FROGLIGHT_STEP", "FROGLIGHT_PLACE", "FROGLIGHT_HIT", "FROGLIGHT_FALL"]),
    ("FROGSPAWN", 1.0, 1.0, ["FROGSPAWN_BREAK", "FROGSPAWNSTEP", "FROGSPAWN_PLACE", "FROGSPAWN_HIT", "FROGSPAWN_FALL"]),
    ("MANGROVE_ROOTS", 1.0, 1.0, ["MANGROVE_ROOTS_BREAK", "MANGROVE_ROOTS_STEP", "MANGROVE_ROOTS_PLACE", "MANGROVE_ROOTS_HIT", "MANGROVE_ROOTS_FALL"]),
    ("MUDDY_MANGROVE_ROOTS", 1.0, 1.0, ["MUDDY_MANGROVE_ROOTS_BREAK", "MUDDY_MANGROVE_ROOTS_STEP", "MUDDY_MANGROVE_ROOTS_PLACE", "MUDDY_MANGROVE_ROOTS_HIT", "MUDDY_MANGROVE_ROOTS_FALL"]),
    ("MUD", 1.0, 1.0, ["MUD_BREAK", "MUD_STEP", "MUD_PLACE", "MUD_HIT", "MUD_FALL"]),
    ("MUD_BRICKS", 1.0, 1.0, ["MUD_BRICKS_BREAK", "MUD_BRICKS_STEP", "MUD_BRICKS_PLACE", "MUD_BRICKS_HIT", "MUD_BRICKS_FALL"]),
    ("PACKED_MUD", 1.0, 1.0, ["PACKED_MUD_BREAK", "PACKED_MUD_STEP", "PACKED_MUD_PLACE", "PACKED_MUD_HIT", "PACKED_MUD_FALL"]),
    ("HANGING_SIGN", 1.0, 1.0, ["HANGING_SIGN_BREAK", "HANGING_SIGN_STEP", "HANGING_SIGN_PLACE", "HANGING_SIGN_HIT", "HANGING_SIGN_FALL"]),
    ("NETHER_WOOD_HANGING_SIGN", 1.0, 1.0, ["NETHER_WOOD_HANGING_SIGN_BREAK", "NETHER_WOOD_HANGING_SIGN_STEP", "NETHER_WOOD_HANGING_SIGN_PLACE", "NETHER_WOOD_HANGING_SIGN_HIT", "NETHER_WOOD_HANGING_SIGN_FALL"]),
    ("BAMBOO_WOOD_HANGING_SIGN", 1.0, 1.0, ["BAMBOO_WOOD_HANGING_SIGN_BREAK", "BAMBOO_WOOD_HANGING_SIGN_STEP", "BAMBOO_WOOD_HANGING_SIGN_PLACE", "BAMBOO_WOOD_HANGING_SIGN_HIT", "BAMBOO_WOOD_HANGING_SIGN_FALL"]),
    ("BAMBOO_WOOD", 1.0, 1.0, ["BAMBOO_WOOD_BREAK", "BAMBOO_WOOD_STEP", "BAMBOO_WOOD_PLACE", "BAMBOO_WOOD_HIT", "BAMBOO_WOOD_FALL"]),
    ("NETHER_WOOD", 1.0, 1.0, ["NETHER_WOOD_BREAK", "NETHER_WOOD_STEP", "NETHER_WOOD_PLACE", "NETHER_WOOD_HIT", "NETHER_WOOD_FALL"]),
    ("CHERRY_WOOD", 1.0, 1.0, ["CHERRY_WOOD_BREAK", "CHERRY_WOOD_STEP", "CHERRY_WOOD_PLACE", "CHERRY_WOOD_HIT", "CHERRY_WOOD_FALL"]),
    ("CHERRY_SAPLING", 1.0, 1.0, ["CHERRY_SAPLING_BREAK", "CHERRY_SAPLING_STEP", "CHERRY_SAPLING_PLACE", "CHERRY_SAPLING_HIT", "CHERRY_SAPLING_FALL"]),
    ("CHERRY_LEAVES", 1.0, 1.0, ["CHERRY_LEAVES_BREAK", "CHERRY_LEAVES_STEP", "CHERRY_LEAVES_PLACE", "CHERRY_LEAVES_HIT", "CHERRY_LEAVES_FALL"]),
    ("CHERRY_WOOD_HANGING_SIGN", 1.0, 1.0, ["CHERRY_WOOD_HANGING_SIGN_BREAK", "CHERRY_WOOD_HANGING_SIGN_STEP", "CHERRY_WOOD_HANGING_SIGN_PLACE", "CHERRY_WOOD_HANGING_SIGN_HIT", "CHERRY_WOOD_HANGING_SIGN_FALL"]),
    ("CHISELED_BOOKSHELF", 1.0, 1.0, ["CHISELED_BOOKSHELF_BREAK", "CHISELED_BOOKSHELF_STEP", "CHISELED_BOOKSHELF_PLACE", "CHISELED_BOOKSHELF_HIT", "CHISELED_BOOKSHELF_FALL"]),
    ("SUSPICIOUS_SAND", 1.0, 1.0, ["SUSPICIOUS_SAND_BREAK", "SUSPICIOUS_SAND_STEP", "SUSPICIOUS_SAND_PLACE", "SUSPICIOUS_SAND_HIT", "SUSPICIOUS_SAND_FALL"]),
    ("SUSPICIOUS_GRAVEL", 1.0, 1.0, ["SUSPICIOUS_GRAVEL_BREAK", "SUSPICIOUS_GRAVEL_STEP", "SUSPICIOUS_GRAVEL_PLACE", "SUSPICIOUS_GRAVEL_HIT", "SUSPICIOUS_GRAVEL_FALL"]),
    ("DECORATED_POT", 1.0, 1.0, ["DECORATED_POT_BREAK", "DECORATED_POT_STEP", "DECORATED_POT_PLACE", "DECORATED_POT_HIT", "DECORATED_POT_FALL"]),
    ("DECORATED_POT_CRACKED", 1.0, 1.0, ["DECORATED_POT_SHATTER", "DECORATED_POT_STEP", "DECORATED_POT_PLACE", "DECORATED_POT_HIT", "DECORATED_POT_FALL"]),
    ("TRIAL_SPAWNER", 1.0, 1.0, ["TRIAL_SPAWNER_BREAK", "TRIAL_SPAWNER_STEP", "TRIAL_SPAWNER_PLACE", "TRIAL_SPAWNER_HIT", "TRIAL_SPAWNER_FALL"]),
    ("SPONGE", 1.0, 1.0, ["SPONGE_BREAK", "SPONGE_STEP", "SPONGE_PLACE", "SPONGE_HIT", "SPONGE_FALL"]),
    ("WET_SPONGE", 1.0, 1.0, ["WET_SPONGE_BREAK", "WET_SPONGE_STEP", "WET_SPONGE_PLACE", "WET_SPONGE_HIT", "WET_SPONGE_FALL"]),
    ("VAULT", 1.0, 1.0, ["VAULT_BREAK", "VAULT_STEP", "VAULT_PLACE", "VAULT_HIT", "VAULT_FALL"]),
    ("HEAVY_CORE", 1.0, 1.0, ["HEAVY_CORE_BREAK", "HEAVY_CORE_STEP", "HEAVY_CORE_PLACE", "HEAVY_CORE_HIT", "HEAVY_CORE_FALL"]),
    ("COBWEB", 1.0, 1.0, ["COBWEB_BREAK", "COBWEB_STEP", "COBWEB_PLACE", "COBWEB_HIT", "COBWEB_FALL"]),
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
/// and behaviour (a door opens, a fence joins its neighbours, a button springs back). The rest
/// copy a property-less block. Each class's states are [`vanilla_definition`]'s.
const TEMPLATES: [(&str, &str); 22] = [
    ("DoorBlock", "minecraft:oak_door"),
    ("TrapDoorBlock", "minecraft:oak_trapdoor"),
    ("SlabBlock", "minecraft:smooth_stone_slab"),
    ("StairBlock", "minecraft:stone_stairs"),
    ("LadderBlock", "minecraft:ladder"),
    ("RotatedPillarBlock", "minecraft:quartz_pillar"),
    ("IronBarsBlock", "minecraft:iron_bars"),
    ("StainedGlassPaneBlock", "minecraft:white_stained_glass_pane"),
    ("FenceBlock", "minecraft:oak_fence"),
    ("FenceGateBlock", "minecraft:oak_fence_gate"),
    ("WallBlock", "minecraft:cobblestone_wall"),
    ("ButtonBlock", "minecraft:oak_button"),
    ("LeverBlock", "minecraft:lever"),
    ("PressurePlateBlock", "minecraft:oak_pressure_plate"),
    ("WeightedPressurePlateBlock", "minecraft:light_weighted_pressure_plate"),
    ("ChainBlock", "minecraft:iron_chain"),
    ("LanternBlock", "minecraft:lantern"),
    ("TorchBlock", "minecraft:torch"),
    ("WallTorchBlock", "minecraft:wall_torch"),
    ("CakeBlock", "minecraft:cake"),
    ("FarmBlock", "minecraft:farmland"),
    ("FlowerBlock", "minecraft:poppy"),
];

/// The template of mod block `b`, whose first vanilla class is `simple`: [`TEMPLATES`]' block, or,
/// where vanilla's behaviour follows what the class's constructor was given (its `BlockSetType`,
/// vanilla's field `type`; a weighted plate's `maxWeight`), the vanilla block given the same:
/// iron's door and trapdoor don't open by hand, stone's button ignores arrows and its plate senses
/// mobs alone, a heavy plate counts to 150.
fn template_of(rt: &mut Rt, b: V, simple: &str) -> Option<&'static str> {
    let template = TEMPLATES.iter().find(|(c, _)| *c == simple)?.1;
    let set = rt.get_field(b, "type");
    let set = (set.r() != 0 && rt.class_of(set) == BLOCK_SET_TYPE).then_some(set);
    let lacks = |rt: &Rt, field: &'static str| set.is_some_and(|s| rt.get_field(s, field).i() == 0);
    let mobs = api_common::get_static(rt, "net/minecraft/world/level/block/state/properties/BlockSetType$PressurePlateSensitivity", "MOBS");
    let senses_mobs = set.is_some_and(|s| mobs.is_some_and(|m| rt.get_field(s, "pressurePlateSensitivity") == m));
    Some(match simple {
        "DoorBlock" if lacks(rt, "canOpenByHand") => "minecraft:iron_door",
        "TrapDoorBlock" if lacks(rt, "canOpenByHand") => "minecraft:iron_trapdoor",
        "ButtonBlock" if lacks(rt, "canButtonBeActivatedByArrows") => "minecraft:stone_button",
        "PressurePlateBlock" if senses_mobs => "minecraft:stone_pressure_plate",
        "WeightedPressurePlateBlock" if rt.get_field(b, "maxWeight").i() > 15 => "minecraft:heavy_weighted_pressure_plate",
        _ => template,
    })
}

/// The nearest of 1.21.1's block class `class` and the vanilla classes above it that has a
/// template ([`TEMPLATES`]), by its short name: weathering copper stairs are stairs. None past
/// `Block`, or for a class the generated table of 1.21.1's classes doesn't have.
fn templated_class(class: &str) -> Option<&'static str> {
    use crate::vanilla_classes::CLASSES;
    let mut c = class;
    for _ in 0..32 {
        let short = c.strip_prefix("net/minecraft/world/level/block/")?;
        if let Some(&(t, _)) = TEMPLATES.iter().find(|(t, _)| *t == short) {
            return Some(t);
        }
        if matches!(short, "Block" | "state/BlockBehaviour") {
            return None;
        }
        c = CLASSES[CLASSES.binary_search_by(|e| e.0.cmp(c)).ok()?].1;
    }
    None
}

/// 1.21.1's block classes that declare an `updateShape` of their own (read from their bytecode), by
/// name under `net/minecraft/world/level/block/`.
const UPDATE_SHAPE_CLASSES: [&str; 104] = [
    "AmethystClusterBlock",
    "AttachedStemBlock",
    "BambooSaplingBlock",
    "BambooStalkBlock",
    "BannerBlock",
    "BarrierBlock",
    "BaseCoralPlantTypeBlock",
    "BaseCoralWallFanBlock",
    "BasePressurePlateBlock",
    "BaseRailBlock",
    "BaseTorchBlock",
    "BedBlock",
    "BeehiveBlock",
    "BellBlock",
    "BigDripleafBlock",
    "BigDripleafStemBlock",
    "BrushableBlock",
    "BubbleColumnBlock",
    "BushBlock",
    "CactusBlock",
    "CakeBlock",
    "CampfireBlock",
    "CandleBlock",
    "CandleCakeBlock",
    "CarpetBlock",
    "CeilingHangingSignBlock",
    "ChainBlock",
    "ChestBlock",
    "ChorusFlowerBlock",
    "ChorusPlantBlock",
    "CocoaBlock",
    "ComparatorBlock",
    "ConcretePowderBlock",
    "ConduitBlock",
    "CoralBlock",
    "CoralFanBlock",
    "CoralPlantBlock",
    "CoralWallFanBlock",
    "DecoratedPotBlock",
    "DirtPathBlock",
    "DoorBlock",
    "DoublePlantBlock",
    "EnderChestBlock",
    "FaceAttachedHorizontalDirectionalBlock",
    "FallingBlock",
    "FarmBlock",
    "FenceBlock",
    "FenceGateBlock",
    "FireBlock",
    "FlowerPotBlock",
    "FrogspawnBlock",
    "GlowLichenBlock",
    "GrowingPlantBodyBlock",
    "GrowingPlantHeadBlock",
    "HangingRootsBlock",
    "HeavyCoreBlock",
    "HugeMushroomBlock",
    "IronBarsBlock",
    "LadderBlock",
    "LanternBlock",
    "LeavesBlock",
    "LightBlock",
    "LightningRodBlock",
    "LiquidBlock",
    "MagmaBlock",
    "MangrovePropaguleBlock",
    "MangroveRootsBlock",
    "MultifaceBlock",
    "NetherPortalBlock",
    "NoteBlock",
    "ObserverBlock",
    "piston/PistonHeadBlock",
    "PitcherCropBlock",
    "PointedDripstoneBlock",
    "RedstoneWallTorchBlock",
    "RedStoneWireBlock",
    "RepeaterBlock",
    "ScaffoldingBlock",
    "SculkSensorBlock",
    "SculkShriekerBlock",
    "SculkVeinBlock",
    "SeagrassBlock",
    "SeaPickleBlock",
    "SignBlock",
    "SlabBlock",
    "SmallDripleafBlock",
    "SnowLayerBlock",
    "SnowyDirtBlock",
    "SoulFireBlock",
    "SoulSandBlock",
    "SporeBlossomBlock",
    "StairBlock",
    "StandingSignBlock",
    "SugarCaneBlock",
    "TrapDoorBlock",
    "TripWireBlock",
    "TripWireHookBlock",
    "VineBlock",
    "WallBannerBlock",
    "WallBlock",
    "WallHangingSignBlock",
    "WallSignBlock",
    "WallTorchBlock",
    "WaterloggedTransparentBlock",
];

/// 1.21.1's block classes that declare a `canSurvive` of their own (read from their bytecode), by
/// name under `net/minecraft/world/level/block/`.
const CAN_SURVIVE_CLASSES: [&str; 59] = [
    "AmethystClusterBlock",
    "BambooSaplingBlock",
    "BambooStalkBlock",
    "BannerBlock",
    "BaseCoralPlantTypeBlock",
    "BaseCoralWallFanBlock",
    "BasePressurePlateBlock",
    "BaseRailBlock",
    "BaseTorchBlock",
    "BellBlock",
    "BigDripleafBlock",
    "BigDripleafStemBlock",
    "BubbleColumnBlock",
    "BushBlock",
    "CactusBlock",
    "CakeBlock",
    "CandleBlock",
    "CandleCakeBlock",
    "CarpetBlock",
    "CeilingHangingSignBlock",
    "ChorusFlowerBlock",
    "ChorusPlantBlock",
    "CocoaBlock",
    "CropBlock",
    "DiodeBlock",
    "DirtPathBlock",
    "DoorBlock",
    "DoublePlantBlock",
    "FaceAttachedHorizontalDirectionalBlock",
    "FarmBlock",
    "FireBlock",
    "FrogspawnBlock",
    "GrindstoneBlock",
    "GrowingPlantBlock",
    "HangingRootsBlock",
    "LadderBlock",
    "LanternBlock",
    "MangrovePropaguleBlock",
    "MultifaceBlock",
    "MushroomBlock",
    "PitcherCropBlock",
    "PointedDripstoneBlock",
    "RedStoneWireBlock",
    "RedstoneWallTorchBlock",
    "ScaffoldingBlock",
    "SeaPickleBlock",
    "SmallDripleafBlock",
    "SnowLayerBlock",
    "SoulFireBlock",
    "SporeBlossomBlock",
    "StandingSignBlock",
    "SugarCaneBlock",
    "TallSeagrassBlock",
    "TripWireHookBlock",
    "VineBlock",
    "WallBannerBlock",
    "WallSignBlock",
    "WallTorchBlock",
    "piston/PistonHeadBlock",
];

/// The class whose `updateShape` a call naming vanilla block class `class` runs (a mod block's
/// `super.updateShape`): the nearest of it and the classes above it that declares one, by name
/// under `net/minecraft/world/level/block/`, `state/BlockBehaviour` (which keeps the state) past
/// them all. None for a class the table of 1.21.1's classes doesn't have.
pub(crate) fn update_shape_declarer(class: &str) -> Option<&'static str> {
    declarer_in(class, &UPDATE_SHAPE_CLASSES)
}

/// The class whose `canSurvive` a call naming vanilla block class `class` runs (a mod block's
/// `super.canSurvive`), as [`update_shape_declarer`] finds `updateShape`'s: `state/BlockBehaviour`
/// (true) past them all.
pub(crate) fn can_survive_declarer(class: &str) -> Option<&'static str> {
    declarer_in(class, &CAN_SURVIVE_CLASSES)
}

/// The nearest of 1.21.1's block class `class` and the classes above it that `declaring` names.
fn declarer_in(class: &str, declaring: &[&'static str]) -> Option<&'static str> {
    use crate::vanilla_classes::CLASSES;
    let mut c = class;
    for _ in 0..32 {
        let short = c.strip_prefix("net/minecraft/world/level/block/")?;
        if let Some(d) = declaring.iter().find(|d| **d == short) {
            return Some(d);
        }
        if short == "state/BlockBehaviour" {
            return Some("state/BlockBehaviour");
        }
        c = CLASSES[CLASSES.binary_search_by(|e| e.0.cmp(c)).ok()?].1;
    }
    None
}

/// The simple names of 1.21.1's block class `class` and the classes above it, nearest first, up to
/// the one whose `updateShape` it runs ([`update_shape_declarer`]): where the game finds that rule
/// among a mod block's template's classes, a renamed or untemplated class passed over (a flower's
/// is its template's, the game's own `FlowerBlock`). Empty past `BlockBehaviour`'s.
pub(crate) fn update_shape_ancestry(class: &str) -> Vec<&'static str> {
    ancestry_in(class, &UPDATE_SHAPE_CLASSES)
}

/// The same for `canSurvive` ([`can_survive_declarer`]).
pub(crate) fn can_survive_ancestry(class: &str) -> Vec<&'static str> {
    ancestry_in(class, &CAN_SURVIVE_CLASSES)
}

fn ancestry_in(class: &str, declaring: &[&str]) -> Vec<&'static str> {
    use crate::vanilla_classes::CLASSES;
    let mut names = Vec::new();
    let Some(mut at) = CLASSES.binary_search_by(|e| e.0.cmp(class)).ok() else {
        return names;
    };
    for _ in 0..32 {
        let (c, parent, _) = CLASSES[at];
        let Some(short) = c.strip_prefix("net/minecraft/world/level/block/") else {
            break;
        };
        if short == "state/BlockBehaviour" {
            names.clear();
            break;
        }
        names.push(short.rsplit('/').next().unwrap_or(short));
        if declaring.contains(&short) {
            break;
        }
        let Ok(next) = CLASSES.binary_search_by(|e| e.0.cmp(parent)) else {
            break;
        };
        at = next;
    }
    names
}

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
        "FarmBlock" => &["MOISTURE"],
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
const RAIL_SHAPE: &str = "net/minecraft/world/level/block/state/properties/RailShape";
const RAIL_SHAPES: [&str; 10] = [
    "north_south", "east_west", "ascending_east", "ascending_west", "ascending_north", "ascending_south", "south_east", "south_west",
    "north_west", "north_east",
];

/// The value vanilla class `class`'s constructor gives a property its definition adds (`DoorBlock`
/// starts closed, unpowered, lower half; a wall with its post up), where `StateDefinition.any()`
/// would take another.
fn vanilla_default(class: &str, field: &str) -> Option<&'static str> {
    if class.rsplit('/').next() == Some("WallBlock") && field == "UP" {
        return Some("true");
    }
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
/// and pull it, placing doesn't replace it, a full cube's faces are sturdy. (A mod block's own
/// faces come from its shapes: `api::face_sturdy`.)
pub(crate) fn state_facts(rt: &Rt, text: &str) -> BlockFacts {
    let (name, _) = crate::host::split_state(text);
    let java = BlockFacts { hardness: 0.0, push: 0, replaceable: false, sturdy: (1 << 18) - 1 };
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
    let name: &'static str = match ENUM_NAMES.iter().find(|(c, t, _)| class.ends_with(c) && *t == text) {
        Some(&(_, _, n)) => n,
        None => rt.static_key(&values[i].to_ascii_uppercase()),
    };
    Some(api::api_enum(rt, class, name, i as i32))
}

/// Property enum constants whose Java name isn't their serialized one upper-cased:
/// (class, serialized name, constant).
const ENUM_NAMES: [(&str, &str, &str); 1] = [("/PistonType", "normal", "DEFAULT")];

/// The serialized name of vanilla enum `class`'s constant `field` (`PistonType.DEFAULT` is `normal`).
pub(crate) fn enum_text(class: &str, field: &str) -> String {
    match ENUM_NAMES.iter().find(|(c, _, n)| class.ends_with(c) && *n == field) {
        Some(&(_, t, _)) => t.to_owned(),
        None => field.to_ascii_lowercase(),
    }
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
            map_color_constant(rt, c)
        }
        // NeoForge's `getTag()`: the colour's item tag, `c:dyes/<colour>`.
        "getTag" => {
            let tag = format!("c:dyes/{}", DYES[id(rt)].0.to_ascii_lowercase());
            crate::fluids::tag_key(rt, "item", &tag)
        }
        // `byId`: out of range gives white, as vanilla's `ByIdMap.OutOfBoundsStrategy.ZERO`.
        "byId" => {
            let i = a(0).i();
            dye(rt, if (0..16).contains(&i) { i as usize } else { 0 })
        }
        // NeoForge's `getColor(stack)`: a dye's own colour, else the first colour whose
        // `c:dyes/<colour>` tag holds the stack's item (its loop stops before black), else null.
        // Create's toolbox, among others, dyes itself only with a stack that has one.
        "getColor" => {
            let stack = a(0);
            if api_common::stack_empty(rt, stack) {
                return Some(V::NULL);
            }
            let item = api_common::item_like_name(rt, rt.get_field(stack, "$item"));
            let colour = |d: &str| d.to_ascii_lowercase();
            if let Some(i) = DYES.iter().position(|(d, _)| item == format!("minecraft:{}_dye", colour(d))) {
                return Some(dye(rt, i));
            }
            for (i, (d, _)) in DYES.iter().enumerate().take(15) {
                if crate::recipes::item_in_tag(rt, &item, &format!("c:dyes/{}", colour(d))) {
                    return Some(dye(rt, i));
                }
            }
            V::NULL
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
/// Whether `class` is one of the vanilla enums a block state's value may be.
pub(crate) fn is_vanilla_enum(class: &str) -> bool {
    vanilla_enum(class).is_some()
}

/// A vanilla enum constant's serialized name (`PistonType.DEFAULT` is `normal`).
pub(crate) fn enum_serialized(rt: &Rt, class: &str, v: V) -> Option<String> {
    let i = rt.get_field(v, "$ordinal").i();
    vanilla_enum(class)?.get(usize::try_from(i).ok()?).map(|s| (*s).to_owned())
}

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

/// The block classes of 1.21.1 whose `getShape(state, level, pos, context)` is `BlockBehaviour`'s
/// (a full block): neither they nor a class above them overrides it, as Minecraft 1.21.1's class
/// files say. A mod block's `super.getShape(...)` to one of them answers that.
const DEFAULT_SHAPE_BLOCKS: [&str; 99] = [
    "AbstractBannerBlock", "AbstractCandleBlock", "AbstractChestBlock", "AbstractFurnaceBlock", "AbstractSkullBlock", "AmethystBlock", "BarrelBlock",
    "BarrierBlock", "BaseEntityBlock", "BeaconBlock", "BeehiveBlock", "BlastFurnaceBlock", "Block", "BrushableBlock", "BuddingAmethystBlock",
    "BushBlock", "CartographyTableBlock", "CarvedPumpkinBlock", "CherryLeavesBlock", "ChiseledBookShelfBlock", "ChorusFlowerBlock",
    "ColoredFallingBlock", "CommandBlock", "ConcretePowderBlock", "CopperBulbBlock", "CoralBlock", "CrafterBlock", "CraftingTableBlock",
    "CryingObsidianBlock", "DirectionalBlock", "DispenserBlock", "DoublePlantBlock", "DropExperienceBlock", "DropperBlock", "EndGatewayBlock",
    "EquipableCarvedPumpkinBlock", "FaceAttachedHorizontalDirectionalBlock", "FallingBlock", "FletchingTableBlock", "FrostedIceBlock", "FurnaceBlock",
    "GlazedTerracottaBlock", "GrassBlock", "HalfTransparentBlock", "HayBlock", "HoneyBlock", "HorizontalDirectionalBlock", "HugeMushroomBlock",
    "IceBlock", "InfestedBlock", "InfestedRotatedPillarBlock", "JigsawBlock", "JukeboxBlock", "LeavesBlock", "LoomBlock", "MagmaBlock",
    "MangroveLeavesBlock", "MangroveRootsBlock", "MossBlock", "MudBlock", "MyceliumBlock", "NetherrackBlock", "NoteBlock", "NyliumBlock",
    "ObserverBlock", "PowderSnowBlock", "PoweredBlock", "PumpkinBlock", "RedStoneOreBlock", "RedstoneLampBlock", "RespawnAnchorBlock",
    "RootedDirtBlock", "RotatedPillarBlock", "SculkBlock", "SculkCatalystBlock", "SculkShriekerBlock", "SlimeBlock", "SmithingTableBlock",
    "SmokerBlock", "SnowyDirtBlock", "SoulSandBlock", "SpawnerBlock", "SpongeBlock", "SpreadingSnowyDirtBlock", "StainedGlassBlock", "StructureBlock",
    "TallFlowerBlock", "TargetBlock", "TintedGlassBlock", "TntBlock", "TransparentBlock", "TrialSpawnerBlock", "VaultBlock",
    "WaterloggedTransparentBlock", "WeatheringCopperBulbBlock", "WeatheringCopperFullBlock", "WeatheringCopperGrateBlock", "WebBlock",
    "WetSpongeBlock",
];

/// The block classes of 1.21.1 declaring their own `getCollisionShape(state, level, pos, context)`,
/// as Minecraft 1.21.1's class files say (`piston/` names that package's).
pub(crate) const COLLISION_SHAPED: [&str; 21] = [
    "BambooStalkBlock", "BellBlock", "BigDripleafBlock", "CactusBlock", "ComposterBlock", "CrossCollisionBlock", "FenceGateBlock",
    "GrindstoneBlock", "HoneyBlock", "LecternBlock", "LiquidBlock", "MudBlock", "PitcherCropBlock", "PowderSnowBlock", "ScaffoldingBlock",
    "SculkShriekerBlock", "SnowLayerBlock", "SoulSandBlock", "WallBlock", "WallHangingSignBlock", "piston/MovingPistonBlock",
];

/// ... and their own `getBlockSupportShape(state, level, pos)`.
pub(crate) const SUPPORT_SHAPED: [&str; 9] = [
    "CeilingHangingSignBlock", "ChorusFlowerBlock", "FenceGateBlock", "LeavesBlock", "MudBlock", "ShulkerBoxBlock", "SnowLayerBlock",
    "SoulSandBlock", "WallHangingSignBlock",
];

/// The class whose method 1.21.1's block class `class` runs, given the classes `declaring` their
/// own (as [`COLLISION_SHAPED`] names them): the first of `class` and the classes above it that is
/// one of them, else `"BlockBehaviour"`. None for a class the generated table of 1.21.1's classes
/// doesn't have.
pub(crate) fn declaring_class(class: &str, declaring: &[&'static str]) -> Option<&'static str> {
    use crate::vanilla_classes::CLASSES;
    let mut c = class;
    for _ in 0..32 {
        let short = c.strip_prefix("net/minecraft/world/level/block/")?;
        if let Some(d) = declaring.iter().find(|d| **d == short) {
            return Some(d);
        }
        if matches!(short, "Block" | "state/BlockBehaviour") {
            return Some("BlockBehaviour");
        }
        c = CLASSES[CLASSES.binary_search_by(|e| e.0.cmp(c)).ok()?].1;
    }
    None
}

/// Vanilla 1.21.1's own `getBlockSupportShape` of [`SUPPORT_SHAPED`] class `class` for a state
/// with properties `props`, in pixels as `Block.box` takes them, read from 1.21.1's classes. None
/// for a hanging sign's, which is its shape. A shulker box's is a closed one's: an open box's
/// (while it opens) needs its block entity.
pub(crate) fn vanilla_support_shape(class: &str, props: &[(&str, &str)]) -> Option<Vec<[f64; 6]>> {
    let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v);
    Some(match class {
        "LeavesBlock" => Vec::new(),
        "MudBlock" | "SoulSandBlock" | "ShulkerBoxBlock" => vec![[0.0, 0.0, 0.0, 16.0, 16.0, 16.0]],
        // `SHAPE_BY_LAYER[layers]`: two pixels a layer.
        "SnowLayerBlock" => {
            let layers = get("layers").parse::<u8>().unwrap_or(1).clamp(1, 8);
            vec![[0.0, 0.0, 0.0, 16.0, 2.0 * f64::from(layers), 16.0]]
        }
        "ChorusFlowerBlock" => vec![[1.0, 0.0, 1.0, 15.0, 15.0, 15.0]],
        // Open: none; closed: a bar along the gate, from 5 pixels up to a fence's height.
        "FenceGateBlock" if get("open") == "true" => Vec::new(),
        "FenceGateBlock" if matches!(get("facing"), "north" | "south") => vec![[0.0, 5.0, 6.0, 16.0, 24.0, 10.0]],
        "FenceGateBlock" => vec![[6.0, 5.0, 0.0, 10.0, 24.0, 16.0]],
        _ => return None,
    })
}

/// The block classes of 1.21.1 whose scheduled `tick(state, level, pos, random)` is `BlockBehaviour`'s,
/// which does nothing: neither they nor a class above them declares one, as Minecraft 1.21.1's class
/// files say. A mod block's `super.tick(...)` to one of them (Farmer's Delight's rice calls
/// `BushBlock.tick` from its random tick) does nothing either.
const DEFAULT_TICK_BLOCKS: [&str; 189] = [
    "AbstractBannerBlock", "AbstractCandleBlock", "AbstractChestBlock", "AbstractFurnaceBlock", "AbstractSkullBlock", "AirBlock", "AmethystBlock",
    "AmethystClusterBlock", "AttachedStemBlock", "AzaleaBlock", "BambooSaplingBlock", "BannerBlock", "BarrierBlock", "BaseCoralFanBlock",
    "BaseCoralPlantBlock", "BaseCoralPlantTypeBlock", "BaseCoralWallFanBlock", "BaseEntityBlock", "BaseFireBlock", "BaseRailBlock",
    "BaseTorchBlock", "BeaconBlock", "BedBlock", "BeehiveBlock", "BeetrootBlock", "BellBlock", "BlastFurnaceBlock", "Block", "BrewingStandBlock",
    "BuddingAmethystBlock", "BushBlock", "CakeBlock", "CampfireBlock", "CandleBlock", "CandleCakeBlock", "CarpetBlock", "CarrotBlock",
    "CartographyTableBlock", "CarvedPumpkinBlock", "CeilingHangingSignBlock", "ChainBlock", "ChiseledBookShelfBlock", "CocoaBlock",
    "ConduitBlock", "CopperBulbBlock", "CraftingTableBlock", "CropBlock", "CrossCollisionBlock", "CryingObsidianBlock", "DaylightDetectorBlock",
    "DeadBushBlock", "DecoratedPotBlock", "DirectionalBlock", "DoorBlock", "DoublePlantBlock", "DropExperienceBlock", "EnchantingTableBlock",
    "EndGatewayBlock", "EndPortalBlock", "EndPortalFrameBlock", "EndRodBlock", "EquipableCarvedPumpkinBlock",
    "FaceAttachedHorizontalDirectionalBlock", "FenceBlock", "FenceGateBlock", "FletchingTableBlock", "FlowerBlock", "FlowerPotBlock",
    "FungusBlock", "FurnaceBlock", "GlazedTerracottaBlock", "GlowLichenBlock", "GrassBlock", "GrindstoneBlock", "HalfTransparentBlock",
    "HangingRootsBlock", "HayBlock", "HeavyCoreBlock", "HoneyBlock", "HopperBlock", "HorizontalDirectionalBlock", "HugeMushroomBlock", "IceBlock",
    "InfestedBlock", "InfestedRotatedPillarBlock", "IronBarsBlock", "JigsawBlock", "JukeboxBlock", "LadderBlock", "LanternBlock", "LeverBlock",
    "LightBlock", "LiquidBlock", "LoomBlock", "MangrovePropaguleBlock", "MangroveRootsBlock", "MossBlock", "MudBlock", "MultifaceBlock",
    "MushroomBlock", "MyceliumBlock", "NetherPortalBlock", "NetherSproutsBlock", "NetherWartBlock", "NetherrackBlock", "NoteBlock", "NyliumBlock",
    "PiglinWallSkullBlock", "PinkPetalsBlock", "PipeBlock", "PitcherCropBlock", "PlayerHeadBlock", "PlayerWallHeadBlock", "PotatoBlock",
    "PowderSnowBlock", "PoweredBlock", "PoweredRailBlock", "PumpkinBlock", "RailBlock", "RedStoneOreBlock", "RedStoneWireBlock",
    "RespawnAnchorBlock", "RodBlock", "RootedDirtBlock", "RootsBlock", "RotatedPillarBlock", "SaplingBlock", "SculkBlock", "SculkVeinBlock",
    "SeaPickleBlock", "SeagrassBlock", "ShulkerBoxBlock", "SignBlock", "SkullBlock", "SlabBlock", "SlimeBlock", "SmallDripleafBlock",
    "SmithingTableBlock", "SmokerBlock", "SnowLayerBlock", "SnowyDirtBlock", "SoulFireBlock", "SpawnerBlock", "SpongeBlock", "SporeBlossomBlock",
    "SpreadingSnowyDirtBlock", "StainedGlassBlock", "StainedGlassPaneBlock", "StairBlock", "StandingSignBlock", "StemBlock", "StonecutterBlock",
    "StructureBlock", "StructureVoidBlock", "SweetBerryBushBlock", "TallFlowerBlock", "TallGrassBlock", "TallSeagrassBlock", "TintedGlassBlock",
    "TntBlock", "TorchBlock", "TorchflowerCropBlock", "TransparentBlock", "TrapDoorBlock", "TrialSpawnerBlock", "TurtleEggBlock", "VaultBlock",
    "VineBlock", "WallBannerBlock", "WallBlock", "WallHangingSignBlock", "WallSignBlock", "WallSkullBlock", "WallTorchBlock", "WaterlilyBlock",
    "WaterloggedTransparentBlock", "WeatheringCopperBulbBlock", "WeatheringCopperDoorBlock", "WeatheringCopperFullBlock",
    "WeatheringCopperGrateBlock", "WeatheringCopperSlabBlock", "WeatheringCopperStairBlock", "WeatheringCopperTrapDoorBlock", "WebBlock",
    "WetSpongeBlock", "WitherRoseBlock", "WitherSkullBlock", "WitherWallSkullBlock", "WoolCarpetBlock",
];

/// Vanilla 1.21.1's `getShape` of block class `class` (its simple name: the class a mod's
/// `super.getShape(...)` names, which may inherit it) for a state with properties `props`, in
/// pixels as `Block.box` takes them; several boxes for a shape vanilla joins. Read from 1.21.1's
/// classes. None for a class not kept here.
fn vanilla_state_shape(class: &str, props: &[(&str, &str)]) -> Option<Vec<[f64; 6]>> {
    let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v);
    let on = |k: &str| get(k) == "true";
    let x_axis = matches!(get("facing"), "east" | "west");
    // The thin boxes along one side (doors, ladders, open trapdoors), named as vanilla names
    // them: `SOUTH_AABB` lies along the north edge, as a closed door facing south does.
    const SOUTH: [f64; 6] = [0.0, 0.0, 0.0, 16.0, 16.0, 3.0];
    const NORTH: [f64; 6] = [0.0, 0.0, 13.0, 16.0, 16.0, 16.0];
    const WEST: [f64; 6] = [13.0, 0.0, 0.0, 16.0, 16.0, 16.0];
    const EAST: [f64; 6] = [0.0, 0.0, 0.0, 3.0, 16.0, 16.0];
    let one = |b: [f64; 6]| Some(vec![b]);
    match class {
        "DoorBlock" => {
            let (closed, right) = (!on("open"), get("hinge") == "right");
            one(match get("facing") {
                "south" => if closed { SOUTH } else if right { EAST } else { WEST },
                "west" => if closed { WEST } else if right { SOUTH } else { NORTH },
                "north" => if closed { NORTH } else if right { WEST } else { EAST },
                _ => if closed { EAST } else if right { NORTH } else { SOUTH },
            })
        }
        "TrapDoorBlock" if !on("open") => one(if get("half") == "top" { [0.0, 13.0, 0.0, 16.0, 16.0, 16.0] } else { [0.0, 0.0, 0.0, 16.0, 3.0, 16.0] }),
        "TrapDoorBlock" => one(match get("facing") {
            "south" => SOUTH,
            "west" => WEST,
            "east" => EAST,
            _ => NORTH,
        }),
        "LadderBlock" => one(match get("facing") {
            "north" => NORTH,
            "south" => SOUTH,
            "west" => WEST,
            _ => EAST,
        }),
        "LeverBlock" => one(match (get("face"), get("facing")) {
            ("floor", _) if x_axis => [4.0, 0.0, 5.0, 12.0, 6.0, 11.0],
            ("floor", _) => [5.0, 0.0, 4.0, 11.0, 6.0, 12.0],
            ("wall", "east") => [0.0, 4.0, 5.0, 6.0, 12.0, 11.0],
            ("wall", "west") => [10.0, 4.0, 5.0, 16.0, 12.0, 11.0],
            ("wall", "south") => [5.0, 4.0, 0.0, 11.0, 12.0, 6.0],
            ("wall", _) => [5.0, 4.0, 10.0, 11.0, 12.0, 16.0],
            _ if x_axis => [4.0, 10.0, 5.0, 12.0, 16.0, 11.0],
            _ => [5.0, 10.0, 4.0, 11.0, 16.0, 12.0],
        }),
        "ButtonBlock" => {
            // Pressed, a button sinks to half its depth.
            let d = if on("powered") { 1.0 } else { 2.0 };
            one(match (get("face"), get("facing")) {
                ("floor", _) if x_axis => [6.0, 0.0, 5.0, 10.0, d, 11.0],
                ("floor", _) => [5.0, 0.0, 6.0, 11.0, d, 10.0],
                ("wall", "east") => [0.0, 6.0, 5.0, d, 10.0, 11.0],
                ("wall", "west") => [16.0 - d, 6.0, 5.0, 16.0, 10.0, 11.0],
                ("wall", "south") => [5.0, 6.0, 0.0, 11.0, 10.0, d],
                ("wall", _) => [5.0, 6.0, 16.0 - d, 11.0, 10.0, 16.0],
                _ if x_axis => [6.0, 16.0 - d, 5.0, 10.0, 16.0, 11.0],
                _ => [5.0, 16.0 - d, 6.0, 11.0, 16.0, 10.0],
            })
        }
        // A diode's plate (a repeater's, a comparator's); a rail's, half a block where it climbs.
        "DiodeBlock" => one([0.0, 0.0, 0.0, 16.0, 2.0, 16.0]),
        "BaseRailBlock" => one([
            0.0,
            0.0,
            0.0,
            16.0,
            if get("shape").starts_with("ascending_") {
                8.0
            } else {
                2.0
            },
            16.0,
        ]),
        // A lectern's base and post, and its sloped top by where it faces (above the block).
        "LecternBlock" => {
            let mut boxes = match get("facing") {
                "north" => vec![
                    [0.0, 10.0, 1.0, 16.0, 14.0, 5.333333],
                    [0.0, 12.0, 5.333333, 16.0, 16.0, 9.666667],
                    [0.0, 14.0, 9.666667, 16.0, 18.0, 14.0],
                ],
                "south" => vec![
                    [0.0, 10.0, 10.666667, 16.0, 14.0, 15.0],
                    [0.0, 12.0, 6.333333, 16.0, 16.0, 10.666667],
                    [0.0, 14.0, 2.0, 16.0, 18.0, 6.333333],
                ],
                "east" => vec![
                    [10.666667, 10.0, 0.0, 15.0, 14.0, 16.0],
                    [6.333333, 12.0, 0.0, 10.666667, 16.0, 16.0],
                    [2.0, 14.0, 0.0, 6.333333, 18.0, 16.0],
                ],
                "west" => vec![
                    [1.0, 10.0, 0.0, 5.333333, 14.0, 16.0],
                    [5.333333, 12.0, 0.0, 9.666667, 16.0, 16.0],
                    [9.666667, 14.0, 0.0, 14.0, 18.0, 16.0],
                ],
                _ => Vec::new(),
            };
            boxes.extend([
                [0.0, 0.0, 0.0, 16.0, 2.0, 16.0],
                [4.0, 2.0, 4.0, 12.0, 14.0, 12.0],
            ]);
            Some(boxes)
        }
        // `getSignalForState(state) > 0`: powered, or a weight on it.
        "PressurePlateBlock" | "WeightedPressurePlateBlock" => {
            let pressed = on("powered") || get("power").parse::<i32>().is_ok_and(|p| p > 0);
            one([1.0, 0.0, 1.0, 15.0, if pressed { 0.5 } else { 1.0 }, 15.0])
        }
        "SlabBlock" => one(match get("type") {
            "double" => [0.0, 0.0, 0.0, 16.0, 16.0, 16.0],
            "top" => [0.0, 8.0, 0.0, 16.0, 16.0, 16.0],
            _ => [0.0, 0.0, 0.0, 16.0, 8.0, 16.0],
        }),
        "StairBlock" => {
            // `SHAPE_BY_STATE[shape.ordinal() * 4 + facing.get2DDataValue()]`: which quarters of the
            // other half the slab carries (1 north-west, 2 north-east, 4 south-west, 8 south-east).
            const SHAPE_BY_STATE: [u8; 20] = [12, 5, 3, 10, 14, 13, 7, 11, 13, 7, 11, 14, 8, 4, 1, 2, 4, 1, 2, 8];
            const SHAPES: [&str; 5] = ["straight", "inner_left", "inner_right", "outer_left", "outer_right"];
            let shape = SHAPES.iter().position(|s| *s == get("shape")).unwrap_or(0);
            let facing = ["south", "west", "north", "east"].iter().position(|f| *f == get("facing")).unwrap_or(2);
            let bits = SHAPE_BY_STATE[shape * 4 + facing];
            let top = get("half") == "top";
            let (y0, y1) = if top { (0.0, 8.0) } else { (8.0, 16.0) };
            let mut boxes = vec![if top { [0.0, 8.0, 0.0, 16.0, 16.0, 16.0] } else { [0.0, 0.0, 0.0, 16.0, 8.0, 16.0] }];
            for (bit, x, z) in [(1, 0.0, 0.0), (2, 8.0, 0.0), (4, 0.0, 8.0), (8, 8.0, 8.0)] {
                if bits & bit != 0 {
                    boxes.push([x, y0, z, x + 8.0, y1, z + 8.0]);
                }
            }
            Some(boxes)
        }
        // `CrossCollisionBlock`: a post, and an arm to each side it connects to.
        "IronBarsBlock" | "StainedGlassPaneBlock" | "FenceBlock" => {
            let (node, arm) = if class == "FenceBlock" { (2.0, 2.0) } else { (1.0, 1.0) };
            let (a0, a1) = (8.0 - arm, 8.0 + arm);
            let mut boxes = vec![[8.0 - node, 0.0, 8.0 - node, 8.0 + node, 16.0, 8.0 + node]];
            for (side, b) in [
                ("north", [a0, 0.0, 0.0, a1, 16.0, a1]),
                ("south", [a0, 0.0, a0, a1, 16.0, 16.0]),
                ("west", [0.0, 0.0, a0, a1, 16.0, a1]),
                ("east", [a0, 0.0, a0, 16.0, 16.0, a1]),
            ] {
                if on(side) {
                    boxes.push(b);
                }
            }
            Some(boxes)
        }
        "FenceGateBlock" => {
            let h = if on("in_wall") { 13.0 } else { 16.0 };
            one(if x_axis { [6.0, 0.0, 0.0, 10.0, h, 16.0] } else { [0.0, 0.0, 6.0, 16.0, h, 10.0] })
        }
        "BaseTorchBlock" | "TorchBlock" | "RedstoneTorchBlock" => one([6.0, 0.0, 6.0, 10.0, 10.0, 10.0]),
        "WallTorchBlock" | "RedstoneWallTorchBlock" => match get("facing") {
            "north" => one([5.5, 3.0, 11.0, 10.5, 13.0, 16.0]),
            "south" => one([5.5, 3.0, 0.0, 10.5, 13.0, 5.0]),
            "west" => one([11.0, 3.0, 5.5, 16.0, 13.0, 10.5]),
            "east" => one([0.0, 3.0, 5.5, 5.0, 13.0, 10.5]),
            _ => None,
        },
        "LanternBlock" => {
            let y = if on("hanging") { 1.0 } else { 0.0 };
            Some(vec![[5.0, y, 5.0, 11.0, y + 7.0, 11.0], [6.0, y + 7.0, 6.0, 10.0, y + 9.0, 10.0]])
        }
        "ChainBlock" => one(match get("axis") {
            "x" => [0.0, 6.5, 6.5, 16.0, 9.5, 9.5],
            "z" => [6.5, 6.5, 0.0, 9.5, 9.5, 16.0],
            _ => [6.5, 0.0, 6.5, 9.5, 16.0, 9.5],
        }),
        "CakeBlock" => {
            let bites = get("bites").parse::<i32>().unwrap_or(0).clamp(0, 6) as f64;
            one([1.0 + 2.0 * bites, 0.0, 1.0, 15.0, 8.0, 15.0])
        }
        "FarmBlock" => one([0.0, 0.0, 0.0, 16.0, 15.0, 16.0]),
        "DecoratedPotBlock" => one([1.0, 0.0, 1.0, 15.0, 16.0, 15.0]),
        "EnderChestBlock" => one([1.0, 0.0, 1.0, 15.0, 14.0, 15.0]),
        // A single chest's box; half of a double one reaches to the side it joins: the facing's
        // clockwise for the left half, its counter-clockwise for the right.
        "ChestBlock" | "TrappedChestBlock" => one(match (get("type"), get("facing")) {
            ("left", "north") | ("right", "south") => [1.0, 0.0, 1.0, 16.0, 14.0, 15.0],
            ("left", "south") | ("right", "north") => [0.0, 0.0, 1.0, 15.0, 14.0, 15.0],
            ("left", "west") | ("right", "east") => [1.0, 0.0, 0.0, 15.0, 14.0, 15.0],
            ("left", "east") | ("right", "west") => [1.0, 0.0, 1.0, 15.0, 14.0, 16.0],
            _ => [1.0, 0.0, 1.0, 15.0, 14.0, 15.0],
        }),
        // The bowl (its top less the inside, and the funnel) with the spout the facing points.
        "HopperBlock" => {
            let mut b = vec![
                [0.0, 10.0, 0.0, 16.0, 11.0, 16.0],
                [0.0, 11.0, 0.0, 2.0, 16.0, 16.0],
                [14.0, 11.0, 0.0, 16.0, 16.0, 16.0],
                [2.0, 11.0, 0.0, 14.0, 16.0, 2.0],
                [2.0, 11.0, 14.0, 14.0, 16.0, 16.0],
                [4.0, 4.0, 4.0, 12.0, 10.0, 12.0],
            ];
            match get("facing") {
                "down" => b.push([6.0, 0.0, 6.0, 10.0, 4.0, 10.0]),
                "east" => b.push([12.0, 4.0, 6.0, 16.0, 8.0, 10.0]),
                "north" => b.push([6.0, 4.0, 0.0, 10.0, 8.0, 4.0]),
                "south" => b.push([6.0, 4.0, 12.0, 10.0, 8.0, 16.0]),
                "west" => b.push([0.0, 4.0, 6.0, 4.0, 8.0, 10.0]),
                _ => {}
            }
            Some(b)
        }
        // Scaffolding to someone not holding scaffolding: its top and posts, and its floor too
        // where it is a bottom piece.
        "ScaffoldingBlock" => {
            let mut b = vec![
                [0.0, 14.0, 0.0, 16.0, 16.0, 16.0],
                [0.0, 0.0, 0.0, 2.0, 16.0, 2.0],
                [14.0, 0.0, 0.0, 16.0, 16.0, 2.0],
                [0.0, 0.0, 14.0, 2.0, 16.0, 16.0],
                [14.0, 0.0, 14.0, 16.0, 16.0, 16.0],
            ];
            if on("bottom") {
                b.push([0.0, 0.0, 0.0, 16.0, 2.0, 16.0]);
            }
            Some(b)
        }
        _ => None,
    }
}

/// The block classes [`vanilla_state_shape`] keeps.
const STATE_SHAPED: [&str; 31] = [
    "BaseRailBlock",
    "BaseTorchBlock",
    "ButtonBlock",
    "CakeBlock",
    "ChainBlock",
    "ChestBlock",
    "DecoratedPotBlock",
    "DiodeBlock",
    "DoorBlock",
    "EnderChestBlock",
    "FarmBlock",
    "FenceBlock",
    "FenceGateBlock",
    "HopperBlock",
    "IronBarsBlock",
    "LadderBlock",
    "LanternBlock",
    "LecternBlock",
    "LeverBlock",
    "PressurePlateBlock",
    "RedstoneTorchBlock",
    "RedstoneWallTorchBlock",
    "ScaffoldingBlock",
    "SlabBlock",
    "StainedGlassPaneBlock",
    "StairBlock",
    "TorchBlock",
    "TrapDoorBlock",
    "TrappedChestBlock",
    "WallTorchBlock",
    "WeightedPressurePlateBlock",
];

/// The vanilla class whose own `getShape` a block of 1.21.1's class `class` draws, where the
/// runtime keeps it ([`STATE_SHAPED`]): the nearest of `class` and the classes above it that is one
/// (a repeater's is its diode's). None where that is `BlockBehaviour`'s full cube, or past a class
/// the generated table of 1.21.1's classes doesn't have.
fn shape_class(class: &str) -> Option<&'static str> {
    declaring_class(class, &STATE_SHAPED).filter(|c| *c != "BlockBehaviour")
}

fn state_shape_class(owner: &str) -> bool {
    owner.strip_prefix("net/minecraft/world/level/block/").is_some_and(|c| STATE_SHAPED.contains(&c))
}

/// `tick`'s descriptor: `BlockBehaviour.tick(BlockState, ServerLevel, BlockPos, RandomSource)`.
const TICK_DESC: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)V";

fn default_tick_class(owner: &str) -> bool {
    owner == "net/minecraft/world/level/block/state/BlockBehaviour"
        || owner.strip_prefix("net/minecraft/world/level/block/").is_some_and(|c| DEFAULT_TICK_BLOCKS.binary_search(&c).is_ok())
}

fn default_shape_class(owner: &str) -> bool {
    owner == "net/minecraft/world/level/block/state/BlockBehaviour"
        || owner.strip_prefix("net/minecraft/world/level/block/").is_some_and(|c| DEFAULT_SHAPE_BLOCKS.binary_search(&c).is_ok())
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(owner, DYE_COLOR | "net/minecraft/world/item/ArmorItem$Type" | BLOCK_PROPERTIES | ITEM_PROPERTIES | WOOD_TYPE | BUILDER)
        || (matches!(owner, ARMOR_ITEM | EQUIPABLE) && name == "getEquipmentSlot" && desc == "()Lnet/minecraft/world/entity/EquipmentSlot;")
        || (owner == ARMOR_ITEM && matches!(name, "getType" | "getMaterial"))
        || (owner == ARMOR_MATERIAL && matches!(name, "defense" | "enchantmentValue" | "toughness" | "knockbackResistance" | "equipSound"))
        || matches!(
            (owner, name),
            ("net/minecraft/world/level/block/SignBlock", "getWoodType")
                | ("net/minecraft/world/level/block/BedBlock", "getConnectedDirection")
                | ("net/minecraft/world/level/block/FaceAttachedHorizontalDirectionalBlock", "getConnectedDirection")
        )
        || (name == "type" && owner.ends_with("SignBlock") && owner.starts_with("net/minecraft/world/level/block/"))
        || (name == "getShape" && (default_shape_class(owner) || state_shape_class(owner)))
        || (name == "tick" && desc == TICK_DESC && default_tick_class(owner))
        || (name == "canSurvive" && plant_class(owner).is_some_and(|c| BUSH_SURVIVAL.contains(&c) || DOUBLE_PLANT_SURVIVAL.contains(&c)))
        || (name == "canSurvive" && api::CAN_SURVIVE.ends_with(desc) && can_survive_declarer(owner).is_some())
        || (name == "mayPlaceOn" && plant_class(owner).is_some_and(|c| BUSH_SOIL.contains(&c)))
        || (owner == "net/minecraft/world/level/block/DoublePlantBlock" && name == "placeAt")
        || matches!(name, "values" | "registerDefaultState" | "defaultBlockState" | "defaultMapColor")
        || (name == "getSerializedName" && enum_class(owner))
        || (name == "toString" && enum_class(owner) && owner.starts_with("net/minecraft/world/level/block/state/properties/"))
        || (owner.starts_with("net/minecraft/world/level/block/") && matches!(name, "createBlockStateDefinition" | "getStateForPlacement" | "<init>"))
        || ((owner.starts_with("net/minecraft/world/item/") || owner == DEFERRED_SPAWN_EGG) && name == "<init>")
        || owner == CAULDRON_INTERACTION
        || owner == ARMOR_LAYER
        || (owner == DISPENSER && matches!(name, "registerBehavior" | "registerProjectileBehavior"))
        || (owner.starts_with("net/minecraft/core/dispenser/") && name == "<init>")
        || (owner == BLOCK_SET_TYPE && matches!(name, "<init>" | "register" | "values"))
        || (owner == BRUSHABLE && matches!(name, "getTurnsInto" | "getBrushSound" | "getBrushCompletedSound"))
        || (owner == RAIL_SHAPE && name == "isAscending")
        || (matches!(owner, SOUND_TYPE | DEFERRED_SOUND_TYPE) && (name == "<init>" || SOUND_GETTERS.iter().any(|(g, ..)| *g == name) || matches!(name, "getVolume" | "getPitch")))
        || (name == "useItemOn" && cauldron_class(owner))
        || (owner == LAYERED_CAULDRON && name == "lowerFillLevel")
        || (matches!(owner, "net/minecraft/world/item/SwordItem" | "net/minecraft/world/item/DiggerItem" | "net/minecraft/world/item/AxeItem" | "net/minecraft/world/item/HoeItem" | "net/minecraft/world/item/PickaxeItem" | "net/minecraft/world/item/ShovelItem") && name == "createAttributes")
        || matches!(owner, TIERS | SIMPLE_TIER)
        || (owner == "net/minecraft/world/item/Tier" && name.starts_with("get"))
}

const TIERS: &str = "net/minecraft/world/item/Tiers";
const DEFERRED_SPAWN_EGG: &str = "net/neoforged/neoforge/common/DeferredSpawnEggItem";
const SIMPLE_TIER: &str = "net/neoforged/neoforge/common/SimpleTier";
/// Vanilla's tool tiers as 1.21.1 declares them: (constant, blocks they can't drop, uses, speed,
/// attack damage bonus, enchantment value).
const TIER_VALUES: [(&str, &str, i32, f32, f32, i32); 6] = [
    ("WOOD", "minecraft:incorrect_for_wooden_tool", 59, 2.0, 0.0, 15),
    ("STONE", "minecraft:incorrect_for_stone_tool", 131, 4.0, 1.0, 5),
    ("IRON", "minecraft:incorrect_for_iron_tool", 250, 6.0, 2.0, 14),
    ("DIAMOND", "minecraft:incorrect_for_diamond_tool", 1561, 8.0, 3.0, 10),
    ("GOLD", "minecraft:incorrect_for_gold_tool", 32, 12.0, 0.0, 22),
    ("NETHERITE", "minecraft:incorrect_for_netherite_tool", 2031, 9.0, 4.0, 15),
];

/// A tier's own answers: vanilla's `Tiers` from the table, NeoForge's `SimpleTier` from what its
/// constructor got.
fn tier_method(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let t = a(0);
    // A `SimpleTier`, or a mod's class extending it.
    if owner == SIMPLE_TIER || rt.class_of(t) == SIMPLE_TIER || !matches!(rt.get_field(t, "$uses"), V::Void) {
        if name == "<init>" {
            // `SimpleTier(incorrectBlocksForDrops, uses, speed, attackDamageBonus, enchantmentValue, repairIngredient)`.
            if !desc.starts_with("(Lnet/minecraft/tags/TagKey;IFFI") {
                return None;
            }
            for (i, f) in ["$incorrect", "$uses", "$speed", "$damage", "$enchantment", "$repair"].into_iter().enumerate() {
                rt.put_field(t, f, a(i + 1));
            }
            return Some(V::Void);
        }
        return Some(match name {
            "getUses" => rt.get_field(t, "$uses"),
            "getSpeed" => rt.get_field(t, "$speed"),
            "getAttackDamageBonus" => rt.get_field(t, "$damage"),
            "getEnchantmentValue" => rt.get_field(t, "$enchantment"),
            "getIncorrectBlocksForDrops" => rt.get_field(t, "$incorrect"),
            "getRepairIngredient" => {
                let supplier = rt.get_field(t, "$repair");
                rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", supplier, &[])
            }
            _ => return None,
        });
    }
    let i = rt.get_field(t, "$ordinal").i();
    let &(_, incorrect, uses, speed, damage, enchantment) = TIER_VALUES.get(usize::try_from(i).ok()?)?;
    Some(match name {
        "getUses" => V::I(uses),
        "getSpeed" => V::F(speed),
        "getAttackDamageBonus" => V::F(damage),
        "getEnchantmentValue" => V::I(enchantment),
        "getIncorrectBlocksForDrops" => crate::fluids::tag_key(rt, "block", incorrect),
        _ => return None,
    })
}

const DISPENSER: &str = "net/minecraft/world/level/block/DispenserBlock";
const ARMOR_LAYER: &str = "net/minecraft/world/item/ArmorMaterial$Layer";
const ARMOR_MATERIAL: &str = "net/minecraft/world/item/ArmorMaterial";
const ARMOR_TYPE: &str = "net/minecraft/world/item/ArmorItem$Type";
const ARMOR_ITEM: &str = "net/minecraft/world/item/ArmorItem";
const EQUIPABLE: &str = "net/minecraft/world/item/Equipable";
const EQUIPMENT_SLOT: &str = "net/minecraft/world/entity/EquipmentSlot";
/// `ArmorItem.Type`'s slots, by its ordinal (helmet, chestplate, leggings, boots, body).
const ARMOR_SLOTS: [&str; 5] = ["HEAD", "CHEST", "LEGS", "FEET", "BODY"];

/// The slot of armour type `ty` (an `ArmorItem.Type`): `Type.getSlot()`. None for no type.
fn armor_slot(rt: &mut Rt, ty: V) -> Option<V> {
    if ty.r() == 0 {
        return None;
    }
    let slot = ARMOR_SLOTS.get(rt.get_field(ty, "$ordinal").i() as usize)?;
    crate::api_common::get_static(rt, EQUIPMENT_SLOT, slot)
}

/// 1.21.1's armour materials (`ArmorMaterials`): defense by `ArmorItem.Type` (helmet, chestplate,
/// leggings, boots, body), enchantment value, equip sound, toughness, knockback resistance.
const ARMOR_MATERIALS: [(&str, [i32; 5], i32, &str, f32, f32); 8] = [
    ("minecraft:leather", [1, 3, 2, 1, 3], 15, "item.armor.equip_leather", 0.0, 0.0),
    ("minecraft:chainmail", [2, 5, 4, 1, 4], 12, "item.armor.equip_chain", 0.0, 0.0),
    ("minecraft:iron", [2, 6, 5, 2, 5], 9, "item.armor.equip_iron", 0.0, 0.0),
    ("minecraft:gold", [2, 5, 3, 1, 7], 25, "item.armor.equip_gold", 0.0, 0.0),
    ("minecraft:diamond", [3, 8, 6, 3, 11], 10, "item.armor.equip_diamond", 2.0, 0.0),
    ("minecraft:turtle", [2, 6, 5, 2, 5], 9, "item.armor.equip_turtle", 0.0, 0.0),
    ("minecraft:netherite", [3, 8, 6, 3, 11], 15, "item.armor.equip_netherite", 3.0, 0.1),
    ("minecraft:armadillo", [3, 8, 6, 3, 11], 10, "item.armor.equip_wolf", 0.0, 0.0),
];

/// A vanilla armour material's record component `name` (`defense()`, `toughness()`...).
fn vanilla_armor_material(rt: &mut Rt, name: &str, material: V) -> Option<V> {
    let id = rt.str_of(material).unwrap_or("").to_owned();
    let &(_, defense, enchantment, sound, toughness, knockback) = ARMOR_MATERIALS.iter().find(|m| m.0 == id)?;
    Some(match name {
        "defense" => {
            const TYPES: [&str; 5] = ["HELMET", "CHESTPLATE", "LEGGINGS", "BOOTS", "BODY"];
            let m = rt.alloc("java/util/EnumMap", Data::Table(Default::default()));
            for (i, (ty, d)) in TYPES.into_iter().zip(defense).enumerate() {
                let k = api::api_enum(rt, ARMOR_TYPE, ty, i as i32);
                let v = crate::api_common::integer(rt, d);
                crate::jutil::table_put(rt, m, k, v);
            }
            m
        }
        "enchantmentValue" => V::I(enchantment),
        "toughness" => V::F(toughness),
        "knockbackResistance" => V::F(knockback),
        "equipSound" => crate::sounds::get_static(rt, "net/minecraft/sounds/SoundEvents", &sound_field(sound)).unwrap_or(V::NULL),
        _ => return None,
    })
}

/// `SoundEvents`' field for sound `id` (`item.armor.equip_iron` is `ARMOR_EQUIP_IRON`).
fn sound_field(id: &str) -> String {
    id.trim_start_matches("item.").replace('.', "_").to_ascii_uppercase()
}

/// `DispenserBlock.DISPENSER_REGISTRY`: the behaviours mods registered, by item.
pub(crate) fn dispenser_behaviours(rt: &mut Rt) -> V {
    rt.intern("compat:dispenser-behaviours".into(), |rt| rt.alloc("java/util/IdentityHashMap", Data::Table(Default::default())))
}

const CAULDRON_INTERACTION: &str = "net/minecraft/core/cauldron/CauldronInteraction";
const INTERACTION_MAP: &str = "net/minecraft/core/cauldron/CauldronInteraction$InteractionMap";
const INTERACT: &str = "net/minecraft/core/cauldron/CauldronInteraction.interact.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/ItemInteractionResult;";

const LAYERED_CAULDRON: &str = "net/minecraft/world/level/block/LayeredCauldronBlock";
/// `CauldronInteraction`'s interaction constants, in the order its static initializer makes them
/// (from `lambda$static$0` on).
const VANILLA_INTERACTIONS: [&str; 6] = ["FILL_WATER", "FILL_LAVA", "FILL_POWDER_SNOW", "SHULKER_BOX", "BANNER", "DYED_ITEM"];

/// `CauldronInteraction.interact`'s descriptor.
fn interact_desc() -> &'static str {
    INTERACT.rsplit_once(".interact.").map_or("", |(_, d)| d)
}

/// Vanilla's `CauldronInteraction.DYED_ITEM`: a dyeable stack with a colour loses it, and the
/// cauldron a level of water (on the server; the stat vanilla awards isn't counted for mods);
/// anything else passes to the block. `args` are `interact`'s.
fn wash_dye(rt: &mut Rt, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (state, level, pos, stack) = (a(0), a(1), a(2), a(5));
    let dyeable = api::get_static(rt, "net/minecraft/tags/ItemTags.DYEABLE");
    let dyed = api::get_static(rt, "net/minecraft/core/component/DataComponents.DYED_COLOR");
    let has = "net/minecraft/world/item/ItemStack.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
    if api::call(rt, "net/minecraft/world/item/ItemStack.is.(Lnet/minecraft/tags/TagKey;)Z", &[stack, dyeable]).i() == 0 || api::call(rt, has, &[stack, dyed]).i() == 0 {
        return api_common::result(rt, "net/minecraft/world/ItemInteractionResult", "PASS_TO_DEFAULT_BLOCK_INTERACTION");
    }
    let client = api::call(rt, "net/minecraft/world/level/Level.isClientSide.()Z", &[level]).i() != 0;
    if !client {
        api::call(rt, "net/minecraft/world/item/ItemStack.remove.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;", &[stack, dyed]);
        lower_fill_level(rt, state, level, pos);
    }
    let sided = "net/minecraft/world/ItemInteractionResult.sidedSuccess.(Z)Lnet/minecraft/world/ItemInteractionResult;";
    api::call(rt, sided, &[V::I(client as i32)])
}

/// `LayeredCauldronBlock.lowerFillLevel`: the cauldron `state` at `pos` a level lower, an empty
/// cauldron below the first.
fn lower_fill_level(rt: &mut Rt, state: V, level: V, pos: V) {
    let text = rt.str_of(state).unwrap_or("minecraft:water_cauldron[level=1]").to_owned();
    let (block, mut props) = crate::host::split_state(&text);
    let lowered = props.iter().find(|(k, _)| *k == "level").and_then(|(_, v)| v.parse::<i32>().ok()).unwrap_or(1) - 1;
    let n = lowered.to_string();
    let next = if lowered <= 0 {
        "minecraft:cauldron".to_owned()
    } else {
        props.retain(|(k, _)| *k != "level");
        props.push(("level", &n));
        crate::host::join_state(block, props)
    };
    let s = api::state(rt, &next);
    let set = "net/minecraft/world/level/Level.setBlockAndUpdate.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Z";
    api::call(rt, set, &[level, pos, s]);
}

/// Vanilla's cauldron blocks (`AbstractCauldronBlock`, `LayeredCauldronBlock`...): the ones whose
/// clicks their interaction map answers.
pub(crate) fn cauldron_class(owner: &str) -> bool {
    owner.starts_with("net/minecraft/world/level/block/") && owner.ends_with("CauldronBlock") && !owner[32..].contains('/')
}

/// `CauldronInteraction.INTERACTIONS`: the interaction maps by name.
fn cauldron_maps(rt: &mut Rt) -> V {
    rt.intern("compat:cauldron-interactions".into(), |rt| rt.alloc("java/util/HashMap", Data::Table(Default::default())))
}

/// The interaction map named `name` (vanilla's `empty`, `water`, `lava`, `powder_snow`, or a mod's),
/// made by `CauldronInteraction.newInteractionMap` when `fresh` or none of that name exists yet: a
/// record of its name and the items' interactions, kept in `INTERACTIONS`.
fn interaction_map(rt: &mut Rt, name: &str, fresh: bool) -> V {
    let all = cauldron_maps(rt);
    let key = rt.string(name);
    if !fresh && let Some(m) = jutil::table_get(rt, all, key).filter(|m| m.r() != 0) {
        return m;
    }
    let m = rt.new_object(INTERACTION_MAP);
    rt.put_field(m, "name", key);
    let items = rt.alloc("it/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap", Data::Table(Default::default()));
    rt.put_field(m, "map", items);
    jutil::table_put(rt, all, key, m);
    m
}

const BLOCK_SET_TYPE: &str = "net/minecraft/world/level/block/state/properties/BlockSetType";
const BRUSHABLE: &str = "net/minecraft/world/level/block/BrushableBlock";
const DEFERRED_SOUND_TYPE: &str = "net/neoforged/neoforge/common/util/DeferredSoundType";

/// The vanilla `SoundType` constant `t` is (`SoundType.METAL`), by its row of [`VANILLA_SOUND_TYPES`].
fn vanilla_sound_type(rt: &Rt, t: V) -> Option<&'static (&'static str, f32, f32, [&'static str; 5])> {
    let n = rt.str_of(rt.get_field(t, "$name"))?;
    VANILLA_SOUND_TYPES.iter().find(|(f, ..)| *f == n)
}

/// The `SoundType` the game's block sounds `s` are: vanilla's constant with that volume, pitch and
/// those five sounds (`SoundType.STONE`, the very object a mod compares with), else a new one
/// holding them.
pub(crate) fn sound_type_of(rt: &mut Rt, s: &crate::host::BlockSounds) -> V {
    let path = |field: &str| {
        let all = &crate::vanilla_sounds::SOUND_EVENTS;
        all.binary_search_by(|(f, ..)| (*f).cmp(field)).ok().map(|i| all[i].1)
    };
    let same = |a: f32, b: f32| (a - b).abs() < 1e-4;
    let row = VANILLA_SOUND_TYPES.iter().find(|(_, volume, pitch, events)| {
        same(*volume, s.volume) && same(*pitch, s.pitch) && events.iter().zip(&s.events).all(|(f, id)| path(f).is_some_and(|p| id.strip_prefix("minecraft:") == Some(p)))
    });
    if let Some(&(name, ..)) = row {
        return named(rt, SOUND_TYPE, name);
    }
    let t = rt.new_object(SOUND_TYPE);
    rt.put_field(t, "volume", V::F(s.volume));
    rt.put_field(t, "pitch", V::F(s.pitch));
    for (&(_, field, _), id) in SOUND_GETTERS.iter().zip(&s.events) {
        let event = rt.intern(format!("sound/{id}"), |rt| crate::sounds::sound_event(rt, id, None));
        rt.put_field(t, field, event);
    }
    t
}

/// The `SoundType` a mod block's `Properties` give it: what `sound(...)` set, else the vanilla
/// block's it copies (`ofFullCopy`) as the game has them, else `Properties.of()`'s stone.
pub(crate) fn properties_sound_type(rt: &mut Rt, block: V) -> V {
    let props = rt.get_field(block, "$properties");
    let own = rt.get_field(props, "$sound");
    if props.r() != 0 && own.r() != 0 {
        return own;
    }
    let copy = rt.str_of(rt.get_field(props, "$copy")).map(str::to_owned);
    if let Some(s) = copy.and_then(|c| rt.host().and_then(|h| h.block_sounds(&c))) {
        return sound_type_of(rt, &s);
    }
    named(rt, SOUND_TYPE, "STONE")
}

/// Whether a mod block's `Properties` say only the right tool gets its drops
/// (`requiresCorrectToolForDrops()`), or the vanilla block's they copy, as the game has it.
pub(crate) fn properties_require_tool(rt: &Rt, block: V) -> bool {
    let props = rt.get_field(block, "$properties");
    if props.r() == 0 {
        return false;
    }
    match rt.get_field(props, "$requiresTool") {
        V::Void | V::NULL => rt.str_of(rt.get_field(props, "$copy")).is_some_and(|c| {
            rt.host().map_or_else(|| COPIED.iter().any(|&(n, .., tool, _)| n == c && tool), |h| h.block_requires_tool(c))
        }),
        v => v.i() != 0,
    }
}
/// `SoundType`'s sound getters, the fields vanilla keeps them in (in its constructor's order) and
/// those a `DeferredSoundType` keeps its suppliers in.
const SOUND_GETTERS: [(&str, &str, &str); 5] = [
    ("getBreakSound", "breakSound", "$breakSound"),
    ("getStepSound", "stepSound", "$stepSound"),
    ("getPlaceSound", "placeSound", "$placeSound"),
    ("getHitSound", "hitSound", "$hitSound"),
    ("getFallSound", "fallSound", "$fallSound"),
];

/// What a block set type is made of past its name: opens by hand, by wind charge, buttons take
/// arrows, plates sense mobs alone, its `SoundType`, and its sounds' prefixes in `SoundEvents`
/// (doors and trapdoors, pressure plates, buttons).
type BlockSet = (bool, bool, bool, bool, &'static str, [&'static str; 3]);
const WOODEN_SET: BlockSet = (true, true, true, false, "WOOD", ["WOODEN", "WOODEN", "WOODEN"]);
const STONE_SET: BlockSet = (true, true, false, true, "STONE", ["IRON", "STONE", "STONE"]);
/// Vanilla's block set types, in 1.21.1's order (`BlockSetType`'s static fields).
const BLOCK_SET_TYPES: [(&str, &str, BlockSet); 16] = [
    ("IRON", "iron", (false, false, false, false, "METAL", ["IRON", "METAL", "STONE"])),
    ("COPPER", "copper", (true, true, false, false, "COPPER", ["COPPER", "METAL", "STONE"])),
    ("GOLD", "gold", (false, true, false, false, "METAL", ["IRON", "METAL", "STONE"])),
    ("STONE", "stone", STONE_SET),
    ("POLISHED_BLACKSTONE", "polished_blackstone", STONE_SET),
    ("OAK", "oak", WOODEN_SET),
    ("SPRUCE", "spruce", WOODEN_SET),
    ("BIRCH", "birch", WOODEN_SET),
    ("ACACIA", "acacia", WOODEN_SET),
    ("CHERRY", "cherry", (true, true, true, false, "CHERRY_WOOD", ["CHERRY_WOOD", "CHERRY_WOOD", "CHERRY_WOOD"])),
    ("JUNGLE", "jungle", WOODEN_SET),
    ("DARK_OAK", "dark_oak", WOODEN_SET),
    ("CRIMSON", "crimson", (true, true, true, false, "NETHER_WOOD", ["NETHER_WOOD", "NETHER_WOOD", "NETHER_WOOD"])),
    ("WARPED", "warped", (true, true, true, false, "NETHER_WOOD", ["NETHER_WOOD", "NETHER_WOOD", "NETHER_WOOD"])),
    ("MANGROVE", "mangrove", WOODEN_SET),
    ("BAMBOO", "bamboo", (true, true, true, false, "BAMBOO_WOOD", ["BAMBOO_WOOD", "BAMBOO_WOOD", "BAMBOO_WOOD"])),
];

/// Sets record `t`'s components (as `BlockSetType`'s canonical constructor would) from `set`.
fn fill_block_set_type(rt: &mut Rt, t: V, name: V, set: BlockSet) {
    let (hand, wind, arrows, mobs, sound, [door, plate, button]) = set;
    rt.put_field(t, "name", name);
    rt.put_field(t, "canOpenByHand", V::I(hand as i32));
    rt.put_field(t, "canOpenByWindCharge", V::I(wind as i32));
    rt.put_field(t, "canButtonBeActivatedByArrows", V::I(arrows as i32));
    let sensitivity = api_common::get_static(rt, "net/minecraft/world/level/block/state/properties/BlockSetType$PressurePlateSensitivity", if mobs { "MOBS" } else { "EVERYTHING" }).unwrap_or(V::NULL);
    rt.put_field(t, "pressurePlateSensitivity", sensitivity);
    let sound_type = get_static(rt, SOUND_TYPE, sound).unwrap_or(V::NULL);
    rt.put_field(t, "soundType", sound_type);
    let sounds = [
        ("doorClose", format!("{door}_DOOR_CLOSE")),
        ("doorOpen", format!("{door}_DOOR_OPEN")),
        ("trapdoorClose", format!("{door}_TRAPDOOR_CLOSE")),
        ("trapdoorOpen", format!("{door}_TRAPDOOR_OPEN")),
        ("pressurePlateClickOff", format!("{plate}_PRESSURE_PLATE_CLICK_OFF")),
        ("pressurePlateClickOn", format!("{plate}_PRESSURE_PLATE_CLICK_ON")),
        ("buttonClickOff", format!("{button}_BUTTON_CLICK_OFF")),
        ("buttonClickOn", format!("{button}_BUTTON_CLICK_ON")),
    ];
    for (field, event) in sounds {
        let v = crate::sounds::get_static(rt, "net/minecraft/sounds/SoundEvents", &event).unwrap_or(V::NULL);
        rt.put_field(t, field, v);
    }
}

/// `BlockSetType.TYPES`' values: vanilla's, made once in their order, then those mods registered.
/// Each vanilla one is also interned as its static field's value.
fn block_set_types(rt: &mut Rt) -> V {
    if let Some(&all) = rt.interned.get("compat:block-set-types") {
        return all;
    }
    let all = jutil::new_list(rt, Vec::new());
    rt.interned.insert("compat:block-set-types".into(), all);
    for (field, name, set) in BLOCK_SET_TYPES {
        let t = rt.new_object(BLOCK_SET_TYPE);
        let n = rt.string(name);
        fill_block_set_type(rt, t, n, set);
        rt.interned.insert(format!("const/{BLOCK_SET_TYPE}.{field}"), t);
        jutil::list_add(rt, all, t);
    }
    all
}

/// `AbstractCauldronBlock.useItemOn`: the interaction block `b`'s map holds for the held stack's item,
/// else `PASS_TO_DEFAULT_BLOCK_INTERACTION` (the map's default). `args` are `useItemOn`'s.
pub(crate) fn cauldron_use(rt: &mut Rt, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let stack = a(1);
    let item = api::call(rt, "net/minecraft/world/item/ItemStack.getItem.()Lnet/minecraft/world/item/Item;", &[stack]);
    let map = rt.get_field(rt.get_field(a(0), "interactions"), "map");
    match jutil::table_get(rt, map, item).filter(|f| f.r() != 0) {
        Some(f) => rt.call_functional(INTERACT, f, &[a(2), a(3), a(4), a(5), a(6), stack]),
        None => api_common::result(rt, "net/minecraft/world/ItemInteractionResult", "PASS_TO_DEFAULT_BLOCK_INTERACTION"),
    }
}

/// given, block constructors collect their states.
/// Vanilla 1.21.1's plant classes whose `canSurvive` is `BushBlock`'s: it, and the ones that
/// inherit it unchanged (their class files declare no `canSurvive` down to `BushBlock`).
const BUSH_SURVIVAL: [&str; 17] = [
    "AttachedStemBlock", "AzaleaBlock", "BushBlock", "DeadBushBlock", "FlowerBlock", "FungusBlock", "NetherSproutsBlock", "NetherWartBlock",
    "PinkPetalsBlock", "RootsBlock", "SaplingBlock", "SeagrassBlock", "StemBlock", "SweetBerryBushBlock", "TallGrassBlock", "WaterlilyBlock",
    "WitherRoseBlock",
];
/// The ones whose `canSurvive` is `DoublePlantBlock`'s.
const DOUBLE_PLANT_SURVIVAL: [&str; 2] = ["DoublePlantBlock", "TallFlowerBlock"];
/// The plant classes whose `mayPlaceOn` is `BushBlock`'s (dirt or farmland).
const BUSH_SOIL: [&str; 8] = ["BushBlock", "DoublePlantBlock", "FlowerBlock", "PinkPetalsBlock", "SaplingBlock", "SweetBerryBushBlock", "TallFlowerBlock", "TallGrassBlock"];
const PLANTS: &str = "net/minecraft/world/level/block/";
const MAY_PLACE_ON: &str = "mayPlaceOn.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const MAY_PLACE_ON_KEY: &str =
    "net/minecraft/world/level/block/BushBlock.mayPlaceOn.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const SUSTAIN: &str = "canSustainPlant.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/neoforged/neoforge/common/util/TriState;";
const SUSTAIN_KEY: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension.canSustainPlant.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/neoforged/neoforge/common/util/TriState;";

/// A vanilla plant class a mod plant's `super.canSurvive`/`super.mayPlaceOn` names: its simple name.
fn plant_class(owner: &str) -> Option<&str> {
    owner.strip_prefix(PLANTS).filter(|c| !c.contains('/'))
}

/// `BushBlock.mayPlaceOn` (dirt or farmland) for a plant class whose own it is; None for another's.
/// 1.21.1's `BlockTags.DIRT` holds grass and podzol, which 26.3's `dirt` doesn't.
fn bush_soil(rt: &Rt, class: &str, soil: &str) -> Option<bool> {
    BUSH_SOIL.contains(&class).then(|| {
        let (block, _) = crate::host::split_state(soil);
        let dirt = crate::vanilla_tags::current("block", "minecraft:dirt").unwrap_or_else(|| "minecraft:dirt".into());
        block == "minecraft:farmland" || rt.host().is_some_and(|h| h.block_in_tag(block, &dirt))
    })
}

/// NeoForge 1.21.1's `canSurvive` of vanilla plant class `class` for the mod plant `this` (a mod
/// block's `super.canSurvive(state, level, pos)`): `DoublePlantBlock`'s checks its lower half
/// below an upper one; `BushBlock`'s asks the soil (`canSustainPlant`: a mod soil's own answer,
/// vanilla's soils leave it to the plant), then the plant's `mayPlaceOn` (the mod's own, else
/// vanilla's). None for a class not kept here.
fn plant_survives(rt: &mut Rt, class: &str, this: V, state: V, level: V, pos: V) -> Option<bool> {
    let text = api_common::text_of(rt, state);
    let p = api::pos_of(rt, pos);
    let below = [p[0], p[1] - 1, p[2]];
    let soil = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(below));
    if DOUBLE_PLANT_SURVIVAL.contains(&class) {
        let (block, props) = crate::host::split_state(&text);
        // An upper half needs this plant's lower half below it; the check made before the block is
        // set (another block's state) is the bush's.
        if props.contains(&("half", "upper")) && crate::glue::mod_block(rt, &text).is_some_and(|(b, _)| b == this) {
            let (under, under_props) = crate::host::split_state(&soil);
            return Some(under == block && under_props.contains(&("half", "lower")));
        }
    } else if !BUSH_SURVIVAL.contains(&class) {
        return None;
    }
    let soil_state = api::state(rt, &soil);
    let soil_pos = api::block_pos(rt, below[0].into(), below[1].into(), below[2].into());
    if let Some((block, soil_class)) = crate::glue::mod_block(rt, &soil)
        && rt.overrides.is_some_and(|o| o(soil_class, SUSTAIN))
        && let Some(vc) = rt.vcall
    {
        let up = api::direction(rt, 1);
        let decision = vc(rt, SUSTAIN_KEY, SUSTAIN, &[block, soil_state, level, soil_pos, up, state]);
        match rt.str_of(rt.get_field(decision, "$name")) {
            Some("TRUE") => return Some(true),
            Some("FALSE") => return Some(false),
            _ => {}
        }
    }
    let plant = rt.class_of(this);
    if rt.overrides.is_some_and(|o| o(plant, MAY_PLACE_ON))
        && let Some(vc) = rt.vcall
    {
        return Some(vc(rt, MAY_PLACE_ON_KEY, MAY_PLACE_ON, &[this, soil_state, level, soil_pos]).i() != 0);
    }
    // `DoublePlantBlock`'s and the bush's own soil; a class between with another (`DeadBushBlock`'s
    // sand) isn't kept.
    bush_soil(rt, if DOUBLE_PLANT_SURVIVAL.contains(&class) { "DoublePlantBlock" } else { class }, &soil)
}

/// `state.canSurvive(level, pos)` of the mod block `b` (class `class`): the block's own
/// `canSurvive` where its class has one, else its vanilla plant base's as NeoForge 1.21.1 has it,
/// which asks the block's own `mayPlaceOn` (Farmer's Delight's wild crops: dirt or sand), or a
/// diode's (a rigid top face below, [`crate::diodes`]). `None` where none is the block's (its base
/// isn't one kept here), or its code threw: the game's rule for the block's template answers then.
pub(crate) fn mod_survives(rt: &mut Rt, b: V, class: &'static str, state: V, level: V, pos: V) -> Option<bool> {
    let survives = survives_by_code(rt, b, class, state, level, pos);
    if rt.thrown.is_some() {
        rt.report_thrown("canSurvive");
        return None;
    }
    survives
}

/// [`mod_survives`] for the mod's own code, which sees what its `canSurvive` throws.
pub(crate) fn survives_by_code(rt: &mut Rt, b: V, class: &'static str, state: V, level: V, pos: V) -> Option<bool> {
    if rt.overrides.is_some_and(|o| o(class, api::CAN_SURVIVE)) {
        let vc = rt.vcall?;
        return Some(vc(rt, api::CAN_SURVIVE_KEY, api::CAN_SURVIVE, &[b, state, level, pos]).i() != 0);
    }
    // A diode's: on a rigid top face, by its own `canSurviveOn` where it has one; a rail's, on a
    // rigid top face.
    if let Some(key) = crate::glue::inherited(rt, class, api::CAN_SURVIVE) {
        let vc = rt.vcall?;
        return Some(vc(rt, key, api::CAN_SURVIVE, &[b, state, level, pos]).i() != 0);
    }
    plant_survives(rt, plant_base(rt, class)?, b, state, level, pos)
}

/// `canSurvive` as vanilla block class `class` (or the nearest class above it that declares one)
/// has it, for `state` at `pos`: the host runs that class's rule where the state's block has the
/// class among its own; true past `BlockBehaviour`'s, and where the host runs nothing.
fn vanilla_survives(rt: &mut Rt, class: &str, state: V, pos: V) -> bool {
    let classes = can_survive_ancestry(class);
    if classes.is_empty() {
        return true;
    }
    let text = api_common::text_of(rt, state);
    let at = api::pos_of(rt, pos);
    rt.host()
        .and_then(|h| h.vanilla_can_survive(&classes, at, &text))
        .unwrap_or(true)
}

/// The vanilla plant class (its simple name) a mod block's class is or extends.
fn plant_base(rt: &Rt, class: &'static str) -> Option<&'static str> {
    plant_class(if class.starts_with("net/minecraft/") { class } else { crate::rotation::vanilla_base(rt, class) })
}

/// The mod's blocks whose survival its own code decides: a class with its own `canSurvive`, a
/// diode, a rail, or a vanilla plant base kept here (whose `canSurvive` asks the block's `mayPlaceOn`). The game asks
/// [`mod_survives`] about these only.
pub fn survival_blocks(rt: &Rt) -> Vec<String> {
    let mut blocks: Vec<String> = rt
        .registered
        .iter()
        .filter(|((registry, _), _)| registry == "block")
        .filter(|(_, b)| {
            let class = rt.class_of(**b);
            rt.overrides.is_some_and(|o| o(class, api::CAN_SURVIVE))
                || crate::glue::inherited(rt, class, api::CAN_SURVIVE).is_some()
                || plant_base(rt, class).is_some_and(|c| BUSH_SURVIVAL.contains(&c) || DOUBLE_PLANT_SURVIVAL.contains(&c))
        })
        .map(|((_, name), _)| name.clone())
        .collect();
    blocks.sort();
    blocks
}

/// `mayPlaceOn(ground, level, groundPos)` of the mod plant `b` (class `class`): its own where it
/// has one. `None` where it hasn't, or its code threw: the plant's template decides.
pub(crate) fn mod_may_place_on(rt: &mut Rt, b: V, class: &'static str, ground: V, level: V, ground_pos: V) -> Option<bool> {
    if !rt.overrides.is_some_and(|o| o(class, MAY_PLACE_ON)) {
        return None;
    }
    let vc = rt.vcall?;
    let may = vc(rt, MAY_PLACE_ON_KEY, MAY_PLACE_ON, &[b, ground, level, ground_pos]);
    if rt.thrown.is_some() {
        rt.report_thrown("mayPlaceOn");
        return None;
    }
    Some(may.i() != 0)
}

/// `DoublePlantBlock.placeAt(level, state, pos, flags)`: the lower half at `pos` and the upper above
/// it, each waterlogged where the level has water (`copyWaterloggedFrom`), when the state has the
/// property.
fn place_double_plant(rt: &mut Rt, level: V, state: V, pos: V, flags: V) {
    let text = api_common::text_of(rt, state);
    let p = api::pos_of(rt, pos);
    for (y, half) in [(p[1], "lower"), (p[1] + 1, "upper")] {
        let at = [p[0], y, p[2]];
        let (block, props) = crate::host::split_state(&text);
        let water = props.iter().any(|(k, _)| *k == "waterlogged") && {
            let here = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(at));
            let fluid = crate::fluids::of_block_state(rt, &here);
            let tag = crate::fluids::tag_key(rt, "fluid", "minecraft:water");
            api::call(rt, "net/minecraft/world/level/material/FluidState.is.(Lnet/minecraft/tags/TagKey;)Z", &[fluid, tag]).i() != 0
        };
        let props: Vec<String> = props
            .iter()
            .map(|&(k, v)| match k {
                "half" => format!("{k}={half}"),
                "waterlogged" => format!("{k}={water}"),
                _ => format!("{k}={v}"),
            })
            .collect();
        let placed = if props.is_empty() { block.to_owned() } else { format!("{block}[{}]", props.join(",")) };
        let placed = api::state(rt, &placed);
        let at = api::block_pos(rt, at[0].into(), at[1].into(), at[2].into());
        api::call(rt, "net/minecraft/world/level/LevelAccessor.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z", &[level, at, placed, flags]);
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    match (owner, name) {
        // A mod plant's `super.canSurvive` and `super.mayPlaceOn` to a vanilla plant class.
        (o, "canSurvive") if plant_class(o).is_some_and(|c| BUSH_SURVIVAL.contains(&c) || DOUBLE_PLANT_SURVIVAL.contains(&c)) => {
            plant_survives(rt, plant_class(o)?, a(0), a(1), a(2), a(3)).map(|b| V::I(b as i32))
        }
        // ... and to any other vanilla block class: `BlockBehaviour`'s is true; a class with a rule
        // of its own (a ladder's wall, farmland's open sky) is the game's, where the block's
        // template has that class, else true (as `BlockBehaviour`'s: nothing breaks the block).
        (o, "canSurvive")
            if api::CAN_SURVIVE.ends_with(desc) && can_survive_declarer(o).is_some() =>
        {
            Some(V::I(vanilla_survives(rt, o, a(1), a(3)) as i32))
        }
        (o, "mayPlaceOn") if plant_class(o).is_some_and(|c| BUSH_SOIL.contains(&c)) => {
            let soil = api_common::text_of(rt, a(1));
            bush_soil(rt, plant_class(o)?, &soil).map(|b| V::I(b as i32))
        }
        ("net/minecraft/world/level/block/DoublePlantBlock", "placeAt") => {
            place_double_plant(rt, a(0), a(1), a(2), a(3));
            Some(V::Void)
        }
        // `BellAttachType.values()`: its constants, in order.
        (o, "values") if desc.starts_with("()[") && enum_class(o) => {
            let all = vanilla_enum_values(rt, o)?;
            let class: &'static str = rt.static_key(&format!("[L{o};"));
            Some(rt.alloc(class, Data::Array(all)))
        }
        (DYE_COLOR, _) => dye_call(rt, name, args),
        // `RailShape.isAscending()`: the four that climb.
        (RAIL_SHAPE, "isAscending") => Some(V::I(api_common::text_of(rt, a(0)).starts_with("ascending_") as i32)),
        // `super.tick(state, level, pos, random)` to a class whose scheduled tick does nothing.
        (o, "tick") if desc == TICK_DESC && default_tick_class(o) => Some(V::Void),
        // `super.getShape(state, level, pos, context)` to a class with vanilla's default.
        (o, "getShape") if desc == GET_SHAPE.trim_start_matches("getShape.") && default_shape_class(o) => Some(crate::shapes::shape(rt, &[FULL_BOX])),
        // ... and to a vanilla kind whose shape follows its state (a door's facing, hinge, openness).
        (o, "getShape") if desc == GET_SHAPE.trim_start_matches("getShape.") && state_shape_class(o) => {
            let class = o.rsplit('/').next().unwrap_or(o);
            let text = rt.str_of(a(1)).unwrap_or("").to_owned();
            let (_, props) = crate::host::split_state(&text);
            let pixels = vanilla_state_shape(class, &props)?;
            let boxes: Vec<crate::shapes::Box3> = pixels.iter().map(|b| b.map(|v| v / 16.0)).collect();
            let joined = crate::shapes::optimized(&boxes);
            Some(rt.intern(format!("vanilla-shape/{joined:?}"), move |rt| crate::shapes::shape(rt, &joined)))
        }
        // `new SoundType(volume, pitch, break, step, place, hit, fall)` and NeoForge's
        // `DeferredSoundType`, whose sounds are suppliers (of a mod's registered sound events).
        (SOUND_TYPE | DEFERRED_SOUND_TYPE, "<init>") if desc.starts_with("(FF") => {
            rt.put_field(a(0), "volume", a(1));
            rt.put_field(a(0), "pitch", a(2));
            for (i, &(_, field, supplier)) in SOUND_GETTERS.iter().enumerate() {
                rt.put_field(a(0), if owner == DEFERRED_SOUND_TYPE { supplier } else { field }, a(3 + i));
            }
            Some(V::Void)
        }
        (SOUND_TYPE | DEFERRED_SOUND_TYPE, "getVolume" | "getPitch") => {
            let field = if name == "getVolume" { "volume" } else { "pitch" };
            Some(match rt.get_field(a(0), field) {
                V::F(f) => V::F(f),
                _ => {
                    let vanilla = vanilla_sound_type(rt, a(0));
                    V::F(vanilla.map_or(1.0, |t| if field == "volume" { t.1 } else { t.2 }))
                }
            })
        }
        (SOUND_TYPE | DEFERRED_SOUND_TYPE, getter) if SOUND_GETTERS.iter().any(|(g, ..)| *g == getter) => {
            let &(_, field, supplier) = SOUND_GETTERS.iter().find(|(g, ..)| *g == getter)?;
            let own = rt.get_field(a(0), field);
            if own.r() != 0 {
                return Some(own);
            }
            let deferred = rt.get_field(a(0), supplier);
            if deferred.r() != 0 {
                return Some(rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", deferred, &[]));
            }
            // A vanilla constant: the sound its table row names.
            let i = SOUND_GETTERS.iter().position(|(g, ..)| *g == getter)?;
            let Some(event) = vanilla_sound_type(rt, a(0)).map(|t| t.3[i]) else { return Some(V::NULL) };
            Some(crate::sounds::get_static(rt, "net/minecraft/sounds/SoundEvents", event).unwrap_or(V::NULL))
        }
        // `new BlockSetType(name)`: a wooden set's (opens by hand, buttons take arrows, plates sense
        // everything, wood's sounds); the canonical constructor is the record's.
        (BLOCK_SET_TYPE, "<init>") if desc == "(Ljava/lang/String;)V" => {
            fill_block_set_type(rt, a(0), a(1), WOODEN_SET);
            Some(V::Void)
        }
        // `register(type)`: kept for `values()`, which lists vanilla's first.
        (BLOCK_SET_TYPE, "register") => {
            let all = block_set_types(rt);
            jutil::list_add(rt, all, a(0));
            Some(a(0))
        }
        (BLOCK_SET_TYPE, "values") if desc == "()Ljava/util/stream/Stream;" => {
            let all = block_set_types(rt);
            let items = jutil::elements(rt, all);
            Some(rt.alloc("java/util/stream/Stream", Data::Stream(items)))
        }
        // `DispenserBlock.registerBehavior(item, behavior)`: what a dispenser does with the item,
        // kept by item (`DISPENSER_REGISTRY`); a projectile's is vanilla's projectile behaviour.
        (DISPENSER, "registerBehavior" | "registerProjectileBehavior") => {
            let item = crate::creative::as_item(rt, a(0));
            let behavior = if name == "registerBehavior" {
                a(1)
            } else {
                let b = rt.new_object("net/minecraft/core/dispenser/ProjectileDispenseBehavior");
                rt.put_field(b, "$item", item);
                b
            };
            let all = dispenser_behaviours(rt);
            jutil::table_put(rt, all, item, behavior);
            Some(V::Void)
        }
        // A mod's behaviour built on vanilla's (`DefaultDispenseItemBehavior`, `OptionalDispense...`):
        // their state is the runtime's fields.
        (o, "<init>") if o.starts_with("net/minecraft/core/dispenser/") => Some(V::Void),
        (CAULDRON_INTERACTION, "newInteractionMap") if desc.starts_with("(Ljava/lang/String;)") => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            Some(interaction_map(rt, &n, true))
        }
        (o, "useItemOn") if cauldron_class(o) => Some(cauldron_use(rt, args)),
        // `CauldronInteraction.DYED_ITEM`'s lambda, called through a map a mod filed it in.
        (CAULDRON_INTERACTION, "lambda$static$5") if desc == interact_desc() => Some(wash_dye(rt, args)),
        (LAYERED_CAULDRON, "lowerFillLevel") => {
            lower_fill_level(rt, a(0), a(1), a(2));
            Some(V::Void)
        }
        // `defaultMapColor()`: what the block's properties give its default state (a colour, a dye's,
        // or a function of the state), else the vanilla block's they copy; vanilla's default is none.
        (_, "defaultMapColor") if desc == "()Lnet/minecraft/world/level/material/MapColor;" => {
            let b = a(0);
            let (props, vanilla) = match rt.str_of(b) {
                Some(n) => (V::NULL, Some(n.to_owned())),
                None => {
                    let p = rt.get_field(b, "$properties");
                    (p, rt.str_of(rt.get_field(p, "$copy")).map(str::to_owned))
                }
            };
            let d = api::call(rt, "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;", &[b]);
            let default_state = rt.str_of(d).unwrap_or("").to_owned();
            let own = if props.r() != 0 { map_color(rt, rt.get_field(props, "$mapColor"), &default_state) } else { None };
            let id = own.or_else(|| vanilla.as_deref().and_then(|c| COPIED.iter().find(|(n, ..)| *n == c)).map(|c| c.4)).unwrap_or(0) as usize;
            Some(map_color_constant(rt, id))
        }
        // A block-state enum's name in states (`SlabType.DOUBLE` is `double`); the property enums
        // also give it as `toString`.
        (o, "getSerializedName" | "toString") if desc == "()Ljava/lang/String;" && enum_class(o) && (name == "getSerializedName" || o.starts_with("net/minecraft/world/level/block/state/properties/")) => {
            let text = vanilla_enum(o)?.get(rt.get_field(a(0), "$ordinal").i().max(0) as usize)?;
            Some(rt.string(text))
        }
        // `ArmorMaterial.Layer(assetName[, suffix, dyeable])`, and its texture's location as
        // vanilla's (`ns:textures/models/armor/<path>_layer_<1|2><suffix>.png`).
        (ARMOR_LAYER, "<init>") => {
            let full = desc.starts_with("(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/String;Z)");
            rt.put_field(a(0), "assetName", a(1));
            let suffix = if full { a(2) } else { rt.string("") };
            rt.put_field(a(0), "suffix", suffix);
            rt.put_field(a(0), "dyeable", if full { a(3) } else { V::I(0) });
            Some(V::Void)
        }
        (ARMOR_LAYER, "texture") => {
            let id = crate::neo::location(rt, rt.get_field(a(0), "assetName"));
            let suffix = rt.str_of(rt.get_field(a(0), "suffix")).unwrap_or("").to_owned();
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            let text = format!("{ns}:textures/models/armor/{path}_layer_{}{suffix}.png", if a(1).i() != 0 { 2 } else { 1 });
            Some(rt.string(&text))
        }
        (ARMOR_LAYER, "dyeable") => Some(V::I((rt.get_field(a(0), "dyeable").i() != 0) as i32)),
        // A vanilla material's values (Mekanism copies netherite's); a mod's own is its record.
        (ARMOR_MATERIAL, _) if rt.str_of(a(0)).is_some() => vanilla_armor_material(rt, name, a(0)),
        // `ArmorItem.Type`: its name and vanilla's durability per material multiplier.
        ("net/minecraft/world/item/ArmorItem$Type", "getName" | "getSerializedName") => {
            let n = rt.str_of(rt.get_field(a(0), "$name")).unwrap_or("").to_ascii_lowercase();
            Some(rt.string(&n))
        }
        (ARMOR_TYPE, "getSlot") => armor_slot(rt, a(0)),
        // `ArmorItem`'s own (a mod's armour, Create's cardboard armour): the slot its type names,
        // which is what makes it `Equipable`; an `Equipable` that isn't one answers itself.
        (ARMOR_ITEM | EQUIPABLE, "getEquipmentSlot") => {
            let ty = rt.get_field(a(0), "type");
            armor_slot(rt, ty)
        }
        (ARMOR_ITEM, "getType") => Some(rt.get_field(a(0), "type")),
        (ARMOR_ITEM, "getMaterial") => Some(rt.get_field(a(0), "material")),
        ("net/minecraft/world/item/ArmorItem$Type", "getDurability") => {
            const HEALTH: [i32; 5] = [11, 16, 15, 13, 16];
            let i = rt.get_field(a(0), "$ordinal").i().clamp(0, 4) as usize;
            Some(V::I(HEALTH[i] * a(1).i()))
        }
        (BLOCK_PROPERTIES, _) => block_properties(rt, name, desc, args),
        (ITEM_PROPERTIES, _) => item_properties(rt, name, args),
        (TIERS | SIMPLE_TIER | "net/minecraft/world/item/Tier", _) => tier_method(rt, owner, name, desc, args),
        // `SwordItem.createAttributes(tier, damage, speed)` (and `DiggerItem`'s, the damage a float,
        // which `AxeItem.createAttributes` and the other tools' name): the main hand's attack damage
        // and speed modifiers, the tier's bonus added to the damage.
        ("net/minecraft/world/item/SwordItem" | "net/minecraft/world/item/DiggerItem" | "net/minecraft/world/item/AxeItem" | "net/minecraft/world/item/HoeItem" | "net/minecraft/world/item/PickaxeItem" | "net/minecraft/world/item/ShovelItem", "createAttributes") => {
            let bonus = rt.call_functional("net/minecraft/world/item/Tier.getAttackDamageBonus.()F", a(0), &[]);
            let bonus = if let V::F(f) = bonus { f } else { 0.0 };
            let damage = match a(1) {
                V::I(i) => i as f32,
                V::F(f) => f,
                _ => 0.0,
            };
            Some(crate::attributes::weapon(rt, f64::from(damage + bonus), f64::from(a(2).f())))
        }
        (WOOD_TYPE, "name") => Some(rt.get_field(a(0), "$name")),
        // `SignBlock.getWoodType(block)`: a sign's own wood, else oak; `sign.type()`.
        ("net/minecraft/world/level/block/SignBlock", "getWoodType") => {
            let w = rt.get_field(a(0), "type");
            Some(if w.r() != 0 { w } else { api::get_static(rt, "net/minecraft/world/level/block/state/properties/WoodType.OAK") })
        }
        (o, "type") if o.ends_with("SignBlock") && o.starts_with("net/minecraft/world/level/block/") && desc == "()Lnet/minecraft/world/level/block/state/properties/WoodType;" => {
            Some(rt.get_field(a(0), "type"))
        }
        // `BedBlock.getConnectedDirection(state)`: where the other half is, from the foot its
        // facing, from the head the opposite.
        ("net/minecraft/world/level/block/BedBlock", "getConnectedDirection") => {
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            let (_, props) = crate::host::split_state(&text);
            let facing = props.iter().find(|(k, _)| *k == "facing").map_or("north", |(_, v)| *v);
            let head = props.iter().any(|(k, v)| *k == "part" && *v == "head");
            let o = crate::api_common::DIRECTION_NAMES.iter().position(|d| *d == facing).unwrap_or(2) as i64;
            Some(api::direction(rt, if head { o ^ 1 } else { o }))
        }
        // `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection(state)` (a lever's, a
        // button's): the side it hangs from, down from a ceiling, up from a floor, else its facing.
        ("net/minecraft/world/level/block/FaceAttachedHorizontalDirectionalBlock", "getConnectedDirection") => {
            let text = rt.str_of(a(0)).unwrap_or("").to_owned();
            let (_, props) = crate::host::split_state(&text);
            let prop = |key: &str| props.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
            let o = match prop("face") {
                Some("ceiling") => 0,
                Some("floor") => 1,
                _ => crate::api_common::DIRECTION_NAMES.iter().position(|d| Some(*d) == prop("facing")).unwrap_or(2) as i64,
            };
            Some(api::direction(rt, o))
        }
        (BRUSHABLE, "getTurnsInto" | "getBrushSound" | "getBrushCompletedSound") => {
            let field = match name {
                "getTurnsInto" => "turnsInto",
                "getBrushSound" => "brushSound",
                _ => "brushCompletedSound",
            };
            Some(match rt.get_field(a(0), field) {
                V::Void => V::NULL,
                v => v,
            })
        }
        // The builder's `properties` (an accessor's `$properties`): its properties by name, as
        // vanilla's `Builder` keeps them.
        (BUILDER, "$properties" | "properties") if desc.starts_with("()") => {
            let list = rt.get_field(a(0), "$props");
            let map = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
            for p in jutil::elements(rt, list) {
                let (pname, _) = api_common::prop_parts(rt, p);
                let key = rt.string(&pname);
                jutil::table_put(rt, map, key, p);
            }
            Some(map)
        }
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
            let block = api::block_name(rt, a(0));
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
            // A sign's or hanging sign's wood (`StandingSignBlock(WoodType, Properties)`), the field
            // vanilla names `type`.
            if let Some(w) = args.iter().skip(1).copied().find(|v| v.r() != 0 && rt.class_of(*v) == WOOD_TYPE) {
                rt.put_field(a(0), "type", w);
            }
            // A door's, trapdoor's, button's or plate's set (`DoorBlock(BlockSetType, Properties)`),
            // also `type`, and a weighted plate's `maxWeight` (`(int, BlockSetType, Properties)`):
            // what its template follows.
            if let Some(t) = args.iter().skip(1).copied().find(|v| v.r() != 0 && rt.class_of(*v) == BLOCK_SET_TYPE) {
                rt.put_field(a(0), "type", t);
            }
            if o == "net/minecraft/world/level/block/WeightedPressurePlateBlock" && desc.starts_with("(I") {
                rt.put_field(a(0), "maxWeight", a(1));
            }
            // A rail's kind: `isStraight` (`BaseRailBlock(isStraight, properties)`), a powered
            // rail's `isActivator`.
            crate::rails::constructed(rt, a(0), o, desc, args);
            // `BrushableBlock(turnsInto, brushSound, brushCompletedSound, properties)`: vanilla's fields.
            if o == BRUSHABLE && desc.starts_with("(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundEvent;") {
                for (i, field) in ["turnsInto", "brushSound", "brushCompletedSound"].into_iter().enumerate() {
                    rt.put_field(a(0), field, a(1 + i));
                }
            }
            // An ore's experience (`DropExperienceBlock(IntProvider, Properties)`), the field vanilla
            // names `xpRange`: what NeoForge's `getExpDrop` draws from.
            if o == "net/minecraft/world/level/block/DropExperienceBlock" && desc.starts_with("(Lnet/minecraft/util/valueproviders/IntProvider;") {
                rt.put_field(a(0), "xpRange", a(1));
            }
            // A cauldron's interaction map (`AbstractCauldronBlock(Properties, InteractionMap)`),
            // the field vanilla names `interactions`.
            if let Some(m) = args.iter().skip(1).copied().find(|v| v.r() != 0 && rt.class_of(*v) == INTERACTION_MAP) {
                rt.put_field(a(0), "interactions", m);
            }
            constructed(rt, a(0), o, props);
            Some(V::Void)
        }
        (o, "<init>") if (o.starts_with("net/minecraft/world/item/") || o == DEFERRED_SPAWN_EGG) && desc.contains("Lnet/minecraft/world/item/Item$Properties;") => {
            if let Some(p) = args.iter().skip(1).copied().find(|v| v.r() != 0 && rt.class_of(*v) == ITEM_PROPERTIES) {
                rt.put_field(a(0), "$properties", p);
            }
            // NeoForge's `DeferredSpawnEggItem(type, background, highlight, properties)`: the entity
            // type's supplier and the egg's two colours, under vanilla's `SpawnEggItem` names.
            if o == DEFERRED_SPAWN_EGG && desc.starts_with("(Ljava/util/function/Supplier;II") {
                rt.put_field(a(0), "typeSupplier", a(1));
                rt.put_field(a(0), "backgroundColor", a(2));
                rt.put_field(a(0), "highlightColor", a(3));
            }
            // `BlockItem(Block, Properties)` and its kinds: the block it places (`Block.asItem()` finds it).
            if desc.starts_with("(Lnet/minecraft/world/level/block/Block;") {
                rt.put_field(a(0), "$block", a(1));
            }
            // `ArmorItem(material, type, properties)`: vanilla's fields `material` and `type`.
            if o == ARMOR_ITEM && desc.starts_with("(Lnet/minecraft/core/Holder;Lnet/minecraft/world/item/ArmorItem$Type;") {
                rt.put_field(a(0), "material", a(1));
                rt.put_field(a(0), "type", a(2));
            }
            // `BucketItem(Fluid, Properties)`: the fluid it holds, vanilla's field `content`.
            if o == "net/minecraft/world/item/BucketItem" {
                rt.put_field(a(0), "content", a(1));
            }
            // `DyeItem(DyeColor, Properties)`: the colour its `getDyeColor` gives back.
            if o == "net/minecraft/world/item/DyeItem"
                && desc.starts_with("(Lnet/minecraft/world/item/DyeColor;")
            {
                rt.put_field(a(0), "$dyeColor", a(1));
            }
            Some(V::Void)
        }
        _ => None,
    }
}

/// Static fields this module owns: `SoundType`, `MapColor`, `DyeColor`, `Rarity` constants.
/// Vanilla blocks' shapes kept in static fields mods read (`ChestBlock.AABB`), by class and field:
/// boxes in pixels, as `Block.box` takes them, several for a shape vanilla joins (`Shapes.or`).
const STATIC_SHAPES: [(&str, &str, &[[f64; 6]]); 9] = [
    ("ChestBlock", "AABB", &[[1.0, 0.0, 1.0, 15.0, 14.0, 15.0]]),
    ("ChestBlock", "NORTH_AABB", &[[1.0, 0.0, 0.0, 15.0, 14.0, 15.0]]),
    ("ChestBlock", "SOUTH_AABB", &[[1.0, 0.0, 1.0, 15.0, 14.0, 16.0]]),
    ("ChestBlock", "WEST_AABB", &[[0.0, 0.0, 1.0, 15.0, 14.0, 15.0]]),
    ("ChestBlock", "EAST_AABB", &[[1.0, 0.0, 1.0, 16.0, 14.0, 15.0]]),
    ("WallHangingSignBlock", "PLANK_NORTHSOUTH", &[[0.0, 14.0, 6.0, 16.0, 16.0, 10.0]]),
    ("WallHangingSignBlock", "PLANK_EASTWEST", &[[6.0, 14.0, 0.0, 10.0, 16.0, 16.0]]),
    ("WallHangingSignBlock", "SHAPE_NORTHSOUTH", &[[0.0, 14.0, 6.0, 16.0, 16.0, 10.0], [1.0, 0.0, 7.0, 15.0, 10.0, 9.0]]),
    ("WallHangingSignBlock", "SHAPE_EASTWEST", &[[6.0, 14.0, 0.0, 10.0, 16.0, 16.0], [7.0, 0.0, 1.0, 9.0, 10.0, 15.0]]),
];

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if let Some(class) = owner.strip_prefix("net/minecraft/world/level/block/")
        && let Some((.., pixels)) = STATIC_SHAPES.iter().find(|(c, f, _)| *c == class && *f == field)
    {
        let boxes: Vec<crate::shapes::Box3> = pixels.iter().map(|b| b.map(|v| v / 16.0)).collect();
        return Some(rt.intern(format!("const/{owner}.{field}"), move |rt| {
            let joined = crate::shapes::optimized(&boxes);
            crate::shapes::shape(rt, &joined)
        }));
    }
    match owner {
        SOUND_TYPE => {
            let &(f, ..) = VANILLA_SOUND_TYPES.iter().find(|(f, ..)| *f == field)?;
            Some(named(rt, SOUND_TYPE, f))
        }
        BLOCK_SET_TYPE => {
            block_set_types(rt);
            rt.interned.get(&format!("const/{BLOCK_SET_TYPE}.{field}")).copied()
        }
        DISPENSER if field == "DISPENSER_REGISTRY" => Some(dispenser_behaviours(rt)),
        // Vanilla's behaviour that puts armour on whoever stands in front (`ArmorItem.dispenseArmor`):
        // one object, its class vanilla's anonymous one, so the game can tell it from a mod's.
        "net/minecraft/world/item/ArmorItem" if field == "DISPENSE_ITEM_BEHAVIOR" => {
            Some(rt.intern("compat:dispense-armor".into(), |rt| rt.new_object("net/minecraft/world/item/ArmorItem$1")))
        }
        TIERS => {
            let i = TIER_VALUES.iter().position(|(n, ..)| *n == field)?;
            Some(api::api_enum(rt, TIERS, TIER_VALUES[i].0, i as i32))
        }
        // Vanilla's armour materials, as the registry's holders of them.
        "net/minecraft/world/item/ArmorMaterials" if matches!(field, "LEATHER" | "CHAIN" | "IRON" | "GOLD" | "DIAMOND" | "TURTLE" | "NETHERITE" | "ARMADILLO") => {
            // `CHAIN` is registered as `chainmail`.
            let id = format!("minecraft:{}", if field == "CHAIN" { "chainmail".to_owned() } else { field.to_ascii_lowercase() });
            Some(rt.intern(format!("compat:armor-material/{id}"), |rt| {
                let m = rt.alloc("net/minecraft/world/item/ArmorMaterial", Data::Str(id.clone()));
                crate::neo::registry_holder(rt, "armor_material", &id, m)
            }))
        }
        CAULDRON_INTERACTION => match field {
            "EMPTY" | "WATER" | "LAVA" | "POWDER_SNOW" => Some(interaction_map(rt, &field.to_ascii_lowercase(), false)),
            "INTERACTIONS" => Some(cauldron_maps(rt)),
            // Vanilla's own interactions (`DYED_ITEM`...), which mods file under their items: each is
            // the lambda vanilla's static initializer makes it from, in that order.
            _ => {
                let i = VANILLA_INTERACTIONS.iter().position(|f| *f == field)?;
                Some(rt.intern(format!("compat:cauldron-interaction/{field}"), move |rt| {
                    crate::functions::method(rt, &format!("{CAULDRON_INTERACTION}.lambda$static${i}.{}", interact_desc()))
                }))
            }
        },
        // Every colour at its id, as `MapColor`'s constructor files them (64 slots).
        MAP_COLOR if field == "MATERIAL_COLORS" => Some(rt.intern(format!("const/{MAP_COLOR}.MATERIAL_COLORS"), |rt| {
            let mut colors = vec![V::NULL; 64];
            for (id, slot) in colors.iter_mut().enumerate().take(MAP_COLORS.len()) {
                *slot = map_color_constant(rt, id);
            }
            rt.alloc("[Lnet/minecraft/world/level/material/MapColor;", crate::Data::Array(colors))
        })),
        MAP_COLOR => {
            let id = MAP_COLORS.iter().position(|c| *c == field)?;
            Some(map_color_constant(rt, id))
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
            vanilla_enum_constant(rt, o, &enum_text(o, field))
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
        // The fields themselves (`properties.destroyTime`, opened by an access transformer).
        f if f.starts_with('$') => properties_field(rt, this, &f[1..]),
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

/// The `BlockBehaviour.Properties` field `name` (1.21.1's) as the builder calls left it: what they
/// stored, else what the vanilla block a copy came from has, else `Properties.of()`'s value.
fn properties_field(rt: &mut Rt, p: V, name: &str) -> Option<V> {
    let got = |rt: &Rt, k: &'static str| match rt.get_field(p, k) {
        V::Void | V::NULL => None,
        v => Some(v),
    };
    let flag = |rt: &Rt, k: &'static str| V::I(got(rt, k).map_or(0, |v| v.i()));
    let unless = |rt: &Rt, k: &'static str| V::I((got(rt, k).map_or(0, |v| v.i()) == 0) as i32);
    let copy = rt.str_of(rt.get_field(p, "$copy")).map(str::to_owned);
    let copied = copy.as_deref().and_then(|c| COPIED.iter().find(|(n, ..)| *n == c));
    Some(match name {
        "destroyTime" => got(rt, "$destroy").unwrap_or_else(|| {
            let game = copy.as_deref().and_then(|c| rt.host().and_then(|h| h.block_facts(c))).map(|f| f.hardness);
            V::F(game.or(copied.map(|c| c.1)).unwrap_or(0.0))
        }),
        "explosionResistance" => got(rt, "$resist").unwrap_or(V::F(copied.map_or(0.0, |c| c.2))),
        "requiresCorrectToolForDrops" => match got(rt, "$requiresTool") {
            Some(v) => v,
            None => V::I(copied.is_some_and(|c| c.3) as i32),
        },
        "friction" => got(rt, "$friction").unwrap_or(V::F(0.6)),
        "speedFactor" => got(rt, "$speedFactor").unwrap_or(V::F(1.0)),
        "jumpFactor" => got(rt, "$jumpFactor").unwrap_or(V::F(1.0)),
        "hasCollision" => unless(rt, "$noCollision"),
        "canOcclude" => unless(rt, "$noOcclusion"),
        "spawnTerrainParticles" => unless(rt, "$noTerrainParticles"),
        "isRandomlyTicking" => flag(rt, "$randomTicks"),
        "replaceable" => flag(rt, "$replaceable"),
        "dynamicShape" => flag(rt, "$dynamicShape"),
        "ignitedByLava" => flag(rt, "$ignitedByLava"),
        "liquid" => flag(rt, "$liquid"),
        "forceSolidOn" => flag(rt, "$forceSolidOn"),
        "forceSolidOff" => flag(rt, "$forceSolidOff"),
        "isAir" => flag(rt, "$air"),
        "soundType" => match got(rt, "$sound") {
            Some(v) => v,
            None => api::get_static(rt, "net/minecraft/world/level/block/SoundType.STONE"),
        },
        "pushReaction" => match got(rt, "$push") {
            Some(v) => v,
            None => push_reaction_named(rt, "normal"),
        },
        // What `lightLevel` set, else `state -> 0`.
        "lightEmission" => match got(rt, "$light") {
            Some(v) => v,
            None => crate::functions::constant(rt, V::I(0)),
        },
        _ => return None,
    })
}

/// `Item.Properties`: stack size, durability, rarity, food, what crafting with it leaves.
fn item_properties(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let field = match name {
        "<init>" => return Some(V::Void),
        "stacksTo" => "$stacksTo",
        "durability" => "$durability",
        "craftRemainder" => "$craftRemainder",
        "rarity" => "$rarity",
        "food" => "$food",
        "attributes" => "$attributes",
        "jukeboxPlayable" => "$jukeboxPlayable",
        "requiredFeatures" => "$requiredFeatures",
        "fireResistant" | "setNoRepair" => {
            let f = if name == "fireResistant" { "$fireResistant" } else { "$noRepair" };
            rt.put_field(this, f, V::I(1));
            return Some(this);
        }
        // `component(type, value)`: the item's default components, by type.
        "component" => {
            let mut m = rt.get_field(this, "$components");
            if m.r() == 0 {
                m = rt.alloc("java/util/LinkedHashMap", crate::Data::Table(Default::default()));
                rt.put_field(this, "$components", m);
            }
            crate::jutil::table_put(rt, m, a(1), a(2));
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
            .then(|| field.as_deref().and_then(|f| vanilla_default(vanilla, f)))
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

/// An enum's constants, in order (`values()`): vanilla's (`EnumProperty.create("shape",
/// RailShape.class, RailShape::isAscending)` filters them) or a mod's.
fn enum_constants(rt: &mut Rt, class: &str) -> Vec<V> {
    if let Some(values) = vanilla_enum_values(rt, class) {
        return values;
    }
    match rt.reflect(class, "values", &[]) {
        Some(arr) if arr.r() != 0 => (0..rt.array_len(arr)).map(|i| rt.array_get(arr, i)).collect(),
        _ => Vec::new(),
    }
}

const POSSIBLE_VALUES: &str = "getPossibleValues.()Ljava/util/Collection;";
const POSSIBLE_VALUES_KEY: &str = "net/minecraft/world/level/block/state/properties/Property.getPossibleValues.()Ljava/util/Collection;";
const VALUE_NAME: &str = "getName.(Ljava/lang/Comparable;)Ljava/lang/String;";
const VALUE_NAME_KEY: &str = "net/minecraft/world/level/block/state/properties/Property.getName.(Ljava/lang/Comparable;)Ljava/lang/String;";
const PARSE_VALUE: &str = "getValue.(Ljava/lang/String;)Ljava/util/Optional;";
const PARSE_VALUE_KEY: &str = "net/minecraft/world/level/block/state/properties/Property.getValue.(Ljava/lang/String;)Ljava/util/Optional;";

/// `p.<short>(args)` when `p` is a mod's own property class that has that method itself.
fn own_property_call(rt: &mut Rt, p: V, key: &'static str, short: &'static str, args: &[V]) -> Option<V> {
    let class = rt.class_of(p);
    if !rt.is_mod_class(class) || !rt.overrides.is_some_and(|o| o(class, short)) {
        return None;
    }
    let vc = rt.vcall?;
    let v = vc(rt, key, short, args);
    if rt.thrown.is_some() {
        rt.report_thrown(short);
        return None;
    }
    Some(v)
}

/// Value `v` of property `p` as a state spells it: by the property's own `getName` when a mod's
/// class has one (`IntegerSetProperty`), else as vanilla's kinds do.
pub fn property_text(rt: &mut Rt, p: V, v: V) -> String {
    if let Some(s) = own_property_call(rt, p, VALUE_NAME_KEY, VALUE_NAME, &[p, v]) {
        return rt.str_of(s).unwrap_or("").to_owned();
    }
    match v {
        V::R(_) => api_common::text_of(rt, v),
        // A value a method reference gave back unboxed.
        raw if api_common::prop_parts(rt, p).1 == "bool" => (raw.i() != 0).to_string(),
        raw => raw.i().to_string(),
    }
}

/// The value of property `p` that a state spelling `text` holds, by the property's own `getValue`
/// when a mod's class has one; None to read it as vanilla's kinds do.
pub fn parsed_value(rt: &mut Rt, p: V, text: &str) -> Option<V> {
    let s = rt.string(text);
    let o = own_property_call(rt, p, PARSE_VALUE_KEY, PARSE_VALUE, &[p, s])?;
    match rt.get_field(o, "value") {
        V::Void => Some(V::NULL),
        v => Some(v),
    }
}

/// A property's values in Java's order, as they read in a state.
pub fn property_values(rt: &mut Rt, p: V) -> Vec<String> {
    let listed = rt.get_field(p, "$values");
    if listed.r() != 0 {
        return jutil::elements(rt, listed).iter().map(|v| rt.str_of(*v).unwrap_or("").to_owned()).collect();
    }
    // A mod's own property class: the values its `getPossibleValues` gives, as its `getName`
    // spells them; kept, as a property's values are fixed once it is made.
    if let Some(values) = own_property_call(rt, p, POSSIBLE_VALUES_KEY, POSSIBLE_VALUES, &[p]) {
        let texts: Vec<String> = jutil::elements(rt, values).into_iter().map(|v| property_text(rt, p, v)).collect();
        if !texts.is_empty() {
            let strings: Vec<V> = texts.iter().map(|t| rt.string(t)).collect();
            let l = jutil::new_list(rt, strings);
            rt.put_field(p, "$values", l);
        }
        return texts;
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

    // The template: a vanilla block of the same kind (a door; weathering copper stairs are
    // stairs), else one without states that lets light through when the block isn't a full cube or
    // doesn't occlude, else stone.
    let kind =
        templated_class(&vanilla).unwrap_or_else(|| vanilla.rsplit('/').next().unwrap_or(""));
    let class = rt.class_of(b);
    let mut vanilla_template = template_of(rt, b, kind).map(str::to_owned);
    let mut template_props: Vec<String> = match vanilla_template {
        Some(_) => {
            let own = vanilla_definition(&vanilla);
            let definition = if own.is_empty() {
                vanilla_definition(kind)
            } else {
                own
            };
            definition
                .iter()
                .filter_map(|f| api_common::vanilla_property(f).map(|(n, _)| n))
                .collect()
        }
        None => Vec::new(),
    };
    // The game orders a block's states by property name, its template's first: a property of its
    // own that sorts before one of the template's can't be added to it, so such a block (Create's
    // glass pipe: `alt` beside a pillar's `axis`) takes a template without states and owns them all.
    if states.iter().any(|(n, _)| !template_props.contains(n) && template_props.iter().any(|t| n < t)) {
        vanilla_template = None;
        template_props.clear();
    }
    // A shape of its own (`getShape`), or the one the vanilla class it extends draws (a lectern's,
    // a diode's), read for every state; a vanilla template keeps its own.
    let own_shape = rt.overrides.is_some_and(|o| o(class, GET_SHAPE));
    let inherited = !vanilla.is_empty() && !default_shape_class(&vanilla);
    let read = own_shape || (inherited && shape_class(&vanilla).is_some());
    let shapes = if vanilla_template.is_none() && read {
        state_shapes(rt, b, name, &states)
    } else {
        None
    };
    let template = match vanilla_template {
        Some(t) => t,
        None => {
            let full = match &shapes {
                Some(s) => s.iter().all(|(_, boxes)| boxes[..] == [FULL_BOX]),
                // Unread: a full cube unless its class or a vanilla one above it draws another.
                None => !own_shape && !inherited,
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
    // The vanilla block its properties copy (`ofFullCopy(Blocks.TALL_GRASS)`): the game carries
    // over what it knows of that block (its hardness, tool, map colour, occlusion, collision...)
    // under the values above.
    if let Some(c) = copy.as_deref().filter(|c| c.starts_with("minecraft:")) {
        p.push(format!("\"copy\": \"{c}\""));
    }
    if props.r() != 0 && rt.get_field(props, "$noCollision").i() != 0 {
        p.push("\"collision\": false".into());
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
    // A class that doesn't draw its own shape draws the one of the vanilla class it extends.
    let owner = match rt.str_of(rt.get_field(b, "$vanilla")).and_then(shape_class) {
        Some(c) => {
            let key = format!("net/minecraft/world/level/block/{c}.{GET_SHAPE}");
            rt.static_key(&key)
        }
        None => GET_SHAPE_KEY,
    };
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
        let shape = vc(rt, owner, GET_SHAPE, &[b, st, V::NULL, origin, ctx]);
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
/// An `Equipable` item (Create's goggles; an `ArmorItem`) as 26.3 has it: an `equippable`
/// component with the slot its `getEquipmentSlot` names, so the game puts it on by use, in the
/// armour slots and from dispensers. None for other items, or a slot that isn't one.
fn equippable(rt: &mut Rt, item: V) -> Option<String> {
    if !rt.instance_of(item, "net/minecraft/world/item/Equipable") {
        return None;
    }
    let slot = rt.call_functional("net/minecraft/world/item/Equipable.getEquipmentSlot.()Lnet/minecraft/world/entity/EquipmentSlot;", item, &[]);
    rt.thrown = None;
    if slot.r() == 0 || matches!(slot, V::Void) {
        return None;
    }
    let name = rt.str_of(rt.get_field(slot, "$name")).unwrap_or("").to_ascii_lowercase();
    ["head", "chest", "legs", "feet", "body", "mainhand", "offhand"]
        .contains(&name.as_str())
        .then(|| format!("\"minecraft:equippable\": {{\"slot\": \"{name}\"}}"))
}

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
            components.extend(equippable(rt, i));
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

    /// A mod's sign made with a wood type (SecurityCraft's secret signs) answers it through
    /// `SignBlock.getWoodType`, any other block oak, as vanilla's does. A bed's other half: from the
    /// foot where it faces, from the head the opposite way (Comforts' sleeping bags).
    #[test]
    fn signs_keep_their_wood_and_beds_find_their_other_half() {
        let mut rt = Rt::new();
        let spruce = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/WoodType.SPRUCE");
        let oak = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/WoodType.OAK");
        let sign = rt.new_object("test/SecretSign");
        let p = props(&mut rt);
        api::call(&mut rt, "net/minecraft/world/level/block/StandingSignBlock.<init>.(Lnet/minecraft/world/level/block/state/properties/WoodType;Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[sign, spruce, p]);
        let wood_of = "net/minecraft/world/level/block/SignBlock.getWoodType.(Lnet/minecraft/world/level/block/Block;)Lnet/minecraft/world/level/block/state/properties/WoodType;";
        assert_eq!(api::call(&mut rt, wood_of, &[sign]), spruce);
        let plain = rt.new_object("test/Plain");
        assert_eq!(api::call(&mut rt, wood_of, &[plain]), oak);
        let connected = "net/minecraft/world/level/block/BedBlock.getConnectedDirection.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/core/Direction;";
        let foot = api::state(&mut rt, "comforts:sleeping_bag_red[facing=east,occupied=false,part=foot]");
        let head = api::state(&mut rt, "comforts:sleeping_bag_red[facing=east,occupied=false,part=head]");
        let (east, west) = (api::direction(&mut rt, 5), api::direction(&mut rt, 4));
        assert_eq!(api::call(&mut rt, connected, &[foot]), east);
        assert_eq!(api::call(&mut rt, connected, &[head]), west);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a mod block reads of vanilla's while it is made (Mekanism's machines): `super.getShape`
    /// to a class with vanilla's default is a full block, and to one with shapes of its own
    /// (`SlabBlock`) its state's; a state's block holder holds the block, by its name; the
    /// properties' `lightEmission` field is what `lightLevel` set, else `state -> 0`.
    #[test]
    fn a_mod_blocks_reads_of_vanillas_answer_as_vanilla() {
        let mut rt = Rt::new();
        crate::install();
        let get_shape = |owner: &str| {
            Box::leak(format!("net/minecraft/world/level/block/{owner}.getShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;").into_boxed_str()) as &'static str
        };
        let machine = rt.new_object("test/Machine");
        let st = api::state(&mut rt, "minecraft:stone");
        for owner in ["Block", "BaseEntityBlock", "HorizontalDirectionalBlock", "state/BlockBehaviour"] {
            let shape = api::call(&mut rt, get_shape(owner), &[machine, st, V::NULL, V::NULL, V::NULL]);
            assert_eq!(crate::shapes::boxes(&rt, shape), Some(vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]), "{owner}");
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // A slab's shape is its own: the half its state says.
        let top = api::state(&mut rt, "ruby:slab[type=top,waterlogged=false]");
        let shape = api::call(&mut rt, get_shape("SlabBlock"), &[machine, top, V::NULL, V::NULL, V::NULL]);
        assert_eq!(crate::shapes::boxes(&rt, shape), Some(vec![[0.0, 0.5, 0.0, 1.0, 1.0, 1.0]]));

        let holder = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.getBlockHolder.()Lnet/minecraft/core/Holder;", &[st]);
        let held = api::call(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]);
        let stone = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.getBlock.()Lnet/minecraft/world/level/block/Block;", &[st]);
        assert_eq!(held, stone);
        let name = api::call(&mut rt, "net/minecraft/core/Holder.getRegisteredName.()Ljava/lang/String;", &[holder]);
        assert_eq!(rt.str_of(name), Some("minecraft:stone"));

        let light = "net/minecraft/world/level/block/state/BlockBehaviour$Properties.$lightEmission.()Ljava/util/function/ToIntFunction;";
        let to_int = "java/util/function/ToIntFunction.applyAsInt.(Ljava/lang/Object;)I";
        let p = props(&mut rt);
        let f = crate::api::game_field(&mut rt, p, "lightEmission", "$lightEmission", light);
        assert_eq!(api::call(&mut rt, to_int, &[f, st]), V::I(0), "Properties.of()'s state -> 0");
        let lit = rt.new_object("test/Lit");
        api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.lightLevel.(Ljava/util/function/ToIntFunction;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[p, lit]);
        assert_eq!(crate::api::game_field(&mut rt, p, "lightEmission", "$lightEmission", light), lit);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);

        // SecurityCraft reads `destroyTime` off the properties it copied; mods read
        // `hasCollision` and `friction` off their own.
        let field = |rt: &mut Rt, p: V, f: &'static str, ty: &str| {
            let key = Box::leak(format!("net/minecraft/world/level/block/state/BlockBehaviour$Properties.${f}.(){ty}").into_boxed_str());
            let own = Box::leak(format!("${f}").into_boxed_str());
            crate::api::game_field(rt, p, f, own, key)
        };
        let stone = rt.string("minecraft:stone");
        let copy = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.ofFullCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[stone]);
        assert_eq!(field(&mut rt, copy, "destroyTime", "F"), V::F(1.5), "stone's");
        assert_eq!(field(&mut rt, copy, "explosionResistance", "F"), V::F(6.0));
        api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.destroyTime.(F)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[copy, V::F(-1.0)]);
        assert_eq!(field(&mut rt, copy, "destroyTime", "F"), V::F(-1.0), "what the mod set over it");
        let p = props(&mut rt);
        assert_eq!(field(&mut rt, p, "destroyTime", "F"), V::F(0.0));
        assert_eq!(field(&mut rt, p, "friction", "F"), V::F(0.6));
        assert_eq!(field(&mut rt, p, "hasCollision", "Z"), V::I(1));
        api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.noCollission.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[p]);
        assert_eq!(field(&mut rt, p, "hasCollision", "Z"), V::I(0));
        assert_eq!(field(&mut rt, p, "canOcclude", "Z"), V::I(0), "no collision takes occlusion with it");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
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
        // Mekanism looks colours up by id in `MATERIAL_COLORS`: orange is 15, the same constant.
        let all = api::get_static(&mut rt, "net/minecraft/world/level/material/MapColor.MATERIAL_COLORS");
        let ids = crate::jutil::elements(&rt, all);
        assert_eq!((ids.len(), ids[15], ids[63]), (64, color, V::NULL));
        // Its fields as a mod reads them: the id, and the colour it was built with.
        assert_eq!((rt.get_field(color, "id"), rt.get_field(color, "col")), (V::I(15), V::I(14188339)));
        assert_eq!(rt.get_field(ids[1], "col"), V::I(8368696), "GRASS");
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
        for part in ["\"template\": \"minecraft:stone\"", "\"item\": false", "\"hardness\": 1.5", "\"sounds\": \"minecraft:andesite\"", "\"map_color\": 11", "\"copy\": \"minecraft:andesite\""] {
            assert!(json.contains(part), "{part} in {json}");
        }
        assert!(!json.contains("\"collision\""), "{json}");
        // Farmer's Delight's wild rice: `ofFullCopy(Blocks.TALL_GRASS)`, a block the table doesn't
        // know: the copy is named for the game to carry over (no collision, light through it).
        // Another plant says `noCollission()` itself.
        let copy = "net/minecraft/world/level/block/state/BlockBehaviour$Properties.ofFullCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;";
        let grass = api::get_static(&mut rt, "net/minecraft/world/level/block/Blocks.TALL_GRASS");
        let p = api::call(&mut rt, copy, &[grass]);
        let rice = rt.new_object("net/minecraft/world/level/block/Block");
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[rice, p]);
        let p = props(&mut rt);
        api::call(&mut rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.noCollission.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[p]);
        let reed = rt.new_object("net/minecraft/world/level/block/Block");
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[reed, p]);
        rt.registered.clear();
        rt.registered.insert(("block".into(), "ruby:rice".into()), rice);
        rt.registered.insert(("block".into(), "ruby:reed".into()), reed);
        let declared: std::collections::HashMap<String, String> = block_declarations(&mut rt).into_iter().collect();
        let rice = &declared["ruby:rice"];
        assert!(rice.contains("\"copy\": \"minecraft:tall_grass\"") && !rice.contains("\"collision\""), "{rice}");
        let reed = &declared["ruby:reed"];
        assert!(reed.contains("\"collision\": false") && !reed.contains("\"copy\""), "{reed}");
        assert!(reed.contains("\"template\": \"minecraft:glass\""), "{reed}");
    }

    #[test]
    fn a_pillar_declares_its_axis_and_the_default_its_constructor_sets() {
        // A mod block extending `RotatedPillarBlock` without its own definition: vanilla's adds the
        // axis; the template is a pillar, so the axis is the template's.
        let mut rt = Rt::new();
        let p = props(&mut rt);
        let b = rt.new_object("net/minecraft/world/level/block/RotatedPillarBlock");
        api::call(&mut rt, "net/minecraft/world/level/block/RotatedPillarBlock.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[b, p]);
        // As vanilla's `RotatedPillarBlock` constructor leaves it: along y; not registered yet, the
        // block's state all the same.
        let d = api::call(&mut rt, "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;", &[b]);
        assert!(rt.str_of(d).is_some_and(|t| t.ends_with("[axis=y]")), "{:?}", rt.str_of(d));
        assert_eq!(api::call(&mut rt, GET_BLOCK, &[d]), b);
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

    /// Vanilla's shapes in static fields (`ChestBlock.AABB`, a hanging sign's joined two boxes) are
    /// its boxes, the same object each read; a brushable block keeps what it turns into and its
    /// sounds; a definition builder lists its properties by name (FramedBlocks reads them).
    #[test]
    fn vanilla_statics_and_fields_mods_read_are_vanillas() {
        let mut rt = Rt::new();
        crate::install();
        let chest = api::get_static(&mut rt, "net/minecraft/world/level/block/ChestBlock.AABB");
        assert_eq!(crate::shapes::boxes(&rt, chest), Some(vec![[1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 14.0 / 16.0, 15.0 / 16.0]]));
        assert_eq!(api::get_static(&mut rt, "net/minecraft/world/level/block/ChestBlock.AABB"), chest);
        let sign = api::get_static(&mut rt, "net/minecraft/world/level/block/WallHangingSignBlock.SHAPE_NORTHSOUTH");
        assert_eq!(crate::shapes::boxes(&rt, sign).map(|b| b.len()), Some(2));
        let p = props(&mut rt);
        let b = rt.new_object(BRUSHABLE);
        let (sand, brush, done) = (rt.string("minecraft:sand"), rt.string("minecraft:item.brush.brushing.sand"), rt.string("minecraft:item.brush.brushing.sand.complete"));
        api::call(
            &mut rt,
            "net/minecraft/world/level/block/BrushableBlock.<init>.(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V",
            &[b, sand, brush, done, p],
        );
        assert_eq!(api::call(&mut rt, "net/minecraft/world/level/block/BrushableBlock.getBrushCompletedSound.()Lnet/minecraft/sounds/SoundEvent;", &[b]), done);
        assert_eq!(api::call(&mut rt, "net/minecraft/world/level/block/BrushableBlock.getTurnsInto.()Lnet/minecraft/world/level/block/Block;", &[b]), sand);
        // `BrushableBlock` adds `dusted`, which its builder then holds by name.
        let builder = rt.new_object(BUILDER);
        let list = jutil::new_list(&mut rt, Vec::new());
        rt.put_field(builder, "$props", list);
        let dusted = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.DUSTED");
        let arr = rt.alloc("[Lnet/minecraft/world/level/block/state/properties/Property;", Data::Array(vec![dusted]));
        api::call(&mut rt, "net/minecraft/world/level/block/state/StateDefinition$Builder.add.([Lnet/minecraft/world/level/block/state/properties/Property;)Lnet/minecraft/world/level/block/state/StateDefinition$Builder;", &[builder, arr]);
        let map = api::call(&mut rt, "net/minecraft/world/level/block/state/StateDefinition$Builder.$properties.()Ljava/util/Map;", &[builder]);
        let key = rt.string("dusted");
        assert_eq!(jutil::table_get(&mut rt, map, key), Some(dusted));
    }

    /// Mod blocks of vanilla's kinds copy the vanilla block that behaves as they do: a fence joins
    /// fences, a wall stands with its post up, farmland keeps its moisture. What their constructor
    /// was given decides as it does in vanilla: a door of iron's `BlockSetType` doesn't open by
    /// hand, a button of stone's ignores arrows, a plate of stone's senses mobs alone, a weighted
    /// plate counting to 150 is the heavy one.
    #[test]
    fn vanilla_kinds_copy_the_vanilla_block_that_behaves_as_they_do() {
        let mut rt = Rt::new();
        let p = props(&mut rt);
        let set = |rt: &mut Rt, f: &str| get_static(rt, BLOCK_SET_TYPE, f).unwrap();
        let (iron, oak, stone) = (set(&mut rt, "IRON"), set(&mut rt, "OAK"), set(&mut rt, "STONE"));
        const P: &str = "Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;";
        const S: &str = "Lnet/minecraft/world/level/block/state/properties/BlockSetType;";
        let blocks = [
            ("ruby:fence", "FenceBlock", format!("({P})V"), vec![p], "minecraft:oak_fence"),
            ("ruby:wall", "WallBlock", format!("({P})V"), vec![p], "minecraft:cobblestone_wall"),
            ("ruby:iron_door", "DoorBlock", format!("({S}{P})V"), vec![iron, p], "minecraft:iron_door"),
            ("ruby:oak_door", "DoorBlock", format!("({S}{P})V"), vec![oak, p], "minecraft:oak_door"),
            ("ruby:button", "ButtonBlock", format!("({S}I{P})V"), vec![stone, V::I(20), p], "minecraft:stone_button"),
            ("ruby:plate", "PressurePlateBlock", format!("({S}{P})V"), vec![stone, p], "minecraft:stone_pressure_plate"),
            ("ruby:heavy_plate", "WeightedPressurePlateBlock", format!("(I{S}{P})V"), vec![V::I(150), iron, p], "minecraft:heavy_weighted_pressure_plate"),
            ("ruby:soil", "FarmBlock", format!("({P})V"), vec![p], "minecraft:farmland"),
        ];
        for (id, class, desc, args, _) in &blocks {
            let owner = rt.static_key(&format!("net/minecraft/world/level/block/{class}"));
            let b = rt.new_object(owner);
            let init = rt.static_key(&format!("{owner}.<init>.{desc}"));
            api::call(&mut rt, init, &[&[b][..], args].concat());
            rt.registered.insert(("block".into(), (*id).into()), b);
        }
        let declared: std::collections::HashMap<String, String> = block_declarations(&mut rt).into_iter().collect();
        for (id, .., template) in &blocks {
            let json = &declared[*id];
            assert!(json.contains(&format!("\"template\": \"{template}\"")), "{id}: {json}");
            // Every state of theirs is one of the template's.
            assert!(json.contains("\"state_properties\": {}"), "{id}: {json}");
        }
        let wall = &declared["ruby:wall"];
        assert!(wall.contains("\"up\": \"true\"") && wall.contains("\"north\": \"none\""), "{wall}");
        assert!(declared["ruby:soil"].contains("\"default_state\": {\"moisture\": \"0\"}"), "{}", declared["ruby:soil"]);
    }

    const GET_BLOCK: &str = "net/minecraft/world/level/block/state/BlockState.getBlock.()Lnet/minecraft/world/level/block/Block;";
    const DEFAULT_STATE: &str = "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";

    #[test]
    fn states_made_before_registration_take_the_registered_name() {
        // Registrate's order: the block is made (its constructor keeps `defaultBlockState()`), then
        // registered (`RegisterEvent.register(key, helper -> helper.register(id, block))`).
        let mut rt = Rt::new();
        let p = props(&mut rt);
        let (b, other) = (rt.new_object("net/minecraft/world/level/block/RotatedPillarBlock"), rt.new_object("net/minecraft/world/level/block/RotatedPillarBlock"));
        for x in [b, other] {
            api::call(&mut rt, "net/minecraft/world/level/block/RotatedPillarBlock.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[x, p]);
        }
        let early = api::call(&mut rt, DEFAULT_STATE, &[b]);
        let theirs = api::call(&mut rt, DEFAULT_STATE, &[other]);
        // Two blocks without names are two blocks.
        assert_ne!(early, theirs);
        assert_eq!(api::call(&mut rt, GET_BLOCK, &[theirs]), other);
        let helper = rt.new_object("net/neoforged/neoforge/registries/RegisterEvent$RegisterHelper");
        let registry = rt.string("block");
        rt.put_field(helper, "$registry", registry);
        let id = rt.string("ruby:log");
        api::call(&mut rt, "net/neoforged/neoforge/registries/RegisterEvent$RegisterHelper.register.(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)V", &[helper, id, b]);
        // The same object, named: what kept it, and the game asking by name, see one state.
        assert_eq!(rt.str_of(early), Some("ruby:log[axis=y]"));
        assert_eq!(api::state(&mut rt, "ruby:log[axis=y]"), early);
        assert_eq!(api::call(&mut rt, DEFAULT_STATE, &[b]), early);
        assert_eq!(api::call(&mut rt, GET_BLOCK, &[early]), b);
        // The other one waits for its own name.
        assert_eq!(api::call(&mut rt, GET_BLOCK, &[theirs]), other);
        assert!(!rt.str_of(theirs).unwrap_or("").starts_with("ruby:"));
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

    /// A mod block's `super.getShape(...)` to a vanilla kind whose shape follows its state answers
    /// vanilla 1.21.1's boxes for that state (from its classes): a door by its facing, hinge and
    /// openness, a pressed button sunk in, a stair's slab and the quarters above it, a pane's post
    /// and the arms to the sides it joins.
    #[test]
    fn super_get_shape_to_a_vanilla_kind_follows_the_state() {
        let mut rt = Rt::new();
        let shape = |rt: &mut Rt, class: &str, state: &str| -> Vec<[f64; 6]> {
            let key = rt.static_key(&format!("net/minecraft/world/level/block/{class}.{GET_SHAPE}"));
            let s = api::state(rt, state);
            let v = api::call(rt, key, &[V::NULL, s, V::NULL, V::NULL, V::NULL]);
            crate::shapes::boxes(rt, v).unwrap().iter().map(|b| b.map(|x| x * 16.0)).collect()
        };
        let inside = |boxes: &[[f64; 6]], p: [f64; 3]| boxes.iter().any(|b| (0..3).all(|i| b[i] < p[i] && p[i] < b[i + 3]));
        let door = shape(&mut rt, "DoorBlock", "ruby:door[facing=north,half=lower,hinge=right,open=true,powered=false]");
        assert_eq!(door, [[13.0, 0.0, 0.0, 16.0, 16.0, 16.0]]);
        let closed = shape(&mut rt, "DoorBlock", "ruby:door[facing=north,half=lower,hinge=right,open=false,powered=false]");
        assert_eq!(closed, [[0.0, 0.0, 13.0, 16.0, 16.0, 16.0]]);
        let button = shape(&mut rt, "ButtonBlock", "ruby:button[face=wall,facing=east,powered=true]");
        assert_eq!(button, [[0.0, 6.0, 5.0, 1.0, 10.0, 11.0]]);
        // Facing east, the step rises on the east half.
        let stair = shape(&mut rt, "StairBlock", "ruby:stairs[facing=east,half=bottom,shape=straight,waterlogged=false]");
        for (p, filled) in [([4.0, 4.0, 4.0], true), ([12.0, 4.0, 12.0], true), ([12.0, 12.0, 4.0], true), ([12.0, 12.0, 12.0], true), ([4.0, 12.0, 4.0], false), ([4.0, 12.0, 12.0], false)] {
            assert_eq!(inside(&stair, p), filled, "{p:?} in {stair:?}");
        }
        let pane = shape(&mut rt, "StainedGlassPaneBlock", "ruby:pane[east=false,north=true,south=false,waterlogged=false,west=true]");
        for (p, filled) in [([8.0, 8.0, 8.0], true), ([8.0, 8.0, 2.0], true), ([2.0, 8.0, 8.0], true), ([14.0, 8.0, 8.0], false), ([8.0, 8.0, 14.0], false), ([3.0, 8.0, 3.0], false)] {
            assert_eq!(inside(&pane, p), filled, "{p:?} in {pane:?}");
        }
        // A chest facing north whose left half joins the east one reaches the east edge (a
        // keypad chest's `super.getShape`); a single one stands a pixel in from each side.
        let left = shape(&mut rt, "ChestBlock", "ruby:chest[facing=north,type=left,waterlogged=false]");
        assert_eq!(left, [[1.0, 0.0, 1.0, 16.0, 14.0, 15.0]]);
        let right = shape(&mut rt, "ChestBlock", "ruby:chest[facing=north,type=right,waterlogged=false]");
        assert_eq!(right, [[0.0, 0.0, 1.0, 15.0, 14.0, 15.0]]);
        let single = shape(&mut rt, "ChestBlock", "ruby:chest[facing=east,type=single,waterlogged=false]");
        assert_eq!(single, [[1.0, 0.0, 1.0, 15.0, 14.0, 15.0]]);
        // A hopper is hollow at the top, and its spout points where it faces.
        let hopper = shape(&mut rt, "HopperBlock", "ruby:hopper[enabled=true,facing=west]");
        for (p, filled) in [([8.0, 10.5, 8.0], true), ([1.0, 14.0, 8.0], true), ([8.0, 14.0, 8.0], false), ([8.0, 7.0, 8.0], true), ([2.0, 6.0, 8.0], true), ([14.0, 6.0, 8.0], false), ([8.0, 2.0, 8.0], false)] {
            assert_eq!(inside(&hopper, p), filled, "{p:?} in {hopper:?}");
        }
    }

    /// A mod block whose class extends one of vanilla's with a shape of its own, and draws none
    /// itself (Create's lectern controller, its latches and pulse repeaters, its controller rail),
    /// is declared see-through with vanilla's shapes for its states: a lectern's base, post and the
    /// sloped top that stands above the block, by where it faces; a repeater's, its diode's plate.
    /// A kind that is a full cube stays opaque; weathering copper stairs are stairs.
    #[test]
    fn a_block_draws_the_shape_of_the_vanilla_class_it_extends() {
        let mut rt = Rt::new();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let p = props(&mut rt);
        const P: &str = "Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;";
        let stairs = format!(
            "(Lnet/minecraft/world/level/block/WeatheringCopper$WeatherState;Lnet/minecraft/world/level/block/state/BlockState;{P})V"
        );
        let blocks = [
            (
                "ruby:lectern",
                "test/Lectern",
                "LecternBlock",
                format!("({P})V"),
                vec![p],
            ),
            (
                "ruby:repeater",
                "test/Repeater",
                "RepeaterBlock",
                format!("({P})V"),
                vec![p],
            ),
            (
                "ruby:machine",
                "test/Machine",
                "HorizontalDirectionalBlock",
                format!("({P})V"),
                vec![p],
            ),
            (
                "ruby:copper_stairs",
                "test/CopperStairs",
                "WeatheringCopperStairBlock",
                stairs,
                vec![V::NULL, V::NULL, p],
            ),
        ];
        for (id, class, vanilla, desc, args) in &blocks {
            let b = rt.new_object(*class);
            let init = rt.static_key(&format!(
                "net/minecraft/world/level/block/{vanilla}.<init>.{desc}"
            ));
            api::call(&mut rt, init, &[&[b][..], args].concat());
            rt.registered.insert(("block".into(), (*id).into()), b);
        }
        let declared: std::collections::HashMap<String, serde_json::Value> =
            block_declarations(&mut rt)
                .into_iter()
                .map(|(id, json)| (id, serde_json::from_str(&json).unwrap()))
                .collect();
        let template = |id: &str| {
            declared[id]["template"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        let boxes = |v: &serde_json::Value| -> Vec<[f64; 6]> {
            let each = v.as_array().unwrap().iter().map(|b| {
                b.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap() * 16.0)
                    .collect::<Vec<f64>>()
            });
            each.map(|b| [b[0], b[1], b[2], b[3], b[4], b[5]]).collect()
        };
        let inside = |boxes: &[[f64; 6]], p: [f64; 3]| {
            boxes
                .iter()
                .any(|b| (0..3).all(|i| b[i] < p[i] && p[i] < b[i + 3]))
        };
        assert_eq!(template("ruby:lectern"), "minecraft:glass");
        let shapes = &declared["ruby:lectern"]["shapes"];
        assert_eq!(shapes["property"], "facing", "{shapes}");
        let (north, south) = (
            boxes(&shapes["values"]["north"]),
            boxes(&shapes["values"]["south"]),
        );
        // The base, the post (not beside it), and the top's high edge, above the block, at the back.
        for (p, filled) in [
            ([8.0, 1.0, 8.0], true),
            ([8.0, 8.0, 8.0], true),
            ([2.0, 8.0, 2.0], false),
            ([8.0, 17.0, 12.0], true),
            ([8.0, 17.0, 4.0], false),
        ] {
            assert_eq!(inside(&north, p), filled, "{p:?} in {north:?}");
        }
        assert!(
            inside(&south, [8.0, 17.0, 4.0]) && !inside(&south, [8.0, 17.0, 12.0]),
            "{south:?}"
        );
        assert_eq!(template("ruby:repeater"), "minecraft:glass");
        assert_eq!(
            boxes(&declared["ruby:repeater"]["shapes"]),
            [[0.0, 0.0, 0.0, 16.0, 2.0, 16.0]]
        );
        assert_eq!(template("ruby:machine"), "minecraft:stone");
        assert!(declared["ruby:machine"].get("shapes").is_none());
        assert_eq!(template("ruby:copper_stairs"), "minecraft:stone_stairs");
        assert_eq!(
            declared["ruby:copper_stairs"]["state_properties"],
            serde_json::json!({})
        );
    }

    /// `EnumProperty.create(name, RailShape.class, RailShape::isAscending)` (FramedBlocks' rail
    /// slopes): the vanilla enum's constants the predicate keeps, in order.
    #[test]
    fn an_enum_property_filters_a_vanilla_enums_constants() {
        fn vcall(rt: &mut Rt, _key: &'static str, _short: &'static str, args: &[V]) -> V {
            // `RailShape::isAscending`, as the translated method reference calls it.
            api::call(rt, "net/minecraft/world/level/block/state/properties/RailShape.isAscending.()Z", &[args[1]])
        }
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        let (name, class) = (rt.string("shape"), rt.string("net/minecraft/world/level/block/state/properties/RailShape"));
        let ascending = rt.new_object("test/IsAscending");
        let p = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/EnumProperty.create.(Ljava/lang/String;Ljava/lang/Class;Ljava/util/function/Predicate;)Lnet/minecraft/world/level/block/state/properties/EnumProperty;", &[name, class, ascending]);
        assert_eq!(property_values(&mut rt, p), ["ascending_east", "ascending_west", "ascending_north", "ascending_south"]);
    }

    /// A mod's own property class (StorageDrawers' `IntegerSetProperty`: `super(name,
    /// Integer.class)`, values `1,2,4` it keeps itself): a block that adds it has the states its
    /// `getPossibleValues` gives, as its `getName` spells them; a state's value is read back
    /// through its own `getValue` and written with its `getName`.
    #[test]
    fn a_mods_own_property_class_gives_a_block_its_states() {
        const SET: &str = "test/IntegerSetProperty";
        const DRAWER: &str = "test/Drawer";
        const INTEGER: &str = "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;";
        fn overrides(class: &str, short: &str) -> bool {
            match class {
                SET => [POSSIBLE_VALUES, VALUE_NAME, PARSE_VALUE].contains(&short),
                DRAWER => short == DEFINE,
                _ => false,
            }
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                POSSIBLE_VALUES => rt.get_field(args[0], "values"),
                VALUE_NAME => {
                    let n = rt.get_field(args[1], "value").i().to_string();
                    rt.string(&n)
                }
                PARSE_VALUE => {
                    let n: Option<i32> = rt.str_of(args[1]).and_then(|t| t.parse().ok());
                    let held = jutil::elements(rt, rt.get_field(args[0], "values"));
                    let found = held.into_iter().find(|v| Some(rt.get_field(*v, "value").i()) == n).unwrap_or(V::NULL);
                    api_common::optional(rt, found)
                }
                DEFINE => {
                    let slots = rt.statics["test/Drawer.SLOTS"];
                    let arr = rt.new_array(1, V::NULL);
                    rt.array_set(arr, 0, slots);
                    api::call(rt, "net/minecraft/world/level/block/state/StateDefinition$Builder.add.([Lnet/minecraft/world/level/block/state/properties/Property;)Lnet/minecraft/world/level/block/state/StateDefinition$Builder;", &[args[1], arr])
                }
                _ => api::call(rt, key, args),
            }
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(SET, "net/minecraft/world/level/block/state/properties/Property");
        rt.supers.insert(DRAWER, "net/minecraft/world/level/block/Block");
        let slots = rt.new_object(SET);
        let (name, class) = (rt.string("slots"), rt.string("java/lang/Integer"));
        api::call(&mut rt, "net/minecraft/world/level/block/state/properties/Property.<init>.(Ljava/lang/String;Ljava/lang/Class;)V", &[slots, name, class]);
        let values: Vec<V> = [1, 2, 4].iter().map(|n| api::call(&mut rt, INTEGER, &[V::I(*n)])).collect();
        let values = jutil::new_list(&mut rt, values);
        rt.put_field(slots, "values", values);
        rt.statics.insert("test/Drawer.SLOTS", slots);
        assert_eq!(api_common::prop_parts(&rt, slots), ("slots".to_owned(), "int".to_owned()));

        let p = props(&mut rt);
        let drawer = rt.new_object(DRAWER);
        api::call(&mut rt, "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[drawer, p]);
        rt.registered.insert(("block".into(), "ruby:drawer".into()), drawer);
        let json = block_declaration(&mut rt, "ruby:drawer", drawer, false);
        assert!(json.contains("\"slots\": {\"type\": \"enum\", \"values\": [\"1\", \"2\", \"4\"]}"), "{json}");
        assert!(json.contains("\"default_state\": {\"slots\": \"1\"}"), "{json}");

        let two = api::state(&mut rt, "ruby:drawer[slots=2]");
        let got = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;", &[two, slots]);
        assert_eq!((rt.class_of(got), rt.get_field(got, "value")), ("java/lang/Integer", V::I(2)));
        let four = api::call(&mut rt, INTEGER, &[V::I(4)]);
        let set = api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;", &[two, slots, four]);
        assert_eq!(rt.str_of(set), Some("ruby:drawer[slots=4]"));
        // Vanilla's kinds spell a value as states do: `FACING.getName(EAST)` is `east`.
        let facing = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.FACING");
        let east = api::get_static(&mut rt, "net/minecraft/core/Direction.EAST");
        let n = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/DirectionProperty.getName.(Ljava/lang/Comparable;)Ljava/lang/String;", &[facing, east]);
        assert_eq!(rt.str_of(n), Some("east"));
    }

    /// A mod plant's `super.canSurvive` to `DoublePlantBlock` or `FlowerBlock` answers as NeoForge
    /// 1.21.1's does. A lower half asks a mod soil's `canSustainPlant` first, then the plant's own
    /// `mayPlaceOn`, else the bush's (1.21.1's dirt, grass and podzol too, or farmland). An upper half needs the plant's lower half
    /// below, except in the check made before the block is set. `DoublePlantBlock.placeAt` sets
    /// both halves, the lower waterlogged where there is water.
    /// A mod block's survival by its own code, as the game's hooks and the world generator ask it:
    /// Farmer's Delight's wild crops (`WildCropBlock extends BushBlock`, its `mayPlaceOn`
    /// `BlockTags.DIRT || BlockTags.SAND`) stand on sand as on grass, which their template's rule
    /// refuses; a block with its own `canSurvive` answers it; a block with neither is left to its
    /// template. Only the first two are claimed.
    #[test]
    fn a_mod_blocks_survival_is_its_own_codes() {
        const WILD: &str = "test/WildCropBlock";
        const LANTERN: &str = "test/HangingLantern";
        const CRATE: &str = "test/Crate";
        const IS: &str = "net/minecraft/world/level/block/state/BlockState.is.(Lnet/minecraft/tags/TagKey;)Z";
        fn overrides(class: &str, short: &str) -> bool {
            (class == WILD && short == MAY_PLACE_ON) || (class == LANTERN && short == api::CAN_SURVIVE)
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                MAY_PLACE_ON if rt.class_of(args[0]) == WILD => {
                    let dirt = api::get_static(rt, "net/minecraft/tags/BlockTags.DIRT");
                    let sand = api::get_static(rt, "net/minecraft/tags/BlockTags.SAND");
                    let on = |rt: &mut Rt, tag: V| api::call(rt, IS, &[args[1], tag]).i() != 0;
                    V::I((on(rt, dirt) || on(rt, sand)) as i32)
                }
                // `!level.getBlockState(pos.above()).isAir()`.
                api::CAN_SURVIVE if rt.class_of(args[0]) == LANTERN => {
                    let above = api::call(rt, "net/minecraft/core/BlockPos.above.()Lnet/minecraft/core/BlockPos;", &[args[3]]);
                    let state = api::call(rt, "net/minecraft/world/level/LevelReader.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;", &[args[2], above]);
                    V::I((api_common::text_of(rt, state) != "minecraft:air") as i32)
                }
                _ => api::call(rt, key, args),
            }
        }
        struct Column(std::collections::BTreeMap<[i32; 3], String>);
        impl crate::host::Host for Column {
            fn block_state(&self, pos: [i32; 3]) -> String {
                self.0.get(&pos).cloned().unwrap_or_else(|| "minecraft:air".into())
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
            // 26.3's tags: 1.21.1's `dirt` (grass and podzol too) is `substrate_overworld`.
            fn block_in_tag(&self, block: &str, tag: &str) -> bool {
                match tag {
                    "minecraft:dirt" => matches!(block, "minecraft:dirt" | "minecraft:coarse_dirt" | "minecraft:rooted_dirt"),
                    "minecraft:substrate_overworld" => matches!(block, "minecraft:dirt" | "minecraft:coarse_dirt" | "minecraft:rooted_dirt" | "minecraft:grass_block" | "minecraft:podzol"),
                    "minecraft:sand" => matches!(block, "minecraft:sand" | "minecraft:red_sand" | "minecraft:suspicious_sand"),
                    _ => false,
                }
            }
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(WILD, "net/minecraft/world/level/block/BushBlock");
        rt.supers.insert(LANTERN, "net/minecraft/world/level/block/Block");
        rt.supers.insert(CRATE, "net/minecraft/world/level/block/Block");
        for (class, id) in [(WILD, "test:wild_cabbages"), (LANTERN, "test:hanging_lantern"), (CRATE, "test:crate")] {
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), id.into()), b);
        }
        assert_eq!(survival_blocks(&rt), ["test:hanging_lantern", "test:wild_cabbages"]);
        let column = |below: &str, above: &str| Column([([0, 0, 0], below.to_owned()), ([0, 2, 0], above.to_owned())].into());
        let survives = |rt: &mut Rt, state: &str, below: &str, above: &str| crate::glue::can_survive(rt, &mut column(below, above), [0, 1, 0], state);
        // The wild crop: grass (1.21.1's dirt) and sand, not stone.
        assert_eq!(survives(&mut rt, "test:wild_cabbages", "minecraft:sand", "minecraft:air"), Some(true));
        assert_eq!(survives(&mut rt, "test:wild_cabbages", "minecraft:grass_block", "minecraft:air"), Some(true));
        assert_eq!(survives(&mut rt, "test:wild_cabbages", "minecraft:stone", "minecraft:air"), Some(false));
        // The lantern's own rule: something above it.
        assert_eq!(survives(&mut rt, "test:hanging_lantern", "minecraft:air", "minecraft:oak_planks"), Some(true));
        assert_eq!(survives(&mut rt, "test:hanging_lantern", "minecraft:stone", "minecraft:air"), Some(false));
        // Neither: the template's rule; a block that isn't the mod's: the game's.
        assert_eq!(survives(&mut rt, "test:crate", "minecraft:air", "minecraft:air"), None);
        assert_eq!(survives(&mut rt, "minecraft:poppy", "minecraft:sand", "minecraft:air"), None);
        // The plant's own `mayPlaceOn`, which its template's survival asks on the live level.
        let may = |rt: &mut Rt, state: &str, ground: &str| crate::glue::may_place_on(rt, &mut column(ground, "minecraft:air"), state, ground, [0, 0, 0]);
        assert_eq!(may(&mut rt, "test:wild_cabbages", "minecraft:red_sand"), Some(true));
        assert_eq!(may(&mut rt, "test:wild_cabbages", "minecraft:gravel"), Some(false));
        assert_eq!(may(&mut rt, "test:crate", "minecraft:sand"), None);
        // The mod's own code asking `state.canSurvive(level, pos)` gets the same answer.
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let (state, pos) = (api::state(&mut rt, "test:wild_cabbages"), api::block_pos(&mut rt, 0, 1, 0));
        let mut on_sand = column("minecraft:sand", "minecraft:air");
        let asked = rt.with_host(&mut on_sand, |rt| {
            api::call(rt, "net/minecraft/world/level/block/state/BlockState.canSurvive.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z", &[state, level, pos])
        });
        assert_eq!(asked, V::I(1));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Farmer's Delight's rice calls `BushBlock.tick` from its random tick (vanilla's scheduled tick
    /// there does nothing) and grows its panicles only into an empty block, as
    /// `LevelReader.isEmptyBlock` says: air of any kind is empty, stone or water is not. A
    /// `super.tick` to a class with a tick of its own (a falling block's) is not answered as empty.
    #[test]
    fn a_rices_tick_and_its_empty_block_test_are_vanillas() {
        let mut rt = Rt::new();
        let mut host = crate::host::MemoryHost::default();
        host.blocks.insert([0, 64, 0], "test:rice[age=3,supporting=false]".into());
        host.blocks.insert([1, 65, 0], "minecraft:cave_air".into());
        host.blocks.insert([2, 65, 0], "minecraft:stone".into());
        host.blocks.insert([3, 65, 0], "minecraft:water[level=0]".into());
        let before = host.blocks.clone();
        let tick = |rt: &mut Rt, class: &str| rt.static_key(&format!("net/minecraft/world/level/block/{class}.tick.{TICK_DESC}"));
        const EMPTY: &str = "net/minecraft/server/level/ServerLevel.isEmptyBlock.(Lnet/minecraft/core/BlockPos;)Z";
        let (bush, falling) = (tick(&mut rt, "BushBlock"), tick(&mut rt, "FallingBlock"));
        let empty = rt.with_host(&mut host, |rt| {
            let level = rt.new_object("net/minecraft/server/level/ServerLevel");
            let rice = rt.new_object("test/RiceBlock");
            let state = api::state(rt, "test:rice[age=3,supporting=false]");
            let (pos, random) = (api::block_pos(rt, 0, 64, 0), rt.new_object("net/minecraft/util/RandomSource"));
            assert_eq!(api::call(rt, bush, &[rice, state, level, pos, random]), V::Void);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
            api::call(rt, falling, &[rice, state, level, pos, random]);
            assert!(rt.unmapped.contains_key(falling), "a falling block's own tick isn't the empty one");
            (0..4)
                .map(|x| {
                    let above = api::block_pos(rt, x, 65, 0);
                    api::call(rt, EMPTY, &[level, above]) == V::I(1)
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(empty, [true, true, false, false]);
        assert_eq!(host.blocks, before, "the empty tick changes nothing");
    }

    #[test]
    fn a_mod_plants_super_survival_is_vanillas() {
        const RICE: &str = "test/Rice";
        const FLOWER: &str = "test/Flower";
        const SOIL: &str = "test/RichSoil";
        fn overrides(class: &str, short: &str) -> bool {
            (class == RICE && short == MAY_PLACE_ON) || (class == SOIL && short == SUSTAIN)
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                // The rice's own soil: sand only.
                MAY_PLACE_ON if rt.class_of(args[0]) == RICE => V::I(api_common::text_of(rt, args[1]).starts_with("minecraft:sand") as i32),
                SUSTAIN if rt.class_of(args[0]) == SOIL => api::api_enum(rt, "net/neoforged/neoforge/common/util/TriState", "TRUE", 0),
                _ => api::call(rt, key, args),
            }
        }
        struct Column(std::collections::BTreeMap<[i32; 3], String>);
        impl crate::host::Host for Column {
            fn block_state(&self, pos: [i32; 3]) -> String {
                self.0.get(&pos).cloned().unwrap_or_else(|| "minecraft:air".into())
            }
            fn set_block_state(&mut self, pos: [i32; 3], state: &str) -> bool {
                self.0.insert(pos, state.into());
                true
            }
            // 26.3's tags: `dirt` is dirt, coarse and rooted dirt; 1.21.1's (grass too) is
            // `substrate_overworld`.
            fn block_in_tag(&self, block: &str, tag: &str) -> bool {
                match tag {
                    "minecraft:dirt" => matches!(block, "minecraft:dirt" | "minecraft:coarse_dirt" | "minecraft:rooted_dirt"),
                    "minecraft:substrate_overworld" => matches!(block, "minecraft:dirt" | "minecraft:coarse_dirt" | "minecraft:rooted_dirt" | "minecraft:grass_block" | "minecraft:podzol"),
                    _ => false,
                }
            }
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        for (class, id) in [(RICE, "test:rice"), (FLOWER, "test:flower"), (SOIL, "test:rich_soil")] {
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), id.into()), b);
        }
        let block = |rt: &Rt, id: &str| rt.registered[&("block".to_owned(), id.to_owned())];
        let (rice, flower) = (block(&rt, "test:rice"), block(&rt, "test:flower"));
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        const DOUBLE: &str = "net/minecraft/world/level/block/DoublePlantBlock.canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
        const FLOWER_SURVIVES: &str = "net/minecraft/world/level/block/FlowerBlock.canSurvive.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";
        let survives = |rt: &mut Rt, key: &'static str, plant: V, state: &str, below: &str| {
            let mut column = Column([([0, 0, 0], below.to_owned())].into());
            let (state, pos) = (api::state(rt, state), api::block_pos(rt, 0, 1, 0));
            rt.with_host(&mut column, |rt| api::call(rt, key, &[plant, state, level, pos])) == V::I(1)
        };
        // The rice's lower half: its own soil, unless a mod soil decides.
        assert!(survives(&mut rt, DOUBLE, rice, "test:rice[half=lower]", "minecraft:sand"));
        assert!(!survives(&mut rt, DOUBLE, rice, "test:rice[half=lower]", "minecraft:dirt"));
        assert!(survives(&mut rt, DOUBLE, rice, "test:rice[half=lower]", "test:rich_soil"));
        // Its upper half: its lower half below; before it is set, the soil check.
        assert!(survives(&mut rt, DOUBLE, rice, "test:rice[half=upper]", "test:rice[half=lower]"));
        assert!(!survives(&mut rt, DOUBLE, rice, "test:rice[half=upper]", "minecraft:sand"));
        assert!(survives(&mut rt, DOUBLE, rice, "minecraft:tall_grass[half=upper]", "minecraft:sand"));
        // A flower that keeps the bush's soil: dirt, farmland.
        assert!(survives(&mut rt, FLOWER_SURVIVES, flower, "test:flower", "minecraft:grass_block"));
        assert!(survives(&mut rt, FLOWER_SURVIVES, flower, "test:flower", "minecraft:podzol"));
        assert!(survives(&mut rt, FLOWER_SURVIVES, flower, "test:flower", "minecraft:farmland[moisture=7]"));
        assert!(!survives(&mut rt, FLOWER_SURVIVES, flower, "test:flower", "minecraft:sand"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);

        // Both halves from any state of the plant; water below keeps the lower half waterlogged.
        let mut column = Column([([5, 1, 5], "minecraft:water[level=0]".to_owned())].into());
        let (state, pos) = (api::state(&mut rt, "test:rice[half=upper,waterlogged=false]"), api::block_pos(&mut rt, 5, 1, 5));
        let place = "net/minecraft/world/level/block/DoublePlantBlock.placeAt.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;I)V";
        rt.with_host(&mut column, |rt| api::call(rt, place, &[level, state, pos, V::I(2)]));
        assert_eq!(column.0.get(&[5, 1, 5]).map(String::as_str), Some("test:rice[half=lower,waterlogged=true]"));
        assert_eq!(column.0.get(&[5, 2, 5]).map(String::as_str), Some("test:rice[half=upper,waterlogged=false]"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The tests' lambdas: `test/Shape` answers a state's text length; `test/Fill`, a cauldron
    /// interaction, keeps the stack it was given and consumes the click.
    fn lambdas(rt: &mut Rt, _key: &'static str, _: &'static str, args: &[V]) -> V {
        match rt.class_of(args[0]) {
            "test/Shape" => V::I(rt.str_of(args[1]).map_or(0, str::len) as i32),
            "test/Fill" => {
                rt.put_field(args[0], "stack", args[6]);
                api_common::result(rt, "net/minecraft/world/ItemInteractionResult", "CONSUME")
            }
            _ => V::Void,
        }
    }

    #[test]
    fn a_blocks_states_are_every_combination_in_vanillas_order() {
        let mut rt = Rt::new();
        rt.vcall = Some(lambdas);
        let p = props(&mut rt);
        let slab = rt.new_object("net/minecraft/world/level/block/SlabBlock");
        api::call(&mut rt, "net/minecraft/world/level/block/SlabBlock.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[slab, p]);
        rt.registered.insert(("block".into(), "ruby:slab".into()), slab);
        // `type` before `waterlogged`, the first varying slowest; each property's values in order.
        let def = api::call(&mut rt, "net/minecraft/world/level/block/Block.getStateDefinition.()Lnet/minecraft/world/level/block/state/StateDefinition;", &[slab]);
        let all = api::call(&mut rt, "net/minecraft/world/level/block/state/StateDefinition.getPossibleStates.()Lcom/google/common/collect/ImmutableList;", &[def]);
        let texts: Vec<String> = jutil::elements(&rt, all).iter().map(|s| rt.str_of(*s).unwrap_or("").to_owned()).collect();
        assert_eq!(
            texts,
            [
                "ruby:slab[type=top,waterlogged=true]",
                "ruby:slab[type=top,waterlogged=false]",
                "ruby:slab[type=bottom,waterlogged=true]",
                "ruby:slab[type=bottom,waterlogged=false]",
                "ruby:slab[type=double,waterlogged=true]",
                "ruby:slab[type=double,waterlogged=false]",
            ]
        );
        // `getShapeForEachState`: each state's answer, read back by state (copycats' shape cache).
        let f = rt.new_object("test/Shape");
        let shapes = api::call(&mut rt, "net/minecraft/world/level/block/Block.getShapeForEachState.(Ljava/util/function/Function;)Lcom/google/common/collect/ImmutableMap;", &[slab, f]);
        assert_eq!(jutil::table_pairs(&rt, shapes).len(), 6);
        let bottom = api::state(&mut rt, "ruby:slab[type=bottom,waterlogged=false]");
        let got = api::call(&mut rt, "com/google/common/collect/ImmutableMap.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[shapes, bottom]);
        assert_eq!(got, V::I("ruby:slab[type=bottom,waterlogged=false]".len() as i32));
        // `cycle` takes the property's next value, the first after the last.
        let ty = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.SLAB_TYPE");
        let cycle = "net/minecraft/world/level/block/state/BlockState.cycle.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Object;";
        let next = api::call(&mut rt, cycle, &[bottom, ty]);
        assert_eq!(rt.str_of(next), Some("ruby:slab[type=double,waterlogged=false]"));
        let wrapped = api::call(&mut rt, cycle, &[next, ty]);
        assert_eq!(rt.str_of(wrapped), Some("ruby:slab[type=top,waterlogged=false]"));
    }

    /// Vanilla 1.21.1's `BlockSetType` and `SoundType` constants (from its class file): iron doors
    /// don't open by hand, stone plates sense mobs alone, metal sounds at pitch 1.5, the anvil at
    /// volume 0.3; a mod's `new BlockSetType(name)` is a wooden set.
    #[test]
    fn block_set_and_sound_types_answer_as_vanilla_does() {
        let mut rt = Rt::new();
        let set = |rt: &mut Rt, f: &str| get_static(rt, BLOCK_SET_TYPE, f).unwrap();
        let ask = |rt: &mut Rt, t: V, m: &str, d: &str| {
            let k = rt.static_key(&format!("{BLOCK_SET_TYPE}.{m}.(){d}"));
            api::call(rt, k, &[t])
        };
        let iron = set(&mut rt, "IRON");
        assert_eq!(ask(&mut rt, iron, "canOpenByHand", "Z"), V::I(0));
        let oak = set(&mut rt, "OAK");
        assert_eq!(ask(&mut rt, oak, "canOpenByHand", "Z"), V::I(1));
        let name = ask(&mut rt, oak, "name", "Ljava/lang/String;");
        assert_eq!(rt.str_of(name), Some("oak"));
        let stone = set(&mut rt, "STONE");
        let mobs = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockSetType$PressurePlateSensitivity.MOBS");
        assert_eq!(ask(&mut rt, stone, "pressurePlateSensitivity", "Lnet/minecraft/world/level/block/state/properties/BlockSetType$PressurePlateSensitivity;"), mobs);
        assert_eq!(set(&mut rt, "OAK"), oak);
        let ruby = rt.new_object(BLOCK_SET_TYPE);
        let n = rt.string("ruby");
        api::call(&mut rt, "net/minecraft/world/level/block/state/properties/BlockSetType.<init>.(Ljava/lang/String;)V", &[ruby, n]);
        let wood = get_static(&mut rt, SOUND_TYPE, "WOOD").unwrap();
        assert_eq!(ask(&mut rt, ruby, "soundType", "Lnet/minecraft/world/level/block/SoundType;"), wood);
        api::call(&mut rt, "net/minecraft/world/level/block/state/properties/BlockSetType.register.(Lnet/minecraft/world/level/block/state/properties/BlockSetType;)Lnet/minecraft/world/level/block/state/properties/BlockSetType;", &[ruby]);
        let all = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/BlockSetType.values.()Ljava/util/stream/Stream;", &[]);
        let listed = match rt.heap.get(all.r() as usize).map(|o| &o.data) { Some(Data::Stream(v)) => v.clone(), _ => Vec::new() };
        assert_eq!(listed.len(), 17);
        assert_eq!(listed.last(), Some(&ruby));
        // Sound types.
        let sound = |rt: &mut Rt, t: V, m: &str, d: &str| {
            let k = rt.static_key(&format!("{SOUND_TYPE}.{m}.(){d}"));
            api::call(rt, k, &[t])
        };
        let metal = get_static(&mut rt, SOUND_TYPE, "METAL").unwrap();
        assert_eq!(sound(&mut rt, metal, "getPitch", "F"), V::F(1.5));
        let anvil = get_static(&mut rt, SOUND_TYPE, "ANVIL").unwrap();
        assert_eq!(sound(&mut rt, anvil, "getVolume", "F"), V::F(0.3));
        let broke = sound(&mut rt, wood, "getBreakSound", "Lnet/minecraft/sounds/SoundEvent;");
        assert_eq!(broke, api::get_static(&mut rt, "net/minecraft/sounds/SoundEvents.WOOD_BREAK"));
        assert_ne!(broke, V::NULL);
        // A mod's own: what it was made with.
        let own = rt.new_object(SOUND_TYPE);
        let events: Vec<V> = ["a", "b", "c", "d", "e"].iter().map(|e| rt.string(e)).collect();
        let mut args = vec![own, V::F(0.5), V::F(2.0)];
        args.extend(events.iter().copied());
        api::call(&mut rt, "net/minecraft/world/level/block/SoundType.<init>.(FFLnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundEvent;Lnet/minecraft/sounds/SoundEvent;)V", &args);
        assert_eq!(sound(&mut rt, own, "getPlaceSound", "Lnet/minecraft/sounds/SoundEvent;"), events[2]);
        assert_eq!(sound(&mut rt, own, "getPitch", "F"), V::F(2.0));
        // Constants whose sounds aren't their own name's: a crop is planted, twisting vines sound
        // like weeping ones at half pitch.
        let crop = get_static(&mut rt, SOUND_TYPE, "CROP").unwrap();
        let planted = api::get_static(&mut rt, "net/minecraft/sounds/SoundEvents.CROP_PLANTED");
        assert_eq!(sound(&mut rt, crop, "getPlaceSound", "Lnet/minecraft/sounds/SoundEvent;"), planted);
        let vines = get_static(&mut rt, SOUND_TYPE, "TWISTING_VINES").unwrap();
        assert_eq!(sound(&mut rt, vines, "getPitch", "F"), V::F(0.5));
    }

    #[test]
    fn an_armour_layer_names_its_texture_as_vanilla_does() {
        let mut rt = Rt::new();
        let layer = rt.new_object(ARMOR_LAYER);
        let id = rt.string("create:copper_diving");
        api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial$Layer.<init>.(Lnet/minecraft/resources/ResourceLocation;)V", &[layer, id]);
        let outer = api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial$Layer.texture.(Z)Lnet/minecraft/resources/ResourceLocation;", &[layer, V::I(0)]);
        assert_eq!(rt.str_of(outer), Some("create:textures/models/armor/copper_diving_layer_1.png"));
        let dyed = rt.new_object(ARMOR_LAYER);
        let (leather, overlay) = (rt.string("leather"), rt.string("_overlay"));
        api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial$Layer.<init>.(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/String;Z)V", &[dyed, leather, overlay, V::I(1)]);
        let inner = api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial$Layer.texture.(Z)Lnet/minecraft/resources/ResourceLocation;", &[dyed, V::I(1)]);
        assert_eq!(rt.str_of(inner), Some("minecraft:textures/models/armor/leather_layer_2_overlay.png"));
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial$Layer.dyeable.()Z", &[dyed]), V::I(1));
    }

    /// Vanilla's armour materials answer 1.21.1's values (`ArmorMaterials`): Mekanism's armour
    /// copies netherite's defense, toughness and knockback resistance.
    #[test]
    fn a_vanilla_armour_material_answers_its_values() {
        let mut rt = Rt::new();
        let material = |rt: &mut Rt, key: &'static str| {
            let holder = api::get_static(rt, key);
            api::call(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder])
        };
        let netherite = material(&mut rt, "net/minecraft/world/item/ArmorMaterials.NETHERITE");
        let defense = api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.defense.()Ljava/util/Map;", &[netherite]);
        for (ty, points) in [("HELMET", 3), ("CHESTPLATE", 8), ("LEGGINGS", 6), ("BOOTS", 3), ("BODY", 11)] {
            let key = Box::leak(format!("net/minecraft/world/item/ArmorItem$Type.{ty}").into_boxed_str());
            let ty = api::get_static(&mut rt, key);
            let boxed = api::call(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[defense, ty]);
            assert_eq!(api::call(&mut rt, "java/lang/Integer.intValue.()I", &[boxed]), V::I(points), "{key}");
        }
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.toughness.()F", &[netherite]), V::F(3.0));
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.knockbackResistance.()F", &[netherite]), V::F(0.1));
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.enchantmentValue.()I", &[netherite]), V::I(15));
        let chain = material(&mut rt, "net/minecraft/world/item/ArmorMaterials.CHAIN");
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.enchantmentValue.()I", &[chain]), V::I(12));
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.toughness.()F", &[chain]), V::F(0.0));
        let sound = api::call(&mut rt, "net/minecraft/world/item/ArmorMaterial.equipSound.()Lnet/minecraft/core/Holder;", &[chain]);
        assert_ne!(sound, V::NULL);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's armour that keeps `ArmorItem`'s `getEquipmentSlot` (Create's cardboard armour) is worn
    /// in the slot its type takes: the game gets that `equippable` slot. It keeps the type and the
    /// material it was made with.
    #[test]
    fn a_mod_armour_item_is_worn_in_the_slot_its_type_takes() {
        fn forward(rt: &mut Rt, key: &'static str, _: &'static str, args: &[V]) -> V {
            api::call(rt, key, args)
        }
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(forward);
        rt.supers.insert("test/CardboardArmorItem", ARMOR_ITEM);
        let material = api::get_static(&mut rt, "net/minecraft/world/item/ArmorMaterials.LEATHER");
        let props = rt.new_object(ITEM_PROPERTIES);
        api::call(&mut rt, "net/minecraft/world/item/Item$Properties.<init>.()V", &[props]);
        for (ty, slot) in [("HELMET", "head"), ("CHESTPLATE", "chest"), ("LEGGINGS", "legs"), ("BOOTS", "feet")] {
            let item = rt.new_object("test/CardboardArmorItem");
            let key = Box::leak(format!("{ARMOR_TYPE}.{ty}").into_boxed_str());
            let ty = api::get_static(&mut rt, key);
            api::call(&mut rt, "net/minecraft/world/item/ArmorItem.<init>.(Lnet/minecraft/core/Holder;Lnet/minecraft/world/item/ArmorItem$Type;Lnet/minecraft/world/item/Item$Properties;)V", &[item, material, ty, props]);
            assert_eq!(equippable(&mut rt, item), Some(format!("\"minecraft:equippable\": {{\"slot\": \"{slot}\"}}")), "{key}");
            assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorItem.getType.()Lnet/minecraft/world/item/ArmorItem$Type;", &[item]), ty);
            assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ArmorItem.getMaterial.()Lnet/minecraft/core/Holder;", &[item]), material);
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_dispenser_behaviour_is_kept_by_its_item() {
        let mut rt = Rt::new();
        let goggles = api_common::item(&mut rt, "create:goggles");
        let behavior = rt.new_object("test/Behavior");
        api::call(&mut rt, "net/minecraft/world/level/block/DispenserBlock.registerBehavior.(Lnet/minecraft/world/level/ItemLike;Lnet/minecraft/core/dispenser/DispenseItemBehavior;)V", &[goggles, behavior]);
        let all = api::get_static(&mut rt, "net/minecraft/world/level/block/DispenserBlock.DISPENSER_REGISTRY");
        assert_eq!(jutil::table_get(&mut rt, all, goggles), Some(behavior));
    }

    #[test]
    fn a_mod_cauldron_answers_clicks_through_its_interaction_map() {
        let mut rt = Rt::new();
        rt.vcall = Some(lambdas);
        let n = rt.string("ruby_breath");
        let map = api::call(&mut rt, "net/minecraft/core/cauldron/CauldronInteraction.newInteractionMap.(Ljava/lang/String;)Lnet/minecraft/core/cauldron/CauldronInteraction$InteractionMap;", &[n]);
        let name = api::call(&mut rt, "net/minecraft/core/cauldron/CauldronInteraction$InteractionMap.name.()Ljava/lang/String;", &[map]);
        assert_eq!(rt.str_of(name), Some("ruby_breath"));
        let items = api::call(&mut rt, "net/minecraft/core/cauldron/CauldronInteraction$InteractionMap.map.()Ljava/util/Map;", &[map]);
        let bucket = api_common::item(&mut rt, "minecraft:bucket");
        let fill = rt.new_object("test/Fill");
        api::call(&mut rt, "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[items, bucket, fill]);
        // Vanilla's maps are one object however a mod reads them; `INTERACTIONS` keeps each by name.
        let empty = api::get_static(&mut rt, "net/minecraft/core/cauldron/CauldronInteraction.EMPTY");
        assert_ne!(empty, V::NULL);
        assert_eq!(api::get_static(&mut rt, "net/minecraft/core/cauldron/CauldronInteraction.EMPTY"), empty);
        let all = api::get_static(&mut rt, "net/minecraft/core/cauldron/CauldronInteraction.INTERACTIONS");
        let key = rt.string("ruby_breath");
        assert_eq!(api::call(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[all, key]), map);
        // `super(properties, map)`: the block's item clicks go through the map.
        let p = props(&mut rt);
        let b = rt.new_object("net/minecraft/world/level/block/AbstractCauldronBlock");
        api::call(&mut rt, "net/minecraft/world/level/block/AbstractCauldronBlock.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;Lnet/minecraft/core/cauldron/CauldronInteraction$InteractionMap;)V", &[b, p, map]);
        let use_on = "net/minecraft/world/level/block/AbstractCauldronBlock.useItemOn.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/phys/BlockHitResult;)Lnet/minecraft/world/ItemInteractionResult;";
        let held = api_common::stack(&mut rt, "minecraft:bucket", 1);
        let r = api::call(&mut rt, use_on, &[b, held, V::NULL, V::NULL, V::NULL, V::NULL, V::NULL, V::NULL]);
        assert_eq!(rt.str_of(r), Some("CONSUME"));
        assert_eq!(rt.get_field(fill, "stack"), held);
        // An item the map doesn't hold passes the click on to the block's own use.
        let stick = api_common::stack(&mut rt, "minecraft:stick", 1);
        let r = api::call(&mut rt, use_on, &[b, stick, V::NULL, V::NULL, V::NULL, V::NULL, V::NULL, V::NULL]);
        assert_eq!(rt.str_of(r), Some("PASS_TO_DEFAULT_BLOCK_INTERACTION"));
    }

    /// `CauldronInteraction.DYED_ITEM`, as a mod files it under its dyeable item and a cauldron calls
    /// it: on the server it washes the colour off and lowers the water a level, to an empty cauldron
    /// after the last; a stack without a colour passes the click on and leaves the water be.
    #[test]
    fn vanillas_dyed_item_interaction_washes_a_dyed_stack() {
        crate::install();
        let mut rt = Rt::new();
        // As a bundle's dispatch does for what isn't the mod's: the runtime answers.
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let mut host = crate::host::MemoryHost::default();
        host.item_tags.insert("minecraft:dyeable".into(), vec!["minecraft:leather_helmet".into()]);
        host.blocks.insert([3, 64, -2], "minecraft:water_cauldron[level=2]".into());
        const SET: &str = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        const HAS: &str = "net/minecraft/world/item/ItemStack.has.(Lnet/minecraft/core/component/DataComponentType;)Z";
        rt.with_host(&mut host, |rt| {
            let wash = api::get_static(rt, "net/minecraft/core/cauldron/CauldronInteraction.DYED_ITEM");
            assert_eq!(api::get_static(rt, "net/minecraft/core/cauldron/CauldronInteraction.DYED_ITEM"), wash, "one object, as vanilla's constant");
            let dyed = api::get_static(rt, "net/minecraft/core/component/DataComponents.DYED_COLOR");
            let helmet = api_common::stack(rt, "minecraft:leather_helmet", 1);
            let dye = |rt: &mut Rt, stack: V| {
                let color = rt.new_object("net/minecraft/world/item/component/DyedItemColor");
                api::call(rt, "net/minecraft/world/item/component/DyedItemColor.<init>.(IZ)V", &[color, V::I(0x3355ff), V::I(1)]);
                api::call(rt, SET, &[stack, dyed, color]);
            };
            dye(rt, helmet);
            let dyeable = api::get_static(rt, "net/minecraft/tags/ItemTags.DYEABLE");
            assert_eq!(api::call(rt, "net/minecraft/world/item/ItemStack.is.(Lnet/minecraft/tags/TagKey;)Z", &[helmet, dyeable]), V::I(1), "{:?}", rt.str_of(rt.get_field(dyeable, "$location")));
            assert_eq!(api::call(rt, HAS, &[helmet, dyed]), V::I(1));
            let level = rt.new_object("net/minecraft/server/level/ServerLevel");
            let pos = api::block_pos(rt, 3, 64, -2);
            let click = |rt: &mut Rt, state: &str, stack: V| {
                let s = api::state(rt, state);
                let r = rt.call_functional(INTERACT, wash, &[s, level, pos, V::NULL, V::NULL, stack]);
                rt.str_of(r).map(str::to_owned)
            };
            assert_eq!(click(rt, "minecraft:water_cauldron[level=2]", helmet).as_deref(), Some("CONSUME"));
            assert_eq!(api::call(rt, HAS, &[helmet, dyed]), V::I(0), "the colour is washed off");
            assert_eq!(rt.host().map(|h| h.block_state([3, 64, -2])).as_deref(), Some("minecraft:water_cauldron[level=1]"));
            assert_eq!(click(rt, "minecraft:water_cauldron[level=1]", helmet).as_deref(), Some("PASS_TO_DEFAULT_BLOCK_INTERACTION"));
            assert_eq!(rt.host().map(|h| h.block_state([3, 64, -2])).as_deref(), Some("minecraft:water_cauldron[level=1]"));
            dye(rt, helmet);
            assert_eq!(click(rt, "minecraft:water_cauldron[level=1]", helmet).as_deref(), Some("CONSUME"));
            assert_eq!(rt.host().map(|h| h.block_state([3, 64, -2])).as_deref(), Some("minecraft:cauldron"));
            // A dyed stack that isn't dyeable is not vanilla's to wash.
            let wool = api_common::stack(rt, "minecraft:white_wool", 1);
            dye(rt, wool);
            assert_eq!(click(rt, "minecraft:water_cauldron[level=3]", wool).as_deref(), Some("PASS_TO_DEFAULT_BLOCK_INTERACTION"));
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
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
        host.facts.insert("minecraft:oak_planks".into(), BlockFacts { hardness: 2.0, push: 0, replaceable: false, sturdy: (1 << 18) - 1 });
        host.facts.insert("minecraft:obsidian".into(), BlockFacts { hardness: 50.0, push: 2, replaceable: false, sturdy: (1 << 18) - 1 });
        host.facts.insert("minecraft:short_grass".into(), BlockFacts { hardness: 0.0, push: 1, replaceable: true, sturdy: 0 });
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

    /// `AxeItem.createAttributes(Tiers.IRON, 6, -3.1)` is `DiggerItem`'s (Aquaculture's axes):
    /// iron's 2 added to the damage, the speed as given.
    #[test]
    fn a_tools_attributes_add_its_tiers_damage() {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let iron = api::get_static(&mut rt, "net/minecraft/world/item/Tiers.IRON");
        let m = api::call(
            &mut rt,
            "net/minecraft/world/item/AxeItem.createAttributes.(Lnet/minecraft/world/item/Tier;FF)Lnet/minecraft/world/item/component/ItemAttributeModifiers;",
            &[iron, V::F(6.0), V::F(-3.1)],
        );
        assert_eq!(rt.get_field(m, "$attackDamage"), V::D(8.0));
        assert_eq!(rt.get_field(m, "$attackSpeed"), V::D(f64::from(-3.1f32)));
    }

    /// NeoForge's `DyeColor.getColor(stack)`, which Create's toolbox asks of what it is clicked with
    /// before it dyes itself: a dye's colour, a stack in a `c:dyes/<colour>` tag that colour, null
    /// for anything else (the toolbox then opens).
    #[test]
    fn a_stack_has_a_dye_colour_only_when_it_is_a_dye() {
        const GET_COLOR: &str = "net/minecraft/world/item/DyeColor.getColor.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/world/item/DyeColor;";
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        host.item_tags.insert("c:dyes/blue".into(), vec!["test:lapis_dust".into()]);
        let (red, blue) = (api::get_static(&mut rt, "net/minecraft/world/item/DyeColor.RED"), api::get_static(&mut rt, "net/minecraft/world/item/DyeColor.BLUE"));
        let got = rt.with_host(&mut host, |rt| {
            ["minecraft:red_dye", "test:lapis_dust", "minecraft:iron_pickaxe"].map(|item| {
                let s = api_common::stack(rt, item, 1);
                api::call(rt, GET_COLOR, &[s])
            })
        });
        assert_eq!(got, [red, blue, V::NULL]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's block definition answers its properties by name (`getProperty("type")`, null for one
    /// it lacks) and a state each property's value (`getValues`); `Block.byItem` finds the block a
    /// block item places, air for another item; a mod block's `super` calls reach vanilla's.
    #[test]
    fn a_definition_names_its_properties_and_items_find_their_blocks() {
        let mut rt = Rt::new();
        rt.vcall = Some(lambdas);
        let p = props(&mut rt);
        let slab = rt.new_object("net/minecraft/world/level/block/SlabBlock");
        api::call(
            &mut rt,
            "net/minecraft/world/level/block/SlabBlock.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V",
            &[slab, p],
        );
        rt.registered
            .insert(("block".into(), "ruby:slab".into()), slab);
        let def = api::call(
            &mut rt,
            "net/minecraft/world/level/block/Block.getStateDefinition.()Lnet/minecraft/world/level/block/state/StateDefinition;",
            &[slab],
        );
        let by_name = |rt: &mut Rt, n: &str| {
            let n = rt.string(n);
            api::call(
                rt,
                "net/minecraft/world/level/block/state/StateDefinition.getProperty.(Ljava/lang/String;)Lnet/minecraft/world/level/block/state/properties/Property;",
                &[def, n],
            )
        };
        let (kind, wet) = (by_name(&mut rt, "type"), by_name(&mut rt, "waterlogged"));
        assert_eq!(api_common::prop_parts(&rt, kind).0, "type");
        assert_eq!(api_common::prop_parts(&rt, wet).0, "waterlogged");
        assert_eq!(by_name(&mut rt, "facing"), V::NULL);
        let all = api::call(
            &mut rt,
            "net/minecraft/world/level/block/state/StateDefinition.getProperties.()Ljava/util/Collection;",
            &[def],
        );
        assert_eq!(jutil::elements(&rt, all), [kind, wet]);
        // The same objects as a state's `getValue` reads by.
        let top = api::state(&mut rt, "ruby:slab[type=top,waterlogged=true]");
        let values = api::call(
            &mut rt,
            "net/minecraft/world/level/block/state/BlockState.getValues.()Ljava/util/Map;",
            &[top],
        );
        let get = "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;";
        let (is_kind, is_wet) = (
            api::call(&mut rt, get, &[values, kind]),
            api::call(&mut rt, get, &[values, wet]),
        );
        let read = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
        assert_eq!(is_kind, api::call(&mut rt, read, &[top, kind]));
        assert_eq!(is_wet, api_common::boolean(&mut rt, true));
        assert_eq!(
            api::call(&mut rt, "java/util/Map.size.()I", &[values]),
            V::I(2)
        );

        let item_props = rt.new_object("net/minecraft/world/item/Item$Properties");
        let slab_item = rt.new_object("net/minecraft/world/item/BlockItem");
        api::call(
            &mut rt,
            "net/minecraft/world/item/BlockItem.<init>.(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/world/item/Item$Properties;)V",
            &[slab_item, slab, item_props],
        );
        let by_item = |rt: &mut Rt, item: V| {
            let b = api::call(
                rt,
                "net/minecraft/world/level/block/Block.byItem.(Lnet/minecraft/world/item/Item;)Lnet/minecraft/world/level/block/Block;",
                &[item],
            );
            api::block_name(rt, b)
        };
        assert_eq!(by_item(&mut rt, slab_item), "ruby:slab");
        let carrot = api_common::item(&mut rt, "minecraft:carrot");
        assert_eq!(
            by_item(&mut rt, carrot),
            "minecraft:carrots",
            "a crop's seed item places the crop"
        );
        let diamond = api_common::item(&mut rt, "minecraft:diamond");
        assert_eq!(by_item(&mut rt, diamond), "minecraft:air");

        // `super.updateShape` keeps the state where no vanilla class between overrides it, and
        // `super.setPlacedBy` leaves the placing to the game.
        let (dir, level, pos) = (
            api::direction(&mut rt, 2),
            crate::glue::level(&mut rt),
            api::block_pos(&mut rt, 0, 64, 0),
        );
        let air = api::state(&mut rt, "minecraft:air");
        for owner in [
            "net/minecraft/world/level/block/Block",
            "net/minecraft/world/level/block/HorizontalDirectionalBlock",
            "net/minecraft/world/level/block/BaseEntityBlock",
        ] {
            assert_eq!(
                update_shape_declarer(owner),
                Some("state/BlockBehaviour"),
                "{owner}"
            );
        }
        assert_eq!(
            update_shape_declarer("net/minecraft/world/level/block/WeatheringCopperStairBlock"),
            Some("StairBlock")
        );
        assert_eq!(
            update_shape_declarer("net/minecraft/world/level/block/ButtonBlock"),
            Some("FaceAttachedHorizontalDirectionalBlock")
        );
        let kept = api::call(
            &mut rt,
            "net/minecraft/world/level/block/HorizontalDirectionalBlock.updateShape.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;",
            &[slab, top, dir, air, level, pos, pos],
        );
        assert_eq!(kept, top);
        let stack = rt.new_object("net/minecraft/world/item/ItemStack");
        let placed = api::call(
            &mut rt,
            "net/minecraft/world/level/block/BaseEntityBlock.setPlacedBy.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;)V",
            &[slab, level, pos, top, V::NULL, stack],
        );
        assert_eq!(placed, V::Void);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod block extending a vanilla class with a shape rule of its own (`FenceBlock`) gets the
    /// game's rule for its `super.updateShape`: the host is asked with that class and those above
    /// it, nearest first, up to the declaring one, and the mod's code goes on with its answer.
    /// Where the host runs no rule (outside a shape update), the state stays as it is.
    #[test]
    fn a_super_update_shape_into_a_vanilla_class_is_the_games_rule() {
        const FENCE: &str = "test/RubyFence";
        fn overrides(class: &str, short: &str) -> bool {
            class == FENCE && short == api::UPDATE_SHAPE
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                // `return super.updateShape(state, direction, neighbour, level, pos, neighbourPos);`
                api::UPDATE_SHAPE if rt.class_of(args[0]) == FENCE => {
                    let key = rt.static_key(&format!(
                        "net/minecraft/world/level/block/FenceBlock.{}",
                        api::UPDATE_SHAPE
                    ));
                    api::call(rt, key, args)
                }
                _ => api::call(rt, key, args),
            }
        }
        type Asked = (Vec<String>, [i32; 3], String, usize, [i32; 3], String);
        /// The game, whose fence rule joins stone.
        #[derive(Default)]
        struct Game {
            asked: Vec<Asked>,
        }
        impl crate::host::Host for Game {
            fn block_state(&self, _pos: [i32; 3]) -> String {
                "minecraft:air".into()
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
            fn vanilla_update_shape(
                &mut self,
                classes: &[&str],
                pos: [i32; 3],
                state: &str,
                direction: usize,
                neighbour_pos: [i32; 3],
                neighbour: &str,
            ) -> Option<String> {
                let names = classes.iter().map(|c| (*c).to_owned()).collect();
                self.asked.push((
                    names,
                    pos,
                    state.to_owned(),
                    direction,
                    neighbour_pos,
                    neighbour.to_owned(),
                ));
                (classes.first() == Some(&"FenceBlock") && neighbour == "minecraft:stone")
                    .then(|| state.replace("east=false", "east=true"))
            }
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers
            .insert(FENCE, "net/minecraft/world/level/block/FenceBlock");
        let fence = rt.new_object(FENCE);
        rt.registered
            .insert(("block".into(), "test:ruby_fence".into()), fence);
        let state =
            "test:ruby_fence[east=false,north=false,south=false,waterlogged=false,west=false]";
        let joined =
            "test:ruby_fence[east=true,north=false,south=false,waterlogged=false,west=false]";
        let mut game = Game::default();
        let east = 5;
        let new = crate::glue::update_shape(
            &mut rt,
            &mut game,
            [0, 64, 0],
            state,
            east,
            [1, 64, 0],
            "minecraft:stone",
        );
        assert_eq!(new.as_deref(), Some(joined));
        let asked: Vec<Asked> = vec![(
            vec!["FenceBlock".into()],
            [0, 64, 0],
            state.into(),
            east,
            [1, 64, 0],
            "minecraft:stone".into(),
        )];
        assert_eq!(game.asked, asked);
        let mut memory = crate::host::MemoryHost::default();
        let kept = crate::glue::update_shape(
            &mut rt,
            &mut memory,
            [0, 64, 0],
            state,
            east,
            [1, 64, 0],
            "minecraft:stone",
        );
        assert_eq!(kept.as_deref(), Some(state));
        // The classes asked: a class's own name first (the game may know it), then its ancestors.
        assert_eq!(
            update_shape_ancestry("net/minecraft/world/level/block/WeatheringCopperStairBlock"),
            ["WeatheringCopperStairBlock", "StairBlock"]
        );
        assert_eq!(
            update_shape_ancestry("net/minecraft/world/level/block/ButtonBlock"),
            ["ButtonBlock", "FaceAttachedHorizontalDirectionalBlock"]
        );
        assert_eq!(
            update_shape_ancestry("net/minecraft/world/level/block/piston/PistonHeadBlock"),
            ["PistonHeadBlock"]
        );
        assert!(
            update_shape_ancestry("net/minecraft/world/level/block/HorizontalDirectionalBlock")
                .is_empty()
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create's metal ladder survives by its `super.canSurvive` (`LadderBlock`'s: a wall behind it)
    /// or a rung of its own above it. The host runs `LadderBlock`'s rule, asked with the class; a
    /// `super.canSurvive` that reaches `BlockBehaviour`'s is true without asking; a host that runs
    /// no rule leaves the block standing.
    #[test]
    fn a_super_can_survive_into_a_vanilla_class_is_the_games_rule() {
        const LADDER: &str = "test/MetalLadderBlock";
        const PEDESTAL: &str = "test/PedestalBlock";
        fn overrides(class: &str, short: &str) -> bool {
            (class == LADDER || class == PEDESTAL) && short == api::CAN_SURVIVE
        }
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            match short {
                // `super.canSurvive(state, level, pos) || level.getBlockState(pos.above())` the same rung.
                api::CAN_SURVIVE if rt.class_of(args[0]) == LADDER => {
                    let key = rt.static_key(&format!(
                        "net/minecraft/world/level/block/LadderBlock.{}",
                        api::CAN_SURVIVE
                    ));
                    if api::call(rt, key, args).i() != 0 {
                        return V::I(1);
                    }
                    let above = api::call(
                        rt,
                        "net/minecraft/core/BlockPos.above.()Lnet/minecraft/core/BlockPos;",
                        &[args[3]],
                    );
                    let get = "net/minecraft/world/level/LevelReader.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
                    let state = api::call(rt, get, &[args[2], above]);
                    V::I(
                        (api_common::text_of(rt, state) == api_common::text_of(rt, args[1])) as i32,
                    )
                }
                // `return super.canSurvive(state, level, pos);`
                api::CAN_SURVIVE if rt.class_of(args[0]) == PEDESTAL => {
                    let key = rt.static_key(&format!(
                        "net/minecraft/world/level/block/Block.{}",
                        api::CAN_SURVIVE
                    ));
                    api::call(rt, key, args)
                }
                _ => api::call(rt, key, args),
            }
        }
        /// The game, whose ladder rule wants stone south of a rung facing north.
        #[derive(Default)]
        struct Game {
            blocks: std::collections::BTreeMap<[i32; 3], String>,
            asked: std::cell::RefCell<Vec<Vec<String>>>,
        }
        impl crate::host::Host for Game {
            fn block_state(&self, pos: [i32; 3]) -> String {
                self.blocks
                    .get(&pos)
                    .cloned()
                    .unwrap_or_else(|| "minecraft:air".into())
            }
            fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
                false
            }
            fn vanilla_can_survive(
                &self,
                classes: &[&str],
                pos: [i32; 3],
                _state: &str,
            ) -> Option<bool> {
                self.asked
                    .borrow_mut()
                    .push(classes.iter().map(|c| (*c).to_owned()).collect());
                (classes.first() == Some(&"LadderBlock"))
                    .then(|| self.block_state([pos[0], pos[1], pos[2] + 1]) == "minecraft:stone")
            }
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers
            .insert(LADDER, "net/minecraft/world/level/block/LadderBlock");
        rt.supers
            .insert(PEDESTAL, "net/minecraft/world/level/block/Block");
        for (class, id) in [(LADDER, "test:metal_ladder"), (PEDESTAL, "test:pedestal")] {
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), id.into()), b);
        }
        let rung = "test:metal_ladder[facing=north,waterlogged=false]";
        let mut game = Game::default();
        game.blocks.insert([0, 1, 1], "minecraft:stone".into());
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut game, [0, 1, 0], rung),
            Some(true),
            "on its wall"
        );
        assert_eq!(*game.asked.borrow(), [vec!["LadderBlock".to_owned()]]);
        game.blocks.clear();
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut game, [0, 1, 0], rung),
            Some(false),
            "no wall, nothing above"
        );
        game.blocks.insert([0, 2, 0], rung.into());
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut game, [0, 1, 0], rung),
            Some(true),
            "hung from the rung above"
        );
        // `BlockBehaviour`'s is true, and the host isn't asked.
        game.asked.borrow_mut().clear();
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut game, [5, 1, 5], "test:pedestal"),
            Some(true)
        );
        assert!(game.asked.borrow().is_empty());
        // A host that runs no rule leaves the block standing.
        let mut memory = crate::host::MemoryHost::default();
        assert_eq!(
            crate::glue::can_survive(&mut rt, &mut memory, [0, 1, 0], rung),
            Some(true)
        );
        // The classes asked: a class, then those above it up to the one declaring the rule.
        assert_eq!(
            can_survive_ancestry("net/minecraft/world/level/block/TorchBlock"),
            ["TorchBlock", "BaseTorchBlock"]
        );
        assert_eq!(
            can_survive_ancestry("net/minecraft/world/level/block/FlowerBlock"),
            ["FlowerBlock", "BushBlock"]
        );
        assert!(can_survive_ancestry("net/minecraft/world/level/block/StairBlock").is_empty());
        assert_eq!(
            can_survive_declarer("net/minecraft/world/level/block/StairBlock"),
            Some("state/BlockBehaviour")
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
