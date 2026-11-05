//! Minecraft 26.3's `BlockTags` and `ItemTags` constants and the tags they name (under
//! `minecraft:`), read from the 26.3 jar with `javap -c`: each constant's `create`/`bind` string, or
//! for one taken from `BlockItemTags`, that shared id's block or item half (`BlockTags.STANDING_SIGNS`
//! is `minecraft:standing_signs`, `ItemTags.SIGNS` `minecraft:signs`).

/// `BlockTags`: (field, tag).
const BLOCK: &[(&str, &str)] = &[
    ("WOOL", "wool"), ("PLANKS", "planks"), ("STONE_BRICKS", "stone_bricks"),
    ("WOODEN_BUTTONS", "wooden_buttons"), ("STONE_BUTTONS", "stone_buttons"),
    ("BUTTONS", "buttons"), ("WOOL_CARPETS", "wool_carpets"), ("WOODEN_DOORS", "wooden_doors"),
    ("WOODEN_STAIRS", "wooden_stairs"), ("WOODEN_SLABS", "wooden_slabs"),
    ("WOODEN_FENCES", "wooden_fences"), ("FENCE_GATES", "fence_gates"),
    ("WOODEN_PRESSURE_PLATES", "wooden_pressure_plates"), ("WOODEN_SHELVES", "wooden_shelves"),
    ("DOORS", "doors"), ("SAPLINGS", "saplings"), ("BAMBOO_BLOCKS", "bamboo_blocks"),
    ("PALE_OAK_LOGS", "pale_oak_logs"), ("JUNGLE_LOGS", "jungle_logs"),
    ("WART_BLOCKS", "wart_blocks"), ("LOGS", "logs"), ("SAND", "sand"), ("SLABS", "slabs"),
    ("WALLS", "walls"), ("STAIRS", "stairs"), ("ANVIL", "anvil"), ("RAILS", "rails"),
    ("LEAVES", "leaves"), ("WOODEN_TRAPDOORS", "wooden_trapdoors"), ("TRAPDOORS", "trapdoors"),
    ("SMALL_FLOWERS", "small_flowers"), ("FLOWERS", "flowers"), ("BEDS", "beds"),
    ("FENCES", "fences"), ("SOUL_FIRE_BASE_BLOCKS", "soul_fire_base_blocks"),
    ("CANDLES", "candles"), ("DAMPENS_VIBRATIONS", "dampens_vibrations"),
    ("GOLD_ORES", "gold_ores"), ("IRON_ORES", "iron_ores"), ("COPPER_ORES", "copper_ores"),
    ("ORES", "ores"), ("DIRT", "dirt"), ("MUD", "mud"), ("MOSS_BLOCKS", "moss_blocks"),
    ("GRASS_BLOCKS", "grass_blocks"), ("TERRACOTTA", "terracotta"),
    ("GLAZED_TERRACOTTA", "glazed_terracotta"), ("CONCRETE", "concrete"),
    ("CONCRETE_SLABS", "concrete_slabs"), ("CONCRETE_STAIRS", "concrete_stairs"),
    ("CONCRETE_POWDERS", "concrete_powders"),
    ("COMPLETES_FIND_TREE_TUTORIAL", "completes_find_tree_tutorial"),
    ("SHULKER_BOXES", "shulker_boxes"), ("COPPER_CHESTS", "copper_chests"),
    ("LIGHTNING_RODS", "lightning_rods"), ("COPPER", "copper"), ("CHAINS", "chains"),
    ("COPPER_GOLEM_STATUES", "copper_golem_statues"), ("LANTERNS", "lanterns"), ("BARS", "bars"),
    ("SKULLS", "skulls"), ("WOOL_SLABS", "wool_slabs"), ("WOOL_STAIRS", "wool_stairs"),
    ("CEILING_HANGING_SIGNS", "ceiling_hanging_signs"), ("STANDING_SIGNS", "standing_signs"),
    ("BEE_ATTRACTIVE", "bee_attractive"), ("MOB_INTERACTABLE_DOORS", "mob_interactable_doors"),
    ("PRESSURE_PLATES", "pressure_plates"), ("STONE_PRESSURE_PLATES", "stone_pressure_plates"),
    ("OVERWORLD_NATURAL_LOGS", "overworld_natural_logs"), ("BANNERS", "banners"),
    ("PIGLIN_REPELLENTS", "piglin_repellents"), ("BADLANDS_TERRACOTTA", "badlands_terracotta"),
    ("FLOWER_POTS", "flower_pots"), ("ENDERMAN_HOLDABLE", "enderman_holdable"), ("ICE", "ice"),
    ("VALID_SPAWN", "valid_spawn"), ("IMPERMEABLE", "impermeable"),
    ("UNDERWATER_BONEMEALS", "underwater_bonemeals"), ("CORAL_BLOCKS", "coral_blocks"),
    ("WALL_CORALS", "wall_corals"), ("CORAL_PLANTS", "coral_plants"), ("CORALS", "corals"),
    ("WALL_SIGNS", "wall_signs"), ("SIGNS", "signs"), ("WALL_HANGING_SIGNS", "wall_hanging_signs"),
    ("ALL_HANGING_SIGNS", "all_hanging_signs"), ("ALL_SIGNS", "all_signs"),
    ("DRAGON_IMMUNE", "dragon_immune"), ("DRAGON_TRANSPARENT", "dragon_transparent"),
    ("WITHER_IMMUNE", "wither_immune"), ("WITHER_SUMMON_BASE_BLOCKS", "wither_summon_base_blocks"),
    ("BEEHIVES", "beehives"), ("CROPS", "crops"), ("BEE_GROWABLES", "bee_growables"),
    ("PORTALS", "portals"), ("FIRE", "fire"), ("NYLIUM", "nylium"),
    ("BEACON_BASE_BLOCKS", "beacon_base_blocks"), ("SOUL_SPEED_BLOCKS", "soul_speed_blocks"),
    ("WALL_POST_OVERRIDE", "wall_post_override"), ("CLIMBABLE", "climbable"),
    ("FALL_DAMAGE_RESETTING", "fall_damage_resetting"), ("HOGLIN_REPELLENTS", "hoglin_repellents"),
    ("STRIDER_WARM_BLOCKS", "strider_warm_blocks"), ("CAMPFIRES", "campfires"),
    ("GUARDED_BY_PIGLINS", "guarded_by_piglins"),
    ("PREVENT_MOB_SPAWNING_INSIDE", "prevent_mob_spawning_inside"),
    ("UNSTABLE_BOTTOM_CENTER", "unstable_bottom_center"), ("EDIBLE_FOR_SHEEP", "edible_for_sheep"),
    ("CAN_GLIDE_THROUGH", "can_glide_through"),
    ("CAUSES_PERIODIC_GEYSER_ERUPTIONS", "causes_periodic_geyser_eruptions"),
    ("CAUSES_CONTINUOUS_GEYSER_ERUPTIONS", "causes_continuous_geyser_eruptions"),
    ("NETHER_PORTAL_FRAME", "nether_portal_frame"),
    ("CONDUIT_EFFECT_BLOCK", "conduit_effect_block"),
    ("INFINIBURN_OVERWORLD", "infiniburn_overworld"), ("INFINIBURN_NETHER", "infiniburn_nether"),
    ("INFINIBURN_END", "infiniburn_end"), ("SUBSTRATE_OVERWORLD", "substrate_overworld"),
    ("BASE_STONE_OVERWORLD", "base_stone_overworld"),
    ("STONE_ORE_REPLACEABLES", "stone_ore_replaceables"),
    ("HEIGHT_SPECIFIC_ORE_REPLACEABLES", "height_specific_ore_replaceables"),
    ("DEEPSLATE_ORE_REPLACEABLES", "deepslate_ore_replaceables"),
    ("BASE_STONE_NETHER", "base_stone_nether"),
    ("BENEATH_TREE_PODZOL_REPLACEABLE", "beneath_tree_podzol_replaceable"),
    ("BENEATH_BAMBOO_PODZOL_REPLACEABLE", "beneath_bamboo_podzol_replaceable"),
    ("CANNOT_REPLACE_BELOW_TREE_TRUNK", "cannot_replace_below_tree_trunk"),
    ("CANNOT_PLACE_BASALT_PILLAR_ON", "cannot_place_basalt_pillar_on"),
    ("UNCARVABLE", "uncarvable"), ("CANDLE_CAKES", "candle_cakes"), ("CAULDRONS", "cauldrons"),
    ("CRYSTAL_SOUND_BLOCKS", "crystal_sound_blocks"),
    ("INSIDE_STEP_SOUND_BLOCKS", "inside_step_sound_blocks"),
    ("COMBINATION_STEP_SOUND_BLOCKS", "combination_step_sound_blocks"),
    ("CAMEL_SAND_STEP_SOUND_BLOCKS", "camel_sand_step_sound_blocks"),
    ("HAPPY_GHAST_AVOIDS", "happy_ghast_avoids"),
    ("OCCLUDES_VIBRATION_SIGNALS", "occludes_vibration_signals"),
    ("CUSHION_USES_COLLISION_SHAPE", "cushion_uses_collision_shape"),
    ("DRIPSTONE_REPLACEABLE", "dripstone_replaceable_blocks"),
    ("SULFUR_SPIKE_REPLACEABLE", "sulfur_spike_replaceable_blocks"), ("CAVE_VINES", "cave_vines"),
    ("MOSS_REPLACEABLE", "moss_replaceable"),
    ("LUSH_GROUND_REPLACEABLE", "lush_ground_replaceable"),
    ("AZALEA_ROOT_REPLACEABLE", "azalea_root_replaceable"),
    ("ICE_SPIKE_REPLACEABLE", "ice_spike_replaceable"),
    ("FOREST_ROCK_CAN_PLACE_ON", "forest_rock_can_place_on"),
    ("HUGE_BROWN_MUSHROOM_CAN_PLACE_ON", "huge_brown_mushroom_can_place_on"),
    ("HUGE_RED_MUSHROOM_CAN_PLACE_ON", "huge_red_mushroom_can_place_on"), ("SNOW", "snow"),
    ("MINEABLE_WITH_AXE", "mineable/axe"), ("MINEABLE_WITH_HOE", "mineable/hoe"),
    ("MINEABLE_WITH_PICKAXE", "mineable/pickaxe"), ("MINEABLE_WITH_SHOVEL", "mineable/shovel"),
    ("SWORD_EFFICIENT", "sword_efficient"), ("SWORD_INSTANTLY_MINES", "sword_instantly_mines"),
    ("SHEARS_EXTREME_BREAKING_SPEED", "shears_extreme_breaking_speed"),
    ("SHEARS_MAJOR_BREAKING_SPEED", "shears_major_breaking_speed"),
    ("SHEARS_MINOR_BREAKING_SPEED", "shears_minor_breaking_speed"),
    ("TURNS_INTO_DIRT_PATH", "turns_into_dirt_path"),
    ("TURNS_INTO_FARMLAND", "turns_into_farmland"), ("NEEDS_DIAMOND_TOOL", "needs_diamond_tool"),
    ("NEEDS_IRON_TOOL", "needs_iron_tool"), ("NEEDS_STONE_TOOL", "needs_stone_tool"),
    ("INCORRECT_FOR_NETHERITE_TOOL", "incorrect_for_netherite_tool"),
    ("INCORRECT_FOR_DIAMOND_TOOL", "incorrect_for_diamond_tool"),
    ("INCORRECT_FOR_IRON_TOOL", "incorrect_for_iron_tool"),
    ("INCORRECT_FOR_COPPER_TOOL", "incorrect_for_copper_tool"),
    ("INCORRECT_FOR_STONE_TOOL", "incorrect_for_stone_tool"),
    ("INCORRECT_FOR_GOLD_TOOL", "incorrect_for_gold_tool"),
    ("INCORRECT_FOR_WOODEN_TOOL", "incorrect_for_wooden_tool"),
    ("FEATURES_CANNOT_REPLACE", "features_cannot_replace"),
    ("LAVA_POOL_STONE_CANNOT_REPLACE", "lava_pool_stone_cannot_replace"),
    ("GEODE_INVALID_BLOCKS", "geode_invalid_blocks"),
    ("FROG_PREFER_JUMP_TO", "frog_prefer_jump_to"),
    ("SCULK_GROWTH_INHIBITORS", "sculk_growth_inhibitors"),
    ("SCULK_REPLACEABLE", "sculk_replaceable"),
    ("SCULK_REPLACEABLE_WORLD_GEN", "sculk_replaceable_world_gen"),
    ("ANCIENT_CITY_REPLACEABLE", "ancient_city_replaceable"),
    ("VIBRATION_RESONATORS", "vibration_resonators"),
    ("ANIMALS_SPAWNABLE_ON", "animals_spawnable_on"),
    ("ARMADILLO_SPAWNABLE_ON", "armadillo_spawnable_on"),
    ("AXOLOTLS_SPAWNABLE_ON", "axolotls_spawnable_on"),
    ("GOATS_SPAWNABLE_ON", "goats_spawnable_on"),
    ("MOOSHROOMS_SPAWNABLE_ON", "mooshrooms_spawnable_on"),
    ("PARROTS_SPAWNABLE_ON", "parrots_spawnable_on"),
    ("POLAR_BEARS_SPAWNABLE_ON_ALTERNATE", "polar_bears_spawnable_on_alternate"),
    ("RABBITS_SPAWNABLE_ON", "rabbits_spawnable_on"), ("FOXES_SPAWNABLE_ON", "foxes_spawnable_on"),
    ("WOLVES_SPAWNABLE_ON", "wolves_spawnable_on"), ("FROGS_SPAWNABLE_ON", "frogs_spawnable_on"),
    ("BATS_SPAWNABLE_ON", "bats_spawnable_on"), ("CAMELS_SPAWNABLE_ON", "camels_spawnable_on"),
    ("AZALEA_GROWS_ON", "azalea_grows_on"), ("CONVERTIBLE_TO_MUD", "convertible_to_mud"),
    ("MANGROVE_LOGS_CAN_GROW_THROUGH", "mangrove_logs_can_grow_through"),
    ("MANGROVE_ROOTS_CAN_GROW_THROUGH", "mangrove_roots_can_grow_through"),
    ("SNAPS_GOAT_HORN", "snaps_goat_horn"), ("REPLACEABLE_BY_TREES", "replaceable_by_trees"),
    ("REPLACEABLE_BY_MUSHROOMS", "replaceable_by_mushrooms"),
    ("ENABLES_BUBBLE_COLUMN_DRAG_DOWN", "enables_bubble_column_drag_down"),
    ("ENABLES_BUBBLE_COLUMN_PUSH_UP", "enables_bubble_column_push_up"),
    ("CATS_CAN_SIT_ON", "cats_can_sit_on"), ("CATS_CAN_LIE_ON", "cats_can_lie_on"),
    ("SPEEDS_UP_ZOMBIE_VILLAGER_CURING", "speeds_up_zombie_villager_curing"),
    ("VILLAGERS_CAN_SLEEP_ON_BED", "villagers_can_sleep_on_bed"),
    ("VILLAGER_BABIES_CAN_JUMP_ON_BED", "villager_babies_can_jump_on_bed"),
    ("SUPPORTS_VEGETATION", "supports_vegetation"),
    ("SUPPORTS_DRY_VEGETATION", "supports_dry_vegetation"), ("SUPPORTS_CROPS", "supports_crops"),
    ("SUPPORTS_STEM_CROPS", "supports_stem_crops"), ("SUPPORTS_STEM_FRUIT", "supports_stem_fruit"),
    ("SUPPORTS_PUMPKIN_STEM", "supports_pumpkin_stem"),
    ("SUPPORTS_MELON_STEM", "supports_melon_stem"),
    ("SUPPORTS_PUMPKIN_STEM_FRUIT", "supports_pumpkin_stem_fruit"),
    ("SUPPORTS_MELON_STEM_FRUIT", "supports_melon_stem_fruit"),
    ("SUPPORTS_SUGAR_CANE", "supports_sugar_cane"),
    ("SUPPORTS_SUGAR_CANE_ADJACENTLY", "supports_sugar_cane_adjacently"),
    ("SUPPORTS_BAMBOO", "supports_bamboo"), ("SUPPORTS_SMALL_DRIPLEAF", "supports_small_dripleaf"),
    ("SUPPORTS_BIG_DRIPLEAF", "supports_big_dripleaf"), ("SUPPORTS_CACTUS", "supports_cactus"),
    ("SUPPORTS_CHORUS_PLANT", "supports_chorus_plant"),
    ("SUPPORTS_CHORUS_FLOWER", "supports_chorus_flower"),
    ("SUPPORTS_NETHER_SPROUTS", "supports_nether_sprouts"), ("SUPPORTS_AZALEA", "supports_azalea"),
    ("SUPPORTS_WARPED_FUNGUS", "supports_warped_fungus"),
    ("SUPPORTS_CRIMSON_FUNGUS", "supports_crimson_fungus"),
    ("SUPPORTS_MANGROVE_PROPAGULE", "supports_mangrove_propagule"),
    ("SUPPORTS_HANGING_MANGROVE_PROPAGULE", "supports_hanging_mangrove_propagule"),
    ("SUPPORTS_NETHER_WART", "supports_nether_wart"),
    ("SUPPORTS_CRIMSON_ROOTS", "supports_crimson_roots"),
    ("SUPPORTS_WARPED_ROOTS", "supports_warped_roots"),
    ("SUPPORTS_WITHER_ROSE", "supports_wither_rose"), ("SUPPORTS_COCOA", "supports_cocoa"),
    ("SUPPORTS_LILY_PAD", "supports_lily_pad"), ("SUPPORTS_FROGSPAWN", "supports_frogspawn"),
    ("PREVENTS_NEARBY_LEAF_DECAY", "prevents_nearby_leaf_decay"),
    ("SUPPORT_OVERRIDE_CACTUS_FLOWER", "support_override_cactus_flower"),
    ("SUPPORT_OVERRIDE_SNOW_LAYER", "support_override_snow_layer"),
    ("CANNOT_SUPPORT_SNOW_LAYER", "cannot_support_snow_layer"),
    ("CANNOT_SUPPORT_SEAGRASS", "cannot_support_seagrass"),
    ("CANNOT_SUPPORT_KELP", "cannot_support_kelp"),
    ("OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT", "overrides_mushroom_light_requirement"),
    ("GROWS_CROPS", "grows_crops"), ("INVALID_SPAWN_INSIDE", "invalid_spawn_inside"),
    ("SNIFFER_DIGGABLE_BLOCK", "sniffer_diggable_block"),
    ("SNIFFER_EGG_HATCH_BOOST", "sniffer_egg_hatch_boost"),
    ("TRAIL_RUINS_REPLACEABLE", "trail_ruins_replaceable"), ("REPLACEABLE", "replaceable"),
    ("ENCHANTMENT_POWER_PROVIDER", "enchantment_power_provider"),
    ("ENCHANTMENT_POWER_TRANSMITTER", "enchantment_power_transmitter"),
    ("MAINTAINS_FARMLAND", "maintains_farmland"),
    ("BLOCKS_WIND_CHARGE_EXPLOSIONS", "blocks_wind_charge_explosions"),
    ("DOES_NOT_BLOCK_HOPPERS", "does_not_block_hoppers"),
    ("SUPPRESSES_BOUNCE", "suppresses_bounce"),
    ("TRIGGERS_AMBIENT_DESERT_SAND_BLOCK_SOUNDS", "triggers_ambient_desert_sand_block_sounds"),
    ("TRIGGERS_AMBIENT_DESERT_DRY_VEGETATION_BLOCK_SOUNDS", "triggers_ambient_desert_dry_vegetation_block_sounds"),
    ("TRIGGERS_AMBIENT_DRIED_GHAST_BLOCK_SOUNDS", "triggers_ambient_dried_ghast_block_sounds"),
    ("REQUIRED_FOR_POPLAR_LEAF_AMBIENCE", "required_for_poplar_leaf_ambience"),
    ("SPELEOTHEMS", "speleothems"), ("DANGEROUS_FOR_TELEPORTATION", "dangerous_for_teleportation"),
    ("CAT_DOES_NOT_TELEPORT_TO", "cat_does_not_teleport_to"),
    ("ENDERMAN_DOES_NOT_TELEPORT_TO", "enderman_does_not_teleport_to"),
    ("SHULKER_DOES_NOT_TELEPORT_TO", "shulker_does_not_teleport_to"),
    ("CONSUMABLE_DOES_NOT_TELEPORT_TO", "consumable_does_not_teleport_to"),
    ("FOX_IMMUNE_TO", "fox_immune_to"), ("POLAR_BEAR_IMMUNE_TO", "polar_bear_immune_to"),
    ("SNOW_GOLEM_IMMUNE_TO", "snow_golem_immune_to"), ("STRAY_IMMUNE_TO", "stray_immune_to"),
    ("WITHER_IMMUNE_TO", "wither_immune_to"),
    ("WITHER_SKELETON_IMMUNE_TO", "wither_skeleton_immune_to"),
    ("DEFAULT_IMMUNE_TO", "default_immune_to"), ("BLOCKS_MOTION", "blocks_motion"),
    ("BLOCKS_MOTION_NO_LEAVES", "blocks_motion_no_leaves"),
    ("ENTITIES_CAN_TELEPORT_TO", "entities_can_teleport_to"),
    ("BLOCKS_DOLPHIN_JUMP", "blocks_dolphin_jump"),
    ("ICE_MELTS_WHEN_DESTROYED_ABOVE", "ice_melts_when_destroyed_above"),
    ("CAUSES_SUFFOCATION", "causes_suffocation"),
    ("BLOCKS_MOTION_IN_HEIGHTMAP", "blocks_motion_in_heightmap"),
    ("BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES", "blocks_motion_in_heightmap_no_leaves"),
    ("BLOCKS_LAVA_FIRE_SPREAD", "blocks_lava_fire_spread"),
    ("BLOCKS_FLUID_FLOW", "blocks_fluid_flow"), ("WASHED_AWAY_BY_FLUIDS", "washed_away_by_fluids"),
    ("AIR", "air"),
];

/// `ItemTags`: (field, tag).
const ITEM: &[(&str, &str)] = &[
    ("WOOL", "wool"), ("PLANKS", "planks"), ("STONE_BRICKS", "stone_bricks"),
    ("WOODEN_BUTTONS", "wooden_buttons"), ("WOOL_CARPETS", "wool_carpets"),
    ("WOOL_STAIRS", "wool_stairs"), ("WOOL_SLABS", "wool_slabs"), ("WOODEN_DOORS", "wooden_doors"),
    ("WOODEN_STAIRS", "wooden_stairs"), ("WOODEN_SLABS", "wooden_slabs"),
    ("WOODEN_FENCES", "wooden_fences"), ("FENCE_GATES", "fence_gates"),
    ("WOODEN_PRESSURE_PLATES", "wooden_pressure_plates"), ("WOODEN_SHELVES", "wooden_shelves"),
    ("SAPLINGS", "saplings"), ("BAMBOO_BLOCKS", "bamboo_blocks"), ("OAK_LOGS", "oak_logs"),
    ("DARK_OAK_LOGS", "dark_oak_logs"), ("PALE_OAK_LOGS", "pale_oak_logs"),
    ("BIRCH_LOGS", "birch_logs"), ("ACACIA_LOGS", "acacia_logs"), ("SPRUCE_LOGS", "spruce_logs"),
    ("MANGROVE_LOGS", "mangrove_logs"), ("POPLAR_LOGS", "poplar_logs"),
    ("JUNGLE_LOGS", "jungle_logs"), ("CHERRY_LOGS", "cherry_logs"),
    ("CRIMSON_STEMS", "crimson_stems"), ("WARPED_STEMS", "warped_stems"),
    ("WART_BLOCKS", "wart_blocks"), ("LOGS_THAT_BURN", "logs_that_burn"), ("LOGS", "logs"),
    ("SAND", "sand"), ("SMELTS_TO_GLASS", "smelts_to_glass"), ("WALLS", "walls"),
    ("ANVIL", "anvil"), ("RAILS", "rails"), ("LEAVES", "leaves"),
    ("WOODEN_TRAPDOORS", "wooden_trapdoors"), ("BEDS", "beds"),
    ("SOUL_FIRE_BASE_BLOCKS", "soul_fire_base_blocks"), ("CANDLES", "candles"),
    ("DAMPENS_VIBRATIONS", "dampens_vibrations"), ("GOLD_ORES", "gold_ores"),
    ("IRON_ORES", "iron_ores"), ("DIAMOND_ORES", "diamond_ores"),
    ("REDSTONE_ORES", "redstone_ores"), ("LAPIS_ORES", "lapis_ores"), ("COAL_ORES", "coal_ores"),
    ("EMERALD_ORES", "emerald_ores"), ("COPPER_ORES", "copper_ores"), ("DIRT", "dirt"),
    ("MUD", "mud"), ("MOSS_BLOCKS", "moss_blocks"), ("TERRACOTTA", "terracotta"),
    ("GLAZED_TERRACOTTA", "glazed_terracotta"), ("CONCRETE", "concrete"),
    ("CONCRETE_STAIRS", "concrete_stairs"), ("CONCRETE_SLABS", "concrete_slabs"),
    ("CONCRETE_POWDERS", "concrete_powders"),
    ("COMPLETES_FIND_TREE_TUTORIAL", "completes_find_tree_tutorial"),
    ("SHULKER_BOXES", "shulker_boxes"), ("COPPER", "copper"), ("SKULLS", "skulls"),
    ("SIGNS", "signs"), ("HANGING_SIGNS", "hanging_signs"), ("BEE_FOOD", "bee_food"),
    ("SULFUR_CUBE_ARCHETYPE_BOUNCY", "sulfur_cube_archetype/bouncy"),
    ("SULFUR_CUBE_ARCHETYPE_SLOW_BOUNCY", "sulfur_cube_archetype/slow_bouncy"),
    ("SULFUR_CUBE_ARCHETYPE_REGULAR", "sulfur_cube_archetype/regular"),
    ("SULFUR_CUBE_ARCHETYPE_SLOW_FLAT", "sulfur_cube_archetype/slow_flat"),
    ("SULFUR_CUBE_ARCHETYPE_FAST_FLAT", "sulfur_cube_archetype/fast_flat"),
    ("SULFUR_CUBE_ARCHETYPE_LIGHT", "sulfur_cube_archetype/light"),
    ("SULFUR_CUBE_ARCHETYPE_FAST_SLIDING", "sulfur_cube_archetype/fast_sliding"),
    ("SULFUR_CUBE_ARCHETYPE_SLOW_SLIDING", "sulfur_cube_archetype/slow_sliding"),
    ("SULFUR_CUBE_ARCHETYPE_STICKY", "sulfur_cube_archetype/sticky"),
    ("SULFUR_CUBE_ARCHETYPE_HIGH_RESISTANCE", "sulfur_cube_archetype/high_resistance"),
    ("SULFUR_CUBE_ARCHETYPE_EXPLOSIVE", "sulfur_cube_archetype/explosive"),
    ("SULFUR_CUBE_ARCHETYPE_HOT", "sulfur_cube_archetype/hot"),
    ("SULFUR_CUBE_SWALLOWABLE", "sulfur_cube_swallowable"), ("BANNERS", "banners"),
    ("PIGLIN_REPELLENTS", "piglin_repellents"), ("PIGLIN_LOVED", "piglin_loved"),
    ("IGNORED_BY_PIGLIN_BABIES", "ignored_by_piglin_babies"),
    ("PIGLIN_SAFE_ARMOR", "piglin_safe_armor"), ("DUPLICATES_ALLAYS", "duplicates_allays"),
    ("EGGS", "eggs"), ("MEAT", "meat"), ("SNIFFER_FOOD", "sniffer_food"),
    ("PIGLIN_FOOD", "piglin_food"), ("FOX_FOOD", "fox_food"), ("COW_FOOD", "cow_food"),
    ("GOAT_FOOD", "goat_food"), ("SHEEP_FOOD", "sheep_food"), ("WOLF_FOOD", "wolf_food"),
    ("CAT_FOOD", "cat_food"), ("HORSE_FOOD", "horse_food"),
    ("ZOMBIE_HORSE_FOOD", "zombie_horse_food"), ("HORSE_TEMPT_ITEMS", "horse_tempt_items"),
    ("HARNESSES", "harnesses"), ("HAPPY_GHAST_FOOD", "happy_ghast_food"),
    ("HAPPY_GHAST_TEMPT_ITEMS", "happy_ghast_tempt_items"), ("CAMEL_FOOD", "camel_food"),
    ("CAMEL_HUSK_FOOD", "camel_husk_food"), ("ARMADILLO_FOOD", "armadillo_food"),
    ("CHICKEN_FOOD", "chicken_food"), ("FROG_FOOD", "frog_food"), ("HOGLIN_FOOD", "hoglin_food"),
    ("LLAMA_FOOD", "llama_food"), ("LLAMA_TEMPT_ITEMS", "llama_tempt_items"),
    ("OCELOT_FOOD", "ocelot_food"), ("PANDA_FOOD", "panda_food"),
    ("PANDA_EATS_FROM_GROUND", "panda_eats_from_ground"), ("PIG_FOOD", "pig_food"),
    ("RABBIT_FOOD", "rabbit_food"), ("STRIDER_FOOD", "strider_food"),
    ("STRIDER_TEMPT_ITEMS", "strider_tempt_items"), ("TURTLE_FOOD", "turtle_food"),
    ("PARROT_FOOD", "parrot_food"), ("PARROT_POISONOUS_FOOD", "parrot_poisonous_food"),
    ("AXOLOTL_FOOD", "axolotl_food"), ("NAUTILUS_BUCKET_FOOD", "nautilus_bucket_food"),
    ("NAUTILUS_FOOD", "nautilus_food"), ("NAUTILUS_TAMING_ITEMS", "nautilus_taming_items"),
    ("SULFUR_CUBE_FOOD", "sulfur_cube_food"), ("MUSHROOMS", "mushrooms"),
    ("NON_FLAMMABLE_WOOD", "non_flammable_wood"), ("BOATS", "boats"),
    ("CHEST_BOATS", "chest_boats"), ("FISHES", "fishes"),
    ("CREEPER_DROP_MUSIC_DISCS", "creeper_drop_music_discs"), ("COALS", "coals"),
    ("ARROWS", "arrows"), ("LECTERN_BOOKS", "lectern_books"),
    ("BOOKSHELF_BOOKS", "bookshelf_books"), ("BEACON_PAYMENT_ITEMS", "beacon_payment_items"),
    ("WOODEN_TOOL_MATERIALS", "wooden_tool_materials"),
    ("STONE_TOOL_MATERIALS", "stone_tool_materials"),
    ("COPPER_TOOL_MATERIALS", "copper_tool_materials"),
    ("IRON_TOOL_MATERIALS", "iron_tool_materials"), ("GOLD_TOOL_MATERIALS", "gold_tool_materials"),
    ("DIAMOND_TOOL_MATERIALS", "diamond_tool_materials"),
    ("NETHERITE_TOOL_MATERIALS", "netherite_tool_materials"),
    ("REPAIRS_LEATHER_ARMOR", "repairs_leather_armor"),
    ("REPAIRS_COPPER_ARMOR", "repairs_copper_armor"),
    ("REPAIRS_CHAIN_ARMOR", "repairs_chain_armor"), ("REPAIRS_IRON_ARMOR", "repairs_iron_armor"),
    ("REPAIRS_GOLD_ARMOR", "repairs_gold_armor"),
    ("REPAIRS_DIAMOND_ARMOR", "repairs_diamond_armor"),
    ("REPAIRS_NETHERITE_ARMOR", "repairs_netherite_armor"),
    ("REPAIRS_TURTLE_HELMET", "repairs_turtle_helmet"),
    ("REPAIRS_WOLF_ARMOR", "repairs_wolf_armor"),
    ("STONE_CRAFTING_MATERIALS", "stone_crafting_materials"),
    ("FREEZE_IMMUNE_WEARABLES", "freeze_immune_wearables"),
    ("CLUSTER_MAX_HARVESTABLES", "cluster_max_harvestables"), ("COMPASSES", "compasses"),
    ("CLONABLE_MAPS", "clonable_maps"), ("EXTENDABLE_MAPS", "extendable_maps"),
    ("CREEPER_IGNITERS", "creeper_igniters"),
    ("NOTE_BLOCK_TOP_INSTRUMENTS", "noteblock_top_instruments"), ("FOOT_ARMOR", "foot_armor"),
    ("LEG_ARMOR", "leg_armor"), ("CHEST_ARMOR", "chest_armor"), ("HEAD_ARMOR", "head_armor"),
    ("TRIMMABLE_ARMOR", "trimmable_armor"), ("TRIM_MATERIALS", "trim_materials"),
    ("DECORATED_POT_SHERDS", "decorated_pot_sherds"),
    ("DECORATED_POT_INGREDIENTS", "decorated_pot_ingredients"), ("SWORDS", "swords"),
    ("AXES", "axes"), ("HOES", "hoes"), ("PICKAXES", "pickaxes"), ("SHOVELS", "shovels"),
    ("SPEARS", "spears"), ("BREAKS_DECORATED_POTS", "breaks_decorated_pots"),
    ("VILLAGER_PLANTABLE_SEEDS", "villager_plantable_seeds"),
    ("VILLAGER_PICKS_UP", "villager_picks_up"), ("FURNACE_MINECART_FUEL", "furnace_minecart_fuel"),
    ("BUNDLES", "bundles"), ("BOOK_CLONING_TARGET", "book_cloning_target"), ("DYES", "dyes"),
    ("LOOM_DYES", "loom_dyes"), ("LOOM_PATTERNS", "loom_patterns"),
    ("CAULDRON_CAN_REMOVE_DYE", "cauldron_can_remove_dye"), ("CAT_COLLAR_DYES", "cat_collar_dyes"),
    ("WOLF_COLLAR_DYES", "wolf_collar_dyes"), ("CUSHIONS", "cushions"),
    ("SKELETON_PREFERRED_WEAPONS", "skeleton_preferred_weapons"),
    ("DROWNED_PREFERRED_WEAPONS", "drowned_preferred_weapons"),
    ("PIGLIN_PREFERRED_WEAPONS", "piglin_preferred_weapons"),
    ("PILLAGER_PREFERRED_WEAPONS", "pillager_preferred_weapons"),
    ("WITHER_SKELETON_DISLIKED_WEAPONS", "wither_skeleton_disliked_weapons"),
    ("SHEARABLE_FROM_COPPER_GOLEM", "shearable_from_copper_golem"),
    ("METAL_NUGGETS", "metal_nuggets"), ("DOUSES_CAMPFIRES", "douses_campfires"),
    ("BREWING_POTION_INPUTS", "brewing_potion_inputs"),
    ("FURNACE_FUEL_BOTTOM_TAKEABLE", "furnace_fuel_bottom_takeable"),
    ("FOOT_ARMOR_ENCHANTABLE", "enchantable/foot_armor"),
    ("LEG_ARMOR_ENCHANTABLE", "enchantable/leg_armor"),
    ("CHEST_ARMOR_ENCHANTABLE", "enchantable/chest_armor"),
    ("HEAD_ARMOR_ENCHANTABLE", "enchantable/head_armor"),
    ("ARMOR_ENCHANTABLE", "enchantable/armor"),
    ("MELEE_WEAPON_ENCHANTABLE", "enchantable/melee_weapon"),
    ("SWEEPING_ENCHANTABLE", "enchantable/sweeping"),
    ("FIRE_ASPECT_ENCHANTABLE", "enchantable/fire_aspect"),
    ("SHARP_WEAPON_ENCHANTABLE", "enchantable/sharp_weapon"),
    ("WEAPON_ENCHANTABLE", "enchantable/weapon"), ("MINING_ENCHANTABLE", "enchantable/mining"),
    ("MINING_LOOT_ENCHANTABLE", "enchantable/mining_loot"),
    ("FISHING_ENCHANTABLE", "enchantable/fishing"), ("TRIDENT_ENCHANTABLE", "enchantable/trident"),
    ("LUNGE_ENCHANTABLE", "enchantable/lunge"),
    ("DURABILITY_ENCHANTABLE", "enchantable/durability"), ("BOW_ENCHANTABLE", "enchantable/bow"),
    ("EQUIPPABLE_ENCHANTABLE", "enchantable/equippable"),
    ("CROSSBOW_ENCHANTABLE", "enchantable/crossbow"),
    ("VANISHING_ENCHANTABLE", "enchantable/vanishing"), ("MACE_ENCHANTABLE", "enchantable/mace"),
    ("MAP_INVISIBILITY_EQUIPMENT", "map_invisibility_equipment"),
    ("GAZE_DISGUISE_EQUIPMENT", "gaze_disguise_equipment"),
];

/// 1.21.1's vanilla tags that 26.3 renamed, split, narrowed or dropped: (registry, 1.21.1 tag, the
/// 26.3 tag a reference to it means, "" where it keeps its name, the 26.3 tags what a mod adds to
/// it goes to). Found by comparing the two versions' vanilla tag data, each tag resolved to the
/// blocks or items 1.21.1 has: a reference means the 26.3 tag holding the 1.21.1 set; an addition
/// goes where the 26.3 tags that take it in are the ones that took it in under 1.21.1, renamed.
/// 26.3 narrowed `dirt` to dirt, coarse dirt and rooted dirt and named the 1.21.1 set
/// `substrate_overworld`, which takes `dirt` in; its `supports_big_dripleaf` lists blocks where
/// 1.21.1's `big_dripleaf_placeable` took `dirt` in. `enchantable/sword` became `melee_weapon`
/// (knockback, looting; sharpness' primary items) and `sweeping` (sweeping edge). `tall_flowers`
/// is gone with nothing holding its set; its entries were flowers (and bee food).
const CHANGED: &[(&str, &str, &str, &[&str])] = &[
    ("block", "dirt", "substrate_overworld", &["dirt", "supports_big_dripleaf"]),
    ("block", "bamboo_plantable_on", "supports_bamboo", &["supports_bamboo"]),
    ("block", "big_dripleaf_placeable", "supports_big_dripleaf", &["supports_big_dripleaf"]),
    ("block", "small_dripleaf_placeable", "supports_small_dripleaf", &["supports_small_dripleaf"]),
    ("block", "concrete_powder", "concrete_powders", &["concrete_powders"]),
    ("block", "convertable_to_mud", "convertible_to_mud", &["convertible_to_mud"]),
    ("block", "mushroom_grow_block", "overrides_mushroom_light_requirement", &["overrides_mushroom_light_requirement"]),
    ("block", "snow_layer_can_survive_on", "support_override_snow_layer", &["support_override_snow_layer"]),
    ("block", "snow_layer_cannot_survive_on", "cannot_support_snow_layer", &["cannot_support_snow_layer"]),
    ("block", "dead_bush_may_place_on", "supports_dry_vegetation", &["supports_dry_vegetation"]),
    ("block", "tall_flowers", "", &["tall_flowers", "flowers"]),
    ("item", "enchantable/sword", "enchantable/melee_weapon", &["enchantable/melee_weapon", "enchantable/sweeping"]),
    ("item", "tall_flowers", "", &["tall_flowers", "bee_food", "flowers"]),
];

fn changed(registry: &str, tag: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static [&'static str])> {
    let registry = registry.trim_start_matches("minecraft:");
    let path = tag.strip_prefix("minecraft:").unwrap_or(tag);
    CHANGED.iter().find(|(r, old, _, _)| *r == registry && *old == path)
}

/// The vanilla tag 26.3 gives what 1.21.1 meant by `tag` (`minecraft:dirt`, or `dirt`) of
/// `registry` (`block`, `minecraft:item`), as `minecraft:substrate_overworld`; None where the name
/// means in 26.3 what it meant.
pub fn current(registry: &str, tag: &str) -> Option<String> {
    let (_, _, now, _) = changed(registry, tag)?;
    (!now.is_empty()).then(|| format!("minecraft:{now}"))
}

/// The vanilla tags of `registry` a mod's addition to 1.21.1's `minecraft:<path>` goes to in 26.3
/// (paths: `dirt` gives `dirt` and `supports_big_dripleaf`); None for a tag 26.3 kept as it was.
pub fn added_to(registry: &str, path: &str) -> Option<&'static [&'static str]> {
    changed(registry, path).map(|(_, _, _, to)| *to)
}

/// The tag `BlockTags.<field>` (`kind` `block`) or `ItemTags.<field>` (`item`) names, as
/// `minecraft:leaves`; None for a field 26.3 doesn't have.
pub fn named(kind: &str, field: &str) -> Option<String> {
    let find = |table: &[(&str, &str)]| table.iter().find(|(f, _)| *f == field).map(|(_, t)| format!("minecraft:{t}"));
    match kind {
        "block" => find(BLOCK),
        // A mod built for 1.21.1 names `ItemTags.STAIRS`; 26.3 keeps the tags blocks and items share
        // in `BlockItemTags`, one id for both halves, and its `ItemTags` no longer has them all.
        "item" => find(ITEM).or_else(|| find(BLOCK)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{added_to, named};
    use crate::{api, Rt, V};

    #[test]
    fn constants_name_the_tags_26_3_gives_them() {
        // Read from the 26.3 jar by hand: `BlockTags.<clinit>` and `BlockItemTags`.
        assert_eq!(named("block", "LEAVES").as_deref(), Some("minecraft:leaves"));
        assert_eq!(named("block", "MINEABLE_WITH_PICKAXE").as_deref(), Some("minecraft:mineable/pickaxe"));
        assert_eq!(named("block", "STANDING_SIGNS").as_deref(), Some("minecraft:standing_signs"));
        assert_eq!(named("block", "SIGNS").as_deref(), Some("minecraft:signs"));
        assert_eq!(named("block", "CEILING_HANGING_SIGNS").as_deref(), Some("minecraft:ceiling_hanging_signs"));
        assert_eq!(named("item", "SIGNS").as_deref(), Some("minecraft:signs"));
        assert_eq!(named("item", "HANGING_SIGNS").as_deref(), Some("minecraft:hanging_signs"));
        assert_eq!(named("item", "BEE_FOOD").as_deref(), Some("minecraft:bee_food"));
        assert_eq!(named("block", "BEE_ATTRACTIVE").as_deref(), Some("minecraft:bee_attractive"));
        assert_eq!(named("item", "PIGLIN_LOVED").as_deref(), Some("minecraft:piglin_loved"));
        // 1.21.1's `ItemTags.STAIRS` and `SLABS`: the item half of the shared id.
        assert_eq!(named("item", "STAIRS").as_deref(), Some("minecraft:stairs"));
        assert_eq!(named("item", "SLABS").as_deref(), Some("minecraft:slabs"));
        assert_eq!(named("block", "NOT_A_TAG"), None);
    }

    /// 26.3's block tags, as its data gives them: `dirt` is dirt, coarse and rooted dirt; the set
    /// 1.21.1 called `dirt` (grass, podzol, mycelium, moss, mud too) is `substrate_overworld`.
    struct Tags26;
    impl crate::host::Host for Tags26 {
        fn block_state(&self, _: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _: [i32; 3], _: &str) -> bool {
            false
        }
        fn block_in_tag(&self, block: &str, tag: &str) -> bool {
            let narrow = ["minecraft:dirt", "minecraft:coarse_dirt", "minecraft:rooted_dirt"];
            let more = ["minecraft:grass_block", "minecraft:podzol", "minecraft:mycelium", "minecraft:moss_block", "minecraft:mud"];
            match tag {
                "minecraft:dirt" => narrow.contains(&block),
                "minecraft:substrate_overworld" => narrow.contains(&block) || more.contains(&block),
                "minecraft:supports_bamboo" => block == "minecraft:sand" || narrow.contains(&block) || more.contains(&block),
                _ => false,
            }
        }
    }

    /// A 1.21.1 mod's vanilla tags hold in 26.3 the blocks they held: its `BlockTags.DIRT` takes
    /// grass in, as 1.21.1's did and 26.3's `dirt` doesn't; constants 26.3 dropped
    /// (`BAMBOO_PLANTABLE_ON`) and keys the mod makes itself name the tag 26.3 renamed them to.
    #[test]
    fn a_1_21_1_mods_vanilla_tags_hold_their_1_21_1_blocks() {
        let mut rt = Rt::new();
        let location = |rt: &mut Rt, key: V| rt.str_of(rt.get_field(key, "$location")).unwrap().to_owned();
        let dirt = api::get_static(&mut rt, "net/minecraft/tags/BlockTags.DIRT");
        let bamboo = api::get_static(&mut rt, "net/minecraft/tags/BlockTags.BAMBOO_PLANTABLE_ON");
        let leaves = api::get_static(&mut rt, "net/minecraft/tags/BlockTags.LEAVES");
        let sword = api::get_static(&mut rt, "net/minecraft/tags/ItemTags.SWORD_ENCHANTABLE");
        let item_dirt = api::get_static(&mut rt, "net/minecraft/tags/ItemTags.DIRT");
        assert_eq!(location(&mut rt, dirt), "minecraft:substrate_overworld");
        assert_eq!(location(&mut rt, bamboo), "minecraft:supports_bamboo");
        assert_eq!(location(&mut rt, sword), "minecraft:enchantable/melee_weapon");
        assert_eq!(location(&mut rt, leaves), "minecraft:leaves");
        // 26.3 has no item tag of 1.21.1's item `dirt` set: the name stays.
        assert_eq!(location(&mut rt, item_dirt), "minecraft:dirt");
        // `TagKey.create(Registries.BLOCK, "minecraft:dirt")`: the constant's key.
        let registry = api::get_static(&mut rt, "net/minecraft/core/registries/Registries.BLOCK");
        let name = rt.string("minecraft:dirt");
        let parse = "net/minecraft/resources/ResourceLocation.parse.(Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;";
        let name = api::call(&mut rt, parse, &[name]);
        let create = "net/minecraft/tags/TagKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;";
        let made = api::call(&mut rt, create, &[registry, name]);
        assert_eq!(location(&mut rt, made), "minecraft:substrate_overworld");
        // `state.is(BlockTags.DIRT)` on grass, coarse dirt and sand, against 26.3's tags.
        let is = "net/minecraft/world/level/block/state/BlockState.is.(Lnet/minecraft/tags/TagKey;)Z";
        let mut tags = Tags26;
        for (block, inside) in [("minecraft:grass_block", true), ("minecraft:coarse_dirt", true), ("minecraft:sand", false)] {
            let state = api::state(&mut rt, block);
            let got = rt.with_host(&mut tags, |rt| api::call(rt, is, &[state, dirt]));
            assert_eq!(got, V::I(inside as i32), "{block} in BlockTags.DIRT");
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a mod adds to a 1.21.1 tag 26.3 renamed goes where 26.3's tags take it in as 1.21.1's
    /// did: to the renamed tag; `dirt`'s stays in `dirt`, which `substrate_overworld` takes in,
    /// and joins `supports_big_dripleaf`, which lists its blocks where 1.21.1 took `dirt` in.
    #[test]
    fn additions_to_renamed_tags_go_where_26_3_takes_them_in() {
        assert_eq!(added_to("block", "dirt"), Some(&["dirt", "supports_big_dripleaf"][..]));
        assert_eq!(added_to("block", "bamboo_plantable_on"), Some(&["supports_bamboo"][..]));
        assert_eq!(added_to("minecraft:item", "enchantable/sword"), Some(&["enchantable/melee_weapon", "enchantable/sweeping"][..]));
        assert_eq!(added_to("block", "leaves"), None);
        assert_eq!(added_to("item", "dirt"), None);
    }
}
