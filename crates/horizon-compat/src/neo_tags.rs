//! NeoForge 1.21.1's `Tags` constants (`Tags.Blocks`, `Tags.Items`, `Tags.Fluids`,
//! `Tags.EntityTypes`) and the tags they name, read from its jar (`neoforge-21.1.219.jar`) with
//! `javap -c`: each field's `tag`/`neoforgeTag` string (`c:` and `neoforge:`), and for
//! `Tags.Items.DYES_<COLOR>`, `DyeColor.<COLOR>.getTag()`, which NeoForge names `c:dyes/<color>`.

use crate::{Rt, V};

/// `Tags.Blocks`: (field, tag).
const BLOCKS: &[(&str, &str)] = &[
    ("ENDERMAN_PLACE_ON_BLACKLIST", "neoforge:enderman_place_on_blacklist"),
    ("NEEDS_WOOD_TOOL", "neoforge:needs_wood_tool"), ("NEEDS_GOLD_TOOL", "neoforge:needs_gold_tool"),
    ("NEEDS_NETHERITE_TOOL", "neoforge:needs_netherite_tool"), ("BARRELS", "c:barrels"),
    ("BARRELS_WOODEN", "c:barrels/wooden"), ("BOOKSHELVES", "c:bookshelves"),
    ("BUDDING_BLOCKS", "c:budding_blocks"), ("BUDS", "c:buds"), ("CHAINS", "c:chains"),
    ("CHESTS", "c:chests"), ("CHESTS_ENDER", "c:chests/ender"), ("CHESTS_TRAPPED", "c:chests/trapped"),
    ("CHESTS_WOODEN", "c:chests/wooden"), ("CLUSTERS", "c:clusters"), ("COBBLESTONES", "c:cobblestones"),
    ("COBBLESTONES_NORMAL", "c:cobblestones/normal"), ("COBBLESTONES_INFESTED", "c:cobblestones/infested"),
    ("COBBLESTONES_MOSSY", "c:cobblestones/mossy"), ("COBBLESTONES_DEEPSLATE", "c:cobblestones/deepslate"),
    ("CONCRETES", "c:concretes"), ("DYED", "c:dyed"), ("DYED_BLACK", "c:dyed/black"),
    ("DYED_BLUE", "c:dyed/blue"), ("DYED_BROWN", "c:dyed/brown"), ("DYED_CYAN", "c:dyed/cyan"),
    ("DYED_GRAY", "c:dyed/gray"), ("DYED_GREEN", "c:dyed/green"), ("DYED_LIGHT_BLUE", "c:dyed/light_blue"),
    ("DYED_LIGHT_GRAY", "c:dyed/light_gray"), ("DYED_LIME", "c:dyed/lime"),
    ("DYED_MAGENTA", "c:dyed/magenta"), ("DYED_ORANGE", "c:dyed/orange"), ("DYED_PINK", "c:dyed/pink"),
    ("DYED_PURPLE", "c:dyed/purple"), ("DYED_RED", "c:dyed/red"), ("DYED_WHITE", "c:dyed/white"),
    ("DYED_YELLOW", "c:dyed/yellow"), ("END_STONES", "c:end_stones"), ("FENCE_GATES", "c:fence_gates"),
    ("FENCE_GATES_WOODEN", "c:fence_gates/wooden"), ("FENCES", "c:fences"),
    ("FENCES_NETHER_BRICK", "c:fences/nether_brick"), ("FENCES_WOODEN", "c:fences/wooden"),
    ("GLASS_BLOCKS", "c:glass_blocks"), ("GLASS_BLOCKS_COLORLESS", "c:glass_blocks/colorless"),
    ("GLASS_BLOCKS_CHEAP", "c:glass_blocks/cheap"), ("GLASS_BLOCKS_TINTED", "c:glass_blocks/tinted"),
    ("GLASS_PANES", "c:glass_panes"), ("GLASS_PANES_COLORLESS", "c:glass_panes/colorless"),
    ("GLAZED_TERRACOTTAS", "c:glazed_terracottas"), ("GRAVELS", "c:gravels"),
    ("HIDDEN_FROM_RECIPE_VIEWERS", "c:hidden_from_recipe_viewers"), ("NETHERRACKS", "c:netherracks"),
    ("OBSIDIANS", "c:obsidians"), ("OBSIDIANS_NORMAL", "c:obsidians/normal"),
    ("OBSIDIANS_CRYING", "c:obsidians/crying"),
    ("ORE_BEARING_GROUND_DEEPSLATE", "c:ore_bearing_ground/deepslate"),
    ("ORE_BEARING_GROUND_NETHERRACK", "c:ore_bearing_ground/netherrack"),
    ("ORE_BEARING_GROUND_STONE", "c:ore_bearing_ground/stone"), ("ORE_RATES_DENSE", "c:ore_rates/dense"),
    ("ORE_RATES_SINGULAR", "c:ore_rates/singular"), ("ORE_RATES_SPARSE", "c:ore_rates/sparse"),
    ("ORES", "c:ores"), ("ORES_COAL", "c:ores/coal"), ("ORES_COPPER", "c:ores/copper"),
    ("ORES_DIAMOND", "c:ores/diamond"), ("ORES_EMERALD", "c:ores/emerald"), ("ORES_GOLD", "c:ores/gold"),
    ("ORES_IRON", "c:ores/iron"), ("ORES_LAPIS", "c:ores/lapis"),
    ("ORES_NETHERITE_SCRAP", "c:ores/netherite_scrap"), ("ORES_QUARTZ", "c:ores/quartz"),
    ("ORES_REDSTONE", "c:ores/redstone"), ("ORES_IN_GROUND_DEEPSLATE", "c:ores_in_ground/deepslate"),
    ("ORES_IN_GROUND_NETHERRACK", "c:ores_in_ground/netherrack"),
    ("ORES_IN_GROUND_STONE", "c:ores_in_ground/stone"), ("PUMPKINS", "c:pumpkins"),
    ("PUMPKINS_NORMAL", "c:pumpkins/normal"), ("PUMPKINS_CARVED", "c:pumpkins/carved"),
    ("PUMPKINS_JACK_O_LANTERNS", "c:pumpkins/jack_o_lanterns"),
    ("PLAYER_WORKSTATIONS_CRAFTING_TABLES", "c:player_workstations/crafting_tables"),
    ("PLAYER_WORKSTATIONS_FURNACES", "c:player_workstations/furnaces"),
    ("RELOCATION_NOT_SUPPORTED", "c:relocation_not_supported"), ("ROPES", "c:ropes"), ("SANDS", "c:sands"),
    ("SANDS_COLORLESS", "c:sands/colorless"), ("SANDS_RED", "c:sands/red"),
    ("SANDSTONE_BLOCKS", "c:sandstone/blocks"), ("SANDSTONE_SLABS", "c:sandstone/slabs"),
    ("SANDSTONE_STAIRS", "c:sandstone/stairs"), ("SANDSTONE_RED_BLOCKS", "c:sandstone/red_blocks"),
    ("SANDSTONE_RED_SLABS", "c:sandstone/red_slabs"), ("SANDSTONE_RED_STAIRS", "c:sandstone/red_stairs"),
    ("SANDSTONE_UNCOLORED_BLOCKS", "c:sandstone/uncolored_blocks"),
    ("SANDSTONE_UNCOLORED_SLABS", "c:sandstone/uncolored_slabs"),
    ("SANDSTONE_UNCOLORED_STAIRS", "c:sandstone/uncolored_stairs"), ("SKULLS", "c:skulls"),
    ("STONES", "c:stones"), ("STORAGE_BLOCKS", "c:storage_blocks"),
    ("STORAGE_BLOCKS_BONE_MEAL", "c:storage_blocks/bone_meal"),
    ("STORAGE_BLOCKS_COAL", "c:storage_blocks/coal"), ("STORAGE_BLOCKS_COPPER", "c:storage_blocks/copper"),
    ("STORAGE_BLOCKS_DIAMOND", "c:storage_blocks/diamond"),
    ("STORAGE_BLOCKS_DRIED_KELP", "c:storage_blocks/dried_kelp"),
    ("STORAGE_BLOCKS_EMERALD", "c:storage_blocks/emerald"), ("STORAGE_BLOCKS_GOLD", "c:storage_blocks/gold"),
    ("STORAGE_BLOCKS_IRON", "c:storage_blocks/iron"), ("STORAGE_BLOCKS_LAPIS", "c:storage_blocks/lapis"),
    ("STORAGE_BLOCKS_NETHERITE", "c:storage_blocks/netherite"),
    ("STORAGE_BLOCKS_RAW_COPPER", "c:storage_blocks/raw_copper"),
    ("STORAGE_BLOCKS_RAW_GOLD", "c:storage_blocks/raw_gold"),
    ("STORAGE_BLOCKS_RAW_IRON", "c:storage_blocks/raw_iron"),
    ("STORAGE_BLOCKS_REDSTONE", "c:storage_blocks/redstone"),
    ("STORAGE_BLOCKS_SLIME", "c:storage_blocks/slime"), ("STORAGE_BLOCKS_WHEAT", "c:storage_blocks/wheat"),
    ("STRIPPED_LOGS", "c:stripped_logs"), ("STRIPPED_WOODS", "c:stripped_woods"),
    ("VILLAGER_JOB_SITES", "c:villager_job_sites"), ("VILLAGER_FARMLANDS", "neoforge:villager_farmlands"),
];

/// `Tags.Items`: (field, tag).
const ITEMS: &[(&str, &str)] = &[
    ("ENCHANTING_FUELS", "neoforge:enchanting_fuels"), ("BARRELS", "c:barrels"),
    ("BARRELS_WOODEN", "c:barrels/wooden"), ("BONES", "c:bones"), ("BOOKSHELVES", "c:bookshelves"),
    ("BRICKS", "c:bricks"), ("BRICKS_NORMAL", "c:bricks/normal"), ("BRICKS_NETHER", "c:bricks/nether"),
    ("BUCKETS", "c:buckets"), ("BUCKETS_EMPTY", "c:buckets/empty"), ("BUCKETS_WATER", "c:buckets/water"),
    ("BUCKETS_LAVA", "c:buckets/lava"), ("BUCKETS_MILK", "c:buckets/milk"),
    ("BUCKETS_POWDER_SNOW", "c:buckets/powder_snow"), ("BUCKETS_ENTITY_WATER", "c:buckets/entity_water"),
    ("BUDDING_BLOCKS", "c:budding_blocks"), ("BUDS", "c:buds"), ("CHAINS", "c:chains"),
    ("CHESTS", "c:chests"), ("CHESTS_ENDER", "c:chests/ender"), ("CHESTS_TRAPPED", "c:chests/trapped"),
    ("CHESTS_WOODEN", "c:chests/wooden"), ("COBBLESTONES", "c:cobblestones"),
    ("COBBLESTONES_NORMAL", "c:cobblestones/normal"), ("COBBLESTONES_INFESTED", "c:cobblestones/infested"),
    ("COBBLESTONES_MOSSY", "c:cobblestones/mossy"), ("COBBLESTONES_DEEPSLATE", "c:cobblestones/deepslate"),
    ("CONCRETES", "c:concretes"), ("CONCRETE_POWDERS", "c:concrete_powders"), ("CLUSTERS", "c:clusters"),
    ("CROPS", "c:crops"), ("CROPS_BEETROOT", "c:crops/beetroot"), ("CROPS_CACTUS", "c:crops/cactus"),
    ("CROPS_CARROT", "c:crops/carrot"), ("CROPS_COCOA_BEAN", "c:crops/cocoa_bean"),
    ("CROPS_MELON", "c:crops/melon"), ("CROPS_NETHER_WART", "c:crops/nether_wart"),
    ("CROPS_POTATO", "c:crops/potato"), ("CROPS_PUMPKIN", "c:crops/pumpkin"),
    ("CROPS_SUGAR_CANE", "c:crops/sugar_cane"), ("CROPS_WHEAT", "c:crops/wheat"), ("DRINKS", "c:drinks"),
    ("DRINKS_WATER", "c:drinks/water"), ("DRINKS_WATERY", "c:drinks/watery"),
    ("DRINKS_MILK", "c:drinks/milk"), ("DRINKS_HONEY", "c:drinks/honey"), ("DRINKS_MAGIC", "c:drinks/magic"),
    ("DRINKS_OMINOUS", "c:drinks/ominous"), ("DRINKS_JUICE", "c:drinks/juice"),
    ("DRINK_CONTAINING_BOTTLE", "c:drink_containing/bottle"),
    ("DRINK_CONTAINING_BUCKET", "c:drink_containing/bucket"), ("DUSTS", "c:dusts"),
    ("DUSTS_REDSTONE", "c:dusts/redstone"), ("DUSTS_GLOWSTONE", "c:dusts/glowstone"), ("DYED", "c:dyed"),
    ("DYED_BLACK", "c:dyed/black"), ("DYED_BLUE", "c:dyed/blue"), ("DYED_BROWN", "c:dyed/brown"),
    ("DYED_CYAN", "c:dyed/cyan"), ("DYED_GRAY", "c:dyed/gray"), ("DYED_GREEN", "c:dyed/green"),
    ("DYED_LIGHT_BLUE", "c:dyed/light_blue"), ("DYED_LIGHT_GRAY", "c:dyed/light_gray"),
    ("DYED_LIME", "c:dyed/lime"), ("DYED_MAGENTA", "c:dyed/magenta"), ("DYED_ORANGE", "c:dyed/orange"),
    ("DYED_PINK", "c:dyed/pink"), ("DYED_PURPLE", "c:dyed/purple"), ("DYED_RED", "c:dyed/red"),
    ("DYED_WHITE", "c:dyed/white"), ("DYED_YELLOW", "c:dyed/yellow"), ("DYES", "c:dyes"), ("EGGS", "c:eggs"),
    ("END_STONES", "c:end_stones"), ("ENDER_PEARLS", "c:ender_pearls"), ("FEATHERS", "c:feathers"),
    ("FENCE_GATES", "c:fence_gates"), ("FENCE_GATES_WOODEN", "c:fence_gates/wooden"), ("FENCES", "c:fences"),
    ("FENCES_NETHER_BRICK", "c:fences/nether_brick"), ("FENCES_WOODEN", "c:fences/wooden"),
    ("FERTILIZERS", "c:fertilizers"), ("FOODS", "c:foods"), ("FOODS_FRUIT", "c:foods/fruit"),
    ("FOODS_VEGETABLE", "c:foods/vegetable"), ("FOODS_BERRY", "c:foods/berry"),
    ("FOODS_BREAD", "c:foods/bread"), ("FOODS_COOKIE", "c:foods/cookie"), ("FOODS_DOUGH", "c:foods/dough"),
    ("FOODS_RAW_MEAT", "c:foods/raw_meat"), ("FOODS_COOKED_MEAT", "c:foods/cooked_meat"),
    ("FOODS_RAW_FISH", "c:foods/raw_fish"), ("FOODS_COOKED_FISH", "c:foods/cooked_fish"),
    ("FOODS_SOUP", "c:foods/soup"), ("FOODS_CANDY", "c:foods/candy"), ("FOODS_PIE", "c:foods/pie"),
    ("FOODS_GOLDEN", "c:foods/golden"), ("FOODS_EDIBLE_WHEN_PLACED", "c:foods/edible_when_placed"),
    ("FOODS_FOOD_POISONING", "c:foods/food_poisoning"), ("ANIMAL_FOODS", "c:animal_foods"),
    ("GEMS", "c:gems"), ("GEMS_DIAMOND", "c:gems/diamond"), ("GEMS_EMERALD", "c:gems/emerald"),
    ("GEMS_AMETHYST", "c:gems/amethyst"), ("GEMS_LAPIS", "c:gems/lapis"),
    ("GEMS_PRISMARINE", "c:gems/prismarine"), ("GEMS_QUARTZ", "c:gems/quartz"),
    ("GLASS_BLOCKS", "c:glass_blocks"), ("GLASS_BLOCKS_COLORLESS", "c:glass_blocks/colorless"),
    ("GLASS_BLOCKS_CHEAP", "c:glass_blocks/cheap"), ("GLASS_BLOCKS_TINTED", "c:glass_blocks/tinted"),
    ("GLASS_PANES", "c:glass_panes"), ("GLASS_PANES_COLORLESS", "c:glass_panes/colorless"),
    ("GLAZED_TERRACOTTAS", "c:glazed_terracottas"), ("GRAVELS", "c:gravels"), ("GUNPOWDERS", "c:gunpowders"),
    ("HIDDEN_FROM_RECIPE_VIEWERS", "c:hidden_from_recipe_viewers"), ("INGOTS", "c:ingots"),
    ("INGOTS_COPPER", "c:ingots/copper"), ("INGOTS_GOLD", "c:ingots/gold"), ("INGOTS_IRON", "c:ingots/iron"),
    ("INGOTS_NETHERITE", "c:ingots/netherite"), ("LEATHERS", "c:leathers"), ("MUSHROOMS", "c:mushrooms"),
    ("MUSIC_DISCS", "c:music_discs"), ("NETHER_STARS", "c:nether_stars"), ("NETHERRACKS", "c:netherracks"),
    ("NUGGETS", "c:nuggets"), ("NUGGETS_GOLD", "c:nuggets/gold"), ("NUGGETS_IRON", "c:nuggets/iron"),
    ("OBSIDIANS", "c:obsidians"), ("OBSIDIANS_NORMAL", "c:obsidians/normal"),
    ("OBSIDIANS_CRYING", "c:obsidians/crying"),
    ("ORE_BEARING_GROUND_DEEPSLATE", "c:ore_bearing_ground/deepslate"),
    ("ORE_BEARING_GROUND_NETHERRACK", "c:ore_bearing_ground/netherrack"),
    ("ORE_BEARING_GROUND_STONE", "c:ore_bearing_ground/stone"), ("ORE_RATES_DENSE", "c:ore_rates/dense"),
    ("ORE_RATES_SINGULAR", "c:ore_rates/singular"), ("ORE_RATES_SPARSE", "c:ore_rates/sparse"),
    ("ORES", "c:ores"), ("ORES_COAL", "c:ores/coal"), ("ORES_COPPER", "c:ores/copper"),
    ("ORES_DIAMOND", "c:ores/diamond"), ("ORES_EMERALD", "c:ores/emerald"), ("ORES_GOLD", "c:ores/gold"),
    ("ORES_IRON", "c:ores/iron"), ("ORES_LAPIS", "c:ores/lapis"),
    ("ORES_NETHERITE_SCRAP", "c:ores/netherite_scrap"), ("ORES_QUARTZ", "c:ores/quartz"),
    ("ORES_REDSTONE", "c:ores/redstone"), ("ORES_IN_GROUND_DEEPSLATE", "c:ores_in_ground/deepslate"),
    ("ORES_IN_GROUND_NETHERRACK", "c:ores_in_ground/netherrack"),
    ("ORES_IN_GROUND_STONE", "c:ores_in_ground/stone"),
    ("PLAYER_WORKSTATIONS_CRAFTING_TABLES", "c:player_workstations/crafting_tables"),
    ("PLAYER_WORKSTATIONS_FURNACES", "c:player_workstations/furnaces"), ("POTIONS", "c:potions"),
    ("POTION_BOTTLE", "c:potions/bottle"), ("PUMPKINS", "c:pumpkins"),
    ("PUMPKINS_NORMAL", "c:pumpkins/normal"), ("PUMPKINS_CARVED", "c:pumpkins/carved"),
    ("PUMPKINS_JACK_O_LANTERNS", "c:pumpkins/jack_o_lanterns"), ("RAW_MATERIALS", "c:raw_materials"),
    ("RAW_MATERIALS_COPPER", "c:raw_materials/copper"), ("RAW_MATERIALS_GOLD", "c:raw_materials/gold"),
    ("RAW_MATERIALS_IRON", "c:raw_materials/iron"), ("RODS", "c:rods"), ("RODS_BLAZE", "c:rods/blaze"),
    ("RODS_BREEZE", "c:rods/breeze"), ("RODS_WOODEN", "c:rods/wooden"), ("ROPES", "c:ropes"),
    ("SANDS", "c:sands"), ("SANDS_COLORLESS", "c:sands/colorless"), ("SANDS_RED", "c:sands/red"),
    ("SANDSTONE_BLOCKS", "c:sandstone/blocks"), ("SANDSTONE_SLABS", "c:sandstone/slabs"),
    ("SANDSTONE_STAIRS", "c:sandstone/stairs"), ("SANDSTONE_RED_BLOCKS", "c:sandstone/red_blocks"),
    ("SANDSTONE_RED_SLABS", "c:sandstone/red_slabs"), ("SANDSTONE_RED_STAIRS", "c:sandstone/red_stairs"),
    ("SANDSTONE_UNCOLORED_BLOCKS", "c:sandstone/uncolored_blocks"),
    ("SANDSTONE_UNCOLORED_SLABS", "c:sandstone/uncolored_slabs"),
    ("SANDSTONE_UNCOLORED_STAIRS", "c:sandstone/uncolored_stairs"), ("SEEDS", "c:seeds"),
    ("SEEDS_BEETROOT", "c:seeds/beetroot"), ("SEEDS_MELON", "c:seeds/melon"),
    ("SEEDS_PUMPKIN", "c:seeds/pumpkin"), ("SEEDS_TORCHFLOWER", "c:seeds/torchflower"),
    ("SEEDS_WHEAT", "c:seeds/wheat"), ("SHULKER_BOXES", "c:shulker_boxes"), ("SLIME_BALLS", "c:slime_balls"),
    ("SLIMEBALLS", "c:slimeballs"), ("STONES", "c:stones"), ("STORAGE_BLOCKS", "c:storage_blocks"),
    ("STORAGE_BLOCKS_BONE_MEAL", "c:storage_blocks/bone_meal"),
    ("STORAGE_BLOCKS_COAL", "c:storage_blocks/coal"), ("STORAGE_BLOCKS_COPPER", "c:storage_blocks/copper"),
    ("STORAGE_BLOCKS_DIAMOND", "c:storage_blocks/diamond"),
    ("STORAGE_BLOCKS_DRIED_KELP", "c:storage_blocks/dried_kelp"),
    ("STORAGE_BLOCKS_EMERALD", "c:storage_blocks/emerald"), ("STORAGE_BLOCKS_GOLD", "c:storage_blocks/gold"),
    ("STORAGE_BLOCKS_IRON", "c:storage_blocks/iron"), ("STORAGE_BLOCKS_LAPIS", "c:storage_blocks/lapis"),
    ("STORAGE_BLOCKS_NETHERITE", "c:storage_blocks/netherite"),
    ("STORAGE_BLOCKS_RAW_COPPER", "c:storage_blocks/raw_copper"),
    ("STORAGE_BLOCKS_RAW_GOLD", "c:storage_blocks/raw_gold"),
    ("STORAGE_BLOCKS_RAW_IRON", "c:storage_blocks/raw_iron"),
    ("STORAGE_BLOCKS_REDSTONE", "c:storage_blocks/redstone"),
    ("STORAGE_BLOCKS_SLIME", "c:storage_blocks/slime"), ("STORAGE_BLOCKS_WHEAT", "c:storage_blocks/wheat"),
    ("STRINGS", "c:strings"), ("STRIPPED_LOGS", "c:stripped_logs"), ("STRIPPED_WOODS", "c:stripped_woods"),
    ("VILLAGER_JOB_SITES", "c:villager_job_sites"), ("TOOLS", "c:tools"), ("TOOLS_SHIELD", "c:tools/shield"),
    ("TOOLS_BOW", "c:tools/bow"), ("TOOLS_CROSSBOW", "c:tools/crossbow"),
    ("TOOLS_FISHING_ROD", "c:tools/fishing_rod"), ("TOOLS_SPEAR", "c:tools/spear"),
    ("TOOLS_SHEAR", "c:tools/shear"), ("TOOLS_BRUSH", "c:tools/brush"), ("TOOLS_IGNITER", "c:tools/igniter"),
    ("TOOLS_MACE", "c:tools/mace"), ("TOOLS_WRENCH", "c:tools/wrench"),
    ("MELEE_WEAPON_TOOLS", "c:tools/melee_weapon"), ("RANGED_WEAPON_TOOLS", "c:tools/ranged_weapon"),
    ("MINING_TOOL_TOOLS", "c:tools/mining_tool"), ("ARMORS", "c:armors"), ("ENCHANTABLES", "c:enchantables"),
    ("DYES_BLACK", "c:dyes/black"), ("DYES_RED", "c:dyes/red"), ("DYES_GREEN", "c:dyes/green"),
    ("DYES_BROWN", "c:dyes/brown"), ("DYES_BLUE", "c:dyes/blue"), ("DYES_PURPLE", "c:dyes/purple"),
    ("DYES_CYAN", "c:dyes/cyan"), ("DYES_LIGHT_GRAY", "c:dyes/light_gray"), ("DYES_GRAY", "c:dyes/gray"),
    ("DYES_PINK", "c:dyes/pink"), ("DYES_LIME", "c:dyes/lime"), ("DYES_YELLOW", "c:dyes/yellow"),
    ("DYES_LIGHT_BLUE", "c:dyes/light_blue"), ("DYES_MAGENTA", "c:dyes/magenta"),
    ("DYES_ORANGE", "c:dyes/orange"), ("DYES_WHITE", "c:dyes/white"),
];

/// `Tags.Fluids`: (field, tag).
const FLUIDS: &[(&str, &str)] = &[
    ("WATER", "c:water"), ("LAVA", "c:lava"), ("MILK", "c:milk"), ("GASEOUS", "c:gaseous"),
    ("HONEY", "c:honey"), ("EXPERIENCE", "c:experience"), ("POTION", "c:potion"),
    ("SUSPICIOUS_STEW", "c:suspicious_stew"), ("MUSHROOM_STEW", "c:mushroom_stew"),
    ("RABBIT_STEW", "c:rabbit_stew"), ("BEETROOT_SOUP", "c:beetroot_soup"),
    ("HIDDEN_FROM_RECIPE_VIEWERS", "c:hidden_from_recipe_viewers"),
];

/// `Tags.EntityTypes`: (field, tag).
const ENTITY_TYPES: &[(&str, &str)] = &[
    ("BOSSES", "c:bosses"), ("MINECARTS", "c:minecarts"), ("BOATS", "c:boats"),
    ("CAPTURING_NOT_SUPPORTED", "c:capturing_not_supported"),
    ("TELEPORTING_NOT_SUPPORTED", "c:teleporting_not_supported"),
];

/// The tag NeoForge's `Tags` class `owner` names by `field`: its registry and the tag's id.
pub fn named(owner: &str, field: &str) -> Option<(&'static str, &'static str)> {
    let (registry, table) = match owner.strip_prefix("net/neoforged/neoforge/common/Tags$")? {
        "Blocks" => ("block", BLOCKS),
        "Items" => ("item", ITEMS),
        "Fluids" => ("fluid", FLUIDS),
        "EntityTypes" => ("entity_type", ENTITY_TYPES),
        _ => return None,
    };
    table.iter().find(|(f, _)| *f == field).map(|(_, t)| (registry, *t))
}

/// `Tags.Blocks.RELOCATION_NOT_SUPPORTED` and the rest: the key of the tag each names. Also the
/// other tag classes' constants (`BiomeTags.IS_END`, `EntityTypeTags.UNDEAD`, `Tags.Biomes.IS_VOID`,
/// `DamageTypeTags.IS_FIRE`), from [`crate::vanilla_tag_keys`].
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    let (registry, tag) = named(owner, field).or_else(|| crate::vanilla_tag_keys::named(owner, field))?;
    Some(crate::fluids::tag_key(rt, registry, tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_tag_constants_name_the_tags_its_jar_gives_them() {
        let b = "net/neoforged/neoforge/common/Tags$Blocks";
        let i = "net/neoforged/neoforge/common/Tags$Items";
        assert_eq!(named(b, "RELOCATION_NOT_SUPPORTED"), Some(("block", "c:relocation_not_supported")));
        assert_eq!(named(b, "ENDERMAN_PLACE_ON_BLACKLIST"), Some(("block", "neoforge:enderman_place_on_blacklist")));
        assert_eq!(named(i, "INGOTS_IRON"), Some(("item", "c:ingots/iron")));
        assert_eq!(named(i, "DYES_LIGHT_BLUE"), Some(("item", "c:dyes/light_blue")));
        assert_eq!(named("net/neoforged/neoforge/common/Tags$Fluids", "MILK"), Some(("fluid", "c:milk")));
        assert_eq!(named(b, "NOT_A_TAG"), None);
    }

    /// The other tag classes' constants: biome tags (`BiomeTags.IS_END`, one under
    /// `has_structure/`, NeoForge's `Tags.Biomes.IS_VOID` under `c:`), a damage type's and an
    /// entity type's, each key in its registry.
    #[test]
    fn other_tag_classes_name_their_registrys_tags() {
        let mut rt = Rt::new();
        let key = |rt: &mut Rt, o: &str, f: &str| {
            let k = get_static(rt, o, f).unwrap();
            let (r, l) = (rt.get_field(k, "$registry"), rt.get_field(k, "$location"));
            format!("{} {}", rt.str_of(r).unwrap(), rt.str_of(l).unwrap())
        };
        assert_eq!(key(&mut rt, "net/minecraft/tags/BiomeTags", "IS_END"), "biome minecraft:is_end");
        assert_eq!(key(&mut rt, "net/minecraft/tags/BiomeTags", "HAS_ANCIENT_CITY"), "biome minecraft:has_structure/ancient_city");
        assert_eq!(key(&mut rt, "net/neoforged/neoforge/common/Tags$Biomes", "IS_VOID"), "biome c:is_void");
        assert_eq!(key(&mut rt, "net/minecraft/tags/DamageTypeTags", "IS_FIRE"), "damage_type minecraft:is_fire");
        assert_eq!(key(&mut rt, "net/minecraft/tags/EntityTypeTags", "UNDEAD"), "entity_type minecraft:undead");
        assert!(get_static(&mut rt, "net/minecraft/tags/BiomeTags", "NOT_A_TAG").is_none());
    }
}
